//! MySQL statements (ext/pdo_mysql/mysql_statement.c) over mysqlnd.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_mysqli::mysqlnd::ps::{self, Convert, ParamBind, ResultBind};
use goro_ext_mysqli::mysqlnd::result::{Field, ResultSet};
use goro_ext_mysqli::mysqlnd::*;

use super::driver::{EInfo, mysql_error, server, with_h};
use crate::core::*;

fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}

/// pdo_mysql_stmt
#[derive(Default)]
pub struct S {
    pub result: Option<Box<ResultSet>>,
    pub fields: Vec<Field>,
    pub einfo: EInfo,
    pub stmt: Option<Rc<RefCell<ps::Stmt>>>,
    pub num_params: i32,
    pub current_row: Option<Vec<Value>>,
    pub max_length: bool,
    pub done: bool,
}

fn with_s<R>(stmt: &StmtRef, f: impl FnOnce(&mut S) -> R) -> Option<R> {
    let mut b = stmt.borrow_mut();
    let s = b.driver_data.as_mut()?.downcast_mut::<S>()?;
    Some(f(s))
}

fn dbh_of(stmt: &StmtRef) -> DbhRef {
    stmt.borrow().dbh.clone().expect("statement handle")
}

/// pdo_mysql_free_result
fn free_result(s: &mut S) {
    if let Some(mut r) = s.result.take() {
        s.current_row = None;
        r.free_result(false);
    }
}

/// The libmysql compat result of mysql_more_results / next_result /
/// store_result loops over the connection: false when next_result failed.
fn eat_results(conn: &goro_ext_mysqli::mysqlnd::conn::ConnRef) -> bool {
    loop {
        if !conn.borrow().more_results() {
            return true;
        }
        if !conn.borrow_mut().next_result() {
            return false;
        }
        if let Some(mut r) = goro_ext_mysqli::mysqlnd::conn::store_result(conn) {
            r.free_result(false);
        }
    }
}

/// pdo_mysql_stmt_dtor
fn stmt_dtor(stmt: &mut Stmt) {
    let sd = match stmt.driver_data.take() {
        Some(d) => d,
        None => return,
    };
    let mut s = match sd.downcast::<S>() {
        Ok(s) => s,
        Err(_) => return,
    };
    free_result(&mut s);
    if let Some(ps) = s.stmt.take() {
        if let Ok(mut p) = ps.try_borrow_mut() {
            p.dtor(false);
        }
    }
    if !s.done && stmt.database_object_handle.is_some() {
        if let Some(dbh) = &stmt.dbh {
            let conn = dbh.try_borrow().ok().and_then(|d| {
                d.driver_data
                    .as_ref()
                    .and_then(|x| x.downcast_ref::<super::driver::H>())
                    .and_then(|h| h.server.clone())
            });
            if let Some(c) = conn {
                if c.try_borrow_mut().is_ok() {
                    eat_results(&c);
                }
            }
        }
    }
}

/// pdo_mysql_stmt_set_row_count
fn set_row_count(stmt: &StmtRef) {
    let ps = with_s(stmt, |s| s.stmt.clone()).flatten();
    if let Some(ps) = ps {
        let rc = ps.borrow().upsert.affected_rows;
        if rc != u64::MAX {
            stmt.borrow_mut().row_count = rc as i64;
        }
    }
}

/// pdo_mysql_fill_stmt_from_result
fn fill_stmt_from_result(vm: &mut Vm, stmt: &StmtRef) -> bool {
    let dbh = dbh_of(stmt);
    let conn = match server(&dbh) {
        Some(c) => c,
        None => return false,
    };
    let buffered = with_h(&dbh, |h| h.buffered).unwrap_or(true);
    let row_count = conn.borrow().upsert.affected_rows;
    if row_count == u64::MAX {
        let result = if !buffered {
            goro_ext_mysqli::mysqlnd::conn::use_result(&conn)
        } else {
            goro_ext_mysqli::mysqlnd::conn::store_result(&conn)
        };
        let result = match result {
            Some(r) => r,
            None => {
                mysql_error(Some(vm), &dbh, Some(stmt));
                return false;
            }
        };
        let nrows = result.num_rows();
        let nfields = result.field_count;
        let fields = result.fields().to_vec();
        with_s(stmt, |s| {
            s.result = Some(result);
            s.fields = fields;
        });
        let mut st = stmt.borrow_mut();
        st.row_count = nrows as i64;
        crate::stmt::set_column_count(&mut st, nfields as i32);
    } else {
        stmt.borrow_mut().row_count = row_count as i64;
    }
    true
}

/// mysqlnd_stmt_bind_one_result
fn bind_one_result(ps: &mut ps::Stmt, param_no: usize) {
    if ps.state < ps::StmtState::Prepared || param_no >= ps.field_count as usize {
        return;
    }
    let n = ps.field_count as usize;
    let rb = ps.result_bind.get_or_insert_with(Vec::new);
    if rb.len() < n {
        rb.resize(
            n,
            ResultBind {
                zv: Value::Undef,
                bound: false,
            },
        );
    }
    rb[param_no] = ResultBind {
        zv: Value::Null,
        bound: true,
    };
}

/// mysqlnd_stmt_bind_one_param
fn bind_one_param(ps: &mut ps::Stmt, param_no: i64, zv: Value, ptype: u8) -> bool {
    if ps.state < ps::StmtState::Prepared {
        ps.error_info.set_str(
            CR_NO_PREPARE_STMT,
            UNKNOWN_SQLSTATE,
            ps::MYSQLND_STMT_NOT_PREPARED,
        );
        return false;
    }
    if param_no < 0 || param_no >= ps.param_count as i64 {
        ps.error_info.set_str(
            CR_INVALID_PARAMETER_NO,
            UNKNOWN_SQLSTATE,
            "Invalid parameter number",
        );
        return false;
    }
    ps.error_info.reset();
    let n = ps.param_count as usize;
    let pb = ps.param_bind.get_or_insert_with(|| {
        vec![
            ParamBind {
                zv: Value::Undef,
                ptype: 0,
                flags: 0,
            };
            n
        ]
    });
    pb[param_no as usize] = ParamBind {
        zv,
        ptype,
        flags: 0,
    };
    ps.send_types_to_server = true;
    true
}

/// pdo_mysql_stmt_after_execute_prepared
fn after_execute_prepared(vm: &mut Vm, stmt: &StmtRef) -> bool {
    let dbh = dbh_of(stmt);
    let ps = match with_s(stmt, |s| s.stmt.clone()).flatten() {
        Some(p) => p,
        None => return false,
    };
    let fc = ps.borrow().field_count as i32;
    crate::stmt::set_column_count(&mut stmt.borrow_mut(), fc);
    let cc = stmt.borrow().column_count.max(0) as usize;
    for i in 0..cc {
        bind_one_result(&mut ps.borrow_mut(), i);
    }
    let meta = ps.borrow_mut().result_metadata();
    if let Some(meta) = meta {
        let fields = meta.fields().to_vec();
        with_s(stmt, |s| {
            s.result = Some(meta);
            s.fields = fields;
        });
        let buffered = with_h(&dbh, |h| h.buffered).unwrap_or(true);
        if buffered && !ps.borrow_mut().store_result() {
            mysql_error(Some(vm), &dbh, Some(stmt));
            return false;
        }
    }
    set_row_count(stmt);
    true
}

/// The PHP conversions of the execute request.
struct VmConvert<'a> {
    vm: &'a mut Vm,
}

impl Convert for VmConvert<'_> {
    fn to_double(&mut self, v: &Value) -> f64 {
        v.deref().to_double()
    }
    fn to_long(&mut self, v: &Value) -> i64 {
        v.deref().to_long()
    }
    fn to_string(&mut self, v: &Value) -> Option<Vec<u8>> {
        let had = self.vm.current_exception.is_some();
        let s = self.vm.value_to_string(&v.deref());
        if !had && self.vm.current_exception.is_some() {
            return None;
        }
        Some(s.as_bytes().to_vec())
    }
}

/// pdo_mysql_stmt_execute
fn stmt_execute(vm: &mut Vm, stmt: &StmtRef) -> bool {
    goro_ext_mysqli::link::sync_globals(vm);
    let dbh = dbh_of(stmt);
    let ps = with_s(stmt, |s| {
        free_result(s);
        s.done = false;
        s.stmt.clone()
    })
    .flatten();
    if let Some(ps) = ps {
        let num_bound = stmt
            .borrow()
            .bound_params
            .as_ref()
            .map(|p| p.len())
            .unwrap_or(0);
        let num_params = with_s(stmt, |s| s.num_params).unwrap_or(0);
        if (num_bound as u32) < num_params as u32 {
            stmt.borrow_mut().error_code = b"HY093".to_vec();
            return false;
        }
        let ok = {
            let mut cv = VmConvert { vm };
            ps.borrow_mut().execute(&mut cv)
        };
        if !ok {
            mysql_error(Some(vm), &dbh, Some(stmt));
            return false;
        }
        return after_execute_prepared(vm, stmt);
    }
    let conn = match server(&dbh) {
        Some(c) => c,
        None => return false,
    };
    let q = stmt
        .borrow()
        .active_query_string
        .clone()
        .unwrap_or_default();
    let ok = conn.borrow_mut().query(&q);
    if !ok {
        mysql_error(Some(vm), &dbh, Some(stmt));
        return false;
    }
    fill_stmt_from_result(vm, stmt)
}

/// pdo_mysql_stmt_next_rowset
fn stmt_next_rowset(vm: &mut Vm, stmt: &StmtRef) -> bool {
    let dbh = dbh_of(stmt);
    let ps = with_s(stmt, |s| {
        free_result(s);
        s.stmt.clone()
    })
    .flatten();
    if let Some(ps) = ps {
        ps.borrow_mut().free_result();
        if !ps.borrow_mut().next_result() {
            mysql_error(Some(vm), &dbh, Some(stmt));
            with_s(stmt, |s| s.done = true);
            return false;
        }
        return after_execute_prepared(vm, stmt);
    }
    let conn = match server(&dbh) {
        Some(c) => c,
        None => return false,
    };
    if !conn.borrow_mut().next_result() {
        mysql_error(Some(vm), &dbh, Some(stmt));
        with_s(stmt, |s| s.done = true);
        return false;
    }
    fill_stmt_from_result(vm, stmt)
}

/// pdo_mysql_stmt_param_hook
fn stmt_param_hook(vm: &mut Vm, stmt: &StmtRef, param: &mut BoundParam, event: u8) -> bool {
    let (ps, num_params) = match with_s(stmt, |s| (s.stmt.clone(), s.num_params)) {
        Some((Some(p), n)) => (p, n),
        _ => return true,
    };
    if !param.is_param {
        return true;
    }
    match event {
        PDO_PARAM_EVT_ALLOC => {
            if param.paramno < 0 || param.paramno >= num_params as i64 {
                stmt.borrow_mut().error_code = b"HY093".to_vec();
                return false;
            }
            true
        }
        PDO_PARAM_EVT_EXEC_PRE => {
            let mut parameter = param.value();
            if param_type(param.param_type) == PDO_PARAM_NULL || matches!(parameter, Value::Null) {
                bind_one_param(
                    &mut ps.borrow_mut(),
                    param.paramno,
                    Value::Null,
                    MYSQL_TYPE_NULL,
                );
                return true;
            }
            match param_type(param.param_type) {
                PDO_PARAM_STMT => return false,
                PDO_PARAM_LOB => {
                    if matches!(parameter, Value::Resource(_)) {
                        match crate::lob::read_all(vm, &parameter) {
                            Some(mem) => {
                                param.set_value(vstr(&mem));
                                parameter = vstr(&mem);
                            }
                            None => {
                                let dbh = dbh_of(stmt);
                                raise_impl_error(
                                    vm,
                                    &dbh,
                                    Some(stmt),
                                    "HY105",
                                    Some("Expected a stream resource"),
                                );
                                return false;
                            }
                        }
                    }
                }
                _ => {}
            }
            let t = match &parameter {
                Value::String(_) => MYSQL_TYPE_VAR_STRING,
                Value::Long(_) => MYSQL_TYPE_LONGLONG,
                Value::True | Value::False => MYSQL_TYPE_TINY,
                Value::Double(_) => MYSQL_TYPE_DOUBLE,
                _ => return false,
            };
            bind_one_param(&mut ps.borrow_mut(), param.paramno, parameter, t);
            true
        }
        _ => true,
    }
}

/// pdo_mysql_stmt_fetch
fn stmt_fetch(_vm: &mut Vm, stmt: &StmtRef, _ori: i64, _offset: i64) -> bool {
    let dbh = dbh_of(stmt);
    let (has_result, ps) = match with_s(stmt, |s| (s.result.is_some(), s.stmt.clone())) {
        Some(x) => x,
        None => return false,
    };
    if !has_result {
        return false;
    }
    if let Some(ps) = ps {
        let r = ps.borrow_mut().fetch();
        return match r {
            Ok(Some(row)) => {
                with_s(stmt, |s| s.current_row = Some(row));
                true
            }
            _ => {
                mysql_error(None, &dbh, Some(stmt));
                false
            }
        };
    }
    let r = with_s(stmt, |s| s.result.as_mut().map(|r| r.fetch_row(true))).flatten();
    match r {
        Some(Ok(Some(row))) => {
            with_s(stmt, |s| s.current_row = Some(row));
            true
        }
        Some(Ok(None)) | None => false,
        Some(Err(())) => {
            mysql_error(None, &dbh, Some(stmt));
            false
        }
    }
}

/// pdo_mysql_stmt_describe
fn stmt_describe(_vm: &mut Vm, stmt: &StmtRef, colno: usize) -> bool {
    let dbh = dbh_of(stmt);
    let fetch_table_names = with_h(&dbh, |h| h.fetch_table_names).unwrap_or(false);
    let mut b = stmt.borrow_mut();
    let st = &mut *b;
    let s = match st.driver_data.as_ref().and_then(|d| d.downcast_ref::<S>()) {
        Some(s) => s,
        None => return false,
    };
    if s.result.is_none() {
        return false;
    }
    if colno >= st.column_count.max(0) as usize {
        return false;
    }
    let cols = match st.columns.as_mut() {
        Some(c) => c,
        None => return false,
    };
    if cols.first().map(|c| c.name.is_some()).unwrap_or(false) {
        return true;
    }
    for (i, c) in cols.iter_mut().enumerate() {
        let f = match s.fields.get(i) {
            Some(f) => f,
            None => break,
        };
        c.name = Some(if fetch_table_names {
            let mut n = f.table.clone();
            n.push(b'.');
            n.extend_from_slice(&f.name);
            n
        } else {
            f.name.clone()
        });
        c.precision = f.decimals as u64;
        c.maxlen = f.length as usize;
    }
    true
}

/// pdo_mysql_stmt_get_col
fn stmt_get_col(_vm: &mut Vm, stmt: &StmtRef, colno: usize) -> Value {
    let b = stmt.borrow();
    let s = match b.driver_data.as_ref().and_then(|d| d.downcast_ref::<S>()) {
        Some(s) => s,
        None => return Value::Null,
    };
    if s.result.is_none() || colno >= b.column_count.max(0) as usize {
        return Value::Null;
    }
    s.current_row
        .as_ref()
        .and_then(|r| r.get(colno).cloned())
        .map(|v| match v {
            Value::Undef => Value::Null,
            v => v,
        })
        .unwrap_or(Value::Null)
}

/// type_to_name_native
fn type_to_name_native(t: u8) -> Option<&'static str> {
    Some(match t {
        MYSQL_TYPE_STRING => "STRING",
        MYSQL_TYPE_VAR_STRING => "VAR_STRING",
        MYSQL_TYPE_TINY => "TINY",
        MYSQL_TYPE_BIT => "BIT",
        MYSQL_TYPE_SHORT => "SHORT",
        MYSQL_TYPE_LONG => "LONG",
        MYSQL_TYPE_LONGLONG => "LONGLONG",
        MYSQL_TYPE_INT24 => "INT24",
        MYSQL_TYPE_FLOAT => "FLOAT",
        MYSQL_TYPE_DOUBLE => "DOUBLE",
        MYSQL_TYPE_DECIMAL => "DECIMAL",
        MYSQL_TYPE_NEWDECIMAL => "NEWDECIMAL",
        MYSQL_TYPE_GEOMETRY => "GEOMETRY",
        MYSQL_TYPE_TIMESTAMP => "TIMESTAMP",
        MYSQL_TYPE_YEAR => "YEAR",
        MYSQL_TYPE_SET => "SET",
        MYSQL_TYPE_ENUM => "ENUM",
        MYSQL_TYPE_DATE => "DATE",
        MYSQL_TYPE_NEWDATE => "NEWDATE",
        MYSQL_TYPE_VECTOR => "VECTOR",
        MYSQL_TYPE_JSON => "JSON",
        MYSQL_TYPE_TIME => "TIME",
        MYSQL_TYPE_DATETIME => "DATETIME",
        MYSQL_TYPE_TINY_BLOB => "TINY_BLOB",
        MYSQL_TYPE_MEDIUM_BLOB => "MEDIUM_BLOB",
        MYSQL_TYPE_LONG_BLOB => "LONG_BLOB",
        MYSQL_TYPE_BLOB => "BLOB",
        MYSQL_TYPE_NULL => "NULL",
        _ => return None,
    })
}

/// pdo_mysql_stmt_col_meta
fn stmt_col_meta(_vm: &mut Vm, stmt: &StmtRef, colno: i64) -> Option<PhpArray> {
    let b = stmt.borrow();
    let s = b.driver_data.as_ref()?.downcast_ref::<S>()?;
    s.result.as_ref()?;
    if colno >= b.column_count as i64 {
        return None;
    }
    let f = s.fields.get(colno as usize)?;
    let key = |k: &str| ArrayKey::String(PhpString::from_bytes(k.as_bytes()));
    let mut ret = PhpArray::new();
    let mut flags = PhpArray::new();
    if !f.def.is_empty() {
        ret.set(key("mysql:def"), vstr(&f.def));
    }
    if f.flags & NOT_NULL_FLAG != 0 {
        flags.push(vstr("not_null"));
    }
    if f.flags & PRI_KEY_FLAG != 0 {
        flags.push(vstr("primary_key"));
    }
    if f.flags & MULTIPLE_KEY_FLAG != 0 {
        flags.push(vstr("multiple_key"));
    }
    if f.flags & UNIQUE_KEY_FLAG != 0 {
        flags.push(vstr("unique_key"));
    }
    if f.flags & BLOB_FLAG != 0 {
        flags.push(vstr("blob"));
    }
    if let Some(n) = type_to_name_native(f.ftype) {
        ret.set(key("native_type"), vstr(n));
    }
    let pdo_type = match f.ftype {
        MYSQL_TYPE_BIT | MYSQL_TYPE_YEAR | MYSQL_TYPE_TINY | MYSQL_TYPE_SHORT
        | MYSQL_TYPE_INT24 | MYSQL_TYPE_LONG | MYSQL_TYPE_LONGLONG => PDO_PARAM_INT,
        _ => PDO_PARAM_STR,
    };
    ret.set(key("pdo_type"), Value::Long(pdo_type));
    ret.set(key("flags"), Value::Array(Rc::new(RefCell::new(flags))));
    ret.set(key("table"), vstr(&f.table));
    Some(ret)
}

/// pdo_mysql_stmt_cursor_closer
fn stmt_cursor_closer(vm: &mut Vm, stmt: &StmtRef) -> bool {
    let dbh = dbh_of(stmt);
    let ps = with_s(stmt, |s| {
        s.done = true;
        free_result(s);
        s.stmt.clone()
    })
    .flatten();
    if let Some(ps) = ps {
        ps.borrow_mut().free_result();
    }
    let conn = match server(&dbh) {
        Some(c) => c,
        None => return true,
    };
    if !eat_results(&conn) {
        mysql_error(Some(vm), &dbh, Some(stmt));
        return false;
    }
    true
}

pub static MYSQL_STMT_METHODS: StmtMethods = StmtMethods {
    dtor: stmt_dtor,
    executer: stmt_execute,
    fetcher: stmt_fetch,
    describer: stmt_describe,
    get_col: stmt_get_col,
    param_hook: Some(stmt_param_hook),
    set_attribute: None,
    get_attribute: None,
    get_column_meta: Some(stmt_col_meta),
    next_rowset: Some(stmt_next_rowset),
    cursor_closer: Some(stmt_cursor_closer),
};
