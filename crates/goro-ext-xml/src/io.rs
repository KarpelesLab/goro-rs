//! Resource loading/saving for the XML extensions (libxml2's I/O layer as
//! seen through PHP's stream wrappers).

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

pub fn strip_file_scheme(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("file://") {
        // file:///abs/path or file://localhost/abs/path
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        return percent_decode(rest);
    }
    path.to_string()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Read a resource (local file or PHP stream wrapper) completely.
fn percent_nul(vm: &mut Vm, filename: &str) -> bool {
    if filename.contains("%00") {
        let f = crate::dom::active_fn(vm);
        vm.emit_warning(&format!(
            "{}(): URI must not contain percent-encoded NUL bytes",
            f
        ));
        return true;
    }
    false
}

thread_local! {
    static REPORTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the last failed read_resource() already reported its error
/// (user entity loader), so callers must not add an I/O warning.
pub fn last_failure_reported() -> bool {
    REPORTED.with(|r| r.get())
}

thread_local! {
    /// The charset the transport (HTTP Content-Type) gave the resource the
    /// last read_resource() read (see `take_transport_charset`).
    static TRANSPORT_CHARSET: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The charset of the resource read last, from its transport headers
/// (php_libxml_sniff_charset_from_stream()), when the parser knows it.
pub fn take_transport_charset() -> Option<String> {
    TRANSPORT_CHARSET.with(|c| c.borrow_mut().take())
}

/// The charset the response headers of the HTTP(S) URL `url` just read
/// declare (php_libxml_sniff_charset_from_stream() on its wrapper data).
pub fn transport_charset(vm: &mut Vm, url: &str) -> Option<Vec<u8>> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return None;
    }
    // (the wrapper data of the stream, as http_get_last_response_headers()
    // reports it)
    let last = match vm
        .functions
        .get(b"http_get_last_response_headers".as_slice())
    {
        Some(&f) => f(vm, &[]).ok(),
        None => None,
    };
    let headers: Vec<Vec<u8>> = match last.map(|v| v.deref()) {
        Some(Value::Array(a)) => a
            .borrow()
            .iter()
            .filter_map(|(_, v)| match v.deref() {
                Value::String(s) => Some(s.as_bytes().to_vec()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    sniff_charset_from_headers(&headers)
}

/// php_libxml_sniff_charset_from_stream(): the charset parameter of the
/// last Content-Type header (scanning back to the status line).
pub fn sniff_charset_from_headers(headers: &[Vec<u8>]) -> Option<Vec<u8>> {
    for h in headers.iter().rev() {
        let colon = h.iter().position(|&c| c == b':');
        let space = h.iter().position(|&c| c == b' ');
        match (colon, space) {
            (None, _) => return None,
            (Some(c), Some(sp)) if sp < c => return None,
            _ => {}
        }
        if h.len() >= 13 && h[..13].eq_ignore_ascii_case(b"content-type:") {
            return sniff_charset_from_string(&h[13..]);
        }
    }
    None
}

/// php_libxml_sniff_charset_from_string() (MIME type parsing, charset only)
pub fn sniff_charset_from_string(s: &[u8]) -> Option<Vec<u8>> {
    let ws = |c: u8| matches!(c, 0x0A | 0x0D | 0x09 | 0x20);
    let token = |c: u8| {
        c == 0x21
            || (0x23..=0x27).contains(&c)
            || matches!(
                c,
                0x2A | 0x2B | 0x2D | 0x2E | 0x5E | 0x5F | 0x60 | 0x7C | 0x7E
            )
            || c.is_ascii_alphanumeric()
    };
    let mut start = 0;
    let mut end = s.len();
    while start < end && ws(s[start]) {
        start += 1;
    }
    while start < end && ws(s[end - 1]) {
        end -= 1;
    }
    let collect = |pos: usize, f: &dyn Fn(u8) -> bool| {
        let mut p = pos;
        while p < end && f(s[p]) {
            p += 1;
        }
        p - pos
    };
    let type_len = collect(start, &|c| c != b'/');
    if type_len == 0 || !s[start..start + type_len].iter().all(|&c| token(c)) {
        return None;
    }
    start += type_len;
    if start >= end {
        return None;
    }
    start += 1;
    let sub_len = collect(start, &|c| c != b';');
    if sub_len == 0 || !s[start..start + sub_len].iter().all(|&c| token(c)) {
        return None;
    }
    start += sub_len;
    while start < end {
        start += 1;
        start += collect(start, &ws);
        let name_len = collect(start, &|c| c != b';' && c != b'=');
        let name = &s[start..start + name_len];
        start += name_len;
        if start < end {
            if s[start] == b';' {
                continue;
            }
            start += 1;
        } else {
            break;
        }
        let value: Vec<u8> = if start < end && s[start] == b'"' {
            // collect an HTTP quoted string (extract-value)
            let mut v = Vec::new();
            start += 1;
            loop {
                let n = collect(start, &|c| c != b'"' && c != b'\\');
                v.extend_from_slice(&s[start..start + n]);
                start += n;
                if start >= end {
                    break;
                }
                let q = s[start];
                start += 1;
                if q == b'\\' {
                    if start >= end {
                        v.push(b'\\');
                        break;
                    }
                    v.push(s[start]);
                    start += 1;
                } else {
                    break;
                }
            }
            start += collect(start, &|c| c != b';');
            v
        } else {
            let n = collect(start, &|c| c != b';');
            let mut v = s[start..start + n].to_vec();
            // (php-src advances by the parameter name's length here)
            start += name_len;
            while v.last().is_some_and(|&c| ws(c)) {
                v.pop();
            }
            if v.is_empty() {
                continue;
            }
            v
        };
        if name.eq_ignore_ascii_case(b"charset")
            && value
                .iter()
                .all(|&c| c == 0x09 || (0x20..0x7F).contains(&c) || c > 0x7F)
        {
            return Some(value);
        }
    }
    None
}

pub fn read_resource(vm: &mut Vm, filename: &str) -> Option<Vec<u8>> {
    REPORTED.with(|r| r.set(false));
    TRANSPORT_CHARSET.with(|c| *c.borrow_mut() = None);
    if percent_nul(vm, filename) {
        return None;
    }
    if let Some(r) =
        crate::libxml::user_entity_load(vm, None, Some(filename), [None, None, None, None])
    {
        if r.is_none() {
            REPORTED.with(|x| x.set(true));
        }
        return r;
    }
    // php_libxml_input_buffer_create_filename()
    if crate::libxml::entity_loader_disabled() {
        return None;
    }
    let path = strip_file_scheme(filename);
    let lower = filename.to_ascii_lowercase();
    if lower == "php://memory" || lower.starts_with("php://temp") {
        // a fresh memory stream is always empty
        return Some(Vec::new());
    }
    if path.contains("://") || path.starts_with("php://") || path.starts_with("data:") {
        // go through file_get_contents (stream wrappers)
        let f = vm.functions.get(&b"file_get_contents"[..]).copied()?;
        let silenced = vm.error_reporting;
        vm.error_reporting = goro_core::errdisplay::ER_SILENT;
        // php_libxml_streams_IO_open_wrapper(): a wrapper with url_stat (a
        // user wrapper, phar://, ftp://) is stat()ed quietly first, and a
        // failed stat is a failed open
        if wrapper_has_url_stat(&lower) {
            let exists = vm.functions.get(&b"file_exists"[..]).copied().map(|fe| {
                fe(
                    vm,
                    &[Value::String(PhpString::from_string(filename.to_string()))],
                )
            });
            if !matches!(exists, Some(Ok(Value::True))) {
                vm.error_reporting = silenced;
                return None;
            }
        }
        let mut fargs = vec![Value::String(PhpString::from_string(filename.to_string()))];
        if let Some(ctx) = crate::libxml::streams_context() {
            fargs.push(Value::False);
            fargs.push(ctx);
        }
        let r = f(vm, &fargs);
        vm.error_reporting = silenced;
        let cs = transport_charset(vm, filename)
            .map(|c| String::from_utf8_lossy(&c).into_owned())
            .filter(|c| crate::parser::decode_to_utf8(b"", c).is_some());
        TRANSPORT_CHARSET.with(|c| *c.borrow_mut() = cs);
        return match r {
            Ok(Value::String(s)) => Some(s.as_bytes().to_vec()),
            _ => None,
        };
    }
    // php_libxml_streams_IO_open_wrapper: a scheme-less or file: URI is
    // unescaped (xmlURIUnescapeString) before it is opened
    let path = match crate::uri::parse_uri(filename) {
        Some(u)
            if u.scheme.as_deref().is_none_or(|s| s.starts_with("file")) && path.contains('%') =>
        {
            strip_file_scheme(&uri_unescape(filename))
        }
        _ => path,
    };
    // The plain files wrapper (without REPORT_ERRORS) checks open_basedir.
    if !goro_core::basedir::check_open_basedir_ex(vm, "", &path, false) {
        return None;
    }
    let full = resolve_path(vm, &path);
    std::fs::read(&full).ok()
}

/// Whether the wrapper of `url` (lowercase) has a url_stat operation: the
/// built-in wrappers without one are http(s), php, data, glob, zip and the
/// compression ones.
fn wrapper_has_url_stat(url: &str) -> bool {
    let Some(i) = url
        .find("://")
        .or_else(|| url.find(':').filter(|_| url.starts_with("data:")))
    else {
        return false;
    };
    !matches!(
        &url[..i],
        "http"
            | "https"
            | "php"
            | "data"
            | "glob"
            | "zip"
            | "compress.zlib"
            | "compress.bzip2"
            | "file"
    )
}

/// xmlURIUnescapeString
pub fn uri_unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit()
        {
            let h = (b[i + 1] as char).to_digit(16).unwrap_or(0);
            let l = (b[i + 2] as char).to_digit(16).unwrap_or(0);
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn resolve_path(_vm: &Vm, path: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(path);
    if p.is_absolute() {
        return p;
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(p),
        Err(_) => p,
    }
}

/// The document URL libxml2 records for a loaded file.
pub fn resource_url(filename: &str) -> String {
    filename.to_string()
}

/// Write bytes to a resource. Returns success.
pub fn write_resource(vm: &mut Vm, filename: &str, data: &[u8]) -> bool {
    if percent_nul(vm, filename) {
        return false;
    }
    let path = strip_file_scheme(filename);
    if path.contains("://") {
        let f = match vm.functions.get(&b"file_put_contents"[..]).copied() {
            Some(f) => f,
            None => return false,
        };
        let r = f(
            vm,
            &[
                Value::String(PhpString::from_string(filename.to_string())),
                Value::String(PhpString::from_vec(data.to_vec())),
            ],
        );
        return matches!(r, Ok(Value::Long(_)));
    }
    // php_libxml_streams_IO_open_write_wrapper(): the plain files wrapper
    // with REPORT_ERRORS checks open_basedir.
    let f = crate::dom::active_fn(vm);
    if !vm.check_open_basedir(&f, &path) {
        vm.emit_warning(&format!(
            "{}({}): Failed to open stream: Operation not permitted",
            f, filename
        ));
        return false;
    }
    let full = resolve_path(vm, &path);
    std::fs::write(full, data).is_ok()
}

/// Write bytes; returns the number of bytes written or -1.
pub fn write_resource_len(vm: &mut Vm, filename: &str, data: &[u8]) -> i64 {
    if write_resource(vm, filename, data) {
        data.len() as i64
    } else {
        -1
    }
}
