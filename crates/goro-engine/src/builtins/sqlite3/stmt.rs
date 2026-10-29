//! `SQLite3Stmt` and `SQLite3Result` (PHP manual: SQLite3Stmt,
//! SQLite3Result).
//!
//! A statement and the results it produced share one cursor, as an
//! `sqlite3_stmt` does: `SQLite3Stmt::reset()` rewinds the result too
//! (tests sqlite3_26_reset_prepared_stmt.phpt, bug79294.phpt).
//! graphitesql runs statements to completion, so the cursor walks rows
//! kept from the last run; a run whose effects could be observed again
//! (writes, callbacks, a database changed since) really runs again.

use std::cell::RefCell;
use std::rc::Rc;

use goro_ext_sqlite::graphitesql::Value as SqlValue;
use goro_ext_sqlite::{Kind, Stmt};
use goro_runtime::{Key, ObjRef, PhpArray, PhpRef, Value};

use super::{
    ASSOC, BOTH, Conn, NUM, T_BLOB, T_FLOAT, T_INTEGER, T_NULL, T_TEXT, common, fail, is_auth,
    run, with_native,
};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::vm::{R, Vm};

/// A bound parameter: a value (bindValue) or a variable (bindParam),
/// with the type asked for (`None`: from the value).
#[derive(Clone)]
pub(super) enum Bind {
    Value(Value, Option<i64>),
    Param(PhpRef, Option<i64>),
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Phase {
    /// Not stepped since it was prepared, executed or reset.
    #[default]
    Ready,
    /// Rows are being read.
    Running,
    /// The last row was read.
    Done,
}

#[derive(Default)]
pub(super) struct Cursor {
    pub phase: Phase,
    /// `columns`/`rows` hold the result of a run.
    pub ran: bool,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<SqlValue>>,
    pub pos: usize,
    pub current: Option<usize>,
    /// The connection's generation when the rows were read.
    pub generation: u64,
    /// The values bound by the last execute() (SQLite keeps bindings
    /// across resets).
    pub values: Vec<SqlValue>,
}

pub(super) struct StmtState {
    pub conn: Rc<Conn>,
    /// `None` once closed.
    pub st: Option<Stmt>,
    /// The SQL as `sqlite3_sql()` reports it.
    pub text: String,
    pub binds: Vec<Option<Bind>>,
    /// `sqlite3_stmt_explain()` mode.
    pub explain: u8,
    pub cur: Cursor,
}

impl StmtState {
    pub fn new(conn: Rc<Conn>, st: Stmt, text: String) -> StmtState {
        let n = st.params.len();
        let explain = st.explain;
        StmtState {
            conn,
            st: Some(st),
            text,
            binds: vec![None; n],
            explain,
            cur: Cursor::default(),
        }
    }

    /// The statement as it runs in the current explain mode.
    fn runnable(&self) -> Option<Stmt> {
        let st = self.st.as_ref()?;
        if self.explain == st.explain {
            return Some(st.clone());
        }
        let mut s = st.clone();
        let body = strip_explain(&st.sql);
        s.sql = match self.explain {
            0 => body.to_string(),
            1 => format!("EXPLAIN {body}"),
            _ => format!("EXPLAIN QUERY PLAN {body}"),
        };
        s.explain = self.explain;
        if self.explain != 0 {
            s.kind = Kind::Query;
            s.readonly = true;
        }
        Some(s)
    }
}

/// The statement without a leading `EXPLAIN [QUERY PLAN]`.
fn strip_explain(sql: &str) -> &str {
    let t = sql.trim_start();
    let word = |s: &str, w: &str| -> Option<usize> {
        let b = s.as_bytes();
        (b.len() >= w.len()
            && b[..w.len()].eq_ignore_ascii_case(w.as_bytes())
            && b.get(w.len()).is_none_or(|c| c.is_ascii_whitespace()))
        .then_some(w.len())
    };
    let Some(n) = word(t, "explain") else {
        return sql;
    };
    let rest = t[n..].trim_start();
    if let Some(n) = word(rest, "query") {
        let r2 = rest[n..].trim_start();
        if let Some(n) = word(r2, "plan") {
            return r2[n..].trim_start();
        }
    }
    rest
}

pub(super) type State = Rc<RefCell<StmtState>>;

struct StmtObj {
    state: Option<State>,
    /// The SQLite3 object (kept alive by its statements).
    db: Option<ObjRef>,
}

impl goro_runtime::NativeData for StmtObj {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        if let Some(d) = &self.db {
            visit(&Value::Object(d.clone()));
        }
        if let Some(s) = &self.state
            && let Ok(s) = s.try_borrow()
        {
            for b in s.binds.iter().flatten() {
                match b {
                    Bind::Value(v, _) => visit(v),
                    Bind::Param(r, _) => visit(&Value::Ref(r.clone())),
                }
            }
        }
    }
    fn clear(&mut self) {
        self.db = None;
        if let Some(s) = &self.state
            && let Ok(mut s) = s.try_borrow_mut()
        {
            s.binds.iter_mut().for_each(|b| *b = None);
        }
    }
}

struct ResultObj {
    state: Option<State>,
    /// The statement object it came from (none for query()).
    stmt: Option<ObjRef>,
    /// Made by query(): finalizing it closes the statement.
    owned: bool,
}

impl goro_runtime::NativeData for ResultObj {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        if let Some(s) = &self.stmt {
            visit(&Value::Object(s.clone()));
        }
    }
    fn clear(&mut self) {
        self.stmt = None;
    }
}

pub(super) fn stmt_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_vm, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(StmtObj {
                state: None,
                db: None,
            }));
        }),
        ..Default::default()
    }
}

pub(super) fn result_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_vm, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(ResultObj {
                state: None,
                stmt: None,
                owned: false,
            }));
        }),
        ..Default::default()
    }
}

pub(super) fn new_stmt(
    vm: &mut Vm,
    conn: Rc<Conn>,
    db: ObjRef,
    st: Stmt,
    text: String,
) -> R<ObjRef> {
    let cls = vm
        .find_class(b"SQLite3Stmt", false)?
        .expect("SQLite3Stmt class");
    let o = vm.instantiate_raw(&cls)?;
    let state = Rc::new(RefCell::new(StmtState::new(conn, st, text)));
    with_native(&o, |s: &mut StmtObj| {
        s.state = Some(state);
        s.db = Some(db);
    });
    Ok(o)
}

pub(super) fn new_result(
    vm: &mut Vm,
    state: State,
    stmt: Option<ObjRef>,
    owned: bool,
) -> R<ObjRef> {
    let cls = vm
        .find_class(b"SQLite3Result", false)?
        .expect("SQLite3Result class");
    let o = vm.instantiate_raw(&cls)?;
    with_native(&o, |r: &mut ResultObj| {
        r.state = Some(state);
        r.stmt = stmt;
        r.owned = owned;
    });
    Ok(o)
}

const DB_NOT_INIT: &str = "The SQLite3 object has not been correctly initialised or is already closed";
const STMT_NOT_INIT: &str =
    "The SQLite3Stmt object has not been correctly initialised or is already closed";
const RES_NOT_INIT: &str =
    "The SQLite3Result object has not been correctly initialised or is already closed";

/// The state of `$this` statement, checked open (test bug66550.phpt:
/// statements of a closed database report the database).
fn this_stmt(vm: &mut Vm, a: &Args) -> R<State> {
    let s = with_native(a.this_obj(), |s: &mut StmtObj| s.state.clone()).flatten();
    let Some(s) = s else {
        return vm.throw("Error", STMT_NOT_INIT);
    };
    let (db_open, open) = {
        let b = s.borrow();
        (b.conn.is_open(), b.st.is_some())
    };
    if !db_open {
        return vm.throw("Error", DB_NOT_INIT);
    }
    if !open {
        return vm.throw("Error", STMT_NOT_INIT);
    }
    Ok(s)
}

fn this_result(vm: &mut Vm, a: &Args) -> R<State> {
    let s = with_native(a.this_obj(), |r: &mut ResultObj| r.state.clone()).flatten();
    match s {
        Some(s) if s.borrow().conn.is_open() && s.borrow().st.is_some() => Ok(s),
        _ => vm.throw("Error", RES_NOT_INIT),
    }
}

// ------------------------------------------------------------------ values

/// The SQL value of a bound PHP value (PHP manual: SQLite3Stmt::bindValue;
/// without a type, it follows the value: bool and int are INTEGER, float
/// FLOAT, null NULL, a stream BLOB, anything else TEXT; test
/// sqlite3_bind_bug68849.phpt). NULL binds as NULL whatever the type.
fn sql_value(vm: &mut Vm, v: &Value, ty: Option<i64>) -> R<SqlValue> {
    let v = v.deref();
    if matches!(v, Value::Null) {
        return Ok(SqlValue::Null);
    }
    let ty = ty.unwrap_or(match &v {
        Value::Bool(_) | Value::Int(_) => T_INTEGER,
        Value::Float(_) => T_FLOAT,
        Value::Resource(_) => T_BLOB,
        _ => T_TEXT,
    });
    Ok(match ty {
        T_INTEGER => SqlValue::Integer(vm.to_int(&v)?),
        T_FLOAT => SqlValue::Real(vm.to_float(&v)?),
        T_BLOB => match &v {
            Value::Resource(r) => SqlValue::Blob(read_stream(vm, r)),
            _ => SqlValue::Blob(vm.to_php_str(&v)?.as_bytes().to_vec()),
        },
        T_NULL => SqlValue::Null,
        _ => common::text(vm.to_php_str(&v)?.as_bytes()),
    })
}

/// The rest of a stream bound as a BLOB (test sqlite3_09_blob_bound_param.phpt).
fn read_stream(vm: &mut Vm, r: &goro_runtime::ResRef) -> Vec<u8> {
    use crate::builtins::file::{Stream, read_rest};
    let data = r.res().data.borrow();
    let Some(cell) = data
        .as_ref()
        .and_then(|d| d.downcast_ref::<RefCell<Stream>>())
    else {
        return Vec::new();
    };
    let cell: *const RefCell<Stream> = cell;
    drop(data);
    // SAFETY: the resource (held by the caller's value) keeps the stream
    // alive for the call.
    let cell = unsafe { &*cell };
    let Ok(mut s) = cell.try_borrow_mut() else {
        return Vec::new();
    };
    read_rest(vm, &mut s)
}

/// The values to run with, by parameter number.
fn bound_values(vm: &mut Vm, state: &State) -> R<Vec<SqlValue>> {
    let binds = state.borrow().binds.clone();
    let mut out = Vec::with_capacity(binds.len());
    for b in &binds {
        out.push(match b {
            None => SqlValue::Null,
            Some(Bind::Value(v, t)) => sql_value(vm, v, *t)?,
            Some(Bind::Param(r, t)) => {
                let v = r.get();
                sql_value(vm, &v, *t)?
            }
        });
    }
    Ok(out)
}

/// Runs the statement with the values bound now; the cursor is left
/// ready (SQLite3Stmt::execute steps once and resets).
pub(super) fn execute_state(
    vm: &mut Vm,
    state: &State,
) -> R<Result<(), goro_ext_sqlite::SqlError>> {
    let values = bound_values(vm, state)?;
    let (conn, st) = {
        let s = state.borrow();
        (s.conn.clone(), s.runnable())
    };
    let Some(st) = st else {
        return Ok(Ok(()));
    };
    let r = run(vm, &conn, &st, &values);
    let mut s = state.borrow_mut();
    s.cur.values = values;
    s.cur.phase = Phase::Ready;
    s.cur.current = None;
    s.cur.pos = 0;
    match r {
        Ok(rows) => {
            s.cur.columns = rows.columns;
            s.cur.rows = rows.rows;
            s.cur.ran = true;
            s.cur.generation = conn.generation.get();
            Ok(Ok(()))
        }
        Err(e) => {
            s.cur.ran = false;
            Ok(Err(e))
        }
    }
}

/// Steps the cursor: the next row, `None` at the end. A step from the
/// start runs the statement again unless the rows of the last run are
/// still what a new run would give.
fn step(vm: &mut Vm, a: &Args, state: &State) -> R<Option<usize>> {
    let phase = state.borrow().cur.phase;
    match phase {
        Phase::Done => return Ok(None),
        Phase::Running => {}
        Phase::Ready => {
            let (fresh, conn, st, values) = {
                let s = state.borrow();
                let st = s.runnable();
                let fresh = s.cur.ran
                    && st.as_ref().is_some_and(|st| st.kind == Kind::Query)
                    && !s.conn.callbacks.get()
                    && s.cur.generation == s.conn.generation.get();
                (fresh, s.conn.clone(), st, s.cur.values.clone())
            };
            if !fresh {
                let Some(st) = st else { return Ok(None) };
                match run(vm, &conn, &st, &values) {
                    Ok(rows) => {
                        let mut s = state.borrow_mut();
                        s.cur.columns = rows.columns;
                        s.cur.rows = rows.rows;
                        s.cur.ran = true;
                        s.cur.generation = conn.generation.get();
                    }
                    Err(e) => {
                        {
                            let mut s = state.borrow_mut();
                            s.cur.ran = false;
                            s.cur.phase = Phase::Done;
                            s.cur.current = None;
                        }
                        let pre = if is_auth(&e) {
                            "Unable to prepare statement"
                        } else {
                            "Unable to execute statement"
                        };
                        let msg = format!("{pre}: {}", e.msg);
                        conn.set_err(e);
                        fail(vm, a, &conn, &msg)?;
                        return Ok(None);
                    }
                }
            }
            let mut s = state.borrow_mut();
            s.cur.phase = Phase::Running;
            s.cur.pos = 0;
        }
    }
    let mut s = state.borrow_mut();
    if s.cur.pos < s.cur.rows.len() {
        let i = s.cur.pos;
        s.cur.pos += 1;
        s.cur.current = Some(i);
        Ok(Some(i))
    } else {
        s.cur.phase = Phase::Done;
        s.cur.current = None;
        Ok(None)
    }
}

/// A row as fetchArray() gives it. A repeated column name keeps its first
/// place and the last value (test gh11451.phpt).
pub(super) fn row_array(columns: &[String], row: &[SqlValue], mode: i64) -> PhpArray {
    let mut arr = PhpArray::new();
    for (i, v) in row.iter().enumerate() {
        let pv = common::to_php(v);
        if mode & NUM != 0 {
            arr.insert(Key::Int(i as i64), pv.clone());
        }
        if mode & ASSOC != 0 {
            let name = columns.get(i).map(String::as_str).unwrap_or("");
            arr.insert(Key::from_bytes(name.as_bytes()), pv);
        }
    }
    arr
}

// ------------------------------------------------------------------ SQLite3Stmt

/// The parameter number for a name or number; names may leave out the
/// `:` (test sqlite3_10_bound_value_name.phpt). 0 when there is none.
fn param_number(state: &StmtState, p: &Value) -> usize {
    let Some(st) = &state.st else { return 0 };
    match p.deref() {
        Value::Int(n) => {
            if n >= 1 && (n as usize) <= st.params.len() {
                n as usize
            } else {
                0
            }
        }
        other => {
            let name = match &other {
                Value::Str(s) => s.to_string_lossy(),
                _ => return 0,
            };
            if name.starts_with([':', '@', '$', '?']) {
                st.param_index(&name)
            } else {
                st.param_index(&format!(":{name}"))
            }
        }
    }
}

fn bind(vm: &mut Vm, a: &mut Args, by_ref: bool) -> R<Value> {
    let state = this_stmt(vm, a)?;
    let ty = if a.len() > 2 { Some(a.int(2)) } else { None };
    let b = if by_ref {
        match a.ref_at(1) {
            Some(r) => Bind::Param(r, ty),
            None => Bind::Value(a.val(1), ty),
        }
    } else {
        Bind::Value(a.val(1).deref(), ty)
    };
    let p = a.val(0);
    let mut s = state.borrow_mut();
    let n = param_number(&s, &p);
    if n == 0 {
        return Ok(Value::Bool(false));
    }
    s.binds[n - 1] = Some(b);
    Ok(Value::Bool(true))
}

pub(super) fn m_bind_param(vm: &mut Vm, a: &mut Args) -> R<Value> {
    bind(vm, a, true)
}

pub(super) fn m_bind_value(vm: &mut Vm, a: &mut Args) -> R<Value> {
    bind(vm, a, false)
}

/// `SQLite3Stmt::clear()`: the bindings go (to NULL), also for a
/// reset statement's next run (test bug70628.phpt).
pub(super) fn m_clear(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_stmt(vm, a)?;
    let mut s = state.borrow_mut();
    s.binds.iter_mut().for_each(|b| *b = None);
    s.cur.values.iter_mut().for_each(|v| *v = SqlValue::Null);
    Ok(Value::Bool(true))
}

pub(super) fn m_close(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_stmt(vm, a)?;
    let mut s = state.borrow_mut();
    s.st = None;
    s.binds.clear();
    s.cur = Cursor::default();
    Ok(Value::Bool(true))
}

pub(super) fn m_execute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_stmt(vm, a)?;
    let conn = state.borrow().conn.clone();
    match execute_state(vm, &state)? {
        Ok(()) => {
            conn.ok();
            let me = a.this_obj().clone();
            new_result(vm, state, Some(me), false).map(Value::Object)
        }
        Err(e) => {
            let pre = if is_auth(&e) {
                "Unable to prepare statement"
            } else {
                "Unable to execute statement"
            };
            let msg = format!("{pre}: {}", e.msg);
            conn.set_err(e);
            fail(vm, a, &conn, &msg)?;
            Ok(Value::Bool(false))
        }
    }
}

/// `SQLite3Stmt::getSQL()`: the statement's text, or with `$expand` the
/// text with each parameter replaced by its bound value as an SQL
/// literal (`sqlite3_expanded_sql`; test sqlite3stmt_getsql_expanded.phpt).
pub(super) fn m_get_sql(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_stmt(vm, a)?;
    let expand = a.bool_or(0, false);
    let text = state.borrow().text.clone();
    if !expand {
        return Ok(Value::from(text.as_str()));
    }
    let values = bound_values(vm, &state)?;
    let s = state.borrow();
    let Some(st) = &s.st else {
        return Ok(Value::Bool(false));
    };
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    // Numbering (lang_expr "Parameters"): `?NNN` is NNN, a bare `?` one
    // more than the largest number so far, a name its own number.
    let mut max = 0usize;
    for (start, end, tok) in param_tokens(&text) {
        out.push_str(&text[last..start]);
        let n = match tok.strip_prefix('?') {
            Some("") => max + 1,
            Some(num) => num.parse().unwrap_or(0),
            None => st.param_index(tok),
        };
        max = max.max(n);
        out.push_str(&literal(n.checked_sub(1).and_then(|i| values.get(i))));
        last = end;
    }
    out.push_str(&text[last..]);
    Ok(Value::from(out.as_str()))
}

/// An SQL literal for a value (sqlite3_expanded_sql).
fn literal(v: Option<&SqlValue>) -> String {
    match v {
        None | Some(SqlValue::Null) => "NULL".into(),
        Some(SqlValue::Integer(i)) => i.to_string(),
        Some(SqlValue::Real(f)) => format!("{f:?}"),
        Some(SqlValue::Text(t)) => format!("'{}'", t.to_str_lossy().replace('\'', "''")),
        Some(SqlValue::Blob(b)) => {
            let mut s = String::from("x'");
            for c in b {
                s.push_str(&format!("{c:02x}"));
            }
            s.push('\'');
            s
        }
    }
}

/// The parameter tokens of an SQL text: (start, end, token), outside
/// strings, quoted identifiers and comments (sqlite.org lang_expr
/// "Parameters").
fn param_tokens(sql: &str) -> Vec<(usize, usize, &str)> {
    let s = sql.as_bytes();
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80;
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        match b {
            b'\'' | b'"' | b'`' | b'[' => {
                let close = if b == b'[' { b']' } else { b };
                let mut j = i + 1;
                while j < s.len() {
                    if s[j] == close {
                        if close != b']' && j + 1 < s.len() && s[j + 1] == close {
                            j += 2;
                            continue;
                        }
                        break;
                    }
                    j += 1;
                }
                i = j + 1;
            }
            b'-' if s.get(i + 1) == Some(&b'-') => {
                while i < s.len() && s[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if s.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < s.len() && !(s[i] == b'*' && s[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b'?' => {
                let mut j = i + 1;
                while j < s.len() && s[j].is_ascii_digit() {
                    j += 1;
                }
                out.push((i, j, &sql[i..j]));
                i = j;
            }
            b':' | b'@' | b'$' => {
                let mut j = i + 1;
                while j < s.len() && ident(s[j]) {
                    j += 1;
                }
                if j > i + 1 {
                    out.push((i, j, &sql[i..j]));
                }
                i = j.max(i + 1);
            }
            _ if ident(b) => {
                while i < s.len() && (ident(s[i]) || s[i] == b'$') {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out
}

pub(super) fn m_param_count(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_stmt(vm, a)?;
    let n = state.borrow().st.as_ref().map_or(0, |s| s.params.len());
    Ok(Value::Int(n as i64))
}

pub(super) fn m_read_only(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_stmt(vm, a)?;
    let r = state.borrow().st.as_ref().is_some_and(|s| s.readonly);
    Ok(Value::Bool(r))
}

pub(super) fn m_reset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_stmt(vm, a)?;
    rewind(&state);
    Ok(Value::Bool(true))
}

fn rewind(state: &State) {
    let mut s = state.borrow_mut();
    s.cur.phase = Phase::Ready;
    s.cur.pos = 0;
    s.cur.current = None;
}

/// `sqlite3_stmt_busy()`: stepped and not yet at its end or reset.
pub(super) fn m_busy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_stmt(vm, a)?;
    let r = state.borrow().cur.phase == Phase::Running;
    Ok(Value::Bool(r))
}

pub(super) fn m_explain(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_stmt(vm, a)?;
    let r = state.borrow().explain;
    Ok(Value::Int(r as i64))
}

/// `SQLite3Stmt::setExplain()` (PHP 8.4; test sqlite3_explain.phpt).
pub(super) fn m_set_explain(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_stmt(vm, a)?;
    let mode = a.int(0);
    if !(0..=2).contains(&mode) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($mode) must be one of the SQLite3Stmt::EXPLAIN_MODE_* constants",
        );
    }
    let mut s = state.borrow_mut();
    s.explain = mode as u8;
    s.cur = Cursor {
        values: std::mem::take(&mut s.cur.values),
        ..Cursor::default()
    };
    Ok(Value::Bool(true))
}

// ------------------------------------------------------------------ SQLite3Result

pub(super) fn r_num_columns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_result(vm, a)?;
    let n = state.borrow().cur.columns.len();
    Ok(Value::Int(n as i64))
}

pub(super) fn r_column_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_result(vm, a)?;
    let i = a.int(0);
    let s = state.borrow();
    match usize::try_from(i).ok().and_then(|i| s.cur.columns.get(i)) {
        Some(n) => Ok(Value::from(n.as_str())),
        None => Ok(Value::Bool(false)),
    }
}

/// The type of a column of the current row; false without one (test
/// bug79294.phpt).
pub(super) fn r_column_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_result(vm, a)?;
    let i = a.int(0);
    let s = state.borrow();
    let Some(row) = s.cur.current.and_then(|r| s.cur.rows.get(r)) else {
        return Ok(Value::Bool(false));
    };
    let t = match usize::try_from(i).ok().and_then(|i| row.get(i)) {
        Some(SqlValue::Integer(_)) => T_INTEGER,
        Some(SqlValue::Real(_)) => T_FLOAT,
        Some(SqlValue::Text(_)) => T_TEXT,
        Some(SqlValue::Blob(_)) => T_BLOB,
        Some(SqlValue::Null) => T_NULL,
        None => return Ok(Value::Bool(false)),
    };
    Ok(Value::Int(t))
}

fn fetch_mode(vm: &mut Vm, a: &Args) -> R<i64> {
    let mode = a.int_or(0, BOTH);
    if !(ASSOC..=BOTH).contains(&mode) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($mode) must be one of SQLITE3_ASSOC, SQLITE3_NUM, or SQLITE3_BOTH",
        );
    }
    Ok(mode)
}

pub(super) fn r_fetch_array(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_result(vm, a)?;
    let mode = fetch_mode(vm, a)?;
    match step(vm, a, &state)? {
        Some(i) => {
            let s = state.borrow();
            Ok(Value::Array(row_array(&s.cur.columns, &s.cur.rows[i], mode)))
        }
        None => Ok(Value::Bool(false)),
    }
}

/// `SQLite3Result::fetchAll()` (PHP 8.5): the remaining rows.
pub(super) fn r_fetch_all(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_result(vm, a)?;
    let mode = fetch_mode(vm, a)?;
    let mut out = PhpArray::new();
    while let Some(i) = step(vm, a, &state)? {
        let s = state.borrow();
        let _ = out.push(Value::Array(row_array(&s.cur.columns, &s.cur.rows[i], mode)));
    }
    Ok(Value::Array(out))
}

pub(super) fn r_reset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_result(vm, a)?;
    rewind(&state);
    Ok(Value::Bool(true))
}

pub(super) fn r_finalize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let state = this_result(vm, a)?;
    let owned = with_native(a.this_obj(), |r: &mut ResultObj| {
        r.state = None;
        r.stmt = None;
        r.owned
    })
    .unwrap_or(false);
    if owned {
        let mut s = state.borrow_mut();
        s.st = None;
        s.cur = Cursor::default();
    } else {
        rewind(&state);
    }
    Ok(Value::Bool(true))
}
