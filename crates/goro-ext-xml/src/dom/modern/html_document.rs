//! Dom\HTMLDocument parsing (html_document.c / html5_parser.c): input
//! decoding and chunking exactly like PHP feeds lexbor, parse error
//! reporting with PHP's line/column computation, and the conversion of the
//! parsed tree into the libxml2-style tree (lexbor_libxml2_bridge_convert).

use super::*;
use crate::html5::dom::{LDom, LKind};
use crate::html5::{self, Parser};

pub const HTML_PARSE_NOERROR: i64 = 1 << 5;
pub const XML_PARSE_COMPACT: i64 = 1 << 16;
pub const HTML_PARSE_NOIMPLIED: i64 = 1 << 13;
pub const DOM_HTML_NO_DEFAULT_NS: i64 = 1 << 31;

const MAX_CHUNK: usize = 4096;
const REPLACEMENT: &[u8] = "\u{FFFD}".as_bytes();

/// lexbor's lxb_encoding_decode_utf_8_single state
#[derive(Default)]
struct Utf8State {
    need: u32,
    lower: u8,
    upper: u8,
    cp: u32,
}

enum U8 {
    Cp(u32),
    Error,
    Continue,
}

/// lxb_encoding_decode_utf_8_single (including its chunk-boundary quirks)
fn utf8_single(st: &mut Utf8State, d: &[u8], i: &mut usize, end: usize) -> U8 {
    let mut needed;
    if st.need != 0 {
        needed = st.need;
        st.need = 0;
        if st.lower != 0 {
            let ch = d.get(*i).copied().unwrap_or(0);
            if ch < st.lower || ch > st.upper {
                st.lower = 0;
                st.need = 0;
                return U8::Error;
            }
            *i += 1;
            needed -= 1;
            st.cp = (st.cp << 6) | (ch as u32 & 0x3F);
            st.lower = 0;
        }
    } else {
        let ch = d[*i];
        *i += 1;
        if ch < 0x80 {
            return U8::Cp(ch as u32);
        } else if ch <= 0xDF {
            if ch < 0xC2 {
                return U8::Error;
            }
            needed = 1;
            st.cp = ch as u32 & 0x1F;
        } else if ch < 0xF0 {
            needed = 2;
            st.cp = ch as u32 & 0x0F;
            if *i == end {
                if ch == 0xE0 {
                    st.lower = 0xA0;
                    st.upper = 0xBF;
                } else if ch == 0xED {
                    st.lower = 0x80;
                    st.upper = 0x9F;
                }
                st.need = needed;
                return U8::Continue;
            }
            let bounds = match ch {
                0xE0 => Some((0xA0, 0xBF)),
                0xED => Some((0x80, 0x9F)),
                _ => None,
            };
            if let Some((lo, hi)) = bounds {
                let c2 = d[*i];
                if c2 < lo || c2 > hi {
                    st.lower = 0;
                    st.need = 0;
                    return U8::Error;
                }
                *i += 1;
                needed -= 1;
                st.cp = (st.cp << 6) | (c2 as u32 & 0x3F);
            }
        } else if ch < 0xF5 {
            needed = 3;
            st.cp = ch as u32 & 0x07;
            if *i == end {
                if ch == 0xF0 {
                    st.lower = 0x90;
                    st.upper = 0xBF;
                } else if ch == 0xF4 {
                    st.lower = 0x80;
                    st.upper = 0x8F;
                }
                st.need = needed;
                return U8::Continue;
            }
            let bounds = match ch {
                0xF0 => Some((0x90, 0xBF)),
                0xF4 => Some((0x80, 0x8F)),
                _ => None,
            };
            if let Some((lo, hi)) = bounds {
                let c2 = d[*i];
                if c2 < lo || c2 > hi {
                    st.lower = 0;
                    st.need = 0;
                    return U8::Error;
                }
                *i += 1;
                needed -= 1;
                st.cp = (st.cp << 6) | (c2 as u32 & 0x3F);
            }
        } else {
            return U8::Error;
        }
    }
    let mut p = *i;
    while p < end {
        let ch = d[p];
        if !(0x80..=0xBF).contains(&ch) {
            *i = p;
            st.lower = 0;
            st.need = 0;
            return U8::Error;
        }
        st.cp = (st.cp << 6) | (ch as u32 & 0x3F);
        needed -= 1;
        if needed == 0 {
            *i = p + 1;
            return U8::Cp(st.cp);
        }
        p += 1;
    }
    *i = p;
    st.need = needed;
    U8::Continue
}

fn utf8_encode(cp: u32) -> Vec<u8> {
    match char::from_u32(cp) {
        Some(c) => c.to_string().into_bytes(),
        None => REPLACEMENT.to_vec(),
    }
}

/// One error as PHP reports it: (message, line, column).
pub type ParseErr = (String, i64, i64);

/// dom_lexbor_libxml2_bridge_application_data + the reporting part of
/// dom_process_parse_chunk.
struct Feeder<'a> {
    p: Parser,
    report: bool,
    input_name: String,
    html_no_implied: bool,
    /// fast path: the bytes `current_input_characters` points into
    chars: &'a [u8],
    chars_base: usize,
    /// slow path: the code points of the last decode call
    codepoints: Option<Vec<u32>>,
    cur_len: usize,
    total_offset: usize,
    last_line: usize,
    last_column: usize,
    last_offset: usize,
    tok_idx: usize,
    tree_idx: usize,
    errors: Vec<ParseErr>,
}

impl<'a> Feeder<'a> {
    fn new(report: bool, input_name: &str, no_implied: bool, chars: &'a [u8]) -> Feeder<'a> {
        let mut p = Parser::new();
        p.begin_document();
        Feeder {
            p,
            report,
            input_name: input_name.to_string(),
            html_no_implied: no_implied,
            chars,
            chars_base: 0,
            codepoints: None,
            cur_len: 0,
            total_offset: 0,
            last_line: 1,
            last_column: 1,
            last_offset: 0,
            tok_idx: 0,
            tree_idx: 0,
            errors: Vec::new(),
        }
    }

    /// dom_find_line_and_column_using_cache
    fn find(&mut self, offset: usize) {
        let mut offset = offset.wrapping_sub(self.total_offset);
        if offset > self.cur_len {
            offset = self.cur_len;
        }
        let (mut line, mut col, mut off) = (self.last_line, self.last_column, self.last_offset);
        if let Some(cps) = &self.codepoints {
            while off < offset {
                if cps.get(off).copied() == Some(0x0A) {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
                off += 1;
            }
        } else {
            while off < offset {
                let c = self.chars.get(self.chars_base + off).copied().unwrap_or(0);
                if c == b'\n' {
                    line += 1;
                    col = 1;
                } else if c & 0xC0 != 0x80 {
                    col += 1;
                }
                off += 1;
            }
        }
        self.last_line = line;
        self.last_column = col;
        self.last_offset = off;
    }

    /// dom_process_parse_chunk
    fn chunk(&mut self, data: &[u8], input_len: usize) {
        self.cur_len = input_len;
        let (s, e) = self.p.push_chunk(data);
        self.p.feed(s, e);
        if self.report {
            while self.tok_idx < self.p.tok_errors.len() {
                let err = self.p.tok_errors[self.tok_idx];
                self.tok_idx += 1;
                let off = err.pos.wrapping_sub(s).wrapping_add(self.total_offset);
                self.find(off);
                self.errors.push((
                    format!(
                        "tokenizer error {} in {}, line: {}, column: {}",
                        err.msg, self.input_name, self.last_line, self.last_column
                    ),
                    self.last_line as i64,
                    self.last_column as i64,
                ));
            }
            while self.tree_idx < self.p.tree_errors.len() {
                let err = self.p.tree_errors[self.tree_idx];
                self.tree_idx += 1;
                let (line, column) = (err.line.wrapping_add(1), err.column.wrapping_add(1));
                if line == 1 && self.html_no_implied && err.msg == html5_untoininmo() {
                    continue;
                }
                let msg = if err.length <= 1 {
                    format!(
                        "tree error {} in {}, line: {}, column: {}",
                        err.msg, self.input_name, line, column
                    )
                } else {
                    format!(
                        "tree error {} in {}, line: {}, column: {}-{}",
                        err.msg,
                        self.input_name,
                        line,
                        column,
                        column.wrapping_add(err.length).wrapping_sub(1)
                    )
                };
                self.errors.push((msg, line as i64, column as i64));
            }
            let end = self.total_offset + input_len;
            self.find(end);
        }
        self.total_offset += input_len;
        self.last_offset = 0;
    }
}

fn html5_untoininmo() -> &'static str {
    "unexpected-token-in-initial-mode"
}

/// The decoding state shared by all blocks of one parse.
struct Decoding {
    fast: bool,
    st: Utf8State,
    continue_pending: bool,
    decoder: Option<encoding_rs::Decoder>,
}

impl Decoding {
    fn new(enc: &str) -> Decoding {
        let fast = enc == "UTF-8";
        Decoding {
            fast,
            st: Utf8State::default(),
            continue_pending: false,
            decoder: if fast {
                None
            } else {
                Some(html5::encoding::to_encoding_rs(enc).new_decoder_without_bom_handling())
            },
        }
    }

    /// dom_parse_decode_encode_step for one input block `src[bs..be]`
    /// whose `current_input_characters` is `chars_base`.
    fn step(&mut self, f: &mut Feeder, src: &[u8], bs: usize, be: usize, chars_base: usize) {
        f.chars_base = chars_base;
        if self.fast {
            self.fast_path(f, src, bs, be);
        } else {
            self.slow_path(f, &src[bs..be], false);
        }
    }

    fn fast_path(&mut self, f: &mut Feeder, src: &[u8], bs: usize, be: usize) {
        let mut i = bs;
        if self.continue_pending {
            let r = utf8_single(&mut self.st, src, &mut i, be);
            let bytes = match r {
                U8::Cp(cp) => utf8_encode(cp),
                _ => REPLACEMENT.to_vec(),
            };
            self.continue_pending = false;
            f.chunk(&bytes, i - bs);
        }
        let mut last_output = i;
        while i != be {
            if self.st.need == 0 {
                while i < be && src[i] < 0x80 {
                    i += 1;
                }
                if i == be {
                    break;
                }
            }
            let backup = i;
            match utf8_single(&mut self.st, src, &mut i, be) {
                U8::Cp(_) => {}
                r => {
                    f.chunk(&src[last_output..backup], i - last_output);
                    if let U8::Continue = r {
                        self.continue_pending = true;
                        return;
                    }
                    f.chunk(REPLACEMENT, 0);
                    last_output = i;
                }
            }
        }
        if i != last_output {
            f.chunk(&src[last_output..i], i - last_output);
        }
    }

    fn slow_path(&mut self, f: &mut Feeder, block: &[u8], last: bool) {
        let dec = match &mut self.decoder {
            Some(d) => d,
            None => return,
        };
        let mut out = String::with_capacity(
            dec.max_utf8_buffer_length(block.len())
                .unwrap_or(block.len() * 3 + 16),
        );
        let _ = dec.decode_to_string(block, &mut out, last);
        let cps: Vec<u32> = out.chars().map(|c| c as u32).collect();
        let used = cps.len();
        f.codepoints = Some(cps);
        // the UTF-8 re-encoding goes through a 4096-byte buffer
        let b = out.as_bytes();
        let mut s = 0;
        while s < b.len() {
            let mut e = (s + MAX_CHUNK).min(b.len());
            while e > s && e < b.len() && (b[e] & 0xC0) == 0x80 {
                e -= 1;
            }
            f.chunk(&b[s..e], used);
            s = e;
        }
        if b.is_empty() && !last {
            f.chunk(b"", used);
        }
    }

    /// dom_parse_decode_encode_finish
    fn finish(&mut self, f: &mut Feeder) {
        if self.fast {
            if self.continue_pending {
                f.codepoints = None;
                f.chunk(REPLACEMENT, 1);
            }
        } else {
            self.slow_path(f, b"", true);
        }
    }
}

/// The result of parsing a document.
pub struct Parsed {
    pub parser: Parser,
    pub errors: Vec<ParseErr>,
    pub encoding: &'static str,
}

/// Parse `src` as an HTML document. `blocks` are the (start, end,
/// chars_base) read blocks in `src` coordinates.
pub fn parse_blocks(
    src: &[u8],
    blocks: &[(usize, usize, usize)],
    encoding: &'static str,
    report: bool,
    input_name: &str,
    no_implied: bool,
) -> Parsed {
    let mut f = Feeder::new(report, input_name, no_implied, src);
    let mut dec = Decoding::new(encoding);
    for &(bs, be, cb) in blocks {
        if bs < be {
            dec.step(&mut f, src, bs, be, cb);
        }
    }
    dec.finish(&mut f);
    f.p.finish();
    Parsed {
        parser: f.p,
        errors: f.errors,
        encoding,
    }
}

/// Dom\HTMLDocument::createFromString blocks: 4096-byte slices of the
/// post-BOM data, with `current_input_characters` starting at the
/// pre-BOM buffer start and sliding by the block size.
pub fn string_blocks(len: usize, bom: usize) -> Vec<(usize, usize, usize)> {
    let mut v = Vec::new();
    let mut pos = bom;
    let mut chars = 0;
    while pos < len {
        let sz = (len - pos).min(MAX_CHUNK);
        v.push((pos, pos + sz, chars));
        pos += sz;
        chars += sz;
    }
    v
}

/// Dom\HTMLDocument::createFromFile blocks: 4096-byte reads; the BOM is
/// skipped in the first one; `current_input_characters` is the read buffer.
pub fn file_blocks(len: usize, bom: usize) -> Vec<(usize, usize, usize)> {
    let mut v = Vec::new();
    let mut pos = 0;
    while pos < len {
        let sz = (len - pos).min(MAX_CHUNK);
        let start = if pos == 0 { bom.min(sz) } else { 0 };
        v.push((pos + start, pos + sz, pos));
        pos += sz;
    }
    v
}

/// Parse an HTML fragment (dom_html_fragment_lexbor_parse) in the context
/// of an element with the given (lowercase) name and namespace URI.
pub fn parse_fragment_input(
    context_name: &str,
    context_ns: Option<&str>,
    compat_mode: u8,
    input: &[u8],
) -> Parser {
    let mut p = Parser::new();
    let tag = p
        .tag_id_lookup(context_name.as_bytes())
        .unwrap_or(html5::_UNDEF);
    let ns = match context_ns.unwrap_or("") {
        XHTML_NS => html5::NS_HTML,
        MATHML_NS => html5::NS_MATH,
        SVG_NS => html5::NS_SVG,
        XLINK_NS => html5::NS_XLINK,
        crate::tree::XML_XML_NAMESPACE => html5::NS_XML,
        XMLNS_NS => html5::NS_XMLNS,
        _ => html5::NS_UNDEF,
    };
    p.begin_fragment(tag, ns, compat_mode);
    if std::str::from_utf8(input).is_ok() {
        let (s, e) = p.push_chunk(input);
        p.feed(s, e);
    } else {
        let mut st = Utf8State::default();
        let end = input.len();
        let mut i = 0;
        let mut last_output = 0;
        while i < end {
            if st.need == 0 && input[i] < 0x80 {
                i += 1;
                continue;
            }
            let backup = i;
            match utf8_single(&mut st, input, &mut i, end) {
                U8::Cp(_) => {}
                _ => {
                    let (s, e) = p.push_chunk(&input[last_output..backup]);
                    p.feed(s, e);
                    let (s, e) = p.push_chunk(REPLACEMENT);
                    p.feed(s, e);
                    last_output = i;
                }
            }
        }
        if i != last_output {
            let (s, e) = p.push_chunk(&input[last_output..i]);
            p.feed(s, e);
        }
    }
    p.finish();
    p
}

// ---------------------------------------------------------------------------
// lexbor_libxml2_bridge_convert

fn ns_href(lns: u8) -> &'static str {
    match lns {
        html5::NS_SVG => SVG_NS,
        html5::NS_MATH => MATHML_NS,
        _ => XHTML_NS,
    }
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Convert the children of `start_parent` (a lexbor node) and append them
/// to `root`. Returns the template (element, content fragment) pairs.
pub fn convert(
    t: &mut Tree,
    p: &Parser,
    start_parent: usize,
    doc: NodeId,
    root: NodeId,
    create_default_ns: bool,
) -> Vec<(NodeId, NodeId)> {
    let mut templates = Vec::new();
    let dom = &p.dom;
    // (lexbor node, current active namespace, libxml parent, libxml ns)
    let mut work: Vec<(usize, u8, NodeId, Option<NsId>)> = Vec::new();
    for c in dom.children(start_parent).into_iter().rev() {
        work.push((c, html5::NS_UNDEF, root, None));
    }
    while let Some((n, active, parent, lns)) = work.pop() {
        let node = &dom.nodes[n];
        match node.kind {
            LKind::Element => {
                let name = match &node.qualified {
                    Some(q) => lossy(q),
                    None => lossy(p.tag_name(node.tag)),
                };
                let el = t.new_element(Some(doc), &name);
                t.add_child(parent, el);
                t.nodes[el].line = node.line.min(65535) as u32;
                let mut cur_ns = lns;
                if create_default_ns && node.ns != active {
                    cur_ns = mapper_get_ns(t, doc, None, Some(ns_href(node.ns)));
                }
                t.nodes[el].ns = cur_ns;
                let mut child_parent = el;
                let mut child_list = node.first;
                if node.tag == html5::TEMPLATE && node.ns == html5::NS_HTML {
                    if create_default_ns {
                        let frag = t.new_fragment(Some(doc));
                        t.nodes[frag].parent = Some(el);
                        templates.push((el, frag));
                        child_parent = frag;
                    }
                    if let Some(c) = node.template_content {
                        child_list = dom.nodes[c].first;
                    }
                }
                let mut kids = Vec::new();
                let mut c = child_list;
                while let Some(x) = c {
                    kids.push(x);
                    c = dom.nodes[x].next;
                }
                for k in kids.into_iter().rev() {
                    work.push((k, node.ns, child_parent, cur_ns));
                }
                convert_attrs(t, p, n, el, doc);
            }
            LKind::Text => {
                let tx = t.new_text(Some(doc), &lossy(&node.data));
                let at = t.add_child(parent, tx);
                if at == tx {
                    t.nodes[tx].line = node.line as u32;
                }
            }
            LKind::Doctype => {
                let name = lossy(&node.data);
                let public = (!node.public_id.is_empty()).then(|| lossy(&node.public_id));
                let system = (!node.system_id.is_empty()).then(|| lossy(&node.system_id));
                let dtd =
                    t.create_int_subset(Some(doc), &name, public.as_deref(), system.as_deref());
                // xmlCreateIntSubset puts the DTD first in an HTML document
                if t.nodes[doc].children != Some(dtd) {
                    t.unlink(dtd);
                    match t.nodes[doc].children {
                        Some(first) => t.insert_before_raw(first, dtd),
                        None => t.link_child_raw(doc, dtd),
                    }
                }
            }
            LKind::Comment => {
                let c = t.new_comment(Some(doc), &lossy(&node.data));
                t.add_child(parent, c);
                t.nodes[c].line = node.line.min(65535) as u32;
            }
            _ => {}
        }
    }
    templates
}

fn convert_attrs(t: &mut Tree, p: &Parser, n: usize, el: NodeId, doc: NodeId) {
    let dom: &LDom = &p.dom;
    for a in &dom.nodes[n].attrs {
        let qn = a.qname();
        let local: &[u8] = if a.has_prefix {
            match qn.iter().position(|&c| c == b':') {
                Some(i) => &qn[i + 1..],
                None => qn,
            }
        } else {
            qn
        };
        let local_s = lossy(local);
        let ns = if a.ns == html5::NS_XMLNS {
            if local_s != "xmlns" {
                mapper_get_ns(t, doc, Some("xmlns"), Some(XMLNS_NS))
            } else {
                mapper_get_ns(t, doc, None, Some(XMLNS_NS))
            }
        } else if a.ns == html5::NS_XLINK {
            mapper_get_ns(t, doc, Some("xlink"), Some(XLINK_NS))
        } else {
            None
        };
        let value = lossy(a.value.as_deref().unwrap_or(b""));
        let at = t.new_attr_node(Some(doc), ns, &local_s, None);
        let tx = t.new_text(Some(doc), &value);
        t.link_child_raw(at, tx);
        t.link_child_raw(el, at);
        if local_s == "id" && a.ns == html5::NS_HTML {
            t.add_id_safe(at, &value);
        }
    }
}

fn search_child(t: &Tree, parent: NodeId, name: &str) -> Option<NodeId> {
    let mut c = t.nodes[parent].children;
    while let Some(x) = c {
        if t.nodes[x].t == NType::Element && t.nodes[x].name == name {
            return Some(x);
        }
        c = t.nodes[x].next;
    }
    None
}

fn remove_and_hoist(t: &mut Tree, parent: NodeId, name: &str) {
    if let Some(n) = search_child(t, parent, name) {
        t.unlink(n);
        while let Some(c) = t.nodes[n].children {
            t.unlink(c);
            t.add_child(parent, c);
        }
    }
}

/// dom_post_process_html5_loading
pub fn post_process(t: &mut Tree, doc: NodeId, options: i64, p: &Parser) {
    if options & HTML_PARSE_NOIMPLIED != 0 {
        if let Some(html) = search_child(t, doc, "html") {
            if !p.has_explicit_head_tag {
                remove_and_hoist(t, html, "head");
            }
            if !p.has_explicit_body_tag {
                remove_and_hoist(t, html, "body");
            }
        }
        if !p.has_explicit_html_tag {
            remove_and_hoist(t, doc, "html");
        }
    }
}

/// Build the libxml2-style document for a parsed HTML document.
pub fn build_document(parsed: &Parsed, options: i64) -> NodeId {
    let p = &parsed.parser;
    let d = tree::with(|t| {
        let d = t.new_html_doc();
        if let Some(di) = t.doc_info_mut(d) {
            di.standalone = 1;
            di.parse_options |= crate::parser::XML_PARSE_BIG_LINES;
        }
        d
    });
    mapper_reset(d);
    let templates = tree::with(|t| {
        let tpl = convert(
            t,
            p,
            p.document,
            d,
            d,
            options & DOM_HTML_NO_DEFAULT_NS == 0,
        );
        post_process(t, d, options, p);
        if let Some(di) = t.doc_info_mut(d) {
            di.encoding = Some(parsed.encoding.to_string());
        }
        tpl
    });
    with_doc_props(d, |props| {
        props.quirks_mode = p.compat_mode;
        for (el, frag) in templates {
            props.templates.insert(el, frag);
        }
    });
    d
}

// ---------------------------------------------------------------------------
// Dom\HTMLDocument::createFromString / createFromFile

fn check_options(vm: &mut Vm, f: &str, options: i64) -> R<()> {
    let valid =
        HTML_PARSE_NOERROR | XML_PARSE_COMPACT | HTML_PARSE_NOIMPLIED | DOM_HTML_NO_DEFAULT_NS;
    if options & !valid != 0 {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($options) contains invalid flags (allowed flags: LIBXML_NOERROR, LIBXML_COMPACT, LIBXML_HTML_NOIMPLIED, Dom\\HTML_NO_DEFAULT_NS)",
                f
            ),
        );
        return Err(());
    }
    Ok(())
}

/// dom_should_register_error_handlers
fn should_report(options: i64) -> bool {
    options & HTML_PARSE_NOERROR == 0
}

fn report_errors(vm: &mut Vm, fname: &str, file: &str, errors: &[ParseErr]) {
    for (msg, line, col) in errors {
        crate::libxml::pretend_error(vm, fname, file, *line, *col, msg);
    }
}

fn resolve_override(vm: &mut Vm, f: &str, e: &Option<String>) -> R<Option<&'static str>> {
    match e {
        None => Ok(None),
        Some(s) => match html5::encoding::by_name(s.as_bytes()) {
            Some(n) => Ok(Some(n)),
            None => {
                throw_error(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #3 ($overrideEncoding) must be a valid document encoding",
                        f
                    ),
                );
                Err(())
            }
        },
    }
}

pub fn html_load(vm: &mut Vm, a: &[Value], file: bool) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 3)?;
    let argname = if file { "path" } else { "source" };
    let source = if file {
        a.path_str(vm, 0, argname)?.into_bytes()
    } else {
        a.str_bytes(vm, 0, argname)?
    };
    let options = a.long(vm, 1, "options", 0)?;
    let override_enc = a.opt_path_str(vm, 2, "overrideEncoding")?;
    let fname = a.f.clone();
    if file && source.windows(3).any(|w| w == b"%00") {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($path) must not contain percent-encoded NUL bytes",
                fname
            ),
        );
        return Err(());
    }
    check_options(vm, &fname, options)?;
    let ov = resolve_override(vm, &fname, &override_enc)?;
    let report = should_report(options);
    let no_implied = options & HTML_PARSE_NOIMPLIED != 0;
    let (data, input_name, url) = if file {
        let path = String::from_utf8_lossy(&source).into_owned();
        let (d, u) = read_html_file(vm, &fname, &path)?;
        (d, path, Some(u))
    } else {
        (source, "Entity".to_string(), None)
    };
    let (enc, bom) = match ov {
        Some(e) => (e, 0),
        None => html5::encoding::determine_encoding(&data),
    };
    let blocks = if file {
        file_blocks(data.len(), bom)
    } else {
        string_blocks(data.len(), bom)
    };
    let parsed = parse_blocks(&data, &blocks, enc, report, &input_name, no_implied);
    report_errors(vm, &fname, &input_name, &parsed.errors);
    let d = build_document(&parsed, options);
    if let Some(u) = url {
        tree::with(|t| {
            if let Some(di) = t.doc_info_mut(d) {
                di.url = Some(u);
            }
        });
    }
    if vm.current_exception.is_some() {
        return Err(());
    }
    Ok(super::document::modern_doc_obj(vm, d))
}

fn uri_escape_path(p: &str) -> String {
    let mut out = String::new();
    for &b in p.as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.!~*'();:@&=+$,".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// php_stream_open_wrapper_ex + the reads of createFromFile. Returns the
/// data and the document URL.
fn read_html_file(vm: &mut Vm, fname: &str, path: &str) -> R<(Vec<u8>, String)> {
    if path.is_empty() {
        throw_error(vm, b"ValueError", "Path must not be empty");
        return Err(());
    }
    let lower = path.to_ascii_lowercase();
    let is_wrapper = (path.contains("://") && !lower.starts_with("file://"))
        || lower.starts_with("php:")
        || lower.starts_with("data:");
    if is_wrapper {
        let f = match vm.functions.get(&b"file_get_contents"[..]).copied() {
            Some(f) => f,
            None => return Err(()),
        };
        let mut fargs = vec![Value::String(PhpString::from_string(path.to_string()))];
        if let Some(ctx) = crate::libxml::streams_context() {
            fargs.push(Value::False);
            fargs.push(ctx);
        }
        let r = f(vm, &fargs);
        if vm.current_exception.is_some() {
            return Err(());
        }
        return match r {
            Ok(Value::String(s)) => Ok((s.as_bytes().to_vec(), path.to_string())),
            _ => {
                throw_exception(vm, &format!("Cannot open file '{}'", path));
                Err(())
            }
        };
    }
    let local = crate::io::strip_file_scheme(path);
    let p = std::path::Path::new(&local);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(c) => c.join(p),
            Err(_) => p.to_path_buf(),
        }
    };
    match std::fs::read(&abs) {
        Ok(d) => {
            let canon = std::fs::canonicalize(&abs).unwrap_or(abs);
            let url = format!("file://{}", uri_escape_path(&canon.to_string_lossy()));
            Ok((d, url))
        }
        Err(_) => {
            vm.emit_warning(&format!(
                "{}({}): Failed to open stream: No such file or directory",
                fname, path
            ));
            if vm.current_exception.is_none() {
                throw_exception(vm, &format!("Cannot open file '{}'", path));
            }
            Err(())
        }
    }
}

fn throw_exception(vm: &mut Vm, msg: &str) {
    let ex = vm.create_exception(b"Exception", msg, vm.current_line);
    vm.current_exception = Some(ex);
}
