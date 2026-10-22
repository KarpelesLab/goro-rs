//! PHP Data Objects: php-src's ext/pdo (the PDO, PDOStatement, PDORow and
//! PDOException classes, the SQL placeholder parser) and ext/pdo_mysql (the
//! MySQL driver and Pdo\Mysql, over goro-ext-mysqli's mysqlnd port), and the
//! PostgreSQL and SQLite drivers.

#![allow(clippy::result_unit_err)]

pub mod core;
pub mod dbh;
pub mod lob;
pub mod mysql;
pub mod objects;
pub mod parser;
pub mod pgsql;
#[rustfmt::skip]
pub mod scanner;
pub mod sqlite;
pub mod sqlstate;
pub mod stmt;

use std::cell::RefCell;

use goro_core::object::{ClassEntry, PropertyDef, Visibility};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, Vm, VmError};
use goro_ext_mysqli::zpp::Func;

pub use parser::{EOI, T_BIND, T_BIND_POS, T_CUSTOM_QUOTE, T_ESCAPED_QUESTION, T_TEXT};

use crate::core::*;

type Impl = fn(&mut Vm, &Func, &[Value]) -> Value;

/// Methods: (class, method as declared, implementation, static)
const METHODS: &[(&str, &str, Impl, bool)] = &[
    ("PDO", "__construct", dbh::pdo_ctor, false),
    ("PDO", "beginTransaction", dbh::pdo_begin_transaction, false),
    ("PDO", "commit", dbh::pdo_commit, false),
    ("PDO", "errorCode", dbh::pdo_error_code, false),
    ("PDO", "errorInfo", dbh::pdo_error_info, false),
    ("PDO", "exec", dbh::pdo_exec, false),
    ("PDO", "getAttribute", dbh::pdo_get_attribute, false),
    (
        "PDO",
        "getAvailableDrivers",
        dbh::pdo_get_available_drivers,
        true,
    ),
    ("PDO", "inTransaction", dbh::pdo_in_transaction, false),
    ("PDO", "lastInsertId", dbh::pdo_last_insert_id, false),
    ("PDO", "prepare", dbh::pdo_prepare, false),
    ("PDO", "query", dbh::pdo_query, false),
    ("PDO", "quote", dbh::pdo_quote, false),
    ("PDO", "rollBack", dbh::pdo_rollback, false),
    ("PDO", "setAttribute", dbh::pdo_set_attribute, false),
    ("PDOStatement", "bindColumn", stmt::stmt_bind_column, false),
    ("PDOStatement", "bindParam", stmt::stmt_bind_param, false),
    ("PDOStatement", "bindValue", stmt::stmt_bind_value, false),
    (
        "PDOStatement",
        "closeCursor",
        stmt::stmt_close_cursor,
        false,
    ),
    (
        "PDOStatement",
        "columnCount",
        stmt::stmt_column_count,
        false,
    ),
    (
        "PDOStatement",
        "debugDumpParams",
        stmt::stmt_debug_dump_params,
        false,
    ),
    ("PDOStatement", "errorCode", stmt::stmt_error_code, false),
    ("PDOStatement", "errorInfo", stmt::stmt_error_info, false),
    ("PDOStatement", "execute", stmt::stmt_execute, false),
    ("PDOStatement", "fetch", stmt::stmt_fetch, false),
    ("PDOStatement", "fetchAll", stmt::stmt_fetch_all, false),
    (
        "PDOStatement",
        "fetchColumn",
        stmt::stmt_fetch_column,
        false,
    ),
    (
        "PDOStatement",
        "fetchObject",
        stmt::stmt_fetch_object,
        false,
    ),
    (
        "PDOStatement",
        "getAttribute",
        stmt::stmt_get_attribute,
        false,
    ),
    (
        "PDOStatement",
        "getColumnMeta",
        stmt::stmt_get_column_meta,
        false,
    ),
    ("PDOStatement", "nextRowset", stmt::stmt_next_rowset, false),
    ("PDOStatement", "rowCount", stmt::stmt_row_count, false),
    (
        "PDOStatement",
        "setAttribute",
        stmt::stmt_set_attribute,
        false,
    ),
    (
        "PDOStatement",
        "setFetchMode",
        stmt::stmt_set_fetch_mode,
        false,
    ),
    (
        "PDOStatement",
        "getIterator",
        stmt::stmt_get_iterator,
        false,
    ),
    (
        "Pdo\\Mysql",
        "getWarningCount",
        mysql::get_warning_count,
        false,
    ),
    (
        "Pdo\\Pgsql",
        "escapeIdentifier",
        pgsql::escape_identifier,
        false,
    ),
    ("Pdo\\Pgsql", "copyFromArray", pgsql::copy_from_array, false),
    ("Pdo\\Pgsql", "copyFromFile", pgsql::copy_from_file, false),
    ("Pdo\\Pgsql", "copyToArray", pgsql::copy_to_array, false),
    ("Pdo\\Pgsql", "copyToFile", pgsql::copy_to_file, false),
    ("Pdo\\Pgsql", "lobCreate", pgsql::lob_create, false),
    ("Pdo\\Pgsql", "lobOpen", pgsql::lob_open, false),
    ("Pdo\\Pgsql", "lobUnlink", pgsql::lob_unlink, false),
    ("Pdo\\Pgsql", "getNotify", pgsql::get_notify, false),
    ("Pdo\\Pgsql", "getPid", pgsql::get_pid, false),
    (
        "Pdo\\Pgsql",
        "setNoticeCallback",
        pgsql::set_notice_callback,
        false,
    ),
    (
        "Pdo\\Sqlite",
        "createAggregate",
        sqlite::create_aggregate,
        false,
    ),
    (
        "Pdo\\Sqlite",
        "createCollation",
        sqlite::create_collation,
        false,
    ),
    (
        "Pdo\\Sqlite",
        "createFunction",
        sqlite::create_function,
        false,
    ),
    (
        "Pdo\\Sqlite",
        "loadExtension",
        sqlite::load_extension,
        false,
    ),
    ("Pdo\\Sqlite", "openBlob", sqlite::open_blob, false),
    (
        "Pdo\\Sqlite",
        "setAuthorizer",
        sqlite::driver::set_authorizer,
        false,
    ),
];

/// The internal ancestors of a native class, nearest first
fn native_chain(class_lower: &[u8]) -> &'static [&'static str] {
    match class_lower {
        b"pdo\\mysql" => &["Pdo\\Mysql", "PDO"],
        b"pdo\\pgsql" => &["Pdo\\Pgsql", "PDO"],
        b"pdo\\sqlite" => &["Pdo\\Sqlite", "PDO"],
        b"pdo" => &["PDO"],
        b"pdostatement" => &["PDOStatement"],
        _ => &[],
    }
}

fn find_method(
    class_lower: &[u8],
    method_lower: &[u8],
) -> Option<&'static (&'static str, &'static str, Impl, bool)> {
    for c in native_chain(class_lower) {
        if let Some(m) = METHODS
            .iter()
            .find(|(mc, m, _, _)| mc == c && m.as_bytes().eq_ignore_ascii_case(method_lower))
        {
            return Some(m);
        }
    }
    None
}

fn is_connect(method_lower: &[u8]) -> bool {
    method_lower == b"connect"
}

fn has_method(class: &[u8], method: &[u8], is_static: bool) -> bool {
    if class == b"internaliterator" {
        return prev_iter_handler()
            .map(|h| (h.has_method)(class, method, is_static))
            .unwrap_or(false)
            || matches!(method, b"current" | b"key" | b"next" | b"valid" | b"rewind");
    }
    let m = method.to_ascii_lowercase();
    if m == b"__construct" {
        return false;
    }
    let pdo_class = matches!(
        class,
        b"pdo" | b"pdo\\mysql" | b"pdo\\pgsql" | b"pdo\\sqlite"
    );
    if is_connect(&m) && pdo_class {
        return true;
    }
    // the driver methods (dbh_method_get): resolved against the handle
    if !is_static
        && pdo_class
        && pgsql::EXT_METHODS
            .iter()
            .chain(sqlite::EXT_METHODS.iter())
            .any(|(n, _)| n.as_bytes().eq_ignore_ascii_case(&m))
    {
        return true;
    }
    match find_method(class, &m) {
        Some(e) => !is_static || e.3,
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
    frame: Option<String>,
    args: &[Value],
    imp: impl FnOnce(&mut Vm, &Func, &[Value]) -> Value,
) -> Value {
    let mut pushed = false;
    let named = frame.is_some();
    if let Some(frame) = frame {
        let has = vm
            .call_stack
            .last()
            .map(|fr| fr.0.eq_ignore_ascii_case(&frame))
            .unwrap_or(false);
        if !has {
            let line = vm.current_line;
            let file = vm.current_file.clone();
            vm.call_stack.push((frame, file, line, args.to_vec(), true));
            pushed = true;
        }
    }
    if named {
        push_active(&f.name);
    }
    let r = imp(vm, f, args);
    if named {
        pop_active();
    }
    if pushed {
        vm.call_stack.pop();
    }
    r
}

fn call_method(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    if class == b"internaliterator" {
        if let Some(o) = this_obj(this) {
            if objects::iter_of(&o).is_some() {
                return stmt::iter_call(vm, &o, &method.to_ascii_lowercase())
                    .unwrap_or(Value::Null);
            }
        }
        return match prev_iter_handler() {
            Some(h) => (h.call)(vm, class, method, called, this, args),
            None => Value::Null,
        };
    }
    let ml = method.to_ascii_lowercase();
    if is_connect(&ml) {
        let f = Func::method("PDO", "connect", None);
        let called = called.to_vec();
        return run_method(
            vm,
            &f,
            Some("PDO::connect".to_string()),
            args,
            |vm, f, a| dbh::pdo_connect(vm, f, a, &called),
        );
    }
    if pgsql::EXT_METHODS
        .iter()
        .chain(sqlite::EXT_METHODS.iter())
        .any(|(n, _)| n.as_bytes().eq_ignore_ascii_case(&ml))
        && find_method(class, &ml).is_none()
    {
        let this_o = this_obj(this);
        if let Some(r) = pgsql::call_ext_method(vm, &ml, this_o.as_ref(), args) {
            return r;
        }
        if let Some(r) = sqlite::call_ext_method(vm, &ml, this_o.as_ref(), args) {
            return r;
        }
        let cname = match &this_o {
            Some(o) => goro_core::value::display_class_name(&o.borrow().class_name),
            None => "PDO".to_string(),
        };
        let name = pgsql::EXT_METHODS
            .iter()
            .chain(sqlite::EXT_METHODS.iter())
            .find(|(n, _)| n.as_bytes().eq_ignore_ascii_case(&ml))
            .map(|(n, _)| *n)
            .unwrap_or("");
        let msg = format!("Call to undefined method {}::{}()", cname, name);
        throw(vm, b"Error", &msg);
        return Value::Null;
    }
    let m = match find_method(class, &ml) {
        Some(m) => m,
        None => return Value::Null,
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
    let f = Func::method(m.0, m.1, if m.3 { None } else { this_o.clone() });
    // foreach reaches getIterator() for the statement's get_iterator
    // handler, which is not a call (no frame).
    let frame = if ml == b"getiterator" {
        None
    } else if this_o.is_some() {
        Some(format!("{}->{}", m.0, m.1))
    } else {
        Some(format!("{}::{}", m.0, m.1))
    };
    run_method(vm, &f, frame, args, m.2)
}

fn param_names(class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    let (_, m) = goro_core::stubs::find_method(class, method)
        .or_else(|| goro_core::stubs::find_method(b"pdo", method))?;
    thread_local! {
        static CACHE: RefCell<std::collections::HashMap<(Vec<u8>, Vec<u8>), &'static [&'static str]>> =
            RefCell::new(std::collections::HashMap::new());
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

thread_local! {
    static PREV_ITER: RefCell<Option<NativeClassHandler>> = const { RefCell::new(None) };
}

fn prev_iter_handler() -> Option<NativeClassHandler> {
    PREV_ITER.with(|p| *p.borrow())
}

/// new PDO(...) / new Pdo\Mysql(...) and subclasses
fn construct_pdo(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    objects::attach_dbh(&this);
    let f = Func::method("PDO", "__construct", Some(this));
    run_method(
        vm,
        &f,
        Some("PDO->__construct".to_string()),
        &args[1..],
        dbh::pdo_ctor,
    );
    if vm.current_exception.is_some() {
        return Err("exception".to_string());
    }
    Ok(())
}

/// row_get_ctor
fn construct_row(vm: &mut Vm, _args: &[Value], _line: u32) -> Result<(), String> {
    throw_pdo(vm, "You may not create a PDORow manually");
    Err("exception".to_string())
}

fn create_dbh(_vm: &mut Vm, o: &Obj) {
    objects::attach_dbh(o);
}

fn create_stmt(_vm: &mut Vm, o: &Obj) {
    objects::attach_stmt(o);
}

/// The value of a PDO class constant
fn pdo_const(name: &str) -> Option<Value> {
    let s = |v: &str| Value::String(PhpString::from_bytes(v.as_bytes()));
    Some(Value::Long(match name {
        "PARAM_NULL" => PDO_PARAM_NULL,
        "PARAM_BOOL" => PDO_PARAM_BOOL,
        "PARAM_INT" => PDO_PARAM_INT,
        "PARAM_STR" => PDO_PARAM_STR,
        "PARAM_LOB" => PDO_PARAM_LOB,
        "PARAM_STMT" => PDO_PARAM_STMT,
        "PARAM_INPUT_OUTPUT" => PDO_PARAM_INPUT_OUTPUT,
        "PARAM_STR_NATL" => PDO_PARAM_STR_NATL,
        "PARAM_STR_CHAR" => PDO_PARAM_STR_CHAR,
        "PARAM_EVT_ALLOC" => PDO_PARAM_EVT_ALLOC as i64,
        "PARAM_EVT_FREE" => PDO_PARAM_EVT_FREE as i64,
        "PARAM_EVT_EXEC_PRE" => PDO_PARAM_EVT_EXEC_PRE as i64,
        "PARAM_EVT_EXEC_POST" => PDO_PARAM_EVT_EXEC_POST as i64,
        "PARAM_EVT_FETCH_PRE" => PDO_PARAM_EVT_FETCH_PRE as i64,
        "PARAM_EVT_FETCH_POST" => PDO_PARAM_EVT_FETCH_POST as i64,
        "PARAM_EVT_NORMALIZE" => PDO_PARAM_EVT_NORMALIZE as i64,
        "FETCH_DEFAULT" => PDO_FETCH_USE_DEFAULT,
        "FETCH_LAZY" => PDO_FETCH_LAZY,
        "FETCH_ASSOC" => PDO_FETCH_ASSOC,
        "FETCH_NUM" => PDO_FETCH_NUM,
        "FETCH_BOTH" => PDO_FETCH_BOTH,
        "FETCH_OBJ" => PDO_FETCH_OBJ,
        "FETCH_BOUND" => PDO_FETCH_BOUND,
        "FETCH_COLUMN" => PDO_FETCH_COLUMN,
        "FETCH_CLASS" => PDO_FETCH_CLASS,
        "FETCH_INTO" => PDO_FETCH_INTO,
        "FETCH_FUNC" => PDO_FETCH_FUNC,
        "FETCH_GROUP" => PDO_FETCH_GROUP,
        "FETCH_UNIQUE" => PDO_FETCH_UNIQUE,
        "FETCH_KEY_PAIR" => PDO_FETCH_KEY_PAIR,
        "FETCH_CLASSTYPE" => PDO_FETCH_CLASSTYPE,
        "FETCH_SERIALIZE" => PDO_FETCH_SERIALIZE,
        "FETCH_PROPS_LATE" => PDO_FETCH_PROPS_LATE,
        "FETCH_NAMED" => PDO_FETCH_NAMED,
        "ATTR_AUTOCOMMIT" => PDO_ATTR_AUTOCOMMIT,
        "ATTR_PREFETCH" => PDO_ATTR_PREFETCH,
        "ATTR_TIMEOUT" => PDO_ATTR_TIMEOUT,
        "ATTR_ERRMODE" => PDO_ATTR_ERRMODE,
        "ATTR_SERVER_VERSION" => PDO_ATTR_SERVER_VERSION,
        "ATTR_CLIENT_VERSION" => PDO_ATTR_CLIENT_VERSION,
        "ATTR_SERVER_INFO" => PDO_ATTR_SERVER_INFO,
        "ATTR_CONNECTION_STATUS" => PDO_ATTR_CONNECTION_STATUS,
        "ATTR_CASE" => PDO_ATTR_CASE,
        "ATTR_CURSOR_NAME" => PDO_ATTR_CURSOR_NAME,
        "ATTR_CURSOR" => PDO_ATTR_CURSOR,
        "ATTR_ORACLE_NULLS" => PDO_ATTR_ORACLE_NULLS,
        "ATTR_PERSISTENT" => PDO_ATTR_PERSISTENT,
        "ATTR_STATEMENT_CLASS" => PDO_ATTR_STATEMENT_CLASS,
        "ATTR_FETCH_TABLE_NAMES" => PDO_ATTR_FETCH_TABLE_NAMES,
        "ATTR_FETCH_CATALOG_NAMES" => PDO_ATTR_FETCH_CATALOG_NAMES,
        "ATTR_DRIVER_NAME" => PDO_ATTR_DRIVER_NAME,
        "ATTR_STRINGIFY_FETCHES" => PDO_ATTR_STRINGIFY_FETCHES,
        "ATTR_MAX_COLUMN_LEN" => PDO_ATTR_MAX_COLUMN_LEN,
        "ATTR_EMULATE_PREPARES" => PDO_ATTR_EMULATE_PREPARES,
        "ATTR_DEFAULT_FETCH_MODE" => PDO_ATTR_DEFAULT_FETCH_MODE,
        "ATTR_DEFAULT_STR_PARAM" => PDO_ATTR_DEFAULT_STR_PARAM,
        "ERRMODE_SILENT" => PDO_ERRMODE_SILENT as i64,
        "ERRMODE_WARNING" => PDO_ERRMODE_WARNING as i64,
        "ERRMODE_EXCEPTION" => PDO_ERRMODE_EXCEPTION as i64,
        "CASE_NATURAL" => PDO_CASE_NATURAL as i64,
        "CASE_LOWER" => PDO_CASE_LOWER as i64,
        "CASE_UPPER" => PDO_CASE_UPPER as i64,
        "NULL_NATURAL" => PDO_NULL_NATURAL as i64,
        "NULL_EMPTY_STRING" => PDO_NULL_EMPTY_STRING as i64,
        "NULL_TO_STRING" => PDO_NULL_TO_STRING as i64,
        "ERR_NONE" => return Some(s("00000")),
        "FETCH_ORI_NEXT" => PDO_FETCH_ORI_NEXT,
        "FETCH_ORI_PRIOR" => PDO_FETCH_ORI_PRIOR,
        "FETCH_ORI_FIRST" => PDO_FETCH_ORI_FIRST,
        "FETCH_ORI_LAST" => PDO_FETCH_ORI_LAST,
        "FETCH_ORI_ABS" => PDO_FETCH_ORI_ABS,
        "FETCH_ORI_REL" => PDO_FETCH_ORI_REL,
        "CURSOR_FWDONLY" => PDO_CURSOR_FWDONLY,
        "CURSOR_SCROLL" => PDO_CURSOR_SCROLL,
        _ => return None,
    }))
}

fn register_constants(vm: &mut Vm) {
    // PDO, in stub order, then the deprecated driver aliases
    let mut list: Vec<(Vec<u8>, Value, Option<String>)> = Vec::new();
    if let Some(c) = goro_core::stubs::find_class(b"PDO") {
        for k in c.consts {
            if let Some(v) = pdo_const(k.name) {
                list.push((k.name.as_bytes().to_vec(), v, None));
            }
        }
    }
    for (name, v) in mysql::CONSTANTS {
        let old = format!("MYSQL_{}", name);
        let msg = format!(
            "Constant PDO::{} is deprecated since 8.5, use Pdo\\Mysql::{} instead",
            old, name
        );
        list.push((old.into_bytes(), Value::Long(*v), Some(msg)));
    }
    list.extend(pgsql::pdo_constants());
    list.extend(sqlite::pdo_constants());
    vm.ext_class_constants.insert(b"pdo".to_vec(), list);
    vm.ext_class_constants
        .insert(b"pdo\\pgsql".to_vec(), pgsql::class_constants());
    vm.ext_class_constants
        .insert(b"pdo\\sqlite".to_vec(), sqlite::class_constants());
    let my: Vec<(Vec<u8>, Value, Option<String>)> = mysql::CONSTANTS
        .iter()
        .map(|(n, v)| (n.as_bytes().to_vec(), Value::Long(*v), None))
        .collect();
    vm.ext_class_constants.insert(b"pdo\\mysql".to_vec(), my);
}

fn prop(name: &str, vis: Visibility, default: Value, class: &str) -> PropertyDef {
    PropertyDef {
        name: name.as_bytes().to_vec(),
        default,
        is_static: false,
        is_readonly: false,
        visibility: vis,
        set_visibility: None,
        declaring_class: class.as_bytes().to_vec(),
        property_type: None,
        has_get_hook: false,
        has_set_hook: false,
        is_virtual: false,
        is_abstract: false,
        is_final: false,
        doc_comment: None,
        attributes: Vec::new(),
    }
}

/// pdo_drivers()
fn w_pdo_drivers(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = Func::function("pdo_drivers");
    if goro_ext_mysqli::zpp::parse(vm, &f, "", &[], args).is_err() {
        return Ok(Value::Null);
    }
    Ok(dbh::available_drivers())
}

/// PHP_MINFO(pdo)
fn minfo(_vm: &mut Vm) -> String {
    let names: Vec<&str> = drivers().iter().map(|d| d.name).collect();
    format!(
        "\nPDO\n\nPDO support => enabled\nPDO drivers => {}\n",
        names.join(", ")
    )
}

fn ini_readonly(_vm: &mut Vm, _key: &[u8], _value: &Value) -> bool {
    false
}

/// PHP_MINIT(pdo) + PHP_MINIT(pdo_mysql)
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"pdo");
    vm.register_function_with_params(b"pdo_drivers", w_pdo_drivers, &[]);
    vm.phpinfo_hooks.push(minfo);
    vm.shutdown_hooks.push(dbh::free_persistent);
    goro_core::gc::register_native_gc(objects::stmt_handlers(), objects::stmt_get_gc);

    let mut pe = ClassEntry::new(b"PDOException".to_vec());
    pe.parent = Some(b"RuntimeException".to_vec());
    pe.properties.push(prop(
        "code",
        Visibility::Protected,
        Value::Long(0),
        "PDOException",
    ));
    pe.properties.push(prop(
        "errorInfo",
        Visibility::Public,
        Value::Null,
        "PDOException",
    ));
    vm.classes.insert(b"pdoexception".to_vec(), pe);

    let h = NativeClassHandler {
        has_method,
        call: call_method,
        prop_added: None,
        prop_write: None,
        compare_check: None,
        prop_read: None,
        param_names: Some(param_names),
    };
    for c in [
        &b"pdo"[..],
        b"pdostatement",
        b"pdorow",
        b"pdo\\mysql",
        b"pdo\\pgsql",
        b"pdo\\sqlite",
    ] {
        vm.native_class_handlers.insert(c.to_vec(), h);
    }
    let prev = vm
        .native_class_handlers
        .get(b"internaliterator".as_slice())
        .copied();
    PREV_ITER.with(|p| *p.borrow_mut() = prev);
    let ih = NativeClassHandler {
        has_method,
        call: call_method,
        prop_added: prev.and_then(|p| p.prop_added),
        prop_write: prev.and_then(|p| p.prop_write),
        compare_check: prev.and_then(|p| p.compare_check),
        prop_read: prev.and_then(|p| p.prop_read),
        param_names: prev.and_then(|p| p.param_names),
    };
    vm.native_class_handlers
        .insert(b"internaliterator".to_vec(), ih);

    vm.object_create_handlers
        .insert(b"pdo".to_vec(), create_dbh);
    vm.object_create_handlers
        .insert(b"pdo\\mysql".to_vec(), create_dbh);
    vm.object_create_handlers
        .insert(b"pdo\\pgsql".to_vec(), create_dbh);
    vm.object_create_handlers
        .insert(b"pdo\\sqlite".to_vec(), create_dbh);
    vm.object_create_handlers
        .insert(b"pdostatement".to_vec(), create_stmt);
    vm.object_construct_handlers
        .insert(b"pdo".to_vec(), construct_pdo);
    vm.object_construct_handlers
        .insert(b"pdo\\mysql".to_vec(), construct_pdo);
    vm.object_construct_handlers
        .insert(b"pdo\\pgsql".to_vec(), construct_pdo);
    vm.object_construct_handlers
        .insert(b"pdo\\sqlite".to_vec(), construct_pdo);
    vm.object_construct_handlers
        .insert(b"pdorow".to_vec(), construct_row);

    mysql::register(vm);
    pgsql::register(vm);
    sqlite::register(vm);
    vm.ini_set_hooks
        .insert(b"pdo_mysql.default_socket".to_vec(), ini_readonly);
    register_constants(vm);
}
