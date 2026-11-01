//! `SoapClient` (PHP manual: The SoapClient class): options, WSDL and
//! non-WSDL calls, request envelopes, the HTTP transport behind
//! `__doRequest()`, responses and faults, trace and cookies.

use std::rc::Rc;

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use super::decode::{Dx, from_xml};
use super::encode::{Cx, TypeMapEntry, instance_of, to_xml};
use super::sdl::{Enc, Function, Sdl};
use super::xmlin::InDoc;
use super::xmlout::{
    El, NS_SOAP11_ENC, NS_SOAP11_ENV, NS_SOAP12_ENC, NS_SOAP12_ENV, NS_SOAP12_RPC, NS_XSD,
};
use super::{
    ErrMode, SoapState, describe, make_fault, pop_mode, push_mode, soap_error, soap_php_error,
    str_prop,
};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::vm::{R, Unwind, Vm};

/// What a client keeps besides its properties.
#[derive(Default)]
pub struct ClientData {
    pub sdl: Option<Rc<Sdl>>,
    pub typemap: Rc<Vec<TypeMapEntry>>,
}

impl goro_runtime::NativeData for ClientData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        for t in self.typemap.iter() {
            if let Some(v) = &t.to_xml {
                visit(v);
            }
            if let Some(v) = &t.from_xml {
                visit(v);
            }
        }
    }
    fn clear(&mut self) {
        self.typemap = Rc::new(Vec::new());
    }
}

pub fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_vm, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(ClientData::default()));
        }),
        ..Default::default()
    }
}

fn with_data<T>(o: &ObjRef, f: impl FnOnce(&mut ClientData) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<ClientData>().map(f)
}

fn sdl_of(o: &ObjRef) -> Option<Rc<Sdl>> {
    with_data(o, |d| d.sdl.clone()).flatten()
}

fn opt<'a>(opts: &'a PhpArray, k: &str) -> Option<Value> {
    opts.get_bytes(k.as_bytes()).map(|v| v.deref())
}

fn int_prop(vm: &Vm, o: &ObjRef, name: &str) -> Option<i64> {
    match vm.get_prop_raw(o, name.as_bytes()) {
        Value::Int(n) => Some(n),
        _ => None,
    }
}

fn bool_prop(vm: &Vm, o: &ObjRef, name: &str) -> bool {
    matches!(vm.get_prop_raw(o, name.as_bytes()), Value::Bool(true))
}

/// The encoding state from a client's options.
pub fn state_of(vm: &mut Vm, o: &ObjRef) -> SoapState {
    let features = int_prop(vm, o, "_features").unwrap_or(0);
    let encoding = str_prop(vm, o, "_encoding");
    let classmap = classmap_of(vm.get_prop_raw(o, b"_classmap"));
    let typemap = with_data(o, |d| d.typemap.clone()).unwrap_or_default();
    SoapState {
        features,
        encoding,
        classmap,
        typemap,
    }
}

/// A classmap maps type names (string keys) to class names (observed on
/// PHP 8.5; test bugs/gh16256.phpt).
pub fn classmap_valid(a: &PhpArray) -> bool {
    a.iter()
        .all(|(k, _)| matches!(k, goro_runtime::KeyRef::Str(_)))
}

pub fn classmap_of(v: Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Value::Array(a) = v {
        for (k, v) in a.iter() {
            if let Value::Str(c) = v.deref() {
                out.push((
                    super::encode::key_str(k).to_string_lossy(),
                    c.to_string_lossy(),
                ));
            }
        }
    }
    out
}

/// Parses the `typemap` option.
pub fn parse_typemap(vm: &mut Vm, v: &Value) -> R<Vec<TypeMapEntry>> {
    let mut out = Vec::new();
    let Value::Array(a) = v else {
        return Ok(out);
    };
    for (_, e) in a.iter() {
        let Value::Array(e) = e.deref() else {
            continue;
        };
        let get = |k: &str| e.get_bytes(k.as_bytes()).map(|v| v.deref());
        let name = match get("type_name") {
            Some(Value::Str(s)) => s.to_string_lossy(),
            _ => continue,
        };
        let ns = match get("type_ns") {
            Some(Value::Str(s)) => s.to_string_lossy(),
            _ => String::new(),
        };
        let to_xml = get("to_xml").filter(|v| !matches!(v, Value::Null));
        let from_xml = get("from_xml").filter(|v| !matches!(v, Value::Null));
        out.push(TypeMapEntry {
            ns,
            name,
            to_xml,
            from_xml,
        });
    }
    let _ = vm;
    Ok(out)
}

/// Loads a WSDL through PHP's streams (local files directly).
pub fn load_sdl(vm: &mut Vm, a: &Args, url: &str) -> Result<Sdl, String> {
    let mut loader = |u: &str| -> Result<Vec<u8>, String> { fetch(vm, a, u) };
    Sdl::load(url, &mut loader)
}

fn fetch(vm: &mut Vm, a: &Args, u: &str) -> Result<Vec<u8>, String> {
    let is_url = u.contains("://");
    if !is_url || u.starts_with("file://") {
        let p = u.strip_prefix("file://").unwrap_or(u);
        let path = vm.fs_path(p.as_bytes());
        return std::fs::read(&path)
            .map_err(|_| format!("failed to load \"{u}\": No such file or directory\n"));
    }
    match super::super::file::open_stream(vm, a, u.as_bytes(), b"rb", true) {
        Ok(Some(mut s)) => Ok(super::super::file::read_rest(vm, &mut s)),
        _ => Err(format!("failed to load \"{u}\": No such file or directory\n")),
    }
}

// ------------------------------------------------------------ construct

pub fn construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let wsdl = a.opt_str(0).map(|s| s.to_string_lossy());
    let has_opts = a.has(1);
    let opts = a.array(1);
    let exceptions = match opt(&opts, "exceptions") {
        Some(v) => vm.to_bool(&v),
        None => true,
    };
    push_mode(if exceptions {
        ErrMode::Throw("Client")
    } else {
        ErrMode::Fatal
    });
    let r = construct_inner(vm, a, &this, wsdl, has_opts, &opts, exceptions);
    pop_mode();
    r
}

fn construct_inner(
    vm: &mut Vm,
    a: &Args,
    this: &ObjRef,
    wsdl: Option<String>,
    has_opts: bool,
    opts: &PhpArray,
    exceptions: bool,
) -> R<Value> {
    let set = |vm: &mut Vm, n: &str, v: Value| vm.set_prop_raw(this, n.as_bytes(), v);
    if wsdl.is_none() {
        if !has_opts {
            return Err(soap_php_error(
                vm,
                a,
                "'location' and 'uri' options are required in nonWSDL mode",
            ));
        }
        let uri = opt(opts, "uri");
        if !matches!(uri, Some(Value::Str(_))) {
            return Err(soap_php_error(vm, a, "'uri' option is required in nonWSDL mode"));
        }
        if !matches!(opt(opts, "location"), Some(Value::Str(_))) {
            return Err(soap_php_error(
                vm,
                a,
                "'location' option is required in nonWSDL mode",
            ));
        }
    }
    if let Some(Value::Str(u)) = opt(opts, "uri") {
        set(vm, "uri", Value::Str(u));
    }
    if wsdl.is_none() {
        if let Some(v) = opt(opts, "style") {
            let n = vm.to_int(&v)?;
            set(vm, "style", Value::Int(n));
        }
        if let Some(v) = opt(opts, "use") {
            let n = vm.to_int(&v)?;
            set(vm, "use", Value::Int(n));
        }
    }
    if let Some(Value::Str(l)) = opt(opts, "location") {
        set(vm, "location", Value::Str(l));
    }
    if let Some(v) = opt(opts, "soap_version") {
        let n = vm.to_int(&v)?;
        set(vm, "_soap_version", Value::Int(if n == 2 { 2 } else { 1 }));
    } else {
        set(vm, "_soap_version", Value::Int(1));
    }
    for (k, p) in [
        ("login", "_login"),
        ("password", "_password"),
        ("proxy_host", "_proxy_host"),
        ("proxy_login", "_proxy_login"),
        ("proxy_password", "_proxy_password"),
        ("user_agent", "_user_agent"),
    ] {
        if let Some(Value::Str(s)) = opt(opts, k) {
            set(vm, p, Value::Str(s));
        }
    }
    if let Some(v) = opt(opts, "authentication") {
        let n = vm.to_int(&v)?;
        if n == 1 {
            set(vm, "_use_digest", Value::Bool(true));
        }
    }
    if let Some(v) = opt(opts, "proxy_port") {
        let n = vm.to_int(&v)?;
        set(vm, "_proxy_port", Value::Int(n));
    }
    if let Some(v) = opt(opts, "trace") {
        let b = vm.to_bool(&v);
        set(vm, "trace", Value::Bool(b));
    }
    if let Some(v) = opt(opts, "compression") {
        let n = vm.to_int(&v)?;
        set(vm, "compression", Value::Int(n));
    }
    if let Some(v) = opt(opts, "encoding") {
        let s = vm.to_php_str(&v)?.to_string_lossy();
        if !super::valid_encoding(&s) {
            return Err(soap_php_error(
                vm,
                a,
                &format!("Invalid 'encoding' option - '{s}'"),
            ));
        }
        set(vm, "_encoding", Value::Str(PhpStr::from(s)));
    }
    set(vm, "_exceptions", Value::Bool(exceptions));
    if let Some(Value::Array(cm)) = opt(opts, "classmap") {
        if !classmap_valid(&cm) {
            return Err(soap_php_error(
                vm,
                a,
                "'classmap' option must be an associative array",
            ));
        }
        set(vm, "_classmap", Value::Array(cm));
    }
    if let Some(v) = opt(opts, "features") {
        let n = vm.to_int(&v)?;
        set(vm, "_features", Value::Int(n));
    }
    if let Some(v) = opt(opts, "connection_timeout") {
        let n = vm.to_int(&v)?;
        if n > 0 {
            set(vm, "_connection_timeout", Value::Int(n));
        }
    }
    if let Some(v) = opt(opts, "keep_alive") {
        let b = vm.to_bool(&v);
        set(vm, "_keep_alive", Value::Bool(b));
    }
    if let Some(v) = opt(opts, "ssl_method") {
        vm.fdeprecated(
            a,
            "The \"ssl_method\" option is deprecated. Use \"ssl\" stream context options instead",
        )?;
        let n = vm.to_int(&v)?;
        set(vm, "_ssl_method", Value::Int(n));
    }
    let ctx = match opt(opts, "stream_context") {
        Some(v @ Value::Resource(_)) => v,
        _ => super::super::sockets::new_context(PhpArray::new()),
    };
    set(vm, "_stream_context", ctx);
    let tm = match opt(opts, "typemap") {
        Some(v @ Value::Array(_)) => parse_typemap(vm, &v)?,
        _ => Vec::new(),
    };
    if !tm.is_empty() {
        let mut arr = PhpArray::new();
        for t in &tm {
            arr.insert(
                Key::from(format!("{}:{}", t.ns, t.name).as_str()),
                Value::Null,
            );
        }
        set(vm, "typemap", Value::Array(arr));
    }
    with_data(this, |d| d.typemap = Rc::new(tm));
    if let Some(w) = wsdl {
        let sdl = match load_sdl(vm, a, &w) {
            Ok(s) => s,
            Err(msg) => {
                if exceptions {
                    let f = make_fault(vm, "WSDL", &msg)?;
                    return Err(Unwind::Throw(f));
                }
                return Err(vm.fatal(&msg));
            }
        };
        let cls = vm.find_class(b"Soap\\Sdl", false)?.expect("Soap\\Sdl");
        let so = vm.instantiate_raw(&cls)?;
        set(vm, "sdl", Value::Object(so));
        let sdl = Rc::new(sdl);
        with_data(this, |d| d.sdl = Some(sdl));
    }
    Ok(Value::Null)
}

// ------------------------------------------------------------ calls

pub fn call(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let name = a.str(0).to_string_lossy();
    let args = a.array(1);
    let args: Vec<Value> = args.iter().map(|(_, v)| v.deref()).collect();
    do_call(vm, a, &this, &name, args, PhpArray::new(), Value::Null, None)
}

pub fn soap_call(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let name = a.str(0).to_string_lossy();
    let args = a.array(1);
    let args: Vec<Value> = args.iter().map(|(_, v)| v.deref()).collect();
    let options = a.array(2);
    let headers = a.val(3);
    let out = a.ref_at(4);
    do_call(vm, a, &this, &name, args, options, headers, out)
}

/// The SoapHeader objects of an input headers argument.
fn header_list(vm: &mut Vm, a: &Args, v: &Value) -> R<Vec<ObjRef>> {
    let mut out = Vec::new();
    match v {
        Value::Null => {}
        Value::Object(o) if instance_of(vm, o, "SoapHeader") => out.push(o.clone()),
        Value::Array(arr) => {
            for (_, h) in arr.iter() {
                match h.deref() {
                    Value::Object(o) if instance_of(vm, &o, "SoapHeader") => out.push(o),
                    _ => {
                        let m = format!("{}(): Invalid SOAP header", a.fname());
                        return Err(vm.fatal(&m));
                    }
                }
            }
        }
        _ => {
            let m = format!("{}(): Invalid SOAP header", a.fname());
            return Err(vm.fatal(&m));
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn do_call(
    vm: &mut Vm,
    a: &Args,
    this: &ObjRef,
    name: &str,
    args: Vec<Value>,
    options: PhpArray,
    in_headers: Value,
    out_headers: Option<goro_runtime::PhpRef>,
) -> R<Value> {
    let exceptions = match vm.get_prop_raw(this, b"_exceptions") {
        Value::Bool(b) => b,
        _ => true,
    };
    let mut headers = header_list(vm, a, &in_headers)?;
    let mut force_header = matches!(in_headers, Value::Array(_));
    if let Value::Array(def) = vm.get_prop_raw(this, b"__default_headers") {
        force_header = true;
        for (_, h) in def.iter() {
            if let Value::Object(o) = h.deref() {
                headers.push(o);
            }
        }
    }
    if let Some(r) = &out_headers {
        r.set(Value::Array(PhpArray::new()));
    }
    vm.set_prop_raw(this, b"__soap_fault", Value::Null);
    push_mode(if exceptions {
        ErrMode::Throw("Client")
    } else {
        ErrMode::Fatal
    });
    let r = call_inner(
        vm,
        this,
        name,
        args,
        &options,
        &headers,
        out_headers.as_ref(),
        force_header,
    );
    pop_mode();
    let r = match r {
        Ok(v) => v,
        Err(Unwind::Throw(f)) if !exceptions && super::is_fault_obj(vm, &Value::Object(f.clone())) => {
            Value::Object(f)
        }
        Err(e) => return Err(e),
    };
    if super::is_fault_obj(vm, &r) {
        vm.set_prop_raw(this, b"__soap_fault", r.clone());
        if exceptions && let Value::Object(f) = r {
            return Err(Unwind::Throw(f));
        }
    }
    Ok(r)
}

#[allow(clippy::too_many_arguments)]
fn call_inner(
    vm: &mut Vm,
    this: &ObjRef,
    name: &str,
    args: Vec<Value>,
    options: &PhpArray,
    headers: &[ObjRef],
    out_headers: Option<&goro_runtime::PhpRef>,
    force_header: bool,
) -> R<Value> {
    let sdl = sdl_of(this);
    let version = int_prop(vm, this, "_soap_version").unwrap_or(1);
    let st = state_of(vm, this);
    let trace = bool_prop(vm, this, "trace");
    let mut location = match opt(options, "location") {
        Some(Value::Str(s)) => Some(s.to_string_lossy()),
        _ => str_prop(vm, this, "location"),
    };
    let mut cx = Cx::new(sdl.clone(), version, &st);
    cx.force_header = force_header;
    let (request, action, func) = match &sdl {
        Some(s) => {
            let Some(f) = s.find_function(name) else {
                let m = format!("Function (\"{name}\") is not a valid method for this service");
                return Ok(Value::Object(make_fault(vm, "Client", &m)?));
            };
            let f = f.clone();
            if location.is_none() {
                location = Some(f.location.clone());
            }
            let action = match opt(options, "soapaction") {
                Some(Value::Str(s)) => s.to_string_lossy(),
                _ => f.soap_action.clone().unwrap_or_default(),
            };
            let req = wsdl_request(vm, &mut cx, s, &f, &args, headers)?;
            (req, action, Some(f))
        }
        None => {
            let uri = match opt(options, "uri") {
                Some(Value::Str(s)) => s.to_string_lossy(),
                _ => str_prop(vm, this, "uri").unwrap_or_default(),
            };
            let action = match opt(options, "soapaction") {
                Some(Value::Str(s)) => s.to_string_lossy(),
                _ => format!("{uri}#{name}"),
            };
            let style = int_prop(vm, this, "style").unwrap_or(1);
            let use_ = int_prop(vm, this, "use").unwrap_or(1);
            let req = plain_request(vm, &mut cx, name, &uri, &args, headers, style != 2, use_ == 2)?;
            (req, action, None)
        }
    };
    if trace {
        vm.set_prop_raw(this, b"__last_request", Value::Str(PhpStr::from(request.clone())));
    }
    let one_way = func.as_ref().is_some_and(|f| f.output.is_none());
    let location = location.unwrap_or_default();
    let resp = vm.call_method(
        this,
        b"__dorequest",
        vec![
            Value::Str(PhpStr::from(request)),
            Value::Str(PhpStr::from(location)),
            Value::Str(PhpStr::from(action)),
            Value::Int(cx.version),
            Value::Bool(one_way),
        ],
    )?;
    // A fault set by the transport.
    if let Value::Object(f) = vm.get_prop_raw(this, b"__soap_fault") {
        return Ok(Value::Object(f));
    }
    let resp = match resp {
        Value::Str(s) => s,
        Value::Null if one_way => return Ok(Value::Null),
        _ => {
            return Ok(Value::Object(make_fault(
                vm,
                "Client",
                "SoapClient::__doRequest() returned non string value",
            )?));
        }
    };
    if trace {
        vm.set_prop_raw(this, b"__last_response", Value::Str(resp.clone()));
    }
    // An empty response is no result (observed on PHP 8.5; tests
    // bugs/bug71711.phpt, bug77410.phpt).
    if resp.is_empty() {
        return Ok(Value::Null);
    }
    parse_response(vm, sdl, func.as_ref(), &st, resp.as_bytes(), out_headers)
}

/// The Envelope (and its prefix) of a new message.
fn envelope(cx: &mut Cx) -> (El, &'static str) {
    let (name, prefix, ns) = if cx.version == 2 {
        ("env:Envelope", "env", NS_SOAP12_ENV)
    } else {
        ("SOAP-ENV:Envelope", "SOAP-ENV", NS_SOAP11_ENV)
    };
    let env = cx.doc.new_el(name);
    cx.doc.root = Some(env);
    cx.doc.declare(env, prefix, ns);
    (env, prefix)
}

/// Finishes an envelope: SOAP-ENC/xsd declarations and the encoding
/// style for encoded messages (observed on PHP 8.5).
fn finish(cx: &mut Cx, env: El, method: Option<El>, encoded: bool) {
    if !encoded {
        return;
    }
    cx.doc.ensure_ns(Some(env), NS_XSD);
    if cx.version == 2 {
        cx.doc.ensure_ns(Some(env), NS_SOAP12_ENC);
        if let Some(m) = method {
            cx.doc.set_attr(m, "env:encodingStyle", NS_SOAP12_ENC);
        }
    } else {
        cx.doc.ensure_ns(Some(env), NS_SOAP11_ENC);
        cx.doc.set_attr(env, "SOAP-ENV:encodingStyle", NS_SOAP11_ENC);
    }
}

/// Writes SoapHeader objects into a Header element.
pub fn write_headers(
    vm: &mut Vm,
    cx: &mut Cx,
    env: El,
    prefix: &str,
    headers: &[ObjRef],
    sdl_headers: &[super::sdl::Header],
) -> R<()> {
    if headers.is_empty() && !cx.force_header {
        return Ok(());
    }
    let h = cx.doc.new_el(&format!("{prefix}:Header"));
    cx.doc.els[h].parent = Some(env);
    cx.doc.els[env].kids.insert(0, super::xmlout::Kid::El(h));
    for o in headers {
        let ns = str_prop(vm, o, "namespace").unwrap_or_default();
        let name = str_prop(vm, o, "name").unwrap_or_default();
        let data = vm.get_prop_raw(o, b"data");
        let el = cx.doc.add_child(h, &name);
        // A header the WSDL declares is encoded as declared.
        let decl = sdl_headers.iter().find(|d| d.name == name && d.ns == ns);
        let (enc, literal) = match (decl, cx.sdl.clone()) {
            (Some(d), Some(s)) => {
                let enc = match &d.part.element {
                    Some((ens, en)) => s.element(ens, en).map(Enc::Type),
                    None => d.part.typ.as_ref().and_then(|t| s.resolve(t)),
                };
                (enc, d.literal)
            }
            _ => (None, true),
        };
        if !matches!(data, Value::Null) {
            to_xml(vm, cx, enc, &data, el, literal)?;
        }
        // The prefix after the content (observed on PHP 8.5; tests
        // interop/Round4/GroupI/r4_groupI_xsd_03*w.phpt).
        let q = cx.doc.qname(Some(el), &ns, &name);
        cx.doc.rename(el, &q);
        if bool_prop(vm, o, "mustUnderstand") {
            let a = format!("{prefix}:mustUnderstand");
            cx.doc.set_attr(el, &a, if cx.version == 2 { "true" } else { "1" });
        }
        match vm.get_prop_raw(o, b"actor") {
            Value::Str(s) => {
                let a = if cx.version == 2 {
                    format!("{prefix}:role")
                } else {
                    format!("{prefix}:actor")
                };
                cx.doc.set_attr(el, &a, &s.to_string_lossy());
            }
            Value::Int(n) => {
                let (a, v) = if cx.version == 2 {
                    (
                        format!("{prefix}:role"),
                        match n {
                            1 => format!("{NS_SOAP12_ENV}/role/next"),
                            2 => format!("{NS_SOAP12_ENV}/role/none"),
                            _ => format!("{NS_SOAP12_ENV}/role/ultimateReceiver"),
                        },
                    )
                } else {
                    (
                        format!("{prefix}:actor"),
                        "http://schemas.xmlsoap.org/soap/actor/next".to_string(),
                    )
                };
                if !(cx.version == 1 && n != 1) {
                    cx.doc.set_attr(el, &a, &v);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// A non-WSDL request.
#[allow(clippy::too_many_arguments)]
fn plain_request(
    vm: &mut Vm,
    cx: &mut Cx,
    name: &str,
    uri: &str,
    args: &[Value],
    headers: &[ObjRef],
    rpc: bool,
    literal: bool,
) -> R<String> {
    let (env, prefix) = envelope(cx);
    let body = cx.doc.add_child(env, &format!("{prefix}:Body"));
    let method = if rpc {
        let q = cx.doc.qname(Some(body), uri, name);
        Some(cx.doc.add_child(body, &q))
    } else {
        None
    };
    let parent = method.unwrap_or(body);
    for (i, v) in args.iter().enumerate() {
        let (pname, val) = match v {
            Value::Object(o) if instance_of(vm, o, "SoapParam") => (
                str_prop(vm, o, "param_name").unwrap_or_default(),
                vm.get_prop_raw(o, b"param_data"),
            ),
            _ => (format!("param{i}"), v.clone()),
        };
        let el = cx.doc.add_child(parent, &pname);
        to_xml(vm, cx, None, &val, el, literal)?;
    }
    write_headers(vm, cx, env, prefix, headers, &[])?;
    finish(cx, env, method, !literal);
    Ok(cx.doc.serialize(env))
}

/// A request for WSDL operation `f`.
fn wsdl_request(
    vm: &mut Vm,
    cx: &mut Cx,
    sdl: &Rc<Sdl>,
    f: &Function,
    args: &[Value],
    headers: &[ObjRef],
) -> R<String> {
    // The soap_version option wins over the binding's (observed on PHP
    // 8.5; test interop/Round4/GroupH/r4_groupH_soapfault_002w.phpt).
    let v = if cx.version == 2 { 2 } else { f.version };
    cx.version = v;
    cx.doc.version = v;
    let (env, prefix) = envelope(cx);
    let body = cx.doc.add_child(env, &format!("{prefix}:Body"));
    let literal = f.input.literal;
    let method = if f.rpc {
        // Without a namespace the call element gets the envelope's prefix
        // (observed on PHP 8.5; test bugs/bug32776.phpt).
        let q = if f.input.ns.is_empty() {
            format!("{prefix}:{}", f.name)
        } else {
            cx.doc.qname(Some(body), &f.input.ns, &f.name)
        };
        Some(cx.doc.add_child(body, &q))
    } else {
        None
    };
    let parent = method.unwrap_or(body);
    // Arguments by position (SoapParam arguments by name).
    for (i, p) in f.input.parts.iter().enumerate() {
        let mut val = args.get(i).cloned();
        for a in args {
            if let Value::Object(o) = a
                && instance_of(vm, o, "SoapParam")
                && str_prop(vm, o, "param_name").as_deref() == Some(&p.name)
            {
                val = Some(vm.get_prop_raw(o, b"param_data"));
            }
        }
        if let Some(Value::Object(o)) = &val
            && instance_of(vm, o, "SoapParam")
        {
            val = Some(vm.get_prop_raw(o, b"param_data"));
        }
        // A missing argument takes its element's default (observed on PHP
        // 8.5; test bugs/bug34643.phpt).
        let default = p
            .element
            .as_ref()
            .and_then(|(ns, n)| sdl.element(ns, n))
            .and_then(|e| {
                let t = &sdl.types[sdl.deref_element(e)];
                t.default.clone().or(t.fixed.clone())
            });
        let val = match (val, default) {
            (Some(v), _) => Some(v),
            (None, Some(d)) => Some(Value::Str(PhpStr::from(d))),
            (None, None) => None,
        };
        let val = match val {
            Some(v) => v,
            // Document parts are written even without an argument
            // (observed on PHP 8.5; test
            // interop/Round3/GroupD/r3_groupD_doclitparams_004w.phpt).
            None if !f.rpc => Value::Null,
            None => continue,
        };
        part_to_xml(vm, cx, sdl, p, &val, parent, literal, f.rpc)?;
    }
    write_headers(vm, cx, env, prefix, headers, &f.input.headers)?;
    finish(cx, env, method, !literal);
    Ok(cx.doc.serialize(env))
}

/// Writes one message part.
#[allow(clippy::too_many_arguments)]
pub fn part_to_xml(
    vm: &mut Vm,
    cx: &mut Cx,
    sdl: &Rc<Sdl>,
    p: &super::sdl::Part,
    val: &Value,
    parent: El,
    literal: bool,
    rpc: bool,
) -> R<El> {
    if let Some((ens, en)) = &p.element {
        let decl = sdl.element(ens, en);
        let el = if rpc {
            cx.doc.add_child(parent, &p.name)
        } else {
            // The element is created before its namespace is needed
            // (observed on PHP 8.5; test schema/schema072.phpt).
            cx.doc.add_child(parent, en)
        };
        let enc = decl.map(Enc::Type);
        to_xml(vm, cx, enc, val, el, literal)?;
        if !rpc && !ens.is_empty() {
            let q = cx.doc.qname(Some(el), ens, en);
            cx.doc.rename(el, &q);
        }
        Ok(el)
    } else {
        let el = cx.doc.add_child(parent, &p.name);
        let enc = p.typ.as_ref().and_then(|t| sdl.resolve(t));
        to_xml(vm, cx, enc, val, el, literal)?;
        Ok(el)
    }
}

// ------------------------------------------------------------ responses

/// Parses a response envelope: the result value or a SoapFault.
pub fn parse_response(
    vm: &mut Vm,
    sdl: Option<Rc<Sdl>>,
    func: Option<&Function>,
    st: &SoapState,
    resp: &[u8],
    out_headers: Option<&goro_runtime::PhpRef>,
) -> R<Value> {
    let doc = match InDoc::parse(resp, None) {
        Ok(d) => d,
        Err(_) => {
            return Ok(Value::Object(make_fault(
                vm,
                "Client",
                "looks like we got no XML document",
            )?));
        }
    };
    let Some(env) = doc.root() else {
        return Ok(Value::Object(make_fault(
            vm,
            "Client",
            "looks like we got no XML document",
        )?));
    };
    let env_ns = doc.ns(env).to_string();
    let version = if env_ns == NS_SOAP12_ENV {
        2
    } else if env_ns == NS_SOAP11_ENV {
        1
    } else {
        return Ok(Value::Object(make_fault(
            vm,
            "VersionMismatch",
            "Wrong Version",
        )?));
    };
    if doc.local(env) != "Envelope" {
        return Ok(Value::Object(make_fault(
            vm,
            "Client",
            "looks like we got XML without \"Envelope\" element",
        )?));
    }
    let mut header = None;
    let mut body = None;
    for c in doc.kids(env) {
        if doc.ns(c) != env_ns {
            continue;
        }
        match doc.local(c) {
            "Header" if header.is_none() && body.is_none() => header = Some(c),
            "Body" if body.is_none() => body = Some(c),
            _ => {}
        }
    }
    let Some(body) = body else {
        return Ok(Value::Object(make_fault(
            vm,
            "Client",
            "Body must be present in a SOAP envelope",
        )?));
    };
    let mut dx = Dx::new(&doc, sdl.clone(), version, st);
    // Fault?
    let first = doc.kids(body).into_iter().next();
    if let Some(fe) = first
        && doc.ns(fe) == env_ns
        && doc.local(fe) == "Fault"
    {
        return parse_fault(vm, &mut dx, fe, version, func);
    }
    // Output headers.
    if let (Some(h), Some(out)) = (header, out_headers) {
        let mut arr = PhpArray::new();
        for c in doc.kids(h) {
            let name = doc.local(c).to_string();
            let mut enc = None;
            if let (Some(f), Some(s)) = (func, &sdl)
                && let Some(o) = &f.output
                && let Some(hd) = o
                    .headers
                    .iter()
                    .find(|hd| hd.name == name && hd.ns == doc.ns(c))
            {
                enc = match &hd.part.element {
                    Some((ens, en)) => s.element(ens, en).map(Enc::Type),
                    None => hd.part.typ.as_ref().and_then(|t| s.resolve(t)),
                };
            }
            let v = from_xml(vm, &mut dx, enc, c)?;
            arr.insert(Key::from(name.as_str()), v);
        }
        out.set(Value::Array(arr));
    }
    let Some(f) = func else {
        // Non-WSDL: the response element's children.
        let Some(resp) = first else {
            return Ok(Value::Null);
        };
        return children_result(vm, &mut dx, resp, version);
    };
    let Some(out) = &f.output else {
        return Ok(Value::Null);
    };
    let s = sdl.expect("WSDL");
    let container = if f.rpc {
        match first {
            Some(r) => r,
            None => return Ok(Value::Null),
        }
    } else {
        body
    };
    let kids = doc.kids(container);
    let mut results: Vec<(String, Value)> = Vec::new();
    for (i, p) in out.parts.iter().enumerate() {
        let (want, enc) = match &p.element {
            Some((ens, en)) => {
                let decl = s.element(ens, en);
                (en.clone(), decl.map(Enc::Type))
            }
            None => (p.name.clone(), p.typ.as_ref().and_then(|t| s.resolve(t))),
        };
        let node = kids
            .iter()
            .copied()
            .find(|&c| doc.local(c) == want)
            .or_else(|| {
                if f.rpc && out.parts.len() == 1 {
                    // A single part: the first element child, whatever
                    // its name (SOAP 1.2 rpc:result names it).
                    kids.iter()
                        .copied()
                        .find(|&c| !(doc.ns(c) == NS_SOAP12_RPC && doc.local(c) == "result"))
                } else if !f.rpc {
                    kids.get(i).copied()
                } else {
                    None
                }
            });
        let v = match node {
            Some(n) => from_xml(vm, &mut dx, enc, n)?,
            None => Value::Null,
        };
        results.push((p.name.clone(), v));
    }
    Ok(match results.len() {
        0 => Value::Null,
        1 => results.pop().map(|r| r.1).unwrap_or(Value::Null),
        _ => {
            let mut a = PhpArray::new();
            for (k, v) in results {
                a.insert(Key::from(k.as_str()), v);
            }
            Value::Array(a)
        }
    })
}

/// The result of a non-WSDL response element: one child gives its value,
/// several an array by name (repeated names collected).
fn children_result(vm: &mut Vm, dx: &mut Dx, resp: goro_xml::NodeId, version: i64) -> R<Value> {
    let d = dx.d;
    let kids: Vec<_> = d
        .kids(resp)
        .into_iter()
        .filter(|&c| !(version == 2 && d.ns(c) == NS_SOAP12_RPC && d.local(c) == "result"))
        .collect();
    match kids.len() {
        0 => Ok(Value::Null),
        1 => from_xml(vm, dx, None, kids[0]),
        _ => {
            let mut a = PhpArray::new();
            let mut multi: Vec<String> = Vec::new();
            for c in kids {
                let name = d.local(c).to_string();
                let v = from_xml(vm, dx, None, c)?;
                let k = Key::from(name.as_str());
                match a.get(k.as_ref()).cloned() {
                    Some(cur) => {
                        let mut list = if multi.contains(&name) {
                            match cur {
                                Value::Array(l) => l,
                                _ => PhpArray::new(),
                            }
                        } else {
                            multi.push(name.clone());
                            let mut l = PhpArray::new();
                            let _ = l.push(cur);
                            l
                        };
                        let _ = list.push(v);
                        a.insert(k, Value::Array(list));
                    }
                    None => {
                        a.insert(k, v);
                    }
                }
            }
            Ok(Value::Array(a))
        }
    }
}

/// A Fault element as a SoapFault.
fn parse_fault(
    vm: &mut Vm,
    dx: &mut Dx,
    fe: goro_xml::NodeId,
    version: i64,
    func: Option<&Function>,
) -> R<Value> {
    let d = dx.d;
    let mut code = None;
    let mut string = String::new();
    let mut raw_string: Option<PhpStr> = None;
    let mut actor = None;
    let mut detail = Value::Null;
    let mut lang = String::new();
    for c in d.kids(fe) {
        let local = d.local(c).to_string();
        if version == 1 {
            match local.as_str() {
                "faultcode" => {
                    let t = d.text(c).trim().to_string();
                    code = Some(t);
                }
                "faultstring" => {
                    // Decoded as an xsd:string (typemaps apply; test
                    // bugs/bug66049.phpt).
                    let enc = super::types::by_code(super::types::XSD_STRING)
                        .map(super::sdl::Enc::Builtin);
                    let v = from_xml(vm, dx, enc, c)?;
                    raw_string = Some(vm.to_php_str(&v)?);
                    string = raw_string.as_ref().map(|s| s.to_string_lossy()).unwrap_or_default();
                }
                "faultactor" => actor = Some(d.text(c)),
                "detail" => detail = fault_detail(vm, dx, c, func)?,
                _ => {}
            }
        } else {
            match local.as_str() {
                "Code" => {
                    for v in d.kids(c) {
                        if d.local(v) == "Value" {
                            code = Some(d.text(v).trim().to_string());
                        }
                    }
                }
                "Reason" => {
                    for t in d.kids(c) {
                        if d.local(t) == "Text" {
                            let enc = super::types::by_code(super::types::XSD_STRING)
                                .map(super::sdl::Enc::Builtin);
                            let v = from_xml(vm, dx, enc, t)?;
                            raw_string = Some(vm.to_php_str(&v)?);
                            string = d.text(t);
                            if let Some(l) = d.attr_ns(t, super::xmlout::NS_XML, "lang") {
                                lang = l;
                            }
                            break;
                        }
                    }
                }
                "Role" | "Node" => actor = Some(d.text(c)),
                "Detail" => detail = fault_detail(vm, dx, c, func)?,
                _ => {}
            }
        }
    }
    let _ = lang;
    // SOAP 1.1 codes keep their prefix (observed on PHP 8.5: "SOAP-ENV:
    // Server"; test bugs/bug66049.phpt); SOAP 1.2 values lose it.
    // (SOAP 1.2 too: "[env:Receiver]"; test bugs/bug51561.phpt.)
    let (ns, code): (Option<String>, String) = match code {
        Some(c) => (None, c),
        None => (None, String::new()),
    };
    let cls = vm.find_class(b"SoapFault", false)?.expect("SoapFault");
    let o = vm.instantiate(&cls)?;
    super::init_fault(
        vm,
        &o,
        Some((ns, code)),
        &string,
        actor,
        detail,
        None,
        Value::Null,
        "",
    );
    if let Some(r) = raw_string {
        super::set_fault_string(vm, &o, r);
    }
    Ok(Value::Object(o))
}

/// A fault's detail: decoded by the WSDL's fault parts or generically.
fn fault_detail(
    vm: &mut Vm,
    dx: &mut Dx,
    det: goro_xml::NodeId,
    func: Option<&Function>,
) -> R<Value> {
    let d = dx.d;
    let kids = d.kids(det);
    if let (Some(f), Some(s)) = (func, dx.sdl.clone())
        && let Some(first) = kids.first()
    {
        for fd in &f.faults {
            if let Some(p) = fd.parts.first() {
                let matches = match &p.element {
                    Some((_, en)) => en == d.local(*first),
                    None => p.name == d.local(*first),
                };
                if matches {
                    let enc = match &p.element {
                        Some((ens, en)) => s.element(ens, en).map(Enc::Type),
                        None => p.typ.as_ref().and_then(|t| s.resolve(t)),
                    };
                    let v = from_xml(vm, dx, enc, *first)?;
                    let o = vm.new_std_object()?;
                    vm.set_prop_raw(&o, d.local(*first).as_bytes(), v);
                    return Ok(Value::Object(o));
                }
            }
        }
    }
    if kids.is_empty() {
        let t = d.text(det);
        if t.is_empty() {
            return Ok(Value::Null);
        }
        return Ok(Value::Str(PhpStr::from(t)));
    }
    from_xml(vm, dx, None, det)
}

// ------------------------------------------------------------ __doRequest

pub fn do_request(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let request = a.str(0);
    let location = a.str(1).to_string_lossy();
    let action = a.str(2).to_string_lossy();
    let version = a.int_or(3, 1);
    let one_way = a.bool_or(4, false);
    match super::http::send(vm, a, &this, request.as_bytes(), &location, &action, version, one_way)? {
        Ok(resp) => Ok(Value::Str(PhpStr::from(resp))),
        Err((code, msg)) => {
            let f = make_fault(vm, code, &msg)?;
            vm.set_prop_raw(&this, b"__soap_fault", Value::Object(f));
            Ok(Value::Null)
        }
    }
}

// ------------------------------------------------------------ accessors

pub fn get_functions(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let Some(sdl) = sdl_of(&this) else {
        return Ok(Value::Null);
    };
    let mut out = PhpArray::new();
    for f in &sdl.functions {
        let _ = out.push(Value::Str(PhpStr::from(describe::function(&sdl, f))));
    }
    Ok(Value::Array(out))
}

pub fn get_types(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let Some(sdl) = sdl_of(&this) else {
        return Ok(Value::Null);
    };
    let mut out = PhpArray::new();
    for &t in &sdl.listed {
        let _ = out.push(Value::Str(PhpStr::from(describe::type_decl(&sdl, t))));
    }
    Ok(Value::Array(out))
}

fn prop_or_null(vm: &mut Vm, a: &Args, name: &[u8]) -> R<Value> {
    let this = a.this_obj().clone();
    Ok(match vm.get_prop_raw(&this, name) {
        v @ Value::Str(_) => v,
        _ => Value::Null,
    })
}

pub fn get_last_request(vm: &mut Vm, a: &mut Args) -> R<Value> {
    prop_or_null(vm, a, b"__last_request")
}

pub fn get_last_response(vm: &mut Vm, a: &mut Args) -> R<Value> {
    prop_or_null(vm, a, b"__last_response")
}

pub fn get_last_request_headers(vm: &mut Vm, a: &mut Args) -> R<Value> {
    prop_or_null(vm, a, b"__last_request_headers")
}

pub fn get_last_response_headers(vm: &mut Vm, a: &mut Args) -> R<Value> {
    prop_or_null(vm, a, b"__last_response_headers")
}

pub fn set_cookie(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let name = a.str(0);
    let mut cookies = match vm.get_prop_raw(&this, b"_cookies") {
        Value::Array(c) => c,
        _ => PhpArray::new(),
    };
    match a.opt_str(1) {
        Some(v) => {
            let mut e = PhpArray::new();
            let _ = e.push(Value::Str(v));
            cookies.insert(Key::from_php_str(&name), Value::Array(e));
        }
        None => {
            cookies.remove(Key::from_php_str(&name).as_ref());
        }
    }
    vm.set_prop_raw(&this, b"_cookies", Value::Array(cookies));
    Ok(Value::Null)
}

pub fn get_cookies(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    Ok(match vm.get_prop_raw(&this, b"_cookies") {
        v @ Value::Array(_) => v,
        _ => Value::Array(PhpArray::new()),
    })
}

pub fn set_soap_headers(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let v = a.val(0);
    match &v {
        Value::Null => {
            vm.set_prop_raw(&this, b"__default_headers", Value::Null);
        }
        _ => {
            let list = header_list(vm, a, &v)?;
            let mut arr = PhpArray::new();
            for h in list {
                let _ = arr.push(Value::Object(h));
            }
            vm.set_prop_raw(&this, b"__default_headers", Value::Array(arr));
        }
    }
    Ok(Value::Bool(true))
}

pub fn set_location(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let old = match vm.get_prop_raw(&this, b"location") {
        v @ Value::Str(_) => v,
        _ => Value::Null,
    };
    match a.opt_str(0) {
        Some(s) if !s.is_empty() => vm.set_prop_raw(&this, b"location", Value::Str(s)),
        _ => vm.set_prop_raw(&this, b"location", Value::Null),
    }
    Ok(old)
}

/// Raises a soap error at the client's level (used by the transport).
pub fn client_error(vm: &mut Vm, msg: &str) -> Unwind {
    soap_error(vm, msg)
}
