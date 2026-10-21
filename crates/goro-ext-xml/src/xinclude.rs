//! XInclude processing: a port of libxml2's xinclude.c (2.13) together
//! with the parts of xpointer.c it needs (the element(), xpointer(),
//! xpath1() and xmlns() schemes, bare names and child sequences; no
//! XPointer locations, which libxml2 builds without by default).
//!
//! The XINCLUDE_START/END marker nodes are not supported: PHP's DOM always
//! processes with XML_PARSE_NOXINCNODE, so the xi:include elements are
//! simply replaced by the included nodes.

use crate::libxml::{self, Style};
use crate::parser::{
    ERR_ERROR, ERR_WARNING, FROM_IO, XML_PARSE_DTDLOAD, XML_PARSE_NOENT, XmlError,
};
use crate::tree::{self, NType, NodeId, Tree, XML_XML_NAMESPACE};
use crate::xpath::{self, XErr, XValue};
use goro_core::vm::Vm;

pub const XINCLUDE_NS: &str = "http://www.w3.org/2003/XInclude";
pub const XINCLUDE_OLD_NS: &str = "http://www.w3.org/2001/XInclude";
pub const XML_PARSE_NOXINCNODE: i64 = 1 << 15;
pub const XML_PARSE_NOBASEFIX: i64 = 1 << 18;

const XINCLUDE_MAX_DEPTH: usize = 40;
const XML_FROM_XINCLUDE: i32 = 11;
const XML_ERR_INTERNAL_ERROR: i32 = 1;
const XML_XINCLUDE_RECURSION: i32 = 1600;
const XML_XINCLUDE_PARSE_VALUE: i32 = 1601;
const XML_XINCLUDE_ENTITY_DEF_MISMATCH: i32 = 1602;
const XML_XINCLUDE_NO_HREF: i32 = 1603;
const XML_XINCLUDE_NO_FALLBACK: i32 = 1604;
const XML_XINCLUDE_HREF_URI: i32 = 1605;
const XML_XINCLUDE_TEXT_DOCUMENT: i32 = 1607;
const XML_XINCLUDE_INVALID_CHAR: i32 = 1608;
const XML_XINCLUDE_UNKNOWN_ENCODING: i32 = 1610;
const XML_XINCLUDE_MULTIPLE_ROOT: i32 = 1611;
const XML_XINCLUDE_XPTR_FAILED: i32 = 1612;
const XML_XINCLUDE_XPTR_RESULT: i32 = 1613;
const XML_XINCLUDE_INCLUDE_IN_INCLUDE: i32 = 1614;
const XML_XINCLUDE_FALLBACKS_IN_INCLUDE: i32 = 1615;
const XML_XINCLUDE_FALLBACK_NOT_IN_INCLUDE: i32 = 1616;
const XML_XINCLUDE_FRAGMENT_ID: i32 = 1618;

/// xmlXIncludeRef
struct Ref {
    uri: String,
    fragment: Option<String>,
    base: Option<String>,
    elem: NodeId,
    /// The included copy (a list of unlinked nodes).
    inc: Option<Vec<NodeId>>,
    xml: bool,
    fallback: bool,
    expanding: bool,
    replace: bool,
}

/// xmlXIncludeDoc
struct DocCache {
    doc: Option<NodeId>,
    url: String,
    expanding: bool,
}

struct Ctx {
    fname: String,
    doc: NodeId,
    inc: Vec<Ref>,
    txt: Vec<(String, String)>,
    urls: Vec<DocCache>,
    nb_errors: usize,
    fatal: bool,
    legacy: bool,
    parse_flags: i64,
    depth: usize,
    /// Namespaces registered with the xmlns() scheme (the XPath context is
    /// shared by all XPointer evaluations of a run).
    xp_ns: Vec<(String, String)>,
}

fn is_xinclude_ns(h: Option<&str>) -> bool {
    matches!(h, Some(XINCLUDE_NS) | Some(XINCLUDE_OLD_NS))
}

/// Whether `n` is an xi:include element.
pub fn is_include(t: &Tree, n: NodeId) -> bool {
    is_xi(t, n, "include")
}

/// Whether `n` is an element in an XInclude namespace named `name`.
fn is_xi(t: &Tree, n: NodeId, name: &str) -> bool {
    let nd = &t.nodes[n];
    nd.t == NType::Element && nd.ns.is_some() && nd.name == name && is_xinclude_ns(t.ns_href(nd.ns))
}

fn is_char(c: u32) -> bool {
    matches!(c, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

/// xmlURIEscapeStr(str, "/;&=+$,")
fn uri_escape(s: &str) -> String {
    let mut out = String::new();
    for &c in s.as_bytes() {
        if c.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&c) || b"/;&=+$,".contains(&c) {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{:02X}", c));
        }
    }
    out
}

/// xmlBuildRelativeURISafe (without the Windows drive handling).
pub fn build_relative_uri(uri: &str, base: &str) -> Option<String> {
    if uri.is_empty() {
        return None;
    }
    let r = match crate::uri::parse_uri(uri) {
        Some(r) => r,
        None => return Some(uri.to_string()),
    };
    if base.is_empty() {
        return Some(uri.to_string());
    }
    let b = match crate::uri::parse_uri(base) {
        Some(b) => b,
        None => return Some(uri.to_string()),
    };
    if r.scheme != b.scheme || r.authority != b.authority {
        return Some(uri.to_string());
    }
    if r.path == b.path {
        return Some(String::new());
    }
    if b.path.is_empty() {
        return Some(uri_escape(&r.path));
    }
    let rpath = if r.path.is_empty() {
        "/".to_string()
    } else {
        r.path.clone()
    };
    let (bp, rp) = (b.path.as_bytes(), rpath.as_bytes());
    if (bp[0] == b'/') != (rp[0] == b'/') {
        return Some(uri.to_string());
    }
    let mut pos = 0;
    while pos < bp.len() && pos < rp.len() && bp[pos] == rp[pos] {
        pos += 1;
    }
    if pos == bp.len() && pos == rp.len() {
        return Some(String::new());
    }
    let mut ix = pos;
    while ix > 0 && rp[ix - 1] != b'/' {
        ix -= 1;
    }
    let uptr = &rpath[ix..];
    let nbslash = bp[ix.min(bp.len())..]
        .iter()
        .filter(|&&c| c == b'/')
        .count();
    if nbslash == 0 && uptr.is_empty() {
        return Some("./".to_string());
    }
    if nbslash == 0 {
        return Some(uri_escape(uptr));
    }
    let mut val = "../".repeat(nbslash);
    if let Some(rest) = uptr.strip_prefix('/') {
        val.push_str(rest);
    } else {
        val.push_str(uptr);
    }
    Some(uri_escape(&val))
}

/// The XPointer evaluator state (xmlXPathParserContext as used by
/// xpointer.c): the input, the value on top of the stack and the error.
struct XPtr<'a> {
    s: &'a [u8],
    pos: usize,
    doc: NodeId,
    value: Option<XValue>,
    /// ctxt->error (reset per XPointer part)
    error: XpErr,
    /// ctx->lastError.code != XML_ERR_OK
    failed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum XpErr {
    Ok,
    UnknownScheme,
    Other,
}

fn is_name_start(c: u8, colon: bool) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 || (colon && c == b':')
}

fn is_name_char(c: u8, colon: bool) -> bool {
    is_name_start(c, colon) || c.is_ascii_digit() || c == b'.' || c == b'-'
}

impl<'a> XPtr<'a> {
    fn cur(&self) -> u8 {
        self.s.get(self.pos).copied().unwrap_or(0)
    }

    fn nxt(&self, n: usize) -> u8 {
        self.s.get(self.pos + n).copied().unwrap_or(0)
    }

    fn next(&mut self) {
        if self.pos < self.s.len() {
            self.pos += 1;
        }
    }

    fn skip_blanks(&mut self) {
        while matches!(self.cur(), b' ' | b'\t' | b'\n' | b'\r') {
            self.next();
        }
    }

    /// xmlXPathParseName / xmlXPathParseNCName
    fn parse_name(&mut self, colon: bool) -> Option<String> {
        if !is_name_start(self.cur(), colon) {
            return None;
        }
        let start = self.pos;
        while self.pos < self.s.len() && is_name_char(self.s[self.pos], colon) {
            self.pos += 1;
        }
        Some(String::from_utf8_lossy(&self.s[start..self.pos]).into_owned())
    }

    /// xmlXPathErr: only the first error of a part is kept.
    fn xpath_err(&mut self, vm: &mut Vm, fname: &str, e: XErr) {
        if self.error != XpErr::Ok {
            return;
        }
        self.error = XpErr::Other;
        self.failed = true;
        xpath::report_error(vm, fname, &e, "");
    }

    /// xmlXPtrErr
    fn xptr_err(&mut self, kind: XpErr) {
        if self.error != XpErr::Ok {
            return;
        }
        self.error = kind;
        self.failed = true;
    }

    /// xmlXPtrGetChildNo
    fn get_child_no(&mut self, vm: &mut Vm, fname: &str, indx: i64) {
        let set = match self.value.take() {
            Some(XValue::NodeSet(s)) => s,
            other => {
                self.value = other;
                self.xpath_err(vm, fname, XErr::InvalidType);
                return;
            }
        };
        if indx <= 0 || set.len() != 1 {
            self.value = Some(XValue::NodeSet(Vec::new()));
            return;
        }
        let cur = tree::with(|t| nth_child(t, set[0], indx));
        self.value = Some(XValue::NodeSet(cur.into_iter().collect()));
    }

    /// xmlXPtrEvalChildSeq on the current input.
    fn child_seq(&mut self, vm: &mut Vm, fname: &str, name: Option<String>) {
        if name.is_none() && self.cur() == b'/' && self.nxt(1) != b'1' {
            self.xptr_err(XpErr::Other);
        }
        if let Some(n) = name {
            // id(name)
            let doc = self.doc;
            let ids: Vec<NodeId> = tree::with(|t| {
                let mut out = Vec::new();
                if let Some(info) = t.doc_info(doc) {
                    for tok in n.split_ascii_whitespace() {
                        if let Some(&a) = info.ids.get(tok) {
                            if let Some(p) = t.nodes[a].parent {
                                out.push(p);
                            }
                        }
                    }
                }
                out
            });
            let mut ids = ids;
            xpath::sort_dedup(&mut ids);
            self.value = Some(XValue::NodeSet(ids));
        }
        while self.cur() == b'/' {
            self.next();
            let mut child: i64 = 0;
            let mut overflow = false;
            while self.cur().is_ascii_digit() {
                let d = (self.cur() - b'0') as i64;
                if child > i32::MAX as i64 / 10 {
                    overflow = true;
                } else {
                    child *= 10;
                }
                if child > i32::MAX as i64 - d {
                    overflow = true;
                } else {
                    child += d;
                }
                self.next();
            }
            if overflow {
                child = 0;
            }
            self.get_child_no(vm, fname, child);
        }
    }

    fn root(&mut self) {
        self.value = Some(XValue::NodeSet(vec![self.doc]));
    }

    /// xmlXPtrEvalXPtrPart
    fn eval_part(
        &mut self,
        vm: &mut Vm,
        fname: &str,
        name: String,
        ns: &mut Vec<(String, String)>,
    ) {
        if self.cur() != b'(' {
            return self.xpath_err(vm, fname, XErr::InvalidExpr);
        }
        self.next();
        let mut level = 1;
        let mut buffer: Vec<u8> = Vec::new();
        while self.cur() != 0 {
            let c = self.cur();
            if c == b')' {
                level -= 1;
                if level == 0 {
                    self.next();
                    break;
                }
            } else if c == b'(' {
                level += 1;
            } else if c == b'^' && matches!(self.nxt(1), b')' | b'(' | b'^') {
                self.next();
            }
            buffer.push(self.cur());
            self.next();
        }
        if level != 0 && self.cur() == 0 {
            return self.xpath_err(vm, fname, XErr::Syntax);
        }
        match name.as_str() {
            "xpointer" | "xpath1" => {
                let expr = String::from_utf8_lossy(&buffer).into_owned();
                let mut ctx = xpath::Ctx {
                    ns,
                    vars: &[],
                    ext: None,
                    doc: Some(self.doc),
                };
                match xpath::eval_ctx(&expr, self.doc, &mut ctx) {
                    Ok(v) => self.value = Some(v),
                    Err(e) => self.xpath_err(vm, fname, e),
                }
            }
            "element" => {
                let mut sub = XPtr {
                    s: &buffer,
                    pos: 0,
                    doc: self.doc,
                    value: self.value.take(),
                    error: self.error,
                    failed: self.failed,
                };
                if buffer.first() == Some(&b'/') {
                    sub.root();
                    sub.child_seq(vm, fname, None);
                } else {
                    match sub.parse_name(true) {
                        Some(n2) => sub.child_seq(vm, fname, Some(n2)),
                        None => sub.xpath_err(vm, fname, XErr::InvalidExpr),
                    }
                }
                self.value = sub.value;
                self.error = sub.error;
                self.failed = sub.failed;
            }
            "xmlns" => {
                let mut sub = XPtr {
                    s: &buffer,
                    pos: 0,
                    doc: self.doc,
                    value: None,
                    error: XpErr::Ok,
                    failed: false,
                };
                let prefix = match sub.parse_name(false) {
                    Some(p) => p,
                    None => return self.xpath_err(vm, fname, XErr::Syntax),
                };
                sub.skip_blanks();
                if sub.cur() != b'=' {
                    return self.xpath_err(vm, fname, XErr::Syntax);
                }
                sub.next();
                sub.skip_blanks();
                let uri = String::from_utf8_lossy(&buffer[sub.pos..]).into_owned();
                ns.retain(|(p, _)| *p != prefix);
                ns.push((prefix, uri));
            }
            _ => self.xptr_err(XpErr::UnknownScheme),
        }
    }

    /// xmlXPtrEvalFullXPtr
    fn eval_full(
        &mut self,
        vm: &mut Vm,
        fname: &str,
        first: String,
        ns: &mut Vec<(String, String)>,
    ) {
        let mut name = Some(first);
        while let Some(n) = name {
            self.error = XpErr::Ok;
            self.eval_part(vm, fname, n, ns);
            // in case of syntax error, stop here
            if self.error == XpErr::Other {
                return;
            }
            if let Some(XValue::NodeSet(s)) = &self.value {
                if !s.is_empty() {
                    return;
                }
            }
            // improper values are a sub-resource error
            self.value = None;
            self.skip_blanks();
            name = self.parse_name(true);
        }
    }

    /// xmlXPtrEvalXPointer
    fn eval_xpointer(&mut self, vm: &mut Vm, fname: &str, ns: &mut Vec<(String, String)>) {
        self.skip_blanks();
        if self.cur() == b'/' {
            self.root();
            self.child_seq(vm, fname, None);
        } else {
            let name = match self.parse_name(true) {
                Some(n) => n,
                None => return self.xpath_err(vm, fname, XErr::InvalidExpr),
            };
            if self.cur() == b'(' {
                return self.eval_full(vm, fname, name, ns);
            }
            self.child_seq(vm, fname, Some(name));
        }
        self.skip_blanks();
        if self.cur() != 0 {
            self.xpath_err(vm, fname, XErr::InvalidExpr);
        }
    }
}

/// xmlXPtrEval: Err(()) when the evaluation failed, Ok(None) when it
/// produced no value.
fn xptr_eval(
    vm: &mut Vm,
    fname: &str,
    s: &str,
    doc: NodeId,
    ns: &mut Vec<(String, String)>,
) -> Result<Option<Vec<NodeId>>, ()> {
    let mut x = XPtr {
        s: s.as_bytes(),
        pos: 0,
        doc,
        value: None,
        error: XpErr::Ok,
        failed: false,
    };
    x.eval_xpointer(vm, fname, ns);
    if x.failed {
        return Err(());
    }
    match x.value {
        None => Ok(None),
        Some(XValue::NodeSet(v)) => Ok(Some(v)),
        // "evaluation failed to return a node set"
        Some(_) => Err(()),
    }
}

/// xmlXPtrGetNthChild
fn nth_child(t: &Tree, cur: NodeId, no: i64) -> Option<NodeId> {
    if t.nodes[cur].t == NType::NamespaceDecl {
        return Some(cur);
    }
    let mut i = 0;
    let mut c = t.nodes[cur].children;
    while let Some(x) = c {
        if matches!(
            t.nodes[x].t,
            NType::Element | NType::Document | NType::HtmlDocument
        ) {
            i += 1;
            if i == no {
                return Some(x);
            }
        }
        c = t.nodes[x].next;
    }
    None
}

impl Ctx {
    /// xmlXIncludeErr
    fn err(&mut self, vm: &mut Vm, node: Option<NodeId>, code: i32, msg: String) {
        if self.fatal {
            return;
        }
        self.nb_errors += 1;
        let mut e = XmlError::simple(XML_FROM_XINCLUDE, code, ERR_ERROR, msg);
        if let Some(n) = node {
            tree::with(|t| {
                e.line = t.nodes[n].line as i32;
                e.file = t.nodes[n].doc.and_then(|d| t.doc_url(d));
            });
        }
        libxml::report(vm, &self.fname, &[e], Style::Generic);
    }

    /// The I/O warning of a failed xmlLoadExternalEntity.
    fn io_warning(&self, vm: &mut Vm, url: &str) {
        // XML_IO_ENOENT
        let mut e = XmlError::simple(
            FROM_IO,
            1524,
            ERR_WARNING,
            format!("failed to load \"{}\": No such file or directory\n", url),
        );
        e.formatted = vec![format!(
            "I/O warning : failed to load \"{}\": No such file or directory",
            url
        )];
        libxml::report(vm, &self.fname, &[e], Style::Generic);
    }

    /// xmlXIncludeGetProp
    fn get_prop(&self, t: &Tree, cur: NodeId, name: &str) -> Option<String> {
        if let Some(a) = t.has_ns_prop(cur, name, Some(XINCLUDE_NS)) {
            return Some(t.node_get_content(a));
        }
        if self.legacy {
            if let Some(a) = t.has_ns_prop(cur, name, Some(XINCLUDE_OLD_NS)) {
                return Some(t.node_get_content(a));
            }
        }
        t.has_ns_prop(cur, name, None)
            .map(|a| t.node_get_content(a))
    }

    /// xmlXIncludeAddNode
    fn add_node(&mut self, vm: &mut Vm, cur: NodeId) -> Option<usize> {
        let (mut fragment, href, parse, base, doc_url, doc_flags) = tree::with(|t| {
            let fragment = self.get_prop(t, cur, "xpointer");
            let href = self.get_prop(t, cur, "href");
            let parse = self.get_prop(t, cur, "parse");
            let base = crate::dom::node::node_get_base(t, cur);
            let doc_url = t.doc_url(self.doc);
            let doc_flags = t.nodes[cur]
                .doc
                .and_then(|d| t.doc_info(d))
                .map(|i| i.parse_options);
            (fragment, href, parse, base, doc_url, doc_flags)
        });
        let href = match href {
            Some(h) => h,
            None => {
                if fragment.is_none() {
                    self.err(
                        vm,
                        Some(cur),
                        XML_XINCLUDE_NO_HREF,
                        "href or xpointer must be present\n".to_string(),
                    );
                    return None;
                }
                String::new()
            }
        };
        let mut xml = true;
        if let Some(p) = &parse {
            if p == "xml" {
                xml = true;
            } else if p == "text" {
                xml = false;
            } else {
                self.err(
                    vm,
                    Some(cur),
                    XML_XINCLUDE_PARSE_VALUE,
                    format!("invalid value {} for 'parse'\n", p),
                );
                return None;
            }
        }
        let uri = match crate::uri::parse_uri(&href) {
            Some(u) => u,
            None => {
                self.err(
                    vm,
                    Some(cur),
                    XML_XINCLUDE_HREF_URI,
                    format!("invalid value href {}\n", href),
                );
                return None;
            }
        };
        let mut href = href;
        if let Some(f) = uri.fragment {
            if self.legacy {
                if fragment.is_none() {
                    fragment = Some(f);
                }
            } else {
                self.err(
                    vm,
                    Some(cur),
                    XML_XINCLUDE_FRAGMENT_ID,
                    format!(
                        "Invalid fragment identifier in URI {} use the xpointer attribute\n",
                        href
                    ),
                );
                return None;
            }
            if let Some(i) = href.find('#') {
                href.truncate(i);
            }
        }
        let local = if !href.is_empty() {
            match crate::uri::build_uri(&href, base.as_deref()) {
                Some(h) => href = h,
                None => {
                    self.err(
                        vm,
                        Some(cur),
                        XML_XINCLUDE_HREF_URI,
                        "failed build URL\n".to_string(),
                    );
                    return None;
                }
            }
            doc_url.as_deref() == Some(href.as_str())
        } else {
            true
        };
        if local && xml && fragment.as_deref().unwrap_or("").is_empty() {
            self.err(
                vm,
                Some(cur),
                XML_XINCLUDE_RECURSION,
                format!("detected a local recursion with no xpointer in {}\n", href),
            );
            return None;
        }
        let base = if self.parse_flags & XML_PARSE_NOBASEFIX == 0
            && doc_flags
                .map(|f| f & XML_PARSE_NOBASEFIX == 0)
                .unwrap_or(false)
        {
            Some(base.unwrap_or_default())
        } else {
            None
        };
        self.inc.push(Ref {
            uri: href,
            fragment,
            base,
            elem: cur,
            inc: None,
            xml,
            fallback: false,
            expanding: false,
            replace: false,
        });
        Some(self.inc.len() - 1)
    }

    /// xmlXIncludeRecurseDoc
    fn recurse_doc(&mut self, vm: &mut Vm, doc: NodeId) {
        let old_doc = self.doc;
        let old_inc = std::mem::take(&mut self.inc);
        self.doc = doc;
        if let Some(root) = tree::with(|t| t.root_element(doc)) {
            self.do_process(vm, root);
        }
        self.doc = old_doc;
        self.inc = old_inc;
    }

    /// xmlXIncludeBaseFixup
    fn base_fixup(&mut self, vm: &mut Vm, cur: NodeId, copy: NodeId, target: &str) {
        let base = tree::with(|t| {
            if t.nodes[cur].t == NType::Element {
                Some(crate::dom::node::node_get_base(t, cur))
            } else {
                None
            }
        });
        let base = match base {
            Some(b) => b,
            None => return,
        };
        if let Some(b) = base.as_deref().filter(|b| *b != target) {
            match build_relative_uri(b, target) {
                None => {
                    self.err(
                        vm,
                        Some(cur),
                        XML_XINCLUDE_HREF_URI,
                        format!("Building relative URI failed: {}\n", b),
                    );
                    return;
                }
                Some(rel) => {
                    if rel.contains('/') {
                        tree::with(|t| {
                            let ns = t.search_ns_by_href(copy, XML_XML_NAMESPACE);
                            t.set_ns_prop(copy, ns, "base", &rel);
                        });
                        return;
                    }
                }
            }
        }
        // delete an existing xml:base when the bases are equal
        tree::with(|t| {
            if let Some(a) = t.has_ns_prop(copy, "base", Some(XML_XML_NAMESPACE)) {
                t.unlink(a);
            }
        });
    }

    /// xmlXIncludeCopyNode: copy `elem` (or its children) into the current
    /// document, expanding nested includes. Err(()) on error.
    fn copy_node(
        &mut self,
        vm: &mut Vm,
        elem: NodeId,
        copy_children: bool,
        target_base: Option<&str>,
    ) -> Result<Vec<NodeId>, ()> {
        let src: Vec<NodeId> = if copy_children {
            tree::with(|t| t.children_of(elem))
        } else {
            vec![elem]
        };
        self.copy_list(vm, &src, None, 0, target_base)
    }

    fn copy_list(
        &mut self,
        vm: &mut Vm,
        src: &[NodeId],
        parent: Option<NodeId>,
        depth: usize,
        target_base: Option<&str>,
    ) -> Result<Vec<NodeId>, ()> {
        let mut result = Vec::new();
        let doc = self.doc;
        for &cur in src {
            let (ty, xi) = tree::with(|t| (t.nodes[cur].t, is_xi(t, cur, "include")));
            if matches!(ty, NType::Document | NType::Dtd) {
                continue;
            }
            if xi {
                let idx = match self.expand_node(vm, cur) {
                    Some(i) => i,
                    None => return Err(()),
                };
                let items = self.inc[idx].inc.clone().unwrap_or_default();
                for item in items {
                    let copy = tree::with(|t| {
                        let c = t.copy_node(item, Some(doc), 1, parent);
                        if let Some(p) = parent {
                            t.link_child_raw(p, c);
                        }
                        c
                    });
                    result.push(copy);
                    if depth == 0 {
                        if let Some(tb) = target_base {
                            self.base_fixup(vm, item, copy, tb);
                        }
                    }
                }
                continue;
            }
            let (copy, kids) = tree::with(|t| {
                let c = t.copy_node(cur, Some(doc), 2, parent);
                if let Some(p) = parent {
                    t.link_child_raw(p, c);
                }
                let kids = if ty == NType::Element {
                    t.children_of(cur)
                } else {
                    Vec::new()
                };
                (c, kids)
            });
            result.push(copy);
            if depth == 0 {
                if let Some(tb) = target_base {
                    self.base_fixup(vm, cur, copy, tb);
                }
            }
            if !kids.is_empty() {
                self.copy_list(vm, &kids, Some(copy), depth + 1, target_base)?;
            }
        }
        Ok(result)
    }

    /// xmlXIncludeCopyXPointer
    fn copy_xpointer(
        &mut self,
        vm: &mut Vm,
        set: &[NodeId],
        target_base: Option<&str>,
    ) -> Option<Vec<NodeId>> {
        let mut list = Vec::new();
        for &n in set {
            let (ty, root) = tree::with(|t| {
                (
                    t.nodes[n].t,
                    if t.is_doc(n) { t.root_element(n) } else { None },
                )
            });
            let node = match ty {
                NType::Document | NType::HtmlDocument => match root {
                    Some(r) => r,
                    None => {
                        self.err(
                            vm,
                            Some(n),
                            XML_ERR_INTERNAL_ERROR,
                            "document without root\n".to_string(),
                        );
                        continue;
                    }
                },
                NType::Text | NType::CData | NType::Element | NType::Pi | NType::Comment => n,
                _ => {
                    self.err(
                        vm,
                        Some(n),
                        XML_XINCLUDE_XPTR_RESULT,
                        "invalid node type in XPtr result\n".to_string(),
                    );
                    continue;
                }
            };
            match self.copy_node(vm, node, false, target_base) {
                Ok(c) if !c.is_empty() => list.extend(c),
                _ => return None,
            }
        }
        Some(list)
    }

    /// xmlXIncludeParseFile
    fn parse_file(&mut self, vm: &mut Vm, url: &str) -> Option<NodeId> {
        let data = match crate::io::read_resource(vm, url) {
            Some(d) => d,
            None => {
                self.io_warning(vm, url);
                return None;
            }
        };
        let out = crate::sax2::parse_doc(&data, Some(url), self.parse_flags | XML_PARSE_DTDLOAD);
        libxml::report(vm, &self.fname, &out.errors, Style::Generic);
        if out.well_formed { out.doc } else { None }
    }

    /// xmlXIncludeMergeEntities
    fn merge_entities(&mut self, vm: &mut Vm, from: NodeId) {
        let doc = self.doc;
        let mismatches = tree::with(|t| {
            let mut mismatches = Vec::new();
            let source = match t.int_subset(from) {
                Some(s) => s,
                None => return mismatches,
            };
            let target = match t.int_subset(doc) {
                Some(d) => d,
                None => {
                    let root = match t.root_element(doc) {
                        Some(r) => r,
                        None => return mismatches,
                    };
                    let name = t.nodes[root].name.clone();
                    t.create_int_subset(Some(doc), &name, None, None)
                }
            };
            let mut sources = vec![source];
            if let Some(ext) = t.doc_info(from).and_then(|i| i.ext_subset) {
                let (te, ts) = t
                    .dtd_info(target)
                    .map(|d| (d.external_id.clone(), d.system_id.clone()))
                    .unwrap_or_default();
                let (se, ss) = t
                    .dtd_info(ext)
                    .map(|d| (d.external_id.clone(), d.system_id.clone()))
                    .unwrap_or_default();
                if te != se && ts != ss {
                    sources.push(ext);
                }
            }
            for src in sources {
                let ents: Vec<NodeId> = t
                    .dtd_info(src)
                    .map(|d| d.entities.iter().map(|(_, e)| *e).collect())
                    .unwrap_or_default();
                for e in ents {
                    let info = match t.entity_info(e) {
                        Some(i) => i.clone(),
                        None => continue,
                    };
                    use tree::EntityType::*;
                    if matches!(
                        info.etype,
                        InternalParameter | ExternalParameter | InternalPredefined
                    ) {
                        continue;
                    }
                    let name = t.nodes[e].name.clone();
                    let content = t.nodes[e].content.clone();
                    match t.get_doc_entity(doc, &name) {
                        None => {
                            t.add_entity(
                                target,
                                &name,
                                info.etype,
                                info.external_id.as_deref(),
                                info.system_id.as_deref(),
                                Some(&content),
                                info.notation.as_deref(),
                            );
                        }
                        Some(prev) => {
                            let ok = match t.entity_info(prev) {
                                None => false,
                                Some(p) => {
                                    if p.etype != info.etype {
                                        false
                                    } else if let (Some(a), Some(b)) =
                                        (&info.system_id, &p.system_id)
                                    {
                                        a == b
                                    } else if let (Some(a), Some(b)) =
                                        (&info.external_id, &p.external_id)
                                    {
                                        a == b
                                    } else {
                                        content == t.nodes[prev].content
                                    }
                                }
                            };
                            if !ok && info.etype == ExternalGeneralUnparsed {
                                mismatches.push((e, name));
                            }
                        }
                    }
                }
            }
            mismatches
        });
        for (e, name) in mismatches {
            self.err(
                vm,
                Some(e),
                XML_XINCLUDE_ENTITY_DEF_MISMATCH,
                format!("mismatch in redefinition of entity {}\n", name),
            );
        }
    }

    /// xmlXIncludeLoadDoc
    fn load_doc(&mut self, vm: &mut Vm, idx: usize) -> Result<(), ()> {
        let url = self.inc[idx].uri.clone();
        let fragment = self.inc[idx].fragment.clone();
        let elem = self.inc[idx].elem;
        let doc_url = tree::with(|t| t.doc_url(self.doc));
        let doc;
        if url.is_empty() || url.starts_with('#') || doc_url.as_deref() == Some(url.as_str()) {
            doc = self.doc;
        } else if let Some(i) = self.urls.iter().position(|c| c.url == url) {
            if self.urls[i].expanding {
                self.err(
                    vm,
                    Some(elem),
                    XML_XINCLUDE_RECURSION,
                    "inclusion loop detected\n".to_string(),
                );
                return Err(());
            }
            doc = self.urls[i].doc.ok_or(())?;
        } else {
            let save = self.parse_flags;
            if fragment.is_some() {
                self.parse_flags |= XML_PARSE_NOENT;
            }
            let d = self.parse_file(vm, &url);
            self.parse_flags = save;
            self.urls.push(DocCache {
                doc: d,
                url: url.clone(),
                expanding: false,
            });
            let cache = self.urls.len() - 1;
            let d = d.ok_or(())?;
            self.merge_entities(vm, d);
            self.urls[cache].expanding = true;
            self.recurse_doc(vm, d);
            self.urls[cache].expanding = false;
            doc = d;
        }
        match fragment {
            None => {
                let root = match tree::with(|t| t.root_element(doc)) {
                    Some(r) => r,
                    None => {
                        self.err(
                            vm,
                            Some(elem),
                            XML_ERR_INTERNAL_ERROR,
                            "document without root\n".to_string(),
                        );
                        return Err(());
                    }
                };
                let target = self.doc;
                let copy = tree::with(|t| t.copy_node(root, Some(target), 1, None));
                self.inc[idx].inc = Some(vec![copy]);
                if let Some(b) = self.inc[idx].base.clone() {
                    self.base_fixup(vm, root, copy, &b);
                }
            }
            Some(frag) => {
                let mut ns = std::mem::take(&mut self.xp_ns);
                let fname = self.fname.clone();
                let r = xptr_eval(vm, &fname, &frag, doc, &mut ns);
                self.xp_ns = ns;
                let set = match r {
                    Err(()) => {
                        self.err(
                            vm,
                            Some(elem),
                            XML_XINCLUDE_XPTR_FAILED,
                            format!("XPointer evaluation failed: #{}\n", frag),
                        );
                        return Err(());
                    }
                    Ok(None) => return Ok(()),
                    Ok(Some(s)) => s,
                };
                let mut kept = Vec::new();
                for n in set {
                    let ty = tree::with(|t| t.nodes[n].t);
                    match ty {
                        NType::Element
                        | NType::Text
                        | NType::CData
                        | NType::EntityRef
                        | NType::Entity
                        | NType::Pi
                        | NType::Comment
                        | NType::Document
                        | NType::HtmlDocument => kept.push(n),
                        NType::Attribute => self.err(
                            vm,
                            Some(elem),
                            XML_XINCLUDE_XPTR_RESULT,
                            format!("XPointer selects an attribute: #{}\n", frag),
                        ),
                        NType::NamespaceDecl => self.err(
                            vm,
                            Some(elem),
                            XML_XINCLUDE_XPTR_RESULT,
                            format!("XPointer selects a namespace: #{}\n", frag),
                        ),
                        _ => self.err(
                            vm,
                            Some(elem),
                            XML_XINCLUDE_XPTR_RESULT,
                            format!("XPointer selects unexpected nodes: #{}\n", frag),
                        ),
                    }
                }
                let base = self.inc[idx].base.clone();
                self.inc[idx].inc = self.copy_xpointer(vm, &kept, base.as_deref());
            }
        }
        Ok(())
    }

    /// xmlXIncludeLoadTxt
    fn load_txt(&mut self, vm: &mut Vm, idx: usize) -> Result<(), ()> {
        let url = self.inc[idx].uri.clone();
        let elem = self.inc[idx].elem;
        let doc = self.doc;
        if url.is_empty() {
            self.err(
                vm,
                Some(elem),
                XML_XINCLUDE_TEXT_DOCUMENT,
                "text serialization of document not available\n".to_string(),
            );
            return Err(());
        }
        if let Some((text, _)) = self.txt.iter().find(|(_, u)| *u == url) {
            let text = text.clone();
            let node = tree::with(|t| t.new_text(Some(doc), &text));
            self.inc[idx].inc = Some(vec![node]);
            return Ok(());
        }
        let encoding = tree::with(|t| self.get_prop(t, elem, "encoding"));
        if let Some(enc) = &encoding {
            if !crate::parser::encoding_supported(enc) {
                self.err(
                    vm,
                    Some(elem),
                    XML_XINCLUDE_UNKNOWN_ENCODING,
                    format!("encoding {} not supported\n", enc),
                );
                return Err(());
            }
        }
        let data = match crate::io::read_resource(vm, &url) {
            Some(d) => d,
            None => {
                self.io_warning(vm, &url);
                return Err(());
            }
        };
        let data = match &encoding {
            Some(enc) => crate::parser::decode_to_utf8(&data, enc).unwrap_or_default(),
            None => data,
        };
        let text = match std::str::from_utf8(&data) {
            Ok(s) if s.chars().all(|c| is_char(c as u32)) => s.to_string(),
            _ => {
                self.err(
                    vm,
                    Some(elem),
                    XML_XINCLUDE_INVALID_CHAR,
                    format!("{} contains invalid char\n", url),
                );
                return Err(());
            }
        };
        let node = tree::with(|t| t.new_text(Some(doc), &text));
        self.txt.push((text, url));
        self.inc[idx].inc = Some(vec![node]);
        Ok(())
    }

    /// xmlXIncludeLoadFallback
    fn load_fallback(&mut self, vm: &mut Vm, fallback: NodeId, idx: usize) -> Result<(), ()> {
        let mut ret = Ok(());
        if tree::with(|t| t.nodes[fallback].children.is_some()) {
            let old = self.nb_errors;
            let base = self.inc[idx].base.clone();
            let r = self.copy_node(vm, fallback, true, base.as_deref());
            self.inc[idx].inc = r.ok().filter(|v| !v.is_empty());
            if self.nb_errors > old {
                ret = Err(());
            }
        } else {
            self.inc[idx].inc = None;
        }
        self.inc[idx].fallback = true;
        ret
    }

    /// xmlXIncludeExpandNode
    fn expand_node(&mut self, vm: &mut Vm, node: NodeId) -> Option<usize> {
        if self.fatal {
            return None;
        }
        if self.depth >= XINCLUDE_MAX_DEPTH {
            self.err(
                vm,
                Some(node),
                XML_XINCLUDE_RECURSION,
                "maximum recursion depth exceeded\n".to_string(),
            );
            self.fatal = true;
            return None;
        }
        if let Some(i) = self.inc.iter().position(|r| r.elem == node) {
            if self.inc[i].expanding {
                self.err(
                    vm,
                    Some(node),
                    XML_XINCLUDE_RECURSION,
                    "inclusion loop detected\n".to_string(),
                );
                return None;
            }
            return Some(i);
        }
        let idx = self.add_node(vm, node)?;
        self.inc[idx].expanding = true;
        self.depth += 1;
        self.load_node(vm, idx);
        self.depth -= 1;
        self.inc[idx].expanding = false;
        Some(idx)
    }

    /// xmlXIncludeLoadNode
    fn load_node(&mut self, vm: &mut Vm, idx: usize) {
        let elem = self.inc[idx].elem;
        let mut ret = if self.inc[idx].xml {
            self.load_doc(vm, idx)
        } else {
            self.load_txt(vm, idx)
        };
        if ret.is_err() {
            let fb = tree::with(|t| {
                t.children_of(elem)
                    .into_iter()
                    .find(|&c| is_xi(t, c, "fallback"))
            });
            if let Some(fb) = fb {
                ret = self.load_fallback(vm, fb, idx);
            }
        }
        if ret.is_err() {
            let uri = self.inc[idx].uri.clone();
            self.err(
                vm,
                Some(elem),
                XML_XINCLUDE_NO_FALLBACK,
                format!("could not load {}, and no fallback was found\n", uri),
            );
        }
    }

    /// xmlXIncludeIncludeNode
    fn include_node(&mut self, vm: &mut Vm, idx: usize) {
        let cur = self.inc[idx].elem;
        let list = self.inc[idx].inc.take().unwrap_or_default();
        let multi = tree::with(|t| match t.nodes[cur].parent {
            Some(p) if t.nodes[p].t != NType::Element => {
                list.iter()
                    .filter(|&&n| t.nodes[n].t == NType::Element)
                    .count()
                    > 1
            }
            _ => false,
        });
        if multi {
            self.err(
                vm,
                Some(cur),
                XML_XINCLUDE_MULTIPLE_ROOT,
                "XInclude error: would result in multiple root nodes\n".to_string(),
            );
            return;
        }
        // XML_PARSE_NOXINCNODE: replace the include by the nodes
        tree::with(|t| {
            let doc = t.nodes[cur].doc;
            for n in list {
                if t.nodes[n].doc != doc {
                    t.set_tree_doc(n, doc);
                }
                t.insert_before_raw(cur, n);
            }
            t.unlink(cur);
        });
    }

    /// xmlXIncludeTestNode
    fn test_node(&mut self, vm: &mut Vm, node: NodeId) -> bool {
        enum R {
            No,
            Include,
            IncludeInInclude,
            MultipleFallbacks,
            FallbackNotInInclude,
        }
        let r = tree::with(|t| {
            let nd = &t.nodes[node];
            if nd.t != NType::Element || nd.ns.is_none() {
                return (R::No, false);
            }
            let href = t.ns_href(nd.ns);
            if !is_xinclude_ns(href) {
                return (R::No, false);
            }
            let old = href == Some(XINCLUDE_OLD_NS);
            if nd.name == "include" {
                let mut nb_fallback = 0;
                for c in t.children_of(node) {
                    let cn = &t.nodes[c];
                    if cn.t == NType::Element && is_xinclude_ns(t.ns_href(cn.ns)) {
                        if cn.name == "include" {
                            return (R::IncludeInInclude, old);
                        }
                        if cn.name == "fallback" {
                            nb_fallback += 1;
                        }
                    }
                }
                if nb_fallback > 1 {
                    return (R::MultipleFallbacks, old);
                }
                return (R::Include, old);
            }
            if nd.name == "fallback" {
                let ok = nd.parent.map(|p| is_xi(t, p, "include")).unwrap_or(false);
                if !ok {
                    return (R::FallbackNotInInclude, old);
                }
            }
            (R::No, old)
        });
        if r.1 {
            self.legacy = true;
        }
        match r.0 {
            R::No => false,
            R::Include => true,
            R::IncludeInInclude => {
                self.err(
                    vm,
                    Some(node),
                    XML_XINCLUDE_INCLUDE_IN_INCLUDE,
                    "include has an 'include' child\n".to_string(),
                );
                false
            }
            R::MultipleFallbacks => {
                self.err(
                    vm,
                    Some(node),
                    XML_XINCLUDE_FALLBACKS_IN_INCLUDE,
                    "include has multiple fallback children\n".to_string(),
                );
                false
            }
            R::FallbackNotInInclude => {
                self.err(
                    vm,
                    Some(node),
                    XML_XINCLUDE_FALLBACK_NOT_IN_INCLUDE,
                    "fallback is not the child of an 'include'\n".to_string(),
                );
                false
            }
        }
    }

    /// xmlXIncludeDoProcess
    fn do_process(&mut self, vm: &mut Vm, tree_root: NodeId) -> i64 {
        let start = self.inc.len();
        // first phase: lookup the elements in the document
        let mut cur = Some(tree_root);
        while let Some(c) = cur {
            let mut descended = false;
            if self.test_node(vm, c) {
                if let Some(i) = self.expand_node(vm, c) {
                    self.inc[i].replace = true;
                }
            } else {
                let first = tree::with(|t| {
                    if matches!(t.nodes[c].t, NType::Document | NType::Element) {
                        t.nodes[c].children
                    } else {
                        None
                    }
                });
                if let Some(f) = first {
                    cur = Some(f);
                    descended = true;
                }
            }
            if !descended {
                let mut x = c;
                cur = tree::with(|t| {
                    loop {
                        if x == tree_root {
                            break Some(tree_root);
                        }
                        if let Some(n) = t.nodes[x].next {
                            break Some(n);
                        }
                        match t.nodes[x].parent {
                            Some(p) => x = p,
                            None => break None,
                        }
                    }
                });
            }
            if cur == Some(tree_root) {
                break;
            }
        }
        // second phase: extend the original document infoset
        let mut ret = 0;
        for i in start..self.inc.len() {
            if self.inc[i].replace {
                self.include_node(vm, i);
                self.inc[i].replace = false;
            } else {
                // includes added indirectly (e.g. inside xi:fallback)
                self.inc[i].inc = None;
            }
            ret += 1;
        }
        ret
    }
}

/// xmlXIncludeProcessFlags(doc, flags): the number of substitutions, or
/// -1 on error. Messages are reported as warnings of `fname`.
pub fn process(vm: &mut Vm, fname: &str, doc: NodeId, flags: i64) -> i64 {
    let root = match tree::with(|t| t.root_element(doc)) {
        Some(r) => r,
        None => return -1,
    };
    let mut ctx = Ctx {
        fname: fname.to_string(),
        doc,
        inc: Vec::new(),
        txt: Vec::new(),
        urls: Vec::new(),
        nb_errors: 0,
        fatal: false,
        legacy: false,
        parse_flags: flags,
        depth: 0,
        xp_ns: Vec::new(),
    };
    let ret = ctx.do_process(vm, root);
    if ret >= 0 && ctx.nb_errors > 0 {
        -1
    } else {
        ret
    }
}
