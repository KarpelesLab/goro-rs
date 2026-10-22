//! Phar::webPhar() and its helpers (phar_object.c): serving the files of
//! the executing phar to a web request.

use goro_core::array::ArrayKey;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::archive::*;
use crate::{A, R, sv, throw, util};

const PE: &[u8] = b"PharException";

/// PHAR_G(mime_types): extension -> (mime, type)
fn mime_type(ext: &[u8]) -> Option<(&'static str, i64)> {
    const T: &[(&str, &str, i64)] = &[
        ("phps", "text/html", PHAR_MIME_PHPS),
        ("c", "text/plain", PHAR_MIME_OTHER),
        ("cc", "text/plain", PHAR_MIME_OTHER),
        ("cpp", "text/plain", PHAR_MIME_OTHER),
        ("c++", "text/plain", PHAR_MIME_OTHER),
        ("dtd", "text/plain", PHAR_MIME_OTHER),
        ("h", "text/plain", PHAR_MIME_OTHER),
        ("log", "text/plain", PHAR_MIME_OTHER),
        ("rng", "text/plain", PHAR_MIME_OTHER),
        ("txt", "text/plain", PHAR_MIME_OTHER),
        ("xsd", "text/plain", PHAR_MIME_OTHER),
        ("php", "", PHAR_MIME_PHP),
        ("inc", "", PHAR_MIME_PHP),
        ("avi", "video/avi", PHAR_MIME_OTHER),
        ("bmp", "image/bmp", PHAR_MIME_OTHER),
        ("css", "text/css", PHAR_MIME_OTHER),
        ("gif", "image/gif", PHAR_MIME_OTHER),
        ("htm", "text/html", PHAR_MIME_OTHER),
        ("html", "text/html", PHAR_MIME_OTHER),
        ("htmls", "text/html", PHAR_MIME_OTHER),
        ("ico", "image/x-ico", PHAR_MIME_OTHER),
        ("jpe", "image/jpeg", PHAR_MIME_OTHER),
        ("jpg", "image/jpeg", PHAR_MIME_OTHER),
        ("jpeg", "image/jpeg", PHAR_MIME_OTHER),
        ("js", "application/x-javascript", PHAR_MIME_OTHER),
        ("midi", "audio/midi", PHAR_MIME_OTHER),
        ("mid", "audio/midi", PHAR_MIME_OTHER),
        ("mod", "audio/mod", PHAR_MIME_OTHER),
        ("mov", "movie/quicktime", PHAR_MIME_OTHER),
        ("mp3", "audio/mp3", PHAR_MIME_OTHER),
        ("mpg", "video/mpeg", PHAR_MIME_OTHER),
        ("mpeg", "video/mpeg", PHAR_MIME_OTHER),
        ("pdf", "application/pdf", PHAR_MIME_OTHER),
        ("png", "image/png", PHAR_MIME_OTHER),
        ("swf", "application/shockwave-flash", PHAR_MIME_OTHER),
        ("tif", "image/tiff", PHAR_MIME_OTHER),
        ("tiff", "image/tiff", PHAR_MIME_OTHER),
        ("wav", "audio/wav", PHAR_MIME_OTHER),
        ("xbm", "image/xbm", PHAR_MIME_OTHER),
        ("xml", "text/xml", PHAR_MIME_OTHER),
    ];
    T.iter()
        .find(|(e, _, _)| e.as_bytes() == ext)
        .map(|(_, m, t)| (*m, *t))
}

/// phar_file_type()
fn file_type(file: &[u8]) -> (Vec<u8>, i64) {
    let Some(dot) = file.iter().rposition(|&c| c == b'.') else {
        return (b"text/plain".to_vec(), PHAR_MIME_OTHER);
    };
    match mime_type(&file[dot + 1..]) {
        Some((m, t)) => (m.as_bytes().to_vec(), t),
        None => (b"application/octet-stream".to_vec(), PHAR_MIME_OTHER),
    }
}

fn header(vm: &mut Vm, line: &str, code: Option<i64>) {
    if let Some(f) = vm.functions.get(b"header".as_slice()).copied() {
        let mut args = vec![sv(line.as_bytes()), Value::True];
        if let Some(c) = code {
            args.push(Value::Long(c));
        }
        let _ = f(vm, &args);
    }
}

/// zend_bailout(): end the request.
fn bailout(vm: &mut Vm) -> R {
    vm.exit_requested = true;
    Ok(Value::Null)
}

fn server_get(vm: &Vm, key: &[u8]) -> Option<Value> {
    let Some(Value::Array(s)) = vm.get_global(b"_SERVER") else {
        return None;
    };
    let v = s
        .borrow()
        .get(&ArrayKey::String(PhpString::from_bytes(key)))
        .cloned();
    v.map(|v| v.deref())
}

fn server_set(vm: &Vm, key: &[u8], v: Value) {
    if let Some(Value::Array(s)) = vm.get_global(b"_SERVER") {
        s.borrow_mut()
            .set(ArrayKey::String(PhpString::from_bytes(key)), v);
    }
}

fn server_str(vm: &Vm, key: &[u8]) -> Option<Vec<u8>> {
    match server_get(vm, key) {
        Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

/// phar_mung_server_vars()
fn mung_server_vars(
    vm: &mut Vm,
    fname: &[u8],
    entry: &[u8],
    basename: &[u8],
    request_uri_len: usize,
) {
    if vm.get_global(b"_SERVER").is_none() {
        return;
    }
    if let Some(path_info) = server_str(vm, b"PATH_INFO") {
        if path_info.len() > entry.len() && path_info.starts_with(entry) {
            let rest = &path_info[entry.len()..];
            let n = request_uri_len.min(rest.len());
            server_set(vm, b"PATH_INFO", sv(&rest[..n]));
            server_set(vm, b"PHAR_PATH_INFO", sv(&path_info));
        }
    }
    if let Some(pt) = server_get(vm, b"PATH_TRANSLATED") {
        let mut s = b"phar://".to_vec();
        s.extend_from_slice(fname);
        s.extend_from_slice(entry);
        server_set(vm, b"PATH_TRANSLATED", sv(&s));
        server_set(vm, b"PHAR_PATH_TRANSLATED", pt);
    }
    let mung = g(|g| g.mung_list);
    if mung == 0 {
        return;
    }
    let strip = |vm: &mut Vm, key: &[u8], phar_key: &[u8]| {
        if let Some(v) = server_str(vm, key) {
            if v.len() > basename.len() && v.starts_with(basename) {
                server_set(vm, key, sv(&v[basename.len()..]));
                server_set(vm, phar_key, sv(&v));
            }
        }
    };
    if mung & 2 != 0 {
        strip(vm, b"REQUEST_URI", b"PHAR_REQUEST_URI");
    }
    if mung & 1 != 0 {
        strip(vm, b"PHP_SELF", b"PHAR_PHP_SELF");
    }
    if mung & 4 != 0 {
        if let Some(v) = server_get(vm, b"SCRIPT_NAME") {
            server_set(vm, b"SCRIPT_NAME", sv(entry));
            server_set(vm, b"PHAR_SCRIPT_NAME", v);
        }
    }
    if mung & 8 != 0 {
        if let Some(v) = server_get(vm, b"SCRIPT_FILENAME") {
            let mut s = b"phar://".to_vec();
            s.extend_from_slice(fname);
            s.extend_from_slice(entry);
            server_set(vm, b"SCRIPT_FILENAME", sv(&s));
            server_set(vm, b"PHAR_SCRIPT_FILENAME", v);
        }
    }
}

/// Compile and run a file of the phar (zend_compile_file + zend_execute).
fn run_file(vm: &mut Vm, name: &[u8]) {
    let Ok(Some(source)) = goro_ext_standard::native_streams::read_whole_url(vm, name, true) else {
        return;
    };
    let file = l(name);
    let mut lexer = goro_parser::Lexer::new(&source);
    lexer.skip_shebang();
    let tokens = lexer.tokenize();
    let _ = goro_parser::take_lexer_deprecations();
    let _ = goro_parser::take_lexer_warnings();
    let mut parser = goro_parser::Parser::new(tokens).with_source(&source);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(e) => {
            let msg = if e.message.starts_with("syntax error") {
                e.message.clone()
            } else {
                "syntax error".to_string()
            };
            vm.fatal_error_cb(4, &msg, &file, e.span.line);
            vm.fatal_emitted = true;
            return;
        }
    };
    let mut compiler = goro_core::compiler::Compiler::new();
    compiler.source_file = name.to_vec();
    let (op_array, classes, warnings) = match compiler.compile(&program) {
        Ok(r) => r,
        Err(e) => {
            let _ = goro_core::compiler::take_pending_compile_warnings();
            vm.fatal_error_cb(1, &e.message, &file, e.line);
            vm.fatal_emitted = true;
            return;
        }
    };
    let _ = goro_core::compiler::take_pending_compile_warnings();
    vm.sync_builtin_frames();
    let saved_file = std::mem::replace(&mut vm.current_file, file.into());
    let saved_line = vm.current_line;
    for (msg, line) in &warnings {
        vm.emit_warning_at(msg, *line);
    }
    let _ = vm.run_compiled(&op_array, classes);
    vm.current_file = saved_file;
    vm.current_line = saved_line;
}

/// phar_file_action()
#[allow(clippy::too_many_arguments)]
fn file_action(
    vm: &mut Vm,
    phar: &ArcRef,
    info: &EntryRef,
    mime_type: &[u8],
    code: i64,
    entry: &[u8],
    arch: &[u8],
    basename: Option<&[u8]>,
    ru_len: usize,
) -> R {
    let mut name = b"phar://".to_vec();
    name.extend_from_slice(arch);
    if entry.first() != Some(&b'/') {
        name.push(b'/');
    }
    name.extend_from_slice(entry);
    match code {
        PHAR_MIME_PHPS => {
            // highlight_file(): the highlighted source
            let src = goro_ext_standard::native_streams::read_whole_url(vm, &name, true)
                .ok()
                .flatten();
            if let (Some(src), Some(f)) = (
                src,
                vm.functions.get(b"highlight_string".as_slice()).copied(),
            ) {
                let _ = f(vm, &[sv(&src)]);
            }
            bailout(vm)
        }
        PHAR_MIME_OTHER => {
            header(vm, &format!("Content-type: {}", l(mime_type)), None);
            let size = info.borrow().uncompressed_filesize;
            header(vm, &format!("Content-length: {}", size), None);
            if util::get_efp(vm, info, true).is_none() {
                if let Err(e) = util::open_jit(vm, phar, info) {
                    if let Some(e) = e {
                        return throw(vm, PE, &e);
                    }
                    return Ok(Value::Null);
                }
            }
            let _ = util::open_entry_fp(vm, info, true);
            let data = util::read_entry_contents(vm, info, true).unwrap_or_default();
            vm.write_output(&data);
            bailout(vm)
        }
        _ => {
            if let Some(b) = basename {
                mung_server_vars(vm, arch, entry, b, ru_len);
            }
            g(|g| {
                g.cwd = None;
                if let Some(cwd) = entry.iter().rposition(|&c| c == b'/') {
                    g.cwd_init = true;
                    if cwd == 0 {
                        g.cwd = None;
                    } else if entry.first() == Some(&b'/') {
                        g.cwd = Some(entry[1..cwd].to_vec());
                    } else {
                        g.cwd = Some(entry[..cwd].to_vec());
                    }
                }
            });
            run_file(vm, &name);
            g(|g| {
                g.cwd = None;
                g.cwd_init = false;
            });
            bailout(vm)
        }
    }
}

fn do_403(vm: &mut Vm) {
    header(vm, "HTTP/1.0 403 Access Denied", Some(403));
    vm.write_output(b"<html>\n <head>\n  <title>Access Denied</title>\n </head>\n <body>\n  <h1>403 - File Access Denied</h1>\n </body>\n</html>");
}

fn do_404(vm: &mut Vm, phar: Option<&ArcRef>, fname: &[u8], f404: Option<&[u8]>) -> R {
    if let (Some(phar), Some(f404)) = (phar, f404.filter(|f| !f.is_empty())) {
        if let Ok(info) = util::get_entry_info(vm, phar, f404, true) {
            return file_action(
                vm,
                phar,
                &info,
                b"text/html",
                PHAR_MIME_PHP,
                f404,
                fname,
                None,
                0,
            );
        }
    }
    header(vm, "HTTP/1.0 404 Not Found", Some(404));
    vm.write_output(b"<html>\n <head>\n  <title>File Not Found</title>\n </head>\n <body>\n  <h1>404 - File Not Found</h1>\n </body>\n</html>");
    bailout(vm)
}

/// phar_postprocess_ru_web(): split "path/to/file.php/extra/stuff" into the
/// entry and the remaining request URI.
fn postprocess_ru_web(fname: &[u8], entry: &mut Vec<u8>, ru: &mut Option<Vec<u8>>) {
    let Some(pphar) = find_fname(fname) else {
        return;
    };
    // e = entry + 1, scanning back over '/' separators
    let full = entry[1..].to_vec();
    let mut e_len = full.len();
    loop {
        if pphar.borrow().manifest.contains_key(&full[..e_len]) {
            if e_len < full.len() {
                *ru = Some(full[e_len..].to_vec());
            } else {
                *ru = None;
            }
            entry.truncate(e_len + 1);
            return;
        }
        match full[..e_len].iter().rposition(|&c| c == b'/') {
            Some(p) => e_len = p,
            None => return,
        }
    }
}

/// Phar::webPhar()
pub(crate) fn web_phar(vm: &mut Vm, a: Vec<A>) -> R {
    let alias = a[0].s();
    let index_php = a[1].s();
    let f404 = a[2].s();
    let mimeoverride = match &a[3] {
        A::Arr(v) => Some(v.clone()),
        _ => None,
    };
    let rewrite = a[4].z().filter(|v| !matches!(v, Value::Null));
    request_initialize();
    if let Err(e) = open_executed_filename(vm, alias.as_deref()) {
        if let Some(e) = e {
            return throw(vm, PE, &e);
        }
        return Ok(Value::Null);
    }
    // retrieve requested file within phar
    if !goro_ext_standard::info::is_web_sapi() {
        return Ok(Value::Null);
    }
    let method = std::env::var("REQUEST_METHOD").unwrap_or_default();
    let has_uri = std::env::var_os("REQUEST_URI").is_some();
    if !has_uri
        || !matches!(
            method.as_str(),
            "GET" | "POST" | "DELETE" | "HEAD" | "OPTIONS" | "PATCH" | "PUT"
        )
    {
        return Ok(Value::Null);
    }
    let Some(fname) = executed_filename(vm) else {
        return Ok(Value::Null);
    };
    let basename = match fname.iter().rposition(|&c| c == b'/') {
        Some(i) => fname[i + 1..].to_vec(),
        None => fname.clone(),
    };
    // cgi-fcgi: SCRIPT_NAME + PATH_INFO
    let Some(script_name) = server_str(vm, b"SCRIPT_NAME") else {
        return Ok(Value::Null);
    };
    if memmem(&script_name, &basename).is_none() {
        return Ok(Value::Null);
    }
    let (mut entry, path_info) = match server_str(vm, b"PATH_INFO") {
        Some(pi) => {
            let mut p = script_name.clone();
            p.extend_from_slice(&pi);
            (pi, p)
        }
        None => (Vec::new(), script_name.clone()),
    };
    let pt = script_name.clone();
    if let Some(cb) = rewrite {
        let r = vm.call_callback(&cb, &[sv(&entry)]);
        if let Some(prev) = vm.current_exception.take() {
            // (no return value: the callback threw; zend_throw_exception_ex()
            // chains the pending exception)
            throw(
                vm,
                PE,
                "phar error: rewrite callback must return a string or false",
            )?;
            if let Some(exc) = &vm.current_exception {
                Vm::exception_set_previous(exc, prev);
            }
            return Ok(Value::Null);
        }
        match r {
            Ok(Value::String(s)) => entry = s.as_bytes().to_vec(),
            Ok(Value::True) | Ok(Value::False) => {
                do_403(vm);
                return bailout(vm);
            }
            Ok(_) => {
                return throw(
                    vm,
                    PE,
                    "phar error: rewrite callback must return a string or false",
                );
            }
            Err(_) => {
                if vm.current_exception.is_none() {
                    return throw(vm, PE, "phar error: failed to call rewrite callback");
                }
                return Ok(Value::Null);
            }
        }
    }
    let _ = pt;
    let mut ru: Option<Vec<u8>> = None;
    if !entry.is_empty() {
        postprocess_ru_web(&fname, &mut entry, &mut ru);
    }
    if entry.is_empty() || entry == b"/" {
        // direct request
        entry = match &index_php {
            Some(i) if !i.is_empty() => {
                if i.first() != Some(&b'/') {
                    let mut e = b"/".to_vec();
                    e.extend_from_slice(i);
                    e
                } else {
                    i.clone()
                }
            }
            _ => b"/index.php".to_vec(),
        };
        let phar = util::get_archive(&fname, None).ok();
        let info = phar
            .as_ref()
            .and_then(|p| util::get_entry_info(vm, p, &entry, false).ok());
        if info.is_none() {
            return do_404(vm, phar.as_ref(), &fname, f404.as_deref());
        }
        header(vm, "HTTP/1.1 301 Moved Permanently", Some(301));
        let loc = if path_info.last() == Some(&b'/') {
            format!("Location: {}{}", l(&path_info), l(&entry[1..]))
        } else {
            format!("Location: {}{}", l(&path_info), l(&entry))
        };
        header(vm, &loc, None);
        return bailout(vm);
    }
    let phar = util::get_archive(&fname, None).ok();
    let info = phar
        .as_ref()
        .and_then(|p| util::get_entry_info(vm, p, &entry, false).ok());
    let (Some(phar), Some(info)) = (phar.clone(), info) else {
        return do_404(vm, phar.as_ref(), &fname, f404.as_deref());
    };
    let mut mime: Option<(Vec<u8>, i64)> = None;
    if let Some(Value::Array(mo)) = &mimeoverride {
        if !mo.borrow().is_empty() {
            if let Some(dot) = entry.iter().rposition(|&c| c == b'.') {
                let ext = &entry[dot + 1..];
                let v = mo
                    .borrow()
                    .get(&ArrayKey::String(PhpString::from_bytes(ext)))
                    .cloned();
                if let Some(v) = v {
                    match v.deref() {
                        Value::Long(n) if n == PHAR_MIME_PHP || n == PHAR_MIME_PHPS => {
                            mime = Some((Vec::new(), n));
                        }
                        Value::Long(_) => {
                            return throw(
                                vm,
                                PE,
                                "Unknown mime type specifier used, only Phar::PHP, Phar::PHPS and a mime type string are allowed",
                            );
                        }
                        Value::String(s) => {
                            mime = Some((s.as_bytes().to_vec(), PHAR_MIME_OTHER));
                        }
                        _ => {
                            return throw(
                                vm,
                                PE,
                                "Unknown mime type specifier used (not a string or int), only Phar::PHP, Phar::PHPS and a mime type string are allowed",
                            );
                        }
                    }
                }
            }
        }
    }
    let (mime_type, code) = mime.unwrap_or_else(|| file_type(&entry));
    let ru_len = ru.as_ref().map(|r| r.len()).unwrap_or(0);
    file_action(
        vm,
        &phar,
        &info,
        &mime_type,
        code,
        &entry,
        &fname,
        Some(&script_name),
        ru_len,
    )
}
