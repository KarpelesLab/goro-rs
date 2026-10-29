//! ext/readline (PHP manual: GNU Readline) without a terminal: the
//! line-editing library is emulated as the libedit build of PHP 8.5
//! behaves on a non-interactive input (observed: READLINE_LIB is
//! "libedit", no prompt is written when stdin is not a terminal,
//! readline_info()'s keys, the "_HiStOrY_V2_" history file).

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "readline",
    functions: &[
        ("function readline(?string $prompt = null): string|false", readline),
        (
            "function readline_info(?string $var_name = null, $value = null): mixed",
            readline_info,
        ),
        (
            "function readline_add_history(string $prompt): true",
            readline_add_history,
        ),
        ("function readline_clear_history(): true", readline_clear_history),
        ("function readline_list_history(): array", readline_list_history),
        (
            "function readline_read_history(?string $filename = null): bool",
            readline_read_history,
        ),
        (
            "function readline_write_history(?string $filename = null): bool",
            readline_write_history,
        ),
        (
            "function readline_completion_function(callable $callback): bool",
            readline_completion_function,
        ),
        (
            "function readline_callback_handler_install(string $prompt, callable $callback): true",
            readline_callback_handler_install,
        ),
        (
            "function readline_callback_read_char(): void",
            readline_callback_read_char,
        ),
        (
            "function readline_callback_handler_remove(): bool",
            readline_callback_handler_remove,
        ),
        ("function readline_redisplay(): void", noop),
        ("function readline_on_new_line(): void", noop),
    ],
    constants: &[("READLINE_LIB", ConstVal::Str("libedit"))],
    listed: true,
    ..Module::EMPTY
};

#[derive(Default)]
struct State {
    history: Vec<Vec<u8>>,
    line_buffer: Vec<u8>,
    readline_name: Vec<u8>,
    attempted_completion_over: i64,
    completion: Option<Value>,
    callback: Option<Value>,
    pending: Vec<u8>,
}

fn st(vm: &mut Vm) -> &mut State {
    vm.ext::<State>()
}

/// One line from standard input (without its newline); None at EOF.
fn read_line(vm: &mut Vm) -> Option<Vec<u8>> {
    vm.out.flush();
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    loop {
        // SAFETY: reading one byte into a valid buffer.
        let n = unsafe { libc::read(0, b.as_mut_ptr() as *mut libc::c_void, 1) };
        if n <= 0 {
            return if line.is_empty() { None } else { Some(line) };
        }
        if b[0] == b'\n' {
            return Some(line);
        }
        line.push(b[0]);
    }
}

fn readline(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let tty = unsafe { libc::isatty(0) } == 1;
    if tty && let Some(p) = a.opt_str(0) {
        vm.echo(&p);
    }
    Ok(match read_line(vm) {
        Some(l) => Value::Str(PhpStr::from(l)),
        None => Value::Bool(false),
    })
}

fn c_str(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(p) => &b[..p],
        None => b,
    }
}

/// readline_info(): libedit's variables; line_buffer, readline_name and
/// attempted_completion_over can be set (the old value is returned).
fn readline_info(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(name) = a.opt_str(0) else {
        let s = st(vm);
        let mut arr = PhpArray::new();
        let k = |n: &str| Key::Str(PhpStr::from(n));
        arr.insert(k("line_buffer"), Value::Str(PhpStr::from(s.line_buffer.clone())));
        arr.insert(k("point"), Value::Int(0));
        arr.insert(k("end"), Value::Int(0));
        arr.insert(k("erase_empty_line"), Value::Int(0));
        arr.insert(k("library_version"), Value::Str(PhpStr::from("EditLine wrapper")));
        arr.insert(
            k("readline_name"),
            Value::Str(PhpStr::from(s.readline_name.clone())),
        );
        arr.insert(
            k("attempted_completion_over"),
            Value::Int(s.attempted_completion_over),
        );
        return Ok(Value::Array(arr));
    };
    let set = a.has(1);
    let v = a.val(1);
    match name.as_bytes() {
        b"line_buffer" | b"readline_name" => {
            let new = if set {
                Some(c_str(&vm.to_php_str(&v)?).to_vec())
            } else {
                None
            };
            let s = st(vm);
            let slot = if name.as_bytes() == b"line_buffer" {
                &mut s.line_buffer
            } else {
                &mut s.readline_name
            };
            let old = slot.clone();
            if let Some(n) = new {
                *slot = n;
            }
            Ok(Value::Str(PhpStr::from(old)))
        }
        b"attempted_completion_over" => {
            let new = if set {
                Some(a.int(1))
            } else {
                None
            };
            let s = st(vm);
            let old = s.attempted_completion_over;
            if let Some(n) = new {
                s.attempted_completion_over = n;
            }
            Ok(Value::Int(old))
        }
        b"point" | b"end" | b"erase_empty_line" => Ok(Value::Int(0)),
        b"library_version" => Ok(Value::Str(PhpStr::from("EditLine wrapper"))),
        _ => Ok(Value::Null),
    }
}

fn readline_add_history(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let l = a.str(0);
    let l = c_str(&l).to_vec();
    st(_vm).history.push(l);
    Ok(Value::Bool(true))
}

fn readline_clear_history(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    st(vm).history.clear();
    Ok(Value::Bool(true))
}

fn readline_list_history(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let h: Vec<Value> = st(vm)
        .history
        .iter()
        .map(|l| Value::Str(PhpStr::from(l.clone())))
        .collect();
    Ok(Value::Array(PhpArray::from_values(h)))
}

/// The history file name, after the open_basedir check; None without a
/// name (libedit's default file in the home directory is not used).
fn history_path(vm: &mut Vm, a: &Args) -> R<Result<Option<std::path::PathBuf>, ()>> {
    let Some(name) = a.opt_str(0) else {
        return Ok(Ok(None));
    };
    if !name.is_empty() && !vm.basedir_allows(&name) {
        let bd = vm
            .get_ini(b"open_basedir")
            .map(|v| v.to_string_lossy())
            .unwrap_or_default();
        vm.fwarn(
            a,
            &format!(
                "open_basedir restriction in effect. File({}) is not within the allowed path(s): ({})",
                name.to_string_lossy(),
                bd
            ),
        )?;
        return Ok(Err(()));
    }
    use std::os::unix::ffi::OsStrExt;
    let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&name));
    Ok(Ok(Some(if p.is_absolute() {
        p.to_path_buf()
    } else {
        vm.cwd.join(p)
    })))
}

const HEADER: &[u8] = b"_HiStOrY_V2_";

/// libedit's encoding of history lines (strvis: whitespace and
/// backslashes as octal escapes).
fn encode(l: &[u8], out: &mut Vec<u8>) {
    for &c in l {
        match c {
            b' ' | b'\t' | b'\n' | b'\r' | b'\\' => {
                out.extend_from_slice(format!("\\{:03o}", c).as_bytes());
            }
            _ => out.push(c),
        }
    }
}

fn decode(l: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < l.len() {
        if l[i] == b'\\'
            && i + 3 < l.len() + 1
            && l.get(i + 1..i + 4).is_some_and(|d| d.iter().all(|c| (b'0'..=b'7').contains(c)))
        {
            let d = &l[i + 1..i + 4];
            out.push(((d[0] - b'0') << 6) | ((d[1] - b'0') << 3) | (d[2] - b'0'));
            i += 4;
        } else {
            out.push(l[i]);
            i += 1;
        }
    }
    out
}

fn readline_read_history(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let path = match history_path(vm, a)? {
        Err(()) => return Ok(Value::Bool(false)),
        Ok(None) => return Ok(Value::Bool(true)),
        Ok(Some(p)) => p,
    };
    let Ok(data) = std::fs::read(&path) else {
        return Ok(Value::Bool(false));
    };
    let mut lines = data.split(|&c| c == b'\n').peekable();
    if lines.peek() == Some(&HEADER) {
        lines.next();
    }
    let mut hist = Vec::new();
    for l in lines {
        if !l.is_empty() {
            hist.push(decode(l));
        }
    }
    st(vm).history.extend(hist);
    Ok(Value::Bool(true))
}

fn readline_write_history(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let path = match history_path(vm, a)? {
        Err(()) => return Ok(Value::Bool(false)),
        Ok(None) => return Ok(Value::Bool(true)),
        Ok(Some(p)) => p,
    };
    let mut out = HEADER.to_vec();
    out.push(b'\n');
    for l in &st(vm).history {
        encode(l, &mut out);
        out.push(b'\n');
    }
    Ok(Value::Bool(std::fs::write(&path, out).is_ok()))
}

fn readline_completion_function(vm: &mut Vm, a: &mut Args) -> R<Value> {
    st(vm).completion = Some(a.val(0));
    Ok(Value::Bool(true))
}

fn readline_callback_handler_install(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = st(vm);
    s.callback = Some(a.val(1));
    s.pending.clear();
    Ok(Value::Bool(true))
}

/// Reads one character; a complete line goes to the installed callback
/// (null at end of input).
fn readline_callback_read_char(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let Some(cb) = st(vm).callback.clone() else {
        return Ok(Value::Null);
    };
    vm.out.flush();
    let mut b = [0u8; 1];
    // SAFETY: reading one byte into a valid buffer.
    let n = unsafe { libc::read(0, b.as_mut_ptr() as *mut libc::c_void, 1) };
    let line = if n <= 0 {
        let p = std::mem::take(&mut st(vm).pending);
        if p.is_empty() {
            Value::Null
        } else {
            Value::Str(PhpStr::from(p))
        }
    } else if b[0] == b'\n' {
        Value::Str(PhpStr::from(std::mem::take(&mut st(vm).pending)))
    } else {
        st(vm).pending.push(b[0]);
        return Ok(Value::Null);
    };
    vm.call_value(&cb, vec![line])?;
    Ok(Value::Null)
}

fn readline_callback_handler_remove(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(st(vm).callback.take().is_some()))
}

fn noop(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Null)
}
