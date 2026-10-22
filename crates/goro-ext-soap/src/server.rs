//! SoapServer (soap.c): request deserialization, dispatch to the PHP
//! functions or class, and response / fault serialization.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::NativeData;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};
use indexmap::IndexMap;

use crate::common::*;
use crate::encoding::*;
use crate::php::*;
use crate::types::*;
use crate::xml::{self, Node};
use crate::zpp::{self, Arg, Func};

pub struct HeaderEntry {
    pub function: Option<FuncRef>,
    /// None: a header added with SoapServer::addSoapHeader()
    pub function_name: Option<String>,
    pub must_understand: bool,
    pub parameters: Vec<Value>,
    pub retval: Value,
    pub hdr: Option<HeaderRef>,
}

pub struct Service {
    pub sdl: Option<SdlRef>,
    pub functions_all: bool,
    /// lowercase name -> declared name
    pub ft: Option<IndexMap<String, String>>,
    pub class_ce: Option<Vec<u8>>,
    pub class_argv: Vec<Value>,
    pub persistence: i64,
    pub soap_object: Option<Value>,
    pub typemap: Option<Rc<Ht<EncRef>>>,
    pub version: i32,
    pub type_: i32,
    pub actor: Option<String>,
    pub uri: Option<String>,
    pub encoding: Option<String>,
    pub class_map: Option<Arr>,
    pub features: i64,
    pub send_errors: bool,
    pub soap_headers: Option<Rc<RefCell<Vec<HeaderEntry>>>>,
    pub trace: bool,
    pub last_response_body: Option<Vec<u8>>,
}

pub type ServiceRef = Rc<RefCell<Service>>;

static SERVER_HANDLERS: NativeObjectHandlers = NativeObjectHandlers::EMPTY;

/// soap_server_object_fetch(obj)->service
pub fn service_of(o: &Obj) -> Option<ServiceRef> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<RefCell<Service>>().ok()
}

fn set_service(o: &Obj, s: ServiceRef) {
    o.borrow_mut().native = Some(NativeData {
        data: s,
        handlers: &SERVER_HANDLERS,
    });
}

const CT_12: &[u8] = b"Content-Type: application/soap+xml; charset=utf-8";
const CT_11: &[u8] = b"Content-Type: text/xml; charset=utf-8";

fn add_header(line: &[u8]) {
    goro_core::sapi_headers::add_header(line, true);
}

fn zlib_output_compression(vm: &Vm) -> bool {
    crate::sdl::ini_bool(vm, "zlib.output_compression")
}

fn server_var(vm: &Vm, name: &str) -> Option<Value> {
    match vm.get_global(b"_SERVER").map(|v| v.deref()) {
        Some(Value::Array(a)) => a.borrow().get_str(name.as_bytes()).map(|v| v.deref()),
        _ => None,
    }
}

/// php_output_discard()
fn output_discard(vm: &mut Vm) {
    if vm.ob_level() > 0 {
        vm.ob_stack_pop(goro_core::ob::OB_POP_DISCARD | goro_core::ob::OB_POP_SILENT);
    }
}

/// soap_server_fault_ex(): write the fault response.
pub fn soap_server_fault_ex(
    vm: &mut Vm,
    function: Option<&FuncRef>,
    fault: &Obj,
    hdr: Option<&HeaderEntry>,
) {
    let soap_version = soap_version();
    let doc = match serialize_response_call(
        vm,
        function,
        None,
        None,
        &Value::Object(fault.clone()),
        hdr.map(std::slice::from_ref).unwrap_or(&[]),
        soap_version,
    ) {
        Ok(Some(d)) => d,
        _ => return,
    };
    let buf = xml::dump_doc(doc);
    xml::free_doc(doc);
    let use_http_error_status = !matches!(
        server_var(vm, "HTTP_USER_AGENT"),
        Some(Value::String(s)) if s.as_bytes() == b"Shockwave Flash"
    );
    if use_http_error_status {
        add_header(b"HTTP/1.1 500 Internal Server Error");
    }
    if zlib_output_compression(vm) {
        add_header(b"Connection: close");
    } else {
        add_header(format!("Content-Length: {}", buf.len()).as_bytes());
    }
    if soap_version == SOAP_1_2 {
        add_header(CT_12);
    } else {
        add_header(CT_11);
    }
    vm.write_output(&buf);
    vm.current_exception = None;
}

/// soap_server_fault(): write a fault and end the script.
#[allow(clippy::too_many_arguments)]
pub fn soap_server_fault(
    vm: &mut Vm,
    code: &[u8],
    string: &[u8],
    actor: Option<&[u8]>,
    details: Option<&Value>,
    name: Option<&[u8]>,
    lang: &[u8],
) -> Bail {
    let f = set_soap_fault(
        vm,
        None,
        None,
        Some(code),
        Some(string),
        actor,
        details,
        name,
        lang,
    );
    soap_server_fault_ex(vm, None, &f, None);
    vm.exit_requested = true;
    Bail
}

pub fn soap_server_fault_en(vm: &mut Vm, code: &str, string: &str) -> Bail {
    soap_server_fault(
        vm,
        code.as_bytes(),
        string.as_bytes(),
        None,
        None,
        None,
        b"en",
    )
}

// ---------------------------------------------------------------------------
// Methods

pub fn construct(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "S!|a", &[], args)?;
    let wsdl = a[0].string();
    let options = a.get(1).and_then(|x| x.arr());
    let guard = begin_code(this, "Server");
    let r = construct_inner(vm, f, this, wsdl, options);
    end_code(guard);
    r?;
    Ok(Value::Null)
}

fn construct_inner(
    vm: &mut Vm,
    f: &Func,
    this: &Obj,
    wsdl: Option<String>,
    options: Option<Arr>,
) -> SR<()> {
    let mut service = Service {
        sdl: None,
        functions_all: false,
        ft: Some(IndexMap::new()),
        class_ce: None,
        class_argv: Vec::new(),
        persistence: 0,
        soap_object: None,
        typemap: None,
        version: SOAP_1_1,
        type_: SOAP_FUNCTIONS,
        actor: None,
        uri: None,
        encoding: None,
        class_map: None,
        features: 0,
        send_errors: true,
        soap_headers: None,
        trace: false,
        last_response_body: None,
    };
    let mut cache_wsdl = if crate::sdl::ini_bool(vm, "soap.wsdl_cache_enabled") {
        crate::sdl::ini_long(vm, "soap.wsdl_cache", 1) as i8 as i64
    } else {
        0
    };
    let mut version = SOAP_1_1;
    let mut typemap_ht: Option<Arr> = None;
    match &options {
        Some(opts) => {
            let ht = opts.borrow().clone();
            if let Some(tmp) = ht.get_str(b"soap_version").map(|v| v.deref()) {
                match tmp {
                    Value::Long(v) if v == SOAP_1_1 as i64 || v == SOAP_1_2 as i64 => {
                        version = v as i32
                    }
                    _ => {
                        return Err(docref_fatal(
                            vm,
                            &f.name,
                            "'soap_version' option must be SOAP_1_1 or SOAP_1_2",
                        ));
                    }
                }
            }
            match ht.get_str(b"uri").map(|v| v.deref()) {
                Some(Value::String(s)) => service.uri = Some(s.to_string_lossy()),
                _ => {
                    if wsdl.is_none() {
                        return Err(docref_fatal(
                            vm,
                            &f.name,
                            "'uri' option is required in nonWSDL mode",
                        ));
                    }
                }
            }
            if let Some(Value::String(s)) = ht.get_str(b"actor").map(|v| v.deref()) {
                service.actor = Some(s.to_string_lossy());
            }
            if let Some(Value::String(s)) = ht.get_str(b"encoding").map(|v| v.deref()) {
                let e = s.to_string_lossy();
                if !goro_ext_xml::parser::encoding_supported(&e) {
                    return Err(docref_fatal(
                        vm,
                        &f.name,
                        format!("Invalid 'encoding' option - '{}'", e),
                    ));
                }
                service.encoding = Some(e);
            }
            if let Some(Value::Array(cm)) = ht.get_str(b"classmap").map(|v| v.deref()) {
                if cm.borrow().looks_packed() && !cm.borrow().is_empty() {
                    return Err(docref_fatal(
                        vm,
                        &f.name,
                        "'classmap' option must be an associative array",
                    ));
                }
                let c = cm.borrow().clone();
                service.class_map = Some(Rc::new(RefCell::new(c)));
            }
            if let Some(Value::Array(tm)) = ht.get_str(b"typemap").map(|v| v.deref()) {
                if !tm.borrow().is_empty() {
                    typemap_ht = Some(tm);
                }
            }
            if let Some(Value::Long(n)) = ht.get_str(b"features").map(|v| v.deref()) {
                service.features = n;
            }
            if let Some(Value::Long(n)) = ht.get_str(b"cache_wsdl").map(|v| v.deref()) {
                cache_wsdl = n;
            }
            if let Some(tmp) = ht.get_str(b"send_errors").map(|v| v.deref()) {
                match tmp {
                    Value::False => service.send_errors = false,
                    Value::True => service.send_errors = true,
                    Value::Long(n) => service.send_errors = n != 0,
                    _ => {}
                }
            }
            if let Some(tmp) = ht.get_str(b"trace").map(|v| v.deref()) {
                if matches!(tmp, Value::True | Value::Long(1)) {
                    service.trace = true;
                }
            }
        }
        None => {
            if wsdl.is_none() {
                return Err(docref_fatal(
                    vm,
                    &f.name,
                    "'uri' option is required in nonWSDL mode",
                ));
            }
        }
    }
    service.version = version;
    if let Some(w) = &wsdl {
        crate::sdl::set_cur_fname(&f.name);
        let sdl = crate::sdl::get_sdl(vm, this, w, cache_wsdl)?;
        if service.uri.is_none() {
            service.uri = Some(
                sdl.borrow()
                    .target_ns
                    .clone()
                    .unwrap_or_else(|| "http://unknown-uri/".to_string()),
            );
        }
        service.sdl = Some(sdl);
    }
    if let Some(tm) = typemap_ht {
        service.typemap = crate::client::soap_create_typemap(vm, service.sdl.as_ref(), &tm)?;
    }
    set_service(this, Rc::new(RefCell::new(service)));
    Ok(())
}

fn fetch_service(vm: &mut Vm, this: &Obj) -> SR<ServiceRef> {
    match service_of(this) {
        Some(s) => Ok(s),
        None => Err(zpp::throw(vm, b"Error", "Cannot fetch SoapServer object")),
    }
}

pub fn set_persistence(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "l", &[], args)?;
    let value = a[0].long().unwrap_or(0);
    let service = fetch_service(vm, this)?;
    let ty = service.borrow().type_;
    if ty == SOAP_CLASS {
        if value == SOAP_PERSISTENCE_SESSION || value == SOAP_PERSISTENCE_REQUEST {
            if value == SOAP_PERSISTENCE_SESSION
                && !vm.loaded_extensions.contains(b"session".as_slice())
            {
                return Err(zpp::throw(
                    vm,
                    b"Error",
                    "SoapServer::setPersistence(): Session persistence cannot be enabled because the session module is not enabled",
                ));
            }
            service.borrow_mut().persistence = value;
        } else {
            return Err(f.value_error(
                vm,
                1,
                "must be either SOAP_PERSISTENCE_SESSION or SOAP_PERSISTENCE_REQUEST when the SOAP server is used in class mode",
            ));
        }
    } else {
        return Err(zpp::throw(
            vm,
            b"Error",
            "SoapServer::setPersistence(): Persistence cannot be set when the SOAP server is used in function mode",
        ));
    }
    Ok(Value::Null)
}

pub fn set_class(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "C*", &[], args)?;
    let Arg::Class(ce) = &a[0] else {
        return Ok(Value::Null);
    };
    let argv = match &a[1] {
        Arg::Rest(r) => r.iter().map(|v| copy_value(&v.deref())).collect(),
        _ => Vec::new(),
    };
    let service = fetch_service(vm, this)?;
    let mut s = service.borrow_mut();
    s.type_ = SOAP_CLASS;
    s.class_ce = Some(ce.clone());
    s.persistence = SOAP_PERSISTENCE_REQUEST;
    s.class_argv = argv;
    Ok(Value::Null)
}

pub fn set_object(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "o", &[], args)?;
    let o = a[0].obj().unwrap();
    let service = fetch_service(vm, this)?;
    let mut s = service.borrow_mut();
    s.type_ = SOAP_OBJECT;
    s.soap_object = Some(Value::Object(o));
    Ok(Value::Null)
}

/// The methods of a class (function_table order), with their visibility.
fn class_methods(vm: &Vm, class: &[u8]) -> Vec<(Vec<u8>, bool)> {
    let mut out: Vec<(Vec<u8>, bool)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cur = Some(lower(class));
    let mut guard = 0;
    while let Some(c) = cur {
        guard += 1;
        if guard > 64 {
            break;
        }
        let Some(ce) = vm.classes.get(&c) else { break };
        for (k, m) in ce.methods.iter() {
            if seen.insert(k.clone()) {
                out.push((
                    m.name.clone(),
                    m.visibility == goro_core::object::Visibility::Public,
                ));
            }
        }
        cur = ce.parent.as_ref().map(|p| lower(p));
    }
    out
}

fn obj_class(v: &Value) -> Vec<u8> {
    match v.deref() {
        Value::Object(o) => o.borrow().class_name.clone(),
        _ => Vec::new(),
    }
}

pub fn get_functions(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    zpp::parse(vm, f, "", &[], args)?;
    let service = fetch_service(vm, this)?;
    let s = service.borrow();
    let mut out = PhpArray::new();
    let class = if s.type_ == SOAP_OBJECT {
        s.soap_object.as_ref().map(obj_class)
    } else if s.type_ == SOAP_CLASS {
        s.class_ce.clone()
    } else {
        None
    };
    if let Some(c) = class {
        for (name, public) in class_methods(vm, &c) {
            if public {
                out.push(vstr(name));
            }
        }
    } else if s.functions_all {
        let mut names: Vec<Vec<u8>> = vm.functions.keys().cloned().collect();
        names.sort();
        for n in names {
            out.push(vstr(n));
        }
        for (k, op) in vm.user_functions.iter() {
            let _ = k;
            out.push(vstr(op.name.clone()));
        }
    } else if let Some(ft) = &s.ft {
        for v in ft.values() {
            out.push(vstr(v));
        }
    }
    Ok(varr(out))
}

/// The declared name of a function (zend_hash_find in EG(function_table)).
fn find_function_name(vm: &Vm, name: &[u8]) -> Option<String> {
    let l = lower(name.strip_prefix(b"\\").unwrap_or(name));
    if let Some(op) = vm.user_functions.get(&l) {
        return Some(String::from_utf8_lossy(&op.name).into_owned());
    }
    if vm.functions.contains_key(&l) {
        let n = goro_core::stubs::find_function(&l)
            .map(|f| f.name.to_string())
            .unwrap_or_else(|| String::from_utf8_lossy(&l).into_owned());
        return Some(n);
    }
    None
}

pub fn add_function(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "z", &[], args)?;
    let fname = a[0].any().unwrap_or(Value::Null).deref();
    let service = fetch_service(vm, this)?;
    match &fname {
        Value::Array(arr) => {
            let ty = service.borrow().type_;
            if ty == SOAP_FUNCTIONS {
                let items: Vec<Value> = arr.borrow().values().map(|v| v.deref()).collect();
                if service.borrow().ft.is_none() {
                    let mut s = service.borrow_mut();
                    s.functions_all = false;
                    s.ft = Some(IndexMap::new());
                }
                for tmp in items {
                    let Value::String(s) = &tmp else {
                        return Err(f.arg_type_error(vm, 1, "must contain only strings"));
                    };
                    let key = s.to_string_lossy().to_ascii_lowercase();
                    match find_function_name(vm, s.as_bytes()) {
                        Some(n) => {
                            service.borrow_mut().ft.as_mut().unwrap().insert(key, n);
                        }
                        None => {
                            let m = format!(
                                "SoapServer::addFunction(): Function \"{}\" not found",
                                s.to_string_lossy()
                            );
                            return Err(zpp::throw(vm, b"TypeError", &m));
                        }
                    }
                }
            }
        }
        Value::String(s) => {
            let key = s.to_string_lossy().to_ascii_lowercase();
            match find_function_name(vm, s.as_bytes()) {
                Some(n) => {
                    let mut sv = service.borrow_mut();
                    if sv.ft.is_none() {
                        sv.functions_all = false;
                        sv.ft = Some(IndexMap::new());
                    }
                    sv.ft.as_mut().unwrap().insert(key, n);
                }
                None => {
                    let m = format!(
                        "must be a valid function name, function \"{}\" not found",
                        s.to_string_lossy()
                    );
                    return Err(f.arg_type_error(vm, 1, &m));
                }
            }
        }
        Value::Long(n) => {
            if *n == SOAP_FUNCTIONS_ALL {
                docref_deprecated(
                    vm,
                    &f.name,
                    "Enabling all functions via SOAP_FUNCTIONS_ALL is deprecated since 8.4, due to possible security concerns. If all PHP functions should be enabled, the flattened return value of get_defined_functions() can be used",
                );
                if vm.current_exception.is_some() {
                    return Err(Bail);
                }
                let mut sv = service.borrow_mut();
                sv.ft = None;
                sv.functions_all = true;
            } else {
                return Err(f.value_error(
                    vm,
                    1,
                    "must be SOAP_FUNCTIONS_ALL when an integer is passed",
                ));
            }
        }
        other => {
            let m = format!(
                "must be of type array|string|int, {} given",
                Vm::value_type_name(other)
            );
            return Err(f.arg_type_error(vm, 1, &m));
        }
    }
    Ok(Value::Null)
}

/// _soap_server_exception()
fn server_exception(vm: &mut Vm, service: &ServiceRef, function: Option<&FuncRef>, this: &Obj) {
    let Some(exc) = vm.current_exception.clone() else {
        return;
    };
    let Value::Object(eo) = &exc else { return };
    if instance_of(vm, eo, "SoapFault") {
        let eo = eo.clone();
        soap_server_fault_ex(vm, function, &eo, None);
    } else if instance_of(vm, eo, "Error") {
        let send = service.borrow().send_errors;
        let msg = if send {
            let m = prop(eo, "message");
            zval_get_string(vm, &m).unwrap_or_default()
        } else {
            b"Internal Error".to_vec()
        };
        let fault = add_soap_fault_ex(vm, this, "Server", Some(&msg), None, None, b"en");
        soap_server_fault_ex(vm, function, &fault, None);
    }
}

fn is_unwind_exit(vm: &Vm) -> bool {
    vm.exit_requested
}

pub fn handle(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "|s!", &[], args)?;
    let arg = a.first().and_then(|x| x.str().map(|s| s.to_vec()));
    let guard = begin_code(this, "Server");
    let service = match service_of(this) {
        Some(s) => s,
        None => {
            end_code(guard);
            return Err(zpp::throw(vm, b"Error", "Cannot fetch SoapServer object"));
        }
    };
    let ver = service.borrow().version;
    g(|g| g.soap_version = ver);
    let r = handle_inner(vm, &service, this, arg);
    end_code(guard);
    r?;
    Ok(Value::Null)
}

fn handle_inner(vm: &mut Vm, service: &ServiceRef, this: &Obj, arg: Option<Vec<u8>>) -> SR<()> {
    let req = goro_ext_standard::request::request_info();
    let method = req.as_ref().and_then(|r| r.request_method.clone());
    let query = req
        .as_ref()
        .map(|r| r.query_string.clone())
        .unwrap_or_default();
    if method.as_deref() == Some(b"GET") && query.eq_ignore_ascii_case(b"wsdl") {
        let sdl = service.borrow().sdl.clone();
        match sdl {
            Some(sdl) => {
                add_header(CT_11);
                let source = sdl.borrow().source.clone();
                match xml::read_file(vm, &source) {
                    Some(data) => vm.write_output(&data),
                    None => {
                        let fname = "SoapServer::handle";
                        docref_warning(
                            vm,
                            fname,
                            "Failed to open stream: No such file or directory",
                        );
                        return Err(soap_server_fault_en(vm, "Server", "Couldn't find WSDL"));
                    }
                }
                return Ok(());
            }
            None => {
                return Err(soap_server_fault_en(
                    vm,
                    "Server",
                    "WSDL generation is not supported yet",
                ));
            }
        }
    }
    // php_output_start_default()
    let h = Vm::ob_make_handler(
        b"default output handler".to_vec(),
        goro_core::ob::ObKind::Default,
        0,
        goro_core::ob::OBH_STDFLAGS,
    );
    vm.ob_handler_start(Some(h));
    let mut soap_action: Option<String> = None;
    let doc_request = match arg {
        Some(a) => xml::parse_memory(&a),
        None => {
            let has_body = req
                .as_ref()
                .is_some_and(|r| r.cgi && r.request_method.is_some());
            if !has_body {
                return Ok(());
            }
            if let Some(Value::String(enc)) = server_var(vm, "HTTP_CONTENT_ENCODING") {
                let e = enc.as_bytes().to_vec();
                if e == b"gzip" || e == b"x-gzip" || e == b"deflate" {
                    // decompressed below
                } else {
                    docref_warning(
                        vm,
                        "SoapServer::handle",
                        format!(
                            "Request is compressed with unknown compression '{}'",
                            String::from_utf8_lossy(&e)
                        ),
                    );
                    return Ok(());
                }
            }
            if let Some(Value::String(sa)) = server_var(vm, "HTTP_SOAPACTION") {
                soap_action = Some(sa.to_string_lossy());
            }
            let mut body = req.as_ref().map(|r| r.body.clone()).unwrap_or_default();
            if let Some(Value::String(enc)) = server_var(vm, "HTTP_CONTENT_ENCODING") {
                if let Some(d) = crate::http::inflate_any(vm, &body, enc.as_bytes()) {
                    body = d;
                }
            }
            match xml::parse_memory(&body) {
                Some(d) => {
                    let _ = &d;
                    Some(d)
                }
                None => None,
            }
        }
    };
    let Some(doc_request) = doc_request else {
        return Err(soap_server_fault_en(vm, "Client", "Bad Request"));
    };
    if xml::has_int_subset(doc_request) {
        let env = xml::get_node(xml::children(doc_request), "Envelope");
        if let Some(env) = env {
            if let Some(h) = xml::ns_href_of(env) {
                if h == SOAP_1_1_ENV_NAMESPACE {
                    g(|g| g.soap_version = SOAP_1_1);
                } else if h == SOAP_1_2_ENV_NAMESPACE {
                    g(|g| g.soap_version = SOAP_1_2);
                }
            }
        }
        xml::free_doc(doc_request);
        return Err(soap_server_fault_en(
            vm,
            "Server",
            "DTD are not supported by SOAP",
        ));
    }
    let (sdl, encoding, class_map, typemap, features, actor) = {
        let s = service.borrow();
        (
            s.sdl.clone(),
            s.encoding.clone(),
            s.class_map.clone(),
            s.typemap.clone(),
            s.features,
            s.actor.clone(),
        )
    };
    let old = g(|g| {
        let old = (
            g.sdl.clone(),
            g.encoding.clone(),
            g.class_map.clone(),
            g.typemap.clone(),
            g.features,
            g.soap_version,
        );
        g.sdl = sdl.clone();
        g.encoding = encoding;
        g.class_map = class_map;
        g.typemap = typemap;
        g.features = features;
        old
    });
    let r = handle_request(
        vm,
        service,
        this,
        doc_request,
        sdl.as_ref(),
        actor.as_deref(),
        soap_action.as_deref(),
    );
    g(|g| {
        g.sdl = old.0;
        g.encoding = old.1;
        g.class_map = old.2;
        g.typemap = old.3;
        g.features = old.4;
        g.soap_version = old.5;
    });
    service.borrow_mut().soap_headers = None;
    r
}

#[allow(clippy::too_many_arguments)]
fn handle_request(
    vm: &mut Vm,
    service: &ServiceRef,
    this: &Obj,
    doc_request: Node,
    sdl: Option<&SdlRef>,
    actor: Option<&str>,
    soap_action: Option<&str>,
) -> SR<()> {
    let dr = deserialize_function_call(vm, sdl, doc_request, actor, soap_action);
    xml::free_doc(doc_request);
    let (function, function_name, params, soap_version, headers) = match dr {
        Ok(d) => d,
        Err(_) if vm.current_exception.is_some() && !is_unwind_exit(vm) => {
            output_discard(vm);
            server_exception(vm, service, None, this);
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    if vm.current_exception.is_some() {
        if !is_unwind_exit(vm) {
            output_discard(vm);
            server_exception(vm, service, function.as_ref(), this);
        }
        return Ok(());
    }
    let headers = Rc::new(RefCell::new(headers));
    service.borrow_mut().soap_headers = Some(headers.clone());
    let ty = service.borrow().type_;
    let mut soap_obj: Option<Value> = None;
    if ty == SOAP_OBJECT {
        soap_obj = service.borrow().soap_object.clone();
    } else if ty == SOAP_CLASS {
        let (ce, argv, persistence) = {
            let s = service.borrow();
            (
                s.class_ce.clone().unwrap_or_default(),
                s.class_argv.clone(),
                s.persistence,
            )
        };
        if persistence == SOAP_PERSISTENCE_SESSION {
            soap_obj = session_object(vm, &ce)?;
        }
        if soap_obj.is_none() {
            let o = match new_object_ex(vm, &ce) {
                Ok(o) => o,
                Err(_) => {
                    output_discard(vm);
                    server_exception(vm, service, function.as_ref(), this);
                    return Ok(());
                }
            };
            let has_ctor = vm
                .classes
                .get(&lower(&ce))
                .is_some_and(|c| c.get_method(b"__construct").is_some());
            if has_ctor {
                let r = call_method(vm, &o, "__construct", &argv);
                if r.is_err() || vm.current_exception.is_some() {
                    if vm.exit_requested {
                        return Err(Bail);
                    }
                    output_discard(vm);
                    server_exception(vm, service, function.as_ref(), this);
                    return Ok(());
                }
            }
            let ov = Value::Object(o);
            if persistence == SOAP_PERSISTENCE_SESSION {
                set_session_object(vm, &ov);
            }
            soap_obj = Some(ov);
        }
    }
    let callable = |name: &str, obj: &Option<Value>| -> Value {
        match obj {
            Some(o) => {
                let mut a = PhpArray::new();
                a.push(o.clone());
                a.push(vstr(name));
                varr(a)
            }
            None => vstr(name),
        }
    };
    let has_fn = |vm: &Vm, name: &str| -> bool {
        let s = service.borrow();
        if s.type_ == SOAP_CLASS || s.type_ == SOAP_OBJECT {
            let class = soap_obj.as_ref().map(obj_class).unwrap_or_default();
            let cl = lower(&class);
            let Some(ce) = vm.classes.get(&cl) else {
                return false;
            };
            let _ = ce;
            class_has_method(vm, &cl, &name.to_ascii_lowercase())
                || class_has_method(vm, &cl, "__call")
        } else if s.functions_all {
            find_function_name(vm, name.as_bytes()).is_some()
        } else {
            s.ft.as_ref()
                .is_some_and(|ft| ft.contains_key(&name.to_ascii_lowercase()))
        }
    };
    // Process the SOAP headers
    let n_headers = headers.borrow().len();
    for i in 0..n_headers {
        let (hname, hparams, hmu) = {
            let h = &headers.borrow()[i];
            (
                h.function_name.clone(),
                h.parameters.clone(),
                h.must_understand,
            )
        };
        let Some(hname) = hname else { continue };
        if has_fn(vm, &hname) {
            let c = callable(&hname, &soap_obj);
            let rv = call_user(vm, &c, &hparams);
            let rv = match rv {
                Ok(v) => v,
                Err(_) if vm.current_exception.is_some() && !vm.exit_requested => Value::Null,
                Err(e) => return Err(e),
            };
            headers.borrow_mut()[i].retval = rv.clone();
            if val_instance_of(vm, &rv, "SoapFault") {
                output_discard(vm);
                let fo = is_obj(&rv).unwrap();
                let hdr = headers.borrow_mut().remove(i);
                soap_server_fault_ex(vm, function.as_ref(), &fo, Some(&hdr));
                return Ok(());
            } else if vm.current_exception.is_some() {
                output_discard(vm);
                server_exception(vm, service, function.as_ref(), this);
                return Ok(());
            }
        } else if hmu {
            return Err(soap_server_fault_en(
                vm,
                "MustUnderstand",
                "Header not understood",
            ));
        }
    }
    let mut retval = Value::Null;
    if has_fn(vm, &function_name) {
        let c = callable(&function_name, &soap_obj);
        match call_user(vm, &c, &params) {
            Ok(v) => retval = v,
            Err(_) if vm.current_exception.is_some() && !vm.exit_requested => {}
            Err(e) => return Err(e),
        }
    } else {
        return Err(raise_fatal(
            vm,
            &format!("Function '{}' doesn't exist", function_name),
        ));
    }
    if vm.current_exception.is_some() {
        if !is_unwind_exit(vm) {
            output_discard(vm);
            server_exception(vm, service, function.as_ref(), this);
        }
        return Ok(());
    }
    if val_instance_of(vm, &retval, "SoapFault") {
        output_discard(vm);
        let fo = is_obj(&retval).unwrap();
        soap_server_fault_ex(vm, function.as_ref(), &fo, None);
        return Ok(());
    }
    let response_name = match function.as_ref().and_then(|f| f.response_name.clone()) {
        Some(n) => n,
        None => format!("{}Response", function_name),
    };
    let uri = service.borrow().uri.clone();
    let hdrs = std::mem::take(&mut *headers.borrow_mut());
    let doc_return = serialize_response_call(
        vm,
        function.as_ref(),
        Some(&response_name),
        uri.as_deref(),
        &retval,
        &hdrs,
        soap_version,
    );
    let doc_return = match doc_return {
        Ok(d) => d,
        Err(e) => {
            if vm.current_exception.is_some() && !vm.exit_requested {
                output_discard(vm);
                server_exception(vm, service, function.as_ref(), this);
                return Ok(());
            }
            return Err(e);
        }
    };
    if vm.current_exception.is_some() {
        output_discard(vm);
        server_exception(vm, service, function.as_ref(), this);
        return Ok(());
    }
    output_discard(vm);
    match doc_return {
        Some(doc) => {
            let buf = xml::dump_doc(doc);
            xml::free_doc(doc);
            if soap_version == SOAP_1_2 {
                add_header(CT_12);
            } else {
                add_header(CT_11);
            }
            if zlib_output_compression(vm) {
                add_header(b"Connection: close");
            } else {
                add_header(format!("Content-Length: {}", buf.len()).as_bytes());
            }
            vm.write_output(&buf);
            if service.borrow().trace {
                service.borrow_mut().last_response_body = Some(buf);
            }
        }
        None => {
            add_header(b"HTTP/1.1 202 Accepted");
            add_header(b"Content-Length: 0");
        }
    }
    Ok(())
}

fn class_has_method(vm: &Vm, cl: &[u8], m: &str) -> bool {
    let mut cur = Some(cl.to_vec());
    let mut guard = 0;
    while let Some(c) = cur {
        guard += 1;
        if guard > 64 {
            break;
        }
        let Some(ce) = vm.classes.get(&c) else {
            return false;
        };
        if ce.methods.contains_key(m.as_bytes()) {
            return true;
        }
        cur = ce.parent.as_ref().map(|p| lower(p));
    }
    false
}

fn session_object(vm: &mut Vm, ce: &[u8]) -> SR<Option<Value>> {
    // php_session_start() when the session is not active
    if let Some(f) = vm.functions.get(b"session_status".as_slice()).copied() {
        let st = f(vm, &[]).unwrap_or(Value::Long(0));
        if st.to_long() == 1 {
            if let Some(start) = vm.functions.get(b"session_start".as_slice()).copied() {
                let _ = start(vm, &[]);
            }
        }
    }
    let sess = vm.get_global(b"_SESSION").map(|v| v.deref());
    if let Some(Value::Array(a)) = sess {
        if let Some(Value::Object(o)) = a
            .borrow()
            .get_str(b"_bogus_session_name")
            .map(|v| v.deref())
        {
            let cn = o.borrow().class_name.clone();
            if cn.eq_ignore_ascii_case(ce) {
                return Ok(Some(Value::Object(o)));
            } else if cn.eq_ignore_ascii_case(b"__PHP_Incomplete_Class") {
                return Err(soap_server_fault_en(
                    vm,
                    "Server",
                    "SoapServer class was deserialized from the session prior to loading the class passed to SoapServer::setClass(). Start the session after loading all classes to resolve this issue.",
                ));
            }
        }
    }
    Ok(None)
}

fn set_session_object(vm: &mut Vm, o: &Value) {
    if let Some(Value::Array(a)) = vm.get_global(b"_SESSION").map(|v| v.deref()) {
        a.borrow_mut().set(
            ArrayKey::String(goro_core::string::PhpString::from_bytes(
                b"_bogus_session_name",
            )),
            o.clone(),
        );
    }
}

pub fn fault(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "ss|szSP", &[], args)?;
    let code = a[0].str().unwrap_or_default().to_vec();
    let string = a[1].str().unwrap_or_default().to_vec();
    let actor = a.get(2).and_then(|x| x.str().map(|s| s.to_vec()));
    let details = a.get(3).and_then(|x| x.any());
    let name = a.get(4).and_then(|x| x.str().map(|s| s.to_vec()));
    let lang = a
        .get(5)
        .and_then(|x| x.str().map(|s| s.to_vec()))
        .unwrap_or_default();
    let guard = begin_code(this, "Server");
    let service = match service_of(this) {
        Some(s) => s,
        None => {
            end_code(guard);
            return Err(zpp::throw(vm, b"Error", "Cannot fetch SoapServer object"));
        }
    };
    let old_enc = g(|g| g.encoding.clone());
    let enc = service.borrow().encoding.clone();
    g(|g| g.encoding = enc);
    let r = soap_server_fault(
        vm,
        &code,
        &string,
        actor.as_deref(),
        details.as_ref(),
        name.as_deref(),
        &lang,
    );
    g(|g| g.encoding = old_enc);
    end_code(guard);
    Err(r)
}

pub fn add_soap_header(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "O", &["SoapHeader"], args)?;
    let h = a[0].obj().unwrap();
    let service = fetch_service(vm, this)?;
    let hp = service.borrow().soap_headers.clone();
    match hp {
        Some(list) => {
            list.borrow_mut().push(HeaderEntry {
                function: None,
                function_name: None,
                must_understand: false,
                parameters: Vec::new(),
                retval: Value::Object(h),
                hdr: None,
            });
            Ok(Value::Null)
        }
        None => Err(zpp::throw(
            vm,
            b"Error",
            "SoapServer::addSoapHeader() may be called only during SOAP request processing",
        )),
    }
}

pub fn get_last_response(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    zpp::parse(vm, f, "", &[], args)?;
    let service = fetch_service(vm, this)?;
    let r = service.borrow().last_response_body.clone();
    Ok(match r {
        Some(b) => vstr(b),
        None => Value::Null,
    })
}

// ---------------------------------------------------------------------------
// Request deserialization

fn deserialize_parameters(
    vm: &mut Vm,
    params: Option<Node>,
    function: Option<&FuncRef>,
) -> SR<Vec<Value>> {
    let mut out: Vec<Value> = Vec::new();
    if let Some(f) = function {
        let Some(rp) = &f.request_params else {
            return Ok(out);
        };
        let use_names = rp.iter().any(|p| {
            p.param_name
                .as_deref()
                .is_some_and(|n| xml::get_node(params, n).is_some())
        });
        if use_names {
            for p in rp {
                let val = p
                    .param_name
                    .as_deref()
                    .and_then(|n| xml::get_node(params, n));
                match val {
                    None => out.push(Value::Null),
                    Some(v) => out.push(master_to_zval(vm, p.encode.as_ref(), v)?),
                }
            }
            return Ok(out);
        }
    }
    let mut num_of_params = 0usize;
    if let Some(first) = params {
        let mut trav = Some(first);
        while let Some(t) = trav {
            if xml::is_element(t) {
                num_of_params += 1;
            }
            trav = xml::next(t);
        }
        let doc_style_empty = num_of_params == 1
            && function.is_some_and(|f| {
                f.soap().is_some_and(|b| b.style == SOAP_DOCUMENT)
                    && f.request_params.as_ref().is_none_or(|r| r.is_empty())
                    && xml::name(first) == f.function_name
            });
        if doc_style_empty {
            num_of_params = 0;
        } else if num_of_params > 0 {
            let mut trav = Some(first);
            while let Some(t) = trav {
                if out.len() >= num_of_params {
                    break;
                }
                if xml::is_element(t) {
                    let mut enc: Option<EncRef> = None;
                    if let Some(f) = function {
                        match f.request_params.as_ref().and_then(|r| r.get(out.len())) {
                            Some(p) => enc = p.encode.clone(),
                            None => {
                                return Err(soap_server_fault_en(
                                    vm,
                                    "Client",
                                    "Error cannot find parameter",
                                ));
                            }
                        }
                    }
                    out.push(master_to_zval(vm, enc.as_ref(), t)?);
                }
                trav = xml::next(t);
            }
        }
    }
    if num_of_params > out.len() {
        return Err(soap_server_fault_en(vm, "Client", "Missing parameter"));
    }
    Ok(out)
}

fn find_function(sdl: Option<&SdlRef>, func: Node) -> (Option<FuncRef>, String) {
    let fname = xml::name(func);
    let mut function = get_function(sdl, &fname);
    if let Some(f) = &function {
        if let Some(fnb) = f.soap() {
            if fnb.style == SOAP_DOCUMENT
                && (xml::children(func).is_some()
                    || f.request_params.as_ref().is_some_and(|r| !r.is_empty()))
            {
                function = None;
            }
        }
    }
    if sdl.is_some() && function.is_none() {
        function = get_doc_function(sdl, Some(func));
    }
    match function {
        Some(f) => {
            let n = f.function_name.clone();
            (Some(f), n)
        }
        None => (None, fname),
    }
}

fn find_function_using_soap_action(
    sdl: Option<&SdlRef>,
    soap_action: &str,
) -> Option<(FuncRef, String)> {
    let sdl = sdl?;
    let mut sa = soap_action;
    if sa.starts_with('"') {
        if sa.len() < 2 || !sa.ends_with('"') {
            return None;
        }
        sa = &sa[1..sa.len() - 1];
    }
    let s = sdl.borrow();
    for f in s.functions.values() {
        if let Some(fnb) = f.soap() {
            if fnb.soap_action.as_deref() == Some(sa) {
                return Some((f.clone(), f.function_name.clone()));
            }
        }
    }
    None
}

type Deserialized = (Option<FuncRef>, String, Vec<Value>, i32, Vec<HeaderEntry>);

fn get_envelope(vm: &mut Vm, first: Option<Node>) -> SR<Option<(Node, i32, &'static str)>> {
    let mut trav = first;
    while let Some(t) = trav {
        if xml::is_element(t) {
            if xml::node_is_equal_ex(t, Some("Envelope"), Some(SOAP_1_1_ENV_NAMESPACE)) {
                g(|g| g.soap_version = SOAP_1_1);
                return Ok(Some((t, SOAP_1_1, SOAP_1_1_ENV_NAMESPACE)));
            }
            if xml::node_is_equal_ex(t, Some("Envelope"), Some(SOAP_1_2_ENV_NAMESPACE)) {
                g(|g| g.soap_version = SOAP_1_2);
                return Ok(Some((t, SOAP_1_2, SOAP_1_2_ENV_NAMESPACE)));
            }
            return Err(soap_server_fault_en(vm, "VersionMismatch", "Wrong Version"));
        }
        trav = xml::next(t);
    }
    Ok(None)
}

fn check_attrs(vm: &mut Vm, node: Node, version: i32, what: &str, ns_required: bool) -> SR<()> {
    let mut a = xml::properties(node);
    while let Some(at) = a {
        if xml::node_ns(at).is_none() {
            if ns_required || version == SOAP_1_2 {
                let m = format!(
                    "A SOAP {} element cannot have non Namespace qualified attributes",
                    what
                );
                return Err(soap_server_fault_en(vm, "Client", &m));
            }
        } else if xml::attr_is_equal_ex(at, "encodingStyle", Some(SOAP_1_2_ENV_NAMESPACE)) {
            if version == SOAP_1_2 {
                let m = format!("encodingStyle cannot be specified on the {}", what);
                return Err(soap_server_fault_en(vm, "Client", &m));
            } else if xml::attr_value(at) != SOAP_1_1_ENC_NAMESPACE {
                return Err(soap_server_fault_en(
                    vm,
                    "Client",
                    "Unknown data encoding style",
                ));
            }
        }
        a = xml::next(at);
    }
    Ok(())
}

fn deserialize_function_call(
    vm: &mut Vm,
    sdl: Option<&SdlRef>,
    request: Node,
    actor: Option<&str>,
    soap_action: Option<&str>,
) -> SR<Deserialized> {
    encode_reset_ns();
    let r = deserialize_function_call_inner(vm, sdl, request, actor, soap_action);
    encode_finish();
    r
}

fn deserialize_function_call_inner(
    vm: &mut Vm,
    sdl: Option<&SdlRef>,
    request: Node,
    actor: Option<&str>,
    soap_action: Option<&str>,
) -> SR<Deserialized> {
    let Some((env, version, envelope_ns)) = get_envelope(vm, xml::children(request))? else {
        return Err(soap_server_fault_en(
            vm,
            "Client",
            "looks like we got XML without \"Envelope\" element",
        ));
    };
    {
        let mut a = xml::properties(env);
        while let Some(at) = a {
            if xml::node_ns(at).is_none() {
                return Err(soap_server_fault_en(
                    vm,
                    "Client",
                    "A SOAP Envelope element cannot have non Namespace qualified attributes",
                ));
            } else if xml::attr_is_equal_ex(at, "encodingStyle", Some(SOAP_1_2_ENV_NAMESPACE)) {
                if version == SOAP_1_2 {
                    return Err(soap_server_fault_en(
                        vm,
                        "Client",
                        "encodingStyle cannot be specified on the Envelope",
                    ));
                } else if xml::attr_value(at) != SOAP_1_1_ENC_NAMESPACE {
                    return Err(soap_server_fault_en(
                        vm,
                        "Client",
                        "Unknown data encoding style",
                    ));
                }
            }
            a = xml::next(at);
        }
    }
    let mut head: Option<Node> = None;
    let mut trav = xml::children(env);
    while let Some(t) = trav.filter(|t| !xml::is_element(*t)) {
        trav = xml::next(t);
    }
    if let Some(t) = trav {
        if xml::node_is_equal_ex(t, Some("Header"), Some(envelope_ns)) {
            head = Some(t);
            trav = xml::next(t);
        }
    }
    let mut body: Option<Node> = None;
    while let Some(t) = trav.filter(|t| !xml::is_element(*t)) {
        trav = xml::next(t);
    }
    if let Some(t) = trav {
        if xml::node_is_equal_ex(t, Some("Body"), Some(envelope_ns)) {
            body = Some(t);
            trav = xml::next(t);
        }
    }
    while let Some(t) = trav.filter(|t| !xml::is_element(*t)) {
        trav = xml::next(t);
    }
    let Some(body) = body else {
        return Err(soap_server_fault_en(
            vm,
            "Client",
            "Body must be present in a SOAP envelope",
        ));
    };
    check_attrs(vm, body, version, "Body", false)?;
    if trav.is_some() && version == SOAP_1_2 {
        return Err(soap_server_fault_en(
            vm,
            "Client",
            "A SOAP 1.2 envelope can contain only Header and Body",
        ));
    }
    let mut func: Option<Node> = None;
    let mut t = xml::children(body);
    while let Some(x) = t {
        if xml::is_element(x) {
            func = Some(x);
            break;
        }
        t = xml::next(x);
    }
    let mut function: Option<FuncRef> = None;
    let mut function_name = String::new();
    if let Some(sa) = soap_action {
        if let Some((f, n)) = find_function_using_soap_action(sdl, sa) {
            function = Some(f);
            function_name = n;
        }
    }
    match func {
        None => {
            if function.is_none() {
                match get_doc_function(sdl, None) {
                    Some(f) => {
                        function_name = f.function_name.clone();
                        function = Some(f);
                    }
                    None => {
                        return Err(soap_server_fault_en(
                            vm,
                            "Client",
                            "looks like we got \"Body\" without function call",
                        ));
                    }
                }
            }
        }
        Some(fnode) => {
            if version == SOAP_1_1 {
                if let Some(a) = xml::get_attribute_ex(
                    xml::properties(fnode),
                    "encodingStyle",
                    Some(SOAP_1_1_ENV_NAMESPACE),
                ) {
                    if xml::attr_value(a) != SOAP_1_1_ENC_NAMESPACE {
                        return Err(soap_server_fault_en(
                            vm,
                            "Client",
                            "Unknown Data Encoding Style",
                        ));
                    }
                }
            } else if let Some(a) = xml::get_attribute_ex(
                xml::properties(fnode),
                "encodingStyle",
                Some(SOAP_1_2_ENV_NAMESPACE),
            ) {
                if xml::attr_value(a) != SOAP_1_2_ENC_NAMESPACE {
                    return Err(soap_server_fault_en(
                        vm,
                        "DataEncodingUnknown",
                        "Unknown Data Encoding Style",
                    ));
                }
            }
            if function.is_none() {
                let (f, n) = find_function(sdl, fnode);
                function = f;
                function_name = n;
            }
            if sdl.is_some() && function.is_none() {
                if version == SOAP_1_2 {
                    return Err(soap_server_fault_en(
                        vm,
                        "rpc:ProcedureNotPresent",
                        "Procedure not present",
                    ));
                } else {
                    let m = format!("Procedure '{}' not present", xml::name(fnode));
                    return Err(raise_fatal(vm, &m));
                }
            }
        }
    }
    let mut headers: Vec<HeaderEntry> = Vec::new();
    if let Some(head) = head {
        let mut a = xml::properties(head);
        while let Some(at) = a {
            if xml::node_ns(at).is_none() {
                return Err(soap_server_fault_en(
                    vm,
                    "Client",
                    "A SOAP Header element cannot have non Namespace qualified attributes",
                ));
            } else if xml::attr_is_equal_ex(at, "encodingStyle", Some(SOAP_1_2_ENV_NAMESPACE)) {
                if version == SOAP_1_2 {
                    return Err(soap_server_fault_en(
                        vm,
                        "Client",
                        "encodingStyle cannot be specified on the Header",
                    ));
                } else if xml::attr_value(at) != SOAP_1_1_ENC_NAMESPACE {
                    return Err(soap_server_fault_en(
                        vm,
                        "Client",
                        "Unknown data encoding style",
                    ));
                }
            }
            a = xml::next(at);
        }
        let mut trav = xml::children(head);
        while let Some(hdr_func) = trav {
            if xml::is_element(hdr_func) {
                let mut ignore = false;
                let props = xml::properties(hdr_func);
                if version == SOAP_1_1 {
                    if let Some(a) =
                        xml::get_attribute_ex(props, "encodingStyle", Some(SOAP_1_1_ENV_NAMESPACE))
                    {
                        if xml::attr_value(a) != SOAP_1_1_ENC_NAMESPACE {
                            return Err(soap_server_fault_en(
                                vm,
                                "Client",
                                "Unknown Data Encoding Style",
                            ));
                        }
                    }
                    if let Some(a) = xml::get_attribute_ex(props, "actor", Some(envelope_ns)) {
                        let v = xml::attr_value(a);
                        if v != SOAP_1_1_ACTOR_NEXT && actor != Some(v.as_str()) {
                            ignore = true;
                        }
                    }
                } else {
                    if let Some(a) =
                        xml::get_attribute_ex(props, "encodingStyle", Some(SOAP_1_2_ENV_NAMESPACE))
                    {
                        if xml::attr_value(a) != SOAP_1_2_ENC_NAMESPACE {
                            return Err(soap_server_fault_en(
                                vm,
                                "DataEncodingUnknown",
                                "Unknown Data Encoding Style",
                            ));
                        }
                    }
                    if let Some(a) = xml::get_attribute_ex(props, "role", Some(envelope_ns)) {
                        let v = xml::attr_value(a);
                        if v != SOAP_1_2_ACTOR_UNLIMATERECEIVER
                            && v != SOAP_1_2_ACTOR_NEXT
                            && actor != Some(v.as_str())
                        {
                            ignore = true;
                        }
                    }
                }
                if !ignore {
                    let mut must_understand = false;
                    if let Some(a) =
                        xml::get_attribute_ex(props, "mustUnderstand", Some(envelope_ns))
                    {
                        let v = xml::attr_value(a);
                        if v == "1" || v == "true" {
                            must_understand = true;
                        } else if v == "0" || v == "false" {
                            must_understand = false;
                        } else {
                            return Err(soap_server_fault_en(
                                vm,
                                "Client",
                                "mustUnderstand value is not boolean",
                            ));
                        }
                    }
                    let (hfunction, hname) = find_function(sdl, hdr_func);
                    let mut hdr: Option<HeaderRef> = None;
                    if hfunction.is_none() && sdl.is_some() {
                        if let Some(fnb) = function.as_ref().and_then(|f| f.soap()) {
                            if let Some(ih) = &fnb.input.headers {
                                let mut key = String::new();
                                if let Some(h) = xml::ns_href_of(hdr_func) {
                                    key.push_str(&h);
                                    key.push(':');
                                }
                                key.push_str(&hname);
                                hdr = ih.find(&key).cloned();
                            }
                        }
                    }
                    let parameters = if let Some(h) = &hdr {
                        vec![master_to_zval(vm, h.encode.as_ref(), hdr_func)?]
                    } else {
                        let mut node = Some(hdr_func);
                        if let Some(fnb) = hfunction.as_ref().and_then(|f| f.soap()) {
                            if fnb.style == SOAP_RPC {
                                node = xml::children(hdr_func);
                            }
                        }
                        deserialize_parameters(vm, node, hfunction.as_ref())?
                    };
                    headers.push(HeaderEntry {
                        function: hfunction,
                        function_name: Some(hname),
                        must_understand,
                        parameters,
                        retval: Value::Null,
                        hdr,
                    });
                }
            }
            trav = xml::next(hdr_func);
        }
    }
    let params_node = match function.as_ref().and_then(|f| f.soap()) {
        Some(fnb) => {
            if fnb.style == SOAP_RPC {
                func.and_then(xml::children)
            } else {
                func
            }
        }
        None => func.and_then(xml::children),
    };
    let params = deserialize_parameters(vm, params_node, function.as_ref())?;
    Ok((function, function_name, params, version, headers))
}

// ---------------------------------------------------------------------------
// Response serialization

/// set_soap_header_attributes()
pub fn set_soap_header_attributes(h: Node, header: &Obj, version: i32) {
    if matches!(prop(header, "mustUnderstand"), Value::True) {
        if version == SOAP_1_1 {
            xml::set_prop(h, "SOAP-ENV:mustUnderstand", "1");
        } else {
            xml::set_prop(h, "env:mustUnderstand", "true");
        }
    }
    match prop(header, "actor") {
        Value::String(s) => {
            let s = s.to_string_lossy();
            if version == SOAP_1_1 {
                xml::set_prop(h, "SOAP-ENV:actor", &s);
            } else {
                xml::set_prop(h, "env:role", &s);
            }
        }
        Value::Long(n) => {
            if version == SOAP_1_1 {
                if n == SOAP_ACTOR_NEXT {
                    xml::set_prop(h, "SOAP-ENV:actor", SOAP_1_1_ACTOR_NEXT);
                }
            } else if n == SOAP_ACTOR_NEXT {
                xml::set_prop(h, "env:role", SOAP_1_2_ACTOR_NEXT);
            } else if n == SOAP_ACTOR_NONE {
                xml::set_prop(h, "env:role", SOAP_1_2_ACTOR_NONE);
            } else if n == SOAP_ACTOR_UNLIMATERECEIVER {
                xml::set_prop(h, "env:role", SOAP_1_2_ACTOR_UNLIMATERECEIVER);
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn serialize_response_call2(
    vm: &mut Vm,
    body: Node,
    function: Option<&FuncRef>,
    function_name: Option<&str>,
    uri: Option<&str>,
    ret: Option<&Value>,
    version: i32,
    main: bool,
    node: Option<&mut Option<Node>>,
) -> SR<i32> {
    let mut method: Option<Node> = None;
    let style;
    let use_;
    let soap_fn = function.and_then(|f| f.soap().cloned());
    if let Some(f) = function.filter(|f| f.is_soap()) {
        let fnb = soap_fn.clone().unwrap();
        style = fnb.style;
        use_ = fnb.output.use_;
        if style == SOAP_RPC {
            let ns = encode_add_ns(body, fnb.output.ns.as_deref());
            if let Some(rn) = &f.response_name {
                method = Some(xml::new_child(body, ns, rn, None));
            } else if f.response_params.is_some() {
                method = Some(xml::new_child(body, ns, &f.function_name, None));
            }
        }
    } else {
        style = if main { SOAP_RPC } else { SOAP_DOCUMENT };
        use_ = if main { SOAP_ENCODED } else { SOAP_LITERAL };
        if style == SOAP_RPC {
            let ns = encode_add_ns(body, uri);
            method = Some(xml::new_child(body, ns, function_name.unwrap_or(""), None));
        }
    }
    let param_count = match function {
        Some(f) => f.response_params.as_ref().map_or(0, |r| r.len()),
        None => 1,
    };
    let is_soap = function.is_some_and(|f| f.is_soap());
    if param_count == 1 {
        let parameter = get_param(function, None, 0, true);
        if style == SOAP_RPC {
            let m = method.unwrap_or(body);
            if main && version == SOAP_1_2 {
                let rpc_ns = xml::new_ns(body, RPC_SOAP12_NAMESPACE, Some(RPC_SOAP12_NS_PREFIX));
                let rpc_result = xml::new_child(m, rpc_ns, "result", None);
                let param = crate::common::serialize_parameter(
                    vm,
                    parameter.as_ref(),
                    ret,
                    0,
                    Some("return"),
                    use_,
                    m,
                )?;
                xml::node_set_content(rpc_result, &xml::name(param));
            } else {
                crate::common::serialize_parameter(
                    vm,
                    parameter.as_ref(),
                    ret,
                    0,
                    Some("return"),
                    use_,
                    m,
                )?;
            }
        } else {
            let param = crate::common::serialize_parameter(
                vm,
                parameter.as_ref(),
                ret,
                0,
                Some("return"),
                use_,
                body,
            )?;
            if is_soap {
                if let Some(el) = parameter.as_ref().and_then(|p| p.element.clone()) {
                    let (name, namens) = {
                        let e = el.borrow();
                        (e.name.clone(), e.namens.clone())
                    };
                    let ns = encode_add_ns(param, namens.as_deref());
                    xml::set_name(param, name.as_deref().unwrap_or(""));
                    xml::set_ns(param, ns);
                }
            } else if xml::name(param) == "return" {
                let ns = encode_add_ns(param, uri);
                xml::set_name(param, function_name.unwrap_or(""));
                xml::set_ns(param, ns);
            }
        }
    } else if param_count > 1 {
        if let Some(Value::Array(a)) = ret.map(|r| r.deref()) {
            let entries: Vec<(ArrayKey, Value)> = a
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (i, (k, data)) in entries.into_iter().enumerate() {
                let (pname, pindex) = match &k {
                    ArrayKey::String(s) => (Some(s.to_string_lossy()), i),
                    ArrayKey::Int(n) => (None, if *n < 0 { usize::MAX } else { *n as usize }),
                };
                let parameter = get_param(function, pname.as_deref(), pindex, true);
                if style == SOAP_RPC {
                    let m = method.unwrap_or(body);
                    crate::common::serialize_parameter(
                        vm,
                        parameter.as_ref(),
                        Some(&data),
                        i,
                        pname.as_deref(),
                        use_,
                        m,
                    )?;
                } else {
                    let param = crate::common::serialize_parameter(
                        vm,
                        parameter.as_ref(),
                        Some(&data),
                        i,
                        pname.as_deref(),
                        use_,
                        body,
                    )?;
                    if is_soap {
                        if let Some(el) = parameter.as_ref().and_then(|p| p.element.clone()) {
                            let (name, namens) = {
                                let e = el.borrow();
                                (e.name.clone(), e.namens.clone())
                            };
                            let ns = encode_add_ns(param, namens.as_deref());
                            xml::set_name(param, name.as_deref().unwrap_or(""));
                            xml::set_ns(param, ns);
                        }
                    }
                }
            }
        }
    }
    if use_ == SOAP_ENCODED && version == SOAP_1_2 {
        if let Some(m) = method {
            let bns = xml::node_ns(body);
            xml::set_ns_prop(m, bns, "encodingStyle", SOAP_1_2_ENC_NAMESPACE);
        }
    }
    if let Some(n) = node {
        *n = method;
    }
    Ok(use_)
}

/// php_escape_html_entities(str, 0, 0 /* ENT_HTML_QUOTE_NONE */)
fn escape_html(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            c => o.push(c),
        }
    }
    o
}

/// serialize_response_call()
pub fn serialize_response_call(
    vm: &mut Vm,
    function: Option<&FuncRef>,
    function_name: Option<&str>,
    uri: Option<&str>,
    ret: &Value,
    headers: &[HeaderEntry],
    version: i32,
) -> SR<Option<Node>> {
    encode_reset_ns();
    let doc = xml::new_doc();
    let r =
        serialize_response_call_inner(vm, doc, function, function_name, uri, ret, headers, version);
    encode_finish();
    match r {
        Ok(true) => Ok(Some(doc)),
        Ok(false) => {
            xml::free_doc(doc);
            Ok(None)
        }
        Err(e) => {
            xml::free_doc(doc);
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn serialize_response_call_inner(
    vm: &mut Vm,
    doc: Node,
    function: Option<&FuncRef>,
    function_name: Option<&str>,
    uri: Option<&str>,
    ret: &Value,
    headers: &[HeaderEntry],
    version: i32,
) -> SR<bool> {
    let envelope = xml::new_doc_node(Some(doc), "Envelope");
    let ns = if version == SOAP_1_1 {
        xml::new_ns(
            envelope,
            SOAP_1_1_ENV_NAMESPACE,
            Some(SOAP_1_1_ENV_NS_PREFIX),
        )
    } else if version == SOAP_1_2 {
        xml::new_ns(
            envelope,
            SOAP_1_2_ENV_NAMESPACE,
            Some(SOAP_1_2_ENV_NS_PREFIX),
        )
    } else {
        return Err(soap_server_fault_en(vm, "Server", "Unknown SOAP version"));
    };
    xml::set_ns(envelope, ns);
    xml::doc_set_root(doc, envelope);
    let mut use_ = SOAP_LITERAL;
    let mut head: Option<Node> = None;
    let body;
    let ret_d = ret.deref();
    if val_instance_of(vm, &ret_d, "SoapFault") {
        let fo = is_obj(&ret_d).unwrap();
        let headerfault = prop(&fo, "headerfault");
        if let Some(h0) = headers.first() {
            if !matches!(headerfault, Value::Null | Value::Undef) {
                let mut hdr_enc: Option<EncRef> = None;
                let mut hdr_use = SOAP_LITERAL;
                let mut hdr_ret: Option<Value> = Some(headerfault.clone());
                let mut hdr_ns = h0.hdr.as_ref().and_then(|h| h.ns.clone());
                let mut hdr_name = h0.function_name.clone();
                let hd = xml::new_child(envelope, ns, "Header", None);
                head = Some(hd);
                if let Some(ho) = is_obj(&headerfault).filter(|o| instance_of(vm, o, "SoapHeader"))
                {
                    let mut key = String::new();
                    if let Value::String(s) = prop(&ho, "namespace") {
                        key.push_str(&s.to_string_lossy());
                        key.push(':');
                        hdr_ns = Some(s.to_string_lossy());
                    }
                    if let Value::String(s) = prop(&ho, "name") {
                        key.push_str(&s.to_string_lossy());
                        hdr_name = Some(s.to_string_lossy());
                    }
                    if let Some(hf) = h0.hdr.as_ref().and_then(|h| h.headerfaults.as_ref()) {
                        if let Some(h) = hf.find(&key) {
                            hdr_enc = h.encode.clone();
                            hdr_use = h.use_;
                        }
                    }
                    let d = prop(&ho, "data");
                    hdr_ret = if matches!(d, Value::Null | Value::Undef) {
                        None
                    } else {
                        Some(d)
                    };
                }
                if let Some(hf) = &h0.function {
                    let fname = h0.function_name.clone();
                    // (the encoded use is overridden for the fault below)
                    serialize_response_call2(
                        vm,
                        hd,
                        Some(hf),
                        fname.as_deref(),
                        uri,
                        hdr_ret.as_ref(),
                        version,
                        false,
                        None,
                    )?;
                } else {
                    let x = master_to_xml(vm, hdr_enc.as_ref(), hdr_ret.as_ref(), hdr_use, hd)?;
                    if let Some(n) = &hdr_name {
                        xml::set_name(x, n);
                    }
                    if let Some(n) = &hdr_ns {
                        let nsp = encode_add_ns(x, Some(n));
                        xml::set_ns(x, nsp);
                    }
                }
            }
        }
        body = xml::new_child(envelope, ns, "Body", None);
        let param = xml::new_child(body, ns, "Fault", None);
        let mut fault_ns: Option<String> = as_str(&prop(&fo, "faultcodens"));
        use_ = SOAP_LITERAL;
        let mut fault: Option<Rc<SdlFault>> = None;
        let fname = prop(&fo, "_name");
        if let Value::String(n) = &fname {
            if let Some(tf) = function
                .and_then(|f| f.faults.as_ref())
                .and_then(|fs| fs.find(&n.to_string_lossy()).cloned())
            {
                if function.unwrap().is_soap() {
                    if let Some(fb) = &tf.binding {
                        use_ = fb.use_;
                        if fault_ns.is_none() {
                            fault_ns = fb.ns.clone();
                        }
                    }
                }
                fault = Some(tf);
            }
        } else if let Some(fs) = function
            .and_then(|f| f.faults.as_ref())
            .filter(|fs| fs.len() == 1)
        {
            let tf = fs.first().cloned().unwrap();
            if function.unwrap().is_soap() {
                if let Some(fb) = &tf.binding {
                    use_ = fb.use_;
                    if fault_ns.is_none() {
                        fault_ns = fb.ns.clone();
                    }
                }
            }
            fault = Some(tf);
        }
        if fault_ns.is_none() {
            if let Some(f) = &fault {
                if f.details.len() == 1 {
                    if let Some(el) = &f.details[0].element {
                        fault_ns = el.borrow().namens.clone();
                    }
                }
            }
        }
        let string_enc = conv(IS_STRING);
        let detail_name = if version == SOAP_1_1 {
            if let Value::String(code) = prop(&fo, "faultcode") {
                let node = xml::new_doc_node(Some(doc), "faultcode");
                let s = escape_html(&code.to_string_lossy());
                xml::add_child(param, node);
                match &fault_ns {
                    Some(fns) => {
                        let nsptr = encode_add_ns(node, Some(fns));
                        let prefix = nsptr.and_then(xml::ns_prefix);
                        let q = match prefix {
                            Some(p) => format!("{}:{}", p, s),
                            None => s,
                        };
                        xml::node_set_content(node, &q);
                    }
                    None => xml::node_set_content(node, &s),
                }
            }
            let fs = prop(&fo, "faultstring");
            if let Value::String(_) = &fs {
                let node = master_to_xml(vm, Some(&string_enc), Some(&fs), SOAP_LITERAL, param)?;
                xml::set_name(node, "faultstring");
            }
            let fa = prop(&fo, "faultactor");
            if let Value::String(_) = &fa {
                let node = master_to_xml(vm, Some(&string_enc), Some(&fa), SOAP_LITERAL, param)?;
                xml::set_name(node, "faultactor");
            }
            "detail".to_string()
        } else {
            if let Value::String(code) = prop(&fo, "faultcode") {
                let node = xml::new_child(param, ns, "Code", None);
                let s = escape_html(&code.to_string_lossy());
                let node = xml::new_child(node, ns, "Value", None);
                match &fault_ns {
                    Some(fns) => {
                        let nsptr = encode_add_ns(node, Some(fns));
                        let prefix = nsptr.and_then(xml::ns_prefix);
                        let q = match prefix {
                            Some(p) => format!("{}:{}", p, s),
                            None => s,
                        };
                        xml::node_set_content(node, &q);
                    }
                    None => xml::node_set_content(node, &s),
                }
            }
            let fs = prop(&fo, "faultstring");
            if let Value::String(_) = &fs {
                let node = xml::new_child(param, ns, "Reason", None);
                let node = master_to_xml(vm, Some(&string_enc), Some(&fs), SOAP_LITERAL, node)?;
                xml::set_name(node, "Text");
                xml::set_ns(node, ns);
                let lang = as_str(&prop(&fo, "lang")).unwrap_or_default();
                xml::set_lang(node, &lang);
            }
            format!("{}:Detail", SOAP_1_2_ENV_NS_PREFIX)
        };
        let detail_v = prop(&fo, "detail");
        if let Some(f) = fault.as_ref().filter(|f| f.details.len() == 1) {
            let mut detail: Option<Value> = if matches!(detail_v, Value::Null | Value::Undef) {
                None
            } else {
                Some(detail_v.clone())
            };
            let node = xml::new_doc_node(Some(doc), &detail_name);
            xml::add_child(param, node);
            let sparam = f.details[0].clone();
            if let (Some(Value::Object(d)), Some(el)) = (detail.as_ref(), &sparam.element) {
                let table = vm.object_prop_table(d);
                if table.len() == 1 {
                    let ename = el.borrow().name.clone().unwrap_or_default();
                    if let Some(e) = table.iter().find(|e| e.key == ename.as_bytes()) {
                        detail = Some(e.value.deref());
                    }
                }
            }
            let x = crate::common::serialize_parameter(
                vm,
                Some(&sparam),
                detail.as_ref(),
                1,
                None,
                use_,
                node,
            )?;
            if let Some(fnb) = function.and_then(|f| f.soap()) {
                if fnb.style == SOAP_RPC && sparam.element.is_none() {
                    if let Some(fb) = &f.binding {
                        if let Some(fns) = &fb.ns {
                            let nsp = encode_add_ns(x, Some(fns));
                            xml::set_ns(x, nsp);
                        }
                    }
                } else if let Some(el) = &sparam.element {
                    let (name, namens) = {
                        let e = el.borrow();
                        (e.name.clone(), e.namens.clone())
                    };
                    let nsp = encode_add_ns(x, namens.as_deref());
                    xml::set_name(x, name.as_deref().unwrap_or(""));
                    xml::set_ns(x, nsp);
                }
            }
            if use_ == SOAP_ENCODED && version == SOAP_1_2 {
                let ens = xml::node_ns(envelope);
                xml::set_ns_prop(x, ens, "encodingStyle", SOAP_1_2_ENC_NAMESPACE);
            }
        } else if !matches!(detail_v, Value::Null | Value::Undef) {
            crate::common::serialize_zval(vm, Some(&detail_v), None, &detail_name, use_, param)?;
        }
    } else {
        if !headers.is_empty() {
            let hd = xml::new_child(envelope, ns, "Header", None);
            head = Some(hd);
            for h in headers {
                if matches!(h.retval, Value::Null) {
                    continue;
                }
                let mut hdr_enc: Option<EncRef> = None;
                let mut hdr_use = SOAP_LITERAL;
                let mut hdr_ret: Option<Value> = Some(h.retval.clone());
                let mut hdr_ns = h.hdr.as_ref().and_then(|x| x.ns.clone());
                let mut hdr_name = h.function_name.clone();
                let header_obj = is_obj(&h.retval).filter(|o| instance_of(vm, o, "SoapHeader"));
                if let Some(ho) = &header_obj {
                    let mut key = String::new();
                    if let Value::String(s) = prop(ho, "namespace") {
                        key.push_str(&s.to_string_lossy());
                        key.push(':');
                        hdr_ns = Some(s.to_string_lossy());
                    }
                    if let Value::String(s) = prop(ho, "name") {
                        key.push_str(&s.to_string_lossy());
                        hdr_name = Some(s.to_string_lossy());
                    }
                    if let Some(fnb) = function.and_then(|f| f.soap()) {
                        if let Some(hh) = fnb.output.headers.as_ref().and_then(|hs| hs.find(&key)) {
                            hdr_enc = hh.encode.clone();
                            hdr_use = hh.use_;
                        }
                    }
                    let d = prop(ho, "data");
                    hdr_ret = if matches!(d, Value::Null | Value::Undef) {
                        None
                    } else {
                        Some(d)
                    };
                }
                if let Some(hf) = &h.function {
                    let mut xml_hdr: Option<Node> = None;
                    if serialize_response_call2(
                        vm,
                        hd,
                        Some(hf),
                        h.function_name.as_deref(),
                        uri,
                        hdr_ret.as_ref(),
                        version,
                        false,
                        Some(&mut xml_hdr),
                    )? == SOAP_ENCODED
                    {
                        use_ = SOAP_ENCODED;
                    }
                    if let (Some(ho), Some(xh)) = (&header_obj, xml_hdr) {
                        set_soap_header_attributes(xh, ho, version);
                    }
                } else {
                    let xh = master_to_xml(vm, hdr_enc.as_ref(), hdr_ret.as_ref(), hdr_use, hd)?;
                    if let Some(n) = &hdr_name {
                        xml::set_name(xh, n);
                    }
                    if let Some(n) = &hdr_ns {
                        let nsp = encode_add_ns(xh, Some(n));
                        xml::set_ns(xh, nsp);
                    }
                    if let Some(ho) = &header_obj {
                        set_soap_header_attributes(xh, ho, version);
                    }
                }
            }
            if xml::children(hd).is_none() {
                xml::unlink(hd);
                head = None;
            }
        }
        body = xml::new_child(envelope, ns, "Body", None);
        if serialize_response_call2(
            vm,
            body,
            function,
            function_name,
            uri,
            Some(ret),
            version,
            true,
            None,
        )? == SOAP_ENCODED
        {
            use_ = SOAP_ENCODED;
        }
    }
    if use_ == SOAP_ENCODED {
        xml::new_ns(envelope, XSD_NAMESPACE, Some(XSD_NS_PREFIX));
        if version == SOAP_1_1 {
            xml::new_ns(
                envelope,
                SOAP_1_1_ENC_NAMESPACE,
                Some(SOAP_1_1_ENC_NS_PREFIX),
            );
            let ens = xml::node_ns(envelope);
            xml::set_ns_prop(envelope, ens, "encodingStyle", SOAP_1_1_ENC_NAMESPACE);
        } else if version == SOAP_1_2 {
            xml::new_ns(
                envelope,
                SOAP_1_2_ENC_NAMESPACE,
                Some(SOAP_1_2_ENC_NS_PREFIX),
            );
        }
    }
    if let Some(f) = function {
        if f.response_name.is_none() && xml::children(body).is_none() && head.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}
