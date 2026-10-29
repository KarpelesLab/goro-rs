//! `PDOStatement` (PHP manual: The PDOStatement class): parameters,
//! execution, errors, column metadata and `debugDumpParams()`.

use std::rc::Rc;

use goro_runtime::{Key, KeyRef, ObjRef, PhpArray, PhpStr, Value};

use super::core::{
    self, DbhData, ERRMODE_EXCEPTION, ErrState, driver_of, err_state, report, with_dbh,
};
use super::driver::{DResult, DriverStmt, DrvError, ExecParam, Failure, ParamKey};
use super::fetch::FetchMode;
use crate::class::NativeHooks;
use crate::func::Args;
use crate::vm::{R, Vm};

pub const PARAM_NULL: i64 = 0;
pub const PARAM_INT: i64 = 1;
pub const PARAM_STR: i64 = 2;
pub const PARAM_LOB: i64 = 3;
pub const PARAM_STMT: i64 = 4;
pub const PARAM_BOOL: i64 = 5;
pub const PARAM_INPUT_OUTPUT: i64 = 0x8000_0000;

/// A bound parameter or column.
#[derive(Clone)]
pub struct Bound {
    pub key: ParamKey,
    /// A reference (bindParam/bindColumn) or the value (bindValue).
    pub var: Value,
    pub ptype: i64,
}

/// A PDOStatement object's state.
pub struct StmtData {
    pub dbh: Option<ObjRef>,
    pub drv: Option<Box<dyn DriverStmt>>,
    /// Whether a driver statement exists (also while `drv` is lent out).
    pub init: bool,
    pub params: Vec<Bound>,
    pub cols: Vec<Bound>,
    pub fetch: FetchMode,
    pub executed: bool,
    /// Column names after execute (ATTR_CASE applied).
    pub columns: Option<Vec<PhpStr>>,
    pub err: ErrState,
    /// The PDORow of FETCH_LAZY fetches.
    pub lazy: Option<goro_runtime::object::WeakObj>,
    /// Inside a fetch that runs user code (constructors).
    pub fetching: bool,
}

impl StmtData {
    fn new() -> StmtData {
        StmtData {
            dbh: None,
            drv: None,
            init: false,
            params: Vec::new(),
            cols: Vec::new(),
            fetch: FetchMode::default(),
            executed: false,
            columns: None,
            err: ErrState::unset(),
            lazy: None,
            fetching: false,
        }
    }
}

pub struct StmtObj(pub StmtData);

impl goro_runtime::NativeData for StmtObj {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        let s = &self.0;
        if let Some(d) = &s.dbh {
            visit(&Value::Object(d.clone()));
        }
        for b in s.params.iter().chain(s.cols.iter()) {
            visit(&b.var);
        }
        s.fetch.trace(visit);
    }
    fn clear(&mut self) {
        let s = &mut self.0;
        s.dbh = None;
        s.params.clear();
        s.cols.clear();
        s.fetch = FetchMode::default();
        s.lazy = None;
    }
}

pub fn with_st<T>(o: &ObjRef, f: impl FnOnce(&mut StmtData) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<StmtObj>().map(|d| f(&mut d.0))
}

pub fn stmt_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            core::mark_not_serializable(vm, o);
            *o.obj().native.borrow_mut() = Some(Box::new(StmtObj(StmtData::new())));
        }),
        get_iterator: Some(super::fetch::iterator_hook),
        ..Default::default()
    }
}

/// The handle settings a statement uses.
pub struct DbhView {
    pub errmode: i64,
    pub case: i64,
    pub oracle_nulls: i64,
    pub stringify: bool,
}

pub fn dbh_view(st: &ObjRef) -> DbhView {
    let dbh = with_st(st, |s| s.dbh.clone()).flatten();
    dbh.and_then(|d| {
        with_dbh(&d, |x: &mut DbhData| DbhView {
            errmode: x.errmode,
            case: x.case,
            oracle_nulls: x.oracle_nulls,
            stringify: x.stringify,
        })
    })
    .unwrap_or(DbhView {
        errmode: ERRMODE_EXCEPTION,
        case: 0,
        oracle_nulls: 0,
        stringify: false,
    })
}

/// Checks that the statement was created by PDO ("X object is
/// uninitialized" Error otherwise).
pub fn check_init(vm: &mut Vm, o: &ObjRef) -> R<()> {
    if with_st(o, |s| s.init).unwrap_or(false) {
        return Ok(());
    }
    let cls = vm.class_of(o).name.to_string_lossy();
    vm.throw("Error", &format!("{cls} object is uninitialized"))
}

/// Records an error on the statement and reports it by the handle's
/// error mode.
pub fn stmt_error(vm: &mut Vm, o: &ObjRef, fname: &str, e: &DrvError) -> R<()> {
    let st = err_state(e);
    let info = st.info();
    with_st(o, |s| s.err = st);
    let mode = dbh_view(o).errmode;
    report(vm, mode, fname, e, info)
}

pub fn stmt_fail(vm: &mut Vm, o: &ObjRef, fname: &str, f: Failure) -> R<()> {
    match f {
        Failure::Db(e) => stmt_error(vm, o, fname, &e),
        Failure::Php(u) => Err(u),
    }
}

/// Runs `f` on the driver statement, lent out of the object so that
/// user code running inside (functions, constructors) can use it.
pub fn with_drv<T>(o: &ObjRef, f: impl FnOnce(&mut Box<dyn DriverStmt>) -> T) -> Option<T> {
    let mut d = with_st(o, |s| s.drv.take()).flatten()?;
    let r = f(&mut d);
    with_st(o, |s| {
        if s.drv.is_none() {
            s.drv = Some(d);
        }
    });
    Some(r)
}

/// Creates a statement object for `sql` (PDO::prepare and PDO::query):
/// the class instantiated, the driver statement prepared, then the
/// constructor run with the class's arguments.
pub fn new_statement(
    vm: &mut Vm,
    dbh: &ObjRef,
    sql: &PhpStr,
    opts: &PhpArray,
    class: Option<(PhpStr, Option<Value>)>,
    fname: &str,
) -> R<Option<ObjRef>> {
    let cname = class
        .as_ref()
        .map(|c| c.0.clone())
        .unwrap_or_else(|| PhpStr::from("PDOStatement"));
    let cls = match vm.find_class(cname.as_bytes(), true)? {
        Some(c) => c,
        None => vm.find_class(b"PDOStatement", false)?.unwrap(),
    };
    let st = vm.instantiate(&cls)?;
    vm.set_prop_raw(&st, b"queryString", Value::Str(sql.clone()));
    let d = driver_of(vm, dbh)?;
    let r = match d.try_borrow_mut() {
        Ok(mut drv) => drv.prepare(vm, sql.as_bytes(), opts),
        Err(_) => Err(Failure::Db(DrvError::new(
            "HY000",
            Some(5),
            Some("database is locked".into()),
        ))),
    };
    let drv = match r {
        Ok(x) => x,
        Err(f) => {
            core::dbh_fail(vm, dbh, fname, f)?;
            return Ok(None);
        }
    };
    let default_fetch = with_dbh(dbh, |x| x.default_fetch).unwrap_or(core::FETCH_BOTH);
    with_st(&st, |s| {
        s.dbh = Some(dbh.clone());
        s.drv = Some(drv);
        s.init = true;
        s.fetch = FetchMode::with_mode(default_fetch);
    });
    if let Some(ctor) = cls.find_method(b"__construct")
        && !ctor.is_native()
    {
        let args = match class.and_then(|c| c.1) {
            Some(Value::Array(a)) => a,
            _ => PhpArray::new(),
        };
        let c = crate::vm::calls::Callee {
            func: ctor.clone(),
            this: Some(st.clone()),
            static_class: &*cls as *const _,
            scope: ctor.scope.get(),
            closure: None,
            magic_name: None,
        };
        vm.call_callee_named(c, args)?;
    } else if let Some((_, Some(Value::Array(args)))) = &class
        && !args.is_empty()
    {
        return vm.throw(
            "Error",
            "User-supplied statement does not accept constructor arguments",
        );
    }
    Ok(Some(st))
}

/// A parameter key from bindParam()/bindValue()/execute(): 1-based
/// positions (0-based inside), names with a leading `:` added.
fn param_key(v: &Value) -> ParamKey {
    match v {
        Value::Int(i) => ParamKey::Pos((*i - 1).max(-1) as usize),
        Value::Str(s) => {
            let b = s.as_bytes();
            if b.first() == Some(&b':') {
                ParamKey::Name(b.to_vec())
            } else {
                let mut n = vec![b':'];
                n.extend_from_slice(b);
                ParamKey::Name(n)
            }
        }
        _ => ParamKey::Pos(0),
    }
}

fn register_param(
    vm: &mut Vm,
    a: &Args,
    o: &ObjRef,
    var: Value,
    ptype: i64,
    is_col: bool,
) -> R<bool> {
    let keyv = a.val(0);
    if let Value::Int(i) = keyv
        && i < 1
        && !is_col
    {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($param) must be greater than or equal to 1",
        );
    }
    let key = if is_col {
        match &keyv {
            Value::Int(i) => ParamKey::Pos((*i - 1).max(0) as usize),
            Value::Str(s) => ParamKey::Name(s.as_bytes().to_vec()),
            _ => ParamKey::Pos(0),
        }
    } else {
        param_key(&keyv)
    };
    // A typed check: PARAM_* must be a known type.
    let base = ptype & !PARAM_INPUT_OUTPUT & 0xffff;
    if !is_col && !(0..=5).contains(&base) {
        let e = DrvError::new("HY000", None, None);
        stmt_error(vm, o, &a.fname(), &e)?;
        return Ok(false);
    }
    with_st(o, |s| {
        let list = if is_col { &mut s.cols } else { &mut s.params };
        let b = Bound { key, var, ptype };
        if let Some(p) = list.iter_mut().find(|x| x.key == b.key) {
            *p = b;
        } else {
            list.push(b);
        }
    });
    Ok(true)
}

pub fn bind_param(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let var = a.get(1).cloned().unwrap_or_default();
    let t = a.int_or(2, PARAM_STR);
    Ok(Value::Bool(register_param(vm, a, &o, var, t, false)?))
}

pub fn bind_value(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let var = a.val(1);
    let t = a.int_or(2, PARAM_STR);
    Ok(Value::Bool(register_param(vm, a, &o, var, t, false)?))
}

pub fn bind_column(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let var = a.get(1).cloned().unwrap_or_default();
    let t = a.int_or(2, PARAM_STR);
    Ok(Value::Bool(register_param(vm, a, &o, var, t, true)?))
}

/// Executes the statement: binds the parameters, runs it, and reads the
/// result columns. `Ok(false)` after a reported error.
pub fn run_execute(vm: &mut Vm, o: &ObjRef, fname: &str, from_query: bool) -> R<bool> {
    // A stream bound by reference as a LOB is read, and the variable
    // takes its contents (test ext/pdo_sqlite/tests/bug70862.phpt).
    let lobs: Vec<goro_runtime::PhpRef> = with_st(o, |s| {
        s.params
            .iter()
            .filter(|b| b.ptype & 0xff == PARAM_LOB)
            .filter_map(|b| match &b.var {
                Value::Ref(r) if matches!(r.get(), Value::Resource(_)) => Some(r.clone()),
                _ => None,
            })
            .collect()
    })
    .unwrap_or_default();
    for r in lobs {
        let f = Value::Str(PhpStr::from("stream_get_contents"));
        let data = match vm.call_value(&f, vec![r.get()])? {
            Value::Str(s) => s,
            _ => PhpStr::empty(),
        };
        let old = r.set(Value::Str(data));
        drop(old);
    }
    let params: Vec<ExecParam> = with_st(o, |s| {
        s.params
            .iter()
            .map(|b| ExecParam {
                key: b.key.clone(),
                value: b.var.deref(),
                ptype: b.ptype,
            })
            .collect()
    })
    .unwrap_or_default();
    with_st(o, |s| {
        s.err = ErrState::default();
        s.columns = None;
    });
    let r: Option<DResult<()>> = with_drv(o, |d| d.execute(vm, &params));
    let r = r.unwrap_or(Ok(()));
    if let Err(f) = r {
        if from_query {
            let dbh = with_st(o, |s| s.dbh.clone()).flatten();
            if let Some(dbh) = dbh {
                core::dbh_fail(vm, &dbh, fname, f)?;
                return Ok(false);
            }
        }
        stmt_fail(vm, o, fname, f)?;
        return Ok(false);
    }
    describe(o);
    with_st(o, |s| s.executed = true);
    Ok(true)
}

/// Reads the column names of the result (ATTR_CASE applied).
pub fn describe(o: &ObjRef) {
    let case = dbh_view(o).case;
    let names = with_drv(o, |d| {
        (0..d.column_count())
            .map(|i| {
                let mut n = d.column_name(i);
                match case {
                    1 => n.make_ascii_uppercase(),
                    2 => n.make_ascii_lowercase(),
                    _ => {}
                }
                PhpStr::from(n.as_slice())
            })
            .collect::<Vec<_>>()
    });
    with_st(o, |s| s.columns = names);
}

pub fn execute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let _caller = crate::builtins::sqlite_common::set_caller("PDOStatement::execute");
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    if let Some(Value::Array(arr)) = a.get(0).map(|v| v.deref()) {
        let mut list = Vec::new();
        for (k, v) in arr.iter() {
            let key = match k {
                KeyRef::Int(i) => ParamKey::Pos(i.max(0) as usize),
                KeyRef::Str(s) => param_key(&Value::Str(s.clone())),
            };
            list.push(Bound {
                key,
                var: v.deref(),
                ptype: PARAM_STR,
            });
        }
        with_st(&o, |s| s.params = list);
    }
    Ok(Value::Bool(run_execute(
        vm,
        &o,
        "PDOStatement::execute",
        false,
    )?))
}

pub fn close_cursor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    with_drv(&o, |d| d.close_cursor());
    with_st(&o, |s| s.err = ErrState::default());
    Ok(Value::Bool(true))
}

pub fn column_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let n = with_st(&o, |s| s.columns.as_ref().map_or(0, |c| c.len())).unwrap_or(0);
    Ok(Value::Int(n as i64))
}

pub fn row_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    Ok(Value::Int(with_drv(&o, |d| d.row_count()).unwrap_or(0)))
}

pub fn error_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    Ok(with_st(&o, |s| s.err.code_value()).unwrap_or_default())
}

pub fn error_info(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    Ok(with_st(&o, |s| s.err.info()).unwrap_or_default())
}

pub fn next_rowset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let r = with_drv(&o, |d| d.next_rowset()).unwrap_or(Ok(false));
    match r {
        Ok(true) => {
            describe(&o);
            Ok(Value::Bool(true))
        }
        Ok(false) => {
            // Not supported by the driver: IM001.
            let e = DrvError::new(
                "IM001",
                None,
                Some("driver does not support multiple rowsets".into()),
            );
            stmt_error(vm, &o, "PDOStatement::nextRowset", &e)?;
            Ok(Value::Bool(false))
        }
        Err(f) => {
            stmt_fail(vm, &o, "PDOStatement::nextRowset", f)?;
            Ok(Value::Bool(false))
        }
    }
}

pub fn get_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let attr = a.int(0);
    let r = with_drv(&o, |d| d.get_attribute(vm, attr)).unwrap_or(Ok(None))?;
    match r {
        Some(v) => Ok(v),
        None => {
            let e = DrvError::new(
                "IM001",
                None,
                Some("driver doesn't support getting that attribute".into()),
            );
            stmt_error(vm, &o, "PDOStatement::getAttribute", &e)?;
            Ok(Value::Bool(false))
        }
    }
}

pub fn set_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let attr = a.int(0);
    let v = a.val(1);
    let r = with_drv(&o, |d| d.set_attribute(vm, attr, &v)).unwrap_or(Ok(None))?;
    match r {
        Some(b) => Ok(Value::Bool(b)),
        None => {
            let e = DrvError::new(
                "IM001",
                None,
                Some("driver doesn't support setting attributes".into()),
            );
            stmt_error(vm, &o, "PDOStatement::setAttribute", &e)?;
            Ok(Value::Bool(false))
        }
    }
}

pub fn get_column_meta(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let i = a.int(0);
    if i < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($column) must be greater than or equal to 0",
        );
    }
    let Some(names) = with_st(&o, |s| s.columns.clone()).flatten() else {
        return Ok(Value::Bool(false));
    };
    let i = i as usize;
    if i >= names.len() {
        return Ok(Value::Bool(false));
    }
    let r = with_drv(&o, |d| d.column_meta(vm, i)).unwrap_or(Ok(None));
    match r {
        Ok(Some(mut m)) => {
            m.insert(Key::from_bytes(b"name"), Value::Str(names[i].clone()));
            m.insert(Key::from_bytes(b"len"), Value::Int(-1));
            m.insert(Key::from_bytes(b"precision"), Value::Int(0));
            Ok(Value::Array(m))
        }
        Ok(None) => Ok(Value::Bool(false)),
        Err(f) => {
            stmt_fail(vm, &o, "PDOStatement::getColumnMeta", f)?;
            Ok(Value::Bool(false))
        }
    }
}

/// `debugDumpParams()` (PHP manual): the SQL, the parameter count and
/// each bound parameter.
pub fn debug_dump_params(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    check_init(vm, &o)?;
    let sql = vm.get_prop_raw(&o, b"queryString");
    let sql = match sql {
        Value::Str(s) => s,
        _ => PhpStr::empty(),
    };
    let params = with_st(&o, |s| s.params.clone()).unwrap_or_default();
    let mut out = Vec::new();
    out.extend_from_slice(format!("SQL: [{}] ", sql.len()).as_bytes());
    out.extend_from_slice(sql.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(format!("Params:  {}\n", params.len()).as_bytes());
    for p in &params {
        match &p.key {
            ParamKey::Name(n) => {
                out.extend_from_slice(format!("Key: Name: [{}] ", n.len()).as_bytes());
                out.extend_from_slice(n);
                out.push(b'\n');
                out.extend_from_slice(b"paramno=-1\n");
                out.extend_from_slice(format!("name=[{}] \"", n.len()).as_bytes());
                out.extend_from_slice(n);
                out.extend_from_slice(b"\"\n");
            }
            ParamKey::Pos(i) => {
                out.extend_from_slice(format!("Key: Position #{i}:\n").as_bytes());
                out.extend_from_slice(format!("paramno={i}\n").as_bytes());
                out.extend_from_slice(b"name=[0] \"\"\n");
            }
        }
        out.extend_from_slice(b"is_param=1\n");
        out.extend_from_slice(format!("param_type={}\n", p.ptype).as_bytes());
    }
    vm.echo(&out);
    Ok(Value::Null)
}

/// The statement's handle, for driver-specific code.
pub fn dbh_of(o: &ObjRef) -> Option<ObjRef> {
    with_st(o, |s| s.dbh.clone()).flatten()
}

#[allow(dead_code)]
fn _keep(_: Rc<()>, _: Key) {}
