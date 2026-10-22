//! php-src's ext/readline (readline.c) over `editor`, an emulation of
//! GNU readline (READLINE_LIB is "readline"). The CLI's interactive shell
//! (readline_cli.c) is driven by the SAPI through the functions exported
//! here.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub mod editor;

pub use editor::{
    Completer, add_history, append_history, read_history, set_completion_append_character,
};

/// The extension name (`extension_loaded('readline')`).
pub const NAME: &[u8] = b"readline";

/// readline_cli.c's DEFAULT_PROMPT
pub const DEFAULT_PROMPT: &[u8] = b"\\b \\> ";

type R = Result<Value, VmError>;

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("readline", readline),
    ("readline_info", readline_info),
    ("readline_add_history", readline_add_history),
    ("readline_clear_history", readline_clear_history),
    ("readline_list_history", readline_list_history),
    ("readline_read_history", readline_read_history),
    ("readline_write_history", readline_write_history),
    ("readline_completion_function", readline_completion_function),
    (
        "readline_callback_handler_install",
        readline_callback_handler_install,
    ),
    ("readline_callback_read_char", readline_callback_read_char),
    (
        "readline_callback_handler_remove",
        readline_callback_handler_remove,
    ),
    ("readline_redisplay", readline_redisplay),
    ("readline_on_new_line", readline_on_new_line),
];

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    vm.phpinfo_hooks.push(minfo);
    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        vm.zpp_extension_verified.insert(
            name.as_bytes().to_vec(),
            goro_core::arginfo::CHECK_ARITY | goro_core::arginfo::CHECK_TYPES,
        );
    }
    vm.constants.insert(
        b"READLINE_LIB".to_vec(),
        Value::String(PhpString::from_bytes(b"readline")),
    );
    vm.module_constants
        .push((NAME.to_vec(), vec![b"READLINE_LIB".to_vec()]));
    // readline_cli.c's INI entries
    vm.constants
        .insert(b"cli.pager".to_vec(), Value::String(PhpString::empty()));
    vm.constants.insert(
        b"cli.prompt".to_vec(),
        Value::String(PhpString::from_bytes(DEFAULT_PROMPT)),
    );
    vm.request_shutdown_hooks.push(rshutdown);
}

/// PHP_MINFO_FUNCTION(cli_readline)
fn minfo(vm: &mut Vm) -> String {
    let ini = |vm: &Vm, k: &[u8]| -> String {
        let v = vm
            .constants
            .get(k)
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_default();
        if v.is_empty() {
            "no value".to_string()
        } else {
            v
        }
    };
    format!(
        "\nreadline\n\nReadline Support => enabled\nReadline library => {}\n\nDirective => Local Value => Master Value\ncli.pager => {} => no value\ncli.prompt => {} => {}\n",
        editor::LIBRARY_VERSION,
        ini(vm, b"cli.pager"),
        ini(vm, b"cli.prompt"),
        String::from_utf8_lossy(DEFAULT_PROMPT)
    )
}

/// PHP_RSHUTDOWN_FUNCTION(readline)
fn rshutdown(_vm: &mut Vm) {
    editor::callback_remove();
    let (a, b) = editor::with(|r| (r.completion_cb.take(), r.callback.take()));
    drop(a);
    drop(b);
}

/// Install the CLI shell's completion unless the script set its own
/// (readline_completion_function() before the shell started).
pub fn install_shell_completion(f: Completer) {
    editor::with(|r| {
        if r.completion_cb.is_none() {
            r.completer = Some(f);
        }
        r.special_prefixes = b"$".to_vec();
    });
}

/// readline() for the CLI shell.
pub fn shell_readline(vm: &mut Vm, prompt: &[u8]) -> Option<Vec<u8>> {
    editor::readline(vm, Some(prompt))
}

// ---------------------------------------------------------------------------

fn arg(args: &[Value], i: usize) -> Value {
    match args.get(i) {
        Some(v) => v.deref(),
        None => Value::Undef,
    }
}

fn sval(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

fn opt_str(args: &[Value], i: usize) -> Option<Vec<u8>> {
    match arg(args, i) {
        Value::Undef | Value::Null => None,
        v => Some(v.to_php_string().as_bytes().to_vec()),
    }
}

fn readline(vm: &mut Vm, args: &[Value]) -> R {
    let prompt = opt_str(args, 0);
    Ok(match editor::readline(vm, prompt.as_deref()) {
        Some(l) => sval(&l),
        None => Value::False,
    })
}

/// try_convert_to_string(): None once an exception is pending.
fn to_str(vm: &mut Vm, v: &Value) -> Option<Vec<u8>> {
    let s = vm.value_to_string_checked(v).ok()?;
    if vm.current_exception.is_some() {
        return None;
    }
    Some(s.as_bytes().to_vec())
}

/// SAFE_STRING() of a C string
fn cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(p) => &b[..p],
        None => b,
    }
}

fn char_str(c: u8) -> Value {
    if c == 0 { sval(b"") } else { sval(&[c]) }
}

fn readline_info(vm: &mut Vm, args: &[Value]) -> R {
    let what = opt_str(args, 0);
    let value = match arg(args, 1) {
        Value::Undef | Value::Null => None,
        v => Some(v),
    };
    let Some(what) = what else {
        let mut a = PhpArray::new();
        let set = |a: &mut PhpArray, k: &str, v: Value| {
            a.set(ArrayKey::String(PhpString::from_bytes(k.as_bytes())), v)
        };
        editor::with(|r| {
            set(&mut a, "line_buffer", sval(cstr(&r.line)));
            set(&mut a, "point", Value::Long(r.point as i64));
            set(&mut a, "end", Value::Long(r.line.len() as i64));
            set(&mut a, "mark", Value::Long(r.mark));
            set(&mut a, "done", Value::Long(r.done));
            set(&mut a, "pending_input", Value::Long(r.pending_input));
            set(&mut a, "prompt", sval(cstr(&r.prompt)));
            set(&mut a, "terminal_name", sval(b""));
            set(
                &mut a,
                "completion_append_character",
                char_str(r.append_char),
            );
            set(
                &mut a,
                "completion_suppress_append",
                if r.suppress_append {
                    Value::True
                } else {
                    Value::False
                },
            );
            set(
                &mut a,
                "library_version",
                sval(editor::LIBRARY_VERSION.as_bytes()),
            );
            set(&mut a, "readline_name", sval(cstr(&r.readline_name)));
            set(
                &mut a,
                "attempted_completion_over",
                Value::Long(r.attempted_completion_over),
            );
        });
        return Ok(Value::Array(Rc::new(RefCell::new(a))));
    };
    let w = what.to_ascii_lowercase();
    let r = match w.as_slice() {
        b"line_buffer" => {
            let old = editor::with(|r| cstr(&r.line).to_vec());
            if let Some(v) = value {
                let Some(s) = to_str(vm, &v) else {
                    return Ok(Value::Null);
                };
                editor::with(|r| {
                    r.line = s;
                    if r.point > r.line.len() {
                        r.point = r.line.len();
                    }
                });
            }
            sval(&old)
        }
        b"point" => Value::Long(editor::with(|r| r.point as i64)),
        b"end" => Value::Long(editor::with(|r| r.line.len() as i64)),
        b"mark" => Value::Long(editor::with(|r| r.mark)),
        b"done" => {
            let old = editor::with(|r| r.done);
            if let Some(v) = value {
                let n = v.to_long();
                editor::with(|r| r.done = n);
            }
            Value::Long(old)
        }
        b"pending_input" => {
            let old = editor::with(|r| r.pending_input);
            if let Some(v) = value {
                let Some(s) = to_str(vm, &v) else {
                    return Ok(Value::Null);
                };
                let c = s.first().copied().unwrap_or(0) as i8 as i64;
                editor::with(|r| r.pending_input = c);
            }
            Value::Long(old)
        }
        b"prompt" => sval(&editor::with(|r| cstr(&r.prompt).to_vec())),
        b"terminal_name" => sval(b""),
        b"completion_suppress_append" => {
            let old = editor::with(|r| r.suppress_append);
            if let Some(v) = value {
                let b = v.to_bool();
                editor::with(|r| r.suppress_append = b);
            }
            if old { Value::True } else { Value::False }
        }
        b"completion_append_character" => {
            let old = editor::with(|r| r.append_char);
            if let Some(v) = value {
                let Some(s) = to_str(vm, &v) else {
                    return Ok(Value::Null);
                };
                let c = s.first().copied().unwrap_or(0);
                editor::with(|r| r.append_char = c);
            }
            char_str(old)
        }
        b"library_version" => sval(editor::LIBRARY_VERSION.as_bytes()),
        b"readline_name" => {
            let old = editor::with(|r| cstr(&r.readline_name).to_vec());
            if let Some(v) = value {
                let Some(s) = to_str(vm, &v) else {
                    return Ok(Value::Null);
                };
                editor::with(|r| r.readline_name = cstr(&s).to_vec());
            }
            sval(&old)
        }
        b"attempted_completion_over" => {
            let old = editor::with(|r| r.attempted_completion_over);
            if let Some(v) = value {
                let n = v.to_long();
                editor::with(|r| r.attempted_completion_over = n);
            }
            Value::Long(old)
        }
        _ => Value::Null,
    };
    Ok(r)
}

fn readline_add_history(_vm: &mut Vm, args: &[Value]) -> R {
    let s = arg(args, 0).to_php_string();
    add_history(cstr(s.as_bytes()));
    Ok(Value::True)
}

fn readline_clear_history(_vm: &mut Vm, _args: &[Value]) -> R {
    editor::with(|r| r.history.clear());
    Ok(Value::True)
}

fn readline_list_history(_vm: &mut Vm, _args: &[Value]) -> R {
    let mut a = PhpArray::new();
    editor::with(|r| {
        for l in &r.history {
            a.push(sval(l));
        }
    });
    Ok(Value::Array(Rc::new(RefCell::new(a))))
}

/// The "|p!" file name argument: Err once an exception is thrown or the
/// open_basedir check failed.
fn history_file(vm: &mut Vm, fname: &str, args: &[Value]) -> Result<Option<Vec<u8>>, bool> {
    let Some(f) = opt_str(args, 0) else {
        return Ok(None);
    };
    if f.contains(&0) {
        let e = vm.create_exception(
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($filename) must not contain any null bytes",
                fname
            ),
            vm.current_line,
        );
        vm.current_exception = Some(e);
        return Err(true);
    }
    let p = String::from_utf8_lossy(&f).into_owned();
    if !vm.check_open_basedir(fname, &p) {
        return Err(false);
    }
    Ok(Some(f))
}

fn readline_read_history(vm: &mut Vm, args: &[Value]) -> R {
    let f = match history_file(vm, "readline_read_history", args) {
        Ok(f) => f,
        Err(thrown) => return Ok(if thrown { Value::Null } else { Value::False }),
    };
    Ok(if read_history(f.as_deref()) != 0 {
        Value::False
    } else {
        Value::True
    })
}

fn readline_write_history(vm: &mut Vm, args: &[Value]) -> R {
    let f = match history_file(vm, "readline_write_history", args) {
        Ok(f) => f,
        Err(thrown) => return Ok(if thrown { Value::Null } else { Value::False }),
    };
    Ok(if editor::write_history(f.as_deref()) != 0 {
        Value::False
    } else {
        Value::True
    })
}

fn readline_completion_function(_vm: &mut Vm, args: &[Value]) -> R {
    let cb = arg(args, 0);
    editor::with(|r| {
        r.completion_cb = Some(cb);
        r.completer = None;
    });
    Ok(Value::True)
}

fn readline_callback_handler_install(vm: &mut Vm, args: &[Value]) -> R {
    let prompt = arg(args, 0).to_php_string().as_bytes().to_vec();
    let cb = arg(args, 1);
    let old = editor::with(|r| r.callback.replace(cb));
    if old.is_some() {
        editor::callback_remove();
    }
    drop(old);
    editor::callback_install(vm, cstr(&prompt));
    Ok(Value::True)
}

fn readline_callback_read_char(vm: &mut Vm, _args: &[Value]) -> R {
    if editor::with(|r| r.callback.is_none()) {
        return Ok(Value::Null);
    }
    for line in editor::callback_read_char(vm) {
        // php_rl_callback_handler()
        let Some(cb) = editor::with(|r| r.callback.clone()) else {
            break;
        };
        let p = match line {
            Some(l) => sval(&l),
            None => Value::Null,
        };
        let _ = vm.call_callback(&cb, &[p]);
        if vm.current_exception.is_some() {
            break;
        }
        // the handler may have removed itself
        if editor::with(|r| r.callback.is_some()) {
            editor::callback_newline(vm);
        }
    }
    Ok(Value::Null)
}

fn readline_callback_handler_remove(_vm: &mut Vm, _args: &[Value]) -> R {
    let old = editor::with(|r| r.callback.take());
    if old.is_some() {
        editor::callback_remove();
    }
    Ok(if old.is_some() {
        Value::True
    } else {
        Value::False
    })
}

fn readline_redisplay(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Null)
}

fn readline_on_new_line(_vm: &mut Vm, _args: &[Value]) -> R {
    Ok(Value::Null)
}
