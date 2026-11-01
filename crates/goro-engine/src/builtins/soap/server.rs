//! `SoapServer` (PHP manual: The SoapServer class): functions, classes
//! and objects as services, request parsing and SOAP header processing
//! (SOAP 1.1 section 4.2, SOAP 1.2 part 1 section 5.2 and 2.6), the
//! response or fault envelope, WSDL output, persistence.

use std::rc::Rc;

use goro_runtime::{ObjRef, PhpArray, PhpStr, Value};

use super::client::{classmap_of, parse_typemap, part_to_xml, write_headers};
use super::decode::{Dx, from_xml};
use super::encode::{Cx, TypeMapEntry, instance_of, to_xml};
use super::sdl::{Enc, Function, Sdl};
use super::xmlin::InDoc;
use super::xmlout::{
    El, NS_SOAP11_ENC, NS_SOAP11_ENV, NS_SOAP12_ENC, NS_SOAP12_ENV, NS_SOAP12_RPC, NS_XSD,
};
use super::{ErrMode, SoapState, make_fault, pop_mode, push_mode, str_prop};
use crate::class::NativeHooks;
use crate::func::Args;
use crate::vm::{R, Unwind, Vm};

#[derive(Clone, Default)]
pub enum Service {
    #[default]
    None,
    Functions(Vec<String>),
    All,
    Class {
        name: String,
        args: Vec<Value>,
    },
    Object(ObjRef),
}

#[derive(Default)]
pub struct ServerData {
    pub sdl: Option<Rc<Sdl>>,
    pub wsdl: Option<String>,
    pub version: i64,
    pub uri: Option<String>,
    pub actor: Option<String>,
    pub state: SoapState,
    pub send_errors: bool,
    pub service: Service,
    pub persistence: i64,
    /// The object of a class service for this request (or session).
    pub instance: Option<ObjRef>,
    /// SoapHeaders added by addSoapHeader() during handle().
    pub out_headers: Vec<ObjRef>,
    pub trace: bool,
    pub last_response: Option<Vec<u8>>,
    pub in_handle: bool,
    pub constructed: bool,
    /// The operation of the request being handled.
    pub cur_func: Option<Function>,
    /// A service function was called for this request.
    pub called: bool,
    /// The soap_version option.
    pub cfg_version: i64,
}

impl goro_runtime::NativeData for ServerData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        for t in self.state.typemap.iter() {
            if let Some(v) = &t.to_xml {
                visit(v);
            }
            if let Some(v) = &t.from_xml {
                visit(v);
            }
        }
        match &self.service {
            Service::Class { args, .. } => {
                for a in args {
                    visit(a);
                }
            }
            Service::Object(o) => visit(&Value::Object(o.clone())),
            _ => {}
        }
        if let Some(o) = &self.instance {
            visit(&Value::Object(o.clone()));
        }
        for h in &self.out_headers {
            visit(&Value::Object(h.clone()));
        }
    }
    fn clear(&mut self) {
        self.service = Service::None;
        self.instance = None;
        self.out_headers.clear();
        self.state.typemap = Rc::new(Vec::new());
    }
}

pub fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            let cls = vm.class_of(o);
            cls.flags
                .set(cls.flags.get() | crate::class::cflags::NOT_CLONEABLE);
            *o.obj().native.borrow_mut() = Some(Box::new(ServerData {
                version: 1,
                send_errors: true,
                persistence: 2,
                ..Default::default()
            }));
        }),
        ..Default::default()
    }
}

fn with<T>(o: &ObjRef, f: impl FnOnce(&mut ServerData) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<ServerData>().map(f)
}

fn opt(opts: &PhpArray, k: &str) -> Option<Value> {
    opts.get_bytes(k.as_bytes()).map(|v| v.deref())
}

/// Writes a fault envelope to the output (and the response headers).
fn send_fault_doc(vm: &mut Vm, version: i64, xml: &str) {
    send_doc(vm, version, xml.as_bytes(), true);
}

fn send_doc(vm: &mut Vm, version: i64, xml: &[u8], fault: bool) {
    let ct: &[u8] = if version == 2 {
        b"Content-Type: application/soap+xml; charset=utf-8"
    } else {
        b"Content-Type: text/xml; charset=utf-8"
    };
    // Headers only while they can still be sent (no warning otherwise).
    if !super::super::headers::sent(vm) {
        if fault {
            let _ = super::super::headers::add_header(
                vm,
                b"HTTP/1.1 500 Internal Server Error",
                true,
                0,
            );
        }
        let _ = super::super::headers::add_header(vm, ct, true, 0);
        let cl = format!("Content-Length: {}", xml.len());
        let _ = super::super::headers::add_header(vm, cl.as_bytes(), true, 0);
    }
    vm.echo(xml);
}

/// The fault envelope for a SoapFault object.
pub fn fault_xml(
    vm: &mut Vm,
    version: i64,
    fault: &ObjRef,
    sdl: Option<Rc<Sdl>>,
    st: &SoapState,
    headers: &[ObjRef],
    func: Option<&Function>,
) -> R<String> {
    let mut cx = Cx::new(sdl, version, st);
    let (env_name, prefix, env_ns) = if version == 2 {
        ("env:Envelope", "env", NS_SOAP12_ENV)
    } else {
        ("SOAP-ENV:Envelope", "SOAP-ENV", NS_SOAP11_ENV)
    };
    let env = cx.doc.new_el(env_name);
    cx.doc.root = Some(env);
    cx.doc.declare(env, prefix, env_ns);
    // A header fault (SoapFault's $headerfault) goes in the Header.
    let hf = vm.get_prop_raw(fault, b"headerfault");
    let mut hlist: Vec<ObjRef> = headers.to_vec();
    if let Value::Object(h) = &hf
        && instance_of(vm, h, "SoapHeader")
    {
        hlist.push(h.clone());
    }
    if !hlist.is_empty() {
        write_headers(vm, &mut cx, env, prefix, &hlist, &[])?;
    }
    let body = cx.doc.add_child(env, &format!("{prefix}:Body"));
    let f = cx.doc.add_child(body, &format!("{prefix}:Fault"));
    let code = str_prop(vm, fault, "faultcode").unwrap_or_default();
    let code_ns = str_prop(vm, fault, "faultcodens");
    // In the `encoding` option's charset (observed on PHP 8.5; tests
    // bugs/bug38005.phpt, bug47273.phpt).
    let string = match vm.get_prop_raw(fault, b"faultstring") {
        Value::Null => String::new(),
        v => {
            let b = vm.to_php_str(&v)?;
            match &cx.encoding {
                Some(e) => super::charset_to_utf8(e, b.as_bytes()),
                None => b.to_string_lossy(),
            }
        }
    };
    let actor = str_prop(vm, fault, "faultactor");
    let detail = vm.get_prop_raw(fault, b"detail");
    let name = str_prop(vm, fault, "_name");
    if version == 2 {
        let c = cx.doc.add_child(f, "env:Code");
        let v = cx.doc.add_child(c, "env:Value");
        let code = match (code.as_str(), code_ns.as_deref()) {
            ("Client", _) | ("Sender", None) => "env:Sender".to_string(),
            ("Server", _) | ("Receiver", None) => "env:Receiver".to_string(),
            ("VersionMismatch" | "MustUnderstand" | "DataEncodingUnknown", _) => {
                format!("env:{code}")
            }
            (_, Some(ns)) if ns == NS_SOAP12_ENV || ns == NS_SOAP11_ENV => format!("env:{code}"),
            (_, Some(ns)) if ns == NS_SOAP12_RPC => format!("rpc:{code}"),
            (_, Some(ns)) if !ns.is_empty() => {
                let p = cx.doc.ensure_ns(Some(env), ns);
                format!("{p}:{code}")
            }
            _ => code.clone(),
        };
        cx.doc.add_text(v, &code);
        let r = cx.doc.add_child(f, "env:Reason");
        let t = cx.doc.add_child(r, "env:Text");
        let lang = str_prop(vm, fault, "lang").filter(|l| !l.is_empty());
        cx.doc
            .set_attr(t, "xml:lang", lang.as_deref().unwrap_or("en"));
        cx.doc.add_text(t, &string);
        if let Some(a) = actor {
            let n = cx.doc.add_child(f, "env:Node");
            cx.doc.add_text(n, &a);
        }
        if !matches!(detail, Value::Null) || wsdl_fault(func, name.as_deref()) {
            let d = cx.doc.add_child(f, "env:Detail");
            write_detail(vm, &mut cx, d, &detail, name.as_deref(), func)?;
        }
    } else {
        let c = cx.doc.add_child(f, "faultcode");
        let code = match code_ns.as_deref() {
            Some(ns) if ns == NS_SOAP11_ENV => format!("SOAP-ENV:{code}"),
            Some(ns) if !ns.is_empty() => {
                let p = cx.doc.ensure_ns(Some(env), ns);
                format!("{p}:{code}")
            }
            _ => code.clone(),
        };
        cx.doc.add_text(c, &code);
        let s = cx.doc.add_child(f, "faultstring");
        cx.doc.add_text(s, &string);
        if let Some(a) = actor {
            let n = cx.doc.add_child(f, "faultactor");
            cx.doc.add_text(n, &a);
        }
        if !matches!(detail, Value::Null) || wsdl_fault(func, name.as_deref()) {
            let d = cx.doc.add_child(f, "detail");
            write_detail(vm, &mut cx, d, &detail, name.as_deref(), func)?;
        }
    }
    Ok(cx.doc.serialize(env))
}

/// Whether a fault name is one the operation declares.
fn wsdl_fault(func: Option<&Function>, name: Option<&str>) -> bool {
    match (func, name) {
        (Some(f), Some(n)) => f.faults.iter().any(|x| x.name == n && !x.parts.is_empty()),
        _ => false,
    }
}

fn write_detail(
    vm: &mut Vm,
    cx: &mut Cx,
    d: El,
    detail: &Value,
    name: Option<&str>,
    func: Option<&Function>,
) -> R<()> {
    // A fault the WSDL declares.
    if let (Some(n), Some(f), Some(sdl)) = (name, func, cx.sdl.clone())
        && let Some(fd) = f.faults.iter().find(|x| x.name == n)
        && let Some(p) = fd.parts.first()
    {
        let mut val = detail.clone();
        if fd.parts.len() > 1
            && let Value::Object(_) | Value::Array(_) = detail
            && let Some(v) = super::encode::get_field(vm, detail, &p.name)?
        {
            val = v;
        }
        if p.element.is_some() {
            part_to_xml(vm, cx, &sdl, p, &val, d, fd.literal, false)?;
        } else {
            // A type part: named after the part, in the fault's namespace
            // (observed on PHP 8.5; tests
            // interop/Round4/GroupH/r4_groupH_complex_rpcenc_*).
            let el = cx.doc.add_child(d, &p.name);
            let enc = p.typ.as_ref().and_then(|t| sdl.resolve(t));
            to_xml(vm, cx, enc, &val, el, fd.literal)?;
            if !fd.ns.is_empty() {
                let q = cx.doc.qname(Some(el), &fd.ns, &p.name);
                cx.doc.rename(el, &q);
            }
        }
        if !fd.literal
            && let Some(env) = cx.doc.root
        {
            cx.doc.ensure_ns(Some(env), super::xmlout::NS_XSD);
            if cx.version == 2 {
                cx.doc.ensure_ns(Some(env), NS_SOAP12_ENC);
            } else {
                cx.doc.ensure_ns(Some(env), NS_SOAP11_ENC);
                cx.doc
                    .set_attr(env, "SOAP-ENV:encodingStyle", NS_SOAP11_ENC);
            }
        }
        return Ok(());
    }
    match detail {
        Value::Object(_) | Value::Array(_) if name.is_none() => {
            let el = cx.doc.add_child(d, "detail_tmp");
            to_xml(vm, cx, None, detail, el, true)?;
            // The value's fields become the detail's children.
            let kids = std::mem::take(&mut cx.doc.els[el].kids);
            cx.doc.els[d].kids.retain(|k| !matches!(k, super::xmlout::Kid::El(e) if *e == el));
            for k in kids {
                if let super::xmlout::Kid::El(e) = &k {
                    cx.doc.els[*e].parent = Some(d);
                }
                cx.doc.els[d].kids.push(k);
            }
            Ok(())
        }
        _ => {
            let el = match name {
                Some(n) => cx.doc.add_child(d, n),
                None => d,
            };
            if el == d {
                let txt = super::encode::value_to_bytes(vm, detail)?;
                cx.doc.add_text(d, &String::from_utf8_lossy(&txt));
                return Ok(());
            }
            to_xml(vm, cx, None, detail, el, true)
        }
    }
}

// ------------------------------------------------------------ construct

pub fn construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let wsdl = a.opt_str(0).map(|s| s.to_string_lossy());
    let opts = a.array(1);
    push_mode(ErrMode::Throw("Server"));
    let r = construct_inner(vm, a, &this, wsdl, &opts);
    pop_mode();
    match r {
        Err(Unwind::Throw(f)) if super::is_fault_obj(vm, &Value::Object(f.clone())) => {
            let version = with(&this, |d| d.version).unwrap_or(1);
            let xml = fault_xml(vm, version, &f, None, &SoapState::default(), &[], None)?;
            send_fault_doc(vm, version, &xml);
            Err(Unwind::Exit)
        }
        r => r,
    }
}

fn construct_inner(
    vm: &mut Vm,
    a_args: &Args,
    this: &ObjRef,
    wsdl: Option<String>,
    opts: &PhpArray,
) -> R<Value> {
    let a = a_args;
    let version = match opt(opts, "soap_version") {
        Some(v) => {
            let n = vm.to_int(&v)?;
            if n != 1 && n != 2 {
                return Err(super::soap_php_error(
                    vm,
                    a,
                    "'soap_version' option must be SOAP_1_1 or SOAP_1_2",
                ));
            }
            n
        }
        None => 1,
    };
    with(this, |d| {
        d.version = version;
        d.cfg_version = version;
    });
    let uri = match opt(opts, "uri") {
        Some(Value::Str(s)) => Some(s.to_string_lossy()),
        _ => None,
    };
    if wsdl.is_none() && uri.is_none() {
        return Err(super::soap_php_error(
            vm,
            a,
            "'uri' option is required in nonWSDL mode",
        ));
    }
    let actor = match opt(opts, "actor") {
        Some(Value::Str(s)) => Some(s.to_string_lossy()),
        _ => None,
    };
    let encoding = match opt(opts, "encoding") {
        Some(v) => {
            let s = vm.to_php_str(&v)?.to_string_lossy();
            if !super::valid_encoding(&s) {
                return Err(super::soap_php_error(
                    vm,
                    a,
                    &format!("Invalid 'encoding' option - '{s}'"),
                ));
            }
            Some(s)
        }
        None => None,
    };
    let classmap = match opt(opts, "classmap") {
        Some(Value::Array(a)) if !super::client::classmap_valid(&a) => {
            return Err(super::soap_php_error(
                vm,
                a_args,
                "'classmap' option must be an associative array",
            ));
        }
        Some(v @ Value::Array(_)) => classmap_of(v),
        _ => Vec::new(),
    };
    let typemap: Vec<TypeMapEntry> = match opt(opts, "typemap") {
        Some(v @ Value::Array(_)) => parse_typemap(vm, &v)?,
        _ => Vec::new(),
    };
    let features = match opt(opts, "features") {
        Some(v) => vm.to_int(&v)?,
        None => 0,
    };
    let send_errors = match opt(opts, "send_errors") {
        Some(v) => vm.to_bool(&v),
        None => true,
    };
    let trace = match opt(opts, "trace") {
        Some(v) => vm.to_bool(&v),
        None => false,
    };
    let sdl = match &wsdl {
        Some(w) => match super::client::load_sdl(vm, a, w) {
            Ok(s) => Some(Rc::new(s)),
            Err(msg) => {
                let f = make_fault(vm, "Server", &msg)?;
                return Err(Unwind::Throw(f));
            }
        },
        None => None,
    };
    with(this, |d| {
        d.sdl = sdl;
        d.wsdl = wsdl;
        d.uri = uri;
        d.actor = actor;
        d.state = SoapState {
            features,
            encoding,
            classmap,
            typemap: Rc::new(typemap),
        };
        d.send_errors = send_errors;
        d.trace = trace;
        d.constructed = true;
    });
    Ok(Value::Null)
}

// ------------------------------------------------------------ setup

pub fn set_persistence(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let mode = a.int(0);
    let is_class = with(&this, |d| matches!(d.service, Service::Class { .. })).unwrap_or(false);
    if !is_class {
        return vm.throw(
            "Error",
            "SoapServer::setPersistence(): Persistence cannot be set when the SOAP server is used in function mode",
        );
    }
    if mode != 1 && mode != 2 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($mode) must be either SOAP_PERSISTENCE_SESSION or SOAP_PERSISTENCE_REQUEST",
        );
    }
    with(&this, |d| d.persistence = mode);
    Ok(Value::Null)
}

pub fn set_class(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let name = a.str(0);
    let Some(cls) = vm.find_class(name.as_bytes(), true)? else {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #1 ($class) must be a valid class name, {} given",
                name.to_string_lossy()
            ),
        );
    };
    let args: Vec<Value> = a.vals.iter().skip(1).map(|v| v.deref()).collect();
    let cname = cls.name.to_string_lossy();
    with(&this, |d| {
        d.service = Service::Class { name: cname, args };
        d.instance = None;
    });
    Ok(Value::Null)
}

pub fn set_object(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let Some(o) = a.obj(0) else {
        return Ok(Value::Null);
    };
    let _ = vm;
    with(&this, |d| d.service = Service::Object(o));
    Ok(Value::Null)
}

pub fn add_function(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let v = a.val(0);
    let mut names = Vec::new();
    match &v {
        Value::Str(s) => {
            let l = crate::class::lower(s.as_bytes());
            match vm.functions.get(&l) {
                Some(f) => names.push(f.name.to_string_lossy()),
                None => {
                    return vm.fthrow(
                        a,
                        "ValueError",
                        &format!(
                            "Argument #1 ($functions) must contain only valid function names, function \"{}\" not found",
                            s.to_string_lossy()
                        ),
                    );
                }
            }
        }
        Value::Array(arr) => {
            for (_, f) in arr.iter() {
                let Value::Str(s) = f.deref() else {
                    return vm.fthrow(
                        a,
                        "TypeError",
                        "Argument #1 ($functions) must contain only strings",
                    );
                };
                let l = crate::class::lower(s.as_bytes());
                match vm.functions.get(&l) {
                    Some(f) => names.push(f.name.to_string_lossy()),
                    None => {
                        return vm.fthrow(
                            a,
                            "ValueError",
                            &format!(
                                "Argument #1 ($functions) must contain only valid function names, function \"{}\" not found",
                                s.to_string_lossy()
                            ),
                        );
                    }
                }
            }
        }
        Value::Int(999) => {
            vm.fdeprecated(
                a,
                "Enabling all functions via SOAP_FUNCTIONS_ALL is deprecated since 8.4, due to possible security concerns. If all PHP functions should be enabled, the flattened return value of get_defined_functions() can be used",
            )?;
            with(&this, |d| d.service = Service::All);
            return Ok(Value::Null);
        }
        Value::Int(_) => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #1 ($functions) must be SOAP_FUNCTIONS_ALL when an integer is passed",
            );
        }
        _ => {
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #1 ($functions) must be of type array|string|int, {} given",
                    Vm::type_name_given(&v)
                ),
            );
        }
    }
    with(&this, |d| {
        let mut cur = match std::mem::take(&mut d.service) {
            Service::Functions(f) => f,
            _ => Vec::new(),
        };
        for n in names {
            if !cur.iter().any(|c| c.eq_ignore_ascii_case(&n)) {
                cur.push(n);
            }
        }
        d.service = Service::Functions(cur);
    });
    Ok(Value::Null)
}

/// The public methods of a class (in declaration order, parents after).
fn class_methods(vm: &mut Vm, name: &str) -> R<Vec<String>> {
    let Some(cls) = vm.find_class(name.as_bytes(), true)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut c = Some(cls);
    while let Some(k) = c {
        let order = k.method_order.borrow().clone();
        let methods = k.methods.borrow();
        for m in order {
            if let Some(f) = methods.get(&m)
                && f.flags & crate::class::mflags::PUBLIC != 0
            {
                let n = f.name.to_string_lossy();
                if !out.iter().any(|x: &String| x.eq_ignore_ascii_case(&n)) {
                    out.push(n);
                }
            }
        }
        drop(methods);
        c = k.parent.clone();
    }
    Ok(out)
}

pub fn get_functions(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let svc = with(&this, |d| d.service.clone()).unwrap_or_default();
    let mut out = PhpArray::new();
    let names: Vec<String> = match svc {
        Service::Functions(f) => f,
        Service::All => {
            let mut v: Vec<String> = vm
                .functions
                .values()
                .filter(|f| !f.is_native())
                .map(|f| f.name.to_string_lossy())
                .collect();
            v.sort();
            v
        }
        Service::Class { name, .. } => class_methods(vm, &name)?,
        Service::Object(o) => {
            let n = vm.class_of(&o).name.to_string_lossy();
            class_methods(vm, &n)?
        }
        Service::None => Vec::new(),
    };
    for n in names {
        let _ = out.push(Value::Str(PhpStr::from(n)));
    }
    Ok(Value::Array(out))
}

pub fn add_soap_header(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let Some(h) = a.obj(0) else {
        return Ok(Value::Null);
    };
    let in_handle = with(&this, |d| d.in_handle).unwrap_or(false);
    if !in_handle {
        return vm.throw(
            "Error",
            "SoapServer::addSoapHeader() may be called only during SOAP request processing",
        );
    }
    with(&this, |d| d.out_headers.push(h));
    Ok(Value::Null)
}

pub fn get_last_response(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    Ok(
        match with(&this, |d| d.last_response.clone()).flatten() {
            Some(r) => Value::Str(PhpStr::from(r)),
            None => Value::Null,
        },
    )
}

/// SoapServer::fault(): sends a fault and ends the script.
pub fn fault(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let code = a.str(0).to_string_lossy();
    let raw_string = a.str(1);
    let string = raw_string.to_string_lossy();
    let actor = a.opt_str(2).map(|s| s.to_string_lossy()).filter(|s| !s.is_empty());
    let details = a.val(3);
    let name = a.opt_str(4).map(|s| s.to_string_lossy()).filter(|s| !s.is_empty());
    let lang = a.str(5).to_string_lossy();
    let cls = vm.find_class(b"SoapFault", false)?.expect("SoapFault");
    let f = vm.instantiate(&cls)?;
    super::init_fault(
        vm,
        &f,
        Some((None, code)),
        &string,
        actor,
        details,
        name,
        Value::Null,
        &lang,
    );
    super::set_fault_string(vm, &f, raw_string);
    let (version, sdl, st, headers) = with(&this, |d| {
        (d.version, d.sdl.clone(), d.state.clone(), d.out_headers.clone())
    })
    .unwrap_or((1, None, SoapState::default(), Vec::new()));
    let xml = fault_xml(vm, version, &f, sdl, &st, &headers, None)?;
    store_last(&this, xml.as_bytes());
    send_fault_doc(vm, version, &xml);
    Err(Unwind::Exit)
}

fn store_last(this: &ObjRef, xml: &[u8]) {
    with(this, |d| {
        if d.trace {
            d.last_response = Some(xml.to_vec());
        }
    });
}

// ------------------------------------------------------------ handle

/// Outcome of processing a request.
enum Out {
    Doc(String),
    Fault(ObjRef, Option<Function>),
    /// A fault built from text (code, message): before the service runs.
    Plain(&'static str, String),
    /// A protocol fault found before the call: sent, then the script
    /// ends (observed on PHP 8.5; tests
    /// interop/Round4/GroupH/r4_groupH_soapfault_00[34]w.phpt).
    Early(ObjRef),
}

pub fn handle(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let constructed = with(&this, |d| d.constructed).unwrap_or(false);
    if !constructed {
        return vm.throw("Error", "Cannot invoke methods on an uninitialized SoapServer");
    }
    let version = with(&this, |d| d.version).unwrap_or(1);
    let request: Vec<u8> = match a.opt_str(0) {
        Some(s) => s.as_bytes().to_vec(),
        None => vm.out.sapi.input.clone(),
    };
    if request.is_empty() && a.opt_str(0).is_none() {
        return wsdl_output(vm, &this, version);
    }
    let request = decompress(vm, request);
    with(&this, |d| {
        d.in_handle = true;
        d.out_headers.clear();
        d.called = false;
        d.version = d.cfg_version;
    });
    // Output of the service is buffered: a fault discards it (observed
    // on PHP 8.5; test bugs/bug42214.phpt).
    let level = vm.out.buffers.len();
    vm.call_value(&Value::Str(PhpStr::from("ob_start")), vec![])?;
    push_mode(ErrMode::Throw("Server"));
    let r = process(vm, &this, &request);
    pop_mode();
    // exit() (SoapServer::fault() included) keeps what was written.
    let discard = matches!(
        r,
        Ok(Out::Fault(..)) | Ok(Out::Plain(..)) | Ok(Out::Early(..)) | Err(Unwind::Throw(_))
    );
    while vm.out.buffers.len() > level {
        let f = if discard { "ob_end_clean" } else { "ob_end_flush" };
        vm.call_value(&Value::Str(PhpStr::from(f)), vec![])?;
    }
    let headers = with(&this, |d| {
        d.in_handle = false;
        std::mem::take(&mut d.out_headers)
    })
    .unwrap_or_default();
    let (sdl, st) = with(&this, |d| (d.sdl.clone(), d.state.clone()))
        .unwrap_or((None, SoapState::default()));
    let out = match r {
        Ok(o) => o,
        Err(Unwind::Throw(e)) => {
            let called = with(&this, |d| d.called).unwrap_or(false);
            if super::is_fault_obj(vm, &Value::Object(e.clone()))
                && !called
                && super::is_internal_fault(&e)
            {
                // A SOAP error before the service ran (decoding): sent,
                // then the script ends (observed on PHP 8.5; test
                // soap12/T56.phpt).
                Out::Early(e)
            } else if super::is_fault_obj(vm, &Value::Object(e.clone())) {
                let f = with(&this, |d| d.cur_func.take()).flatten();
                Out::Fault(e, f)
            } else if instance_of(vm, &e, "Exception") {
                // Exceptions leave handle() (Errors become faults; observed
                // on PHP 8.5; test bugs/bug47273.phpt).
                return Err(Unwind::Throw(e));
            } else {
                // Other exceptions become Server faults (observed on PHP
                // 8.5; tests server014.phpt, bugs/bug31422.phpt).
                let send = with(&this, |d| d.send_errors).unwrap_or(true);
                let msg = if send {
                    let msg = vm.get_prop_raw(&e, b"message");
                    vm.to_php_str(&msg)?.to_string_lossy()
                } else {
                    "Internal Error".to_string()
                };
                Out::Plain("Server", msg)
            }
        }
        Err(e) => return Err(e),
    };
    // The request's SOAP version.
    let _ = version;
    let ver = with(&this, |d| d.version).unwrap_or(1);
    match out {
        Out::Doc(xml) => {
            store_last(&this, xml.as_bytes());
            if !xml.is_empty() {
                send_doc(vm, ver, xml.as_bytes(), false);
            }
        }
        Out::Fault(f, func) => {
            let xml = fault_xml(vm, ver, &f, sdl, &st, &headers, func.as_ref())?;
            store_last(&this, xml.as_bytes());
            send_fault_doc(vm, ver, &xml);
        }
        Out::Early(f) => {
            let ver = with(&this, |d| d.version).unwrap_or(ver);
            let xml = fault_xml(vm, ver, &f, sdl, &st, &[], None)?;
            store_last(&this, xml.as_bytes());
            send_fault_doc(vm, ver, &xml);
            return Err(Unwind::Exit);
        }
        Out::Plain(code, msg) => {
            let f = make_fault(vm, code, &msg)?;
            let xml = fault_xml(vm, ver, &f, sdl, &st, &headers, None)?;
            store_last(&this, xml.as_bytes());
            send_fault_doc(vm, ver, &xml);
        }
    }
    Ok(Value::Null)
}

fn decompress(vm: &mut Vm, req: Vec<u8>) -> Vec<u8> {
    let enc = vm
        .out
        .sapi
        .request_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(b"content-encoding"))
        .map(|(_, v)| String::from_utf8_lossy(v).to_ascii_lowercase());
    let enc = enc.or_else(|| std::env::var("HTTP_CONTENT_ENCODING").ok().map(|s| s.to_ascii_lowercase()));
    match enc.as_deref().map(str::trim) {
        Some("gzip") | Some("x-gzip") => goro_ext_zlib::gzdecode(&req, 0).unwrap_or(req),
        Some("deflate") => goro_ext_zlib::zlib_decode(&req, 0)
            .or_else(|_| goro_ext_zlib::gzinflate(&req, 0))
            .unwrap_or(req),
        _ => req,
    }
}

/// handle() without a request: the WSDL (or a fault without one).
fn wsdl_output(vm: &mut Vm, this: &ObjRef, version: i64) -> R<Value> {
    let wsdl = with(this, |d| d.wsdl.clone()).flatten();
    match wsdl {
        Some(w) => {
            let path = vm.fs_path(w.as_bytes());
            match std::fs::read(&path) {
                Ok(b) if vm.functions.contains_key(&crate::class::lower(b"readfile")) => {
                    if !super::super::headers::sent(vm) {
                        let _ = super::super::headers::add_header(
                            vm,
                            b"Content-Type: text/xml; charset=utf-8",
                            true,
                            0,
                        );
                    }
                    vm.echo(&b);
                }
                _ => {
                    let f = make_fault(vm, "Server", "Couldn't find WSDL")?;
                    let xml = fault_xml(vm, version, &f, None, &SoapState::default(), &[], None)?;
                    send_fault_doc(vm, version, &xml);                    return Err(Unwind::Exit);
                }
            }
        }
        None => {
            let f = make_fault(vm, "Server", "WSDL generation is not supported yet")?;
            let xml = fault_xml(vm, version, &f, None, &SoapState::default(), &[], None)?;
            send_fault_doc(vm, version, &xml);            return Err(Unwind::Exit);
        }
    }
    Ok(Value::Null)
}

fn fault_out(vm: &mut Vm, code: &'static str, msg: &str) -> R<Out> {
    let f = make_fault(vm, code, msg)?;
    Ok(Out::Early(f))
}

fn is_true(v: &str) -> Option<bool> {
    match v.trim() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

fn process(vm: &mut Vm, this: &ObjRef, request: &[u8]) -> R<Out> {
    let (sdl, st, uri, actor, svc) = with(this, |d| {
        (
            d.sdl.clone(),
            d.state.clone(),
            d.uri.clone(),
            d.actor.clone(),
            d.service.clone(),
        )
    })
    .expect("server data");
    // DTDs are refused (observed on PHP 8.5; soap12 tests).
    let doc = match InDoc::parse(request, None) {
        Ok(d) => d,
        Err(_) => return fault_out(vm, "Client", "Bad Request"),
    };
    if goro_xml_has_dtd(&doc) {
        return fault_out(vm, "Server", "DTD are not supported by SOAP");
    }
    let Some(env) = doc.root() else {
        return fault_out(vm, "Client", "Bad Request");
    };
    let env_ns = doc.ns(env).to_string();
    let version = if env_ns == NS_SOAP12_ENV {
        2
    } else if env_ns == NS_SOAP11_ENV {
        1
    } else {
        return fault_out(vm, "VersionMismatch", "Wrong Version");
    };
    if doc.local(env) != "Envelope" {
        return fault_out(vm, "Client", "looks like we got XML without \"Envelope\" element");
    }
    with(this, |d| d.version = version);
    let sender = if version == 2 { "Sender" } else { "Client" };
    // Envelope checks (SOAP 1.2 part 1 section 5.1, 5.3).
    for (ns, local, _) in doc.attrs(env) {
        if ns == env_ns && local == "encodingStyle" && version == 2 {
            return fault_out(vm, sender, "encodingStyle cannot be specified on the Envelope");
        }
        if ns.is_empty() && version == 2 {
            return fault_out(
                vm,
                sender,
                "A SOAP Envelope element cannot have non Namespace qualified attributes",
            );
        }
    }
    let mut header = None;
    let mut body = None;
    for c in doc.kids(env) {
        if doc.ns(c) == env_ns && doc.local(c) == "Header" && header.is_none() && body.is_none() {
            header = Some(c);
        } else if doc.ns(c) == env_ns && doc.local(c) == "Body" && body.is_none() {
            body = Some(c);
        } else if version == 2 {
            return fault_out(vm, sender, "A SOAP 1.2 envelope can contain only Header and Body");
        }
    }
    let Some(body) = body else {
        return fault_out(vm, sender, "Body must be present in a SOAP envelope");
    };
    if version == 2 {
        for (ns, local, _) in doc.attrs(body) {
            if ns == env_ns && local == "encodingStyle" {
                return fault_out(vm, sender, "encodingStyle cannot be specified on the Body");
            }
        }
    }
    let mut dx = Dx::new(&doc, sdl.clone(), version, &st);
    // The call.
    let call = doc.kids(body).into_iter().next();
    let (func_name, func_def): (Option<String>, Option<Function>) = match (&sdl, call) {
        (_, None) => (None, None),
        (Some(s), Some(c)) => {
            let local = doc.local(c).to_string();
            // rpc operations by name, document ones by their first part
            // (its element, or its name for a type part).
            let f = s
                .functions
                .iter()
                .find(|f| f.rpc && f.name.eq_ignore_ascii_case(&local))
                .or_else(|| {
                    s.functions.iter().find(|f| {
                        !f.rpc
                            && f.input.parts.first().is_some_and(|p| match &p.element {
                                Some((_, n)) => n == &local,
                                None => p.name == local,
                            })
                    })
                })
                .or_else(|| s.find_function(&local))
                .cloned();
            match f {
                Some(f) => (Some(f.name.clone()), Some(f)),
                None => (Some(local), None),
            }
        }
        (None, Some(c)) => (Some(doc.local(c).to_string()), None),
    };
    with(this, |d| d.cur_func = func_def.clone());
    // SOAP 1.2 encodingStyle of the call.
    if version == 2
        && let Some(c) = call
        && let Some(es) = doc.attr_ns(c, NS_SOAP12_ENV, "encodingStyle")
        && es != NS_SOAP12_ENC
    {
        return fault_out(vm, "DataEncodingUnknown", "Unknown Data Encoding Style");
    }
    if version == 1
        && let Some(c) = call
        && let Some(es) = doc.attr_ns(c, NS_SOAP11_ENV, "encodingStyle")
        && es != NS_SOAP11_ENC
        && !es.is_empty()
    {
        return fault_out(vm, "Client", "Unknown Data Encoding Style");
    }
    // The service object.
    let target = match service_target(vm, this, &svc)? {
        Some(t) => t,
        None => Target::None,
    };
    // Headers.
    let mut resp_headers: Vec<(String, String, Value, bool)> = Vec::new();
    if let Some(h) = header {
        // Who each header is for, and its mustUnderstand (all checked
        // before any is processed; observed on PHP 8.5; test
        // soap12/T23.phpt).
        let mut todo = Vec::new();
        for hc in doc.kids(h) {
            let role = if version == 2 {
                doc.attr_ns(hc, NS_SOAP12_ENV, "role")
            } else {
                doc.attr_ns(hc, NS_SOAP11_ENV, "actor")
            };
            if let Some(r) = &role {
                let ours = if version == 2 {
                    r == &format!("{NS_SOAP12_ENV}/role/next")
                        || r == &format!("{NS_SOAP12_ENV}/role/ultimateReceiver")
                } else {
                    r == "http://schemas.xmlsoap.org/soap/actor/next"
                };
                if !ours && actor.as_deref() != Some(r.as_str()) {
                    continue;
                }
            }
            let mu = doc.attr_ns(hc, &env_ns, "mustUnderstand");
            let must = match &mu {
                None => false,
                Some(v) => match is_true(v) {
                    Some(b) => b,
                    None => {
                        return fault_out(vm, sender, "mustUnderstand value is not boolean");
                    }
                },
            };
            todo.push((hc, must));
        }
        for (hc, must) in todo {
            let hname = doc.local(hc).to_string();
            // The handler: in WSDL mode the operation the header element
            // is the request of, else a function of the header's name.
            let hfunc = sdl.as_ref().and_then(|s| {
                s.functions
                    .iter()
                    .find(|f| {
                        if f.rpc {
                            f.name == hname
                        } else {
                            f.input.parts.first().is_some_and(|p| match &p.element {
                                Some((_, n)) => n == &hname,
                                None => p.name == hname,
                            })
                        }
                    })
                    .cloned()
            });
            // A header the called operation declares: a method of the
            // header's name (tests interop/Round4/GroupI/r4_groupI_xsd_03*).
            let hns = doc.ns(hc).to_string();
            // (Without a call: a header any operation declares; test
            // soap12/T63.phpt.)
            let find = |f: &Function| {
                f.input
                    .headers
                    .iter()
                    .find(|d| d.name == hname && d.ns == hns)
                    .cloned()
            };
            let (decl, decl_func) = match (&func_def, &sdl) {
                (Some(f), _) => (find(f), Some(f.clone())),
                (None, Some(s)) => match s.functions.iter().find(|f| find(f).is_some()) {
                    Some(f) => (find(f), Some(f.clone())),
                    None => (None, None),
                },
                _ => (None, None),
            };
            let hfunc = if decl.is_some() { None } else { hfunc };
            let call_name = match &hfunc {
                Some(f) => f.name.clone(),
                None => hname.clone(),
            };
            // Else any method of the header's name (test bugs/bug50762.phpt).
            let handler_ok = target.has(vm, &call_name)?;
            if !handler_ok {
                if must {
                    return fault_out(vm, "MustUnderstand", "Header not understood");
                }
                continue;
            }
            let part = match (&hfunc, &decl) {
                (Some(f), _) => f.input.parts.first().cloned(),
                (None, Some(d)) => Some(d.part.clone()),
                _ => None,
            };
            let enc = match (&part, &sdl) {
                (Some(p), Some(s)) => match &p.element {
                    Some((ens, en)) => s.element(ens, en).map(Enc::Type),
                    None => p.typ.as_ref().and_then(|t| s.resolve(t)),
                },
                _ => None,
            };
            let arg = from_xml(vm, &mut dx, enc, hc)?;
            with(this, |d| d.called = true);
            let r = target.call(vm, &call_name, vec![arg])?;
            if super::is_fault_obj(vm, &r)
                && let Value::Object(f) = r
            {
                return Ok(Out::Fault(f, func_def.clone()));
            }
            if let Value::Object(o) = &r
                && instance_of(vm, o, "SoapHeader")
            {
                with(this, |d| d.out_headers.push(o.clone()));
                continue;
            }
            if matches!(r, Value::Null) {
                continue;
            }
            // The header's operation's response element comes back as a
            // header (observed on PHP 8.5; test soap12/T01.phpt).
            if decl.is_some()
                && let Some(f) = &decl_func
                && let Some(o) = &f.output
                && let Some(oh) = o.headers.first()
            {
                resp_headers.push((oh.ns.clone(), oh.name.clone(), r, oh.literal));
            } else if let Some(f) = &hfunc
                && let Some(o) = &f.output
                && let Some(p) = o.parts.first()
            {
                let (ns, name) = match &p.element {
                    Some((ns, n)) => (ns.clone(), n.clone()),
                    None => (o.ns.clone(), p.name.clone()),
                };
                resp_headers.push((ns, name, r, o.literal));
            }
        }
    }
    let Some(call) = call else {
        // An empty body: only headers.
        return Ok(Out::Doc(response_doc(
            vm, this, &sdl, &st, version, None, None, &[], &resp_headers, false,
        )?));
    };
    let Some(fname) = func_name else {
        return fault_out(vm, "Server", "Procedure '' not present");
    };
    // Is it in the WSDL?
    if sdl.is_some() && func_def.is_none() {
        if version == 2 {
            return Ok(Out::Early(proc_not_present(vm)?));
        }
        return fault_out(vm, "Server", &format!("Procedure '{fname}' not present"));
    }
    // Arguments.
    let mut args = Vec::new();
    match &func_def {
        None => {
            for c in doc.kids(call) {
                args.push(from_xml(vm, &mut dx, None, c)?);
            }
        }
        Some(f) => {
            let s = sdl.clone().expect("WSDL");
            if f.rpc {
                let kids = doc.kids(call);
                for (i, p) in f.input.parts.iter().enumerate() {
                    let node = kids
                        .iter()
                        .copied()
                        .find(|&c| doc.local(c) == p.name)
                        .or_else(|| kids.get(i).copied());
                    let enc = match &p.element {
                        Some((ens, en)) => s.element(ens, en).map(Enc::Type),
                        None => p.typ.as_ref().and_then(|t| s.resolve(t)),
                    };
                    match node {
                        Some(n) => args.push(from_xml(vm, &mut dx, enc, n)?),
                        None => args.push(Value::Null),
                    }
                }
            } else {
                let kids = doc.kids(body);
                for (i, p) in f.input.parts.iter().enumerate() {
                    let (want, enc) = match &p.element {
                        Some((ens, en)) => (en.clone(), s.element(ens, en).map(Enc::Type)),
                        None => (p.name.clone(), p.typ.as_ref().and_then(|t| s.resolve(t))),
                    };
                    let node = kids
                        .iter()
                        .copied()
                        .find(|&c| doc.local(c) == want)
                        .or_else(|| kids.get(i).copied());
                    if let Some(n) = node {
                        args.push(from_xml(vm, &mut dx, enc, n)?);
                    }
                }
            }
        }
    }
    // Is it offered? (After the arguments are decoded; observed on PHP
    // 8.5; test bugs/gh16259.phpt.)
    // An object service: a missing method is a fault of its own (observed
    // on PHP 8.5; test bugs/bug73037.phpt).
    if let Target::Object(_) = &target
        && !target.has(vm, &fname)?
    {
        let call_name = func_def.as_ref().map(|f| f.name.clone()).unwrap_or(fname.clone());
        let f = make_fault(vm, "Server", &format!("Function '{call_name}' doesn't exist"))?;
        return Ok(Out::Fault(f, None));
    }
    if !target.has(vm, &fname)? {
        if version == 2 {
            return Ok(Out::Early(proc_not_present(vm)?));
        }
        return fault_out(vm, "Server", &format!("Procedure '{fname}' not present"));
    }
    let call_name = match &func_def {
        Some(f) => f.name.clone(),
        None => fname.clone(),
    };
    with(this, |d| d.called = true);
    let result = target.call(vm, &call_name, args)?;
    if super::is_fault_obj(vm, &result)
        && let Value::Object(f) = result
    {
        return Ok(Out::Fault(f, func_def));
    }
    let resp_name = doc.local(call).to_string();
    let _ = uri;
    Ok(Out::Doc(response_doc(
        vm,
        this,
        &sdl,
        &st,
        version,
        func_def.as_ref(),
        Some(&resp_name),
        &[result],
        &resp_headers,
        true,
    )?))
}

fn proc_not_present(vm: &mut Vm) -> R<ObjRef> {
    let cls = vm.find_class(b"SoapFault", false)?.expect("SoapFault");
    let f = vm.instantiate(&cls)?;
    super::init_fault(
        vm,
        &f,
        Some((Some(NS_SOAP12_RPC.to_string()), "ProcedureNotPresent".into())),
        "Procedure not present",
        None,
        Value::Null,
        None,
        Value::Null,
        "",
    );
    Ok(f)
}

fn goro_xml_has_dtd(doc: &InDoc) -> bool {
    doc.all_kids(doc.doc)
        .into_iter()
        .any(|c| doc.arena.node_type(c) == goro_xml::NodeType::DocumentType)
}

/// What calls go to.
enum Target {
    None,
    Functions(Vec<String>),
    All,
    Object(ObjRef),
}

impl Target {
    fn has(&self, vm: &mut Vm, name: &str) -> R<bool> {
        Ok(match self {
            Target::None => false,
            Target::Functions(f) => f.iter().any(|n| n.eq_ignore_ascii_case(name)),
            Target::All => vm
                .functions
                .contains_key(&crate::class::lower(name.as_bytes())),
            Target::Object(o) => {
                let cls = vm.class_of(o);
                match cls.find_method(&crate::class::lower(name.as_bytes())) {
                    Some(m) => m.flags & crate::class::mflags::PUBLIC != 0,
                    None => cls.find_method(b"__call").is_some(),
                }
            }
        })
    }

    fn call(&self, vm: &mut Vm, name: &str, args: Vec<Value>) -> R<Value> {
        match self {
            Target::None => Ok(Value::Null),
            Target::Functions(_) | Target::All => {
                vm.call_value(&Value::Str(PhpStr::from(name)), args)
            }
            Target::Object(o) => {
                let mut cb = PhpArray::new();
                let _ = cb.push(Value::Object(o.clone()));
                let _ = cb.push(Value::Str(PhpStr::from(name)));
                vm.call_value(&Value::Array(cb), args)
            }
        }
    }
}

const SESSION_KEY: &str = "_bogus_session_name";

fn service_target(vm: &mut Vm, this: &ObjRef, svc: &Service) -> R<Option<Target>> {
    Ok(Some(match svc {
        Service::None => Target::None,
        Service::Functions(f) => Target::Functions(f.clone()),
        Service::All => Target::All,
        Service::Object(o) => Target::Object(o.clone()),
        Service::Class { name, args } => {
            let persistence = with(this, |d| d.persistence).unwrap_or(2);
            if persistence == 1 {
                // SOAP_PERSISTENCE_SESSION: the object lives in $_SESSION.
                if let Some(o) = session_object(vm, name) {
                    if super::encode::class_is(vm, &o, "__PHP_Incomplete_Class") {
                        let f = make_fault(
                            vm,
                            "Server",
                            "SoapServer class was deserialized from the session prior to loading the class passed to SoapServer::setClass(). Start the session after loading all classes to resolve this issue.",
                        )?;
                        return Err(Unwind::Throw(f));
                    }
                    return Ok(Some(Target::Object(o)));
                }
            }
            let Some(cls) = vm.find_class(name.as_bytes(), true)? else {
                return Ok(Some(Target::None));
            };
            let o = vm.instantiate(&cls)?;
            if cls.find_method(b"__construct").is_some() {
                vm.call_method(&o, b"__construct", args.clone())?;
            }
            // SOAP_PERSISTENCE_REQUEST: a new object for each request
            // (test server010.phpt).
            if persistence == 1 {
                store_session_object(vm, &o);
            }
            Target::Object(o)
        }
    }))
}

fn session_object(vm: &mut Vm, class: &str) -> Option<ObjRef> {
    let p = vm.global_var_ptr(&PhpStr::from("_SESSION"), false)?;
    // SAFETY: the pointer is the global's slot.
    let v = unsafe { (*p).deref() };
    let Value::Array(a) = v else { return None };
    match a.get_bytes(SESSION_KEY.as_bytes()).map(|v| v.deref()) {
        Some(Value::Object(o))
            if encode_class_is(vm, &o, class)
                || encode_class_is(vm, &o, "__PHP_Incomplete_Class") =>
        {
            Some(o)
        }
        _ => None,
    }
}

fn encode_class_is(vm: &mut Vm, o: &ObjRef, class: &str) -> bool {
    super::encode::class_is(vm, o, class)
}

fn store_session_object(vm: &mut Vm, o: &ObjRef) {
    let Some(p) = vm.global_var_ptr(&PhpStr::from("_SESSION"), true) else {
        return;
    };
    // SAFETY: the pointer is the global's slot.
    unsafe {
        let cur = (*p).deref();
        let mut a = match cur {
            Value::Array(a) => a,
            _ => PhpArray::new(),
        };
        a.insert(
            goro_runtime::Key::from(SESSION_KEY),
            Value::Object(o.clone()),
        );
        match &*p {
            Value::Ref(r) => {
                r.set(Value::Array(a));
            }
            _ => {
                *p = Value::Array(a);
            }
        }
    }
}

/// The response envelope.
#[allow(clippy::too_many_arguments)]
fn response_doc(
    vm: &mut Vm,
    this: &ObjRef,
    sdl: &Option<Rc<Sdl>>,
    st: &SoapState,
    version: i64,
    func: Option<&Function>,
    req_name: Option<&str>,
    results: &[Value],
    resp_headers: &[(String, String, Value, bool)],
    has_call: bool,
) -> R<String> {
    let mut cx = Cx::new(sdl.clone(), version, st);
    let (env_name, prefix, env_ns) = if version == 2 {
        ("env:Envelope", "env", NS_SOAP12_ENV)
    } else {
        ("SOAP-ENV:Envelope", "SOAP-ENV", NS_SOAP11_ENV)
    };
    let env = cx.doc.new_el(env_name);
    cx.doc.root = Some(env);
    cx.doc.declare(env, prefix, env_ns);
    // Headers first (observed on PHP 8.5: their namespaces are declared
    // before the body's; test bugs/bug65018.phpt).
    let added = with(this, |d| d.out_headers.clone()).unwrap_or_default();
    if !added.is_empty() || !resp_headers.is_empty() {
        let h = cx.doc.add_child(env, &format!("{prefix}:Header"));
        for (ns, name, v, literal) in resp_headers {
            let q = cx.doc.qname(Some(h), ns, name);
            let el = cx.doc.add_child(h, &q);
            let enc = sdl
                .as_ref()
                .and_then(|s| s.element(ns, name).map(Enc::Type));
            to_xml(vm, &mut cx, enc, v, el, *literal)?;
        }
        for o in &added {
            let ns = str_prop(vm, o, "namespace").unwrap_or_default();
            let name = str_prop(vm, o, "name").unwrap_or_default();
            let data = vm.get_prop_raw(o, b"data");
            let el = cx.doc.add_child(h, &name);
            // Declared by the operation's output headers?
            let decl = func
                .and_then(|f| f.output.as_ref())
                .and_then(|o| o.headers.iter().find(|d| d.name == name && d.ns == ns));
            let (enc, literal) = match (decl, sdl) {
                (Some(d), Some(s)) => (
                    match &d.part.element {
                        Some((ens, en)) => s.element(ens, en).map(Enc::Type),
                        None => d.part.typ.as_ref().and_then(|t| s.resolve(t)),
                    },
                    d.literal,
                ),
                _ => (None, true),
            };
            if !matches!(data, Value::Null) {
                to_xml(vm, &mut cx, enc, &data, el, literal)?;
            }
            let q = cx.doc.qname(Some(el), &ns, &name);
            cx.doc.rename(el, &q);
            if matches!(vm.get_prop_raw(o, b"mustUnderstand"), Value::Bool(true)) {
                let a = format!("{prefix}:mustUnderstand");
                cx.doc
                    .set_attr(el, &a, if version == 2 { "true" } else { "1" });
            }
        }
    }
    let body = cx.doc.add_child(env, &format!("{prefix}:Body"));
    if !has_call {
        return Ok(cx.doc.serialize(env));
    }
    let uri = with(this, |d| d.uri.clone()).flatten().unwrap_or_default();
    let result = results.first().cloned().unwrap_or(Value::Null);
    match (func, sdl) {
        (Some(f), Some(s)) => {
            let Some(out) = &f.output else {
                // One-way operations send nothing back (observed on PHP
                // 8.5; test bugs/bug32776.phpt).
                return Ok(String::new());
            };
            let literal = out.literal;
            if f.rpc {
                let rname = format!("{}Response", f.name);
                let q = if out.ns.is_empty() {
                    format!("{prefix}:{rname}")
                } else {
                    cx.doc.qname(Some(body), &out.ns, &rname)
                };
                let m = cx.doc.add_child(body, &q);
                let n = out.parts.len();
                if version == 2 && n == 1 && !literal {
                    cx.doc.declare(body, "rpc", NS_SOAP12_RPC);
                    let r = cx.doc.add_child(m, "rpc:result");
                    cx.doc.add_text(r, &out.parts[0].name);
                }
                write_results(vm, &mut cx, s, &out.parts, &result, m, literal, true)?;
                if !literal {
                    finish_resp(&mut cx, env, m);
                }
            } else {
                write_results(vm, &mut cx, s, &out.parts, &result, body, literal, false)?;
                if !literal {
                    finish_resp(&mut cx, env, body);
                }
            }
        }
        _ => {
            // Non-WSDL: <ns:nameResponse><return>.
            let rname = format!("{}Response", req_name.unwrap_or(""));
            let q = cx.doc.qname(Some(body), &uri, &rname);
            let m = cx.doc.add_child(body, &q);
            if version == 2 {
                cx.doc.declare(body, "rpc", NS_SOAP12_RPC);
                let r = cx.doc.add_child(m, "rpc:result");
                cx.doc.add_text(r, "return");
            }
            let el = cx.doc.add_child(m, "return");
            to_xml(vm, &mut cx, None, &result, el, false)?;
            finish_resp(&mut cx, env, m);
        }
    }
    Ok(cx.doc.serialize(env))
}

fn finish_resp(cx: &mut Cx, env: El, m: El) {
    cx.doc.ensure_ns(Some(env), NS_XSD);
    if cx.version == 2 {
        cx.doc.ensure_ns(Some(env), NS_SOAP12_ENC);
        cx.doc.set_attr(m, "env:encodingStyle", NS_SOAP12_ENC);
    } else {
        cx.doc.ensure_ns(Some(env), NS_SOAP11_ENC);
        cx.doc
            .set_attr(env, "SOAP-ENV:encodingStyle", NS_SOAP11_ENC);
    }
}

#[allow(clippy::too_many_arguments)]
fn write_results(
    vm: &mut Vm,
    cx: &mut Cx,
    s: &Rc<Sdl>,
    parts: &[super::sdl::Part],
    result: &Value,
    parent: El,
    literal: bool,
    rpc: bool,
) -> R<()> {
    match parts.len() {
        0 => Ok(()),
        1 => {
            part_to_xml(vm, cx, s, &parts[0], result, parent, literal, rpc)?;
            Ok(())
        }
        _ => {
            // By part name, else by position (observed on PHP 8.5; test
            // gh9720.phpt).
            let list: Vec<Value> = match result {
                Value::Array(a) => a.iter().map(|(_, v)| v.deref()).collect(),
                _ => Vec::new(),
            };
            for (i, p) in parts.iter().enumerate() {
                let v = match super::encode::get_field(vm, result, &p.name)? {
                    Some(v) => v,
                    None => list.get(i).cloned().unwrap_or(Value::Null),
                };
                part_to_xml(vm, cx, s, p, &v, parent, literal, rpc)?;
            }
            Ok(())
        }
    }
}
