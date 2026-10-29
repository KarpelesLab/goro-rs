//! pdo_sqlite (PHP manual: SQLite (PDO), PDO_SQLITE DSN, Pdo\Sqlite):
//! the "sqlite" PDO driver on goro-ext-sqlite.
//!
//! Behaviour from the manual and the ext/pdo_sqlite and ext/pdo tests:
//! SQLITE_* result codes become SQLSTATEs (constraint → 23000, else
//! HY000), errors read "SQLSTATE[HY000]: General error: 1 no such
//! table: t", fetched columns keep their SQLite types (INTEGER → int,
//! REAL → float, TEXT and BLOB → string), and statements run when
//! executed with every row kept for fetching.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use goro_ext_sqlite::graphitesql::Value as SqlValue;
use goro_ext_sqlite::{self as gs, Db, Kind, Rows, SqlError, Stmt};
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::driver::*;
use crate::builtins::sqlite_common as sc;
use crate::vm::{R, Vm};

/// Pdo\Sqlite attribute numbers (constants of the class).
pub const ATTR_OPEN_FLAGS: i64 = 1000;
pub const ATTR_READONLY_STATEMENT: i64 = 1001;
pub const ATTR_EXTENDED_RESULT_CODES: i64 = 1002;
pub const ATTR_BUSY_STATEMENT: i64 = 1003;
pub const ATTR_EXPLAIN_STATEMENT: i64 = 1004;
pub const ATTR_TRANSACTION_MODE: i64 = 1005;

/// A connection, shared by the PDO object and its statements.
pub struct Conn {
    pub db: Db,
    /// The last error (code, extended code, message).
    pub err: SqlError,
    pub extended: bool,
    /// Callables registered as functions, aggregates, collations.
    pub callbacks: Vec<Value>,
    pub authorizer: Option<Value>,
    /// Whether open_basedir applies to ATTACH.
    pub txn_mode: i64,
    pub open_flags: i64,
}

impl Conn {
    /// The PDO error of the last SQLite error.
    pub fn pdo_error(&self) -> DrvError {
        to_pdo(&self.err, self.extended)
    }
}

/// SQLite result code → SQLSTATE.
pub fn sqlstate(code: i32) -> &'static str {
    match code {
        gs::SQLITE_OK => "00000",
        gs::SQLITE_NOTFOUND => "42S02",
        gs::SQLITE_INTERRUPT => "01002",
        gs::SQLITE_NOLFS => "HYC00",
        gs::SQLITE_TOOBIG => "22001",
        gs::SQLITE_CONSTRAINT => "23000",
        _ => "HY000",
    }
}

fn to_pdo(e: &SqlError, extended: bool) -> DrvError {
    let code = if extended { e.ext_code } else { e.code };
    if code == gs::SQLITE_OK {
        return DrvError::new("00000", None, None);
    }
    DrvError::new(sqlstate(code), Some(code as i64), Some(e.msg.clone()))
}

pub struct SqliteDriver {
    pub conn: Rc<RefCell<Conn>>,
}

thread_local! {
    /// Shared-cache in-memory databases by name.
    static SHARED: RefCell<std::collections::HashMap<String, std::rc::Weak<RefCell<Conn>>>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Runs a database call with the interpreter published to callbacks,
/// turning an exception raised by one into a PHP failure.
fn guarded<T>(vm: &mut Vm, f: impl FnOnce() -> T) -> Result<T, Failure> {
    let r = {
        let _g = sc::enter(vm);
        f()
    };
    match sc::take_pending() {
        Some(u) => Err(Failure::Php(u)),
        None => Ok(r),
    }
}

impl SqliteDriver {
    /// Opens the database of a DSN (`sqlite:<path>`, `sqlite::memory:`,
    /// `sqlite:file:…` URIs).
    pub fn open(vm: &mut Vm, path: &[u8], flags: i64) -> Result<SqliteDriver, DrvError> {
        let p = String::from_utf8_lossy(path).into_owned();
        let mut f = flags as i32;
        if p.starts_with("file:") {
            f |= gs::OPEN_URI;
        }
        // In-memory databases opened with cache=shared are one database
        // for every connection naming them (sharedcache.html, test
        // pdo_sqlite_filename_uri.phpt).
        let shared = (p.starts_with("file:")
            && p.contains("cache=shared")
            && (p.starts_with("file::memory:") || p.contains("mode=memory")))
        .then(|| p.split('?').next().unwrap_or("").to_string());
        if let Some(key) = &shared
            && let Some(c) = SHARED.with(|s| s.borrow().get(key).and_then(|w| w.upgrade()))
        {
            return Ok(SqliteDriver { conn: c });
        }
        match Db::open(&p, f) {
            Ok(db) => {
                let _ = vm;
                let conn = Rc::new(RefCell::new(Conn {
                    db,
                    err: SqlError::ok(),
                    extended: false,
                    callbacks: Vec::new(),
                    authorizer: None,
                    txn_mode: 0,
                    open_flags: flags,
                }));
                if let Some(key) = shared {
                    SHARED.with(|s| s.borrow_mut().insert(key, Rc::downgrade(&conn)));
                }
                Ok(SqliteDriver { conn })
            }
            Err(e) => Err(DrvError::new(
                sqlstate(e.code),
                Some(e.code as i64),
                Some(e.msg),
            )),
        }
    }

    fn fail(&self, e: SqlError) -> Failure {
        let mut c = self.conn.borrow_mut();
        c.err = e;
        Failure::Db(c.pdo_error())
    }

    /// Runs each statement of a script in turn (sqlite3_exec).
    fn run_sql(&mut self, vm: &mut Vm, sql: &str) -> DResult<i64> {
        let conn = self.conn.clone();
        let mut rest = sql;
        loop {
            let p = conn.borrow().db.prepare(rest);
            let (st, tail) = p.map_err(|e| self.fail(e))?;
            if let Some(st) = st {
                if let Err(e) = attach_allowed(vm, &st) {
                    return Err(self.fail(e));
                }
                let r = guarded(vm, || conn.borrow_mut().db.run(&st, &[]))?;
                if let Err(e) = r {
                    return Err(self.fail(e));
                }
            }
            if tail >= rest.len() {
                break;
            }
            rest = &rest[tail..];
        }
        let mut c = self.conn.borrow_mut();
        c.err = SqlError::ok();
        Ok(c.db.changes())
    }
}

/// ATTACH is refused for files open_basedir excludes and for names that
/// aren't literal (PHP's authorizer, test gh9032.phpt):
/// "23 not authorized".
pub fn attach_allowed(vm: &mut Vm, st: &Stmt) -> Result<(), SqlError> {
    let Some(file) = &st.attach else {
        return Ok(());
    };
    let denied = || SqlError::new(gs::SQLITE_AUTH, "not authorized");
    let Some(f) = file else {
        return Err(denied());
    };
    if f.is_empty() || f == ":memory:" || (f.starts_with("file:") && f.contains("mode=memory")) {
        return Ok(());
    }
    let path = f.strip_prefix("file:").unwrap_or(f);
    let path = path.split('?').next().unwrap_or(path);
    if !vm.basedir_allows(path.as_bytes()) {
        return Err(denied());
    }
    Ok(())
}

impl Driver for SqliteDriver {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    fn placeholders(&self) -> Placeholders {
        Placeholders::Both
    }

    fn prepare(
        &mut self,
        _vm: &mut Vm,
        sql: &[u8],
        _opts: &PhpArray,
    ) -> DResult<Box<dyn DriverStmt>> {
        let text = gs::scan::sql_from_bytes(sql);
        let r = self.conn.borrow().db.prepare(&text);
        match r {
            Ok((st, _tail)) => {
                self.conn.borrow_mut().err = SqlError::ok();
                Ok(Box::new(SqliteStmt {
                    conn: self.conn.clone(),
                    st,
                    rows: None,
                    pos: 0,
                    executed: false,
                    changes: 0,
                    explain: 0,
                    origins: None,
                }))
            }
            Err(e) => Err(self.fail(e)),
        }
    }

    fn exec(&mut self, vm: &mut Vm, sql: &[u8]) -> DResult<i64> {
        let text = gs::scan::sql_from_bytes(sql);
        // sqlite3_changes(): the rows changed by the last INSERT, UPDATE
        // or DELETE, even when later statements changed none (test
        // pdosqlite_setauthorizer.phpt: a SELECT after an INSERT gives 1).
        self.run_sql(vm, &text)
    }

    fn quote(&mut self, vm: &mut Vm, s: &[u8], _ptype: i64) -> R<Option<Vec<u8>>> {
        // A NUL byte can't be in an SQL text literal: a warning and false
        // (test gh13952.phpt); so is a string too long for SQLite (test
        // bug81740.phpt).
        if s.contains(&0) {
            vm.warn("PDO::quote(): SQLite PDO::quote does not support null bytes")?;
            return Ok(None);
        }
        if s.len() >= 0x7fff_fffe {
            return Ok(None);
        }
        // SQL text reaches graphitesql as UTF-8: bytes that aren't go in
        // as a blob cast to TEXT, which keeps them exactly (test
        // gh13952.phpt round-trips "\x80").
        if std::str::from_utf8(s).is_err() {
            let mut out = b"CAST(X'".to_vec();
            for b in s {
                out.extend_from_slice(format!("{b:02X}").as_bytes());
            }
            out.extend_from_slice(b"' AS TEXT)");
            return Ok(Some(out));
        }
        let mut out = vec![b'\''];
        out.extend(gs::scan::escape_string(s));
        out.push(b'\'');
        Ok(Some(out))
    }

    fn begin(&mut self, vm: &mut Vm) -> DResult<()> {
        let mode = self.conn.borrow().txn_mode;
        let sql = match mode {
            1 => "BEGIN IMMEDIATE TRANSACTION",
            2 => "BEGIN EXCLUSIVE TRANSACTION",
            _ => "BEGIN",
        };
        self.run_sql(vm, sql).map(|_| ())
    }

    fn commit(&mut self, vm: &mut Vm) -> DResult<()> {
        self.run_sql(vm, "COMMIT").map(|_| ())
    }

    fn rollback(&mut self, vm: &mut Vm) -> DResult<()> {
        self.run_sql(vm, "ROLLBACK").map(|_| ())
    }

    fn in_transaction(&self) -> Option<bool> {
        Some(!self.conn.borrow().db.autocommit())
    }

    fn last_insert_id(&mut self, _name: Option<&[u8]>) -> DResult<Vec<u8>> {
        Ok(self
            .conn
            .borrow()
            .db
            .last_insert_rowid()
            .to_string()
            .into_bytes())
    }

    fn set_attribute(&mut self, vm: &mut Vm, attr: i64, v: &Value) -> R<Option<bool>> {
        match attr {
            // PDO::ATTR_TIMEOUT: seconds of busy timeout.
            2 => {
                let s = match v {
                    Value::Int(i) => *i,
                    Value::Bool(b) => *b as i64,
                    Value::Str(s) if goro_runtime::numeric::is_numeric_str(s.as_bytes()) => {
                        vm.to_int(v)?
                    }
                    other => {
                        return vm.type_error(&format!(
                            "Attribute value must be of type int for selected attribute, {} given",
                            Vm::type_name_given(other)
                        ));
                    }
                };
                self.conn
                    .borrow_mut()
                    .db
                    .busy_timeout(s.saturating_mul(1000));
                Ok(Some(true))
            }
            ATTR_EXTENDED_RESULT_CODES => {
                let b = vm.to_bool(v);
                self.conn.borrow_mut().extended = b;
                Ok(Some(true))
            }
            ATTR_TRANSACTION_MODE => {
                let n = vm.to_int(v)?;
                if !(0..=2).contains(&n) {
                    return Ok(Some(false));
                }
                self.conn.borrow_mut().txn_mode = n;
                Ok(Some(true))
            }
            ATTR_OPEN_FLAGS => Ok(Some(false)),
            _ => Ok(None),
        }
    }

    fn get_attribute(&mut self, _vm: &mut Vm, attr: i64) -> R<Option<Value>> {
        Ok(match attr {
            // ATTR_SERVER_VERSION, ATTR_CLIENT_VERSION: the library.
            4 | 5 => Some(Value::Str(PhpStr::from(gs::VERSION))),
            ATTR_TRANSACTION_MODE => Some(Value::Int(self.conn.borrow().txn_mode)),
            ATTR_EXTENDED_RESULT_CODES => Some(Value::Bool(self.conn.borrow().extended)),
            ATTR_OPEN_FLAGS => Some(Value::Int(self.conn.borrow().open_flags)),
            _ => None,
        })
    }

    fn last_error(&self) -> Option<DrvError> {
        Some(self.conn.borrow().pdo_error())
    }

    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        if let Ok(c) = self.conn.try_borrow() {
            for v in &c.callbacks {
                visit(v);
            }
            if let Some(a) = &c.authorizer {
                visit(a);
            }
        }
    }

    fn clear(&mut self) {
        if let Ok(mut c) = self.conn.try_borrow_mut() {
            c.callbacks.clear();
            c.authorizer = None;
        }
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct SqliteStmt {
    conn: Rc<RefCell<Conn>>,
    /// `None` for an empty statement (only spaces and comments).
    st: Option<Stmt>,
    rows: Option<Rows>,
    pos: usize,
    executed: bool,
    changes: i64,
    /// Pdo\Sqlite::ATTR_EXPLAIN_STATEMENT.
    explain: i64,
    origins: Option<Vec<(Option<String>, Option<String>)>>,
}

/// A PHP parameter value as SQL, by PDO::PARAM_* type (PHP manual:
/// PDOStatement::bindValue; pdo_sqlite binds INT/BOOL as integers, LOB
/// as blobs, NULL and null values as NULL, the rest as text).
fn bind_value(vm: &mut Vm, v: &Value, ptype: i64) -> R<SqlValue> {
    let base = ptype & 0x3fff_ffff & !0x2000_0000;
    let v = v.deref();
    if matches!(v, Value::Null) {
        return Ok(SqlValue::Null);
    }
    Ok(match base {
        0 => SqlValue::Null,
        1 | 5 => match &v {
            Value::Int(i) => SqlValue::Integer(*i),
            Value::Bool(b) => SqlValue::Integer(*b as i64),
            Value::Float(f) => {
                if f.fract() == 0.0 && f.abs() < 9.2e18 {
                    SqlValue::Integer(*f as i64)
                } else {
                    SqlValue::Real(*f)
                }
            }
            _ => SqlValue::Integer(vm.to_int(&v)?),
        },
        3 => match &v {
            Value::Resource(_) => {
                let f = Value::Str(PhpStr::from("stream_get_contents"));
                match vm.call_value(&f, vec![v.clone()])? {
                    Value::Str(s) => SqlValue::Blob(s.as_bytes().to_vec()),
                    _ => SqlValue::Null,
                }
            }
            _ => {
                let s = vm.to_php_str(&v)?;
                SqlValue::Blob(s.as_bytes().to_vec())
            }
        },
        // PARAM_STR: always text, so `'1' = ?` with 1 matches (test
        // pdo_sqlite_parser.phpt); column affinity converts on insert.
        _ => {
            let s = vm.to_php_str(&v)?;
            sc::text(s.as_bytes())
        }
    })
}

impl SqliteStmt {
    fn fail(&self, e: SqlError) -> Failure {
        let mut c = self.conn.borrow_mut();
        c.err = e;
        Failure::Db(c.pdo_error())
    }

    fn native_type(v: Option<&SqlValue>) -> &'static str {
        match v {
            Some(SqlValue::Integer(_)) => "integer",
            Some(SqlValue::Real(_)) => "double",
            Some(SqlValue::Text(_)) => "string",
            Some(SqlValue::Blob(_)) => "blob",
            _ => "null",
        }
    }
}

impl DriverStmt for SqliteStmt {
    fn execute(&mut self, vm: &mut Vm, params: &[ExecParam]) -> DResult<()> {
        let Some(st) = self.st.clone() else {
            self.executed = true;
            self.rows = Some(Rows::default());
            return Ok(());
        };
        let mut values: Vec<SqlValue> = vec![SqlValue::Null; st.params.len()];
        for p in params {
            let idx = match &p.key {
                ParamKey::Pos(i) => {
                    if *i >= st.params.len() {
                        let mut c = self.conn.borrow_mut();
                        c.err = SqlError::new(gs::SQLITE_RANGE, "column index out of range");
                        return Err(Failure::Db(c.pdo_error()));
                    }
                    *i
                }
                ParamKey::Name(n) => {
                    let name = String::from_utf8_lossy(n);
                    match st.param_index(&name) {
                        0 => {
                            return Err(Failure::Db(DrvError::pdo(
                                "HY093",
                                "parameter was not defined",
                            )));
                        }
                        k => k - 1,
                    }
                }
            };
            values[idx] = bind_value(vm, &p.value, p.ptype)?;
        }
        if let Err(e) = attach_allowed(vm, &st) {
            return Err(self.fail(e));
        }
        let mut run = st.clone();
        if self.explain == 1 {
            run.sql = format!("EXPLAIN {}", st.sql);
            run.kind = Kind::Query;
        } else if self.explain == 2 {
            run.sql = format!("EXPLAIN QUERY PLAN {}", st.sql);
            run.kind = Kind::Query;
        }
        let conn = self.conn.clone();
        let r = guarded(vm, || conn.borrow_mut().db.run(&run, &values))?;
        match r {
            Ok(rows) => {
                let mut c = self.conn.borrow_mut();
                c.err = SqlError::ok();
                self.changes = if st.kind == Kind::Query {
                    0
                } else {
                    c.db.changes()
                };
                drop(c);
                self.rows = Some(rows);
                self.pos = 0;
                self.executed = true;
                Ok(())
            }
            Err(e) => {
                self.rows = None;
                Err(self.fail(e))
            }
        }
    }

    fn column_count(&self) -> usize {
        self.rows.as_ref().map_or(0, |r| r.columns.len())
    }

    fn column_name(&self, i: usize) -> Vec<u8> {
        self.rows
            .as_ref()
            .and_then(|r| r.columns.get(i))
            .map(|s| s.as_bytes().to_vec())
            .unwrap_or_default()
    }

    fn fetch(&mut self, _vm: &mut Vm) -> DResult<Option<Vec<Value>>> {
        let Some(rows) = &self.rows else {
            return Ok(None);
        };
        let Some(row) = rows.rows.get(self.pos) else {
            return Ok(None);
        };
        let out = row.iter().map(sc::to_php).collect();
        self.pos += 1;
        Ok(Some(out))
    }

    fn row_count(&self) -> i64 {
        self.changes
    }

    fn column_meta(&mut self, _vm: &mut Vm, i: usize) -> DResult<Option<PhpArray>> {
        let Some(rows) = &self.rows else {
            return Ok(None);
        };
        if i >= rows.columns.len() {
            return Ok(None);
        }
        // The type of the value in the current row (the last fetched).
        let cur = if self.pos > 0 {
            rows.rows.get(self.pos - 1).and_then(|r| r.get(i))
        } else {
            None
        };
        let ncols = rows.columns.len();
        if self.origins.is_none() {
            let o = match &self.st {
                Some(st) => self.conn.borrow().db.column_origins(st, ncols),
                None => vec![(None, None); ncols],
            };
            self.origins = Some(o);
        }
        let (decl, table) = self.origins.as_ref().unwrap()[i].clone();
        let mut m = PhpArray::new();
        let nt = Self::native_type(cur);
        m.insert(
            Key::from_bytes(b"native_type"),
            Value::Str(PhpStr::from(nt)),
        );
        let pdo_type = match cur {
            Some(SqlValue::Integer(_)) => 1,
            Some(SqlValue::Null) | None => 0,
            _ => 2,
        };
        m.insert(Key::from_bytes(b"pdo_type"), Value::Int(pdo_type));
        if let Some(d) = decl {
            m.insert(
                Key::from_bytes(b"sqlite:decl_type"),
                Value::Str(PhpStr::from(d.as_str())),
            );
        }
        if let Some(t) = table {
            m.insert(
                Key::from_bytes(b"table"),
                Value::Str(PhpStr::from(t.as_str())),
            );
        }
        m.insert(Key::from_bytes(b"flags"), Value::Array(PhpArray::new()));
        Ok(Some(m))
    }

    fn close_cursor(&mut self) {
        if let Some(r) = &self.rows {
            self.pos = r.rows.len();
        }
    }

    fn active(&self) -> bool {
        self.rows
            .as_ref()
            .is_some_and(|r| self.pos < r.rows.len() && self.executed)
    }

    fn set_attribute(&mut self, vm: &mut Vm, attr: i64, v: &Value) -> R<Option<bool>> {
        match attr {
            ATTR_EXPLAIN_STATEMENT => {
                let n = match v.deref() {
                    Value::Int(n) => n,
                    other => {
                        return vm.type_error(&format!(
                            "explain mode must be of type int, {} given",
                            crate::vm::Vm::type_name_given(&other)
                        ));
                    }
                };
                if !(0..=2).contains(&n) {
                    return vm.value_error(
                        "explain mode must be one of the Pdo\\Sqlite::EXPLAIN_MODE_* constants",
                    );
                }
                self.explain = n;
                Ok(Some(true))
            }
            _ => Ok(None),
        }
    }

    fn get_attribute(&mut self, _vm: &mut Vm, attr: i64) -> R<Option<Value>> {
        Ok(match attr {
            ATTR_READONLY_STATEMENT => {
                Some(Value::Bool(self.st.as_ref().is_none_or(|s| s.readonly)))
            }
            ATTR_BUSY_STATEMENT => Some(Value::Bool(self.active())),
            ATTR_EXPLAIN_STATEMENT => Some(Value::Int(self.explain)),
            _ => None,
        })
    }

    fn param_count(&self) -> Option<usize> {
        Some(self.st.as_ref().map_or(0, |s| s.params.len()))
    }
}
