//! mysqli_stmt (mysqli_api.c, mysqli_nonapi.c, mysqli.c) and
//! mysqli_warning (mysqli_warning.c).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::link::{
    flush, get_warnings, new_result_object, new_warning_object, report_index, report_mode,
    sync_globals, throw_sql_exception,
};
use crate::mysqlnd::conn::ConnRef;
use crate::mysqlnd::ps::{self, Convert, ParamBind, ResultBind, Stmt};
use crate::mysqlnd::*;
use crate::objects::*;
use crate::zpp::*;

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

/// MYSQLI_REPORT_STMT_ERROR
pub fn report_stmt_error(vm: &mut Vm, f: &Func, st: &Rc<RefCell<Stmt>>) {
    flush(vm, f);
    if report_mode() & MYSQLI_REPORT_ERROR == 0 {
        return;
    }
    let (no, s, e) = {
        let s = st.borrow();
        (
            s.error_info.error_no,
            s.sqlstate(),
            s.error_info.error.clone(),
        )
    };
    if no != 0 {
        throw_sql_exception(vm, f, &s, no as i64, &e);
    }
}

fn vbool(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

pub fn stmt_prop(vm: &mut Vm, o: &Obj, name: &[u8], quiet: bool) -> Result<Value, ()> {
    let check = match name {
        b"errno" | b"error" | b"sqlstate" | b"error_list" => MYSQLI_STATUS_INITIALIZED,
        _ => MYSQLI_STATUS_VALID,
    };
    let r = match get_res(o) {
        Some(r) => r,
        None => {
            if !quiet {
                if matches!(name, b"affected_rows" | b"id" | b"error_list") {
                    throw(vm, b"Error", "Property access is not allowed yet");
                } else {
                    let m = format!("{} object is already closed", ce_name(o));
                    throw(vm, b"Error", &m);
                }
            }
            return Err(());
        }
    };
    if r.status.get() < check {
        if !quiet {
            throw(vm, b"Error", "Property access is not allowed yet");
        }
        return Err(());
    }
    let ms = match &r.kind {
        Kind::Stmt(s) => s.clone(),
        _ => return Err(()),
    };
    let st = ms.borrow().stmt.clone();
    let st = match st {
        Some(s) => s,
        None => {
            if name == b"error_list" {
                return Ok(empty_array());
            }
            return Err(());
        }
    };
    let s = st.borrow();
    Ok(match name {
        b"affected_rows" => {
            let rc = s.upsert.affected_rows;
            if rc == u64::MAX {
                Value::Long(-1)
            } else {
                long_or_str(rc)
            }
        }
        b"insert_id" => long_or_str(s.upsert.last_insert_id),
        b"num_rows" => long_or_str(s.num_rows()),
        b"param_count" => Value::Long(s.param_count as i64),
        b"field_count" => Value::Long(s.field_count as i64),
        b"errno" => Value::Long(s.error_info.error_no as i64),
        b"error" => vstr(&s.error_info.error),
        b"error_list" => error_list(&s.error_info.error_list),
        b"sqlstate" => vstr(s.sqlstate()),
        b"id" => Value::Long(s.stmt_id as i64),
        _ => Value::Null,
    })
}

pub fn warning_prop(vm: &mut Vm, o: &Obj, name: &[u8], quiet: bool) -> Result<Value, ()> {
    let r = match get_res(o) {
        Some(r) => r,
        None => {
            if !quiet {
                let m = format!("{} object is already closed", ce_name(o));
                throw(vm, b"Error", &m);
            }
            return Err(());
        }
    };
    let (list, idx) = match &r.kind {
        Kind::Warning(l, i) => (l.clone(), i.get()),
        _ => return Err(()),
    };
    let w = match list.get(idx) {
        Some(w) => w.clone(),
        None => return Err(()),
    };
    Ok(match name {
        b"message" => w.reason.clone(),
        b"sqlstate" => w.sqlstate.clone(),
        b"errno" => Value::Long(w.errorno),
        _ => Value::Null,
    })
}

fn stmt_args(
    vm: &mut Vm,
    f: &Func,
    spec: &str,
    args: &[Value],
    check: u8,
) -> Result<(Vec<Arg>, Obj, Rc<RefCell<MyStmt>>, Rc<RefCell<Stmt>>), ()> {
    sync_globals(vm);
    let a = parse(vm, f, spec, &[b"mysqli_stmt"], args)?;
    let o = a[0].obj().unwrap();
    let (ms, st) = fetch_stmt(vm, &o, check)?;
    Ok((a, o, ms, st))
}

fn new_stmt_object(vm: &mut Vm, st: Stmt, query: Option<Vec<u8>>, status: u8) -> Value {
    let ms = MyStmt {
        stmt: Some(Rc::new(RefCell::new(st))),
        query,
    };
    let r = Resource {
        status: Cell::new(status),
        kind: Kind::Stmt(Rc::new(RefCell::new(ms))),
    };
    Value::Object(new_object(vm, b"mysqli_stmt", Base::Stmt, Some(r)))
}

/// close_stmt_and_copy_errors helper: moves the statement's errors (and
/// the affected rows) back into the connection after closing it.
fn close_stmt_and_copy_errors(st: &Rc<RefCell<Stmt>>, conn: &ConnRef) {
    let error_info = st.borrow().error_info.clone();
    let affected = conn.borrow().upsert.affected_rows;
    st.borrow_mut().dtor(false);
    let mut c = conn.borrow_mut();
    c.error_info = error_info;
    c.upsert.affected_rows = affected;
}

pub fn mysqli_prepare(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    sync_globals(vm);
    let a = match parse(vm, f, "Os", &[b"mysqli"], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let o = a[0].obj().unwrap();
    let (_, c) = match fetch_conn(vm, &o, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let q = a[1].str().unwrap_or(b"").to_vec();
    let mut st = Stmt::new(c.clone());
    let ok = st.prepare(&q);
    if !ok {
        // mysql_stmt_close() clears errors, so we have to store them temporarily
        let saved = c.borrow().error_info.clone();
        st.dtor(false);
        c.borrow_mut().error_info = saved;
        drop(st);
        crate::link::report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    let query = if !q.is_empty() && report_mode() & MYSQLI_REPORT_INDEX != 0 {
        Some(q)
    } else {
        None
    };
    new_stmt_object(vm, st, query, MYSQLI_STATUS_VALID)
}

pub fn mysqli_stmt_init(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    sync_globals(vm);
    let a = match parse(vm, f, "O", &[b"mysqli"], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let o = a[0].obj().unwrap();
    let (_, c) = match fetch_conn(vm, &o, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    new_stmt_object(vm, Stmt::new(c), None, MYSQLI_STATUS_INITIALIZED)
}

/// mysqli_stmt::__construct
pub fn mysqli_stmt_ctor(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    sync_globals(vm);
    let a = match parse(
        vm,
        &Func {
            is_method: false,
            ..f.clone()
        },
        "O|s!",
        &[b"mysqli"],
        args,
    ) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let link = a[0].obj().unwrap();
    let (_, c) = match fetch_conn(vm, &link, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let this = match &f.this {
        Some(t) => t.clone(),
        None => return Value::Null,
    };
    let st = Rc::new(RefCell::new(Stmt::new(c)));
    let ms = MyStmt {
        stmt: Some(st.clone()),
        query: None,
    };
    let r = Resource {
        status: Cell::new(MYSQLI_STATUS_INITIALIZED),
        kind: Kind::Stmt(Rc::new(RefCell::new(ms))),
    };
    set_res(&this, Some(r));
    if let Arg::Str(q) = &a[1] {
        if !st.borrow_mut().prepare(q) {
            report_stmt_error(vm, f, &st);
            return Value::False;
        }
        flush(vm, f);
        if let Some(r) = get_res(&this) {
            r.status.set(MYSQLI_STATUS_VALID);
        }
    }
    Value::Null
}

pub fn mysqli_stmt_prepare(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, o, _, st) = match stmt_args(vm, f, "Os", args, MYSQLI_STATUS_INITIALIZED) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let q = a[1].str().unwrap_or(b"").to_vec();
    if !st.borrow_mut().prepare(&q) {
        report_stmt_error(vm, f, &st);
        return Value::False;
    }
    flush(vm, f);
    if let Some(r) = get_res(&o) {
        r.status.set(MYSQLI_STATUS_VALID);
    }
    Value::True
}

pub fn mysqli_stmt_affected_rows(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let rc = st.borrow().upsert.affected_rows;
    if rc == u64::MAX {
        return Value::Long(-1);
    }
    long_or_str(rc)
}

pub fn mysqli_stmt_attr_get(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, _, st) = match stmt_args(vm, f, "Ol", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let attr = a[1].long_or(0);
    match st.borrow().attr_get(attr) {
        Some(v) => Value::Long(v as i64),
        None => {
            f.value_error(
                vm,
                f.arg_pos(2),
                "must be either MYSQLI_STMT_ATTR_UPDATE_MAX_LENGTH or MYSQLI_STMT_ATTR_CURSOR_TYPE",
            );
            Value::Null
        }
    }
}

pub fn mysqli_stmt_attr_set(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, _, st) = match stmt_args(vm, f, "Oll", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let attr = a[1].long_or(0);
    let mode_in = a[2].long_or(0);
    let mode: u64 = match attr {
        ps::STMT_ATTR_UPDATE_MAX_LENGTH => {
            if mode_in != 0 && mode_in != 1 {
                f.value_error(
                    vm,
                    f.arg_pos(3),
                    "must be 0 or 1 for attribute MYSQLI_STMT_ATTR_UPDATE_MAX_LENGTH",
                );
                return Value::Null;
            }
            mode_in as u64
        }
        ps::STMT_ATTR_CURSOR_TYPE => match mode_in {
            0 | 1 => mode_in as u64,
            _ => {
                f.value_error(
                    vm,
                    f.arg_pos(3),
                    "must be one of the MYSQLI_CURSOR_TYPE_* constants for attribute MYSQLI_STMT_ATTR_CURSOR_TYPE",
                );
                return Value::Null;
            }
        },
        _ => {
            f.value_error(
                vm,
                f.arg_pos(2),
                "must be either MYSQLI_STMT_ATTR_UPDATE_MAX_LENGTH or MYSQLI_STMT_ATTR_CURSOR_TYPE",
            );
            return Value::Null;
        }
    };
    if !st.borrow_mut().attr_set(attr, mode) {
        report_stmt_error(vm, f, &st);
        return Value::False;
    }
    Value::True
}

/// mysqli_stmt_bind_param_do_bind
fn bind_param_do_bind(
    vm: &mut Vm,
    f: &Func,
    st: &Rc<RefCell<Stmt>>,
    vars: &[Value],
    types: &[u8],
    arg_num: usize,
) -> Result<bool, ()> {
    if vars.is_empty() {
        return Ok(true);
    }
    let mut params = Vec::with_capacity(vars.len());
    for (i, v) in vars.iter().enumerate() {
        let t = match types[i] {
            b'd' => MYSQL_TYPE_DOUBLE,
            b'i' => MYSQL_TYPE_LONGLONG,
            b'b' => MYSQL_TYPE_LONG_BLOB,
            b's' => MYSQL_TYPE_VAR_STRING,
            _ => {
                f.value_error(
                    vm,
                    arg_num,
                    "must only contain the \"b\", \"d\", \"i\", \"s\" type specifiers",
                );
                return Err(());
            }
        };
        params.push(ParamBind {
            zv: v.clone(),
            ptype: t,
            flags: 0,
        });
    }
    Ok(st.borrow_mut().bind_parameters(params))
}

pub fn mysqli_stmt_bind_param(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, _, st) = match stmt_args(vm, f, "Os*", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let types = a[1].str().unwrap_or(b"").to_vec();
    let vars = match &a[2] {
        Arg::Rest(v) => v.clone(),
        _ => Vec::new(),
    };
    if types.is_empty() {
        f.must_not_be_empty(vm, f.arg_pos(2));
        return Value::Null;
    }
    if types.len() != vars.len() {
        f.count_error(
            vm,
            "The number of elements in the type definition string must match the number of bind variables",
        );
        return Value::Null;
    }
    if types.len() != st.borrow().param_count as usize {
        f.count_error(
            vm,
            "The number of variables must match the number of parameters in the prepared statement",
        );
        return Value::Null;
    }
    let r = match bind_param_do_bind(vm, f, &st, &vars, &types, f.arg_pos(2)) {
        Ok(r) => r,
        Err(()) => return Value::Null,
    };
    report_stmt_error(vm, f, &st);
    vbool(r)
}

pub fn mysqli_stmt_bind_result(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, _, st) = match stmt_args(vm, f, "O+", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let vars = match &a[1] {
        Arg::Rest(v) => v.clone(),
        _ => Vec::new(),
    };
    if vars.len() != st.borrow().field_count as usize {
        f.count_error(
            vm,
            "Number of bind variables doesn't match number of fields in prepared statement",
        );
        return Value::Null;
    }
    let binds: Vec<ResultBind> = vars
        .into_iter()
        .map(|zv| ResultBind { zv, bound: false })
        .collect();
    vbool(st.borrow_mut().bind_result(binds))
}

pub fn mysqli_stmt_close(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, o, ms, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    st.borrow_mut().dtor(false);
    ms.borrow_mut().stmt = None;
    clear_resource(&o);
    flush(vm, f);
    Value::True
}

pub fn mysqli_stmt_data_seek(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    sync_globals(vm);
    let a = match parse(vm, f, "Ol", &[b"mysqli_stmt"], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let offset = a[1].long_or(0);
    if offset < 0 {
        f.value_error(vm, f.arg_pos(2), "must be greater than or equal to 0");
        return Value::Null;
    }
    let o = a[0].obj().unwrap();
    let (_, st) = match fetch_stmt(vm, &o, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    st.borrow_mut().data_seek(offset as u64);
    Value::Null
}

pub fn mysqli_stmt_errno(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_INITIALIZED) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    Value::Long(st.borrow().error_info.error_no as i64)
}

pub fn mysqli_stmt_error(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_INITIALIZED) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    vstr(st.borrow().error_info.error.clone())
}

pub fn mysqli_stmt_error_list(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_INITIALIZED) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    error_list(&st.borrow().error_info.error_list)
}

fn bind_input_params(
    vm: &mut Vm,
    f: &Func,
    st: &Rc<RefCell<Stmt>>,
    input: &Rc<RefCell<goro_core::array::PhpArray>>,
    argpos: usize,
) -> Result<bool, ()> {
    let arr = input.borrow();
    let is_list = arr
        .iter()
        .enumerate()
        .all(|(i, (k, _))| matches!(k, goro_core::array::ArrayKey::Int(n) if *n == i as i64));
    if !is_list {
        drop(arr);
        f.value_error(vm, argpos, "must be a list array");
        return Err(());
    }
    let n = arr.len();
    let pc = st.borrow().param_count as usize;
    if n != pc {
        drop(arr);
        f.value_error(
            vm,
            argpos,
            &format!("must consist of exactly {} elements, {} present", pc, n),
        );
        return Err(());
    }
    let params: Vec<ParamBind> = arr
        .iter()
        .map(|(_, v)| ParamBind {
            zv: v.clone(),
            ptype: MYSQL_TYPE_VAR_STRING,
            flags: 0,
        })
        .collect();
    drop(arr);
    Ok(st.borrow_mut().bind_parameters(params))
}

pub fn mysqli_stmt_execute(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, ms, st) = match stmt_args(vm, f, "O|h!", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let mut ret = Value::True;
    if let Arg::Arr(input) = &a[1] {
        match bind_input_params(vm, f, &st, input, f.arg_pos(2)) {
            Ok(true) => {}
            Ok(false) => {
                report_stmt_error(vm, f, &st);
                ret = Value::False;
            }
            Err(()) => return Value::Null,
        }
    }
    let ok = {
        let mut cv = VmConvert { vm };
        st.borrow_mut().execute(&mut cv)
    };
    if vm.current_exception.is_some() {
        return Value::Null;
    }
    if !ok {
        report_stmt_error(vm, f, &st);
        ret = Value::False;
    } else {
        flush(vm, f);
        if !matches!(ret, Value::False) {
            ret = Value::True;
        }
    }
    if report_mode() & MYSQLI_REPORT_INDEX != 0 {
        let q = ms.borrow().query.clone().unwrap_or_default();
        let s = st.borrow().upsert.server_status;
        report_index(vm, f, &q, s);
    }
    ret
}

pub fn mysqli_execute_query(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    sync_globals(vm);
    let a = match parse(vm, f, "Os|h!", &[b"mysqli"], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let o = a[0].obj().unwrap();
    let (_, c) = match fetch_conn(vm, &o, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let q = a[1].str().unwrap_or(b"").to_vec();
    let st = Rc::new(RefCell::new(Stmt::new(c.clone())));
    let ok = st.borrow_mut().prepare(&q);
    if !ok {
        report_stmt_error(vm, f, &st);
        close_stmt_and_copy_errors(&st, &c);
        return Value::False;
    }
    let query = if !q.is_empty() && report_mode() & MYSQLI_REPORT_INDEX != 0 {
        Some(q.clone())
    } else {
        None
    };
    if let Arg::Arr(input) = &a[2] {
        let arr = input.borrow();
        let is_list = arr
            .iter()
            .enumerate()
            .all(|(i, (k, _))| matches!(k, goro_core::array::ArrayKey::Int(n) if *n == i as i64));
        let n = arr.len();
        drop(arr);
        if !is_list {
            st.borrow_mut().dtor(false);
            f.value_error(vm, f.arg_pos(3), "must be a list array");
            return Value::Null;
        }
        let pc = st.borrow().param_count as usize;
        if n != pc {
            st.borrow_mut().dtor(false);
            f.value_error(
                vm,
                f.arg_pos(3),
                &format!("must consist of exactly {} elements, {} present", pc, n),
            );
            return Value::Null;
        }
        let params: Vec<ParamBind> = input
            .borrow()
            .iter()
            .map(|(_, v)| ParamBind {
                zv: v.clone(),
                ptype: MYSQL_TYPE_VAR_STRING,
                flags: 0,
            })
            .collect();
        if !st.borrow_mut().bind_parameters(params) {
            close_stmt_and_copy_errors(&st, &c);
            return Value::False;
        }
    }
    let ok = {
        let mut cv = VmConvert { vm };
        st.borrow_mut().execute(&mut cv)
    };
    if vm.current_exception.is_some() {
        st.borrow_mut().dtor(false);
        return Value::Null;
    }
    if !ok {
        report_stmt_error(vm, f, &st);
        if report_mode() & MYSQLI_REPORT_INDEX != 0 {
            let s = st.borrow().upsert.server_status;
            report_index(vm, f, query.as_deref().unwrap_or(b""), s);
        }
        close_stmt_and_copy_errors(&st, &c);
        return Value::False;
    }
    flush(vm, f);
    if st.borrow().field_count == 0 {
        close_stmt_and_copy_errors(&st, &c);
        return Value::True;
    }
    if report_mode() & MYSQLI_REPORT_INDEX != 0 {
        let s = st.borrow().upsert.server_status;
        report_index(vm, f, query.as_deref().unwrap_or(b""), s);
    }
    let r = st.borrow_mut().get_result();
    let r = match r {
        Some(r) => r,
        None => {
            report_stmt_error(vm, f, &st);
            close_stmt_and_copy_errors(&st, &c);
            return Value::False;
        }
    };
    flush(vm, f);
    let v = new_result_object(vm, r);
    close_stmt_and_copy_errors(&st, &c);
    v
}

/// ZEND_TRY_ASSIGN_VALUE_EX to a bound variable
fn assign_bound(target: &Value, v: Value) {
    if let Value::Reference(r) = target {
        *r.borrow_mut() = v;
    }
}

pub fn mysqli_stmt_fetch(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let r = st.borrow_mut().fetch();
    flush(vm, f);
    match r {
        Err(()) => {
            report_stmt_error(vm, f, &st);
            Value::False
        }
        Ok(None) => Value::Null,
        Ok(Some(row)) => {
            let binds = st.borrow().result_bind.clone();
            if let Some(binds) = binds {
                for (i, b) in binds.iter().enumerate() {
                    if b.bound {
                        assign_bound(&b.zv, row.get(i).cloned().unwrap_or(Value::Null));
                    }
                }
            }
            Value::True
        }
    }
}

pub fn mysqli_stmt_field_count(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    Value::Long(st.borrow().field_count as i64)
}

pub fn mysqli_stmt_free_result(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    st.borrow_mut().free_result();
    flush(vm, f);
    Value::Null
}

pub fn mysqli_stmt_get_result(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let r = st.borrow_mut().get_result();
    match r {
        Some(r) => {
            flush(vm, f);
            new_result_object(vm, r)
        }
        None => {
            report_stmt_error(vm, f, &st);
            Value::False
        }
    }
}

pub fn mysqli_stmt_get_warnings(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let (wc, conn) = {
        let s = st.borrow();
        (s.upsert.warning_count, s.conn.clone())
    };
    if wc == 0 {
        return Value::False;
    }
    let conn = match conn {
        Some(c) => c,
        None => return Value::False,
    };
    let w = get_warnings(&conn);
    flush(vm, f);
    if w.is_empty() {
        return Value::False;
    }
    new_warning_object(vm, w)
}

pub fn mysqli_stmt_insert_id(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    long_or_str(st.borrow().upsert.last_insert_id)
}

pub fn mysqli_stmt_more_results(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    vbool(st.borrow().more_results())
}

pub fn mysqli_stmt_next_result(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let ok = st.borrow_mut().next_result();
    if !ok {
        report_stmt_error(vm, f, &st);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

pub fn mysqli_stmt_num_rows(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    long_or_str(st.borrow().num_rows())
}

pub fn mysqli_stmt_param_count(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    Value::Long(st.borrow().param_count as i64)
}

pub fn mysqli_stmt_reset(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let ok = st.borrow_mut().reset();
    flush(vm, f);
    vbool(ok)
}

pub fn mysqli_stmt_result_metadata(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let r = st.borrow_mut().result_metadata();
    match r {
        Some(r) => new_result_object(vm, r),
        None => {
            report_stmt_error(vm, f, &st);
            Value::False
        }
    }
}

pub fn mysqli_stmt_send_long_data(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, _, st) = match stmt_args(vm, f, "Ols", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let n = a[1].long_or(0);
    if n < 0 {
        f.value_error(vm, f.arg_pos(2), "must be greater than or equal to 0");
        return Value::Null;
    }
    let data = a[2].str().unwrap_or(b"").to_vec();
    let ok = st.borrow_mut().send_long_data(n as u32, &data);
    flush(vm, f);
    vbool(ok)
}

pub fn mysqli_stmt_store_result(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    // mysqlnd_stmt_store_result
    let fc = st.borrow().field_count;
    let ok = fc == 0 || st.borrow_mut().store_result();
    if !ok {
        report_stmt_error(vm, f, &st);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

pub fn mysqli_stmt_sqlstate(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, _, st) = match stmt_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    vstr(st.borrow().sqlstate())
}

// ── mysqli_warning ────────────────────────────────────────────────────

pub fn mysqli_warning_ctor(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    throw(vm, b"Error", "Cannot directly construct mysqli_warning");
    Value::Null
}

pub fn mysqli_warning_next(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let this = match &f.this {
        Some(t) => t.clone(),
        None => return Value::False,
    };
    if get_res(&this).is_some() {
        let r = match fetch_resource(vm, &this, MYSQLI_STATUS_VALID) {
            Ok(r) => r,
            Err(()) => return Value::Null,
        };
        if let Kind::Warning(list, idx) = &r.kind {
            if idx.get() + 1 < list.len() {
                idx.set(idx.get() + 1);
                return Value::True;
            }
        }
    }
    Value::False
}
