//! Reading parsed XML (WSDL documents, schemas, SOAP messages) from
//! goro-xml's tree: element children, names, attributes and QName values.

use goro_xml::{Arena, NodeId, NodeType};

/// A parsed document.
pub struct InDoc {
    pub arena: Arena,
    pub doc: NodeId,
}

impl InDoc {
    /// Parses `input`; `Err` holds the first error's message.
    pub fn parse(input: &[u8], url: Option<&str>) -> Result<InDoc, String> {
        // Charsets the XML parser lacks but iconv knows (windows-1257...):
        // transcoded to UTF-8 first (test bugs/bug29109.phpt).
        let converted;
        let mut input = input;
        if let Some((name, off)) = goro_xml::encoding::declared_encoding(input)
            && goro_xml::encoding::lookup(&name).is_none()
            && let Some(mut utf8) =
                goro_charset::iconv::iconv(name.as_bytes(), b"UTF-8", input, &mut Vec::new())
            && utf8.len() >= off + name.len()
            && &utf8[off..off + name.len()] == name.as_bytes()
        {
            utf8.splice(off..off + name.len(), b"UTF-8".iter().copied());
            converted = utf8;
            input = &converted;
        }
        let mut arena = Arena::new();
        let mut opts = goro_xml::xml::ParseOptions::with_flags(0);
        opts.url = url.map(|s| s.to_string());
        let b = goro_xml::xml::build(&mut arena, input, &opts, None);
        if !b.ok {
            let msg = b
                .errors
                .first()
                .map(|e| e.message.trim_end().to_string())
                .unwrap_or_default();
            return Err(msg);
        }
        Ok(InDoc { arena, doc: b.doc })
    }

    pub fn root(&self) -> Option<NodeId> {
        self.arena.document_element(self.doc)
    }

    /// Element children of `n`.
    pub fn kids(&self, n: NodeId) -> Vec<NodeId> {
        self.arena
            .children(n)
            .filter(|&c| self.arena.node_type(c) == NodeType::Element)
            .collect()
    }

    /// All children (any type).
    pub fn all_kids(&self, n: NodeId) -> Vec<NodeId> {
        self.arena.children(n).collect()
    }

    pub fn is_elem(&self, n: NodeId) -> bool {
        self.arena.node_type(n) == NodeType::Element
    }

    pub fn is_text(&self, n: NodeId) -> bool {
        matches!(self.arena.node_type(n), NodeType::Text | NodeType::CData)
    }

    pub fn local<'a>(&'a self, n: NodeId) -> &'a str {
        match self.arena.qname(n) {
            Some(q) => &q.local,
            None => "",
        }
    }

    pub fn ns<'a>(&'a self, n: NodeId) -> &'a str {
        match self.arena.qname(n) {
            Some(q) => q.ns_str(),
            None => "",
        }
    }

    pub fn prefix(&self, n: NodeId) -> Option<String> {
        self.arena
            .qname(n)
            .and_then(|q| q.prefix.as_ref().map(|p| p.to_string()))
    }

    pub fn qualified(&self, n: NodeId) -> String {
        self.arena
            .qname(n)
            .map(|q| q.qualified())
            .unwrap_or_default()
    }

    /// Whether `n` is element `local` in namespace `ns`.
    pub fn is(&self, n: NodeId, ns: &str, local: &str) -> bool {
        self.is_elem(n) && self.local(n) == local && self.ns(n) == ns
    }

    /// An attribute without namespace.
    pub fn attr(&self, n: NodeId, name: &str) -> Option<String> {
        for a in self.arena.attrs(n) {
            if let Some(d) = self.arena.attr(a)
                && d.name.ns.is_none()
                && &*d.name.local == name
            {
                return Some(self.arena.attr_value(a));
            }
        }
        None
    }

    /// An attribute in namespace `ns`.
    pub fn attr_ns(&self, n: NodeId, ns: &str, name: &str) -> Option<String> {
        for a in self.arena.attrs(n) {
            if let Some(d) = self.arena.attr(a)
                && d.name.ns.as_deref() == Some(ns)
                && &*d.name.local == name
            {
                return Some(self.arena.attr_value(a));
            }
        }
        None
    }

    /// Attributes: (namespace, local, value).
    pub fn attrs(&self, n: NodeId) -> Vec<(String, String, String)> {
        let mut v = Vec::new();
        for a in self.arena.attrs(n) {
            if let Some(d) = self.arena.attr(a) {
                v.push((
                    d.name.ns_str().to_string(),
                    d.name.local.to_string(),
                    self.arena.attr_value(a),
                ));
            }
        }
        v
    }

    pub fn text(&self, n: NodeId) -> String {
        self.arena.text_content(n)
    }

    /// The namespace a prefix maps to at `n` (`None` prefix: default).
    pub fn lookup(&self, n: NodeId, prefix: Option<&str>) -> Option<String> {
        self.arena.lookup_ns(n, prefix).map(|s| s.to_string())
    }

    /// Resolves a QName value: (namespace, local). An unbound prefix gives
    /// `None`; no prefix takes the default namespace (or "").
    pub fn resolve_qname(&self, n: NodeId, v: &str) -> Option<(String, String)> {
        let v = v.trim();
        match v.find(':') {
            Some(i) => {
                let p = &v[..i];
                let ns = self.lookup(n, Some(p))?;
                Some((ns, v[i + 1..].to_string()))
            }
            None => Some((self.lookup(n, None).unwrap_or_default(), v.to_string())),
        }
    }

    /// Serializes a node with the namespaces it uses declared on it.
    pub fn to_xml_fixup(&self, n: NodeId) -> String {
        let opts = goro_xml::serialize::SaveOptions {
            fixup_ns: true,
            ..Default::default()
        };
        goro_xml::serialize::to_string(&self.arena, n, &opts)
    }

    /// Serializes a node as libxml does (no declaration).
    pub fn to_xml(&self, n: NodeId) -> String {
        let opts = goro_xml::serialize::SaveOptions::default();
        goro_xml::serialize::to_string(&self.arena, n, &opts)
    }
}
