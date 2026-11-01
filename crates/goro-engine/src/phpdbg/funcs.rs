//! The phpdbg_* functions (PHP manual: phpdbg functions).

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::commands::{add_bp, bp_file, symbol_bp};
use super::{BpKind, with};
use crate::bytecode::{FuncCode, Op};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static PHPDBG_MODULE: Module = Module {
    name: "phpdbg",
    functions: &[
        ("function phpdbg_break_next(): void", break_next),
        (
            "function phpdbg_break_file(string $file, int $line): void",
            break_file,
        ),
        (
            "function phpdbg_break_method(string $class, string $method): void",
            break_method,
        ),
        (
            "function phpdbg_break_function(string $function): void",
            break_function,
        ),
        ("function phpdbg_clear(): void", clear),
        (
            "function phpdbg_color(int $element, string $color): void",
            nothing,
        ),
        ("function phpdbg_prompt(string $string): void", prompt),
        ("function phpdbg_exec(string $context): string|bool", exec),
        ("function phpdbg_start_oplog(): void", start_oplog),
        (
            "function phpdbg_end_oplog(array $options = []): ?array",
            end_oplog,
        ),
        (
            "function phpdbg_get_executable(array $options = []): array",
            get_executable,
        ),
    ],
    constants: &[
        ("PHPDBG_VERSION", ConstVal::Str("0.5.0")),
        ("PHPDBG_FILE", ConstVal::Int(1)),
        ("PHPDBG_METHOD", ConstVal::Int(2)),
        ("PHPDBG_LINENO", ConstVal::Int(3)),
        ("PHPDBG_FUNC", ConstVal::Int(4)),
        ("PHPDBG_COLOR_PROMPT", ConstVal::Int(1)),
        ("PHPDBG_COLOR_NOTICE", ConstVal::Int(2)),
        ("PHPDBG_COLOR_ERROR", ConstVal::Int(3)),
    ],
    ..Module::EMPTY
};

fn nothing(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Null)
}

/// A breakpoint at the next statement (test
/// sapi/phpdbg/tests/phpdbg_break_next.phpt).
fn break_next(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let addr = vm.ex.top.ops as usize + vm.ex.ip() * std::mem::size_of::<Op>();
    add_bp(vm, BpKind::Next { addr, done: false });
    Ok(Value::Null)
}

fn break_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0).to_string_lossy();
    let line = a.int(1).max(0) as u32;
    add_bp(
        vm,
        BpKind::Line {
            file: bp_file(&f),
            line,
        },
    );
    Ok(Value::Null)
}

fn break_method(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = format!(
        "{}::{}",
        a.str(0).to_string_lossy(),
        a.str(1).to_string_lossy()
    );
    add_bp(vm, symbol_bp(&s));
    Ok(Value::Null)
}

fn break_function(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0).to_string_lossy();
    add_bp(vm, symbol_bp(&s));
    Ok(Value::Null)
}

fn clear(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    with(|d| d.bps.clear());
    Ok(Value::Null)
}

fn prompt(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = a.str(0).to_string_lossy();
    with(|d| d.prompt = format!("{} ", p.trim_end()));
    Ok(Value::Null)
}

fn exec(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    use std::os::unix::ffi::OsStrExt;
    let p = a.str(0);
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(&p));
    if !path.is_file() {
        return Ok(Value::Bool(false));
    }
    let abs = std::fs::canonicalize(path)
        .map(|a| a.as_os_str().as_bytes().to_vec())
        .unwrap_or_else(|_| p.to_vec());
    let prev = with(|d| d.script.as_ref().map(|s| s.path.clone()));
    if prev.as_deref() == Some(&abs[..]) {
        return Ok(Value::Bool(true));
    }
    let src = std::rc::Rc::new(std::fs::read(path).unwrap_or_default());
    with(|d| {
        d.script = Some(super::Script {
            path: abs.clone(),
            src,
        })
    });
    Ok(match prev {
        Some(p) => Value::Str(PhpStr::from(p)),
        None => Value::Bool(true),
    })
}

fn start_oplog(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    with(|d| d.oplog = true);
    Ok(Value::Null)
}

/// Ends the oplog. The per-opline counts need Zend's opcodes: the log is
/// returned empty.
fn end_oplog(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if !with(|d| std::mem::take(&mut d.oplog)) {
        // Test sapi/phpdbg/tests/phpdbg_oplog_002.phpt.
        vm.warn("Cannot end an oplog without starting it")?;
        return Ok(Value::Null);
    }
    Ok(Value::Array(PhpArray::new()))
}

/// The lines of a function's statements.
fn statement_lines(code: &FuncCode, out: &mut Vec<u32>) {
    for (i, op) in code.ops.iter().enumerate() {
        if !matches!(op, Op::Ticks) {
            continue;
        }
        let next = code.ops.get(i + 1);
        if matches!(
            next,
            Some(Op::Return { .. } | Op::ReturnRef { .. } | Op::GeneratorReturn { .. })
        ) {
            continue;
        }
        let l = code.line_of(i);
        if !out.contains(&l) {
            out.push(l);
        }
    }
}

/// The executable lines of the user functions and methods, by file
/// (test sapi/phpdbg/tests/phpdbg_get_executable_stream_wrapper.phpt).
fn get_executable(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut files: Vec<(PhpStr, Vec<u32>)> = Vec::new();
    let mut add = |code: &FuncCode| {
        if code.file.is_empty() {
            return;
        }
        let i = match files.iter().position(|(f, _)| *f == code.file) {
            Some(i) => i,
            None => {
                files.push((code.file.clone(), Vec::new()));
                files.len() - 1
            }
        };
        statement_lines(code, &mut files[i].1);
    };
    for n in &vm.user_functions {
        if let Some(c) = vm.functions.get(n).and_then(|f| f.code()) {
            add(c);
        }
    }
    for n in &vm.class_order {
        if let Some(c) = vm.classes.get(n)
            && c.decl.is_some()
        {
            for m in c.methods.borrow().values() {
                if std::ptr::eq(m.scope.get(), &**c)
                    && let Some(code) = m.code()
                {
                    add(code);
                }
            }
        }
    }
    let mut out = PhpArray::new();
    for (f, mut lines) in files {
        lines.sort_unstable();
        let mut a = PhpArray::new();
        for l in lines {
            a.insert(Key::Int(l as i64), Value::Int(0));
        }
        out.insert(Key::Str(f), Value::Array(a));
    }
    Ok(Value::Array(out))
}
