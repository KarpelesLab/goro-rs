//! ext/sqlite3 (PHP manual: SQLite3): the `SQLite3`, `SQLite3Stmt`,
//! `SQLite3Result` and `SQLite3Exception` classes, on the goro-ext-sqlite
//! connections (graphitesql).
//!
//! Behaviour from the manual's SQLite3 chapter and the expectations of
//! ext/sqlite3/tests/*.phpt: diagnostics are warnings
//! ("SQLite3::query(): Unable to prepare statement: …") unless
//! `enableExceptions(true)` turned them into `SQLite3Exception`s;
//! `SQLite3Stmt::execute()` runs the statement once and leaves it reset,
//! so the first `fetchArray()` runs it again (tests gh20699.phpt,
//! sqlite3_trampoline_create_collation.phpt); `query()` whose result is
//! not used runs the whole script like `exec()` (tests
//! sqlite3_enable_exceptions.phpt, bug81742.phpt).

mod blob;
mod stmt;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_ext_sqlite::graphitesql::Value as SqlValue;
use goro_ext_sqlite::{self as gs, Db, SqlError, Stmt, scan};
use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use super::sqlite_common as common;
use crate::class::NativeHooks;
use crate::func::{Args, NativeFn};
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Unwind, Vm};

// ------------------------------------------------------------------ classes

const SQLITE3_CLASS: &str = r#"<?php
class SQLite3 {
    public const int OK = 0;
    public const int DENY = 1;
    public const int IGNORE = 2;
    public const int CREATE_INDEX = 1;
    public const int CREATE_TABLE = 2;
    public const int CREATE_TEMP_INDEX = 3;
    public const int CREATE_TEMP_TABLE = 4;
    public const int CREATE_TEMP_TRIGGER = 5;
    public const int CREATE_TEMP_VIEW = 6;
    public const int CREATE_TRIGGER = 7;
    public const int CREATE_VIEW = 8;
    public const int DELETE = 9;
    public const int DROP_INDEX = 10;
    public const int DROP_TABLE = 11;
    public const int DROP_TEMP_INDEX = 12;
    public const int DROP_TEMP_TABLE = 13;
    public const int DROP_TEMP_TRIGGER = 14;
    public const int DROP_TEMP_VIEW = 15;
    public const int DROP_TRIGGER = 16;
    public const int DROP_VIEW = 17;
    public const int INSERT = 18;
    public const int PRAGMA = 19;
    public const int READ = 20;
    public const int SELECT = 21;
    public const int TRANSACTION = 22;
    public const int UPDATE = 23;
    public const int ATTACH = 24;
    public const int DETACH = 25;
    public const int ALTER_TABLE = 26;
    public const int REINDEX = 27;
    public const int ANALYZE = 28;
    public const int CREATE_VTABLE = 29;
    public const int DROP_VTABLE = 30;
    public const int FUNCTION = 31;
    public const int SAVEPOINT = 32;
    public const int COPY = 0;
    public const int RECURSIVE = 33;
    public function __construct(string $filename, int $flags = SQLITE3_OPEN_READWRITE | SQLITE3_OPEN_CREATE, string $encryptionKey = "") {}
    public function open(string $filename, int $flags = SQLITE3_OPEN_READWRITE | SQLITE3_OPEN_CREATE, string $encryptionKey = ""): void {}
    public function close(): bool {}
    public static function version(): array {}
    public function lastInsertRowID(): int {}
    public function lastErrorCode(): int {}
    public function lastExtendedErrorCode(): int {}
    public function lastErrorMsg(): string {}
    public function changes(): int {}
    public function busyTimeout(int $milliseconds): bool {}
    public function loadExtension(string $name): bool {}
    public function backup(SQLite3 $destination, string $sourceDatabase = "main", string $destinationDatabase = "main"): bool {}
    public static function escapeString(string $string): string {}
    public function prepare(string $query): SQLite3Stmt|false {}
    public function exec(string $query): bool {}
    public function query(string $query): SQLite3Result|false {}
    public function querySingle(string $query, bool $entireRow = false): mixed {}
    public function createFunction(string $name, callable $callback, int $argCount = -1, int $flags = 0): bool {}
    public function createAggregate(string $name, callable $stepCallback, callable $finalCallback, int $argCount = -1): bool {}
    public function createCollation(string $name, callable $callback): bool {}
    public function openBlob(string $table, string $column, int $rowid, string $database = "main", int $flags = SQLITE3_OPEN_READONLY) {}
    public function enableExceptions(bool $enable = false): bool {}
    public function enableExtendedResultCodes(bool $enable = true): bool {}
    public function setAuthorizer(?callable $callback): bool {}
}"#;

const STMT_CLASS: &str = r#"<?php
class SQLite3Stmt {
    public const int EXPLAIN_MODE_PREPARED = 0;
    public const int EXPLAIN_MODE_EXPLAIN = 1;
    public const int EXPLAIN_MODE_EXPLAIN_QUERY_PLAN = 2;
    private function __construct(SQLite3 $sqlite3, string $query) {}
    public function bindParam(string|int $param, mixed &$var, int $type = SQLITE3_TEXT): bool {}
    public function bindValue(string|int $param, mixed $value, int $type = SQLITE3_TEXT): bool {}
    public function clear(): bool {}
    public function close(): bool {}
    public function execute(): SQLite3Result|false {}
    public function getSQL(bool $expand = false): string|false {}
    public function paramCount(): int {}
    public function readOnly(): bool {}
    public function reset(): bool {}
    public function busy(): bool {}
    public function explain(): int {}
    public function setExplain(int $mode): bool {}
}"#;

const RESULT_CLASS: &str = r#"<?php
class SQLite3Result {
    private function __construct() {}
    public function numColumns(): int {}
    public function columnName(int $column): string|false {}
    public function columnType(int $column): int|false {}
    public function fetchArray(int $mode = SQLITE3_BOTH): array|false {}
    public function fetchAll(int $mode = SQLITE3_BOTH): array|false {}
    public function reset(): bool {}
    public function finalize(): bool {}
}"#;

const EXCEPTION_CLASS: &str = r#"<?php
class SQLite3Exception extends Exception {}"#;

static METHODS: &[(&str, NativeFn)] = &[
    ("sqlite3::__construct", m_construct),
    ("sqlite3::open", m_open),
    ("sqlite3::close", m_close),
    ("sqlite3::version", m_version),
    ("sqlite3::lastinsertrowid", m_last_insert_rowid),
    ("sqlite3::lasterrorcode", m_last_error_code),
    ("sqlite3::lastextendederrorcode", m_last_extended_error_code),
    ("sqlite3::lasterrormsg", m_last_error_msg),
    ("sqlite3::changes", m_changes),
    ("sqlite3::busytimeout", m_busy_timeout),
    ("sqlite3::loadextension", m_load_extension),
    ("sqlite3::backup", m_backup),
    ("sqlite3::escapestring", m_escape_string),
    ("sqlite3::prepare", m_prepare),
    ("sqlite3::exec", m_exec),
    ("sqlite3::query", m_query),
    ("sqlite3::querysingle", m_query_single),
    ("sqlite3::createfunction", m_create_function),
    ("sqlite3::createaggregate", m_create_aggregate),
    ("sqlite3::createcollation", m_create_collation),
    ("sqlite3::openblob", blob::m_open_blob),
    ("sqlite3::enableexceptions", m_enable_exceptions),
    (
        "sqlite3::enableextendedresultcodes",
        m_enable_extended_result_codes,
    ),
    ("sqlite3::setauthorizer", m_set_authorizer),
    ("sqlite3stmt::bindparam", stmt::m_bind_param),
    ("sqlite3stmt::bindvalue", stmt::m_bind_value),
    ("sqlite3stmt::clear", stmt::m_clear),
    ("sqlite3stmt::close", stmt::m_close),
    ("sqlite3stmt::execute", stmt::m_execute),
    ("sqlite3stmt::getsql", stmt::m_get_sql),
    ("sqlite3stmt::paramcount", stmt::m_param_count),
    ("sqlite3stmt::readonly", stmt::m_read_only),
    ("sqlite3stmt::reset", stmt::m_reset),
    ("sqlite3stmt::busy", stmt::m_busy),
    ("sqlite3stmt::explain", stmt::m_explain),
    ("sqlite3stmt::setexplain", stmt::m_set_explain),
    ("sqlite3result::numcolumns", stmt::r_num_columns),
    ("sqlite3result::columnname", stmt::r_column_name),
    ("sqlite3result::columntype", stmt::r_column_type),
    ("sqlite3result::fetcharray", stmt::r_fetch_array),
    ("sqlite3result::fetchall", stmt::r_fetch_all),
    ("sqlite3result::reset", stmt::r_reset),
    ("sqlite3result::finalize", stmt::r_finalize),
];

/// Constants (PHP manual: SQLite3 predefined constants).
pub(super) const ASSOC: i64 = 1;
pub(super) const NUM: i64 = 2;
pub(super) const BOTH: i64 = 3;
pub(super) const T_INTEGER: i64 = 1;
pub(super) const T_FLOAT: i64 = 2;
pub(super) const T_TEXT: i64 = 3;
pub(super) const T_BLOB: i64 = 4;
pub(super) const T_NULL: i64 = 5;
const OPEN_READONLY: i64 = 1;
const OPEN_READWRITE: i64 = 2;
const OPEN_CREATE: i64 = 4;
const DETERMINISTIC: i64 = 0x800;

pub static MODULE: Module = Module {
    name: "sqlite3",
    classes: &[
        ("sqlite3", SQLITE3_CLASS),
        ("sqlite3stmt", STMT_CLASS),
        ("sqlite3result", RESULT_CLASS),
        ("sqlite3exception", EXCEPTION_CLASS),
    ],
    methods: METHODS,
    hooks: &[
        ("sqlite3", db_hooks),
        ("sqlite3stmt", stmt::stmt_hooks),
        ("sqlite3result", stmt::result_hooks),
    ],
    constants: &[
        ("SQLITE3_ASSOC", ConstVal::Int(ASSOC)),
        ("SQLITE3_NUM", ConstVal::Int(NUM)),
        ("SQLITE3_BOTH", ConstVal::Int(BOTH)),
        ("SQLITE3_INTEGER", ConstVal::Int(T_INTEGER)),
        ("SQLITE3_FLOAT", ConstVal::Int(T_FLOAT)),
        ("SQLITE3_TEXT", ConstVal::Int(T_TEXT)),
        ("SQLITE3_BLOB", ConstVal::Int(T_BLOB)),
        ("SQLITE3_NULL", ConstVal::Int(T_NULL)),
        ("SQLITE3_OPEN_READONLY", ConstVal::Int(OPEN_READONLY)),
        ("SQLITE3_OPEN_READWRITE", ConstVal::Int(OPEN_READWRITE)),
        ("SQLITE3_OPEN_CREATE", ConstVal::Int(OPEN_CREATE)),
        ("SQLITE3_DETERMINISTIC", ConstVal::Int(DETERMINISTIC)),
    ],
    // PHP manual: SQLite3 runtime configuration.
    ini: &[("sqlite3.extension_dir", ""), ("sqlite3.defensive", "1")],
    info: Some(info),
    ..Module::EMPTY
};

fn info(_vm: &mut Vm) -> String {
    format!(
        "SQLite3 support => enabled\nSQLite Library => {}\n",
        gs::VERSION
    )
}

// ------------------------------------------------------------------ state

/// A connection, shared by the `SQLite3` object and its statements and
/// results (closing the database finalizes them all).
pub(super) struct Conn {
    /// `None` once closed.
    pub db: RefCell<Option<Db>>,
    /// Bumped by every statement that may write: cached rows of an
    /// earlier run are stale after it.
    pub generation: Cell<u64>,
    /// User functions, aggregates, collations or an authorizer are
    /// registered: statements must really run again (their callbacks are
    /// observable).
    pub callbacks: Cell<bool>,
    /// The last error (`sqlite3_errcode`, `sqlite3_extended_errcode`,
    /// `sqlite3_errmsg`).
    pub err: RefCell<SqlError>,
    pub extended: Cell<bool>,
    pub exceptions: Cell<bool>,
}

impl Conn {
    pub fn is_open(&self) -> bool {
        self.db.borrow().is_some()
    }

    pub fn set_err(&self, e: SqlError) {
        *self.err.borrow_mut() = e;
    }

    pub fn ok(&self) {
        self.set_err(SqlError::ok());
    }

    /// The error code as reported (extended codes when enabled).
    pub fn code(&self) -> i32 {
        let e = self.err.borrow();
        if self.extended.get() {
            e.ext_code
        } else {
            e.code
        }
    }
}

#[derive(Default)]
struct DbObj {
    conn: Option<Rc<Conn>>,
    /// The PHP callbacks the connection holds (for the cycle collector).
    callbacks: Vec<Value>,
}

impl goro_runtime::NativeData for DbObj {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        for v in &self.callbacks {
            visit(v);
        }
    }
    fn clear(&mut self) {
        self.callbacks.clear();
        if let Some(c) = &self.conn
            && let Ok(mut d) = c.db.try_borrow_mut()
        {
            *d = None;
        }
    }
}

fn db_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_vm, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(DbObj::default()));
        }),
        clone: Some(|_, _, to| {
            *to.obj().native.borrow_mut() = Some(Box::new(DbObj::default()));
        }),
        ..Default::default()
    }
}

pub(super) fn with_native<T: 'static, U>(o: &ObjRef, f: impl FnOnce(&mut T) -> U) -> Option<U> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<T>().map(f)
}

const NOT_INIT: &str = "The SQLite3 object has not been correctly initialised or is already closed";

/// The connection of an `SQLite3` object (any state).
fn conn_of(o: &ObjRef) -> Option<Rc<Conn>> {
    with_native(o, |d: &mut DbObj| d.conn.clone()).flatten()
}

/// The open connection of `$this`, or the "not initialised" Error.
fn open_conn(vm: &mut Vm, a: &Args) -> R<Rc<Conn>> {
    open_conn_of(vm, a.this_obj())
}

pub(super) fn open_conn_of(vm: &mut Vm, o: &ObjRef) -> R<Rc<Conn>> {
    match conn_of(o) {
        Some(c) if c.is_open() => Ok(c),
        _ => vm.throw("Error", NOT_INIT),
    }
}

fn keep_callback(o: &ObjRef, v: &Value) {
    with_native(o, |d: &mut DbObj| d.callbacks.push(v.clone()));
}

// ------------------------------------------------------------------ errors

/// Reports a failed call: a warning ("SQLite3::query(): msg"), or with
/// exceptions enabled an `SQLite3Exception` carrying the error code. An
/// exception a callback raised during the call is rethrown (in warning
/// mode) or chained as the previous exception (tests bug72668.phpt,
/// sqlite3_40_setauthorizer.phpt).
pub(super) fn fail(vm: &mut Vm, a: &Args, conn: &Conn, msg: &str) -> R<()> {
    let pending = common::take_pending();
    if conn.exceptions.get() {
        let code = conn.code();
        let e = vm.create_exception("SQLite3Exception", msg, code as i64)?;
        if let Some(p) = pending {
            match p {
                Unwind::Throw(prev) => vm.set_prop_raw(&e, b"previous", Value::Object(prev)),
                other => return Err(other),
            }
        }
        return Err(Unwind::Throw(e));
    }
    if let Some(p) = pending {
        return Err(p);
    }
    vm.fwarn(a, msg)
}

/// A warning or exception that is not an SQLite error.
pub(super) fn complain(vm: &mut Vm, a: &Args, conn: &Conn, msg: &str) -> R<()> {
    if conn.exceptions.get() {
        let e = vm.create_exception("SQLite3Exception", msg, 0)?;
        return Err(Unwind::Throw(e));
    }
    vm.fwarn(a, msg)
}

/// Extended result codes graphitesql's messages don't tell apart: a
/// UNIQUE failure on the table's PRIMARY KEY (or rowid alias) is
/// SQLITE_CONSTRAINT_PRIMARYKEY (rescode.html; test
/// sqlite3_38_extended_error.phpt).
fn refine(db: &Db, e: &mut SqlError) {
    if e.ext_code != 2067 {
        return;
    }
    let Some(list) = e.msg.strip_prefix("UNIQUE constraint failed: ") else {
        return;
    };
    let mut table = None;
    let mut cols = Vec::new();
    for part in list.split(", ") {
        let Some((t, c)) = part.split_once('.') else {
            return;
        };
        if table.is_some_and(|x| x != t) {
            return;
        }
        table = Some(t);
        cols.push(c.to_ascii_lowercase());
    }
    let Some(table) = table else { return };
    let q = format!("PRAGMA table_info({})", scan::quote_ident(table));
    let Ok(r) = db.conn.query(&q) else { return };
    let mut pk: Vec<(i64, String)> = r
        .rows
        .iter()
        .filter_map(|row| {
            let n = match row.get(5)? {
                SqlValue::Integer(n) if *n > 0 => *n,
                _ => return None,
            };
            let name = match row.get(1)? {
                SqlValue::Text(t) => t.to_str_lossy().to_ascii_lowercase(),
                _ => return None,
            };
            Some((n, name))
        })
        .collect();
    pk.sort();
    let pk: Vec<String> = pk.into_iter().map(|(_, n)| n).collect();
    if !pk.is_empty() && pk == cols {
        e.ext_code = 1555;
    }
}

// ------------------------------------------------------------------ running

/// The text of the first statement of `sql` as `sqlite3_sql()` gives it:
/// from the start through its `;`.
fn statement_text(sql: &str) -> String {
    let (_, end, _) = scan::first_statement(sql);
    let mut t = sql[..end].to_string();
    if sql[end..].starts_with(';') {
        t.push(';');
    }
    t
}

/// `sqlite3_prepare_v2()` on the connection, with the checks SQLite makes
/// at prepare time that graphitesql doesn't: the schema table is not
/// writable (sqlite3.defensive; test sqlite3_defensive.phpt), and with
/// open_basedir set, ATTACH may only name in-memory databases or files
/// inside it (tests bug81742.phpt, gh9032.phpt).
pub(super) fn prepare(
    vm: &mut Vm,
    conn: &Conn,
    sql: &str,
) -> Result<Option<(Stmt, String, usize)>, SqlError> {
    let r = {
        let _g = common::enter(vm);
        let db = conn.db.try_borrow();
        match db.as_ref().ok().and_then(|d| d.as_ref()) {
            Some(db) => prepare_dqs(db, sql),
            None => Err(SqlError::new(gs::SQLITE_MISUSE, gs::errstr(gs::SQLITE_MISUSE))),
        }
    };
    if common::has_pending() {
        return Err(SqlError::new(gs::SQLITE_AUTH, "not authorized"));
    }
    let (st, tail) = r?;
    let Some(st) = st else {
        return Ok(None);
    };
    check_extra(vm, &st)?;
    Ok(Some((st, statement_text(sql), tail)))
}

fn check_extra(vm: &mut Vm, st: &Stmt) -> Result<(), SqlError> {
    use goro_ext_sqlite::graphitesql::sql::ast::{Expr, Literal, Statement};
    let kw = scan::first_keyword(&st.sql);
    let dml = matches!(
        kw.as_str(),
        "INSERT" | "UPDATE" | "DELETE" | "REPLACE" | "WITH"
    );
    if kw != "ATTACH" && !(dml && st.sql.to_ascii_lowercase().contains("sqlite_")) {
        return Ok(());
    }
    let Ok(ast) = goro_ext_sqlite::graphitesql::sql::parse_one(&st.sql) else {
        return Ok(());
    };
    let target = match &ast {
        Statement::Insert(i) => Some(&i.table),
        Statement::Update(u) => Some(&u.table),
        Statement::Delete(d) => Some(&d.table),
        Statement::Attach { file, .. } => {
            let basedir = vm.get_ini(b"open_basedir").is_some_and(|b| !b.is_empty());
            if !basedir {
                return Ok(());
            }
            let allowed = match file {
                Expr::Literal(Literal::Str(f)) => {
                    f.is_empty()
                        || f == ":memory:"
                        || (!f.starts_with("file:") && vm.basedir_allows(f.as_bytes()))
                }
                _ => false,
            };
            if allowed {
                return Ok(());
            }
            return Err(SqlError::new(gs::SQLITE_AUTH, "not authorized"));
        }
        _ => None,
    };
    if let Some(t) = target {
        let l = t.to_ascii_lowercase();
        if matches!(
            l.as_str(),
            "sqlite_master" | "sqlite_schema" | "sqlite_temp_master" | "sqlite_temp_schema"
        ) {
            return Err(SqlError::new(
                gs::SQLITE_ERROR,
                format!("table {t} may not be modified"),
            ));
        }
    }
    Ok(())
}

/// A double-quoted name that is no column: SQLite reads it as a string
/// literal (quirks.html "Double-quoted String Literals Are Accepted";
/// tests sqlite3_29_createfunction.phpt, sqlite3_rename_column.phpt).
/// graphitesql reports it as an unknown column; the statement is
/// retried with that name as a '…' literal.
fn dqs_fix(sql: &str, e: &SqlError) -> Option<String> {
    let name = e
        .msg
        .strip_prefix("no such column: \"")?
        .strip_suffix("\" - should this be a string literal in single-quotes?")?;
    let quoted = format!("\"{}\"", name.replace('"', "\"\""));
    let lit = format!("'{}'", name.replace('\'', "''"));
    let b = sql.as_bytes();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    let mut changed = false;
    while i < b.len() {
        match b[i] {
            b'\'' => {
                let mut j = i + 1;
                while j < b.len() {
                    if b[j] == b'\'' {
                        if b.get(j + 1) == Some(&b'\'') {
                            j += 2;
                            continue;
                        }
                        break;
                    }
                    j += 1;
                }
                let j = (j + 1).min(b.len());
                out.push_str(&sql[i..j]);
                i = j;
            }
            b'"' if sql[i..].starts_with(&quoted) => {
                out.push_str(&lit);
                i += quoted.len();
                changed = true;
            }
            _ => {
                let ch = sql[i..].chars().next().unwrap_or('\0');
                out.push(ch);
                i += ch.len_utf8().max(1);
            }
        }
    }
    changed.then_some(out)
}

fn prepare_dqs(db: &Db, sql: &str) -> Result<(Option<Stmt>, usize), SqlError> {
    let mut err = match db.prepare(sql) {
        Err(e) => e,
        ok => return ok,
    };
    let (start, end, tail) = scan::first_statement(sql);
    let mut text = sql[start..end].to_string();
    for _ in 0..16 {
        let Some(fixed) = dqs_fix(&text, &err) else {
            break;
        };
        text = fixed;
        match db.prepare(&text) {
            Ok((st, _)) => return Ok((st, tail)),
            Err(e) => err = e,
        }
    }
    Err(err)
}

fn run_dqs(db: &mut Db, st: &Stmt, values: &[SqlValue]) -> Result<gs::Rows, SqlError> {
    let mut err = match db.run(st, values) {
        Err(e) => e,
        ok => return ok,
    };
    let mut st = st.clone();
    for _ in 0..16 {
        let Some(fixed) = dqs_fix(&st.sql, &err) else {
            break;
        };
        st.sql = fixed;
        match db.run(&st, values) {
            Err(e) => err = e,
            ok => return ok,
        }
    }
    Err(err)
}

/// Runs a prepared statement. An exception raised by a callback makes it
/// fail (the caller rethrows it through [`fail`]).
pub(super) fn run(
    vm: &mut Vm,
    conn: &Conn,
    st: &Stmt,
    values: &[SqlValue],
) -> Result<gs::Rows, SqlError> {
    let r = {
        let _g = common::enter(vm);
        match conn.db.try_borrow_mut() {
            Ok(mut d) => match d.as_mut() {
                Some(db) => {
                    let mut r = run_dqs(db, st, values);
                    if let Err(e) = &mut r {
                        refine(db, e);
                    }
                    r
                }
                None => Err(SqlError::new(gs::SQLITE_MISUSE, gs::errstr(gs::SQLITE_MISUSE))),
            },
            // A callback running SQL on the connection that runs it.
            Err(_) => Err(SqlError::new(gs::SQLITE_LOCKED, gs::errstr(gs::SQLITE_LOCKED))),
        }
    };
    if !st.readonly {
        conn.generation.set(conn.generation.get() + 1);
    }
    match r {
        Ok(_) if common::has_pending() => {
            Err(SqlError::new(gs::SQLITE_ERROR, common::CALLBACK_THREW))
        }
        r => r,
    }
}

/// Whether a run failed in the authorizer: SQLite refuses such statements
/// when they are prepared ("Unable to prepare statement: not authorized").
pub(super) fn is_auth(e: &SqlError) -> bool {
    e.code == gs::SQLITE_AUTH
}

/// Runs every statement of a script (`sqlite3_exec`); the error message
/// of the first failing one.
fn exec_script(vm: &mut Vm, conn: &Conn, sql: &str) -> Result<(), SqlError> {
    let mut rest = sql;
    loop {
        let r = prepare(vm, conn, rest)?;
        let tail = match r {
            Some((st, _, tail)) => {
                run(vm, conn, &st, &[])?;
                tail
            }
            None => rest.len(),
        };
        if tail >= rest.len() {
            return Ok(());
        }
        rest = &rest[tail..];
    }
}

pub(super) fn sql_str(s: &PhpStr) -> String {
    String::from_utf8_lossy(s.as_bytes()).into_owned()
}

// ------------------------------------------------------------------ SQLite3

fn m_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    do_open(vm, a)?;
    Ok(Value::Null)
}

fn m_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    do_open(vm, a)?;
    Ok(Value::Null)
}

/// `SQLite3::open()` (PHP manual): errors are exceptions whatever the
/// error mode (tests sqlite3_15_open_error.phpt, sqlite3_21_security.phpt,
/// sqlite3_31_open.phpt).
fn do_open(vm: &mut Vm, a: &mut Args) -> R<()> {
    let o = a.this_obj().clone();
    if conn_of(&o).is_some_and(|c| c.is_open()) {
        return vm.throw("Exception", "Already initialised DB Object");
    }
    let name = a.str(0);
    let flags = a.int_or(1, OPEN_READWRITE | OPEN_CREATE);
    if name.as_bytes().contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($filename) must not contain any null bytes",
        );
    }
    let raw = sql_str(&name);
    let path = if raw.is_empty() || raw == ":memory:" || raw.starts_with("file:") {
        raw.clone()
    } else {
        if !vm.basedir_allows(name.as_bytes()) {
            let bd = vm
                .get_ini(b"open_basedir")
                .map(|v| v.to_string_lossy())
                .unwrap_or_default();
            vm.fwarn(
                a,
                &format!(
                    "open_basedir restriction in effect. File({raw}) is not within the allowed path(s): ({bd})"
                ),
            )?;
            return vm.throw("Exception", &format!("open_basedir prohibits opening {raw}"));
        }
        vm.fs_path(name.as_bytes()).to_string_lossy().into_owned()
    };
    let db = match Db::open(&path, flags as i32) {
        Ok(db) => db,
        Err(e) => {
            return vm.throw("Exception", &format!("Unable to open database: {}", e.msg));
        }
    };
    let conn = Rc::new(Conn {
        db: RefCell::new(Some(db)),
        generation: Cell::new(0),
        callbacks: Cell::new(false),
        err: RefCell::new(SqlError::ok()),
        extended: Cell::new(false),
        exceptions: Cell::new(false),
    });
    // sqlite3.defensive (PHP manual): SQLITE_DBCONFIG_DEFENSIVE; the
    // schema stays read-only (see `check_extra`).
    with_native(&o, |d: &mut DbObj| {
        d.conn = Some(conn);
        d.callbacks.clear();
    });
    Ok(())
}

fn m_close(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let Some(conn) = conn_of(&o) else {
        return vm.throw("Error", NOT_INIT);
    };
    match conn.db.try_borrow_mut() {
        Ok(mut d) => {
            let old = d.take();
            drop(d);
            drop(old);
        }
        Err(_) => {
            vm.fwarn(a, "Unable to close database: database is locked")?;
            return Ok(Value::Bool(false));
        }
    }
    conn.set_err(SqlError {
        code: 0,
        ext_code: 0,
        msg: String::new(),
    });
    with_native(&o, |d: &mut DbObj| d.callbacks.clear());
    Ok(Value::Bool(true))
}

fn m_version(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut arr = PhpArray::new();
    arr.insert(Key::from_bytes(b"versionString"), Value::from(gs::VERSION));
    arr.insert(
        Key::from_bytes(b"versionNumber"),
        Value::Int(gs::version_number()),
    );
    Ok(Value::Array(arr))
}

fn m_last_insert_rowid(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let r = c.db.borrow().as_ref().map_or(0, |d| d.last_insert_rowid());
    Ok(Value::Int(r))
}

/// The last error (0 / "" once closed: test bug69972.phpt).
fn last_err(vm: &mut Vm, a: &Args) -> R<Option<Rc<Conn>>> {
    match conn_of(a.this_obj()) {
        Some(c) => Ok(Some(c)),
        None => vm.throw("Error", NOT_INIT),
    }
}

fn m_last_error_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = last_err(vm, a)?;
    Ok(Value::Int(c.map_or(0, |c| c.code() as i64)))
}

fn m_last_extended_error_code(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = last_err(vm, a)?;
    Ok(Value::Int(c.map_or(0, |c| c.err.borrow().ext_code as i64)))
}

fn m_last_error_msg(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = last_err(vm, a)?;
    Ok(Value::from(
        c.map(|c| c.err.borrow().msg.clone())
            .unwrap_or_default()
            .as_str(),
    ))
}

fn m_changes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let r = c.db.borrow().as_ref().map_or(0, |d| d.changes());
    Ok(Value::Int(r))
}

fn m_busy_timeout(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let ms = a.int(0);
    if let Some(d) = c.db.borrow_mut().as_mut() {
        d.busy_timeout(ms);
    }
    Ok(Value::Bool(true))
}

/// `SQLite3::loadExtension()` (PHP manual): extensions load from
/// sqlite3.extension_dir only. graphitesql can't load native SQLite
/// extensions, so loading always fails with the message SQLite gives a
/// file that isn't one (tests sqlite3_22_loadextension.phpt,
/// sqlite3_33_load_extension_param.phpt, sqlite3_34_load_extension_ext_dir.phpt).
fn m_load_extension(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let dir = vm
        .get_ini(b"sqlite3.extension_dir")
        .map(|v| v.to_string_lossy())
        .unwrap_or_default();
    if dir.is_empty() {
        complain(vm, a, &c, "SQLite Extensions are disabled")?;
        return Ok(Value::Bool(false));
    }
    let name = a.str(0);
    if name.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($name) must not be empty");
    }
    let path = format!("{}/{}", dir, name.to_string_lossy());
    let full = vm.fs_path(path.as_bytes());
    let dir_full = vm.fs_path(dir.as_bytes());
    let inside = match (std::fs::canonicalize(&full), std::fs::canonicalize(&dir_full)) {
        (Ok(f), Ok(d)) => f.starts_with(&d),
        _ => false,
    };
    if !inside && std::fs::canonicalize(&full).is_ok() {
        complain(
            vm,
            a,
            &c,
            "Unable to open extensions outside the defined directory",
        )?;
        return Ok(Value::Bool(false));
    }
    complain(vm, a, &c, &format!("Unable to load extension at '{path}'"))?;
    Ok(Value::Bool(false))
}

/// `SQLite3::backup()` (PHP manual): copies the database image into the
/// destination connection (graphitesql has no incremental backup API:
/// serialize/restore). A source inside a transaction is busy (test
/// sqlite3_38_backup.phpt).
fn m_backup(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let Some(dst_obj) = a.obj(0) else {
        return Ok(Value::Bool(false));
    };
    let d = open_conn_of(vm, &dst_obj)?;
    let src_name = a.str(1);
    let dst_name = a.str(2);
    if Rc::ptr_eq(&c, &d) && src_name == dst_name {
        complain(vm, a, &c, "Backup failed: source and destination are the same")?;
        return Ok(Value::Bool(false));
    }
    for n in [&src_name, &dst_name] {
        if !n.as_bytes().eq_ignore_ascii_case(b"main") {
            let msg = format!("Backup failed: unknown database {}", n.to_string_lossy());
            complain(vm, a, &c, &msg)?;
            return Ok(Value::Bool(false));
        }
    }
    let image = {
        let db = c.db.borrow();
        let db = db.as_ref().expect("open");
        if !db.autocommit() {
            Err(SqlError::new(gs::SQLITE_BUSY, "source database is busy"))
        } else {
            db.serialize()
        }
    };
    let r = image.and_then(|img| match d.db.try_borrow_mut() {
        Ok(mut g) => g.as_mut().expect("open").restore(&img),
        Err(_) => Err(SqlError::new(gs::SQLITE_LOCKED, gs::errstr(gs::SQLITE_LOCKED))),
    });
    match r {
        Ok(()) => {
            d.generation.set(d.generation.get() + 1);
            Ok(Value::Bool(true))
        }
        Err(e) => {
            let msg = format!("Backup failed: {}", e.msg);
            c.set_err(e);
            fail(vm, a, &c, &msg)?;
            Ok(Value::Bool(false))
        }
    }
}

fn m_escape_string(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    Ok(Value::from(scan::escape_string(s.as_bytes()).as_slice()))
}

fn m_prepare(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let sql = sql_str(&a.str(0));
    if sql.is_empty() {
        return Ok(Value::Bool(false));
    }
    match prepare(vm, &c, &sql) {
        Ok(Some((st, text, _))) => {
            c.ok();
            let db_obj = a.this_obj().clone();
            stmt::new_stmt(vm, c, db_obj, st, text).map(Value::Object)
        }
        Ok(None) => Ok(Value::Bool(false)),
        Err(e) => {
            let msg = format!("Unable to prepare statement: {}", e.msg);
            c.set_err(e);
            fail(vm, a, &c, &msg)?;
            Ok(Value::Bool(false))
        }
    }
}

fn m_exec(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let sql = sql_str(&a.str(0));
    match exec_script(vm, &c, &sql) {
        Ok(()) => {
            c.ok();
            Ok(Value::Bool(true))
        }
        Err(e) => {
            let msg = e.msg.clone();
            c.set_err(e);
            fail(vm, a, &c, &msg)?;
            Ok(Value::Bool(false))
        }
    }
}

/// `SQLite3::query()`: prepares and runs the first statement and gives
/// its result. When the result is not used, the whole script runs as
/// `exec()` does (its diagnostics are SQLite's bare messages).
fn m_query(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let sql = sql_str(&a.str(0));
    if sql.is_empty() {
        return Ok(Value::Bool(false));
    }
    if vm.caller_discards_result() {
        if let Err(e) = exec_script(vm, &c, &sql) {
            let msg = e.msg.clone();
            c.set_err(e);
            fail(vm, a, &c, &msg)?;
        } else {
            c.ok();
        }
        return Ok(Value::Bool(false));
    }
    let (st, text) = match prepare(vm, &c, &sql) {
        Ok(Some((st, text, _))) => (st, text),
        Ok(None) => return Ok(Value::Bool(false)),
        Err(e) => {
            let msg = format!("Unable to prepare statement: {}", e.msg);
            c.set_err(e);
            fail(vm, a, &c, &msg)?;
            return Ok(Value::Bool(false));
        }
    };
    let state = stmt::StmtState::new(c.clone(), st, text);
    let state = Rc::new(RefCell::new(state));
    match stmt::execute_state(vm, &state)? {
        Ok(()) => {
            c.ok();
            stmt::new_result(vm, state, None, true).map(Value::Object)
        }
        Err(e) => {
            let pre = if is_auth(&e) {
                "Unable to prepare statement"
            } else {
                "Unable to execute statement"
            };
            let msg = format!("{pre}: {}", e.msg);
            c.set_err(e);
            fail(vm, a, &c, &msg)?;
            Ok(Value::Bool(false))
        }
    }
}

fn m_query_single(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let sql = sql_str(&a.str(0));
    let entire = a.bool_or(1, false);
    if sql.is_empty() {
        return Ok(Value::Bool(false));
    }
    let st = match prepare(vm, &c, &sql) {
        Ok(Some((st, _, _))) => st,
        Ok(None) => return Ok(Value::Bool(false)),
        Err(e) => {
            let msg = format!("Unable to prepare statement: {}", e.msg);
            c.set_err(e);
            fail(vm, a, &c, &msg)?;
            return Ok(Value::Bool(false));
        }
    };
    let rows = match run(vm, &c, &st, &[]) {
        Ok(r) => r,
        Err(e) => {
            let pre = if is_auth(&e) {
                "Unable to prepare statement"
            } else {
                "Unable to execute statement"
            };
            let msg = format!("{pre}: {}", e.msg);
            c.set_err(e);
            fail(vm, a, &c, &msg)?;
            return Ok(Value::Bool(false));
        }
    };
    c.ok();
    let Some(row) = rows.rows.first() else {
        return Ok(if entire {
            Value::Array(PhpArray::new())
        } else {
            Value::Null
        });
    };
    if entire {
        return Ok(Value::Array(stmt::row_array(&rows.columns, row, ASSOC)));
    }
    Ok(row.first().map(common::to_php).unwrap_or(Value::Null))
}

/// Registered names: an empty one is refused (tests
/// sqlite3_trampoline_*_no_leak.phpt).
fn m_create_function(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let name = sql_str(&a.str(0));
    let cb = a.val(1);
    let argc = a.int_or(2, -1);
    if name.is_empty() {
        return Ok(Value::Bool(false));
    }
    if let Some(d) = c.db.borrow_mut().as_mut() {
        d.create_function(&name, common::scalar(&name, cb.clone(), argc));
    }
    c.callbacks.set(true);
    keep_callback(a.this_obj(), &cb);
    Ok(Value::Bool(true))
}

fn m_create_aggregate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let name = sql_str(&a.str(0));
    let step = a.val(1);
    let fin = a.val(2);
    let argc = a.int_or(3, -1);
    if name.is_empty() {
        return Ok(Value::Bool(false));
    }
    if let Some(d) = c.db.borrow_mut().as_mut() {
        d.create_aggregate(
            &name,
            common::aggregate(&name, step.clone(), fin.clone(), argc),
        );
    }
    c.callbacks.set(true);
    keep_callback(a.this_obj(), &step);
    keep_callback(a.this_obj(), &fin);
    Ok(Value::Bool(true))
}

fn m_create_collation(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let name = sql_str(&a.str(0));
    let cb = a.val(1);
    if name.is_empty() {
        return Ok(Value::Bool(false));
    }
    if let Some(d) = c.db.borrow_mut().as_mut() {
        d.create_collation(&name, common::collation(cb.clone()));
    }
    c.callbacks.set(true);
    keep_callback(a.this_obj(), &cb);
    Ok(Value::Bool(true))
}

fn m_enable_exceptions(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let on = a.bool_or(0, false);
    if !on {
        // PHP 8.5 (test sqlite3_enable_exceptions.phpt).
        vm.fdeprecated(a, "Use of warnings for SQLite3 is deprecated")?;
    }
    Ok(Value::Bool(c.exceptions.replace(on)))
}

fn m_enable_extended_result_codes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    c.extended.set(a.bool_or(0, true));
    Ok(Value::Bool(true))
}

/// `SQLite3::setAuthorizer()` (PHP manual): the callback gets the action
/// code and four strings or nulls, and returns SQLite3::OK, DENY or
/// IGNORE; any other return denies, with the reason chained to the error
/// (test sqlite3_40_setauthorizer.phpt).
fn m_set_authorizer(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = open_conn(vm, a)?;
    let cb = a.val(0);
    let db = c.db.borrow();
    let Some(d) = db.as_ref() else {
        return Ok(Value::Bool(false));
    };
    if matches!(cb.deref(), Value::Null) {
        d.set_authorizer(None);
        return Ok(Value::Bool(true));
    }
    let cb2 = cb.clone();
    d.set_authorizer(Some(Box::new(move |action, a1, a2, a3, a4| {
        let s = |x: Option<&str>| x.map_or(Value::Null, Value::from);
        let args = vec![Value::Int(action as i64), s(a1), s(a2), s(a3), s(a4)];
        match common::call(&cb2, args) {
            Ok(v) => match v.deref() {
                Value::Int(n) if (0..=2).contains(&n) => n as i32,
                Value::Int(n) => {
                    let msg = format!("The authorizer callback returned an invalid value: {n}");
                    if let Some(u) = common::with_vm(|vm| vm.throw_error_msg("ValueError", &msg)) {
                        common::set_pending(u);
                    }
                    1
                }
                _ => {
                    let msg = "The authorizer callback returned an invalid type: expected int";
                    if let Some(u) = common::with_vm(|vm| vm.throw_error_msg("TypeError", msg)) {
                        common::set_pending(u);
                    }
                    1
                }
            },
            Err(_) => 1,
        }
    })));
    drop(db);
    c.callbacks.set(true);
    keep_callback(a.this_obj(), &cb);
    Ok(Value::Bool(true))
}
