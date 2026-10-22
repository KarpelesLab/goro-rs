//! Tree helpers (libxml2 / xsltutils.c equivalents) and the error channel
//! (libxslt's generic error function as installed by ext/xsl).

use goro_core::vm::Vm;
use goro_ext_xml::tree::{self, NType, NodeId, NsId, Tree, XML_XML_NAMESPACE};
use std::cell::RefCell;

pub const XSLT_NAMESPACE: &str = "http://www.w3.org/1999/XSL/Transform";
pub const XSLT_DEFAULT_VERSION: &str = "1.0";
pub const XSLT_DEFAULT_VENDOR: &str = "libxslt";
pub const XSLT_DEFAULT_URL: &str = "http://xmlsoft.org/XSLT/";

pub fn tw<R>(f: impl FnOnce(&mut Tree) -> R) -> R {
    tree::with(f)
}

// ---------------------------------------------------------------------------
// Error output

thread_local! {
    /// LIBXML(error_buffer): message parts not ended by a newline yet.
    static BUF: RefCell<String> = const { RefCell::new(String::new()) };
}

/// php_libxml_internal_error_handler: accumulate `msg`; once it ends with a
/// newline the buffered text becomes one warning (or libxml error entry).
pub fn php_error(vm: &mut Vm, msg: &str) {
    let mut s = msg;
    let mut output = false;
    while let Some(x) = s.strip_suffix('\n') {
        s = x;
        output = true;
    }
    BUF.with(|b| b.borrow_mut().push_str(s));
    if output {
        let text = BUF.with(|b| std::mem::take(&mut *b.borrow_mut()));
        // the buffer is used as a C string
        let text = match text.find('\0') {
            Some(i) => text[..i].to_string(),
            None => text,
        };
        let f = goro_ext_xml::dom::active_fn(vm);
        if goro_ext_xml::libxml::use_internal_errors() {
            goro_ext_xml::libxml::issue_error(vm, &f, &text);
        } else if vm.current_exception.is_none() {
            vm.emit_warning(&format!("{}(): {}", f, text));
        }
    }
}

/// xsl_libxslt_error_handler with a "%s" format: PHP-specific wording.
pub fn php_error_s(vm: &mut Vm, msg: &str) {
    if let Some(i) = msg.find("xsltMaxDepth (--maxdepth)") {
        let r = format!(
            "{}$maxTemplateDepth{}",
            &msg[..i],
            &msg[i + "xsltMaxDepth (--maxdepth)".len()..]
        );
        php_error(vm, &r);
    } else if let Some(i) = msg.find("maxTemplateVars (--maxvars)") {
        let r = format!(
            "{}$maxTemplateVars{}",
            &msg[..i],
            &msg[i + "maxTemplateVars (--maxvars)".len()..]
        );
        php_error(vm, &r);
    } else {
        php_error(vm, msg);
    }
}

/// xsltPrintErrorContext + message (xsltTransformError). `runtime` selects
/// "runtime error" (a transformation is running) over "compilation error".
pub fn transform_error(
    vm: &mut Vm,
    runtime: bool,
    has_style: bool,
    node: Option<NodeId>,
    msg: &str,
) {
    let ty = if runtime {
        "runtime error"
    } else if has_style {
        "compilation error"
    } else {
        "error"
    };
    let (file, line, name) = match node {
        None => (None, 0, None),
        Some(n) => tw(|t| {
            let nd = &t.nodes[n];
            if matches!(nd.t, NType::Document | NType::HtmlDocument) {
                (t.doc_info(n).and_then(|d| d.url.clone()), 0, None)
            } else {
                let file = nd
                    .doc
                    .and_then(|d| t.doc_info(d).and_then(|i| i.url.clone()));
                let line = get_line_no(t, n);
                let name = if nd.name.is_empty() {
                    None
                } else {
                    Some(nd.name.clone())
                };
                (file, line, name)
            }
        }),
    };
    let ctxline = match (&file, line, &name) {
        (Some(f), l, Some(n)) if l != 0 => format!("{}: file {} line {} element {}\n", ty, f, l, n),
        (Some(f), _, Some(n)) => format!("{}: file {} element {}\n", ty, f, n),
        (Some(f), l, None) if l != 0 => format!("{}: file {} line {}\n", ty, f, l),
        (Some(f), _, None) => format!("{}: file {}\n", ty, f),
        (None, _, Some(n)) => format!("{}: element {}\n", ty, n),
        (None, _, None) => format!("{}\n", ty),
    };
    php_error(vm, &ctxline);
    php_error_s(vm, msg);
}

/// xmlGetLineNo
pub fn get_line_no(t: &Tree, n: NodeId) -> u32 {
    let nd = &t.nodes[n];
    match nd.t {
        NType::Attribute | NType::NamespaceDecl => nd.parent.map(|p| t.nodes[p].line).unwrap_or(0),
        _ => nd.line,
    }
}

// ---------------------------------------------------------------------------
// Node helpers

pub fn is_xslt_elem(t: &Tree, n: NodeId) -> bool {
    let nd = &t.nodes[n];
    nd.t == NType::Element && t.ns_href(nd.ns) == Some(XSLT_NAMESPACE)
}

pub fn is_xslt_name(t: &Tree, n: NodeId, name: &str) -> bool {
    t.nodes[n].name == name
}

pub fn is_xslt_elem_named(t: &Tree, n: NodeId, name: &str) -> bool {
    is_xslt_elem(t, n) && t.nodes[n].name == name
}

/// IS_XSLT_REAL_NODE
pub fn is_real_node(t: &Tree, n: NodeId) -> bool {
    let nd = &t.nodes[n];
    match nd.t {
        NType::Element => !nd.name.starts_with(' '),
        NType::Text | NType::CData | NType::Pi | NType::Comment | NType::Attribute => true,
        _ => false,
    }
}

/// xsltIsBlank
pub fn is_blank(s: &str) -> bool {
    s.bytes()
        .all(|c| c == 0x20 || c == 0x09 || c == 0x0A || c == 0x0D)
}

pub fn is_blank_node(t: &Tree, n: NodeId) -> bool {
    t.nodes[n].t == NType::Text && is_blank(&t.nodes[n].content)
}

/// xmlGetNsProp(node, name, ns): attribute `name` in namespace `ns`
/// (None: no namespace).
pub fn get_ns_prop(t: &Tree, n: NodeId, name: &str, ns: Option<&str>) -> Option<String> {
    t.has_ns_prop(n, name, ns).map(|a| t.node_get_content(a))
}

/// xmlGetProp: the first attribute named `name`, whatever its namespace.
pub fn get_prop(t: &Tree, n: NodeId, name: &str) -> Option<String> {
    t.has_prop(n, name).map(|a| t.node_get_content(a))
}

/// xsltGetNsProp / xsltGetCNsProp: an attribute of an XSLT element, in no
/// namespace (when the element is in `ns`) or in `ns`.
pub fn xslt_ns_prop(t: &Tree, n: NodeId, name: &str, ns: Option<&str>) -> Option<String> {
    let ns = match ns {
        None => return get_prop(t, n, name),
        Some(x) => x,
    };
    let nd = &t.nodes[n];
    if nd.t != NType::Element {
        return None;
    }
    let mut a = nd.properties;
    while let Some(x) = a {
        let at = &t.nodes[x];
        if at.name == name {
            let ok = match at.ns {
                None => nd.ns.is_some() && t.ns_href(nd.ns) == Some(ns),
                Some(_) => t.ns_href(at.ns) == Some(ns),
            };
            if ok {
                return Some(t.node_get_content(x));
            }
        }
        a = at.next;
    }
    None
}

/// xmlGetNsList: the namespaces in scope of `n` (nearest first, shadowed
/// ones skipped).
pub fn ns_list(t: &Tree, n: NodeId) -> Vec<NsId> {
    let mut out: Vec<NsId> = Vec::new();
    let mut cur = Some(n);
    while let Some(c) = cur {
        if t.nodes[c].t == NType::Element {
            for &ns in &t.nodes[c].ns_def {
                let p = &t.nss[ns].prefix;
                if !out.iter().any(|&o| t.nss[o].prefix == *p) {
                    out.push(ns);
                }
            }
        }
        cur = t.nodes[c].parent;
    }
    out
}

/// The in-scope namespace list as used by XPath (prefix -> href).
pub fn xpath_ns_list(t: &Tree, n: NodeId) -> Vec<(String, String)> {
    let mut v = Vec::new();
    for ns in ns_list(t, n) {
        if let Some(p) = &t.nss[ns].prefix {
            v.push((p.clone(), t.nss[ns].href.clone()));
        }
    }
    v
}

/// xsltSplitQName: (prefix, local)
pub fn split_qname(s: &str) -> (Option<String>, String) {
    if s.starts_with(':') {
        return (None, s.to_string());
    }
    match s.find(':') {
        None => (None, s.to_string()),
        Some(i) => (Some(s[..i].to_string()), s[i + 1..].to_string()),
    }
}

/// xmlSplitQName2: Some((prefix, local)) only for a prefixed name.
pub fn split_qname2(s: &str) -> Option<(String, String)> {
    if s.starts_with(':') {
        return None;
    }
    let i = s.find(':')?;
    if i + 1 >= s.len() {
        return None;
    }
    Some((s[..i].to_string(), s[i + 1..].to_string()))
}

/// xmlValidateQName (space not allowed): true when valid.
pub fn valid_qname(s: &str) -> bool {
    goro_ext_xml::parser::is_valid_qname(s)
}

pub fn valid_ncname(s: &str) -> bool {
    goro_ext_xml::parser::is_valid_ncname(s)
}

pub fn search_ns(t: &mut Tree, n: NodeId, prefix: Option<&str>) -> Option<NsId> {
    t.search_ns(n, prefix)
}

/// The URI part of xsltGetQNameURI: Ok((uri, local)) or Err(()) after
/// reporting "no namespace bound".
pub fn get_qname_uri(
    vm: &mut Vm,
    node: NodeId,
    qname: &str,
) -> Result<(Option<String>, String), ()> {
    if qname.is_empty() {
        return Ok((None, String::new()));
    }
    if qname.starts_with(':') {
        return Ok((None, qname.to_string()));
    }
    let i = match qname.find(':') {
        None => return Ok((None, qname.to_string())),
        Some(i) => i,
    };
    if let Some(rest) = qname.strip_prefix("xml:") {
        if rest.is_empty() {
            return Ok((None, qname.to_string()));
        }
        return Ok((Some(XML_XML_NAMESPACE.to_string()), rest.to_string()));
    }
    let prefix = &qname[..i];
    let local = &qname[i + 1..];
    let ns = tw(|t| {
        t.search_ns(node, Some(prefix))
            .map(|n| t.nss[n].href.clone())
    });
    match ns {
        None => {
            php_error(
                vm,
                &format!(
                    "{}:{} : no namespace bound to prefix {}\n",
                    prefix, local, prefix
                ),
            );
            Err(())
        }
        Some(h) => Ok((Some(h), local.to_string())),
    }
}

/// xmlBuildURI
pub fn build_uri(uri: &str, base: Option<&str>) -> Option<String> {
    goro_ext_xml::uri::build_uri(uri, base)
}

/// xmlNodeGetBase
pub fn node_get_base(t: &Tree, n: NodeId) -> Option<String> {
    goro_ext_xml::dom::node::node_get_base(t, n)
}

pub fn doc_url(t: &Tree, doc: NodeId) -> Option<String> {
    t.doc_info(doc).and_then(|d| d.url.clone())
}

/// The root of a node's tree.
pub fn tree_root(t: &Tree, mut n: NodeId) -> NodeId {
    while let Some(p) = t.nodes[n].parent {
        n = p;
    }
    n
}

/// xmlNodeGetSpacePreserve: 1 preserve, 0 default, -1 unspecified
pub fn get_space_preserve(t: &Tree, n: NodeId) -> i32 {
    let mut cur = Some(n);
    while let Some(c) = cur {
        if t.nodes[c].t == NType::Element {
            if let Some(a) = t.has_ns_prop(c, "space", Some(XML_XML_NAMESPACE)) {
                let v = t.node_get_content(a);
                if v == "preserve" {
                    return 1;
                }
                if v == "default" {
                    return 0;
                }
            }
        }
        cur = t.nodes[c].parent;
    }
    -1
}
