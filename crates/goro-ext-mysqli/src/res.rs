//! mysqli_result: fetching (mysqli.c's php_mysqli_fetch_into_hash, the
//! fetch functions of mysqli_api.c / mysqli_nonapi.c) and the result
//! iterator (mysqli_result_iterator.c).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::link::{flush, new_result_object, report_index, report_mode, sync_globals};
use crate::mysqlnd::result::{Field, ResultSet};

use crate::objects::*;
use crate::zpp::*;

const MYSQLI_ASSOC: i64 = 1;
const MYSQLI_NUM: i64 = 2;
const MYSQLI_BOTH: i64 = 3;

pub fn result_prop(vm: &mut Vm, o: &Obj, name: &[u8], quiet: bool) -> Result<Value, ()> {
    let r = match get_res(o) {
        Some(r) => r,
        None => {
            if !quiet {
                if name == b"type" || name == b"lengths" {
                    throw(vm, b"Error", "Property access is not allowed yet");
                } else {
                    let m = format!("{} object is already closed", ce_name(o));
                    throw(vm, b"Error", &m);
                }
            }
            return Err(());
        }
    };
    if r.status.get() < MYSQLI_STATUS_VALID {
        if !quiet {
            throw(vm, b"Error", "Property access is not allowed yet");
        }
        return Err(());
    }
    let res = match &r.kind {
        Kind::Result(res) => res.clone(),
        _ => return Err(()),
    };
    let res = res.borrow();
    Ok(match name {
        b"current_field" => Value::Long(res.field_tell() as i64),
        b"field_count" => Value::Long(res.field_count as i64),
        b"lengths" => {
            if res.field_count == 0 {
                Value::Null
            } else {
                match res.fetch_lengths() {
                    Some(l) => {
                        let mut arr = PhpArray::new();
                        for (i, v) in l.iter().enumerate() {
                            arr.set(ArrayKey::Int(i as i64), Value::Long(*v as i64));
                        }
                        Value::Array(Rc::new(RefCell::new(arr)))
                    }
                    None => Value::Null,
                }
            }
        }
        b"num_rows" => long_or_str(res.num_rows()),
        b"type" => Value::Long(if res.is_unbuffered() { 1 } else { 0 }),
        _ => Value::Null,
    })
}

fn result_args(
    vm: &mut Vm,
    f: &Func,
    spec: &str,
    args: &[Value],
) -> Result<(Vec<Arg>, Obj, Rc<RefCell<ResultSet>>), ()> {
    sync_globals(vm);
    let a = parse(vm, f, spec, &[b"mysqli_result"], args)?;
    let o = a[0].obj().unwrap();
    let r = fetch_result(vm, &o, MYSQLI_STATUS_VALID)?;
    Ok((a, o, r))
}

/// The row as an array (mysqlnd_res::fetch_into): Ok(Some/None) or Err
/// (FALSE).
pub fn fetch_into(res: &Rc<RefCell<ResultSet>>, flags: i64) -> Result<Option<Value>, ()> {
    let mut r = res.borrow_mut();
    if let Some(c) = &r.conn {
        c.borrow_mut().error_info.reset();
    }
    let row = r.fetch_row(true)?;
    let row = match row {
        Some(row) => row,
        None => return Ok(None),
    };
    let fields: Vec<Field> = r.fields().to_vec();
    drop(r);
    Ok(Some(row_to_array(&row, &fields, flags)))
}

pub fn row_to_array(row: &[Value], fields: &[Field], flags: i64) -> Value {
    let mut arr = PhpArray::new();
    for (i, f) in fields.iter().enumerate() {
        let v = row.get(i).cloned().unwrap_or(Value::Null);
        if flags & MYSQLI_NUM != 0 {
            let k = ArrayKey::Int(i as i64);
            if !arr.contains_key(&k) {
                arr.set(k, v.clone());
            }
        }
        if flags & MYSQLI_ASSOC != 0 {
            let k = if f.is_numeric {
                ArrayKey::Int(f.num_key)
            } else {
                ArrayKey::String(PhpString::from_bytes(&f.name))
            };
            arr.set(k, v);
        }
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}

/// php_mysqli_fetch_into_hash_aux
pub fn fetch_into_hash_aux(
    vm: &mut Vm,
    f: &Func,
    res: &Rc<RefCell<ResultSet>>,
    fetchtype: i64,
) -> Value {
    let r = fetch_into(res, fetchtype);
    flush(vm, f);
    let v = match r {
        Ok(Some(v)) => v,
        Ok(None) => Value::Null,
        Err(()) => Value::False,
    };
    if report_mode() & crate::objects::MYSQLI_REPORT_ERROR != 0 {
        let conn = res.borrow().conn.clone();
        if let Some(c) = conn {
            let (no, st, er) = {
                let c = c.borrow();
                (
                    c.error_info.error_no,
                    c.sqlstate(),
                    c.error_info.error.clone(),
                )
            };
            if no != 0 {
                crate::link::throw_sql_exception(vm, f, &st, no as i64, &er);
            }
        }
    }
    v
}

/// php_mysqli_fetch_into_hash
fn fetch_into_hash(
    vm: &mut Vm,
    f: &Func,
    args: &[Value],
    override_flags: i64,
    into_object: bool,
) -> Value {
    sync_globals(vm);
    let mut ce: Vec<u8> = b"stdclass".to_vec();
    let mut ctor_params: Option<Rc<RefCell<PhpArray>>> = None;
    let a;
    let fetchtype;
    if into_object {
        a = match parse(vm, f, "O|Ch", &[b"mysqli_result"], args) {
            Ok(a) => a,
            Err(()) => return Value::Null,
        };
        if let Arg::Class(c) = &a[1] {
            let lower = c.to_ascii_lowercase();
            let lower = lower
                .strip_prefix(b"\\")
                .map(|x| x.to_vec())
                .unwrap_or(lower);
            // zend_parse_arg_class
            if !vm.classes.contains_key(&lower) && lower != b"stdclass" {
                let _ = vm.try_autoload_class(c);
            }
            if !vm.classes.contains_key(&lower) && lower != b"stdclass" {
                let m = format!(
                    "{} must be a valid class name, {} given",
                    f.arg_prefix(f.arg_pos(2)),
                    String::from_utf8_lossy(c)
                );
                throw(vm, b"TypeError", &m);
                return Value::Null;
            }
            ce = lower;
        }
        if let Arg::Arr(p) = &a[2] {
            ctor_params = Some(p.clone());
        }
        if let Some(c) = vm.classes.get(&ce) {
            if c.is_interface || c.is_trait || c.is_abstract || c.is_enum {
                let m = format!(
                    "Class {} cannot be instantiated",
                    String::from_utf8_lossy(&c.name)
                );
                throw(vm, b"Error", &m);
                return Value::Null;
            }
        }
        fetchtype = MYSQLI_ASSOC;
    } else if override_flags != 0 {
        a = match parse(vm, f, "O", &[b"mysqli_result"], args) {
            Ok(a) => a,
            Err(()) => return Value::Null,
        };
        fetchtype = override_flags;
    } else {
        a = match parse(vm, f, "O|l", &[b"mysqli_result"], args) {
            Ok(a) => a,
            Err(()) => return Value::Null,
        };
        fetchtype = a[1].long_or(MYSQLI_BOTH);
    }
    let o = a[0].obj().unwrap();
    let res = match fetch_result(vm, &o, MYSQLI_STATUS_VALID) {
        Ok(r) => r,
        Err(()) => return Value::Null,
    };
    if !(MYSQLI_ASSOC..=MYSQLI_BOTH).contains(&fetchtype) {
        f.value_error(
            vm,
            f.arg_pos(2),
            "must be one of MYSQLI_NUM, MYSQLI_ASSOC, or MYSQLI_BOTH",
        );
        return Value::Null;
    }
    let row = fetch_into_hash_aux(vm, f, &res, fetchtype);
    if into_object {
        if let Value::Array(arr) = &row {
            return make_object(vm, f, &ce, arr, ctor_params);
        }
    }
    row
}

/// object_init_ex + zend_merge_properties + constructor call
fn make_object(
    vm: &mut Vm,
    f: &Func,
    ce: &[u8],
    arr: &Rc<RefCell<PhpArray>>,
    ctor_params: Option<Rc<RefCell<PhpArray>>>,
) -> Value {
    let disp = if ce == b"stdclass" {
        b"stdClass".to_vec()
    } else {
        vm.classes
            .get(ce)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| ce.to_vec())
    };
    let id = vm.alloc_object_id();
    let mut obj = PhpObject::new(disp, id);
    vm.apply_class_property_defaults(&mut obj, ce);
    let o = Rc::new(RefCell::new(obj));
    vm.run_object_create_hook(&o);
    let ov = Value::Object(o.clone());
    let has_set = vm
        .classes
        .get(ce)
        .map(|c| c.methods.contains_key(b"__set".as_slice()))
        .unwrap_or(false);
    let entries: Vec<(ArrayKey, Value)> = arr
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, v) in entries {
        let name: Vec<u8> = match &k {
            ArrayKey::Int(n) => n.to_string().into_bytes(),
            ArrayKey::String(s) => s.as_bytes().to_vec(),
        };
        let declared = vm
            .classes
            .get(ce)
            .map(|c| c.properties.iter().any(|p| p.name == name && !p.is_static))
            .unwrap_or(false);
        if has_set && !declared {
            let _ = vm.call_object_method(
                &ov,
                b"__set",
                &[Value::String(PhpString::from_vec(name)), v],
            );
            if vm.current_exception.is_some() {
                return Value::Null;
            }
        } else {
            o.borrow_mut().set_property(name, v);
        }
    }
    let ctor = vm
        .classes
        .get(ce)
        .and_then(|c| c.get_method(b"__construct"))
        .map(|m| {
            let op = &m.op_array;
            let implicit = u32::from(op.cv_names.first().map(|n| n.as_slice()) == Some(b"this"));
            let variadic = op.variadic_param.is_some()
                || op.param_count.saturating_sub(implicit) > op.required_param_count;
            (
                op.required_param_count as usize,
                variadic,
                m.declaring_class.clone(),
            )
        });
    if let Some((required, variadic, declaring)) = ctor {
        let params: Vec<Value> = match &ctor_params {
            Some(p) => p.borrow().iter().map(|(_, v)| v.clone()).collect(),
            None => Vec::new(),
        };
        // zend_call_function() checks the count like a userland call
        if params.len() < required {
            let cname = vm
                .classes
                .get(&declaring)
                .map(|c| String::from_utf8_lossy(&c.name).into_owned())
                .unwrap_or_else(|| String::from_utf8_lossy(&declaring).into_owned());
            let msg = format!(
                "Too few arguments to function {}::__construct(), {} passed and {} {} expected",
                cname,
                params.len(),
                if variadic { "at least" } else { "exactly" },
                required
            );
            let line = vm.current_line;
            let exc = vm.create_exception(b"ArgumentCountError", &msg, line);
            vm.current_exception = Some(exc);
            return Value::Null;
        }
        let _ = vm.call_object_method(&ov, b"__construct", &params);
        if vm.current_exception.is_some() {
            return Value::Null;
        }
    } else if let Some(p) = &ctor_params {
        if !p.borrow().is_empty() {
            let cname = String::from_utf8_lossy(&o.borrow().class_name).into_owned();
            f.value_error(
                vm,
                f.arg_pos(3),
                &format!(
                    "must be empty when the specified class ({}) does not have a constructor",
                    cname
                ),
            );
            return Value::Null;
        }
    }
    ov
}

pub fn mysqli_fetch_array(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    fetch_into_hash(vm, f, args, 0, false)
}

pub fn mysqli_fetch_assoc(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    fetch_into_hash(vm, f, args, MYSQLI_ASSOC, false)
}

pub fn mysqli_fetch_row(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    fetch_into_hash(vm, f, args, MYSQLI_NUM, false)
}

pub fn mysqli_fetch_object(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    fetch_into_hash(vm, f, args, MYSQLI_ASSOC, true)
}

pub fn mysqli_fetch_column(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, res) = match result_args(vm, f, "O|l", args) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let col = a[1].long_or(0);
    if col < 0 {
        f.value_error(vm, f.arg_pos(2), "must be greater than or equal to 0");
        return Value::Null;
    }
    if col >= res.borrow().field_count as i64 {
        f.value_error(
            vm,
            f.arg_pos(2),
            "must be less than the number of fields for this result set",
        );
        return Value::Null;
    }
    let row = fetch_into_hash_aux(vm, f, &res, MYSQLI_NUM);
    match row {
        Value::Array(a) => a.borrow().get_int(col).cloned().unwrap_or(Value::Null),
        _ => Value::False,
    }
}

pub fn mysqli_fetch_all(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, res) = match result_args(vm, f, "O|l", args) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let mode = a[1].long_or(MYSQLI_NUM);
    if mode == 0 || mode & !MYSQLI_BOTH != 0 {
        f.value_error(
            vm,
            f.arg_pos(2),
            "must be one of MYSQLI_NUM, MYSQLI_ASSOC, or MYSQLI_BOTH",
        );
        return Value::Null;
    }
    let mut out = PhpArray::new();
    let mut i = 0i64;
    loop {
        let row = fetch_into_hash_aux(vm, f, &res, mode);
        if vm.current_exception.is_some() {
            return Value::Null;
        }
        if !matches!(row, Value::Array(_)) {
            break;
        }
        out.set(ArrayKey::Int(i), row);
        i += 1;
    }
    Value::Array(Rc::new(RefCell::new(out)))
}

/// php_add_field_properties
fn field_object(vm: &mut Vm, field: &Field) -> Value {
    let id = vm.alloc_object_id();
    let mut o = PhpObject::new(b"stdClass".to_vec(), id);
    o.set_property(b"name".to_vec(), vstr(&field.name));
    o.set_property(b"orgname".to_vec(), vstr(&field.org_name));
    o.set_property(b"table".to_vec(), vstr(&field.table));
    o.set_property(b"orgtable".to_vec(), vstr(&field.org_table));
    o.set_property(b"def".to_vec(), vstr(&field.def));
    o.set_property(b"db".to_vec(), vstr(&field.db));
    o.set_property(b"catalog".to_vec(), vstr("def"));
    o.set_property(b"max_length".to_vec(), Value::Long(0));
    o.set_property(b"length".to_vec(), Value::Long(field.length as i64));
    o.set_property(b"charsetnr".to_vec(), Value::Long(field.charsetnr as i64));
    o.set_property(b"flags".to_vec(), Value::Long(field.flags as i64));
    o.set_property(b"type".to_vec(), Value::Long(field.ftype as i64));
    o.set_property(b"decimals".to_vec(), Value::Long(field.decimals as i64));
    Value::Object(Rc::new(RefCell::new(o)))
}

pub fn mysqli_fetch_field(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, res) = match result_args(vm, f, "O", args) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let field = {
        let mut r = res.borrow_mut();
        match r.meta.as_mut() {
            Some(m) => m.fetch_field().cloned(),
            None => None,
        }
    };
    match field {
        Some(fl) => field_object(vm, &fl),
        None => Value::False,
    }
}

pub fn mysqli_fetch_fields(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, res) = match result_args(vm, f, "O", args) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let fields: Vec<Field> = res.borrow().fields().to_vec();
    let mut arr = PhpArray::new();
    for (i, fl) in fields.iter().enumerate() {
        let v = field_object(vm, fl);
        arr.set(ArrayKey::Int(i as i64), v);
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}

pub fn mysqli_fetch_field_direct(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    sync_globals(vm);
    let a = match parse(vm, f, "Ol", &[b"mysqli_result"], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let offset = a[1].long_or(0);
    if offset < 0 {
        f.value_error(vm, f.arg_pos(2), "must be greater than or equal to 0");
        return Value::Null;
    }
    let o = a[0].obj().unwrap();
    let res = match fetch_result(vm, &o, MYSQLI_STATUS_VALID) {
        Ok(r) => r,
        Err(()) => return Value::Null,
    };
    if offset >= res.borrow().field_count as i64 {
        f.value_error(
            vm,
            f.arg_pos(2),
            "must be less than the number of fields for this result set",
        );
        return Value::Null;
    }
    let field = res.borrow().fields().get(offset as usize).cloned();
    match field {
        Some(fl) => field_object(vm, &fl),
        None => Value::False,
    }
}

pub fn mysqli_fetch_lengths(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, res) = match result_args(vm, f, "O", args) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let l = res.borrow().fetch_lengths();
    match l {
        Some(l) => {
            let mut arr = PhpArray::new();
            for (i, v) in l.iter().enumerate() {
                arr.set(ArrayKey::Int(i as i64), Value::Long(*v as i64));
            }
            Value::Array(Rc::new(RefCell::new(arr)))
        }
        None => Value::False,
    }
}

pub fn mysqli_field_seek(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    sync_globals(vm);
    let a = match parse(vm, f, "Ol", &[b"mysqli_result"], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let n = a[1].long_or(0);
    if n < 0 {
        f.value_error(vm, f.arg_pos(2), "must be greater than or equal to 0");
        return Value::Null;
    }
    let o = a[0].obj().unwrap();
    let res = match fetch_result(vm, &o, MYSQLI_STATUS_VALID) {
        Ok(r) => r,
        Err(()) => return Value::Null,
    };
    if n as u32 as i64 != n || n as u32 >= res.borrow().field_count {
        f.value_error(
            vm,
            f.arg_pos(2),
            "must be less than the number of fields for this result set",
        );
        return Value::Null;
    }
    res.borrow_mut().field_seek(n as u32);
    Value::True
}

pub fn mysqli_field_tell(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, res) = match result_args(vm, f, "O", args) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    Value::Long(res.borrow().field_tell() as i64)
}

pub fn mysqli_free_result(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, o, res) = match result_args(vm, f, "O", args) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    res.borrow_mut().free_result(false);
    clear_resource(&o);
    flush(vm, f);
    Value::Null
}

pub fn mysqli_num_fields(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, res) = match result_args(vm, f, "O", args) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    Value::Long(res.borrow().field_count as i64)
}

pub fn mysqli_num_rows(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, res) = match result_args(vm, f, "O", args) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    if res.borrow().unbuffered_and_not_eof() {
        throw(
            vm,
            b"Error",
            "mysqli_num_rows() cannot be used in MYSQLI_USE_RESULT mode",
        );
        return Value::Null;
    }
    long_or_str(res.borrow().num_rows())
}

pub fn mysqli_data_seek(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    sync_globals(vm);
    let a = match parse(vm, f, "Ol", &[b"mysqli_result"], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let offset = a[1].long_or(0);
    if offset < 0 {
        f.value_error(vm, f.arg_pos(2), "must be greater than or equal to 0");
        return Value::Null;
    }
    let o = a[0].obj().unwrap();
    let res = match fetch_result(vm, &o, MYSQLI_STATUS_VALID) {
        Ok(r) => r,
        Err(()) => return Value::Null,
    };
    if res.borrow().is_unbuffered() {
        let m = if f.is_method {
            "mysqli_result::data_seek() cannot be used in MYSQLI_USE_RESULT mode"
        } else {
            "mysqli_data_seek() cannot be used in MYSQLI_USE_RESULT mode"
        };
        throw(vm, b"Error", m);
        return Value::Null;
    }
    if offset as u64 >= res.borrow().num_rows() {
        return Value::False;
    }
    res.borrow_mut().data_seek(offset as u64);
    Value::True
}

/// mysqli_result::__construct
pub fn mysqli_result_ctor(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    sync_globals(vm);
    let a = match parse(
        vm,
        &Func {
            is_method: false,
            ..f.clone()
        },
        "O|l",
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
    let resmode = a[1].long_or(0);
    let result = match resmode {
        0 => crate::mysqlnd::conn::store_result(&c),
        1 => crate::mysqlnd::conn::use_result(&c),
        _ => {
            f.value_error(
                vm,
                2,
                "must be either MYSQLI_STORE_RESULT or MYSQLI_USE_RESULT",
            );
            return Value::Null;
        }
    };
    let result = match result {
        Some(r) => r,
        None => {
            crate::link::report_conn_error(vm, f, &c);
            return Value::False;
        }
    };
    flush(vm, f);
    if report_mode() & MYSQLI_REPORT_INDEX != 0 {
        let st = c.borrow().upsert.server_status;
        report_index(vm, f, b"from previous query", st);
    }
    if let Some(this) = &f.this {
        set_res(
            this,
            Some(Resource {
                status: std::cell::Cell::new(MYSQLI_STATUS_VALID),
                kind: Kind::Result(Rc::new(RefCell::new(*result))),
            }),
        );
        Value::Null
    } else {
        new_result_object(vm, result)
    }
}

// ── the result iterator ───────────────────────────────────────────────

/// php_mysqli_result_iterator
pub struct ResultIter {
    pub result: Obj,
    pub current_row: RefCell<Value>,
    pub row_num: std::cell::Cell<i64>,
}

static ITER_HANDLERS: goro_core::vm::NativeObjectHandlers =
    goro_core::vm::NativeObjectHandlers::EMPTY;

pub fn iter_of(o: &Obj) -> Option<Rc<ResultIter>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<ResultIter>().ok()
}

/// mysqli_result::getIterator
pub fn mysqli_result_get_iterator(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let this = match &f.this {
        Some(t) => t.clone(),
        None => return Value::Null,
    };
    let id = vm.alloc_object_id();
    let o = Rc::new(RefCell::new(PhpObject::new(
        b"InternalIterator".to_vec(),
        id,
    )));
    o.borrow_mut().native = Some(goro_core::object::NativeData {
        data: Rc::new(ResultIter {
            result: this,
            current_row: RefCell::new(Value::Undef),
            row_num: std::cell::Cell::new(-1),
        }) as Rc<dyn std::any::Any>,
        handlers: &ITER_HANDLERS,
    });
    Value::Object(o)
}

fn iter_move_forward(vm: &mut Vm, it: &ResultIter) {
    let f = Func::method("mysqli_result", "fetch_assoc", Some(it.result.clone()));
    let res = match get_res(&it.result) {
        Some(r) => r,
        None => {
            let m = format!("{} object is already closed", ce_name(&it.result));
            throw(vm, b"Error", &m);
            return;
        }
    };
    if res.status.get() < MYSQLI_STATUS_VALID {
        let m = format!("{} object is not fully initialized", ce_name(&it.result));
        throw(vm, b"Error", &m);
        return;
    }
    let r = match &res.kind {
        Kind::Result(r) => r.clone(),
        _ => return,
    };
    let row = fetch_into_hash_aux(vm, &f, &r, MYSQLI_ASSOC);
    let is_arr = matches!(row, Value::Array(_));
    *it.current_row.borrow_mut() = row;
    if is_arr {
        it.row_num.set(it.row_num.get() + 1);
    }
}

/// The InternalIterator methods for a mysqli_result iterator.
pub fn iter_call(vm: &mut Vm, it: &ResultIter, method: &[u8]) -> Value {
    match method {
        b"rewind" => {
            let res = get_res(&it.result).and_then(|r| match &r.kind {
                Kind::Result(r) => Some(r.clone()),
                _ => None,
            });
            if let Some(r) = res {
                let unbuf = r.borrow().unbuf.as_ref().map(|u| u.eof_reached);
                match unbuf {
                    Some(true) => {
                        vm.emit_warning(
                            "Data fetched with MYSQLI_USE_RESULT can be iterated only once",
                        );
                        return Value::Null;
                    }
                    Some(false) => {}
                    None => {
                        r.borrow_mut().data_seek(0);
                    }
                }
            }
            it.row_num.set(-1);
            iter_move_forward(vm, it);
            Value::Null
        }
        b"valid" => {
            if matches!(*it.current_row.borrow(), Value::Array(_)) {
                Value::True
            } else {
                Value::False
            }
        }
        b"current" => match &*it.current_row.borrow() {
            Value::Undef => Value::Null,
            v => v.clone(),
        },
        b"key" => Value::Long(it.row_num.get()),
        b"next" => {
            iter_move_forward(vm, it);
            Value::Null
        }
        _ => Value::Null,
    }
}
