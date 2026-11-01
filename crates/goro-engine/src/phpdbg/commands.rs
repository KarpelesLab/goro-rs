//! The phpdbg commands (PHP manual: phpdbg, Commands; messages as the
//! tests in sapi/phpdbg/tests show them).

use std::rc::Rc;

use goro_runtime::{PhpStr, Value};

use super::{
    Bp, BpKind, Flow, RunEnd, Script, Step, confirm, listing, msg, read_raw_line, sync_out, with,
    write_raw,
};
use crate::bytecode::{FuncCode, fflags};
use crate::vm::{Unwind, Vm, frame_flags};

/// Runs one command line; `Some` when it resumes, starts or abandons the
/// script.
pub(crate) fn run_command(vm: &mut Vm, line: &str) -> Option<Flow> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (cmd, rest) = match line.find(char::is_whitespace) {
        Some(p) => (&line[..p], line[p..].trim()),
        None => (line, ""),
    };
    let running = with(|d| d.running);
    match cmd {
        "q" | "quit" => super::quit(vm),
        "r" | "run" => return cmd_run(vm, rest, running),
        "c" | "continue" => {
            if running {
                return Some(Flow::Resume);
            }
            not_running(vm);
        }
        "s" | "step" | "n" | "next" | "u" | "until" | "F" | "finish" | "L" | "leave" => {
            if !running {
                not_running(vm);
                return None;
            }
            return step_cmd(vm, cmd);
        }
        "b" | "break" => cmd_break(vm, rest, running),
        "w" | "watch" => super::watch::command(vm, rest, running),
        "C" | "clear" => cmd_clear(vm),
        "X" | "clean" => return cmd_clean(vm, running),
        "e" | "exec" => return cmd_exec(vm, rest, running),
        "stdin" => cmd_stdin(vm, rest, running),
        "ev" => cmd_ev(vm, rest, running),
        "t" | "back" => cmd_back(vm, running),
        "f" | "frame" => {
            if !running {
                msg(vm, "[No active op array!]");
            }
        }
        "l" | "list" => cmd_list(vm, rest, running),
        "i" | "info" => cmd_info(vm, rest, running),
        "p" | "print" => {
            if !running && rest.is_empty() {
                msg(vm, "[Not Executing!]");
            } else {
                msg(
                    vm,
                    "[Opline listings need Zend opcodes; goro has its own bytecode]",
                );
            }
        }
        "R" | "register" => cmd_register(vm, rest),
        "S" | "set" => {}
        "h" | "help" => cmd_help(vm, rest),
        "sh" => msg(vm, "[sh is not available]"),
        "source" | "<" => {
            let lines = std::fs::read_to_string(rest).unwrap_or_default();
            with(|d| {
                for (i, l) in lines
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .enumerate()
                {
                    d.queue.insert(i, l.to_string());
                }
            });
        }
        _ => {
            let lname = cmd.to_ascii_lowercase().into_bytes();
            if with(|d| d.registered.contains(&lname)) {
                call_registered(vm, cmd, rest);
            } else {
                msg(vm, &format!("[The command \"{}\" could not be found]", cmd));
            }
        }
    }
    None
}

fn not_running(vm: &mut Vm) {
    // After an uncaught exception ended the script (test
    // sapi/phpdbg/tests/exceptions_002.phpt).
    if with(|d| d.last_uncaught) {
        msg(vm, "[The stack contains nothing !]");
    } else {
        msg(vm, "[Not running]");
    }
}

/// The running position (depth, code, line).
fn here(vm: &Vm) -> (usize, usize, u32) {
    let depth = vm.ex.frames.len();
    let code = vm.ex.top.code as usize;
    let line = with(|d| d.cur.as_ref().map_or(0, |c| c.line));
    (depth, code, line)
}

fn step_cmd(vm: &mut Vm, cmd: &str) -> Option<Flow> {
    let (depth, code, line) = here(vm);
    let step = match cmd {
        "s" | "step" => Step::Into { depth, code, line },
        "n" | "next" | "u" | "until" => Step::Over { depth, code, line },
        "F" | "finish" => Step::Finish { depth },
        _ => {
            if with(|d| d.cur.as_ref().is_some_and(|c| c.ret_marker)) {
                msg(vm, "[Already at the end of the function]");
                return None;
            }
            Step::Leave { depth }
        }
    };
    with(|d| d.step = Some(step));
    Some(Flow::Resume)
}

/// Splits command arguments like a shell: quotes group, a backslash
/// escapes the next character (inside quotes too: test
/// sapi/phpdbg/tests/run_002.phpt); a `<` word names the stdin file.
fn split_words(s: &str) -> (Vec<(String, bool)>, Option<String>) {
    let mut words = Vec::new();
    let mut stdin = None;
    let mut cur = String::new();
    let mut have = false;
    let mut quoted = false;
    let mut quote: Option<char> = None;
    let mut redirect = false;
    let mut chars = s.chars().peekable();
    let mut finish = |cur: &mut String, have: &mut bool, quoted: &mut bool, redirect: &mut bool| {
        if *have {
            if *redirect {
                stdin = Some(std::mem::take(cur));
                *redirect = false;
            } else {
                words.push((std::mem::take(cur), *quoted));
            }
        }
        *have = false;
        *quoted = false;
    };
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) if c == '\\' => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                have = true;
                quoted = true;
            }
            None if c == '\\' => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    have = true;
                }
            }
            None if c == '<' && !have => {
                redirect = true;
            }
            None if c.is_whitespace() => {
                if have {
                    finish(&mut cur, &mut have, &mut quoted, &mut redirect);
                }
            }
            None => {
                cur.push(c);
                have = true;
            }
        }
    }
    finish(&mut cur, &mut have, &mut quoted, &mut redirect);
    (words, stdin)
}

fn cmd_run(vm: &mut Vm, rest: &str, running: bool) -> Option<Flow> {
    if with(|d| d.script.is_none()) {
        msg(vm, "[Nothing to execute!]");
        return None;
    }
    let (words, stdin) = split_words(rest);
    let args = if words.is_empty() {
        None
    } else {
        Some(words.into_iter().map(|(w, _)| w.into_bytes()).collect())
    };
    let stdin = stdin.map(String::into_bytes);
    if running {
        if confirm(vm, "Do you really want to restart execution? (type y or n): ") {
            return Some(Flow::Abort(RunEnd::Restart(args, stdin)));
        }
        return None;
    }
    Some(Flow::Run(args, stdin))
}

// ------------------------------------------------------------ breakpoints

/// Adds a breakpoint, announcing it.
pub(crate) fn add_bp(vm: &mut Vm, kind: BpKind) {
    let desc = kind.desc();
    let id = with(|d| {
        if d.bps.iter().any(|b| b.kind.same(&kind)) {
            return None;
        }
        let id = d.next_bp;
        d.next_bp += 1;
        d.bps.push(Bp { id, kind, hits: 0 });
        Some(id)
    });
    match id {
        Some(id) => msg(vm, &format!("[Breakpoint #{} added at {}]", id, desc)),
        None => msg(vm, &format!("[Breakpoint exists at {}]", desc)),
    }
}

/// The file a breakpoint names: as an absolute path when it exists
/// (relative to the working directory or to the script's directory).
pub(crate) fn bp_file(name: &str) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    let p = std::path::Path::new(name);
    if let Ok(a) = std::fs::canonicalize(p) {
        return a.as_os_str().as_bytes().to_vec();
    }
    if let Some(s) = with(|d| d.script.as_ref().map(|s| s.path.clone()))
        && let Some(dir) = std::path::Path::new(std::ffi::OsStr::from_bytes(&s)).parent()
        && let Ok(a) = std::fs::canonicalize(dir.join(p))
    {
        return a.as_os_str().as_bytes().to_vec();
    }
    name.as_bytes().to_vec()
}

/// A method or function breakpoint from `class::method` or `func`.
pub(crate) fn symbol_bp(s: &str) -> BpKind {
    let s = s.strip_prefix('\\').unwrap_or(s);
    match s.split_once("::") {
        Some((c, m)) => BpKind::Method {
            class: c.to_ascii_lowercase().into_bytes(),
            method: m.to_ascii_lowercase().into_bytes(),
            shown: s.to_string(),
        },
        None => BpKind::Func {
            lname: s.to_ascii_lowercase().into_bytes(),
            shown: s.to_string(),
        },
    }
}

fn cmd_break(vm: &mut Vm, rest: &str, running: bool) {
    let mut words = rest.split_whitespace();
    let Some(first) = words.next() else {
        // `break` alone: at the current line.
        if let Some(c) = with(|d| d.cur.clone()) {
            add_bp(
                vm,
                BpKind::Line {
                    file: c.file,
                    line: c.line,
                },
            );
        } else {
            msg(vm, "[No active op array!]");
        }
        return;
    };
    if first == "del" || first == "d" {
        let Some(n) = words.next().and_then(|w| w.parse::<u32>().ok()) else {
            msg(vm, "[Malformed input]");
            return;
        };
        let found = with(|d| {
            let before = d.bps.len();
            d.bps.retain(|b| b.id != n);
            before != d.bps.len()
        });
        if found {
            msg(vm, &format!("[Deleted breakpoint #{}]", n));
        } else {
            msg(vm, &format!("[Failed to find breakpoint #{}]", n));
        }
        return;
    }
    if let Ok(line) = first.parse::<u32>() {
        let file = if running {
            vm.cur_file().to_vec()
        } else {
            with(|d| d.script.as_ref().map(|s| s.path.clone())).unwrap_or_default()
        };
        add_bp(vm, BpKind::Line { file, line });
        return;
    }
    if let Some((f, l)) = first.rsplit_once(':')
        && !f.ends_with(':')
        && let Ok(line) = l.parse::<u32>()
    {
        add_bp(
            vm,
            BpKind::Line {
                file: bp_file(f),
                line,
            },
        );
        return;
    }
    if first.len() > 5 && first[..5].eq_ignore_ascii_case("ZEND_") {
        add_bp(
            vm,
            BpKind::Opcode {
                name: first.to_ascii_uppercase(),
            },
        );
        return;
    }
    let valid = first
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'\\' || b == b':' || b >= 0x80);
    if !valid {
        msg(vm, "[Malformed input]");
        return;
    }
    add_bp(vm, symbol_bp(first));
}

fn cmd_clear(vm: &mut Vm) {
    let counts = with(|d| {
        let mut c = [0usize; 4];
        for b in &d.bps {
            match b.kind {
                BpKind::Line { .. } => c[0] += 1,
                BpKind::Func { .. } => c[1] += 1,
                BpKind::Method { .. } => c[2] += 1,
                BpKind::Opcode { .. } | BpKind::Next { .. } => c[3] += 1,
            }
        }
        d.bps.clear();
        c
    });
    msg(
        vm,
        &format!(
            "Clearing Breakpoints\n\
             File              {}\n\
             Functions         {}\n\
             Methods           {}\n\
             Oplines           {}\n\
             File oplines      0\n\
             Function oplines  0\n\
             Method oplines    0\n\
             Conditionals      0",
            counts[0], counts[1], counts[2], counts[3]
        ),
    );
}

fn cmd_clean(vm: &mut Vm, running: bool) -> Option<Flow> {
    if !confirm(
        vm,
        "Do you really want to clean your current environment? (type y or n): ",
    ) {
        return None;
    }
    let classes = vm
        .class_order
        .iter()
        .filter(|n| vm.classes.get(*n).is_some_and(|c| c.decl.is_some()))
        .count();
    let funcs = vm.user_functions.len();
    let consts = vm.user_constants.len();
    msg(
        vm,
        &format!(
            "Cleaning Execution Environment\n\
             Classes    {}\n\
             Functions  {}\n\
             Constants  {}\n\
             Includes   0",
            classes, funcs, consts
        ),
    );
    if running {
        return Some(Flow::Abort(RunEnd::Stop));
    }
    None
}

fn cmd_exec(vm: &mut Vm, rest: &str, running: bool) -> Option<Flow> {
    if rest.is_empty() {
        msg(vm, "[Malformed input]");
        return None;
    }
    if running {
        if confirm(
            vm,
            "Do you really want to stop execution to set a new execution context? (type y or n): ",
        ) {
            with(|d| d.queue.push_front(format!("exec {}", rest)));
            return Some(Flow::Abort(RunEnd::Stop));
        }
        return None;
    }
    super::set_context(vm, rest.as_bytes(), true);
    None
}

/// The execution context read from stdin.
pub(crate) fn set_stdin_context(vm: &mut Vm, src: Vec<u8>) {
    let path = b"Standard input code".to_vec();
    let src = Rc::new(src);
    with(|d| {
        d.sources.insert(path.clone(), src.clone());
        d.script = Some(Script {
            path: path.clone(),
            src: src.clone(),
        });
    });
    super::compile_context(vm, &path, &src, "stdin input");
}

/// `stdin DELIM`: the code up to a line holding DELIM (test
/// sapi/phpdbg/tests/stdin_001.phpt).
fn cmd_stdin(vm: &mut Vm, rest: &str, running: bool) {
    if running {
        msg(vm, "[Cannot change the execution context while running]");
        return;
    }
    let delim = rest.trim();
    let mut src = Vec::new();
    while let Some(l) = read_raw_line() {
        if l == delim {
            break;
        }
        src.extend_from_slice(l.as_bytes());
        src.push(b'\n');
    }
    set_stdin_context(vm, src);
}

// ------------------------------------------------------------ evaluation

/// How `ev` and registered calls show a value: strings as they are,
/// arrays and objects as print_r() does.
fn show_value(vm: &mut Vm, v: &Value) -> Vec<u8> {
    match v.deref() {
        Value::Array(_) | Value::Object(_) => {
            let mut out = Vec::new();
            let _ = vm.print_r_into(&mut out, v, 0);
            out
        }
        other => vm
            .to_php_str(&other)
            .map(|s| s.to_vec())
            .unwrap_or_default(),
    }
}

/// An exception escaping evaluated code: reported as uncaught, the script
/// goes on (test sapi/phpdbg/tests/exceptions_002.phpt).
fn report_eval_exception(vm: &mut Vm, e: goro_runtime::ObjRef) {
    let (_, file, line) = super::exception_place(vm, &e);
    let s = match vm.exception_to_string(&e) {
        Ok(s) => s,
        Err(_) => vm.class_of(&e).name.to_string_lossy(),
    };
    let m = format!("Uncaught {}\n  thrown", s);
    vm.display_error(crate::vm::errors::E_ERROR, &m, &file, line);
}

fn cmd_ev(vm: &mut Vm, code: &str, running: bool) {
    let mut src = b"return ".to_vec();
    src.extend_from_slice(code.as_bytes());
    src.extend_from_slice(b";");
    let r = if running && vm.has_frame() {
        vm.eval_in_frame(&src, b"eval()'d code")
    } else {
        let mut s = b"<?php ".to_vec();
        s.extend_from_slice(&src);
        vm.run_main_code_value(&s, b"eval()'d code")
    };
    match r {
        Ok(v) => {
            let mut b = show_value(vm, &v);
            b.push(b'\n');
            sync_out(vm);
            write_raw(&b);
        }
        Err(Unwind::Throw(e)) => {
            report_eval_exception(vm, e);
            sync_out(vm);
        }
        Err(_) => sync_out(vm),
    }
}

fn cmd_register(vm: &mut Vm, name: &str) {
    let l = name.trim().to_ascii_lowercase();
    if l.is_empty() {
        msg(vm, "[Malformed input]");
        return;
    }
    if !vm.functions.contains_key(&PhpStr::from(l.as_str())) {
        msg(
            vm,
            &format!("[The requested function ({}) could not be found]", name),
        );
        return;
    }
    with(|d| {
        if !d.registered.contains(&l.clone().into_bytes()) {
            d.registered.push(l.clone().into_bytes());
        }
    });
    msg(vm, &format!("[Registered {}]", name));
}

/// A registered function called as a command, with its arguments
/// (numbers as numbers): its output, then its result (test
/// sapi/phpdbg/tests/register_function.phpt).
fn call_registered(vm: &mut Vm, name: &str, rest: &str) {
    let (words, _) = split_words(rest);
    let args: Vec<Value> = words
        .into_iter()
        .map(|(w, quoted)| {
            if !quoted {
                if let Ok(i) = w.parse::<i64>() {
                    return Value::Int(i);
                }
                if let Ok(f) = w.parse::<f64>()
                    && w.contains('.')
                {
                    return Value::Float(f);
                }
            }
            Value::Str(PhpStr::from(w))
        })
        .collect();
    let f = Value::Str(PhpStr::from(name));
    match vm.call_value(&f, args) {
        Ok(v) => {
            let mut b = show_value(vm, &v);
            b.push(b'\n');
            sync_out(vm);
            write_raw(&b);
        }
        Err(Unwind::Throw(e)) => {
            report_eval_exception(vm, e);
            sync_out(vm);
        }
        Err(_) => sync_out(vm),
    }
}

// ------------------------------------------------------------ frames

/// A frame's function as phpdbg names it.
fn frame_name(code: &FuncCode, flags: u32) -> String {
    if flags & frame_flags::GLOBAL != 0 || code.flags & fflags::PSEUDO_MAIN != 0 {
        return "{main}".into();
    }
    code.display_name()
}

fn cmd_back(vm: &mut Vm, running: bool) {
    if !running || !vm.has_frame() {
        msg(vm, "[The stack contains nothing !]");
        return;
    }
    let mut out = String::new();
    let n = vm.ex.frames.len();
    let mut k = 0;
    for i in (0..n).rev() {
        let f = &vm.ex.frames[i];
        let code = f.code();
        if code.file.is_empty() {
            continue;
        }
        if f.flags & frame_flags::ATTACHED != 0 && code.flags & fflags::PSEUDO_MAIN != 0 {
            continue;
        }
        let ip = if i == n - 1 {
            vm.ex.ip.saturating_sub(1)
        } else {
            (f.ip as usize).saturating_sub(1)
        };
        let line = code.line_of(ip);
        let name = frame_name(code, f.flags);
        let call = if name == "{main}" {
            name
        } else {
            let mut args = Vec::new();
            let np = (f.argc as usize).min(code.params.len());
            for a in 0..np {
                // SAFETY: parameter slots of a live frame.
                let v = unsafe { (*f.base.add(a)).clone() };
                args.push(crate::vm::errors::trace_arg(&v.unwrap_or_default().deref()));
            }
            format!("{}({})", name, args.join(", "))
        };
        out.push_str(&format!(
            "frame #{}: {} at {}:{}\n",
            k,
            call,
            code.file.to_string_lossy(),
            line
        ));
        k += 1;
    }
    let out = out.trim_end().to_string();
    msg(vm, &out);
}

// ------------------------------------------------------------ list

fn cmd_list(vm: &mut Vm, rest: &str, running: bool) {
    let cur = with(|d| d.cur.clone());
    let script = with(|d| d.script.as_ref().map(|s| s.path.clone()));
    let (file, start, mark) = match (&cur, running) {
        (Some(c), true) => (c.file.clone(), c.line, true),
        _ => match script {
            Some(p) => (p, 1, false),
            None => {
                msg(vm, "[No file to list]");
                return;
            }
        },
    };
    let mut w = rest.split_whitespace();
    let n = match w.next() {
        None => 10,
        Some(x) => match x.parse::<u32>() {
            Ok(n) => n,
            Err(_) => {
                // `list func NAME` / `list file:line`
                let target = if x == "f" || x == "func" || x == "m" || x == "method" {
                    w.next().unwrap_or("")
                } else {
                    x
                };
                if let Some((f, l)) = target.rsplit_once(':')
                    && let Ok(l) = l.parse::<u32>()
                {
                    listing(vm, &bp_file(f), l, 10, false);
                    return;
                }
                let lname = target.trim_start_matches('\\').to_ascii_lowercase();
                if let Some(code) = vm
                    .functions
                    .get(&PhpStr::from(lname.as_str()))
                    .and_then(|f| f.code().cloned())
                {
                    listing(
                        vm,
                        &code.file,
                        code.line_start,
                        code.line_end.saturating_sub(code.line_start) + 1,
                        false,
                    );
                } else {
                    msg(vm, &format!("[Function {} not found]", target));
                }
                return;
            }
        },
    };
    listing(vm, &file, start, n, mark);
}

// ------------------------------------------------------------ info

/// A value's type as `info` shows it.
fn type_label(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Str(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Resource(_) => "resource",
        Value::Ref(_) => "reference",
    }
}

/// The value line under a scalar in `info vars` / `info constants`.
fn value_line(v: &Value) -> Option<String> {
    Some(match v {
        Value::Str(s) => format!("string ({}) \"{}\"", s.len(), s.to_string_lossy()),
        Value::Int(i) => format!("int ({})", i),
        Value::Float(f) => format!(
            "float ({})",
            goro_runtime::convert::float_to_string(*f, 14).to_string_lossy()
        ),
        Value::Bool(b) => format!("bool ({})", if *b { "true" } else { "false" }),
        _ => return None,
    })
}

fn var_row(addr: usize, refs: usize, v: &Value, name: &str) -> String {
    let mut s = format!(
        "{:#018x} {:<8}{:<10}{}",
        addr,
        refs,
        type_label(v),
        name
    );
    if let Some(l) = value_line(v) {
        s.push('\n');
        s.push_str(&l);
    }
    s
}

const VAR_HEADER: &str = "Address            Refs    Type      Variable";

fn cmd_info(vm: &mut Vm, rest: &str, running: bool) {
    let what = rest.split_whitespace().next().unwrap_or("");
    match what {
        "b" | "break" => info_break(vm),
        "c" | "classes" => info_classes(vm, running),
        "f" | "funcs" => info_funcs(vm, running),
        "F" | "files" => {
            let files: Vec<String> = vm.included.iter().map(|f| f.to_string_lossy()).collect();
            let mut s = format!("[Included files: {}]", files.len());
            for f in files {
                s.push_str(&format!("\nFile: {}", f));
            }
            msg(vm, &s);
        }
        "e" | "error" => match vm.err.last.clone() {
            None => msg(vm, "[No error found!]"),
            Some((_, m, f, l)) => msg(
                vm,
                &format!(
                    "[Last error: {} at {} line {}]",
                    m.to_string_lossy(),
                    f.to_string_lossy(),
                    l
                ),
            ),
        },
        "d" | "constants" => {
            let names = vm.user_constants.clone();
            let mut rows = Vec::new();
            for n in &names {
                if let Some(v) = vm.constants.get(n) {
                    rows.push(var_row(n.as_ptr() as usize, 1, v, &n.to_string_lossy()));
                }
            }
            let mut s = format!("[User-defined constants ({})]", rows.len());
            if !rows.is_empty() {
                s.push_str("\nAddress            Refs    Type      Constant");
                for r in rows {
                    s.push('\n');
                    s.push_str(&r);
                }
            }
            msg(vm, &s);
        }
        "v" | "vars" => {
            if !running || !vm.has_frame() {
                msg(vm, "[No active op array!]");
                return;
            }
            info_vars(vm);
        }
        "g" | "globals" => {
            let names = [
                "_GET", "_POST", "_COOKIE", "_SERVER", "_ENV", "_REQUEST", "_FILES",
            ];
            let mut s = format!("[Superglobal variables ({})]\n{}", names.len(), VAR_HEADER);
            let g = vm.globals_array();
            for n in names {
                let v = g.get_bytes(n.as_bytes()).cloned().unwrap_or_default();
                let refs = if n == "_REQUEST" { 1 } else { 2 };
                let addr = match &v {
                    Value::Array(a) => a.data_addr(),
                    _ => 0,
                };
                s.push('\n');
                s.push_str(&format!(
                    "{:#018x} {:<8}{:<10}${}",
                    addr,
                    refs,
                    type_label(&v),
                    n
                ));
            }
            msg(vm, &s);
        }
        "l" | "literal" => {
            if !running || !vm.has_frame() {
                msg(vm, "[No active op array!]");
                return;
            }
            let code = vm.frame().code();
            let name = frame_label(code, vm.frame().flags);
            let mut lits: Vec<String> = Vec::new();
            for c in &code.consts {
                let t = match c {
                    Value::Str(s) => s.to_string_lossy(),
                    Value::Int(i) => i.to_string(),
                    Value::Float(f) => goro_runtime::convert::float_to_string(*f, 14).to_string_lossy(),
                    _ => continue,
                };
                if !lits.contains(&t) {
                    lits.push(t);
                }
            }
            let mut s = format!("[Literal Constants in {} ({})]", name, lits.len());
            for (i, l) in lits.iter().enumerate() {
                s.push_str(&format!("\n|-------- C{} -------> [{}]", i, l));
            }
            msg(vm, &s);
        }
        "m" | "memory" => {
            let u = vm.memory_in_use();
            let p = vm.memory_peak();
            msg(
                vm,
                &format!(
                    "[Memory Manager - Default]\n|-------> Current: \t{:.2} kB\t\n|-------> Peak: \t{:.2} kB",
                    u as f64 / 1024.0,
                    p as f64 / 1024.0
                ),
            );
        }
        _ => msg(vm, "[Malformed input]"),
    }
}

/// `foo()` / `A::b()` / the global scope, for info headers.
fn frame_label(code: &FuncCode, flags: u32) -> String {
    if flags & frame_flags::GLOBAL != 0 || code.flags & fflags::PSEUDO_MAIN != 0 {
        return "the global scope".into();
    }
    format!("{}()", code.display_name())
}

fn info_vars(vm: &mut Vm) {
    let f = vm.frame();
    let code = f.code();
    let label = frame_label(code, f.flags);
    let mut rows = Vec::new();
    for i in 0..code.num_cvs as usize {
        // SAFETY: variable slots of the running frame.
        let v = unsafe { (*f.base.add(i)).clone() };
        let Some(v) = v else { continue };
        let name = format!("${}", code.cv_names[i].to_string_lossy());
        let (addr, refs, shown) = match &v {
            Value::Ref(r) => (f.base as usize + i * 16, 2, r.get()),
            other => (f.base as usize + i * 16, 1, other.clone()),
        };
        rows.push(var_row(addr, refs, &shown, &name));
    }
    let mut s = format!("[Variables in {} ({})]", label, rows.len());
    if !rows.is_empty() {
        s.push('\n');
        s.push_str(VAR_HEADER);
        for r in rows {
            s.push('\n');
            s.push_str(&r);
        }
    }
    msg(vm, &s);
}

fn info_break(vm: &mut Vm) {
    let sections = with(|d| {
        let mut files = Vec::new();
        let mut funcs = Vec::new();
        let mut methods = Vec::new();
        let mut ops = Vec::new();
        for b in &d.bps {
            let row = format!("#{}\t\t{}", b.id, b.kind.desc());
            match b.kind {
                BpKind::Line { .. } => files.push(row),
                BpKind::Func { .. } => funcs.push(row),
                BpKind::Method { .. } => methods.push(row),
                BpKind::Opcode { .. } | BpKind::Next { .. } => ops.push(row),
            }
        }
        let mut out = Vec::new();
        for (title, rows) in [
            ("File Breakpoints:", files),
            ("Function Breakpoints:", funcs),
            ("Method Breakpoints:", methods),
            ("Opcode Breakpoints:", ops),
        ] {
            if !rows.is_empty() {
                out.push(format!(
                    "------------------------------------------------\n{}\n{}",
                    title,
                    rows.join("\n")
                ));
            }
        }
        out
    });
    for s in sections {
        msg(vm, &s);
    }
}

/// User functions: (name, file, line).
fn user_functions(vm: &Vm, running: bool) -> Vec<(String, String, u32)> {
    let mut out = Vec::new();
    if running {
        for n in &vm.user_functions {
            if let Some(code) = vm.functions.get(n).and_then(|f| f.code()) {
                out.push((
                    code.name.to_string_lossy(),
                    code.file.to_string_lossy(),
                    code.line_start,
                ));
            }
        }
        return out;
    }
    with(|d| {
        if let Some(u) = &d.unit {
            for &i in &u.hoisted_funcs {
                let c = &u.main.children[i as usize];
                out.push((
                    c.name.to_string_lossy(),
                    c.file.to_string_lossy(),
                    c.line_start,
                ));
            }
        }
    });
    out
}

fn info_funcs(vm: &mut Vm, running: bool) {
    let fs = user_functions(vm, running);
    let mut s = format!("[User Functions ({})]", fs.len());
    for (n, f, l) in fs {
        s.push_str(&format!("\n|-------- {} in {} on line {}", n, f, l));
    }
    msg(vm, &s);
}

/// User classes: (name, method count, parent and whether it is known,
/// file, line).
type ClassRow = (String, usize, Option<(String, bool)>, String, u32);

fn user_classes(vm: &Vm, running: bool) -> Vec<ClassRow> {
    let mut out = Vec::new();
    if running {
        for n in &vm.class_order {
            if let Some(c) = vm.classes.get(n)
                && c.decl.is_some()
            {
                let parent = c.parent.as_ref().map(|p| (p.name.to_string_lossy(), true));
                out.push((
                    c.name.to_string_lossy(),
                    c.method_order.borrow().len(),
                    parent,
                    c.file.as_ref().map(|f| f.to_string_lossy()).unwrap_or_default(),
                    c.line_start,
                ));
            }
        }
        return out;
    }
    with(|d| {
        if let Some(u) = &d.unit {
            let names: Vec<Vec<u8>> = u
                .toplevel_classes
                .iter()
                .map(|&i| u.main.classes[i as usize].name.to_ascii_lowercase())
                .collect();
            for (k, &i) in u.toplevel_classes.iter().enumerate() {
                let c = &u.main.classes[i as usize];
                let parent = c.parent.as_ref().map(|p| {
                    let lp = super::lower(p);
                    // Linked only when the parent was declared before.
                    let known = names[..k].contains(&lp) || vm.classes.contains_key(&PhpStr::from(lp));
                    (p.to_string_lossy(), known)
                });
                out.push((
                    c.name.to_string_lossy(),
                    c.methods.len(),
                    parent,
                    c.file.to_string_lossy(),
                    c.line_start,
                ));
            }
        }
    });
    out
}

fn info_classes(vm: &mut Vm, running: bool) {
    let cs = user_classes(vm, running);
    let mut s = format!("[User Classes ({})]", cs.len());
    for (n, m, parent, f, l) in cs {
        s.push_str(&format!("\nUser Class {} ({})", n, m));
        if let Some((p, known)) = parent {
            s.push_str(&format!("\n|-------- User Class {}", p));
            if !known {
                s.push_str(
                    " (not yet linked because declaration for parent was not encountered when declaring the class)",
                );
            }
        }
        s.push_str(&format!("\n|---- in {} on line {}", f, l));
    }
    msg(vm, &s);
}

// ------------------------------------------------------------ help

fn cmd_help(vm: &mut Vm, topic: &str) {
    const COMMANDS: &[(&str, &str)] = &[
        ("exec", "set execution context"),
        ("stdin", "read the code to execute from stdin"),
        ("run", "attempt execution"),
        ("step", "continue execution until the next line"),
        ("next", "continue execution until the next line, over calls"),
        ("continue", "continue execution"),
        ("until", "continue past the current line"),
        ("finish", "continue past the end of the stack frame"),
        ("leave", "continue until the end of the stack frame"),
        ("break", "set a breakpoint"),
        ("watch", "set a watchpoint"),
        ("clear", "clear all breakpoints"),
        ("clean", "clean the execution environment"),
        ("back", "show the trace"),
        ("frame", "switch to a frame"),
        ("list", "list source"),
        ("info", "show debugging information"),
        ("print", "print opcodes"),
        ("ev", "evaluate some code"),
        ("register", "register a function as a command"),
        ("set", "set debugger settings"),
        ("source", "execute a phpdbginit script"),
        ("quit", "exit phpdbg"),
    ];
    let t = topic.trim();
    if t.is_empty() {
        let mut s = String::from("phpdbg commands:");
        for (c, d) in COMMANDS {
            s.push_str(&format!("\n  {:<10} {}", c, d));
        }
        msg(vm, &s);
        return;
    }
    match COMMANDS.iter().find(|(c, _)| *c == t) {
        Some((c, d)) => msg(vm, &format!("{}: {}", c, d)),
        None => msg(vm, &format!("[No help topic found for {}]", t)),
    }
}
