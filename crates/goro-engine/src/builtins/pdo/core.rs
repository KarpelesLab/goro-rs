//! The `PDO` class (PHP manual: The PDO class): connecting, attributes,
//! errors and their modes, exec/query/prepare, quoting and transactions.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use super::driver::{Driver, DrvError, Failure, error_message};
use super::sqlite::SqliteDriver;
use crate::class::NativeHooks;
use crate::func::Args;
use crate::vm::{R, Unwind, Vm};

pub const ERRMODE_SILENT: i64 = 0;
pub const ERRMODE_WARNING: i64 = 1;
pub const ERRMODE_EXCEPTION: i64 = 2;

pub const ATTR_AUTOCOMMIT: i64 = 0;
pub const ATTR_PREFETCH: i64 = 1;
pub const ATTR_TIMEOUT: i64 = 2;
pub const ATTR_ERRMODE: i64 = 3;
pub const ATTR_SERVER_VERSION: i64 = 4;
pub const ATTR_CLIENT_VERSION: i64 = 5;
pub const ATTR_SERVER_INFO: i64 = 6;
pub const ATTR_CONNECTION_STATUS: i64 = 7;
pub const ATTR_CASE: i64 = 8;
pub const ATTR_CURSOR_NAME: i64 = 9;
pub const ATTR_CURSOR: i64 = 10;
pub const ATTR_ORACLE_NULLS: i64 = 11;
pub const ATTR_PERSISTENT: i64 = 12;
pub const ATTR_STATEMENT_CLASS: i64 = 13;
pub const ATTR_FETCH_TABLE_NAMES: i64 = 14;
pub const ATTR_FETCH_CATALOG_NAMES: i64 = 15;
pub const ATTR_DRIVER_NAME: i64 = 16;
pub const ATTR_STRINGIFY_FETCHES: i64 = 17;
pub const ATTR_MAX_COLUMN_LEN: i64 = 18;
pub const ATTR_DEFAULT_FETCH_MODE: i64 = 19;
pub const ATTR_EMULATE_PREPARES: i64 = 20;
pub const ATTR_DEFAULT_STR_PARAM: i64 = 21;

pub const FETCH_BOTH: i64 = 4;

/// The error state of a handle (`errorCode()`/`errorInfo()`).
#[derive(Clone, Debug)]
pub struct ErrState {
    pub sqlstate: String,
    pub code: Option<i64>,
    pub msg: Option<String>,
}

impl Default for ErrState {
    fn default() -> ErrState {
        ErrState {
            sqlstate: "00000".into(),
            code: None,
            msg: None,
        }
    }
}

impl ErrState {
    /// No operation yet: errorCode() is null (test pdo_039.phpt).
    pub fn unset() -> ErrState {
        ErrState {
            sqlstate: String::new(),
            code: None,
            msg: None,
        }
    }

    /// errorCode(): the SQLSTATE, null before any operation.
    pub fn code_value(&self) -> Value {
        if self.sqlstate.is_empty() {
            Value::Null
        } else {
            Value::Str(PhpStr::from(self.sqlstate.as_str()))
        }
    }

    pub fn info(&self) -> Value {
        let mut a = PhpArray::new();
        let _ = a.push(Value::Str(PhpStr::from(self.sqlstate.as_str())));
        let _ = a.push(self.code.map_or(Value::Null, Value::Int));
        let _ = a.push(
            self.msg
                .as_ref()
                .map_or(Value::Null, |m| Value::Str(PhpStr::from(m.as_str()))),
        );
        Value::Array(a)
    }
}

pub type DriverRef = Rc<RefCell<Box<dyn Driver>>>;

/// A PDO object's state.
pub struct DbhData {
    pub drv: Option<DriverRef>,
    pub errmode: i64,
    pub case: i64,
    pub oracle_nulls: i64,
    pub stringify: bool,
    pub default_fetch: i64,
    /// ATTR_STATEMENT_CLASS: the class and constructor arguments.
    pub stmt_class: Option<(PhpStr, Option<Value>)>,
    pub err: ErrState,
    pub persistent: bool,
    pub default_str_param: i64,
    /// PDO::beginTransaction() was called and not yet ended (for drivers
    /// that don't track it).
    pub in_txn: bool,
}

impl DbhData {
    fn new() -> DbhData {
        DbhData {
            drv: None,
            errmode: ERRMODE_EXCEPTION,
            case: 0,
            oracle_nulls: 0,
            stringify: false,
            default_fetch: FETCH_BOTH,
            stmt_class: None,
            err: ErrState::unset(),
            persistent: false,
            default_str_param: 0,
            in_txn: false,
        }
    }
}

pub struct DbhObj(pub DbhData);

impl goro_runtime::NativeData for DbhObj {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        if let Some((_, Some(a))) = &self.0.stmt_class {
            visit(a);
        }
        if !self.0.persistent
            && let Some(d) = &self.0.drv
            && let Ok(d) = d.try_borrow()
        {
            d.trace(visit);
        }
    }
    fn clear(&mut self) {
        self.0.stmt_class = None;
        if !self.0.persistent
            && let Some(d) = &self.0.drv
            && let Ok(mut d) = d.try_borrow_mut()
        {
            d.clear();
        }
    }
}

pub fn with_dbh<T>(o: &ObjRef, f: impl FnOnce(&mut DbhData) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<DbhObj>().map(|d| f(&mut d.0))
}

pub fn dbh_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(DbhObj(DbhData::new())));
        }),
        ..Default::default()
    }
}

thread_local! {
    /// Persistent connections by DSN and credentials (PHP manual:
    /// Connections and Connection management).
    static PERSISTENT: RefCell<HashMap<Vec<u8>, DriverRef>> = RefCell::new(HashMap::new());
}

/// The driver of an initialized PDO object, or the "object is
/// uninitialized" Error.
pub fn driver_of(vm: &mut Vm, o: &ObjRef) -> R<DriverRef> {
    match with_dbh(o, |d| d.drv.clone()).flatten() {
        Some(d) => Ok(d),
        None => {
            let cls = vm.class_of(o).name.to_string_lossy();
            vm.throw("Error", &format!("{cls} object is uninitialized"))
        }
    }
}

/// Creates the PDOException for an error (its `code` is the SQLSTATE,
/// `errorInfo` the error info array).
pub fn pdo_exception(vm: &mut Vm, msg: &str, info: Option<Value>, state: &str) -> Unwind {
    match vm.create_exception("PDOException", msg, 0) {
        Ok(ex) => {
            vm.set_prop_raw(&ex, b"code", Value::Str(PhpStr::from(state)));
            if let Some(i) = info {
                vm.set_prop_raw(&ex, b"errorInfo", i);
            }
            Unwind::Throw(ex)
        }
        Err(u) => u,
    }
}

/// Reports an error by the handle's error mode: nothing, an E_WARNING
/// "fname(): SQLSTATE[...]...", or a PDOException.
pub fn report(vm: &mut Vm, errmode: i64, fname: &str, e: &DrvError, info: Value) -> R<()> {
    let msg = error_message(e);
    match errmode {
        ERRMODE_SILENT => Ok(()),
        ERRMODE_WARNING => vm.warn(&format!("{fname}(): {msg}")),
        _ => Err(pdo_exception(vm, &msg, Some(info), &e.sqlstate)),
    }
}

/// The errorInfo() of an error: driver errors show their code and text;
/// errors raised by PDO itself (no driver code) show nulls.
pub fn err_state(e: &DrvError) -> ErrState {
    ErrState {
        sqlstate: e.sqlstate.clone(),
        code: e.code,
        msg: if e.code.is_some() {
            e.msg.clone()
        } else {
            None
        },
    }
}

/// Records an error on the handle and reports it.
pub fn dbh_error(vm: &mut Vm, o: &ObjRef, fname: &str, e: &DrvError) -> R<()> {
    let st = err_state(e);
    let info = st.info();
    let mode = with_dbh(o, |d| {
        d.err = st;
        d.errmode
    })
    .unwrap_or(ERRMODE_EXCEPTION);
    report(vm, mode, fname, e, info)
}

/// Handles a failure of a database call on the handle: records and
/// reports a database error, passes a PHP exception through.
pub fn dbh_fail(vm: &mut Vm, o: &ObjRef, fname: &str, f: Failure) -> R<()> {
    match f {
        Failure::Db(e) => dbh_error(vm, o, fname, &e),
        Failure::Php(u) => Err(u),
    }
}

fn clear_err(o: &ObjRef) {
    with_dbh(o, |d| d.err = ErrState::default());
}

/// The drivers available (`PDO::getAvailableDrivers()`).
pub fn pdo_drivers(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::from_values([Value::Str(
        PhpStr::from("sqlite"),
    )])))
}

/// Splits a DSN into driver name and the rest; `uri:` DSNs are read
/// from the file they name (PHP manual: PDO::__construct).
fn parse_dsn(vm: &mut Vm, fname: &str, dsn: &[u8]) -> R<(Vec<u8>, Vec<u8>)> {
    let mut dsn = dsn.to_vec();
    if !dsn.contains(&b':') {
        // An alias: the pdo.dsn.<name> ini setting.
        let key = [b"pdo.dsn.".as_slice(), &dsn].concat();
        match vm.get_ini(&key) {
            Some(v) if v.as_bytes().contains(&b':') => dsn = v.as_bytes().to_vec(),
            _ => {
                return Err(pdo_exception(
                    vm,
                    &format!("{fname}(): Argument #1 ($dsn) must be a valid data source name"),
                    None,
                    "",
                ));
            }
        }
    }
    if let Some(rest) = dsn.strip_prefix(b"uri:") {
        // PHP 8.5 (test pdo_sqlite___construct_uri.phpt).
        vm.deprecated(
            "Looking up the DSN from a URI is deprecated due to possible security concerns with DSNs coming from remote URIs",
        )?;
        let path = String::from_utf8_lossy(rest).into_owned();
        let data = std::fs::read(&path).ok();
        match data {
            Some(d) => {
                let line = d.split(|&b| b == b'\n').next().unwrap_or(&[]);
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                if !line.contains(&b':') {
                    return Err(pdo_exception(
                        vm,
                        &format!(
                            "{fname}(): Argument #1 ($dsn) must be a valid data source name (via URI)"
                        ),
                        None,
                        "",
                    ));
                }
                dsn = line.to_vec();
            }
            None => {
                return Err(pdo_exception(
                    vm,
                    &format!(
                        "{fname}(): Argument #1 ($dsn) must be a valid data source name (via URI)"
                    ),
                    None,
                    "",
                ));
            }
        }
    }
    let p = dsn.iter().position(|&b| b == b':').unwrap_or(dsn.len());
    let name = dsn[..p].to_vec();
    let rest = dsn.get(p + 1..).unwrap_or(&[]).to_vec();
    Ok((name, rest))
}

/// The class a driver's connections get from `PDO::connect()`.
fn driver_class(name: &[u8]) -> Option<&'static str> {
    match name {
        b"sqlite" => Some("Pdo\\Sqlite"),
        b"mysql" => Some("Pdo\\Mysql"),
        b"pgsql" => Some("Pdo\\Pgsql"),
        b"firebird" => Some("Pdo\\Firebird"),
        b"odbc" => Some("Pdo\\Odbc"),
        b"dblib" => Some("Pdo\\Dblib"),
        _ => None,
    }
}

/// Opens the connection of a PDO object: `PDO::__construct()` and
/// `PDO::connect()`.
fn init_connection(vm: &mut Vm, o: &ObjRef, fname: &str, a: &Args, from_connect: bool) -> R<()> {
    let dsn = a.str(0);
    let user = a.opt_str(1);
    let pass = a.opt_str(2);
    let opts = match a.get(3).map(|v| v.deref()) {
        Some(Value::Array(arr)) => Some(arr),
        _ => None,
    };
    let (driver, rest) = parse_dsn(vm, fname, dsn.as_bytes())?;
    // A driver subclass only connects to its own driver (RFC: PDO driver
    // specific subclasses).
    let cls = vm.class_of(o);
    if !from_connect {
        let mut c = Some(&*cls);
        while let Some(k) = c {
            let ln = k.lname.as_bytes();
            if let Some(n) = ln.strip_prefix(b"pdo\\") {
                let own: &[u8] = n;
                if own != driver.as_slice() {
                    let dname = k.name.to_string_lossy();
                    return Err(pdo_exception(
                        vm,
                        &format!(
                            "{dname}::__construct() cannot be used for connecting to the \"{}\" driver, either call {}::__construct() or PDO::__construct() instead",
                            String::from_utf8_lossy(&driver),
                            driver_class(&driver).unwrap_or("PDO")
                        ),
                        None,
                        "",
                    ));
                }
                break;
            }
            c = k.parent.as_deref();
        }
    }
    if driver.as_slice() != b"sqlite" {
        return Err(pdo_exception(vm, "could not find driver", None, ""));
    }
    let persistent = opts
        .as_ref()
        .and_then(|o| o.get_int(ATTR_PERSISTENT).map(|v| v.deref()))
        .is_some_and(|v| match &v {
            Value::Str(s) => !s.is_empty() && s.as_bytes() != b"0",
            other => goro_runtime::convert::to_bool_quiet(other),
        });
    let pkey = {
        let mut k = dsn.as_bytes().to_vec();
        k.push(0);
        k.extend_from_slice(user.as_ref().map_or(&b""[..], |u| u.as_bytes()));
        k.push(0);
        k.extend_from_slice(pass.as_ref().map_or(&b""[..], |u| u.as_bytes()));
        if let Some(Value::Str(s)) = opts
            .as_ref()
            .and_then(|o| o.get_int(ATTR_PERSISTENT).map(|v| v.deref()))
        {
            k.push(0);
            k.extend_from_slice(s.as_bytes());
        }
        k
    };
    let mut drv: Option<DriverRef> = None;
    if persistent {
        drv = PERSISTENT.with(|p| p.borrow().get(&pkey).cloned());
    }
    if drv.is_none() {
        let flags = opts
            .as_ref()
            .and_then(|o| o.get_int(super::sqlite::ATTR_OPEN_FLAGS).map(|v| v.deref()))
            .map(|v| match v {
                Value::Int(i) => i,
                _ => 6,
            })
            .unwrap_or(
                goro_ext_sqlite::OPEN_READWRITE as i64 | goro_ext_sqlite::OPEN_CREATE as i64,
            );
        let path = rest.clone();
        // open_basedir applies to database files (test
        // ext/pdo_sqlite/tests/pdo_sqlite_open_basedir.phpt).
        let pstr = String::from_utf8_lossy(&path).into_owned();
        let is_mem = pstr.is_empty()
            || pstr == ":memory:"
            || (pstr.starts_with("file:") && pstr.contains("mode=memory"));
        if !is_mem {
            let fpath = if let Some(u) = pstr.strip_prefix("file:") {
                let u = u.split('?').next().unwrap_or("");
                u.strip_prefix("//").unwrap_or(u).to_string()
            } else {
                pstr.clone()
            };
            if !vm.basedir_allows(fpath.as_bytes()) {
                let bd = vm
                    .get_ini(b"open_basedir")
                    .map(|v| v.to_string_lossy())
                    .unwrap_or_default();
                // A plain path gets open_basedir's warning (thrown during
                // the constructor) chained before PDO's own exception; a URI
                // only the latter (tests pdo_sqlite_open_basedir.phpt,
                // pdo_sqlite_open_basedir_uri.phpt, open_basedir.phpt).
                let outer = pdo_exception(
                    vm,
                    &format!("open_basedir prohibits opening {pstr}"),
                    None,
                    "",
                );
                if !pstr.starts_with("file:")
                    && let Unwind::Throw(o) = &outer
                {
                    let w = format!(
                        "{fname}(): open_basedir restriction in effect. File({fpath}) is not within the allowed path(s): ({bd})"
                    );
                    if let Unwind::Throw(inner) = pdo_exception(vm, &w, None, "") {
                        vm.set_prop_raw(o, b"previous", Value::Object(inner));
                    }
                }
                return Err(outer);
            }
        }
        match SqliteDriver::open(vm, &path, flags) {
            Ok(d) => {
                let d: Box<dyn Driver> = Box::new(d);
                let r = Rc::new(RefCell::new(d));
                if persistent {
                    PERSISTENT.with(|p| p.borrow_mut().insert(pkey, r.clone()));
                }
                drv = Some(r);
            }
            Err(e) => {
                let msg = format!(
                    "SQLSTATE[{}] [{}] {}",
                    e.sqlstate,
                    e.code.unwrap_or(0),
                    e.msg.clone().unwrap_or_default()
                );
                let info = err_state(&e).info();
                // The code is the driver's error code here (test
                // ext/pdo_sqlite/tests/bug50728.phpt: 14).
                let u = pdo_exception(vm, &msg, Some(info), &e.sqlstate);
                if let Unwind::Throw(ex) = &u {
                    vm.set_prop_raw(ex, b"code", Value::Int(e.code.unwrap_or(0)));
                }
                return Err(u);
            }
        }
    }
    with_dbh(o, |d| {
        d.drv = drv;
        d.persistent = persistent;
    });
    // The options, as setAttribute() would set them.
    if let Some(opts) = opts {
        for (k, v) in opts.iter() {
            let goro_runtime::KeyRef::Int(attr) = k else {
                continue;
            };
            if attr == ATTR_PERSISTENT || attr == super::sqlite::ATTR_OPEN_FLAGS {
                continue;
            }
            set_attr(vm, o, "PDO::__construct", attr, &v.deref())?;
        }
    }
    Ok(())
}

pub fn pdo_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if with_dbh(&o, |d| d.drv.is_some()).unwrap_or(false) {
        return vm.throw("Error", "Cannot call constructor twice");
    }
    let cls = vm.class_of(&o).name.to_string_lossy();
    init_connection(vm, &o, &format!("{cls}::__construct"), a, false)?;
    Ok(Value::Null)
}

/// `PDO::connect()`: the driver's subclass for `PDO::connect()`, or the
/// called class when it fits the driver.
pub fn pdo_connect(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let called = if a.static_class.is_null() {
        "PDO".to_string()
    } else {
        unsafe { &*a.static_class }.name.to_string_lossy()
    };
    let (driver, _) = parse_dsn(vm, &format!("{called}::connect"), a.str(0).as_bytes())?;
    let dclass = driver_class(&driver);
    let lcalled = called.to_ascii_lowercase();
    let target = if lcalled == "pdo" {
        match dclass {
            Some(c) => match vm.find_class(c.as_bytes(), false)? {
                Some(k) => k,
                None => vm.find_class(b"PDO", false)?.unwrap(),
            },
            None => vm.find_class(b"PDO", false)?.unwrap(),
        }
    } else {
        let k = vm.find_class(called.as_bytes(), false)?.unwrap();
        let ok = match dclass {
            Some(dc) => match vm.find_class(dc.as_bytes(), false)? {
                Some(dk) => k.is_subclass_of(&dk),
                None => false,
            },
            None => false,
        };
        if !ok {
            return Err(pdo_exception(
                vm,
                &format!(
                    "{called}::connect() cannot be used for connecting to the \"{}\" driver, either call {}::connect() or PDO::connect() instead",
                    String::from_utf8_lossy(&driver),
                    dclass.unwrap_or("PDO")
                ),
                None,
                "",
            ));
        }
        k
    };
    let o = vm.instantiate(&target)?;
    let fname = format!("{}::connect", target.name.to_string_lossy());
    init_connection(vm, &o, &fname, a, true)?;
    // A user subclass's constructor runs (test pdosqlite_006).
    Ok(Value::Object(o))
}

pub fn pdo_begin(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let d = driver_of(vm, &o)?;
    let active = {
        let drv = d.borrow();
        drv.in_transaction()
    }
    .unwrap_or_else(|| with_dbh(&o, |x| x.in_txn).unwrap_or(false));
    if active {
        return Err(pdo_exception(
            vm,
            "There is already an active transaction",
            None,
            "",
        ));
    }
    clear_err(&o);
    let r = d.borrow_mut().begin(vm);
    match r {
        Ok(()) => {
            with_dbh(&o, |x| x.in_txn = true);
            Ok(Value::Bool(true))
        }
        Err(f) => {
            dbh_fail(vm, &o, "PDO::beginTransaction", f)?;
            Ok(Value::Bool(false))
        }
    }
}

fn in_txn(o: &ObjRef, d: &DriverRef) -> bool {
    let x = d.borrow().in_transaction();
    x.unwrap_or_else(|| with_dbh(o, |x| x.in_txn).unwrap_or(false))
}

pub fn pdo_commit(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let d = driver_of(vm, &o)?;
    if !in_txn(&o, &d) {
        return Err(pdo_exception(
            vm,
            "There is no active transaction",
            None,
            "",
        ));
    }
    clear_err(&o);
    let r = d.borrow_mut().commit(vm);
    match r {
        Ok(()) => {
            with_dbh(&o, |x| x.in_txn = false);
            Ok(Value::Bool(true))
        }
        Err(f) => {
            dbh_fail(vm, &o, "PDO::commit", f)?;
            Ok(Value::Bool(false))
        }
    }
}

pub fn pdo_rollback(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let d = driver_of(vm, &o)?;
    if !in_txn(&o, &d) {
        return Err(pdo_exception(
            vm,
            "There is no active transaction",
            None,
            "",
        ));
    }
    clear_err(&o);
    let r = d.borrow_mut().rollback(vm);
    match r {
        Ok(()) => {
            with_dbh(&o, |x| x.in_txn = false);
            Ok(Value::Bool(true))
        }
        Err(f) => {
            dbh_fail(vm, &o, "PDO::rollBack", f)?;
            Ok(Value::Bool(false))
        }
    }
}

pub fn pdo_in_transaction(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let d = driver_of(vm, &o)?;
    Ok(Value::Bool(in_txn(&o, &d)))
}

pub fn pdo_error_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    driver_of(vm, &o)?;
    Ok(with_dbh(&o, |d| d.err.code_value()).unwrap_or_default())
}

pub fn pdo_error_info(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    driver_of(vm, &o)?;
    Ok(with_dbh(&o, |d| d.err.info()).unwrap_or_default())
}

pub fn pdo_exec(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let _caller = crate::builtins::sqlite_common::set_caller("PDO::exec");
    let o = a.this_obj().clone();
    let sql = a.str(0);
    if sql.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($statement) cannot be empty");
    }
    let d = driver_of(vm, &o)?;
    clear_err(&o);
    let r = match d.try_borrow_mut() {
        Ok(mut drv) => drv.exec(vm, sql.as_bytes()),
        Err(_) => Err(Failure::Db(DrvError::new(
            "HY000",
            Some(5),
            Some("database is locked".into()),
        ))),
    };
    match r {
        Ok(n) => Ok(Value::Int(n)),
        Err(f) => {
            dbh_fail(vm, &o, "PDO::exec", f)?;
            Ok(Value::Bool(false))
        }
    }
}

pub fn pdo_last_insert_id(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let d = driver_of(vm, &o)?;
    clear_err(&o);
    let name = a.opt_str(0);
    let r = d
        .borrow_mut()
        .last_insert_id(name.as_ref().map(|n| n.as_bytes()));
    match r {
        Ok(id) => Ok(Value::Str(PhpStr::from(id.as_slice()))),
        Err(f) => {
            dbh_fail(vm, &o, "PDO::lastInsertId", f)?;
            Ok(Value::Bool(false))
        }
    }
}

pub fn pdo_quote(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let d = driver_of(vm, &o)?;
    clear_err(&o);
    let s = a.str(0);
    let t = a.int_or(1, 2);
    if !d.borrow().supports_quote() {
        let e = DrvError::new(
            "IM001",
            None,
            Some("driver does not support quoting".into()),
        );
        dbh_error(vm, &o, "PDO::quote", &e)?;
        return Ok(Value::Bool(false));
    }
    let r = d.borrow_mut().quote(vm, s.as_bytes(), t)?;
    match r {
        Some(q) => Ok(Value::Str(PhpStr::from(q.as_slice()))),
        None => Ok(Value::Bool(false)),
    }
}

/// Checks an ATTR_STATEMENT_CLASS value: `[class]` or `[class, args]`,
/// a PDOStatement subclass without a public constructor.
pub fn check_stmt_class(vm: &mut Vm, fname: &str, v: &Value) -> R<(PhpStr, Option<Value>)> {
    let pre = format!("{fname}(): Argument #2 ($value) ");
    let arr = match v {
        Value::Array(a) => a.clone(),
        other => {
            return vm.type_error(&format!(
                "{pre}PDO::ATTR_STATEMENT_CLASS value must be of type array, {} given",
                Vm::type_name_given(other)
            ));
        }
    };
    let name = match arr.get_int(0).map(|v| v.deref()) {
        Some(Value::Str(s)) => s,
        _ => {
            return vm.type_error(&format!(
                "{pre}PDO::ATTR_STATEMENT_CLASS class must be a valid class"
            ));
        }
    };
    let Some(cls) = vm.find_class(name.as_bytes(), true)? else {
        return vm.type_error(&format!(
            "{pre}PDO::ATTR_STATEMENT_CLASS class must be a valid class"
        ));
    };
    let base = vm.find_class(b"PDOStatement", false)?.unwrap();
    if !cls.is_subclass_of(&base) {
        return vm.type_error(&format!(
            "{pre}PDO::ATTR_STATEMENT_CLASS class must be derived from PDOStatement"
        ));
    }
    if cls.has_flag(crate::class::cflags::ABSTRACT) {
        // Instantiation reports it later (test
        // pdo_ATTR_STATEMENT_CLASS_with_abstract_class.phpt).
    }
    if let Some(ctor) = cls.find_method(b"__construct")
        && ctor.flags & crate::class::mflags::PUBLIC != 0
        && ctor.flags & (crate::class::mflags::PROTECTED | crate::class::mflags::PRIVATE) == 0
        && !std::ptr::eq(ctor.scope.get(), &*base)
    {
        return vm.type_error(&format!(
            "{pre}User-supplied statement class cannot have a public constructor"
        ));
    }
    let args = match arr.get_int(1).map(|v| v.deref()) {
        None | Some(Value::Null) => None,
        Some(Value::Array(x)) => Some(Value::Array(x)),
        Some(other) => {
            return vm.type_error(&format!(
                "{pre}PDO::ATTR_STATEMENT_CLASS constructor_args must be of type ?array, {} given",
                Vm::type_name_given(&other)
            ));
        }
    };
    Ok((PhpStr::from(cls.name.as_bytes()), args))
}

/// setAttribute() and the constructor options.
fn set_attr(vm: &mut Vm, o: &ObjRef, fname: &str, attr: i64, v: &Value) -> R<bool> {
    let int_val = |vm: &mut Vm, v: &Value, what: &str| -> R<i64> {
        match v {
            Value::Int(i) => Ok(*i),
            Value::Bool(b) => Ok(*b as i64),
            Value::Str(s) if goro_runtime::numeric::is_numeric_str(s.as_bytes()) => {
                Ok(goro_runtime::convert::str_to_int(s.as_bytes()))
            }
            Value::Str(_) => vm.type_error(&format!(
                "{fname}(): Argument #2 ($value) {what}must be of type int, string given"
            )),
            other => vm.type_error(&format!(
                "{fname}(): Argument #2 ($value) {what}must be of type int, {} given",
                Vm::type_name_given(other)
            )),
        }
    };
    match attr {
        ATTR_ERRMODE => {
            let m = int_val(vm, v, "")?;
            if !(0..=2).contains(&m) {
                return vm.value_error(&format!(
                    "{fname}(): Argument #2 ($value) must be one of the PDO::ERRMODE_* constants"
                ));
            }
            with_dbh(o, |d| d.errmode = m);
            Ok(true)
        }
        ATTR_CASE => {
            let m = int_val(vm, v, "")?;
            if !(0..=2).contains(&m) {
                return vm.value_error(&format!(
                    "{fname}(): Argument #2 ($value) must be one of the PDO::CASE_* constants"
                ));
            }
            with_dbh(o, |d| d.case = m);
            Ok(true)
        }
        ATTR_ORACLE_NULLS => {
            let m = int_val(vm, v, "")?;
            if !(0..=2).contains(&m) {
                return vm.value_error(&format!(
                    "{fname}(): Argument #2 ($value) must be one of the PDO::NULL_* constants"
                ));
            }
            with_dbh(o, |d| d.oracle_nulls = m);
            Ok(true)
        }
        ATTR_DEFAULT_FETCH_MODE => {
            let m = match v {
                Value::Array(arr) => match arr.get_int(0).map(|x| x.deref()) {
                    Some(Value::Int(i)) => i,
                    _ => {
                        return vm.value_error(&format!(
                            "{fname}(): Argument #2 ($value) mode must be an integer"
                        ));
                    }
                },
                other => int_val(vm, other, "")?,
            };
            with_dbh(o, |d| d.default_fetch = m);
            Ok(true)
        }
        ATTR_STRINGIFY_FETCHES => {
            // Bool attributes take bools and ints (test bug_44159.phpt).
            let b = match v {
                Value::Bool(b) => *b,
                Value::Int(i) => *i != 0,
                other => {
                    return vm.type_error(&format!(
                        "Attribute value must be of type bool for selected attribute, {} given",
                        Vm::type_name_given(other)
                    ));
                }
            };
            with_dbh(o, |d| d.stringify = b);
            Ok(true)
        }
        ATTR_STATEMENT_CLASS => {
            if with_dbh(o, |d| d.persistent).unwrap_or(false) {
                return vm.type_error(&format!(
                    "{fname}(): Argument #2 ($value) PDO::ATTR_STATEMENT_CLASS cannot be used with persistent PDO instances"
                ));
            }
            let sc = check_stmt_class(vm, fname, v)?;
            let is_base = sc.0.as_bytes().eq_ignore_ascii_case(b"PDOStatement") && sc.1.is_none();
            with_dbh(o, |d| d.stmt_class = if is_base { None } else { Some(sc) });
            Ok(true)
        }
        ATTR_DEFAULT_STR_PARAM => {
            let m = int_val(vm, v, "")?;
            with_dbh(o, |d| d.default_str_param = m);
            Ok(true)
        }
        ATTR_AUTOCOMMIT
        | ATTR_EMULATE_PREPARES
        | ATTR_TIMEOUT
        | ATTR_PREFETCH
        | ATTR_FETCH_TABLE_NAMES
        | ATTR_FETCH_CATALOG_NAMES
        | ATTR_MAX_COLUMN_LEN
        | ATTR_CURSOR_NAME
        | ATTR_CURSOR
        | ATTR_PERSISTENT
        | ATTR_DRIVER_NAME
        | ATTR_SERVER_VERSION
        | ATTR_CLIENT_VERSION
        | ATTR_SERVER_INFO
        | ATTR_CONNECTION_STATUS
        | _ => {
            let d = driver_of(vm, o)?;
            let r = d.borrow_mut().set_attribute(vm, attr, v)?;
            match r {
                Some(ok) => Ok(ok),
                None => {
                    let e = DrvError::new(
                        "IM001",
                        None,
                        Some("driver does not support that attribute".into()),
                    );
                    let st = err_state(&e);
                    with_dbh(o, |d| d.err = st);
                    Ok(false)
                }
            }
        }
    }
}

pub fn pdo_set_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    driver_of(vm, &o)?;
    clear_err(&o);
    let attr = a.int(0);
    let v = a.val(1);
    Ok(Value::Bool(set_attr(
        vm,
        &o,
        "PDO::setAttribute",
        attr,
        &v,
    )?))
}

pub fn pdo_get_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let d = driver_of(vm, &o)?;
    clear_err(&o);
    let attr = a.int(0);
    let x = with_dbh(&o, |d| {
        Some(match attr {
            ATTR_PERSISTENT => Value::Bool(d.persistent),
            ATTR_CASE => Value::Int(d.case),
            ATTR_ORACLE_NULLS => Value::Int(d.oracle_nulls),
            ATTR_ERRMODE => Value::Int(d.errmode),
            ATTR_DRIVER_NAME => Value::Str(PhpStr::from("sqlite")),
            ATTR_STATEMENT_CLASS => {
                let mut arr = PhpArray::new();
                match &d.stmt_class {
                    Some((n, args)) => {
                        let _ = arr.push(Value::Str(n.clone()));
                        if let Some(a) = args {
                            let _ = arr.push(a.clone());
                        }
                    }
                    None => {
                        let _ = arr.push(Value::Str(PhpStr::from("PDOStatement")));
                    }
                }
                Value::Array(arr)
            }
            ATTR_DEFAULT_FETCH_MODE => Value::Int(d.default_fetch),
            ATTR_STRINGIFY_FETCHES => Value::Bool(d.stringify),
            ATTR_DEFAULT_STR_PARAM => Value::Int(d.default_str_param),
            _ => return None,
        })
    })
    .flatten();
    if let Some(v) = x {
        return Ok(v);
    }
    let r = d.borrow_mut().get_attribute(vm, attr)?;
    match r {
        Some(v) => Ok(v),
        None => {
            let e = DrvError::new(
                "IM001",
                None,
                Some("driver does not support that attribute".into()),
            );
            dbh_error(vm, &o, "PDO::getAttribute", &e)?;
            Ok(Value::Bool(false))
        }
    }
}

pub fn pdo_prepare(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let _caller = crate::builtins::sqlite_common::set_caller("PDO::prepare");
    let o = a.this_obj().clone();
    let sql = a.str(0);
    let opts = match a.get(1).map(|v| v.deref()) {
        Some(Value::Array(x)) => x,
        _ => PhpArray::new(),
    };
    driver_of(vm, &o)?;
    if sql.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($query) cannot be empty");
    }
    clear_err(&o);
    let sc = match opts.get_int(ATTR_STATEMENT_CLASS).map(|v| v.deref()) {
        Some(v) => Some(check_stmt_class(vm, "PDO::prepare", &v)?),
        None => with_dbh(&o, |d| d.stmt_class.clone()).flatten(),
    };
    match super::stmt::new_statement(vm, &o, &sql, &opts, sc, "PDO::prepare")? {
        Some(st) => Ok(Value::Object(st)),
        None => Ok(Value::Bool(false)),
    }
}

pub fn pdo_query(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let _caller = crate::builtins::sqlite_common::set_caller("PDO::query");
    let o = a.this_obj().clone();
    let sql = a.str(0);
    driver_of(vm, &o)?;
    if sql.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($query) cannot be empty");
    }
    clear_err(&o);
    let mode = a.opt_int(1);
    let extra: Vec<Value> = a.vals.iter().skip(2).map(|v| v.deref()).collect();
    if mode.is_none() && !extra.is_empty() {
        return vm.fthrow(
            a,
            "ArgumentCountError",
            "Argument #2 ($fetchMode) must be of type int when fetch mode arguments are given",
        );
    }
    let sc = with_dbh(&o, |d| d.stmt_class.clone()).flatten();
    let Some(st) = super::stmt::new_statement(vm, &o, &sql, &PhpArray::new(), sc, "PDO::query")?
    else {
        return Ok(Value::Bool(false));
    };
    if let Some(m) = mode {
        super::fetch::apply_fetch_mode(vm, &st, "PDO::query", m, &extra, 1)?;
    }
    if super::stmt::run_execute(vm, &st, "PDO::query", true)? {
        Ok(Value::Object(st))
    } else {
        Ok(Value::Bool(false))
    }
}
