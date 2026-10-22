//! In-memory XML tree modelled on libxml2's xmlNode/xmlDoc/xmlNs structures.
//!
//! All nodes of all documents live in one thread-local arena and are addressed
//! by `NodeId` (an index). Nodes are never freed (unlinked nodes stay in the
//! arena), which keeps ids held by PHP objects valid for the whole request.

use goro_core::object::PhpObject;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Weak;

pub type NodeId = usize;
pub type NsId = usize;

pub const XML_XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
pub const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";

/// Node flag: the content pointer is NULL (e.g. `new DOMComment()`), as
/// opposed to an empty string; such comments are not serialized.
pub const F_NULL_CONTENT: u32 = 1;
/// Node flag: created by the modern (Dom\) API without a document; its
/// wrapper uses the Dom\ classes.
pub const F_MODERN: u32 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NType {
    Element = 1,
    Attribute = 2,
    Text = 3,
    CData = 4,
    EntityRef = 5,
    Entity = 6,
    Pi = 7,
    Comment = 8,
    Document = 9,
    DocumentType = 10,
    DocumentFrag = 11,
    Notation = 12,
    HtmlDocument = 13,
    Dtd = 14,
    ElementDecl = 15,
    AttributeDecl = 16,
    EntityDecl = 17,
    NamespaceDecl = 18,
}

impl NType {
    pub fn code(self) -> i64 {
        self as i64
    }
}

#[derive(Clone, Debug)]
pub struct Ns {
    pub href: String,
    pub prefix: Option<String>,
    /// Element (or document for the implicit xml namespace) declaring it.
    pub owner: Option<NodeId>,
}

#[derive(Clone, Debug, Default)]
pub struct DocInfo {
    pub version: Option<String>,
    pub encoding: Option<String>,
    /// -1: not specified, 0: no, 1: yes
    pub standalone: i32,
    pub url: Option<String>,
    pub int_subset: Option<NodeId>,
    pub ext_subset: Option<NodeId>,
    pub ids: HashMap<String, NodeId>,
    pub html: bool,
    pub xml_ns: Option<NsId>,
    /// Parser options the document was loaded with.
    pub parse_options: i64,
}

#[derive(Clone, Debug, Default)]
pub struct DtdInfo {
    pub external_id: Option<String>,
    pub system_id: Option<String>,
    pub entities: Vec<(String, NodeId)>,
    pub pentities: Vec<(String, NodeId)>,
    pub notations: Vec<(String, Option<String>, Option<String>)>,
    /// Raw text of the internal subset (for serialization of declarations we
    /// do not model precisely).
    pub attr_defaults: Vec<AttrDecl>,
    /// Element declaration table (dtd->elements), including the
    /// XML_ELEMENT_TYPE_UNDEFINED placeholders created by ATTLIST.
    pub elements: Vec<ElemDecl>,
    /// Lookup tables over attr_defaults (see find_attr_decl), built on
    /// first use and again once declarations were added.
    pub attr_cache: RefCell<AttrCache>,
}

/// Indexes of DtdInfo::attr_defaults by hashed names; entries are
/// candidates (in declaration order) that the lookups check.
#[derive(Clone, Debug, Default)]
pub struct AttrCache {
    /// attr_defaults.len() when built.
    len: usize,
    /// By (element, name, prefix).
    by_name: HashMap<u64, Vec<usize>>,
    /// The ID attributes, by (element, attribute qname).
    ids: HashMap<u64, Vec<usize>>,
}

/// FNV-1a over the parts, each terminated (`None` hashes apart from "").
fn names_hash(parts: &[Option<&str>]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        let (bytes, end): (&[u8], u8) = match p {
            Some(s) => (s.as_bytes(), 0),
            None => (b"", 1),
        };
        for &b in bytes.iter().chain(std::iter::once(&end)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

impl DtdInfo {
    fn with_attr_cache<R>(&self, f: impl FnOnce(&AttrCache) -> R) -> R {
        let mut c = self.attr_cache.borrow_mut();
        if c.len > self.attr_defaults.len() {
            *c = AttrCache::default();
        }
        for (i, a) in self.attr_defaults.iter().enumerate().skip(c.len) {
            let k = names_hash(&[Some(&a.elem), Some(&a.name), a.prefix.as_deref()]);
            c.by_name.entry(k).or_default().push(i);
            if a.atype == "ID" {
                let full = match &a.prefix {
                    Some(p) => format!("{}:{}", p, a.name),
                    None => a.name.clone(),
                };
                let k = names_hash(&[Some(&a.elem), Some(&full)]);
                c.ids.entry(k).or_default().push(i);
            }
        }
        c.len = self.attr_defaults.len();
        f(&c)
    }

    /// The first declaration of attribute `name` (`prefix`) of element
    /// `elem` (xmlGetDtdQAttrDesc).
    pub fn find_attr_decl(&self, elem: &str, name: &str, prefix: Option<&str>) -> Option<usize> {
        if self.attr_defaults.is_empty() {
            return None;
        }
        let k = names_hash(&[Some(elem), Some(name), prefix]);
        self.with_attr_cache(|c| {
            c.by_name.get(&k)?.iter().copied().find(|&i| {
                let a = &self.attr_defaults[i];
                a.elem == elem && a.name == name && a.prefix.as_deref() == prefix
            })
        })
    }

    /// Whether attribute `attr_q` (its qualified name) of element `elem_q`
    /// is declared of type ID.
    pub fn is_id_decl(&self, elem_q: &str, attr_q: &str) -> bool {
        if self.attr_defaults.is_empty() {
            return false;
        }
        let k = names_hash(&[Some(elem_q), Some(attr_q)]);
        self.with_attr_cache(|c| {
            c.ids.get(&k).is_some_and(|v| {
                v.iter().any(|&i| {
                    let a = &self.attr_defaults[i];
                    let full_eq = match &a.prefix {
                        Some(p) => {
                            attr_q.len() == p.len() + 1 + a.name.len()
                                && attr_q.starts_with(p.as_str())
                                && attr_q.as_bytes()[p.len()] == b':'
                                && attr_q.ends_with(a.name.as_str())
                        }
                        None => a.name == attr_q,
                    };
                    a.elem == elem_q && full_eq && a.atype == "ID"
                })
            })
        })
    }
}

/// xmlElement: an element declaration as kept in the DTD hash table.
#[derive(Clone, Debug, Default)]
pub struct ElemDecl {
    /// Local name.
    pub name: String,
    pub prefix: Option<String>,
    /// 0 UNDEFINED, 1 EMPTY, 2 ANY, 3 MIXED, 4 ELEMENT
    pub etype: u8,
    pub content: Option<crate::parser::ElemContent>,
    /// Attribute declarations of this element (elem->attributes, nexth
    /// order) as (dtd node, index in that dtd's attr_defaults).
    pub attrs: Vec<(NodeId, usize)>,
    /// The ElementDecl node linked in the DTD.
    pub node: Option<NodeId>,
    /// Compiled content model (elem->contModel).
    pub model: Option<std::rc::Rc<crate::valid::ContModel>>,
}

#[derive(Clone, Debug, Default)]
pub struct AttrDecl {
    pub elem: String,
    pub name: String,
    pub prefix: Option<String>,
    /// CDATA, ID, IDREF, ... enumeration text
    pub atype: String,
    /// #REQUIRED, #IMPLIED, #FIXED or empty
    pub def: String,
    pub default: Option<String>,
    /// XML_ATTRIBUTE_* type (1 CDATA .. 10 NOTATION)
    pub atype_n: u8,
    /// XML_ATTRIBUTE_* default (1 NONE, 2 REQUIRED, 3 IMPLIED, 4 FIXED)
    pub def_n: u8,
    /// Enumerated values.
    pub tree: Vec<String>,
    /// The AttributeDecl node linked in the DTD.
    pub node: Option<NodeId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityType {
    InternalGeneral = 1,
    ExternalGeneralParsed = 2,
    ExternalGeneralUnparsed = 3,
    InternalParameter = 4,
    ExternalParameter = 5,
    InternalPredefined = 6,
}

#[derive(Clone, Debug)]
pub struct EntityInfo {
    pub etype: EntityType,
    pub external_id: Option<String>,
    pub system_id: Option<String>,
    pub notation: Option<String>,
    /// Replacement text (content) for internal entities.
    pub orig: Option<String>,
    /// Whether children have been parsed from `content`.
    pub checked: bool,
}

#[derive(Clone, Debug, Default)]
pub enum Extra {
    #[default]
    None,
    Doc(Box<DocInfo>),
    Dtd(Box<DtdInfo>),
    Entity(Box<EntityInfo>),
    ElementDecl(String),
    AttributeDecl(Box<AttrDecl>),
    Notation(Option<String>, Option<String>),
}

#[derive(Clone, Debug)]
pub struct Node {
    pub t: NType,
    pub name: String,
    pub content: String,
    pub parent: Option<NodeId>,
    pub children: Option<NodeId>,
    pub last: Option<NodeId>,
    pub next: Option<NodeId>,
    pub prev: Option<NodeId>,
    pub properties: Option<NodeId>,
    pub ns: Option<NsId>,
    pub ns_def: Vec<NsId>,
    pub doc: Option<NodeId>,
    pub line: u32,
    pub extra: Extra,
    /// Attribute type (1 = CDATA, 2 = ID, ...) for attributes.
    pub atype: u8,
    /// Cached PHP wrapper object (DOM).
    pub wrapper: Option<Weak<RefCell<PhpObject>>>,
    /// For text nodes created by the parser: whether the node is a
    /// "noenc" text (already escaped). Unused for now.
    pub flags: u32,
    /// The template contents (a document fragment) of an HTML <template>
    /// element of a modern document (php_dom_*_templated_content).
    pub template: Option<NodeId>,
}

impl Node {
    fn new(t: NType, name: &str) -> Node {
        Node {
            t,
            name: name.to_string(),
            content: String::new(),
            parent: None,
            children: None,
            last: None,
            next: None,
            prev: None,
            properties: None,
            ns: None,
            ns_def: Vec::new(),
            doc: None,
            line: 0,
            extra: Extra::None,
            atype: 0,
            wrapper: None,
            flags: 0,
            template: None,
        }
    }
}

#[derive(Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub nss: Vec<Ns>,
    /// predefined entity declarations (lazily created)
    pub predef: Vec<(String, NodeId)>,
    /// Slots given back with `release`, reused by `alloc`.
    pub free_slots: Vec<NodeId>,
}

thread_local! {
    static TREE: RefCell<Tree> = RefCell::new(Tree::default());
}

/// Run `f` with mutable access to the global tree. Must not be re-entered.
pub fn with<R>(f: impl FnOnce(&mut Tree) -> R) -> R {
    TREE.with(|t| f(&mut t.borrow_mut()))
}

/// Like `with`, but returns None instead of panicking when the tree is
/// already borrowed (or being torn down).
pub fn try_with<R>(f: impl FnOnce(&mut Tree) -> R) -> Option<R> {
    TREE.try_with(|t| t.try_borrow_mut().ok().map(|mut b| f(&mut b)))
        .ok()
        .flatten()
}

pub fn is_blank_char(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}

pub fn is_blank_str(s: &str) -> bool {
    s.bytes().all(is_blank_char)
}

impl Tree {
    // ------------------------------------------------------------------
    // Allocation

    pub fn alloc(&mut self, n: Node) -> NodeId {
        if let Some(id) = self.free_slots.pop() {
            self.nodes[id] = n;
            return id;
        }
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    /// Give back the slot of an unlinked node that nothing (no PHP object,
    /// no other node) refers to any more, e.g. the nodes an XMLReader is
    /// done with. Its id may be reused by a later allocation.
    pub fn release(&mut self, id: NodeId) {
        self.nodes[id] = Node::new(NType::Text, "");
        self.free_slots.push(id);
    }

    pub fn n(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub fn nm(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id]
    }

    pub fn new_doc(&mut self, version: Option<&str>) -> NodeId {
        let mut n = Node::new(NType::Document, "");
        n.extra = Extra::Doc(Box::new(DocInfo {
            version: version.map(|s| s.to_string()),
            standalone: -1,
            ..Default::default()
        }));
        let id = self.alloc(n);
        self.nodes[id].doc = Some(id);
        id
    }

    pub fn new_html_doc(&mut self) -> NodeId {
        let mut n = Node::new(NType::HtmlDocument, "");
        n.extra = Extra::Doc(Box::new(DocInfo {
            version: None,
            standalone: -1,
            html: true,
            ..Default::default()
        }));
        let id = self.alloc(n);
        self.nodes[id].doc = Some(id);
        id
    }

    pub fn doc_info(&self, doc: NodeId) -> Option<&DocInfo> {
        match &self.nodes[doc].extra {
            Extra::Doc(d) => Some(d),
            _ => None,
        }
    }

    pub fn doc_info_mut(&mut self, doc: NodeId) -> Option<&mut DocInfo> {
        match &mut self.nodes[doc].extra {
            Extra::Doc(d) => Some(d),
            _ => None,
        }
    }

    pub fn dtd_info(&self, dtd: NodeId) -> Option<&DtdInfo> {
        match &self.nodes[dtd].extra {
            Extra::Dtd(d) => Some(d),
            _ => None,
        }
    }

    pub fn dtd_info_mut(&mut self, dtd: NodeId) -> Option<&mut DtdInfo> {
        match &mut self.nodes[dtd].extra {
            Extra::Dtd(d) => Some(d),
            _ => None,
        }
    }

    pub fn entity_info(&self, e: NodeId) -> Option<&EntityInfo> {
        match &self.nodes[e].extra {
            Extra::Entity(d) => Some(d),
            _ => None,
        }
    }

    pub fn new_node(&mut self, doc: Option<NodeId>, t: NType, name: &str) -> NodeId {
        let mut n = Node::new(t, name);
        n.doc = doc;
        self.alloc(n)
    }

    pub fn new_element(&mut self, doc: Option<NodeId>, name: &str) -> NodeId {
        self.new_node(doc, NType::Element, name)
    }

    pub fn new_text(&mut self, doc: Option<NodeId>, content: &str) -> NodeId {
        let id = self.new_node(doc, NType::Text, "text");
        self.nodes[id].content = content.to_string();
        id
    }

    pub fn new_cdata(&mut self, doc: Option<NodeId>, content: &str) -> NodeId {
        let id = self.new_node(doc, NType::CData, "");
        self.nodes[id].content = content.to_string();
        id
    }

    pub fn new_comment(&mut self, doc: Option<NodeId>, content: &str) -> NodeId {
        let id = self.new_node(doc, NType::Comment, "comment");
        self.nodes[id].content = content.to_string();
        id
    }

    pub fn new_pi(&mut self, doc: Option<NodeId>, target: &str, content: &str) -> NodeId {
        let id = self.new_node(doc, NType::Pi, target);
        self.nodes[id].content = content.to_string();
        id
    }

    pub fn new_fragment(&mut self, doc: Option<NodeId>) -> NodeId {
        self.new_node(doc, NType::DocumentFrag, "")
    }

    /// xmlNewReference: an entity reference node whose children/last point
    /// at the entity declaration (if known).
    pub fn new_entity_ref(&mut self, doc: Option<NodeId>, name: &str) -> NodeId {
        let name = name.trim_start_matches('&').trim_end_matches(';');
        let id = self.new_node(doc, NType::EntityRef, name);
        if let Some(ent) = self.get_doc_entity_any(doc, name) {
            self.nodes[id].children = Some(ent);
            self.nodes[id].last = Some(ent);
        }
        id
    }

    /// xmlNewDocProp / xmlNewNsProp without linking.
    pub fn new_attr_node(
        &mut self,
        doc: Option<NodeId>,
        ns: Option<NsId>,
        name: &str,
        value: Option<&str>,
    ) -> NodeId {
        let id = self.new_node(doc, NType::Attribute, name);
        self.nodes[id].ns = ns;
        if let Some(v) = value {
            if !v.is_empty() {
                let t = self.new_text(doc, v);
                self.link_child_raw(id, t);
            }
        }
        id
    }

    pub fn new_ns_raw(&mut self, href: &str, prefix: Option<&str>, owner: Option<NodeId>) -> NsId {
        self.nss.push(Ns {
            href: href.to_string(),
            prefix: prefix.map(|s| s.to_string()),
            owner,
        });
        self.nss.len() - 1
    }

    /// xmlNewNs: declare a namespace on `node`. Returns None if the prefix is
    /// already declared on that node.
    pub fn new_ns(
        &mut self,
        node: Option<NodeId>,
        href: &str,
        prefix: Option<&str>,
    ) -> Option<NsId> {
        if prefix == Some("xml") {
            return None;
        }
        if let Some(n) = node {
            for &ns in &self.nodes[n].ns_def {
                if self.nss[ns].prefix.as_deref() == prefix {
                    return None;
                }
            }
        }
        let id = self.new_ns_raw(href, prefix, node);
        if let Some(n) = node {
            if self.nodes[n].t == NType::Element {
                self.nodes[n].ns_def.push(id);
            }
        }
        Some(id)
    }

    // ------------------------------------------------------------------
    // Navigation helpers

    pub fn children_of(&self, id: NodeId) -> Vec<NodeId> {
        let mut v = Vec::new();
        if matches!(
            self.nodes[id].t,
            NType::EntityRef
                | NType::Attribute
                | NType::Element
                | NType::Document
                | NType::HtmlDocument
                | NType::DocumentFrag
                | NType::Entity
                | NType::EntityDecl
                | NType::Dtd
        ) {
            if self.nodes[id].t == NType::EntityRef {
                return v;
            }
            let mut c = self.nodes[id].children;
            while let Some(x) = c {
                v.push(x);
                c = self.nodes[x].next;
            }
        }
        v
    }

    pub fn attrs_of(&self, id: NodeId) -> Vec<NodeId> {
        let mut v = Vec::new();
        if self.nodes[id].t != NType::Element {
            return v;
        }
        let mut c = self.nodes[id].properties;
        while let Some(x) = c {
            v.push(x);
            c = self.nodes[x].next;
        }
        v
    }

    pub fn doc_of(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].doc
    }

    pub fn is_doc(&self, id: NodeId) -> bool {
        matches!(self.nodes[id].t, NType::Document | NType::HtmlDocument)
    }

    pub fn root_element(&self, doc: NodeId) -> Option<NodeId> {
        let mut c = self.nodes[doc].children;
        while let Some(x) = c {
            if self.nodes[x].t == NType::Element {
                return Some(x);
            }
            c = self.nodes[x].next;
        }
        None
    }

    pub fn int_subset(&self, doc: NodeId) -> Option<NodeId> {
        self.doc_info(doc).and_then(|d| d.int_subset)
    }

    // ------------------------------------------------------------------
    // Linking

    /// Append `child` as the last child of `parent` with no text merging.
    pub fn link_child_raw(&mut self, parent: NodeId, child: NodeId) {
        if self.nodes[child].t == NType::Attribute && self.nodes[parent].t == NType::Element {
            // attributes go to the property list
            self.nodes[child].parent = Some(parent);
            self.nodes[child].next = None;
            match self.nodes[parent].properties {
                None => {
                    self.nodes[child].prev = None;
                    self.nodes[parent].properties = Some(child);
                }
                Some(first) => {
                    let mut last = first;
                    while let Some(n) = self.nodes[last].next {
                        last = n;
                    }
                    self.nodes[last].next = Some(child);
                    self.nodes[child].prev = Some(last);
                }
            }
            return;
        }
        self.nodes[child].parent = Some(parent);
        self.nodes[child].next = None;
        match self.nodes[parent].last {
            None => {
                self.nodes[child].prev = None;
                self.nodes[parent].children = Some(child);
                self.nodes[parent].last = Some(child);
            }
            Some(last) => {
                self.nodes[last].next = Some(child);
                self.nodes[child].prev = Some(last);
                self.nodes[parent].last = Some(child);
            }
        }
    }

    /// Set the document of a subtree (xmlSetTreeDoc).
    pub fn set_tree_doc(&mut self, id: NodeId, doc: Option<NodeId>) {
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if self.nodes[n].doc == doc && n != id {
                // still need to descend: children may differ
            }
            self.nodes[n].doc = doc;
            if self.nodes[n].t == NType::EntityRef {
                // re-resolve the entity in the new document
                let name = self.nodes[n].name.clone();
                let ent = doc.and_then(|d| self.get_doc_entity(d, &name));
                self.nodes[n].children = ent;
                self.nodes[n].last = ent;
                continue;
            }
            if self.nodes[n].t == NType::Element {
                let mut a = self.nodes[n].properties;
                while let Some(x) = a {
                    stack.push(x);
                    a = self.nodes[x].next;
                }
            }
            let mut c = self.nodes[n].children;
            while let Some(x) = c {
                stack.push(x);
                c = self.nodes[x].next;
            }
        }
    }

    /// xmlUnlinkNode
    pub fn unlink(&mut self, id: NodeId) {
        let t = self.nodes[id].t;
        if t == NType::Dtd {
            if let Some(doc) = self.nodes[id].doc {
                if let Some(di) = self.doc_info_mut(doc) {
                    if di.int_subset == Some(id) {
                        di.int_subset = None;
                    }
                    if di.ext_subset == Some(id) {
                        di.ext_subset = None;
                    }
                }
            }
        }
        let parent = self.nodes[id].parent;
        let prev = self.nodes[id].prev;
        let next = self.nodes[id].next;
        if let Some(p) = parent {
            if t == NType::Attribute {
                if self.nodes[p].properties == Some(id) {
                    self.nodes[p].properties = next;
                }
            } else {
                if self.nodes[p].children == Some(id) {
                    self.nodes[p].children = next;
                }
                if self.nodes[p].last == Some(id) {
                    self.nodes[p].last = prev;
                }
            }
        }
        if let Some(pv) = prev {
            self.nodes[pv].next = next;
        }
        if let Some(nx) = next {
            self.nodes[nx].prev = prev;
        }
        self.nodes[id].parent = None;
        self.nodes[id].prev = None;
        self.nodes[id].next = None;
    }

    /// Insert `new` before `cur` (no merging).
    pub fn insert_before_raw(&mut self, cur: NodeId, new: NodeId) {
        let parent = self.nodes[cur].parent;
        let prev = self.nodes[cur].prev;
        self.nodes[new].parent = parent;
        self.nodes[new].next = Some(cur);
        self.nodes[new].prev = prev;
        self.nodes[cur].prev = Some(new);
        match prev {
            Some(pv) => self.nodes[pv].next = Some(new),
            None => {
                if let Some(p) = parent {
                    if self.nodes[new].t == NType::Attribute {
                        self.nodes[p].properties = Some(new);
                    } else {
                        self.nodes[p].children = Some(new);
                    }
                }
            }
        }
    }

    /// Insert `new` after `cur` (no merging).
    pub fn insert_after_raw(&mut self, cur: NodeId, new: NodeId) {
        let parent = self.nodes[cur].parent;
        let next = self.nodes[cur].next;
        self.nodes[new].parent = parent;
        self.nodes[new].prev = Some(cur);
        self.nodes[new].next = next;
        self.nodes[cur].next = Some(new);
        match next {
            Some(nx) => self.nodes[nx].prev = Some(new),
            None => {
                if let Some(p) = parent {
                    if self.nodes[new].t != NType::Attribute {
                        self.nodes[p].last = Some(new);
                    }
                }
            }
        }
    }

    /// xmlAddChild: appends, merging text nodes into an adjacent text node
    /// and replacing same-named attributes. Returns the node that ended up in
    /// the tree (which may be the merged-into node).
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) -> NodeId {
        if self.nodes[child].parent.is_some() {
            self.unlink(child);
        }
        let pdoc = self.nodes[parent].doc;
        if self.nodes[child].doc != pdoc {
            self.set_tree_doc(child, pdoc);
        }
        if self.nodes[child].t == NType::Text {
            if self.nodes[parent].t == NType::Text {
                let c = self.nodes[child].content.clone();
                self.nodes[parent].content.push_str(&c);
                return parent;
            }
            if let Some(last) = self.nodes[parent].last {
                if self.nodes[last].t == NType::Text
                    && self.nodes[last].name == self.nodes[child].name
                {
                    let c = self.nodes[child].content.clone();
                    self.nodes[last].content.push_str(&c);
                    return last;
                }
            }
        }
        if self.nodes[child].t == NType::Attribute {
            if self.nodes[parent].t != NType::Element {
                return child;
            }
            let name = self.nodes[child].name.clone();
            let href = self.nodes[child].ns.map(|n| self.nss[n].href.clone());
            if let Some(old) = self.has_ns_prop(parent, &name, href.as_deref()) {
                if old != child {
                    self.remove_id_for_attr(old);
                    self.unlink(old);
                }
            }
            self.link_child_raw(parent, child);
            return child;
        }
        self.link_child_raw(parent, child);
        child
    }

    /// xmlAddNextSibling with text merging
    pub fn add_next_sibling(&mut self, cur: NodeId, elem: NodeId) -> NodeId {
        if self.nodes[elem].parent.is_some()
            || self.nodes[elem].prev.is_some()
            || self.nodes[elem].next.is_some()
        {
            self.unlink(elem);
        }
        if self.nodes[elem].t == NType::Text {
            if self.nodes[cur].t == NType::Text {
                let c = self.nodes[elem].content.clone();
                self.nodes[cur].content.push_str(&c);
                return cur;
            }
            if let Some(nx) = self.nodes[cur].next {
                if self.nodes[nx].t == NType::Text {
                    let c = self.nodes[elem].content.clone();
                    let old = std::mem::take(&mut self.nodes[nx].content);
                    self.nodes[nx].content = c + &old;
                    return nx;
                }
            }
        }
        let doc = self.nodes[cur].doc;
        if self.nodes[elem].doc != doc {
            self.set_tree_doc(elem, doc);
        }
        self.insert_after_raw(cur, elem);
        elem
    }

    /// xmlAddPrevSibling with text merging
    pub fn add_prev_sibling(&mut self, cur: NodeId, elem: NodeId) -> NodeId {
        if self.nodes[elem].parent.is_some()
            || self.nodes[elem].prev.is_some()
            || self.nodes[elem].next.is_some()
        {
            self.unlink(elem);
        }
        if self.nodes[elem].t == NType::Text {
            if self.nodes[cur].t == NType::Text {
                let c = self.nodes[elem].content.clone();
                let old = std::mem::take(&mut self.nodes[cur].content);
                self.nodes[cur].content = c + &old;
                return cur;
            }
            if let Some(pv) = self.nodes[cur].prev {
                if self.nodes[pv].t == NType::Text {
                    let c = self.nodes[elem].content.clone();
                    self.nodes[pv].content.push_str(&c);
                    return pv;
                }
            }
        }
        let doc = self.nodes[cur].doc;
        if self.nodes[elem].doc != doc {
            self.set_tree_doc(elem, doc);
        }
        self.insert_before_raw(cur, elem);
        elem
    }

    // ------------------------------------------------------------------
    // Namespaces

    /// The implicit `xml` namespace of a document.
    pub fn xml_ns(&mut self, doc: Option<NodeId>) -> NsId {
        if let Some(d) = doc {
            if let Some(ns) = self.doc_info(d).and_then(|i| i.xml_ns) {
                return ns;
            }
            let ns = self.new_ns_raw(XML_XML_NAMESPACE, Some("xml"), Some(d));
            if let Some(i) = self.doc_info_mut(d) {
                i.xml_ns = Some(ns);
            }
            return ns;
        }
        self.new_ns_raw(XML_XML_NAMESPACE, Some("xml"), None)
    }

    /// xmlSearchNs: find the namespace bound to `prefix` in scope of `node`.
    pub fn search_ns(&mut self, node: NodeId, prefix: Option<&str>) -> Option<NsId> {
        if prefix == Some("xml") {
            let doc = self.nodes[node].doc;
            return Some(self.xml_ns(doc));
        }
        let mut cur = Some(node);
        while let Some(c) = cur {
            let t = self.nodes[c].t;
            if matches!(t, NType::EntityRef | NType::EntityDecl | NType::Entity) {
                return None;
            }
            if t == NType::Element {
                for &ns in &self.nodes[c].ns_def {
                    if self.nss[ns].prefix.as_deref() == prefix {
                        return Some(ns);
                    }
                }
                if c != node {
                    if let Some(ns) = self.nodes[c].ns {
                        if self.nss[ns].prefix.as_deref() == prefix {
                            return Some(ns);
                        }
                    }
                }
            }
            cur = self.nodes[c].parent;
        }
        None
    }

    /// Non-mutating search (does not create the xml namespace).
    pub fn search_ns_ro(&self, node: NodeId, prefix: Option<&str>) -> Option<NsId> {
        let mut cur = Some(node);
        while let Some(c) = cur {
            let t = self.nodes[c].t;
            if t == NType::Element {
                for &ns in &self.nodes[c].ns_def {
                    if self.nss[ns].prefix.as_deref() == prefix {
                        return Some(ns);
                    }
                }
                if c != node {
                    if let Some(ns) = self.nodes[c].ns {
                        if self.nss[ns].prefix.as_deref() == prefix {
                            return Some(ns);
                        }
                    }
                }
            }
            cur = self.nodes[c].parent;
        }
        None
    }

    /// xmlSearchNsByHref
    pub fn search_ns_by_href(&mut self, node: NodeId, href: &str) -> Option<NsId> {
        if href == XML_XML_NAMESPACE {
            let doc = self.nodes[node].doc;
            return Some(self.xml_ns(doc));
        }
        let is_attr = self.nodes[node].t == NType::Attribute;
        let orig = node;
        let mut cur = Some(node);
        while let Some(c) = cur {
            let t = self.nodes[c].t;
            if matches!(t, NType::EntityRef | NType::EntityDecl | NType::Entity) {
                return None;
            }
            if t == NType::Element {
                for &ns in &self.nodes[c].ns_def {
                    if self.nss[ns].href == href && (!is_attr || self.nss[ns].prefix.is_some()) {
                        // check it is not shadowed
                        let p = self.nss[ns].prefix.clone();
                        if self.search_ns_ro(orig, p.as_deref()) == Some(ns) {
                            return Some(ns);
                        }
                    }
                }
                if c != orig {
                    if let Some(ns) = self.nodes[c].ns {
                        if self.nss[ns].href == href && (!is_attr || self.nss[ns].prefix.is_some())
                        {
                            let p = self.nss[ns].prefix.clone();
                            if self.search_ns_ro(orig, p.as_deref()) == Some(ns) {
                                return Some(ns);
                            }
                        }
                    }
                }
            }
            cur = self.nodes[c].parent;
        }
        None
    }

    pub fn ns_href(&self, ns: Option<NsId>) -> Option<&str> {
        ns.map(|n| self.nss[n].href.as_str())
    }

    pub fn ns_prefix(&self, ns: Option<NsId>) -> Option<&str> {
        ns.and_then(|n| self.nss[n].prefix.as_deref())
    }

    /// Qualified name: prefix:name
    pub fn qname(&self, id: NodeId) -> String {
        let n = &self.nodes[id];
        if matches!(n.t, NType::Element | NType::Attribute) {
            if let Some(p) = self.ns_prefix(n.ns) {
                return format!("{}:{}", p, n.name);
            }
        }
        n.name.clone()
    }

    // ------------------------------------------------------------------
    // Attributes

    /// xmlHasNsProp: find attribute by local name and namespace href
    /// (None = no namespace).
    pub fn has_ns_prop(&self, node: NodeId, name: &str, href: Option<&str>) -> Option<NodeId> {
        if self.nodes[node].t != NType::Element {
            return None;
        }
        let mut a = self.nodes[node].properties;
        while let Some(x) = a {
            let n = &self.nodes[x];
            if n.name == name {
                let h = n.ns.map(|ns| self.nss[ns].href.as_str());
                if h == href {
                    return Some(x);
                }
            }
            a = n.next;
        }
        None
    }

    /// xmlHasProp: by name regardless of namespace
    pub fn has_prop(&self, node: NodeId, name: &str) -> Option<NodeId> {
        if self.nodes[node].t != NType::Element {
            return None;
        }
        let mut a = self.nodes[node].properties;
        while let Some(x) = a {
            if self.nodes[x].name == name {
                return Some(x);
            }
            a = self.nodes[x].next;
        }
        None
    }

    /// Find attribute by qualified name (prefix:local) as written.
    pub fn has_qname_prop(&self, node: NodeId, qname: &str) -> Option<NodeId> {
        self.attrs_of(node)
            .into_iter()
            .find(|&a| self.qname(a) == qname)
    }

    /// xmlGetNoNsProp
    pub fn get_no_ns_prop(&self, node: NodeId, name: &str) -> Option<String> {
        self.has_ns_prop(node, name, None)
            .map(|a| self.node_get_content(a))
    }

    /// xmlGetProp
    pub fn get_prop(&self, node: NodeId, name: &str) -> Option<String> {
        self.has_prop(node, name).map(|a| self.node_get_content(a))
    }

    /// xmlSetNsProp: create or replace the attribute.
    pub fn set_ns_prop(
        &mut self,
        node: NodeId,
        ns: Option<NsId>,
        name: &str,
        value: &str,
    ) -> NodeId {
        let href = ns.map(|n| self.nss[n].href.clone());
        if let Some(a) = self.has_ns_prop(node, name, href.as_deref()) {
            // An ID stays one (its new value registered if free).
            let id = self.nodes[a].atype == 2;
            self.remove_id_for_attr(a);
            self.nodes[a].ns = ns;
            self.set_children_text(a, value);
            if id {
                self.add_id_safe(a, value);
                self.nodes[a].atype = 2;
            }
            return a;
        }
        let doc = self.nodes[node].doc;
        let a = self.new_attr_node(doc, ns, name, Some(value));
        self.link_child_raw(node, a);
        if self.is_id(node, a) {
            self.add_id_safe(a, value);
        }
        a
    }

    /// xmlIsID: xml:id, "id" (and a@name) in HTML documents, or an
    /// attribute declared with type ID in the DTD.
    pub fn is_id(&self, el: NodeId, at: NodeId) -> bool {
        let aname = &self.nodes[at].name;
        if aname == "id" && self.ns_prefix(self.nodes[at].ns) == Some("xml") {
            return true;
        }
        let d = match self.nodes[at].doc.or(self.nodes[el].doc) {
            Some(d) => d,
            None => return false,
        };
        let info = match self.doc_info(d) {
            Some(i) => i,
            None => return false,
        };
        if self.nodes[d].t == NType::HtmlDocument || info.html {
            if aname == "id" {
                return true;
            }
            return self.nodes[el].t == NType::Element
                && aname == "name"
                && self.nodes[el].name == "a";
        }
        let elem_q = self.qname(el);
        let attr_q = self.qname(at);
        for sub in [info.int_subset, info.ext_subset].into_iter().flatten() {
            if let Some(dd) = self.dtd_info(sub) {
                if dd.is_id_decl(&elem_q, &attr_q) {
                    return true;
                }
            }
        }
        false
    }

    /// xmlAddIDSafe: mark the attribute as an ID and register it (an
    /// existing registration of the same value wins).
    /// xmlAddIDSafe: registers `value` as the ID of attribute `at` and marks
    /// it an ID; an ID already defined in the document is left alone (false).
    pub fn add_id_safe(&mut self, at: NodeId, value: &str) -> bool {
        let Some(d) = self.nodes[at].doc else {
            return false;
        };
        let Some(di) = self.doc_info_mut(d) else {
            return false;
        };
        if di.ids.contains_key(value) {
            return false;
        }
        di.ids.insert(value.to_string(), at);
        self.nodes[at].atype = 2;
        true
    }

    /// xmlNewProp + link (does not replace existing)
    pub fn new_prop(
        &mut self,
        node: NodeId,
        ns: Option<NsId>,
        name: &str,
        value: Option<&str>,
    ) -> NodeId {
        let doc = self.nodes[node].doc;
        let a = self.new_attr_node(doc, ns, name, value);
        self.link_child_raw(node, a);
        if let Some(v) = value {
            if self.is_id(node, a) {
                self.add_id_safe(a, v);
            }
        }
        a
    }

    pub fn remove_id_for_attr(&mut self, attr: NodeId) {
        if self.nodes[attr].atype == 2 {
            if let Some(doc) = self.nodes[attr].doc {
                let v = self.node_get_content(attr);
                if let Some(di) = self.doc_info_mut(doc) {
                    if di.ids.get(&v) == Some(&attr) {
                        di.ids.remove(&v);
                    }
                }
            }
            self.nodes[attr].atype = 0;
        }
    }

    /// Replace all children of `id` with a single text node.
    pub fn set_children_text(&mut self, id: NodeId, value: &str) {
        let kids = self.children_of(id);
        for k in kids {
            self.unlink(k);
        }
        self.nodes[id].children = None;
        self.nodes[id].last = None;
        if !value.is_empty() {
            let doc = self.nodes[id].doc;
            let t = self.new_text(doc, value);
            self.link_child_raw(id, t);
        }
    }

    // ------------------------------------------------------------------
    // Content

    /// xmlNodeGetContent
    pub fn node_get_content(&self, id: NodeId) -> String {
        let n = &self.nodes[id];
        match n.t {
            NType::Text | NType::CData | NType::Comment | NType::Pi => n.content.clone(),
            NType::Attribute => {
                let mut s = String::new();
                let mut c = n.children;
                while let Some(x) = c {
                    match self.nodes[x].t {
                        NType::Text | NType::CData => s.push_str(&self.nodes[x].content),
                        NType::EntityRef => s.push_str(&self.node_get_content(x)),
                        _ => {}
                    }
                    c = self.nodes[x].next;
                }
                s
            }
            NType::EntityRef => {
                let mut s = String::new();
                if let Some(ent) = n.children {
                    self.collect_text(ent, &mut s);
                }
                if !s.is_empty() {
                    s
                } else {
                    // xmlGetDocEntity() finds the predefined entities too.
                    match n.name.as_str() {
                        "lt" => "<",
                        "gt" => ">",
                        "amp" => "&",
                        "apos" => "'",
                        "quot" => "\"",
                        _ => "",
                    }
                    .to_string()
                }
            }
            NType::Element
            | NType::DocumentFrag
            | NType::Document
            | NType::HtmlDocument
            | NType::Entity
            | NType::EntityDecl => {
                let mut s = String::new();
                self.collect_text(id, &mut s);
                s
            }
            NType::NamespaceDecl => String::new(),
            _ => String::new(),
        }
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        let mut c = self.nodes[id].children;
        while let Some(x) = c {
            match self.nodes[x].t {
                NType::Text | NType::CData => out.push_str(&self.nodes[x].content),
                NType::Element | NType::DocumentFrag => self.collect_text(x, out),
                NType::EntityRef => {
                    if let Some(ent) = self.nodes[x].children {
                        self.collect_text(ent, out);
                    }
                }
                _ => {}
            }
            c = self.nodes[x].next;
        }
    }

    /// xmlNodeListGetString(doc, list, inLine=1): concatenated text of a
    /// sibling list (text and entity refs).
    pub fn list_get_string(&self, first: Option<NodeId>, in_line: bool) -> String {
        let mut s = String::new();
        let mut c = first;
        while let Some(x) = c {
            match self.nodes[x].t {
                NType::Text | NType::CData => s.push_str(&self.nodes[x].content),
                NType::EntityRef => {
                    if in_line {
                        s.push_str(&self.node_get_content(x));
                    } else {
                        s.push('&');
                        s.push_str(&self.nodes[x].name);
                        s.push(';');
                    }
                }
                _ => {}
            }
            c = self.nodes[x].next;
        }
        s
    }

    /// xmlNodeSetContent
    pub fn node_set_content(&mut self, id: NodeId, value: &str) {
        match self.nodes[id].t {
            NType::Element
            | NType::Attribute
            | NType::DocumentFrag
            | NType::Document
            | NType::HtmlDocument => {
                self.set_children_text(id, value);
            }
            NType::Text | NType::CData | NType::Comment | NType::Pi => {
                self.nodes[id].content = value.to_string();
                self.nodes[id].flags &= !F_NULL_CONTENT;
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------
    // Entities

    /// The predefined entities (lt, gt, amp, apos, quot) as shared,
    /// document-less entity declarations (xmlGetPredefinedEntity).
    pub fn predefined_entity(&mut self, name: &str) -> Option<NodeId> {
        let content = match name {
            "lt" => "<",
            "gt" => ">",
            "amp" => "&",
            "apos" => "'",
            "quot" => "\"",
            _ => return None,
        };
        if let Some(id) = self.predef.iter().find(|(n, _)| n == name).map(|x| x.1) {
            return Some(id);
        }
        let id = self.new_node(None, NType::EntityDecl, name);
        self.nodes[id].content = content.to_string();
        self.nodes[id].extra = Extra::Entity(Box::new(EntityInfo {
            etype: EntityType::InternalPredefined,
            external_id: None,
            system_id: None,
            notation: None,
            orig: Some(content.to_string()),
            checked: false,
        }));
        self.predef.push((name.to_string(), id));
        Some(id)
    }

    /// xmlGetDocEntity: document entities, then the predefined ones.
    pub fn get_doc_entity_any(&mut self, doc: Option<NodeId>, name: &str) -> Option<NodeId> {
        if let Some(d) = doc {
            if let Some(e) = self.get_doc_entity(d, name) {
                return Some(e);
            }
        }
        self.predefined_entity(name)
    }

    pub fn get_doc_entity(&self, doc: NodeId, name: &str) -> Option<NodeId> {
        let info = self.doc_info(doc)?;
        for sub in [info.int_subset, info.ext_subset].into_iter().flatten() {
            if let Some(d) = self.dtd_info(sub) {
                for (n, id) in &d.entities {
                    if n == name {
                        return Some(*id);
                    }
                }
            }
        }
        None
    }

    pub fn get_param_entity(&self, doc: NodeId, name: &str) -> Option<NodeId> {
        let info = self.doc_info(doc)?;
        for sub in [info.int_subset, info.ext_subset].into_iter().flatten() {
            if let Some(d) = self.dtd_info(sub) {
                for (n, id) in &d.pentities {
                    if n == name {
                        return Some(*id);
                    }
                }
            }
        }
        None
    }

    /// Create the internal subset node of a document.
    pub fn create_int_subset(
        &mut self,
        doc: Option<NodeId>,
        name: &str,
        external_id: Option<&str>,
        system_id: Option<&str>,
    ) -> NodeId {
        let id = self.new_node(doc, NType::Dtd, name);
        self.nodes[id].extra = Extra::Dtd(Box::new(DtdInfo {
            external_id: external_id.map(|s| s.to_string()),
            system_id: system_id.map(|s| s.to_string()),
            ..Default::default()
        }));
        if let Some(d) = doc {
            if let Some(di) = self.doc_info_mut(d) {
                di.int_subset = Some(id);
            }
            // DTD goes before the root element
            let mut c = self.nodes[d].children;
            let mut inserted = false;
            while let Some(x) = c {
                if self.nodes[x].t == NType::Element {
                    self.insert_before_raw(x, id);
                    inserted = true;
                    break;
                }
                c = self.nodes[x].next;
            }
            if !inserted {
                self.link_child_raw(d, id);
            }
        }
        id
    }

    pub fn add_entity(
        &mut self,
        dtd: NodeId,
        name: &str,
        etype: EntityType,
        external_id: Option<&str>,
        system_id: Option<&str>,
        content: Option<&str>,
        notation: Option<&str>,
    ) -> Option<NodeId> {
        let param = matches!(
            etype,
            EntityType::InternalParameter | EntityType::ExternalParameter
        );
        if let Some(d) = self.dtd_info(dtd) {
            let list = if param { &d.pentities } else { &d.entities };
            if list.iter().any(|(n, _)| n == name) {
                return None;
            }
        }
        let doc = self.nodes[dtd].doc;
        let id = self.new_node(doc, NType::EntityDecl, name);
        self.nodes[id].content = content.unwrap_or("").to_string();
        self.nodes[id].extra = Extra::Entity(Box::new(EntityInfo {
            etype,
            external_id: external_id.map(|s| s.to_string()),
            system_id: system_id.map(|s| s.to_string()),
            notation: notation.map(|s| s.to_string()),
            orig: content.map(|s| s.to_string()),
            checked: false,
        }));
        self.link_child_raw(dtd, id);
        if let Some(d) = self.dtd_info_mut(dtd) {
            if param {
                d.pentities.push((name.to_string(), id));
            } else {
                d.entities.push((name.to_string(), id));
            }
        }
        Some(id)
    }

    // ------------------------------------------------------------------
    // Copy

    /// xmlDocCopyNode(node, doc, extended): 1 = recursive, 2 = shallow with
    /// attributes and namespaces.
    pub fn copy_node(
        &mut self,
        id: NodeId,
        doc: Option<NodeId>,
        extended: u8,
        parent: Option<NodeId>,
    ) -> NodeId {
        let src = self.nodes[id].clone();
        match src.t {
            NType::Document | NType::HtmlDocument => {
                return self.copy_doc(id, extended == 1);
            }
            NType::Dtd => {
                let n = self.new_node(doc, NType::Dtd, &src.name);
                self.nodes[n].extra = Extra::Dtd(Box::new(DtdInfo {
                    external_id: self.dtd_info(id).and_then(|d| d.external_id.clone()),
                    system_id: self.dtd_info(id).and_then(|d| d.system_id.clone()),
                    ..Default::default()
                }));
                if extended == 1 {
                    for c in self.children_of(id) {
                        if matches!(self.nodes[c].t, NType::ElementDecl | NType::AttributeDecl) {
                            // xmlCopyDtd copies element and attribute declarations too
                            let src = self.nodes[c].clone();
                            let d = self.new_node(doc, src.t, &src.name);
                            self.nodes[d].extra = src.extra.clone();
                            self.nodes[d].ns = src.ns;
                            self.link_child_raw(n, d);
                            continue;
                        }
                        if let Extra::Entity(info) = self.nodes[c].extra.clone() {
                            let nm = self.nodes[c].name.clone();
                            let e = self.add_entity(
                                n,
                                &nm,
                                info.etype,
                                info.external_id.as_deref(),
                                info.system_id.as_deref(),
                                info.orig.as_deref(),
                                info.notation.as_deref(),
                            );
                            if let Some(e) = e {
                                for cc in self.children_of(c) {
                                    let cp = self.copy_node(cc, doc, 1, Some(e));
                                    self.link_child_raw(e, cp);
                                }
                            }
                        }
                    }
                    let (notations, mut defs, mut elems) = self
                        .dtd_info(id)
                        .map(|d| {
                            (
                                d.notations.clone(),
                                d.attr_defaults.clone(),
                                d.elements.clone(),
                            )
                        })
                        .unwrap_or_default();
                    for a in defs.iter_mut() {
                        a.node = None;
                    }
                    for e in elems.iter_mut() {
                        e.node = None;
                        e.attrs.retain(|(dd, _)| *dd == id);
                        for a in e.attrs.iter_mut() {
                            a.0 = n;
                        }
                    }
                    if let Some(d) = self.dtd_info_mut(n) {
                        d.notations = notations;
                        d.attr_defaults = defs;
                        d.elements = elems;
                        d.attr_cache = Default::default();
                    }
                }
                return n;
            }
            _ => {}
        }
        let mut n = Node::new(src.t, &src.name);
        n.content = src.content.clone();
        n.doc = doc;
        n.line = src.line;
        n.atype = 0;
        n.extra = match &src.extra {
            Extra::Doc(_) | Extra::Dtd(_) => Extra::None,
            e => e.clone(),
        };
        let nid = self.alloc(n);
        if src.t == NType::EntityRef {
            let ent = doc.and_then(|d| self.get_doc_entity(d, &src.name));
            let ent = ent.or(src.children);
            self.nodes[nid].children = ent;
            self.nodes[nid].last = ent;
            return nid;
        }
        // temporarily attach to parent for namespace lookup
        self.nodes[nid].parent = parent;
        if src.t == NType::Element {
            for &ns in &src.ns_def {
                let (href, prefix) = (self.nss[ns].href.clone(), self.nss[ns].prefix.clone());
                let nn = self.new_ns_raw(&href, prefix.as_deref(), Some(nid));
                self.nodes[nid].ns_def.push(nn);
            }
        }
        // xmlCopyProp without a target element drops the namespace
        let keep_ns = !(src.t == NType::Attribute && parent.is_none());
        if let Some(ns) = src.ns.filter(|_| keep_ns) {
            let prefix = self.nss[ns].prefix.clone();
            let href = self.nss[ns].href.clone();
            let found = self.search_ns(nid, prefix.as_deref());
            let found = match found {
                Some(f) if self.nss[f].href == href => Some(f),
                _ => None,
            };
            let ns_new = match found {
                Some(f) => f,
                None => {
                    // declare on the root of the copy
                    let mut root = nid;
                    while let Some(p) = self.nodes[root].parent {
                        if p == root || self.nodes[p].t != NType::Element {
                            break;
                        }
                        root = p;
                    }
                    if src.t == NType::Attribute && self.nodes[root].t != NType::Element {
                        self.new_ns_raw(&href, prefix.as_deref(), None)
                    } else if self.nodes[root].t == NType::Element {
                        match self.new_ns(Some(root), &href, prefix.as_deref()) {
                            Some(x) => x,
                            None => self.new_ns_raw(&href, prefix.as_deref(), None),
                        }
                    } else {
                        self.new_ns_raw(&href, prefix.as_deref(), None)
                    }
                }
            };
            self.nodes[nid].ns = Some(ns_new);
        }
        if src.t == NType::Element && extended != 0 {
            let mut a = src.properties;
            while let Some(x) = a {
                let ca = self.copy_node(x, doc, 1, Some(nid));
                self.link_child_raw(nid, ca);
                if self.nodes[x].atype == 2 {
                    self.nodes[ca].atype = 2;
                    if let Some(d) = doc {
                        let v = self.node_get_content(ca);
                        if let Some(di) = self.doc_info_mut(d) {
                            di.ids.entry(v).or_insert(ca);
                        }
                    }
                }
                a = self.nodes[x].next;
            }
        }
        if (src.t == NType::Element && extended == 1)
            || src.t == NType::Attribute
            || src.t == NType::DocumentFrag
        {
            let mut c = src.children;
            while let Some(x) = c {
                let cc = self.copy_node(x, doc, 1, Some(nid));
                self.link_child_raw(nid, cc);
                c = self.nodes[x].next;
            }
        }
        self.nodes[nid].parent = None;
        nid
    }

    pub fn copy_doc(&mut self, id: NodeId, recursive: bool) -> NodeId {
        let info = self.doc_info(id).cloned().unwrap_or_default();
        let t = self.nodes[id].t;
        let nd = if t == NType::HtmlDocument {
            self.new_html_doc()
        } else {
            self.new_doc(info.version.as_deref())
        };
        if let Some(di) = self.doc_info_mut(nd) {
            di.encoding = info.encoding.clone();
            di.standalone = info.standalone;
            di.url = info.url.clone();
            di.html = info.html;
            di.parse_options = info.parse_options;
        }
        if recursive {
            for c in self.children_of(id) {
                let cp = self.copy_node(c, Some(nd), 1, Some(nd));
                self.link_child_raw(nd, cp);
                if self.nodes[c].t == NType::Dtd && info.int_subset == Some(c) {
                    if let Some(di) = self.doc_info_mut(nd) {
                        di.int_subset = Some(cp);
                    }
                    // re-resolve entity refs created before the DTD existed
                }
            }
            // fix entity refs now that the DTD exists
            let all = self.descendants(nd);
            for d in all {
                if self.nodes[d].t == NType::EntityRef {
                    let name = self.nodes[d].name.clone();
                    let ent = self.get_doc_entity(nd, &name);
                    self.nodes[d].children = ent;
                    self.nodes[d].last = ent;
                }
            }
        }
        nd
    }

    /// All descendants in document order (excluding attributes).
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = Vec::new();
        if self.nodes[id].t == NType::EntityRef {
            return out;
        }
        let kids = self.children_of(id);
        for k in kids.into_iter().rev() {
            stack.push(k);
        }
        while let Some(n) = stack.pop() {
            out.push(n);
            if self.nodes[n].t == NType::EntityRef {
                continue;
            }
            let kids = self.children_of(n);
            for k in kids.into_iter().rev() {
                stack.push(k);
            }
        }
        out
    }

    /// Whether `anc` is an ancestor of (or equal to) `node`.
    pub fn is_ancestor_or_self(&self, anc: NodeId, node: NodeId) -> bool {
        let mut cur = Some(node);
        while let Some(c) = cur {
            if c == anc {
                return true;
            }
            cur = self.nodes[c].parent;
        }
        false
    }

    /// Get the base URL of a document.
    pub fn doc_url(&self, doc: NodeId) -> Option<String> {
        self.doc_info(doc).and_then(|d| d.url.clone())
    }

    /// xmlGetNodePath
    pub fn node_path(&self, id: NodeId) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            let n = &self.nodes[c];
            match n.t {
                NType::Document | NType::HtmlDocument => {
                    break;
                }
                NType::Element => {
                    let name = self.qname(c);
                    let name = if n.ns.is_some() && self.ns_prefix(n.ns).is_none() {
                        "*".to_string()
                    } else {
                        name
                    };
                    // count same-name siblings
                    let same = |o: NodeId| -> bool {
                        let on = &self.nodes[o];
                        on.t == NType::Element
                            && on.name == n.name
                            && self.ns_href(on.ns) == self.ns_href(n.ns)
                            && self.ns_prefix(on.ns) == self.ns_prefix(n.ns)
                    };
                    let mut idx = 1;
                    let mut p = n.prev;
                    while let Some(x) = p {
                        if same(x) {
                            idx += 1;
                        }
                        p = self.nodes[x].prev;
                    }
                    let mut has_more = idx > 1;
                    if !has_more {
                        let mut nx = n.next;
                        while let Some(x) = nx {
                            if same(x) {
                                has_more = true;
                                break;
                            }
                            nx = self.nodes[x].next;
                        }
                    }
                    if n.ns.is_some() && self.ns_prefix(n.ns).is_none() {
                        // default namespace: *[local-name()...] form? libxml uses "*"
                        let mut idx2 = 1;
                        let mut p = n.prev;
                        while let Some(x) = p {
                            if self.nodes[x].t == NType::Element {
                                idx2 += 1;
                            }
                            p = self.nodes[x].prev;
                        }
                        let mut more = idx2 > 1;
                        if !more {
                            let mut nx = n.next;
                            while let Some(x) = nx {
                                if self.nodes[x].t == NType::Element {
                                    more = true;
                                    break;
                                }
                                nx = self.nodes[x].next;
                            }
                        }
                        if more {
                            parts.push(format!("*[{}]", idx2));
                        } else {
                            parts.push("*".to_string());
                        }
                    } else if has_more {
                        parts.push(format!("{}[{}]", name, idx));
                    } else {
                        parts.push(name);
                    }
                }
                NType::Attribute => {
                    parts.push(format!("@{}", self.qname(c)));
                }
                NType::Text | NType::CData | NType::Comment | NType::Pi => {
                    let kind = |t: NType| -> bool {
                        match n.t {
                            NType::Text | NType::CData => matches!(t, NType::Text | NType::CData),
                            NType::Pi => t == NType::Pi,
                            other => t == other,
                        }
                    };
                    let mut idx = 1;
                    let mut p = n.prev;
                    while let Some(x) = p {
                        if kind(self.nodes[x].t)
                            && (n.t != NType::Pi || self.nodes[x].name == n.name)
                        {
                            idx += 1;
                        }
                        p = self.nodes[x].prev;
                    }
                    let mut more = idx > 1;
                    if !more {
                        let mut nx = n.next;
                        while let Some(x) = nx {
                            if kind(self.nodes[x].t)
                                && (n.t != NType::Pi || self.nodes[x].name == n.name)
                            {
                                more = true;
                                break;
                            }
                            nx = self.nodes[x].next;
                        }
                    }
                    let base = match n.t {
                        NType::Comment => "comment()".to_string(),
                        NType::Pi => format!("processing-instruction('{}')", n.name),
                        _ => "text()".to_string(),
                    };
                    if more {
                        parts.push(format!("{}[{}]", base, idx));
                    } else {
                        parts.push(base);
                    }
                }
                _ => {}
            }
            cur = n.parent;
        }
        parts.reverse();
        format!("/{}", parts.join("/"))
    }
}
