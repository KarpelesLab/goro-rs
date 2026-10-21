//! ext/libxml: error collection (libxml_use_internal_errors & co) and the
//! PHP-side reporting of parser errors.

use crate::parser::XmlError;
use goro_core::array::PhpArray;
use goro_core::object::{ClassEntry, PhpObject, PropertyDef, Visibility};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct State {
    internal: bool,
    errors: Vec<XmlError>,
    last: Option<XmlError>,
    entity_loader: Option<Value>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn use_internal_errors() -> bool {
    STATE.with(|s| s.borrow().internal)
}

pub fn entity_loader() -> Option<Value> {
    STATE.with(|s| s.borrow().entity_loader.clone())
}

/// How errors are surfaced as PHP warnings when internal errors are off.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// libxml2's generic handler: each output line becomes a warning
    /// (SimpleXML, XMLReader).
    Generic,
    /// The parser-context handlers used by DOM: "msg in Entity, line: N".
    Ctx,
}

/// Record errors (last error / internal list) and emit PHP warnings for
/// them as PHP's libxml glue would. `fname` is the PHP function name used
/// as the warning prefix (e.g. "simplexml_load_string").
pub fn report(vm: &mut Vm, fname: &str, errors: &[XmlError], style: Style) {
    for e in errors {
        let internal = STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.last = Some(e.clone());
            if s.internal {
                s.errors.push(e.clone());
                true
            } else {
                false
            }
        });
        if internal || vm.current_exception.is_some() {
            continue;
        }
        match style {
            Style::Generic => {
                for chunk in &e.formatted {
                    vm.emit_warning(&format!("{}(): {}", fname, chunk));
                }
            }
            Style::Ctx => {
                let msg = e.message.trim_end_matches('\n');
                let loc = match &e.file {
                    Some(f) => format!("{} in {}, line: {}", msg, f, e.line),
                    None => format!("{} in Entity, line: {}", msg, e.line),
                };
                if e.level == crate::parser::ERR_WARNING {
                    let line = vm.current_line;
                    vm.emit_notice_at(&format!("{}(): {}", fname, loc), line);
                } else {
                    vm.emit_warning(&format!("{}(): {}", fname, loc));
                }
            }
        }
    }
}

/// php_libxml_issue_error / a "pretend" error raised by PHP glue code.
pub fn issue_error(vm: &mut Vm, fname: &str, msg: &str) {
    let e = XmlError::simple(0, 1, 2, msg.to_string());
    let internal = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.last = Some(e.clone());
        if s.internal {
            s.errors.push(e.clone());
            true
        } else {
            false
        }
    });
    if !internal {
        vm.emit_warning(&format!("{}(): {}", fname, msg));
    }
}

fn error_object(vm: &mut Vm, e: &XmlError) -> Value {
    let id = vm.alloc_object_id();
    let mut o = PhpObject::new(b"LibXMLError".to_vec(), id);
    o.set_property(b"level".to_vec(), Value::Long(e.level as i64));
    o.set_property(b"code".to_vec(), Value::Long(e.code as i64));
    o.set_property(b"column".to_vec(), Value::Long(e.col as i64));
    o.set_property(
        b"message".to_vec(),
        Value::String(PhpString::from_string(e.message.clone())),
    );
    o.set_property(
        b"file".to_vec(),
        Value::String(PhpString::from_string(e.file.clone().unwrap_or_default())),
    );
    o.set_property(b"line".to_vec(), Value::Long(e.line as i64));
    Value::Object(Rc::new(RefCell::new(o)))
}

fn libxml_use_internal_errors(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let prev = use_internal_errors();
    if let Some(v) = args.first() {
        if !matches!(v, Value::Null | Value::Undef) {
            let on = v.is_truthy();
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                if !on {
                    s.errors.clear();
                }
                s.internal = on;
            });
        }
    }
    Ok(if prev { Value::True } else { Value::False })
}

fn libxml_get_errors(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let errs = STATE.with(|s| {
        let s = s.borrow();
        if s.internal {
            s.errors.clone()
        } else {
            Vec::new()
        }
    });
    let mut arr = PhpArray::new();
    for e in &errs {
        let o = error_object(vm, e);
        arr.push(o);
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn libxml_get_last_error(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let e = STATE.with(|s| {
        let s = s.borrow();
        if s.internal {
            s.errors.last().cloned()
        } else {
            s.last.clone()
        }
    });
    match e {
        Some(e) => Ok(error_object(vm, &e)),
        None => Ok(Value::False),
    }
}

fn libxml_clear_errors(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.errors.clear();
        s.last = None;
    });
    Ok(Value::Null)
}

fn libxml_set_streams_context(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Null)
}

fn libxml_disable_entity_loader(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.emit_deprecated("Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default");
    Ok(Value::False)
}

fn libxml_set_external_entity_loader(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let v = args.first().cloned().unwrap_or(Value::Null);
    STATE.with(|s| {
        s.borrow_mut().entity_loader = if matches!(v, Value::Null | Value::Undef) {
            None
        } else {
            Some(v)
        };
    });
    Ok(Value::True)
}

fn libxml_get_external_entity_loader(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(entity_loader().unwrap_or(Value::Null))
}

fn prop(name: &str, default: Value) -> PropertyDef {
    PropertyDef {
        name: name.as_bytes().to_vec(),
        default,
        is_static: false,
        is_readonly: false,
        visibility: Visibility::Public,
        set_visibility: None,
        declaring_class: b"libxmlerror".to_vec(),
        property_type: None,
        has_get_hook: false,
        has_set_hook: false,
        is_virtual: false,
        is_abstract: false,
        is_final: false,
        doc_comment: None,
        attributes: Vec::new(),
    }
}

pub fn register(vm: &mut Vm) {
    let c = |vm: &mut Vm, n: &str, v: i64| {
        vm.constants.insert(n.as_bytes().to_vec(), Value::Long(v));
    };
    c(vm, "LIBXML_VERSION", 21308);
    vm.constants.insert(
        b"LIBXML_DOTTED_VERSION".to_vec(),
        Value::String(PhpString::from_bytes(b"2.13.8")),
    );
    vm.constants.insert(
        b"LIBXML_LOADED_VERSION".to_vec(),
        Value::String(PhpString::from_bytes(b"21308")),
    );
    c(vm, "LIBXML_NOENT", 2);
    c(vm, "LIBXML_DTDLOAD", 4);
    c(vm, "LIBXML_DTDATTR", 8);
    c(vm, "LIBXML_DTDVALID", 16);
    c(vm, "LIBXML_NOERROR", 32);
    c(vm, "LIBXML_NOWARNING", 64);
    c(vm, "LIBXML_NOBLANKS", 256);
    c(vm, "LIBXML_XINCLUDE", 1024);
    c(vm, "LIBXML_NSCLEAN", 8192);
    c(vm, "LIBXML_NOCDATA", 16384);
    c(vm, "LIBXML_NONET", 2048);
    c(vm, "LIBXML_PEDANTIC", 128);
    c(vm, "LIBXML_COMPACT", 65536);
    c(vm, "LIBXML_NOXMLDECL", 2);
    c(vm, "LIBXML_PARSEHUGE", 524288);
    c(vm, "LIBXML_BIGLINES", 4194304);
    c(vm, "LIBXML_NOEMPTYTAG", 4);
    c(vm, "LIBXML_SCHEMA_CREATE", 1);
    c(vm, "LIBXML_HTML_NOIMPLIED", 8192);
    c(vm, "LIBXML_HTML_NODEFDTD", 4);
    c(vm, "LIBXML_ERR_NONE", 0);
    c(vm, "LIBXML_ERR_WARNING", 1);
    c(vm, "LIBXML_ERR_ERROR", 2);
    c(vm, "LIBXML_ERR_FATAL", 3);
    c(vm, "LIBXML_RECOVER", 1);
    c(vm, "LIBXML_NO_XXE", 8388608);

    vm.register_function(b"libxml_use_internal_errors", libxml_use_internal_errors);
    vm.register_function(b"libxml_get_errors", libxml_get_errors);
    vm.register_function(b"libxml_clear_errors", libxml_clear_errors);
    vm.register_function(b"libxml_get_last_error", libxml_get_last_error);
    vm.register_function(b"libxml_set_streams_context", libxml_set_streams_context);
    vm.register_function(
        b"libxml_disable_entity_loader",
        libxml_disable_entity_loader,
    );
    vm.register_function(
        b"libxml_set_external_entity_loader",
        libxml_set_external_entity_loader,
    );
    vm.register_function(
        b"libxml_get_external_entity_loader",
        libxml_get_external_entity_loader,
    );
    vm.register_extension(b"libxml");

    let mut ce = ClassEntry::new(b"LibXMLError".to_vec());
    for (n, d) in [
        ("level", Value::Long(0)),
        ("code", Value::Long(0)),
        ("column", Value::Long(0)),
        ("message", Value::String(PhpString::empty())),
        ("file", Value::String(PhpString::empty())),
        ("line", Value::Long(0)),
    ] {
        ce.properties.push(prop(n, d));
    }
    vm.classes.insert(b"libxmlerror".to_vec(), ce);
}
