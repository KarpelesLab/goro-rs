//! php-src's ext/pgsql (pgsql.c) over `libpq`, a pure-Rust port of
//! PostgreSQL's client library (also used by pdo_pgsql).

#![allow(clippy::result_unit_err)]

pub mod copy;
pub mod fetch;
pub mod handles;
pub mod libpq;
pub mod link;
pub mod lob;
pub mod meta;
pub mod zpp;

use goro_core::object::ClassEntry;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

/// The extension name (`extension_loaded('pgsql')`).
pub const NAME: &[u8] = b"pgsql";

pub type R = Result<Value, VmError>;

/// An implementation: (vm, the called name, args)
pub type Impl = fn(&mut Vm, &'static str, &[Value]) -> Value;

/// The libpq version string (php_libpq_version)
pub fn libpq_version() -> String {
    let v = libpq::LIB_VERSION;
    let major = v / 10000;
    if major >= 10 {
        format!("{}.{}", major, v % 10000)
    } else {
        format!("{}.{}.{}", major, v / 100 % 100, v % 100)
    }
}

const LONG_CONSTANTS: &[(&str, i64)] = &[
    ("PGSQL_CONNECT_FORCE_NEW", 2),
    ("PGSQL_CONNECT_ASYNC", 4),
    ("PGSQL_ASSOC", 1),
    ("PGSQL_NUM", 2),
    ("PGSQL_BOTH", 3),
    ("PGSQL_NOTICE_LAST", 1),
    ("PGSQL_NOTICE_ALL", 2),
    ("PGSQL_NOTICE_CLEAR", 3),
    ("PGSQL_CONNECTION_BAD", 1),
    ("PGSQL_CONNECTION_OK", 0),
    ("PGSQL_CONNECTION_STARTED", 2),
    ("PGSQL_CONNECTION_MADE", 3),
    ("PGSQL_CONNECTION_AWAITING_RESPONSE", 4),
    ("PGSQL_CONNECTION_AUTH_OK", 5),
    ("PGSQL_CONNECTION_SSL_STARTUP", 7),
    ("PGSQL_CONNECTION_SETENV", 6),
    ("PGSQL_POLLING_FAILED", 0),
    ("PGSQL_POLLING_READING", 1),
    ("PGSQL_POLLING_WRITING", 2),
    ("PGSQL_POLLING_OK", 3),
    ("PGSQL_POLLING_ACTIVE", 4),
    ("PGSQL_TRANSACTION_IDLE", 0),
    ("PGSQL_TRANSACTION_ACTIVE", 1),
    ("PGSQL_TRANSACTION_INTRANS", 2),
    ("PGSQL_TRANSACTION_INERROR", 3),
    ("PGSQL_TRANSACTION_UNKNOWN", 4),
    ("PGSQL_ERRORS_TERSE", 0),
    ("PGSQL_ERRORS_DEFAULT", 1),
    ("PGSQL_ERRORS_VERBOSE", 2),
    ("PGSQL_ERRORS_SQLSTATE", 3),
    ("PGSQL_SEEK_SET", 0),
    ("PGSQL_SEEK_CUR", 1),
    ("PGSQL_SEEK_END", 2),
    ("PGSQL_STATUS_LONG", 1),
    ("PGSQL_STATUS_STRING", 2),
    ("PGSQL_EMPTY_QUERY", 0),
    ("PGSQL_COMMAND_OK", 1),
    ("PGSQL_TUPLES_OK", 2),
    ("PGSQL_TUPLES_CHUNK", 12),
    ("PGSQL_COPY_OUT", 3),
    ("PGSQL_COPY_IN", 4),
    ("PGSQL_BAD_RESPONSE", 5),
    ("PGSQL_NONFATAL_ERROR", 6),
    ("PGSQL_FATAL_ERROR", 7),
    ("PGSQL_DIAG_SEVERITY", b'S' as i64),
    ("PGSQL_DIAG_SQLSTATE", b'C' as i64),
    ("PGSQL_DIAG_MESSAGE_PRIMARY", b'M' as i64),
    ("PGSQL_DIAG_MESSAGE_DETAIL", b'D' as i64),
    ("PGSQL_DIAG_MESSAGE_HINT", b'H' as i64),
    ("PGSQL_DIAG_STATEMENT_POSITION", b'P' as i64),
    ("PGSQL_DIAG_INTERNAL_POSITION", b'p' as i64),
    ("PGSQL_DIAG_INTERNAL_QUERY", b'q' as i64),
    ("PGSQL_DIAG_CONTEXT", b'W' as i64),
    ("PGSQL_DIAG_SOURCE_FILE", b'F' as i64),
    ("PGSQL_DIAG_SOURCE_LINE", b'L' as i64),
    ("PGSQL_DIAG_SOURCE_FUNCTION", b'R' as i64),
    ("PGSQL_DIAG_SCHEMA_NAME", b's' as i64),
    ("PGSQL_DIAG_TABLE_NAME", b't' as i64),
    ("PGSQL_DIAG_COLUMN_NAME", b'c' as i64),
    ("PGSQL_DIAG_DATATYPE_NAME", b'd' as i64),
    ("PGSQL_DIAG_CONSTRAINT_NAME", b'n' as i64),
    ("PGSQL_DIAG_SEVERITY_NONLOCALIZED", b'V' as i64),
    ("PGSQL_CONV_IGNORE_DEFAULT", 2),
    ("PGSQL_CONV_FORCE_NULL", 4),
    ("PGSQL_CONV_IGNORE_NOT_NULL", 8),
    ("PGSQL_DML_ESCAPE", 4096),
    ("PGSQL_DML_NO_CONV", 256),
    ("PGSQL_DML_EXEC", 512),
    ("PGSQL_DML_ASYNC", 1024),
    ("PGSQL_DML_STRING", 2048),
    ("PGSQL_TRACE_SUPPRESS_TIMESTAMPS", 1),
    ("PGSQL_TRACE_REGRESS_MODE", 2),
    ("PGSQL_SHOW_CONTEXT_NEVER", 0),
    ("PGSQL_SHOW_CONTEXT_ERRORS", 1),
    ("PGSQL_SHOW_CONTEXT_ALWAYS", 2),
];

/// (name, default, changeable at runtime)
const INI: &[(&str, &str, bool)] = &[
    ("pgsql.allow_persistent", "1", false),
    ("pgsql.max_persistent", "-1", false),
    ("pgsql.max_links", "-1", false),
    ("pgsql.auto_reset_persistent", "0", false),
    ("pgsql.ignore_notice", "0", true),
    ("pgsql.log_notice", "0", true),
];

/// The #[\Deprecated] aliases: (name, the function to use instead)
fn deprecation(name: &str) -> Option<&'static str> {
    Some(match name {
        "pg_errormessage" => "pg_last_error",
        "pg_numrows" => "pg_num_rows",
        "pg_numfields" => "pg_num_fields",
        "pg_cmdtuples" => "pg_affected_rows",
        "pg_fieldname" => "pg_field_name",
        "pg_fieldsize" => "pg_field_size",
        "pg_fieldtype" => "pg_field_type",
        "pg_fieldnum" => "pg_field_num",
        "pg_result" => "pg_fetch_result",
        "pg_fieldprtlen" => "pg_field_prtlen",
        "pg_fieldisnull" => "pg_field_is_null",
        "pg_freeresult" => "pg_free_result",
        "pg_getlastoid" => "pg_last_oid",
        "pg_locreate" => "pg_lo_create",
        "pg_lounlink" => "pg_lo_unlink",
        "pg_loopen" => "pg_lo_open",
        "pg_loclose" => "pg_lo_close",
        "pg_loread" => "pg_lo_read",
        "pg_lowrite" => "pg_lo_write",
        "pg_loreadall" => "pg_lo_read_all",
        "pg_loimport" => "pg_lo_import",
        "pg_loexport" => "pg_lo_export",
        "pg_setclientencoding" => "pg_set_client_encoding",
        "pg_clientencoding" => "pg_client_encoding",
        _ => return None,
    })
}

/// Run an implementation: the docref name, the deprecation, then the
/// notices libpq reported meanwhile.
fn run(vm: &mut Vm, name: &'static str, args: &[Value], imp: Impl) -> R {
    if let Some(d) = deprecation(name) {
        vm.emit_function_deprecation(&format!(
            "Function {}() is deprecated since 8.0, use {}() instead",
            name, d
        ))?;
        if !own_arity(name) {
            vm.zpp_self_check_flags(name.as_bytes(), args, goro_core::arginfo::CHECK_ARITY)?;
        }
    }
    handles::push_active(name);
    let r = imp(vm, name, args);
    handles::flush_notices(vm);
    handles::pop_active();
    Ok(r)
}

macro_rules! functions {
    ($($name:literal => $imp:path),* $(,)?) => {
        const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
            $(
                ($name, {
                    fn w(vm: &mut Vm, args: &[Value]) -> R {
                        run(vm, $name, args, $imp)
                    }
                    w
                }),
            )*
        ];
    };
}

functions! {
    "pg_connect" => link::pg_connect,
    "pg_pconnect" => link::pg_pconnect,
    "pg_connect_poll" => link::pg_connect_poll,
    "pg_close" => link::pg_close,
    "pg_dbname" => link::pg_dbname,
    "pg_last_error" => link::pg_last_error,
    "pg_errormessage" => link::pg_last_error,
    "pg_options" => link::pg_options,
    "pg_port" => link::pg_port,
    "pg_tty" => link::pg_tty,
    "pg_host" => link::pg_host,
    "pg_version" => link::pg_version,
    "pg_jit" => link::pg_jit,
    "pg_service" => link::pg_service,
    "pg_parameter_status" => link::pg_parameter_status,
    "pg_ping" => link::pg_ping,
    "pg_query" => link::pg_query,
    "pg_exec" => link::pg_query,
    "pg_query_params" => link::pg_query_params,
    "pg_prepare" => link::pg_prepare,
    "pg_execute" => link::pg_execute,
    "pg_num_rows" => fetch::pg_num_rows,
    "pg_numrows" => fetch::pg_num_rows,
    "pg_num_fields" => fetch::pg_num_fields,
    "pg_numfields" => fetch::pg_num_fields,
    "pg_affected_rows" => fetch::pg_affected_rows,
    "pg_cmdtuples" => fetch::pg_affected_rows,
    "pg_last_notice" => link::pg_last_notice,
    "pg_field_table" => fetch::pg_field_table,
    "pg_field_name" => fetch::pg_field_name,
    "pg_fieldname" => fetch::pg_field_name,
    "pg_field_size" => fetch::pg_field_size,
    "pg_fieldsize" => fetch::pg_field_size,
    "pg_field_type" => fetch::pg_field_type,
    "pg_fieldtype" => fetch::pg_field_type,
    "pg_field_type_oid" => fetch::pg_field_type_oid,
    "pg_field_num" => fetch::pg_field_num,
    "pg_fieldnum" => fetch::pg_field_num,
    "pg_fetch_result" => fetch::pg_fetch_result,
    "pg_result" => fetch::pg_fetch_result,
    "pg_fetch_row" => fetch::pg_fetch_row,
    "pg_fetch_assoc" => fetch::pg_fetch_assoc,
    "pg_fetch_array" => fetch::pg_fetch_array,
    "pg_fetch_object" => fetch::pg_fetch_object,
    "pg_fetch_all" => fetch::pg_fetch_all,
    "pg_fetch_all_columns" => fetch::pg_fetch_all_columns,
    "pg_result_seek" => fetch::pg_result_seek,
    "pg_field_prtlen" => fetch::pg_field_prtlen,
    "pg_fieldprtlen" => fetch::pg_fieldprtlen,
    "pg_field_is_null" => fetch::pg_field_is_null,
    "pg_fieldisnull" => fetch::pg_fieldisnull,
    "pg_free_result" => fetch::pg_free_result,
    "pg_freeresult" => fetch::pg_free_result,
    "pg_last_oid" => fetch::pg_last_oid,
    "pg_getlastoid" => fetch::pg_last_oid,
    "pg_trace" => link::pg_trace,
    "pg_untrace" => link::pg_untrace,
    "pg_lo_create" => lob::pg_lo_create,
    "pg_locreate" => lob::pg_lo_create,
    "pg_lo_unlink" => lob::pg_lo_unlink,
    "pg_lounlink" => lob::pg_lo_unlink,
    "pg_lo_open" => lob::pg_lo_open,
    "pg_loopen" => lob::pg_lo_open,
    "pg_lo_close" => lob::pg_lo_close,
    "pg_loclose" => lob::pg_lo_close,
    "pg_lo_read" => lob::pg_lo_read,
    "pg_loread" => lob::pg_lo_read,
    "pg_lo_write" => lob::pg_lo_write,
    "pg_lowrite" => lob::pg_lo_write,
    "pg_lo_read_all" => lob::pg_lo_read_all,
    "pg_loreadall" => lob::pg_lo_read_all,
    "pg_lo_import" => lob::pg_lo_import,
    "pg_loimport" => lob::pg_lo_import,
    "pg_lo_export" => lob::pg_lo_export,
    "pg_loexport" => lob::pg_lo_export,
    "pg_lo_seek" => lob::pg_lo_seek,
    "pg_lo_tell" => lob::pg_lo_tell,
    "pg_lo_truncate" => lob::pg_lo_truncate,
    "pg_set_error_verbosity" => link::pg_set_error_verbosity,
    "pg_set_client_encoding" => link::pg_set_client_encoding,
    "pg_setclientencoding" => link::pg_set_client_encoding,
    "pg_client_encoding" => link::pg_client_encoding,
    "pg_clientencoding" => link::pg_client_encoding,
    "pg_end_copy" => copy::pg_end_copy,
    "pg_put_line" => copy::pg_put_line,
    "pg_copy_to" => copy::pg_copy_to,
    "pg_copy_from" => copy::pg_copy_from,
    "pg_escape_string" => copy::pg_escape_string,
    "pg_escape_bytea" => copy::pg_escape_bytea,
    "pg_unescape_bytea" => copy::pg_unescape_bytea,
    "pg_escape_literal" => copy::pg_escape_literal,
    "pg_escape_identifier" => copy::pg_escape_identifier,
    "pg_result_error" => fetch::pg_result_error,
    "pg_result_error_field" => fetch::pg_result_error_field,
    "pg_connection_status" => link::pg_connection_status,
    "pg_transaction_status" => link::pg_transaction_status,
    "pg_connection_reset" => link::pg_connection_reset,
    "pg_cancel_query" => link::pg_cancel_query,
    "pg_connection_busy" => link::pg_connection_busy,
    "pg_send_query" => link::pg_send_query,
    "pg_send_query_params" => link::pg_send_query_params,
    "pg_send_prepare" => link::pg_send_prepare,
    "pg_send_execute" => link::pg_send_execute,
    "pg_get_result" => link::pg_get_result,
    "pg_result_status" => fetch::pg_result_status,
    "pg_get_notify" => link::pg_get_notify,
    "pg_get_pid" => link::pg_get_pid,
    "pg_socket" => link::pg_socket,
    "pg_consume_input" => link::pg_consume_input,
    "pg_flush" => link::pg_flush,
    "pg_meta_data" => meta::pg_meta_data,
    "pg_convert" => meta::pg_convert,
    "pg_insert" => meta::pg_insert,
    "pg_update" => meta::pg_update,
    "pg_delete" => meta::pg_delete,
    "pg_select" => meta::pg_select,
    "pg_set_error_context_visibility" => link::pg_set_error_context_visibility,
    "pg_result_memory_size" => fetch::pg_result_memory_size,
    "pg_change_password" => link::pg_change_password,
    "pg_put_copy_data" => copy::pg_put_copy_data,
    "pg_put_copy_end" => copy::pg_put_copy_end,
    "pg_socket_poll" => link::pg_socket_poll,
    "pg_set_chunked_rows_size" => link::pg_set_chunked_rows_size,
    "pg_close_stmt" => link::pg_close_stmt,
}

/// Functions whose argument count errors are their own
fn own_arity(name: &str) -> bool {
    matches!(
        name,
        "pg_lo_unlink"
            | "pg_lounlink"
            | "pg_lo_open"
            | "pg_loopen"
            | "pg_lo_import"
            | "pg_loimport"
            | "pg_lo_export"
            | "pg_loexport"
    )
}

/// PHP_MINFO(pgsql)
fn minfo(vm: &mut Vm) -> String {
    let (np, nl) = handles::PGG.with(|g| {
        let g = g.borrow();
        (g.num_persistent, g.num_links)
    });
    let mut out = String::new();
    out.push_str("\npgsql\n\n");
    out.push_str("PostgreSQL Support => enabled\n");
    out.push_str(&format!(
        "PostgreSQL (libpq) Version => {}\n",
        libpq_version()
    ));
    out.push_str("Multibyte character support => enabled\n");
    out.push_str(&format!("Active Persistent Links => {}\n", np));
    out.push_str(&format!("Active Links => {}\n", nl));
    out.push_str("\nDirective => Local Value => Master Value\n");
    for (name, _, _) in INI {
        let raw = vm
            .constants
            .get(name.as_bytes())
            .map(|v| v.to_php_string().to_string_lossy())
            .unwrap_or_default();
        let shown = match *name {
            "pgsql.max_persistent" | "pgsql.max_links" if raw == "-1" => "Unlimited".to_string(),
            "pgsql.max_persistent" | "pgsql.max_links" => raw,
            _ => {
                let on = matches!(
                    raw.to_ascii_lowercase().as_str(),
                    "1" | "on" | "yes" | "true"
                );
                if on { "On" } else { "Off" }.to_string()
            }
        };
        out.push_str(&format!("{} => {} => {}\n", name, shown, shown));
    }
    out
}

fn ini_set_hook(_vm: &mut Vm, key: &[u8], _value: &Value) -> bool {
    match INI.iter().find(|(n, _, _)| n.as_bytes() == key) {
        Some((_, _, changeable)) => *changeable,
        None => true,
    }
}

/// `new PgSql\Connection` & co.: the get_constructor handlers
fn ctor_call(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    _called: &[u8],
    _this: Option<&Value>,
    _args: &[Value],
) -> Value {
    if method.eq_ignore_ascii_case(b"__construct") {
        let msg = match class {
            b"pgsql\\connection" => {
                "Cannot directly construct PgSql\\Connection, use pg_connect() or pg_pconnect() instead"
            }
            b"pgsql\\result" => {
                "Cannot directly construct PgSql\\Result, use a dedicated function instead"
            }
            _ => "Cannot directly construct PgSql\\Lob, use pg_lo_open() instead",
        };
        zpp::throw(vm, b"Error", msg);
    }
    Value::Null
}

/// PHP_MINIT(pgsql)
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    vm.phpinfo_hooks.push(minfo);
    for class in [
        handles::CONNECTION_CLASS,
        handles::RESULT_CLASS,
        handles::LOB_CLASS,
    ] {
        let mut ce = match goro_core::stubs::class_entry(vm, class.as_bytes()) {
            Some(stub) => stub.clone(),
            None => ClassEntry::new(class.as_bytes().to_vec()),
        };
        ce.is_final = true;
        let lower = class.as_bytes().to_ascii_lowercase();
        vm.classes.insert(lower.clone(), ce);
        vm.native_class_handlers.insert(
            lower,
            goro_core::vm::NativeClassHandler {
                has_method: |_c, m, _s| m.eq_ignore_ascii_case(b"__construct"),
                call: ctor_call,
                prop_added: None,
                prop_write: None,
                compare_check: None,
                prop_read: None,
                param_names: None,
            },
        );
    }
    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        if !own_arity(name) {
            vm.zpp_extension_verified
                .insert(name.as_bytes().to_vec(), goro_core::arginfo::CHECK_ARITY);
        }
    }
    let ver = Value::String(PhpString::from_string(libpq_version()));
    vm.constants
        .insert(b"PGSQL_LIBPQ_VERSION".to_vec(), ver.clone());
    vm.constants
        .insert(b"PGSQL_LIBPQ_VERSION_STR".to_vec(), ver);
    vm.internal_deprecated_constants.insert(
        b"PGSQL_LIBPQ_VERSION_STR".to_vec(),
        "Constant PGSQL_LIBPQ_VERSION_STR is deprecated since 8.0, as it is the same as PGSQL_LIBPQ_VERSION".to_string(),
    );
    for &(name, v) in LONG_CONSTANTS {
        vm.constants
            .insert(name.as_bytes().to_vec(), Value::Long(v));
    }
    let mut names: Vec<Vec<u8>> = vec![
        b"PGSQL_LIBPQ_VERSION".to_vec(),
        b"PGSQL_LIBPQ_VERSION_STR".to_vec(),
    ];
    names.extend(LONG_CONSTANTS.iter().map(|(n, _)| n.as_bytes().to_vec()));
    vm.module_constants.push((NAME.to_vec(), names));
    for (name, def, _) in INI {
        vm.constants
            .entry(name.as_bytes().to_vec())
            .or_insert_with(|| Value::String(PhpString::from_bytes(def.as_bytes())));
        vm.ini_set_hooks
            .insert(name.as_bytes().to_vec(), ini_set_hook);
    }
    vm.shutdown_hooks.push(link::rshutdown);
}
