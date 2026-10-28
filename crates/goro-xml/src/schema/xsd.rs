//! A W3C XML Schema 1.0 validator for the common subset (global and local
//! element declarations, named and anonymous complex types with
//! `sequence`/`choice`/`all` content, attributes with use/default/fixed,
//! simple content, the built-in atomic types and `simpleType`
//! restrictions with enumerations, lengths and bounds), for
//! `DOMDocument::schemaValidate()` and `XMLReader::setSchema()`.
//!
//! Messages follow libxml2's (observed on PHP 8.5; tests
//! ext/dom/tests/DOMDocument_schemaValidate_*.phpt).

use std::collections::HashMap;

use crate::tree::{Arena, NodeData, NodeId, QName};

/// The XML Schema namespace.
pub const XS_NS: &str = "http://www.w3.org/2001/XMLSchema";
const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// A namespace and local name.
type Name = (Option<String>, String);

#[derive(Clone, Debug)]
enum TypeRef {
    Builtin(String),
    Named(Name),
    Complex(Box<ComplexType>),
    Simple(Box<SimpleType>),
}

#[derive(Clone, Debug, Default)]
struct SimpleType {
    base: Option<Box<TypeRef>>,
    enumeration: Vec<String>,
    min_len: Option<usize>,
    max_len: Option<usize>,
    length: Option<usize>,
    min_incl: Option<f64>,
    max_incl: Option<f64>,
    min_excl: Option<f64>,
    max_excl: Option<f64>,
    pattern_digits_only: bool,
    list: bool,
}

#[derive(Clone, Debug, Default)]
struct ComplexType {
    content: Option<Particle>,
    attrs: Vec<AttrDecl>,
    any_attr: bool,
    mixed: bool,
    simple: Option<TypeRef>,
}

#[derive(Clone, Debug)]
struct ElemDecl {
    name: Name,
    ty: Option<TypeRef>,
    reference: Option<Name>,
    min: u32,
    max: Option<u32>,
    nillable: bool,
    fixed: Option<String>,
    default: Option<String>,
}

#[derive(Clone, Debug)]
enum Particle {
    Element(ElemDecl),
    Seq(Vec<Particle>, u32, Option<u32>),
    Choice(Vec<Particle>, u32, Option<u32>),
    All(Vec<Particle>, u32, Option<u32>),
    Any(u32, Option<u32>),
}

#[derive(Clone, Debug)]
struct AttrDecl {
    name: Name,
    ty: Option<TypeRef>,
    required: bool,
    prohibited: bool,
    default: Option<String>,
    fixed: Option<String>,
}

/// A compiled schema.
#[derive(Clone, Debug, Default)]
pub struct Schema {
    elements: HashMap<Name, ElemDecl>,
    types: HashMap<Name, TypeRef>,
    groups: HashMap<Name, Particle>,
    attr_groups: HashMap<Name, Vec<AttrDecl>>,
}

fn xs_children(a: &Arena, n: NodeId) -> Vec<(String, NodeId)> {
    a.children(n)
        .filter_map(|c| {
            let e = a.element(c)?;
            (e.name.ns.as_deref() == Some(XS_NS)).then(|| (e.name.local.to_string(), c))
        })
        .collect()
}

fn attr(a: &Arena, n: NodeId, name: &str) -> Option<String> {
    a.get_attr_node_ns(n, None, name).map(|x| a.attr_value(x))
}

fn occurs(a: &Arena, n: NodeId) -> (u32, Option<u32>) {
    let min = attr(a, n, "minOccurs")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(1);
    let max = match attr(a, n, "maxOccurs").as_deref().map(str::trim) {
        Some("unbounded") => None,
        Some(v) => Some(v.parse().unwrap_or(1)),
        None => Some(1),
    };
    (min, max)
}

struct Parser<'a> {
    a: &'a Arena,
    target: Option<String>,
    elem_qualified: bool,
    attr_qualified: bool,
    schema: Schema,
    errors: Vec<String>,
}

impl Parser<'_> {
    /// Resolves a QName attribute value (`xs:string`, `bks:BookForm`).
    fn qname(&self, at: NodeId, v: &str) -> Name {
        let (p, l) = match v.split_once(':') {
            Some((p, l)) => (Some(p), l),
            None => (None, v),
        };
        let ns = self.a.lookup_ns(at, p).map(|s| s.to_string());
        (ns, l.trim().to_string())
    }

    fn type_ref(&self, at: NodeId, v: &str) -> TypeRef {
        let (ns, l) = self.qname(at, v);
        if ns.as_deref() == Some(XS_NS) {
            TypeRef::Builtin(l)
        } else {
            TypeRef::Named((ns, l))
        }
    }

    fn element(&mut self, n: NodeId, global: bool) -> ElemDecl {
        let (min, max) = if global {
            (1, Some(1))
        } else {
            occurs(self.a, n)
        };
        if let Some(r) = attr(self.a, n, "ref") {
            let name = self.qname(n, &r);
            return ElemDecl {
                name: name.clone(),
                ty: None,
                reference: Some(name),
                min,
                max,
                nillable: false,
                fixed: None,
                default: None,
            };
        }
        let local = attr(self.a, n, "name").unwrap_or_default();
        let qualified = global
            || match attr(self.a, n, "form").as_deref() {
                Some("qualified") => true,
                Some("unqualified") => false,
                _ => self.elem_qualified,
            };
        let ns = if qualified { self.target.clone() } else { None };
        let mut ty = attr(self.a, n, "type").map(|t| self.type_ref(n, &t));
        for (k, c) in xs_children(self.a, n) {
            match k.as_str() {
                "complexType" => ty = Some(TypeRef::Complex(Box::new(self.complex(c)))),
                "simpleType" => ty = Some(TypeRef::Simple(Box::new(self.simple(c)))),
                _ => {}
            }
        }
        ElemDecl {
            name: (ns, local),
            ty,
            reference: None,
            min,
            max,
            nillable: attr(self.a, n, "nillable").as_deref() == Some("true"),
            fixed: attr(self.a, n, "fixed"),
            default: attr(self.a, n, "default"),
        }
    }

    fn particle(&mut self, kind: &str, n: NodeId) -> Option<Particle> {
        let (min, max) = occurs(self.a, n);
        match kind {
            "element" => Some(Particle::Element(self.element(n, false))),
            "sequence" | "choice" | "all" => {
                let mut v = Vec::new();
                for (k, c) in xs_children(self.a, n) {
                    if let Some(p) = self.particle(&k, c) {
                        v.push(p);
                    }
                }
                Some(match kind {
                    "sequence" => Particle::Seq(v, min, max),
                    "choice" => Particle::Choice(v, min, max),
                    _ => Particle::All(v, min, max),
                })
            }
            "any" => Some(Particle::Any(min, max)),
            "group" => {
                let r = attr(self.a, n, "ref")?;
                let name = self.qname(n, &r);
                let g = self.schema.groups.get(&name).cloned()?;
                Some(Particle::Seq(vec![g], min, max))
            }
            _ => None,
        }
    }

    fn attribute(&mut self, n: NodeId) -> AttrDecl {
        let (name, ty) = if let Some(r) = attr(self.a, n, "ref") {
            (self.qname(n, &r), None)
        } else {
            let local = attr(self.a, n, "name").unwrap_or_default();
            let qualified = match attr(self.a, n, "form").as_deref() {
                Some("qualified") => true,
                Some("unqualified") => false,
                _ => self.attr_qualified,
            };
            let ns = if qualified { self.target.clone() } else { None };
            let mut ty = attr(self.a, n, "type").map(|t| self.type_ref(n, &t));
            for (k, c) in xs_children(self.a, n) {
                if k == "simpleType" {
                    ty = Some(TypeRef::Simple(Box::new(self.simple(c))));
                }
            }
            ((ns, local), ty)
        };
        let usage = attr(self.a, n, "use").unwrap_or_default();
        AttrDecl {
            name,
            ty,
            required: usage == "required",
            prohibited: usage == "prohibited",
            default: attr(self.a, n, "default"),
            fixed: attr(self.a, n, "fixed"),
        }
    }

    fn attrs_of(&mut self, n: NodeId, ct: &mut ComplexType) {
        for (k, c) in xs_children(self.a, n) {
            match k.as_str() {
                "attribute" => {
                    let d = self.attribute(c);
                    ct.attrs.push(d);
                }
                "attributeGroup" => {
                    if let Some(r) = attr(self.a, c, "ref") {
                        let name = self.qname(c, &r);
                        if let Some(g) = self.schema.attr_groups.get(&name) {
                            ct.attrs.extend(g.iter().cloned());
                        }
                    }
                }
                "anyAttribute" => ct.any_attr = true,
                _ => {}
            }
        }
    }

    fn complex(&mut self, n: NodeId) -> ComplexType {
        let mut ct = ComplexType {
            mixed: attr(self.a, n, "mixed").as_deref() == Some("true"),
            ..Default::default()
        };
        for (k, c) in xs_children(self.a, n) {
            match k.as_str() {
                "sequence" | "choice" | "all" | "group" => ct.content = self.particle(&k, c),
                "simpleContent" | "complexContent" => {
                    for (k2, c2) in xs_children(self.a, c) {
                        if k2 == "extension" || k2 == "restriction" {
                            let base = attr(self.a, c2, "base").map(|b| self.type_ref(c2, &b));
                            if k == "simpleContent" {
                                ct.simple = base;
                            } else if let Some(TypeRef::Named(nm)) = &base
                                && let Some(TypeRef::Complex(bt)) = self.schema.types.get(nm)
                                && k2 == "extension"
                            {
                                ct.attrs.extend(bt.attrs.iter().cloned());
                                ct.content = bt.content.clone();
                            }
                            for (k3, c3) in xs_children(self.a, c2) {
                                if matches!(k3.as_str(), "sequence" | "choice" | "all" | "group") {
                                    let p = self.particle(&k3, c3);
                                    ct.content = match (ct.content.take(), p) {
                                        (Some(a), Some(b)) => {
                                            Some(Particle::Seq(vec![a, b], 1, Some(1)))
                                        }
                                        (a, b) => a.or(b),
                                    };
                                }
                            }
                            self.attrs_of(c2, &mut ct);
                        }
                    }
                }
                _ => {}
            }
        }
        self.attrs_of(n, &mut ct);
        ct
    }

    fn simple(&mut self, n: NodeId) -> SimpleType {
        let mut st = SimpleType::default();
        for (k, c) in xs_children(self.a, n) {
            match k.as_str() {
                "restriction" => {
                    st.base = attr(self.a, c, "base").map(|b| Box::new(self.type_ref(c, &b)));
                    for (f, fc) in xs_children(self.a, c) {
                        let v = attr(self.a, fc, "value").unwrap_or_default();
                        match f.as_str() {
                            "enumeration" => st.enumeration.push(v),
                            "minLength" => st.min_len = v.parse().ok(),
                            "maxLength" => st.max_len = v.parse().ok(),
                            "length" => st.length = v.parse().ok(),
                            "minInclusive" => st.min_incl = v.parse().ok(),
                            "maxInclusive" => st.max_incl = v.parse().ok(),
                            "minExclusive" => st.min_excl = v.parse().ok(),
                            "maxExclusive" => st.max_excl = v.parse().ok(),
                            "pattern" => st.pattern_digits_only = v == "[0-9]+" || v == "\\d+",
                            "simpleType" => {
                                st.base = Some(Box::new(TypeRef::Simple(Box::new(self.simple(fc)))))
                            }
                            _ => {}
                        }
                    }
                }
                "list" => st.list = true,
                _ => {}
            }
        }
        st
    }
}

/// True if `root` is an `xs:schema` element.
pub fn is_schema_root(a: &Arena, root: NodeId) -> bool {
    a.element(root)
        .is_some_and(|e| e.name.ns.as_deref() == Some(XS_NS) && &*e.name.local == "schema")
}

/// The `schemaLocation`s of the `xs:include`, `xs:import` and
/// `xs:redefine` children of a schema root.
pub fn includes(a: &Arena, root: NodeId) -> Vec<(String, String)> {
    xs_children(a, root)
        .into_iter()
        .filter(|(k, _)| matches!(k.as_str(), "include" | "import" | "redefine"))
        .filter_map(|(k, c)| attr(a, c, "schemaLocation").map(|l| (k, l)))
        .collect()
}

/// Compiles the schema documents whose `xs:schema` elements are `roots`
/// (the main one first, then the included ones).
pub fn parse(a: &Arena, roots: &[NodeId]) -> Result<Schema, Vec<String>> {
    let Some(&root) = roots.first() else {
        return Err(vec!["The document has no document element.".into()]);
    };
    if !is_schema_root(a, root) {
        let n = qname_display(&a.element(root).unwrap().name);
        return Err(vec![format!(
            "Element '{n}': The document is not a schema document."
        )]);
    }
    let target_of = |r: NodeId| attr(a, r, "targetNamespace").filter(|s| !s.is_empty());
    let main_target = target_of(root);
    let mut p = Parser {
        a,
        target: main_target.clone(),
        elem_qualified: false,
        attr_qualified: false,
        schema: Schema::default(),
        errors: Vec::new(),
    };
    let setup = |p: &mut Parser, r: NodeId| {
        // Included schemas without a target namespace take the including
        // one's (XML Schema 1.0 part 1, 4.2.1).
        p.target = target_of(r).or_else(|| main_target.clone());
        p.elem_qualified = attr(a, r, "elementFormDefault").as_deref() == Some("qualified");
        p.attr_qualified = attr(a, r, "attributeFormDefault").as_deref() == Some("qualified");
    };
    // Named types, groups and attribute groups first (forward references
    // resolve on the second pass).
    for _ in 0..2 {
        for &r in roots {
            setup(&mut p, r);
            for (k, c) in xs_children(a, r) {
                let name = (p.target.clone(), attr(a, c, "name").unwrap_or_default());
                match k.as_str() {
                    "complexType" => {
                        let t = p.complex(c);
                        p.schema.types.insert(name, TypeRef::Complex(Box::new(t)));
                    }
                    "simpleType" => {
                        let t = p.simple(c);
                        p.schema.types.insert(name, TypeRef::Simple(Box::new(t)));
                    }
                    "group" => {
                        for (k2, c2) in xs_children(a, c) {
                            if let Some(part) = p.particle(&k2, c2) {
                                p.schema.groups.insert(name.clone(), part);
                            }
                        }
                    }
                    "attributeGroup" => {
                        let mut ct = ComplexType::default();
                        p.attrs_of(c, &mut ct);
                        p.schema.attr_groups.insert(name, ct.attrs);
                    }
                    _ => {}
                }
            }
        }
    }
    for &r in roots {
        setup(&mut p, r);
        for (k, c) in xs_children(a, r) {
            if k == "element" {
                let d = p.element(c, true);
                p.schema.elements.insert(d.name.clone(), d);
            }
        }
    }
    if !p.errors.is_empty() {
        return Err(p.errors);
    }
    Ok(p.schema)
}

fn qname_display(q: &QName) -> String {
    match &q.ns {
        Some(ns) if !ns.is_empty() => format!("{{{ns}}}{}", q.local),
        _ => q.local.to_string(),
    }
}

fn name_display(n: &Name) -> String {
    match &n.0 {
        Some(ns) => format!("{{{ns}}}{}", n.1),
        None => n.1.clone(),
    }
}

/// True if `v` is a valid value of the built-in XML Schema type `t`
/// (XML Schema 1.0 part 2; types not checked accept anything).
pub(crate) fn builtin_ok(t: &str, v: &str) -> bool {
    let s = v.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r'));
    let int = |s: &str| {
        let d = s.strip_prefix(['+', '-']).unwrap_or(s);
        !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
    };
    let num = |s: &str| -> Option<f64> {
        if s.is_empty() {
            return None;
        }
        s.parse::<f64>().ok()
    };
    match t {
        "string" | "anyType" | "anySimpleType" => true,
        "normalizedString" | "token" | "language" => true,
        "boolean" => matches!(s, "true" | "false" | "1" | "0"),
        "integer" | "long" | "int" | "short" | "byte" | "unsignedLong" | "unsignedInt"
        | "unsignedShort" | "unsignedByte" => {
            if !int(s) {
                return false;
            }
            let Ok(i) = s.parse::<i128>() else {
                return t == "integer";
            };
            match t {
                "long" => i >= i64::MIN as i128 && i <= i64::MAX as i128,
                "int" => i >= i32::MIN as i128 && i <= i32::MAX as i128,
                "short" => i >= i16::MIN as i128 && i <= i16::MAX as i128,
                "byte" => i >= i8::MIN as i128 && i <= i8::MAX as i128,
                "unsignedLong" => i >= 0 && i <= u64::MAX as i128,
                "unsignedInt" => i >= 0 && i <= u32::MAX as i128,
                "unsignedShort" => i >= 0 && i <= u16::MAX as i128,
                "unsignedByte" => i >= 0 && i <= u8::MAX as i128,
                _ => true,
            }
        }
        "positiveInteger" => int(s) && s.parse::<i128>().map_or(!s.starts_with('-'), |i| i > 0),
        "nonNegativeInteger" => int(s) && s.parse::<i128>().map_or(!s.starts_with('-'), |i| i >= 0),
        "negativeInteger" => int(s) && s.parse::<i128>().map_or(s.starts_with('-'), |i| i < 0),
        "nonPositiveInteger" => int(s) && s.parse::<i128>().map_or(s.starts_with('-'), |i| i <= 0),
        "decimal" => {
            let d = s.strip_prefix(['+', '-']).unwrap_or(s);
            !d.is_empty()
                && d.bytes().filter(|&b| b == b'.').count() <= 1
                && d.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                && d != "."
        }
        "float" | "double" => {
            matches!(s, "INF" | "-INF" | "NaN")
                || (num(s).is_some() && !s.contains(['i', 'I', 'n', 'N']))
        }
        "date" => {
            let b = s.as_bytes();
            b.len() >= 10
                && b[4] == b'-'
                && b[7] == b'-'
                && b[..4]
                    .iter()
                    .chain(&b[5..7])
                    .chain(&b[8..10])
                    .all(u8::is_ascii_digit)
        }
        "dateTime" => s.len() >= 19 && s.as_bytes()[10] == b'T' && builtin_ok("date", &s[..10]),
        "time" => {
            let b = s.as_bytes();
            b.len() >= 8 && b[2] == b':' && b[5] == b':'
        }
        "NCName" | "ID" | "IDREF" | "ENTITY" => crate::chars::is_ncname(s),
        "Name" => crate::chars::is_name(s),
        "NMTOKEN" => crate::chars::is_nmtoken(s),
        "QName" => crate::chars::is_qname(s),
        "NMTOKENS" | "IDREFS" | "ENTITIES" => !s.is_empty(),
        _ => true,
    }
}

/// One validity error: the node it is about, libxml's error code and the
/// message (without the trailing newline).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalid {
    /// The element the error is reported on.
    pub node: NodeId,
    /// libxml's error code (`LibXMLError::$code`).
    pub code: u32,
    /// The message.
    pub message: String,
}

// libxml2 xmlParserErrors codes of the schema validity errors (libxml2
// API documentation).
const E_ELT_1: u32 = 1845;
const E_DATATYPE: u32 = 1824;
const E_ATTR_NOT_ALLOWED: u32 = 1866;
const E_ATTR_MISSING: u32 = 1868;
const E_CONTENT: u32 = 1871;
const E_SIMPLE_CONTENT: u32 = 1841;
const E_FIXED: u32 = 1843;

struct V<'a> {
    schema: &'a Schema,
    errors: Vec<Invalid>,
    create: bool,
}

impl V<'_> {
    fn err(&mut self, node: NodeId, code: u32, message: String) {
        self.errors.push(Invalid {
            node,
            code,
            message,
        });
    }

    fn resolve<'b>(&'b self, t: &'b TypeRef) -> &'b TypeRef {
        let mut t = t;
        for _ in 0..16 {
            match t {
                TypeRef::Named(n) => match self.schema.types.get(n) {
                    Some(x) => t = x,
                    None => return t,
                },
                _ => return t,
            }
        }
        t
    }

    fn type_name(t: &TypeRef) -> String {
        match t {
            TypeRef::Builtin(b) => format!("atomic type 'xs:{b}'"),
            TypeRef::Named(n) => format!("atomic type '{}'", name_display(n)),
            _ => "local atomic type".into(),
        }
    }

    /// Checks a simple value; the error text when invalid.
    fn simple_ok(&self, t: &TypeRef, v: &str) -> Option<String> {
        match self.resolve(t) {
            TypeRef::Builtin(b) => {
                if builtin_ok(b, v) {
                    None
                } else {
                    Some(format!(
                        "'{v}' is not a valid value of the atomic type 'xs:{b}'."
                    ))
                }
            }
            TypeRef::Simple(st) => {
                if let Some(b) = &st.base
                    && let Some(e) = self.simple_ok(b, v)
                {
                    return Some(e);
                }
                let len = v.chars().count();
                let name = Self::type_name(t);
                if !st.enumeration.is_empty() && !st.enumeration.iter().any(|e| e == v) {
                    let set = st
                        .enumeration
                        .iter()
                        .map(|e| format!("'{e}'"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Some(format!(
                        "[facet 'enumeration'] The value '{v}' is not an element of the set {{{set}}}."
                    ));
                }
                if st.length.is_some_and(|l| len != l)
                    || st.min_len.is_some_and(|l| len < l)
                    || st.max_len.is_some_and(|l| len > l)
                {
                    return Some(format!("'{v}' is not a valid value of the {name}."));
                }
                if let Ok(x) = v.trim().parse::<f64>()
                    && (st.min_incl.is_some_and(|m| x < m)
                        || st.max_incl.is_some_and(|m| x > m)
                        || st.min_excl.is_some_and(|m| x <= m)
                        || st.max_excl.is_some_and(|m| x >= m))
                {
                    return Some(format!("'{v}' is not a valid value of the {name}."));
                }
                if st.pattern_digits_only && !v.bytes().all(|b| b.is_ascii_digit()) {
                    return Some(format!("'{v}' is not a valid value of the {name}."));
                }
                None
            }
            _ => None,
        }
    }

    fn element(&mut self, a: &mut Arena, el: NodeId, decl: &ElemDecl) {
        let decl = match &decl.reference {
            Some(r) => match self.schema.elements.get(r) {
                Some(d) => d.clone(),
                None => return,
            },
            None => decl.clone(),
        };
        let ename = qname_display(&a.element(el).unwrap().name);
        if decl.nillable
            && a.get_attr_node_ns(el, Some(XSI_NS), "nil")
                .is_some_and(|x| a.attr_value(x) == "true")
        {
            return;
        }
        let ty = decl
            .ty
            .clone()
            .unwrap_or(TypeRef::Builtin("anyType".into()));
        let resolved = self.resolve(&ty).clone();
        match &resolved {
            TypeRef::Complex(ct) => self.complex(a, el, &ename, ct),
            TypeRef::Builtin(b) if b == "anyType" => {}
            t => {
                for at in a.attrs(el).collect::<Vec<_>>() {
                    let q = a.qname(at).unwrap();
                    if q.ns.as_deref() == Some(XSI_NS) {
                        continue;
                    }
                    let an = q.qualified();
                    self.err(
                        el,
                        E_ATTR_NOT_ALLOWED,
                        format!("Element '{ename}', attribute '{an}': The attribute '{an}' is not allowed."),
                    );
                }
                if a.children(el).any(|c| a.is_element(c)) {
                    self.err(
                        el,
                        E_SIMPLE_CONTENT,
                        format!("Element '{ename}': Element content is not allowed, because the type definition is simple."),
                    );
                    return;
                }
                let v = a.text_content(el);
                let v = if v.is_empty() {
                    decl.default.clone().or(decl.fixed.clone()).unwrap_or(v)
                } else {
                    v
                };
                if let Some(e) = self.simple_ok(t, &v) {
                    self.err(el, E_DATATYPE, format!("Element '{ename}': {e}"));
                } else if let Some(f) = &decl.fixed
                    && &v != f
                {
                    self.err(
                        el,
                        E_FIXED,
                        format!("Element '{ename}': The value '{v}' does not match the fixed value constraint '{f}'."),
                    );
                }
            }
        }
    }

    fn complex(&mut self, a: &mut Arena, el: NodeId, ename: &str, ct: &ComplexType) {
        let present: Vec<NodeId> = a.attrs(el).collect();
        for at in &present {
            let q = a.qname(*at).unwrap().clone();
            if q.ns.as_deref() == Some(XSI_NS) {
                continue;
            }
            let decl = ct
                .attrs
                .iter()
                .find(|d| d.name.1 == *q.local && d.name.0.as_deref() == q.ns.as_deref());
            let an = q.qualified();
            match decl {
                Some(d) if !d.prohibited => {
                    let v = a.attr_value(*at);
                    if let Some(t) = &d.ty
                        && let Some(e) = self.simple_ok(t, &v)
                    {
                        self.err(
                            el,
                            E_DATATYPE,
                            format!("Element '{ename}', attribute '{an}': {e}"),
                        );
                    } else if let Some(f) = &d.fixed
                        && &v != f
                    {
                        self.err(
                            el,
                            E_FIXED,
                            format!("Element '{ename}', attribute '{an}': The value '{v}' does not match the fixed value constraint '{f}'."),
                        );
                    }
                }
                _ if ct.any_attr => {}
                _ => self.err(
                    el,
                    E_ATTR_NOT_ALLOWED,
                    format!(
                        "Element '{ename}', attribute '{an}': The attribute '{an}' is not allowed."
                    ),
                ),
            }
        }
        for d in &ct.attrs {
            let has = present.iter().any(|at| {
                let q = a.qname(*at).unwrap();
                *q.local == *d.name.1 && q.ns.as_deref() == d.name.0.as_deref()
            });
            if has {
                continue;
            }
            if d.required {
                self.err(
                    el,
                    E_ATTR_MISSING,
                    format!(
                        "Element '{ename}': The attribute '{}' is required but missing.",
                        name_display(&d.name)
                    ),
                );
            } else if self.create
                && let Some(v) = d.default.clone().or_else(|| d.fixed.clone())
            {
                // LIBXML_SCHEMA_CREATE adds defaulted attributes (test
                // DOMDocument_schemaValidate_addAttrs.phpt).
                let doc = a.doc_of(el);
                let at = a.new_attribute(
                    doc,
                    QName {
                        prefix: None,
                        local: d.name.1.as_str().into(),
                        ns: None,
                    },
                    &v,
                );
                a.raw_append_attr(el, at);
            }
        }
        if let Some(t) = &ct.simple {
            let v = a.text_content(el);
            if let Some(e) = self.simple_ok(t, &v) {
                self.err(el, E_DATATYPE, format!("Element '{ename}': {e}"));
            }
            return;
        }
        let mut elems = Vec::new();
        for c in a.children(el).collect::<Vec<_>>() {
            match a.data(c) {
                NodeData::Element(_) => elems.push(c),
                NodeData::Text(t) | NodeData::CData(t)
                    if !ct.mixed && !t.chars().all(|ch| matches!(ch, ' ' | '\t' | '\n' | '\r')) =>
                {
                    let msg = if ct.content.is_none() {
                        format!(
                            "Element '{ename}': Character content is not allowed, because the content type is empty."
                        )
                    } else {
                        format!(
                            "Element '{ename}': Character content other than whitespace is not allowed because the content type is 'element-only'."
                        )
                    };
                    self.err(el, E_CONTENT, msg);
                    return;
                }
                _ => {}
            }
        }
        let names: Vec<Name> = elems
            .iter()
            .map(|&c| {
                let q = &a.element(c).unwrap().name;
                (q.ns.as_ref().map(|s| s.to_string()), q.local.to_string())
            })
            .collect();
        let Some(content) = &ct.content else {
            if let Some(&first) = elems.first() {
                let n = qname_display(&a.element(first).unwrap().name);
                self.err(
                    first,
                    E_CONTENT,
                    format!("Element '{n}': This element is not expected."),
                );
            }
            return;
        };
        if !self
            .matches(content, &names, 0, false)
            .contains(&names.len())
        {
            // The first child that no longer fits, or the missing ones.
            let mut cands = Vec::new();
            self.candidates(content, &mut cands);
            let expected = |v: &Self, prefix: &[Name]| -> Vec<String> {
                let mut out: Vec<String> = Vec::new();
                for c in &cands {
                    let mut t = prefix.to_vec();
                    t.push(c.clone());
                    let s = name_display(c);
                    if v.matches(content, &t, 0, true).contains(&t.len()) && !out.contains(&s) {
                        out.push(s);
                    }
                }
                out
            };
            let bad = (0..names.len()).find(|&i| {
                !self
                    .matches(content, &names[..=i], 0, true)
                    .contains(&(i + 1))
            });
            match bad {
                Some(i) => {
                    let exp = expected(self, &names[..i]);
                    let n = qname_display(&a.element(elems[i]).unwrap().name);
                    let msg = if exp.is_empty() {
                        format!("Element '{n}': This element is not expected.")
                    } else {
                        format!(
                            "Element '{n}': This element is not expected. Expected is ( {} ).",
                            exp.join(", ")
                        )
                    };
                    self.err(elems[i], E_CONTENT, msg);
                }
                None => {
                    let exp = expected(self, &names);
                    self.err(
                        el,
                        E_CONTENT,
                        format!(
                            "Element '{ename}': Missing child element(s). Expected is ( {} ).",
                            exp.join(", ")
                        ),
                    );
                }
            }
            return;
        }
        let mut assign: Vec<Option<ElemDecl>> = vec![None; names.len()];
        self.assign_full(content, &names, 0, names.len(), &mut assign);
        for (i, &c) in elems.iter().enumerate() {
            if let Some(d) = assign[i].clone() {
                self.element(a, c, &d);
            }
        }
    }

    /// The element names a particle can contain, in order.
    fn candidates(&self, p: &Particle, out: &mut Vec<Name>) {
        match p {
            Particle::Element(d) => {
                let n = self.decl_name(d).clone();
                if !out.contains(&n) {
                    out.push(n);
                }
            }
            Particle::Seq(v, ..) | Particle::Choice(v, ..) | Particle::All(v, ..) => {
                for x in v {
                    self.candidates(x, out);
                }
            }
            Particle::Any(..) => {}
        }
    }

    fn decl_name<'b>(&'b self, d: &'b ElemDecl) -> &'b Name {
        d.reference.as_ref().unwrap_or(&d.name)
    }

    fn occurs_of(p: &Particle) -> (u32, Option<u32>) {
        match p {
            Particle::Element(d) => (d.min, d.max),
            Particle::Seq(_, a, b) | Particle::Choice(_, a, b) | Particle::All(_, a, b) => (*a, *b),
            Particle::Any(a, b) => (*a, *b),
        }
    }

    /// The positions reachable by matching `p` (with its occurrence
    /// bounds) from `at`. With `prefix`, running out of input counts as a
    /// match (to test whether `names` can start a valid content).
    fn matches(&self, p: &Particle, names: &[Name], at: usize, prefix: bool) -> Vec<usize> {
        if prefix && at == names.len() {
            return vec![at];
        }
        let (min, max) = Self::occurs_of(p);
        let mut out = Vec::new();
        let mut frontier = vec![at];
        let mut count = 0u32;
        if min == 0 {
            out.push(at);
        }
        while !frontier.is_empty() && !max.is_some_and(|m| count >= m) {
            let mut next = Vec::new();
            for &f in &frontier {
                for e in self.match_once(p, names, f, prefix) {
                    // Out of input in prefix mode, or an empty match (which
                    // can repeat up to any minimum): done along this path.
                    if (prefix && e == names.len()) || e == f {
                        if !out.contains(&e) {
                            out.push(e);
                        }
                    } else if e > f && !next.contains(&e) {
                        next.push(e);
                    }
                }
            }
            count += 1;
            if count >= min {
                for &e in &next {
                    if !out.contains(&e) {
                        out.push(e);
                    }
                }
            }
            frontier = next;
            if count as usize > names.len() + 1 {
                break;
            }
        }
        out
    }

    fn match_once(&self, p: &Particle, names: &[Name], at: usize, prefix: bool) -> Vec<usize> {
        if prefix && at == names.len() {
            return vec![at];
        }
        match p {
            Particle::Element(d) => {
                if at < names.len() && names[at] == *self.decl_name(d) {
                    vec![at + 1]
                } else {
                    Vec::new()
                }
            }
            Particle::Any(..) => {
                if at < names.len() {
                    vec![at + 1]
                } else {
                    Vec::new()
                }
            }
            Particle::Seq(v, ..) => {
                let mut cur = vec![at];
                for x in v {
                    let mut next = Vec::new();
                    for &c in &cur {
                        for e in self.matches(x, names, c, prefix) {
                            if !next.contains(&e) {
                                next.push(e);
                            }
                        }
                    }
                    cur = next;
                    if cur.is_empty() {
                        break;
                    }
                }
                cur
            }
            Particle::Choice(v, ..) => {
                let mut out = Vec::new();
                for x in v {
                    for e in self.matches(x, names, at, prefix) {
                        if !out.contains(&e) {
                            out.push(e);
                        }
                    }
                }
                out
            }
            Particle::All(v, ..) => {
                // Each member at most once, in any order.
                let mut used = vec![false; v.len()];
                let mut pos = at;
                loop {
                    if prefix && pos == names.len() {
                        return vec![pos];
                    }
                    let mut progressed = false;
                    for (i, x) in v.iter().enumerate() {
                        if used[i] {
                            continue;
                        }
                        if let Some(&e) = self.match_once(x, names, pos, false).iter().max() {
                            used[i] = true;
                            pos = e;
                            progressed = true;
                        }
                    }
                    if !progressed {
                        break;
                    }
                }
                let complete = v
                    .iter()
                    .enumerate()
                    .all(|(i, x)| used[i] || Self::occurs_of(x).0 == 0);
                if complete { vec![pos] } else { Vec::new() }
            }
        }
    }

    /// Assigns declarations to the children along a match of `p` from
    /// `at` to `end`.
    fn assign_full(
        &self,
        p: &Particle,
        names: &[Name],
        at: usize,
        end: usize,
        assign: &mut Vec<Option<ElemDecl>>,
    ) -> bool {
        let (min, max) = Self::occurs_of(p);
        self.assign_rep(p, names, (at, end), (min, max), 0, assign)
    }

    fn assign_rep(
        &self,
        p: &Particle,
        names: &[Name],
        (at, end): (usize, usize),
        (min, max): (u32, Option<u32>),
        count: u32,
        assign: &mut Vec<Option<ElemDecl>>,
    ) -> bool {
        if at == end && count >= min {
            return true;
        }
        if max.is_some_and(|m| count >= m) {
            return false;
        }
        for e in self.match_once(p, names, at, false).into_iter().rev() {
            if e <= at || e > end {
                continue;
            }
            let mut trial = assign.clone();
            if self.assign_once(p, names, at, e, &mut trial)
                && self.assign_rep(p, names, (e, end), (min, max), count + 1, &mut trial)
            {
                *assign = trial;
                return true;
            }
        }
        // Optional rest: zero further repetitions from here.
        at == end && count >= min
    }

    fn assign_once(
        &self,
        p: &Particle,
        names: &[Name],
        at: usize,
        end: usize,
        assign: &mut Vec<Option<ElemDecl>>,
    ) -> bool {
        match p {
            Particle::Element(d) => {
                if end == at + 1 && names[at] == *self.decl_name(d) {
                    assign[at] = Some(d.clone());
                    true
                } else {
                    false
                }
            }
            Particle::Any(..) => end == at + 1,
            Particle::Seq(v, ..) => self.assign_seq(v, names, at, end, assign),
            Particle::Choice(v, ..) => v.iter().any(|x| {
                let mut t = assign.clone();
                if self.assign_full(x, names, at, end, &mut t) {
                    *assign = t;
                    true
                } else {
                    false
                }
            }),
            Particle::All(v, ..) => {
                for i in at..end {
                    for x in v {
                        if let Particle::Element(d) = x
                            && names[i] == *self.decl_name(d)
                        {
                            assign[i] = Some(d.clone());
                        }
                    }
                }
                true
            }
        }
    }

    fn assign_seq(
        &self,
        v: &[Particle],
        names: &[Name],
        at: usize,
        end: usize,
        assign: &mut Vec<Option<ElemDecl>>,
    ) -> bool {
        let Some((first, rest)) = v.split_first() else {
            return at == end;
        };
        for e in self.matches(first, names, at, false).into_iter().rev() {
            if e > end {
                continue;
            }
            let mut t = assign.clone();
            if self.assign_full(first, names, at, e, &mut t)
                && self.assign_seq(rest, names, e, end, &mut t)
            {
                *assign = t;
                return true;
            }
        }
        false
    }
}

impl Schema {
    /// Validates document `doc`; with `create`, defaulted attributes are
    /// added (LIBXML_SCHEMA_CREATE).
    pub fn validate(&self, a: &mut Arena, doc: NodeId, create: bool) -> Vec<Invalid> {
        let Some(root) = a.document_element(doc) else {
            return vec![Invalid {
                node: doc,
                code: E_ELT_1,
                message: "The document has no document element.".into(),
            }];
        };
        self.validate_root(a, root, create)
    }

    /// Validates the subtree of element `root` as a validation root.
    pub fn validate_root(&self, a: &mut Arena, root: NodeId, create: bool) -> Vec<Invalid> {
        let q = a.element(root).unwrap().name.clone();
        let key = (q.ns.as_ref().map(|s| s.to_string()), q.local.to_string());
        let mut v = V {
            schema: self,
            errors: Vec::new(),
            create,
        };
        match self.elements.get(&key) {
            Some(d) => {
                let d = d.clone();
                v.element(a, root, &d);
            }
            None => v.err(
                root,
                E_ELT_1,
                format!(
                    "Element '{}': No matching global declaration available for the validation root.",
                    qname_display(&q)
                ),
            ),
        }
        v.errors
    }
}
