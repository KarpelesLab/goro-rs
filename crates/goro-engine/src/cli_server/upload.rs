//! multipart/form-data request bodies (RFC 7578): form fields to $_POST,
//! files to $_FILES (PHP manual: Handling file uploads, POST method
//! uploads; test sapi/cli/tests/php_cli_server_005).

use goro_runtime::{PhpArray, PhpStr, Value};

use crate::Vm;
use crate::builtins::string::register_variable;

/// The boundary parameter of a multipart Content-Type.
pub fn boundary(content_type: &[u8]) -> Option<Vec<u8>> {
    for param in content_type.split(|&c| c == b';').skip(1) {
        let param = param.trim_ascii();
        if param.len() > 9 && param[..9].eq_ignore_ascii_case(b"boundary=") {
            let mut v = &param[9..];
            if v.len() >= 2 && v[0] == b'"' && v[v.len() - 1] == b'"' {
                v = &v[1..v.len() - 1];
            }
            return (!v.is_empty()).then(|| v.to_vec());
        }
    }
    None
}

/// A parameter of a Content-Disposition value (`name="x"`).
fn disposition_param(v: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let mut i = 0;
    while i < v.len() {
        // Skip to the next parameter.
        while i < v.len() && (v[i] == b';' || v[i].is_ascii_whitespace()) {
            i += 1;
        }
        let start = i;
        while i < v.len() && v[i] != b'=' && v[i] != b';' {
            i += 1;
        }
        let name = v[start..i].trim_ascii();
        if i >= v.len() || v[i] == b';' {
            continue;
        }
        i += 1;
        let mut val = Vec::new();
        if i < v.len() && v[i] == b'"' {
            i += 1;
            while i < v.len() && v[i] != b'"' {
                if v[i] == b'\\' && i + 1 < v.len() && v[i + 1] == b'"' {
                    i += 1;
                }
                val.push(v[i]);
                i += 1;
            }
            i += 1;
        } else {
            while i < v.len() && v[i] != b';' {
                val.push(v[i]);
                i += 1;
            }
            val = val.trim_ascii().to_vec();
        }
        if name.eq_ignore_ascii_case(key) {
            return Some(val);
        }
    }
    None
}

/// The index just after the next line break from `i`.
fn after_line(b: &[u8], i: usize) -> Option<usize> {
    b[i..].iter().position(|&c| c == b'\n').map(|p| i + p + 1)
}

fn find(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
    if from > h.len() || n.is_empty() {
        return None;
    }
    h[from..]
        .windows(n.len())
        .position(|w| w == n)
        .map(|p| p + from)
}

fn truthy(v: &[u8]) -> bool {
    let l = v.trim_ascii().to_ascii_lowercase();
    matches!(&l[..], b"on" | b"yes" | b"true") || goro_runtime::convert::str_to_int(&l) != 0
}

fn size_ini(vm: &Vm, k: &[u8], d: i64) -> i64 {
    vm.get_ini(k)
        .map(|v| crate::vm::setup::parse_size(&v))
        .unwrap_or(d)
}

/// Parses `body` into `post` and `files`; returns the temporary files.
pub fn parse(vm: &mut Vm, body: &[u8], bound: &[u8], post: &mut PhpArray, files: &mut PhpArray) {
    let mut delim = b"--".to_vec();
    delim.extend_from_slice(bound);
    // The first delimiter starts the body or a line.
    let mut pos = if body.starts_with(&delim) {
        0
    } else {
        let mut nl = b"\n".to_vec();
        nl.extend_from_slice(&delim);
        match find(body, &nl, 0) {
            Some(p) => p + 1,
            None => return,
        }
    };
    let uploads_on = vm.get_ini(b"file_uploads").is_none_or(|v| truthy(&v));
    let max_size = size_ini(vm, b"upload_max_filesize", 2 << 20);
    let max_files = size_ini(vm, b"max_file_uploads", 20);
    let mut form_max: Option<i64> = None;
    let mut nfiles = 0;
    let tmp_dir = vm
        .get_ini(b"upload_tmp_dir")
        .filter(|d| !d.is_empty())
        .map(|d| d.to_vec())
        .or_else(|| {
            std::env::var_os("TMPDIR").map(|d| {
                use std::os::unix::ffi::OsStrExt;
                d.as_bytes().to_vec()
            })
        })
        .unwrap_or_else(|| b"/tmp".to_vec());
    loop {
        let after = pos + delim.len();
        if body[after..].starts_with(b"--") {
            return;
        }
        let Some(mut i) = after_line(body, after) else {
            return;
        };
        // Part headers.
        let mut disposition = Vec::new();
        let mut ctype = Vec::new();
        loop {
            let Some(next) = after_line(body, i) else {
                return;
            };
            let line = body[i..next].trim_ascii();
            i = next;
            if line.is_empty() {
                break;
            }
            if let Some(c) = line.iter().position(|&c| c == b':') {
                let n = line[..c].trim_ascii();
                let v = line[c + 1..].trim_ascii();
                if n.eq_ignore_ascii_case(b"content-disposition") {
                    disposition = v.to_vec();
                } else if n.eq_ignore_ascii_case(b"content-type") {
                    ctype = v.to_vec();
                }
            }
        }
        // The content ends before the line break preceding the next
        // delimiter.
        let mut nl = b"\n".to_vec();
        nl.extend_from_slice(&delim);
        let Some(end) = find(body, &nl, i) else {
            return;
        };
        let mut cend = end;
        if cend > i && body[cend - 1] == b'\r' {
            cend -= 1;
        }
        let content = &body[i..cend];
        pos = end + 1;
        let Some(name) = disposition_param(&disposition, b"name") else {
            continue;
        };
        match disposition_param(&disposition, b"filename") {
            None => {
                if name == b"MAX_FILE_SIZE" {
                    form_max = Some(goro_runtime::convert::str_to_int(content));
                }
                register_variable(post, &name, Value::Str(PhpStr::from(content.to_vec())));
            }
            Some(_) if !uploads_on => {}
            Some(full) => {
                nfiles += 1;
                if nfiles > max_files {
                    continue;
                }
                let base_name = match full.iter().rposition(|&c| c == b'/' || c == b'\\') {
                    Some(p) => full[p + 1..].to_vec(),
                    None => full.clone(),
                };
                let (mut tmp, mut error, mut size) = (Vec::new(), 0i64, content.len() as i64);
                if full.is_empty() {
                    error = 4; // UPLOAD_ERR_NO_FILE
                    size = 0;
                } else if max_size > 0 && size > max_size {
                    error = 1; // UPLOAD_ERR_INI_SIZE
                    size = 0;
                } else if form_max.is_some_and(|m| size > m) {
                    error = 2; // UPLOAD_ERR_FORM_SIZE
                    size = 0;
                } else {
                    match write_tmp(&tmp_dir, content) {
                        Some(p) => {
                            vm.out.sapi.uploads.push(p.clone());
                            tmp = p;
                        }
                        None => {
                            error = 7; // UPLOAD_ERR_CANT_WRITE
                            size = 0;
                        }
                    }
                }
                let (typ, full, base_name) = if error == 4 {
                    (Vec::new(), Vec::new(), Vec::new())
                } else {
                    (ctype.clone(), full, base_name)
                };
                let bracket = name.iter().position(|&c| c == b'[').unwrap_or(name.len());
                let (base, rest) = name.split_at(bracket);
                let fields: [(&[u8], Value); 6] = [
                    (b"name", Value::Str(PhpStr::from(base_name))),
                    (b"full_path", Value::Str(PhpStr::from(full))),
                    (b"type", Value::Str(PhpStr::from(typ))),
                    (b"tmp_name", Value::Str(PhpStr::from(tmp))),
                    (b"error", Value::Int(error)),
                    (b"size", Value::Int(size)),
                ];
                for (k, v) in fields {
                    let mut n = base.to_vec();
                    n.push(b'[');
                    n.extend_from_slice(k);
                    n.push(b']');
                    n.extend_from_slice(rest);
                    register_variable(files, &n, v);
                }
            }
        }
    }
}

/// Writes an uploaded file to `dir` under a fresh "php" + random name.
fn write_tmp(dir: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    for _ in 0..16 {
        let mut r = [0u8; 12];
        crate::builtins::math::os_random(&mut r);
        let mut p = dir.to_vec();
        if p.last() != Some(&b'/') {
            p.push(b'/');
        }
        p.extend_from_slice(b"php");
        p.extend(r.iter().map(|b| ALNUM[*b as usize % ALNUM.len()]));
        let path = std::path::Path::new(std::ffi::OsStr::from_bytes(&p));
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            if f.write_all(data).is_ok() {
                return Some(p);
            }
            let _ = std::fs::remove_file(path);
            return None;
        }
    }
    None
}
