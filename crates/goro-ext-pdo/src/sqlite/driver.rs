//! The SQLite database handle (ext/pdo_sqlite/sqlite_driver.c) over
//! goro-sqlite, the pure-Rust SQLite port.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_mysqli::zpp::{Arg, Func, parse};
use goro_sqlite::consts::*;
use goro_sqlite::int::{Context, FuncUserData, Mem, Sqlite3, XAuth, XCmp, XFinal, XSFunc};
use goro_sqlite::{mainapi, vdbeapi};

use super::statement::{S, STMT_METHODS};
use super::*;
use crate::core::*;

fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}

/// pdo_sqlite_error_info
#[derive(Clone, Debug, Default)]
pub struct EInfo {
    pub errcode: u32,
    pub errmsg: Option<Vec<u8>>,
}

/// pdo_sqlite_db_handle
pub struct H {
    pub db: Option<ConnRef>,
    pub einfo: EInfo,
    /// struct pdo_sqlite_func (name, argc): the callbacks themselves live
    /// in the closures registered with the connection
    pub funcs: Vec<(Vec<u8>, i32)>,
    /// struct pdo_sqlite_collation (names)
    pub collations: Vec<Vec<u8>>,
    /// authorizer_fcc (shared with the authorizer closure)
    pub authorizer: Rc<RefCell<Option<Value>>>,
    pub transaction_mode: i64,
}

/// Run `f` on the SQLite handle of a PDO handle
pub fn with_h<R>(dbh: &DbhRef, f: impl FnOnce(&mut H) -> R) -> Option<R> {
    let mut d = dbh.try_borrow_mut().ok()?;
    let h = d.driver_data.as_mut()?.downcast_mut::<H>()?;
    Some(f(h))
}

/// H->db
pub fn conn_of(dbh: &DbhRef) -> Option<ConnRef> {
    with_h(dbh, |h| h.db.clone()).flatten()
}

/// (sqlite3_errcode, sqlite3_errmsg) of the connection; a connection that
/// is busy running a callback reports a misuse.
fn db_error(conn: Option<&ConnRef>) -> (i32, Vec<u8>) {
    let misuse = || {
        (
            SQLITE_MISUSE,
            mainapi::sqlite3ErrStr(SQLITE_MISUSE).unwrap_or_default(),
        )
    };
    let Some(conn) = conn else {
        return misuse();
    };
    conn.with(|db| {
        (
            mainapi::sqlite3_errcode(db),
            mainapi::sqlite3_errmsg(db).unwrap_or_default(),
        )
    })
    .unwrap_or_else(misuse)
}

/// _pdo_sqlite_error
pub fn sqlite_error(vm: Option<&mut Vm>, dbh: &DbhRef, stmt: Option<&StmtRef>) -> i32 {
    let conn = conn_of(dbh);
    let (code, msg) = db_error(conn.as_ref());
    sqlite_error_code(vm, dbh, stmt, code, Some(msg))
}

/// _pdo_sqlite_error with H->einfo.errcode set to `code` (and the message
/// read from the connection, as the C code does)
pub fn sqlite_error_code(
    vm: Option<&mut Vm>,
    dbh: &DbhRef,
    stmt: Option<&StmtRef>,
    code: i32,
    msg: Option<Vec<u8>>,
) -> i32 {
    let errcode = code as u32;
    let errmsg = with_h(dbh, |h| {
        h.einfo.errcode = errcode;
        if errcode != SQLITE_OK as u32 {
            h.einfo.errmsg = msg.clone();
        }
        h.einfo.errmsg.clone()
    })
    .flatten();
    let state: &[u8] = if errcode == SQLITE_OK as u32 {
        PDO_ERR_NONE
    } else {
        match code {
            SQLITE_NOTFOUND => b"42S02",
            SQLITE_INTERRUPT => b"01002",
            SQLITE_NOLFS => b"HYC00",
            SQLITE_TOOBIG => b"22001",
            SQLITE_CONSTRAINT => b"23000",
            _ => b"HY000",
        }
    };
    match stmt {
        Some(s) => s.borrow_mut().error_code = state.to_vec(),
        None => dbh.borrow_mut().error_code = state.to_vec(),
    }
    if errcode == SQLITE_OK as u32 {
        return 0;
    }
    if dbh.borrow().methods.is_none() {
        if let Some(vm) = vm {
            throw_exception(vm, errcode, errmsg.as_deref().unwrap_or(b""), state);
        }
    }
    errcode as i32
}

/// pdo_sqlite_fetch_error_func
fn fetch_error_func(_vm: &mut Vm, dbh: &DbhRef, _stmt: Option<&StmtRef>, info: &mut PhpArray) {
    let einfo = with_h(dbh, |h| h.einfo.clone()).unwrap_or_default();
    if einfo.errcode != 0 {
        info.push(Value::Long(einfo.errcode as i64));
        info.push(vstr(einfo.errmsg.unwrap_or_default()));
    }
}

/// pdo_sqlite_cleanup_callbacks
pub fn cleanup_callbacks(dbh: &DbhRef) {
    let taken = with_h(dbh, |h| {
        h.authorizer.borrow_mut().take();
        (
            std::mem::take(&mut h.funcs),
            std::mem::take(&mut h.collations),
            h.db.clone(),
        )
    });
    let Some((funcs, collations, conn)) = taken else {
        return;
    };
    let Some(conn) = conn else {
        return;
    };
    // (most recently registered first, like the C list)
    let _ = conn.with(|db| {
        for (name, argc) in funcs.iter().rev() {
            // delete the function from the handle
            mainapi::sqlite3_create_function(
                db,
                Some(name),
                *argc,
                SQLITE_UTF8 as i32,
                FuncUserData::None,
                None,
                None,
                None,
            );
        }
        for name in collations.iter().rev() {
            // delete the collation from the handle
            mainapi::sqlite3_create_collation(db, Some(name), SQLITE_UTF8 as i32, None);
        }
    });
}

/// sqlite_handle_closer
fn handle_closer(dbh: &DbhRef) {
    let has = with_h(dbh, |_| ()).is_some();
    if !has {
        return;
    }
    cleanup_callbacks(dbh);
    let h = dbh.borrow_mut().driver_data.take();
    if let Some(h) = h {
        if let Ok(mut h) = h.downcast::<H>() {
            if let Some(conn) = h.db.take() {
                conn.close();
            }
        }
    }
}

/// sqlite_handle_preparer
fn handle_preparer(
    vm: &mut Vm,
    dbh: &DbhRef,
    sql: &[u8],
    stmt: &StmtRef,
    driver_options: Option<&Arr>,
) -> bool {
    let conn = conn_of(dbh);
    {
        let mut st = stmt.borrow_mut();
        st.driver_data = Some(Box::new(S {
            conn: conn.clone(),
            ..S::default()
        }));
        st.methods = Some(&STMT_METHODS);
        st.supports_placeholders = PDO_PLACEHOLDER_POSITIONAL | PDO_PLACEHOLDER_NAMED;
    }
    if attr_lval(driver_options, PDO_ATTR_CURSOR, PDO_CURSOR_FWDONLY) != PDO_CURSOR_FWDONLY {
        let (_, msg) = db_error(conn.as_ref());
        sqlite_error_code(Some(vm), dbh, None, SQLITE_ERROR, Some(msg));
        return false;
    }
    let Some(conn) = conn else {
        return false;
    };
    let r = run(vm, &conn, |db| {
        let mut id = None;
        let mut tail = 0usize;
        let rc = goro_sqlite::prepare::sqlite3_prepare_v2(
            db,
            Some(sql),
            sql.len() as i32,
            &mut id,
            &mut tail,
        );
        (rc, id)
    });
    match r {
        Some((SQLITE_OK, id)) => {
            super::statement::with_s(stmt, |s| s.stmt = id);
            true
        }
        Some(_) => {
            sqlite_error(Some(vm), dbh, None);
            false
        }
        None => {
            busy_error(vm, dbh, None);
            false
        }
    }
}

/// The connection is running a callback (no re-entrant use): reported as
/// SQLITE_MISUSE.
pub fn busy_error(vm: &mut Vm, dbh: &DbhRef, stmt: Option<&StmtRef>) {
    sqlite_error_code(
        Some(vm),
        dbh,
        stmt,
        SQLITE_MISUSE,
        mainapi::sqlite3ErrStr(SQLITE_MISUSE),
    );
}

/// sqlite3_exec(db, sql, NULL, NULL, NULL): None when the connection is
/// unusable
fn exec(vm: Option<&mut Vm>, conn: &ConnRef, sql: &[u8]) -> Option<i32> {
    let f = |db: &mut Sqlite3| {
        let mut err = None;
        goro_sqlite::legacy::sqlite3_exec(db, Some(sql), None, &mut err)
    };
    match vm {
        Some(vm) => run(vm, conn, f),
        None => run_last(conn, f),
    }
}

/// sqlite_handle_doer
fn handle_doer(vm: &mut Vm, dbh: &DbhRef, sql: &[u8]) -> i64 {
    let Some(conn) = conn_of(dbh) else {
        return -1;
    };
    match exec(Some(vm), &conn, sql) {
        Some(SQLITE_OK) => conn
            .with(|db| mainapi::sqlite3_changes(db) as i64)
            .unwrap_or(0),
        Some(_) => {
            sqlite_error(Some(vm), dbh, None);
            -1
        }
        None => {
            busy_error(vm, dbh, None);
            -1
        }
    }
}

/// pdo_sqlite_last_insert_id
fn last_insert_id(_vm: &mut Vm, dbh: &DbhRef, _name: Option<&[u8]>) -> Option<Vec<u8>> {
    let conn = conn_of(dbh)?;
    let id = conn.with(mainapi::sqlite3_last_insert_rowid)?;
    Some(id.to_string().into_bytes())
}

/// sqlite_handle_quoter (NB: doesn't handle binary strings... use prepared
/// stmts for that)
fn handle_quoter(vm: &mut Vm, dbh: &DbhRef, unquoted: &[u8], _paramtype: i64) -> Option<Vec<u8>> {
    if unquoted.len() > (i32::MAX as usize - 3) / 2 {
        return None;
    }
    if unquoted.contains(&0) {
        let mode = dbh.borrow().error_mode;
        if mode == PDO_ERRMODE_EXCEPTION {
            throw_pdo(vm, "SQLite PDO::quote does not support null bytes");
        } else if mode == PDO_ERRMODE_WARNING {
            docref_warning(vm, "SQLite PDO::quote does not support null bytes");
        }
        return None;
    }
    let q = goro_sqlite::printf::sqlite3_snprintf(
        (2 * unquoted.len() + 3) as i32,
        b"'%q'",
        &[goro_sqlite::printf::PfArg::Str(Some(unquoted))],
    );
    let n = q.iter().position(|&c| c == 0).unwrap_or(q.len());
    Some(q[..n].to_vec())
}

fn transaction_cmd(dbh: &DbhRef, cmd: &[u8]) -> bool {
    let Some(conn) = conn_of(dbh) else {
        return false;
    };
    match exec(None, &conn, cmd) {
        Some(SQLITE_OK) => true,
        Some(_) => {
            sqlite_error(None, dbh, None);
            false
        }
        None => {
            sqlite_error_code(
                None,
                dbh,
                None,
                SQLITE_MISUSE,
                mainapi::sqlite3ErrStr(SQLITE_MISUSE),
            );
            false
        }
    }
}

/// sqlite_handle_begin
fn handle_begin(dbh: &DbhRef) -> bool {
    let mode = with_h(dbh, |h| h.transaction_mode).unwrap_or(0);
    let begin: &[u8] = match mode {
        PDO_SQLITE_TRANSACTION_MODE_DEFERRED => b"BEGIN DEFERRED TRANSACTION",
        PDO_SQLITE_TRANSACTION_MODE_IMMEDIATE => b"BEGIN IMMEDIATE TRANSACTION",
        PDO_SQLITE_TRANSACTION_MODE_EXCLUSIVE => b"BEGIN EXCLUSIVE TRANSACTION",
        _ => b"BEGIN",
    };
    transaction_cmd(dbh, begin)
}

/// sqlite_handle_commit
fn handle_commit(dbh: &DbhRef) -> bool {
    transaction_cmd(dbh, b"COMMIT")
}

/// sqlite_handle_rollback
fn handle_rollback(dbh: &DbhRef) -> bool {
    transaction_cmd(dbh, b"ROLLBACK")
}

/// pdo_sqlite_get_attribute
fn get_attribute(_vm: &mut Vm, dbh: &DbhRef, attr: i64) -> AttrResult {
    match attr {
        PDO_ATTR_CLIENT_VERSION | PDO_ATTR_SERVER_VERSION => {
            AttrResult::Value(vstr(mainapi::sqlite3_libversion().unwrap_or_default()))
        }
        PDO_SQLITE_ATTR_TRANSACTION_MODE => AttrResult::Value(Value::Long(
            with_h(dbh, |h| h.transaction_mode).unwrap_or(0),
        )),
        _ => AttrResult::Unsupported,
    }
}

/// pdo_sqlite_in_transaction: it's not possible in sqlite3 to explicitly
/// turn autocommit off other than manually starting a transaction.
pub fn in_transaction(dbh: &DbhRef) -> bool {
    match conn_of(dbh) {
        Some(c) => c.with(mainapi::sqlite3_get_autocommit) == Some(0),
        None => false,
    }
}

/// pdo_sqlite_set_attr
fn set_attribute(vm: &mut Vm, dbh: &DbhRef, attr: i64, val: &Value) -> bool {
    match attr {
        PDO_ATTR_TIMEOUT => {
            let Some(lval) = get_long_param(vm, val) else {
                return false;
            };
            if let Some(c) = conn_of(dbh) {
                c.with(|db| mainapi::sqlite3_busy_timeout(db, lval.wrapping_mul(1000) as i32));
            }
            true
        }
        PDO_SQLITE_ATTR_EXTENDED_RESULT_CODES => {
            let Some(lval) = get_long_param(vm, val) else {
                return false;
            };
            if let Some(c) = conn_of(dbh) {
                c.with(|db| mainapi::sqlite3_extended_result_codes(db, lval as i32));
            }
            true
        }
        PDO_SQLITE_ATTR_TRANSACTION_MODE => {
            let Some(lval) = get_long_param(vm, val) else {
                return false;
            };
            match lval {
                PDO_SQLITE_TRANSACTION_MODE_DEFERRED
                | PDO_SQLITE_TRANSACTION_MODE_IMMEDIATE
                | PDO_SQLITE_TRANSACTION_MODE_EXCLUSIVE => {
                    with_h(dbh, |h| h.transaction_mode = lval);
                    true
                }
                _ => false,
            }
        }
        _ => false,
    }
}

// ── user functions, aggregates, collations ────────────────────────────

/// aggregate_context
#[derive(Default)]
struct AggregateContext {
    /// the context value (a reference; None while undefined)
    val: Option<Rc<RefCell<Value>>>,
    row: i64,
}

/// The PHP value of an SQL function argument
fn arg_value(m: &mut Mem) -> Value {
    match vdbeapi::sqlite3_value_type(m) {
        SQLITE_INTEGER => Value::Long(vdbeapi::sqlite3_value_int(m) as i64),
        SQLITE_FLOAT => Value::Double(vdbeapi::sqlite3_value_double(m)),
        SQLITE_NULL => Value::Null,
        _ => {
            let t = vdbeapi::sqlite3_value_text(m).map(|t| t.to_vec());
            let n = vdbeapi::sqlite3_value_bytes(m).max(0) as usize;
            let mut t = t.unwrap_or_default();
            t.truncate(n);
            vstr(t)
        }
    }
}

/// zend_call_known_fcc(): None when the call failed (UNDEF retval)
fn call_php(vm: &mut Vm, cb: &Value, args: &[Value]) -> Option<Value> {
    if vm.current_exception.is_some() {
        return None;
    }
    let r = vm.call_callback(cb, args);
    if vm.current_exception.is_some() {
        return None;
    }
    match r {
        Ok(v) => Some(match v {
            Value::Undef => Value::Null,
            v => v,
        }),
        Err(_) => None,
    }
}

/// Set the result of a scalar function or of a finalized aggregate:
/// Err when the value could not be converted (FAILURE).
fn set_result(vm: &mut Vm, ctx: &mut Context, retval: Option<Value>) -> Result<(), ()> {
    match retval {
        Some(v) => match v.deref() {
            Value::Long(n) => vdbeapi::sqlite3_result_int(ctx, n as i32),
            Value::Null => vdbeapi::sqlite3_result_null(ctx),
            Value::Double(d) => vdbeapi::sqlite3_result_double(ctx, d),
            other => {
                let s = match vm.value_to_string_checked(&other) {
                    Ok(s) => s,
                    Err(_) => return Err(()),
                };
                if vm.current_exception.is_some() {
                    return Err(());
                }
                let b = s.as_bytes();
                vdbeapi::sqlite3_result_text(ctx, Some(b), b.len() as i32);
            }
        },
        None => vdbeapi::sqlite3_result_error(ctx, Some(b"failed to invoke callback"), 0),
    }
    Ok(())
}

/// do_callback for a scalar function
fn func_callback(cb: &Value, ctx: &mut Context, argv: &mut [Mem]) {
    let Some(vm) = vm_ptr() else {
        vdbeapi::sqlite3_result_error(ctx, Some(b"failed to invoke callback"), 0);
        return;
    };
    let vm = unsafe { &mut *vm };
    let args: Vec<Value> = argv.iter_mut().map(arg_value).collect();
    let retval = call_php(vm, cb, &args);
    let _ = set_result(vm, ctx, retval);
}

/// do_callback for an aggregate's step (`argv` Some) or final (None)
fn agg_callback(cb: &Value, ctx: &mut Context, argv: Option<&mut [Mem]>) {
    let Some(vm) = vm_ptr() else {
        if argv.is_none() {
            vdbeapi::sqlite3_result_error(ctx, Some(b"failed to invoke callback"), 0);
        }
        return;
    };
    let vm = unsafe { &mut *vm };
    let (val, row) = {
        let Some(agg) = vdbeapi::sqlite3_aggregate_context::<AggregateContext>(
            ctx,
            std::mem::size_of::<AggregateContext>() as i32,
        ) else {
            return;
        };
        let val = agg
            .val
            .get_or_insert_with(|| Rc::new(RefCell::new(Value::Null)))
            .clone();
        agg.row += 1;
        (val, agg.row)
    };
    let mut args = vec![Value::Reference(val.clone()), Value::Long(row)];
    let is_final = argv.is_none();
    if let Some(argv) = argv {
        args.extend(argv.iter_mut().map(arg_value));
    }
    let retval = call_php(vm, cb, &args);
    if is_final {
        // only set the sqlite return value if we are finalizing
        let _ = set_result(vm, ctx, retval);
        if let Some(agg) = vdbeapi::sqlite3_aggregate_context::<AggregateContext>(ctx, 0) {
            agg.val = None;
        }
    } else if let Some(v) = retval {
        // we're stepping in an aggregate; the return value goes into the
        // context
        *val.borrow_mut() = v.deref();
    }
}

/// php_sqlite3_collation_callback
fn collation_callback(cb: &Value, a: &[u8], b: &[u8]) -> i32 {
    let Some(vm) = vm_ptr() else {
        return 0;
    };
    let vm = unsafe { &mut *vm };
    let Some(retval) = call_php(vm, cb, &[vstr(a), vstr(b)]) else {
        return 0;
    };
    match retval.deref() {
        Value::Long(n) => n.signum() as i32,
        other => {
            let m = format!(
                "{}(): Return value of the collation callback must be of type int, {} returned",
                active_function_name(vm),
                Vm::value_type_name(&other)
            );
            throw(vm, b"TypeError", &m);
            // FAILURE
            -1
        }
    }
}

/// The '{f}' argument of a create* method: Err after throwing
fn callable_arg(vm: &mut Vm, f: &Func, n: usize, v: &Value, nullable: bool) -> Result<Value, ()> {
    let v = v.deref();
    if nullable && matches!(v, Value::Null) {
        return Ok(Value::Null);
    }
    let r = vm.zend_is_callable_ex(&v, 0);
    if vm.current_exception.is_some() {
        return Err(());
    }
    if !r.ok {
        let m = format!(
            "{} must be a valid callback{}, {}",
            f.arg_prefix(n),
            if nullable { " or null" } else { "" },
            r.error.unwrap_or_default()
        );
        throw(vm, b"TypeError", &m);
        return Err(());
    }
    Ok(v)
}

fn arg_any(a: &Arg) -> Value {
    match a {
        Arg::Any(v) => v.clone(),
        _ => Value::Null,
    }
}

/// PDO_CONSTRUCT_CHECK + the SQLite handle
pub fn this_dbh(vm: &mut Vm, f: &Func) -> Option<DbhRef> {
    let o = f.this.clone()?;
    let dbh = crate::objects::dbh_of(&o)?;
    if dbh.borrow().driver.is_none() {
        let m = format!(
            "{} object is uninitialized",
            goro_core::value::display_class_name(&o.borrow().class_name)
        );
        throw(vm, b"Error", &m);
        return None;
    }
    Some(dbh)
}

/// pdo_sqlite_create_function_internal
pub fn create_function_internal(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "sz|zz", &[], args) else {
        return Value::False;
    };
    let Ok(cb) = callable_arg(vm, f, 2, &arg_any(&a[1]), false) else {
        return Value::False;
    };
    let Ok(b) = parse(vm, f, "zz|ll", &[], args) else {
        return Value::False;
    };
    let name = a[0].str().unwrap_or(b"").to_vec();
    let argc = b[2].long_or(-1);
    let flags = b[3].long_or(0);
    let Some(dbh) = this_dbh(vm, f) else {
        return Value::False;
    };
    let Some(conn) = conn_of(&dbh) else {
        return Value::False;
    };
    let x: XSFunc =
        Rc::new(move |ctx: &mut Context, argv: &mut [Mem]| func_callback(&cb, ctx, argv));
    let ret = run(vm, &conn, |db| {
        mainapi::sqlite3_create_function(
            db,
            Some(&name),
            argc as i32,
            (flags as i32) | SQLITE_UTF8 as i32,
            FuncUserData::None,
            Some(x),
            None,
            None,
        )
    });
    if ret == Some(SQLITE_OK) {
        with_h(&dbh, |h| h.funcs.push((name, argc as i32)));
        return Value::True;
    }
    Value::False
}

/// pdo_sqlite_create_aggregate_internal
pub fn create_aggregate_internal(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "szz|z", &[], args) else {
        return Value::False;
    };
    let Ok(step) = callable_arg(vm, f, 2, &arg_any(&a[1]), false) else {
        return Value::False;
    };
    let Ok(fini) = callable_arg(vm, f, 3, &arg_any(&a[2]), false) else {
        return Value::False;
    };
    let Ok(b) = parse(vm, f, "zzz|l", &[], args) else {
        return Value::False;
    };
    let name = a[0].str().unwrap_or(b"").to_vec();
    let argc = b[3].long_or(-1);
    let Some(dbh) = this_dbh(vm, f) else {
        return Value::False;
    };
    let Some(conn) = conn_of(&dbh) else {
        return Value::False;
    };
    let xstep: XSFunc =
        Rc::new(move |ctx: &mut Context, argv: &mut [Mem]| agg_callback(&step, ctx, Some(argv)));
    let xfinal: XFinal = Rc::new(move |ctx: &mut Context| agg_callback(&fini, ctx, None));
    let ret = run(vm, &conn, |db| {
        mainapi::sqlite3_create_function(
            db,
            Some(&name),
            argc as i32,
            SQLITE_UTF8 as i32,
            FuncUserData::None,
            None,
            Some(xstep),
            Some(xfinal),
        )
    });
    if ret == Some(SQLITE_OK) {
        with_h(&dbh, |h| h.funcs.push((name, argc as i32)));
        return Value::True;
    }
    Value::False
}

/// pdo_sqlite_create_collation_internal
pub fn create_collation_internal(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "sz", &[], args) else {
        return Value::Null;
    };
    let Ok(cb) = callable_arg(vm, f, 2, &arg_any(&a[1]), false) else {
        return Value::Null;
    };
    let name = a[0].str().unwrap_or(b"").to_vec();
    let Some(dbh) = this_dbh(vm, f) else {
        return Value::Null;
    };
    let Some(conn) = conn_of(&dbh) else {
        return Value::False;
    };
    let x: XCmp = Rc::new(move |a: &[u8], b: &[u8]| collation_callback(&cb, a, b));
    let ret = run(vm, &conn, |db| {
        mainapi::sqlite3_create_collation(db, Some(&name), SQLITE_UTF8 as i32, Some(x))
    });
    if ret == Some(SQLITE_OK) {
        with_h(&dbh, |h| h.collations.push(name));
        return Value::True;
    }
    Value::False
}

/// Pdo\Sqlite::setAuthorizer
pub fn set_authorizer(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "z", &[], args) else {
        return Value::Null;
    };
    let Ok(cb) = callable_arg(vm, f, 1, &arg_any(&a[0]), true) else {
        return Value::Null;
    };
    let Some(dbh) = this_dbh(vm, f) else {
        return Value::Null;
    };
    let cb = if matches!(cb, Value::Null) {
        None
    } else {
        Some(cb)
    };
    with_h(&dbh, |h| *h.authorizer.borrow_mut() = cb);
    Value::Null
}

/// The connection's authorizer: open_basedir checks of ATTACH, then the
/// user's callback
fn authorizer(
    cb: &Rc<RefCell<Option<Value>>>,
    access_type: i32,
    arg1: Option<&[u8]>,
    arg2: Option<&[u8]>,
    arg3: Option<&[u8]>,
    arg4: Option<&[u8]>,
) -> i32 {
    let Some(vm) = vm_ptr() else {
        return SQLITE_OK;
    };
    let vm = unsafe { &mut *vm };
    if !goro_core::basedir::open_basedir_value(vm).is_empty()
        && access_type == SQLITE_ATTACH
        && make_filename_safe(vm, arg1).is_none()
    {
        return SQLITE_DENY;
    }
    // fallback to access allowed if authorizer callback is not defined
    let Some(f) = cb.borrow().clone() else {
        return SQLITE_OK;
    };
    let ns = |s: Option<&[u8]>| match s {
        Some(s) => vstr(s),
        None => Value::Null,
    };
    let argv = [
        Value::Long(access_type as i64),
        ns(arg1),
        ns(arg2),
        ns(arg3),
        ns(arg4),
    ];
    let Some(retval) = call_php(vm, &f, &argv) else {
        return SQLITE_DENY;
    };
    match retval.deref() {
        Value::Long(n) => {
            if n != SQLITE_OK as i64 && n != SQLITE_IGNORE as i64 && n != SQLITE_DENY as i64 {
                let m = format!(
                    "{}(): Return value of the authorizer callback must be one of Pdo\\Sqlite::OK, Pdo\\Sqlite::DENY, or Pdo\\Sqlite::IGNORE",
                    active_function_name(vm)
                );
                throw(vm, b"ValueError", &m);
                SQLITE_DENY
            } else {
                n as i32
            }
        }
        other => {
            let m = format!(
                "{}(): Return value of the authorizer callback must be of type int, {} returned",
                active_function_name(vm),
                Vm::value_type_name(&other)
            );
            throw(vm, b"TypeError", &m);
            SQLITE_DENY
        }
    }
}

/// make_filename_safe
fn make_filename_safe(vm: &mut Vm, filename: Option<&[u8]>) -> Option<Vec<u8>> {
    let filename = filename?;
    let filename = match filename.iter().position(|&c| c == 0) {
        Some(p) => &filename[..p],
        None => filename,
    };
    let basedir = !goro_core::basedir::open_basedir_value(vm).is_empty();
    if !filename.is_empty() && filename.len() >= 5 && filename[..5].eq_ignore_ascii_case(b"file:") {
        if basedir {
            return None;
        }
        return Some(filename.to_vec());
    }
    if !filename.is_empty() && filename != b":memory:" {
        let fullpath = goro_core::basedir::expand_filepath(&String::from_utf8_lossy(filename))?;
        let fname = active_function_name(vm);
        if !goro_core::basedir::check_open_basedir_ex(vm, &fname, &fullpath, true) {
            return None;
        }
        return Some(fullpath.into_bytes());
    }
    Some(filename.to_vec())
}

pub static SQLITE_METHODS: DbhMethods = DbhMethods {
    closer: handle_closer,
    preparer: handle_preparer,
    doer: handle_doer,
    quoter: Some(handle_quoter),
    begin: Some(handle_begin),
    commit: Some(handle_commit),
    rollback: Some(handle_rollback),
    set_attribute: Some(set_attribute),
    last_id: Some(last_insert_id),
    fetch_err: Some(fetch_error_func),
    get_attribute: Some(get_attribute),
    check_liveness: None,
    persistent_shutdown: Some(cleanup_callbacks),
    in_transaction: Some(in_transaction),
    scanner: Some(crate::scanner::sqlite_scanner),
};

/// pdo_sqlite_handle_factory
fn handle_factory(vm: &mut Vm, dbh: &DbhRef, driver_options: Option<&Arr>) -> bool {
    set_vm(vm);
    let authz = Rc::new(RefCell::new(None));
    {
        let mut d = dbh.borrow_mut();
        d.driver_data = Some(Box::new(H {
            db: None,
            einfo: EInfo::default(),
            funcs: Vec::new(),
            collations: Vec::new(),
            authorizer: authz.clone(),
            transaction_mode: 0,
        }));
        // skip all but this one param event
        d.skip_param_evt = 0x7F ^ (1 << PDO_PARAM_EVT_EXEC_PRE);
    }
    let ret = factory_inner(vm, dbh, driver_options, authz);
    dbh.borrow_mut().methods = Some(&SQLITE_METHODS);
    ret
}

fn factory_inner(
    vm: &mut Vm,
    dbh: &DbhRef,
    driver_options: Option<&Arr>,
    authz: Rc<RefCell<Option<Value>>>,
) -> bool {
    let ds = dbh.borrow().data_source.clone().unwrap_or_default();
    let Some(filename) = make_filename_safe(vm, Some(&ds)) else {
        let ds = match ds.iter().position(|&c| c == 0) {
            Some(p) => &ds[..p],
            None => &ds[..],
        };
        let m = format!(
            "open_basedir prohibits opening {}",
            String::from_utf8_lossy(ds)
        );
        // the open_basedir warning was turned into an exception (EH_THROW)
        let prev = vm.warning_capture.as_mut().and_then(|c| {
            if c.is_empty() {
                None
            } else {
                Some(c.remove(0))
            }
        });
        if let Some(p) = prev {
            throw_pdo(vm, &p);
        }
        let prev = vm.current_exception.take();
        throw_pdo(vm, &m);
        if let (Some(p), Some(Value::Object(o))) = (prev, &vm.current_exception) {
            o.borrow_mut().set_property(b"previous".to_vec(), p);
        }
        return false;
    };
    let mut flags = attr_lval(
        driver_options,
        PDO_SQLITE_ATTR_OPEN_FLAGS,
        (SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE) as i64,
    );
    if goro_core::basedir::open_basedir_value(vm).is_empty() {
        flags |= SQLITE_OPEN_URI as i64;
    }
    let mut db: Option<Box<Sqlite3>> = None;
    let i = mainapi::sqlite3_open_v2(Some(&filename), &mut db, flags as i32, None);
    let conn = db.map(Conn::new);
    with_h(dbh, |h| h.db = conn.clone());
    if i != SQLITE_OK {
        sqlite_error(Some(vm), dbh, None);
        return false;
    }
    let Some(conn) = conn else {
        return false;
    };
    let a = authz.clone();
    let x: XAuth = Rc::new(move |t, a1, a2, a3, a4| authorizer(&a, t, a1, a2, a3, a4));
    let mut timeout = 60;
    if driver_options.is_some() {
        timeout = attr_lval(driver_options, PDO_ATTR_TIMEOUT, timeout);
    }
    conn.with(|db| {
        goro_sqlite::auth::sqlite3_set_authorizer(db, Some(x));
        mainapi::sqlite3_busy_timeout(db, timeout.wrapping_mul(1000) as i32);
    });
    let mut d = dbh.borrow_mut();
    d.alloc_own_columns = true;
    d.max_escaped_char_length = 2;
    true
}

pub static SQLITE_DRIVER: Driver = Driver {
    name: "sqlite",
    factory: handle_factory,
};
