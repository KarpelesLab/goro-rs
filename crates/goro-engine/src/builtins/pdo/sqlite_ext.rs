//! The methods of `Pdo\Sqlite` (PHP manual: Pdo\Sqlite) and the
//! deprecated `PDO::sqliteCreate*()` aliases: user functions,
//! aggregates, collations, the authorizer, blob streams and extension
//! loading.

use std::cell::RefCell;
use std::rc::Rc;

use goro_ext_sqlite::graphitesql::Value as SqlValue;
use goro_ext_sqlite::scan::quote_ident;
use goro_runtime::{PhpStr, Value};

use super::core::{driver_of, pdo_exception};
use super::sqlite::{Conn, SqliteDriver};
use crate::builtins::file::{Backing, Stream, new_stream_res};
use crate::builtins::sqlite_common as sc;
use crate::builtins::streams::NativeStream;
use crate::func::Args;
use crate::vm::{R, Vm};

/// The SQLite connection of a PDO object connected to SQLite.
fn conn_of(vm: &mut Vm, a: &Args) -> R<Rc<RefCell<Conn>>> {
    let o = a.this_obj().clone();
    let d = driver_of(vm, &o)?;
    let mut b = d.borrow_mut();
    match b.as_any().downcast_mut::<SqliteDriver>() {
        Some(s) => Ok(s.conn.clone()),
        None => {
            drop(b);
            vm.throw("Error", "PDO object is not connected to SQLite")
        }
    }
}

/// The PHP 8.5 deprecation of the `PDO::sqliteCreate*()` methods, raised
/// before their callables are checked (tests php_8.5_deprecations.phpt,
/// pdo_sqlite_createaggregate_002.phpt).
fn legacy(vm: &mut Vm, a: &Args, method: &str, new: &str, callables: &[(usize, &str)]) -> R<()> {
    vm.deprecated(&format!(
        "Method PDO::{method}() is deprecated since 8.5, use Pdo\\Sqlite::{new}() instead"
    ))?;
    for &(i, pname) in callables {
        let v = a.val(i);
        if !vm.is_callable_value(&v, false) {
            let why = vm.callable_error(&v);
            return vm.type_error(&format!(
                "PDO::{method}(): Argument #{} (${pname}) must be a valid callback, {why}",
                i + 1
            ));
        }
    }
    Ok(())
}

pub fn legacy_create_function(vm: &mut Vm, a: &mut Args) -> R<Value> {
    legacy(
        vm,
        a,
        "sqliteCreateFunction",
        "createFunction",
        &[(1, "callback")],
    )?;
    create_function(vm, a)
}

pub fn legacy_create_aggregate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    legacy(
        vm,
        a,
        "sqliteCreateAggregate",
        "createAggregate",
        &[(1, "step"), (2, "finalize")],
    )?;
    create_aggregate(vm, a)
}

pub fn legacy_create_collation(vm: &mut Vm, a: &mut Args) -> R<Value> {
    legacy(
        vm,
        a,
        "sqliteCreateCollation",
        "createCollation",
        &[(1, "callback")],
    )?;
    create_collation(vm, a)
}

pub fn create_function(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let conn = conn_of(vm, a)?;
    let name = a.str(0).to_string_lossy();
    let cb = sc::bind_callable(vm, &a.val(1))?;
    let argc = a.int_or(2, -1);
    let f = sc::scalar(&name, cb.clone(), argc);
    let mut c = conn.borrow_mut();
    c.db.create_function(&name, f);
    c.callbacks.push(cb);
    Ok(Value::Bool(true))
}

pub fn create_aggregate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let conn = conn_of(vm, a)?;
    let name = a.str(0).to_string_lossy();
    let step = sc::bind_callable(vm, &a.val(1))?;
    let fin = sc::bind_callable(vm, &a.val(2))?;
    let argc = a.int_or(3, -1);
    let f = sc::aggregate(&name, step.clone(), fin.clone(), argc);
    let mut c = conn.borrow_mut();
    c.db.create_aggregate(&name, f);
    c.callbacks.push(step);
    c.callbacks.push(fin);
    Ok(Value::Bool(true))
}

pub fn create_collation(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let conn = conn_of(vm, a)?;
    let name = a.str(0).to_string_lossy();
    let cb = sc::bind_callable(vm, &a.val(1))?;
    let f = sc::collation(cb.clone());
    let mut c = conn.borrow_mut();
    c.db.create_collation(&name, f);
    c.callbacks.push(cb);
    Ok(Value::Bool(true))
}

/// `loadExtension()`: SQLite extensions are native libraries, which this
/// implementation can't load (the error of a file that isn't one).
pub fn load_extension(vm: &mut Vm, a: &mut Args) -> R<Value> {
    conn_of(vm, a)?;
    let name = a.str(0);
    if name.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($name) cannot be empty");
    }
    Err(pdo_exception(
        vm,
        &format!("Unable to load extension \"{}\"", name.to_string_lossy()),
        None,
        "",
    ))
}

pub fn set_authorizer(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let conn = conn_of(vm, a)?;
    let cb = match a.val(0) {
        Value::Null => Value::Null,
        v => sc::bind_callable(vm, &v)?,
    };
    let mut c = conn.borrow_mut();
    if matches!(cb, Value::Null) {
        c.authorizer = None;
        c.db.set_authorizer(None);
    } else {
        c.authorizer = Some(cb.clone());
        c.db.set_authorizer(Some(sc::authorizer(cb, "Pdo\\Sqlite")));
    }
    Ok(Value::Null)
}

/// A blob opened with `openBlob()`: its bytes, read when opened and
/// written back on each write (the blob can't grow).
struct BlobStream {
    conn: Rc<RefCell<Conn>>,
    db: String,
    table: String,
    column: String,
    rowid: i64,
    data: Vec<u8>,
    pos: usize,
    writable: bool,
}

impl NativeStream for BlobStream {
    fn read(&mut self, _vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.data.len().saturating_sub(self.pos));
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }

    fn write(&mut self, vm: &mut Vm, data: &[u8]) -> std::io::Result<usize> {
        if !self.writable {
            let _ = vm.warn("fwrite(): Can't write to blob stream: is open as read only");
            return Err(std::io::Error::other("read only"));
        }
        if self.pos + data.len() > self.data.len() {
            let _ = vm.warn("fwrite(): It is not possible to increase the size of a BLOB");
            return Err(std::io::Error::other("too big"));
        }
        self.data[self.pos..self.pos + data.len()].copy_from_slice(data);
        self.pos += data.len();
        let sql = format!(
            "UPDATE {}.{} SET {} = ? WHERE rowid = ?",
            quote_ident(&self.db),
            quote_ident(&self.table),
            quote_ident(&self.column)
        );
        let mut c = self.conn.borrow_mut();
        let r = c.db.prepare(&sql).and_then(|(st, _)| match st {
            Some(st) => {
                c.db.run(
                    &st,
                    &[
                        SqlValue::Blob(self.data.clone()),
                        SqlValue::Integer(self.rowid),
                    ],
                )
                .map(|_| ())
            }
            None => Ok(()),
        });
        match r {
            Ok(()) => Ok(data.len()),
            Err(_) => Err(std::io::Error::other("write failed")),
        }
    }

    fn seek(&mut self, _vm: &mut Vm, pos: std::io::SeekFrom) -> Option<std::io::Result<u64>> {
        let len = self.data.len() as i64;
        let p = match pos {
            std::io::SeekFrom::Start(p) => p as i64,
            std::io::SeekFrom::Current(d) => self.pos as i64 + d,
            std::io::SeekFrom::End(d) => len + d,
        };
        if p < 0 || p > len {
            return Some(Err(std::io::Error::other("bad seek")));
        }
        self.pos = p as usize;
        Some(Ok(p as u64))
    }

    fn tell(&mut self) -> Option<u64> {
        Some(self.pos as u64)
    }

    fn eof(&self) -> bool {
        self.pos >= self.data.len()
    }
}

/// `openBlob()`: a stream on one blob (PHP manual: Pdo\Sqlite::openBlob).
pub fn open_blob(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let conn = conn_of(vm, a)?;
    let table = a.str(0).to_string_lossy();
    let column = a.str(1).to_string_lossy();
    let rowid = a.int(2);
    let db = a
        .opt_str(3)
        .map_or_else(|| "main".to_string(), |s| s.to_string_lossy());
    let flags = a.int_or(4, 1);
    let sql = format!(
        "SELECT {} FROM {}.{} WHERE rowid = ?",
        quote_ident(&column),
        quote_ident(&db),
        quote_ident(&table)
    );
    let r = {
        let mut c = conn.borrow_mut();
        match c.db.prepare(&sql) {
            Ok((Some(st), _)) => c.db.run(&st, &[SqlValue::Integer(rowid)]),
            Ok((None, _)) => Ok(Default::default()),
            Err(e) => Err(e),
        }
    };
    let data = match r {
        Ok(rows) => match rows.rows.first().and_then(|r| r.first()) {
            Some(SqlValue::Blob(b)) => b.clone(),
            Some(SqlValue::Text(t)) => t.as_bytes().to_vec(),
            Some(_) | None => {
                return Ok(Value::Bool(false));
            }
        },
        Err(_) => return Ok(Value::Bool(false)),
    };
    let bs = BlobStream {
        conn,
        db,
        table,
        column,
        rowid,
        data,
        pos: 0,
        writable: flags & goro_ext_sqlite::OPEN_READWRITE as i64 != 0,
    };
    let s = Stream::new(Backing::Native(Box::new(bs)), b"r+b", b"", "", "SQLite3");
    Ok(Value::Resource(new_stream_res(s)))
}

#[allow(dead_code)]
fn _unused(_: PhpStr) {}
