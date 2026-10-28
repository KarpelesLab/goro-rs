//! A RELAX NG validator (`DOMDocument::relaxNGValidate()`,
//! `XMLReader::setRelaxNGSchema()`): the full XML syntax of patterns
//! (RELAX NG specification, OASIS 2001) with `include`, `externalRef`,
//! `combine` and nested grammars, and the XML Schema datatype library,
//! validated with the derivative algorithm (James Clark, "An algorithm
//! for RELAX NG validation").
//!
//! Messages follow libxml2's, observed on PHP 8.5 (tests
//! ext/dom/tests/DOMDocument_relaxNGValidate*.phpt).

use std::collections::HashMap;
use std::rc::Rc;

use crate::schema::Invalid;
use crate::tree::{Arena, NodeData, NodeId};
use crate::xml::{EntityLoader, ParseOptions};

/// The RELAX NG namespace.
pub const RNG_NS: &str = "http://relaxng.org/ns/structure/1.0";
const XSD_TYPES: &str = "http://www.w3.org/2001/XMLSchema-datatypes";

#[derive(Debug, PartialEq)]
enum NameClass {
    Name(String, String),
    Any(Option<Rc<NameClass>>),
    Ns(String, Option<Rc<NameClass>>),
    Choice(Rc<NameClass>, Rc<NameClass>),
}

impl NameClass {
    fn contains(&self, ns: &str, local: &str) -> bool {
        match self {
            NameClass::Name(n, l) => n == ns && l == local,
            NameClass::Any(ex) => !ex.as_ref().is_some_and(|e| e.contains(ns, local)),
            NameClass::Ns(n, ex) => n == ns && !ex.as_ref().is_some_and(|e| e.contains(ns, local)),
            NameClass::Choice(a, b) => a.contains(ns, local) || b.contains(ns, local),
        }
    }

    fn names(&self, out: &mut Vec<String>) {
        match self {
            NameClass::Name(_, l) => out.push(l.clone()),
            NameClass::Choice(a, b) => {
                a.names(out);
                b.names(out);
            }
            _ => {}
        }
    }
}

/// A datatype: its library (empty for the built-in one) and name.
#[derive(Debug, PartialEq, Clone)]
struct Dt {
    lib: String,
    name: String,
}

#[derive(Debug, PartialEq)]
enum Pat {
    Empty,
    NotAllowed,
    Text,
    Choice(Rc<Pat>, Rc<Pat>),
    Interleave(Rc<Pat>, Rc<Pat>),
    Group(Rc<Pat>, Rc<Pat>),
    OneOrMore(Rc<Pat>),
    List(Rc<Pat>),
    Data(Dt, Option<Rc<Pat>>),
    Value(Dt, String),
    Attribute(Rc<NameClass>, Rc<Pat>),
    Element(Rc<NameClass>, Rc<Pat>),
    Ref(usize),
    After(Rc<Pat>, Rc<Pat>),
}

type P = Rc<Pat>;

thread_local! {
    static EMPTY: P = Rc::new(Pat::Empty);
    static NOT_ALLOWED: P = Rc::new(Pat::NotAllowed);
}

fn empty() -> P {
    EMPTY.with(Rc::clone)
}

fn not_allowed() -> P {
    NOT_ALLOWED.with(Rc::clone)
}

fn is_na(p: &P) -> bool {
    matches!(**p, Pat::NotAllowed)
}

fn same(a: &P, b: &P) -> bool {
    Rc::ptr_eq(a, b) || a == b
}

fn choice(a: P, b: P) -> P {
    if is_na(&a) {
        return b;
    }
    if is_na(&b) || same(&a, &b) {
        return a;
    }
    // Keep choices free of duplicates (the derivatives of repetitions
    // would otherwise grow without bound).
    if let Pat::Choice(x, y) = &*a
        && (same(x, &b) || same(y, &b))
    {
        return a;
    }
    Rc::new(Pat::Choice(a, b))
}

fn group(a: P, b: P) -> P {
    if is_na(&a) || is_na(&b) {
        return not_allowed();
    }
    if matches!(*a, Pat::Empty) {
        return b;
    }
    if matches!(*b, Pat::Empty) {
        return a;
    }
    Rc::new(Pat::Group(a, b))
}

fn interleave(a: P, b: P) -> P {
    if is_na(&a) || is_na(&b) {
        return not_allowed();
    }
    if matches!(*a, Pat::Empty) {
        return b;
    }
    if matches!(*b, Pat::Empty) {
        return a;
    }
    Rc::new(Pat::Interleave(a, b))
}

fn after(a: P, b: P) -> P {
    if is_na(&a) || is_na(&b) {
        return not_allowed();
    }
    Rc::new(Pat::After(a, b))
}

fn one_or_more(a: P) -> P {
    if is_na(&a) {
        return a;
    }
    Rc::new(Pat::OneOrMore(a))
}

fn is_ws(s: &str) -> bool {
    s.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
}

/// A compiled grammar.
#[derive(Debug)]
pub struct Grammar {
    start: P,
    defs: Vec<P>,
}

// ------------------------------------------------------------- compiling

#[derive(Clone)]
struct Ctx {
    ns: String,
    lib: String,
    base: Option<String>,
}

#[derive(Default)]
struct Scope {
    names: HashMap<String, usize>,
    /// Pending bodies of each define: (combine, pattern).
    start: Vec<(Option<String>, P)>,
}

/// One `define` or `start` body with its `combine` method.
type Part = (Option<String>, P);

struct Compiler<'a> {
    a: &'a mut Arena,
    loader: &'a mut dyn EntityLoader,
    /// Define bodies by index (combine method, patterns).
    bodies: Vec<(String, Vec<Part>)>,
    scopes: Vec<Scope>,
    errors: Vec<String>,
    depth: usize,
}

fn rng_children(a: &Arena, n: NodeId) -> Vec<(String, NodeId)> {
    a.children(n)
        .filter_map(|c| {
            let e = a.element(c)?;
            (e.name.ns.as_deref() == Some(RNG_NS)).then(|| (e.name.local.to_string(), c))
        })
        .collect()
}

fn attr(a: &Arena, n: NodeId, name: &str) -> Option<String> {
    a.get_attr_node_ns(n, None, name).map(|x| a.attr_value(x))
}

fn trimmed(a: &Arena, n: NodeId, name: &str) -> Option<String> {
    attr(a, n, name).map(|s| s.trim().to_string())
}

impl Compiler<'_> {
    fn ctx_of(&self, n: NodeId, ctx: &Ctx) -> Ctx {
        let mut c = ctx.clone();
        if let Some(ns) = attr(self.a, n, "ns") {
            c.ns = ns;
        }
        if let Some(l) = attr(self.a, n, "datatypeLibrary") {
            c.lib = l;
        }
        c
    }

    fn index(&mut self, scope: usize, name: &str) -> usize {
        if let Some(&i) = self.scopes[scope].names.get(name) {
            return i;
        }
        let i = self.bodies.len();
        self.bodies.push((name.to_string(), Vec::new()));
        self.scopes[scope].names.insert(name.to_string(), i);
        i
    }

    /// Loads an included or external document: its root element.
    fn load(&mut self, href: &str, base: Option<&str>) -> Option<(NodeId, Option<String>)> {
        let url = crate::xml::resolve_system_id(href, base);
        let bytes = self.loader.load(None, href, base).ok()?;
        let mut opts = ParseOptions::with_flags(0);
        opts.url = Some(url.clone());
        let b = crate::xml::build(self.a, &bytes, &opts, None);
        if !b.ok {
            return None;
        }
        Some((self.a.document_element(b.doc)?, Some(url)))
    }

    /// Patterns of the children of `n`, as a group.
    fn group_of(&mut self, n: NodeId, ctx: &Ctx, scope: usize) -> Option<P> {
        let mut out: Option<P> = None;
        for (k, c) in rng_children(self.a, n) {
            let p = self.pattern(&k, c, ctx, scope);
            out = Some(match out {
                Some(o) => group(o, p),
                None => p,
            });
        }
        out
    }

    fn fold(&mut self, n: NodeId, ctx: &Ctx, scope: usize, f: fn(P, P) -> P) -> P {
        let mut out: Option<P> = None;
        for (k, c) in rng_children(self.a, n) {
            let p = self.pattern(&k, c, ctx, scope);
            out = Some(match out {
                Some(o) => f(o, p),
                None => p,
            });
        }
        out.unwrap_or_else(empty)
    }

    fn name_class(&mut self, k: &str, n: NodeId, ctx: &Ctx, attribute: bool) -> Rc<NameClass> {
        let ctx = self.ctx_of(n, ctx);
        let except = |me: &mut Self| -> Option<Rc<NameClass>> {
            let ex = rng_children(me.a, n)
                .into_iter()
                .find(|(k, _)| k == "except")?;
            let mut out: Option<Rc<NameClass>> = None;
            for (k2, c2) in rng_children(me.a, ex.1) {
                let nc = me.name_class(&k2, c2, &ctx, attribute);
                out = Some(match out {
                    Some(o) => Rc::new(NameClass::Choice(o, nc)),
                    None => nc,
                });
            }
            out
        };
        match k {
            "name" => {
                let t = self.a.text_content(n).trim().to_string();
                self.qname(n, &t, &ctx, attribute)
            }
            "anyName" => Rc::new(NameClass::Any(except(self))),
            "nsName" => Rc::new(NameClass::Ns(ctx.ns.clone(), except(self))),
            "choice" => {
                let mut out: Option<Rc<NameClass>> = None;
                for (k2, c2) in rng_children(self.a, n) {
                    let nc = self.name_class(&k2, c2, &ctx, attribute);
                    out = Some(match out {
                        Some(o) => Rc::new(NameClass::Choice(o, nc)),
                        None => nc,
                    });
                }
                out.unwrap_or_else(|| Rc::new(NameClass::Any(None)))
            }
            _ => Rc::new(NameClass::Any(None)),
        }
    }

    fn qname(&self, n: NodeId, t: &str, ctx: &Ctx, attribute: bool) -> Rc<NameClass> {
        match t.split_once(':') {
            Some((p, l)) => {
                let ns = self
                    .a
                    .lookup_ns(n, Some(p))
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                Rc::new(NameClass::Name(ns, l.to_string()))
            }
            // Unprefixed attribute names are in no namespace unless the
            // pattern has its own `ns` attribute (RELAX NG 4.10).
            None if attribute => {
                let ns = attr(self.a, n, "ns").unwrap_or_default();
                Rc::new(NameClass::Name(ns, t.to_string()))
            }
            None => Rc::new(NameClass::Name(ctx.ns.clone(), t.to_string())),
        }
    }

    fn pattern(&mut self, k: &str, n: NodeId, ctx: &Ctx, scope: usize) -> P {
        self.depth += 1;
        let p = if self.depth > 200 {
            not_allowed()
        } else {
            self.pattern_inner(k, n, ctx, scope)
        };
        self.depth -= 1;
        p
    }

    fn pattern_inner(&mut self, k: &str, n: NodeId, ctx: &Ctx, scope: usize) -> P {
        let ctx = self.ctx_of(n, ctx);
        match k {
            "element" | "attribute" => {
                let is_attr = k == "attribute";
                let kids = rng_children(self.a, n);
                let (nc, rest) = match trimmed(self.a, n, "name") {
                    Some(name) => (self.qname(n, &name, &ctx, is_attr), &kids[..]),
                    None => match kids.split_first() {
                        Some(((k0, c0), rest)) => (self.name_class(k0, *c0, &ctx, is_attr), rest),
                        None => {
                            self.errors.push(format!(
                                "xmlRelaxNGParse{}: {} has no content",
                                if is_attr { "Attribute" } else { "Element" },
                                k
                            ));
                            return not_allowed();
                        }
                    },
                };
                let mut content: Option<P> = None;
                for (k2, c2) in rest.iter().cloned() {
                    let p = self.pattern(&k2, c2, &ctx, scope);
                    content = Some(match content {
                        Some(o) => group(o, p),
                        None => p,
                    });
                }
                if is_attr {
                    let c = content.unwrap_or_else(|| Rc::new(Pat::Text));
                    Rc::new(Pat::Attribute(nc, c))
                } else {
                    match content {
                        Some(c) => Rc::new(Pat::Element(nc, c)),
                        None => {
                            self.errors
                                .push("xmlRelaxNGParseElement: element has no content".into());
                            not_allowed()
                        }
                    }
                }
            }
            "group" => self.fold(n, &ctx, scope, group),
            "interleave" => self.fold(n, &ctx, scope, interleave),
            "choice" => {
                let mut out: Option<P> = None;
                for (k2, c2) in rng_children(self.a, n) {
                    let p = self.pattern(&k2, c2, &ctx, scope);
                    out = Some(match out {
                        Some(o) => Rc::new(Pat::Choice(o, p)),
                        None => p,
                    });
                }
                out.unwrap_or_else(not_allowed)
            }
            "optional" => {
                let p = self.fold(n, &ctx, scope, group);
                choice(p, empty())
            }
            "zeroOrMore" => {
                let p = self.fold(n, &ctx, scope, group);
                choice(one_or_more(p), empty())
            }
            "oneOrMore" => {
                let p = self.fold(n, &ctx, scope, group);
                one_or_more(p)
            }
            "mixed" => {
                let p = self.fold(n, &ctx, scope, group);
                interleave(p, Rc::new(Pat::Text))
            }
            "list" => {
                let p = self.fold(n, &ctx, scope, group);
                Rc::new(Pat::List(p))
            }
            "empty" => empty(),
            "text" => Rc::new(Pat::Text),
            "notAllowed" => not_allowed(),
            "ref" | "parentRef" => {
                let name = trimmed(self.a, n, "name").unwrap_or_default();
                let s = if k == "parentRef" {
                    scope.saturating_sub(1)
                } else {
                    scope
                };
                Rc::new(Pat::Ref(self.index(s, &name)))
            }
            "data" => {
                let dt = Dt {
                    lib: ctx.lib.clone(),
                    name: trimmed(self.a, n, "type").unwrap_or_default(),
                };
                let ex = rng_children(self.a, n)
                    .into_iter()
                    .find(|(k, _)| k == "except")
                    .map(|(_, e)| self.fold(e, &ctx, scope, choice));
                Rc::new(Pat::Data(dt, ex))
            }
            "value" => {
                let dt = match trimmed(self.a, n, "type") {
                    Some(t) => Dt {
                        lib: ctx.lib.clone(),
                        name: t,
                    },
                    None => Dt {
                        lib: String::new(),
                        name: "token".into(),
                    },
                };
                Rc::new(Pat::Value(dt, self.a.text_content(n)))
            }
            "externalRef" => {
                let href = trimmed(self.a, n, "href").unwrap_or_default();
                match self.load(&href, ctx.base.as_deref()) {
                    Some((root, url)) => {
                        let mut c2 = self.ctx_of(root, &ctx);
                        c2.base = url;
                        let k2 = self
                            .a
                            .element(root)
                            .map(|e| e.name.local.to_string())
                            .unwrap_or_default();
                        self.pattern(&k2, root, &c2, scope)
                    }
                    None => {
                        self.errors
                            .push(format!("xmlRelaxNGParse: could not load {href}"));
                        not_allowed()
                    }
                }
            }
            "grammar" => {
                self.scopes.push(Scope::default());
                let s = self.scopes.len() - 1;
                self.grammar(n, &ctx, s, &[], false);
                let sc = self.scopes.pop().unwrap();
                self.combine(sc.start, "<start>")
            }
            _ => not_allowed(),
        }
    }

    fn combine(&mut self, parts: Vec<(Option<String>, P)>, what: &str) -> P {
        if parts.is_empty() {
            self.errors
                .push(format!("xmlRelaxNGParse: {what} has no definition"));
            return not_allowed();
        }
        let interleaved = parts
            .iter()
            .any(|(c, _)| c.as_deref() == Some("interleave"));
        let mut out: Option<P> = None;
        for (_, p) in parts {
            out = Some(match out {
                Some(o) if interleaved => interleave(o, p),
                Some(o) => Rc::new(Pat::Choice(o, p)),
                None => p,
            });
        }
        out.unwrap()
    }

    /// The names a grammar's content defines (`div` included) and whether
    /// it has a `start`.
    fn defined(&self, n: NodeId, names: &mut Vec<String>, start: &mut bool) {
        for (k, c) in rng_children(self.a, n) {
            match k.as_str() {
                "define" => names.extend(trimmed(self.a, c, "name")),
                "start" => *start = true,
                "div" => self.defined(c, names, start),
                _ => {}
            }
        }
    }

    /// Processes grammar content into `scope`, skipping definitions that
    /// an including `include` overrides.
    fn grammar(&mut self, n: NodeId, ctx: &Ctx, scope: usize, skip: &[String], skip_start: bool) {
        for (k, c) in rng_children(self.a, n) {
            let cctx = self.ctx_of(c, ctx);
            match k.as_str() {
                "start" => {
                    if skip_start {
                        continue;
                    }
                    let comb = trimmed(self.a, c, "combine");
                    let p = self.group_of(c, &cctx, scope).unwrap_or_else(not_allowed);
                    self.scopes[scope].start.push((comb, p));
                }
                "define" => {
                    let name = trimmed(self.a, c, "name").unwrap_or_default();
                    if skip.contains(&name) {
                        continue;
                    }
                    let comb = trimmed(self.a, c, "combine");
                    let p = self.group_of(c, &cctx, scope).unwrap_or_else(not_allowed);
                    let i = self.index(scope, &name);
                    self.bodies[i].1.push((comb, p));
                }
                "div" => self.grammar(c, &cctx, scope, skip, skip_start),
                "include" => {
                    let href = trimmed(self.a, c, "href").unwrap_or_default();
                    let mut names = skip.to_vec();
                    let mut start = skip_start;
                    self.defined(c, &mut names, &mut start);
                    match self.load(&href, ctx.base.as_deref()) {
                        Some((root, url)) => {
                            let mut ictx = self.ctx_of(root, &cctx);
                            ictx.base = url;
                            self.grammar(root, &ictx, scope, &names, start);
                        }
                        None => {
                            self.errors
                                .push(format!("xmlRelaxNGParse: could not load {href}"));
                        }
                    }
                    self.grammar(c, &cctx, scope, skip, skip_start);
                }
                _ => {}
            }
        }
    }
}

/// Compiles the RELAX NG schema whose root element is `root` (in arena
/// `a`, where included documents are loaded through `loader`).
pub fn compile(
    a: &mut Arena,
    root: NodeId,
    loader: &mut dyn EntityLoader,
) -> Result<Grammar, Vec<String>> {
    let is_rng = a
        .element(root)
        .is_some_and(|e| e.name.ns.as_deref() == Some(RNG_NS));
    if !is_rng {
        return Err(vec![
            "xmlRelaxNGParseDocument: RelaxNG empty or failed to compile".into(),
        ]);
    }
    let base = a.doc(a.doc_of(root)).url.clone();
    let k = a.element(root).unwrap().name.local.to_string();
    let mut c = Compiler {
        a,
        loader,
        bodies: Vec::new(),
        scopes: vec![Scope::default()],
        errors: Vec::new(),
        depth: 0,
    };
    let ctx0 = Ctx {
        ns: String::new(),
        lib: String::new(),
        base,
    };
    let ctx = c.ctx_of(root, &ctx0);
    let start = if k == "grammar" {
        c.grammar(root, &ctx, 0, &[], false);
        let st = std::mem::take(&mut c.scopes[0].start);
        c.combine(st, "<start>")
    } else {
        c.pattern(&k, root, &ctx, 0)
    };
    let mut defs = Vec::new();
    for (name, parts) in std::mem::take(&mut c.bodies) {
        if parts.is_empty() {
            c.errors
                .push(format!("Reference {name} has no matching definition"));
            defs.push(not_allowed());
        } else {
            defs.push(c.combine(parts, &name));
        }
    }
    if !c.errors.is_empty() {
        return Err(c.errors);
    }
    Ok(Grammar { start, defs })
}

// ------------------------------------------------------------ validating

fn datatype_ok(dt: &Dt, v: &str) -> bool {
    if dt.lib.is_empty() {
        return true;
    }
    if dt.lib == XSD_TYPES {
        return super::xsd::builtin_ok(&dt.name, v);
    }
    true
}

fn value_eq(dt: &Dt, a: &str, b: &str) -> bool {
    if dt.lib.is_empty() && dt.name == "string" {
        return a == b;
    }
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    norm(a) == norm(b)
}

enum Child {
    Text(String),
    Elem(NodeId),
}

struct V<'a> {
    g: &'a Grammar,
    errors: Vec<Invalid>,
}

const E_ELEMNAME: u32 = 13;
const E_ELEMWRONG: u32 = 38;
const E_NOTEXT: u32 = 26;
const E_ATTR: u32 = 39;
const E_DATATYPE: u32 = 31;
const E_CONTENT: u32 = 30;
const E_NOELEM: u32 = 25;

impl V<'_> {
    fn deref(&self, i: usize) -> P {
        self.g.defs[i].clone()
    }

    fn nullable(&self, p: &P) -> bool {
        self.nullable_d(p, 0)
    }

    fn nullable_d(&self, p: &P, d: usize) -> bool {
        if d > 500 {
            return false;
        }
        match &**p {
            Pat::Empty | Pat::Text => true,
            Pat::Group(a, b) | Pat::Interleave(a, b) => {
                self.nullable_d(a, d + 1) && self.nullable_d(b, d + 1)
            }
            Pat::Choice(a, b) => self.nullable_d(a, d + 1) || self.nullable_d(b, d + 1),
            Pat::OneOrMore(a) => self.nullable_d(a, d + 1),
            Pat::Ref(i) => self.nullable_d(&self.deref(*i), d + 1),
            _ => false,
        }
    }

    fn apply_after(&self, p: &P, f: &dyn Fn(P) -> P) -> P {
        match &**p {
            Pat::After(a, b) => after(a.clone(), f(b.clone())),
            Pat::Choice(a, b) => choice(self.apply_after(a, f), self.apply_after(b, f)),
            _ => not_allowed(),
        }
    }

    fn start_open(&self, p: &P, ns: &str, local: &str) -> P {
        match &**p {
            Pat::Choice(a, b) => {
                choice(self.start_open(a, ns, local), self.start_open(b, ns, local))
            }
            Pat::Element(nc, c) => {
                if nc.contains(ns, local) {
                    after(c.clone(), empty())
                } else {
                    not_allowed()
                }
            }
            Pat::Interleave(a, b) => {
                let (a1, b1) = (a.clone(), b.clone());
                choice(
                    self.apply_after(&self.start_open(a, ns, local), &move |x| {
                        interleave(x, b1.clone())
                    }),
                    self.apply_after(&self.start_open(b, ns, local), &move |x| {
                        interleave(a1.clone(), x)
                    }),
                )
            }
            Pat::OneOrMore(a) => {
                let pp = p.clone();
                self.apply_after(&self.start_open(a, ns, local), &move |x| {
                    group(x, choice(pp.clone(), empty()))
                })
            }
            Pat::Group(a, b) => {
                let b1 = b.clone();
                let x = self.apply_after(&self.start_open(a, ns, local), &move |y| {
                    group(y, b1.clone())
                });
                if self.nullable(a) {
                    choice(x, self.start_open(b, ns, local))
                } else {
                    x
                }
            }
            Pat::After(a, b) => {
                let b1 = b.clone();
                self.apply_after(&self.start_open(a, ns, local), &move |x| {
                    after(x, b1.clone())
                })
            }
            Pat::Ref(i) => self.start_open(&self.deref(*i), ns, local),
            _ => not_allowed(),
        }
    }

    fn att(&self, p: &P, ns: &str, local: &str, v: &str) -> P {
        match &**p {
            Pat::After(a, b) => after(self.att(a, ns, local, v), b.clone()),
            Pat::Choice(a, b) => choice(self.att(a, ns, local, v), self.att(b, ns, local, v)),
            Pat::Group(a, b) => choice(
                group(self.att(a, ns, local, v), b.clone()),
                group(a.clone(), self.att(b, ns, local, v)),
            ),
            Pat::Interleave(a, b) => choice(
                interleave(self.att(a, ns, local, v), b.clone()),
                interleave(a.clone(), self.att(b, ns, local, v)),
            ),
            Pat::OneOrMore(a) => group(self.att(a, ns, local, v), choice(p.clone(), empty())),
            Pat::Attribute(nc, c) => {
                if nc.contains(ns, local) && self.value_match(c, v) {
                    empty()
                } else {
                    not_allowed()
                }
            }
            Pat::Ref(i) => self.att(&self.deref(*i), ns, local, v),
            _ => not_allowed(),
        }
    }

    fn value_match(&self, p: &P, v: &str) -> bool {
        (self.nullable(p) && is_ws(v)) || self.nullable(&self.text(p, v))
    }

    fn start_close(&self, p: &P) -> P {
        match &**p {
            Pat::After(a, b) => after(self.start_close(a), b.clone()),
            Pat::Choice(a, b) => choice(self.start_close(a), self.start_close(b)),
            Pat::Group(a, b) => group(self.start_close(a), self.start_close(b)),
            Pat::Interleave(a, b) => interleave(self.start_close(a), self.start_close(b)),
            Pat::OneOrMore(a) => one_or_more(self.start_close(a)),
            Pat::Attribute(..) => not_allowed(),
            Pat::Ref(i) => {
                let d = self.deref(*i);
                if self.has_attribute(&d, 0) {
                    self.start_close(&d)
                } else {
                    p.clone()
                }
            }
            _ => p.clone(),
        }
    }

    /// True if attribute patterns are reachable without crossing an
    /// element.
    fn has_attribute(&self, p: &P, d: usize) -> bool {
        if d > 100 {
            return false;
        }
        match &**p {
            Pat::Attribute(..) => true,
            Pat::Choice(a, b) | Pat::Group(a, b) | Pat::Interleave(a, b) | Pat::After(a, b) => {
                self.has_attribute(a, d + 1) || self.has_attribute(b, d + 1)
            }
            Pat::OneOrMore(a) => self.has_attribute(a, d + 1),
            Pat::Ref(i) => self.has_attribute(&self.deref(*i), d + 1),
            _ => false,
        }
    }

    fn text(&self, p: &P, s: &str) -> P {
        match &**p {
            Pat::Choice(a, b) => choice(self.text(a, s), self.text(b, s)),
            Pat::Interleave(a, b) => choice(
                interleave(self.text(a, s), b.clone()),
                interleave(a.clone(), self.text(b, s)),
            ),
            Pat::Group(a, b) => {
                let x = group(self.text(a, s), b.clone());
                if self.nullable(a) {
                    choice(x, self.text(b, s))
                } else {
                    x
                }
            }
            Pat::After(a, b) => after(self.text(a, s), b.clone()),
            Pat::OneOrMore(a) => group(self.text(a, s), choice(p.clone(), empty())),
            Pat::Text => p.clone(),
            Pat::Value(dt, v) => {
                if value_eq(dt, v, s) {
                    empty()
                } else {
                    not_allowed()
                }
            }
            Pat::Data(dt, ex) => {
                if datatype_ok(dt, s) && !ex.as_ref().is_some_and(|e| self.value_match(e, s)) {
                    empty()
                } else {
                    not_allowed()
                }
            }
            Pat::List(a) => {
                let mut q = a.clone();
                for w in s.split_whitespace() {
                    q = self.text(&q, w);
                }
                if self.nullable(&q) {
                    empty()
                } else {
                    not_allowed()
                }
            }
            Pat::Ref(i) => self.text(&self.deref(*i), s),
            _ => not_allowed(),
        }
    }

    fn end_tag(&self, p: &P) -> P {
        match &**p {
            Pat::Choice(a, b) => choice(self.end_tag(a), self.end_tag(b)),
            Pat::After(a, b) => {
                if self.nullable(a) {
                    b.clone()
                } else {
                    not_allowed()
                }
            }
            _ => not_allowed(),
        }
    }

    /// The first datatype expected for text in the current content.
    fn datatype(&self, p: &P, d: usize) -> Option<String> {
        if d > 100 {
            return None;
        }
        match &**p {
            Pat::Data(dt, _) | Pat::Value(dt, _) => Some(dt.name.clone()),
            Pat::After(a, _) | Pat::OneOrMore(a) | Pat::List(a) => self.datatype(a, d + 1),
            Pat::Choice(a, b) | Pat::Group(a, b) | Pat::Interleave(a, b) => {
                self.datatype(a, d + 1).or_else(|| self.datatype(b, d + 1))
            }
            Pat::Ref(i) => self.datatype(&self.deref(*i), d + 1),
            _ => None,
        }
    }

    /// The attribute pattern matching a name, if any (for messages).
    fn attr_dt(&self, p: &P, ns: &str, local: &str, d: usize) -> Option<Option<String>> {
        if d > 100 {
            return None;
        }
        match &**p {
            Pat::Attribute(nc, c) if nc.contains(ns, local) => Some(self.datatype(c, 0)),
            Pat::After(a, _) | Pat::OneOrMore(a) => self.attr_dt(a, ns, local, d + 1),
            Pat::Choice(a, b) | Pat::Group(a, b) | Pat::Interleave(a, b) => self
                .attr_dt(a, ns, local, d + 1)
                .or_else(|| self.attr_dt(b, ns, local, d + 1)),
            Pat::Ref(i) => self.attr_dt(&self.deref(*i), ns, local, d + 1),
            _ => None,
        }
    }

    fn element_names(&self, p: &P, out: &mut Vec<String>, d: usize) {
        if d > 100 {
            return;
        }
        match &**p {
            Pat::Element(nc, _) => nc.names(out),
            Pat::Choice(a, b) | Pat::Interleave(a, b) => {
                self.element_names(a, out, d + 1);
                self.element_names(b, out, d + 1);
            }
            Pat::Group(a, b) => {
                self.element_names(a, out, d + 1);
                if self.nullable(a) {
                    self.element_names(b, out, d + 1);
                }
            }
            Pat::After(a, _) | Pat::OneOrMore(a) => self.element_names(a, out, d + 1),
            Pat::Ref(i) => self.element_names(&self.deref(*i), out, d + 1),
            _ => {}
        }
    }

    fn err(&mut self, node: NodeId, code: u32, message: String) {
        self.errors.push(Invalid {
            node,
            code,
            message,
        });
    }

    /// The derivative of `p` by element `el`; `None` after an error.
    fn element(&mut self, a: &Arena, p: &P, el: NodeId, root: bool) -> Option<P> {
        let q = a.element(el)?.name.clone();
        let ns = q.ns.as_deref().unwrap_or("");
        let local = &*q.local;
        let mut cur = self.start_open(p, ns, local);
        if is_na(&cur) {
            if root {
                let mut names = Vec::new();
                self.element_names(p, &mut names, 0);
                let exp = names.first().cloned().unwrap_or_default();
                self.err(
                    el,
                    E_ELEMNAME,
                    format!("Expecting element {exp}, got {local}"),
                );
            } else {
                self.err(
                    el,
                    E_ELEMWRONG,
                    format!("Did not expect element {local} there"),
                );
            }
            return None;
        }
        for at in a.attrs(el).collect::<Vec<_>>() {
            let aq = a.qname(at)?.clone();
            let ans = aq.ns.as_deref().unwrap_or("");
            if ans == crate::tree::XMLNS_NS {
                continue;
            }
            let v = a.attr_value(at);
            let next = self.att(&cur, ans, &aq.local, &v);
            if is_na(&next) {
                match self.attr_dt(&cur, ans, &aq.local, 0) {
                    Some(dt) => {
                        if let Some(t) = dt {
                            self.err(
                                el,
                                E_DATATYPE,
                                format!("Type {t} doesn't allow value '{v}'"),
                            );
                        }
                        self.err(
                            el,
                            E_ATTR,
                            format!("Element {local} failed to validate attributes"),
                        );
                    }
                    None => self.err(
                        el,
                        E_ATTR,
                        format!("Invalid attribute {} for element {local}", aq.local),
                    ),
                }
                return None;
            }
            cur = next;
        }
        cur = self.start_close(&cur);
        if is_na(&cur) {
            self.err(
                el,
                E_ATTR,
                format!("Element {local} failed to validate attributes"),
            );
            return None;
        }
        // Children: text runs merged, comments and PIs ignored.
        let mut kids: Vec<Child> = Vec::new();
        for c in a.children(el) {
            match a.data(c) {
                NodeData::Element(_) => kids.push(Child::Elem(c)),
                NodeData::Text(t) | NodeData::CData(t) => match kids.last_mut() {
                    Some(Child::Text(s)) => s.push_str(t),
                    _ => kids.push(Child::Text(t.to_string())),
                },
                NodeData::EntityRef(_) => {
                    let t = a.text_content(c);
                    match kids.last_mut() {
                        Some(Child::Text(s)) => s.push_str(&t),
                        _ => kids.push(Child::Text(t)),
                    }
                }
                _ => {}
            }
        }
        let only_text = kids.iter().all(|k| matches!(k, Child::Text(_)));
        if only_text {
            let s: String = kids
                .iter()
                .map(|k| match k {
                    Child::Text(t) => t.as_str(),
                    Child::Elem(_) => "",
                })
                .collect();
            let d = self.text(&cur, &s);
            let next = if is_ws(&s) { choice(cur.clone(), d) } else { d };
            if is_na(&next) {
                match self.datatype(&cur, 0) {
                    Some(t) => {
                        self.err(
                            el,
                            E_DATATYPE,
                            format!("Type {t} doesn't allow value '{s}'"),
                        );
                        self.err(el, E_DATATYPE, format!("Error validating datatype {t}"));
                        self.err(
                            el,
                            E_CONTENT,
                            format!("Element {local} failed to validate content"),
                        );
                    }
                    None => self.err(
                        el,
                        E_NOTEXT,
                        format!("Did not expect text in element {local} content"),
                    ),
                }
                return None;
            }
            cur = next;
        } else {
            for k in kids {
                match k {
                    Child::Text(s) if is_ws(&s) => {}
                    Child::Text(s) => {
                        let next = self.text(&cur, &s);
                        if is_na(&next) {
                            self.err(
                                el,
                                E_NOTEXT,
                                format!("Did not expect text in element {local} content"),
                            );
                            return None;
                        }
                        cur = next;
                    }
                    Child::Elem(c) => cur = self.element(a, &cur, c, false)?,
                }
            }
        }
        let end = self.end_tag(&cur);
        if is_na(&end) {
            self.err(el, E_NOELEM, "Expecting an element , got nothing".into());
            return None;
        }
        Some(end)
    }
}

impl Grammar {
    /// Validates the document element `root` and its subtree.
    pub fn validate_root(&self, a: &Arena, root: NodeId) -> Vec<Invalid> {
        let mut v = V {
            g: self,
            errors: Vec::new(),
        };
        if let Some(p) = v.element(a, &self.start, root, true)
            && !v.nullable(&p)
        {
            v.err(root, E_CONTENT, "Element failed to validate content".into());
        }
        v.errors
    }

    /// Validates document `doc`.
    pub fn validate(&self, a: &Arena, doc: NodeId) -> Vec<Invalid> {
        match a.document_element(doc) {
            Some(r) => self.validate_root(a, r),
            None => vec![Invalid {
                node: doc,
                code: E_NOELEM,
                message: "Expecting an element , got nothing".into(),
            }],
        }
    }
}
