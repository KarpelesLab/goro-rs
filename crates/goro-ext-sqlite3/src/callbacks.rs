//! The engine callbacks: user functions and aggregates
//! (sqlite3_do_callback()), collations (php_sqlite3_callback_compare())
//! and the authorizer (php_sqlite3_authorizer()).

use std::rc::Rc;

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_sqlite::consts::*;
use goro_sqlite::int::{Context, Mem, Sqlite3, XAuth, XCmp, XFinal, XSFunc};
use goro_sqlite::vdbeapi::*;

use crate::conn::{CollEntry, FuncEntry, Shared, current_vm, with_nested};
use crate::php_sqlite3_error;

/// php_sqlite3_agg_context
#[derive(Default)]
pub struct AggCtx {
    /// None: IS_UNDEF
    pub zval_context: Option<Value>,
    pub row_count: i64,
}

/// zend_call_known_fcc(): None when the call raised an exception (or one
/// was already pending: zend_call_function() does not call then).
fn call(vm: &mut Vm, cb: &Value, args: &[Value]) -> Option<Value> {
    if vm.current_exception.is_some() || vm.exit_requested {
        return None;
    }
    match vm.call_callback(cb, args) {
        Ok(v) => {
            if vm.current_exception.is_some() || vm.exit_requested {
                None
            } else {
                Some(v.deref())
            }
        }
        Err(_) => None,
    }
}

/// The PHP value of an SQL function argument
fn arg_value(v: &mut Mem) -> Value {
    match sqlite3_value_type(v) {
        SQLITE_INTEGER => Value::Long(sqlite3_value_int64(v)),
        SQLITE_FLOAT => Value::Double(sqlite3_value_double(v)),
        SQLITE_NULL => Value::Null,
        _ => {
            let n = sqlite3_value_bytes(v).max(0) as usize;
            let t = sqlite3_value_text(v).unwrap_or_default();
            let t = &t[..n.min(t.len())];
            Value::String(PhpString::from_bytes(t))
        }
    }
}

/// zval_try_get_tmp_string(): None when an exception was thrown.
fn try_string(vm: &mut Vm, v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        Value::Array(_) => {
            vm.emit_warning("Array to string conversion");
            Some(b"Array".to_vec())
        }
        _ => match vm.value_to_string_checked(v) {
            Ok(s) if vm.current_exception.is_none() => Some(s.as_bytes().to_vec()),
            _ => None,
        },
    }
}

/// Which kind of call sqlite3_do_callback() makes.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Scalar,
    Step,
    Final,
}

/// sqlite3_do_callback()
fn do_callback(cb: Option<Value>, argv: &mut [Mem], ctx: &mut Context, kind: Kind) {
    let Some(vm) = current_vm() else {
        sqlite3_result_error(ctx, Some(b"failed to invoke callback"), 0);
        return;
    };
    let is_agg = kind != Kind::Scalar;
    let mut zargs: Vec<Value> = Vec::with_capacity(argv.len() + 2);
    if is_agg {
        // summon the aggregation context
        let (c, n) = match sqlite3_aggregate_context::<AggCtx>(ctx, 1) {
            Some(a) => {
                if a.zval_context.is_none() {
                    a.zval_context = Some(Value::Null);
                }
                (a.zval_context.clone().unwrap_or(Value::Null), a.row_count)
            }
            None => (Value::Null, 0),
        };
        zargs.push(c);
        zargs.push(Value::Long(n));
    }
    for a in argv.iter_mut() {
        zargs.push(arg_value(a));
    }

    let dbp: *mut Sqlite3 = &mut *ctx.db;
    let retval = match &cb {
        Some(cb) => with_nested(dbp, || call(vm, cb, &zargs)),
        None => None,
    };
    drop(zargs);

    if kind != Kind::Step {
        // only set the sqlite return value if we are a scalar function, or
        // if we are finalizing an aggregate
        match &retval {
            Some(v) => match v {
                Value::Long(n) => sqlite3_result_int64(ctx, *n),
                Value::Null => sqlite3_result_null(ctx),
                Value::Double(d) => sqlite3_result_double(ctx, *d),
                other => {
                    if let Some(s) = try_string(vm, other) {
                        sqlite3_result_text(ctx, Some(&s), s.len() as i32);
                    }
                }
            },
            None => sqlite3_result_error(ctx, Some(b"failed to invoke callback"), 0),
        }
        if is_agg {
            if let Some(a) = sqlite3_aggregate_context::<AggCtx>(ctx, 1) {
                a.zval_context = None;
            }
        }
    } else {
        // we're stepping in an aggregate; the return value goes into the
        // context
        if let Some(a) = sqlite3_aggregate_context::<AggCtx>(ctx, 1) {
            a.zval_context = retval;
        }
    }
}

/// The xSFunc of SQLite3::createFunction() (php_sqlite3_callback_func)
pub fn scalar_func(func: Rc<FuncEntry>) -> XSFunc {
    Rc::new(move |ctx: &mut Context, argv: &mut [Mem]| {
        let cb = func.func.borrow().clone();
        do_callback(cb, argv, ctx, Kind::Scalar);
    })
}

/// php_sqlite3_callback_step()
pub fn step_func(func: Rc<FuncEntry>) -> XSFunc {
    Rc::new(move |ctx: &mut Context, argv: &mut [Mem]| {
        if let Some(a) = sqlite3_aggregate_context::<AggCtx>(ctx, 1) {
            a.row_count += 1;
        }
        let cb = func.step.borrow().clone();
        do_callback(cb, argv, ctx, Kind::Step);
    })
}

/// php_sqlite3_callback_final()
pub fn final_func(func: Rc<FuncEntry>) -> XFinal {
    Rc::new(move |ctx: &mut Context| {
        if let Some(a) = sqlite3_aggregate_context::<AggCtx>(ctx, 1) {
            a.row_count = 0;
        }
        let cb = func.fini.borrow().clone();
        do_callback(cb, &mut [], ctx, Kind::Final);
    })
}

/// php_sqlite3_callback_compare()
pub fn compare_func(coll: Rc<CollEntry>) -> XCmp {
    Rc::new(move |a: &[u8], b: &[u8]| -> i32 {
        let Some(vm) = current_vm() else {
            return 0;
        };
        // Exception occurred on previous callback. Don't attempt to call
        // function.
        if vm.current_exception.is_some() {
            return 0;
        }
        let Some(cb) = coll.cmp_func.borrow().clone() else {
            return 0;
        };
        let zargs = [
            Value::String(PhpString::from_bytes(a)),
            Value::String(PhpString::from_bytes(b)),
        ];
        let r = call(vm, &cb, &zargs);
        if vm.current_exception.is_some() {
            return 0;
        }
        match r {
            Some(Value::Long(n)) => n as i32,
            _ => {
                php_sqlite3_error(
                    vm,
                    None,
                    0,
                    "An error occurred while invoking the compare callback (invalid return type).  Collation behaviour is undefined.",
                );
                0
            }
        }
    })
}

fn opt_str(s: Option<&[u8]>) -> Value {
    match s {
        Some(s) => Value::String(PhpString::from_bytes(crate::zpp::cstr(s))),
        None => Value::Null,
    }
}

/// php_sqlite3_authorizer()
pub fn authorizer(shared: Rc<Shared>) -> XAuth {
    Rc::new(
        move |action: i32,
              arg1: Option<&[u8]>,
              arg2: Option<&[u8]>,
              arg3: Option<&[u8]>,
              arg4: Option<&[u8]>|
              -> i32 {
            let Some(vm) = current_vm() else {
                return SQLITE_OK;
            };
            // Check open_basedir restrictions first
            if !goro_core::basedir::open_basedir_value(vm).is_empty() && action == SQLITE_ATTACH {
                let Some(a1) = arg1 else {
                    return SQLITE_DENY;
                };
                let a1 = crate::zpp::cstr(a1);
                if a1 != b":memory:" && !a1.is_empty() {
                    if a1.starts_with(b"file:") {
                        // starts with "file:"
                        return SQLITE_DENY;
                    }
                    let name = crate::conn::active_name(vm);
                    let p = String::from_utf8_lossy(a1).into_owned();
                    if !vm.check_open_basedir(&name, &p) {
                        return SQLITE_DENY;
                    }
                }
            }

            // fallback to access allowed if authorizer callback is not
            // defined
            let Some(cb) = shared.authorizer_fcc.borrow().clone() else {
                return SQLITE_OK;
            };

            // call userland authorizer callback, if set
            let argv = [
                Value::Long(action as i64),
                opt_str(arg1),
                opt_str(arg2),
                opt_str(arg3),
                opt_str(arg4),
            ];
            let mut authreturn = SQLITE_DENY;
            match call(vm, &cb, &argv) {
                None => php_sqlite3_error(
                    vm,
                    Some(&shared),
                    0,
                    "An error occurred while invoking the authorizer callback",
                ),
                Some(Value::Long(n)) => {
                    authreturn = n as i32;
                    if authreturn != SQLITE_OK
                        && authreturn != SQLITE_IGNORE
                        && authreturn != SQLITE_DENY
                    {
                        php_sqlite3_error(
                            vm,
                            Some(&shared),
                            0,
                            &format!(
                                "The authorizer callback returned an invalid value: {}",
                                authreturn
                            ),
                        );
                        authreturn = SQLITE_DENY;
                    }
                }
                Some(_) => php_sqlite3_error(
                    vm,
                    Some(&shared),
                    0,
                    "The authorizer callback returned an invalid type: expected int",
                ),
            }
            authreturn
        },
    )
}
