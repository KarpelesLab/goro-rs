//! PostgreSQL statements (ext/pdo_pgsql/pgsql_statement.c).

use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_pgsql::libpq::{ExecStatus, Oid, PgResult};

use super::driver::*;
use super::*;
use crate::core::*;

const BOOLOID: Oid = 16;
const BYTEAOID: Oid = 17;
const DATEOID: Oid = 1082;
const INT2OID: Oid = 21;
const INT4OID: Oid = 23;
const INT8OID: Oid = 20;
const OIDOID: Oid = 26;
const TEXTOID: Oid = 25;
const TIMESTAMPOID: Oid = 1114;
const VARCHAROID: Oid = 1043;
const FLOAT4OID: Oid = 700;
const FLOAT8OID: Oid = 701;

const FIN_DISCARD: u8 = 0x1;
const FIN_CLOSE: u8 = 0x2;
const FIN_ABORT: u8 = 0x4;

fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}

/// pdo_pgsql_stmt
#[derive(Default)]
pub struct S {
    pub conn: Option<ConnRef>,
    pub result: Option<PgResult>,
    /// pdo_pgsql_column.pgsql_type
    pub cols: Vec<Oid>,
    pub cursor_name: Option<Vec<u8>>,
    pub stmt_name: Option<Vec<u8>>,
    pub query: Vec<u8>,
    pub param_values: Option<Vec<Option<Vec<u8>>>>,
    pub param_formats: Vec<i32>,
    pub param_types: Vec<Oid>,
    pub current_row: i64,
    pub is_prepared: bool,
    pub is_unbuffered: bool,
    pub is_running_unbuffered: bool,
}

pub fn with_s<R>(stmt: &StmtRef, f: impl FnOnce(&mut S) -> R) -> Option<R> {
    let mut b = stmt.borrow_mut();
    let s = b.driver_data.as_mut()?.downcast_mut::<S>()?;
    Some(f(s))
}

fn dbh_of(stmt: &StmtRef) -> DbhRef {
    stmt.borrow().dbh.clone().expect("statement handle")
}

fn take_s(stmt: &StmtRef) -> Option<Box<S>> {
    let d = stmt.borrow_mut().driver_data.take()?;
    match d.downcast::<S>() {
        Ok(s) => Some(s),
        Err(d) => {
            stmt.borrow_mut().driver_data = Some(d);
            None
        }
    }
}

fn put_s(stmt: &StmtRef, s: Box<S>) {
    stmt.borrow_mut().driver_data = Some(s);
}

fn is_running(dbh: &DbhRef, stmt: Option<&StmtRef>) -> bool {
    let Some(stmt) = stmt else {
        return false;
    };
    with_h(dbh, |h| {
        h.running_stmt
            .as_ref()
            .and_then(|w| w.upgrade())
            .is_some_and(|r| Rc::ptr_eq(&r, stmt))
    })
    .unwrap_or(false)
}

/// pgsql_stmt_finish
fn stmt_finish(s: &mut S, dbh: Option<&DbhRef>, me: Option<&StmtRef>, fin_mode: u8) {
    let Some(conn) = s.conn.clone() else {
        s.result = None;
        return;
    };
    if s.is_running_unbuffered && s.result.is_some() && fin_mode & FIN_ABORT != 0 {
        let _ = conn.borrow_mut().cancel();
        s.is_running_unbuffered = false;
    }
    s.result = None;
    if s.is_running_unbuffered {
        let mut c = conn.borrow_mut();
        while c.get_result().is_some() {}
        s.is_running_unbuffered = false;
    }
    if s.is_prepared && fin_mode & FIN_CLOSE != 0 {
        if let Some(name) = s.stmt_name.clone() {
            if let Ok(mut c) = conn.try_borrow_mut() {
                let _ = c.close_prepared(&name);
            }
            s.is_prepared = false;
            if let Some(dbh) = dbh {
                if is_running(dbh, me) {
                    with_h(dbh, |h| h.running_stmt = None);
                }
            }
        }
    }
}

/// pgsql_stmt_dtor
fn stmt_dtor(stmt: &mut Stmt) {
    let Some(sd) = stmt.driver_data.take() else {
        return;
    };
    let Ok(mut s) = sd.downcast::<S>() else {
        return;
    };
    let usable = stmt.database_object_handle.is_some()
        && stmt.dbh.as_ref().is_some_and(|d| {
            d.try_borrow()
                .is_ok_and(|d| !d.is_closed && d.driver_data.is_some())
        });
    let dbh = stmt.dbh.clone();
    let mode = FIN_DISCARD | if usable { FIN_CLOSE | FIN_ABORT } else { 0 };
    stmt_finish(&mut s, if usable { dbh.as_ref() } else { None }, None, mode);
    if usable {
        if let Some(dbh) = &dbh {
            // drop a dangling running_stmt (this statement is going away)
            with_h(dbh, |h| {
                if h.running_stmt
                    .as_ref()
                    .is_some_and(|w| w.upgrade().is_none())
                {
                    h.running_stmt = None;
                }
            });
        }
    }
    if let Some(cursor) = s.cursor_name.take() {
        if usable {
            if let Some(conn) = &s.conn {
                if let Ok(mut c) = conn.try_borrow_mut() {
                    let mut q = b"CLOSE ".to_vec();
                    q.extend_from_slice(&cursor);
                    let _ = c.exec(&q);
                }
            }
        }
    }
}

fn nparams(stmt: &StmtRef) -> usize {
    stmt.borrow()
        .bound_params
        .as_ref()
        .map(|t| t.len())
        .unwrap_or(0)
}

/// The libpq parameter arrays for `n` parameters (text values stop at a
/// NUL like strlen())
fn params_for(s: &S, n: usize) -> (Vec<Option<Vec<u8>>>, Vec<i32>, Vec<Oid>) {
    let mut vals = Vec::with_capacity(n);
    let mut fmts = Vec::with_capacity(n);
    let mut types = Vec::with_capacity(n);
    // (libpq sends no parameter types when the array is NULL)
    let have_types = s.param_values.is_some();
    for i in 0..n {
        let f = s.param_formats.get(i).copied().unwrap_or(0);
        let v = s
            .param_values
            .as_ref()
            .and_then(|p| p.get(i).cloned())
            .flatten();
        vals.push(v);
        fmts.push(f);
        if have_types {
            types.push(s.param_types.get(i).copied().unwrap_or(0));
        }
    }
    (vals, fmts, types)
}

/// pgsql_stmt_execute
fn stmt_execute(vm: &mut Vm, stmt: &StmtRef) -> bool {
    let dbh = dbh_of(stmt);
    let Some(mut s) = take_s(stmt) else {
        return false;
    };
    let ok = execute_inner(vm, stmt, &dbh, &mut s);
    put_s(stmt, s);
    ok
}

fn execute_inner(vm: &mut Vm, stmt: &StmtRef, dbh: &DbhRef, s: &mut S) -> bool {
    let Some(conn) = s.conn.clone() else {
        return false;
    };
    let in_trans = in_transaction(dbh);
    // in unbuffered mode, finish any running statement
    if s.is_unbuffered {
        let running = with_h(dbh, |h| h.running_stmt.take())
            .flatten()
            .and_then(|w| w.upgrade());
        if let Some(r) = running {
            if Rc::ptr_eq(&r, stmt) {
                stmt_finish(s, Some(dbh), Some(stmt), FIN_CLOSE);
            } else if let Some(mut other) = take_s(&r) {
                stmt_finish(&mut other, Some(dbh), Some(&r), FIN_CLOSE);
                put_s(&r, other);
            }
        }
    }
    // ensure that we free any previous unfetched results
    stmt_finish(s, Some(dbh), Some(stmt), 0);
    s.current_row = 0;

    let n = nparams(stmt);
    let supports = stmt.borrow().supports_placeholders;
    let active = stmt
        .borrow()
        .active_query_string
        .clone()
        .unwrap_or_default();
    let mut dispatch_result = true;
    if let Some(cursor) = s.cursor_name.clone() {
        if s.is_prepared {
            let mut q = b"CLOSE ".to_vec();
            q.extend_from_slice(&cursor);
            let _ = conn.borrow_mut().exec(&q);
        }
        let mut q = b"DECLARE ".to_vec();
        q.extend_from_slice(&cursor);
        q.extend_from_slice(b" SCROLL CURSOR WITH HOLD FOR ");
        q.extend_from_slice(&active);
        s.result = conn.borrow_mut().exec(&q);
        let status = status_of(s.result.as_ref());
        if status != ExecStatus::CommandOk && status != ExecStatus::TuplesOk {
            let st = sqlstate(s.result.as_ref());
            pgsql_error(
                Some(vm),
                dbh,
                Some(stmt),
                status as i64,
                st.as_deref(),
                None,
            );
            return false;
        }
        s.is_prepared = true;
        let mut q = b"FETCH FORWARD 0 FROM ".to_vec();
        q.extend_from_slice(&cursor);
        s.result = conn.borrow_mut().exec(&q);
    } else if let Some(name) = s.stmt_name.clone() {
        if !s.is_prepared {
            loop {
                let (_, _, types) = params_for(s, n);
                s.result = conn.borrow_mut().prepare(&name, &s.query, &types);
                let status = status_of(s.result.as_ref());
                match status {
                    ExecStatus::CommandOk | ExecStatus::TuplesOk => {
                        s.is_prepared = true;
                        s.result = None;
                        break;
                    }
                    _ => {
                        let st = sqlstate(s.result.as_ref());
                        if st.as_deref() == Some(b"42P05") {
                            let _ = conn.borrow_mut().close_prepared(&name);
                            continue;
                        }
                        pgsql_error(
                            Some(vm),
                            dbh,
                            Some(stmt),
                            status as i64,
                            st.as_deref(),
                            None,
                        );
                        return false;
                    }
                }
            }
        }
        let (vals, fmts, _) = params_for(s, n);
        if s.is_unbuffered {
            dispatch_result = conn
                .borrow_mut()
                .send_query_prepared(&name, &vals, &fmts, 0);
        } else {
            s.result = conn.borrow_mut().exec_prepared(&name, &vals, &fmts, 0);
        }
    } else if supports == PDO_PLACEHOLDER_NAMED {
        let (vals, fmts, types) = params_for(s, n);
        if s.is_unbuffered {
            dispatch_result = conn
                .borrow_mut()
                .send_query_params(&s.query, &types, &vals, &fmts, 0);
        } else {
            s.result = conn
                .borrow_mut()
                .exec_params(&s.query, &types, &vals, &fmts, 0);
        }
    } else if s.is_unbuffered {
        dispatch_result = conn.borrow_mut().send_query(&active);
    } else {
        s.result = conn.borrow_mut().exec(&active);
    }

    let weak = Rc::downgrade(stmt);
    with_h(dbh, |h| h.running_stmt = Some(weak));

    if s.is_unbuffered {
        if !dispatch_result {
            pgsql_error(Some(vm), dbh, Some(stmt), 0, None, None);
            with_h(dbh, |h| h.running_stmt = None);
            return false;
        }
        s.is_running_unbuffered = true;
        let _ = conn.borrow_mut().set_single_row_mode();
        s.result = conn.borrow_mut().get_result();
    }

    let status = status_of(s.result.as_ref());
    if status != ExecStatus::CommandOk
        && status != ExecStatus::TuplesOk
        && status != ExecStatus::SingleTuple
    {
        let st = sqlstate(s.result.as_ref());
        pgsql_error(
            Some(vm),
            dbh,
            Some(stmt),
            status as i64,
            st.as_deref(),
            None,
        );
        return false;
    }
    let res = s.result.as_ref().unwrap();
    let ncols = res.nfields();
    let (row_count, oid) = if status == ExecStatus::CommandOk {
        (atol(res.cmd_tuples()), Some(res.oid_value() as i64))
    } else {
        (res.ntuples() as i64, None)
    };
    {
        let mut st = stmt.borrow_mut();
        st.column_count = ncols as i32;
        st.row_count = row_count;
    }
    if s.cols.is_empty() {
        s.cols = vec![0; ncols];
    }
    if let Some(oid) = oid {
        with_h(dbh, |h| h.pgoid = oid);
    }
    if in_trans && !in_transaction(dbh) {
        close_lob_streams(dbh);
    }
    true
}

/// pgsql_stmt_param_hook
fn param_hook(vm: &mut Vm, stmt: &StmtRef, param: &mut BoundParam, event: u8) -> bool {
    let supports = stmt.borrow().supports_placeholders;
    if supports == PDO_PLACEHOLDER_NAMED && param.is_param {
        match event {
            PDO_PARAM_EVT_FREE => {}
            PDO_PARAM_EVT_NORMALIZE => {
                if let Some(name) = param.name.clone() {
                    if name.first() == Some(&b'$') {
                        param.paramno = atol(&name[1..]);
                    } else {
                        let mapped = stmt
                            .borrow()
                            .bound_param_map
                            .as_ref()
                            .and_then(|m| m.get(&HKey::Str(name.clone())).cloned());
                        match mapped {
                            Some(nv) => param.paramno = atol(&nv[1..]) - 1,
                            None => {
                                let dbh = dbh_of(stmt);
                                pgsql_error(
                                    Some(vm),
                                    &dbh,
                                    Some(stmt),
                                    0,
                                    Some(b"HY093"),
                                    Some(&name),
                                );
                                return false;
                            }
                        }
                    }
                }
            }
            PDO_PARAM_EVT_ALLOC => {
                let ok = {
                    let st = stmt.borrow();
                    match st.bound_param_map.as_ref() {
                        None => return true,
                        Some(m) => m.get(&HKey::Int(param.paramno)).is_some(),
                    }
                };
                if !ok {
                    let dbh = dbh_of(stmt);
                    pgsql_error(
                        Some(vm),
                        &dbh,
                        Some(stmt),
                        0,
                        Some(b"HY093"),
                        Some(b"parameter was not defined"),
                    );
                    return false;
                }
            }
            PDO_PARAM_EVT_EXEC_POST | PDO_PARAM_EVT_FETCH_PRE | PDO_PARAM_EVT_FETCH_POST => {}
            PDO_PARAM_EVT_EXEC_PRE => {
                let map_len = match stmt.borrow().bound_param_map.as_ref() {
                    None => return true,
                    Some(m) => m.len(),
                };
                if param.paramno < 0 {
                    return true;
                }
                let idx = param.paramno as usize;
                let ptype = param_type(param.param_type);
                let mut value = param.value();
                if ptype == PDO_PARAM_LOB && matches!(value, Value::Resource(_)) {
                    let id = match &value {
                        Value::Resource(id) => *id,
                        _ => 0,
                    };
                    if let Some(oid) = lob_stream_oid(id) {
                        with_s(stmt, |s| {
                            ensure_params(s, map_len, idx);
                            s.param_values.as_mut().unwrap()[idx] =
                                Some(oid.to_be_bytes().to_vec());
                            s.param_formats[idx] = 1;
                            s.param_types[idx] = OIDOID;
                        });
                        return true;
                    }
                    match crate::lob::read_all(vm, &value) {
                        Some(d) => {
                            value = vstr(d);
                            param.set_value(value.clone());
                        }
                        None => {
                            // expected a stream resource
                            let dbh = dbh_of(stmt);
                            pgsql_error(
                                Some(vm),
                                &dbh,
                                Some(stmt),
                                PGRES_FATAL_ERROR,
                                Some(b"HY105"),
                                None,
                            );
                            return false;
                        }
                    }
                }
                let (v, fmt) = if ptype == PDO_PARAM_NULL || matches!(value, Value::Null) {
                    (None, None)
                } else if matches!(value, Value::True | Value::False) {
                    (
                        Some(if matches!(value, Value::True) {
                            b"t".to_vec()
                        } else {
                            b"f".to_vec()
                        }),
                        Some(0),
                    )
                } else {
                    let sv = match &value {
                        Value::String(s) => s.as_bytes().to_vec(),
                        other => {
                            let s = vm.value_to_string(other);
                            param.set_value(Value::String(s.clone()));
                            s.as_bytes().to_vec()
                        }
                    };
                    (Some(sv), Some(0))
                };
                with_s(stmt, |s| {
                    ensure_params(s, map_len, idx);
                    s.param_values.as_mut().unwrap()[idx] = v;
                    if let Some(f) = fmt {
                        s.param_formats[idx] = f;
                    }
                    s.param_types[idx] = 0;
                    if ptype == PDO_PARAM_LOB {
                        s.param_formats[idx] = 1;
                    }
                });
            }
            _ => {}
        }
    } else if param.is_param && event == PDO_PARAM_EVT_NORMALIZE {
        // We need to manually convert to a pg native boolean value
        if param_type(param.param_type) == PDO_PARAM_BOOL
            && (param.param_type & PDO_PARAM_INPUT_OUTPUT) != PDO_PARAM_INPUT_OUTPUT
        {
            let t = param.value().is_truthy();
            param.param_type = PDO_PARAM_STR;
            param.parameter = vstr(if t { "t" } else { "f" });
        }
    }
    true
}

fn ensure_params(s: &mut S, len: usize, idx: usize) {
    let len = len.max(idx + 1);
    let vals = s.param_values.get_or_insert_with(Vec::new);
    if vals.len() < len {
        vals.resize(len, None);
    }
    if s.param_formats.len() < len {
        s.param_formats.resize(len, 0);
    }
    if s.param_types.len() < len {
        s.param_types.resize(len, 0);
    }
}

/// pgsql_stmt_fetch
fn stmt_fetch(vm: &mut Vm, stmt: &StmtRef, ori: i64, offset: i64) -> bool {
    let dbh = dbh_of(stmt);
    let Some(mut s) = take_s(stmt) else {
        return false;
    };
    let r = fetch_inner(vm, stmt, &dbh, &mut s, ori, offset);
    put_s(stmt, s);
    r
}

fn fetch_inner(
    vm: &mut Vm,
    stmt: &StmtRef,
    dbh: &DbhRef,
    s: &mut S,
    ori: i64,
    offset: i64,
) -> bool {
    let Some(conn) = s.conn.clone() else {
        return false;
    };
    if let Some(cursor) = s.cursor_name.clone() {
        let ori_str = match ori {
            PDO_FETCH_ORI_NEXT => "NEXT".to_string(),
            PDO_FETCH_ORI_PRIOR => "BACKWARD".to_string(),
            PDO_FETCH_ORI_FIRST => "FIRST".to_string(),
            PDO_FETCH_ORI_LAST => "LAST".to_string(),
            PDO_FETCH_ORI_ABS => format!("ABSOLUTE {}", offset),
            PDO_FETCH_ORI_REL => format!("RELATIVE {}", offset),
            _ => return false,
        };
        s.result = None;
        let mut q = format!("FETCH {} FROM ", ori_str).into_bytes();
        q.extend_from_slice(&cursor);
        s.result = conn.borrow_mut().exec(&q);
        let status = status_of(s.result.as_ref());
        if status != ExecStatus::CommandOk && status != ExecStatus::TuplesOk {
            let st = sqlstate(s.result.as_ref());
            pgsql_error(
                Some(vm),
                dbh,
                Some(stmt),
                status as i64,
                st.as_deref(),
                None,
            );
            return false;
        }
        if s.result.as_ref().is_some_and(|r| r.ntuples() > 0) {
            s.current_row = 1;
            return true;
        }
        return false;
    }
    let row_count = stmt.borrow().row_count;
    if s.is_running_unbuffered && s.current_row >= row_count {
        s.result = None;
        s.result = conn.borrow_mut().get_result();
        let status = status_of(s.result.as_ref());
        if status != ExecStatus::CommandOk
            && status != ExecStatus::TuplesOk
            && status != ExecStatus::SingleTuple
        {
            let st = sqlstate(s.result.as_ref());
            pgsql_error(
                Some(vm),
                dbh,
                Some(stmt),
                status as i64,
                st.as_deref(),
                None,
            );
            return false;
        }
        let rc = s.result.as_ref().map(|r| r.ntuples() as i64).unwrap_or(0);
        stmt.borrow_mut().row_count = rc;
        s.current_row = 0;
        if rc == 0 {
            s.is_running_unbuffered = false;
            // libpq requires looping until getResult returns null
            stmt_finish(s, Some(dbh), Some(stmt), 0);
        }
    }
    let row_count = stmt.borrow().row_count;
    if s.current_row < row_count {
        s.current_row += 1;
        true
    } else {
        false
    }
}

/// pgsql_stmt_describe
fn stmt_describe(_vm: &mut Vm, stmt: &StmtRef, colno: usize) -> bool {
    let info = with_s(stmt, |s| {
        let r = s.result.as_ref()?;
        let t = r.ftype(colno);
        if s.cols.len() <= colno {
            s.cols.resize(colno + 1, 0);
        }
        s.cols[colno] = t;
        Some((
            r.fname(colno).unwrap_or(b"").to_vec(),
            r.fsize(colno) as i64,
            r.fmod(colno) as i64,
        ))
    })
    .flatten();
    let Some((name, size, modv)) = info else {
        return false;
    };
    let mut st = stmt.borrow_mut();
    if let Some(c) = st.columns.as_mut().and_then(|c| c.get_mut(colno)) {
        c.name = Some(name);
        c.maxlen = size as usize;
        c.precision = modv as u64;
    }
    true
}

fn strncmp_eq(ptr: &[u8], lit: &[u8], len: usize) -> bool {
    for i in 0..len {
        let a = ptr.get(i).copied().unwrap_or(0);
        let b = lit.get(i).copied().unwrap_or(0);
        if a != b {
            return false;
        }
        if a == 0 {
            return true;
        }
    }
    true
}

/// zend_strtod(): the longest float prefix
fn strtod(s: &[u8]) -> f64 {
    let t = String::from_utf8_lossy(s);
    let t = t.trim_start();
    let mut end = 0;
    let b = t.as_bytes();
    let mut best = 0.0;
    while end < b.len() {
        end += 1;
        if let Ok(v) = t[..end].parse::<f64>() {
            best = v;
        }
    }
    best
}

fn stmt_get_col(vm: &mut Vm, stmt: &StmtRef, colno: usize) -> Value {
    stmt_get_col_typed(vm, stmt, colno, None)
}

/// pgsql_stmt_get_col
fn stmt_get_col_typed(vm: &mut Vm, stmt: &StmtRef, colno: usize, ty: Option<i64>) -> Value {
    let cell = with_s(stmt, |s| {
        let r = s.result.as_ref()?;
        let row = (s.current_row - 1).max(0) as usize;
        if r.is_null(row, colno) {
            return Some(None);
        }
        Some(Some((
            r.value(row, colno).to_vec(),
            s.cols.get(colno).copied().unwrap_or(0),
            s.conn.clone(),
        )))
    })
    .flatten();
    let Some(cell) = cell else {
        return Value::Null;
    };
    let Some((ptr, t, conn)) = cell else {
        return Value::Null;
    };
    let len = ptr.len();
    match t {
        BOOLOID => vbool(ptr.first() == Some(&b't')),
        INT2OID | INT4OID | INT8OID => Value::Long(atol(&ptr)),
        FLOAT4OID | FLOAT8OID => {
            if strncmp_eq(&ptr, b"Infinity", len) {
                Value::Double(f64::INFINITY)
            } else if strncmp_eq(&ptr, b"-Infinity", len) {
                Value::Double(f64::NEG_INFINITY)
            } else if strncmp_eq(&ptr, b"NaN", len) {
                Value::Double(f64::NAN)
            } else {
                Value::Double(strtod(&ptr))
            }
        }
        OIDOID => {
            let oid = atol(&ptr) as u32;
            if ty == Some(PDO_PARAM_LOB) {
                let Some(conn) = conn else {
                    return Value::Null;
                };
                let loid = conn
                    .borrow_mut()
                    .lo_open(oid, goro_ext_pgsql::libpq::lo::INV_READ);
                if loid >= 0 {
                    let obj = stmt.borrow().database_object_handle.clone();
                    if let Some(obj) = obj {
                        if let Some(id) = create_lob_stream(vm, &obj, loid, oid) {
                            return Value::Resource(id);
                        }
                    }
                }
                Value::Null
            } else {
                Value::Long(oid as i64)
            }
        }
        BYTEAOID => {
            let data = goro_ext_pgsql::libpq::unescape_bytea(&ptr);
            crate::lob::memory_stream(vm, &data)
        }
        _ => vstr(ptr),
    }
}

/// pdo_pgsql_translate_oid_to_table
fn translate_oid_to_table(conn: &ConnRef, oid: Oid) -> Option<Vec<u8>> {
    let q = format!("SELECT RELNAME FROM PG_CLASS WHERE OID={}", oid as i32);
    let r = conn.borrow_mut().exec(q.as_bytes())?;
    if r.status != ExecStatus::TuplesOk {
        return None;
    }
    if r.ntuples() == 0 || r.is_null(0, 0) {
        return None;
    }
    Some(r.value(0, 0).to_vec())
}

/// pgsql_stmt_get_column_meta
fn stmt_col_meta(_vm: &mut Vm, stmt: &StmtRef, colno: i64) -> Option<PhpArray> {
    let column_count = stmt.borrow().column_count as i64;
    let (t, table_oid, conn) = with_s(stmt, |s| {
        let r = s.result.as_ref()?;
        if colno >= column_count {
            return None;
        }
        Some((
            s.cols.get(colno as usize).copied().unwrap_or(0),
            r.ftable(colno as usize),
            s.conn.clone()?,
        ))
    })
    .flatten()?;
    let k = |s: &str| ArrayKey::String(PhpString::from_bytes(s.as_bytes()));
    let mut a = PhpArray::new();
    a.set(k("pgsql:oid"), Value::Long(t as i64));
    a.set(k("pgsql:table_oid"), Value::Long(table_oid as i64));
    if let Some(tn) = translate_oid_to_table(&conn, table_oid) {
        a.set(k("table"), vstr(tn));
    }
    let label = match t {
        BOOLOID => Some("bool"),
        BYTEAOID => Some("bytea"),
        INT8OID => Some("int8"),
        INT2OID => Some("int2"),
        INT4OID => Some("int4"),
        FLOAT4OID => Some("float4"),
        FLOAT8OID => Some("float8"),
        TEXTOID => Some("text"),
        VARCHAROID => Some("varchar"),
        DATEOID => Some("date"),
        TIMESTAMPOID => Some("timestamp"),
        _ => None,
    };
    match label {
        Some(l) => a.set(k("native_type"), vstr(l)),
        None => {
            let q = format!("SELECT TYPNAME FROM PG_TYPE WHERE OID={}", t);
            if let Some(r) = conn.borrow_mut().exec(q.as_bytes()) {
                if r.status == ExecStatus::TuplesOk && r.ntuples() == 1 {
                    a.set(k("native_type"), vstr(r.value(0, 0)));
                }
            }
        }
    }
    let pt = match t {
        BOOLOID => PDO_PARAM_BOOL,
        INT2OID | INT4OID | INT8OID => PDO_PARAM_INT,
        OIDOID | BYTEAOID => PDO_PARAM_LOB,
        _ => PDO_PARAM_STR,
    };
    a.set(k("pdo_type"), Value::Long(pt));
    Some(a)
}

fn stmt_cursor_closer(_vm: &mut Vm, _stmt: &StmtRef) -> bool {
    true
}

/// pgsql_stmt_get_attr
fn stmt_get_attr(vm: &mut Vm, stmt: &StmtRef, attr: i64) -> AttrResult {
    match attr {
        PDO_PGSQL_ATTR_RESULT_MEMORY_SIZE => {
            if stmt.borrow().executed {
                let n = with_s(stmt, |s| s.result.as_ref().map(|r| r.memory_size as i64))
                    .flatten()
                    .unwrap_or(0);
                AttrResult::Value(Value::Long(n))
            } else {
                let name = with_s(stmt, |s| s.stmt_name.clone()).flatten();
                let msg = format!(
                    "statement '{}' has not been executed yet",
                    String::from_utf8_lossy(name.as_deref().unwrap_or(b"(null)"))
                );
                let dbh = dbh_of(stmt);
                pgsql_error(
                    Some(vm),
                    &dbh,
                    Some(stmt),
                    0,
                    Some(b"HY000"),
                    Some(msg.as_bytes()),
                );
                AttrResult::Value(Value::Null)
            }
        }
        _ => AttrResult::Unsupported,
    }
}

pub static STMT_METHODS: StmtMethods = StmtMethods {
    dtor: stmt_dtor,
    executer: stmt_execute,
    fetcher: stmt_fetch,
    describer: stmt_describe,
    get_col: stmt_get_col,
    get_col_typed: Some(stmt_get_col_typed),
    param_hook: Some(param_hook),
    set_attribute: None,
    get_attribute: Some(stmt_get_attr),
    get_column_meta: Some(stmt_col_meta),
    next_rowset: None,
    cursor_closer: Some(stmt_cursor_closer),
};
