//! CURLFile and CURLStringFile (ext/curl/curl_file.c)

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::object::{ClassEntry, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, Vm};

use crate::zpp;

fn register_class(vm: &mut Vm, name: &[u8]) {
    let ce = match goro_core::stubs::class_entry(vm, name) {
        Some(stub) => {
            let mut ce = stub.clone();
            // the methods are native
            ce.methods.clear();
            ce
        }
        None => ClassEntry::new(name.to_vec()),
    };
    let lower = name.to_ascii_lowercase();
    vm.classes.insert(lower.clone(), ce);
    vm.native_class_handlers.insert(
        lower,
        NativeClassHandler {
            has_method,
            call: call_method,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: Some(param_names),
        },
    );
}

pub fn register(vm: &mut Vm) {
    register_class(vm, b"CURLFile");
    register_class(vm, b"CURLStringFile");
}

/// The declaring class of `method` and its canonical name.
fn canon(class: &[u8], method: &[u8]) -> Option<(&'static str, &'static str)> {
    let m = method.to_ascii_lowercase();
    let file = class.eq_ignore_ascii_case(b"curlfile");
    Some(match (file, m.as_slice()) {
        (true, b"__construct") => ("CURLFile", "__construct"),
        (true, b"getfilename") => ("CURLFile", "getFilename"),
        (true, b"getmimetype") => ("CURLFile", "getMimeType"),
        (true, b"getpostfilename") => ("CURLFile", "getPostFilename"),
        (true, b"setmimetype") => ("CURLFile", "setMimeType"),
        (true, b"setpostfilename") => ("CURLFile", "setPostFilename"),
        (false, b"__construct") => ("CURLStringFile", "__construct"),
        _ => return None,
    })
}

fn has_method(class: &[u8], method: &[u8], _is_static: bool) -> bool {
    canon(class, method).is_some()
}

fn param_names(class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    let (c, m) = canon(class, method)?;
    Some(match (c, m) {
        ("CURLFile", "__construct") => &["filename", "mime_type", "posted_filename"],
        ("CURLFile", "setMimeType") => &["mime_type"],
        ("CURLFile", "setPostFilename") => &["posted_filename"],
        ("CURLStringFile", "__construct") => &["data", "postname", "mime"],
        _ => &[],
    })
}

fn set_str(o: &Rc<RefCell<PhpObject>>, name: &[u8], v: &[u8]) {
    o.borrow_mut()
        .set_property(name.to_vec(), Value::String(PhpString::from_bytes(v)));
}

/// curlfile_ctor()
fn curlfile_ctor(vm: &mut Vm, f: &str, o: &Rc<RefCell<PhpObject>>, args: &[Value]) {
    let a = match zpp::parse(vm, f, "p|s!s!", args) {
        Ok(a) => a,
        Err(()) => return,
    };
    set_str(o, b"name", a[0].str());
    if a[1].is_given() {
        set_str(o, b"mime", a[1].str());
    }
    if a[2].is_given() {
        set_str(o, b"postname", a[2].str());
    }
}

fn call_method(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let Some((c, m)) = canon(class, method) else {
        return Value::Null;
    };
    let fname = format!("{}::{}", c, m);
    let Some(Value::Object(o)) = this.map(|t| t.deref()) else {
        zpp::throw(
            vm,
            b"Error",
            &format!("Non-static method {}() cannot be called statically", fname),
        );
        return Value::Null;
    };
    match (c, m) {
        ("CURLFile", "__construct") => curlfile_ctor(vm, &fname, &o, args),
        ("CURLFile", "getFilename" | "getMimeType" | "getPostFilename") => {
            if zpp::parse(vm, &fname, "", args).is_err() {
                return Value::Null;
            }
            let p: &[u8] = match m {
                "getFilename" => b"name",
                "getMimeType" => b"mime",
                _ => b"postname",
            };
            return o.borrow().get_property(p).deref();
        }
        ("CURLFile", "setMimeType" | "setPostFilename") => {
            let a = match zpp::parse(vm, &fname, "s", args) {
                Ok(a) => a,
                Err(()) => return Value::Null,
            };
            let p: &[u8] = if m == "setMimeType" {
                b"mime"
            } else {
                b"postname"
            };
            set_str(&o, p, a[0].str());
        }
        ("CURLStringFile", "__construct") => {
            let a = match zpp::parse(vm, &fname, "ss|s", args) {
                Ok(a) => a,
                Err(()) => return Value::Null,
            };
            set_str(&o, b"data", a[0].str());
            set_str(&o, b"postname", a[1].str());
            if a[2].is_given() {
                set_str(&o, b"mime", a[2].str());
            } else {
                set_str(&o, b"mime", b"application/octet-stream");
            }
        }
        _ => {}
    }
    Value::Null
}

pub fn curl_file_create(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(b"CURLFile".to_vec(), id);
    if let Some(ce) = vm.classes.get(b"curlfile".as_slice()) {
        for p in &ce.properties {
            if !p.is_static {
                obj.set_property(p.name.clone(), p.default.clone());
            }
        }
    }
    let o = Rc::new(RefCell::new(obj));
    curlfile_ctor(vm, f, &o, args);
    if vm.current_exception.is_some() {
        return Value::Null;
    }
    Value::Object(o)
}
