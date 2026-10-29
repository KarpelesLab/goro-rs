//! multipart/form-data request bodies (RFC 7578): form fields to $_POST,
//! files to $_FILES (PHP manual: Handling file uploads, POST method
//! uploads, Error Messages Explained; tests sapi/cli/tests/
//! php_cli_server_005.phpt, tests/basic/rfc1867_*.phpt, 028.phpt, 032.phpt,
//! ext/standard/tests/http/request_parse_body/multipart_*.phpt).
//!
//! - The boundary parameter may be quoted; unquoted, it ends at `,` or `;`
//!   (tests 032.phpt, rfc1867_boundary_2.phpt).
//! - Content-Disposition parameters may be quoted with `"` or `'`; inside
//!   quotes `\` escapes the quote and itself, unquoted only itself (test
//!   028.phpt).
//! - A part with neither name nor filename garbles the body (test
//!   rfc1867_garbled_mime_headers.phpt); a file part without a name is
//!   numbered (test rfc1867_anonymous_upload.phpt); a file whose name has
//!   malformed brackets is dropped (tests rfc1867_malicious_input.phpt,
//!   bug55500.phpt).
//! - Files: UPLOAD_ERR_INI_SIZE (1) over upload_max_filesize,
//!   UPLOAD_ERR_FORM_SIZE (2) over a preceding MAX_FILE_SIZE field,
//!   UPLOAD_ERR_PARTIAL (3) without a closing delimiter, UPLOAD_ERR_NO_FILE
//!   (4) for an empty filename (not counted in max_file_uploads); a failed
//!   upload has no type, temporary name or size.

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::Vm;
use crate::builtins::string::register_variable;

/// The limits a body is parsed with (php.ini, or request_parse_body()
/// options).
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub upload_max_filesize: i64,
    pub max_file_uploads: i64,
    pub max_input_vars: i64,
    /// max_multipart_body_parts; negative: max_input_vars +
    /// max_file_uploads (PHP manual: max_multipart_body_parts).
    pub max_parts: i64,
}

fn size_ini(vm: &Vm, k: &[u8], d: i64) -> i64 {
    vm.get_ini(k)
        .filter(|v| !v.trim_ascii().is_empty())
        .map(|v| crate::vm::setup::parse_size(&v))
        .unwrap_or(d)
}

impl Limits {
    pub fn from_ini(vm: &Vm) -> Limits {
        Limits {
            upload_max_filesize: size_ini(vm, b"upload_max_filesize", 2 << 20),
            max_file_uploads: size_ini(vm, b"max_file_uploads", 20),
            max_input_vars: size_ini(vm, b"max_input_vars", 1000),
            max_parts: size_ini(vm, b"max_multipart_body_parts", -1),
        }
    }

    fn parts(&self) -> i64 {
        if self.max_parts < 0 {
            self.max_input_vars.max(0) + self.max_file_uploads.max(0)
        } else {
            self.max_parts
        }
    }
}

/// The boundary parameter of a multipart Content-Type, or the error
/// message.
pub fn boundary(content_type: &[u8]) -> Result<Vec<u8>, &'static str> {
    const MISSING: &str = "Missing boundary in multipart/form-data POST data";
    const INVALID: &str = "Invalid boundary in multipart/form-data POST data";
    let lower = content_type.to_ascii_lowercase();
    let Some(p) = lower.windows(9).position(|w| w == b"boundary=") else {
        return Err(MISSING);
    };
    let v = &content_type[p + 9..];
    let v = if let Some(q) = v.strip_prefix(b"\"") {
        match q.iter().position(|&c| c == b'"') {
            Some(e) => &q[..e],
            None => return Err(INVALID),
        }
    } else {
        let end = v
            .iter()
            .position(|&c| c == b',' || c == b';')
            .unwrap_or(v.len());
        v[..end].trim_ascii()
    };
    if v.is_empty() {
        return Err(INVALID);
    }
    Ok(v.to_vec())
}

/// A parameter of a Content-Disposition value (`name="x"`).
fn disposition_param(v: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let mut i = 0;
    while i < v.len() {
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
        while i < v.len() && (v[i] == b' ' || v[i] == b'\t') {
            i += 1;
        }
        let mut val = Vec::new();
        if i < v.len() && (v[i] == b'"' || v[i] == b'\'') {
            let q = v[i];
            i += 1;
            while i < v.len() && v[i] != q {
                if v[i] == b'\\' && i + 1 < v.len() && (v[i + 1] == q || v[i + 1] == b'\\') {
                    i += 1;
                }
                val.push(v[i]);
                i += 1;
            }
            i += 1;
        } else {
            while i < v.len() && v[i] != b';' {
                if v[i] == b'\\' && v.get(i + 1) == Some(&b'\\') {
                    i += 1;
                }
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

/// A file field's name is a base and `[...]` groups, nothing else.
fn well_formed(name: &[u8]) -> bool {
    let Some(open) = name.iter().position(|&c| c == b'[') else {
        return true;
    };
    let mut rest = &name[open..];
    while !rest.is_empty() {
        if rest[0] != b'[' {
            return false;
        }
        let Some(close) = rest.iter().position(|&c| c == b']') else {
            return false;
        };
        if rest[1..close].contains(&b'[') {
            return false;
        }
        rest = &rest[close + 1..];
    }
    true
}

/// Parses `body` into `post` and `files`; the error message, if parsing
/// stopped (what was parsed before stays).
pub fn parse(
    vm: &mut Vm,
    body: &[u8],
    bound: &[u8],
    limits: Limits,
    post: &mut PhpArray,
    files: &mut PhpArray,
) -> Result<(), String> {
    let mut delim = b"--".to_vec();
    delim.extend_from_slice(bound);
    let mut nl_delim = b"\n".to_vec();
    nl_delim.extend_from_slice(&delim);
    // The first delimiter starts the body or a line.
    let mut pos = if body.starts_with(&delim) {
        0
    } else {
        match find(body, &nl_delim, 0) {
            Some(p) => p + 1,
            None => return Ok(()),
        }
    };
    let uploads_on = vm.get_ini(b"file_uploads").is_none_or(|v| truthy(&v));
    let mut form_max: Option<i64> = None;
    let (mut nfiles, mut nvars, mut nparts) = (0i64, 0i64, 0i64);
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
            return Ok(());
        }
        let Some(mut i) = after_line(body, after) else {
            return Ok(());
        };
        // Part headers, up to an empty line (folded lines join the
        // previous one).
        let mut disposition: Option<Vec<u8>> = None;
        let mut ctype = Vec::new();
        let mut last: Option<u8> = None;
        loop {
            let Some(next) = after_line(body, i) else {
                return Ok(());
            };
            let raw = &body[i..next];
            let line = raw.trim_ascii();
            i = next;
            if line.is_empty() {
                break;
            }
            if raw[0] == b' ' || raw[0] == b'\t' {
                match last {
                    Some(b'd') => {
                        if let Some(d) = disposition.as_mut() {
                            d.push(b' ');
                            d.extend_from_slice(line);
                        }
                    }
                    Some(b't') => {
                        ctype.push(b' ');
                        ctype.extend_from_slice(line);
                    }
                    _ => {}
                }
                continue;
            }
            last = None;
            if let Some(c) = line.iter().position(|&c| c == b':') {
                let n = line[..c].trim_ascii();
                let v = line[c + 1..].trim_ascii();
                if n.eq_ignore_ascii_case(b"content-disposition") {
                    disposition = Some(v.to_vec());
                    last = Some(b'd');
                } else if n.eq_ignore_ascii_case(b"content-type") {
                    ctype = v.to_vec();
                    last = Some(b't');
                }
            }
        }
        nparts += 1;
        if nparts > limits.parts() {
            return Err(format!(
                "Multipart body parts limit exceeded {}. To increase the limit change max_multipart_body_parts in php.ini.",
                limits.parts()
            ));
        }
        let disposition = disposition.unwrap_or_default();
        let name = disposition_param(&disposition, b"name");
        let filename = disposition_param(&disposition, b"filename");
        if name.is_none() && filename.is_none() {
            return Err("File Upload Mime headers garbled".into());
        }
        // The content ends before the line break preceding the next
        // delimiter; without one, the part is cut short.
        let end = find(body, &nl_delim, i);
        let (content, partial) = match end {
            Some(end) => {
                let mut cend = end;
                if cend > i && body[cend - 1] == b'\r' {
                    cend -= 1;
                }
                pos = end + 1;
                (&body[i..cend], false)
            }
            None => (&body[i..], true),
        };
        match filename {
            None => {
                let name = name.unwrap_or_default();
                if partial {
                    return Ok(());
                }
                nvars += 1;
                if limits.max_input_vars >= 0 && nvars > limits.max_input_vars {
                    return Err(format!(
                        "Input variables exceeded {}. To increase the limit change max_input_vars in php.ini.",
                        limits.max_input_vars
                    ));
                }
                if name == b"MAX_FILE_SIZE" {
                    form_max = Some(goro_runtime::convert::str_to_int(content));
                }
                register_variable(post, &name, Value::Str(PhpStr::from(content.to_vec())));
            }
            Some(_) if !uploads_on => {}
            Some(_) if name.as_ref().is_some_and(|n| !well_formed(n)) => {}
            Some(full) => {
                if !full.is_empty() {
                    nfiles += 1;
                    if limits.max_file_uploads >= 0 && nfiles > limits.max_file_uploads {
                        return Err(
                            "Maximum number of allowable file uploads has been exceeded".into()
                        );
                    }
                }
                let base_name = match full.iter().rposition(|&c| c == b'/' || c == b'\\') {
                    Some(p) => full[p + 1..].to_vec(),
                    None => full.clone(),
                };
                let (mut tmp, mut error, mut size) = (Vec::new(), 0i64, content.len() as i64);
                if full.is_empty() {
                    error = 4; // UPLOAD_ERR_NO_FILE
                } else if partial {
                    error = 3; // UPLOAD_ERR_PARTIAL
                } else if limits.upload_max_filesize > 0 && size > limits.upload_max_filesize {
                    error = 1; // UPLOAD_ERR_INI_SIZE
                } else if form_max.is_some_and(|m| size > m) {
                    error = 2; // UPLOAD_ERR_FORM_SIZE
                } else {
                    match write_tmp(&tmp_dir, content) {
                        Some(p) => {
                            vm.out.sapi.uploads.push(p.clone());
                            tmp = p;
                        }
                        None => error = 7, // UPLOAD_ERR_CANT_WRITE
                    }
                }
                let typ = match ctype.iter().position(|&c| c == b';') {
                    Some(p) => ctype[..p].trim_ascii().to_vec(),
                    None => ctype.clone(),
                };
                let (typ, full, base_name) = match error {
                    0 => (typ, full, base_name),
                    4 => (Vec::new(), Vec::new(), Vec::new()),
                    _ => (Vec::new(), full, base_name),
                };
                if error != 0 {
                    size = 0;
                }
                let fields: [(&[u8], Value); 6] = [
                    (b"name", Value::Str(PhpStr::from(base_name))),
                    (b"full_path", Value::Str(PhpStr::from(full))),
                    (b"type", Value::Str(PhpStr::from(typ))),
                    (b"tmp_name", Value::Str(PhpStr::from(tmp))),
                    (b"error", Value::Int(error)),
                    (b"size", Value::Int(size)),
                ];
                match name {
                    Some(name) => {
                        let bracket = name.iter().position(|&c| c == b'[').unwrap_or(name.len());
                        let (base, rest) = name.split_at(bracket);
                        for (k, v) in fields {
                            let mut n = base.to_vec();
                            n.push(b'[');
                            n.extend_from_slice(k);
                            n.push(b']');
                            n.extend_from_slice(rest);
                            register_variable(files, &n, v);
                        }
                    }
                    None => {
                        let mut entry = PhpArray::new();
                        for (k, v) in fields {
                            entry.insert(Key::from_bytes(k), v);
                        }
                        let _ = files.push(Value::Array(entry));
                    }
                }
                if partial {
                    return Ok(());
                }
            }
        }
        if partial {
            return Ok(());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries() {
        assert_eq!(
            boundary(b"multipart/form-data; boundary=BVoyv, charset=iso-8859-1").as_deref(),
            Ok(&b"BVoyv"[..])
        );
        assert_eq!(
            boundary(b"multipart/form-data; boundary=\"a b\"").as_deref(),
            Ok(&b"a b"[..])
        );
        assert!(boundary(b"multipart/form-data; boundary=\"foobar").is_err());
        assert!(boundary(b"multipart/form-data").is_err());
    }

    #[test]
    fn disposition_quoting() {
        let p = |v: &[u8]| disposition_param(v, b"name").unwrap();
        assert_eq!(p(b"form-data; name=name\\\\5"), b"name\\5");
        assert_eq!(p(b"form-data; name=name\\'6"), b"name\\'6");
        assert_eq!(p(b"form-data; name='name\\'10'"), b"name'10");
        assert_eq!(p(b"form-data; name='name\\\"11'"), b"name\\\"11");
        assert_eq!(p(b"form-data; name=\"name\\\"15\""), b"name\"15");
        assert_eq!(
            disposition_param(b"form-data; filename=\"a\"", b"name"),
            None
        );
    }

    #[test]
    fn file_names() {
        assert!(well_formed(b"file[]"));
        assert!(well_formed(b"a[b][c]"));
        assert!(!well_formed(b"foo[]bar"));
        assert!(!well_formed(b"file[[type]"));
        assert!(!well_formed(b"file[name]["));
    }
}
