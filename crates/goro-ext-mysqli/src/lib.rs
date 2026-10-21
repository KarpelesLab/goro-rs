use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, PhpObject, PropertyDef, Visibility};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use mysql_async::prelude::*;

// ── Constants ──────────────────────────────────────────────────────────

const MYSQLI_ASSOC: i64 = 1;
const MYSQLI_NUM: i64 = 2;
const MYSQLI_BOTH: i64 = 3;

// ── Tokio runtime (created lazily, thread-local) ──────────────────────

thread_local! {
    static RUNTIME: RefCell<Option<tokio::runtime::Runtime>> = const { RefCell::new(None) };
}

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    RUNTIME.with(|rt| {
        let mut rt_ref = rt.borrow_mut();
        if rt_ref.is_none() {
            *rt_ref = Some(
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("failed to create tokio runtime for mysqli"),
            );
        }
        rt_ref.as_ref().unwrap().block_on(fut)
    })
}

// ── Connection storage ────────────────────────────────────────────────

struct MysqliConnection {
    /// Original opts — used by the pool / get_conn() and to re-establish a
    /// connection if `conn` has been disconnected.
    opts: mysql_async::Opts,
    /// Sticky session connection. Lazily set on first use; kept across calls
    /// so SET commands (sql_mode, transactions, user vars) persist between
    /// queries. Behind RefCell so we can mutate during block_on without
    /// holding the CONNECTIONS HashMap borrow.
    conn: Rc<RefCell<Option<mysql_async::Conn>>>,
    last_error: String,
    last_errno: i64,
    affected_rows: i64,
    insert_id: i64,
    #[allow(dead_code)]
    server_info: String,
    #[allow(dead_code)]
    autocommit: bool,
}

thread_local! {
    static CONNECTIONS: RefCell<HashMap<u64, MysqliConnection>> = RefCell::new(HashMap::new());
    static LAST_CONNECT_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
    static LAST_CONNECT_ERRNO: RefCell<i64> = const { RefCell::new(0) };
}

// ── Result storage ────────────────────────────────────────────────────

/// Per-column metadata captured from mysql_async for type-aware bind_result
/// conversion. We store one entry per column in column order.
#[derive(Clone, Debug)]
struct ColumnMeta {
    #[allow(dead_code)]
    name: String,
    /// MySQL column type code (1=TINY, 2=SHORT, 3=LONG, 4=FLOAT, 5=DOUBLE,
    /// 7=TIMESTAMP, 8=LONGLONG, 9=INT24, 10=DATE, 11=TIME, 12=DATETIME,
    /// 13=YEAR, 246=DECIMAL/NEWDECIMAL, 252=BLOB family, 253=VAR_STRING,
    /// 254=STRING).
    mysql_type: u8,
    /// True if the column is unsigned (UNSIGNED_FLAG bit).
    #[allow(dead_code)]
    unsigned: bool,
    /// Decimal precision (fractional digits for DATETIME / TIMESTAMP / TIME,
    /// number of decimal places for DECIMAL / FLOAT / DOUBLE). Drives how
    /// many trailing fractional digits we render for DATETIME column types.
    decimals: u8,
}

struct MysqliResult {
    rows: Vec<Vec<(String, Option<String>)>>,
    current_row: usize,
    num_rows: usize,
    /// Optional per-column metadata. Present when produced by exec_iter
    /// (prepared statement path), absent for synthetic results.
    columns: Vec<ColumnMeta>,
    /// Raw row values (one entry per row, each a Vec of mysql_async::Value
    /// matching `columns`). Populated for prepared-statement results so
    /// bind_result / fetch can return typed PHP values. Empty for legacy
    /// string-only paths.
    raw_rows: Vec<Vec<mysql_async::Value>>,
}

thread_local! {
    static RESULTS: RefCell<HashMap<u64, MysqliResult>> = RefCell::new(HashMap::new());
}

// ── Prepared statement storage ────────────────────────────────────────

struct MysqliStmt {
    conn_id: u64,
    query: String,
    /// Type string from bind_param (e.g. "iisb"). Used at execute() time to
    /// coerce each live value to the target mysql_async::Value variant.
    param_types: Option<String>,
    /// Smart-pointer view of each bound parameter — the actual PHP variable
    /// that should be re-read at execute() time. For "i"/"d"/"s" types this
    /// is read each execute. For "b" (blob) we also append data buffered
    /// via send_long_data.
    param_refs: Vec<Rc<RefCell<Value>>>,
    /// Per-parameter blob accumulator used by send_long_data. Indexed by
    /// param position (0-based). Only populated for "b" type params.
    param_blob_buffers: Vec<Vec<u8>>,
    /// References bound for result columns by bind_result. fetch() writes
    /// each row's columns into these (in order) via *r.borrow_mut() = val.
    bound_result_refs: Vec<Rc<RefCell<Value>>>,
    result_id: Option<u64>,
    #[allow(dead_code)]
    last_error: String,
    #[allow(dead_code)]
    last_errno: i64,
    #[allow(dead_code)]
    affected_rows: i64,
    #[allow(dead_code)]
    insert_id: i64,
}

thread_local! {
    static STMTS: RefCell<HashMap<u64, MysqliStmt>> = RefCell::new(HashMap::new());
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Retrieve the sticky session info (opts + sticky conn handle) for a
/// connection id. The returned `Rc<RefCell<Option<Conn>>>` is shared with
/// the stored `MysqliConnection`, so taking/restoring the connection in
/// an async block keeps it persistent across mysqli calls.
fn get_session(conn_id: u64) -> Option<(mysql_async::Opts, Rc<RefCell<Option<mysql_async::Conn>>>)> {
    CONNECTIONS.with(|c| {
        c.borrow().get(&conn_id).map(|conn| (conn.opts.clone(), conn.conn.clone()))
    })
}

/// Acquire (or re-establish) the sticky mysql_async::Conn behind a
/// session handle. Must be invoked inside `block_on(async { ... })`.
async fn take_conn(opts: &mysql_async::Opts, holder: &Rc<RefCell<Option<mysql_async::Conn>>>)
    -> Result<mysql_async::Conn, mysql_async::Error>
{
    if let Some(c) = holder.borrow_mut().take() {
        return Ok(c);
    }
    mysql_async::Conn::new(opts.clone()).await
}

/// Restore the connection to the holder so the next mysqli call reuses it.
fn restore_conn(holder: &Rc<RefCell<Option<mysql_async::Conn>>>, conn: mysql_async::Conn) {
    *holder.borrow_mut() = Some(conn);
}

fn get_object_id(val: &Value) -> Option<u64> {
    if let Value::Object(obj) = val {
        Some(obj.borrow().object_id)
    } else {
        None
    }
}

fn mysql_error_code(e: &mysql_async::Error) -> i64 {
    if let mysql_async::Error::Server(se) = e {
        se.code as i64
    } else {
        2002
    }
}

fn opt_str_to_value(val: &Option<String>) -> Value {
    match val {
        Some(s) => Value::String(PhpString::from_string(s.clone())),
        None => Value::Null,
    }
}

/// Safely convert a mysql_async::Value to Option<String>.
/// Handles NULL, integers, doubles, dates, bytes (potentially non-utf8) etc.
/// Uses lossy utf8 conversion for non-string types and arbitrary bytes.
fn mysql_value_to_opt_string(val: &mysql_async::Value) -> Option<String> {
    use mysql_async::Value as MV;
    match val {
        MV::NULL => None,
        MV::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
        MV::Int(i) => Some(i.to_string()),
        MV::UInt(u) => Some(u.to_string()),
        MV::Float(f) => Some(f.to_string()),
        MV::Double(d) => {
            // PHP-style double formatting: don't print trailing zeros.
            // Use a default that won't lose precision badly.
            if d.fract() == 0.0 && d.abs() < 1e16 {
                Some(format!("{}", *d as i64))
            } else {
                Some(format!("{}", d))
            }
        }
        MV::Date(y, m, d, h, mi, s, us) => {
            if *us != 0 {
                Some(format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}", y, m, d, h, mi, s, us))
            } else if *h != 0 || *mi != 0 || *s != 0 {
                Some(format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, h, mi, s))
            } else {
                Some(format!("{:04}-{:02}-{:02}", y, m, d))
            }
        }
        MV::Time(neg, days, h, m, s, us) => {
            let total_h = (*days as u64) * 24 + *h as u64;
            let sign = if *neg { "-" } else { "" };
            if *us != 0 {
                Some(format!("{}{:02}:{:02}:{:02}.{:06}", sign, total_h, m, s, us))
            } else {
                Some(format!("{}{:02}:{:02}:{:02}", sign, total_h, m, s))
            }
        }
    }
}

/// Extract a row's columns as Vec of (name, Option<String>) values.
/// Safe replacement for `row.get::<Option<String>, _>(i)` which panics on
/// type mismatch.
fn row_to_string_columns(row: &mysql_async::Row, col_names: &[String]) -> Vec<(String, Option<String>)> {
    let mut rd = Vec::with_capacity(col_names.len());
    for (i, name) in col_names.iter().enumerate() {
        let v = row.as_ref(i).map(mysql_value_to_opt_string).unwrap_or(None);
        rd.push((name.clone(), v));
    }
    rd
}

type RowData = Vec<Vec<(String, Option<String>)>>;
type RawRows = Vec<Vec<mysql_async::Value>>;
/// (Some(QueryRows), affected, insert_id) or None for non-result queries.
/// QueryRows = (rows-as-strings, num_rows, columns, raw_rows)
type QueryRows = (RowData, usize, Vec<ColumnMeta>, RawRows);
type QueryResult = Result<(Option<QueryRows>, u64, u64), mysql_async::Error>;

/// Build per-column metadata from mysql_async columns.
fn build_column_meta(columns: &[mysql_async::Column]) -> Vec<ColumnMeta> {
    use mysql_async::consts::ColumnFlags;
    columns.iter().map(|c| ColumnMeta {
        name: c.name_str().to_string(),
        mysql_type: c.column_type() as u8,
        unsigned: c.flags().contains(ColumnFlags::UNSIGNED_FLAG),
        decimals: c.decimals(),
    }).collect()
}

/// Convert a 4-byte MySQL FLOAT into a PHP double using the same "%.6G"-style
/// rounding that mysqlnd / libmysql apply. Raw `f32 as f64` would propagate
/// IEEE-754 noise (e.g. 5.1f32 expands to 5.099999904632568f64). PHP instead
/// truncates the value to 6 significant decimal digits — enough to uniquely
/// represent any f32 to display precision — so `float(5.1)` parses back to
/// 5.1f64. This matches PHP test expectations exactly.
fn f32_to_php_double(f: f32) -> f64 {
    if !f.is_finite() {
        return f as f64;
    }
    if f == 0.0 {
        return 0.0;
    }
    // C printf "%.6G" prints 6 significant digits, switching to %e form when
    // the exponent is < -4 or >= precision. Mirror that here.
    let abs = f.abs() as f64;
    let exp = abs.log10().floor() as i32;
    let use_scientific = !(-4..6).contains(&exp);
    let s = if use_scientific {
        // %.5e is 6 significant digits with one before the dot.
        format!("{:.5e}", f as f64)
    } else {
        // Plain decimal with 6 significant digits.
        let digits_after_point = (5 - exp).max(0) as usize;
        format!("{:.*}", digits_after_point, f as f64)
    };
    s.parse::<f64>().unwrap_or(f as f64)
}

/// Format the fractional-second portion of a datetime / time value following
/// the column's `decimals` field. PHP renders the trailing dot only when the
/// column actually declares a fractional precision (e.g. DATETIME(4) →
/// ".NNNN"). When `decimals` is 0 we omit the suffix entirely.
fn format_fractional(us: u32, decimals: u8) -> String {
    if decimals == 0 { return String::new(); }
    let decimals = decimals.min(6) as usize;
    // us has 6 digits resolution. Right-truncate to `decimals` digits.
    let scale = 10u32.pow((6 - decimals) as u32);
    let truncated = us / scale;
    format!(".{:0width$}", truncated, width = decimals)
}

/// Convert a raw column value to a PHP `Value`, respecting the column's type.
/// Used by mysqli_stmt_fetch / bind_result to produce native int/float when the
/// column is numeric, NULL for NULL, and string otherwise (with PHP-style
/// datetime / time formatting).
fn mysql_value_to_php(val: &mysql_async::Value, meta: &ColumnMeta) -> Value {
    use mysql_async::Value as MV;
    // MySQL field type codes for numeric columns.
    const TINY: u8 = 1; const SHORT: u8 = 2; const LONG: u8 = 3; const FLOAT: u8 = 4;
    const DOUBLE: u8 = 5; const TIMESTAMP: u8 = 7; const LONGLONG: u8 = 8; const INT24: u8 = 9;
    const DATETIME: u8 = 12; const YEAR: u8 = 13;
    // YEAR is a special integer column type in MySQL but PHP returns it as
    // a string in textual form (e.g. "2010") to match libmysql / mysqlnd.
    let is_int_type = matches!(meta.mysql_type, TINY | SHORT | LONG | LONGLONG | INT24);
    let is_float_type = matches!(meta.mysql_type, FLOAT | DOUBLE);
    let is_year_type = meta.mysql_type == YEAR;
    let is_dt_with_time = matches!(meta.mysql_type, DATETIME | TIMESTAMP);
    match val {
        MV::NULL => Value::Null,
        // YEAR: render as 4-character string.
        _ if is_year_type => {
            let n = match val {
                MV::Int(i) => *i,
                MV::UInt(u) => *u as i64,
                MV::Bytes(b) => std::str::from_utf8(b).ok()
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0),
                _ => 0,
            };
            Value::String(PhpString::from_string(format!("{:04}", n)))
        }
        MV::Int(i) if is_int_type => Value::Long(*i),
        MV::UInt(u) if is_int_type => Value::Long(*u as i64),
        MV::Int(i) => Value::Long(*i),
        MV::UInt(u) => Value::Long(*u as i64),
        MV::Float(f) if is_float_type => Value::Double(f32_to_php_double(*f)),
        MV::Double(d) if is_float_type => Value::Double(*d),
        MV::Float(f) => Value::Double(f32_to_php_double(*f)),
        MV::Double(d) => Value::Double(*d),
        MV::Bytes(b) => {
            // Numeric columns may arrive as text (textual protocol). Coerce
            // back to int / float.
            if is_int_type {
                if let Ok(s) = std::str::from_utf8(b) {
                    if let Ok(n) = s.parse::<i64>() { return Value::Long(n); }
                }
            }
            if is_float_type {
                if let Ok(s) = std::str::from_utf8(b) {
                    if let Ok(f) = s.parse::<f64>() { return Value::Double(f); }
                }
            }
            // DECIMAL / strings: keep as string. (PHP returns DECIMAL as a
            // string to preserve precision.)
            Value::String(PhpString::from_bytes(b))
        }
        MV::Date(y, m, d, h, mi, s, us) => {
            let frac = format_fractional(*us, meta.decimals);
            let formatted = if is_dt_with_time {
                // DATETIME and TIMESTAMP always show HH:MM:SS, regardless of
                // whether the time portion is zero.
                format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}{}", y, m, d, h, mi, s, frac)
            } else if *h != 0 || *mi != 0 || *s != 0 || *us != 0 {
                format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}{}", y, m, d, h, mi, s, frac)
            } else {
                format!("{:04}-{:02}-{:02}", y, m, d)
            };
            Value::String(PhpString::from_string(formatted))
        }
        MV::Time(neg, days, h, m, s, us) => {
            let total_h = (*days as u64) * 24 + *h as u64;
            let sign = if *neg { "-" } else { "" };
            let frac = format_fractional(*us, meta.decimals);
            let formatted = format!("{}{:02}:{:02}:{:02}{}", sign, total_h, m, s, frac);
            Value::String(PhpString::from_string(formatted))
        }
    }
}

// ── Registration ──────────────────────────────────────────────────────

pub fn register(vm: &mut Vm) {
    vm.register_extension(b"mysqli");
    register_classes(vm);
    vm.register_function(b"mysqli_connect", mysqli_connect);
    vm.register_function(b"mysqli_real_connect", mysqli_real_connect);
    vm.register_function(b"mysqli_close", mysqli_close);
    vm.register_function(b"mysqli_query", mysqli_query);
    vm.register_function(b"mysqli_real_query", mysqli_real_query);
    vm.register_function(b"mysqli_fetch_assoc", mysqli_fetch_assoc);
    vm.register_function(b"mysqli_fetch_array", mysqli_fetch_array);
    vm.register_function(b"mysqli_fetch_row", mysqli_fetch_row);
    vm.register_function(b"mysqli_fetch_all", mysqli_fetch_all);
    vm.register_function(b"mysqli_fetch_object", mysqli_fetch_object);
    vm.register_function(b"mysqli_num_rows", mysqli_num_rows);
    vm.register_function(b"mysqli_affected_rows", mysqli_affected_rows);
    vm.register_function(b"mysqli_insert_id", mysqli_insert_id);
    vm.register_function(b"mysqli_error", mysqli_error);
    vm.register_function(b"mysqli_errno", mysqli_errno);
    vm.register_function(b"mysqli_real_escape_string", mysqli_real_escape_string);
    vm.register_function(b"mysqli_escape_string", mysqli_real_escape_string);
    vm.register_function(b"mysqli_prepare", mysqli_prepare);
    vm.register_function(b"mysqli_stmt_bind_param", mysqli_stmt_bind_param);
    vm.register_function(b"mysqli_stmt_execute", mysqli_stmt_execute);
    vm.register_function(b"mysqli_stmt_get_result", mysqli_stmt_get_result);
    vm.register_function(b"mysqli_stmt_close", mysqli_stmt_close);
    vm.register_function(b"mysqli_select_db", mysqli_select_db);
    vm.register_function(b"mysqli_ping", mysqli_ping);
    vm.register_function(b"mysqli_set_charset", mysqli_set_charset);
    vm.register_function(b"mysqli_begin_transaction", mysqli_begin_transaction);
    vm.register_function(b"mysqli_commit", mysqli_commit);
    vm.register_function(b"mysqli_rollback", mysqli_rollback);
    vm.register_function(b"mysqli_autocommit", mysqli_autocommit);
    vm.register_function(b"mysqli_free_result", mysqli_free_result);
    vm.register_function(b"mysqli_connect_error", mysqli_connect_error);
    vm.register_function(b"mysqli_connect_errno", mysqli_connect_errno);
    vm.register_function(b"mysqli_multi_query", mysqli_multi_query);
    vm.register_function(b"mysqli_next_result", mysqli_next_result);
    vm.register_function(b"mysqli_store_result", mysqli_store_result);
    vm.register_function(b"mysqli_more_results", mysqli_more_results);
    vm.register_function(b"mysqli_field_count", mysqli_field_count);
    vm.register_function(b"mysqli_character_set_name", mysqli_character_set_name);
    vm.register_function(b"mysqli_report", mysqli_report_fn);
    vm.register_function(b"mysqli_init", mysqli_init_fn);
    vm.register_function(b"mysqli_get_client_info", mysqli_get_client_info_fn);
    vm.register_function(b"mysqli_get_server_version", mysqli_get_server_version_fn);
    vm.register_function(b"mysqli_get_host_info", mysqli_get_host_info_fn);
    vm.register_function(b"mysqli_get_proto_info", mysqli_get_proto_info_fn);
    vm.register_function(b"mysqli_get_server_info", mysqli_get_server_info_fn);
    vm.register_function(b"mysqli_get_client_version", mysqli_get_client_version_fn);
    vm.register_function(b"mysqli_thread_id", mysqli_thread_id_fn);
    vm.register_function(b"mysqli_thread_safe", mysqli_thread_safe_fn);
    vm.register_function(b"mysqli_warning_count", mysqli_warning_count_fn);
    vm.register_function(b"mysqli_sqlstate", mysqli_sqlstate_fn);
    vm.register_function(b"mysqli_data_seek", mysqli_data_seek_fn);
    vm.register_function(b"mysqli_change_user", mysqli_change_user_fn);
    vm.register_function(b"mysqli_options", mysqli_options_fn);
    vm.register_function(b"mysqli_get_charset", mysqli_get_charset_fn);
    vm.register_function(b"mysqli_kill", mysqli_kill_fn);
    vm.register_function(b"mysqli_stat", mysqli_stat_fn);
    vm.register_function(b"mysqli_dump_debug_info", mysqli_dump_debug_info_fn);
    vm.register_function(b"mysqli_debug", mysqli_debug_fn);
    vm.register_function(b"mysqli_refresh", mysqli_refresh_fn);
    vm.register_function(b"mysqli_savepoint", mysqli_savepoint_fn);
    vm.register_function(b"mysqli_release_savepoint", mysqli_release_savepoint_fn);
    vm.register_function(b"mysqli_fetch_field", mysqli_fetch_field_fn);
    vm.register_function(b"mysqli_fetch_fields", mysqli_fetch_fields_fn);
    vm.register_function(b"mysqli_fetch_field_direct", mysqli_fetch_field_direct_fn);
    vm.register_function(b"mysqli_field_seek", mysqli_field_seek_fn);
    vm.register_function(b"mysqli_field_tell", mysqli_field_tell_fn);
    vm.register_function(b"mysqli_num_fields", mysqli_num_fields_fn);
    vm.register_function(b"mysqli_fetch_lengths", mysqli_fetch_lengths_fn);
    vm.register_function(b"mysqli_fetch_column", mysqli_fetch_column_fn);
    vm.register_function(b"mysqli_stmt_init", mysqli_stmt_init_fn);
    vm.register_function(b"mysqli_stmt_prepare", mysqli_stmt_prepare_fn);
    vm.register_function(b"mysqli_stmt_affected_rows", mysqli_stmt_affected_rows_fn);
    vm.register_function(b"mysqli_stmt_num_rows", mysqli_stmt_num_rows_fn);
    vm.register_function(b"mysqli_stmt_field_count", mysqli_stmt_field_count_fn);
    vm.register_function(b"mysqli_stmt_param_count", mysqli_stmt_param_count_fn);
    vm.register_function(b"mysqli_stmt_insert_id", mysqli_stmt_insert_id_fn);
    vm.register_function(b"mysqli_stmt_errno", mysqli_stmt_errno_fn);
    vm.register_function(b"mysqli_stmt_error", mysqli_stmt_error_fn);
    vm.register_function(b"mysqli_stmt_sqlstate", mysqli_stmt_sqlstate_fn);
    vm.register_function(b"mysqli_stmt_free_result", mysqli_stmt_free_result_fn);
    vm.register_function(b"mysqli_stmt_reset", mysqli_stmt_reset_fn);
    vm.register_function(b"mysqli_stmt_store_result", mysqli_stmt_store_result_fn);
    vm.register_function(b"mysqli_stmt_fetch", mysqli_stmt_fetch_fn);
    vm.register_function(b"mysqli_stmt_bind_result", mysqli_stmt_bind_result_fn);
    vm.register_function(b"mysqli_stmt_attr_set", mysqli_stmt_attr_set_fn);
    vm.register_function(b"mysqli_stmt_attr_get", mysqli_stmt_attr_get_fn);
    vm.register_function(b"mysqli_stmt_data_seek", mysqli_stmt_data_seek_fn);
    vm.register_function(b"mysqli_stmt_send_long_data", mysqli_stmt_send_long_data_fn);
    vm.register_function(b"mysqli_stmt_result_metadata", mysqli_stmt_result_metadata_fn);
    vm.register_function(b"mysqli_stmt_get_warnings", mysqli_stmt_get_warnings_fn);
    vm.register_function(b"mysqli_get_warnings", mysqli_get_warnings_fn);
    vm.register_function(b"mysqli_execute_query", mysqli_execute_query_fn);
    vm.register_function(b"mysqli_set_local_infile_handler", mysqli_set_local_infile_handler_fn);
    vm.register_function(b"mysqli_set_local_infile_default", mysqli_set_local_infile_default_fn);
    vm.register_function(b"mysqli_error_list", mysqli_error_list_fn);
    vm.register_function(b"mysqli_stmt_more_results", mysqli_stmt_more_results_fn);
    vm.register_function(b"mysqli_stmt_next_result", mysqli_stmt_next_result_fn);

    vm.constants.insert(b"MYSQLI_ASSOC".to_vec(), Value::Long(MYSQLI_ASSOC));
    vm.constants.insert(b"MYSQLI_NUM".to_vec(), Value::Long(MYSQLI_NUM));
    vm.constants.insert(b"MYSQLI_BOTH".to_vec(), Value::Long(MYSQLI_BOTH));
    vm.constants.insert(b"MYSQLI_STORE_RESULT".to_vec(), Value::Long(0));
    vm.constants.insert(b"MYSQLI_USE_RESULT".to_vec(), Value::Long(1));
    vm.constants.insert(b"MYSQLI_CLIENT_COMPRESS".to_vec(), Value::Long(32));
    vm.constants.insert(b"MYSQLI_CLIENT_SSL".to_vec(), Value::Long(2048));
    vm.constants.insert(b"MYSQLI_TRANS_START_READ_ONLY".to_vec(), Value::Long(4));
    vm.constants.insert(b"MYSQLI_TRANS_START_READ_WRITE".to_vec(), Value::Long(2));
    vm.constants.insert(b"MYSQLI_TRANS_START_WITH_CONSISTENT_SNAPSHOT".to_vec(), Value::Long(1));
    vm.constants.insert(b"MYSQLI_REPORT_OFF".to_vec(), Value::Long(0));
    vm.constants.insert(b"MYSQLI_REPORT_ERROR".to_vec(), Value::Long(1));
    vm.constants.insert(b"MYSQLI_REPORT_STRICT".to_vec(), Value::Long(2));
    vm.constants.insert(b"MYSQLI_REPORT_INDEX".to_vec(), Value::Long(4));
    vm.constants.insert(b"MYSQLI_REPORT_ALL".to_vec(), Value::Long(255));
}

// ── mysqli_connect ────────────────────────────────────────────────────

fn mysqli_connect(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let host = args.first()
        .filter(|v| !matches!(v, Value::Null | Value::Undef))
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let username = args.get(1)
        .filter(|v| !matches!(v, Value::Null | Value::Undef))
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "root".to_string());
    let password = args.get(2)
        .filter(|v| !matches!(v, Value::Null | Value::Undef))
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let database = args.get(3)
        .filter(|v| !matches!(v, Value::Null | Value::Undef))
        .map(|v| v.to_php_string().to_string_lossy());
    let port = args.get(4)
        .filter(|v| !matches!(v, Value::Null | Value::Undef))
        .map(|v| v.to_long() as u16)
        .unwrap_or(3306);

    let actual_host = if host.starts_with("p:") { &host[2..] } else { &host };

    let mut builder = mysql_async::OptsBuilder::default()
        .ip_or_hostname(actual_host)
        .tcp_port(port)
        .user(Some(&username))
        .pass(Some(&password));
    if let Some(ref db) = database {
        builder = builder.db_name(Some(db));
    }

    let opts: mysql_async::Opts = builder.into();

    let conn_result = block_on(async {
        let conn = mysql_async::Conn::new(opts.clone()).await?;
        let sv = conn.server_version();
        let info = format!("{}.{}.{}", sv.0, sv.1, sv.2);
        Ok::<(mysql_async::Conn, String), mysql_async::Error>((conn, info))
    });

    match conn_result {
        Ok((conn, server_info)) => {
            LAST_CONNECT_ERROR.with(|e| *e.borrow_mut() = String::new());
            LAST_CONNECT_ERRNO.with(|e| *e.borrow_mut() = 0);
            let id = vm.next_object_id();
            let obj = PhpObject::new(b"mysqli".to_vec(), id);
            CONNECTIONS.with(|c| c.borrow_mut().insert(id, MysqliConnection {
                opts,
                conn: Rc::new(RefCell::new(Some(conn))),
                last_error: String::new(), last_errno: 0,
                affected_rows: 0, insert_id: 0, server_info, autocommit: true,
            }));
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }
        Err(e) => {
            let msg = e.to_string();
            let errno = mysql_error_code(&e);
            LAST_CONNECT_ERROR.with(|err| *err.borrow_mut() = msg);
            LAST_CONNECT_ERRNO.with(|err| *err.borrow_mut() = errno);
            Ok(Value::False)
        }
    }
}

// ── mysqli_close ──────────────────────────────────────────────────────

fn mysqli_close(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let removed = CONNECTIONS.with(|c| c.borrow_mut().remove(&id));
    if let Some(conn) = removed {
        if let Some(c) = conn.conn.borrow_mut().take() {
            block_on(async { c.disconnect().await.ok(); });
        }
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}

// ── exec_query helper ─────────────────────────────────────────────────

fn exec_query(opts: &mysql_async::Opts, holder: &Rc<RefCell<Option<mysql_async::Conn>>>, query_str: &str) -> QueryResult {
    let opts = opts.clone();
    let holder = holder.clone();
    block_on(async move {
        let mut conn = take_conn(&opts, &holder).await?;
        let result = async {
            let mut qr = conn.query_iter(query_str).await?;
            let columns_raw: Vec<mysql_async::Column> = qr.columns_ref().to_vec();
            if columns_raw.is_empty() {
                let affected = qr.affected_rows();
                let insert_id = qr.last_insert_id().unwrap_or(0);
                drop(qr);
                return Ok((None, affected, insert_id));
            }
            let col_meta = build_column_meta(&columns_raw);
            let col_names: Vec<String> = columns_raw.iter().map(|c| c.name_str().to_string()).collect();
            let raw_rows_async: Vec<mysql_async::Row> = qr.collect().await?;
            let mut rows = Vec::with_capacity(raw_rows_async.len());
            let mut raw_rows: Vec<Vec<mysql_async::Value>> = Vec::with_capacity(raw_rows_async.len());
            for row in &raw_rows_async {
                rows.push(row_to_string_columns(row, &col_names));
                let mut rr: Vec<mysql_async::Value> = Vec::with_capacity(col_meta.len());
                for i in 0..col_meta.len() {
                    rr.push(row.as_ref(i).cloned().unwrap_or(mysql_async::Value::NULL));
                }
                raw_rows.push(rr);
            }
            let num_rows = rows.len();
            Ok::<_, mysql_async::Error>((Some((rows, num_rows, col_meta, raw_rows)), 0u64, 0u64))
        }.await;
        restore_conn(&holder, conn);
        result
    })
}

// ── run_sql helper ────────────────────────────────────────────────────

fn run_sql(id: u64, sql: &str) -> bool {
    let session = get_session(id);
    let (opts, holder) = match session { Some(s) => s, None => return false };
    let sql = sql.to_string();
    let result: Result<(), mysql_async::Error> = block_on(async move {
        let mut conn = take_conn(&opts, &holder).await?;
        let res = conn.query_drop(&sql).await;
        restore_conn(&holder, conn);
        res
    });
    match result {
        Ok(()) => {
            CONNECTIONS.with(|c| {
                if let Some(conn) = c.borrow_mut().get_mut(&id) {
                    conn.last_error.clear();
                    conn.last_errno = 0;
                }
            });
            true
        }
        Err(e) => {
            let msg = e.to_string();
            let errno = mysql_error_code(&e);
            CONNECTIONS.with(|c| {
                if let Some(conn) = c.borrow_mut().get_mut(&id) {
                    conn.last_error = msg;
                    conn.last_errno = errno;
                }
            });
            false
        }
    }
}

// ── mysqli_query ──────────────────────────────────────────────────────

fn mysqli_query(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let query_str = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    let session = get_session(id);
    let (opts, holder) = match session { Some(s) => s, None => return Ok(Value::False) };

    match exec_query(&opts, &holder, &query_str) {
        Ok((Some((rows, num_rows, columns, raw_rows)), _, _)) => {
            let res_id = vm.next_object_id();
            let res_obj = PhpObject::new(b"mysqli_result".to_vec(), res_id);
            RESULTS.with(|r| r.borrow_mut().insert(res_id, MysqliResult { rows, current_row: 0, num_rows, columns, raw_rows }));
            CONNECTIONS.with(|c| { if let Some(conn) = c.borrow_mut().get_mut(&id) { conn.last_error.clear(); conn.last_errno = 0; } });
            Ok(Value::Object(Rc::new(RefCell::new(res_obj))))
        }
        Ok((None, affected, insert_id)) => {
            CONNECTIONS.with(|c| {
                if let Some(conn) = c.borrow_mut().get_mut(&id) {
                    conn.affected_rows = affected as i64;
                    conn.insert_id = insert_id as i64;
                    conn.last_error.clear();
                    conn.last_errno = 0;
                }
            });
            Ok(Value::True)
        }
        Err(e) => {
            let msg = e.to_string();
            let errno = mysql_error_code(&e);
            CONNECTIONS.with(|c| { if let Some(conn) = c.borrow_mut().get_mut(&id) { conn.last_error = msg; conn.last_errno = errno; } });
            Ok(Value::False)
        }
    }
}

// ── mysqli_real_query ─────────────────────────────────────────────────

fn mysqli_real_query(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let query_str = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    let session = get_session(id);
    let (opts, holder) = match session { Some(s) => s, None => return Ok(Value::False) };

    let result: Result<(u64, u64), mysql_async::Error> = block_on(async move {
        let mut conn = take_conn(&opts, &holder).await?;
        let res = async {
            conn.query_drop(&query_str).await?;
            let affected = conn.affected_rows();
            let insert_id = conn.last_insert_id().unwrap_or(0);
            Ok::<_, mysql_async::Error>((affected, insert_id))
        }.await;
        restore_conn(&holder, conn);
        res
    });

    match result {
        Ok((affected, insert_id)) => {
            CONNECTIONS.with(|c| {
                if let Some(conn) = c.borrow_mut().get_mut(&id) {
                    conn.affected_rows = affected as i64;
                    conn.insert_id = insert_id as i64;
                    conn.last_error.clear();
                    conn.last_errno = 0;
                }
            });
            Ok(Value::True)
        }
        Err(e) => {
            let msg = e.to_string();
            let errno = mysql_error_code(&e);
            CONNECTIONS.with(|c| { if let Some(conn) = c.borrow_mut().get_mut(&id) { conn.last_error = msg; conn.last_errno = errno; } });
            Ok(Value::False)
        }
    }
}

// ── fetch functions ───────────────────────────────────────────────────

fn mysqli_fetch_assoc(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    RESULTS.with(|r| {
        let mut results = r.borrow_mut();
        let result = match results.get_mut(&id) { Some(r) => r, None => return Ok(Value::Null) };
        if result.current_row >= result.num_rows { return Ok(Value::Null); }
        let row = &result.rows[result.current_row];
        result.current_row += 1;
        let mut arr = PhpArray::new();
        for (name, val) in row {
            arr.set(ArrayKey::String(PhpString::from_string(name.clone())), opt_str_to_value(val));
        }
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    })
}

fn mysqli_fetch_array(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let mode = args.get(1).filter(|v| !matches!(v, Value::Null | Value::Undef)).map(|v| v.to_long()).unwrap_or(MYSQLI_BOTH);
    RESULTS.with(|r| {
        let mut results = r.borrow_mut();
        let result = match results.get_mut(&id) { Some(r) => r, None => return Ok(Value::Null) };
        if result.current_row >= result.num_rows { return Ok(Value::Null); }
        let row = &result.rows[result.current_row];
        result.current_row += 1;
        let mut arr = PhpArray::new();
        for (i, (name, val)) in row.iter().enumerate() {
            let php_val = opt_str_to_value(val);
            if mode & MYSQLI_NUM != 0 { arr.set(ArrayKey::Int(i as i64), php_val.clone()); }
            if mode & MYSQLI_ASSOC != 0 { arr.set(ArrayKey::String(PhpString::from_string(name.clone())), php_val); }
        }
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    })
}

fn mysqli_fetch_row(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    RESULTS.with(|r| {
        let mut results = r.borrow_mut();
        let result = match results.get_mut(&id) { Some(r) => r, None => return Ok(Value::Null) };
        if result.current_row >= result.num_rows { return Ok(Value::Null); }
        let row = &result.rows[result.current_row];
        result.current_row += 1;
        let mut arr = PhpArray::new();
        for (i, (_, val)) in row.iter().enumerate() {
            arr.set(ArrayKey::Int(i as i64), opt_str_to_value(val));
        }
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    })
}

fn mysqli_fetch_all(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let mode = args.get(1).filter(|v| !matches!(v, Value::Null | Value::Undef)).map(|v| v.to_long()).unwrap_or(MYSQLI_NUM);
    RESULTS.with(|r| {
        let mut results = r.borrow_mut();
        let result = match results.get_mut(&id) {
            Some(r) => r,
            None => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
        };
        let mut outer = PhpArray::new();
        for (idx, row) in result.rows.iter().enumerate() {
            let mut inner = PhpArray::new();
            for (i, (name, val)) in row.iter().enumerate() {
                let php_val = opt_str_to_value(val);
                if mode & MYSQLI_NUM != 0 { inner.set(ArrayKey::Int(i as i64), php_val.clone()); }
                if mode & MYSQLI_ASSOC != 0 { inner.set(ArrayKey::String(PhpString::from_string(name.clone())), php_val); }
            }
            outer.set(ArrayKey::Int(idx as i64), Value::Array(Rc::new(RefCell::new(inner))));
        }
        result.current_row = result.num_rows;
        Ok(Value::Array(Rc::new(RefCell::new(outer))))
    })
}

fn mysqli_fetch_object(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    RESULTS.with(|r| {
        let mut results = r.borrow_mut();
        let result = match results.get_mut(&id) { Some(r) => r, None => return Ok(Value::Null) };
        if result.current_row >= result.num_rows { return Ok(Value::Null); }
        let row = &result.rows[result.current_row];
        result.current_row += 1;
        let obj_id = vm.next_object_id();
        let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
        for (name, val) in row {
            obj.set_property(name.as_bytes().to_vec(), opt_str_to_value(val));
        }
        Ok(Value::Object(Rc::new(RefCell::new(obj))))
    })
}

// ── simple getters ────────────────────────────────────────────────────

fn mysqli_num_rows(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    RESULTS.with(|r| {
        Ok(Value::Long(r.borrow().get(&id).map(|res| res.num_rows as i64).unwrap_or(0)))
    })
}

fn mysqli_affected_rows(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    CONNECTIONS.with(|c| {
        Ok(Value::Long(c.borrow().get(&id).map(|conn| conn.affected_rows).unwrap_or(-1)))
    })
}

fn mysqli_insert_id(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    CONNECTIONS.with(|c| {
        Ok(Value::Long(c.borrow().get(&id).map(|conn| conn.insert_id).unwrap_or(0)))
    })
}

fn mysqli_error(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    CONNECTIONS.with(|c| {
        Ok(Value::String(PhpString::from_string(
            c.borrow().get(&id).map(|conn| conn.last_error.clone()).unwrap_or_default()
        )))
    })
}

fn mysqli_errno(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    CONNECTIONS.with(|c| {
        Ok(Value::Long(c.borrow().get(&id).map(|conn| conn.last_errno).unwrap_or(0)))
    })
}

// ── mysqli_real_escape_string ─────────────────────────────────────────

fn mysqli_real_escape_string(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    let mut escaped = String::with_capacity(input.len() * 2);
    for ch in input.chars() {
        match ch {
            '\0' => escaped.push_str("\\0"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\\' => escaped.push_str("\\\\"),
            '\'' => escaped.push_str("\\'"),
            '"' => escaped.push_str("\\\""),
            '\x1a' => escaped.push_str("\\Z"),
            _ => escaped.push(ch),
        }
    }
    Ok(Value::String(PhpString::from_string(escaped)))
}

// ── prepared statements ───────────────────────────────────────────────

fn mysqli_prepare(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let conn_id = args.first().and_then(get_object_id).unwrap_or(0);
    let query = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    let has_conn = CONNECTIONS.with(|c| c.borrow().contains_key(&conn_id));
    if !has_conn { return Ok(Value::False); }
    let stmt_id = vm.next_object_id();
    let stmt_obj = PhpObject::new(b"mysqli_stmt".to_vec(), stmt_id);
    STMTS.with(|s| s.borrow_mut().insert(stmt_id, MysqliStmt {
        conn_id, query, param_types: None,
        param_refs: Vec::new(),
        param_blob_buffers: Vec::new(),
        bound_result_refs: Vec::new(),
        result_id: None, last_error: String::new(), last_errno: 0,
        affected_rows: 0, insert_id: 0,
    }));
    Ok(Value::Object(Rc::new(RefCell::new(stmt_obj))))
}

/// Extract a Rc<RefCell<Value>> reference handle from a parameter. If the
/// arg is already a Value::Reference, share it. Otherwise create a fresh
/// reference cell seeded with the current value (the original variable
/// is no longer linked, but the call still functions for the by-value
/// fall-through case).
fn extract_or_new_ref(val: &Value) -> Rc<RefCell<Value>> {
    if let Value::Reference(r) = val {
        r.clone()
    } else {
        Rc::new(RefCell::new(val.clone()))
    }
}

fn mysqli_stmt_bind_param(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    let types = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    // Each subsequent arg should be a Value::Reference; capture the cell so we
    // can read live values back at execute() time. send_long_data later may
    // populate per-param blob buffers.
    let refs: Vec<Rc<RefCell<Value>>> = args.iter().skip(2).map(extract_or_new_ref).collect();
    let n = refs.len();
    STMTS.with(|s| {
        let mut stmts = s.borrow_mut();
        if let Some(stmt) = stmts.get_mut(&stmt_id) {
            stmt.param_types = Some(types);
            stmt.param_refs = refs;
            stmt.param_blob_buffers = vec![Vec::new(); n];
            Ok(Value::True)
        } else {
            Ok(Value::False)
        }
    })
}

/// Coerce a PHP value to a mysql_async::Value following the given type char.
/// Type chars: 'i' = integer, 'd' = double, 's' = string, 'b' = blob.
fn php_to_mysql_param(val: &Value, type_ch: u8, blob_buf: &[u8]) -> mysql_async::Value {
    // Deref reference once for inspection.
    let inner: Value = if let Value::Reference(r) = val { r.borrow().clone() } else { val.clone() };
    match type_ch {
        b'i' => match &inner {
            Value::Null => mysql_async::Value::NULL,
            Value::True => mysql_async::Value::Int(1),
            Value::False => mysql_async::Value::Int(0),
            Value::Long(n) => mysql_async::Value::Int(*n),
            Value::Double(f) => mysql_async::Value::Int(*f as i64),
            other => {
                let s = other.to_php_string().to_string_lossy();
                s.parse::<i64>().map(mysql_async::Value::Int)
                    .unwrap_or(mysql_async::Value::Int(0))
            }
        },
        b'd' => match &inner {
            Value::Null => mysql_async::Value::NULL,
            Value::Long(n) => mysql_async::Value::Double(*n as f64),
            Value::Double(f) => mysql_async::Value::Double(*f),
            Value::True => mysql_async::Value::Double(1.0),
            Value::False => mysql_async::Value::Double(0.0),
            other => {
                let s = other.to_php_string().to_string_lossy();
                s.parse::<f64>().map(mysql_async::Value::Double)
                    .unwrap_or(mysql_async::Value::Double(0.0))
            }
        },
        b'b' => {
            // If send_long_data buffered bytes for this param, use that;
            // otherwise treat like 's' (and let the bound variable's value
            // fill in when send_long_data is not used).
            if !blob_buf.is_empty() {
                return mysql_async::Value::Bytes(blob_buf.to_vec());
            }
            match &inner {
                Value::Null => mysql_async::Value::NULL,
                Value::String(s) => mysql_async::Value::Bytes(s.as_bytes().to_vec()),
                other => mysql_async::Value::Bytes(other.to_php_string().as_bytes().to_vec()),
            }
        }
        _ => match &inner {
            // 's' (string) — also the fallback.
            Value::Null => mysql_async::Value::NULL,
            Value::String(s) => mysql_async::Value::Bytes(s.as_bytes().to_vec()),
            Value::Long(n) => mysql_async::Value::Bytes(n.to_string().into_bytes()),
            Value::Double(f) => mysql_async::Value::Bytes(f.to_string().into_bytes()),
            Value::True => mysql_async::Value::Bytes(b"1".to_vec()),
            Value::False => mysql_async::Value::Bytes(Vec::new()),
            other => mysql_async::Value::Bytes(other.to_php_string().as_bytes().to_vec()),
        }
    }
}

fn mysqli_stmt_execute(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    // Capture (conn_id, query, types, refs, blob_buffers) without holding the
    // STMTS borrow across the blocking I/O call.
    let stmt_info = STMTS.with(|s| {
        s.borrow().get(&stmt_id).map(|st| (
            st.conn_id,
            st.query.clone(),
            st.param_types.clone().unwrap_or_default(),
            st.param_refs.clone(),
            st.param_blob_buffers.clone(),
        ))
    });
    let (conn_id, query, types_str, param_refs, blob_buffers) = match stmt_info {
        Some(i) => i, None => return Ok(Value::False)
    };
    let session = get_session(conn_id);
    let (opts, holder) = match session { Some(s) => s, None => return Ok(Value::False) };

    // Build mysql params from the live values now (not from cached values
    // captured at bind_param time).
    let mut mysql_params: Vec<mysql_async::Value> = Vec::with_capacity(param_refs.len());
    let types_bytes = types_str.as_bytes();
    for (i, r) in param_refs.iter().enumerate() {
        let t = types_bytes.get(i).copied().unwrap_or(b's');
        let live = Value::Reference(r.clone());
        let empty_vec = Vec::new();
        let buf: &[u8] = blob_buffers.get(i).map(|b| b.as_slice()).unwrap_or(empty_vec.as_slice());
        mysql_params.push(php_to_mysql_param(&live, t, buf));
    }

    let result: QueryResult = block_on(async move {
        let mut conn = take_conn(&opts, &holder).await?;
        let res = async {
            let stmt = conn.prep(&query).await?;
            let mut qr = conn.exec_iter(&stmt, mysql_params).await?;
            let columns_raw: Vec<mysql_async::Column> = qr.columns_ref().to_vec();
            if columns_raw.is_empty() {
                let affected = qr.affected_rows();
                let insert_id = qr.last_insert_id().unwrap_or(0);
                drop(qr);
                conn.close(stmt).await?;
                return Ok((None, affected, insert_id));
            }
            let col_meta = build_column_meta(&columns_raw);
            let col_names: Vec<String> = columns_raw.iter().map(|c| c.name_str().to_string()).collect();
            let raw_rows_async: Vec<mysql_async::Row> = qr.collect().await?;
            let mut rows = Vec::with_capacity(raw_rows_async.len());
            let mut raw_rows: Vec<Vec<mysql_async::Value>> = Vec::with_capacity(raw_rows_async.len());
            for row in &raw_rows_async {
                rows.push(row_to_string_columns(row, &col_names));
                let mut rr: Vec<mysql_async::Value> = Vec::with_capacity(col_meta.len());
                for i in 0..col_meta.len() {
                    rr.push(row.as_ref(i).cloned().unwrap_or(mysql_async::Value::NULL));
                }
                raw_rows.push(rr);
            }
            let num_rows = rows.len();
            conn.close(stmt).await?;
            Ok::<_, mysql_async::Error>((Some((rows, num_rows, col_meta, raw_rows)), 0u64, 0u64))
        }.await;
        restore_conn(&holder, conn);
        res
    });

    match result {
        Ok((rows_opt, affected, insert_id)) => {
            STMTS.with(|s| {
                if let Some(stmt) = s.borrow_mut().get_mut(&stmt_id) {
                    stmt.affected_rows = affected as i64;
                    stmt.insert_id = insert_id as i64;
                    stmt.last_error.clear();
                    stmt.last_errno = 0;
                    if let Some((rows, num_rows, columns, raw_rows)) = rows_opt {
                        let temp_key = stmt_id | 0x8000_0000_0000_0000;
                        RESULTS.with(|r| r.borrow_mut().insert(temp_key, MysqliResult {
                            rows, current_row: 0, num_rows, columns, raw_rows,
                        }));
                    }
                }
            });
            // Clear blob buffers after execute (matches PHP libmysql behaviour:
            // long_data is consumed by a single execute and must be re-sent).
            STMTS.with(|s| {
                if let Some(stmt) = s.borrow_mut().get_mut(&stmt_id) {
                    for buf in stmt.param_blob_buffers.iter_mut() { buf.clear(); }
                }
            });
            CONNECTIONS.with(|c| {
                if let Some(conn) = c.borrow_mut().get_mut(&conn_id) {
                    conn.affected_rows = affected as i64;
                    conn.insert_id = insert_id as i64;
                    conn.last_error.clear();
                    conn.last_errno = 0;
                }
            });
            Ok(Value::True)
        }
        Err(e) => {
            let msg = e.to_string();
            let errno = mysql_error_code(&e);
            STMTS.with(|s| { if let Some(stmt) = s.borrow_mut().get_mut(&stmt_id) { stmt.last_error = msg.clone(); stmt.last_errno = errno; } });
            CONNECTIONS.with(|c| { if let Some(conn) = c.borrow_mut().get_mut(&conn_id) { conn.last_error = msg; conn.last_errno = errno; } });
            Ok(Value::False)
        }
    }
}

fn mysqli_stmt_get_result(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    let temp_key = stmt_id | 0x8000_0000_0000_0000;
    let stored = RESULTS.with(|r| r.borrow_mut().remove(&temp_key));
    match stored {
        Some(result) => {
            let res_id = vm.next_object_id();
            let res_obj = PhpObject::new(b"mysqli_result".to_vec(), res_id);
            RESULTS.with(|r| r.borrow_mut().insert(res_id, result));
            STMTS.with(|s| { if let Some(stmt) = s.borrow_mut().get_mut(&stmt_id) { stmt.result_id = Some(res_id); } });
            Ok(Value::Object(Rc::new(RefCell::new(res_obj))))
        }
        None => Ok(Value::False),
    }
}

fn mysqli_stmt_close(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    let removed = STMTS.with(|s| s.borrow_mut().remove(&stmt_id));
    if let Some(stmt) = removed {
        if let Some(res_id) = stmt.result_id { RESULTS.with(|r| r.borrow_mut().remove(&res_id)); }
        RESULTS.with(|r| r.borrow_mut().remove(&(stmt_id | 0x8000_0000_0000_0000)));
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}

// ── connection operations ─────────────────────────────────────────────

fn mysqli_select_db(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let db = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    if run_sql(id, &format!("USE `{}`", db.replace('`', "``"))) { Ok(Value::True) } else { Ok(Value::False) }
}

fn mysqli_ping(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let session = get_session(id);
    let (opts, holder) = match session { Some(s) => s, None => return Ok(Value::False) };
    let result: Result<(), mysql_async::Error> = block_on(async move {
        let mut conn = take_conn(&opts, &holder).await?;
        let res = conn.ping().await;
        restore_conn(&holder, conn);
        res
    });
    if result.is_ok() { Ok(Value::True) } else { Ok(Value::False) }
}

fn mysqli_set_charset(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let charset = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_else(|| "utf8".to_string());
    if run_sql(id, &format!("SET NAMES '{}'", charset.replace('\'', "''"))) { Ok(Value::True) } else { Ok(Value::False) }
}

fn mysqli_begin_transaction(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    if run_sql(id, "START TRANSACTION") { Ok(Value::True) } else { Ok(Value::False) }
}

fn mysqli_commit(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    if run_sql(id, "COMMIT") { Ok(Value::True) } else { Ok(Value::False) }
}

fn mysqli_rollback(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    if run_sql(id, "ROLLBACK") { Ok(Value::True) } else { Ok(Value::False) }
}

fn mysqli_autocommit(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let enable = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
    let sql = if enable { "SET autocommit = 1" } else { "SET autocommit = 0" };
    if run_sql(id, sql) {
        CONNECTIONS.with(|c| { if let Some(conn) = c.borrow_mut().get_mut(&id) { conn.autocommit = enable; } });
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}

fn mysqli_free_result(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    RESULTS.with(|r| r.borrow_mut().remove(&id));
    Ok(Value::Null)
}

fn mysqli_connect_error(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    LAST_CONNECT_ERROR.with(|e| {
        let err = e.borrow();
        if err.is_empty() { Ok(Value::Null) } else { Ok(Value::String(PhpString::from_string(err.clone()))) }
    })
}

fn mysqli_connect_errno(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    LAST_CONNECT_ERRNO.with(|e| Ok(Value::Long(*e.borrow())))
}

fn mysqli_multi_query(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let query_str = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    if run_sql(id, &query_str) { Ok(Value::True) } else { Ok(Value::False) }
}

fn mysqli_next_result(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn mysqli_store_result(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }
fn mysqli_more_results(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> { Ok(Value::False) }

fn mysqli_field_count(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    RESULTS.with(|r| {
        Ok(Value::Long(r.borrow().get(&id).and_then(|res| res.rows.first()).map(|row| row.len() as i64).unwrap_or(0)))
    })
}

fn mysqli_character_set_name(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"utf8mb4")))
}

fn mysqli_report_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Stub - just return true
    Ok(Value::True)
}

fn mysqli_init_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // Return a mysqli object (similar to new mysqli but not connected)
    let id = vm.next_object_id();
    let mut obj = goro_core::object::PhpObject::new(b"mysqli".to_vec(), id);
    obj.set_property(b"__conn_id".to_vec(), Value::Long(-1));
    obj.set_property(b"connect_errno".to_vec(), Value::Long(0));
    obj.set_property(b"connect_error".to_vec(), Value::Null);
    Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(obj))))
}

fn mysqli_get_client_info_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"mysqlnd 8.5.4")))
}

fn mysqli_get_server_version_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(80100)) // 8.1.0
}

// ── Class registration ────────────────────────────────────────────────

fn register_classes(vm: &mut Vm) {
    // mysqli_driver — used by connect.inc to set report_mode.
    let mut driver = ClassEntry::new(b"mysqli_driver".to_vec());
    driver.properties.push(PropertyDef {
        name: b"client_info".to_vec(),
        default: Value::String(PhpString::from_bytes(b"mysqlnd 8.5.4")),
        is_static: false,
        is_readonly: false,
        visibility: Visibility::Public,
        set_visibility: None,
        declaring_class: b"mysqli_driver".to_vec(),
        property_type: None,
        has_get_hook: false,
        has_set_hook: false,
        is_virtual: false,
        is_abstract: false,
        is_final: false,
        doc_comment: None,
        attributes: Vec::new(),
    });
    driver.properties.push(PropertyDef {
        name: b"client_version".to_vec(),
        default: Value::Long(80500),
        is_static: false,
        is_readonly: false,
        visibility: Visibility::Public,
        set_visibility: None,
        declaring_class: b"mysqli_driver".to_vec(),
        property_type: None,
        has_get_hook: false,
        has_set_hook: false,
        is_virtual: false,
        is_abstract: false,
        is_final: false,
        doc_comment: None,
        attributes: Vec::new(),
    });
    driver.properties.push(PropertyDef {
        name: b"driver_version".to_vec(),
        default: Value::Long(101012),
        is_static: false,
        is_readonly: false,
        visibility: Visibility::Public,
        set_visibility: None,
        declaring_class: b"mysqli_driver".to_vec(),
        property_type: None,
        has_get_hook: false,
        has_set_hook: false,
        is_virtual: false,
        is_abstract: false,
        is_final: false,
        doc_comment: None,
        attributes: Vec::new(),
    });
    driver.properties.push(PropertyDef {
        name: b"report_mode".to_vec(),
        default: Value::Long(0),
        is_static: false,
        is_readonly: false,
        visibility: Visibility::Public,
        set_visibility: None,
        declaring_class: b"mysqli_driver".to_vec(),
        property_type: None,
        has_get_hook: false,
        has_set_hook: false,
        is_virtual: false,
        is_abstract: false,
        is_final: false,
        doc_comment: None,
        attributes: Vec::new(),
    });
    vm.classes.insert(b"mysqli_driver".to_vec(), driver);

    // mysqli class — stub with the commonly-accessed properties.
    let mut mysqli = ClassEntry::new(b"mysqli".to_vec());
    for (name, default) in [
        ("affected_rows", Value::Long(0)),
        ("client_info", Value::String(PhpString::from_bytes(b"mysqlnd 8.5.4"))),
        ("client_version", Value::Long(80500)),
        ("connect_errno", Value::Long(0)),
        ("connect_error", Value::Null),
        ("errno", Value::Long(0)),
        ("error", Value::String(PhpString::from_bytes(b""))),
        ("error_list", Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
        ("field_count", Value::Long(0)),
        ("host_info", Value::String(PhpString::from_bytes(b""))),
        ("info", Value::Null),
        ("insert_id", Value::Long(0)),
        ("server_info", Value::String(PhpString::from_bytes(b""))),
        ("server_version", Value::Long(0)),
        ("sqlstate", Value::String(PhpString::from_bytes(b"00000"))),
        ("protocol_version", Value::Long(10)),
        ("thread_id", Value::Long(0)),
        ("warning_count", Value::Long(0)),
    ] {
        mysqli.properties.push(PropertyDef {
            name: name.as_bytes().to_vec(),
            default,
            is_static: false,
            is_readonly: false,
            visibility: Visibility::Public,
            set_visibility: None,
            declaring_class: b"mysqli".to_vec(),
            property_type: None,
            has_get_hook: false,
            has_set_hook: false,
            is_virtual: false,
            is_abstract: false,
            is_final: false,
            doc_comment: None,
            attributes: Vec::new(),
        });
    }
    vm.classes.insert(b"mysqli".to_vec(), mysqli);

    // mysqli_result — stub.
    let mut mr = ClassEntry::new(b"mysqli_result".to_vec());
    for (name, default) in [
        ("current_field", Value::Long(0)),
        ("field_count", Value::Long(0)),
        ("lengths", Value::Null),
        ("num_rows", Value::Long(0)),
        ("type", Value::Long(0)),
    ] {
        mr.properties.push(PropertyDef {
            name: name.as_bytes().to_vec(),
            default,
            is_static: false,
            is_readonly: false,
            visibility: Visibility::Public,
            set_visibility: None,
            declaring_class: b"mysqli_result".to_vec(),
            property_type: None,
            has_get_hook: false,
            has_set_hook: false,
            is_virtual: false,
            is_abstract: false,
            is_final: false,
            doc_comment: None,
            attributes: Vec::new(),
        });
    }
    vm.classes.insert(b"mysqli_result".to_vec(), mr);

    // mysqli_stmt — stub.
    let mut ms = ClassEntry::new(b"mysqli_stmt".to_vec());
    for (name, default) in [
        ("affected_rows", Value::Long(0)),
        ("errno", Value::Long(0)),
        ("error", Value::String(PhpString::from_bytes(b""))),
        ("error_list", Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
        ("field_count", Value::Long(0)),
        ("insert_id", Value::Long(0)),
        ("num_rows", Value::Long(0)),
        ("param_count", Value::Long(0)),
        ("sqlstate", Value::String(PhpString::from_bytes(b"00000"))),
        ("id", Value::Long(0)),
    ] {
        ms.properties.push(PropertyDef {
            name: name.as_bytes().to_vec(),
            default,
            is_static: false,
            is_readonly: false,
            visibility: Visibility::Public,
            set_visibility: None,
            declaring_class: b"mysqli_stmt".to_vec(),
            property_type: None,
            has_get_hook: false,
            has_set_hook: false,
            is_virtual: false,
            is_abstract: false,
            is_final: false,
            doc_comment: None,
            attributes: Vec::new(),
        });
    }
    vm.classes.insert(b"mysqli_stmt".to_vec(), ms);

    // mysqli_warning — stub for properties accessed by tests.
    let mut mw = ClassEntry::new(b"mysqli_warning".to_vec());
    for (name, default) in [
        ("message", Value::String(PhpString::from_bytes(b""))),
        ("sqlstate", Value::String(PhpString::from_bytes(b"00000"))),
        ("errno", Value::Long(0)),
    ] {
        mw.properties.push(PropertyDef {
            name: name.as_bytes().to_vec(),
            default,
            is_static: false,
            is_readonly: false,
            visibility: Visibility::Public,
            set_visibility: None,
            declaring_class: b"mysqli_warning".to_vec(),
            property_type: None,
            has_get_hook: false,
            has_set_hook: false,
            is_virtual: false,
            is_abstract: false,
            is_final: false,
            doc_comment: None,
            attributes: Vec::new(),
        });
    }
    vm.classes.insert(b"mysqli_warning".to_vec(), mw);

    // mysqli_sql_exception extends RuntimeException — register as plain class.
    let mut se = ClassEntry::new(b"mysqli_sql_exception".to_vec());
    se.parent = Some(b"RuntimeException".to_vec());
    vm.classes.insert(b"mysqli_sql_exception".to_vec(), se);
}

// ── mysqli_real_connect ───────────────────────────────────────────────

fn mysqli_real_connect(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Signature: mysqli_real_connect(mysqli $mysqli, ?string $hostname = null,
    //   ?string $username = null, ?string $password = null, ?string $database = null,
    //   ?int $port = null, ?string $socket = null, int $flags = 0): bool
    let target_obj = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(Value::False),
    };

    let host = args.get(1)
        .filter(|v| !matches!(v, Value::Null | Value::Undef))
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let username = args.get(2)
        .filter(|v| !matches!(v, Value::Null | Value::Undef))
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "root".to_string());
    let password = args.get(3)
        .filter(|v| !matches!(v, Value::Null | Value::Undef))
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let database = args.get(4)
        .filter(|v| !matches!(v, Value::Null | Value::Undef))
        .map(|v| v.to_php_string().to_string_lossy())
        .filter(|s| !s.is_empty());
    let port = args.get(5)
        .filter(|v| !matches!(v, Value::Null | Value::Undef))
        .map(|v| v.to_long() as u16)
        .unwrap_or(3306);
    let _socket = args.get(6)
        .filter(|v| !matches!(v, Value::Null | Value::Undef))
        .map(|v| v.to_php_string().to_string_lossy());
    let _flags = args.get(7).map(|v| v.to_long()).unwrap_or(0);

    let actual_host = if host.starts_with("p:") { &host[2..] } else { &host };
    let mut builder = mysql_async::OptsBuilder::default()
        .ip_or_hostname(actual_host)
        .tcp_port(port)
        .user(Some(&username))
        .pass(Some(&password));
    if let Some(ref db) = database {
        builder = builder.db_name(Some(db));
    }

    let opts: mysql_async::Opts = builder.into();
    let conn_result = block_on(async {
        let conn = mysql_async::Conn::new(opts.clone()).await?;
        let sv = conn.server_version();
        let info = format!("{}.{}.{}", sv.0, sv.1, sv.2);
        Ok::<(mysql_async::Conn, String), mysql_async::Error>((conn, info))
    });

    match conn_result {
        Ok((conn, server_info)) => {
            LAST_CONNECT_ERROR.with(|e| *e.borrow_mut() = String::new());
            LAST_CONNECT_ERRNO.with(|e| *e.borrow_mut() = 0);
            // Reuse the existing object id so $mysqli stays as-is.
            let id = target_obj.borrow().object_id;
            CONNECTIONS.with(|c| c.borrow_mut().insert(id, MysqliConnection {
                opts,
                conn: Rc::new(RefCell::new(Some(conn))),
                last_error: String::new(), last_errno: 0,
                affected_rows: 0, insert_id: 0, server_info, autocommit: true,
            }));
            // Populate visible properties on the object.
            {
                let mut ob = target_obj.borrow_mut();
                ob.set_property(b"connect_errno".to_vec(), Value::Long(0));
                ob.set_property(b"connect_error".to_vec(), Value::Null);
                ob.set_property(b"errno".to_vec(), Value::Long(0));
                ob.set_property(b"error".to_vec(), Value::String(PhpString::from_bytes(b"")));
            }
            Ok(Value::True)
        }
        Err(e) => {
            let msg = e.to_string();
            let errno = mysql_error_code(&e);
            LAST_CONNECT_ERROR.with(|err| *err.borrow_mut() = msg.clone());
            LAST_CONNECT_ERRNO.with(|err| *err.borrow_mut() = errno);
            let mut ob = target_obj.borrow_mut();
            ob.set_property(b"connect_errno".to_vec(), Value::Long(errno));
            ob.set_property(b"connect_error".to_vec(), Value::String(PhpString::from_string(msg)));
            Ok(Value::False)
        }
    }
}

// ── Stub functions for compatibility ────────────────────────────────────

fn mysqli_get_host_info_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let info = CONNECTIONS.with(|c| {
        c.borrow().get(&id).map(|_| "localhost via TCP/IP".to_string())
    }).unwrap_or_default();
    Ok(Value::String(PhpString::from_string(info)))
}

fn mysqli_get_proto_info_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(10))
}

fn mysqli_get_server_info_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let info = CONNECTIONS.with(|c| {
        c.borrow().get(&id).map(|conn| conn.server_info.clone())
    }).unwrap_or_else(|| "8.0.43".to_string());
    Ok(Value::String(PhpString::from_string(info)))
}

fn mysqli_get_client_version_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(80500))
}

fn mysqli_thread_id_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(1))
}

fn mysqli_thread_safe_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn mysqli_warning_count_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn mysqli_sqlstate_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"00000")))
}

fn mysqli_data_seek_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let offset = args.get(1).map(|v| v.to_long()).unwrap_or(0) as usize;
    RESULTS.with(|r| {
        if let Some(res) = r.borrow_mut().get_mut(&id) {
            if offset <= res.num_rows {
                res.current_row = offset;
                return Ok(Value::True);
            }
        }
        Ok(Value::False)
    })
}

fn mysqli_change_user_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn mysqli_options_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn mysqli_get_charset_fn(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(b"stdClass".to_vec(), id);
    obj.set_property(b"charset".to_vec(), Value::String(PhpString::from_bytes(b"utf8mb4")));
    obj.set_property(b"collation".to_vec(), Value::String(PhpString::from_bytes(b"utf8mb4_general_ci")));
    obj.set_property(b"dir".to_vec(), Value::String(PhpString::from_bytes(b"")));
    obj.set_property(b"min_length".to_vec(), Value::Long(1));
    obj.set_property(b"max_length".to_vec(), Value::Long(4));
    obj.set_property(b"number".to_vec(), Value::Long(45));
    obj.set_property(b"state".to_vec(), Value::Long(1));
    obj.set_property(b"comment".to_vec(), Value::String(PhpString::from_bytes(b"UTF-8 Unicode")));
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

fn mysqli_kill_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn mysqli_stat_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"Uptime: 0 Threads: 1 Questions: 0")))
}

fn mysqli_dump_debug_info_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn mysqli_debug_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn mysqli_refresh_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn mysqli_savepoint_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let name = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    if run_sql(id, &format!("SAVEPOINT `{}`", name.replace('`', "``"))) { Ok(Value::True) } else { Ok(Value::False) }
}

fn mysqli_release_savepoint_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let name = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    if run_sql(id, &format!("RELEASE SAVEPOINT `{}`", name.replace('`', "``"))) { Ok(Value::True) } else { Ok(Value::False) }
}

fn mysqli_fetch_field_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let (field_name, idx_now) = RESULTS.with(|r| {
        let mut results = r.borrow_mut();
        let result = match results.get_mut(&id) { Some(r) => r, None => return (None, 0usize) };
        // Use field_seek index stored somewhere - simplistic approach using current_field
        let row = result.rows.first();
        let idx = result.current_row.min(result.num_rows); // not quite right but a fallback
        let name = row.and_then(|r| r.get(idx).map(|(n, _)| n.clone()));
        (name, idx)
    });
    match field_name {
        Some(name) => {
            let obj_id = vm.next_object_id();
            let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
            obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(name)));
            obj.set_property(b"orgname".to_vec(), Value::String(PhpString::from_bytes(b"")));
            obj.set_property(b"table".to_vec(), Value::String(PhpString::from_bytes(b"")));
            obj.set_property(b"orgtable".to_vec(), Value::String(PhpString::from_bytes(b"")));
            obj.set_property(b"max_length".to_vec(), Value::Long(0));
            obj.set_property(b"length".to_vec(), Value::Long(255));
            obj.set_property(b"charsetnr".to_vec(), Value::Long(45));
            obj.set_property(b"flags".to_vec(), Value::Long(0));
            obj.set_property(b"type".to_vec(), Value::Long(253));
            obj.set_property(b"decimals".to_vec(), Value::Long(0));
            obj.set_property(b"db".to_vec(), Value::String(PhpString::from_bytes(b"")));
            obj.set_property(b"def".to_vec(), Value::String(PhpString::from_bytes(b"")));
            let _ = idx_now;
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }
        None => Ok(Value::False),
    }
}

fn mysqli_fetch_fields_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let names = RESULTS.with(|r| {
        r.borrow().get(&id).and_then(|res| res.rows.first()).map(|row| row.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()).unwrap_or_default()
    });
    let mut arr = PhpArray::new();
    for (i, name) in names.iter().enumerate() {
        let obj_id = vm.next_object_id();
        let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
        obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(name.clone())));
        obj.set_property(b"orgname".to_vec(), Value::String(PhpString::from_bytes(b"")));
        obj.set_property(b"table".to_vec(), Value::String(PhpString::from_bytes(b"")));
        obj.set_property(b"orgtable".to_vec(), Value::String(PhpString::from_bytes(b"")));
        obj.set_property(b"max_length".to_vec(), Value::Long(0));
        obj.set_property(b"length".to_vec(), Value::Long(255));
        obj.set_property(b"charsetnr".to_vec(), Value::Long(45));
        obj.set_property(b"flags".to_vec(), Value::Long(0));
        obj.set_property(b"type".to_vec(), Value::Long(253));
        obj.set_property(b"decimals".to_vec(), Value::Long(0));
        obj.set_property(b"db".to_vec(), Value::String(PhpString::from_bytes(b"")));
        obj.set_property(b"def".to_vec(), Value::String(PhpString::from_bytes(b"")));
        arr.set(ArrayKey::Int(i as i64), Value::Object(Rc::new(RefCell::new(obj))));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn mysqli_fetch_field_direct_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let idx = args.get(1).map(|v| v.to_long()).unwrap_or(0) as usize;
    let name = RESULTS.with(|r| {
        r.borrow().get(&id).and_then(|res| res.rows.first()).and_then(|row| row.get(idx).map(|(n, _)| n.clone()))
    });
    match name {
        Some(n) => {
            let obj_id = vm.next_object_id();
            let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
            obj.set_property(b"name".to_vec(), Value::String(PhpString::from_string(n)));
            obj.set_property(b"orgname".to_vec(), Value::String(PhpString::from_bytes(b"")));
            obj.set_property(b"table".to_vec(), Value::String(PhpString::from_bytes(b"")));
            obj.set_property(b"orgtable".to_vec(), Value::String(PhpString::from_bytes(b"")));
            obj.set_property(b"max_length".to_vec(), Value::Long(0));
            obj.set_property(b"length".to_vec(), Value::Long(255));
            obj.set_property(b"charsetnr".to_vec(), Value::Long(45));
            obj.set_property(b"flags".to_vec(), Value::Long(0));
            obj.set_property(b"type".to_vec(), Value::Long(253));
            obj.set_property(b"decimals".to_vec(), Value::Long(0));
            obj.set_property(b"db".to_vec(), Value::String(PhpString::from_bytes(b"")));
            obj.set_property(b"def".to_vec(), Value::String(PhpString::from_bytes(b"")));
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }
        None => Ok(Value::False),
    }
}

fn mysqli_field_seek_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn mysqli_field_tell_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn mysqli_num_fields_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    RESULTS.with(|r| {
        Ok(Value::Long(r.borrow().get(&id).and_then(|res| res.rows.first()).map(|row| row.len() as i64).unwrap_or(0)))
    })
}

fn mysqli_fetch_lengths_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    RESULTS.with(|r| {
        let results = r.borrow();
        let result = match results.get(&id) { Some(r) => r, None => return Ok(Value::False) };
        // Use the last fetched row (current_row - 1) lengths
        if result.current_row == 0 { return Ok(Value::False); }
        let idx = result.current_row - 1;
        if idx >= result.num_rows { return Ok(Value::False); }
        let row = &result.rows[idx];
        let mut arr = PhpArray::new();
        for (i, (_, v)) in row.iter().enumerate() {
            let len = v.as_ref().map(|s| s.len() as i64).unwrap_or(0);
            arr.set(ArrayKey::Int(i as i64), Value::Long(len));
        }
        Ok(Value::Array(Rc::new(RefCell::new(arr))))
    })
}

fn mysqli_fetch_column_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let col = args.get(1).map(|v| v.to_long()).unwrap_or(0) as usize;
    RESULTS.with(|r| {
        let mut results = r.borrow_mut();
        let result = match results.get_mut(&id) { Some(r) => r, None => return Ok(Value::False) };
        if result.current_row >= result.num_rows { return Ok(Value::False); }
        let row = &result.rows[result.current_row];
        result.current_row += 1;
        let val = row.get(col).map(|(_, v)| opt_str_to_value(v)).unwrap_or(Value::False);
        Ok(val)
    })
}

// ── mysqli_stmt functions ─────────────────────────────────────────────

fn mysqli_stmt_init_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let conn_id = args.first().and_then(get_object_id).unwrap_or(0);
    let stmt_id = vm.next_object_id();
    let stmt_obj = PhpObject::new(b"mysqli_stmt".to_vec(), stmt_id);
    STMTS.with(|s| s.borrow_mut().insert(stmt_id, MysqliStmt {
        conn_id, query: String::new(), param_types: None,
        param_refs: Vec::new(),
        param_blob_buffers: Vec::new(),
        bound_result_refs: Vec::new(),
        result_id: None, last_error: String::new(), last_errno: 0,
        affected_rows: 0, insert_id: 0,
    }));
    Ok(Value::Object(Rc::new(RefCell::new(stmt_obj))))
}

fn mysqli_stmt_prepare_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    let query = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    STMTS.with(|s| {
        if let Some(stmt) = s.borrow_mut().get_mut(&stmt_id) {
            stmt.query = query;
            Ok(Value::True)
        } else {
            Ok(Value::False)
        }
    })
}

fn mysqli_stmt_affected_rows_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    STMTS.with(|s| Ok(Value::Long(s.borrow().get(&stmt_id).map(|st| st.affected_rows).unwrap_or(-1))))
}

fn mysqli_stmt_num_rows_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    let temp_key = stmt_id | 0x8000_0000_0000_0000;
    RESULTS.with(|r| {
        let rs = r.borrow();
        let n = rs.get(&temp_key).map(|res| res.num_rows as i64)
            .or_else(|| {
                STMTS.with(|s| s.borrow().get(&stmt_id).and_then(|st| st.result_id)
                    .and_then(|rid| rs.get(&rid).map(|res| res.num_rows as i64)))
            })
            .unwrap_or(0);
        Ok(Value::Long(n))
    })
}

fn mysqli_stmt_field_count_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    let temp_key = stmt_id | 0x8000_0000_0000_0000;
    RESULTS.with(|r| {
        let rs = r.borrow();
        let n = rs.get(&temp_key).and_then(|res| res.rows.first()).map(|row| row.len() as i64).unwrap_or(0);
        Ok(Value::Long(n))
    })
}

fn mysqli_stmt_param_count_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    STMTS.with(|s| {
        let count = s.borrow().get(&stmt_id).map(|st| st.query.matches('?').count() as i64).unwrap_or(0);
        Ok(Value::Long(count))
    })
}

fn mysqli_stmt_insert_id_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    STMTS.with(|s| Ok(Value::Long(s.borrow().get(&stmt_id).map(|st| st.insert_id).unwrap_or(0))))
}

fn mysqli_stmt_errno_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    STMTS.with(|s| Ok(Value::Long(s.borrow().get(&stmt_id).map(|st| st.last_errno).unwrap_or(0))))
}

fn mysqli_stmt_error_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    STMTS.with(|s| Ok(Value::String(PhpString::from_string(
        s.borrow().get(&stmt_id).map(|st| st.last_error.clone()).unwrap_or_default()
    ))))
}

fn mysqli_stmt_sqlstate_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"00000")))
}

fn mysqli_stmt_free_result_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    let temp_key = stmt_id | 0x8000_0000_0000_0000;
    RESULTS.with(|r| { r.borrow_mut().remove(&temp_key); });
    Ok(Value::Null)
}

fn mysqli_stmt_reset_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn mysqli_stmt_store_result_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn mysqli_stmt_fetch_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Returns: TRUE for a fetched row, NULL when no more rows, FALSE on error.
    // If bind_result was called, populate the bound references with typed
    // PHP values matching the column metadata.
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    let temp_key = stmt_id | 0x8000_0000_0000_0000;
    // Take a snapshot of (raw row, columns) and the bound refs, then write
    // outside the STMTS / RESULTS borrows to avoid re-entrancy issues.
    let bound_refs: Vec<Rc<RefCell<Value>>> = STMTS.with(|s| {
        s.borrow().get(&stmt_id).map(|st| st.bound_result_refs.clone()).unwrap_or_default()
    });
    let snapshot: Option<(Vec<mysql_async::Value>, Vec<ColumnMeta>)> = RESULTS.with(|r| {
        let mut results = r.borrow_mut();
        let result = match results.get_mut(&temp_key) { Some(r) => r, None => return None };
        if result.current_row >= result.num_rows { return None; }
        let idx = result.current_row;
        result.current_row += 1;
        let raw = result.raw_rows.get(idx).cloned().unwrap_or_default();
        Some((raw, result.columns.clone()))
    });
    let (raw_row, cols) = match snapshot { Some(s) => s, None => return Ok(Value::Null) };
    if !bound_refs.is_empty() && !cols.is_empty() {
        for (i, r) in bound_refs.iter().enumerate() {
            let v = if let (Some(rv), Some(meta)) = (raw_row.get(i), cols.get(i)) {
                mysql_value_to_php(rv, meta)
            } else {
                Value::Null
            };
            *r.borrow_mut() = v;
        }
    }
    Ok(Value::True)
}

fn mysqli_stmt_bind_result_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    // Each subsequent arg should be a Value::Reference into the caller's
    // variable. Capture and store; fetch() writes typed values into these.
    let refs: Vec<Rc<RefCell<Value>>> = args.iter().skip(1).map(extract_or_new_ref).collect();
    STMTS.with(|s| {
        if let Some(stmt) = s.borrow_mut().get_mut(&stmt_id) {
            stmt.bound_result_refs = refs;
            Ok(Value::True)
        } else {
            Ok(Value::False)
        }
    })
}

fn mysqli_stmt_attr_set_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn mysqli_stmt_attr_get_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn mysqli_stmt_data_seek_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}

fn mysqli_stmt_send_long_data_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // mysqli_stmt_send_long_data(mysqli_stmt $stmt, int $param_num, string $data)
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    let param_num = args.get(1).map(|v| v.to_long() as usize).unwrap_or(0);
    let data = args.get(2).map(|v| v.to_php_string().as_bytes().to_vec()).unwrap_or_default();
    STMTS.with(|s| {
        let mut stmts = s.borrow_mut();
        if let Some(stmt) = stmts.get_mut(&stmt_id) {
            // Ensure the blob buffer has enough slots — bind_param resizes
            // to the number of params, but defensively grow here too.
            if param_num >= stmt.param_blob_buffers.len() {
                stmt.param_blob_buffers.resize(param_num + 1, Vec::new());
            }
            stmt.param_blob_buffers[param_num].extend_from_slice(&data);
            Ok(Value::True)
        } else {
            Ok(Value::False)
        }
    })
}

fn mysqli_stmt_result_metadata_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Return a mysqli_result-shaped object derived from the stmt's columns.
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    let temp_key = stmt_id | 0x8000_0000_0000_0000;
    let columns: Option<Vec<(String, Option<String>)>> = RESULTS.with(|r| {
        r.borrow().get(&temp_key).and_then(|res| res.rows.first().cloned())
    });
    match columns {
        Some(cols) => {
            let res_id = vm.next_object_id();
            let res_obj = PhpObject::new(b"mysqli_result".to_vec(), res_id);
            // Store a faux result with just one fake row representing the columns.
            RESULTS.with(|r| r.borrow_mut().insert(res_id, MysqliResult {
                rows: vec![cols], current_row: 0, num_rows: 0,
                columns: Vec::new(), raw_rows: Vec::new(),
            }));
            Ok(Value::Object(Rc::new(RefCell::new(res_obj))))
        }
        None => Ok(Value::False),
    }
}

fn mysqli_stmt_get_warnings_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn mysqli_get_warnings_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn mysqli_execute_query_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // mysqli_execute_query(mysqli $mysqli, string $query, ?array $params = null): mysqli_result|bool
    // Simple impl: substitute parameters and run query.
    let id = args.first().and_then(get_object_id).unwrap_or(0);
    let query = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    let params: Option<Vec<Value>> = args.get(2).and_then(|v| {
        if let Value::Array(arr) = v {
            Some(arr.borrow().iter().map(|(_, v)| v.clone()).collect())
        } else {
            None
        }
    });
    let session = get_session(id);
    let (opts, holder) = match session { Some(s) => s, None => return Ok(Value::False) };

    let result: QueryResult = block_on(async move {
        let mut conn = take_conn(&opts, &holder).await?;
        let res = async {
            let mp: Vec<mysql_async::Value> = if let Some(ps) = params {
                ps.iter().map(|v| match v {
                    Value::Null => mysql_async::Value::NULL,
                    Value::Long(n) => mysql_async::Value::Int(*n),
                    Value::Double(f) => mysql_async::Value::Double(*f),
                    Value::True => mysql_async::Value::Int(1),
                    Value::False => mysql_async::Value::Int(0),
                    _ => mysql_async::Value::Bytes(v.to_php_string().to_string_lossy().into_bytes()),
                }).collect()
            } else { Vec::new() };
            let stmt = conn.prep(&query).await?;
            let mut qr = conn.exec_iter(&stmt, mp).await?;
            let columns_raw: Vec<mysql_async::Column> = qr.columns_ref().to_vec();
            if columns_raw.is_empty() {
                let affected = qr.affected_rows();
                let insert_id = qr.last_insert_id().unwrap_or(0);
                drop(qr);
                conn.close(stmt).await?;
                return Ok((None, affected, insert_id));
            }
            let col_meta = build_column_meta(&columns_raw);
            let col_names: Vec<String> = columns_raw.iter().map(|c| c.name_str().to_string()).collect();
            let raw_rows_async: Vec<mysql_async::Row> = qr.collect().await?;
            let mut rows = Vec::with_capacity(raw_rows_async.len());
            let mut raw_rows: Vec<Vec<mysql_async::Value>> = Vec::with_capacity(raw_rows_async.len());
            for row in &raw_rows_async {
                rows.push(row_to_string_columns(row, &col_names));
                let mut rr: Vec<mysql_async::Value> = Vec::with_capacity(col_meta.len());
                for i in 0..col_meta.len() {
                    rr.push(row.as_ref(i).cloned().unwrap_or(mysql_async::Value::NULL));
                }
                raw_rows.push(rr);
            }
            let num_rows = rows.len();
            conn.close(stmt).await?;
            Ok::<_, mysql_async::Error>((Some((rows, num_rows, col_meta, raw_rows)), 0u64, 0u64))
        }.await;
        restore_conn(&holder, conn);
        res
    });

    match result {
        Ok((Some((rows, num_rows, columns, raw_rows)), _, _)) => {
            let res_id = vm.next_object_id();
            let res_obj = PhpObject::new(b"mysqli_result".to_vec(), res_id);
            RESULTS.with(|r| r.borrow_mut().insert(res_id, MysqliResult {
                rows, current_row: 0, num_rows, columns, raw_rows,
            }));
            Ok(Value::Object(Rc::new(RefCell::new(res_obj))))
        }
        Ok(_) => Ok(Value::True),
        Err(_) => Ok(Value::False),
    }
}

fn mysqli_set_local_infile_handler_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn mysqli_set_local_infile_default_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}

fn mysqli_error_list_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
}

fn mysqli_stmt_more_results_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}

fn mysqli_stmt_next_result_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::False)
}
