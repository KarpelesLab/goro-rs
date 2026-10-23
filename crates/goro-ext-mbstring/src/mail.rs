//! mb_send_mail (mbstring.c) together with the parts of
//! ext/standard/mail.c it relies on (php_mail, php_mail_build_headers).

use std::io::Write;

use goro_core::array::ArrayKey;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::mbfl::{self, EncNo, Encoding};
use crate::mbstring::*;
use crate::mime::mime_header_encode;
use crate::state;
use crate::zpp::*;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"mb_send_mail", mb_send_mail);
    vm.register_function(b"mail", mail_fn);
}

/// SKIP_LONG_HEADER_SEP + control-char replacement + trailing space trim
/// applied by PHP_FUNCTION(mail) to $to and $subject.
fn sanitize_header_value(v: &[u8]) -> Vec<u8> {
    let mut out = v.to_vec();
    while let Some(&c) = out.last() {
        if matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            out.pop();
        } else {
            break;
        }
    }
    let mut i = 0;
    while i < out.len() {
        if out[i] == 0 {
            break;
        }
        if out[i] < 0x20 || out[i] == 0x7f {
            // SKIP_LONG_HEADER_SEP: CRLF followed by space/tab is kept
            if out[i] == b'\r'
                && out.get(i + 1) == Some(&b'\n')
                && matches!(out.get(i + 2), Some(b' ') | Some(b'\t'))
            {
                i += 3;
                while i < out.len() && matches!(out[i], b' ' | b'\t') {
                    i += 1;
                }
                continue;
            }
            out[i] = b' ';
        }
        i += 1;
    }
    out
}

/// PHP_FUNCTION(mail)
fn mail_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mail";
    if args.len() < 3 {
        let msg = format!("mail() expects at least 3 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let to = path_arg(vm, f, args, 0, "to")?;
    let subject = path_arg(vm, f, args, 1, "subject")?;
    let message = path_arg(vm, f, args, 2, "message")?;
    let headers: Option<Vec<u8>> = match args.get(3).map(|v| v.deref()) {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => Some(build_headers(vm, &Value::Array(a))?),
        Some(_) => {
            let h = str_arg(vm, f, args, 3, "additional_headers")?;
            if h.contains(&0) {
                return Err(arg_value_error(
                    vm,
                    f,
                    4,
                    "additional_headers",
                    "must not contain any null bytes",
                ));
            }
            let mut end = h.len();
            while end > 0 && matches!(h[end - 1], b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0) {
                end -= 1;
            }
            Some(h[..end].to_vec())
        }
    };
    let extra = match args.get(4) {
        Some(_) => {
            let e = path_arg(vm, f, args, 4, "additional_params")?;
            Some(e)
        }
        None => None,
    };
    let to_r = sanitize_header_value(&to);
    let subject_r = sanitize_header_value(&subject);
    let force = ini_bytes(vm, "mail.force_extra_parameters").filter(|v| !v.is_empty());
    let extra_cmd = match (force, extra) {
        (Some(fp), _) => Some(escape_shell_cmd(&fp)),
        (None, Some(e)) => Some(escape_shell_cmd(&e)),
        (None, None) => None,
    };
    let hdr = headers.filter(|h| !h.is_empty());
    let ok = php_mail(
        vm,
        f,
        &to_r,
        &subject_r,
        &message,
        hdr.as_deref(),
        extra_cmd.as_deref(),
    );
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    Ok(if ok { Value::True } else { Value::False })
}

fn ini_bytes(vm: &Vm, key: &str) -> Option<Vec<u8>> {
    vm.constants
        .get(key.as_bytes())
        .map(|v| v.to_php_string().as_bytes().to_vec())
}

fn ini_bool(vm: &Vm, key: &str) -> bool {
    match ini_bytes(vm, key) {
        Some(v) => {
            let s = String::from_utf8_lossy(&v).trim().to_ascii_lowercase();
            matches!(s.as_str(), "1" | "on" | "yes" | "true")
        }
        None => false,
    }
}

/// C: `_php_mbstr_parse_mail_headers`; returns lower-cased header names
/// with their values.
fn parse_mail_headers(s: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut state = 0;
    let mut crlf_state: i32 = -1;
    let mut token: Option<usize> = None;
    let mut token_pos = 0usize;
    let mut fld_name: Option<Vec<u8>> = None;
    let mut fld_val: Option<Vec<u8>>;
    let update = |out: &mut Vec<(Vec<u8>, Vec<u8>)>, name: Vec<u8>, val: Vec<u8>| {
        let name = name.to_ascii_lowercase();
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some(e) => e.1 = val,
            None => out.push((name, val)),
        }
    };
    let mut i = 0;
    'outer: while i < s.len() {
        let c = s[i];
        match c {
            b':' => {
                if crlf_state == 1 {
                    token_pos += 1;
                }
                if state == 0 || state == 1 {
                    if let Some(t) = token {
                        if token_pos > 0 {
                            fld_name = Some(s[t..(t + token_pos).min(s.len())].to_vec());
                        }
                    }
                    state = 2;
                } else {
                    token_pos += 1;
                }
                crlf_state = 0;
            }
            b'\n' => {
                if crlf_state == -1 {
                    break 'outer;
                }
                crlf_state = -1;
            }
            b'\r' => {
                if crlf_state == 1 {
                    token_pos += 1;
                } else {
                    crlf_state = 1;
                }
            }
            b' ' | b'\t' => {
                if crlf_state == -1 {
                    if state == 3 {
                        state = 4;
                    } else {
                        state = 5;
                    }
                } else {
                    if crlf_state == 1 {
                        token_pos += 1;
                    }
                    if state == 1 || state == 3 {
                        token_pos += 1;
                    }
                }
                crlf_state = 0;
            }
            _ => {
                let mut fallthrough = false;
                match state {
                    0 => {
                        token = Some(i);
                        token_pos = 0;
                        state = 1;
                    }
                    2 => {
                        if crlf_state != -1 {
                            token = Some(i);
                            token_pos = 0;
                            state = 3;
                        } else {
                            fallthrough = true;
                        }
                    }
                    3 => fallthrough = true,
                    4 => {
                        token_pos += 1;
                        state = 3;
                    }
                    _ => {}
                }
                if fallthrough && crlf_state == -1 {
                    fld_val = token
                        .filter(|_| token_pos > 0)
                        .map(|t| s[t..(t + token_pos).min(s.len())].to_vec());
                    if let (Some(n), Some(v)) = (fld_name.take(), fld_val.take()) {
                        update(&mut out, n, v);
                    }
                    fld_name = None;
                    token = Some(i);
                    token_pos = 0;
                    state = 1;
                }
                if crlf_state == 1 {
                    token_pos += 1;
                }
                token_pos += 1;
                crlf_state = 0;
            }
        }
        i += 1;
    }
    if state == 2 {
        token = None;
        token_pos = 0;
        state = 3;
    }
    if state == 3 {
        let v = token
            .filter(|_| token_pos > 0)
            .map(|t| s[t..(t + token_pos).min(s.len())].to_vec());
        if let (Some(n), Some(v)) = (fld_name.take(), v) {
            update(&mut out, n, v);
        }
    }
    out
}

/// A header name as php_error_docref()'s "%s" prints it: up to its first NUL.
fn cstr(key: &[u8]) -> &[u8] {
    key.iter().position(|&c| c == 0).map_or(key, |p| &key[..p])
}

fn check_field_name(key: &[u8]) -> bool {
    !key.is_empty() && key.iter().all(|&c| (33..=126).contains(&c) && c != b':')
}

fn check_field_value(vm: &mut Vm, key: &[u8], val: &[u8]) -> Result<(), VmError> {
    let k = String::from_utf8_lossy(cstr(key));
    let mut i = 0;
    while i < val.len() {
        let c = val[i];
        if c == b'\r' {
            if i + 1 < val.len() && val[i + 1] == b'\n' {
                if i + 2 < val.len() && (val[i + 2] == b' ' || val[i + 2] == b'\t') {
                    i += 3;
                    continue;
                }
                return Err(value_error(
                    vm,
                    format!(
                        "Header \"{}\" contains CRLF characters that are used as a line separator and are not allowed in the header",
                        k
                    ),
                ));
            }
            return Err(value_error(
                vm,
                format!(
                    "Header \"{}\" contains CR character that is not allowed in the header",
                    k
                ),
            ));
        }
        if c == b'\n' {
            return Err(value_error(
                vm,
                format!(
                    "Header \"{}\" contains LF character that is not allowed in the header",
                    k
                ),
            ));
        }
        if c == 0 {
            return Err(value_error(
                vm,
                format!(
                    "Header \"{}\" contains NULL character that is not allowed in the header",
                    k
                ),
            ));
        }
        i += 1;
    }
    Ok(())
}

fn build_header_elem(
    vm: &mut Vm,
    out: &mut Vec<u8>,
    key: &[u8],
    val: &Value,
) -> Result<(), VmError> {
    match val.deref() {
        Value::String(s) => {
            if !check_field_name(key) {
                return Err(value_error(
                    vm,
                    format!(
                        "Header name \"{}\" contains invalid characters",
                        goro_core::string::msg_str(cstr(key))
                    ),
                ));
            }
            check_field_value(vm, key, s.as_bytes())?;
            out.extend_from_slice(key);
            out.extend_from_slice(b": ");
            out.extend_from_slice(s.as_bytes());
            out.extend_from_slice(b"\r\n");
            Ok(())
        }
        Value::Array(a) => {
            let entries: Vec<(ArrayKey, Value)> = a
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (k, v) in entries {
                if let ArrayKey::String(ks) = &k {
                    return Err(type_error(
                        vm,
                        format!(
                            "Header \"{}\" must only contain numeric keys, \"{}\" found",
                            goro_core::string::msg_str(cstr(key)),
                            ks.to_msg_string()
                        ),
                    ));
                }
                let v = v.deref();
                if !matches!(v, Value::String(_)) {
                    return Err(type_error(
                        vm,
                        format!(
                            "Header \"{}\" must only contain values of type string, {} found",
                            goro_core::string::msg_str(cstr(key)),
                            type_name(&v)
                        ),
                    ));
                }
                build_header_elem(vm, out, key, &v)?;
            }
            Ok(())
        }
        other => Err(type_error(
            vm,
            format!(
                "Header \"{}\" must be of type array|string, {} given",
                goro_core::string::msg_str(cstr(key)),
                type_name(&other)
            ),
        )),
    }
}

/// C: `php_mail_build_headers`
fn build_headers(vm: &mut Vm, headers: &Value) -> Result<Vec<u8>, VmError> {
    let arr = match headers {
        Value::Array(a) => a.clone(),
        _ => return Ok(Vec::new()),
    };
    let entries: Vec<(ArrayKey, Value)> = arr
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut out = Vec::new();
    for (k, v) in entries {
        let key = match k {
            ArrayKey::Int(n) => {
                return Err(type_error(
                    vm,
                    format!("Header name cannot be numeric, {} given", n),
                ));
            }
            ArrayKey::String(s) => s.as_bytes().to_vec(),
        };
        let lower = key.to_ascii_lowercase();
        let single_only = [
            "orig-date",
            "from",
            "sender",
            "reply-to",
            "cc",
            "bcc",
            "message-id",
            "references",
            "in-reply-to",
        ];
        if lower == b"to" {
            return Err(value_error(
                vm,
                "The additional headers cannot contain the \"To\" header".to_string(),
            ));
        }
        if lower == b"subject" {
            return Err(value_error(
                vm,
                "The additional headers cannot contain the \"Subject\" header".to_string(),
            ));
        }
        let v = v.deref();
        if let Some(t) = single_only
            .iter()
            .find(|t| t.as_bytes() == lower.as_slice())
        {
            if matches!(v, Value::Array(_)) {
                return Err(type_error(
                    vm,
                    format!("Header \"{}\" must be of type string, array given", t),
                ));
            }
        }
        build_header_elem(vm, &mut out, &key, &v)?;
    }
    if out.len() >= 2 {
        out.truncate(out.len() - 2);
    }
    Ok(out)
}

fn detect_multiple_crlf(hdr: &[u8]) -> bool {
    if hdr.is_empty() {
        return false;
    }
    let c = hdr[0];
    if !(33..=126).contains(&c) || c == b':' {
        return true;
    }
    let at = |i: usize| -> u8 { hdr.get(i).copied().unwrap_or(0) };
    let mut i = 0;
    while i < hdr.len() && hdr[i] != 0 {
        if hdr[i] == b'\r' {
            if at(i + 1) == 0
                || at(i + 1) == b'\r'
                || (at(i + 1) == b'\n'
                    && (at(i + 2) == 0 || at(i + 2) == b'\n' || at(i + 2) == b'\r'))
            {
                return true;
            }
            i += 2;
        } else if hdr[i] == b'\n' {
            if at(i + 1) == 0 || at(i + 1) == b'\r' || at(i + 1) == b'\n' {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

/// php_escape_shell_cmd
fn escape_shell_cmd(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        match c {
            b'"' | b'\'' => {
                // escape quotes only if unpaired
                if s[i + 1..].contains(&c) {
                    let close = i + 1 + s[i + 1..].iter().position(|&x| x == c).unwrap();
                    out.extend_from_slice(&s[i..=close]);
                    i = close + 1;
                    continue;
                }
                out.push(b'\\');
                out.push(c);
            }
            b'#' | b'&' | b';' | b'`' | b'|' | b'*' | b'?' | b'~' | b'<' | b'>' | b'^' | b'('
            | b')' | b'[' | b']' | b'{' | b'}' | b'$' | b'\\' | b',' | 0x0A | 0xFF => {
                out.push(b'\\');
                out.push(c);
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// C: `php_mail`
pub fn php_mail(
    vm: &mut Vm,
    func: &str,
    to: &[u8],
    subject: &[u8],
    message: &[u8],
    headers: Option<&[u8]>,
    extra_cmd: Option<&[u8]>,
) -> bool {
    let sendmail_path = match ini_bytes(vm, "sendmail_path") {
        Some(p) if !p.is_empty() => p,
        _ => b"/usr/sbin/sendmail -t -i".to_vec(),
    };
    let cr_lf_mode = ini_bytes(vm, "mail.cr_lf_mode").filter(|v| !v.is_empty());
    let line_sep: &[u8] = match cr_lf_mode.as_deref() {
        Some(b"lf") | Some(b"mixed") | Some(b"os") => b"\n",
        _ => {
            if ini_bool(vm, "mail.mixed_lf_and_crlf") {
                b"\n"
            } else {
                b"\r\n"
            }
        }
    };
    if let Some(log) = ini_bytes(vm, "mail.log").filter(|v| !v.is_empty()) {
        let mut line = format!(
            "mail() on [{}:{}]: To: {} -- Headers: {} -- Subject: {}",
            vm.current_file,
            vm.current_line,
            String::from_utf8_lossy(to),
            String::from_utf8_lossy(headers.unwrap_or(b"")),
            String::from_utf8_lossy(subject)
        )
        .into_bytes();
        if headers.is_some() {
            for c in line.iter_mut() {
                if *c == b'\r' || *c == b'\n' {
                    *c = b' ';
                }
            }
        }
        if log != b"syslog" {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let days = now.div_euclid(86400);
            let secs = now.rem_euclid(86400);
            // civil_from_days
            let z = days + 719468;
            let era = z.div_euclid(146097);
            let doe = z - era * 146097;
            let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
            let months = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            let mut entry = format!(
                "[{:02}-{}-{:04} {:02}:{:02}:{:02} UTC] ",
                d,
                months[(m - 1) as usize],
                y,
                secs / 3600,
                (secs / 60) % 60,
                secs % 60
            )
            .into_bytes();
            entry.extend_from_slice(&line);
            entry.push(b'\n');
            let path = String::from_utf8_lossy(&log).to_string();
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                Ok(mut fh) => {
                    let _ = fh.write_all(&entry);
                }
                Err(e) => {
                    // php_stream_open_wrapper(REPORT_ERRORS)
                    let msg = e.to_string();
                    let msg = msg.split(" (os error").next().unwrap_or("").to_string();
                    vm.emit_warning(&format!(
                        "{}({}): Failed to open stream: {}",
                        func, path, msg
                    ));
                }
            }
        }
    }
    if vm.current_exception.is_some() {
        return false;
    }
    let mut hdr: Option<Vec<u8>> = headers.map(|h| h.to_vec());
    if ini_bool(vm, "mail.add_x_header") {
        let file = vm.current_file.clone();
        let base = std::path::Path::new(&file)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        let uid = unsafe { libc_getuid() };
        let mut x = format!("X-PHP-Originating-Script: {}:{}", uid, base).into_bytes();
        if let Some(h) = &hdr {
            if !h.is_empty() {
                x.extend_from_slice(line_sep);
                x.extend_from_slice(h);
            }
        }
        hdr = Some(x);
    }
    if let Some(h) = &hdr {
        if detect_multiple_crlf(h) {
            vm.emit_warning(&format!(
                "{}(): Multiple or malformed newlines found in additional_header",
                func
            ));
            return false;
        }
    }
    let mut cmd = sendmail_path.clone();
    if let Some(e) = extra_cmd {
        cmd.push(b' ');
        cmd.extend_from_slice(e);
    }
    let mut data: Vec<u8> = Vec::new();
    data.extend_from_slice(b"To: ");
    data.extend_from_slice(to);
    data.extend_from_slice(line_sep);
    data.extend_from_slice(b"Subject: ");
    data.extend_from_slice(subject);
    data.extend_from_slice(line_sep);
    if let Some(h) = &hdr {
        data.extend_from_slice(h);
        data.extend_from_slice(line_sep);
    }
    data.extend_from_slice(line_sep);
    if cr_lf_mode.as_deref() == Some(b"lf") {
        let mut i = 0;
        while i < message.len() {
            if i + 1 < message.len() && message[i] == b'\r' && message[i + 1] == b'\n' {
                data.push(b'\n');
                i += 2;
            } else {
                data.push(message[i]);
                i += 1;
            }
        }
    } else {
        data.extend_from_slice(message);
    }
    data.extend_from_slice(line_sep);
    let cmd_s = String::from_utf8_lossy(&cmd).to_string();
    let child = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&cmd_s)
        .stdin(std::process::Stdio::piped())
        .spawn();
    match child {
        Ok(mut c) => {
            if let Some(mut stdin) = c.stdin.take() {
                let _ = stdin.write_all(&data);
            }
            match c.wait() {
                Ok(st) => {
                    use std::os::unix::process::ExitStatusExt;
                    if let Some(sig) = st.signal() {
                        let name = match sig {
                            1 => "Hangup",
                            2 => "Interrupt",
                            3 => "Quit",
                            6 => "Aborted",
                            9 => "Killed",
                            13 => "Broken pipe",
                            14 => "Alarm clock",
                            15 => "Terminated",
                            _ => "Unknown signal",
                        };
                        vm.emit_warning(&format!(
                            "{}(): Sendmail killed by signal {} ({})",
                            func, sig, name
                        ));
                        return false;
                    }
                    let ret = st.code().unwrap_or(-1);
                    // EX_OK (0) and EX_TEMPFAIL (75) count as success
                    if ret != 0 && ret != 75 {
                        vm.emit_warning(&format!(
                            "{}(): Sendmail exited with non-zero exit code {}",
                            func, ret
                        ));
                        false
                    } else {
                        true
                    }
                }
                Err(_) => false,
            }
        }
        Err(_) => {
            vm.emit_warning(&format!(
                "{}(): Could not execute mail delivery program '{}'",
                func,
                goro_core::string::msg_str(&sendmail_path)
            ));
            false
        }
    }
}

unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

fn path_arg(
    vm: &mut Vm,
    func: &str,
    args: &[Value],
    idx: usize,
    pname: &str,
) -> Result<Vec<u8>, VmError> {
    let s = str_arg(vm, func, args, idx, pname)?;
    if s.contains(&0) {
        return Err(arg_value_error(
            vm,
            func,
            idx + 1,
            pname,
            "must not contain any null bytes",
        ));
    }
    Ok(s)
}

fn mb_send_mail(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_send_mail";
    state::sync(vm);
    let lang = state::no2language(state::with(|s| s.language));
    let mut tran_cs: &'static Encoding =
        mbfl::no2encoding(lang.mail_charset).unwrap_or(&mbfl::basic::ENC_UTF8);
    let head_enc: &'static Encoding =
        mbfl::no2encoding(lang.mail_header_encoding).unwrap_or(&mbfl::basic::ENC_8BIT);
    let mut body_enc: &'static Encoding =
        mbfl::no2encoding(lang.mail_body_encoding).unwrap_or(&mbfl::basic::ENC_8BIT);

    let to = path_arg(vm, f, args, 0, "to")?;
    let subject = path_arg(vm, f, args, 1, "subject")?;
    let message = path_arg(vm, f, args, 2, "message")?;
    let mut str_headers: Option<Vec<u8>> = None;
    match args.get(3).map(|v| v.deref()) {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) => {
            str_headers = Some(build_headers(vm, &Value::Array(a))?);
        }
        Some(_) => {
            let h = str_arg(vm, f, args, 3, "additional_headers")?;
            if h.contains(&0) {
                return Err(arg_value_error(
                    vm,
                    f,
                    4,
                    "additional_headers",
                    "must not contain any null bytes",
                ));
            }
            // php_trim(..., 2): right trim of default whitespace
            let mut end = h.len();
            while end > 0 && matches!(h[end - 1], b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0) {
                end -= 1;
            }
            str_headers = Some(h[..end].to_vec());
        }
    }
    let extra_cmd = match str_or_null_arg(vm, f, args, 4, "additional_params")? {
        Some(e) => {
            if e.contains(&0) {
                return Err(arg_value_error(
                    vm,
                    f,
                    5,
                    "additional_params",
                    "must not contain any null bytes",
                ));
            }
            Some(e)
        }
        None => None,
    };

    let ht = str_headers
        .as_deref()
        .map(parse_mail_headers)
        .unwrap_or_default();
    let find = |name: &[u8]| ht.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
    let mut suppress_content_type = false;
    let mut suppress_cte = false;
    if let Some(ct) = find(b"content-type") {
        if let Some(semi) = ct.iter().position(|&c| c == b';') {
            let mut p = semi + 1;
            while p < ct.len() && (ct[p] == b' ' || ct[p] == b'\t') {
                p += 1;
            }
            if p < ct.len() {
                let rest = &ct[p..];
                let mut parts = rest
                    .split(|&c| c == b'=' || c == b' ')
                    .filter(|x| !x.is_empty());
                if let Some(param) = parts.next() {
                    if param.eq_ignore_ascii_case(b"charset") {
                        // charset = strtok(NULL, "= \"")
                        let after = &rest[param.len()..];
                        let cs: Vec<u8> = after
                            .split(|&c| c == b'=' || c == b' ' || c == b'"')
                            .find(|x| !x.is_empty())
                            .map(|x| x.to_vec())
                            .unwrap_or_default();
                        let enc = if cs.is_empty() {
                            Some(tran_cs)
                        } else {
                            mbfl::name2encoding(&cs)
                        };
                        tran_cs = match enc {
                            Some(e) => e,
                            None => {
                                vm.emit_warning(&format!(
                                    "mb_send_mail(): Unsupported charset \"{}\" - will be regarded as ascii",
                                    goro_core::string::msg_str(&cs)
                                ));
                                &mbfl::singlebyte::ENC_ASCII
                            }
                        };
                    }
                }
            }
        }
        suppress_content_type = true;
    }
    if let Some(cte) = find(b"content-transfer-encoding") {
        let e = mbfl::name2encoding(&cte);
        match e.map(|e| e.no) {
            Some(EncNo::Base64) | Some(EncNo::SevenBit) | Some(EncNo::EightBit) => {
                body_enc = e.unwrap()
            }
            _ => {
                vm.emit_warning(&format!(
                    "mb_send_mail(): Unsupported transfer encoding \"{}\" - will be regarded as 8bit",
                    goro_core::string::msg_str(&cte)
                ));
                body_enc = &mbfl::basic::ENC_8BIT;
            }
        }
        suppress_cte = true;
    }

    // To:
    let mut to_r = to.clone();
    while let Some(&c) = to_r.last() {
        if c.is_ascii_whitespace() || c == 0x0B {
            to_r.pop();
        } else {
            break;
        }
    }
    let mut i = 0;
    while i < to_r.len() {
        if to_r[i] < 0x20 || to_r[i] == 0x7F {
            if to_r[i] == b'\r'
                && to_r.get(i + 1) == Some(&b'\n')
                && matches!(to_r.get(i + 2), Some(b' ') | Some(b'\t'))
            {
                i += 2;
                while matches!(to_r.get(i + 1), Some(b' ') | Some(b'\t')) {
                    i += 1;
                }
                i += 1;
                continue;
            }
            to_r[i] = b' ';
        }
        i += 1;
    }

    // Subject:
    let mut enc = state::current_internal_encoding();
    let detect = state::with(|s| (s.current_detect_order_list.clone(), s.strict_detection));
    if enc.no == EncNo::Pass {
        enc = guess_encoding(&[&subject], &detect.0, detect.1, false).unwrap_or(enc);
    }
    let line_sep: &[u8] = if ini_bool(vm, "mail.mixed_lf_and_crlf") {
        b"\n"
    } else {
        b"\r\n"
    };
    let indent = ("Subject: [PHP-jp nnnnnnnn]".len() + line_sep.len()) as i64;
    let subject_enc = if tran_cs.from_wchar.is_some() && tran_cs.mime_name.is_some() {
        mime_header_encode(
            &subject,
            enc,
            tran_cs,
            head_enc.no == EncNo::Base64,
            line_sep,
            indent,
        )
    } else {
        subject.clone()
    };

    // message body
    let mut msg_enc = state::current_internal_encoding();
    if msg_enc.no == EncNo::Pass {
        msg_enc = guess_encoding(&[&message], &detect.0, detect.1, false).unwrap_or(msg_enc);
    }
    let (tmp, _) = mbfl::fast_convert(
        &message,
        msg_enc,
        tran_cs,
        b'?' as u32,
        mbfl::MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR,
    );
    let (conv, _) = mbfl::fast_convert(
        &tmp,
        &mbfl::basic::ENC_8BIT,
        body_enc,
        b'?' as u32,
        mbfl::MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR,
    );

    // other headers
    let mut s: Vec<u8> = Vec::new();
    let mut empty = true;
    if let Some(h) = &str_headers {
        if !h.is_empty() {
            let mut len = h.len();
            if h[len - 1] == b'\n' {
                len -= 1;
            }
            if len > 0 && h[len - 1] == b'\r' {
                len -= 1;
            }
            s.extend_from_slice(&h[..len]);
            empty = false;
        }
    }
    if find(b"mime-version").is_none() {
        if !empty {
            s.extend_from_slice(line_sep);
        }
        s.extend_from_slice(b"MIME-Version: 1.0");
        empty = false;
    }
    if !suppress_content_type {
        if !empty {
            s.extend_from_slice(line_sep);
        }
        s.extend_from_slice(b"Content-Type: text/plain");
        if let Some(p) = mbfl::preferred_mime_name(tran_cs) {
            s.extend_from_slice(b"; charset=");
            s.extend_from_slice(p.as_bytes());
        }
        empty = false;
    }
    if !suppress_cte {
        if !empty {
            s.extend_from_slice(line_sep);
        }
        s.extend_from_slice(b"Content-Transfer-Encoding: ");
        s.extend_from_slice(
            mbfl::preferred_mime_name(body_enc)
                .unwrap_or("7bit")
                .as_bytes(),
        );
    }

    let force = ini_bytes(vm, "mail.force_extra_parameters").filter(|v| !v.is_empty());
    let extra = match (force, extra_cmd) {
        (Some(fe), _) => Some(escape_shell_cmd(&fe)),
        (None, Some(e)) => Some(escape_shell_cmd(&e)),
        _ => None,
    };
    Ok(vbool(php_mail(
        vm,
        f,
        &to_r,
        &subject_enc,
        &conv,
        Some(&s),
        extra.as_deref(),
    )))
}
