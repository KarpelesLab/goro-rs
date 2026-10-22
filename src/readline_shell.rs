//! The CLI's interactive shell (`php -a`), ext/readline's readline_cli.c:
//! prompts, the "is this a complete statement" scanner, code completion
//! and the read-eval loop.

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::engine;

fn ini_str(vm: &Vm, k: &[u8]) -> Option<Vec<u8>> {
    vm.constants
        .get(k)
        .map(|v| v.to_php_string().as_bytes().to_vec())
}

/// Evaluate a piece of code; the output it printed. Diagnostics and
/// uncaught exceptions are reported like zend_eval_stringl() does.
fn eval_capture(vm: &mut Vm, code: &[u8], name: &str) -> (Vec<u8>, EvalStatus) {
    vm.flush_cli_output();
    let direct = vm.cli_direct_output;
    vm.cli_direct_output = false;
    let status = eval(vm, code, name);
    let out = if vm.ob_stack.is_empty() {
        vm.take_output()
    } else {
        Vec::new()
    };
    vm.cli_direct_output = direct;
    (out, status)
}

#[derive(PartialEq)]
enum EvalStatus {
    Ok,
    /// a fatal error (EG(exit_status) = 255)
    Fatal,
    /// exit() with this status
    Exit(i32),
}

fn eval(vm: &mut Vm, code: &[u8], name: &str) -> EvalStatus {
    let mut src = b"<?php ".to_vec();
    src.extend_from_slice(code);
    let prev_file = std::mem::replace(&mut vm.current_file, name.to_string());
    let status = match engine::compile(&src, name, false) {
        Ok(mut c) => {
            engine::load(vm, &mut c);
            let r = vm.execute_chunk(&c.op_array);
            if vm.exit_requested {
                vm.exit_requested = false;
                vm.current_exception = None;
                EvalStatus::Exit(vm.exit_code as i32)
            } else if vm.current_exception.is_some() {
                let line = r.as_ref().err().map(|e| e.line).unwrap_or(0);
                report_exception(vm, name, line);
                EvalStatus::Ok
            } else if r.is_err() || vm.fatal_emitted {
                if let Err(e) = r {
                    if !vm.fatal_emitted {
                        vm.report_uncaught(&e);
                    }
                }
                vm.fatal_emitted = false;
                EvalStatus::Fatal
            } else {
                EvalStatus::Ok
            }
        }
        Err(e) => {
            engine::report_compile_error(vm, &e, name);
            EvalStatus::Fatal
        }
    };
    vm.current_file = prev_file;
    status
}

/// zend_exception_error(ex, E_WARNING)
fn report_exception(vm: &mut Vm, name: &str, err_line: u32) {
    let Some(exc) = vm.current_exception.take() else {
        return;
    };
    let (file, line) = match &exc {
        Value::Object(o) => {
            let mut o = o.borrow_mut();
            let mut file = o.get_property(b"file").to_php_string().to_string_lossy();
            let mut line = o.get_property(b"line").to_long() as u32;
            // (an error raised while no line was current)
            if file.is_empty() {
                file = name.to_string();
                o.set_property(
                    b"file".to_vec(),
                    Value::String(PhpString::from_string(file.clone())),
                );
            }
            if line == 0 && err_line != 0 {
                line = err_line;
                o.set_property(b"line".to_vec(), Value::Long(line as i64));
            }
            (file, line)
        }
        _ => (String::new(), 0),
    };
    let s = match vm.value_to_string_checked(&exc) {
        Ok(v) => v.to_string_lossy(),
        Err(_) => String::new(),
    };
    vm.current_exception = None;
    let prev_file = std::mem::replace(&mut vm.current_file, file);
    vm.emit_warning_at(&format!("Uncaught {}\n  thrown", s), line);
    vm.current_file = prev_file;
}

/// cli_get_prompt()
fn cli_get_prompt(vm: &mut Vm, block: &[u8], prompt: u8) -> Vec<u8> {
    let spec =
        ini_str(vm, b"cli.prompt").unwrap_or_else(|| goro_ext_readline::DEFAULT_PROMPT.to_vec());
    let spec = if spec.is_empty() {
        // (an empty string makes the loop below read past its end in C;
        // the prompt is then empty)
        return Vec::new();
    } else {
        spec
    };
    let mut out = Vec::new();
    let mut unicode_warned = false;
    let mut i = 0;
    while i < spec.len() {
        let c = spec[i];
        if c == b'\\' {
            let n = spec.get(i + 1).copied().unwrap_or(0);
            match n {
                b'\\' => {
                    out.push(b'\\');
                    i += 1;
                }
                b'n' => {
                    out.push(b'\n');
                    i += 1;
                }
                b't' => {
                    out.push(b'\t');
                    i += 1;
                }
                b'e' => {
                    out.push(0x1b);
                    i += 1;
                }
                b'v' => {
                    out.extend_from_slice(crate::PHP_VERSION.as_bytes());
                    i += 1;
                }
                b'b' => {
                    out.extend_from_slice(block);
                    i += 1;
                }
                b'>' => {
                    out.push(prompt);
                    i += 1;
                }
                b'`' => {
                    out.push(b'`');
                    i += 1;
                }
                _ => out.push(b'\\'),
            }
        } else if c == b'`' {
            if let Some(len) = spec[i + 1..].iter().position(|&x| x == b'`') {
                let code = spec[i + 1..i + 1 + len].to_vec();
                // the output goes to the prompt (and to the terminal)
                let (o, _) = eval_capture(vm, &code, "php prompt code");
                vm.write_stdout_stream(&o);
                out.extend_from_slice(&o);
                i += len + 1;
            }
        } else if c & 0x80 == 0 {
            out.push(c);
        } else {
            if !unicode_warned {
                vm.emit_warning("prompt contains unsupported unicode characters");
                unicode_warned = true;
            }
            out.push(b'?');
        }
        i += 1;
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum CodeType {
    Body,
    Sstring,
    Dstring,
    SstringEsc,
    DstringEsc,
    CommentLine,
    CommentBlock,
    HeredocStart,
    Heredoc,
    Outside,
}

/// cli_is_valid_code(): whether `code` ends a statement, and the prompt
/// for what follows.
fn cli_is_valid_code(vm: &mut Vm, code: &[u8]) -> (bool, Vec<u8>) {
    use CodeType::*;
    let len = code.len();
    let at = |i: usize| -> u8 { code.get(i).copied().unwrap_or(0) };
    let mut valid_end = true;
    let mut last_valid_end = true;
    let mut brackets_count = 0i32;
    let mut brace_count = 0i32;
    let mut code_type = Body;
    let mut heredoc_tag: Option<usize> = None;
    let mut heredoc_len = 0usize;
    let short_tags = ini_str(vm, b"short_open_tag")
        .map(|v| {
            let l = String::from_utf8_lossy(&v).to_ascii_lowercase();
            matches!(l.as_str(), "1" | "on" | "yes" | "true")
        })
        .unwrap_or(true);
    let mut i = 0;
    while i < len {
        match code_type {
            Body => match code[i] {
                b'{' => {
                    brackets_count += 1;
                    valid_end = false;
                }
                b'}' => {
                    if brackets_count > 0 {
                        brackets_count -= 1;
                    }
                    valid_end = brackets_count == 0;
                }
                b'(' => {
                    brace_count += 1;
                    valid_end = false;
                }
                b')' => {
                    if brace_count > 0 {
                        brace_count -= 1;
                    }
                    valid_end = false;
                }
                b';' => valid_end = brace_count == 0 && brackets_count == 0,
                b' ' | b'\r' | b'\n' | b'\t' => {}
                b'\'' => code_type = Sstring,
                b'"' => code_type = Dstring,
                b'#' => {
                    if at(i + 1) == b'[' {
                        valid_end = false;
                    } else {
                        code_type = CommentLine;
                    }
                }
                b'/' => {
                    if at(i + 1) == b'/' {
                        i += 1;
                        code_type = CommentLine;
                    } else if at(i + 1) == b'*' {
                        last_valid_end = valid_end;
                        valid_end = false;
                        code_type = CommentBlock;
                        i += 1;
                    } else {
                        valid_end = false;
                    }
                }
                b'?' => {
                    if at(i + 1) == b'>' {
                        i += 1;
                        code_type = Outside;
                    } else {
                        valid_end = false;
                    }
                }
                b'<' => {
                    valid_end = false;
                    if i + 2 < len && code[i + 1] == b'<' && code[i + 2] == b'<' {
                        i += 2;
                        code_type = HeredocStart;
                        heredoc_tag = None;
                        heredoc_len = 0;
                    }
                }
                _ => valid_end = false,
            },
            Sstring => {
                if code[i] == b'\\' {
                    code_type = SstringEsc;
                } else if code[i] == b'\'' {
                    code_type = Body;
                }
            }
            SstringEsc => code_type = Sstring,
            Dstring => {
                if code[i] == b'\\' {
                    code_type = DstringEsc;
                } else if code[i] == b'"' {
                    code_type = Body;
                }
            }
            DstringEsc => code_type = Dstring,
            CommentLine => {
                if code[i] == b'\n' {
                    code_type = Body;
                }
            }
            CommentBlock => {
                if i > 0 && code[i - 1] == b'*' && code[i] == b'/' {
                    code_type = Body;
                    valid_end = last_valid_end;
                }
            }
            HeredocStart => match code[i] {
                b' ' | b'\t' | b'\'' | b'"' => {}
                b'\r' | b'\n' => {
                    code_type = if heredoc_tag.is_some() { Heredoc } else { Body };
                }
                _ => {
                    if heredoc_tag.is_none() {
                        heredoc_tag = Some(i);
                    }
                    heredoc_len += 1;
                }
            },
            Heredoc => {
                let tag_at = heredoc_tag.unwrap_or(0);
                let tag = &code[tag_at..tag_at + heredoc_len];
                // strncmp(code + i - heredoc_len + 1, tag, heredoc_len)
                if i + 1 >= heredoc_len {
                    let s = i + 1 - heredoc_len;
                    if code.get(s..s + heredoc_len) == Some(tag) {
                        let c = at(i + 1);
                        let ident = c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80;
                        if !ident {
                            // only blanks between the line start and the tag
                            let mut p = s as isize - 1;
                            while p >= 0 && (code[p as usize] == b' ' || code[p as usize] == b'\t')
                            {
                                p -= 1;
                            }
                            if p >= 0 && code[p as usize] == b'\n' {
                                code_type = Body;
                            }
                        }
                    }
                }
            }
            Outside => {
                if (short_tags && i >= 1 && code[i - 1..].starts_with(b"<?"))
                    || (i > 3 && code[i - 4..].starts_with(b"<?php"))
                {
                    code_type = Body;
                }
            }
        }
        i += 1;
    }
    let prompt = match code_type {
        Sstring | SstringEsc => cli_get_prompt(vm, b"php", b'\''),
        Dstring | DstringEsc => cli_get_prompt(vm, b"php", b'"'),
        CommentBlock => cli_get_prompt(vm, b"/* ", b'>'),
        Heredoc => cli_get_prompt(vm, b"<<<", b'>'),
        Outside => cli_get_prompt(vm, b"   ", b'>'),
        _ => {
            if brace_count != 0 {
                cli_get_prompt(vm, b"php", b'(')
            } else if brackets_count != 0 {
                cli_get_prompt(vm, b"php", b'{')
            } else {
                cli_get_prompt(vm, b"php", b'>')
            }
        }
    };
    (valid_end && brackets_count == 0, prompt)
}

/// The values of an array returned by a PHP function.
fn call_list(vm: &mut Vm, func: &str, args: &[Value]) -> Vec<Value> {
    let f = Value::String(PhpString::from_bytes(func.as_bytes()));
    match vm.call_callback(&f, args).map(|v| v.deref()) {
        Ok(Value::Array(a)) => a.borrow().values().map(|v| v.deref()).collect(),
        _ => {
            vm.current_exception = None;
            Vec::new()
        }
    }
}

fn names(vm: &mut Vm, func: &str, args: &[Value]) -> Vec<Vec<u8>> {
    call_list(vm, func, args)
        .into_iter()
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .collect()
}

/// The keys of an array returned by a PHP function.
fn key_names(vm: &mut Vm, func: &str, args: &[Value]) -> Vec<Vec<u8>> {
    let f = Value::String(PhpString::from_bytes(func.as_bytes()));
    match vm.call_callback(&f, args).map(|v| v.deref()) {
        Ok(Value::Array(a)) => a
            .borrow()
            .keys()
            .map(|k| match k {
                goro_core::array::ArrayKey::Int(n) => n.to_string().into_bytes(),
                goro_core::array::ArrayKey::String(s) => s.as_bytes().to_vec(),
            })
            .collect(),
        _ => {
            vm.current_exception = None;
            Vec::new()
        }
    }
}

/// cli_completion_generator(), all the matches at once.
fn cli_completion(vm: &mut Vm, text: &[u8], _start: usize, _end: usize) -> Option<Vec<Vec<u8>>> {
    use goro_ext_readline::set_completion_append_character as append;
    let mut out = Vec::new();
    if text.first() == Some(&b'$') {
        let vars: Vec<Vec<u8>> = match engine::compile(
            b"<?php return array_keys($GLOBALS);",
            "php shell code",
            false,
        ) {
            Ok(mut c) => {
                engine::load(vm, &mut c);
                match vm.execute_chunk(&c.op_array) {
                    Ok(Value::Array(a)) => a
                        .borrow()
                        .values()
                        .map(|v| v.to_php_string().as_bytes().to_vec())
                        .collect(),
                    _ => Vec::new(),
                }
            }
            Err(_) => Vec::new(),
        };
        for v in vars {
            if v.starts_with(&text[1..]) {
                let mut m = b"$".to_vec();
                m.extend_from_slice(&v);
                out.push(m);
                append(0);
            }
        }
        return Some(out);
    }
    if text.first() == Some(&b'#') && text.get(1) != Some(&b'[') {
        for k in key_names(vm, "ini_get_all", &[Value::Null, Value::False]) {
            if k.starts_with(&text[1..]) {
                let mut m = b"#".to_vec();
                m.extend_from_slice(&k);
                out.push(m);
                append(b'=');
            }
        }
        return Some(out);
    }
    let lc = text.to_ascii_lowercase();
    if let Some(p) = text.windows(2).position(|w| w == b"::") {
        // Class::member
        let class = &text[..p];
        let cv = Value::String(PhpString::from_bytes(class));
        let exists = Value::String(PhpString::from_bytes(b"class_exists"));
        if !vm
            .call_callback(&exists, std::slice::from_ref(&cv))
            .is_ok_and(|v| v.to_bool())
        {
            vm.current_exception = None;
            return None;
        }
        let lmember = &lc[p + 2..];
        let member = &text[p + 2..];
        let cname = class.to_vec();
        for m in names(vm, "get_class_methods", std::slice::from_ref(&cv)) {
            if m.to_ascii_lowercase().starts_with(lmember) {
                let mut s = cname.clone();
                s.extend_from_slice(b"::");
                s.extend_from_slice(&m);
                out.push(s);
                append(b'(');
            }
        }
        let consts = {
            let code = format!(
                "<?php return array_keys((new ReflectionClass('{}'))->getConstants());",
                String::from_utf8_lossy(class).replace(['\\', '\''], "")
            );
            match engine::compile(code.as_bytes(), "php shell code", false) {
                Ok(mut c) => {
                    engine::load(vm, &mut c);
                    match vm.execute_chunk(&c.op_array) {
                        Ok(Value::Array(a)) => a
                            .borrow()
                            .values()
                            .map(|v| v.to_php_string().as_bytes().to_vec())
                            .collect(),
                        _ => {
                            vm.current_exception = None;
                            Vec::new()
                        }
                    }
                }
                Err(_) => Vec::new(),
            }
        };
        // (php-src compares the constant names with the start of the
        // whole text, as long as the member part)
        let quirk = &text[..member.len()];
        for k in consts {
            if k.starts_with(quirk) {
                let mut s = cname.clone();
                s.extend_from_slice(b"::");
                s.extend_from_slice(&k);
                out.push(s);
                append(0);
            }
        }
        return Some(out);
    }
    // functions, then constants, then classes
    let funcs = call_list(vm, "get_defined_functions", &[]);
    for group in funcs {
        if let Value::Array(a) = group {
            for f in a.borrow().values() {
                let f = f.to_php_string().as_bytes().to_vec();
                if f.to_ascii_lowercase().starts_with(&lc) {
                    out.push(f);
                    append(b'(');
                }
            }
        }
    }
    for k in key_names(vm, "get_defined_constants", &[]) {
        if k.starts_with(text) {
            out.push(k);
            append(0);
        }
    }
    let mut classes = names(vm, "get_declared_classes", &[]);
    classes.extend(names(vm, "get_declared_interfaces", &[]));
    classes.extend(names(vm, "get_declared_traits", &[]));
    for c in classes {
        if c.to_ascii_lowercase().starts_with(&lc) {
            out.push(c);
            append(0);
        }
    }
    Some(out)
}

/// readline_shell_run(): the exit status.
pub fn shell_run(vm: &mut Vm) -> i32 {
    let mut code: Vec<u8> = Vec::new();
    let mut prompt = cli_get_prompt(vm, b"php", b'>');
    let mut exit_status = 0;
    vm.flush_cli_output();

    if let Some(f) = ini_str(vm, b"auto_prepend_file").filter(|f| !f.is_empty()) {
        let quoted = String::from_utf8_lossy(&f)
            .replace('\\', "\\\\")
            .replace('\'', "\\'");
        let snippet = format!("require '{}';", quoted);
        let (o, st) = eval_capture(vm, snippet.as_bytes(), "php shell code");
        vm.write_stdout_stream(&o);
        if let EvalStatus::Exit(n) = st {
            exit_status = n;
        }
    }

    let history_file: Vec<u8> = {
        use std::os::unix::ffi::OsStringExt;
        match std::env::var_os("PHP_HISTFILE") {
            Some(f) => f.into_vec(),
            None => {
                let mut h = std::env::var_os("HOME")
                    .map(|h| h.into_vec())
                    .unwrap_or_default();
                h.extend_from_slice(b"/.php_history");
                h
            }
        }
    };
    goro_ext_readline::install_shell_completion(cli_completion);
    goro_ext_readline::read_history(Some(&history_file));

    let mut history_lines_to_write = 0usize;
    while let Some(line) = goro_ext_readline::shell_readline(vm, &prompt) {
        if line == b"exit" || line == b"quit" {
            break;
        }
        if code.is_empty() && line.is_empty() {
            continue;
        }
        // "#name=value" sets an INI entry
        if line.first() == Some(&b'#') && line.get(1) != Some(&b'[') {
            if let Some(p) = line[1..].iter().position(|&c| c == b'=') {
                let cmd = line[1..1 + p].to_vec();
                let param = line[p + 2..].to_vec();
                let f = Value::String(PhpString::from_bytes(b"ini_set"));
                let _ = vm.call_callback(
                    &f,
                    &[
                        Value::String(PhpString::from_vec(cmd)),
                        Value::String(PhpString::from_vec(param)),
                    ],
                );
                vm.current_exception = None;
                vm.flush_cli_output();
                goro_ext_readline::add_history(&line);
                prompt = cli_get_prompt(vm, b"php", b'>');
                continue;
            }
        }
        code.extend_from_slice(&line);
        code.push(b'\n');
        if !line.is_empty() {
            goro_ext_readline::add_history(&line);
            history_lines_to_write += 1;
        }
        let (valid, p) = cli_is_valid_code(vm, &code);
        prompt = p;
        if !valid {
            continue;
        }
        if history_lines_to_write > 0 {
            goro_ext_readline::append_history(history_lines_to_write, &history_file);
            history_lines_to_write = 0;
        }
        let src = std::mem::take(&mut code);
        let (out, status) = eval_capture(vm, &src, "php shell code");
        match status {
            EvalStatus::Exit(n) => exit_status = n,
            EvalStatus::Fatal => exit_status = 255,
            EvalStatus::Ok => {}
        }
        // cli.pager: the output also goes through the pager command
        let pager = ini_str(vm, b"cli.pager").filter(|p| !p.is_empty());
        let mut pager_used = false;
        if let Some(cmd) = pager.filter(|_| !out.is_empty()) {
            vm.flush_cli_output();
            use std::os::unix::ffi::OsStrExt;
            if let Ok(mut child) = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(std::ffi::OsStr::from_bytes(&cmd))
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    let _ = stdin.write_all(&out);
                }
                let _ = child.wait();
                pager_used = true;
            }
        }
        let last = out.last().copied();
        vm.write_stdout_stream(&out);
        if let Some(c) = last {
            if c != b'\n' && !pager_used {
                vm.write_stdout_stream(b"\n");
            }
        }
    }
    vm.flush_cli_output();
    exit_status
}
