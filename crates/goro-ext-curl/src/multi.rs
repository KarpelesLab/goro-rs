//! curl_multi_* (ext/curl/multi.c). Transfers of a multi handle run to
//! completion, one after the other, when curl_multi_exec() is called.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_standard::curl_streams as streams;

use crate::consts::c::*;
use crate::zpp::{self, Obj};
use crate::{CurlObj, PHP_CURL_RETURN, cleanup_handle, curl_of, perform, verify_handlers};

const CURLMSG_DONE: i64 = 1;
const CURLM_UNKNOWN_OPTION: i64 = 6;

pub(crate) struct MultiObj {
    pub st: RefCell<MultiState>,
}

pub(crate) struct MultiState {
    /// The easy handles, in the order they were added.
    pub easyh: Vec<Value>,
    /// Handles still to transfer.
    pub pending: Vec<u64>,
    pub err_no: i64,
    pub server_push: Option<Value>,
    pub msgs: VecDeque<(Value, i64)>,
    pub pool: crate::engine::ConnPool,
}

fn multi_of(o: &Obj) -> Option<Rc<MultiObj>> {
    let d = o.borrow().native.as_ref().map(|n| n.data.clone())?;
    d.downcast::<MultiObj>().ok()
}

fn arg_multi(o: &Obj) -> Rc<MultiObj> {
    multi_of(o).unwrap_or_else(|| Rc::new(new_multi()))
}

fn new_multi() -> MultiObj {
    MultiObj {
        st: RefCell::new(MultiState {
            easyh: Vec::new(),
            pending: Vec::new(),
            err_no: 0,
            server_push: None,
            msgs: VecDeque::new(),
            pool: crate::engine::ConnPool::new(),
        }),
    }
}

fn obj_id(v: &Value) -> u64 {
    match v {
        Value::Object(o) => o.borrow().object_id,
        _ => 0,
    }
}

fn easy_of(v: &Value) -> Option<Rc<CurlObj>> {
    match v {
        Value::Object(o) => curl_of(o),
        _ => None,
    }
}

pub fn curl_multi_init(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    if zpp::parse(vm, f, "", args).is_err() {
        return Value::Null;
    }
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(b"CurlMultiHandle".to_vec(), id);
    obj.native = Some(NativeData {
        data: Rc::new(new_multi()) as Rc<dyn std::any::Any>,
        handlers: &crate::MULTI_HANDLERS,
    });
    Value::Object(Rc::new(RefCell::new(obj)))
}

pub fn curl_multi_add_handle(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "MC", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let mo = a[0].obj();
    let m = arg_multi(&mo);
    let chv = Value::Object(a[1].obj());
    let Some(c) = easy_of(&chv) else {
        return Value::Long(CURLM_BAD_EASY_HANDLE);
    };
    verify_handlers(vm, &c, true);
    cleanup_handle(&c);
    let mid = mo.borrow().object_id;
    let error = if c.st.borrow().multi.is_some() {
        CURLM_ADDED_ALREADY
    } else {
        c.st.borrow_mut().multi = Some(mid);
        let pool = m.st.borrow().pool.clone();
        crate::with_easy(&c, |e| e.set_conn_pool(Some(pool)));
        CURLM_OK
    };
    let mut st = m.st.borrow_mut();
    st.err_no = error;
    if error == CURLM_OK {
        st.pending.push(obj_id(&chv));
        st.easyh.push(chv);
    }
    Value::Long(error)
}

/// curl_multi_remove_handle() of libcurl
fn remove_easy(m: &MultiObj, mid: u64, chv: &Value) -> i64 {
    let Some(c) = easy_of(chv) else {
        return CURLM_BAD_EASY_HANDLE;
    };
    let owner = c.st.borrow().multi;
    match owner {
        None => return CURLM_OK,
        Some(o) if o != mid => return CURLM_BAD_EASY_HANDLE,
        _ => {}
    }
    c.st.borrow_mut().multi = None;
    crate::with_easy(&c, |e| e.set_conn_pool(None));
    let id = obj_id(chv);
    let mut st = m.st.borrow_mut();
    st.pending.retain(|p| *p != id);
    st.msgs.retain(|(h, _)| obj_id(h) != id);
    CURLM_OK
}

pub fn curl_multi_remove_handle(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "MC", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let mo = a[0].obj();
    let m = arg_multi(&mo);
    let mid = mo.borrow().object_id;
    let chv = Value::Object(a[1].obj());
    let error = remove_easy(&m, mid, &chv);
    let mut st = m.st.borrow_mut();
    st.err_no = error;
    if error == CURLM_OK {
        let id = obj_id(&chv);
        if let Some(p) = st.easyh.iter().position(|v| obj_id(v) == id) {
            st.easyh.remove(p);
        }
    }
    Value::Long(error)
}

pub fn curl_multi_get_handles(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "M", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let m = arg_multi(&a[0].obj());
    let mut arr = PhpArray::new();
    for h in m.st.borrow().easyh.iter() {
        arr.push(h.clone());
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}

pub fn curl_multi_select(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "M|d", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let timeout = match &a[1] {
        zpp::Arg::Double(d) => *d,
        _ => 1.0,
    };
    let max = i32::MAX as f64 / 1000.0;
    if !(timeout >= 0.0 && timeout <= max) {
        zpp::value_error(vm, f, 2, &format!("must be between 0 and {:.6}", max));
        return Value::Null;
    }
    Value::Long(0)
}

pub fn curl_multi_exec(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Mz", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let m = arg_multi(&a[0].obj());
    let handles: Vec<Value> = m.st.borrow().easyh.clone();
    for h in &handles {
        if let Some(c) = easy_of(h) {
            verify_handlers(vm, &c, true);
        }
    }
    // curl_multi_perform(): run the transfers still to do
    loop {
        let next = {
            let mut st = m.st.borrow_mut();
            if st.pending.is_empty() {
                None
            } else {
                Some(st.pending.remove(0))
            }
        };
        let Some(id) = next else { break };
        let Some(h) = handles.iter().find(|v| obj_id(v) == id).cloned() else {
            continue;
        };
        let Some(c) = easy_of(&h) else { continue };
        let code = perform(vm, &h, &c);
        if code != 0 {
            let mut cs = c.st.borrow_mut();
            if cs.handlers.write.method == PHP_CURL_RETURN {
                cs.handlers.write.buf.clear();
                cs.handlers.write.buf_set = false;
            }
        }
        if let Some(id) = crate::stream_id(&c.st.borrow().handlers.std_err) {
            if streams::is_stream(id) {
                streams::flush(vm, id);
            }
        }
        m.st.borrow_mut().msgs.push_back((h.clone(), code as i64));
        if vm.current_exception.is_some() {
            break;
        }
    }
    let still_running = m.st.borrow().pending.len() as i64;
    if let Value::Reference(r) = &a[1].any() {
        *r.borrow_mut() = Value::Long(still_running);
    }
    m.st.borrow_mut().err_no = CURLM_OK;
    Value::Long(CURLM_OK)
}

pub fn curl_multi_getcontent(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "C", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(c) = curl_of(&a[0].obj()) else {
        return Value::Null;
    };
    let st = c.st.borrow();
    if st.handlers.write.method == PHP_CURL_RETURN {
        return Value::String(PhpString::from_bytes(&st.handlers.write.buf));
    }
    Value::Null
}

pub fn curl_multi_info_read(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "M|z", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let m = arg_multi(&a[0].obj());
    let msg = m.st.borrow_mut().msgs.pop_front();
    let Some((h, result)) = msg else {
        return Value::False;
    };
    let queued = m.st.borrow().msgs.len() as i64;
    if let Value::Reference(r) = &a[1].any() {
        *r.borrow_mut() = Value::Long(queued);
    }
    let mut arr = PhpArray::new();
    let k = |s: &str| ArrayKey::String(PhpString::from_string(s.to_string()));
    arr.set(k("msg"), Value::Long(CURLMSG_DONE));
    arr.set(k("result"), Value::Long(result));
    if let Some(c) = easy_of(&h) {
        c.st.borrow_mut().err_no = result;
        arr.set(k("handle"), h);
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}

pub fn curl_multi_close(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "M", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let mo = a[0].obj();
    let m = arg_multi(&mo);
    let mid = mo.borrow().object_id;
    let handles: Vec<Value> = m.st.borrow().easyh.clone();
    for h in &handles {
        if let Some(c) = easy_of(h) {
            verify_handlers(vm, &c, true);
        }
        remove_easy(&m, mid, h);
    }
    m.st.borrow_mut().easyh.clear();
    Value::Null
}

pub fn curl_multi_errno(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "M", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let m = arg_multi(&a[0].obj());
    Value::Long(m.st.borrow().err_no)
}

pub fn curl_multi_strerror(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    Value::String(PhpString::from_bytes(
        crate::engine::multi_strerror(a[0].long() as i32).as_bytes(),
    ))
}

pub fn curl_multi_setopt(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Mlz", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let m = arg_multi(&a[0].obj());
    let option = a[1].long();
    let zvalue = a[2].any().deref();
    let error = match option {
        CURLMOPT_PIPELINING
        | CURLMOPT_MAXCONNECTS
        | CURLMOPT_CHUNK_LENGTH_PENALTY_SIZE
        | CURLMOPT_CONTENT_LENGTH_PENALTY_SIZE
        | CURLMOPT_MAX_HOST_CONNECTIONS
        | CURLMOPT_MAX_PIPELINE_LENGTH
        | CURLMOPT_MAX_TOTAL_CONNECTIONS
        | CURLMOPT_MAX_CONCURRENT_STREAMS => {
            let lval = zpp::zval_get_long(vm, &zvalue);
            if option == CURLMOPT_PIPELINING && (lval & 1) != 0 {
                vm.emit_warning(&format!("{}(): CURLPIPE_HTTP1 is no longer supported", f));
            }
            CURLM_OK
        }
        CURLMOPT_PUSHFUNCTION => {
            m.st.borrow_mut().server_push = None;
            if let Err(e) = streams::check_callable(vm, &zvalue) {
                if vm.current_exception.is_none() {
                    zpp::arg_type_error(
                        vm,
                        f,
                        2,
                        &format!(
                            "must be a valid callback for option CURLMOPT_PUSHFUNCTION, {}",
                            e
                        ),
                    );
                }
                return Value::False;
            }
            m.st.borrow_mut().server_push = Some(zvalue.clone());
            CURLM_OK
        }
        _ => {
            zpp::value_error(vm, f, 2, "is not a valid cURL multi option");
            CURLM_UNKNOWN_OPTION
        }
    };
    m.st.borrow_mut().err_no = error;
    if error == CURLM_OK {
        Value::True
    } else {
        Value::False
    }
}
