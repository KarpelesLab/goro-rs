//! curl_share_* (ext/curl/share.c)

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::array::PhpArray;

const CURLSHE_OK: i64 = 0;
const CURLSHE_BAD_OPTION: i64 = 1;
use goro_core::object::{NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::consts::c::*;
use crate::engine::Share;
use crate::zpp::{self, Obj};

pub(crate) struct ShareObj {
    pub share: Rc<RefCell<Share>>,
    pub err_no: RefCell<i64>,
}

thread_local! {
    /// CURL_G(persistent_curlsh): persistent share handles by option set.
    static PERSISTENT: RefCell<HashMap<u64, Rc<RefCell<Share>>>> = RefCell::new(HashMap::new());
}

pub(crate) fn share_of(o: &Obj) -> Option<Rc<ShareObj>> {
    let d = o.borrow().native.as_ref().map(|n| n.data.clone())?;
    d.downcast::<ShareObj>().ok()
}

fn new_share_object(vm: &mut Vm, class: &[u8], share: Rc<RefCell<Share>>) -> Value {
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(class.to_vec(), id);
    obj.native = Some(NativeData {
        data: Rc::new(ShareObj {
            share,
            err_no: RefCell::new(0),
        }) as Rc<dyn std::any::Any>,
        handlers: &crate::SHARE_HANDLERS,
    });
    Value::Object(Rc::new(RefCell::new(obj)))
}

pub fn curl_share_init(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    if zpp::parse(vm, f, "", args).is_err() {
        return Value::Null;
    }
    new_share_object(vm, b"CurlShareHandle", Rc::new(RefCell::new(Share::new())))
}

pub fn curl_share_close(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    vm.emit_deprecated(
        "Function curl_share_close() is deprecated since 8.5, as it has no effect since PHP 8.0",
    );
    if vm.current_exception.is_some() {
        return Value::Null;
    }
    let _ = zpp::parse(vm, f, "H", args);
    Value::Null
}

pub fn curl_share_setopt(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Hlz", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let Some(sh) = share_of(&a[0].obj()) else {
        return Value::False;
    };
    let option = a[1].long();
    let error = match option {
        CURLSHOPT_SHARE | CURLSHOPT_UNSHARE => {
            let v = zpp::zval_get_long(vm, &a[2].any());
            sh.share.borrow_mut().setopt(option as u32, v) as i64
        }
        _ => {
            zpp::value_error(vm, f, 2, "is not a valid cURL share option");
            CURLSHE_BAD_OPTION
        }
    };
    *sh.err_no.borrow_mut() = error;
    if error == CURLSHE_OK {
        Value::True
    } else {
        Value::False
    }
}

pub fn curl_share_errno(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "H", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    match share_of(&a[0].obj()) {
        Some(sh) => Value::Long(*sh.err_no.borrow()),
        None => Value::Long(0),
    }
}

pub fn curl_share_strerror(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    Value::String(PhpString::from_bytes(
        crate::engine::share_strerror(a[0].long() as u32).as_bytes(),
    ))
}

pub fn curl_share_init_persistent(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "a", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let zpp::Arg::Arr(opts) = &a[0] else {
        return Value::Null;
    };
    let entries: Vec<Value> = opts.borrow().values().cloned().collect();
    if entries.is_empty() {
        zpp::value_error(vm, f, 1, "must not be empty");
        return Value::Null;
    }
    let mut persistent_id: u64 = 0;
    for e in &entries {
        let Some(option) = zpp::try_get_long(e) else {
            let ty = vm_value_name(&e.deref());
            zpp::arg_type_error(
                vm,
                f,
                1,
                &format!("must contain only int values, {} given", ty),
            );
            return Value::Null;
        };
        match option {
            CURL_LOCK_DATA_COOKIE => {
                zpp::value_error(
                    vm,
                    f,
                    1,
                    "must not contain CURL_LOCK_DATA_COOKIE because sharing cookies across PHP requests is unsafe",
                );
                return Value::Null;
            }
            CURL_LOCK_DATA_DNS => persistent_id |= 1,
            CURL_LOCK_DATA_SSL_SESSION => persistent_id |= 2,
            CURL_LOCK_DATA_CONNECT => persistent_id |= 4,
            CURL_LOCK_DATA_PSL => persistent_id |= 8,
            _ => {
                zpp::value_error(vm, f, 1, "must contain only CURL_LOCK_DATA_* constants");
                return Value::Null;
            }
        }
    }
    let mut prop = PhpArray::new();
    let list = [
        (1, CURL_LOCK_DATA_DNS),
        (2, CURL_LOCK_DATA_SSL_SESSION),
        (4, CURL_LOCK_DATA_CONNECT),
        (8, CURL_LOCK_DATA_PSL),
    ];
    for (bit, v) in list {
        if persistent_id & bit != 0 {
            prop.push(Value::Long(v));
        }
    }
    let share = PERSISTENT.with(|p| p.borrow().get(&persistent_id).cloned());
    let share = match share {
        Some(s) => s,
        None => {
            let s = Rc::new(RefCell::new(Share::new()));
            for (bit, v) in list {
                if persistent_id & bit != 0 {
                    let e = s.borrow_mut().setopt(CURLSHOPT_SHARE as u32, v);
                    if e != 0 {
                        zpp::throw(
                            vm,
                            b"Exception",
                            &format!(
                                "Could not construct persistent cURL share handle: {}",
                                crate::engine::share_strerror(e)
                            ),
                        );
                        return Value::Null;
                    }
                }
            }
            PERSISTENT.with(|p| p.borrow_mut().insert(persistent_id, s.clone()));
            s
        }
    };
    let obj = new_share_object(vm, b"CurlSharePersistentHandle", share);
    if let Value::Object(o) = &obj {
        o.borrow_mut().set_property(
            b"options".to_vec(),
            Value::Array(Rc::new(RefCell::new(prop))),
        );
    }

    obj
}

fn vm_value_name(v: &Value) -> String {
    goro_core::vm::Vm::value_type_name(v)
}
