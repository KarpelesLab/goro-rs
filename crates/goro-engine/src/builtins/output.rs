//! Output control functions (PHP manual: Output Control) and flush().

use crate::class::{DisplayName, TypeName};
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

fn handler_name(vm: &mut Vm, cb: &Value) -> PhpStr {
    match cb {
        Value::Null => PhpStr::from("default output handler"),
        Value::Object(o) if vm.closure_data(o).is_some() => PhpStr::from("Closure::__invoke"),
        other => vm.callable_name(other),
    }
}

fn ob_start(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cb = a.val(0);
    if !cb.is_null() && !vm.is_callable_value(&cb, false) {
        let name = vm.callable_name(&cb);
        vm.warn(&format!(
            "ob_start(): {} \"{}\" not found or invalid function name",
            if matches!(cb, Value::Array(_)) {
                "class"
            } else {
                "function"
            },
            name.to_string_lossy()
        ))?;
        vm.notice_msg("ob_start(): Failed to create buffer")?;
        return Ok(Value::Bool(false));
    }
    if vm.out.buffers.iter().any(|b| b.in_handler) {
        return vm.throw(
            "Error",
            "ob_start(): Cannot use output buffering in output buffering display handlers",
        );
    }
    let chunk = a.int_or(1, 0).max(0) as usize;
    let flags = a.int_or(2, crate::vm::output::PHP_OUTPUT_HANDLER_STDFLAGS);
    let name = handler_name(vm, &cb);
    vm.out.buffers.push(ObBuffer {
        data: Vec::new(),
        callback: if cb.is_null() { None } else { Some(cb) },
        chunk_size: if chunk == 1 { 4096 } else { chunk },
        flags,
        name,
        started: false,
        in_handler: false,
    });
    Ok(Value::Bool(true))
}

fn no_buffer(vm: &mut Vm, what: &str, verb: &str) -> R<Value> {
    vm.notice_msg(&format!(
        "{}(): Failed to {} buffer. No buffer to {}",
        what, verb, verb
    ))?;
    Ok(Value::Bool(false))
}

fn check_flag(vm: &mut Vm, what: &str, verb: &str, flag: i64) -> R<bool> {
    let b = vm.out.buffers.last().unwrap();
    if b.flags & flag == 0 {
        let n = b.name.dname();
        vm.notice_msg(&format!(
            "{}(): Failed to {} buffer of {} (0)",
            what, verb, n
        ))?;
        return Ok(false);
    }
    Ok(true)
}

fn ob_get_clean(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if vm.out.buffers.is_empty() {
        return Ok(Value::Bool(false));
    }
    let data = vm.out.buffers.last().unwrap().data.clone();
    if !check_flag(
        vm,
        "ob_get_clean",
        "discard",
        crate::vm::output::PHP_OUTPUT_HANDLER_REMOVABLE,
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
    let data = vm.out.buffers.last().unwrap().data.clone();
    vm.ob_end_flush()?;
    Ok(Value::Str(PhpStr::from(data)))
}

fn ob_end_clean(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if vm.out.buffers.is_empty() {
        return no_buffer(vm, "ob_end_clean", "delete");
    }
    if !check_flag(
        vm,
        "ob_end_clean",
        "discard",
        crate::vm::output::PHP_OUTPUT_HANDLER_REMOVABLE,
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
    if !check_flag(
        vm,
        "ob_end_flush",
        "send",
        crate::vm::output::PHP_OUTPUT_HANDLER_REMOVABLE,
    )? {
        return Ok(Value::Bool(false));
    }
    vm.ob_end_flush()?;
    Ok(Value::Bool(true))
}

fn ob_clean(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if vm.out.buffers.is_empty() {
        return no_buffer(vm, "ob_clean", "delete");
    }
    if !check_flag(
        vm,
        "ob_clean",
        "delete",
        crate::vm::output::PHP_OUTPUT_HANDLER_CLEANABLE,
    )? {
        return Ok(Value::Bool(false));
    }
    vm.ob_clean_top(false)?;
    Ok(Value::Bool(true))
}

fn ob_flush(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    if vm.out.buffers.is_empty() {
        return no_buffer(vm, "ob_flush", "flush");
    }
    if !check_flag(
        vm,
        "ob_flush",
        "flush",
        crate::vm::output::PHP_OUTPUT_HANDLER_FLUSHABLE,
    )? {
        return Ok(Value::Bool(false));
    }
    vm.ob_flush_top(crate::vm::output::PHP_OUTPUT_HANDLER_FLUSH)?;
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
    let mut flags = b.flags;
    if b.started {
        flags |= 0x1000;
    }
    a.insert(Key::from("flags"), Value::Int(flags));
    a.insert(Key::from("level"), Value::Int(level as i64));
    a.insert(Key::from("chunk_size"), Value::Int(b.chunk_size as i64));
    a.insert(
        Key::from("buffer_size"),
        Value::Int(if b.chunk_size > 0 {
            b.chunk_size.max(16384) as i64
        } else {
            16384
        }),
    );
    a.insert(Key::from("buffer_used"), Value::Int(b.data.len() as i64));
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
