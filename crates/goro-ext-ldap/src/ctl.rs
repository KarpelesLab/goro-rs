//! Control <-> PHP array conversion (php_ldap_control_from_array and
//! _php_ldap_control_to_array).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ldap::consts::*;
use goro_ldap::controls::{self, SortKey};
use goro_ldap::message::Control;

use crate::zpp::throw;

fn akey(s: &[u8]) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(s))
}
fn sval(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

fn arr_get<'a>(a: &'a PhpArray, key: &[u8]) -> Option<&'a Value> {
    a.get_str(key)
}

fn as_array(v: &Value) -> Option<Rc<RefCell<PhpArray>>> {
    match v.deref() {
        Value::Array(a) => Some(a),
        _ => None,
    }
}

/// The active function name for the "%s(): Control must..." messages.
pub fn control_from_array(vm: &mut Vm, fname: &str, ctrl: &PhpArray) -> Result<Control, ()> {
    let Some(oidv) = arr_get(ctrl, b"oid") else {
        throw(
            vm,
            b"ValueError",
            &format!("{}(): Control must have an \"oid\" key", fname),
        );
        return Err(());
    };
    let oid = vm.value_to_string(&oidv.deref()).as_bytes().to_vec();
    if vm.current_exception.is_some() {
        return Err(());
    }
    let criticality = arr_get(ctrl, b"iscritical")
        .map(|v| v.is_truthy())
        .unwrap_or(false);

    let mut value: Option<Vec<u8>> = None;
    if let Some(val) = arr_get(ctrl, b"value") {
        let val = val.deref();
        if !matches!(val, Value::Array(_)) {
            value = Some(vm.value_to_string(&val).as_bytes().to_vec());
            if vm.current_exception.is_some() {
                return Err(());
            }
        } else {
            let a = as_array(&val).unwrap();
            let a = a.borrow();
            let oid_s = oid.as_slice();
            if oid_s == LDAP_CONTROL_PAGEDRESULTS.as_bytes() {
                let size = arr_get(&a, b"size").map(|v| v.to_long()).unwrap_or(1);
                let cookie = arr_get(&a, b"cookie")
                    .map(|v| vm.value_to_string(&v.deref()).as_bytes().to_vec())
                    .unwrap_or_default();
                value = Some(controls::page_value(size, &cookie));
            } else if oid_s == LDAP_CONTROL_ASSERT.as_bytes() {
                let Some(f) = arr_get(&a, b"filter") else {
                    throw(
                        vm,
                        b"ValueError",
                        &format!("{}(): Control must have a \"filter\" key", fname),
                    );
                    return Err(());
                };
                let fs = vm.value_to_string(&f.deref()).as_bytes().to_vec();
                value = controls::assertion_value(&fs);
                if value.is_none() {
                    vm.emit_warning(&format!(
                        "{}(): Failed to create assert control value: {} ({})",
                        fname,
                        goro_ldap::err2string(LDAP_PARAM_ERROR),
                        LDAP_PARAM_ERROR
                    ));
                }
            } else if oid_s == LDAP_CONTROL_VALUESRETURNFILTER.as_bytes() {
                let Some(f) = arr_get(&a, b"filter") else {
                    throw(
                        vm,
                        b"ValueError",
                        &format!("{}(): Control must have a \"filter\" key", fname),
                    );
                    return Err(());
                };
                let fs = vm.value_to_string(&f.deref()).as_bytes().to_vec();
                match controls::valuesreturnfilter_value(&fs) {
                    Some(v) => value = Some(v),
                    None => {
                        vm.emit_warning(&format!(
                            "{}(): Failed to create control value: Bad ValuesReturnFilter: {}",
                            fname,
                            String::from_utf8_lossy(&fs)
                        ));
                    }
                }
            } else if oid_s == LDAP_CONTROL_PRE_READ.as_bytes()
                || oid_s == LDAP_CONTROL_POST_READ.as_bytes()
            {
                let Some(attrs) = arr_get(&a, b"attrs").and_then(as_array) else {
                    throw(
                        vm,
                        b"ValueError",
                        &format!("{}(): Control must have an \"attrs\" key", fname),
                    );
                    return Err(());
                };
                let ab = attrs.borrow();
                let n = ab.len();
                let mut list = Vec::new();
                for i in 0..n {
                    let v = ab.get(&ArrayKey::Int(i as i64));
                    let Some(v) = v else {
                        vm.emit_warning(&format!("{}(): Failed to encode attribute list", fname));
                        return Err(());
                    };
                    list.push(vm.value_to_string(&v.deref()).as_bytes().to_vec());
                    if vm.current_exception.is_some() {
                        return Err(());
                    }
                }
                value = Some(controls::attrs_value(&list));
            } else if oid_s == LDAP_CONTROL_SORTREQUEST.as_bytes() {
                let n = a.len();
                let mut keys = Vec::new();
                for i in 0..n {
                    let Some(sk) = a.get(&ArrayKey::Int(i as i64)).and_then(as_array) else {
                        vm.emit_warning(&format!("{}(): Failed to encode sort keys list", fname));
                        return Err(());
                    };
                    let skb = sk.borrow();
                    let Some(attr) = arr_get(&skb, b"attr") else {
                        throw(
                            vm,
                            b"ValueError",
                            &format!("{}(): Sort key list must have an \"attr\" key", fname),
                        );
                        return Err(());
                    };
                    let attr = vm.value_to_string(&attr.deref()).as_bytes().to_vec();
                    if vm.current_exception.is_some() {
                        return Err(());
                    }
                    let oid = arr_get(&skb, b"oid")
                        .map(|v| vm.value_to_string(&v.deref()).as_bytes().to_vec());
                    let reverse = arr_get(&skb, b"reverse")
                        .map(|v| v.is_truthy())
                        .unwrap_or(false);
                    keys.push(SortKey { attr, oid, reverse });
                }
                value = Some(controls::sort_value(&keys));
            } else if oid_s == LDAP_CONTROL_VLVREQUEST.as_bytes() {
                let before = match arr_get(&a, b"before") {
                    Some(v) => v.to_long(),
                    None => {
                        throw(
                            vm,
                            b"ValueError",
                            &format!(
                                "{}(): Array value for VLV control must have a \"before\" key",
                                fname
                            ),
                        );
                        return Err(());
                    }
                };
                let after = match arr_get(&a, b"after") {
                    Some(v) => v.to_long(),
                    None => {
                        throw(
                            vm,
                            b"ValueError",
                            &format!(
                                "{}(): Array value for VLV control must have an \"after\" key",
                                fname
                            ),
                        );
                        return Err(());
                    }
                };
                let attrvalue = arr_get(&a, b"attrvalue")
                    .map(|v| vm.value_to_string(&v.deref()).as_bytes().to_vec());
                let (offset, count) = if attrvalue.is_some() {
                    (0, 0)
                } else if let Some(off) = arr_get(&a, b"offset") {
                    let offset = off.to_long();
                    let count = match arr_get(&a, b"count") {
                        Some(v) => v.to_long(),
                        None => {
                            throw(
                                vm,
                                b"ValueError",
                                &format!(
                                    "{}(): Array value for VLV control must have a \"count\" key",
                                    fname
                                ),
                            );
                            return Err(());
                        }
                    };
                    (offset, count)
                } else {
                    throw(
                        vm,
                        b"ValueError",
                        &format!(
                            "{}(): Array value for VLV control must have either an \"attrvalue\" or an \"offset\" key",
                            fname
                        ),
                    );
                    return Err(());
                };
                let context = arr_get(&a, b"context")
                    .map(|v| vm.value_to_string(&v.deref()).as_bytes().to_vec());
                value = Some(controls::vlv_value(
                    before,
                    after,
                    attrvalue.as_deref(),
                    offset,
                    count,
                    context.as_deref(),
                ));
            } else {
                throw(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): Control OID {} cannot be of type array",
                        fname,
                        String::from_utf8_lossy(&oid)
                    ),
                );
                return Err(());
            }
        }
    }
    Ok(Control {
        oid,
        criticality,
        value,
    })
}

/// php_ldap_controls_from_array
pub fn controls_from_array(
    vm: &mut Vm,
    fname: &str,
    arr: &PhpArray,
    arg_num: usize,
) -> Result<Vec<Control>, ()> {
    let mut out = Vec::new();
    for (_k, v) in arr.iter() {
        let v = v.deref();
        let Value::Array(a) = &v else {
            crate::zpp::type_error(
                vm,
                fname,
                arg_num,
                "must contain only arrays, where each array is a control",
            );
            return Err(());
        };
        let a = a.borrow().clone();
        match control_from_array(vm, fname, &a) {
            Ok(c) => out.push(c),
            Err(()) => return Err(()),
        }
    }
    Ok(out)
}

/// _php_ldap_control_to_array for one control. `request` adds "iscritical".
pub fn control_to_array(ctrl: &Control, request: bool) -> PhpArray {
    let mut out = PhpArray::new();
    out.set(akey(b"oid"), sval(&ctrl.oid));
    if request {
        out.set(
            akey(b"iscritical"),
            if ctrl.criticality {
                Value::True
            } else {
                Value::False
            },
        );
    }
    let oid = ctrl.oid.as_slice();
    let empty = ctrl.value.as_ref().map(|v| v.is_empty()).unwrap_or(true);
    if oid == LDAP_CONTROL_PAGEDRESULTS.as_bytes() && !empty {
        match controls::parse_page(ctrl.value.as_ref().unwrap()) {
            Ok((size, cookie)) => {
                let mut v = PhpArray::new();
                v.set(akey(b"size"), Value::Long(size));
                v.set(akey(b"cookie"), sval(&cookie));
                out.set(akey(b"value"), Value::Array(Rc::new(RefCell::new(v))));
            }
            Err(_) => out.set(akey(b"value"), Value::Null),
        }
    } else if (oid == LDAP_CONTROL_PRE_READ.as_bytes() || oid == LDAP_CONTROL_POST_READ.as_bytes())
        && !empty
    {
        match controls::parse_readentry(ctrl.value.as_ref().unwrap()) {
            Ok((dn, attrs)) => {
                let mut v = PhpArray::new();
                v.set(akey(b"dn"), sval(&dn));
                for (name, vals) in attrs {
                    let mut va = PhpArray::new();
                    for val in vals {
                        va.push(sval(&val));
                    }
                    v.set(akey(&name), Value::Array(Rc::new(RefCell::new(va))));
                }
                out.set(akey(b"value"), Value::Array(Rc::new(RefCell::new(v))));
            }
            Err(_) => out.set(akey(b"value"), Value::Null),
        }
    } else if oid == LDAP_CONTROL_SORTRESPONSE.as_bytes() && !empty {
        match controls::parse_sortresponse(ctrl.value.as_ref().unwrap()) {
            Ok((code, attr)) => {
                let mut v = PhpArray::new();
                v.set(akey(b"errcode"), Value::Long(code));
                if let Some(a) = attr {
                    v.set(akey(b"attribute"), sval(&a));
                }
                out.set(akey(b"value"), Value::Array(Rc::new(RefCell::new(v))));
            }
            Err(_) => out.set(akey(b"value"), Value::Null),
        }
    } else if oid == LDAP_CONTROL_VLVRESPONSE.as_bytes() && !empty {
        match controls::parse_vlvresponse(ctrl.value.as_ref().unwrap()) {
            Ok((target, count, code, context)) => {
                let mut v = PhpArray::new();
                v.set(akey(b"target"), Value::Long(target));
                v.set(akey(b"count"), Value::Long(count));
                v.set(akey(b"errcode"), Value::Long(code));
                if let Some(c) = context {
                    v.set(akey(b"context"), sval(&c));
                }
                out.set(akey(b"value"), Value::Array(Rc::new(RefCell::new(v))));
            }
            Err(_) => out.set(akey(b"value"), Value::Null),
        }
    } else {
        match &ctrl.value {
            Some(v) if !v.is_empty() => out.set(akey(b"value"), sval(v)),
            _ => out.set(akey(b"value"), Value::Null),
        }
    }
    out
}

/// _php_ldap_controls_to_array into an assoc array keyed by oid.
pub fn controls_to_array(ctrls: &[Control], request: bool) -> PhpArray {
    let mut out = PhpArray::new();
    for c in ctrls {
        let a = control_to_array(c, request);
        out.set(akey(&c.oid), Value::Array(Rc::new(RefCell::new(a))));
    }
    out
}
