use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
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
    static RUNTIME: RefCell<Option<tokio::runtime::Runtime>> = RefCell::new(None);
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
    pool: mysql_async::Pool,
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
    static LAST_CONNECT_ERROR: RefCell<String> = RefCell::new(String::new());
    static LAST_CONNECT_ERRNO: RefCell<i64> = RefCell::new(0);
}

// ── Result storage ────────────────────────────────────────────────────

struct MysqliResult {
    rows: Vec<Vec<(String, Option<String>)>>,
    current_row: usize,
    num_rows: usize,
}

thread_local! {
    static RESULTS: RefCell<HashMap<u64, MysqliResult>> = RefCell::new(HashMap::new());
}

// ── Prepared statement storage ────────────────────────────────────────

struct MysqliStmt {
    conn_id: u64,
    query: String,
    #[allow(dead_code)]
    param_types: Option<String>,
    param_values: Vec<Value>,
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

type RowData = Vec<Vec<(String, Option<String>)>>;
type QueryResult = Result<(Option<(RowData, usize)>, u64, u64), mysql_async::Error>;

// ── Registration ──────────────────────────────────────────────────────

pub fn register(vm: &mut Vm) {
    vm.register_extension(b"mysqli");
    vm.register_function(b"mysqli_connect", mysqli_connect);
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

    let pool = mysql_async::Pool::new(builder);

    let conn_result = block_on(async {
        let conn = pool.get_conn().await?;
        let sv = conn.server_version();
        let info = format!("{}.{}.{}", sv.0, sv.1, sv.2);
        drop(conn);
        Ok::<String, mysql_async::Error>(info)
    });

    match conn_result {
        Ok(server_info) => {
            LAST_CONNECT_ERROR.with(|e| *e.borrow_mut() = String::new());
            LAST_CONNECT_ERRNO.with(|e| *e.borrow_mut() = 0);
            let id = vm.next_object_id();
            let obj = PhpObject::new(b"mysqli".to_vec(), id);
            CONNECTIONS.with(|c| c.borrow_mut().insert(id, MysqliConnection {
                pool, last_error: String::new(), last_errno: 0,
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
        block_on(async { conn.pool.disconnect().await.ok(); });
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}

// ── exec_query helper ─────────────────────────────────────────────────

fn exec_query(pool: &mysql_async::Pool, query_str: &str) -> QueryResult {
    let pool = pool.clone();
    block_on(async {
        let mut conn = pool.get_conn().await?;
        let mut qr = conn.query_iter(query_str).await?;
        let columns: Vec<mysql_async::Column> = qr.columns_ref().to_vec();
        if columns.is_empty() {
            let affected = qr.affected_rows();
            let insert_id = qr.last_insert_id().unwrap_or(0);
            drop(qr);
            return Ok((None, affected, insert_id));
        }
        let col_names: Vec<String> = columns.iter().map(|c| c.name_str().to_string()).collect();
        let raw_rows: Vec<mysql_async::Row> = qr.collect().await?;
        let mut rows = Vec::with_capacity(raw_rows.len());
        for row in &raw_rows {
            let mut rd = Vec::with_capacity(col_names.len());
            for (i, name) in col_names.iter().enumerate() {
                let val: Option<String> = row.get(i);
                rd.push((name.clone(), val));
            }
            rows.push(rd);
        }
        let num_rows = rows.len();
        Ok((Some((rows, num_rows)), 0u64, 0u64))
    })
}

// ── run_sql helper ────────────────────────────────────────────────────

fn run_sql(id: u64, sql: &str) -> bool {
    let pool = CONNECTIONS.with(|c| c.borrow().get(&id).map(|conn| conn.pool.clone()));
    let pool = match pool { Some(p) => p, None => return false };
    let result: Result<(), mysql_async::Error> = block_on(async {
        let mut conn = pool.get_conn().await?;
        conn.query_drop(sql).await?;
        Ok(())
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
    let pool = CONNECTIONS.with(|c| c.borrow().get(&id).map(|conn| conn.pool.clone()));
    let pool = match pool { Some(p) => p, None => return Ok(Value::False) };

    match exec_query(&pool, &query_str) {
        Ok((Some((rows, num_rows)), _, _)) => {
            let res_id = vm.next_object_id();
            let res_obj = PhpObject::new(b"mysqli_result".to_vec(), res_id);
            RESULTS.with(|r| r.borrow_mut().insert(res_id, MysqliResult { rows, current_row: 0, num_rows }));
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
    let pool = CONNECTIONS.with(|c| c.borrow().get(&id).map(|conn| conn.pool.clone()));
    let pool = match pool { Some(p) => p, None => return Ok(Value::False) };

    let result: Result<(u64, u64), mysql_async::Error> = block_on(async {
        let mut conn = pool.get_conn().await?;
        conn.query_drop(&query_str).await?;
        let affected = conn.affected_rows();
        let insert_id = conn.last_insert_id().unwrap_or(0);
        Ok((affected, insert_id))
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
        conn_id, query, param_types: None, param_values: Vec::new(),
        result_id: None, last_error: String::new(), last_errno: 0,
        affected_rows: 0, insert_id: 0,
    }));
    Ok(Value::Object(Rc::new(RefCell::new(stmt_obj))))
}

fn mysqli_stmt_bind_param(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    let types = args.get(1).map(|v| v.to_php_string().to_string_lossy()).unwrap_or_default();
    let params: Vec<Value> = args.iter().skip(2).cloned().collect();
    STMTS.with(|s| {
        let mut stmts = s.borrow_mut();
        if let Some(stmt) = stmts.get_mut(&stmt_id) {
            stmt.param_types = Some(types);
            stmt.param_values = params;
            Ok(Value::True)
        } else {
            Ok(Value::False)
        }
    })
}

fn mysqli_stmt_execute(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let stmt_id = args.first().and_then(get_object_id).unwrap_or(0);
    let stmt_info = STMTS.with(|s| {
        s.borrow().get(&stmt_id).map(|st| (st.conn_id, st.query.clone(), st.param_values.clone()))
    });
    let (conn_id, query, param_values) = match stmt_info { Some(i) => i, None => return Ok(Value::False) };
    let pool = CONNECTIONS.with(|c| c.borrow().get(&conn_id).map(|conn| conn.pool.clone()));
    let pool = match pool { Some(p) => p, None => return Ok(Value::False) };

    let result: QueryResult = block_on(async {
        let mut conn = pool.get_conn().await?;
        let mut mysql_params: Vec<mysql_async::Value> = Vec::new();
        for val in &param_values {
            match val {
                Value::Null => mysql_params.push(mysql_async::Value::NULL),
                Value::Long(n) => mysql_params.push(mysql_async::Value::Int(*n)),
                Value::Double(f) => mysql_params.push(mysql_async::Value::Double(*f)),
                Value::True => mysql_params.push(mysql_async::Value::Int(1)),
                Value::False => mysql_params.push(mysql_async::Value::Int(0)),
                _ => {
                    let s = val.to_php_string().to_string_lossy();
                    mysql_params.push(mysql_async::Value::Bytes(s.into_bytes()));
                }
            }
        }
        let stmt = conn.prep(&query).await?;
        let mut qr = conn.exec_iter(&stmt, mysql_params).await?;
        let columns: Vec<mysql_async::Column> = qr.columns_ref().to_vec();
        if columns.is_empty() {
            let affected = qr.affected_rows();
            let insert_id = qr.last_insert_id().unwrap_or(0);
            drop(qr);
            conn.close(stmt).await?;
            return Ok((None, affected, insert_id));
        }
        let col_names: Vec<String> = columns.iter().map(|c| c.name_str().to_string()).collect();
        let raw_rows: Vec<mysql_async::Row> = qr.collect().await?;
        let mut rows = Vec::with_capacity(raw_rows.len());
        for row in &raw_rows {
            let mut rd = Vec::with_capacity(col_names.len());
            for (i, name) in col_names.iter().enumerate() {
                let v: Option<String> = row.get(i);
                rd.push((name.clone(), v));
            }
            rows.push(rd);
        }
        let num_rows = rows.len();
        conn.close(stmt).await?;
        Ok((Some((rows, num_rows)), 0u64, 0u64))
    });

    match result {
        Ok((rows_opt, affected, insert_id)) => {
            STMTS.with(|s| {
                if let Some(stmt) = s.borrow_mut().get_mut(&stmt_id) {
                    stmt.affected_rows = affected as i64;
                    stmt.insert_id = insert_id as i64;
                    stmt.last_error.clear();
                    stmt.last_errno = 0;
                    if let Some((rows, num_rows)) = rows_opt {
                        let temp_key = stmt_id | 0x8000_0000_0000_0000;
                        RESULTS.with(|r| r.borrow_mut().insert(temp_key, MysqliResult { rows, current_row: 0, num_rows }));
                    }
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
    let pool = CONNECTIONS.with(|c| c.borrow().get(&id).map(|conn| conn.pool.clone()));
    let pool = match pool { Some(p) => p, None => return Ok(Value::False) };
    let result: Result<(), mysql_async::Error> = block_on(async {
        let mut conn = pool.get_conn().await?;
        conn.ping().await?;
        Ok(())
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
