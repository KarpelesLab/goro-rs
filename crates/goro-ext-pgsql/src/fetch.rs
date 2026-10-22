//! The result functions of pgsql.c: counts, field information, fetching
//! rows, the result status and error fields.

use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::handles::*;
use crate::libpq::{ExecStatus, PgResult};
use crate::link::{arr_value, skey, sval};
use crate::zpp::{self, Arg, parse, throw, value_error};

/// PGSQL_RETURN_OID
pub fn oid_value(oid: u32) -> Value {
    Value::Long(oid as i64)
}

fn one_result(vm: &mut Vm, f: &'static str, args: &[Value]) -> Option<Rc<ResultHandle>> {
    let a = parse(vm, f, "R", args).ok()?;
    result_handle(vm, &a[0].obj()?)
}

/// Run `g` on the result
fn with_res<T>(h: &ResultHandle, g: impl FnOnce(&PgResult) -> T) -> T {
    let r = h.result.borrow();
    g(r.as_ref().unwrap())
}

pub fn pg_num_rows(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some(h) = one_result(vm, f, args) else {
        return Value::Null;
    };
    Value::Long(with_res(&h, |r| r.ntuples() as i64))
}

pub fn pg_num_fields(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some(h) = one_result(vm, f, args) else {
        return Value::Null;
    };
    Value::Long(with_res(&h, |r| r.nfields() as i64))
}

/// atoi()
pub fn atoi(s: &[u8]) -> i64 {
    let t = String::from_utf8_lossy(s);
    let t = t.trim_start();
    let mut end = 0;
    let b = t.as_bytes();
    if end < b.len() && (b[end] == b'-' || b[end] == b'+') {
        end += 1;
    }
    while end < b.len() && b[end].is_ascii_digit() {
        end += 1;
    }
    t[..end]
        .parse::<i64>()
        .map(|v| v as i32 as i64)
        .unwrap_or(0)
}

pub fn pg_affected_rows(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some(h) = one_result(vm, f, args) else {
        return Value::Null;
    };
    Value::Long(with_res(&h, |r| atoi(r.cmd_tuples())))
}

/// get_field_name: the type name of an OID (cached from pg_type)
pub fn get_field_name(conn: &ConnRef, oid: u32) -> Vec<u8> {
    if let Some(n) = PGG.with(|g| g.borrow().field_oids.get(&oid).cloned()) {
        return n;
    }
    let res = conn.borrow_mut().exec(b"select oid,typname from pg_type");
    let res = match res {
        Some(r) if r.status == ExecStatus::TuplesOk => r,
        _ => return Vec::new(),
    };
    let oid_off = res.fnumber(b"oid");
    let name_off = res.fnumber(b"typname");
    let mut ret = None;
    PGG.with(|g| {
        let mut g = g.borrow_mut();
        for i in 0..res.ntuples() {
            if oid_off < 0 || name_off < 0 {
                break;
            }
            let o: u32 = std::str::from_utf8(res.value(i, oid_off as usize))
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .map(|v| v as u32)
                .unwrap_or(0);
            let name = res.value(i, name_off as usize).to_vec();
            if ret.is_none() && o == oid {
                ret = Some(name.clone());
            }
            g.field_oids.insert(o, name);
        }
    });
    ret.unwrap_or_default()
}

pub fn pg_field_table(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Rl|b", args) else {
        return Value::Null;
    };
    let Some(h) = result_handle(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let fnum = a[1].long_or(-1);
    let return_oid = a[2].bool_or(false);
    if fnum < 0 {
        value_error(vm, f, 2, "must be greater than or equal to 0");
        return Value::Null;
    }
    if fnum >= with_res(&h, |r| r.nfields() as i64) {
        value_error(
            vm,
            f,
            2,
            "must be less than the number of fields for this result set",
        );
        return Value::Null;
    }
    let oid = with_res(&h, |r| r.ftable(fnum as usize));
    if oid == 0 {
        return Value::False;
    }
    if return_oid {
        return oid_value(oid);
    }
    if let Some(t) = PGG.with(|g| g.borrow().table_oids.get(&oid).cloned()) {
        return sval(&t);
    }
    let Some(conn) = h.conn.borrow().clone() else {
        return Value::False;
    };
    let q = format!("select relname from pg_class where oid={}", oid);
    let res = conn.borrow_mut().exec(q.as_bytes());
    let res = match res {
        Some(r) if r.status == ExecStatus::TuplesOk => r,
        _ => return Value::False,
    };
    let name = res.value(0, 0).to_vec();
    PGG.with(|g| g.borrow_mut().table_oids.insert(oid, name.clone()));
    sval(&name)
}

#[derive(Clone, Copy)]
enum FieldInfo {
    Name,
    Size,
    Type,
    TypeOid,
}

/// php_pgsql_get_field_info
fn field_info(vm: &mut Vm, f: &'static str, args: &[Value], what: FieldInfo) -> Value {
    let Ok(a) = parse(vm, f, "Rl", args) else {
        return Value::Null;
    };
    let Some(h) = result_handle(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let field = a[1].long_or(0);
    if field < 0 {
        value_error(vm, f, 2, "must be greater than or equal to 0");
        return Value::Null;
    }
    if field >= with_res(&h, |r| r.nfields() as i64) {
        value_error(
            vm,
            f,
            2,
            "must be less than the number of fields for this result set",
        );
        return Value::Null;
    }
    let field = field as usize;
    match what {
        FieldInfo::Name => sval(&with_res(&h, |r| r.fname(field).unwrap_or(b"").to_vec())),
        FieldInfo::Size => Value::Long(with_res(&h, |r| r.fsize(field) as i64)),
        FieldInfo::Type => {
            let oid = with_res(&h, |r| r.ftype(field));
            let Some(conn) = h.conn.borrow().clone() else {
                return sval(b"");
            };
            sval(&get_field_name(&conn, oid))
        }
        FieldInfo::TypeOid => oid_value(with_res(&h, |r| r.ftype(field))),
    }
}

pub fn pg_field_name(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    field_info(vm, f, args, FieldInfo::Name)
}

pub fn pg_field_size(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    field_info(vm, f, args, FieldInfo::Size)
}

pub fn pg_field_type(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    field_info(vm, f, args, FieldInfo::Type)
}

pub fn pg_field_type_oid(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    field_info(vm, f, args, FieldInfo::TypeOid)
}

pub fn pg_field_num(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Rs", args) else {
        return Value::Null;
    };
    let Some(h) = result_handle(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let name = a[1].str().to_vec();
    let n = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Value::Long(with_res(&h, |r| r.fnumber(&name[..n]) as i64))
}

/// field_arg_to_offset
fn field_arg_to_offset(vm: &mut Vm, r: &PgResult, field: &Arg, arg_num: usize) -> i64 {
    match field {
        Arg::Str(name) => {
            let n = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            let off = r.fnumber(&name[..n]) as i64;
            if off < 0 {
                throw(
                    vm,
                    b"ValueError",
                    &format!(
                        "Argument #{} must be a field name from this result set",
                        arg_num
                    ),
                );
                return -1;
            }
            off
        }
        _ => {
            let off = field.long_or(0);
            if off < 0 {
                throw(
                    vm,
                    b"ValueError",
                    &format!("Argument #{} must be greater than or equal to 0", arg_num),
                );
                return -1;
            }
            if off >= r.nfields() as i64 {
                throw(
                    vm,
                    b"ValueError",
                    &format!(
                        "Argument #{} must be less than the number of fields for this result set",
                        arg_num
                    ),
                );
                return -1;
            }
            off
        }
    }
}

/// The (result, row, field) of pg_fetch_result & co.: Ok(None) when the
/// function returns false, Err(()) when it threw.
fn result_row_field(
    vm: &mut Vm,
    f: &'static str,
    args: &[Value],
    nullable_row: bool,
    deprecate_two: bool,
) -> Result<Option<(Rc<ResultHandle>, usize, usize)>, ()> {
    let n = args.len();
    let (o, row, field) = if n == 2 {
        if deprecate_two {
            vm.emit_deprecated(&format!(
                "Calling {}() with 2 arguments is deprecated, use the 3-parameter signature with a null $row parameter instead",
                f
            ));
            if vm.current_exception.is_some() {
                return Err(());
            }
        }
        let a = parse(vm, f, "R", &args[..1])?;
        let field = zpp::str_or_long(vm, f, 2, &args[1])?;
        (a[0].obj().unwrap(), None, field)
    } else if n == 3 {
        let spec = if nullable_row { "Rl!" } else { "Rl" };
        let a = parse(vm, f, spec, &args[..2])?;
        let field = zpp::str_or_long(vm, f, 3, &args[2])?;
        (a[0].obj().unwrap(), a[1].opt_long(), field)
    } else {
        zpp::wrong_count(vm, f, 2, 3, n);
        return Err(());
    };
    let Some(h) = result_handle(vm, &o) else {
        return Err(());
    };
    let ntuples = with_res(&h, |r| r.ntuples() as i64);
    let pgsql_row = match row {
        None => {
            if h.row.get() < 0 {
                h.row.set(0);
            }
            let r = h.row.get();
            if r < 0 || r >= ntuples {
                return Ok(None);
            }
            r
        }
        Some(row) => {
            if row < 0 {
                value_error(vm, f, 2, "must be greater than or equal to 0");
                return Err(());
            }
            if row >= ntuples {
                warning(
                    vm,
                    &format!(
                        "Unable to jump to row {} on PostgreSQL result index {}",
                        row,
                        o.borrow().object_id
                    ),
                );
                return Ok(None);
            }
            row
        }
    };
    let off = {
        let r = h.result.borrow();
        field_arg_to_offset(vm, r.as_ref().unwrap(), &field, n)
    };
    if off < 0 {
        return Err(());
    }
    Ok(Some((h, pgsql_row as usize, off as usize)))
}

pub fn pg_fetch_result(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let is_alias = f == "pg_result";
    let n = args.len();
    match result_row_field(vm, f, args, !is_alias, !is_alias) {
        Err(()) => Value::Null,
        Ok(None) => Value::False,
        Ok(Some((h, row, field))) => {
            // the internal row counter moves on when no row was given
            let row_given = n == 3 && !matches!(args[1].deref(), Value::Null);
            if !row_given {
                h.row.set(h.row.get() + 1);
            }
            with_res(&h, |r| {
                if r.is_null(row, field) {
                    Value::Null
                } else {
                    sval(r.value(row, field))
                }
            })
        }
    }
}

/// php_pgsql_fetch_hash
fn fetch_hash(
    vm: &mut Vm,
    f: &'static str,
    args: &[Value],
    default_type: i64,
    into_object: bool,
) -> Value {
    let (o, row, mut result_type, class, ctor_args) = if into_object {
        let Ok(a) = parse(vm, f, "R|l!sa", args) else {
            return Value::Null;
        };
        let class = match &a[2] {
            Arg::Str(c) => {
                // Z_PARAM_CLASS (zend_parse_arg_class)
                let lower = c.to_ascii_lowercase();
                let lower = lower
                    .strip_prefix(b"\\")
                    .map(|x| x.to_vec())
                    .unwrap_or(lower);
                if !vm.classes.contains_key(&lower) && lower != b"stdclass" {
                    let _ = vm.try_autoload_class(c);
                }
                if !vm.classes.contains_key(&lower) && lower != b"stdclass" {
                    zpp::type_error(
                        vm,
                        f,
                        3,
                        &format!(
                            "must be a valid class name, {} given",
                            String::from_utf8_lossy(c)
                        ),
                    );
                    return Value::Null;
                }
                if let Some(ce) = vm.classes.get(&lower) {
                    if ce.is_interface || ce.is_trait || ce.is_abstract || ce.is_enum {
                        let what = if ce.is_interface {
                            "interface"
                        } else if ce.is_trait {
                            "trait"
                        } else if ce.is_enum {
                            "enum"
                        } else {
                            "abstract class"
                        };
                        let m = format!(
                            "Cannot instantiate {} {}",
                            what,
                            String::from_utf8_lossy(&ce.name)
                        );
                        throw(vm, b"Error", &m);
                        return Value::Null;
                    }
                }
                Some(lower)
            }
            _ => None,
        };
        (a[0].obj().unwrap(), a[1].opt_long(), 1, class, a[3].arr())
    } else {
        let Ok(a) = parse(vm, f, "R|l!l", args) else {
            return Value::Null;
        };
        (
            a[0].obj().unwrap(),
            a[1].opt_long(),
            a[2].long_or(default_type),
            None,
            None,
        )
    };
    if into_object {
        result_type = 1;
    }
    if let Some(r) = row {
        if r < 0 {
            value_error(vm, f, 2, "must be greater than or equal to 0");
            return Value::Null;
        }
    }
    if result_type & 3 == 0 {
        value_error(
            vm,
            f,
            3,
            "must be one of PGSQL_ASSOC, PGSQL_NUM, or PGSQL_BOTH",
        );
        return Value::Null;
    }
    let Some(h) = result_handle(vm, &o) else {
        return Value::Null;
    };
    let ntuples = with_res(&h, |r| r.ntuples() as i64);
    let pgsql_row = match row {
        Some(r) => {
            if r >= ntuples {
                warning(
                    vm,
                    &format!(
                        "Unable to jump to row {} on PostgreSQL result index {}",
                        r,
                        o.borrow().object_id
                    ),
                );
                return Value::False;
            }
            h.row.set(r);
            r
        }
        None => {
            let r = h.row.get();
            if r < 0 || r >= ntuples {
                return Value::False;
            }
            h.row.set(r + 1);
            r
        }
    } as usize;
    let arr = with_res(&h, |r| {
        let mut arr = PhpArray::new();
        for i in 0..r.nfields() {
            let name = r.fname(i).unwrap_or(b"");
            let v = if r.is_null(pgsql_row, i) {
                Value::Null
            } else {
                let e = r.value(pgsql_row, i);
                let n = e.iter().position(|&c| c == 0).unwrap_or(e.len());
                sval(&e[..n])
            };
            if result_type & 2 != 0 {
                arr.set(ArrayKey::Int(i as i64), v.clone());
            }
            if result_type & 1 != 0 {
                arr.set(skey(name), v);
            }
        }
        arr
    });
    if !into_object {
        return arr_value(arr);
    }
    let class = class.unwrap_or_else(|| b"stdclass".to_vec());
    make_object(vm, f, &class, arr, ctor_args)
}

/// object_init_ex + zend_merge_properties + the constructor call
pub fn make_object(
    vm: &mut Vm,
    f: &'static str,
    ce: &[u8],
    arr: PhpArray,
    ctor_params: Option<std::rc::Rc<std::cell::RefCell<PhpArray>>>,
) -> Value {
    use goro_core::object::PhpObject;
    use goro_core::string::PhpString;
    let ce = ce.to_ascii_lowercase();
    let disp = if ce == b"stdclass" {
        b"stdClass".to_vec()
    } else {
        vm.classes
            .get(&ce)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| ce.to_vec())
    };
    let id = vm.alloc_object_id();
    let mut obj = PhpObject::new(disp, id);
    vm.apply_class_property_defaults(&mut obj, &ce);
    let o = std::rc::Rc::new(std::cell::RefCell::new(obj));
    vm.run_object_create_hook(&o);
    let ov = Value::Object(o.clone());
    let has_set = vm
        .classes
        .get(&ce)
        .map(|c| c.methods.contains_key(b"__set".as_slice()))
        .unwrap_or(false);
    let entries: Vec<(ArrayKey, Value)> = arr.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, v) in entries {
        let name: Vec<u8> = match &k {
            ArrayKey::Int(n) => n.to_string().into_bytes(),
            ArrayKey::String(s) => s.as_bytes().to_vec(),
        };
        let declared = vm
            .classes
            .get(&ce)
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
        .get(&ce)
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
            throw(vm, b"ArgumentCountError", &msg);
            return Value::Null;
        }
        let _ = vm.call_object_method(&ov, b"__construct", &params);
        if vm.current_exception.is_some() {
            return Value::Null;
        }
    } else if let Some(p) = &ctor_params {
        if !p.borrow().is_empty() {
            let cname = String::from_utf8_lossy(&o.borrow().class_name).into_owned();
            value_error(
                vm,
                f,
                3,
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

pub fn pg_fetch_row(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    fetch_hash(vm, f, args, 2, false)
}

pub fn pg_fetch_assoc(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    fetch_hash(vm, f, args, 1, false)
}

pub fn pg_fetch_array(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    fetch_hash(vm, f, args, 3, false)
}

pub fn pg_fetch_object(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    fetch_hash(vm, f, args, 1, true)
}

/// php_pgsql_result2array
pub fn result2array(r: &PgResult, result_type: i64) -> PhpArray {
    let mut ret = PhpArray::new();
    for row in 0..r.ntuples() {
        let mut a = PhpArray::new();
        for i in 0..r.nfields() {
            let name = r.fname(i).unwrap_or(b"");
            let v = if r.is_null(row, i) {
                Value::Null
            } else {
                let e = r.value(row, i);
                let n = e.iter().position(|&c| c == 0).unwrap_or(e.len());
                sval(&e[..n])
            };
            if result_type & 1 != 0 {
                a.set(skey(name), v.clone());
            }
            if result_type & 2 != 0 {
                a.push(v);
            }
        }
        ret.set(ArrayKey::Int(row as i64), arr_value(a));
    }
    ret
}

pub fn pg_fetch_all(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "R|l", args) else {
        return Value::Null;
    };
    let t = a[1].long_or(1);
    if t & 3 == 0 {
        value_error(
            vm,
            f,
            2,
            "must be one of PGSQL_ASSOC, PGSQL_NUM, or PGSQL_BOTH",
        );
        return Value::Null;
    }
    let Some(h) = result_handle(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    arr_value(with_res(&h, |r| result2array(r, t)))
}

pub fn pg_fetch_all_columns(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "R|l", args) else {
        return Value::Null;
    };
    let Some(h) = result_handle(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let colno = a[1].long_or(0);
    if colno < 0 {
        value_error(vm, f, 2, "must be greater than or equal to 0");
        return Value::Null;
    }
    if colno >= with_res(&h, |r| r.nfields() as i64) {
        value_error(
            vm,
            f,
            2,
            "must be less than the number of fields for this result set",
        );
        return Value::Null;
    }
    let col = colno as usize;
    arr_value(with_res(&h, |r| {
        let mut a = PhpArray::new();
        for row in 0..r.ntuples() {
            let v = if r.is_null(row, col) {
                Value::Null
            } else {
                let e = r.value(row, col);
                let n = e.iter().position(|&c| c == 0).unwrap_or(e.len());
                sval(&e[..n])
            };
            a.set(ArrayKey::Int(row as i64), v);
        }
        a
    }))
}

pub fn pg_result_seek(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Rl", args) else {
        return Value::Null;
    };
    let Some(h) = result_handle(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let row = a[1].long_or(0);
    if row < 0 || row >= with_res(&h, |r| r.ntuples() as i64) {
        return Value::False;
    }
    h.row.set(row);
    Value::True
}

/// php_pgsql_data_info
fn data_info(
    vm: &mut Vm,
    f: &'static str,
    args: &[Value],
    length: bool,
    nullable_row: bool,
) -> Value {
    match result_row_field(vm, f, args, nullable_row, nullable_row) {
        Err(()) => Value::Null,
        Ok(None) => Value::False,
        Ok(Some((h, row, field))) => with_res(&h, |r| {
            if length {
                Value::Long(r.get_length(row, field) as i64)
            } else {
                Value::Long(r.is_null(row, field) as i64)
            }
        }),
    }
}

pub fn pg_field_prtlen(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    data_info(vm, f, args, true, true)
}

pub fn pg_fieldprtlen(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    data_info(vm, f, args, true, false)
}

pub fn pg_field_is_null(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    data_info(vm, f, args, false, true)
}

pub fn pg_fieldisnull(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    data_info(vm, f, args, false, false)
}

pub fn pg_free_result(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some(h) = one_result(vm, f, args) else {
        return Value::Null;
    };
    *h.result.borrow_mut() = None;
    Value::True
}

pub fn pg_last_oid(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some(h) = one_result(vm, f, args) else {
        return Value::Null;
    };
    let oid = with_res(&h, |r| r.oid_value());
    if oid == 0 {
        return Value::False;
    }
    oid_value(oid)
}

pub fn pg_result_error(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "R", args) else {
        return Value::Null;
    };
    let Some(h) = result_of(&a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let r = h.result.borrow();
    match r.as_ref() {
        None => Value::False,
        Some(r) => {
            let m = r.error_message();
            let n = m.iter().position(|&c| c == 0).unwrap_or(m.len());
            sval(&m[..n])
        }
    }
}

pub fn pg_result_error_field(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Rl", args) else {
        return Value::Null;
    };
    let Some(h) = result_of(&a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let code = a[1].long_or(0);
    let r = h.result.borrow();
    let Some(r) = r.as_ref() else {
        return Value::False;
    };
    let mask: i64 = b"SCMDHPpqWFLR".iter().fold(0, |m, &c| m | c as i64);
    if code & mask != 0 {
        match r.error_field(code as i32 as u8) {
            Some(v) => sval(v),
            None => Value::Null,
        }
    } else {
        Value::False
    }
}

pub fn pg_result_status(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "R|l", args) else {
        return Value::Null;
    };
    let Some(h) = result_handle(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    match a[1].long_or(1) {
        1 => Value::Long(with_res(&h, |r| r.status as i64)),
        2 => sval(&with_res(&h, |r| r.cmd_status.clone())),
        _ => {
            value_error(
                vm,
                f,
                2,
                "must be either PGSQL_STATUS_LONG or PGSQL_STATUS_STRING",
            );
            Value::Null
        }
    }
}

pub fn pg_result_memory_size(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some(h) = one_result(vm, f, args) else {
        return Value::Null;
    };
    Value::Long(with_res(&h, |r| r.memory_size as i64))
}
