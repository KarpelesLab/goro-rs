//! ext/pdo_pgsql: the PostgreSQL PDO driver (pdo_pgsql.c, pgsql_driver.c)
//! and Pdo\Pgsql, over goro-ext-pgsql's libpq port.

pub mod driver;
pub mod statement;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_mysqli::zpp::{Arg, Func, parse};
use goro_ext_pgsql::libpq::exec::CopyData;
use goro_ext_pgsql::libpq::lo::{INV_READ, INV_WRITE};
use goro_ext_pgsql::libpq::{ExecStatus, Oid};

use crate::core::*;
use driver::*;

pub const PDO_PGSQL_ATTR_DISABLE_PREPARES: i64 = PDO_ATTR_DRIVER_SPECIFIC;
pub const PDO_PGSQL_ATTR_RESULT_MEMORY_SIZE: i64 = PDO_ATTR_DRIVER_SPECIFIC + 1;

/// Pdo\Pgsql's constants in declaration order: (name, value, deprecation)
pub const CONSTANTS: &[(&str, i64, Option<&str>)] = &[
    (
        "ATTR_DISABLE_PREPARES",
        PDO_PGSQL_ATTR_DISABLE_PREPARES,
        None,
    ),
    (
        "ATTR_RESULT_MEMORY_SIZE",
        PDO_PGSQL_ATTR_RESULT_MEMORY_SIZE,
        None,
    ),
    ("TRANSACTION_IDLE", 0, Some("as it has no effect")),
    ("TRANSACTION_ACTIVE", 1, Some("as it has no effect")),
    ("TRANSACTION_INTRANS", 2, Some("as it has no effect")),
    ("TRANSACTION_INERROR", 3, Some("as it has no effect")),
    ("TRANSACTION_UNKNOWN", 4, Some("as it has no effect")),
];

/// The PDO:: constants pdo_pgsql's MINIT adds (all deprecated)
pub fn pdo_constants() -> Vec<(Vec<u8>, Value, Option<String>)> {
    let mut v = vec![(
        b"PGSQL_ATTR_DISABLE_PREPARES".to_vec(),
        Value::Long(PDO_PGSQL_ATTR_DISABLE_PREPARES),
        Some(
            "Constant PDO::PGSQL_ATTR_DISABLE_PREPARES is deprecated since 8.5, use Pdo\\Pgsql::ATTR_DISABLE_PREPARES instead"
                .to_string(),
        ),
    )];
    for (i, n) in ["IDLE", "ACTIVE", "INTRANS", "INERROR", "UNKNOWN"]
        .iter()
        .enumerate()
    {
        let name = format!("PGSQL_TRANSACTION_{}", n);
        let msg = format!("Constant PDO::{} is deprecated since 8.5", name);
        v.push((name.into_bytes(), Value::Long(i as i64), Some(msg)));
    }
    v
}

/// Pdo\Pgsql's class constants for Vm::ext_class_constants
pub fn class_constants() -> Vec<(Vec<u8>, Value, Option<String>)> {
    CONSTANTS
        .iter()
        .map(|(n, v, d)| {
            (
                n.as_bytes().to_vec(),
                Value::Long(*v),
                d.map(|d| format!("Constant Pdo\\Pgsql::{} is deprecated since 8.5, {}", n, d)),
            )
        })
        .collect()
}

fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}

// ── notices, deferred stream closes ───────────────────────────────────

thread_local! {
    /// The notices libpq passed to the notice processor: (handle id, message)
    static NOTICES: RefCell<Vec<(usize, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
    /// Large object streams to close (pdo_pgsql_close_lob_streams)
    static PENDING_CLOSE: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
    /// Notice processor argument -> handle
    static HANDLES: RefCell<HashMap<usize, Weak<RefCell<Dbh>>>> = RefCell::new(HashMap::new());
    /// The open large object streams: resource id -> oid
    static LOB_OIDS: RefCell<HashMap<i64, Oid>> = RefCell::new(HashMap::new());
}

/// _pdo_pgsql_notice (queued; the callback runs from flush())
pub fn notice_processor(arg: usize, msg: &[u8]) {
    NOTICES.with(|n| n.borrow_mut().push((arg, msg.to_vec())));
}

pub fn register_handle(id: usize, dbh: &DbhRef) {
    HANDLES.with(|h| h.borrow_mut().insert(id, Rc::downgrade(dbh)));
}

pub fn unregister_handle(id: usize) {
    HANDLES.with(|h| h.borrow_mut().remove(&id));
}

pub fn queue_stream_close(ids: Vec<i64>) {
    if !ids.is_empty() {
        PENDING_CLOSE.with(|p| p.borrow_mut().extend(ids));
    }
}

/// The oid of a pdo_pgsql large object stream (php_stream_is())
pub fn lob_stream_oid(id: i64) -> Option<Oid> {
    LOB_OIDS.with(|m| m.borrow().get(&id).copied())
}

/// Run what libpq calls queued: the notice callbacks and the large object
/// stream closes.
pub fn flush(vm: &mut Vm) {
    let closes = PENDING_CLOSE.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for id in closes.into_iter().rev() {
        if let Some(h) = vm.socket_stream_hooks {
            (h.close_stream)(vm, id);
        }
    }
    loop {
        let n = NOTICES.with(|n| {
            let mut n = n.borrow_mut();
            if n.is_empty() {
                None
            } else {
                Some(n.remove(0))
            }
        });
        let Some((id, msg)) = n else {
            break;
        };
        let dbh = HANDLES.with(|h| h.borrow().get(&id).and_then(|w| w.upgrade()));
        let Some(dbh) = dbh else {
            continue;
        };
        let cb = with_h(&dbh, |h| h.notice_callback.clone()).flatten();
        if let Some(cb) = cb {
            if vm.current_exception.is_some() {
                continue;
            }
            let _ = vm.call_callback(&cb, &[vstr(&msg)]);
        }
    }
}

// ── large object streams ──────────────────────────────────────────────

/// pdo_pgsql_lob_self
struct LobStream {
    conn: ConnRef,
    lfd: i32,
    dbh_obj: Option<Obj>,
    id: Rc<Cell<i64>>,
}

impl goro_core::ext_stream::ExtStream for LobStream {
    fn read(&mut self, _vm: &mut Vm, count: usize, eof: &mut bool) -> Option<Vec<u8>> {
        let r = self.conn.borrow_mut().lo_read(self.lfd, count)?;
        if r.is_empty() {
            *eof = true;
        }
        Some(r)
    }
    fn write(&mut self, _vm: &mut Vm, buf: &[u8]) -> i64 {
        self.conn.borrow_mut().lo_write(self.lfd, buf) as i64
    }
    fn close(&mut self, vm: &mut Vm) {
        let _ = self.conn.borrow_mut().lo_close(self.lfd);
        let id = self.id.get();
        LOB_OIDS.with(|m| m.borrow_mut().remove(&id));
        if let Some(o) = self.dbh_obj.take() {
            if let Some(dbh) = crate::objects::dbh_of(&o) {
                with_h(&dbh, |h| h.lob_streams.retain(|x| *x != id));
            }
        }
        flush(vm);
    }
    fn seek(&mut self, _vm: &mut Vm, offset: i64, whence: i64) -> Option<i64> {
        let pos = self
            .conn
            .borrow_mut()
            .lo_lseek64(self.lfd, offset, whence as i32);
        if pos >= 0 { Some(pos) } else { None }
    }
    fn label(&self) -> &'static str {
        "pdo_pgsql lob stream"
    }
}

/// pdo_pgsql_create_lob_stream
pub fn create_lob_stream(vm: &mut Vm, dbh_obj: &Obj, lfd: i32, oid: Oid) -> Option<i64> {
    let dbh = crate::objects::dbh_of(dbh_obj)?;
    let conn = server(&dbh)?;
    let idc = Rc::new(Cell::new(0));
    let s = LobStream {
        conn,
        lfd,
        dbh_obj: Some(dbh_obj.clone()),
        id: idc.clone(),
    };
    let id = goro_core::ext_stream::alloc(vm, Rc::new(RefCell::new(s)), "r+b")?;
    idc.set(id);
    LOB_OIDS.with(|m| m.borrow_mut().insert(id, oid));
    with_h(&dbh, |h| h.lob_streams.push(id));
    Some(id)
}

// ── Pdo\Pgsql methods and the PDO::pgsql* driver methods ─────────────

/// PDO_CONSTRUCT_CHECK + the PostgreSQL handle
fn this_dbh(vm: &mut Vm, f: &Func) -> Option<(Obj, DbhRef)> {
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
    Some((o, dbh))
}

fn conn_of(dbh: &DbhRef) -> Option<ConnRef> {
    server(dbh)
}

/// The copy query: COPY table [(fields)] FROM STDIN / TO STDIN ...
fn copy_query(
    table: &[u8],
    fields: Option<&[u8]>,
    dir: &str,
    delim: &[u8],
    null_as: &[u8],
) -> Vec<u8> {
    let cut = |s: &[u8]| -> Vec<u8> {
        match s.iter().position(|&c| c == 0) {
            Some(p) => s[..p].to_vec(),
            None => s.to_vec(),
        }
    };
    let mut q = b"COPY ".to_vec();
    q.extend_from_slice(&cut(table));
    if let Some(f) = fields {
        q.extend_from_slice(b" (");
        q.extend_from_slice(&cut(f));
        q.push(b')');
    }
    q.push(b' ');
    q.extend_from_slice(dir.as_bytes());
    q.extend_from_slice(b" WITH DELIMITER E'");
    q.push(if delim.is_empty() { b'\t' } else { delim[0] });
    q.extend_from_slice(b"' NULL AS E'");
    if null_as.is_empty() {
        q.extend_from_slice(b"\\\\N");
    } else {
        q.extend_from_slice(&cut(null_as));
    }
    q.push(b'\'');
    q
}

fn drain_results(conn: &ConnRef) {
    let mut c = conn.borrow_mut();
    while c.get_result().is_some() {}
}

fn copy_end(vm: &mut Vm, dbh: &DbhRef, conn: &ConnRef) -> Value {
    if conn.borrow_mut().put_copy_end(None) != 1 {
        pgsql_error(Some(vm), dbh, None, PGRES_FATAL_ERROR, None, None);
        handle_dbh_err(vm, dbh);
        return Value::False;
    }
    let mut failed = false;
    loop {
        let r = conn.borrow_mut().get_result();
        let Some(r) = r else {
            break;
        };
        if r.status != ExecStatus::CommandOk {
            let st = sqlstate(Some(&r));
            pgsql_error(Some(vm), dbh, None, PGRES_FATAL_ERROR, st.as_deref(), None);
            failed = true;
        }
    }
    handle_dbh_err(vm, dbh);
    vbool(!failed)
}

fn copy_start_failed(
    vm: &mut Vm,
    dbh: &DbhRef,
    r: Option<&goro_ext_pgsql::libpq::PgResult>,
) -> Value {
    let st = sqlstate(r);
    pgsql_error(Some(vm), dbh, None, PGRES_FATAL_ERROR, st.as_deref(), None);
    handle_dbh_err(vm, dbh);
    Value::False
}

fn opt_str(a: &Arg) -> Option<Vec<u8>> {
    a.str().map(|s| s.to_vec())
}

/// 'p': no NUL bytes
fn check_path(vm: &mut Vm, f: &Func, n: usize, s: &[u8]) -> bool {
    if s.contains(&0) {
        f.value_error(vm, n, "must not contain any null bytes");
        return false;
    }
    true
}

/// pgsqlCopyFromArray_internal
pub fn copy_from_array(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "sz|sss!", &[], args) else {
        return Value::Null;
    };
    let rows = match &a[1] {
        Arg::Any(v) => v.deref(),
        _ => Value::Null,
    };
    let rows_list: Vec<Value> = match &rows {
        Value::Array(arr) => arr.borrow().iter().map(|(_, v)| v.clone()).collect(),
        Value::Generator(_) => Vec::new(),
        Value::Object(o) => {
            let cn = o.borrow().class_name.to_ascii_lowercase();
            let iterable = cn == b"generator"
                || instanceof(vm, &cn, b"traversable")
                || vm.class_implements_interface(&cn, b"traversable")
                || match vm.functions.get(b"is_iterable".as_slice()).copied() {
                    Some(f) => matches!(f(vm, std::slice::from_ref(&rows)), Ok(Value::True)),
                    None => false,
                };
            if !iterable {
                f.type_error(vm, 2, "Traversable|array", &rows);
                return Value::Null;
            }
            Vec::new()
        }
        other => {
            f.type_error(vm, 2, "Traversable|array", other);
            return Value::Null;
        }
    };
    let Some((_, dbh)) = this_dbh(vm, f) else {
        return Value::Null;
    };
    dbh_clear_err(&dbh);
    let Some(conn) = conn_of(&dbh) else {
        return Value::False;
    };
    let table = a[0].str().unwrap_or(b"").to_vec();
    let delim = opt_str(&a[2]).unwrap_or_default();
    let null_as = opt_str(&a[3]).unwrap_or_default();
    let fields = opt_str(&a[4]);
    let q = copy_query(&table, fields.as_deref(), "FROM STDIN", &delim, &null_as);
    drain_results(&conn);
    let r = conn.borrow_mut().exec(&q);
    flush(vm);
    if status_of(r.as_ref()) != ExecStatus::CopyIn {
        return copy_start_failed(vm, &dbh, r.as_ref());
    }
    let rows_list = if let Value::Object(_) | Value::Generator(_) = &rows {
        let it = vm.functions.get(b"iterator_to_array".as_slice()).copied();
        let Some(it) = it else {
            return Value::False;
        };
        match it(vm, &[rows.clone(), Value::False]) {
            Ok(Value::Array(arr)) => arr.borrow().iter().map(|(_, v)| v.clone()).collect(),
            _ => return Value::Null,
        }
    } else {
        rows_list
    };
    if vm.current_exception.is_some() {
        return Value::Null;
    }
    for v in rows_list {
        let line = match vm.value_to_string_checked(&v.deref()) {
            Ok(s) => s.as_bytes().to_vec(),
            Err(_) => {
                pgsql_error(Some(vm), &dbh, None, PGRES_FATAL_ERROR, None, None);
                handle_dbh_err(vm, &dbh);
                return Value::False;
            }
        };
        if vm.current_exception.is_some() {
            return Value::False;
        }
        let mut line = line;
        if !line.is_empty() && *line.last().unwrap() != b'\n' {
            line.push(b'\n');
        }
        if conn.borrow_mut().put_copy_data(&line) != 1 {
            pgsql_error(Some(vm), &dbh, None, PGRES_FATAL_ERROR, None, None);
            handle_dbh_err(vm, &dbh);
            return Value::False;
        }
    }
    let r = copy_end(vm, &dbh, &conn);
    flush(vm);
    r
}

/// pgsqlCopyFromFile_internal
pub fn copy_from_file(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "ss|sss!", &[], args) else {
        return Value::Null;
    };
    let filename = a[1].str().unwrap_or(b"").to_vec();
    if !check_path(vm, f, 2, &filename) {
        return Value::Null;
    }
    let Some((_, dbh)) = this_dbh(vm, f) else {
        return Value::Null;
    };
    dbh_clear_err(&dbh);
    let Some(conn) = conn_of(&dbh) else {
        return Value::False;
    };
    let path = String::from_utf8_lossy(&filename).into_owned();
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(_) => {
            pgsql_error(
                Some(vm),
                &dbh,
                None,
                PGRES_FATAL_ERROR,
                None,
                Some(b"Unable to open the file"),
            );
            handle_dbh_err(vm, &dbh);
            return Value::False;
        }
    };
    let table = a[0].str().unwrap_or(b"").to_vec();
    let delim = opt_str(&a[2]).unwrap_or_default();
    let null_as = opt_str(&a[3]).unwrap_or_default();
    let fields = opt_str(&a[4]);
    let q = copy_query(&table, fields.as_deref(), "FROM STDIN", &delim, &null_as);
    drain_results(&conn);
    let r = conn.borrow_mut().exec(&q);
    flush(vm);
    if status_of(r.as_ref()) != ExecStatus::CopyIn {
        return copy_start_failed(vm, &dbh, r.as_ref());
    }
    for line in data.split_inclusive(|&c| c == b'\n') {
        if conn.borrow_mut().put_copy_data(line) != 1 {
            pgsql_error(Some(vm), &dbh, None, PGRES_FATAL_ERROR, None, None);
            handle_dbh_err(vm, &dbh);
            return Value::False;
        }
    }
    let r = copy_end(vm, &dbh, &conn);
    flush(vm);
    r
}

/// The rows of a COPY ... TO STDIN: Err after an error was raised
fn copy_out(vm: &mut Vm, f: &Func, a: &[Arg], first: usize) -> Result<Vec<Vec<u8>>, Value> {
    let Some((_, dbh)) = this_dbh(vm, f) else {
        return Err(Value::Null);
    };
    dbh_clear_err(&dbh);
    let Some(conn) = conn_of(&dbh) else {
        return Err(Value::False);
    };
    drain_results(&conn);
    let table = a[0].str().unwrap_or(b"").to_vec();
    let delim = opt_str(&a[first]).unwrap_or_default();
    let null_as = opt_str(&a[first + 1]).unwrap_or_default();
    let fields = opt_str(&a[first + 2]);
    let q = copy_query(&table, fields.as_deref(), "TO STDIN", &delim, &null_as);
    let r = conn.borrow_mut().exec(&q);
    flush(vm);
    if status_of(r.as_ref()) != ExecStatus::CopyOut {
        return Err(copy_start_failed(vm, &dbh, r.as_ref()));
    }
    let mut rows = Vec::new();
    loop {
        let d = conn.borrow_mut().get_copy_data(false);
        match d {
            CopyData::Row(r) => rows.push(r),
            CopyData::Done => break,
            _ => {
                pgsql_error(Some(vm), &dbh, None, PGRES_FATAL_ERROR, None, None);
                handle_dbh_err(vm, &dbh);
                return Err(Value::False);
            }
        }
    }
    drain_results(&conn);
    flush(vm);
    Ok(rows)
}

/// pgsqlCopyToArray_internal
pub fn copy_to_array(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "s|sss!", &[], args) else {
        return Value::Null;
    };
    match copy_out(vm, f, &a, 1) {
        Ok(rows) => {
            let mut arr = PhpArray::new();
            for r in rows {
                arr.push(vstr(r));
            }
            Value::Array(Rc::new(RefCell::new(arr)))
        }
        Err(v) => v,
    }
}

/// pgsqlCopyToFile_internal
pub fn copy_to_file(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "ss|sss!", &[], args) else {
        return Value::Null;
    };
    let filename = a[1].str().unwrap_or(b"").to_vec();
    if !check_path(vm, f, 2, &filename) {
        return Value::Null;
    }
    let Some((_, dbh)) = this_dbh(vm, f) else {
        return Value::Null;
    };
    dbh_clear_err(&dbh);
    let path = String::from_utf8_lossy(&filename).into_owned();
    let mut file = match std::fs::File::create(&path) {
        Ok(f) => f,
        Err(_) => {
            pgsql_error(
                Some(vm),
                &dbh,
                None,
                PGRES_FATAL_ERROR,
                None,
                Some(b"Unable to open the file for writing"),
            );
            handle_dbh_err(vm, &dbh);
            return Value::False;
        }
    };
    let a2 = [a[0].clone(), a[2].clone(), a[3].clone(), a[4].clone()];
    match copy_out(vm, f, &a2, 1) {
        Ok(rows) => {
            use std::io::Write;
            for r in rows {
                if file.write_all(&r).is_err() {
                    pgsql_error(
                        Some(vm),
                        &dbh,
                        None,
                        PGRES_FATAL_ERROR,
                        None,
                        Some(b"Unable to write to file"),
                    );
                    handle_dbh_err(vm, &dbh);
                    return Value::False;
                }
            }
            Value::True
        }
        Err(v) => v,
    }
}

/// pgsqlLOBCreate_internal
pub fn lob_create(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let Some((_, dbh)) = this_dbh(vm, f) else {
        return Value::Null;
    };
    dbh_clear_err(&dbh);
    let Some(conn) = conn_of(&dbh) else {
        return Value::False;
    };
    let oid = conn.borrow_mut().lo_creat(INV_READ | INV_WRITE);
    flush(vm);
    if oid != 0 {
        return vstr(oid.to_string());
    }
    pgsql_error(Some(vm), &dbh, None, PGRES_FATAL_ERROR, None, None);
    handle_dbh_err(vm, &dbh);
    Value::False
}

/// strtoul(): (value, ok) — ok false on ERANGE/EINVAL with a 0 result
fn strtoul(s: &[u8]) -> Option<Oid> {
    let t = String::from_utf8_lossy(s);
    let t = t.trim_start();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let d: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
    if d.is_empty() {
        // no conversion: 0 (EINVAL is only reported for a bad base)
        return Some(0);
    }
    match d.parse::<u64>() {
        Ok(v) => {
            let v = if neg { v.wrapping_neg() } else { v };
            Some(v as u32)
        }
        Err(_) => None,
    }
}

/// pgsqlLOBOpen_internal
pub fn lob_open(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "s|s", &[], args) else {
        return Value::Null;
    };
    let Some(oid) = strtoul(a[0].str().unwrap_or(b"")) else {
        return Value::False;
    };
    let mode = a[1].str().unwrap_or(b"rb").to_vec();
    let m = if mode.contains(&b'+') || mode.contains(&b'w') {
        INV_READ | INV_WRITE
    } else {
        INV_READ
    };
    let Some((o, dbh)) = this_dbh(vm, f) else {
        return Value::Null;
    };
    dbh_clear_err(&dbh);
    let Some(conn) = conn_of(&dbh) else {
        return Value::False;
    };
    let lfd = conn.borrow_mut().lo_open(oid, m);
    flush(vm);
    if lfd >= 0 {
        if let Some(id) = create_lob_stream(vm, &o, lfd, oid) {
            return Value::resource(id);
        }
    } else {
        pgsql_error(Some(vm), &dbh, None, PGRES_FATAL_ERROR, None, None);
    }
    handle_dbh_err(vm, &dbh);
    Value::False
}

/// pgsqlLOBUnlink_internal
pub fn lob_unlink(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "s", &[], args) else {
        return Value::Null;
    };
    let Some(oid) = strtoul(a[0].str().unwrap_or(b"")) else {
        return Value::False;
    };
    let Some((_, dbh)) = this_dbh(vm, f) else {
        return Value::Null;
    };
    dbh_clear_err(&dbh);
    let Some(conn) = conn_of(&dbh) else {
        return Value::False;
    };
    let r = conn.borrow_mut().lo_unlink(oid);
    flush(vm);
    if r == 1 {
        return Value::True;
    }
    pgsql_error(Some(vm), &dbh, None, PGRES_FATAL_ERROR, None, None);
    handle_dbh_err(vm, &dbh);
    Value::False
}

/// pgsqlGetNotify_internal
pub fn get_notify(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "|ll", &[], args) else {
        return Value::Null;
    };
    let Some((_, dbh)) = this_dbh(vm, f) else {
        return Value::Null;
    };
    let mut result_type = a[0].long_or(PDO_FETCH_USE_DEFAULT);
    let mut ms_timeout = a[1].long_or(0);
    if result_type == PDO_FETCH_USE_DEFAULT {
        result_type = dbh.borrow().default_fetch_type as i64;
    }
    if result_type != PDO_FETCH_BOTH
        && result_type != PDO_FETCH_ASSOC
        && result_type != PDO_FETCH_NUM
    {
        f.value_error(
            vm,
            1,
            "must be one of PDO::FETCH_BOTH, PDO::FETCH_ASSOC, or PDO::FETCH_NUM",
        );
        return Value::Null;
    }
    if ms_timeout < 0 {
        f.value_error(vm, 2, "must be greater than or equal to 0");
        return Value::Null;
    } else if ms_timeout > i32::MAX as i64 {
        docref_warning(vm, &format!("Timeout was shrunk to {}", i32::MAX));
        ms_timeout = i32::MAX as i64;
    }
    let Some(conn) = conn_of(&dbh) else {
        return Value::False;
    };
    if !conn.borrow_mut().consume_input() {
        pgsql_error(Some(vm), &dbh, None, PGRES_FATAL_ERROR, None, None);
        handle_dbh_err(vm, &dbh);
        return Value::False;
    }
    let mut n = conn.borrow_mut().notifies();
    if ms_timeout != 0 && n.is_none() {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(ms_timeout as u64);
        let _ = conn.borrow_mut().wait_timed(true, false, Some(deadline));
        if !conn.borrow_mut().consume_input() {
            pgsql_error(Some(vm), &dbh, None, PGRES_FATAL_ERROR, None, None);
            handle_dbh_err(vm, &dbh);
            return Value::False;
        }
        n = conn.borrow_mut().notifies();
    }
    flush(vm);
    let Some(n) = n else {
        return Value::False;
    };
    let mut arr = PhpArray::new();
    let k = |s: &str| ArrayKey::String(PhpString::from_bytes(s.as_bytes()));
    if result_type == PDO_FETCH_NUM || result_type == PDO_FETCH_BOTH {
        arr.set(ArrayKey::Int(0), vstr(&n.relname));
        arr.set(ArrayKey::Int(1), Value::Long(n.be_pid as i64));
        if !n.extra.is_empty() {
            arr.set(ArrayKey::Int(2), vstr(&n.extra));
        }
    }
    if result_type == PDO_FETCH_ASSOC || result_type == PDO_FETCH_BOTH {
        arr.set(k("message"), vstr(&n.relname));
        arr.set(k("pid"), Value::Long(n.be_pid as i64));
        if !n.extra.is_empty() {
            arr.set(k("payload"), vstr(&n.extra));
        }
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}

/// pgsqlGetPid_internal
pub fn get_pid(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let Some((_, dbh)) = this_dbh(vm, f) else {
        return Value::Null;
    };
    match conn_of(&dbh) {
        Some(c) => Value::Long(c.borrow().backend_pid() as i64),
        None => Value::Long(0),
    }
}

/// Pdo\Pgsql::escapeIdentifier
pub fn escape_identifier(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "s", &[], args) else {
        return Value::Null;
    };
    let Some((_, dbh)) = this_dbh(vm, f) else {
        return Value::Null;
    };
    dbh_clear_err(&dbh);
    let Some(conn) = conn_of(&dbh) else {
        throw(
            vm,
            b"Error",
            "PostgreSQL connection has already been closed",
        );
        return Value::Null;
    };
    let r = conn
        .borrow_mut()
        .escape_identifier(a[0].str().unwrap_or(b""));
    match r {
        Some(s) => vstr(s),
        None => {
            pgsql_error(Some(vm), &dbh, None, PGRES_FATAL_ERROR, None, None);
            handle_dbh_err(vm, &dbh);
            Value::Null
        }
    }
}

/// Pdo\Pgsql::setNoticeCallback
pub fn set_notice_callback(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let v = args.first().map(|v| v.deref()).unwrap_or(Value::Undef);
    if matches!(v, Value::Undef) {
        let m = format!("{}() expects exactly 1 argument, 0 given", f.name);
        throw(vm, b"ArgumentCountError", &m);
        return Value::Null;
    }
    let cb = if matches!(v, Value::Null) {
        None
    } else {
        let r = vm.zend_is_callable_ex(&v, 0);
        if vm.current_exception.is_some() {
            return Value::Null;
        }
        if !r.ok {
            let m = format!(
                "{} must be a valid callback or null, {}",
                f.arg_prefix(1),
                r.error.unwrap_or_default()
            );
            throw(vm, b"TypeError", &m);
            return Value::Null;
        }
        Some(v)
    };
    let Some((_, dbh)) = this_dbh(vm, f) else {
        return Value::Null;
    };
    with_h(&dbh, |h| h.notice_callback = cb);
    Value::Null
}

type Impl = fn(&mut Vm, &Func, &[Value]) -> Value;

/// The Pdo\Pgsql implementations of the shared methods
pub fn class_method(name: &str) -> Option<Impl> {
    Some(match name {
        "copyFromArray" => copy_from_array,
        "copyFromFile" => copy_from_file,
        "copyToArray" => copy_to_array,
        "copyToFile" => copy_to_file,
        "lobCreate" => lob_create,
        "lobOpen" => lob_open,
        "lobUnlink" => lob_unlink,
        "getNotify" => get_notify,
        "getPid" => get_pid,
        _ => return None,
    })
}

/// PDO_PGSql_Ext: the deprecated PDO::pgsql* driver methods
pub const EXT_METHODS: &[(&str, &str)] = &[
    ("pgsqlCopyFromArray", "copyFromArray"),
    ("pgsqlCopyFromFile", "copyFromFile"),
    ("pgsqlCopyToArray", "copyToArray"),
    ("pgsqlCopyToFile", "copyToFile"),
    ("pgsqlLOBCreate", "lobCreate"),
    ("pgsqlLOBOpen", "lobOpen"),
    ("pgsqlLOBUnlink", "lobUnlink"),
    ("pgsqlGetNotify", "getNotify"),
    ("pgsqlGetPid", "getPid"),
];

/// Whether `this` is a PDO handle of the pgsql driver
fn is_pgsql_handle(this: Option<&Obj>) -> bool {
    let Some(o) = this else {
        return false;
    };
    let Some(dbh) = crate::objects::dbh_of(o) else {
        return false;
    };
    dbh.borrow().driver.is_some_and(|d| d.name == "pgsql")
}

/// dbh_method_get + the call of a PDO::pgsql* method. None: no such method
/// for this handle.
pub fn call_ext_method(
    vm: &mut Vm,
    method_lower: &[u8],
    this: Option<&Obj>,
    args: &[Value],
) -> Option<Value> {
    let (old, new) = EXT_METHODS
        .iter()
        .find(|(o, _)| o.as_bytes().eq_ignore_ascii_case(method_lower))?;
    if !is_pgsql_handle(this) {
        return None;
    }
    let msg = format!(
        "Method PDO::{}() is deprecated since 8.5, use Pdo\\Pgsql::{}() instead",
        old, new
    );
    if vm.emit_method_deprecation(&msg, old) {
        return Some(Value::Null);
    }
    let params = goro_core::stubs::find_method(b"PDO_PGSql_Ext", old.as_bytes())
        .map(|(_, m)| m.args.iter().map(|a| a.name).collect())
        .unwrap_or_default();
    let f = Func {
        name: format!("PDO::{}", old),
        is_method: true,
        this: this.cloned(),
        params,
    };
    let imp = class_method(new)?;
    push_active(&f.name);
    let frame = format!("PDO->{}", old);
    let line = vm.current_line;
    let file = vm.current_file.clone();
    vm.call_stack.push((frame, file, line, args.to_vec(), true));
    let r = imp(vm, &f, args);
    vm.call_stack.pop();
    pop_active();
    Some(r)
}

/// PHP_MINFO(pdo_pgsql)
pub fn minfo(_vm: &mut Vm) -> String {
    format!(
        "\npdo_pgsql\n\nPDO Driver for PostgreSQL => enabled\nPostgreSQL(libpq) Version => {}\n",
        libpq_version()
    )
}

/// PHP_MINIT(pdo_pgsql)
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"pdo_pgsql");
    register_driver(&PGSQL_DRIVER);
    register_driver_specific_ce(&PGSQL_DRIVER, b"pdo\\pgsql");
    vm.phpinfo_hooks.push(minfo);
}
