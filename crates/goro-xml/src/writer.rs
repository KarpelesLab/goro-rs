//! A streaming XML writer with the output PHP's `XMLWriter` produces
//! (observed on PHP 8.5; fixture `tests/fixtures/writer.php`):
//!
//! - indentation (`setIndent`) puts each element, comment and PI on its own
//!   line, prefixed by the indent string (default one space) per level,
//!   except that text never gets a line break or indentation of its own;
//! - text escapes `<`, `>`, `&`, `"` and CR; attribute values also tab and
//!   line feed;
//! - namespace declarations needed by `*Ns` calls are written after the
//!   attributes, the element's own last;
//! - the document encoding (from `startDocument`) decides the output
//!   bytes; unrepresentable characters become `&#DEC;`.

use crate::chars::{is_name, is_ncname};

/// Why a writer call failed (PHP returns false, or throws for invalid
/// names).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriterError {
    /// The call is not allowed in the current state.
    InvalidState,
    /// The name is not a valid XML name (PHP: ValueError "must be a valid
    /// element/attribute/... name").
    InvalidName,
    /// A namespace prefix is already bound to another URI.
    NamespaceConflict,
}

type R = Result<(), WriterError>;

#[derive(Debug)]
enum Frame {
    Element {
        name: String,
        /// The start tag is still open (`<name attr...`).
        open: bool,
        /// Something was written inside.
        content: bool,
        /// The last thing written inside was text.
        last_text: bool,
        /// Declarations to emit at the end of the start tag.
        pending_ns: Vec<(Option<String>, String)>,
        /// This element's own declaration (emitted last).
        own_ns: Option<(Option<String>, String)>,
        /// Prefixes declared on this element.
        declared: Vec<(Option<String>, String)>,
        /// Attribute names written, to reject duplicates.
        attrs: Vec<String>,
    },
    Attribute,
    Cdata,
    Comment,
    Pi,
    Dtd {
        /// `[` written.
        subset: bool,
    },
    DtdElement,
    DtdAttlist,
    DtdEntity,
}

/// The writer.
#[derive(Debug)]
pub struct XmlWriter {
    out: Vec<u8>,
    stack: Vec<Frame>,
    indent: bool,
    indent_str: String,
    encoding: Option<String>,
    doc_started: bool,
    /// Something has been written at the top level.
    top_content: bool,
    /// Text was written in the open PI (the first gets a space before it).
    pi_text: bool,
}

impl Default for XmlWriter {
    fn default() -> Self {
        XmlWriter::new()
    }
}

fn escape(s: &str, attr: bool, out: &mut String) {
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\r' => out.push_str("&#13;"),
            '\n' if attr => out.push_str("&#10;"),
            '\t' if attr => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
}

impl XmlWriter {
    /// A writer to memory (`openMemory`).
    pub fn new() -> XmlWriter {
        XmlWriter {
            out: Vec::new(),
            stack: Vec::new(),
            indent: false,
            indent_str: " ".into(),
            encoding: None,
            doc_started: false,
            top_content: false,
            pi_text: false,
        }
    }

    /// `setIndent`.
    pub fn set_indent(&mut self, on: bool) {
        self.indent = on;
    }

    /// `setIndentString`.
    pub fn set_indent_string(&mut self, s: &str) {
        self.indent_str = s.to_string();
    }

    /// `flush($empty)` / `outputMemory($flush)`: the buffered output.
    pub fn flush(&mut self, empty: bool) -> Vec<u8> {
        if empty {
            std::mem::take(&mut self.out)
        } else {
            self.out.clone()
        }
    }

    fn emit(&mut self, s: &str) {
        match &self.encoding {
            None => self.out.extend_from_slice(s.as_bytes()),
            Some(e) => {
                let b = crate::serialize::encode_output(s, Some(e));
                self.out.extend_from_slice(&b);
            }
        }
    }

    fn level(&self) -> usize {
        self.stack
            .iter()
            .filter(|f| matches!(f, Frame::Element { .. }))
            .count()
    }

    fn write_indent(&mut self, level: usize) {
        let s = self.indent_str.repeat(level);
        self.emit(&s);
    }

    /// Closes a pending start tag (`>`), emitting namespace declarations.
    fn close_start_tag(&mut self) {
        let mut text = String::new();
        if let Some(Frame::Element {
            open,
            pending_ns,
            own_ns,
            ..
        }) = self.stack.last_mut()
            && *open
        {
            *open = false;
            for (p, u) in pending_ns.drain(..).chain(own_ns.take()) {
                match p {
                    Some(p) => {
                        text.push_str(" xmlns:");
                        text.push_str(&p);
                    }
                    None => text.push_str(" xmlns"),
                }
                text.push_str("=\"");
                escape(&u, true, &mut text);
                text.push('"');
            }
            text.push('>');
        } else {
            return;
        }
        self.emit(&text);
    }

    /// Prepares to write content into the current container. `block` is
    /// true for elements, comments and PIs (which get indentation).
    fn before_content(&mut self, block: bool) -> R {
        if matches!(self.stack.last(), Some(Frame::Attribute)) {
            self.end_attribute()?;
        }
        let was_open = matches!(self.stack.last(), Some(Frame::Element { open: true, .. }));
        self.close_start_tag();
        let level = self.level();
        if let Some(Frame::Element {
            content, last_text, ..
        }) = self.stack.last_mut()
        {
            *content = true;
            *last_text = !block;
        }
        if block && self.indent {
            if was_open {
                self.emit("\n");
            }
            if level > 0 || self.top_content {
                // Top-level blocks after the first need no indent.
            }
            self.write_indent(level);
        }
        if level == 0 {
            self.top_content = true;
        }
        Ok(())
    }

    /// The enclosing element's end tag gets no indentation.
    fn mark_inline(&mut self) {
        if let Some(Frame::Element { last_text, .. }) = self.stack.last_mut() {
            *last_text = true;
        }
    }

    fn after_block(&mut self) {
        if self.indent {
            self.emit("\n");
        }
    }

    /// `startDocument($version, $encoding, $standalone)`.
    pub fn start_document(
        &mut self,
        version: Option<&str>,
        encoding: Option<&str>,
        standalone: Option<&str>,
    ) -> R {
        if self.doc_started || !self.stack.is_empty() {
            return Err(WriterError::InvalidState);
        }
        self.doc_started = true;
        let mut s = format!("<?xml version=\"{}\"", version.unwrap_or("1.0"));
        if let Some(e) = encoding.filter(|e| !e.is_empty()) {
            s.push_str(&format!(" encoding=\"{e}\""));
        }
        if let Some(sa) = standalone.filter(|s| !s.is_empty()) {
            s.push_str(&format!(" standalone=\"{sa}\""));
        }
        s.push_str("?>\n");
        self.emit(&s);
        if let Some(e) = encoding.filter(|e| !e.is_empty() && !e.eq_ignore_ascii_case("UTF-8")) {
            self.encoding = Some(e.to_string());
        }
        Ok(())
    }

    /// `endDocument()`: closes everything open.
    pub fn end_document(&mut self) -> R {
        while let Some(f) = self.stack.last() {
            match f {
                Frame::Element { .. } => self.end_element()?,
                Frame::Attribute => self.end_attribute()?,
                Frame::Cdata => self.end_cdata()?,
                Frame::Comment => self.end_comment()?,
                Frame::Pi => self.end_pi()?,
                Frame::Dtd { .. } => self.end_dtd()?,
                Frame::DtdElement | Frame::DtdAttlist | Frame::DtdEntity => self.end_dtd_part()?,
            }
        }
        if !self.indent {
            self.emit("\n");
        }
        self.doc_started = false;
        Ok(())
    }

    /// `startElement($name)`.
    pub fn start_element(&mut self, name: &str) -> R {
        if !is_name(name) {
            return Err(WriterError::InvalidName);
        }
        self.start_element_inner(name.to_string(), None)
    }

    fn start_element_inner(&mut self, name: String, own_ns: Option<(Option<String>, String)>) -> R {
        if matches!(
            self.stack.last(),
            Some(Frame::Cdata | Frame::Comment | Frame::Pi | Frame::Dtd { .. })
        ) || matches!(
            self.stack.last(),
            Some(Frame::DtdElement | Frame::DtdAttlist | Frame::DtdEntity)
        ) {
            return Err(WriterError::InvalidState);
        }
        self.before_content(true)?;
        self.emit(&format!("<{name}"));
        let declared = own_ns.iter().cloned().collect();
        self.stack.push(Frame::Element {
            name,
            open: true,
            content: false,
            last_text: false,
            pending_ns: Vec::new(),
            own_ns,
            declared,
            attrs: Vec::new(),
        });
        Ok(())
    }

    /// The namespace bound to `prefix` in the open elements.
    fn ns_in_scope(&self, prefix: Option<&str>) -> Option<&str> {
        for f in self.stack.iter().rev() {
            if let Frame::Element {
                declared,
                pending_ns,
                ..
            } = f
            {
                for (p, u) in declared.iter().chain(pending_ns.iter()) {
                    if p.as_deref() == prefix {
                        return Some(u);
                    }
                }
            }
        }
        None
    }

    /// `startElementNs($prefix, $name, $namespace)`.
    pub fn start_element_ns(&mut self, prefix: Option<&str>, name: &str, uri: Option<&str>) -> R {
        if !is_ncname(name) || prefix.is_some_and(|p| !is_ncname(p)) {
            return Err(WriterError::InvalidName);
        }
        let qname = match prefix {
            Some(p) if !p.is_empty() => format!("{p}:{name}"),
            _ => name.to_string(),
        };
        let prefix = prefix.filter(|p| !p.is_empty());
        let own = match uri {
            Some(u) if self.ns_in_scope(prefix) != Some(u) => {
                Some((prefix.map(str::to_string), u.to_string()))
            }
            _ => None,
        };
        self.start_element_inner(qname, own)
    }

    /// `endElement()`.
    pub fn end_element(&mut self) -> R {
        if matches!(self.stack.last(), Some(Frame::Attribute)) {
            self.end_attribute()?;
        }
        if !matches!(self.stack.last(), Some(Frame::Element { .. })) {
            return Err(WriterError::InvalidState);
        }
        let open = matches!(self.stack.last(), Some(Frame::Element { open: true, .. }));
        if open {
            // Empty element: emit declarations then "/>".
            let mut text = String::new();
            if let Some(Frame::Element {
                pending_ns, own_ns, ..
            }) = self.stack.last_mut()
            {
                for (p, u) in pending_ns.drain(..).chain(own_ns.take()) {
                    match p {
                        Some(p) => {
                            text.push_str(" xmlns:");
                            text.push_str(&p);
                        }
                        None => text.push_str(" xmlns"),
                    }
                    text.push_str("=\"");
                    escape(&u, true, &mut text);
                    text.push('"');
                }
            }
            text.push_str("/>");
            self.emit(&text);
            self.stack.pop();
        } else {
            let Some(Frame::Element {
                name, last_text, ..
            }) = self.stack.pop()
            else {
                unreachable!()
            };
            if self.indent && !last_text {
                let level = self.level();
                self.write_indent(level);
            }
            self.emit(&format!("</{name}>"));
        }
        self.after_block();
        Ok(())
    }

    /// `fullEndElement()`: always `</name>`.
    pub fn full_end_element(&mut self) -> R {
        if matches!(self.stack.last(), Some(Frame::Attribute)) {
            self.end_attribute()?;
        }
        if !matches!(self.stack.last(), Some(Frame::Element { .. })) {
            return Err(WriterError::InvalidState);
        }
        if matches!(self.stack.last(), Some(Frame::Element { open: true, .. })) {
            self.close_start_tag();
            if let Some(Frame::Element { last_text, .. }) = self.stack.last_mut() {
                *last_text = true;
            }
        }
        self.end_element()
    }

    /// `startAttribute($name)`.
    pub fn start_attribute(&mut self, name: &str) -> R {
        if !is_name(name) {
            return Err(WriterError::InvalidName);
        }
        self.start_attribute_inner(name.to_string())
    }

    fn start_attribute_inner(&mut self, qname: String) -> R {
        match self.stack.last_mut() {
            Some(Frame::Element {
                open: true, attrs, ..
            }) => {
                if attrs.contains(&qname) {
                    return Err(WriterError::InvalidState);
                }
                attrs.push(qname.clone());
            }
            _ => return Err(WriterError::InvalidState),
        }
        self.emit(&format!(" {qname}=\""));
        self.stack.push(Frame::Attribute);
        Ok(())
    }

    /// `startAttributeNs($prefix, $name, $namespace)`.
    pub fn start_attribute_ns(&mut self, prefix: Option<&str>, name: &str, uri: Option<&str>) -> R {
        if !is_ncname(name) || prefix.is_some_and(|p| !p.is_empty() && !is_ncname(p)) {
            return Err(WriterError::InvalidName);
        }
        let prefix = prefix.filter(|p| !p.is_empty());
        let qname = match prefix {
            Some(p) => format!("{p}:{name}"),
            None => name.to_string(),
        };
        if prefix == Some("xmlns") || (prefix.is_none() && name == "xmlns") {
            // Writing a declaration directly.
            return self.start_attribute_inner(qname);
        }
        if let (Some(p), Some(u)) = (prefix, uri) {
            match self.ns_in_scope(Some(p)) {
                Some(existing) if existing != u => {
                    // Only a conflict if declared on the current element.
                    let on_current = matches!(self.stack.last(),
                        Some(Frame::Element { declared, pending_ns, .. })
                            if declared.iter().chain(pending_ns.iter()).any(|(dp, _)| dp.as_deref() == Some(p)));
                    if on_current {
                        return Err(WriterError::NamespaceConflict);
                    }
                    self.add_pending_ns(p, u)?;
                }
                Some(_) => {}
                None => self.add_pending_ns(p, u)?,
            }
        }
        self.start_attribute_inner(qname)
    }

    fn add_pending_ns(&mut self, p: &str, u: &str) -> R {
        match self.stack.last_mut() {
            Some(Frame::Element {
                open: true,
                pending_ns,
                ..
            }) => {
                pending_ns.push((Some(p.to_string()), u.to_string()));
                Ok(())
            }
            _ => Err(WriterError::InvalidState),
        }
    }

    /// `endAttribute()`.
    pub fn end_attribute(&mut self) -> R {
        if !matches!(self.stack.last(), Some(Frame::Attribute)) {
            return Err(WriterError::InvalidState);
        }
        self.stack.pop();
        self.emit("\"");
        Ok(())
    }

    /// `writeAttribute($name, $value)`.
    pub fn write_attribute(&mut self, name: &str, value: &str) -> R {
        self.start_attribute(name)?;
        self.text(value)?;
        self.end_attribute()
    }

    /// `writeAttributeNs($prefix, $name, $namespace, $value)`.
    pub fn write_attribute_ns(
        &mut self,
        prefix: Option<&str>,
        name: &str,
        uri: Option<&str>,
        value: &str,
    ) -> R {
        self.start_attribute_ns(prefix, name, uri)?;
        self.text(value)?;
        self.end_attribute()
    }

    /// `text($content)`.
    pub fn text(&mut self, s: &str) -> R {
        let mut t = String::new();
        match self.stack.last() {
            Some(Frame::Attribute) => escape(s, true, &mut t),
            Some(Frame::Pi) => {
                if !self.pi_text {
                    self.pi_text = true;
                    t.push(' ');
                }
                t.push_str(s);
            }
            Some(Frame::Cdata | Frame::Comment | Frame::DtdEntity) => t.push_str(s),
            Some(Frame::Element { .. }) | None => {
                self.before_content(false)?;
                escape(s, false, &mut t);
            }
            Some(Frame::Dtd { .. } | Frame::DtdElement | Frame::DtdAttlist) => t.push_str(s),
        }
        self.emit(&t);
        Ok(())
    }

    /// `writeRaw($content)`.
    pub fn write_raw(&mut self, s: &str) -> R {
        if matches!(self.stack.last(), Some(Frame::Element { .. })) {
            self.before_content(false)?;
        }
        self.emit(s);
        Ok(())
    }

    /// `writeElement($name, $content)`: `<name/>` for null content,
    /// `<name></name>` for "".
    pub fn write_element(&mut self, name: &str, content: Option<&str>) -> R {
        self.start_element(name)?;
        if let Some(c) = content {
            self.text(c)?;
            return self.full_end_element();
        }
        self.end_element()
    }

    /// `writeElementNs($prefix, $name, $namespace, $content)`.
    pub fn write_element_ns(
        &mut self,
        prefix: Option<&str>,
        name: &str,
        uri: Option<&str>,
        content: Option<&str>,
    ) -> R {
        self.start_element_ns(prefix, name, uri)?;
        if let Some(c) = content {
            self.text(c)?;
            return self.full_end_element();
        }
        self.end_element()
    }

    /// `startCdata()`.
    pub fn start_cdata(&mut self) -> R {
        if !matches!(self.stack.last(), Some(Frame::Element { .. })) {
            return Err(WriterError::InvalidState);
        }
        self.before_content(false)?;
        self.emit("<![CDATA[");
        self.stack.push(Frame::Cdata);
        Ok(())
    }

    /// `endCdata()`.
    pub fn end_cdata(&mut self) -> R {
        if !matches!(self.stack.last(), Some(Frame::Cdata)) {
            return Err(WriterError::InvalidState);
        }
        self.stack.pop();
        self.emit("]]>");
        Ok(())
    }

    /// `writeCdata($content)`.
    pub fn write_cdata(&mut self, s: &str) -> R {
        self.start_cdata()?;
        self.text(s)?;
        self.end_cdata()
    }

    /// `startComment()`.
    pub fn start_comment(&mut self) -> R {
        if matches!(
            self.stack.last(),
            Some(Frame::Cdata | Frame::Comment | Frame::Pi)
        ) {
            return Err(WriterError::InvalidState);
        }
        self.before_content(true)?;
        self.emit("<!--");
        self.stack.push(Frame::Comment);
        Ok(())
    }

    /// `endComment()`.
    pub fn end_comment(&mut self) -> R {
        if !matches!(self.stack.last(), Some(Frame::Comment)) {
            return Err(WriterError::InvalidState);
        }
        self.stack.pop();
        self.emit("-->");
        self.after_block();
        // The closing tag after a comment is not indented (observed on
        // PHP 8.5).
        self.mark_inline();
        Ok(())
    }

    /// `writeComment($content)`.
    pub fn write_comment(&mut self, s: &str) -> R {
        self.start_comment()?;
        self.text(s)?;
        self.end_comment()
    }

    /// `startPi($target)`.
    pub fn start_pi(&mut self, target: &str) -> R {
        if !is_name(target) {
            return Err(WriterError::InvalidName);
        }
        if target.eq_ignore_ascii_case("xml") {
            return Err(WriterError::InvalidName);
        }
        if matches!(
            self.stack.last(),
            Some(Frame::Cdata | Frame::Comment | Frame::Pi)
        ) {
            return Err(WriterError::InvalidState);
        }
        // A PI gets no line break or indentation before it, one after it,
        // and the closing tag after it no indentation (observed on PHP 8.5).
        self.before_content(false)?;
        self.emit(&format!("<?{target}"));
        self.stack.push(Frame::Pi);
        self.pi_text = false;
        Ok(())
    }

    /// `endPi()`.
    pub fn end_pi(&mut self) -> R {
        if !matches!(self.stack.last(), Some(Frame::Pi)) {
            return Err(WriterError::InvalidState);
        }
        self.stack.pop();
        self.emit("?>");
        self.after_block();
        self.mark_inline();
        Ok(())
    }

    /// `writePi($target, $content)` (a space always separates the content).
    pub fn write_pi(&mut self, target: &str, content: &str) -> R {
        self.start_pi(target)?;
        self.emit(" ");
        self.emit(content);
        self.end_pi()
    }

    /// `startDtd($qualifiedName, $publicId, $systemId)`.
    pub fn start_dtd(&mut self, name: &str, public: Option<&str>, system: Option<&str>) -> R {
        if !self.stack.is_empty() {
            return Err(WriterError::InvalidState);
        }
        if !is_name(name) {
            return Err(WriterError::InvalidName);
        }
        let mut s = format!("<!DOCTYPE {name}");
        match (public, system) {
            (Some(p), Some(sy)) => s.push_str(&format!(" PUBLIC \"{p}\" \"{sy}\"")),
            (Some(p), None) => s.push_str(&format!(" PUBLIC \"{p}\"")),
            (None, Some(sy)) => s.push_str(&format!(" SYSTEM \"{sy}\"")),
            (None, None) => {}
        }
        self.emit(&s);
        self.stack.push(Frame::Dtd { subset: false });
        Ok(())
    }

    /// Opens the internal subset; `Ok(false)` at the top level (DTD
    /// declarations may be written without `startDtd()`, observed on PHP
    /// 8.5: test ext/xmlwriter 008).
    fn open_subset(&mut self) -> Result<bool, WriterError> {
        if self.stack.is_empty() {
            return Ok(false);
        }
        match self.stack.last_mut() {
            Some(Frame::Dtd { subset }) => {
                if !*subset {
                    *subset = true;
                    self.emit(" [");
                    if self.indent {
                        self.emit("\n");
                    }
                }
                Ok(true)
            }
            _ => Err(WriterError::InvalidState),
        }
    }

    /// `endDtd()`.
    pub fn end_dtd(&mut self) -> R {
        if matches!(
            self.stack.last(),
            Some(Frame::DtdElement | Frame::DtdAttlist | Frame::DtdEntity)
        ) {
            self.end_dtd_part()?;
        }
        match self.stack.pop() {
            Some(Frame::Dtd { subset }) => {
                if subset {
                    self.emit("]");
                }
                self.emit(">");
                if self.indent {
                    self.emit("\n");
                }
                self.top_content = true;
                Ok(())
            }
            Some(other) => {
                self.stack.push(other);
                Err(WriterError::InvalidState)
            }
            None => Err(WriterError::InvalidState),
        }
    }

    fn dtd_part(&mut self, frame: Frame, name: &str, head: &str) -> R {
        if !is_name(name) {
            return Err(WriterError::InvalidName);
        }
        let in_subset = self.open_subset()?;
        if self.indent && in_subset {
            let s = self.indent_str.clone();
            self.emit(&s);
        }
        self.emit(head);
        self.stack.push(frame);
        Ok(())
    }

    fn end_dtd_part(&mut self) -> R {
        match self.stack.pop() {
            Some(Frame::DtdEntity) => self.emit("\">"),
            Some(Frame::DtdElement | Frame::DtdAttlist) => self.emit(">"),
            Some(other) => {
                self.stack.push(other);
                return Err(WriterError::InvalidState);
            }
            None => return Err(WriterError::InvalidState),
        }
        if self.indent {
            self.emit("\n");
        }
        Ok(())
    }

    /// `startDtdElement($qualifiedName)`.
    pub fn start_dtd_element(&mut self, name: &str) -> R {
        self.dtd_part(Frame::DtdElement, name, &format!("<!ELEMENT {name} "))
    }
    /// `endDtdElement()`.
    pub fn end_dtd_element(&mut self) -> R {
        self.end_dtd_part()
    }
    /// `writeDtdElement($name, $content)`.
    pub fn write_dtd_element(&mut self, name: &str, content: &str) -> R {
        self.start_dtd_element(name)?;
        self.emit(content);
        self.end_dtd_part()
    }
    /// `startDtdAttlist($name)`.
    pub fn start_dtd_attlist(&mut self, name: &str) -> R {
        self.dtd_part(Frame::DtdAttlist, name, &format!("<!ATTLIST {name} "))
    }
    /// `endDtdAttlist()`.
    pub fn end_dtd_attlist(&mut self) -> R {
        self.end_dtd_part()
    }
    /// `writeDtdAttlist($name, $content)`.
    pub fn write_dtd_attlist(&mut self, name: &str, content: &str) -> R {
        self.start_dtd_attlist(name)?;
        self.emit(content);
        self.end_dtd_part()
    }
    /// `startDtdEntity($name, $isParam)`.
    pub fn start_dtd_entity(&mut self, name: &str, parameter: bool) -> R {
        let head = if parameter {
            format!("<!ENTITY % {name} \"")
        } else {
            format!("<!ENTITY {name} \"")
        };
        self.dtd_part(Frame::DtdEntity, name, &head)
    }
    /// `endDtdEntity()`.
    pub fn end_dtd_entity(&mut self) -> R {
        self.end_dtd_part()
    }
    /// `writeDtdEntity($name, $content, ...)` for an internal entity.
    pub fn write_dtd_entity(&mut self, name: &str, content: &str, parameter: bool) -> R {
        self.start_dtd_entity(name, parameter)?;
        self.emit(content);
        self.end_dtd_part()
    }

    /// `writeDtdEntity($name, $content, $isParam, $publicId, $systemId,
    /// $notationData)` for an external entity (a public or system ID
    /// given): `<!ENTITY name PUBLIC "p" "s" NDATA n>` (observed on PHP
    /// 8.5; test bug39504).
    pub fn write_dtd_external_entity(
        &mut self,
        name: &str,
        parameter: bool,
        public: Option<&str>,
        system: Option<&str>,
        ndata: Option<&str>,
    ) -> R {
        if !is_name(name) {
            return Err(WriterError::InvalidName);
        }
        let in_subset = self.open_subset()?;
        if self.indent && in_subset {
            let s = self.indent_str.clone();
            self.emit(&s);
        }
        let mut t = if parameter {
            format!("<!ENTITY % {name}")
        } else {
            format!("<!ENTITY {name}")
        };
        match (public, system) {
            (Some(p), Some(sy)) => t.push_str(&format!(" PUBLIC \"{p}\" \"{sy}\"")),
            (Some(p), None) => t.push_str(&format!(" PUBLIC \"{p}\"")),
            (None, Some(sy)) => t.push_str(&format!(" SYSTEM \"{sy}\"")),
            (None, None) => {}
        }
        if let Some(n) = ndata.filter(|n| !n.is_empty()) {
            t.push_str(&format!(" NDATA {n}"));
        }
        t.push('>');
        self.emit(&t);
        if self.indent {
            self.emit("\n");
        }
        Ok(())
    }

    /// `writeDtd($name, $publicId, $systemId, $content)`.
    pub fn write_dtd(
        &mut self,
        name: &str,
        public: Option<&str>,
        system: Option<&str>,
        subset: Option<&str>,
    ) -> R {
        self.start_dtd(name, public, system)?;
        if let Some(s) = subset.filter(|s| !s.is_empty()) {
            self.emit(" [");
            self.emit(s);
            self.emit("]");
        }
        self.stack.pop();
        self.emit(">");
        if self.indent {
            self.emit("\n");
        }
        self.top_content = true;
        Ok(())
    }
}
