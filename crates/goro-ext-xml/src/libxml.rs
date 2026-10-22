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
    /// Text of messages not ended by a newline yet (PHP's error_buffer:
    /// its handlers only emit complete lines).
    pending: String,
    entity_loader: Option<Value>,
    streams_context: Option<Value>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn use_internal_errors() -> bool {
    STATE.with(|s| s.borrow().internal)
}

/// php_libxml_external_entity_loader with a user callback installed:
/// None when no callback is set (use the default loader), Some(None) when
/// loading failed (already reported), Some(Some(data)) on success.
/// `ctx` holds the parser context's directory, intSubName, extSubURI and
/// extSubSystem.
pub fn user_entity_load(
    vm: &mut Vm,
    public: Option<&str>,
    system: Option<&str>,
    ctx: [Option<String>; 4],
) -> Option<Option<Vec<u8>>> {
    let cb = entity_loader()?;
    let s = |v: Option<&str>| match v {
        Some(x) => Value::String(PhpString::from_string(x.to_string())),
        None => Value::Null,
    };
    let mut arr = PhpArray::new();
    for (k, v) in ["directory", "intSubName", "extSubURI", "extSubSystem"]
        .iter()
        .zip(ctx.iter())
    {
        arr.set(
            goro_core::array::ArrayKey::String(PhpString::from_bytes(k.as_bytes())),
            s(v.as_deref()),
        );
    }
    let params = [
        s(public),
        s(system),
        Value::Array(Rc::new(RefCell::new(arr))),
    ];
    let r = vm.call_callback(&cb, &params);
    let fname = crate::dom::active_fn(vm);
    if vm.current_exception.is_some() || vm.exit_requested {
        return Some(None);
    }
    let rv = match r {
        Ok(v) => v.deref(),
        Err(_) => return Some(None),
    };
    let resource: Option<String> = match &rv {
        Value::String(st) => Some(st.to_string_lossy()),
        Value::Resource(_) => {
            let data = vm
                .functions
                .get(&b"stream_get_contents"[..])
                .copied()
                .and_then(|f| f(vm, std::slice::from_ref(&rv)).ok());
            if let Some(Value::String(d)) = data {
                return Some(Some(d.as_bytes().to_vec()));
            }
            None
        }
        Value::Null | Value::Undef => None,
        other => {
            let st = vm.value_to_string(other);
            if vm.current_exception.is_some() {
                return Some(None);
            }
            Some(st.to_string_lossy())
        }
    };
    match resource {
        None => {
            let msg = match public {
                None => {
                    "Failed to load external entity because the resolver function returned null"
                        .to_string()
                }
                Some(id) => format!("Failed to load external entity \"{}\"", id),
            };
            issue_error(vm, &fname, &msg);
            Some(None)
        }
        Some(path) => match std::fs::read(crate::io::strip_file_scheme(&path)) {
            Ok(d) => Some(Some(d)),
            Err(_) => {
                let msg = format!(
                    "I/O warning : failed to load \"{}\": No such file or directory",
                    path
                );
                issue_error(vm, &fname, &msg);
                Some(None)
            }
        },
    }
}

/// The stream context set with libxml_set_streams_context().
pub fn streams_context() -> Option<Value> {
    STATE.with(|s| s.borrow().streams_context.clone())
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
    report_ex(vm, fname, errors, style, &|_| false)
}

/// `report`, with errors for which `silenced` is true only recorded
/// (libxml_get_errors) and never emitted as warnings.
pub fn report_ex(
    vm: &mut Vm,
    fname: &str,
    errors: &[XmlError],
    style: Style,
    silenced: &dyn Fn(&XmlError) -> bool,
) {
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
        if internal || vm.current_exception.is_some() || vm.exit_requested || silenced(e) {
            continue;
        }
        let pending = STATE.with(|s| std::mem::take(&mut s.borrow_mut().pending));
        match style {
            Style::Generic => {
                for (i, chunk) in e.formatted.iter().enumerate() {
                    if i == 0 && !pending.is_empty() {
                        vm.emit_warning(&format!("{}(): {}{}", fname, pending, chunk));
                    } else {
                        vm.emit_warning(&format!("{}(): {}", fname, chunk));
                    }
                }
            }
            Style::Ctx => {
                if !e.message.ends_with('\n') {
                    STATE.with(|s| s.borrow_mut().pending = format!("{}{}", pending, e.message));
                    continue;
                }
                let full = format!("{}{}", pending, e.message);
                let msg = full.trim_end_matches('\n');
                if e.no_ctx {
                    vm.emit_warning(&format!("{}(): {}", fname, msg));
                    continue;
                }
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

/// php_libxml_pretend_ctx_error_ex: an error raised by PHP glue code with
/// a position (the HTML5 parser's tokenizer/tree errors).
pub fn pretend_error(vm: &mut Vm, fname: &str, file: &str, line: i64, col: i64, msg: &str) {
    let mut e = XmlError::simple(0, 1, 2, msg.to_string());
    e.file = Some(file.to_string());
    e.line = line as i32;
    e.col = col as i32;
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
    if !internal && vm.current_exception.is_none() {
        vm.emit_warning(&format!("{}(): {}", fname, msg));
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

fn libxml_set_streams_context(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let v = args.first().map(|v| v.deref()).unwrap_or(Value::Undef);
    match v {
        Value::Resource(_) if v.resource_type_name().as_deref() != Some(b"stream-context") => {
            // zend_fetch_resource_ex(..., "Stream-Context", ...)
            let e = vm.create_exception(
                b"TypeError",
                "libxml_set_streams_context(): supplied resource is not a valid Stream-Context resource",
                vm.current_line,
            );
            vm.current_exception = Some(e);
            Ok(Value::Null)
        }
        Value::Resource(_) => {
            STATE.with(|s| s.borrow_mut().streams_context = Some(v));
            Ok(Value::Null)
        }
        Value::Undef => {
            let e = vm.create_exception(
                b"ArgumentCountError",
                "libxml_set_streams_context() expects exactly 1 argument, 0 given",
                vm.current_line,
            );
            vm.current_exception = Some(e);
            Ok(Value::Null)
        }
        other => {
            let msg = format!(
                "libxml_set_streams_context(): Argument #1 ($context) must be of type resource, {} given",
                crate::dom::type_name_of(&other)
            );
            let e = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(e);
            Ok(Value::Null)
        }
    }
}

fn libxml_disable_entity_loader(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.emit_deprecated("Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default");
    Ok(Value::False)
}

fn libxml_set_external_entity_loader(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let v = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    if !matches!(v, Value::Null | Value::Undef) {
        let r = vm.zend_is_callable_ex(&v, 0);
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
        if !r.ok {
            let why = r.error.unwrap_or_default();
            let msg = format!(
                "libxml_set_external_entity_loader(): Argument #1 ($resolver_function) must be a valid callback or null, {}",
                why
            );
            let e = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(e);
            return Ok(Value::Null);
        }
    }
    let set = !matches!(v, Value::Null | Value::Undef);
    STATE.with(|s| {
        s.borrow_mut().entity_loader = if set { Some(v) } else { None };
    });
    if set {
        // External loads done by the parser go to the user callback. The
        // parser has no access to the VM, so the hook reaches it through the
        // VM that installed it (the script's VM, alive for the whole run;
        // parsing never uses its own `&mut Vm` while a load is running).
        let vmp: *mut Vm = vm;
        crate::parser::set_ext_loader(Some(Box::new(move |public, _system, url| {
            let ctx = crate::parser::LOAD_CONTEXT.with(|c| c.borrow().clone());
            // SAFETY: see above; the pointer targets the live script VM.
            let vm = unsafe { &mut *vmp };
            match user_entity_load(vm, public, Some(url), ctx) {
                Some(Some(d)) => Ok((d, None)),
                Some(None) => Err(Some((crate::parser::EXT_LOAD_REPORTED, String::new()))),
                None => crate::parser::default_ext_load(url),
            }
        })));
    } else {
        crate::parser::set_ext_loader(None);
    }
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
    crate::image_svg::register(vm);
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
