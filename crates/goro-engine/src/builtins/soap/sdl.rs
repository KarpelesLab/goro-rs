//! The service description ext/soap builds from a WSDL 1.1 document (W3C
//! Note: WSDL 1.1, with its SOAP 1.1/1.2 bindings) and the XML Schemas in
//! it (XML Schema Part 1 structures, the SOAP 1.1 section 5 and SOAP 1.2
//! part 2 array conventions): the types, elements and attributes the
//! encoder uses, and the operations of the SOAP ports.
//!
//! What PHP keeps and how it names things (anonymous types named after
//! their element or `anonymousN`, the order `__getTypes()` lists them in,
//! components looked up by local name, the error messages) follows the
//! ext/soap tests' expected output and observations on PHP 8.5.

use std::collections::HashMap;

use goro_xml::NodeId;

use super::types::{self, Bi};
use super::xmlin::InDoc;
use super::xmlout::{
    NS_SOAP11_ENC, NS_SOAP12_ENC, NS_WSDL, NS_WSDL_SOAP11, NS_WSDL_SOAP12, NS_XML, NS_XSD,
};

pub type TId = usize;

/// A reference to a type.
#[derive(Clone, Debug, PartialEq)]
pub enum TRef {
    /// A qualified name, looked up when used.
    Q(String, String),
    /// An anonymous (or already resolved) type.
    Id(TId),
}

/// What a type reference stands for.
#[derive(Clone, Copy, Debug)]
pub enum Enc {
    Builtin(Bi),
    Type(TId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// simpleType/restriction
    Simple,
    List,
    Union,
    Complex,
    /// An element declaration.
    Element,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deriv {
    None,
    SimpleRestriction,
    SimpleExtension,
    ComplexRestriction,
    ComplexExtension,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MKind {
    Sequence,
    All,
    Choice,
    Group,
    Element,
    Any,
}

/// A content model particle.
#[derive(Clone, Debug)]
pub struct Model {
    pub kind: MKind,
    pub min: i64,
    /// -1: unbounded.
    pub max: i64,
    pub items: Vec<Model>,
    /// The element declaration (MKind::Element).
    pub elem: Option<TId>,
    /// The group referenced (MKind::Group): its (ns, name).
    pub group: Option<(String, String)>,
}

/// An attribute declaration (or use).
#[derive(Clone, Debug, Default)]
pub struct Attr {
    pub name: String,
    pub ns: String,
    pub qualified: bool,
    pub typ: Option<TRef>,
    pub default: Option<String>,
    pub fixed: Option<String>,
    pub required: bool,
    /// `ref=` (resolved when used).
    pub r#ref: Option<(String, String)>,
    /// `attributeGroup ref=` placeholder.
    pub group: Option<(String, String)>,
    /// wsdl:arrayType (namespace, type name, dimensions).
    pub array_type: Option<(String, String, String)>,
    /// wsdl:itemType (SOAP 1.2 arrays).
    pub item_type: Option<(String, String)>,
    /// wsdl:arraySize (SOAP 1.2 arrays).
    pub array_size: Option<String>,
}

/// A type (or element declaration).
#[derive(Clone, Debug)]
pub struct SType {
    pub kind: Kind,
    pub name: String,
    pub ns: String,
    /// The base type (simple restriction, derivations), or the element's
    /// type.
    pub base: Option<TRef>,
    pub deriv: Deriv,
    pub model: Option<Model>,
    pub attrs: Vec<Attr>,
    pub nillable: bool,
    pub min: i64,
    pub max: i64,
    pub default: Option<String>,
    pub fixed: Option<String>,
    /// Elements: namespace-qualified in instances.
    pub qualified: bool,
    pub enums: Vec<String>,
    pub list_item: Option<TRef>,
    pub union: Vec<TRef>,
    /// For element refs: the global element.
    pub r#ref: Option<(String, String)>,
    /// A global component (named type or global element).
    pub global: bool,
    /// Created for an anonymous definition.
    pub anonymous: bool,
    /// The type was named after its element (anonymous complex/simple
    /// type of an element).
    pub from_element: bool,
    pub mixed: bool,
    /// The SOAP-ENC Array derivation (SOAP 1.1 or 1.2 Array base).
    pub soap_array: bool,
}

impl SType {
    fn new(kind: Kind, name: &str, ns: &str) -> SType {
        SType {
            kind,
            name: name.to_string(),
            ns: ns.to_string(),
            base: None,
            deriv: Deriv::None,
            model: None,
            attrs: Vec::new(),
            nillable: false,
            min: 1,
            max: 1,
            default: None,
            fixed: None,
            qualified: false,
            enums: Vec::new(),
            list_item: None,
            union: Vec::new(),
            r#ref: None,
            global: false,
            anonymous: false,
            from_element: false,
            mixed: false,
            soap_array: false,
        }
    }
}

/// A message part.
#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    /// `element=`
    pub element: Option<(String, String)>,
    /// `type=`
    pub typ: Option<TRef>,
}

/// The input or output of an operation as bound.
#[derive(Clone, Debug, Default)]
pub struct Body {
    pub literal: bool,
    pub ns: String,
    pub encoding_style: Option<String>,
    pub parts: Vec<Part>,
    /// The `name` of the portType input/output, if any.
    pub name: Option<String>,
    pub headers: Vec<Header>,
}

/// A soap:header binding.
#[derive(Clone, Debug)]
pub struct Header {
    pub name: String,
    pub ns: String,
    pub literal: bool,
    pub encoding_style: Option<String>,
    pub part: Part,
    pub faults: Vec<Header>,
}

/// A wsdl:fault of an operation.
#[derive(Clone, Debug)]
pub struct FaultDef {
    pub name: String,
    pub literal: bool,
    pub ns: String,
    pub parts: Vec<Part>,
}

/// A bound operation.
#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub rpc: bool,
    pub soap_action: Option<String>,
    pub input: Body,
    pub output: Option<Body>,
    pub faults: Vec<FaultDef>,
    pub version: i64,
    pub location: String,
}

/// A parsed WSDL.
#[derive(Default)]
pub struct Sdl {
    pub types: Vec<SType>,
    /// Types in the order __getTypes() lists them.
    pub listed: Vec<TId>,
    pub named: HashMap<(String, String), TId>,
    pub elements: HashMap<(String, String), TId>,
    pub attributes: HashMap<(String, String), Attr>,
    pub attr_groups: HashMap<(String, String), Vec<Attr>>,
    pub groups: HashMap<(String, String), Model>,
    pub functions: Vec<Function>,
    pub source: String,
    pub target_ns: String,
    anon: u32,
}

/// Loads the bytes of a WSDL or schema URL: Ok((bytes, resolved url)).
pub type Loader<'a> = dyn FnMut(&str) -> Result<Vec<u8>, String> + 'a;

struct Ctx<'a, 'b> {
    sdl: Sdl,
    load: &'a mut Loader<'b>,
    docs: Vec<InDoc>,
    urls: Vec<String>,
    /// Schema locations loaded.
    schemas: Vec<String>,
    messages: HashMap<String, (usize, NodeId)>,
    port_types: HashMap<String, (usize, NodeId)>,
    bindings: HashMap<String, (usize, NodeId)>,
    services: Vec<(usize, NodeId)>,
}

fn err<T>(m: impl Into<String>) -> Result<T, String> {
    Err(format!("SOAP-ERROR: Parsing WSDL: {}", m.into()))
}

fn schema_err<T>(m: impl Into<String>) -> Result<T, String> {
    Err(format!("SOAP-ERROR: Parsing Schema: {}", m.into()))
}

/// A reference's name without its prefix (only the first one: names may
/// contain colons; observed on PHP 8.5; test gh8538.phpt).
fn local_part(q: &str) -> &str {
    match q.find(':') {
        Some(i) => &q[i + 1..],
        None => q,
    }
}

/// Resolves `base` against a relative reference (RFC 3986 section 5,
/// for file paths and URLs).
pub fn resolve_url(base: &str, rel: &str) -> String {
    if rel.contains("://") || rel.starts_with('/') || base.is_empty() {
        return rel.to_string();
    }
    match base.rfind('/') {
        Some(i) => format!("{}{}", &base[..=i], rel),
        None => rel.to_string(),
    }
}

fn occurs(v: Option<String>, d: i64) -> i64 {
    match v {
        None => d,
        Some(s) => {
            let s = s.trim();
            if s == "unbounded" {
                -1
            } else {
                s.parse().unwrap_or(d)
            }
        }
    }
}

impl Sdl {
    /// Loads and parses the WSDL at `url`.
    pub fn load(url: &str, load: &mut Loader) -> Result<Sdl, String> {
        let mut ctx = Ctx {
            sdl: Sdl {
                source: url.to_string(),
                ..Default::default()
            },
            load,
            docs: Vec::new(),
            urls: Vec::new(),
            schemas: Vec::new(),
            messages: HashMap::new(),
            port_types: HashMap::new(),
            bindings: HashMap::new(),
            services: Vec::new(),
        };
        ctx.load_wsdl(url, true)?;
        ctx.bind()?;
        Ok(ctx.sdl)
    }

    /// Parses a standalone schema document into this description (used by
    /// nothing but tests of the parser).
    pub fn ty(&self, id: TId) -> &SType {
        &self.types[id]
    }

    fn add(&mut self, t: SType) -> TId {
        self.types.push(t);
        self.types.len() - 1
    }

    /// Resolves a type reference.
    pub fn resolve(&self, r: &TRef) -> Option<Enc> {
        match r {
            TRef::Id(i) => Some(Enc::Type(*i)),
            TRef::Q(ns, name) => {
                if let Some(&i) = self.named.get(&(ns.clone(), name.clone())) {
                    return Some(Enc::Type(i));
                }
                types::by_name(ns, name).map(Enc::Builtin)
            }
        }
    }

    /// Finds a named type by namespace and name.
    pub fn find_type(&self, ns: &str, name: &str) -> Option<Enc> {
        self.resolve(&TRef::Q(ns.to_string(), name.to_string()))
    }

    pub fn element(&self, ns: &str, name: &str) -> Option<TId> {
        self.elements
            .get(&(ns.to_string(), name.to_string()))
            .copied()
    }

    /// The element declaration an element (or element ref) stands for.
    pub fn deref_element(&self, id: TId) -> TId {
        let mut id = id;
        for _ in 0..16 {
            let t = &self.types[id];
            match &t.r#ref {
                Some((ns, n)) => match self.element(ns, n) {
                    Some(g) if g != id => id = g,
                    _ => break,
                },
                None => break,
            }
        }
        id
    }

    /// The attributes of a type, attribute groups and refs expanded.
    pub fn attrs_of(&self, id: TId) -> Vec<Attr> {
        let mut out = Vec::new();
        self.expand_attrs(&self.types[id].attrs, &mut out, 0);
        out
    }

    fn expand_attrs(&self, attrs: &[Attr], out: &mut Vec<Attr>, depth: u32) {
        if depth > 16 {
            return;
        }
        for a in attrs {
            if let Some(g) = &a.group {
                if let Some(ga) = self.attr_groups.get(g) {
                    self.expand_attrs(ga, out, depth + 1);
                }
                continue;
            }
            if let Some(r) = &a.r#ref {
                let mut na = match self.attributes.get(r) {
                    Some(g) => g.clone(),
                    None => Attr {
                        name: r.1.clone(),
                        ns: r.0.clone(),
                        ..Default::default()
                    },
                };
                if r.0 == NS_XML {
                    na.ns = NS_XML.to_string();
                    na.qualified = true;
                }
                if a.default.is_some() {
                    na.default = a.default.clone();
                }
                if a.fixed.is_some() {
                    na.fixed = a.fixed.clone();
                }
                na.array_type = a.array_type.clone().or(na.array_type);
                na.item_type = a.item_type.clone().or(na.item_type);
                na.array_size = a.array_size.clone().or(na.array_size);
                na.r#ref = Some(r.clone());
                out.push(na);
                continue;
            }
            out.push(a.clone());
        }
    }

    /// The simple type a simple-content complex type's value has (its
    /// bases followed through complex simple-content types).
    pub fn simple_base(&self, t: TId) -> Option<Enc> {
        let mut id = t;
        for _ in 0..16 {
            let ty = &self.types[id];
            match ty.base.as_ref().and_then(|b| self.resolve(b)) {
                Some(Enc::Type(b)) if b != id && self.types[b].kind == Kind::Complex => id = b,
                other => return other,
            }
        }
        None
    }

    /// A model group's particle (`group ref=`).
    pub fn group(&self, q: &(String, String)) -> Option<&Model> {
        self.groups.get(q)
    }

    pub fn find_function(&self, name: &str) -> Option<&Function> {
        self.functions
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name))
            .or_else(|| self.functions.iter().find(|f| f.name == name))
    }
}

const XSD: &str = NS_XSD;

impl<'a, 'b> Ctx<'a, 'b> {
    fn d(&self, i: usize) -> &InDoc {
        &self.docs[i]
    }

    fn load_doc(&mut self, url: &str) -> Result<usize, String> {
        let bytes = (self.load)(url)?;
        let doc = match InDoc::parse(&bytes, Some(url)) {
            Ok(d) => d,
            Err(_) => return err(format!("Couldn't load from '{url}'")),
        };
        self.docs.push(doc);
        self.urls.push(url.to_string());
        Ok(self.docs.len() - 1)
    }

    fn load_wsdl(&mut self, url: &str, top: bool) -> Result<(), String> {
        if self.urls.iter().any(|u| u == url) {
            return Ok(());
        }
        let di = match self.load_doc(url) {
            Ok(d) => d,
            Err(e) if e.starts_with("SOAP-ERROR") => return Err(e),
            Err(e) => {
                return err(format!("Couldn't load from '{url}' : {e}"));
            }
        };
        let Some(root) = self.d(di).root() else {
            return err(format!("Couldn't find <definitions> in '{url}'"));
        };
        // wsdl:import of a schema document.
        if !top && self.d(di).is(root, XSD, "schema") {
            return self.load_schema(di, root);
        }
        if !self.d(di).is(root, NS_WSDL, "definitions") {
            return err(format!("Couldn't find <definitions> in '{url}'"));
        }
        if top {
            self.sdl.target_ns = self.d(di).attr(root, "targetNamespace").unwrap_or_default();
        }
        // Character data in <definitions> (observed on PHP 8.5; test
        // bugs/bug80672.phpt).
        for c in self.d(di).all_kids(root) {
            if self.d(di).arena.node_type(c) == goro_xml::NodeType::CData {
                return err("Unexpected WSDL element <>");
            }
        }
        for c in self.d(di).kids(root) {
            let (ns, local) = {
                let d = self.d(di);
                (d.ns(c).to_string(), d.local(c).to_string())
            };
            if ns != NS_WSDL {
                // Extensibility elements: a required unknown one is an error.
                let req = self.d(di).attr_ns(c, NS_WSDL, "required");
                if req.as_deref() == Some("true") || req.as_deref() == Some("1") {
                    return err(format!("Unknown required WSDL extension '{ns}'"));
                }
                continue;
            }
            match local.as_str() {
                "types" => {
                    for s in self.d(di).kids(c) {
                        if self.d(di).is(s, XSD, "schema") {
                            self.load_schema(di, s)?;
                        }
                    }
                }
                "import" => {
                    if let Some(loc) = self.d(di).attr(c, "location") {
                        let u = resolve_url(url, &loc);
                        self.load_wsdl(&u, false)?;
                    }
                }
                "message" => {
                    let Some(name) = self.d(di).attr(c, "name") else {
                        return err("No name associated with <message>");
                    };
                    if self.messages.contains_key(&name) {
                        return err(format!("<message> '{name}' already defined"));
                    }
                    self.messages.insert(name, (di, c));
                }
                "portType" => {
                    let Some(name) = self.d(di).attr(c, "name") else {
                        return err("No name associated with <portType>");
                    };
                    if self.port_types.contains_key(&name) {
                        return err(format!("<portType> '{name}' already defined"));
                    }
                    self.port_types.insert(name, (di, c));
                }
                "binding" => {
                    let Some(name) = self.d(di).attr(c, "name") else {
                        return err("No name associated with <binding>");
                    };
                    if self.bindings.contains_key(&name) {
                        return err(format!("<binding> '{name}' already defined"));
                    }
                    self.bindings.insert(name, (di, c));
                }
                "service" => {
                    self.services.push((di, c));
                }
                "documentation" => {}
                other => {
                    return err(format!("Unexpected WSDL element <{other}>"));
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------- binding

    fn bind(&mut self) -> Result<(), String> {
        let mut found_port = false;
        let mut any_service = false;
        let services = self.services.clone();
        for (di, svc) in services {
            any_service = true;
            for port in self.d(di).kids(svc) {
                if !self.d(di).is(port, NS_WSDL, "port") {
                    continue;
                }
                // The address (any namespace: soap, soap12, http...).
                let mut addr = None;
                let mut version = 0;
                for a in self.d(di).kids(port) {
                    let d = self.d(di);
                    if d.local(a) == "address" {
                        // A SOAP address wins over others.
                        if d.ns(a) == NS_WSDL_SOAP11 {
                            version = 1;
                            addr = Some(a);
                            break;
                        } else if d.ns(a) == NS_WSDL_SOAP12 {
                            version = 2;
                            addr = Some(a);
                            break;
                        }
                        if addr.is_none() {
                            addr = Some(a);
                        }
                    }
                }
                let Some(addr) = addr else {
                    continue;
                };
                self.check_required(di, port)?;
                let Some(location) = self.d(di).attr(addr, "location") else {
                    return err("No location associated with <port>");
                };
                let Some(bname) = self.d(di).attr(port, "binding") else {
                    return err("No binding associated with <port>");
                };
                let bname = local_part(&bname).to_string();
                let Some(&(bdi, bnode)) = self.bindings.get(&bname) else {
                    return err(format!("No <binding> element with name '{bname}'"));
                };
                // A SOAP binding (its version from the address, else from
                // the soap:binding element); other transports are skipped
                // (observed on PHP 8.5; tests bugs/bug50698_*.phpt).
                let mut style_rpc = false;
                let mut http_binding = false;
                let mut soap_binding = None;
                for b in self.d(bdi).kids(bnode) {
                    let d = self.d(bdi);
                    if d.local(b) == "binding"
                        && (d.ns(b) == NS_WSDL_SOAP11 || d.ns(b) == NS_WSDL_SOAP12)
                    {
                        soap_binding = Some(b);
                        break;
                    }
                }
                if let Some(b) = soap_binding {
                    let d = self.d(bdi);
                    if version == 0 {
                        // A SOAP binding behind a non-SOAP address (an
                        // HTTP one): its operations are rpc/encoded calls
                        // (observed on PHP 8.5; test bugs/bug47049.phpt).
                        http_binding = true;
                        version = if d.ns(b) == NS_WSDL_SOAP12 { 2 } else { 1 };
                    }
                    style_rpc = d.attr(b, "style").as_deref() == Some("rpc");
                    if let Some(t) = d.attr(b, "transport")
                        && t != "http://schemas.xmlsoap.org/soap/http"
                    {
                        continue;
                    }
                }
                if version == 0 {
                    continue;
                }
                let soap_ns = if version == 2 {
                    NS_WSDL_SOAP12
                } else {
                    NS_WSDL_SOAP11
                };
                found_port = true;
                let first = self.sdl.functions.len();
                self.bind_operations(bdi, bnode, soap_ns, style_rpc, version, &location)?;
                if http_binding {
                    let tns = self.sdl.target_ns.clone();
                    for f in &mut self.sdl.functions[first..] {
                        f.rpc = true;
                        f.input.literal = false;
                        f.input.ns = tns.clone();
                        if let Some(o) = &mut f.output {
                            o.literal = false;
                            o.ns = tns.clone();
                        }
                    }
                }
            }
        }
        if !found_port {
            if any_service {
                return err("Could not find any usable binding services in WSDL.");
            }
            return err("Couldn't bind to service");
        }
        Ok(())
    }

    /// Extensibility elements marked wsdl:required in namespaces PHP
    /// doesn't know are errors (WSDL 1.1 section 2.1.3).
    fn check_required(&self, di: usize, n: NodeId) -> Result<(), String> {
        for c in self.d(di).kids(n) {
            let d = self.d(di);
            let ns = d.ns(c);
            if [NS_WSDL, NS_WSDL_SOAP11, NS_WSDL_SOAP12, super::xmlout::NS_WSDL_HTTP, super::xmlout::NS_WSDL_MIME, XSD]
                .contains(&ns)
            {
                continue;
            }
            let req = d.attr_ns(c, NS_WSDL, "required");
            if matches!(req.as_deref(), Some("true") | Some("1")) {
                return err(format!("Unknown required WSDL extension '{ns}'"));
            }
        }
        Ok(())
    }

    fn bind_operations(
        &mut self,
        bdi: usize,
        bnode: NodeId,
        soap_ns: &str,
        default_rpc: bool,
        version: i64,
        location: &str,
    ) -> Result<(), String> {
        self.check_required(bdi, bnode)?;
        let Some(pt_name) = self.d(bdi).attr(bnode, "type") else {
            return err("Missing 'type' attribute in <binding>");
        };
        let pt_name = local_part(&pt_name).to_string();
        let Some(&(pdi, pnode)) = self.port_types.get(&pt_name) else {
            return err(format!("Missing <portType> with name '{pt_name}'"));
        };
        for op in self.d(bdi).kids(bnode) {
            if !self.d(bdi).is(op, NS_WSDL, "operation") {
                continue;
            }
            let Some(name) = self.d(bdi).attr(op, "name") else {
                return err("Missing 'name' attribute for <operation>");
            };
            if self.sdl.functions.iter().any(|f| f.name == name) {
                continue;
            }
            // The portType operation.
            let mut pt_op = None;
            for o in self.d(pdi).kids(pnode) {
                if self.d(pdi).is(o, NS_WSDL, "operation")
                    && self.d(pdi).attr(o, "name").as_deref() == Some(&name)
                {
                    pt_op = Some(o);
                    break;
                }
            }
            let Some(pt_op) = pt_op else {
                return err(format!("Missing <portType>/<operation> with name '{name}'"));
            };
            let mut rpc = default_rpc;
            let mut soap_action = None;
            for c in self.d(bdi).kids(op) {
                if self.d(bdi).is(c, soap_ns, "operation") {
                    if let Some(s) = self.d(bdi).attr(c, "style") {
                        rpc = s == "rpc";
                    }
                    soap_action = self.d(bdi).attr(c, "soapAction");
                }
            }
            // Input and output.
            let mut input = None;
            let mut output = None;
            let mut pt_faults = Vec::new();
            for c in self.d(pdi).kids(pt_op) {
                let d = self.d(pdi);
                if d.ns(c) != NS_WSDL {
                    continue;
                }
                match d.local(c) {
                    "input" => input = Some(c),
                    "output" => output = Some(c),
                    "fault" => pt_faults.push(c),
                    _ => {}
                }
            }
            let Some(input) = input else {
                // Notification operations are not supported.
                continue;
            };
            let mut b_in = None;
            let mut b_out = None;
            let mut b_faults = Vec::new();
            for c in self.d(bdi).kids(op) {
                let d = self.d(bdi);
                if d.ns(c) != NS_WSDL {
                    continue;
                }
                match d.local(c) {
                    "input" => b_in = Some(c),
                    "output" => b_out = Some(c),
                    "fault" => b_faults.push(c),
                    _ => {}
                }
            }
            let in_body = self.body(pdi, input, bdi, b_in, soap_ns)?;
            let out_body = match output {
                Some(o) => Some(self.body(pdi, o, bdi, b_out, soap_ns)?),
                None => None,
            };
            let mut faults = Vec::new();
            for f in pt_faults {
                let fname = self.d(pdi).attr(f, "name").unwrap_or_default();
                let parts = match self.d(pdi).attr(f, "message") {
                    Some(m) => self.message_parts(&m)?,
                    None => Vec::new(),
                };
                let mut literal = true;
                let mut ns = String::new();
                for bf in &b_faults {
                    if self.d(bdi).attr(*bf, "name").as_deref() == Some(&fname) {
                        for sf in self.d(bdi).kids(*bf) {
                            if self.d(bdi).is(sf, soap_ns, "fault") {
                                literal =
                                    self.d(bdi).attr(sf, "use").as_deref() != Some("encoded");
                                ns = self.d(bdi).attr(sf, "namespace").unwrap_or_default();
                            }
                        }
                    }
                }
                faults.push(FaultDef {
                    name: fname,
                    literal,
                    ns,
                    parts,
                });
            }
            self.sdl.functions.push(Function {
                name,
                rpc,
                soap_action,
                input: in_body,
                output: out_body,
                faults,
                version,
                location: location.to_string(),
            });
        }
        Ok(())
    }

    fn message_parts(&mut self, msg: &str) -> Result<Vec<Part>, String> {
        let m = local_part(msg).to_string();
        let Some(&(mdi, mnode)) = self.messages.get(&m) else {
            return err(format!("Missing <message> with name '{m}'"));
        };
        let mut parts = Vec::new();
        for p in self.d(mdi).kids(mnode) {
            if !self.d(mdi).is(p, NS_WSDL, "part") {
                continue;
            }
            let d = self.d(mdi);
            let Some(name) = d.attr(p, "name") else {
                return err("No name associated with <part> '{}'");
            };
            let element = d
                .attr(p, "element")
                .and_then(|e| d.resolve_qname(p, &e));
            let typ = d
                .attr(p, "type")
                .and_then(|t| d.resolve_qname(p, &t))
                .map(|(n, l)| TRef::Q(n, l));
            parts.push(Part { name, element, typ });
        }
        Ok(parts)
    }

    fn body(
        &mut self,
        pdi: usize,
        pt_io: NodeId,
        bdi: usize,
        b_io: Option<NodeId>,
        soap_ns: &str,
    ) -> Result<Body, String> {
        let msg = self.d(pdi).attr(pt_io, "message");
        let Some(msg) = msg else {
            return err("Missing name for <input> of '{}'");
        };
        let mut parts = self.message_parts(&msg)?;
        let mut body = Body {
            literal: true,
            name: self.d(pdi).attr(pt_io, "name"),
            ..Default::default()
        };
        let Some(b_io) = b_io else {
            body.parts = parts;
            return Ok(body);
        };
        for c in self.d(bdi).kids(b_io) {
            let d = self.d(bdi);
            if d.ns(c) != soap_ns {
                continue;
            }
            match d.local(c) {
                "body" => {
                    body.literal = d.attr(c, "use").as_deref() != Some("encoded");
                    body.ns = d.attr(c, "namespace").unwrap_or_default();
                    body.encoding_style = d.attr(c, "encodingStyle");
                    if let Some(ps) = d.attr(c, "parts") {
                        let names: Vec<&str> = ps.split_whitespace().collect();
                        let mut sel = Vec::new();
                        for n in names {
                            match parts.iter().position(|p| p.name == n) {
                                Some(i) => sel.push(parts[i].clone()),
                                None => return err(format!("Missing part '{n}' in <message>")),
                            }
                        }
                        parts = sel;
                    }
                }
                "header" => {
                    let h = self.header(bdi, c)?;
                    body.headers.push(h);
                }
                _ => {}
            }
        }
        body.parts = parts;
        Ok(body)
    }

    fn header(&mut self, bdi: usize, h: NodeId) -> Result<Header, String> {
        let (msg, part, literal, ns, es) = {
            let d = self.d(bdi);
            (
                d.attr(h, "message"),
                d.attr(h, "part"),
                d.attr(h, "use").as_deref() != Some("encoded"),
                d.attr(h, "namespace").unwrap_or_default(),
                d.attr(h, "encodingStyle"),
            )
        };
        let Some(msg) = msg else {
            return err("Missing message attribute for <header>");
        };
        let Some(part) = part else {
            return err("Missing part attribute for <header>");
        };
        let parts = self.message_parts(&msg)?;
        let Some(p) = parts.into_iter().find(|p| p.name == part) else {
            return err(format!("Missing part '{part}' in <message>"));
        };
        let (name, hns) = match &p.element {
            Some((ens, en)) => (en.clone(), ens.clone()),
            None => (p.name.clone(), ns.clone()),
        };
        let mut faults = Vec::new();
        for f in self.d(bdi).kids(h) {
            if self.d(bdi).local(f) == "headerfault" {
                faults.push(self.header(bdi, f)?);
            }
        }
        Ok(Header {
            name,
            ns: hns,
            literal,
            encoding_style: es,
            part: p,
            faults,
        })
    }

    // -------------------------------------------------------------- schema

    fn load_schema(&mut self, di: usize, s: NodeId) -> Result<(), String> {
        let tns = self.d(di).attr(s, "targetNamespace").unwrap_or_default();
        self.schema_body(di, s, &tns)
    }

    fn schema_body(&mut self, di: usize, s: NodeId, tns: &str) -> Result<(), String> {
        let efd = self.d(di).attr(s, "elementFormDefault").as_deref() == Some("qualified");
        let afd = self.d(di).attr(s, "attributeFormDefault").as_deref() == Some("qualified");
        let sc = SchemaCtx {
            di,
            tns: tns.to_string(),
            efd,
            afd,
        };
        let base_url = self.urls[di].clone();
        for c in self.d(di).kids(s) {
            if self.d(di).ns(c) != XSD {
                continue;
            }
            let local = self.d(di).local(c).to_string();
            match local.as_str() {
                "import" => {
                    let ns = self.d(di).attr(c, "namespace");
                    if let Some(loc) = self.d(di).attr(c, "schemaLocation") {
                        if ns.as_deref() == Some(tns) && !tns.is_empty() {
                            return schema_err(format!(
                                "can't import schema from '{}', namespace must not match the enclosing schema 'targetNamespace'",
                                resolve_url(&base_url, &loc)
                            ));
                        }
                        let u = resolve_url(&base_url, &loc);
                        self.import_schema(&u, ns.as_deref(), false)?;
                    }
                }
                "include" | "redefine" => {
                    if let Some(loc) = self.d(di).attr(c, "schemaLocation") {
                        let u = resolve_url(&base_url, &loc);
                        self.include_schema(&u, tns)?;
                    }
                }
                "simpleType" => {
                    self.simple_type(&sc, c, None)?;
                }
                "complexType" => {
                    self.complex_type(&sc, c, None)?;
                }
                "element" => {
                    self.element(&sc, c, true)?;
                }
                "attribute" => {
                    let a = self.attribute(&sc, c, true)?;
                    self.sdl
                        .attributes
                        .insert((tns.to_string(), a.name.clone()), a);
                }
                "attributeGroup" => {
                    let name = self.d(di).attr(c, "name").unwrap_or_default();
                    let attrs = self.attr_list(&sc, c)?;
                    self.sdl.attr_groups.insert((tns.to_string(), name), attrs);
                }
                "group" => {
                    let name = self.d(di).attr(c, "name").unwrap_or_default();
                    for g in self.d(di).kids(c) {
                        let d = self.d(di);
                        if d.ns(g) == XSD && matches!(d.local(g), "sequence" | "all" | "choice") {
                            let m = self.particle(&sc, g)?;
                            self.sdl.groups.insert((tns.to_string(), name.clone()), m);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn import_schema(&mut self, url: &str, ns: Option<&str>, _x: bool) -> Result<(), String> {
        if self.schemas.iter().any(|u| u == url) {
            return Ok(());
        }
        self.schemas.push(url.to_string());
        // A schema that can't be loaded is skipped (observed on PHP 8.5:
        // test bugs/bug29839.phpt imports a missing xml.xsd).
        let di = match self.load_doc(url) {
            Ok(d) => d,
            Err(_) => return Ok(()),
        };
        let Some(root) = self.d(di).root() else {
            return schema_err(format!("can't import schema from '{url}'"));
        };
        if !self.d(di).is(root, XSD, "schema") {
            return schema_err(format!("can't import schema from '{url}'"));
        }
        let tns = self.d(di).attr(root, "targetNamespace");
        match (ns, tns.as_deref()) {
            (Some(n), None) => {
                return schema_err(format!(
                    "can't import schema from '{url}', missing 'targetNamespace', expected '{n}'"
                ));
            }
            (Some(n), Some(t)) if n != t => {
                return schema_err(format!(
                    "can't import schema from '{url}', unexpected 'targetNamespace'='{t}', expected '{n}'"
                ));
            }
            (None, Some(t)) => {
                return schema_err(format!(
                    "can't import schema from '{url}', unexpected 'targetNamespace'='{t}', expected no 'targetNamespace'"
                ));
            }
            _ => {}
        }
        let tns = tns.unwrap_or_default();
        self.schema_body(di, root, &tns)
    }

    fn include_schema(&mut self, url: &str, tns: &str) -> Result<(), String> {
        if self.schemas.iter().any(|u| u == url) {
            return Ok(());
        }
        self.schemas.push(url.to_string());
        let di = match self.load_doc(url) {
            Ok(d) => d,
            Err(_) => return schema_err(format!("can't include schema from '{url}'")),
        };
        let Some(root) = self.d(di).root() else {
            return schema_err(format!("can't include schema from '{url}'"));
        };
        if !self.d(di).is(root, XSD, "schema") {
            return schema_err(format!("can't include schema from '{url}'"));
        }
        let t = self.d(di).attr(root, "targetNamespace");
        let tns = t.unwrap_or_else(|| tns.to_string());
        self.schema_body(di, root, &tns)
    }

    fn qref(&self, di: usize, n: NodeId, v: &str) -> Option<TRef> {
        self.d(di).resolve_qname(n, v).map(|(ns, l)| TRef::Q(ns, l))
    }

    fn anon_name(&mut self) -> String {
        self.sdl.anon += 1;
        format!("anonymous{}", self.sdl.anon)
    }

    /// Registers a type in the listing order (and by name for globals).
    fn register(&mut self, t: SType, global: bool) -> TId {
        let key = (t.ns.clone(), t.name.clone());
        let id = self.sdl.add(t);
        self.sdl.listed.push(id);
        if global && !self.sdl.named.contains_key(&key) {
            self.sdl.named.insert(key, id);
        }
        id
    }

    /// `<simpleType>`; `name` names an anonymous one.
    fn simple_type(
        &mut self,
        sc: &SchemaCtx,
        n: NodeId,
        name: Option<(String, bool)>,
    ) -> Result<TId, String> {
        let di = sc.di;
        let own = self.d(di).attr(n, "name");
        let global = own.is_some() && name.is_none();
        let (tname, from_el) = match (own, name) {
            (_, Some((n, from_el))) => (n, from_el),
            (Some(n), None) => (n, false),
            (None, None) => (self.anon_name(), false),
        };
        let mut t = SType::new(Kind::Simple, &tname, &sc.tns);
        t.global = global;
        t.from_element = from_el;
        let id = self.register(t, global);
        for c in self.d(di).kids(n) {
            if self.d(di).ns(c) != XSD {
                continue;
            }
            let local = self.d(di).local(c).to_string();
            match local.as_str() {
                "restriction" => {
                    if let Some(b) = self.d(di).attr(c, "base") {
                        let r = self.qref(di, c, &b);
                        self.sdl.types[id].base = r;
                    }
                    self.facets(sc, c, id)?;
                }
                "list" => {
                    self.sdl.types[id].kind = Kind::List;
                    if let Some(it) = self.d(di).attr(c, "itemType") {
                        let r = self.qref(di, c, &it);
                        self.sdl.types[id].list_item = r;
                    } else {
                        for st in self.d(di).kids(c) {
                            if self.d(di).is(st, XSD, "simpleType") {
                                let a = self.anon_name();
                                let sid = self.simple_type(sc, st, Some((a, false)))?;
                                self.sdl.types[sid].anonymous = true;
                                self.sdl.types[id].list_item = Some(TRef::Id(sid));
                            }
                        }
                    }
                }
                "union" => {
                    self.sdl.types[id].kind = Kind::Union;
                    if let Some(mt) = self.d(di).attr(c, "memberTypes") {
                        for m in mt.split_whitespace() {
                            if let Some(r) = self.qref(di, c, m) {
                                self.sdl.types[id].union.push(r);
                            }
                        }
                    }
                    for st in self.d(di).kids(c) {
                        if self.d(di).is(st, XSD, "simpleType") {
                            let a = self.anon_name();
                            let sid = self.simple_type(sc, st, Some((a, false)))?;
                            self.sdl.types[sid].anonymous = true;
                            self.sdl.types[id].union.push(TRef::Id(sid));
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(id)
    }

    /// Facets of a restriction (and an inner anonymous simpleType base).
    fn facets(&mut self, sc: &SchemaCtx, r: NodeId, id: TId) -> Result<(), String> {
        let di = sc.di;
        for f in self.d(di).kids(r) {
            if self.d(di).ns(f) != XSD {
                continue;
            }
            let local = self.d(di).local(f).to_string();
            match local.as_str() {
                "enumeration" => {
                    if let Some(v) = self.d(di).attr(f, "value") {
                        self.sdl.types[id].enums.push(v);
                    }
                }
                "simpleType" => {
                    let sid = self.simple_type(sc, f, None)?;
                    if self.sdl.types[id].base.is_none() {
                        self.sdl.types[id].base = Some(TRef::Id(sid));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// `<complexType>`; `name` names an anonymous one (its element).
    fn complex_type(
        &mut self,
        sc: &SchemaCtx,
        n: NodeId,
        name: Option<String>,
    ) -> Result<TId, String> {
        let di = sc.di;
        let own = self.d(di).attr(n, "name");
        let global = own.is_some() && name.is_none();
        let from_el = name.is_some();
        let tname = match (own, name) {
            (_, Some(n)) => n,
            (Some(n), None) => n,
            (None, None) => self.anon_name(),
        };
        let mut t = SType::new(Kind::Complex, &tname, &sc.tns);
        t.global = global;
        t.from_element = from_el;
        t.mixed = matches!(self.d(di).attr(n, "mixed").as_deref(), Some("true") | Some("1"));
        let id = self.register(t, global);
        self.complex_body(sc, n, id)?;
        Ok(id)
    }

    fn complex_body(&mut self, sc: &SchemaCtx, n: NodeId, id: TId) -> Result<(), String> {
        let di = sc.di;
        for c in self.d(di).kids(n) {
            if self.d(di).ns(c) != XSD {
                continue;
            }
            let local = self.d(di).local(c).to_string();
            match local.as_str() {
                "simpleContent" | "complexContent" => {
                    let simple = local == "simpleContent";
                    for dnode in self.d(di).kids(c) {
                        if self.d(di).ns(dnode) != XSD {
                            continue;
                        }
                        let dl = self.d(di).local(dnode).to_string();
                        let ext = match dl.as_str() {
                            "restriction" => false,
                            "extension" => true,
                            _ => continue,
                        };
                        self.sdl.types[id].deriv = match (simple, ext) {
                            (true, false) => Deriv::SimpleRestriction,
                            (true, true) => Deriv::SimpleExtension,
                            (false, false) => Deriv::ComplexRestriction,
                            (false, true) => Deriv::ComplexExtension,
                        };
                        if let Some(b) = self.d(di).attr(dnode, "base") {
                            let r = self.qref(di, dnode, &b);
                            if let Some(TRef::Q(ns, l)) = &r
                                && l == "Array"
                                && (ns == NS_SOAP11_ENC || ns == NS_SOAP12_ENC)
                            {
                                self.sdl.types[id].soap_array = true;
                            }
                            self.sdl.types[id].base = r;
                        }
                        if simple && !ext {
                            self.facets(sc, dnode, id)?;
                        }
                        self.content(sc, dnode, id)?;
                    }
                }
                _ => {}
            }
        }
        self.content(sc, n, id)
    }

    /// Particles and attributes directly under `n`.
    fn content(&mut self, sc: &SchemaCtx, n: NodeId, id: TId) -> Result<(), String> {
        let di = sc.di;
        for c in self.d(di).kids(n) {
            if self.d(di).ns(c) != XSD {
                continue;
            }
            let local = self.d(di).local(c).to_string();
            match local.as_str() {
                "sequence" | "all" | "choice" | "group" => {
                    let m = self.particle(sc, c)?;
                    self.sdl.types[id].model = Some(m);
                }
                "attribute" => {
                    let a = self.attribute(sc, c, false)?;
                    self.sdl.types[id].attrs.push(a);
                }
                "attributeGroup" => {
                    if let Some(r) = self.d(di).attr(c, "ref")
                        && let Some((ns, l)) = self.d(di).resolve_qname(c, &r)
                    {
                        self.sdl.types[id].attrs.push(Attr {
                            group: Some((ns, l)),
                            ..Default::default()
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn attr_list(&mut self, sc: &SchemaCtx, n: NodeId) -> Result<Vec<Attr>, String> {
        let di = sc.di;
        let mut v = Vec::new();
        for c in self.d(di).kids(n) {
            if self.d(di).ns(c) != XSD {
                continue;
            }
            match self.d(di).local(c) {
                "attribute" => {
                    let a = self.attribute(sc, c, false)?;
                    v.push(a);
                }
                "attributeGroup" => {
                    if let Some(r) = self.d(di).attr(c, "ref")
                        && let Some((ns, l)) = self.d(di).resolve_qname(c, &r)
                    {
                        v.push(Attr {
                            group: Some((ns, l)),
                            ..Default::default()
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(v)
    }

    fn particle(&mut self, sc: &SchemaCtx, n: NodeId) -> Result<Model, String> {
        let di = sc.di;
        let local = self.d(di).local(n).to_string();
        let min = occurs(self.d(di).attr(n, "minOccurs"), 1);
        let max = occurs(self.d(di).attr(n, "maxOccurs"), 1);
        let kind = match local.as_str() {
            "sequence" => MKind::Sequence,
            "all" => MKind::All,
            "choice" => MKind::Choice,
            "group" => MKind::Group,
            "any" => MKind::Any,
            _ => MKind::Element,
        };
        let mut m = Model {
            kind,
            min,
            max,
            items: Vec::new(),
            elem: None,
            group: None,
        };
        match kind {
            MKind::Group => {
                if let Some(r) = self.d(di).attr(n, "ref") {
                    m.group = self.d(di).resolve_qname(n, &r);
                } else {
                    for g in self.d(di).kids(n) {
                        let d = self.d(di);
                        if d.ns(g) == XSD && matches!(d.local(g), "sequence" | "all" | "choice") {
                            let p = self.particle(sc, g)?;
                            m.items.push(p);
                        }
                    }
                }
            }
            MKind::Any => {}
            _ => {
                for c in self.d(di).kids(n) {
                    if self.d(di).ns(c) != XSD {
                        continue;
                    }
                    let cl = self.d(di).local(c).to_string();
                    match cl.as_str() {
                        "element" => {
                            let e = self.element(sc, c, false)?;
                            let t = &self.sdl.types[e];
                            m.items.push(Model {
                                kind: MKind::Element,
                                min: t.min,
                                max: t.max,
                                items: Vec::new(),
                                elem: Some(e),
                                group: None,
                            });
                        }
                        "sequence" | "all" | "choice" | "group" | "any" => {
                            let p = self.particle(sc, c)?;
                            m.items.push(p);
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(m)
    }

    /// `<element>` (global or local); returns its declaration.
    fn element(&mut self, sc: &SchemaCtx, n: NodeId, global: bool) -> Result<TId, String> {
        let di = sc.di;
        let d = self.d(di);
        let min = occurs(d.attr(n, "minOccurs"), 1);
        let max = occurs(d.attr(n, "maxOccurs"), 1);
        if let Some(r) = d.attr(n, "ref") {
            let (ns, l) = d
                .resolve_qname(n, &r)
                .unwrap_or_else(|| (String::new(), local_part(&r).to_string()));
            let mut t = SType::new(Kind::Element, &l, &ns);
            t.r#ref = Some((ns, l));
            t.min = min;
            t.max = max;
            t.qualified = false;
            let id = self.sdl.add(t);
            return Ok(id);
        }
        let name = d.attr(n, "name").unwrap_or_default();
        let nillable = matches!(d.attr(n, "nillable").as_deref(), Some("true") | Some("1"));
        let form = d.attr(n, "form");
        // Global elements too follow elementFormDefault (observed on PHP
        // 8.5; tests schema/schema034.phpt, schema035.phpt).
        let qualified = match form.as_deref() {
            Some("qualified") => true,
            Some("unqualified") => false,
            _ => sc.efd,
        };
        let default = d.attr(n, "default");
        let fixed = d.attr(n, "fixed");
        let typ = d.attr(n, "type").and_then(|t| d.resolve_qname(n, &t));
        let mut t = SType::new(Kind::Element, &name, &sc.tns);
        t.min = min;
        t.max = max;
        t.nillable = nillable;
        t.qualified = qualified;
        t.default = default;
        t.fixed = fixed;
        t.global = global;
        t.base = typ.map(|(ns, l)| TRef::Q(ns, l));
        let id = self.sdl.add(t);
        if global {
            self.sdl
                .elements
                .entry((sc.tns.clone(), name.clone()))
                .or_insert(id);
        }
        for c in self.d(di).kids(n) {
            if self.d(di).ns(c) != XSD {
                continue;
            }
            let cl = self.d(di).local(c).to_string();
            match cl.as_str() {
                "complexType" => {
                    let ct = self.complex_type(sc, c, Some(name.clone()))?;
                    self.sdl.types[ct].anonymous = true;
                    self.sdl.types[id].base = Some(TRef::Id(ct));
                }
                "simpleType" => {
                    let st = self.simple_type(sc, c, Some((name.clone(), true)))?;
                    self.sdl.types[st].anonymous = true;
                    self.sdl.types[id].base = Some(TRef::Id(st));
                }
                _ => {}
            }
        }
        Ok(id)
    }

    fn attribute(&mut self, sc: &SchemaCtx, n: NodeId, global: bool) -> Result<Attr, String> {
        let di = sc.di;
        let d = self.d(di);
        let mut a = Attr {
            default: d.attr(n, "default"),
            fixed: d.attr(n, "fixed"),
            required: d.attr(n, "use").as_deref() == Some("required"),
            ..Default::default()
        };
        if let Some(r) = d.attr(n, "ref") {
            a.r#ref = d.resolve_qname(n, &r);
        } else {
            a.name = d.attr(n, "name").unwrap_or_default();
            a.ns = sc.tns.clone();
            let _ = global;
            a.qualified = match d.attr(n, "form").as_deref() {
                Some("qualified") => true,
                Some("unqualified") => false,
                _ => sc.afd,
            };
            a.typ = d
                .attr(n, "type")
                .and_then(|t| d.resolve_qname(n, &t))
                .map(|(ns, l)| TRef::Q(ns, l));
        }
        // wsdl:arrayType / itemType / arraySize.
        if let Some(at) = d.attr_ns(n, NS_WSDL, "arrayType") {
            let (q, dims) = match at.find('[') {
                Some(i) => (at[..i].to_string(), at[i..].to_string()),
                None => (at.clone(), String::new()),
            };
            if let Some((ns, l)) = d.resolve_qname(n, &q) {
                a.array_type = Some((ns, l, dims));
            }
        }
        if let Some(it) = d.attr_ns(n, NS_WSDL, "itemType")
            && let Some(q) = d.resolve_qname(n, &it)
        {
            a.item_type = Some(q);
        }
        if let Some(sz) = d.attr_ns(n, NS_WSDL, "arraySize") {
            a.array_size = Some(sz);
        }
        for c in self.d(di).kids(n) {
            if self.d(di).is(c, XSD, "simpleType") {
                let st = self.simple_type(sc, c, None)?;
                a.typ = Some(TRef::Id(st));
            }
        }
        Ok(a)
    }
}

struct SchemaCtx {
    di: usize,
    tns: String,
    efd: bool,
    afd: bool,
}
