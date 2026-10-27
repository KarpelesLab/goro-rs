//! php_encoding.c: converting PHP values to XML nodes and back, by XSD and
//! SOAP-ENC type (the default encodings) or by the types of a WSDL's schema.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_xml::tree::NType;

use crate::php::*;
use crate::types::*;
use crate::xml::{self, Node};

// ---------------------------------------------------------------------------
// The default encodings

struct DefEnc {
    type_: i64,
    type_str: Option<&'static str>,
    ns: Option<&'static str>,
    to_zval: ToZval,
    to_xml: ToXml,
}

fn default_table() -> Vec<DefEnc> {
    macro_rules! e {
        ($t:expr, $s:expr, $ns:expr, $z:expr, $x:expr) => {
            DefEnc {
                type_: $t,
                type_str: $s,
                ns: $ns,
                to_zval: $z,
                to_xml: $x,
            }
        };
    }
    let xsd = Some(XSD_NAMESPACE);
    let x99 = Some(XSD_1999_NAMESPACE);
    vec![
        e!(
            UNKNOWN_TYPE,
            None,
            None,
            guess_zval_convert,
            guess_xml_convert
        ),
        e!(
            IS_NULL,
            Some("nil"),
            Some(XSI_NAMESPACE),
            to_zval_null,
            to_xml_null
        ),
        e!(
            IS_STRING,
            Some("string"),
            xsd,
            to_zval_string,
            to_xml_string
        ),
        e!(IS_LONG, Some("int"), xsd, to_zval_long, to_xml_long),
        e!(IS_DOUBLE, Some("float"), xsd, to_zval_double, to_xml_double),
        e!(IS_FALSE, Some("boolean"), xsd, to_zval_bool, to_xml_bool),
        e!(IS_TRUE, Some("boolean"), xsd, to_zval_bool, to_xml_bool),
        e!(
            IS_ARRAY,
            Some("Array"),
            Some(SOAP_1_1_ENC_NAMESPACE),
            to_zval_array,
            guess_array_map
        ),
        e!(
            IS_OBJECT,
            Some("Struct"),
            Some(SOAP_1_1_ENC_NAMESPACE),
            to_zval_object,
            to_xml_object
        ),
        e!(
            IS_ARRAY,
            Some("Array"),
            Some(SOAP_1_2_ENC_NAMESPACE),
            to_zval_array,
            guess_array_map
        ),
        e!(
            IS_OBJECT,
            Some("Struct"),
            Some(SOAP_1_2_ENC_NAMESPACE),
            to_zval_object,
            to_xml_object
        ),
        e!(
            XSD_STRING,
            Some("string"),
            xsd,
            to_zval_string,
            to_xml_string
        ),
        e!(XSD_BOOLEAN, Some("boolean"), xsd, to_zval_bool, to_xml_bool),
        e!(
            XSD_DECIMAL,
            Some("decimal"),
            xsd,
            to_zval_stringc,
            to_xml_string
        ),
        e!(XSD_FLOAT, Some("float"), xsd, to_zval_double, to_xml_double),
        e!(
            XSD_DOUBLE,
            Some("double"),
            xsd,
            to_zval_double,
            to_xml_double
        ),
        e!(
            XSD_DATETIME,
            Some("dateTime"),
            xsd,
            to_zval_stringc,
            to_xml_datetime
        ),
        e!(XSD_TIME, Some("time"), xsd, to_zval_stringc, to_xml_time),
        e!(XSD_DATE, Some("date"), xsd, to_zval_stringc, to_xml_date),
        e!(
            XSD_GYEARMONTH,
            Some("gYearMonth"),
            xsd,
            to_zval_stringc,
            to_xml_gyearmonth
        ),
        e!(XSD_GYEAR, Some("gYear"), xsd, to_zval_stringc, to_xml_gyear),
        e!(
            XSD_GMONTHDAY,
            Some("gMonthDay"),
            xsd,
            to_zval_stringc,
            to_xml_gmonthday
        ),
        e!(XSD_GDAY, Some("gDay"), xsd, to_zval_stringc, to_xml_gday),
        e!(
            XSD_GMONTH,
            Some("gMonth"),
            xsd,
            to_zval_stringc,
            to_xml_gmonth
        ),
        e!(
            XSD_DURATION,
            Some("duration"),
            xsd,
            to_zval_stringc,
            to_xml_duration
        ),
        e!(
            XSD_HEXBINARY,
            Some("hexBinary"),
            xsd,
            to_zval_hexbin,
            to_xml_hexbin
        ),
        e!(
            XSD_BASE64BINARY,
            Some("base64Binary"),
            xsd,
            to_zval_base64,
            to_xml_base64
        ),
        e!(XSD_LONG, Some("long"), xsd, to_zval_long, to_xml_long),
        e!(XSD_INT, Some("int"), xsd, to_zval_long, to_xml_long),
        e!(XSD_SHORT, Some("short"), xsd, to_zval_long, to_xml_long),
        e!(XSD_BYTE, Some("byte"), xsd, to_zval_long, to_xml_long),
        e!(
            XSD_NONPOSITIVEINTEGER,
            Some("nonPositiveInteger"),
            xsd,
            to_zval_long,
            to_xml_long
        ),
        e!(
            XSD_POSITIVEINTEGER,
            Some("positiveInteger"),
            xsd,
            to_zval_long,
            to_xml_long
        ),
        e!(
            XSD_NONNEGATIVEINTEGER,
            Some("nonNegativeInteger"),
            xsd,
            to_zval_long,
            to_xml_long
        ),
        e!(
            XSD_NEGATIVEINTEGER,
            Some("negativeInteger"),
            xsd,
            to_zval_long,
            to_xml_long
        ),
        e!(
            XSD_UNSIGNEDBYTE,
            Some("unsignedByte"),
            xsd,
            to_zval_long,
            to_xml_long
        ),
        e!(
            XSD_UNSIGNEDSHORT,
            Some("unsignedShort"),
            xsd,
            to_zval_long,
            to_xml_long
        ),
        e!(
            XSD_UNSIGNEDINT,
            Some("unsignedInt"),
            xsd,
            to_zval_long,
            to_xml_long
        ),
        e!(
            XSD_UNSIGNEDLONG,
            Some("unsignedLong"),
            xsd,
            to_zval_long,
            to_xml_long
        ),
        e!(XSD_INTEGER, Some("integer"), xsd, to_zval_long, to_xml_long),
        e!(
            XSD_ANYTYPE,
            Some("anyType"),
            xsd,
            guess_zval_convert,
            guess_xml_convert
        ),
        e!(
            XSD_UR_TYPE,
            Some("ur-type"),
            xsd,
            guess_zval_convert,
            guess_xml_convert
        ),
        e!(
            XSD_ANYURI,
            Some("anyURI"),
            xsd,
            to_zval_stringc,
            to_xml_string
        ),
        e!(
            XSD_QNAME,
            Some("QName"),
            xsd,
            to_zval_stringc,
            to_xml_string
        ),
        e!(
            XSD_NOTATION,
            Some("NOTATION"),
            xsd,
            to_zval_stringc,
            to_xml_string
        ),
        e!(
            XSD_NORMALIZEDSTRING,
            Some("normalizedString"),
            xsd,
            to_zval_stringr,
            to_xml_string
        ),
        e!(
            XSD_TOKEN,
            Some("token"),
            xsd,
            to_zval_stringc,
            to_xml_string
        ),
        e!(
            XSD_LANGUAGE,
            Some("language"),
            xsd,
            to_zval_stringc,
            to_xml_string
        ),
        e!(
            XSD_NMTOKEN,
            Some("NMTOKEN"),
            xsd,
            to_zval_stringc,
            to_xml_string
        ),
        e!(
            XSD_NMTOKENS,
            Some("NMTOKENS"),
            xsd,
            to_zval_stringc,
            to_xml_list1
        ),
        e!(XSD_NAME, Some("Name"), xsd, to_zval_stringc, to_xml_string),
        e!(
            XSD_NCNAME,
            Some("NCName"),
            xsd,
            to_zval_stringc,
            to_xml_string
        ),
        e!(XSD_ID, Some("ID"), xsd, to_zval_stringc, to_xml_string),
        e!(
            XSD_IDREF,
            Some("IDREF"),
            xsd,
            to_zval_stringc,
            to_xml_string
        ),
        e!(
            XSD_IDREFS,
            Some("IDREFS"),
            xsd,
            to_zval_stringc,
            to_xml_list1
        ),
        e!(
            XSD_ENTITY,
            Some("ENTITY"),
            xsd,
            to_zval_stringc,
            to_xml_string
        ),
        e!(
            XSD_ENTITIES,
            Some("ENTITIES"),
            xsd,
            to_zval_stringc,
            to_xml_list1
        ),
        e!(
            APACHE_MAP,
            Some("Map"),
            Some(APACHE_NAMESPACE),
            to_zval_map,
            to_xml_map
        ),
        e!(
            SOAP_ENC_OBJECT,
            Some("Struct"),
            Some(SOAP_1_1_ENC_NAMESPACE),
            to_zval_object,
            to_xml_object
        ),
        e!(
            SOAP_ENC_ARRAY,
            Some("Array"),
            Some(SOAP_1_1_ENC_NAMESPACE),
            to_zval_array,
            to_xml_array
        ),
        e!(
            SOAP_ENC_OBJECT,
            Some("Struct"),
            Some(SOAP_1_2_ENC_NAMESPACE),
            to_zval_object,
            to_xml_object
        ),
        e!(
            SOAP_ENC_ARRAY,
            Some("Array"),
            Some(SOAP_1_2_ENC_NAMESPACE),
            to_zval_array,
            to_xml_array
        ),
        e!(
            XSD_STRING,
            Some("string"),
            x99,
            to_zval_string,
            to_xml_string
        ),
        e!(XSD_BOOLEAN, Some("boolean"), x99, to_zval_bool, to_xml_bool),
        e!(
            XSD_DECIMAL,
            Some("decimal"),
            x99,
            to_zval_stringc,
            to_xml_string
        ),
        e!(XSD_FLOAT, Some("float"), x99, to_zval_double, to_xml_double),
        e!(
            XSD_DOUBLE,
            Some("double"),
            x99,
            to_zval_double,
            to_xml_double
        ),
        e!(XSD_LONG, Some("long"), x99, to_zval_long, to_xml_long),
        e!(XSD_INT, Some("int"), x99, to_zval_long, to_xml_long),
        e!(XSD_SHORT, Some("short"), x99, to_zval_long, to_xml_long),
        e!(XSD_BYTE, Some("byte"), x99, to_zval_long, to_xml_long),
        e!(
            XSD_1999_TIMEINSTANT,
            Some("timeInstant"),
            x99,
            to_zval_stringc,
            to_xml_string
        ),
        e!(
            XSD_ANYXML,
            Some("<anyXML>"),
            Some("<anyXML>"),
            to_zval_any,
            to_xml_any
        ),
    ]
}

pub struct Defaults {
    /// php_soap_defEnc: "ns:type" (or "type") -> encoder
    pub by_name: indexmap::IndexMap<String, EncRef>,
    /// php_soap_defEncIndex: type -> encoder
    pub by_type: std::collections::HashMap<i64, EncRef>,
    pub all: Vec<EncRef>,
}

thread_local! {
    pub static DEFAULTS: Defaults = build_defaults();
}

fn build_defaults() -> Defaults {
    let mut by_name = indexmap::IndexMap::new();
    let mut by_type = std::collections::HashMap::new();
    let mut all = Vec::new();
    for d in default_table() {
        let clark = match (d.ns, d.type_str) {
            (Some(ns), Some(t)) => Some(format!("{{{}}}{}", ns, t)),
            _ => None,
        };
        let enc = Encoder::new(
            EncDetails {
                type_: d.type_,
                type_str: d.type_str.map(|s| s.to_string()),
                ns: d.ns.map(|s| s.to_string()),
                clark,
                sdl_type: None,
                map: None,
            },
            d.to_xml,
            d.to_zval,
        );
        if let Some(t) = d.type_str {
            let key = match d.ns {
                Some(ns) => format!("{}:{}", ns, t),
                None => t.to_string(),
            };
            by_name.entry(key).or_insert_with(|| enc.clone());
        }
        by_type.entry(d.type_).or_insert_with(|| enc.clone());
        all.push(enc);
    }
    Defaults {
        by_name,
        by_type,
        all,
    }
}

pub fn def_by_type(t: i64) -> Option<EncRef> {
    DEFAULTS.with(|d| d.by_type.get(&t).cloned())
}

pub fn def_by_name(nscat: &str) -> Option<EncRef> {
    DEFAULTS.with(|d| d.by_name.get(nscat).cloned())
}

/// php_soap_defEncNs: default prefixes of well-known namespaces.
pub fn def_ns_prefix(ns: &str) -> Option<&'static str> {
    match ns {
        XSD_1999_NAMESPACE | XSD_NAMESPACE => Some(XSD_NS_PREFIX),
        XSI_NAMESPACE => Some(XSI_NS_PREFIX),
        XML_NAMESPACE => Some(XML_NS_PREFIX),
        SOAP_1_1_ENC_NAMESPACE => Some(SOAP_1_1_ENC_NS_PREFIX),
        SOAP_1_2_ENC_NAMESPACE => Some(SOAP_1_2_ENC_NS_PREFIX),
        _ => None,
    }
}

/// get_conversion()
pub fn get_conversion(vm: &mut Vm, t: i64) -> SR<EncRef> {
    match def_by_type(t) {
        Some(e) => Ok(e),
        None => Err(soap_error(vm, "Encoding: Cannot find encoding")),
    }
}

/// A default encoder known to exist.
pub fn conv(t: i64) -> EncRef {
    def_by_type(t).expect("default encoder")
}

// ---------------------------------------------------------------------------
// Encoder lookup (php_sdl.c)

/// get_encoder_ex(): by "ns:type" key in the defaults, then the SDL.
pub fn get_encoder_ex(sdl: Option<&SdlRef>, nscat: &str) -> Option<EncRef> {
    if let Some(e) = def_by_name(nscat) {
        return Some(e);
    }
    let s = sdl?;
    let s = s.borrow();
    s.encoders.as_ref()?.find(nscat).cloned()
}

/// get_encoder()
pub fn get_encoder(sdl: Option<&SdlRef>, ns: Option<&str>, type_: &str) -> Option<EncRef> {
    let nscat = format!("{}:{}", ns.unwrap_or(""), type_);
    let enc = get_encoder_ex(sdl, &nscat);
    if enc.is_some() {
        return enc;
    }
    if ns == Some(SOAP_1_1_ENC_NAMESPACE) || ns == Some(SOAP_1_2_ENC_NAMESPACE) {
        let enc = get_encoder_ex(None, &format!("{}:{}", XSD_NAMESPACE, type_));
        if let (Some(e), Some(s)) = (&enc, sdl) {
            let d = e.d.borrow().clone();
            let new_enc = Encoder::new(
                EncDetails {
                    ns: ns.map(|s| s.to_string()),
                    ..d
                },
                e.to_xml.get(),
                e.to_zval.get(),
            );
            let mut sb = s.borrow_mut();
            sb.encoders
                .get_or_insert_with(Ht::new)
                .update(&nscat, new_enc.clone());
            return Some(new_enc);
        }
        return enc;
    }
    None
}

/// get_encoder_from_prefix()
pub fn get_encoder_from_prefix(sdl: Option<&SdlRef>, node: Node, type_: &str) -> Option<EncRef> {
    let (cptype, ns) = xml::parse_namespace(type_);
    match xml::search_ns(node, ns.as_deref()) {
        Some(nsptr) => {
            let href = xml::ns_href(nsptr);
            get_encoder(sdl, Some(&href), &cptype).or_else(|| get_encoder_ex(sdl, &cptype))
        }
        None => get_encoder_ex(sdl, type_),
    }
}

fn find_encoder_by_type_name(sdl: Option<&SdlRef>, type_: &str) -> Option<EncRef> {
    let s = sdl?.borrow();
    for enc in s.encoders.as_ref()?.values() {
        let d = enc.d.borrow();
        if type_.starts_with('{') {
            if d.clark.as_deref() == Some(type_) {
                return Some(enc.clone());
            }
        } else if d.type_str.as_deref() == Some(type_) {
            return Some(enc.clone());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Whitespace

pub fn whitespace_replace(s: &str) -> String {
    s.chars()
        .map(|c| {
            if matches!(c, '\t' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

pub fn whitespace_collapse(s: &str) -> String {
    let r = whitespace_replace(s);
    let mut out = String::with_capacity(r.len());
    let mut old = '\0';
    for c in r.trim_start_matches(' ').chars() {
        if c != ' ' || old != ' ' {
            out.push(c);
        }
        old = c;
    }
    if old == ' ' {
        out.pop();
    }
    out
}

// ---------------------------------------------------------------------------
// References (href / id)

/// The identity of a value for the reference map: objects and PHP
/// references (the zval address in C).
fn ident(v: &Value) -> Option<usize> {
    match v {
        Value::Object(o) => Some(Rc::as_ptr(o) as *const u8 as usize),
        Value::Reference(r) => match &*r.borrow() {
            Value::Object(o) => Some(Rc::as_ptr(o) as *const u8 as usize),
            _ => Some(Rc::as_ptr(r) as *const u8 as usize),
        },
        _ => None,
    }
}

/// soap_check_zval_ref(): true when the value was already serialized (the
/// node then refers to it with href/ref).
fn soap_check_zval_ref(data: &Value, node: Node) -> bool {
    let Some(id) = ident(data) else {
        return false;
    };
    let found = g(|g| match g.ref_map.as_mut() {
        None => Err(()),
        Some(m) => Ok(m.out.get(&id).copied()),
    });
    let Ok(found) = found else {
        return false;
    };
    match found {
        Some(node_ptr) => {
            if node_ptr == node {
                return false;
            }
            if soap_version() == SOAP_1_1 {
                let id = match xml::get_attribute(xml::properties(node_ptr), "id") {
                    Some(a) => format!("#{}", xml::attr_value(a)),
                    None => {
                        let n = g(|g| {
                            g.cur_uniq_ref += 1;
                            g.cur_uniq_ref
                        });
                        let id = format!("#ref{}", n);
                        xml::set_prop(node_ptr, "id", &id[1..]);
                        id
                    }
                };
                xml::set_prop(node, "href", &id);
            } else {
                let id = match xml::get_attribute_ex(
                    xml::properties(node_ptr),
                    "id",
                    Some(SOAP_1_2_ENC_NAMESPACE),
                ) {
                    Some(a) => format!("#{}", xml::attr_value(a)),
                    None => {
                        let n = g(|g| {
                            g.cur_uniq_ref += 1;
                            g.cur_uniq_ref
                        });
                        let id = format!("#ref{}", n);
                        set_ns_prop(node_ptr, SOAP_1_2_ENC_NAMESPACE, "id", &id[1..]);
                        id
                    }
                };
                set_ns_prop(node, SOAP_1_2_ENC_NAMESPACE, "ref", &id);
            }
            true
        }
        None => {
            g(|g| {
                if let Some(m) = g.ref_map.as_mut() {
                    m.out.insert(id, node);
                }
            });
            false
        }
    }
}

/// soap_check_xml_ref(): the value already decoded from this node.
fn soap_check_xml_ref(node: Node) -> Option<Value> {
    g(|g| g.ref_map.as_ref().and_then(|m| m.inp.get(&node).cloned()))
}

fn soap_add_xml_ref(data: &Value, node: Node) {
    g(|g| {
        if let Some(m) = g.ref_map.as_mut() {
            m.inp.insert(node, data.clone());
        }
    })
}

fn take_ref_map() -> Option<RefMap> {
    g(|g| g.ref_map.take())
}

fn restore_ref_map(m: Option<RefMap>) {
    g(|g| g.ref_map = m)
}

// ---------------------------------------------------------------------------
// Master encode/decode

fn is_soap_var(v: &Value) -> Option<Obj> {
    match v.deref() {
        Value::Object(o) if o.borrow().class_name.eq_ignore_ascii_case(b"SoapVar") => Some(o),
        _ => None,
    }
}

fn typemap_lookup(key: &str) -> Option<EncRef> {
    g(|g| g.typemap.as_ref().and_then(|t| t.find(key).cloned()))
}

fn has_typemap() -> bool {
    g(|g| g.typemap.is_some())
}

fn nscat_of(enc: &EncRef) -> Option<String> {
    let d = enc.d.borrow();
    let ts = d.type_str.as_ref()?;
    Some(match &d.ns {
        Some(ns) => format!("{}:{}", ns, ts),
        None => ts.clone(),
    })
}

fn master_to_xml_int(
    vm: &mut Vm,
    encode: Option<&EncRef>,
    data: Option<&Value>,
    style: i32,
    parent: Node,
    check_class_map: bool,
) -> SR<Node> {
    // ZVAL_DEREF keeps the reference's identity for the ref map.
    if let Some(var) = data.and_then(is_soap_var) {
        let ztype = prop(&var, "enc_type");
        let Value::Long(ztype) = ztype else {
            return Err(soap_error(
                vm,
                "Encoding: SoapVar has no 'enc_type' property",
            ));
        };
        let zstype = as_str(&prop(&var, "enc_stype"));
        let zns = as_str(&prop(&var, "enc_ns"));
        let sdl = cur_sdl();
        let mut enc: Option<EncRef> = None;
        if let Some(st) = &zstype {
            enc = match &zns {
                Some(ns) => get_encoder(sdl.as_ref(), Some(ns), st),
                None => get_encoder_ex(sdl.as_ref(), st),
            };
            if enc.is_none() && has_typemap() {
                let key = match &zns {
                    Some(ns) => format!("{}:{}", ns, st),
                    None => st.clone(),
                };
                enc = typemap_lookup(&key);
            }
        }
        if enc.is_none() {
            enc = Some(get_conversion(vm, ztype)?);
        }
        let enc = enc.or_else(|| encode.cloned());
        let value = prop(&var, "enc_value");
        let value_opt = if matches!(value, Value::Undef) {
            None
        } else {
            Some(value)
        };
        let node = master_to_xml(vm, enc.as_ref(), value_opt.as_ref(), style, parent)?;
        if style == SOAP_ENCODED || (sdl.is_some() && !opt_same_enc(encode, enc.as_ref())) {
            if let Some(st) = &zstype {
                set_ns_and_type_ex(node, zns.as_deref(), st);
            }
        }
        // xmlNodeSetName() ignores names longer than INT_MAX.
        let name = prop(&var, "enc_name");
        let too_long = matches!(name.deref(), Value::String(s) if s.len() > i32::MAX as usize);
        if let Some(n) = (!too_long).then(|| as_str(&name)).flatten() {
            xml::set_name(node, &n);
        }
        if let Some(nsn) = as_str(&prop(&var, "enc_namens")) {
            let nsp = encode_add_ns(node, Some(&nsn));
            xml::set_ns(node, nsp);
        }
        return Ok(node);
    }
    let mut encode = encode.cloned();
    let mut add_type = false;
    if check_class_map {
        let cm = g(|g| g.class_map.clone());
        let dv = data.map(|d| d.deref());
        if let (Some(cm), Some(Value::Object(o))) =
            (cm, dv.as_ref().filter(|d| !is_recursive(d)).cloned())
        {
            let ce_name = o.borrow().class_name.clone();
            let entries: Vec<(ArrayKey, Value)> = cm
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.deref()))
                .collect();
            for (k, v) in entries {
                if let (Value::String(cls), ArrayKey::String(type_name)) = (&v, &k) {
                    if cls.as_bytes().eq_ignore_ascii_case(&ce_name) {
                        let type_name = type_name.to_string_lossy();
                        let mut enc = None;
                        if let Some(sdl) = cur_sdl() {
                            let tns = sdl.borrow().target_ns.clone();
                            enc = get_encoder(Some(&sdl), tns.as_deref(), &type_name);
                            if enc.is_none() {
                                enc = find_encoder_by_type_name(Some(&sdl), &type_name);
                            }
                        }
                        if let Some(enc) = enc {
                            if !opt_same_enc(encode.as_ref(), Some(&enc)) && style == SOAP_LITERAL {
                                add_type = true;
                            }
                            encode = Some(enc);
                        }
                        break;
                    }
                }
            }
        }
    }
    let mut encode = match encode {
        Some(e) => e,
        None => get_conversion(vm, UNKNOWN_TYPE)?,
    };
    if has_typemap() {
        if let Some(key) = nscat_of(&encode) {
            if let Some(n) = typemap_lookup(&key) {
                encode = n;
            }
        }
    }
    let f = encode.to_xml.get();
    let node = f(vm, &encode, data, style, parent)?;
    if add_type {
        set_ns_and_type(node, &encode);
    }
    Ok(node)
}

/// master_to_xml()
pub fn master_to_xml(
    vm: &mut Vm,
    encode: Option<&EncRef>,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    master_to_xml_int(vm, encode, data, style, parent, true)
}

fn master_to_zval_int(vm: &mut Vm, encode: &EncRef, data: Node) -> SR<Value> {
    let mut encode = encode.clone();
    if has_typemap() {
        if let Some(key) = nscat_of(&encode) {
            if let Some(n) = typemap_lookup(&key) {
                encode = n;
            }
        } else if let Some(ta) =
            xml::get_attribute_ex(xml::properties(data), "type", Some(XSI_NAMESPACE))
        {
            let (cptype, ns) = xml::parse_namespace(&xml::attr_value(ta));
            let mut key = String::new();
            if let Some(nsptr) = xml::search_ns(data, ns.as_deref()) {
                key.push_str(&xml::ns_href(nsptr));
                key.push(':');
            }
            key.push_str(&cptype);
            if let Some(n) = typemap_lookup(&key) {
                encode = n;
            }
        }
    }
    let f = encode.to_zval.get();
    f(vm, &encode, Some(data))
}

/// master_to_zval()
pub fn master_to_zval(vm: &mut Vm, encode: Option<&EncRef>, data: Node) -> SR<Value> {
    let data = check_and_resolve_href(vm, Some(data))?.expect("node");
    let encode = match encode {
        None => get_conversion(vm, UNKNOWN_TYPE)?,
        Some(encode) => {
            let mut encode = encode.clone();
            if let Some(ta) =
                xml::get_attribute_ex(xml::properties(data), "type", Some(XSI_NAMESPACE))
            {
                let sdl = cur_sdl();
                let enc = get_encoder_from_prefix(sdl.as_ref(), data, &xml::attr_value(ta));
                if let Some(enc) = enc.filter(|e| !same_enc(e, &encode)) {
                    if !derives_back(&enc) {
                        encode = enc;
                    }
                }
            }
            encode
        }
    };
    master_to_zval_int(vm, &encode, data)
}

/// The loop of master_to_zval()/guess_zval_convert(): whether a non-complex
/// type chain of `enc` leads back to itself (then it is not used).
fn derives_back(enc: &EncRef) -> bool {
    let mut tmp = Some(enc.clone());
    let mut guard = 0;
    while let Some(t) = tmp {
        guard += 1;
        if guard > 1000 {
            return true;
        }
        let Some(st) = t.sdl_type() else {
            break;
        };
        let (kind, senc) = {
            let s = st.borrow();
            (s.kind, s.encode.clone())
        };
        if kind == XSD_TYPEKIND_COMPLEX {
            break;
        }
        if let Some(se) = &senc {
            if same_enc(enc, se) || same_enc(&t, se) {
                return true;
            }
        }
        tmp = senc;
    }
    false
}

// ---------------------------------------------------------------------------
// User type map callbacks

/// to_xml_user()
pub fn to_xml_user(
    vm: &mut Vm,
    type_: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let mut ret: Option<Node> = None;
    if let Some(cb) = type_.map().and_then(|m| m.to_xml.clone()) {
        let arg = data.map(|d| d.deref()).unwrap_or(Value::Null);
        let rv = match call_user(vm, &cb, &[arg]) {
            Ok(v) => v,
            Err(_) if vm.current_exception.is_some() && !vm.exit_requested => Value::Null,
            Err(e) => return Err(e),
        };
        if let Value::String(s) = rv.deref() {
            if let Some(doc) = xml::parse_memory(s.as_bytes()) {
                if let Some(c) = xml::children(doc) {
                    ret = Some(xml::copy_node(c, xml::doc(parent)));
                }
                xml::free_doc(doc);
            }
        }
    }
    let ret = match ret {
        Some(r) => r,
        None => xml::new_doc_node(xml::doc(parent), "BOGUS"),
    };
    let ret = xml::add_child(parent, ret);
    if style == SOAP_ENCODED {
        set_ns_and_type(ret, type_);
    }
    Ok(ret)
}

/// to_zval_user()
pub fn to_zval_user(vm: &mut Vm, type_: &EncRef, node: Option<Node>) -> SR<Value> {
    let Some(cb) = type_.map().and_then(|m| m.to_zval.clone()) else {
        return Ok(Value::Null);
    };
    let Some(node) = node else {
        return Ok(Value::Null);
    };
    let copy = xml::copy_node(node, None);
    let s = xml::node_dump(copy);
    xml::free_doc(copy);
    let r = match call_user(vm, &cb, &[vstr(s)]) {
        Ok(v) => v,
        Err(_) if vm.current_exception.is_some() && !vm.exit_requested => Value::Null,
        Err(e) => return Err(e),
    };
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    Ok(r)
}

// ---------------------------------------------------------------------------
// Strings

/// FIND_XML_NULL
fn xml_is_null(data: Option<Node>) -> bool {
    match data {
        None => true,
        Some(d) => xml::get_attribute_ex(xml::properties(d), "nil", Some(XSI_NAMESPACE)).is_some(),
    }
}

fn encoding_out(s: &str) -> Value {
    // xmlCharEncOutFunc(SOAP_GLOBAL(encoding)): UTF-8 to the client's charset
    match g(|g| g.encoding.clone()) {
        Some(enc) => match goro_ext_xml::parser::encode_from_utf8(s, &enc) {
            Some(b) => vstr(b),
            None => vstr(s),
        },
        None => vstr(s),
    }
}

enum StrKind {
    Plain,
    Replace,
    Collapse,
}

fn to_zval_string_ex(vm: &mut Vm, data: Option<Node>, kind: StrKind) -> SR<Value> {
    if xml_is_null(data) {
        return Ok(Value::Null);
    }
    let d = data.unwrap();
    match xml::first_child_info(d) {
        Some((NType::Text, content, false)) => {
            let content = match kind {
                StrKind::Plain => content,
                StrKind::Replace => {
                    let c = whitespace_replace(&content);
                    set_first_child_content(d, &c);
                    c
                }
                StrKind::Collapse => {
                    let c = whitespace_collapse(&content);
                    set_first_child_content(d, &c);
                    c
                }
            };
            Ok(encoding_out(&content))
        }
        Some((NType::CData, content, false)) => Ok(vstr(content)),
        Some(_) => Err(soap_error(vm, "Encoding: Violation of encoding rules")),
        None => Ok(vstr("")),
    }
}

fn set_first_child_content(d: Node, s: &str) {
    if let Some(c) = xml::children(d) {
        xml::set_raw_content(c, s);
    }
}

fn to_zval_string(vm: &mut Vm, _t: &EncRef, data: Option<Node>) -> SR<Value> {
    to_zval_string_ex(vm, data, StrKind::Plain)
}

fn to_zval_stringr(vm: &mut Vm, _t: &EncRef, data: Option<Node>) -> SR<Value> {
    to_zval_string_ex(vm, data, StrKind::Replace)
}

pub fn to_zval_stringc(vm: &mut Vm, _t: &EncRef, data: Option<Node>) -> SR<Value> {
    to_zval_string_ex(vm, data, StrKind::Collapse)
}

/// php_base64_decode() (non-strict: invalid characters are skipped)
pub fn base64_decode(input: &[u8]) -> Vec<u8> {
    fn rev(c: u8) -> i32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as i32,
            b'a'..=b'z' => (c - b'a') as i32 + 26,
            b'0'..=b'9' => (c - b'0') as i32 + 52,
            b'+' => 62,
            b'/' => 63,
            _ => -1,
        }
    }
    let mut out: Vec<u8> = Vec::with_capacity(input.len() * 3 / 4 + 1);
    let mut i = 0usize;
    let mut cur: u8 = 0;
    for &c in input {
        if c == b'=' {
            continue;
        }
        let ch = rev(c);
        if ch < 0 {
            continue;
        }
        let ch = ch as u8;
        match i % 4 {
            0 => cur = ch << 2,
            1 => {
                out.push(cur | (ch >> 4));
                cur = (ch & 0x0f) << 4;
            }
            2 => {
                out.push(cur | (ch >> 2));
                cur = (ch & 0x03) << 6;
            }
            _ => out.push(cur | ch),
        }
        i += 1;
    }
    out
}

pub fn base64_encode(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for ch in input.chunks(3) {
        let b0 = ch[0] as u32;
        let b1 = ch.get(1).copied().unwrap_or(0) as u32;
        let b2 = ch.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if ch.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if ch.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn to_zval_base64(vm: &mut Vm, _t: &EncRef, data: Option<Node>) -> SR<Value> {
    if xml_is_null(data) {
        return Ok(Value::Null);
    }
    let d = data.unwrap();
    match xml::first_child_info(d) {
        Some((NType::Text, content, false)) => {
            let c = whitespace_collapse(&content);
            set_first_child_content(d, &c);
            Ok(vstr(base64_decode(c.as_bytes())))
        }
        Some((NType::CData, content, false)) => Ok(vstr(base64_decode(content.as_bytes()))),
        Some(_) => Err(soap_error(vm, "Encoding: Violation of encoding rules")),
        None => Ok(vstr("")),
    }
}

fn to_zval_hexbin(vm: &mut Vm, _t: &EncRef, data: Option<Node>) -> SR<Value> {
    if xml_is_null(data) {
        return Ok(Value::Null);
    }
    let d = data.unwrap();
    let content = match xml::first_child_info(d) {
        Some((NType::Text, content, false)) => {
            let c = whitespace_collapse(&content);
            set_first_child_content(d, &c);
            c
        }
        Some((NType::CData, content, false)) => content,
        Some(_) => return Err(soap_error(vm, "Encoding: Violation of encoding rules")),
        None => return Ok(vstr("")),
    };
    let b = content.as_bytes();
    let n = b.len() / 2;
    let mut out = Vec::with_capacity(n);
    let hex = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    for i in 0..n {
        let (Some(h), Some(l)) = (hex(b[2 * i]), hex(b[2 * i + 1])) else {
            return Err(soap_error(vm, "Encoding: Violation of encoding rules"));
        };
        out.push((h << 4) | l);
    }
    Ok(vstr(out))
}

/// zend_value_error() without bailing out: a pending exception becomes the
/// new one's previous.
fn value_error_chained(vm: &mut Vm, msg: &str) {
    let e = vm.create_exception(b"ValueError", msg, vm.current_line);
    if let Some(prev) = vm.current_exception.take() {
        Vm::exception_set_previous(&e, prev);
    }
    vm.current_exception = Some(e);
}

/// get_serialization_string_from_zval()
fn serialization_string(vm: &mut Vm, data: &Value) -> SR<Vec<u8>> {
    let d = data.deref();
    if let Value::Object(o) = &d {
        let cl = lower(&o.borrow().class_name);
        let backing = vm
            .classes
            .get(&cl)
            .filter(|c| c.is_enum)
            .map(|c| c.enum_backing_type.clone());
        if let Some(backing) = backing {
            if backing.is_none() {
                value_error_chained(vm, "Non-backed enums have no default serialization");
                return Ok(Vec::new());
            }
            let v = prop(o, "value");
            return zval_get_string(vm, &v);
        }
    }
    zval_get_string(vm, &d)
}

pub fn zval_get_string(vm: &mut Vm, v: &Value) -> SR<Vec<u8>> {
    match v.deref() {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        other => match vm.value_to_string_checked(&other) {
            Ok(s) => {
                if vm.current_exception.is_some() {
                    return Err(Bail);
                }
                Ok(s.as_bytes().to_vec())
            }
            Err(_) => Err(Bail),
        },
    }
}

fn serialization_long(vm: &mut Vm, data: &Value) -> SR<i64> {
    let d = data.deref();
    if let Value::Object(o) = &d {
        let cl = lower(&o.borrow().class_name);
        let backing = vm
            .classes
            .get(&cl)
            .filter(|c| c.is_enum)
            .map(|c| c.enum_backing_type.clone());
        if let Some(backing) = backing {
            match backing.as_deref() {
                Some(b"int") => return Ok(prop(o, "value").to_long()),
                None => {
                    value_error_chained(vm, "Non-backed enums have no default serialization");
                    return Ok(0);
                }
                Some(_) => {
                    value_error_chained(vm, "String-backed enum cannot be serialized as int");
                    return Ok(0);
                }
            }
        }
    }
    Ok(zval_get_long(vm, &d))
}

pub fn zval_get_long(_vm: &mut Vm, v: &Value) -> i64 {
    v.deref().to_long()
}

/// A new <BOGUS> child of `parent` (to be renamed by the caller).
fn bogus(parent: Node) -> Node {
    let n = xml::new_doc_node(xml::doc(parent), "BOGUS");
    xml::add_child(parent, n)
}

/// FIND_ZVAL_NULL: true when the value is NULL (xsi:nil set in encoded style).
fn zval_null(data: Option<&Value>, node: Node, style: i32) -> bool {
    let is_null = match data {
        None => true,
        Some(v) => matches!(v.deref(), Value::Null),
    };
    if is_null && style == SOAP_ENCODED {
        set_xsi_nil(node);
    }
    is_null
}

fn invalid_utf8_message(s: &[u8]) -> String {
    // The C code copies the string and marks the first invalid byte.
    let mut err: Vec<u8> = s.to_vec();
    let mut i = 0usize;
    let mut bad: Option<u8> = None;
    while let Some(&c) = err.get(i) {
        if c == 0 {
            break;
        }
        i += 1;
        let at = |k: usize| err.get(k).copied().unwrap_or(0);
        if c & 0x80 == 0 {
        } else if c & 0xe0 == 0xc0 {
            if at(i) & 0xc0 != 0x80 {
                bad = Some(c);
                break;
            }
            i += 1;
        } else if c & 0xf0 == 0xe0 {
            if at(i) & 0xc0 != 0x80 || at(i + 1) & 0xc0 != 0x80 {
                bad = Some(c);
                break;
            }
            i += 2;
        } else if c & 0xf8 == 0xf0 {
            if at(i) & 0xc0 != 0x80 || at(i + 1) & 0xc0 != 0x80 || at(i + 2) & 0xc0 != 0x80 {
                bad = Some(c);
                break;
            }
            i += 3;
        } else {
            bad = Some(c);
            break;
        }
    }
    if let Some(c) = bad {
        let hexd = |v: u8| if v > 9 { b'a' + v - 10 } else { b'0' + v };
        err.truncate(i - 1);
        err.push(b'\\');
        err.push(b'x');
        err.push(hexd(c >> 4));
        err.push(hexd(c & 15));
        err.extend_from_slice(b"...");
    }
    String::from_utf8_lossy(&err).into_owned()
}

fn to_xml_string(
    vm: &mut Vm,
    type_: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let ret = bogus(parent);
    if zval_null(data, ret, style) {
        return Ok(ret);
    }
    let mut s = serialization_string(vm, data.unwrap())?;
    if let Some(enc) = g(|g| g.encoding.clone()) {
        if let Some(u) = goro_ext_xml::parser::decode_to_utf8(&s, &enc) {
            s = u;
        }
    }
    let text = match String::from_utf8(s.clone()) {
        Ok(t) => t,
        Err(_) => {
            let m = format!(
                "Encoding: string '{}' is not a valid utf-8 string",
                invalid_utf8_message(&s)
            );
            return Err(soap_error(vm, m));
        }
    };
    // The text ends at a NUL byte (C string).
    let text = match text.find('\0') {
        Some(p) => text[..p].to_string(),
        None => text,
    };
    xml::add_text(ret, &text);
    if style == SOAP_ENCODED {
        set_ns_and_type(ret, type_);
    }
    Ok(ret)
}

fn to_xml_base64(
    vm: &mut Vm,
    type_: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let ret = bogus(parent);
    if zval_null(data, ret, style) {
        return Ok(ret);
    }
    let s = serialization_string(vm, data.unwrap())?;
    xml::add_text(ret, &base64_encode(&s));
    if style == SOAP_ENCODED {
        set_ns_and_type(ret, type_);
    }
    Ok(ret)
}

fn to_xml_hexbin(
    vm: &mut Vm,
    type_: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let ret = bogus(parent);
    if zval_null(data, ret, style) {
        return Ok(ret);
    }
    let s = match data.unwrap().deref() {
        Value::String(s) => s.as_bytes().to_vec(),
        other => serialization_string(vm, &other)?,
    };
    let mut out = String::with_capacity(s.len() * 2);
    for b in s {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 15) as usize] as char);
    }
    xml::add_text(ret, &out);
    if style == SOAP_ENCODED {
        set_ns_and_type(ret, type_);
    }
    Ok(ret)
}

fn to_zval_double(vm: &mut Vm, _t: &EncRef, data: Option<Node>) -> SR<Value> {
    if xml_is_null(data) {
        return Ok(Value::Null);
    }
    let d = data.unwrap();
    match xml::first_child_info(d) {
        Some((NType::Text, content, false)) => {
            let c = whitespace_collapse(&content);
            set_first_child_content(d, &c);
            match goro_core::value::parse_numeric_str(c.as_bytes(), false) {
                Some((goro_core::value::NumStr::Long(l), _)) => Ok(Value::Double(l as f64)),
                Some((goro_core::value::NumStr::Double(f), _)) => Ok(Value::Double(f)),
                None => {
                    let l = c.to_ascii_lowercase();
                    if l.starts_with("nan") {
                        Ok(Value::Double(f64::NAN))
                    } else if l.starts_with("inf") {
                        Ok(Value::Double(f64::INFINITY))
                    } else if l.starts_with("-inf") {
                        Ok(Value::Double(f64::NEG_INFINITY))
                    } else {
                        Err(soap_error(vm, "Encoding: Violation of encoding rules"))
                    }
                }
            }
        }
        Some(_) => Err(soap_error(vm, "Encoding: Violation of encoding rules")),
        None => Ok(Value::Null),
    }
}

fn to_zval_long(vm: &mut Vm, _t: &EncRef, data: Option<Node>) -> SR<Value> {
    if xml_is_null(data) {
        return Ok(Value::Null);
    }
    let d = data.unwrap();
    match xml::first_child_info(d) {
        Some((NType::Text, content, false)) => {
            let c = whitespace_collapse(&content);
            set_first_child_content(d, &c);
            match goro_core::value::parse_numeric_str(c.as_bytes(), false) {
                Some((goro_core::value::NumStr::Long(l), _)) => Ok(Value::Long(l)),
                Some((goro_core::value::NumStr::Double(f), _)) => Ok(Value::Double(f)),
                None => Err(soap_error(vm, "Encoding: Violation of encoding rules")),
            }
        }
        Some(_) => Err(soap_error(vm, "Encoding: Violation of encoding rules")),
        None => Ok(Value::Null),
    }
}

fn to_xml_long(
    vm: &mut Vm,
    type_: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let ret = bogus(parent);
    if zval_null(data, ret, style) {
        return Ok(ret);
    }
    let d = data.unwrap().deref();
    if let Value::Double(f) = d {
        xml::node_set_content(ret, &format_f0(f.floor()));
    } else {
        let l = serialization_long(vm, &d)?;
        xml::node_set_content(ret, &l.to_string());
    }
    if style == SOAP_ENCODED {
        set_ns_and_type(ret, type_);
    }
    Ok(ret)
}

/// snprintf("%0.0F", d)
fn format_f0(d: f64) -> String {
    if d.is_nan() {
        return "NAN".to_string();
    }
    if d.is_infinite() {
        return if d < 0.0 { "-INF" } else { "INF" }.to_string();
    }
    let s = format!("{:.0}", d);
    if s == "-0" { "-0".to_string() } else { s }
}

/// zend_gcvt(value, precision, '.', 'E') (INF/NAN cut to `ndigit`
/// characters by its snprintf())
pub fn gcvt_upper(value: f64, ndigit: i32) -> String {
    if !value.is_finite() {
        let nd = if ndigit < 0 { 17 } else { ndigit as usize };
        return goro_core::value::nonfinite_gcvt(value, nd);
    }
    String::from_utf8_lossy(&goro_core::value::zend_gcvt(value, ndigit, b'.', b'E')).into_owned()
}

fn to_xml_double(
    vm: &mut Vm,
    type_: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let ret = bogus(parent);
    if zval_null(data, ret, style) {
        return Ok(ret);
    }
    let _ = vm;
    let d = data.unwrap().deref().to_double();
    let precision = goro_core::value::get_php_precision();
    xml::node_set_content(ret, &gcvt_upper(d, precision));
    if style == SOAP_ENCODED {
        set_ns_and_type(ret, type_);
    }
    Ok(ret)
}

fn to_zval_bool(vm: &mut Vm, _t: &EncRef, data: Option<Node>) -> SR<Value> {
    if xml_is_null(data) {
        return Ok(Value::Null);
    }
    let d = data.unwrap();
    let Some((ty, content, has_next)) = xml::first_child_info(d) else {
        return Ok(Value::Null);
    };
    if ty != NType::Text || has_next {
        return Err(soap_error(vm, "Encoding: Violation of encoding rules"));
    }
    let c = whitespace_collapse(&content);
    set_first_child_content(d, &c);
    if c.is_empty() || c == "0" || c.eq_ignore_ascii_case("f") || c.eq_ignore_ascii_case("false") {
        Ok(Value::False)
    } else {
        Ok(Value::True)
    }
}

fn to_xml_bool(
    vm: &mut Vm,
    type_: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let ret = bogus(parent);
    if zval_null(data, ret, style) {
        return Ok(ret);
    }
    let _ = vm;
    let t = data.unwrap().deref().is_truthy();
    xml::node_set_content(ret, if t { "true" } else { "false" });
    if style == SOAP_ENCODED {
        set_ns_and_type(ret, type_);
    }
    Ok(ret)
}

fn to_zval_null(_vm: &mut Vm, _t: &EncRef, _data: Option<Node>) -> SR<Value> {
    Ok(Value::Null)
}

fn to_xml_null(
    _vm: &mut Vm,
    _t: &EncRef,
    _data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let ret = bogus(parent);
    if style == SOAP_ENCODED {
        set_xsi_nil(ret);
    }
    Ok(ret)
}

// ---------------------------------------------------------------------------
// Properties of objects and arrays

fn set_zval_property(vm: &mut Vm, object: &Value, name: &str, val: Value) {
    match object {
        Value::Object(o) => write_property(vm, o, name, val),
        Value::Array(a) => {
            a.borrow_mut().set(
                ArrayKey::String(PhpString::from_string(name.to_string())),
                val,
            );
        }
        _ => {}
    }
}

thread_local! {
    /// Property guards of the magic methods (zend_get_property_guard()).
    static GUARDS: RefCell<HashSet<(usize, String, u8)>> = RefCell::new(HashSet::new());
}

/// Call magic method `m` (`__get`/`__set`/`__unset`) for a missing
/// property, unless already inside it for that property.
fn magic_call(
    vm: &mut Vm,
    o: &Obj,
    m: &str,
    kind: u8,
    name: &str,
    extra: Option<Value>,
) -> Option<Value> {
    let cl = lower(&o.borrow().class_name);
    if !class_method_exists(vm, &cl, m.as_bytes()) {
        return None;
    }
    let key = (Rc::as_ptr(o) as *const u8 as usize, name.to_string(), kind);
    if !GUARDS.with(|g| g.borrow_mut().insert(key.clone())) {
        return None;
    }
    let mut args = vec![vstr(name)];
    if let Some(e) = extra {
        args.push(e);
    }
    let r = call_method(vm, o, m, &args);
    GUARDS.with(|g| g.borrow_mut().remove(&key));
    r.ok().map(|v| v.deref())
}

fn has_storage(o: &Obj, key: &[u8]) -> bool {
    o.borrow()
        .properties
        .iter()
        .any(|(k, _)| k.as_slice() == key)
}

/// zend_update_property() from the object's class scope.
fn write_property(vm: &mut Vm, o: &Obj, name: &str, val: Value) {
    let val = coerce_typed_prop(vm, o, name, val);
    let key = own_prop_key(o, name);
    if !has_storage(o, &key) && magic_call(vm, o, "__set", 1, name, Some(val.clone())).is_some() {
        return;
    }
    let mut b = o.borrow_mut();
    if let Some(e) = b.properties.iter_mut().find(|(k, _)| *k == key) {
        match &e.1 {
            Value::Reference(r) => *r.borrow_mut() = val,
            _ => e.1 = val,
        }
        return;
    }
    b.properties.push((key, val));
}

/// The storage key of property `name` as seen from the object's class.
fn own_prop_key(o: &Obj, name: &str) -> Vec<u8> {
    let b = o.borrow();
    let mut m = vec![0u8];
    m.extend_from_slice(&b.class_name);
    m.push(0);
    m.extend_from_slice(name.as_bytes());
    if b.properties.iter().any(|(k, _)| *k == m) {
        return m;
    }
    name.as_bytes().to_vec()
}

/// Weak-mode coercion of a value written to a typed property of a
/// classmap class (zend_verify_property_type()).
fn coerce_typed_prop(vm: &mut Vm, o: &Obj, name: &str, val: Value) -> Value {
    let cl = lower(&o.borrow().class_name);
    let mut cur = Some(cl);
    let mut ty: Option<goro_core::opcode::ParamType> = None;
    let mut guard = 0;
    while let Some(c) = cur {
        guard += 1;
        if guard > 64 {
            break;
        }
        let Some(ce) = vm.classes.get(&c) else { break };
        if let Some(p) = ce
            .properties
            .iter()
            .find(|p| p.name == name.as_bytes() && !p.is_static)
        {
            ty = p.property_type.clone();
            break;
        }
        cur = ce.parent.as_ref().map(|p| lower(p));
    }
    let Some(ty) = ty else { return val };
    let s = format!("{:?}", ty).to_ascii_lowercase();
    let simple = |n: &str| s.contains(n);
    match &val {
        Value::String(st) => {
            if simple("int") && !simple("string") {
                if let Some((n, _)) = goro_core::value::parse_numeric_str(st.as_bytes(), false) {
                    return match n {
                        goro_core::value::NumStr::Long(l) => Value::Long(l),
                        goro_core::value::NumStr::Double(d) if simple("float") => Value::Double(d),
                        goro_core::value::NumStr::Double(d) => {
                            if d.fract() == 0.0 {
                                Value::Long(d as i64)
                            } else {
                                val
                            }
                        }
                    };
                }
            } else if simple("float") && !simple("string") {
                if let Some((n, _)) = goro_core::value::parse_numeric_str(st.as_bytes(), false) {
                    return match n {
                        goro_core::value::NumStr::Long(l) => Value::Double(l as f64),
                        goro_core::value::NumStr::Double(d) => Value::Double(d),
                    };
                }
            } else if simple("bool") && !simple("string") {
                return if val.is_truthy() {
                    Value::True
                } else {
                    Value::False
                };
            }
            val
        }
        Value::Long(l) if simple("float") && !simple("int") => Value::Double(*l as f64),
        Value::Long(l) if simple("string") && !simple("int") => vstr(l.to_string()),
        _ => val,
    }
}

/// get_zval_property(vm, ): the (deref'd) property of an object or the string
/// key of an array; None when not set.
fn get_zval_property(vm: &mut Vm, object: &Value, name: &str) -> Option<Value> {
    match object {
        Value::Object(o) => {
            let key = own_prop_key(o, name);
            let v = {
                let b = o.borrow();
                b.properties
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.deref())
            };
            match v {
                None => magic_call(vm, o, "__get", 0, name, None),
                Some(Value::Undef) => None,
                Some(v) => Some(v),
            }
        }
        Value::Array(a) => a.borrow().get_str(name.as_bytes()).map(|v| v.deref()),
        _ => None,
    }
}

fn unset_zval_property(vm: &mut Vm, object: &Value, name: &str) {
    match object {
        Value::Object(o) => {
            let key = own_prop_key(o, name);
            if !has_storage(o, &key) {
                magic_call(vm, o, "__unset", 2, name, None);
                return;
            }
            o.borrow_mut().properties.retain(|(k, _)| *k != key);
        }
        Value::Array(a) => {
            a.borrow_mut()
                .remove(&ArrayKey::String(PhpString::from_string(name.to_string())));
        }
        _ => {}
    }
}

fn model_to_zval_any(vm: &mut Vm, ret: &Value, node: Option<Node>) -> SR<()> {
    let mut any: Option<Value> = None;
    let mut name: Option<String> = None;
    let mut node = node;
    while let Some(n) = node {
        if get_zval_property(vm, ret, &xml::name(n)).is_none() {
            let val = master_to_zval(vm, Some(&conv(XSD_ANYXML)), n)?;
            if let Some(a) = &any {
                if !matches!(a, Value::Array(_)) {
                    let mut arr = PhpArray::new();
                    match &name {
                        Some(nm) => arr_set_str(&mut arr, nm, a.clone()),
                        None => arr.push(a.clone()),
                    }
                    any = Some(varr(arr));
                }
            }
            let mut val = val;
            let mut nn = n;
            if matches!(&val, Value::String(s) if s.as_bytes().first() == Some(&b'<')) {
                name = None;
                while let Some(nx) = xml::next(nn) {
                    let val2 = master_to_zval(vm, Some(&conv(XSD_ANYXML)), nx)?;
                    let ok = matches!(&val2, Value::String(_))
                        && matches!(&val, Value::String(s) if s.as_bytes().first() == Some(&b'<'));
                    if !ok {
                        break;
                    }
                    let mut s = as_bytes(&val).unwrap_or_default();
                    s.extend_from_slice(&as_bytes(&val2).unwrap_or_default());
                    val = vstr(s);
                    nn = nx;
                }
            } else {
                name = Some(xml::name(nn));
            }
            match &any {
                None => {
                    if let Some(nm) = name.take() {
                        let mut arr = PhpArray::new();
                        arr_set_str(&mut arr, &nm, val);
                        any = Some(varr(arr));
                    } else {
                        any = Some(val);
                    }
                }
                Some(Value::Array(a)) => {
                    if let Some(nm) = &name {
                        let existing = a.borrow().get(&symtable_key(nm.as_bytes())).cloned();
                        match existing {
                            Some(Value::Array(el)) => el.borrow_mut().push(val),
                            // The C code adds the value to a temporary array
                            // that is then dropped.
                            Some(_) => {}
                            None => arr_set_str(&mut a.borrow_mut(), nm, val),
                        }
                    } else {
                        a.borrow_mut().push(val);
                    }
                    name = None;
                }
                Some(_) => {}
            }
            node = xml::next(nn);
            continue;
        }
        node = xml::next(n);
    }
    if let Some(a) = any {
        set_zval_property(vm, ret, name.as_deref().unwrap_or("any"), a);
    }
    Ok(())
}

fn model_elem_value(vm: &mut Vm, el: &TypeRef, node: Node, resolved: Option<Node>) -> SR<Value> {
    let (fixed, def, nillable, enc, ename) = {
        let e = el.borrow();
        (
            e.fixed.clone(),
            e.def.clone(),
            e.nillable,
            e.encode.clone(),
            e.name.clone(),
        )
    };
    let r_node = resolved.unwrap_or(node);
    if xml::first_child_has_content(r_node) {
        let content = xml::children(r_node).map(xml::content).unwrap_or_default();
        if let Some(f) = &fixed {
            if *f != content {
                let m = format!(
                    "Encoding: Element '{}' has fixed value '{}' (value '{}' is not allowed)",
                    ename.unwrap_or_default(),
                    f,
                    content
                );
                return Err(soap_error(vm, m));
            }
        }
        return master_to_zval(vm, enc.as_ref(), r_node);
    }
    if let Some(f) = &fixed {
        let dummy = xml::new_doc_node(xml::doc(node), "BOGUS");
        xml::node_set_content(dummy, f);
        return master_to_zval(vm, enc.as_ref(), dummy);
    }
    if let Some(d) = def.filter(|_| !nillable) {
        let dummy = xml::new_doc_node(xml::doc(node), "BOGUS");
        xml::node_set_content(dummy, &d);
        return master_to_zval(vm, enc.as_ref(), dummy);
    }
    master_to_zval(vm, enc.as_ref(), r_node)
}

fn model_to_zval_object(vm: &mut Vm, ret: &Value, model: &ModelRef, data: Node) -> SR<()> {
    let (kind, u, max_occurs) = {
        let m = model.borrow();
        (m.kind, m.u.clone(), m.max_occurs)
    };
    match kind {
        XSD_CONTENT_ELEMENT => {
            let ModelU::Element(el) = u else {
                return Ok(());
            };
            let (ename, nillable) = {
                let e = el.borrow();
                (e.name.clone(), e.nillable)
            };
            let Some(ename) = ename else { return Ok(()) };
            let Some(node) = xml::get_node(xml::children(data), &ename) else {
                return Ok(());
            };
            let r_node = check_and_resolve_href(vm, Some(node))?;
            let mut val = model_elem_value(vm, &el, node, r_node)?;
            if let Some(mut n2) = xml::get_node(xml::next(node), &ename) {
                let mut arr = PhpArray::new();
                arr.push(val);
                loop {
                    let v = model_elem_value(vm, &el, n2, None)?;
                    arr.push(v);
                    match xml::get_node(xml::next(n2), &ename) {
                        Some(n) => n2 = n,
                        None => break,
                    }
                }
                val = varr(arr);
            } else if (!matches!(val, Value::Null) || !nillable)
                && g(|g| g.features) & SOAP_SINGLE_ELEMENT_ARRAYS != 0
                && (max_occurs == -1 || max_occurs > 1)
            {
                let mut arr = PhpArray::new();
                arr.push(val);
                val = varr(arr);
            }
            set_zval_property(vm, ret, &ename, val);
        }
        XSD_CONTENT_ALL | XSD_CONTENT_SEQUENCE | XSD_CONTENT_CHOICE => {
            let ModelU::Content(content) = u else {
                return Ok(());
            };
            let mut any = false;
            for tmp in &content {
                if tmp.borrow().kind == XSD_CONTENT_ANY {
                    any = true;
                } else {
                    model_to_zval_object(vm, ret, tmp, data)?;
                }
            }
            if any {
                model_to_zval_any(vm, ret, xml::children(data))?;
            }
        }
        XSD_CONTENT_GROUP => {
            if let ModelU::Group(gr) = u {
                let m = gr.borrow().model.clone();
                if let Some(m) = m {
                    model_to_zval_object(vm, ret, &m, data)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn type_kind(t: &TypeRef) -> u8 {
    t.borrow().kind
}

fn is_simple_kind(k: u8) -> bool {
    matches!(
        k,
        XSD_TYPEKIND_SIMPLE | XSD_TYPEKIND_LIST | XSD_TYPEKIND_UNION
    )
}

/// The encoder of the simple base type of a restriction/extension chain.
fn simple_base(enc: Option<EncRef>) -> Option<EncRef> {
    let mut enc = enc;
    let mut guard = 0;
    while let Some(e) = enc.clone() {
        guard += 1;
        if guard > 1000 {
            break;
        }
        match e.sdl_type() {
            Some(st) if !is_simple_kind(type_kind(&st)) => {
                enc = st.borrow().encode.clone();
            }
            _ => break,
        }
    }
    enc
}

/// object_init_ex(): None (with the Error pending) when the class cannot be
/// instantiated; decoding then goes on with NULL.
fn object_init(vm: &mut Vm, ce: &[u8]) -> Option<Value> {
    if vm.current_exception.is_some() {
        // zend_throw_error() while an exception is pending keeps the first
        let saved = vm.current_exception.take();
        let r = new_object_ex(vm, ce).ok().map(Value::Object);
        if vm.current_exception.is_none() {
            vm.current_exception = saved;
        }
        return r;
    }
    new_object_ex(vm, ce).ok().map(Value::Object)
}

fn to_zval_object_ex(
    vm: &mut Vm,
    type_: &EncRef,
    data: Option<Node>,
    pce: Option<&[u8]>,
) -> SR<Value> {
    let sdl_type = type_.sdl_type();
    let mut ce: Vec<u8> = b"stdClass".to_vec();
    if let Some(p) = pce {
        ce = p.to_vec();
    } else if let (Some(cm), Some(ts)) = (g(|g| g.class_map.clone()), type_.type_str()) {
        let clark = type_.d.borrow().clark.clone();
        let mut classname: Option<Value> = None;
        if let Some(c) = clark {
            classname = cm.borrow().get_str(c.as_bytes()).map(|v| v.deref());
        }
        if classname.is_none() {
            classname = cm.borrow().get_str(ts.as_bytes()).map(|v| v.deref());
        }
        if let Some(Value::String(cn)) = classname {
            if let Some(n) = lookup_class(vm, cn.as_bytes()) {
                ce = n;
            }
        }
    }
    let is_std = ce.eq_ignore_ascii_case(b"stdClass");
    let mut redo_any = false;
    let ret: Value;
    if let Some(st) = &sdl_type {
        let (kind, senc) = {
            let s = st.borrow();
            (s.kind, s.encode.clone())
        };
        let senc_is_self = senc.as_ref().is_some_and(|e| same_enc(e, type_));
        if kind == XSD_TYPEKIND_RESTRICTION && senc.is_some() && !senc_is_self {
            let enc = simple_base(senc.clone());
            let Some(d) = data else {
                return Ok(Value::Null);
            };
            if let Some(enc) = enc {
                if let Some(r) = soap_check_xml_ref(d) {
                    return Ok(r);
                }
                ret = match object_init(vm, &ce) {
                    Some(v) => v,
                    None => return Ok(Value::Null),
                };
                let base = master_to_zval_int(vm, &enc, d)?;
                set_zval_property(vm, &ret, "_", base);
            } else {
                if xml_is_null(data) {
                    return Ok(Value::Null);
                }
                if let Some(r) = soap_check_xml_ref(d) {
                    return Ok(r);
                }
                ret = match object_init(vm, &ce) {
                    Some(v) => v,
                    None => return Ok(Value::Null),
                };
                soap_add_xml_ref(&ret, d);
            }
        } else if kind == XSD_TYPEKIND_EXTENSION && senc.is_some() && !senc_is_self {
            let senc = senc.unwrap();
            let base_st = senc.sdl_type();
            if base_st
                .as_ref()
                .is_some_and(|b| !is_simple_kind(type_kind(b)))
            {
                if xml_is_null(data) {
                    return Ok(Value::Null);
                }
                let d = data.unwrap();
                if let Some(r) = soap_check_xml_ref(d) {
                    return Ok(r);
                }
                let base_st = base_st.unwrap();
                let (bkind, benc) = {
                    let b = base_st.borrow();
                    (b.kind, b.encode.clone())
                };
                let r = if !is_std
                    && senc.to_zval.get() as usize == sdl_guess_convert_zval as ToZval as usize
                    && matches!(
                        bkind,
                        XSD_TYPEKIND_COMPLEX | XSD_TYPEKIND_RESTRICTION | XSD_TYPEKIND_EXTENSION
                    )
                    && benc
                        .as_ref()
                        .is_none_or(|e| e.type_() != IS_ARRAY && e.type_() != SOAP_ENC_ARRAY)
                {
                    to_zval_object_ex(vm, &senc, Some(d), Some(&ce))?
                } else {
                    master_to_zval_int(vm, &senc, d)?
                };
                soap_add_xml_ref(&r, d);
                redo_any = get_zval_property(vm, &r, "any").is_some();
                if let Value::Object(o) = &r {
                    if !is_std {
                        let name = vm
                            .classes
                            .get(&lower(&ce))
                            .map(|c| c.name.clone())
                            .unwrap_or(ce.clone());
                        o.borrow_mut().set_class_name(name);
                    }
                }
                ret = r;
            } else {
                let Some(d) = data else {
                    return Ok(Value::Null);
                };
                if let Some(r) = soap_check_xml_ref(d) {
                    return Ok(r);
                }
                ret = match object_init(vm, &ce) {
                    Some(v) => v,
                    None => return Ok(Value::Null),
                };
                soap_add_xml_ref(&ret, d);
                let base = master_to_zval_int(vm, &senc, d)?;
                set_zval_property(vm, &ret, "_", base);
            }
        } else {
            if xml_is_null(data) {
                return Ok(Value::Null);
            }
            let d = data.unwrap();
            if let Some(r) = soap_check_xml_ref(d) {
                return Ok(r);
            }
            ret = match object_init(vm, &ce) {
                Some(v) => v,
                None => return Ok(Value::Null),
            };
            soap_add_xml_ref(&ret, d);
        }
        let d = data.unwrap();
        let (model, attrs) = {
            let s = st.borrow();
            (s.model.clone(), s.attributes.clone())
        };
        if let Some(m) = model {
            model_to_zval_object(vm, &ret, &m, d)?;
            if redo_any {
                if get_zval_property(vm, &ret, "any").is_none() {
                    model_to_zval_any(vm, &ret, xml::children(d))?;
                    soap_add_xml_ref(&ret, d);
                } else {
                    unset_zval_property(vm, &ret, "any");
                }
            }
        }
        if let Some(attrs) = attrs {
            for attr in attrs.values() {
                let (aname, fixed, def, aenc) = {
                    let a = attr.borrow();
                    (
                        a.name.clone(),
                        a.fixed.clone(),
                        a.def.clone(),
                        a.encode.clone(),
                    )
                };
                let Some(aname) = aname else { continue };
                let val = xml::get_attribute_any_ns(xml::properties(d), &aname);
                let mut str_val: Option<String> = None;
                if let Some(v) = val.filter(|v| xml::attr_has_value(*v)) {
                    let s = xml::attr_value(v);
                    if let Some(f) = &fixed {
                        if *f != s {
                            let m = format!(
                                "Encoding: Attribute '{}' has fixed value '{}' (value '{}' is not allowed)",
                                aname, f, s
                            );
                            return Err(soap_error(vm, m));
                        }
                    }
                    str_val = Some(s);
                } else if let Some(f) = fixed {
                    str_val = Some(f);
                } else if let Some(df) = def {
                    str_val = Some(df);
                }
                if let Some(sv) = str_val {
                    let dummy = xml::new_doc_node(None, "BOGUS");
                    xml::add_text(dummy, &sv);
                    let v = master_to_zval(vm, aenc.as_ref(), dummy)?;
                    set_zval_property(vm, &ret, &aname, v);
                }
            }
        }
        return Ok(ret);
    }
    if xml_is_null(data) {
        return Ok(Value::Null);
    }
    let d = data.unwrap();
    if let Some(r) = soap_check_xml_ref(d) {
        return Ok(r);
    }
    let ret = match object_init(vm, &ce) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    soap_add_xml_ref(&ret, d);
    let mut trav = xml::children(d);
    while let Some(tr) = trav {
        if xml::is_element(tr) {
            let tmp = master_to_zval(vm, None, tr)?;
            let tname = xml::name(tr);
            match get_zval_property(vm, &ret, &tname) {
                None => {
                    if xml::next(tr).is_none() || xml::get_node(xml::next(tr), &tname).is_none() {
                        set_zval_property(vm, &ret, &tname, tmp);
                    } else {
                        let mut arr = PhpArray::new();
                        arr.push(tmp);
                        set_zval_property(vm, &ret, &tname, varr(arr));
                    }
                }
                Some(Value::Array(a)) => {
                    a.borrow_mut().push(tmp);
                }
                Some(p) => {
                    let mut arr = PhpArray::new();
                    arr.push(p);
                    arr.push(tmp);
                    set_zval_property(vm, &ret, &tname, varr(arr));
                }
            }
        }
        trav = xml::next(tr);
    }
    Ok(ret)
}

fn to_zval_object(vm: &mut Vm, type_: &EncRef, data: Option<Node>) -> SR<Value> {
    to_zval_object_ex(vm, type_, data, None)
}

fn check_fixed_node(vm: &mut Vm, el: &TypeRef, property: Node) -> SR<()> {
    let (fixed, name) = {
        let e = el.borrow();
        (e.fixed.clone(), e.name.clone())
    };
    if let Some(f) = fixed {
        if xml::first_child_has_content(property) {
            let c = xml::children(property)
                .map(xml::content)
                .unwrap_or_default();
            if c != f {
                let m = format!(
                    "Encoding: Element '{}' has fixed value '{}' (value '{}' is not allowed)",
                    name.unwrap_or_default(),
                    f,
                    c
                );
                return Err(soap_error(vm, m));
            }
        }
    }
    Ok(())
}

fn qualify_element(el: &TypeRef, property: Node, style: i32) {
    let (namens, form) = {
        let e = el.borrow();
        (e.namens.clone(), e.form)
    };
    if style == SOAP_LITERAL && namens.is_some() && form == XSD_FORM_QUALIFIED {
        let nsp = encode_add_ns(property, namens.as_deref());
        xml::set_ns(property, nsp);
    }
}

fn model_to_xml_object(
    vm: &mut Vm,
    node: Node,
    model: &ModelRef,
    object: &Value,
    style: i32,
    strict: bool,
) -> SR<i32> {
    let (kind, u, min_occurs, max_occurs) = {
        let m = model.borrow();
        (m.kind, m.u.clone(), m.min_occurs, m.max_occurs)
    };
    match kind {
        XSD_CONTENT_ELEMENT => {
            let ModelU::Element(el) = u else { return Ok(1) };
            let (ename, nillable, enc) = {
                let e = el.borrow();
                (
                    e.name.clone().unwrap_or_default(),
                    e.nillable,
                    e.encode.clone(),
                )
            };
            let data = get_zval_property(vm, object, &ename);
            if let Some(d) = &data {
                if matches!(d, Value::Null) && !nillable && min_occurs > 0 && !strict {
                    return Ok(0);
                }
            }
            if let Some(d) = data {
                let is_list_arr = matches!(&d, Value::Array(a) if is_list(&a.borrow()));
                if (max_occurs == -1 || max_occurs > 1) && is_list_arr {
                    let items: Vec<Value> = match &d {
                        Value::Array(a) => a.borrow().values().cloned().collect(),
                        _ => Vec::new(),
                    };
                    for item in items {
                        let property;
                        if matches!(item.deref(), Value::Null) && nillable {
                            property = bogus(node);
                            set_xsi_nil(property);
                        } else {
                            property = master_to_xml(vm, enc.as_ref(), Some(&item), style, node)?;
                            check_fixed_node(vm, &el, property)?;
                        }
                        xml::set_name(property, &ename);
                        qualify_element(&el, property, style);
                    }
                } else {
                    let property;
                    if matches!(d, Value::Null) && nillable {
                        property = bogus(node);
                        set_xsi_nil(property);
                    } else if matches!(d, Value::Null) && min_occurs == 0 {
                        return Ok(1);
                    } else {
                        property = master_to_xml(vm, enc.as_ref(), Some(&d), style, node)?;
                        check_fixed_node(vm, &el, property)?;
                    }
                    xml::set_name(property, &ename);
                    qualify_element(&el, property, style);
                }
                return Ok(1);
            } else if strict && nillable && min_occurs > 0 {
                let property = xml::new_doc_node(xml::doc(node), &ename);
                xml::add_child(node, property);
                set_xsi_nil(property);
                qualify_element(&el, property, style);
                return Ok(1);
            } else if min_occurs == 0 {
                return Ok(2);
            } else {
                if strict {
                    let m = format!("Encoding: object has no '{}' property", ename);
                    return Err(soap_error(vm, m));
                }
                return Ok(0);
            }
        }
        XSD_CONTENT_ANY => {
            let data = get_zval_property(vm, object, "any");
            if let Some(d) = data {
                let enc = conv(XSD_ANYXML);
                let is_list_arr = matches!(&d, Value::Array(a) if is_list(&a.borrow()));
                if (max_occurs == -1 || max_occurs > 1) && is_list_arr {
                    let items: Vec<Value> = match &d {
                        Value::Array(a) => a.borrow().values().cloned().collect(),
                        _ => Vec::new(),
                    };
                    for item in items {
                        master_to_xml(vm, Some(&enc), Some(&item), style, node)?;
                    }
                } else {
                    master_to_xml(vm, Some(&enc), Some(&d), style, node)?;
                }
                return Ok(1);
            } else if min_occurs == 0 {
                return Ok(2);
            } else {
                if strict {
                    return Err(soap_error(vm, "Encoding: object has no 'any' property"));
                }
                return Ok(0);
            }
        }
        XSD_CONTENT_SEQUENCE | XSD_CONTENT_ALL => {
            let ModelU::Content(content) = u else {
                return Ok(1);
            };
            let mut strict = strict;
            for tmp in &content {
                let tmin = tmp.borrow().min_occurs;
                if model_to_xml_object(vm, node, tmp, object, style, strict && tmin > 0)? == 0
                    && (!strict || tmin > 0)
                {
                    return Ok(0);
                }
                strict = true;
            }
            return Ok(1);
        }
        XSD_CONTENT_CHOICE => {
            let ModelU::Content(content) = u else {
                return Ok(1);
            };
            let mut ret = 0;
            for tmp in &content {
                let r = model_to_xml_object(vm, node, tmp, object, style, false)?;
                if r == 1 {
                    return Ok(1);
                } else if r != 0 {
                    ret = 1;
                }
            }
            return Ok(ret);
        }
        XSD_CONTENT_GROUP => {
            if let ModelU::Group(gr) = u {
                let m = gr.borrow().model.clone();
                if let Some(m) = m {
                    return model_to_xml_object(
                        vm,
                        node,
                        &m,
                        object,
                        style,
                        strict && min_occurs > 0,
                    );
                }
            }
        }
        _ => {}
    }
    Ok(1)
}

fn model_array_element(model: &ModelRef) -> Option<TypeRef> {
    let (kind, u, max) = {
        let m = model.borrow();
        (m.kind, m.u.clone(), m.max_occurs)
    };
    match kind {
        XSD_CONTENT_ELEMENT => {
            if max == -1 || max > 1 {
                if let ModelU::Element(e) = u {
                    return Some(e);
                }
            }
            None
        }
        XSD_CONTENT_SEQUENCE | XSD_CONTENT_ALL | XSD_CONTENT_CHOICE => {
            let ModelU::Content(c) = u else { return None };
            if c.len() != 1 {
                return None;
            }
            model_array_element(&c[0])
        }
        XSD_CONTENT_GROUP => {
            if let ModelU::Group(gr) = u {
                let m = gr.borrow().model.clone();
                return m.and_then(|m| model_array_element(&m));
            }
            None
        }
        _ => None,
    }
}

/// Unmangle a property table key (zend_unmangle_property_name()).
fn unmangle(key: &[u8]) -> Vec<u8> {
    if key.first() == Some(&0) {
        if let Some(p) = key[1..].iter().position(|&c| c == 0) {
            return key[p + 2..].to_vec();
        }
    }
    key.to_vec()
}

fn to_xml_object(
    vm: &mut Vm,
    type_: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let sdl_type = type_.sdl_type();
    let dv = data.map(|d| d.deref());
    if dv.is_none() || matches!(dv, Some(Value::Null)) {
        let x = bogus(parent);
        if style == SOAP_ENCODED {
            set_xsi_nil(x);
            set_ns_and_type(x, type_);
        }
        return Ok(x);
    }
    let dv = dv.unwrap();
    let data = data.unwrap();
    let has_prop = matches!(dv, Value::Object(_) | Value::Array(_));
    if let Some(st) = &sdl_type {
        let (kind, senc, attrs, model) = {
            let s = st.borrow();
            (
                s.kind,
                s.encode.clone(),
                s.attributes.clone(),
                s.model.clone(),
            )
        };
        let senc_is_self = senc.as_ref().is_some_and(|e| same_enc(e, type_));
        let x_param;
        if kind == XSD_TYPEKIND_RESTRICTION && senc.is_some() && !senc_is_self {
            let enc = simple_base(senc.clone());
            if let Some(enc) = enc {
                if let Some(tmp) = get_zval_property(vm, &dv, "_") {
                    x_param = master_to_xml(vm, Some(&enc), Some(&tmp), style, parent)?;
                } else if !has_prop {
                    x_param = master_to_xml(vm, Some(&enc), Some(data), style, parent)?;
                } else {
                    x_param = bogus(parent);
                }
            } else {
                x_param = bogus(parent);
            }
        } else if kind == XSD_TYPEKIND_EXTENSION && senc.is_some() && !senc_is_self {
            let senc = senc.unwrap();
            let base_st = senc.sdl_type();
            if base_st
                .as_ref()
                .is_some_and(|b| !is_simple_kind(type_kind(b)))
            {
                // GC_TRY_PROTECT_RECURSION(prop): the class map is not
                // consulted again for this value.
                let guard = recursion_enter(&dv);
                let r = master_to_xml(vm, Some(&senc), Some(data), style, parent);
                if guard == Some(true) {
                    recursion_leave(&dv);
                }
                x_param = r?;
            } else if let Some(tmp) = get_zval_property(vm, &dv, "_") {
                x_param = master_to_xml(vm, Some(&senc), Some(&tmp), style, parent)?;
            } else if !has_prop {
                x_param = master_to_xml(vm, Some(&senc), Some(data), style, parent)?;
            } else {
                x_param = bogus(parent);
            }
        } else {
            x_param = bogus(parent);
        }
        if soap_check_zval_ref(data, x_param) {
            return Ok(x_param);
        }
        if has_prop {
            let array_el =
                if matches!(&dv, Value::Array(a) if is_list(&a.borrow())) && attrs.is_none() {
                    model.as_ref().and_then(model_array_element)
                } else {
                    None
                };
            if let Some(array_el) = array_el {
                let items: Vec<Value> = match &dv {
                    Value::Array(a) => a.borrow().values().cloned().collect(),
                    _ => Vec::new(),
                };
                let (aname, anil, aenc) = {
                    let e = array_el.borrow();
                    (
                        e.name.clone().unwrap_or_default(),
                        e.nillable,
                        e.encode.clone(),
                    )
                };
                for val in items {
                    let property;
                    if matches!(val.deref(), Value::Null) && anil {
                        property = bogus(x_param);
                        set_xsi_nil(property);
                    } else {
                        property = master_to_xml(vm, aenc.as_ref(), Some(&val), style, x_param)?;
                    }
                    xml::set_name(property, &aname);
                    qualify_element(&array_el, property, style);
                }
            } else if let Some(m) = &model {
                model_to_xml_object(vm, x_param, m, &dv, style, true)?;
            }
            if let Some(attrs) = attrs {
                // Attributes can't refer to other attributes.
                let old = take_ref_map();
                let r = (|| -> SR<()> {
                    for attr in attrs.values() {
                        let (aname, aenc, fixed, namens, form) = {
                            let a = attr.borrow();
                            (
                                a.name.clone(),
                                a.encode.clone(),
                                a.fixed.clone(),
                                a.namens.clone(),
                                a.form,
                            )
                        };
                        let Some(aname) = aname else { continue };
                        let Some(zattr) = get_zval_property(vm, &dv, &aname) else {
                            continue;
                        };
                        let dummy =
                            master_to_xml(vm, aenc.as_ref(), Some(&zattr), SOAP_LITERAL, x_param)?;
                        if xml::first_child_has_content(dummy) {
                            let c = xml::children(dummy).map(xml::content).unwrap_or_default();
                            if let Some(f) = &fixed {
                                if *f != c {
                                    let m = format!(
                                        "Encoding: Attribute '{}' has fixed value '{}' (value '{}' is not allowed)",
                                        aname, f, c
                                    );
                                    return Err(soap_error(vm, m));
                                }
                            }
                            if namens.as_deref().is_some_and(|ns| ns == XML_NAMESPACE)
                                || (namens.is_some() && form == XSD_FORM_QUALIFIED)
                            {
                                let nsp = encode_add_ns(x_param, namens.as_deref());
                                xml::set_ns_prop(x_param, nsp, &aname, &c);
                            } else {
                                xml::set_prop(x_param, &aname, &c);
                            }
                        }
                        xml::unlink(dummy);
                    }
                    Ok(())
                })();
                restore_ref_map(old);
                r?;
            }
        }
        if style == SOAP_ENCODED {
            set_ns_and_type(x_param, type_);
        }
        return Ok(x_param);
    }
    let x_param = bogus(parent);
    if soap_check_zval_ref(data, x_param) {
        return Ok(x_param);
    }
    if has_prop {
        let entries: Vec<(Option<Vec<u8>>, Value)> = match &dv {
            Value::Object(o) => vm
                .object_prop_table(o)
                .into_iter()
                .filter(|e| !matches!(e.value, Value::Undef))
                .map(|e| (Some(unmangle(&e.key)), e.value))
                .collect(),
            Value::Array(a) => a
                .borrow()
                .iter()
                .map(|(k, v)| {
                    (
                        match k {
                            ArrayKey::String(s) => Some(s.as_bytes().to_vec()),
                            ArrayKey::Int(_) => None,
                        },
                        v.clone(),
                    )
                })
                .collect(),
            _ => Vec::new(),
        };
        for (key, zprop) in entries {
            let zt = zval_type(&zprop);
            let enc = get_conversion(vm, zt)?;
            let property = master_to_xml(vm, Some(&enc), Some(&zprop), style, x_param)?;
            if let Some(k) = key {
                xml::set_name(property, &String::from_utf8_lossy(&k));
            }
        }
    }
    if style == SOAP_ENCODED {
        set_ns_and_type(x_param, type_);
    }
    Ok(x_param)
}

// ---------------------------------------------------------------------------
// Arrays

thread_local! {
    static RECURSION: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// GC_TRY_PROTECT_RECURSION(): Some(true) when protected now, Some(false)
/// when already being encoded, None for non-arrays.
fn recursion_key(v: &Value) -> Option<usize> {
    match v {
        Value::Array(a) => Some(Rc::as_ptr(a) as *const u8 as usize),
        Value::Object(o) => Some(Rc::as_ptr(o) as *const u8 as usize),
        _ => None,
    }
}

fn recursion_enter(v: &Value) -> Option<bool> {
    let p = recursion_key(v)?;
    Some(RECURSION.with(|r| r.borrow_mut().insert(p)))
}

fn recursion_leave(v: &Value) {
    if let Some(p) = recursion_key(v) {
        RECURSION.with(|r| r.borrow_mut().remove(&p));
    }
}

/// GC_IS_RECURSIVE()
fn is_recursive(v: &Value) -> bool {
    match recursion_key(v) {
        Some(p) => RECURSION.with(|r| r.borrow().contains(&p)),
        None => false,
    }
}

fn guess_array_map(
    vm: &mut Vm,
    _t: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let mut enc = None;
    if let Some(Value::Array(a)) = data.map(|d| d.deref()) {
        enc = Some(if !is_list(&a.borrow()) {
            get_conversion(vm, APACHE_MAP)?
        } else {
            get_conversion(vm, SOAP_ENC_ARRAY)?
        });
    }
    let enc = match enc {
        Some(e) => e,
        None => get_conversion(vm, IS_NULL)?,
    };
    master_to_xml(vm, Some(&enc), data, style, parent)
}

fn calc_dimension_12(vm: &mut Vm, s: &str) -> SR<usize> {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut p = 0usize;
    let mut flag = false;
    while p < b.len() && !b[p].is_ascii_digit() && b[p] != b'*' {
        p += 1;
    }
    if p < b.len() && b[p] == b'*' {
        i += 1;
        p += 1;
    }
    while p < b.len() {
        if b[p].is_ascii_digit() {
            if !flag {
                i += 1;
                flag = true;
            }
        } else if b[p] == b'*' {
            return Err(soap_error(
                vm,
                "Encoding: '*' may only be first arraySize value in list",
            ));
        } else {
            flag = false;
        }
        p += 1;
    }
    Ok(i)
}

fn get_position_12(vm: &mut Vm, dimension: usize, s: &str) -> SR<Vec<i64>> {
    let mut pos = vec![0i64; dimension.max(1)];
    let b = s.as_bytes();
    let mut i: isize = -1;
    let mut flag = false;
    let mut p = 0usize;
    while p < b.len() && !b[p].is_ascii_digit() && b[p] != b'*' {
        p += 1;
    }
    if p < b.len() && b[p] == b'*' {
        p += 1;
        i += 1;
    }
    while p < b.len() {
        if b[p].is_ascii_digit() {
            if !flag {
                i += 1;
                flag = true;
            }
            if (i as usize) < pos.len() {
                pos[i as usize] = pos[i as usize] * 10 + (b[p] - b'0') as i64;
            }
        } else if b[p] == b'*' {
            return Err(soap_error(
                vm,
                "Encoding: '*' may only be first arraySize value in list",
            ));
        } else {
            flag = false;
        }
        p += 1;
    }
    Ok(pos)
}

fn calc_dimension(s: &str) -> usize {
    let mut i = 1;
    for c in s.bytes() {
        if c == b']' {
            break;
        }
        if c == b',' {
            i += 1;
        }
    }
    i
}

fn get_position_ex(dimension: usize, s: &str, pos: &mut [i64]) {
    for p in pos.iter_mut() {
        *p = 0;
    }
    let mut i = 0usize;
    for c in s.bytes() {
        if c == b']' || i >= dimension {
            break;
        }
        if c.is_ascii_digit() {
            pos[i] = pos[i] * 10 + (c - b'0') as i64;
        } else if c == b',' {
            i += 1;
        }
    }
}

fn get_position(dimension: usize, s: &str) -> Vec<i64> {
    let mut pos = vec![0i64; dimension];
    get_position_ex(dimension, s, &mut pos);
    pos
}

fn name_array_item(
    x: Node,
    type_: Option<&TypeRef>,
    enc: Option<&EncRef>,
    ns: Option<usize>,
    style: i32,
) {
    if let Some(t) = type_ {
        let n = t.borrow().name.clone().unwrap_or_default();
        xml::set_name(x, &n);
    } else if let (true, Some(ts)) = (style == SOAP_LITERAL, enc.and_then(|e| e.type_str())) {
        xml::set_name(x, &ts);
        xml::set_ns(x, ns);
    } else {
        xml::set_name(x, "item");
    }
}

#[allow(clippy::too_many_arguments)]
fn add_xml_array_elements(
    vm: &mut Vm,
    x_param: Node,
    type_: Option<&TypeRef>,
    enc: Option<&EncRef>,
    ns: Option<usize>,
    dimension: usize,
    dims: &[i64],
    data: Option<&Value>,
    style: i32,
) -> SR<()> {
    let dv = data.map(|d| d.deref());
    if let Some(Value::Array(a)) = &dv {
        let dvv = dv.clone().unwrap();
        if recursion_enter(&dvv) == Some(false) {
            return Err(crate::zpp::throw(
                vm,
                b"ValueError",
                "Recursive array cannot be encoded",
            ));
        }
        let items: Vec<Value> = a.borrow().values().cloned().collect();
        let mut j = 0i64;
        let r = (|| -> SR<()> {
            for zdata in items {
                if j >= dims[0] {
                    break;
                }
                if dimension == 1 {
                    let x = match enc {
                        None => {
                            let e = get_conversion(vm, zval_type(&zdata))?;
                            master_to_xml(vm, Some(&e), Some(&zdata), style, x_param)?
                        }
                        Some(e) => master_to_xml(vm, Some(e), Some(&zdata), style, x_param)?,
                    };
                    name_array_item(x, type_, enc, ns, style);
                } else {
                    add_xml_array_elements(
                        vm,
                        x_param,
                        type_,
                        enc,
                        ns,
                        dimension - 1,
                        &dims[1..],
                        Some(&zdata),
                        style,
                    )?;
                }
                j += 1;
            }
            if dimension == 1 {
                while j < dims[0] {
                    let x = bogus(x_param);
                    name_array_item(x, type_, enc, ns, style);
                    j += 1;
                }
            } else {
                while j < dims[0] {
                    add_xml_array_elements(
                        vm,
                        x_param,
                        type_,
                        enc,
                        ns,
                        dimension - 1,
                        &dims[1..],
                        None,
                        style,
                    )?;
                    j += 1;
                }
            }
            Ok(())
        })();
        recursion_leave(&dvv);
        r
    } else {
        for _ in 0..dims[0].max(0) {
            if dimension == 1 {
                let x = bogus(x_param);
                name_array_item(x, type_, enc, ns, style);
            } else {
                add_xml_array_elements(
                    vm,
                    x_param,
                    type_,
                    enc,
                    ns,
                    dimension - 1,
                    &dims[1..],
                    None,
                    style,
                )?;
            }
        }
        Ok(())
    }
}

fn extra_attr(st: &TypeRef, attr_key: &str, ext_key: &str) -> Option<ExtraAttr> {
    let s = st.borrow();
    let a = s.attributes.as_ref()?.find(attr_key)?.clone();
    let a = a.borrow();
    a.extra.as_ref()?.find(ext_key).cloned()
}

fn single_element(st: &TypeRef) -> Option<TypeRef> {
    let s = st.borrow();
    let els = s.elements.as_ref()?;
    if els.len() != 1 {
        return None;
    }
    els.first().cloned()
}

/// Iterate an object implementing Traversable into an array.
fn traversable_to_array(vm: &mut Vm, o: &Value) -> SR<Value> {
    let f = vm.functions.get(b"iterator_to_array".as_slice()).copied();
    match f {
        Some(f) => match f(vm, &[o.clone(), Value::True]) {
            Ok(v) => {
                if vm.current_exception.is_some() {
                    return Err(Bail);
                }
                Ok(v)
            }
            Err(e) => {
                if vm.current_exception.is_none() {
                    PENDING_FATAL.with(|p| *p.borrow_mut() = Some(e));
                }
                Err(Bail)
            }
        },
        None => Ok(varr(PhpArray::new())),
    }
}

fn to_xml_array(
    vm: &mut Vm,
    type_: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let sdl_type = type_.sdl_type();
    let soap_version = soap_version();
    let x_param = bogus(parent);
    let mut dv = data.map(|d| d.deref());
    if dv.is_none() || matches!(dv, Some(Value::Null)) {
        if style == SOAP_ENCODED {
            set_xsi_nil(x_param);
            if g(|g| g.features) & SOAP_USE_XSI_ARRAY_TYPE != 0 {
                let ns = if soap_version == SOAP_1_1 {
                    SOAP_1_1_ENC_NAMESPACE
                } else {
                    SOAP_1_2_ENC_NAMESPACE
                };
                set_ns_and_type_ex(x_param, Some(ns), "Array");
            } else {
                set_ns_and_type(x_param, type_);
            }
        }
        return Ok(x_param);
    }
    if let Some(Value::Object(o)) = &dv {
        if instance_of(vm, o, "Traversable") {
            let o = o.clone();
            dv = Some(traversable_to_array(vm, &Value::Object(o))?);
        }
    } else if let Some(Value::Generator(gref)) = &dv {
        if matches!(
            gref.borrow().state,
            goro_core::generator::GeneratorState::Completed
        ) {
            return Err(crate::zpp::throw(
                vm,
                b"Exception",
                "Cannot traverse an already closed generator",
            ));
        }
        let gv = Value::Generator(gref.clone());
        dv = Some(traversable_to_array(vm, &gv)?);
    }
    let dv = dv.unwrap();
    if let Value::Array(arr) = &dv {
        let mut i = arr.borrow().len() as i64;
        let mut array_type = String::new();
        let mut array_size = String::new();
        let mut enc: Option<EncRef> = None;
        let mut element_type: Option<TypeRef> = None;
        let mut dimension = 1usize;
        let dims: Vec<i64>;
        let ext_at = sdl_type.as_ref().and_then(|st| {
            extra_attr(
                st,
                &format!("{}:arrayType", SOAP_1_1_ENC_NAMESPACE),
                &format!("{}:arrayType", WSDL_NAMESPACE),
            )
        });
        let ext_it = sdl_type.as_ref().and_then(|st| {
            extra_attr(
                st,
                &format!("{}:itemType", SOAP_1_2_ENC_NAMESPACE),
                &format!("{}:itemType", WSDL_NAMESPACE),
            )
        });
        let ext_as = sdl_type.as_ref().and_then(|st| {
            extra_attr(
                st,
                &format!("{}:arraySize", SOAP_1_2_ENC_NAMESPACE),
                &format!("{}:arraySize", WSDL_NAMESPACE),
            )
        });
        if let Some(ext) = ext_at {
            let mut value = ext.val.clone();
            if let Some(p) = value.rfind('[') {
                let end = value[p + 1..].to_string();
                value.truncate(p);
                dimension = calc_dimension(&end);
            }
            if let Some(ns) = &ext.ns {
                enc = get_encoder(cur_sdl().as_ref(), Some(ns), &value);
                get_type_str(x_param, Some(ns), &value, &mut array_type);
            } else {
                array_type.push_str(&value);
            }
            let mut d = vec![0i64; dimension];
            d[0] = i;
            let mut el = dv.clone();
            for di in d.iter_mut().skip(1) {
                if let Value::Array(a) = &el {
                    if !a.borrow().is_empty() {
                        let first = a
                            .borrow()
                            .values()
                            .next()
                            .cloned()
                            .unwrap_or(Value::Null)
                            .deref();
                        el = first;
                        *di = match &el {
                            Value::Array(a2) => a2.borrow().len() as i64,
                            _ => 0,
                        };
                    }
                }
            }
            array_size.push_str(&d[0].to_string());
            for x in &d[1..] {
                array_size.push(',');
                array_size.push_str(&x.to_string());
            }
            dims = d;
        } else if let Some(ext) = ext_it {
            if let Some(ns) = &ext.ns {
                enc = get_encoder(cur_sdl().as_ref(), Some(ns), &ext.val);
                get_type_str(x_param, Some(ns), &ext.val, &mut array_type);
            } else {
                array_type.push_str(&ext.val);
            }
            if let Some(ext2) = ext_as {
                dimension = calc_dimension_12(vm, &ext2.val)?;
                let mut d = get_position_12(vm, dimension, &ext2.val)?;
                if d[0] == 0 {
                    d[0] = i;
                }
                array_size.push_str(&d[0].to_string());
                for x in d.iter().take(dimension).skip(1) {
                    array_size.push(',');
                    array_size.push_str(&x.to_string());
                }
                dims = d;
            } else {
                dims = vec![0];
                array_size.push_str(&i.to_string());
            }
        } else if let Some(ext) = ext_as {
            dimension = calc_dimension_12(vm, &ext.val)?;
            let mut d = get_position_12(vm, dimension, &ext.val)?;
            if d[0] == 0 {
                d[0] = i;
            }
            array_size.push_str(&d[0].to_string());
            for x in d.iter().take(dimension).skip(1) {
                array_size.push(',');
                array_size.push_str(&x.to_string());
            }
            dims = d;
            let et = sdl_type.as_ref().and_then(single_element);
            let et_enc = et.as_ref().and_then(|e| e.borrow().encode.clone());
            if let (Some(et), Some(ee)) = (et, et_enc.filter(|e| e.type_str().is_some())) {
                element_type = Some(et);
                get_type_str(
                    x_param,
                    ee.ns().as_deref(),
                    &ee.type_str().unwrap(),
                    &mut array_type,
                );
                enc = Some(ee);
            } else {
                enc = get_array_type(vm, x_param, &dv, &mut array_type)?;
            }
        } else if let Some((et, ee)) = sdl_type.as_ref().and_then(single_element).and_then(|et| {
            let e = et.borrow().encode.clone();
            e.filter(|e| e.type_str().is_some()).map(|e| (et, e))
        }) {
            element_type = Some(et);
            get_type_str(
                x_param,
                ee.ns().as_deref(),
                &ee.type_str().unwrap(),
                &mut array_type,
            );
            enc = Some(ee);
            array_size.push_str(&i.to_string());
            dims = vec![i];
        } else {
            enc = get_array_type(vm, x_param, &dv, &mut array_type)?;
            array_size.push_str(&i.to_string());
            dims = vec![i];
        }
        if style == SOAP_ENCODED {
            if soap_version == SOAP_1_1 {
                if array_type == "xsd:anyType" {
                    array_type = "xsd:ur-type".to_string();
                }
                array_type.push('[');
                array_type.push_str(&array_size);
                array_type.push(']');
                set_ns_prop(x_param, SOAP_1_1_ENC_NAMESPACE, "arrayType", &array_type);
            } else {
                let size = array_size.replace(',', " ");
                set_ns_prop(x_param, SOAP_1_2_ENC_NAMESPACE, "itemType", &array_type);
                set_ns_prop(x_param, SOAP_1_2_ENC_NAMESPACE, "arraySize", &size);
            }
        }
        let ns = match &enc {
            Some(e) => encode_add_ns(x_param, e.ns().as_deref()),
            None => None,
        };
        i = 0;
        let _ = i;
        add_xml_array_elements(
            vm,
            x_param,
            element_type.as_ref(),
            enc.as_ref(),
            ns,
            dimension,
            &dims,
            Some(&dv),
            style,
        )?;
    }
    if style == SOAP_ENCODED {
        if g(|g| g.features) & SOAP_USE_XSI_ARRAY_TYPE != 0 {
            let ns = if soap_version == SOAP_1_1 {
                SOAP_1_1_ENC_NAMESPACE
            } else {
                SOAP_1_2_ENC_NAMESPACE
            };
            set_ns_and_type_ex(x_param, Some(ns), "Array");
        } else {
            set_ns_and_type(x_param, type_);
        }
    }
    Ok(x_param)
}

fn get_soap_enc_attribute(props: Option<Node>, name: &str) -> Option<Node> {
    xml::get_attribute_ex(props, name, Some(SOAP_1_1_ENC_NAMESPACE))
        .or_else(|| xml::get_attribute_ex(props, name, Some(SOAP_1_2_ENC_NAMESPACE)))
}

fn attr_content(a: Node) -> Option<String> {
    if xml::attr_has_value(a) {
        Some(xml::attr_value(a))
    } else {
        None
    }
}

fn to_zval_array(vm: &mut Vm, type_: &EncRef, data: Option<Node>) -> SR<Value> {
    if xml_is_null(data) {
        return Ok(Value::Null);
    }
    let d = data.unwrap();
    let props = xml::properties(d);
    let sdl = cur_sdl();
    let st = type_.sdl_type();
    let mut enc: Option<EncRef> = None;
    let mut dimension = 1usize;
    let mut dims: Option<Vec<i64>> = None;
    if let Some(c) = get_soap_enc_attribute(props, "arrayType").and_then(attr_content) {
        let (mut typ, ns) = xml::parse_namespace(&c);
        let nsptr = xml::search_ns(d, ns.as_deref());
        if let Some(p) = typ.rfind('[') {
            let end = typ[p + 1..].to_string();
            typ.truncate(p);
            dimension = calc_dimension(&end);
            dims = Some(get_position(dimension, &end));
        }
        if let Some(nsptr) = nsptr {
            enc = get_encoder(sdl.as_ref(), Some(&xml::ns_href(nsptr)), &typ);
        }
    } else if let Some(c) = get_soap_enc_attribute(props, "itemType").and_then(attr_content) {
        let (typ, ns) = xml::parse_namespace(&c);
        if let Some(nsptr) = xml::search_ns(d, ns.as_deref()) {
            enc = get_encoder(sdl.as_ref(), Some(&xml::ns_href(nsptr)), &typ);
        }
        if let Some(c2) = get_soap_enc_attribute(props, "arraySize").and_then(attr_content) {
            dimension = calc_dimension_12(vm, &c2)?;
            dims = Some(get_position_12(vm, dimension, &c2)?);
        } else {
            dims = Some(vec![0]);
        }
    } else if let Some(c2) = get_soap_enc_attribute(props, "arraySize").and_then(attr_content) {
        dimension = calc_dimension_12(vm, &c2)?;
        dims = Some(get_position_12(vm, dimension, &c2)?);
    } else if let Some(ext) = st.as_ref().and_then(|st| {
        extra_attr(
            st,
            &format!("{}:arrayType", SOAP_1_1_ENC_NAMESPACE),
            &format!("{}:arrayType", WSDL_NAMESPACE),
        )
    }) {
        let mut typ = ext.val.clone();
        if let Some(p) = typ.rfind('[') {
            typ.truncate(p);
        }
        if let Some(ns) = &ext.ns {
            enc = get_encoder(sdl.as_ref(), Some(ns), &typ);
        }
        dims = Some(vec![0]);
    } else if let Some(ext) = st.as_ref().and_then(|st| {
        extra_attr(
            st,
            &format!("{}:itemType", SOAP_1_2_ENC_NAMESPACE),
            &format!("{}:itemType", WSDL_NAMESPACE),
        )
    }) {
        if let Some(ns) = &ext.ns {
            enc = get_encoder(sdl.as_ref(), Some(ns), &ext.val);
        }
        if let Some(ext2) = st.as_ref().and_then(|st| {
            extra_attr(
                st,
                &format!("{}:arraySize", SOAP_1_2_ENC_NAMESPACE),
                &format!("{}:arraySize", WSDL_NAMESPACE),
            )
        }) {
            dimension = calc_dimension_12(vm, &ext2.val)?;
            dims = Some(get_position_12(vm, dimension, &ext2.val)?);
        } else {
            dims = Some(vec![0]);
        }
    } else if let Some(ext) = st.as_ref().and_then(|st| {
        extra_attr(
            st,
            &format!("{}:arraySize", SOAP_1_2_ENC_NAMESPACE),
            &format!("{}:arraySize", WSDL_NAMESPACE),
        )
    }) {
        dimension = calc_dimension_12(vm, &ext.val)?;
        dims = Some(get_position_12(vm, dimension, &ext.val)?);
        if let Some(e) = st
            .as_ref()
            .and_then(single_element)
            .and_then(|et| et.borrow().encode.clone())
        {
            enc = Some(e);
        }
    } else if let Some(e) = st
        .as_ref()
        .and_then(single_element)
        .and_then(|et| et.borrow().encode.clone())
    {
        enc = Some(e);
    }
    let mut dims = match dims {
        Some(d) => d,
        None => {
            dimension = 1;
            vec![0]
        }
    };
    if dims.len() < dimension {
        dims.resize(dimension, 0);
    }
    let mut pos = vec![0i64; dimension];
    if let Some(c) = get_soap_enc_attribute(props, "offset").and_then(attr_content) {
        let tmp = match c.rfind('[') {
            Some(p) => &c[p..],
            None => &c[..],
        };
        get_position_ex(dimension, tmp, &mut pos);
    }
    let ret = new_arr();
    let mut trav = xml::children(d);
    while let Some(tr) = trav {
        if xml::is_element(tr) {
            let position = get_soap_enc_attribute(xml::properties(tr), "position");
            let tmp_val = master_to_zval(vm, enc.as_ref(), tr)?;
            if let Some(c) = position.and_then(attr_content) {
                let tmp = match c.rfind('[') {
                    Some(p) => c[p..].to_string(),
                    None => c.clone(),
                };
                get_position_ex(dimension, &tmp, &mut pos);
            }
            // intermediate arrays for multidimensional arrays
            let mut ar = ret.clone();
            for p in pos.iter().take(dimension - 1) {
                let existing = ar.borrow().get_int(*p).cloned();
                let next = match existing {
                    Some(Value::Array(a2)) => a2,
                    _ => {
                        let a2 = new_arr();
                        ar.borrow_mut()
                            .set(ArrayKey::Int(*p), Value::Array(a2.clone()));
                        a2
                    }
                };
                ar = next;
            }
            ar.borrow_mut()
                .set(ArrayKey::Int(pos[dimension - 1]), tmp_val);
            // increment position
            let mut i = dimension;
            while i > 0 {
                i -= 1;
                pos[i] += 1;
                if pos[i] >= dims[i] {
                    if i > 0 {
                        pos[i] = 0;
                    }
                } else {
                    break;
                }
            }
        }
        trav = xml::next(tr);
    }
    Ok(Value::Array(ret))
}

// ---------------------------------------------------------------------------
// Apache maps

fn to_xml_map(
    vm: &mut Vm,
    type_: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let x_param = bogus(parent);
    if zval_null(data, x_param, style) {
        return Ok(x_param);
    }
    let dv = data.unwrap().deref();
    if let Value::Array(a) = &dv {
        if recursion_enter(&dv) == Some(false) {
            return Err(crate::zpp::throw(
                vm,
                b"ValueError",
                "Recursive array cannot be encoded",
            ));
        }
        let entries: Vec<(ArrayKey, Value)> = a
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let r = (|| -> SR<()> {
            let doc = xml::doc(parent);
            for (k, v) in entries {
                let item = xml::new_doc_node(doc, "item");
                xml::add_child(x_param, item);
                let key = xml::new_doc_node(doc, "key");
                xml::add_child(item, key);
                match &k {
                    ArrayKey::String(s) => {
                        if style == SOAP_ENCODED {
                            set_xsi_type(key, "xsd:string");
                        }
                        xml::node_set_content(key, &s.to_string_lossy());
                    }
                    ArrayKey::Int(n) => {
                        if style == SOAP_ENCODED {
                            set_xsi_type(key, "xsd:int");
                        }
                        xml::node_set_content(key, &n.to_string());
                    }
                }
                let e = get_conversion(vm, zval_type(&v))?;
                let xparam = master_to_xml(vm, Some(&e), Some(&v), style, item)?;
                xml::set_name(xparam, "value");
            }
            Ok(())
        })();
        recursion_leave(&dv);
        r?;
    }
    if style == SOAP_ENCODED {
        set_ns_and_type(x_param, type_);
    }
    Ok(x_param)
}

fn to_zval_map(vm: &mut Vm, _t: &EncRef, data: Option<Node>) -> SR<Value> {
    if xml_is_null(data) {
        return Ok(Value::Null);
    }
    let d = data.unwrap();
    if xml::children(d).is_none() {
        return Ok(Value::Null);
    }
    let ret = new_arr();
    let mut trav = xml::children(d);
    while let Some(t0) = trav {
        let Some(item) = xml::get_node(Some(t0), "item") else {
            break;
        };
        let Some(xml_key) = xml::get_node(xml::children(item), "key") else {
            return Err(soap_error(
                vm,
                "Encoding: Can't decode apache map, missing key",
            ));
        };
        let Some(xml_value) = xml::get_node(xml::children(item), "value") else {
            return Err(soap_error(
                vm,
                "Encoding: Can't decode apache map, missing value",
            ));
        };
        let key = master_to_zval(vm, None, xml_key)?;
        let value = master_to_zval(vm, None, xml_value)?;
        match key.deref() {
            Value::String(s) => ret.borrow_mut().set(symtable_key(s.as_bytes()), value),
            Value::Long(n) => ret.borrow_mut().set(ArrayKey::Int(n), value),
            _ => {
                return Err(soap_error(
                    vm,
                    "Encoding: Can't decode apache map, only Strings or Longs are allowed as keys",
                ));
            }
        }
        trav = xml::next(item);
    }
    Ok(Value::Array(ret))
}

// ---------------------------------------------------------------------------
// Guessing

fn guess_xml_convert(
    vm: &mut Vm,
    _t: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let enc = match data {
        Some(d) => get_conversion(vm, zval_type(d))?,
        None => get_conversion(vm, IS_NULL)?,
    };
    master_to_xml_int(vm, Some(&enc), data, style, parent, false)
}

fn guess_zval_convert(vm: &mut Vm, type_: &EncRef, data: Option<Node>) -> SR<Value> {
    let data = check_and_resolve_href(vm, data)?;
    let mut type_name: Option<String> = None;
    let enc: EncRef = match data {
        None => get_conversion(vm, IS_NULL)?,
        Some(d)
            if xml::get_attribute_ex(xml::properties(d), "nil", Some(XSI_NAMESPACE)).is_some() =>
        {
            get_conversion(vm, IS_NULL)?
        }
        Some(d) => {
            let mut enc: Option<EncRef> = None;
            if let Some(ta) = xml::get_attribute_ex(xml::properties(d), "type", Some(XSI_NAMESPACE))
            {
                let tn = xml::attr_value(ta);
                enc = get_encoder_from_prefix(cur_sdl().as_ref(), d, &tn);
                type_name = Some(tn);
                if enc.as_ref().is_some_and(|e| same_enc(e, type_)) {
                    enc = None;
                }
                if let Some(e) = &enc {
                    if derives_back(e) {
                        enc = None;
                    }
                }
            }
            match enc {
                Some(e) => e,
                None => {
                    let props = xml::properties(d);
                    if get_soap_enc_attribute(props, "arrayType").is_some()
                        || get_soap_enc_attribute(props, "itemType").is_some()
                        || get_soap_enc_attribute(props, "arraySize").is_some()
                    {
                        get_conversion(vm, SOAP_ENC_ARRAY)?
                    } else {
                        let mut e = get_conversion(vm, XSD_STRING)?;
                        let mut trav = xml::children(d);
                        while let Some(tr) = trav {
                            if xml::is_element(tr) {
                                e = get_conversion(vm, SOAP_ENC_OBJECT)?;
                                break;
                            }
                            trav = xml::next(tr);
                        }
                        e
                    }
                }
            }
        }
    };
    let ret = match data {
        Some(d) => master_to_zval_int(vm, &enc, d)?,
        None => {
            let f = enc.to_zval.get();
            f(vm, &enc, None)?
        }
    };
    if let (Some(_), Some(tn), Some(d)) = (cur_sdl(), type_name, data) {
        if enc.sdl_type().is_some() {
            let sv = new_object(vm, b"SoapVar");
            set_prop(&sv, "enc_type", Value::Long(enc.type_()));
            set_prop(&sv, "enc_value", ret);
            let (cptype, ns) = xml::parse_namespace(&tn);
            set_prop(&sv, "enc_stype", vstr(&cptype));
            if let Some(nsptr) = xml::search_ns(d, ns.as_deref()) {
                set_prop(&sv, "enc_ns", vstr(xml::ns_href(nsptr)));
            }
            return Ok(Value::Object(sv));
        }
    }
    Ok(ret)
}

// ---------------------------------------------------------------------------
// Dates

fn strftime_local(ts: i64, format: &str) -> Option<(String, i64)> {
    let t: libc::time_t = ts as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: localtime_r writes the broken-down time into `tm`.
    let r = unsafe { libc::localtime_r(&t, &mut tm) };
    if r.is_null() {
        return None;
    }
    let fmt = std::ffi::CString::new(format).ok()?;
    let mut buf = vec![0u8; 256];
    // SAFETY: the buffer is large enough for the formats used here.
    let n = unsafe {
        libc::strftime(
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            fmt.as_ptr(),
            &tm,
        )
    };
    buf.truncate(n);
    Some((String::from_utf8_lossy(&buf).into_owned(), tm.tm_gmtoff))
}

#[allow(clippy::too_many_arguments)]
fn to_xml_datetime_ex(
    vm: &mut Vm,
    type_: &EncRef,
    data: Option<&Value>,
    format: &str,
    ext_format: &str,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let x = bogus(parent);
    if zval_null(data, x, style) {
        return Ok(x);
    }
    match data.unwrap().deref() {
        Value::Long(ts) => {
            let Some((mut buf, off)) = strftime_local(ts, format) else {
                let m = format!("Encoding: Invalid timestamp {}", ts);
                return Err(soap_error(vm, m));
            };
            let tz = format!(
                "{}{:02}:{:02}",
                if off < 0 { '-' } else { '+' },
                (off / 3600).abs(),
                ((off % 3600) / 60).abs()
            );
            if tz == "+00:00" {
                buf.push('Z');
            } else {
                buf.push_str(&tz);
            }
            xml::node_set_content(x, &buf);
        }
        Value::String(s) => xml::node_set_content(x, &s.to_string_lossy()),
        Value::Object(o) => {
            if instance_of(vm, &o, "DateTimeInterface") {
                let r = call_method(vm, &o, "format", &[vstr(ext_format)])?;
                match r {
                    Value::String(s) => xml::node_set_content(x, &s.to_string_lossy()),
                    _ => return Err(soap_error(vm, "Encoding: Invalid DateTimeInterface")),
                }
            }
        }
        _ => {}
    }
    if style == SOAP_ENCODED {
        set_ns_and_type(x, type_);
    }
    Ok(x)
}

fn to_xml_duration(vm: &mut Vm, t: &EncRef, d: Option<&Value>, s: i32, p: Node) -> SR<Node> {
    to_xml_string(vm, t, d, s, p)
}

fn to_xml_datetime(vm: &mut Vm, t: &EncRef, d: Option<&Value>, s: i32, p: Node) -> SR<Node> {
    to_xml_datetime_ex(vm, t, d, "%Y-%m-%dT%H:%M:%S", "Y-m-d\\TH:i:s.up", s, p)
}

fn to_xml_time(vm: &mut Vm, t: &EncRef, d: Option<&Value>, s: i32, p: Node) -> SR<Node> {
    to_xml_datetime_ex(vm, t, d, "%H:%M:%S", "H:i:s.up", s, p)
}

fn to_xml_date(vm: &mut Vm, t: &EncRef, d: Option<&Value>, s: i32, p: Node) -> SR<Node> {
    to_xml_datetime_ex(vm, t, d, "%Y-%m-%d", "Y-m-dp", s, p)
}

fn to_xml_gyearmonth(vm: &mut Vm, t: &EncRef, d: Option<&Value>, s: i32, p: Node) -> SR<Node> {
    to_xml_datetime_ex(vm, t, d, "%Y-%m", "Y-mp", s, p)
}

fn to_xml_gyear(vm: &mut Vm, t: &EncRef, d: Option<&Value>, s: i32, p: Node) -> SR<Node> {
    to_xml_datetime_ex(vm, t, d, "%Y", "Yp", s, p)
}

fn to_xml_gmonthday(vm: &mut Vm, t: &EncRef, d: Option<&Value>, s: i32, p: Node) -> SR<Node> {
    to_xml_datetime_ex(vm, t, d, "--%m-%d", "--m-dp", s, p)
}

fn to_xml_gday(vm: &mut Vm, t: &EncRef, d: Option<&Value>, s: i32, p: Node) -> SR<Node> {
    to_xml_datetime_ex(vm, t, d, "---%d", "---dp", s, p)
}

fn to_xml_gmonth(vm: &mut Vm, t: &EncRef, d: Option<&Value>, s: i32, p: Node) -> SR<Node> {
    to_xml_datetime_ex(vm, t, d, "--%m--", "--m--p", s, p)
}

// ---------------------------------------------------------------------------
// Lists and unions

fn to_zval_list(vm: &mut Vm, enc: &EncRef, data: Option<Node>) -> SR<Value> {
    to_zval_stringc(vm, enc, data)
}

fn to_xml_list(
    vm: &mut Vm,
    enc: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let mut list_enc: Option<EncRef> = None;
    if let Some(st) = enc.sdl_type() {
        let s = st.borrow();
        if s.kind == XSD_TYPEKIND_LIST {
            if let Some(els) = &s.elements {
                if let Some(t) = els.first() {
                    list_enc = t.borrow().encode.clone();
                }
            }
        }
    }
    let ret = bogus(parent);
    if zval_null(data, ret, style) {
        return Ok(ret);
    }
    let old = take_ref_map();
    let r = (|| -> SR<()> {
        let dv = data.unwrap().deref();
        let mut list = String::new();
        let items: Vec<Value> = match &dv {
            Value::Array(a) => a.borrow().values().cloned().collect(),
            other => {
                let s = match other {
                    Value::String(s) => s.as_bytes().to_vec(),
                    o => serialization_string(vm, o)?,
                };
                let s = whitespace_collapse(&String::from_utf8_lossy(&s));
                s.split(' ').filter(|p| !p.is_empty()).map(vstr).collect()
            }
        };
        for item in items {
            let dummy = master_to_xml(vm, list_enc.as_ref(), Some(&item), SOAP_LITERAL, ret)?;
            if xml::first_child_has_content(dummy) {
                let c = xml::children(dummy).map(xml::content).unwrap_or_default();
                if !list.is_empty() {
                    list.push(' ');
                }
                list.push_str(&c);
            } else {
                return Err(soap_error(vm, "Encoding: Violation of encoding rules"));
            }
            xml::unlink(dummy);
        }
        xml::node_set_content(ret, &list);
        Ok(())
    })();
    restore_ref_map(old);
    r?;
    Ok(ret)
}

fn to_xml_list1(
    vm: &mut Vm,
    enc: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    to_xml_list(vm, enc, data, style, parent)
}

fn to_zval_union(vm: &mut Vm, enc: &EncRef, data: Option<Node>) -> SR<Value> {
    to_zval_list(vm, enc, data)
}

fn to_xml_union(
    vm: &mut Vm,
    enc: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    to_xml_list(vm, enc, data, style, parent)
}

// ---------------------------------------------------------------------------
// <anyXML>

fn to_zval_any(vm: &mut Vm, _t: &EncRef, data: Option<Node>) -> SR<Value> {
    let Some(d) = data else {
        return Ok(Value::Null);
    };
    if let Some(sdl) = cur_sdl() {
        let name = xml::name(d);
        let mut key = String::new();
        if let Some(h) = xml::ns_href_of(d) {
            key.push_str(&h);
            key.push(':');
        }
        key.push_str(&name);
        let t = sdl
            .borrow()
            .elements
            .as_ref()
            .and_then(|e| e.find(&key).cloned());
        if let Some(t) = t {
            let enc = t.borrow().encode.clone();
            if let Some(enc) = enc {
                return master_to_zval_int(vm, &enc, d);
            }
        }
    }
    Ok(vstr(xml::node_dump(d)))
}

fn to_xml_any(
    vm: &mut Vm,
    _t: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let dv = data.map(|d| d.deref()).unwrap_or(Value::Null);
    if let Value::Array(a) = &dv {
        let enc = get_conversion(vm, XSD_ANYXML)?;
        let entries: Vec<(ArrayKey, Value)> = a
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut ret = parent;
        for (k, el) in entries {
            ret = master_to_xml(vm, Some(&enc), Some(&el), style, parent)?;
            if xml::name(ret) != "textnoenc" {
                if let ArrayKey::String(s) = &k {
                    xml::set_name(ret, &s.to_string_lossy());
                } else {
                    xml::set_name(ret, "");
                }
            }
        }
        return Ok(ret);
    }
    let s = serialization_string(vm, &dv)?;
    Ok(xml::add_text_noenc(parent, &String::from_utf8_lossy(&s)))
}

// ---------------------------------------------------------------------------
// SDL-typed values

pub fn sdl_guess_convert_zval(vm: &mut Vm, enc: &EncRef, data: Option<Node>) -> SR<Value> {
    let Some(st) = enc.sdl_type() else {
        return guess_zval_convert(vm, enc, data);
    };
    let (kind, tenc) = {
        let s = st.borrow();
        (s.kind, s.encode.clone())
    };
    match kind {
        XSD_TYPEKIND_SIMPLE => match tenc {
            Some(te) if !same_enc(&te, enc) => match data {
                Some(d) => master_to_zval_int(vm, &te, d),
                None => {
                    let f = te.to_zval.get();
                    f(vm, &te, None)
                }
            },
            _ => guess_zval_convert(vm, enc, data),
        },
        XSD_TYPEKIND_LIST => to_zval_list(vm, enc, data),
        XSD_TYPEKIND_UNION => to_zval_union(vm, enc, data),
        XSD_TYPEKIND_COMPLEX | XSD_TYPEKIND_RESTRICTION | XSD_TYPEKIND_EXTENSION => {
            if tenc
                .as_ref()
                .is_some_and(|e| e.type_() == IS_ARRAY || e.type_() == SOAP_ENC_ARRAY)
            {
                return to_zval_array(vm, enc, data);
            }
            to_zval_object(vm, enc, data)
        }
        _ => Err(soap_error(vm, "Encoding: Internal Error")),
    }
}

pub fn sdl_guess_convert_xml(
    vm: &mut Vm,
    enc: &EncRef,
    data: Option<&Value>,
    style: i32,
    parent: Node,
) -> SR<Node> {
    let Some(st) = enc.sdl_type() else {
        let ret = guess_xml_convert(vm, enc, data, style, parent)?;
        if style == SOAP_ENCODED {
            set_ns_and_type(ret, enc);
        }
        return Ok(ret);
    };
    let (kind, tenc) = {
        let s = st.borrow();
        (s.kind, s.encode.clone())
    };
    let ret = match kind {
        XSD_TYPEKIND_SIMPLE => match tenc {
            Some(te) if !same_enc(&te, enc) => master_to_xml(vm, Some(&te), data, style, parent)?,
            _ => guess_xml_convert(vm, enc, data, style, parent)?,
        },
        XSD_TYPEKIND_LIST => to_xml_list(vm, enc, data, style, parent)?,
        XSD_TYPEKIND_UNION => to_xml_union(vm, enc, data, style, parent)?,
        XSD_TYPEKIND_COMPLEX | XSD_TYPEKIND_RESTRICTION | XSD_TYPEKIND_EXTENSION => {
            if tenc
                .as_ref()
                .is_some_and(|e| e.type_() == IS_ARRAY || e.type_() == SOAP_ENC_ARRAY)
            {
                return to_xml_array(vm, enc, data, style, parent);
            }
            return to_xml_object(vm, enc, data, style, parent);
        }
        _ => return Err(soap_error(vm, "Encoding: Internal Error")),
    };
    if style == SOAP_ENCODED {
        set_ns_and_type(ret, enc);
    }
    Ok(ret)
}

/// check_and_resolve_href()
pub fn check_and_resolve_href(vm: &mut Vm, data: Option<Node>) -> SR<Option<Node>> {
    let Some(d) = data else { return Ok(None) };
    let props = xml::properties(d);
    if props.is_none() {
        return Ok(Some(d));
    }
    if let Some(href) = xml::get_attribute(props, "href") {
        let h = xml::attr_value(href);
        if let Some(id) = h.strip_prefix('#') {
            let root = xml::doc(d).and_then(xml::children);
            let ret = xml::get_node_with_attribute_recursive_ex(root, None, None, "id", id, None);
            return match ret {
                Some(r) => Ok(Some(r)),
                None => Err(soap_error(
                    vm,
                    format!("Encoding: Unresolved reference '{}'", h),
                )),
            };
        }
        return Err(soap_error(
            vm,
            format!("Encoding: External reference '{}'", h),
        ));
    }
    if let Some(href) = xml::get_attribute_ex(props, "ref", Some(SOAP_1_2_ENC_NAMESPACE)) {
        let h = xml::attr_value(href);
        let id = h.strip_prefix('#').unwrap_or(&h).to_string();
        let root = xml::doc(d).and_then(xml::children);
        let ret = xml::get_node_with_attribute_recursive_ex(
            root,
            None,
            None,
            "id",
            &id,
            Some(SOAP_1_2_ENC_NAMESPACE),
        );
        return match ret {
            None => Err(soap_error(
                vm,
                format!("Encoding: Unresolved reference '{}'", h),
            )),
            Some(r) if r == d => Err(soap_error(
                vm,
                format!(
                    "Encoding: Violation of id and ref information items '{}'",
                    h
                ),
            )),
            Some(r) => Ok(Some(r)),
        };
    }
    Ok(Some(d))
}

// ---------------------------------------------------------------------------
// Namespaces and types

pub fn set_ns_and_type(node: Node, enc: &EncRef) {
    let (ns, ts) = {
        let d = enc.d.borrow();
        (d.ns.clone(), d.type_str.clone())
    };
    set_ns_and_type_ex(node, ns.as_deref(), ts.as_deref().unwrap_or(""));
}

pub fn set_ns_and_type_ex(node: Node, ns: Option<&str>, type_: &str) {
    let mut s = String::new();
    get_type_str(node, ns, type_, &mut s);
    set_xsi_type(node, &s);
}

fn search_ns_prefix_by_href(node: Node, href: &str) -> Option<usize> {
    xml::t(|t| {
        let orig = node;
        let mut cur = Some(node);
        while let Some(n) = cur {
            let ty = t.nodes[n].t;
            if matches!(ty, NType::EntityRef | NType::Entity | NType::EntityDecl) {
                return None;
            }
            if ty == NType::Element {
                for &ns in &t.nodes[n].ns_def.clone() {
                    let (p, h) = (t.nss[ns].prefix.clone(), t.nss[ns].href.clone());
                    if p.is_some() && h == href && t.search_ns(orig, p.as_deref()) == Some(ns) {
                        return Some(ns);
                    }
                }
                if orig != n {
                    if let Some(ns) = t.nodes[n].ns {
                        let (p, h) = (t.nss[ns].prefix.clone(), t.nss[ns].href.clone());
                        if p.is_some() && h == href && t.search_ns(orig, p.as_deref()) == Some(ns) {
                            return Some(ns);
                        }
                    }
                }
            }
            cur = t.nodes[n].parent;
        }
        None
    })
}

/// encode_add_ns(): the namespace declaration for `ns` in scope of `node`,
/// declared on the root element when missing.
pub fn encode_add_ns(node: Node, ns: Option<&str>) -> Option<usize> {
    let ns = ns?;
    let mut xmlns = xml::search_ns_by_href(node, ns);
    if let Some(x) = xmlns {
        if xml::ns_prefix(x).is_none() {
            xmlns = search_ns_prefix_by_href(node, ns);
        }
    }
    if xmlns.is_some() {
        return xmlns;
    }
    let root = xml::doc(node).and_then(xml::children).unwrap_or(node);
    if let Some(prefix) = def_ns_prefix(ns) {
        return xml::new_ns(root, ns, Some(prefix));
    }
    let mut num = g(|g| {
        g.cur_uniq_ns += 1;
        g.cur_uniq_ns
    });
    let prefix = loop {
        let p = format!("ns{}", num);
        if xml::search_ns(node, Some(&p)).is_none() {
            break p;
        }
        num = g(|g| {
            g.cur_uniq_ns += 1;
            g.cur_uniq_ns
        });
    };
    xml::new_ns(root, ns, Some(&prefix))
}

pub fn set_ns_prop(node: Node, ns: &str, name: &str, val: &str) {
    let nsp = encode_add_ns(node, Some(ns));
    xml::set_ns_prop(node, nsp, name, val);
}

pub fn set_xsi_nil(node: Node) {
    set_ns_prop(node, XSI_NAMESPACE, "nil", "true");
}

pub fn set_xsi_type(node: Node, type_: &str) {
    set_ns_prop(node, XSI_NAMESPACE, "type", type_);
}

/// encode_reset_ns()
pub fn encode_reset_ns() {
    g(|g| {
        g.cur_uniq_ns = 0;
        g.cur_uniq_ref = 0;
        g.ref_map = Some(RefMap::default());
    })
}

/// encode_finish()
pub fn encode_finish() {
    g(|g| {
        g.cur_uniq_ns = 0;
        g.cur_uniq_ref = 0;
        g.ref_map = None;
    })
}

fn get_array_type(
    vm: &mut Vm,
    node: Node,
    array: &Value,
    type_: &mut String,
) -> SR<Option<EncRef>> {
    let Value::Array(a) = array.deref() else {
        type_.push_str("xsd:anyType");
        return Ok(Some(get_conversion(vm, XSD_ANYTYPE)?));
    };
    let items: Vec<Value> = a.borrow().values().map(|v| v.deref()).collect();
    let mut i = 0;
    let mut different = false;
    let (mut cur_type, mut prev_type) = (0i64, 0i64);
    let (mut cur_stype, mut prev_stype): (Option<String>, Option<String>) = (None, None);
    let (mut cur_ns, mut prev_ns): (Option<String>, Option<String>) = (None, None);
    for tmp in &items {
        if let Some(var) = is_soap_var(tmp) {
            let Value::Long(t) = prop(&var, "enc_type") else {
                return Err(soap_error(
                    vm,
                    "Encoding: SoapVar has no 'enc_type' property",
                ));
            };
            cur_type = t;
            if cur_type == UNKNOWN_TYPE {
                cur_type = zval_type(&prop(&var, "enc_value"));
            }
            cur_stype = as_str(&prop(&var, "enc_stype"));
            cur_ns = as_str(&prop(&var, "enc_ns"));
        } else if matches!(tmp, Value::Array(x) if !is_list(&x.borrow())) {
            cur_type = APACHE_MAP;
            cur_stype = None;
            cur_ns = None;
        } else {
            cur_type = zval_type(tmp);
            cur_stype = None;
            cur_ns = None;
        }
        if i > 0
            && (cur_type != prev_type
                || (cur_stype.is_some() && prev_stype.is_some() && cur_stype != prev_stype)
                || (cur_stype.is_none() && prev_stype.is_some())
                || (cur_ns.is_some() && prev_ns.is_some() && cur_ns != prev_ns)
                || (cur_ns.is_none() && prev_ns.is_some()))
        {
            different = true;
            break;
        }
        prev_type = cur_type;
        prev_stype = cur_stype.clone();
        prev_ns = cur_ns.clone();
        i += 1;
    }
    if different || i == 0 {
        type_.push_str("xsd:anyType");
        return Ok(Some(get_conversion(vm, XSD_ANYTYPE)?));
    }
    if let Some(st) = cur_stype {
        let mut array_type = String::new();
        if let Some(ns) = &cur_ns {
            let x = encode_add_ns(node, Some(ns));
            type_.push_str(&x.and_then(xml::ns_prefix).unwrap_or_default());
            type_.push(':');
            array_type.push_str(ns);
            array_type.push(':');
        }
        type_.push_str(&st);
        array_type.push_str(&st);
        return Ok(get_encoder_ex(cur_sdl().as_ref(), &array_type));
    }
    let enc = get_conversion(vm, cur_type)?;
    let (ns, ts) = (enc.ns(), enc.type_str());
    get_type_str(node, ns.as_deref(), ts.as_deref().unwrap_or(""), type_);
    Ok(Some(enc))
}

/// get_type_str(): "prefix:type" with the prefix declared as needed.
pub fn get_type_str(node: Node, ns: Option<&str>, type_: &str, ret: &mut String) {
    if let Some(ns) = ns {
        let v = soap_version();
        let ns = if v == SOAP_1_2 && ns == SOAP_1_1_ENC_NAMESPACE {
            SOAP_1_2_ENC_NAMESPACE
        } else if v == SOAP_1_1 && ns == SOAP_1_2_ENC_NAMESPACE {
            SOAP_1_1_ENC_NAMESPACE
        } else {
            ns
        };
        let x = encode_add_ns(node, Some(ns));
        ret.push_str(&x.and_then(xml::ns_prefix).unwrap_or_default());
        ret.push(':');
    }
    ret.push_str(type_);
}

/// A fresh encoder of the default table (copied by typemaps).
pub fn new_encoder_from(base: &EncRef) -> EncDetails {
    base.d.borrow().clone()
}

pub fn same_fn_xml(a: ToXml, b: ToXml) -> bool {
    a as usize == b as usize
}

pub fn to_xml_user_fn() -> ToXml {
    to_xml_user
}

pub fn to_zval_user_fn() -> ToZval {
    to_zval_user
}

pub fn sdl_guess_fns() -> (ToXml, ToZval) {
    (sdl_guess_convert_xml, sdl_guess_convert_zval)
}

/// The value of a PhpString as a UTF-8 Rust string (lossy).
pub fn pstr(s: &PhpString) -> String {
    s.to_string_lossy()
}
