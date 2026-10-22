//! The large object functions of pgsql.c (pg_lo_*).

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::fetch::oid_value;
use crate::handles::*;
use crate::libpq::lo::{INV_READ, INV_WRITE};
use crate::zpp::{self, Arg, parse, parse_ex, throw, value_error};

/// is_valid_oid_string (strtoul over the whole string)
fn valid_oid_string(s: &[u8]) -> Option<u32> {
    let t = String::from_utf8_lossy(s);
    let t2 = t.trim_start();
    let (neg, digits) = match t2.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, t2.strip_prefix('+').unwrap_or(t2)),
    };
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        if s.is_empty() {
            // strtoul("") = 0 with end == start: valid only if empty
            return Some(0);
        }
        return None;
    }
    let v: u64 = digits.parse().unwrap_or(u64::MAX);
    let v = if neg { v.wrapping_neg() } else { v };
    Some(v as u32)
}

fn invalid_oid(vm: &mut Vm) {
    throw(vm, b"ValueError", "Invalid OID value passed");
}

/// An OID argument: string or int (pg_lo_create / pg_lo_import's $oid)
fn oid_arg(vm: &mut Vm, v: &Value) -> Option<u32> {
    match v.deref() {
        Value::String(s) => match valid_oid_string(s.as_bytes()) {
            Some(o) => Some(o),
            None => {
                invalid_oid(vm);
                None
            }
        },
        Value::Long(n) => {
            if n < 0 {
                invalid_oid(vm);
                return None;
            }
            Some(n as u32)
        }
        other => {
            let m = format!(
                "OID value must be of type string|int, {} given",
                Vm::value_type_name(&other)
            );
            throw(vm, b"TypeError", &m);
            None
        }
    }
}

pub fn pg_lo_create(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "|zz", args) else {
        return Value::Null;
    };
    let (mut link, mut oid) = (a[0].clone(), a[1].clone());
    if args.len() == 1 && !matches!(link, Arg::Any(Value::Object(_))) {
        oid = link;
        link = Arg::Absent;
    }
    let conn = match &link {
        Arg::Absent => match link_or_default(vm, None) {
            Some((_, c)) => c,
            None => return Value::Null,
        },
        Arg::Any(Value::Object(o))
            if o.borrow()
                .class_name
                .eq_ignore_ascii_case(CONNECTION_CLASS.as_bytes()) =>
        {
            match link_conn(vm, o) {
                Some((_, c)) => c,
                None => return Value::Null,
            }
        }
        _ => {
            zpp::type_error(
                vm,
                f,
                1,
                "must be of type PgSql\\Connection when the connection is provided",
            );
            return Value::Null;
        }
    };
    if let Arg::Any(v) = &oid {
        let Some(wanted) = oid_arg(vm, v) else {
            return Value::Null;
        };
        let r = conn.borrow_mut().lo_create(wanted);
        if r == 0 {
            warning(vm, "Unable to create PostgreSQL large object");
            return Value::False;
        }
        return oid_value(r);
    }
    let r = conn.borrow_mut().lo_creat(INV_READ | INV_WRITE);
    if r == 0 {
        warning(vm, "Unable to create PostgreSQL large object");
        return Value::False;
    }
    oid_value(r)
}

/// The quiet zpp attempts of the overloaded functions: (connection, args)
fn try_specs(
    vm: &mut Vm,
    f: &'static str,
    args: &[Value],
    specs: &[&str],
) -> Option<(usize, Vec<Arg>)> {
    for (i, s) in specs.iter().enumerate() {
        if let Ok(a) = parse_ex(vm, f, s, args, true) {
            return Some((i, a));
        }
        if vm.current_exception.is_some() {
            return None;
        }
    }
    None
}

/// The connection of an overload: the link argument or the default link
fn overload_conn(vm: &mut Vm, a: &[Arg], has_link: bool) -> Option<ConnRef> {
    if has_link {
        link_conn(vm, &a[0].obj()?).map(|x| x.1)
    } else {
        link_or_default(vm, None).map(|x| x.1)
    }
}

pub fn pg_lo_unlink(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((which, a)) = try_specs(vm, f, args, &["Cs", "Cl", "s", "l"]) else {
        if vm.current_exception.is_none() {
            throw(
                vm,
                b"ArgumentCountError",
                &format!("Requires 1 or 2 arguments, {} given", args.len()),
            );
        }
        return Value::Null;
    };
    let has_link = which < 2;
    let oa = if has_link { &a[1] } else { &a[0] };
    let oid = match oa {
        Arg::Str(s) => match valid_oid_string(s) {
            Some(o) => o,
            None => {
                invalid_oid(vm);
                return Value::Null;
            }
        },
        _ => {
            let n = oa.long_or(0);
            if n <= 0 {
                invalid_oid(vm);
                return Value::Null;
            }
            n as u32
        }
    };
    let Some(conn) = overload_conn(vm, &a, has_link) else {
        return Value::Null;
    };
    if conn.borrow_mut().lo_unlink(oid) == -1 {
        warning(
            vm,
            &format!("Unable to delete PostgreSQL large object {}", oid),
        );
        return Value::False;
    }
    Value::True
}

pub fn pg_lo_open(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((which, a)) = try_specs(vm, f, args, &["Css", "Cls", "ss", "ls"]) else {
        if vm.current_exception.is_none() {
            throw(
                vm,
                b"ArgumentCountError",
                &format!("Requires 1 or 2 arguments, {} given", args.len()),
            );
        }
        return Value::Null;
    };
    let has_link = which < 2;
    let (oa, mode) = if has_link {
        (&a[1], a[2].str().to_vec())
    } else {
        (&a[0], a[1].str().to_vec())
    };
    let oid = match oa {
        Arg::Str(s) => match valid_oid_string(s) {
            Some(o) => o,
            None => {
                invalid_oid(vm);
                return Value::Null;
            }
        },
        _ => {
            let n = oa.long_or(0);
            if n <= 0 {
                invalid_oid(vm);
                return Value::Null;
            }
            n as u32
        }
    };
    let Some(conn) = overload_conn(vm, &a, has_link) else {
        return Value::Null;
    };
    let (pmode, create) = match mode.as_slice() {
        b"r" => (INV_READ, false),
        b"w" => (INV_WRITE, true),
        b"r+" => (INV_READ | INV_WRITE, false),
        b"w+" => (INV_READ | INV_WRITE, true),
        _ => {
            throw(
                vm,
                b"ValueError",
                "Mode must be one of 'r', 'r+', 'w', or 'w+'",
            );
            return Value::Null;
        }
    };
    let mut fd = conn.borrow_mut().lo_open(oid, pmode);
    if fd == -1 {
        if !create {
            warning(vm, "Unable to open PostgreSQL large object");
            return Value::False;
        }
        let noid = conn.borrow_mut().lo_creat(INV_READ | INV_WRITE);
        if noid == 0 {
            warning(vm, "Unable to create PostgreSQL large object");
            return Value::False;
        }
        fd = conn.borrow_mut().lo_open(noid, pmode);
        if fd == -1 {
            if conn.borrow_mut().lo_unlink(noid) == -1 {
                warning(
                    vm,
                    "Something is really messed up! Your database is badly corrupted in a way NOT related to PHP",
                );
            } else {
                warning(vm, "Unable to open PostgreSQL large object");
            }
            return Value::False;
        }
    }
    new_lob(vm, &conn, fd)
}

fn one_lob(
    vm: &mut Vm,
    f: &'static str,
    spec: &str,
    args: &[Value],
) -> Option<(std::rc::Rc<LobHandle>, ConnRef, Vec<Arg>)> {
    let a = parse(vm, f, spec, args).ok()?;
    let (h, c) = lob_handle(vm, &a[0].obj()?)?;
    Some((h, c, a))
}

pub fn pg_lo_close(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((h, c, _)) = one_lob(vm, f, "L", args) else {
        return Value::Null;
    };
    if c.borrow_mut().lo_close(h.lofd.get()) < 0 {
        warning(
            vm,
            &format!(
                "Unable to close PostgreSQL large object descriptor {}",
                h.lofd.get()
            ),
        );
        Value::False
    } else {
        Value::True
    }
}

pub fn pg_lo_read(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "L|l", args) else {
        return Value::Null;
    };
    let Some((h, c)) = lob_handle(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let len = a[1].long_or(8192);
    if len < 0 {
        value_error(vm, f, 2, "must be greater or equal than 0");
        return Value::Null;
    }
    match c.borrow_mut().lo_read(h.lofd.get(), len as usize) {
        Some(b) => crate::link::sval(&b),
        None => Value::False,
    }
}

pub fn pg_lo_write(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Lp|l!", args) else {
        return Value::Null;
    };
    let data = a[1].str().to_vec();
    let len = match a[2].opt_long() {
        Some(l) => {
            if l < 0 {
                value_error(vm, f, 3, "must be greater than or equal to 0");
                return Value::Null;
            }
            if l > data.len() as i64 {
                value_error(
                    vm,
                    f,
                    3,
                    "must be less than or equal to the length of argument #2 ($buf)",
                );
                return Value::Null;
            }
            l as usize
        }
        None => data.len(),
    };
    let Some((h, c)) = lob_handle(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let n = c.borrow_mut().lo_write(h.lofd.get(), &data[..len]);
    if n == -1 {
        return Value::False;
    }
    Value::Long(n as i64)
}

pub fn pg_lo_read_all(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((h, c, _)) = one_lob(vm, f, "L", args) else {
        return Value::Null;
    };
    let mut total = 0i64;
    loop {
        let r = c.borrow_mut().lo_read(h.lofd.get(), 8192);
        match r {
            Some(b) if !b.is_empty() => {
                total += b.len() as i64;
                vm.write_output(&b);
            }
            _ => break,
        }
    }
    Value::Long(total)
}

pub fn pg_lo_import(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((which, a)) = try_specs(vm, f, args, &["Cp|z", "p|z"]) else {
        if vm.current_exception.is_none() {
            throw(
                vm,
                b"ArgumentCountError",
                &format!("Wrong parameter count for {}()", f),
            );
        }
        return Value::Null;
    };
    let has_link = which == 0;
    let (file, oid) = if has_link {
        (a[1].str().to_vec(), a[2].clone())
    } else {
        (a[0].str().to_vec(), a[1].clone())
    };
    let Some(conn) = overload_conn(vm, &a, has_link) else {
        return Value::Null;
    };
    let file = String::from_utf8_lossy(&file).into_owned();
    if !vm.check_open_basedir(f, &file) {
        return Value::False;
    }
    let wanted = match &oid {
        Arg::Any(v) => match oid_arg(vm, v) {
            Some(o) => o,
            None => return Value::Null,
        },
        _ => 0,
    };
    let r = conn.borrow_mut().lo_import(&file, wanted);
    if r == 0 {
        return Value::False;
    }
    oid_value(r)
}

pub fn pg_lo_export(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((which, a)) = try_specs(vm, f, args, &["Clp", "Csp", "lp", "sp"]) else {
        if vm.current_exception.is_none() {
            throw(
                vm,
                b"ArgumentCountError",
                &format!("Requires 2 or 3 arguments, {} given", args.len()),
            );
        }
        return Value::Null;
    };
    let has_link = which < 2;
    let (oa, file, argn) = if has_link {
        (&a[1], a[2].str().to_vec(), 2)
    } else {
        (&a[0], a[1].str().to_vec(), 1)
    };
    let oid = match oa {
        Arg::Str(s) => match valid_oid_string(s) {
            Some(o) => o,
            None => {
                value_error(vm, f, argn, "Invalid OID value passed");
                return Value::Null;
            }
        },
        _ => {
            let n = oa.long_or(0);
            if n <= 0 {
                value_error(vm, f, argn, "Invalid OID value passed");
                return Value::Null;
            }
            n as u32
        }
    };
    let Some(conn) = overload_conn(vm, &a, has_link) else {
        return Value::Null;
    };
    let file = String::from_utf8_lossy(&file).into_owned();
    if !vm.check_open_basedir(f, &file) {
        return Value::False;
    }
    if conn.borrow_mut().lo_export(oid, &file) == -1 {
        return Value::False;
    }
    Value::True
}

pub fn pg_lo_seek(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Ll|l", args) else {
        return Value::Null;
    };
    let offset = a[1].long_or(0);
    let whence = a[2].long_or(1);
    if !(0..=2).contains(&whence) {
        value_error(
            vm,
            f,
            3,
            "must be one of PGSQL_SEEK_SET, PGSQL_SEEK_CUR, or PGSQL_SEEK_END",
        );
        return Value::Null;
    }
    let Some((h, c)) = lob_handle(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let mut cc = c.borrow_mut();
    let r = if cc.server_version() >= 90300 {
        cc.lo_lseek64(h.lofd.get(), offset, whence as i32)
    } else {
        cc.lo_lseek(h.lofd.get(), offset as i32, whence as i32) as i64
    };
    crate::link::bval(r > -1)
}

pub fn pg_lo_tell(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((h, c, _)) = one_lob(vm, f, "L", args) else {
        return Value::Null;
    };
    let mut cc = c.borrow_mut();
    let r = if cc.server_version() >= 90300 {
        cc.lo_tell64(h.lofd.get())
    } else {
        cc.lo_tell(h.lofd.get()) as i64
    };
    Value::Long(r)
}

pub fn pg_lo_truncate(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((h, c, a)) = one_lob(vm, f, "Ll", args) else {
        return Value::Null;
    };
    let size = a[1].long_or(0);
    let r = c.borrow_mut().lo_truncate(h.lofd.get(), size);
    crate::link::bval(r == 0)
}
