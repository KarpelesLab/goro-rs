//! Phar::webPhar(), the front controller of a phar run by a web SAPI
//! (PHP manual: Phar::webPhar, Phar::mungServer, Phar::interceptFileFuncs).
//! The request's path inside the archive (PATH_INFO, else the request URI
//! after the script name) goes through the rewrite callback, then:
//! - nothing (or `/`): a 301 redirect to the index file;
//! - a missing file: the not-found script, else a 404 page;
//! - a PHP file runs in the global scope, a PHPS file is shown
//!   highlighted, other files are sent with their MIME type;
//!
//! and the request ends. Behaviour from the manual and the tests
//! ext/phar/tests/frontcontroller*.phpt (and their tar/ and zip/ copies).

use goro_runtime::{Key, KeyRef, PhpArray, PhpStr, Value};

use super::{Shared, norm_inner, state};
use crate::func::Args;
use crate::vm::{R, Unwind, Vm};

/// How a file is answered.
#[derive(Clone)]
enum Kind {
    Php,
    Phps,
    Mime(Vec<u8>),
}

/// The MIME types of file extensions (PHP manual: Phar::webPhar, the
/// default mimetypes).
const MIMES: &[(&str, &str)] = &[
    ("c", "text/plain"),
    ("cc", "text/plain"),
    ("cpp", "text/plain"),
    ("c++", "text/plain"),
    ("dtd", "text/plain"),
    ("h", "text/plain"),
    ("log", "text/plain"),
    ("rng", "text/plain"),
    ("txt", "text/plain"),
    ("xsd", "text/plain"),
    ("avi", "video/avi"),
    ("bmp", "image/bmp"),
    ("css", "text/css"),
    ("gif", "image/gif"),
    ("htm", "text/html"),
    ("html", "text/html"),
    ("htmls", "text/html"),
    ("ico", "image/x-ico"),
    ("jpe", "image/jpeg"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("js", "application/x-javascript"),
    ("midi", "audio/midi"),
    ("mid", "audio/midi"),
    ("mod", "audio/mod"),
    ("mov", "movie/quicktime"),
    ("mp3", "audio/mp3"),
    ("mpg", "video/mpeg"),
    ("mpeg", "video/mpeg"),
    ("pdf", "application/pdf"),
    ("png", "image/png"),
    ("swf", "application/shockwave-flash"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
    ("wav", "audio/wav"),
    ("xbm", "image/xbm"),
    ("xml", "text/xml"),
];

const SPECIFIER: &str = "only Phar::PHP, Phar::PHPS and a mime type string are allowed";

const NOT_FOUND: &str = "<html>\n <head>\n  <title>File Not Found</title>\n </head>\n <body>\n  <h1>404 - File Not Found</h1>\n </body>\n</html>";

const DENIED: &str = "<html>\n <head>\n  <title>Access Denied</title>\n </head>\n <body>\n  <h1>403 - File Access Denied</h1>\n </body>\n</html>";

pub(super) fn web_phar(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let alias = a.opt_str(0).map(|s| s.to_vec());
    let index = a
        .opt_str(1)
        .map(|s| s.to_vec())
        .unwrap_or_else(|| b"index.php".to_vec());
    let not_found = a.opt_str(2).map(|s| s.to_vec());
    let mimes = if a.has(3) {
        a.array(3)
    } else {
        PhpArray::new()
    };
    let rewrite = a.get(4).map(|v| v.deref()).filter(|v| !v.is_null());
    // the running file is opened as a phar first, under any SAPI (test
    // webphar_compilefail.phpt)
    super::classes::map_running(vm, alias)?;
    if !vm.out.sapi.web() {
        return Ok(Value::Null);
    }
    let mut overrides: Vec<(Vec<u8>, Kind)> = Vec::new();
    for (k, v) in mimes.iter() {
        let ext = match k {
            KeyRef::Int(n) => n.to_string().into_bytes(),
            KeyRef::Str(s) => s.as_bytes().to_vec(),
        };
        let kind = match v.deref() {
            Value::Int(0) => Kind::Php,
            Value::Int(1) => Kind::Phps,
            Value::Int(_) => {
                return vm.throw(
                    "PharException",
                    &format!("Unknown mime type specifier used, {SPECIFIER}"),
                );
            }
            Value::Str(s) => Kind::Mime(s.as_bytes().to_vec()),
            _ => {
                return vm.throw(
                    "PharException",
                    &format!("Unknown mime type specifier used (not a string or int), {SPECIFIER}"),
                );
            }
        };
        overrides.push((ext, kind));
    }
    let f = vm.cur_file().to_vec();
    let key = super::abs_path(vm, &f);
    let Some(s) = state(vm).archives.get(&key).cloned() else {
        return Ok(Value::Null);
    };
    let server = server_array(vm);
    let get = |n: &str| match server.get_bytes(n.as_bytes()).map(|v| v.deref()) {
        Some(Value::Str(s)) => Some(s.as_bytes().to_vec()),
        _ => None,
    };
    let script_name = get("SCRIPT_NAME").unwrap_or_default();
    let request_uri = get("REQUEST_URI").unwrap_or_default();
    let mut path = match get("PATH_INFO").filter(|p| !p.is_empty()) {
        Some(p) => p,
        None => {
            // a request for another script is left alone
            let Some(rest) = request_uri.strip_prefix(&script_name[..]) else {
                return Ok(Value::Null);
            };
            let end = rest.iter().position(|&c| c == b'?').unwrap_or(rest.len());
            rest[..end].to_vec()
        }
    };
    if let Some(cb) = rewrite {
        let r = vm.call_value(&cb, vec![Value::Str(PhpStr::from(path.clone()))]);
        match r {
            Ok(Value::Str(p)) => path = p.as_bytes().to_vec(),
            Ok(Value::Bool(false)) => {
                crate::builtins::headers::add_header(vm, b"HTTP/1.0 403 Access Denied", true, 0)?;
                vm.echo(DENIED.as_bytes());
                return Err(Unwind::Exit);
            }
            Ok(_) => {
                vm.echo(b"phar error: rewrite callback must return a string or false");
                return Err(Unwind::Exit);
            }
            Err(Unwind::Throw(e)) => {
                vm.echo(b"phar error: rewrite callback must return a string or false");
                return Err(Unwind::Throw(e));
            }
            Err(e) => return Err(e),
        }
    }
    let inner = norm_inner(&path);
    if inner.is_empty() {
        let idx = norm_inner(&index);
        if is_file(&s, &idx) {
            // (tests frontcontroller4, 5, 7)
            crate::builtins::headers::add_header(vm, b"HTTP/1.1 301 Moved Permanently", true, 0)?;
            let mut loc = b"Location: ".to_vec();
            loc.extend_from_slice(&script_name);
            loc.push(b'/');
            loc.extend_from_slice(&idx);
            crate::builtins::headers::add_header(vm, &loc, true, 0)?;
            return Err(Unwind::Exit);
        }
        return missing(vm, &s, &key, not_found, &script_name);
    }
    // the file, or the longest leading part of the path naming one, the
    // rest becoming PATH_INFO (test frontcontroller25)
    let mut found = None;
    if is_file(&s, &inner) {
        found = Some((inner.clone(), None));
    } else {
        let mut cut = inner.len();
        while let Some(p) = inner[..cut].iter().rposition(|&c| c == b'/') {
            cut = p;
            if is_file(&s, &inner[..cut]) {
                found = Some((inner[..cut].to_vec(), Some(inner[cut..].to_vec())));
                break;
            }
        }
    }
    match found {
        Some((entry, extra)) => {
            let kind = kind_of(&entry, &overrides);
            serve(vm, &s, &key, &entry, extra, kind, &script_name)
        }
        None => missing(vm, &s, &key, not_found, &script_name),
    }
}

/// A file (not a directory) of the archive, or a mounted file.
fn is_file(s: &Shared, inner: &[u8]) -> bool {
    let l = s.borrow();
    if let Some(ext) = super::wrapper::mounted(&l.mounts, inner) {
        return super::os_path(&ext).is_file();
    }
    l.a.get(inner).is_some_and(|e| !e.is_dir)
}

/// The not-found script, or a 404 page (tests frontcontroller6, 8, 24).
fn missing(
    vm: &mut Vm,
    s: &Shared,
    key: &[u8],
    not_found: Option<Vec<u8>>,
    script_name: &[u8],
) -> R<Value> {
    if let Some(nf) = not_found {
        let nf = norm_inner(&nf);
        if is_file(s, &nf) {
            // it runs as PHP whatever its extension maps to (test
            // frontcontroller24)
            return serve(vm, s, key, &nf, None, Kind::Php, script_name);
        }
    }
    crate::builtins::headers::add_header(vm, b"HTTP/1.0 404 Not Found", true, 0)?;
    vm.echo(NOT_FOUND.as_bytes());
    Err(Unwind::Exit)
}

fn kind_of(entry: &[u8], overrides: &[(Vec<u8>, Kind)]) -> Kind {
    let base = match entry.iter().rposition(|&c| c == b'/') {
        Some(p) => &entry[p + 1..],
        None => entry,
    };
    let Some(dot) = base.iter().rposition(|&c| c == b'.') else {
        // no extension: plain text (test frontcontroller27)
        return Kind::Mime(b"text/plain".to_vec());
    };
    let ext = &base[dot + 1..];
    if let Some((_, k)) = overrides.iter().find(|(e, _)| e == ext) {
        return k.clone();
    }
    match ext {
        b"php" | b"inc" => Kind::Php,
        b"phps" => Kind::Phps,
        _ => Kind::Mime(
            MIMES
                .iter()
                .find(|(e, _)| e.as_bytes() == ext)
                .map(|(_, m)| m.as_bytes())
                .unwrap_or(b"application/octet-stream")
                .to_vec(),
        ),
    }
}

/// Answers the request with a file of the archive, then ends it.
fn serve(
    vm: &mut Vm,
    s: &Shared,
    key: &[u8],
    entry: &[u8],
    extra: Option<Vec<u8>>,
    kind: Kind,
    script_name: &[u8],
) -> R<Value> {
    let mut url = b"phar://".to_vec();
    url.extend_from_slice(key);
    url.push(b'/');
    url.extend_from_slice(entry);
    let src = match super::wrapper::read_entry(vm, s, entry) {
        Ok(d) => d,
        Err(m) => {
            vm.echo(m.as_bytes());
            return Err(Unwind::Exit);
        }
    };
    match kind {
        Kind::Phps => {
            let html = crate::builtins::highlight::highlight(vm, &src);
            vm.echo(&html);
        }
        Kind::Mime(m) => {
            let mut h = b"Content-type: ".to_vec();
            h.extend_from_slice(&m);
            crate::builtins::headers::add_header(vm, &h, true, 0)?;
            let l = format!("Content-length: {}", src.len());
            crate::builtins::headers::add_header(vm, l.as_bytes(), true, 0)?;
            vm.echo(&src);
        }
        Kind::Php => {
            mung(vm, entry, &url, extra, script_name);
            // relative includes (./, ../) start from the file's directory
            // (tests frontcontroller22, 34)
            let mut dir = b"phar://".to_vec();
            dir.extend_from_slice(key);
            if let Some(p) = entry.iter().rposition(|&c| c == b'/') {
                dir.push(b'/');
                dir.extend_from_slice(&entry[..p]);
            }
            crate::builtins::streams::set_virtual_cwd(vm, Some(dir));
            let r = vm.run_front_file(&src, &url);
            crate::builtins::streams::set_virtual_cwd(vm, None);
            r?;
        }
    }
    Err(Unwind::Exit)
}

/// The global $_SERVER array (a copy).
fn server_array(vm: &mut Vm) -> PhpArray {
    match vm.global_var_ptr(&PhpStr::from("_SERVER"), false) {
        // SAFETY: a read of the global's slot.
        Some(p) => match unsafe { (*p).deref() } {
            Value::Array(a) => a,
            _ => PhpArray::new(),
        },
        None => PhpArray::new(),
    }
}

/// $_SERVER as the file sees it: PATH_TRANSLATED names it, PATH_INFO is
/// what follows its name in the path, and the entries Phar::mungServer()
/// named describe it, their first values kept as PHAR_<name> (test
/// frontcontroller21).
fn mung(vm: &mut Vm, entry: &[u8], url: &[u8], extra: Option<Vec<u8>>, script_name: &[u8]) {
    let names = state(vm).mung.clone();
    let mut server = server_array(vm);
    let sv = |b: &[u8]| Value::Str(PhpStr::from(b.to_vec()));
    server.insert(Key::from("PATH_TRANSLATED"), sv(url));
    if let Some(e) = extra {
        server.insert(Key::from("PATH_INFO"), sv(&e));
    }
    for n in names {
        let old = match server.get_bytes(&n).map(|v| v.deref()) {
            Some(Value::Str(s)) => s.as_bytes().to_vec(),
            _ => continue,
        };
        let new = match &n[..] {
            b"SCRIPT_FILENAME" => url.to_vec(),
            b"SCRIPT_NAME" => {
                let mut v = b"/".to_vec();
                v.extend_from_slice(entry);
                v
            }
            _ => match old.strip_prefix(script_name) {
                Some(rest) if !script_name.is_empty() => rest.to_vec(),
                _ => old.clone(),
            },
        };
        let mut pk = b"PHAR_".to_vec();
        pk.extend_from_slice(&n);
        server.insert(Key::from_bytes(&pk), sv(&old));
        server.insert(Key::from_bytes(&n), sv(&new));
    }
    if let Some(p) = vm.global_var_ptr(&PhpStr::from("_SERVER"), true) {
        // SAFETY: the global's slot, written once.
        unsafe {
            (*p).assign(Value::Array(server));
        }
    }
}
