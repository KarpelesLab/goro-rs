//! The PDO class (pdo_dbh.c).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_mysqli::zpp::{Arg, Func, parse};

use crate::core::*;
use crate::objects::{attach_stmt, dbh_data, dbh_of, in_transaction, stmt_of};

fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}

fn ce_name(o: &Obj) -> String {
    goro_core::value::display_class_name(&o.borrow().class_name)
}

/// The PDO object and its handle; PDO_CONSTRUCT_CHECK when `check`.
fn this_dbh(vm: &mut Vm, f: &Func, check: bool) -> Option<(Obj, DbhRef)> {
    let o = f.this.clone()?;
    let dbh = dbh_of(&o)?;
    if check && dbh.borrow().driver.is_none() {
        let m = format!("{} object is uninitialized", ce_name(&o));
        throw(vm, b"Error", &m);
        return None;
    }
    Some((o, dbh))
}

thread_local! {
    /// EG(persistent_list) entries of PDO handles
    static PERSISTENT: RefCell<Vec<(Vec<u8>, DbhRef)>> = const { RefCell::new(Vec::new()) };
}

/// PHP_MSHUTDOWN: the persistent handles are freed (php_pdo_pdbh_dtor).
pub fn free_persistent(_vm: &mut Vm) {
    let list = PERSISTENT.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for (_, d) in list {
        crate::objects::dbh_free(&d, true);
    }
}

/// The INI value of an option, as cfg_get_string() sees it.
fn cfg_get_string(vm: &Vm, name: &[u8]) -> Option<Vec<u8>> {
    match vm.constants.get(name) {
        Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
        Some(Value::Null) | None => None,
        Some(v) => Some(v.to_php_string().as_bytes().to_vec()),
    }
}

/// dsn_from_uri: the first line of the resource
fn dsn_from_uri(vm: &mut Vm, uri: &[u8]) -> Option<Vec<u8>> {
    let s = |b: &[u8]| Value::String(PhpString::from_bytes(b));
    let fopen = vm.functions.get(b"fopen".as_slice()).copied()?;
    let fgets = vm.functions.get(b"fgets".as_slice()).copied()?;
    let fclose = vm.functions.get(b"fclose".as_slice()).copied()?;
    let h = fopen(vm, &[s(uri), s(b"rb")]).ok()?;
    if !matches!(h, Value::Resource(_)) {
        return None;
    }
    // php_stream_get_line(stream, buf, 512): at most 511 bytes
    let line = fgets(vm, &[h.clone(), Value::Long(512)]).ok();
    let _ = fclose(vm, &[h]);
    match line {
        Some(Value::String(l)) => Some(l.as_bytes().to_vec()),
        _ => None,
    }
}

/// create_driver_specific_pdo_object: Ok(Some(object)) for connect().
fn create_driver_specific_pdo_object(
    vm: &mut Vm,
    f: &Func,
    driver: &'static Driver,
    called_scope: &[u8],
    is_connect: bool,
) -> Result<Option<Obj>, ()> {
    let ces = driver_specific_ces();
    let by_driver = ces
        .iter()
        .find(|(n, _)| *n == driver.name)
        .map(|(_, c)| c.clone());
    let mut by_called: Option<Vec<u8>> = None;
    for (_, ce) in &ces {
        if called_scope != b"pdo" && instanceof(vm, called_scope, ce) {
            by_called = Some(called_scope.to_vec());
            break;
        }
    }
    let what = if is_connect { "connect" } else { "__construct" };
    let called_disp = String::from_utf8_lossy(&class_display(vm, called_scope)).into_owned();
    let _ = f;
    if let Some(cc) = by_called {
        if let Some(bd) = &by_driver {
            if !instanceof(vm, &cc, bd) {
                let m = format!(
                    "{}::{}() cannot be used for connecting to the \"{}\" driver, either call {}::{}() or PDO::{}() instead",
                    called_disp,
                    what,
                    driver.name,
                    String::from_utf8_lossy(&class_display(vm, bd)),
                    what,
                    what
                );
                throw_pdo(vm, &m);
                return Err(());
            }
            if is_connect {
                return object_init_ex(vm, called_scope).map(Some).ok_or(());
            }
            return Ok(None);
        } else {
            let m = format!(
                "{}::{}() cannot be used for connecting to an unknown driver, call PDO::{}() instead",
                called_disp, what, what
            );
            throw_pdo(vm, &m);
            return Err(());
        }
    }
    if called_scope != b"pdo" && !is_connect {
        return Ok(None);
    }
    if let Some(bd) = by_driver {
        if called_scope != b"pdo" {
            let m = format!(
                "{}::{}() cannot be used for connecting to the \"{}\" driver, either call {}::{}() or PDO::{}() instead",
                called_disp,
                what,
                driver.name,
                String::from_utf8_lossy(&class_display(vm, &bd)),
                what,
                what
            );
            throw_pdo(vm, &m);
            return Err(());
        }
        if is_connect {
            return object_init_ex(vm, &bd).map(Some).ok_or(());
        }
    } else if is_connect {
        return object_init_ex(vm, called_scope).map(Some).ok_or(());
    }
    Ok(None)
}

/// zend_argument_error(pdo_exception_ce, 1, ...)
fn argument_error_pdo(vm: &mut Vm, f: &Func, msg: &str) {
    let m = format!("{} {}", f.arg_prefix(1), msg);
    throw_pdo(vm, &m);
}

/// php_pdo_internal_construct_driver
fn internal_construct(
    vm: &mut Vm,
    f: &Func,
    args: &[Value],
    current: Option<Obj>,
    called_scope: &[u8],
) -> Value {
    let a = match parse(vm, f, "s|s!s!a!", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let mut data_source = a[0].str().unwrap_or(b"").to_vec();
    // C strings end at the first NUL
    if let Some(p) = data_source.iter().position(|&c| c == 0) {
        data_source.truncate(p);
    }
    let username = a[1].str().map(|s| s.to_vec());
    let password = a[2].str().map(|s| s.to_vec());
    let options = match &a[3] {
        Arg::Arr(o) => Some(o.clone()),
        _ => None,
    };

    let mut colon = data_source.iter().position(|&c| c == b':');
    if colon.is_none() {
        let mut key = b"pdo.dsn.".to_vec();
        key.extend_from_slice(&data_source);
        key.truncate(511);
        match cfg_get_string(vm, &key) {
            Some(d) => {
                data_source = d;
                colon = data_source.iter().position(|&c| c == b':');
                if colon.is_none() {
                    let m = format!(
                        "invalid data source name (via INI: {})",
                        String::from_utf8_lossy(&key)
                    );
                    throw_pdo(vm, &m);
                    return Value::Null;
                }
            }
            None => {
                argument_error_pdo(vm, f, "must be a valid data source name");
                return Value::Null;
            }
        }
    }
    if data_source.starts_with(b"uri:") {
        let line = vm.current_line;
        vm.emit_deprecated_at(
            "Looking up the DSN from a URI is deprecated due to possible security concerns with DSNs coming from remote URIs",
            line,
        );
        if vm.current_exception.is_some() {
            return Value::Null;
        }
        let uri = data_source[4..].to_vec();
        match dsn_from_uri(vm, &uri) {
            Some(d) => data_source = d,
            None => {
                argument_error_pdo(vm, f, "must be a valid data source URI");
                return Value::Null;
            }
        }
        colon = data_source.iter().position(|&c| c == b':');
        if colon.is_none() {
            argument_error_pdo(vm, f, "must be a valid data source name (via URI)");
            return Value::Null;
        }
    }
    let colon = colon.unwrap();
    let driver = match find_driver(&data_source[..colon]) {
        Some(d) => d,
        None => {
            throw_pdo(vm, "could not find driver");
            return Value::Null;
        }
    };
    let new_obj =
        match create_driver_specific_pdo_object(vm, f, driver, called_scope, current.is_none()) {
            Ok(o) => o,
            Err(()) => return Value::Null,
        };
    let obj = match new_obj.clone().or(current) {
        Some(o) => o,
        None => return Value::Null,
    };
    let data = match dbh_data(&obj) {
        Some(d) => d,
        None => return Value::Null,
    };
    let mut dbh = data.inner.borrow().clone();
    let mut is_persistent = false;
    let mut call_factory = true;

    // is this supposed to be a persistent connection ?
    if let Some(opts) = &options {
        let v = opts
            .borrow()
            .get_int(PDO_ATTR_PERSISTENT)
            .map(|v| v.deref());
        let mut hash_key: Option<Vec<u8>> = None;
        if let Some(v) = v {
            let mut k = b"PDO:DBH:DSN=".to_vec();
            k.extend_from_slice(&data_source);
            k.push(b':');
            k.extend_from_slice(username.as_deref().unwrap_or(b""));
            k.push(b':');
            k.extend_from_slice(password.as_deref().unwrap_or(b""));
            let user_key = match &v {
                Value::String(s) => {
                    !s.is_empty()
                        && !matches!(
                            goro_core::vm::Vm::value_to_array_key(Value::String(s.clone())),
                            ArrayKey::Int(_)
                        )
                        && !is_numeric(s.as_bytes())
                }
                _ => false,
            };
            if user_key {
                k.push(b':');
                k.extend_from_slice(v.to_php_string().as_bytes());
                is_persistent = true;
            } else {
                is_persistent = zval_get_long(&v) != 0;
            }
            hash_key = Some(k);
        }
        if is_persistent {
            let key = hash_key.clone().unwrap();
            let mut pdbh = PERSISTENT.with(|p| {
                p.borrow()
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, d)| d.clone())
            });
            if let Some(p) = &pdbh {
                let live = p.borrow().methods.and_then(|m| m.check_liveness);
                if let Some(check) = live {
                    if !check(vm, p) {
                        // nope... need to kill it
                        {
                            let mut b = p.borrow_mut();
                            b.refcount = b.refcount.saturating_sub(1);
                        }
                        PERSISTENT.with(|l| l.borrow_mut().retain(|(k, _)| *k != key));
                        crate::objects::dbh_free(p, true);
                        pdbh = None;
                    }
                }
            }
            let pd = match pdbh {
                Some(p) => {
                    call_factory = false;
                    p
                }
                None => {
                    let mut d = Dbh::new();
                    d.refcount = 1;
                    d.is_persistent = true;
                    d.persistent_id = Some(key.clone());
                    d.def_stmt_ce = dbh.borrow().def_stmt_ce.clone();
                    Rc::new(RefCell::new(d))
                }
            };
            *data.inner.borrow_mut() = pd.clone();
            pd.borrow_mut().refcount += 1;
            dbh = pd;
        }
    }

    if call_factory {
        let mut d = dbh.borrow_mut();
        d.data_source = Some(data_source[colon + 1..].to_vec());
        d.username = username.clone();
        d.password = password.clone();
        d.default_fetch_type = PDO_FETCH_BOTH as u16;
    }
    {
        let ac = attr_lval(options.as_ref(), PDO_ATTR_AUTOCOMMIT, 1);
        let em = attr_lval(
            options.as_ref(),
            PDO_ATTR_ERRMODE,
            PDO_ERRMODE_EXCEPTION as i64,
        );
        let mut d = dbh.borrow_mut();
        d.auto_commit = ac != 0;
        d.error_mode = em as u8;
    }

    // warnings become PDOExceptions while constructing (EH_THROW)
    let prev_capture = vm.warning_capture.replace(Vec::new());
    let restore = |vm: &mut Vm| {
        let captured = std::mem::replace(&mut vm.warning_capture, prev_capture.clone());
        if let Some(first) = captured.and_then(|c| c.into_iter().next()) {
            if vm.current_exception.is_none() {
                throw_pdo(vm, &first);
            }
        }
    };

    let ok = if !call_factory {
        true
    } else {
        let r = (driver.factory)(vm, &dbh, options.as_ref());
        flush_mysqlnd(vm);
        if r && is_persistent {
            let key = dbh.borrow().persistent_id.clone().unwrap_or_default();
            PERSISTENT.with(|p| {
                let mut p = p.borrow_mut();
                p.retain(|(k, _)| *k != key);
                p.push((key, dbh.clone()));
            });
        }
        if r {
            dbh.borrow_mut().driver = Some(driver);
        }
        r
    };
    if ok {
        if let Some(opts) = &options {
            let entries: Vec<(ArrayKey, Value)> = opts
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (k, v) in entries {
                let k = match k {
                    ArrayKey::Int(n) => n,
                    ArrayKey::String(_) => continue,
                };
                let v = v.deref();
                attribute_set(vm, &dbh, k, &v, 3);
            }
        }
        restore(vm);
        return match new_obj {
            Some(o) => Value::Object(o),
            None => Value::Null,
        };
    }
    // the connection failed; things will tidy up in free_storage
    if is_persistent {
        let mut d = dbh.borrow_mut();
        d.refcount = d.refcount.saturating_sub(1);
    }
    restore(vm);
    if vm.current_exception.is_none() {
        throw_pdo(vm, "Constructor failed");
    }
    Value::Null
}

/// is_numeric_string() (any numeric string, int or float)
fn is_numeric(s: &[u8]) -> bool {
    let t = String::from_utf8_lossy(s);
    let t = t.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let t = t.trim_end_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    !t.is_empty() && t.parse::<f64>().is_ok() && !t.contains(['x', 'X', 'n', 'N', 'i', 'I'])
}

/// PDO::__construct
pub fn pdo_ctor(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let this = match &f.this {
        Some(o) => o.clone(),
        None => return Value::Null,
    };
    let cl = this.borrow().class_name.to_ascii_lowercase();
    internal_construct(vm, f, args, Some(this), &cl);
    Value::Null
}

/// PDO::connect
pub fn pdo_connect(vm: &mut Vm, f: &Func, args: &[Value], called: &[u8]) -> Value {
    let called = if called.is_empty() {
        b"pdo".to_vec()
    } else {
        called.to_ascii_lowercase()
    };
    internal_construct(vm, f, args, None, &called)
}

/// pdo_stmt_instantiate
fn stmt_instantiate(vm: &mut Vm, ce_lower: &[u8], has_ctor_args: bool) -> Option<Obj> {
    if has_ctor_args && constructor_of(vm, ce_lower).is_none() {
        throw(
            vm,
            b"Error",
            "User-supplied statement does not accept constructor arguments",
        );
        return None;
    }
    let o = match object_init_ex(vm, ce_lower) {
        Some(o) => o,
        None => {
            if vm.current_exception.is_none() {
                throw(
                    vm,
                    b"Error",
                    "Cannot instantiate user-supplied statement class",
                );
            }
            return None;
        }
    };
    attach_stmt(&o);
    Some(o)
}

/// pdo_stmt_construct
fn stmt_construct(vm: &mut Vm, stmt: &StmtRef, obj: &Obj, ctor_args: Option<&Arr>) {
    let q = stmt.borrow().query_string.clone().unwrap_or_default();
    obj.borrow_mut()
        .set_property(b"queryString".to_vec(), vstr(&q));
    let cl = obj.borrow().class_name.to_ascii_lowercase();
    if constructor_of(vm, &cl).is_some() {
        call_constructor(vm, obj, ctor_args);
    }
}

/// Whether a class's constructor is private or protected
fn ctor_is_non_public(vm: &Vm, ce_lower: &[u8]) -> Option<bool> {
    let c = vm.classes.get(ce_lower)?;
    let m = c.get_method(b"__construct")?;
    Some(m.visibility != goro_core::object::Visibility::Public)
}

/// Link a statement to its handle (stmt->dbh, database_object_handle)
fn link_stmt(stmt: &StmtRef, dbh: &DbhRef, dbh_obj: &Obj, query: &[u8]) {
    let mut s = stmt.borrow_mut();
    s.query_string = Some(query.to_vec());
    s.default_fetch_type = dbh.borrow().default_fetch_type as i64;
    s.dbh = Some(dbh.clone());
    s.database_object_handle = Some(dbh_obj.clone());
}

/// PDO::prepare
pub fn pdo_prepare(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "s|a", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (dbh_obj, dbh) = match this_dbh(vm, f, true) {
        Some(x) => x,
        None => return Value::Null,
    };
    let statement = a[0].str().unwrap_or(b"").to_vec();
    if statement.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Value::Null;
    }
    dbh_clear_err(&dbh);
    let options = match &a[1] {
        Arg::Arr(o) => Some(o.clone()),
        _ => None,
    };
    let mut dbstmt_ce: Vec<u8>;
    let ctor_args: Option<Value>;
    let class_opt = options
        .as_ref()
        .and_then(|o| o.borrow().get_int(PDO_ATTR_STATEMENT_CLASS).cloned());
    if let Some(value) = class_opt {
        let value = value.deref();
        let arr = match &value {
            Value::Array(a) => a.clone(),
            other => {
                let m = format!(
                    "PDO::ATTR_STATEMENT_CLASS value must be of type array, {} given",
                    Vm::value_type_name(other)
                );
                throw(vm, b"TypeError", &m);
                return Value::Null;
            }
        };
        let item = arr.borrow().get_int(0).cloned();
        let item = match item {
            Some(i) => i.deref(),
            None => {
                throw(
                    vm,
                    b"ValueError",
                    "PDO::ATTR_STATEMENT_CLASS value must be an array with the format array(classname, constructor_args)",
                );
                return Value::Null;
            }
        };
        let pce = match &item {
            Value::String(s) => lookup_class(vm, s.as_bytes()),
            _ => None,
        };
        if vm.current_exception.is_some() {
            return Value::Null;
        }
        dbstmt_ce = match pce {
            Some(c) => c,
            None => {
                throw(
                    vm,
                    b"TypeError",
                    "PDO::ATTR_STATEMENT_CLASS class must be a valid class",
                );
                return Value::Null;
            }
        };
        if !instanceof(vm, &dbstmt_ce, b"pdostatement") {
            throw(
                vm,
                b"TypeError",
                "PDO::ATTR_STATEMENT_CLASS class must be derived from PDOStatement",
            );
            return Value::Null;
        }
        if ctor_is_non_public(vm, &dbstmt_ce) == Some(false) {
            throw(
                vm,
                b"TypeError",
                "User-supplied statement class cannot have a public constructor",
            );
            return Value::Null;
        }
        let item1 = arr.borrow().get_int(1).cloned();
        match item1 {
            Some(i) => {
                if !matches!(i.deref(), Value::Array(_)) {
                    let m = format!(
                        "PDO::ATTR_STATEMENT_CLASS constructor_args must be of type ?array, {} given",
                        Vm::value_type_name(&value)
                    );
                    throw(vm, b"TypeError", &m);
                    return Value::Null;
                }
                ctor_args = Some(i.deref());
            }
            None => ctor_args = None,
        }
    } else {
        dbstmt_ce = dbh.borrow().def_stmt_ce.to_ascii_lowercase();
        ctor_args = dbh.borrow().def_stmt_ctor_args.clone();
    }
    dbstmt_ce = dbstmt_ce.to_ascii_lowercase();
    let obj = match stmt_instantiate(vm, &dbstmt_ce, ctor_args.is_some()) {
        Some(o) => o,
        None => return Value::Null,
    };
    let stmt = match stmt_of(&obj) {
        Some(s) => s,
        None => return Value::Null,
    };
    link_stmt(&stmt, &dbh, &dbh_obj, &statement);
    let preparer = dbh.borrow().methods.map(|m| m.preparer);
    let ok = match preparer {
        Some(p) => p(vm, &dbh, &statement, &stmt, options.as_ref()),
        None => false,
    };
    flush_mysqlnd(vm);
    if ok {
        let ca = match &ctor_args {
            Some(Value::Array(a)) => Some(a.clone()),
            _ => None,
        };
        stmt_construct(vm, &stmt, &obj, ca.as_ref());
        return Value::Object(obj);
    }
    handle_dbh_err(vm, &dbh);
    Value::False
}

/// PDO::beginTransaction
pub fn pdo_begin_transaction(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (_, dbh) = match this_dbh(vm, f, true) {
        Some(x) => x,
        None => return Value::Null,
    };
    if in_transaction(&dbh) {
        throw_pdo(vm, "There is already an active transaction");
        return Value::Null;
    }
    let begin = dbh.borrow().methods.and_then(|m| m.begin);
    let begin = match begin {
        Some(b) => b,
        None => {
            throw_pdo(vm, "This driver doesn't support transactions");
            return Value::Null;
        }
    };
    let r = begin(&dbh);
    flush_mysqlnd(vm);
    if r {
        dbh.borrow_mut().in_txn = true;
        return Value::True;
    }
    handle_dbh_err(vm, &dbh);
    Value::False
}

/// PDO::commit / PDO::rollBack
fn commit_or_rollback(vm: &mut Vm, f: &Func, args: &[Value], commit: bool) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (_, dbh) = match this_dbh(vm, f, true) {
        Some(x) => x,
        None => return Value::Null,
    };
    if !in_transaction(&dbh) {
        throw_pdo(vm, "There is no active transaction");
        return Value::Null;
    }
    let m = dbh.borrow().methods;
    let func = m.and_then(|m| if commit { m.commit } else { m.rollback });
    let r = match func {
        Some(func) => func(&dbh),
        None => false,
    };
    flush_mysqlnd(vm);
    if r {
        dbh.borrow_mut().in_txn = false;
        return Value::True;
    }
    handle_dbh_err(vm, &dbh);
    Value::False
}

pub fn pdo_commit(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    commit_or_rollback(vm, f, args, true)
}

pub fn pdo_rollback(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    commit_or_rollback(vm, f, args, false)
}

/// PDO::inTransaction
pub fn pdo_in_transaction(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (_, dbh) = match this_dbh(vm, f, true) {
        Some(x) => x,
        None => return Value::Null,
    };
    if in_transaction(&dbh) {
        Value::True
    } else {
        Value::False
    }
}

/// pdo_dbh_attribute_set
pub fn attribute_set(vm: &mut Vm, dbh: &DbhRef, attr: i64, value: &Value, argn: u32) -> bool {
    let value = value.deref();
    match attr {
        PDO_ATTR_ERRMODE => {
            let lval = match get_long_param(vm, &value) {
                Some(l) => l,
                None => return false,
            };
            match lval {
                0..=2 => {
                    dbh.borrow_mut().error_mode = lval as u8;
                    return true;
                }
                _ => {
                    arg_error(
                        vm,
                        b"ValueError",
                        argn,
                        "Error mode must be one of the PDO::ERRMODE_* constants",
                    );
                    return false;
                }
            }
        }
        PDO_ATTR_CASE => {
            let lval = match get_long_param(vm, &value) {
                Some(l) => l,
                None => return false,
            };
            match lval {
                0..=2 => {
                    dbh.borrow_mut().desired_case = lval as u8;
                    return true;
                }
                _ => {
                    arg_error(
                        vm,
                        b"ValueError",
                        argn,
                        "Case folding mode must be one of the PDO::CASE_* constants",
                    );
                    return false;
                }
            }
        }
        PDO_ATTR_ORACLE_NULLS => {
            let lval = match get_long_param(vm, &value) {
                Some(l) => l,
                None => return false,
            };
            dbh.borrow_mut().oracle_nulls = lval as u8;
            return true;
        }
        PDO_ATTR_DEFAULT_FETCH_MODE => {
            let lval;
            if let Value::Array(a) = &value {
                if let Some(Value::Long(t)) = a.borrow().get_int(0) {
                    if *t == PDO_FETCH_INTO || *t == PDO_FETCH_CLASS {
                        arg_error(
                            vm,
                            b"ValueError",
                            argn,
                            "PDO::FETCH_INTO and PDO::FETCH_CLASS cannot be set as the default fetch mode",
                        );
                        return false;
                    }
                }
                lval = zval_get_long(&value);
            } else {
                lval = match get_long_param(vm, &value) {
                    Some(l) => l,
                    None => return false,
                };
            }
            if lval == PDO_FETCH_USE_DEFAULT {
                arg_error(
                    vm,
                    b"ValueError",
                    argn,
                    "Fetch mode must be a bitmask of PDO::FETCH_* constants",
                );
                return false;
            }
            dbh.borrow_mut().default_fetch_type = lval as u16;
            return true;
        }
        PDO_ATTR_STRINGIFY_FETCHES => {
            let bval = match get_bool_param(vm, &value) {
                Some(b) => b,
                None => return false,
            };
            dbh.borrow_mut().stringify = bval;
            let sa = dbh.borrow().methods.and_then(|m| m.set_attribute);
            if let Some(sa) = sa {
                sa(vm, dbh, attr, &value);
                flush_mysqlnd(vm);
            }
            return true;
        }
        PDO_ATTR_STATEMENT_CLASS => {
            if dbh.borrow().is_persistent {
                raise_impl_error(
                    vm,
                    dbh,
                    None,
                    "HY000",
                    Some("PDO::ATTR_STATEMENT_CLASS cannot be used with persistent PDO instances"),
                );
                handle_dbh_err(vm, dbh);
                return false;
            }
            let arr = match &value {
                Value::Array(a) => a.clone(),
                other => {
                    let m = format!(
                        "PDO::ATTR_STATEMENT_CLASS value must be of type array, {} given",
                        Vm::value_type_name(other)
                    );
                    arg_error(vm, b"TypeError", argn, &m);
                    return false;
                }
            };
            let item = arr.borrow().get_int(0).cloned();
            let item = match item {
                Some(i) => i.deref(),
                None => {
                    arg_error(
                        vm,
                        b"ValueError",
                        argn,
                        "PDO::ATTR_STATEMENT_CLASS value must be an array with the format array(classname, constructor_args)",
                    );
                    return false;
                }
            };
            let pce = match &item {
                Value::String(s) => lookup_class(vm, s.as_bytes()),
                _ => None,
            };
            if vm.current_exception.is_some() {
                return false;
            }
            let pce = match pce {
                Some(c) => c,
                None => {
                    arg_error(
                        vm,
                        b"TypeError",
                        argn,
                        "PDO::ATTR_STATEMENT_CLASS class must be a valid class",
                    );
                    return false;
                }
            };
            if !instanceof(vm, &pce, b"pdostatement") {
                arg_error(
                    vm,
                    b"TypeError",
                    argn,
                    "PDO::ATTR_STATEMENT_CLASS class must be derived from PDOStatement",
                );
                return false;
            }
            if ctor_is_non_public(vm, &pce) == Some(false) {
                arg_error(
                    vm,
                    b"TypeError",
                    argn,
                    "User-supplied statement class cannot have a public constructor",
                );
                return false;
            }
            let disp = class_display(vm, &pce);
            {
                let mut d = dbh.borrow_mut();
                d.def_stmt_ce = disp;
                d.def_stmt_ctor_args = None;
            }
            let item1 = arr.borrow().get_int(1).cloned();
            if let Some(i) = item1 {
                if !matches!(i.deref(), Value::Array(_)) {
                    let m = format!(
                        "PDO::ATTR_STATEMENT_CLASS constructor_args must be of type ?array, {} given",
                        Vm::value_type_name(&value)
                    );
                    arg_error(vm, b"TypeError", argn, &m);
                    return false;
                }
                dbh.borrow_mut().def_stmt_ctor_args = Some(i.deref());
            }
            return true;
        }
        _ => {}
    }
    let sa = dbh.borrow().methods.and_then(|m| m.set_attribute);
    let sa = match sa {
        Some(s) => s,
        None => {
            raise_impl_error(
                vm,
                dbh,
                None,
                "IM001",
                Some("driver does not support setting attributes"),
            );
            return false;
        }
    };
    dbh_clear_err(dbh);
    let r = sa(vm, dbh, attr, &value);
    flush_mysqlnd(vm);
    if r {
        return true;
    }
    if vm.current_exception.is_none() {
        handle_dbh_err(vm, dbh);
    }
    false
}

/// PDO::setAttribute
pub fn pdo_set_attribute(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "lz", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let o = match &f.this {
        Some(o) => o.clone(),
        None => return Value::Null,
    };
    if let Some(d) = dbh_of(&o) {
        dbh_clear_err(&d);
    }
    let (_, dbh) = match this_dbh(vm, f, true) {
        Some(x) => x,
        None => return Value::Null,
    };
    let attr = a[0].long_or(0);
    let value = match &a[1] {
        Arg::Any(v) => v.clone(),
        _ => Value::Null,
    };
    if attribute_set(vm, &dbh, attr, &value, 2) {
        Value::True
    } else {
        Value::False
    }
}

/// PDO::getAttribute
pub fn pdo_get_attribute(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "l", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let o = match &f.this {
        Some(o) => o.clone(),
        None => return Value::Null,
    };
    if let Some(d) = dbh_of(&o) {
        dbh_clear_err(&d);
    }
    let (_, dbh) = match this_dbh(vm, f, true) {
        Some(x) => x,
        None => return Value::Null,
    };
    let attr = a[0].long_or(0);
    {
        let d = dbh.borrow();
        match attr {
            PDO_ATTR_PERSISTENT => return vbool(d.is_persistent),
            PDO_ATTR_CASE => return Value::Long(d.desired_case as i64),
            PDO_ATTR_ORACLE_NULLS => return Value::Long(d.oracle_nulls as i64),
            PDO_ATTR_ERRMODE => return Value::Long(d.error_mode as i64),
            PDO_ATTR_DRIVER_NAME => return vstr(d.driver.map(|x| x.name).unwrap_or("")),
            PDO_ATTR_STATEMENT_CLASS => {
                let mut arr = PhpArray::new();
                arr.push(vstr(&d.def_stmt_ce));
                if let Some(ca) = &d.def_stmt_ctor_args {
                    arr.push(ca.clone());
                }
                return Value::Array(Rc::new(RefCell::new(arr)));
            }
            PDO_ATTR_DEFAULT_FETCH_MODE => return Value::Long(d.default_fetch_type as i64),
            PDO_ATTR_STRINGIFY_FETCHES => return vbool(d.stringify),
            _ => {}
        }
    }
    let ga = dbh.borrow().methods.and_then(|m| m.get_attribute);
    let ga = match ga {
        Some(g) => g,
        None => {
            raise_impl_error(
                vm,
                &dbh,
                None,
                "IM001",
                Some("driver does not support getting attributes"),
            );
            return Value::False;
        }
    };
    let r = ga(vm, &dbh, attr);
    flush_mysqlnd(vm);
    match r {
        AttrResult::Error => {
            handle_dbh_err(vm, &dbh);
            Value::False
        }
        AttrResult::Unsupported => {
            raise_impl_error(
                vm,
                &dbh,
                None,
                "IM001",
                Some("driver does not support that attribute"),
            );
            Value::False
        }
        AttrResult::Value(v) => v,
    }
}

/// PDO::exec
pub fn pdo_exec(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "s", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let statement = a[0].str().unwrap_or(b"").to_vec();
    if statement.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Value::Null;
    }
    let o = match &f.this {
        Some(o) => o.clone(),
        None => return Value::Null,
    };
    if let Some(d) = dbh_of(&o) {
        dbh_clear_err(&d);
    }
    let (_, dbh) = match this_dbh(vm, f, true) {
        Some(x) => x,
        None => return Value::Null,
    };
    let doer = dbh.borrow().methods.map(|m| m.doer);
    let ret = match doer {
        Some(d) => d(vm, &dbh, &statement),
        None => -1,
    };
    flush_mysqlnd(vm);
    if ret == -1 {
        handle_dbh_err(vm, &dbh);
        Value::False
    } else {
        Value::Long(ret)
    }
}

/// PDO::lastInsertId
pub fn pdo_last_insert_id(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "|s!", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, dbh) = match this_dbh(vm, f, true) {
        Some(x) => x,
        None => return Value::Null,
    };
    dbh_clear_err(&dbh);
    let li = dbh.borrow().methods.and_then(|m| m.last_id);
    let li = match li {
        Some(l) => l,
        None => {
            raise_impl_error(
                vm,
                &dbh,
                None,
                "IM001",
                Some("driver does not support lastInsertId()"),
            );
            return Value::False;
        }
    };
    let name = a[0].str().map(|s| s.to_vec());
    match li(vm, &dbh, name.as_deref()) {
        Some(id) => vstr(id),
        None => {
            handle_dbh_err(vm, &dbh);
            Value::False
        }
    }
}

/// PDO::errorCode
pub fn pdo_error_code(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (_, dbh) = match this_dbh(vm, f, true) {
        Some(x) => x,
        None => return Value::Null,
    };
    let d = dbh.borrow();
    if let Some((qs, _)) = &d.query_stmt {
        return vstr(&qs.borrow().error_code);
    }
    if d.error_code.is_empty() {
        return Value::Null;
    }
    vstr(&d.error_code)
}

/// PDO::errorInfo
pub fn pdo_error_info(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (_, dbh) = match this_dbh(vm, f, true) {
        Some(x) => x,
        None => return Value::Null,
    };
    let mut arr = PhpArray::new();
    let (code, qs, fe) = {
        let d = dbh.borrow();
        let qs = d.query_stmt.as_ref().map(|(s, _)| s.clone());
        let code = match &qs {
            Some(s) => s.borrow().error_code.clone(),
            None => d.error_code.clone(),
        };
        (code, qs, d.methods.and_then(|m| m.fetch_err))
    };
    arr.push(vstr(&code));
    if code.as_slice() != PDO_ERR_NONE {
        if let Some(fe) = fe {
            fe(vm, &dbh, qs.as_ref(), &mut arr);
        }
    }
    while arr.len() < 3 {
        arr.push(Value::Null);
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}

/// PDO::query
pub fn pdo_query(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "s|l!*", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (dbh_obj, dbh) = match this_dbh(vm, f, true) {
        Some(x) => x,
        None => return Value::Null,
    };
    let statement = a[0].str().unwrap_or(b"").to_vec();
    if statement.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Value::Null;
    }
    let fetch_mode = a[1].long();
    let rest: Vec<Value> = match &a[2] {
        Arg::Rest(r) => r.iter().map(|v| v.deref()).collect(),
        _ => Vec::new(),
    };
    dbh_clear_err(&dbh);
    let (ce, ctor_args) = {
        let d = dbh.borrow();
        (
            d.def_stmt_ce.to_ascii_lowercase(),
            d.def_stmt_ctor_args.clone(),
        )
    };
    let obj = match stmt_instantiate(vm, &ce, ctor_args.is_some()) {
        Some(o) => o,
        None => return Value::Null,
    };
    let stmt = match stmt_of(&obj) {
        Some(s) => s,
        None => return Value::Null,
    };
    link_stmt(&stmt, &dbh, &dbh_obj, &statement);
    {
        let mut s = stmt.borrow_mut();
        s.active_query_string = Some(statement.clone());
        s.active_is_query = true;
    }
    let preparer = dbh.borrow().methods.map(|m| m.preparer);
    let ok = match preparer {
        Some(p) => p(vm, &dbh, &statement, &stmt, None),
        None => false,
    };
    flush_mysqlnd(vm);
    if ok {
        stmt.borrow_mut().clear_err();
        let mode_ok = match fetch_mode {
            None => true,
            Some(m) => crate::stmt::setup_fetch_mode(vm, &stmt, m, 2, &rest),
        };
        if mode_ok {
            stmt.borrow_mut().clear_err();
            let exec = stmt.borrow().methods.map(|m| m.executer);
            let executed = match exec {
                Some(e) => e(vm, &stmt),
                None => false,
            };
            flush_mysqlnd(vm);
            if executed {
                let mut ret = true;
                if !stmt.borrow().executed {
                    if dbh.borrow().alloc_own_columns {
                        ret = crate::stmt::describe_columns(vm, &stmt);
                    }
                    stmt.borrow_mut().executed = true;
                }
                if ret {
                    let ca = match &ctor_args {
                        Some(Value::Array(a)) => Some(a.clone()),
                        _ => None,
                    };
                    stmt_construct(vm, &stmt, &obj, ca.as_ref());
                    return Value::Object(obj);
                }
            }
        }
        // something broke
        stmt.borrow_mut().database_object_handle = None;
        dbh.borrow_mut().query_stmt = Some((stmt.clone(), obj));
        handle_stmt_err(vm, &stmt);
    } else {
        handle_dbh_err(vm, &dbh);
    }
    Value::False
}

/// PDO::quote
pub fn pdo_quote(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "s|l", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, dbh) = match this_dbh(vm, f, true) {
        Some(x) => x,
        None => return Value::Null,
    };
    dbh_clear_err(&dbh);
    let q = dbh.borrow().methods.and_then(|m| m.quoter);
    let q = match q {
        Some(q) => q,
        None => {
            raise_impl_error(
                vm,
                &dbh,
                None,
                "IM001",
                Some("driver does not support quoting"),
            );
            return Value::False;
        }
    };
    let s = a[0].str().unwrap_or(b"").to_vec();
    let ptype = a[1].long_or(PDO_PARAM_STR);
    match q(vm, &dbh, &s, ptype) {
        Some(r) => vstr(r),
        None => {
            handle_dbh_err(vm, &dbh);
            Value::False
        }
    }
}

/// PDO::getAvailableDrivers / pdo_drivers()
pub fn available_drivers() -> Value {
    let mut arr = PhpArray::new();
    for d in drivers() {
        arr.push(vstr(d.name));
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}

pub fn pdo_get_available_drivers(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    available_drivers()
}
