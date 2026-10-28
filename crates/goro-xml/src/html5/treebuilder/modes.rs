//! The insertion modes (WHATWG HTML "13.2.6.4 The rules for parsing tokens
//! in HTML content") and the rules for foreign content (13.2.6.5).

use super::super::tags::T;
use super::super::tokenizer::{Doctype, State, Tag};
use super::{Afe, Mode, Ns, Scope, TreeBuilder, all_ws, ws_prefix_len};
use crate::tree::{DtdData, NodeData, QuirksMode};

/// Result of a mode handler: done, or reprocess in the (new) mode.
#[derive(PartialEq, Eq)]
enum Flow {
    Done,
    Again,
}
use Flow::{Again, Done};

const QUIRKS_PUBLIC_PREFIXES: &[&str] = &[
    "+//silmaril//dtd html pro v0r11 19970101//",
    "-//as//dtd html 3.0 aswedit + extensions//",
    "-//advasoft ltd//dtd html 3.0 aswedit + extensions//",
    "-//ietf//dtd html 2.0 level 1//",
    "-//ietf//dtd html 2.0 level 2//",
    "-//ietf//dtd html 2.0 strict level 1//",
    "-//ietf//dtd html 2.0 strict level 2//",
    "-//ietf//dtd html 2.0 strict//",
    "-//ietf//dtd html 2.0//",
    "-//ietf//dtd html 2.1e//",
    "-//ietf//dtd html 3.0//",
    "-//ietf//dtd html 3.2 final//",
    "-//ietf//dtd html 3.2//",
    "-//ietf//dtd html 3//",
    "-//ietf//dtd html level 0//",
    "-//ietf//dtd html level 1//",
    "-//ietf//dtd html level 2//",
    "-//ietf//dtd html level 3//",
    "-//ietf//dtd html strict level 0//",
    "-//ietf//dtd html strict level 1//",
    "-//ietf//dtd html strict level 2//",
    "-//ietf//dtd html strict level 3//",
    "-//ietf//dtd html strict//",
    "-//ietf//dtd html//",
    "-//metrius//dtd metrius presentational//",
    "-//microsoft//dtd internet explorer 2.0 html strict//",
    "-//microsoft//dtd internet explorer 2.0 html//",
    "-//microsoft//dtd internet explorer 2.0 tables//",
    "-//microsoft//dtd internet explorer 3.0 html strict//",
    "-//microsoft//dtd internet explorer 3.0 html//",
    "-//microsoft//dtd internet explorer 3.0 tables//",
    "-//netscape comm. corp.//dtd html//",
    "-//netscape comm. corp.//dtd strict html//",
    "-//o'reilly and associates//dtd html 2.0//",
    "-//o'reilly and associates//dtd html extended 1.0//",
    "-//o'reilly and associates//dtd html extended relaxed 1.0//",
    "-//sq//dtd html 2.0 hotmetal + extensions//",
    "-//softquad software//dtd hotmetal pro 6.0::19990601::extensions to html 4.0//",
    "-//softquad//dtd hotmetal pro 4.0::19971010::extensions to html 4.0//",
    "-//spyglass//dtd html 2.0 extended//",
    "-//sun microsystems corp.//dtd hotjava html//",
    "-//sun microsystems corp.//dtd hotjava strict html//",
    "-//w3c//dtd html 3 1995-03-24//",
    "-//w3c//dtd html 3.2 draft//",
    "-//w3c//dtd html 3.2 final//",
    "-//w3c//dtd html 3.2//",
    "-//w3c//dtd html 3.2s draft//",
    "-//w3c//dtd html 4.0 frameset//",
    "-//w3c//dtd html 4.0 transitional//",
    "-//w3c//dtd html experimental 19960712//",
    "-//w3c//dtd html experimental 970421//",
    "-//w3c//dtd w3 html//",
    "-//w3o//dtd w3 html 3.0//",
    "-//webtechs//dtd mozilla html 2.0//",
    "-//webtechs//dtd mozilla html//",
];

fn starts_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn doctype_quirks(d: &Doctype) -> QuirksMode {
    if d.force_quirks || d.name.as_deref() != Some("html") {
        return QuirksMode::Quirks;
    }
    let public = d.public_id.as_deref();
    let system = d.system_id.as_deref();
    if let Some(p) = public {
        if p.eq_ignore_ascii_case("-//W3O//DTD W3 HTML Strict 3.0//EN//")
            || p.eq_ignore_ascii_case("-/W3C/DTD HTML 4.0 Transitional/EN")
            || p.eq_ignore_ascii_case("HTML")
        {
            return QuirksMode::Quirks;
        }
        if QUIRKS_PUBLIC_PREFIXES.iter().any(|x| starts_ci(p, x)) {
            return QuirksMode::Quirks;
        }
        let html401 = starts_ci(p, "-//W3C//DTD HTML 4.01 Frameset//")
            || starts_ci(p, "-//W3C//DTD HTML 4.01 Transitional//");
        if system.is_none() && html401 {
            return QuirksMode::Quirks;
        }
        if starts_ci(p, "-//W3C//DTD XHTML 1.0 Frameset//")
            || starts_ci(p, "-//W3C//DTD XHTML 1.0 Transitional//")
            || (system.is_some() && html401)
        {
            return QuirksMode::LimitedQuirks;
        }
    }
    if let Some(s) = system
        && s.eq_ignore_ascii_case("http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd")
    {
        return QuirksMode::Quirks;
    }
    QuirksMode::NoQuirks
}

impl TreeBuilder<'_> {
    fn is_fragment(&self) -> bool {
        self.ctx.is_some()
    }

    fn flush_table_text_if_pending(&mut self) {
        if self.mode != Mode::InTableText {
            return;
        }
        let text = std::mem::take(&mut self.pending_table_text);
        if self.pending_table_text_nonws {
            self.err("characters-in-table-text");
            self.foster = true;
            self.reconstruct_afe();
            self.insert_text(&text, self.span.0);
            self.frameset_ok = false;
            self.foster = false;
        } else {
            self.insert_text(&text, self.span.0);
        }
        self.pending_table_text_nonws = false;
        self.mode = self.orig_mode;
    }

    // ----- character tokens ----------------------------------------------------

    pub(super) fn text_in_mode(&mut self, text: &str, first: usize) {
        let mut s = text;
        let owned: String;
        loop {
            if s.is_empty() {
                return;
            }
            match self.mode {
                Mode::Initial => {
                    s = &s[ws_prefix_len(s)..];
                    if s.is_empty() {
                        return;
                    }
                    if !self.opts.no_implied {
                        self.err("unexpected-token-in-initial-mode");
                    }
                    self.quirks = QuirksMode::Quirks;
                    self.mode = Mode::BeforeHtml;
                }
                Mode::BeforeHtml => {
                    s = &s[ws_prefix_len(s)..];
                    if s.is_empty() {
                        return;
                    }
                    self.before_html_anything_else();
                }
                Mode::BeforeHead => {
                    s = &s[ws_prefix_len(s)..];
                    if s.is_empty() {
                        return;
                    }
                    let h = self.insert_implied("head");
                    self.head = Some(h);
                    self.mode = Mode::InHead;
                }
                Mode::InHead | Mode::InHeadNoscript | Mode::AfterHead => {
                    let n = ws_prefix_len(s);
                    if n > 0 {
                        self.insert_text(&s[..n], first);
                        s = &s[n..];
                        if s.is_empty() {
                            return;
                        }
                    }
                    match self.mode {
                        Mode::InHead => {
                            self.pop();
                            self.mode = Mode::AfterHead;
                        }
                        Mode::InHeadNoscript => {
                            self.err("unexpected-token");
                            self.pop();
                            self.mode = Mode::InHead;
                        }
                        _ => {
                            self.insert_implied("body");
                            self.mode = Mode::InBody;
                        }
                    }
                }
                Mode::InBody | Mode::InCaption | Mode::InCell | Mode::InTemplate => {
                    self.body_text(s, first);
                    return;
                }
                Mode::Text => {
                    self.insert_text(s, first);
                    return;
                }
                Mode::InTable | Mode::InTableBody | Mode::InRow => {
                    let cur_table_like = self.stack.last().is_some_and(|o| {
                        o.ns == Ns::Html
                            && matches!(
                                o.t,
                                T::Table | T::Tbody | T::Template | T::Tfoot | T::Thead | T::Tr
                            )
                    });
                    if cur_table_like {
                        self.pending_table_text.clear();
                        self.pending_table_text_nonws = false;
                        self.orig_mode = self.mode;
                        self.mode = Mode::InTableText;
                    } else {
                        // Anything else: foster parenting through "in body".
                        self.err("unexpected-token");
                        self.foster = true;
                        self.body_text(s, first);
                        self.foster = false;
                        return;
                    }
                }
                Mode::InTableText => {
                    if s.contains('\0') {
                        owned = s.replace('\0', "");
                        s = &owned;
                    }
                    if !all_ws(s) {
                        self.pending_table_text_nonws = true;
                    }
                    self.pending_table_text.push_str(s);
                    return;
                }
                Mode::InColumnGroup => {
                    let n = ws_prefix_len(s);
                    if n > 0 {
                        self.insert_text(&s[..n], first);
                        s = &s[n..];
                        if s.is_empty() {
                            return;
                        }
                    }
                    if !self.cur_is(T::Colgroup) {
                        self.err("unexpected-token");
                        // Ignore one character at a time; whitespace among
                        // the rest is also handled here.
                        let c = s.chars().next().unwrap();
                        s = &s[c.len_utf8()..];
                        continue;
                    }
                    self.pop();
                    self.mode = Mode::InTable;
                }
                Mode::InSelect | Mode::InSelectInTable => {
                    if s.contains('\0') {
                        owned = s.replace('\0', "");
                        s = &owned;
                    }
                    self.insert_text(s, first);
                    return;
                }
                Mode::AfterBody | Mode::AfterAfterBody => {
                    let n = ws_prefix_len(s);
                    if n > 0 {
                        self.body_text(&s[..n], first);
                        s = &s[n..];
                        if s.is_empty() {
                            return;
                        }
                    }
                    self.err("unexpected-token");
                    self.mode = Mode::InBody;
                }
                Mode::InFrameset | Mode::AfterFrameset => {
                    let ws: String = s.chars().filter(|c| c.is_ascii_whitespace()).collect();
                    if ws.len() != s.len() {
                        self.err("unexpected-token");
                    }
                    self.insert_text(&ws, first);
                    return;
                }
                Mode::AfterAfterFrameset => {
                    let ws: String = s.chars().filter(|c| c.is_ascii_whitespace()).collect();
                    if ws.len() != s.len() {
                        self.err("unexpected-token");
                    }
                    self.body_text(&ws, first);
                    return;
                }
            }
        }
    }

    fn body_text(&mut self, s: &str, first: usize) {
        let owned;
        let s = if s.contains('\0') {
            owned = s.replace('\0', "");
            &owned
        } else {
            s
        };
        if s.is_empty() {
            return;
        }
        self.reconstruct_afe();
        self.insert_text(s, first);
        if !all_ws(s) {
            self.frameset_ok = false;
        }
    }

    pub(super) fn foreign_text(&mut self, s: &str, first: usize) {
        if s.contains('\0') {
            self.err("unexpected-null-character");
            let r = s.replace('\0', "\u{FFFD}");
            self.insert_text(&r, first);
            if !s.bytes().all(|c| c == 0 || super::is_ws_byte(c)) {
                self.frameset_ok = false;
            }
        } else {
            self.insert_text(s, first);
            if !all_ws(s) {
                self.frameset_ok = false;
            }
        }
    }

    // ----- comments, doctypes, EOF -------------------------------------------------

    pub(super) fn comment_in_mode(&mut self, text: String, start: usize) {
        self.flush_table_text_if_pending();
        match self.mode {
            Mode::Initial | Mode::BeforeHtml | Mode::AfterAfterBody | Mode::AfterAfterFrameset => {
                let doc = self.doc;
                self.insert_comment_at(text, start, (doc, None));
            }
            Mode::AfterBody => {
                let html = self.stack[0].node;
                self.insert_comment_at(text, start, (html, None));
            }
            _ => self.insert_comment(text, start),
        }
    }

    pub(super) fn doctype_in_mode(&mut self, d: Box<Doctype>) {
        self.flush_table_text_if_pending();
        match self.mode {
            Mode::Initial => {
                let bad = d.name.as_deref() != Some("html")
                    || d.public_id.is_some()
                    || d.system_id
                        .as_deref()
                        .is_some_and(|s| s != "about:legacy-compat");
                if bad {
                    self.err("bad-doctype-token-in-initial-mode");
                }
                let dtd = DtdData {
                    name: d.name.as_deref().unwrap_or("").into(),
                    public_id: d.public_id.clone(),
                    system_id: d.system_id.clone(),
                    ..DtdData::default()
                };
                let owner = self.owner;
                let n = self.arena.create(owner, NodeData::Doctype(Box::new(dtd)));
                let line = self.line_at(d.start);
                self.arena.node_mut(n).line = line;
                let doc = self.doc;
                self.arena.raw_append(doc, n);
                if let NodeData::Document(dd) = &mut self.arena.node_mut(doc).data {
                    dd.doctype = Some(n);
                }
                self.quirks = doctype_quirks(&d);
                self.mode = Mode::BeforeHtml;
            }
            Mode::InBody => self.err("doctype-token-in-body-mode"),
            Mode::InHead => self.err("doctype-token-in-head-mode"),
            Mode::BeforeHtml => self.err("doctype-token-in-before-html-mode"),
            Mode::BeforeHead => self.err("doctype-token-in-before-head-mode"),
            Mode::AfterHead => self.err("doctype-token-in-after-head-mode"),
            Mode::AfterAfterBody | Mode::AfterAfterFrameset => {
                self.err("doctype-token-in-body-mode")
            }
            _ => self.err("unexpected-token"),
        }
    }

    pub(super) fn eof_in_mode(&mut self) {
        self.flush_table_text_if_pending();
        loop {
            match self.mode {
                Mode::Initial => {
                    if !self.opts.no_implied {
                        self.err("unexpected-token-in-initial-mode");
                    }
                    self.quirks = QuirksMode::Quirks;
                    self.mode = Mode::BeforeHtml;
                }
                Mode::BeforeHtml => self.before_html_anything_else(),
                Mode::BeforeHead => {
                    let h = self.insert_implied("head");
                    self.head = Some(h);
                    self.mode = Mode::InHead;
                }
                Mode::InHead => {
                    self.pop();
                    self.mode = Mode::AfterHead;
                }
                Mode::InHeadNoscript => {
                    self.pop();
                    self.mode = Mode::InHead;
                }
                Mode::AfterHead => {
                    self.insert_implied("body");
                    self.mode = Mode::InBody;
                }
                Mode::InBody
                | Mode::InTable
                | Mode::InCaption
                | Mode::InColumnGroup
                | Mode::InTableBody
                | Mode::InRow
                | Mode::InCell
                | Mode::InSelect
                | Mode::InSelectInTable => {
                    if !self.template_modes.is_empty() {
                        self.mode = Mode::InTemplate;
                        continue;
                    }
                    return;
                }
                Mode::InTemplate => {
                    if !self.has_in_stack(T::Template) {
                        return;
                    }
                    self.pop_until(T::Template);
                    self.clear_afe_to_marker();
                    self.template_modes.pop();
                    self.reset_mode();
                }
                Mode::Text => {
                    self.pop();
                    self.mode = self.orig_mode;
                }
                Mode::InTableText => {
                    self.mode = self.orig_mode;
                }
                _ => return,
            }
        }
    }

    fn before_html_anything_else(&mut self) {
        let tag = Tag {
            name: "html".into(),
            ..Tag::default()
        };
        let el = self.new_element_node(&tag, Ns::Html, "html");
        let doc = self.doc;
        self.arena.raw_append(doc, el);
        let name = self.intern("html");
        self.stack.push(super::Open {
            node: el,
            ns: Ns::Html,
            t: T::Html,
            name,
            html_ip: false,
        });
        self.mode = Mode::BeforeHead;
    }

    // ----- start tags --------------------------------------------------------------

    pub(super) fn start_tag(&mut self, mut tag: Tag) {
        self.flush_table_text_if_pending();
        let mut t = T::from_name(&tag.name);
        loop {
            let flow = match self.mode {
                Mode::Initial => {
                    if !self.opts.no_implied {
                        self.err("unexpected-token-in-initial-mode");
                    }
                    self.quirks = QuirksMode::Quirks;
                    self.mode = Mode::BeforeHtml;
                    Again
                }
                Mode::BeforeHtml => {
                    if t == T::Html {
                        let el = self.insert_html_root(&tag);
                        self.explicit.push(el);
                        self.mode = Mode::BeforeHead;
                        Done
                    } else {
                        self.before_html_anything_else();
                        Again
                    }
                }
                Mode::BeforeHead => match t {
                    T::Html => self.in_body_start(&mut tag, &mut t),
                    T::Head => {
                        let h = self.insert_html(&tag);
                        self.explicit.push(h);
                        self.head = Some(h);
                        self.mode = Mode::InHead;
                        Done
                    }
                    _ => {
                        let h = self.insert_implied("head");
                        self.head = Some(h);
                        self.mode = Mode::InHead;
                        Again
                    }
                },
                Mode::InHead => self.in_head_start(&mut tag, t),
                Mode::InHeadNoscript => match t {
                    T::Html => self.in_body_start(&mut tag, &mut t),
                    T::Basefont | T::Bgsound | T::Link | T::Meta | T::Noframes | T::Style => {
                        self.in_head_start(&mut tag, t)
                    }
                    T::Head | T::Noscript => {
                        self.err("unexpected-token");
                        Done
                    }
                    _ => {
                        self.err("unexpected-token");
                        self.pop();
                        self.mode = Mode::InHead;
                        Again
                    }
                },
                Mode::AfterHead => match t {
                    T::Html => self.in_body_start(&mut tag, &mut t),
                    T::Body => {
                        let b = self.insert_html(&tag);
                        self.explicit.push(b);
                        self.frameset_ok = false;
                        self.mode = Mode::InBody;
                        Done
                    }
                    T::Frameset => {
                        self.insert_html(&tag);
                        self.mode = Mode::InFrameset;
                        Done
                    }
                    T::Base
                    | T::Basefont
                    | T::Bgsound
                    | T::Link
                    | T::Meta
                    | T::Noframes
                    | T::Script
                    | T::Style
                    | T::Template
                    | T::Title => {
                        self.err("unexpected-token");
                        let head = self.head.expect("head element");
                        let name = self.intern("head");
                        self.stack.push(super::Open {
                            node: head,
                            ns: Ns::Html,
                            t: T::Head,
                            name,
                            html_ip: false,
                        });
                        let f = self.in_head_start(&mut tag, t);
                        if let Some(i) = self.stack.iter().rposition(|o| o.node == head) {
                            self.stack.remove(i);
                        }
                        f
                    }
                    T::Head => {
                        self.err("unexpected-token");
                        Done
                    }
                    _ => {
                        self.insert_implied("body");
                        self.mode = Mode::InBody;
                        Again
                    }
                },
                Mode::InBody => self.in_body_start(&mut tag, &mut t),
                Mode::Text => Done,
                Mode::InTable => self.in_table_start(&mut tag, &mut t),
                Mode::InTableText => unreachable!("flushed above"),
                Mode::InCaption => match t {
                    T::Caption
                    | T::Col
                    | T::Colgroup
                    | T::Tbody
                    | T::Td
                    | T::Tfoot
                    | T::Th
                    | T::Thead
                    | T::Tr => {
                        if !self.in_scope(T::Caption, Scope::Table) {
                            self.err("unexpected-token");
                            Done
                        } else {
                            self.close_caption();
                            Again
                        }
                    }
                    _ => self.in_body_start(&mut tag, &mut t),
                },
                Mode::InColumnGroup => match t {
                    T::Html => self.in_body_start(&mut tag, &mut t),
                    T::Col => {
                        self.insert_html(&tag);
                        self.pop();
                        Done
                    }
                    T::Template => self.in_head_start(&mut tag, t),
                    _ => {
                        if !self.cur_is(T::Colgroup) {
                            self.err("unexpected-token");
                            Done
                        } else {
                            self.pop();
                            self.mode = Mode::InTable;
                            Again
                        }
                    }
                },
                Mode::InTableBody => match t {
                    T::Tr => {
                        self.clear_to_table_body_context();
                        self.insert_html(&tag);
                        self.mode = Mode::InRow;
                        Done
                    }
                    T::Th | T::Td => {
                        self.err("unexpected-token");
                        self.clear_to_table_body_context();
                        self.insert_implied("tr");
                        self.mode = Mode::InRow;
                        Again
                    }
                    T::Caption | T::Col | T::Colgroup | T::Tbody | T::Tfoot | T::Thead => {
                        if !(self.in_scope(T::Tbody, Scope::Table)
                            || self.in_scope(T::Thead, Scope::Table)
                            || self.in_scope(T::Tfoot, Scope::Table))
                        {
                            self.err("unexpected-token");
                            Done
                        } else {
                            self.clear_to_table_body_context();
                            self.pop();
                            self.mode = Mode::InTable;
                            Again
                        }
                    }
                    _ => self.in_table_start(&mut tag, &mut t),
                },
                Mode::InRow => match t {
                    T::Th | T::Td => {
                        self.clear_to_row_context();
                        self.insert_html(&tag);
                        self.mode = Mode::InCell;
                        self.afe.push(Afe::Marker);
                        Done
                    }
                    T::Caption
                    | T::Col
                    | T::Colgroup
                    | T::Tbody
                    | T::Tfoot
                    | T::Thead
                    | T::Tr => {
                        if !self.in_scope(T::Tr, Scope::Table) {
                            self.err("unexpected-token");
                            Done
                        } else {
                            self.clear_to_row_context();
                            self.pop();
                            self.mode = Mode::InTableBody;
                            Again
                        }
                    }
                    _ => self.in_table_start(&mut tag, &mut t),
                },
                Mode::InCell => match t {
                    T::Caption
                    | T::Col
                    | T::Colgroup
                    | T::Tbody
                    | T::Td
                    | T::Tfoot
                    | T::Th
                    | T::Thead
                    | T::Tr => {
                        if !(self.in_scope(T::Td, Scope::Table)
                            || self.in_scope(T::Th, Scope::Table))
                        {
                            self.err("unexpected-token");
                            Done
                        } else {
                            self.close_cell();
                            Again
                        }
                    }
                    _ => self.in_body_start(&mut tag, &mut t),
                },
                Mode::InSelect => self.in_select_start(&mut tag, t),
                Mode::InSelectInTable => match t {
                    T::Caption
                    | T::Table
                    | T::Tbody
                    | T::Tfoot
                    | T::Thead
                    | T::Tr
                    | T::Td
                    | T::Th => {
                        self.err("unexpected-token");
                        self.pop_until(T::Select);
                        self.reset_mode();
                        Again
                    }
                    _ => self.in_select_start(&mut tag, t),
                },
                Mode::InTemplate => match t {
                    T::Base
                    | T::Basefont
                    | T::Bgsound
                    | T::Link
                    | T::Meta
                    | T::Noframes
                    | T::Script
                    | T::Style
                    | T::Template
                    | T::Title => self.in_head_start(&mut tag, t),
                    T::Caption | T::Colgroup | T::Tbody | T::Tfoot | T::Thead => {
                        self.switch_template_mode(Mode::InTable)
                    }
                    T::Col => self.switch_template_mode(Mode::InColumnGroup),
                    T::Tr => self.switch_template_mode(Mode::InTableBody),
                    T::Td | T::Th => self.switch_template_mode(Mode::InRow),
                    _ => self.switch_template_mode(Mode::InBody),
                },
                Mode::AfterBody => {
                    if t == T::Html {
                        self.in_body_start(&mut tag, &mut t)
                    } else {
                        self.err("unexpected-token");
                        self.mode = Mode::InBody;
                        Again
                    }
                }
                Mode::InFrameset => match t {
                    T::Html => self.in_body_start(&mut tag, &mut t),
                    T::Frameset => {
                        self.insert_html(&tag);
                        Done
                    }
                    T::Frame => {
                        self.insert_html(&tag);
                        self.pop();
                        Done
                    }
                    T::Noframes => self.in_head_start(&mut tag, t),
                    _ => {
                        self.err("unexpected-token");
                        Done
                    }
                },
                Mode::AfterFrameset => match t {
                    T::Html => self.in_body_start(&mut tag, &mut t),
                    T::Noframes => self.in_head_start(&mut tag, t),
                    _ => {
                        self.err("unexpected-token");
                        Done
                    }
                },
                Mode::AfterAfterBody => {
                    if t == T::Html {
                        self.in_body_start(&mut tag, &mut t)
                    } else {
                        self.err("unexpected-token");
                        self.mode = Mode::InBody;
                        Again
                    }
                }
                Mode::AfterAfterFrameset => match t {
                    T::Html => self.in_body_start(&mut tag, &mut t),
                    T::Noframes => self.in_head_start(&mut tag, t),
                    _ => {
                        self.err("unexpected-token");
                        Done
                    }
                },
            };
            if flow == Done {
                return;
            }
        }
    }

    fn switch_template_mode(&mut self, m: Mode) -> Flow {
        self.template_modes.pop();
        self.template_modes.push(m);
        self.mode = m;
        Again
    }

    fn insert_html_root(&mut self, tag: &Tag) -> crate::tree::NodeId {
        let el = self.new_element_node(tag, Ns::Html, "html");
        let doc = self.doc;
        self.arena.raw_append(doc, el);
        let name = self.intern("html");
        self.stack.push(super::Open {
            node: el,
            ns: Ns::Html,
            t: T::Html,
            name,
            html_ip: false,
        });
        el
    }

    fn generic_text_element(&mut self, tag: &Tag, state: State) {
        self.insert_html(tag);
        self.tok.state = state;
        self.orig_mode = self.mode;
        self.mode = Mode::Text;
    }

    fn in_head_start(&mut self, tag: &mut Tag, t: T) -> Flow {
        match t {
            T::Html => {
                let mut tt = t;
                self.in_body_start(tag, &mut tt)
            }
            T::Base | T::Basefont | T::Bgsound | T::Link | T::Meta => {
                self.insert_html(tag);
                self.pop();
                Done
            }
            T::Title => {
                self.generic_text_element(tag, State::RcData);
                Done
            }
            T::Noscript if self.opts.scripting => {
                self.generic_text_element(tag, State::RawText);
                Done
            }
            T::Noframes | T::Style => {
                self.generic_text_element(tag, State::RawText);
                Done
            }
            T::Noscript => {
                self.insert_html(tag);
                self.mode = Mode::InHeadNoscript;
                Done
            }
            T::Script => {
                self.generic_text_element(tag, State::ScriptData);
                Done
            }
            T::Template => {
                self.insert_html(tag);
                self.afe.push(Afe::Marker);
                self.frameset_ok = false;
                self.mode = Mode::InTemplate;
                self.template_modes.push(Mode::InTemplate);
                Done
            }
            T::Head => {
                self.err("head-token-in-head-mode");
                Done
            }
            _ => {
                self.pop();
                self.mode = Mode::AfterHead;
                Again
            }
        }
    }

    fn in_body_start(&mut self, tag: &mut Tag, t: &mut T) -> Flow {
        match *t {
            T::Html => {
                self.err("unexpected-token");
                if self.has_in_stack(T::Template) {
                    return Done;
                }
                let top = self.stack[0].node;
                self.add_missing_attrs(top, tag);
                Done
            }
            T::Base
            | T::Basefont
            | T::Bgsound
            | T::Link
            | T::Meta
            | T::Noframes
            | T::Script
            | T::Style
            | T::Template
            | T::Title => self.in_head_start(tag, *t),
            T::Body => {
                self.err("unexpected-token");
                if self.stack.len() < 2
                    || !(self.stack[1].ns == Ns::Html && self.stack[1].t == T::Body)
                    || self.has_in_stack(T::Template)
                {
                    return Done;
                }
                self.frameset_ok = false;
                let body = self.stack[1].node;
                self.add_missing_attrs(body, tag);
                Done
            }
            T::Frameset => {
                self.err("unexpected-token");
                if self.stack.len() < 2
                    || !(self.stack[1].ns == Ns::Html && self.stack[1].t == T::Body)
                    || !self.frameset_ok
                {
                    return Done;
                }
                let body = self.stack[1].node;
                self.arena.detach(body);
                self.stack.truncate(1);
                self.insert_html(tag);
                self.mode = Mode::InFrameset;
                Done
            }
            T::Address
            | T::Article
            | T::Aside
            | T::Blockquote
            | T::Center
            | T::Details
            | T::Dialog
            | T::Dir
            | T::Div
            | T::Dl
            | T::Fieldset
            | T::Figcaption
            | T::Figure
            | T::Footer
            | T::Header
            | T::Hgroup
            | T::Main
            | T::Menu
            | T::Nav
            | T::Ol
            | T::P
            | T::Search
            | T::Section
            | T::Summary
            | T::Ul => {
                self.close_p_if_in_button_scope();
                self.insert_html(tag);
                Done
            }
            T::H1 | T::H2 | T::H3 | T::H4 | T::H5 | T::H6 => {
                self.close_p_if_in_button_scope();
                if self
                    .stack
                    .last()
                    .is_some_and(|o| o.ns == Ns::Html && o.t.is_heading())
                {
                    self.err("unexpected-element-in-open-elements-stack");
                    self.pop();
                }
                self.insert_html(tag);
                Done
            }
            T::Pre | T::Listing => {
                self.close_p_if_in_button_scope();
                self.insert_html(tag);
                self.skip_lf = true;
                self.frameset_ok = false;
                Done
            }
            T::Form => {
                let has_template = self.has_in_stack(T::Template);
                if self.form.is_some() && !has_template {
                    self.err("unexpected-token");
                    return Done;
                }
                self.close_p_if_in_button_scope();
                let f = self.insert_html(tag);
                if !has_template {
                    self.form = Some(f);
                }
                Done
            }
            T::Li | T::Dd | T::Dt => {
                self.frameset_ok = false;
                let mut i = self.stack.len();
                while i > 0 {
                    i -= 1;
                    let o = &self.stack[i];
                    let matches = o.ns == Ns::Html
                        && (if *t == T::Li {
                            o.t == T::Li
                        } else {
                            matches!(o.t, T::Dd | T::Dt)
                        });
                    if matches {
                        let ot = o.t;
                        self.generate_implied_end_tags(Some(ot));
                        if !self.cur_is(ot) {
                            self.err("unexpected-element-in-open-elements-stack");
                        }
                        self.pop_until(ot);
                        break;
                    }
                    let special = match o.ns {
                        Ns::Html => o.t.is_special() && !matches!(o.t, T::Address | T::Div | T::P),
                        Ns::Math => matches!(
                            &*o.name,
                            "mi" | "mo" | "mn" | "ms" | "mtext" | "annotation-xml"
                        ),
                        Ns::Svg => matches!(&*o.name, "foreignObject" | "desc" | "title"),
                    };
                    if special {
                        break;
                    }
                }
                self.close_p_if_in_button_scope();
                self.insert_html(tag);
                Done
            }
            T::Plaintext => {
                self.close_p_if_in_button_scope();
                self.insert_html(tag);
                self.tok.state = State::PlainText;
                Done
            }
            T::Button => {
                if self.in_scope(T::Button, Scope::Default) {
                    self.err("unexpected-token");
                    self.generate_implied_end_tags(None);
                    self.pop_until(T::Button);
                }
                self.reconstruct_afe();
                self.insert_html(tag);
                self.frameset_ok = false;
                Done
            }
            T::A => {
                let mut found = None;
                for e in self.afe.iter().rev() {
                    match e {
                        Afe::Marker => break,
                        Afe::El { node, t: et, .. } if *et == T::A => {
                            found = Some(*node);
                            break;
                        }
                        _ => {}
                    }
                }
                if let Some(a) = found {
                    self.err("unexpected-element-in-active-formatting-stack");
                    self.adoption_agency("a", T::A);
                    if let Some(i) = self.afe_index_of(a) {
                        self.afe.remove(i);
                    }
                    if let Some(i) = self.stack_index_of(a) {
                        self.stack.remove(i);
                    }
                }
                self.reconstruct_afe();
                let el = self.insert_html(tag);
                self.push_afe(el, tag);
                Done
            }
            T::B
            | T::Big
            | T::Code
            | T::Em
            | T::Font
            | T::I
            | T::S
            | T::Small
            | T::Strike
            | T::Strong
            | T::Tt
            | T::U => {
                self.reconstruct_afe();
                let el = self.insert_html(tag);
                self.push_afe(el, tag);
                Done
            }
            T::Nobr => {
                self.reconstruct_afe();
                if self.in_scope(T::Nobr, Scope::Default) {
                    self.err("unexpected-token");
                    self.adoption_agency("nobr", T::Nobr);
                    self.reconstruct_afe();
                }
                let el = self.insert_html(tag);
                self.push_afe(el, tag);
                Done
            }
            T::Applet | T::Marquee | T::Object => {
                self.reconstruct_afe();
                self.insert_html(tag);
                self.afe.push(Afe::Marker);
                self.frameset_ok = false;
                Done
            }
            T::Table => {
                if self.quirks != QuirksMode::Quirks {
                    self.close_p_if_in_button_scope();
                }
                self.insert_html(tag);
                self.frameset_ok = false;
                self.mode = Mode::InTable;
                Done
            }
            T::Area | T::Br | T::Embed | T::Img | T::Keygen | T::Wbr => {
                self.reconstruct_afe();
                self.insert_html(tag);
                self.pop();
                self.frameset_ok = false;
                Done
            }
            T::Input => {
                self.reconstruct_afe();
                self.insert_html(tag);
                self.pop();
                if !tag
                    .attr("type")
                    .is_some_and(|v| v.eq_ignore_ascii_case("hidden"))
                {
                    self.frameset_ok = false;
                }
                Done
            }
            T::Param | T::Source | T::Track => {
                self.insert_html(tag);
                self.pop();
                Done
            }
            T::Hr => {
                self.close_p_if_in_button_scope();
                self.insert_html(tag);
                self.pop();
                self.frameset_ok = false;
                Done
            }
            T::Image => {
                self.err("unexpected-token");
                tag.name = "img".into();
                *t = T::Img;
                self.in_body_start(tag, t)
            }
            T::Textarea => {
                self.insert_html(tag);
                self.skip_lf = true;
                self.tok.state = State::RcData;
                self.orig_mode = self.mode;
                self.frameset_ok = false;
                self.mode = Mode::Text;
                Done
            }
            T::Xmp => {
                self.close_p_if_in_button_scope();
                self.reconstruct_afe();
                self.frameset_ok = false;
                self.generic_text_element(tag, State::RawText);
                Done
            }
            T::Iframe => {
                self.frameset_ok = false;
                self.generic_text_element(tag, State::RawText);
                Done
            }
            T::Noembed => {
                self.generic_text_element(tag, State::RawText);
                Done
            }
            T::Noscript if self.opts.scripting => {
                self.generic_text_element(tag, State::RawText);
                Done
            }
            T::Select => {
                self.reconstruct_afe();
                self.insert_html(tag);
                self.frameset_ok = false;
                self.mode = match self.mode {
                    Mode::InTable
                    | Mode::InCaption
                    | Mode::InTableBody
                    | Mode::InRow
                    | Mode::InCell => Mode::InSelectInTable,
                    _ => Mode::InSelect,
                };
                Done
            }
            T::Optgroup | T::Option => {
                if self.cur_is(T::Option) {
                    self.pop();
                }
                self.reconstruct_afe();
                self.insert_html(tag);
                Done
            }
            T::Rb | T::Rtc => {
                if self.in_scope(T::Ruby, Scope::Default) {
                    self.generate_implied_end_tags(None);
                    if !self.cur_is(T::Ruby) {
                        self.err("missing-element-in-open-elements-stack");
                    }
                }
                self.insert_html(tag);
                Done
            }
            T::Rp | T::Rt => {
                if self.in_scope(T::Ruby, Scope::Default) {
                    self.generate_implied_end_tags(Some(T::Rtc));
                    if !(self.cur_is(T::Rtc) || self.cur_is(T::Ruby)) {
                        self.err("missing-element-in-open-elements-stack");
                    }
                }
                self.insert_html(tag);
                Done
            }
            T::Math | T::Svg => {
                self.reconstruct_afe();
                let ns = if *t == T::Math { Ns::Math } else { Ns::Svg };
                self.insert_element(tag, ns);
                if tag.self_closing {
                    self.pop();
                }
                Done
            }
            T::Caption
            | T::Col
            | T::Colgroup
            | T::Frame
            | T::Head
            | T::Tbody
            | T::Td
            | T::Tfoot
            | T::Th
            | T::Thead
            | T::Tr => {
                self.err("unexpected-token");
                Done
            }
            _ => {
                self.reconstruct_afe();
                self.insert_html(tag);
                Done
            }
        }
    }

    fn add_missing_attrs(&mut self, el: crate::tree::NodeId, tag: &Tag) {
        for a in &tag.attrs {
            if self.arena.get_attr_node(el, &a.name).is_none() {
                let owner = self.owner;
                let q = crate::tree::QName {
                    prefix: None,
                    local: self.intern(&a.name),
                    ns: None,
                };
                let at = self.arena.new_attribute(owner, q, &a.value);
                self.arena.raw_append_attr(el, at);
            }
        }
    }

    fn clear_to_table_context(&mut self) {
        while let Some(o) = self.stack.last() {
            if o.ns == Ns::Html && matches!(o.t, T::Table | T::Template | T::Html) {
                break;
            }
            self.stack.pop();
        }
    }

    fn clear_to_table_body_context(&mut self) {
        while let Some(o) = self.stack.last() {
            if o.ns == Ns::Html
                && matches!(
                    o.t,
                    T::Tbody | T::Tfoot | T::Thead | T::Template | T::Html
                )
            {
                break;
            }
            self.stack.pop();
        }
    }

    fn clear_to_row_context(&mut self) {
        while let Some(o) = self.stack.last() {
            if o.ns == Ns::Html && matches!(o.t, T::Tr | T::Template | T::Html) {
                break;
            }
            self.stack.pop();
        }
    }

    fn close_caption(&mut self) {
        self.generate_implied_end_tags(None);
        if !self.cur_is(T::Caption) {
            self.err("unexpected-element-in-open-elements-stack");
        }
        self.pop_until(T::Caption);
        self.clear_afe_to_marker();
        self.mode = Mode::InTable;
    }

    fn close_cell(&mut self) {
        self.generate_implied_end_tags(None);
        if !(self.cur_is(T::Td) || self.cur_is(T::Th)) {
            self.err("unexpected-element-in-open-elements-stack");
        }
        while let Some(o) = self.stack.pop() {
            if o.ns == Ns::Html && matches!(o.t, T::Td | T::Th) {
                break;
            }
        }
        self.clear_afe_to_marker();
        self.mode = Mode::InRow;
    }

    fn in_table_start(&mut self, tag: &mut Tag, t: &mut T) -> Flow {
        match *t {
            T::Caption => {
                self.clear_to_table_context();
                self.afe.push(Afe::Marker);
                self.insert_html(tag);
                self.mode = Mode::InCaption;
                Done
            }
            T::Colgroup => {
                self.clear_to_table_context();
                self.insert_html(tag);
                self.mode = Mode::InColumnGroup;
                Done
            }
            T::Col => {
                self.clear_to_table_context();
                self.insert_implied("colgroup");
                self.mode = Mode::InColumnGroup;
                Again
            }
            T::Tbody | T::Tfoot | T::Thead => {
                self.clear_to_table_context();
                self.insert_html(tag);
                self.mode = Mode::InTableBody;
                Done
            }
            T::Td | T::Th | T::Tr => {
                self.clear_to_table_context();
                self.insert_implied("tbody");
                self.mode = Mode::InTableBody;
                Again
            }
            T::Table => {
                self.err("unexpected-token");
                if !self.in_scope(T::Table, Scope::Table) {
                    return Done;
                }
                self.pop_until(T::Table);
                self.reset_mode();
                Again
            }
            T::Style | T::Script | T::Template => self.in_head_start(tag, *t),
            T::Input
                if tag
                    .attr("type")
                    .is_some_and(|v| v.eq_ignore_ascii_case("hidden")) =>
            {
                self.err("unexpected-token");
                self.insert_html(tag);
                self.pop();
                Done
            }
            T::Form => {
                self.err("unexpected-token");
                if self.has_in_stack(T::Template) || self.form.is_some() {
                    return Done;
                }
                let f = self.insert_html(tag);
                self.form = Some(f);
                self.pop();
                Done
            }
            _ => {
                // Anything else: foster parenting.
                self.foster = true;
                let f = self.in_body_start(tag, t);
                self.foster = false;
                f
            }
        }
    }

    fn in_select_start(&mut self, tag: &mut Tag, t: T) -> Flow {
        match t {
            T::Html => {
                let mut tt = t;
                self.in_body_start(tag, &mut tt)
            }
            T::Option => {
                if self.cur_is(T::Option) {
                    self.pop();
                }
                self.insert_html(tag);
                Done
            }
            T::Optgroup => {
                if self.cur_is(T::Option) {
                    self.pop();
                }
                if self.cur_is(T::Optgroup) {
                    self.pop();
                }
                self.insert_html(tag);
                Done
            }
            T::Hr => {
                if self.cur_is(T::Option) {
                    self.pop();
                }
                if self.cur_is(T::Optgroup) {
                    self.pop();
                }
                self.insert_html(tag);
                self.pop();
                Done
            }
            T::Select => {
                self.err("unexpected-token");
                if !self.in_scope(T::Select, Scope::Select) {
                    return Done;
                }
                self.pop_until(T::Select);
                self.reset_mode();
                Done
            }
            T::Input | T::Keygen | T::Textarea => {
                self.err("unexpected-token");
                if !self.in_scope(T::Select, Scope::Select) {
                    return Done;
                }
                self.pop_until(T::Select);
                self.reset_mode();
                Again
            }
            T::Script | T::Template => self.in_head_start(tag, t),
            _ => {
                self.err("unexpected-token");
                Done
            }
        }
    }

    // ----- end tags --------------------------------------------------------------

    pub(super) fn end_tag(&mut self, tag: Tag) {
        self.flush_table_text_if_pending();
        let t = T::from_name(&tag.name);
        loop {
            let flow = match self.mode {
                Mode::Initial => {
                    if !self.opts.no_implied {
                        self.err("unexpected-token-in-initial-mode");
                    }
                    self.quirks = QuirksMode::Quirks;
                    self.mode = Mode::BeforeHtml;
                    Again
                }
                Mode::BeforeHtml => match t {
                    T::Head | T::Body | T::Html | T::Br => {
                        self.before_html_anything_else();
                        Again
                    }
                    _ => {
                        self.err("unexpected-closed-token-in-before-html-mode");
                        Done
                    }
                },
                Mode::BeforeHead => match t {
                    T::Head | T::Body | T::Html | T::Br => {
                        let h = self.insert_implied("head");
                        self.head = Some(h);
                        self.mode = Mode::InHead;
                        Again
                    }
                    _ => {
                        self.err("unexpected-closed-token-in-before-head-mode");
                        Done
                    }
                },
                Mode::InHead => self.in_head_end(&tag, t),
                Mode::InHeadNoscript => match t {
                    T::Noscript => {
                        self.pop();
                        self.mode = Mode::InHead;
                        Done
                    }
                    T::Br => {
                        self.err("unexpected-closed-token");
                        self.pop();
                        self.mode = Mode::InHead;
                        Again
                    }
                    _ => {
                        self.err("unexpected-closed-token");
                        Done
                    }
                },
                Mode::AfterHead => match t {
                    T::Template => self.in_head_end(&tag, t),
                    T::Body | T::Html | T::Br => {
                        self.insert_implied("body");
                        self.mode = Mode::InBody;
                        Again
                    }
                    _ => {
                        self.err("unexpected-closed-token");
                        Done
                    }
                },
                Mode::InBody => self.in_body_end(&tag, t),
                Mode::Text => {
                    self.pop();
                    self.mode = self.orig_mode;
                    Done
                }
                Mode::InTable => self.in_table_end(&tag, t),
                Mode::InTableText => unreachable!("flushed above"),
                Mode::InCaption => match t {
                    T::Caption => {
                        if !self.in_scope(T::Caption, Scope::Table) {
                            self.err("unexpected-closed-token");
                        } else {
                            self.close_caption();
                        }
                        Done
                    }
                    T::Table => {
                        if !self.in_scope(T::Caption, Scope::Table) {
                            self.err("unexpected-closed-token");
                            Done
                        } else {
                            self.close_caption();
                            Again
                        }
                    }
                    T::Body
                    | T::Col
                    | T::Colgroup
                    | T::Html
                    | T::Tbody
                    | T::Td
                    | T::Tfoot
                    | T::Th
                    | T::Thead
                    | T::Tr => {
                        self.err("unexpected-closed-token");
                        Done
                    }
                    _ => self.in_body_end(&tag, t),
                },
                Mode::InColumnGroup => match t {
                    T::Colgroup => {
                        if !self.cur_is(T::Colgroup) {
                            self.err("unexpected-closed-token");
                        } else {
                            self.pop();
                            self.mode = Mode::InTable;
                        }
                        Done
                    }
                    T::Col => {
                        self.err("unexpected-closed-token");
                        Done
                    }
                    T::Template => self.in_head_end(&tag, t),
                    _ => {
                        if !self.cur_is(T::Colgroup) {
                            self.err("unexpected-closed-token");
                            Done
                        } else {
                            self.pop();
                            self.mode = Mode::InTable;
                            Again
                        }
                    }
                },
                Mode::InTableBody => match t {
                    T::Tbody | T::Tfoot | T::Thead => {
                        if !self.in_scope(t, Scope::Table) {
                            self.err("unexpected-closed-token");
                        } else {
                            self.clear_to_table_body_context();
                            self.pop();
                            self.mode = Mode::InTable;
                        }
                        Done
                    }
                    T::Table => {
                        if !(self.in_scope(T::Tbody, Scope::Table)
                            || self.in_scope(T::Thead, Scope::Table)
                            || self.in_scope(T::Tfoot, Scope::Table))
                        {
                            self.err("unexpected-closed-token");
                            Done
                        } else {
                            self.clear_to_table_body_context();
                            self.pop();
                            self.mode = Mode::InTable;
                            Again
                        }
                    }
                    T::Body
                    | T::Caption
                    | T::Col
                    | T::Colgroup
                    | T::Html
                    | T::Td
                    | T::Th
                    | T::Tr => {
                        self.err("unexpected-closed-token");
                        Done
                    }
                    _ => self.in_table_end(&tag, t),
                },
                Mode::InRow => match t {
                    T::Tr => {
                        if !self.in_scope(T::Tr, Scope::Table) {
                            self.err("unexpected-closed-token");
                        } else {
                            self.clear_to_row_context();
                            self.pop();
                            self.mode = Mode::InTableBody;
                        }
                        Done
                    }
                    T::Table => {
                        if !self.in_scope(T::Tr, Scope::Table) {
                            self.err("unexpected-closed-token");
                            Done
                        } else {
                            self.clear_to_row_context();
                            self.pop();
                            self.mode = Mode::InTableBody;
                            Again
                        }
                    }
                    T::Tbody | T::Tfoot | T::Thead => {
                        if !self.in_scope(t, Scope::Table) {
                            self.err("unexpected-closed-token");
                            Done
                        } else if !self.in_scope(T::Tr, Scope::Table) {
                            Done
                        } else {
                            self.clear_to_row_context();
                            self.pop();
                            self.mode = Mode::InTableBody;
                            Again
                        }
                    }
                    T::Body | T::Caption | T::Col | T::Colgroup | T::Html | T::Td | T::Th => {
                        self.err("unexpected-closed-token");
                        Done
                    }
                    _ => self.in_table_end(&tag, t),
                },
                Mode::InCell => match t {
                    T::Td | T::Th => {
                        if !self.in_scope(t, Scope::Table) {
                            self.err("unexpected-closed-token");
                        } else {
                            self.generate_implied_end_tags(None);
                            if !self.cur_is(t) {
                                self.err("unexpected-element-in-open-elements-stack");
                            }
                            self.pop_until(t);
                            self.clear_afe_to_marker();
                            self.mode = Mode::InRow;
                        }
                        Done
                    }
                    T::Body | T::Caption | T::Col | T::Colgroup | T::Html => {
                        self.err("unexpected-closed-token");
                        Done
                    }
                    T::Table | T::Tbody | T::Tfoot | T::Thead | T::Tr => {
                        if !self.in_scope(t, Scope::Table) {
                            self.err("unexpected-closed-token");
                            Done
                        } else {
                            self.close_cell();
                            Again
                        }
                    }
                    _ => self.in_body_end(&tag, t),
                },
                Mode::InSelect => self.in_select_end(&tag, t),
                Mode::InSelectInTable => match t {
                    T::Caption
                    | T::Table
                    | T::Tbody
                    | T::Tfoot
                    | T::Thead
                    | T::Tr
                    | T::Td
                    | T::Th => {
                        self.err("unexpected-closed-token");
                        if !self.in_scope(t, Scope::Table) {
                            Done
                        } else {
                            self.pop_until(T::Select);
                            self.reset_mode();
                            Again
                        }
                    }
                    _ => self.in_select_end(&tag, t),
                },
                Mode::InTemplate => match t {
                    T::Template => self.in_head_end(&tag, t),
                    _ => {
                        self.err("unexpected-closed-token");
                        Done
                    }
                },
                Mode::AfterBody => match t {
                    T::Html => {
                        if self.is_fragment() {
                            self.err("unexpected-closed-token");
                        } else {
                            self.mode = Mode::AfterAfterBody;
                        }
                        Done
                    }
                    _ => {
                        self.err("unexpected-closed-token");
                        self.mode = Mode::InBody;
                        Again
                    }
                },
                Mode::InFrameset => match t {
                    T::Frameset => {
                        if self.stack.len() == 1 {
                            self.err("unexpected-closed-token");
                        } else {
                            self.pop();
                            if !self.is_fragment() && !self.cur_is(T::Frameset) {
                                self.mode = Mode::AfterFrameset;
                            }
                        }
                        Done
                    }
                    _ => {
                        self.err("unexpected-closed-token");
                        Done
                    }
                },
                Mode::AfterFrameset => match t {
                    T::Html => {
                        self.mode = Mode::AfterAfterFrameset;
                        Done
                    }
                    _ => {
                        self.err("unexpected-closed-token");
                        Done
                    }
                },
                Mode::AfterAfterBody => {
                    self.err("unexpected-closed-token");
                    self.mode = Mode::InBody;
                    Again
                }
                Mode::AfterAfterFrameset => {
                    self.err("unexpected-closed-token");
                    Done
                }
            };
            if flow == Done {
                return;
            }
        }
    }

    fn in_head_end(&mut self, _tag: &Tag, t: T) -> Flow {
        match t {
            T::Head => {
                self.pop();
                self.mode = Mode::AfterHead;
                Done
            }
            T::Body | T::Html | T::Br => {
                self.pop();
                self.mode = Mode::AfterHead;
                Again
            }
            T::Template => {
                if !self.has_in_stack(T::Template) {
                    self.err("template-closed-token-without-opening-in-head-mode");
                    return Done;
                }
                self.generate_all_implied_end_tags();
                if !self.cur_is(T::Template) {
                    self.err("unexpected-element-in-open-elements-stack");
                }
                self.pop_until(T::Template);
                self.clear_afe_to_marker();
                self.template_modes.pop();
                self.reset_mode();
                Done
            }
            _ => {
                self.err("unexpected-closed-token");
                Done
            }
        }
    }

    fn body_end_ok(&mut self) -> bool {
        // "If the stack of open elements does not have a body element in
        // scope, this is a parse error; ignore the token."
        if !self.in_scope(T::Body, Scope::Default) {
            self.err("unexpected-closed-token");
            return false;
        }
        true
    }

    fn in_body_end(&mut self, tag: &Tag, t: T) -> Flow {
        match t {
            T::Template => self.in_head_end(tag, t),
            T::Body => {
                if self.body_end_ok() {
                    self.mode = Mode::AfterBody;
                }
                Done
            }
            T::Html => {
                if self.body_end_ok() {
                    self.mode = Mode::AfterBody;
                    Again
                } else {
                    Done
                }
            }
            T::Address
            | T::Article
            | T::Aside
            | T::Blockquote
            | T::Button
            | T::Center
            | T::Details
            | T::Dialog
            | T::Dir
            | T::Div
            | T::Dl
            | T::Fieldset
            | T::Figcaption
            | T::Figure
            | T::Footer
            | T::Header
            | T::Hgroup
            | T::Listing
            | T::Main
            | T::Menu
            | T::Nav
            | T::Ol
            | T::Pre
            | T::Search
            | T::Section
            | T::Summary
            | T::Ul => {
                if !self.in_scope(t, Scope::Default) {
                    self.err("unexpected-closed-token");
                    return Done;
                }
                self.generate_implied_end_tags(None);
                if !self.cur_is(t) {
                    self.err("unexpected-element-in-open-elements-stack");
                }
                self.pop_until(t);
                Done
            }
            T::Form => {
                if !self.has_in_stack(T::Template) {
                    let node = self.form.take();
                    match node {
                        Some(n) if self.node_in_scope(n) => {
                            self.generate_implied_end_tags(None);
                            if self.current().node != n {
                                self.err("unexpected-element-in-open-elements-stack");
                            }
                            if let Some(i) = self.stack_index_of(n) {
                                self.stack.remove(i);
                            }
                        }
                        _ => self.err("unexpected-closed-token"),
                    }
                } else {
                    if !self.in_scope(T::Form, Scope::Default) {
                        self.err("unexpected-closed-token");
                        return Done;
                    }
                    self.generate_implied_end_tags(None);
                    if !self.cur_is(T::Form) {
                        self.err("unexpected-element-in-open-elements-stack");
                    }
                    self.pop_until(T::Form);
                }
                Done
            }
            T::P => {
                if !self.in_scope(T::P, Scope::Button) {
                    self.err("unexpected-closed-token");
                    self.insert_implied("p");
                }
                self.close_p();
                Done
            }
            T::Li => {
                if !self.in_scope(T::Li, Scope::ListItem) {
                    self.err("unexpected-closed-token");
                    return Done;
                }
                self.generate_implied_end_tags(Some(T::Li));
                if !self.cur_is(T::Li) {
                    self.err("unexpected-element-in-open-elements-stack");
                }
                self.pop_until(T::Li);
                Done
            }
            T::Dd | T::Dt => {
                if !self.in_scope(t, Scope::Default) {
                    self.err("unexpected-closed-token");
                    return Done;
                }
                self.generate_implied_end_tags(Some(t));
                if !self.cur_is(t) {
                    self.err("unexpected-element-in-open-elements-stack");
                }
                self.pop_until(t);
                Done
            }
            T::H1 | T::H2 | T::H3 | T::H4 | T::H5 | T::H6 => {
                if !self.in_scope_by(Scope::Default, |o| o.ns == Ns::Html && o.t.is_heading()) {
                    self.err("unexpected-closed-token");
                    return Done;
                }
                self.generate_implied_end_tags(None);
                if !self.cur_is(t) {
                    self.err("unexpected-element-in-open-elements-stack");
                }
                self.pop_until_heading();
                Done
            }
            T::A
            | T::B
            | T::Big
            | T::Code
            | T::Em
            | T::Font
            | T::I
            | T::Nobr
            | T::S
            | T::Small
            | T::Strike
            | T::Strong
            | T::Tt
            | T::U => {
                if !self.adoption_agency(&tag.name, t) {
                    self.any_other_end_tag(&tag.name);
                }
                Done
            }
            T::Applet | T::Marquee | T::Object => {
                if !self.in_scope(t, Scope::Default) {
                    self.err("unexpected-closed-token");
                    return Done;
                }
                self.generate_implied_end_tags(None);
                if !self.cur_is(t) {
                    self.err("unexpected-element-in-open-elements-stack");
                }
                self.pop_until(t);
                self.clear_afe_to_marker();
                Done
            }
            T::Br => {
                self.err("unexpected-closed-token");
                let mut tag2 = Tag {
                    name: "br".into(),
                    start: tag.start,
                    name_first: tag.name_first,
                    name_last: tag.name_last,
                    ..Tag::default()
                };
                let mut tt = T::Br;
                self.in_body_start(&mut tag2, &mut tt)
            }
            _ => {
                self.any_other_end_tag(&tag.name);
                Done
            }
        }
    }

    fn any_other_end_tag(&mut self, name: &str) {
        let mut i = self.stack.len();
        while i > 0 {
            i -= 1;
            let o = &self.stack[i];
            if o.ns == Ns::Html && &*o.name == name {
                let n = o.node;
                // Generate implied end tags, except for elements with this name.
                while let Some(c) = self.stack.last() {
                    if c.ns == Ns::Html
                        && &*c.name != name
                        && matches!(
                            c.t,
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
                    {
                        self.stack.pop();
                    } else {
                        break;
                    }
                }
                if self.current().node != n {
                    self.err("unexpected-element-in-open-elements-stack");
                }
                self.pop_until_node(n);
                return;
            }
            let special = match o.ns {
                Ns::Html => o.t.is_special(),
                Ns::Math => matches!(
                    &*o.name,
                    "mi" | "mo" | "mn" | "ms" | "mtext" | "annotation-xml"
                ),
                Ns::Svg => matches!(&*o.name, "foreignObject" | "desc" | "title"),
            };
            if special {
                // PHP reports nothing for this parse error (observed on
                // PHP 8.5: `<body></b>`).
                return;
            }
        }
    }

    fn in_table_end(&mut self, tag: &Tag, t: T) -> Flow {
        match t {
            T::Table => {
                if !self.in_scope(T::Table, Scope::Table) {
                    self.err("unexpected-closed-token");
                    return Done;
                }
                self.pop_until(T::Table);
                self.reset_mode();
                Done
            }
            T::Body
            | T::Caption
            | T::Col
            | T::Colgroup
            | T::Html
            | T::Tbody
            | T::Td
            | T::Tfoot
            | T::Th
            | T::Thead
            | T::Tr => {
                self.err("unexpected-closed-token");
                Done
            }
            T::Template => self.in_head_end(tag, t),
            _ => {
                self.foster = true;
                let f = self.in_body_end(tag, t);
                self.foster = false;
                f
            }
        }
    }

    fn in_select_end(&mut self, tag: &Tag, t: T) -> Flow {
        match t {
            T::Optgroup => {
                if self.cur_is(T::Option)
                    && self.stack.len() >= 2
                    && self.stack[self.stack.len() - 2].ns == Ns::Html
                    && self.stack[self.stack.len() - 2].t == T::Optgroup
                {
                    self.pop();
                }
                if self.cur_is(T::Optgroup) {
                    self.pop();
                } else {
                    self.err("unexpected-closed-token");
                }
                Done
            }
            T::Option => {
                if self.cur_is(T::Option) {
                    self.pop();
                } else {
                    self.err("unexpected-closed-token");
                }
                Done
            }
            T::Select => {
                if !self.in_scope(T::Select, Scope::Select) {
                    self.err("unexpected-closed-token");
                    return Done;
                }
                self.pop_until(T::Select);
                self.reset_mode();
                Done
            }
            T::Template => self.in_head_end(tag, t),
            _ => {
                self.err("unexpected-closed-token");
                Done
            }
        }
    }

    // ----- foreign content ---------------------------------------------------------

    fn breaks_out(tag: &Tag) -> bool {
        match tag.name.as_str() {
            "b" | "big" | "blockquote" | "body" | "br" | "center" | "code" | "dd" | "div"
            | "dl" | "dt" | "em" | "embed" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "head"
            | "hr" | "i" | "img" | "li" | "listing" | "menu" | "meta" | "nobr" | "ol" | "p"
            | "pre" | "ruby" | "s" | "small" | "span" | "strong" | "strike" | "sub" | "sup"
            | "table" | "tt" | "u" | "ul" | "var" => true,
            "font" => tag
                .attrs
                .iter()
                .any(|a| matches!(a.name.as_str(), "color" | "face" | "size")),
            _ => false,
        }
    }

    fn pop_to_html_or_ip(&mut self) {
        while let Some(o) = self.stack.last() {
            if o.ns == Ns::Html || Self::is_mathml_text_ip(o) || Self::is_html_ip(o) {
                break;
            }
            self.stack.pop();
        }
    }

    pub(super) fn foreign_start(&mut self, tag: Tag) {
        self.flush_table_text_if_pending();
        if Self::breaks_out(&tag) {
            self.err("unexpected-token");
            self.pop_to_html_or_ip();
            self.start_tag(tag);
            return;
        }
        let ns = self.adjusted_current().map(|o| o.ns).unwrap_or(Ns::Html);
        self.insert_element(&tag, ns);
        if tag.self_closing {
            self.pop();
        }
    }

    pub(super) fn foreign_end(&mut self, tag: Tag) {
        self.flush_table_text_if_pending();
        if tag.name == "br" || tag.name == "p" {
            self.err("unexpected-closed-token");
            self.pop_to_html_or_ip();
            self.end_tag(tag);
            return;
        }
        let mut i = self.stack.len() - 1;
        if !self.stack[i].name.eq_ignore_ascii_case(&tag.name) {
            self.err("unexpected-closed-token");
        }
        loop {
            if i == 0 {
                return;
            }
            if self.stack[i].name.eq_ignore_ascii_case(&tag.name) {
                self.stack.truncate(i);
                return;
            }
            i -= 1;
            if self.stack[i].ns == Ns::Html {
                self.end_tag(tag);
                return;
            }
        }
    }
}
