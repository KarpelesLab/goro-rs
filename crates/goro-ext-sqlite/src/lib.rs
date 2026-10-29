//! SQLite connections for ext/sqlite3 and pdo_sqlite, on graphitesql (a
//! pure-Rust SQLite reimplementation that reads and writes the SQLite 3
//! file format).
//!
//! This layer gives the extensions the shape of SQLite's C interface as
//! documented on sqlite.org (c3ref): `sqlite3_open_v2` flags and URI
//! filenames, prepared statements with a tail, parameter numbering and
//! names, statement read-only-ness, result codes with extended codes and
//! `sqlite3_errmsg` texts. Statements run to completion when first
//! stepped: their rows are kept and handed out one at a time.

pub mod scan;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub use graphitesql;
use graphitesql::sql::ast::{ResultColumn, Statement};
use graphitesql::{Connection, Value};

pub use scan::Param;

/// `sqlite3_open_v2()` flags (c3ref/c_open_autoproxy.html).
pub const OPEN_READONLY: i32 = 0x1;
pub const OPEN_READWRITE: i32 = 0x2;
pub const OPEN_CREATE: i32 = 0x4;
pub const OPEN_URI: i32 = 0x40;
pub const OPEN_MEMORY: i32 = 0x80;

/// Primary result codes (rescode.html).
pub const SQLITE_OK: i32 = 0;
pub const SQLITE_ERROR: i32 = 1;
pub const SQLITE_INTERNAL: i32 = 2;
pub const SQLITE_PERM: i32 = 3;
pub const SQLITE_ABORT: i32 = 4;
pub const SQLITE_BUSY: i32 = 5;
pub const SQLITE_LOCKED: i32 = 6;
pub const SQLITE_NOMEM: i32 = 7;
pub const SQLITE_READONLY: i32 = 8;
pub const SQLITE_INTERRUPT: i32 = 9;
pub const SQLITE_IOERR: i32 = 10;
pub const SQLITE_CORRUPT: i32 = 11;
pub const SQLITE_NOTFOUND: i32 = 12;
pub const SQLITE_FULL: i32 = 13;
pub const SQLITE_CANTOPEN: i32 = 14;
pub const SQLITE_PROTOCOL: i32 = 15;
pub const SQLITE_EMPTY: i32 = 16;
pub const SQLITE_SCHEMA: i32 = 17;
pub const SQLITE_TOOBIG: i32 = 18;
pub const SQLITE_CONSTRAINT: i32 = 19;
pub const SQLITE_MISMATCH: i32 = 20;
pub const SQLITE_MISUSE: i32 = 21;
pub const SQLITE_NOLFS: i32 = 22;
pub const SQLITE_AUTH: i32 = 23;
pub const SQLITE_FORMAT: i32 = 24;
pub const SQLITE_RANGE: i32 = 25;
pub const SQLITE_NOTADB: i32 = 26;
pub const SQLITE_ROW: i32 = 100;
pub const SQLITE_DONE: i32 = 101;

/// The library version reported to PHP (graphitesql targets this SQLite).
pub const VERSION: &str = graphitesql::TARGET_SQLITE_VERSION;

/// `sqlite3_libversion_number()`: X*1000000 + Y*1000 + Z.
pub fn version_number() -> i64 {
    let mut n = 0i64;
    for (i, p) in VERSION.split('.').take(3).enumerate() {
        let v: i64 = p.parse().unwrap_or(0);
        n += v * [1_000_000, 1_000, 1][i];
    }
    n
}

/// The text of a primary result code (`sqlite3_errstr()`, rescode.html).
pub fn errstr(code: i32) -> &'static str {
    match code & 0xff {
        SQLITE_OK => "not an error",
        SQLITE_ERROR => "SQL logic error",
        SQLITE_INTERNAL => "internal error",
        SQLITE_PERM => "access permission denied",
        SQLITE_ABORT => "query aborted",
        SQLITE_BUSY => "database is locked",
        SQLITE_LOCKED => "database table is locked",
        SQLITE_NOMEM => "out of memory",
        SQLITE_READONLY => "attempt to write a readonly database",
        SQLITE_INTERRUPT => "interrupted",
        SQLITE_IOERR => "disk I/O error",
        SQLITE_CORRUPT => "database disk image is malformed",
        SQLITE_NOTFOUND => "unknown operation",
        SQLITE_FULL => "database or disk is full",
        SQLITE_CANTOPEN => "unable to open database file",
        SQLITE_PROTOCOL => "locking protocol",
        SQLITE_SCHEMA => "database schema has changed",
        SQLITE_TOOBIG => "string or blob too big",
        SQLITE_CONSTRAINT => "constraint failed",
        SQLITE_MISMATCH => "datatype mismatch",
        SQLITE_MISUSE => "bad parameter or other API misuse",
        SQLITE_NOLFS => "large file support is disabled",
        SQLITE_AUTH => "authorization denied",
        SQLITE_RANGE => "column index out of range",
        SQLITE_NOTADB => "file is not a database",
        SQLITE_ROW => "another row available",
        SQLITE_DONE => "no more rows available",
        _ => "unknown error",
    }
}

/// An error: primary code, extended code and `sqlite3_errmsg()` text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlError {
    pub code: i32,
    pub ext_code: i32,
    pub msg: String,
}

impl SqlError {
    pub fn new(code: i32, msg: impl Into<String>) -> SqlError {
        SqlError {
            code,
            ext_code: code,
            msg: msg.into(),
        }
    }

    pub fn ok() -> SqlError {
        SqlError::new(SQLITE_OK, "not an error")
    }

    /// An error raised by a user function (`sqlite3_result_error`).
    pub fn user(msg: impl Into<String>) -> graphitesql::Error {
        graphitesql::Error::Error(msg.into())
    }

    fn from_engine(e: &graphitesql::Error) -> SqlError {
        use graphitesql::Error as E;
        let code = e.code();
        let msg = match e {
            E::Error(m) | E::ErrorAt(m, _) | E::Parse(m) | E::ParseAt(m, _) | E::Constraint(m) => {
                m.clone()
            }
            E::Corrupt(_) => errstr(SQLITE_CORRUPT).to_string(),
            E::Io(_) => errstr(SQLITE_IOERR).to_string(),
            E::Busy => errstr(SQLITE_BUSY).to_string(),
            E::CantOpen(_) => errstr(SQLITE_CANTOPEN).to_string(),
            E::Unsupported(m) => format!("not supported: {m}"),
            other => other.to_string(),
        };
        // graphitesql's text for writes to a read-only connection.
        let msg = if msg == "database is read-only" {
            errstr(SQLITE_READONLY).to_string()
        } else {
            msg
        };
        let code = if msg == "attempt to write a readonly database" {
            SQLITE_READONLY
        } else if msg.starts_with("not authorized") {
            SQLITE_AUTH
        } else if msg == "interrupted" {
            SQLITE_INTERRUPT
        } else {
            code
        };
        // Extended constraint codes (rescode.html#extrc).
        let ext_code = if code == SQLITE_CONSTRAINT {
            if msg.starts_with("UNIQUE constraint failed") {
                2067
            } else if msg.starts_with("NOT NULL constraint failed") {
                1299
            } else if msg.starts_with("CHECK constraint failed") {
                275
            } else if msg.starts_with("FOREIGN KEY constraint failed") {
                787
            } else if msg.starts_with("PRIMARY KEY") {
                1555
            } else if msg.starts_with("cannot store") {
                3091
            } else {
                // RAISE() in a trigger: SQLITE_CONSTRAINT_TRIGGER.
                1811
            }
        } else {
            code
        };
        SqlError {
            code,
            ext_code,
            msg,
        }
    }
}

impl std::fmt::Display for SqlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

/// How a statement runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Produces rows (SELECT, VALUES, read PRAGMA, EXPLAIN).
    Query,
    /// INSERT/UPDATE/DELETE … RETURNING.
    Returning,
    /// Everything else: runs for its effect.
    Exec,
}

/// A prepared statement.
#[derive(Clone, Debug)]
pub struct Stmt {
    /// The statement text (without its `;` or what follows).
    pub sql: String,
    pub kind: Kind,
    /// Parameters by SQLite number (index + 1).
    pub params: Vec<Param>,
    /// `sqlite3_stmt_readonly()`.
    pub readonly: bool,
    /// `sqlite3_stmt_isexplain()`: 0, 1 (EXPLAIN) or 2 (EXPLAIN QUERY PLAN).
    pub explain: u8,
    /// For `ATTACH`: the file when written as a string literal, `None`
    /// when it is an expression (a parameter...). PHP refuses to attach
    /// files outside open_basedir and unknown ones (test gh9032.phpt).
    pub attach: Option<Option<String>>,
}

impl Stmt {
    /// `sqlite3_bind_parameter_index()`: the number of a named parameter
    /// (with its sigil), 0 when there is none.
    pub fn param_index(&self, name: &str) -> usize {
        self.params
            .iter()
            .position(|p| p.name.as_deref() == Some(name))
            .map_or(0, |i| i + 1)
    }
}

/// A finished statement's results.
#[derive(Clone, Debug, Default)]
pub struct Rows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

thread_local! {
    /// Names (lower case) of the user functions registered.
    static UDF_NAMES: RefCell<std::collections::HashSet<String>> =
        RefCell::new(std::collections::HashSet::new());
    /// The resolution calls still expected per user function for the
    /// statement running.
    static DRY_BUDGET: RefCell<HashMap<String, usize>> = RefCell::new(HashMap::new());
    /// Collations registered by name (graphitesql keeps collations in a
    /// process-wide table of `Send` functions; the PHP callbacks live here).
    #[allow(clippy::type_complexity)]
    static COLLATIONS: RefCell<HashMap<String, Rc<dyn Fn(&str, &str) -> std::cmp::Ordering>>> =
        RefCell::new(HashMap::new());
}

/// A database connection.
pub struct Db {
    pub conn: Connection,
    /// The file name as opened (`:memory:` for an in-memory database).
    pub filename: String,
    pub readonly: bool,
    /// Whether this is an in-memory database.
    pub memory: bool,
}

/// What an `open` names.
struct Target {
    path: String,
    memory: bool,
    flags: i32,
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = h {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Reads a `file:` URI (uri.html): the path, and the `mode` and `vfs`
/// query parameters.
fn parse_uri(uri: &str, mut flags: i32) -> Result<Target, SqlError> {
    let rest = &uri[5..];
    let (path_part, query) = match rest.find('?') {
        Some(p) => (&rest[..p], &rest[p + 1..]),
        None => (rest, ""),
    };
    let path_part = path_part.split('#').next().unwrap_or("");
    let path = if let Some(r) = path_part.strip_prefix("//") {
        // file://authority/path: only an empty or "localhost" authority.
        let slash = r.find('/').unwrap_or(r.len());
        let auth = &r[..slash];
        if !auth.is_empty() && auth != "localhost" {
            return Err(SqlError::new(
                SQLITE_ERROR,
                format!("invalid uri authority: {auth}"),
            ));
        }
        &r[slash..]
    } else {
        path_part
    };
    let path = pct_decode(path);
    // An empty path is a private temporary database (uri.html).
    let mut memory = path == ":memory:" || path.is_empty();
    for kv in query.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        let v = pct_decode(v);
        match k {
            "mode" => match v.as_str() {
                "ro" => flags = (flags & !(OPEN_READWRITE | OPEN_CREATE)) | OPEN_READONLY,
                "rw" => flags = (flags & !(OPEN_READONLY | OPEN_CREATE)) | OPEN_READWRITE,
                "rwc" => flags = (flags & !OPEN_READONLY) | OPEN_READWRITE | OPEN_CREATE,
                "memory" => memory = true,
                _ => {
                    return Err(SqlError::new(
                        SQLITE_ERROR,
                        format!("no such access mode: {v}"),
                    ));
                }
            },
            "vfs" if !v.is_empty() => {
                return Err(SqlError::new(SQLITE_ERROR, format!("no such vfs: {v}")));
            }
            _ => {}
        }
    }
    Ok(Target {
        path,
        memory,
        flags,
    })
}

impl Db {
    /// `sqlite3_open_v2(filename, flags)`. An empty name or `:memory:` is
    /// a private in-memory database; a `file:` name is a URI when `flags`
    /// has [`OPEN_URI`].
    pub fn open(filename: &str, flags: i32) -> Result<Db, SqlError> {
        let t = if flags & OPEN_URI != 0 && filename.starts_with("file:") {
            parse_uri(filename, flags)?
        } else {
            Target {
                path: filename.to_string(),
                memory: filename.is_empty() || filename == ":memory:",
                flags,
            }
        };
        let flags = t.flags;
        let readonly = flags & OPEN_READONLY != 0 && flags & OPEN_READWRITE == 0;
        if t.memory || flags & OPEN_MEMORY != 0 {
            let conn = Connection::open_memory().map_err(|e| SqlError::from_engine(&e))?;
            return Ok(Db {
                conn,
                filename: ":memory:".to_string(),
                readonly,
                memory: true,
            });
        }
        let cant = || SqlError::new(SQLITE_CANTOPEN, errstr(SQLITE_CANTOPEN));
        let meta = std::fs::metadata(&t.path);
        if let Ok(m) = &meta
            && m.is_dir()
        {
            return Err(cant());
        }
        let conn = match meta {
            Ok(m) if m.len() > 0 => {
                if readonly {
                    Connection::open_readonly(&t.path)
                } else {
                    Connection::open(&t.path)
                }
            }
            Ok(_) => {
                // An empty file is an empty database (fileformat2.html).
                if readonly {
                    return Connection::open_memory()
                        .map(|conn| Db {
                            conn,
                            filename: t.path.clone(),
                            readonly,
                            memory: false,
                        })
                        .map_err(|e| SqlError::from_engine(&e));
                }
                Connection::create(&t.path)
            }
            Err(_) => {
                if flags & OPEN_CREATE == 0 || readonly {
                    return Err(cant());
                }
                Connection::create(&t.path)
            }
        };
        let conn = conn.map_err(|e| match e {
            graphitesql::Error::CantOpen(_) | graphitesql::Error::Io(_) => cant(),
            other => {
                let mut e = SqlError::from_engine(&other);
                if e.code == SQLITE_ERROR && e.msg.contains("not a database") {
                    e = SqlError::new(SQLITE_NOTADB, errstr(SQLITE_NOTADB));
                }
                e
            }
        })?;
        Ok(Db {
            conn,
            filename: t.path,
            readonly,
            memory: false,
        })
    }

    /// `sqlite3_prepare_v2()`: compiles the first statement of `sql`.
    /// Returns the statement (`None` when `sql` holds none) and the byte
    /// offset of the tail.
    pub fn prepare(&self, sql: &str) -> Result<(Option<Stmt>, usize), SqlError> {
        let (start, end, tail) = scan::first_statement(sql);
        if start == end {
            return Ok((None, tail));
        }
        let mut owned = sql[start..end].to_string();
        let mut tries = 0;
        let ast = loop {
            let ast = graphitesql::sql::parse_one(&owned).map_err(|e| SqlError::from_engine(&e))?;
            match self.check(&ast, &owned) {
                Ok(()) => break ast,
                Err(e) => match dqs_rewrite(&owned, &e) {
                    Some(s) if tries < 32 => {
                        owned = s;
                        tries += 1;
                    }
                    _ => return Err(e),
                },
            }
        };
        let text = owned.as_str();
        let (kind, readonly, explain) = classify(&ast, text);
        let attach = match &ast {
            Statement::Attach { file, .. } => Some(match file {
                graphitesql::sql::ast::Expr::Literal(graphitesql::sql::ast::Literal::Str(s)) => {
                    Some(s.clone())
                }
                _ => None,
            }),
            _ => None,
        };
        Ok((
            Some(Stmt {
                sql: text.to_string(),
                kind,
                params: scan::scan_params(text),
                readonly,
                explain,
                attach,
            }),
            tail,
        ))
    }

    /// Name resolution done at prepare time, as SQLite does: unknown
    /// tables and columns are reported before any row runs.
    fn check(&self, ast: &Statement, text: &str) -> Result<(), SqlError> {
        let probe = |schema: &Option<String>, table: &str| -> Result<(), SqlError> {
            let name = match schema {
                Some(s) => format!("{}.{}", scan::quote_ident(s), scan::quote_ident(table)),
                None => scan::quote_ident(table),
            };
            match self.conn.query(&format!("SELECT 1 FROM {name} WHERE 0")) {
                Ok(_) => Ok(()),
                Err(e) => {
                    let e = SqlError::from_engine(&e);
                    if e.msg.starts_with("no such table") || e.msg.starts_with("unknown database") {
                        let shown = match schema {
                            Some(s) => format!("{s}.{table}"),
                            None => table.to_string(),
                        };
                        if e.msg.starts_with("no such table") {
                            return Err(SqlError::new(
                                SQLITE_ERROR,
                                format!("no such table: {shown}"),
                            ));
                        }
                        return Err(e);
                    }
                    Ok(())
                }
            }
        };
        match ast {
            // EXPLAIN compiles a SELECT without running it; shapes the
            // compiler doesn't take are checked when they run.
            Statement::Select(_) => match self.conn.query(&format!("EXPLAIN {text}")) {
                Err(e) if !matches!(e, graphitesql::Error::Unsupported(_)) => {
                    Err(SqlError::from_engine(&e))
                }
                _ => Ok(()),
            },
            Statement::Insert(i) => probe(&i.schema, &i.table),
            Statement::Update(u) => probe(&u.schema, &u.table),
            Statement::Delete(d) => probe(&d.schema, &d.table),
            _ => Ok(()),
        }
    }

    /// Runs a prepared statement with its parameter values (by SQLite
    /// number; missing ones are NULL).
    pub fn run(&mut self, st: &Stmt, values: &[Value]) -> Result<Rows, SqlError> {
        let mut params = graphitesql::exec::eval::Params::default();
        for (i, p) in st.params.iter().enumerate() {
            let v = values.get(i).cloned().unwrap_or(Value::Null);
            match &p.name {
                Some(n) => params.named.push((n.clone(), v)),
                None => {
                    let pos = p.engine_pos.max(1);
                    if params.positional.len() < pos {
                        params.positional.resize(pos, Value::Null);
                    }
                    params.positional[pos - 1] = v;
                }
            }
        }
        let mut sql = std::borrow::Cow::Borrowed(st.sql.as_str());
        let mut tries = 0;
        loop {
            Self::set_dry_budget(&sql);
            let r = match st.kind {
                Kind::Query => self.conn.query_params(&sql, &params).map(|q| Rows {
                    columns: q.columns,
                    rows: q.rows,
                }),
                Kind::Returning => self.conn.execute_returning(&sql, &params).map(|q| Rows {
                    columns: q.columns,
                    rows: q.rows,
                }),
                Kind::Exec => self
                    .conn
                    .execute_params(&sql, &params)
                    .map(|_| Rows::default()),
            };
            DRY_BUDGET.with(|b| b.borrow_mut().clear());
            match r {
                Ok(rows) => return Ok(rows),
                Err(e) => {
                    let mut e = SqlError::from_engine(&e);
                    match dqs_rewrite(&sql, &e) {
                        Some(s) if tries < 32 => {
                            sql = std::borrow::Cow::Owned(s);
                            tries += 1;
                        }
                        _ => {
                            self.refine_pk(&mut e);
                            return Err(e);
                        }
                    }
                }
            }
        }
    }

    /// A UNIQUE failure on exactly the table's PRIMARY KEY columns (or
    /// rowid alias) is SQLITE_CONSTRAINT_PRIMARYKEY (rescode.html), which
    /// graphitesql's message doesn't tell apart.
    fn refine_pk(&self, e: &mut SqlError) {
        if e.ext_code != 2067 {
            return;
        }
        let Some(list) = e.msg.strip_prefix("UNIQUE constraint failed: ") else {
            return;
        };
        let mut table: Option<&str> = None;
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
        let Ok(r) = self.conn.query(&q) else { return };
        let mut pk: Vec<(i64, String)> = r
            .rows
            .iter()
            .filter_map(|row| {
                let n = match row.get(5)? {
                    Value::Integer(n) if *n > 0 => *n,
                    _ => return None,
                };
                Some((n, text(row.get(1)?)?.to_ascii_lowercase()))
            })
            .collect();
        pk.sort();
        let pk: Vec<String> = pk.into_iter().map(|(_, n)| n).collect();
        if !pk.is_empty() && pk == cols {
            e.ext_code = 1555;
        }
    }

    /// `sqlite3_exec()` without a callback: runs each statement of a
    /// script in turn, stopping at the first error.
    pub fn exec(&mut self, sql: &str) -> Result<(), SqlError> {
        let mut rest = sql;
        loop {
            let (st, tail) = self.prepare(rest)?;
            if let Some(st) = st {
                self.run(&st, &[])?;
            }
            if tail >= rest.len() {
                return Ok(());
            }
            rest = &rest[tail..];
        }
    }

    pub fn changes(&self) -> i64 {
        self.conn.changes()
    }

    pub fn total_changes(&self) -> i64 {
        self.conn.total_changes()
    }

    pub fn last_insert_rowid(&self) -> i64 {
        self.conn.last_insert_rowid()
    }

    /// `sqlite3_get_autocommit()`.
    pub fn autocommit(&self) -> bool {
        self.conn.is_autocommit()
    }

    /// `sqlite3_busy_timeout()`.
    pub fn busy_timeout(&mut self, ms: i64) {
        let _ = self
            .conn
            .execute(&format!("PRAGMA busy_timeout = {}", ms.max(0)));
    }

    /// A user scalar function.
    pub fn create_function(
        &mut self,
        name: &str,
        f: impl Fn(&[Value]) -> graphitesql::Result<Value> + 'static,
    ) {
        // graphitesql calls each user function once with NULL arguments
        // before running a statement, to check that the name resolves; the
        // calls the statement's text accounts for are answered here
        // without running the user's function (see `dry_budget`).
        let key = name.to_ascii_lowercase();
        UDF_NAMES.with(|u| u.borrow_mut().insert(key.clone()));
        self.conn.register_function(name, move |args: &[Value]| {
            if args.iter().all(|a| matches!(a, Value::Null)) {
                let dry = DRY_BUDGET.with(|b| {
                    let mut b = b.borrow_mut();
                    match b.get_mut(&key) {
                        Some(n) if *n > 0 => {
                            *n -= 1;
                            true
                        }
                        _ => false,
                    }
                });
                if dry {
                    return Ok(Value::Null);
                }
            }
            f(args)
        });
    }

    /// Sets the dry-resolve allowance of each user function for one
    /// statement: the number of times its name is called in the text.
    fn set_dry_budget(sql: &str) {
        let names = UDF_NAMES.with(|u| u.borrow().clone());
        let mut budget = HashMap::new();
        if !names.is_empty() {
            for name in scan::called_names(sql) {
                if names.contains(&name) {
                    *budget.entry(name).or_insert(0usize) += 1;
                }
            }
        }
        DRY_BUDGET.with(|b| *b.borrow_mut() = budget);
    }

    /// A user aggregate.
    pub fn create_aggregate(
        &mut self,
        name: &str,
        factory: impl Fn() -> Box<dyn graphitesql::AggregateFunction> + 'static,
    ) {
        self.conn.register_aggregate_function(name, factory);
    }

    /// A user collation. graphitesql's collations are process-wide; the
    /// function is kept per thread under its (case-insensitive) name.
    #[allow(clippy::type_complexity)]
    pub fn create_collation(
        &mut self,
        name: &str,
        f: Rc<dyn Fn(&str, &str) -> std::cmp::Ordering>,
    ) {
        let key = name.to_ascii_lowercase();
        COLLATIONS.with(|c| c.borrow_mut().insert(key.clone(), f));
        self.conn.register_collation(name, move |a, b| {
            let f = COLLATIONS.with(|c| c.borrow().get(&key).cloned());
            match f {
                Some(f) => f(a, b),
                None => a.cmp(b),
            }
        });
    }

    /// `sqlite3_set_authorizer()`; `None` removes it.
    #[allow(clippy::type_complexity, clippy::redundant_closure)]
    pub fn set_authorizer(
        &self,
        cb: Option<
            Box<dyn FnMut(i32, Option<&str>, Option<&str>, Option<&str>, Option<&str>) -> i32>,
        >,
    ) {
        match cb {
            Some(mut cb) => self
                .conn
                .set_authorizer(move |a, b, c, d, e| cb(a, b, c, d, e)),
            None => self.conn.clear_authorizer(),
        }
    }

    /// The database image (`sqlite3_serialize`), for backups.
    pub fn serialize(&self) -> Result<Vec<u8>, SqlError> {
        self.conn.serialize().map_err(|e| SqlError::from_engine(&e))
    }

    /// Replaces this database's content (the target side of a backup).
    pub fn restore(&mut self, image: &[u8]) -> Result<(), SqlError> {
        self.conn
            .restore_from(image)
            .map_err(|e| SqlError::from_engine(&e))
    }

    /// The declared type and origin table of each result column of a
    /// SELECT (`sqlite3_column_decltype`, `sqlite3_column_table_name`):
    /// known for columns that name a table column directly.
    pub fn column_origins(&self, st: &Stmt, ncols: usize) -> Vec<(Option<String>, Option<String>)> {
        let mut out = vec![(None, None); ncols];
        let Ok(Statement::Select(sel)) = graphitesql::sql::parse_one(&st.sql) else {
            return out;
        };
        if !sel.compound.is_empty() {
            return out;
        }
        let Some(from) = &sel.from else { return out };
        // (alias or name, table name, columns (name, type)).
        #[allow(clippy::type_complexity)]
        let mut tables: Vec<(String, String, Vec<(String, String)>)> = Vec::new();
        for t in std::iter::once(&from.first).chain(from.joins.iter().map(|j| &j.table)) {
            if t.subquery.is_some() || t.tvf_args.is_some() {
                tables.push((String::new(), String::new(), Vec::new()));
                continue;
            }
            let cols = self.table_columns(t.schema.as_deref(), &t.name);
            let alias = t.alias.clone().unwrap_or_else(|| t.name.clone());
            tables.push((alias, t.name.clone(), cols));
        }
        let mut idx = 0;
        for rc in &sel.columns {
            match rc {
                ResultColumn::Wildcard => {
                    for (_, tn, cols) in &tables {
                        for (_, ty) in cols {
                            if idx < ncols {
                                out[idx] = (decl(ty), Some(tn.clone()));
                            }
                            idx += 1;
                        }
                    }
                }
                ResultColumn::TableWildcard(a) => {
                    if let Some((_, tn, cols)) =
                        tables.iter().find(|(al, _, _)| al.eq_ignore_ascii_case(a))
                    {
                        for (_, ty) in cols {
                            if idx < ncols {
                                out[idx] = (decl(ty), Some(tn.clone()));
                            }
                            idx += 1;
                        }
                    }
                }
                ResultColumn::Expr { expr, .. } => {
                    if let graphitesql::sql::ast::Expr::Column { table, column, .. } = expr {
                        let found = tables.iter().find_map(|(al, tn, cols)| {
                            if let Some(q) = table
                                && !q.eq_ignore_ascii_case(al)
                            {
                                return None;
                            }
                            cols.iter()
                                .find(|(c, _)| c.eq_ignore_ascii_case(column))
                                .map(|(_, ty)| (decl(ty), Some(tn.clone())))
                                .or_else(|| {
                                    let rowid = ["rowid", "oid", "_rowid_"]
                                        .iter()
                                        .any(|r| r.eq_ignore_ascii_case(column));
                                    (rowid && !cols.is_empty())
                                        .then(|| (Some("INTEGER".to_string()), Some(tn.clone())))
                                })
                        });
                        if let Some(f) = found
                            && idx < ncols
                        {
                            out[idx] = f;
                        }
                    }
                    idx += 1;
                }
            }
        }
        out
    }

    /// The columns of a table: (name, declared type).
    pub fn table_columns(&self, schema: Option<&str>, table: &str) -> Vec<(String, String)> {
        let q = match schema {
            Some(s) => format!(
                "PRAGMA {}.table_info({})",
                scan::quote_ident(s),
                scan::quote_ident(table)
            ),
            None => format!("PRAGMA table_info({})", scan::quote_ident(table)),
        };
        let Ok(r) = self.conn.query(&q) else {
            return Vec::new();
        };
        r.rows
            .iter()
            .filter_map(|row| {
                let name = text(row.get(1)?)?;
                let ty = row.get(2).and_then(text).unwrap_or_default();
                Some((name, ty))
            })
            .collect()
    }
}

/// SQLite's double-quoted string literal fallback (quirks.html, "Double-
/// quoted String Literals Are Accepted"): a `"name"` that names no column
/// is a string. graphitesql reports such names as an error; the query is
/// rewritten with that name as a `'…'` literal and tried again.
fn dqs_rewrite(sql: &str, e: &SqlError) -> Option<String> {
    let rest = e.msg.strip_prefix("no such column: \"")?;
    let name = rest.strip_suffix("\" - should this be a string literal in single-quotes?")?;
    let quoted_ident = format!("\"{}\"", name.replace('"', "\"\""));
    let literal = format!("'{}'", name.replace('\'', "''"));
    let b = sql.as_bytes();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    let mut changed = false;
    while i < b.len() {
        match b[i] {
            b'\'' | b'`' | b'[' => {
                let close = if b[i] == b'[' { b']' } else { b[i] };
                let mut j = i + 1;
                while j < b.len() {
                    if b[j] == close {
                        if close != b']' && j + 1 < b.len() && b[j + 1] == close {
                            j += 2;
                            continue;
                        }
                        break;
                    }
                    j += 1;
                }
                let end = (j + 1).min(b.len());
                out.push_str(&sql[i..end]);
                i = end;
            }
            b'"' => {
                let mut j = i + 1;
                while j < b.len() {
                    if b[j] == b'"' {
                        if j + 1 < b.len() && b[j + 1] == b'"' {
                            j += 2;
                            continue;
                        }
                        break;
                    }
                    j += 1;
                }
                let end = (j + 1).min(b.len());
                let tok = &sql[i..end];
                // Not a qualified name (`t."c"`).
                let qualified = out.ends_with('.');
                if tok == quoted_ident && !qualified {
                    out.push_str(&literal);
                    changed = true;
                } else {
                    out.push_str(tok);
                }
                i = end;
            }
            _ => {
                let ch = sql[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    changed.then_some(out)
}

fn decl(ty: &str) -> Option<String> {
    (!ty.is_empty()).then(|| ty.to_string())
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::Text(t) => Some(t.to_str_lossy().into_owned()),
        _ => None,
    }
}

/// How a parsed statement runs, whether it is read-only, and its EXPLAIN
/// mode (c3ref/stmt_readonly.html: transaction control statements and
/// reads are read-only).
fn classify(ast: &Statement, text: &str) -> (Kind, bool, u8) {
    match ast {
        Statement::Select(_) => (Kind::Query, true, 0),
        Statement::Explain { query_plan, .. } => (Kind::Query, true, 1 + *query_plan as u8),
        Statement::Pragma(p) => {
            if p.value.is_some() && !is_info_pragma(&p.name) {
                (Kind::Exec, false, 0)
            } else {
                (Kind::Query, true, 0)
            }
        }
        Statement::Insert(_) | Statement::Update(_) | Statement::Delete(_) => {
            let k = if scan::has_keyword(text, "returning") {
                Kind::Returning
            } else {
                Kind::Exec
            };
            (k, false, 0)
        }
        Statement::Begin
        | Statement::Commit
        | Statement::Rollback
        | Statement::Savepoint(_)
        | Statement::Release(_)
        | Statement::RollbackTo(_) => (Kind::Exec, true, 0),
        _ => {
            let kw = scan::first_keyword(text);
            if kw == "VALUES" || kw == "WITH" {
                (Kind::Query, true, 0)
            } else {
                (Kind::Exec, false, 0)
            }
        }
    }
}

/// Pragmas whose argument selects what to report (pragma.html).
fn is_info_pragma(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "table_info"
            | "table_xinfo"
            | "index_info"
            | "index_xinfo"
            | "index_list"
            | "foreign_key_list"
            | "foreign_key_check"
            | "integrity_check"
            | "quick_check"
            | "table_list"
            | "function_list"
            | "pragma_list"
            | "collation_list"
            | "database_list"
            | "module_list"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let mut db = Db::open(":memory:", OPEN_READWRITE | OPEN_CREATE).unwrap();
        db.exec("CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t(v) VALUES('a')")
            .unwrap();
        let (st, _) = db.prepare("SELECT v, :x, ? FROM t").unwrap();
        let st = st.unwrap();
        assert_eq!(st.params.len(), 2);
        let r = db
            .run(&st, &[Value::Integer(5), Value::Integer(6)])
            .unwrap();
        assert_eq!(r.rows[0][1], Value::Integer(5));
        assert_eq!(r.rows[0][2], Value::Integer(6));
        assert_eq!(
            db.column_origins(&st, 3)[0],
            (Some("TEXT".into()), Some("t".into()))
        );
        let e = db.prepare("SELECT * FROM nope").unwrap_err();
        assert_eq!(e.msg, "no such table: nope");
        let e = db.prepare("INSERT INTO nope VALUES(1)").unwrap_err();
        assert_eq!(e.msg, "no such table: nope");
        let e = db.prepare("SELEC 1").unwrap_err();
        assert_eq!(e.msg, "near \"SELEC\": syntax error");
        let e = db.exec("INSERT INTO t VALUES(1, 'b')").unwrap_err();
        assert_eq!(e.code, 19);
    }
}
