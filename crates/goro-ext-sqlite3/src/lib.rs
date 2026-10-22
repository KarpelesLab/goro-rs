//! php-src's ext/sqlite3 (sqlite3.c): the SQLite3, SQLite3Stmt,
//! SQLite3Result and SQLite3Exception classes on goro-sqlite, a pure-Rust
//! port of the SQLite engine.

use goro_core::object::ClassEntry;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, Vm};
use goro_sqlite::consts::*;

mod blob;
mod callbacks;
mod conn;
mod db;
mod result;
mod stmt;
mod zpp;

use conn::{Shared, active_name, pop_active, push_active};
use zpp::{Func, Obj, throw, throw_with_code};

/// The extension name (`extension_loaded('sqlite3')`).
pub const NAME: &[u8] = b"sqlite3";

/// PHP_SQLITE3_ASSOC & co.
pub const PHP_SQLITE3_ASSOC: i64 = 1;
pub const PHP_SQLITE3_NUM: i64 = 2;
pub const PHP_SQLITE3_BOTH: i64 = PHP_SQLITE3_ASSOC | PHP_SQLITE3_NUM;

/// php_sqlite3_error(): an SQLite3Exception in exception mode, else a
/// warning (always a warning without a database object).
pub fn php_sqlite3_error(vm: &mut Vm, db: Option<&Shared>, errcode: i32, msg: &str) {
    if db.is_some_and(|d| d.exception.get()) {
        throw_with_code(vm, b"SQLite3Exception", msg, errcode as i64);
    } else {
        let name = active_name(vm);
        let line = vm.current_line;
        if name.is_empty() {
            vm.emit_warning_at(msg, line);
        } else {
            vm.emit_warning_at(&format!("{}(): {}", name, msg), line);
        }
    }
}

/// sqlite3_errmsg() as a Rust string
pub fn errmsg(db: &mut goro_sqlite::int::Sqlite3) -> String {
    let m = goro_sqlite::mainapi::sqlite3_errmsg(db).unwrap_or_default();
    String::from_utf8_lossy(zpp::cstr(&m)).into_owned()
}

type Impl = fn(&mut Vm, &Func, Option<&Obj>, &[Value]) -> Value;

/// (class, method as declared, implementation, static)
const METHODS: &[(&str, &str, Impl, bool)] = &[
    ("SQLite3", "open", db::open, false),
    ("SQLite3", "close", db::close, false),
    ("SQLite3", "version", db::version, true),
    ("SQLite3", "lastInsertRowID", db::last_insert_rowid, false),
    ("SQLite3", "lastErrorCode", db::last_error_code, false),
    (
        "SQLite3",
        "lastExtendedErrorCode",
        db::last_extended_error_code,
        false,
    ),
    ("SQLite3", "lastErrorMsg", db::last_error_msg, false),
    ("SQLite3", "changes", db::changes, false),
    ("SQLite3", "busyTimeout", db::busy_timeout, false),
    ("SQLite3", "loadExtension", db::load_extension, false),
    ("SQLite3", "backup", db::backup, false),
    ("SQLite3", "escapeString", db::escape_string, true),
    ("SQLite3", "prepare", db::prepare, false),
    ("SQLite3", "exec", db::exec, false),
    ("SQLite3", "query", db::query, false),
    ("SQLite3", "querySingle", db::query_single, false),
    ("SQLite3", "createFunction", db::create_function, false),
    ("SQLite3", "createAggregate", db::create_aggregate, false),
    ("SQLite3", "createCollation", db::create_collation, false),
    ("SQLite3", "openBlob", blob::open_blob, false),
    ("SQLite3", "enableExceptions", db::enable_exceptions, false),
    (
        "SQLite3",
        "enableExtendedResultCodes",
        db::enable_extended_result_codes,
        false,
    ),
    ("SQLite3", "setAuthorizer", db::set_authorizer, false),
    ("SQLite3Stmt", "bindParam", stmt::bind_param, false),
    ("SQLite3Stmt", "bindValue", stmt::bind_value, false),
    ("SQLite3Stmt", "clear", stmt::clear, false),
    ("SQLite3Stmt", "close", stmt::close, false),
    ("SQLite3Stmt", "execute", stmt::execute, false),
    ("SQLite3Stmt", "getSQL", stmt::get_sql, false),
    ("SQLite3Stmt", "paramCount", stmt::param_count, false),
    ("SQLite3Stmt", "readOnly", stmt::read_only, false),
    ("SQLite3Stmt", "reset", stmt::reset, false),
    ("SQLite3Stmt", "busy", stmt::busy, false),
    ("SQLite3Stmt", "explain", stmt::explain, false),
    ("SQLite3Stmt", "setExplain", stmt::set_explain, false),
    ("SQLite3Result", "numColumns", result::num_columns, false),
    ("SQLite3Result", "columnName", result::column_name_m, false),
    ("SQLite3Result", "columnType", result::column_type, false),
    ("SQLite3Result", "fetchArray", result::fetch_array, false),
    ("SQLite3Result", "fetchAll", result::fetch_all, false),
    ("SQLite3Result", "reset", result::reset, false),
    ("SQLite3Result", "finalize", result::finalize, false),
];

fn class_of(class_lower: &[u8]) -> Option<&'static str> {
    match class_lower {
        b"sqlite3" => Some("SQLite3"),
        b"sqlite3stmt" => Some("SQLite3Stmt"),
        b"sqlite3result" => Some("SQLite3Result"),
        _ => None,
    }
}

fn find_method(
    class: &str,
    method_lower: &[u8],
) -> Option<&'static (&'static str, &'static str, Impl, bool)> {
    METHODS
        .iter()
        .find(|(c, m, _, _)| *c == class && m.as_bytes().eq_ignore_ascii_case(method_lower))
}

fn has_method(class: &[u8], method: &[u8], is_static: bool) -> bool {
    let Some(c) = class_of(class) else {
        return false;
    };
    match find_method(c, &method.to_ascii_lowercase()) {
        Some(m) => !is_static || m.3,
        None => false,
    }
}

fn this_obj(this: Option<&Value>) -> Option<Obj> {
    match this {
        Some(Value::Object(o)) => Some(o.clone()),
        Some(Value::Reference(r)) => match &*r.borrow() {
            Value::Object(o) => Some(o.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// Run a method implementation with its frame and docref name.
fn run_method(
    vm: &mut Vm,
    f: &Func,
    frame: String,
    this: Option<&Obj>,
    args: &[Value],
    imp: Impl,
) -> Value {
    let has = vm
        .call_stack
        .last()
        .map(|fr| fr.0.eq_ignore_ascii_case(&frame))
        .unwrap_or(false);
    if !has {
        let line = vm.current_line;
        let file = vm.current_file.clone();
        vm.call_stack.push((frame, file, line, args.to_vec(), true));
    }
    push_active(&f.name);
    let r = imp(vm, f, this, args);
    pop_active();
    if !has {
        vm.call_stack.pop();
    }
    r
}

fn call_method(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let Some(c) = class_of(class) else {
        return Value::Null;
    };
    let ml = method.to_ascii_lowercase();
    let Some(m) = find_method(c, &ml) else {
        return Value::Null;
    };
    let this_o = this_obj(this);
    if !m.3 && this_o.is_none() {
        let msg = format!(
            "Non-static method {}::{}() cannot be called statically",
            m.0, m.1
        );
        throw(vm, b"Error", &msg);
        return Value::Null;
    }
    let f = Func::method(m.0, m.1);
    let frame = if m.3 {
        format!("{}::{}", m.0, m.1)
    } else {
        format!("{}->{}", m.0, m.1)
    };
    let this_o = if m.3 { None } else { this_o };
    run_method(vm, &f, frame, this_o.as_ref(), args, m.2)
}

fn param_names(class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    let (_, m) = goro_core::stubs::find_method(class, method)?;
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<(Vec<u8>, Vec<u8>), &'static [&'static str]>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let key = (class.to_vec(), method.to_ascii_lowercase());
    if let Some(v) = CACHE.with(|c| c.borrow().get(&key).copied()) {
        return Some(v);
    }
    let names: Vec<&'static str> = m.args.iter().map(|a| a.name).collect();
    let leaked: &'static [&'static str] = Box::leak(names.into_boxed_slice());
    CACHE.with(|c| c.borrow_mut().insert(key, leaked));
    Some(leaked)
}

fn ctor_this(args: &[Value]) -> Option<Obj> {
    match args.first() {
        Some(Value::Object(o)) => Some(o.clone()),
        _ => None,
    }
}

/// new SQLite3(...) (SQLite3::__construct is an alias of SQLite3::open)
fn construct_db(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let Some(this) = ctor_this(args) else {
        return Ok(());
    };
    conn::attach_db(&this);
    let f = Func::method("SQLite3", "__construct");
    run_method(
        vm,
        &f,
        "SQLite3->__construct".to_string(),
        Some(&this),
        &args[1..],
        db::open,
    );
    if vm.current_exception.is_some() {
        return Err("exception".to_string());
    }
    Ok(())
}

/// The private constructors: zend_std_get_constructor()'s check
fn private_ctor(vm: &mut Vm, class: &str) -> Result<(), String> {
    let scope = vm.current_class_scope();
    if scope
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case(class.as_bytes()))
    {
        return Ok(());
    }
    let from = match scope {
        Some(s) => format!("scope {}", String::from_utf8_lossy(&s)),
        None => "global scope".to_string(),
    };
    throw(
        vm,
        b"Error",
        &format!("Call to private {}::__construct() from {}", class, from),
    );
    Err("exception".to_string())
}

/// SQLite3Stmt::__construct
fn construct_stmt(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    private_ctor(vm, "SQLite3Stmt")?;
    let Some(this) = ctor_this(args) else {
        return Ok(());
    };
    conn::attach_stmt(&this);
    let f = Func::method("SQLite3Stmt", "__construct");
    run_method(
        vm,
        &f,
        "SQLite3Stmt->__construct".to_string(),
        Some(&this),
        &args[1..],
        stmt::construct,
    );
    if vm.current_exception.is_some() {
        return Err("exception".to_string());
    }
    Ok(())
}

/// SQLite3Result::__construct
fn construct_result(vm: &mut Vm, _args: &[Value], _line: u32) -> Result<(), String> {
    private_ctor(vm, "SQLite3Result")?;
    throw(
        vm,
        b"Exception",
        "SQLite3Result cannot be directly instantiated",
    );
    Err("exception".to_string())
}

fn create_db(_vm: &mut Vm, o: &Obj) {
    conn::attach_db(o);
}

fn create_stmt(_vm: &mut Vm, o: &Obj) {
    conn::attach_stmt(o);
}

fn create_result(_vm: &mut Vm, o: &Obj) {
    conn::attach_result(o);
}

/// The value of an SQLite3 class constant (@cvalue)
fn class_const(name: &str) -> Option<i64> {
    Some(match name {
        "OK" => SQLITE_OK,
        "DENY" => SQLITE_DENY,
        "IGNORE" => SQLITE_IGNORE,
        "CREATE_INDEX" => SQLITE_CREATE_INDEX,
        "CREATE_TABLE" => SQLITE_CREATE_TABLE,
        "CREATE_TEMP_INDEX" => SQLITE_CREATE_TEMP_INDEX,
        "CREATE_TEMP_TABLE" => SQLITE_CREATE_TEMP_TABLE,
        "CREATE_TEMP_TRIGGER" => SQLITE_CREATE_TEMP_TRIGGER,
        "CREATE_TEMP_VIEW" => SQLITE_CREATE_TEMP_VIEW,
        "CREATE_TRIGGER" => SQLITE_CREATE_TRIGGER,
        "CREATE_VIEW" => SQLITE_CREATE_VIEW,
        "DELETE" => SQLITE_DELETE,
        "DROP_INDEX" => SQLITE_DROP_INDEX,
        "DROP_TABLE" => SQLITE_DROP_TABLE,
        "DROP_TEMP_INDEX" => SQLITE_DROP_TEMP_INDEX,
        "DROP_TEMP_TABLE" => SQLITE_DROP_TEMP_TABLE,
        "DROP_TEMP_TRIGGER" => SQLITE_DROP_TEMP_TRIGGER,
        "DROP_TEMP_VIEW" => SQLITE_DROP_TEMP_VIEW,
        "DROP_TRIGGER" => SQLITE_DROP_TRIGGER,
        "DROP_VIEW" => SQLITE_DROP_VIEW,
        "INSERT" => SQLITE_INSERT,
        "PRAGMA" => SQLITE_PRAGMA,
        "READ" => SQLITE_READ,
        "SELECT" => SQLITE_SELECT,
        "TRANSACTION" => SQLITE_TRANSACTION,
        "UPDATE" => SQLITE_UPDATE,
        "ATTACH" => SQLITE_ATTACH,
        "DETACH" => SQLITE_DETACH,
        "ALTER_TABLE" => SQLITE_ALTER_TABLE,
        "REINDEX" => SQLITE_REINDEX,
        "ANALYZE" => SQLITE_ANALYZE,
        "CREATE_VTABLE" => SQLITE_CREATE_VTABLE,
        "DROP_VTABLE" => SQLITE_DROP_VTABLE,
        "FUNCTION" => SQLITE_FUNCTION,
        "SAVEPOINT" => SQLITE_SAVEPOINT,
        "COPY" => SQLITE_COPY,
        "RECURSIVE" => SQLITE_RECURSIVE,
        _ => return None,
    } as i64)
}

/// The global constants (register_sqlite3_symbols())
const CONSTANTS: &[(&str, i64)] = &[
    ("SQLITE3_ASSOC", PHP_SQLITE3_ASSOC),
    ("SQLITE3_NUM", PHP_SQLITE3_NUM),
    ("SQLITE3_BOTH", PHP_SQLITE3_BOTH),
    ("SQLITE3_INTEGER", SQLITE_INTEGER as i64),
    ("SQLITE3_FLOAT", SQLITE_FLOAT as i64),
    ("SQLITE3_TEXT", SQLITE3_TEXT as i64),
    ("SQLITE3_BLOB", SQLITE_BLOB as i64),
    ("SQLITE3_NULL", SQLITE_NULL as i64),
    ("SQLITE3_OPEN_READONLY", SQLITE_OPEN_READONLY as i64),
    ("SQLITE3_OPEN_READWRITE", SQLITE_OPEN_READWRITE as i64),
    ("SQLITE3_OPEN_CREATE", SQLITE_OPEN_CREATE as i64),
    ("SQLITE3_DETERMINISTIC", SQLITE_DETERMINISTIC as i64),
];

/// PHP_INI: (name, default, changeable by ini_set())
const INI: &[(&str, Option<&str>, bool)] = &[
    ("sqlite3.extension_dir", None, false),
    ("sqlite3.defensive", Some("1"), true),
];

fn ini_set_hook(_vm: &mut Vm, key: &[u8], _value: &Value) -> bool {
    match INI.iter().find(|(n, _, _)| n.as_bytes() == key) {
        Some((_, _, changeable)) => *changeable,
        None => true,
    }
}

/// The value of INI setting `name` (None: no value)
pub fn ini_value(vm: &Vm, name: &str) -> Option<Vec<u8>> {
    match vm.constants.get(name.as_bytes()) {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.to_php_string().as_bytes().to_vec()),
    }
}

/// zend_ini_parse_bool()
pub fn ini_bool(v: &[u8]) -> bool {
    let s = String::from_utf8_lossy(v).to_ascii_lowercase();
    match s.as_str() {
        "true" | "yes" | "on" => true,
        _ => {
            let t = s.trim_start();
            let digits: String = t
                .chars()
                .enumerate()
                .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+')))
                .map(|(_, c)| c)
                .collect();
            digits.parse::<i64>().map(|n| n != 0).unwrap_or(false)
        }
    }
}

/// PHP_MINFO(sqlite3)
fn minfo(vm: &mut Vm) -> String {
    let mut out = String::from("\nsqlite3\n\nSQLite3 support => enabled\n");
    let lib = goro_sqlite::mainapi::sqlite3_libversion().unwrap_or_default();
    out.push_str(&format!(
        "SQLite Library => {}\n",
        String::from_utf8_lossy(zpp::cstr(&lib))
    ));
    out.push_str("\nDirective => Local Value => Master Value\n");
    let mut names: Vec<&str> = INI.iter().map(|(n, _, _)| *n).collect();
    names.sort();
    for name in names {
        let raw = ini_value(vm, name);
        let shown = match (name, raw) {
            ("sqlite3.defensive", Some(r)) => {
                if ini_bool(&r) {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }
            }
            (_, Some(r)) if !r.is_empty() => String::from_utf8_lossy(&r).into_owned(),
            _ => "no value".to_string(),
        };
        out.push_str(&format!("{} => {} => {}\n", name, shown, shown));
    }
    out
}

/// PHP_MINIT(sqlite3)
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);

    // SQLite3Exception extends Exception
    let mut se = ClassEntry::new(b"SQLite3Exception".to_vec());
    se.parent = Some(b"Exception".to_vec());
    vm.classes.insert(b"sqlite3exception".to_vec(), se);

    let h = NativeClassHandler {
        has_method,
        call: call_method,
        prop_added: None,
        prop_write: None,
        compare_check: None,
        prop_read: None,
        param_names: Some(param_names),
    };
    for c in [&b"sqlite3"[..], b"sqlite3stmt", b"sqlite3result"] {
        vm.native_class_handlers.insert(c.to_vec(), h);
    }
    vm.object_create_handlers
        .insert(b"sqlite3".to_vec(), create_db);
    vm.object_create_handlers
        .insert(b"sqlite3stmt".to_vec(), create_stmt);
    vm.object_create_handlers
        .insert(b"sqlite3result".to_vec(), create_result);
    vm.object_construct_handlers
        .insert(b"sqlite3".to_vec(), construct_db);
    vm.object_construct_handlers
        .insert(b"sqlite3stmt".to_vec(), construct_stmt);
    vm.object_construct_handlers
        .insert(b"sqlite3result".to_vec(), construct_result);

    for (name, v) in CONSTANTS {
        vm.constants
            .insert(name.as_bytes().to_vec(), Value::Long(*v));
    }
    let mut list: Vec<(Vec<u8>, Value, Option<String>)> = Vec::new();
    if let Some(c) = goro_core::stubs::find_class(b"SQLite3") {
        for k in c.consts {
            if let Some(v) = class_const(k.name) {
                list.push((k.name.as_bytes().to_vec(), Value::Long(v), None));
            }
        }
    }
    vm.ext_class_constants.insert(b"sqlite3".to_vec(), list);

    for (name, def, _) in INI {
        let v = match def {
            Some(d) => Value::String(PhpString::from_bytes(d.as_bytes())),
            None => Value::Null,
        };
        vm.constants.entry(name.as_bytes().to_vec()).or_insert(v);
        vm.ini_set_hooks
            .insert(name.as_bytes().to_vec(), ini_set_hook);
    }
    vm.phpinfo_hooks.push(minfo);
}
