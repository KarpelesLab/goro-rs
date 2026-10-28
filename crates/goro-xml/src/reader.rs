//! A pull reader with the node stream of PHP's `XMLReader` (observed on
//! PHP 8.5; fixture `tests/fixtures/reader.php`).
//!
//! The document is parsed incrementally into a tree (in chunks, as libxml's
//! reader does), and the cursor walks it: start tags, text, end tags of
//! non-empty elements, and so on. Whitespace-only text is reported as
//! significant whitespace; entity references stay unexpanded unless
//! `SUBST_ENTITIES` is set.

use std::collections::HashSet;

use crate::error::XmlError;
use crate::serialize::{SaveOptions, to_string};
use crate::tree::EntityDecl;
use crate::tree::{Arena, DocKind, NodeData, NodeId, XMLNS_NS};
use crate::tree::{DtdData, QName};
use crate::xml::{EntityLoader, ParseOptions, Parser, Sink, StartTag, Status, TreeBuilder, opt};

/// `XMLReader` node types.
pub mod node_type {
    /// XMLReader::NONE
    pub const NONE: i64 = 0;
    /// XMLReader::ELEMENT
    pub const ELEMENT: i64 = 1;
    /// XMLReader::ATTRIBUTE
    pub const ATTRIBUTE: i64 = 2;
    /// XMLReader::TEXT
    pub const TEXT: i64 = 3;
    /// XMLReader::CDATA
    pub const CDATA: i64 = 4;
    /// XMLReader::ENTITY_REF
    pub const ENTITY_REF: i64 = 5;
    /// XMLReader::PI
    pub const PI: i64 = 7;
    /// XMLReader::COMMENT
    pub const COMMENT: i64 = 8;
    /// XMLReader::DOC_TYPE
    pub const DOC_TYPE: i64 = 10;
    /// XMLReader::SIGNIFICANT_WHITESPACE
    pub const SIGNIFICANT_WHITESPACE: i64 = 14;
    /// XMLReader::END_ELEMENT
    pub const END_ELEMENT: i64 = 15;
}

/// `XMLReader::setParserProperty` flags.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReaderProps {
    /// XMLReader::LOADDTD
    pub load_dtd: bool,
    /// XMLReader::DEFAULTATTRS
    pub default_attrs: bool,
    /// XMLReader::VALIDATE
    pub validate: bool,
    /// XMLReader::SUBST_ENTITIES
    pub subst_entities: bool,
}

/// One attribute as the reader exposes it (namespace declarations first).
#[derive(Clone, Debug)]
enum AttrRef {
    Ns(usize),
    Attr(NodeId),
}

/// The reader.
pub struct XmlReader {
    /// The arena holding the document being read.
    pub arena: Arena,
    /// The document.
    pub doc: NodeId,
    input: Vec<u8>,
    fed: usize,
    parser: Option<Parser<'static>>,
    /// The loader of external resources the parser uses (declared after
    /// `parser`, which borrows it, so it is dropped after it).
    loader: Option<Box<dyn EntityLoader>>,
    /// Parser options (from `XMLReader::XML($source, $encoding, $flags)`).
    pub flags: u32,
    /// Parser properties (set before the first `read`).
    pub props: ReaderProps,
    /// The document URL.
    pub url: Option<String>,
    /// The encoding argument.
    pub encoding: Option<String>,
    closed: HashSet<NodeId>,
    empty: HashSet<NodeId>,
    stack: Vec<NodeId>,
    done: bool,
    /// Parsing failed: `read()` returns false from now on.
    pub failed: bool,
    /// Errors not yet taken by the caller (`take_errors`).
    errors: Vec<XmlError>,
    cur: Option<NodeId>,
    at_end: bool,
    attr: Option<usize>,
    started: bool,
}

struct ReaderSink<'a, 'b> {
    tb: TreeBuilder<'a>,
    closed: &'b mut HashSet<NodeId>,
    empty: &'b mut HashSet<NodeId>,
    stack: &'b mut Vec<NodeId>,
}

impl Sink for ReaderSink<'_, '_> {
    fn xml_decl(&mut self, v: &str, e: Option<&str>, s: i8) {
        self.tb.xml_decl(v, e, s)
    }
    fn doctype(&mut self, d: &DtdData, x: Option<&DtdData>) {
        self.tb.doctype(d, x)
    }
    fn start_element(&mut self, tag: &mut StartTag<'_>) {
        self.tb.start_element(tag);
        let el = self.tb.current();
        if tag.empty {
            self.empty.insert(el);
        }
        self.stack.push(el);
    }
    fn end_element(&mut self, name: &QName) {
        if let Some(el) = self.stack.pop() {
            self.closed.insert(el);
        }
        self.tb.end_element(name)
    }
    fn text(&mut self, t: &str) {
        self.tb.text(t)
    }
    fn cdata(&mut self, t: &str) {
        self.tb.cdata(t)
    }
    fn comment(&mut self, t: &str) {
        self.tb.comment(t)
    }
    fn pi(&mut self, t: &str, d: &str) {
        self.tb.pi(t, d)
    }
    fn entity_start(&mut self, name: &str, decl: Option<&EntityDecl>) -> bool {
        self.tb.entity_start(name, decl)
    }
    fn entity_end(&mut self, name: &str) {
        self.tb.entity_end(name)
    }
    fn error(&mut self, e: &XmlError) {
        self.tb.error(e)
    }
    fn set_line(&mut self, l: u32) {
        self.tb.set_line(l)
    }
}

/// How much input libxml's reader parses at a time.
const CHUNK: usize = 512;

impl XmlReader {
    /// `XMLReader::XML($source, $encoding, $flags)` / `open()` (with the
    /// file contents).
    pub fn new(input: Vec<u8>, encoding: Option<&str>, flags: u32, url: Option<&str>) -> XmlReader {
        let mut arena = Arena::new();
        let doc = arena.new_document(DocKind::Xml);
        arena.doc_mut(doc).url = url.map(str::to_string);
        XmlReader {
            arena,
            doc,
            input,
            fed: 0,
            parser: None,
            loader: None,
            flags,
            props: ReaderProps::default(),
            url: url.map(str::to_string),
            encoding: encoding.map(str::to_string),
            closed: HashSet::new(),
            empty: HashSet::new(),
            stack: Vec::new(),
            done: false,
            failed: false,
            errors: Vec::new(),
            cur: None,
            at_end: false,
            attr: None,
            started: false,
        }
    }

    /// Sets the loader of external resources (DTDs, entities); must be
    /// called before the first `read`.
    pub fn set_loader(&mut self, l: Box<dyn EntityLoader>) {
        if self.parser.is_none() {
            self.loader = Some(l);
        }
    }

    /// Errors reported since the last call.
    pub fn take_errors(&mut self) -> Vec<XmlError> {
        std::mem::take(&mut self.errors)
    }

    fn feed(&mut self) -> bool {
        if self.done || self.failed {
            return false;
        }
        if self.parser.is_none() {
            let mut flags = self.flags;
            if self.props.subst_entities {
                flags |= opt::NOENT;
            }
            if self.props.default_attrs {
                flags |= opt::DTDATTR;
            }
            if self.props.load_dtd {
                flags |= opt::DTDLOAD;
            }
            if self.props.validate {
                flags |= opt::DTDVALID;
            }
            let mut po = ParseOptions::with_flags(flags);
            po.url = self.url.clone();
            po.encoding_override = self.encoding.clone();
            // SAFETY: the box lives as long as the parser (see the field
            // order) and is not otherwise used while it exists.
            let loader = self.loader.as_mut().map(|b| {
                let p: *mut dyn EntityLoader = b.as_mut();
                unsafe { &mut *p }
            });
            self.parser = Some(Parser::new(po.clone(), loader));
        }
        let end = (self.fed + CHUNK).min(self.input.len());
        let is_final = end == self.input.len();
        let chunk = self.input[self.fed..end].to_vec();
        self.fed = end;
        let po = self.parser.as_ref().unwrap().opts.clone();
        let parser = self.parser.as_mut().unwrap();
        // Later chunks continue in the element still open.
        let parent = self.stack.last().copied().unwrap_or(self.doc);
        let tb = TreeBuilder::new(&mut self.arena, parent, &po);
        let mut sink = ReaderSink {
            tb,
            closed: &mut self.closed,
            empty: &mut self.empty,
            stack: &mut self.stack,
        };
        let status = parser.feed(&chunk, is_final, &mut sink);
        let errs = std::mem::take(&mut sink.tb.errors);
        self.errors.extend(errs);
        match status {
            Status::Done => {
                self.done = true;
                self.closed.insert(self.doc);
            }
            Status::Failed => self.failed = true,
            Status::NeedMore => {}
        }
        true
    }

    /// True once node `n` has been parsed to its end.
    pub fn is_node_complete(&self, n: NodeId) -> bool {
        self.is_closed(n)
    }

    fn is_closed(&self, n: NodeId) -> bool {
        n == self.doc && self.done || self.closed.contains(&n)
    }

    /// Feeds until `next` gives an answer.
    fn wait<T>(&mut self, mut f: impl FnMut(&Self) -> Option<T>) -> Option<T> {
        loop {
            if let Some(v) = f(self) {
                return Some(v);
            }
            if self.failed || !self.feed() {
                return f(self);
            }
            if self.failed {
                return None;
            }
        }
    }

    /// `read()`: moves to the next node. False at the end or on error.
    pub fn read(&mut self) -> bool {
        self.attr = None;
        if self.failed {
            return false;
        }
        let next = if !self.started {
            self.started = true;
            self.wait(|r| {
                let first = r.arena.first_child(r.doc);
                match first {
                    Some(f) if r.ready(f) => Some(Some((f, false))),
                    None if r.done => Some(None),
                    _ => None,
                }
            })
        } else {
            let (cur, at_end) = match self.cur {
                Some(c) => (c, self.at_end),
                None => return false,
            };
            self.wait(|r| r.step(cur, at_end))
        };
        match next.flatten() {
            Some((n, end)) => {
                self.cur = Some(n);
                self.at_end = end;
                !self.failed
            }
            None => {
                self.cur = None;
                false
            }
        }
    }

    /// A node can be reported once nothing more can be appended to it.
    fn ready(&self, n: NodeId) -> bool {
        if self.arena.is_element(n) {
            return true;
        }
        self.arena.next_sibling(n).is_some()
            || self.arena.parent(n).is_some_and(|p| self.is_closed(p))
    }

    /// The node after (n, at_end) in reader order; `None` when more input
    /// is needed, `Some(None)` at the end.
    fn step(&self, n: NodeId, at_end: bool) -> Option<Option<(NodeId, bool)>> {
        let a = &self.arena;
        let descend = a.is_element(n) && !at_end && !self.empty.contains(&n);
        if descend {
            if let Some(c) = a.first_child(n) {
                return if self.ready(c) {
                    Some(Some((c, false)))
                } else {
                    None
                };
            }
            return if self.is_closed(n) {
                Some(Some((n, true)))
            } else {
                None
            };
        }
        self.after(n)
    }

    fn after(&self, n: NodeId) -> Option<Option<(NodeId, bool)>> {
        let a = &self.arena;
        if let Some(s) = a.next_sibling(n) {
            return if self.ready(s) {
                Some(Some((s, false)))
            } else {
                None
            };
        }
        let p = a.parent(n)?;
        if !self.is_closed(p) {
            return None;
        }
        if p == self.doc || !a.is_element(p) {
            if matches!(a.data(p), NodeData::EntityRef(_)) {
                return self.after(p);
            }
            return Some(None);
        }
        Some(Some((p, true)))
    }

    /// `next($name)`: the next sibling (skipping the subtree), optionally
    /// the next one with that name.
    pub fn next(&mut self, name: Option<&str>) -> bool {
        self.attr = None;
        if !self.started {
            if !self.read() {
                return false;
            }
            if name.is_none_or(|nm| self.name() == nm) {
                return true;
            }
        }
        loop {
            let Some(cur) = self.cur else {
                return false;
            };
            let next = if self.at_end || !self.arena.is_element(cur) {
                self.wait(|r| r.after(cur))
            } else {
                // Skip the subtree.
                self.wait(|r| {
                    if r.is_closed(cur) || r.empty.contains(&cur) {
                        Some(())
                    } else {
                        None
                    }
                });
                self.wait(|r| r.after(cur))
            };
            match next.flatten() {
                Some((n, end)) => {
                    self.cur = Some(n);
                    self.at_end = end;
                }
                None => {
                    self.cur = None;
                    return false;
                }
            }
            if self.failed {
                return false;
            }
            match name {
                None => return true,
                Some(nm) => {
                    if !self.at_end && self.name() == nm {
                        return true;
                    }
                }
            }
        }
    }

    fn attr_list(&self) -> Vec<AttrRef> {
        let Some(n) = self.cur else {
            return Vec::new();
        };
        if self.at_end {
            return Vec::new();
        }
        let Some(e) = self.arena.element(n) else {
            return Vec::new();
        };
        let mut v: Vec<AttrRef> = (0..e.ns_decls.len()).map(AttrRef::Ns).collect();
        v.extend(self.arena.attrs(n).map(AttrRef::Attr));
        v
    }

    /// The current node (an attribute when positioned on one).
    pub fn current_node(&self) -> Option<NodeId> {
        let n = self.cur?;
        match self.attr {
            Some(i) => match self.attr_list().get(i)? {
                AttrRef::Attr(a) => Some(*a),
                AttrRef::Ns(_) => None,
            },
            None => Some(n),
        }
    }

    /// `nodeType`.
    pub fn node_type(&self) -> i64 {
        use node_type::*;
        let Some(n) = self.cur else {
            return NONE;
        };
        if self.attr.is_some() {
            return ATTRIBUTE;
        }
        if self.at_end {
            return END_ELEMENT;
        }
        match self.arena.data(n) {
            NodeData::Element(_) => ELEMENT,
            NodeData::Text(t) => {
                if t.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r')) {
                    SIGNIFICANT_WHITESPACE
                } else {
                    TEXT
                }
            }
            NodeData::CData(_) => CDATA,
            NodeData::EntityRef(_) => ENTITY_REF,
            NodeData::Pi { .. } => PI,
            NodeData::Comment(_) => COMMENT,
            NodeData::Doctype(_) => DOC_TYPE,
            _ => NONE,
        }
    }

    fn attr_names(&self, i: usize) -> Option<(String, String, String, String, String)> {
        let n = self.cur?;
        let e = self.arena.element(n)?;
        Some(match self.attr_list().get(i)? {
            AttrRef::Ns(k) => {
                let d = &e.ns_decls[*k];
                match &d.prefix {
                    Some(p) => (
                        format!("xmlns:{p}"),
                        p.to_string(),
                        "xmlns".into(),
                        XMLNS_NS.into(),
                        d.uri.to_string(),
                    ),
                    None => (
                        "xmlns".into(),
                        "xmlns".into(),
                        String::new(),
                        XMLNS_NS.into(),
                        d.uri.to_string(),
                    ),
                }
            }
            AttrRef::Attr(a) => {
                let ad = self.arena.attr(*a)?;
                (
                    ad.name.qualified(),
                    ad.name.local.to_string(),
                    ad.name.prefix.as_deref().unwrap_or("").to_string(),
                    ad.name.ns.as_deref().unwrap_or("").to_string(),
                    self.arena.attr_value(*a),
                )
            }
        })
    }

    /// `name`.
    pub fn name(&self) -> String {
        if let Some(i) = self.attr {
            return self.attr_names(i).map(|t| t.0).unwrap_or_default();
        }
        match self.cur {
            Some(n) => self.arena.node_name(n),
            None => String::new(),
        }
    }

    /// `localName`.
    pub fn local_name(&self) -> String {
        if let Some(i) = self.attr {
            return self.attr_names(i).map(|t| t.1).unwrap_or_default();
        }
        match self.cur {
            Some(n) => match self.arena.qname(n) {
                Some(q) => q.local.to_string(),
                None => self.arena.node_name(n),
            },
            None => String::new(),
        }
    }

    /// `prefix`.
    pub fn prefix(&self) -> String {
        if let Some(i) = self.attr {
            return self.attr_names(i).map(|t| t.2).unwrap_or_default();
        }
        self.cur
            .and_then(|n| self.arena.qname(n))
            .and_then(|q| q.prefix.as_deref().map(str::to_string))
            .unwrap_or_default()
    }

    /// `namespaceURI`.
    pub fn namespace_uri(&self) -> String {
        if let Some(i) = self.attr {
            return self.attr_names(i).map(|t| t.3).unwrap_or_default();
        }
        self.cur
            .and_then(|n| self.arena.qname(n))
            .and_then(|q| q.ns.as_deref().map(str::to_string))
            .unwrap_or_default()
    }

    /// `value`.
    pub fn value(&self) -> String {
        if let Some(i) = self.attr {
            return self.attr_names(i).map(|t| t.4).unwrap_or_default();
        }
        match self.cur {
            Some(n) if !self.at_end => match self.arena.data(n) {
                NodeData::Text(t) | NodeData::CData(t) | NodeData::Comment(t) => t.clone(),
                NodeData::Pi { data, .. } => data.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }

    /// `hasValue`.
    pub fn has_value(&self) -> bool {
        use node_type::*;
        matches!(
            self.node_type(),
            ATTRIBUTE | TEXT | CDATA | PI | COMMENT | SIGNIFICANT_WHITESPACE
        )
    }

    /// `depth`.
    pub fn depth(&self) -> i64 {
        let Some(n) = self.cur else {
            return 0;
        };
        let mut d = 0;
        let mut p = self.arena.parent(n);
        while let Some(x) = p {
            if x == self.doc {
                break;
            }
            if self.arena.is_element(x) {
                d += 1;
            }
            p = self.arena.parent(x);
        }
        d + self.attr.is_some() as i64
    }

    /// `isEmptyElement`.
    pub fn is_empty_element(&self) -> bool {
        self.attr.is_none() && !self.at_end && self.cur.is_some_and(|n| self.empty.contains(&n))
    }

    /// `attributeCount` (namespace declarations included).
    pub fn attribute_count(&self) -> i64 {
        if self.attr.is_some() {
            return 0;
        }
        self.attr_list().len() as i64
    }

    /// `xmlLang`.
    pub fn xml_lang(&self) -> String {
        let mut p = self.cur;
        while let Some(x) = p {
            if self.arena.is_element(x)
                && let Some(a) = self
                    .arena
                    .get_attr_node_ns(x, Some(crate::tree::XML_NS), "lang")
            {
                return self.arena.attr_value(a);
            }
            p = self.arena.parent(x);
        }
        String::new()
    }

    /// `baseURI`.
    pub fn base_uri(&self) -> String {
        self.url.clone().unwrap_or_default()
    }

    /// `moveToFirstAttribute()`.
    pub fn move_to_first_attribute(&mut self) -> bool {
        self.move_to_attribute_no(0)
    }

    /// `moveToNextAttribute()`.
    pub fn move_to_next_attribute(&mut self) -> bool {
        let next = self.attr.map_or(0, |i| i + 1);
        self.move_to_attribute_no(next)
    }

    /// `moveToAttributeNo($index)` (namespace declarations first).
    pub fn move_to_attribute_no(&mut self, i: usize) -> bool {
        let saved = self.attr.take();
        if i < self.attr_list().len() {
            self.attr = Some(i);
            true
        } else {
            self.attr = saved;
            false
        }
    }

    /// `moveToAttribute($name)`.
    pub fn move_to_attribute(&mut self, name: &str) -> bool {
        let saved = self.attr.take();
        let n = self.attr_list().len();
        for i in 0..n {
            if self.attr_names(i).is_some_and(|t| t.0 == name) {
                self.attr = Some(i);
                return true;
            }
        }
        self.attr = saved;
        false
    }

    /// `moveToAttributeNs($localName, $namespace)`.
    pub fn move_to_attribute_ns(&mut self, local: &str, ns: &str) -> bool {
        let saved = self.attr.take();
        let n = self.attr_list().len();
        for i in 0..n {
            if self
                .attr_names(i)
                .is_some_and(|t| t.1 == local && t.3 == ns)
            {
                self.attr = Some(i);
                return true;
            }
        }
        self.attr = saved;
        false
    }

    /// `moveToElement()`.
    pub fn move_to_element(&mut self) -> bool {
        self.attr.take().is_some()
    }

    /// The current element's attributes as `getAttribute*()` see them:
    /// `(qualified name, local name, namespace, value)`, namespace
    /// declarations first; available on end tags too, not while
    /// positioned on an attribute (observed on PHP 8.5; tests
    /// ext/xmlreader 003, 010).
    fn element_attrs(&self) -> Vec<(String, String, String, String)> {
        let mut v = Vec::new();
        let Some(n) = self.cur else { return v };
        if self.attr.is_some() {
            return v;
        }
        let Some(e) = self.arena.element(n) else {
            return v;
        };
        for d in &e.ns_decls {
            match &d.prefix {
                Some(p) => v.push((
                    format!("xmlns:{p}"),
                    p.to_string(),
                    XMLNS_NS.to_string(),
                    d.uri.to_string(),
                )),
                None => v.push((
                    "xmlns".into(),
                    "xmlns".into(),
                    XMLNS_NS.to_string(),
                    d.uri.to_string(),
                )),
            }
        }
        for a in self.arena.attrs(n) {
            if let Some(ad) = self.arena.attr(a) {
                v.push((
                    ad.name.qualified(),
                    ad.name.local.to_string(),
                    ad.name.ns.as_deref().unwrap_or("").to_string(),
                    self.arena.attr_value(a),
                ));
            }
        }
        v
    }

    /// `getAttribute($name)`.
    pub fn get_attribute(&self, name: &str) -> Option<String> {
        self.element_attrs()
            .into_iter()
            .find(|t| t.0 == name)
            .map(|t| t.3)
    }

    /// `getAttributeNs($localName, $namespace)`.
    pub fn get_attribute_ns(&self, local: &str, ns: &str) -> Option<String> {
        self.element_attrs()
            .into_iter()
            .find(|t| t.1 == local && t.2 == ns)
            .map(|t| t.3)
    }

    /// `getAttributeNo($index)`: counts attributes only, not namespace
    /// declarations (observed on PHP 8.5).
    pub fn get_attribute_no(&self, i: usize) -> Option<String> {
        let n = self.cur?;
        if self.attr.is_some() {
            return None;
        }
        let a = self.arena.attrs(n).nth(i)?;
        Some(self.arena.attr_value(a))
    }

    /// `lookupNamespace($prefix)`.
    pub fn lookup_namespace(&self, prefix: &str) -> Option<String> {
        let n = self.cur?;
        let p = if prefix.is_empty() {
            None
        } else {
            Some(prefix)
        };
        self.arena.lookup_ns(n, p).map(|s| s.to_string())
    }

    fn save(&self, n: NodeId, shallow: bool) -> String {
        let o = SaveOptions {
            fixup_ns: true,
            shallow,
            ..Default::default()
        };
        to_string(&self.arena, n, &o)
    }

    /// `readOuterXml()`.
    pub fn read_outer_xml(&mut self) -> String {
        let Some(n) = self.current_node() else {
            return String::new();
        };
        if self.at_end {
            return self.save(n, true);
        }
        match self.arena.data(n) {
            NodeData::Doctype(_) => String::new(),
            NodeData::Element(_) => {
                self.wait(|r| {
                    if r.is_closed(n) || r.empty.contains(&n) {
                        Some(())
                    } else {
                        None
                    }
                });
                self.save(n, false)
            }
            NodeData::Attribute(_) => {
                let a = self.arena.attr(n).unwrap();
                let mut s = format!("{}=\"", a.name.qualified());
                crate::serialize::escape_attr(&self.arena.attr_value(n), &mut s);
                s.push('"');
                s
            }
            _ => self.save(n, false),
        }
    }

    /// `readInnerXml()`.
    pub fn read_inner_xml(&mut self) -> String {
        let Some(n) = self.current_node() else {
            return String::new();
        };
        if self.at_end || !self.arena.is_element(n) {
            if let NodeData::Attribute(_) = self.arena.data(n) {
                return self.arena.attr_value(n);
            }
            return String::new();
        }
        self.wait(|r| {
            if r.is_closed(n) || r.empty.contains(&n) {
                Some(())
            } else {
                None
            }
        });
        let mut s = String::new();
        for c in self.arena.children(n).collect::<Vec<_>>() {
            s.push_str(&self.save(c, false));
        }
        s
    }

    /// `readString()`.
    pub fn read_string(&mut self) -> String {
        let Some(n) = self.current_node() else {
            return String::new();
        };
        if self.at_end {
            return String::new();
        }
        match self.arena.data(n) {
            NodeData::Element(_) => {
                self.wait(|r| {
                    if r.is_closed(n) || r.empty.contains(&n) {
                        Some(())
                    } else {
                        None
                    }
                });
                let mut s = String::new();
                let mut stack = vec![n];
                while let Some(x) = stack.pop() {
                    // Entity references are not expanded (observed on PHP 8.5).
                    if !matches!(self.arena.data(x), NodeData::EntityRef(_)) {
                        let kids: Vec<NodeId> = self.arena.children(x).collect();
                        for &k in kids.iter().rev() {
                            stack.push(k);
                        }
                    }
                    if x != n
                        && let NodeData::Text(t) | NodeData::CData(t) = self.arena.data(x)
                    {
                        s.push_str(t);
                    }
                }
                s
            }
            NodeData::Text(t) | NodeData::CData(t) => t.clone(),
            NodeData::Attribute(_) => self.arena.attr_value(n),
            _ => String::new(),
        }
    }

    /// `expand()`: the current node (complete), in the reader's arena. Use
    /// [`Arena::import_from`] to copy it into another document.
    pub fn expand(&mut self) -> Option<NodeId> {
        let n = self.current_node()?;
        if self.arena.is_element(n) {
            self.wait(|r| {
                if r.is_closed(n) || r.empty.contains(&n) {
                    Some(())
                } else {
                    None
                }
            });
        }
        Some(n)
    }
}
