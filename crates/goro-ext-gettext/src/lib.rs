//! php-src's ext/gettext (gettext.c) over `intl`, an emulation of GNU
//! libintl reading .mo catalogs.

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub mod intl;

/// The extension name (`extension_loaded('gettext')`).
pub const NAME: &[u8] = b"gettext";

type R = Result<Value, VmError>;

const MAX_DOMAIN_LENGTH: usize = 1024;
const MAX_MSGID_LENGTH: usize = 4096;

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("textdomain", textdomain),
    ("gettext", gettext),
    ("_", gettext),
    ("dgettext", dgettext),
    ("dcgettext", dcgettext),
    ("bindtextdomain", bindtextdomain),
    ("ngettext", ngettext),
    ("dngettext", dngettext),
    ("dcngettext", dcngettext),
    ("bind_textdomain_codeset", bind_textdomain_codeset),
];

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    // PHP_MINFO_FUNCTION(gettext)
    vm.phpinfo_hooks
        .push(|_| "\ngettext\n\nGetText Support => enabled\n".to_string());
    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        vm.zpp_extension_verified.insert(
            name.as_bytes().to_vec(),
            goro_core::arginfo::CHECK_ARITY | goro_core::arginfo::CHECK_TYPES,
        );
    }
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> R {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    Ok(Value::Null)
}

fn value_error(vm: &mut Vm, func: &str, n: usize, what: &str) -> R {
    let name = goro_core::stubs::find_function(func.as_bytes())
        .and_then(|f| f.args.get(n - 1))
        .map(|a| a.name)
        .unwrap_or("");
    let msg = format!("{}(): Argument #{} (${}) {}", func, n, name, what);
    throw(vm, b"ValueError", &msg)
}

fn arg(args: &[Value], i: usize) -> Value {
    match args.get(i) {
        Some(v) => v.deref(),
        None => Value::Undef,
    }
}

fn bytes(args: &[Value], i: usize) -> Vec<u8> {
    arg(args, i).to_php_string().as_bytes().to_vec()
}

fn opt_bytes(args: &[Value], i: usize) -> Option<Vec<u8>> {
    match arg(args, i) {
        Value::Undef | Value::Null => None,
        v => Some(v.to_php_string().as_bytes().to_vec()),
    }
}

fn sval(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

/// A C string view.
fn cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(p) => &b[..p],
        None => b,
    }
}

/// PHP_GETTEXT_DOMAIN_LENGTH_CHECK(): false after the ValueError.
fn domain_check(vm: &mut Vm, f: &str, n: usize, domain: &[u8]) -> bool {
    if domain.len() > MAX_DOMAIN_LENGTH {
        let _ = value_error(vm, f, n, "is too long");
        return false;
    }
    if domain.is_empty() {
        let _ = value_error(vm, f, n, "must not be empty");
        return false;
    }
    true
}

/// PHP_GETTEXT_LENGTH_CHECK()
fn length_check(vm: &mut Vm, f: &str, n: usize, s: &[u8]) -> bool {
    if s.len() > MAX_MSGID_LENGTH {
        let _ = value_error(vm, f, n, "is too long");
        return false;
    }
    true
}

/// PHP_DCGETTEXT_CATEGORY_CHECK()
fn category_check(vm: &mut Vm, f: &str, n: usize, category: i64) -> bool {
    if category == intl::LC_ALL {
        let _ = value_error(vm, f, n, "cannot be LC_ALL");
        return false;
    }
    true
}

fn textdomain(vm: &mut Vm, args: &[Value]) -> R {
    let f = "textdomain";
    let domain = opt_bytes(args, 0);
    if let Some(d) = &domain {
        if !domain_check(vm, f, 1, d) {
            return Ok(Value::Null);
        }
        if d == b"0" {
            return value_error(vm, f, 1, "cannot be zero");
        }
    }
    let d = domain.as_deref().map(cstr);
    Ok(sval(&intl::textdomain(d)))
}

/// The translation, or `msgid` itself (the whole string) when there is none.
fn translated_or(msgid: Vec<u8>, t: Option<Vec<u8>>) -> Value {
    match t {
        Some(t) => sval(&t),
        None => Value::String(PhpString::from_vec(msgid)),
    }
}

fn gettext(vm: &mut Vm, args: &[Value]) -> R {
    let msgid = bytes(args, 0);
    if !length_check(vm, "gettext", 1, &msgid) {
        return Ok(Value::Null);
    }
    let t = intl::dcigettext(None, &msgid, None, libc::LC_MESSAGES as i64);
    Ok(translated_or(msgid, t))
}

fn dgettext(vm: &mut Vm, args: &[Value]) -> R {
    let f = "dgettext";
    let (domain, msgid) = (bytes(args, 0), bytes(args, 1));
    if !domain_check(vm, f, 1, &domain) || !length_check(vm, f, 2, &msgid) {
        return Ok(Value::Null);
    }
    let t = intl::dcigettext(Some(cstr(&domain)), &msgid, None, libc::LC_MESSAGES as i64);
    Ok(translated_or(msgid, t))
}

fn dcgettext(vm: &mut Vm, args: &[Value]) -> R {
    let f = "dcgettext";
    let (domain, msgid) = (bytes(args, 0), bytes(args, 1));
    let category = arg(args, 2).to_long();
    if !domain_check(vm, f, 1, &domain)
        || !length_check(vm, f, 2, &msgid)
        || !category_check(vm, f, 3, category)
    {
        return Ok(Value::Null);
    }
    let t = intl::dcigettext(Some(cstr(&domain)), &msgid, None, category);
    Ok(translated_or(msgid, t))
}

fn bindtextdomain(vm: &mut Vm, args: &[Value]) -> R {
    let f = "bindtextdomain";
    let domain = bytes(args, 0);
    if domain.contains(&0) {
        return value_error(vm, f, 1, "must not contain any null bytes");
    }
    let dir = opt_bytes(args, 1);
    if dir.as_ref().is_some_and(|d| d.contains(&0)) {
        return value_error(vm, f, 2, "must not contain any null bytes");
    }
    if !domain_check(vm, f, 1, &domain) {
        return Ok(Value::Null);
    }
    let Some(dir) = dir else {
        return Ok(sval(&intl::bindtextdomain(&domain, None)));
    };
    let dir_name = if !dir.is_empty() && dir != b"0" {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        match std::fs::canonicalize(std::ffi::OsStr::from_bytes(&dir)) {
            Ok(p) => p.into_os_string().into_vec(),
            Err(_) => return Ok(Value::False),
        }
    } else {
        use std::os::unix::ffi::OsStringExt;
        match std::env::current_dir() {
            Ok(p) => p.into_os_string().into_vec(),
            Err(_) => return Ok(Value::False),
        }
    };
    Ok(sval(&intl::bindtextdomain(&domain, Some(&dir_name))))
}

/// The untranslated plural: msgid1 for n == 1, else msgid2.
fn plural_default(msgid1: &[u8], msgid2: &[u8], n: u64) -> Value {
    sval(cstr(if n == 1 { msgid1 } else { msgid2 }))
}

fn ngettext(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ngettext";
    let (m1, m2) = (bytes(args, 0), bytes(args, 1));
    let n = arg(args, 2).to_long() as u64;
    if !length_check(vm, f, 1, &m1) || !length_check(vm, f, 2, &m2) {
        return Ok(Value::Null);
    }
    Ok(
        match intl::dcigettext(None, &m1, Some((&m2, n)), libc::LC_MESSAGES as i64) {
            Some(t) => sval(&t),
            None => plural_default(&m1, &m2, n),
        },
    )
}

fn dngettext(vm: &mut Vm, args: &[Value]) -> R {
    let f = "dngettext";
    let (domain, m1, m2) = (bytes(args, 0), bytes(args, 1), bytes(args, 2));
    let n = arg(args, 3).to_long() as u64;
    if !domain_check(vm, f, 1, &domain)
        || !length_check(vm, f, 2, &m1)
        || !length_check(vm, f, 3, &m2)
    {
        return Ok(Value::Null);
    }
    Ok(
        match intl::dcigettext(
            Some(cstr(&domain)),
            &m1,
            Some((&m2, n)),
            libc::LC_MESSAGES as i64,
        ) {
            Some(t) => sval(&t),
            None => plural_default(&m1, &m2, n),
        },
    )
}

fn dcngettext(vm: &mut Vm, args: &[Value]) -> R {
    let f = "dcngettext";
    let (domain, m1, m2) = (bytes(args, 0), bytes(args, 1), bytes(args, 2));
    let n = arg(args, 3).to_long() as u64;
    let category = arg(args, 4).to_long();
    if !domain_check(vm, f, 1, &domain)
        || !length_check(vm, f, 2, &m1)
        || !length_check(vm, f, 3, &m2)
        || !category_check(vm, f, 5, category)
    {
        return Ok(Value::Null);
    }
    Ok(
        match intl::dcigettext(Some(cstr(&domain)), &m1, Some((&m2, n)), category) {
            Some(t) => sval(&t),
            None => plural_default(&m1, &m2, n),
        },
    )
}

fn bind_textdomain_codeset(vm: &mut Vm, args: &[Value]) -> R {
    let f = "bind_textdomain_codeset";
    let domain = bytes(args, 0);
    if !domain_check(vm, f, 1, &domain) {
        return Ok(Value::Null);
    }
    let codeset = opt_bytes(args, 1);
    match intl::bind_textdomain_codeset(cstr(&domain), codeset.as_deref().map(cstr)) {
        Some(c) => Ok(sval(&c)),
        None => Ok(Value::False),
    }
}
