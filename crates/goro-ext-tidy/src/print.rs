//! The pretty printer.
//!
//! Layout rules from the tests' expected output: a block element starts and
//! ends on a line boundary, and a pending line is only ended when something
//! needs a new one, so a node's markup can end without a line break (tests
//! 010.phpt, 012.phpt: `<b>Hi</b>` and `</html>` have no line break,
//! `</head>` and `</body>` do), and that holds for the whole document too.

use crate::config::Encoding;
use crate::lexer::{KEPT_AMP, OPAQUE_BASE, encode_output, representable};
use crate::tags;
use crate::{Attr, Document, NodeId, NodeType};

struct Opts {
    indent: i64,
    indent_spaces: usize,
    with_tabs: bool,
    wrap: usize,
    xhtml: bool,
    xml: bool,
    enc: Encoding,
    numeric: bool,
    quote_amp: bool,
    quote_marks: bool,
    quote_nbsp: bool,
    hide_comments: bool,
    upper_tags: bool,
    upper_attrs: bool,
    indent_attrs: bool,
    wrap_attrs: bool,
    literal_attrs: bool,
    break_before_br: bool,
    hide_endtags: bool,
    sort_alpha: bool,
    priority: Vec<String>,
    vertical: bool,
    indent_cdata: bool,
    nl: &'static str,
}

impl Opts {
    fn new(d: &Document) -> Opts {
        let c = &d.config;
        let output_xml = c.bool("output-xml") || c.bool("input-xml");
        Opts {
            indent: c.int("indent"),
            indent_spaces: c.int("indent-spaces").clamp(0, 256) as usize,
            with_tabs: c.bool("indent-with-tabs"),
            wrap: c.int("wrap").max(0) as usize,
            xhtml: c.bool("output-xhtml"),
            xml: output_xml,
            enc: c.encoding("output-encoding"),
            numeric: c.bool("numeric-entities"),
            quote_amp: c.bool("quote-ampersand"),
            quote_marks: c.bool("quote-marks"),
            quote_nbsp: c.bool("quote-nbsp"),
            hide_comments: c.bool("hide-comments"),
            upper_tags: c.bool("uppercase-tags") && !output_xml && !c.bool("output-xhtml"),
            upper_attrs: c.int("uppercase-attributes") == 1
                && !output_xml
                && !c.bool("output-xhtml"),
            indent_attrs: c.bool("indent-attributes"),
            wrap_attrs: c.bool("wrap-attributes"),
            literal_attrs: c.bool("literal-attributes"),
            break_before_br: c.bool("break-before-br"),
            hide_endtags: c.bool("hide-endtags") || c.bool("omit-optional-tags"),
            sort_alpha: c.int("sort-attributes") == 1,
            priority: c.tag_list("priority-attributes"),
            vertical: c.int("vertical-space") == 1,
            indent_cdata: c.bool("indent-cdata"),
            nl: match c.int("newline") {
                1 => "\r\n",
                2 => "\r",
                _ => "\n",
            },
        }
    }
}

struct Printer<'a> {
    d: &'a Document,
    o: Opts,
    out: String,
    /// The line being built, without its indent.
    line: String,
    width: usize,
    line_indent: usize,
    wrap_at: Option<usize>,
    /// Inside `pre`: newlines are literal and nothing wraps.
    pre: u32,
}

/// The document as saved.
pub fn document(d: &Document) -> Vec<u8> {
    // Test 027.phpt: no markup, no output.
    if !d.config.bool("markup") {
        return Vec::new();
    }
    // The force-output option's meaning: errors stop the output without it.
    if d.error_count() > 0 && !d.config.bool("force-output") {
        return Vec::new();
    }
    let mut p = Printer::new(d);
    let show_body = d.config.int("show-body-only");
    let body = d.body();
    let body_only = match show_body {
        1 => body,
        2 => body.filter(|&b| d.nodes[b].implicit),
        _ => None,
    };
    match body_only {
        Some(b) => {
            for &c in &d.nodes[b].children {
                p.node(c, 0);
            }
        }
        None => p.node(d.root, 0),
    }
    // Tests gh12980.phpt and gh20374.phpt: no line break after </html>; an
    // earlier block's end tag has written its own (test 018.phpt).
    p.flush_pending();
    p.finish()
}

/// One node's markup: the pending line stays unbroken (test 025.phpt).
pub fn node(d: &Document, id: NodeId) -> Vec<u8> {
    let mut p = Printer::new(d);
    p.node(id, 0);
    p.flush_pending();
    p.finish()
}

impl<'a> Printer<'a> {
    fn new(d: &'a Document) -> Printer<'a> {
        Printer {
            d,
            o: Opts::new(d),
            out: String::new(),
            line: String::new(),
            width: 0,
            line_indent: 0,
            wrap_at: None,
            pre: 0,
        }
    }

    fn finish(self) -> Vec<u8> {
        encode_output(&self.out, self.o.enc)
    }

    fn write_indent(&mut self, n: usize) {
        if self.o.with_tabs && self.o.indent_spaces > 0 {
            for _ in 0..n / self.o.indent_spaces {
                self.out.push('\t');
            }
        } else {
            for _ in 0..n {
                self.out.push(' ');
            }
        }
    }

    fn write_line(&mut self) {
        if !self.line.is_empty() {
            self.write_indent(self.line_indent);
            let line = std::mem::take(&mut self.line);
            self.out.push_str(&line);
        }
        self.width = 0;
        self.wrap_at = None;
    }

    /// Writes out the pending text without ending the line.
    fn flush_pending(&mut self) {
        self.write_line();
    }

    /// Ends the line, whatever it holds.
    fn flush(&mut self) {
        self.write_line();
        self.out.push_str(self.o.nl);
    }

    /// Ends the line if something is pending.
    fn cond_flush(&mut self, indent: usize) {
        if !self.line.is_empty() {
            self.flush();
        }
        self.line_indent = indent;
    }

    fn set_indent(&mut self, indent: usize) {
        if self.line.is_empty() {
            self.line_indent = indent;
        }
    }

    /// Adds text with no break opportunity in it.
    fn put(&mut self, s: &str) {
        self.line.push_str(s);
        self.width += s.chars().count();
        self.check_wrap();
    }

    /// Adds a space lines can break at.
    fn put_space(&mut self) {
        if self.pre > 0 {
            self.line.push(' ');
            self.width += 1;
            return;
        }
        self.wrap_at = Some(self.line.len());
        self.line.push(' ');
        self.width += 1;
    }

    fn check_wrap(&mut self) {
        if self.o.wrap == 0 || self.pre > 0 || self.width + self.line_indent <= self.o.wrap {
            return;
        }
        let Some(at) = self.wrap_at else { return };
        if at == 0 {
            return;
        }
        let rest = self.line[at + 1..].to_string();
        self.line.truncate(at);
        self.flush();
        self.width = rest.chars().count();
        self.line = rest;
        self.wrap_at = None;
    }

    fn name(&self, s: &str) -> String {
        if self.o.upper_tags {
            s.to_ascii_uppercase()
        } else {
            s.to_string()
        }
    }

    /// One character of text or of an attribute value as markup.
    fn escaped(&self, c: char, attr: bool, out: &mut String) {
        let v = c as u32;
        match c {
            '&' if self.o.quote_amp => out.push_str("&amp;"),
            KEPT_AMP => out.push('&'),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr || self.o.quote_marks => out.push_str("&quot;"),
            '\u{A0}' if self.o.quote_nbsp => out.push_str(if self.o.numeric || self.o.xml {
                "&#160;"
            } else {
                "&nbsp;"
            }),
            _ if v < 0x80 => out.push(c),
            _ if (OPAQUE_BASE + 0x80..OPAQUE_BASE + 0x100).contains(&v) => out.push(c),
            _ if self.o.numeric => out.push_str(&format!("&#{v};")),
            _ if representable(c, self.o.enc) => out.push(c),
            _ if self.o.xml => out.push_str(&format!("&#{v};")),
            _ => match goro_xml::html::entity_name(c) {
                Some(n) => {
                    out.push('&');
                    out.push_str(n);
                    out.push(';');
                }
                None => out.push_str(&format!("&#{v};")),
            },
        }
    }

    /// Text: spaces are break opportunities outside `pre`.
    fn text(&mut self, s: &str) {
        let mut buf = String::new();
        for c in s.chars() {
            if c == '\n' && self.pre > 0 {
                self.literal_newline();
                continue;
            }
            if c == ' ' {
                self.put_space();
                continue;
            }
            buf.clear();
            self.escaped(c, false, &mut buf);
            let b = std::mem::take(&mut buf);
            self.put(&b);
            buf = b;
        }
    }

    /// A line break written as it is, with no indent on the next line.
    fn literal_newline(&mut self) {
        self.flush();
        self.line_indent = 0;
    }

    /// Text written as it is (pseudo elements, script content).
    fn raw(&mut self, s: &str) {
        for (i, part) in s.split('\n').enumerate() {
            if i > 0 {
                self.literal_newline();
            }
            self.line.push_str(part);
            self.width += part.chars().count();
        }
    }

    fn attr_value(&self, v: &str) -> String {
        let mut out = String::with_capacity(v.len());
        let mut sp = false;
        for c in v.chars() {
            if !self.o.literal_attrs && c.is_ascii_whitespace() {
                if !sp {
                    out.push(' ');
                }
                sp = true;
                continue;
            }
            sp = false;
            self.escaped(c, true, &mut out);
        }
        out
    }

    fn ordered_attrs(&self, attrs: &'a [Attr]) -> Vec<&'a Attr> {
        let mut v: Vec<&Attr> = attrs.iter().collect();
        if self.o.sort_alpha {
            v.sort_by(|a, b| a.name.cmp(&b.name));
        }
        if !self.o.priority.is_empty() {
            let rank = |a: &Attr| {
                self.o
                    .priority
                    .iter()
                    .position(|p| *p == a.name)
                    .unwrap_or(usize::MAX)
            };
            v.sort_by_key(|a| rank(a));
        }
        v
    }

    fn start_tag(&mut self, id: NodeId, indent: usize) {
        let d = self.d;
        let n = &d.nodes[id];
        self.set_indent(indent);
        let name = self.name(&n.name);
        self.put("<");
        self.put(&name);
        let attr_indent = indent + name.chars().count() + 2;
        let xmlish = self.o.xhtml || self.o.xml;
        for (i, a) in self.ordered_attrs(&n.attrs).into_iter().enumerate() {
            if self.o.indent_attrs && i > 0 {
                self.cond_flush(attr_indent);
            } else if self.o.indent_attrs {
                self.line.push(' ');
                self.width += 1;
            } else {
                self.put_space();
            }
            let an = if self.o.upper_attrs {
                a.name.to_ascii_uppercase()
            } else {
                a.name.clone()
            };
            let text = match &a.value {
                // Test gh12980.phpt: `boolean` prints as `boolean=""`, and
                // real boolean attributes keep the short form in HTML.
                None if !xmlish && tags::is_boolean_attr(&a.name) => an,
                None if xmlish && tags::is_boolean_attr(&a.name) => {
                    format!("{an}=\"{}\"", a.name)
                }
                None => format!("{an}=\"\""),
                Some(v) => format!("{an}=\"{}\"", self.attr_value(v)),
            };
            if self.o.wrap_attrs {
                for (j, part) in text.split(' ').enumerate() {
                    if j > 0 {
                        self.put_space();
                    }
                    self.put(part);
                }
            } else {
                self.put(&text);
            }
        }
        if self.o.indent_attrs && !n.attrs.is_empty() {
            self.line_indent = indent.min(self.line_indent);
        }
        let void = n.kind == NodeType::StartEnd;
        if void && xmlish {
            self.put(" />");
        } else {
            self.put(">");
        }
    }

    fn end_tag(&mut self, id: NodeId, indent: usize) {
        let n = &self.d.nodes[id];
        if self.o.hide_endtags && self.d.flags(id) & tags::OPT_END != 0 {
            return;
        }
        self.set_indent(indent);
        let name = self.name(&n.name);
        self.put("</");
        self.put(&name);
        self.put(">");
    }

    fn children(&mut self, id: NodeId, indent: usize) {
        let d = self.d;
        for &c in &d.nodes[id].children {
            self.node(c, indent);
        }
    }

    fn node(&mut self, id: NodeId, indent: usize) {
        let d = self.d;
        let n = &d.nodes[id];
        match n.kind {
            NodeType::Root => {
                // Document-level nodes keep to their own lines; the last one
                // leaves its line pending (test 010.phpt).
                for (i, &c) in n.children.iter().enumerate() {
                    if i > 0 {
                        self.cond_flush(0);
                    }
                    self.node(c, 0);
                }
            }
            NodeType::Text => {
                self.set_indent(indent);
                self.text(&n.text);
            }
            NodeType::Comment => {
                if self.o.hide_comments {
                    return;
                }
                self.set_indent(indent);
                let text = if self.o.xhtml || self.o.xml {
                    n.text.replace("--", "==")
                } else {
                    n.text.clone()
                };
                self.put("<!--");
                self.raw(&text);
                self.put("-->");
            }
            NodeType::Doctype => self.doctype(id),
            // The text of a processing instruction keeps any closing `?`.
            NodeType::ProcIns => self.pseudo(indent, "<?", &n.text, ">"),
            NodeType::Php => self.pseudo(indent, "<?", &n.text, "?>"),
            NodeType::XmlDecl => {
                self.cond_flush(0);
                self.put("<?");
                self.raw(n.text.trim_end());
                self.put("?>");
                self.cond_flush(0);
            }
            NodeType::Asp => self.pseudo(indent, "<%", &n.text, "%>"),
            NodeType::Jste => self.pseudo(indent, "<#", &n.text, "#>"),
            NodeType::Section => {
                self.set_indent(indent);
                self.put("<!");
                self.raw(&n.text);
                self.put(">");
            }
            NodeType::Cdata => {
                self.set_indent(indent);
                self.put("<![CDATA[");
                self.raw(&n.text);
                self.put("]]>");
            }
            NodeType::Start | NodeType::StartEnd | NodeType::End => self.element(id, indent),
        }
    }

    /// `<?php ?>`, `<% %>`, `<# #>` and processing instructions: written as
    /// they came, staying in a line of text; what follows them at the
    /// structural level starts its own line (test 023.phpt).
    fn pseudo(&mut self, indent: usize, open: &str, text: &str, close: &str) {
        self.set_indent(indent);
        self.put(open);
        self.raw(text);
        self.put(close);
    }

    fn doctype(&mut self, id: NodeId) {
        let text = &self.d.nodes[id].text;
        self.cond_flush(0);
        self.put("<!DOCTYPE");
        self.put_space();
        // The system identifier goes on its own line when it doesn't fit
        // (the layout of Tidy's XHTML output, from memory: test 020.phpt
        // accepts any).
        let parts = doctype_parts(text);
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                self.put_space();
            }
            self.put(p);
        }
        self.put(">");
        self.cond_flush(0);
    }

    fn should_indent(&self, id: NodeId) -> bool {
        if self.o.indent == 0 {
            return false;
        }
        let d = self.d;
        let n = &d.nodes[id];
        if n.children.is_empty() || n.name == "html" {
            return false;
        }
        let has_block = n.children.iter().any(|&c| {
            matches!(d.nodes[c].kind, NodeType::Start | NodeType::StartEnd)
                && d.flags(c) & tags::INLINE == 0
        });
        let f = d.flags(id);
        if f & tags::STRUCT != 0 && f & tags::INLINE == 0 {
            return true;
        }
        if has_block {
            return true;
        }
        // Auto indents only where blocks nest; yes also indents the text of
        // elements other than the paragraph-like ones.
        self.o.indent == 1 && f & tags::NO_INDENT == 0
    }

    fn element(&mut self, id: NodeId, indent: usize) {
        let d = self.d;
        let n = &d.nodes[id];
        let f = if n.tag.is_none() && n.user.is_none() {
            // A generic XML element: inline if it holds text.
            self.xml_flags(id)
        } else {
            d.flags(id)
        };
        // Void elements.
        if n.kind == NodeType::StartEnd || f & tags::EMPTY != 0 {
            if f & tags::INLINE == 0 {
                self.cond_flush(indent);
                self.start_tag(id, indent);
                self.cond_flush(indent);
            } else {
                if n.name == "br" && self.o.break_before_br {
                    self.cond_flush(indent);
                }
                self.start_tag(id, indent);
                if n.name == "br" {
                    // Test 020.phpt: a line break follows <br>.
                    self.cond_flush(indent);
                }
            }
            return;
        }
        if f & tags::RAW != 0 {
            self.raw_element(id, indent);
            return;
        }
        if f & tags::PRE != 0 {
            if f & tags::INLINE == 0 {
                self.cond_flush(indent);
            }
            self.start_tag(id, indent);
            self.pre += 1;
            self.children(id, indent);
            self.pre -= 1;
            self.end_tag(id, indent);
            if f & tags::INLINE == 0 {
                self.cond_flush(indent);
            }
            return;
        }
        if f & tags::INLINE != 0 && f & tags::STRUCT == 0 {
            self.start_tag(id, indent);
            self.children(id, indent);
            self.end_tag(id, indent);
            return;
        }
        // Block level: starts on a new line; title-like and head-level
        // elements with text behave the same way.
        self.cond_flush(indent);
        self.start_tag(id, indent);
        let ind = self.should_indent(id);
        let child_indent = if ind {
            indent + self.o.indent_spaces
        } else {
            indent
        };
        if ind || f & tags::STRUCT != 0 {
            self.cond_flush(child_indent);
        }
        self.children(id, child_indent);
        if ind || f & tags::STRUCT != 0 {
            self.cond_flush(indent);
        }
        self.end_tag(id, indent);
        // Test 010.phpt: the end tag of the document element is left pending.
        if n.name != "html" {
            self.cond_flush(indent);
            if self.o.vertical {
                self.flush();
            }
        }
    }

    fn xml_flags(&self, id: NodeId) -> u32 {
        let d = self.d;
        let n = &d.nodes[id];
        let has_text = n
            .children
            .iter()
            .any(|&c| d.nodes[c].kind == NodeType::Text);
        if has_text || n.children.is_empty() {
            tags::BLOCK | tags::NO_INDENT
        } else {
            tags::BLOCK | tags::STRUCT
        }
    }

    /// `script`, `style` and the like: the content stays as written, between
    /// lines of their own.
    fn raw_element(&mut self, id: NodeId, indent: usize) {
        let d = self.d;
        let n = &d.nodes[id];
        let f = d.flags(id);
        if f & tags::PRE == 0 && (f & tags::INLINE == 0 || n.name == "script") {
            self.cond_flush(indent);
        }
        self.start_tag(id, indent);
        let text: String = n
            .children
            .iter()
            .map(|&c| d.nodes[c].text.as_str())
            .collect();
        if f & tags::PRE != 0 {
            self.pre += 1;
            self.text(&text);
            self.pre -= 1;
            self.end_tag(id, indent);
            self.cond_flush(indent);
            return;
        }
        let body = text.trim_matches('\n');
        if !body.trim().is_empty() {
            let cdata = (self.o.xhtml || self.o.xml) && body.contains(['<', '&']);
            let (open, close) = match n.name.as_str() {
                "style" => ("/*<![CDATA[*/", "/*]]>*/"),
                _ => ("//<![CDATA[", "//]]>"),
            };
            self.flush();
            self.line_indent = if self.o.indent_cdata { indent } else { 0 };
            if cdata {
                self.raw(open);
                self.flush();
            }
            self.raw(body);
            self.flush();
            if cdata {
                self.raw(close);
                self.flush();
            }
        }
        self.end_tag(id, indent);
        if f & tags::INLINE == 0 || n.name == "script" {
            self.cond_flush(indent);
        }
    }
}

/// A doctype's text split where it can wrap: name and keywords stay together
/// and each quoted identifier is one piece.
fn doctype_parts(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut quoted: Option<char> = None;
    for c in text.chars() {
        match quoted {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quoted = None;
                    parts.push(std::mem::take(&mut cur));
                }
            }
            None if c == '"' || c == '\'' => {
                let head = std::mem::take(&mut cur);
                if !head.trim().is_empty() {
                    parts.push(head.trim().to_string());
                }
                cur.push(c);
                quoted = Some(c);
            }
            None => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_string());
    }
    parts
}
