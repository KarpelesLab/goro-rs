//! Multi and share handles (PHP manual: curl_multi_* and curl_share_*
//! functions). Transfers added to a multi handle run when
//! curl_multi_exec() is called; each completes within that call, then
//! its CURLMSG_DONE message waits for curl_multi_info_read(). Messages,
//! return codes and option checks observed on PHP 8.5 and in the ext/curl
//! tests.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use goro_ext_curl::consts as c;
use goro_ext_curl::cookie::CookieJar;
use goro_ext_curl::transfer::Pool;
use goro_runtime::{Key, ObjRef, PhpArray, Value};

use super::easy::{self, with_easy};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::vm::{R, Vm};

// ------------------------------------------------------------ multi

#[derive(Default)]
struct Multi {
    handles: Vec<ObjRef>,
    /// Handles whose transfer has not run yet.
    pending: Vec<ObjRef>,
    msgs: VecDeque<(i64, ObjRef)>,
    errno: i64,
    pool: Rc<RefCell<Pool>>,
    push: Option<Value>,
}

struct MultiObj(Multi);

impl goro_runtime::NativeData for MultiObj {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        for h in &self.0.handles {
            visit(&Value::Object(h.clone()));
        }
        for (_, h) in &self.0.msgs {
            visit(&Value::Object(h.clone()));
        }
        if let Some(p) = &self.0.push {
            visit(p);
        }
    }
    fn clear(&mut self) {
        self.0.handles.clear();
        self.0.pending.clear();
        self.0.msgs.clear();
        self.0.push = None;
    }
}

fn with_multi<T>(o: &ObjRef, f: impl FnOnce(&mut Multi) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<MultiObj>().map(|m| f(&mut m.0))
}

pub(super) fn multi_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(MultiObj(Multi::default())));
        }),
        cast: Some(|_, o, to| match to {
            crate::bytecode::CastTo::Int => Some(Ok(Value::Int(o.handle() as i64))),
            _ => None,
        }),
        destroy: Some(|_, o| {
            let hs = with_multi(o, |m| std::mem::take(&mut m.handles)).unwrap_or_default();
            for h in hs {
                with_easy(&h, |e| e.in_multi = false);
            }
        }),
        ..Default::default()
    }
}

pub(super) fn no_construct_multi(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.throw(
        "Error",
        "Cannot directly construct CurlMultiHandle, use curl_multi_init() instead",
    )
}

fn multi_arg(a: &Args) -> ObjRef {
    a.obj(0).expect("CurlMultiHandle argument")
}

pub(super) fn curl_multi_init(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let cls = vm
        .find_class(b"CurlMultiHandle", false)?
        .expect("CurlMultiHandle is declared");
    Ok(Value::Object(vm.instantiate_raw(&cls)?))
}

pub(super) fn curl_multi_add_handle(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = multi_arg(a);
    let h = a.obj(1).expect("CurlHandle argument");
    if with_easy(&h, |e| e.in_multi).unwrap_or(false) {
        with_multi(&m, |m| m.errno = c::CURLM_ADDED_ALREADY);
        return Ok(Value::Int(c::CURLM_ADDED_ALREADY));
    }
    easy::check_streams(vm, &a.fname(), &h)?;
    with_easy(&h, |e| e.in_multi = true);
    with_multi(&m, |m| {
        m.errno = 0;
        m.handles.push(h.clone());
        m.pending.push(h);
    });
    Ok(Value::Int(c::CURLM_OK))
}

pub(super) fn curl_multi_remove_handle(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = multi_arg(a);
    let h = a.obj(1).expect("CurlHandle argument");
    let found = with_multi(&m, |m| {
        m.errno = 0;
        let before = m.handles.len();
        m.handles.retain(|x| !ObjRef::ptr_eq(x, &h));
        m.pending.retain(|x| !ObjRef::ptr_eq(x, &h));
        m.msgs.retain(|(_, x)| !ObjRef::ptr_eq(x, &h));
        before != m.handles.len()
    })
    .unwrap_or(false);
    if found {
        with_easy(&h, |e| e.in_multi = false);
    }
    Ok(Value::Int(c::CURLM_OK))
}

pub(super) fn curl_multi_get_handles(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let hs = with_multi(&multi_arg(a), |m| m.handles.clone()).unwrap_or_default();
    let mut arr = PhpArray::new();
    for h in hs {
        let _ = arr.push(Value::Object(h));
    }
    Ok(Value::Array(arr))
}

pub(super) fn curl_multi_close(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = multi_arg(a);
    let hs = with_multi(&m, |m| {
        m.pending.clear();
        m.msgs.clear();
        std::mem::take(&mut m.handles)
    })
    .unwrap_or_default();
    for h in hs {
        with_easy(&h, |e| e.in_multi = false);
    }
    Ok(Value::Null)
}

pub(super) fn curl_multi_errno(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(
        with_multi(&multi_arg(a), |m| m.errno).unwrap_or(0),
    ))
}

pub(super) fn curl_multi_strerror(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::from(
        goro_ext_curl::strerror::multi(a.int(0)).as_bytes(),
    ))
}

pub(super) fn curl_multi_exec(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = multi_arg(a);
    loop {
        let next = with_multi(&m, |m| {
            if m.pending.is_empty() {
                None
            } else {
                Some((m.pending.remove(0), m.pool.clone()))
            }
        })
        .flatten();
        let Some((h, pool)) = next else { break };
        let r = easy::run(vm, a, &h, Some(pool));
        let code = match &r {
            Ok(Ok(())) => 0,
            Ok(Err(f)) => f.code,
            Err(_) => 0,
        };
        with_multi(&m, |m| {
            if m.handles.iter().any(|x| ObjRef::ptr_eq(x, &h)) {
                m.msgs.push_back((code, h.clone()));
            }
        });
        let _ = r?;
    }
    a.set_ref(1, Value::Int(0));
    with_multi(&m, |m| m.errno = 0);
    Ok(Value::Int(c::CURLM_OK))
}

pub(super) fn curl_multi_info_read(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = multi_arg(a);
    let (msg, left) = with_multi(&m, |m| (m.msgs.pop_front(), m.msgs.len())).unwrap_or((None, 0));
    if a.has(1) {
        a.set_ref(1, Value::Int(left as i64));
    }
    let Some((code, h)) = msg else {
        return Ok(Value::Bool(false));
    };
    let mut arr = PhpArray::new();
    arr.insert(Key::from("msg"), Value::Int(c::CURLMSG_DONE));
    arr.insert(Key::from("result"), Value::Int(code));
    arr.insert(Key::from("handle"), Value::Object(h));
    Ok(Value::Array(arr))
}

pub(super) fn curl_multi_select(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let t = if a.has(1) { a.float(1) } else { 1.0 };
    if !(0.0..=2147483.647).contains(&t) || t.is_nan() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($timeout) must be between 0 and 2147483.647000",
        );
    }
    // Pending transfers run in the next curl_multi_exec(): ready now.
    let ready = with_multi(&multi_arg(a), |m| m.pending.len()).unwrap_or(0);
    Ok(Value::Int(ready as i64))
}

pub(super) fn curl_multi_setopt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = multi_arg(a);
    let opt = a.int(1);
    let v = a.val(2);
    match opt {
        c::CURLMOPT_PUSHFUNCTION => {
            if !v.is_null() && !vm.is_callable_value(&v, false) {
                let why = vm.callable_error(&v);
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!(
                        "Argument #2 ($option) must be a valid callback for option CURLMOPT_PUSHFUNCTION, {why}"
                    ),
                );
            }
            let cb = if v.is_null() { None } else { Some(v) };
            with_multi(&m, |m| {
                m.push = cb;
                m.errno = 0;
            });
            Ok(Value::Bool(true))
        }
        c::CURLMOPT_PIPELINING => {
            let n = vm.to_int(&v)?;
            if n & 1 != 0 {
                vm.fwarn(a, "CURLPIPE_HTTP1 is no longer supported")?;
            }
            with_multi(&m, |m| m.errno = 0);
            Ok(Value::Bool(true))
        }
        c::CURLMOPT_MAXCONNECTS
        | c::CURLMOPT_MAX_HOST_CONNECTIONS
        | c::CURLMOPT_MAX_PIPELINE_LENGTH
        | c::CURLMOPT_MAX_TOTAL_CONNECTIONS
        | c::CURLMOPT_CONTENT_LENGTH_PENALTY_SIZE
        | c::CURLMOPT_CHUNK_LENGTH_PENALTY_SIZE
        | c::CURLMOPT_MAX_CONCURRENT_STREAMS => {
            vm.to_int(&v)?;
            with_multi(&m, |m| m.errno = 0);
            Ok(Value::Bool(true))
        }
        _ => {
            with_multi(&m, |m| m.errno = 6);
            vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($option) is not a valid cURL multi option",
            )
        }
    }
}

// ------------------------------------------------------------ share

/// What a share handle shares.
#[derive(Default)]
pub(super) struct Share {
    cookies: Option<Rc<RefCell<CookieJar>>>,
    connections: Option<Rc<RefCell<Pool>>>,
    errno: i64,
}

struct ShareObj(Rc<RefCell<Share>>);

impl goro_runtime::NativeData for ShareObj {}

fn with_share<T>(o: &ObjRef, f: impl FnOnce(&Rc<RefCell<Share>>) -> T) -> Option<T> {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<ShareObj>().map(|s| f(&s.0))
}

/// The cookie store a share handle shares, if any.
pub(super) fn share_jar(o: &ObjRef) -> Option<Rc<RefCell<CookieJar>>> {
    with_share(o, |s| s.borrow().cookies.clone()).flatten()
}

/// The connection cache a share handle shares, if any.
pub(super) fn share_pool(o: &ObjRef) -> Option<Rc<RefCell<Pool>>> {
    with_share(o, |s| s.borrow().connections.clone()).flatten()
}

pub(super) fn share_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            *o.obj().native.borrow_mut() =
                Some(Box::new(ShareObj(Rc::new(RefCell::new(Share::default())))));
        }),
        cast: Some(|_, o, to| match to {
            crate::bytecode::CastTo::Int => Some(Ok(Value::Int(o.handle() as i64))),
            _ => None,
        }),
        ..Default::default()
    }
}

pub(super) fn no_construct_share(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.throw(
        "Error",
        "Cannot directly construct CurlShareHandle, use curl_share_init() instead",
    )
}

pub(super) fn no_construct_persistent(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.throw(
        "Error",
        "Cannot directly construct CurlSharePersistentHandle, use curl_share_init_persistent() instead",
    )
}

pub(super) fn curl_share_init(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let cls = vm
        .find_class(b"CurlShareHandle", false)?
        .expect("CurlShareHandle is declared");
    Ok(Value::Object(vm.instantiate_raw(&cls)?))
}

pub(super) fn curl_share_close(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.deprecated(
        "Function curl_share_close() is deprecated since 8.5, as it has no effect since PHP 8.0",
    )?;
    Ok(Value::Null)
}

pub(super) fn curl_share_errno(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.obj(0).expect("CurlShareHandle argument");
    Ok(Value::Int(
        with_share(&o, |s| s.borrow().errno).unwrap_or(0),
    ))
}

pub(super) fn curl_share_strerror(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::from(
        goro_ext_curl::strerror::share(a.int(0)).as_bytes(),
    ))
}

fn apply_lock(s: &mut Share, data: i64, on: bool) -> bool {
    match data {
        c::CURL_LOCK_DATA_COOKIE => {
            s.cookies = if on {
                Some(s.cookies.take().unwrap_or_default())
            } else {
                None
            };
            true
        }
        c::CURL_LOCK_DATA_CONNECT => {
            s.connections = if on {
                Some(s.connections.take().unwrap_or_default())
            } else {
                None
            };
            true
        }
        c::CURL_LOCK_DATA_DNS | c::CURL_LOCK_DATA_SSL_SESSION | c::CURL_LOCK_DATA_PSL => true,
        _ => false,
    }
}

pub(super) fn curl_share_setopt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.obj(0).expect("CurlShareHandle argument");
    let opt = a.int(1);
    let v = a.val(2);
    if opt != c::CURLSHOPT_SHARE && opt != c::CURLSHOPT_UNSHARE {
        with_share(&o, |s| s.borrow_mut().errno = 1);
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($option) is not a valid cURL share option",
        );
    }
    let data = vm.to_int(&v)?;
    let ok = with_share(&o, |s| {
        let mut s = s.borrow_mut();
        let ok = apply_lock(&mut s, data, opt == c::CURLSHOPT_SHARE);
        s.errno = if ok { 0 } else { 1 };
        ok
    })
    .unwrap_or(false);
    Ok(Value::Bool(ok))
}

/// Persistent shares, by their sorted option list (PHP manual:
/// curl_share_init_persistent(); one per process here).
#[derive(Default)]
struct Persistent(HashMap<Vec<i64>, Rc<RefCell<Share>>>);

pub(super) fn curl_share_init_persistent(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    if arr.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($share_options) must not be empty",
        );
    }
    let mut opts = Vec::new();
    for (_, v) in arr.iter() {
        let n = match v.deref() {
            Value::Int(n) => n,
            other => vm.to_int(&other)?,
        };
        if n == c::CURL_LOCK_DATA_COOKIE {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #1 ($share_options) must not contain CURL_LOCK_DATA_COOKIE because sharing cookies across PHP requests is unsafe",
            );
        }
        if !matches!(
            n,
            c::CURL_LOCK_DATA_DNS
                | c::CURL_LOCK_DATA_SSL_SESSION
                | c::CURL_LOCK_DATA_CONNECT
                | c::CURL_LOCK_DATA_PSL
        ) {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #1 ($share_options) must contain only CURL_LOCK_DATA_* constants",
            );
        }
        opts.push(n);
    }
    opts.sort_unstable();
    opts.dedup();
    let shared = vm
        .ext::<Persistent>()
        .0
        .entry(opts.clone())
        .or_insert_with(|| {
            let mut s = Share::default();
            for &o in &opts {
                apply_lock(&mut s, o, true);
            }
            Rc::new(RefCell::new(s))
        })
        .clone();
    let cls = vm
        .find_class(b"CurlSharePersistentHandle", false)?
        .expect("CurlSharePersistentHandle is declared");
    let o = vm.instantiate_raw(&cls)?;
    *o.obj().native.borrow_mut() = Some(Box::new(ShareObj(shared)));
    let mut list = PhpArray::new();
    for n in opts {
        let _ = list.push(Value::Int(n));
    }
    vm.set_prop_raw(&o, b"options", Value::Array(list));
    Ok(Value::Object(o))
}
