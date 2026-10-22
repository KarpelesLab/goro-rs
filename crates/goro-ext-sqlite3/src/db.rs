//! The SQLite3 methods.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_sqlite::consts::*;
use goro_sqlite::int::{FuncUserData, Sqlite3, StmtId};
use goro_sqlite::mainapi::*;
use goro_sqlite::vdbeapi::*;

use crate::conn::*;
use crate::zpp::{Arg, Func, Obj, cstr, parse, throw, vbool, vstr};
use crate::{errmsg, php_sqlite3_error};

const NOT_INIT: &str = "The SQLite3 object has not been correctly initialised or is already closed";

/// The payload of `$this`
pub fn this_db(this: Option<&Obj>) -> Rc<DbData> {
    match this {
        Some(o) => attach_db(o),
        None => Rc::new(DbData::default()),
    }
}

/// SQLITE3_CHECK_INITIALIZED(db_obj, db_obj->initialised, SQLite3)
pub fn check_initialised(vm: &mut Vm, d: &DbData) -> Result<(), ()> {
    if !d.st.borrow().initialised {
        throw(vm, b"Error", NOT_INIT);
        return Err(());
    }
    Ok(())
}

/// SQLITE3_CHECK_INITIALIZED(db_obj, db_obj->db, SQLite3)
fn check_has_db(vm: &mut Vm, d: &DbData) -> Result<(), ()> {
    if !d.st.borrow().has_db {
        throw(vm, b"Error", NOT_INIT);
        return Err(());
    }
    Ok(())
}

/// !USED_RET() of the method call running (`method`: lowercase name)
pub fn result_unused(vm: &mut Vm, method: &str) -> bool {
    match vm.call_result_unused.take() {
        Some(n) => {
            let n = n.to_ascii_lowercase();
            n.ends_with(method.as_bytes())
                && (n.len() == method.len() || n[..n.len() - method.len()].ends_with(b":"))
        }
        None => false,
    }
}

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The text of an sqlite3_exec() error (NULL is "(null)" in spprintf).
fn errtext(e: Option<Vec<u8>>) -> String {
    match e {
        Some(m) => s(cstr(&m)),
        None => "(null)".to_string(),
    }
}

/// SQLite3::open() / SQLite3::__construct()
pub fn open(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "p|ls", None, args) else {
        return Value::Null;
    };
    let filename = a[0].str_or(b"").to_vec();
    let flags = a[1].long_or((SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE) as i64);

    if d.st.borrow().initialised {
        throw(vm, b"Exception", "Already initialised DB Object");
        return Value::Null;
    }

    let fullpath: Vec<u8> = if !filename.is_empty() && filename != b":memory:" {
        let Some(full) = goro_core::basedir::expand_filepath(&s(&filename)) else {
            throw(vm, b"Exception", "Unable to expand filepath");
            return Value::Null;
        };
        let name = f.name.clone();
        if !vm.check_open_basedir(&name, &full) {
            throw(
                vm,
                b"Exception",
                &format!("open_basedir prohibits opening {}", full),
            );
            return Value::Null;
        }
        full.into_bytes()
    } else {
        // filename equals "" or ":memory:"
        filename
    };

    let mut db: Option<Box<Sqlite3>> = None;
    let rc = sqlite3_open_v2(Some(&fullpath), &mut db, flags as i32, None);
    if rc != SQLITE_OK {
        let msg = match db.as_deref_mut() {
            Some(c) => errmsg(c),
            None => s(cstr(&sqlite3_errstr(rc).unwrap_or_default())),
        };
        throw(
            vm,
            b"Exception",
            &format!("Unable to open database: {}", msg),
        );
        let had = db.is_some();
        if let Some(mut c) = db {
            sqlite3_close(&mut c);
        }
        if had {
            d.st.borrow_mut().has_db = true;
        }
        return Value::Null;
    }
    let mut db = db.expect("sqlite3_open_v2() succeeded");

    *d.shared.authorizer_fcc.borrow_mut() = None;
    goro_sqlite::auth::sqlite3_set_authorizer(
        &mut db,
        Some(crate::callbacks::authorizer(d.shared.clone())),
    );
    if crate::ini_value(vm, "sqlite3.defensive").is_some_and(|v| crate::ini_bool(&v)) {
        sqlite3_db_config(&mut db, SQLITE_DBCONFIG_DEFENSIVE, 1, None);
    }
    d.conn_ptr.set(&*db as *const Sqlite3 as usize);
    match d.conn.try_borrow_mut() {
        Ok(mut c) => *c = Some(db),
        Err(_) => {
            throw(vm, b"Exception", "Unable to open database: busy");
            return Value::Null;
        }
    }
    let mut st = d.st.borrow_mut();
    st.initialised = true;
    st.has_db = true;
    Value::Null
}

/// SQLite3::close()
pub fn close(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    if d.st.borrow().initialised {
        free_list_clean(&d);
        let has_conn = d.conn.try_borrow().map(|c| c.is_some()).unwrap_or(true);
        if has_conn {
            let Ok((errcode, msg)) = d.with_conn(vm, |db| {
                let rc = sqlite3_close(db);
                (
                    rc,
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
                    &format!("Unable to close database: {}", msg),
                );
                return Value::False;
            }
            if let Ok(mut c) = d.conn.try_borrow_mut() {
                *c = None;
            }
        }
        d.st.borrow_mut().initialised = false;
    }
    Value::True
}

/// SQLite3::exec()
pub fn exec(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "S", None, args) else {
        return Value::Null;
    };
    if check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    let sql = a[0].str_or(b"").to_vec();
    let Ok((errcode, err)) = d.with_conn(vm, |db| {
        let mut err: Option<Vec<u8>> = None;
        let rc = goro_sqlite::legacy::sqlite3_exec(db, Some(cstr(&sql)), None, &mut err);
        (rc, err)
    }) else {
        return Value::Null;
    };
    if errcode != SQLITE_OK {
        php_sqlite3_error(vm, Some(&d.shared), errcode, &errtext(err));
        return Value::False;
    }
    Value::True
}

/// SQLite3::version()
pub fn version(vm: &mut Vm, f: &Func, _this: Option<&Obj>, args: &[Value]) -> Value {
    if parse(vm, f, "", None, args).is_err() {
        return Value::Null;
    }
    let mut arr = PhpArray::new();
    let v = sqlite3_libversion().unwrap_or_default();
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"versionString")),
        vstr(cstr(&v)),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"versionNumber")),
        Value::Long(sqlite3_libversion_number() as i64),
    );
    Value::Array(Rc::new(RefCell::new(arr)))
}

/// SQLite3::lastInsertRowID()
pub fn last_insert_rowid(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    if parse(vm, f, "", None, args).is_err() || check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    match d.with_conn(vm, sqlite3_last_insert_rowid) {
        Ok(n) => Value::Long(n),
        Err(()) => Value::Null,
    }
}

/// SQLite3::lastErrorCode()
pub fn last_error_code(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    if parse(vm, f, "", None, args).is_err() || check_has_db(vm, &d).is_err() {
        return Value::Null;
    }
    if !d.st.borrow().initialised {
        return Value::Long(0);
    }
    match d.with_conn(vm, sqlite3_errcode) {
        Ok(n) => Value::Long(n as i64),
        Err(()) => Value::Null,
    }
}

/// SQLite3::lastExtendedErrorCode()
pub fn last_extended_error_code(
    vm: &mut Vm,
    f: &Func,
    this: Option<&Obj>,
    args: &[Value],
) -> Value {
    let d = this_db(this);
    if parse(vm, f, "", None, args).is_err() || check_has_db(vm, &d).is_err() {
        return Value::Null;
    }
    if !d.st.borrow().initialised {
        return Value::Long(0);
    }
    match d.with_conn(vm, sqlite3_extended_errcode) {
        Ok(n) => Value::Long(n as i64),
        Err(()) => Value::Null,
    }
}

/// SQLite3::enableExtendedResultCodes()
pub fn enable_extended_result_codes(
    vm: &mut Vm,
    f: &Func,
    this: Option<&Obj>,
    args: &[Value],
) -> Value {
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "|b", None, args) else {
        return Value::Null;
    };
    let enable = a[0].bool_or(true);
    if check_has_db(vm, &d).is_err() {
        return Value::Null;
    }
    if d.st.borrow().initialised {
        match d.with_conn(vm, |db| {
            sqlite3_extended_result_codes(db, if enable { 1 } else { 0 })
        }) {
            Ok(SQLITE_OK) => return Value::True,
            Ok(_) => {}
            Err(()) => return Value::Null,
        }
    }
    Value::False
}

/// SQLite3::lastErrorMsg()
pub fn last_error_msg(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    if parse(vm, f, "", None, args).is_err() || check_has_db(vm, &d).is_err() {
        return Value::Null;
    }
    if !d.st.borrow().initialised {
        return vstr(b"");
    }
    match d.with_conn(vm, |db| sqlite3_errmsg(db).unwrap_or_default()) {
        Ok(m) => vstr(cstr(&m)),
        Err(()) => Value::Null,
    }
}

/// SQLite3::busyTimeout()
pub fn busy_timeout(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "l", None, args) else {
        return Value::Null;
    };
    if check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    let ms = a[0].long_or(0) as i32;
    if d.with_conn(vm, |db| sqlite3_busy_timeout(db, ms)).is_err() {
        return Value::Null;
    }
    Value::True
}

/// VCWD_REALPATH()
fn realpath(p: &[u8]) -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(p));
    std::fs::canonicalize(path)
        .ok()
        .map(|p| p.as_os_str().as_bytes().to_vec())
}

/// SQLite3::loadExtension()
pub fn load_extension(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "s", None, args) else {
        return Value::Null;
    };
    if check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    let extension = a[0].str_or(b"").to_vec();
    let Some(extension_dir) = crate::ini_value(vm, "sqlite3.extension_dir") else {
        php_sqlite3_error(vm, Some(&d.shared), 0, "SQLite Extensions are disabled");
        return Value::False;
    };
    if extension.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Value::Null;
    }
    let extension_dir = cstr(&extension_dir).to_vec();
    let extension = cstr(&extension).to_vec();
    let mut lib_path = extension_dir.clone();
    if extension_dir.last() != Some(&b'/') {
        lib_path.push(b'/');
    }
    lib_path.extend_from_slice(&extension);

    let Some(fullpath) = realpath(&lib_path) else {
        php_sqlite3_error(
            vm,
            Some(&d.shared),
            0,
            &format!("Unable to load extension at '{}'", s(&lib_path)),
        );
        return Value::False;
    };

    if !fullpath.starts_with(&extension_dir) {
        php_sqlite3_error(
            vm,
            Some(&d.shared),
            0,
            "Unable to open extensions outside the defined directory",
        );
        return Value::False;
    }

    // Extension loading should only be enabled for when we attempt to load
    let Ok((rc, err)) = d.with_conn(vm, |db| {
        goro_sqlite::loadext::sqlite3_enable_load_extension(db, 1);
        let mut err: Option<Vec<u8>> = None;
        let rc = goro_sqlite::loadext::sqlite3_load_extension(db, Some(&fullpath), None, &mut err);
        goro_sqlite::loadext::sqlite3_enable_load_extension(db, 0);
        (rc, err)
    }) else {
        return Value::Null;
    };
    if rc != SQLITE_OK {
        php_sqlite3_error(vm, Some(&d.shared), 0, &errtext(err));
        return Value::False;
    }
    Value::True
}

/// SQLite3::changes()
pub fn changes(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    if parse(vm, f, "", None, args).is_err() || check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    match d.with_conn(vm, sqlite3_changes) {
        Ok(n) => Value::Long(n as i64),
        Err(()) => Value::Null,
    }
}

/// SQLite3::escapeString()
pub fn escape_string(vm: &mut Vm, f: &Func, _this: Option<&Obj>, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "S", None, args) else {
        return Value::Null;
    };
    let sql = a[0].str_or(b"");
    if sql.is_empty() {
        return vstr(b"");
    }
    let r = goro_sqlite::printf::sqlite3_mprintf(
        b"%q",
        &[goro_sqlite::printf::PfArg::Str(Some(cstr(sql)))],
    );
    vstr(cstr(&r))
}

/// sqlite3_prepare_v2(db, sql, len, &stmt, NULL)
pub fn prepare_v2(db: &mut Sqlite3, sql: &[u8]) -> (i32, Option<StmtId>) {
    let mut stmt: Option<StmtId> = None;
    let mut tail = 0usize;
    let rc = goro_sqlite::prepare::sqlite3_prepare_v2(
        db,
        Some(sql),
        sql.len() as i32,
        &mut stmt,
        &mut tail,
    );
    (rc, stmt)
}

/// object_init_ex(SQLite3Stmt) with stmt_obj->db_obj = db_obj and
/// Z_ADDREF(db)
pub fn new_stmt_object(vm: &mut Vm, db_obj: &Obj, d: &Rc<DbData>) -> (Obj, Rc<StmtData>) {
    let o = new_object(vm, b"SQLite3Stmt");
    let s = attach_stmt(&o);
    s.st.borrow_mut().db_obj = Some((db_obj.clone(), d.clone()));
    (o, s)
}

/// zend_llist_add_element(&db_obj->free_list, &stmt_obj)
pub fn free_list_add(d: &DbData, s: &Rc<StmtData>) {
    let stmt = s.st.borrow().stmt;
    d.st.borrow_mut().free_list.push(FreeEntry {
        stmt_obj: Rc::downgrade(s),
        stmt,
    });
}

/// SQLite3::prepare()
pub fn prepare(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "S", None, args) else {
        return Value::Null;
    };
    if check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    let sql = a[0].str_or(b"").to_vec();
    if sql.is_empty() {
        return Value::False;
    }
    let Some(this_o) = this else {
        return Value::Null;
    };
    let (o, sd) = new_stmt_object(vm, this_o, &d);
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
        let mut st = sd.st.borrow_mut();
        st.stmt = stmt;
        st.initialised = true;
    }
    free_list_add(&d, &sd);
    Value::Object(o)
}

/// sqlite3_step() of a statement (NULL: SQLITE_MISUSE)
pub fn step(db: &mut Sqlite3, stmt: Option<StmtId>) -> i32 {
    match stmt {
        Some(id) => sqlite3_step(db, id),
        None => SQLITE_MISUSE,
    }
}

/// SQLite3::query()
pub fn query(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let unused = result_unused(vm, "query");
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "S", None, args) else {
        return Value::Null;
    };
    if check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    let sql = a[0].str_or(b"").to_vec();
    if sql.is_empty() {
        return Value::False;
    }

    // If there was no return value then just execute the query
    if unused {
        let Ok((errcode, err)) = d.with_conn(vm, |db| {
            let mut err: Option<Vec<u8>> = None;
            let rc = goro_sqlite::legacy::sqlite3_exec(db, Some(cstr(&sql)), None, &mut err);
            (rc, err)
        }) else {
            return Value::Null;
        };
        if errcode != SQLITE_OK {
            php_sqlite3_error(vm, Some(&d.shared), errcode, &errtext(err));
        }
        return Value::False;
    }

    let Some(this_o) = this else {
        return Value::Null;
    };
    let (stmt_o, sd) = new_stmt_object(vm, this_o, &d);
    let Ok((rc, stmt, msg)) = d.with_conn(vm, |db| {
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
    if rc != SQLITE_OK {
        php_sqlite3_error(
            vm,
            Some(&d.shared),
            rc,
            &format!("Unable to prepare statement: {}", msg),
        );
        return Value::False;
    }
    {
        let mut st = sd.st.borrow_mut();
        st.stmt = stmt;
        st.initialised = true;
    }

    let ro = new_object(vm, b"SQLite3Result");
    let rd = attach_result(&ro);
    {
        let mut r = rd.st.borrow_mut();
        r.db_obj = Some(d.clone());
        r.stmt_obj = Some(stmt_o);
        r.column_names = None;
        r.column_count = -1;
    }

    let Ok(return_code) = d.with_conn(vm, |db| step(db, stmt)) else {
        return Value::Null;
    };
    match return_code {
        SQLITE_ROW | SQLITE_DONE => {
            free_list_add(&d, &sd);
            if let Some(id) = stmt {
                let _ = d.with_conn(vm, |db| sqlite3_reset(db, id));
            }
            Value::Object(ro)
        }
        _ => {
            if vm.current_exception.is_none() {
                if let Ok((code, msg)) = d.with_conn(vm, |db| (sqlite3_errcode(db), errmsg(db))) {
                    php_sqlite3_error(
                        vm,
                        Some(&d.shared),
                        code,
                        &format!("Unable to execute statement: {}", msg),
                    );
                }
            }
            if let Some(id) = stmt {
                let _ = d.with_conn(vm, |db| sqlite3_finalize(db, id));
            }
            sd.st.borrow_mut().initialised = false;
            Value::False
        }
    }
}

/// sqlite_value_to_zval()
pub fn value_to_zval(db: &mut Sqlite3, stmt: StmtId, column: i32) -> Value {
    match sqlite3_column_type(db, stmt, column) {
        SQLITE_INTEGER => Value::Long(sqlite3_column_int64(db, stmt, column)),
        SQLITE_FLOAT => Value::Double(sqlite3_column_double(db, stmt, column)),
        SQLITE_NULL => Value::Null,
        SQLITE3_TEXT => {
            let t = sqlite3_column_text(db, stmt, column).unwrap_or_default();
            vstr(cstr(&t))
        }
        _ => {
            let b = sqlite3_column_blob(db, stmt, column).unwrap_or_default();
            let n = sqlite3_column_bytes(db, stmt, column).max(0) as usize;
            vstr(&b[..n.min(b.len())])
        }
    }
}

/// The name of column `i` (a C string)
pub fn column_name(db: &mut Sqlite3, stmt: StmtId, i: i32) -> Option<Vec<u8>> {
    sqlite3_column_name(db, stmt, i).map(|n| cstr(&n).to_vec())
}

/// SQLite3::querySingle()
pub fn query_single(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let unused = result_unused(vm, "querysingle");
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "S|b", None, args) else {
        return Value::Null;
    };
    if check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    let sql = a[0].str_or(b"").to_vec();
    let entire_row = a[1].bool_or(false);
    if sql.is_empty() {
        return Value::False;
    }

    // If there was no return value then just execute the query
    if unused {
        let Ok((errcode, err)) = d.with_conn(vm, |db| {
            let mut err: Option<Vec<u8>> = None;
            let rc = goro_sqlite::legacy::sqlite3_exec(db, Some(cstr(&sql)), None, &mut err);
            (rc, err)
        }) else {
            return Value::Null;
        };
        if errcode != SQLITE_OK {
            php_sqlite3_error(vm, Some(&d.shared), errcode, &errtext(err));
        }
        return Value::False;
    }

    let Ok((rc, stmt, msg)) = d.with_conn(vm, |db| {
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
    if rc != SQLITE_OK {
        php_sqlite3_error(
            vm,
            Some(&d.shared),
            rc,
            &format!("Unable to prepare statement: {}", msg),
        );
        return Value::False;
    }

    let Ok(return_code) = d.with_conn(vm, |db| step(db, stmt)) else {
        return Value::Null;
    };
    let ret = match (return_code, stmt) {
        (SQLITE_ROW, Some(id)) => {
            let r = d.with_conn(vm, |db| {
                if !entire_row {
                    value_to_zval(db, id, 0)
                } else {
                    let count = sqlite3_data_count(db, id);
                    let mut arr = PhpArray::new();
                    for i in 0..count {
                        let data = value_to_zval(db, id, i);
                        let name = column_name(db, id, i).unwrap_or_default();
                        arr.set(ArrayKey::String(PhpString::from_vec(name)), data);
                    }
                    Value::Array(Rc::new(RefCell::new(arr)))
                }
            });
            r.unwrap_or(Value::Null)
        }
        (SQLITE_DONE, _) => {
            if !entire_row {
                Value::Null
            } else {
                Value::Array(Rc::new(RefCell::new(PhpArray::new())))
            }
        }
        _ => {
            if vm.current_exception.is_none() {
                if let Ok((code, msg)) = d.with_conn(vm, |db| (sqlite3_errcode(db), errmsg(db))) {
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
    };
    if let Some(id) = stmt {
        let _ = d.with_conn(vm, |db| sqlite3_finalize(db, id));
    }
    ret
}

fn name_or_false(a: &Arg) -> Vec<u8> {
    a.str_or(b"").to_vec()
}

/// SQLite3::createFunction()
pub fn create_function(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "SF|ll", None, args) else {
        return Value::Null;
    };
    if check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    let sql_func = name_or_false(&a[0]);
    if sql_func.is_empty() {
        return Value::False;
    }
    let callback = a[1].value();
    let argc = a[2].long_or(-1) as i32;
    let flags = a[3].long_or(0) as i32;

    let func = Rc::new(FuncEntry {
        func_name: sql_func.clone(),
        argc,
        func: RefCell::new(None),
        step: RefCell::new(None),
        fini: RefCell::new(None),
    });
    let xfunc = crate::callbacks::scalar_func(func.clone());
    let Ok(rc) = d.with_conn(vm, |db| {
        sqlite3_create_function(
            db,
            Some(cstr(&sql_func)),
            argc,
            flags | SQLITE_UTF8 as i32,
            FuncUserData::None,
            Some(xfunc),
            None,
            None,
        )
    }) else {
        return Value::Null;
    };
    if rc == SQLITE_OK {
        *func.func.borrow_mut() = callback;
        d.st.borrow_mut().funcs.push(func);
        return Value::True;
    }
    Value::False
}

/// SQLite3::createAggregate()
pub fn create_aggregate(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "SFF|l", None, args) else {
        return Value::False;
    };
    if !d.st.borrow().initialised {
        throw(vm, b"Error", NOT_INIT);
        return Value::False;
    }
    let sql_func = name_or_false(&a[0]);
    if sql_func.is_empty() {
        return Value::False;
    }
    let step_cb = a[1].value();
    let fini_cb = a[2].value();
    let argc = a[3].long_or(-1) as i32;

    let func = Rc::new(FuncEntry {
        func_name: sql_func.clone(),
        argc,
        func: RefCell::new(None),
        step: RefCell::new(None),
        fini: RefCell::new(None),
    });
    let xstep = crate::callbacks::step_func(func.clone());
    let xfinal = crate::callbacks::final_func(func.clone());
    let Ok(rc) = d.with_conn(vm, |db| {
        sqlite3_create_function(
            db,
            Some(cstr(&sql_func)),
            argc,
            SQLITE_UTF8 as i32,
            FuncUserData::None,
            None,
            Some(xstep),
            Some(xfinal),
        )
    }) else {
        return Value::Null;
    };
    if rc == SQLITE_OK {
        *func.step.borrow_mut() = step_cb;
        *func.fini.borrow_mut() = fini_cb;
        d.st.borrow_mut().funcs.push(func);
        return Value::True;
    }
    Value::False
}

/// SQLite3::createCollation()
pub fn create_collation(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "SF", None, args) else {
        return Value::Null;
    };
    if check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    let name = name_or_false(&a[0]);
    if name.is_empty() {
        return Value::False;
    }
    let coll = Rc::new(CollEntry {
        collation_name: name.clone(),
        cmp_func: RefCell::new(None),
    });
    let xcmp = crate::callbacks::compare_func(coll.clone());
    let Ok(rc) = d.with_conn(vm, |db| {
        sqlite3_create_collation(db, Some(cstr(&name)), SQLITE_UTF8 as i32, Some(xcmp))
    }) else {
        return Value::Null;
    };
    if rc == SQLITE_OK {
        *coll.cmp_func.borrow_mut() = a[1].value();
        d.st.borrow_mut().collations.push(coll);
        return Value::True;
    }
    Value::False
}

/// SQLite3::enableExceptions()
pub fn enable_exceptions(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "|b", None, args) else {
        return Value::Null;
    };
    let enable = a[0].bool_or(false);
    let ret = vbool(d.shared.exception.get());
    if !enable {
        let m = format!("{}(): Use of warnings for SQLite3 is deprecated", f.name);
        vm.emit_deprecated(&m);
    }
    d.shared.exception.set(enable);
    ret
}

/// SQLite3::setAuthorizer()
pub fn set_authorizer(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "f!", None, args) else {
        return Value::Null;
    };
    let d = this_db(this);
    if check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    // Clear previously set callback; only enable userland authorizer if
    // argument is not NULL
    *d.shared.authorizer_fcc.borrow_mut() = a[0].value();
    Value::True
}

/// SQLite3::backup()
pub fn backup(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "O|pp", Some((b"sqlite3", "SQLite3")), args) else {
        return Value::Null;
    };
    let source = this_db(this);
    if check_initialised(vm, &source).is_err() {
        return Value::Null;
    }
    let Some(dest_o) = a[0].obj() else {
        return Value::Null;
    };
    let dest = attach_db(&dest_o);
    if check_initialised(vm, &dest).is_err() {
        return Value::Null;
    }
    let source_dbname = a[1].str_or(b"main").to_vec();
    let destination_dbname = a[2].str_or(b"main").to_vec();

    let Ok((rc, msg)) = source.with_two_conns(vm, &dest, |src, dst| {
        use goro_sqlite::backup::*;
        let rc = match dst {
            Some(dst) => {
                match sqlite3_backup_init(
                    dst,
                    Some(&destination_dbname),
                    Some(src),
                    Some(&source_dbname),
                ) {
                    Some(bk) => {
                        let mut rc;
                        loop {
                            rc = sqlite3_backup_step(dst, src, &bk, -1);
                            if rc != SQLITE_OK {
                                break;
                            }
                        }
                        sqlite3_backup_finish(dst, src, bk)
                    }
                    None => sqlite3_errcode(src),
                }
            }
            None => {
                match sqlite3_backup_init(
                    src,
                    Some(&destination_dbname),
                    None,
                    Some(&source_dbname),
                ) {
                    Some(_) => SQLITE_MISUSE,
                    None => sqlite3_errcode(src),
                }
            }
        };
        (rc, errmsg(src))
    }) else {
        return Value::Null;
    };

    if rc != SQLITE_OK {
        let m = if rc == SQLITE_BUSY {
            "Backup failed: source database is busy".to_string()
        } else if rc == SQLITE_LOCKED {
            "Backup failed: source database is locked".to_string()
        } else {
            format!("Backup failed: {}", msg)
        };
        php_sqlite3_error(vm, Some(&source.shared), rc, &m);
        return Value::False;
    }
    Value::True
}
