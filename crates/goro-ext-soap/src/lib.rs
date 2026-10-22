//! ext/soap: SoapClient, SoapServer and friends (soap.c, php_encoding.c,
//! php_sdl.c, php_schema.c, php_packet_soap.c, php_http.c, php_xml.c).
//!
//! The classes are declared from PHP source (prelude.php) and made
//! internal; their methods forward to `__soap_method()`, the native
//! implementation, whose own frame is kept out of backtraces.

pub mod client;
pub mod common;
pub mod encoding;
pub mod http;
pub mod http_client;
pub mod packet;
pub mod php;
pub mod schema;
pub mod sdl;
pub mod server;
pub mod types;
pub mod xml;
pub mod zpp;

use std::rc::Rc;

use goro_core::object::ClassEntry;
use goro_core::opcode::OpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::php::*;
use crate::types::*;
use crate::zpp::{Arg, Func};

pub const NAME: &[u8] = b"soap";

const PRELUDE: &[u8] = include_bytes!("prelude.php");

const CLASSES: &[&str] = &[
    "Soap\\Url",
    "Soap\\Sdl",
    "SoapParam",
    "SoapHeader",
    "SoapFault",
    "SoapVar",
    "SoapServer",
    "SoapClient",
];

const LONG_CONSTANTS: &[(&str, i64)] = &[
    ("SOAP_1_1", 1),
    ("SOAP_1_2", 2),
    ("SOAP_PERSISTENCE_SESSION", 1),
    ("SOAP_PERSISTENCE_REQUEST", 2),
    ("SOAP_FUNCTIONS_ALL", 999),
    ("SOAP_ENCODED", 1),
    ("SOAP_LITERAL", 2),
    ("SOAP_RPC", 1),
    ("SOAP_DOCUMENT", 2),
    ("SOAP_ACTOR_NEXT", 1),
    ("SOAP_ACTOR_NONE", 2),
    ("SOAP_ACTOR_UNLIMATERECEIVER", 3),
    ("SOAP_COMPRESSION_ACCEPT", 0x20),
    ("SOAP_COMPRESSION_GZIP", 0x00),
    ("SOAP_COMPRESSION_DEFLATE", 0x10),
    ("SOAP_AUTHENTICATION_BASIC", 0),
    ("SOAP_AUTHENTICATION_DIGEST", 1),
    ("UNKNOWN_TYPE", 999998),
    ("XSD_STRING", 101),
    ("XSD_BOOLEAN", 102),
    ("XSD_DECIMAL", 103),
    ("XSD_FLOAT", 104),
    ("XSD_DOUBLE", 105),
    ("XSD_DURATION", 106),
    ("XSD_DATETIME", 107),
    ("XSD_TIME", 108),
    ("XSD_DATE", 109),
    ("XSD_GYEARMONTH", 110),
    ("XSD_GYEAR", 111),
    ("XSD_GMONTHDAY", 112),
    ("XSD_GDAY", 113),
    ("XSD_GMONTH", 114),
    ("XSD_HEXBINARY", 115),
    ("XSD_BASE64BINARY", 116),
    ("XSD_ANYURI", 117),
    ("XSD_QNAME", 118),
    ("XSD_NOTATION", 119),
    ("XSD_NORMALIZEDSTRING", 120),
    ("XSD_TOKEN", 121),
    ("XSD_LANGUAGE", 122),
    ("XSD_NMTOKEN", 123),
    ("XSD_NAME", 124),
    ("XSD_NCNAME", 125),
    ("XSD_ID", 126),
    ("XSD_IDREF", 127),
    ("XSD_IDREFS", 128),
    ("XSD_ENTITY", 129),
    ("XSD_ENTITIES", 130),
    ("XSD_INTEGER", 131),
    ("XSD_NONPOSITIVEINTEGER", 132),
    ("XSD_NEGATIVEINTEGER", 133),
    ("XSD_LONG", 134),
    ("XSD_INT", 135),
    ("XSD_SHORT", 136),
    ("XSD_BYTE", 137),
    ("XSD_NONNEGATIVEINTEGER", 138),
    ("XSD_UNSIGNEDLONG", 139),
    ("XSD_UNSIGNEDINT", 140),
    ("XSD_UNSIGNEDSHORT", 141),
    ("XSD_UNSIGNEDBYTE", 142),
    ("XSD_POSITIVEINTEGER", 143),
    ("XSD_NMTOKENS", 144),
    ("XSD_ANYTYPE", 145),
    ("XSD_ANYXML", 147),
    ("APACHE_MAP", 200),
    ("SOAP_ENC_OBJECT", 301),
    ("SOAP_ENC_ARRAY", 300),
    ("XSD_1999_TIMEINSTANT", 401),
    ("SOAP_SINGLE_ELEMENT_ARRAYS", 1),
    ("SOAP_WAIT_ONE_WAY_CALLS", 2),
    ("SOAP_USE_XSI_ARRAY_TYPE", 4),
    ("WSDL_CACHE_NONE", 0),
    ("WSDL_CACHE_DISK", 1),
    ("WSDL_CACHE_MEMORY", 2),
    ("WSDL_CACHE_BOTH", 3),
    ("SOAP_SSL_METHOD_TLS", 0),
    ("SOAP_SSL_METHOD_SSLv2", 1),
    ("SOAP_SSL_METHOD_SSLv3", 2),
    ("SOAP_SSL_METHOD_SSLv23", 3),
];

const INI: &[(&str, &str)] = &[
    ("soap.wsdl_cache_enabled", "1"),
    ("soap.wsdl_cache_dir", "/tmp"),
    ("soap.wsdl_cache_ttl", "86400"),
    ("soap.wsdl_cache", "1"),
    ("soap.wsdl_cache_limit", "5"),
];

pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    for (name, f) in [
        (
            "use_soap_error_handler",
            use_soap_error_handler as goro_core::vm::BuiltinFn,
        ),
        ("is_soap_fault", is_soap_fault),
    ] {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
    }
    vm.register_function(b"__soap_method", soap_method);
    let mut names = Vec::new();
    for &(name, v) in LONG_CONSTANTS {
        vm.constants
            .insert(name.as_bytes().to_vec(), Value::Long(v));
        names.push(name.as_bytes().to_vec());
    }
    for (name, v) in [
        ("XSD_NAMESPACE", XSD_NAMESPACE),
        ("XSD_1999_NAMESPACE", XSD_1999_NAMESPACE),
    ] {
        vm.constants.insert(
            name.as_bytes().to_vec(),
            Value::String(PhpString::from_bytes(v.as_bytes())),
        );
        names.push(name.as_bytes().to_vec());
    }
    vm.internal_deprecated_constants.insert(
        b"SOAP_FUNCTIONS_ALL".to_vec(),
        "Constant SOAP_FUNCTIONS_ALL is deprecated since 8.4, as enabling all functions is a possible security concern"
            .to_string(),
    );
    vm.module_constants.push((NAME.to_vec(), names));
    for &(k, v) in INI {
        vm.constants.insert(
            k.as_bytes().to_vec(),
            Value::String(PhpString::from_bytes(v.as_bytes())),
        );
    }
    register_classes(vm);
}

fn register_classes(vm: &mut Vm) {
    let Some((op_array, classes)) = compile_prelude() else {
        return;
    };
    let saved_file = std::mem::take(&mut vm.current_file);
    let saved_line = vm.current_line;
    let _ = vm.run_compiled(&op_array, classes);
    vm.current_file = saved_file;
    vm.current_line = saved_line;
    vm.current_exception = None;
    for c in CLASSES {
        make_internal(vm, &c.to_ascii_lowercase());
    }
}

fn compile_prelude() -> Option<(OpArray, Vec<ClassEntry>)> {
    let mut lexer = goro_parser::Lexer::new(PRELUDE);
    let tokens = lexer.tokenize();
    let _ = goro_parser::take_lexer_deprecations();
    let _ = goro_parser::take_lexer_warnings();
    let mut parser = goro_parser::Parser::new(tokens).with_source(PRELUDE);
    let program = parser.parse().ok()?;
    let compiler = goro_core::compiler::Compiler::new();
    let (mut op_array, mut classes, _) = compiler.compile(&program).ok()?;
    let _ = goro_core::compiler::take_pending_compile_warnings();
    clear_lines(&mut op_array);
    for c in &mut classes {
        for m in c.methods.values_mut() {
            clear_lines(Rc::make_mut(&mut m.op_array));
        }
    }
    Some((op_array, classes))
}

/// Internal code has no lines: the VM keeps reporting the caller's.
fn clear_lines(op: &mut OpArray) {
    for o in &mut op.ops {
        o.line = 0;
    }
    op.decl_line = 0;
    op.end_line = 0;
    op.filename.clear();
    for c in &mut op.child_functions {
        clear_lines(c);
    }
}

fn make_internal(vm: &mut Vm, lower: &str) {
    let Some(ce) = vm.classes.get_mut(lower.as_bytes()) else {
        return;
    };
    ce.filename = None;
    ce.start_line = 0;
    ce.end_line = 0;
    ce.doc_comment = None;
    let Some(sc) = goro_core::stubs::find_self_declared_class(lower.as_bytes()) else {
        return;
    };
    for m in ce.methods.values_mut() {
        let Some(f) = sc
            .methods
            .iter()
            .find(|f| f.name.as_bytes().eq_ignore_ascii_case(&m.name))
        else {
            continue;
        };
        let oa = Rc::make_mut(&mut m.op_array);
        oa.internal_sig = Some(f);
        oa.doc_comment = None;
        m.doc_comment = None;
    }
}

// ---------------------------------------------------------------------------
// Functions

fn use_soap_error_handler(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = Func::new("use_soap_error_handler");
    let old = g(|g| g.use_soap_error_handler);
    if let Ok(a) = zpp::parse(vm, &f, "|b", &[], args) {
        let h = match a.first() {
            Some(Arg::Bool(b)) => *b,
            _ => true,
        };
        g(|g| g.use_soap_error_handler = h);
    } else {
        return Ok(Value::Null);
    }
    Ok(if old { Value::True } else { Value::False })
}

fn is_soap_fault(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = Func::new("is_soap_fault");
    match zpp::parse(vm, &f, "z", &[], args) {
        Ok(a) => {
            let v = a[0].any().unwrap_or(Value::Null);
            Ok(if val_instance_of(vm, &v, "SoapFault") {
                Value::True
            } else {
                Value::False
            })
        }
        Err(_) => Ok(Value::Null),
    }
}

// ---------------------------------------------------------------------------
// Methods

/// `__soap_method(name, $this, args)`: the native bodies of the methods.
fn soap_method(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // Keep this helper's frame out of backtraces: the method's own frame
    // stands for the internal call.
    let hidden = vm
        .current_builtin_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("__soap_method"));
    if hidden {
        vm.pop_builtin_frame();
    }
    let r = dispatch(vm, args);
    if hidden {
        vm.push_builtin_frame(b"__soap_method", &[], 0);
    }
    match r {
        Ok(v) => Ok(v),
        Err(Bail) => match take_pending_fatal() {
            Some(e) => Err(e),
            None => Ok(Value::Null),
        },
    }
}

fn dispatch(vm: &mut Vm, args: &[Value]) -> SR<Value> {
    let name = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let this = match args.get(1).map(|v| v.deref()) {
        Some(Value::Object(o)) => o,
        _ => {
            if name == "SoapClient::__soapCall" {
                return Ok(client::take_out_headers());
            }
            return Ok(Value::Null);
        }
    };
    let f = Func::new(&name);
    let margs = f.positional(vm, args.get(2).unwrap_or(&Value::Null))?;
    let a = &margs[..];
    match name.as_str() {
        "SoapParam::__construct" => soap_param_construct(vm, &f, &this, a),
        "SoapHeader::__construct" => soap_header_construct(vm, &f, &this, a),
        "SoapFault::__construct" => soap_fault_construct(vm, &f, &this, a),
        "SoapFault::__toString" => soap_fault_to_string(vm, &f, &this, a),
        "SoapVar::__construct" => soap_var_construct(vm, &f, &this, a),
        "SoapServer::__construct" => server::construct(vm, &f, &this, a),
        "SoapServer::fault" => server::fault(vm, &f, &this, a),
        "SoapServer::addSoapHeader" => server::add_soap_header(vm, &f, &this, a),
        "SoapServer::setPersistence" => server::set_persistence(vm, &f, &this, a),
        "SoapServer::setClass" => server::set_class(vm, &f, &this, a),
        "SoapServer::setObject" => server::set_object(vm, &f, &this, a),
        "SoapServer::getFunctions" => server::get_functions(vm, &f, &this, a),
        "SoapServer::addFunction" => server::add_function(vm, &f, &this, a),
        "SoapServer::handle" => server::handle(vm, &f, &this, a),
        "SoapServer::__getLastResponse" => server::get_last_response(vm, &f, &this, a),
        "SoapClient::__construct" => client::construct(vm, &f, &this, a),
        "SoapClient::__call" => client::call(vm, &f, &this, a),
        "SoapClient::__soapCall" => client::soap_call(vm, &f, &this, a),
        "SoapClient::__getFunctions" => client::get_functions(vm, &f, &this, a),
        "SoapClient::__getTypes" => client::get_types(vm, &f, &this, a),
        "SoapClient::__getLastRequest" => client::get_prop(vm, &f, &this, a, "__last_request"),
        "SoapClient::__getLastResponse" => client::get_prop(vm, &f, &this, a, "__last_response"),
        "SoapClient::__getLastRequestHeaders" => {
            client::get_prop(vm, &f, &this, a, "__last_request_headers")
        }
        "SoapClient::__getLastResponseHeaders" => {
            client::get_prop(vm, &f, &this, a, "__last_response_headers")
        }
        "SoapClient::__doRequest" => client::do_request_method(vm, &f, &this, a),
        "SoapClient::__setCookie" => client::set_cookie(vm, &f, &this, a),
        "SoapClient::__getCookies" => client::get_prop(vm, &f, &this, a, "_cookies"),
        "SoapClient::__setSoapHeaders" => client::set_soap_headers(vm, &f, &this, a),
        "SoapClient::__setLocation" => client::set_location(vm, &f, &this, a),
        _ => Ok(Value::Null),
    }
}

fn soap_param_construct(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "zS", &[], args)?;
    let data = a[0].any().unwrap_or(Value::Null);
    let name = a[1].str().unwrap_or_default().to_vec();
    if name.is_empty() {
        return Err(f.must_not_be_empty(vm, 2));
    }
    set_prop(this, "param_name", vstr(name));
    set_prop(this, "param_data", copy_value(&data.deref()));
    Ok(Value::Null)
}

fn soap_header_construct(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "SS|zbz", &[], args)?;
    let ns = a[0].str().unwrap_or_default().to_vec();
    let name = a[1].str().unwrap_or_default().to_vec();
    let data = a.get(2).and_then(|x| x.any());
    let must_understand = a.get(3).is_some_and(|x| x.bool());
    // Z_PARAM_STR_OR_LONG_OR_NULL
    let actor = match a.get(4).and_then(|x| x.any()).map(|v| v.deref()) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Err(s.as_bytes().to_vec())),
        Some(Value::Long(n)) => Some(Ok(n)),
        Some(Value::Double(d)) if d.fract() == 0.0 && d.is_finite() => Some(Ok(d as i64)),
        Some(Value::True) => Some(Ok(1)),
        Some(Value::False) => Some(Ok(0)),
        Some(other) => return Err(f.type_error(vm, 5, "string|int|null", &other)),
    };
    if ns.is_empty() {
        return Err(f.must_not_be_empty(vm, 1));
    }
    if name.is_empty() {
        return Err(f.must_not_be_empty(vm, 2));
    }
    set_prop(this, "namespace", vstr(ns));
    set_prop(this, "name", vstr(name));
    if let Some(d) = data {
        set_prop(this, "data", copy_value(&d.deref()));
    }
    set_prop(
        this,
        "mustUnderstand",
        if must_understand {
            Value::True
        } else {
            Value::False
        },
    );
    match actor {
        Some(Err(s)) => {
            if s.len() > 2 {
                set_prop(this, "actor", vstr(s));
            } else {
                return Err(f.value_error(vm, 5, "must be longer than 2 characters"));
            }
        }
        Some(Ok(n)) => {
            if n == SOAP_ACTOR_NEXT || n == SOAP_ACTOR_NONE || n == SOAP_ACTOR_UNLIMATERECEIVER {
                set_prop(this, "actor", Value::Long(n));
            } else {
                return Err(f.value_error(
                    vm,
                    5,
                    "must be one of SOAP_ACTOR_NEXT, SOAP_ACTOR_NONE, or SOAP_ACTOR_UNLIMATERECEIVER",
                ));
            }
        }
        None => {}
    }
    Ok(Value::Null)
}

fn soap_fault_construct(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    // Z_PARAM_ARRAY_HT_OR_STR_OR_NULL(code)
    let code_arg = args.first().map(|v| v.deref());
    let mut checked = args.to_vec();
    if let Some(c) = &code_arg {
        match c {
            Value::Array(_) | Value::String(_) | Value::Null | Value::Undef => {}
            Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
                checked[0] = vstr(c.to_php_string().as_bytes());
            }
            other => {
                if !matches!(other, Value::Object(_)) || vm.value_to_string_checked(other).is_err()
                {
                    if vm.current_exception.is_some() && !matches!(other, Value::Object(_)) {
                        return Err(Bail);
                    }
                    vm.current_exception = None;
                    return Err(f.type_error(vm, 1, "array|string|null", other));
                }
                let s = vm
                    .value_to_string_checked(other)
                    .unwrap_or_else(|_| PhpString::from_bytes(b""));
                checked[0] = Value::String(s);
            }
        }
    }
    let a = zpp::parse(vm, f, "zs|s!z!S!z!P", &[], &checked)?;
    let mut fault_code: Option<String> = None;
    let mut fault_code_ns: Option<String> = None;
    let code = a[0].any().map(|v| v.deref()).unwrap_or(Value::Null);
    let has_code = !matches!(code, Value::Null);
    match &code {
        Value::String(s) => fault_code = Some(s.to_string_lossy()),
        Value::Array(ht) => {
            let ht = ht.borrow();
            if ht.len() == 2 {
                let t_ns = ht.get_int(0).map(|v| v.deref());
                let t_code = ht.get_int(1).map(|v| v.deref());
                if let (Some(Value::String(n)), Some(Value::String(c))) = (t_ns, t_code) {
                    fault_code_ns = Some(n.to_string_lossy());
                    fault_code = Some(c.to_string_lossy());
                }
            }
        }
        _ => {}
    }
    if has_code && fault_code.as_deref().is_none_or(|c| c.is_empty()) {
        return Err(f.value_error(vm, 1, "is not a valid fault code"));
    }
    let fault_string = a[1].string().unwrap_or_default();
    let actor = a.get(2).and_then(|x| x.string());
    let details = a.get(3).and_then(|x| x.any());
    let name = a.get(4).and_then(|x| x.string()).filter(|n| !n.is_empty());
    let headerfault = a.get(5).and_then(|x| x.any());
    // soap_fault_dtor_properties()
    set_prop(this, "faultstring", vstr(""));
    for p in [
        "faultcode",
        "faultcodens",
        "faultactor",
        "detail",
        "_name",
        "headerfault",
    ] {
        set_prop(this, p, Value::Null);
    }
    set_prop(this, "lang", vstr(""));
    set_soap_fault(
        vm,
        Some(this),
        fault_code_ns.as_deref(),
        fault_code.as_deref(),
        Some(&fault_string),
        actor.as_deref(),
        details.as_ref(),
        name.as_deref(),
        "en",
    );
    if let Some(h) = headerfault {
        set_prop(this, "headerfault", copy_value(&h.deref()));
    }
    Ok(Value::Null)
}

fn soap_fault_to_string(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    zpp::parse(vm, f, "", &[], args)?;
    let faultcode = prop(this, "faultcode");
    let faultstring = prop(this, "faultstring");
    let file = prop(this, "file");
    let line = prop(this, "line");
    let trace = call_method(vm, this, "getTraceAsString", &[])?;
    let fc = encoding::zval_get_string(vm, &faultcode)?;
    let fs = encoding::zval_get_string(vm, &faultstring)?;
    let fl = encoding::zval_get_string(vm, &file)?;
    let ln = line.to_long();
    let tr = encoding::zval_get_string(vm, &trace)?;
    let mut out = Vec::new();
    out.extend_from_slice(b"SoapFault exception: [");
    out.extend_from_slice(&fc);
    out.extend_from_slice(b"] ");
    out.extend_from_slice(&fs);
    out.extend_from_slice(b" in ");
    out.extend_from_slice(&fl);
    out.extend_from_slice(format!(":{}\nStack trace:\n", ln).as_bytes());
    if tr.is_empty() {
        out.extend_from_slice(b"#0 {main}\n");
    } else {
        out.extend_from_slice(&tr);
    }
    Ok(vstr(out))
}

fn soap_var_construct(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "z!l!|S!S!S!S!", &[], args)?;
    match &a[1] {
        Arg::Long(t) => {
            if encoding::def_by_type(*t).is_some() {
                set_prop(this, "enc_type", Value::Long(*t));
            } else {
                return Err(f.value_error(vm, 2, "is not a valid encoding"));
            }
        }
        _ => set_prop(this, "enc_type", Value::Long(UNKNOWN_TYPE)),
    }
    if let Some(d) = a[0].any() {
        set_prop(this, "enc_value", copy_value(&d.deref()));
    }
    for (i, p) in [
        (2, "enc_stype"),
        (3, "enc_ns"),
        (4, "enc_name"),
        (5, "enc_namens"),
    ] {
        if let Some(s) = a.get(i).and_then(|x| x.str()) {
            if !s.is_empty() {
                set_prop(this, p, vstr(s));
            }
        }
    }
    Ok(Value::Null)
}
