//! A WHATWG HTML5 parser: a faithful port of lexbor 2.5.0 (the parser
//! bundled with php-src for `Dom\HTMLDocument`), including PHP's lexbor
//! patches (line/column tracking, implied-tag observations) and lexbor's
//! own deviations from the specification, so that trees, quirks modes and
//! the parse errors PHP reports match php-src exactly.
//!
//! The tokenizer works on a single byte arena (`mem`) holding every input
//! chunk back to back: lexbor's pointers become indices into it (0 is the
//! NULL pointer). Chunk boundaries matter because lexbor tracks columns at
//! state-function granularity, so callers feed the same chunks as PHP.

#![allow(clippy::collapsible_else_if)]

pub mod dom;
pub mod encoding;
pub mod tables;
mod tokenizer;
mod tree;
mod treemodes;

#[cfg(test)]
mod tests;

use dom::{LDom, NodeId};
use std::collections::HashMap;
pub use tables::*;

pub const NS_UNDEF: u8 = 0;
pub const NS_HTML: u8 = 2;
pub const NS_MATH: u8 = 3;
pub const NS_SVG: u8 = 4;
pub const NS_XLINK: u8 = 5;
pub const NS_XML: u8 = 6;
pub const NS_XMLNS: u8 = 7;

pub const TOKEN_CLOSE: u32 = 0x1;
pub const TOKEN_CLOSE_SELF: u32 = 0x2;
pub const TOKEN_FORCE_QUIRKS: u32 = 0x4;

pub const CMODE_NO_QUIRKS: u8 = 0;
pub const CMODE_QUIRKS: u8 = 1;
pub const CMODE_LIMITED_QUIRKS: u8 = 2;

#[derive(Clone, Default, Debug)]
pub struct TokenAttr {
    pub name_begin: usize,
    pub name_end: usize,
    pub value_begin: usize,
    pub value_end: usize,
    /// lowercased local name (None: not set yet)
    pub name: Option<Vec<u8>>,
    pub value: Option<Vec<u8>>,
}

#[derive(Clone, Default, Debug)]
pub struct Token {
    pub begin: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    /// indices into the tokenizer's temporary buffer
    pub text_start: usize,
    pub text_end: usize,
    pub attrs: Vec<TokenAttr>,
    /// lexbor's `base_element`: clone attributes from this node
    pub base_element: Option<NodeId>,
    pub null_count: usize,
    pub tag_id: u32,
    pub typ: u32,
}

impl Token {
    fn clean(&mut self) {
        *self = Token::default();
    }
}

/// Insertion modes (lexbor's insertion mode function pointers).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    InHeadNoscript,
    AfterHead,
    InBody,
    InBodySkipNewLine,
    InBodySkipNewLineTextarea,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum AttrAdjust {
    None,
    MathMl,
    Svg,
}

/// A tokenizer error: (message, position in `mem`).
#[derive(Clone, Copy, Debug)]
pub struct TokError {
    pub msg: &'static str,
    pub pos: usize,
}

/// A tree construction error with lexbor's (zero-based) token position.
#[derive(Clone, Copy, Debug)]
pub struct TreeError {
    pub msg: &'static str,
    pub line: usize,
    pub column: usize,
    pub length: usize,
}

pub(crate) use tokenizer::St;

pub struct Parser {
    /// all input chunks back to back; index 0 is the NULL pointer
    pub mem: Vec<u8>,
    // ---- tokenizer ----
    state: St,
    state_return: St,
    token: Token,
    tmp: Vec<u8>,
    begin: usize,
    last: usize,
    markup_static: &'static [u8],
    markup_ptr: usize,
    tmp_tag_id: u32,
    temp: usize,
    entity: usize,
    entity_match: Option<(u8, &'static [u8])>,
    entity_start: usize,
    entity_end: usize,
    entity_number: u32,
    is_attribute: bool,
    is_eof: bool,
    cur_line: usize,
    cur_col: usize,
    pub tok_errors: Vec<TokError>,
    tag_names: HashMap<Vec<u8>, u32>,
    custom_tags: Vec<Vec<u8>>,
    eof_pos: usize,
    // ---- tree builder ----
    pub dom: LDom,
    pub document: NodeId,
    fragment: Option<NodeId>,
    form: Option<NodeId>,
    head: Option<NodeId>,
    open_elements: Vec<NodeId>,
    active_formatting: Vec<NodeId>,
    template_modes: Vec<Mode>,
    pending_table: Vec<Vec<u8>>,
    pending_have_non_ws: bool,
    pub tree_errors: Vec<TreeError>,
    foster_parenting: bool,
    frameset_ok: bool,
    scripting: bool,
    pub has_explicit_html_tag: bool,
    pub has_explicit_head_tag: bool,
    pub has_explicit_body_tag: bool,
    mode: Mode,
    original_mode: Mode,
    before_append_attr: AttrAdjust,
    pub compat_mode: u8,
    /// line of the token the tokenizer is currently emitting
    tkz_token_line: usize,
}

/// The marker entry in the list of active formatting elements.
pub const MARKER: NodeId = 0;

impl Parser {
    pub fn new() -> Parser {
        let mut dom = LDom::new();
        let document = dom.new_document();
        let mut tag_names = HashMap::new();
        for (i, n) in TAG_NAMES.iter().enumerate() {
            tag_names.insert(n.as_bytes().to_vec(), i as u32);
        }
        Parser {
            mem: vec![0],
            state: St::DataBefore,
            state_return: St::Data,
            token: Token::default(),
            tmp: Vec::new(),
            begin: 0,
            last: 0,
            markup_static: b"",
            markup_ptr: 0,
            tmp_tag_id: 0,
            temp: 0,
            entity: 0,
            entity_match: None,
            entity_start: 0,
            entity_end: 0,
            entity_number: 0,
            is_attribute: false,
            is_eof: false,
            cur_line: 0,
            cur_col: 0,
            tok_errors: Vec::new(),
            tag_names,
            custom_tags: Vec::new(),
            eof_pos: 0,
            dom,
            document,
            fragment: None,
            form: None,
            head: None,
            open_elements: Vec::new(),
            active_formatting: Vec::new(),
            template_modes: Vec::new(),
            pending_table: Vec::new(),
            pending_have_non_ws: false,
            tree_errors: Vec::new(),
            foster_parenting: false,
            frameset_ok: true,
            scripting: false,
            has_explicit_html_tag: false,
            has_explicit_head_tag: false,
            has_explicit_body_tag: false,
            mode: Mode::Initial,
            original_mode: Mode::Initial,
            before_append_attr: AttrAdjust::None,
            compat_mode: CMODE_NO_QUIRKS,
            tkz_token_line: 0,
        }
    }

    /// lxb_tag_append_lower
    pub fn tag_id_for(&mut self, name: &[u8]) -> u32 {
        let lower: Vec<u8> = name.iter().map(|c| c.to_ascii_lowercase()).collect();
        if let Some(&id) = self.tag_names.get(&lower) {
            return id;
        }
        let id = _LAST_ENTRY + 1 + self.custom_tags.len() as u32;
        self.custom_tags.push(lower.clone());
        self.tag_names.insert(lower, id);
        id
    }

    /// lxb_tag_data_by_name: known tags only (no insertion)
    pub fn tag_id_lookup(&self, name: &[u8]) -> Option<u32> {
        let lower: Vec<u8> = name.iter().map(|c| c.to_ascii_lowercase()).collect();
        self.tag_names.get(&lower).copied()
    }

    pub fn tag_name(&self, id: u32) -> &[u8] {
        if (id as usize) < TAG_NAMES.len() {
            TAG_NAMES[id as usize].as_bytes()
        } else if id > _LAST_ENTRY {
            &self.custom_tags[(id - _LAST_ENTRY - 1) as usize]
        } else {
            b""
        }
    }

    /// Append an input chunk to the arena, returning its (start, end).
    pub fn push_chunk(&mut self, data: &[u8]) -> (usize, usize) {
        let s = self.mem.len();
        self.mem.extend_from_slice(data);
        (s, self.mem.len())
    }

    /// lxb_html_document_parse_chunk_begin
    pub fn begin_document(&mut self) {
        self.mode = Mode::Initial;
    }

    /// Feed one chunk previously stored with `push_chunk`.
    pub fn feed(&mut self, start: usize, end: usize) {
        self.tkz_chunk(start, end);
    }

    /// lxb_html_document_parse_chunk_end
    pub fn finish(&mut self) {
        // a gap byte so that `last` never equals the EOF buffer pointer
        self.mem.push(0);
        self.eof_pos = self.mem.len();
        self.mem.push(0);
        self.tkz_end();
    }

    /// lxb_html_parse_fragment_chunk_begin with a context element
    /// (tag id or LXB_TAG__UNDEF, namespace) and the owner's quirks mode.
    pub fn begin_fragment(&mut self, tag_id: u32, ns: u8, compat_mode: u8) {
        self.compat_mode = compat_mode;
        self.tkz_set_state_by_tag(false, tag_id, ns);
        self.tmp_tag_id = _UNDEF;
        let root = self.dom.new_element(HTML, NS_HTML);
        let doc = self.document;
        self.dom.append_child(doc, root);
        let ctx = self.dom.new_element(tag_id, ns);
        self.fragment = Some(ctx);
        self.open_elements.push(root);
        if tag_id == TEMPLATE && ns == NS_HTML {
            self.template_modes.push(Mode::InTemplate);
        }
        self.reset_insertion_mode_appropriately();
        if tag_id == FORM && ns == NS_HTML {
            let form = self.dom.new_element(FORM, NS_HTML);
            self.form = Some(form);
        }
    }

    /// The root `<html>` of a fragment parse (its children are the result).
    pub fn fragment_root(&self) -> Option<NodeId> {
        self.dom.nodes[self.document].first
    }
}

impl Default for Parser {
    fn default() -> Self {
        Parser::new()
    }
}
