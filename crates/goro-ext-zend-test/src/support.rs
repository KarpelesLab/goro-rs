//! Engine services the extension's functions need (the zend_API calls of
//! test.c), built on the VM's public interface.

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

const E_PARSE: i64 = 4;

/// instanceof_function(): `v` is an object of class `class` (or of a
/// subclass / implementing class), user or internal.
pub(crate) fn instance_of(vm: &Vm, v: &Value, class: &[u8]) -> bool {
    let target = class
        .strip_prefix(b"\\")
        .unwrap_or(class)
        .to_ascii_lowercase();
    match v {
        Value::Object(o) => is_a(vm, &o.borrow().class_name.to_ascii_lowercase(), &target, 0),
        Value::Generator(_) => matches!(
            target.as_slice(),
            b"generator" | b"iterator" | b"traversable"
        ),
        _ => false,
    }
}

fn is_a(vm: &Vm, class_lower: &[u8], target: &[u8], depth: usize) -> bool {
    if class_lower == target {
        return true;
    }
    if depth > 64 {
        return false;
    }
    match vm.classes.get(class_lower) {
        Some(ce) => {
            ce.parent
                .as_ref()
                .is_some_and(|p| is_a(vm, &p.to_ascii_lowercase(), target, depth + 1))
                || ce
                    .interfaces
                    .iter()
                    .any(|i| is_a(vm, &i.to_ascii_lowercase(), target, depth + 1))
        }
        None => goro_core::stubs::stub_instanceof(class_lower, target).unwrap_or(false),
    }
}

/// zend_lookup_class() with autoloading.
pub(crate) fn class_exists_autoload(vm: &mut Vm, name: &[u8]) -> bool {
    let name = name.strip_prefix(b"\\").unwrap_or(name);
    let lower = name.to_ascii_lowercase();
    if vm.classes.contains_key(&lower) || vm.is_known_builtin_class(&lower) {
        return true;
    }
    vm.try_autoload_class(name);
    vm.classes.contains_key(&lower) || vm.is_known_builtin_class(&lower)
}

/// Run a PHP snippet (no `<?php`) in the calling scope, reporting at the
/// caller's line. `vars` are its local variables. Ok(None) when it threw.
pub(crate) fn run_snippet(
    vm: &mut Vm,
    src: &str,
    vars: &[(&str, Value)],
) -> Result<Option<Value>, VmError> {
    let source = format!("<?php {}", src).into_bytes();
    let mut lexer = goro_parser::Lexer::new(&source);
    let tokens = lexer.tokenize();
    let mut parser = goro_parser::Parser::new(tokens);
    let Ok(program) = parser.parse() else {
        return Ok(None);
    };
    let compiler = goro_core::compiler::Compiler::new();
    let Ok((mut op_array, _, _)) = compiler.compile(&program) else {
        return Ok(None);
    };
    let line = vm.current_line;
    for op in op_array.ops.iter_mut() {
        op.line = line;
    }
    let mut cvs = vec![Value::Undef; op_array.cv_names.len()];
    for (name, val) in vars {
        if let Some(i) = op_array
            .cv_names
            .iter()
            .position(|n| n.as_slice() == name.as_bytes())
        {
            cvs[i] = val.clone();
        }
    }
    let r = vm.execute_op_array_pub(&op_array, cvs);
    if vm.current_exception.is_some() {
        return Ok(None);
    }
    r.map(Some)
}

/// get_function_or_method_name() of the frame calling the builtin:
/// "main" for top-level code of a script, an included file or eval().
pub(crate) fn current_function_name(vm: &Vm) -> String {
    for f in vm.call_stack.iter().rev() {
        let name = f.0.as_str();
        if name.eq_ignore_ascii_case("zend_get_current_func_name") {
            continue;
        }
        return match name {
            "include" | "include_once" | "require" | "require_once" | "eval" => "main".to_string(),
            n => n.replace("->", "::"),
        };
    }
    "main".to_string()
}

/// zend_call_method(): call `method` on `this` (an object) or on class
/// `class_lower` without an object.
pub(crate) fn call_method(
    vm: &mut Vm,
    this: &Value,
    class_lower: Option<&[u8]>,
    method: &[u8],
    args: &[Value],
) -> Result<Value, VmError> {
    let method_lower = method.to_ascii_lowercase();
    if let Value::Object(o) = this {
        let mut cb = goro_core::array::PhpArray::new();
        cb.push(Value::Object(o.clone()));
        cb.push(Value::String(PhpString::from_vec(method.to_vec())));
        let cb = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(cb)));
        return vm.call_callback(&cb, args);
    }
    let Some(class_lower) = class_lower else {
        return Ok(Value::Null);
    };
    let found = vm.classes.get(class_lower).and_then(|ce| {
        ce.get_method(&method_lower)
            .map(|m| (ce.name.clone(), m.clone()))
    });
    match found {
        // A non-static method called without an object: runs with the
        // class as its called scope (zend_call_known_function()).
        Some((class_name, m)) if !m.is_static => {
            let op = m.op_array.clone();
            let mut cvs = vec![Value::Undef; op.cv_names.len()];
            for (i, a) in args.iter().enumerate() {
                if let Some(slot) = cvs.get_mut(i + 1) {
                    *slot = a.clone();
                }
            }
            let display = format!(
                "{}::{}",
                String::from_utf8_lossy(&class_name),
                String::from_utf8_lossy(&m.name)
            );
            vm.call_stack.push((
                display,
                vm.current_file.clone(),
                vm.current_line,
                args.to_vec(),
                false,
            ));
            vm.called_class_stack.push(class_lower.to_vec());
            vm.push_class_scope(m.declaring_class.clone());
            let r = vm.execute_op_array_pub(&op, cvs);
            vm.pop_class_scope();
            vm.called_class_stack.pop();
            vm.call_stack.pop();
            r
        }
        _ => {
            let class_name = vm
                .classes
                .get(class_lower)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| class_lower.to_vec());
            let mut a = goro_core::array::PhpArray::new();
            for v in args {
                a.push(v.clone());
            }
            let r = run_snippet(
                vm,
                "return $c::$m(...$a);",
                &[
                    ("c", Value::String(PhpString::from_vec(class_name))),
                    ("m", Value::String(PhpString::from_vec(method.to_vec()))),
                    (
                        "a",
                        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(a))),
                    ),
                ],
            )?;
            Ok(r.unwrap_or(Value::Null))
        }
    }
}

/// compile_string() + zend_execute(): `source` as a script named `file`.
/// A parse error is fatal (bailout).
pub(crate) fn compile_and_run(
    vm: &mut Vm,
    source: &[u8],
    file: &str,
    skip_shebang: bool,
) -> Result<(), VmError> {
    let mut lexer = goro_parser::Lexer::new(source);
    if skip_shebang {
        lexer.skip_shebang();
    }
    let tokens = lexer.tokenize();
    let _ = goro_parser::take_lexer_deprecations();
    let _ = goro_parser::take_lexer_warnings();
    let mut parser = goro_parser::Parser::new(tokens).with_source(source);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(e) => {
            let msg = if goro_parser::is_php_parse_error(&e.message) {
                e.message.clone()
            } else {
                "syntax error".to_string()
            };
            vm.fatal_error_cb(E_PARSE, &msg, file, e.span.line);
            vm.fatal_emitted = true;
            return Err(VmError {
                message: msg,
                line: e.span.line,
            });
        }
    };
    let mut compiler = goro_core::compiler::Compiler::new();
    compiler.source_file = file.as_bytes().to_vec();
    let (op_array, classes, warnings) = match compiler.compile(&program) {
        Ok(r) => r,
        Err(e) => {
            let _ = goro_core::compiler::take_pending_compile_warnings();
            vm.fatal_error_cb(1, &e.message, file, e.line);
            vm.fatal_emitted = true;
            return Err(VmError {
                message: e.message,
                line: e.line,
            });
        }
    };
    let _ = goro_core::compiler::take_pending_compile_warnings();
    // The builtin's frame is located in its caller's file.
    vm.sync_builtin_frames();
    let saved_file = std::mem::replace(&mut vm.current_file, file.to_string());
    let saved_line = vm.current_line;
    for (msg, line) in &warnings {
        vm.emit_warning_at(msg, *line);
    }
    let r = vm.run_compiled(&op_array, classes);
    vm.current_file = saved_file;
    vm.current_line = saved_line;
    r.map(|_| ())
}
