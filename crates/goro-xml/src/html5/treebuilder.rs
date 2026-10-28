//! HTML tree construction (WHATWG HTML "13.2.6 Tree construction").
//!
//! Builds nodes directly in a [`tree::Arena`](crate::tree::Arena). The
//! algorithm follows the HTML Standard as PHP 8.5's parser implements it:
//! scripting disabled, no processing instructions, and the `<select>`
//! parsing rules from before the 2025 "customizable select" change (observed
//! on PHP 8.5: `<select><div>x</div></select>` drops the `div`).
//!
//! Tree errors carry the names PHP reports (observed on PHP 8.5, e.g.
//! `unexpected-token-in-initial-mode`, `unexpected-closed-token`,
//! `characters-in-table-text`); like PHP, many parse errors of the standard
//! are not reported.

use std::collections::HashMap;

mod modes;

use super::tags::{T, foreign_attr_fix, svg_attr_fix, svg_tag_fix};
use super::tokenizer::{Attr, State, Tag, Token, Tokenizer};
use crate::tree::{
    Arena, AttrData, ElementData, HTML_NS, MATHML_NS, NodeData, NodeId, QName, QuirksMode, SVG_NS,
    Str,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    InHeadNoscript,
    AfterHead,
    InBody,
    Text,
    InTable,
    InTableText,
    InCaption,
    InColumnGroup,
    InTableBody,
    InRow,
    InCell,
    InSelect,
    InSelectInTable,
    InTemplate,
    AfterBody,
    InFrameset,
    AfterFrameset,
    AfterAfterBody,
    AfterAfterFrameset,
}

/// Element namespaces as the parser sees them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Ns {
    Html,
    Svg,
    Math,
}

/// An entry of the stack of open elements.
#[derive(Clone, Debug)]
struct Open {
    node: NodeId,
    ns: Ns,
    t: T,
    name: Str,
    /// MathML `annotation-xml` with an HTML encoding (HTML integration point).
    html_ip: bool,
}

#[derive(Clone, Debug)]
enum Afe {
    Marker,
    El {
        node: NodeId,
        t: T,
        name: Str,
        attrs: Vec<Attr>,
        line: u32,
    },
}

/// A tree construction error: code and the source span of the token
/// (`first..end` in bytes).
#[derive(Clone, Copy, Debug)]
pub(crate) struct TreeError {
    pub code: &'static str,
    pub first: usize,
    pub end: usize,
}

/// Options for the tree builder.
#[derive(Clone, Debug, Default)]
pub(crate) struct BuildOptions {
    /// Put HTML elements in no namespace (`Dom\HTML_NO_DEFAULT_NS`).
    pub no_default_ns: bool,
    /// Record errors.
    pub collect_errors: bool,
    /// Scripting flag (PHP parses with scripting disabled).
    pub scripting: bool,
    /// LIBXML_HTML_NOIMPLIED: initial-mode errors are not reported.
    pub no_implied: bool,
    /// Follow the WHATWG standard where PHP 8.5 differs.
    pub standard: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Default,
    ListItem,
    Button,
    Table,
    Select,
}

/// A fragment parsing context.
pub(crate) struct Context {
    pub node: NodeId,
    pub ns: Ns,
    pub t: T,
    pub name: Str,
    pub html_ip: bool,
}

pub(crate) struct TreeBuilder<'a> {
    pub arena: &'a mut Arena,
    /// Container of the tree (the document, or the scratch document of a
    /// fragment parse).
    pub doc: NodeId,
    /// Owner document of the created nodes.
    owner: NodeId,
    pub tok: Tokenizer,
    mode: Mode,
    orig_mode: Mode,
    template_modes: Vec<Mode>,
    stack: Vec<Open>,
    afe: Vec<Afe>,
    head: Option<NodeId>,
    form: Option<NodeId>,
    frameset_ok: bool,
    foster: bool,
    pending_table_text: String,
    pending_table_text_nonws: bool,
    skip_lf: bool,
    ctx: Option<Context>,
    /// The context element as a stack entry (adjusted current node).
    ctx_open: Option<Open>,
    opts: BuildOptions,
    pub errors: Vec<TreeError>,
    span: (usize, usize),
    /// The token being processed came from the end of the input: PHP
    /// reports no tree errors for it.
    suppress: bool,
    names: HashMap<Box<str>, Str, std::hash::BuildHasherDefault<FxHasher>>,
    ns_html: Option<Str>,
    ns_svg: Str,
    ns_math: Str,
    pub quirks: QuirksMode,
    /// html/head/body created from a start tag (LIBXML_HTML_NOIMPLIED).
    pub explicit: Vec<NodeId>,
    /// Line tracking: byte offset and 0-based line already counted.
    line_off: usize,
    line_no: u32,
}

/// Whitespace for the tree builder. PHP does not count U+000C FORM FEED
/// (observed on PHP 8.5: a lone form feed ends up in `body`).
/// A small fast hasher for the name interner (keys are short tag and
/// attribute names).
#[derive(Default)]
pub(crate) struct FxHasher(u64);

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
        }
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

#[inline]
fn is_ws_byte(c: u8) -> bool {
    matches!(c, b'\t' | b'\n' | b'\r' | b' ')
}

fn ws_prefix_len(s: &str) -> usize {
    s.bytes().take_while(|&c| is_ws_byte(c)).count()
}

fn all_ws(s: &str) -> bool {
    s.bytes().all(is_ws_byte)
}

impl<'a> TreeBuilder<'a> {
    pub fn new(
        arena: &'a mut Arena,
        doc: NodeId,
        owner: NodeId,
        tok: Tokenizer,
        opts: BuildOptions,
    ) -> TreeBuilder<'a> {
        let ns_html = if opts.no_default_ns {
            None
        } else {
            Some(Str::from(HTML_NS))
        };
        TreeBuilder {
            arena,
            doc,
            owner,
            tok,
            mode: Mode::Initial,
            orig_mode: Mode::Initial,
            template_modes: Vec::new(),
            stack: Vec::new(),
            afe: Vec::new(),
            head: None,
            form: None,
            frameset_ok: true,
            foster: false,
            pending_table_text: String::new(),
            pending_table_text_nonws: false,
            skip_lf: false,
            ctx: None,
            ctx_open: None,
            opts,
            errors: Vec::new(),
            span: (0, 0),
            suppress: false,
            names: HashMap::default(),
            ns_html,
            ns_svg: Str::from(SVG_NS),
            ns_math: Str::from(MATHML_NS),
            quirks: QuirksMode::NoQuirks,
            explicit: Vec::new(),
            line_off: 0,
            line_no: 0,
        }
    }

    // ----- driver ------------------------------------------------------------

    /// Runs the parser to the end of the input.
    pub fn run(&mut self) {
        loop {
            self.tok.allow_cdata = self.adjusted_current().is_some_and(|o| o.ns != Ns::Html);
            let t = self.tok.next_token();
            let eof = matches!(t, Token::Eof { .. });
            self.dispatch(t);
            if eof {
                break;
            }
        }
    }

    /// Sets up fragment parsing with a context element ("13.4 Parsing HTML
    /// fragments"). The caller has created `self.doc` and runs [`run`] after.
    pub fn init_fragment(&mut self, ctx: Context, form: Option<NodeId>, quirks: QuirksMode) {
        self.quirks = quirks;
        if ctx.ns == Ns::Html {
            self.tok.state = match ctx.t {
                T::Title | T::Textarea => State::RcData,
                T::Style | T::Xmp | T::Iframe | T::Noembed | T::Noframes => State::RawText,
                T::Noscript if self.opts.scripting => State::RawText,
                T::Script => State::ScriptData,
                T::Plaintext => State::PlainText,
                _ => State::Data,
            };
        }
        let html = self.new_element_node(
            &Tag {
                name: "html".into(),
                ..Tag::default()
            },
            Ns::Html,
            "html",
        );
        self.arena.raw_append(self.doc, html);
        let name = self.intern("html");
        self.stack.push(Open {
            node: html,
            ns: Ns::Html,
            t: T::Html,
            name,
            html_ip: false,
        });
        if ctx.ns == Ns::Html && ctx.t == T::Template {
            self.template_modes.push(Mode::InTemplate);
        }
        self.ctx_open = Some(Open {
            node: ctx.node,
            ns: ctx.ns,
            t: ctx.t,
            name: ctx.name.clone(),
            html_ip: ctx.html_ip,
        });
        self.ctx = Some(ctx);
        self.reset_mode();
        self.form = form;
    }

    /// The root `html` element of a fragment parse.
    pub fn fragment_root(&self) -> Option<NodeId> {
        self.stack.first().map(|o| o.node)
    }

    fn dispatch(&mut self, t: Token) {
        self.suppress = self.tok.last_at_eof;
        match t {
            Token::Text { text, first, end } => {
                self.span = (first, end);
                let mut s: &str = &text;
                if self.skip_lf {
                    self.skip_lf = false;
                    if let Some(r) = s.strip_prefix('\n') {
                        s = r;
                    }
                    if s.is_empty() {
                        return;
                    }
                }
                if self.mode == Mode::InBody
                    && s.len() == text.len()
                    && !text.contains('\0')
                    && self.use_html_rules_for_text()
                {
                    // Common case: the whole token goes into the tree.
                    self.reconstruct_afe();
                    if !all_ws(&text) {
                        self.frameset_ok = false;
                    }
                    self.insert_text_cow(std::borrow::Cow::Owned(text), first);
                } else if self.use_html_rules_for_text() {
                    self.text_in_mode(s, first);
                } else {
                    self.foreign_text(s, first);
                }
            }
            Token::Tag(tag) => {
                self.skip_lf = false;
                self.span = (tag.name_first, tag.name_end);
                if tag.end {
                    if self.use_html_rules_for_end() {
                        self.end_tag(tag);
                    } else {
                        self.foreign_end(tag);
                    }
                } else if self.use_html_rules_for_start(&tag) {
                    self.start_tag(tag);
                } else {
                    self.foreign_start(tag);
                }
            }
            Token::Comment { text, start, data } => {
                self.skip_lf = false;
                // PHP's span for comments ends one byte early (observed on
                // PHP 8.5: `<!--xyz-->` at column 28 is reported as 28-29).
                self.span = (data, data + text.len().saturating_sub(1));
                if self.use_html_rules_for_text() || self.stack.is_empty() {
                    self.comment_in_mode(text, start);
                } else {
                    let loc = self.insertion_place(None);
                    self.insert_comment_at(text, start, loc);
                }
            }
            Token::Doctype(d) => {
                self.skip_lf = false;
                self.span = (d.kw_first, d.kw_end);
                if self.adjusted_current().is_none_or(|o| o.ns == Ns::Html) {
                    self.doctype_in_mode(d);
                } else {
                    self.err("doctype-token-foreign-content-mode");
                }
            }
            Token::Eof { pos } => {
                self.span = (pos, pos);
                self.eof_in_mode();
            }
        }
    }

    // ----- dispatcher conditions (13.2.6 tree construction dispatcher) ---------

    fn adjusted_current(&self) -> Option<&Open> {
        if self.stack.len() == 1
            && let Some(c) = &self.ctx_open
        {
            // The context element stands in for the html root.
            return Some(c);
        }
        self.stack.last()
    }

    fn is_mathml_text_ip(o: &Open) -> bool {
        o.ns == Ns::Math && matches!(&*o.name, "mi" | "mo" | "mn" | "ms" | "mtext")
    }

    fn is_html_ip(o: &Open) -> bool {
        match o.ns {
            Ns::Math => o.html_ip,
            Ns::Svg => matches!(&*o.name, "foreignObject" | "desc" | "title"),
            Ns::Html => false,
        }
    }

    fn use_html_rules_for_text(&self) -> bool {
        match self.adjusted_current() {
            None => true,
            Some(o) => o.ns == Ns::Html || Self::is_mathml_text_ip(o) || Self::is_html_ip(o),
        }
    }

    fn use_html_rules_for_end(&self) -> bool {
        match self.adjusted_current() {
            None => true,
            Some(o) => o.ns == Ns::Html,
        }
    }

    fn use_html_rules_for_start(&self, tag: &Tag) -> bool {
        match self.adjusted_current() {
            None => true,
            Some(o) => {
                o.ns == Ns::Html
                    || (Self::is_mathml_text_ip(o)
                        && tag.name != "mglyph"
                        && tag.name != "malignmark")
                    || (o.ns == Ns::Math && &*o.name == "annotation-xml" && tag.name == "svg")
                    || Self::is_html_ip(o)
            }
        }
    }

    /// The tag of an HTML element name (`search` is an ordinary element
    /// for PHP, observed on PHP 8.5).
    pub(crate) fn tag_t(&self, name: &str) -> T {
        match T::from_name(name) {
            T::Search if !self.opts.standard => T::Other,
            t => t,
        }
    }

    // ----- errors and lines ------------------------------------------------------

    #[inline]
    fn err(&mut self, code: &'static str) {
        if self.opts.collect_errors && !self.suppress {
            self.errors.push(TreeError {
                code,
                first: self.span.0,
                end: self.span.1,
            });
        }
    }

    /// 0-based line of the byte offset (offsets mostly increase).
    fn line_at(&mut self, off: usize) -> u32 {
        let input = self.tok.input();
        let off = off.min(input.len());
        if off >= self.line_off {
            self.line_no += memchr::memchr_iter(b'\n', &input[self.line_off..off]).count() as u32;
        } else {
            self.line_no = memchr::memchr_iter(b'\n', &input[..off]).count() as u32;
        }
        self.line_off = off;
        self.line_no
    }

    // ----- node creation ---------------------------------------------------------

    fn intern(&mut self, s: &str) -> Str {
        if let Some(v) = self.names.get(s) {
            return v.clone();
        }
        let v: Str = Str::from(s);
        self.names.insert(s.into(), v.clone());
        v
    }

    fn ns_str(&self, ns: Ns) -> Option<Str> {
        match ns {
            Ns::Html => self.ns_html.clone(),
            Ns::Svg => Some(self.ns_svg.clone()),
            Ns::Math => Some(self.ns_math.clone()),
        }
    }

    /// Creates an element node with the tag's attributes (adjusting
    /// foreign attributes for SVG/MathML elements).
    fn new_element_node(&mut self, tag: &Tag, ns: Ns, local: &str) -> NodeId {
        let name = QName {
            prefix: None,
            local: self.intern(local),
            ns: self.ns_str(ns),
        };
        let owner = self.owner;
        let el = self.arena.create(
            owner,
            NodeData::Element(Box::new(ElementData {
                name,
                first_attr: None,
                last_attr: None,
                ns_decls: Vec::new(),
                template_contents: None,
            })),
        );
        let line = if tag.start == 0 && tag.name_first == 0 && tag.attrs.is_empty() {
            // Implied element: no source position.
            0
        } else {
            self.line_at(tag.start)
        };
        self.arena.node_mut(el).line = line;
        for a in &tag.attrs {
            let q = if ns == Ns::Html {
                QName {
                    prefix: None,
                    local: self.intern(&a.name),
                    ns: None,
                }
            } else {
                let mut name: &str = &a.name;
                if ns == Ns::Svg {
                    if let Some(f) = svg_attr_fix(name) {
                        name = f;
                    }
                } else if name == "definitionurl" {
                    name = "definitionURL";
                }
                match foreign_attr_fix(name) {
                    // PHP turns xml:lang and xml:space into plain `lang` and
                    // `space` attributes (observed on PHP 8.5).
                    Some((_, l, crate::tree::XML_NS)) if !self.opts.standard => QName {
                        prefix: None,
                        local: self.intern(l),
                        ns: None,
                    },
                    Some((p, l, n)) => QName {
                        prefix: p.map(|p| self.intern(p)),
                        local: self.intern(l),
                        ns: Some(self.intern(n)),
                    },
                    None => QName {
                        prefix: None,
                        local: self.intern(name),
                        ns: None,
                    },
                }
            };
            let at = self.arena.create(
                owner,
                NodeData::Attribute(Box::new(AttrData {
                    name: q,
                    is_id: false,
                    defaulted: false,
                })),
            );
            self.arena.node_mut(at).line = line;
            if !a.value.is_empty() {
                let t = self.arena.create(owner, NodeData::Text(a.value.clone()));
                self.arena.node_mut(t).line = line;
                self.arena.raw_append(at, t);
            }
            self.arena.raw_append_attr(el, at);
        }
        if ns == Ns::Html && local == "template" && self.ns_html.is_some() {
            let frag = self.arena.create(owner, NodeData::Fragment);
            if let Some(e) = self.arena.element_mut(el) {
                e.template_contents = Some(frag);
            }
        }
        el
    }

    /// The appropriate place for inserting a node: (parent, before).
    fn insertion_place(&self, target: Option<NodeId>) -> (NodeId, Option<NodeId>) {
        let target_idx = match target {
            Some(t) => self.stack.iter().rposition(|o| o.node == t),
            None => self.stack.len().checked_sub(1),
        };
        let (mut parent, mut before) = match target_idx {
            Some(i) => (self.stack[i].node, None),
            None => (target.unwrap_or(self.doc), None),
        };
        let parent_entry = target_idx.map(|i| &self.stack[i]);
        if self.foster
            && let Some(pe) = parent_entry
            && pe.ns == Ns::Html
            && matches!(pe.t, T::Table | T::Tbody | T::Tfoot | T::Thead | T::Tr)
        {
            let last = self
                .stack
                .iter()
                .rposition(|o| o.ns == Ns::Html && matches!(o.t, T::Table | T::Template));
            match last {
                None => {
                    return (self.stack[0].node, None);
                }
                Some(i) if self.stack[i].t == T::Template => {
                    parent = self.stack[i].node;
                    before = None;
                }
                Some(i) => {
                    let table = self.stack[i].node;
                    match self.arena.parent(table) {
                        Some(p) => {
                            parent = p;
                            before = Some(table);
                        }
                        None => {
                            parent = self.stack[i - 1].node;
                            before = None;
                        }
                    }
                }
            }
        }
        if let Some(e) = self.arena.element(parent)
            && let Some(tc) = e.template_contents
        {
            return (tc, None);
        }
        (parent, before)
    }

    fn insert_at(&mut self, (parent, before): (NodeId, Option<NodeId>), node: NodeId) {
        self.arena.raw_insert_before(parent, node, before);
    }

    /// Inserts an element for the token at the appropriate place and pushes
    /// it onto the stack.
    fn insert_element(&mut self, tag: &Tag, ns: Ns) -> NodeId {
        let local: String = if ns == Ns::Svg {
            svg_tag_fix(&tag.name).unwrap_or(&tag.name).to_string()
        } else {
            tag.name.clone()
        };
        let loc = self.insertion_place(None);
        let el = self.new_element_node(tag, ns, &local);
        self.insert_at(loc, el);
        let html_ip = ns == Ns::Math
            && local == "annotation-xml"
            && tag.attr("encoding").is_some_and(|e| {
                e.eq_ignore_ascii_case("text/html")
                    || e.eq_ignore_ascii_case("application/xhtml+xml")
            });
        let name = self.intern(&local);
        self.stack.push(Open {
            node: el,
            ns,
            t: if ns == Ns::Html {
                self.tag_t(&local)
            } else {
                T::Other
            },
            name,
            html_ip,
        });
        el
    }

    fn insert_html(&mut self, tag: &Tag) -> NodeId {
        self.insert_element(tag, Ns::Html)
    }

    /// Inserts an implied HTML element with no attributes.
    fn insert_implied(&mut self, name: &str) -> NodeId {
        let tag = Tag {
            name: name.into(),
            ..Tag::default()
        };
        self.insert_html(&tag)
    }

    fn insert_text(&mut self, s: &str, first: usize) {
        self.insert_text_cow(std::borrow::Cow::Borrowed(s), first);
    }

    /// Inserts text, moving `s` into a new text node when there is one.
    fn insert_text_cow(&mut self, s: std::borrow::Cow<'_, str>, first: usize) {
        if s.is_empty() {
            return;
        }
        let (parent, before) = self.insertion_place(None);
        if matches!(self.arena.data(parent), NodeData::Document(_)) {
            return;
        }
        let prev = match before {
            Some(b) => self.arena.prev_sibling(b),
            None => self.arena.last_child(parent),
        };
        if let Some(p) = prev
            && let NodeData::Text(t) = &mut self.arena.node_mut(p).data
        {
            t.push_str(&s);
            return;
        }
        let line = self.line_at(first);
        let owner = self.owner;
        let t = self.arena.create(owner, NodeData::Text(s.into_owned()));
        self.arena.node_mut(t).line = line;
        self.arena.raw_insert_before(parent, t, before);
    }

    fn insert_comment_at(&mut self, text: String, start: usize, loc: (NodeId, Option<NodeId>)) {
        let line = self.line_at(start);
        let owner = self.owner;
        let c = self.arena.create(owner, NodeData::Comment(text));
        self.arena.node_mut(c).line = line;
        self.insert_at(loc, c);
    }

    fn insert_comment(&mut self, text: String, start: usize) {
        let loc = self.insertion_place(None);
        self.insert_comment_at(text, start, loc);
    }

    // ----- stack helpers -----------------------------------------------------------

    fn current(&self) -> &Open {
        self.stack.last().expect("open element")
    }

    fn cur_is(&self, t: T) -> bool {
        self.stack
            .last()
            .is_some_and(|o| o.ns == Ns::Html && o.t == t)
    }

    fn pop(&mut self) -> Option<Open> {
        self.stack.pop()
    }

    fn pop_until(&mut self, t: T) {
        while let Some(o) = self.stack.pop() {
            if o.ns == Ns::Html && o.t == t {
                break;
            }
        }
    }

    fn pop_until_node(&mut self, n: NodeId) {
        while let Some(o) = self.stack.pop() {
            if o.node == n {
                break;
            }
        }
    }

    fn pop_until_heading(&mut self) {
        while let Some(o) = self.stack.pop() {
            if o.ns == Ns::Html && o.t.is_heading() {
                break;
            }
        }
    }

    fn has_in_stack(&self, t: T) -> bool {
        self.stack.iter().any(|o| o.ns == Ns::Html && o.t == t)
    }

    fn is_barrier(o: &Open, scope: Scope) -> bool {
        match scope {
            Scope::Select => !(o.ns == Ns::Html && matches!(o.t, T::Optgroup | T::Option)),
            Scope::Table => o.ns == Ns::Html && matches!(o.t, T::Html | T::Table | T::Template),
            _ => {
                let base = match o.ns {
                    Ns::Html => matches!(
                        o.t,
                        T::Applet
                            | T::Caption
                            | T::Html
                            | T::Table
                            | T::Td
                            | T::Th
                            | T::Marquee
                            | T::Object
                            | T::Template
                    ),
                    Ns::Math => matches!(
                        &*o.name,
                        "mi" | "mo" | "mn" | "ms" | "mtext" | "annotation-xml"
                    ),
                    Ns::Svg => matches!(&*o.name, "foreignObject" | "desc" | "title"),
                };
                base || (o.ns == Ns::Html
                    && match scope {
                        Scope::ListItem => matches!(o.t, T::Ol | T::Ul),
                        Scope::Button => o.t == T::Button,
                        _ => false,
                    })
            }
        }
    }

    fn in_scope_by(&self, scope: Scope, f: impl Fn(&Open) -> bool) -> bool {
        for o in self.stack.iter().rev() {
            if f(o) {
                return true;
            }
            if Self::is_barrier(o, scope) {
                return false;
            }
        }
        false
    }

    fn in_scope(&self, t: T, scope: Scope) -> bool {
        self.in_scope_by(scope, |o| o.ns == Ns::Html && o.t == t)
    }

    fn node_in_scope(&self, n: NodeId) -> bool {
        self.in_scope_by(Scope::Default, |o| o.node == n)
    }

    fn generate_implied_end_tags(&mut self, except: Option<T>) {
        while let Some(o) = self.stack.last() {
            if o.ns == Ns::Html
                && matches!(
                    o.t,
                    T::Dd
                        | T::Dt
                        | T::Li
                        | T::Optgroup
                        | T::Option
                        | T::P
                        | T::Rb
                        | T::Rp
                        | T::Rt
                        | T::Rtc
                )
                && Some(o.t) != except
            {
                self.stack.pop();
            } else {
                break;
            }
        }
    }

    fn generate_all_implied_end_tags(&mut self) {
        while let Some(o) = self.stack.last() {
            if o.ns == Ns::Html
                && matches!(
                    o.t,
                    T::Caption
                        | T::Colgroup
                        | T::Dd
                        | T::Dt
                        | T::Li
                        | T::Optgroup
                        | T::Option
                        | T::P
                        | T::Rb
                        | T::Rp
                        | T::Rt
                        | T::Rtc
                        | T::Tbody
                        | T::Td
                        | T::Tfoot
                        | T::Th
                        | T::Thead
                        | T::Tr
                )
            {
                self.stack.pop();
            } else {
                break;
            }
        }
    }

    fn close_p(&mut self) {
        self.generate_implied_end_tags(Some(T::P));
        if !self.cur_is(T::P) {
            self.err("unexpected-element-in-open-elements-stack");
        }
        self.pop_until(T::P);
    }

    fn close_p_if_in_button_scope(&mut self) {
        if self.in_scope(T::P, Scope::Button) {
            self.close_p();
        }
    }

    // ----- active formatting elements -------------------------------------------------

    fn push_afe(&mut self, node: NodeId, tag: &Tag) {
        let o = self.current().clone();
        // Noah's Ark clause.
        let mut count = 0;
        let mut earliest = None;
        for i in (0..self.afe.len()).rev() {
            match &self.afe[i] {
                Afe::Marker => break,
                Afe::El { t, name, attrs, .. } => {
                    if *t == o.t && *name == o.name && same_attrs(attrs, &tag.attrs) {
                        count += 1;
                        earliest = Some(i);
                    }
                }
            }
        }
        if count >= 3
            && let Some(i) = earliest
        {
            self.afe.remove(i);
        }
        let line = self.arena.node(node).line;
        self.afe.push(Afe::El {
            node,
            t: o.t,
            name: o.name,
            attrs: tag.attrs.clone(),
            line,
        });
    }

    fn afe_index_of(&self, n: NodeId) -> Option<usize> {
        self.afe
            .iter()
            .rposition(|e| matches!(e, Afe::El { node, .. } if *node == n))
    }

    fn stack_index_of(&self, n: NodeId) -> Option<usize> {
        self.stack.iter().rposition(|o| o.node == n)
    }

    fn reconstruct_afe(&mut self) {
        let Some(last) = self.afe.last() else {
            return;
        };
        match last {
            Afe::Marker => return,
            Afe::El { node, .. } => {
                if self.stack_index_of(*node).is_some() {
                    return;
                }
            }
        }
        let mut i = self.afe.len() - 1;
        loop {
            if i == 0 {
                break;
            }
            i -= 1;
            let stop = match &self.afe[i] {
                Afe::Marker => true,
                Afe::El { node, .. } => self.stack_index_of(*node).is_some(),
            };
            if stop {
                i += 1;
                break;
            }
        }
        while i < self.afe.len() {
            let (name, attrs, line) = match &self.afe[i] {
                Afe::El {
                    name, attrs, line, ..
                } => (name.to_string(), attrs.clone(), *line),
                Afe::Marker => unreachable!(),
            };
            let tag = Tag {
                name,
                attrs,
                ..Tag::default()
            };
            let el = self.insert_html(&tag);
            // Recreated elements have no source line (observed on PHP 8.5).
            let _ = line;
            if let Afe::El { node, .. } = &mut self.afe[i] {
                *node = el;
            }
            i += 1;
        }
    }

    fn clear_afe_to_marker(&mut self) {
        while let Some(e) = self.afe.pop() {
            if matches!(e, Afe::Marker) {
                break;
            }
        }
    }

    /// The adoption agency algorithm. Returns false when the caller must
    /// act as for "any other end tag".
    fn adoption_agency(&mut self, subject: &str, t: T) -> bool {
        // Step 2.
        if let Some(c) = self.stack.last()
            && c.ns == Ns::Html
            && &*c.name == subject
            && self.afe_index_of(c.node).is_none()
        {
            self.stack.pop();
            return true;
        }
        for _ in 0..8 {
            // Step 4.3: the formatting element.
            let mut fe_idx = None;
            for i in (0..self.afe.len()).rev() {
                match &self.afe[i] {
                    Afe::Marker => break,
                    Afe::El { name, .. } if &**name == subject => {
                        fe_idx = Some(i);
                        break;
                    }
                    _ => {}
                }
            }
            let Some(fe_i) = fe_idx else {
                return false;
            };
            let fe = match &self.afe[fe_i] {
                Afe::El { node, .. } => *node,
                Afe::Marker => unreachable!(),
            };
            let Some(fe_s) = self.stack_index_of(fe) else {
                self.err("missing-element-in-open-elements-stack");
                self.afe.remove(fe_i);
                return true;
            };
            if !self.node_in_scope(fe) {
                self.err("missing-element-in-scope");
                return true;
            }
            if self.current().node != fe {
                self.err("unexpected-element-in-open-elements-stack");
            }
            // Furthest block.
            let fb_s = (fe_s + 1..self.stack.len()).find(|&i| {
                let o = &self.stack[i];
                match o.ns {
                    Ns::Html => o.t.is_special(),
                    Ns::Math => matches!(
                        &*o.name,
                        "mi" | "mo" | "mn" | "ms" | "mtext" | "annotation-xml"
                    ),
                    Ns::Svg => matches!(&*o.name, "foreignObject" | "desc" | "title"),
                }
            });
            let Some(fb_s) = fb_s else {
                self.stack.truncate(fe_s);
                self.afe.remove(fe_i);
                return true;
            };
            let fb = self.stack[fb_s].node;
            let common = self.stack[fe_s - 1].node;
            let mut bookmark = fe_i;
            let mut node_s = fb_s;
            let mut last_node = fb;
            let mut inner = 0;
            loop {
                inner += 1;
                node_s -= 1;
                let node = self.stack[node_s].node;
                if node == fe {
                    break;
                }
                let mut node_afe = self.afe_index_of(node);
                if inner > 3
                    && let Some(ai) = node_afe
                {
                    self.afe.remove(ai);
                    if ai < bookmark {
                        bookmark -= 1;
                    }
                    node_afe = None;
                }
                let Some(ai) = node_afe else {
                    self.stack.remove(node_s);
                    continue;
                };
                // Replace node with a new element.
                let (name, attrs, line) = match &self.afe[ai] {
                    Afe::El {
                        name, attrs, line, ..
                    } => (name.to_string(), attrs.clone(), *line),
                    Afe::Marker => unreachable!(),
                };
                let tag = Tag {
                    name: name.clone(),
                    attrs,
                    ..Tag::default()
                };
                let new_el = self.new_element_node(&tag, Ns::Html, &name);
                let _ = line;
                if let Afe::El { node: n, .. } = &mut self.afe[ai] {
                    *n = new_el;
                }
                self.stack[node_s].node = new_el;
                if last_node == fb {
                    bookmark = ai + 1;
                }
                self.arena.detach(last_node);
                self.arena.raw_append(new_el, last_node);
                last_node = new_el;
            }
            // Step 14.
            self.arena.detach(last_node);
            let loc = self.insertion_place(Some(common));
            self.insert_at(loc, last_node);
            // Steps 15-17.
            let (name, attrs, line) = match &self.afe[self.afe_index_of(fe).unwrap()] {
                Afe::El {
                    name, attrs, line, ..
                } => (name.to_string(), attrs.clone(), *line),
                Afe::Marker => unreachable!(),
            };
            let tag = Tag {
                name: name.clone(),
                attrs,
                ..Tag::default()
            };
            let new_el = self.new_element_node(&tag, Ns::Html, &name);
            while let Some(c) = self.arena.first_child(fb) {
                self.arena.detach(c);
                self.arena.raw_append(new_el, c);
            }
            self.arena.raw_append(fb, new_el);
            // Step 18.
            let fe_i = self.afe_index_of(fe).unwrap();
            let entry = Afe::El {
                node: new_el,
                t,
                name: self.intern(&name),
                attrs: tag.attrs.clone(),
                line,
            };
            self.afe.remove(fe_i);
            if bookmark > fe_i {
                bookmark -= 1;
            }
            let bookmark = bookmark.min(self.afe.len());
            self.afe.insert(bookmark, entry);
            // Step 19.
            let fe_s = self.stack_index_of(fe).unwrap();
            let fe_open = self.stack.remove(fe_s);
            let fb_s = self.stack_index_of(fb).unwrap();
            self.stack.insert(
                fb_s + 1,
                Open {
                    node: new_el,
                    ..fe_open
                },
            );
        }
        true
    }

    // ----- mode reset ------------------------------------------------------------------

    fn reset_mode(&mut self) {
        let mut i = self.stack.len();
        loop {
            if i == 0 {
                self.mode = Mode::InBody;
                return;
            }
            i -= 1;
            let last = i == 0;
            let (ns, t) = match &self.ctx {
                Some(c) if last => (c.ns, c.t),
                _ => (self.stack[i].ns, self.stack[i].t),
            };
            if ns != Ns::Html {
                if last {
                    self.mode = Mode::InBody;
                    return;
                }
                continue;
            }
            match t {
                T::Select => {
                    if !last {
                        let mut j = i;
                        while j > 0 {
                            j -= 1;
                            let a = &self.stack[j];
                            if a.ns == Ns::Html && a.t == T::Template {
                                break;
                            }
                            if a.ns == Ns::Html && a.t == T::Table {
                                self.mode = Mode::InSelectInTable;
                                return;
                            }
                        }
                    }
                    self.mode = Mode::InSelect;
                    return;
                }
                T::Td | T::Th if !last => {
                    self.mode = Mode::InCell;
                    return;
                }
                T::Tr => {
                    self.mode = Mode::InRow;
                    return;
                }
                T::Tbody | T::Thead | T::Tfoot => {
                    self.mode = Mode::InTableBody;
                    return;
                }
                T::Caption => {
                    self.mode = Mode::InCaption;
                    return;
                }
                T::Colgroup => {
                    self.mode = Mode::InColumnGroup;
                    return;
                }
                T::Table => {
                    self.mode = Mode::InTable;
                    return;
                }
                T::Template => {
                    self.mode = *self.template_modes.last().unwrap_or(&Mode::InTemplate);
                    return;
                }
                T::Head if !last => {
                    self.mode = Mode::InHead;
                    return;
                }
                T::Body => {
                    self.mode = Mode::InBody;
                    return;
                }
                T::Frameset => {
                    self.mode = Mode::InFrameset;
                    return;
                }
                T::Html => {
                    self.mode = if self.head.is_none() {
                        Mode::BeforeHead
                    } else {
                        Mode::AfterHead
                    };
                    return;
                }
                _ => {
                    if last {
                        self.mode = Mode::InBody;
                        return;
                    }
                }
            }
        }
    }
}

fn same_attrs(a: &[Attr], b: &[Attr]) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|x| b.iter().any(|y| y.name == x.name && y.value == x.value))
}
