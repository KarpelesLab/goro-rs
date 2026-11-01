//! Decoding SOAP XML into PHP values: the built-in datatypes, SOAP-ENC
//! arrays (offsets, positions, multi-dimensional), structs, Apache maps,
//! the WSDL's types, `href`/`id` (SOAP 1.1) and `enc:ref`/`enc:id` (SOAP
//! 1.2) references, classmaps and typemaps. PHP's results (stdClass for
//! structs, repeated elements as arrays, numeric overflow to float...)
//! follow the ext/soap tests and observations on PHP 8.5.

use std::collections::HashMap;
use std::rc::Rc;

use goro_runtime::{Key, PhpArray, PhpStr, Value};
use goro_xml::NodeId;

use super::encode::{SOAP_SINGLE_ELEMENT_ARRAYS, TypeMapEntry, array_info};
use super::sdl::{Deriv, Enc, Kind, MKind, Model, Sdl, TId};
use super::types::{self, Conv};
use super::xmlin::InDoc;
use super::xmlout::{NS_SOAP11_ENC, NS_SOAP12_ENC, NS_XSI};
use super::{SoapState, soap_error};
use crate::vm::{R, Vm};

pub struct Dx<'a> {
    pub d: &'a InDoc,
    pub sdl: Option<Rc<Sdl>>,
    pub version: i64,
    pub features: i64,
    pub encoding: Option<String>,
    pub classmap: Vec<(String, String)>,
    pub typemap: Rc<Vec<TypeMapEntry>>,
    pub ids: HashMap<String, NodeId>,
    pub done: HashMap<NodeId, Value>,
    /// Inside a repeating model group.
    pub rep: u32,
    /// The content model being decoded has an `<any>`.
    pub pending_any: bool,
}

impl<'a> Dx<'a> {
    pub fn new(d: &'a InDoc, sdl: Option<Rc<Sdl>>, version: i64, st: &SoapState) -> Dx<'a> {
        let mut ids = HashMap::new();
        if let Some(r) = d.root() {
            collect_ids(d, r, &mut ids);
        }
        Dx {
            d,
            sdl,
            version,
            features: st.features,
            encoding: st.encoding.clone(),
            classmap: st.classmap.clone(),
            typemap: st.typemap.clone(),
            ids,
            done: HashMap::new(),
            rep: 0,
            pending_any: false,
        }
    }
}

fn collect_ids(d: &InDoc, n: NodeId, ids: &mut HashMap<String, NodeId>) {
    if let Some(id) = d.attr(n, "id") {
        ids.entry(id).or_insert(n);
    }
    if let Some(id) = d.attr_ns(n, NS_SOAP12_ENC, "id") {
        ids.entry(id).or_insert(n);
    }
    for c in d.kids(n) {
        collect_ids(d, c, ids);
    }
}

/// The text of a node (its text children).
pub fn node_text(d: &InDoc, n: NodeId) -> String {
    d.text(n)
}

fn has_element_kids(d: &InDoc, n: NodeId) -> bool {
    !d.kids(n).is_empty()
}

/// Converts decoded UTF-8 text for the `encoding` option.
fn out_str(dx: &Dx, s: &str) -> Value {
    match &dx.encoding {
        Some(e) => Value::Str(PhpStr::from(super::utf8_to_charset(e, s))),
        None => Value::Str(PhpStr::from(s)),
    }
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn violation<T>(vm: &mut Vm) -> R<T> {
    Err(soap_error(vm, "Encoding: Violation of encoding rules"))
}

/// The xsi:type of a node, resolved.
fn xsi_type(d: &InDoc, n: NodeId) -> Option<(String, String)> {
    let t = d.attr_ns(n, NS_XSI, "type")?;
    d.resolve_qname(n, &t)
}

fn is_nil(d: &InDoc, n: NodeId) -> bool {
    matches!(
        d.attr_ns(n, NS_XSI, "nil").as_deref().map(str::trim),
        Some("true") | Some("1")
    )
}

/// Decodes node `n` with encoding `enc` (`None`: by xsi:type or guessed).
pub fn from_xml(vm: &mut Vm, dx: &mut Dx, enc: Option<Enc>, n: NodeId) -> R<Value> {
    let d = dx.d;
    // References.
    let href = if dx.version == 2 {
        d.attr_ns(n, NS_SOAP12_ENC, "ref")
    } else {
        d.attr(n, "href")
    };
    if let Some(h) = href {
        if dx.version == 2 && d.attr_ns(n, NS_SOAP12_ENC, "id").is_some() {
            return Err(soap_error(
                vm,
                &format!("Encoding: Violation of id and ref information items '{h}'"),
            ));
        }
        let id = h.strip_prefix('#').unwrap_or(&h).to_string();
        let Some(&target) = dx.ids.get(&id) else {
            return Err(soap_error(
                vm,
                &format!("Encoding: Unresolved reference '{h}'"),
            ));
        };
        if let Some(v) = dx.done.get(&target) {
            return Ok(v.clone());
        }
        let v = decode_node(vm, dx, enc, target)?;
        dx.done.insert(target, v.clone());
        return Ok(v);
    }
    if let Some(v) = dx.done.get(&n) {
        return Ok(v.clone());
    }
    let has_id = d.attr(n, "id").is_some() || d.attr_ns(n, NS_SOAP12_ENC, "id").is_some();
    let v = decode_node(vm, dx, enc, n)?;
    if has_id {
        dx.done.insert(n, v.clone());
    }
    Ok(v)
}

fn decode_node(vm: &mut Vm, dx: &mut Dx, enc: Option<Enc>, n: NodeId) -> R<Value> {
    let d = dx.d;
    let mut enc = enc;
    // No declared type works like anyType (observed on PHP 8.5; test
    // bugs/bug73237.phpt).
    if enc.is_none() {
        return any_type(vm, dx, n);
    }
    if let Some(Enc::Builtin(b)) = enc
        && b.conv == Conv::AnyType
    {
        return any_type(vm, dx, n);
    }
    // xsi:type names the encoding when it is known.
    if let Some((ns, name)) = xsi_type(d, n) {
        let found = super::encode::enc_for(dx.sdl.as_deref(), &ns, &name);
        let user = dx.typemap.iter().any(|t| t.ns == ns && t.name == name);
        if let Some(f) = found {
            // A WSDL type stays when xsi:type names a built-in it derives
            // from... PHP takes the instance's type.
            enc = Some(f);
        } else if !user && enc.is_none() {
            enc = None;
        }
        if user {
            return typemap_from_xml(vm, dx, &ns, &name, n);
        }
    }
    if let Some((ns, name)) = enc_qname(dx, enc)
        && dx
            .typemap
            .iter()
            .any(|t| t.ns == ns && t.name == name && t.from_xml.is_some())
    {
        return typemap_from_xml(vm, dx, &ns, &name, n);
    }
    if is_nil(d, n) {
        return nil_value(vm, dx, enc, n);
    }
    match enc {
        None => guess(vm, dx, n),
        Some(Enc::Builtin(b)) => builtin(vm, dx, b.conv, n),
        Some(Enc::Type(t)) => wsdl_type(vm, dx, t, n),
    }
}

fn enc_qname(dx: &Dx, enc: Option<Enc>) -> Option<(String, String)> {
    if dx.typemap.is_empty() {
        return None;
    }
    match enc? {
        Enc::Builtin(b) => Some((b.ns.to_string(), b.name.to_string())),
        Enc::Type(t) => {
            let sdl = dx.sdl.as_ref()?;
            let mut ty = &sdl.types[t];
            if ty.kind == Kind::Element
                && let Some(Enc::Type(b)) = ty.base.as_ref().and_then(|b| sdl.resolve(b))
            {
                ty = &sdl.types[b];
            }
            Some((ty.ns.clone(), ty.name.clone()))
        }
    }
}

fn typemap_from_xml(vm: &mut Vm, dx: &mut Dx, ns: &str, name: &str, n: NodeId) -> R<Value> {
    let f = dx
        .typemap
        .iter()
        .find(|t| t.ns == ns && t.name == name)
        .and_then(|t| t.from_xml.clone());
    let Some(f) = f else {
        return guess(vm, dx, n);
    };
    let xml = dx.d.to_xml_fixup(n);
    vm.call_value(&f, vec![Value::Str(PhpStr::from(xml))])
}

/// anyType: the instance's xsi:type; a WSDL type's value comes back as a
/// SoapVar keeping the type (observed on PHP 8.5: enc_type 0, enc_stype
/// and enc_ns set; test interop/Round4/GroupI/r4_groupI_xsd_028w.phpt).
fn any_type(vm: &mut Vm, dx: &mut Dx, n: NodeId) -> R<Value> {
    let d = dx.d;
    if is_nil(d, n) {
        return Ok(Value::Null);
    }
    let Some((ns, name)) = xsi_type(d, n) else {
        return guess(vm, dx, n);
    };
    if dx.typemap.iter().any(|t| t.ns == ns && t.name == name) {
        return typemap_from_xml(vm, dx, &ns, &name, n);
    }
    match super::encode::enc_for(dx.sdl.as_deref(), &ns, &name) {
        Some(Enc::Builtin(b)) if b.conv == Conv::AnyType => guess(vm, dx, n),
        Some(Enc::Builtin(b)) => builtin(vm, dx, b.conv, n),
        Some(Enc::Type(t)) => {
            let v = wsdl_type(vm, dx, t, n)?;
            let cls = vm.find_class(b"SoapVar", false)?.expect("SoapVar");
            let o = vm.instantiate(&cls)?;
            vm.set_prop_raw(&o, b"enc_type", Value::Int(0));
            vm.set_prop_raw(&o, b"enc_value", v);
            vm.set_prop_raw(&o, b"enc_stype", Value::Str(PhpStr::from(name)));
            vm.set_prop_raw(&o, b"enc_ns", Value::Str(PhpStr::from(ns)));
            Ok(Value::Object(o))
        }
        None => guess(vm, dx, n),
    }
}

/// No type information: arrays, structs or strings by the node's shape.
fn guess(vm: &mut Vm, dx: &mut Dx, n: NodeId) -> R<Value> {
    let d = dx.d;
    let enc11 = d.attr_ns(n, NS_SOAP11_ENC, "arrayType").is_some();
    let enc12 = d.attr_ns(n, NS_SOAP12_ENC, "itemType").is_some()
        || d.attr_ns(n, NS_SOAP12_ENC, "arraySize").is_some();
    if enc11 || enc12 {
        return soap_array(vm, dx, None, n);
    }
    if let Some((ns, name)) = xsi_type(d, n) {
        if (ns == NS_SOAP11_ENC || ns == NS_SOAP12_ENC) && name == "Array" {
            return soap_array(vm, dx, None, n);
        }
        if ns == super::xmlout::NS_APACHE && name == "Map" {
            return map(vm, dx, n);
        }
    }
    if has_element_kids(d, n) {
        return generic_struct(vm, dx, n, None);
    }
    // Text (or nothing).
    let t = node_text(d, n);
    Ok(out_str(dx, &t))
}

/// A struct without a content model: the children by name, repeated
/// names collected into arrays.
fn generic_struct(vm: &mut Vm, dx: &mut Dx, n: NodeId, class: Option<String>) -> R<Value> {
    let d = dx.d;
    let obj = new_object(vm, dx, class.as_deref())?;
    register_early(dx, n, &obj);
    let mut seen: Vec<(String, bool)> = Vec::new();
    for c in d.kids(n) {
        let name = d.local(c).to_string();
        let v = from_xml(vm, dx, None, c)?;
        add_prop(vm, &obj, &name, v, &mut seen);
    }
    Ok(Value::Object(obj))
}

/// A struct with an id is known before its content is decoded, so
/// references to it from inside resolve to it (observed on PHP 8.5; test
/// bugs/bug55323.phpt).
fn register_early(dx: &mut Dx, n: NodeId, obj: &goro_runtime::ObjRef) {
    let d = dx.d;
    if d.attr(n, "id").is_some() || d.attr_ns(n, NS_SOAP12_ENC, "id").is_some() {
        dx.done.insert(n, Value::Object(obj.clone()));
    }
}

/// Adds a property; repeated names become arrays.
fn add_prop(
    vm: &mut Vm,
    obj: &goro_runtime::ObjRef,
    name: &str,
    v: Value,
    seen: &mut Vec<(String, bool)>,
) {
    // A property the object already has (a classmapped class's default)
    // becomes an array too (observed on PHP 8.5; test
    // bugs/segfault_assertion_props.phpt).
    if !seen.iter().any(|s| s.0 == name) {
        let cur = vm.get_prop_raw(obj, name.as_bytes());
        if !matches!(cur, Value::Null) {
            let mut arr = match cur {
                Value::Array(a) => a,
                other => {
                    let mut a = PhpArray::new();
                    let _ = a.push(other);
                    a
                }
            };
            let _ = arr.push(v);
            seen.push((name.to_string(), true));
            set_prop(vm, obj, name, Value::Array(arr));
            return;
        }
    }
    if let Some(s) = seen.iter_mut().find(|s| s.0 == name) {
        let cur = vm.get_prop_raw(obj, name.as_bytes());
        let mut arr = if s.1 {
            match cur {
                Value::Array(a) => a,
                _ => PhpArray::new(),
            }
        } else {
            let mut a = PhpArray::new();
            let _ = a.push(cur);
            a
        };
        let _ = arr.push(v);
        s.1 = true;
        set_prop(vm, obj, name, Value::Array(arr));
    } else {
        seen.push((name.to_string(), false));
        set_prop(vm, obj, name, v);
    }
}

pub fn set_prop(vm: &mut Vm, obj: &goro_runtime::ObjRef, name: &str, v: Value) {
    // Undeclared properties of a class with __set() go through it
    // (observed on PHP 8.5; test classmap004.phpt).
    let cls = vm.class_of(obj);
    if vm.find_prop_slot_any(&cls, name.as_bytes()).is_none() && cls.find_method(b"__set").is_some()
    {
        let _ = vm.write_prop(obj, &PhpStr::from(name), v, None);
        return;
    }
    vm.set_prop_raw(obj, name.as_bytes(), v);
}

/// A new object of the classmap's class for `type_name`, or stdClass.
fn new_object(vm: &mut Vm, dx: &Dx, type_name: Option<&str>) -> R<goro_runtime::ObjRef> {
    new_object_ns(vm, dx, type_name, None)
}

/// A new object for a type: the classmap's "{ns}name" entry, else
/// "name" (PHP manual: SoapClient classmap option).
fn new_object_ns(
    vm: &mut Vm,
    dx: &Dx,
    type_name: Option<&str>,
    ns: Option<&str>,
) -> R<goro_runtime::ObjRef> {
    let full = match (type_name, ns) {
        (Some(t), Some(n)) => Some(format!("{{{n}}}{t}")),
        _ => None,
    };
    let entry = full
        .as_ref()
        .and_then(|f| dx.classmap.iter().find(|(k, _)| k == f))
        .or_else(|| type_name.and_then(|t| dx.classmap.iter().find(|(k, _)| k == t)));
    if let Some((_, c)) = entry {
        let c = c.clone();
        match vm.find_class(c.as_bytes(), true)? {
            Some(cls) => {
                let o = vm.instantiate(&cls)?;
                return Ok(o);
            }
            None => {
                return Err(soap_error(vm, &format!("Encoding: Class '{c}' not found")));
            }
        }
    }
    vm.new_std_object()
}

fn trimmed(d: &InDoc, n: NodeId) -> String {
    node_text(d, n).trim().to_string()
}

pub fn parse_int(s: &str) -> Option<i64> {
    let t = s.strip_prefix('+').unwrap_or(s);
    if t.is_empty() {
        return None;
    }
    t.parse::<i64>().ok()
}

fn builtin(vm: &mut Vm, dx: &mut Dx, conv: Conv, n: NodeId) -> R<Value> {
    let d = dx.d;
    // Elements where a simple value is expected (observed on PHP 8.5;
    // tests soap12/T27.phpt, T58.phpt).
    if !matches!(
        conv,
        Conv::AnyType | Conv::AnyXml | Conv::Array | Conv::Struct | Conv::Map
    ) && has_element_kids(d, n)
    {
        return violation(vm);
    }
    match conv {
        Conv::String | Conv::Duration | Conv::Decimal => {
            // Elements where a string is expected (observed on PHP 8.5;
            // test soap12/T27.phpt).
            if has_element_kids(d, n) {
                return violation(vm);
            }
            let t = node_text(d, n);
            Ok(out_str(dx, &t))
        }
        Conv::Token => {
            let t = collapse(&node_text(d, n));
            Ok(out_str(dx, &t))
        }
        Conv::Normalized => {
            let t: String = node_text(d, n)
                .chars()
                .map(|c| {
                    if matches!(c, '\t' | '\n' | '\r') {
                        ' '
                    } else {
                        c
                    }
                })
                .collect();
            Ok(out_str(dx, &t))
        }
        Conv::Boolean => {
            let t = trimmed(d, n);
            match t.as_str() {
                "true" | "1" | "TRUE" | "True" => Ok(Value::Bool(true)),
                "false" | "0" | "FALSE" | "False" => Ok(Value::Bool(false)),
                "" => Ok(Value::Bool(false)),
                _ => Ok(Value::Bool(vm.to_bool(&Value::Str(PhpStr::from(t))))),
            }
        }
        Conv::Long | Conv::BigInt => {
            let t = trimmed(d, n);
            if t.is_empty() {
                return Ok(Value::Null);
            }
            if let Some(i) = parse_int(&t) {
                return Ok(Value::Int(i));
            }
            match parse_float(&t) {
                Some(f) => {
                    if conv == Conv::Long && f.fract() == 0.0 && f.abs() < 9.2e18 {
                        Ok(Value::Int(f as i64))
                    } else {
                        Ok(Value::Float(f))
                    }
                }
                None => violation(vm),
            }
        }
        Conv::Double => {
            let t = trimmed(d, n);
            if t.is_empty() {
                return Ok(Value::Null);
            }
            match t.as_str() {
                "NaN" | "NAN" => return Ok(Value::Float(f64::NAN)),
                "INF" => return Ok(Value::Float(f64::INFINITY)),
                "-INF" => return Ok(Value::Float(f64::NEG_INFINITY)),
                _ => {}
            }
            if let Some(i) = parse_int(&t) {
                return Ok(Value::Int(i));
            }
            match parse_float(&t) {
                Some(f) => Ok(Value::Float(f)),
                None => violation(vm),
            }
        }
        Conv::Base64 => {
            let t: String = node_text(d, n).split_whitespace().collect();
            match unbase64(&t) {
                Some(b) => Ok(Value::Str(PhpStr::from(b))),
                None => violation(vm),
            }
        }
        Conv::Hex => {
            let t = trimmed(d, n);
            let b = t.as_bytes();
            if !b.len().is_multiple_of(2) {
                return violation(vm);
            }
            let mut out = Vec::with_capacity(b.len() / 2);
            for p in b.chunks(2) {
                let h = (p[0] as char).to_digit(16);
                let l = (p[1] as char).to_digit(16);
                match (h, l) {
                    (Some(h), Some(l)) => out.push((h * 16 + l) as u8),
                    _ => return violation(vm),
                }
            }
            Ok(Value::Str(PhpStr::from(out)))
        }
        Conv::DateTime
        | Conv::Time
        | Conv::Date
        | Conv::GYearMonth
        | Conv::GYear
        | Conv::GMonthDay
        | Conv::GDay
        | Conv::GMonth => {
            let t = node_text(d, n);
            Ok(out_str(dx, &t))
        }
        Conv::AnyType => any_type(vm, dx, n),
        Conv::AnyXml => {
            let mut s = String::new();
            for c in d.all_kids(n) {
                s.push_str(&d.to_xml(c));
            }
            Ok(Value::Str(PhpStr::from(s)))
        }
        Conv::Array => soap_array(vm, dx, None, n),
        Conv::Struct => generic_struct(vm, dx, n, Some("Struct".to_string())),
        Conv::Map => map(vm, dx, n),
    }
}

pub fn parse_float(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let ok = t
        .bytes()
        .all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'));
    if !ok {
        return None;
    }
    t.parse::<f64>().ok()
}

fn unbase64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Apache map: `item` children with `key` and `value`.
fn map(vm: &mut Vm, dx: &mut Dx, n: NodeId) -> R<Value> {
    let d = dx.d;
    let mut arr = PhpArray::new();
    for item in d.kids(n) {
        let mut key = None;
        let mut val = Value::Null;
        for c in d.kids(item) {
            match d.local(c) {
                "key" => key = Some(from_xml(vm, dx, None, c)?),
                "value" => val = from_xml(vm, dx, None, c)?,
                _ => {}
            }
        }
        let Some(k) = key else {
            return Err(soap_error(
                vm,
                "Encoding: Can't decode apache map, missing key",
            ));
        };
        let k = match k {
            Value::Int(i) => Key::Int(i),
            other => Key::from_php_str(&vm.to_php_str(&other)?),
        };
        arr.insert(k, val);
    }
    Ok(Value::Array(arr))
}

/// Parses "[2,3]" / "2 3" dimensions.
fn parse_dims(s: &str) -> Vec<i64> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    s.split([',', ' '])
        .filter(|p| !p.is_empty())
        .map(|p| p.trim().parse::<i64>().unwrap_or(-1))
        .collect()
}

/// SOAP-ENC arrays.
fn soap_array(vm: &mut Vm, dx: &mut Dx, tid: Option<TId>, n: NodeId) -> R<Value> {
    let d = dx.d;
    let mut item_enc: Option<Enc> = None;
    let mut dims: Vec<i64> = vec![-1];
    // From the instance.
    if let Some(at) = d.attr_ns(n, NS_SOAP11_ENC, "arrayType") {
        let (q, dm) = match at.find('[') {
            Some(i) => (&at[..i], &at[i..]),
            None => (at.as_str(), ""),
        };
        if let Some((ns, name)) = d.resolve_qname(n, q) {
            item_enc = super::encode::enc_for(dx.sdl.as_deref(), &ns, &name);
        }
        // Only the last [...] group gives this array's dimensions.
        let last = match dm.rfind('[') {
            Some(i) => &dm[i..],
            None => dm,
        };
        if !last.is_empty() {
            dims = parse_dims(last);
        }
    }
    if let Some(it) = d.attr_ns(n, NS_SOAP12_ENC, "itemType")
        && let Some((ns, name)) = d.resolve_qname(n, &it)
    {
        item_enc = super::encode::enc_for(dx.sdl.as_deref(), &ns, &name);
    }
    if let Some(sz) = d.attr_ns(n, NS_SOAP12_ENC, "arraySize") {
        let parts: Vec<&str> = sz.split_whitespace().collect();
        if parts.iter().skip(1).any(|p| *p == "*") {
            return Err(soap_error(
                vm,
                "Encoding: '*' may only be first arraySize value in list",
            ));
        }
        dims = parts
            .iter()
            .map(|p| p.parse::<i64>().unwrap_or(-1))
            .collect();
    }
    // From the WSDL.
    let mut item_elem_name: Option<String> = None;
    if let (Some(t), Some(sdl)) = (tid, dx.sdl.clone())
        && let Some(info) = array_info(&sdl, t)
    {
        if item_enc.is_none() {
            item_enc = info
                .item
                .as_ref()
                .and_then(|(ns, nm)| sdl.find_type(ns, nm));
        }
        if let Some(e) = info.elem {
            let ed = &sdl.types[sdl.deref_element(e)];
            item_elem_name = Some(ed.name.clone());
            if item_enc.is_none() {
                item_enc = ed.base.as_ref().and_then(|b| sdl.resolve(b));
            }
        }
        if dims.len() == 1 && dims[0] == -1 && info.dims > 1 {
            dims = if info.sizes.len() == info.dims {
                info.sizes.clone()
            } else {
                vec![-1; info.dims]
            };
        }
    }
    let _ = item_elem_name;
    // Offset.
    let mut pos: Vec<i64> = vec![0; dims.len()];
    if let Some(off) = d.attr_ns(n, NS_SOAP11_ENC, "offset") {
        let o = parse_dims(&off);
        for (i, v) in o.iter().enumerate() {
            if i < pos.len() {
                pos[i] = *v;
            }
        }
    }
    let mut arr = PhpArray::new();
    for c in d.kids(n) {
        if let Some(p) = d.attr_ns(c, NS_SOAP11_ENC, "position") {
            let pp = parse_dims(&p);
            for (i, v) in pp.iter().enumerate() {
                if i < pos.len() {
                    pos[i] = *v;
                }
            }
        }
        let v = from_xml(vm, dx, item_enc, c)?;
        if dims.len() <= 1 {
            arr.insert(Key::Int(pos[0]), v);
        } else {
            insert_nd(&mut arr, &pos, v);
        }
        // Advance.
        let last = pos.len() - 1;
        pos[last] += 1;
        let mut i = last;
        while i > 0 {
            if dims[i] > 0 && pos[i] >= dims[i] {
                pos[i] = 0;
                pos[i - 1] += 1;
            }
            i -= 1;
        }
    }
    Ok(Value::Array(arr))
}

fn insert_nd(arr: &mut PhpArray, pos: &[i64], v: Value) {
    if pos.len() == 1 {
        arr.insert(Key::Int(pos[0]), v);
        return;
    }
    let k = Key::Int(pos[0]);
    let mut sub = match arr.get(k.as_ref()) {
        Some(Value::Array(a)) => a.clone(),
        _ => PhpArray::new(),
    };
    insert_nd(&mut sub, &pos[1..], v);
    arr.insert(k, Value::Array(sub));
}

/// A WSDL type.
fn wsdl_type(vm: &mut Vm, dx: &mut Dx, t: TId, n: NodeId) -> R<Value> {
    let sdl = dx.sdl.clone().expect("WSDL");
    let ty = &sdl.types[t];
    match ty.kind {
        Kind::Element => {
            let e = sdl.deref_element(t);
            let base = sdl.types[e].base.as_ref().and_then(|b| sdl.resolve(b));
            from_xml_typed(vm, dx, base, n)
        }
        Kind::Simple => {
            let base = ty.base.as_ref().and_then(|b| sdl.resolve(b));
            match base {
                Some(b) => from_xml_typed(vm, dx, Some(b), n),
                None if ty.base.is_some() => builtin(vm, dx, Conv::String, n),
                None => guess(vm, dx, n),
            }
        }
        Kind::List | Kind::Union => {
            let t = node_text(dx.d, n);
            Ok(out_str(dx, &t))
        }
        Kind::Complex => complex(vm, dx, &sdl, t, n),
    }
}

/// A nil element: null, or for a simple-content type with attributes an
/// object with "_" null (observed on PHP 8.5; test schema/schema062.phpt).
fn nil_value(vm: &mut Vm, dx: &mut Dx, enc: Option<Enc>, n: NodeId) -> R<Value> {
    let Some(Enc::Type(t)) = enc else {
        return Ok(Value::Null);
    };
    let Some(sdl) = dx.sdl.clone() else {
        return Ok(Value::Null);
    };
    let mut id = t;
    if sdl.types[id].kind == Kind::Element {
        match sdl.types[sdl.deref_element(id)]
            .base
            .as_ref()
            .and_then(|b| sdl.resolve(b))
        {
            Some(Enc::Type(b)) => id = b,
            _ => return Ok(Value::Null),
        }
    }
    if sdl.types[id].kind != Kind::Complex
        || simple_content_base(&sdl, id).is_none()
        || sdl.attrs_of(id).is_empty()
    {
        return Ok(Value::Null);
    }
    let class = Some(sdl.types[id].name.clone());
    let obj = new_object(vm, dx, class.as_deref())?;
    set_prop(vm, &obj, "_", Value::Null);
    attrs(vm, dx, &sdl, id, n, &obj)?;
    Ok(Value::Object(obj))
}

/// Decodes with an encoding, without letting xsi:type change it again.
fn from_xml_typed(vm: &mut Vm, dx: &mut Dx, enc: Option<Enc>, n: NodeId) -> R<Value> {
    if is_nil(dx.d, n) {
        return nil_value(vm, dx, enc, n);
    }
    match enc {
        None => guess(vm, dx, n),
        Some(Enc::Builtin(b)) => builtin(vm, dx, b.conv, n),
        Some(Enc::Type(t)) => wsdl_type(vm, dx, t, n),
    }
}

fn simple_content_base(sdl: &Sdl, t: TId) -> Option<Option<Enc>> {
    let ty = &sdl.types[t];
    match ty.deriv {
        Deriv::SimpleRestriction | Deriv::SimpleExtension => {
            Some(ty.base.as_ref().and_then(|b| sdl.resolve(b)))
        }
        Deriv::ComplexRestriction | Deriv::ComplexExtension => {
            match ty.base.as_ref().and_then(|b| sdl.resolve(b)) {
                Some(Enc::Type(b)) if b != t && sdl.types[b].kind == Kind::Complex => {
                    simple_content_base(sdl, b)
                }
                _ => None,
            }
        }
        Deriv::None => None,
    }
}

fn complex(vm: &mut Vm, dx: &mut Dx, sdl: &Rc<Sdl>, t: TId, n: NodeId) -> R<Value> {
    let ty = &sdl.types[t];
    if array_info(sdl, t).is_some() {
        return soap_array(vm, dx, Some(t), n);
    }
    if matches!(
        ty.deriv,
        Deriv::ComplexExtension | Deriv::ComplexRestriction
    ) && let Some(Enc::Builtin(b)) = ty.base.as_ref().and_then(|b| sdl.resolve(b))
    {
        match b.conv {
            Conv::Map => return map(vm, dx, n),
            Conv::Array => return soap_array(vm, dx, None, n),
            _ => {}
        }
    }
    let class = Some(ty.name.clone());
    let obj = new_object_ns(vm, dx, class.as_deref(), Some(&ty.ns))?;
    register_early(dx, n, &obj);
    if simple_content_base(sdl, t).is_some() {
        let b = sdl.simple_base(t);
        let v = match b {
            Some(b) => from_xml_typed(vm, dx, Some(b), n)?,
            None => {
                let t = node_text(dx.d, n);
                out_str(dx, &t)
            }
        };
        set_prop(vm, &obj, "_", v);
    } else {
        let mut used = Vec::new();
        let saved = std::mem::replace(&mut dx.pending_any, false);
        let r = content(vm, dx, sdl, t, n, &obj, &mut used);
        let had = std::mem::replace(&mut dx.pending_any, saved);
        r?;
        if had {
            any_rest(vm, dx, n, &obj, &mut used)?;
        }
    }
    attrs(vm, dx, sdl, t, n, &obj)?;
    Ok(Value::Object(obj))
}

fn content(
    vm: &mut Vm,
    dx: &mut Dx,
    sdl: &Rc<Sdl>,
    t: TId,
    n: NodeId,
    obj: &goro_runtime::ObjRef,
    used: &mut Vec<NodeId>,
) -> R<()> {
    let ty = &sdl.types[t];
    if ty.deriv == Deriv::ComplexExtension
        && let Some(Enc::Type(b)) = ty.base.as_ref().and_then(|b| sdl.resolve(b))
        && b != t
        && sdl.types[b].kind == Kind::Complex
    {
        content(vm, dx, sdl, b, n, obj, used)?;
    }
    match ty.model.clone() {
        Some(m) => model(vm, dx, sdl, &m, n, obj, used)?,
        None => {
            if ty.deriv == Deriv::None && has_element_kids(dx.d, n) && ty.attrs.is_empty() {
                // No model: whatever children there are.
                let mut seen = Vec::new();
                for c in dx.d.kids(n) {
                    let name = dx.d.local(c).to_string();
                    let v = from_xml(vm, dx, None, c)?;
                    add_prop(vm, obj, &name, v, &mut seen);
                }
            }
        }
    }
    Ok(())
}

fn model(
    vm: &mut Vm,
    dx: &mut Dx,
    sdl: &Rc<Sdl>,
    m: &Model,
    n: NodeId,
    obj: &goro_runtime::ObjRef,
    used: &mut Vec<NodeId>,
) -> R<()> {
    match m.kind {
        MKind::Element => {
            let Some(e) = m.elem else { return Ok(()) };
            let ed_id = sdl.deref_element(e);
            let ed = &sdl.types[ed_id];
            let name = ed.name.clone();
            // No type at all: anyType (test
            // interop/Round4/GroupI/r4_groupI_xsd_028w.phpt).
            let enc = match &ed.base {
                Some(b) => sdl.resolve(b),
                None if ed.r#ref.is_none() => Some(Enc::Builtin(types::get(types::XSD_ANYTYPE))),
                None => None,
            };
            let d = dx.d;
            let nodes: Vec<NodeId> = d
                .kids(n)
                .into_iter()
                .filter(|&c| d.local(c) == name && !used.contains(&c))
                .collect();
            if nodes.is_empty() {
                if let Some(def) = ed.fixed.clone().or(ed.default.clone()) {
                    let v = default_value(vm, dx, enc, &def)?;
                    set_prop(vm, obj, &name, v);
                }
                return Ok(());
            }
            // ref="xsd:schema": the markup (test bugs/bug30106.phpt).
            if super::encode::is_schema_ref(&sdl.types[e]) {
                let c = nodes[0];
                used.push(c);
                set_prop(vm, obj, &name, Value::Str(PhpStr::from(d.to_xml(c))));
                return Ok(());
            }
            let multi = m.max != 1 || sdl.types[e].max != 1 || dx.rep > 0;
            let single_nil = nodes.len() == 1 && is_nil(d, nodes[0]);
            // Repeated elements are arrays whatever maxOccurs says
            // (observed on PHP 8.5; test bugs/bug73237.phpt).
            if nodes.len() > 1
                || (multi && dx.features & SOAP_SINGLE_ELEMENT_ARRAYS != 0 && !single_nil)
            {
                let mut arr = PhpArray::new();
                for c in nodes {
                    used.push(c);
                    let v = from_xml(vm, dx, enc, c)?;
                    let _ = arr.push(v);
                }
                set_prop(vm, obj, &name, Value::Array(arr));
            } else {
                let c = nodes[0];
                used.push(c);
                let v = from_xml(vm, dx, enc, c)?;
                set_prop(vm, obj, &name, v);
            }
            Ok(())
        }
        MKind::Sequence | MKind::All | MKind::Choice => {
            let rep = m.max != 1;
            if rep {
                dx.rep += 1;
            }
            for it in &m.items {
                let r = model(vm, dx, sdl, it, n, obj, used);
                if r.is_err() {
                    if rep {
                        dx.rep -= 1;
                    }
                    return r;
                }
            }
            if rep {
                dx.rep -= 1;
            }
            Ok(())
        }
        MKind::Group => {
            match m.group.as_ref().and_then(|q| sdl.group(q)).cloned() {
                Some(g) => model(vm, dx, sdl, &g, n, obj, used)?,
                None => {
                    for it in &m.items {
                        model(vm, dx, sdl, it, n, obj, used)?;
                    }
                }
            }
            Ok(())
        }
        MKind::Any => {
            // Taken from what the named elements leave (after the whole
            // model; observed on PHP 8.5; test bugs/bug44882.phpt).
            dx.pending_any = true;
            Ok(())
        }
    }
}

/// `<any>`: the children no element took. Untyped ones are serialized,
/// those with a known xsi:type decoded by name; a single run of untyped
/// ones is a string, otherwise an array (observed on PHP 8.5; tests
/// any.phpt, bugs/bug73237.phpt).
fn any_rest(
    vm: &mut Vm,
    dx: &mut Dx,
    n: NodeId,
    obj: &goro_runtime::ObjRef,
    used: &mut Vec<NodeId>,
) -> R<()> {
    let d = dx.d;
    let rest: Vec<NodeId> = d
        .kids(n)
        .into_iter()
        .filter(|c| !used.contains(c))
        .collect();
    let mut arr = PhpArray::new();
    let mut pending = String::new();
    let mut typed_any = false;
    for c in rest {
        used.push(c);
        let typed = xsi_type(d, c)
            .is_some_and(|(ns, nm)| super::encode::enc_for(dx.sdl.as_deref(), &ns, &nm).is_some());
        if typed {
            if !pending.is_empty() {
                let _ = arr.push(Value::Str(PhpStr::from(std::mem::take(&mut pending))));
            }
            // Typed by its xsi:type, not wrapped (test any.phpt).
            let enc = xsi_type(d, c)
                .and_then(|(ns, nm)| super::encode::enc_for(dx.sdl.as_deref(), &ns, &nm));
            let v = from_xml(vm, dx, enc, c)?;
            arr.insert(Key::from(d.local(c)), v);
            typed_any = true;
        } else {
            pending.push_str(&d.to_xml(c));
        }
    }
    if !typed_any {
        if !pending.is_empty() {
            set_prop(vm, obj, "any", Value::Str(PhpStr::from(pending)));
        }
        return Ok(());
    }
    if !pending.is_empty() {
        let _ = arr.push(Value::Str(PhpStr::from(pending)));
    }
    set_prop(vm, obj, "any", Value::Array(arr));
    Ok(())
}

fn default_value(vm: &mut Vm, dx: &mut Dx, enc: Option<Enc>, s: &str) -> R<Value> {
    let xml = format!("<x>{}</x>", super::xml_escape(s));
    let doc = match InDoc::parse(xml.as_bytes(), None) {
        Ok(d) => d,
        Err(_) => return Ok(Value::Str(PhpStr::from(s))),
    };
    let root = doc.root().expect("root");
    let mut sub = Dx {
        d: &doc,
        sdl: dx.sdl.clone(),
        version: dx.version,
        features: dx.features,
        encoding: dx.encoding.clone(),
        classmap: dx.classmap.clone(),
        typemap: dx.typemap.clone(),
        ids: HashMap::new(),
        done: HashMap::new(),
        rep: 0,
        pending_any: false,
    };
    from_xml_typed(vm, &mut sub, enc, root)
}

fn attrs(
    vm: &mut Vm,
    dx: &mut Dx,
    sdl: &Rc<Sdl>,
    t: TId,
    n: NodeId,
    obj: &goro_runtime::ObjRef,
) -> R<()> {
    let ty = &sdl.types[t];
    if matches!(ty.deriv, Deriv::ComplexExtension | Deriv::SimpleExtension)
        && let Some(Enc::Type(b)) = ty.base.as_ref().and_then(|b| sdl.resolve(b))
        && b != t
        && sdl.types[b].kind == Kind::Complex
    {
        attrs(vm, dx, sdl, b, n, obj)?;
    }
    let d = dx.d;
    for a in sdl.attrs_of(t) {
        if a.name.is_empty() {
            continue;
        }
        let val = if (a.qualified || a.r#ref.is_some()) && !a.ns.is_empty() {
            d.attr_ns(n, &a.ns, &a.name).or_else(|| d.attr(n, &a.name))
        } else {
            d.attr(n, &a.name)
        };
        let enc = a.typ.as_ref().and_then(|x| sdl.resolve(x));
        match val {
            Some(v) => {
                let x = default_value(vm, dx, enc, &v)?;
                set_prop(vm, obj, &a.name, x);
            }
            None => {
                if let Some(def) = a.fixed.clone().or(a.default.clone()) {
                    let x = default_value(vm, dx, enc, &def)?;
                    set_prop(vm, obj, &a.name, x);
                }
            }
        }
    }
    Ok(())
}

/// The encoding for a type code (used for typed decoding by callers).
pub fn builtin_enc(code: i64) -> Option<Enc> {
    types::by_code(code).map(Enc::Builtin)
}
