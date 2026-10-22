//! The SQLite3Result methods.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_sqlite::consts::*;
use goro_sqlite::int::{Sqlite3, StmtId};
use goro_sqlite::vdbeapi::*;

use crate::conn::*;
use crate::db::{column_name, result_unused, step, value_to_zval};
use crate::php_sqlite3_error;
use crate::stmt::stmt_error;
use crate::zpp::{Func, Obj, parse, throw, vstr};
use crate::{PHP_SQLITE3_ASSOC, PHP_SQLITE3_NUM};

fn this_result(this: Option<&Obj>) -> Rc<ResultData> {
    match this {
        Some(o) => attach_result(o),
        None => Rc::new(ResultData::default()),
    }
}

/// SQLITE3_CHECK_INITIALIZED(result_obj->db_obj,
/// result_obj->stmt_obj->initialised, SQLite3Result): the database and the
/// statement (None for a statement that compiled to nothing).
fn check(vm: &mut Vm, r: &ResultData) -> Result<(Rc<DbData>, Option<StmtId>), ()> {
    let (db, stmt_o) = {
        let st = r.st.borrow();
        (st.db_obj.clone(), st.stmt_obj.clone())
    };
    let s = stmt_o.as_ref().and_then(stmt_data);
    if let (Some(d), Some(s)) = (db, s) {
        let st = s.st.borrow();
        if st.initialised {
            return Ok((d, st.stmt));
        }
    }
    throw(
        vm,
        b"Error",
        "The SQLite3Result object has not been correctly initialised or is already closed",
    );
    Err(())
}

/// sqlite3result_clear_column_names_cache()
fn clear_cache(r: &ResultData) {
    let mut st = r.st.borrow_mut();
    st.column_names = None;
    st.column_count = -1;
}

/// sqlite3result_fill_column_names_cache()
fn fill_cache(db: &mut Sqlite3, stmt: Option<StmtId>, n: i32) -> Vec<Vec<u8>> {
    (0..n)
        .map(|i| match stmt {
            Some(id) => column_name(db, id, i).unwrap_or_default(),
            None => Vec::new(),
        })
        .collect()
}

/// php_sqlite3_fetch_one()
fn fetch_one(
    db: &mut Sqlite3,
    stmt: StmtId,
    n_cols: i32,
    names: Option<&[Vec<u8>]>,
    mode: i64,
) -> PhpArray {
    let mut result = PhpArray::new();
    for i in 0..n_cols {
        let data = value_to_zval(db, stmt, i);
        if mode & PHP_SQLITE3_NUM != 0 {
            result.set(goro_core::array::ArrayKey::Int(i as i64), data.clone());
        }
        if mode & PHP_SQLITE3_ASSOC != 0 {
            // the last column of a name wins
            let name = names
                .and_then(|n| n.get(i as usize))
                .cloned()
                .unwrap_or_default();
            let key = Vm::value_to_array_key(Value::String(PhpString::from_vec(name)));
            result.set(key, data);
        }
    }
    result
}

fn arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

/// SQLite3Result::numColumns()
pub fn num_columns(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let r = this_result(this);
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    let Ok((d, stmt)) = check(vm, &r) else {
        return Value::Null;
    };
    let Some(id) = stmt else {
        return Value::Long(0);
    };
    match d.with_conn(vm, |db| sqlite3_column_count(db, id)) {
        Ok(n) => Value::Long(n as i64),
        Err(()) => Value::Null,
    }
}

/// SQLite3Result::columnName()
pub fn column_name_m(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let r = this_result(this);
    let Ok(a) = parse(vm, f, "l", None, args) else {
        return Value::Null;
    };
    let column = a[0].long_or(0) as i32;
    let Ok((d, stmt)) = check(vm, &r) else {
        return Value::Null;
    };
    let Some(id) = stmt else {
        return Value::False;
    };
    match d.with_conn(vm, |db| column_name(db, id, column)) {
        Ok(Some(n)) => vstr(n),
        Ok(None) => Value::False,
        Err(()) => Value::Null,
    }
}

/// SQLite3Result::columnType()
pub fn column_type(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let r = this_result(this);
    let Ok(a) = parse(vm, f, "l", None, args) else {
        return Value::Null;
    };
    let column = a[0].long_or(0) as i32;
    let Ok((d, stmt)) = check(vm, &r) else {
        return Value::Null;
    };
    let Some(id) = stmt else {
        return Value::False;
    };
    match d.with_conn(vm, |db| {
        if sqlite3_data_count(db, id) == 0 {
            None
        } else {
            Some(sqlite3_column_type(db, id, column))
        }
    }) {
        Ok(Some(t)) => Value::Long(t as i64),
        Ok(None) => Value::False,
        Err(()) => Value::Null,
    }
}

/// SQLite3Result::fetchArray()
pub fn fetch_array(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let unused = result_unused(vm, "fetcharray");
    let r = this_result(this);
    let Ok(a) = parse(vm, f, "|l", None, args) else {
        return Value::Null;
    };
    let mode = a[0].long_or(crate::PHP_SQLITE3_BOTH);
    let Ok((d, stmt)) = check(vm, &r) else {
        return Value::Null;
    };
    let Ok(ret) = d.with_conn(vm, |db| step(db, stmt)) else {
        return Value::Null;
    };
    match (ret, stmt) {
        (SQLITE_ROW, Some(id)) => {
            // If there was no return value then just skip fetching
            if unused {
                return Value::False;
            }
            let (count, names) = {
                let st = r.st.borrow();
                (st.column_count, st.column_names.clone())
            };
            let out = d.with_conn(vm, |db| {
                let n_cols = if count == -1 {
                    sqlite3_column_count(db, id)
                } else {
                    count
                };
                // Cache column names to speed up repeated fetchArray calls.
                let names = if mode & PHP_SQLITE3_ASSOC != 0 && names.is_none() {
                    Some(fill_cache(db, stmt, n_cols))
                } else {
                    names
                };
                let row = fetch_one(db, id, n_cols, names.as_deref(), mode);
                (n_cols, names, row)
            });
            let Ok((n_cols, names, row)) = out else {
                return Value::Null;
            };
            let mut st = r.st.borrow_mut();
            st.column_count = n_cols;
            if names.is_some() {
                st.column_names = names;
            }
            arr(row)
        }
        (SQLITE_DONE, _) => Value::False,
        _ => {
            if let Ok((code, msg)) = d.with_conn(vm, |db| stmt_error(db, stmt)) {
                php_sqlite3_error(
                    vm,
                    Some(&d.shared),
                    code,
                    &format!("Unable to execute statement: {}", msg),
                );
            }
            Value::False
        }
    }
}

/// SQLite3Result::fetchAll()
pub fn fetch_all(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let r = this_result(this);
    let Ok(a) = parse(vm, f, "|l", None, args) else {
        return Value::Null;
    };
    let mode = a[0].long_or(crate::PHP_SQLITE3_BOTH);
    let Ok((d, stmt)) = check(vm, &r) else {
        return Value::Null;
    };
    let names = r.st.borrow().column_names.clone();
    let Ok((nb_cols, names)) = d.with_conn(vm, |db| {
        let nb_cols = match stmt {
            Some(id) => sqlite3_column_count(db, id),
            None => 0,
        };
        let names = if mode & PHP_SQLITE3_ASSOC != 0 && names.is_none() {
            Some(fill_cache(db, stmt, nb_cols))
        } else {
            names
        };
        (nb_cols, names)
    }) else {
        return Value::Null;
    };
    {
        let mut st = r.st.borrow_mut();
        if names.is_some() {
            st.column_names = names.clone();
        }
        st.column_count = nb_cols;
    }
    let mut out = PhpArray::new();
    loop {
        let Ok(rc) = d.with_conn(vm, |db| step(db, stmt)) else {
            return Value::Null;
        };
        match (rc, stmt) {
            (SQLITE_ROW, Some(id)) => {
                let Ok(row) =
                    d.with_conn(vm, |db| fetch_one(db, id, nb_cols, names.as_deref(), mode))
                else {
                    return Value::Null;
                };
                out.push(arr(row));
            }
            (SQLITE_DONE, _) => break,
            _ => {
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
                return Value::False;
            }
        }
    }
    arr(out)
}

/// SQLite3Result::reset()
pub fn reset(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let r = this_result(this);
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    let Ok((d, stmt)) = check(vm, &r) else {
        return Value::Null;
    };
    clear_cache(&r);
    let Some(id) = stmt else {
        return Value::True;
    };
    match d.with_conn(vm, |db| sqlite3_reset(db, id)) {
        Ok(SQLITE_OK) => Value::True,
        Ok(_) => Value::False,
        Err(()) => Value::Null,
    }
}

/// SQLite3Result::finalize()
pub fn finalize(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let r = this_result(this);
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    let Ok((d, stmt)) = check(vm, &r) else {
        return Value::Null;
    };
    clear_cache(&r);
    // We need to finalize an internal statement: php-src's
    // zend_llist_del_element() compares the address of the statement
    // object pointer with the sqlite3_stmt pointers of the list, which
    // never matches, so the statement stays as it is.
    if r.st.borrow().is_prepared_statement {
        if let Some(id) = stmt {
            let _ = d.with_conn(vm, |db| sqlite3_reset(db, id));
        }
    }
    Value::True
}
