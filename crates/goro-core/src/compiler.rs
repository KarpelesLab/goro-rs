use std::collections::HashMap;

use goro_parser::ast::*;

use crate::object::{ClassEntry, MethodDef, PropertyDef, Visibility as ObjVisibility};
use crate::opcode::{Op, OpArray, OpCode, OperandType, ParamType, ParamTypeInfo};
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
    /// Whether this is a switch statement (for "continue targeting switch" warning)
    is_switch: bool,
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
    /// Current namespace (e.g. b"Foo\\Bar"), empty for global namespace
    current_namespace: Vec<u8>,
    /// Use imports: maps lowercase short name (alias) -> fully qualified name (original case)
    use_map: HashMap<Vec<u8>, Vec<u8>>,
    /// Use imports for functions: maps lowercase short name -> fully qualified name
    use_function_map: HashMap<Vec<u8>, Vec<u8>>,
    /// Use imports for constants: maps short name -> fully qualified name (case-sensitive)
    use_const_map: HashMap<Vec<u8>, Vec<u8>>,
    /// Label offsets for goto: maps label name -> instruction offset
    label_offsets: HashMap<Vec<u8>, u32>,
    /// Pending gotos for forward references: maps label name -> list of jmp instruction offsets
    pending_gotos: HashMap<Vec<u8>, Vec<u32>>,
    /// Source file path (for __FILE__, __DIR__)
    pub source_file: Vec<u8>,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    /// Check if a builtin function's parameter at given position is by-reference
    fn is_builtin_byref_param(func_name: Option<&[u8]>, pos: usize) -> bool {
        match func_name {
            Some(name) => {
                let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
                match (lower.as_slice(), pos) {
                    // preg_match($pattern, $subject, &$matches, $flags, $offset)
                    (b"preg_match", 2) => true,
                    (b"preg_match_all", 2) => true,
                    // preg_replace($pattern, $replacement, $subject, $limit, &$count)
                    (b"preg_replace", 4) => true,
                    // preg_replace_callback($pattern, $callback, $subject, $limit, &$count)
                    (b"preg_replace_callback", 4) => true,
                    // preg_replace_callback_array($patterns, $subject, $limit, &$count)
                    (b"preg_replace_callback_array", 3) => true,
                    // sscanf with output vars (positions 2+)
                    (b"sscanf", p) if p >= 2 => true,
                    // str_replace($search, $replace, $subject, &$count)
                    (b"str_replace", 3) => true,
                    // str_ireplace($search, $replace, $subject, &$count)
                    (b"str_ireplace", 3) => true,
                    // similar_text($str1, $str2, &$percent)
                    (b"similar_text", 2) => true,
                    // settype(&$var, $type)
                    (b"settype", 0) => true,
                    // parse_str($string, &$result)
                    (b"parse_str", 1) => true,
                    _ => false,
                }
            }
            None => false,
        }
    }

    pub fn new() -> Self {
        Self {
            op_array: OpArray::new(),
            loop_stack: Vec::new(),
            compiled_classes: Vec::new(),
            current_class: None,
            current_parent_class: None,
            finally_targets: Vec::new(),
            current_namespace: Vec::new(),
            use_map: HashMap::new(),
            use_function_map: HashMap::new(),
            use_const_map: HashMap::new(),
            label_offsets: HashMap::new(),
            pending_gotos: HashMap::new(),
            source_file: Vec::new(),
        }
    }

    /// Compile a property name expression, treating bare identifiers as string literals
    /// (not as constant lookups). This is important for $obj->property syntax.
    fn compile_property_name(&mut self, expr: &Expr) -> CompileResult<OperandType> {
        if let ExprKind::Identifier(name) = &expr.kind {
            // Bare identifier in property context is always a string literal
            let idx = self.op_array.add_literal(Value::String(PhpString::from_vec(name.clone())));
            Ok(OperandType::Const(idx))
        } else {
            self.compile_expr(expr)
        }
    }

    /// Prefix a name with the current namespace. E.g. if namespace is "Foo\Bar" and name is "Baz",
    /// returns "Foo\Bar\Baz". If namespace is empty, returns name unchanged.
    fn prefix_with_namespace(&self, name: &[u8]) -> Vec<u8> {
        if self.current_namespace.is_empty() {
            name.to_vec()
        } else {
            let mut result = self.current_namespace.clone();
            result.push(b'\\');
            result.extend_from_slice(name);
            result
        }
    }

    /// Resolve a class/interface/trait name. Rules:
    /// 1. If the name starts with \, it's fully qualified -- strip the leading \
    /// 2. If the name contains a backslash (qualified), check first part against use aliases
    /// 3. If unqualified (no backslash):
    ///    - Check use imports first
    ///    - Otherwise prefix with current namespace
    /// Special names like "self", "parent", "static" are not resolved.
    /// Resolve a magic constant or known constant name in class constant context.
    fn resolve_class_const_magic(&self, name_lower: &[u8], class_name: &[u8], line: u32) -> Value {
        match name_lower {
            b"__method__" | b"__function__" => {
                // In class constant context (not a method), __METHOD__ and __FUNCTION__ are ""
                Value::String(PhpString::empty())
            }
            b"__class__" => {
                Value::String(PhpString::from_vec(class_name.to_vec()))
            }
            b"__line__" => {
                Value::Long(line as i64)
            }
            b"__file__" => {
                Value::String(PhpString::from_vec(self.source_file.clone()))
            }
            b"__dir__" => {
                let path = String::from_utf8_lossy(&self.source_file);
                let dir = if let Some(pos) = path.rfind('/') {
                    &path[..pos]
                } else {
                    "."
                };
                Value::String(PhpString::from_string(dir.to_string()))
            }
            b"__namespace__" => {
                Value::String(PhpString::from_vec(self.current_namespace.clone()))
            }
            b"__trait__" => {
                Value::String(PhpString::empty())
            }
            b"true" => Value::True,
            b"false" => Value::False,
            b"null" => Value::Null,
            b"php_eol" => Value::String(PhpString::from_bytes(b"\n")),
            b"php_int_max" => Value::Long(i64::MAX),
            b"php_int_min" => Value::Long(i64::MIN),
            b"php_int_size" => Value::Long(8),
            b"php_major_version" => Value::Long(8),
            b"php_minor_version" => Value::Long(5),
            _ => Value::Null,
        }
    }

    fn resolve_class_name(&self, name: &[u8]) -> Vec<u8> {
        // Special names are never resolved
        if name.eq_ignore_ascii_case(b"self")
            || name.eq_ignore_ascii_case(b"parent")
            || name.eq_ignore_ascii_case(b"static")
        {
            return name.to_vec();
        }

        // Fully qualified name (starts with \) -- strip leading \ and use as-is
        if name.starts_with(b"\\") {
            return name[1..].to_vec();
        }

        if let Some(pos) = name.iter().position(|&b| b == b'\\') {
            // Qualified name (contains backslash but doesn't start with one)
            // Check if the first part matches a use alias
            let first_part = &name[..pos];
            let first_part_lower: Vec<u8> = first_part.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(resolved) = self.use_map.get(&first_part_lower) {
                let mut result = resolved.clone();
                result.extend_from_slice(&name[pos..]);
                return result;
            }
            // Otherwise, prefix with namespace
            if self.current_namespace.is_empty() {
                name.to_vec()
            } else {
                self.prefix_with_namespace(name)
            }
        } else {
            // Unqualified name
            let name_lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(resolved) = self.use_map.get(&name_lower) {
                return resolved.clone();
            }
            // Prefix with namespace
            self.prefix_with_namespace(name)
        }
    }

    /// Resolve a function name. Rules similar to class names, but:
    /// - Uses use_function_map instead of use_map
    /// - Unqualified function calls fall back to global at RUNTIME (not compile time),
    ///   so we still prefix with namespace, and VM handles fallback
    fn resolve_function_name(&self, name: &[u8]) -> Vec<u8> {
        // Fully qualified name (starts with \) -- strip leading \ and use as-is
        if name.starts_with(b"\\") {
            return name[1..].to_vec();
        }

        if let Some(pos) = name.iter().position(|&b| b == b'\\') {
            // Qualified name
            let first_part = &name[..pos];
            let first_part_lower: Vec<u8> = first_part.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(resolved) = self.use_function_map.get(&first_part_lower) {
                let mut result = resolved.clone();
                result.extend_from_slice(&name[pos..]);
                return result;
            }
            // Also check regular use map for namespace aliases
            if let Some(resolved) = self.use_map.get(&first_part_lower) {
                let mut result = resolved.clone();
                result.extend_from_slice(&name[pos..]);
                return result;
            }
            if self.current_namespace.is_empty() {
                name.to_vec()
            } else {
                self.prefix_with_namespace(name)
            }
        } else {
            // Unqualified name - check function use map first
            let name_lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(resolved) = self.use_function_map.get(&name_lower) {
                return resolved.clone();
            }
            // Prefix with namespace (VM will fall back to global)
            self.prefix_with_namespace(name)
        }
    }

    /// Compile and emit SendVal/SendNamedVal/SendUnpack opcodes for function call arguments.
    fn compile_send_args(&mut self, args: &[Argument], line: u32) -> CompileResult<()> {
        self.compile_send_args_with_name(args, line, None)
    }

    fn compile_send_args_with_name(
        &mut self,
        args: &[Argument],
        line: u32,
        func_name: Option<&[u8]>,
    ) -> CompileResult<()> {
        let mut has_named = false;
        let mut has_unpack = false;
        for (i, arg) in args.iter().enumerate() {
            if arg.unpack {
                // Cannot use argument unpacking after named arguments
                if has_named {
                    return Err(CompileError {
                        message: "Cannot use argument unpacking after named arguments".into(),
                        line,
                    });
                }
                has_unpack = true;
                let val = self.compile_expr(&arg.value)?;
                self.op_array.emit(Op {
                    opcode: OpCode::SendUnpack,
                    op1: val,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line,
                });
            } else if let Some(name) = &arg.name {
                has_named = true;
                let val = self.compile_expr(&arg.value)?;
                let name_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(name.clone())));
                self.op_array.emit(Op {
                    opcode: OpCode::SendNamedVal,
                    op1: val,
                    op2: OperandType::Const(name_idx),
                    result: OperandType::Unused,
                    line,
                });
            } else {
                // Cannot use positional argument after named argument
                if has_named {
                    return Err(CompileError {
                        message: "Cannot use positional argument after named argument".into(),
                        line,
                    });
                }
                let val = self.compile_expr(&arg.value)?;
                // Check if this argument position is by-ref for known builtins
                let is_byref = Self::is_builtin_byref_param(func_name, i);
                let pos_idx = self.op_array.add_literal(Value::Long(i as i64));
                self.op_array.emit(Op {
                    opcode: if is_byref { OpCode::SendRef } else { OpCode::SendVal },
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
    /// Returns (level, is_non_integer). level=0 means invalid (0 literal), is_non_integer=true means variable/expr operand.
    fn extract_break_continue_level(level_expr: &Option<Expr>) -> (usize, bool) {
        match level_expr {
            Some(expr) => match &expr.kind {
                ExprKind::Int(n) => (*n as usize, false),
                _ => (0, true), // Non-integer operand (variable expression)
            },
            None => (1, false),
        }
    }

    /// Compile a complete program
    /// Compile a program, returning the op_array and compiled classes
    pub fn compile(mut self, program: &Program) -> CompileResult<(OpArray, Vec<ClassEntry>)> {
        // First pass: process namespace/use declarations and compile function/class declarations
        // Namespace/use must be processed in order so that class/function names are prefixed correctly
        self.compile_hoisting_pass(&program.statements)?;
        // Reset namespace state for second pass (will be re-applied by NamespaceDecl/UseDecl)
        self.current_namespace = Vec::new();
        self.use_map = HashMap::new();
        self.use_function_map = HashMap::new();
        self.use_const_map = HashMap::new();
        // Second pass: compile everything else
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::FunctionDecl { .. } => {
                    // Already compiled in first pass
                }
                StmtKind::ClassDecl { name, .. } if !name.starts_with(b"__anonymous_class_") => {
                    // Already compiled in first pass (skip non-anonymous classes)
                }
                StmtKind::NamespaceDecl { name, body } => {
                    // Set namespace state for second pass
                    if let Some(parts) = name {
                        let mut ns = Vec::new();
                        for (i, part) in parts.iter().enumerate() {
                            if i > 0 { ns.push(b'\\'); }
                            ns.extend_from_slice(part);
                        }
                        self.current_namespace = ns;
                    } else {
                        self.current_namespace = Vec::new();
                    }
                    self.use_map = HashMap::new();
                    self.use_function_map = HashMap::new();
                    self.use_const_map = HashMap::new();

                    if let Some(body_stmts) = body {
                        // Bracketed namespace: compile body statements (except func/class decls)
                        for s in body_stmts {
                            match &s.kind {
                                StmtKind::FunctionDecl { .. } => {
                                    // Already compiled in first pass
                                }
                                StmtKind::ClassDecl { name, .. } if !name.starts_with(b"__anonymous_class_") => {
                                    // Already compiled in first pass
                                }
                                _ => {
                                    self.compile_stmt(s)?;
                                }
                            }
                        }
                        // Reset after block
                        self.current_namespace = Vec::new();
                        self.use_map = HashMap::new();
                        self.use_function_map = HashMap::new();
                        self.use_const_map = HashMap::new();
                    }
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

    /// First pass: process only namespace/use/function/class declarations for hoisting.
    /// For bracketed namespaces, recurse into the body.
    fn compile_hoisting_pass(&mut self, stmts: &[Statement]) -> CompileResult<()> {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::NamespaceDecl { name, body } => {
                    // Set namespace state
                    if let Some(parts) = name {
                        let mut ns = Vec::new();
                        for (i, part) in parts.iter().enumerate() {
                            if i > 0 { ns.push(b'\\'); }
                            ns.extend_from_slice(part);
                        }
                        self.current_namespace = ns;
                    } else {
                        self.current_namespace = Vec::new();
                    }
                    self.use_map = HashMap::new();
                    self.use_function_map = HashMap::new();
                    self.use_const_map = HashMap::new();

                    if let Some(body_stmts) = body {
                        // Recurse into bracketed namespace body
                        self.compile_hoisting_pass(body_stmts)?;
                        // Reset after block
                        self.current_namespace = Vec::new();
                        self.use_map = HashMap::new();
                        self.use_function_map = HashMap::new();
                        self.use_const_map = HashMap::new();
                    }
                }
                StmtKind::UseDecl(_) => {
                    self.compile_stmt(stmt)?;
                }
                StmtKind::FunctionDecl { .. } => {
                    self.compile_stmt(stmt)?;
                }
                StmtKind::ClassDecl { name, .. } if !name.starts_with(b"__anonymous_class_") => {
                    self.compile_stmt(stmt)?;
                }
                _ => {}
            }
        }
        Ok(())
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
                    is_switch: false,
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
                    is_switch: false,
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
                    is_switch: false,
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
                by_ref,
                body,
                ..
            } => {
                // Check for $this in foreach target variables
                if self.check_foreach_this_assign(value, stmt.span.line)? {
                    return Err(CompileError {
                        message: "Cannot re-assign $this".into(),
                        line: stmt.span.line,
                    });
                }
                if let Some(k) = key {
                    if self.check_foreach_this_assign(k, stmt.span.line)? {
                        return Err(CompileError {
                            message: "Cannot re-assign $this".into(),
                            line: stmt.span.line,
                        });
                    }
                }
                let arr = self.compile_expr(expr)?;

                // Create iterator temp
                let iter_tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: if *by_ref { OpCode::ForeachInitRef } else { OpCode::ForeachInit },
                    op1: arr,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(iter_tmp),
                    line: stmt.span.line,
                });

                self.loop_stack.push(LoopContext {
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                    continue_target: None,
                    is_switch: false,
                });

                let loop_start = self.op_array.current_offset();

                // Fetch next value (or jump to end if done)
                let val_tmp = self.op_array.alloc_temp();
                let jmp_done = if *by_ref {
                    // For by-ref foreach, ForeachNextRef stores reference directly in val_tmp
                    let jmp = self.op_array.emit(Op {
                        opcode: OpCode::ForeachNextRef,
                        op1: OperandType::Tmp(iter_tmp),
                        op2: OperandType::JmpTarget(0), // patched later
                        result: OperandType::Tmp(val_tmp),
                        line: stmt.span.line,
                    });
                    jmp
                } else {
                    self.op_array.emit(Op {
                        opcode: OpCode::ForeachNext,
                        op1: OperandType::Tmp(iter_tmp),
                        op2: OperandType::JmpTarget(0), // patched later
                        result: OperandType::Tmp(val_tmp),
                        line: stmt.span.line,
                    })
                };

                // Assign value to the value variable
                if *by_ref {
                    // For by-ref: directly replace CV slot with the reference (don't write through existing ref)
                    if let ExprKind::Variable(name) = &value.kind {
                        let cv = self.op_array.get_or_create_cv(name);
                        self.op_array.emit(Op {
                            opcode: OpCode::AssignRef,
                            op1: OperandType::Cv(cv),
                            op2: OperandType::Tmp(val_tmp),
                            result: OperandType::Unused,
                            line: stmt.span.line,
                        });
                    }
                } else if let ExprKind::Variable(name) = &value.kind {
                    let cv = self.op_array.get_or_create_cv(name);
                    self.op_array.emit(Op {
                        opcode: OpCode::Assign,
                        op1: OperandType::Cv(cv),
                        op2: OperandType::Tmp(val_tmp),
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                } else {
                    // Handle list/array destructuring in foreach value:
                    // foreach ($arr as list($a, $b)) or foreach ($arr as [$a, $b])
                    let elems: Option<Vec<ArrayElement>> = match &value.kind {
                        ExprKind::Array(elems) => Some(elems.clone()),
                        ExprKind::FunctionCall { name, args }
                            if matches!(&name.kind, ExprKind::Identifier(n) if n.eq_ignore_ascii_case(b"list")) =>
                        {
                            Some(
                                args.iter()
                                    .map(|a| ArrayElement {
                                        key: None,
                                        value: a.value.clone(),
                                        unpack: false,
                                    })
                                    .collect(),
                            )
                        }
                        _ => None,
                    };
                    if let Some(elems) = elems {
                        self.compile_list_destructure(
                            &elems,
                            OperandType::Tmp(val_tmp),
                            stmt.span.line,
                        )?;
                    }
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
                    is_switch: true,
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
                        if default_index.is_some() {
                            // Report error at the duplicate default clause
                            let err_line = case.body.first()
                                .map(|s| s.span.line.saturating_sub(1))
                                .unwrap_or(stmt.span.line);
                            return Err(CompileError {
                                message: "Switch statements may only contain one default clause".into(),
                                line: err_line,
                            });
                        }
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
                let (level, is_non_integer) = Self::extract_break_continue_level(level_expr);
                if is_non_integer {
                    return Err(CompileError {
                        message: "'break' operator with non-integer operand is no longer supported".into(),
                        line: stmt.span.line,
                    });
                }
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
                let (level, is_non_integer) = Self::extract_break_continue_level(level_expr);
                if is_non_integer {
                    return Err(CompileError {
                        message: "'continue' operator with non-integer operand is no longer supported".into(),
                        line: stmt.span.line,
                    });
                }
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
                name, params, body, return_type, ..
            } => {
                // Check for promoted properties in free functions
                for param in params {
                    if param.visibility.is_some() || param.readonly {
                        return Err(CompileError {
                            message: "Cannot declare promoted property outside a constructor".to_string(),
                            line: stmt.span.line,
                        });
                    }
                }

                // Check if this function contains yield (making it a generator)
                let is_generator = stmts_contain_yield(body);

                // Prefix function name with namespace
                let qualified_name = self.prefix_with_namespace(name);

                // Compile the function body into a sub-OpArray
                let mut func_compiler = Compiler::new();
                func_compiler.current_namespace = self.current_namespace.clone();
                func_compiler.use_map = self.use_map.clone();
                func_compiler.use_function_map = self.use_function_map.clone();
                func_compiler.use_const_map = self.use_const_map.clone();
                func_compiler.op_array.name = qualified_name.clone();
                func_compiler.op_array.is_generator = is_generator;
                func_compiler.op_array.decl_line = stmt.span.line;
                func_compiler.source_file = self.source_file.clone();

                // Set return type
                if let Some(hint) = return_type {
                    // Check for self/parent outside of class scope
                    if self.current_class.is_none() {
                        if let TypeHint::Simple(name) = hint {
                            let name_lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if name_lower == b"self" {
                                return Err(CompileError {
                                    message: "Cannot use \"self\" when no class scope is active".into(),
                                    line: stmt.span.line,
                                });
                            }
                            if name_lower == b"parent" {
                                return Err(CompileError {
                                    message: "Cannot use \"parent\" when no class scope is active".into(),
                                    line: stmt.span.line,
                                });
                            }
                        }
                    }
                    func_compiler.op_array.return_type = Some(type_hint_to_param_type_with_ns(hint, &self.current_namespace, &self.use_map));
                }

                // Validate parameter types
                for param in params.iter() {
                    if let Some(hint) = &param.type_hint {
                        let simple_hint = match hint {
                            TypeHint::Simple(n) => Some(n),
                            TypeHint::Nullable(inner) => {
                                if let TypeHint::Simple(n) = inner.as_ref() { Some(n) } else { None }
                            }
                            _ => None,
                        };
                        if let Some(n) = simple_hint {
                            let lower: Vec<u8> = n.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if lower == b"void" {
                                return Err(CompileError {
                                    message: "void cannot be used as a parameter type".to_string(),
                                    line: stmt.span.line,
                                });
                            }
                            if lower == b"never" {
                                return Err(CompileError {
                                    message: "never cannot be used as a parameter type".to_string(),
                                    line: stmt.span.line,
                                });
                            }
                        }
                    }
                }

                // Set up parameter CVs and default values
                func_compiler.op_array.param_count = params.len() as u32;
                // Count required params (those without defaults and not variadic)
                func_compiler.op_array.required_param_count = params
                    .iter()
                    .filter(|p| p.default.is_none() && !p.variadic)
                    .count() as u32;
                for param in params {
                    let cv = func_compiler.op_array.get_or_create_cv(&param.name);
                    if param.variadic {
                        func_compiler.op_array.variadic_param = Some(cv);
                    }

                    // Store parameter type info
                    let type_info = param.type_hint.as_ref().map(|hint| {
                        let mut pt = type_hint_to_param_type_with_ns(hint, &self.current_namespace, &self.use_map);
                        // Implicitly nullable: if default is null and type is not already nullable/mixed
                        if let Some(default_expr) = &param.default {
                            if matches!(default_expr.kind, ExprKind::Null) && !is_type_nullable_or_mixed(&pt) {
                                pt = ParamType::Nullable(Box::new(pt));
                            }
                        }
                        ParamTypeInfo {
                            param_type: pt,
                            param_name: param.name.clone(),
                        }
                    });
                    // Ensure param_types vec is large enough
                    while func_compiler.op_array.param_types.len() <= cv as usize {
                        func_compiler.op_array.param_types.push(None);
                    }
                    func_compiler.op_array.param_types[cv as usize] = type_info;

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
                    .add_literal(Value::String(PhpString::from_vec(qualified_name)));
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

            StmtKind::Declare { directives, body } => {
                // Handle const declarations (parsed as Declare directives)
                for (name, value) in directives {
                    let name_lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    // Skip declare(strict_types=1) and similar
                    if name_lower == b"strict_types" || name_lower == b"encoding" || name_lower == b"ticks" {
                        continue;
                    }
                    // This is a const declaration: const FOO = value;
                    // Emit it as a define() call
                    // Build the fully-qualified constant name
                    let fqn = self.prefix_with_namespace(name);
                    let name_idx = self.op_array.add_literal(Value::String(PhpString::from_vec(fqn)));
                    let define_idx = self.op_array.add_literal(Value::String(PhpString::from_bytes(b"define")));
                    let arg_count_idx = self.op_array.add_literal(Value::Long(2));
                    self.op_array.emit(Op {
                        opcode: OpCode::InitFCall,
                        op1: OperandType::Const(define_idx),
                        op2: OperandType::Const(arg_count_idx),
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                    // Send const name as first arg
                    let pos0 = self.op_array.add_literal(Value::Long(0));
                    self.op_array.emit(Op {
                        opcode: OpCode::SendVal,
                        op1: OperandType::Const(name_idx),
                        op2: OperandType::Const(pos0),
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                    // Send value as second arg
                    let val_op = self.compile_expr(value)?;
                    let pos1 = self.op_array.add_literal(Value::Long(1));
                    self.op_array.emit(Op {
                        opcode: OpCode::SendVal,
                        op1: val_op,
                        op2: OperandType::Const(pos1),
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                    // Call define()
                    let tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::DoFCall,
                        op1: OperandType::Unused,
                        op2: OperandType::Unused,
                        result: OperandType::Tmp(tmp),
                        line: stmt.span.line,
                    });
                }
                // If body exists, compile it
                if let Some(body_stmts) = body {
                    for s in body_stmts {
                        self.compile_stmt(s)?;
                    }
                }
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
                            if index.is_none() {
                                return Err(CompileError {
                                    message: "Cannot use [] for unsetting".into(),
                                    line: stmt.span.line,
                                });
                            }
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
                            let prop_operand = self.compile_property_name(property)?;
                            self.op_array.emit(Op {
                                opcode: OpCode::PropertyUnset,
                                op1: obj_operand,
                                op2: prop_operand,
                                result: OperandType::Unused,
                                line: stmt.span.line,
                            });
                        }
                        _ => {
                            // unset($var) - directly set variable to Undef (breaks reference links)
                            let operand = self.compile_expr(expr)?;
                            self.op_array.emit(Op {
                                opcode: OpCode::UnsetCv,
                                op1: operand,
                                op2: OperandType::Unused,
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

                // If there's a finally block, push a second exception handler
                // so that re-thrown exceptions from catch-miss go through finally
                let finally_guard = if finally_body.is_some() {
                    Some(self.op_array.emit(Op {
                        opcode: OpCode::TryBegin,
                        op1: OperandType::JmpTarget(0), // no catch - patched to finally
                        op2: OperandType::JmpTarget(0), // finally target - patched below
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    }))
                } else {
                    None
                };

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
                            let raw_type_name: Vec<u8> = if type_parts.len() == 1 {
                                type_parts[0].clone()
                            } else {
                                type_parts.join(&b'\\')
                            };
                            let type_name = self.resolve_class_name(&raw_type_name);
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
                // End the finally guard before the rethrow
                if finally_guard.is_some() {
                    self.op_array.emit(Op {
                        opcode: OpCode::TryEnd,
                        op1: OperandType::Unused,
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                }

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

                    // Pop the finally target BEFORE compiling finally body
                    // so that returns inside finally don't loop back to finally
                    self.finally_targets.pop();

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

                    // Patch TryBegin's finally target
                    self.op_array.ops[try_begin as usize].op2 =
                        OperandType::JmpTarget(finally_start);

                    // Patch the finally guard to point to finally
                    if let Some(guard_pos) = finally_guard {
                        self.op_array.ops[guard_pos as usize].op1 =
                            OperandType::JmpTarget(finally_start); // catch goes to finally
                        self.op_array.ops[guard_pos as usize].op2 =
                            OperandType::JmpTarget(finally_start); // finally target
                    }

                    self.op_array.current_offset()
                } else {
                    let end = self.op_array.current_offset();
                    self.op_array.ops[try_begin as usize].op2 = OperandType::JmpTarget(end);
                    end
                };

                // Patch all jump-to-end targets
                // When there's a finally block, normal flow must go through it
                if has_finally {
                    // Find the finally_start from TryBegin's op2
                    if let OperandType::JmpTarget(fs) = self.op_array.ops[try_begin as usize].op2 {
                        self.op_array.patch_jump(jmp_after_try, fs);
                        for jmp in end_of_catch_jumps {
                            self.op_array.patch_jump(jmp, fs);
                        }
                    } else {
                        self.op_array.patch_jump(jmp_after_try, finally_or_end);
                        for jmp in end_of_catch_jumps {
                            self.op_array.patch_jump(jmp, finally_or_end);
                        }
                    }
                } else {
                    self.op_array.patch_jump(jmp_after_try, finally_or_end);
                    for jmp in end_of_catch_jumps {
                        self.op_array.patch_jump(jmp, finally_or_end);
                    }
                }
                Ok(())
            }

            StmtKind::ClassDecl {
                name,
                modifiers,
                extends,
                implements,
                body,
                enum_backing_type,
            } => {
                // Prefix class name with namespace
                let qualified_name = self.prefix_with_namespace(name);
                // Check for reserved class names
                let name_lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
                if name_lower == b"self" || name_lower == b"parent" || name_lower == b"static" {
                    let (article, kind) = if modifiers.is_interface {
                        ("an", "interface")
                    } else if modifiers.is_trait {
                        ("a", "trait")
                    } else {
                        ("a", "class")
                    };
                    return Err(CompileError {
                        message: format!("Cannot use \"{}\" as {} {} name as it is reserved",
                            String::from_utf8_lossy(name), article, kind),
                        line: stmt.span.line,
                    });
                }
                let mut class = ClassEntry::new(qualified_name.clone());
                // Resolve parent class name - also check for reserved names
                if let Some(p) = extends.as_ref() {
                    let p_lower: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                    if p_lower == b"self" || p_lower == b"parent" || p_lower == b"static" {
                        let kind_name = if modifiers.is_interface { "interface" } else { "class" };
                        return Err(CompileError {
                            message: format!("Cannot use \"{}\" as {} name, as it is reserved",
                                String::from_utf8_lossy(p), kind_name),
                            line: stmt.span.line,
                        });
                    }
                }
                class.parent = extends.as_ref().map(|p| self.resolve_class_name(p));
                // Resolve interface names - also check for reserved names
                for iface in implements.iter() {
                    let iface_lower: Vec<u8> = iface.iter().map(|b| b.to_ascii_lowercase()).collect();
                    if iface_lower == b"self" || iface_lower == b"parent" || iface_lower == b"static" {
                        return Err(CompileError {
                            message: format!("Cannot use \"{}\" as interface name, as it is reserved",
                                String::from_utf8_lossy(iface)),
                            line: stmt.span.line,
                        });
                    }
                }
                // Check for duplicate interface implementations
                {
                    let mut seen: Vec<Vec<u8>> = Vec::new();
                    for iface in implements.iter() {
                        let resolved = self.resolve_class_name(iface);
                        let lower: Vec<u8> = resolved.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if seen.contains(&lower) {
                            let kind = if modifiers.is_enum { "Enum" } else if modifiers.is_interface { "Interface" } else { "Class" };
                            let iface_display = String::from_utf8_lossy(iface);
                            return Err(CompileError {
                                message: format!("{} {} cannot implement previously implemented interface {}",
                                    kind, String::from_utf8_lossy(name), iface_display),
                                line: stmt.span.line,
                            });
                        }
                        seen.push(lower);
                    }
                }
                class.interfaces = implements.iter().map(|i| self.resolve_class_name(i)).collect();
                class.is_abstract = modifiers.is_abstract;
                class.is_final = modifiers.is_final;
                class.is_readonly = modifiers.is_readonly;
                class.is_interface = modifiers.is_interface;
                class.is_trait = modifiers.is_trait;
                class.is_enum = modifiers.is_enum;
                class.enum_backing_type = enum_backing_type.clone();

                // Enums automatically implement UnitEnum (and BackedEnum if backed)
                if modifiers.is_enum {
                    class.interfaces.push(b"UnitEnum".to_vec());
                    if enum_backing_type.is_some() {
                        class.interfaces.push(b"BackedEnum".to_vec());
                    }
                }

                // Validate enum backing type
                if let Some(bt) = &enum_backing_type {
                    if !bt.eq_ignore_ascii_case(b"int") && !bt.eq_ignore_ascii_case(b"string") {
                        return Err(CompileError {
                            message: format!("Enum backing type must be int or string, {} given",
                                String::from_utf8_lossy(bt)),
                            line: stmt.span.line,
                        });
                    }
                }

                for member in body {
                    match member {
                        ClassMember::Property {
                            name: prop_name,
                            type_hint,
                            default,
                            visibility,
                            is_static,
                            is_readonly,
                        } => {
                            // Enums cannot include properties
                            if modifiers.is_enum {
                                return Err(CompileError {
                                    message: format!("Enum {} cannot include properties",
                                        String::from_utf8_lossy(name)),
                                    line: stmt.span.line,
                                });
                            }
                            // Properties cannot have type callable
                            if let Some(hint) = type_hint {
                                let check_callable = |h: &TypeHint| -> bool {
                                    match h {
                                        TypeHint::Simple(n) => n.eq_ignore_ascii_case(b"callable"),
                                        TypeHint::Nullable(inner) => matches!(inner.as_ref(), TypeHint::Simple(n) if n.eq_ignore_ascii_case(b"callable")),
                                        TypeHint::Union(types) => types.iter().any(|t| matches!(t, TypeHint::Simple(n) if n.eq_ignore_ascii_case(b"callable"))),
                                        _ => false,
                                    }
                                };
                                if check_callable(hint) {
                                    return Err(CompileError {
                                        message: format!("Property {}::${} cannot have type callable",
                                            String::from_utf8_lossy(name), String::from_utf8_lossy(prop_name)),
                                        line: stmt.span.line,
                                    });
                                }
                                // Properties cannot have type void
                                let check_void = |h: &TypeHint| -> bool {
                                    match h {
                                        TypeHint::Simple(n) => n.eq_ignore_ascii_case(b"void"),
                                        _ => false,
                                    }
                                };
                                if check_void(hint) {
                                    return Err(CompileError {
                                        message: format!("Property {}::${} cannot have type void",
                                            String::from_utf8_lossy(name), String::from_utf8_lossy(prop_name)),
                                        line: stmt.span.line,
                                    });
                                }
                                // Properties cannot have type never
                                let check_never = |h: &TypeHint| -> bool {
                                    match h {
                                        TypeHint::Simple(n) => n.eq_ignore_ascii_case(b"never"),
                                        _ => false,
                                    }
                                };
                                if check_never(hint) {
                                    return Err(CompileError {
                                        message: format!("Property {}::${} cannot have type never",
                                            String::from_utf8_lossy(name), String::from_utf8_lossy(prop_name)),
                                        line: stmt.span.line,
                                    });
                                }
                            }
                            // Readonly property validations
                            let prop_is_readonly = *is_readonly || modifiers.is_readonly;
                            if prop_is_readonly {
                                // Readonly properties must have a type declaration
                                if type_hint.is_none() {
                                    return Err(CompileError {
                                        message: format!("Readonly property {}::${} must have type", String::from_utf8_lossy(name), String::from_utf8_lossy(prop_name)),
                                        line: stmt.span.line,
                                    });
                                }
                                // Readonly properties cannot be static
                                if *is_static {
                                    return Err(CompileError {
                                        message: format!("Static property {}::${} cannot be readonly", String::from_utf8_lossy(name), String::from_utf8_lossy(prop_name)),
                                        line: stmt.span.line,
                                    });
                                }
                                // Readonly properties cannot have a default value
                                if default.is_some() {
                                    return Err(CompileError {
                                        message: format!("Readonly property {}::${} cannot have default value", String::from_utf8_lossy(name), String::from_utf8_lossy(prop_name)),
                                        line: stmt.span.line,
                                    });
                                }
                            }
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
                                    ExprKind::Array(elements) => {
                                        let mut arr = crate::array::PhpArray::new();
                                        let mut all_const = true;
                                        for elem in elements {
                                            let val = Self::eval_const_expr(&elem.value);
                                            if let Some(v) = val {
                                                if let Some(key_expr) = &elem.key {
                                                    if let Some(k) = Self::eval_const_expr(key_expr) {
                                                        let key = match k {
                                                            Value::Long(n) => crate::array::ArrayKey::Int(n),
                                                            Value::String(s) => crate::array::ArrayKey::String(s),
                                                            _ => { all_const = false; break; }
                                                        };
                                                        arr.set(key, v);
                                                    } else {
                                                        all_const = false;
                                                        break;
                                                    }
                                                } else {
                                                    arr.push(v);
                                                }
                                            } else {
                                                all_const = false;
                                                break;
                                            }
                                        }
                                        if all_const {
                                            Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr)))
                                        } else {
                                            Value::Null
                                        }
                                    }
                                    ExprKind::UnaryOp { op: UnaryOp::Negate, operand, .. } => {
                                        match Self::eval_const_expr(operand) {
                                            Some(Value::Long(n)) => Value::Long(-n),
                                            Some(Value::Double(f)) => Value::Double(-f),
                                            _ => Value::Null,
                                        }
                                    }
                                    ExprKind::ClassConstAccess { class, constant } => {
                                        // Handle self::CONST, ClassName::CONST in property defaults
                                        let class_name = match &class.kind {
                                            ExprKind::Identifier(name) => {
                                                let resolved = self.resolve_class_name(name);
                                                if resolved.eq_ignore_ascii_case(b"self") {
                                                    qualified_name.clone()
                                                } else if resolved.eq_ignore_ascii_case(b"parent") {
                                                    extends.as_ref().map(|p| self.resolve_class_name(p)).unwrap_or(resolved)
                                                } else {
                                                    resolved
                                                }
                                            }
                                            _ => qualified_name.clone(),
                                        };
                                        let mut marker = b"__deferred_const__::".to_vec();
                                        marker.extend_from_slice(&class_name);
                                        marker.push(b':');
                                        marker.push(b':');
                                        marker.extend_from_slice(constant);
                                        Value::String(PhpString::from_vec(marker))
                                    }
                                    ExprKind::BinaryOp { op, left, right, .. } => {
                                        let l = Self::eval_const_expr(left);
                                        let r = Self::eval_const_expr(right);
                                        if let (Some(lv), Some(rv)) = (l, r) {
                                            match op {
                                                BinaryOp::Add => lv.add(&rv),
                                                BinaryOp::Sub => lv.sub(&rv),
                                                BinaryOp::Mul => lv.mul(&rv),
                                                BinaryOp::Concat => {
                                                    let mut result = lv.to_php_string().as_bytes().to_vec();
                                                    result.extend_from_slice(rv.to_php_string().as_bytes());
                                                    Value::String(PhpString::from_vec(result))
                                                }
                                                BinaryOp::BitwiseOr => Value::Long(lv.to_long() | rv.to_long()),
                                                BinaryOp::BitwiseAnd => Value::Long(lv.to_long() & rv.to_long()),
                                                _ => Value::Null,
                                            }
                                        } else {
                                            Value::Null
                                        }
                                    }
                                    ExprKind::Identifier(cname) => {
                                        let lower: Vec<u8> = cname.iter().map(|c| c.to_ascii_lowercase()).collect();
                                        self.resolve_class_const_magic(&lower, &qualified_name, expr.span.line)
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
                            let declaring_class_lower: Vec<u8> = qualified_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            let prop_type = type_hint.as_ref().map(|hint| {
                                type_hint_to_param_type_with_ns(hint, &self.current_namespace, &self.use_map)
                            });
                            // Check for duplicate property names
                            if class.properties.iter().any(|p| p.name == *prop_name) {
                                return Err(CompileError {
                                    message: format!("Cannot redeclare {}::${}",
                                        String::from_utf8_lossy(name), String::from_utf8_lossy(prop_name)),
                                    line: stmt.span.line,
                                });
                            }
                            class.properties.push(PropertyDef {
                                name: prop_name.clone(),
                                default: default_val,
                                is_static: *is_static,
                                visibility: vis,
                                declaring_class: declaring_class_lower,
                                is_readonly: prop_is_readonly,
                                property_type: prop_type,
                            });
                        }
                        ClassMember::Method {
                            name: method_name,
                            params,
                            return_type: method_return_type,
                            body: method_body,
                            visibility,
                            is_static,
                            is_abstract,
                            is_final: method_is_final,
                            line: method_line,
                        } => {
                            // Check: a method cannot be both abstract and final
                            if *is_abstract && *method_is_final {
                                return Err(CompileError {
                                    message: "Cannot use the final modifier on an abstract method".to_string(),
                                    line: *method_line,
                                });
                            }
                            // Check: abstract method cannot be private (except in traits, since PHP 8.0)
                            if *is_abstract && matches!(visibility, goro_parser::ast::Visibility::Private) && !modifiers.is_trait {
                                return Err(CompileError {
                                    message: format!("Abstract function {}::{}() cannot be declared private",
                                        String::from_utf8_lossy(name), String::from_utf8_lossy(method_name)),
                                    line: *method_line,
                                });
                            }
                            // Enums cannot have abstract methods
                            if modifiers.is_enum && *is_abstract {
                                return Err(CompileError {
                                    message: format!("Enum method {}::{}() must not be abstract",
                                        String::from_utf8_lossy(name), String::from_utf8_lossy(method_name)),
                                    line: *method_line,
                                });
                            }
                            // Enums cannot include certain magic methods
                            if modifiers.is_enum {
                                let mn_lower: Vec<u8> = method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                let forbidden_magic = matches!(mn_lower.as_slice(),
                                    b"__get" | b"__set" | b"__destruct" | b"__clone"
                                    | b"__sleep" | b"__wakeup" | b"__set_state"
                                    | b"__unserialize" | b"__serialize"
                                    | b"__isset" | b"__unset" | b"__debuginfo"
                                    | b"__construct" | b"__tostring"
                                );
                                if forbidden_magic {
                                    return Err(CompileError {
                                        message: format!("Enum {} cannot include magic method {}",
                                            String::from_utf8_lossy(name), String::from_utf8_lossy(method_name)),
                                        line: *method_line,
                                    });
                                }
                                // Backed enums cannot redeclare from/tryFrom, any enum cannot redeclare cases
                                let is_reserved_enum_method = if enum_backing_type.is_some() {
                                    matches!(mn_lower.as_slice(), b"from" | b"tryfrom" | b"cases")
                                } else {
                                    matches!(mn_lower.as_slice(), b"cases")
                                };
                                if is_reserved_enum_method {
                                    return Err(CompileError {
                                        message: format!("Cannot redeclare {}::{}()",
                                            String::from_utf8_lossy(name), String::from_utf8_lossy(method_name)),
                                        line: *method_line,
                                    });
                                }
                            }
                            // Enforce: __construct and __destruct cannot declare return types
                            {
                                let mn_lower: Vec<u8> = method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                if method_return_type.is_some() {
                                    if mn_lower == b"__construct" || mn_lower == b"__destruct" {
                                        return Err(CompileError {
                                            message: format!("Method {}::{}() cannot declare a return type", String::from_utf8_lossy(name), String::from_utf8_lossy(method_name)),
                                            line: *method_line,
                                        });
                                    }
                                    // Magic method return type restrictions
                                    if let Some(rt) = method_return_type {
                                        let class_display = String::from_utf8_lossy(name);
                                        // Helper: check if a type hint matches a simple type name
                                        let is_simple_type = |hint: &TypeHint, expected: &[u8]| -> bool {
                                            match hint {
                                                TypeHint::Simple(n) => n.eq_ignore_ascii_case(expected),
                                                _ => false,
                                            }
                                        };
                                        // Helper: check if type is ?array (nullable array)
                                        let is_nullable_array = |hint: &TypeHint| -> bool {
                                            match hint {
                                                TypeHint::Simple(n) => n.eq_ignore_ascii_case(b"array"),
                                                TypeHint::Nullable(inner) => matches!(inner.as_ref(), TypeHint::Simple(n) if n.eq_ignore_ascii_case(b"array")),
                                                TypeHint::Union(types) => {
                                                    // array|null or null|array
                                                    types.len() == 2 && types.iter().any(|t| is_simple_type(t, b"array")) && types.iter().any(|t| is_simple_type(t, b"null"))
                                                }
                                                _ => false,
                                            }
                                        };
                                        // __clone, __set, __unset, __unserialize, __wakeup: must be void
                                        if mn_lower == b"__clone" || mn_lower == b"__set" || mn_lower == b"__unset"
                                            || mn_lower == b"__unserialize" || mn_lower == b"__wakeup"
                                        {
                                            if !is_simple_type(rt, b"void") {
                                                return Err(CompileError {
                                                    message: format!("{}::{}(): Return type must be void when declared",
                                                        class_display, String::from_utf8_lossy(method_name)),
                                                    line: *method_line,
                                                });
                                            }
                                        }
                                        // __isset: must be bool
                                        if mn_lower == b"__isset" {
                                            if !is_simple_type(rt, b"bool") {
                                                return Err(CompileError {
                                                    message: format!("{}::__isset(): Return type must be bool when declared", class_display),
                                                    line: *method_line,
                                                });
                                            }
                                        }
                                        // __toString: must be string
                                        if mn_lower == b"__tostring" {
                                            if !is_simple_type(rt, b"string") {
                                                return Err(CompileError {
                                                    message: format!("{}::__toString(): Return type must be string when declared", class_display),
                                                    line: *method_line,
                                                });
                                            }
                                        }
                                        // __debugInfo: must be ?array (array or null|array or ?array)
                                        if mn_lower == b"__debuginfo" {
                                            if !is_nullable_array(rt) {
                                                return Err(CompileError {
                                                    message: format!("{}::__debugInfo(): Return type must be ?array when declared", class_display),
                                                    line: *method_line,
                                                });
                                            }
                                        }
                                        // __serialize, __sleep: must be array
                                        if mn_lower == b"__serialize" || mn_lower == b"__sleep" {
                                            if !is_simple_type(rt, b"array") {
                                                return Err(CompileError {
                                                    message: format!("{}::{}(): Return type must be array when declared",
                                                        class_display, String::from_utf8_lossy(method_name)),
                                                    line: *method_line,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                            // Validate magic method argument counts and static modifiers
                            {
                                let mn_lower: Vec<u8> = method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                let class_display = String::from_utf8_lossy(name);
                                let method_display = String::from_utf8_lossy(method_name);
                                // Cannot be static checks
                                if matches!(mn_lower.as_slice(), b"__construct" | b"__destruct" | b"__clone") {
                                    if *is_static {
                                        return Err(CompileError {
                                            message: format!("Method {}::{}() cannot be static",
                                                class_display, method_display),
                                            line: *method_line,
                                        });
                                    }
                                }
                                // __clone() cannot take arguments
                                if mn_lower == b"__clone" && !params.is_empty() {
                                    return Err(CompileError {
                                        message: format!("Method {}::__clone() cannot take arguments",
                                            class_display),
                                        line: *method_line,
                                    });
                                }
                                // __destruct() cannot take arguments
                                if mn_lower == b"__destruct" && !params.is_empty() {
                                    return Err(CompileError {
                                        message: format!("Method {}::__destruct() cannot take arguments",
                                            class_display),
                                        line: *method_line,
                                    });
                                }
                                // __isset() must take exactly 1 argument
                                if mn_lower == b"__isset" && params.len() != 1 {
                                    return Err(CompileError {
                                        message: format!("Method {}::__isset() must take exactly 1 argument",
                                            class_display),
                                        line: *method_line,
                                    });
                                }
                                // __unset() must take exactly 1 argument
                                if mn_lower == b"__unset" && params.len() != 1 {
                                    return Err(CompileError {
                                        message: format!("Method {}::__unset() must take exactly 1 argument",
                                            class_display),
                                        line: *method_line,
                                    });
                                }
                                // __call() must take exactly 2 arguments
                                if mn_lower == b"__call" && params.len() != 2 {
                                    return Err(CompileError {
                                        message: format!("Method {}::__call() must take exactly 2 arguments",
                                            class_display),
                                        line: *method_line,
                                    });
                                }
                                // __callStatic() must take exactly 2 arguments
                                if mn_lower == b"__callstatic" && params.len() != 2 {
                                    return Err(CompileError {
                                        message: format!("Method {}::__callStatic() must take exactly 2 arguments",
                                            class_display),
                                        line: *method_line,
                                    });
                                }
                                // __get() must take exactly 1 argument
                                if mn_lower == b"__get" && params.len() != 1 {
                                    return Err(CompileError {
                                        message: format!("Method {}::__get() must take exactly 1 argument",
                                            class_display),
                                        line: *method_line,
                                    });
                                }
                                // __set() must take exactly 2 arguments
                                if mn_lower == b"__set" && params.len() != 2 {
                                    return Err(CompileError {
                                        message: format!("Method {}::__set() must take exactly 2 arguments",
                                            class_display),
                                        line: *method_line,
                                    });
                                }
                            }
                            // Add promoted properties from constructor params
                            {
                                let mn_lower: Vec<u8> =
                                    method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                // Check for promoted properties in non-constructors
                                if mn_lower != b"__construct" {
                                    for param in params {
                                        if param.visibility.is_some() || param.readonly {
                                            return Err(CompileError {
                                                message: "Cannot declare promoted property outside a constructor".to_string(),
                                                line: stmt.span.line,
                                            });
                                        }
                                    }
                                }
                                // Check for promoted properties in abstract constructors
                                if mn_lower == b"__construct" && *is_abstract {
                                    for param in params {
                                        if param.visibility.is_some() || param.readonly {
                                            return Err(CompileError {
                                                message: "Cannot declare promoted property in an abstract constructor".to_string(),
                                                line: stmt.span.line,
                                            });
                                        }
                                    }
                                }
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
                                            let declaring_class_lower: Vec<u8> = qualified_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                            let promoted_prop_type = param.type_hint.as_ref().map(|hint| {
                                                type_hint_to_param_type_with_ns(hint, &self.current_namespace, &self.use_map)
                                            });
                                            // Check for duplicate property from promotion
                                            if class.properties.iter().any(|p| p.name == param.name) {
                                                return Err(CompileError {
                                                    message: format!("Cannot redeclare {}::${}",
                                                        String::from_utf8_lossy(name), String::from_utf8_lossy(&param.name)),
                                                    line: *method_line,
                                                });
                                            }
                                            class.properties.push(PropertyDef {
                                                name: param.name.clone(),
                                                default: Value::Null,
                                                is_static: false,
                                                visibility: prop_vis,
                                                declaring_class: declaring_class_lower,
                                                is_readonly: param.readonly || modifiers.is_readonly,
                                                property_type: promoted_prop_type,
                                            });
                                        }
                                    }
                                }
                            }

                            if let Some(body_stmts) = method_body {
                                // Check if method body contains yield
                                let method_is_generator = stmts_contain_yield(body_stmts);

                                let mut method_compiler = Compiler::new();
                                method_compiler.current_namespace = self.current_namespace.clone();
                                method_compiler.use_map = self.use_map.clone();
                                method_compiler.use_function_map = self.use_function_map.clone();
                                method_compiler.use_const_map = self.use_const_map.clone();
                                method_compiler.op_array.name = method_name.clone();
                                method_compiler.op_array.is_generator = method_is_generator;
                                method_compiler.op_array.decl_line = *method_line;
                                method_compiler.source_file = self.source_file.clone();
                                if let Some(hint) = method_return_type {
                                    method_compiler.op_array.return_type =
                                        Some(type_hint_to_param_type_with_ns(hint, &self.current_namespace, &self.use_map));
                                }
                                method_compiler.current_class = Some(qualified_name.clone());
                                method_compiler.current_parent_class = class.parent.clone();
                                method_compiler.op_array.scope_class = Some(qualified_name.iter().map(|b| b.to_ascii_lowercase()).collect());

                                // First CV is always $this (for non-static methods)
                                if !is_static {
                                    method_compiler.op_array.get_or_create_cv(b"this");
                                }

                                // Set up parameter CVs with default values
                                // Set param_count and required_param_count for the method
                                method_compiler.op_array.param_count = params.len() as u32
                                    + if *is_static { 0 } else { 1 }; // +1 for $this
                                method_compiler.op_array.required_param_count = params
                                    .iter()
                                    .filter(|p| p.default.is_none() && !p.variadic)
                                    .count() as u32;

                                for param in params {
                                    let cv = method_compiler.op_array.get_or_create_cv(&param.name);

                                    // Handle variadic parameter
                                    if param.variadic {
                                        method_compiler.op_array.variadic_param = Some(cv);
                                    }

                                    // Store parameter type info
                                    let type_info =
                                        param.type_hint.as_ref().map(|hint| {
                                            let mut pt = type_hint_to_param_type_with_ns(hint, &self.current_namespace, &self.use_map);
                                            // Implicitly nullable: if default is null and type is not already nullable/mixed
                                            if let Some(default_expr) = &param.default {
                                                if matches!(default_expr.kind, ExprKind::Null) && !is_type_nullable_or_mixed(&pt) {
                                                    pt = ParamType::Nullable(Box::new(pt));
                                                }
                                            }
                                            ParamTypeInfo {
                                                param_type: pt,
                                                param_name: param.name.clone(),
                                            }
                                        });
                                    while method_compiler.op_array.param_types.len() <= cv as usize
                                    {
                                        method_compiler.op_array.param_types.push(None);
                                    }
                                    method_compiler.op_array.param_types[cv as usize] = type_info;

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
                                // Check for duplicate method names
                                if class.methods.contains_key(&lower_name) {
                                    return Err(CompileError {
                                        message: format!("Cannot redeclare {}::{}()",
                                            String::from_utf8_lossy(name), String::from_utf8_lossy(method_name)),
                                        line: *method_line,
                                    });
                                }
                                let declaring_class_lower: Vec<u8> = qualified_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                class.methods.insert(
                                    lower_name,
                                    MethodDef {
                                        name: method_name.clone(),
                                        op_array: method_compiler.op_array,
                                        param_count,
                                        is_static: *is_static,
                                        is_abstract: *is_abstract,
                                        is_final: *method_is_final,
                                        visibility: vis,
                                        declaring_class: declaring_class_lower,
                                    },
                                );
                            } else {
                                // Non-abstract method without body in a concrete class
                                if !*is_abstract && !modifiers.is_abstract && !modifiers.is_interface {
                                    return Err(CompileError {
                                        message: format!("Non-abstract method {}::{}() must contain body",
                                            String::from_utf8_lossy(name), String::from_utf8_lossy(method_name)),
                                        line: *method_line,
                                    });
                                }
                                // Abstract method or interface method (no body)
                                let vis = match visibility {
                                    Visibility::Public => ObjVisibility::Public,
                                    Visibility::Protected => ObjVisibility::Protected,
                                    Visibility::Private => ObjVisibility::Private,
                                };
                                let param_count = params.len();
                                let lower_name: Vec<u8> =
                                    method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                let declaring_class_lower: Vec<u8> = qualified_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                class.methods.insert(
                                    lower_name,
                                    MethodDef {
                                        name: method_name.clone(),
                                        op_array: OpArray::new(),
                                        param_count,
                                        is_static: *is_static,
                                        is_abstract: true,
                                        is_final: *method_is_final,
                                        visibility: vis,
                                        declaring_class: declaring_class_lower,
                                    },
                                );
                            }
                        }
                        ClassMember::ClassConstant {
                            name: const_name,
                            value: const_expr,
                            ..
                        } => {
                            let val = if let Some(v) = Self::eval_const_expr(const_expr) {
                                v
                            } else {
                                // Handle class constant references like self::B, ClassName::CONST
                                match &const_expr.kind {
                                    ExprKind::ClassConstAccess { class: class_expr, constant } => {
                                        // First try to resolve from the same class's already-compiled constants
                                        if let Some(val) = Self::eval_class_const_expr(const_expr, &class, &qualified_name, extends.as_deref(), self) {
                                            val
                                        } else {
                                            // Store as a deferred constant reference marker
                                            // Format: __deferred_const__::ClassName::CONSTANT_NAME
                                            let class_name = match &class_expr.kind {
                                                ExprKind::Identifier(name) => {
                                                    let resolved = self.resolve_class_name(name);
                                                    if resolved.eq_ignore_ascii_case(b"self") {
                                                        qualified_name.clone()
                                                    } else if resolved.eq_ignore_ascii_case(b"parent") {
                                                        extends.as_ref().map(|p| self.resolve_class_name(p)).unwrap_or(resolved)
                                                    } else {
                                                        resolved
                                                    }
                                                }
                                                _ => qualified_name.clone(),
                                            };
                                            let mut marker = b"__deferred_const__::".to_vec();
                                            marker.extend_from_slice(&class_name);
                                            marker.push(b':');
                                            marker.push(b':');
                                            marker.extend_from_slice(constant);
                                            Value::String(PhpString::from_vec(marker))
                                        }
                                    }
                                    ExprKind::BinaryOp { op, left, right, .. } => {
                                        // Handle simple binary ops on constants
                                        // Try resolving with class context first (for self::CONST references)
                                        let l = Self::eval_const_expr(left).or_else(|| {
                                            Self::eval_class_const_expr(left, &class, &qualified_name, extends.as_deref(), self)
                                        });
                                        let r = Self::eval_const_expr(right).or_else(|| {
                                            Self::eval_class_const_expr(right, &class, &qualified_name, extends.as_deref(), self)
                                        });
                                        if let (Some(lv), Some(rv)) = (l, r) {
                                            match op {
                                                BinaryOp::Add => lv.add(&rv),
                                                BinaryOp::Sub => lv.sub(&rv),
                                                BinaryOp::Mul => lv.mul(&rv),
                                                BinaryOp::Concat => {
                                                    let mut result = lv.to_php_string().as_bytes().to_vec();
                                                    result.extend_from_slice(rv.to_php_string().as_bytes());
                                                    Value::String(PhpString::from_vec(result))
                                                }
                                                BinaryOp::BitwiseOr => Value::Long(lv.to_long() | rv.to_long()),
                                                BinaryOp::BitwiseAnd => Value::Long(lv.to_long() & rv.to_long()),
                                                BinaryOp::ShiftLeft => Value::Long(lv.to_long() << rv.to_long()),
                                                BinaryOp::ShiftRight => Value::Long(lv.to_long() >> rv.to_long()),
                                                _ => Value::Null,
                                            }
                                        } else {
                                            // Create a deferred marker for the entire expression
                                            Value::Null
                                        }
                                    }
                                    ExprKind::ConstantAccess(parts) => {
                                        // Handle magic constants and known constants in class constant context
                                        let name_lower: Vec<u8> = parts.iter().flat_map(|p| p.iter()).copied().collect();
                                        let name_lower_lc: Vec<u8> = name_lower.iter().map(|c| c.to_ascii_lowercase()).collect();
                                        self.resolve_class_const_magic(&name_lower_lc, &qualified_name, const_expr.span.line)
                                    }
                                    ExprKind::Identifier(name) => {
                                        let name_lower_lc: Vec<u8> = name.iter().map(|c| c.to_ascii_lowercase()).collect();
                                        self.resolve_class_const_magic(&name_lower_lc, &qualified_name, const_expr.span.line)
                                    }
                                    _ => Value::Null,
                                }
                            };
                            class.constants.insert(const_name.clone(), val);
                        }
                        ClassMember::EnumCase { name: case_name, value } => {
                            // 'case' can only be used in enums
                            if !modifiers.is_enum {
                                return Err(CompileError {
                                    message: format!("Case can only be used in enums"),
                                    line: stmt.span.line,
                                });
                            }
                            // Backed enum cases must have values, unit enum cases must not
                            if enum_backing_type.is_some() && value.is_none() {
                                return Err(CompileError {
                                    message: format!("Case {} of backed enum {} must have a value",
                                        String::from_utf8_lossy(case_name), String::from_utf8_lossy(name)),
                                    line: stmt.span.line,
                                });
                            }
                            if enum_backing_type.is_none() && value.is_some() {
                                return Err(CompileError {
                                    message: format!("Case {} of non-backed enum {} must not have a value",
                                        String::from_utf8_lossy(case_name), String::from_utf8_lossy(name)),
                                    line: stmt.span.line,
                                });
                            }
                            // Enum cases: store the backing value for backed enums
                            let backing_value = if let Some(val_expr) = value {
                                if let Some(v) = Self::eval_const_expr(&val_expr) {
                                    v
                                } else {
                                    // Handle class constant references and magic constants like class constant defaults
                                    match &val_expr.kind {
                                        ExprKind::ClassConstAccess { class: class_expr, constant } => {
                                            // Try to resolve from same class or other classes
                                            if let Some(val) = Self::eval_class_const_expr(&val_expr, &class, &qualified_name, extends.as_deref(), self) {
                                                val
                                            } else {
                                                let class_name = match &class_expr.kind {
                                                    ExprKind::Identifier(cn) => {
                                                        let resolved = self.resolve_class_name(cn);
                                                        if resolved.eq_ignore_ascii_case(b"self") {
                                                            qualified_name.clone()
                                                        } else if resolved.eq_ignore_ascii_case(b"parent") {
                                                            extends.as_ref().map(|p| self.resolve_class_name(p)).unwrap_or(resolved)
                                                        } else {
                                                            resolved
                                                        }
                                                    }
                                                    _ => qualified_name.clone(),
                                                };
                                                let mut marker = b"__deferred_const__::".to_vec();
                                                marker.extend_from_slice(&class_name);
                                                marker.push(b':');
                                                marker.push(b':');
                                                marker.extend_from_slice(constant);
                                                Value::String(PhpString::from_vec(marker))
                                            }
                                        }
                                        ExprKind::ConstantAccess(parts) => {
                                            let name_lower: Vec<u8> = parts.iter().flat_map(|p| p.iter()).copied().collect();
                                            let name_lower_lc: Vec<u8> = name_lower.iter().map(|c| c.to_ascii_lowercase()).collect();
                                            self.resolve_class_const_magic(&name_lower_lc, &qualified_name, val_expr.span.line)
                                        }
                                        ExprKind::Identifier(ident_name) => {
                                            let name_lower_lc: Vec<u8> = ident_name.iter().map(|c| c.to_ascii_lowercase()).collect();
                                            self.resolve_class_const_magic(&name_lower_lc, &qualified_name, val_expr.span.line)
                                        }
                                        ExprKind::BinaryOp { op, left, right, .. } => {
                                            let l = Self::eval_const_expr(left).or_else(|| {
                                                Self::eval_class_const_expr(left, &class, &qualified_name, extends.as_deref(), self)
                                            });
                                            let r = Self::eval_const_expr(right).or_else(|| {
                                                Self::eval_class_const_expr(right, &class, &qualified_name, extends.as_deref(), self)
                                            });
                                            if let (Some(lv), Some(rv)) = (l, r) {
                                                match op {
                                                    BinaryOp::Add => lv.add(&rv),
                                                    BinaryOp::Sub => lv.sub(&rv),
                                                    BinaryOp::Mul => lv.mul(&rv),
                                                    BinaryOp::Concat => {
                                                        let mut result = lv.to_php_string().as_bytes().to_vec();
                                                        result.extend_from_slice(rv.to_php_string().as_bytes());
                                                        Value::String(PhpString::from_vec(result))
                                                    }
                                                    BinaryOp::BitwiseOr => Value::Long(lv.to_long() | rv.to_long()),
                                                    BinaryOp::BitwiseAnd => Value::Long(lv.to_long() & rv.to_long()),
                                                    _ => Value::Null,
                                                }
                                            } else {
                                                Value::Null
                                            }
                                        }
                                        _ => Value::Null,
                                    }
                                }
                            } else {
                                Value::Null
                            };
                            // Check for duplicate backing values
                            if enum_backing_type.is_some() {
                                for (existing_name, existing_val) in &class.enum_cases {
                                    if existing_val.identical(&backing_value) {
                                        return Err(CompileError {
                                            message: format!("Duplicate value in enum {} for cases {} and {}",
                                                String::from_utf8_lossy(name),
                                                String::from_utf8_lossy(existing_name),
                                                String::from_utf8_lossy(case_name)),
                                            line: stmt.span.line,
                                        });
                                    }
                                }
                            }
                            // Validate backing type matches
                            if let Some(bt) = &enum_backing_type {
                                if bt.eq_ignore_ascii_case(b"int") && !matches!(backing_value, Value::Long(_) | Value::Null) {
                                    // Type mismatch: string value for int-backed enum
                                    // (Null means unit enum case, which is already handled above)
                                }
                                if bt.eq_ignore_ascii_case(b"string") && !matches!(backing_value, Value::String(_) | Value::Null) {
                                    // Type mismatch: int value for string-backed enum
                                }
                            }
                            // Store in enum_cases list for runtime enum object creation
                            class.enum_cases.push((case_name.clone(), backing_value.clone()));
                            // Store a special marker constant "__enum_case__::CaseName"
                            // The VM will detect this and create/return enum case objects
                            let marker = Value::String(crate::string::PhpString::from_vec(
                                [b"__enum_case__::" as &[u8], case_name.as_slice()].concat()
                            ));
                            class.constants.insert(case_name.clone(), marker);
                        }
                        ClassMember::TraitUse { traits, adaptations } => {
                            // Interfaces cannot use traits
                            if modifiers.is_interface {
                                let trait_list: Vec<String> = traits.iter().map(|t| String::from_utf8_lossy(t).to_string()).collect();
                                return Err(CompileError {
                                    message: format!("Cannot use traits inside of interfaces. {} is used in {}",
                                        trait_list.first().unwrap_or(&String::new()), String::from_utf8_lossy(name)),
                                    line: stmt.span.line,
                                });
                            }
                            for trait_name in traits {
                                class.traits.push(self.resolve_class_name(trait_name));
                            }
                            for adaptation in adaptations {
                                match adaptation {
                                    goro_parser::ast::TraitAdaptation::Alias {
                                        trait_name,
                                        method,
                                        new_name,
                                        new_visibility,
                                    } => {
                                        class.trait_adaptations.push(
                                            crate::object::TraitAdaptation::Alias {
                                                trait_name: trait_name.as_ref().map(|n| self.resolve_class_name(n)),
                                                method: method.clone(),
                                                new_name: new_name.clone(),
                                                new_visibility: new_visibility.as_ref().map(|v| match v {
                                                    goro_parser::ast::Visibility::Public => crate::object::Visibility::Public,
                                                    goro_parser::ast::Visibility::Protected => crate::object::Visibility::Protected,
                                                    goro_parser::ast::Visibility::Private => crate::object::Visibility::Private,
                                                }),
                                            },
                                        );
                                    }
                                    goro_parser::ast::TraitAdaptation::Precedence {
                                        trait_name,
                                        method,
                                        instead_of,
                                    } => {
                                        class.trait_adaptations.push(
                                            crate::object::TraitAdaptation::Precedence {
                                                trait_name: self.resolve_class_name(trait_name),
                                                method: method.clone(),
                                                instead_of: instead_of.iter().map(|n| self.resolve_class_name(n)).collect(),
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                // Store the class and emit a DeclareClass opcode
                let class_idx = self.compiled_classes.len();
                self.compiled_classes.push(class);

                let name_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(qualified_name)));
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

            StmtKind::NamespaceDecl { name, body } => {
                // Set current namespace
                if let Some(parts) = name {
                    // Join namespace parts with backslash
                    let mut ns = Vec::new();
                    for (i, part) in parts.iter().enumerate() {
                        if i > 0 {
                            ns.push(b'\\');
                        }
                        ns.extend_from_slice(part);
                    }
                    self.current_namespace = ns;
                } else {
                    self.current_namespace = Vec::new();
                }
                // Clear use maps when entering a new namespace
                self.use_map = HashMap::new();
                self.use_function_map = HashMap::new();
                self.use_const_map = HashMap::new();

                // If this is a block namespace { ... }, compile the body
                if let Some(body_stmts) = body {
                    for s in body_stmts {
                        self.compile_stmt(s)?;
                    }
                    // After block namespace, reset to global
                    self.current_namespace = Vec::new();
                    self.use_map = HashMap::new();
                    self.use_function_map = HashMap::new();
                    self.use_const_map = HashMap::new();
                }
                Ok(())
            }

            StmtKind::UseDecl(items) => {
                for item in items {
                    // Build the fully qualified name from parts
                    let mut fqn = Vec::new();
                    for (i, part) in item.name.iter().enumerate() {
                        if i > 0 {
                            fqn.push(b'\\');
                        }
                        fqn.extend_from_slice(part);
                    }
                    // Determine the alias (short name)
                    let alias = if let Some(ref a) = item.alias {
                        a.clone()
                    } else {
                        // Last part of the name
                        item.name.last().cloned().unwrap_or_default()
                    };
                    let alias_lower: Vec<u8> = alias.iter().map(|b| b.to_ascii_lowercase()).collect();
                    match item.kind {
                        UseKind::Normal => {
                            self.use_map.insert(alias_lower, fqn);
                        }
                        UseKind::Function => {
                            self.use_function_map.insert(alias_lower, fqn);
                        }
                        UseKind::Constant => {
                            self.use_const_map.insert(alias.clone(), fqn);
                        }
                    }
                }
                Ok(())
            }

            StmtKind::Label(name) => {
                // Check for duplicate labels
                if self.label_offsets.contains_key(name) {
                    let label_display = String::from_utf8_lossy(name);
                    return Err(CompileError {
                        message: format!("Label '{}' already defined", label_display),
                        line: stmt.span.line,
                    });
                }
                let offset = self.op_array.current_offset();
                self.label_offsets.insert(name.clone(), offset);
                // Patch any pending gotos that reference this label
                if let Some(gotos) = self.pending_gotos.remove(name) {
                    for goto_offset in gotos {
                        self.op_array.patch_jump(goto_offset, offset);
                    }
                }
                Ok(())
            }
            StmtKind::Goto(name) => {
                if let Some(&target) = self.label_offsets.get(name) {
                    // Label already seen - emit jump
                    self.op_array.emit(Op {
                        opcode: OpCode::Jmp,
                        op1: OperandType::JmpTarget(target),
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                } else {
                    // Label not yet seen - emit placeholder jump
                    let jmp = self.op_array.emit(Op {
                        opcode: OpCode::Jmp,
                        op1: OperandType::JmpTarget(0),
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                    self.pending_gotos
                        .entry(name.clone())
                        .or_insert_with(Vec::new)
                        .push(jmp);
                }
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

    /// Compile list/array destructuring assignment from a source array operand.
    /// Recursively handles nested list() and [] patterns.
    fn compile_list_destructure(
        &mut self,
        elems: &[ArrayElement],
        arr_op: OperandType,
        line: u32,
    ) -> CompileResult<()> {
        for (i, elem) in elems.iter().enumerate() {
            let idx_op = if let Some(key_expr) = &elem.key {
                self.compile_expr(key_expr)?
            } else {
                let idx_const = self.op_array.add_literal(Value::Long(i as i64));
                OperandType::Const(idx_const)
            };

            if let ExprKind::Variable(name) = &elem.value.kind {
                let cv = self.op_array.get_or_create_cv(name);
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::ListGet,
                    op1: arr_op,
                    op2: idx_op,
                    result: OperandType::Tmp(tmp),
                    line,
                });
                self.op_array.emit(Op {
                    opcode: OpCode::Assign,
                    op1: OperandType::Cv(cv),
                    op2: OperandType::Tmp(tmp),
                    result: OperandType::Unused,
                    line,
                });
            } else {
                // Check for nested destructuring
                let nested_elems = match &elem.value.kind {
                    ExprKind::Array(elems) => Some(elems.clone()),
                    ExprKind::FunctionCall { name, args }
                        if matches!(&name.kind, ExprKind::Identifier(n) if n.eq_ignore_ascii_case(b"list")) =>
                    {
                        Some(
                            args.iter()
                                .map(|a| ArrayElement {
                                    key: None,
                                    value: a.value.clone(),
                                    unpack: false,
                                })
                                .collect(),
                        )
                    }
                    ExprKind::Null => None, // Skip empty slots
                    _ => None,
                };
                if let Some(nested) = nested_elems {
                    let sub_arr = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::ListGet,
                        op1: arr_op,
                        op2: idx_op,
                        result: OperandType::Tmp(sub_arr),
                        line,
                    });
                    self.compile_list_destructure(&nested, OperandType::Tmp(sub_arr), line)?;
                }
            }
        }
        Ok(())
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
                // Check for nullsafe in write context
                if matches!(&target.kind, ExprKind::PropertyAccess { nullsafe: true, .. }) {
                    return Err(CompileError {
                        message: "Can't use nullsafe operator in write context".into(),
                        line: target.span.line,
                    });
                }
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
                        let prop_operand = match &property.kind {
                            ExprKind::Identifier(name) => {
                                let name_idx = self
                                    .op_array
                                    .add_literal(Value::String(PhpString::from_vec(name.clone())));
                                OperandType::Const(name_idx)
                            }
                            _ => self.compile_expr(property)?,
                        };
                        self.op_array.emit(Op {
                            opcode: OpCode::PropertySet,
                            op1: obj,
                            op2: val,
                            result: prop_operand,
                            line: expr.span.line,
                        });
                        Ok(val)
                    }
                    ExprKind::StaticPropertyAccess { class, property } => {
                        let class_name = match &class.kind {
                            ExprKind::Identifier(name) => {
                                let resolved = self.resolve_class_name(name);
                                if resolved.eq_ignore_ascii_case(b"self") {
                                    self.current_class.clone().unwrap_or(resolved)
                                } else if resolved.eq_ignore_ascii_case(b"static") {
                                    // Late static binding: resolve at runtime
                                    b"static".to_vec()
                                } else if resolved.eq_ignore_ascii_case(b"parent") {
                                    self.current_parent_class.clone().unwrap_or(resolved)
                                } else {
                                    resolved
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
                        // Also supports keyed: ["a" => $x, "b" => $y] = $arr
                        let elems: Vec<_> = match &target.kind {
                            ExprKind::Array(elems) => elems.clone(),
                            ExprKind::FunctionCall { name, args }
                                if matches!(&name.kind, ExprKind::Identifier(n) if n.eq_ignore_ascii_case(b"list")) =>
                            {
                                args.iter()
                                    .map(|a| ArrayElement {
                                        key: None,
                                        value: a.value.clone(),
                                        unpack: false,
                                    })
                                    .collect()
                            }
                            _ => {
                                let msg = match &target.kind {
                                    ExprKind::FunctionCall { .. } => "Can't use function return value in write context",
                                    ExprKind::MethodCall { .. } => "Can't use method return value in write context",
                                    _ => "Can't use function return value in write context",
                                };
                                return Err(CompileError {
                                    message: msg.into(),
                                    line: expr.span.line,
                                });
                            }
                        };

                        let arr_op = val;
                        self.compile_list_destructure(&elems, arr_op, expr.span.line)?;
                        Ok(arr_op)
                    }
                }
            }

            ExprKind::CompoundAssign { op, target, value } => {
                // Check for nullsafe in write context
                if matches!(&target.kind, ExprKind::PropertyAccess { nullsafe: true, .. }) {
                    return Err(CompileError {
                        message: "Can't use nullsafe operator in write context".into(),
                        line: target.span.line,
                    });
                }
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
                            // $arr[] op= val: append with compound op
                            // For a new element, starting value is null/empty
                            // So $arr[] .= "test" => $arr[] = "" . "test" = "test"
                            // Just compile as a simple array append
                            self.op_array.emit(Op {
                                opcode: OpCode::ArrayAppend,
                                op1: arr,
                                op2: val,
                                result: OperandType::Unused,
                                line: expr.span.line,
                            });
                            return Ok(val);
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
                    BinaryOp::LogicalXor => OpCode::BoolXor,
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
                // Unary plus just coerces to number - return the operand directly
                if matches!(op, UnaryOp::Plus) {
                    let val = self.compile_expr(operand)?;
                    // PHP unary + coerces to number but preserves negative zero
                    // Use a dedicated UnaryPlus opcode
                    let tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::UnaryPlus,
                        op1: val,
                        op2: OperandType::Unused,
                        result: OperandType::Tmp(tmp),
                        line: expr.span.line,
                    });
                    return Ok(OperandType::Tmp(tmp));
                }

                // Handle increment/decrement on property access: $obj->prop++ etc.
                if matches!(op, UnaryOp::PostIncrement | UnaryOp::PostDecrement | UnaryOp::PreIncrement | UnaryOp::PreDecrement)
                {
                    if matches!(&operand.kind, ExprKind::PropertyAccess { nullsafe: true, .. }) {
                        return Err(CompileError {
                            message: "Can't use nullsafe operator in write context".into(),
                            line: operand.span.line,
                        });
                    }
                    if let ExprKind::PropertyAccess { object, property, .. } = &operand.kind {
                        let obj_op = self.compile_expr(object)?;
                        let prop_name = match &property.kind {
                            ExprKind::Identifier(name) => name.clone(),
                            _ => vec![],
                        };
                        if !prop_name.is_empty() {
                            let name_idx = self.op_array.add_literal(Value::String(PhpString::from_vec(prop_name)));
                            // Fetch current value
                            let old_tmp = self.op_array.alloc_temp();
                            self.op_array.emit(Op {
                                opcode: OpCode::PropertyGet,
                                op1: obj_op,
                                op2: OperandType::Const(name_idx),
                                result: OperandType::Tmp(old_tmp),
                                line: expr.span.line,
                            });
                            // Increment/decrement
                            let new_tmp = self.op_array.alloc_temp();
                            let inc_opcode = match op {
                                UnaryOp::PostIncrement | UnaryOp::PreIncrement => OpCode::PostIncrement,
                                _ => OpCode::PostDecrement,
                            };
                            self.op_array.emit(Op {
                                opcode: inc_opcode,
                                op1: OperandType::Tmp(old_tmp),
                                op2: OperandType::Unused,
                                result: OperandType::Tmp(new_tmp),
                                line: expr.span.line,
                            });
                            // Write back to property (old_tmp now contains new value after PostInc/Dec modified it)
                            self.op_array.emit(Op {
                                opcode: OpCode::PropertySet,
                                op1: obj_op,
                                op2: OperandType::Tmp(old_tmp),
                                result: OperandType::Const(name_idx),
                                line: expr.span.line,
                            });
                            // For post-inc/dec, return old value (new_tmp); for pre, return new value (old_tmp)
                            return match op {
                                UnaryOp::PostIncrement | UnaryOp::PostDecrement => Ok(OperandType::Tmp(new_tmp)),
                                _ => Ok(OperandType::Tmp(old_tmp)),
                            };
                        }
                    }
                }

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
                // Special case: compact() - build array from variable names
                if let ExprKind::Identifier(n) = &name.kind {
                    let func_lower: Vec<u8> = n.iter().map(|b| b.to_ascii_lowercase()).collect();
                    if func_lower == b"extract" && !args.is_empty() {
                        // extract($array [, $flags [, $prefix]])
                        // Compile the array argument
                        let arr_operand = self.compile_expr(&args[0].value)?;
                        let flags = if args.len() > 1 {
                            self.compile_expr(&args[1].value)?
                        } else {
                            let idx = self.op_array.add_literal(Value::Long(0)); // EXTR_OVERWRITE
                            OperandType::Const(idx)
                        };
                        let tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::Extract,
                            op1: arr_operand,
                            op2: flags,
                            result: OperandType::Tmp(tmp),
                            line: expr.span.line,
                        });
                        return Ok(OperandType::Tmp(tmp));
                    }
                    if func_lower == b"compact" && !args.is_empty() {
                        // compact("foo", "bar") => ["foo" => $foo, "bar" => $bar]
                        let arr_tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::ArrayNew,
                            op1: OperandType::Unused,
                            op2: OperandType::Unused,
                            result: OperandType::Tmp(arr_tmp),
                            line: expr.span.line,
                        });
                        for arg in args {
                            if let ExprKind::String(s) = &arg.value.kind {
                                let cv = self.op_array.get_or_create_cv(s);
                                let key_idx = self.op_array.add_literal(Value::String(PhpString::from_vec(s.clone())));
                                self.op_array.emit(Op {
                                    opcode: OpCode::ArraySet,
                                    op1: OperandType::Tmp(arr_tmp),
                                    op2: OperandType::Cv(cv),
                                    result: OperandType::Const(key_idx),
                                    line: expr.span.line,
                                });
                            }
                        }
                        return Ok(OperandType::Tmp(arr_tmp));
                    }
                }

                // Compile the function name
                let resolved_name = match &name.kind {
                    ExprKind::Identifier(n) => Some(self.resolve_function_name(n)),
                    _ => None,
                };
                let name_op = match &resolved_name {
                    Some(resolved) => {
                        let idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_vec(resolved.clone())));
                        OperandType::Const(idx)
                    }
                    None => self.compile_expr(name)?,
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

                // Send arguments (pass function name for by-ref detection)
                let func_name_for_ref = if let Some(ref r) = resolved_name {
                    Some(r.as_slice())
                } else if let ExprKind::Identifier(ref n) = name.kind {
                    Some(n.as_slice())
                } else {
                    None
                };
                self.compile_send_args_with_name(args, expr.span.line, func_name_for_ref)?;

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

            ExprKind::Pipe { value, callable } => {
                // $x |> $f compiles as $f($x)
                // Desugar into a FunctionCall expression
                let arg = Argument {
                    name: None,
                    value: (**value).clone(),
                    unpack: false,
                };
                let synthetic_call = Expr {
                    kind: ExprKind::FunctionCall {
                        name: callable.clone(),
                        args: vec![arg],
                    },
                    span: expr.span,
                };
                return self.compile_expr(&synthetic_call);
            }

            ExprKind::NullCoalesce { left, right } => {
                // $a ?? $b: if $a is not null, use $a, else use $b
                // Suppress warnings for the left side (undefined keys, undefined vars, etc.)
                self.op_array.emit(Op {
                    opcode: OpCode::ErrorSuppress,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                let result_tmp = self.op_array.alloc_temp();

                // For ArrayAccess on objects, $obj[key] ?? default should call
                // offsetExists first, then offsetGet only if exists returns true
                if matches!(left.kind, ExprKind::ArrayAccess { .. }) {
                    // Flatten the chain of ArrayAccess nodes
                    let mut chain: Vec<&Expr> = Vec::new();
                    let mut base_expr: &Expr = left;
                    while let ExprKind::ArrayAccess { array, index } = &base_expr.kind {
                        if let Some(idx_expr) = index {
                            chain.push(idx_expr);
                        }
                        base_expr = array;
                    }
                    chain.reverse();

                    let base_op = self.compile_expr(base_expr)?;
                    let mut jmp_to_right: Vec<usize> = Vec::new();
                    let mut current = base_op;

                    for idx_expr in chain.iter() {
                        let idx = self.compile_expr(idx_expr)?;

                        // Check offsetExists first
                        let isset_tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::ArrayIsset,
                            op1: current.clone(),
                            op2: idx.clone(),
                            result: OperandType::Tmp(isset_tmp),
                            line: expr.span.line,
                        });
                        let jmp_pos = self.op_array.ops.len();
                        jmp_to_right.push(jmp_pos);
                        self.op_array.emit(Op {
                            opcode: OpCode::JmpZ,
                            op1: OperandType::Tmp(isset_tmp),
                            op2: OperandType::JmpTarget(0), // patched later
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });

                        // Get the value (only reached if exists)
                        let get_tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::ArrayGet,
                            op1: current,
                            op2: idx,
                            result: OperandType::Tmp(get_tmp),
                            line: expr.span.line,
                        });
                        current = OperandType::Tmp(get_tmp);
                    }

                    // Final value check: even if key exists, the value might be null
                    let check_tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::IssetCheck,
                        op1: current.clone(),
                        op2: OperandType::Unused,
                        result: OperandType::Tmp(check_tmp),
                        line: expr.span.line,
                    });
                    let jmp_null_pos = self.op_array.ops.len();
                    jmp_to_right.push(jmp_null_pos);
                    self.op_array.emit(Op {
                        opcode: OpCode::JmpZ,
                        op1: OperandType::Tmp(check_tmp),
                        op2: OperandType::JmpTarget(0), // patched later
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

                    // Not null: use left value
                    self.op_array.emit(Op {
                        opcode: OpCode::Assign,
                        op1: OperandType::Tmp(result_tmp),
                        op2: current,
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

                    // Right side: use default value
                    let right_target = self.op_array.current_offset();
                    for jmp_pos in jmp_to_right {
                        self.op_array.ops[jmp_pos].op2 = OperandType::JmpTarget(right_target as u32);
                    }
                    self.op_array.emit(Op {
                        opcode: OpCode::ErrorRestore,
                        op1: OperandType::Unused,
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
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

                    return Ok(OperandType::Tmp(result_tmp));
                }

                let left_val = self.compile_expr(left)?;
                self.op_array.emit(Op {
                    opcode: OpCode::ErrorRestore,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                // Check if left is set (not null and not undef)
                let check_tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::IssetCheck,
                    op1: left_val,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(check_tmp),
                    line: expr.span.line,
                });
                // If NOT set (null/undef), jump to use right side
                let jmp_null = self.op_array.emit(Op {
                    opcode: OpCode::JmpZ,
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
                        message: "Cannot use [] for reading".into(),
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
                    b"__file__" => {
                        if self.source_file.is_empty() {
                            Value::String(PhpString::from_bytes(b"unknown"))
                        } else {
                            Value::String(PhpString::from_vec(self.source_file.clone()))
                        }
                    }
                    b"__dir__" => {
                        if self.source_file.is_empty() {
                            Value::String(PhpString::from_bytes(b"."))
                        } else {
                            // Extract directory from file path
                            let path = String::from_utf8_lossy(&self.source_file);
                            let dir = if let Some(pos) = path.rfind('/') {
                                &path[..pos]
                            } else if let Some(pos) = path.rfind('\\') {
                                &path[..pos]
                            } else {
                                "."
                            };
                            Value::String(PhpString::from_string(dir.to_string()))
                        }
                    }
                    b"__function__" => {
                        let name = self.op_array.name.clone();
                        Value::String(PhpString::from_vec(name))
                    }
                    b"__class__" => {
                        let class_val = if let Some(ref class_name) = self.current_class {
                            Value::String(PhpString::from_vec(class_name.clone()))
                        } else {
                            Value::String(PhpString::empty())
                        };
                        // Track this literal as a __CLASS__ magic constant for trait patching
                        let idx = self.op_array.add_literal(class_val);
                        self.op_array.class_const_literals.push(idx);
                        return Ok(OperandType::Const(idx));
                    }
                    b"__method__" => {
                        let method_val = if let Some(ref class_name) = self.current_class {
                            let mut method = class_name.clone();
                            method.extend_from_slice(b"::");
                            method.extend_from_slice(&self.op_array.name);
                            Value::String(PhpString::from_vec(method))
                        } else if self.op_array.name.is_empty() || self.op_array.name == b"main" {
                            // __METHOD__ outside of a method returns ""
                            Value::String(PhpString::empty())
                        } else {
                            Value::String(PhpString::from_vec(self.op_array.name.clone()))
                        };
                        // Track this literal as a __CLASS__-derived constant for trait patching (for __METHOD__)
                        let idx = self.op_array.add_literal(method_val);
                        // __METHOD__ also needs patching (class part)
                        // We use a negative convention: store -(idx+1) for __METHOD__ literals
                        // Actually, let's just handle it in the trait patching code
                        return Ok(OperandType::Const(idx));
                    }
                    b"__namespace__" => Value::String(PhpString::from_vec(self.current_namespace.clone())),
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
                    b"stdin" => Value::Long(1),
                    b"stdout" => Value::Long(2),
                    b"stderr" => Value::Long(3),
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
                    b"sort_natural" => Value::Long(6),
                    b"sort_locale_string" => Value::Long(5),
                    b"sort_asc" => Value::Long(4),
                    b"sort_desc" => Value::Long(3),
                    b"array_filter_use_both" => Value::Long(1),
                    b"array_filter_use_key" => Value::Long(2),
                    b"array_unique_regular" => Value::Long(0),
                    b"count_normal" => Value::Long(0),
                    b"count_recursive" => Value::Long(1),
                    // Rounding mode constants
                    b"php_round_half_up" => Value::Long(0),
                    b"php_round_half_down" => Value::Long(1),
                    b"php_round_half_even" => Value::Long(2),
                    b"php_round_half_odd" => Value::Long(3),
                    b"php_round_ceiling" => Value::Long(4),
                    b"php_round_floor" => Value::Long(5),
                    b"php_round_toward_zero" => Value::Long(6),
                    b"php_round_away_from_zero" => Value::Long(7),
                    // INI
                    b"ini_user" => Value::Long(1),
                    b"ini_perdir" => Value::Long(2),
                    b"ini_system" => Value::Long(4),
                    b"ini_all" => Value::Long(7),
                    // HTML entity constants
                    b"ent_compat" => Value::Long(2),
                    b"ent_quotes" => Value::Long(3),
                    b"ent_noquotes" => Value::Long(0),
                    b"ent_html401" => Value::Long(0),
                    b"ent_xml1" => Value::Long(16),
                    b"ent_xhtml" => Value::Long(32),
                    b"ent_html5" => Value::Long(48),
                    b"ent_substitute" => Value::Long(8),
                    b"ent_disallowed" => Value::Long(128),
                    // EXTR_ constants
                    b"extr_overwrite" => Value::Long(0),
                    b"extr_skip" => Value::Long(1),
                    b"extr_prefix_same" => Value::Long(2),
                    b"extr_prefix_all" => Value::Long(3),
                    b"extr_prefix_invalid" => Value::Long(4),
                    b"extr_if_exists" => Value::Long(6),
                    b"extr_prefix_if_exists" => Value::Long(7),
                    b"extr_refs" => Value::Long(256),
                    _ => {
                        // Unknown identifier - emit runtime constant lookup
                        // Handle fully-qualified names (starting with \), use aliases, or namespace prefix
                        let qualified = if name.starts_with(b"\\") {
                            // Fully qualified: strip leading \
                            name[1..].to_vec()
                        } else if lower.starts_with(b"namespace\\") {
                            // namespace\foo is a namespace-relative name
                            // Replace "namespace\" with the current namespace
                            let rest = &name[b"namespace\\".len()..];
                            if self.current_namespace.is_empty() {
                                rest.to_vec()
                            } else {
                                let mut result = self.current_namespace.clone();
                                result.push(b'\\');
                                result.extend_from_slice(rest);
                                result
                            }
                        } else if name.contains(&b'\\') {
                            // Qualified name: check use aliases for first part
                            let first_sep = name.iter().position(|&b| b == b'\\').unwrap();
                            let first_part = &name[..first_sep];
                            let first_lower: Vec<u8> = first_part.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(resolved) = self.use_map.get(&first_lower) {
                                let mut result = resolved.clone();
                                result.extend_from_slice(&name[first_sep..]);
                                result
                            } else {
                                self.prefix_with_namespace(name)
                            }
                        } else {
                            // Unqualified: check use_const_map, then prefix with namespace
                            if let Some(resolved) = self.use_const_map.get(name) {
                                resolved.clone()
                            } else {
                                self.prefix_with_namespace(name)
                            }
                        };
                        let name_idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_vec(qualified)));
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
                            // Resolve through namespace (handles "static" correctly since
                            // resolve_class_name passes it through unchanged)
                            self.resolve_class_name(name)
                        }
                    }
                    _ => {
                        let class_operand = self.compile_expr(class)?;
                        let tmp = self.op_array.alloc_temp();

                        // Create the object with dynamic class name
                        self.op_array.emit(Op {
                            opcode: OpCode::NewObject,
                            op1: class_operand,
                            op2: OperandType::Unused,
                            result: OperandType::Tmp(tmp),
                            line: expr.span.line,
                        });

                        // Call constructor
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

                        return Ok(OperandType::Tmp(tmp));
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
                    ExprKind::Identifier(name) => self.resolve_class_name(name),
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
                let code_op = self.compile_expr(inner)?;
                let result = OperandType::Tmp(self.op_array.alloc_temp());
                self.op_array.emit(Op {
                    opcode: OpCode::Eval,
                    op1: code_op,
                    op2: OperandType::Unused,
                    result,
                    line: expr.span.line,
                });
                Ok(result)
            }

            ExprKind::Isset(exprs) => {
                // isset() returns true if all vars are set and not null
                // Uses IssetCheck opcode that checks for both Undef and Null
                // Suppress warnings during isset check (e.g., Undefined array key)
                self.op_array.emit(Op {
                    opcode: OpCode::ErrorSuppress,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });
                if exprs.len() == 1 {
                    // Check if expression is a property access -> use PropertyIsset
                    let is_prop_access = matches!(
                        exprs[0].kind,
                        ExprKind::PropertyAccess { .. }
                    );
                    if is_prop_access {
                        if let ExprKind::PropertyAccess { object, property, .. } = &exprs[0].kind {
                            // For isset($aa[0]->foo), the inner object might be an ArrayAccess
                            // We need to check offsetExists first before calling offsetGet
                            let obj_operand = if matches!(object.kind, ExprKind::ArrayAccess { .. }) {
                                // Compile with isset-aware ArrayAccess handling
                                let mut chain: Vec<&Expr> = Vec::new();
                                let mut base_expr: &Expr = object;
                                while let ExprKind::ArrayAccess { array, index } = &base_expr.kind {
                                    if let Some(idx_expr) = index {
                                        chain.push(idx_expr);
                                    }
                                    base_expr = array;
                                }
                                chain.reverse();

                                let base_op = self.compile_expr(base_expr)?;
                                let result_tmp = self.op_array.alloc_temp();
                                let mut jmp_patches: Vec<usize> = Vec::new();
                                let mut current = base_op;

                                for idx_expr in chain.iter() {
                                    let idx = self.compile_expr(idx_expr)?;
                                    let isset_tmp = self.op_array.alloc_temp();
                                    self.op_array.emit(Op {
                                        opcode: OpCode::ArrayIsset,
                                        op1: current.clone(),
                                        op2: idx.clone(),
                                        result: OperandType::Tmp(isset_tmp),
                                        line: expr.span.line,
                                    });
                                    let jmp_pos = self.op_array.ops.len();
                                    jmp_patches.push(jmp_pos);
                                    self.op_array.emit(Op {
                                        opcode: OpCode::JmpZ,
                                        op1: OperandType::Tmp(isset_tmp),
                                        op2: OperandType::JmpTarget(0),
                                        result: OperandType::Unused,
                                        line: expr.span.line,
                                    });
                                    let get_tmp = self.op_array.alloc_temp();
                                    self.op_array.emit(Op {
                                        opcode: OpCode::ArrayGet,
                                        op1: current,
                                        op2: idx,
                                        result: OperandType::Tmp(get_tmp),
                                        line: expr.span.line,
                                    });
                                    current = OperandType::Tmp(get_tmp);
                                }

                                // Now do PropertyIsset on the result
                                let prop_operand = self.compile_property_name(property)?;
                                let tmp = self.op_array.alloc_temp();
                                self.op_array.emit(Op {
                                    opcode: OpCode::PropertyIsset,
                                    op1: current,
                                    op2: prop_operand,
                                    result: OperandType::Tmp(tmp),
                                    line: expr.span.line,
                                });
                                self.op_array.emit(Op {
                                    opcode: OpCode::Assign,
                                    op1: OperandType::Tmp(result_tmp),
                                    op2: OperandType::Tmp(tmp),
                                    result: OperandType::Unused,
                                    line: expr.span.line,
                                });

                                // Jump past false
                                let jmp_end_pos = self.op_array.ops.len();
                                self.op_array.emit(Op {
                                    opcode: OpCode::Jmp,
                                    op1: OperandType::JmpTarget(0),
                                    op2: OperandType::Unused,
                                    result: OperandType::Unused,
                                    line: expr.span.line,
                                });

                                // False label
                                let false_target = self.op_array.ops.len() as u32;
                                let false_lit = self.op_array.add_literal(Value::False);
                                self.op_array.emit(Op {
                                    opcode: OpCode::Assign,
                                    op1: OperandType::Tmp(result_tmp),
                                    op2: OperandType::Const(false_lit),
                                    result: OperandType::Unused,
                                    line: expr.span.line,
                                });
                                let end_target = self.op_array.ops.len() as u32;
                                for jmp_pos in jmp_patches {
                                    self.op_array.ops[jmp_pos].op2 = OperandType::JmpTarget(false_target);
                                }
                                self.op_array.ops[jmp_end_pos].op1 = OperandType::JmpTarget(end_target);

                                self.op_array.emit(Op {
                                    opcode: OpCode::ErrorRestore,
                                    op1: OperandType::Unused,
                                    op2: OperandType::Unused,
                                    result: OperandType::Unused,
                                    line: expr.span.line,
                                });
                                return Ok(OperandType::Tmp(result_tmp));
                            } else {
                                self.compile_expr(object)?
                            };
                            let prop_operand = self.compile_property_name(property)?;
                            let tmp = self.op_array.alloc_temp();
                            self.op_array.emit(Op {
                                opcode: OpCode::PropertyIsset,
                                op1: obj_operand,
                                op2: prop_operand,
                                result: OperandType::Tmp(tmp),
                                line: expr.span.line,
                            });
                            self.op_array.emit(Op {
                                opcode: OpCode::ErrorRestore,
                                op1: OperandType::Unused,
                                op2: OperandType::Unused,
                                result: OperandType::Unused,
                                line: expr.span.line,
                            });
                            return Ok(OperandType::Tmp(tmp));
                        }
                    }
                    // Check if expression is an array access -> use ArrayIsset
                    // This ensures objects implementing ArrayAccess call offsetExists()
                    // For nested access like isset($a[0][1][2]), we need to:
                    // 1. ArrayIsset($a, 0) - if false, jump to false label
                    // 2. ArrayGet($a, 0) -> tmp1
                    // 3. ArrayIsset(tmp1, 1) - if false, jump to false label
                    // 4. ArrayGet(tmp1, 1) -> tmp2
                    // 5. ArrayIsset(tmp2, 2) - final result
                    if matches!(exprs[0].kind, ExprKind::ArrayAccess { .. }) {
                        // Flatten the chain of ArrayAccess nodes
                        let mut chain: Vec<&Expr> = Vec::new(); // indices
                        let mut base_expr = &exprs[0];
                        while let ExprKind::ArrayAccess { array, index } = &base_expr.kind {
                            if let Some(idx_expr) = index {
                                chain.push(idx_expr);
                            }
                            base_expr = array;
                        }
                        chain.reverse(); // now chain[0] is the first index, chain[last] is the last

                        // Also check if base_expr is a PropertyAccess - handle isset($obj->prop[$key])
                        let base_op = self.compile_expr(base_expr)?;

                        let result_tmp = self.op_array.alloc_temp();
                        let false_label = self.op_array.ops.len() as u32 + 9999; // placeholder

                        // We'll collect jump positions that need patching to jump to the false label
                        let mut jmp_patches: Vec<usize> = Vec::new();

                        let mut current = base_op;
                        for (i, idx_expr) in chain.iter().enumerate() {
                            let is_last = i == chain.len() - 1;
                            let idx = self.compile_expr(idx_expr)?;

                            // Emit ArrayIsset
                            let isset_tmp = self.op_array.alloc_temp();
                            self.op_array.emit(Op {
                                opcode: OpCode::ArrayIsset,
                                op1: current.clone(),
                                op2: idx.clone(),
                                result: OperandType::Tmp(isset_tmp),
                                line: expr.span.line,
                            });

                            if is_last {
                                // Final level: the result of ArrayIsset is the final result
                                self.op_array.emit(Op {
                                    opcode: OpCode::Assign,
                                    op1: OperandType::Tmp(result_tmp),
                                    op2: OperandType::Tmp(isset_tmp),
                                    result: OperandType::Unused,
                                    line: expr.span.line,
                                });
                            } else {
                                // Intermediate level: if false, jump to end
                                let jmp_pos = self.op_array.ops.len();
                                jmp_patches.push(jmp_pos);
                                self.op_array.emit(Op {
                                    opcode: OpCode::JmpZ,
                                    op1: OperandType::Tmp(isset_tmp),
                                    op2: OperandType::JmpTarget(0), // will be patched
                                    result: OperandType::Unused,
                                    line: expr.span.line,
                                });

                                // Emit ArrayGet to get the value for the next level
                                let get_tmp = self.op_array.alloc_temp();
                                self.op_array.emit(Op {
                                    opcode: OpCode::ArrayGet,
                                    op1: current,
                                    op2: idx,
                                    result: OperandType::Tmp(get_tmp),
                                    line: expr.span.line,
                                });
                                current = OperandType::Tmp(get_tmp);
                            }
                        }

                        // Jump past the false assignment
                        let jmp_end_pos = self.op_array.ops.len();
                        self.op_array.emit(Op {
                            opcode: OpCode::Jmp,
                            op1: OperandType::JmpTarget(0), // will be patched
                            op2: OperandType::Unused,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });

                        // False label: set result to false
                        let false_target = self.op_array.ops.len() as u32;
                        let false_lit = self.op_array.add_literal(Value::False);
                        self.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Tmp(result_tmp),
                            op2: OperandType::Const(false_lit),
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });

                        // End label
                        let end_target = self.op_array.ops.len() as u32;

                        // Patch all jump positions
                        for jmp_pos in jmp_patches {
                            self.op_array.ops[jmp_pos].op2 = OperandType::JmpTarget(false_target);
                        }
                        self.op_array.ops[jmp_end_pos].op1 = OperandType::JmpTarget(end_target);

                        self.op_array.emit(Op {
                            opcode: OpCode::ErrorRestore,
                            op1: OperandType::Unused,
                            op2: OperandType::Unused,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        return Ok(OperandType::Tmp(result_tmp));
                    }
                    let val = self.compile_expr(&exprs[0])?;
                    let tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::IssetCheck,
                        op1: val,
                        op2: OperandType::Unused,
                        result: OperandType::Tmp(tmp),
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
                } else {
                    // Multiple args: AND all together
                    let mut result_tmp = self.op_array.alloc_temp();
                    for (i, e) in exprs.iter().enumerate() {
                        // For array access expressions, use ArrayIsset to call offsetExists
                        let check_tmp = if let ExprKind::ArrayAccess { array, index } = &e.kind {
                            let arr = self.compile_expr(array)?;
                            let idx = if let Some(idx_expr) = index {
                                self.compile_expr(idx_expr)?
                            } else {
                                let lit_idx = self.op_array.add_literal(Value::Null);
                                OperandType::Const(lit_idx)
                            };
                            let t = self.op_array.alloc_temp();
                            self.op_array.emit(Op {
                                opcode: OpCode::ArrayIsset,
                                op1: arr,
                                op2: idx,
                                result: OperandType::Tmp(t),
                                line: expr.span.line,
                            });
                            t
                        } else {
                            let val = self.compile_expr(e)?;
                            let t = self.op_array.alloc_temp();
                            self.op_array.emit(Op {
                                opcode: OpCode::IssetCheck,
                                op1: val,
                                op2: OperandType::Unused,
                                result: OperandType::Tmp(t),
                                line: expr.span.line,
                            });
                            t
                        };
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
                    self.op_array.emit(Op {
                        opcode: OpCode::ErrorRestore,
                        op1: OperandType::Unused,
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
                    Ok(OperandType::Tmp(result_tmp))
                }
            }

            ExprKind::Empty(inner) => {
                self.op_array.emit(Op {
                    opcode: OpCode::ErrorSuppress,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                // For empty($obj[$key]), PHP calls offsetExists first.
                // If false -> empty is true. If true -> calls offsetGet, checks if value is empty.
                if matches!(inner.kind, ExprKind::ArrayAccess { .. }) {
                    let mut chain: Vec<&Expr> = Vec::new();
                    let mut base_expr: &Expr = inner;
                    while let ExprKind::ArrayAccess { array, index } = &base_expr.kind {
                        if let Some(idx_expr) = index {
                            chain.push(idx_expr);
                        }
                        base_expr = array;
                    }
                    chain.reverse();

                    let base_op = self.compile_expr(base_expr)?;
                    let result_tmp = self.op_array.alloc_temp();
                    let mut jmp_to_true: Vec<usize> = Vec::new(); // jumps when exists=false -> empty=true
                    let mut current = base_op;

                    for (i, idx_expr) in chain.iter().enumerate() {
                        let is_last = i == chain.len() - 1;
                        let idx = self.compile_expr(idx_expr)?;

                        // Check offsetExists
                        let isset_tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::ArrayIsset,
                            op1: current.clone(),
                            op2: idx.clone(),
                            result: OperandType::Tmp(isset_tmp),
                            line: expr.span.line,
                        });
                        let jmp_pos = self.op_array.ops.len();
                        jmp_to_true.push(jmp_pos);
                        self.op_array.emit(Op {
                            opcode: OpCode::JmpZ,
                            op1: OperandType::Tmp(isset_tmp),
                            op2: OperandType::JmpTarget(0), // patched later
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });

                        // Get the value
                        let get_tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::ArrayGet,
                            op1: current,
                            op2: idx,
                            result: OperandType::Tmp(get_tmp),
                            line: expr.span.line,
                        });
                        current = OperandType::Tmp(get_tmp);
                    }

                    self.op_array.emit(Op {
                        opcode: OpCode::ErrorRestore,
                        op1: OperandType::Unused,
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });

                    // Value exists: check if it's empty (BooleanNot)
                    let not_tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::BooleanNot,
                        op1: current,
                        op2: OperandType::Unused,
                        result: OperandType::Tmp(not_tmp),
                        line: expr.span.line,
                    });
                    self.op_array.emit(Op {
                        opcode: OpCode::Assign,
                        op1: OperandType::Tmp(result_tmp),
                        op2: OperandType::Tmp(not_tmp),
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
                    let jmp_end_pos = self.op_array.ops.len();
                    self.op_array.emit(Op {
                        opcode: OpCode::Jmp,
                        op1: OperandType::JmpTarget(0),
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });

                    // True label (key doesn't exist -> empty is true)
                    let true_target = self.op_array.ops.len() as u32;
                    let true_lit = self.op_array.add_literal(Value::True);
                    self.op_array.emit(Op {
                        opcode: OpCode::Assign,
                        op1: OperandType::Tmp(result_tmp),
                        op2: OperandType::Const(true_lit),
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

                    let end_target = self.op_array.ops.len() as u32;
                    for jmp_pos in jmp_to_true {
                        self.op_array.ops[jmp_pos].op2 = OperandType::JmpTarget(true_target);
                    }
                    self.op_array.ops[jmp_end_pos].op1 = OperandType::JmpTarget(end_target);

                    return Ok(OperandType::Tmp(result_tmp));
                }

                let val = self.compile_expr(inner)?;
                self.op_array.emit(Op {
                    opcode: OpCode::ErrorRestore,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });
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
                is_static,
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
                closure_compiler.op_array.is_static_closure = *is_static;
                closure_compiler.op_array.decl_line = expr.span.line;
                closure_compiler.source_file = self.source_file.clone();
                closure_compiler.current_class = self.current_class.clone();
                closure_compiler.current_parent_class = self.current_parent_class.clone();
                // Inherit scope_class from the enclosing function for visibility checks
                closure_compiler.op_array.scope_class = self.op_array.scope_class.clone()
                    .or_else(|| self.current_class.as_ref().map(|c| c.iter().map(|b| b.to_ascii_lowercase()).collect()));

                // If inside a class method and not static, automatically capture $this
                let has_this = !is_static
                    && self.current_class.is_some()
                    && self.op_array.cv_names.contains(&b"this".to_vec());
                if has_this {
                    closure_compiler.op_array.get_or_create_cv(b"this");
                }

                // Set up use vars as the first CVs (before params)
                for use_var in use_vars {
                    closure_compiler
                        .op_array
                        .get_or_create_cv(&use_var.variable);
                }
                // Count required params and set param_count
                closure_compiler.op_array.param_count = params.len() as u32
                    + use_vars.len() as u32
                    + if !*is_static && self.current_class.is_some() { 1 } else { 0 };
                closure_compiler.op_array.required_param_count = params
                    .iter()
                    .filter(|p| p.default.is_none() && !p.variadic)
                    .count() as u32;

                // Set up parameter CVs
                for param in params {
                    let cv = closure_compiler.op_array.get_or_create_cv(&param.name);

                    // Handle variadic parameter
                    if param.variadic {
                        closure_compiler.op_array.variadic_param = Some(cv);
                    }

                    // Store parameter type info
                    let type_info = param.type_hint.as_ref().map(|hint| {
                        let mut pt = type_hint_to_param_type_with_ns(hint, &self.current_namespace, &self.use_map);
                        if let Some(default_expr) = &param.default {
                            if matches!(default_expr.kind, ExprKind::Null) && !is_type_nullable_or_mixed(&pt) {
                                pt = ParamType::Nullable(Box::new(pt));
                            }
                        }
                        ParamTypeInfo {
                            param_type: pt,
                            param_name: param.name.clone(),
                        }
                    });
                    while closure_compiler.op_array.param_types.len() <= cv as usize {
                        closure_compiler.op_array.param_types.push(None);
                    }
                    closure_compiler.op_array.param_types[cv as usize] = type_info;

                    // Compile default value
                    if let Some(default_expr) = &param.default {
                        let default_val = closure_compiler.compile_expr(default_expr)?;
                        let undef_idx = closure_compiler.op_array.add_literal(Value::Undef);
                        let check_tmp = closure_compiler.op_array.alloc_temp();
                        closure_compiler.op_array.emit(Op {
                            opcode: OpCode::Identical,
                            op1: OperandType::Cv(cv),
                            op2: OperandType::Const(undef_idx),
                            result: OperandType::Tmp(check_tmp),
                            line: 0,
                        });
                        let jmp_skip = closure_compiler.op_array.emit(Op {
                            opcode: OpCode::JmpZ,
                            op1: OperandType::Tmp(check_tmp),
                            op2: OperandType::JmpTarget(0),
                            result: OperandType::Unused,
                            line: 0,
                        });
                        closure_compiler.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Cv(cv),
                            op2: default_val,
                            result: OperandType::Unused,
                            line: 0,
                        });
                        let after = closure_compiler.op_array.current_offset();
                        closure_compiler.op_array.patch_jump(jmp_skip, after);
                    }
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

                // If there are use vars (or $this), create an array [closure_name, use_val_1, ...]
                let needs_capture = !use_vars.is_empty() || has_this;
                if needs_capture {
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
                    // Capture $this if in class context
                    if has_this {
                        let this_cv = self.op_array.get_or_create_cv(b"this");
                        self.op_array.emit(Op {
                            opcode: OpCode::ArrayAppend,
                            op1: OperandType::Tmp(arr_tmp),
                            op2: OperandType::Cv(this_cv),
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                    }
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
                closure_compiler.op_array.decl_line = expr.span.line;
                closure_compiler.source_file = self.source_file.clone();
                closure_compiler.current_class = self.current_class.clone();
                closure_compiler.current_parent_class = self.current_parent_class.clone();
                // Inherit scope_class from the enclosing function for visibility checks
                closure_compiler.op_array.scope_class = self.op_array.scope_class.clone()
                    .or_else(|| self.current_class.as_ref().map(|c| c.iter().map(|b| b.to_ascii_lowercase()).collect()));

                // Set up use vars as the first CVs (before params)
                for uv in &use_vars {
                    closure_compiler.op_array.get_or_create_cv(uv);
                }
                // Set up parameter CVs
                for param in params {
                    let cv = closure_compiler.op_array.get_or_create_cv(&param.name);

                    // Store parameter type info
                    let type_info = param.type_hint.as_ref().map(|hint| {
                        let mut pt = type_hint_to_param_type_with_ns(hint, &self.current_namespace, &self.use_map);
                        if let Some(default_expr) = &param.default {
                            if matches!(default_expr.kind, ExprKind::Null) && !is_type_nullable_or_mixed(&pt) {
                                pt = ParamType::Nullable(Box::new(pt));
                            }
                        }
                        ParamTypeInfo {
                            param_type: pt,
                            param_name: param.name.clone(),
                        }
                    });
                    while closure_compiler.op_array.param_types.len() <= cv as usize {
                        closure_compiler.op_array.param_types.push(None);
                    }
                    closure_compiler.op_array.param_types[cv as usize] = type_info;
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

            ExprKind::YieldFrom(inner) => {
                let val = self.compile_expr(inner)?;
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::YieldFrom,
                    op1: val,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
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

            ExprKind::ThrowExpr(inner) => {
                let val = self.compile_expr(inner)?;
                self.op_array.emit(Op {
                    opcode: OpCode::Throw,
                    op1: val,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });
                // throw never returns, but we need a result for expression context
                let null_idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(null_idx))
            }

            ExprKind::ClassConstAccess { class, constant } => {
                // Handle ClassName::class, ClassName::CONST, self::CONST
                let class_name = match &class.kind {
                    ExprKind::Identifier(name) => self.resolve_class_name(name),
                    _ => {
                        // $expr::class - emit GetClassName opcode
                        if constant == b"class" {
                            let obj_operand = self.compile_expr(class)?;
                            let tmp = self.op_array.alloc_temp();
                            self.op_array.emit(Op {
                                opcode: OpCode::GetClassName,
                                op1: obj_operand,
                                op2: OperandType::Unused,
                                result: OperandType::Tmp(tmp),
                                line: expr.span.line,
                            });
                            return Ok(OperandType::Tmp(tmp));
                        }
                        // $obj::CONST - get class name from object, then look up constant
                        let obj_operand = self.compile_expr(class)?;
                        // Get class name at runtime
                        let class_tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::GetClassName,
                            op1: obj_operand,
                            op2: OperandType::Unused,
                            result: OperandType::Tmp(class_tmp),
                            line: expr.span.line,
                        });
                        // Look up the constant using StaticPropGet
                        let const_name_idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_vec(constant.clone())));
                        let tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::StaticPropGet,
                            op1: OperandType::Tmp(class_tmp),
                            op2: OperandType::Const(const_name_idx),
                            result: OperandType::Tmp(tmp),
                            line: expr.span.line,
                        });
                        return Ok(OperandType::Tmp(tmp));
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

            ExprKind::DynamicClassConstAccess { class, constant } => {
                // Dynamic class constant fetch: Foo::{$expr}
                let class_name = match &class.kind {
                    ExprKind::Identifier(name) => {
                        let resolved = self.resolve_class_name(name);
                        if resolved.eq_ignore_ascii_case(b"self") {
                            self.current_class.clone().unwrap_or(resolved)
                        } else if resolved.eq_ignore_ascii_case(b"parent") {
                            self.current_parent_class.clone().unwrap_or(resolved)
                        } else if resolved.eq_ignore_ascii_case(b"static") {
                            b"static".to_vec()
                        } else {
                            resolved
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
                let const_expr = self.compile_expr(constant)?;
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::StaticPropGet,
                    op1: OperandType::Const(class_idx),
                    op2: const_expr,
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
                    ExprKind::Identifier(name) => Some(self.resolve_class_name(name)),
                    _ => None, // Dynamic class expression - resolve at runtime
                };

                if let Some(class_name) = class_name {
                    // Static class name known at compile time
                    // Resolve self:: and parent:: to actual class names
                    // static:: is kept as literal "static" for late static binding
                    let resolved_class = if class_name.eq_ignore_ascii_case(b"self") {
                        self.current_class.clone().unwrap_or(class_name.clone())
                    } else if class_name.eq_ignore_ascii_case(b"static") {
                        b"static".to_vec()
                    } else if class_name.eq_ignore_ascii_case(b"parent") {
                        self.current_parent_class
                            .clone()
                            .unwrap_or(class_name.clone())
                    } else {
                        class_name.clone()
                    };

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
                } else {
                    // Dynamic class expression: $obj::method() or expr::method()
                    // Compile the class expression, then use DynamicStaticCall opcode
                    let class_operand = self.compile_expr(class)?;
                    let method_idx = self
                        .op_array
                        .add_literal(Value::String(PhpString::from_vec(method.to_vec())));
                    let arg_count = self.op_array.add_literal(Value::Long(args.len() as i64));
                    self.op_array.emit(Op {
                        opcode: OpCode::InitDynamicStaticCall,
                        op1: class_operand,
                        op2: OperandType::Const(method_idx),
                        result: OperandType::Const(arg_count),
                        line: expr.span.line,
                    });
                }

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

            ExprKind::DynamicStaticMethodCall {
                class,
                method,
                args,
            } => {
                // Dynamic static method call: Foo::$method()
                let class_operand = match &class.kind {
                    ExprKind::Identifier(name) => {
                        let resolved = self.resolve_class_name(name);
                        let resolved = if resolved.eq_ignore_ascii_case(b"self") {
                            self.current_class.clone().unwrap_or(resolved)
                        } else if resolved.eq_ignore_ascii_case(b"parent") {
                            self.current_parent_class.clone().unwrap_or(resolved)
                        } else {
                            resolved
                        };
                        let idx = self.op_array.add_literal(Value::String(PhpString::from_vec(resolved)));
                        OperandType::Const(idx)
                    }
                    _ => self.compile_expr(class)?,
                };
                let method_operand = self.compile_expr(method)?;
                let arg_count = self.op_array.add_literal(Value::Long(args.len() as i64));
                self.op_array.emit(Op {
                    opcode: OpCode::InitDynamicStaticCall,
                    op1: class_operand,
                    op2: method_operand,
                    result: OperandType::Const(arg_count),
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
                        let resolved = self.resolve_class_name(name);
                        // Resolve self/parent, keep static as literal for LSB
                        if resolved.eq_ignore_ascii_case(b"self") {
                            self.current_class.clone().unwrap_or(resolved)
                        } else if resolved.eq_ignore_ascii_case(b"static") {
                            // Late static binding: resolve at runtime
                            b"static".to_vec()
                        } else if resolved.eq_ignore_ascii_case(b"parent") {
                            self.current_parent_class.clone().unwrap_or(resolved)
                        } else {
                            resolved
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
                let prop_operand = match &property.kind {
                    ExprKind::Identifier(name) => {
                        let name_idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_vec(name.clone())));
                        OperandType::Const(name_idx)
                    }
                    _ => {
                        // Dynamic property name: $obj->$prop
                        self.compile_expr(property)?
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

                self.op_array.emit(Op {
                    opcode: OpCode::PropertyGet,
                    op1: obj,
                    op2: prop_operand,
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

                let method_operand = match &method.kind {
                    ExprKind::Identifier(name) => {
                        let name_idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_vec(name.clone())));
                        OperandType::Const(name_idx)
                    }
                    _ => {
                        // Dynamic method name (e.g., $this->$method())
                        self.compile_expr(method)?
                    }
                };

                self.op_array.emit(Op {
                    opcode: OpCode::InitMethodCall,
                    op1: obj,
                    op2: method_operand,
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
                // Check for $this = &$value
                if let ExprKind::Variable(target_name) = &target.kind {
                    if target_name == b"this" && self.current_class.is_some() {
                        return Err(CompileError {
                            message: "Cannot re-assign $this".into(),
                            line: expr.span.line,
                        });
                    }
                }
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

            ExprKind::FirstClassCallable(target) => {
                return self.compile_first_class_callable(target, expr.span.line);
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

    /// Compile first-class callable syntax: strlen(...), $obj->method(...), Foo::method(...)
    /// Creates a synthetic closure that wraps the call and forwards arguments via ...$args.
    fn compile_first_class_callable(
        &mut self,
        target: &CallableTarget,
        line: u32,
    ) -> CompileResult<OperandType> {
        // Check for nullsafe operator which cannot be combined with closure creation
        if let CallableTarget::Method { nullsafe: true, .. } = target {
            return Err(CompileError {
                message: "Cannot combine nullsafe operator with Closure creation".into(),
                line,
            });
        }

        let closure_id = self.op_array.child_functions.len();
        let closure_name = format!("__closure_fcc_{}", closure_id).into_bytes();

        let mut cc = Compiler::new();
        cc.op_array.name = closure_name.clone();
        cc.current_class = self.current_class.clone();
        cc.current_parent_class = self.current_parent_class.clone();
        cc.op_array.scope_class = self.op_array.scope_class.clone()
            .or_else(|| self.current_class.as_ref().map(|c| c.iter().map(|b| b.to_ascii_lowercase()).collect()));

        // Determine what needs to be captured.
        // - Function with Identifier: no capture needed (resolved at compile time)
        // - Function with dynamic expr (Variable, etc.): capture the callable value
        // - Method call: capture the object
        // - Static method: no capture needed (resolved at compile time)
        let needs_capture = match target {
            CallableTarget::Function(name_expr) => !matches!(name_expr.kind, ExprKind::Identifier(_)),
            CallableTarget::Method { .. } => true,
            CallableTarget::StaticMethod { .. } => false,
        };

        if needs_capture {
            // Reserve cv0 for the captured value (object or callable)
            cc.op_array.get_or_create_cv(b"__fcc_captured");
        }

        // Set up variadic ...$args parameter
        let args_cv = cc.op_array.get_or_create_cv(b"args");
        cc.op_array.param_count = 1;
        cc.op_array.required_param_count = 0;
        cc.op_array.variadic_param = Some(args_cv);

        // Now emit the call inside the closure body
        match target {
            CallableTarget::Function(name_expr) => {
                let call_op = if let ExprKind::Identifier(n) = &name_expr.kind {
                    // Static function name - resolve at compile time
                    let resolved_name = self.resolve_function_name(n);
                    let name_idx = cc.op_array.add_literal(Value::String(PhpString::from_vec(resolved_name)));
                    OperandType::Const(name_idx)
                } else {
                    // Dynamic callable - use captured value from cv0
                    OperandType::Cv(0) // __fcc_captured
                };
                let arg_count_idx = cc.op_array.add_literal(Value::Long(0));
                cc.op_array.emit(Op {
                    opcode: OpCode::InitFCall,
                    op1: call_op,
                    op2: OperandType::Const(arg_count_idx),
                    result: OperandType::Unused,
                    line,
                });
                cc.op_array.emit(Op {
                    opcode: OpCode::SendUnpack,
                    op1: OperandType::Cv(args_cv),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line,
                });
                let result_tmp = cc.op_array.alloc_temp();
                cc.op_array.emit(Op {
                    opcode: OpCode::DoFCall,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(result_tmp),
                    line,
                });
                cc.op_array.emit(Op {
                    opcode: OpCode::Return,
                    op1: OperandType::Tmp(result_tmp),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line,
                });
            }
            CallableTarget::Method { method, .. } => {
                // Object is captured in cv0 (__fcc_captured)
                let obj_cv = 0u32;
                let method_name = match &method.kind {
                    ExprKind::Identifier(name) => name.clone(),
                    _ => b"__invoke".to_vec(),
                };
                let method_idx = cc.op_array.add_literal(Value::String(PhpString::from_vec(method_name)));
                cc.op_array.emit(Op {
                    opcode: OpCode::InitMethodCall,
                    op1: OperandType::Cv(obj_cv),
                    op2: OperandType::Const(method_idx),
                    result: OperandType::Unused,
                    line,
                });
                cc.op_array.emit(Op {
                    opcode: OpCode::SendUnpack,
                    op1: OperandType::Cv(args_cv),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line,
                });
                let result_tmp = cc.op_array.alloc_temp();
                cc.op_array.emit(Op {
                    opcode: OpCode::DoFCall,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(result_tmp),
                    line,
                });
                cc.op_array.emit(Op {
                    opcode: OpCode::Return,
                    op1: OperandType::Tmp(result_tmp),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line,
                });
            }
            CallableTarget::StaticMethod { class, method } => {
                let class_name = match &class.kind {
                    ExprKind::Identifier(name) => self.resolve_class_name(name),
                    _ => {
                        return Err(CompileError {
                            message: "unsupported static method callable target".into(),
                            line,
                        });
                    }
                };
                let resolved_class = if class_name.eq_ignore_ascii_case(b"self") {
                    self.current_class.clone().unwrap_or(class_name.clone())
                } else if class_name.eq_ignore_ascii_case(b"static") {
                    b"static".to_vec()
                } else if class_name.eq_ignore_ascii_case(b"parent") {
                    self.current_parent_class.clone().unwrap_or(class_name.clone())
                } else {
                    class_name.clone()
                };
                let mut func_name = resolved_class;
                func_name.extend_from_slice(b"::");
                func_name.extend_from_slice(method);
                let name_idx = cc.op_array.add_literal(Value::String(PhpString::from_vec(func_name)));
                let arg_count_idx = cc.op_array.add_literal(Value::Long(0));
                cc.op_array.emit(Op {
                    opcode: OpCode::InitFCall,
                    op1: OperandType::Const(name_idx),
                    op2: OperandType::Const(arg_count_idx),
                    result: OperandType::Unused,
                    line,
                });
                cc.op_array.emit(Op {
                    opcode: OpCode::SendUnpack,
                    op1: OperandType::Cv(args_cv),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line,
                });
                let result_tmp = cc.op_array.alloc_temp();
                cc.op_array.emit(Op {
                    opcode: OpCode::DoFCall,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(result_tmp),
                    line,
                });
                cc.op_array.emit(Op {
                    opcode: OpCode::Return,
                    op1: OperandType::Tmp(result_tmp),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line,
                });
            }
        }

        self.op_array.child_functions.push(cc.op_array);

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
            line,
        });

        if needs_capture {
            // Compile the captured value in the outer scope
            let captured_op = match target {
                CallableTarget::Method { object, .. } => self.compile_expr(object)?,
                CallableTarget::Function(name_expr) => self.compile_expr(name_expr)?,
                _ => unreachable!(),
            };

            let arr_tmp = self.op_array.alloc_temp();
            self.op_array.emit(Op {
                opcode: OpCode::ArrayNew,
                op1: OperandType::Unused,
                op2: OperandType::Unused,
                result: OperandType::Tmp(arr_tmp),
                line,
            });
            let name_val = self
                .op_array
                .add_literal(Value::String(PhpString::from_vec(closure_name)));
            self.op_array.emit(Op {
                opcode: OpCode::ArrayAppend,
                op1: OperandType::Tmp(arr_tmp),
                op2: OperandType::Const(name_val),
                result: OperandType::Unused,
                line,
            });
            self.op_array.emit(Op {
                opcode: OpCode::ArrayAppend,
                op1: OperandType::Tmp(arr_tmp),
                op2: captured_op,
                result: OperandType::Unused,
                line,
            });
            Ok(OperandType::Tmp(arr_tmp))
        } else {
            // No captures - just return the closure name
            let name_val_idx = self
                .op_array
                .add_literal(Value::String(PhpString::from_vec(closure_name)));
            Ok(OperandType::Const(name_val_idx))
        }
    }

    /// Evaluate a constant expression at compile time (for property defaults, etc.)
    /// Check if an expression (foreach value/key target) contains $this assignment
    fn check_foreach_this_assign(&self, expr: &Expr, _line: u32) -> CompileResult<bool> {
        match &expr.kind {
            ExprKind::Variable(name) if name == b"this" => Ok(true),
            ExprKind::Array(elems) => {
                for elem in elems {
                    if self.check_foreach_this_assign(&elem.value, _line)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            ExprKind::FunctionCall { name, args }
                if matches!(&name.kind, ExprKind::Identifier(n) if n.eq_ignore_ascii_case(b"list")) =>
            {
                for arg in args {
                    if self.check_foreach_this_assign(&arg.value, _line)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn eval_const_expr(expr: &Expr) -> Option<Value> {
        match &expr.kind {
            ExprKind::Int(n) => Some(Value::Long(*n)),
            ExprKind::Float(f) => Some(Value::Double(*f)),
            ExprKind::String(s) => Some(Value::String(PhpString::from_vec(s.clone()))),
            ExprKind::True => Some(Value::True),
            ExprKind::False => Some(Value::False),
            ExprKind::Null => Some(Value::Null),
            ExprKind::Array(elements) => {
                let mut arr = crate::array::PhpArray::new();
                for elem in elements {
                    let val = Self::eval_const_expr(&elem.value)?;
                    if let Some(key_expr) = &elem.key {
                        let k = Self::eval_const_expr(key_expr)?;
                        let key = match k {
                            Value::Long(n) => crate::array::ArrayKey::Int(n),
                            Value::String(s) => crate::array::ArrayKey::String(s),
                            _ => return None,
                        };
                        arr.set(key, val);
                    } else {
                        arr.push(val);
                    }
                }
                Some(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr))))
            }
            ExprKind::UnaryOp { op: UnaryOp::Negate, operand, .. } => {
                match Self::eval_const_expr(operand)? {
                    Value::Long(n) => Some(Value::Long(-n)),
                    Value::Double(f) => Some(Value::Double(-f)),
                    _ => None,
                }
            }
            ExprKind::UnaryOp { op: UnaryOp::BitwiseNot, operand, .. } => {
                match Self::eval_const_expr(operand)? {
                    Value::Long(n) => Some(Value::Long(!n)),
                    _ => None,
                }
            }
            ExprKind::BinaryOp { left, op, right, .. } => {
                let l = Self::eval_const_expr(left)?;
                let r = Self::eval_const_expr(right)?;
                match (l, op, r) {
                    (Value::Long(a), BinaryOp::ShiftLeft, Value::Long(b)) => Some(Value::Long(a << b)),
                    (Value::Long(a), BinaryOp::ShiftRight, Value::Long(b)) => Some(Value::Long(a >> b)),
                    (Value::Long(a), BinaryOp::BitwiseAnd, Value::Long(b)) => Some(Value::Long(a & b)),
                    (Value::Long(a), BinaryOp::BitwiseOr, Value::Long(b)) => Some(Value::Long(a | b)),
                    (Value::Long(a), BinaryOp::BitwiseXor, Value::Long(b)) => Some(Value::Long(a ^ b)),
                    (Value::Long(a), BinaryOp::Add, Value::Long(b)) => Some(Value::Long(a.wrapping_add(b))),
                    (Value::Long(a), BinaryOp::Sub, Value::Long(b)) => Some(Value::Long(a.wrapping_sub(b))),
                    (Value::Long(a), BinaryOp::Mul, Value::Long(b)) => Some(Value::Long(a.wrapping_mul(b))),
                    (Value::Long(a), BinaryOp::Div, Value::Long(b)) if b != 0 => {
                        if a % b == 0 { Some(Value::Long(a / b)) } else { Some(Value::Double(a as f64 / b as f64)) }
                    }
                    (Value::Long(a), BinaryOp::Mod, Value::Long(b)) if b != 0 => Some(Value::Long(a % b)),
                    (Value::Long(a), BinaryOp::Pow, Value::Long(b)) if b >= 0 => Some(Value::Long(a.wrapping_pow(b as u32))),
                    (Value::Double(a), BinaryOp::Add, Value::Double(b)) => Some(Value::Double(a + b)),
                    (Value::Double(a), BinaryOp::Sub, Value::Double(b)) => Some(Value::Double(a - b)),
                    (Value::Double(a), BinaryOp::Mul, Value::Double(b)) => Some(Value::Double(a * b)),
                    (Value::Double(a), BinaryOp::Div, Value::Double(b)) if b != 0.0 => Some(Value::Double(a / b)),
                    (Value::Long(a), BinaryOp::Add, Value::Double(b)) => Some(Value::Double(a as f64 + b)),
                    (Value::Double(a), BinaryOp::Add, Value::Long(b)) => Some(Value::Double(a + b as f64)),
                    (Value::String(a), BinaryOp::Concat, Value::String(b)) => {
                        let mut result = a.as_bytes().to_vec();
                        result.extend_from_slice(b.as_bytes());
                        Some(Value::String(PhpString::from_vec(result)))
                    }
                    // String concatenation with non-string right side
                    (Value::String(a), BinaryOp::Concat, rv) => {
                        let mut result = a.as_bytes().to_vec();
                        result.extend_from_slice(rv.to_php_string().as_bytes());
                        Some(Value::String(PhpString::from_vec(result)))
                    }
                    // Boolean/logical operators - always return bool
                    (l, BinaryOp::BooleanAnd, r) | (l, BinaryOp::LogicalAnd, r) => {
                        Some(if l.is_truthy() && r.is_truthy() { Value::True } else { Value::False })
                    }
                    (l, BinaryOp::BooleanOr, r) | (l, BinaryOp::LogicalOr, r) => {
                        Some(if l.is_truthy() || r.is_truthy() { Value::True } else { Value::False })
                    }
                    (l, BinaryOp::LogicalXor, r) => {
                        Some(if l.is_truthy() ^ r.is_truthy() { Value::True } else { Value::False })
                    }
                    // Comparison operators
                    (Value::Long(a), BinaryOp::Less, Value::Long(b)) => Some(if a < b { Value::True } else { Value::False }),
                    (Value::Long(a), BinaryOp::Greater, Value::Long(b)) => Some(if a > b { Value::True } else { Value::False }),
                    (Value::Long(a), BinaryOp::LessEqual, Value::Long(b)) => Some(if a <= b { Value::True } else { Value::False }),
                    (Value::Long(a), BinaryOp::GreaterEqual, Value::Long(b)) => Some(if a >= b { Value::True } else { Value::False }),
                    (Value::Long(a), BinaryOp::Equal, Value::Long(b)) => Some(if a == b { Value::True } else { Value::False }),
                    (Value::Long(a), BinaryOp::Identical, Value::Long(b)) => Some(if a == b { Value::True } else { Value::False }),
                    (Value::Long(a), BinaryOp::NotEqual, Value::Long(b)) => Some(if a != b { Value::True } else { Value::False }),
                    (Value::Long(a), BinaryOp::NotIdentical, Value::Long(b)) => Some(if a != b { Value::True } else { Value::False }),
                    // Equal/NotEqual with string comparison (loose)
                    (Value::String(a), BinaryOp::Equal, Value::String(b)) => Some(if a.as_bytes() == b.as_bytes() { Value::True } else { Value::False }),
                    (Value::String(a), BinaryOp::NotEqual, Value::String(b)) => Some(if a.as_bytes() != b.as_bytes() { Value::True } else { Value::False }),
                    (Value::Long(a), BinaryOp::Equal, Value::String(b)) | (Value::String(b), BinaryOp::Equal, Value::Long(a)) => {
                        let s = std::str::from_utf8(b.as_bytes()).unwrap_or("");
                        Some(if s.parse::<i64>().ok() == Some(a) { Value::True } else { Value::False })
                    }
                    (Value::Long(a), BinaryOp::NotEqual, Value::String(b)) | (Value::String(b), BinaryOp::NotEqual, Value::Long(a)) => {
                        let s = std::str::from_utf8(b.as_bytes()).unwrap_or("");
                        Some(if s.parse::<i64>().ok() != Some(a) { Value::True } else { Value::False })
                    }
                    _ => None,
                }
            }
            // Ternary operator
            ExprKind::Ternary { condition, if_true, if_false } => {
                let cond = Self::eval_const_expr(condition)?;
                if cond.is_truthy() {
                    if let Some(true_expr) = if_true {
                        Self::eval_const_expr(true_expr)
                    } else {
                        // Short ternary: $a ?: $b - return the condition value itself
                        Some(cond)
                    }
                } else {
                    Self::eval_const_expr(if_false)
                }
            }
            // UnaryOp::BooleanNot (!)
            ExprKind::UnaryOp { op: UnaryOp::BooleanNot, operand, .. } => {
                let val = Self::eval_const_expr(operand)?;
                Some(if val.is_truthy() { Value::False } else { Value::True })
            }
            // UnaryOp::Plus (+)
            ExprKind::UnaryOp { op: UnaryOp::Plus, operand, .. } => {
                Self::eval_const_expr(operand)
            }
            // Handle Identifier constants (TRUE, FALSE, NULL, PHP_INT_MAX, etc.)
            ExprKind::Identifier(name) => {
                let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
                match lower.as_slice() {
                    b"true" => Some(Value::True),
                    b"false" => Some(Value::False),
                    b"null" => Some(Value::Null),
                    b"php_eol" => Some(Value::String(PhpString::from_bytes(b"\n"))),
                    b"php_int_max" => Some(Value::Long(i64::MAX)),
                    b"php_int_min" => Some(Value::Long(i64::MIN)),
                    b"php_int_size" => Some(Value::Long(8)),
                    b"php_major_version" => Some(Value::Long(8)),
                    b"php_minor_version" => Some(Value::Long(5)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Try to resolve an expression as a class constant reference.
    /// This handles `self::CONST`, `ClassName::CONST` etc. during class compilation.
    fn eval_class_const_expr(expr: &Expr, class: &ClassEntry, qualified_name: &[u8], extends: Option<&[u8]>, compiler: &Compiler) -> Option<Value> {
        match &expr.kind {
            ExprKind::ClassConstAccess { class: class_expr, constant } => {
                let target_class = match &class_expr.kind {
                    ExprKind::Identifier(name) => {
                        let resolved = compiler.resolve_class_name(name);
                        let lower: Vec<u8> = resolved.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if lower == b"self" || lower == b"static" {
                            qualified_name.to_vec()
                        } else if lower == b"parent" {
                            extends.map(|p| compiler.resolve_class_name(p)).unwrap_or(resolved)
                        } else {
                            resolved
                        }
                    }
                    _ => return None,
                };
                // Check if this references the same class
                let target_lower: Vec<u8> = target_class.iter().map(|b| b.to_ascii_lowercase()).collect();
                let self_lower: Vec<u8> = qualified_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                if target_lower == self_lower {
                    // Look up the constant in the class being compiled
                    if let Some(val) = class.constants.get(constant) {
                        // Make sure the value isn't a deferred marker
                        if let Value::String(s) = val {
                            if s.as_bytes().starts_with(b"__deferred_const__::") {
                                return None;
                            }
                        }
                        return Some(val.clone());
                    }
                }
                None
            }
            _ => None,
        }
    }
}

/// Convert an AST TypeHint into a runtime ParamType.
/// For built-in types (int, string, etc.), the name is stored lowercase.
/// For class names, the original case is preserved for error messages.
fn type_hint_to_param_type(hint: &TypeHint) -> ParamType {
    type_hint_to_param_type_with_ns(hint, &[], &HashMap::new())
}

/// Convert an AST TypeHint into a runtime ParamType with namespace resolution.
fn type_hint_to_param_type_with_ns(hint: &TypeHint, namespace: &[u8], use_map: &HashMap<Vec<u8>, Vec<u8>>) -> ParamType {
    match hint {
        TypeHint::Simple(name) => {
            let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
            // Check if it's a built-in type; if so, store lowercase
            match lower.as_slice() {
                b"int" | b"integer" | b"float" | b"double" | b"string" | b"bool" | b"boolean"
                | b"array" | b"object" | b"callable" | b"iterable" | b"mixed" | b"null"
                | b"void" | b"self" | b"parent" | b"static" | b"false" | b"true" | b"never" => {
                    ParamType::Simple(lower)
                }
                _ => {
                    // Class name: resolve through namespace
                    // Check for fully qualified (starts with \)
                    if name.starts_with(b"\\") {
                        return ParamType::Simple(name[1..].to_vec());
                    }
                    // Check use map
                    if let Some(pos) = name.iter().position(|&b| b == b'\\') {
                        let first_part = &name[..pos];
                        let first_lower: Vec<u8> = first_part.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if let Some(resolved) = use_map.get(&first_lower) {
                            let mut result = resolved.clone();
                            result.extend_from_slice(&name[pos..]);
                            return ParamType::Simple(result);
                        }
                        // Qualified but no use match: prefix with namespace
                        if namespace.is_empty() {
                            ParamType::Simple(name.clone())
                        } else {
                            let mut result = namespace.to_vec();
                            result.push(b'\\');
                            result.extend_from_slice(name);
                            ParamType::Simple(result)
                        }
                    } else {
                        // Unqualified: check use map
                        let name_lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if let Some(resolved) = use_map.get(&name_lower) {
                            return ParamType::Simple(resolved.clone());
                        }
                        // Prefix with namespace
                        if namespace.is_empty() {
                            ParamType::Simple(name.clone())
                        } else {
                            let mut result = namespace.to_vec();
                            result.push(b'\\');
                            result.extend_from_slice(name);
                            ParamType::Simple(result)
                        }
                    }
                }
            }
        }
        TypeHint::Nullable(inner) => ParamType::Nullable(Box::new(type_hint_to_param_type_with_ns(inner, namespace, use_map))),
        TypeHint::Union(types) => {
            ParamType::Union(types.iter().map(|t| type_hint_to_param_type_with_ns(t, namespace, use_map)).collect())
        }
        TypeHint::Intersection(types) => {
            ParamType::Intersection(types.iter().map(|t| type_hint_to_param_type_with_ns(t, namespace, use_map)).collect())
        }
    }
}

/// Check if a ParamType is already nullable or mixed (which implicitly allows null)
fn is_type_nullable_or_mixed(pt: &ParamType) -> bool {
    match pt {
        ParamType::Nullable(_) => true,
        ParamType::Simple(name) => {
            let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
            lower == b"mixed" || lower == b"null"
        }
        ParamType::Union(types) => {
            types.iter().any(|t| matches!(t, ParamType::Simple(n) if {
                let lower: Vec<u8> = n.iter().map(|b| b.to_ascii_lowercase()).collect();
                lower == b"null" || lower == b"mixed"
            }))
        }
        ParamType::Intersection(_) => false,
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
        ExprKind::Cast(_, e) | ExprKind::Clone(e) | ExprKind::Spread(e) | ExprKind::Print(e) | ExprKind::ThrowExpr(e) => {
            collect_expr_variables(e, vars);
        }
        ExprKind::PropertyAccess { object, .. } => {
            collect_expr_variables(object, vars);
        }
        ExprKind::Instanceof { expr, .. } => collect_expr_variables(expr, vars),
        ExprKind::ArrowFunction { body, .. } => collect_expr_variables(body, vars),
        ExprKind::Closure { use_vars, .. } => {
            // Don't recurse into closures body, but DO collect use vars
            // so that enclosing arrow functions know to capture them
            for uv in use_vars {
                if !vars.contains(&uv.variable) {
                    vars.push(uv.variable.clone());
                }
            }
        }
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
        ExprKind::Clone(e) | ExprKind::Spread(e) | ExprKind::Print(e) | ExprKind::ThrowExpr(e) => expr_contains_yield(e),
        ExprKind::PropertyAccess { object, .. } => expr_contains_yield(object),
        ExprKind::Include { path, .. } => expr_contains_yield(path),
        _ => false,
    }
}
