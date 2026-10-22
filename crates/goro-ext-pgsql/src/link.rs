//! The connection functions of pgsql.c: connecting, link information,
//! queries (synchronous and asynchronous), notices and notifications,
//! tracing and the connection settings.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::handles::*;
use crate::libpq::exec::Param;
use crate::libpq::{
    ConnStatus, ContextVisibility, ExecStatus, PQTRACE_REGRESS_MODE, PQTRACE_SUPPRESS_TIMESTAMPS,
    PgConn, PgResult, TransStatus, Verbosity,
};
use crate::zpp::{self, Arg, Obj, parse, value_error};

const PGSQL_CONNECT_FORCE_NEW: i64 = 2;
const PGSQL_CONNECT_ASYNC: i64 = 4;

pub fn sval(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

pub fn bval(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

pub fn arr_value(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

pub fn skey(s: &[u8]) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(s))
}

fn ini_long(vm: &Vm, name: &[u8]) -> i64 {
    vm.constants
        .get(name)
        .map(|v| {
            let s = v.to_php_string().to_string_lossy();
            match s.to_ascii_lowercase().as_str() {
                "on" | "yes" | "true" => 1,
                _ => s.trim().parse::<i64>().unwrap_or(0),
            }
        })
        .unwrap_or(0)
}

/// zend_strtod(PQparameterStatus(pgsql, "server_version"))
fn server_version_f(c: &PgConn) -> f64 {
    let v = c.parameter_status(b"server_version").unwrap_or(b"");
    let s = String::from_utf8_lossy(v);
    let t: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = t.splitn(3, '.');
    let a = parts.next().unwrap_or("");
    let b = parts.next();
    let num = match b {
        Some(b) => format!("{}.{}", a, b),
        None => a.to_string(),
    };
    num.parse().unwrap_or(0.0)
}

/// The libpq calls of a function: notices are flushed afterwards by the
/// wrapper.
fn connect_conn(conninfo: &[u8], is_async: bool) -> PgConn {
    if is_async {
        PgConn::connect_start(conninfo)
    } else {
        PgConn::connectdb(conninfo)
    }
}

/// php_pgsql_do_connect
fn do_connect(vm: &mut Vm, f: &'static str, args: &[Value], persistent: bool) -> Value {
    let Ok(a) = parse(vm, f, "p|l", args) else {
        return Value::Null;
    };
    let connstring = a[0].str().to_vec();
    let connect_type = a[1].long_or(0);
    let mut hash = b"pgsql".to_vec();
    hash.extend_from_slice(&connstring);
    hash.push(b'_');
    hash.extend_from_slice(
        (connect_type & !PGSQL_CONNECT_FORCE_NEW)
            .to_string()
            .as_bytes(),
    );

    let max_links = ini_long(vm, b"pgsql.max_links");
    let allow_persistent = ini_long(vm, b"pgsql.allow_persistent") != 0;
    let (obj, link) = if persistent && allow_persistent {
        let existing = PGG.with(|g| g.borrow().persistent.get(&hash).cloned());
        let conn = match existing {
            None => {
                let (nl, np) = PGG.with(|g| {
                    let g = g.borrow();
                    (g.num_links, g.num_persistent)
                });
                let max_persistent = ini_long(vm, b"pgsql.max_persistent");
                if max_links != -1 && nl >= max_links {
                    warning(
                        vm,
                        &format!("Cannot create new link. Too many open links ({})", nl),
                    );
                    return Value::False;
                }
                if max_persistent != -1 && np >= max_persistent {
                    warning(
                        vm,
                        &format!(
                            "Cannot create new link. Too many open persistent links ({})",
                            np
                        ),
                    );
                    return Value::False;
                }
                let c = connect_conn(&connstring, false);
                if c.status == ConnStatus::Bad {
                    let msg = trim_message(c.error_message());
                    warning(
                        vm,
                        &format!(
                            "Unable to connect to PostgreSQL server: {}",
                            String::from_utf8_lossy(&msg)
                        ),
                    );
                    return Value::False;
                }
                let c = Rc::new(RefCell::new(c));
                PGG.with(|g| {
                    let mut g = g.borrow_mut();
                    g.persistent.insert(hash.clone(), c.clone());
                    g.num_links += 1;
                    g.num_persistent += 1;
                });
                c
            }
            Some(c) => {
                if connect_type & PGSQL_CONNECT_FORCE_NEW != 0 {
                    let mut cc = c.borrow_mut();
                    while cc.get_result().is_some() {}
                    cc.reset();
                }
                if ini_long(vm, b"pgsql.auto_reset_persistent") & 1 != 0 {
                    c.borrow_mut().exec(b"select 1");
                }
                if c.borrow().status == ConnStatus::Bad {
                    c.borrow_mut().reset();
                    if c.borrow().status == ConnStatus::Bad {
                        warning(vm, "PostgreSQL connection lost, unable to reconnect");
                        let old = PGG.with(|g| g.borrow_mut().persistent.remove(&hash));
                        drop(old);
                        return Value::False;
                    }
                }
                if server_version_f(&c.borrow()) >= 7.2 {
                    c.borrow_mut().exec(b"RESET ALL;");
                }
                c
            }
        };
        let (o, l) = new_link(vm, conn, hash.clone(), true);
        (o, l)
    } else {
        if connect_type & PGSQL_CONNECT_FORCE_NEW == 0 {
            let found = PGG.with(|g| g.borrow().connections.get(&hash).and_then(|w| w.upgrade()));
            if let Some(o) = found {
                set_default_link(&o);
                return Value::Object(o);
            }
        }
        let nl = PGG.with(|g| g.borrow().num_links);
        if max_links != -1 && nl >= max_links {
            warning(
                vm,
                &format!("Cannot create new link. Too many open links ({})", nl),
            );
            return Value::False;
        }
        let c = connect_conn(&connstring, connect_type & PGSQL_CONNECT_ASYNC != 0);
        if c.status == ConnStatus::Bad {
            let msg = trim_message(c.error_message());
            warning(
                vm,
                &format!(
                    "Unable to connect to PostgreSQL server: {}",
                    String::from_utf8_lossy(&msg)
                ),
            );
            return Value::False;
        }
        let conn = Rc::new(RefCell::new(c));
        let (o, l) = new_link(vm, conn, hash.clone(), false);
        PGG.with(|g| {
            let mut g = g.borrow_mut();
            g.connections.insert(hash.clone(), Rc::downgrade(&o));
            g.num_links += 1;
        });
        (o, l)
    };
    if !ignore_notices(vm) {
        if let Some(c) = link.conn.borrow().as_ref() {
            c.borrow_mut()
                .set_notice_processor(notice_processor, link.id);
        }
    }
    set_default_link(&obj);
    Value::Object(obj)
}

pub fn pg_connect(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    do_connect(vm, f, args, false)
}

pub fn pg_pconnect(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    do_connect(vm, f, args, true)
}

fn one_link(vm: &mut Vm, f: &'static str, args: &[Value]) -> Option<(Rc<LinkHandle>, ConnRef)> {
    let a = parse(vm, f, "C", args).ok()?;
    link_conn(vm, &a[0].obj()?)
}

fn opt_link(vm: &mut Vm, f: &'static str, args: &[Value]) -> Option<(Rc<LinkHandle>, ConnRef)> {
    let a = parse(vm, f, "|?C", args).ok()?;
    link_or_default(vm, a[0].obj())
}

pub fn pg_connect_poll(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((_, c)) = one_link(vm, f, args) else {
        return Value::Null;
    };
    let r = c.borrow_mut().connect_poll();
    Value::Long(r as i64)
}

pub fn pg_close(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "|?C", args) else {
        return Value::Null;
    };
    let Some(o) = a[0].obj() else {
        if fetch_default_link(vm).is_none() {
            return Value::Null;
        }
        clear_default_link();
        return Value::True;
    };
    let Some((l, _)) = link_conn(vm, &o) else {
        return Value::Null;
    };
    if default_link().is_some_and(|d| Rc::ptr_eq(&d, &o)) {
        clear_default_link();
    }
    link_free(&l);
    Value::True
}

#[derive(Clone, Copy, PartialEq)]
enum Info {
    Dbname,
    ErrorMessage,
    Options,
    Port,
    Tty,
    Host,
    Version,
    Jit,
    Service,
}

/// php_pgsql_get_link_info
fn link_info(vm: &mut Vm, f: &'static str, args: &[Value], what: Info) -> Value {
    let Some((_, c)) = opt_link(vm, f, args) else {
        return Value::Null;
    };
    let result: Vec<u8> = {
        let mut cc = c.borrow_mut();
        match what {
            Info::Dbname => cc.db().as_bytes().to_vec(),
            Info::ErrorMessage => return sval(&trim_message(cc.error_message())),
            Info::Options => cc.options().as_bytes().to_vec(),
            Info::Port => cc.port().into_bytes(),
            Info::Tty => Vec::new(),
            Info::Host => cc.host().into_bytes(),
            Info::Service => cc.service().as_bytes().to_vec(),
            Info::Version => {
                let mut a = PhpArray::new();
                a.set(skey(b"client"), sval(crate::libpq_version().as_bytes()));
                a.set(skey(b"protocol"), Value::Long(cc.protocol_version() as i64));
                a.set(
                    skey(b"server"),
                    sval(cc.parameter_status(b"server_version").unwrap_or(b"")),
                );
                for k in [
                    "server_encoding",
                    "client_encoding",
                    "is_superuser",
                    "session_authorization",
                    "DateStyle",
                    "IntervalStyle",
                    "TimeZone",
                    "integer_datetimes",
                    "standard_conforming_strings",
                    "application_name",
                ] {
                    let v = match cc.parameter_status(k.as_bytes()) {
                        Some(v) => sval(v),
                        None => Value::Null,
                    };
                    a.set(skey(k.as_bytes()), v);
                }
                return arr_value(a);
            }
            Info::Jit => {
                let mut a = PhpArray::new();
                for (q, k) in [
                    (&b"SHOW jit_provider"[..], &b"jit_provider"[..]),
                    (b"SHOW jit", b"jit"),
                ] {
                    let v = match cc.exec(q) {
                        Some(r) if r.status == ExecStatus::TuplesOk => sval(r.value(0, 0)),
                        _ => Value::Null,
                    };
                    a.set(skey(k), v);
                }
                return arr_value(a);
            }
        }
    };
    sval(&result)
}

pub fn pg_dbname(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    link_info(vm, f, args, Info::Dbname)
}

pub fn pg_last_error(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    link_info(vm, f, args, Info::ErrorMessage)
}

pub fn pg_options(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    link_info(vm, f, args, Info::Options)
}

pub fn pg_port(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    link_info(vm, f, args, Info::Port)
}

pub fn pg_tty(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    link_info(vm, f, args, Info::Tty)
}

pub fn pg_host(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    link_info(vm, f, args, Info::Host)
}

pub fn pg_version(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    link_info(vm, f, args, Info::Version)
}

pub fn pg_jit(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    link_info(vm, f, args, Info::Jit)
}

pub fn pg_service(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    link_info(vm, f, args, Info::Service)
}

pub fn pg_parameter_status(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let (link, param) = match zpp::parse_ex(vm, f, "Cs", args, true) {
        Ok(a) => {
            let Some(l) = link_conn(vm, &a[0].obj().unwrap()) else {
                return Value::Null;
            };
            (l, a[1].str().to_vec())
        }
        Err(()) => {
            let Ok(a) = parse(vm, f, "s", args) else {
                return Value::Null;
            };
            let Some(l) = link_or_default(vm, None) else {
                return Value::Null;
            };
            (l, a[0].str().to_vec())
        }
    };
    let c = link.1.borrow();
    let n = param.iter().position(|&x| x == 0).unwrap_or(param.len());
    match c.parameter_status(&param[..n]) {
        Some(v) => sval(v),
        None => Value::False,
    }
}

pub fn pg_ping(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((_, c)) = opt_link(vm, f, args) else {
        return Value::Null;
    };
    let mut c = c.borrow_mut();
    c.exec(b"SELECT 1;");
    if c.status == ConnStatus::Ok {
        return Value::True;
    }
    c.reset();
    bval(c.status == ConnStatus::Ok)
}

/// The (link, rest of the arguments) of the overloaded functions taking an
/// optional connection first: `n` is the argument count without it.
fn overloaded(
    vm: &mut Vm,
    f: &'static str,
    args: &[Value],
    spec: &str,
) -> Option<(ConnRef, Vec<Arg>)> {
    let n_spec = spec.len();
    let n = args.len();
    if n == n_spec {
        let a = parse(vm, f, spec, args).ok()?;
        let (_, c) = link_or_default(vm, None)?;
        Some((c, a))
    } else if n == n_spec + 1 {
        let full = format!("C{}", spec);
        let mut a = parse(vm, f, &full, args).ok()?;
        let o = a.remove(0).obj()?;
        let (_, c) = link_conn(vm, &o)?;
        Some((c, a))
    } else {
        zpp::wrong_count(vm, f, n_spec, n_spec + 1, n);
        None
    }
}

/// The leftover results before a blocking query
fn blocking_start(vm: &mut Vm, c: &ConnRef) -> bool {
    if c.borrow_mut().set_nonblocking(false) != 0 {
        notice(vm, "Cannot set connection to blocking mode");
        return false;
    }
    let mut leftover = false;
    while c.borrow_mut().get_result().is_some() {
        leftover = true;
    }
    if leftover {
        notice(
            vm,
            "Found results on this connection. Use pg_get_result() to get these results first",
        );
    }
    true
}

/// The outcome of pg_query & co.
fn query_result(vm: &mut Vm, c: &ConnRef, res: Option<PgResult>) -> Value {
    let status = match &res {
        Some(r) => r.status as i32,
        None => c.borrow().status as i32,
    };
    match status {
        0 | 5 | 6 | 7 => {
            pq_error(vm, "Query failed: %s", c);
            Value::False
        }
        _ => match res {
            Some(r) => new_result(vm, c, r),
            None => Value::False,
        },
    }
}

fn auto_reset(vm: &Vm) -> bool {
    ini_long(vm, b"pgsql.auto_reset_persistent") & 2 != 0
}

pub fn pg_query(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((c, a)) = overloaded(vm, f, args, "s") else {
        return Value::Null;
    };
    if !blocking_start(vm, &c) {
        return Value::False;
    }
    let q = a[0].str().to_vec();
    let mut res = c.borrow_mut().exec(&q);
    if auto_reset(vm) && c.borrow().status != ConnStatus::Ok {
        c.borrow_mut().reset();
        res = c.borrow_mut().exec(&q);
    }
    query_result(vm, &c, res)
}

/// The parameters of an array (NULL stays NULL); None if a conversion
/// threw.
pub fn params_of(vm: &mut Vm, arr: &Rc<RefCell<PhpArray>>) -> Option<Vec<Param>> {
    let vals: Vec<Value> = arr.borrow().iter().map(|(_, v)| v.deref()).collect();
    let mut out = Vec::with_capacity(vals.len());
    for v in vals {
        match v {
            Value::Null => out.push(None),
            v => {
                let s = match vm.value_to_string_checked(&v) {
                    Ok(s) => s,
                    Err(_) => return None,
                };
                if vm.current_exception.is_some() {
                    return None;
                }
                let b = s.as_bytes();
                // estrndup + strlen: a parameter ends at a NUL
                let n = b.iter().position(|&x| x == 0).unwrap_or(b.len());
                out.push(Some(b[..n].to_vec()));
            }
        }
    }
    Some(out)
}

pub fn pg_query_params(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((c, a)) = overloaded(vm, f, args, "sa") else {
        return Value::Null;
    };
    if !blocking_start(vm, &c) {
        return Value::False;
    }
    let q = a[0].str().to_vec();
    let Some(params) = params_of(vm, &a[1].arr().unwrap()) else {
        return Value::Null;
    };
    let mut res = c.borrow_mut().exec_params(&q, &[], &params, &[], 0);
    if auto_reset(vm) && c.borrow().status != ConnStatus::Ok {
        c.borrow_mut().reset();
        res = c.borrow_mut().exec_params(&q, &[], &params, &[], 0);
    }
    query_result(vm, &c, res)
}

pub fn pg_prepare(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((c, a)) = overloaded(vm, f, args, "ss") else {
        return Value::Null;
    };
    if !blocking_start(vm, &c) {
        return Value::False;
    }
    let (name, q) = (a[0].str().to_vec(), a[1].str().to_vec());
    let mut res = c.borrow_mut().prepare(&name, &q, &[]);
    if auto_reset(vm) && c.borrow().status != ConnStatus::Ok {
        c.borrow_mut().reset();
        res = c.borrow_mut().prepare(&name, &q, &[]);
    }
    query_result(vm, &c, res)
}

pub fn pg_execute(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((c, a)) = overloaded(vm, f, args, "sa") else {
        return Value::Null;
    };
    if !blocking_start(vm, &c) {
        return Value::False;
    }
    let name = a[0].str().to_vec();
    let Some(params) = params_of(vm, &a[1].arr().unwrap()) else {
        return Value::Null;
    };
    let mut res = c.borrow_mut().exec_prepared(&name, &params, &[], 0);
    if auto_reset(vm) && c.borrow().status != ConnStatus::Ok {
        c.borrow_mut().reset();
        res = c.borrow_mut().exec_prepared(&name, &params, &[], 0);
    }
    query_result(vm, &c, res)
}

pub fn pg_last_notice(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "C|l", args) else {
        return Value::Null;
    };
    let Some((l, _)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let option = a[1].long_or(1);
    let notices = l.notices.borrow().clone();
    match option {
        1 => match notices.and_then(|n| n.last().cloned()) {
            Some(n) => sval(&n),
            None => sval(b""),
        },
        2 => {
            let mut arr = PhpArray::new();
            for n in notices.unwrap_or_default() {
                arr.push(sval(&n));
            }
            arr_value(arr)
        }
        3 => {
            if let Some(n) = l.notices.borrow_mut().as_mut() {
                n.clear();
            }
            Value::True
        }
        _ => {
            value_error(
                vm,
                f,
                2,
                "must be one of PGSQL_NOTICE_LAST, PGSQL_NOTICE_ALL, or PGSQL_NOTICE_CLEAR",
            );
            Value::Null
        }
    }
}

pub fn pg_trace(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "p|s?Cl", args) else {
        return Value::Null;
    };
    let Some((_, c)) = link_or_default(vm, a[2].obj()) else {
        return Value::Null;
    };
    let filename = String::from_utf8_lossy(a[0].str()).into_owned();
    let mode = match &a[1] {
        Arg::Str(m) => String::from_utf8_lossy(m).into_owned(),
        _ => "w".to_string(),
    };
    let trace_mode = a[3].long_or(0);
    let mut opts = std::fs::OpenOptions::new();
    let m = mode.replace(['b', 't'], "");
    match m.as_str() {
        "r" => opts.read(true),
        "r+" => opts.read(true).write(true),
        "w" => opts.write(true).create(true).truncate(true),
        "w+" => opts.read(true).write(true).create(true).truncate(true),
        "a" => opts.append(true).create(true),
        "a+" => opts.read(true).append(true).create(true),
        "x" => opts.write(true).create_new(true),
        "x+" => opts.read(true).write(true).create_new(true),
        "c" => opts.write(true).create(true),
        "c+" => opts.read(true).write(true).create(true),
        _ => {
            vm.emit_warning(&format!(
                "{}({}): Failed to open stream: `{}' is not a valid mode for fopen",
                active(),
                filename,
                mode
            ));
            return Value::False;
        }
    };
    let file = match opts.open(&filename) {
        Ok(file) => file,
        Err(e) => {
            let msg = match e.raw_os_error() {
                Some(n) => crate::libpq::conn::strerror(n),
                None => e.to_string(),
            };
            // php_stream_display_wrapper_errors: docref with the path
            vm.emit_warning(&format!(
                "{}({}): Failed to open stream: {}",
                active(),
                filename,
                msg
            ));
            return Value::False;
        }
    };
    c.borrow_mut().set_trace(file);
    if trace_mode > 0 {
        if trace_mode & (PQTRACE_SUPPRESS_TIMESTAMPS | PQTRACE_REGRESS_MODE) as i64 == 0 {
            value_error(
                vm,
                f,
                4,
                "must be PGSQL_TRACE_SUPPRESS_TIMESTAMPS and/or PGSQL_TRACE_REGRESS_MODE",
            );
            return Value::Null;
        }
        c.borrow_mut().trace_flags = trace_mode as i32;
    }
    Value::True
}

pub fn pg_untrace(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((_, c)) = opt_link(vm, f, args) else {
        return Value::Null;
    };
    c.borrow_mut().untrace();
    Value::True
}

pub fn pg_set_error_verbosity(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((c, a)) = overloaded(vm, f, args, "l") else {
        return Value::Null;
    };
    let v = a[0].long_or(0);
    // PQERRORS_TERSE|PQERRORS_DEFAULT|PQERRORS_VERBOSE|PQERRORS_SQLSTATE
    if v & 3 != 0 {
        let old = c.borrow_mut().set_error_verbosity(Verbosity::from_i64(v));
        Value::Long(old as i64)
    } else {
        Value::False
    }
}

pub fn pg_set_error_context_visibility(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cl", args) else {
        return Value::Null;
    };
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let v = a[1].long_or(0);
    if v == 0 || v & (1 | 2) != 0 {
        let old = c
            .borrow_mut()
            .set_error_context_visibility(ContextVisibility::from_i64(v));
        Value::Long(old as i64)
    } else {
        value_error(
            vm,
            f,
            2,
            "must be one of PGSQL_SHOW_CONTEXT_NEVER, PGSQL_SHOW_CONTEXT_ERRORS or PGSQL_SHOW_CONTEXT_ALWAYS",
        );
        Value::Null
    }
}

pub fn pg_set_client_encoding(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((c, a)) = overloaded(vm, f, args, "s") else {
        return Value::Null;
    };
    let enc = a[0].str().to_vec();
    Value::Long(c.borrow_mut().set_client_encoding(&enc) as i64)
}

pub fn pg_client_encoding(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((_, c)) = opt_link(vm, f, args) else {
        return Value::Null;
    };
    let enc = c.borrow().client_encoding;
    sval(crate::libpq::encoding::encoding_to_char(enc).as_bytes())
}

pub fn pg_connection_status(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((_, c)) = one_link(vm, f, args) else {
        return Value::Null;
    };
    Value::Long(c.borrow().status as i64)
}

pub fn pg_transaction_status(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((_, c)) = one_link(vm, f, args) else {
        return Value::Null;
    };
    let s: TransStatus = c.borrow().transaction_status();
    Value::Long(s as i64)
}

pub fn pg_connection_reset(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((_, c)) = one_link(vm, f, args) else {
        return Value::Null;
    };
    c.borrow_mut().reset();
    bval(c.borrow().status != ConnStatus::Bad)
}

/// php_pgsql_do_async
fn do_async(vm: &mut Vm, f: &'static str, args: &[Value], cancel: bool) -> Value {
    let Some((_, c)) = one_link(vm, f, args) else {
        return Value::Null;
    };
    if c.borrow_mut().set_nonblocking(true) != 0 {
        notice(vm, "Cannot set connection to nonblocking mode");
        return Value::False;
    }
    let ret = if !cancel {
        let mut cc = c.borrow_mut();
        cc.consume_input();
        bval(cc.is_busy())
    } else {
        let r = c.borrow_mut().cancel();
        let ret = match r {
            Ok(()) => Value::True,
            Err(e) => {
                vm.emit_warning(&format!("cannot cancel the query: {}", e));
                Value::False
            }
        };
        while c.borrow_mut().get_result().is_some() {}
        ret
    };
    if c.borrow_mut().set_nonblocking(false) != 0 {
        notice(vm, "Cannot set connection to blocking mode");
    }
    ret
}

pub fn pg_cancel_query(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    do_async(vm, f, args, true)
}

pub fn pg_connection_busy(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    do_async(vm, f, args, false)
}

/// _php_pgsql_link_has_results
fn has_results(c: &ConnRef) -> bool {
    c.borrow_mut().get_result().is_some()
}

#[derive(Clone)]
enum Send {
    Query(Vec<u8>),
    Params(Vec<u8>, Vec<Param>),
    Prepare(Vec<u8>, Vec<u8>),
    Execute(Vec<u8>, Vec<Param>),
}

fn do_send(c: &ConnRef, s: &Send) -> bool {
    let mut c = c.borrow_mut();
    match s {
        Send::Query(q) => c.send_query(q),
        Send::Params(q, p) => c.send_query_params(q, &[], p, &[], 0),
        Send::Prepare(n, q) => c.send_prepare(n, q, &[]),
        Send::Execute(n, p) => c.send_query_prepared(n, p, &[], 0),
    }
}

/// The common part of pg_send_query & co. (after the arguments)
fn send_common(vm: &mut Vm, c: &ConnRef, what: impl FnOnce(&mut Vm) -> Option<Send>) -> Value {
    let is_non_blocking = c.borrow().is_nonblocking();
    if !is_non_blocking && c.borrow_mut().set_nonblocking(true) == -1 {
        notice(vm, "Cannot set connection to nonblocking mode");
        return Value::False;
    }
    if has_results(c) {
        notice(
            vm,
            "There are results on this connection. Call pg_get_result() until it returns FALSE",
        );
    }
    let Some(s) = what(vm) else {
        return Value::Null;
    };
    let ret;
    if is_non_blocking {
        if !do_send(c, &s) {
            return Value::False;
        }
        ret = c.borrow_mut().flush();
    } else {
        if !do_send(c, &s) {
            if auto_reset(vm) && c.borrow().status != ConnStatus::Ok {
                c.borrow_mut().reset();
            }
            if !do_send(c, &s) {
                return Value::False;
            }
        }
        let mut r;
        loop {
            r = c.borrow_mut().flush();
            if r == 0 {
                break;
            }
            if r == -1 {
                notice(vm, "Could not empty PostgreSQL send buffer");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        ret = r;
        if c.borrow_mut().set_nonblocking(false) != 0 {
            notice(vm, "Cannot set connection to blocking mode");
        }
    }
    match ret {
        0 => Value::True,
        -1 => Value::False,
        _ => Value::Long(0),
    }
}

pub fn pg_send_query(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cs", args) else {
        return Value::Null;
    };
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let q = a[1].str().to_vec();
    send_common(vm, &c, |_| Some(Send::Query(q)))
}

pub fn pg_send_query_params(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Csa", args) else {
        return Value::Null;
    };
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let q = a[1].str().to_vec();
    let arr = a[2].arr().unwrap();
    send_common(vm, &c, |vm| {
        let p = params_of(vm, &arr)?;
        Some(Send::Params(q, p))
    })
}

pub fn pg_send_prepare(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Css", args) else {
        return Value::Null;
    };
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let (n, q) = (a[1].str().to_vec(), a[2].str().to_vec());
    send_common(vm, &c, |_| Some(Send::Prepare(n, q)))
}

pub fn pg_send_execute(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Csa", args) else {
        return Value::Null;
    };
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let n = a[1].str().to_vec();
    let arr = a[2].arr().unwrap();
    send_common(vm, &c, |vm| {
        let p = params_of(vm, &arr)?;
        Some(Send::Execute(n, p))
    })
}

pub fn pg_get_result(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((_, c)) = one_link(vm, f, args) else {
        return Value::Null;
    };
    let r = c.borrow_mut().get_result();
    match r {
        Some(r) => new_result(vm, &c, r),
        None => Value::False,
    }
}

pub fn pg_get_notify(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "C|l", args) else {
        return Value::Null;
    };
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
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
    let mut cc = c.borrow_mut();
    cc.consume_input();
    let Some(n) = cc.notifies() else {
        return Value::False;
    };
    let v9 = server_version_f(&cc) >= 9.0;
    let mut arr = PhpArray::new();
    if t & 2 != 0 {
        arr.set(ArrayKey::Int(0), sval(&n.relname));
        arr.set(ArrayKey::Int(1), Value::Long(n.be_pid as i64));
        if v9 {
            arr.set(ArrayKey::Int(2), sval(&n.extra));
        }
    }
    if t & 1 != 0 {
        arr.set(skey(b"message"), sval(&n.relname));
        arr.set(skey(b"pid"), Value::Long(n.be_pid as i64));
        if v9 {
            arr.set(skey(b"payload"), sval(&n.extra));
        }
    }
    arr_value(arr)
}

pub fn pg_get_pid(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((_, c)) = one_link(vm, f, args) else {
        return Value::Null;
    };
    Value::Long(c.borrow().backend_pid() as i64)
}

pub fn pg_socket(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((_, c)) = one_link(vm, f, args) else {
        return Value::Null;
    };
    let fd = c.borrow().socket();
    if fd < 0 {
        return Value::False;
    }
    let Some(hooks) = vm.socket_stream_hooks else {
        return Value::False;
    };
    // a stream of its own over a duplicate of the socket (closing it
    // leaves the connection alone)
    let dup = unsafe { libc::dup(fd) };
    if dup < 0 {
        return Value::False;
    }
    let id = (hooks.stream_from_socket)(vm, dup);
    Value::resource(id)
}

pub fn pg_consume_input(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((_, c)) = one_link(vm, f, args) else {
        return Value::Null;
    };
    let r = c.borrow_mut().consume_input();
    bval(r)
}

pub fn pg_flush(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Some((_, c)) = one_link(vm, f, args) else {
        return Value::Null;
    };
    let nb = c.borrow().is_nonblocking();
    if !nb && c.borrow_mut().set_nonblocking(true) == -1 {
        notice(vm, "Cannot set connection to nonblocking mode");
        return Value::False;
    }
    let ret = c.borrow_mut().flush();
    if !nb && c.borrow_mut().set_nonblocking(false) == -1 {
        notice(vm, "Failed resetting connection to blocking mode");
    }
    match ret {
        0 => Value::True,
        1 => Value::Long(0),
        _ => Value::False,
    }
}

pub fn pg_change_password(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Css", args) else {
        return Value::Null;
    };
    if a[1].str().is_empty() {
        zpp::must_not_be_empty(vm, f, 2);
        return Value::Null;
    }
    if a[2].str().is_empty() {
        zpp::must_not_be_empty(vm, f, 3);
        return Value::Null;
    }
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let (user, pw) = (a[1].str().to_vec(), a[2].str().to_vec());
    let mut cc = c.borrow_mut();
    let Some(enc) = crate::libpq::auth::encrypt_password_conn(&mut cc, &pw, &user, None) else {
        return Value::False;
    };
    let Some(fmtpw) = cc.escape_literal(enc.as_bytes()) else {
        return Value::False;
    };
    let Some(fmtuser) = cc.escape_identifier(&user) else {
        return Value::False;
    };
    let mut q = b"ALTER USER ".to_vec();
    q.extend_from_slice(&fmtuser);
    q.extend_from_slice(b" PASSWORD ");
    q.extend_from_slice(&fmtpw);
    let r = cc.exec(&q);
    bval(matches!(r, Some(r) if r.status == ExecStatus::CommandOk))
}

pub fn pg_socket_poll(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "rll|l", args) else {
        return Value::Null;
    };
    let Arg::Res(id) = a[0] else {
        return Value::Null;
    };
    let Some(hooks) = vm.socket_stream_hooks else {
        return Value::Null;
    };
    let v = Value::resource(id);
    // php_stream_cast(..., 0): no error reporting
    let saved = vm.warning_capture.replace(Vec::new());
    let r = (hooks.stream_socket_fd)(vm, &v, f);
    vm.warning_capture = saved;
    let fd = match r {
        Ok(Some(fd)) => fd,
        Ok(None) => {
            vm.current_exception = None;
            zpp::type_error(vm, f, 1, "invalid resource socket");
            return Value::Null;
        }
        Err(_) => return Value::Null,
    };
    let (read, write) = (a[1].long_or(0), a[2].long_or(0));
    let ts = a[3].long_or(-1);
    if read == 0 && write == 0 {
        return Value::Long(0);
    }
    let ms = if ts != -1 {
        let now = unsafe { libc::time(std::ptr::null_mut()) } as i64;
        if ts > now { (ts - now) * 1000 } else { 0 }
    } else {
        -1
    };
    let r = crate::libpq::conn::poll_fd(fd, read != 0, write != 0, ms as i32);
    Value::Long(if r > 0 { 1 } else { r as i64 })
}

pub fn pg_set_chunked_rows_size(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cl", args) else {
        return Value::Null;
    };
    let size = a[1].long_or(0);
    if !(1..=i32::MAX as i64).contains(&size) {
        value_error(vm, f, 2, &format!("must be between 1 and {}", i32::MAX));
        return Value::Null;
    }
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let r = c.borrow_mut().set_chunked_rows_mode(size as i32);
    bval(r)
}

pub fn pg_close_stmt(vm: &mut Vm, f: &'static str, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "Cp", args) else {
        return Value::Null;
    };
    if a[1].str().is_empty() {
        zpp::must_not_be_empty(vm, f, 2);
        return Value::Null;
    }
    let Some((_, c)) = link_conn(vm, &a[0].obj().unwrap()) else {
        return Value::Null;
    };
    let name = a[1].str().to_vec();
    let r = c.borrow_mut().close_prepared(&name);
    match r {
        Some(r) if r.status == ExecStatus::CommandOk => new_result(vm, &c, r),
        _ => Value::False,
    }
}

/// PHP_RSHUTDOWN(pgsql): release the default link and roll back the
/// transactions of persistent connections.
pub fn rshutdown(_vm: &mut Vm) {
    clear_default_link();
    let pers: Vec<ConnRef> = PGG.with(|g| g.borrow().persistent.values().cloned().collect());
    for c in pers {
        let mut c = c.borrow_mut();
        let _ = c.set_nonblocking(false);
        while c.get_result().is_some() {}
        if c.transaction_status() != TransStatus::Idle && c.status == ConnStatus::Ok {
            c.exec(b"ROLLBACK;");
        }
    }
    let _ = Obj::clone;
}
