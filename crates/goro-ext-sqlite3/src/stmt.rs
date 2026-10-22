//! The SQLite3Stmt methods.

use std::rc::Rc;

use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_sqlite::consts::*;
use goro_sqlite::int::{Sqlite3, StmtId};
use goro_sqlite::vdbeapi::*;

use crate::conn::*;
use crate::db::{free_list_add, prepare_v2, step};
use crate::zpp::{Func, Obj, cstr, long_arg, parse, str_or_long, throw, vbool, vstr};
use crate::{errmsg, php_sqlite3_error};

/// The payload of `$this`
fn this_stmt(this: Option<&Obj>) -> Rc<StmtData> {
    match this {
        Some(o) => attach_stmt(o),
        None => Rc::new(StmtData::default()),
    }
}

/// SQLITE3_CHECK_INITIALIZED(stmt_obj->db_obj, stmt_obj->initialised,
/// SQLite3): the database payload.
fn check_initialised(vm: &mut Vm, s: &StmtData) -> Result<Rc<DbData>, ()> {
    let st = s.st.borrow();
    match (&st.db_obj, st.initialised) {
        (Some((_, d)), true) => Ok(d.clone()),
        _ => {
            drop(st);
            throw(
                vm,
                b"Error",
                "The SQLite3 object has not been correctly initialised or is already closed",
            );
            Err(())
        }
    }
}

/// SQLITE3_CHECK_INITIALIZED_STMT(stmt_obj->stmt, SQLite3Stmt)
fn check_stmt(vm: &mut Vm, s: &StmtData) -> Result<StmtId, ()> {
    match s.st.borrow().stmt {
        Some(id) => Ok(id),
        None => {
            throw(
                vm,
                b"Error",
                "The SQLite3Stmt object has not been correctly initialised or is already closed",
            );
            Err(())
        }
    }
}

/// Both checks: (database, statement)
fn check_both(vm: &mut Vm, s: &StmtData) -> Result<(Rc<DbData>, StmtId), ()> {
    let d = check_initialised(vm, s)?;
    let id = check_stmt(vm, s)?;
    Ok((d, id))
}

/// sqlite3_errcode(sqlite3_db_handle(stmt)) and sqlite3_errmsg(): with a
/// NULL statement, SQLITE_MISUSE and "out of memory".
pub fn stmt_error(db: &mut Sqlite3, stmt: Option<StmtId>) -> (i32, String) {
    match stmt {
        Some(_) => (goro_sqlite::mainapi::sqlite3_errcode(db), errmsg(db)),
        None => (SQLITE_MISUSE, "out of memory".to_string()),
    }
}

/// SQLite3Stmt::paramCount()
pub fn param_count(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let s = this_stmt(this);
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    let Ok((d, id)) = check_both(vm, &s) else {
        return Value::Null;
    };
    match d.with_conn(vm, |db| sqlite3_bind_parameter_count(db, id)) {
        Ok(n) => Value::Long(n as i64),
        Err(()) => Value::Null,
    }
}

/// SQLite3Stmt::close()
pub fn close(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let s = this_stmt(this);
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    if check_initialised(vm, &s).is_err() {
        return Value::Null;
    }
    // zend_llist_del_element(&db_obj->free_list, stmt_obj,
    // php_sqlite3_compare_stmt_free): php-src compares the statement
    // object with the sqlite3_stmt pointers of the list, which never
    // matches, so nothing is removed nor finalized.
    Value::True
}

/// SQLite3Stmt::reset()
pub fn reset(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let s = this_stmt(this);
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    let Ok((d, id)) = check_both(vm, &s) else {
        return Value::Null;
    };
    let Ok((rc, (code, msg))) = d.with_conn(vm, |db| {
        let rc = sqlite3_reset(db, id);
        (rc, stmt_error(db, Some(id)))
    }) else {
        return Value::Null;
    };
    if rc != SQLITE_OK {
        php_sqlite3_error(
            vm,
            Some(&d.shared),
            code,
            &format!("Unable to reset statement: {}", msg),
        );
        return Value::False;
    }
    Value::True
}

/// SQLite3Stmt::clear()
pub fn clear(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let s = this_stmt(this);
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    let Ok((d, id)) = check_both(vm, &s) else {
        return Value::Null;
    };
    let Ok((rc, (code, msg))) = d.with_conn(vm, |db| {
        let rc = sqlite3_clear_bindings(db, id);
        (rc, stmt_error(db, Some(id)))
    }) else {
        return Value::Null;
    };
    if rc != SQLITE_OK {
        php_sqlite3_error(
            vm,
            Some(&d.shared),
            code,
            &format!("Unable to clear statement: {}", msg),
        );
        return Value::False;
    }
    let old = s.st.borrow_mut().bound_params.take();
    drop(old);
    Value::True
}

/// SQLite3Stmt::readOnly()
pub fn read_only(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let s = this_stmt(this);
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    let Ok((d, id)) = check_both(vm, &s) else {
        return Value::Null;
    };
    match d.with_conn(vm, |db| sqlite3_stmt_readonly(db, id)) {
        Ok(n) => vbool(n != 0),
        Err(()) => Value::Null,
    }
}

/// SQLite3Stmt::busy()
pub fn busy(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let s = this_stmt(this);
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    let Ok((d, id)) = check_both(vm, &s) else {
        return Value::Null;
    };
    match d.with_conn(vm, |db| sqlite3_stmt_busy(db, id)) {
        Ok(n) => vbool(n != 0),
        Err(()) => Value::Null,
    }
}

/// SQLite3Stmt::explain()
pub fn explain(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let s = this_stmt(this);
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    let Ok((d, id)) = check_both(vm, &s) else {
        return Value::Null;
    };
    match d.with_conn(vm, |db| sqlite3_stmt_isexplain(db, id)) {
        Ok(n) => Value::Long(n as i64),
        Err(()) => Value::Null,
    }
}

/// SQLite3Stmt::setExplain()
pub fn set_explain(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let s = this_stmt(this);
    let Ok(a) = parse(vm, f, "l", None, args) else {
        return Value::Null;
    };
    let mode = a[0].long_or(0);
    if !(0..=2).contains(&mode) {
        f.value_error(
            vm,
            1,
            "must be one of the SQLite3Stmt::EXPLAIN_MODE_* constants",
        );
        return Value::Null;
    }
    let Ok((d, id)) = check_both(vm, &s) else {
        return Value::Null;
    };
    match d.with_conn(vm, |db| sqlite3_stmt_explain(db, id, mode as i32)) {
        Ok(rc) => vbool(rc == SQLITE_OK),
        Err(()) => Value::Null,
    }
}

/// What a bound parameter is bound as.
enum Bind {
    Null,
    Int(i64),
    Double(f64),
    Blob(Vec<u8>),
    Text(Vec<u8>),
}

/// Write `v` back into a bound parameter (convert_to_long() & co. convert
/// the referenced variable in place).
fn write_back(slot: &Value, v: Value) {
    if let Value::Reference(r) = slot {
        *r.borrow_mut() = v;
    }
}

/// zval_try_get_tmp_string(): None after an exception
fn try_string(vm: &mut Vm, v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        Value::Array(_) => {
            vm.emit_warning("Array to string conversion");
            Some(b"Array".to_vec())
        }
        _ => match vm.value_to_string_checked(v) {
            Ok(s) if vm.current_exception.is_none() => Some(s.as_bytes().to_vec()),
            _ => None,
        },
    }
}

/// php_sqlite3_bind_params(): Err == FAILURE
fn bind_params(vm: &mut Vm, s: &StmtData, d: &DbData) -> Result<(), ()> {
    let (params, stmt) = {
        let st = s.st.borrow();
        (st.bound_params.clone(), st.stmt)
    };
    let Some(params) = params else {
        return Ok(());
    };
    for (key, param) in params {
        // parameter must be a reference?
        let parameter = param.parameter.deref();
        let number = param.param_number;
        let bind = if matches!(parameter, Value::Null) {
            // If the ZVAL is null then it should be bound as that
            Bind::Null
        } else {
            match param.type_ as i32 {
                SQLITE_INTEGER => {
                    let n = match &parameter {
                        Value::Long(n) => *n,
                        other => other.to_long(),
                    };
                    if !matches!(parameter, Value::Long(_)) {
                        write_back(&param.parameter, Value::Long(n));
                        set_param_value(s, &key, Value::Long(n));
                    }
                    Bind::Int(n)
                }
                SQLITE_FLOAT => {
                    let x = match &parameter {
                        Value::Double(x) => *x,
                        other => other.to_double(),
                    };
                    if !matches!(parameter, Value::Double(_)) {
                        write_back(&param.parameter, Value::Double(x));
                        set_param_value(s, &key, Value::Double(x));
                    }
                    Bind::Double(x)
                }
                SQLITE_BLOB => {
                    if let Value::Resource(id) = &parameter {
                        if !goro_ext_standard::ext_io::is_stream(vm, &parameter) {
                            php_sqlite3_error(
                                vm,
                                Some(&d.shared),
                                0,
                                &format!("Unable to read stream for parameter {}", number),
                            );
                            return Err(());
                        }
                        Bind::Blob(goro_ext_standard::ext_io::read_all(vm, *id))
                    } else {
                        Bind::Blob(try_string(vm, &parameter).unwrap_or_default())
                    }
                }
                SQLITE3_TEXT => match try_string(vm, &parameter) {
                    Some(t) => Bind::Text(t),
                    None => return Err(()),
                },
                SQLITE_NULL => Bind::Null,
                _ => {
                    // php-src formats this message with "%pd", which its
                    // spprintf() rejects with an E_CORE_ERROR bailout.
                    let file = std::mem::replace(&mut vm.current_file, "Unknown".to_string());
                    vm.emit_fatal_typed_at(
                        16,
                        "printf \"p\" modifier is no longer supported, use ZEND_LONG_FMT",
                        0,
                    );
                    vm.current_file = file;
                    vm.exit_requested = true;
                    vm.exit_code = 255;
                    let _ = &d;
                    return Err(());
                }
            }
        };
        let i = number as i32;
        let rc = d.with_conn(vm, |db| match stmt {
            None => SQLITE_MISUSE,
            Some(id) => match &bind {
                Bind::Null => sqlite3_bind_null(db, id, i),
                Bind::Int(n) => sqlite3_bind_int64(db, id, i, *n),
                Bind::Double(x) => sqlite3_bind_double(db, id, i, *x),
                Bind::Blob(b) => sqlite3_bind_blob(db, id, i, b, b.len() as i32),
                Bind::Text(t) => sqlite3_bind_text(db, id, i, Some(t), t.len() as i32),
            },
        })?;
        if rc != SQLITE_OK {
            php_sqlite3_error(
                vm,
                Some(&d.shared),
                rc,
                &format!("Unable to bind parameter number {}", number),
            );
        }
    }
    Ok(())
}

/// Update the stored value of a by-value bound parameter after an in-place
/// conversion.
fn set_param_value(s: &StmtData, key: &ParamKey, v: Value) {
    let mut st = s.st.borrow_mut();
    if let Some(list) = st.bound_params.as_mut() {
        if let Some((_, p)) = list.iter_mut().find(|(k, _)| k == key) {
            if !matches!(p.parameter, Value::Reference(_)) {
                p.parameter = v;
            }
        }
    }
}

/// SQLite3Stmt::getSQL()
pub fn get_sql(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let s = this_stmt(this);
    let Ok(a) = parse(vm, f, "|b", None, args) else {
        return Value::Null;
    };
    let expanded = a[0].bool_or(false);
    let Ok((d, id)) = check_both(vm, &s) else {
        return Value::Null;
    };
    let bind_rc = bind_params(vm, &s, &d);
    if bind_rc.is_err() || vm.current_exception.is_some() {
        return Value::False;
    }
    let Ok(sql) = d.with_conn(vm, |db| {
        if expanded {
            sqlite3_expanded_sql(db, id)
        } else {
            sqlite3_sql(db, id)
        }
    }) else {
        return Value::Null;
    };
    vstr(cstr(&sql.unwrap_or_default()))
}

/// register_bound_parameter_to_sqlite()
fn register_bound_parameter(vm: &mut Vm, mut param: BoundParam, s: &StmtData) -> Result<bool, ()> {
    let Some(d) = s.db() else {
        return Ok(false);
    };
    let stmt = s.st.borrow().stmt;
    // We need a : prefix to resolve a name to a parameter number
    if let Some(name) = param.name.take() {
        let name = if name.first() != Some(&b':') && name.first() != Some(&b'@') {
            let mut t = Vec::with_capacity(name.len() + 1);
            t.push(b':');
            t.extend_from_slice(&name);
            t
        } else {
            name
        };
        // do lookup
        let n = match stmt {
            Some(id) => d.with_conn(vm, |db| {
                sqlite3_bind_parameter_index(db, id, Some(cstr(&name)))
            })?,
            None => 0,
        };
        param.param_number = n as i64;
        param.name = Some(name);
    }

    if param.param_number < 1 {
        return Ok(false);
    }

    let mut st = s.st.borrow_mut();
    let hash = st.bound_params.get_or_insert_with(Vec::new);
    let number = param.param_number;
    hash.retain(|(k, _)| *k != ParamKey::Index(number));
    let key = match &param.name {
        Some(n) => ParamKey::Name(n.clone()),
        None => ParamKey::Index(number),
    };
    match hash.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = param,
        None => hash.push((key, param)),
    }
    Ok(true)
}

/// sqlite3stmt_bind(): the common implementation of ::bindParam() and
/// ::bindValue()
fn bind(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value], by_ref: bool) -> Value {
    let s = this_stmt(this);
    let Ok(a) = parse(vm, f, "zz|z", None, args) else {
        return Value::Null;
    };
    let first = a[0].value().unwrap_or(Value::Null);
    let Ok(key) = str_or_long(vm, f, 1, &first) else {
        return Value::Null;
    };
    let parameter = a[1].value().unwrap_or(Value::Null);
    let type_given = a[2].value();
    let ty = match &type_given {
        Some(v) => match long_arg(vm, f, 3, v) {
            Ok(n) => Some(n),
            Err(()) => return Value::Null,
        },
        None => None,
    };
    if check_both(vm, &s).is_err() {
        return Value::Null;
    }
    let parameter = if by_ref { parameter } else { parameter.deref() };
    let mut param = BoundParam {
        param_number: -1,
        name: None,
        type_: SQLITE3_TEXT as i64,
        parameter: parameter.clone(),
    };
    match key {
        Ok(n) => param.param_number = n,
        Err(name) => param.name = Some(name),
    }
    match ty {
        Some(t) => param.type_ = t,
        // PHP_SQLITE3_SET_TYPE: the type of the zval as passed (a
        // reference for ::bindParam())
        None => {
            param.type_ = match &parameter {
                Value::Long(_) | Value::True | Value::False => SQLITE_INTEGER,
                Value::Double(_) => SQLITE_FLOAT,
                Value::Null => SQLITE_NULL,
                _ => SQLITE_TEXT,
            } as i64
        }
    }
    match register_bound_parameter(vm, param, &s) {
        Ok(true) => Value::True,
        Ok(false) => Value::False,
        Err(()) => Value::Null,
    }
}

/// SQLite3Stmt::bindParam()
pub fn bind_param(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    bind(vm, f, this, args, true)
}

/// SQLite3Stmt::bindValue()
pub fn bind_value(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    bind(vm, f, this, args, false)
}

/// SQLite3Stmt::execute()
pub fn execute(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let s = this_stmt(this);
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    let Ok(d) = check_initialised(vm, &s) else {
        return Value::Null;
    };
    let stmt = s.st.borrow().stmt;

    // Always reset statement before execution, see bug #77051
    if let Some(id) = stmt {
        if d.with_conn(vm, |db| sqlite3_reset(db, id)).is_err() {
            return Value::Null;
        }
    }

    // Bind parameters to the statement
    let bind_rc = bind_params(vm, &s, &d);
    if bind_rc.is_err() || vm.current_exception.is_some() {
        return Value::False;
    }

    let Ok(return_code) = d.with_conn(vm, |db| step(db, stmt)) else {
        return Value::Null;
    };
    match return_code {
        SQLITE_ROW | SQLITE_DONE => {
            if let Some(id) = stmt {
                let _ = d.with_conn(vm, |db| sqlite3_reset(db, id));
            }
            let Some(this_o) = this else {
                return Value::Null;
            };
            let ro = new_object(vm, b"SQLite3Result");
            let rd = attach_result(&ro);
            {
                let mut r = rd.st.borrow_mut();
                r.is_prepared_statement = true;
                r.db_obj = Some(d.clone());
                r.stmt_obj = Some(this_o.clone());
                r.column_names = None;
                r.column_count = -1;
            }
            Value::Object(ro)
        }
        rc => {
            if rc == SQLITE_ERROR {
                if let Some(id) = stmt {
                    let _ = d.with_conn(vm, |db| sqlite3_reset(db, id));
                }
            }
            if vm.current_exception.is_none() {
                if let Ok((code, msg)) = d.with_conn(vm, |db| stmt_error(db, stmt)) {
                    php_sqlite3_error(
                        vm,
                        Some(&d.shared),
                        code,
                        &format!("Unable to execute statement: {}", msg),
                    );
                }
            }
            Value::False
        }
    }
}

/// SQLite3Stmt::__construct()
pub fn construct(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let s = this_stmt(this);
    let Ok(a) = parse(vm, f, "OS", Some((b"sqlite3", "SQLite3")), args) else {
        return Value::Null;
    };
    let Some(db_o) = a[0].obj() else {
        return Value::Null;
    };
    let d = attach_db(&db_o);
    if crate::db::check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    let sql = a[1].str_or(b"").to_vec();
    if sql.is_empty() {
        return Value::False;
    }
    s.st.borrow_mut().db_obj = Some((db_o.clone(), d.clone()));
    let Ok((errcode, stmt, msg)) = d.with_conn(vm, |db| {
        let (rc, stmt) = prepare_v2(db, &sql);
        (
            rc,
            stmt,
            if rc != SQLITE_OK {
                errmsg(db)
            } else {
                String::new()
            },
        )
    }) else {
        return Value::Null;
    };
    if errcode != SQLITE_OK {
        php_sqlite3_error(
            vm,
            Some(&d.shared),
            errcode,
            &format!("Unable to prepare statement: {}", msg),
        );
        return Value::False;
    }
    {
        let mut st = s.st.borrow_mut();
        st.stmt = stmt;
        st.initialised = true;
    }
    free_list_add(&d, &s);
    Value::Null
}
