//! XML canonicalization (libxml2's c14n.c): Canonical XML 1.0 and
//! Exclusive XML Canonicalization 1.0, with or without comments, over a
//! whole document, a subtree (PHP's parent lookup callback) or an XPath
//! node-set. Plus the PHP glue of DOMNode::C14N() (ext/dom/node.c).

use crate::dom::{throw_error, type_name_of};
use crate::parser::XmlError;
use crate::tree::{self, F_NULL_CONTENT, NType, NodeId, NsId, Tree, XML_XML_NAMESPACE};
use crate::xpath::{self, XValue};
use goro_core::value::Value;
use goro_core::vm::Vm;
use std::collections::HashSet;

const XML_FROM_C14N: i32 = 21;
const XML_ERR_INVALID_URI: i32 = 91;
const XML_C14N_INVALID_NODE: i32 = 1953;
const XML_C14N_RELATIVE_NAMESPACE: i32 = 1955;

/// Which nodes are rendered (xmlC14NIsVisibleCallback).
enum Vis {
    /// No callback: everything is visible.
    All,
    /// dom_canonicalize_node_parent_lookup_cb: the subtree of a node.
    Subtree(NodeId),
    /// xmlC14NIsNodeInNodeset: an XPath node-set. Namespace nodes are
    /// matched by (element, prefix) like xmlXPathNodeSetContains.
    Set {
        nodes: HashSet<NodeId>,
        ns: HashSet<(NodeId, String)>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pos {
    Before,
    Inside,
    After,
}

/// xmlC14NVisibleNsStack: namespaces rendered by output ancestors.
#[derive(Default)]
struct NsStack {
    tab: Vec<(NsId, NodeId)>,
    prev_start: usize,
    prev_end: usize,
}

struct Ctx<'a> {
    t: &'a Tree,
    vis: &'a Vis,
    with_comments: bool,
    exclusive: bool,
    inclusive_prefixes: Option<&'a [String]>,
    out: String,
    pos: Pos,
    parent_is_doc: bool,
    stack: NsStack,
    errors: Vec<XmlError>,
}

fn c14n_err(code: i32, msg: String) -> XmlError {
    XmlError::simple(XML_FROM_C14N, code, crate::parser::ERR_ERROR, msg)
}

/// xmlOutputBufferWriteQuotedString
fn write_quoted(out: &mut String, s: &str) {
    if s.contains('"') {
        if s.contains('\'') {
            out.push('"');
            out.push_str(&s.replace('"', "&quot;"));
            out.push('"');
        } else {
            out.push('\'');
            out.push_str(s);
            out.push('\'');
        }
    } else {
        out.push('"');
        out.push_str(s);
        out.push('"');
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Norm {
    Attr,
    Comment,
    Pi,
    Text,
}

/// xmlC11NNormalizeString
fn normalize(out: &mut String, s: &str, mode: Norm) {
    for c in s.chars() {
        match c {
            '<' if matches!(mode, Norm::Attr | Norm::Text) => out.push_str("&lt;"),
            '>' if mode == Norm::Text => out.push_str("&gt;"),
            '&' if matches!(mode, Norm::Attr | Norm::Text) => out.push_str("&amp;"),
            '"' if mode == Norm::Attr => out.push_str("&quot;"),
            '\t' if mode == Norm::Attr => out.push_str("&#x9;"),
            '\n' if mode == Norm::Attr => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            c => out.push(c),
        }
    }
}

/// xmlC14NStrEqual: NULL and "" compare equal.
fn str_eq(a: Option<&str>, b: Option<&str>) -> bool {
    a.unwrap_or("") == b.unwrap_or("")
}

/// xmlStrcmp with NULL sorting first.
fn strcmp(a: Option<&str>, b: Option<&str>) -> std::cmp::Ordering {
    match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(x), Some(y)) => x.as_bytes().cmp(y.as_bytes()),
    }
}

/// xmlListInsert: insert before the first element not lower than `v`.
fn list_insert<T: Copy>(list: &mut Vec<T>, v: T, cmp: impl Fn(T, T) -> std::cmp::Ordering) {
    let pos = list
        .iter()
        .position(|&x| cmp(x, v) != std::cmp::Ordering::Less)
        .unwrap_or(list.len());
    list.insert(pos, v);
}

impl<'a> Ctx<'a> {
    fn prefix(&self, ns: NsId) -> Option<&'a str> {
        self.t.nss[ns].prefix.as_deref()
    }

    fn href(&self, ns: NsId) -> &'a str {
        self.t.nss[ns].href.as_str()
    }

    /// xmlC14NIsXmlNs
    fn is_xml_ns(&self, ns: NsId) -> bool {
        self.prefix(ns) == Some("xml") && self.href(ns) == XML_XML_NAMESPACE
    }

    /// xmlSearchNs (the `xml` prefix maps to the document's implicit
    /// namespace, which is never one of the declarations).
    fn search_ns(&self, node: NodeId, prefix: Option<&str>) -> Option<NsId> {
        if prefix == Some("xml") {
            let doc = self.t.nodes[node].doc?;
            return self.t.doc_info(doc).and_then(|d| d.xml_ns);
        }
        let mut cur = Some(node);
        while let Some(c) = cur {
            let n = &self.t.nodes[c];
            if matches!(n.t, NType::EntityRef | NType::EntityDecl | NType::Entity) {
                return None;
            }
            if n.t == NType::Element {
                for &ns in &n.ns_def {
                    if self.prefix(ns) == prefix {
                        return Some(ns);
                    }
                }
                if c != node {
                    if let Some(ns) = n.ns {
                        if self.prefix(ns) == prefix {
                            return Some(ns);
                        }
                    }
                }
            }
            cur = n.parent;
        }
        None
    }

    fn in_subtree(&self, root: NodeId, from: Option<NodeId>) -> bool {
        let mut cur = from;
        while let Some(c) = cur {
            if c == root {
                return true;
            }
            cur = self.t.nodes[c].parent;
        }
        false
    }

    /// xmlC14NIsVisible for a tree node.
    fn visible(&self, node: NodeId, parent: Option<NodeId>) -> bool {
        match self.vis {
            Vis::All => true,
            Vis::Subtree(root) => node == *root || self.in_subtree(*root, parent),
            Vis::Set { nodes, .. } => nodes.contains(&node),
        }
    }

    /// xmlC14NIsVisible for a namespace (declaration) seen from `parent`.
    fn ns_visible(&self, ns: NsId, parent: NodeId) -> bool {
        match self.vis {
            Vis::All => true,
            Vis::Subtree(root) => self.in_subtree(*root, Some(parent)),
            Vis::Set { ns: set, .. } => {
                let parent = if self.t.nodes[parent].t == NType::Attribute {
                    self.t.nodes[parent].parent.unwrap_or(parent)
                } else {
                    parent
                };
                set.contains(&(parent, self.prefix(ns).unwrap_or("").to_string()))
            }
        }
    }

    // ---- rendered namespaces stack

    fn stack_add(&mut self, ns: NsId, node: NodeId) {
        self.stack.tab.push((ns, node));
    }

    fn stack_save(&self) -> (usize, usize, usize) {
        (
            self.stack.tab.len(),
            self.stack.prev_start,
            self.stack.prev_end,
        )
    }

    fn stack_restore(&mut self, s: (usize, usize, usize)) {
        self.stack.tab.truncate(s.0);
        self.stack.prev_start = s.1;
        self.stack.prev_end = s.2;
    }

    fn stack_shift(&mut self) {
        self.stack.prev_start = self.stack.prev_end;
        self.stack.prev_end = self.stack.tab.len();
    }

    /// xmlC14NVisibleNsStackFind (`None` is the empty default namespace).
    fn stack_find(&self, ns: Option<NsId>) -> bool {
        let prefix = ns.and_then(|n| self.prefix(n)).unwrap_or("");
        let href = ns.map(|n| self.href(n)).unwrap_or("");
        let has_empty_ns = prefix.is_empty() && href.is_empty();
        let start = if has_empty_ns {
            0
        } else {
            self.stack.prev_start
        };
        for i in (start..self.stack.tab.len()).rev() {
            let ns1 = self.stack.tab[i].0;
            if str_eq(Some(prefix), self.prefix(ns1)) {
                return href == self.href(ns1);
            }
        }
        has_empty_ns
    }

    /// xmlExcC14NVisibleNsStackFind
    fn exc_stack_find(&self, ns: Option<NsId>) -> bool {
        let prefix = ns.and_then(|n| self.prefix(n)).unwrap_or("");
        let href = ns.map(|n| self.href(n)).unwrap_or("");
        let has_empty_ns = prefix.is_empty() && href.is_empty();
        for i in (0..self.stack.tab.len()).rev() {
            let (ns1, node) = self.stack.tab[i];
            if str_eq(Some(prefix), self.prefix(ns1)) {
                if href == self.href(ns1) {
                    return self.ns_visible(ns1, node);
                }
                return false;
            }
        }
        has_empty_ns
    }

    // ---- output

    /// xmlC14NPrintNamespaces (`None` prints xmlns="").
    fn print_ns(&mut self, ns: Option<NsId>) {
        match ns.and_then(|n| self.prefix(n)) {
            Some(p) => {
                self.out.push_str(" xmlns:");
                self.out.push_str(p);
                self.out.push('=');
            }
            None => self.out.push_str(" xmlns="),
        }
        match ns {
            Some(n) => {
                let h = self.href(n);
                write_quoted(&mut self.out, h);
            }
            None => self.out.push_str("\"\""),
        }
    }

    fn ns_cmp(&self) -> impl Fn(NsId, NsId) -> std::cmp::Ordering + 'a {
        let t = self.t;
        move |a, b| {
            if a == b {
                std::cmp::Ordering::Equal
            } else {
                strcmp(t.nss[a].prefix.as_deref(), t.nss[b].prefix.as_deref())
            }
        }
    }

    /// xmlC14NProcessNamespacesAxis
    fn namespaces_axis(&mut self, cur: NodeId, visible: bool) {
        let mut list: Vec<NsId> = Vec::new();
        let mut has_empty_ns = false;
        let cmp = self.ns_cmp();
        let t = self.t;
        let mut n = Some(cur);
        while let Some(nn) = n {
            for &ns in &t.nodes[nn].ns_def {
                let tmp = self.search_ns(cur, self.prefix(ns));
                if tmp == Some(ns) && !self.is_xml_ns(ns) && self.ns_visible(ns, cur) {
                    let already = self.stack_find(Some(ns));
                    if visible {
                        self.stack_add(ns, cur);
                    }
                    if !already {
                        list_insert(&mut list, ns, &cmp);
                    }
                    if self.prefix(ns).unwrap_or("").is_empty() {
                        has_empty_ns = true;
                    }
                }
            }
            n = t.nodes[nn].parent;
        }
        if visible && !has_empty_ns && !self.stack_find(None) {
            self.print_ns(None);
        }
        for ns in list {
            self.print_ns(Some(ns));
        }
    }

    /// xmlExcC14NProcessNamespacesAxis
    fn exc_namespaces_axis(&mut self, cur: NodeId, visible: bool) {
        let mut list: Vec<NsId> = Vec::new();
        let mut has_empty_ns = false;
        let mut has_visibly_utilized_empty_ns = false;
        let mut has_empty_ns_in_inclusive_list = false;
        let cmp = self.ns_cmp();
        let t = self.t;
        if let Some(prefixes) = self.inclusive_prefixes {
            for p in prefixes {
                let prefix = if p == "#default" || p.is_empty() {
                    has_empty_ns_in_inclusive_list = true;
                    None
                } else {
                    Some(p.as_str())
                };
                if let Some(ns) = self.search_ns(cur, prefix) {
                    if !self.is_xml_ns(ns) && self.ns_visible(ns, cur) {
                        let already = self.stack_find(Some(ns));
                        if visible {
                            self.stack_add(ns, cur);
                        }
                        if !already {
                            list_insert(&mut list, ns, &cmp);
                        }
                        if self.prefix(ns).unwrap_or("").is_empty() {
                            has_empty_ns = true;
                        }
                    }
                }
            }
        }
        // the element's own namespace
        let ns = match t.nodes[cur].ns {
            Some(ns) => Some(ns),
            None => {
                has_visibly_utilized_empty_ns = true;
                self.search_ns(cur, None)
            }
        };
        if let Some(ns) = ns.filter(|&ns| !self.is_xml_ns(ns)) {
            if visible && self.ns_visible(ns, cur) && !self.exc_stack_find(Some(ns)) {
                list_insert(&mut list, ns, &cmp);
            }
            if visible {
                self.stack_add(ns, cur);
            }
            if self.prefix(ns).unwrap_or("").is_empty() {
                has_empty_ns = true;
            }
        }
        // namespaces of the attributes
        let mut a = t.nodes[cur].properties;
        while let Some(attr) = a {
            let ans = t.nodes[attr].ns;
            match ans {
                Some(ans) if !self.is_xml_ns(ans) && self.visible(attr, Some(cur)) => {
                    let already = self.exc_stack_find(Some(ans));
                    self.stack_add(ans, cur);
                    if !already && visible {
                        list_insert(&mut list, ans, &cmp);
                    }
                    if self.prefix(ans).unwrap_or("").is_empty() {
                        has_empty_ns = true;
                    }
                }
                Some(ans)
                    if self.prefix(ans).unwrap_or("").is_empty() && self.href(ans).is_empty() =>
                {
                    has_visibly_utilized_empty_ns = true;
                }
                _ => {}
            }
            a = t.nodes[attr].next;
        }
        if visible
            && has_visibly_utilized_empty_ns
            && !has_empty_ns
            && !has_empty_ns_in_inclusive_list
        {
            if !self.exc_stack_find(None) {
                self.print_ns(None);
            }
        } else if visible
            && !has_empty_ns
            && has_empty_ns_in_inclusive_list
            && !self.stack_find(None)
        {
            self.print_ns(None);
        }
        for ns in list {
            self.print_ns(Some(ns));
        }
    }

    /// xmlC14NAttrsCompare
    fn attr_cmp(&self) -> impl Fn(NodeId, NodeId) -> std::cmp::Ordering + 'a {
        let t = self.t;
        move |a1, a2| {
            use std::cmp::Ordering;
            if a1 == a2 {
                return Ordering::Equal;
            }
            let (n1, n2) = (&t.nodes[a1], &t.nodes[a2]);
            if n1.ns == n2.ns {
                return n1.name.as_bytes().cmp(n2.name.as_bytes());
            }
            let (ns1, ns2) = match (n1.ns, n2.ns) {
                (None, _) => return Ordering::Less,
                (_, None) => return Ordering::Greater,
                (Some(x), Some(y)) => (x, y),
            };
            if t.nss[ns1].prefix.is_none() {
                return Ordering::Less;
            }
            if t.nss[ns2].prefix.is_none() {
                return Ordering::Greater;
            }
            match t.nss[ns1].href.as_bytes().cmp(t.nss[ns2].href.as_bytes()) {
                Ordering::Equal => n1.name.as_bytes().cmp(n2.name.as_bytes()),
                o => o,
            }
        }
    }

    fn is_xml_attr(&self, attr: NodeId) -> bool {
        self.t.nodes[attr]
            .ns
            .map(|ns| self.is_xml_ns(ns))
            .unwrap_or(false)
    }

    /// xmlC14NProcessAttrsAxis (C14N 1.0 and exclusive modes)
    fn attrs_axis(&mut self, cur: NodeId, parent_visible: bool) {
        let cmp = self.attr_cmp();
        let mut list: Vec<NodeId> = Vec::new();
        for attr in self.t.attrs_of(cur) {
            if self.visible(attr, Some(cur)) {
                list_insert(&mut list, attr, &cmp);
            }
        }
        if !self.exclusive && parent_visible {
            // inherit xml:* attributes from invisible ancestors
            if let Some(p) = self.t.nodes[cur].parent {
                if !self.visible(p, self.t.nodes[p].parent) {
                    let mut tmp = Some(p);
                    while let Some(x) = tmp {
                        for attr in self.t.attrs_of(x) {
                            if self.is_xml_attr(attr)
                                && !list
                                    .iter()
                                    .any(|&l| cmp(l, attr) == std::cmp::Ordering::Equal)
                            {
                                list_insert(&mut list, attr, &cmp);
                            }
                        }
                        tmp = self.t.nodes[x].parent;
                    }
                }
            }
        }
        for attr in list {
            self.out.push(' ');
            if let Some(p) = self.t.nodes[attr]
                .ns
                .and_then(|ns| self.prefix(ns))
                .filter(|p| !p.is_empty())
            {
                self.out.push_str(p);
                self.out.push(':');
            }
            self.out.push_str(&self.t.nodes[attr].name);
            self.out.push_str("=\"");
            let v = self.t.list_get_string(self.t.nodes[attr].children, true);
            normalize(&mut self.out, &v, Norm::Attr);
            self.out.push('"');
        }
    }

    /// xmlC14NCheckForRelativeNamespaces
    fn check_relative_ns(&mut self, cur: NodeId) -> Result<(), ()> {
        let t = self.t;
        for &ns in &t.nodes[cur].ns_def {
            let href = self.href(ns);
            if href.is_empty() {
                continue;
            }
            match crate::uri::parse_uri_strict(href) {
                None => {
                    // raised with the element: its line and document URL
                    self.errors.push(crate::relaxng::node_error_in(
                        Some(t),
                        XML_FROM_C14N,
                        XML_ERR_INVALID_URI,
                        crate::parser::ERR_ERROR,
                        Some(cur),
                        "parsing namespace uri".to_string(),
                    ));
                    return Err(());
                }
                Some(u) => {
                    if u.scheme.as_deref().unwrap_or("").is_empty() {
                        let s = match &u.scheme {
                            Some(s) => s.clone(),
                            None => "(null)".to_string(),
                        };
                        self.errors.push(c14n_err(
                            XML_C14N_RELATIVE_NAMESPACE,
                            format!("Relative namespace UR is invalid here : {}\n", s),
                        ));
                        return Err(());
                    }
                }
            }
        }
        Ok(())
    }

    fn push_qname(&mut self, cur: NodeId) {
        if let Some(p) = self.t.nodes[cur]
            .ns
            .and_then(|ns| self.prefix(ns))
            .filter(|p| !p.is_empty())
        {
            self.out.push_str(p);
            self.out.push(':');
        }
        self.out.push_str(&self.t.nodes[cur].name);
    }

    /// xmlC14NProcessElementNode
    fn element(&mut self, cur: NodeId, visible: bool) -> Result<(), ()> {
        self.check_relative_ns(cur)?;
        let state = self.stack_save();
        let mut parent_is_doc = false;
        if visible {
            if self.parent_is_doc {
                parent_is_doc = true;
                self.parent_is_doc = false;
                self.pos = Pos::Inside;
            }
            self.out.push('<');
            self.push_qname(cur);
        }
        if self.exclusive {
            self.exc_namespaces_axis(cur, visible);
        } else {
            self.namespaces_axis(cur, visible);
        }
        if visible {
            self.stack_shift();
        }
        self.attrs_axis(cur, visible);
        if visible {
            self.out.push('>');
        }
        let t = self.t;
        if let Some(c) = t.nodes[cur].children {
            self.node_list(Some(c))?;
        }
        if visible {
            self.out.push_str("</");
            self.push_qname(cur);
            self.out.push('>');
            if parent_is_doc {
                self.parent_is_doc = true;
                self.pos = Pos::After;
            }
        }
        self.stack_restore(state);
        Ok(())
    }

    fn invalid_node(&mut self, ty: &str) -> Result<(), ()> {
        self.errors.push(c14n_err(
            XML_C14N_INVALID_NODE,
            format!("Node {} is invalid here : processing node\n", ty),
        ));
        Err(())
    }

    /// xmlC14NProcessNode
    fn node(&mut self, cur: NodeId) -> Result<(), ()> {
        let t = self.t;
        let n = &t.nodes[cur];
        let visible = self.visible(cur, n.parent);
        match n.t {
            NType::Element => self.element(cur, visible)?,
            NType::Text | NType::CData => {
                if visible && n.flags & F_NULL_CONTENT == 0 {
                    normalize(&mut self.out, &n.content, Norm::Text);
                }
            }
            NType::Pi => {
                if visible {
                    self.out
                        .push_str(if self.pos == Pos::After { "\n<?" } else { "<?" });
                    self.out.push_str(&n.name);
                    if !n.content.is_empty() {
                        self.out.push(' ');
                        normalize(&mut self.out, &n.content, Norm::Pi);
                    }
                    self.out.push_str(if self.pos == Pos::Before {
                        "?>\n"
                    } else {
                        "?>"
                    });
                }
            }
            NType::Comment => {
                if visible && self.with_comments {
                    self.out.push_str(if self.pos == Pos::After {
                        "\n<!--"
                    } else {
                        "<!--"
                    });
                    if n.flags & F_NULL_CONTENT == 0 {
                        normalize(&mut self.out, &n.content, Norm::Comment);
                    }
                    self.out.push_str(if self.pos == Pos::Before {
                        "-->\n"
                    } else {
                        "-->"
                    });
                }
            }
            NType::Document | NType::DocumentFrag | NType::HtmlDocument => {
                if let Some(c) = n.children {
                    self.pos = Pos::Before;
                    self.parent_is_doc = true;
                    self.node_list(Some(c))?;
                }
            }
            NType::Attribute => return self.invalid_node("XML_ATTRIBUTE_NODE"),
            NType::NamespaceDecl => return self.invalid_node("XML_NAMESPACE_DECL"),
            NType::EntityRef => return self.invalid_node("XML_ENTITY_REF_NODE"),
            NType::Entity => return self.invalid_node("XML_ENTITY_NODE"),
            NType::DocumentType
            | NType::Notation
            | NType::Dtd
            | NType::ElementDecl
            | NType::AttributeDecl
            | NType::EntityDecl => {}
        }
        Ok(())
    }

    /// xmlC14NProcessNodeList
    fn node_list(&mut self, first: Option<NodeId>) -> Result<(), ()> {
        let mut cur = first;
        while let Some(c) = cur {
            self.node(c)?;
            cur = self.t.nodes[c].next;
        }
        Ok(())
    }
}

/// xmlC14NExecute over `doc`. Returns the canonical form, or the errors.
fn execute(
    t: &Tree,
    doc: NodeId,
    vis: &Vis,
    exclusive: bool,
    prefixes: Option<&[String]>,
    with_comments: bool,
) -> Result<String, Vec<XmlError>> {
    let mut ctx = Ctx {
        t,
        vis,
        with_comments,
        exclusive,
        inclusive_prefixes: if exclusive { prefixes } else { None },
        out: String::new(),
        pos: Pos::Before,
        parent_is_doc: true,
        stack: NsStack::default(),
        errors: Vec::new(),
    };
    match ctx.node_list(t.nodes[doc].children) {
        Ok(()) => Ok(ctx.out),
        Err(()) => Err(ctx.errors),
    }
}

/// The argument checks and execution of dom_canonicalization (minus the
/// output, which `open_output` opens). `xpath_argno` is the 1-based
/// position of $xpath.
/// Returns None when canonicalization failed (PHP returns false).
#[allow(clippy::too_many_arguments, clippy::result_unit_err)]
pub fn canonicalize(
    vm: &mut Vm,
    fname: &str,
    node: NodeId,
    doc: NodeId,
    exclusive: bool,
    with_comments: bool,
    xpath: Option<&Value>,
    prefixes: Option<&Value>,
    xpath_argno: usize,
    open_output: &mut dyn FnMut(&mut Vm) -> bool,
) -> Result<Option<Vec<u8>>, ()> {
    let vis = match xpath {
        None => {
            if tree::with(|t| t.is_doc(node)) {
                Vis::All
            } else {
                Vis::Subtree(node)
            }
        }
        Some(xv) => {
            let arr = match xv {
                Value::Array(a) => a.clone(),
                _ => return Ok(None),
            };
            let (query, nslist) = {
                let a = arr.borrow();
                let q = a.get_str(b"query").map(|v| v.deref());
                let mut nslist: Vec<(String, String)> = Vec::new();
                if let Some(Value::Array(nsa)) = a.get_str(b"namespaces").map(|v| v.deref()) {
                    for (k, v) in nsa.borrow().iter() {
                        if let (goro_core::array::ArrayKey::String(k), Value::String(v)) =
                            (k, v.deref())
                        {
                            nslist.push((k.to_string_lossy(), v.to_string_lossy()));
                        }
                    }
                }
                (q, nslist)
            };
            let query = match query {
                None => {
                    throw_error(
                        vm,
                        b"ValueError",
                        &format!(
                            "{}(): Argument #{} ($xpath) must have a \"query\" key",
                            fname, xpath_argno
                        ),
                    );
                    return Err(());
                }
                Some(Value::String(s)) => s.to_string_lossy(),
                Some(other) => {
                    throw_error(
                        vm,
                        b"TypeError",
                        &format!(
                            "{}(): Argument #{} ($xpath) \"query\" option must be a string, {} given",
                            fname,
                            xpath_argno,
                            type_name_of(&other)
                        ),
                    );
                    return Err(());
                }
            };
            let mut ctx = xpath::Ctx {
                ns: &nslist,
                vars: &[],
                ext: None,
                doc: Some(doc),
            };
            let res = xpath::eval_ctx(&query, node, &mut ctx);
            let set = match res {
                Ok(XValue::NodeSet(s)) => s,
                Ok(_) => {
                    throw_error(vm, b"Error", "XPath query did not return a nodeset");
                    return Err(());
                }
                Err(e) => {
                    xpath::report_error(vm, fname, &e, &query);
                    throw_error(vm, b"Error", "XPath query did not return a nodeset");
                    return Err(());
                }
            };
            tree::with(|t| {
                let mut nodes = HashSet::new();
                let mut ns = HashSet::new();
                for n in set {
                    let nd = &t.nodes[n];
                    if nd.t == NType::NamespaceDecl {
                        if let Some(p) = nd.parent {
                            ns.insert((p, nd.name.clone()));
                        }
                    } else {
                        nodes.insert(n);
                    }
                }
                Vis::Set { nodes, ns }
            })
        }
    };
    let mut inclusive: Option<Vec<String>> = None;
    if let Some(Value::Array(pa)) = prefixes {
        if exclusive {
            let v: Vec<String> = pa
                .borrow()
                .values()
                .filter_map(|v| {
                    if let Value::String(s) = v.deref() {
                        Some(s.to_string_lossy())
                    } else {
                        None
                    }
                })
                .collect();
            inclusive = Some(v);
        } else {
            let line = vm.current_line;
            vm.emit_notice_at(
                &format!(
                    "{}(): Inclusive namespace prefixes only allowed in exclusive mode.",
                    fname
                ),
                line,
            );
        }
    }
    if !open_output(vm) {
        return Ok(None);
    }
    let r = tree::with(|t| execute(t, doc, &vis, exclusive, inclusive.as_deref(), with_comments));
    match r {
        Ok(s) => Ok(Some(s.into_bytes())),
        Err(errors) => {
            crate::libxml::report_ex(vm, fname, &errors, crate::libxml::Style::Generic, &|_| true);
            Ok(None)
        }
    }
}
