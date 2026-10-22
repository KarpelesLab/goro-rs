//! php_xml.c: XML helpers of ext/soap, plus the small set of libxml2 tree
//! functions the extension uses (xmlNewChild, xmlSetProp, xmlNodeSetContent,
//! xmlDocDumpMemory, ...), over goro-ext-xml's libxml2-style tree.

use goro_core::vm::Vm;
use goro_ext_xml::parser::{XML_PARSE_HUGE, XML_PARSE_NOBLANKS};
use goro_ext_xml::tree::{self, NType, NodeId, NsId, Tree};

pub type Node = NodeId;

/// Run `f` with the global tree.
#[inline]
pub fn t<R>(f: impl FnOnce(&mut Tree) -> R) -> R {
    tree::with(f)
}

// ---------------------------------------------------------------------------
// Accessors

pub fn name(n: Node) -> String {
    t(|t| t.nodes[n].name.clone())
}

pub fn ntype(n: Node) -> NType {
    t(|t| t.nodes[n].t)
}

pub fn is_element(n: Node) -> bool {
    ntype(n) == NType::Element
}

pub fn children(n: Node) -> Option<Node> {
    t(|t| t.nodes[n].children)
}

pub fn last(n: Node) -> Option<Node> {
    t(|t| t.nodes[n].last)
}

pub fn next(n: Node) -> Option<Node> {
    t(|t| t.nodes[n].next)
}

pub fn parent(n: Node) -> Option<Node> {
    t(|t| t.nodes[n].parent)
}

pub fn properties(n: Node) -> Option<Node> {
    t(|t| {
        if t.nodes[n].t == NType::Element {
            t.nodes[n].properties
        } else {
            None
        }
    })
}

pub fn doc(n: Node) -> Option<Node> {
    t(|t| t.nodes[n].doc)
}

pub fn content(n: Node) -> String {
    t(|t| t.nodes[n].content.clone())
}

pub fn set_raw_content(n: Node, s: &str) {
    t(|t| t.nodes[n].content = s.to_string())
}

/// node->ns
pub fn node_ns(n: Node) -> Option<NsId> {
    t(|t| t.nodes[n].ns)
}

/// node->ns->href
pub fn ns_href_of(n: Node) -> Option<String> {
    t(|t| t.nodes[n].ns.map(|ns| t.nss[ns].href.clone()))
}

pub fn ns_href(ns: NsId) -> String {
    t(|t| t.nss[ns].href.clone())
}

pub fn ns_prefix(ns: NsId) -> Option<String> {
    t(|t| t.nss[ns].prefix.clone())
}

/// The value of an attribute (attr->children->content).
pub fn attr_value(a: Node) -> String {
    t(|t| t.node_get_content(a))
}

/// Whether the attribute has any content (attr->children &&
/// attr->children->content).
pub fn attr_has_value(a: Node) -> bool {
    t(|t| t.nodes[a].children.is_some())
}

/// The first child: (type, content, has next sibling).
pub fn first_child_info(n: Node) -> Option<(NType, String, bool)> {
    t(|t| {
        let c = t.nodes[n].children?;
        Some((
            t.nodes[c].t,
            t.nodes[c].content.clone(),
            t.nodes[c].next.is_some(),
        ))
    })
}

/// node->children->content exists (a text-like first child).
pub fn first_child_has_content(n: Node) -> bool {
    t(|t| match t.nodes[n].children {
        Some(c) => matches!(
            t.nodes[c].t,
            NType::Text | NType::CData | NType::Comment | NType::Pi
        ),
        None => false,
    })
}

// ---------------------------------------------------------------------------
// php_xml.c

/// node_find_ns(): the node's namespace, or the default one in scope.
pub fn node_find_ns_href(n: Node) -> Option<String> {
    t(|t| {
        if let Some(ns) = t.nodes[n].ns {
            return Some(t.nss[ns].href.clone());
        }
        t.search_ns(n, None).map(|ns| t.nss[ns].href.clone())
    })
}

pub fn attr_is_equal_ex(a: Node, name: &str, ns: Option<&str>) -> bool {
    t(|t| {
        let n = &t.nodes[a];
        if n.name != name {
            return false;
        }
        match (ns, n.ns) {
            (Some(ns), Some(x)) => t.nss[x].href == ns,
            (Some(_), None) => false,
            (None, Some(_)) => false,
            (None, None) => true,
        }
    })
}

pub fn node_is_equal_ex(n: Node, name: Option<&str>, ns: Option<&str>) -> bool {
    let nm_ok = match name {
        None => true,
        Some(nm) => t(|t| t.nodes[n].name == nm),
    };
    if !nm_ok {
        return false;
    }
    if let Some(ns) = ns {
        return node_find_ns_href(n).as_deref() == Some(ns);
    }
    true
}

pub fn node_is_equal(n: Node, name: &str) -> bool {
    node_is_equal_ex(n, Some(name), None)
}

pub fn node_is_equal_ex_one_of(n: Node, name: &str, namespaces: &[&str]) -> bool {
    if t(|t| t.nodes[n].name != name) {
        return false;
    }
    match node_find_ns_href(n) {
        Some(h) => namespaces.contains(&h.as_str()),
        None => false,
    }
}

pub fn get_attribute_any_ns(first: Option<Node>, name: &str) -> Option<Node> {
    t(|t| {
        let mut a = first;
        while let Some(x) = a {
            if t.nodes[x].name == name {
                return Some(x);
            }
            a = t.nodes[x].next;
        }
        None
    })
}

pub fn get_attribute_ex(first: Option<Node>, name: &str, ns: Option<&str>) -> Option<Node> {
    let mut a = first;
    while let Some(x) = a {
        if attr_is_equal_ex(x, name, ns) {
            return Some(x);
        }
        a = next(x);
    }
    None
}

pub fn get_attribute(first: Option<Node>, name: &str) -> Option<Node> {
    get_attribute_ex(first, name, None)
}

pub fn get_node_ex(first: Option<Node>, name: &str, ns: Option<&str>) -> Option<Node> {
    let mut n = first;
    while let Some(x) = n {
        if node_is_equal_ex(x, Some(name), ns) {
            return Some(x);
        }
        n = next(x);
    }
    None
}

pub fn get_node(first: Option<Node>, name: &str) -> Option<Node> {
    get_node_ex(first, name, None)
}

pub fn get_node_with_attribute_ex(
    first: Option<Node>,
    name: Option<&str>,
    name_ns: Option<&str>,
    attribute: &str,
    value: &str,
    attr_ns: Option<&str>,
) -> Option<Node> {
    let mut node = first;
    while let Some(mut n) = node {
        if let Some(nm) = name {
            n = get_node_ex(Some(n), nm, name_ns)?;
        }
        if let Some(a) = get_attribute_ex(properties(n), attribute, attr_ns) {
            if attr_value(a) == value {
                return Some(n);
            }
        }
        node = next(n);
    }
    None
}

pub fn get_node_with_attribute_recursive_ex(
    first: Option<Node>,
    name: Option<&str>,
    name_ns: Option<&str>,
    attribute: &str,
    value: &str,
    attr_ns: Option<&str>,
) -> Option<Node> {
    let mut node = first;
    while let Some(n) = node {
        if node_is_equal_ex(n, name, name_ns) {
            if let Some(a) = get_attribute_ex(properties(n), attribute, attr_ns) {
                if attr_value(a) == value {
                    return Some(n);
                }
            }
        }
        if let Some(c) = children(n) {
            if let Some(r) = get_node_with_attribute_recursive_ex(
                Some(c),
                name,
                name_ns,
                attribute,
                value,
                attr_ns,
            ) {
                return Some(r);
            }
        }
        node = next(n);
    }
    None
}

/// parse_namespace(): ("value", Some("prefix")) for "prefix:value".
pub fn parse_namespace(inval: &str) -> (String, Option<String>) {
    match inval.rfind(':') {
        Some(p) if p != 0 => (inval[p + 1..].to_string(), Some(inval[..p].to_string())),
        _ => (inval.to_string(), None),
    }
}

/// xmlSearchNs(doc, node, prefix)
pub fn search_ns(n: Node, prefix: Option<&str>) -> Option<NsId> {
    t(|t| t.search_ns(n, prefix))
}

/// xmlSearchNsByHref(doc, node, href)
pub fn search_ns_by_href(n: Node, href: &str) -> Option<NsId> {
    t(|t| t.search_ns_by_href(n, href))
}

// ---------------------------------------------------------------------------
// Parsing

fn strip_comments(t: &mut Tree, n: Node) {
    let mut c = t.nodes[n].children;
    while let Some(x) = c {
        let nx = t.nodes[x].next;
        match t.nodes[x].t {
            NType::Comment => t.unlink(x),
            NType::Element => strip_comments(t, x),
            _ => {}
        }
        c = nx;
    }
}

/// cleanup_xml_node(): removes blank text, comments and other
/// insignificant nodes.
fn cleanup_xml_node(t: &mut Tree, n: Node) {
    let mut c = t.nodes[n].children;
    while let Some(x) = c {
        let nx = t.nodes[x].next;
        match t.nodes[x].t {
            NType::Text => {
                if tree::is_blank_str(&t.nodes[x].content) {
                    t.unlink(x);
                }
            }
            NType::Element | NType::CData => {
                if t.nodes[x].children.is_some() {
                    cleanup_xml_node(t, x);
                }
            }
            _ => t.unlink(x),
        }
        c = nx;
    }
}

const PARSE_OPTIONS: i64 = XML_PARSE_NOBLANKS | XML_PARSE_HUGE;

/// soap_xmlParse_ex(): Ok(doc), or Err(last error message) when the
/// document isn't well-formed.
fn parse_ex(data: &[u8], url: Option<&str>) -> Result<Node, Option<String>> {
    let out = goro_ext_xml::sax2::parse_doc(data, url, PARSE_OPTIONS);
    let msg = out.errors.last().map(|e| e.message.clone());
    match out.doc {
        Some(d) if out.well_formed => {
            t(|t| strip_comments(t, d));
            Ok(d)
        }
        Some(d) => {
            free_doc(d);
            Err(msg)
        }
        None => Err(msg),
    }
}

/// soap_xmlParseMemory()
pub fn parse_memory(data: &[u8]) -> Option<Node> {
    parse_ex(data, None).ok()
}

/// soap_xmlParseFile(): Ok(doc) or Err(libxml error message).
pub fn parse_file(vm: &mut Vm, filename: &str) -> Result<Node, Option<String>> {
    let data = match read_file(vm, filename) {
        Some(d) => d,
        None => {
            return Err(Some(format!(
                "failed to load \"{}\": No such file or directory\n",
                filename
            )));
        }
    };
    let doc = parse_ex(&data, Some(filename))?;
    t(|t| cleanup_xml_node(t, doc));
    Ok(doc)
}

/// Read a file through the PHP streams layer (allow_url_fopen forced on).
pub fn read_file(vm: &mut Vm, filename: &str) -> Option<Vec<u8>> {
    let key = b"allow_url_fopen".as_slice();
    let saved = vm.constants.get(key).cloned();
    vm.constants.insert(
        key.to_vec(),
        goro_core::value::Value::String(goro_core::string::PhpString::from_bytes(b"1")),
    );
    let r = goro_ext_xml::io::read_resource(vm, filename);
    match saved {
        Some(v) => {
            vm.constants.insert(key.to_vec(), v);
        }
        None => {
            vm.constants.remove(key);
        }
    }
    r
}

/// xmlGetIntSubset(doc) != NULL
pub fn has_int_subset(doc: Node) -> bool {
    t(|t| t.int_subset(doc).is_some())
}

/// xmlFreeDoc(): give the document's nodes back to the arena.
pub fn free_doc(doc: Node) {
    t(|t| {
        let mut all = Vec::new();
        let mut stack = vec![doc];
        while let Some(n) = stack.pop() {
            all.push(n);
            let mut a = if t.nodes[n].t == NType::Element {
                t.nodes[n].properties
            } else {
                None
            };
            while let Some(x) = a {
                stack.push(x);
                a = t.nodes[x].next;
            }
            if matches!(t.nodes[n].t, NType::EntityRef) {
                continue;
            }
            let mut c = t.nodes[n].children;
            while let Some(x) = c {
                stack.push(x);
                c = t.nodes[x].next;
            }
        }
        // Nodes held by PHP objects (they cannot be ours) are left alone.
        if all.iter().any(|&n| t.nodes[n].wrapper.is_some()) {
            return;
        }
        for n in all {
            t.release(n);
        }
    })
}

// ---------------------------------------------------------------------------
// Tree building

/// xmlNewDoc("1.0") with encoding UTF-8 (as the SOAP serializers set it).
pub fn new_doc() -> Node {
    t(|t| {
        let d = t.new_doc(Some("1.0"));
        if let Some(i) = t.doc_info_mut(d) {
            i.encoding = Some("UTF-8".to_string());
        }
        d
    })
}

/// xmlNewDocNode(doc, NULL, name, NULL): an unlinked element.
pub fn new_doc_node(doc: Option<Node>, name: &str) -> Node {
    t(|t| t.new_element(doc, name))
}

/// xmlDocSetRootElement
pub fn doc_set_root(doc: Node, root: Node) {
    t(|t| {
        t.nodes[root].doc = Some(doc);
        t.link_child_raw(doc, root);
    })
}

/// xmlAddChild
pub fn add_child(parent: Node, child: Node) -> Node {
    t(|t| t.add_child(parent, child))
}

/// xmlNewChild(parent, ns, name, content)
pub fn new_child(parent: Node, ns: Option<NsId>, name: &str, content: Option<&str>) -> Node {
    let d = doc(parent);
    let n = new_doc_node(d, name);
    t(|t| t.nodes[n].ns = ns);
    if let Some(c) = content {
        node_set_content(n, c);
    }
    t(|t| t.link_child_raw(parent, n));
    n
}

/// xmlNewNs(node, href, prefix)
pub fn new_ns(node: Node, href: &str, prefix: Option<&str>) -> Option<NsId> {
    t(|t| t.new_ns(Some(node), href, prefix))
}

/// xmlSetNs
pub fn set_ns(node: Node, ns: Option<NsId>) {
    t(|t| t.nodes[node].ns = ns)
}

/// xmlNodeSetName
pub fn set_name(node: Node, name: &str) {
    t(|t| t.nodes[node].name = name.to_string())
}

/// xmlSetNsProp
pub fn set_ns_prop(node: Node, ns: Option<NsId>, name: &str, value: &str) -> Node {
    t(|t| t.set_ns_prop(node, ns, name, value))
}

/// xmlSetProp (QName prefixes resolved in scope)
pub fn set_prop(node: Node, name: &str, value: &str) -> Node {
    if let Some(p) = name.find(':') {
        if p > 0 && p + 1 < name.len() {
            if let Some(ns) = search_ns(node, Some(&name[..p])) {
                return set_ns_prop(node, Some(ns), &name[p + 1..], value);
            }
        }
    }
    set_ns_prop(node, None, name, value)
}

/// xmlNodeSetLang: xml:lang
pub fn set_lang(node: Node, lang: &str) {
    let ns = t(|t| {
        let d = t.nodes[node].doc;
        t.xml_ns(d)
    });
    set_ns_prop(node, Some(ns), "lang", lang);
}

/// Decode character and predefined entity references (xmlStringGetNodeList).
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'&' {
            if let Some(end) = s[i + 1..].find(';') {
                let ent = &s[i + 1..i + 1 + end];
                let rep = if let Some(num) = ent.strip_prefix('#') {
                    let v = if let Some(h) = num.strip_prefix('x').or(num.strip_prefix('X')) {
                        u32::from_str_radix(h, 16).ok()
                    } else {
                        num.parse::<u32>().ok()
                    };
                    v.and_then(char::from_u32).map(|c| c.to_string())
                } else {
                    match ent {
                        "lt" => Some("<".to_string()),
                        "gt" => Some(">".to_string()),
                        "amp" => Some("&".to_string()),
                        "quot" => Some("\"".to_string()),
                        "apos" => Some("'".to_string()),
                        _ => None,
                    }
                };
                if let Some(r) = rep {
                    out.push_str(&r);
                    i += end + 2;
                    continue;
                }
            }
        }
        let ch = s[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// xmlNodeSetContent
pub fn node_set_content(node: Node, value: &str) {
    let v = decode_entities(value);
    t(|t| t.node_set_content(node, &v))
}

/// xmlNewTextLen + xmlAddChild
pub fn add_text(parent: Node, value: &str) {
    t(|t| {
        let d = t.nodes[parent].doc;
        let n = t.new_text(d, value);
        t.add_child(parent, n);
    })
}

/// A text node whose content is written out verbatim (xmlStringTextNoenc),
/// appended without merging.
pub fn add_text_noenc(parent: Node, value: &str) -> Node {
    t(|t| {
        let d = t.nodes[parent].doc;
        let n = t.new_text(d, value);
        t.nodes[n].name = "textnoenc".to_string();
        t.nodes[n].flags |= 1;
        t.link_child_raw(parent, n);
        n
    })
}

/// xmlUnlinkNode
pub fn unlink(n: Node) {
    t(|t| t.unlink(n))
}

/// xmlDocCopyNode(node, doc, 1)
pub fn copy_node(n: Node, doc: Option<Node>) -> Node {
    t(|t| t.copy_node(n, doc, 1, None))
}

/// xmlDocDumpMemory(): the document as UTF-8 XML with its declaration.
pub fn dump_doc(doc: Node) -> Vec<u8> {
    t(|t| goro_ext_xml::serialize::doc_to_bytes(t, doc, None, false, false, false))
}

/// xmlNodeDump(buf, NULL, node, 0, 0): no encoding, so non-ASCII
/// characters are written as character references.
pub fn node_dump(n: Node) -> String {
    t(|t| {
        let mut ctx = goro_ext_xml::serialize::SaveCtx::new(None, false);
        goro_ext_xml::serialize::dump_node(t, &mut ctx, n);
        ctx.out
    })
}

/// The document's URL.
pub fn doc_url(doc: Node) -> Option<String> {
    t(|t| t.doc_url(doc))
}

/// schema_location_construct_uri(): the attribute's value resolved against
/// the node's base.
pub fn construct_uri(attr: Node) -> String {
    let (base, value) = t(|t| {
        let p = t.nodes[attr].parent.unwrap_or(attr);
        let base = goro_ext_xml::dom::node::node_get_base(t, p)
            .or_else(|| t.nodes[attr].doc.and_then(|d| t.doc_url(d)));
        (base, t.node_get_content(attr))
    });
    goro_ext_xml::uri::build_uri(&value, base.as_deref()).unwrap_or(value)
}
