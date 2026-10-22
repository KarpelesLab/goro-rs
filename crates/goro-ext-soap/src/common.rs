//! Helpers of soap.c shared by the client and the server: parameter
//! serialization, function lookup, and the __getFunctions()/__getTypes()
//! descriptions.

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::encoding::master_to_xml;
use crate::php::*;
use crate::types::*;
use crate::xml::{self, Node};

/// serialize_parameter()
pub fn serialize_parameter(
    vm: &mut Vm,
    param: Option<&ParamRef>,
    param_val: Option<&Value>,
    index: usize,
    name: Option<&str>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let mut param_val = param_val.cloned();
    let mut name = name.map(|s| s.to_string());
    if let Some(Value::Object(o)) = param_val.as_ref().map(|v| v.deref()) {
        if o.borrow().class_name.eq_ignore_ascii_case(b"SoapParam") {
            let pn = prop(&o, "param_name");
            let pd = prop(&o, "param_data");
            if let (Value::String(s), false) = (&pn, matches!(pd, Value::Undef)) {
                param_val = Some(pd);
                name = Some(s.to_string_lossy());
            }
        }
    }
    let param_name = match param.and_then(|p| p.param_name.clone()) {
        Some(n) => n,
        None => match name {
            Some(n) => n,
            None => format!("param{}", index),
        },
    };
    serialize_zval(vm, param_val.as_ref(), param, &param_name, style, parent)
}

/// serialize_zval()
pub fn serialize_zval(
    vm: &mut Vm,
    val: Option<&Value>,
    param: Option<&ParamRef>,
    param_name: &str,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let mut val = val.cloned();
    let enc = param.and_then(|p| p.encode.clone());
    if let Some(p) = param {
        if val.is_none() {
            if let Some(el) = &p.element {
                let e = el.borrow();
                if let Some(f) = &e.fixed {
                    val = Some(vstr(f));
                } else if let (Some(d), false) = (&e.def, e.nillable) {
                    val = Some(vstr(d));
                }
            }
        }
    }
    let x = master_to_xml(vm, enc.as_ref(), val.as_ref(), style, parent)?;
    let nm = xml::name(x);
    if nm.is_empty() || nm == "BOGUS" {
        xml::set_name(x, param_name);
    }
    Ok(x)
}

/// get_param()
pub fn get_param(
    function: Option<&FuncRef>,
    param_name: Option<&str>,
    index: usize,
    response: bool,
) -> Option<ParamRef> {
    let f = function?;
    let ht = if response {
        f.response_params.as_ref()?
    } else {
        f.request_params.as_ref()?
    };
    match param_name {
        Some(n) => ht
            .iter()
            .find(|p| p.param_name.as_deref() == Some(n))
            .cloned(),
        None => ht.get(index).cloned(),
    }
}

/// get_function()
pub fn get_function(sdl: Option<&SdlRef>, name: &str) -> Option<FuncRef> {
    let s = sdl?.borrow();
    let lc = name.to_ascii_lowercase();
    if let Some(f) = s.functions.find(&lc) {
        return Some(f.clone());
    }
    s.requests.as_ref().and_then(|r| r.find(&lc).cloned())
}

/// get_doc_function()
pub fn get_doc_function(sdl: Option<&SdlRef>, params: Option<Node>) -> Option<FuncRef> {
    let s = sdl?.borrow();
    for (k, tmp) in s.functions.iter() {
        if !matches!(k, HKey::Str(_)) {
            continue;
        }
        let Some(fnb) = tmp.soap() else { continue };
        if fnb.style != SOAP_DOCUMENT {
            continue;
        }
        let rp = tmp.request_params.as_ref();
        match params {
            None => {
                if rp.is_none_or(|r| r.is_empty()) {
                    return Some(tmp.clone());
                }
            }
            Some(first) => {
                let Some(rp) = rp.filter(|r| !r.is_empty()) else {
                    continue;
                };
                let mut ok = true;
                let mut node = Some(first);
                for param in rp {
                    let Some(n) = node else {
                        // node->name of NULL would crash in C: treat as no match
                        ok = false;
                        break;
                    };
                    if let Some(el) = &param.element {
                        let e = el.borrow();
                        if e.name.as_deref() != Some(xml::name(n).as_str()) {
                            ok = false;
                            break;
                        }
                        let nns = xml::ns_href_of(n);
                        match (&e.namens, &nns) {
                            (Some(a), Some(b)) => {
                                if a != b {
                                    ok = false;
                                    break;
                                }
                            }
                            (None, None) => {}
                            _ => {
                                ok = false;
                                break;
                            }
                        }
                    } else if param.param_name.as_deref() != Some(xml::name(n).as_str()) {
                        ok = false;
                        break;
                    }
                    node = xml::next(n);
                }
                if ok {
                    return Some(tmp.clone());
                }
            }
        }
    }
    None
}

fn type_str_of(enc: &Option<EncRef>) -> Option<String> {
    enc.as_ref().and_then(|e| e.type_str())
}

/// function_to_string()
pub fn function_to_string(f: &SdlFunction) -> String {
    let mut buf = String::new();
    match &f.response_params {
        Some(rp) if !rp.is_empty() => {
            if rp.len() == 1 {
                match type_str_of(&rp[0].encode) {
                    Some(t) => {
                        buf.push_str(&t);
                        buf.push(' ');
                    }
                    None => buf.push_str("UNKNOWN "),
                }
            } else {
                buf.push_str("list(");
                for (i, p) in rp.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(", ");
                    }
                    buf.push_str(&type_str_of(&p.encode).unwrap_or_else(|| "UNKNOWN".into()));
                    buf.push_str(" $");
                    buf.push_str(p.param_name.as_deref().unwrap_or(""));
                }
                buf.push_str(") ");
            }
        }
        _ => buf.push_str("void "),
    }
    buf.push_str(&f.function_name);
    buf.push('(');
    if let Some(rp) = &f.request_params {
        for (i, p) in rp.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            buf.push_str(&type_str_of(&p.encode).unwrap_or_else(|| "UNKNOWN".into()));
            buf.push_str(" $");
            buf.push_str(p.param_name.as_deref().unwrap_or(""));
        }
    }
    buf.push(')');
    buf
}

fn model_to_string(model: &ModelRef, buf: &mut String, level: usize) {
    let (kind, u) = {
        let m = model.borrow();
        (m.kind, m.u.clone())
    };
    match kind {
        XSD_CONTENT_ELEMENT => {
            if let ModelU::Element(e) = u {
                type_to_string(&e, buf, level);
                buf.push_str(";\n");
            }
        }
        XSD_CONTENT_ANY => {
            for _ in 0..level {
                buf.push(' ');
            }
            buf.push_str("<anyXML> any;\n");
        }
        XSD_CONTENT_SEQUENCE | XSD_CONTENT_ALL | XSD_CONTENT_CHOICE => {
            if let ModelU::Content(c) = u {
                for t in &c {
                    model_to_string(t, buf, level);
                }
            }
        }
        XSD_CONTENT_GROUP => {
            if let ModelU::Group(gr) = u {
                let m = gr.borrow().model.clone();
                if let Some(m) = m {
                    model_to_string(&m, buf, level);
                }
            }
        }
        _ => {}
    }
}

fn extra(t: &SdlType, attr_key: &str, ext_key: &str) -> Option<ExtraAttr> {
    let a = t.attributes.as_ref()?.find(attr_key)?.clone();
    let a = a.borrow();
    a.extra.as_ref()?.find(ext_key).cloned()
}

/// type_to_string()
pub fn type_to_string(type_: &TypeRef, buf: &mut String, level: usize) {
    let t = type_.borrow().clone();
    let spaces: String = " ".repeat(level);
    buf.push_str(&spaces);
    let name = t.name.clone().unwrap_or_default();
    match t.kind {
        XSD_TYPEKIND_SIMPLE => {
            match &t.encode {
                Some(e) => {
                    buf.push_str(&e.type_str().unwrap_or_default());
                    buf.push(' ');
                }
                None => buf.push_str("anyType "),
            }
            buf.push_str(&name);
            if let Some(en) = t.restrictions.as_ref().and_then(|r| r.enumeration.as_ref()) {
                buf.push_str(" {");
                let mut first = true;
                for (k, _) in en.iter() {
                    if let HKey::Str(k) = k {
                        if first {
                            first = false;
                        } else {
                            buf.push_str(", ");
                        }
                        buf.push_str(k);
                    }
                }
                buf.push('}');
            }
        }
        XSD_TYPEKIND_LIST => {
            buf.push_str("list ");
            buf.push_str(&name);
            if let Some(els) = &t.elements {
                buf.push_str(" {");
                for it in els.values() {
                    buf.push_str(&it.borrow().name.clone().unwrap_or_default());
                }
                buf.push('}');
            }
        }
        XSD_TYPEKIND_UNION => {
            buf.push_str("union ");
            buf.push_str(&name);
            if let Some(els) = &t.elements {
                buf.push_str(" {");
                for it in els.values() {
                    // `first` is never set in the C code: a comma before each
                    buf.push(',');
                    buf.push_str(&it.borrow().name.clone().unwrap_or_default());
                }
                buf.push('}');
            }
        }
        XSD_TYPEKIND_COMPLEX | XSD_TYPEKIND_RESTRICTION | XSD_TYPEKIND_EXTENSION => {
            let is_array = t
                .encode
                .as_ref()
                .is_some_and(|e| e.type_() == IS_ARRAY || e.type_() == SOAP_ENC_ARRAY);
            if is_array {
                if let Some(ext) = extra(
                    &t,
                    &format!("{}:arrayType", SOAP_1_1_ENC_NAMESPACE),
                    &format!("{}:arrayType", WSDL_NAMESPACE),
                ) {
                    let (head, end) = match ext.val.find('[') {
                        Some(p) => (&ext.val[..p], Some(&ext.val[p..])),
                        None => (&ext.val[..], None),
                    };
                    if head.is_empty() {
                        buf.push_str("anyType");
                    } else {
                        buf.push_str(head);
                    }
                    buf.push(' ');
                    buf.push_str(&name);
                    if let Some(e) = end {
                        buf.push_str(e);
                    }
                } else {
                    if let Some(ext) = extra(
                        &t,
                        &format!("{}:itemType", SOAP_1_2_ENC_NAMESPACE),
                        &format!("{}:itemType", WSDL_NAMESPACE),
                    ) {
                        buf.push_str(&ext.val);
                        buf.push(' ');
                    } else if let Some(ets) = t
                        .elements
                        .as_ref()
                        .filter(|e| e.len() == 1)
                        .and_then(|e| e.first().cloned())
                        .and_then(|el| type_str_of(&el.borrow().encode))
                    {
                        buf.push_str(&ets);
                        buf.push(' ');
                    } else {
                        buf.push_str("anyType ");
                    }
                    buf.push_str(&name);
                    if let Some(ext) = extra(
                        &t,
                        &format!("{}:arraySize", SOAP_1_2_ENC_NAMESPACE),
                        &format!("{}:itemType", WSDL_NAMESPACE),
                    ) {
                        buf.push('[');
                        buf.push_str(&ext.val);
                        buf.push(']');
                    } else {
                        buf.push_str("[]");
                    }
                }
            } else {
                buf.push_str("struct ");
                buf.push_str(&name);
                buf.push(' ');
                buf.push_str("{\n");
                if (t.kind == XSD_TYPEKIND_RESTRICTION || t.kind == XSD_TYPEKIND_EXTENSION)
                    && t.encode.is_some()
                {
                    let mut enc = t.encode.clone();
                    let mut guard = 0;
                    while let Some(e) = enc.clone() {
                        guard += 1;
                        if guard > 1000 {
                            break;
                        }
                        let Some(st) = e.sdl_type() else { break };
                        let (k, senc) = {
                            let s = st.borrow();
                            (s.kind, s.encode.clone())
                        };
                        if senc.as_ref().is_some_and(|x| same_enc(x, &e))
                            || matches!(
                                k,
                                XSD_TYPEKIND_SIMPLE | XSD_TYPEKIND_LIST | XSD_TYPEKIND_UNION
                            )
                        {
                            break;
                        }
                        enc = senc;
                    }
                    if enc.is_some() {
                        buf.push_str(&spaces);
                        buf.push(' ');
                        buf.push_str(&type_str_of(&t.encode).unwrap_or_default());
                        buf.push_str(" _;\n");
                    }
                }
                if let Some(m) = &t.model {
                    model_to_string(m, buf, level + 1);
                }
                if let Some(attrs) = &t.attributes {
                    for a in attrs.values() {
                        let a = a.borrow();
                        buf.push_str(&spaces);
                        buf.push(' ');
                        match type_str_of(&a.encode) {
                            Some(ts) => {
                                buf.push_str(&ts);
                                buf.push(' ');
                            }
                            None => buf.push_str("UNKNOWN "),
                        }
                        buf.push_str(a.name.as_deref().unwrap_or(""));
                        buf.push_str(";\n");
                    }
                }
                buf.push_str(&spaces);
                buf.push('}');
            }
        }
        _ => {}
    }
}
