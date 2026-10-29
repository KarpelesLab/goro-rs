//! The XML documents ext/soap writes (request and response envelopes, the
//! fault documents): a small element tree whose namespace declarations
//! live on the envelope in the order the encoder first needs them, and its
//! serialization as `<?xml version="1.0" encoding="UTF-8"?>` + the
//! element + a newline, with libxml's escaping (observed on PHP 8.5: every
//! envelope in ext/soap/tests is written that way, with empty elements as
//! `<x/>`).

use goro_xml::serialize::{escape_attr, escape_text};

pub const NS_SOAP11_ENV: &str = "http://schemas.xmlsoap.org/soap/envelope/";
pub const NS_SOAP11_ENC: &str = "http://schemas.xmlsoap.org/soap/encoding/";
pub const NS_SOAP12_ENV: &str = "http://www.w3.org/2003/05/soap-envelope";
pub const NS_SOAP12_ENC: &str = "http://www.w3.org/2003/05/soap-encoding";
pub const NS_SOAP12_RPC: &str = "http://www.w3.org/2003/05/soap-rpc";
pub const NS_XSD: &str = "http://www.w3.org/2001/XMLSchema";
pub const NS_XSD1999: &str = "http://www.w3.org/1999/XMLSchema";
pub const NS_XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
pub const NS_XML: &str = "http://www.w3.org/XML/1998/namespace";
pub const NS_APACHE: &str = "http://xml.apache.org/xml-soap";
pub const NS_WSDL: &str = "http://schemas.xmlsoap.org/wsdl/";
pub const NS_WSDL_SOAP11: &str = "http://schemas.xmlsoap.org/wsdl/soap/";
pub const NS_WSDL_SOAP12: &str = "http://schemas.xmlsoap.org/wsdl/soap12/";
pub const NS_WSDL_HTTP: &str = "http://schemas.xmlsoap.org/wsdl/http/";
pub const NS_WSDL_MIME: &str = "http://schemas.xmlsoap.org/wsdl/mime/";

/// An element index in an [`OutDoc`].
pub type El = usize;

#[derive(Clone, Debug)]
pub enum Kid {
    El(El),
    Text(String),
    /// Already serialized markup (XSD_ANYXML, `<any>` content).
    Raw(String),
}

#[derive(Clone, Debug, Default)]
pub struct Element {
    /// Qualified name.
    pub name: String,
    /// Namespace declarations made on this element: (prefix or "", uri).
    pub ns: Vec<(String, String)>,
    /// Attributes: (qualified name, value).
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<Kid>,
    pub parent: Option<El>,
    /// The element's namespace (for lookups by the decoder of our own
    /// output; not serialized by itself).
    pub ns_uri: Option<String>,
}

/// An output document.
#[derive(Clone, Debug, Default)]
pub struct OutDoc {
    pub els: Vec<Element>,
    /// The element namespace declarations are added to (the envelope).
    pub root: Option<El>,
    /// The next `nsN` number.
    pub next_ns: u32,
    /// The SOAP version (1 or 2) the well-known prefixes follow.
    pub version: i64,
}

impl OutDoc {
    pub fn new(version: i64) -> OutDoc {
        OutDoc {
            els: Vec::new(),
            root: None,
            next_ns: 1,
            version,
        }
    }

    pub fn new_el(&mut self, name: &str) -> El {
        self.els.push(Element {
            name: name.to_string(),
            ..Default::default()
        });
        self.els.len() - 1
    }

    pub fn append(&mut self, parent: El, child: El) {
        self.els[child].parent = Some(parent);
        self.els[parent].kids.push(Kid::El(child));
    }

    pub fn add_child(&mut self, parent: El, name: &str) -> El {
        let e = self.new_el(name);
        self.append(parent, e);
        e
    }

    pub fn add_text(&mut self, el: El, text: &str) {
        if let Some(Kid::Text(t)) = self.els[el].kids.last_mut() {
            t.push_str(text);
            return;
        }
        self.els[el].kids.push(Kid::Text(text.to_string()));
    }

    pub fn add_raw(&mut self, el: El, raw: &str) {
        self.els[el].kids.push(Kid::Raw(raw.to_string()));
    }

    /// Sets an attribute, in place when it exists.
    pub fn set_attr(&mut self, el: El, name: &str, value: &str) {
        let e = &mut self.els[el];
        if let Some(a) = e.attrs.iter_mut().find(|a| a.0 == name) {
            a.1 = value.to_string();
        } else {
            e.attrs.push((name.to_string(), value.to_string()));
        }
    }

    pub fn get_attr(&self, el: El, name: &str) -> Option<&str> {
        self.els[el]
            .attrs
            .iter()
            .find(|a| a.0 == name)
            .map(|a| a.1.as_str())
    }

    pub fn remove_attr(&mut self, el: El, name: &str) {
        self.els[el].attrs.retain(|a| a.0 != name);
    }

    pub fn rename(&mut self, el: El, name: &str) {
        self.els[el].name = name.to_string();
    }

    /// The local part of an element's name.
    pub fn local_name(&self, el: El) -> &str {
        let n = &self.els[el].name;
        match n.find(':') {
            Some(i) => &n[i + 1..],
            None => n,
        }
    }

    /// The text content of an element (its text and its descendants').
    pub fn text_of(&self, el: El) -> String {
        let mut s = String::new();
        for k in &self.els[el].kids {
            match k {
                Kid::Text(t) => s.push_str(t),
                Kid::El(c) => s.push_str(&self.text_of(*c)),
                Kid::Raw(_) => {}
            }
        }
        s
    }

    pub fn has_element_kids(&self, el: El) -> bool {
        self.els[el]
            .kids
            .iter()
            .any(|k| matches!(k, Kid::El(_) | Kid::Raw(_)))
    }

    pub fn is_empty(&self, el: El) -> bool {
        self.els[el].kids.is_empty()
    }

    /// Finds the prefix declared for `uri` on `el` or its ancestors.
    pub fn lookup_prefix(&self, mut el: El, uri: &str) -> Option<String> {
        loop {
            if let Some((p, _)) = self.els[el].ns.iter().find(|(_, u)| u == uri) {
                return Some(p.clone());
            }
            match self.els[el].parent {
                Some(p) => el = p,
                None => break,
            }
        }
        // Not attached yet: the envelope's declarations.
        if let Some(r) = self.root
            && let Some((p, _)) = self.els[r].ns.iter().find(|(_, u)| u == uri)
        {
            return Some(p.clone());
        }
        None
    }

    /// Whether `prefix` is declared on the envelope.
    fn prefix_taken(&self, prefix: &str) -> bool {
        match self.root {
            Some(r) => self.els[r].ns.iter().any(|(p, _)| p == prefix),
            None => false,
        }
    }

    /// The well-known prefix of a namespace.
    pub fn well_known(&self, uri: &str) -> Option<&'static str> {
        Some(match uri {
            NS_XSD => "xsd",
            NS_XSI => "xsi",
            NS_XML => "xml",
            NS_SOAP11_ENV => "SOAP-ENV",
            NS_SOAP11_ENC => "SOAP-ENC",
            NS_SOAP12_ENV => "env",
            NS_SOAP12_ENC => "enc",
            NS_SOAP12_RPC => "rpc",
            _ => return None,
        })
    }

    /// The prefix for `uri`, declaring it on the envelope (`nsN` for
    /// namespaces without a well-known prefix) when it is not in scope at
    /// `el` (observed on PHP 8.5: ext/soap declares every namespace on the
    /// envelope in the order the encoder first needs it).
    pub fn ensure_ns(&mut self, el: Option<El>, uri: &str) -> String {
        if uri == NS_XML {
            return "xml".to_string();
        }
        if let Some(e) = el
            && let Some(p) = self.lookup_prefix(e, uri)
            && !p.is_empty()
        {
            return p;
        }
        if el.is_none()
            && let Some(r) = self.root
            && let Some(p) = self.lookup_prefix(r, uri)
            && !p.is_empty()
        {
            return p;
        }
        let prefix = match self.well_known(uri) {
            Some(p) if !self.prefix_taken(p) => p.to_string(),
            _ => loop {
                let p = format!("ns{}", self.next_ns);
                self.next_ns += 1;
                if !self.prefix_taken(&p) {
                    break p;
                }
            },
        };
        let target = self.root.or(el).unwrap_or(0);
        if !self.els.is_empty() {
            self.els[target].ns.push((prefix.clone(), uri.to_string()));
        }
        prefix
    }

    /// Declares `prefix` for `uri` on `el`.
    pub fn declare(&mut self, el: El, prefix: &str, uri: &str) {
        let e = &mut self.els[el];
        if !e.ns.iter().any(|(p, _)| p == prefix) {
            e.ns.push((prefix.to_string(), uri.to_string()));
        }
    }

    /// `prefix:local` for a namespace (no prefix when `uri` is empty).
    pub fn qname(&mut self, el: Option<El>, uri: &str, local: &str) -> String {
        if uri.is_empty() {
            return local.to_string();
        }
        let p = self.ensure_ns(el, uri);
        format!("{p}:{local}")
    }

    /// Serializes the document with the XML declaration.
    pub fn serialize(&self, root: El) -> String {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        self.write(root, &mut out);
        out.push('\n');
        out
    }

    /// Serializes one element (no declaration).
    pub fn write(&self, el: El, out: &mut String) {
        let e = &self.els[el];
        out.push('<');
        out.push_str(&e.name);
        for (p, u) in &e.ns {
            if p.is_empty() {
                out.push_str(" xmlns=\"");
            } else {
                out.push_str(" xmlns:");
                out.push_str(p);
                out.push_str("=\"");
            }
            escape_attr(u, out);
            out.push('"');
        }
        for (n, v) in &e.attrs {
            out.push(' ');
            out.push_str(n);
            out.push_str("=\"");
            escape_attr(v, out);
            out.push('"');
        }
        if e.kids.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for k in &e.kids {
            match k {
                Kid::El(c) => self.write(*c, out),
                Kid::Text(t) => escape_text(t, out),
                Kid::Raw(r) => out.push_str(r),
            }
        }
        out.push_str("</");
        out.push_str(&e.name);
        out.push('>');
    }
}
