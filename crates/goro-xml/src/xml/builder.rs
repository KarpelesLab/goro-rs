//! Building a document tree from parser events.

use super::parser::{AttrPiece, Parser, StartTag, Status};
use super::{EntityLoader, ParseOptions, Sink, opt};
use crate::error::{Domain, LEVEL_ERROR, XmlError};
use crate::tree::{
    Arena, AttrData, DocKind, DtdData, EntityDecl, NodeData, NodeId, NsDecl, QName, Str, XMLNS_NS,
};

/// A [`Sink`] that builds nodes under a parent in an arena.
pub struct TreeBuilder<'a> {
    /// The arena.
    pub arena: &'a mut Arena,
    /// The document the nodes belong to.
    pub doc: NodeId,
    cur: NodeId,
    noent: bool,
    modern: bool,
    /// Errors in order (parser errors and the builder's own, such as
    /// duplicate IDs).
    pub errors: Vec<XmlError>,
    line: u32,
    url: Option<String>,
    /// Nodes created at the top of the target (fragment parsing).
    pub top: Vec<NodeId>,
    target: NodeId,
}

impl<'a> TreeBuilder<'a> {
    /// A builder appending to `parent` (a document, element or fragment).
    pub fn new(arena: &'a mut Arena, parent: NodeId, opts: &ParseOptions) -> TreeBuilder<'a> {
        let doc = arena.doc_of(parent);
        TreeBuilder {
            arena,
            doc,
            cur: parent,
            noent: opts.has(opt::NOENT),
            modern: opts.modern,
            errors: Vec::new(),
            line: 0,
            url: opts.url.clone(),
            top: Vec::new(),
            target: parent,
        }
    }

    fn append(&mut self, n: NodeId) {
        self.arena.node_mut(n).line = self.line;
        self.arena.raw_append(self.cur, n);
        if self.cur == self.target {
            self.top.push(n);
        }
    }

    fn add_text(&mut self, text: &str) {
        if let Some(last) = self.arena.last_child(self.cur)
            && let NodeData::Text(t) = &mut self.arena.node_mut(last).data
        {
            t.push_str(text);
            return;
        }
        let t = self.arena.new_text(self.doc, text);
        self.append(t);
    }

    fn report(&mut self, code: u32, msg: String) {
        self.errors.push(XmlError {
            domain: Domain::Valid,
            code,
            level: LEVEL_ERROR,
            message: msg,
            file: self.url.clone(),
            line: self.line,
            column: 0,
        });
    }
}

fn doctype_node(arena: &mut Arena, doc: NodeId, dtd: &DtdData) -> NodeId {
    arena.create(doc, NodeData::Doctype(Box::new(dtd.clone())))
}

impl Sink for TreeBuilder<'_> {
    fn xml_decl(&mut self, version: &str, encoding: Option<&str>, standalone: i8) {
        let d = self.arena.doc_mut(self.doc);
        d.version = Some(version.to_string());
        d.encoding = encoding.map(str::to_string);
        d.standalone = standalone;
    }

    fn doctype(&mut self, dtd: &DtdData, external: Option<&DtdData>) {
        let n = doctype_node(self.arena, self.doc, dtd);
        self.append(n);
        self.arena.doc_mut(self.doc).doctype = Some(n);
        if let Some(e) = external {
            let x = doctype_node(self.arena, self.doc, e);
            self.arena.doc_mut(self.doc).ext_subset = Some(x);
        }
    }

    fn set_line(&mut self, line: u32) {
        self.line = line;
    }

    fn start_element(&mut self, tag: &StartTag<'_>) {
        self.line = tag.line;
        let el = self.arena.new_element(self.doc, tag.name.clone());
        if self.modern {
            for d in tag.ns_decls {
                let name = match &d.prefix {
                    Some(p) => QName {
                        prefix: Some("xmlns".into()),
                        local: p.clone(),
                        ns: Some(XMLNS_NS.into()),
                    },
                    None => QName {
                        prefix: None,
                        local: "xmlns".into(),
                        ns: Some(XMLNS_NS.into()),
                    },
                };
                let a = self.arena.new_attribute(self.doc, name, &d.uri);
                self.arena.raw_append_attr(el, a);
            }
        } else if let Some(e) = self.arena.element_mut(el) {
            e.ns_decls = tag.ns_decls.to_vec();
        }
        for at in tag.attrs {
            let a = self.arena.create(
                self.doc,
                NodeData::Attribute(Box::new(AttrData {
                    name: at.name.clone(),
                    is_id: at.is_id,
                    defaulted: at.defaulted,
                })),
            );
            match (&at.pieces, self.noent) {
                (Some(pieces), false) => {
                    for p in pieces {
                        let n = match p {
                            AttrPiece::Text(t) => self.arena.new_text(self.doc, t.as_str()),
                            AttrPiece::Entity(name) => self
                                .arena
                                .create(self.doc, NodeData::EntityRef(name.clone())),
                        };
                        self.arena.raw_append(a, n);
                    }
                }
                _ => {
                    if !at.value.is_empty() {
                        let t = self.arena.new_text(self.doc, at.value.as_str());
                        self.arena.raw_append(a, t);
                    }
                }
            }
            self.arena.node_mut(a).line = tag.line;
            if at.is_id {
                let ids = &self.arena.doc(self.doc).ids;
                if ids.contains_key(&at.value) {
                    let v = at.value.clone();
                    self.report(513, format!("ID {v} already defined\n"));
                    // The attribute stays an ID-typed attribute but the
                    // first element keeps the ID.
                    self.arena.attr_mut(a).unwrap().is_id = true;
                    // Append without registering.
                    let last = self.arena.element(el).and_then(|e| e.last_attr);
                    {
                        let node = self.arena.node_mut(a);
                        node.parent = Some(el);
                        node.prev = last;
                    }
                    match last {
                        Some(l) => self.arena.node_mut(l).next = Some(a),
                        None => self.arena.element_mut(el).unwrap().first_attr = Some(a),
                    }
                    self.arena.element_mut(el).unwrap().last_attr = Some(a);
                    continue;
                }
                self.arena
                    .doc_mut(self.doc)
                    .ids
                    .insert(at.value.clone(), el);
            }
            // raw_append_attr registers IDs only for connected elements;
            // the element is not attached yet, so register above.
            let last = self.arena.element(el).and_then(|e| e.last_attr);
            {
                let node = self.arena.node_mut(a);
                node.parent = Some(el);
                node.prev = last;
            }
            match last {
                Some(l) => self.arena.node_mut(l).next = Some(a),
                None => self.arena.element_mut(el).unwrap().first_attr = Some(a),
            }
            self.arena.element_mut(el).unwrap().last_attr = Some(a);
        }
        self.append(el);
        self.cur = el;
    }

    fn end_element(&mut self, _name: &QName) {
        if let Some(p) = self.arena.parent(self.cur) {
            self.cur = p;
        }
    }

    fn text(&mut self, text: &str) {
        self.add_text(text);
    }

    fn cdata(&mut self, text: &str) {
        let n = self
            .arena
            .create(self.doc, NodeData::CData(text.to_string()));
        self.append(n);
    }

    fn comment(&mut self, text: &str) {
        let n = self
            .arena
            .create(self.doc, NodeData::Comment(text.to_string()));
        self.append(n);
    }

    fn pi(&mut self, target: &str, data: &str) {
        let n = self.arena.create(
            self.doc,
            NodeData::Pi {
                target: target.into(),
                data: data.to_string(),
            },
        );
        self.append(n);
    }

    fn entity_start(&mut self, name: &str, decl: Option<&EntityDecl>) -> bool {
        if self.noent && decl.is_some() {
            return true;
        }
        let n = self
            .arena
            .create(self.doc, NodeData::EntityRef(Str::from(name)));
        self.append(n);
        self.cur = n;
        true
    }

    fn entity_end(&mut self, _name: &str) {
        if matches!(self.arena.data(self.cur), NodeData::EntityRef(_))
            && let Some(p) = self.arena.parent(self.cur)
        {
            self.cur = p;
        }
    }

    fn error(&mut self, e: &XmlError) {
        self.errors.push(e.clone());
    }
}

/// The result of building a document.
#[derive(Debug)]
pub struct Built {
    /// The document node (created even when parsing failed; free it with
    /// [`Arena::free_subtree`] if unused).
    pub doc: NodeId,
    /// Errors and warnings in order.
    pub errors: Vec<XmlError>,
    /// The document is well-formed (or was recovered with LIBXML_RECOVER).
    pub ok: bool,
}

/// Parses `input` into a new document in `arena`.
pub fn build(
    arena: &mut Arena,
    input: &[u8],
    opts: &ParseOptions,
    loader: Option<&mut dyn EntityLoader>,
) -> Built {
    let doc = arena.new_document(DocKind::Xml);
    arena.doc_mut(doc).modern = opts.modern;
    arena.doc_mut(doc).url = opts.url.clone();
    build_into(arena, doc, input, opts, loader)
}

/// Parses `input` into the (empty) document `doc`.
pub fn build_into(
    arena: &mut Arena,
    doc: NodeId,
    input: &[u8],
    opts: &ParseOptions,
    loader: Option<&mut dyn EntityLoader>,
) -> Built {
    let mut parser = Parser::new(opts.clone(), loader);
    let mut b = TreeBuilder::new(arena, doc, opts);
    let status = parser.parse(input, &mut b);
    let mut errors = std::mem::take(&mut b.errors);
    let mut ok = status == Status::Done;
    if ok || opts.has(opt::RECOVER) {
        if opts.has(opt::DTDVALID) {
            let v = super::validate::validate(arena, doc);
            for e in &v {
                errors.push(e.clone());
            }
        }
        if opts.has(opt::RECOVER) {
            ok = true;
        }
    }
    Built { doc, errors, ok }
}

/// Parses well-balanced content into `parent` (`DOMDocumentFragment::appendXML`
/// and similar). Namespaces in scope at `parent` apply; entities of the
/// owner document are known. Returns the nodes added at the top level,
/// the errors and whether the content was well-formed.
pub fn parse_fragment(
    arena: &mut Arena,
    parent: NodeId,
    input: &[u8],
    opts: &ParseOptions,
) -> (Vec<NodeId>, Vec<XmlError>, bool) {
    let mut parser = Parser::new_fragment(opts.clone(), None);
    // In-scope namespaces of the context.
    let ctx = match arena.data(parent) {
        NodeData::Element(_) => Some(parent),
        _ => arena.parent(parent).filter(|&p| arena.is_element(p)),
    };
    if let Some(el) = ctx {
        let mut decls: Vec<NsDecl> = arena.in_scope_namespaces(el);
        decls.reverse();
        for d in decls {
            if d.prefix.as_deref() != Some("xml") {
                parser.push_ns(d.prefix.as_deref(), &d.uri);
            }
        }
        if let Some(e) = arena.element(el)
            && e.name.prefix.is_none()
            && let Some(ns) = &e.name.ns
        {
            let ns = ns.clone();
            parser.push_ns(None, &ns);
        }
    }
    let doc = arena.doc_of(parent);
    let dtd = arena.doc(doc).doctype.and_then(|d| arena.dtd(d).cloned());
    let ext = arena
        .doc(doc)
        .ext_subset
        .and_then(|d| arena.dtd(d).cloned());
    parser.set_dtd(dtd, ext);
    let mut b = TreeBuilder::new(arena, parent, opts);
    let status = parser.parse(input, &mut b);
    let top = std::mem::take(&mut b.top);
    let errors = std::mem::take(&mut b.errors);
    (top, errors, status == Status::Done)
}
