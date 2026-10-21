//! Resource loading/saving for the XML extensions (libxml2's I/O layer as
//! seen through PHP's stream wrappers).

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

fn strip_file_scheme(path: &str) -> String {
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
        if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 {
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
pub fn read_resource(vm: &mut Vm, filename: &str) -> Option<Vec<u8>> {
    let path = strip_file_scheme(filename);
    if path.contains("://") || path.starts_with("php://") || path.starts_with("data:") {
        // go through file_get_contents (stream wrappers)
        let f = vm.functions.get(&b"file_get_contents".to_vec()).copied()?;
        let silenced = vm.error_reporting;
        vm.error_reporting = 0;
        let r = f(
            vm,
            &[Value::String(PhpString::from_string(filename.to_string()))],
        );
        vm.error_reporting = silenced;
        return match r {
            Ok(Value::String(s)) => Some(s.as_bytes().to_vec()),
            _ => None,
        };
    }
    let full = resolve_path(vm, &path);
    match std::fs::read(&full) {
        Ok(d) => Some(d),
        Err(_) => None,
    }
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
    let path = strip_file_scheme(filename);
    if path.contains("://") {
        let f = match vm.functions.get(&b"file_put_contents".to_vec()).copied() {
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
