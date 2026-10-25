//! php-src's ext/filter: filter_var() & co (filter.c), the validation
//! filters (logical_filters.c), the sanitizing filters
//! (sanitizing_filters.c) and the callback filter, plus the SAPI input
//! filter (php_sapi_filter()) that keeps the raw request variables for
//! filter_input() and applies `filter.default` to the superglobals.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::ClassEntry;
use goro_core::opcode::OpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use goro_ext_standard::request;

mod logical;
mod sanitize;

/// The extension name (`extension_loaded('filter')`).
pub const NAME: &[u8] = b"filter";

type R = Result<Value, VmError>;

pub(crate) const FILTER_REQUIRE_ARRAY: i64 = 0x1000000;
pub(crate) const FILTER_REQUIRE_SCALAR: i64 = 0x2000000;
pub(crate) const FILTER_FORCE_ARRAY: i64 = 0x4000000;
pub(crate) const FILTER_NULL_ON_FAILURE: i64 = 0x8000000;
pub(crate) const FILTER_THROW_ON_FAILURE: i64 = 0x10000000;

pub(crate) const FILTER_FLAG_ALLOW_OCTAL: i64 = 0x0001;
pub(crate) const FILTER_FLAG_ALLOW_HEX: i64 = 0x0002;
pub(crate) const FILTER_FLAG_STRIP_LOW: i64 = 0x0004;
pub(crate) const FILTER_FLAG_STRIP_HIGH: i64 = 0x0008;
pub(crate) const FILTER_FLAG_ENCODE_LOW: i64 = 0x0010;
pub(crate) const FILTER_FLAG_ENCODE_HIGH: i64 = 0x0020;
pub(crate) const FILTER_FLAG_ENCODE_AMP: i64 = 0x0040;
pub(crate) const FILTER_FLAG_NO_ENCODE_QUOTES: i64 = 0x0080;
pub(crate) const FILTER_FLAG_EMPTY_STRING_NULL: i64 = 0x0100;
pub(crate) const FILTER_FLAG_STRIP_BACKTICK: i64 = 0x0200;
pub(crate) const FILTER_FLAG_ALLOW_FRACTION: i64 = 0x1000;
pub(crate) const FILTER_FLAG_ALLOW_THOUSAND: i64 = 0x2000;
pub(crate) const FILTER_FLAG_ALLOW_SCIENTIFIC: i64 = 0x4000;
pub(crate) const FILTER_FLAG_PATH_REQUIRED: i64 = 0x040000;
pub(crate) const FILTER_FLAG_QUERY_REQUIRED: i64 = 0x080000;
pub(crate) const FILTER_FLAG_IPV4: i64 = 0x00100000;
pub(crate) const FILTER_FLAG_IPV6: i64 = 0x00200000;
pub(crate) const FILTER_FLAG_NO_RES_RANGE: i64 = 0x00400000;
pub(crate) const FILTER_FLAG_NO_PRIV_RANGE: i64 = 0x00800000;
pub(crate) const FILTER_FLAG_GLOBAL_RANGE: i64 = 0x20000000;
pub(crate) const FILTER_FLAG_HOSTNAME: i64 = 0x100000;
pub(crate) const FILTER_FLAG_EMAIL_UNICODE: i64 = 0x100000;

const FILTER_VALIDATE_INT: i64 = 0x0101;
const FILTER_VALIDATE_BOOL: i64 = 0x0102;
const FILTER_VALIDATE_FLOAT: i64 = 0x0103;
const FILTER_VALIDATE_REGEXP: i64 = 0x0110;
const FILTER_VALIDATE_URL: i64 = 0x0111;
const FILTER_VALIDATE_EMAIL: i64 = 0x0112;
const FILTER_VALIDATE_IP: i64 = 0x0113;
const FILTER_VALIDATE_MAC: i64 = 0x0114;
const FILTER_VALIDATE_DOMAIN: i64 = 0x0115;
const FILTER_VALIDATE_LAST: i64 = 0x0115;
const FILTER_VALIDATE_ALL: i64 = 0x0100;
const FILTER_DEFAULT: i64 = 0x0204;
const FILTER_UNSAFE_RAW: i64 = 0x0204;
const FILTER_SANITIZE_STRING: i64 = 0x0201;
const FILTER_SANITIZE_ENCODED: i64 = 0x0202;
const FILTER_SANITIZE_SPECIAL_CHARS: i64 = 0x0203;
const FILTER_SANITIZE_EMAIL: i64 = 0x0205;
const FILTER_SANITIZE_URL: i64 = 0x0206;
const FILTER_SANITIZE_NUMBER_INT: i64 = 0x0207;
const FILTER_SANITIZE_NUMBER_FLOAT: i64 = 0x0208;
const FILTER_SANITIZE_FULL_SPECIAL_CHARS: i64 = 0x020a;
const FILTER_SANITIZE_ADD_SLASHES: i64 = 0x020b;
const FILTER_SANITIZE_LAST: i64 = 0x020b;
const FILTER_SANITIZE_ALL: i64 = 0x0200;
const FILTER_CALLBACK: i64 = 0x0400;

const FAILED_EXCEPTION: &[u8] = b"Filter\\FilterFailedException";

/// The context a filter runs in.
pub(crate) struct Ctx<'a> {
    pub flags: i64,
    /// The "options" array (or, for the callback filter, the callable).
    pub options: Option<&'a Value>,
    /// get_active_function_name()
    pub fname: &'a str,
}

impl Ctx<'_> {
    /// An entry of the options array.
    pub fn opt(&self, name: &str) -> Option<Value> {
        match self.options {
            Some(Value::Array(a)) => a.borrow().get_str(name.as_bytes()).map(|v| v.deref()),
            _ => None,
        }
    }
}

/// A filter: SUCCESS (true) or FAILURE (false, only with
/// FILTER_THROW_ON_FAILURE). `value` is a string on entry.
type FilterFn = fn(&mut Vm, &mut Value, &Ctx) -> bool;

struct FilterEntry {
    name: &'static str,
    id: i64,
    func: FilterFn,
}

const FILTER_LIST: &[FilterEntry] = &[
    FilterEntry {
        name: "int",
        id: FILTER_VALIDATE_INT,
        func: logical::filter_int,
    },
    FilterEntry {
        name: "boolean",
        id: FILTER_VALIDATE_BOOL,
        func: logical::filter_boolean,
    },
    FilterEntry {
        name: "float",
        id: FILTER_VALIDATE_FLOAT,
        func: logical::filter_float,
    },
    FilterEntry {
        name: "validate_regexp",
        id: FILTER_VALIDATE_REGEXP,
        func: logical::validate_regexp,
    },
    FilterEntry {
        name: "validate_domain",
        id: FILTER_VALIDATE_DOMAIN,
        func: logical::validate_domain,
    },
    FilterEntry {
        name: "validate_url",
        id: FILTER_VALIDATE_URL,
        func: logical::validate_url,
    },
    FilterEntry {
        name: "validate_email",
        id: FILTER_VALIDATE_EMAIL,
        func: logical::validate_email,
    },
    FilterEntry {
        name: "validate_ip",
        id: FILTER_VALIDATE_IP,
        func: logical::validate_ip,
    },
    FilterEntry {
        name: "validate_mac",
        id: FILTER_VALIDATE_MAC,
        func: logical::validate_mac,
    },
    FilterEntry {
        name: "string",
        id: FILTER_SANITIZE_STRING,
        func: sanitize::filter_string,
    },
    FilterEntry {
        name: "stripped",
        id: FILTER_SANITIZE_STRING,
        func: sanitize::filter_string,
    },
    FilterEntry {
        name: "encoded",
        id: FILTER_SANITIZE_ENCODED,
        func: sanitize::filter_encoded,
    },
    FilterEntry {
        name: "special_chars",
        id: FILTER_SANITIZE_SPECIAL_CHARS,
        func: sanitize::filter_special_chars,
    },
    FilterEntry {
        name: "full_special_chars",
        id: FILTER_SANITIZE_FULL_SPECIAL_CHARS,
        func: sanitize::filter_full_special_chars,
    },
    FilterEntry {
        name: "unsafe_raw",
        id: FILTER_UNSAFE_RAW,
        func: sanitize::filter_unsafe_raw,
    },
    FilterEntry {
        name: "email",
        id: FILTER_SANITIZE_EMAIL,
        func: sanitize::filter_email,
    },
    FilterEntry {
        name: "url",
        id: FILTER_SANITIZE_URL,
        func: sanitize::filter_url,
    },
    FilterEntry {
        name: "number_int",
        id: FILTER_SANITIZE_NUMBER_INT,
        func: sanitize::filter_number_int,
    },
    FilterEntry {
        name: "number_float",
        id: FILTER_SANITIZE_NUMBER_FLOAT,
        func: sanitize::filter_number_float,
    },
    FilterEntry {
        name: "add_slashes",
        id: FILTER_SANITIZE_ADD_SLASHES,
        func: sanitize::filter_add_slashes,
    },
    FilterEntry {
        name: "callback",
        id: FILTER_CALLBACK,
        func: filter_callback,
    },
];

/// PHP_FILTER_ID_EXISTS()
fn filter_id_exists(id: i64) -> bool {
    (FILTER_SANITIZE_ALL..=FILTER_SANITIZE_LAST).contains(&id)
        || (FILTER_VALIDATE_ALL..=FILTER_VALIDATE_LAST).contains(&id)
        || id == FILTER_CALLBACK
}

/// php_find_filter(): unknown ids fall back to the default filter.
fn find_filter(id: i64) -> &'static FilterEntry {
    FILTER_LIST
        .iter()
        .find(|f| f.id == id)
        .or_else(|| FILTER_LIST.iter().find(|f| f.id == FILTER_DEFAULT))
        .unwrap_or(&FILTER_LIST[0])
}

/// RETURN_VALIDATION_FAILED
pub(crate) fn validation_failed(vm: &Vm, value: &mut Value, flags: i64) -> bool {
    if vm.current_exception.is_some() {
        return true;
    }
    if flags & FILTER_THROW_ON_FAILURE != 0 {
        *value = Value::Null;
        return false;
    }
    *value = if flags & FILTER_NULL_ON_FAILURE != 0 {
        Value::Null
    } else {
        Value::False
    };
    true
}

pub(crate) fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
}

pub(crate) fn sval(v: Vec<u8>) -> Value {
    Value::String(PhpString::from_vec(v))
}

pub(crate) fn bytes(v: &Value) -> Vec<u8> {
    match v {
        Value::String(s) => s.as_bytes().to_vec(),
        other => other.to_php_string().as_bytes().to_vec(),
    }
}

// ---------------------------------------------------------------------------
// registration

const CONSTANTS: &[(&str, i64)] = &[
    ("INPUT_POST", request::PARSE_POST as i64),
    ("INPUT_GET", request::PARSE_GET as i64),
    ("INPUT_COOKIE", request::PARSE_COOKIE as i64),
    ("INPUT_ENV", request::PARSE_ENV as i64),
    ("INPUT_SERVER", request::PARSE_SERVER as i64),
    ("FILTER_FLAG_NONE", 0),
    ("FILTER_REQUIRE_SCALAR", FILTER_REQUIRE_SCALAR),
    ("FILTER_REQUIRE_ARRAY", FILTER_REQUIRE_ARRAY),
    ("FILTER_FORCE_ARRAY", FILTER_FORCE_ARRAY),
    ("FILTER_NULL_ON_FAILURE", FILTER_NULL_ON_FAILURE),
    ("FILTER_THROW_ON_FAILURE", FILTER_THROW_ON_FAILURE),
    ("FILTER_VALIDATE_INT", FILTER_VALIDATE_INT),
    ("FILTER_VALIDATE_BOOLEAN", FILTER_VALIDATE_BOOL),
    ("FILTER_VALIDATE_BOOL", FILTER_VALIDATE_BOOL),
    ("FILTER_VALIDATE_FLOAT", FILTER_VALIDATE_FLOAT),
    ("FILTER_VALIDATE_REGEXP", FILTER_VALIDATE_REGEXP),
    ("FILTER_VALIDATE_DOMAIN", FILTER_VALIDATE_DOMAIN),
    ("FILTER_VALIDATE_URL", FILTER_VALIDATE_URL),
    ("FILTER_VALIDATE_EMAIL", FILTER_VALIDATE_EMAIL),
    ("FILTER_VALIDATE_IP", FILTER_VALIDATE_IP),
    ("FILTER_VALIDATE_MAC", FILTER_VALIDATE_MAC),
    ("FILTER_DEFAULT", FILTER_DEFAULT),
    ("FILTER_UNSAFE_RAW", FILTER_UNSAFE_RAW),
    ("FILTER_SANITIZE_STRING", FILTER_SANITIZE_STRING),
    ("FILTER_SANITIZE_STRIPPED", FILTER_SANITIZE_STRING),
    ("FILTER_SANITIZE_ENCODED", FILTER_SANITIZE_ENCODED),
    (
        "FILTER_SANITIZE_SPECIAL_CHARS",
        FILTER_SANITIZE_SPECIAL_CHARS,
    ),
    (
        "FILTER_SANITIZE_FULL_SPECIAL_CHARS",
        FILTER_SANITIZE_FULL_SPECIAL_CHARS,
    ),
    ("FILTER_SANITIZE_EMAIL", FILTER_SANITIZE_EMAIL),
    ("FILTER_SANITIZE_URL", FILTER_SANITIZE_URL),
    ("FILTER_SANITIZE_NUMBER_INT", FILTER_SANITIZE_NUMBER_INT),
    ("FILTER_SANITIZE_NUMBER_FLOAT", FILTER_SANITIZE_NUMBER_FLOAT),
    ("FILTER_SANITIZE_ADD_SLASHES", FILTER_SANITIZE_ADD_SLASHES),
    ("FILTER_CALLBACK", FILTER_CALLBACK),
    ("FILTER_FLAG_ALLOW_OCTAL", FILTER_FLAG_ALLOW_OCTAL),
    ("FILTER_FLAG_ALLOW_HEX", FILTER_FLAG_ALLOW_HEX),
    ("FILTER_FLAG_STRIP_LOW", FILTER_FLAG_STRIP_LOW),
    ("FILTER_FLAG_STRIP_HIGH", FILTER_FLAG_STRIP_HIGH),
    ("FILTER_FLAG_STRIP_BACKTICK", FILTER_FLAG_STRIP_BACKTICK),
    ("FILTER_FLAG_ENCODE_LOW", FILTER_FLAG_ENCODE_LOW),
    ("FILTER_FLAG_ENCODE_HIGH", FILTER_FLAG_ENCODE_HIGH),
    ("FILTER_FLAG_ENCODE_AMP", FILTER_FLAG_ENCODE_AMP),
    ("FILTER_FLAG_NO_ENCODE_QUOTES", FILTER_FLAG_NO_ENCODE_QUOTES),
    (
        "FILTER_FLAG_EMPTY_STRING_NULL",
        FILTER_FLAG_EMPTY_STRING_NULL,
    ),
    ("FILTER_FLAG_ALLOW_FRACTION", FILTER_FLAG_ALLOW_FRACTION),
    ("FILTER_FLAG_ALLOW_THOUSAND", FILTER_FLAG_ALLOW_THOUSAND),
    ("FILTER_FLAG_ALLOW_SCIENTIFIC", FILTER_FLAG_ALLOW_SCIENTIFIC),
    ("FILTER_FLAG_PATH_REQUIRED", FILTER_FLAG_PATH_REQUIRED),
    ("FILTER_FLAG_QUERY_REQUIRED", FILTER_FLAG_QUERY_REQUIRED),
    ("FILTER_FLAG_IPV4", FILTER_FLAG_IPV4),
    ("FILTER_FLAG_IPV6", FILTER_FLAG_IPV6),
    ("FILTER_FLAG_NO_RES_RANGE", FILTER_FLAG_NO_RES_RANGE),
    ("FILTER_FLAG_NO_PRIV_RANGE", FILTER_FLAG_NO_PRIV_RANGE),
    ("FILTER_FLAG_GLOBAL_RANGE", FILTER_FLAG_GLOBAL_RANGE),
    ("FILTER_FLAG_HOSTNAME", FILTER_FLAG_HOSTNAME),
    ("FILTER_FLAG_EMAIL_UNICODE", FILTER_FLAG_EMAIL_UNICODE),
];

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("filter_has_var", filter_has_var),
    ("filter_input", filter_input),
    ("filter_var", filter_var),
    ("filter_input_array", filter_input_array),
    ("filter_var_array", filter_var_array),
    ("filter_list", filter_list),
    ("filter_id", filter_id),
];

/// MINIT: functions, constants, INI entries, classes and the SAPI input
/// filter.
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
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
    for &(name, v) in CONSTANTS {
        vm.constants
            .insert(name.as_bytes().to_vec(), Value::Long(v));
    }
    for name in ["FILTER_SANITIZE_STRING", "FILTER_SANITIZE_STRIPPED"] {
        vm.internal_deprecated_constants.insert(
            name.as_bytes().to_vec(),
            format!(
                "Constant {} is deprecated since 8.1, use htmlspecialchars() instead",
                name
            ),
        );
    }
    vm.module_constants.push((
        NAME.to_vec(),
        CONSTANTS
            .iter()
            .map(|(n, _)| n.as_bytes().to_vec())
            .collect(),
    ));
    // INI (PHP_INI_SYSTEM | PHP_INI_PERDIR)
    vm.constants.insert(
        b"filter.default".to_vec(),
        Value::String(PhpString::from_bytes(b"unsafe_raw")),
    );
    vm.constants
        .insert(b"filter.default_flags".to_vec(), Value::Null);
    for k in [&b"filter.default"[..], b"filter.default_flags"] {
        vm.ini_set_hooks.insert(k.to_vec(), reject_runtime);
    }
    vm.startup_hooks.push(startup);
    request::set_input_filter(sapi_filter);
    register_classes(vm);
}

fn reject_runtime(_vm: &mut Vm, _name: &[u8], _value: &Value) -> bool {
    false
}

/// IF_G(default_filter) (UpdateDefaultFilter)
fn default_filter(vm: &Vm) -> i64 {
    let v = vm
        .constants
        .get(b"filter.default".as_slice())
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    FILTER_LIST
        .iter()
        .find(|f| f.name.as_bytes().eq_ignore_ascii_case(&v))
        .map(|f| f.id)
        .unwrap_or(FILTER_DEFAULT)
}

/// IF_G(default_filter_flags) (OnUpdateFlags)
fn default_filter_flags(vm: &Vm) -> i64 {
    match vm.constants.get(b"filter.default_flags".as_slice()) {
        None | Some(Value::Null) => FILTER_FLAG_NO_ENCODE_QUOTES,
        Some(v) => atoi(v.to_php_string().as_bytes()),
    }
}

fn atoi(s: &[u8]) -> i64 {
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_whitespace() {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut n: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        n = n.wrapping_mul(10).wrapping_add((s[i] - b'0') as i64);
        i += 1;
    }
    if neg { n.wrapping_neg() } else { n }
}

/// The deprecation UpdateDefaultFilter raises at startup.
fn startup(vm: &mut Vm) {
    if default_filter(vm) != FILTER_DEFAULT && vm.error_reporting & 8192 != 0 {
        vm.display_error(
            "Deprecated",
            "The filter.default ini setting is deprecated",
            "Unknown",
            0,
        );
    }
}

/// php_sapi_filter(): the value a request variable is registered with in
/// the superglobal (the raw one is kept by the request module).
fn sapi_filter(vm: &mut Vm, _arg: u8, val: &[u8]) -> Value {
    let filter = default_filter(vm);
    let mut v = sval(val.to_vec());
    if filter != FILTER_UNSAFE_RAW {
        let flags = default_filter_flags(vm);
        zval_filter(vm, &mut v, filter, flags, None, "Unknown");
    }
    v
}

fn register_classes(vm: &mut Vm) {
    const PRELUDE: &[u8] = include_bytes!("prelude.php");
    let Some((op_array, classes)) =
        goro_core::script_cache::prelude("filter", || compile_prelude(PRELUDE))
    else {
        return;
    };
    let saved_file = std::mem::take(&mut vm.current_file);
    let saved_line = vm.current_line;
    let _ = vm.run_compiled(&op_array, classes);
    vm.current_file = saved_file;
    vm.current_line = saved_line;
    vm.current_exception = None;
    for c in [
        &b"filter\\filterexception"[..],
        b"filter\\filterfailedexception",
    ] {
        if let Some(ce) = vm.classes.get_mut(c) {
            ce.filename = None;
            ce.start_line = 0;
            ce.end_line = 0;
        }
    }
}

fn compile_prelude(src: &[u8]) -> Option<(OpArray, Vec<ClassEntry>)> {
    let mut lexer = goro_parser::Lexer::new(src);
    let tokens = lexer.tokenize();
    let _ = goro_parser::take_lexer_deprecations();
    let _ = goro_parser::take_lexer_warnings();
    let mut parser = goro_parser::Parser::new(tokens).with_source(src);
    let program = parser.parse().ok()?;
    let compiler = goro_core::compiler::Compiler::new();
    let (mut op_array, classes, _) = compiler.compile(&program).ok()?;
    let _ = goro_core::compiler::take_pending_compile_warnings();
    for o in &mut op_array.ops {
        o.line = 0;
    }
    op_array.filename.clear();
    Some((op_array, classes))
}

// ---------------------------------------------------------------------------
// filtering

/// php_zval_filter()
fn zval_filter(
    vm: &mut Vm,
    value: &mut Value,
    filter: i64,
    flags: i64,
    options: Option<&Value>,
    fname: &str,
) {
    let entry = find_filter(filter);
    let mut skip_filter = false;
    // #49274: objects without __toString() fail nicely.
    if let Value::Object(o) = &value {
        let (has, class) = {
            let ob = o.borrow();
            let lower = ob.class_name.to_ascii_lowercase();
            let has = vm
                .classes
                .get(&lower)
                .is_some_and(|c| c.get_method(b"__tostring").is_some())
                || has_native_tostring(&lower);
            (has, goro_core::value::display_class_name(&ob.class_name))
        };
        if !has {
            if flags & FILTER_THROW_ON_FAILURE != 0 {
                throw(
                    vm,
                    FAILED_EXCEPTION,
                    &format!(
                        "filter validation failed: object of type {} has no __toString() method",
                        class
                    ),
                );
                *value = Value::Null;
                return;
            }
            *value = if flags & FILTER_NULL_ON_FAILURE != 0 {
                Value::Null
            } else {
                Value::False
            };
            skip_filter = true;
        }
    }
    if !skip_filter {
        // Here be strings
        let s = vm.value_to_string(value);
        if vm.current_exception.is_some() {
            return;
        }
        *value = Value::String(s.clone());
        let ctx = Ctx {
            flags,
            options,
            fname,
        };
        let ok = (entry.func)(vm, value, &ctx);
        if flags & FILTER_THROW_ON_FAILURE != 0 && !ok {
            throw(
                vm,
                FAILED_EXCEPTION,
                &format!(
                    "filter validation failed: filter {} not satisfied by '{}'",
                    entry.name,
                    s.to_msg_string()
                ),
            );
            return;
        }
    }
    // handle_default:
    if let Some(Value::Array(opts)) = options {
        let failed = if flags & FILTER_NULL_ON_FAILURE != 0 {
            matches!(value, Value::Null)
        } else {
            matches!(value, Value::False)
        };
        if failed {
            if let Some(d) = opts.borrow().get_str(b"default") {
                *value = d.deref();
            }
        }
    }
}

/// Internal classes implemented natively that have __toString().
fn has_native_tostring(lower: &[u8]) -> bool {
    goro_core::stubs::find_class_lower(lower).is_some_and(|c| {
        c.methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case("__toString"))
    })
}

/// php_zval_filter_recursive()
fn zval_filter_recursive(
    vm: &mut Vm,
    value: &mut Value,
    filter: i64,
    flags: i64,
    options: Option<&Value>,
    fname: &str,
    depth: usize,
) {
    if let Value::Array(a) = value {
        if depth > 256 {
            return;
        }
        // Separate the array, then filter its elements in place.
        let mut arr = a.borrow().clone();
        let keys: Vec<ArrayKey> = arr.iter().map(|(k, _)| k.clone()).collect();
        for k in keys {
            // A reference (shared by the caller) is filtered in place.
            if let Some(Value::Reference(r)) = arr.get(&k) {
                if Rc::strong_count(r) > 1 {
                    let r = r.clone();
                    let mut inner = r.borrow().clone();
                    if let Value::Array(_) = &inner {
                        zval_filter_recursive(
                            vm,
                            &mut inner,
                            filter,
                            flags,
                            options,
                            fname,
                            depth + 1,
                        );
                    } else {
                        zval_filter(vm, &mut inner, filter, flags, options, fname);
                    }
                    *r.borrow_mut() = inner;
                    if vm.current_exception.is_some() {
                        break;
                    }
                    continue;
                }
            }
            let mut el = arr.get(&k).map(|v| v.deref()).unwrap_or(Value::Null);
            if let Value::Array(inner) = &el {
                // Recursion protection (Z_IS_RECURSIVE)
                if Rc::ptr_eq(inner, a) {
                    continue;
                }
                let mut copy = Value::Array(Rc::new(RefCell::new(inner.borrow().clone())));
                zval_filter_recursive(vm, &mut copy, filter, flags, options, fname, depth + 1);
                el = copy;
            } else {
                zval_filter(vm, &mut el, filter, flags, options, fname);
            }
            arr.set(k, el);
            if vm.current_exception.is_some() {
                break;
            }
        }
        *value = Value::Array(Rc::new(RefCell::new(arr)));
    } else {
        zval_filter(vm, value, filter, flags, options, fname);
    }
}

/// The `$options` argument: an array (filter_args_ht) or an int.
enum FilterArgs<'a> {
    Ht(&'a PhpArray),
    Long(i64),
}

/// zval_get_long(): objects without a cast handler warn and become 1.
fn zval_get_long(vm: &mut Vm, v: &Value) -> i64 {
    match v {
        Value::Object(o) => {
            if let Some(c) = goro_core::value::native_cast(o, 1) {
                return c.to_long();
            }
            if let Some(n) = vm.object_to_long.and_then(|f| f(v)) {
                return n;
            }
            let cn = goro_core::value::display_class_name(&o.borrow().class_name);
            vm.emit_warning(&format!(
                "Object of class {} could not be converted to int",
                cn
            ));
            1
        }
        other => other.to_long(),
    }
}

/// php_filter_call()
fn filter_call(
    vm: &mut Vm,
    filtered: &mut Value,
    mut filter: i64,
    args: FilterArgs,
    mut filter_flags: i64,
    options_arg_num: u32,
    fname: &str,
) {
    let mut options: Option<Value> = None;
    match args {
        FilterArgs::Long(l) => {
            if filter != -1 {
                filter_flags = l;
                if filter_flags & (FILTER_REQUIRE_ARRAY | FILTER_FORCE_ARRAY) == 0 {
                    filter_flags |= FILTER_REQUIRE_SCALAR;
                }
            } else {
                filter = l;
            }
        }
        FilterArgs::Ht(ht) => {
            if let Some(v) = ht.get_str(b"filter") {
                filter = zval_get_long(vm, &v.deref());
            }
            if let Some(v) = ht.get_str(b"options") {
                let v = v.deref();
                if filter != FILTER_CALLBACK {
                    if matches!(v, Value::Array(_)) {
                        options = Some(v);
                    }
                } else {
                    options = Some(v);
                    filter_flags = 0;
                }
            }
            if let Some(v) = ht.get_str(b"flags") {
                filter_flags = zval_get_long(vm, &v.deref());
                if filter_flags & (FILTER_REQUIRE_ARRAY | FILTER_FORCE_ARRAY) == 0 {
                    filter_flags |= FILTER_REQUIRE_SCALAR;
                }
            }
        }
    }
    if filter_flags & FILTER_NULL_ON_FAILURE != 0 && filter_flags & FILTER_THROW_ON_FAILURE != 0 {
        *filtered = Value::Null;
        let name = arg_name(fname, options_arg_num);
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #{} (${}) cannot use both FILTER_NULL_ON_FAILURE and FILTER_THROW_ON_FAILURE",
                fname, options_arg_num, name
            ),
        );
        return;
    }
    if matches!(filtered, Value::Array(_)) {
        if filter_flags & FILTER_REQUIRE_SCALAR != 0 {
            if filter_flags & FILTER_THROW_ON_FAILURE != 0 {
                *filtered = Value::Null;
                throw(
                    vm,
                    FAILED_EXCEPTION,
                    "filter validation failed: not a scalar value (got an array)",
                );
            } else if filter_flags & FILTER_NULL_ON_FAILURE != 0 {
                *filtered = Value::Null;
            } else {
                *filtered = Value::False;
            }
            return;
        }
        zval_filter_recursive(
            vm,
            filtered,
            filter,
            filter_flags,
            options.as_ref(),
            fname,
            0,
        );
        return;
    }
    if filter_flags & FILTER_REQUIRE_ARRAY != 0 {
        if filter_flags & FILTER_THROW_ON_FAILURE != 0 {
            throw(
                vm,
                FAILED_EXCEPTION,
                &format!(
                    "filter validation failed: not an array (got {})",
                    Vm::value_type_name(filtered)
                ),
            );
            *filtered = Value::Null;
            return;
        }
        *filtered = if filter_flags & FILTER_NULL_ON_FAILURE != 0 {
            Value::Null
        } else {
            Value::False
        };
        return;
    }
    zval_filter(vm, filtered, filter, filter_flags, options.as_ref(), fname);
    if vm.current_exception.is_some() {
        return;
    }
    if filter_flags & FILTER_FORCE_ARRAY != 0 {
        let mut arr = PhpArray::new();
        arr.push(std::mem::replace(filtered, Value::Null));
        *filtered = Value::Array(Rc::new(RefCell::new(arr)));
    }
}

fn arg_name(fname: &str, n: u32) -> &'static str {
    goro_core::stubs::find_function(fname.as_bytes())
        .and_then(|f| f.args.get(n as usize - 1))
        .map(|a| a.name)
        .unwrap_or("options")
}

/// php_filter_array_handler()
fn filter_array_handler(
    vm: &mut Vm,
    input: &PhpArray,
    op: FilterArgs,
    add_empty: bool,
    fname: &str,
) -> R {
    match op {
        FilterArgs::Long(l) => {
            let mut rv = Value::Array(Rc::new(RefCell::new(input.clone())));
            filter_call(
                vm,
                &mut rv,
                -1,
                FilterArgs::Long(l),
                FILTER_REQUIRE_ARRAY,
                2,
                fname,
            );
            Ok(rv)
        }
        FilterArgs::Ht(ht) => {
            let mut out = PhpArray::new();
            for (k, elm) in ht.iter() {
                let key = match k {
                    ArrayKey::Int(_) => {
                        throw(
                            vm,
                            b"TypeError",
                            &format!(
                                "{}(): Argument #2 ($options) must contain only string keys",
                                fname
                            ),
                        );
                        return Ok(Value::Null);
                    }
                    ArrayKey::String(s) => s.clone(),
                };
                if key.as_bytes().is_empty() {
                    throw(
                        vm,
                        b"ValueError",
                        &format!(
                            "{}(): Argument #2 ($options) cannot contain empty keys",
                            fname
                        ),
                    );
                    return Ok(Value::Null);
                }
                match input.get_str(key.as_bytes()) {
                    None => {
                        if add_empty {
                            out.set(ArrayKey::String(key), Value::Null);
                        }
                    }
                    Some(tmp) => {
                        let mut nval = dup(&tmp.deref());
                        let elm = elm.deref();
                        let fa = match &elm {
                            Value::Array(_) => None,
                            other => {
                                let id = zval_get_long(vm, other);
                                if !filter_id_exists(id) {
                                    vm.emit_warning(&format!(
                                        "{}(): Unknown filter with ID {}",
                                        fname, id
                                    ));
                                }
                                // php_filter_call() is passed a second
                                // zval_get_long() of the definition.
                                Some(zval_get_long(vm, other))
                            }
                        };
                        match (&elm, fa) {
                            (Value::Array(a), _) => {
                                let a = a.borrow().clone();
                                filter_call(
                                    vm,
                                    &mut nval,
                                    -1,
                                    FilterArgs::Ht(&a),
                                    FILTER_REQUIRE_SCALAR,
                                    2,
                                    fname,
                                );
                            }
                            (_, Some(id)) => filter_call(
                                vm,
                                &mut nval,
                                -1,
                                FilterArgs::Long(id),
                                FILTER_REQUIRE_SCALAR,
                                2,
                                fname,
                            ),
                            _ => {}
                        }
                        if vm.current_exception.is_some() {
                            return Ok(Value::Null);
                        }
                        out.set(ArrayKey::String(key), nval);
                    }
                }
            }
            Ok(Value::Array(Rc::new(RefCell::new(out))))
        }
    }
}

/// ZVAL_DUP(): arrays are separated.
fn dup(v: &Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(Rc::new(RefCell::new(a.borrow().clone()))),
        other => other.clone(),
    }
}

/// php_filter_get_storage(): None (with ValueError when the source is
/// invalid) or the input array of the source.
fn get_storage(vm: &mut Vm, arg: i64) -> Result<Option<Rc<RefCell<PhpArray>>>, ()> {
    let a = match arg {
        x if x == request::PARSE_GET as i64
            || x == request::PARSE_POST as i64
            || x == request::PARSE_COOKIE as i64
            || x == request::PARSE_SERVER as i64 =>
        {
            request::raw_input(x as u8)
        }
        x if x == request::PARSE_ENV as i64 => {
            request::raw_input(request::PARSE_ENV).or_else(|| match vm.get_global(b"_ENV") {
                Some(Value::Array(a)) => Some(a),
                Some(Value::Reference(r)) => match &*r.borrow() {
                    Value::Array(a) => Some(a.clone()),
                    _ => None,
                },
                _ => None,
            })
        }
        _ => {
            throw(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #1 (${}) must be an INPUT_* constant",
                    current_fname(vm),
                    if current_fname(vm) == "filter_has_var" {
                        "input_type"
                    } else {
                        "type"
                    }
                ),
            );
            return Err(());
        }
    };
    Ok(a)
}

thread_local! {
    static CURRENT_FN: std::cell::Cell<&'static str> = const { std::cell::Cell::new("") };
}

fn current_fname(_vm: &Vm) -> &'static str {
    CURRENT_FN.with(|c| c.get())
}

fn set_fname(name: &'static str) {
    CURRENT_FN.with(|c| c.set(name));
}

fn arg(args: &[Value], i: usize) -> Value {
    match args.get(i) {
        Some(v) => v.deref(),
        None => Value::Undef,
    }
}

/// Z_PARAM_ARRAY_HT_OR_LONG (already type-checked by the arginfo layer).
fn ht_or_long(v: &Value, default: i64) -> Result<Option<PhpArray>, i64> {
    match v {
        Value::Array(a) => Ok(Some(a.borrow().clone())),
        Value::Undef => Err(default),
        other => Err(other.to_long()),
    }
}

fn filter_has_var(vm: &mut Vm, args: &[Value]) -> R {
    set_fname("filter_has_var");
    let t = arg(args, 0).to_long();
    let name = arg(args, 1).to_php_string();
    let Ok(storage) = get_storage(vm, t) else {
        return Ok(Value::Null);
    };
    let found = storage.is_some_and(|a| a.borrow().get_str(name.as_bytes()).is_some());
    Ok(if found { Value::True } else { Value::False })
}

fn filter_input(vm: &mut Vm, args: &[Value]) -> R {
    set_fname("filter_input");
    let fetch_from = arg(args, 0).to_long();
    let var = arg(args, 1).to_php_string();
    let filter = match arg(args, 2) {
        Value::Undef => FILTER_DEFAULT,
        v => v.to_long(),
    };
    let fa = ht_or_long(&arg(args, 3), 0);
    if !filter_id_exists(filter) {
        vm.emit_warning(&format!(
            "filter_input(): Unknown filter with ID {}",
            filter
        ));
        return Ok(Value::False);
    }
    let Ok(input) = get_storage(vm, fetch_from) else {
        return Ok(Value::Null);
    };
    let tmp = input.and_then(|a| a.borrow().get_str(var.as_bytes()).map(|v| v.deref()));
    let Some(tmp) = tmp else {
        let filter_flags = match &fa {
            Err(l) => *l,
            Ok(Some(ht)) => match ht.get_str(b"flags") {
                Some(v) => zval_get_long(vm, &v.deref()),
                None => 0,
            },
            Ok(None) => 0,
        };
        if filter_flags & FILTER_NULL_ON_FAILURE != 0 && filter_flags & FILTER_THROW_ON_FAILURE != 0
        {
            throw(
                vm,
                b"ValueError",
                "filter_input(): Argument #4 ($options) cannot use both FILTER_NULL_ON_FAILURE and FILTER_THROW_ON_FAILURE",
            );
            return Ok(Value::Null);
        }
        if filter_flags & FILTER_THROW_ON_FAILURE != 0 {
            throw(
                vm,
                FAILED_EXCEPTION,
                &format!("input value '{}' not found", var.to_msg_string()),
            );
            return Ok(Value::Null);
        }
        if let Ok(Some(ht)) = &fa {
            if let Some(Value::Array(opt)) = ht.get_str(b"options").map(|v| v.deref()) {
                if let Some(def) = opt.borrow().get_str(b"default") {
                    return Ok(def.deref());
                }
            }
        }
        return Ok(if filter_flags & FILTER_NULL_ON_FAILURE != 0 {
            Value::False
        } else {
            Value::Null
        });
    };
    let mut rv = dup(&tmp);
    match &fa {
        Ok(Some(ht)) => filter_call(
            vm,
            &mut rv,
            filter,
            FilterArgs::Ht(ht),
            FILTER_REQUIRE_SCALAR,
            4,
            "filter_input",
        ),
        Ok(None) => filter_call(
            vm,
            &mut rv,
            filter,
            FilterArgs::Long(0),
            FILTER_REQUIRE_SCALAR,
            4,
            "filter_input",
        ),
        Err(l) => filter_call(
            vm,
            &mut rv,
            filter,
            FilterArgs::Long(*l),
            FILTER_REQUIRE_SCALAR,
            4,
            "filter_input",
        ),
    }
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    Ok(rv)
}

fn filter_var(vm: &mut Vm, args: &[Value]) -> R {
    set_fname("filter_var");
    let data = arg(args, 0);
    let data = if matches!(data, Value::Undef) {
        Value::Null
    } else {
        data
    };
    let filter = match arg(args, 1) {
        Value::Undef => FILTER_DEFAULT,
        v => v.to_long(),
    };
    let fa = ht_or_long(&arg(args, 2), 0);
    if !filter_id_exists(filter) {
        vm.emit_warning(&format!("filter_var(): Unknown filter with ID {}", filter));
        return Ok(Value::False);
    }
    let mut rv = dup(&data);
    match &fa {
        Ok(Some(ht)) => filter_call(
            vm,
            &mut rv,
            filter,
            FilterArgs::Ht(ht),
            FILTER_REQUIRE_SCALAR,
            3,
            "filter_var",
        ),
        Ok(None) => filter_call(
            vm,
            &mut rv,
            filter,
            FilterArgs::Long(0),
            FILTER_REQUIRE_SCALAR,
            3,
            "filter_var",
        ),
        Err(l) => filter_call(
            vm,
            &mut rv,
            filter,
            FilterArgs::Long(*l),
            FILTER_REQUIRE_SCALAR,
            3,
            "filter_var",
        ),
    }
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    Ok(rv)
}

fn add_empty_arg(args: &[Value]) -> bool {
    match arg(args, 2) {
        Value::Undef => true,
        v => v.is_truthy(),
    }
}

fn filter_input_array(vm: &mut Vm, args: &[Value]) -> R {
    set_fname("filter_input_array");
    let fetch_from = arg(args, 0).to_long();
    let op = ht_or_long(&arg(args, 1), FILTER_DEFAULT);
    let add_empty = add_empty_arg(args);
    if let Err(l) = op {
        if !filter_id_exists(l) {
            vm.emit_warning(&format!(
                "filter_input_array(): Unknown filter with ID {}",
                l
            ));
            return Ok(Value::False);
        }
    }
    let Ok(input) = get_storage(vm, fetch_from) else {
        return Ok(Value::Null);
    };
    let Some(input) = input else {
        return Ok(Value::Null);
    };
    let input = input.borrow().clone();
    let r = match &op {
        Ok(Some(ht)) => filter_array_handler(
            vm,
            &input,
            FilterArgs::Ht(ht),
            add_empty,
            "filter_input_array",
        ),
        Ok(None) => filter_array_handler(
            vm,
            &input,
            FilterArgs::Long(0),
            add_empty,
            "filter_input_array",
        ),
        Err(l) => filter_array_handler(
            vm,
            &input,
            FilterArgs::Long(*l),
            add_empty,
            "filter_input_array",
        ),
    }?;
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    Ok(r)
}

fn filter_var_array(vm: &mut Vm, args: &[Value]) -> R {
    set_fname("filter_var_array");
    let Value::Array(input) = arg(args, 0) else {
        return Ok(Value::Null);
    };
    let op = ht_or_long(&arg(args, 1), FILTER_DEFAULT);
    let add_empty = add_empty_arg(args);
    if let Err(l) = op {
        if !filter_id_exists(l) {
            vm.emit_warning(&format!("filter_var_array(): Unknown filter with ID {}", l));
            return Ok(Value::False);
        }
    }
    let input = input.borrow().clone();
    let r = match &op {
        Ok(Some(ht)) => filter_array_handler(
            vm,
            &input,
            FilterArgs::Ht(ht),
            add_empty,
            "filter_var_array",
        ),
        Ok(None) => filter_array_handler(
            vm,
            &input,
            FilterArgs::Long(0),
            add_empty,
            "filter_var_array",
        ),
        Err(l) => filter_array_handler(
            vm,
            &input,
            FilterArgs::Long(*l),
            add_empty,
            "filter_var_array",
        ),
    }?;
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    Ok(r)
}

fn filter_list(_vm: &mut Vm, _args: &[Value]) -> R {
    let mut arr = PhpArray::new();
    for f in FILTER_LIST {
        arr.push(sval(f.name.as_bytes().to_vec()));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn filter_id(_vm: &mut Vm, args: &[Value]) -> R {
    let name = arg(args, 0).to_php_string();
    // strcmp(): the name ends at a NUL byte.
    let n = name.as_bytes();
    let n = &n[..n.iter().position(|&c| c == 0).unwrap_or(n.len())];
    Ok(FILTER_LIST
        .iter()
        .find(|f| f.name.as_bytes() == n)
        .map(|f| Value::Long(f.id))
        .unwrap_or(Value::False))
}

/// php_filter_callback()
fn filter_callback(vm: &mut Vm, value: &mut Value, ctx: &Ctx) -> bool {
    let callable = ctx.options.cloned();
    let ok = callable.as_ref().is_some_and(|c| {
        vm.zend_is_callable_ex(c, goro_core::callable::IS_CALLABLE_SUPPRESS_DEPRECATIONS)
            .ok
    });
    if !ok {
        throw(
            vm,
            b"TypeError",
            &format!("{}(): Option must be a valid callback", ctx.fname),
        );
        *value = Value::Null;
        return true;
    }
    let arg = value.clone();
    match vm.call_callback(&callable.unwrap(), &[arg]) {
        Ok(v) if vm.current_exception.is_none() => {
            *value = match v {
                Value::Undef => Value::Null,
                Value::Reference(r) => r.borrow().clone(),
                other => other,
            };
        }
        _ => *value = Value::Null,
    }
    true
}
