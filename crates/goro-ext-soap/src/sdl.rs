//! php_sdl.c: loading a WSDL into the service description (SDL).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::encoding::{get_encoder_ex, get_encoder_from_prefix};
use crate::php::*;
use crate::schema::{load_schema, schema_pass2};
use crate::types::*;
use crate::xml::{self, Node};

pub struct SdlCtx {
    pub sdl: SdlRef,
    pub docs: Ht<Node>,
    pub messages: Ht<Node>,
    pub bindings: Ht<Node>,
    pub port_types: Ht<Node>,
    pub services: Ht<Node>,
    pub attributes: Option<Ht<AttrRef>>,
    pub attribute_groups: Option<Ht<TypeRef>>,
}

fn attr(n: Node, name: &str) -> Option<Node> {
    xml::get_attribute(xml::properties(n), name)
}

fn attr_val(n: Node, name: &str) -> Option<String> {
    attr(n, name).map(xml::attr_value)
}

/// get_element()
fn get_element(sdl: &SdlRef, node: Node, type_: &str) -> Option<TypeRef> {
    let s = sdl.borrow();
    let els = s.elements.as_ref()?;
    let (cptype, ns) = xml::parse_namespace(type_);
    match xml::search_ns(node, ns.as_deref()) {
        Some(nsptr) => {
            let nscat = format!("{}:{}", xml::ns_href(nsptr), cptype);
            els.find(&nscat).or_else(|| els.find(type_)).cloned()
        }
        None => els.find(type_).cloned(),
    }
}

fn is_wsdl_element(vm: &mut Vm, node: Node) -> SR<bool> {
    if let Some(href) = xml::ns_href_of(node) {
        if href != WSDL_NAMESPACE {
            if let Some(a) =
                xml::get_attribute_ex(xml::properties(node), "required", Some(WSDL_NAMESPACE))
            {
                let v = xml::attr_value(a);
                if v == "1" || v == "true" {
                    return Err(soap_error(
                        vm,
                        format!("Parsing WSDL: Unknown required WSDL extension '{}'", href),
                    ));
                }
            }
            return Ok(false);
        }
    }
    Ok(true)
}

/// soap_xmlParseFile() of a document referenced by the WSDL.
pub fn load_doc(vm: &mut Vm, _ctx: &mut SdlCtx, uri: &str) -> Result<Node, Option<String>> {
    xml::parse_file(vm, uri)
}

fn safe_name(n: Node) -> String {
    xml::name(n)
}

fn load_wsdl_ex(vm: &mut Vm, uri: &str, ctx: &mut SdlCtx, include: bool) -> SR<()> {
    if ctx.docs.exists(uri) {
        return Ok(());
    }
    let wsdl = match load_doc(vm, ctx, uri) {
        Ok(d) => d,
        Err(msg) => {
            if let Some(m) = msg.as_deref() {
                if m.starts_with("failed to load") {
                    let w = format!("I/O warning : {}", m.trim_end_matches('\n'));
                    let fname = cur_fname();
                    docref_warning(vm, &fname, w);
                }
            }
            return Err(match msg {
                Some(m) => soap_error(
                    vm,
                    format!("Parsing WSDL: Couldn't load from '{}' : {}", uri, m),
                ),
                None => soap_error(vm, format!("Parsing WSDL: Couldn't load from '{}'", uri)),
            });
        }
    };
    ctx.docs.add(uri, wsdl);
    let root = xml::children(wsdl);
    let Some(definitions) = xml::get_node_ex(root, "definitions", Some(WSDL_NAMESPACE)) else {
        if include {
            if let Some(schema) = xml::get_node_ex(root, "schema", Some(XSD_NAMESPACE)) {
                return load_schema(vm, ctx, schema);
            }
        }
        return Err(soap_error(
            vm,
            format!("Parsing WSDL: Couldn't find <definitions> in '{}'", uri),
        ));
    };
    if !include {
        if let Some(tns) = attr_val(definitions, "targetNamespace") {
            ctx.sdl.borrow_mut().target_ns = Some(tns);
        }
    }
    let mut trav = xml::children(definitions);
    while let Some(tr) = trav {
        if !is_wsdl_element(vm, tr)? {
            trav = xml::next(tr);
            continue;
        }
        if xml::node_is_equal(tr, "types") {
            let mut t2 = xml::children(tr);
            while let Some(x) = t2 {
                if xml::node_is_equal_ex(x, Some("schema"), Some(XSD_NAMESPACE)) {
                    load_schema(vm, ctx, x)?;
                } else if is_wsdl_element(vm, x)? && !xml::node_is_equal(x, "documentation") {
                    return Err(soap_error(
                        vm,
                        format!("Parsing WSDL: Unexpected WSDL element <{}>", safe_name(x)),
                    ));
                }
                t2 = xml::next(x);
            }
        } else if xml::node_is_equal(tr, "import") {
            if let Some(loc) = attr(tr, "location") {
                let u = xml::construct_uri(loc);
                load_wsdl_ex(vm, &u, ctx, true)?;
            }
        } else {
            let kind = ["message", "portType", "binding", "service"]
                .into_iter()
                .find(|k| xml::node_is_equal(tr, k));
            match kind {
                Some(k) => {
                    let name = attr(tr, "name").filter(|a| xml::attr_has_value(*a));
                    match name {
                        Some(n) => {
                            let nv = xml::attr_value(n);
                            let table = match k {
                                "message" => &mut ctx.messages,
                                "portType" => &mut ctx.port_types,
                                "binding" => &mut ctx.bindings,
                                _ => &mut ctx.services,
                            };
                            if !table.add(&nv, tr) {
                                return Err(soap_error(
                                    vm,
                                    format!("Parsing WSDL: <{}> '{}' already defined", k, nv),
                                ));
                            }
                        }
                        None => {
                            return Err(soap_error(
                                vm,
                                format!("Parsing WSDL: <{}> has no name attribute", k),
                            ));
                        }
                    }
                }
                None => {
                    if !xml::node_is_equal(tr, "documentation") {
                        return Err(soap_error(
                            vm,
                            format!("Parsing WSDL: Unexpected WSDL element <{}>", safe_name(tr)),
                        ));
                    }
                }
            }
        }
        trav = xml::next(tr);
    }
    Ok(())
}

fn strip_prefix_name(s: &str, last: bool) -> String {
    let p = if last { s.rfind(':') } else { s.find(':') };
    match p {
        Some(p) => s[p + 1..].to_string(),
        None => s.to_string(),
    }
}

fn wsdl_soap_binding_header(
    vm: &mut Vm,
    ctx: &SdlCtx,
    header: Node,
    wsdl_soap_ns: &str,
    fault: bool,
) -> SR<SoapBindingHeader> {
    let Some(tmp) = attr_val(header, "message") else {
        return Err(soap_error(
            vm,
            "Parsing WSDL: Missing message attribute for <header>",
        ));
    };
    let ctype = strip_prefix_name(&tmp, true);
    let Some(message) = ctx.messages.find(&ctype).copied() else {
        return Err(soap_error(
            vm,
            format!("Parsing WSDL: Missing <message> with name '{}'", tmp),
        ));
    };
    let Some(part_name) = attr_val(header, "part") else {
        return Err(soap_error(
            vm,
            "Parsing WSDL: Missing part attribute for <header>",
        ));
    };
    let Some(part) = xml::get_node_with_attribute_ex(
        xml::children(message),
        Some("part"),
        Some(WSDL_NAMESPACE),
        "name",
        &part_name,
        None,
    ) else {
        return Err(soap_error(
            vm,
            format!("Parsing WSDL: Missing part '{}' in <message>", part_name),
        ));
    };
    let mut h = SoapBindingHeader {
        name: part_name.clone(),
        ..Default::default()
    };
    h.use_ = match attr_val(header, "use") {
        Some(u) if u == "encoded" => SOAP_ENCODED,
        _ => SOAP_LITERAL,
    };
    h.ns = attr_val(header, "namespace");
    if h.use_ == SOAP_ENCODED {
        match attr_val(header, "encodingStyle") {
            Some(es) => {
                if es == SOAP_1_1_ENC_NAMESPACE {
                    h.encoding_style = SOAP_ENCODING_1_1;
                } else if es == SOAP_1_2_ENC_NAMESPACE {
                    h.encoding_style = SOAP_ENCODING_1_2;
                } else {
                    return Err(soap_error(
                        vm,
                        format!("Parsing WSDL: Unknown encodingStyle '{}'", es),
                    ));
                }
            }
            None => return Err(soap_error(vm, "Parsing WSDL: Unspecified encodingStyle")),
        }
    }
    if let Some(t) = attr_val(part, "type") {
        h.encode = get_encoder_from_prefix(Some(&ctx.sdl), part, &t);
    } else if let Some(e) = attr_val(part, "element") {
        h.element = get_element(&ctx.sdl, part, &e);
        if let Some(el) = &h.element {
            let el = el.borrow();
            h.encode = el.encode.clone();
            if h.ns.is_none() && el.namens.is_some() {
                h.ns = el.namens.clone();
            }
            if let Some(n) = &el.name {
                h.name = n.clone();
            }
        }
    }
    if !fault {
        let mut trav = xml::children(header);
        while let Some(tr) = trav {
            if xml::node_is_equal_ex(tr, Some("headerfault"), Some(wsdl_soap_ns)) {
                let hf = wsdl_soap_binding_header(vm, ctx, tr, wsdl_soap_ns, true)?;
                let key = match &hf.ns {
                    Some(ns) => format!("{}:{}", ns, hf.name),
                    None => hf.name.clone(),
                };
                h.headerfaults
                    .get_or_insert_with(Ht::new)
                    .add(&key, Rc::new(hf));
            } else if is_wsdl_element(vm, tr)? && !xml::node_is_equal(tr, "documentation") {
                return Err(soap_error(
                    vm,
                    format!("Parsing WSDL: Unexpected WSDL element <{}>", safe_name(tr)),
                ));
            }
            trav = xml::next(tr);
        }
    }
    Ok(h)
}

fn wsdl_soap_binding_body(
    vm: &mut Vm,
    ctx: &SdlCtx,
    node: Node,
    wsdl_soap_ns: &str,
    binding: &mut SoapBindingBody,
    params: &mut Option<Vec<ParamRef>>,
) -> SR<()> {
    let mut trav = xml::children(node);
    while let Some(tr) = trav {
        if xml::node_is_equal_ex(tr, Some("body"), Some(wsdl_soap_ns)) {
            let body = tr;
            binding.use_ = match attr_val(body, "use") {
                Some(u) if u == "literal" => SOAP_LITERAL,
                _ => SOAP_ENCODED,
            };
            if let Some(ns) = attr_val(body, "namespace") {
                binding.ns = Some(ns);
            }
            if let Some(parts) = attr_val(body, "parts") {
                let mut ht: Vec<ParamRef> = Vec::new();
                let list = params.get_or_insert_with(Vec::new);
                for part in parts.split(' ').filter(|p| !p.is_empty()) {
                    let pos = list
                        .iter()
                        .position(|p| p.param_name.as_deref() == Some(part));
                    match pos {
                        Some(i) => {
                            ht.push(Rc::new((*list[i]).clone()));
                            // param->paramName = NULL in the old table
                            let mut old = (*list[i]).clone();
                            old.param_name = None;
                            list[i] = Rc::new(old);
                        }
                        None => {
                            return Err(soap_error(
                                vm,
                                format!("Parsing WSDL: Missing part '{}' in <message>", part),
                            ));
                        }
                    }
                }
                *params = Some(ht);
            }
            if binding.use_ == SOAP_ENCODED {
                match attr_val(body, "encodingStyle") {
                    Some(es) => {
                        if es == SOAP_1_1_ENC_NAMESPACE {
                            binding.encoding_style = SOAP_ENCODING_1_1;
                        } else if es == SOAP_1_2_ENC_NAMESPACE {
                            binding.encoding_style = SOAP_ENCODING_1_2;
                        } else {
                            return Err(soap_error(
                                vm,
                                format!("Parsing WSDL: Unknown encodingStyle '{}'", es),
                            ));
                        }
                    }
                    None => return Err(soap_error(vm, "Parsing WSDL: Unspecified encodingStyle")),
                }
            }
        } else if xml::node_is_equal_ex(tr, Some("header"), Some(wsdl_soap_ns)) {
            let h = wsdl_soap_binding_header(vm, ctx, tr, wsdl_soap_ns, false)?;
            let key = match &h.ns {
                Some(ns) => format!("{}:{}", ns, h.name),
                None => h.name.clone(),
            };
            binding
                .headers
                .get_or_insert_with(Ht::new)
                .add(&key, Rc::new(h));
        } else if is_wsdl_element(vm, tr)? && !xml::node_is_equal(tr, "documentation") {
            return Err(soap_error(
                vm,
                format!("Parsing WSDL: Unexpected WSDL element <{}>", safe_name(tr)),
            ));
        }
        trav = xml::next(tr);
    }
    Ok(())
}

fn wsdl_message(vm: &mut Vm, ctx: &SdlCtx, message_name: &str) -> SR<Vec<ParamRef>> {
    let ctype = strip_prefix_name(message_name, true);
    let Some(message) = ctx.messages.find(&ctype).copied() else {
        return Err(soap_error(
            vm,
            format!(
                "Parsing WSDL: Missing <message> with name '{}'",
                message_name
            ),
        ));
    };
    let mut params = Vec::new();
    let mut trav = xml::children(message);
    while let Some(tr) = trav {
        if let Some(h) = xml::ns_href_of(tr) {
            if h != WSDL_NAMESPACE {
                return Err(soap_error(
                    vm,
                    format!(
                        "Parsing WSDL: Unexpected extensibility element <{}>",
                        safe_name(tr)
                    ),
                ));
            }
        }
        if xml::node_is_equal(tr, "documentation") {
            trav = xml::next(tr);
            continue;
        }
        if !xml::node_is_equal(tr, "part") {
            return Err(soap_error(
                vm,
                format!("Parsing WSDL: Unexpected WSDL element <{}>", safe_name(tr)),
            ));
        }
        let part = tr;
        let Some(name) = attr_val(part, "name") else {
            return Err(soap_error(
                vm,
                format!(
                    "Parsing WSDL: No name associated with <part> '{}'",
                    safe_name(message)
                ),
            ));
        };
        let mut param = SdlParam {
            param_name: Some(name),
            ..Default::default()
        };
        if let Some(t) = attr_val(part, "type") {
            param.encode = get_encoder_from_prefix(Some(&ctx.sdl), part, &t);
        } else if let Some(e) = attr_val(part, "element") {
            param.element = get_element(&ctx.sdl, part, &e);
            if let Some(el) = &param.element {
                param.encode = el.borrow().encode.clone();
            }
        }
        params.push(Rc::new(param));
        trav = xml::next(tr);
    }
    Ok(params)
}

thread_local! {
    static FNAME: RefCell<String> = const { RefCell::new(String::new()) };
}

/// The method loading the WSDL (for "I/O warning" docref prefixes).
pub fn set_cur_fname(f: &str) {
    FNAME.with(|x| *x.borrow_mut() = f.to_string());
}

fn cur_fname() -> String {
    FNAME.with(|x| x.borrow().clone())
}

fn load_wsdl(vm: &mut Vm, uri: &str) -> SR<SdlRef> {
    let sdl = Rc::new(RefCell::new(Sdl {
        source: uri.to_string(),
        ..Default::default()
    }));
    let mut ctx = SdlCtx {
        sdl: sdl.clone(),
        docs: Ht::new(),
        messages: Ht::new(),
        bindings: Ht::new(),
        port_types: Ht::new(),
        services: Ht::new(),
        attributes: None,
        attribute_groups: None,
    };
    let r = load_wsdl_body(vm, uri, &mut ctx);
    for d in ctx.docs.values() {
        xml::free_doc(*d);
    }
    r?;
    Ok(sdl)
}

fn load_wsdl_body(vm: &mut Vm, uri: &str, ctx: &mut SdlCtx) -> SR<()> {
    load_wsdl_ex(vm, uri, ctx, false)?;
    schema_pass2(vm, ctx)?;
    let n = ctx.services.len();
    if n == 0 {
        return Err(soap_error(vm, "Parsing WSDL: Couldn't bind to service"));
    }
    for i in 0..n {
        let service = *ctx.services.get_index(i).unwrap().1;
        let mut has_soap_port = false;
        let mut trav = xml::children(service);
        while let Some(tr) = trav {
            if !is_wsdl_element(vm, tr)? || xml::node_is_equal(tr, "documentation") {
                trav = xml::next(tr);
                continue;
            }
            if !xml::node_is_equal(tr, "port") {
                return Err(soap_error(
                    vm,
                    format!("Parsing WSDL: Unexpected WSDL element <{}>", safe_name(tr)),
                ));
            }
            let port = tr;
            let mut binding_type = 0;
            let Some(binding_attr) = attr_val(port, "binding") else {
                return Err(soap_error(
                    vm,
                    "Parsing WSDL: No binding associated with <port>",
                ));
            };
            let mut address: Option<Node> = None;
            let mut wsdl_soap_ns: &str = "";
            let mut t2 = xml::children(port);
            while let Some(x) = t2 {
                if xml::node_is_equal(x, "address") {
                    if let Some(h) = xml::ns_href_of(x) {
                        if h == WSDL_SOAP11_NAMESPACE {
                            address = Some(x);
                            wsdl_soap_ns = WSDL_SOAP11_NAMESPACE;
                            binding_type = BINDING_SOAP;
                        } else if h == WSDL_SOAP12_NAMESPACE {
                            address = Some(x);
                            wsdl_soap_ns = WSDL_SOAP12_NAMESPACE;
                            binding_type = BINDING_SOAP;
                        } else if h == RPC_SOAP12_NAMESPACE {
                            address = Some(x);
                            wsdl_soap_ns = RPC_SOAP12_NAMESPACE;
                            binding_type = BINDING_SOAP;
                        } else if h == WSDL_HTTP11_NAMESPACE || h == WSDL_HTTP12_NAMESPACE {
                            address = Some(x);
                            binding_type = BINDING_HTTP;
                        }
                    }
                }
                if Some(x) != address
                    && is_wsdl_element(vm, x)?
                    && !xml::node_is_equal(x, "documentation")
                {
                    return Err(soap_error(
                        vm,
                        format!("Parsing WSDL: Unexpected WSDL element <{}>", safe_name(x)),
                    ));
                }
                t2 = xml::next(x);
            }
            if address.is_none() || binding_type == BINDING_HTTP {
                if has_soap_port || xml::next(tr).is_some() || i < n - 1 {
                    trav = xml::next(tr);
                    continue;
                } else if address.is_none() {
                    return Err(soap_error(
                        vm,
                        "Parsing WSDL: No address associated with <port>",
                    ));
                }
            }
            has_soap_port = true;
            let address = address.unwrap();
            let Some(location) = attr_val(address, "location") else {
                return Err(soap_error(
                    vm,
                    "Parsing WSDL: No location associated with <port>",
                ));
            };
            let ctype = strip_prefix_name(&binding_attr, true);
            let Some(binding) = ctx.bindings.find(&ctype).copied() else {
                return Err(soap_error(
                    vm,
                    format!("Parsing WSDL: No <binding> element with name '{}'", ctype),
                ));
            };
            let mut soap_binding: Option<SoapBinding> = None;
            if binding_type == BINDING_SOAP {
                let mut sb = SoapBinding {
                    style: SOAP_DOCUMENT,
                    transport: 0,
                };
                let sbn = xml::get_node_ex(xml::children(binding), "binding", Some(wsdl_soap_ns));
                let mut skip = false;
                if let Some(sbn) = sbn {
                    if attr_val(sbn, "style").as_deref() == Some("rpc") {
                        sb.style = SOAP_RPC;
                    }
                    if let Some(tp) = attr_val(sbn, "transport") {
                        if tp == WSDL_HTTP_TRANSPORT {
                            sb.transport = 1;
                        } else {
                            skip = true;
                        }
                    }
                }
                if skip {
                    trav = xml::next(tr);
                    continue;
                }
                soap_binding = Some(sb);
            }
            let Some(bname) = attr_val(binding, "name") else {
                return Err(soap_error(
                    vm,
                    "Parsing WSDL: Missing 'name' attribute for <binding>",
                ));
            };
            let Some(btype) = attr_val(binding, "type") else {
                return Err(soap_error(
                    vm,
                    "Parsing WSDL: Missing 'type' attribute for <binding>",
                ));
            };
            let ctype = strip_prefix_name(&btype, false);
            let Some(port_type) = ctx.port_types.find(&ctype).copied() else {
                return Err(soap_error(
                    vm,
                    format!("Parsing WSDL: Missing <portType> with name '{}'", bname),
                ));
            };
            let tmpbinding = Rc::new(SdlBinding {
                name: bname.clone(),
                location,
                binding_type,
                soap: soap_binding.clone(),
            });
            let mut t2 = xml::children(binding);
            while let Some(op) = t2 {
                if (binding_type == BINDING_SOAP
                    && xml::node_is_equal_ex(op, Some("binding"), Some(wsdl_soap_ns)))
                    || !is_wsdl_element(vm, op)?
                    || xml::node_is_equal(op, "documentation")
                {
                    t2 = xml::next(op);
                    continue;
                }
                if !xml::node_is_equal(op, "operation") {
                    return Err(soap_error(
                        vm,
                        format!("Parsing WSDL: Unexpected WSDL element <{}>", safe_name(op)),
                    ));
                }
                let operation = op;
                let Some(op_name) = attr_val(operation, "name") else {
                    return Err(soap_error(
                        vm,
                        "Parsing WSDL: Missing 'name' attribute for <operation>",
                    ));
                };
                let mut t3 = xml::children(operation);
                while let Some(x) = t3 {
                    if binding_type == BINDING_SOAP
                        && xml::node_is_equal_ex(x, Some("operation"), Some(wsdl_soap_ns))
                    {
                    } else if is_wsdl_element(vm, x)?
                        && !xml::node_is_equal(x, "input")
                        && !xml::node_is_equal(x, "output")
                        && !xml::node_is_equal(x, "fault")
                        && !xml::node_is_equal(x, "documentation")
                    {
                        return Err(soap_error(
                            vm,
                            format!("Parsing WSDL: Unexpected WSDL element <{}>", safe_name(x)),
                        ));
                    }
                    t3 = xml::next(x);
                }
                let Some(pt_op) = xml::get_node_with_attribute_ex(
                    xml::children(port_type),
                    Some("operation"),
                    Some(WSDL_NAMESPACE),
                    "name",
                    &op_name,
                    None,
                ) else {
                    return Err(soap_error(
                        vm,
                        format!(
                            "Parsing WSDL: Missing <portType>/<operation> with name '{}'",
                            op_name
                        ),
                    ));
                };
                let mut function = SdlFunction {
                    function_name: op_name.clone(),
                    ..Default::default()
                };
                if binding_type == BINDING_SOAP {
                    let sb = soap_binding.as_ref().unwrap();
                    let mut fb = SoapBindingFunction {
                        style: sb.style,
                        ..Default::default()
                    };
                    if let Some(so) =
                        xml::get_node_ex(xml::children(operation), "operation", Some(wsdl_soap_ns))
                    {
                        if let Some(a) = attr_val(so, "soapAction") {
                            fb.soap_action = Some(a);
                        }
                        match attr_val(so, "style") {
                            Some(s) => {
                                fb.style = if s == "rpc" { SOAP_RPC } else { SOAP_DOCUMENT };
                            }
                            None => fb.style = sb.style,
                        }
                    }
                    function.binding_attrs = Some(fb);
                }
                if let Some(input) =
                    xml::get_node_ex(xml::children(pt_op), "input", Some(WSDL_NAMESPACE))
                {
                    let Some(message) = attr_val(input, "message") else {
                        return Err(soap_error(
                            vm,
                            format!("Parsing WSDL: Missing name for <input> of '{}'", op_name),
                        ));
                    };
                    function.request_params = Some(wsdl_message(vm, ctx, &message)?);
                    function.request_name = Some(function.function_name.clone());
                    if binding_type == BINDING_SOAP {
                        if let Some(inb) = xml::get_node_ex(
                            xml::children(operation),
                            "input",
                            Some(WSDL_NAMESPACE),
                        ) {
                            let mut fb = function.binding_attrs.take().unwrap();
                            wsdl_soap_binding_body(
                                vm,
                                ctx,
                                inb,
                                wsdl_soap_ns,
                                &mut fb.input,
                                &mut function.request_params,
                            )?;
                            function.binding_attrs = Some(fb);
                        }
                    }
                }
                if let Some(output) =
                    xml::get_node_ex(xml::children(pt_op), "output", Some(WSDL_NAMESPACE))
                {
                    let Some(message) = attr_val(output, "message") else {
                        return Err(soap_error(
                            vm,
                            format!("Parsing WSDL: Missing name for <output> of '{}'", op_name),
                        ));
                    };
                    function.response_params = Some(wsdl_message(vm, ctx, &message)?);
                    function.response_name = Some(format!("{}Response", function.function_name));
                    if binding_type == BINDING_SOAP {
                        if let Some(outb) = xml::get_node_ex(
                            xml::children(operation),
                            "output",
                            Some(WSDL_NAMESPACE),
                        ) {
                            let mut fb = function.binding_attrs.take().unwrap();
                            wsdl_soap_binding_body(
                                vm,
                                ctx,
                                outb,
                                wsdl_soap_ns,
                                &mut fb.output,
                                &mut function.response_params,
                            )?;
                            function.binding_attrs = Some(fb);
                        }
                    }
                }
                let mut fault = xml::children(pt_op);
                while let Some(fnode) = fault {
                    if xml::node_is_equal_ex(fnode, Some("fault"), Some(WSDL_NAMESPACE)) {
                        let Some(fname) = attr_val(fnode, "name") else {
                            return Err(soap_error(
                                vm,
                                format!("Parsing WSDL: Missing name for <fault> of '{}'", op_name),
                            ));
                        };
                        let Some(message) = attr_val(fnode, "message") else {
                            return Err(soap_error(
                                vm,
                                format!("Parsing WSDL: Missing name for <output> of '{}'", op_name),
                            ));
                        };
                        let details = wsdl_message(vm, ctx, &message)?;
                        if details.len() > 1 {
                            return Err(soap_error(
                                vm,
                                format!(
                                    "Parsing WSDL: The fault message '{}' must have a single part",
                                    message
                                ),
                            ));
                        }
                        let mut f = SdlFault {
                            name: fname.clone(),
                            details,
                            binding: None,
                        };
                        if binding_type == BINDING_SOAP {
                            if let Some(sf) = xml::get_node_with_attribute_ex(
                                xml::children(operation),
                                Some("fault"),
                                Some(WSDL_NAMESPACE),
                                "name",
                                &fname,
                                None,
                            ) {
                                let mut fnodes = xml::children(sf);
                                while let Some(fx) = fnodes {
                                    if xml::node_is_equal_ex(fx, Some("fault"), Some(wsdl_soap_ns))
                                    {
                                        let mut fb = SoapBindingFault {
                                            use_: match attr_val(fx, "use") {
                                                Some(u) if u == "encoded" => SOAP_ENCODED,
                                                _ => SOAP_LITERAL,
                                            },
                                            ns: attr_val(fx, "namespace"),
                                            encoding_style: 0,
                                        };
                                        if fb.use_ == SOAP_ENCODED {
                                            match attr_val(fx, "encodingStyle") {
                                                Some(es) => {
                                                    if es == SOAP_1_1_ENC_NAMESPACE {
                                                        fb.encoding_style = SOAP_ENCODING_1_1;
                                                    } else if es == SOAP_1_2_ENC_NAMESPACE {
                                                        fb.encoding_style = SOAP_ENCODING_1_2;
                                                    } else {
                                                        return Err(soap_error(
                                                            vm,
                                                            format!(
                                                                "Parsing WSDL: Unknown encodingStyle '{}'",
                                                                es
                                                            ),
                                                        ));
                                                    }
                                                }
                                                None => {
                                                    return Err(soap_error(
                                                        vm,
                                                        "Parsing WSDL: Unspecified encodingStyle",
                                                    ));
                                                }
                                            }
                                        }
                                        f.binding = Some(fb);
                                    } else if is_wsdl_element(vm, fx)?
                                        && !xml::node_is_equal(fx, "documentation")
                                    {
                                        return Err(soap_error(
                                            vm,
                                            format!(
                                                "Parsing WSDL: Unexpected WSDL element <{}>",
                                                safe_name(fx)
                                            ),
                                        ));
                                    }
                                    fnodes = xml::next(fx);
                                }
                            }
                        }
                        let faults = function.faults.get_or_insert_with(Ht::new);
                        if !faults.add(&fname, Rc::new(f)) {
                            return Err(soap_error(
                                vm,
                                format!(
                                    "Parsing WSDL: <fault> with name '{}' already defined in '{}'",
                                    fname, op_name
                                ),
                            ));
                        }
                    }
                    fault = xml::next(fnode);
                }
                function.binding = Some(tmpbinding.clone());
                let lc = function.function_name.to_ascii_lowercase();
                let req = function.request_name.clone();
                let fname = function.function_name.clone();
                let func = Rc::new(function);
                {
                    let mut s = ctx.sdl.borrow_mut();
                    if !s.functions.add(&lc, func.clone()) {
                        s.functions.push(func.clone());
                    }
                    if let Some(rn) = req {
                        if rn != fname {
                            s.requests
                                .get_or_insert_with(Ht::new)
                                .add(&rn.to_ascii_lowercase(), func.clone());
                        }
                    }
                }
                t2 = xml::next(op);
            }
            {
                let mut s = ctx.sdl.borrow_mut();
                let b = s.bindings.get_or_insert_with(Ht::new);
                if !b.add(&bname, tmpbinding.clone()) {
                    b.push(tmpbinding.clone());
                }
            }
            trav = xml::next(tr);
        }
    }
    if ctx
        .sdl
        .borrow()
        .bindings
        .as_ref()
        .is_none_or(|b| b.is_empty())
    {
        return Err(soap_error(
            vm,
            "Parsing WSDL: Could not find any usable binding services in WSDL.",
        ));
    }
    Ok(())
}

pub fn ini_str(vm: &Vm, k: &str) -> String {
    vm.constants
        .get(k.as_bytes())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default()
}

pub fn ini_long(vm: &Vm, k: &str, default: i64) -> i64 {
    match vm.constants.get(k.as_bytes()) {
        Some(v) => crate::schema::atoi(&v.to_php_string().to_string_lossy()),
        None => default,
    }
}

pub fn ini_bool(vm: &Vm, k: &str) -> bool {
    let s = ini_str(vm, k).to_ascii_lowercase();
    matches!(s.trim(), "1" | "on" | "yes" | "true") || crate::schema::atoi(&s) != 0
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// get_sdl()
pub fn get_sdl(vm: &mut Vm, this: &Obj, uri: &str, cache_wsdl: i64) -> SR<SdlRef> {
    let mut cache_wsdl = cache_wsdl;
    let mut uri = uri.to_string();
    if !(uri.contains(':') || uri.starts_with('/')) {
        match std::fs::canonicalize(&uri) {
            Ok(p) => uri = p.to_string_lossy().into_owned(),
            Err(_) => cache_wsdl = WSDL_CACHE_NONE,
        }
    }
    let t = now();
    let ttl = ini_long(vm, "soap.wsdl_cache_ttl", 86400);
    if cache_wsdl & WSDL_CACHE_MEMORY != 0 {
        let hit = g(|g| g.mem_cache.get(&uri).cloned());
        if let Some((time, sdl)) = hit {
            if time < t - ttl {
                g(|g| {
                    g.mem_cache.shift_remove(&uri);
                });
            } else {
                return Ok(sdl);
            }
        }
    }
    let old_code = g(|g| g.error_code);
    g(|g| g.error_code = Some("WSDL"));
    // The context only matters to stream wrappers: local files are read
    // without one (and no context resource is allocated for them).
    let lower = uri.to_ascii_lowercase();
    let remote = uri.contains("://") && !lower.starts_with("file://");
    let orig_ctx = remote.then(|| {
        let new_ctx = crate::http_client::wsdl_context(vm, this, &uri);
        goro_ext_xml::libxml::switch_streams_context(Some(new_ctx))
    });
    let r = load_wsdl(vm, &uri);
    if let Some(orig) = orig_ctx {
        goro_ext_xml::libxml::switch_streams_context(orig);
    }
    g(|g| g.error_code = old_code);
    let sdl = r?;
    if cache_wsdl & WSDL_CACHE_MEMORY != 0 {
        let limit = ini_long(vm, "soap.wsdl_cache_limit", 5);
        let full = g(|g| limit > 0 && limit as usize <= g.mem_cache.len());
        if full {
            // in-memory cache overflow: drop the oldest entry
            let oldest = g(|g| {
                let mut latest = t;
                let mut key = None;
                for (k, (time, _)) in g.mem_cache.iter() {
                    if *time < latest {
                        latest = *time;
                        key = Some(k.clone());
                    }
                }
                key
            });
            match oldest {
                Some(k) => {
                    g(|g| {
                        g.mem_cache.shift_remove(&k);
                    });
                }
                None => return Ok(sdl),
            }
        }
        g(|g| {
            g.mem_cache.insert(uri.clone(), (t, sdl.clone()));
        });
    }
    Ok(sdl)
}

/// get_encoder_ex() for a "type" or "ns:type" string of the typemap.
pub fn typemap_base(sdl: Option<&SdlRef>, nscat: &str) -> Option<EncRef> {
    get_encoder_ex(sdl, nscat)
}

pub fn value_is_long(v: &Value) -> Option<i64> {
    match v.deref() {
        Value::Long(n) => Some(n),
        _ => None,
    }
}
