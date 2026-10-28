//! The node tree: an arena of nodes shared by any number of documents.
//!
//! One [`Arena`] holds documents and the nodes that belong to them, so a node
//! moves between documents (DOM `adoptNode`) without changing its
//! [`NodeId`]. The model follows the DOM Standard and DOM Level 3 Core
//! (node types, attribute nodes, namespace declarations), with the extra
//! node kinds PHP's DOM exposes from its libxml heritage (entity references,
//! DTD declarations, namespace nodes for XPath).
//!
//! Text is stored as UTF-8 (`String`); names are shared [`Str`] values.
//!
//! Memory: the embedder takes *holds* on the nodes it refers to
//! ([`Arena::hold`], [`Arena::release`]). A document lives while any node it
//! owns is held; a detached subtree lives while any node in it is held.
//! Releasing the last hold frees what became unreachable. Operations that
//! free nodes (setting text content, `normalize`) keep held nodes alive as
//! detached roots, as PHP keeps nodes a PHP object refers to.
//! [`Arena::free_subtree`] frees an unheld detached subtree explicitly.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::rc::Rc;

/// A shared immutable string (names, namespace URIs).
pub type Str = Rc<str>;

/// The XML namespace (Namespaces in XML 1.0, section 3).
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
/// The XMLNS namespace (Namespaces in XML 1.0, section 3).
pub const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
/// The HTML namespace (WHATWG Infra).
pub const HTML_NS: &str = "http://www.w3.org/1999/xhtml";
/// The SVG namespace (WHATWG Infra).
pub const SVG_NS: &str = "http://www.w3.org/2000/svg";
/// The MathML namespace (WHATWG Infra).
pub const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";
/// The XLink namespace (WHATWG Infra).
pub const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

/// A node handle. Valid for the [`Arena`] that created it until the node is
/// freed.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NodeId(NonZeroU32);

impl NodeId {
    /// The slot index (stable while the node lives), usable as a map key.
    pub fn index(self) -> u32 {
        self.0.get()
    }
    /// Rebuilds a handle from [`NodeId::index`].
    pub fn from_index(i: u32) -> Option<NodeId> {
        NonZeroU32::new(i).map(NodeId)
    }
}

/// The DOM `nodeType` values (DOM Level 3 Core), plus the libxml-specific
/// kinds PHP exposes with their libxml numbers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum NodeType {
    /// `XML_ELEMENT_NODE`
    Element = 1,
    /// `XML_ATTRIBUTE_NODE`
    Attribute = 2,
    /// `XML_TEXT_NODE`
    Text = 3,
    /// `XML_CDATA_SECTION_NODE`
    CData = 4,
    /// `XML_ENTITY_REF_NODE`
    EntityRef = 5,
    /// `XML_ENTITY_NODE` (entity declarations)
    Entity = 6,
    /// `XML_PI_NODE`
    Pi = 7,
    /// `XML_COMMENT_NODE`
    Comment = 8,
    /// `XML_DOCUMENT_NODE`
    Document = 9,
    /// `XML_DOCUMENT_TYPE_NODE` (PHP reports doctypes with this value)
    DocumentType = 10,
    /// `XML_DOCUMENT_FRAG_NODE`
    Fragment = 11,
    /// `XML_NOTATION_NODE`
    Notation = 12,
    /// `XML_HTML_DOCUMENT_NODE`
    HtmlDocument = 13,
    /// `XML_ELEMENT_DECL`
    ElementDecl = 15,
    /// `XML_ATTRIBUTE_DECL`
    AttributeDecl = 16,
    /// `XML_NAMESPACE_DECL` (XPath namespace nodes, `DOMNameSpaceNode`)
    Namespace = 18,
}

/// What kind of document a document node is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DocKind {
    /// An XML document (`DOMDocument`, `Dom\XMLDocument`).
    #[default]
    Xml,
    /// A document from the legacy HTML parser (`DOMDocument::loadHTML`).
    Html,
    /// An HTML5 document (`Dom\HTMLDocument`).
    Html5,
}

/// HTML quirks mode (WHATWG DOM "document mode").
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum QuirksMode {
    /// "no-quirks"
    #[default]
    NoQuirks,
    /// "limited-quirks"
    LimitedQuirks,
    /// "quirks"
    Quirks,
}

/// Document-level data.
#[derive(Clone, Debug, Default)]
pub struct DocData {
    /// XML, legacy HTML or HTML5.
    pub kind: DocKind,
    /// The XML declaration's version (`None` when there was none; serializers
    /// print "1.0" then).
    pub version: Option<String>,
    /// The declared or set encoding (`DOMDocument::$encoding`).
    pub encoding: Option<String>,
    /// `standalone`: -1 absent, 0 no, 1 yes.
    pub standalone: i8,
    /// The document URI (`documentURI`).
    pub url: Option<String>,
    /// The internal subset / doctype node, if any.
    pub doctype: Option<NodeId>,
    /// The external subset (when loaded), holding its declarations.
    pub ext_subset: Option<NodeId>,
    /// `xml:id` and DTD ID attribute values to their element.
    pub ids: HashMap<String, NodeId>,
    /// HTML quirks mode.
    pub quirks: QuirksMode,
    /// True for the spec-compliant `Dom\` documents (PHP 8.4 "Opt-in DOM
    /// spec compliance" RFC), which change namespace and serialization rules.
    pub modern: bool,
    /// Content type for HTML5 documents.
    pub content_type: Option<String>,
    /// External holds on nodes owned by this document (see
    /// [`Arena::hold`]); not copied by clones.
    pub holds: u32,
}

/// An XML namespace declaration (`xmlns:prefix="uri"` or `xmlns="uri"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NsDecl {
    /// The declared prefix (`None` for the default namespace).
    pub prefix: Option<Str>,
    /// The namespace name ("" undeclares the default namespace).
    pub uri: Str,
}

/// A qualified name with its namespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QName {
    /// The prefix, if any.
    pub prefix: Option<Str>,
    /// The local name.
    pub local: Str,
    /// The namespace name, if any.
    pub ns: Option<Str>,
}

impl QName {
    /// A name without namespace or prefix.
    pub fn local(name: &str) -> QName {
        QName {
            prefix: None,
            local: name.into(),
            ns: None,
        }
    }
    /// The qualified name `prefix:local` or `local`.
    pub fn qualified(&self) -> String {
        match &self.prefix {
            Some(p) => format!("{p}:{}", self.local),
            None => self.local.to_string(),
        }
    }
    /// True when the qualified name equals `name`.
    pub fn is(&self, name: &str) -> bool {
        match &self.prefix {
            None => &*self.local == name,
            Some(p) => {
                name.len() == p.len() + 1 + self.local.len()
                    && name.starts_with(&**p)
                    && name.as_bytes()[p.len()] == b':'
                    && name.ends_with(&*self.local)
            }
        }
    }
    /// The namespace URI as `&str` ("" when none).
    pub fn ns_str(&self) -> &str {
        self.ns.as_deref().unwrap_or("")
    }
}

/// Element data.
#[derive(Clone, Debug)]
pub struct ElementData {
    /// The element's name and namespace.
    pub name: QName,
    /// First attribute node (attributes are chained through `next`/`prev`
    /// and have the element as parent).
    pub first_attr: Option<NodeId>,
    /// Last attribute node.
    pub last_attr: Option<NodeId>,
    /// Namespace declarations made on this element (legacy DOM; the
    /// `Dom\` classes keep `xmlns` attributes as attributes instead).
    pub ns_decls: Vec<NsDecl>,
    /// For HTML5 `<template>`: the template contents fragment.
    pub template_contents: Option<NodeId>,
}

/// DTD attribute types (XML 1.0 section 3.3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AttrType {
    /// CDATA
    #[default]
    Cdata,
    /// ID
    Id,
    /// IDREF
    IdRef,
    /// IDREFS
    IdRefs,
    /// ENTITY
    Entity,
    /// ENTITIES
    Entities,
    /// NMTOKEN
    NmToken,
    /// NMTOKENS
    NmTokens,
    /// An enumeration
    Enumeration,
    /// NOTATION
    Notation,
}

impl AttrType {
    /// The keyword in an ATTLIST declaration.
    pub fn keyword(self) -> &'static str {
        match self {
            AttrType::Cdata => "CDATA",
            AttrType::Id => "ID",
            AttrType::IdRef => "IDREF",
            AttrType::IdRefs => "IDREFS",
            AttrType::Entity => "ENTITY",
            AttrType::Entities => "ENTITIES",
            AttrType::NmToken => "NMTOKEN",
            AttrType::NmTokens => "NMTOKENS",
            AttrType::Enumeration => "",
            AttrType::Notation => "NOTATION",
        }
    }
}

/// Attribute data. The value is held by the attribute's children (text and
/// entity reference nodes), as in DOM Level 3 Core.
#[derive(Clone, Debug)]
pub struct AttrData {
    /// The attribute's name and namespace.
    pub name: QName,
    /// The attribute is an ID (DTD type ID, `xml:id`, or set with
    /// `setIdAttribute`).
    pub is_id: bool,
    /// The attribute came from a DTD default (LIBXML_DTDATTR) rather than
    /// the document.
    pub defaulted: bool,
}

/// Entity declaration types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    /// Internal general entity.
    InternalGeneral,
    /// External parsed general entity.
    ExternalGeneralParsed,
    /// External unparsed general entity (with NDATA).
    ExternalGeneralUnparsed,
    /// Internal parameter entity.
    InternalParameter,
    /// External parameter entity.
    ExternalParameter,
    /// One of the five predefined entities.
    Predefined,
}

/// An entity declaration.
#[derive(Clone, Debug)]
pub struct EntityDecl {
    /// The entity name.
    pub name: Str,
    /// Its kind.
    pub kind: EntityKind,
    /// The replacement text (internal entities) as declared, after
    /// character and parameter-entity references are expanded.
    pub value: Option<String>,
    /// The original literal as written (for serialization).
    pub orig: Option<String>,
    /// PUBLIC identifier.
    pub public_id: Option<String>,
    /// SYSTEM identifier.
    pub system_id: Option<String>,
    /// NDATA notation name.
    pub notation: Option<String>,
    /// Whether the expansion has been checked (well-formedness of content).
    pub checked: bool,
}

/// An attribute default declaration (`#REQUIRED`, `#IMPLIED`, `#FIXED`, or
/// a plain default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrDefault {
    /// No default keyword: the value is the default.
    None,
    /// `#REQUIRED`
    Required,
    /// `#IMPLIED`
    Implied,
    /// `#FIXED`
    Fixed,
}

/// An `<!ATTLIST>` attribute definition.
#[derive(Clone, Debug)]
pub struct AttrDecl {
    /// The element the attribute belongs to.
    pub element: Str,
    /// The attribute's qualified name.
    pub name: Str,
    /// Its type.
    pub atype: AttrType,
    /// Enumerated values (for enumerations and NOTATION).
    pub values: Vec<String>,
    /// Default kind.
    pub default: AttrDefault,
    /// The default value, normalized.
    pub value: Option<String>,
}

/// Element content model (XML 1.0 section 3.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentModel {
    /// `EMPTY`
    Empty,
    /// `ANY`
    Any,
    /// `(#PCDATA)` or `(#PCDATA|a|b)*`.
    Mixed(Vec<Str>),
    /// A children content particle.
    Children(Particle),
}

/// Occurrence indicator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Occur {
    /// Exactly once.
    One,
    /// `?`
    Opt,
    /// `*`
    Star,
    /// `+`
    Plus,
}

impl Occur {
    /// The suffix character.
    pub fn suffix(self) -> &'static str {
        match self {
            Occur::One => "",
            Occur::Opt => "?",
            Occur::Star => "*",
            Occur::Plus => "+",
        }
    }
}

/// A content particle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Particle {
    /// An element name.
    Name(Str, Occur),
    /// `(a , b)`
    Seq(Vec<Particle>, Occur),
    /// `(a | b)`
    Choice(Vec<Particle>, Occur),
}

/// An `<!ELEMENT>` declaration.
#[derive(Clone, Debug)]
pub struct ElementDecl {
    /// Element name.
    pub name: Str,
    /// Content model.
    pub content: ContentModel,
}

/// A `<!NOTATION>` declaration.
#[derive(Clone, Debug)]
pub struct NotationDecl {
    /// Notation name.
    pub name: Str,
    /// PUBLIC identifier.
    pub public_id: Option<String>,
    /// SYSTEM identifier.
    pub system_id: Option<String>,
}

/// One declaration of a DTD, in source order (for serialization).
#[derive(Clone, Debug)]
pub enum DtdItem {
    /// An entity declaration (index into `entities` or `pentities` by name).
    Entity(Str, bool),
    /// An element declaration.
    Element(Str),
    /// An attribute-list declaration (element, attribute).
    Attr(Str, Str),
    /// A notation.
    Notation(Str),
    /// A comment.
    Comment(String),
    /// A processing instruction.
    Pi(String, String),
}

/// A document type declaration and its subset (internal or external).
#[derive(Clone, Debug, Default)]
pub struct DtdData {
    /// The root element name.
    pub name: Str,
    /// PUBLIC identifier.
    pub public_id: Option<String>,
    /// SYSTEM identifier.
    pub system_id: Option<String>,
    /// General entities.
    pub entities: HashMap<Str, EntityDecl>,
    /// Parameter entities.
    pub pentities: HashMap<Str, EntityDecl>,
    /// Element declarations.
    pub elements: HashMap<Str, ElementDecl>,
    /// Attribute declarations, by element name, in declaration order.
    pub attlists: HashMap<Str, Vec<AttrDecl>>,
    /// Notations.
    pub notations: HashMap<Str, NotationDecl>,
    /// Declarations in source order.
    pub items: Vec<DtdItem>,
    /// The internal subset text as written (`DOMDocumentType::$internalSubset`
    /// is regenerated from `items`; this keeps the original for HTML5).
    pub raw_internal: Option<String>,
}

/// The per-kind data of a node.
#[derive(Clone, Debug)]
pub enum NodeData {
    /// A document.
    Document(Box<DocData>),
    /// A document fragment.
    Fragment,
    /// An element.
    Element(ElementData),
    /// An attribute.
    Attribute(AttrData),
    /// Text.
    Text(String),
    /// A CDATA section.
    CData(String),
    /// A comment.
    Comment(String),
    /// A processing instruction.
    Pi {
        /// The target.
        target: Str,
        /// The data.
        data: String,
    },
    /// An entity reference (resolved by name in the document's DTD).
    EntityRef(Str),
    /// A document type node with its declarations.
    Doctype(Box<DtdData>),
    /// An entity declaration node (as seen through `DOMDocumentType::$entities`).
    EntityDecl(Box<EntityDecl>),
    /// A notation node.
    Notation(Box<NotationDecl>),
    /// An element declaration node.
    ElementDecl(Box<ElementDecl>),
    /// An attribute declaration node.
    AttributeDecl(Box<AttrDecl>),
    /// A namespace node (XPath namespace axis; `DOMNameSpaceNode`). Its
    /// parent is the element it belongs to, but it is not one of its
    /// children.
    Namespace(NsDecl),
}

/// A node.
#[derive(Clone, Debug)]
pub struct Node {
    /// The per-kind data.
    pub data: NodeData,
    /// The parent (for attributes: the owner element).
    pub parent: Option<NodeId>,
    /// First child.
    pub first_child: Option<NodeId>,
    /// Last child.
    pub last_child: Option<NodeId>,
    /// Previous sibling.
    pub prev: Option<NodeId>,
    /// Next sibling.
    pub next: Option<NodeId>,
    /// The owner document (`None` only for a document node itself).
    pub doc: Option<NodeId>,
    /// The source line (1-based; 0 when unknown).
    pub line: u32,
    /// External holds on this node (see [`Arena::hold`]).
    pub holds: u32,
}

impl Node {
    fn new(data: NodeData, doc: Option<NodeId>) -> Node {
        Node {
            data,
            parent: None,
            first_child: None,
            last_child: None,
            prev: None,
            next: None,
            doc,
            line: 0,
            holds: 0,
        }
    }
}

/// DOM exceptions (WHATWG DOM, `DOMException` names) and the legacy
/// warnings PHP's DOM reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomError {
    /// `HierarchyRequestError` (code 3)
    HierarchyRequest,
    /// `WrongDocumentError` (code 4)
    WrongDocument,
    /// `InvalidCharacterError` (code 5)
    InvalidCharacter,
    /// `NoModificationAllowedError` (code 7)
    NoModificationAllowed,
    /// `NotFoundError` (code 8)
    NotFound,
    /// `NotSupportedError` (code 9)
    NotSupported,
    /// `InUseAttributeError` (code 10)
    InUseAttribute,
    /// `InvalidStateError` (code 11)
    InvalidState,
    /// `SyntaxError` (code 12)
    Syntax,
    /// `InvalidModificationError` (code 13)
    InvalidModification,
    /// `NamespaceError` (code 14)
    Namespace,
    /// `IndexSizeError` (code 1)
    IndexSize,
}

impl DomError {
    /// The legacy `DOMException::$code`.
    pub fn code(self) -> i32 {
        match self {
            DomError::IndexSize => 1,
            DomError::HierarchyRequest => 3,
            DomError::WrongDocument => 4,
            DomError::InvalidCharacter => 5,
            DomError::NoModificationAllowed => 7,
            DomError::NotFound => 8,
            DomError::NotSupported => 9,
            DomError::InUseAttribute => 10,
            DomError::InvalidState => 11,
            DomError::Syntax => 12,
            DomError::InvalidModification => 13,
            DomError::Namespace => 14,
        }
    }
    /// The message PHP's `DOMException` carries (observed on PHP 8.5).
    pub fn message(self) -> &'static str {
        match self {
            DomError::IndexSize => "Index Size Error",
            DomError::HierarchyRequest => "Hierarchy Request Error",
            DomError::WrongDocument => "Wrong Document Error",
            DomError::InvalidCharacter => "Invalid Character Error",
            DomError::NoModificationAllowed => "No Modification Allowed Error",
            DomError::NotFound => "Not Found Error",
            DomError::NotSupported => "Not Supported Error",
            DomError::InUseAttribute => "Inuse Attribute Error",
            DomError::InvalidState => "Invalid State Error",
            DomError::Syntax => "Syntax Error",
            DomError::InvalidModification => "Invalid Modification Error",
            DomError::Namespace => "Namespace Error",
        }
    }
}

/// The arena.
#[derive(Default, Debug)]
pub struct Arena {
    nodes: Vec<Option<Node>>,
    free: Vec<u32>,
    /// Documents whose last hold went away through an adoption (freed by
    /// [`Arena::collect`]).
    unheld_docs: Vec<NodeId>,
    /// Bumped by every structural change (live collections cache on it).
    generation: u64,
}

/// Iterator over a sibling chain.
pub struct Siblings<'a> {
    arena: &'a Arena,
    cur: Option<NodeId>,
}

impl Iterator for Siblings<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let c = self.cur?;
        self.cur = self.arena.node(c).next;
        Some(c)
    }
}

/// Pre-order iterator over the descendants of a node (not including it,
/// not including attributes).
pub struct Descendants<'a> {
    arena: &'a Arena,
    root: NodeId,
    cur: Option<NodeId>,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let c = self.cur?;
        self.cur = self.arena.next_in_tree(c, self.root);
        Some(c)
    }
}

impl Arena {
    /// A new, empty arena.
    pub fn new() -> Arena {
        Arena {
            nodes: vec![None],
            free: Vec::new(),
            unheld_docs: Vec::new(),
            generation: 0,
        }
    }

    fn alloc(&mut self, n: Node) -> NodeId {
        if self.nodes.is_empty() {
            self.nodes.push(None);
        }
        if let Some(i) = self.free.pop() {
            self.nodes[i as usize] = Some(n);
            return NodeId(NonZeroU32::new(i).unwrap());
        }
        let i = self.nodes.len() as u32;
        self.nodes.push(Some(n));
        NodeId(NonZeroU32::new(i).unwrap())
    }

    /// Number of live nodes.
    pub fn live_nodes(&self) -> usize {
        self.nodes.len().saturating_sub(1 + self.free.len())
    }

    /// True if `id` refers to a live node.
    pub fn is_live(&self, id: NodeId) -> bool {
        self.nodes
            .get(id.0.get() as usize)
            .is_some_and(|n| n.is_some())
    }

    /// The node. Panics on a freed handle.
    #[inline]
    pub fn node(&self, id: NodeId) -> &Node {
        self.nodes[id.0.get() as usize]
            .as_ref()
            .expect("freed node")
    }

    /// The node, mutably.
    #[inline]
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.nodes[id.0.get() as usize]
            .as_mut()
            .expect("freed node")
    }

    /// The node's data.
    #[inline]
    pub fn data(&self, id: NodeId) -> &NodeData {
        &self.node(id).data
    }

    // ----- creation -------------------------------------------------------

    /// Creates a document node.
    pub fn new_document(&mut self, kind: DocKind) -> NodeId {
        self.alloc(Node::new(
            NodeData::Document(Box::new(DocData {
                kind,
                standalone: -1,
                ..Default::default()
            })),
            None,
        ))
    }

    /// Creates a detached node of any kind owned by `doc`.
    pub fn create(&mut self, doc: NodeId, data: NodeData) -> NodeId {
        self.alloc(Node::new(data, Some(doc)))
    }

    /// Creates an element.
    pub fn new_element(&mut self, doc: NodeId, name: QName) -> NodeId {
        self.create(
            doc,
            NodeData::Element(ElementData {
                name,
                first_attr: None,
                last_attr: None,
                ns_decls: Vec::new(),
                template_contents: None,
            }),
        )
    }

    /// Creates a text node.
    pub fn new_text(&mut self, doc: NodeId, text: impl Into<String>) -> NodeId {
        self.create(doc, NodeData::Text(text.into()))
    }

    /// Creates an attribute node with a value (one text child, or none when
    /// the value is empty).
    pub fn new_attribute(&mut self, doc: NodeId, name: QName, value: &str) -> NodeId {
        let a = self.create(
            doc,
            NodeData::Attribute(AttrData {
                name,
                is_id: false,
                defaulted: false,
            }),
        );
        if !value.is_empty() {
            let t = self.new_text(doc, value);
            self.raw_append(a, t);
        }
        a
    }

    // ----- accessors ------------------------------------------------------

    /// The node type.
    pub fn node_type(&self, id: NodeId) -> NodeType {
        match &self.node(id).data {
            NodeData::Document(d) => {
                if d.kind == DocKind::Html {
                    NodeType::HtmlDocument
                } else {
                    NodeType::Document
                }
            }
            NodeData::Fragment => NodeType::Fragment,
            NodeData::Element(_) => NodeType::Element,
            NodeData::Attribute(_) => NodeType::Attribute,
            NodeData::Text(_) => NodeType::Text,
            NodeData::CData(_) => NodeType::CData,
            NodeData::Comment(_) => NodeType::Comment,
            NodeData::Pi { .. } => NodeType::Pi,
            NodeData::EntityRef(_) => NodeType::EntityRef,
            NodeData::Doctype(_) => NodeType::DocumentType,
            NodeData::EntityDecl(_) => NodeType::Entity,
            NodeData::Notation(_) => NodeType::Notation,
            NodeData::ElementDecl(_) => NodeType::ElementDecl,
            NodeData::AttributeDecl(_) => NodeType::AttributeDecl,
            NodeData::Namespace(_) => NodeType::Namespace,
        }
    }

    /// Parent.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }
    /// First child.
    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).first_child
    }
    /// Last child.
    pub fn last_child(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).last_child
    }
    /// Next sibling.
    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).next
    }
    /// Previous sibling.
    pub fn prev_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).prev
    }
    /// Children iterator.
    pub fn children(&self, id: NodeId) -> Siblings<'_> {
        Siblings {
            arena: self,
            cur: self.node(id).first_child,
        }
    }
    /// Attributes iterator (empty for non-elements).
    pub fn attrs(&self, id: NodeId) -> Siblings<'_> {
        let cur = match &self.node(id).data {
            NodeData::Element(e) => e.first_attr,
            _ => None,
        };
        Siblings { arena: self, cur }
    }
    /// Descendants in document order (pre-order), excluding `id` itself.
    pub fn descendants(&self, id: NodeId) -> Descendants<'_> {
        Descendants {
            arena: self,
            root: id,
            cur: self.node(id).first_child,
        }
    }

    /// Next node in pre-order within `root`'s subtree.
    pub fn next_in_tree(&self, n: NodeId, root: NodeId) -> Option<NodeId> {
        let node = self.node(n);
        if let Some(c) = node.first_child
            && !matches!(node.data, NodeData::EntityRef(_))
        {
            return Some(c);
        }
        let mut cur = n;
        loop {
            if cur == root {
                return None;
            }
            let nd = self.node(cur);
            if let Some(s) = nd.next {
                return Some(s);
            }
            cur = nd.parent?;
        }
    }

    /// The owner document (for a document node: itself).
    pub fn doc_of(&self, id: NodeId) -> NodeId {
        self.node(id).doc.unwrap_or(id)
    }

    /// Document data of the document `doc`. Panics if `doc` is not a document.
    pub fn doc(&self, doc: NodeId) -> &DocData {
        match &self.node(doc).data {
            NodeData::Document(d) => d,
            _ => panic!("not a document"),
        }
    }
    /// Document data, mutably.
    pub fn doc_mut(&mut self, doc: NodeId) -> &mut DocData {
        match &mut self.node_mut(doc).data {
            NodeData::Document(d) => d,
            _ => panic!("not a document"),
        }
    }

    /// Element data, if `id` is an element.
    pub fn element(&self, id: NodeId) -> Option<&ElementData> {
        match &self.node(id).data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }
    /// Element data, mutably.
    pub fn element_mut(&mut self, id: NodeId) -> Option<&mut ElementData> {
        match &mut self.node_mut(id).data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }
    /// Attribute data, if `id` is an attribute.
    pub fn attr(&self, id: NodeId) -> Option<&AttrData> {
        match &self.node(id).data {
            NodeData::Attribute(a) => Some(a),
            _ => None,
        }
    }
    /// Attribute data, mutably.
    pub fn attr_mut(&mut self, id: NodeId) -> Option<&mut AttrData> {
        match &mut self.node_mut(id).data {
            NodeData::Attribute(a) => Some(a),
            _ => None,
        }
    }
    /// The doctype data of a doctype node.
    pub fn dtd(&self, id: NodeId) -> Option<&DtdData> {
        match &self.node(id).data {
            NodeData::Doctype(d) => Some(d),
            _ => None,
        }
    }
    /// The doctype data, mutably.
    pub fn dtd_mut(&mut self, id: NodeId) -> Option<&mut DtdData> {
        match &mut self.node_mut(id).data {
            NodeData::Doctype(d) => Some(d),
            _ => None,
        }
    }

    /// True for elements.
    pub fn is_element(&self, id: NodeId) -> bool {
        matches!(self.node(id).data, NodeData::Element(_))
    }
    /// True for text and CDATA nodes.
    pub fn is_text(&self, id: NodeId) -> bool {
        matches!(self.node(id).data, NodeData::Text(_) | NodeData::CData(_))
    }

    /// The name of an element or attribute.
    pub fn qname(&self, id: NodeId) -> Option<&QName> {
        match &self.node(id).data {
            NodeData::Element(e) => Some(&e.name),
            NodeData::Attribute(a) => Some(&a.name),
            _ => None,
        }
    }

    /// DOM `nodeName` as legacy DOM reports it (qualified name for elements
    /// and attributes, `#text`, `#comment`, ...).
    pub fn node_name(&self, id: NodeId) -> String {
        match &self.node(id).data {
            NodeData::Document(_) => "#document".into(),
            NodeData::Fragment => "#document-fragment".into(),
            NodeData::Element(e) => e.name.qualified(),
            NodeData::Attribute(a) => a.name.qualified(),
            NodeData::Text(_) => "#text".into(),
            NodeData::CData(_) => "#cdata-section".into(),
            NodeData::Comment(_) => "#comment".into(),
            NodeData::Pi { target, .. } => target.to_string(),
            NodeData::EntityRef(n) => n.to_string(),
            NodeData::Doctype(d) => d.name.to_string(),
            NodeData::EntityDecl(e) => e.name.to_string(),
            NodeData::Notation(n) => n.name.to_string(),
            NodeData::ElementDecl(e) => e.name.to_string(),
            NodeData::AttributeDecl(a) => a.name.to_string(),
            NodeData::Namespace(n) => match &n.prefix {
                Some(p) => format!("xmlns:{p}"),
                None => "xmlns".into(),
            },
        }
    }

    /// The character data of text, CDATA, comment and PI nodes.
    pub fn char_data(&self, id: NodeId) -> Option<&str> {
        match &self.node(id).data {
            NodeData::Text(s) | NodeData::CData(s) | NodeData::Comment(s) => Some(s),
            NodeData::Pi { data, .. } => Some(data),
            _ => None,
        }
    }
    /// The character data, mutably.
    pub fn char_data_mut(&mut self, id: NodeId) -> Option<&mut String> {
        match &mut self.node_mut(id).data {
            NodeData::Text(s) | NodeData::CData(s) | NodeData::Comment(s) => Some(s),
            NodeData::Pi { data, .. } => Some(data),
            _ => None,
        }
    }

    /// The value of an attribute node (its children's text, entity
    /// references expanded).
    pub fn attr_value(&self, attr: NodeId) -> String {
        let n = self.node(attr);
        if let Some(c) = n.first_child
            && n.last_child == Some(c)
            && let NodeData::Text(t) = &self.node(c).data
        {
            return t.clone();
        }
        let mut s = String::new();
        self.collect_text(attr, &mut s, true);
        s
    }

    /// Replaces an attribute's value.
    pub fn set_attr_value(&mut self, attr: NodeId, value: &str) {
        self.generation += 1;
        self.remove_children(attr);
        if !value.is_empty() {
            let doc = self.doc_of(attr);
            let t = self.new_text(doc, value);
            self.raw_append(attr, t);
        }
        self.refresh_id(attr);
    }

    fn refresh_id(&mut self, attr: NodeId) {
        let is_id = self.attr(attr).is_some_and(|a| a.is_id);
        if is_id && let Some(el) = self.parent(attr) {
            let v = self.attr_value(attr);
            let doc = self.doc_of(attr);
            if self.is_connected(el) {
                self.doc_mut(doc).ids.insert(v, el);
            }
        }
    }

    fn collect_text(&self, id: NodeId, out: &mut String, expand_entities: bool) {
        let mut c = self.node(id).first_child;
        while let Some(n) = c {
            let node = self.node(n);
            match &node.data {
                NodeData::Text(t) | NodeData::CData(t) => out.push_str(t),
                NodeData::Element(_) | NodeData::Fragment => {
                    self.collect_text(n, out, expand_entities)
                }
                NodeData::EntityRef(name) if expand_entities => {
                    if node.first_child.is_some() {
                        self.collect_text(n, out, true);
                    } else if let Some(v) = self.entity_value(self.doc_of(n), name) {
                        out.push_str(&v);
                    }
                }
                _ => {}
            }
            c = node.next;
        }
    }

    /// DOM `textContent` getter (legacy semantics: documents give their
    /// whole text, comments and PIs their data).
    pub fn text_content(&self, id: NodeId) -> String {
        match &self.node(id).data {
            NodeData::Text(s) | NodeData::CData(s) | NodeData::Comment(s) => s.clone(),
            NodeData::Pi { data, .. } => data.clone(),
            NodeData::Attribute(_) => self.attr_value(id),
            NodeData::Namespace(n) => n.uri.to_string(),
            NodeData::EntityDecl(e) => e.value.clone().unwrap_or_default(),
            NodeData::Doctype(_) | NodeData::Notation(_) => String::new(),
            _ => {
                let mut s = String::new();
                self.collect_text(id, &mut s, true);
                s
            }
        }
    }

    /// DOM `textContent` setter: replaces the children with one text node
    /// (or sets character data).
    pub fn set_text_content(&mut self, id: NodeId, text: &str) {
        match &mut self.node_mut(id).data {
            NodeData::Text(s) | NodeData::CData(s) | NodeData::Comment(s) => {
                *s = text.to_string();
                return;
            }
            NodeData::Pi { data, .. } => {
                *data = text.to_string();
                return;
            }
            NodeData::Attribute(_) => {
                self.set_attr_value(id, text);
                return;
            }
            NodeData::Element(_) | NodeData::Fragment => {}
            _ => return,
        }
        self.remove_children(id);
        if !text.is_empty() {
            let doc = self.doc_of(id);
            let t = self.new_text(doc, text);
            self.raw_append(id, t);
        }
    }

    /// The replacement text of a general entity declared in `doc`'s DTDs,
    /// or a predefined entity.
    pub fn entity_value(&self, doc: NodeId, name: &str) -> Option<String> {
        if let Some(e) = self.find_entity(doc, name) {
            return e.value.clone();
        }
        predefined_entity(name).map(|c| c.to_string())
    }

    /// Looks a general entity up in the internal then external subset.
    pub fn find_entity(&self, doc: NodeId, name: &str) -> Option<&EntityDecl> {
        let d = match &self.node(doc).data {
            NodeData::Document(d) => d,
            _ => return None,
        };
        for sub in [d.doctype, d.ext_subset].into_iter().flatten() {
            if let Some(dtd) = self.dtd(sub)
                && let Some(e) = dtd.entities.get(name)
            {
                return Some(e);
            }
        }
        None
    }

    // ----- raw tree surgery (no checks) ------------------------------------

    /// Detaches `n` from its parent (or attribute list). No checks.
    pub fn detach(&mut self, n: NodeId) {
        self.generation += 1;
        let (parent, prev, next, is_attr) = {
            let node = self.node(n);
            (
                node.parent,
                node.prev,
                node.next,
                matches!(node.data, NodeData::Attribute(_)),
            )
        };
        let Some(p) = parent else {
            return;
        };
        if is_attr {
            // Detaching an ID attribute removes the ID.
            if self.attr(n).is_some_and(|a| a.is_id) {
                let v = self.attr_value(n);
                let doc = self.doc_of(n);
                if self.doc(doc).ids.get(&v) == Some(&p) {
                    self.doc_mut(doc).ids.remove(&v);
                }
            }
            if let Some(pv) = prev {
                self.node_mut(pv).next = next;
            } else if let Some(e) = self.element_mut(p) {
                e.first_attr = next;
            }
            if let Some(nx) = next {
                self.node_mut(nx).prev = prev;
            } else if let Some(e) = self.element_mut(p) {
                e.last_attr = prev;
            }
        } else {
            if let Some(pv) = prev {
                self.node_mut(pv).next = next;
            } else {
                self.node_mut(p).first_child = next;
            }
            if let Some(nx) = next {
                self.node_mut(nx).prev = prev;
            } else {
                self.node_mut(p).last_child = prev;
            }
            if let NodeData::Document(_) = self.node(p).data
                && matches!(self.node(n).data, NodeData::Doctype(_))
                && self.doc(p).doctype == Some(n)
            {
                self.doc_mut(p).doctype = None;
            }
        }
        let node = self.node_mut(n);
        node.parent = None;
        node.prev = None;
        node.next = None;
    }

    /// Appends `child` (detached) as the last child of `parent`. No checks.
    pub fn raw_append(&mut self, parent: NodeId, child: NodeId) {
        self.generation += 1;
        let last = self.node(parent).last_child;
        {
            let c = self.node_mut(child);
            c.parent = Some(parent);
            c.prev = last;
            c.next = None;
        }
        match last {
            Some(l) => self.node_mut(l).next = Some(child),
            None => self.node_mut(parent).first_child = Some(child),
        }
        self.node_mut(parent).last_child = Some(child);
    }

    /// Inserts `child` (detached) before `reference` (a child of `parent`),
    /// or appends when `reference` is `None`. No checks.
    pub fn raw_insert_before(&mut self, parent: NodeId, child: NodeId, reference: Option<NodeId>) {
        self.generation += 1;
        let Some(r) = reference else {
            return self.raw_append(parent, child);
        };
        let prev = self.node(r).prev;
        {
            let c = self.node_mut(child);
            c.parent = Some(parent);
            c.prev = prev;
            c.next = Some(r);
        }
        self.node_mut(r).prev = Some(child);
        match prev {
            Some(p) => self.node_mut(p).next = Some(child),
            None => self.node_mut(parent).first_child = Some(child),
        }
    }

    /// Appends an attribute node (detached) to an element's attribute list.
    /// No duplicate check.
    pub fn raw_append_attr(&mut self, el: NodeId, attr: NodeId) {
        self.generation += 1;
        let last = self.element(el).and_then(|e| e.last_attr);
        {
            let a = self.node_mut(attr);
            a.parent = Some(el);
            a.prev = last;
            a.next = None;
        }
        match last {
            Some(l) => self.node_mut(l).next = Some(attr),
            None => {
                if let Some(e) = self.element_mut(el) {
                    e.first_attr = Some(attr);
                }
            }
        }
        if let Some(e) = self.element_mut(el) {
            e.last_attr = Some(attr);
        }
        self.refresh_id(attr);
    }

    /// Removes and frees all children of `n`.
    pub fn remove_children(&mut self, n: NodeId) {
        while let Some(c) = self.node(n).first_child {
            self.detach(c);
            self.free_subtree(c);
        }
    }

    /// Frees a detached subtree (children, attributes, template contents).
    /// Handles to these nodes become invalid.
    pub fn free_subtree(&mut self, n: NodeId) {
        self.generation += 1;
        if !self.is_live(n) {
            return;
        }
        if self.node(n).holds > 0 {
            self.detach(n);
            return;
        }
        let mut stack = vec![n];
        while let Some(x) = stack.pop() {
            // A held node survives as a detached root with its subtree (its
            // parent and siblings are being freed: their links just go).
            if x != n && self.is_live(x) && self.node(x).holds > 0 {
                let node = self.node_mut(x);
                node.parent = None;
                node.prev = None;
                node.next = None;
                continue;
            }
            let Some(node) = self.nodes[x.0.get() as usize].take() else {
                continue;
            };
            let mut c = node.first_child;
            while let Some(ch) = c {
                c = self.node(ch).next;
                stack.push(ch);
            }
            if let NodeData::Element(e) = &node.data {
                let mut a = e.first_attr;
                while let Some(at) = a {
                    a = self.node(at).next;
                    stack.push(at);
                }
                if let Some(t) = e.template_contents {
                    stack.push(t);
                }
            }
            if let NodeData::Document(d) = &node.data
                && let Some(e) = d.ext_subset
                && self.is_live(e)
                && self.node(e).parent.is_none()
            {
                stack.push(e);
            }
            self.free.push(x.0.get());
        }
    }

    /// The structural change counter: it changes whenever nodes are
    /// attached, detached, freed or attribute values set, so a live
    /// collection can reuse its last result while it is unchanged.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Records a change [`Arena::generation`] users must see (a rename, a
    /// class attribute edited in place).
    pub fn touch(&mut self) {
        self.generation += 1;
    }

    // ----- holds --------------------------------------------------------------

    /// Takes a hold on `id`: it (and its document, and the detached subtree
    /// it is in) stays alive until the matching [`Arena::release`].
    pub fn hold(&mut self, id: NodeId) {
        self.node_mut(id).holds += 1;
        let d = self.doc_of(id);
        self.doc_mut(d).holds += 1;
        // A namespace (or DTD declaration) node refers to its element (or
        // doctype) without being one of its children.
        if self.is_side_node(id)
            && let Some(p) = self.node(id).parent
        {
            self.hold(p);
        }
    }

    /// Namespace and DTD declaration nodes: they have a parent that does
    /// not list them among its children.
    fn is_side_node(&self, id: NodeId) -> bool {
        matches!(
            self.node(id).data,
            NodeData::Namespace(_)
                | NodeData::EntityDecl(_)
                | NodeData::Notation(_)
                | NodeData::ElementDecl(_)
                | NodeData::AttributeDecl(_)
        )
    }

    /// Number of holds on `id`.
    pub fn holds(&self, id: NodeId) -> u32 {
        self.node(id).holds
    }

    /// True if a node of the subtree at `n` (attributes and template
    /// contents included) is held.
    pub fn subtree_held(&self, n: NodeId) -> bool {
        let mut stack = vec![n];
        while let Some(x) = stack.pop() {
            let node = self.node(x);
            if node.holds > 0 {
                return true;
            }
            let mut c = node.first_child;
            while let Some(ch) = c {
                stack.push(ch);
                c = self.node(ch).next;
            }
            if let NodeData::Element(e) = &node.data {
                let mut a = e.first_attr;
                while let Some(at) = a {
                    stack.push(at);
                    a = self.node(at).next;
                }
                if let Some(t) = e.template_contents {
                    stack.push(t);
                }
            }
        }
        false
    }

    /// Releases a hold taken with [`Arena::hold`], freeing the document when
    /// none of its nodes is held any more, or the detached subtree `id` is
    /// in when nothing in it is. Returns the documents freed.
    pub fn release(&mut self, id: NodeId) -> Vec<NodeId> {
        let node = self.node_mut(id);
        node.holds -= 1;
        let d = self.doc_of(id);
        let dd = self.doc_mut(d);
        dd.holds -= 1;
        if dd.holds == 0 {
            self.unheld_docs.push(d);
            return self.collect();
        }
        if self.is_side_node(id) {
            let parent = self.node(id).parent;
            if self.node(id).holds == 0 {
                self.free_namespace_node(id);
            }
            if let Some(p) = parent {
                let mut freed = self.release(p);
                freed.extend(self.collect());
                return freed;
            }
        } else {
            // An unreferenced detached root goes, its referenced
            // descendants surviving as detached roots (PHP frees nodes
            // nothing refers to; tests ext/dom/tests/delayed_freeing).
            let r = self.root(id);
            if r != d && self.node(r).holds == 0 {
                self.free_subtree(r);
            }
        }
        self.collect()
    }

    /// Frees a namespace node (XPath namespace axis results), which is not
    /// one of its element's children.
    pub fn free_namespace_node(&mut self, id: NodeId) {
        if self.is_live(id)
            && matches!(self.node(id).data, NodeData::Namespace(_))
            && self.node(id).holds == 0
        {
            self.nodes[id.0.get() as usize] = None;
            self.free.push(id.0.get());
        }
    }

    /// Frees the documents left without holds (by adoptions moving their
    /// held nodes elsewhere, or by [`Arena::release`]). Returns them.
    pub fn collect(&mut self) -> Vec<NodeId> {
        let mut freed = Vec::new();
        while let Some(d) = self.unheld_docs.pop() {
            if self.is_live(d)
                && matches!(self.node(d).data, NodeData::Document(_))
                && self.doc(d).holds == 0
            {
                self.free_subtree(d);
                freed.push(d);
            }
        }
        freed
    }

    /// Frees a document nothing holds (a document the embedder created and
    /// dropped without taking holds, such as a failed parse).
    pub fn free_document_if_unheld(&mut self, d: NodeId) -> bool {
        if self.is_live(d) && self.doc(d).holds == 0 {
            self.free_subtree(d);
            true
        } else {
            false
        }
    }

    // ----- queries --------------------------------------------------------

    /// The root of the tree containing `n` (following parents).
    pub fn root(&self, mut n: NodeId) -> NodeId {
        while let Some(p) = self.node(n).parent {
            n = p;
        }
        n
    }

    /// True if `n` is in its document's tree.
    pub fn is_connected(&self, n: NodeId) -> bool {
        let r = self.root(n);
        matches!(self.node(r).data, NodeData::Document(_))
    }

    /// True if `anc` is an inclusive ancestor of `n`.
    pub fn is_inclusive_ancestor(&self, anc: NodeId, mut n: NodeId) -> bool {
        loop {
            if n == anc {
                return true;
            }
            match self.node(n).parent {
                Some(p) => n = p,
                None => return false,
            }
        }
    }

    /// The document element.
    pub fn document_element(&self, doc: NodeId) -> Option<NodeId> {
        self.children(doc).find(|&c| self.is_element(c))
    }

    /// Index of `n` among its parent's children.
    pub fn index_in_parent(&self, n: NodeId) -> usize {
        let mut i = 0;
        let mut c = self.node(n).prev;
        while let Some(p) = c {
            i += 1;
            c = self.node(p).prev;
        }
        i
    }

    /// The attribute of `el` with qualified name `name`.
    pub fn get_attr_node(&self, el: NodeId, name: &str) -> Option<NodeId> {
        self.attrs(el)
            .find(|&a| self.attr(a).is_some_and(|d| d.name.is(name)))
    }

    /// The attribute of `el` with namespace `ns` and local name `local`.
    pub fn get_attr_node_ns(&self, el: NodeId, ns: Option<&str>, local: &str) -> Option<NodeId> {
        let ns = ns.filter(|s| !s.is_empty());
        self.attrs(el).find(|&a| {
            self.attr(a).is_some_and(|d| {
                &*d.name.local == local && d.name.ns.as_deref().filter(|s| !s.is_empty()) == ns
            })
        })
    }

    /// The value of the attribute named `name`.
    pub fn get_attribute(&self, el: NodeId, name: &str) -> Option<String> {
        self.get_attr_node(el, name).map(|a| self.attr_value(a))
    }

    /// Sets (adds or replaces) an attribute without namespace by qualified
    /// name; returns the attribute node.
    pub fn set_attribute(&mut self, el: NodeId, name: &str, value: &str) -> NodeId {
        if let Some(a) = self.get_attr_node(el, name) {
            self.set_attr_value(a, value);
            return a;
        }
        let doc = self.doc_of(el);
        let q = match name.split_once(':') {
            Some((p, l)) if !p.is_empty() && !l.is_empty() && !self.is_modern(doc) => QName {
                prefix: Some(p.into()),
                local: l.into(),
                ns: None,
            },
            _ => QName::local(name),
        };
        let a = self.new_attribute(doc, q, value);
        self.raw_append_attr(el, a);
        a
    }

    /// True for `Dom\` (spec-compliant) documents.
    pub fn is_modern(&self, doc: NodeId) -> bool {
        matches!(&self.node(doc).data, NodeData::Document(d) if d.modern)
    }

    /// The element with ID `id` (DOM `getElementById`), in document order
    /// among connected elements.
    pub fn get_element_by_id(&self, doc: NodeId, id: &str) -> Option<NodeId> {
        if let Some(&e) = self.doc(doc).ids.get(id)
            && self.is_live(e)
            && self.is_connected(e)
            && self.doc_of(e) == doc
            && self
                .attrs(e)
                .any(|a| self.attr(a).is_some_and(|d| d.is_id) && self.attr_value(a) == id)
        {
            return Some(e);
        }
        // Fallback: scan (IDs set after insertion, moved nodes).
        self.descendants(doc).find(|&n| {
            self.is_element(n)
                && self
                    .attrs(n)
                    .any(|a| self.attr(a).is_some_and(|d| d.is_id) && self.attr_value(a) == id)
        })
    }

    /// Elements under `root` (descendants only) whose qualified name is
    /// `name` (`*` matches all), in document order.
    pub fn elements_by_tag_name(&self, root: NodeId, name: &str) -> Vec<NodeId> {
        self.descendants(root)
            .filter(|&n| match self.element(n) {
                Some(e) => name == "*" || e.name.is(name),
                None => false,
            })
            .collect()
    }

    /// Elements under `root` by namespace and local name (`*` wildcards;
    /// `ns` of `Some("")` or `None` means no namespace).
    pub fn elements_by_tag_name_ns(
        &self,
        root: NodeId,
        ns: Option<&str>,
        local: &str,
    ) -> Vec<NodeId> {
        let ns = ns.filter(|s| !s.is_empty());
        self.descendants(root)
            .filter(|&n| match self.element(n) {
                Some(e) => {
                    (local == "*" || &*e.name.local == local)
                        && (ns == Some("*") || e.name.ns.as_deref().filter(|s| !s.is_empty()) == ns)
                }
                None => false,
            })
            .collect()
    }

    /// Document order comparison of two nodes in the same tree (attributes
    /// and namespace nodes sort right after their element).
    pub fn cmp_order(&self, a: NodeId, b: NodeId) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        if a == b {
            return Ordering::Equal;
        }
        let pa = self.order_path(a);
        let pb = self.order_path(b);
        if pa[0].0 != pb[0].0 {
            return pa[0].0.cmp(&pb[0].0);
        }
        let mut i = 0;
        while i < pa.len() && i < pb.len() && pa[i] == pb[i] {
            i += 1;
        }
        if i == pa.len() {
            return Ordering::Less;
        }
        if i == pb.len() {
            return Ordering::Greater;
        }
        pa[i].1.cmp(&pb[i].1)
    }

    /// Path from the root: (node, sort key among siblings). Attribute and
    /// namespace nodes get keys below any child.
    fn order_path(&self, n: NodeId) -> Vec<(NodeId, (u8, usize))> {
        let mut v = Vec::new();
        let mut cur = n;
        loop {
            let node = self.node(cur);
            let key = match &node.data {
                NodeData::Namespace(_) => (0u8, 0usize),
                NodeData::Attribute(_) => {
                    let mut i = 0;
                    let mut c = node.prev;
                    while let Some(p) = c {
                        i += 1;
                        c = self.node(p).prev;
                    }
                    (1, i)
                }
                _ => (2, self.index_in_parent(cur)),
            };
            v.push((cur, key));
            match node.parent {
                Some(p) => cur = p,
                None => break,
            }
        }
        v.reverse();
        // Root identity first so different trees compare by root id.
        v[0].1 = (0, v[0].0.index() as usize);
        v
    }

    /// Sorts and dedups nodes into document order.
    pub fn sort_document_order(&self, nodes: &mut Vec<NodeId>) {
        nodes.sort_by(|&a, &b| self.cmp_order(a, b));
        nodes.dedup();
    }

    // ----- namespaces -----------------------------------------------------

    /// The namespace URI bound to `prefix` (`None` = default namespace) in
    /// scope at `n`, following legacy declarations (DOM Level 3
    /// `lookupNamespaceURI` with libxml's model).
    pub fn lookup_ns(&self, n: NodeId, prefix: Option<&str>) -> Option<Str> {
        if prefix == Some("xml") {
            return Some(XML_NS.into());
        }
        if prefix == Some("xmlns") {
            return Some(XMLNS_NS.into());
        }
        let mut cur = Some(match &self.node(n).data {
            NodeData::Attribute(_) | NodeData::Namespace(_) => self.node(n).parent?,
            NodeData::Document(_) => self.document_element(n)?,
            _ => n,
        });
        while let Some(c) = cur {
            if let Some(e) = self.element(c) {
                for d in &e.ns_decls {
                    if d.prefix.as_deref() == prefix {
                        return if d.uri.is_empty() {
                            None
                        } else {
                            Some(d.uri.clone())
                        };
                    }
                }
                // Modern documents: xmlns attributes are attributes.
                for a in self.attrs(c) {
                    let ad = self.attr(a).unwrap();
                    if ad.name.ns.as_deref() == Some(XMLNS_NS) {
                        let decl_prefix = if ad.name.prefix.is_some() {
                            Some(&*ad.name.local)
                        } else {
                            None
                        };
                        if decl_prefix == prefix {
                            let v = self.attr_value(a);
                            return if v.is_empty() { None } else { Some(v.into()) };
                        }
                    }
                }
                if self.is_modern(self.doc_of(c))
                    && e.name.prefix.as_deref() == prefix
                    && e.name.ns.is_some()
                {
                    return e.name.ns.clone();
                }
            }
            cur = self.node(c).parent;
        }
        None
    }

    /// A prefix bound to `uri` in scope at `n` (DOM `lookupPrefix`).
    pub fn lookup_prefix(&self, n: NodeId, uri: &str) -> Option<Str> {
        if uri.is_empty() {
            return None;
        }
        let mut cur = Some(match &self.node(n).data {
            NodeData::Attribute(_) | NodeData::Namespace(_) => self.node(n).parent?,
            NodeData::Document(_) => self.document_element(n)?,
            _ => n,
        });
        while let Some(c) = cur {
            if let Some(e) = self.element(c) {
                for d in &e.ns_decls {
                    if &*d.uri == uri
                        && let Some(p) = &d.prefix
                        && self.lookup_ns(n, Some(p)).as_deref() == Some(uri)
                    {
                        return Some(p.clone());
                    }
                }
            }
            cur = self.node(c).parent;
        }
        None
    }

    /// All namespace declarations in scope at element `el` (innermost wins),
    /// as XPath's namespace axis sees them, plus the implicit `xml` one.
    pub fn in_scope_namespaces(&self, el: NodeId) -> Vec<NsDecl> {
        let mut out: Vec<NsDecl> = Vec::new();
        let mut seen: Vec<Option<Str>> = Vec::new();
        let mut cur = Some(el);
        while let Some(c) = cur {
            if let Some(e) = self.element(c) {
                for d in &e.ns_decls {
                    if !seen.contains(&d.prefix) {
                        seen.push(d.prefix.clone());
                        if !d.uri.is_empty() {
                            out.push(d.clone());
                        }
                    }
                }
                for a in self.attrs(c) {
                    let ad = self.attr(a).unwrap();
                    if ad.name.ns.as_deref() == Some(XMLNS_NS) {
                        let p: Option<Str> = if ad.name.prefix.is_some() {
                            Some(ad.name.local.clone())
                        } else {
                            None
                        };
                        if !seen.contains(&p) {
                            seen.push(p.clone());
                            let v = self.attr_value(a);
                            if !v.is_empty() {
                                out.push(NsDecl {
                                    prefix: p,
                                    uri: v.into(),
                                });
                            }
                        }
                    }
                }
            }
            cur = self.node(c).parent;
        }
        out.push(NsDecl {
            prefix: Some("xml".into()),
            uri: XML_NS.into(),
        });
        out
    }

    /// Adds a namespace declaration on `el` unless the same prefix is
    /// already declared there.
    pub fn declare_ns(&mut self, el: NodeId, prefix: Option<&str>, uri: &str) -> bool {
        let Some(e) = self.element_mut(el) else {
            return false;
        };
        if e.ns_decls.iter().any(|d| d.prefix.as_deref() == prefix) {
            return false;
        }
        e.ns_decls.push(NsDecl {
            prefix: prefix.map(Into::into),
            uri: uri.into(),
        });
        true
    }

    // ----- DOM operations ---------------------------------------------------

    fn can_have_children(&self, n: NodeId) -> bool {
        matches!(
            self.node(n).data,
            NodeData::Document(_)
                | NodeData::Fragment
                | NodeData::Element(_)
                | NodeData::Attribute(_)
        )
    }

    /// The DOM Standard's "ensure pre-insertion validity" for inserting
    /// `node` into `parent` before `child`.
    pub fn check_pre_insert(
        &self,
        parent: NodeId,
        node: NodeId,
        child: Option<NodeId>,
    ) -> Result<(), DomError> {
        if !self.can_have_children(parent) {
            return Err(DomError::HierarchyRequest);
        }
        if self.is_inclusive_ancestor(node, parent) {
            return Err(DomError::HierarchyRequest);
        }
        if let Some(c) = child
            && self.node(c).parent != Some(parent)
        {
            return Err(DomError::NotFound);
        }
        match &self.node(node).data {
            NodeData::Document(_)
            | NodeData::Attribute(_)
            | NodeData::Namespace(_)
            | NodeData::EntityDecl(_)
            | NodeData::Notation(_)
            | NodeData::ElementDecl(_)
            | NodeData::AttributeDecl(_) => return Err(DomError::HierarchyRequest),
            NodeData::Text(_) | NodeData::CData(_)
                if matches!(self.node(parent).data, NodeData::Document(_)) =>
            {
                return Err(DomError::HierarchyRequest);
            }
            NodeData::Doctype(_) if !matches!(self.node(parent).data, NodeData::Document(_)) => {
                return Err(DomError::HierarchyRequest);
            }
            _ => {}
        }
        if matches!(self.node(parent).data, NodeData::Document(_)) {
            let has_el_child = |except: Option<NodeId>| {
                self.children(parent)
                    .any(|c| Some(c) != except && self.is_element(c))
            };
            let doctype_after = |c: Option<NodeId>| {
                let mut cur = c;
                while let Some(x) = cur {
                    if matches!(self.node(x).data, NodeData::Doctype(_)) {
                        return true;
                    }
                    cur = self.node(x).next;
                }
                false
            };
            match &self.node(node).data {
                NodeData::Fragment => {
                    let els = self.children(node).filter(|&c| self.is_element(c)).count();
                    if els > 1 || self.children(node).any(|c| self.is_text(c)) {
                        return Err(DomError::HierarchyRequest);
                    }
                    if els == 1 && (has_el_child(None) || doctype_after(child)) {
                        return Err(DomError::HierarchyRequest);
                    }
                }
                NodeData::Element(_) => {
                    if has_el_child(Some(node)) || doctype_after(child) {
                        return Err(DomError::HierarchyRequest);
                    }
                }
                NodeData::Doctype(_) => {
                    if self
                        .children(parent)
                        .any(|c| c != node && matches!(self.node(c).data, NodeData::Doctype(_)))
                    {
                        return Err(DomError::HierarchyRequest);
                    }
                    let el_before = match child {
                        Some(c) => {
                            let mut cur = self.node(c).prev;
                            let mut found = false;
                            while let Some(x) = cur {
                                if self.is_element(x) {
                                    found = true;
                                }
                                cur = self.node(x).prev;
                            }
                            found
                        }
                        None => has_el_child(None),
                    };
                    if el_before {
                        return Err(DomError::HierarchyRequest);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Inserts `node` before `child` (or appends): checks, then moves it,
    /// expanding fragments. `node` is adopted into `parent`'s document.
    /// Returns the inserted nodes.
    pub fn insert_before(
        &mut self,
        parent: NodeId,
        node: NodeId,
        child: Option<NodeId>,
    ) -> Result<Vec<NodeId>, DomError> {
        self.check_pre_insert(parent, node, child)?;
        let mut child = child;
        if child == Some(node) {
            child = self.node(node).next;
        }
        Ok(self.insert_unchecked(parent, node, child))
    }

    /// Inserts without validity checks (callers that implement legacy
    /// rules). Fragments are expanded.
    pub fn insert_unchecked(
        &mut self,
        parent: NodeId,
        node: NodeId,
        child: Option<NodeId>,
    ) -> Vec<NodeId> {
        let doc = self.doc_of(parent);
        let mut inserted = Vec::new();
        if matches!(self.node(node).data, NodeData::Fragment) {
            while let Some(c) = self.node(node).first_child {
                self.detach(c);
                self.adopt(c, doc);
                self.raw_insert_before(parent, c, child);
                inserted.push(c);
            }
        } else {
            self.detach(node);
            self.adopt(node, doc);
            self.raw_insert_before(parent, node, child);
            inserted.push(node);
        }
        for &n in &inserted {
            if matches!(self.node(n).data, NodeData::Doctype(_))
                && matches!(self.node(parent).data, NodeData::Document(_))
            {
                self.doc_mut(parent).doctype = Some(n);
            }
            self.register_ids(n);
        }
        inserted
    }

    /// Re-registers ID attributes found in the subtree at `n`.
    pub fn register_ids(&mut self, n: NodeId) {
        if !self.is_connected(n) {
            return;
        }
        let doc = self.doc_of(n);
        let mut found = Vec::new();
        let mut stack = vec![n];
        while let Some(x) = stack.pop() {
            if self.is_element(x) {
                for a in self.attrs(x) {
                    if self.attr(a).is_some_and(|d| d.is_id) {
                        found.push((self.attr_value(a), x));
                    }
                }
                stack.extend(self.children(x));
            }
        }
        let ids = &mut self.doc_mut(doc).ids;
        for (v, e) in found {
            ids.entry(v).or_insert(e);
        }
    }

    /// DOM `appendChild`.
    pub fn append_child(&mut self, parent: NodeId, node: NodeId) -> Result<Vec<NodeId>, DomError> {
        self.insert_before(parent, node, None)
    }

    /// DOM `removeChild`.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<NodeId, DomError> {
        if self.node(child).parent != Some(parent)
            || matches!(self.node(child).data, NodeData::Attribute(_))
        {
            return Err(DomError::NotFound);
        }
        self.detach(child);
        Ok(child)
    }

    /// DOM `replaceChild(node, child)`.
    pub fn replace_child(
        &mut self,
        parent: NodeId,
        node: NodeId,
        child: NodeId,
    ) -> Result<NodeId, DomError> {
        if !self.can_have_children(parent) || self.is_inclusive_ancestor(node, parent) {
            return Err(DomError::HierarchyRequest);
        }
        if self.node(child).parent != Some(parent) {
            return Err(DomError::NotFound);
        }
        if node == child {
            return Ok(child);
        }
        let mut reference = self.node(child).next;
        if reference == Some(node) {
            reference = self.node(node).next;
        }
        // Validate as if `child` were removed.
        self.detach(child);
        let r = self.check_pre_insert(parent, node, reference);
        if let Err(e) = r {
            self.raw_insert_before(parent, child, reference);
            return Err(e);
        }
        self.insert_unchecked(parent, node, reference);
        Ok(child)
    }

    /// Sets the owner document of a subtree (DOM "adopt"): detached nodes
    /// only change owner; IDs move along.
    pub fn adopt(&mut self, n: NodeId, doc: NodeId) {
        if self.node(n).doc == Some(doc) || matches!(self.node(n).data, NodeData::Document(_)) {
            return;
        }
        let old_doc = self.doc_of(n);
        let mut moved = 0u32;
        let mut stack = vec![n];
        while let Some(x) = stack.pop() {
            self.node_mut(x).doc = Some(doc);
            let node = self.node(x);
            moved += node.holds;
            let mut c = node.first_child;
            while let Some(ch) = c {
                stack.push(ch);
                c = self.node(ch).next;
            }
            if let NodeData::Element(e) = &node.data {
                let mut a = e.first_attr;
                if let Some(t) = e.template_contents {
                    stack.push(t);
                }
                while let Some(at) = a {
                    stack.push(at);
                    a = self.node(at).next;
                }
            }
        }
        if moved > 0 {
            self.doc_mut(doc).holds += moved;
            let od = self.doc_mut(old_doc);
            od.holds -= moved;
            if od.holds == 0 {
                self.unheld_docs.push(old_doc);
            }
        }
    }

    /// Deep or shallow copy of `n` into document `doc` (DOM `cloneNode` and
    /// `importNode`). The copy is detached.
    pub fn clone_node(&mut self, n: NodeId, doc: NodeId, deep: bool) -> NodeId {
        let src = self.node(n);
        let mut data = src.data.clone();
        let line = src.line;
        if let NodeData::Element(e) = &mut data {
            e.first_attr = None;
            e.last_attr = None;
            e.template_contents = None;
        }
        if let NodeData::Document(d) = &mut data {
            d.doctype = None;
            d.ext_subset = None;
            d.ids.clear();
            d.holds = 0;
        }
        let is_doc = matches!(data, NodeData::Document(_));
        let copy = self.alloc(Node::new(data, if is_doc { None } else { Some(doc) }));
        self.node_mut(copy).line = line;
        let target_doc = if is_doc { copy } else { doc };
        if let Some(e) = self.element(n) {
            let tc = e.template_contents;
            let attrs: Vec<NodeId> = self.attrs(n).collect();
            for a in attrs {
                let ac = self.clone_node(a, target_doc, true);
                self.raw_append_attr(copy, ac);
            }
            if deep && let Some(t) = tc {
                let tcc = self.clone_node(t, target_doc, true);
                self.element_mut(copy).unwrap().template_contents = Some(tcc);
            }
        }
        let attr_like = matches!(self.node(n).data, NodeData::Attribute(_));
        if deep || attr_like {
            let kids: Vec<NodeId> = self.children(n).collect();
            for k in kids {
                let kc = self.clone_node(k, target_doc, true);
                self.raw_append(copy, kc);
                if is_doc && matches!(self.node(kc).data, NodeData::Doctype(_)) {
                    self.doc_mut(copy).doctype = Some(kc);
                }
            }
        }
        copy
    }

    /// DOM `normalize()`: merges adjacent text nodes and removes empty ones
    /// in the subtree.
    pub fn normalize(&mut self, n: NodeId) {
        let mut stack = vec![n];
        while let Some(x) = stack.pop() {
            let mut c = self.node(x).first_child;
            while let Some(ch) = c {
                let next = self.node(ch).next;
                if let NodeData::Text(t) = &self.node(ch).data {
                    if t.is_empty() {
                        self.detach(ch);
                        self.free_subtree(ch);
                        c = next;
                        continue;
                    }
                    let mut nx = next;
                    while let Some(s) = nx {
                        let after = self.node(s).next;
                        let NodeData::Text(st) = &self.node(s).data else {
                            break;
                        };
                        let st = st.clone();
                        if let NodeData::Text(t) = &mut self.node_mut(ch).data {
                            t.push_str(&st);
                        }
                        self.detach(s);
                        self.free_subtree(s);
                        nx = after;
                    }
                    c = nx;
                    continue;
                }
                if matches!(self.node(ch).data, NodeData::Element(_)) {
                    stack.push(ch);
                    let attrs: Vec<NodeId> = self.attrs(ch).collect();
                    stack.extend(attrs);
                }
                c = next;
            }
        }
    }

    /// DOM `isEqualNode`.
    pub fn is_equal_node(&self, a: NodeId, b: NodeId) -> bool {
        let (na, nb) = (self.node(a), self.node(b));
        let same = match (&na.data, &nb.data) {
            (NodeData::Element(x), NodeData::Element(y)) => {
                if x.name != y.name {
                    return false;
                }
                if x.ns_decls.len() != y.ns_decls.len()
                    || !x.ns_decls.iter().all(|d| y.ns_decls.contains(d))
                {
                    return false;
                }
                let aa: Vec<NodeId> = self.attrs(a).collect();
                let ab: Vec<NodeId> = self.attrs(b).collect();
                if aa.len() != ab.len() {
                    return false;
                }
                aa.iter()
                    .all(|&p| ab.iter().any(|&q| self.is_equal_node(p, q)))
            }
            (NodeData::Attribute(x), NodeData::Attribute(y)) => {
                x.name.ns == y.name.ns
                    && x.name.local == y.name.local
                    && self.attr_value(a) == self.attr_value(b)
            }
            (NodeData::Text(x), NodeData::Text(y))
            | (NodeData::CData(x), NodeData::CData(y))
            | (NodeData::Comment(x), NodeData::Comment(y)) => return x == y,
            (
                NodeData::Pi {
                    target: t1,
                    data: d1,
                },
                NodeData::Pi {
                    target: t2,
                    data: d2,
                },
            ) => return t1 == t2 && d1 == d2,
            (NodeData::Doctype(x), NodeData::Doctype(y)) => {
                return x.name == y.name
                    && x.public_id == y.public_id
                    && x.system_id == y.system_id;
            }
            (NodeData::Document(_), NodeData::Document(_))
            | (NodeData::Fragment, NodeData::Fragment) => true,
            (NodeData::EntityRef(x), NodeData::EntityRef(y)) => x == y,
            (NodeData::EntityDecl(x), NodeData::EntityDecl(y)) => {
                // Name and identifiers only (observed on PHP 8.5, test
                // ext/dom/tests/DOMNode_isEqualNode.phpt).
                return x.name == y.name
                    && x.public_id == y.public_id
                    && x.system_id == y.system_id;
            }
            (NodeData::Notation(x), NodeData::Notation(y)) => {
                return x.name == y.name
                    && x.public_id == y.public_id
                    && x.system_id == y.system_id;
            }
            (NodeData::Namespace(x), NodeData::Namespace(y)) => return x == y,
            _ => false,
        };
        if !same {
            return false;
        }
        if matches!(na.data, NodeData::Attribute(_)) {
            return true;
        }
        let ca: Vec<NodeId> = self.children(a).collect();
        let cb: Vec<NodeId> = self.children(b).collect();
        ca.len() == cb.len() && ca.iter().zip(&cb).all(|(&x, &y)| self.is_equal_node(x, y))
    }

    /// The node's source line.
    pub fn line(&self, n: NodeId) -> u32 {
        self.node(n).line
    }

    /// libxml-style node path (`DOMNode::getNodePath`), e.g.
    /// `/root/child[2]/@attr`, `/root/text()[1]`, `/*[1]` for namespaced
    /// elements with a default namespace.
    pub fn node_path(&self, n: NodeId) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut cur = n;
        loop {
            let node = self.node(cur);
            match &node.data {
                NodeData::Document(_) => break,
                NodeData::Element(e) => {
                    let name = if e.name.ns.is_some() && e.name.prefix.is_none() {
                        "*".to_string()
                    } else {
                        e.name.qualified()
                    };
                    let same = |x: NodeId| match self.element(x) {
                        Some(o) => {
                            if name == "*" {
                                true
                            } else {
                                o.name == e.name
                                    || (o.name.prefix == e.name.prefix
                                        && o.name.local == e.name.local)
                            }
                        }
                        None => false,
                    };
                    let (idx, total) = self.sibling_index(cur, same);
                    if name == "*" && total > 1 {
                        parts.push(format!("*[{idx}]"));
                    } else if total > 1 {
                        parts.push(format!("{name}[{idx}]"));
                    } else {
                        parts.push(name);
                    }
                }
                NodeData::Attribute(a) => parts.push(format!("@{}", a.name.qualified())),
                NodeData::Text(_) | NodeData::CData(_) => {
                    let (idx, total) = self.sibling_index(cur, |x| self.is_text(x));
                    parts.push(if total > 1 {
                        format!("text()[{idx}]")
                    } else {
                        "text()".into()
                    });
                }
                NodeData::Comment(_) => {
                    let (idx, total) = self
                        .sibling_index(cur, |x| matches!(self.node(x).data, NodeData::Comment(_)));
                    parts.push(if total > 1 {
                        format!("comment()[{idx}]")
                    } else {
                        "comment()".into()
                    });
                }
                NodeData::Pi { target, .. } => {
                    let (idx, total) = self.sibling_index(
                        cur,
                        |x| matches!(&self.node(x).data, NodeData::Pi { target: t, .. } if t == target),
                    );
                    parts.push(if total > 1 {
                        format!("processing-instruction('{target}')[{idx}]")
                    } else {
                        format!("processing-instruction('{target}')")
                    });
                }
                NodeData::Namespace(ns) => {
                    parts.push(format!("namespace::{}", ns.prefix.as_deref().unwrap_or("")));
                }
                _ => {
                    parts.push(String::new());
                }
            }
            match node.parent {
                Some(p) => cur = p,
                None => {
                    if !matches!(node.data, NodeData::Document(_)) {
                        // Detached subtree: libxml prints a relative path.
                        let mut s = String::new();
                        for (i, p) in parts.iter().rev().enumerate() {
                            if i > 0 {
                                s.push('/');
                            }
                            s.push_str(p);
                        }
                        return s;
                    }
                    break;
                }
            }
        }
        if parts.is_empty() {
            return "/".into();
        }
        let mut s = String::new();
        for p in parts.iter().rev() {
            s.push('/');
            s.push_str(p);
        }
        s
    }

    fn sibling_index(&self, n: NodeId, same: impl Fn(NodeId) -> bool) -> (usize, usize) {
        let mut idx = 1;
        let mut c = self.node(n).prev;
        while let Some(p) = c {
            if same(p) {
                idx += 1;
            }
            c = self.node(p).prev;
        }
        let mut total = idx;
        let mut c = self.node(n).next;
        while let Some(p) = c {
            if same(p) {
                total += 1;
            }
            c = self.node(p).next;
        }
        (idx, total)
    }

    /// Copies a node (deep) from another arena into document `doc` of
    /// this one (`XMLReader::expand($doc)`, cross-arena import). The copy
    /// is detached.
    pub fn import_from(&mut self, src: &Arena, n: NodeId, doc: NodeId) -> NodeId {
        let node = src.node(n);
        let mut data = node.data.clone();
        if let NodeData::Element(e) = &mut data {
            e.first_attr = None;
            e.last_attr = None;
            e.template_contents = None;
        }
        if let NodeData::Document(d) = &mut data {
            d.holds = 0;
        }
        let copy = self.create(doc, data);
        self.node_mut(copy).line = node.line;
        if src.is_element(n) {
            for a in src.attrs(n).collect::<Vec<_>>() {
                let ac = self.import_from(src, a, doc);
                self.raw_append_attr(copy, ac);
            }
        }
        for c in src.children(n).collect::<Vec<_>>() {
            let cc = self.import_from(src, c, doc);
            self.raw_append(copy, cc);
        }
        copy
    }
}

/// The five predefined entities (XML 1.0 section 4.6).
pub fn predefined_entity(name: &str) -> Option<char> {
    Some(match name {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "apos" => '\'',
        "quot" => '"',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_query() {
        let mut a = Arena::new();
        let d = a.new_document(DocKind::Xml);
        let r = a.new_element(d, QName::local("root"));
        a.append_child(d, r).unwrap();
        let c1 = a.new_element(d, QName::local("c"));
        let c2 = a.new_element(d, QName::local("c"));
        a.append_child(r, c1).unwrap();
        a.append_child(r, c2).unwrap();
        let t = a.new_text(d, "hi");
        a.append_child(c2, t).unwrap();
        assert_eq!(a.text_content(r), "hi");
        assert_eq!(a.node_path(t), "/root/c[2]/text()");
        assert_eq!(a.elements_by_tag_name(d, "c"), vec![c1, c2]);
        assert_eq!(a.append_child(c1, r), Err(DomError::HierarchyRequest));
        let r2 = a.new_element(d, QName::local("r2"));
        assert_eq!(a.append_child(d, r2), Err(DomError::HierarchyRequest));
        assert_eq!(a.cmp_order(c2, c1), std::cmp::Ordering::Greater);
        let at = a.set_attribute(c1, "x", "1");
        assert_eq!(a.cmp_order(at, c2), std::cmp::Ordering::Less);
        assert_eq!(a.node_path(at), "/root/c[1]/@x");
    }
}
