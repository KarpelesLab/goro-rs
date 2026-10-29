//! Shared by ext/sqlite3 and pdo_sqlite: SQL value conversions and the
//! bridge that lets SQLite call PHP callbacks (user functions,
//! aggregates, collations, the authorizer) while a statement runs.
//!
//! graphitesql takes `'static` Rust closures for these. They reach the
//! interpreter through the pointer [`enter`] publishes for the duration
//! of a database call (the native function that runs the statement holds
//! `&mut Vm` and does not touch it until the call returns). A PHP
//! exception (or exit) raised in a callback stops the statement with an
//! error and is kept in [`take_pending`] for the caller to rethrow.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;

use goro_ext_sqlite::graphitesql::{self, Value as SqlValue};
use goro_runtime::{PhpStr, Value};

use crate::class::TypeName;

use crate::vm::{R, Unwind, Vm};

thread_local! {
    static VM: Cell<*mut Vm> = const { Cell::new(std::ptr::null_mut()) };
    static PENDING: RefCell<Option<Unwind>> = const { RefCell::new(None) };
}

/// Publishes `vm` to callbacks until the guard drops.
pub struct VmGuard(*mut Vm);

impl Drop for VmGuard {
    fn drop(&mut self) {
        VM.with(|c| c.set(self.0));
    }
}

/// Makes `vm` reachable from SQLite callbacks while a database call runs.
pub fn enter(vm: &mut Vm) -> VmGuard {
    let prev = VM.with(|c| c.replace(vm as *mut Vm));
    VmGuard(prev)
}

/// Runs `f` with the interpreter of the database call in progress.
pub fn with_vm<T>(f: impl FnOnce(&mut Vm) -> T) -> Option<T> {
    let p = VM.with(|c| c.get());
    if p.is_null() {
        return None;
    }
    // SAFETY: `p` was published by `enter` from a `&mut Vm` that its
    // owner doesn't use until the database call (and so this callback)
    // returns; callbacks run on that thread, one at a time.
    let vm = unsafe { &mut *p };
    Some(f(vm))
}

/// Keeps an exception raised in a callback (the first one wins).
pub fn set_pending(u: Unwind) {
    PENDING.with(|p| {
        let mut p = p.borrow_mut();
        if p.is_none() {
            *p = Some(u);
        }
    });
}

/// The exception a callback raised during the last database call.
pub fn take_pending() -> Option<Unwind> {
    PENDING.with(|p| p.borrow_mut().take())
}

/// Whether a callback has raised an exception not yet taken.
pub fn has_pending() -> bool {
    PENDING.with(|p| p.borrow().is_some())
}

/// The error message a statement stops with when a callback threw.
pub const CALLBACK_THREW: &str = "PHP callback raised an exception";

/// SQL value to PHP: INTEGER → int, REAL → float, TEXT and BLOB →
/// string, NULL → null.
pub fn to_php(v: &SqlValue) -> Value {
    match v {
        SqlValue::Null => Value::Null,
        SqlValue::Integer(i) => Value::Int(*i),
        SqlValue::Real(f) => Value::Float(*f),
        SqlValue::Text(t) => Value::Str(PhpStr::from(t.as_bytes())),
        SqlValue::Blob(b) => Value::Str(PhpStr::from(b.as_slice())),
    }
}

/// A PHP string as SQL TEXT.
pub fn text(b: &[u8]) -> SqlValue {
    SqlValue::Text(graphitesql::Text::from_bytes(b.to_vec()))
}

/// The result of a user function: null → NULL, bool and int → INTEGER,
/// float → REAL, anything else as its string conversion (TEXT).
pub fn from_php_result(vm: &mut Vm, v: &Value) -> R<SqlValue> {
    Ok(match v.deref() {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(b as i64),
        Value::Int(i) => SqlValue::Integer(i),
        Value::Float(f) => SqlValue::Real(f),
        Value::Str(s) => text(s.as_bytes()),
        other => {
            let s = vm.to_php_str(&other)?;
            text(s.as_bytes())
        }
    })
}

/// Runs a PHP callback from inside SQLite. An exception becomes a
/// pending unwind and an SQL error; `None` when no interpreter is
/// available.
pub fn call(cb: &Value, args: Vec<Value>) -> Result<Value, graphitesql::Error> {
    if has_pending() {
        return Err(graphitesql::Error::Error(CALLBACK_THREW.into()));
    }
    let r = with_vm(|vm| vm.call_value(cb, args));
    match r {
        Some(Ok(v)) => Ok(v),
        Some(Err(u)) => {
            set_pending(u);
            Err(graphitesql::Error::Error(CALLBACK_THREW.into()))
        }
        None => Err(graphitesql::Error::Error("no interpreter".into())),
    }
}

/// A callable resolved where it is registered, as a Closure: a private
/// method named by `[$this, 'm']` stays callable when SQLite calls it
/// later from elsewhere (test ext/pdo_sqlite/tests/gh20095.phpt).
pub fn bind_callable(vm: &mut Vm, cb: &Value) -> R<Value> {
    if let Value::Object(_) = cb.deref() {
        return Ok(cb.deref());
    }
    let c = vm.resolve_callable(cb, true)?;
    Ok(Value::Object(vm.closure_from_callee(c)?))
}

/// A scalar user function: `cb(...$args)`. `argc` ≥ 0 is checked (the
/// "wrong number of arguments" error of a function registered with a
/// fixed arity).
pub fn scalar(
    name: &str,
    cb: Value,
    argc: i64,
) -> impl Fn(&[SqlValue]) -> graphitesql::Result<SqlValue> + 'static {
    let name = name.to_string();
    move |args: &[SqlValue]| {
        if argc >= 0 && args.len() as i64 != argc {
            return Err(graphitesql::Error::Error(format!(
                "wrong number of arguments to function {name}()"
            )));
        }
        let a = args.iter().map(to_php).collect();
        let v = call(&cb, a)?;
        match with_vm(|vm| from_php_result(vm, &v)) {
            Some(Ok(r)) => Ok(r),
            Some(Err(u)) => {
                set_pending(u);
                Err(graphitesql::Error::Error(CALLBACK_THREW.into()))
            }
            None => Ok(SqlValue::Null),
        }
    }
}

/// A user aggregate: `step($context, $rownumber, ...$values)` returns the
/// new context, `final($context, $rowcount)` the result (PHP manual:
/// SQLite3::createAggregate, Pdo\Sqlite::createAggregate).
struct Aggregate {
    step: Value,
    fin: Value,
    argc: i64,
    name: String,
    context: Value,
    rows: i64,
}

impl graphitesql::AggregateFunction for Aggregate {
    fn step(&mut self, args: &[SqlValue]) -> graphitesql::Result<()> {
        if self.argc >= 0 && args.len() as i64 != self.argc {
            return Err(graphitesql::Error::Error(format!(
                "wrong number of arguments to function {}()",
                self.name
            )));
        }
        self.rows += 1;
        // The context goes by reference, so a step taking `&$context`
        // works without a warning (test pdo_sqlite_createaggregate.phpt).
        let ctx = goro_runtime::PhpRef::new(std::mem::take(&mut self.context));
        let mut a = vec![Value::Ref(ctx), Value::Int(self.rows)];
        a.extend(args.iter().map(to_php));
        self.context = call(&self.step, a)?;
        Ok(())
    }

    fn finalize(&mut self) -> graphitesql::Result<SqlValue> {
        let ctx = goro_runtime::PhpRef::new(std::mem::take(&mut self.context));
        let a = vec![Value::Ref(ctx), Value::Int(self.rows)];
        let v = call(&self.fin, a)?;
        match with_vm(|vm| from_php_result(vm, &v)) {
            Some(Ok(r)) => Ok(r),
            Some(Err(u)) => {
                set_pending(u);
                Err(graphitesql::Error::Error(CALLBACK_THREW.into()))
            }
            None => Ok(SqlValue::Null),
        }
    }
}

/// The factory graphitesql calls for each group of a user aggregate.
pub fn aggregate(
    name: &str,
    step: Value,
    fin: Value,
    argc: i64,
) -> impl Fn() -> Box<dyn graphitesql::AggregateFunction> + 'static {
    let name = name.to_string();
    move || {
        Box::new(Aggregate {
            step: step.clone(),
            fin: fin.clone(),
            argc,
            name: name.clone(),
            context: Value::Null,
            rows: 0,
        })
    }
}

/// A user collation: `cb($a, $b)` returns <0, 0 or >0 (an int: other
/// types are a TypeError, test pdo_sqlite_createcollation_wrong_callback).
/// An exception is kept pending (the comparison reads as equal).
pub fn collation(cb: Value) -> std::rc::Rc<dyn Fn(&str, &str) -> Ordering> {
    std::rc::Rc::new(move |a: &str, b: &str| {
        let args = vec![Value::Str(PhpStr::from(a)), Value::Str(PhpStr::from(b))];
        match call(&cb, args) {
            Ok(Value::Int(n)) => n.cmp(&0),
            Ok(other) => {
                let msg = format!(
                    "{}Return value of the collation callback must be of type int, {} returned",
                    caller_prefix(),
                    other.tname()
                );
                if let Some(Err(u)) = with_vm(|vm| vm.type_error::<()>(&msg)) {
                    set_pending(u);
                }
                Ordering::Equal
            }
            Err(_) => Ordering::Equal,
        }
    })
}

thread_local! {
    /// The PHP function running the current database call ("PDO::query"),
    /// for the messages of errors raised in callbacks.
    static CALLER: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Restores the previous caller name when dropped.
pub struct CallerGuard(String);

impl Drop for CallerGuard {
    fn drop(&mut self) {
        let prev = std::mem::take(&mut self.0);
        CALLER.with(|c| *c.borrow_mut() = prev);
    }
}

/// Names the PHP function running database calls until the guard drops.
pub fn set_caller(name: &str) -> CallerGuard {
    let prev = CALLER.with(|c| std::mem::replace(&mut *c.borrow_mut(), name.to_string()));
    CallerGuard(prev)
}

/// "PDO::query(): " (empty when no caller is set).
pub fn caller_prefix() -> String {
    CALLER.with(|c| {
        let c = c.borrow();
        if c.is_empty() {
            String::new()
        } else {
            format!("{}(): ", c)
        }
    })
}

/// SQLITE_DENY: what an authorizer returns to refuse an action.
pub const AUTH_DENY: i32 = 1;

/// A user authorizer: `cb($action, $arg1, $arg2, $dbname, $trigger)`
/// returns `<class>::OK`, `DENY` or `IGNORE` (PHP manual:
/// SQLite3::setAuthorizer, Pdo\Sqlite::setAuthorizer). Other results
/// are a TypeError/ValueError and deny the action.
#[allow(clippy::type_complexity)]
pub fn authorizer(
    cb: Value,
    class: &'static str,
) -> Box<dyn FnMut(i32, Option<&str>, Option<&str>, Option<&str>, Option<&str>) -> i32> {
    Box::new(move |action, a1, a2, db, trigger| {
        let s = |x: Option<&str>| x.map_or(Value::Null, |v| Value::Str(PhpStr::from(v)));
        let args = vec![Value::Int(action as i64), s(a1), s(a2), s(db), s(trigger)];
        match call(&cb, args) {
            Ok(Value::Int(n)) if (0..=2).contains(&n) => n as i32,
            Ok(Value::Int(_)) => {
                let msg = format!(
                    "{}Return value of the authorizer callback must be one of {class}::OK, {class}::DENY, or {class}::IGNORE",
                    caller_prefix()
                );
                if let Some(Err(u)) = with_vm(|vm| vm.value_error::<()>(&msg)) {
                    set_pending(u);
                }
                AUTH_DENY
            }
            Ok(other) => {
                let msg = format!(
                    "{}Return value of the authorizer callback must be of type int, {} returned",
                    caller_prefix(),
                    other.tname()
                );
                if let Some(Err(u)) = with_vm(|vm| vm.type_error::<()>(&msg)) {
                    set_pending(u);
                }
                AUTH_DENY
            }
            Err(_) => AUTH_DENY,
        }
    })
}
