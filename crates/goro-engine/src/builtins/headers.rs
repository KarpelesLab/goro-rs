//! header(), http_response_code(), setcookie() and friends (PHP manual:
//! Network Functions), over the SAPI response state in `vm.out.sapi`.
//!
//! In the CLI only the status code is kept (observed on PHP 8.5:
//! headers_list() stays empty); the built-in web server keeps the header
//! list and sends it.

use goro_runtime::{PhpArray, PhpStr, Value};

use crate::registry::Module;
use crate::vm::sapi::{DAYS, MONTHS, civil, header_name, now};
use crate::{Args, R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function header(string $header, bool $replace = true, int $response_code = 0): void",
            header,
        ),
        (
            "function header_remove(?string $name = null): void",
            header_remove,
        ),
        ("function headers_list(): array", headers_list),
        (
            "function headers_sent(&$filename = null, &$line = null): bool",
            headers_sent,
        ),
        (
            "function http_response_code(int $response_code = 0): int|bool",
            http_response_code,
        ),
        (
            "function setcookie(string $name, string $value = \"\", array|int $expires_or_options = 0, string $path = \"\", string $domain = \"\", bool $secure = false, bool $httponly = false): bool",
            setcookie,
        ),
        (
            "function setrawcookie(string $name, string $value = \"\", array|int $expires_or_options = 0, string $path = \"\", string $domain = \"\", bool $secure = false, bool $httponly = false): bool",
            setrawcookie,
        ),
        (
            "function is_uploaded_file(string $filename): bool",
            is_uploaded_file,
        ),
        (
            "function move_uploaded_file(string $from, string $to): bool",
            move_uploaded_file,
        ),
    ],
    ..Module::EMPTY
};

/// Once output started, the web server's headers are final: they are
/// sent, with the default Content-type (test
/// sapi/cli/tests/php_cli_server_019).
pub fn commit(vm: &mut Vm) {
    if vm.out.written > 0 && vm.out.sapi.conn.is_some() {
        vm.out.flush();
    }
}

/// The headers can no longer change.
pub fn sent(vm: &Vm) -> bool {
    vm.out.written > 0 || vm.out.sapi.sent
}

/// Where the output started, for the "headers already sent" messages.
fn started_at(vm: &Vm) -> String {
    match &vm.out.sapi.output_start {
        Some((f, l)) => format!(" (output started at {}:{})", String::from_utf8_lossy(f), l),
        None => String::new(),
    }
}

fn warn_sent(vm: &mut Vm) -> R<()> {
    // (test ext/standard/tests/general_functions/head.phpt)
    let at = started_at(vm);
    let msg = if at.is_empty() {
        "Cannot modify header information - headers already sent".to_string()
    } else {
        format!(
            "Cannot modify header information - headers already sent by{}",
            at
        )
    };
    vm.warn(&msg)
}

/// Adds a header line as header() does; `Ok(false)` when it was refused.
pub fn add_header(vm: &mut Vm, line: &[u8], replace: bool, code: i64) -> R<bool> {
    if sent(vm) {
        warn_sent(vm)?;
        return Ok(false);
    }
    let given_blank = !line.is_empty() && line.trim_ascii().is_empty();
    // Trailing whitespace goes first: a line break there is no second
    // header (test ext/standard/tests/general_functions/bug60227_1.phpt).
    let mut line = line.to_vec();
    while line.last().is_some_and(|c| c.is_ascii_whitespace()) {
        line.pop();
    }
    // (observed on PHP 8.5; NUL first: test
    // ext/standard/tests/general_functions/bug60227_3.phpt)
    if line.contains(&0) {
        vm.warn("Header may not contain NUL bytes")?;
        return Ok(false);
    }
    if line.contains(&b'\n') || line.contains(&b'\r') {
        vm.warn("Header may not contain more than a single header, new line detected")?;
        return Ok(false);
    }
    let s = &mut vm.out.sapi;
    if s.status == 0 && s.web() {
        s.status = 200;
    }
    if line.len() >= 5 && line[..5].eq_ignore_ascii_case(b"HTTP/") {
        // A status line (PHP manual: header()).
        let code = line
            .iter()
            .position(|&c| c == b' ')
            .map(|p| {
                let rest = &line[p + 1..];
                let end = rest
                    .iter()
                    .position(|c| !c.is_ascii_digit())
                    .unwrap_or(rest.len());
                goro_runtime::convert::str_to_int(&rest[..end])
            })
            .unwrap_or(0);
        if code > 0 {
            s.status = code;
        }
        s.status_line = Some(line);
        return Ok(true);
    }
    if line.trim_ascii().is_empty() {
        // header(" ") is accepted and does nothing visible (test
        // sapi/cli/tests/bug66830); the server keeps an empty entry
        // (observed on PHP 8.5).
        if s.web() && given_blank {
            s.headers.push(Vec::new());
        }
        return Ok(true);
    }
    let name = header_name(&line).to_vec();
    let has_colon = line.contains(&b':');
    if has_colon && name.eq_ignore_ascii_case(b"location") {
        // PHP manual: header(): a redirect unless a 201 or 3xx status is set.
        if s.status != 201 && !(300..400).contains(&s.status) {
            s.status = 302;
            // The status line goes with its code (test
            // ext/standard/tests/general_functions/header_redirection_002).
            s.status_line = None;
        }
    } else if has_colon && name.eq_ignore_ascii_case(b"www-authenticate") {
        // (test sapi/cli/tests/php_cli_server_007)
        s.status = 401;
    }
    if has_colon && replace && name.eq_ignore_ascii_case(b"content-type") {
        // The type set this way names the response's type; a text type
        // gets the default charset (observed on PHP 8.5: "text/plain"
        // becomes "Content-type: text/plain;charset=UTF-8").
        let p = line.iter().position(|&c| c == b':').unwrap();
        let mut v = line[p + 1..].trim_ascii().to_vec();
        let charset = vm.get_ini(b"default_charset").unwrap_or_default();
        let lower = v.to_ascii_lowercase();
        if lower.starts_with(b"text/")
            && !lower.windows(8).any(|w| w == b"charset=")
            && !charset.is_empty()
        {
            v.extend_from_slice(b";charset=");
            v.extend_from_slice(&charset);
        }
        vm.ext::<super::core_funcs::ResponseHeaders>().content_type = Some(PhpStr::from(v.clone()));
        let mut l = b"Content-type: ".to_vec();
        l.extend_from_slice(&v);
        line = l;
    }
    let s = &mut vm.out.sapi;
    if code > 0 {
        s.status = code;
    }
    if s.web() {
        if replace && has_colon {
            s.headers
                .retain(|h| !(h.contains(&b':') && header_name(h).eq_ignore_ascii_case(&name)));
        }
        s.headers.push(line);
    }
    Ok(true)
}

fn header(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let h = a.str(0);
    let replace = a.bool_or(1, true);
    let code = a.int_or(2, 0);
    add_header(vm, &h, replace, code)?;
    Ok(Value::Null)
}

fn header_remove(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if sent(vm) {
        warn_sent(vm)?;
        return Ok(Value::Null);
    }
    let s = &mut vm.out.sapi;
    match a.opt_str(0) {
        None => s.headers.clear(),
        Some(n) => {
            let n = n.trim_ascii().to_vec();
            s.headers
                .retain(|h| !header_name(h).eq_ignore_ascii_case(&n));
        }
    }
    Ok(Value::Null)
}

fn headers_list(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    commit(vm);
    let list = vm
        .out
        .sapi
        .headers
        .iter()
        .map(|h| Value::Str(PhpStr::from(h.clone())));
    Ok(Value::Array(PhpArray::from_values(list)))
}

fn headers_sent(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let is = sent(vm);
    let (f, l) = match (&vm.out.sapi.output_start, is) {
        (Some((f, l)), true) => (f.clone(), *l as i64),
        _ => (Vec::new(), 0),
    };
    if !a.is_empty() {
        a.set_ref(0, Value::Str(PhpStr::from(f)));
    }
    if a.len() > 1 {
        a.set_ref(1, Value::Int(l));
    }
    Ok(Value::Bool(is))
}

/// PHP manual: http_response_code(): false/true outside a web server
/// while no status was set (test
/// ext/standard/tests/general_functions/http_response_code.phpt).
fn http_response_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let code = a.int_or(0, 0);
    if code == 0 {
        let st = vm.out.sapi.status;
        return Ok(if st == 0 {
            Value::Bool(false)
        } else {
            Value::Int(st)
        });
    }
    if sent(vm) {
        let at = started_at(vm);
        vm.fwarn(
            a,
            &format!("Cannot set response code - headers already sent{}", at),
        )?;
        return Ok(Value::Bool(false));
    }
    if vm.out.sapi.status_line.is_some() {
        // The status line keeps its code; the code still changes (test
        // sapi/cli/tests/gh18582).
        vm.fwarn(
            a,
            "Calling http_response_code() after header('HTTP/...') has no effect",
        )?;
    }
    let prev = vm.out.sapi.status;
    vm.out.sapi.status = code;
    Ok(if prev == 0 {
        Value::Bool(true)
    } else {
        Value::Int(prev)
    })
}

const COOKIE_BAD: &[u8] = b",; \t\r\n\x0b\x0c";
const COOKIE_BAD_MSG: &str = r#"",", ";", " ", "\t", "\r", "\n", "\013", or "\014""#;

/// setcookie()/setrawcookie() (PHP manual: setcookie(); messages as in
/// tests ext/standard/tests/network/setcookie_error.phpt and
/// setcookie_array_option_error.phpt).
fn cookie(vm: &mut Vm, a: &mut Args, raw: bool) -> R<Value> {
    let f = a.fname();
    let name = a.str(0);
    let value = a.opt_str(1).unwrap_or_default();
    let mut expires = 0i64;
    let mut path = PhpStr::empty();
    let mut domain = PhpStr::empty();
    let mut secure = false;
    let mut httponly = false;
    let mut samesite = PhpStr::empty();
    let mut partitioned = false;
    match a.val(2) {
        Value::Array(opts) => {
            if a.len() > 3 {
                return Err(vm.throw_error_msg(
                    "ArgumentCountError",
                    &format!(
                        "{}(): Expects exactly 3 arguments when argument #3 ($expires_or_options) is an array",
                        f
                    ),
                ));
            }
            for (k, v) in opts.iter() {
                let goro_runtime::KeyRef::Str(k) = k else {
                    return vm
                        .value_error(&format!("{}(): option array cannot have numeric keys", f));
                };
                let lk = k.to_ascii_lowercase();
                match &lk[..] {
                    b"expires" => expires = vm.to_int(v)?,
                    b"path" => path = vm.to_php_str(v)?,
                    b"domain" => domain = vm.to_php_str(v)?,
                    b"secure" => secure = vm.to_bool(v),
                    b"httponly" => httponly = vm.to_bool(v),
                    b"samesite" => samesite = vm.to_php_str(v)?,
                    b"partitioned" => partitioned = vm.to_bool(v),
                    _ => {
                        return vm.value_error(&format!(
                            "{}(): option \"{}\" is invalid",
                            f,
                            k.to_string_lossy()
                        ));
                    }
                }
            }
        }
        v => {
            expires = vm.to_int(&v)?;
            path = a.opt_str(3).unwrap_or_default();
            domain = a.opt_str(4).unwrap_or_default();
            secure = a.bool_or(5, false);
            httponly = a.bool_or(6, false);
        }
    }
    if name.is_empty() {
        return vm.value_error(&format!("{}(): Argument #1 ($name) must not be empty", f));
    }
    if name.iter().any(|c| c == &b'=' || COOKIE_BAD.contains(c)) {
        return vm.value_error(&format!(
            r#"{}(): Argument #1 ($name) cannot contain "=", {}"#,
            f, COOKIE_BAD_MSG
        ));
    }
    if raw && value.iter().any(|c| COOKIE_BAD.contains(c)) {
        return vm.value_error(&format!(
            "{}(): Argument #2 ($value) cannot contain {}",
            f, COOKIE_BAD_MSG
        ));
    }
    if path.iter().any(|c| COOKIE_BAD.contains(c)) {
        return vm.value_error(&format!(
            "{}(): \"path\" option cannot contain {}",
            f, COOKIE_BAD_MSG
        ));
    }
    if domain.iter().any(|c| COOKIE_BAD.contains(c)) {
        return vm.value_error(&format!(
            "{}(): \"domain\" option cannot contain {}",
            f, COOKIE_BAD_MSG
        ));
    }
    if partitioned && !secure {
        return vm.value_error(&format!(
            "{}(): \"partitioned\" option cannot be used without \"secure\" option",
            f
        ));
    }
    let date = |t: i64| {
        let (y, mo, d, h, mi, s, wd) = civil(t);
        (
            y,
            format!(
                "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
                DAYS[wd as usize],
                d,
                MONTHS[mo as usize - 1],
                y,
                h,
                mi,
                s
            ),
        )
    };
    let mut h = b"Set-Cookie: ".to_vec();
    h.extend_from_slice(&name);
    h.push(b'=');
    if value.is_empty() {
        // An empty value deletes the cookie (setcookie.phpt).
        h.extend_from_slice(b"deleted; expires=");
        h.extend_from_slice(date(1).1.as_bytes());
        h.extend_from_slice(b"; Max-Age=0");
    } else {
        if raw {
            h.extend_from_slice(&value);
        } else {
            h.extend_from_slice(&goro_ext_standard_rawurlencode(&value));
        }
        if expires != 0 {
            let (y, d) = date(expires);
            if y > 9999 {
                return vm.value_error(&format!(
                    "{}(): \"expires\" option cannot have a year greater than 9999",
                    f
                ));
            }
            h.extend_from_slice(b"; expires=");
            h.extend_from_slice(d.as_bytes());
            let age = (expires - now()).max(0);
            h.extend_from_slice(format!("; Max-Age={}", age).as_bytes());
        }
    }
    if !path.is_empty() {
        h.extend_from_slice(b"; path=");
        h.extend_from_slice(&path);
    }
    if !domain.is_empty() {
        h.extend_from_slice(b"; domain=");
        h.extend_from_slice(&domain);
    }
    if secure {
        h.extend_from_slice(b"; secure");
    }
    if httponly {
        h.extend_from_slice(b"; HttpOnly");
    }
    if !samesite.is_empty() {
        h.extend_from_slice(b"; SameSite=");
        h.extend_from_slice(&samesite);
    }
    if partitioned {
        h.extend_from_slice(b"; Partitioned");
    }
    Ok(Value::Bool(add_header(vm, &h, false, 0)?))
}

/// RFC 3986 percent-encoding as rawurlencode() does.
fn goro_ext_standard_rawurlencode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c);
        } else {
            out.extend_from_slice(format!("%{:02X}", c).as_bytes());
        }
    }
    out
}

fn setcookie(vm: &mut Vm, a: &mut Args) -> R<Value> {
    cookie(vm, a, false)
}

fn setrawcookie(vm: &mut Vm, a: &mut Args) -> R<Value> {
    cookie(vm, a, true)
}

fn is_uploaded_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    Ok(Value::Bool(
        vm.out.sapi.uploads.iter().any(|u| u[..] == f[..]),
    ))
}

/// PHP manual: move_uploaded_file(): only files uploaded with this request.
fn move_uploaded_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    use std::os::unix::ffi::OsStrExt;
    let from = a.str(0);
    let to = a.str(1);
    let Some(i) = vm.out.sapi.uploads.iter().position(|u| u[..] == from[..]) else {
        return Ok(Value::Bool(false));
    };
    let src = std::path::Path::new(std::ffi::OsStr::from_bytes(&from));
    let dst = std::path::Path::new(std::ffi::OsStr::from_bytes(&to));
    let dst = if dst.is_absolute() {
        dst.to_path_buf()
    } else {
        vm.cwd.join(dst)
    };
    let ok = std::fs::rename(src, &dst).is_ok()
        || (std::fs::copy(src, &dst).is_ok() && std::fs::remove_file(src).is_ok());
    if ok {
        vm.out.sapi.uploads.remove(i);
    } else {
        vm.fwarn(
            a,
            &format!(
                "Unable to move \"{}\" to \"{}\"",
                from.to_string_lossy(),
                to.to_string_lossy()
            ),
        )?;
    }
    Ok(Value::Bool(ok))
}
