//! SQLite statements (ext/pdo_sqlite/sqlite_statement.c).

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_sqlite::consts::*;
use goro_sqlite::int::StmtId;
use goro_sqlite::{mainapi, vdbeapi};

use super::driver::*;
use super::*;
use crate::core::*;

fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}

/// pdo_sqlite_stmt
#[derive(Default)]
pub struct S {
    pub conn: Option<ConnRef>,
    pub stmt: Option<StmtId>,
    pub pre_fetched: bool,
    pub done: bool,
}

pub fn with_s<R>(stmt: &StmtRef, f: impl FnOnce(&mut S) -> R) -> Option<R> {
    let mut b = stmt.borrow_mut();
    let s = b.driver_data.as_mut()?.downcast_mut::<S>()?;
    Some(f(s))
}

fn dbh_of(stmt: &StmtRef) -> DbhRef {
    stmt.borrow().dbh.clone().expect("statement handle")
}

/// (connection, statement handle)
fn parts(stmt: &StmtRef) -> Option<(ConnRef, Option<StmtId>)> {
    with_s(stmt, |s| s.conn.clone().map(|c| (c, s.stmt))).flatten()
}

/// pdo_sqlite_error_stmt
fn error_stmt(vm: &mut Vm, stmt: &StmtRef) {
    let dbh = dbh_of(stmt);
    sqlite_error(Some(vm), &dbh, Some(stmt));
}

/// pdo_sqlite_stmt_dtor
fn stmt_dtor(stmt: &mut Stmt) {
    let Some(sd) = stmt.driver_data.take() else {
        return;
    };
    let Ok(mut s) = sd.downcast::<S>() else {
        return;
    };
    if let (Some(conn), Some(id)) = (s.conn.take(), s.stmt.take()) {
        conn.finalize(id);
    }
}

/// pdo_sqlite_stmt_execute
fn stmt_execute(vm: &mut Vm, stmt: &StmtRef) -> bool {
    let Some((conn, Some(id))) = parts(stmt) else {
        return false;
    };
    let executed = stmt.borrow().executed;
    let done = with_s(stmt, |s| s.done).unwrap_or(false);
    let r = run(vm, &conn, |db| {
        if executed && !done {
            vdbeapi::sqlite3_reset(db, id);
        }
        match vdbeapi::sqlite3_step(db, id) {
            SQLITE_ROW => Ok((true, vdbeapi::sqlite3_data_count(db, id), 0)),
            SQLITE_DONE => {
                let n = vdbeapi::sqlite3_column_count(db, id);
                let changes = mainapi::sqlite3_changes(db);
                vdbeapi::sqlite3_reset(db, id);
                Ok((false, n, changes))
            }
            rc => {
                if rc == SQLITE_ERROR {
                    vdbeapi::sqlite3_reset(db, id);
                }
                Err(())
            }
        }
    });
    with_s(stmt, |s| s.done = false);
    match r {
        Some(Ok((true, n, _))) => {
            with_s(stmt, |s| s.pre_fetched = true);
            crate::stmt::set_column_count(&mut stmt.borrow_mut(), n);
            true
        }
        Some(Ok((false, n, changes))) => {
            {
                let mut st = stmt.borrow_mut();
                crate::stmt::set_column_count(&mut st, n);
                st.row_count = changes as i64;
            }
            with_s(stmt, |s| s.done = true);
            true
        }
        Some(Err(())) => {
            error_stmt(vm, stmt);
            false
        }
        None => {
            let dbh = dbh_of(stmt);
            busy_error(vm, &dbh, Some(stmt));
            false
        }
    }
}

/// The value bound to a parameter, through its reference
fn param_value(param: &BoundParam) -> Value {
    param.value()
}

/// pdo_sqlite_stmt_param_hook
fn param_hook(vm: &mut Vm, stmt: &StmtRef, param: &mut BoundParam, event: u8) -> bool {
    if event != PDO_PARAM_EVT_EXEC_PRE {
        return true;
    }
    let Some((conn, Some(id))) = parts(stmt) else {
        return false;
    };
    let executed = stmt.borrow().executed;
    let done = with_s(stmt, |s| s.done).unwrap_or(false);
    if executed && !done {
        run(vm, &conn, |db| vdbeapi::sqlite3_reset(db, id));
        with_s(stmt, |s| s.done = true);
    }
    if !param.is_param {
        return true;
    }
    if param.paramno == -1 {
        let name = param.name.clone().unwrap_or_default();
        let idx = run(vm, &conn, |db| {
            vdbeapi::sqlite3_bind_parameter_index(db, id, Some(&name))
        })
        .unwrap_or(0);
        param.paramno = idx as i64 - 1;
    }
    let pos = (param.paramno + 1) as i32;
    let bind_null = |vm: &mut Vm| run(vm, &conn, |db| vdbeapi::sqlite3_bind_null(db, id, pos));
    match param_type(param.param_type) {
        PDO_PARAM_STMT => false,
        PDO_PARAM_NULL => {
            if bind_null(vm) == Some(SQLITE_OK) {
                return true;
            }
            error_stmt(vm, stmt);
            false
        }
        PDO_PARAM_INT | PDO_PARAM_BOOL => {
            let v = param_value(param);
            if matches!(v, Value::Null) {
                if bind_null(vm) == Some(SQLITE_OK) {
                    return true;
                }
            } else {
                // convert_to_long()
                let n = match &v {
                    Value::Long(n) => *n,
                    other => {
                        let n = other.to_long();
                        param.set_value(Value::Long(n));
                        n
                    }
                };
                if run(vm, &conn, |db| vdbeapi::sqlite3_bind_int64(db, id, pos, n))
                    == Some(SQLITE_OK)
                {
                    return true;
                }
            }
            error_stmt(vm, stmt);
            false
        }
        PDO_PARAM_LOB => {
            let v = param_value(param);
            let data = if let Value::Resource(_) = v {
                match crate::lob::read_all(vm, &v) {
                    Some(d) => {
                        param.set_value(vstr(&d));
                        d
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
            } else if matches!(v, Value::Null) {
                if bind_null(vm) == Some(SQLITE_OK) {
                    return true;
                }
                error_stmt(vm, stmt);
                return false;
            } else {
                match try_convert_to_string(vm, param, &v) {
                    Some(s) => s,
                    None => return false,
                }
            };
            run(vm, &conn, |db| {
                vdbeapi::sqlite3_bind_blob(db, id, pos, &data, data.len() as i32)
            }) == Some(SQLITE_OK)
        }
        _ => {
            let v = param_value(param);
            if matches!(v, Value::Null) {
                if bind_null(vm) == Some(SQLITE_OK) {
                    return true;
                }
            } else {
                let Some(s) = try_convert_to_string(vm, param, &v) else {
                    return false;
                };
                if run(vm, &conn, |db| {
                    vdbeapi::sqlite3_bind_text(db, id, pos, Some(&s), s.len() as i32)
                }) == Some(SQLITE_OK)
                {
                    return true;
                }
            }
            error_stmt(vm, stmt);
            false
        }
    }
}

/// try_convert_to_string() of the bound value (converted in place)
fn try_convert_to_string(vm: &mut Vm, param: &mut BoundParam, v: &Value) -> Option<Vec<u8>> {
    if let Value::String(s) = v {
        return Some(s.as_bytes().to_vec());
    }
    let s = vm.value_to_string_checked(v).ok()?;
    if vm.current_exception.is_some() {
        return None;
    }
    param.set_value(Value::String(s.clone()));
    Some(s.as_bytes().to_vec())
}

/// pdo_sqlite_stmt_fetch
fn stmt_fetch(vm: &mut Vm, stmt: &StmtRef, _ori: i64, _offset: i64) -> bool {
    let Some((conn, id)) = parts(stmt) else {
        return false;
    };
    let Some(id) = id else {
        return false;
    };
    let (pre, done) = with_s(stmt, |s| (s.pre_fetched, s.done)).unwrap_or((false, true));
    if pre {
        with_s(stmt, |s| s.pre_fetched = false);
        return true;
    }
    if done {
        return false;
    }
    let r = run(vm, &conn, |db| {
        let i = vdbeapi::sqlite3_step(db, id);
        if i == SQLITE_DONE || i == SQLITE_ERROR {
            vdbeapi::sqlite3_reset(db, id);
        }
        i
    });
    match r {
        Some(SQLITE_ROW) => true,
        Some(SQLITE_DONE) => {
            with_s(stmt, |s| s.done = true);
            false
        }
        Some(_) => {
            error_stmt(vm, stmt);
            false
        }
        None => {
            let dbh = dbh_of(stmt);
            busy_error(vm, &dbh, Some(stmt));
            false
        }
    }
}

/// pdo_sqlite_stmt_describe
fn stmt_describe(vm: &mut Vm, stmt: &StmtRef, colno: usize) -> bool {
    let Some((conn, Some(id))) = parts(stmt) else {
        return false;
    };
    let r = run(vm, &conn, |db| {
        if colno as i32 >= vdbeapi::sqlite3_column_count(db, id) {
            return None;
        }
        Some(vdbeapi::sqlite3_column_name(db, id, colno as i32).unwrap_or_default())
    });
    let Some(Some(name)) = r else {
        // error invalid column
        error_stmt(vm, stmt);
        return false;
    };
    let mut st = stmt.borrow_mut();
    if let Some(c) = st.columns.as_mut().and_then(|c| c.get_mut(colno)) {
        c.name = Some(name);
        c.maxlen = usize::MAX;
        c.precision = 0;
    }
    true
}

/// pdo_sqlite_stmt_get_col
fn stmt_get_col(vm: &mut Vm, stmt: &StmtRef, colno: usize) -> Value {
    let Some((conn, Some(id))) = parts(stmt) else {
        return Value::Null;
    };
    let i = colno as i32;
    let r = run(vm, &conn, |db| {
        if i >= vdbeapi::sqlite3_data_count(db, id) {
            return None;
        }
        Some(match vdbeapi::sqlite3_column_type(db, id, i) {
            SQLITE_NULL => Value::Null,
            SQLITE_INTEGER => Value::Long(vdbeapi::sqlite3_column_int64(db, id, i)),
            SQLITE_FLOAT => Value::Double(vdbeapi::sqlite3_column_double(db, id, i)),
            SQLITE_BLOB => {
                let mut b = vdbeapi::sqlite3_column_blob(db, id, i).unwrap_or_default();
                let n = vdbeapi::sqlite3_column_bytes(db, id, i).max(0) as usize;
                b.truncate(n);
                vstr(b)
            }
            _ => {
                let mut t = vdbeapi::sqlite3_column_text(db, id, i).unwrap_or_default();
                let n = vdbeapi::sqlite3_column_bytes(db, id, i).max(0) as usize;
                t.truncate(n);
                vstr(t)
            }
        })
    });
    match r {
        Some(Some(v)) => v,
        _ => {
            // error invalid column
            error_stmt(vm, stmt);
            Value::Null
        }
    }
}

/// pdo_sqlite_stmt_col_meta
fn stmt_col_meta(vm: &mut Vm, stmt: &StmtRef, colno: i64) -> Option<PhpArray> {
    let (conn, id) = parts(stmt)?;
    let id = id?;
    if !stmt.borrow().executed {
        return None;
    }
    let i = colno as i32;
    let r = run(vm, &conn, |db| {
        if colno >= vdbeapi::sqlite3_column_count(db, id) as i64 {
            return None;
        }
        Some((
            vdbeapi::sqlite3_column_type(db, id, i),
            vdbeapi::sqlite3_column_decltype(db, id, i),
            vdbeapi::sqlite3_column_table_name(db, id, i),
        ))
    });
    let Some(Some((ty, decl, table))) = r else {
        // error invalid column
        error_stmt(vm, stmt);
        return None;
    };
    let k = |s: &str| ArrayKey::String(PhpString::from_bytes(s.as_bytes()));
    let mut a = PhpArray::new();
    let mut flags = PhpArray::new();
    let (native, pdo_type) = match ty {
        SQLITE_NULL => (Some("null"), PDO_PARAM_NULL),
        SQLITE_FLOAT => (Some("double"), PDO_PARAM_STR),
        SQLITE_BLOB => {
            flags.push(vstr("blob"));
            (Some("string"), PDO_PARAM_STR)
        }
        SQLITE_TEXT => (Some("string"), PDO_PARAM_STR),
        SQLITE_INTEGER => (Some("integer"), PDO_PARAM_INT),
        _ => (None, 0),
    };
    if let Some(n) = native {
        a.set(k("native_type"), vstr(n));
        a.set(k("pdo_type"), Value::Long(pdo_type));
    }
    if let Some(d) = decl {
        a.set(k("sqlite:decl_type"), vstr(d));
    }
    if let Some(t) = table {
        a.set(k("table"), vstr(t));
    }
    a.set(
        k("flags"),
        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(flags))),
    );
    Some(a)
}

/// pdo_sqlite_stmt_cursor_closer
fn stmt_cursor_closer(vm: &mut Vm, stmt: &StmtRef) -> bool {
    if let Some((conn, Some(id))) = parts(stmt) {
        run(vm, &conn, |db| vdbeapi::sqlite3_reset(db, id));
    }
    true
}

/// pdo_sqlite_stmt_get_attribute
fn stmt_get_attr(vm: &mut Vm, stmt: &StmtRef, attr: i64) -> AttrResult {
    let q = |vm: &mut Vm, f: fn(&mut goro_sqlite::int::Sqlite3, StmtId) -> i32| -> i32 {
        match parts(stmt) {
            Some((conn, Some(id))) => run(vm, &conn, |db| f(db, id)).unwrap_or(0),
            _ => 0,
        }
    };
    match attr {
        PDO_SQLITE_ATTR_READONLY_STATEMENT => {
            AttrResult::Value(vbool(q(vm, vdbeapi::sqlite3_stmt_readonly) != 0))
        }
        PDO_SQLITE_ATTR_BUSY_STATEMENT => {
            AttrResult::Value(vbool(q(vm, vdbeapi::sqlite3_stmt_busy) != 0))
        }
        PDO_SQLITE_ATTR_EXPLAIN_STATEMENT => {
            AttrResult::Value(Value::Long(q(vm, vdbeapi::sqlite3_stmt_isexplain) as i64))
        }
        _ => AttrResult::Unsupported,
    }
}

/// pdo_sqlite_stmt_set_attribute
fn stmt_set_attr(vm: &mut Vm, stmt: &StmtRef, attr: i64, val: &Value) -> bool {
    match attr {
        PDO_SQLITE_ATTR_EXPLAIN_STATEMENT => {
            let v = val.deref();
            let Value::Long(mode) = v else {
                let m = format!(
                    "explain mode must be of type int, {} given",
                    Vm::value_type_name(&v)
                );
                throw(vm, b"TypeError", &m);
                return false;
            };
            if !(0..=2).contains(&mode) {
                throw(
                    vm,
                    b"ValueError",
                    "explain mode must be one of the Pdo\\Sqlite::EXPLAIN_MODE_* constants",
                );
                return false;
            }
            match parts(stmt) {
                Some((conn, Some(id))) => {
                    run(vm, &conn, |db| {
                        vdbeapi::sqlite3_stmt_explain(db, id, mode as i32)
                    }) == Some(SQLITE_OK)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

pub static STMT_METHODS: StmtMethods = StmtMethods {
    dtor: stmt_dtor,
    executer: stmt_execute,
    fetcher: stmt_fetch,
    describer: stmt_describe,
    get_col: stmt_get_col,
    get_col_typed: None,
    param_hook: Some(param_hook),
    set_attribute: Some(stmt_set_attr),
    get_attribute: Some(stmt_get_attr),
    get_column_meta: Some(stmt_col_meta),
    next_rowset: None,
    cursor_closer: Some(stmt_cursor_closer),
};
