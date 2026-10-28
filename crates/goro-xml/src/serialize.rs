//! XML serialization as PHP's legacy DOM, SimpleXML and XMLReader print it
//! (`DOMDocument::saveXML`, `SimpleXMLElement::asXML`).
//!
//! Rules observed on PHP 8.5 (fixtures `tests/fixtures/serialize*`):
//! - the XML declaration lists version, the document encoding when set,
//!   and `standalone` when set; each top-level node ends with a newline;
//! - `formatOutput` indents element children by two spaces per level,
//!   only in elements whose children include no text;
//! - without a document encoding, non-ASCII characters are written as
//!   `&#xHEX;`; with an encoding that cannot represent a character, as
//!   `&#DEC;`;
//! - DTDs print notations first, then the declarations in source order.

use goro_charset::{Encoding, Subst, SubstMode};

use crate::tree::{
    Arena, AttrDefault, AttrType, ContentModel, DtdData, DtdItem, EntityDecl, EntityKind, NodeData,
    NodeId, Occur, Particle,
};

/// Serialization settings.
#[derive(Clone, Debug, Default)]
pub struct SaveOptions {
    /// `formatOutput`.
    pub format: bool,
    /// LIBXML_NOEMPTYTAG.
    pub no_empty_tags: bool,
    /// LIBXML_NOXMLDECL.
    pub no_xml_decl: bool,
    /// Indentation string per level (libxml default: two spaces).
    pub indent: Option<String>,
    /// Declare namespaces used but not declared within the output (what
    /// a copied node gets; `XMLReader::readOuterXml`).
    pub fixup_ns: bool,
    /// Write elements without their children.
    pub shallow: bool,
}

impl SaveOptions {
    /// From `formatOutput` and `LIBXML_*` save flags.
    pub fn new(format: bool, flags: u32) -> SaveOptions {
        SaveOptions {
            format,
            no_empty_tags: flags & crate::xml::opt::NOEMPTYTAG != 0,
            no_xml_decl: flags & crate::xml::opt::NOXMLDECL != 0,
            indent: None,
            fixup_ns: false,
            shallow: false,
        }
    }
}

struct Writer<'a> {
    arena: &'a Arena,
    out: String,
    opts: &'a SaveOptions,
    xhtml: bool,
    scope: Vec<(Option<crate::tree::Str>, crate::tree::Str)>,
}

/// Escapes text content (`<`, `>`, `&`, CR).
pub fn escape_text(s: &str, out: &mut String) {
    let bytes = s.as_bytes();
    let mut last = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let rep = match b {
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'&' => "&amp;",
            b'\r' => "&#13;",
            _ => continue,
        };
        out.push_str(&s[last..i]);
        out.push_str(rep);
        last = i + 1;
    }
    out.push_str(&s[last..]);
}

/// Escapes an attribute value (also `"`, tab, LF and CR).
pub fn escape_attr(s: &str, out: &mut String) {
    let bytes = s.as_bytes();
    let mut last = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let rep = match b {
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'&' => "&amp;",
            b'"' => "&quot;",
            b'\t' => "&#9;",
            b'\n' => "&#10;",
            b'\r' => "&#13;",
            _ => continue,
        };
        out.push_str(&s[last..i]);
        out.push_str(rep);
        last = i + 1;
    }
    out.push_str(&s[last..]);
}

/// True for the XHTML 1.0 DTDs, whose documents libxml saves with XHTML
/// rules (observed on PHP 8.5: `<html></html>` instead of `<html/>`).
fn is_xhtml(arena: &Arena, doc: NodeId) -> bool {
    let Some(dt) = arena.doc(doc).doctype else {
        return false;
    };
    let Some(d) = arena.dtd(dt) else {
        return false;
    };
    matches!(
        d.public_id.as_deref(),
        Some("-//W3C//DTD XHTML 1.0 Strict//EN")
            | Some("-//W3C//DTD XHTML 1.0 Transitional//EN")
            | Some("-//W3C//DTD XHTML 1.0 Frameset//EN")
    )
}

/// The XHTML void elements (XHTML 1.0 Appendix C.2: empty elements use the
/// minimized form, others never do).
fn xhtml_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "basefont"
            | "br"
            | "col"
            | "frame"
            | "hr"
            | "img"
            | "input"
            | "isindex"
            | "link"
            | "meta"
            | "param"
    )
}

impl Writer<'_> {
    fn indent(&mut self, level: usize) {
        match &self.opts.indent {
            Some(s) => {
                for _ in 0..level {
                    self.out.push_str(s);
                }
            }
            None => {
                for _ in 0..level {
                    self.out.push_str("  ");
                }
            }
        }
    }

    fn texty(&self, n: NodeId) -> bool {
        matches!(
            self.arena.data(n),
            NodeData::Text(_) | NodeData::CData(_) | NodeData::EntityRef(_)
        )
    }

    fn node(&mut self, n: NodeId, level: usize, format: bool) {
        let arena = self.arena;
        match arena.data(n) {
            NodeData::Document(_) => self.document(n),
            NodeData::Fragment => {
                for c in arena.children(n) {
                    self.node(c, level, format);
                }
            }
            NodeData::Element(e) => {
                let name: std::borrow::Cow<str> = match &e.name.prefix {
                    None => std::borrow::Cow::Borrowed(&e.name.local),
                    Some(_) => std::borrow::Cow::Owned(e.name.qualified()),
                };
                self.out.push('<');
                self.out.push_str(&name);
                let scope_mark = self.scope.len();
                let mut decls = e.ns_decls.clone();
                if self.opts.fixup_ns {
                    for d in &e.ns_decls {
                        self.scope.push((d.prefix.clone(), d.uri.clone()));
                    }
                    let mut need: Vec<(Option<crate::tree::Str>, crate::tree::Str)> = Vec::new();
                    if let Some(ns) = &e.name.ns {
                        need.push((e.name.prefix.clone(), ns.clone()));
                    }
                    for a in arena.attrs(n) {
                        let an = &arena.attr(a).unwrap().name;
                        if let (Some(p), Some(ns)) = (&an.prefix, &an.ns)
                            && &**p != "xml"
                            && &**ns != crate::tree::XMLNS_NS
                        {
                            need.push((Some(p.clone()), ns.clone()));
                        }
                    }
                    for (p, u) in need {
                        let bound = self
                            .scope
                            .iter()
                            .rev()
                            .find(|(sp, _)| *sp == p)
                            .map(|(_, su)| su.clone());
                        if bound.as_deref() != Some(&*u) {
                            decls.push(crate::tree::NsDecl {
                                prefix: p.clone(),
                                uri: u.clone(),
                            });
                            self.scope.push((p, u));
                        }
                    }
                }
                for d in &decls {
                    match &d.prefix {
                        Some(p) => {
                            self.out.push_str(" xmlns:");
                            self.out.push_str(p);
                        }
                        None => self.out.push_str(" xmlns"),
                    }
                    self.out.push_str("=\"");
                    escape_attr(&d.uri, &mut self.out);
                    self.out.push('"');
                }
                for a in arena.attrs(n) {
                    self.out.push(' ');
                    self.attr(a);
                }
                if arena.first_child(n).is_none() || self.opts.shallow {
                    self.scope.truncate(scope_mark);
                    if self.xhtml {
                        if xhtml_void(&name) && e.name.prefix.is_none() {
                            self.out.push_str(" />");
                        } else {
                            self.out.push_str("></");
                            self.out.push_str(&name);
                            self.out.push('>');
                        }
                    } else if self.opts.no_empty_tags {
                        self.out.push_str("></");
                        self.out.push_str(&name);
                        self.out.push('>');
                    } else {
                        self.out.push_str("/>");
                    }
                    return;
                }
                self.out.push('>');
                let fmt = format && !arena.children(n).any(|c| self.texty(c));
                if fmt {
                    self.out.push('\n');
                }
                for c in arena.children(n) {
                    if fmt {
                        self.indent(level + 1);
                    }
                    self.node(c, level + 1, fmt);
                    if fmt {
                        self.out.push('\n');
                    }
                }
                if fmt {
                    self.indent(level);
                }
                self.scope.truncate(scope_mark);
                self.out.push_str("</");
                self.out.push_str(&name);
                self.out.push('>');
            }
            NodeData::Attribute(_) => {
                self.out.push(' ');
                self.attr(n);
            }
            NodeData::Text(t) => escape_text(t, &mut self.out),
            NodeData::CData(t) => {
                // "]]>" cannot appear in a section: split it.
                let mut rest = t.as_str();
                loop {
                    match rest.find("]]>") {
                        Some(i) => {
                            self.out.push_str("<![CDATA[");
                            self.out.push_str(&rest[..i + 2]);
                            self.out.push_str("]]>");
                            rest = &rest[i + 2..];
                        }
                        None => {
                            self.out.push_str("<![CDATA[");
                            self.out.push_str(rest);
                            self.out.push_str("]]>");
                            break;
                        }
                    }
                }
            }
            NodeData::Comment(t) => {
                self.out.push_str("<!--");
                self.out.push_str(t);
                self.out.push_str("-->");
            }
            NodeData::Pi { target, data } => {
                self.out.push_str("<?");
                self.out.push_str(target);
                if !data.is_empty() {
                    self.out.push(' ');
                    self.out.push_str(data);
                }
                self.out.push_str("?>");
            }
            NodeData::EntityRef(name) => {
                self.out.push('&');
                self.out.push_str(name);
                self.out.push(';');
            }
            NodeData::Doctype(d) => self.doctype(d),
            NodeData::EntityDecl(e) => self.entity_decl(e),
            NodeData::Notation(nt) => {
                self.out.push_str("<!NOTATION ");
                self.out.push_str(&nt.name);
                self.external_id(nt.public_id.as_deref(), nt.system_id.as_deref());
                self.out.push_str(" >");
            }
            NodeData::ElementDecl(e) => {
                self.out.push_str("<!ELEMENT ");
                self.out.push_str(&e.name);
                self.out.push(' ');
                content_model(&e.content, &mut self.out);
                self.out.push('>');
            }
            NodeData::AttributeDecl(a) => attr_decl(a, &mut self.out),
            NodeData::Namespace(d) => {
                // libxml prints namespace nodes as their declaration.
                match &d.prefix {
                    Some(p) => {
                        self.out.push_str("xmlns:");
                        self.out.push_str(p);
                    }
                    None => self.out.push_str("xmlns"),
                }
                self.out.push_str("=\"");
                escape_attr(&d.uri, &mut self.out);
                self.out.push('"');
            }
        }
    }

    fn attr(&mut self, a: NodeId) {
        let arena = self.arena;
        let ad = arena.attr(a).unwrap();
        if let Some(p) = &ad.name.prefix {
            self.out.push_str(p);
            self.out.push(':');
        }
        self.out.push_str(&ad.name.local);
        self.out.push_str("=\"");
        for c in arena.children(a) {
            match arena.data(c) {
                NodeData::Text(t) => escape_attr(t, &mut self.out),
                NodeData::EntityRef(name) => {
                    self.out.push('&');
                    self.out.push_str(name);
                    self.out.push(';');
                }
                _ => {}
            }
        }
        self.out.push('"');
    }

    fn external_id(&mut self, public: Option<&str>, system: Option<&str>) {
        match (public, system) {
            (Some(p), Some(s)) => {
                self.out.push_str(" PUBLIC ");
                quote(p, &mut self.out);
                self.out.push(' ');
                quote(s, &mut self.out);
            }
            (Some(p), None) => {
                self.out.push_str(" PUBLIC ");
                quote(p, &mut self.out);
            }
            (None, Some(s)) => {
                self.out.push_str(" SYSTEM ");
                quote(s, &mut self.out);
            }
            (None, None) => {}
        }
    }

    fn doctype(&mut self, d: &DtdData) {
        self.out.push_str("<!DOCTYPE ");
        self.out.push_str(&d.name);
        self.external_id(d.public_id.as_deref(), d.system_id.as_deref());
        if d.items
            .iter()
            .any(|i| !matches!(i, DtdItem::Comment(_) | DtdItem::Pi(..)))
        {
            self.out.push_str(" [\n");
            self.dtd_body(d);
            self.out.push(']');
        }
        self.out.push('>');
    }

    /// The declarations of a subset, as libxml lists them.
    pub fn dtd_body(&mut self, d: &DtdData) {
        // Notations come first, latest declared first (observed on PHP 8.5).
        for it in d.items.iter().rev() {
            if let DtdItem::Notation(n) = it
                && let Some(nt) = d.notations.get(n)
            {
                self.out.push_str("<!NOTATION ");
                self.out.push_str(&nt.name);
                self.external_id(nt.public_id.as_deref(), nt.system_id.as_deref());
                self.out.push_str(" >\n");
            }
        }
        for it in &d.items {
            match it {
                DtdItem::Notation(_) => {}
                DtdItem::Element(n) => {
                    if let Some(e) = d.elements.get(n) {
                        self.out.push_str("<!ELEMENT ");
                        self.out.push_str(&e.name);
                        self.out.push(' ');
                        content_model(&e.content, &mut self.out);
                        self.out.push_str(">\n");
                    }
                }
                DtdItem::Attr(el, an) => {
                    if let Some(a) = d
                        .attlists
                        .get(el)
                        .and_then(|l| l.iter().find(|a| &a.name == an))
                    {
                        attr_decl(a, &mut self.out);
                        self.out.push('\n');
                    }
                }
                DtdItem::Entity(n, pe) => {
                    let map = if *pe { &d.pentities } else { &d.entities };
                    if let Some(e) = map.get(n) {
                        self.entity_decl(e);
                        self.out.push('\n');
                    }
                }
                DtdItem::Comment(c) => {
                    self.out.push_str("<!--");
                    self.out.push_str(c);
                    self.out.push_str("-->");
                }
                DtdItem::Pi(t, data) => {
                    self.out.push_str("<?");
                    self.out.push_str(t);
                    if !data.is_empty() {
                        self.out.push(' ');
                        self.out.push_str(data);
                    }
                    self.out.push_str("?>");
                }
            }
        }
    }

    fn entity_decl(&mut self, e: &EntityDecl) {
        self.out.push_str("<!ENTITY ");
        if matches!(
            e.kind,
            EntityKind::InternalParameter | EntityKind::ExternalParameter
        ) {
            self.out.push_str("% ");
        }
        self.out.push_str(&e.name);
        match e.kind {
            EntityKind::InternalGeneral
            | EntityKind::InternalParameter
            | EntityKind::Predefined => {
                self.out.push(' ');
                let v = e.orig.as_deref().or(e.value.as_deref()).unwrap_or("");
                quote_entity(v, &mut self.out);
            }
            _ => {
                self.external_id(e.public_id.as_deref(), e.system_id.as_deref());
                if let Some(n) = &e.notation {
                    self.out.push_str(" NDATA ");
                    self.out.push_str(n);
                }
            }
        }
        self.out.push('>');
    }

    fn document(&mut self, doc: NodeId) {
        let arena = self.arena;
        let d = arena.doc(doc);
        if !self.opts.no_xml_decl {
            self.out.push_str("<?xml version=\"");
            self.out.push_str(d.version.as_deref().unwrap_or("1.0"));
            self.out.push('"');
            if let Some(e) = &d.encoding {
                self.out.push_str(" encoding=\"");
                self.out.push_str(e);
                self.out.push('"');
            }
            match d.standalone {
                1 => self.out.push_str(" standalone=\"yes\""),
                0 => self.out.push_str(" standalone=\"no\""),
                _ => {}
            }
            self.out.push_str("?>\n");
        }
        for c in arena.children(doc) {
            self.node(c, 0, self.opts.format);
            self.out.push('\n');
        }
    }
}

fn quote(s: &str, out: &mut String) {
    if s.contains('"') {
        out.push('\'');
        out.push_str(s);
        out.push('\'');
    } else {
        out.push('"');
        out.push_str(s);
        out.push('"');
    }
}

fn quote_entity(s: &str, out: &mut String) {
    if s.contains('"') && !s.contains('\'') {
        out.push('\'');
        out.push_str(s);
        out.push('\'');
    } else {
        out.push('"');
        out.push_str(&s.replace('"', "&quot;"));
        out.push('"');
    }
}

fn particle(p: &Particle, out: &mut String) {
    match p {
        Particle::Name(n, o) => {
            out.push_str(n);
            out.push_str(o.suffix());
        }
        Particle::Seq(items, o) | Particle::Choice(items, o) => {
            let sep = if matches!(p, Particle::Seq(..)) {
                " , "
            } else {
                " | "
            };
            out.push('(');
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(sep);
                }
                particle(it, out);
            }
            out.push(')');
            out.push_str(o.suffix());
        }
    }
}

/// Writes a content model as libxml prints it (`(a , b)`, `(a | b)*`).
pub fn content_model(c: &ContentModel, out: &mut String) {
    match c {
        ContentModel::Empty => out.push_str("EMPTY"),
        ContentModel::Any => out.push_str("ANY"),
        ContentModel::Mixed(names) => {
            out.push_str("(#PCDATA");
            for n in names {
                out.push_str(" | ");
                out.push_str(n);
            }
            out.push(')');
            if !names.is_empty() {
                out.push('*');
            }
        }
        ContentModel::Children(p) => {
            // A lone name still gets parentheses, with its occurrence
            // outside: `(c+)` prints as `(c)+` (observed on PHP 8.5).
            match p {
                Particle::Name(n, o) => {
                    out.push('(');
                    out.push_str(n);
                    out.push(')');
                    out.push_str(o.suffix());
                }
                Particle::Seq(items, Occur::One) | Particle::Choice(items, Occur::One)
                    if items.len() == 1 && matches!(items[0], Particle::Name(..)) =>
                {
                    if let Particle::Name(n, o) = &items[0] {
                        out.push('(');
                        out.push_str(n);
                        out.push(')');
                        out.push_str(o.suffix());
                    }
                }
                _ => particle(p, out),
            }
        }
    }
}

/// Writes an `<!ATTLIST>` declaration for one attribute.
pub fn attr_decl(a: &crate::tree::AttrDecl, out: &mut String) {
    out.push_str("<!ATTLIST ");
    out.push_str(&a.element);
    out.push(' ');
    out.push_str(&a.name);
    out.push(' ');
    match a.atype {
        AttrType::Enumeration => {
            out.push('(');
            out.push_str(&a.values.join(" | "));
            out.push(')');
        }
        AttrType::Notation => {
            out.push_str("NOTATION (");
            out.push_str(&a.values.join(" | "));
            out.push(')');
        }
        t => out.push_str(t.keyword()),
    }
    match a.default {
        AttrDefault::Required => out.push_str(" #REQUIRED"),
        AttrDefault::Implied => out.push_str(" #IMPLIED"),
        AttrDefault::Fixed => out.push_str(" #FIXED"),
        AttrDefault::None => {}
    }
    if let Some(v) = &a.value {
        out.push(' ');
        out.push('"');
        out.push_str(v);
        out.push('"');
    }
    out.push('>');
}

/// Encodes serialized UTF-8 text for a document encoding: `None` writes
/// non-ASCII as `&#xHEX;`; an encoding writes what it cannot represent as
/// `&#DEC;` (observed on PHP 8.5).
pub fn encode_output(s: &str, encoding: Option<&str>) -> Vec<u8> {
    let enc = match encoding {
        None => None,
        Some(name) => match crate::encoding::lookup(name) {
            Some(Encoding::Utf8) => return s.as_bytes().to_vec(),
            Some(e) => Some(e),
            None => return s.as_bytes().to_vec(),
        },
    };
    if s.is_ascii() {
        if let Some(e) = enc
            && !e.is_ascii_compatible()
        {
        } else {
            return s.as_bytes().to_vec();
        }
    }
    let mut out = Vec::with_capacity(s.len());
    match enc {
        None => {
            for c in s.chars() {
                if c.is_ascii() {
                    out.push(c as u8);
                } else {
                    out.extend_from_slice(format!("&#x{:X};", c as u32).as_bytes());
                }
            }
        }
        Some(e) => {
            let sub = Subst {
                mode: SubstMode::None,
                ch: 0,
            };
            if matches!(
                e,
                Encoding::Utf16
                    | Encoding::Utf16Le
                    | Encoding::Utf16Be
                    | Encoding::Ucs4
                    | Encoding::Utf32
            ) {
                let cps: Vec<u32> = s.chars().map(|c| c as u32).collect();
                return goro_charset::encode(&cps, e, &sub).bytes;
            }
            let mut buf = [0u32; 1];
            for c in s.chars() {
                if c.is_ascii() && e.is_ascii_compatible() {
                    out.push(c as u8);
                    continue;
                }
                buf[0] = c as u32;
                let r = goro_charset::encode(&buf, e, &sub);
                if r.illegal > 0 || r.bytes.is_empty() {
                    out.extend_from_slice(format!("&#{};", c as u32).as_bytes());
                } else {
                    out.extend_from_slice(&r.bytes);
                }
            }
        }
    }
    out
}

/// Serializes a node (or a whole document) to UTF-8 text without the final
/// encoding step.
pub fn to_string(arena: &Arena, n: NodeId, opts: &SaveOptions) -> String {
    let doc = arena.doc_of(n);
    let mut w = Writer {
        arena,
        out: String::new(),
        opts,
        xhtml: is_xhtml(arena, doc),
        scope: Vec::new(),
    };
    w.node(n, 0, opts.format);
    w.out
}

/// `DOMDocument::saveXML()` / `saveXML($node)`: the bytes PHP returns, in
/// the document's encoding.
pub fn save_xml(arena: &Arena, n: NodeId, opts: &SaveOptions) -> Vec<u8> {
    let doc = arena.doc_of(n);
    let s = to_string(arena, n, opts);
    encode_output(&s, arena.doc(doc).encoding.as_deref())
}

/// The internal subset text (`DOMDocumentType::$internalSubset`).
pub fn internal_subset(d: &DtdData) -> Option<String> {
    if d.items.is_empty() {
        return None;
    }
    let arena = Arena::new();
    let opts = SaveOptions::default();
    let mut w = Writer {
        arena: &arena,
        out: String::new(),
        opts: &opts,
        xhtml: false,
        scope: Vec::new(),
    };
    w.dtd_body(d);
    Some(w.out)
}
