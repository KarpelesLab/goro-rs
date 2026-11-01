//! Builds the repaired tree. The document's skeleton is always
//! html > head (with a title) > body or frameset; misplaced and unknown
//! content is moved or dropped, with a message for each repair.
//!
//! The order of the parsing steps follows what the messages and positions in
//! tests 004.phpt, 006.phpt, 010.phpt and 012.phpt show: the head is inferred
//! at the first token that can't be part of it, with that token's position;
//! the missing title is reported against the head after the whole input has
//! been read, and the inserted title has the end of input's position.

use crate::lexer::{Lexer, TokKind, Token, decode_input};
use crate::tags::{self, Tag};
use crate::{Config, Document, Level, NodeId, NodeType, UserKind};

/// Open elements beyond this are dropped: the printer and the cleaner recurse
/// over the tree.
const MAX_DEPTH: usize = 512;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    BeforeHtml,
    InHtml,
    InHead,
    AfterHead,
    Body,
    Frames,
    AfterFrames,
}

/// An inline element closed by a block start tag, to reopen in that block.
#[derive(Clone)]
struct Pending {
    name: String,
    attrs: Vec<crate::Attr>,
    tag: Option<&'static Tag>,
    user: Option<UserKind>,
}

struct Parser<'d> {
    d: &'d mut Document,
    lex: Lexer,
    phase: Phase,
    html: Option<NodeId>,
    head: Option<NodeId>,
    open: Vec<NodeId>,
    pending: Vec<Pending>,
    seen_doctype_check: bool,
    body_closed: bool,
    after_body_reported: bool,
    user_block: Vec<String>,
    user_empty: Vec<String>,
    user_inline: Vec<String>,
    user_pre: Vec<String>,
    custom: Option<UserKind>,
    last_pos: (u32, u32),
}

pub fn parse(d: &mut Document, input: &[u8]) {
    let cfg: &Config = &d.config;
    let (chars, invalid) = decode_input(input, cfg.encoding("input-encoding"));
    let xml = cfg.bool("input-xml");
    let tab = cfg.int("tab-size").clamp(1, 64) as u32;
    let lex = Lexer::new(
        chars,
        tab,
        xml,
        cfg.bool("preserve-entities"),
        cfg.bool("assume-xml-procins"),
    );
    if invalid {
        d.report(
            Level::Warning,
            1,
            1,
            "replacing invalid character sequences in the input",
        );
    }
    let custom = match d.config.int("custom-tags") {
        1 => Some(UserKind::Block),
        2 => Some(UserKind::Empty),
        3 => Some(UserKind::Inline),
        4 => Some(UserKind::Pre),
        _ => None,
    };
    let user_block = d.config.tag_list("new-blocklevel-tags");
    let user_empty = d.config.tag_list("new-empty-tags");
    let user_inline = d.config.tag_list("new-inline-tags");
    let user_pre = d.config.tag_list("new-pre-tags");
    let mut p = Parser {
        d,
        lex,
        phase: Phase::BeforeHtml,
        html: None,
        head: None,
        open: Vec::new(),
        pending: Vec::new(),
        seen_doctype_check: false,
        body_closed: false,
        after_body_reported: false,
        user_block,
        user_empty,
        user_inline,
        user_pre,
        custom,
        last_pos: (1, 1),
    };
    if xml {
        p.parse_xml();
    } else {
        p.parse_html();
        p.finish_html();
    }
}

fn is_ws(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_whitespace())
}

fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut sp = false;
    for c in s.chars() {
        if c.is_ascii_whitespace() {
            if !sp {
                out.push(' ');
            }
            sp = true;
        } else {
            out.push(c);
            sp = false;
        }
    }
    out
}

impl Parser<'_> {
    fn next(&mut self) -> Token {
        let t = self.lex.next_token();
        for (l, a, b, m) in std::mem::take(&mut self.lex.msgs) {
            self.d.report(l, a, b, &m);
        }
        self.last_pos = (t.line, t.col);
        t
    }

    fn warn(&mut self, t: &Token, m: &str) {
        self.d.report(Level::Warning, t.line, t.col, m);
    }

    /// The tag behaviour of a name: built in, declared, or an autonomous
    /// custom element when `custom-tags` is on (test gh20374.phpt; the HTML
    /// Standard's custom element names contain a hyphen).
    fn resolve(&self, name: &str) -> (Option<&'static Tag>, Option<UserKind>) {
        let has = |l: &Vec<String>| l.iter().any(|n| n == name);
        if has(&self.user_block) {
            return (None, Some(UserKind::Block));
        }
        if has(&self.user_empty) {
            return (None, Some(UserKind::Empty));
        }
        if has(&self.user_inline) {
            return (None, Some(UserKind::Inline));
        }
        if has(&self.user_pre) {
            return (None, Some(UserKind::Pre));
        }
        if let Some(t) = tags::lookup(name) {
            return (Some(t), None);
        }
        if self.custom.is_some() && name.find('-').is_some_and(|i| i > 0) {
            return (None, self.custom);
        }
        (None, None)
    }

    fn doctype_check(&mut self, t: &Token) {
        if !self.seen_doctype_check {
            self.seen_doctype_check = true;
            if self.d.doctype().is_none() {
                self.warn(t, "missing <!DOCTYPE> declaration");
            }
        }
    }

    /// A node for a token that isn't an element.
    fn leaf(&mut self, t: &Token) -> Option<NodeId> {
        let kind = match t.kind {
            TokKind::Comment => NodeType::Comment,
            TokKind::ProcIns => NodeType::ProcIns,
            TokKind::Asp => NodeType::Asp,
            TokKind::Jste => NodeType::Jste,
            TokKind::Php => NodeType::Php,
            TokKind::Cdata => NodeType::Cdata,
            TokKind::Section => NodeType::Section,
            TokKind::XmlDecl => NodeType::XmlDecl,
            _ => return None,
        };
        let id = self.d.new_node(kind, "", t.line, t.col);
        self.d.nodes[id].text = t.text.clone();
        Some(id)
    }

    fn element(
        &mut self,
        name: &str,
        tag: Option<&'static Tag>,
        user: Option<UserKind>,
        line: u32,
        col: u32,
    ) -> NodeId {
        let id = self.d.new_node(NodeType::Start, name, line, col);
        let n = &mut self.d.nodes[id];
        n.tag = tag;
        n.user = user;
        if tag.is_some_and(|t| t.flags & tags::PROPRIETARY != 0) {
            self.d.proprietary_seen = true;
        }
        id
    }

    /// An element for a built-in tag inserted by the parser.
    fn inferred(&mut self, name: &str, line: u32, col: u32) -> NodeId {
        let id = self.element(name, tags::lookup(name), None, line, col);
        self.d.nodes[id].implicit = true;
        id
    }

    /// Attributes of a token, with repeated ones resolved
    /// (`repeated-attributes`).
    fn set_attrs(&mut self, id: NodeId, t: &Token) {
        let keep_last = self.d.config.int("repeated-attributes") == 1;
        let mut attrs: Vec<crate::Attr> = Vec::with_capacity(t.attrs.len());
        for a in &t.attrs {
            if let Some(old) = attrs.iter_mut().find(|o| o.name == a.name) {
                let dropped = if keep_last {
                    old.value.clone()
                } else {
                    a.value.clone()
                };
                let m = format!(
                    "<{}> dropping value \"{}\" for repeated attribute \"{}\"",
                    t.name,
                    dropped.unwrap_or_default(),
                    a.name
                );
                self.warn(t, &m);
                if keep_last {
                    *old = a.clone();
                }
            } else {
                attrs.push(a.clone());
            }
        }
        if t.name == "img" && !attrs.iter().any(|a| a.name == "alt") {
            self.warn(t, "<img> lacks \"alt\" attribute");
        }
        if t.name == "html" && attrs.iter().any(|a| a.name == "xmlns") {
            self.d.xhtml = true;
        }
        self.d.nodes[id].attrs = attrs;
    }

    fn top(&self) -> Option<NodeId> {
        self.open.last().copied()
    }

    fn flags(&self, id: NodeId) -> u32 {
        self.d.flags(id)
    }

    // ----- the skeleton ---------------------------------------------------

    fn parse_html(&mut self) {
        loop {
            let t = self.next();
            let eof = t.kind == TokKind::Eof;
            self.dispatch(t);
            if eof {
                break;
            }
        }
    }

    /// Hands a token to the phase's handler, which may move to another phase
    /// and leave the token for it.
    fn dispatch(&mut self, t: Token) {
        let mut t = Some(t);
        while let Some(tok) = t.take() {
            t = match self.phase {
                Phase::BeforeHtml => self.before_html(tok),
                Phase::InHtml => self.in_html(tok),
                Phase::InHead => self.in_head(tok),
                Phase::AfterHead => self.after_head(tok),
                Phase::Body => self.in_body(tok),
                Phase::Frames => self.in_frames(tok),
                Phase::AfterFrames => self.after_frames(tok),
            };
        }
    }

    /// A start tag of an unknown element: the error and the discard message
    /// of test 006.phpt. `true` when the token was dropped.
    fn drop_unknown(&mut self, t: &Token) -> bool {
        if t.kind == TokKind::Start && matches!(self.resolve(&t.name), (None, None)) {
            let m = format!("<{}> is not recognized!", t.name);
            self.d.report(Level::Error, t.line, t.col, &m);
            self.warn(t, &format!("discarding unexpected <{}>", t.name));
            return true;
        }
        false
    }

    fn before_html(&mut self, t: Token) -> Option<Token> {
        let root = self.d.root;
        match t.kind {
            TokKind::Text if is_ws(&t.text) => None,
            TokKind::Doctype => {
                self.add_doctype(&t);
                None
            }
            TokKind::Comment
            | TokKind::ProcIns
            | TokKind::Asp
            | TokKind::Jste
            | TokKind::Php
            | TokKind::XmlDecl
            | TokKind::Section
            | TokKind::Cdata => {
                let id = self.leaf(&t)?;
                self.d.append(root, id);
                None
            }
            TokKind::Start if t.name == "html" => {
                self.doctype_check(&t);
                let id = self.element("html", tags::lookup("html"), None, t.line, t.col);
                self.set_attrs(id, &t);
                self.d.append(root, id);
                self.html = Some(id);
                self.phase = Phase::InHtml;
                None
            }
            TokKind::End => {
                self.doctype_check(&t);
                self.warn(&t, &format!("discarding unexpected </{}>", t.name));
                None
            }
            TokKind::Start if self.resolve(&t.name).1 == Some(UserKind::Empty) => {
                // An empty element declared through the configuration stays
                // where it is, ahead of the document (test gh20374.phpt).
                self.doctype_check(&t);
                let (tag, user) = self.resolve(&t.name);
                let id = self.element(&t.name, tag, user, t.line, t.col);
                self.d.nodes[id].kind = NodeType::StartEnd;
                self.set_attrs(id, &t);
                self.d.append(root, id);
                None
            }
            _ => {
                self.doctype_check(&t);
                let id = self.inferred("html", t.line, t.col);
                self.d.append(root, id);
                self.html = Some(id);
                self.phase = Phase::InHtml;
                Some(t)
            }
        }
    }

    fn add_doctype(&mut self, t: &Token) {
        if self.d.doctype().is_some() {
            self.warn(t, "discarding unexpected <!DOCTYPE>");
            return;
        }
        let id = self.d.new_node(NodeType::Doctype, "html", t.line, t.col);
        self.d.nodes[id].text = t.text.clone();
        let root = self.d.root;
        self.d.append(root, id);
        if t.text.contains("XHTML") {
            self.d.xhtml = true;
        }
        // The name after the keyword is the root element's.
        if let Some(n) = t.text.split_whitespace().next() {
            self.d.nodes[id].name = n.to_ascii_lowercase();
        }
    }

    fn html_id(&self) -> NodeId {
        self.html.unwrap_or(self.d.root)
    }

    fn start_head(&mut self, line: u32, col: u32) {
        let id = self.inferred("head", line, col);
        let h = self.html_id();
        self.d.append(h, id);
        self.head = Some(id);
    }

    fn in_html(&mut self, t: Token) -> Option<Token> {
        let h = self.html_id();
        match t.kind {
            TokKind::Text if is_ws(&t.text) => None,
            TokKind::Comment
            | TokKind::ProcIns
            | TokKind::Asp
            | TokKind::Jste
            | TokKind::Php
            | TokKind::Section => {
                let id = self.leaf(&t)?;
                self.d.append(h, id);
                None
            }
            TokKind::Doctype | TokKind::XmlDecl => {
                self.warn(&t, "discarding unexpected <!DOCTYPE>");
                None
            }
            // Test 004.phpt: nothing has opened yet that this could close.
            TokKind::End => {
                self.warn(&t, &format!("discarding unexpected </{}>", t.name));
                None
            }
            TokKind::Start if t.name == "head" => {
                self.start_head(t.line, t.col);
                if let Some(id) = self.head {
                    self.set_attrs(id, &t);
                    self.d.nodes[id].implicit = false;
                }
                self.phase = Phase::InHead;
                None
            }
            TokKind::Start if t.name == "html" => {
                self.warn(&t, "discarding unexpected <html>");
                None
            }
            TokKind::Start
                if tags::lookup(&t.name).is_some_and(|g| g.flags & tags::HEAD != 0) =>
            {
                self.start_head(t.line, t.col);
                self.phase = Phase::InHead;
                Some(t)
            }
            _ => {
                self.start_head(t.line, t.col);
                self.phase = Phase::AfterHead;
                Some(t)
            }
        }
    }

    fn in_head(&mut self, t: Token) -> Option<Token> {
        let head = self.head.unwrap_or(self.d.root);
        match t.kind {
            TokKind::Text if is_ws(&t.text) => None,
            TokKind::Comment
            | TokKind::ProcIns
            | TokKind::Asp
            | TokKind::Jste
            | TokKind::Php
            | TokKind::Section => {
                let id = self.leaf(&t)?;
                self.d.append(head, id);
                None
            }
            TokKind::End if t.name == "head" => {
                self.phase = Phase::AfterHead;
                None
            }
            TokKind::End if t.name != "html" && t.name != "body" => {
                self.warn(&t, &format!("discarding unexpected </{}>", t.name));
                None
            }
            TokKind::Start
                if t.name == "head" => {
                self.warn(&t, "discarding unexpected <head>");
                None
            }
            TokKind::Start
                if tags::lookup(&t.name).is_some_and(|g| g.flags & tags::HEAD != 0)
                    || t.name == "noscript" =>
            {
                if t.name == "noscript" {
                    self.phase = Phase::AfterHead;
                    return Some(t);
                }
                self.head_element(&t);
                None
            }
            TokKind::Text => {
                if !self.d.nodes[head].implicit {
                    self.warn(&t, "plain text isn't allowed in <head> elements");
                }
                self.phase = Phase::AfterHead;
                Some(t)
            }
            _ => {
                self.phase = Phase::AfterHead;
                Some(t)
            }
        }
    }

    /// Reads a head-level element (with its text content, if any) into the
    /// head.
    fn head_element(&mut self, t: &Token) {
        let head = self.head.unwrap_or(self.d.root);
        if t.name == "title"
            && self.d.nodes[head]
                .children
                .iter()
                .any(|&c| self.d.is_element(c, "title"))
        {
            self.warn(t, "discarding unexpected <title>");
            self.skip_text_element(&t.name, true);
            return;
        }
        let id = self.build_element(t);
        self.d.append(head, id);
        self.read_text_content(id, t);
    }

    fn skip_text_element(&mut self, name: &str, decode: bool) {
        let _ = self.lex.raw_until_end(name, decode);
        let e = self.next();
        if e.kind != TokKind::End && e.kind != TokKind::Eof {
            self.dispatch(e);
        }
    }

    fn build_element(&mut self, t: &Token) -> NodeId {
        let (tag, user) = self.resolve(&t.name);
        let id = self.element(&t.name, tag, user, t.line, t.col);
        self.set_attrs(id, t);
        if self.flags(id) & tags::EMPTY != 0 {
            self.d.nodes[id].kind = NodeType::StartEnd;
        }
        id
    }

    /// The content of raw-text and title-like elements. Returns whether the
    /// element was one.
    fn read_text_content(&mut self, id: NodeId, t: &Token) -> bool {
        let f = self.flags(id);
        if f & (tags::RAW | tags::RCDATA) == 0 {
            return f & tags::EMPTY != 0;
        }
        if t.self_closed {
            return true;
        }
        let (mut text, line, col) = self.lex.raw_until_end(&t.name, f & tags::RCDATA != 0);
        if t.name == "title" {
            text = collapse(&text).trim().to_string();
        }
        if !text.is_empty() {
            let c = self.d.new_node(NodeType::Text, "", line, col);
            self.d.nodes[c].text = text;
            self.d.append(id, c);
        }
        let e = self.next();
        if e.kind == TokKind::Eof {
            self.warn(&e, &format!("missing </{}>", t.name));
            self.dispatch(e);
        }
        true
    }

    fn after_head(&mut self, t: Token) -> Option<Token> {
        let h = self.html_id();
        match t.kind {
            TokKind::Text if is_ws(&t.text) => None,
            TokKind::Comment
            | TokKind::ProcIns
            | TokKind::Asp
            | TokKind::Jste
            | TokKind::Php
            | TokKind::Section => {
                let id = self.leaf(&t)?;
                self.d.append(h, id);
                None
            }
            TokKind::Doctype | TokKind::XmlDecl => {
                self.warn(&t, "discarding unexpected <!DOCTYPE>");
                None
            }
            TokKind::End => {
                self.warn(&t, &format!("discarding unexpected </{}>", t.name));
                None
            }
            TokKind::Eof => {
                // Test 004.phpt: an empty document gets its body silently.
                let id = self.inferred("body", t.line, t.col);
                self.d.append(h, id);
                self.open.push(id);
                self.phase = Phase::Body;
                Some(t)
            }
            TokKind::Start if self.drop_unknown(&t) => None,
            TokKind::Start if t.name == "body" => {
                let id = self.build_element(&t);
                self.d.append(h, id);
                self.open.push(id);
                self.phase = Phase::Body;
                None
            }
            TokKind::Start if t.name == "frameset" => {
                let id = self.build_element(&t);
                self.d.append(h, id);
                self.open.push(id);
                self.phase = Phase::Frames;
                None
            }
            TokKind::Start if t.name == "html" || t.name == "head" => {
                self.warn(&t, &format!("discarding unexpected <{}>", t.name));
                None
            }
            TokKind::Start if tags::moves_to_head(&t.name) => {
                self.warn(&t, &format!("<{}> isn't allowed in <html> elements", t.name));
                self.head_element(&t);
                None
            }
            _ => {
                self.warn(&t, "inserting implicit <body>");
                let id = self.inferred("body", t.line, t.col);
                self.d.append(h, id);
                self.open.push(id);
                self.phase = Phase::Body;
                Some(t)
            }
        }
    }

    fn in_frames(&mut self, t: Token) -> Option<Token> {
        let Some(top) = self.top() else {
            self.phase = Phase::AfterFrames;
            return Some(t);
        };
        match t.kind {
            TokKind::Text if is_ws(&t.text) => None,
            TokKind::Comment | TokKind::ProcIns | TokKind::Asp | TokKind::Jste | TokKind::Php => {
                let id = self.leaf(&t)?;
                self.d.append(top, id);
                None
            }
            TokKind::Start if t.name == "frame" => {
                let id = self.build_element(&t);
                self.d.append(top, id);
                None
            }
            TokKind::Start if t.name == "frameset" => {
                let id = self.build_element(&t);
                self.d.append(top, id);
                self.open.push(id);
                None
            }
            TokKind::Start if t.name == "noframes" => {
                let id = self.build_element(&t);
                self.d.append(top, id);
                self.open.push(id);
                self.phase = Phase::Body;
                None
            }
            TokKind::End if t.name == "frameset" => {
                self.open.pop();
                if self.open.is_empty() {
                    self.phase = Phase::AfterFrames;
                }
                None
            }
            TokKind::Eof => {
                self.warn(&t, "missing </frameset>");
                self.open.clear();
                self.phase = Phase::AfterFrames;
                None
            }
            TokKind::Start if self.drop_unknown(&t) => None,
            TokKind::Start => {
                self.warn(&t, &format!("<{}> isn't allowed in <frameset> elements", t.name));
                self.warn(&t, &format!("discarding unexpected <{}>", t.name));
                None
            }
            TokKind::End => {
                self.warn(&t, &format!("discarding unexpected </{}>", t.name));
                None
            }
            TokKind::Text => {
                self.warn(&t, "plain text isn't allowed in <frameset> elements");
                None
            }
            _ => None,
        }
    }

    fn after_frames(&mut self, t: Token) -> Option<Token> {
        let h = self.html_id();
        match t.kind {
            TokKind::Text if is_ws(&t.text) => None,
            TokKind::Comment | TokKind::ProcIns | TokKind::Asp | TokKind::Jste | TokKind::Php => {
                let id = self.leaf(&t)?;
                self.d.append(h, id);
                None
            }
            TokKind::End if t.name == "html" => None,
            TokKind::Eof => None,
            TokKind::Start => {
                self.drop_unknown(&t);
                None
            }
            TokKind::End => {
                self.warn(&t, &format!("discarding unexpected </{}>", t.name));
                None
            }
            _ => None,
        }
    }

    // ----- body content ---------------------------------------------------

    fn in_body(&mut self, t: Token) -> Option<Token> {
        match t.kind {
            TokKind::Eof => {
                self.close_all(&t);
                None
            }
            TokKind::Text => {
                self.text(&t);
                None
            }
            TokKind::Comment
            | TokKind::ProcIns
            | TokKind::Asp
            | TokKind::Jste
            | TokKind::Php
            | TokKind::Section
            | TokKind::Cdata => {
                let id = self.leaf(&t)?;
                let top = self.top().unwrap_or(self.d.root);
                self.d.append(top, id);
                None
            }
            TokKind::Doctype | TokKind::XmlDecl => {
                self.warn(&t, "discarding unexpected <!DOCTYPE>");
                None
            }
            TokKind::Start => {
                self.start(&t);
                None
            }
            TokKind::End => {
                self.end(&t);
                None
            }
        }
    }

    /// Reports content after `</body>`, once; it stays in the body.
    fn after_body_check(&mut self, t: &Token) {
        if self.body_closed && !self.after_body_reported {
            self.after_body_reported = true;
            self.warn(t, "content occurs after end of body");
        }
    }

    fn text(&mut self, t: &Token) {
        if t.text.is_empty() {
            return;
        }
        let Some(mut top) = self.top() else { return };
        let ws = is_ws(&t.text);
        let f = self.flags(top);
        let pre = self.open.iter().any(|&o| self.flags(o) & tags::PRE != 0);
        if ws && !pre {
            // Whitespace in a structural element means nothing.
            if f & tags::STRUCT != 0 && self.d.nodes[top].name != "body"
                || self.d.nodes[top].name == "body" && self.d.nodes[top].children.is_empty()
            {
                return;
            }
        } else {
            self.after_body_check(t);
            self.reopen_for_text(t);
            top = match self.fix_context_for_inline(t) {
                Some(p) => p,
                None => return,
            };
        }
        let text = if pre {
            t.text.clone()
        } else {
            collapse(&t.text)
        };
        // Text next to text (a dropped tag between them) joins up.
        if let Some(&last) = self.d.nodes[top].children.last()
            && self.d.nodes[last].kind == NodeType::Text
        {
            self.d.nodes[last].text.push_str(&text);
            return;
        }
        let id = self.d.new_node(NodeType::Text, "", t.line, t.col);
        self.d.nodes[id].text = text;
        self.d.append(top, id);
    }

    /// Reopens inline elements a block start closed.
    fn reopen_for_text(&mut self, t: &Token) {
        if self.pending.is_empty() {
            return;
        }
        for p in std::mem::take(&mut self.pending) {
            let Some(top) = self.fix_context_for_inline(t) else {
                return;
            };
            let id = self.element(&p.name, p.tag, p.user, t.line, t.col);
            self.d.nodes[id].attrs = p.attrs;
            self.d.nodes[id].implicit = true;
            self.d.append(top, id);
            self.open.push(id);
        }
    }

    /// Makes the top of the stack able to hold phrasing content by inserting
    /// what's missing; `None` when it can't.
    fn fix_context_for_inline(&mut self, t: &Token) -> Option<NodeId> {
        let top = self.top()?;
        let name = self.d.nodes[top].name.clone();
        let f = self.flags(top);
        if f & tags::LIST != 0 {
            return Some(self.infer_child(top, "li", t));
        }
        if f & tags::DEFLIST != 0 {
            return Some(self.infer_child(top, "dd", t));
        }
        match name.as_str() {
            "table" | "tbody" | "thead" | "tfoot" => Some(self.foster(t)),
            "colgroup" => {
                self.open.pop();
                self.fix_context_for_inline(t)
            }
            "tr" => Some(self.infer_child(top, "td", t)),
            "select" | "optgroup" | "datalist" => {
                self.warn(t, &format!("discarding content that isn't allowed in <{name}>"));
                None
            }
            _ => Some(top),
        }
    }

    fn infer_child(&mut self, parent: NodeId, name: &str, t: &Token) -> NodeId {
        self.warn(t, &format!("missing <{name}>"));
        let id = self.inferred(name, t.line, t.col);
        self.d.append(parent, id);
        self.open.push(id);
        id
    }

    /// Content in a table outside any cell goes ahead of the table: returns
    /// a place for it. An implicit `div` holds it, so it closes with the
    /// table's next part.
    fn foster(&mut self, t: &Token) -> NodeId {
        let table = self
            .open
            .iter()
            .rev()
            .copied()
            .find(|&o| self.d.nodes[o].name == "table");
        let Some(table) = table else {
            return self.top().unwrap_or(self.d.root);
        };
        self.warn(t, "content isn't allowed in <table> elements, moving it before the table");
        let id = self.inferred("div", t.line, t.col);
        self.d.insert_before(table, id);
        self.open.push(id);
        id
    }

    fn scope_find(&self, want: impl Fn(&Document, NodeId) -> bool, barrier: &[&str]) -> Option<usize> {
        for (i, &o) in self.open.iter().enumerate().rev() {
            if want(self.d, o) {
                return Some(i);
            }
            if barrier.contains(&self.d.nodes[o].name.as_str()) {
                return None;
            }
        }
        None
    }

    /// Pops down to keep `len` open elements, warning for those whose end
    /// tag can't be left out.
    fn pop_to(&mut self, len: usize, before: &str, t: &Token) {
        while self.open.len() > len {
            let Some(id) = self.open.pop() else { break };
            if self.flags(id) & tags::OPT_END == 0 && !self.d.nodes[id].implicit {
                let m = format!("missing </{}> before {before}", self.d.nodes[id].name);
                self.warn(t, &m);
            }
        }
    }

    fn start(&mut self, t: &Token) {
        if self.drop_unknown(t) {
            return;
        }
        if self.open.len() >= MAX_DEPTH {
            self.warn(t, &format!("discarding <{}>: elements nested too deeply", t.name));
            return;
        }
        let name = t.name.as_str();
        let (tag, user) = self.resolve(name);
        let flags = tag.map_or(
            match user {
                Some(UserKind::Block) => tags::BLOCK,
                Some(UserKind::Empty) => tags::INLINE | tags::EMPTY,
                Some(UserKind::Inline) => tags::INLINE,
                Some(UserKind::Pre) => tags::BLOCK | tags::PRE,
                None => 0,
            },
            |g| g.flags,
        );
        if flags & tags::OBSOLETE != 0 && !self.d.xhtml && self.d.declared_version().is_none() {
            self.warn(t, &format!("<{name}> element removed from HTML5"));
        }
        match name {
            "html" | "head" | "frameset" | "frame" => {
                self.warn(t, &format!("discarding unexpected <{name}>"));
                return;
            }
            "body" => {
                // Inside noframes a body is that element's content.
                if !self.open.iter().any(|&o| self.d.nodes[o].name == "noframes") {
                    self.warn(t, "discarding unexpected <body>");
                    return;
                }
            }
            _ => {}
        }
        if tags::moves_to_head(name) && self.head.is_some() {
            let top = self.top().map_or("body", |o| self.d.nodes[o].name.as_str());
            let m = format!("<{name}> isn't allowed in <{top}> elements");
            self.warn(t, &m);
            self.head_element(t);
            return;
        }
        self.after_body_check(t);
        if !self.place_for(t, name, flags) {
            return;
        }
        let Some(parent) = self.top() else { return };
        let id = self.build_element(t);
        self.d.append(parent, id);
        if self.read_text_content(id, t) {
            return;
        }
        self.open.push(id);
    }

    /// Closes what the new element implicitly ends and inserts what it
    /// needs; `false` when the element can't be placed and was dropped.
    fn place_for(&mut self, t: &Token, name: &str, flags: u32) -> bool {
        let table_barrier = ["table", "body", "noframes"];
        let is_list = |d: &Document, o| d.flags(o) & tags::LIST != 0;
        match name {
            "li" => {
                if let Some(i) = self.scope_find(is_list, &["td", "th", "table", "body"]) {
                    self.close_silently(i + 1, t);
                    return true;
                }
                self.close_inline_and_p(t, name);
                self.warn(t, "missing <ul>");
                let Some(top) = self.top() else { return false };
                let ul = self.inferred("ul", t.line, t.col);
                self.d.append(top, ul);
                self.open.push(ul);
                true
            }
            "dt" | "dd" => {
                let is_dl = |d: &Document, o| d.flags(o) & tags::DEFLIST != 0;
                if let Some(i) = self.scope_find(is_dl, &["td", "th", "table", "body"]) {
                    self.close_silently(i + 1, t);
                    return true;
                }
                self.close_inline_and_p(t, name);
                self.warn(t, "missing <dl>");
                let Some(top) = self.top() else { return false };
                let dl = self.inferred("dl", t.line, t.col);
                self.d.append(top, dl);
                self.open.push(dl);
                true
            }
            "tr" => {
                let want = |d: &Document, o: NodeId| {
                    matches!(d.nodes[o].name.as_str(), "table" | "tbody" | "thead" | "tfoot")
                };
                match self.scope_find(want, &table_barrier) {
                    Some(i) => {
                        self.close_silently(i + 1, t);
                        true
                    }
                    None => self.not_allowed(t, name),
                }
            }
            "td" | "th" => {
                let want = |d: &Document, o: NodeId| {
                    matches!(
                        d.nodes[o].name.as_str(),
                        "tr" | "table" | "tbody" | "thead" | "tfoot"
                    )
                };
                match self.scope_find(want, &table_barrier) {
                    Some(i) => {
                        self.close_silently(i + 1, t);
                        let Some(top) = self.top() else { return false };
                        if self.d.nodes[top].name != "tr" {
                            self.infer_child(top, "tr", t);
                        }
                        true
                    }
                    None => self.not_allowed(t, name),
                }
            }
            "thead" | "tbody" | "tfoot" | "caption" | "colgroup" | "col" => {
                let want = |d: &Document, o: NodeId| {
                    d.nodes[o].name == "table" || (name == "col" && d.nodes[o].name == "colgroup")
                };
                match self.scope_find(want, &table_barrier) {
                    Some(i) => {
                        self.close_silently(i + 1, t);
                        true
                    }
                    None => self.not_allowed(t, name),
                }
            }
            "option" | "optgroup" => {
                let want = |d: &Document, o: NodeId| {
                    matches!(d.nodes[o].name.as_str(), "select" | "datalist")
                        || (name == "option" && d.nodes[o].name == "optgroup")
                };
                match self.scope_find(want, &["body", "td", "th"]) {
                    Some(i) => {
                        self.close_silently(i + 1, t);
                        true
                    }
                    None => self.not_allowed(t, name),
                }
            }
            _ if flags & tags::INLINE != 0 => {
                if name == "a" {
                    let is_a = |d: &Document, o: NodeId| d.nodes[o].name == "a";
                    if let Some(i) = self.scope_find(is_a, &["td", "th", "table", "body", "div", "p"]) {
                        self.pop_to(i, "<a>", t);
                    }
                }
                self.reopen_for_text(t);
                self.fix_context_for_inline(t).is_some()
            }
            _ => {
                self.close_inline_and_p(t, name);
                self.fix_context_for_inline(t).is_some()
            }
        }
    }

    fn not_allowed(&mut self, t: &Token, name: &str) -> bool {
        let top = self.top().map_or("body".to_string(), |o| self.d.nodes[o].name.clone());
        self.warn(t, &format!("<{name}> isn't allowed in <{top}> elements"));
        self.warn(t, &format!("discarding unexpected <{name}>"));
        false
    }

    fn close_silently(&mut self, len: usize, t: &Token) {
        let before = format!("<{}>", t.name);
        self.pop_to(len, &before, t);
        self.pending.clear();
    }

    /// A block start tag ends the inline elements open around it (they
    /// reopen inside when text comes) and an open paragraph or heading.
    fn close_inline_and_p(&mut self, t: &Token, name: &str) {
        loop {
            let Some(top) = self.top() else { return };
            let f = self.flags(top);
            if f & tags::INLINE != 0 && f & (tags::MIXED | tags::STRUCT) == 0 {
                let n = &self.d.nodes[top];
                let p = Pending {
                    name: n.name.clone(),
                    attrs: n.attrs.clone(),
                    tag: n.tag,
                    user: n.user,
                };
                if !n.implicit {
                    let m = format!("missing </{}> before <{name}>", n.name);
                    self.warn(t, &m);
                }
                self.pending.insert(0, p);
                self.open.pop();
                continue;
            }
            if f & tags::PHRASING_ONLY != 0 {
                if f & tags::HEADING != 0 {
                    let m = format!("missing </{}> before <{name}>", self.d.nodes[top].name);
                    self.warn(t, &m);
                }
                self.open.pop();
                continue;
            }
            return;
        }
    }

    fn end(&mut self, t: &Token) {
        let name = t.name.as_str();
        if name == "body" || name == "html" {
            let keep = self
                .open
                .iter()
                .position(|&o| self.d.nodes[o].name == "body")
                .map_or(1, |i| i + 1);
            let before = format!("</{name}>");
            self.pop_to(keep, &before, t);
            self.pending.clear();
            self.body_closed = true;
            return;
        }
        let (tag, user) = self.resolve(name);
        let inline = tag.map_or(user == Some(UserKind::Inline), |g| {
            g.flags & tags::INLINE != 0 && g.flags & tags::MIXED == 0
        });
        let found = self.find_open_for_end(name, inline);
        match found {
            Some(i) => {
                // Reopened inline elements whose block ends go back on the
                // list: they apply to what comes after the block too.
                let mut reopen = Vec::new();
                while self.open.len() > i + 1 {
                    let Some(id) = self.open.pop() else { break };
                    let n = &self.d.nodes[id];
                    if self.flags(id) & tags::OPT_END == 0 {
                        if n.implicit && self.flags(id) & tags::INLINE != 0 {
                            reopen.insert(
                                0,
                                Pending {
                                    name: n.name.clone(),
                                    attrs: n.attrs.clone(),
                                    tag: n.tag,
                                    user: n.user,
                                },
                            );
                        } else if !n.implicit {
                            let m = format!("missing </{}> before </{name}>", n.name);
                            self.warn(t, &m);
                        }
                    }
                }
                self.open.pop();
                self.pending.splice(0..0, reopen);
                self.frames_phase_after_pop();
            }
            None => {
                if let Some(i) = self.pending.iter().position(|p| p.name == name) {
                    self.pending.remove(i);
                } else {
                    self.warn(t, &format!("discarding unexpected </{name}>"));
                }
            }
        }
    }

    /// After `</noframes>`, back to the frameset's own content.
    fn frames_phase_after_pop(&mut self) {
        if let Some(top) = self.top()
            && self.d.nodes[top].name == "frameset"
        {
            self.phase = Phase::Frames;
        }
    }

    /// The open element an end tag closes. An inline end tag can't reach
    /// across a block, and no end tag across a table cell or table that it
    /// doesn't name.
    fn find_open_for_end(&self, name: &str, inline: bool) -> Option<usize> {
        for (i, &o) in self.open.iter().enumerate().rev() {
            let n = &self.d.nodes[o];
            if n.name == name {
                return Some(i);
            }
            let f = self.flags(o);
            if inline && (f & tags::INLINE == 0 || f & tags::MIXED != 0) {
                return None;
            }
            if matches!(n.name.as_str(), "td" | "th" | "table" | "caption" | "body") {
                let table_part = tags::lookup(name).is_some_and(|g| {
                    g.flags & tags::TABLE_PART != 0 || g.name == "table"
                });
                if !table_part || n.name == "table" && name != "table" {
                    return None;
                }
            }
        }
        None
    }

    /// The end of the input closes everything.
    fn close_all(&mut self, t: &Token) {
        while let Some(id) = self.open.pop() {
            let n = &self.d.nodes[id];
            if self.flags(id) & tags::OPT_END == 0 && !n.implicit {
                let m = format!("missing </{}>", n.name);
                self.warn(t, &m);
            }
        }
    }

    /// What follows the input: the title, and the whitespace rules.
    fn finish_html(&mut self) {
        let (line, col) = self.last_pos;
        if let Some(head) = self.d.head() {
            let has_title = self.d.nodes[head]
                .children
                .iter()
                .any(|&c| self.d.is_element(c, "title"));
            if !has_title {
                let (hl, hc) = (self.d.nodes[head].line, self.d.nodes[head].col);
                self.d
                    .report(Level::Warning, hl, hc, "inserting missing 'title' element");
                let id = self.inferred("title", line, col);
                self.d.append(head, id);
            }
        }
        if let Some(h) = self.html {
            normalize_block(self.d, h);
        }
    }

    // ----- XML ------------------------------------------------------------

    /// `input-xml`: a generic tree, without the HTML content model.
    fn parse_xml(&mut self) {
        let root = self.d.root;
        let mut stack: Vec<NodeId> = Vec::new();
        loop {
            let t = self.next();
            let top = stack.last().copied().unwrap_or(root);
            match t.kind {
                TokKind::Eof => {
                    for &o in stack.iter().rev() {
                        let m = format!("missing </{}>", self.d.nodes[o].name);
                        self.warn(&t, &m);
                    }
                    break;
                }
                TokKind::Text => {
                    if !is_ws(&t.text) {
                        if stack.is_empty() {
                            self.warn(&t, "plain text isn't allowed outside the document element");
                        }
                        let id = self.d.new_node(NodeType::Text, "", t.line, t.col);
                        self.d.nodes[id].text = t.text.clone();
                        self.d.append(top, id);
                    }
                }
                TokKind::Doctype => self.add_doctype(&t),
                TokKind::Start => {
                    if stack.len() >= MAX_DEPTH {
                        self.warn(&t, "discarding element: nested too deeply");
                        continue;
                    }
                    let id = self.d.new_node(NodeType::Start, &t.name, t.line, t.col);
                    self.set_attrs(id, &t);
                    self.d.append(top, id);
                    if t.self_closed {
                        self.d.nodes[id].kind = NodeType::StartEnd;
                    } else {
                        stack.push(id);
                    }
                }
                TokKind::End => match stack.iter().rposition(|&o| self.d.nodes[o].name == t.name) {
                    Some(i) => {
                        for &o in stack[i + 1..].iter().rev() {
                            let m = format!("missing </{}>", self.d.nodes[o].name);
                            self.d.report(Level::Warning, t.line, t.col, &m);
                        }
                        stack.truncate(i);
                    }
                    None => self.warn(&t, &format!("discarding unexpected </{}>", t.name)),
                },
                _ => {
                    if let Some(id) = self.leaf(&t) {
                        self.d.append(top, id);
                    }
                }
            }
        }
    }
}

// ----- whitespace ---------------------------------------------------------

/// Applies HTML's whitespace rules to a block's phrasing content: spaces at
/// the ends of a run disappear, spaces between words are single, and a space
/// at the edge of an inline element moves outside it.
fn normalize_block(d: &mut Document, id: NodeId) {
    if d.flags(id) & (tags::PRE | tags::RAW | tags::RCDATA) != 0 {
        return;
    }
    hoist_spaces(d, id);
    let mut run = Run {
        space_before: true,
        last_text: None,
    };
    walk_run(d, id, &mut run);
    trim_run_end(d, &mut run);
    remove_empty_text(d, id);
}

struct Run {
    space_before: bool,
    last_text: Option<NodeId>,
}

fn trim_run_end(d: &mut Document, run: &mut Run) {
    if let Some(l) = run.last_text.take() {
        let t = &mut d.nodes[l].text;
        if t.ends_with(' ') {
            t.pop();
        }
    }
}

fn is_inline_element(d: &Document, id: NodeId) -> bool {
    let n = &d.nodes[id];
    matches!(n.kind, NodeType::Start | NodeType::StartEnd)
        && d.flags(id) & tags::INLINE != 0
        && d.flags(id) & tags::STRUCT == 0
}

fn walk_run(d: &mut Document, id: NodeId, run: &mut Run) {
    let children = d.nodes[id].children.clone();
    for c in children {
        match d.nodes[c].kind {
            NodeType::Text => {
                let t = &mut d.nodes[c].text;
                if run.space_before && t.starts_with(' ') {
                    t.remove(0);
                }
                if !t.is_empty() {
                    run.space_before = t.ends_with(' ');
                    run.last_text = Some(c);
                }
            }
            NodeType::Start | NodeType::StartEnd => {
                let f = d.flags(c);
                if is_inline_element(d, c) {
                    if d.is_element(c, "br") {
                        trim_run_end(d, run);
                        run.space_before = true;
                    } else if f & tags::EMPTY != 0 {
                        run.space_before = false;
                        run.last_text = None;
                    } else if f & (tags::RAW | tags::RCDATA | tags::PRE) != 0 {
                        run.space_before = false;
                        run.last_text = None;
                    } else if f & tags::MIXED != 0
                        && d.nodes[c]
                            .children
                            .iter()
                            .any(|&k| matches!(d.nodes[k].kind, NodeType::Start) && !is_inline_element(d, k))
                    {
                        trim_run_end(d, run);
                        normalize_block(d, c);
                        run.space_before = true;
                    } else {
                        walk_run(d, c, run);
                    }
                } else {
                    trim_run_end(d, run);
                    normalize_block(d, c);
                    run.space_before = true;
                }
            }
            NodeType::Cdata | NodeType::Php | NodeType::Asp | NodeType::Jste => {
                run.space_before = false;
                run.last_text = None;
            }
            _ => {}
        }
    }
}

/// `<b>a </b>c` becomes `<b>a</b> c`, and `<b> a</b>` becomes ` <b>a</b>`.
fn hoist_spaces(d: &mut Document, id: NodeId) {
    let children = d.nodes[id].children.clone();
    for c in children {
        if !matches!(d.nodes[c].kind, NodeType::Start) {
            continue;
        }
        hoist_spaces(d, c);
        if !is_inline_element(d, c) || d.flags(c) & (tags::PRE | tags::RAW | tags::RCDATA) != 0 {
            continue;
        }
        if let Some(&first) = d.nodes[c].children.first()
            && d.nodes[first].kind == NodeType::Text
            && d.nodes[first].text.starts_with(' ')
        {
            d.nodes[first].text.remove(0);
            add_space(d, c, false);
        }
        if let Some(&last) = d.nodes[c].children.last()
            && d.nodes[last].kind == NodeType::Text
            && d.nodes[last].text.ends_with(' ')
        {
            d.nodes[last].text.pop();
            add_space(d, c, true);
        }
    }
}

/// A space beside an element, merging with a text sibling there.
fn add_space(d: &mut Document, id: NodeId, after: bool) {
    let sib = if after {
        d.next_sibling(id)
    } else {
        d.prev_sibling(id)
    };
    if let Some(s) = sib
        && d.nodes[s].kind == NodeType::Text
    {
        let t = &mut d.nodes[s].text;
        if after {
            if !t.starts_with(' ') {
                t.insert(0, ' ');
            }
        } else if !t.ends_with(' ') {
            t.push(' ');
        }
        return;
    }
    let (line, col) = (d.nodes[id].line, d.nodes[id].col);
    let sp = d.new_node(NodeType::Text, "", line, col);
    d.nodes[sp].text.push(' ');
    let Some(p) = d.nodes[id].parent else { return };
    let i = d.nodes[p].children.iter().position(|&c| c == id).unwrap_or(0);
    d.nodes[sp].parent = Some(p);
    d.nodes[p].children.insert(if after { i + 1 } else { i }, sp);
}

fn remove_empty_text(d: &mut Document, id: NodeId) {
    if d.flags(id) & (tags::PRE | tags::RAW | tags::RCDATA) != 0 {
        return;
    }
    let children = d.nodes[id].children.clone();
    for c in children {
        if d.nodes[c].kind == NodeType::Text {
            if d.nodes[c].text.is_empty() {
                d.detach(c);
            }
        } else {
            remove_empty_text(d, c);
        }
    }
}
