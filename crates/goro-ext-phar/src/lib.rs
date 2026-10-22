//! php-src ext/phar: the phar:// stream wrapper, the Phar, PharData and
//! PharFileInfo classes, the phar, tar and zip archive formats.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::ClassEntry;
use goro_core::opcode::OpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub mod archive;
mod intercept;
mod object;
pub mod pathcheck;
mod stream;
mod stub;
pub mod tar;
pub mod util;
mod webphar;
pub mod zip;

use archive::*;

/// The extension name (`extension_loaded('phar')`).
pub const NAME: &[u8] = b"Phar";

pub(crate) type R = Result<Value, VmError>;

const PRELUDE: &[u8] = include_bytes!("prelude.php");

thread_local! {
    /// Name of the Phar method running (php_error_docref() prefixes).
    static CUR_FN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static PREV_RESOLVE: std::cell::Cell<Option<fn(&mut Vm, &str) -> Option<Result<String, VmError>>>> =
        const { std::cell::Cell::new(None) };
    static PREV_STREAM: std::cell::Cell<Option<fn(&mut Vm, &str, u8) -> Option<Result<Vec<u8>, VmError>>>> =
        const { std::cell::Cell::new(None) };
    /// Plain .phar files whose compilation is redirected (phar_compile_file).
    /// (file compiled, URL opened instead, whether a failure to open the
    /// URL falls back to the file itself)
    static COMPILE_REDIRECT: RefCell<Vec<(String, Vec<u8>, bool)>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn cur_fn(vm: &Vm) -> String {
    CUR_FN
        .with(|c| c.borrow().last().cloned())
        .or_else(|| vm.current_builtin_name())
        .unwrap_or_default()
}

/// The REPORT_ERRORS warning of php_stream_open_wrapper(fname, "w+b").
pub(crate) fn open_failed_warning(vm: &mut Vm, fname: &[u8]) {
    let e = std::io::Error::last_os_error().to_string();
    let e = e.split(" (os error").next().unwrap_or("").to_string();
    let f = cur_fn(vm);
    vm.emit_warning(&format!(
        "{}({}): Failed to open stream: {}",
        f,
        l(fname),
        e
    ));
}

/// Throw `class` with `msg` (sets the pending exception).
pub(crate) fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> R {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    Ok(Value::Null)
}

pub(crate) fn sv(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

pub(crate) fn arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

// ---------------------------------------------------------------------------
// zend_parse_parameters()

/// A parsed parameter.
#[derive(Clone)]
pub(crate) enum A {
    /// not passed (optional)
    U,
    /// null for a nullable ('!') parameter
    N,
    S(Vec<u8>),
    L(i64),
    B(bool),
    Arr(Value),
    Z(Value),
}

impl A {
    pub fn s(&self) -> Option<Vec<u8>> {
        match self {
            A::S(s) => Some(s.clone()),
            _ => None,
        }
    }
    pub fn l(&self) -> Option<i64> {
        match self {
            A::L(n) => Some(*n),
            _ => None,
        }
    }
    pub fn b(&self) -> Option<bool> {
        match self {
            A::B(b) => Some(*b),
            _ => None,
        }
    }
    pub fn z(&self) -> Option<Value> {
        match self {
            A::Z(v) | A::Arr(v) => Some(v.clone()),
            _ => None,
        }
    }
}

/// Parameter names of `Class::method` from the stub.
fn param_names(fname: &str) -> Vec<&'static str> {
    let Some((c, m)) = fname.split_once("::") else {
        return Vec::new();
    };
    goro_core::stubs::find_self_declared_class(c.as_bytes())
        .and_then(|sc| sc.methods.iter().find(|f| f.name.eq_ignore_ascii_case(m)))
        .map(|f| f.args.iter().map(|a| a.name).collect())
        .unwrap_or_default()
}

/// The arguments of a prelude method (`...$args`: positional and named),
/// laid out by position. Err when an exception was thrown.
pub(crate) fn margs(vm: &mut Vm, fname: &str, args: &Value) -> Result<Vec<Option<Value>>, ()> {
    let names = param_names(fname);
    let mut list: Vec<Option<Value>> = Vec::new();
    if let Value::Array(a) = args.deref() {
        for (k, v) in a.borrow().iter() {
            match k {
                ArrayKey::Int(_) => list.push(Some(v.clone())),
                ArrayKey::String(name) => {
                    let Some(pos) = names.iter().position(|p| p.as_bytes() == name.as_bytes())
                    else {
                        let _ = throw(
                            vm,
                            b"Error",
                            &format!("Unknown named parameter ${}", name.to_string_lossy()),
                        );
                        return Err(());
                    };
                    if list.len() <= pos {
                        list.resize(pos + 1, None);
                    } else if list[pos].is_some() {
                        let _ = throw(
                            vm,
                            b"Error",
                            &format!(
                                "Named parameter ${} overwrites previous argument",
                                name.to_string_lossy()
                            ),
                        );
                        return Err(());
                    }
                    list[pos] = Some(v.clone());
                }
            }
        }
    }
    Ok(list)
}

/// zend_parse_parameters(): `spec` like "p|ls!". None when an exception
/// was thrown.
pub(crate) fn zpp(vm: &mut Vm, fname: &str, list: &[Option<Value>], spec: &str) -> Option<Vec<A>> {
    zpp_ex(vm, fname, list, spec, false)
}

/// ZEND_PARSE_PARAMS_QUIET when `quiet`.
pub(crate) fn zpp_ex(
    vm: &mut Vm,
    fname: &str,
    list: &[Option<Value>],
    spec: &str,
    quiet: bool,
) -> Option<Vec<A>> {
    let names = param_names(fname);
    // parse the spec
    let mut specs: Vec<(char, bool)> = Vec::new();
    let mut required = None;
    for c in spec.chars() {
        match c {
            '|' => required = Some(specs.len()),
            '!' => {
                if let Some(last) = specs.last_mut() {
                    last.1 = true;
                }
            }
            c => specs.push((c, false)),
        }
    }
    let min = required.unwrap_or(specs.len());
    let max = specs.len();
    let given = list.len();
    if given < min || given > max {
        if quiet {
            return None;
        }
        let (kind, n) = if min == max {
            ("exactly", max)
        } else if given < min {
            ("at least", min)
        } else {
            ("at most", max)
        };
        let msg = format!(
            "{}() expects {} {} argument{}, {} given",
            fname,
            kind,
            n,
            if n == 1 { "" } else { "s" },
            given
        );
        let _ = throw(vm, b"ArgumentCountError", &msg);
        return None;
    }
    let line = vm.current_line;
    let mut out = Vec::with_capacity(max);
    for (i, &(c, nullable)) in specs.iter().enumerate() {
        let name = names.get(i).copied().unwrap_or("");
        let prefix = format!("{}(): Argument #{} (${})", fname, i + 1, name);
        let Some(v) = list.get(i).and_then(|v| v.clone()) else {
            if i < min {
                if !quiet {
                    let _ = throw(vm, b"ArgumentCountError", &format!("{} not passed", prefix));
                }
                return None;
            }
            out.push(A::U);
            continue;
        };
        let v = v.deref();
        let tyname = |t: &str| {
            if nullable {
                format!("?{}", t)
            } else {
                t.to_string()
            }
        };
        let type_error = |vm: &mut Vm, t: &str, v: &Value| {
            if !quiet {
                let msg = format!(
                    "{} must be of type {}, {} given",
                    prefix,
                    t,
                    Vm::value_type_name(v)
                );
                let _ = throw(vm, b"TypeError", &msg);
            }
        };
        if matches!(v, Value::Null | Value::Undef) && nullable {
            out.push(A::N);
            continue;
        }
        match c {
            's' | 'p' | 'S' | 'P' | 'l' | 'b' => {
                let (mask, t) = match c {
                    'l' => (goro_core::arginfo::T_INT, "int"),
                    'b' => (goro_core::arginfo::T_BOOL, "bool"),
                    _ => (goro_core::arginfo::T_STRING, "string"),
                };
                let v = if matches!(v, Value::Null | Value::Undef) {
                    if quiet {
                        return None;
                    }
                    let msg = format!(
                        "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                        fname,
                        i + 1,
                        name,
                        t
                    );
                    vm.emit_deprecated_at(&msg, line);
                    if vm.current_exception.is_some() {
                        return None;
                    }
                    match c {
                        'l' => Value::Long(0),
                        'b' => Value::False,
                        _ => Value::String(PhpString::empty()),
                    }
                } else {
                    if quiet
                        && !matches!(
                            v,
                            Value::String(_)
                                | Value::Long(_)
                                | Value::Double(_)
                                | Value::True
                                | Value::False
                        )
                    {
                        return None;
                    }
                    match vm.zpp_coerce_scalar(mask, &v, false, line) {
                        Ok(Some(nv)) => nv,
                        Ok(None) => {
                            type_error(vm, &tyname(t), &v);
                            return None;
                        }
                        Err(()) => return None,
                    }
                };
                match c {
                    'l' => out.push(A::L(v.to_long())),
                    'b' => out.push(A::B(v.is_truthy())),
                    _ => {
                        let s = v.to_php_string().as_bytes().to_vec();
                        if (c == 'p' || c == 'P') && s.contains(&0) {
                            if !quiet {
                                let msg = format!("{} must not contain any null bytes", prefix);
                                let _ = throw(vm, b"ValueError", &msg);
                            }
                            return None;
                        }
                        out.push(A::S(s));
                    }
                }
            }
            'a' | 'h' => {
                if !matches!(v, Value::Array(_)) {
                    type_error(vm, &tyname("array"), &v);
                    return None;
                }
                out.push(A::Arr(v));
            }
            'r' => {
                if !matches!(v, Value::Resource(_)) {
                    type_error(vm, &tyname("resource"), &v);
                    return None;
                }
                out.push(A::Z(v));
            }
            'f' => {
                // Z_PARAM_FUNC(_OR_NULL)
                let r = vm.zend_is_callable_ex(
                    &v,
                    goro_core::callable::IS_CALLABLE_SUPPRESS_DEPRECATIONS,
                );
                if vm.current_exception.is_some() {
                    return None;
                }
                if !r.ok {
                    if !quiet {
                        let msg = format!(
                            "{} must be a valid callback{}, {}",
                            prefix,
                            if nullable { " or null" } else { "" },
                            r.error.unwrap_or_default()
                        );
                        let _ = throw(vm, b"TypeError", &msg);
                    }
                    return None;
                }
                out.push(A::Z(v));
            }
            _ => out.push(A::Z(v)),
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// registration

fn compile_prelude() -> Option<(OpArray, Vec<ClassEntry>)> {
    let mut lexer = goro_parser::Lexer::new(PRELUDE);
    let tokens = lexer.tokenize();
    let _ = goro_parser::take_lexer_deprecations();
    let _ = goro_parser::take_lexer_warnings();
    let mut parser = goro_parser::Parser::new(tokens)
        .with_source(PRELUDE)
        .trusted();
    let program = parser.parse().ok()?;
    let compiler = goro_core::compiler::Compiler::new();
    let (mut op_array, mut classes, _) = compiler.compile(&program).ok()?;
    let _ = goro_core::compiler::take_pending_compile_warnings();
    clear_lines(&mut op_array);
    for c in &mut classes {
        for m in c.methods.values_mut() {
            clear_lines(Rc::make_mut(&mut m.op_array));
        }
    }
    Some((op_array, classes))
}

fn clear_lines(op: &mut OpArray) {
    for o in &mut op.ops {
        o.line = 0;
    }
    op.decl_line = 0;
    op.end_line = 0;
    op.filename.clear();
    for c in &mut op.child_functions {
        clear_lines(c);
    }
}

fn make_internal(vm: &mut Vm, lower: &[u8]) {
    let Some(ce) = vm.classes.get_mut(lower) else {
        return;
    };
    ce.filename = None;
    ce.start_line = 0;
    ce.end_line = 0;
    ce.doc_comment = None;
    let Some(sc) = goro_core::stubs::find_self_declared_class(lower) else {
        return;
    };
    let mut patched: Vec<(Vec<u8>, Rc<OpArray>)> = Vec::new();
    for m in ce.methods.values_mut() {
        let Some(f) = sc
            .methods
            .iter()
            .find(|f| f.name.as_bytes().eq_ignore_ascii_case(&m.name))
        else {
            continue;
        };
        let oa = Rc::make_mut(&mut m.op_array);
        oa.internal_sig = Some(f);
        oa.doc_comment = None;
        m.doc_comment = None;
        let mut key = lower.to_vec();
        key.extend_from_slice(b"::");
        key.extend_from_slice(&m.name.to_ascii_lowercase());
        patched.push((key, m.op_array.clone()));
    }
    // (the copies registered for static calls)
    for (key, op) in patched {
        if let Some(f) = vm.user_functions.get_mut(&key) {
            *f = op;
        }
    }
}

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("__phar_m", object::dispatch),
    ("__phar_build_begin", object::build_begin),
    ("__phar_build_item", object::build_item),
    ("__phar_build_end", object::build_end),
];

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(b"phar".as_slice()) {
        return;
    }
    vm.register_extension(NAME);
    for &(name, f) in FUNCTIONS {
        vm.register_function(name.as_bytes(), f);
    }
    // INI
    vm.constants.insert(b"phar.readonly".to_vec(), sv(b"1"));
    vm.constants.insert(b"phar.require_hash".to_vec(), sv(b"1"));
    vm.constants.insert(b"phar.cache_list".to_vec(), sv(b""));
    vm.ini_validators
        .insert(b"phar.readonly".to_vec(), on_update_readonly);
    vm.ini_validators
        .insert(b"phar.require_hash".to_vec(), on_update_require_hash);
    vm.ini_validators
        .insert(b"phar.cache_list".to_vec(), on_update_cache_list);
    // classes
    if let Some((op_array, classes)) = compile_prelude() {
        let names: Vec<Vec<u8>> = classes
            .iter()
            .map(|c| c.name.to_ascii_lowercase())
            .collect();
        let saved_file = std::mem::take(&mut vm.current_file);
        let saved_line = vm.current_line;
        let _ = vm.run_compiled(&op_array, classes);
        vm.current_file = saved_file;
        vm.current_line = saved_line;
        vm.current_exception = None;
        for lower in &names {
            make_internal(vm, lower);
        }
    }
    // the stream wrapper and the compile/resolve hooks
    goro_ext_standard::native_streams::register_native_wrapper(
        b"phar",
        Rc::new(stream::PharWrapper),
    );
    PREV_RESOLVE.with(|p| p.set(vm.include_resolve_hook));
    PREV_STREAM.with(|p| p.set(vm.include_stream_hook));
    vm.include_resolve_hook = Some(include_resolve);
    vm.include_stream_hook = Some(include_stream);
    intercept::register(vm);
    vm.phpinfo_hooks.push(phpinfo);
}

fn ini_parse_bool(v: &[u8]) -> bool {
    let s = String::from_utf8_lossy(v).trim().to_ascii_lowercase();
    match s.as_str() {
        "true" | "on" | "yes" => true,
        _ => {
            // atoi()
            let digits: String = s
                .chars()
                .enumerate()
                .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+')))
                .map(|(_, c)| c)
                .collect();
            digits.parse::<i64>().map(|n| n != 0).unwrap_or(false)
        }
    }
}

/// phar_ini_modify_handler()
fn ini_modify(value: &[u8], startup: bool, is_readonly: bool) -> bool {
    let ini = ini_parse_bool(value);
    let old = g(|g| {
        if is_readonly {
            g.readonly_orig
        } else {
            g.require_hash_orig
        }
    });
    if startup {
        g(|g| {
            if is_readonly {
                g.readonly_orig = ini;
            } else {
                g.require_hash_orig = ini;
            }
        });
    } else if old && !ini {
        return false;
    }
    if is_readonly {
        let list: Vec<ArcRef> = g(|g| {
            g.readonly = ini;
            if g.request_init {
                g.fname_map.values().cloned().collect()
            } else {
                Vec::new()
            }
        });
        for p in list {
            let mut p = p.borrow_mut();
            if !p.is_data {
                p.is_writeable = !ini;
            }
        }
    } else {
        g(|g| g.require_hash = ini);
    }
    true
}

fn on_update_readonly(_vm: &mut Vm, value: &[u8], startup: bool) -> bool {
    ini_modify(value, startup, true)
}

fn on_update_require_hash(_vm: &mut Vm, value: &[u8], startup: bool) -> bool {
    ini_modify(value, startup, false)
}

/// phar.cache_list (PHP_INI_SYSTEM): the archives are opened at startup.
fn on_update_cache_list(vm: &mut Vm, value: &[u8], startup: bool) -> bool {
    if !startup {
        return false;
    }
    for key in value.split(|&c| c == b':') {
        if key.is_empty() {
            continue;
        }
        if open_from_filename(vm, key, None, 0).is_err() {
            break;
        }
    }
    true
}

/// PHP_MINFO_FUNCTION(phar)
fn phpinfo(vm: &mut Vm) -> String {
    let ini = |vm: &Vm, k: &[u8]| -> String {
        vm.constants
            .get(k)
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_default()
    };
    let b = |s: String| -> String {
        if ini_parse_bool(s.as_bytes()) {
            "On".to_string()
        } else {
            "Off".to_string()
        }
    };
    let ro = b(ini(vm, b"phar.readonly"));
    let rh = b(ini(vm, b"phar.require_hash"));
    let cl = ini(vm, b"phar.cache_list");
    let cl = if cl.is_empty() {
        "no value".to_string()
    } else {
        cl
    };
    let openssl = if vm.loaded_extensions.contains(b"openssl".as_slice()) {
        "enabled"
    } else {
        "disabled (install ext/openssl)"
    };
    format!(
        "\nPhar\n\nPhar: PHP Archive support => enabled\nPhar API version => 1.1.1\nPhar-based phar archives => enabled\nTar-based phar archives => enabled\nZIP-based phar archives => enabled\ngzip compression => enabled\nbzip2 compression => enabled\nOpenSSL support => {}\n\n\nPhar based on pear/PHP_Archive, original concept by Davey Shafik.\nPhar fully realized by Gregory Beaver and Marcus Boerger.\nPortions of tar implementation Copyright (c) 2003-2009 Tim Kientzle.\nDirective => Local Value => Master Value\nphar.cache_list => {cl} => {cl}\nphar.readonly => {ro} => {ro}\nphar.require_hash => {rh} => {rh}\n",
        openssl
    )
}

// ---------------------------------------------------------------------------
// include / require (phar_resolve_path, phar_compile_file)

fn prev_resolve(vm: &mut Vm, path: &str) -> Option<Result<String, VmError>> {
    PREV_RESOLVE.with(|p| p.get()).and_then(|f| f(vm, path))
}

fn prev_stream(vm: &mut Vm, path: &str, kind: u8) -> Option<Result<Vec<u8>, VmError>> {
    PREV_STREAM
        .with(|p| p.get())
        .and_then(|f| f(vm, path, kind))
}

fn phar_active() -> bool {
    goro_ext_standard::native_streams::native_wrapper_active(b"phar")
}

/// The opened path of a phar:// URL ("phar://<archive>/<entry>").
fn canonical_url(vm: &mut Vm, path: &[u8]) -> Option<Vec<u8>> {
    let (arch, entry) = split_fname(vm, path, 2, 0).ok()?;
    let phar = open_from_filename(vm, &arch, None, 0).ok()?;
    if entry == b"/" {
        // the stub: opened_path is the archive itself
        return Some(phar.borrow().fname.clone());
    }
    let mut o = b"phar://".to_vec();
    o.extend_from_slice(&phar.borrow().fname);
    {
        let e = &entry[1..];
        if let Ok(en) = util::get_entry_info(vm, &phar, e, false) {
            o.push(b'/');
            o.extend_from_slice(&en.borrow().filename);
            return Some(o);
        }
        o.extend_from_slice(&entry);
    }
    Some(o)
}

/// phar_compile_file(): a tar/zip-based or compressed phar compiled from
/// its plain file name runs its stub (or its decompressed contents).
fn compile_redirect(vm: &mut Vm, file: &[u8]) -> bool {
    let Ok(phar) = open_from_filename(vm, file, None, 0) else {
        return false;
    };
    let (is_zip, is_tar, flags, fname) = {
        let pb = phar.borrow();
        (pb.is_zip, pb.is_tar, pb.flags, pb.fname.clone())
    };
    let url = if is_zip || is_tar {
        let mut url = b"phar://".to_vec();
        url.extend_from_slice(&fname);
        url.extend_from_slice(b"/.phar/stub.php");
        url
    } else if flags & PHAR_FILE_COMPRESSION_MASK != 0 {
        Vec::new()
    } else {
        return false;
    };
    COMPILE_REDIRECT.with(|c| c.borrow_mut().push((l(file), url, true)));
    true
}

fn include_resolve(vm: &mut Vm, path: &str) -> Option<Result<String, VmError>> {
    let p = path.as_bytes();
    if !phar_active() {
        return prev_resolve(vm, path);
    }
    if starts_with_ci(p, b"phar://") {
        let c = canonical_url(vm, p).unwrap_or_else(|| p.to_vec());
        if !starts_with_ci(&c, b"phar://") {
            // the stub of the archive: opened through the wrapper, named
            // after the archive (its opened_path)
            COMPILE_REDIRECT.with(|r| r.borrow_mut().push((l(&c), p.to_vec(), false)));
        }
        return Some(Ok(l(&c)));
    }
    let is_rel = !p.is_empty() && p[0] != b'/' && !has_scheme(p);
    if is_rel {
        if let Some((ret, _)) = util::find_in_include_path(vm, p) {
            if starts_with_ci(&ret, b"phar://") {
                let c = canonical_url(vm, &ret).unwrap_or(ret);
                return Some(Ok(l(&c)));
            }
        }
        // php_resolve_path() over include_path entries naming phar archives
        let inc = vm
            .constants
            .get(b"include_path".as_slice())
            .map(|v| v.to_php_string().as_bytes().to_vec())
            .unwrap_or_default();
        if memmem(&inc, b"phar://").is_some() {
            if let Some(found) = util::resolve_path(vm, p, &inc) {
                if starts_with_ci(&found, b"phar://") {
                    let c = canonical_url(vm, &found).unwrap_or(found);
                    return Some(Ok(l(&c)));
                }
            }
        }
        // php_resolve_path(): the directory of the executing script
        if let Some(exec) = executed_filename(vm) {
            if starts_with_ci(&exec, b"phar://") && !p.starts_with(b"./") && !p.starts_with(b"../")
            {
                if let Some(slash) = exec.iter().rposition(|&c| c == b'/') {
                    let mut t = exec[..slash + 1].to_vec();
                    t.extend_from_slice(p);
                    let found = goro_ext_standard::native_streams::stat_path(
                        vm,
                        &t,
                        PHP_STREAM_URL_STAT_QUIET,
                    )
                    .ok()
                    .flatten()
                    .is_some();
                    if found {
                        let c = canonical_url(vm, &t).unwrap_or(t);
                        return Some(Ok(l(&c)));
                    }
                }
            }
        }
    }
    if memmem(p, b".phar").is_some() && memmem(p, b"://").is_none() {
        let resolved = prev_resolve(vm, path);
        let file = match &resolved {
            Some(Err(_)) => return resolved,
            Some(Ok(r)) => r.clone(),
            None => {
                let exec = executed_filename(vm).map(|e| l(&e)).unwrap_or_default();
                resolve_plain(path, &exec)
            }
        };
        if std::path::Path::new(&file).is_file() && compile_redirect(vm, file.as_bytes()) {
            return Some(Ok(file));
        }
        return resolved;
    }
    prev_resolve(vm, path)
}

/// The include path of a relative plain file (no include_path search).
fn resolve_plain(path: &str, exec_file: &str) -> String {
    if path.starts_with('/') {
        return path.to_string();
    }
    if std::path::Path::new(path).exists() {
        if let Some(full) = expand_filepath(path.as_bytes()) {
            return l(&full);
        }
    }
    if let Some(dir) = std::path::Path::new(exec_file).parent() {
        let cand = dir.join(path);
        if cand.exists() {
            return cand.to_string_lossy().into_owned();
        }
    }
    expand_filepath(path.as_bytes())
        .map(|p| l(&p))
        .unwrap_or_else(|| path.to_string())
}

fn include_stream(vm: &mut Vm, path: &str, kind: u8) -> Option<Result<Vec<u8>, VmError>> {
    let redirect = COMPILE_REDIRECT.with(|c| {
        let mut c = c.borrow_mut();
        let pos = c.iter().position(|(f, _, _)| f == path)?;
        let (_, url, fallback) = c.remove(pos);
        Some((url, fallback))
    });
    if let Some((url, fallback)) = redirect {
        if url.is_empty() {
            // compressed phar: the decompressed contents
            let phar = open_from_filename(vm, path.as_bytes(), None, 0).ok()?;
            if phar.borrow().fp.is_none() {
                let _ = util::open_archive_fp(vm, &phar);
            }
            let data = phar.borrow().fp.as_ref().map(|f| f.borrow().clone());
            return data.map(Ok);
        }
        if !fallback {
            return prev_stream(vm, &l(&url), kind);
        }
        // (when the stub cannot be opened, the archive itself is compiled)
        match goro_ext_standard::native_streams::include_open_url(vm, &l(&url), kind) {
            Ok(Some(src)) => return Some(Ok(src)),
            Ok(None) if vm.current_exception.is_none() => {}
            Ok(None) => {
                return Some(Err(VmError {
                    message: String::new(),
                    line: vm.current_line,
                }));
            }
            Err(e) => return Some(Err(e)),
        }
    }
    prev_stream(vm, path, kind)
}

/// Native data of Phar/PharData objects.
pub(crate) struct PharObj {
    pub archive: RefCell<Option<ArcRef>>,
}

impl Drop for PharObj {
    fn drop(&mut self) {
        if let Some(a) = self.archive.borrow_mut().take() {
            let busy = archive::G.with(|g| g.try_borrow_mut().is_err());
            if !busy {
                archive_delref(&a);
            }
        }
    }
}

/// Native data of PharFileInfo objects.
pub(crate) struct EntryObj {
    pub entry: RefCell<Option<EntryRef>>,
}

impl EntryObj {
    pub fn release(&self) {
        let Some(e) = self.entry.borrow_mut().take() else {
            return;
        };
        if e.borrow().is_temp_dir {
            return;
        }
        let phar = {
            let mut eb = e.borrow_mut();
            eb.fileinfo_lock_count = eb.fileinfo_lock_count.saturating_sub(1);
            eb.phar.upgrade()
        };
        if let Some(p) = phar {
            let busy = archive::G.with(|g| g.try_borrow_mut().is_err());
            if !busy {
                archive_delref(&p);
            }
        }
    }
}

impl Drop for EntryObj {
    fn drop(&mut self) {
        self.release();
    }
}

/// phar_compile_file() for the main script: a tar/zip-based phar runs its
/// `.phar/stub.php`, a compressed phar its decompressed contents. None
/// when the script is compiled as it is.
pub fn main_script_source(
    filename: &str,
    source: &[u8],
    ini: &[(String, String)],
) -> Option<Vec<u8>> {
    if !filename.contains(".phar") || filename.contains("://") {
        return None;
    }
    // (the startup INI settings of the request)
    for (k, v) in ini {
        match k.as_str() {
            "phar.readonly" => {
                ini_modify(v.as_bytes(), true, true);
            }
            "phar.require_hash" => {
                ini_modify(v.as_bytes(), true, false);
            }
            _ => {}
        }
    }
    let _ = source;
    // (the request is not set up yet: a VM for the hash/bz2 functions)
    let mut vm = Vm::new();
    goro_ext_standard::register_standard_functions(&mut vm);
    goro_ext_hash::register(&mut vm);
    goro_ext_bz2::register(&mut vm);
    let phar = open_from_filename(&mut vm, filename.as_bytes(), None, 0).ok()?;
    let (is_zip, is_tar, flags) = {
        let p = phar.borrow();
        (p.is_zip, p.is_tar, p.flags)
    };
    if is_zip || is_tar {
        let fname = phar.borrow().fname.clone();
        let idata = util::get_entry_data(&mut vm, &fname, b".phar/stub.php", b"r", 0, false)
            .ok()
            .flatten()?;
        let data = util::read_entry_contents(&mut vm, &idata.internal_file, true);
        entry_delref(idata);
        return data;
    }
    if flags & PHAR_FILE_COMPRESSION_MASK != 0 {
        if phar.borrow().fp.is_none() {
            let _ = util::open_archive_fp(&mut vm, &phar);
        }
        let data = phar.borrow().fp.as_ref().map(|f| f.borrow().clone());
        return data;
    }
    None
}
