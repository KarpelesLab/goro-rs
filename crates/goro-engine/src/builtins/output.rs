//! Output control functions (PHP manual: Output Control) and flush().

use crate::class::DisplayName;
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::func::Args;
use crate::registry::Module;
use crate::vm::output::ObBuffer;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "output",
    functions: &[
        (
            "function ob_start($callback = null, int $chunk_size = 0, int $flags = PHP_OUTPUT_HANDLER_STDFLAGS): bool",
            ob_start,
        ),
        ("function ob_get_clean(): string|false", ob_get_clean),
        ("function ob_get_contents(): string|false", ob_get_contents),
        ("function ob_get_flush(): string|false", ob_get_flush),
        ("function ob_end_clean(): bool", ob_end_clean),
        ("function ob_end_flush(): bool", ob_end_flush),
        ("function ob_clean(): bool", ob_clean),
        ("function ob_flush(): bool", ob_flush),
        ("function ob_get_level(): int", ob_get_level),
        ("function ob_get_length(): int|false", ob_get_length),
        (
            "function ob_get_status(bool $full_status = false): array",
            ob_get_status,
        ),
        ("function ob_list_handlers(): array", ob_list_handlers),
        (
            "function ob_implicit_flush(bool $enable = true): void",
            ob_implicit_flush,
        ),
        (
            "function output_add_rewrite_var(string $name, string $value): bool",
            ret_true,
        ),
        ("function output_reset_rewrite_vars(): bool", ret_true),
        ("function flush(): void", flush),
    ],
    listed: false,
    ..Module::EMPTY
};

/// The output_handler setting: a handler started before the script runs
/// (PHP manual: output_handler, "Redirect all the output of your scripts
/// to a function").
pub fn start_ini_handler(vm: &mut Vm) {
    let Some(h) = vm.get_ini(b"output_handler") else {
        return;
    };
    if h.is_empty() {
        return;
    }
    let cb = Value::Str(h);
    let name = handler_name(vm, &cb);
    vm.out.buffers.push(ObBuffer::new(
        Some(cb),
        0,
        crate::vm::output::PHP_OUTPUT_HANDLER_STDFLAGS,
        name,
    ));
}

fn handler_name(vm: &mut Vm, cb: &Value) -> PhpStr {
    match cb {
        Value::Null => PhpStr::from("default output handler"),
        Value::Object(o) if vm.closure_data(o).is_some() => {
            let c = vm.closure_data(o).unwrap();
            PhpStr::from(vm.func_display_name(&c.func))
        }
        other => vm.callable_name(other),
    }
}

/// Output functions other than the read-only ones are fatal inside a
/// handler (tests tests/output/ob_011.phpt, ob_start_error_005.phpt).
fn check_not_in_handler(vm: &mut Vm, what: &str) -> R<()> {
    if vm.out.handler_level().is_some() {
        // Output buffering shuts down: buffered output is lost and later
        // ob_start() calls fail (tests tests/output/gh20837.phpt,
        // gh20352.phpt).
        vm.out.deactivated = true;
        let levels = std::mem::take(&mut vm.out.buffers);
        let r = vm.fatal(&format!(
            "{}(): Cannot use output buffering in output buffering display handlers",
            what
        ));
        drop(levels);
        return Err(r);
    }
    Ok(())
}

fn ob_start(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cb = a.val(0);
    check_not_in_handler(vm, "ob_start")?;
    if vm.out.deactivated {
        vm.notice_msg("ob_start(): Failed to create buffer")?;
        return Ok(Value::Bool(false));
    }
    if !cb.is_null() && !vm.is_callable_value(&cb, false) {
        let why = vm.callable_error(&cb);
        vm.warn(&format!("ob_start(): {}", why))?;
        vm.notice_msg("ob_start(): Failed to create buffer")?;
        return Ok(Value::Bool(false));
    }
    // Since PHP 5.4 a chunk size of 1 is no longer special (PHP manual:
    // ob_start changelog).
    let chunk = a.int_or(1, 0).max(0) as usize;
    let flags = a.int_or(2, crate::vm::output::PHP_OUTPUT_HANDLER_STDFLAGS);
    let name = handler_name(vm, &cb);
    vm.out.buffers.push(ObBuffer::new(
        if cb.is_null() { None } else { Some(cb) },
        chunk,
        flags,
        name,
    ));
    Ok(Value::Bool(true))
}

fn no_buffer(vm: &mut Vm, what: &str, verb: &str) -> R<Value> {
    vm.notice_msg(&format!(
        "{}(): Failed to {} buffer. No buffer to {}",
        what, verb, verb
    ))?;
    Ok(Value::Bool(false))
}

/// Whether the top buffer allows the operation; otherwise the notices
/// PHP gives (one per refused step), as observed on PHP 8.5.
fn check_flag(vm: &mut Vm, what: &str, verbs: &[&str], flag: i64) -> R<bool> {
    let level = vm.out.buffers.len() - 1;
    let b = vm.out.buffers.last().unwrap();
    if b.flags & flag == 0 {
        let n = b.name.dname();
        for verb in verbs {
            vm.notice_msg(&format!(
                "{}(): Failed to {} buffer of {} ({})",
                what, verb, n, level
            ))?;
        }
        return Ok(false);
    }
    Ok(true)
}

use crate::vm::output::{
    PHP_OUTPUT_HANDLER_CLEANABLE, PHP_OUTPUT_HANDLER_FLUSH, PHP_OUTPUT_HANDLER_FLUSHABLE,
    PHP_OUTPUT_HANDLER_REMOVABLE,
};

fn ob_get_clean(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if vm.out.buffers.is_empty() {
        return Ok(Value::Bool(false));
    }
    check_not_in_handler(vm, "ob_get_clean")?;
    let data = vm.out.buffers.last().unwrap().data.clone();
    if !check_flag(
        vm,
        "ob_get_clean",
        &["discard", "delete"],
        PHP_OUTPUT_HANDLER_REMOVABLE,
    )? {
        return Ok(Value::Str(PhpStr::from(data)));
    }
    vm.ob_clean_top(true)?;
    Ok(Value::Str(PhpStr::from(data)))
}

fn ob_get_contents(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(match vm.out.buffers.last() {
        Some(b) => Value::Str(PhpStr::from(b.data.clone())),
        None => Value::Bool(false),
    })
}

fn ob_get_flush(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if vm.out.buffers.is_empty() {
        vm.notice_msg(
            "ob_get_flush(): Failed to delete and flush buffer. No buffer to delete or flush",
        )?;
        return Ok(Value::Bool(false));
    }
    check_not_in_handler(vm, "ob_get_flush")?;
    let data = vm.out.buffers.last().unwrap().data.clone();
    if !check_flag(
        vm,
        "ob_get_flush",
        &["send", "delete"],
        PHP_OUTPUT_HANDLER_REMOVABLE,
    )? {
        return Ok(Value::Str(PhpStr::from(data)));
    }
    vm.ob_end_flush()?;
    Ok(Value::Str(PhpStr::from(data)))
}

fn ob_end_clean(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if vm.out.buffers.is_empty() {
        return no_buffer(vm, "ob_end_clean", "delete");
    }
    check_not_in_handler(vm, "ob_end_clean")?;
    if !check_flag(
        vm,
        "ob_end_clean",
        &["discard"],
        PHP_OUTPUT_HANDLER_REMOVABLE,
    )? {
        return Ok(Value::Bool(false));
    }
    vm.ob_clean_top(true)?;
    Ok(Value::Bool(true))
}

fn ob_end_flush(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if vm.out.buffers.is_empty() {
        vm.notice_msg(
            "ob_end_flush(): Failed to delete and flush buffer. No buffer to delete or flush",
        )?;
        return Ok(Value::Bool(false));
    }
    check_not_in_handler(vm, "ob_end_flush")?;
    if !check_flag(vm, "ob_end_flush", &["send"], PHP_OUTPUT_HANDLER_REMOVABLE)? {
        return Ok(Value::Bool(false));
    }
    vm.ob_end_flush()?;
    Ok(Value::Bool(true))
}

fn ob_clean(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if vm.out.buffers.is_empty() {
        return no_buffer(vm, "ob_clean", "delete");
    }
    check_not_in_handler(vm, "ob_clean")?;
    if !check_flag(vm, "ob_clean", &["delete"], PHP_OUTPUT_HANDLER_CLEANABLE)? {
        return Ok(Value::Bool(false));
    }
    vm.ob_clean_top(false)?;
    Ok(Value::Bool(true))
}

fn ob_flush(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if vm.out.buffers.is_empty() {
        return no_buffer(vm, "ob_flush", "flush");
    }
    check_not_in_handler(vm, "ob_flush")?;
    if !check_flag(vm, "ob_flush", &["flush"], PHP_OUTPUT_HANDLER_FLUSHABLE)? {
        return Ok(Value::Bool(false));
    }
    vm.ob_flush_top(PHP_OUTPUT_HANDLER_FLUSH)?;
    Ok(Value::Bool(true))
}

fn ob_get_level(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(vm.out.buffers.len() as i64))
}

fn ob_get_length(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(match vm.out.buffers.last() {
        Some(b) => Value::Int(b.data.len() as i64),
        None => Value::Bool(false),
    })
}

fn status_of(b: &ObBuffer, level: usize) -> PhpArray {
    let mut a = PhpArray::new();
    a.insert(Key::from("name"), Value::Str(b.name.clone()));
    a.insert(
        Key::from("type"),
        Value::Int(if b.callback.is_some() { 1 } else { 0 }),
    );
    a.insert(Key::from("flags"), Value::Int(b.status_flags()));
    a.insert(Key::from("level"), Value::Int(level as i64));
    a.insert(Key::from("chunk_size"), Value::Int(b.chunk_size as i64));
    let (size, used) = if b.disabled {
        (0, 0)
    } else {
        (b.size as i64, b.data.len() as i64)
    };
    a.insert(Key::from("buffer_size"), Value::Int(size));
    a.insert(Key::from("buffer_used"), Value::Int(used));
    a
}

fn ob_get_status(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if a.bool_or(0, false) {
        let mut out = PhpArray::new();
        for (i, b) in vm.out.buffers.iter().enumerate() {
            let _ = out.push(Value::Array(status_of(b, i)));
        }
        return Ok(Value::Array(out));
    }
    Ok(Value::Array(match vm.out.buffers.last() {
        Some(b) => status_of(b, vm.out.buffers.len() - 1),
        None => PhpArray::new(),
    }))
}

fn ob_list_handlers(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::from_values(
        vm.out.buffers.iter().map(|b| Value::Str(b.name.clone())),
    )))
}

fn ob_implicit_flush(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.out.implicit_flush = a.bool_or(0, true);
    Ok(Value::Null)
}

fn ret_true(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(true))
}

fn flush(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if vm.out.buffers.is_empty() {
        vm.out.flush();
    }
    Ok(Value::Null)
}
