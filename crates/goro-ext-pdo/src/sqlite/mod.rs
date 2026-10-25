//! ext/pdo_sqlite: the SQLite PDO driver (pdo_sqlite.c, sqlite_driver.c)
//! and Pdo\Sqlite, over goro-sqlite (a pure-Rust port of SQLite).
//!
//! The connection (`Conn`) is shared by the handle, its statements and its
//! blob streams. SQLite calls back into PHP (user functions, aggregates,
//! collations, the authorizer) while the connection is in use: those
//! callbacks reach the VM through the pointer saved when the engine was
//! entered, and a callback that tries to use the same connection again
//! gets an error instead (goro-sqlite is not re-entrant).

pub mod driver;
pub mod statement;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_mysqli::zpp::{Func, parse};
use goro_sqlite::consts::*;
use goro_sqlite::int::{Sqlite3, StmtId};
use goro_sqlite::mainapi;

use crate::core::*;
use driver::*;

// enum of php_pdo_sqlite_int.h
pub const PDO_SQLITE_ATTR_OPEN_FLAGS: i64 = PDO_ATTR_DRIVER_SPECIFIC;
pub const PDO_SQLITE_ATTR_READONLY_STATEMENT: i64 = PDO_ATTR_DRIVER_SPECIFIC + 1;
pub const PDO_SQLITE_ATTR_EXTENDED_RESULT_CODES: i64 = PDO_ATTR_DRIVER_SPECIFIC + 2;
pub const PDO_SQLITE_ATTR_BUSY_STATEMENT: i64 = PDO_ATTR_DRIVER_SPECIFIC + 3;
pub const PDO_SQLITE_ATTR_EXPLAIN_STATEMENT: i64 = PDO_ATTR_DRIVER_SPECIFIC + 4;
pub const PDO_SQLITE_ATTR_TRANSACTION_MODE: i64 = PDO_ATTR_DRIVER_SPECIFIC + 5;

// enum pdo_sqlite_transaction_mode
pub const PDO_SQLITE_TRANSACTION_MODE_DEFERRED: i64 = 0;
pub const PDO_SQLITE_TRANSACTION_MODE_IMMEDIATE: i64 = 1;
pub const PDO_SQLITE_TRANSACTION_MODE_EXCLUSIVE: i64 = 2;

/// Pdo\Sqlite's constants in declaration order
pub const CONSTANTS: &[(&str, i64)] = &[
    ("DETERMINISTIC", SQLITE_DETERMINISTIC as i64),
    ("OPEN_READONLY", SQLITE_OPEN_READONLY as i64),
    ("OPEN_READWRITE", SQLITE_OPEN_READWRITE as i64),
    ("OPEN_CREATE", SQLITE_OPEN_CREATE as i64),
    ("ATTR_OPEN_FLAGS", PDO_SQLITE_ATTR_OPEN_FLAGS),
    (
        "ATTR_READONLY_STATEMENT",
        PDO_SQLITE_ATTR_READONLY_STATEMENT,
    ),
    (
        "ATTR_EXTENDED_RESULT_CODES",
        PDO_SQLITE_ATTR_EXTENDED_RESULT_CODES,
    ),
    ("ATTR_BUSY_STATEMENT", PDO_SQLITE_ATTR_BUSY_STATEMENT),
    ("ATTR_EXPLAIN_STATEMENT", PDO_SQLITE_ATTR_EXPLAIN_STATEMENT),
    ("ATTR_TRANSACTION_MODE", PDO_SQLITE_ATTR_TRANSACTION_MODE),
    (
        "TRANSACTION_MODE_DEFERRED",
        PDO_SQLITE_TRANSACTION_MODE_DEFERRED,
    ),
    (
        "TRANSACTION_MODE_IMMEDIATE",
        PDO_SQLITE_TRANSACTION_MODE_IMMEDIATE,
    ),
    (
        "TRANSACTION_MODE_EXCLUSIVE",
        PDO_SQLITE_TRANSACTION_MODE_EXCLUSIVE,
    ),
    ("EXPLAIN_MODE_PREPARED", 0),
    ("EXPLAIN_MODE_EXPLAIN", 1),
    ("EXPLAIN_MODE_EXPLAIN_QUERY_PLAN", 2),
    ("OK", SQLITE_OK as i64),
    ("DENY", SQLITE_DENY as i64),
    ("IGNORE", SQLITE_IGNORE as i64),
];

/// The deprecated PDO::SQLITE_* aliases PHP_MINIT(pdo_sqlite) registers
const PDO_ALIASES: &[&str] = &[
    "DETERMINISTIC",
    "ATTR_OPEN_FLAGS",
    "OPEN_READONLY",
    "OPEN_READWRITE",
    "OPEN_CREATE",
    "ATTR_READONLY_STATEMENT",
    "ATTR_EXTENDED_RESULT_CODES",
];

fn const_value(name: &str) -> i64 {
    CONSTANTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

/// The PDO:: constants pdo_sqlite's MINIT adds (all deprecated)
pub fn pdo_constants() -> Vec<(Vec<u8>, Value, Option<String>)> {
    PDO_ALIASES
        .iter()
        .map(|n| {
            let old = format!("SQLITE_{}", n);
            let msg = format!(
                "Constant PDO::{} is deprecated since 8.5, use Pdo\\Sqlite::{} instead",
                old, n
            );
            (old.into_bytes(), Value::Long(const_value(n)), Some(msg))
        })
        .collect()
}

/// Pdo\Sqlite's class constants for Vm::ext_class_constants
pub fn class_constants() -> Vec<(Vec<u8>, Value, Option<String>)> {
    CONSTANTS
        .iter()
        .map(|(n, v)| (n.as_bytes().to_vec(), Value::Long(*v), None))
        .collect()
}

// ── the connection ────────────────────────────────────────────────────

/// An open SQLite connection (H->db), shared with the statements and the
/// blob streams. `db` is None once closed.
pub struct Conn {
    db: RefCell<Option<Box<Sqlite3>>>,
    /// statements freed while the connection was in use, finalized later
    pending: RefCell<Vec<StmtId>>,
    /// sqlite3_close_v2() was called while the connection was in use
    close_pending: Cell<bool>,
}

pub type ConnRef = Rc<Conn>;

impl Conn {
    pub fn new(db: Box<Sqlite3>) -> ConnRef {
        Rc::new(Conn {
            db: RefCell::new(Some(db)),
            pending: RefCell::new(Vec::new()),
            close_pending: Cell::new(false),
        })
    }

    /// Run `f` on the connection: None when it is closed or busy (a
    /// callback of this very connection is running).
    pub fn with<R>(&self, f: impl FnOnce(&mut Sqlite3) -> R) -> Option<R> {
        let r = {
            let mut b = self.db.try_borrow_mut().ok()?;
            let db = b.as_deref_mut()?;
            let r = f(db);
            let pending = std::mem::take(&mut *self.pending.borrow_mut());
            for id in pending {
                goro_sqlite::vdbeapi::sqlite3_finalize(db, id);
            }
            r
        };
        if self.close_pending.get() {
            self.close();
        }
        Some(r)
    }

    /// Whether the connection is open
    pub fn is_open(&self) -> bool {
        match self.db.try_borrow() {
            Ok(b) => b.is_some(),
            Err(_) => true,
        }
    }

    /// sqlite3_finalize(), deferred while the connection is in use
    pub fn finalize(&self, id: StmtId) {
        match self.db.try_borrow_mut() {
            Ok(mut b) => {
                if let Some(db) = b.as_deref_mut() {
                    goro_sqlite::vdbeapi::sqlite3_finalize(db, id);
                }
            }
            Err(_) => self.pending.borrow_mut().push(id),
        }
    }

    /// sqlite3_close_v2(): the connection becomes a zombie until its
    /// statements and blobs are finalized; it is freed when the last
    /// reference goes away.
    pub fn close(&self) {
        match self.db.try_borrow_mut() {
            Ok(mut b) => {
                self.close_pending.set(false);
                if let Some(db) = b.as_deref_mut() {
                    mainapi::sqlite3_close_v2(db);
                }
                // keep a zombie that still has statements open (blob
                // streams), as sqlite3_close_v2() does
                let zombie = b
                    .as_ref()
                    .is_some_and(|db| !db.aVdbe.iter().all(|s| !s.used));
                if !zombie {
                    *b = None;
                }
            }
            Err(_) => self.close_pending.set(true),
        }
    }
}

thread_local! {
    /// The VM running PHP code, for the callbacks SQLite makes
    static VM_PTR: Cell<*mut Vm> = const { Cell::new(std::ptr::null_mut()) };
}

/// Remember the VM for the callbacks of SQLite calls made where no VM is at
/// hand (transaction hooks, statement destruction).
pub fn set_vm(vm: &mut Vm) {
    VM_PTR.with(|p| p.set(vm as *mut Vm));
}

pub fn vm_ptr() -> Option<*mut Vm> {
    let p = VM_PTR.with(|p| p.get());
    if p.is_null() { None } else { Some(p) }
}

/// Run `f` on the connection with the VM available to the callbacks.
pub fn run<R>(vm: &mut Vm, conn: &ConnRef, f: impl FnOnce(&mut Sqlite3) -> R) -> Option<R> {
    let p = vm as *mut Vm;
    let prev = VM_PTR.with(|c| c.replace(p));
    let r = conn.with(f);
    VM_PTR.with(|c| c.set(if prev.is_null() { p } else { prev }));
    r
}

/// Run `f` on the connection with the last VM seen.
pub fn run_last<R>(conn: &ConnRef, f: impl FnOnce(&mut Sqlite3) -> R) -> Option<R> {
    conn.with(f)
}

// ── blob streams ──────────────────────────────────────────────────────

/// php_error_docref() from a stream operation: prefixed by the stream
/// function running it (fwrite() ...)
fn stream_warning(vm: &mut Vm, msg: &str) {
    match vm.current_builtin_name() {
        Some(f) if !f.is_empty() => vm.emit_warning(&format!("{}(): {}", f, msg)),
        _ => vm.emit_warning(msg),
    }
}

/// php_stream_pdosqlite3_data
struct BlobStream {
    conn: ConnRef,
    blob: Option<Box<goro_sqlite::vdbeblob::Blob>>,
    position: usize,
    size: usize,
    flags: i64,
}

impl goro_core::ext_stream::ExtStream for BlobStream {
    fn read(&mut self, _vm: &mut Vm, count: usize, eof: &mut bool) -> Option<Vec<u8>> {
        let mut count = count;
        if self.position + count >= self.size {
            count = self.size - self.position;
            *eof = true;
        }
        if count == 0 {
            return Some(Vec::new());
        }
        let blob = self.blob.as_deref_mut()?;
        let pos = self.position;
        let mut buf = vec![0u8; count];
        let rc = self.conn.with(|db| {
            goro_sqlite::vdbeblob::sqlite3_blob_read(db, blob, &mut buf, count as i32, pos as i32)
        });
        if rc != Some(SQLITE_OK) {
            return None;
        }
        self.position += count;
        Some(buf)
    }

    fn write(&mut self, vm: &mut Vm, buf: &[u8]) -> i64 {
        let count = buf.len();
        if self.flags & SQLITE_OPEN_READONLY as i64 != 0 {
            stream_warning(vm, "Can't write to blob stream: is open as read only");
            return -1;
        }
        if self.position + count > self.size {
            stream_warning(vm, "It is not possible to increase the size of a BLOB");
            return -1;
        }
        let Some(blob) = self.blob.as_deref_mut() else {
            return -1;
        };
        let pos = self.position;
        let rc = self.conn.with(|db| {
            goro_sqlite::vdbeblob::sqlite3_blob_write(db, blob, buf, count as i32, pos as i32)
        });
        if rc != Some(SQLITE_OK) {
            return -1;
        }
        if self.position + count >= self.size {
            self.position = self.size;
        } else {
            self.position += count;
        }
        count as i64
    }

    fn close(&mut self, _vm: &mut Vm) {
        if let Some(mut blob) = self.blob.take() {
            // an error occurred, but it still closed
            let _ = self
                .conn
                .with(|db| goro_sqlite::vdbeblob::sqlite3_blob_close(db, &mut blob));
        }
    }

    fn seek(&mut self, _vm: &mut Vm, offset: i64, whence: i64) -> Option<i64> {
        let size = self.size as i64;
        let pos = self.position as i64;
        match whence {
            // SEEK_CUR
            1 => {
                if offset < 0 {
                    if pos < -offset {
                        self.position = 0;
                        None
                    } else {
                        self.position = (pos + offset) as usize;
                        Some(self.position as i64)
                    }
                } else if pos + offset > size {
                    self.position = self.size;
                    None
                } else {
                    self.position = (pos + offset) as usize;
                    Some(self.position as i64)
                }
            }
            // SEEK_SET
            0 => {
                if size < offset {
                    self.position = self.size;
                    None
                } else {
                    self.position = offset.max(0) as usize;
                    Some(self.position as i64)
                }
            }
            // SEEK_END
            2 => {
                if offset > 0 {
                    self.position = self.size;
                    None
                } else if size < -offset {
                    self.position = 0;
                    None
                } else {
                    self.position = (size + offset) as usize;
                    Some(self.position as i64)
                }
            }
            _ => None,
        }
    }

    fn label(&self) -> &'static str {
        "PDOSQLite"
    }
}

/// Pdo\Sqlite::openBlob
pub fn open_blob(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Some(dbh) = this_dbh(vm, f) else {
        return Value::Null;
    };
    let Ok(a) = parse(vm, f, "ssl|s!l", &[], args) else {
        return Value::Null;
    };
    for (i, n) in [(0usize, 1usize), (1, 2), (3, 4)] {
        if a[i].str().is_some_and(|s| s.contains(&0)) {
            let fname = &f.name;
            let pname = f.params.get(n - 1).copied().unwrap_or("");
            let m = format!(
                "{}(): Argument #{} (${}) must not contain any null bytes",
                fname, n, pname
            );
            throw(vm, b"ValueError", &m);
            return Value::Null;
        }
    }
    let table = a[0].str().unwrap_or(b"").to_vec();
    let column = a[1].str().unwrap_or(b"").to_vec();
    let rowid = a[2].long_or(0);
    let dbname = a[3].str().unwrap_or(b"main").to_vec();
    let flags = a[4].long_or(SQLITE_OPEN_READONLY as i64);
    let Some(conn) = conn_of(&dbh) else {
        return Value::False;
    };
    let sqlite_flags = i32::from(flags & SQLITE_OPEN_READWRITE as i64 != 0);
    let r = run(vm, &conn, |db| {
        let mut blob = None;
        let rc = goro_sqlite::vdbeblob::sqlite3_blob_open(
            db,
            Some(&dbname),
            Some(&table),
            Some(&column),
            rowid,
            sqlite_flags,
            &mut blob,
        );
        if rc != SQLITE_OK {
            Err(mainapi::sqlite3_errmsg(db).unwrap_or_default())
        } else {
            Ok(blob)
        }
    });
    let blob = match r {
        Some(Ok(Some(b))) => b,
        Some(Err(msg)) => {
            vm.emit_warning(&format!(
                "Unable to open blob: {}",
                String::from_utf8_lossy(&msg)
            ));
            return Value::False;
        }
        _ => {
            vm.emit_warning("Unable to open blob: bad parameter or other API misuse");
            return Value::False;
        }
    };
    let mut blob = blob;
    let size = goro_sqlite::vdbeblob::sqlite3_blob_bytes(&mut blob).max(0) as usize;
    let s = BlobStream {
        conn,
        blob: Some(blob),
        position: 0,
        size,
        flags,
    };
    let mode = if sqlite_flags != 0 { "r+b" } else { "rb" };
    match goro_core::ext_stream::alloc(vm, Rc::new(RefCell::new(s)), mode) {
        Some(id) => Value::Resource(goro_core::value::ResourceRef::new(id)),
        None => Value::False,
    }
}

/// Pdo\Sqlite::loadExtension
pub fn load_extension(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let Ok(a) = parse(vm, f, "s", &[], args) else {
        return Value::Null;
    };
    let ext = a[0].str().unwrap_or(b"").to_vec();
    if ext.contains(&0) {
        let m = format!("{} must not contain any null bytes", f.arg_prefix(1));
        throw(vm, b"ValueError", &m);
        return Value::Null;
    }
    if ext.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Value::Null;
    }
    let Some(dbh) = this_dbh(vm, f) else {
        return Value::Null;
    };
    let path = String::from_utf8_lossy(&ext).into_owned();
    let fullpath = match std::fs::canonicalize(&path) {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => {
            throw_pdo(vm, &format!("Unable to load extension \"{}\"", path));
            return Value::Null;
        }
    };
    let Some(conn) = conn_of(&dbh) else {
        return Value::Null;
    };
    let r = run(vm, &conn, |db| {
        // This only enables extension loading for the C api, not for SQL
        mainapi::sqlite3_db_config(db, SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, 1, None);
        let mut errtext = None;
        let rc = goro_sqlite::loadext::sqlite3_load_extension(
            db,
            Some(fullpath.as_bytes()),
            None,
            &mut errtext,
        );
        mainapi::sqlite3_db_config(db, SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, 0, None);
        (rc, errtext)
    });
    match r {
        Some((SQLITE_OK, _)) => Value::Null,
        Some((_, errtext)) => {
            let m = format!(
                "Unable to load extension \"{}\"",
                String::from_utf8_lossy(&errtext.unwrap_or_default())
            );
            throw_pdo(vm, &m);
            Value::Null
        }
        None => {
            throw_pdo(
                vm,
                "Unable to load extension \"bad parameter or other API misuse\"",
            );
            Value::Null
        }
    }
}

pub fn create_function(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    create_function_internal(vm, f, args)
}

pub fn create_aggregate(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    create_aggregate_internal(vm, f, args)
}

pub fn create_collation(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    create_collation_internal(vm, f, args)
}

type Impl = fn(&mut Vm, &Func, &[Value]) -> Value;

/// PDO_SQLite_Ext: the deprecated PDO::sqlite* driver methods
pub const EXT_METHODS: &[(&str, &str)] = &[
    ("sqliteCreateFunction", "createFunction"),
    ("sqliteCreateAggregate", "createAggregate"),
    ("sqliteCreateCollation", "createCollation"),
];

fn class_method(name: &str) -> Option<Impl> {
    Some(match name {
        "createFunction" => create_function,
        "createAggregate" => create_aggregate,
        "createCollation" => create_collation,
        _ => return None,
    })
}

/// Whether `this` is a PDO handle of the sqlite driver
fn is_sqlite_handle(this: Option<&Obj>) -> bool {
    let Some(o) = this else {
        return false;
    };
    let Some(dbh) = crate::objects::dbh_of(o) else {
        return false;
    };
    dbh.borrow().driver.is_some_and(|d| d.name == "sqlite")
}

/// dbh_method_get + the call of a PDO::sqlite* method. None: no such
/// method for this handle.
pub fn call_ext_method(
    vm: &mut Vm,
    method_lower: &[u8],
    this: Option<&Obj>,
    args: &[Value],
) -> Option<Value> {
    let (old, new) = EXT_METHODS
        .iter()
        .find(|(o, _)| o.as_bytes().eq_ignore_ascii_case(method_lower))?;
    if !is_sqlite_handle(this) {
        return None;
    }
    let msg = format!(
        "Method PDO::{}() is deprecated since 8.5, use Pdo\\Sqlite::{}() instead",
        old, new
    );
    if vm.emit_method_deprecation(&msg, old) {
        return Some(Value::Null);
    }
    let params = goro_core::stubs::find_method(b"PDO_SQLite_Ext", old.as_bytes())
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

/// PHP_MINFO(pdo_sqlite)
pub fn minfo(_vm: &mut Vm) -> String {
    format!(
        "\npdo_sqlite\n\nPDO Driver for SQLite 3.x => enabled\nSQLite Library => {}\n",
        String::from_utf8_lossy(&mainapi::sqlite3_libversion().unwrap_or_default())
    )
}

fn shutdown(_vm: &mut Vm) {
    VM_PTR.with(|p| p.set(std::ptr::null_mut()));
}

/// PHP_MINIT(pdo_sqlite)
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"pdo_sqlite");
    register_driver(&SQLITE_DRIVER);
    register_driver_specific_ce(&SQLITE_DRIVER, b"pdo\\sqlite");
    vm.phpinfo_hooks.push(minfo);
    vm.shutdown_hooks.push(shutdown);
}
