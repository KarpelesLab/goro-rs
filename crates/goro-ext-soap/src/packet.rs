//! php_packet_soap.c: parsing the response of a SOAP call.

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_xml::tree::NType;

use crate::encoding::{conv, master_to_zval};
use crate::php::*;
use crate::types::*;
use crate::xml::{self, Node};

fn fault(vm: &mut Vm, this: &Obj, code: &str, msg: &str, doc: Node) -> SR<bool> {
    add_soap_fault(vm, this, code, msg);
    xml::free_doc(doc);
    Ok(false)
}

fn check_enc_attrs(
    vm: &mut Vm,
    this: &Obj,
    node: Node,
    version: i32,
    what: &str,
    doc: Node,
    ns_required: bool,
) -> SR<Option<bool>> {
    let mut a = xml::properties(node);
    while let Some(at) = a {
        if xml::node_ns(at).is_none() {
            if ns_required || version == SOAP_1_2 {
                let m = format!(
                    "A SOAP {} element cannot have non Namespace qualified attributes",
                    what
                );
                return fault(vm, this, "Client", &m, doc).map(Some);
            }
        } else if xml::attr_is_equal_ex(at, "encodingStyle", Some(SOAP_1_2_ENV_NAMESPACE)) {
            if version == SOAP_1_2 {
                let m = format!("encodingStyle cannot be specified on the {}", what);
                return fault(vm, this, "Client", &m, doc).map(Some);
            } else if xml::attr_value(at) != SOAP_1_1_ENC_NAMESPACE {
                return fault(vm, this, "Client", "Unknown data encoding style", doc).map(Some);
            }
        }
        a = xml::next(at);
    }
    Ok(None)
}

/// parse_packet_soap(): Ok(true) with the return value, Ok(false) when a
/// fault was set on the client.
pub fn parse_packet_soap(
    vm: &mut Vm,
    this: &Obj,
    buffer: &[u8],
    fn_: Option<&FuncRef>,
    return_value: &mut Value,
    soap_headers: Option<&Arr>,
) -> SR<bool> {
    *return_value = Value::Null;
    if buffer.is_empty() {
        return Ok(true);
    }
    let Some(response) = xml::parse_memory(buffer) else {
        add_soap_fault(vm, this, "Client", "looks like we got no XML document");
        return Ok(false);
    };
    if xml::has_int_subset(response) {
        return fault(
            vm,
            this,
            "Client",
            "DTD are not supported by SOAP",
            response,
        );
    }
    let mut env: Option<Node> = None;
    let mut envelope_ns = "";
    let mut soap_version = SOAP_1_1;
    let mut trav = xml::children(response);
    while let Some(tr) = trav {
        if xml::is_element(tr) {
            if env.is_none()
                && xml::node_is_equal_ex(tr, Some("Envelope"), Some(SOAP_1_1_ENV_NAMESPACE))
            {
                env = Some(tr);
                envelope_ns = SOAP_1_1_ENV_NAMESPACE;
                soap_version = SOAP_1_1;
            } else if env.is_none()
                && xml::node_is_equal_ex(tr, Some("Envelope"), Some(SOAP_1_2_ENV_NAMESPACE))
            {
                env = Some(tr);
                envelope_ns = SOAP_1_2_ENV_NAMESPACE;
                soap_version = SOAP_1_2;
            } else {
                return fault(vm, this, "VersionMismatch", "Wrong Version", response);
            }
        }
        trav = xml::next(tr);
    }
    let Some(env) = env else {
        return fault(
            vm,
            this,
            "Client",
            "looks like we got XML without \"Envelope\" element",
            response,
        );
    };
    if let Some(r) = check_enc_attrs(vm, this, env, soap_version, "Envelope", response, true)? {
        return Ok(r);
    }
    // <Header>
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
    // <Body>
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
        return fault(
            vm,
            this,
            "Client",
            "Body must be present in a SOAP envelope",
            response,
        );
    };
    if let Some(r) = check_enc_attrs(vm, this, body, soap_version, "Body", response, false)? {
        return Ok(r);
    }
    if trav.is_some() && soap_version == SOAP_1_2 {
        return fault(
            vm,
            this,
            "Client",
            "A SOAP 1.2 envelope can contain only Header and Body",
            response,
        );
    }
    if let Some(h) = head {
        if let Some(r) = check_enc_attrs(vm, this, h, soap_version, "Header", response, true)? {
            return Ok(r);
        }
    }
    // <Fault>
    if let Some(f) = xml::get_node_ex(xml::children(body), "Fault", Some(envelope_ns)) {
        let mut faultcode: Option<String> = None;
        let mut faultstring: Option<Vec<u8>> = None;
        let mut faultactor: Option<Vec<u8>> = None;
        let mut lang = String::new();
        let mut details = Value::Undef;
        let string_conv = conv(IS_STRING);
        if soap_version == SOAP_1_1 {
            if let Some(t) = xml::get_node(xml::children(f), "faultcode") {
                if let Some(c) = xml::children(t) {
                    faultcode = Some(xml::content(c));
                }
            }
            if let Some(t) = xml::get_node(xml::children(f), "faultstring") {
                if xml::children(t).is_some() {
                    let zv = master_to_zval(vm, Some(&string_conv), t)?;
                    faultstring = Some(crate::encoding::zval_get_string(vm, &zv)?);
                }
            }
            if let Some(t) = xml::get_node(xml::children(f), "faultactor") {
                if xml::children(t).is_some() {
                    let zv = master_to_zval(vm, Some(&string_conv), t)?;
                    faultactor = Some(crate::encoding::zval_get_string(vm, &zv)?);
                }
            }
            if let Some(t) = xml::get_node(xml::children(f), "detail") {
                details = master_to_zval(vm, None, t)?;
            }
        } else {
            if let Some(t) = xml::get_node(xml::children(f), "Code") {
                if xml::children(t).is_some() {
                    if let Some(v) = xml::get_node(xml::children(t), "Value") {
                        if let Some(c) = xml::children(v) {
                            faultcode = Some(xml::content(c));
                        }
                    }
                }
            }
            if let Some(t) = xml::get_node(xml::children(f), "Reason") {
                if xml::children(t).is_some() {
                    if let Some(tx) = xml::get_node(xml::children(t), "Text") {
                        if xml::children(tx).is_some() {
                            let zv = master_to_zval(vm, Some(&string_conv), tx)?;
                            faultstring = Some(crate::encoding::zval_get_string(vm, &zv)?);
                            if let Some(la) = xml::get_attribute_ex(
                                xml::properties(tx),
                                "lang",
                                Some(XML_NAMESPACE),
                            ) {
                                if xml::attr_has_value(la) {
                                    lang = xml::attr_value(la);
                                }
                            }
                        }
                    }
                }
            }
            if let Some(t) = xml::get_node(xml::children(f), "Detail") {
                details = master_to_zval(vm, None, t)?;
            }
        }
        add_soap_fault_ex_code(
            vm,
            this,
            faultcode.as_deref().map(str::as_bytes),
            faultstring.as_deref(),
            faultactor.as_deref(),
            &details,
            lang.as_bytes(),
        );
        xml::free_doc(response);
        return Ok(false);
    }
    // the content of <Body>
    let ret = new_arr();
    let mut param_count = 0;
    let mut hdrs: Option<Ht<HeaderRef>> = None;
    let mut resp = xml::children(body);
    while let Some(r) = resp.filter(|r| !xml::is_element(*r)) {
        resp = xml::next(r);
    }
    if let Some(resp) = resp {
        if let Some(fb) = fn_.and_then(|f| f.soap()) {
            let fn_ = fn_.unwrap();
            hdrs = fb.output.headers.clone();
            if let Some(rp) = &fn_.response_params {
                let res_count = rp.len();
                for param in rp {
                    let (name, ns): (Option<String>, Option<String>) = if fb.style == SOAP_DOCUMENT
                    {
                        match &param.element {
                            Some(el) => {
                                let e = el.borrow();
                                (e.name.clone(), e.namens.clone())
                            }
                            None => (param.param_name.clone(), None),
                        }
                    } else {
                        (fn_.response_name.clone(), None)
                    };
                    let name = name.unwrap_or_default();
                    let mut cur = xml::get_node_ex(Some(resp), &name, ns.as_deref());
                    if cur.is_none() {
                        cur = xml::get_node(Some(resp), &name);
                    }
                    if cur.is_none() && fb.style == SOAP_RPC {
                        cur = Some(resp);
                    }
                    let mut val: Option<Node> = None;
                    if let Some(c) = cur {
                        if fb.style == SOAP_DOCUMENT {
                            val = Some(c);
                        } else {
                            let pn = param.param_name.clone().unwrap_or_default();
                            val = xml::get_node(xml::children(c), &pn);
                            if res_count == 1 {
                                if val.is_none() {
                                    val = xml::get_node(xml::children(c), "return");
                                }
                                if val.is_none() {
                                    val = xml::get_node(xml::children(c), "result");
                                }
                                if val.is_none() {
                                    if let Some(ch) = xml::children(c) {
                                        if xml::next(ch).is_none() {
                                            val = Some(ch);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let tmp = match val {
                        None => Value::Null,
                        Some(v) => master_to_zval(vm, param.encode.as_ref(), v)?,
                    };
                    arr_set_str(
                        &mut ret.borrow_mut(),
                        param.param_name.as_deref().unwrap_or(""),
                        tmp,
                    );
                    param_count += 1;
                }
            }
        } else {
            let mut val = xml::children(resp);
            while val.is_some() {
                while let Some(v) = val.filter(|v| !xml::is_element(*v)) {
                    val = xml::next(v);
                }
                let Some(v) = val else { break };
                if !xml::node_is_equal_ex(v, Some("result"), Some(RPC_SOAP12_NAMESPACE)) {
                    let tmp = master_to_zval(vm, None, v)?;
                    let vname = xml::name(v);
                    let key = symtable_key(vname.as_bytes());
                    let existing = ret.borrow().get(&key).cloned();
                    match existing {
                        Some(Value::Array(a)) => a.borrow_mut().push(tmp),
                        Some(_) => {
                            // add_next_index_zval() on a non-array fails
                        }
                        None => {
                            if xml::next(v).is_some()
                                && xml::get_node(xml::next(v), &vname).is_some()
                            {
                                let mut arr = PhpArray::new();
                                arr.push(tmp);
                                ret.borrow_mut().set(key, varr(arr));
                            } else {
                                ret.borrow_mut().set(key, tmp);
                            }
                        }
                    }
                    param_count += 1;
                }
                val = xml::next(v);
            }
        }
    }
    *return_value = if param_count == 0 {
        Value::Null
    } else if param_count == 1 {
        ret.borrow().values().next().cloned().unwrap_or(Value::Null)
    } else {
        Value::Array(ret)
    };
    if let (Some(sh), Some(head)) = (soap_headers, head) {
        let mut trav = xml::children(head);
        while let Some(tr) = trav {
            if xml::ntype(tr) == NType::Element {
                let mut enc: Option<EncRef> = None;
                if let Some(h) = &hdrs {
                    let mut key = String::new();
                    if let Some(href) = xml::ns_href_of(tr) {
                        key.push_str(&href);
                        key.push(':');
                    }
                    key.push_str(&xml::name(tr));
                    if let Some(hdr) = h.find(&key) {
                        enc = hdr.encode.clone();
                    }
                }
                let val = master_to_zval(vm, enc.as_ref(), tr)?;
                arr_set_str(&mut sh.borrow_mut(), &xml::name(tr), val);
            }
            trav = xml::next(tr);
        }
    }
    xml::free_doc(response);
    Ok(true)
}

/// add_soap_fault() with a possibly empty fault code and an UNDEF detail.
fn add_soap_fault_ex_code(
    vm: &mut Vm,
    this: &Obj,
    code: Option<&[u8]>,
    string: Option<&[u8]>,
    actor: Option<&[u8]>,
    details: &Value,
    lang: &[u8],
) {
    let d = if matches!(details, Value::Undef) {
        None
    } else {
        Some(details)
    };
    add_soap_fault_opt(vm, this, code, string, actor, d, lang);
}

pub fn unused(_k: ArrayKey) {}
