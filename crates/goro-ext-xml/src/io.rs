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

pub fn read_resource(vm: &mut Vm, filename: &str) -> Option<Vec<u8>> {
    REPORTED.with(|r| r.set(false));
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
        let mut fargs = vec![Value::String(PhpString::from_string(filename.to_string()))];
        if let Some(ctx) = crate::libxml::streams_context() {
            fargs.push(Value::False);
            fargs.push(ctx);
        }
        let r = f(vm, &fargs);
        vm.error_reporting = silenced;
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
