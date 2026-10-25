//! PHP's mysqli extension (php-src ext/mysqli) on top of a port of mysqlnd
//! (ext/mysqlnd), talking the MySQL client/server protocol directly.

#![allow(clippy::result_unit_err)]

pub mod link;
pub mod mysqlnd;
pub mod objects;
pub mod res;
pub mod stmt;
pub mod zpp;

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::object::ClassEntry;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, Vm, VmError};

use zpp::Func;

type Impl = fn(&mut Vm, &Func, &[Value]) -> Value;

/// The procedural functions: (name, implementation)
const FUNCTIONS: &[(&str, Impl)] = &[
    ("mysqli_affected_rows", link::mysqli_affected_rows),
    ("mysqli_autocommit", link::mysqli_autocommit),
    ("mysqli_begin_transaction", link::mysqli_begin_transaction),
    ("mysqli_change_user", link::mysqli_change_user),
    ("mysqli_character_set_name", link::mysqli_character_set_name),
    ("mysqli_close", link::mysqli_close),
    ("mysqli_commit", link::mysqli_commit),
    ("mysqli_connect", link::mysqli_connect),
    ("mysqli_connect_errno", link::mysqli_connect_errno),
    ("mysqli_connect_error", link::mysqli_connect_error),
    ("mysqli_data_seek", res::mysqli_data_seek),
    ("mysqli_dump_debug_info", link::mysqli_dump_debug_info),
    ("mysqli_debug", link::mysqli_debug),
    ("mysqli_errno", link::mysqli_errno),
    ("mysqli_error", link::mysqli_error),
    ("mysqli_error_list", link::mysqli_error_list),
    ("mysqli_stmt_execute", stmt::mysqli_stmt_execute),
    ("mysqli_execute", stmt::mysqli_stmt_execute),
    ("mysqli_execute_query", stmt::mysqli_execute_query),
    ("mysqli_fetch_field", res::mysqli_fetch_field),
    ("mysqli_fetch_fields", res::mysqli_fetch_fields),
    ("mysqli_fetch_field_direct", res::mysqli_fetch_field_direct),
    ("mysqli_fetch_lengths", res::mysqli_fetch_lengths),
    ("mysqli_fetch_all", res::mysqli_fetch_all),
    ("mysqli_fetch_array", res::mysqli_fetch_array),
    ("mysqli_fetch_assoc", res::mysqli_fetch_assoc),
    ("mysqli_fetch_object", res::mysqli_fetch_object),
    ("mysqli_fetch_row", res::mysqli_fetch_row),
    ("mysqli_fetch_column", res::mysqli_fetch_column),
    ("mysqli_field_count", link::mysqli_field_count),
    ("mysqli_field_seek", res::mysqli_field_seek),
    ("mysqli_field_tell", res::mysqli_field_tell),
    ("mysqli_free_result", res::mysqli_free_result),
    (
        "mysqli_get_connection_stats",
        link::mysqli_get_connection_stats,
    ),
    ("mysqli_get_client_stats", link::mysqli_get_client_stats),
    ("mysqli_get_charset", link::mysqli_get_charset),
    ("mysqli_get_client_info", link::mysqli_get_client_info),
    ("mysqli_get_client_version", link::mysqli_get_client_version),
    ("mysqli_get_links_stats", link::mysqli_get_links_stats),
    ("mysqli_get_host_info", link::mysqli_get_host_info),
    ("mysqli_get_proto_info", link::mysqli_get_proto_info),
    ("mysqli_get_server_info", link::mysqli_get_server_info),
    ("mysqli_get_server_version", link::mysqli_get_server_version),
    ("mysqli_get_warnings", link::mysqli_get_warnings),
    ("mysqli_init", link::mysqli_init),
    ("mysqli_info", link::mysqli_info),
    ("mysqli_insert_id", link::mysqli_insert_id),
    ("mysqli_kill", link::mysqli_kill),
    ("mysqli_more_results", link::mysqli_more_results),
    ("mysqli_multi_query", link::mysqli_multi_query),
    ("mysqli_next_result", link::mysqli_next_result),
    ("mysqli_num_fields", res::mysqli_num_fields),
    ("mysqli_num_rows", res::mysqli_num_rows),
    ("mysqli_options", link::mysqli_options),
    ("mysqli_set_opt", link::mysqli_options),
    ("mysqli_ping", link::mysqli_ping),
    ("mysqli_poll", link::mysqli_poll),
    ("mysqli_prepare", stmt::mysqli_prepare),
    ("mysqli_report", link::mysqli_report),
    ("mysqli_query", link::mysqli_query),
    ("mysqli_real_connect", link::mysqli_real_connect),
    ("mysqli_real_escape_string", link::mysqli_real_escape_string),
    ("mysqli_escape_string", link::mysqli_real_escape_string),
    ("mysqli_real_query", link::mysqli_real_query),
    ("mysqli_reap_async_query", link::mysqli_reap_async_query),
    ("mysqli_release_savepoint", link::mysqli_release_savepoint),
    ("mysqli_rollback", link::mysqli_rollback),
    ("mysqli_savepoint", link::mysqli_savepoint),
    ("mysqli_select_db", link::mysqli_select_db),
    ("mysqli_set_charset", link::mysqli_set_charset),
    ("mysqli_stmt_affected_rows", stmt::mysqli_stmt_affected_rows),
    ("mysqli_stmt_attr_get", stmt::mysqli_stmt_attr_get),
    ("mysqli_stmt_attr_set", stmt::mysqli_stmt_attr_set),
    ("mysqli_stmt_bind_param", stmt::mysqli_stmt_bind_param),
    ("mysqli_stmt_bind_result", stmt::mysqli_stmt_bind_result),
    ("mysqli_stmt_close", stmt::mysqli_stmt_close),
    ("mysqli_stmt_data_seek", stmt::mysqli_stmt_data_seek),
    ("mysqli_stmt_errno", stmt::mysqli_stmt_errno),
    ("mysqli_stmt_error", stmt::mysqli_stmt_error),
    ("mysqli_stmt_error_list", stmt::mysqli_stmt_error_list),
    ("mysqli_stmt_fetch", stmt::mysqli_stmt_fetch),
    ("mysqli_stmt_field_count", stmt::mysqli_stmt_field_count),
    ("mysqli_stmt_free_result", stmt::mysqli_stmt_free_result),
    ("mysqli_stmt_get_result", stmt::mysqli_stmt_get_result),
    ("mysqli_stmt_get_warnings", stmt::mysqli_stmt_get_warnings),
    ("mysqli_stmt_init", stmt::mysqli_stmt_init),
    ("mysqli_stmt_insert_id", stmt::mysqli_stmt_insert_id),
    ("mysqli_stmt_more_results", stmt::mysqli_stmt_more_results),
    ("mysqli_stmt_next_result", stmt::mysqli_stmt_next_result),
    ("mysqli_stmt_num_rows", stmt::mysqli_stmt_num_rows),
    ("mysqli_stmt_param_count", stmt::mysqli_stmt_param_count),
    ("mysqli_stmt_prepare", stmt::mysqli_stmt_prepare),
    ("mysqli_stmt_reset", stmt::mysqli_stmt_reset),
    (
        "mysqli_stmt_result_metadata",
        stmt::mysqli_stmt_result_metadata,
    ),
    (
        "mysqli_stmt_send_long_data",
        stmt::mysqli_stmt_send_long_data,
    ),
    ("mysqli_stmt_store_result", stmt::mysqli_stmt_store_result),
    ("mysqli_stmt_sqlstate", stmt::mysqli_stmt_sqlstate),
    ("mysqli_sqlstate", link::mysqli_sqlstate),
    ("mysqli_ssl_set", link::mysqli_ssl_set),
    ("mysqli_stat", link::mysqli_stat),
    ("mysqli_store_result", link::mysqli_store_result),
    ("mysqli_thread_id", link::mysqli_thread_id),
    ("mysqli_thread_safe", link::mysqli_thread_safe),
    ("mysqli_use_result", link::mysqli_use_result),
    ("mysqli_warning_count", link::mysqli_warning_count),
    ("mysqli_refresh", link::mysqli_refresh),
];

/// Methods: (class, method as declared, implementation, static)
const METHODS: &[(&str, &str, Impl, bool)] = &[
    ("mysqli", "__construct", link::mysqli_ctor, false),
    ("mysqli", "autocommit", link::mysqli_autocommit, false),
    (
        "mysqli",
        "begin_transaction",
        link::mysqli_begin_transaction,
        false,
    ),
    ("mysqli", "change_user", link::mysqli_change_user, false),
    (
        "mysqli",
        "character_set_name",
        link::mysqli_character_set_name,
        false,
    ),
    ("mysqli", "close", link::mysqli_close, false),
    ("mysqli", "commit", link::mysqli_commit, false),
    ("mysqli", "connect", link::mysqli_connect, false),
    (
        "mysqli",
        "dump_debug_info",
        link::mysqli_dump_debug_info,
        false,
    ),
    ("mysqli", "debug", link::mysqli_debug, false),
    ("mysqli", "get_charset", link::mysqli_get_charset, false),
    ("mysqli", "execute_query", stmt::mysqli_execute_query, false),
    (
        "mysqli",
        "get_client_info",
        link::mysqli_get_client_info,
        false,
    ),
    (
        "mysqli",
        "get_connection_stats",
        link::mysqli_get_connection_stats,
        false,
    ),
    (
        "mysqli",
        "get_server_info",
        link::mysqli_get_server_info,
        false,
    ),
    ("mysqli", "get_warnings", link::mysqli_get_warnings, false),
    ("mysqli", "init", link::mysqli_init, false),
    ("mysqli", "kill", link::mysqli_kill, false),
    ("mysqli", "multi_query", link::mysqli_multi_query, false),
    ("mysqli", "more_results", link::mysqli_more_results, false),
    ("mysqli", "next_result", link::mysqli_next_result, false),
    ("mysqli", "ping", link::mysqli_ping, false),
    ("mysqli", "poll", link::mysqli_poll, true),
    ("mysqli", "prepare", stmt::mysqli_prepare, false),
    ("mysqli", "query", link::mysqli_query, false),
    ("mysqli", "real_connect", link::mysqli_real_connect, false),
    (
        "mysqli",
        "real_escape_string",
        link::mysqli_real_escape_string,
        false,
    ),
    (
        "mysqli",
        "reap_async_query",
        link::mysqli_reap_async_query,
        false,
    ),
    (
        "mysqli",
        "escape_string",
        link::mysqli_real_escape_string,
        false,
    ),
    ("mysqli", "real_query", link::mysqli_real_query, false),
    (
        "mysqli",
        "release_savepoint",
        link::mysqli_release_savepoint,
        false,
    ),
    ("mysqli", "rollback", link::mysqli_rollback, false),
    ("mysqli", "savepoint", link::mysqli_savepoint, false),
    ("mysqli", "select_db", link::mysqli_select_db, false),
    ("mysqli", "set_charset", link::mysqli_set_charset, false),
    ("mysqli", "options", link::mysqli_options, false),
    ("mysqli", "set_opt", link::mysqli_options, false),
    ("mysqli", "ssl_set", link::mysqli_ssl_set, false),
    ("mysqli", "stat", link::mysqli_stat, false),
    ("mysqli", "stmt_init", stmt::mysqli_stmt_init, false),
    ("mysqli", "store_result", link::mysqli_store_result, false),
    ("mysqli", "thread_safe", link::mysqli_thread_safe, false),
    ("mysqli", "use_result", link::mysqli_use_result, false),
    ("mysqli", "refresh", link::mysqli_refresh, false),
    (
        "mysqli_result",
        "__construct",
        res::mysqli_result_ctor,
        false,
    ),
    ("mysqli_result", "close", res::mysqli_free_result, false),
    ("mysqli_result", "free", res::mysqli_free_result, false),
    ("mysqli_result", "data_seek", res::mysqli_data_seek, false),
    (
        "mysqli_result",
        "fetch_field",
        res::mysqli_fetch_field,
        false,
    ),
    (
        "mysqli_result",
        "fetch_fields",
        res::mysqli_fetch_fields,
        false,
    ),
    (
        "mysqli_result",
        "fetch_field_direct",
        res::mysqli_fetch_field_direct,
        false,
    ),
    ("mysqli_result", "fetch_all", res::mysqli_fetch_all, false),
    (
        "mysqli_result",
        "fetch_array",
        res::mysqli_fetch_array,
        false,
    ),
    (
        "mysqli_result",
        "fetch_assoc",
        res::mysqli_fetch_assoc,
        false,
    ),
    (
        "mysqli_result",
        "fetch_object",
        res::mysqli_fetch_object,
        false,
    ),
    ("mysqli_result", "fetch_row", res::mysqli_fetch_row, false),
    (
        "mysqli_result",
        "fetch_column",
        res::mysqli_fetch_column,
        false,
    ),
    ("mysqli_result", "field_seek", res::mysqli_field_seek, false),
    (
        "mysqli_result",
        "free_result",
        res::mysqli_free_result,
        false,
    ),
    (
        "mysqli_result",
        "getIterator",
        res::mysqli_result_get_iterator,
        false,
    ),
    ("mysqli_stmt", "__construct", stmt::mysqli_stmt_ctor, false),
    ("mysqli_stmt", "attr_get", stmt::mysqli_stmt_attr_get, false),
    ("mysqli_stmt", "attr_set", stmt::mysqli_stmt_attr_set, false),
    (
        "mysqli_stmt",
        "bind_param",
        stmt::mysqli_stmt_bind_param,
        false,
    ),
    (
        "mysqli_stmt",
        "bind_result",
        stmt::mysqli_stmt_bind_result,
        false,
    ),
    ("mysqli_stmt", "close", stmt::mysqli_stmt_close, false),
    (
        "mysqli_stmt",
        "data_seek",
        stmt::mysqli_stmt_data_seek,
        false,
    ),
    ("mysqli_stmt", "execute", stmt::mysqli_stmt_execute, false),
    ("mysqli_stmt", "fetch", stmt::mysqli_stmt_fetch, false),
    (
        "mysqli_stmt",
        "get_warnings",
        stmt::mysqli_stmt_get_warnings,
        false,
    ),
    (
        "mysqli_stmt",
        "result_metadata",
        stmt::mysqli_stmt_result_metadata,
        false,
    ),
    (
        "mysqli_stmt",
        "more_results",
        stmt::mysqli_stmt_more_results,
        false,
    ),
    (
        "mysqli_stmt",
        "next_result",
        stmt::mysqli_stmt_next_result,
        false,
    ),
    ("mysqli_stmt", "num_rows", stmt::mysqli_stmt_num_rows, false),
    (
        "mysqli_stmt",
        "send_long_data",
        stmt::mysqli_stmt_send_long_data,
        false,
    ),
    (
        "mysqli_stmt",
        "free_result",
        stmt::mysqli_stmt_free_result,
        false,
    ),
    ("mysqli_stmt", "reset", stmt::mysqli_stmt_reset, false),
    ("mysqli_stmt", "prepare", stmt::mysqli_stmt_prepare, false),
    (
        "mysqli_stmt",
        "store_result",
        stmt::mysqli_stmt_store_result,
        false,
    ),
    (
        "mysqli_stmt",
        "get_result",
        stmt::mysqli_stmt_get_result,
        false,
    ),
    (
        "mysqli_warning",
        "__construct",
        stmt::mysqli_warning_ctor,
        false,
    ),
    ("mysqli_warning", "next", stmt::mysqli_warning_next, false),
    (
        "mysqli_sql_exception",
        "getSqlState",
        sql_exception_get_sql_state,
        false,
    ),
];

/// The #[\Deprecated] functions and methods: "since, message"
fn deprecation(name: &str) -> Option<&'static str> {
    Some(match name {
        "mysqli_execute" => "8.5, use mysqli_stmt_execute() instead",
        "mysqli_kill" | "mysqli::kill" => "8.4, use KILL CONNECTION/QUERY SQL statement instead",
        "mysqli_ping" => {
            "8.4, because the reconnect feature has been removed in PHP 8.2 and this function is now redundant"
        }
        "mysqli::ping" => {
            "8.4, because the reconnect feature has been removed in PHP 8.2 and this method is now redundant"
        }
        "mysqli_refresh" | "mysqli::refresh" => "8.4, use FLUSH SQL statement instead",
        "mysqli::get_client_info" => "8.1, use mysqli_get_client_info() instead",
        "mysqli::init" => "8.1, replace calls to parent::init() with parent::__construct()",
        _ => return None,
    })
}

fn sql_exception_get_sql_state(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if zpp::parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    match &f.this {
        Some(o) => {
            let v = o.borrow().get_property(b"sqlstate");
            Value::String(vm.value_to_string(&v))
        }
        None => Value::Null,
    }
}

fn find_method(
    class_lower: &[u8],
    method_lower: &[u8],
) -> Option<&'static (&'static str, &'static str, Impl, bool)> {
    METHODS.iter().find(|(c, m, _, _)| {
        c.as_bytes() == class_lower && m.as_bytes().eq_ignore_ascii_case(method_lower)
    })
}

fn has_method(class: &[u8], method: &[u8], _is_static: bool) -> bool {
    if class == b"internaliterator" {
        return prev_iter_handler()
            .map(|h| (h.has_method)(class, method, _is_static))
            .unwrap_or(false)
            || matches!(method, b"current" | b"key" | b"next" | b"valid" | b"rewind");
    }
    // constructors are run through object_construct_handlers
    let m = method.to_ascii_lowercase();
    if m == b"__construct" {
        return false;
    }
    match find_method(class, &m) {
        // a static call only finds static methods
        Some(e) => !_is_static || e.3,
        None => false,
    }
}

fn this_obj(this: Option<&Value>) -> Option<zpp::Obj> {
    match this {
        Some(Value::Object(o)) => Some(o.clone()),
        Some(Value::Reference(r)) => match &*r.borrow() {
            Value::Object(o) => Some(o.clone()),
            _ => None,
        },
        _ => None,
    }
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
            if let Some(it) = res::iter_of(&o) {
                return res::iter_call(vm, &it, &method.to_ascii_lowercase());
            }
        }
        return match prev_iter_handler() {
            Some(h) => (h.call)(vm, class, method, called, this, args),
            None => Value::Null,
        };
    }
    let m = match find_method(class, &method.to_ascii_lowercase()) {
        Some(m) => m,
        None => return Value::Null,
    };
    let this_o = this_obj(this);
    if !m.3 && this_o.is_none() {
        let msg = format!(
            "Non-static method {}::{}() cannot be called statically",
            m.0, m.1
        );
        zpp::throw(vm, b"Error", &msg);
        return Value::Null;
    }
    let f = Func::method(m.0, m.1, if m.3 { None } else { this_o.clone() });
    vm.flush_cli_output();
    if let Some(d) = deprecation(&f.name) {
        vm.emit_deprecated(&format!("Method {}() is deprecated since {}", f.name, d));
        if vm.current_exception.is_some() {
            return Value::Null;
        }
    }
    // When invoked from the engine there may be no call frame yet.
    let mut pushed = false;
    if let Some(_o) = &this_o {
        let frame = format!("{}->{}", m.0, m.1);
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
    let r = (m.2)(vm, &f, args);
    if pushed {
        vm.call_stack.pop();
    }
    r
}

fn param_names(class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    let (_, m) = goro_core::stubs::find_method(class, method)?;
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

fn construct_mysqli(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    construct_generic(vm, args, "mysqli", link::mysqli_ctor)
}

fn construct_result(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    construct_generic(vm, args, "mysqli_result", res::mysqli_result_ctor)
}

fn construct_stmt(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    construct_generic(vm, args, "mysqli_stmt", stmt::mysqli_stmt_ctor)
}

fn construct_generic(vm: &mut Vm, args: &[Value], class: &str, imp: Impl) -> Result<(), String> {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    objects::create_object(vm, &this);
    vm.flush_cli_output();
    let f = Func::method(class, "__construct", Some(this));
    let line = vm.current_line;
    let file = vm.current_file.clone();
    vm.call_stack.push((
        format!("{}->__construct", class),
        file,
        line,
        args[1..].to_vec(),
        true,
    ));
    imp(vm, &f, &args[1..]);
    vm.call_stack.pop();
    if vm.current_exception.is_some() {
        return Err("exception".to_string());
    }
    Ok(())
}

macro_rules! wrap {
    ($($name:ident => $imp:path),* $(,)?) => {
        $(
            fn $name(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
                let fname = &stringify!($name)[2..];
                let f = Func::function(fname);
                // (the CLI writes output unbuffered: it is out before the
                // call blocks on the server)
                vm.flush_cli_output();
                if let Some(d) = deprecation(fname) {
                    vm.emit_function_deprecation(&format!("Function {}() is deprecated since {}", fname, d))?;
                }
                Ok($imp(vm, &f, args))
            }
        )*
    };
}

wrap! {
    w_mysqli_affected_rows => link::mysqli_affected_rows,
    w_mysqli_autocommit => link::mysqli_autocommit,
    w_mysqli_begin_transaction => link::mysqli_begin_transaction,
    w_mysqli_change_user => link::mysqli_change_user,
    w_mysqli_character_set_name => link::mysqli_character_set_name,
    w_mysqli_close => link::mysqli_close,
    w_mysqli_commit => link::mysqli_commit,
    w_mysqli_connect => link::mysqli_connect,
    w_mysqli_connect_errno => link::mysqli_connect_errno,
    w_mysqli_connect_error => link::mysqli_connect_error,
    w_mysqli_data_seek => res::mysqli_data_seek,
    w_mysqli_dump_debug_info => link::mysqli_dump_debug_info,
    w_mysqli_debug => link::mysqli_debug,
    w_mysqli_errno => link::mysqli_errno,
    w_mysqli_error => link::mysqli_error,
    w_mysqli_error_list => link::mysqli_error_list,
    w_mysqli_stmt_execute => stmt::mysqli_stmt_execute,
    w_mysqli_execute => stmt::mysqli_stmt_execute,
    w_mysqli_execute_query => stmt::mysqli_execute_query,
    w_mysqli_fetch_field => res::mysqli_fetch_field,
    w_mysqli_fetch_fields => res::mysqli_fetch_fields,
    w_mysqli_fetch_field_direct => res::mysqli_fetch_field_direct,
    w_mysqli_fetch_lengths => res::mysqli_fetch_lengths,
    w_mysqli_fetch_all => res::mysqli_fetch_all,
    w_mysqli_fetch_array => res::mysqli_fetch_array,
    w_mysqli_fetch_assoc => res::mysqli_fetch_assoc,
    w_mysqli_fetch_object => res::mysqli_fetch_object,
    w_mysqli_fetch_row => res::mysqli_fetch_row,
    w_mysqli_fetch_column => res::mysqli_fetch_column,
    w_mysqli_field_count => link::mysqli_field_count,
    w_mysqli_field_seek => res::mysqli_field_seek,
    w_mysqli_field_tell => res::mysqli_field_tell,
    w_mysqli_free_result => res::mysqli_free_result,
    w_mysqli_get_connection_stats => link::mysqli_get_connection_stats,
    w_mysqli_get_client_stats => link::mysqli_get_client_stats,
    w_mysqli_get_charset => link::mysqli_get_charset,
    w_mysqli_get_client_info => link::mysqli_get_client_info,
    w_mysqli_get_client_version => link::mysqli_get_client_version,
    w_mysqli_get_links_stats => link::mysqli_get_links_stats,
    w_mysqli_get_host_info => link::mysqli_get_host_info,
    w_mysqli_get_proto_info => link::mysqli_get_proto_info,
    w_mysqli_get_server_info => link::mysqli_get_server_info,
    w_mysqli_get_server_version => link::mysqli_get_server_version,
    w_mysqli_get_warnings => link::mysqli_get_warnings,
    w_mysqli_init => link::mysqli_init,
    w_mysqli_info => link::mysqli_info,
    w_mysqli_insert_id => link::mysqli_insert_id,
    w_mysqli_kill => link::mysqli_kill,
    w_mysqli_more_results => link::mysqli_more_results,
    w_mysqli_multi_query => link::mysqli_multi_query,
    w_mysqli_next_result => link::mysqli_next_result,
    w_mysqli_num_fields => res::mysqli_num_fields,
    w_mysqli_num_rows => res::mysqli_num_rows,
    w_mysqli_options => link::mysqli_options,
    w_mysqli_set_opt => link::mysqli_options,
    w_mysqli_ping => link::mysqli_ping,
    w_mysqli_poll => link::mysqli_poll,
    w_mysqli_prepare => stmt::mysqli_prepare,
    w_mysqli_report => link::mysqli_report,
    w_mysqli_query => link::mysqli_query,
    w_mysqli_real_connect => link::mysqli_real_connect,
    w_mysqli_real_escape_string => link::mysqli_real_escape_string,
    w_mysqli_escape_string => link::mysqli_real_escape_string,
    w_mysqli_real_query => link::mysqli_real_query,
    w_mysqli_reap_async_query => link::mysqli_reap_async_query,
    w_mysqli_release_savepoint => link::mysqli_release_savepoint,
    w_mysqli_rollback => link::mysqli_rollback,
    w_mysqli_savepoint => link::mysqli_savepoint,
    w_mysqli_select_db => link::mysqli_select_db,
    w_mysqli_set_charset => link::mysqli_set_charset,
    w_mysqli_stmt_affected_rows => stmt::mysqli_stmt_affected_rows,
    w_mysqli_stmt_attr_get => stmt::mysqli_stmt_attr_get,
    w_mysqli_stmt_attr_set => stmt::mysqli_stmt_attr_set,
    w_mysqli_stmt_bind_param => stmt::mysqli_stmt_bind_param,
    w_mysqli_stmt_bind_result => stmt::mysqli_stmt_bind_result,
    w_mysqli_stmt_close => stmt::mysqli_stmt_close,
    w_mysqli_stmt_data_seek => stmt::mysqli_stmt_data_seek,
    w_mysqli_stmt_errno => stmt::mysqli_stmt_errno,
    w_mysqli_stmt_error => stmt::mysqli_stmt_error,
    w_mysqli_stmt_error_list => stmt::mysqli_stmt_error_list,
    w_mysqli_stmt_fetch => stmt::mysqli_stmt_fetch,
    w_mysqli_stmt_field_count => stmt::mysqli_stmt_field_count,
    w_mysqli_stmt_free_result => stmt::mysqli_stmt_free_result,
    w_mysqli_stmt_get_result => stmt::mysqli_stmt_get_result,
    w_mysqli_stmt_get_warnings => stmt::mysqli_stmt_get_warnings,
    w_mysqli_stmt_init => stmt::mysqli_stmt_init,
    w_mysqli_stmt_insert_id => stmt::mysqli_stmt_insert_id,
    w_mysqli_stmt_more_results => stmt::mysqli_stmt_more_results,
    w_mysqli_stmt_next_result => stmt::mysqli_stmt_next_result,
    w_mysqli_stmt_num_rows => stmt::mysqli_stmt_num_rows,
    w_mysqli_stmt_param_count => stmt::mysqli_stmt_param_count,
    w_mysqli_stmt_prepare => stmt::mysqli_stmt_prepare,
    w_mysqli_stmt_reset => stmt::mysqli_stmt_reset,
    w_mysqli_stmt_result_metadata => stmt::mysqli_stmt_result_metadata,
    w_mysqli_stmt_send_long_data => stmt::mysqli_stmt_send_long_data,
    w_mysqli_stmt_store_result => stmt::mysqli_stmt_store_result,
    w_mysqli_stmt_sqlstate => stmt::mysqli_stmt_sqlstate,
    w_mysqli_sqlstate => link::mysqli_sqlstate,
    w_mysqli_ssl_set => link::mysqli_ssl_set,
    w_mysqli_stat => link::mysqli_stat,
    w_mysqli_store_result => link::mysqli_store_result,
    w_mysqli_thread_id => link::mysqli_thread_id,
    w_mysqli_thread_safe => link::mysqli_thread_safe,
    w_mysqli_use_result => link::mysqli_use_result,
    w_mysqli_warning_count => link::mysqli_warning_count,
    w_mysqli_refresh => link::mysqli_refresh,
}

const WRAPPERS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("mysqli_affected_rows", w_mysqli_affected_rows),
    ("mysqli_autocommit", w_mysqli_autocommit),
    ("mysqli_begin_transaction", w_mysqli_begin_transaction),
    ("mysqli_change_user", w_mysqli_change_user),
    ("mysqli_character_set_name", w_mysqli_character_set_name),
    ("mysqli_close", w_mysqli_close),
    ("mysqli_commit", w_mysqli_commit),
    ("mysqli_connect", w_mysqli_connect),
    ("mysqli_connect_errno", w_mysqli_connect_errno),
    ("mysqli_connect_error", w_mysqli_connect_error),
    ("mysqli_data_seek", w_mysqli_data_seek),
    ("mysqli_dump_debug_info", w_mysqli_dump_debug_info),
    ("mysqli_debug", w_mysqli_debug),
    ("mysqli_errno", w_mysqli_errno),
    ("mysqli_error", w_mysqli_error),
    ("mysqli_error_list", w_mysqli_error_list),
    ("mysqli_stmt_execute", w_mysqli_stmt_execute),
    ("mysqli_execute", w_mysqli_execute),
    ("mysqli_execute_query", w_mysqli_execute_query),
    ("mysqli_fetch_field", w_mysqli_fetch_field),
    ("mysqli_fetch_fields", w_mysqli_fetch_fields),
    ("mysqli_fetch_field_direct", w_mysqli_fetch_field_direct),
    ("mysqli_fetch_lengths", w_mysqli_fetch_lengths),
    ("mysqli_fetch_all", w_mysqli_fetch_all),
    ("mysqli_fetch_array", w_mysqli_fetch_array),
    ("mysqli_fetch_assoc", w_mysqli_fetch_assoc),
    ("mysqli_fetch_object", w_mysqli_fetch_object),
    ("mysqli_fetch_row", w_mysqli_fetch_row),
    ("mysqli_fetch_column", w_mysqli_fetch_column),
    ("mysqli_field_count", w_mysqli_field_count),
    ("mysqli_field_seek", w_mysqli_field_seek),
    ("mysqli_field_tell", w_mysqli_field_tell),
    ("mysqli_free_result", w_mysqli_free_result),
    ("mysqli_get_connection_stats", w_mysqli_get_connection_stats),
    ("mysqli_get_client_stats", w_mysqli_get_client_stats),
    ("mysqli_get_charset", w_mysqli_get_charset),
    ("mysqli_get_client_info", w_mysqli_get_client_info),
    ("mysqli_get_client_version", w_mysqli_get_client_version),
    ("mysqli_get_links_stats", w_mysqli_get_links_stats),
    ("mysqli_get_host_info", w_mysqli_get_host_info),
    ("mysqli_get_proto_info", w_mysqli_get_proto_info),
    ("mysqli_get_server_info", w_mysqli_get_server_info),
    ("mysqli_get_server_version", w_mysqli_get_server_version),
    ("mysqli_get_warnings", w_mysqli_get_warnings),
    ("mysqli_init", w_mysqli_init),
    ("mysqli_info", w_mysqli_info),
    ("mysqli_insert_id", w_mysqli_insert_id),
    ("mysqli_kill", w_mysqli_kill),
    ("mysqli_more_results", w_mysqli_more_results),
    ("mysqli_multi_query", w_mysqli_multi_query),
    ("mysqli_next_result", w_mysqli_next_result),
    ("mysqli_num_fields", w_mysqli_num_fields),
    ("mysqli_num_rows", w_mysqli_num_rows),
    ("mysqli_options", w_mysqli_options),
    ("mysqli_set_opt", w_mysqli_set_opt),
    ("mysqli_ping", w_mysqli_ping),
    ("mysqli_poll", w_mysqli_poll),
    ("mysqli_prepare", w_mysqli_prepare),
    ("mysqli_report", w_mysqli_report),
    ("mysqli_query", w_mysqli_query),
    ("mysqli_real_connect", w_mysqli_real_connect),
    ("mysqli_real_escape_string", w_mysqli_real_escape_string),
    ("mysqli_escape_string", w_mysqli_escape_string),
    ("mysqli_real_query", w_mysqli_real_query),
    ("mysqli_reap_async_query", w_mysqli_reap_async_query),
    ("mysqli_release_savepoint", w_mysqli_release_savepoint),
    ("mysqli_rollback", w_mysqli_rollback),
    ("mysqli_savepoint", w_mysqli_savepoint),
    ("mysqli_select_db", w_mysqli_select_db),
    ("mysqli_set_charset", w_mysqli_set_charset),
    ("mysqli_stmt_affected_rows", w_mysqli_stmt_affected_rows),
    ("mysqli_stmt_attr_get", w_mysqli_stmt_attr_get),
    ("mysqli_stmt_attr_set", w_mysqli_stmt_attr_set),
    ("mysqli_stmt_bind_param", w_mysqli_stmt_bind_param),
    ("mysqli_stmt_bind_result", w_mysqli_stmt_bind_result),
    ("mysqli_stmt_close", w_mysqli_stmt_close),
    ("mysqli_stmt_data_seek", w_mysqli_stmt_data_seek),
    ("mysqli_stmt_errno", w_mysqli_stmt_errno),
    ("mysqli_stmt_error", w_mysqli_stmt_error),
    ("mysqli_stmt_error_list", w_mysqli_stmt_error_list),
    ("mysqli_stmt_fetch", w_mysqli_stmt_fetch),
    ("mysqli_stmt_field_count", w_mysqli_stmt_field_count),
    ("mysqli_stmt_free_result", w_mysqli_stmt_free_result),
    ("mysqli_stmt_get_result", w_mysqli_stmt_get_result),
    ("mysqli_stmt_get_warnings", w_mysqli_stmt_get_warnings),
    ("mysqli_stmt_init", w_mysqli_stmt_init),
    ("mysqli_stmt_insert_id", w_mysqli_stmt_insert_id),
    ("mysqli_stmt_more_results", w_mysqli_stmt_more_results),
    ("mysqli_stmt_next_result", w_mysqli_stmt_next_result),
    ("mysqli_stmt_num_rows", w_mysqli_stmt_num_rows),
    ("mysqli_stmt_param_count", w_mysqli_stmt_param_count),
    ("mysqli_stmt_prepare", w_mysqli_stmt_prepare),
    ("mysqli_stmt_reset", w_mysqli_stmt_reset),
    ("mysqli_stmt_result_metadata", w_mysqli_stmt_result_metadata),
    ("mysqli_stmt_send_long_data", w_mysqli_stmt_send_long_data),
    ("mysqli_stmt_store_result", w_mysqli_stmt_store_result),
    ("mysqli_stmt_sqlstate", w_mysqli_stmt_sqlstate),
    ("mysqli_sqlstate", w_mysqli_sqlstate),
    ("mysqli_ssl_set", w_mysqli_ssl_set),
    ("mysqli_stat", w_mysqli_stat),
    ("mysqli_store_result", w_mysqli_store_result),
    ("mysqli_thread_id", w_mysqli_thread_id),
    ("mysqli_thread_safe", w_mysqli_thread_safe),
    ("mysqli_use_result", w_mysqli_use_result),
    ("mysqli_warning_count", w_mysqli_warning_count),
    ("mysqli_refresh", w_mysqli_refresh),
];

fn long_consts() -> Vec<(&'static str, i64)> {
    use mysqlnd::*;
    vec![
        ("MYSQLI_READ_DEFAULT_GROUP", MYSQL_READ_DEFAULT_GROUP),
        ("MYSQLI_READ_DEFAULT_FILE", MYSQL_READ_DEFAULT_FILE),
        ("MYSQLI_OPT_CONNECT_TIMEOUT", MYSQL_OPT_CONNECT_TIMEOUT),
        ("MYSQLI_OPT_LOCAL_INFILE", MYSQL_OPT_LOCAL_INFILE),
        (
            "MYSQLI_OPT_LOAD_DATA_LOCAL_DIR",
            MYSQL_OPT_LOAD_DATA_LOCAL_DIR,
        ),
        ("MYSQLI_INIT_COMMAND", MYSQL_INIT_COMMAND),
        ("MYSQLI_OPT_READ_TIMEOUT", MYSQL_OPT_READ_TIMEOUT),
        (
            "MYSQLI_OPT_NET_CMD_BUFFER_SIZE",
            MYSQLND_OPT_NET_CMD_BUFFER_SIZE,
        ),
        (
            "MYSQLI_OPT_NET_READ_BUFFER_SIZE",
            MYSQLND_OPT_NET_READ_BUFFER_SIZE,
        ),
        (
            "MYSQLI_OPT_INT_AND_FLOAT_NATIVE",
            MYSQLND_OPT_INT_AND_FLOAT_NATIVE,
        ),
        (
            "MYSQLI_OPT_SSL_VERIFY_SERVER_CERT",
            MYSQL_OPT_SSL_VERIFY_SERVER_CERT,
        ),
        ("MYSQLI_SERVER_PUBLIC_KEY", MYSQL_SERVER_PUBLIC_KEY),
        ("MYSQLI_CLIENT_SSL", CLIENT_SSL as i64),
        ("MYSQLI_CLIENT_COMPRESS", CLIENT_COMPRESS as i64),
        ("MYSQLI_CLIENT_INTERACTIVE", CLIENT_INTERACTIVE as i64),
        ("MYSQLI_CLIENT_IGNORE_SPACE", CLIENT_IGNORE_SPACE as i64),
        ("MYSQLI_CLIENT_NO_SCHEMA", CLIENT_NO_SCHEMA as i64),
        ("MYSQLI_CLIENT_FOUND_ROWS", CLIENT_FOUND_ROWS as i64),
        (
            "MYSQLI_CLIENT_SSL_VERIFY_SERVER_CERT",
            CLIENT_SSL_VERIFY_SERVER_CERT as i64,
        ),
        (
            "MYSQLI_CLIENT_SSL_DONT_VERIFY_SERVER_CERT",
            CLIENT_SSL_DONT_VERIFY_SERVER_CERT as i64,
        ),
        (
            "MYSQLI_CLIENT_CAN_HANDLE_EXPIRED_PASSWORDS",
            CLIENT_CAN_HANDLE_EXPIRED_PASSWORDS as i64,
        ),
        (
            "MYSQLI_OPT_CAN_HANDLE_EXPIRED_PASSWORDS",
            MYSQL_OPT_CAN_HANDLE_EXPIRED_PASSWORDS,
        ),
        ("MYSQLI_STORE_RESULT", 0),
        ("MYSQLI_USE_RESULT", 1),
        ("MYSQLI_ASYNC", 8),
        ("MYSQLI_STORE_RESULT_COPY_DATA", 16),
        ("MYSQLI_ASSOC", 1),
        ("MYSQLI_NUM", 2),
        ("MYSQLI_BOTH", 3),
        ("MYSQLI_STMT_ATTR_UPDATE_MAX_LENGTH", 0),
        ("MYSQLI_STMT_ATTR_CURSOR_TYPE", 1),
        ("MYSQLI_CURSOR_TYPE_NO_CURSOR", 0),
        ("MYSQLI_CURSOR_TYPE_READ_ONLY", 1),
        ("MYSQLI_NOT_NULL_FLAG", NOT_NULL_FLAG as i64),
        ("MYSQLI_PRI_KEY_FLAG", PRI_KEY_FLAG as i64),
        ("MYSQLI_UNIQUE_KEY_FLAG", UNIQUE_KEY_FLAG as i64),
        ("MYSQLI_MULTIPLE_KEY_FLAG", MULTIPLE_KEY_FLAG as i64),
        ("MYSQLI_BLOB_FLAG", BLOB_FLAG as i64),
        ("MYSQLI_UNSIGNED_FLAG", UNSIGNED_FLAG as i64),
        ("MYSQLI_ZEROFILL_FLAG", ZEROFILL_FLAG as i64),
        ("MYSQLI_AUTO_INCREMENT_FLAG", AUTO_INCREMENT_FLAG as i64),
        ("MYSQLI_TIMESTAMP_FLAG", TIMESTAMP_FLAG as i64),
        ("MYSQLI_SET_FLAG", SET_FLAG as i64),
        ("MYSQLI_NUM_FLAG", NUM_FLAG as i64),
        ("MYSQLI_PART_KEY_FLAG", PART_KEY_FLAG as i64),
        ("MYSQLI_GROUP_FLAG", GROUP_FLAG as i64),
        ("MYSQLI_ENUM_FLAG", ENUM_FLAG as i64),
        ("MYSQLI_BINARY_FLAG", BINARY_FLAG as i64),
        ("MYSQLI_NO_DEFAULT_VALUE_FLAG", NO_DEFAULT_VALUE_FLAG as i64),
        ("MYSQLI_ON_UPDATE_NOW_FLAG", ON_UPDATE_NOW_FLAG as i64),
        ("MYSQLI_TYPE_DECIMAL", MYSQL_TYPE_DECIMAL as i64),
        ("MYSQLI_TYPE_TINY", MYSQL_TYPE_TINY as i64),
        ("MYSQLI_TYPE_SHORT", MYSQL_TYPE_SHORT as i64),
        ("MYSQLI_TYPE_LONG", MYSQL_TYPE_LONG as i64),
        ("MYSQLI_TYPE_FLOAT", MYSQL_TYPE_FLOAT as i64),
        ("MYSQLI_TYPE_DOUBLE", MYSQL_TYPE_DOUBLE as i64),
        ("MYSQLI_TYPE_NULL", MYSQL_TYPE_NULL as i64),
        ("MYSQLI_TYPE_TIMESTAMP", MYSQL_TYPE_TIMESTAMP as i64),
        ("MYSQLI_TYPE_LONGLONG", MYSQL_TYPE_LONGLONG as i64),
        ("MYSQLI_TYPE_INT24", MYSQL_TYPE_INT24 as i64),
        ("MYSQLI_TYPE_DATE", MYSQL_TYPE_DATE as i64),
        ("MYSQLI_TYPE_TIME", MYSQL_TYPE_TIME as i64),
        ("MYSQLI_TYPE_DATETIME", MYSQL_TYPE_DATETIME as i64),
        ("MYSQLI_TYPE_YEAR", MYSQL_TYPE_YEAR as i64),
        ("MYSQLI_TYPE_NEWDATE", MYSQL_TYPE_NEWDATE as i64),
        ("MYSQLI_TYPE_ENUM", MYSQL_TYPE_ENUM as i64),
        ("MYSQLI_TYPE_SET", MYSQL_TYPE_SET as i64),
        ("MYSQLI_TYPE_TINY_BLOB", MYSQL_TYPE_TINY_BLOB as i64),
        ("MYSQLI_TYPE_MEDIUM_BLOB", MYSQL_TYPE_MEDIUM_BLOB as i64),
        ("MYSQLI_TYPE_LONG_BLOB", MYSQL_TYPE_LONG_BLOB as i64),
        ("MYSQLI_TYPE_BLOB", MYSQL_TYPE_BLOB as i64),
        ("MYSQLI_TYPE_VAR_STRING", MYSQL_TYPE_VAR_STRING as i64),
        ("MYSQLI_TYPE_STRING", MYSQL_TYPE_STRING as i64),
        ("MYSQLI_TYPE_CHAR", MYSQL_TYPE_TINY as i64),
        ("MYSQLI_TYPE_GEOMETRY", MYSQL_TYPE_GEOMETRY as i64),
        ("MYSQLI_TYPE_VECTOR", MYSQL_TYPE_VECTOR as i64),
        ("MYSQLI_TYPE_JSON", MYSQL_TYPE_JSON as i64),
        ("MYSQLI_TYPE_NEWDECIMAL", MYSQL_TYPE_NEWDECIMAL as i64),
        ("MYSQLI_TYPE_BIT", MYSQL_TYPE_BIT as i64),
        ("MYSQLI_SET_CHARSET_NAME", MYSQL_SET_CHARSET_NAME),
        ("MYSQLI_NO_DATA", 100),
        ("MYSQLI_DATA_TRUNCATED", 101),
        ("MYSQLI_REPORT_INDEX", 4),
        ("MYSQLI_REPORT_ERROR", 1),
        ("MYSQLI_REPORT_STRICT", 2),
        ("MYSQLI_REPORT_ALL", 255),
        ("MYSQLI_REPORT_OFF", 0),
        ("MYSQLI_DEBUG_TRACE_ENABLED", 0),
        (
            "MYSQLI_SERVER_QUERY_NO_GOOD_INDEX_USED",
            SERVER_QUERY_NO_GOOD_INDEX_USED as i64,
        ),
        (
            "MYSQLI_SERVER_QUERY_NO_INDEX_USED",
            SERVER_QUERY_NO_INDEX_USED as i64,
        ),
        ("MYSQLI_SERVER_QUERY_WAS_SLOW", SERVER_QUERY_WAS_SLOW as i64),
        ("MYSQLI_SERVER_PS_OUT_PARAMS", SERVER_PS_OUT_PARAMS as i64),
        ("MYSQLI_REFRESH_GRANT", 1),
        ("MYSQLI_REFRESH_LOG", 2),
        ("MYSQLI_REFRESH_TABLES", 4),
        ("MYSQLI_REFRESH_HOSTS", 8),
        ("MYSQLI_REFRESH_STATUS", 16),
        ("MYSQLI_REFRESH_THREADS", 32),
        ("MYSQLI_REFRESH_REPLICA", 64),
        ("MYSQLI_REFRESH_SLAVE", 64),
        ("MYSQLI_REFRESH_MASTER", 128),
        ("MYSQLI_REFRESH_BACKUP_LOG", 0x200000),
        (
            "MYSQLI_TRANS_START_WITH_CONSISTENT_SNAPSHOT",
            TRANS_START_WITH_CONSISTENT_SNAPSHOT as i64,
        ),
        (
            "MYSQLI_TRANS_START_READ_WRITE",
            TRANS_START_READ_WRITE as i64,
        ),
        ("MYSQLI_TRANS_START_READ_ONLY", TRANS_START_READ_ONLY as i64),
        ("MYSQLI_TRANS_COR_AND_CHAIN", TRANS_COR_AND_CHAIN as i64),
        (
            "MYSQLI_TRANS_COR_AND_NO_CHAIN",
            TRANS_COR_AND_NO_CHAIN as i64,
        ),
        ("MYSQLI_TRANS_COR_RELEASE", TRANS_COR_RELEASE as i64),
        ("MYSQLI_TRANS_COR_NO_RELEASE", TRANS_COR_NO_RELEASE as i64),
    ]
}

const DEPRECATED_CONSTS: &[(&str, &str)] = &[
    (
        "MYSQLI_STORE_RESULT_COPY_DATA",
        "Constant MYSQLI_STORE_RESULT_COPY_DATA is deprecated since 8.4, as the mysqli_store_result() parameter is unused since 8.1",
    ),
    (
        "MYSQLI_NO_DATA",
        "Constant MYSQLI_NO_DATA is deprecated since 8.1, as it was unused",
    ),
    (
        "MYSQLI_DATA_TRUNCATED",
        "Constant MYSQLI_DATA_TRUNCATED is deprecated since 8.1, as it was unused",
    ),
    (
        "MYSQLI_SERVER_QUERY_NO_GOOD_INDEX_USED",
        "Constant MYSQLI_SERVER_QUERY_NO_GOOD_INDEX_USED is deprecated since 8.1, as it was unused",
    ),
    (
        "MYSQLI_SERVER_QUERY_NO_INDEX_USED",
        "Constant MYSQLI_SERVER_QUERY_NO_INDEX_USED is deprecated since 8.1, as it was unused",
    ),
    (
        "MYSQLI_SERVER_QUERY_WAS_SLOW",
        "Constant MYSQLI_SERVER_QUERY_WAS_SLOW is deprecated since 8.1, as it was unused",
    ),
    (
        "MYSQLI_SERVER_PS_OUT_PARAMS",
        "Constant MYSQLI_SERVER_PS_OUT_PARAMS is deprecated since 8.1, as it was unused",
    ),
    (
        "MYSQLI_REFRESH_GRANT",
        "Constant MYSQLI_REFRESH_GRANT is deprecated since 8.4, as mysqli_refresh() is deprecated",
    ),
    (
        "MYSQLI_REFRESH_LOG",
        "Constant MYSQLI_REFRESH_LOG is deprecated since 8.4, as mysqli_refresh() is deprecated",
    ),
    (
        "MYSQLI_REFRESH_TABLES",
        "Constant MYSQLI_REFRESH_TABLES is deprecated since 8.4, as mysqli_refresh() is deprecated",
    ),
    (
        "MYSQLI_REFRESH_HOSTS",
        "Constant MYSQLI_REFRESH_HOSTS is deprecated since 8.4, as mysqli_refresh() is deprecated",
    ),
    (
        "MYSQLI_REFRESH_STATUS",
        "Constant MYSQLI_REFRESH_STATUS is deprecated since 8.4, as mysqli_refresh() is deprecated",
    ),
    (
        "MYSQLI_REFRESH_THREADS",
        "Constant MYSQLI_REFRESH_THREADS is deprecated since 8.4, as mysqli_refresh() is deprecated",
    ),
    (
        "MYSQLI_REFRESH_REPLICA",
        "Constant MYSQLI_REFRESH_REPLICA is deprecated since 8.4, as mysqli_refresh() is deprecated",
    ),
    (
        "MYSQLI_REFRESH_SLAVE",
        "Constant MYSQLI_REFRESH_SLAVE is deprecated since 8.4, as mysqli_refresh() is deprecated",
    ),
    (
        "MYSQLI_REFRESH_MASTER",
        "Constant MYSQLI_REFRESH_MASTER is deprecated since 8.4, as mysqli_refresh() is deprecated",
    ),
    (
        "MYSQLI_REFRESH_BACKUP_LOG",
        "Constant MYSQLI_REFRESH_BACKUP_LOG is deprecated since 8.4, as mysqli_refresh() is deprecated",
    ),
    (
        "MYSQLI_IS_MARIADB",
        "Constant MYSQLI_IS_MARIADB is deprecated since 8.2, as it is always false",
    ),
];

/// INI entries: (name, default, changeable at runtime)
const INI: &[(&str, Option<&str>, bool)] = &[
    ("mysqli.max_links", Some("-1"), false),
    ("mysqli.max_persistent", Some("-1"), false),
    ("mysqli.allow_persistent", Some("1"), false),
    ("mysqli.rollback_on_cached_plink", Some("0"), false),
    ("mysqli.default_host", None, true),
    ("mysqli.default_user", None, true),
    ("mysqli.default_pw", None, true),
    ("mysqli.default_port", Some("3306"), true),
    ("mysqli.default_socket", None, true),
    ("mysqli.allow_local_infile", Some("0"), false),
    ("mysqli.local_infile_directory", None, false),
    ("mysqlnd.collect_statistics", Some("1"), true),
    ("mysqlnd.collect_memory_statistics", Some("0"), false),
    ("mysqlnd.debug", None, false),
    ("mysqlnd.trace_alloc", None, false),
    ("mysqlnd.net_cmd_buffer_size", Some("4096"), true),
    ("mysqlnd.net_read_buffer_size", Some("32768"), true),
    ("mysqlnd.net_read_timeout", Some("86400"), true),
    ("mysqlnd.log_mask", Some("0"), true),
    ("mysqlnd.mempool_default_size", Some("16000"), true),
    ("mysqlnd.sha256_server_public_key", None, false),
];

/// Text-mode DISPLAY_INI_ENTRIES() of the entries starting with `prefix`
fn info_ini_entries(vm: &Vm, prefix: &str, out: &mut String) {
    out.push_str("\nDirective => Local Value => Master Value\n");
    for (name, _, _) in INI.iter().filter(|(n, _, _)| n.starts_with(prefix)) {
        let raw = vm
            .constants
            .get(name.as_bytes())
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_default();
        let shown = match *name {
            "mysqli.allow_persistent"
            | "mysqli.allow_local_infile"
            | "mysqli.rollback_on_cached_plink"
            | "mysqlnd.collect_statistics"
            | "mysqlnd.collect_memory_statistics" => {
                let on = matches!(
                    raw.to_ascii_lowercase().as_str(),
                    "1" | "on" | "yes" | "true"
                );
                if on { "On" } else { "Off" }.to_string()
            }
            "mysqli.max_links" | "mysqli.max_persistent" if raw == "-1" => "Unlimited".to_string(),
            _ if raw.is_empty() => "no value".to_string(),
            _ => raw,
        };
        out.push_str(&format!("{} => {} => {}\n", name, shown, shown));
    }
}

/// PHP_MINFO(mysqli) and PHP_MINFO(mysqlnd), text mode
fn minfo(vm: &mut Vm) -> String {
    let mut out = String::new();
    let (active_p, inactive_p, links) = link::MYG.with(|g| {
        let g = g.borrow();
        (
            g.num_active_persistent,
            g.num_inactive_persistent,
            g.num_links,
        )
    });
    out.push_str("\nmysqli\n\n");
    out.push_str("MysqlI Support => enabled\n");
    out.push_str(&format!(
        "Client API library version => {}\n",
        link::client_info(vm)
    ));
    out.push_str(&format!("Active Persistent Links => {}\n", active_p));
    out.push_str(&format!("Inactive Persistent Links => {}\n", inactive_p));
    out.push_str(&format!("Active Links => {}\n", links));
    info_ini_entries(vm, "mysqli.", &mut out);
    out.push_str("\nmysqlnd\n\n");
    out.push_str("mysqlnd => enabled\n");
    out.push_str(&format!("Version => {}\n", link::client_info(vm)));
    out.push_str("Compression => supported\n");
    out.push_str("core SSL => not supported\n");
    out.push_str("extended SSL => not supported\n");
    let ini = |vm: &Vm, k: &str| {
        vm.constants
            .get(k.as_bytes())
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_default()
    };
    out.push_str(&format!(
        "Command buffer size => {}\n",
        ini(vm, "mysqlnd.net_cmd_buffer_size")
    ));
    out.push_str(&format!(
        "Read buffer size => {}\n",
        ini(vm, "mysqlnd.net_read_buffer_size")
    ));
    out.push_str(&format!(
        "Read timeout => {}\n",
        ini(vm, "mysqlnd.net_read_timeout")
    ));
    out.push_str("Collecting statistics => Yes\n");
    out.push_str("Collecting memory statistics => No\n");
    out.push_str("Tracing => n/a\n");
    out.push_str("Loaded plugins => mysqlnd,auth_plugin_mysql_native_password,auth_plugin_mysql_clear_password,auth_plugin_caching_sha2_password,auth_plugin_sha256_password\n");
    out.push_str("API Extensions => mysqli\n");
    info_ini_entries(vm, "mysqlnd.", &mut out);
    out.push_str("\nmysqlnd statistics =>  \n");
    let values = mysqlnd::stats::global_values();
    for (name, v) in mysqlnd::stats::NAMES.iter().zip(values) {
        out.push_str(&format!("{} => {}\n", name, v));
    }
    out
}

fn ini_set_hook(vm: &mut Vm, key: &[u8], value: &Value) -> bool {
    if let Some((_, _, changeable)) = INI.iter().find(|(n, _, _)| n.as_bytes() == key) {
        if !changeable {
            return false;
        }
    }
    if key == b"mysqlnd.net_cmd_buffer_size" {
        let n = value.to_long();
        if n < 4096 {
            return false;
        }
    }
    let _ = vm;
    true
}

pub fn register(vm: &mut Vm) {
    vm.register_extension(b"mysqlnd");
    vm.register_extension(b"mysqli");

    for (name, func) in WRAPPERS {
        let params: Vec<&'static [u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|f| f.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        let params: &'static [&'static [u8]] = Box::leak(params.into_boxed_slice());
        vm.register_function_with_params(name.as_bytes(), *func, params);
    }
    let _ = FUNCTIONS;

    for (name, v) in long_consts() {
        vm.constants
            .insert(name.as_bytes().to_vec(), Value::Long(v));
    }
    vm.constants
        .insert(b"MYSQLI_IS_MARIADB".to_vec(), Value::False);
    for (name, msg) in DEPRECATED_CONSTS {
        vm.internal_deprecated_constants
            .insert(name.as_bytes().to_vec(), msg.to_string());
    }

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

    // classes: the VM knows internal classes by name (their shape comes from
    // the stubs); only the exception needs an entry for its property.
    let mut se = ClassEntry::new(b"mysqli_sql_exception".to_vec());
    se.parent = Some(b"RuntimeException".to_vec());
    se.is_final = true;
    se.properties = goro_core::stubs::own_properties(vm, b"mysqli_sql_exception");
    vm.classes.insert(b"mysqli_sql_exception".to_vec(), se);

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
        &b"mysqli"[..],
        b"mysqli_driver",
        b"mysqli_result",
        b"mysqli_stmt",
        b"mysqli_warning",
        b"mysqli_sql_exception",
    ] {
        vm.native_class_handlers.insert(c.to_vec(), h);
    }
    // InternalIterator is shared with other extensions (DatePeriod...)
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

    for c in [
        &b"mysqli"[..],
        b"mysqli_driver",
        b"mysqli_result",
        b"mysqli_stmt",
        b"mysqli_warning",
    ] {
        vm.object_create_handlers
            .insert(c.to_vec(), objects::create_object);
    }
    vm.object_construct_handlers
        .insert(b"mysqli".to_vec(), construct_mysqli);
    vm.object_construct_handlers
        .insert(b"mysqli_result".to_vec(), construct_result);
    vm.object_construct_handlers
        .insert(b"mysqli_stmt".to_vec(), construct_stmt);
    let _ = Rc::new(());
}
