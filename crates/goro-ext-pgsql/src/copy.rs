//! COPY and the escaping functions of pgsql.c.

use goro_core::array::PhpArray;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::handles::*;
use crate::libpq::ExecStatus;
use crate::libpq::exec::CopyData;
use crate::link::{arr_value, sval};
use crate::zpp::{self, Arg, parse, value_error};

pub fn pg_end_copy(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "|?C", args) else {
        return Value::Null;
    };
    let Some((_, c)) = link_or_default(vm, a[0].obj()) else {
        return Value::Null;
    };
    let r = c.borrow_mut().end_copy();
    if r != 0 {
        pq_error(vm, "Query failed: %s", &c);
        return Value::False;
    }
    Value::True
}

pub fn pg_put_line(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let n = args.len();
    let (c, q) = if n == 1 {
        let Ok(a) = parse(vm, f, "s", args) else {
            return Value::Null;
        };
        let Some((_, c)) = link_or_default(vm, None) else {
            return Value::Null;
        };
        (c, a[0].str().to_vec())
    } else if n == 2 {
        let Ok(a) = parse(vm, f, "Cs", args) else {
            return Value::Null;
        };
        let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
            return Value::Null;
        };
        (c, a[1].str().to_vec())
    } else {
        zpp::wrong_count(vm, f, 1, 2, n);
        return Value::Null;
    };
    let r = c.borrow_mut().put_line(&q);
    if r == -1 {
        pq_error(vm, "Query failed: %s", &c);
        return Value::False;
    }
    Value::True
}

/// The delimiter and NULL marker arguments of pg_copy_to / pg_copy_from
fn copy_opts(
    vm: &mut Vm,
    f: &'static str,
    delim: &Arg,
    null_as: &Arg,
    argn: usize,
) -> Option<(u8, Vec<u8>)> {
    let d = match delim {
        Arg::Str(s) => {
            if s.len() != 1 {
                value_error(vm, f, argn, "must be one character");
                return None;
            }
            s[0]
        }
        _ => b'\t',
    };
    let n = match null_as {
        Arg::Str(s) => {
            let k = s.iter().position(|&c| c == 0).unwrap_or(s.len());
            s[..k].to_vec()
        }
        _ => b"\\\\N".to_vec(),
    };
    Some((d, n))
}

pub fn pg_copy_to(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cp|ss", args) else {
        return Value::Null;
    };
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let Some((delim, null_as)) = copy_opts(vm, f, &a[2], &a[3], 3) else {
        return Value::Null;
    };
    let mut q = b"COPY ".to_vec();
    q.extend_from_slice(a[1].str());
    q.extend_from_slice(b" TO STDOUT DELIMITER E'");
    q.push(delim);
    q.extend_from_slice(b"' NULL AS E'");
    q.extend_from_slice(&null_as);
    q.push(b'\'');
    let res = {
        let mut cc = c.borrow_mut();
        while cc.get_result().is_some() {}
        cc.exec(&q)
    };
    let status = match &res {
        Some(r) => r.status as i32,
        None => c.borrow().status as i32,
    };
    if status != ExecStatus::CopyOut as i32 {
        pq_error(vm, "Copy command failed: %s", &c);
        return Value::False;
    }
    if res.is_none() {
        return Value::False;
    }
    let mut arr = PhpArray::new();
    loop {
        let r = c.borrow_mut().get_copy_data(false);
        match r {
            CopyData::Done => break,
            CopyData::WouldBlock | CopyData::Error => {
                pq_error(vm, "getline failed: %s", &c);
                return Value::False;
            }
            CopyData::Row(d) => {
                let n = d.iter().position(|&x| x == 0).unwrap_or(d.len());
                arr.push(sval(&d[..n]));
            }
        }
    }
    while c.borrow_mut().get_result().is_some() {}
    arr_value(arr)
}

/// pgsql_copy_from_query
fn copy_from_query(vm: &mut Vm, c: &ConnRef, v: &Value) -> Result<(), bool> {
    let s = match vm.value_to_string_checked(&v.deref()) {
        Ok(s) => s,
        Err(_) => return Err(true),
    };
    if vm.current_exception.is_some() {
        return Err(true);
    }
    let mut b = s.as_bytes().to_vec();
    if !b.is_empty() && b[b.len() - 1] != b'\n' {
        b.push(b'\n');
    }
    if c.borrow_mut().put_copy_data(&b) != 1 {
        return Err(false);
    }
    Ok(())
}

pub fn pg_copy_from(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cpz|ss", args) else {
        return Value::Null;
    };
    // Z_PARAM_ITERABLE
    let rows = match &a[2] {
        Arg::Any(v @ (Value::Array(_) | Value::Generator(_))) => v.clone(),
        Arg::Any(v @ Value::Object(_)) if vm.obj_instanceof(v, b"traversable") => v.clone(),
        Arg::Any(v) => {
            zpp::type_mismatch(vm, f, 3, "Traversable|array", v);
            return Value::Null;
        }
        _ => return Value::Null,
    };
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let Some((delim, null_as)) = copy_opts(vm, f, &a[3], &a[4], 4) else {
        return Value::Null;
    };
    let mut q = b"COPY ".to_vec();
    q.extend_from_slice(a[1].str());
    q.extend_from_slice(b" FROM STDIN DELIMITER E'");
    q.push(delim);
    q.extend_from_slice(b"' NULL AS E'");
    q.extend_from_slice(&null_as);
    q.push(b'\'');
    let res = {
        let mut cc = c.borrow_mut();
        while cc.get_result().is_some() {}
        cc.exec(&q)
    };
    let status = match &res {
        Some(r) => r.status as i32,
        None => c.borrow().status as i32,
    };
    if status != ExecStatus::CopyIn as i32 {
        pq_error(vm, "Copy command failed: %s", &c);
        return Value::False;
    }
    if res.is_none() {
        return Value::False;
    }
    match &rows {
        Value::Array(arr) => {
            let vals: Vec<Value> = arr.borrow().iter().map(|(_, v)| v.clone()).collect();
            for v in vals {
                match copy_from_query(vm, &c, &v) {
                    Ok(()) => {}
                    Err(true) => return Value::Null,
                    Err(false) => {
                        pq_error(vm, "copy failed: %s", &c);
                        return Value::False;
                    }
                }
            }
        }
        _ => {
            let all = vm.call_callback(
                &Value::String(goro_core::string::PhpString::from_bytes(
                    b"iterator_to_array",
                )),
                &[rows.clone(), Value::False],
            );
            if vm.current_exception.is_some() {
                return Value::Null;
            }
            let vals: Vec<Value> = match all {
                Ok(Value::Array(a)) => a.borrow().iter().map(|(_, v)| v.clone()).collect(),
                _ => return Value::Null,
            };
            for v in vals {
                match copy_from_query(vm, &c, &v) {
                    Ok(()) => {}
                    Err(true) => return Value::Null,
                    Err(false) => {
                        pq_error(vm, "copy failed: %s", &c);
                        return Value::False;
                    }
                }
            }
        }
    }
    if c.borrow_mut().put_copy_end(None) != 1 {
        pq_error(vm, "putcopyend failed: %s", &c);
        return Value::False;
    }
    let mut failed = false;
    loop {
        let r = c.borrow_mut().get_result();
        let Some(r) = r else {
            break;
        };
        if r.status != ExecStatus::CommandOk {
            pq_error(vm, "Copy command failed: %s", &c);
            failed = true;
        }
    }
    crate::link::bval(!failed)
}

/// The overloaded (connection?, string) functions: (link, string); the
/// link is the default one (possibly none) with one argument.
fn link_and_string(
    vm: &mut Vm,
    f: &'static str,
    args: &[Value],
    require_default: bool,
) -> Option<(Option<ConnRef>, Vec<u8>)> {
    if args.len() == 1 {
        let a = parse(vm, f, "s", args).ok()?;
        let s = a[0].str().to_vec();
        if require_default {
            let (_, c) = link_or_default(vm, None)?;
            return Some((Some(c), s));
        }
        let l = default_link().and_then(|o| link_of(&o));
        deprecated(
            vm,
            "Automatic fetching of PostgreSQL connection is deprecated",
        );
        if vm.current_exception.is_some() {
            return None;
        }
        let c = l.and_then(|l| l.conn.borrow().clone());
        Some((c, s))
    } else {
        let a = parse(vm, f, "Cs", args).ok()?;
        let (_, c) = link_conn(vm, &a[0].obj()?)?;
        Some((Some(c), a[1].str().to_vec()))
    }
}

pub fn pg_escape_string(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((c, s)) = link_and_string(vm, f, args, false) else {
        return Value::Null;
    };
    match c {
        Some(c) => {
            let (out, err) = c.borrow_mut().escape_string_conn(&s);
            if err {
                value_error(vm, f, args.len(), "Escaping string failed");
                return Value::Null;
            }
            sval(&out)
        }
        None => sval(&crate::libpq::escape_string(&s)),
    }
}

pub fn pg_escape_bytea(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((c, s)) = link_and_string(vm, f, args, false) else {
        return Value::Null;
    };
    match c {
        Some(c) => sval(&c.borrow_mut().escape_bytea_conn(&s)),
        None => sval(&crate::libpq::escape_bytea(&s)),
    }
}

pub fn pg_unescape_bytea(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "s", args) else {
        return Value::Null;
    };
    sval(&crate::libpq::unescape_bytea(a[0].str()))
}

fn escape_internal(vm: &mut Vm, f: &'static str, args: &[Value], literal: bool) -> Value {
    let Some((c, s)) = link_and_string(vm, f, args, true) else {
        return Value::Null;
    };
    let c = c.unwrap();
    let r = if literal {
        c.borrow_mut().escape_literal(&s)
    } else {
        c.borrow_mut().escape_identifier(&s)
    };
    match r {
        Some(v) => sval(&v),
        None => {
            warning(vm, "Failed to escape");
            Value::False
        }
    }
}

pub fn pg_escape_literal(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    escape_internal(vm, f, args, true)
}

pub fn pg_escape_identifier(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    escape_internal(vm, f, args, false)
}

pub fn pg_put_copy_data(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cs", args) else {
        return Value::Null;
    };
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let d = a[1].str().to_vec();
    Value::Long(c.borrow_mut().put_copy_data(&d) as i64)
}

pub fn pg_put_copy_end(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "C|s!", args) else {
        return Value::Null;
    };
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let err = match &a[1] {
        Arg::Str(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    Value::Long(c.borrow_mut().put_copy_end(err.as_deref()) as i64)
}
