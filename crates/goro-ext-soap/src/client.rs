//! SoapClient (soap.c): request serialization, the transport hook
//! (__doRequest) and response decoding.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::NativeData;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::common::*;
use crate::encoding::*;
use crate::php::*;
use crate::types::*;
use crate::xml::{self, Node};
use crate::zpp::{self, Func};

static SDL_HANDLERS: NativeObjectHandlers = NativeObjectHandlers::EMPTY;

thread_local! {
    /// The output headers of the last __soapCall() (handed to the
    /// by-reference parameter by the PHP side of the method).
    static OUT_HEADERS: RefCell<Option<Arr>> = const { RefCell::new(None) };
}

pub fn take_out_headers() -> Value {
    match OUT_HEADERS.with(|o| o.borrow_mut().take()) {
        Some(a) => Value::Array(a),
        None => varr(PhpArray::new()),
    }
}

/// A Soap\Sdl object holding the SDL.
fn new_sdl_object(vm: &mut Vm, sdl: SdlRef) -> Value {
    let o = new_object(vm, b"Soap\\Sdl");
    o.borrow_mut().native = Some(NativeData {
        data: sdl,
        handlers: &SDL_HANDLERS,
    });
    Value::Object(o)
}

/// FETCH_THIS_SDL
pub fn client_sdl(vm: &Vm, this: &Obj) -> Option<SdlRef> {
    let v = cprop(this, "sdl");
    let o = is_obj(&v)?;
    if !instance_of(vm, &o, "Soap\\Sdl") {
        return None;
    }
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<RefCell<Sdl>>().ok()
}

fn find_str(ht: &PhpArray, k: &str) -> Option<Value> {
    ht.get_str(k.as_bytes()).map(|v| v.deref())
}

/// soap_create_typemap()
pub fn soap_create_typemap(
    vm: &mut Vm,
    sdl: Option<&SdlRef>,
    ht: &Arr,
) -> SR<Option<Rc<Ht<EncRef>>>> {
    let mut typemap: Option<Ht<EncRef>> = None;
    let entries: Vec<Value> = ht.borrow().values().map(|v| v.deref()).collect();
    for tmp in entries {
        let Value::Array(ht2) = &tmp else {
            let m = format!(
                "SoapHeader::__construct(): \"typemap\" option must be of type array, {} given",
                Vm::value_type_name(&tmp)
            );
            return Err(zpp::throw(vm, b"TypeError", &m));
        };
        let mut type_name: Option<String> = None;
        let mut type_ns: Option<String> = None;
        let mut to_xml: Option<Value> = None;
        let mut to_zval: Option<Value> = None;
        for (k, v) in ht2.borrow().iter() {
            let ArrayKey::String(name) = k else { continue };
            let v = v.deref();
            match name.as_bytes() {
                b"type_name" => {
                    if let Value::String(s) = &v {
                        type_name = Some(s.to_string_lossy());
                    }
                }
                b"type_ns" => {
                    if let Value::String(s) = &v {
                        type_ns = Some(s.to_string_lossy());
                    }
                }
                b"to_xml" => to_xml = Some(v.clone()),
                b"from_xml" => to_zval = Some(v.clone()),
                _ => {}
            }
        }
        let Some(type_name) = type_name else { continue };
        let enc = match &type_ns {
            Some(ns) => get_encoder(sdl, Some(ns), &type_name),
            None => get_encoder_ex(sdl, &type_name),
        };
        let mut d;
        let (fx, fz);
        match &enc {
            Some(e) => {
                let ed = e.d.borrow();
                d = EncDetails {
                    type_: ed.type_,
                    ns: ed.ns.clone(),
                    type_str: ed.type_str.clone(),
                    sdl_type: ed.sdl_type.clone(),
                    clark: None,
                    map: None,
                };
                fx = e.to_xml.get();
                fz = e.to_zval.get();
            }
            None => {
                let u = conv(UNKNOWN_TYPE);
                d = EncDetails {
                    type_: u.type_(),
                    ns: type_ns.clone(),
                    type_str: Some(type_name.clone()),
                    ..Default::default()
                };
                fx = u.to_xml.get();
                fz = u.to_zval.get();
            }
        }
        let mut map = Mapping::default();
        let (mut fx, mut fz) = (fx, fz);
        let base_map = enc.as_ref().and_then(|e| e.map());
        if let Some(t) = to_xml {
            map.to_xml = Some(t);
            fx = to_xml_user_fn();
        } else if let Some(bm) = base_map.as_ref().and_then(|m| m.to_xml.clone()) {
            map.to_xml = Some(bm);
        }
        if let Some(t) = to_zval {
            map.to_zval = Some(t);
            fz = to_zval_user_fn();
        } else if let Some(bm) = base_map.as_ref().and_then(|m| m.to_zval.clone()) {
            map.to_zval = Some(bm);
        }
        d.map = Some(Rc::new(map));
        let new_enc = Encoder::new(d, fx, fz);
        let key = match &type_ns {
            Some(ns) => format!("{}:{}", ns, type_name),
            None => type_name.clone(),
        };
        typemap.get_or_insert_with(Ht::new).update(&key, new_enc);
    }
    Ok(typemap.map(Rc::new))
}

fn call_builtin(vm: &mut Vm, name: &str, args: &[Value]) -> Option<Value> {
    let f = vm.functions.get(name.as_bytes()).copied()?;
    f(vm, args).ok()
}

pub fn construct(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "S!|a", &[], args)?;
    let wsdl = a[0].string();
    let options = a.get(1).and_then(|x| x.arr());
    let guard = begin_code(this, "Client");
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
    let mut cache_wsdl = if crate::sdl::ini_bool(vm, "soap.wsdl_cache_enabled") {
        crate::sdl::ini_long(vm, "soap.wsdl_cache", 1) as i8 as i64
    } else {
        0
    };
    let mut soap_version = SOAP_1_1;
    let mut typemap_ht: Option<Arr> = None;
    match &options {
        Some(opts) => {
            let ht = opts.borrow().clone();
            if wsdl.is_none() {
                match find_str(&ht, "uri") {
                    Some(Value::String(s)) => set_cprop(this, "uri", Value::String(s)),
                    _ => {
                        return Err(docref_fatal(
                            vm,
                            &f.name,
                            "'uri' option is required in nonWSDL mode",
                        ));
                    }
                }
                if let Some(Value::Long(n)) = find_str(&ht, "style") {
                    if n == SOAP_RPC as i64 || n == SOAP_DOCUMENT as i64 {
                        set_cprop(this, "style", Value::Long(n));
                    }
                }
                if let Some(Value::Long(n)) = find_str(&ht, "use") {
                    if n == SOAP_LITERAL as i64 || n == SOAP_ENCODED as i64 {
                        set_cprop(this, "use", Value::Long(n));
                    }
                }
            }
            let mut context = match find_str(&ht, "stream_context") {
                Some(r @ Value::Resource(_)) => Some(r),
                _ => call_builtin(vm, "stream_context_create", &[]),
            };
            match find_str(&ht, "location") {
                Some(Value::String(s)) => set_cprop(this, "location", Value::String(s)),
                _ => {
                    if wsdl.is_none() {
                        return Err(docref_fatal(
                            vm,
                            &f.name,
                            "'location' option is required in nonWSDL mode",
                        ));
                    }
                }
            }
            if let Some(Value::Long(n)) = find_str(&ht, "soap_version") {
                if n == SOAP_1_1 as i64 || n == SOAP_1_2 as i64 {
                    soap_version = n as i32;
                }
            }
            if let Some(Value::String(login)) = find_str(&ht, "login") {
                set_cprop(this, "_login", Value::String(login));
                if let Some(Value::String(p)) = find_str(&ht, "password") {
                    set_cprop(this, "_password", Value::String(p));
                }
                if let Some(Value::Long(n)) = find_str(&ht, "authentication") {
                    if n == SOAP_AUTHENTICATION_DIGEST {
                        set_cprop(this, "_use_digest", Value::True);
                    }
                }
            }
            if let Some(Value::String(host)) = find_str(&ht, "proxy_host") {
                set_cprop(this, "_proxy_host", Value::String(host));
                if let Some(p) = find_str(&ht, "proxy_port") {
                    let n = match p {
                        Value::Long(n) => n,
                        other => other.to_long(),
                    };
                    set_cprop(this, "_proxy_port", Value::Long(n));
                }
                if let Some(Value::String(l)) = find_str(&ht, "proxy_login") {
                    set_cprop(this, "_proxy_login", Value::String(l));
                    if let Some(Value::String(p)) = find_str(&ht, "proxy_password") {
                        set_cprop(this, "_proxy_password", Value::String(p));
                    }
                }
            }
            if let Some(Value::String(lc)) = find_str(&ht, "local_cert") {
                if context.is_none() {
                    context = call_builtin(vm, "stream_context_create", &[]);
                }
                if let Some(c) = &context {
                    call_builtin(
                        vm,
                        "stream_context_set_option",
                        &[
                            c.clone(),
                            vstr("ssl"),
                            vstr("local_cert"),
                            Value::String(lc),
                        ],
                    );
                    if let Some(Value::String(pp)) = find_str(&ht, "passphrase") {
                        call_builtin(
                            vm,
                            "stream_context_set_option",
                            &[
                                c.clone(),
                                vstr("ssl"),
                                vstr("passphrase"),
                                Value::String(pp),
                            ],
                        );
                    }
                }
            }
            if let Some(t) = find_str(&ht, "trace") {
                if matches!(t, Value::True | Value::Long(1)) {
                    set_cprop(this, "trace", Value::True);
                }
            }
            if let Some(e) = find_str(&ht, "exceptions") {
                if matches!(e, Value::False | Value::Long(0)) {
                    set_cprop(this, "_exceptions", Value::False);
                }
            }
            if let Some(Value::Long(n)) = find_str(&ht, "compression") {
                let has = [
                    "gzinflate",
                    "gzdeflate",
                    "gzuncompress",
                    "gzcompress",
                    "gzencode",
                ]
                .iter()
                .all(|fname| vm.functions.contains_key(fname.as_bytes()));
                if has {
                    set_cprop(this, "compression", Value::Long(n));
                }
            }
            if let Some(Value::String(e)) = find_str(&ht, "encoding") {
                let es = e.to_string_lossy();
                if !goro_ext_xml::parser::encoding_supported(&es) {
                    return Err(docref_fatal(
                        vm,
                        &f.name,
                        format!("Invalid 'encoding' option - '{}'", es),
                    ));
                }
                set_cprop(this, "_encoding", Value::String(e));
            }
            if let Some(Value::Array(cm)) = find_str(&ht, "classmap") {
                if cm.borrow().looks_packed() && !cm.borrow().is_empty() {
                    return Err(docref_fatal(
                        vm,
                        &f.name,
                        "'classmap' option must be an associative array",
                    ));
                }
                set_cprop(this, "_classmap", copy_value(&Value::Array(cm)));
            }
            if let Some(Value::Array(tm)) = find_str(&ht, "typemap") {
                if !tm.borrow().is_empty() {
                    typemap_ht = Some(tm);
                }
            }
            if let Some(Value::Long(n)) = find_str(&ht, "features") {
                set_cprop(this, "_features", Value::Long(n));
            }
            if let Some(ct) = find_str(&ht, "connection_timeout") {
                let l = ct.to_long();
                if l > 0 {
                    set_cprop(this, "_connection_timeout", Value::Long(l));
                }
            }
            if let Some(c) = context {
                set_cprop(this, "_stream_context", c);
            }
            if let Some(Value::Long(n)) = find_str(&ht, "cache_wsdl") {
                cache_wsdl = n;
            }
            if let Some(Value::String(ua)) = find_str(&ht, "user_agent") {
                set_cprop(this, "_user_agent", Value::String(ua));
            }
            if let Some(ka) = find_str(&ht, "keep_alive") {
                if matches!(ka, Value::False | Value::Long(0)) {
                    set_cprop(this, "_keep_alive", Value::False);
                }
            }
            if let Some(Value::Long(n)) = find_str(&ht, "ssl_method") {
                set_cprop(this, "_ssl_method", Value::Long(n));
                docref_deprecated(
                    vm,
                    &f.name,
                    "The \"ssl_method\" option is deprecated. Use \"ssl\" stream context options instead",
                );
            }
        }
        None => {
            if wsdl.is_none() {
                return Err(docref_fatal(
                    vm,
                    &f.name,
                    "'location' and 'uri' options are required in nonWSDL mode",
                ));
            }
        }
    }
    set_cprop(this, "_soap_version", Value::Long(soap_version as i64));
    let mut sdl: Option<SdlRef> = None;
    if let Some(w) = &wsdl {
        let old = g(|g| {
            let o = g.soap_version;
            g.soap_version = soap_version;
            o
        });
        crate::sdl::set_cur_fname(&f.name);
        let r = crate::sdl::get_sdl(vm, this, w, cache_wsdl);
        g(|g| g.soap_version = old);
        let s = r?;
        let so = new_sdl_object(vm, s.clone());
        set_cprop(this, "sdl", so);
        sdl = Some(s);
    }
    if let Some(tm) = typemap_ht {
        if let Some(t) = soap_create_typemap(vm, sdl.as_ref(), &tm)? {
            set_cprop(
                this,
                "typemap",
                Value::Array(Rc::new(RefCell::new(typemap_to_array(&t)))),
            );
            TYPEMAPS.with(|m| {
                m.borrow_mut()
                    .insert(Rc::as_ptr(this) as *const u8 as usize, t);
            });
        }
    }
    Ok(())
}

thread_local! {
    /// The typemap encoders of each client (the C code keeps the encoder
    /// pointers in the typemap property's array).
    static TYPEMAPS: RefCell<std::collections::HashMap<usize, Rc<Ht<EncRef>>>> =
        RefCell::new(std::collections::HashMap::new());
}

fn typemap_to_array(t: &Ht<EncRef>) -> PhpArray {
    let mut a = PhpArray::new();
    for (k, _) in t.iter() {
        if let HKey::Str(s) = k {
            a.set(
                ArrayKey::String(PhpString::from_string(s.clone())),
                Value::Null,
            );
        }
    }
    a
}

fn client_typemap(this: &Obj) -> Option<Rc<Ht<EncRef>>> {
    if !matches!(cprop(this, "typemap"), Value::Array(_)) {
        return None;
    }
    TYPEMAPS.with(|m| {
        m.borrow()
            .get(&(Rc::as_ptr(this) as *const u8 as usize))
            .cloned()
    })
}

fn do_request(
    vm: &mut Vm,
    this: &Obj,
    request: Node,
    location: &str,
    action: Option<&str>,
    version: i32,
    one_way: bool,
) -> SR<(bool, Value)> {
    let buf = xml::dump_doc(request);
    let params = [
        vstr(&buf),
        vstr(location),
        vstr(action.unwrap_or("")),
        Value::Long(version as i64),
        if one_way { Value::True } else { Value::False },
    ];
    let trace = matches!(cprop(this, "trace"), Value::True);
    if trace {
        set_cprop(this, "__last_request", params[0].clone());
    }
    let response = call_method(vm, this, "__doRequest", &params);
    let response = match response {
        Ok(r) => r,
        Err(_) if vm.current_exception.is_some() && !vm.exit_requested => Value::Null,
        Err(e) => return Err(e),
    };
    let mut ret = true;
    let response = response.deref();
    if !matches!(response, Value::String(_)) {
        let err_exc = vm
            .current_exception
            .as_ref()
            .is_some_and(|e| val_instance_of(vm, e, "Error"));
        if err_exc {
            // a programmer error in __doRequest(): let it bubble up
        } else if is_obj(&cprop(this, "__soap_fault")).is_none() {
            add_soap_fault(
                vm,
                this,
                "Client",
                "SoapClient::__doRequest() returned non string value",
            );
        }
        ret = false;
    } else if trace {
        set_cprop(this, "__last_response", response.clone());
    }
    if ret && is_obj(&cprop(this, "__soap_fault")).is_some() {
        ret = false;
    }
    Ok((ret, response))
}

#[allow(clippy::too_many_arguments)]
fn do_soap_call(
    vm: &mut Vm,
    this: &Obj,
    function: &str,
    real_args: &[Value],
    location: Option<String>,
    soap_action: Option<String>,
    call_uri: Option<String>,
    soap_headers: Option<&[Value]>,
    output_headers: Option<&Arr>,
) -> SR<Value> {
    let guard = begin_code(this, "Client");
    let r = do_soap_call_inner(
        vm,
        this,
        function,
        real_args,
        location,
        soap_action,
        call_uri,
        soap_headers,
        output_headers,
    );
    end_code(guard);
    r
}

#[allow(clippy::too_many_arguments)]
fn do_soap_call_inner(
    vm: &mut Vm,
    this: &Obj,
    function: &str,
    real_args: &[Value],
    location: Option<String>,
    soap_action: Option<String>,
    call_uri: Option<String>,
    soap_headers: Option<&[Value]>,
    output_headers: Option<&Arr>,
) -> SR<Value> {
    let trace = matches!(cprop(this, "trace"), Value::True);
    if trace {
        set_cprop(this, "__last_request", Value::Null);
        set_cprop(this, "__last_response", Value::Null);
    }
    let soap_version = match cprop(this, "_soap_version") {
        Value::Long(2) => SOAP_1_2,
        _ => SOAP_1_1,
    };
    let location = location.or_else(|| as_str(&cprop(this, "location")));
    let sdl = client_sdl(vm, this);
    let typemap = client_typemap(this);
    // clear_soap_fault()
    set_cprop(this, "__soap_fault", Value::Null);
    let encoding = as_str(&cprop(this, "_encoding"));
    let class_map = match cprop(this, "_classmap") {
        Value::Array(a) => Some(a),
        _ => None,
    };
    let features = match cprop(this, "_features") {
        Value::Long(n) => n,
        _ => 0,
    };
    let old = g(|g| {
        let old = (
            g.sdl.clone(),
            g.encoding.clone(),
            g.class_map.clone(),
            g.typemap.clone(),
            g.features,
        );
        g.soap_version = soap_version;
        g.sdl = sdl.clone();
        g.encoding = encoding;
        g.class_map = class_map;
        g.typemap = typemap;
        g.features = features;
        old
    });
    let r = do_soap_call_body(
        vm,
        this,
        function,
        real_args,
        location,
        soap_action,
        call_uri,
        soap_headers,
        output_headers,
        sdl.as_ref(),
        soap_version,
    );
    g(|g| {
        g.sdl = old.0;
        g.encoding = old.1;
        g.class_map = old.2;
        g.typemap = old.3;
        g.features = old.4;
    });
    r
}

#[allow(clippy::too_many_arguments)]
fn do_soap_call_body(
    vm: &mut Vm,
    this: &Obj,
    function: &str,
    real_args: &[Value],
    location: Option<String>,
    soap_action: Option<String>,
    call_uri: Option<String>,
    soap_headers: Option<&[Value]>,
    output_headers: Option<&Arr>,
    sdl: Option<&SdlRef>,
    soap_version: i32,
) -> SR<Value> {
    let mut return_value = Value::Null;
    let mut ret = false;
    if let Some(sdl_ref) = sdl {
        match get_function(sdl, function) {
            Some(fn_) => {
                let binding = fn_.binding.clone().unwrap();
                let one_way = fn_.response_name.is_none()
                    && fn_.response_params.is_none()
                    && soap_headers.is_none();
                let loc = location.clone().unwrap_or_else(|| binding.location.clone());
                let resp = if binding.binding_type == BINDING_SOAP {
                    let fnb = fn_.soap().cloned().unwrap();
                    let request = serialize_function_call(
                        vm,
                        this,
                        Some(&fn_),
                        None,
                        fnb.input.ns.as_deref(),
                        real_args,
                        soap_version,
                        soap_headers,
                    )?;
                    let r = do_request(
                        vm,
                        this,
                        request,
                        &loc,
                        fnb.soap_action.as_deref(),
                        soap_version,
                        one_way,
                    );
                    xml::free_doc(request);
                    r?
                } else {
                    let tns = sdl_ref.borrow().target_ns.clone();
                    let request = serialize_function_call(
                        vm,
                        this,
                        Some(&fn_),
                        None,
                        tns.as_deref(),
                        real_args,
                        soap_version,
                        soap_headers,
                    )?;
                    let r = do_request(vm, this, request, &loc, None, soap_version, one_way);
                    xml::free_doc(request);
                    r?
                };
                ret = resp.0;
                if ret {
                    if let Value::String(s) = &resp.1 {
                        encode_reset_ns();
                        let r = crate::packet::parse_packet_soap(
                            vm,
                            this,
                            s.as_bytes(),
                            Some(&fn_),
                            &mut return_value,
                            output_headers,
                        );
                        encode_finish();
                        ret = r?;
                    }
                }
            }
            None => {
                let m = format!(
                    "Function (\"{}\") is not a valid method for this service",
                    function
                );
                add_soap_fault(vm, this, "Client", &m);
            }
        }
    } else {
        let uri = cprop(this, "uri");
        if !matches!(uri, Value::String(_)) {
            add_soap_fault(vm, this, "Client", "Error finding \"uri\" property");
        } else if location.is_none() {
            add_soap_fault(
                vm,
                this,
                "Client",
                "Error could not find \"location\" property",
            );
        } else {
            let call_uri = call_uri.unwrap_or_else(|| as_str(&uri).unwrap_or_default());
            let request = serialize_function_call(
                vm,
                this,
                None,
                Some(function),
                Some(&call_uri),
                real_args,
                soap_version,
                soap_headers,
            )?;
            let action = match &soap_action {
                None => format!("{}#{}", call_uri, function),
                Some(a) => a.clone(),
            };
            let r = do_request(
                vm,
                this,
                request,
                location.as_deref().unwrap(),
                Some(&action),
                soap_version,
                false,
            );
            xml::free_doc(request);
            let resp = r?;
            ret = resp.0;
            if ret {
                if let Value::String(s) = &resp.1 {
                    encode_reset_ns();
                    let r = crate::packet::parse_packet_soap(
                        vm,
                        this,
                        s.as_bytes(),
                        None,
                        &mut return_value,
                        output_headers,
                    );
                    encode_finish();
                    ret = r?;
                }
            }
        }
    }
    let fault = cprop(this, "__soap_fault");
    if !ret {
        if is_obj(&fault).is_some() {
            return_value = fault.clone();
        } else {
            let f = add_soap_fault_ex(vm, this, "Client", Some("Unknown Error"), None, None, "en");
            return_value = Value::Object(f);
        }
    } else if is_obj(&fault).is_some() {
        return_value = fault.clone();
    }
    if vm.current_exception.is_none()
        && val_instance_of(vm, &return_value, "SoapFault")
        && !matches!(cprop(this, "_exceptions"), Value::False)
    {
        vm.current_exception = Some(return_value.clone());
        return Err(Bail);
    }
    Ok(return_value)
}

/// serialize_function_call()
#[allow(clippy::too_many_arguments)]
fn serialize_function_call(
    vm: &mut Vm,
    this: &Obj,
    function: Option<&FuncRef>,
    function_name: Option<&str>,
    uri: Option<&str>,
    arguments: &[Value],
    version: i32,
    soap_headers: Option<&[Value]>,
) -> SR<Node> {
    encode_reset_ns();
    let doc = xml::new_doc();
    let r = serialize_function_call_inner(
        vm,
        this,
        doc,
        function,
        function_name,
        uri,
        arguments,
        version,
        soap_headers,
    );
    encode_finish();
    match r {
        Ok(()) => Ok(doc),
        Err(e) => {
            xml::free_doc(doc);
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn serialize_function_call_inner(
    vm: &mut Vm,
    this: &Obj,
    doc: Node,
    function: Option<&FuncRef>,
    function_name: Option<&str>,
    uri: Option<&str>,
    arguments: &[Value],
    version: i32,
    soap_headers: Option<&[Value]>,
) -> SR<()> {
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
        return Err(soap_error(vm, "Unknown SOAP version"));
    };
    xml::set_ns(envelope, ns);
    xml::doc_set_root(doc, envelope);
    let head = soap_headers.map(|_| xml::new_child(envelope, ns, "Header", None));
    let body = xml::new_child(envelope, ns, "Body", None);
    let mut method: Option<Node> = None;
    let style;
    let mut use_;
    let mut hdrs: Option<Ht<HeaderRef>> = None;
    if let Some(fnb) = function.and_then(|f| f.soap()) {
        let f = function.unwrap();
        hdrs = fnb.input.headers.clone();
        style = fnb.style;
        use_ = fnb.input.use_;
        if style == SOAP_RPC {
            let nsp = encode_add_ns(body, fnb.input.ns.as_deref());
            let name = f
                .request_name
                .clone()
                .unwrap_or_else(|| f.function_name.clone());
            method = Some(xml::new_child(body, nsp, &name, None));
        }
    } else {
        style = match cprop(this, "style") {
            Value::Long(n) => n as i32,
            _ => SOAP_RPC,
        };
        if style == SOAP_RPC {
            let nsp = encode_add_ns(body, uri);
            if let Some(n) = function_name {
                method = Some(xml::new_child(body, nsp, n, None));
            } else if let Some(n) = function.and_then(|f| f.request_name.clone()) {
                method = Some(xml::new_child(body, nsp, &n, None));
            } else if let Some(f) = function {
                method = Some(xml::new_child(body, nsp, &f.function_name, None));
            } else {
                method = Some(body);
            }
        } else {
            method = Some(body);
        }
        use_ = match cprop(this, "use") {
            Value::Long(n) if n == SOAP_LITERAL as i64 => SOAP_LITERAL,
            _ => SOAP_ENCODED,
        };
    }
    let is_soap_fn = function.is_some_and(|f| f.is_soap());
    let serialize_one = |vm: &mut Vm, i: usize, arg: Option<&Value>| -> SR<()> {
        let parameter = get_param(function, None, i, false);
        if style == SOAP_RPC {
            let m = method.unwrap_or(body);
            serialize_parameter(vm, parameter.as_ref(), arg, i, None, use_, m)?;
        } else if style == SOAP_DOCUMENT {
            let param = serialize_parameter(vm, parameter.as_ref(), arg, i, None, use_, body)?;
            if is_soap_fn {
                if let Some(el) = parameter.as_ref().and_then(|p| p.element.clone()) {
                    let (name, namens) = {
                        let e = el.borrow();
                        (e.name.clone(), e.namens.clone())
                    };
                    let nsp = encode_add_ns(param, namens.as_deref());
                    xml::set_name(param, name.as_deref().unwrap_or(""));
                    xml::set_ns(param, nsp);
                }
            }
        }
        Ok(())
    };
    for (i, a) in arguments.iter().enumerate() {
        serialize_one(vm, i, Some(a))?;
    }
    if let Some(rp) = function.and_then(|f| f.request_params.as_ref()) {
        let n = rp.len();
        if n > arguments.len() {
            for i in arguments.len()..n {
                serialize_one(vm, i, None)?;
            }
        }
    }
    if let (Some(head), Some(sh)) = (head, soap_headers) {
        for header in sh {
            let Some(ho) = is_obj(header).filter(|o| instance_of(vm, o, "SoapHeader")) else {
                continue;
            };
            let (name, hns) = (prop(&ho, "name"), prop(&ho, "namespace"));
            let (Value::String(name), Value::String(hns)) = (name, hns) else {
                continue;
            };
            let name = name.to_string_lossy();
            let hns = hns.to_string_lossy();
            let mut hdr_use = SOAP_LITERAL;
            let mut enc: Option<EncRef> = None;
            if let Some(hs) = &hdrs {
                let key = format!("{}:{}", hns, name);
                if let Some(hdr) = hs.find(&key) {
                    hdr_use = hdr.use_;
                    enc = hdr.encode.clone();
                    if hdr_use == SOAP_ENCODED {
                        use_ = SOAP_ENCODED;
                    }
                }
            }
            let data = prop(&ho, "data");
            let h = if !matches!(data, Value::Null | Value::Undef) {
                let h = master_to_xml(vm, enc.as_ref(), Some(&data), hdr_use, head)?;
                xml::set_name(h, &name);
                h
            } else {
                let h = xml::new_doc_node(Some(doc), &name);
                xml::add_child(head, h);
                h
            };
            let nsptr = encode_add_ns(h, Some(&hns));
            xml::set_ns(h, nsptr);
            crate::server::set_soap_header_attributes(h, &ho, version);
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
            if let Some(m) = method {
                let ens = xml::node_ns(envelope);
                xml::set_ns_prop(m, ens, "encodingStyle", SOAP_1_2_ENC_NAMESPACE);
            }
        }
    }
    Ok(())
}

fn verify_soap_headers_array(vm: &Vm, a: &PhpArray) -> bool {
    a.values()
        .all(|v| val_instance_of(vm, &v.deref(), "SoapHeader"))
}

/// soap_client_call_common()
#[allow(clippy::too_many_arguments)]
fn soap_client_call_common(
    vm: &mut Vm,
    this: &Obj,
    function: &str,
    args: &PhpArray,
    location: Option<String>,
    soap_action: Option<String>,
    uri: Option<String>,
    soap_headers: Option<Vec<Value>>,
    output_headers: Option<&Arr>,
) -> SR<Value> {
    let mut soap_headers = soap_headers;
    if let Value::Array(dh) = cprop(this, "__default_headers") {
        let defaults: Vec<Value> = dh.borrow().values().cloned().collect();
        match &mut soap_headers {
            Some(sh) => {
                for d in defaults {
                    if matches!(d.deref(), Value::Object(_)) {
                        sh.push(d);
                    }
                }
            }
            None => soap_headers = Some(defaults),
        }
    }
    let real_args: Vec<Value> = args.values().map(|v| v.deref()).collect();
    do_soap_call(
        vm,
        this,
        function,
        &real_args,
        location,
        soap_action,
        uri,
        soap_headers.as_deref(),
        output_headers,
    )
}

pub fn call(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "Sh", &[], args)?;
    let function = a[0].string().unwrap_or_default();
    let args = a[1].arr().unwrap();
    let args = args.borrow().clone();
    soap_client_call_common(vm, this, &function, &args, None, None, None, None, None)
}

pub fn soap_call(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "Sh|h!zz", &[], args)?;
    let function = a[0].string().unwrap_or_default();
    let fargs = a[1].arr().unwrap().borrow().clone();
    let mut location = None;
    let mut soap_action = None;
    let mut uri = None;
    if let Some(opts) = a.get(2).and_then(|x| x.arr()) {
        let o = opts.borrow();
        if let Some(Value::String(s)) = find_str(&o, "location") {
            location = Some(s.to_string_lossy());
        }
        if let Some(Value::String(s)) = find_str(&o, "soapaction") {
            soap_action = Some(s.to_string_lossy());
        }
        if let Some(Value::String(s)) = find_str(&o, "uri") {
            uri = Some(s.to_string_lossy());
        }
    }
    let output_headers = if args.len() > 4 {
        let a = new_arr();
        OUT_HEADERS.with(|o| *o.borrow_mut() = Some(a.clone()));
        Some(a)
    } else {
        None
    };
    let headers = a.get(3).and_then(|x| x.any()).map(|v| v.deref());
    let soap_headers: Option<Vec<Value>> = match headers {
        None | Some(Value::Null) | Some(Value::Undef) => None,
        Some(Value::Array(h)) => {
            if !verify_soap_headers_array(vm, &h.borrow()) {
                return Err(docref_fatal(vm, &f.name, "Invalid SOAP header"));
            }
            Some(h.borrow().values().cloned().collect())
        }
        Some(Value::Object(o)) if instance_of(vm, &o, "SoapHeader") => Some(vec![Value::Object(o)]),
        Some(other) => {
            let m = format!(
                "must be of type SoapHeader|array|null, {} given",
                Vm::value_type_name(&other)
            );
            return Err(f.arg_type_error(vm, 4, &m));
        }
    };
    soap_client_call_common(
        vm,
        this,
        &function,
        &fargs,
        location,
        soap_action,
        uri,
        soap_headers,
        output_headers.as_ref(),
    )
}

pub fn get_functions(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    zpp::parse(vm, f, "", &[], args)?;
    let Some(sdl) = client_sdl(vm, this) else {
        return Ok(Value::Null);
    };
    let mut out = PhpArray::new();
    for func in sdl.borrow().functions.values() {
        out.push(vstr(function_to_string(func)));
    }
    Ok(varr(out))
}

pub fn get_types(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    zpp::parse(vm, f, "", &[], args)?;
    let Some(sdl) = client_sdl(vm, this) else {
        return Ok(Value::Null);
    };
    let mut out = PhpArray::new();
    let types: Vec<TypeRef> = sdl
        .borrow()
        .types
        .as_ref()
        .map(|t| t.values().cloned().collect())
        .unwrap_or_default();
    for t in types {
        let mut buf = String::new();
        type_to_string(&t, &mut buf, 0);
        out.push(vstr(buf));
    }
    Ok(varr(out))
}

pub fn get_prop(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value], name: &str) -> SR<Value> {
    zpp::parse(vm, f, "", &[], args)?;
    Ok(copy_value(&cprop(this, name)))
}

pub fn do_request_method(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "SSsl|bS!", &[], args)?;
    let buf = a[0].str().unwrap_or_default().to_vec();
    let location = a[1].string().unwrap_or_default();
    let action = a[2].string().unwrap_or_default();
    let version = a[3].long().unwrap_or(1);
    let mut one_way = a.get(4).is_some_and(|x| x.bool());
    let uri_parser = a.get(5).and_then(|x| x.string());
    if g(|g| g.features) & SOAP_WAIT_ONE_WAY_CALLS != 0 {
        one_way = false;
    }
    let r = crate::http::make_http_soap_request(
        vm,
        this,
        &buf,
        &location,
        &action,
        version,
        uri_parser.as_deref(),
        !one_way,
    )?;
    match r {
        Some(v) => {
            if one_way {
                Ok(vstr(""))
            } else {
                Ok(v)
            }
        }
        None => Ok(Value::Null),
    }
}

pub fn set_cookie(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "S|S!", &[], args)?;
    let name = a[0].str().unwrap_or_default().to_vec();
    let val = a.get(1).and_then(|x| x.str().map(|s| s.to_vec()));
    let cookies = match cprop(this, "_cookies") {
        Value::Array(c) => c,
        _ => new_arr(),
    };
    let key = symtable_key(&name);
    {
        let mut c = cookies.borrow_mut();
        match val {
            None => {
                c.remove(&key);
            }
            Some(v) => {
                let mut z = PhpArray::new();
                z.set(ArrayKey::Int(0), vstr(v));
                c.set(key, varr(z));
            }
        }
    }
    set_cprop(this, "_cookies", Value::Array(cookies));
    Ok(Value::Null)
}

pub fn set_soap_headers(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "|z", &[], args)?;
    let headers = a.first().and_then(|x| x.any()).map(|v| v.deref());
    match headers {
        None | Some(Value::Null) => set_cprop(this, "__default_headers", Value::Null),
        Some(Value::Array(h)) => {
            if !verify_soap_headers_array(vm, &h.borrow()) {
                return Err(docref_fatal(vm, &f.name, "Invalid SOAP header"));
            }
            set_cprop(this, "__default_headers", copy_value(&Value::Array(h)));
        }
        Some(Value::Object(o)) if instance_of(vm, &o, "SoapHeader") => {
            let mut d = PhpArray::new();
            d.push(Value::Object(o));
            set_cprop(this, "__default_headers", varr(d));
        }
        Some(other) => {
            let m = format!(
                "must be of type SoapHeader|array|null, {} given",
                Vm::value_type_name(&other)
            );
            return Err(f.arg_type_error(vm, 1, &m));
        }
    }
    Ok(Value::True)
}

pub fn set_location(vm: &mut Vm, f: &Func, this: &Obj, args: &[Value]) -> SR<Value> {
    let a = zpp::parse(vm, f, "|S!", &[], args)?;
    let old = cprop(this, "location");
    match a.first().and_then(|x| x.str().map(|s| s.to_vec())) {
        Some(l) if !l.is_empty() => set_cprop(this, "location", vstr(l)),
        _ => set_cprop(this, "location", Value::Null),
    }
    Ok(old)
}
