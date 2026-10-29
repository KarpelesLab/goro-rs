//! mail() through the sendmail_path program (PHP manual: mail(), "Mail
//! Runtime Configuration"; header checks and line endings from tests
//! ext/standard/tests/mail/*.phpt and observed on PHP 8.5).

use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::ExitStatusExt;

use goro_runtime::{KeyRef, PhpStr, Value};

use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[(
        "function mail(string $to, string $subject, string $message, array|string $additional_headers = [], string $additional_params = \"\"): bool",
        mail,
    )],
    ini: &[
        ("mail.log", ""),
        ("mail.force_extra_parameters", ""),
        ("mail.cr_lf_mode", "crlf"),
        ("mail.mixed_lf_and_crlf", "0"),
        ("sendmail_from", ""),
    ],
    on_ini_set: Some(on_ini_set),
    listed: false,
    ..Module::EMPTY
};

/// sendmail_path and the line ending settings are startup-only.
fn on_ini_set(_vm: &mut Vm, k: &[u8], _v: &[u8]) -> R<bool> {
    Ok(!matches!(
        k,
        b"sendmail_path" | b"mail.cr_lf_mode" | b"mail.mixed_lf_and_crlf"
    ))
}

/// Headers that may appear once only (an array value is refused).
const SINGLE: &[&str] = &[
    "orig-date",
    "from",
    "sender",
    "reply-to",
    "to",
    "cc",
    "bcc",
    "message-id",
    "in-reply-to",
    "subject",
];

fn c_str(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// Checks a header value given in an array: line breaks are only allowed
/// as folding (CRLF then space or tab).
fn check_value(vm: &mut Vm, name: &[u8], v: &[u8]) -> R<()> {
    let name = c_str(name);
    let mut i = 0;
    while i < v.len() {
        let what = match v[i] {
            b'\r' if v.get(i + 1) == Some(&b'\n') => {
                if matches!(v.get(i + 2), Some(b' ' | b'\t')) {
                    i += 3;
                    continue;
                }
                "contains CRLF characters that are used as a line separator and are not allowed in the header"
            }
            b'\r' => "contains CR character that is not allowed in the header",
            b'\n' => "contains LF character that is not allowed in the header",
            0 => "contains NULL character that is not allowed in the header",
            _ => {
                i += 1;
                continue;
            }
        };
        return vm.throw("ValueError", &format!("Header \"{name}\" {what}"));
    }
    Ok(())
}

fn value_name(v: &Value) -> String {
    Vm::type_name_given(v)
}

/// The header lines of an `$additional_headers` array.
fn headers_from_array(vm: &mut Vm, arr: &goro_runtime::PhpArray) -> R<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let add = |out: &mut Vec<u8>, k: &[u8], v: &[u8]| {
        if !out.is_empty() {
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(k);
        out.extend_from_slice(b": ");
        out.extend_from_slice(v);
    };
    for (k, v) in arr.iter() {
        let name = match k {
            KeyRef::Int(n) => {
                return vm.throw(
                    "TypeError",
                    &format!("Header name cannot be numeric, {n} given"),
                );
            }
            KeyRef::Str(s) => s.to_vec(),
        };
        if name.is_empty() || name.iter().any(|&c| !(33..=126).contains(&c) || c == b':') {
            return vm.throw(
                "ValueError",
                &format!(
                    "Header name \"{}\" contains invalid characters",
                    c_str(&name)
                ),
            );
        }
        let lower = name.to_ascii_lowercase();
        if lower == b"to" {
            return vm.throw(
                "ValueError",
                "The additional headers cannot contain the \"To\" header",
            );
        }
        if lower == b"subject" {
            return vm.throw(
                "ValueError",
                "The additional headers cannot contain the \"Subject\" header",
            );
        }
        let single = SINGLE.iter().any(|h| h.as_bytes() == lower.as_slice());
        let v = v.deref();
        match &v {
            Value::Str(s) => {
                check_value(vm, &name, s)?;
                add(&mut out, &name, s);
            }
            Value::Array(list) if !single => {
                for (ik, iv) in list.iter() {
                    if let KeyRef::Str(s) = ik {
                        return vm.throw(
                            "TypeError",
                            &format!(
                                "Header \"{}\" must only contain numeric keys, \"{}\" found",
                                c_str(&name),
                                s.to_string_lossy()
                            ),
                        );
                    }
                    let iv = iv.deref();
                    let Value::Str(s) = &iv else {
                        return vm.throw(
                            "TypeError",
                            &format!(
                                "Header \"{}\" must only contain values of type string, {} found",
                                c_str(&name),
                                value_name(&iv)
                            ),
                        );
                    };
                    check_value(vm, &name, s)?;
                    add(&mut out, &name, s);
                }
            }
            other => {
                let want = if single { "string" } else { "array|string" };
                return vm.throw(
                    "TypeError",
                    &format!(
                        "Header \"{}\" must be of type {want}, {} given",
                        c_str(&name),
                        value_name(other)
                    ),
                );
            }
        }
    }
    Ok(out)
}

/// A headers string is refused when it starts with a line break or has
/// several in a row (CRLF, LF and CR each count as one).
fn malformed(h: &[u8]) -> bool {
    if matches!(h.first(), Some(b'\r' | b'\n')) {
        return true;
    }
    let mut i = 0;
    while i < h.len() {
        let next = match h[i] {
            b'\r' if h.get(i + 1) == Some(&b'\n') => i + 2,
            b'\r' | b'\n' => i + 1,
            _ => {
                i += 1;
                continue;
            }
        };
        if matches!(h.get(next), Some(b'\r' | b'\n')) {
            return true;
        }
        i = next;
    }
    false
}

fn spaced(b: &[u8]) -> Vec<u8> {
    b.iter()
        .map(|&c| if c == b'\r' || c == b'\n' { b' ' } else { c })
        .collect()
}

fn ini(vm: &Vm, k: &[u8]) -> Vec<u8> {
    vm.get_ini(k).map(|v| v.to_vec()).unwrap_or_default()
}

fn ini_on(vm: &Vm, k: &[u8]) -> bool {
    let v = ini(vm, k).to_ascii_lowercase();
    matches!(v.as_slice(), b"1" | b"on" | b"yes" | b"true")
}

fn write_log(vm: &mut Vm, a: &Args, to: &[u8], headers: &[u8], subject: &[u8]) -> R<()> {
    let path = ini(vm, b"mail.log");
    if path.is_empty() || path == b"syslog" {
        return Ok(());
    }
    let stamp = vm.call_value(
        &Value::Str(PhpStr::from("date")),
        vec![Value::Str(PhpStr::from("d-M-Y H:i:s e"))],
    )?;
    let stamp = vm.to_php_str(&stamp)?;
    let mut line = Vec::new();
    line.push(b'[');
    line.extend_from_slice(&stamp);
    line.extend_from_slice(b"] mail() on [");
    line.extend_from_slice(&vm.cur_file());
    line.extend_from_slice(format!(":{}]: To: ", vm.cur_line()).as_bytes());
    line.extend_from_slice(&spaced(to));
    line.extend_from_slice(b" -- Headers: ");
    line.extend_from_slice(&spaced(headers));
    line.extend_from_slice(b" -- Subject: ");
    line.extend_from_slice(&spaced(subject));
    line.push(b'\n');
    if let Some(mut s) = super::file::open_stream(vm, a, &path, b"a", false)? {
        let _ = s.write_all(vm, &line);
    }
    Ok(())
}

fn mail(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let names = [
        "to",
        "subject",
        "message",
        "additional_headers",
        "additional_params",
    ];
    for (i, n) in names.iter().enumerate() {
        if let Some(Value::Str(s)) = a.get(i).map(|v| v.deref())
            && s.contains(&0)
        {
            return vm.fthrow(
                a,
                "ValueError",
                &format!("Argument #{} (${n}) must not contain any null bytes", i + 1),
            );
        }
    }
    let to = spaced(a.bytes(0));
    let subject = spaced(a.bytes(1));
    let message = a.str(2);
    let mut headers = match a.val(3).deref() {
        Value::Array(arr) => headers_from_array(vm, &arr)?,
        Value::Str(s) => s.to_vec(),
        _ => Vec::new(),
    };
    while matches!(headers.last(), Some(b'\r' | b'\n')) {
        headers.pop();
    }
    if malformed(&headers) {
        vm.fwarn(
            a,
            "Multiple or malformed newlines found in additional_header",
        )?;
        return Ok(Value::Bool(false));
    }
    write_log(vm, a, &to, &headers, &subject)?;

    // the command: sendmail_path, then the (escaped) extra parameters
    let mut cmd = ini(vm, b"sendmail_path");
    let forced = ini(vm, b"mail.force_extra_parameters");
    let extra = if !forced.is_empty() {
        forced
    } else {
        a.opt_str(4).map(|s| s.to_vec()).unwrap_or_default()
    };
    if !extra.is_empty() {
        cmd.push(b' ');
        cmd.extend_from_slice(&super::misc::escape_cmd(&extra));
    }

    let mode = ini(vm, b"mail.cr_lf_mode");
    let mode = if ini_on(vm, b"mail.mixed_lf_and_crlf") {
        b"mixed".to_vec()
    } else {
        mode
    };
    let eol: &[u8] = match mode.as_slice() {
        b"lf" | b"mixed" | b"os" => b"\n",
        _ => b"\r\n",
    };
    let mut body = Vec::new();
    for (k, v) in [(&b"To: "[..], &to), (b"Subject: ", &subject)] {
        body.extend_from_slice(k);
        body.extend_from_slice(v);
        body.extend_from_slice(eol);
    }
    if ini_on(vm, b"mail.add_x_header") {
        let file = vm.cur_file();
        let uid = std::fs::metadata(std::ffi::OsStr::from_bytes(&file))
            .map(|m| std::os::unix::fs::MetadataExt::uid(&m))
            .unwrap_or_else(|_| unsafe { libc::getuid() });
        body.extend_from_slice(format!("X-PHP-Originating-Script: {uid}:").as_bytes());
        body.extend_from_slice(&super::file::basename_bytes(&file, b""));
        body.extend_from_slice(eol);
    }
    if !headers.is_empty() {
        body.extend_from_slice(&headers);
        body.extend_from_slice(eol);
    }
    body.extend_from_slice(eol);
    if matches!(mode.as_slice(), b"lf" | b"os") {
        let mut i = 0;
        while i < message.len() {
            if message[i] == b'\r' && message.get(i + 1) == Some(&b'\n') {
                i += 1;
                continue;
            }
            body.push(message[i]);
            i += 1;
        }
    } else {
        body.extend_from_slice(&message);
    }
    body.extend_from_slice(eol);

    vm.out.flush();
    let child = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(std::ffi::OsStr::from_bytes(&cmd))
        .current_dir(&vm.cwd)
        .stdin(std::process::Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(_) => {
            let msg = format!(
                "Could not execute mail delivery program '{}'",
                String::from_utf8_lossy(&cmd)
            );
            vm.fwarn(a, &msg)?;
            return Ok(Value::Bool(false));
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&body);
    }
    let status = match child.wait() {
        Ok(s) => s,
        Err(_) => return Ok(Value::Bool(false)),
    };
    if let Some(sig) = status.signal() {
        let name = unsafe { std::ffi::CStr::from_ptr(libc::strsignal(sig)) }
            .to_string_lossy()
            .into_owned();
        vm.fwarn(a, &format!("Sendmail killed by signal {sig} ({name})"))?;
        return Ok(Value::Bool(false));
    }
    match status.code() {
        // EX_TEMPFAIL: the message is queued (observed on PHP 8.5)
        Some(0) | Some(75) => Ok(Value::Bool(true)),
        Some(c) => {
            vm.fwarn(a, &format!("Sendmail exited with non-zero exit code {c}"))?;
            Ok(Value::Bool(false))
        }
        None => Ok(Value::Bool(false)),
    }
}
