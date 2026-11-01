//! Encoding PHP values as SOAP XML (SOAP 1.1 section 5 and SOAP 1.2 part
//! 2 encoding, or literal XML Schema instances): the built-in datatypes,
//! SOAP-ENC arrays and structs, Apache maps, the WSDL's types, `SoapVar`,
//! typemaps and multi-reference values.
//!
//! The details PHP's output shows — the order namespaces get declared on
//! the envelope, which elements get `xsi:type`, where `id`/`href` go, how
//! floats and dates are written — follow the ext/soap tests' expected
//! output and observations on PHP 8.5.

use std::rc::Rc;

use goro_runtime::{Key, KeyRef, ObjRef, PhpArray, PhpStr, Value};

use super::sdl::{Deriv, Enc, Kind, MKind, Model, SType, Sdl, TId, TRef};
use super::types::{self, Bi, Conv};
use super::xmlout::{El, Kid, NS_APACHE, NS_SOAP11_ENC, NS_SOAP12_ENC, NS_XSD, NS_XSI, OutDoc};
use super::{SoapState, soap_error};
use crate::vm::{R, Vm};

/// A typemap entry (PHP manual: SoapClient::__construct, `typemap`).
#[derive(Clone, Debug)]
pub struct TypeMapEntry {
    pub ns: String,
    pub name: String,
    pub to_xml: Option<Value>,
    pub from_xml: Option<Value>,
}

/// Per-message encoding state.
pub struct Cx {
    pub sdl: Option<Rc<Sdl>>,
    pub doc: OutDoc,
    pub version: i64,
    pub features: i64,
    /// The `encoding` option (strings are converted from it to UTF-8).
    pub encoding: Option<String>,
    /// `classmap`: (type name, class name).
    pub classmap: Vec<(String, String)>,
    pub typemap: Rc<Vec<TypeMapEntry>>,
    /// Objects already written in this message: (address, element, id).
    pub refs: Vec<(usize, El, Option<String>)>,
    pub ref_n: u32,
    /// Encoding a value nested in itself (recursion guard).
    pub depth: u32,
    /// Inside a repeating model group (its elements repeat too).
    pub rep: u32,
    /// Arrays being written (their data addresses), to refuse recursion.
    pub arrays: Vec<usize>,
    /// Write a Header element even without headers (an array of
    /// headers was given; observed on PHP 8.5; test bugs/bug70388.phpt).
    pub force_header: bool,
}

pub const SOAP_SINGLE_ELEMENT_ARRAYS: i64 = 1;
pub const SOAP_USE_XSI_ARRAY_TYPE: i64 = 4;

impl Cx {
    pub fn new(sdl: Option<Rc<Sdl>>, version: i64, st: &SoapState) -> Cx {
        Cx {
            sdl,
            doc: OutDoc::new(version),
            version,
            features: st.features,
            encoding: st.encoding.clone(),
            classmap: st.classmap.clone(),
            typemap: st.typemap.clone(),
            refs: Vec::new(),
            ref_n: 0,
            depth: 0,
            rep: 0,
            force_header: false,
            arrays: Vec::new(),
        }
    }

    pub fn enc_ns(&self) -> &'static str {
        if self.version == 2 {
            NS_SOAP12_ENC
        } else {
            NS_SOAP11_ENC
        }
    }

    /// Sets `xsi:type` (the type's namespace is declared first).
    pub fn set_type(&mut self, el: El, ns: &str, name: &str) {
        let v = self.doc.qname(Some(el), ns, name);
        let a = self.doc.qname(Some(el), NS_XSI, "type");
        self.doc.set_attr(el, &a, &v);
    }

    pub fn set_nil(&mut self, el: El) {
        let a = self.doc.qname(Some(el), NS_XSI, "nil");
        self.doc.set_attr(el, &a, "true");
    }
}

pub fn class_is(vm: &mut Vm, o: &ObjRef, name: &str) -> bool {
    let cls = vm.class_of(o);
    let mut c = Some(cls);
    while let Some(k) = c {
        if k.name.as_bytes().eq_ignore_ascii_case(name.as_bytes()) {
            return true;
        }
        c = k.parent.clone();
    }
    false
}

/// `instanceof` an interface or class by name.
pub fn instance_of(vm: &mut Vm, o: &ObjRef, name: &str) -> bool {
    let Ok(Some(target)) = vm.find_class(name.as_bytes(), false) else {
        return false;
    };
    let cls = vm.class_of(o);
    Rc::ptr_eq(&cls, &target) || cls.is_subclass_of(&target)
}

fn is_list(a: &PhpArray) -> bool {
    let mut i = 0i64;
    for (k, _) in a.iter() {
        match k {
            KeyRef::Int(n) if n == i => i += 1,
            _ => return false,
        }
    }
    true
}

/// The UTF-8 text of a string, converted from the `encoding` option or
/// checked ("not a valid utf-8 string", observed on PHP 8.5; test
/// bugs/bug42488.phpt).
pub fn to_utf8(vm: &mut Vm, cx: &Cx, s: &[u8]) -> R<String> {
    if let Some(enc) = &cx.encoding {
        return Ok(super::charset_to_utf8(enc, s));
    }
    match std::str::from_utf8(s) {
        Ok(t) => Ok(t.to_string()),
        Err(e) => {
            let good = e.valid_up_to();
            let mut shown = String::from_utf8_lossy(&s[..good]).into_owned();
            if let Some(b) = s.get(good) {
                shown.push_str(&format!("\\x{:02X}", b));
            }
            Err(soap_error(
                vm,
                &format!("Encoding: string '{shown}...' is not a valid utf-8 string"),
            ))
        }
    }
}

pub fn value_to_bytes(vm: &mut Vm, v: &Value) -> R<Vec<u8>> {
    match v {
        Value::Str(s) => Ok(s.as_bytes().to_vec()),
        Value::Float(f) => Ok(fmt_double(vm, *f).into_bytes()),
        _ => Ok(vm.to_php_str(v)?.as_bytes().to_vec()),
    }
}

/// A float as PHP's SOAP writes it: `precision` significant digits,
/// "INF", "-INF", "NAN" (observed on PHP 8.5).
pub fn fmt_double(vm: &Vm, f: f64) -> String {
    if f.is_nan() {
        return "NAN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "INF".into() } else { "-INF".into() };
    }
    let p = vm.ini.precision;
    let mut out = Vec::new();
    goro_runtime::float_fmt::format_php_repr(f, if p == 0 { 1 } else { p }, b'E', &mut out);
    String::from_utf8(out).unwrap_or_default()
}

/// The (namespace, name) a value's guessed encoding has.
fn guess_qname(vm: &mut Vm, cx: &Cx, v: &Value) -> Option<(String, String)> {
    let v = v.deref();
    match &v {
        Value::Null => None,
        Value::Bool(_) => Some((NS_XSD.into(), "boolean".into())),
        Value::Int(_) => Some((NS_XSD.into(), "int".into())),
        Value::Float(_) => Some((NS_XSD.into(), "float".into())),
        Value::Str(_) => Some((NS_XSD.into(), "string".into())),
        Value::Array(a) => {
            if is_list(a) {
                Some((cx.enc_ns().into(), "Array".into()))
            } else {
                Some((NS_APACHE.into(), "Map".into()))
            }
        }
        Value::Object(o) => {
            if class_is(vm, o, "SoapVar") {
                let stype = vm.get_prop_raw(o, b"enc_stype");
                let sns = vm.get_prop_raw(o, b"enc_ns");
                if let Value::Str(t) = stype {
                    let ns = match sns {
                        Value::Str(n) => n.to_string_lossy(),
                        _ => String::new(),
                    };
                    return Some((ns, t.to_string_lossy()));
                }
                let code = match vm.get_prop_raw(o, b"enc_type") {
                    Value::Int(n) => n,
                    _ => types::UNKNOWN_TYPE,
                };
                if code == types::UNKNOWN_TYPE {
                    let val = vm.get_prop_raw(o, b"enc_value");
                    return guess_qname(vm, cx, &val);
                }
                let b = types::by_code(code)?;
                let ns = if b.ns == NS_SOAP11_ENC {
                    cx.enc_ns()
                } else {
                    b.ns
                };
                return Some((ns.into(), b.name.into()));
            }
            if let Some((ns, n)) = classmap_type(vm, cx, o) {
                return Some((ns, n));
            }
            Some((cx.enc_ns().into(), "Struct".into()))
        }
        _ => None,
    }
}

/// The WSDL type a classmap gives an object's class.
fn classmap_type(vm: &mut Vm, cx: &Cx, o: &ObjRef) -> Option<(String, String)> {
    if cx.classmap.is_empty() {
        return None;
    }
    let sdl = cx.sdl.clone()?;
    let cname = vm.class_of(o).name.to_string_lossy();
    for (t, c) in &cx.classmap {
        if c.eq_ignore_ascii_case(&cname) {
            // Find the type by name in the WSDL.
            for (k, &id) in &sdl.named {
                if &k.1 == t && sdl.types[id].kind == Kind::Complex {
                    return Some((k.0.clone(), k.1.clone()));
                }
            }
        }
    }
    None
}

/// Encodes `v` into element `el` with encoding `enc` (`None`: guessed
/// from the value). `literal` is the body's use.
pub fn to_xml(
    vm: &mut Vm,
    cx: &mut Cx,
    enc: Option<Enc>,
    v: &Value,
    el: El,
    literal: bool,
) -> R<()> {
    let v = v.deref();
    if cx.depth > 64 {
        return Err(soap_error(vm, "Encoding: recursion detected"));
    }
    if let Value::Object(o) = &v
        && class_is(vm, o, "SoapVar")
    {
        return soapvar_to_xml(vm, cx, o, el, literal);
    }
    // A user typemap's to_xml.
    if let Some((ns, name)) = enc_qname(cx, enc)
        && apply_typemap(vm, cx, &ns, &name, &v, el, literal)?
    {
        return Ok(());
    }
    if let Value::Null = v {
        if literal {
            // Literal: an empty element, xsi:nil for nillable elements.
            if let Some(Enc::Type(t)) = enc
                && let Some(sdl) = cx.sdl.clone()
                && sdl.types[t].kind == Kind::Element
                && sdl.types[sdl.deref_element(t)].nillable
            {
                cx.set_nil(el);
            }
            return Ok(());
        }
        cx.set_nil(el);
        // A null for SOAP-ENC Array/Struct or a map still gets the type
        // (observed on PHP 8.5; test interop/Round2/Base/r2_base_009s.phpt).
        if !literal
            && let Some(Enc::Builtin(b)) = enc
            && matches!(b.conv, Conv::Array | Conv::Struct | Conv::Map)
        {
            let ns = if b.ns == NS_SOAP11_ENC {
                cx.enc_ns()
            } else {
                b.ns
            };
            cx.set_type(el, ns, b.name);
        }
        if !literal
            && let Some(Enc::Type(t)) = enc
            && let Some(sdl) = cx.sdl.clone()
        {
            let ty = &sdl.types[t];
            if ty.kind == Kind::Complex {
                let (ns, n) = (ty.ns.clone(), ty.name.clone());
                cx.set_type(el, &ns, &n);
            } else if ty.kind == Kind::Element
                && let Some(Enc::Type(b)) = ty.base.as_ref().and_then(|b| sdl.resolve(b))
                && sdl.types[b].kind == Kind::Complex
            {
                let (ns, n) = (sdl.types[b].ns.clone(), sdl.types[b].name.clone());
                cx.set_type(el, &ns, &n);
            }
        }
        return Ok(());
    }
    // Multi-reference values (SOAP 1.1 section 5.4.1, `id`/`href`), for
    // values written as structs.
    if !literal
        && let Value::Object(o) = &v
        && struct_like(cx, enc)
    {
        let addr = o.obj() as *const _ as usize;
        if let Some(i) = cx.refs.iter().position(|r| r.0 == addr) {
            let target = cx.refs[i].1;
            let id = match &cx.refs[i].2 {
                Some(id) => id.clone(),
                None => {
                    cx.ref_n += 1;
                    let id = format!("ref{}", cx.ref_n);
                    if cx.version == 2 {
                        let a = cx.doc.qname(Some(target), NS_SOAP12_ENC, "id");
                        cx.doc.set_attr(target, &a, &id);
                    } else {
                        cx.doc.set_attr(target, "id", &id);
                    }
                    cx.refs[i].2 = Some(id.clone());
                    id
                }
            };
            if cx.version == 2 {
                let a = cx.doc.qname(Some(el), NS_SOAP12_ENC, "ref");
                cx.doc.set_attr(el, &a, &format!("#{id}"));
            } else {
                cx.doc.set_attr(el, "href", &format!("#{id}"));
            }
            return Ok(());
        }
        cx.refs.push((addr, el, None));
    }
    // An object whose class the classmap gives a (derived) WSDL type is
    // written as that type (test classmap003.phpt).
    let mut enc = enc;
    let mut switched = None;
    if let Value::Object(o) = &v
        && enc.is_some()
        && let Some(sdl) = cx.sdl.clone()
        && let Some((ns, n)) = classmap_type(vm, cx, o)
        && let Some(Enc::Type(t2)) = sdl.find_type(&ns, &n)
        && !matches!(enc, Some(Enc::Type(t)) if t == t2 || elem_type(&sdl, t) == Some(t2))
        && sdl.types[t2].kind == Kind::Complex
    {
        enc = Some(Enc::Type(t2));
        switched = Some((ns, n));
    }
    // A recursive array (observed on PHP 8.5; test gh16318.phpt).
    if cx.depth == 0
        && let Value::Array(a) = &v
    {
        let mut stack = Vec::new();
        if array_recursive(a, &mut stack) {
            return vm.throw("ValueError", "Recursive array cannot be encoded");
        }
    }
    cx.depth += 1;
    let r = match enc {
        None => guess_to_xml(vm, cx, &v, el, literal),
        Some(Enc::Builtin(b)) => builtin_to_xml(vm, cx, b, &v, el, literal),
        Some(Enc::Type(t)) => type_to_xml(vm, cx, t, &v, el, literal),
    };
    cx.depth -= 1;
    if cx.depth == 0
        && let Some(e) = PENDING.with(|p| p.borrow_mut().take())
    {
        return Err(crate::vm::Unwind::Throw(e));
    }
    // A literal message names the substituted type (observed on PHP 8.5;
    // test bugs/gh18640.phpt).
    if r.is_ok()
        && literal
        && let Some((ns, n)) = switched
    {
        cx.set_type(el, &ns, &n);
    }
    r
}

thread_local! {
    static PENDING: std::cell::RefCell<Option<ObjRef>> = const { std::cell::RefCell::new(None) };
}

fn pend_error(vm: &mut Vm, msg: &str) -> R<()> {
    let e = vm.create_exception("ValueError", msg, 0)?;
    if let Some(prev) = PENDING.with(|p| p.borrow_mut().take()) {
        vm.set_prop_raw(&e, b"previous", Value::Object(prev));
    }
    PENDING.with(|p| *p.borrow_mut() = Some(e));
    Ok(())
}

/// Whether an array contains itself (through references).
fn array_recursive(a: &PhpArray, stack: &mut Vec<usize>) -> bool {
    if a.is_empty() || stack.len() > 64 {
        return false;
    }
    let addr = a.data_addr();
    if stack.contains(&addr) {
        return true;
    }
    stack.push(addr);
    for (_, v) in a.iter() {
        if let Value::Array(sub) = v.deref()
            && array_recursive(&sub, stack)
        {
            return true;
        }
    }
    stack.pop();
    false
}

/// The type an element declaration has.
fn elem_type(sdl: &Sdl, t: TId) -> Option<TId> {
    if sdl.types[t].kind != Kind::Element {
        return None;
    }
    match sdl.types[sdl.deref_element(t)]
        .base
        .as_ref()
        .and_then(|b| sdl.resolve(b))
    {
        Some(Enc::Type(b)) => Some(b),
        _ => None,
    }
}

/// Whether an encoding writes a struct (objects shared by reference).
fn struct_like(cx: &Cx, enc: Option<Enc>) -> bool {
    match enc {
        None => true,
        Some(Enc::Builtin(b)) => matches!(b.conv, Conv::Struct | Conv::AnyType | Conv::Map),
        Some(Enc::Type(t)) => match &cx.sdl {
            Some(sdl) => {
                let mut id = t;
                for _ in 0..16 {
                    let ty = &sdl.types[id];
                    match ty.kind {
                        Kind::Complex => return true,
                        Kind::Element => match ty.base.as_ref().and_then(|b| sdl.resolve(b)) {
                            Some(Enc::Type(b)) => id = b,
                            Some(Enc::Builtin(b)) => {
                                return matches!(b.conv, Conv::Struct | Conv::AnyType | Conv::Map);
                            }
                            None => return true,
                        },
                        _ => return false,
                    }
                }
                false
            }
            None => false,
        },
    }
}

fn enc_qname(cx: &Cx, enc: Option<Enc>) -> Option<(String, String)> {
    if cx.typemap.is_empty() {
        return None;
    }
    match enc? {
        Enc::Builtin(b) => Some((b.ns.to_string(), b.name.to_string())),
        Enc::Type(t) => {
            let sdl = cx.sdl.as_ref()?;
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

/// A typemap's to_xml for type (ns, name): its markup replaces `el`; in
/// encoded messages the root gets xsi:type, with the xsi prefix the
/// markup declares (observed on PHP 8.5; tests typemap002.phpt,
/// typemap013.phpt).
#[allow(clippy::too_many_arguments)]
fn apply_typemap(
    vm: &mut Vm,
    cx: &mut Cx,
    ns: &str,
    name: &str,
    v: &Value,
    el: El,
    literal: bool,
) -> R<bool> {
    let Some(f) = cx
        .typemap
        .iter()
        .find(|t| t.ns == ns && t.name == name)
        .and_then(|t| t.to_xml.clone())
    else {
        return Ok(false);
    };
    let r = vm.call_value(&f, vec![v.clone()])?;
    let s = vm.to_php_str(&r)?.to_string_lossy();
    let s = s.trim();
    let mut s = match s.strip_prefix("<?xml") {
        Some(rest) => match rest.find("?>") {
            Some(i) => rest[i + 2..].trim_start().to_string(),
            None => s.to_string(),
        },
        None => s.to_string(),
    };
    if !literal && let Some(end) = root_tag_end(&s) {
        let tag = &s[..end];
        let decl = format!("=\"{NS_XSI}\"");
        let prefix = tag.find(&decl).and_then(|i| {
            let before = &tag[..i];
            before.rfind("xmlns:").map(|j| before[j + 6..].to_string())
        });
        let prefix = match prefix {
            Some(p) => p,
            None => cx.doc.ensure_ns(Some(el), NS_XSI),
        };
        let t = cx.doc.qname(Some(el), ns, name);
        let at = if s[..end].ends_with('/') {
            end - 1
        } else {
            end
        };
        s.insert_str(at, &format!(" {prefix}:type=\"{t}\""));
    }
    replace_raw(cx, el, &s);
    Ok(true)
}

/// The offset of the `>` ending the root start tag.
fn root_tag_end(s: &str) -> Option<usize> {
    let mut q: Option<char> = None;
    for (i, c) in s.char_indices().skip(1) {
        match q {
            Some(x) if c == x => q = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' => q = Some(c),
                '>' => return Some(i),
                _ => {}
            },
        }
    }
    None
}

/// Replaces element `el` in its parent with raw markup.
pub fn replace_raw(cx: &mut Cx, el: El, raw: &str) {
    if let Some(p) = cx.doc.els[el].parent {
        for k in cx.doc.els[p].kids.iter_mut() {
            if matches!(k, Kid::El(e) if *e == el) {
                *k = Kid::Raw(raw.to_string());
                return;
            }
        }
    }
    cx.doc.els[el].kids = vec![Kid::Raw(raw.to_string())];
}

/// The guessed encoding of a value (the PHP type; `anyType`).
fn guess_to_xml(vm: &mut Vm, cx: &mut Cx, v: &Value, el: El, literal: bool) -> R<()> {
    match v {
        Value::Null => {
            cx.set_nil(el);
            Ok(())
        }
        Value::Bool(_) => builtin_to_xml(vm, cx, types::get(types::XSD_BOOLEAN), v, el, literal),
        Value::Int(_) => builtin_to_xml(vm, cx, types::get(types::XSD_INT), v, el, literal),
        Value::Float(_) => builtin_to_xml(vm, cx, types::get(types::XSD_FLOAT), v, el, literal),
        Value::Str(_) => builtin_to_xml(vm, cx, types::get(types::XSD_STRING), v, el, literal),
        Value::Array(a) => {
            if is_list(a) {
                array_to_xml(vm, cx, None, v, el, literal)
            } else {
                map_to_xml(vm, cx, v, el, literal)
            }
        }
        Value::Object(o) => {
            if let Some((ns, n)) = classmap_type(vm, cx, o)
                && let Some(sdl) = cx.sdl.clone()
                && let Some(Enc::Type(t)) = sdl.find_type(&ns, &n)
            {
                return type_to_xml(vm, cx, t, v, el, literal);
            }
            struct_to_xml(vm, cx, v, el, literal)
        }
        _ => Err(soap_error(vm, "Encoding: Cannot find encoding")),
    }
}

/// `SoapVar`: its type code or type name, node name and namespace.
fn soapvar_to_xml(vm: &mut Vm, cx: &mut Cx, o: &ObjRef, el: El, literal: bool) -> R<()> {
    let code = match vm.get_prop_raw(o, b"enc_type") {
        Value::Int(n) => n,
        _ => types::UNKNOWN_TYPE,
    };
    let val = vm.get_prop_raw(o, b"enc_value");
    let stype = match vm.get_prop_raw(o, b"enc_stype") {
        Value::Str(s) => Some(s.to_string_lossy()),
        _ => None,
    };
    let sns = match vm.get_prop_raw(o, b"enc_ns") {
        Value::Str(s) => Some(s.to_string_lossy()),
        _ => None,
    };
    let name = match vm.get_prop_raw(o, b"enc_name") {
        Value::Str(s) => Some(s.to_string_lossy()),
        _ => None,
    };
    let namens = match vm.get_prop_raw(o, b"enc_namens") {
        Value::Str(s) => Some(s.to_string_lossy()),
        _ => None,
    };
    if let Some(n) = &name {
        let q = match &namens {
            Some(ns) if !ns.is_empty() => cx.doc.qname(Some(el), ns, n),
            _ => n.clone(),
        };
        cx.doc.rename(el, &q);
    }
    // The encoding: a WSDL type by name, else the type code.
    let mut enc = None;
    if let Some(t) = &stype
        && let Some(sdl) = cx.sdl.clone()
    {
        enc = sdl.find_type(sns.as_deref().unwrap_or(""), t);
    }
    if enc.is_none() && code != types::UNKNOWN_TYPE && code != 0 {
        enc = types::by_code(code).map(|b| {
            if b.ns == NS_SOAP11_ENC && cx.version == 2 {
                Enc::Builtin(types::by_name(NS_SOAP12_ENC, b.name).unwrap_or(b))
            } else {
                Enc::Builtin(b)
            }
        });
    }
    if code == types::XSD_ANYXML {
        let s = vm.to_php_str(&val)?;
        replace_raw(cx, el, &s.to_string_lossy());
        return Ok(());
    }
    if let Some(t) = &stype
        && apply_typemap(vm, cx, sns.as_deref().unwrap_or(""), t, &val, el, literal)?
    {
        return Ok(());
    }
    if enc.is_none() && (code == types::UNKNOWN_TYPE || code == 0) && stype.is_none() {
        return to_xml(vm, cx, None, &val, el, literal);
    }
    to_xml(vm, cx, enc, &val, el, literal)?;
    if let Some(t) = &stype
        && !literal
    {
        cx.set_type(el, sns.as_deref().unwrap_or(""), t);
    } else if let Some(t) = &stype {
        // Literal use: the explicit type is still written.
        cx.set_type(el, sns.as_deref().unwrap_or(""), t);
    }
    Ok(())
}

/// The text a built-in encoding writes for a value.
pub fn builtin_text(vm: &mut Vm, cx: &Cx, b: Bi, v: &Value) -> R<Option<String>> {
    let mut v = v.deref();
    // Enums: a backed enum's value (observed on PHP 8.5; test
    // gh15711.phpt).
    if let Value::Object(o) = &v
        && instance_of(vm, o, "UnitEnum")
        && !matches!(
            b.conv,
            Conv::AnyType | Conv::Struct | Conv::Map | Conv::Array | Conv::AnyXml
        )
    {
        // Errors are kept while the rest is written; the last one is
        // thrown (observed on PHP 8.5; test gh15711.phpt).
        if !instance_of(vm, o, "BackedEnum") {
            pend_error(vm, "Non-backed enums have no default serialization")?;
            return Ok(Some(String::new()));
        }
        let val = vm.get_prop_raw(o, b"value");
        if matches!(b.conv, Conv::Long | Conv::BigInt) && matches!(val, Value::Str(_)) {
            pend_error(vm, "String-backed enum cannot be serialized as int")?;
            return Ok(Some(String::new()));
        }
        v = val;
    }
    Ok(Some(match b.conv {
        Conv::String | Conv::Token | Conv::Normalized | Conv::Duration | Conv::Decimal => {
            let bytes = value_to_bytes(vm, &v)?;
            to_utf8(vm, cx, &bytes)?
        }
        Conv::Boolean => {
            let t = vm.to_bool(&v);
            if t { "true".into() } else { "false".into() }
        }
        Conv::Long => match &v {
            Value::Float(f) if f.is_finite() && f.abs() >= 9.2e18 => format!("{:.0}", f),
            Value::Float(f) => format!("{}", *f as i64),
            _ => {
                let n = vm.to_int(&v)?;
                n.to_string()
            }
        },
        Conv::BigInt => match &v {
            Value::Float(f) => format!("{:.0}", f),
            _ => {
                let n = vm.to_int(&v)?;
                n.to_string()
            }
        },
        Conv::Double => {
            let f = match &v {
                Value::Float(f) => *f,
                _ => vm.to_float(&v)?,
            };
            fmt_double(vm, f)
        }
        Conv::Base64 => {
            let bytes = value_to_bytes(vm, &v)?;
            base64(&bytes)
        }
        Conv::Hex => {
            let bytes = value_to_bytes(vm, &v)?;
            let mut s = String::with_capacity(bytes.len() * 2);
            for c in bytes {
                s.push_str(&format!("{:02X}", c));
            }
            s
        }
        Conv::DateTime
        | Conv::Time
        | Conv::Date
        | Conv::GYearMonth
        | Conv::GYear
        | Conv::GMonthDay
        | Conv::GDay
        | Conv::GMonth => date_text(vm, cx, b.conv, &v)?,
        _ => return Ok(None),
    }))
}

fn builtin_to_xml(vm: &mut Vm, cx: &mut Cx, b: Bi, v: &Value, el: El, literal: bool) -> R<()> {
    match b.conv {
        Conv::AnyType => return guess_to_xml(vm, cx, v, el, literal),
        Conv::AnyXml => {
            let s = vm.to_php_str(v)?;
            replace_raw(cx, el, &s.to_string_lossy());
            return Ok(());
        }
        Conv::Array => return array_to_xml(vm, cx, None, v, el, literal),
        Conv::Struct => return struct_to_xml(vm, cx, v, el, literal),
        Conv::Map => return map_to_xml(vm, cx, v, el, literal),
        _ => {}
    }
    if let Some(t) = builtin_text(vm, cx, b, v)? {
        // An empty string is still a text node: `<a></a>` (observed on
        // PHP 8.5; test interop/Round2/Base/r2_base_002p.phpt).
        cx.doc.els[el].kids.push(Kid::Text(t));
    }
    // The list types NMTOKENS, IDREFS, ENTITIES carry no xsi:type
    // (observed on PHP 8.5; test gh15711.phpt).
    let list = matches!(
        b.code,
        types::XSD_NMTOKENS | types::XSD_IDREFS | types::XSD_ENTITIES
    );
    if !literal && !list {
        let ns = if b.ns == NS_SOAP11_ENC && cx.version == 2 {
            NS_SOAP12_ENC
        } else {
            b.ns
        };
        cx.set_type(el, ns, b.name);
    }
    Ok(())
}

pub fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

/// dateTime and friends: timestamps in local time with the offset ("Z"
/// for UTC), DateTimeInterface objects in their own time zone with
/// microseconds, strings as given (observed on PHP 8.5; tests
/// schema/schema064.phpt, schema/schema086.phpt).
fn date_text(vm: &mut Vm, cx: &Cx, conv: Conv, v: &Value) -> R<String> {
    match v {
        Value::Int(_) | Value::Float(_) => {
            let ts = match v {
                Value::Int(n) => *n,
                Value::Float(f) => *f as i64,
                _ => 0,
            };
            let (y, mo, d, h, mi, s, off) = super::local_time(ts);
            let date = match conv {
                Conv::DateTime => format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}"),
                Conv::Time => format!("{h:02}:{mi:02}:{s:02}"),
                Conv::Date => format!("{y:04}-{mo:02}-{d:02}"),
                Conv::GYearMonth => format!("{y:04}-{mo:02}"),
                Conv::GYear => format!("{y:04}"),
                Conv::GMonthDay => format!("--{mo:02}-{d:02}"),
                Conv::GDay => format!("---{d:02}"),
                Conv::GMonth => format!("--{mo:02}--"),
                _ => String::new(),
            };
            Ok(format!("{date}{}", tz_suffix(off)))
        }
        Value::Object(o) if instance_of(vm, o, "DateTimeInterface") => {
            let fmt = match conv {
                Conv::DateTime => "Y-m-d\\TH:i:s.u",
                Conv::Time => "H:i:s.u",
                Conv::Date => "Y-m-d",
                Conv::GYearMonth => "Y-m",
                Conv::GYear => "Y",
                Conv::GMonthDay => "--m-d",
                Conv::GDay => "---d",
                Conv::GMonth => "--m--",
                _ => "",
            };
            let r = vm.call_method(o, b"format", vec![Value::Str(PhpStr::from(fmt))])?;
            let off = vm.call_method(o, b"getoffset", vec![])?;
            let off = match off {
                Value::Int(n) => n,
                _ => 0,
            };
            Ok(format!(
                "{}{}",
                vm.to_php_str(&r)?.to_string_lossy(),
                tz_suffix(off)
            ))
        }
        _ => {
            let b = value_to_bytes(vm, v)?;
            to_utf8(vm, cx, &b)
        }
    }
}

fn tz_suffix(off: i64) -> String {
    if off == 0 {
        return "Z".into();
    }
    let sign = if off < 0 { '-' } else { '+' };
    let a = off.abs();
    format!("{sign}{:02}:{:02}", a / 3600, (a % 3600) / 60)
}

/// The elements of an array-like value (arrays, Traversable objects).
pub fn iter_values(vm: &mut Vm, v: &Value) -> R<Option<Vec<(Value, Value)>>> {
    match v {
        Value::Array(a) => Ok(Some(
            a.iter().map(|(k, v)| (k.to_value(), v.deref())).collect(),
        )),
        Value::Object(o) if instance_of(vm, o, "Traversable") => {
            let items = vm.iterate_to_vec(v, true)?;
            Ok(Some(items))
        }
        _ => Ok(None),
    }
}

/// SOAP-ENC:Array (or a WSDL array type `tid`).
fn array_to_xml(
    vm: &mut Vm,
    cx: &mut Cx,
    tid: Option<TId>,
    v: &Value,
    el: El,
    literal: bool,
) -> R<()> {
    let items = match iter_values(vm, v)? {
        Some(i) => i,
        None => {
            // A scalar for an array: written as is.
            return guess_to_xml(vm, cx, v, el, literal);
        }
    };
    let sdl = cx.sdl.clone();
    // What the WSDL says about the items.
    let mut item_enc: Option<Enc> = None;
    let mut item_q: Option<(String, String)> = None;
    let mut item_name = "item".to_string();
    let mut item_elem: Option<TId> = None;
    let mut dims_decl = 1usize;
    if let (Some(t), Some(sdl)) = (tid, &sdl) {
        if let Some(info) = array_info(sdl, t) {
            item_q = info.item.clone();
            item_enc = info.item.as_ref().and_then(|(ns, n)| sdl.find_type(ns, n));
            dims_decl = info.dims.max(1);
            if let Some(e) = info.elem {
                item_elem = Some(e);
                let ed = &sdl.types[sdl.deref_element(e)];
                item_name = ed.name.clone();
                if item_enc.is_none() {
                    item_enc = ed.base.as_ref().and_then(|b| sdl.resolve(b));
                    if let Some(Enc::Builtin(b)) = item_enc {
                        item_q = Some((b.ns.into(), b.name.into()));
                    } else if let Some(Enc::Type(x)) = item_enc {
                        item_q = Some((sdl.types[x].ns.clone(), sdl.types[x].name.clone()));
                    }
                }
            }
        }
    }
    // Dimensions (nested arrays for multi-dimensional types).
    let mut dims: Vec<usize> = vec![items.len()];
    let mut flat: Vec<Value> = Vec::new();
    if dims_decl > 1 {
        flatten(vm, &items, dims_decl, &mut dims, &mut flat, 0)?;
    } else {
        flat = items.iter().map(|(_, v)| v.clone()).collect();
    }
    let dims_s = dims
        .iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join(",");
    if !literal {
        // The item type: declared, or the common guessed type.
        let mut q = item_q.clone();
        let uniform = if q.is_none() {
            let mut common: Option<Option<(String, String)>> = None;
            let mut same = true;
            for it in &flat {
                let g = guess_qname(vm, cx, it);
                match &common {
                    None => common = Some(g),
                    Some(c) => {
                        if *c != g {
                            same = false;
                        }
                    }
                }
            }
            match (same, common) {
                (true, Some(Some(g))) => {
                    q = Some(g);
                    true
                }
                _ => false,
            }
        } else {
            true
        };
        if cx.version == 2 {
            let (tv, ok) = match &q {
                Some((ns, n)) if uniform => (cx.doc.qname(Some(el), ns, n), true),
                _ => (String::new(), false),
            };
            // An array of structs also declares the SOAP 1.1 encoding
            // namespace (as the expected output of test bugs/bug41566.phpt
            // shows).
            if ok && matches!(&q, Some((ns, n)) if ns == NS_SOAP12_ENC && n == "Struct") {
                cx.doc.ensure_ns(Some(el), NS_SOAP11_ENC);
            }
            let a = cx.doc.qname(Some(el), NS_SOAP12_ENC, "itemType");
            let tv = if ok {
                tv
            } else {
                cx.doc.qname(Some(el), NS_XSD, "anyType")
            };
            cx.doc.set_attr(el, &a, &tv);
            let a = cx.doc.qname(Some(el), NS_SOAP12_ENC, "arraySize");
            cx.doc.set_attr(el, &a, &dims_s.replace(',', " "));
        } else {
            match &q {
                Some((ns, n)) if uniform => {
                    let tv = cx.doc.qname(Some(el), ns, n);
                    let a = cx.doc.qname(Some(el), NS_SOAP11_ENC, "arrayType");
                    cx.doc.set_attr(el, &a, &format!("{tv}[{dims_s}]"));
                }
                _ => {
                    let a = cx.doc.qname(Some(el), NS_SOAP11_ENC, "arrayType");
                    let tv = cx.doc.qname(Some(el), NS_XSD, "ur-type");
                    cx.doc.set_attr(el, &a, &format!("{tv}[{dims_s}]"));
                }
            }
        }
    }
    if literal && let Some((ns, n)) = &item_q {
        // The item type's namespace is declared anyway (observed on PHP
        // 8.5; test schema/schema057.phpt).
        let _ = cx.doc.qname(Some(el), ns, n);
    }
    // Literal arrays from an arrayType attribute name their items after
    // the item type (observed on PHP 8.5; test schema/schema056.phpt).
    let lit_name = if literal && item_elem.is_none() {
        item_q.as_ref().map(|(ns, n)| (ns.clone(), n.clone()))
    } else {
        None
    };
    for it in flat {
        let name = match &lit_name {
            Some((ns, n)) => cx.doc.qname(Some(el), ns, n),
            None => match item_elem.and_then(|e| sdl.as_ref().map(|s| (s, e))) {
                Some((s, e)) => {
                    let ed = &s.types[s.deref_element(e)];
                    if ed.qualified {
                        let (ns, n) = (ed.ns.clone(), ed.name.clone());
                        cx.doc.qname(Some(el), &ns, &n)
                    } else {
                        item_name.clone()
                    }
                }
                None => item_name.clone(),
            },
        };
        let c = cx.doc.add_child(el, &name);
        to_xml(vm, cx, item_enc, &it, c, literal)?;
    }
    if !literal {
        match (tid, &sdl) {
            (Some(t), Some(sdl)) if cx.features & SOAP_USE_XSI_ARRAY_TYPE == 0 => {
                let (ns, n) = (sdl.types[t].ns.clone(), sdl.types[t].name.clone());
                cx.set_type(el, &ns, &n);
            }
            _ => {
                let ns = cx.enc_ns();
                cx.set_type(el, ns, "Array");
            }
        }
    }
    Ok(())
}

fn flatten(
    vm: &mut Vm,
    items: &[(Value, Value)],
    dims: usize,
    out_dims: &mut Vec<usize>,
    flat: &mut Vec<Value>,
    level: usize,
) -> R<()> {
    if level + 1 >= dims {
        for (_, v) in items {
            flat.push(v.clone());
        }
        return Ok(());
    }
    for (_, v) in items {
        if let Some(sub) = iter_values(vm, v)? {
            if out_dims.len() <= level + 1 {
                out_dims.push(sub.len());
            } else if out_dims[level + 1] < sub.len() {
                out_dims[level + 1] = sub.len();
            }
            flatten(vm, &sub, dims, out_dims, flat, level + 1)?;
        } else {
            flat.push(v.clone());
        }
    }
    Ok(())
}

/// What a WSDL array type says about its items.
pub struct ArrayInfo {
    pub item: Option<(String, String)>,
    pub dims: usize,
    /// Declared sizes (wsdl:arraySize; -1 for "*").
    pub sizes: Vec<i64>,
    /// The item element (arrays defined by a content model).
    pub elem: Option<TId>,
}

/// Array details of type `t`: wsdl:arrayType (SOAP 1.1), wsdl:itemType
/// and wsdl:arraySize (SOAP 1.2), or a single repeated element.
pub fn array_info(sdl: &Sdl, t: TId) -> Option<ArrayInfo> {
    let mut id = t;
    for _ in 0..16 {
        let ty = &sdl.types[id];
        if ty.kind == Kind::Element {
            match ty.base.as_ref().and_then(|b| sdl.resolve(b)) {
                Some(Enc::Type(b)) => {
                    id = b;
                    continue;
                }
                _ => return None,
            }
        }
        if ty.soap_array {
            let attrs = sdl.attrs_of(id);
            let mut info = ArrayInfo {
                item: None,
                dims: 1,
                sizes: Vec::new(),
                elem: None,
            };
            for a in &attrs {
                if let Some((ns, n, d)) = &a.array_type {
                    info.item = Some((ns.clone(), n.clone()));
                    info.dims = d.matches(',').count() + 1;
                    // "[][]": arrays of arrays.
                    if d.matches('[').count() > 1 {
                        info.dims = 1;
                    }
                }
                if let Some((ns, n)) = &a.item_type {
                    info.item = Some((ns.clone(), n.clone()));
                }
                if let Some(sz) = &a.array_size {
                    info.dims = sz.split_whitespace().count().max(1);
                    info.sizes = sz
                        .split_whitespace()
                        .map(|p| p.parse::<i64>().unwrap_or(-1))
                        .collect();
                }
            }
            if let Some(m) = &ty.model
                && let Some(e) = single_element(sdl, m)
            {
                info.elem = Some(e);
            }
            return Some(info);
        }
        // Derived from an array type.
        match ty.base.as_ref().and_then(|b| sdl.resolve(b)) {
            Some(Enc::Type(b))
                if b != id
                    && matches!(
                        ty.deriv,
                        Deriv::ComplexRestriction | Deriv::ComplexExtension
                    ) =>
            {
                id = b
            }
            _ => return None,
        }
    }
    None
}

fn single_element(sdl: &Sdl, m: &Model) -> Option<TId> {
    match m.kind {
        MKind::Element => m.elem,
        MKind::Group => sdl
            .group(m.group.as_ref()?)
            .and_then(|g| single_element(sdl, g)),
        _ => {
            if m.items.len() == 1 {
                single_element(sdl, &m.items[0])
            } else {
                None
            }
        }
    }
}

/// Apache map (`http://xml.apache.org/xml-soap` Map).
fn map_to_xml(vm: &mut Vm, cx: &mut Cx, v: &Value, el: El, literal: bool) -> R<()> {
    let items = iter_values(vm, v)?.unwrap_or_default();
    for (k, val) in items {
        let item = cx.doc.add_child(el, "item");
        let key = cx.doc.add_child(item, "key");
        let (kt, ks) = match &k {
            Value::Int(n) => ("int", n.to_string()),
            _ => ("string", vm.to_php_str(&k)?.to_string_lossy()),
        };
        if !literal {
            // The key's xsi:type is written before its namespace is
            // needed (observed on PHP 8.5; test schema/schema054.phpt).
            let a = cx.doc.qname(Some(key), NS_XSI, "type");
            let t = cx.doc.qname(Some(key), NS_XSD, kt);
            cx.doc.set_attr(key, &a, &t);
        }
        cx.doc.add_text(key, &ks);
        let value = cx.doc.add_child(item, "value");
        to_xml(vm, cx, None, &val, value, literal)?;
    }
    if !literal {
        cx.set_type(el, NS_APACHE, "Map");
    }
    Ok(())
}

/// The properties of an object (all visibilities) or an array's entries.
pub fn fields(vm: &mut Vm, v: &Value) -> Vec<(PhpStr, Value)> {
    match v {
        Value::Object(o) => {
            let a = vm.plain_props_array(o, false);
            a.iter().map(|(k, v)| (key_str(k), v.deref())).collect()
        }
        Value::Array(a) => a.iter().map(|(k, v)| (key_str(k), v.deref())).collect(),
        _ => Vec::new(),
    }
}

pub fn key_str(k: KeyRef) -> PhpStr {
    match k {
        KeyRef::Int(n) => PhpStr::from(n.to_string()),
        KeyRef::Str(s) => s.clone(),
    }
}

/// SOAP-ENC:Struct: an object's properties or an array's entries.
fn struct_to_xml(vm: &mut Vm, cx: &mut Cx, v: &Value, el: El, literal: bool) -> R<()> {
    let props = fields(vm, v);
    for (name, val) in props {
        let n = name.to_string_lossy();
        let c = cx.doc.add_child(el, &n);
        to_xml(vm, cx, None, &val, c, literal)?;
    }
    if !literal {
        let ns = cx.enc_ns();
        cx.set_type(el, ns, "Struct");
    }
    Ok(())
}

/// A property of an object or an entry of an array, `None` when absent.
pub fn get_field(vm: &mut Vm, v: &Value, name: &str) -> R<Option<Value>> {
    match v {
        Value::Object(o) => {
            let a = vm.plain_props_array(o, false);
            if let Some(x) = a.get_bytes(name.as_bytes()) {
                return Ok(Some(x.deref()));
            }
            // __get() (test classmap004.phpt).
            let cls = vm.class_of(o);
            if cls.find_method(b"__get").is_some() {
                let isset = if cls.find_method(b"__isset").is_some() {
                    vm.prop_isset(o, &PhpStr::from(name), false)?
                } else {
                    true
                };
                if isset {
                    let r = vm.read_prop(o, &PhpStr::from(name), true, None)?;
                    return Ok(Some(r.deref()));
                }
            }
            Ok(None)
        }
        Value::Array(a) => Ok(a.get(Key::from(name).as_ref()).map(|x| x.deref())),
        _ => Ok(None),
    }
}

/// A WSDL type.
fn type_to_xml(vm: &mut Vm, cx: &mut Cx, t: TId, v: &Value, el: El, literal: bool) -> R<()> {
    let sdl = cx.sdl.clone().expect("WSDL types need a WSDL");
    let ty = &sdl.types[t];
    match ty.kind {
        Kind::Element => {
            let e = sdl.deref_element(t);
            let et = &sdl.types[e];
            let base = et.base.as_ref().and_then(|b| sdl.resolve(b));
            to_xml(vm, cx, base, v, el, literal)
        }
        Kind::Simple => {
            // BackedEnum values encode as their value (test gh15711.phpt).
            let v = backed_value(vm, v)?;
            let base = ty.base.as_ref().and_then(|b| sdl.resolve(b));
            match base {
                Some(b) => to_xml(vm, cx, Some(b), &v, el, literal)?,
                None => {
                    if ty.base.is_some() {
                        builtin_to_xml(vm, cx, types::string(), &v, el, literal)?
                    } else {
                        guess_to_xml(vm, cx, &v, el, literal)?
                    }
                }
            }
            if !literal {
                let (ns, n) = (ty.ns.clone(), ty.name.clone());
                cx.set_type(el, &ns, &n);
            }
            Ok(())
        }
        Kind::List => {
            let item = ty.list_item.as_ref().and_then(|b| sdl.resolve(b));
            let parts: Vec<Value> = match iter_values(vm, v)? {
                Some(items) => items.into_iter().map(|(_, v)| v).collect(),
                None => {
                    let s = value_to_bytes(vm, v)?;
                    String::from_utf8_lossy(&s)
                        .split_whitespace()
                        .map(|p| Value::Str(PhpStr::from(p)))
                        .collect()
                }
            };
            let mut out = Vec::new();
            for p in parts {
                out.push(simple_text(vm, cx, &sdl, item, &p)?);
            }
            let s = out.join(" ");
            if !s.is_empty() {
                cx.doc.add_text(el, &s);
            }
            if !literal {
                let (ns, n) = (ty.ns.clone(), ty.name.clone());
                cx.set_type(el, &ns, &n);
            }
            Ok(())
        }
        Kind::Union => {
            let s = match iter_values(vm, v)? {
                Some(items) => {
                    let mut out = Vec::new();
                    for (_, x) in items {
                        let b = value_to_bytes(vm, &x)?;
                        out.push(to_utf8(vm, cx, &b)?);
                    }
                    out.join(" ")
                }
                None => {
                    let b = value_to_bytes(vm, v)?;
                    to_utf8(vm, cx, &b)?
                }
            };
            if !s.is_empty() {
                cx.doc.add_text(el, &s);
            }
            if !literal {
                let (ns, n) = (ty.ns.clone(), ty.name.clone());
                cx.set_type(el, &ns, &n);
            }
            Ok(())
        }
        Kind::Complex => complex_to_xml(vm, cx, &sdl, t, v, el, literal),
    }
}

fn backed_value(vm: &mut Vm, v: &Value) -> R<Value> {
    if let Value::Object(o) = v
        && instance_of(vm, o, "BackedEnum")
    {
        return Ok(vm.get_prop_raw(o, b"value"));
    }
    Ok(v.clone())
}

/// The text of a simple value with encoding `enc` (list items).
fn simple_text(vm: &mut Vm, cx: &mut Cx, sdl: &Sdl, enc: Option<Enc>, v: &Value) -> R<String> {
    let mut enc = enc;
    for _ in 0..16 {
        match enc {
            Some(Enc::Builtin(b)) => {
                return Ok(builtin_text(vm, cx, b, v)?.unwrap_or_default());
            }
            Some(Enc::Type(t)) => {
                let ty = &sdl.types[t];
                match ty.kind {
                    Kind::Simple | Kind::Element => {
                        enc = ty.base.as_ref().and_then(|b| sdl.resolve(b));
                        if enc.is_none() {
                            break;
                        }
                    }
                    Kind::List | Kind::Union | Kind::Complex => break,
                }
            }
            None => break,
        }
    }
    let b = value_to_bytes(vm, v)?;
    to_utf8(vm, cx, &b)
}

/// Whether a type is (derived from) a simple-content type, and its
/// ultimate simple base.
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

fn complex_to_xml(
    vm: &mut Vm,
    cx: &mut Cx,
    sdl: &Rc<Sdl>,
    t: TId,
    v: &Value,
    el: El,
    literal: bool,
) -> R<()> {
    let ty = &sdl.types[t];
    if array_info(sdl, t).is_some() {
        return array_to_xml(vm, cx, Some(t), v, el, literal);
    }
    // Derivations from built-in complex encodings (Apache Map, Struct).
    if matches!(
        ty.deriv,
        Deriv::ComplexExtension | Deriv::ComplexRestriction
    ) && let Some(Enc::Builtin(b)) = ty.base.as_ref().and_then(|b| sdl.resolve(b))
    {
        match b.conv {
            Conv::Map => return map_to_xml(vm, cx, v, el, literal),
            Conv::Array => return array_to_xml(vm, cx, None, v, el, literal),
            _ => {}
        }
    }
    // Objects implementing Traversable, ArrayObject... are arrays for
    // a content model (tests server030.phpt, server031.phpt).
    let v = match v {
        Value::Object(o)
            if instance_of(vm, o, "Traversable")
                && !class_is(vm, o, "stdClass")
                && fields(vm, v).is_empty() =>
        {
            let items = vm.iterate_to_vec(v, true)?;
            let mut a = PhpArray::new();
            for (k, x) in items {
                let key = match k {
                    Value::Int(n) => Key::Int(n),
                    other => Key::from_php_str(&vm.to_php_str(&other)?),
                };
                a.insert(key, x);
            }
            Value::Array(a)
        }
        _ => v.clone(),
    };
    let v = &v;
    complex_body(vm, cx, sdl, t, v, el, literal, 0)?;
    if !literal {
        let (ns, n) = (ty.ns.clone(), ty.name.clone());
        cx.set_type(el, &ns, &n);
    }
    Ok(())
}

/// The content and attributes of complex type `t`: an extension writes
/// its base first (with the base's xsi:type, replaced afterwards), a
/// simple-content type its "_" value, others their content model
/// (observed on PHP 8.5; tests schema/schema042.phpt to schema049.phpt).
#[allow(clippy::too_many_arguments)]
fn complex_body(
    vm: &mut Vm,
    cx: &mut Cx,
    sdl: &Rc<Sdl>,
    t: TId,
    v: &Value,
    el: El,
    literal: bool,
    depth: u32,
) -> R<()> {
    let ty = &sdl.types[t];
    let base_complex = match ty.base.as_ref().and_then(|b| sdl.resolve(b)) {
        Some(Enc::Type(b)) if b != t && sdl.types[b].kind == Kind::Complex && depth < 16 => Some(b),
        _ => None,
    };
    let is_ext = matches!(ty.deriv, Deriv::SimpleExtension | Deriv::ComplexExtension);
    if is_ext && let Some(b) = base_complex {
        complex_body(vm, cx, sdl, b, v, el, literal, depth + 1)?;
        if !literal {
            let (ns, n) = (sdl.types[b].ns.clone(), sdl.types[b].name.clone());
            cx.set_type(el, &ns, &n);
        }
        if let Some(m) = ty.model.clone() {
            model_to_xml(vm, cx, sdl, &m, v, el, literal, true)?;
        }
        return own_attrs_to_xml(vm, cx, sdl, t, v, el);
    }
    if simple_content_base(sdl, t).is_some() {
        // Simple content: the value is "_" (or the value itself).
        let val = match v {
            Value::Object(_) | Value::Array(_) => get_field(vm, v, "_")?.unwrap_or(Value::Null),
            other => other.clone(),
        };
        match &val {
            Value::Null => cx.set_nil(el),
            _ => match sdl.simple_base(t) {
                Some(b) => to_xml(vm, cx, Some(b), &val, el, literal)?,
                None => builtin_to_xml(vm, cx, types::string(), &val, el, literal)?,
            },
        }
        if matches!(v, Value::Object(_) | Value::Array(_)) {
            own_attrs_to_xml(vm, cx, sdl, t, v, el)?;
        }
        return Ok(());
    }
    if let Some(m) = ty.model.clone() {
        model_to_xml(vm, cx, sdl, &m, v, el, literal, true)?;
    }
    own_attrs_to_xml(vm, cx, sdl, t, v, el)
}

/// The attributes type `t` declares.
fn own_attrs_to_xml(vm: &mut Vm, cx: &mut Cx, sdl: &Rc<Sdl>, t: TId, v: &Value, el: El) -> R<()> {
    if !matches!(v, Value::Object(_) | Value::Array(_)) {
        return Ok(());
    }
    for a in sdl.attrs_of(t) {
        if a.name.is_empty() {
            continue;
        }
        let val = get_field(vm, v, &a.name)?;
        let Some(val) = val else {
            continue;
        };
        if let Value::Null = val {
            continue;
        }
        let enc = a.typ.as_ref().and_then(|x| sdl.resolve(x));
        let text = if let Value::Object(_) = &val {
            // An object: encoded on a detached node, its text used (none:
            // no attribute; observed on PHP 8.5; test bugs/gh18640.phpt).
            let tmp = cx.doc.new_el("attr");
            cx.doc.els[tmp].parent = Some(el);
            to_xml(vm, cx, enc, &val, tmp, true)?;
            if cx.doc.has_element_kids(tmp) || cx.doc.is_empty(tmp) {
                continue;
            }
            cx.doc.text_of(tmp)
        } else {
            simple_text(vm, cx, sdl, enc, &val)?
        };
        if let Some(f) = &a.fixed
            && f != &text
        {
            return Err(soap_error(
                vm,
                &format!(
                    "Encoding: Attribute '{}' has fixed value '{}' (value '{}' is not allowed)",
                    a.name, f, text
                ),
            ));
        }
        let name = if a.qualified && !a.ns.is_empty() {
            cx.doc.qname(Some(el), &a.ns, &a.name)
        } else {
            a.name.clone()
        };
        cx.doc.set_attr(el, &name, &text);
    }
    Ok(())
}

/// Writes a content model; `false` when a required element is missing
/// and `strict` is off (choice alternatives).
#[allow(clippy::too_many_arguments)]
fn model_to_xml(
    vm: &mut Vm,
    cx: &mut Cx,
    sdl: &Rc<Sdl>,
    m: &Model,
    v: &Value,
    el: El,
    literal: bool,
    strict: bool,
) -> R<bool> {
    let group_rep = m.kind != MKind::Element && m.max != 1;
    if group_rep {
        cx.rep += 1;
    }
    let r = model_inner(vm, cx, sdl, m, v, el, literal, strict);
    if group_rep {
        cx.rep -= 1;
    }
    r
}

#[allow(clippy::too_many_arguments)]
fn model_inner(
    vm: &mut Vm,
    cx: &mut Cx,
    sdl: &Rc<Sdl>,
    m: &Model,
    v: &Value,
    el: El,
    literal: bool,
    strict: bool,
) -> R<bool> {
    match m.kind {
        MKind::Element => {
            let Some(e) = m.elem else { return Ok(true) };
            element_to_xml(vm, cx, sdl, e, m, v, el, literal, strict)
        }
        MKind::Sequence | MKind::All => {
            for it in &m.items {
                if !model_to_xml(vm, cx, sdl, it, v, el, literal, strict && m.min != 0)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        MKind::Choice => {
            // A repeating choice writes every alternative present
            // (observed on PHP 8.5; test bugs/bug35142.phpt).
            let mut any = false;
            for it in &m.items {
                if model_present(vm, sdl, it, v)? {
                    let r = model_to_xml(vm, cx, sdl, it, v, el, literal, strict)?;
                    if m.max == 1 {
                        return Ok(r);
                    }
                    any = true;
                }
            }
            Ok(any || !strict || m.min == 0 || m.items.is_empty())
        }
        MKind::Group => {
            let g = m.group.as_ref().and_then(|q| sdl.group(q)).cloned();
            match g {
                Some(g) => model_to_xml(vm, cx, sdl, &g, v, el, literal, strict),
                None => {
                    let mut ok = true;
                    for it in &m.items {
                        ok &= model_to_xml(vm, cx, sdl, it, v, el, literal, strict)?;
                    }
                    Ok(ok)
                }
            }
        }
        MKind::Any => {
            if let Some(a) = get_field(vm, v, "any")? {
                match &a {
                    Value::Str(s) => {
                        let raw = s.to_string_lossy();
                        declare_raw_prefixes(cx, el, &raw);
                        cx.doc.add_raw(el, &raw);
                    }
                    Value::Array(arr) => {
                        for (k, x) in arr.iter() {
                            match x.deref() {
                                Value::Str(s) => {
                                    cx.doc.add_raw(el, &s.to_string_lossy());
                                }
                                other => {
                                    let n = key_str(k).to_string_lossy();
                                    let c = cx.doc.add_child(el, &n);
                                    to_xml(vm, cx, None, &other, c, literal)?;
                                }
                            }
                        }
                    }
                    other => {
                        let c = cx.doc.add_child(el, "any");
                        to_xml(vm, cx, None, other, c, literal)?;
                    }
                }
            }
            Ok(true)
        }
    }
}

/// Whether an element declaration is `ref="xsd:schema"`.
pub fn is_schema_ref(decl: &SType) -> bool {
    matches!(&decl.r#ref, Some((ns, n)) if ns == NS_XSD && n == "schema")
}

/// Markup inserted as is: the well-known prefixes it uses get declared
/// on the envelope (observed on PHP 8.5, which parses it in the
/// document's context; test bugs/bug30106.phpt).
fn declare_raw_prefixes(cx: &mut Cx, el: El, raw: &str) {
    for (p, uri) in [
        ("xsd", NS_XSD),
        ("xsi", NS_XSI),
        ("SOAP-ENC", NS_SOAP11_ENC),
    ] {
        let tag = format!("<{p}:");
        let attr = format!(" {p}:");
        if (raw.contains(&tag) || raw.contains(&attr)) && !raw.contains(&format!("xmlns:{p}=")) {
            cx.doc.ensure_ns(Some(el), uri);
        }
    }
}

/// Whether a choice alternative has its data in `v`.
fn model_present(vm: &mut Vm, sdl: &Rc<Sdl>, m: &Model, v: &Value) -> R<bool> {
    match m.kind {
        MKind::Element => {
            let Some(e) = m.elem else { return Ok(false) };
            let ed = &sdl.types[sdl.deref_element(e)];
            Ok(get_field(vm, v, &ed.name)?.is_some())
        }
        MKind::Any => Ok(get_field(vm, v, "any")?.is_some()),
        MKind::Group => match m.group.as_ref().and_then(|q| sdl.group(q)).cloned() {
            Some(g) => model_present(vm, sdl, &g, v),
            None => Ok(false),
        },
        _ => {
            for it in &m.items {
                if model_present(vm, sdl, it, v)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn element_to_xml(
    vm: &mut Vm,
    cx: &mut Cx,
    sdl: &Rc<Sdl>,
    e: TId,
    m: &Model,
    v: &Value,
    el: El,
    literal: bool,
    strict: bool,
) -> R<bool> {
    let ed_id = sdl.deref_element(e);
    let ed: &SType = &sdl.types[ed_id];
    let decl = &sdl.types[e];
    let name = ed.name.clone();
    // Encoded messages never qualify local elements (observed on PHP
    // 8.5; tests interop/Round4/GroupH/r4_groupH_complex_rpcenc_*).
    let qualified = ed.qualified && literal;
    let nillable = ed.nillable || decl.nillable;
    let min = m.min;
    let max = if cx.rep > 0 { -1 } else { m.max };
    let mut val = get_field(vm, v, &name)?;
    // A reference to xsd:schema holds markup (`<anyXML>` in __getTypes();
    // observed on PHP 8.5; test bugs/bug30106.phpt).
    if is_schema_ref(decl) {
        if let Some(Value::Str(s)) = &val {
            let raw = s.to_string_lossy();
            declare_raw_prefixes(cx, el, &raw);
            cx.doc.add_raw(el, &raw);
        }
        return Ok(true);
    }
    // A list for a struct whose element repeats: the list is that
    // element's values (observed on PHP 8.5; tests
    // interop/Round3/GroupD/r3_groupD_doclit_002w.phpt).
    if val.is_none()
        && max != 1
        && let Value::Array(a) = v
        && is_list(a)
    {
        val = Some(v.clone());
    }
    let enc = ed.base.as_ref().and_then(|b| sdl.resolve(b));
    let ns = ed.ns.clone();
    // The element's prefix is set after its content is written, so the
    // content's namespaces are declared first (observed on PHP 8.5; test
    // interop/Round3/GroupD/r3_groupD_compound2_001w.phpt).
    let qualify = |cx: &mut Cx, c: El| {
        if qualified && !ns.is_empty() {
            let q = cx.doc.qname(Some(c), &ns, &name);
            cx.doc.rename(c, &q);
        }
    };
    match val {
        None => {
            if min == 0 {
                return Ok(true);
            }
            if nillable {
                let c = cx.doc.add_child(el, &name);
                cx.set_nil(c);
                qualify(cx, c);
                return Ok(true);
            }
            if !strict {
                return Ok(false);
            }
            // A scalar for a struct: nothing to take fields from (observed
            // on PHP 8.5; test bugs/bug73237.phpt).
            if !matches!(v, Value::Object(_) | Value::Array(_)) {
                return Ok(true);
            }
            if let Some(d) = ed.fixed.clone().or(ed.default.clone()) {
                let c = cx.doc.add_child(el, &name);
                to_xml(vm, cx, enc, &Value::Str(PhpStr::from(d)), c, literal)?;
                qualify(cx, c);
                return Ok(true);
            }
            Err(soap_error(
                vm,
                &format!("Encoding: object has no '{name}' property"),
            ))
        }
        Some(Value::Null) if !nillable && min == 0 => Ok(true),
        Some(Value::Null) => {
            let c = cx.doc.add_child(el, &name);
            if !literal {
                to_xml(vm, cx, enc, &Value::Null, c, literal)?;
            } else if nillable {
                cx.set_nil(c);
            }
            qualify(cx, c);
            Ok(true)
        }
        Some(x) => {
            let repeat = max != 1
                && match &x {
                    Value::Array(a) => is_list(a),
                    _ => false,
                };
            if repeat {
                let items = iter_values(vm, &x)?.unwrap_or_default();
                for (_, it) in items {
                    let c = cx.doc.add_child(el, &name);
                    if let Value::Null = it {
                        if nillable || !literal {
                            cx.set_nil(c);
                        }
                    } else {
                        to_xml(vm, cx, enc, &it, c, literal)?;
                    }
                    qualify(cx, c);
                }
            } else {
                let c = cx.doc.add_child(el, &name);
                to_xml(vm, cx, enc, &x, c, literal)?;
                // A type the WSDL names but nothing defines: the value as
                // guessed, typed with that name (observed on PHP 8.5;
                // test gh15711.phpt).
                if enc.is_none()
                    && !literal
                    && let Some(super::sdl::TRef::Q(tns, tn)) = &ed.base
                {
                    let (tns, tn) = (tns.clone(), tn.clone());
                    cx.set_type(c, &tns, &tn);
                }
                qualify(cx, c);
            }
            Ok(true)
        }
    }
}

/// Resolves the encoding of a type reference given as a QName attribute
/// (SoapVar names, xsi:type values).
pub fn enc_for(sdl: Option<&Sdl>, ns: &str, name: &str) -> Option<Enc> {
    if let Some(s) = sdl
        && let Some(e) = s.find_type(ns, name)
    {
        return Some(e);
    }
    types::by_name(ns, name).map(Enc::Builtin)
}

pub fn tref_enc(sdl: &Sdl, r: &Option<TRef>) -> Option<Enc> {
    r.as_ref().and_then(|x| sdl.resolve(x))
}
