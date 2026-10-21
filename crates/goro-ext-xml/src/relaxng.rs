//! RelaxNG schemas: parsing, simplification, compilation of the
//! deterministic parts into automata and validation of document trees,
//! ported from libxml2 2.13 relaxng.c.
//!
//! Public API:
//! - [`parse_file`] / [`parse_memory`]: build a [`RelaxNg`] grammar
//!   (xmlRelaxNGNewParserCtxt / xmlRelaxNGNewMemParserCtxt + xmlRelaxNGParse).
//!   Errors (schema document parser errors, I/O warnings and RelaxNG parser
//!   errors) are handed to the `sink` as they are raised, so that they come
//!   out in libxml2's order relative to errors reported by the user entity
//!   loader.
//! - [`validate_doc()`] is xmlRelaxNGValidateDoc.
//! - [`ValidCtxt`]: a validation context, also offering the progressive
//!   (streaming) interface used by xmlreader (xmlRelaxNGValidatePushElement,
//!   PushCData, PopElement, FullElement).

use crate::parser::{ERR_ERROR, ERR_WARNING, FROM_IO, FROM_VALID, XmlError};
use crate::tree::{self, NType, NodeId, Tree};
use crate::xmlerrors as xe;
use crate::xmlregexp::{AM_AUTOMATA_RNG, Automata, ExecCtxt, Regexp, StateId};
use crate::xmlschemastypes as xst;
use goro_core::vm::Vm;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const RNG_NS: &str = "http://relaxng.org/ns/structure/1.0";
const XSD_DT_NS: &str = "http://www.w3.org/2001/XMLSchema-datatypes";
const MAX_ERROR: usize = 5;

type DefId = usize;
type GId = usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum T {
    Noop,
    Empty,
    NotAllowed,
    Except,
    Text,
    Element,
    Datatype,
    Param,
    Value,
    List,
    Attribute,
    Def,
    Ref,
    ExternalRef,
    ParentRef,
    Optional,
    ZeroOrMore,
    OneOrMore,
    Choice,
    Group,
    Interleave,
    Start,
}

// define flags
const IS_NULLABLE: i32 = 1 << 0;
const IS_NOT_NULLABLE: i32 = 1 << 1;
const IS_INDETERMINIST: i32 = 1 << 2;
const IS_MIXED: i32 = 1 << 3;
const IS_TRIABLE: i32 = 1 << 4;
const IS_PROCESSED: i32 = 1 << 5;
const IS_COMPILABLE: i32 = 1 << 6;
const IS_NOT_COMPILABLE: i32 = 1 << 7;
const IS_EXTERNAL_REF: i32 = 1 << 8;

// parser flags
const IN_ATTRIBUTE: i32 = 1 << 0;
const IN_ONEORMORE: i32 = 1 << 1;
const IN_LIST: i32 = 1 << 2;
const IN_DATAEXCEPT: i32 = 1 << 3;
const IN_START: i32 = 1 << 4;
const IN_OOMGROUP: i32 = 1 << 5;
const IN_OOMINTERLEAVE: i32 = 1 << 6;
const IN_EXTERNALREF: i32 = 1 << 7;
const IN_ANYEXCEPT: i32 = 1 << 8;
const IN_NSEXCEPT: i32 = 1 << 9;

// validation flags
const FLAGS_IGNORABLE: i32 = 1;
const FLAGS_NEGATIVE: i32 = 2;
const FLAGS_MIXED_CONTENT: i32 = 4;
const FLAGS_NOERROR: i32 = 8;

// partition flags
const IS_DETERMINIST: i32 = 1;
const IS_NEEDCHECK: i32 = 2;

// content types
const CT_ERROR: i32 = -1;
const CT_EMPTY: i32 = 0;
const CT_SIMPLE: i32 = 1;
const CT_COMPLEX: i32 = 2;

const ERROR_IS_DUP: i32 = 1;

// xmlRelaxNGValidErr
const ERR_OK: i32 = 0;
const ERR_MEMORY: i32 = 1;
const ERR_TYPE: i32 = 2;
const ERR_TYPEVAL: i32 = 3;
const ERR_DUPID: i32 = 4;
const ERR_TYPECMP: i32 = 5;
const ERR_NOSTATE: i32 = 6;
const ERR_NODEFINE: i32 = 7;
const ERR_LISTEXTRA: i32 = 8;
const ERR_INTERNODATA: i32 = 10;
const ERR_INTERSEQ: i32 = 11;
const ERR_INTEREXTRA: i32 = 12;
const ERR_ELEMNAME: i32 = 13;
const ERR_ELEMNONS: i32 = 15;
const ERR_ELEMWRONGNS: i32 = 17;
const ERR_ELEMEXTRANS: i32 = 19;
const ERR_ELEMNOTEMPTY: i32 = 21;
const ERR_NOELEM: i32 = 22;
const ERR_NOTELEM: i32 = 23;
const ERR_ATTRVALID: i32 = 24;
const ERR_CONTENTVALID: i32 = 25;
const ERR_EXTRACONTENT: i32 = 26;
const ERR_INVALIDATTR: i32 = 27;
const ERR_DATAELEM: i32 = 28;
const ERR_VALELEM: i32 = 29;
const ERR_LISTELEM: i32 = 30;
const ERR_DATATYPE: i32 = 31;
const ERR_VALUE: i32 = 32;
const ERR_LIST: i32 = 33;
const ERR_NOGRAMMAR: i32 = 34;
const ERR_EXTRADATA: i32 = 35;
const ERR_LACKDATA: i32 = 36;
const ERR_INTERNAL: i32 = 37;
const ERR_ELEMWRONG: i32 = 38;
const ERR_TEXTWRONG: i32 = 39;

/// A registered datatype library.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lib {
    /// http://www.w3.org/2001/XMLSchema-datatypes
    Xsd,
    /// the built-in library (string and token)
    Builtin,
}

type TriageKey = (String, Option<String>);

#[derive(Clone, Debug)]
struct IGroup {
    rule: DefId,
    defs: Option<Vec<DefId>>,
    #[allow(dead_code)]
    attrs: Option<Vec<DefId>>,
}

#[derive(Clone, Debug)]
struct Partition {
    nbgroups: usize,
    triage: HashMap<TriageKey, usize>,
    flags: i32,
    groups: Vec<IGroup>,
}

#[derive(Clone, Debug)]
struct Define {
    typ: T,
    node: Option<NodeId>,
    name: Option<String>,
    ns: Option<String>,
    value: Option<String>,
    lib: Option<Lib>,
    /// choice triage table (IS_TRIABLE)
    triage: Option<HashMap<TriageKey, DefId>>,
    /// interleave partitions
    partition: Option<Rc<Partition>>,
    /// precomputed value of a <value> (define->attrs in libxml2)
    val: Option<xst::Val>,
    content: Option<DefId>,
    parent: Option<DefId>,
    next: Option<DefId>,
    attrs: Option<DefId>,
    name_class: Option<DefId>,
    next_hash: Option<DefId>,
    depth: i32,
    dflags: i32,
    cont_model: Option<Rc<Regexp>>,
}

impl Define {
    fn new(node: Option<NodeId>) -> Define {
        Define {
            typ: T::Empty,
            node,
            name: None,
            ns: None,
            value: None,
            lib: None,
            triage: None,
            partition: None,
            val: None,
            content: None,
            parent: None,
            next: None,
            attrs: None,
            name_class: None,
            next_hash: None,
            depth: -1,
            dflags: 0,
            cont_model: None,
        }
    }
}

/// An insertion-ordered string-keyed table (xmlHashTable as used here;
/// libxml2's scan order is randomized, so any fixed order is fine).
#[derive(Clone, Debug, Default)]
struct OMap(Vec<(String, DefId)>);

impl OMap {
    fn add(&mut self, key: Option<&str>, v: DefId) -> i32 {
        let k = match key {
            Some(k) => k,
            None => return -1,
        };
        if self.0.iter().any(|(n, _)| n == k) {
            return -1;
        }
        self.0.push((k.to_string(), v));
        0
    }
    fn lookup(&self, key: Option<&str>) -> Option<DefId> {
        let k = key?;
        self.0.iter().find(|(n, _)| n == k).map(|(_, v)| *v)
    }
}

#[derive(Clone, Debug, Default)]
struct Grammar {
    parent: Option<GId>,
    children: Option<GId>,
    next: Option<GId>,
    start: Option<DefId>,
    defs: Option<OMap>,
    refs: Option<OMap>,
}

/// A compiled RelaxNG grammar (xmlRelaxNG).
pub struct RelaxNg {
    defs: Vec<Define>,
    start: Option<DefId>,
    idref: bool,
    /// The (simplified) schema document, kept alive for the define nodes.
    #[allow(dead_code)]
    doc: Option<NodeId>,
}

impl std::fmt::Debug for RelaxNg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RelaxNg({} defines)", self.defs.len())
    }
}

// ---------------------------------------------------------------------------
// Tree access helpers (each takes the tree lock briefly)

fn ntype(n: NodeId) -> NType {
    tree::with(|t| t.nodes[n].t)
}

fn nname(n: NodeId) -> String {
    tree::with(|t| t.nodes[n].name.clone())
}

fn nns(n: NodeId) -> Option<String> {
    tree::with(|t| t.ns_href(t.nodes[n].ns).map(|s| s.to_string()))
}

fn has_ns(n: NodeId) -> bool {
    tree::with(|t| t.nodes[n].ns.is_some())
}

fn nnext(n: NodeId) -> Option<NodeId> {
    tree::with(|t| t.nodes[n].next)
}

fn nprev(n: NodeId) -> Option<NodeId> {
    tree::with(|t| t.nodes[n].prev)
}

fn nparent(n: NodeId) -> Option<NodeId> {
    tree::with(|t| t.nodes[n].parent)
}

fn nchildren(n: NodeId) -> Option<NodeId> {
    tree::with(|t| t.nodes[n].children)
}

fn set_next(n: NodeId, v: Option<NodeId>) {
    tree::with(|t| t.nodes[n].next = v)
}

/// Raw content of a text-like node.
fn ncontent(n: NodeId) -> String {
    tree::with(|t| t.nodes[n].content.clone())
}

/// xmlNodeGetContent
fn get_content(n: NodeId) -> String {
    tree::with(|t| t.node_get_content(n))
}

fn is_element(n: Option<NodeId>) -> bool {
    n.map(|n| ntype(n) == NType::Element).unwrap_or(false)
}

fn is_text_like(n: NodeId) -> bool {
    matches!(ntype(n), NType::Text | NType::CData)
}

/// IS_RELAXNG(node, name)
fn is_rng(n: Option<NodeId>, name: &str) -> bool {
    match n {
        None => false,
        Some(n) => tree::with(|t| {
            let nd = &t.nodes[n];
            nd.t == NType::Element
                && nd.ns.is_some()
                && nd.name == name
                && t.ns_href(nd.ns) == Some(RNG_NS)
        }),
    }
}

/// xmlGetProp
fn get_prop(n: NodeId, name: &str) -> Option<String> {
    tree::with(|t| t.get_prop(n, name))
}

/// xmlHasProp
fn has_prop(n: NodeId, name: &str) -> bool {
    tree::with(|t| t.has_prop(n, name).is_some())
}

/// xmlSetProp (no namespace)
fn set_prop(n: NodeId, name: &str, value: &str) {
    tree::with(|t| {
        if t.nodes[n].t == NType::Element {
            t.set_ns_prop(n, None, name, value);
        }
    })
}

/// xmlUnsetProp
fn unset_prop(n: NodeId, name: &str) {
    tree::with(|t| {
        if let Some(a) = t.has_ns_prop(n, name, None) {
            t.unlink(a);
        }
    })
}

fn unlink(n: NodeId) {
    tree::with(|t| t.unlink(n))
}

fn root_element(doc: NodeId) -> Option<NodeId> {
    tree::with(|t| t.root_element(doc))
}

fn is_blank_bytes(s: &[u8]) -> bool {
    s.iter().all(|&c| matches!(c, b' ' | b'\t' | b'\n' | b'\r'))
}

fn is_blank_ch(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}

/// IS_BLANK_NODE: the content of a text node is made of blanks (non text
/// nodes have no content and count as blank)
fn is_blank_node(n: NodeId) -> bool {
    tree::with(|t| {
        let nd = &t.nodes[n];
        if matches!(
            nd.t,
            NType::Text | NType::CData | NType::Comment | NType::Pi
        ) {
            is_blank_bytes(nd.content.as_bytes())
        } else {
            true
        }
    })
}

/// xmlIsBlankNode
fn xml_is_blank_node(n: NodeId) -> bool {
    tree::with(|t| {
        let nd = &t.nodes[n];
        if !matches!(nd.t, NType::Text | NType::CData) {
            return false;
        }
        is_blank_bytes(nd.content.as_bytes())
    })
}

/// xmlRelaxNGNormExtSpace: strip leading and trailing blanks (and, when
/// there are leading blanks, inner blank runs are kept as is).
fn norm_ext_space(value: &str) -> String {
    let b = value.as_bytes();
    let mut cur = 0;
    while cur < b.len() && is_blank_ch(b[cur]) {
        cur += 1;
    }
    if cur == 0 {
        // no leading blanks: cut at the start of the trailing blank run
        let mut out: Vec<u8> = Vec::new();
        let mut i = 0;
        loop {
            while i < b.len() && !is_blank_ch(b[i]) {
                i += 1;
            }
            if i >= b.len() {
                out.extend_from_slice(b);
                break;
            }
            let start = i;
            while i < b.len() && is_blank_ch(b[i]) {
                i += 1;
            }
            if i >= b.len() {
                out.extend_from_slice(&b[..start]);
                break;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    } else {
        // leading blanks: copy, collapsing nothing but dropping blank runs
        // except the char following each (libxml2 drops the blanks in
        // between words when copying in place)
        let mut out: Vec<u8> = Vec::new();
        loop {
            while cur < b.len() && !is_blank_ch(b[cur]) {
                out.push(b[cur]);
                cur += 1;
            }
            if cur >= b.len() {
                break;
            }
            while cur < b.len() && is_blank_ch(b[cur]) {
                cur += 1;
            }
            if cur >= b.len() {
                break;
            }
            out.push(b[cur]);
            cur += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
}

/// xmlRelaxNGNormalize: normalizeWhiteSpace()
fn normalize(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() && is_blank_ch(s[i]) {
        i += 1;
    }
    while i < s.len() {
        if is_blank_ch(s[i]) {
            while i < s.len() && is_blank_ch(s[i]) {
                i += 1;
            }
            if i >= s.len() {
                break;
            }
            out.push(b' ');
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

/// xmlURIEscapeStr(str, list)
fn uri_escape(s: &str, list: &[u8]) -> String {
    let mut out = String::new();
    for &ch in s.as_bytes() {
        let unreserved = ch.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&ch);
        if ch != b'@' && !unreserved && !list.contains(&ch) {
            out.push_str(&format!("%{:02X}", ch));
        } else {
            out.push(ch as char);
        }
    }
    out
}

/// xmlGetLineNo
fn get_line_no(t: &Tree, n: NodeId, depth: u32) -> i64 {
    if depth >= 5 {
        return -1;
    }
    let nd = &t.nodes[n];
    let big = nd
        .doc
        .and_then(|d| t.doc_info(d))
        .map(|i| i.parse_options & crate::parser::XML_PARSE_BIG_LINES != 0)
        .unwrap_or(false);
    match nd.t {
        NType::Element | NType::Text | NType::Comment | NType::Pi => {
            if nd.line < 65535 {
                return nd.line as i64;
            }
            let r = if nd.t == NType::Text && big {
                nd.line as i64
            } else if nd.t == NType::Element && nd.children.is_some() {
                get_line_no(t, nd.children.unwrap(), depth + 1)
            } else if let Some(x) = nd.next {
                get_line_no(t, x, depth + 1)
            } else if let Some(x) = nd.prev {
                get_line_no(t, x, depth + 1)
            } else {
                -1
            };
            if r == -1 || r == 65535 { 65535 } else { r }
        }
        _ => {
            if let Some(p) = nd.prev {
                if matches!(
                    t.nodes[p].t,
                    NType::Element | NType::Text | NType::Comment | NType::Pi
                ) {
                    return get_line_no(t, p, depth + 1);
                }
            }
            match nd.parent {
                Some(p) if t.nodes[p].t == NType::Element => get_line_no(t, p, depth + 1),
                _ => -1,
            }
        }
    }
}

/// An error as __xmlRaiseError builds it from a node (xmlVUpdateError):
/// the file and line come from the closest element ancestor.
fn node_error(domain: i32, code: i32, level: i32, node: Option<NodeId>, msg: String) -> XmlError {
    let (file, line) = match node {
        None => (None, 0),
        Some(n) => tree::with(|t| {
            let mut n = n;
            for _ in 0..10 {
                if t.nodes[n].t == NType::Element || t.nodes[n].parent.is_none() {
                    break;
                }
                n = t.nodes[n].parent.unwrap();
            }
            let file = t.nodes[n].doc.and_then(|d| t.doc_url(d));
            let mut line: i64 = 0;
            if t.nodes[n].t == NType::Element {
                line = t.nodes[n].line as i64;
            }
            if line == 0 || line == 65535 {
                line = get_line_no(t, n, 0);
            }
            (file, line as i32)
        }),
    };
    let formatted = vec![msg.trim_end_matches('\n').to_string()];
    XmlError {
        domain,
        code,
        level,
        message: msg,
        file,
        line,
        col: 0,
        formatted,
        no_ctx: false,
    }
}

// ---------------------------------------------------------------------------
// Datatype libraries

fn lookup_lib(ns: &str) -> Option<Lib> {
    match ns {
        XSD_DT_NS => Some(Lib::Xsd),
        RNG_NS => Some(Lib::Builtin),
        _ => None,
    }
}

/// The "have" function of a library.
fn lib_have(lib: Lib, typ: &str) -> i32 {
    match lib {
        Lib::Xsd => xst::predefined_type(typ, xst::XS_NS).is_some() as i32,
        Lib::Builtin => (typ == "string" || typ == "token") as i32,
    }
}

fn node_ctx(node: Option<NodeId>) -> Option<xst::TreeNode> {
    node.map(xst::TreeNode)
}

/// The "check" function of a library: 1 valid, 0 invalid, 2 ID error, -1
/// internal error.
fn lib_check(
    lib: Lib,
    typ: &str,
    value: &str,
    want_val: bool,
    node: Option<NodeId>,
) -> (i32, Option<xst::Val>) {
    match lib {
        Lib::Xsd => {
            let vt = match xst::predefined_type(typ, xst::XS_NS) {
                Some(v) => v,
                None => return (-1, None),
            };
            let nc = node_ctx(node);
            let (ret, val) = xst::val_predef_type_node(
                vt,
                value,
                want_val,
                nc.as_ref().map(|n| n as &dyn xst::NodeCtx),
            );
            let r = if ret == 2 {
                2
            } else if ret == 0 {
                1
            } else if ret > 0 {
                0
            } else {
                -1
            };
            (r, val)
        }
        Lib::Builtin => ((typ == "string" || typ == "token") as i32, None),
    }
}

/// The "comp" function of a library: 1 equal, 0 different, -1 error.
fn lib_compare(
    lib: Lib,
    typ: &str,
    value1: &str,
    node1: Option<NodeId>,
    comp1: Option<&xst::Val>,
    value2: &str,
    node2: Option<NodeId>,
) -> i32 {
    match lib {
        Lib::Xsd => {
            let vt = match xst::predefined_type(typ, xst::XS_NS) {
                Some(v) => v,
                None => return -1,
            };
            let res1 = match comp1 {
                Some(v) => v.clone(),
                None => {
                    let nc = node_ctx(node1);
                    let (ret, v) = xst::val_predef_type_node(
                        vt,
                        value1,
                        true,
                        nc.as_ref().map(|n| n as &dyn xst::NodeCtx),
                    );
                    if ret != 0 {
                        return -1;
                    }
                    match v {
                        Some(v) => v,
                        None => return -1,
                    }
                }
            };
            let nc = node_ctx(node2);
            let (ret, res2) = xst::val_predef_type_node(
                vt,
                value2,
                true,
                nc.as_ref().map(|n| n as &dyn xst::NodeCtx),
            );
            if ret != 0 {
                return -1;
            }
            let r = match res2 {
                Some(v2) => xst::compare_values(&res1, &v2),
                None => -2,
            };
            if r == -2 {
                -1
            } else if r == 0 {
                1
            } else {
                0
            }
        }
        Lib::Builtin => {
            if typ == "string" {
                (value1 == value2) as i32
            } else if typ == "token" {
                if value1 != value2 {
                    (normalize(value1.as_bytes()) == normalize(value2.as_bytes())) as i32
                } else {
                    1
                }
            } else {
                -1
            }
        }
    }
}

/// xmlSchemaNormalizeValue for a built-in type (None when unchanged)
fn normalize_builtin(vt: i32, value: &str) -> Option<String> {
    match vt {
        xst::STRING | xst::ANYSIMPLETYPE => None,
        xst::NORMSTRING => xst::whitespace_replace(value),
        _ => xst::collapse_string(value),
    }
}

/// An error of the temporary schema parser context xmlSchemaCheckFacet
/// uses (xmlSchemaSimpleTypeErr).
fn simple_type_err(errs: &mut Vec<XmlError>, code: i32, value: &str, vt: i32, list: bool) {
    let msg = format!(
        "'{}' is not a valid value of the {} type 'xs:{}'.\n",
        value,
        if list { "list" } else { "atomic" },
        xst::builtin_name(vt)
    );
    errs.push(node_error(xe::FROM_SCHEMASP, code, ERR_ERROR, None, msg));
}

/// xmlSchemaVCheckCVCSimpleType(pctxt, NULL, builtin type, value, &val,
/// fireErrors = 1, normalize = 1, isNormalized)
fn check_cvc_simple_type(
    errs: &mut Vec<XmlError>,
    vt: i32,
    value: &str,
    normalized: bool,
) -> (i32, Option<xst::Val>) {
    if xst::is_list(vt) {
        let norm = if normalized {
            None
        } else {
            normalize_builtin(vt, value)
        };
        let value = norm.as_deref().unwrap_or(value);
        let item = xst::list_item_type(vt).unwrap_or(xst::NMTOKEN);
        let mut ret = 0;
        for tok in value
            .split([' ', '\t', '\n', '\r'])
            .filter(|s| !s.is_empty())
        {
            let (r, _) = check_cvc_simple_type(errs, item, tok, true);
            if r != 0 {
                if r < 0 {
                    return (-1, None);
                }
                ret = xe::SCHEMAV_CVC_DATATYPE_VALID_1_2_2;
                break;
            }
        }
        if ret > 0 {
            let shown = xst::collapse_string(value).unwrap_or_else(|| value.to_string());
            simple_type_err(errs, ret, &shown, vt, true);
        }
        return (ret, None);
    }
    let norm = if normalized {
        None
    } else {
        normalize_builtin(vt, value)
    };
    let value = norm.as_deref().unwrap_or(value);
    let (mut ret, v) = xst::val_predef_type_node_no_norm(vt, value, true, None);
    if ret != 0 {
        if ret < 0 {
            return (-1, None);
        }
        ret = xe::SCHEMAV_CVC_DATATYPE_VALID_1_2_1;
        simple_type_err(errs, ret, value, vt, false);
    }
    (ret, v)
}

/// xmlSchemaCheckFacet for a built-in type as called from RelaxNG (only the
/// errors of the value checks and regexp compilation are reported): 0 when
/// the facet value is acceptable.
fn check_facet(facet: &mut xst::Facet, vt: i32, errs: &mut Vec<XmlError>) -> i32 {
    match facet.typ {
        xst::FACET_MININCLUSIVE
        | xst::FACET_MINEXCLUSIVE
        | xst::FACET_MAXINCLUSIVE
        | xst::FACET_MAXEXCLUSIVE
        | xst::FACET_ENUMERATION => {
            let (ret, v) = check_cvc_simple_type(errs, vt, &facet.value, false);
            if ret != 0 {
                return if ret < 0 { -1 } else { 1 };
            }
            facet.val = v;
            0
        }
        xst::FACET_PATTERN => match crate::xmlregexp::regexp_compile(&facet.value) {
            Ok(r) => {
                facet.regexp = Some(Rc::new(r));
                0
            }
            Err(msgs) => {
                // raised without any channel: only recorded
                for m in msgs {
                    let mut e = XmlError::simple(
                        xe::FROM_REGEXP,
                        xe::REGEXP_COMPILE_ERROR,
                        crate::parser::ERR_FATAL,
                        format!("{}\n", m),
                    );
                    e.formatted = Vec::new();
                    errs.push(e);
                }
                1
            }
        },
        xst::FACET_TOTALDIGITS
        | xst::FACET_FRACTIONDIGITS
        | xst::FACET_LENGTH
        | xst::FACET_MAXLENGTH
        | xst::FACET_MINLENGTH => {
            let bt = if facet.typ == xst::FACET_TOTALDIGITS {
                xst::PINTEGER
            } else {
                xst::NNINTEGER
            };
            let (ret, v) = xst::val_predef_type_node(bt, &facet.value, true, None);
            if ret != 0 {
                return if ret < 0 { -1 } else { 1 };
            }
            facet.val = v;
            0
        }
        xst::FACET_WHITESPACE => {
            facet.whitespace = match facet.value.as_str() {
                "preserve" => xst::WS_PRESERVE,
                "replace" => xst::WS_REPLACE,
                "collapse" => xst::WS_COLLAPSE,
                _ => return 1,
            };
            0
        }
        _ => 0,
    }
}

/// xmlRelaxNGSchemaFacetCheck: 0 if the value satisfies the facet.
fn lib_facet(
    lib: Lib,
    typ: &str,
    facetname: Option<&str>,
    val: Option<&str>,
    strval: &str,
    value: Option<&xst::Val>,
    errs: &mut Vec<XmlError>,
) -> Option<i32> {
    if lib != Lib::Xsd {
        return None;
    }
    let vt = match xst::predefined_type(typ, xst::XS_NS) {
        Some(v) => v,
        None => return Some(-1),
    };
    let ft = match facetname.unwrap_or("") {
        "minInclusive" => xst::FACET_MININCLUSIVE,
        "minExclusive" => xst::FACET_MINEXCLUSIVE,
        "maxInclusive" => xst::FACET_MAXINCLUSIVE,
        "maxExclusive" => xst::FACET_MAXEXCLUSIVE,
        "totalDigits" => xst::FACET_TOTALDIGITS,
        "fractionDigits" => xst::FACET_FRACTIONDIGITS,
        "pattern" => xst::FACET_PATTERN,
        "enumeration" => xst::FACET_ENUMERATION,
        "whiteSpace" => xst::FACET_WHITESPACE,
        "length" => xst::FACET_LENGTH,
        "maxLength" => xst::FACET_MAXLENGTH,
        "minLength" => xst::FACET_MINLENGTH,
        _ => return Some(-1),
    };
    let mut facet = xst::Facet::new(ft, val.unwrap_or(""));
    if check_facet(&mut facet, vt, errs) != 0 {
        return Some(-1);
    }
    let ret = xst::validate_facet(Some(vt), &facet, strval, value);
    Some(if ret != 0 { -1 } else { 0 })
}

// ---------------------------------------------------------------------------
// Parser context

/// Where errors raised while parsing a schema go (with the VM, so that the
/// caller can report them immediately).
pub type Sink<'a> = &'a mut dyn FnMut(&mut Vm, XmlError);

struct Docu {
    href: String,
    doc: NodeId,
    content: Option<DefId>,
}

struct Incl {
    href: String,
    doc: NodeId,
}

#[derive(Clone, Copy)]
enum Psvi {
    Inc(usize),
    Docu(usize),
}

struct PCtxt<'a> {
    vm: &'a mut Vm,
    sink: Sink<'a>,
    defs: Vec<Define>,
    grammars: Vec<Grammar>,
    grammar: Option<GId>,
    parentgrammar: Option<GId>,
    flags: i32,
    nb_errors: i32,
    def: Option<DefId>,
    nb_interleaves: i32,
    interleaves: Option<Vec<(String, DefId)>>,
    documents: Vec<Docu>,
    includes: Vec<Incl>,
    url: Option<String>,
    doc_tab: Vec<usize>,
    inc_tab: Vec<usize>,
    idref: bool,
    am: Option<Automata>,
    state: Option<StateId>,
    psvi: HashMap<NodeId, Psvi>,
}

impl<'a> PCtxt<'a> {
    fn new(vm: &'a mut Vm, sink: Sink<'a>) -> PCtxt<'a> {
        PCtxt {
            vm,
            sink,
            defs: Vec::new(),
            grammars: Vec::new(),
            grammar: None,
            parentgrammar: None,
            flags: 0,
            nb_errors: 0,
            def: None,
            nb_interleaves: 0,
            interleaves: None,
            documents: Vec::new(),
            includes: Vec::new(),
            url: None,
            doc_tab: Vec::new(),
            inc_tab: Vec::new(),
            idref: false,
            am: None,
            state: None,
            psvi: HashMap::new(),
        }
    }

    /// xmlRngPErr
    fn perr(&mut self, node: Option<NodeId>, code: i32, msg: String) {
        self.nb_errors += 1;
        let e = node_error(xe::FROM_RELAXNGP, code, ERR_ERROR, node, msg);
        (self.sink)(self.vm, e);
    }

    fn emit(&mut self, e: XmlError) {
        (self.sink)(self.vm, e);
    }

    fn new_def(&mut self, node: Option<NodeId>) -> DefId {
        self.defs.push(Define::new(node));
        self.defs.len() - 1
    }

    fn d(&self, id: DefId) -> &Define {
        &self.defs[id]
    }

    fn dm(&mut self, id: DefId) -> &mut Define {
        &mut self.defs[id]
    }

    fn new_grammar(&mut self) -> GId {
        self.grammars.push(Grammar::default());
        self.grammars.len() - 1
    }

    fn cur_grammar(&mut self) -> &mut Grammar {
        let g = self.grammar.expect("no current grammar");
        &mut self.grammars[g]
    }

    // -----------------------------------------------------------------
    // Documents

    /// xmlRelaxReadFile
    fn read_file(&mut self, url: &str) -> Option<NodeId> {
        let data = match crate::io::read_resource(self.vm, url) {
            Some(d) => d,
            None => {
                if !crate::io::last_failure_reported() {
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
                    self.emit(e);
                }
                return None;
            }
        };
        self.parse_bytes(&data, Some(url))
    }

    fn parse_bytes(&mut self, data: &[u8], url: Option<&str>) -> Option<NodeId> {
        let out = crate::sax2::parse_doc(data, url, 0);
        for e in out.errors {
            self.emit(e);
        }
        if !out.well_formed {
            return None;
        }
        out.doc
    }

    /// xmlRelaxNGRemoveRedefine
    fn remove_redefine(&mut self, target: Option<NodeId>, name: Option<&str>) -> bool {
        let mut found = false;
        let mut tmp = target;
        while let Some(c) = tmp {
            let tmp2 = nnext(c);
            if name.is_none() && is_rng(Some(c), "start") {
                found = true;
                unlink(c);
            } else if name.is_some() && is_rng(Some(c), "define") {
                if let Some(name2) = get_prop(c, "name") {
                    let name2 = norm_ext_space(&name2);
                    if Some(name2.as_str()) == name {
                        found = true;
                        unlink(c);
                    }
                }
            } else if is_rng(Some(c), "include") {
                let inc_doc = match self.psvi.get(&c) {
                    Some(Psvi::Inc(i)) => Some(self.includes[*i].doc),
                    Some(Psvi::Docu(i)) => Some(self.documents[*i].doc),
                    None => None,
                };
                if let Some(d) = inc_doc {
                    if let Some(first) = nchildren(d) {
                        if nname(first) == "grammar" {
                            let rc = root_element(d).and_then(nchildren);
                            if self.remove_redefine(rc, name) {
                                found = true;
                            }
                        }
                    }
                }
                if self.remove_redefine(nchildren(c), name) {
                    found = true;
                }
            }
            tmp = tmp2;
        }
        found
    }

    /// xmlRelaxNGLoadInclude
    fn load_include(&mut self, url: &str, node: NodeId, ns: Option<&str>) -> Option<usize> {
        for &i in &self.inc_tab {
            if self.includes[i].href == url {
                self.perr(
                    None,
                    xe::RNGP_INCLUDE_RECURSE,
                    format!("Detected an Include recursion for {}\n", url),
                );
                return None;
            }
        }
        let doc = match self.read_file(url) {
            Some(d) => d,
            None => {
                self.perr(
                    Some(node),
                    xe::RNGP_PARSE_ERROR,
                    format!("xmlRelaxNG: could not load {}\n", url),
                );
                return None;
            }
        };
        self.includes.push(Incl {
            href: url.to_string(),
            doc,
        });
        let idx = self.includes.len() - 1;
        if let Some(ns) = ns {
            if let Some(root) = root_element(doc) {
                if !has_prop(root, "ns") {
                    set_prop(root, "ns", ns);
                }
            }
        }
        self.inc_tab.push(idx);
        self.cleanup_doc(doc)?;
        self.inc_tab.pop();
        let root = match root_element(doc) {
            Some(r) => r,
            None => {
                self.perr(
                    Some(node),
                    xe::RNGP_EMPTY,
                    format!("xmlRelaxNG: included document is empty {}\n", url),
                );
                return None;
            }
        };
        if !is_rng(Some(root), "grammar") {
            self.perr(
                Some(node),
                xe::RNGP_GRAMMAR_MISSING,
                format!(
                    "xmlRelaxNG: included document {} root is not a grammar\n",
                    url
                ),
            );
            return None;
        }
        // elimination of redefined rules in the include
        let mut cur = nchildren(node);
        while let Some(c) = cur {
            if is_rng(Some(c), "start") {
                let found = self.remove_redefine(nchildren(root), None);
                if !found {
                    self.perr(
                        Some(node),
                        xe::RNGP_START_MISSING,
                        format!(
                            "xmlRelaxNG: include {} has a start but not the included grammar\n",
                            url
                        ),
                    );
                }
            } else if is_rng(Some(c), "define") {
                match get_prop(c, "name") {
                    None => self.perr(
                        Some(node),
                        xe::RNGP_NAME_MISSING,
                        format!("xmlRelaxNG: include {} has define without name\n", url),
                    ),
                    Some(name) => {
                        let name = norm_ext_space(&name);
                        let found = self.remove_redefine(nchildren(root), Some(&name));
                        if !found {
                            self.perr(Some(node), xe::RNGP_DEFINE_MISSING, format!("xmlRelaxNG: include {} has a define {} but not the included grammar\n", url, name));
                        }
                    }
                }
            }
            if is_rng(Some(c), "div") && nchildren(c).is_some() {
                cur = nchildren(c);
            } else if let Some(nx) = nnext(c) {
                cur = Some(nx);
            } else {
                let mut c = c;
                while nparent(c) != Some(node) && nparent(c).and_then(nnext).is_none() {
                    c = nparent(c).unwrap();
                }
                cur = if nparent(c) != Some(node) {
                    nparent(c).and_then(nnext)
                } else {
                    None
                };
            }
        }
        Some(idx)
    }

    /// xmlRelaxNGLoadExternalRef
    fn load_external_ref(&mut self, url: &str, ns: Option<&str>) -> Option<usize> {
        for &i in &self.doc_tab {
            if self.documents[i].href == url {
                self.perr(
                    None,
                    xe::RNGP_EXTERNALREF_RECURSE,
                    format!("Detected an externalRef recursion for {}\n", url),
                );
                return None;
            }
        }
        let doc = match self.read_file(url) {
            Some(d) => d,
            None => {
                self.perr(
                    None,
                    xe::RNGP_PARSE_ERROR,
                    format!("xmlRelaxNG: could not load {}\n", url),
                );
                return None;
            }
        };
        self.documents.push(Docu {
            href: url.to_string(),
            doc,
            content: None,
        });
        let idx = self.documents.len() - 1;
        if let Some(ns) = ns {
            if let Some(root) = root_element(doc) {
                if !has_prop(root, "ns") {
                    set_prop(root, "ns", ns);
                }
            }
        }
        self.doc_tab.push(idx);
        self.cleanup_doc(doc)?;
        self.doc_tab.pop();
        Some(idx)
    }

    /// xmlRelaxNGCleanupDoc
    fn cleanup_doc(&mut self, doc: NodeId) -> Option<NodeId> {
        let root = match root_element(doc) {
            Some(r) => r,
            None => {
                let u = self.url.clone().unwrap_or_else(|| "(null)".to_string());
                self.perr(
                    Some(doc),
                    xe::RNGP_EMPTY,
                    format!("xmlRelaxNGParse: {} is empty\n", u),
                );
                return None;
            }
        };
        self.cleanup_tree(root);
        Some(doc)
    }

    /// xmlRelaxNGCleanupAttributes
    fn cleanup_attributes(&mut self, node: NodeId) {
        let attrs = tree::with(|t| t.attrs_of(node));
        let nodename = nname(node);
        for a in attrs {
            let (aname, ans) = tree::with(|t| {
                (
                    t.nodes[a].name.clone(),
                    t.ns_href(t.nodes[a].ns).map(|s| s.to_string()),
                )
            });
            if !(ans.is_none() || ans.as_deref() == Some(RNG_NS)) {
                continue;
            }
            let forbidden = |allowed: &[&str]| !allowed.contains(&nodename.as_str());
            match aname.as_str() {
                "name" => {
                    if forbidden(&[
                        "element",
                        "attribute",
                        "ref",
                        "parentRef",
                        "param",
                        "define",
                    ]) {
                        self.perr(
                            Some(node),
                            xe::RNGP_FORBIDDEN_ATTRIBUTE,
                            format!("Attribute {} is not allowed on {}\n", aname, nodename),
                        );
                    }
                }
                "type" => {
                    if forbidden(&["value", "data"]) {
                        self.perr(
                            Some(node),
                            xe::RNGP_FORBIDDEN_ATTRIBUTE,
                            format!("Attribute {} is not allowed on {}\n", aname, nodename),
                        );
                    }
                }
                "href" => {
                    if forbidden(&["externalRef", "include"]) {
                        self.perr(
                            Some(node),
                            xe::RNGP_FORBIDDEN_ATTRIBUTE,
                            format!("Attribute {} is not allowed on {}\n", aname, nodename),
                        );
                    }
                }
                "combine" => {
                    if forbidden(&["start", "define"]) {
                        self.perr(
                            Some(node),
                            xe::RNGP_FORBIDDEN_ATTRIBUTE,
                            format!("Attribute {} is not allowed on {}\n", aname, nodename),
                        );
                    }
                }
                "datatypeLibrary" => {
                    let val = get_content(a);
                    if !val.is_empty() {
                        match crate::uri::parse_uri(&val) {
                            None => self.perr(
                                Some(node),
                                xe::RNGP_INVALID_URI,
                                format!("Attribute {} contains invalid URI {}\n", aname, val),
                            ),
                            Some(uri) => {
                                if uri.scheme.is_none() {
                                    self.perr(
                                        Some(node),
                                        xe::RNGP_URI_NOT_ABSOLUTE,
                                        format!(
                                            "Attribute {} URI {} is not absolute\n",
                                            aname, val
                                        ),
                                    );
                                }
                                if uri.fragment.is_some() {
                                    self.perr(
                                        Some(node),
                                        xe::RNGP_URI_FRAGMENT,
                                        format!(
                                            "Attribute {} URI {} has a fragment ID\n",
                                            aname, val
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
                "ns" => {}
                _ => self.perr(
                    Some(node),
                    xe::RNGP_UNKNOWN_ATTRIBUTE,
                    format!("Unknown attribute {} on {}\n", aname, nodename),
                ),
            }
        }
    }

    /// The "ns" attribute inherited from the closest ancestor element.
    fn inherited_ns(start: Option<NodeId>) -> Option<String> {
        let mut tmp = start;
        while let Some(t) = tmp {
            if ntype(t) != NType::Element {
                break;
            }
            if let Some(ns) = get_prop(t, "ns") {
                return Some(ns);
            }
            tmp = nparent(t);
        }
        None
    }

    /// xmlRelaxNGCleanupTree
    fn cleanup_tree(&mut self, root: NodeId) {
        let mut delete: Option<NodeId> = None;
        let mut cur = Some(root);
        while let Some(c) = cur {
            if let Some(d) = delete.take() {
                unlink(d);
            }
            let mut skip = false;
            let ty = ntype(c);
            if ty == NType::Element {
                if nns(c).as_deref() != Some(RNG_NS) {
                    let p = nparent(c);
                    if let Some(p) = p {
                        if ntype(p) == NType::Element {
                            let pn = nname(p);
                            if pn == "name" || pn == "value" || pn == "param" {
                                self.perr(
                                    Some(c),
                                    xe::RNGP_FOREIGN_ELEMENT,
                                    format!("element {} doesn't allow foreign elements\n", pn),
                                );
                            }
                        }
                    }
                    delete = Some(c);
                    skip = true;
                } else {
                    self.cleanup_attributes(c);
                    let name = nname(c);
                    if name == "externalRef" {
                        let mut ns = get_prop(c, "ns");
                        if ns.is_none() {
                            ns = Self::inherited_ns(nparent(c));
                        }
                        match get_prop(c, "href") {
                            None => {
                                self.perr(
                                    Some(c),
                                    xe::RNGP_MISSING_HREF,
                                    "xmlRelaxNGParse: externalRef has no href attribute\n"
                                        .to_string(),
                                );
                                delete = Some(c);
                                skip = true;
                            }
                            Some(href) => {
                                match crate::uri::parse_uri(&href) {
                                    None => {
                                        self.perr(
                                            Some(c),
                                            xe::RNGP_HREF_ERROR,
                                            format!("Incorrect URI for externalRef {}\n", href),
                                        );
                                        delete = Some(c);
                                        skip = true;
                                    }
                                    Some(u) if u.fragment.is_some() => {
                                        self.perr(
                                            Some(c),
                                            xe::RNGP_HREF_ERROR,
                                            format!(
                                                "Fragment forbidden in URI for externalRef {}\n",
                                                href
                                            ),
                                        );
                                        delete = Some(c);
                                        skip = true;
                                    }
                                    Some(_) => {
                                        let base =
                                            tree::with(|t| crate::dom::node::node_get_base(t, c));
                                        match crate::uri::build_uri(&href, base.as_deref()) {
                                            None => {
                                                self.perr(Some(c), xe::RNGP_HREF_ERROR, format!("Failed to compute URL for externalRef {}\n", href));
                                                delete = Some(c);
                                                skip = true;
                                            }
                                            Some(url) => {
                                                match self.load_external_ref(&url, ns.as_deref()) {
                                                    None => {
                                                        self.perr(
                                                            Some(c),
                                                            xe::RNGP_EXTERNAL_REF_FAILURE,
                                                            format!(
                                                                "Failed to load externalRef {}\n",
                                                                url
                                                            ),
                                                        );
                                                        delete = Some(c);
                                                        skip = true;
                                                    }
                                                    Some(d) => {
                                                        self.psvi.insert(c, Psvi::Docu(d));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if name == "include" {
                        match get_prop(c, "href") {
                            None => {
                                self.perr(
                                    Some(c),
                                    xe::RNGP_MISSING_HREF,
                                    "xmlRelaxNGParse: include has no href attribute\n".to_string(),
                                );
                                delete = Some(c);
                                skip = true;
                            }
                            Some(href) => {
                                let base = tree::with(|t| crate::dom::node::node_get_base(t, c));
                                match crate::uri::build_uri(&href, base.as_deref()) {
                                    None => {
                                        self.perr(
                                            Some(c),
                                            xe::RNGP_HREF_ERROR,
                                            format!("Failed to compute URL for include {}\n", href),
                                        );
                                        delete = Some(c);
                                        skip = true;
                                    }
                                    Some(url) => {
                                        let mut ns = get_prop(c, "ns");
                                        if ns.is_none() {
                                            ns = Self::inherited_ns(nparent(c));
                                        }
                                        match self.load_include(&url, c, ns.as_deref()) {
                                            None => {
                                                self.perr(
                                                    Some(c),
                                                    xe::RNGP_INCLUDE_FAILURE,
                                                    format!("Failed to load include {}\n", url),
                                                );
                                                delete = Some(c);
                                                skip = true;
                                            }
                                            Some(i) => {
                                                self.psvi.insert(c, Psvi::Inc(i));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if name == "element" || name == "attribute" {
                        if let Some(nm) = get_prop(c, "name") {
                            let text = tree::with(|t| {
                                let doc = t.nodes[c].doc;
                                let ns = t.nodes[c].ns;
                                let node = t.new_element(doc, "name");
                                t.nodes[node].ns = ns;
                                let txt = t.new_text(doc, &nm);
                                t.link_child_raw(node, txt);
                                match t.nodes[c].children {
                                    None => {
                                        t.link_child_raw(c, node);
                                    }
                                    Some(first) => {
                                        t.add_prev_sibling(first, node);
                                    }
                                }
                                node
                            });
                            unset_prop(c, "name");
                            if let Some(ns) = get_prop(c, "ns") {
                                set_prop(text, "ns", &ns);
                            } else if name == "attribute" {
                                set_prop(text, "ns", "");
                            }
                        }
                    } else if name == "name" || name == "nsName" || name == "value" {
                        if !has_prop(c, "ns") {
                            let ns = Self::inherited_ns(nparent(c));
                            set_prop(c, "ns", ns.as_deref().unwrap_or(""));
                        }
                        if name == "name" {
                            let qn = get_content(c);
                            if let Some((prefix, local)) = xst::split_qname2(&qn) {
                                let href = tree::with(|t| {
                                    t.search_ns(c, Some(&prefix)).map(|n| t.nss[n].href.clone())
                                });
                                match href {
                                    None => self.perr(
                                        Some(c),
                                        xe::RNGP_PREFIX_UNDEFINED,
                                        format!(
                                            "xmlRelaxNGParse: no namespace for prefix {}\n",
                                            prefix
                                        ),
                                    ),
                                    Some(h) => {
                                        set_prop(c, "ns", &h);
                                        tree::with(|t| t.node_set_content(c, &local));
                                    }
                                }
                            }
                        }
                        if name == "nsName" && self.flags & IN_NSEXCEPT != 0 {
                            self.perr(
                                Some(c),
                                xe::RNGP_PAT_NSNAME_EXCEPT_NSNAME,
                                "Found nsName/except//nsName forbidden construct\n".to_string(),
                            );
                        }
                    } else if name == "except" && c != root {
                        let oldflags = self.flags;
                        let pn = nparent(c).map(nname);
                        if pn.as_deref() == Some("anyName") {
                            self.flags |= IN_ANYEXCEPT;
                            self.cleanup_tree(c);
                            self.flags = oldflags;
                            skip = true;
                        } else if pn.as_deref() == Some("nsName") {
                            self.flags |= IN_NSEXCEPT;
                            self.cleanup_tree(c);
                            self.flags = oldflags;
                            skip = true;
                        }
                    } else if name == "anyName" {
                        if self.flags & IN_ANYEXCEPT != 0 {
                            self.perr(
                                Some(c),
                                xe::RNGP_PAT_ANYNAME_EXCEPT_ANYNAME,
                                "Found anyName/except//anyName forbidden construct\n".to_string(),
                            );
                        } else if self.flags & IN_NSEXCEPT != 0 {
                            self.perr(
                                Some(c),
                                xe::RNGP_PAT_NSNAME_EXCEPT_ANYNAME,
                                "Found nsName/except//anyName forbidden construct\n".to_string(),
                            );
                        }
                    }
                    if !skip && name == "div" {
                        let ns = get_prop(c, "ns");
                        let mut child = nchildren(c);
                        let mut ins = c;
                        while let Some(ch) = child {
                            if let Some(ns) = &ns {
                                if ntype(ch) == NType::Element && !has_prop(ch, "ns") {
                                    set_prop(ch, "ns", ns);
                                }
                            }
                            let tmp = nnext(ch);
                            ins = tree::with(|t| {
                                t.unlink(ch);
                                t.add_next_sibling(ins, ch)
                            });
                            child = tmp;
                        }
                        tree::with(|t| {
                            if let Some(p) = t.nodes[c].parent {
                                let defs = std::mem::take(&mut t.nodes[c].ns_def);
                                t.nodes[p].ns_def.extend(defs);
                            }
                        });
                        delete = Some(c);
                        skip = true;
                    }
                }
            } else if ty == NType::Text || ty == NType::CData {
                if is_blank_node(c) {
                    let p = nparent(c);
                    if is_element(p) {
                        let pn = nname(p.unwrap());
                        if pn != "value" && pn != "param" {
                            delete = Some(c);
                        }
                    } else {
                        delete = Some(c);
                        skip = true;
                    }
                }
            } else {
                delete = Some(c);
                skip = true;
            }
            if !skip {
                if let Some(ch) = nchildren(c) {
                    if !matches!(
                        ntype(ch),
                        NType::EntityDecl | NType::EntityRef | NType::Entity
                    ) {
                        cur = Some(ch);
                        continue;
                    }
                }
            }
            // skip_children:
            if let Some(nx) = nnext(c) {
                cur = Some(nx);
                continue;
            }
            let mut p = Some(c);
            cur = None;
            while let Some(x) = p {
                p = nparent(x);
                match p {
                    None => break,
                    Some(pp) if pp == root => break,
                    Some(pp) => {
                        if let Some(nx) = nnext(pp) {
                            cur = Some(nx);
                            break;
                        }
                    }
                }
            }
        }
        if let Some(d) = delete {
            unlink(d);
        }
    }
}

impl<'a> PCtxt<'a> {
    // -----------------------------------------------------------------
    // Parsing

    /// xmlRelaxNGGetDataTypeLibrary
    fn datatype_library(node: NodeId) -> Option<String> {
        let check = |n: NodeId| -> Option<Option<String>> {
            let ret = get_prop(n, "datatypeLibrary")?;
            if ret.is_empty() {
                return Some(None);
            }
            Some(Some(uri_escape(&ret, b":/#?")))
        };
        if is_rng(Some(node), "data") || is_rng(Some(node), "value") {
            if let Some(r) = check(node) {
                return r;
            }
        }
        let mut n = nparent(node);
        while let Some(x) = n {
            if ntype(x) != NType::Element {
                break;
            }
            if let Some(r) = check(x) {
                return r;
            }
            n = nparent(x);
        }
        None
    }

    /// Library lookup and "have" check shared by value and data.
    fn setup_type(
        &mut self,
        node: NodeId,
        def: DefId,
        typ: &str,
        library: &str,
    ) -> (Option<Lib>, i32) {
        let lib = lookup_lib(library);
        let mut success = 0;
        match lib {
            None => {
                self.perr(
                    Some(node),
                    xe::RNGP_UNKNOWN_TYPE_LIB,
                    format!("Use of unregistered type library '{}'\n", library),
                );
                self.dm(def).lib = None;
            }
            Some(l) => {
                self.dm(def).lib = Some(l);
                success = lib_have(l, typ);
                if success != 1 {
                    self.perr(
                        Some(node),
                        xe::RNGP_TYPE_NOT_FOUND,
                        format!(
                            "Error type '{}' is not exported by type library '{}'\n",
                            typ, library
                        ),
                    );
                }
            }
        }
        (lib, success)
    }

    /// xmlRelaxNGParseValue
    fn parse_value(&mut self, node: NodeId) -> Option<DefId> {
        let def = self.new_def(Some(node));
        self.dm(def).typ = T::Value;
        let mut lib = None;
        let mut success = 0;
        if let Some(typ) = get_prop(node, "type") {
            let typ = norm_ext_space(&typ);
            if xst::validate_ncname(&typ, false) != 0 {
                self.perr(
                    Some(node),
                    xe::RNGP_TYPE_VALUE,
                    format!("value type '{}' is not an NCName\n", typ),
                );
            }
            let library = Self::datatype_library(node).unwrap_or_else(|| RNG_NS.to_string());
            self.dm(def).name = Some(typ.clone());
            self.dm(def).ns = Some(library.clone());
            let (l, s) = self.setup_type(node, def, &typ, &library);
            lib = l;
            success = s;
        }
        match nchildren(node) {
            None => self.dm(def).value = Some(String::new()),
            Some(ch) => {
                if !is_text_like(ch) || nnext(ch).is_some() {
                    self.perr(
                        Some(node),
                        xe::RNGP_TEXT_EXPECTED,
                        "Expecting a single text value for <value>content\n".to_string(),
                    );
                } else {
                    let value = get_content(node);
                    self.dm(def).value = Some(value.clone());
                    if let (Some(l), 1) = (lib, success) {
                        let name = self.d(def).name.clone().unwrap_or_default();
                        let (s, val) = lib_check(l, &name, &value, true, Some(node));
                        if s != 1 {
                            self.perr(
                                Some(node),
                                xe::RNGP_INVALID_VALUE,
                                format!(
                                    "Value '{}' is not acceptable for type '{}'\n",
                                    value, name
                                ),
                            );
                        } else if val.is_some() {
                            self.dm(def).val = val;
                        }
                    }
                }
            }
        }
        Some(def)
    }

    /// xmlRelaxNGParseData
    fn parse_data(&mut self, node: NodeId) -> Option<DefId> {
        let typ = match get_prop(node, "type") {
            None => {
                self.perr(
                    Some(node),
                    xe::RNGP_TYPE_MISSING,
                    "data has no type\n".to_string(),
                );
                return None;
            }
            Some(t) => norm_ext_space(&t),
        };
        if xst::validate_ncname(&typ, false) != 0 {
            self.perr(
                Some(node),
                xe::RNGP_TYPE_VALUE,
                format!("data type '{}' is not an NCName\n", typ),
            );
        }
        let library = Self::datatype_library(node).unwrap_or_else(|| RNG_NS.to_string());
        let def = self.new_def(Some(node));
        self.dm(def).typ = T::Datatype;
        self.dm(def).name = Some(typ.clone());
        self.dm(def).ns = Some(library.clone());
        let (_lib, success) = self.setup_type(node, def, &typ, &library);
        if success == 1 && library == XSD_DT_NS && (typ == "IDREF" || typ == "IDREFS") {
            self.idref = true;
        }
        let mut content = nchildren(node);
        let mut lastparam: Option<DefId> = None;
        while let Some(c) = content {
            if nname(c) != "param" {
                break;
            }
            if library == RNG_NS {
                self.perr(
                    Some(node),
                    xe::RNGP_PARAM_FORBIDDEN,
                    format!(
                        "Type library '{}' does not allow type parameters\n",
                        library
                    ),
                );
                content = nnext(c);
                while let Some(x) = content {
                    if nname(x) != "param" {
                        break;
                    }
                    content = nnext(x);
                }
            } else {
                let param = self.new_def(Some(node));
                self.dm(param).typ = T::Param;
                let pname = get_prop(c, "name");
                if pname.is_none() {
                    self.perr(
                        Some(node),
                        xe::RNGP_PARAM_NAME_MISSING,
                        "param has no name\n".to_string(),
                    );
                }
                self.dm(param).name = pname;
                self.dm(param).value = Some(get_content(c));
                match lastparam {
                    None => self.dm(def).attrs = Some(param),
                    Some(l) => self.dm(l).next = Some(param),
                }
                lastparam = Some(param);
                content = nnext(c);
            }
        }
        if let Some(c) = content {
            if nname(c) == "except" {
                let except = self.new_def(Some(node));
                self.dm(except).typ = T::Except;
                let mut child = nchildren(c);
                self.dm(def).content = Some(except);
                if child.is_none() {
                    self.perr(
                        Some(c),
                        xe::RNGP_EXCEPT_NO_CONTENT,
                        "except has no content\n".to_string(),
                    );
                }
                let mut last: Option<DefId> = None;
                while let Some(ch) = child {
                    if let Some(tmp2) = self.parse_pattern(ch) {
                        match last {
                            None => self.dm(except).content = Some(tmp2),
                            Some(l) => self.dm(l).next = Some(tmp2),
                        }
                        last = Some(tmp2);
                    }
                    child = nnext(ch);
                }
                content = nnext(c);
            }
        }
        if let Some(c) = content {
            self.perr(
                Some(c),
                xe::RNGP_DATA_CONTENT,
                format!("Element data has unexpected content {}\n", nname(c)),
            );
        }
        Some(def)
    }

    /// xmlRelaxNGParseInterleave
    fn parse_interleave(&mut self, node: NodeId) -> Option<DefId> {
        let def = self.new_def(Some(node));
        self.dm(def).typ = T::Interleave;
        let name = format!("interleave{}", self.nb_interleaves);
        self.nb_interleaves += 1;
        self.interleaves
            .get_or_insert_with(Vec::new)
            .push((name, def));
        let mut child = nchildren(node);
        if child.is_none() {
            self.perr(
                Some(node),
                xe::RNGP_INTERLEAVE_NO_CONTENT,
                "Element interleave is empty\n".to_string(),
            );
        }
        let mut last: Option<DefId> = None;
        while let Some(ch) = child {
            let cur = if is_rng(Some(ch), "element") {
                self.parse_element(ch)
            } else {
                self.parse_pattern(ch)
            };
            if let Some(cur) = cur {
                self.dm(cur).parent = Some(def);
                match last {
                    None => self.dm(def).content = Some(cur),
                    Some(l) => self.dm(l).next = Some(cur),
                }
                last = Some(cur);
            }
            child = nnext(ch);
        }
        Some(def)
    }

    /// xmlRelaxNGParseInclude
    fn parse_include(&mut self, node: NodeId) -> i32 {
        let incl = match self.psvi.get(&node) {
            Some(Psvi::Inc(i)) => self.includes[*i].doc,
            Some(Psvi::Docu(i)) => self.documents[*i].doc,
            None => {
                self.perr(
                    Some(node),
                    xe::RNGP_INCLUDE_EMPTY,
                    "Include node has no data\n".to_string(),
                );
                return -1;
            }
        };
        let root = match root_element(incl) {
            Some(r) => r,
            None => {
                self.perr(
                    Some(node),
                    xe::RNGP_EMPTY,
                    "Include document is empty\n".to_string(),
                );
                return -1;
            }
        };
        if nname(root) != "grammar" {
            self.perr(
                Some(node),
                xe::RNGP_GRAMMAR_MISSING,
                "Include document root is not a grammar\n".to_string(),
            );
            return -1;
        }
        let mut ret = 0;
        if let Some(rc) = nchildren(root) {
            if self.parse_grammar_content(Some(rc)) != 0 {
                ret = -1;
            }
        }
        if let Some(nc) = nchildren(node) {
            if self.parse_grammar_content(Some(nc)) != 0 {
                ret = -1;
            }
        }
        ret
    }

    /// xmlRelaxNGParseDefine
    fn parse_define(&mut self, node: NodeId) -> i32 {
        let mut ret = 0;
        match get_prop(node, "name") {
            None => self.perr(
                Some(node),
                xe::RNGP_DEFINE_NAME_MISSING,
                "define has no name\n".to_string(),
            ),
            Some(name) => {
                let name = norm_ext_space(&name);
                if xst::validate_ncname(&name, false) != 0 {
                    self.perr(
                        Some(node),
                        xe::RNGP_INVALID_DEFINE_NAME,
                        format!("define name '{}' is not an NCName\n", name),
                    );
                }
                let def = self.new_def(Some(node));
                self.dm(def).typ = T::Def;
                self.dm(def).name = Some(name.clone());
                match nchildren(node) {
                    None => self.perr(
                        Some(node),
                        xe::RNGP_DEFINE_EMPTY,
                        "define has no children\n".to_string(),
                    ),
                    Some(ch) => {
                        let c = self.parse_patterns(Some(ch), false);
                        self.dm(def).content = c;
                    }
                }
                let res = {
                    let defs = self.cur_grammar().defs.get_or_insert_with(OMap::default);
                    if defs.add(Some(&name), def) < 0 {
                        Some(defs.lookup(Some(&name)))
                    } else {
                        None
                    }
                };
                if let Some(res) = res {
                    match res {
                        None => {
                            self.perr(
                                Some(node),
                                xe::RNGP_DEFINE_CREATE_FAILED,
                                format!("Internal error on define aggregation of {}\n", name),
                            );
                            ret = -1;
                        }
                        Some(mut prev) => {
                            while let Some(n) = self.d(prev).next_hash {
                                prev = n;
                            }
                            self.dm(prev).next_hash = Some(def);
                        }
                    }
                }
            }
        }
        ret
    }

    /// xmlRelaxNGParseImportRefs
    fn parse_import_refs(&mut self, grammar: GId) {
        let refs = match &self.grammars[grammar].refs {
            None => return,
            Some(r) => r.0.clone(),
        };
        if self.cur_grammar().refs.is_none() {
            self.cur_grammar().refs = Some(OMap::default());
        }
        for (name, def) in refs {
            self.dm(def).dflags |= IS_EXTERNAL_REF;
            let tmp = self
                .cur_grammar()
                .refs
                .as_mut()
                .unwrap()
                .add(Some(&name), def);
            if tmp < 0 {
                let dname = self.d(def).name.clone();
                let prev = self
                    .cur_grammar()
                    .refs
                    .as_ref()
                    .unwrap()
                    .lookup(dname.as_deref());
                match prev {
                    None => match dname {
                        Some(n) => self.perr(
                            None,
                            xe::RNGP_REF_CREATE_FAILED,
                            format!("Error refs definitions '{}'\n", n),
                        ),
                        None => self.perr(
                            None,
                            xe::RNGP_REF_CREATE_FAILED,
                            "Error refs definitions\n".to_string(),
                        ),
                    },
                    Some(prev) => {
                        let pn = self.d(prev).next_hash;
                        self.dm(def).next_hash = pn;
                        self.dm(prev).next_hash = Some(def);
                    }
                }
            }
        }
    }

    /// xmlRelaxNGProcessExternalRef
    fn process_external_ref(&mut self, node: NodeId) -> Option<DefId> {
        let docu = match self.psvi.get(&node) {
            Some(Psvi::Docu(i)) => *i,
            _ => return None,
        };
        let def = self.new_def(Some(node));
        self.dm(def).typ = T::ExternalRef;
        if self.documents[docu].content.is_none() {
            let root = match root_element(self.documents[docu].doc) {
                Some(r) => r,
                None => {
                    let u = self.url.clone().unwrap_or_else(|| "(null)".to_string());
                    self.perr(
                        Some(node),
                        xe::RNGP_EXTERNALREF_EMTPY,
                        format!("xmlRelaxNGParse: {} is empty\n", u),
                    );
                    return None;
                }
            };
            let mut new_ns = false;
            if get_prop(root, "ns").is_none() {
                if let Some(ns) = Self::inherited_ns(Some(node)) {
                    set_prop(root, "ns", &ns);
                    new_ns = true;
                }
            }
            let oldflags = self.flags;
            self.flags |= IN_EXTERNALREF;
            let top = self.parse_document(root);
            self.flags = oldflags;
            if let Some(top) = top {
                self.documents[docu].content = self.grammars[top].start;
                if self.grammars[top].refs.is_some() {
                    self.parse_import_refs(top);
                }
            }
            if new_ns {
                unset_prop(root, "ns");
            }
        }
        let c = self.documents[docu].content;
        self.dm(def).content = c;
        Some(def)
    }

    fn add_ref(&mut self, g: GId, node: NodeId, def: DefId, parent_ref: bool) -> Option<DefId> {
        let name = self.d(def).name.clone();
        let res = {
            let refs = self.grammars[g].refs.get_or_insert_with(OMap::default);
            if refs.add(name.as_deref(), def) < 0 {
                Some(refs.lookup(name.as_deref()))
            } else {
                None
            }
        };
        if let Some(res) = res {
            match res {
                None => {
                    if parent_ref {
                        self.perr(
                            Some(node),
                            xe::RNGP_PARENTREF_CREATE_FAILED,
                            format!(
                                "Internal error parentRef definitions '{}'\n",
                                name.unwrap_or_default()
                            ),
                        );
                    } else {
                        match name {
                            Some(n) => self.perr(
                                Some(node),
                                xe::RNGP_REF_CREATE_FAILED,
                                format!("Error refs definitions '{}'\n", n),
                            ),
                            None => self.perr(
                                Some(node),
                                xe::RNGP_REF_CREATE_FAILED,
                                "Error refs definitions\n".to_string(),
                            ),
                        }
                    }
                    return None;
                }
                Some(prev) => {
                    let pn = self.d(prev).next_hash;
                    self.dm(def).next_hash = pn;
                    self.dm(prev).next_hash = Some(def);
                }
            }
        }
        Some(def)
    }

    fn simple_container(&mut self, node: NodeId, typ: T, group: bool) -> DefId {
        let def = self.new_def(Some(node));
        self.dm(def).typ = typ;
        match nchildren(node) {
            None => self.perr(
                Some(node),
                xe::RNGP_EMPTY_CONSTRUCT,
                format!("Element {} is empty\n", nname(node)),
            ),
            Some(ch) => {
                let c = self.parse_patterns(Some(ch), group);
                self.dm(def).content = c;
            }
        }
        def
    }

    /// xmlRelaxNGParsePattern
    fn parse_pattern(&mut self, node: NodeId) -> Option<DefId> {
        let n = Some(node);
        if is_rng(n, "element") {
            self.parse_element(node)
        } else if is_rng(n, "attribute") {
            self.parse_attribute(node)
        } else if is_rng(n, "empty") {
            let def = self.new_def(n);
            self.dm(def).typ = T::Empty;
            if nchildren(node).is_some() {
                self.perr(
                    n,
                    xe::RNGP_EMPTY_NOT_EMPTY,
                    "empty: had a child node\n".to_string(),
                );
            }
            Some(def)
        } else if is_rng(n, "text") {
            let def = self.new_def(n);
            self.dm(def).typ = T::Text;
            if nchildren(node).is_some() {
                self.perr(
                    n,
                    xe::RNGP_TEXT_HAS_CHILD,
                    "text: had a child node\n".to_string(),
                );
            }
            Some(def)
        } else if is_rng(n, "zeroOrMore") {
            Some(self.simple_container(node, T::ZeroOrMore, true))
        } else if is_rng(n, "oneOrMore") {
            Some(self.simple_container(node, T::OneOrMore, true))
        } else if is_rng(n, "optional") {
            Some(self.simple_container(node, T::Optional, true))
        } else if is_rng(n, "choice") {
            Some(self.simple_container(node, T::Choice, false))
        } else if is_rng(n, "group") {
            Some(self.simple_container(node, T::Group, false))
        } else if is_rng(n, "ref") {
            let def = self.new_def(n);
            self.dm(def).typ = T::Ref;
            let name = get_prop(node, "name");
            match name {
                None => self.perr(n, xe::RNGP_REF_NO_NAME, "ref has no name\n".to_string()),
                Some(nm) => {
                    let nm = norm_ext_space(&nm);
                    if xst::validate_ncname(&nm, false) != 0 {
                        self.perr(
                            n,
                            xe::RNGP_REF_NAME_INVALID,
                            format!("ref name '{}' is not an NCName\n", nm),
                        );
                    }
                    self.dm(def).name = Some(nm);
                }
            }
            if nchildren(node).is_some() {
                self.perr(n, xe::RNGP_REF_NOT_EMPTY, "ref is not empty\n".to_string());
            }
            let g = self.grammar.expect("grammar");
            self.add_ref(g, node, def, false)
        } else if is_rng(n, "data") {
            self.parse_data(node)
        } else if is_rng(n, "value") {
            self.parse_value(node)
        } else if is_rng(n, "list") {
            Some(self.simple_container(node, T::List, false))
        } else if is_rng(n, "interleave") {
            self.parse_interleave(node)
        } else if is_rng(n, "externalRef") {
            self.process_external_ref(node)
        } else if is_rng(n, "notAllowed") {
            let def = self.new_def(n);
            self.dm(def).typ = T::NotAllowed;
            if nchildren(node).is_some() {
                self.perr(
                    n,
                    xe::RNGP_NOTALLOWED_NOT_EMPTY,
                    "xmlRelaxNGParse: notAllowed element is not empty\n".to_string(),
                );
            }
            Some(def)
        } else if is_rng(n, "grammar") {
            let oldparent = self.parentgrammar;
            let old = self.grammar;
            self.parentgrammar = old;
            let g = self.parse_grammar(nchildren(node));
            if old.is_some() {
                self.grammar = old;
                self.parentgrammar = oldparent;
            }
            self.grammars[g].start
        } else if is_rng(n, "parentRef") {
            let pg = match self.parentgrammar {
                None => {
                    self.perr(
                        n,
                        xe::RNGP_PARENTREF_NO_PARENT,
                        "Use of parentRef without a parent grammar\n".to_string(),
                    );
                    return None;
                }
                Some(p) => p,
            };
            let def = self.new_def(n);
            self.dm(def).typ = T::ParentRef;
            match get_prop(node, "name") {
                None => self.perr(
                    n,
                    xe::RNGP_PARENTREF_NO_NAME,
                    "parentRef has no name\n".to_string(),
                ),
                Some(nm) => {
                    let nm = norm_ext_space(&nm);
                    if xst::validate_ncname(&nm, false) != 0 {
                        self.perr(
                            n,
                            xe::RNGP_PARENTREF_NAME_INVALID,
                            format!("parentRef name '{}' is not an NCName\n", nm),
                        );
                    }
                    self.dm(def).name = Some(nm);
                }
            }
            if nchildren(node).is_some() {
                self.perr(
                    n,
                    xe::RNGP_PARENTREF_NOT_EMPTY,
                    "parentRef is not empty\n".to_string(),
                );
            }
            if self.grammars[pg].refs.is_none() {
                self.grammars[pg].refs = Some(OMap::default());
            }
            if self.d(def).name.is_some() {
                return self.add_ref(pg, node, def, true);
            }
            Some(def)
        } else if is_rng(n, "mixed") {
            if nchildren(node).is_none() {
                self.perr(n, xe::RNGP_EMPTY_CONSTRUCT, "Mixed is empty\n".to_string());
                return None;
            }
            let def = self.parse_interleave(node)?;
            if let Some(c) = self.d(def).content {
                if self.d(c).next.is_some() {
                    let tmp = self.new_def(n);
                    self.dm(tmp).typ = T::Group;
                    self.dm(tmp).content = Some(c);
                    self.dm(def).content = Some(tmp);
                }
            }
            let tmp = self.new_def(n);
            self.dm(tmp).typ = T::Text;
            let c = self.d(def).content;
            self.dm(tmp).next = c;
            self.dm(def).content = Some(tmp);
            Some(def)
        } else {
            self.perr(
                n,
                xe::RNGP_UNKNOWN_CONSTRUCT,
                format!("Unexpected node {} is not a pattern\n", nname(node)),
            );
            None
        }
    }

    /// xmlRelaxNGParseAttribute
    fn parse_attribute(&mut self, node: NodeId) -> Option<DefId> {
        let ret = self.new_def(Some(node));
        self.dm(ret).typ = T::Attribute;
        let pd = self.def;
        self.dm(ret).parent = pd;
        let mut child = nchildren(node);
        let c0 = match child {
            None => {
                self.perr(
                    Some(node),
                    xe::RNGP_ATTRIBUTE_EMPTY,
                    "xmlRelaxNGParseattribute: attribute has no children\n".to_string(),
                );
                return Some(ret);
            }
            Some(c) => c,
        };
        let old_flags = self.flags;
        self.flags |= IN_ATTRIBUTE;
        if self.parse_name_class(c0, ret).is_some() {
            child = nnext(c0);
        }
        if let Some(ch) = child {
            if let Some(cur) = self.parse_pattern(ch) {
                match self.d(cur).typ {
                    T::Start | T::Param | T::Except => self.perr(
                        Some(node),
                        xe::RNGP_ATTRIBUTE_CONTENT,
                        "attribute has invalid content\n".to_string(),
                    ),
                    T::Noop => self.perr(
                        Some(node),
                        xe::RNGP_ATTRIBUTE_NOOP,
                        "RNG Internal error, noop found in attribute\n".to_string(),
                    ),
                    _ => {
                        self.dm(ret).content = Some(cur);
                        self.dm(cur).parent = Some(ret);
                    }
                }
            }
            child = nnext(ch);
        }
        if child.is_some() {
            self.perr(
                Some(node),
                xe::RNGP_ATTRIBUTE_CHILDREN,
                "attribute has multiple children\n".to_string(),
            );
        }
        self.flags = old_flags;
        Some(ret)
    }

    /// xmlRelaxNGParseExceptNameClass
    fn parse_except_name_class(&mut self, node: NodeId, attr: bool) -> Option<DefId> {
        if !is_rng(Some(node), "except") {
            self.perr(
                Some(node),
                xe::RNGP_EXCEPT_MISSING,
                "Expecting an except node\n".to_string(),
            );
            return None;
        }
        if nnext(node).is_some() {
            self.perr(
                Some(node),
                xe::RNGP_EXCEPT_MULTIPLE,
                "exceptNameClass allows only a single except node\n".to_string(),
            );
        }
        if nchildren(node).is_none() {
            self.perr(
                Some(node),
                xe::RNGP_EXCEPT_EMPTY,
                "except has no content\n".to_string(),
            );
            return None;
        }
        let ret = self.new_def(Some(node));
        self.dm(ret).typ = T::Except;
        let mut child = nchildren(node);
        let mut last: Option<DefId> = None;
        while let Some(ch) = child {
            let cur = self.new_def(Some(ch));
            self.dm(cur).typ = if attr { T::Attribute } else { T::Element };
            if self.parse_name_class(ch, cur).is_some() {
                match last {
                    None => self.dm(ret).content = Some(cur),
                    Some(l) => self.dm(l).next = Some(cur),
                }
                last = Some(cur);
            }
            child = nnext(ch);
        }
        Some(ret)
    }

    /// xmlRelaxNGParseNameClass
    fn parse_name_class(&mut self, node: NodeId, def: DefId) -> Option<DefId> {
        let n = Some(node);
        let mut ret = def;
        if is_rng(n, "name") || is_rng(n, "anyName") || is_rng(n, "nsName") {
            let dt = self.d(def).typ;
            if dt != T::Element && dt != T::Attribute {
                ret = self.new_def(n);
                self.dm(ret).parent = Some(def);
                self.dm(ret).typ = if self.flags & IN_ATTRIBUTE != 0 {
                    T::Attribute
                } else {
                    T::Element
                };
            }
        }
        if is_rng(n, "name") {
            let val = norm_ext_space(&get_content(node));
            if xst::validate_ncname(&val, false) != 0 {
                match nparent(node) {
                    Some(p) => self.perr(
                        n,
                        xe::RNGP_ELEMENT_NAME,
                        format!("Element {} name '{}' is not an NCName\n", nname(p), val),
                    ),
                    None => self.perr(
                        n,
                        xe::RNGP_ELEMENT_NAME,
                        format!("name '{}' is not an NCName\n", val),
                    ),
                }
            }
            self.dm(ret).name = Some(val.clone());
            let ns = get_prop(node, "ns");
            self.dm(ret).ns = ns.clone();
            if self.flags & IN_ATTRIBUTE != 0 {
                if ns.as_deref() == Some("http://www.w3.org/2000/xmlns") {
                    self.perr(
                        n,
                        xe::RNGP_XML_NS,
                        format!(
                            "Attribute with namespace '{}' is not allowed\n",
                            ns.as_deref().unwrap()
                        ),
                    );
                }
                if ns.as_deref() == Some("") && val == "xmlns" {
                    self.perr(
                        n,
                        xe::RNGP_XMLNS_NAME,
                        "Attribute with QName 'xmlns' is not allowed\n".to_string(),
                    );
                }
            }
        } else if is_rng(n, "anyName") {
            self.dm(ret).name = None;
            self.dm(ret).ns = None;
            if let Some(ch) = nchildren(node) {
                let attr = self.d(def).typ == T::Attribute;
                let nc = self.parse_except_name_class(ch, attr);
                self.dm(ret).name_class = nc;
            }
        } else if is_rng(n, "nsName") {
            self.dm(ret).name = None;
            let ns = get_prop(node, "ns");
            self.dm(ret).ns = ns.clone();
            if ns.is_none() {
                self.perr(
                    n,
                    xe::RNGP_NSNAME_NO_NS,
                    "nsName has no ns attribute\n".to_string(),
                );
            }
            if self.flags & IN_ATTRIBUTE != 0
                && ns.as_deref() == Some("http://www.w3.org/2000/xmlns")
            {
                self.perr(
                    n,
                    xe::RNGP_XML_NS,
                    format!(
                        "Attribute with namespace '{}' is not allowed\n",
                        ns.as_deref().unwrap()
                    ),
                );
            }
            if let Some(ch) = nchildren(node) {
                let attr = self.d(def).typ == T::Attribute;
                let nc = self.parse_except_name_class(ch, attr);
                self.dm(ret).name_class = nc;
            }
        } else if is_rng(n, "choice") {
            if self.d(def).typ == T::Choice {
                ret = def;
            } else {
                ret = self.new_def(n);
                self.dm(ret).parent = Some(def);
                self.dm(ret).typ = T::Choice;
            }
            match nchildren(node) {
                None => self.perr(
                    n,
                    xe::RNGP_CHOICE_EMPTY,
                    "Element choice is empty\n".to_string(),
                ),
                Some(_) => {
                    let mut child = nchildren(node);
                    let mut last: Option<DefId> = None;
                    while let Some(ch) = child {
                        if let Some(tmp) = self.parse_name_class(ch, ret) {
                            match last {
                                None => last = Some(tmp),
                                Some(l) => {
                                    if tmp != ret {
                                        self.dm(l).next = Some(tmp);
                                        last = Some(tmp);
                                    }
                                }
                            }
                        }
                        child = nnext(ch);
                    }
                }
            }
        } else {
            self.perr(
                n,
                xe::RNGP_CHOICE_CONTENT,
                format!(
                    "expecting name, anyName, nsName or choice : got {}\n",
                    nname(node)
                ),
            );
            return None;
        }
        if ret != def {
            match self.d(def).name_class {
                None => self.dm(def).name_class = Some(ret),
                Some(mut tmp) => {
                    while let Some(nx) = self.d(tmp).next {
                        tmp = nx;
                    }
                    self.dm(tmp).next = Some(ret);
                }
            }
        }
        Some(ret)
    }

    /// xmlRelaxNGParseElement
    fn parse_element(&mut self, node: NodeId) -> Option<DefId> {
        let ret = self.new_def(Some(node));
        self.dm(ret).typ = T::Element;
        let pd = self.def;
        self.dm(ret).parent = pd;
        let c0 = match nchildren(node) {
            None => {
                self.perr(
                    Some(node),
                    xe::RNGP_ELEMENT_EMPTY,
                    "xmlRelaxNGParseElement: element has no children\n".to_string(),
                );
                return Some(ret);
            }
            Some(c) => c,
        };
        let mut child = Some(c0);
        if self.parse_name_class(c0, ret).is_some() {
            child = nnext(c0);
        }
        if child.is_none() {
            self.perr(
                Some(node),
                xe::RNGP_ELEMENT_NO_CONTENT,
                "xmlRelaxNGParseElement: element has no content\n".to_string(),
            );
            return Some(ret);
        }
        let mut last: Option<DefId> = None;
        while let Some(ch) = child {
            if let Some(cur) = self.parse_pattern(ch) {
                self.dm(cur).parent = Some(ret);
                match self.d(cur).typ {
                    T::Attribute => {
                        let a = self.d(ret).attrs;
                        self.dm(cur).next = a;
                        self.dm(ret).attrs = Some(cur);
                    }
                    T::Start => self.perr(
                        Some(node),
                        xe::RNGP_ELEMENT_CONTENT,
                        "RNG Internal error, start found in element\n".to_string(),
                    ),
                    T::Param => self.perr(
                        Some(node),
                        xe::RNGP_ELEMENT_CONTENT,
                        "RNG Internal error, param found in element\n".to_string(),
                    ),
                    T::Except => self.perr(
                        Some(node),
                        xe::RNGP_ELEMENT_CONTENT,
                        "RNG Internal error, except found in element\n".to_string(),
                    ),
                    T::Noop => self.perr(
                        Some(node),
                        xe::RNGP_ELEMENT_CONTENT,
                        "RNG Internal error, noop found in element\n".to_string(),
                    ),
                    _ => match last {
                        None => {
                            self.dm(ret).content = Some(cur);
                            last = Some(cur);
                        }
                        Some(l) => {
                            if self.d(l).typ == T::Element && self.d(ret).content == Some(l) {
                                let g = self.new_def(Some(node));
                                self.dm(g).typ = T::Group;
                                self.dm(g).content = Some(l);
                                self.dm(ret).content = Some(g);
                            }
                            self.dm(l).next = Some(cur);
                            last = Some(cur);
                        }
                    },
                }
            }
            child = nnext(ch);
        }
        Some(ret)
    }

    /// xmlRelaxNGParsePatterns
    fn parse_patterns(&mut self, nodes: Option<NodeId>, group: bool) -> Option<DefId> {
        let parent = self.def;
        let mut def: Option<DefId> = None;
        let mut last: Option<DefId> = None;
        let mut nodes = nodes;
        while let Some(n) = nodes {
            if is_rng(Some(n), "element") {
                let cur = self.parse_element(n)?;
                match def {
                    None => {
                        def = Some(cur);
                        last = Some(cur);
                    }
                    Some(d) => {
                        if group && self.d(d).typ == T::Element && def == last {
                            let g = self.new_def(Some(n));
                            self.dm(g).typ = T::Group;
                            self.dm(g).content = last;
                            def = Some(g);
                        }
                        let l = last.unwrap();
                        self.dm(l).next = Some(cur);
                        last = Some(cur);
                    }
                }
                self.dm(cur).parent = parent;
            } else if let Some(cur) = self.parse_pattern(n) {
                match def {
                    None => {
                        def = Some(cur);
                        last = Some(cur);
                    }
                    Some(_) => {
                        let l = last.unwrap();
                        self.dm(l).next = Some(cur);
                        last = Some(cur);
                    }
                }
            }
            nodes = nnext(n);
        }
        def
    }

    /// xmlRelaxNGParseStart
    fn parse_start(&mut self, nodes: Option<NodeId>) -> i32 {
        let n = match nodes {
            None => {
                self.perr(
                    None,
                    xe::RNGP_START_EMPTY,
                    "start has no children\n".to_string(),
                );
                return -1;
            }
            Some(n) => n,
        };
        let def = if is_rng(Some(n), "empty") {
            let d = self.new_def(Some(n));
            self.dm(d).typ = T::Empty;
            if nchildren(n).is_some() {
                self.perr(
                    Some(n),
                    xe::RNGP_EMPTY_CONTENT,
                    "element empty is not empty\n".to_string(),
                );
            }
            Some(d)
        } else if is_rng(Some(n), "notAllowed") {
            let d = self.new_def(Some(n));
            self.dm(d).typ = T::NotAllowed;
            if nchildren(n).is_some() {
                self.perr(
                    Some(n),
                    xe::RNGP_NOTALLOWED_NOT_EMPTY,
                    "element notAllowed is not empty\n".to_string(),
                );
            }
            Some(d)
        } else {
            self.parse_patterns(Some(n), true)
        };
        match self.cur_grammar().start {
            Some(mut last) => {
                while let Some(nx) = self.d(last).next {
                    last = nx;
                }
                self.dm(last).next = def;
            }
            None => self.cur_grammar().start = def,
        }
        if let Some(nx) = nnext(n) {
            self.perr(
                Some(nx),
                xe::RNGP_START_CONTENT,
                "start more than one children\n".to_string(),
            );
            return -1;
        }
        0
    }

    /// xmlRelaxNGParseGrammarContent
    fn parse_grammar_content(&mut self, nodes: Option<NodeId>) -> i32 {
        if nodes.is_none() {
            self.perr(
                None,
                xe::RNGP_GRAMMAR_EMPTY,
                "grammar has no children\n".to_string(),
            );
            return -1;
        }
        let mut ret = 0;
        let mut nodes = nodes;
        while let Some(n) = nodes {
            if is_rng(Some(n), "start") {
                match nchildren(n) {
                    None => self.perr(
                        Some(n),
                        xe::RNGP_START_EMPTY,
                        "start has no children\n".to_string(),
                    ),
                    Some(ch) => {
                        if self.parse_start(Some(ch)) != 0 {
                            ret = -1;
                        }
                    }
                }
            } else if is_rng(Some(n), "define") {
                if self.parse_define(n) != 0 {
                    ret = -1;
                }
            } else if is_rng(Some(n), "include") {
                if self.parse_include(n) != 0 {
                    ret = -1;
                }
            } else {
                self.perr(
                    Some(n),
                    xe::RNGP_GRAMMAR_CONTENT,
                    format!("grammar has unexpected child {}\n", nname(n)),
                );
                ret = -1;
            }
            nodes = nnext(n);
        }
        ret
    }

    /// xmlRelaxNGCheckReference
    fn check_reference(&mut self, r: DefId, name: &str) {
        if self.d(r).dflags & IS_EXTERNAL_REF != 0 {
            return;
        }
        let rnode = self.d(r).node;
        if self.d(r).content.is_some() {
            self.perr(
                rnode,
                1,
                format!(
                    "Internal error: reference has content in CheckReference {}\n",
                    name
                ),
            );
            return;
        }
        let def = self
            .cur_grammar()
            .defs
            .as_ref()
            .and_then(|d| d.lookup(Some(name)));
        match def {
            Some(def) => {
                let mut cur = Some(r);
                while let Some(c) = cur {
                    self.dm(c).content = Some(def);
                    cur = self.d(c).next_hash;
                }
            }
            None => self.perr(
                rnode,
                xe::RNGP_REF_NO_DEF,
                format!("Reference {} has no matching definition\n", name),
            ),
        }
    }

    /// The interleave registration shared by combine of starts and defines.
    fn register_interleave(&mut self, cur: DefId) {
        let name = format!("interleave{}", self.nb_interleaves);
        self.nb_interleaves += 1;
        self.interleaves
            .get_or_insert_with(Vec::new)
            .push((name, cur));
    }

    /// xmlRelaxNGCheckCombine
    fn check_combine(&mut self, define: DefId, name: &str) {
        if self.d(define).next_hash.is_none() {
            return;
        }
        let dnode = self.d(define).node;
        let mut choice_or_interleave = -1;
        let mut missing = false;
        let mut cur = Some(define);
        while let Some(c) = cur {
            let combine = self.d(c).node.and_then(|n| get_prop(n, "combine"));
            match combine {
                Some(cb) => {
                    if cb == "choice" {
                        if choice_or_interleave == -1 {
                            choice_or_interleave = 1;
                        } else if choice_or_interleave == 0 {
                            self.perr(
                                dnode,
                                xe::RNGP_DEF_CHOICE_AND_INTERLEAVE,
                                format!(
                                    "Defines for {} use both 'choice' and 'interleave'\n",
                                    name
                                ),
                            );
                        }
                    } else if cb == "interleave" {
                        if choice_or_interleave == -1 {
                            choice_or_interleave = 0;
                        } else if choice_or_interleave == 1 {
                            self.perr(
                                dnode,
                                xe::RNGP_DEF_CHOICE_AND_INTERLEAVE,
                                format!(
                                    "Defines for {} use both 'choice' and 'interleave'\n",
                                    name
                                ),
                            );
                        }
                    } else {
                        self.perr(
                            dnode,
                            xe::RNGP_UNKNOWN_COMBINE,
                            format!("Defines for {} use unknown combine value '{}''\n", name, cb),
                        );
                    }
                }
                None => {
                    if !missing {
                        missing = true;
                    } else {
                        self.perr(
                            dnode,
                            xe::RNGP_NEED_COMBINE,
                            format!("Some defines for {} needs the combine attribute\n", name),
                        );
                    }
                }
            }
            cur = self.d(c).next_hash;
        }
        if choice_or_interleave == -1 {
            choice_or_interleave = 0;
        }
        let cur = self.new_def(dnode);
        self.dm(cur).typ = if choice_or_interleave == 0 {
            T::Interleave
        } else {
            T::Choice
        };
        let mut tmp = Some(define);
        let mut last: Option<DefId> = None;
        while let Some(t) = tmp {
            if let Some(tc) = self.d(t).content {
                let tmp2 = if self.d(tc).next.is_some() {
                    let n = self.d(tc).node;
                    let g = self.new_def(n);
                    self.dm(g).typ = T::Group;
                    self.dm(g).content = Some(tc);
                    g
                } else {
                    tc
                };
                match last {
                    None => self.dm(cur).content = Some(tmp2),
                    Some(l) => self.dm(l).next = Some(tmp2),
                }
                last = Some(tmp2);
            }
            self.dm(t).content = Some(cur);
            tmp = self.d(t).next_hash;
        }
        self.dm(define).content = Some(cur);
        if choice_or_interleave == 0 {
            self.register_interleave(cur);
        }
    }

    /// xmlRelaxNGCombineStart
    fn combine_start(&mut self, grammar: GId) {
        let starts = match self.grammars[grammar].start {
            Some(s) if self.d(s).next.is_some() => s,
            _ => return,
        };
        let mut choice_or_interleave = -1;
        let mut missing = false;
        let mut cur = Some(starts);
        while let Some(c) = cur {
            let cnode = self.d(c).node;
            let parent = cnode.and_then(nparent);
            let combine = match parent {
                Some(p) if nname(p) == "start" => get_prop(p, "combine"),
                _ => {
                    self.perr(
                        cnode,
                        xe::RNGP_START_MISSING,
                        "Internal error: start element not found\n".to_string(),
                    );
                    None
                }
            };
            match combine {
                Some(cb) => {
                    if cb == "choice" {
                        if choice_or_interleave == -1 {
                            choice_or_interleave = 1;
                        } else if choice_or_interleave == 0 {
                            self.perr(
                                cnode,
                                xe::RNGP_START_CHOICE_AND_INTERLEAVE,
                                "<start> use both 'choice' and 'interleave'\n".to_string(),
                            );
                        }
                    } else if cb == "interleave" {
                        if choice_or_interleave == -1 {
                            choice_or_interleave = 0;
                        } else if choice_or_interleave == 1 {
                            self.perr(
                                cnode,
                                xe::RNGP_START_CHOICE_AND_INTERLEAVE,
                                "<start> use both 'choice' and 'interleave'\n".to_string(),
                            );
                        }
                    } else {
                        self.perr(
                            cnode,
                            xe::RNGP_UNKNOWN_COMBINE,
                            format!("<start> uses unknown combine value '{}''\n", cb),
                        );
                    }
                }
                None => {
                    if !missing {
                        missing = true;
                    } else {
                        self.perr(
                            cnode,
                            xe::RNGP_NEED_COMBINE,
                            "Some <start> element miss the combine attribute\n".to_string(),
                        );
                    }
                }
            }
            cur = self.d(c).next;
        }
        if choice_or_interleave == -1 {
            choice_or_interleave = 0;
        }
        let sn = self.d(starts).node;
        let cur = self.new_def(sn);
        self.dm(cur).typ = if choice_or_interleave == 0 {
            T::Interleave
        } else {
            T::Choice
        };
        self.dm(cur).content = self.grammars[grammar].start;
        self.grammars[grammar].start = Some(cur);
        if choice_or_interleave == 0 {
            self.register_interleave(cur);
        }
    }

    /// xmlRelaxNGParseGrammar
    fn parse_grammar(&mut self, nodes: Option<NodeId>) -> GId {
        let ret = self.new_grammar();
        self.link_grammar(ret);
        let old = self.grammar;
        self.grammar = Some(ret);
        self.parse_grammar_content(nodes);
        self.grammar = Some(ret);
        if self.grammars[ret].start.is_none() {
            self.perr(
                nodes,
                xe::RNGP_GRAMMAR_NO_START,
                "Element <grammar> has no <start>\n".to_string(),
            );
        }
        self.combine_start(ret);
        if let Some(defs) = self.grammars[ret].defs.clone() {
            for (name, d) in defs.0 {
                self.check_combine(d, &name);
            }
        }
        if let Some(refs) = self.grammars[ret].refs.clone() {
            for (name, r) in refs.0 {
                self.check_reference(r, &name);
            }
        }
        self.grammar = old;
        ret
    }

    fn link_grammar(&mut self, ret: GId) {
        self.grammars[ret].parent = self.grammar;
        if let Some(g) = self.grammar {
            match self.grammars[g].children {
                None => self.grammars[g].children = Some(ret),
                Some(mut tmp) => {
                    while let Some(n) = self.grammars[tmp].next {
                        tmp = n;
                    }
                    self.grammars[tmp].next = Some(ret);
                }
            }
        }
    }

    /// xmlRelaxNGParseDocument: returns the top grammar.
    fn parse_document(&mut self, node: NodeId) -> Option<GId> {
        let top;
        if is_rng(Some(node), "grammar") {
            top = self.parse_grammar(nchildren(node));
        } else {
            top = self.new_grammar();
            self.link_grammar(top);
            let old = self.grammar;
            self.grammar = Some(top);
            self.parse_start(Some(node));
            if old.is_some() {
                self.grammar = old;
            }
        }
        if let Some(start) = self.grammars[top].start {
            self.check_cycles(Some(start), 0);
            if self.flags & IN_EXTERNALREF == 0 {
                self.simplify(self.grammars[top].start, None);
                while let Some(s) = self.grammars[top].start {
                    if self.d(s).typ == T::Noop && self.d(s).next.is_some() {
                        self.grammars[top].start = self.d(s).content;
                    } else {
                        break;
                    }
                }
                let s = self.grammars[top].start;
                self.check_rules(s, IN_START, T::Noop);
            }
        }
        Some(top)
    }
}

/// xmlRelaxNGIsNullable
fn is_nullable(defs: &mut [Define], define: Option<DefId>) -> i32 {
    let d = match define {
        None => return -1,
        Some(d) => d,
    };
    if defs[d].dflags & IS_NULLABLE != 0 {
        return 1;
    }
    if defs[d].dflags & IS_NOT_NULLABLE != 0 {
        return 0;
    }
    let ret = match defs[d].typ {
        T::Empty | T::Text => 1,
        T::Noop | T::Def | T::Ref | T::ExternalRef | T::ParentRef | T::OneOrMore => {
            let c = defs[d].content;
            is_nullable(defs, c)
        }
        T::Except
        | T::NotAllowed
        | T::Element
        | T::Datatype
        | T::Param
        | T::Value
        | T::List
        | T::Attribute => 0,
        T::Choice => {
            let mut list = defs[d].content;
            let mut r = 0;
            while let Some(l) = list {
                r = is_nullable(defs, Some(l));
                if r != 0 {
                    break;
                }
                list = defs[l].next;
            }
            r
        }
        T::Start | T::Interleave | T::Group => {
            let mut list = defs[d].content;
            while let Some(l) = list {
                let r = is_nullable(defs, Some(l));
                if r != 1 {
                    if r == 0 {
                        defs[d].dflags |= IS_NOT_NULLABLE;
                    }
                    return r;
                }
                list = defs[l].next;
            }
            return 1;
        }
        _ => return -1,
    };
    if ret == 0 {
        defs[d].dflags |= IS_NOT_NULLABLE;
    }
    if ret == 1 {
        defs[d].dflags |= IS_NULLABLE;
    }
    ret
}

/// xmlRelaxNGElementMatch without a validation context (no error
/// reporting): does the (name, ns) pair match the define name class?
fn element_match_noctx(defs: &[Define], define: DefId, name: &str, ns: Option<&str>) -> i32 {
    let d = &defs[define];
    if let Some(n) = &d.name {
        if name != n {
            return 0;
        }
    }
    match &d.ns {
        Some(dns) if !dns.is_empty() => match ns {
            None => return 0,
            Some(h) if h != dns => return 0,
            _ => {}
        },
        _ => {
            // ELEMEXTRANS on either the define name or the element name
            if ns.is_some() && (d.name.is_some() || d.ns.is_some()) {
                return 0;
            }
        }
    }
    let nc = match d.name_class {
        None => return 1,
        Some(nc) => nc,
    };
    match defs[nc].typ {
        T::Except => {
            let mut list = defs[nc].content;
            while let Some(l) = list {
                let r = element_match_noctx(defs, l, name, ns);
                if r == 1 {
                    return 0;
                }
                if r < 0 {
                    return r;
                }
                list = defs[l].next;
            }
            1
        }
        T::Choice => {
            let mut list = defs[nc].name_class;
            while let Some(l) = list {
                let r = element_match_noctx(defs, l, name, ns);
                if r == 1 {
                    return 1;
                }
                if r < 0 {
                    return r;
                }
                list = defs[l].next;
            }
            0
        }
        _ => -1,
    }
}

const INVALID_NAME: &str = "\u{1}";

/// xmlRelaxNGCompareNameClasses: 1 distinct, 0 may match the same names
fn compare_name_classes(defs: &[Define], def1: DefId, def2: DefId) -> i32 {
    let mut ret;
    let d1 = &defs[def1];
    let d2 = &defs[def2];
    if d1.typ == T::Element || d1.typ == T::Attribute {
        if d2.typ == T::Text {
            return 1;
        }
        let name = d1.name.as_deref().unwrap_or(INVALID_NAME);
        let ns = match &d1.ns {
            Some(n) if !n.is_empty() => Some(n.as_str()),
            _ => None,
        };
        if element_match_noctx(defs, def2, name, ns) != 0 {
            ret = match d1.name_class {
                Some(nc) => compare_name_classes(defs, nc, def2),
                None => 0,
            };
        } else {
            ret = 1;
        }
    } else if d1.typ == T::Text {
        if d2.typ == T::Text {
            return 0;
        }
        return 1;
    } else if d1.typ == T::Except {
        ret = match d1.content {
            Some(c) => compare_name_classes(defs, c, def2),
            None => 1,
        };
        if ret == 0 {
            ret = 1;
        } else if ret == 1 {
            ret = 0;
        }
    } else {
        ret = 0;
    }
    if ret == 0 {
        return ret;
    }
    if d2.typ == T::Element || d2.typ == T::Attribute {
        let name = d2.name.as_deref().unwrap_or(INVALID_NAME);
        let ns = match &d2.ns {
            Some(n) if n.is_empty() => None,
            Some(n) => Some(n.as_str()),
            None => Some(INVALID_NAME),
        };
        if element_match_noctx(defs, def1, name, ns) != 0 {
            ret = match d2.name_class {
                Some(nc) => compare_name_classes(defs, nc, def1),
                None => 0,
            };
        } else {
            ret = 1;
        }
    } else {
        ret = 0;
    }
    ret
}

/// xmlRelaxNGCompareElemDefLists: 1 distinct, 0 may overlap
fn compare_elem_def_lists(
    defs: &[Define],
    def1: Option<&Vec<DefId>>,
    def2: Option<&Vec<DefId>>,
) -> i32 {
    let (l1, l2) = match (def1, def2) {
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => (a, b),
        _ => return 1,
    };
    for &a in l1 {
        for &b in l2 {
            if compare_name_classes(defs, a, b) == 0 {
                return 0;
            }
        }
    }
    1
}

impl<'a> PCtxt<'a> {
    /// xmlRelaxNGCheckCycles
    fn check_cycles(&mut self, cur: Option<DefId>, depth: i32) -> i32 {
        let mut ret = 0;
        let mut cur = cur;
        while ret == 0 {
            let c = match cur {
                None => break,
                Some(c) => c,
            };
            let typ = self.d(c).typ;
            if typ == T::Ref || typ == T::ParentRef {
                if self.d(c).depth == -1 {
                    self.dm(c).depth = depth;
                    let cc = self.d(c).content;
                    ret = self.check_cycles(cc, depth);
                    self.dm(c).depth = -2;
                } else if depth == self.d(c).depth {
                    let n = self.d(c).node;
                    let name = self
                        .d(c)
                        .name
                        .clone()
                        .unwrap_or_else(|| "(null)".to_string());
                    self.perr(
                        n,
                        xe::RNGP_REF_CYCLE,
                        format!("Detected a cycle in {} references\n", name),
                    );
                    return -1;
                }
            } else if typ == T::Element {
                let cc = self.d(c).content;
                ret = self.check_cycles(cc, depth + 1);
            } else {
                let cc = self.d(c).content;
                ret = self.check_cycles(cc, depth);
            }
            cur = self.d(c).next;
        }
        ret
    }

    /// xmlRelaxNGTryUnlink
    fn try_unlink(
        &mut self,
        cur: DefId,
        parent: Option<DefId>,
        prev: Option<DefId>,
    ) -> Option<DefId> {
        let mut prev = prev;
        match prev {
            Some(p) => {
                let n = self.d(cur).next;
                self.dm(p).next = n;
            }
            None => match parent {
                Some(par) => {
                    let n = self.d(cur).next;
                    if self.d(par).content == Some(cur) {
                        self.dm(par).content = n;
                    } else if self.d(par).attrs == Some(cur) {
                        self.dm(par).attrs = n;
                    } else if self.d(par).name_class == Some(cur) {
                        self.dm(par).name_class = n;
                    }
                }
                None => {
                    self.dm(cur).typ = T::Noop;
                    prev = Some(cur);
                }
            },
        }
        prev
    }

    /// xmlRelaxNGSimplify
    fn simplify(&mut self, cur: Option<DefId>, parent: Option<DefId>) {
        let mut prev: Option<DefId> = None;
        let mut cur = cur;
        let ptype = |s: &Self| parent.map(|p| s.d(p).typ);
        while let Some(mut c) = cur {
            let typ = self.d(c).typ;
            if typ == T::Ref || typ == T::ParentRef {
                if self.d(c).depth != -3 {
                    self.dm(c).depth = -3;
                    let cc = self.d(c).content;
                    self.simplify(cc, Some(c));
                }
            } else if typ == T::NotAllowed {
                self.dm(c).parent = parent;
                let pt = ptype(self);
                if matches!(
                    pt,
                    Some(
                        T::Attribute
                            | T::List
                            | T::Group
                            | T::Interleave
                            | T::OneOrMore
                            | T::ZeroOrMore
                    )
                ) {
                    self.dm(parent.unwrap()).typ = T::NotAllowed;
                    break;
                }
                if pt == Some(T::Choice) {
                    prev = self.try_unlink(c, parent, prev);
                } else {
                    prev = Some(c);
                }
            } else if typ == T::Empty {
                self.dm(c).parent = parent;
                let pt = ptype(self);
                if matches!(pt, Some(T::OneOrMore | T::ZeroOrMore)) {
                    self.dm(parent.unwrap()).typ = T::Empty;
                    break;
                }
                if matches!(pt, Some(T::Group | T::Interleave)) {
                    prev = self.try_unlink(c, parent, prev);
                } else {
                    prev = Some(c);
                }
            } else {
                self.dm(c).parent = parent;
                if let Some(cc) = self.d(c).content {
                    self.simplify(Some(cc), Some(c));
                }
                if self.d(c).typ != T::Value {
                    if let Some(a) = self.d(c).attrs {
                        self.simplify(Some(a), Some(c));
                    }
                }
                if let Some(nc) = self.d(c).name_class {
                    self.simplify(Some(nc), Some(c));
                }
                if self.d(c).typ == T::Element {
                    while let Some(cc) = self.d(c).content {
                        let attronly = self.generate_attributes(cc);
                        if attronly == 1 {
                            let n = self.d(cc).next;
                            self.dm(c).content = n;
                            let a = self.d(c).attrs;
                            self.dm(cc).next = a;
                            self.dm(c).attrs = Some(cc);
                        } else {
                            break;
                        }
                    }
                    let mut pre = self.d(c).content;
                    while let Some(p) = pre {
                        let tmp = match self.d(p).next {
                            None => break,
                            Some(t) => t,
                        };
                        let attronly = self.generate_attributes(tmp);
                        if attronly == 1 {
                            let n = self.d(tmp).next;
                            self.dm(p).next = n;
                            let a = self.d(c).attrs;
                            self.dm(tmp).next = a;
                            self.dm(c).attrs = Some(tmp);
                        } else {
                            pre = Some(tmp);
                        }
                    }
                }
                let t2 = self.d(c).typ;
                if t2 == T::Group || t2 == T::Interleave {
                    match self.d(c).content {
                        None => self.dm(c).typ = T::Empty,
                        Some(cc) if self.d(cc).next.is_none() => {
                            if parent.is_none() && prev.is_none() {
                                self.dm(c).typ = T::Noop;
                            } else if prev.is_none() {
                                self.dm(parent.unwrap()).content = Some(cc);
                                let n = self.d(c).next;
                                self.dm(cc).next = n;
                                c = cc;
                            } else {
                                let n = self.d(c).next;
                                self.dm(cc).next = n;
                                self.dm(prev.unwrap()).next = Some(cc);
                                c = cc;
                            }
                        }
                        _ => {}
                    }
                }
                let t3 = self.d(c).typ;
                let pt = ptype(self);
                if t3 == T::Except
                    && self.d(c).content.map(|x| self.d(x).typ) == Some(T::NotAllowed)
                {
                    prev = self.try_unlink(c, parent, prev);
                } else if t3 == T::NotAllowed {
                    if matches!(
                        pt,
                        Some(
                            T::Attribute
                                | T::List
                                | T::Group
                                | T::Interleave
                                | T::OneOrMore
                                | T::ZeroOrMore
                        )
                    ) {
                        self.dm(parent.unwrap()).typ = T::NotAllowed;
                        break;
                    }
                    if pt == Some(T::Choice) {
                        prev = self.try_unlink(c, parent, prev);
                    } else {
                        prev = Some(c);
                    }
                } else if t3 == T::Empty {
                    if matches!(pt, Some(T::OneOrMore | T::ZeroOrMore)) {
                        self.dm(parent.unwrap()).typ = T::Empty;
                        break;
                    }
                    if matches!(pt, Some(T::Group | T::Interleave | T::Choice)) {
                        prev = self.try_unlink(c, parent, prev);
                    } else {
                        prev = Some(c);
                    }
                } else {
                    prev = Some(c);
                }
            }
            cur = self.d(c).next;
        }
    }

    /// xmlRelaxNGGenerateAttributes: 1 if the define can only generate
    /// attributes.
    fn generate_attributes(&mut self, def: DefId) -> i32 {
        if self.nb_errors != 0 {
            return -1;
        }
        let mut cur = Some(def);
        while let Some(c) = cur {
            let typ = self.d(c).typ;
            if matches!(
                typ,
                T::Element | T::Text | T::Datatype | T::Param | T::List | T::Value | T::Empty
            ) {
                return 0;
            }
            if matches!(
                typ,
                T::Choice
                    | T::Interleave
                    | T::Group
                    | T::OneOrMore
                    | T::ZeroOrMore
                    | T::Optional
                    | T::ParentRef
                    | T::ExternalRef
                    | T::Ref
                    | T::Def
            ) {
                if let Some(cc) = self.d(c).content {
                    let mut tmp = Some(cc);
                    while let Some(t) = tmp {
                        self.dm(t).parent = Some(c);
                        tmp = self.d(t).next;
                    }
                    cur = Some(cc);
                    continue;
                }
            }
            if c == def {
                break;
            }
            if let Some(n) = self.d(c).next {
                cur = Some(n);
                continue;
            }
            let mut x = c;
            loop {
                match self.d(x).parent {
                    None => {
                        cur = None;
                        break;
                    }
                    Some(p) => {
                        if p == def {
                            return 1;
                        }
                        if let Some(n) = self.d(p).next {
                            cur = Some(n);
                            break;
                        }
                        x = p;
                    }
                }
            }
        }
        1
    }

    /// xmlRelaxNGGetElements: eora 0 elements (and text), 1 attributes, 2
    /// elements, text and data
    fn get_elements(&mut self, def: DefId, eora: i32) -> Option<Vec<DefId>> {
        if self.nb_errors != 0 {
            return None;
        }
        let mut ret: Option<Vec<DefId>> = None;
        let mut cur = Some(def);
        while let Some(c) = cur {
            let typ = self.d(c).typ;
            if (eora == 0 && matches!(typ, T::Element | T::Text))
                || (eora == 1 && typ == T::Attribute)
                || (eora == 2
                    && matches!(typ, T::Datatype | T::Element | T::List | T::Text | T::Value))
            {
                ret.get_or_insert_with(Vec::new).push(c);
            } else if matches!(
                typ,
                T::Choice
                    | T::Interleave
                    | T::Group
                    | T::OneOrMore
                    | T::ZeroOrMore
                    | T::Optional
                    | T::ParentRef
                    | T::Ref
                    | T::Def
                    | T::ExternalRef
            ) {
                if let Some(cc) = self.d(c).content {
                    let mut tmp = Some(cc);
                    while let Some(t) = tmp {
                        self.dm(t).parent = Some(c);
                        tmp = self.d(t).next;
                    }
                    cur = Some(cc);
                    continue;
                }
            }
            if c == def {
                break;
            }
            if let Some(n) = self.d(c).next {
                cur = Some(n);
                continue;
            }
            let mut x = c;
            loop {
                match self.d(x).parent {
                    None => {
                        cur = None;
                        break;
                    }
                    Some(p) => {
                        if p == def {
                            return ret;
                        }
                        if let Some(n) = self.d(p).next {
                            cur = Some(n);
                            break;
                        }
                        x = p;
                    }
                }
            }
        }
        ret
    }

    /// The triage key of an element or text define.
    fn triage_key(&self, d: DefId) -> Option<TriageKey> {
        let df = self.d(d);
        let ns = match &df.ns {
            Some(n) if !n.is_empty() => Some(n.clone()),
            _ => None,
        };
        match df.typ {
            T::Text => Some(("#text".to_string(), None)),
            T::Element => match &df.name {
                Some(n) => Some((n.clone(), ns)),
                None => Some(("#any".to_string(), ns)),
            },
            _ => None,
        }
    }

    /// xmlRelaxNGCheckChoiceDeterminism
    fn check_choice_determinism(&mut self, def: DefId) {
        if self.d(def).typ != T::Choice || self.d(def).dflags & IS_PROCESSED != 0 {
            return;
        }
        if self.nb_errors != 0 {
            return;
        }
        let is_nullable = is_nullable(&mut self.defs, Some(def));
        let mut children = Vec::new();
        let mut cur = self.d(def).content;
        while let Some(c) = cur {
            children.push(c);
            cur = self.d(c).next;
        }
        let mut triage: Option<HashMap<TriageKey, DefId>> = None;
        let mut is_triable = 1;
        if is_nullable == 0 {
            triage = Some(HashMap::new());
        } else {
            is_triable = 0;
        }
        let mut lists: Vec<Option<Vec<DefId>>> = Vec::new();
        for &c in &children {
            let l = self.get_elements(c, 0);
            match &l {
                None => is_triable = 0,
                Some(v) if v.is_empty() => is_triable = 0,
                Some(v) => {
                    if is_triable == 1 {
                        for &t in v {
                            if is_triable != 1 {
                                break;
                            }
                            match self.triage_key(t) {
                                Some(k) => {
                                    let tr = triage.as_mut().unwrap();
                                    match tr.entry(k) {
                                        std::collections::hash_map::Entry::Occupied(_) => {
                                            is_triable = -1
                                        }
                                        std::collections::hash_map::Entry::Vacant(v) => {
                                            v.insert(c);
                                        }
                                    }
                                }
                                None => is_triable = -1,
                            }
                        }
                    }
                }
            }
            lists.push(l);
        }
        let mut is_indeterminist = false;
        for i in 0..lists.len() {
            if lists[i].is_none() {
                continue;
            }
            for j in 0..i {
                if lists[j].is_none() {
                    continue;
                }
                if compare_elem_def_lists(&self.defs, lists[i].as_ref(), lists[j].as_ref()) == 0 {
                    is_indeterminist = true;
                }
            }
        }
        if is_indeterminist {
            self.dm(def).dflags |= IS_INDETERMINIST;
        }
        if is_triable == 1 {
            self.dm(def).dflags |= IS_TRIABLE;
            self.dm(def).triage = triage;
        }
        self.dm(def).dflags |= IS_PROCESSED;
    }

    /// xmlRelaxNGCheckGroupAttrs
    fn check_group_attrs(&mut self, def: DefId) {
        let typ = self.d(def).typ;
        if typ != T::Group && typ != T::Element {
            return;
        }
        if self.d(def).dflags & IS_PROCESSED != 0 {
            return;
        }
        if self.nb_errors != 0 {
            return;
        }
        let mut items = Vec::new();
        let mut cur = self.d(def).attrs;
        while let Some(c) = cur {
            items.push(c);
            cur = self.d(c).next;
        }
        let mut cur = self.d(def).content;
        while let Some(c) = cur {
            items.push(c);
            cur = self.d(c).next;
        }
        let mut lists = Vec::new();
        for c in items {
            lists.push(self.get_elements(c, 1));
        }
        let node = self.d(def).node;
        for i in 0..lists.len() {
            if lists[i].is_none() {
                continue;
            }
            for j in 0..i {
                if lists[j].is_none() {
                    continue;
                }
                if compare_elem_def_lists(&self.defs, lists[i].as_ref(), lists[j].as_ref()) == 0 {
                    self.perr(
                        node,
                        xe::RNGP_GROUP_ATTR_CONFLICT,
                        "Attributes conflicts in group\n".to_string(),
                    );
                }
            }
        }
        self.dm(def).dflags |= IS_PROCESSED;
    }

    /// xmlRelaxNGComputeInterleaves
    fn compute_interleaves(&mut self, def: DefId) {
        if self.nb_errors != 0 {
            return;
        }
        let mut groups: Vec<IGroup> = Vec::new();
        let mut is_mixed = 0;
        let mut cur = self.d(def).content;
        while let Some(c) = cur {
            if self.d(c).typ == T::Text {
                is_mixed += 1;
            }
            let defs = self.get_elements(c, 2);
            let attrs = self.get_elements(c, 1);
            groups.push(IGroup {
                rule: c,
                defs,
                attrs,
            });
            cur = self.d(c).next;
        }
        let nbgroups = groups.len();
        let mut triage: HashMap<TriageKey, usize> = HashMap::new();
        let mut is_determinist = 1;
        let node = self.d(def).node;
        for i in 0..nbgroups {
            for j in i + 1..nbgroups {
                if compare_elem_def_lists(
                    &self.defs,
                    groups[i].defs.as_ref(),
                    groups[j].defs.as_ref(),
                ) == 0
                {
                    self.perr(
                        node,
                        xe::RNGP_ELEM_TEXT_CONFLICT,
                        "Element or text conflicts in interleave\n".to_string(),
                    );
                }
                if compare_elem_def_lists(
                    &self.defs,
                    groups[i].attrs.as_ref(),
                    groups[j].attrs.as_ref(),
                ) == 0
                {
                    self.perr(
                        node,
                        xe::RNGP_ATTR_CONFLICT,
                        "Attributes conflicts in interleave\n".to_string(),
                    );
                }
            }
            match &groups[i].defs {
                Some(v) if !v.is_empty() => {
                    for &t in v.clone().iter() {
                        let key = self.triage_key(t);
                        match key {
                            Some(k) => {
                                if self.d(t).typ == T::Element
                                    && self.d(t).name.is_none()
                                    && self.d(t).name_class.is_some()
                                {
                                    is_determinist = 2;
                                }
                                match triage.entry(k) {
                                    std::collections::hash_map::Entry::Occupied(_) => {
                                        is_determinist = -1
                                    }
                                    std::collections::hash_map::Entry::Vacant(v) => {
                                        v.insert(i + 1);
                                    }
                                }
                            }
                            None => is_determinist = -1,
                        }
                    }
                }
                _ => is_determinist = 0,
            }
        }
        let mut flags = 0;
        if is_determinist == 1 {
            flags = IS_DETERMINIST;
        }
        if is_determinist == 2 {
            flags = IS_DETERMINIST | IS_NEEDCHECK;
        }
        let p = Partition {
            nbgroups,
            triage,
            flags,
            groups,
        };
        self.dm(def).partition = Some(Rc::new(p));
        if is_mixed != 0 {
            self.dm(def).dflags |= IS_MIXED;
        }
    }

    /// xmlRelaxNGCheckRules
    fn check_rules(&mut self, cur: Option<DefId>, flags: i32, ptype: T) -> i32 {
        let mut val = CT_EMPTY;
        let mut cur = cur;
        while let Some(c) = cur {
            let mut ret;
            let typ = self.d(c).typ;
            let node = self.d(c).node;
            match typ {
                T::Ref | T::ParentRef => {
                    if flags & IN_DATAEXCEPT != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_DATA_EXCEPT_REF,
                            "Found forbidden pattern data/except//ref\n".to_string(),
                        );
                    }
                    if self.d(c).content.is_none() {
                        if typ == T::ParentRef {
                            self.perr(
                                node,
                                xe::RNGP_REF_NO_DEF,
                                "Internal found no define for parent refs\n".to_string(),
                            );
                        } else {
                            let n = self.d(c).name.clone().unwrap_or_else(|| "null".to_string());
                            self.perr(
                                node,
                                xe::RNGP_REF_NO_DEF,
                                format!("Internal found no define for ref {}\n", n),
                            );
                        }
                    }
                    if self.d(c).depth > -4 {
                        self.dm(c).depth = -4;
                        let cc = self.d(c).content;
                        ret = self.check_rules(cc, flags, typ);
                        self.dm(c).depth = ret - 15;
                    } else if self.d(c).depth == -4 {
                        ret = CT_COMPLEX;
                    } else {
                        ret = self.d(c).depth + 15;
                    }
                }
                T::Element => {
                    self.check_group_attrs(c);
                    if flags & IN_DATAEXCEPT != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_DATA_EXCEPT_ELEM,
                            "Found forbidden pattern data/except//element(ref)\n".to_string(),
                        );
                    }
                    if flags & IN_LIST != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_LIST_ELEM,
                            "Found forbidden pattern list//element(ref)\n".to_string(),
                        );
                    }
                    if flags & IN_ATTRIBUTE != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_ATTR_ELEM,
                            "Found forbidden pattern attribute//element(ref)\n".to_string(),
                        );
                        self.perr(
                            node,
                            xe::RNGP_PAT_ATTR_ELEM,
                            "Found forbidden pattern attribute//element(ref)\n".to_string(),
                        );
                    }
                    let a = self.d(c).attrs;
                    ret = self.check_rules(a, 0, typ);
                    let name = self
                        .d(c)
                        .name
                        .clone()
                        .unwrap_or_else(|| "(null)".to_string());
                    if ret != CT_EMPTY {
                        self.perr(
                            node,
                            xe::RNGP_ELEM_CONTENT_EMPTY,
                            format!("Element {} attributes have a content type error\n", name),
                        );
                    }
                    let cc = self.d(c).content;
                    ret = self.check_rules(cc, 0, typ);
                    if ret == CT_ERROR {
                        self.perr(
                            node,
                            xe::RNGP_ELEM_CONTENT_ERROR,
                            format!("Element {} has a content type error\n", name),
                        );
                    } else {
                        ret = CT_COMPLEX;
                    }
                }
                T::Attribute => {
                    if flags & IN_ATTRIBUTE != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_ATTR_ATTR,
                            "Found forbidden pattern attribute//attribute\n".to_string(),
                        );
                    }
                    if flags & IN_LIST != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_LIST_ATTR,
                            "Found forbidden pattern list//attribute\n".to_string(),
                        );
                    }
                    if flags & IN_OOMGROUP != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_ONEMORE_GROUP_ATTR,
                            "Found forbidden pattern oneOrMore//group//attribute\n".to_string(),
                        );
                    }
                    if flags & IN_OOMINTERLEAVE != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_ONEMORE_INTERLEAVE_ATTR,
                            "Found forbidden pattern oneOrMore//interleave//attribute\n"
                                .to_string(),
                        );
                    }
                    if flags & IN_DATAEXCEPT != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_DATA_EXCEPT_ATTR,
                            "Found forbidden pattern data/except//attribute\n".to_string(),
                        );
                    }
                    if flags & IN_START != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_START_ATTR,
                            "Found forbidden pattern start//attribute\n".to_string(),
                        );
                    }
                    if flags & IN_ONEORMORE == 0
                        && self.d(c).name.is_none()
                        && self.d(c).name_class.is_none()
                    {
                        if self.d(c).ns.is_none() {
                            self.perr(
                                node,
                                xe::RNGP_ANYNAME_ATTR_ANCESTOR,
                                "Found anyName attribute without oneOrMore ancestor\n".to_string(),
                            );
                        } else {
                            self.perr(
                                node,
                                xe::RNGP_NSNAME_ATTR_ANCESTOR,
                                "Found nsName attribute without oneOrMore ancestor\n".to_string(),
                            );
                        }
                    }
                    let cc = self.d(c).content;
                    self.check_rules(cc, flags | IN_ATTRIBUTE, typ);
                    ret = CT_EMPTY;
                }
                T::OneOrMore | T::ZeroOrMore => {
                    if flags & IN_DATAEXCEPT != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_DATA_EXCEPT_ONEMORE,
                            "Found forbidden pattern data/except//oneOrMore\n".to_string(),
                        );
                    }
                    if flags & IN_START != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_START_ONEMORE,
                            "Found forbidden pattern start//oneOrMore\n".to_string(),
                        );
                    }
                    let cc = self.d(c).content;
                    ret = self.check_rules(cc, flags | IN_ONEORMORE, typ);
                    ret = group_content_type(ret, ret);
                }
                T::List => {
                    if flags & IN_LIST != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_LIST_LIST,
                            "Found forbidden pattern list//list\n".to_string(),
                        );
                    }
                    if flags & IN_DATAEXCEPT != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_DATA_EXCEPT_LIST,
                            "Found forbidden pattern data/except//list\n".to_string(),
                        );
                    }
                    if flags & IN_START != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_START_LIST,
                            "Found forbidden pattern start//list\n".to_string(),
                        );
                    }
                    let cc = self.d(c).content;
                    ret = self.check_rules(cc, flags | IN_LIST, typ);
                }
                T::Group => {
                    if flags & IN_DATAEXCEPT != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_DATA_EXCEPT_GROUP,
                            "Found forbidden pattern data/except//group\n".to_string(),
                        );
                    }
                    if flags & IN_START != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_START_GROUP,
                            "Found forbidden pattern start//group\n".to_string(),
                        );
                    }
                    let nflags = if flags & IN_ONEORMORE != 0 {
                        flags | IN_OOMGROUP
                    } else {
                        flags
                    };
                    let cc = self.d(c).content;
                    ret = self.check_rules(cc, nflags, typ);
                    self.check_group_attrs(c);
                }
                T::Interleave => {
                    if flags & IN_LIST != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_LIST_INTERLEAVE,
                            "Found forbidden pattern list//interleave\n".to_string(),
                        );
                    }
                    if flags & IN_DATAEXCEPT != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_DATA_EXCEPT_INTERLEAVE,
                            "Found forbidden pattern data/except//interleave\n".to_string(),
                        );
                    }
                    if flags & IN_START != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_DATA_EXCEPT_INTERLEAVE,
                            "Found forbidden pattern start//interleave\n".to_string(),
                        );
                    }
                    let nflags = if flags & IN_ONEORMORE != 0 {
                        flags | IN_OOMINTERLEAVE
                    } else {
                        flags
                    };
                    let cc = self.d(c).content;
                    ret = self.check_rules(cc, nflags, typ);
                }
                T::Except => {
                    let p = self.d(c).parent;
                    let nflags = if p.map(|p| self.d(p).typ) == Some(T::Datatype) {
                        flags | IN_DATAEXCEPT
                    } else {
                        flags
                    };
                    let cc = self.d(c).content;
                    ret = self.check_rules(cc, nflags, typ);
                }
                T::Datatype => {
                    if flags & IN_START != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_START_DATA,
                            "Found forbidden pattern start//data\n".to_string(),
                        );
                    }
                    let cc = self.d(c).content;
                    self.check_rules(cc, flags, typ);
                    ret = CT_SIMPLE;
                }
                T::Value => {
                    if flags & IN_START != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_START_VALUE,
                            "Found forbidden pattern start//value\n".to_string(),
                        );
                    }
                    let cc = self.d(c).content;
                    self.check_rules(cc, flags, typ);
                    ret = CT_SIMPLE;
                }
                T::Text => {
                    if flags & IN_LIST != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_LIST_TEXT,
                            "Found forbidden pattern list//text\n".to_string(),
                        );
                    }
                    if flags & IN_DATAEXCEPT != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_DATA_EXCEPT_TEXT,
                            "Found forbidden pattern data/except//text\n".to_string(),
                        );
                    }
                    if flags & IN_START != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_START_TEXT,
                            "Found forbidden pattern start//text\n".to_string(),
                        );
                    }
                    ret = CT_COMPLEX;
                }
                T::Empty => {
                    if flags & IN_DATAEXCEPT != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_DATA_EXCEPT_EMPTY,
                            "Found forbidden pattern data/except//empty\n".to_string(),
                        );
                    }
                    if flags & IN_START != 0 {
                        self.perr(
                            node,
                            xe::RNGP_PAT_START_EMPTY,
                            "Found forbidden pattern start//empty\n".to_string(),
                        );
                    }
                    ret = CT_EMPTY;
                }
                T::Choice => {
                    self.check_choice_determinism(c);
                    let cc = self.d(c).content;
                    ret = self.check_rules(cc, flags, typ);
                }
                _ => {
                    let cc = self.d(c).content;
                    ret = self.check_rules(cc, flags, typ);
                }
            }
            cur = self.d(c).next;
            match ptype {
                T::Group => val = group_content_type(val, ret),
                T::Interleave => {
                    // libxml2 computes and drops the combined type here
                }
                T::Choice => val = max_content_type(val, ret),
                T::List => val = CT_SIMPLE,
                T::Except => {
                    val = if ret == CT_ERROR { CT_ERROR } else { CT_SIMPLE };
                }
                _ => val = group_content_type(val, ret),
            }
        }
        val
    }

    // -----------------------------------------------------------------
    // Compilation of content models

    /// xmlRelaxNGIsCompilable
    fn is_compilable(&mut self, def: Option<DefId>) -> i32 {
        let d = match def {
            None => return -1,
            Some(d) => d,
        };
        let typ = self.d(d).typ;
        if typ != T::Element && self.d(d).dflags & IS_COMPILABLE != 0 {
            return 1;
        }
        if typ != T::Element && self.d(d).dflags & IS_NOT_COMPILABLE != 0 {
            return 0;
        }
        let mut ret = -1;
        match typ {
            T::Noop => {
                let c = self.d(d).content;
                ret = self.is_compilable(c);
            }
            T::Text | T::Empty => ret = 1,
            T::Element => {
                if self.d(d).dflags & (IS_NOT_COMPILABLE | IS_COMPILABLE) == 0 {
                    let mut list = self.d(d).content;
                    while let Some(l) = list {
                        ret = self.is_compilable(Some(l));
                        if ret != 1 {
                            break;
                        }
                        list = self.d(l).next;
                    }
                    if ret == 0 {
                        self.dm(d).dflags &= !IS_COMPILABLE;
                        self.dm(d).dflags |= IS_NOT_COMPILABLE;
                    }
                    if ret == 1 {
                        // libxml2 uses &= here, clearing the other flags
                        self.dm(d).dflags &= IS_NOT_COMPILABLE;
                        if self.d(d).dflags == 0 {
                            self.dm(d).dflags |= IS_COMPILABLE;
                        }
                    }
                }
                return if self.d(d).name_class.is_some() || self.d(d).name.is_none() {
                    0
                } else {
                    1
                };
            }
            T::Ref | T::ExternalRef | T::ParentRef => {
                if self.d(d).depth == -20 {
                    return 1;
                }
                self.dm(d).depth = -20;
                let mut list = self.d(d).content;
                while let Some(l) = list {
                    ret = self.is_compilable(Some(l));
                    if ret != 1 {
                        break;
                    }
                    list = self.d(l).next;
                }
            }
            T::Start
            | T::Optional
            | T::ZeroOrMore
            | T::OneOrMore
            | T::Choice
            | T::Group
            | T::Def => {
                let mut list = self.d(d).content;
                while let Some(l) = list {
                    ret = self.is_compilable(Some(l));
                    if ret != 1 {
                        break;
                    }
                    list = self.d(l).next;
                }
            }
            T::Except
            | T::Attribute
            | T::Interleave
            | T::Datatype
            | T::List
            | T::Param
            | T::Value
            | T::NotAllowed => ret = 0,
        }
        if ret == 0 {
            self.dm(d).dflags |= IS_NOT_COMPILABLE;
        }
        if ret == 1 {
            self.dm(d).dflags |= IS_COMPILABLE;
        }
        ret
    }

    fn am_state(&self) -> StateId {
        self.state.expect("automaton state")
    }

    /// Compile the content of `def` into a fresh automaton (START and
    /// ELEMENT).
    fn compile_content(&mut self, def: DefId, require_determinist_check_first: bool) {
        let oldam = self.am.take();
        let oldstate = self.state;
        self.dm(def).depth = -25;
        let mut am = Automata::new();
        am.set_flags(AM_AUTOMATA_RNG);
        self.state = Some(am.init_state());
        self.am = Some(am);
        let mut list = self.d(def).content;
        while let Some(l) = list {
            self.compile(Some(l));
            list = self.d(l).next;
        }
        let mut am = self.am.take().unwrap();
        am.set_final_state(self.am_state());
        let model = if require_determinist_check_first {
            if am.is_determinist() {
                am.compile()
            } else {
                None
            }
        } else {
            am.compile().filter(|r| r.is_determinist())
        };
        self.dm(def).cont_model = model.map(Rc::new);
        self.state = oldstate;
        self.am = oldam;
    }

    /// xmlRelaxNGCompile
    fn compile(&mut self, def: Option<DefId>) -> i32 {
        let d = match def {
            None => return -1,
            Some(d) => d,
        };
        let mut ret = 0;
        match self.d(d).typ {
            T::Start => {
                if self.is_compilable(Some(d)) == 1 && self.d(d).depth != -25 {
                    self.compile_content(d, true);
                }
            }
            T::Element => {
                if let (true, Some(name)) = (self.am.is_some(), self.d(d).name.clone()) {
                    let ns = self.d(d).ns.clone();
                    let from = self.am_state();
                    if let Some(am) = self.am.as_mut() {
                        self.state = am.new_transition2(from, None, &name, ns.as_deref(), Some(d));
                    }
                }
                if self.d(d).dflags & IS_COMPILABLE != 0 && self.d(d).depth != -25 {
                    self.compile_content(d, false);
                } else {
                    let oldam = self.am.take();
                    let oldstate = self.state;
                    self.am = oldam;
                    ret = self.try_compile(Some(d));
                    // xmlRelaxNGTryCompile resets ctxt->am; restore it
                    self.state = self.state.or(oldstate);
                }
            }
            T::Noop => {
                let c = self.d(d).content;
                ret = self.compile(c);
            }
            T::Optional => {
                let oldstate = self.am_state();
                let mut list = self.d(d).content;
                while let Some(l) = list {
                    self.compile(Some(l));
                    list = self.d(l).next;
                }
                let st = self.am_state();
                self.am.as_mut().unwrap().new_epsilon(oldstate, Some(st));
            }
            T::ZeroOrMore => {
                let s = self.am_state();
                let ns = self.am.as_mut().unwrap().new_epsilon(s, None);
                self.state = Some(ns);
                let oldstate = ns;
                let mut list = self.d(d).content;
                while let Some(l) = list {
                    self.compile(Some(l));
                    list = self.d(l).next;
                }
                let st = self.am_state();
                let am = self.am.as_mut().unwrap();
                am.new_epsilon(st, Some(oldstate));
                let ns = am.new_epsilon(oldstate, None);
                self.state = Some(ns);
            }
            T::OneOrMore => {
                let mut list = self.d(d).content;
                while let Some(l) = list {
                    self.compile(Some(l));
                    list = self.d(l).next;
                }
                let oldstate = self.am_state();
                let mut list = self.d(d).content;
                while let Some(l) = list {
                    self.compile(Some(l));
                    list = self.d(l).next;
                }
                let st = self.am_state();
                let am = self.am.as_mut().unwrap();
                am.new_epsilon(st, Some(oldstate));
                let ns = am.new_epsilon(oldstate, None);
                self.state = Some(ns);
            }
            T::Choice => {
                let mut target: Option<StateId> = None;
                let oldstate = self.state;
                let mut list = self.d(d).content;
                while let Some(l) = list {
                    self.state = oldstate;
                    ret = self.compile(Some(l));
                    if ret != 0 {
                        break;
                    }
                    match target {
                        None => target = self.state,
                        Some(t) => {
                            let st = self.am_state();
                            self.am.as_mut().unwrap().new_epsilon(st, Some(t));
                        }
                    }
                    list = self.d(l).next;
                }
                self.state = target;
            }
            T::Ref | T::ExternalRef | T::ParentRef | T::Group | T::Def => {
                let mut list = self.d(d).content;
                while let Some(l) = list {
                    ret = self.compile(Some(l));
                    if ret != 0 {
                        break;
                    }
                    list = self.d(l).next;
                }
            }
            T::Text => {
                let s = self.am_state();
                let ns = self.am.as_mut().unwrap().new_epsilon(s, None);
                self.state = Some(ns);
                let oldstate = ns;
                let c = self.d(d).content;
                self.compile(c);
                let st = self.am_state();
                let am = self.am.as_mut().unwrap();
                am.new_transition(st, Some(st), "#text", None);
                let ns = am.new_epsilon(oldstate, None);
                self.state = Some(ns);
            }
            T::Empty => {
                let s = self.am_state();
                let ns = self.am.as_mut().unwrap().new_epsilon(s, None);
                self.state = Some(ns);
            }
            _ => {
                // libxml2 prints "RNG internal error trying to compile"
            }
        }
        ret
    }

    /// xmlRelaxNGTryCompile
    fn try_compile(&mut self, def: Option<DefId>) -> i32 {
        let d = match def {
            None => return -1,
            Some(d) => d,
        };
        let typ = self.d(d).typ;
        if typ == T::Start || typ == T::Element {
            self.is_compilable(Some(d));
            if self.d(d).dflags & IS_COMPILABLE != 0 && self.d(d).depth != -25 {
                let saved = self.am.take();
                let r = self.compile(Some(d));
                self.am = saved;
                return r;
            }
        }
        let mut ret = 0;
        match typ {
            T::Noop => {
                let c = self.d(d).content;
                ret = self.try_compile(c);
            }
            T::Optional
            | T::ZeroOrMore
            | T::OneOrMore
            | T::Choice
            | T::Group
            | T::Def
            | T::Start
            | T::Ref
            | T::ExternalRef
            | T::ParentRef => {
                let mut list = self.d(d).content;
                while let Some(l) = list {
                    ret = self.try_compile(Some(l));
                    if ret != 0 {
                        break;
                    }
                    list = self.d(l).next;
                }
            }
            _ => ret = 0,
        }
        ret
    }
}

/// xmlRelaxNGGroupContentType
fn group_content_type(ct1: i32, ct2: i32) -> i32 {
    if ct1 == CT_ERROR || ct2 == CT_ERROR {
        return CT_ERROR;
    }
    if ct1 == CT_EMPTY {
        return ct2;
    }
    if ct2 == CT_EMPTY {
        return ct1;
    }
    if ct1 == CT_COMPLEX && ct2 == CT_COMPLEX {
        return CT_COMPLEX;
    }
    CT_ERROR
}

/// xmlRelaxNGMaxContentType
fn max_content_type(ct1: i32, ct2: i32) -> i32 {
    if ct1 == CT_ERROR || ct2 == CT_ERROR {
        return CT_ERROR;
    }
    if ct1 == CT_SIMPLE || ct2 == CT_SIMPLE {
        return CT_SIMPLE;
    }
    if ct1 == CT_COMPLEX || ct2 == CT_COMPLEX {
        return CT_COMPLEX;
    }
    CT_EMPTY
}

enum Source<'b> {
    Url(&'b str),
    Memory(&'b [u8]),
}

impl<'a> PCtxt<'a> {
    /// xmlRelaxNGParse
    fn parse(&mut self, src: Source) -> Option<RelaxNg> {
        let doc = match src {
            Source::Url(u) => {
                self.url = Some(u.to_string());
                match self.read_file(u) {
                    Some(d) => d,
                    None => {
                        self.perr(
                            None,
                            xe::RNGP_PARSE_ERROR,
                            format!("xmlRelaxNGParse: could not load {}\n", u),
                        );
                        return None;
                    }
                }
            }
            Source::Memory(b) => {
                let d = match self.parse_bytes(b, None) {
                    Some(d) => d,
                    None => {
                        self.perr(
                            None,
                            xe::RNGP_PARSE_ERROR,
                            "xmlRelaxNGParse: could not parse schemas\n".to_string(),
                        );
                        return None;
                    }
                };
                tree::with(|t| {
                    if let Some(di) = t.doc_info_mut(d) {
                        di.url = Some("in_memory_buffer".to_string());
                    }
                });
                self.url = Some("in_memory_buffer".to_string());
                d
            }
        };
        self.cleanup_doc(doc)?;
        let root = match root_element(doc) {
            Some(r) => r,
            None => {
                let u = self.url.clone().unwrap_or_else(|| "schemas".to_string());
                self.perr(
                    Some(doc),
                    xe::RNGP_EMPTY,
                    format!("xmlRelaxNGParse: {} is empty\n", u),
                );
                return None;
            }
        };
        let top = self.parse_document(root)?;
        if let Some(il) = self.interleaves.clone() {
            for (_, d) in il {
                self.compute_interleaves(d);
            }
        }
        if self.nb_errors > 0 {
            return None;
        }
        if let Some(s) = self.grammars[top].start {
            if self.d(s).typ != T::Start {
                let def = self.new_def(None);
                self.dm(def).typ = T::Start;
                self.dm(def).content = Some(s);
                self.grammars[top].start = Some(def);
            }
            let s = self.grammars[top].start;
            self.am = None;
            self.try_compile(s);
        }
        Some(RelaxNg {
            defs: std::mem::take(&mut self.defs),
            start: self.grammars[top].start,
            idref: self.idref,
            doc: Some(doc),
        })
    }
}

/// Parse a RelaxNG schema from a file or URL (xmlRelaxNGNewParserCtxt +
/// xmlRelaxNGParse). Every error is passed to `sink` when it is raised.
pub fn parse_file(vm: &mut Vm, url: &str, sink: Sink) -> Option<Rc<RelaxNg>> {
    let mut p = PCtxt::new(vm, sink);
    p.parse(Source::Url(url)).map(Rc::new)
}

/// Parse a RelaxNG schema from memory (xmlRelaxNGNewMemParserCtxt +
/// xmlRelaxNGParse); the schema document gets the URL "in_memory_buffer".
pub fn parse_memory(vm: &mut Vm, data: &[u8], sink: Sink) -> Option<Rc<RelaxNg>> {
    let mut p = PCtxt::new(vm, sink);
    p.parse(Source::Memory(data)).map(Rc::new)
}

/// `parse_file` collecting the errors instead.
pub fn parse_file_collect(vm: &mut Vm, url: &str) -> (Option<Rc<RelaxNg>>, Vec<XmlError>) {
    let mut errs = Vec::new();
    let r = parse_file(vm, url, &mut |_, e| errs.push(e));
    (r, errs)
}

/// `parse_memory` collecting the errors instead.
pub fn parse_memory_collect(vm: &mut Vm, data: &[u8]) -> (Option<Rc<RelaxNg>>, Vec<XmlError>) {
    let mut errs = Vec::new();
    let r = parse_memory(vm, data, &mut |_, e| errs.push(e));
    (r, errs)
}

// ---------------------------------------------------------------------------
// Validation

/// A pointer into a NUL-terminated string buffer (values are scanned with
/// C string semantics, lists are split in place by replacing blanks with
/// NULs).
#[derive(Clone)]
struct VPtr {
    buf: Rc<Vec<u8>>,
    off: usize,
}

impl VPtr {
    fn new(s: Vec<u8>) -> VPtr {
        VPtr {
            buf: Rc::new(s),
            off: 0,
        }
    }
    fn at(&self, i: usize) -> u8 {
        self.buf.get(self.off + i).copied().unwrap_or(0)
    }
    fn cstr(&self) -> &[u8] {
        let s = &self.buf[self.off.min(self.buf.len())..];
        match s.iter().position(|&c| c == 0) {
            Some(p) => &s[..p],
            None => s,
        }
    }
    fn string(&self) -> String {
        String::from_utf8_lossy(self.cstr()).into_owned()
    }
    fn same(a: &Option<VPtr>, b: &Option<VPtr>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(x), Some(y)) => Rc::ptr_eq(&x.buf, &y.buf) && x.off == y.off,
            _ => false,
        }
    }
}

/// xmlRelaxNGValidState
#[derive(Clone)]
struct VState {
    node: Option<NodeId>,
    seq: Option<NodeId>,
    attrs: Vec<Option<NodeId>>,
    nb_attr_left: i32,
    value: Option<VPtr>,
    endvalue: Option<VPtr>,
}

type SRef = Rc<RefCell<VState>>;

fn copy_state(s: &SRef) -> SRef {
    Rc::new(RefCell::new(s.borrow().clone()))
}

/// xmlRelaxNGEqualValidState
fn equal_states(a: &SRef, b: &SRef) -> bool {
    if Rc::ptr_eq(a, b) {
        return true;
    }
    let a = a.borrow();
    let b = b.borrow();
    if a.node != b.node
        || a.seq != b.seq
        || a.nb_attr_left != b.nb_attr_left
        || a.attrs.len() != b.attrs.len()
    {
        return false;
    }
    if !VPtr::same(&a.endvalue, &b.endvalue) {
        return false;
    }
    if !VPtr::same(&a.value, &b.value) {
        match (&a.value, &b.value) {
            (Some(x), Some(y)) if x.cstr() == y.cstr() => {}
            _ => return false,
        }
    }
    a.attrs == b.attrs
}

/// xmlRelaxNGAddStates: 1 added, 0 duplicate (dropped)
fn add_states(states: &mut Vec<SRef>, state: SRef) -> i32 {
    for s in states.iter() {
        if equal_states(&state, s) {
            return 0;
        }
    }
    states.push(state);
    1
}

/// A stacked validation error (xmlRelaxNGValidError).
#[derive(Clone)]
struct VErr {
    err: i32,
    flags: i32,
    node: Option<NodeId>,
    seq: Option<NodeId>,
    arg1: Option<String>,
    arg2: Option<String>,
}

/// A RelaxNG validation context (xmlRelaxNGValidCtxt).
pub struct ValidCtxt {
    schema: Rc<RelaxNg>,
    errors: Vec<XmlError>,
    flags: i32,
    depth: i32,
    idref: bool,
    err_no: i32,
    err_tab: Vec<VErr>,
    err_nr: usize,
    err_max: usize,
    err: Option<usize>,
    state: Option<SRef>,
    states: Option<Vec<SRef>>,
    elem_tab: Vec<ExecCtxt>,
    pstate: i32,
    pnode: Option<NodeId>,
    pdef: Option<DefId>,
    perr: i32,
    doc: Option<NodeId>,
    psvi: HashMap<NodeId, DefId>,
}

/// xmlRelaxNGGetErrorString
fn error_string(err: i32, arg1: Option<&str>, arg2: Option<&str>) -> Option<String> {
    let a1 = arg1.unwrap_or("");
    let a2 = arg2.unwrap_or("");
    let msg = match err {
        ERR_OK => return None,
        ERR_MEMORY => return Some("out of memory\n".to_string()),
        ERR_TYPE => format!("failed to validate type {}\n", a1),
        ERR_TYPEVAL => format!("Type {} doesn't allow value '{}'\n", a1, a2),
        ERR_DUPID => format!("ID {} redefined\n", a1),
        ERR_TYPECMP => format!("failed to compare type {}\n", a1),
        ERR_NOSTATE => return Some("Internal error: no state\n".to_string()),
        ERR_NODEFINE => return Some("Internal error: no define\n".to_string()),
        ERR_INTERNAL => format!("Internal error: {}\n", a1),
        ERR_LISTEXTRA => format!("Extra data in list: {}\n", a1),
        ERR_INTERNODATA => return Some("Internal: interleave block has no data\n".to_string()),
        ERR_INTERSEQ => return Some("Invalid sequence in interleave\n".to_string()),
        ERR_INTEREXTRA => format!("Extra element {} in interleave\n", a1),
        ERR_ELEMNAME => format!("Expecting element {}, got {}\n", a1, a2),
        ERR_ELEMNONS => format!("Expecting a namespace for element {}\n", a1),
        ERR_ELEMWRONGNS => format!("Element {} has wrong namespace: expecting {}\n", a1, a2),
        ERR_ELEMWRONG => format!("Did not expect element {} there\n", a1),
        ERR_TEXTWRONG => format!("Did not expect text in element {} content\n", a1),
        ERR_ELEMEXTRANS => format!("Expecting no namespace for element {}\n", a1),
        ERR_ELEMNOTEMPTY => format!("Expecting element {} to be empty\n", a1),
        ERR_NOELEM => format!("Expecting an element {}, got nothing\n", a1),
        ERR_NOTELEM => return Some("Expecting an element got text\n".to_string()),
        ERR_ATTRVALID => format!("Element {} failed to validate attributes\n", a1),
        ERR_CONTENTVALID => format!("Element {} failed to validate content\n", a1),
        ERR_EXTRACONTENT => format!("Element {} has extra content: {}\n", a1, a2),
        ERR_INVALIDATTR => format!("Invalid attribute {} for element {}\n", a1, a2),
        ERR_LACKDATA => format!("Datatype element {} contains no data\n", a1),
        ERR_DATAELEM => format!("Datatype element {} has child elements\n", a1),
        ERR_VALELEM => format!("Value element {} has child elements\n", a1),
        ERR_LISTELEM => format!("List element {} has child elements\n", a1),
        ERR_DATATYPE => format!("Error validating datatype {}\n", a1),
        ERR_VALUE => format!("Error validating value {}\n", a1),
        ERR_LIST => return Some("Error validating list\n".to_string()),
        ERR_NOGRAMMAR => return Some("No top grammar defined\n".to_string()),
        ERR_EXTRADATA => return Some("Extra data in the document\n".to_string()),
        _ => return Some("Unknown error !\n".to_string()),
    };
    // snprintf(msg, 1000, ...)
    if msg.len() > 999 {
        let b = &msg.as_bytes()[..999];
        return Some(String::from_utf8_lossy(b).into_owned());
    }
    Some(msg)
}

impl ValidCtxt {
    /// xmlRelaxNGNewValidCtxt
    pub fn new(schema: Rc<RelaxNg>) -> ValidCtxt {
        let idref = schema.idref;
        ValidCtxt {
            schema,
            errors: Vec::new(),
            flags: 0,
            depth: 0,
            idref,
            err_no: ERR_OK,
            err_tab: Vec::new(),
            err_nr: 0,
            err_max: 0,
            err: None,
            state: None,
            states: None,
            elem_tab: Vec::new(),
            pstate: 0,
            pnode: None,
            pdef: None,
            perr: 0,
            doc: None,
            psvi: HashMap::new(),
        }
    }

    /// Take the errors raised so far.
    pub fn take_errors(&mut self) -> Vec<XmlError> {
        std::mem::take(&mut self.errors)
    }

    fn defs(&self) -> &[Define] {
        &self.schema.defs
    }

    // -----------------------------------------------------------------
    // Errors

    /// xmlRngVErr
    fn rng_verr(&mut self, node: Option<NodeId>, err: i32, msg: String) {
        self.errors
            .push(node_error(xe::FROM_RELAXNGV, err, ERR_ERROR, node, msg));
    }

    /// xmlRelaxNGShowValidError
    fn show_valid_error(
        &mut self,
        err: i32,
        node: Option<NodeId>,
        child: Option<NodeId>,
        arg1: Option<&str>,
        arg2: Option<&str>,
    ) {
        if self.flags & FLAGS_NOERROR != 0 {
            return;
        }
        let msg = match error_string(err, arg1, arg2) {
            None => return,
            Some(m) => m,
        };
        if self.err_no == ERR_OK {
            self.err_no = err;
        }
        self.rng_verr(child.or(node), err, msg);
    }

    fn state_node_seq(&self) -> (Option<NodeId>, Option<NodeId>) {
        match &self.state {
            Some(s) => {
                let s = s.borrow();
                (s.node, s.seq)
            }
            None => (None, None),
        }
    }

    /// xmlRelaxNGValidErrorPush
    fn valid_error_push(&mut self, err: i32, arg1: Option<&str>, arg2: Option<&str>, dup: bool) {
        if self.err_max == 0 {
            self.err_max = 8;
            self.err_nr = 0;
            self.err = None;
        }
        if self.err_nr >= self.err_max {
            self.err_max *= 2;
            self.err = Some(self.err_nr - 1);
        }
        if let (Some(e), Some(st)) = (self.err, &self.state) {
            if self.err_tab[e].node == st.borrow().node && self.err_tab[e].err == err {
                return;
            }
        }
        let (node, seq) = self.state_node_seq();
        let cur = VErr {
            err,
            flags: if dup { ERROR_IS_DUP } else { 0 },
            node,
            seq,
            arg1: arg1.map(|s| s.to_string()),
            arg2: arg2.map(|s| s.to_string()),
        };
        if self.err_nr < self.err_tab.len() {
            self.err_tab[self.err_nr] = cur;
        } else {
            self.err_tab.push(cur);
        }
        self.err = Some(self.err_nr);
        self.err_nr += 1;
    }

    fn free_dup_args(&mut self, i: usize) {
        let e = &mut self.err_tab[i];
        if e.flags & ERROR_IS_DUP != 0 {
            e.arg1 = None;
            e.arg2 = None;
            e.flags = 0;
        }
    }

    /// xmlRelaxNGValidErrorPop
    fn valid_error_pop(&mut self) {
        if self.err_nr == 0 {
            self.err = None;
            return;
        }
        self.err_nr -= 1;
        self.err = if self.err_nr > 0 {
            Some(self.err_nr - 1)
        } else {
            None
        };
        self.free_dup_args(self.err_nr);
    }

    /// xmlRelaxNGPopErrors
    fn pop_errors(&mut self, level: usize) {
        for i in level..self.err_nr {
            self.free_dup_args(i);
        }
        self.err_nr = level;
        if self.err_nr == 0 {
            self.err = None;
        }
    }

    /// xmlRelaxNGDumpValidError
    fn dump_valid_error(&mut self) {
        let mut k = 0;
        for i in 0..self.err_nr {
            if k < MAX_ERROR {
                let e = self.err_tab[i].clone();
                let dupd = (0..i).any(|j| {
                    let d = &self.err_tab[j];
                    e.err == d.err && e.node == d.node && e.arg1 == d.arg1 && e.arg2 == d.arg2
                });
                if !dupd {
                    self.show_valid_error(
                        e.err,
                        e.node,
                        e.seq,
                        e.arg1.as_deref(),
                        e.arg2.as_deref(),
                    );
                    k += 1;
                }
            }
            self.free_dup_args(i);
        }
        self.err_nr = 0;
    }

    /// xmlRelaxNGAddValidError
    fn add_valid_error(&mut self, err: i32, arg1: Option<&str>, arg2: Option<&str>, dup: bool) {
        if self.flags & FLAGS_NOERROR != 0 {
            return;
        }
        if self.flags & FLAGS_IGNORABLE == 0 || self.flags & FLAGS_NEGATIVE != 0 {
            if self.err_nr != 0 {
                self.dump_valid_error();
            }
            let (mut node, seq) = self.state_node_seq();
            if node.is_none() && seq.is_none() {
                node = self.pnode;
            }
            self.show_valid_error(err, node, seq, arg1, arg2);
        } else {
            self.valid_error_push(err, arg1, arg2, dup);
        }
    }

    fn verr(&mut self, err: i32) {
        self.add_valid_error(err, None, None, false);
    }

    fn verr2(&mut self, err: i32, a: Option<&str>) {
        self.add_valid_error(err, a, None, false);
    }

    fn verr3(&mut self, err: i32, a: Option<&str>, b: Option<&str>) {
        self.add_valid_error(err, a, b, false);
    }

    fn dump_if_not_ignorable(&mut self) {
        if self.flags & FLAGS_IGNORABLE == 0 {
            self.dump_valid_error();
        }
    }

    // -----------------------------------------------------------------
    // States

    fn seq(&self) -> Option<NodeId> {
        self.state.as_ref().and_then(|s| s.borrow().seq)
    }

    fn set_seq(&self, v: Option<NodeId>) {
        if let Some(s) = &self.state {
            s.borrow_mut().seq = v;
        }
    }

    /// xmlRelaxNGNewValidState
    fn new_valid_state(&self, node: Option<NodeId>) -> Option<SRef> {
        let (n, seq, attrs) = match node {
            None => {
                let doc = self.doc?;
                let root = root_element(doc)?;
                (Some(doc), Some(root), Vec::new())
            }
            Some(n) => tree::with(|t| {
                let mut attrs = Vec::new();
                let mut a = if t.nodes[n].t == NType::Element {
                    t.nodes[n].properties
                } else {
                    None
                };
                while let Some(x) = a {
                    attrs.push(Some(x));
                    a = t.nodes[x].next;
                }
                (Some(n), t.nodes[n].children, attrs)
            }),
        };
        let nb = attrs.len() as i32;
        Some(Rc::new(RefCell::new(VState {
            node: n,
            seq,
            attrs,
            nb_attr_left: nb,
            value: None,
            endvalue: None,
        })))
    }

    /// xmlRelaxNGSkipIgnored
    fn skip_ignored(&self, node: Option<NodeId>) -> Option<NodeId> {
        let mixed = self.flags & FLAGS_MIXED_CONTENT != 0;
        let mut node = node;
        while let Some(n) = node {
            let skip = match ntype(n) {
                NType::Comment | NType::Pi => true,
                NType::Text | NType::CData => mixed || is_blank_node(n),
                _ => false,
            };
            if !skip {
                break;
            }
            node = nnext(n);
        }
        node
    }

    // -----------------------------------------------------------------
    // Compiled content

    fn run_compiled_callbacks(&mut self, cbs: Vec<(Option<usize>, Option<usize>)>) {
        for (td, _) in cbs {
            let def = match td {
                None => continue,
                Some(d) => d,
            };
            if self.defs()[def].typ != T::Element {
                if self.err_no == ERR_OK {
                    self.err_no = ERR_INTERNAL;
                }
                continue;
            }
            let ret = self.validate_definition(Some(def));
            if ret != 0 {
                self.perr = ret;
            }
        }
    }

    fn push_node(exec: &mut ExecCtxt, n: NodeId) -> i32 {
        let (name, ns) = tree::with(|t| {
            (
                t.nodes[n].name.clone(),
                t.nodes[n].ns.map(|x| t.nss[x].href.clone()),
            )
        });
        match ns {
            Some(h) => exec.push_string2(&name, Some(&h), Some(0)),
            None => exec.push_string(Some(&name), Some(0)),
        }
    }

    /// xmlRelaxNGValidateCompiledContent
    fn validate_compiled_content(&mut self, regexp: Rc<Regexp>, content: Option<NodeId>) -> i32 {
        let oldperr = self.perr;
        let mut exec = ExecCtxt::new(regexp);
        self.perr = 0;
        let mut ret = 0;
        let mut cur = content;
        while let Some(c) = cur {
            self.set_seq(Some(c));
            match ntype(c) {
                NType::Text | NType::CData => {
                    if !xml_is_blank_node(c) {
                        ret = exec.push_string(Some("#text"), Some(0));
                        let cbs = std::mem::take(&mut exec.callbacks);
                        self.run_compiled_callbacks(cbs);
                        if ret < 0 {
                            let pn = nparent(c).map(nname);
                            self.verr2(ERR_TEXTWRONG, pn.as_deref());
                        }
                    }
                }
                NType::Element => {
                    ret = Self::push_node(&mut exec, c);
                    let cbs = std::mem::take(&mut exec.callbacks);
                    self.run_compiled_callbacks(cbs);
                    if ret < 0 {
                        let n = nname(c);
                        self.verr2(ERR_ELEMWRONG, Some(&n));
                    }
                }
                _ => {}
            }
            if ret < 0 {
                break;
            }
            cur = nnext(c);
        }
        ret = exec.push_string(None, None);
        if ret == 1 {
            ret = 0;
            self.set_seq(None);
        } else if ret == 0 {
            self.verr2(ERR_NOELEM, Some(""));
            ret = -1;
            self.dump_if_not_ignorable();
        } else {
            ret = -1;
        }
        if ret == 0 && self.perr != 0 {
            ret = self.perr;
        }
        self.perr = oldperr;
        ret
    }

    // -----------------------------------------------------------------
    // Progressive validation

    fn run_progressive_callbacks(&mut self, cbs: Vec<(Option<usize>, Option<usize>)>) {
        for (td, _) in cbs {
            self.progressive_callback(td);
        }
    }

    /// xmlRelaxNGValidateProgressiveCallback
    fn progressive_callback(&mut self, define: Option<DefId>) {
        let node = self.pnode;
        self.pstate = 1;
        let def = match define {
            None => return,
            Some(d) => d,
        };
        if self.defs()[def].typ != T::Element {
            if self.err_no == ERR_OK {
                self.err_no = ERR_INTERNAL;
            }
            self.pstate = -1;
            return;
        }
        let node = match node {
            Some(n) if ntype(n) == NType::Element => n,
            _ => {
                self.verr(ERR_NOTELEM);
                self.dump_if_not_ignorable();
                self.pstate = -1;
                return;
            }
        };
        let cm = match &self.defs()[def].cont_model {
            None => {
                self.pstate = 0;
                self.pdef = Some(def);
                return;
            }
            Some(c) => c.clone(),
        };
        self.elem_tab.push(ExecCtxt::new(cm));
        let state = match self.new_valid_state(Some(node)) {
            None => {
                self.pstate = -1;
                return;
            }
            Some(s) => s,
        };
        let oldstate = self.state.replace(state);
        let mut ret = 0;
        if let Some(a) = self.defs()[def].attrs {
            ret = self.validate_attribute_list(Some(a));
            if ret != 0 {
                self.pstate = -1;
                let n = nname(node);
                self.verr2(ERR_ATTRVALID, Some(&n));
            }
        }
        if self.state.is_some() {
            self.set_seq(None);
            if self.validate_element_end(true) != 0 {
                self.pstate = -1;
            }
        } else if let Some(sts) = self.states.clone() {
            let mut tmp = -1;
            let oldflags = self.flags;
            for s in &sts {
                self.state = Some(s.clone());
                self.set_seq(None);
                if self.validate_element_end(false) == 0 {
                    tmp = 0;
                    break;
                }
            }
            if tmp != 0 {
                self.flags |= FLAGS_IGNORABLE;
                self.log_best_error();
            }
            self.states = None;
            if ret == 0 && tmp == -1 {
                self.pstate = -1;
            }
            self.flags = oldflags;
        }
        if self.pstate == -1 {
            self.dump_if_not_ignorable();
        }
        self.state = oldstate;
    }

    /// xmlRelaxNGValidatePushElement: 1 if no validation problem was found,
    /// 0 if the element requires a full subtree (call `full_element` once
    /// expanded), -1 on error.
    pub fn push_element(&mut self, elem: NodeId) -> i32 {
        if self.doc.is_none() {
            self.doc = tree::with(|t| t.nodes[elem].doc);
        }
        if self.elem_tab.is_empty() {
            let define = match self.schema.start {
                None => {
                    self.verr(ERR_NOGRAMMAR);
                    return -1;
                }
                Some(d) => d,
            };
            let cm = match &self.defs()[define].cont_model {
                None => {
                    self.pdef = Some(define);
                    return 0;
                }
                Some(c) => c.clone(),
            };
            self.elem_tab.push(ExecCtxt::new(cm));
        }
        self.pnode = Some(elem);
        self.pstate = 0;
        let ret = {
            let exec = self.elem_tab.last_mut().unwrap();
            Self::push_node(exec, elem)
        };
        let cbs = std::mem::take(&mut self.elem_tab.last_mut().unwrap().callbacks);
        // the callbacks may push the new element's exec context: take the
        // ones of the context we pushed to before running them
        self.run_progressive_callbacks(cbs);
        if ret < 0 {
            let n = nname(elem);
            self.verr2(ERR_ELEMWRONG, Some(&n));
            ret
        } else if self.pstate == 0 {
            0
        } else if self.pstate < 0 {
            -1
        } else {
            1
        }
    }

    /// xmlRelaxNGValidatePushCData: 1 if no validation problem was found,
    /// -1 otherwise.
    pub fn push_cdata(&mut self, data: &str) -> i32 {
        if self.elem_tab.is_empty() {
            return -1;
        }
        if is_blank_bytes(data.as_bytes()) {
            return 1;
        }
        let exec = self.elem_tab.last_mut().unwrap();
        let ret = exec.push_string(Some("#text"), Some(0));
        let cbs = std::mem::take(&mut exec.callbacks);
        self.run_progressive_callbacks(cbs);
        if ret < 0 {
            self.verr2(ERR_TEXTWRONG, Some(" TODO "));
            return -1;
        }
        1
    }

    /// xmlRelaxNGValidatePopElement: 1 if no validation problem was found,
    /// otherwise -1.
    pub fn pop_element(&mut self, _elem: NodeId) -> i32 {
        let mut exec = match self.elem_tab.pop() {
            None => return -1,
            Some(e) => e,
        };
        let ret = exec.push_string(None, None);
        if ret == 0 {
            self.verr2(ERR_NOELEM, Some(""));
            -1
        } else if ret < 0 {
            -1
        } else {
            1
        }
    }

    /// xmlRelaxNGValidateFullElement: validate a full subtree after
    /// `push_element` returned 0. 1 if valid, -1 otherwise.
    pub fn full_element(&mut self, elem: NodeId) -> i32 {
        let pdef = match self.pdef {
            None => return -1,
            Some(p) => p,
        };
        let state = match self.new_valid_state(nparent(elem)) {
            None => return -1,
            Some(s) => s,
        };
        state.borrow_mut().seq = Some(elem);
        self.state = Some(state);
        self.err_no = ERR_OK;
        let ret = self.validate_definition(Some(pdef));
        let r = if ret != 0 || self.err_no != ERR_OK {
            -1
        } else {
            1
        };
        self.state = None;
        r
    }

    // -----------------------------------------------------------------
    // Values

    /// xmlRelaxNGValidateDatatype
    fn validate_datatype(&mut self, value: Option<&[u8]>, def: DefId, node: Option<NodeId>) -> i32 {
        let schema = self.schema.clone();
        let d = &schema.defs[def];
        let lib = match d.lib {
            None => return -1,
            Some(l) => l,
        };
        let name = d.name.clone().unwrap_or_default();
        let vstr = value.map(|v| String::from_utf8_lossy(v).into_owned());
        let has_params = d
            .attrs
            .map(|a| schema.defs[a].typ == T::Param)
            .unwrap_or(false);
        let (r, result) = match &vstr {
            None => (-1, None),
            Some(v) => lib_check(lib, &name, v, has_params, node),
        };
        let mut ret;
        if r < 0 {
            self.verr2(ERR_TYPE, Some(&name));
            return -1;
        } else if r == 1 {
            ret = 0;
        } else if r == 2 {
            self.add_valid_error(ERR_DUPID, vstr.as_deref(), None, true);
            ret = 2;
        } else {
            self.add_valid_error(ERR_TYPEVAL, Some(&name), vstr.as_deref(), true);
            ret = -1;
        }
        let mut cur = d.attrs;
        while ret == 0 {
            let c = match cur {
                Some(c) if schema.defs[c].typ == T::Param => c,
                _ => break,
            };
            if let Some(t) = lib_facet(
                lib,
                &name,
                schema.defs[c].name.as_deref(),
                schema.defs[c].value.as_deref(),
                vstr.as_deref().unwrap_or(""),
                result.as_ref(),
                &mut self.errors,
            ) {
                if t != 0 {
                    ret = -1;
                }
            }
            cur = schema.defs[c].next;
        }
        if ret == 0 {
            if let Some(content) = d.content {
                if let Some(st) = self.state.clone() {
                    let (oldvalue, oldend) = {
                        let s = st.borrow();
                        (s.value.clone(), s.endvalue.clone())
                    };
                    {
                        let mut s = st.borrow_mut();
                        s.value = Some(VPtr::new(value.unwrap_or(b"").to_vec()));
                        s.endvalue = None;
                    }
                    ret = self.validate_value(Some(content));
                    let mut s = st.borrow_mut();
                    s.value = oldvalue;
                    s.endvalue = oldend;
                }
            }
        }
        ret
    }

    /// xmlRelaxNGNextValue
    fn next_value(&mut self) {
        let st = match &self.state {
            None => return,
            Some(s) => s.clone(),
        };
        let mut s = st.borrow_mut();
        let (cur, end) = match (&s.value, &s.endvalue) {
            (Some(c), Some(e)) => (c.clone(), e.clone()),
            _ => {
                s.value = None;
                s.endvalue = None;
                return;
            }
        };
        let mut off = cur.off;
        let len = cur.buf.len();
        while off < len && cur.buf[off] != 0 {
            off += 1;
        }
        let same_buf = Rc::ptr_eq(&cur.buf, &end.buf);
        while !(same_buf && off == end.off) && off < len && cur.buf[off] == 0 {
            off += 1;
        }
        if same_buf && off == end.off {
            s.value = None;
        } else {
            s.value = Some(VPtr {
                buf: cur.buf.clone(),
                off,
            });
        }
    }

    fn state_value(&self) -> Option<VPtr> {
        self.state.as_ref().and_then(|s| s.borrow().value.clone())
    }

    fn set_state_value(&self, v: Option<VPtr>) {
        if let Some(s) = &self.state {
            s.borrow_mut().value = v;
        }
    }

    /// xmlRelaxNGValidateValueList
    fn validate_value_list(&mut self, defines: Option<DefId>) -> i32 {
        let mut ret = 0;
        let mut d = defines;
        while let Some(x) = d {
            ret = self.validate_value(Some(x));
            if ret != 0 {
                break;
            }
            d = self.defs()[x].next;
        }
        ret
    }

    /// xmlRelaxNGValidateValue
    fn validate_value(&mut self, define: Option<DefId>) -> i32 {
        let def = match define {
            None => return -1,
            Some(d) => d,
        };
        let schema = self.schema.clone();
        let d = &schema.defs[def];
        let value = self.state_value();
        let mut ret = 0;
        match d.typ {
            T::Empty => {
                if let Some(v) = &value {
                    if v.at(0) != 0 {
                        let mut idx = 0;
                        while is_blank_ch(v.at(idx)) {
                            idx += 1;
                        }
                        if v.at(idx) != 0 {
                            ret = -1;
                        }
                    }
                }
            }
            T::Text => {}
            T::Value => {
                let vs = value.as_ref().map(|v| v.cstr().to_vec());
                let dv = d.value.clone().unwrap_or_default();
                if vs.as_deref() != Some(dv.as_bytes()) {
                    if let Some(name) = &d.name {
                        ret = match d.lib {
                            Some(l) => {
                                let v2 =
                                    vs.as_ref().map(|v| String::from_utf8_lossy(v).into_owned());
                                let snode = self.state.as_ref().and_then(|s| s.borrow().node);
                                match v2 {
                                    Some(v2) => lib_compare(
                                        l,
                                        name,
                                        &dv,
                                        d.node,
                                        d.val.as_ref(),
                                        &v2,
                                        snode,
                                    ),
                                    None => {
                                        if l == Lib::Builtin && name == "string" {
                                            0
                                        } else {
                                            -1
                                        }
                                    }
                                }
                            }
                            None => -1,
                        };
                        if ret < 0 {
                            self.verr2(ERR_TYPECMP, Some(name));
                            return -1;
                        } else if ret == 1 {
                            ret = 0;
                        } else {
                            ret = -1;
                        }
                    } else {
                        let nval = normalize(dv.as_bytes());
                        match &vs {
                            Some(v) if normalize(v) == nval => {}
                            _ => ret = -1,
                        }
                    }
                }
                if ret == 0 {
                    self.next_value();
                }
            }
            T::Datatype => {
                let vs = value.as_ref().map(|v| v.cstr().to_vec());
                let seq = self.seq();
                ret = self.validate_datatype(vs.as_deref(), def, seq);
                if ret == 0 {
                    self.next_value();
                }
            }
            T::Choice => {
                let oldflags = self.flags;
                self.flags |= FLAGS_IGNORABLE;
                let oldvalue = self.state_value();
                let mut list = d.content;
                while let Some(l) = list {
                    ret = self.validate_value(Some(l));
                    if ret == 0 {
                        break;
                    }
                    self.set_state_value(oldvalue.clone());
                    list = schema.defs[l].next;
                }
                self.flags = oldflags;
                if ret != 0 {
                    self.dump_if_not_ignorable();
                } else if self.err_nr > 0 {
                    self.pop_errors(0);
                }
            }
            T::List => {
                let st = match self.state.clone() {
                    None => return -1,
                    Some(s) => s,
                };
                let (oldvalue, oldend) = {
                    let s = st.borrow();
                    (s.value.clone(), s.endvalue.clone())
                };
                let mut buf: Vec<u8> = oldvalue
                    .as_ref()
                    .map(|v| v.cstr().to_vec())
                    .unwrap_or_default();
                for b in buf.iter_mut() {
                    if is_blank_ch(*b) {
                        *b = 0;
                    }
                }
                let len = buf.len();
                let rc = Rc::new(buf);
                let endv = VPtr {
                    buf: rc.clone(),
                    off: len,
                };
                let mut cur = 0;
                while cur < len && rc[cur] == 0 {
                    cur += 1;
                }
                {
                    let mut s = st.borrow_mut();
                    s.endvalue = Some(endv.clone());
                    s.value = Some(VPtr {
                        buf: rc.clone(),
                        off: cur,
                    });
                }
                let mut list = d.content;
                while let Some(l) = list {
                    {
                        let mut s = st.borrow_mut();
                        if VPtr::same(&s.value, &s.endvalue) {
                            s.value = None;
                        }
                    }
                    ret = self.validate_value(Some(l));
                    if ret != 0 {
                        break;
                    }
                    list = schema.defs[l].next;
                }
                let (cv, ce) = {
                    let s = st.borrow();
                    (s.value.clone(), s.endvalue.clone())
                };
                if let (0, Some(v)) = (ret, &cv) {
                    if !VPtr::same(&cv, &ce) {
                        let extra = v.string();
                        self.verr2(ERR_LISTEXTRA, Some(&extra));
                        ret = -1;
                    }
                }
                let mut s = st.borrow_mut();
                s.value = oldvalue;
                s.endvalue = oldend;
            }
            T::OneOrMore | T::ZeroOrMore => {
                if d.typ == T::OneOrMore {
                    ret = self.validate_value_list(d.content);
                    if ret != 0 {
                        return ret;
                    }
                }
                let v = self.state_value();
                match &v {
                    None => return 0,
                    Some(p) if p.at(0) == 0 => return 0,
                    _ => {}
                }
                let oldflags = self.flags;
                self.flags |= FLAGS_IGNORABLE;
                let mut cur = v;
                let mut temp: Option<VPtr> = None;
                loop {
                    let end = self
                        .state
                        .as_ref()
                        .and_then(|s| s.borrow().endvalue.clone());
                    if cur.is_none() || VPtr::same(&cur, &end) || VPtr::same(&temp, &cur) {
                        break;
                    }
                    temp = cur.clone();
                    ret = self.validate_value_list(d.content);
                    if ret != 0 {
                        self.set_state_value(temp.clone());
                        ret = 0;
                        break;
                    }
                    cur = self.state_value();
                }
                self.flags = oldflags;
                if self.err_nr > 0 {
                    self.pop_errors(0);
                }
            }
            T::Optional => {
                let v = self.state_value();
                match &v {
                    None => return 0,
                    Some(p) if p.at(0) == 0 => return 0,
                    _ => {}
                }
                let oldflags = self.flags;
                self.flags |= FLAGS_IGNORABLE;
                let temp = v;
                ret = self.validate_value(d.content);
                self.flags = oldflags;
                if ret != 0 {
                    self.set_state_value(temp);
                    if self.err_nr > 0 {
                        self.pop_errors(0);
                    }
                    return 0;
                }
                if self.err_nr > 0 {
                    self.pop_errors(0);
                }
            }
            T::Except => {
                let mut list = d.content;
                while let Some(l) = list {
                    ret = self.validate_value(Some(l));
                    if ret == 0 {
                        ret = -1;
                        break;
                    } else {
                        ret = 0;
                    }
                    list = schema.defs[l].next;
                }
            }
            T::Def | T::Group => {
                let mut list = d.content;
                while let Some(l) = list {
                    ret = self.validate_value(Some(l));
                    if ret != 0 {
                        ret = -1;
                        break;
                    } else {
                        ret = 0;
                    }
                    list = schema.defs[l].next;
                }
            }
            T::Ref | T::ParentRef => match d.content {
                None => {
                    self.verr(ERR_NODEFINE);
                    ret = -1;
                }
                Some(c) => ret = self.validate_value(Some(c)),
            },
            _ => ret = -1,
        }
        ret
    }

    /// xmlRelaxNGValidateValueContent
    fn validate_value_content(&mut self, defines: Option<DefId>) -> i32 {
        self.validate_value_list(defines)
    }

    // -----------------------------------------------------------------
    // Attributes

    /// xmlRelaxNGAttributeMatch
    fn attribute_match(&self, define: DefId, prop: NodeId) -> i32 {
        let defs = self.defs();
        let d = &defs[define];
        let (pname, pns) = tree::with(|t| {
            (
                t.nodes[prop].name.clone(),
                t.nodes[prop].ns.map(|x| t.nss[x].href.clone()),
            )
        });
        if let Some(n) = &d.name {
            if *n != pname {
                return 0;
            }
        }
        if let Some(dns) = &d.ns {
            if dns.is_empty() {
                if pns.is_some() {
                    return 0;
                }
            } else if pns.as_deref() != Some(dns.as_str()) {
                return 0;
            }
        }
        let nc = match d.name_class {
            None => return 1,
            Some(nc) => nc,
        };
        match defs[nc].typ {
            T::Except => {
                let mut list = defs[nc].content;
                while let Some(l) = list {
                    let r = self.attribute_match(l, prop);
                    if r == 1 {
                        return 0;
                    }
                    if r < 0 {
                        return r;
                    }
                    list = defs[l].next;
                }
                1
            }
            T::Choice => {
                let mut list = defs[nc].name_class;
                while let Some(l) = list {
                    let r = self.attribute_match(l, prop);
                    if r == 1 {
                        return 1;
                    }
                    if r < 0 {
                        return r;
                    }
                    list = defs[l].next;
                }
                0
            }
            _ => 0,
        }
    }

    /// xmlRelaxNGValidateAttribute
    fn validate_attribute(&mut self, define: DefId) -> i32 {
        let st = match self.state.clone() {
            None => return -1,
            Some(s) => s,
        };
        if st.borrow().nb_attr_left <= 0 {
            return -1;
        }
        let schema = self.schema.clone();
        let d = &schema.defs[define];
        let attrs = st.borrow().attrs.clone();
        let mut found: Option<(usize, NodeId)> = None;
        if let Some(name) = &d.name {
            for (i, a) in attrs.iter().enumerate() {
                let a = match a {
                    Some(a) => *a,
                    None => continue,
                };
                let (an, ans) = tree::with(|t| {
                    (
                        t.nodes[a].name.clone(),
                        t.nodes[a].ns.map(|x| t.nss[x].href.clone()),
                    )
                });
                if *name == an {
                    let dns_empty = d.ns.as_deref().map(|s| s.is_empty()).unwrap_or(true);
                    if (dns_empty && ans.is_none())
                        || (ans.is_some() && d.ns.is_some() && ans == d.ns)
                    {
                        found = Some((i, a));
                        break;
                    }
                }
            }
        } else {
            for (i, a) in attrs.iter().enumerate() {
                if let Some(a) = a {
                    if self.attribute_match(define, *a) == 1 {
                        found = Some((i, *a));
                        break;
                    }
                }
            }
        }
        let (i, prop) = match found {
            None => return -1,
            Some(f) => f,
        };
        let value = get_content(prop);
        let (oldvalue, oldseq) = {
            let s = st.borrow();
            (s.value.clone(), s.seq)
        };
        {
            let mut s = st.borrow_mut();
            s.seq = Some(prop);
            s.value = Some(VPtr::new(value.into_bytes()));
            if d.name.is_some() {
                s.endvalue = None;
            }
        }
        let ret = self.validate_value_content(d.content);
        {
            let mut s = st.borrow_mut();
            s.value = oldvalue;
            s.seq = oldseq;
            if ret == 0 {
                s.attrs[i] = None;
                s.nb_attr_left -= 1;
            }
        }
        ret
    }

    /// xmlRelaxNGValidateAttributeList
    fn validate_attribute_list(&mut self, defines: Option<DefId>) -> i32 {
        let mut ret = 0;
        let mut needmore = false;
        let mut cur = defines;
        while let Some(c) = cur {
            if self.defs()[c].typ == T::Attribute {
                if self.validate_attribute(c) != 0 {
                    ret = -1;
                }
            } else {
                needmore = true;
            }
            cur = self.defs()[c].next;
        }
        if !needmore {
            return ret;
        }
        let mut cur = defines;
        while let Some(c) = cur {
            if self.defs()[c].typ != T::Attribute {
                let res;
                if self.state.is_some() || self.states.is_some() {
                    res = self.validate_definition(Some(c));
                    if res < 0 {
                        ret = -1;
                    }
                } else {
                    self.verr(ERR_NOSTATE);
                    return -1;
                }
                if res == -1 {
                    break;
                }
            }
            cur = self.defs()[c].next;
        }
        ret
    }

    // -----------------------------------------------------------------
    // Elements

    /// xmlRelaxNGNodeMatchesList
    fn node_matches_list(&self, node: NodeId, list: Option<&Vec<DefId>>) -> bool {
        let list = match list {
            None => return false,
            Some(l) => l,
        };
        let ty = ntype(node);
        for &cur in list {
            let ct = self.defs()[cur].typ;
            if ty == NType::Element && ct == T::Element {
                let (name, ns) = tree::with(|t| {
                    (
                        t.nodes[node].name.clone(),
                        t.nodes[node].ns.map(|x| t.nss[x].href.clone()),
                    )
                });
                if element_match_noctx(self.defs(), cur, &name, ns.as_deref()) == 1 {
                    return true;
                }
            } else if matches!(ty, NType::Text | NType::CData)
                && matches!(ct, T::Datatype | T::List | T::Text | T::Value)
            {
                return true;
            }
        }
        false
    }

    /// xmlRelaxNGElementMatch (with error reporting)
    fn element_match(&mut self, define: DefId, elem: NodeId) -> i32 {
        let schema = self.schema.clone();
        let d = &schema.defs[define];
        let (ename, ens) = tree::with(|t| {
            (
                t.nodes[elem].name.clone(),
                t.nodes[elem].ns.map(|x| t.nss[x].href.clone()),
            )
        });
        let has_ns = has_ns(elem);
        if let Some(n) = &d.name {
            if *n != ename {
                self.verr3(ERR_ELEMNAME, Some(n), Some(&ename));
                return 0;
            }
        }
        match &d.ns {
            Some(dns) if !dns.is_empty() => {
                if !has_ns {
                    self.verr2(ERR_ELEMNONS, Some(&ename));
                    return 0;
                } else if ens.as_deref() != Some(dns.as_str()) {
                    self.verr3(ERR_ELEMWRONGNS, Some(&ename), Some(dns));
                    return 0;
                }
            }
            _ => {
                if has_ns && d.ns.is_some() && d.name.is_none() {
                    self.verr2(ERR_ELEMEXTRANS, Some(&ename));
                    return 0;
                } else if has_ns && d.name.is_some() {
                    self.verr2(ERR_ELEMEXTRANS, d.name.as_deref());
                    return 0;
                }
            }
        }
        let nc = match d.name_class {
            None => return 1,
            Some(nc) => nc,
        };
        let ret;
        match schema.defs[nc].typ {
            T::Except => {
                let oldflags = self.flags;
                self.flags |= FLAGS_IGNORABLE;
                let mut list = schema.defs[nc].content;
                while let Some(l) = list {
                    let r = self.element_match(l, elem);
                    if r == 1 {
                        self.flags = oldflags;
                        return 0;
                    }
                    if r < 0 {
                        self.flags = oldflags;
                        return r;
                    }
                    list = schema.defs[l].next;
                }
                ret = 1;
                self.flags = oldflags;
            }
            T::Choice => {
                let oldflags = self.flags;
                self.flags |= FLAGS_IGNORABLE;
                let mut r = 0;
                let mut list = schema.defs[nc].name_class;
                while let Some(l) = list {
                    r = self.element_match(l, elem);
                    if r == 1 {
                        self.flags = oldflags;
                        return 1;
                    }
                    if r < 0 {
                        self.flags = oldflags;
                        return r;
                    }
                    list = schema.defs[l].next;
                }
                if r != 0 {
                    self.dump_if_not_ignorable();
                } else if self.err_nr > 0 {
                    self.pop_errors(0);
                }
                ret = 0;
                self.flags = oldflags;
            }
            _ => ret = -1,
        }
        ret
    }

    /// xmlRelaxNGBestState
    fn best_state(&self) -> i32 {
        let sts = match &self.states {
            Some(s) if !s.is_empty() => s,
            _ => return -1,
        };
        let mut best = -1;
        let mut value = 1000000;
        for (i, s) in sts.iter().enumerate() {
            let s = s.borrow();
            if s.seq.is_some() {
                if best == -1 || value > 100000 {
                    value = 100000;
                    best = i as i32;
                }
            } else {
                let tmp = s.nb_attr_left;
                if best == -1 || value > tmp {
                    value = tmp;
                    best = i as i32;
                }
            }
        }
        best
    }

    /// xmlRelaxNGLogBestError
    fn log_best_error(&mut self) {
        let n = match &self.states {
            Some(s) if !s.is_empty() => s.len(),
            _ => return,
        };
        let best = self.best_state();
        if best >= 0 && (best as usize) < n {
            self.state = Some(self.states.as_ref().unwrap()[best as usize].clone());
            self.validate_element_end(true);
        }
    }

    /// xmlRelaxNGValidateElementEnd
    fn validate_element_end(&mut self, dolog: bool) -> i32 {
        let st = match self.state.clone() {
            None => return 0,
            Some(s) => s,
        };
        let seq = st.borrow().seq;
        if seq.is_some() {
            let s = self.skip_ignored(seq);
            st.borrow_mut().seq = s;
            if let Some(s) = s {
                if dolog {
                    let nn = st.borrow().node.map(nname);
                    let sn = nname(s);
                    self.verr3(ERR_EXTRACONTENT, nn.as_deref(), Some(&sn));
                }
                return -1;
            }
        }
        let attrs = st.borrow().attrs.clone();
        for (i, a) in attrs.iter().enumerate() {
            if let Some(a) = a {
                if dolog {
                    let an = nname(*a);
                    let nn = st.borrow().node.map(nname);
                    self.verr3(ERR_INVALIDATTR, Some(&an), nn.as_deref());
                }
                return -1 - i as i32;
            }
        }
        0
    }

    // -----------------------------------------------------------------
    // Interleave

    fn triage_lookup<V: Copy>(triage: &HashMap<TriageKey, V>, node: NodeId) -> Option<V> {
        let (ty, name, ns) = tree::with(|t| {
            (
                t.nodes[node].t,
                t.nodes[node].name.clone(),
                t.nodes[node].ns.map(|x| t.nss[x].href.clone()),
            )
        });
        let get = |n: &str, ns: Option<&str>| {
            triage
                .get(&(n.to_string(), ns.map(|s| s.to_string())))
                .copied()
        };
        match ty {
            NType::Text | NType::CData => get("#text", None),
            NType::Element => {
                let mut r = match &ns {
                    Some(h) => get(&name, Some(h)).or_else(|| get("#any", Some(h))),
                    None => get(&name, None),
                };
                if r.is_none() {
                    r = get("#any", None);
                }
                r
            }
            _ => None,
        }
    }

    /// xmlRelaxNGValidateInterleave
    fn validate_interleave(&mut self, define: DefId) -> i32 {
        let schema = self.schema.clone();
        let d = &schema.defs[define];
        let err_nr = self.err_nr;
        let part = match &d.partition {
            None => {
                self.verr(ERR_INTERNODATA);
                return -1;
            }
            Some(p) => p.clone(),
        };
        let nbgroups = part.nbgroups;
        let oldflags = self.flags;
        let mut ret;
        if d.dflags & IS_MIXED != 0 {
            self.flags |= FLAGS_MIXED_CONTENT;
            if nbgroups == 2 {
                if self.state.is_some() {
                    let s = self.skip_ignored(self.seq());
                    self.set_seq(s);
                }
                let rule = if schema.defs[part.groups[0].rule].typ == T::Text {
                    part.groups[1].rule
                } else {
                    part.groups[0].rule
                };
                ret = self.validate_definition(Some(rule));
                if ret == 0 && self.state.is_some() {
                    let s = self.skip_ignored(self.seq());
                    self.set_seq(s);
                }
                self.flags = oldflags;
                return ret;
            }
        }
        let mut list: Vec<Option<NodeId>> = vec![None; nbgroups];
        let mut lasts: Vec<Option<NodeId>> = vec![None; nbgroups];
        let mut cur = self.skip_ignored(self.seq());
        let start = cur;
        let mut lastchg: Option<NodeId> = None;
        ret = 0;
        while let Some(c) = cur {
            self.set_seq(Some(c));
            let i = if part.flags & IS_DETERMINIST != 0 {
                match Self::triage_lookup(&part.triage, c) {
                    None => nbgroups,
                    Some(v) => {
                        let i = v - 1;
                        if part.flags & IS_NEEDCHECK != 0
                            && !self.node_matches_list(c, part.groups[i].defs.as_ref())
                        {
                            nbgroups
                        } else {
                            i
                        }
                    }
                }
            } else {
                (0..nbgroups)
                    .find(|&i| self.node_matches_list(c, part.groups[i].defs.as_ref()))
                    .unwrap_or(nbgroups)
            };
            if i >= nbgroups {
                break;
            }
            match lasts[i] {
                Some(l) => {
                    set_next(l, Some(c));
                    lasts[i] = Some(c);
                }
                None => {
                    list[i] = Some(c);
                    lasts[i] = Some(c);
                }
            }
            let nx = nnext(c);
            lastchg = if nx.is_some() { nx } else { Some(c) };
            cur = self.skip_ignored(nx);
        }
        let lastelem = cur;
        let mut oldstate = self.state.clone();
        let mut last: Option<NodeId> = None;
        let mut done = false;
        for i in 0..nbgroups {
            self.state = oldstate.as_ref().map(copy_state);
            if self.state.is_none() {
                ret = -1;
                break;
            }
            let group = &part.groups[i];
            if let Some(l) = lasts[i] {
                last = nnext(l);
                set_next(l, None);
            }
            self.set_seq(list[i]);
            ret = self.validate_definition(Some(group.rule));
            if ret != 0 {
                break;
            }
            if self.state.is_some() {
                let c = self.skip_ignored(self.seq());
                oldstate = self.state.take();
                let open = d
                    .parent
                    .map(|p| {
                        schema.defs[p].typ == T::Def
                            && schema.defs[p].name.as_deref() == Some("open-name-class")
                    })
                    .unwrap_or(false);
                if let (Some(cn), false) = (c, open) {
                    let n = nname(cn);
                    self.verr2(ERR_INTEREXTRA, Some(&n));
                    ret = -1;
                    self.state = oldstate.clone();
                    done = true;
                    break;
                }
            } else if let Some(sts) = self.states.take() {
                let mut found = false;
                let mut best: i32 = -1;
                let mut lowattr = -1;
                let mut c: Option<NodeId> = None;
                for (j, s) in sts.iter().enumerate() {
                    c = self.skip_ignored(s.borrow().seq);
                    let nb = s.borrow().nb_attr_left;
                    if c.is_none() {
                        if !found {
                            lowattr = nb;
                            best = j as i32;
                        }
                        found = true;
                        if nb <= lowattr {
                            lowattr = nb;
                            best = j as i32;
                        }
                        if lowattr == 0 {
                            break;
                        }
                    } else if !found {
                        if lowattr == -1 || nb <= lowattr {
                            lowattr = nb;
                            best = j as i32;
                        }
                    }
                }
                if !sts.is_empty() {
                    oldstate = if best != -1 {
                        Some(sts[best as usize].clone())
                    } else {
                        Some(sts[sts.len() - 1].clone())
                    };
                }
                if !found {
                    let n = c.map(nname).unwrap_or_else(|| "noname".to_string());
                    self.verr2(ERR_INTEREXTRA, Some(&n));
                    ret = -1;
                    self.state = oldstate.clone();
                    done = true;
                    break;
                }
            } else {
                ret = -1;
                break;
            }
            if let Some(l) = lasts[i] {
                set_next(l, last);
            }
        }
        if !done {
            self.state = oldstate;
            self.set_seq(lastelem);
            if ret != 0 {
                self.verr(ERR_INTERSEQ);
                ret = -1;
            }
        }
        self.flags = oldflags;
        // rebuild the next links from the prev ones
        let mut c = lastchg;
        while let Some(x) = c {
            if Some(x) == start {
                break;
            }
            let p = match nprev(x) {
                None => break,
                Some(p) => p,
            };
            set_next(p, Some(x));
            c = Some(p);
        }
        if ret == 0 && self.err_nr > err_nr {
            self.pop_errors(err_nr);
        }
        ret
    }

    /// xmlRelaxNGValidateDefinitionList
    fn validate_definition_list(&mut self, defines: Option<DefId>) -> i32 {
        if defines.is_none() {
            self.verr2(ERR_INTERNAL, Some("NULL definition list"));
            return -1;
        }
        let mut ret = 0;
        let mut d = defines;
        while let Some(x) = d {
            let res;
            if self.state.is_some() || self.states.is_some() {
                res = self.validate_definition(Some(x));
                if res < 0 {
                    ret = -1;
                }
            } else {
                self.verr(ERR_NOSTATE);
                return -1;
            }
            if res == -1 {
                break;
            }
            d = self.defs()[x].next;
        }
        ret
    }

    /// Error pops after a successful element match (xmlRelaxNGValidateState)
    fn pop_matching_errors(&mut self, node: NodeId) {
        let nm = nname(node);
        while let Some(e) = self.err {
            let er = &self.err_tab[e];
            let m = (er.err == ERR_ELEMNAME && er.arg2.as_deref() == Some(nm.as_str()))
                || (er.err == ERR_ELEMEXTRANS && er.arg1.as_deref() == Some(nm.as_str()))
                || er.err == ERR_NOELEM
                || er.err == ERR_NOTELEM;
            if !m {
                break;
            }
            self.valid_error_pop();
        }
    }

    /// Text content of a node list (DATATYPE, VALUE and LIST): None when an
    /// element is found.
    fn collect_text(&mut self, node: Option<NodeId>, err: i32) -> Option<Vec<u8>> {
        let mut content: Vec<u8> = Vec::new();
        let mut child = node;
        while let Some(c) = child {
            match ntype(c) {
                NType::Element => {
                    let pn = node.and_then(nparent).map(nname);
                    self.verr2(err, pn.as_deref());
                    return None;
                }
                NType::Text | NType::CData => content.extend_from_slice(ncontent(c).as_bytes()),
                _ => {}
            }
            child = nnext(c);
        }
        Some(content)
    }

    fn validate_element_states(
        &mut self,
        nseq: Option<Option<NodeId>>,
        oldflags: i32,
        ret: &mut i32,
    ) {
        let sts = self.states.clone().unwrap();
        let mut tmp = -1;
        for s in &sts {
            self.state = Some(s.clone());
            if let Some(ns) = nseq {
                s.borrow_mut().seq = ns;
            }
            if self.validate_element_end(false) == 0 {
                tmp = 0;
                break;
            }
        }
        if tmp != 0 {
            self.flags |= FLAGS_IGNORABLE;
            self.log_best_error();
        }
        self.flags = oldflags;
        self.states = None;
        if *ret == 0 && tmp == -1 {
            *ret = -1;
        }
    }

    /// xmlRelaxNGValidateState
    fn validate_state(&mut self, define: Option<DefId>) -> i32 {
        let def = match define {
            None => {
                self.verr(ERR_NODEFINE);
                return -1;
            }
            Some(d) => d,
        };
        let schema = self.schema.clone();
        let d = &schema.defs[def];
        let mut node = self.seq();
        self.depth += 1;
        let mut ret = 0;
        match d.typ {
            T::Empty => ret = 0,
            T::NotAllowed => ret = -1,
            T::Text => {
                while let Some(n) = node {
                    if matches!(
                        ntype(n),
                        NType::Text | NType::Comment | NType::Pi | NType::CData
                    ) {
                        node = nnext(n);
                    } else {
                        break;
                    }
                }
                self.set_seq(node);
            }
            T::Element => {
                ret = self.validate_element(def, node);
            }
            T::Optional => {
                let err_nr = self.err_nr;
                let oldflags = self.flags;
                self.flags |= FLAGS_IGNORABLE;
                let oldstate = self.state.as_ref().map(copy_state);
                ret = self.validate_definition_list(d.content);
                if ret != 0 {
                    self.state = oldstate;
                    self.flags = oldflags;
                    ret = 0;
                    if self.err_nr > err_nr {
                        self.pop_errors(err_nr);
                    }
                } else {
                    if let Some(sts) = self.states.as_mut() {
                        if let Some(os) = oldstate {
                            add_states(sts, os);
                        }
                    } else {
                        let mut sts = Vec::new();
                        if let Some(os) = oldstate {
                            add_states(&mut sts, os);
                        }
                        if let Some(s) = self.state.take() {
                            add_states(&mut sts, s);
                        }
                        self.states = Some(sts);
                    }
                    self.flags = oldflags;
                    ret = 0;
                    if self.err_nr > err_nr {
                        self.pop_errors(err_nr);
                    }
                }
            }
            T::OneOrMore | T::ZeroOrMore => {
                let mut skip = false;
                if d.typ == T::OneOrMore {
                    let err_nr = self.err_nr;
                    ret = self.validate_definition_list(d.content);
                    if ret != 0 {
                        skip = true;
                    } else if self.err_nr > err_nr {
                        self.pop_errors(err_nr);
                    }
                }
                if !skip {
                    ret = self.validate_zero_or_more(def);
                }
            }
            T::Choice => {
                node = self.skip_ignored(node);
                let err_nr = self.err_nr;
                if let (true, Some(triage), Some(n)) =
                    (d.dflags & IS_TRIABLE != 0, d.triage.as_ref(), node)
                {
                    match Self::triage_lookup(triage, n) {
                        None => {
                            ret = -1;
                            let nm = nname(n);
                            self.verr2(ERR_ELEMWRONG, Some(&nm));
                        }
                        Some(l) => {
                            ret = self.validate_definition(Some(l));
                        }
                    }
                } else {
                    let oldflags = self.flags;
                    self.flags |= FLAGS_IGNORABLE;
                    let mut states: Option<Vec<SRef>> = None;
                    let mut oldstate: Option<SRef> = None;
                    let mut list = d.content;
                    while let Some(l) = list {
                        oldstate = self.state.as_ref().map(copy_state);
                        ret = self.validate_definition(Some(l));
                        if ret == 0 {
                            let sts = states.get_or_insert_with(Vec::new);
                            if let Some(s) = self.state.take() {
                                add_states(sts, s);
                            } else if let Some(cs) = self.states.take() {
                                for s in cs {
                                    add_states(sts, s);
                                }
                            }
                        } else {
                            self.state = None;
                        }
                        self.state = oldstate.clone();
                        list = schema.defs[l].next;
                    }
                    let _ = oldstate;
                    if let Some(sts) = states {
                        self.states = Some(sts);
                        self.state = None;
                        ret = 0;
                    } else {
                        self.states = None;
                    }
                    self.flags = oldflags;
                    if ret != 0 {
                        self.dump_if_not_ignorable();
                    } else if self.err_nr > err_nr {
                        self.pop_errors(err_nr);
                    }
                }
            }
            T::Def | T::Group => ret = self.validate_definition_list(d.content),
            T::Interleave => ret = self.validate_interleave(def),
            T::Attribute => ret = self.validate_attribute(def),
            T::Start | T::Noop | T::Ref | T::ExternalRef | T::ParentRef => {
                ret = self.validate_definition(d.content)
            }
            T::Datatype => match self.collect_text(node, ERR_DATAELEM) {
                None => ret = -1,
                Some(content) => {
                    let seq = self.seq();
                    ret = self.validate_datatype(Some(&content), def, seq);
                    if ret == -1 {
                        self.verr2(ERR_DATATYPE, d.name.as_deref());
                    } else if ret == 0 {
                        self.set_seq(None);
                    }
                }
            },
            T::Value => match self.collect_text(node, ERR_VALELEM) {
                None => ret = -1,
                Some(content) => {
                    let oldvalue = self.state_value();
                    self.set_state_value(Some(VPtr::new(content)));
                    ret = self.validate_value(Some(def));
                    self.set_state_value(oldvalue);
                    if ret == -1 {
                        self.verr2(ERR_VALUE, d.name.as_deref());
                    } else if ret == 0 {
                        self.set_seq(None);
                    }
                }
            },
            T::List => match self.collect_text(node, ERR_LISTELEM) {
                None => ret = -1,
                Some(content) => {
                    let len = content.len();
                    let p = VPtr::new(content);
                    let endp = VPtr {
                        buf: p.buf.clone(),
                        off: len,
                    };
                    let (oldvalue, oldend) = match &self.state {
                        Some(s) => {
                            let s = s.borrow();
                            (s.value.clone(), s.endvalue.clone())
                        }
                        None => (None, None),
                    };
                    if let Some(s) = &self.state {
                        let mut s = s.borrow_mut();
                        s.value = Some(p);
                        s.endvalue = Some(endp);
                    }
                    ret = self.validate_value(Some(def));
                    if let Some(s) = &self.state {
                        let mut s = s.borrow_mut();
                        s.value = oldvalue;
                        s.endvalue = oldend;
                    }
                    if ret == -1 {
                        self.verr(ERR_LIST);
                    } else if ret == 0 {
                        if let Some(n) = node {
                            self.set_seq(nnext(n));
                        }
                    }
                }
            },
            T::Except | T::Param => ret = -1,
        }
        self.depth -= 1;
        ret
    }

    /// The XML_RELAXNG_ELEMENT case of xmlRelaxNGValidateState
    fn validate_element(&mut self, def: DefId, node: Option<NodeId>) -> i32 {
        let schema = self.schema.clone();
        let d = &schema.defs[def];
        let mut err_nr = self.err_nr;
        let node = self.skip_ignored(node);
        let n = match node {
            None => {
                self.verr2(ERR_NOELEM, d.name.as_deref());
                self.dump_if_not_ignorable();
                return -1;
            }
            Some(n) => n,
        };
        if ntype(n) != NType::Element {
            self.verr(ERR_NOTELEM);
            self.dump_if_not_ignorable();
            return -1;
        }
        if self.psvi.get(&n) == Some(&def) {
            let s = self.skip_ignored(nnext(n));
            self.set_seq(s);
            if self.err_nr > err_nr {
                self.pop_errors(err_nr);
            }
            if self.err_nr != 0 {
                self.pop_matching_errors(n);
            }
            return 0;
        }
        let mut ret = self.element_match(def, n);
        if ret <= 0 {
            self.dump_if_not_ignorable();
            return -1;
        }
        ret = 0;
        if self.err_nr != 0 {
            if self.err_nr > err_nr {
                self.pop_errors(err_nr);
            }
            self.pop_matching_errors(n);
        }
        err_nr = self.err_nr;
        let oldflags = self.flags;
        if self.flags & FLAGS_MIXED_CONTENT != 0 {
            self.flags -= FLAGS_MIXED_CONTENT;
        }
        let state = match self.new_valid_state(Some(n)) {
            None => {
                self.dump_if_not_ignorable();
                return -1;
            }
            Some(s) => s,
        };
        let oldstate = self.state.replace(state);
        let nname_s = nname(n);
        if let Some(a) = d.attrs {
            if self.validate_attribute_list(Some(a)) != 0 {
                ret = -1;
                self.verr2(ERR_ATTRVALID, Some(&nname_s));
            }
        }
        if let Some(cm) = &d.cont_model {
            let tmpstate = self.state.take();
            let tmpstates = self.states.take();
            let nstate = self.new_valid_state(Some(n));
            self.state = nstate;
            let seq0 = self.seq();
            let tmp = self.validate_compiled_content(cm.clone(), seq0);
            let nseq = self.seq();
            self.state = tmpstate;
            self.states = tmpstates;
            if tmp != 0 {
                ret = -1;
            }
            if self.states.is_some() {
                self.validate_element_states(Some(nseq), oldflags, &mut ret);
            } else {
                self.set_seq(nseq);
                if ret == 0 {
                    ret = self.validate_element_end(true);
                }
            }
        } else {
            if let Some(c) = d.content {
                let tmp = self.validate_definition_list(Some(c));
                if tmp != 0 {
                    ret = -1;
                    if self.state.is_none() {
                        self.state = oldstate.clone();
                        self.verr2(ERR_CONTENTVALID, Some(&nname_s));
                        self.state = None;
                    } else {
                        self.verr2(ERR_CONTENTVALID, Some(&nname_s));
                    }
                }
            }
            if self.states.is_some() {
                self.validate_element_states(None, oldflags, &mut ret);
            } else if ret == 0 {
                ret = self.validate_element_end(true);
            }
        }
        if ret == 0 {
            self.psvi.insert(n, def);
        }
        self.flags = oldflags;
        self.state = oldstate;
        if self.state.is_some() {
            let s = self.skip_ignored(nnext(n));
            self.set_seq(s);
        }
        if ret != 0 {
            if self.flags & FLAGS_IGNORABLE == 0 {
                self.dump_valid_error();
                ret = 0;
            }
        } else if self.err_nr > err_nr {
            self.pop_errors(err_nr);
        }
        ret
    }

    /// The XML_RELAXNG_ZEROORMORE case of xmlRelaxNGValidateState
    fn validate_zero_or_more(&mut self, def: DefId) -> i32 {
        let content = self.defs()[def].content;
        let mut res: Vec<SRef> = Vec::new();
        if let Some(s) = &self.state {
            add_states(&mut res, copy_state(s));
        } else if let Some(sts) = &self.states {
            for s in sts {
                add_states(&mut res, copy_state(s));
            }
        }
        let oldflags = self.flags;
        self.flags |= FLAGS_IGNORABLE;
        let mut states: Option<Vec<SRef>> = None;
        loop {
            let mut progress = false;
            let mut base = res.len();
            if self.states.is_some() {
                states = self.states.take();
                let list = states.as_ref().unwrap().clone();
                for s in list {
                    self.state = Some(s);
                    self.states = None;
                    let ret = self.validate_definition_list(content);
                    if ret == 0 {
                        if let Some(st) = self.state.take() {
                            if add_states(&mut res, st) == 1 {
                                progress = true;
                            }
                        } else if let Some(sts) = self.states.take() {
                            for s in sts {
                                if add_states(&mut res, s) == 1 {
                                    progress = true;
                                }
                            }
                        }
                    } else {
                        self.state = None;
                    }
                }
            } else {
                let ret = self.validate_definition_list(content);
                if ret != 0 {
                    self.state = None;
                } else {
                    base = res.len();
                    if let Some(st) = self.state.take() {
                        if add_states(&mut res, st) == 1 {
                            progress = true;
                        }
                    } else if let Some(sts) = self.states.take() {
                        for s in &sts {
                            if add_states(&mut res, s.clone()) == 1 {
                                progress = true;
                            }
                        }
                        if states.is_none() {
                            states = Some(sts);
                        }
                    }
                }
            }
            if progress {
                if res.len() - base == 1 {
                    self.state = Some(copy_state(&res[base]));
                } else {
                    if states.is_none() {
                        states = self.states.take();
                        if states.is_none() {
                            break;
                        }
                    }
                    let mut st = states.take().unwrap();
                    st.clear();
                    for r in &res[base..] {
                        add_states(&mut st, copy_state(r));
                    }
                    self.states = Some(st);
                }
            } else {
                break;
            }
        }
        self.states = Some(res);
        self.flags = oldflags;
        0
    }

    /// xmlRelaxNGValidateDefinition
    fn validate_definition(&mut self, define: Option<DefId>) -> i32 {
        if self.state.is_some() && self.states.is_some() {
            self.state = None;
        }
        if self.states.as_ref().map(|s| s.len() == 1).unwrap_or(true) {
            if let Some(sts) = self.states.take() {
                self.state = Some(sts[0].clone());
            }
            let ret = self.validate_state(define);
            if self.state.is_some() && self.states.is_some() {
                self.state = None;
            }
            if self.states.as_ref().map(|s| s.len()) == Some(1) {
                let sts = self.states.take().unwrap();
                self.state = Some(sts[0].clone());
            }
            return ret;
        }
        let mut states = self.states.take().unwrap();
        let mut res: Option<Vec<SRef>> = None;
        let mut j = 0;
        let oldflags = self.flags;
        self.flags |= FLAGS_IGNORABLE;
        for i in 0..states.len() {
            self.state = Some(states[i].clone());
            self.states = None;
            let ret = self.validate_state(define);
            if self.state.is_some() && self.states.is_some() {
                self.state = None;
            }
            if ret == 0 {
                if self.states.is_none() {
                    if let Some(r) = res.as_mut() {
                        if let Some(s) = self.state.take() {
                            add_states(r, s);
                        }
                    } else if let Some(s) = self.state.take() {
                        states[j] = s;
                        j += 1;
                    }
                } else if res.is_none() {
                    let mut r = self.states.take().unwrap();
                    for s in states.iter().take(j) {
                        add_states(&mut r, s.clone());
                    }
                    res = Some(r);
                } else {
                    let cs = self.states.take().unwrap();
                    let r = res.as_mut().unwrap();
                    for s in cs {
                        add_states(r, s);
                    }
                }
            } else {
                self.state = None;
                self.states = None;
            }
        }
        self.flags = oldflags;
        let ret;
        if let Some(r) = res {
            self.states = Some(r);
            ret = 0;
        } else if j > 1 {
            states.truncate(j);
            self.states = Some(states);
            ret = 0;
        } else if j == 1 {
            self.state = Some(states[0].clone());
            ret = 0;
        } else {
            ret = -1;
            self.states = None;
        }
        if self.state.is_some() && self.states.is_some() {
            self.state = None;
        }
        ret
    }

    /// The IDREF checks of xmlValidateDocumentFinal.
    fn validate_document_final(&mut self, doc: NodeId) -> bool {
        let refs: Vec<(NodeId, NodeId, u8, String, String)> = tree::with(|t| {
            let mut v = Vec::new();
            for n in t.descendants(doc) {
                if t.nodes[n].t != NType::Element {
                    continue;
                }
                for a in t.attrs_of(n) {
                    let at = t.nodes[a].atype;
                    if at == 3 || at == 4 {
                        v.push((a, n, at, t.nodes[a].name.clone(), t.node_get_content(a)));
                    }
                }
            }
            v
        });
        let mut valid = true;
        for (_a, el, at, aname, value) in refs {
            let value = norm_ext_space(&value);
            let ids: Vec<String> = if at == 3 {
                vec![value]
            } else {
                value
                    .split([' ', '\t', '\n', '\r'])
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect()
            };
            for id in ids {
                let known = tree::with(|t| {
                    t.doc_info(doc)
                        .map(|d| d.ids.contains_key(&id))
                        .unwrap_or(false)
                });
                if !known {
                    let kind = if at == 3 { "IDREF" } else { "IDREFS" };
                    let msg = format!(
                        "{} attribute {} references an unknown ID \"{}\"\n",
                        kind, aname, id
                    );
                    self.errors
                        .push(node_error(FROM_VALID, 536, ERR_ERROR, Some(el), msg));
                    valid = false;
                }
            }
        }
        valid
    }

    /// xmlRelaxNGValidateDocument
    fn validate_document(&mut self, doc: NodeId) -> i32 {
        self.err_no = ERR_OK;
        let start = self.schema.start;
        let state = self.new_valid_state(None);
        self.state = state.clone();
        let mut ret = self.validate_definition(start);
        let orig_seq = state.as_ref().and_then(|s| s.borrow().seq);
        if self.state.is_some() && orig_seq.is_some() {
            let seq = self.seq();
            if self.skip_ignored(seq).is_some() && ret != -1 {
                self.verr(ERR_EXTRADATA);
                ret = -1;
            }
        } else if let Some(sts) = self.states.clone() {
            let mut tmp = -1;
            for s in &sts {
                let seq = s.borrow().seq;
                if self.skip_ignored(seq).is_none() {
                    tmp = 0;
                }
            }
            if tmp == -1 && ret != -1 {
                self.verr(ERR_EXTRADATA);
                ret = -1;
            }
        }
        self.state = None;
        if ret != 0 {
            self.dump_valid_error();
        }
        if self.idref && !self.validate_document_final(doc) {
            ret = -1;
        }
        if ret == 0 && self.err_no != ERR_OK {
            ret = -1;
        }
        ret
    }

    /// xmlRelaxNGValidateDoc: 0 if valid, a positive number if invalid.
    pub fn validate_doc(&mut self, doc: NodeId) -> i32 {
        self.doc = Some(doc);
        let ret = self.validate_document(doc);
        self.psvi.clear();
        if ret == -1 { 1 } else { ret }
    }
}

/// Validate a document against a grammar (xmlRelaxNGValidateDoc): returns
/// 0 if valid, > 0 if invalid, < 0 on internal error, with the errors in
/// the order libxml2 raises them.
pub fn validate_doc(schema: &Rc<RelaxNg>, doc: NodeId) -> (i32, Vec<XmlError>) {
    let mut ctxt = ValidCtxt::new(schema.clone());
    let r = ctxt.validate_doc(doc);
    (r, ctxt.take_errors())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk a document the way xmlreader drives the progressive interface.
    fn stream(ctxt: &mut ValidCtxt, n: NodeId, errs: &mut i32) {
        match ntype(n) {
            NType::Element => {
                let r = ctxt.push_element(n);
                if r == 0 {
                    if ctxt.full_element(n) != 1 {
                        *errs += 1;
                    }
                    return;
                }
                if r != 1 {
                    *errs += 1;
                }
                let mut c = nchildren(n);
                while let Some(x) = c {
                    stream(ctxt, x, errs);
                    c = nnext(x);
                }
                if ctxt.pop_element(n) != 1 {
                    *errs += 1;
                }
            }
            NType::Text | NType::CData => {
                if ctxt.push_cdata(&ncontent(n)) != 1 {
                    *errs += 1;
                }
            }
            _ => {}
        }
    }

    fn check(rng: &str, xml: &str) -> (i32, i32, Vec<String>, Vec<String>) {
        let mut vm = Vm::new();
        let (schema, perrs) = parse_memory_collect(&mut vm, rng.as_bytes());
        assert!(
            perrs.is_empty(),
            "{:?}",
            perrs.iter().map(|e| e.message.clone()).collect::<Vec<_>>()
        );
        let schema = schema.unwrap();
        let doc = crate::sax2::parse_doc(xml.as_bytes(), None, 0).doc.unwrap();
        let (r, e1) = validate_doc(&schema, doc);
        let mut ctxt = ValidCtxt::new(schema);
        let mut errs = 0;
        stream(&mut ctxt, root_element(doc).unwrap(), &mut errs);
        let e2 = ctxt.take_errors();
        (
            r,
            errs,
            e1.into_iter().map(|e| e.message).collect(),
            e2.into_iter().map(|e| e.message).collect(),
        )
    }

    const R: &str = "xmlns='http://relaxng.org/ns/structure/1.0'";

    #[test]
    fn streaming() {
        let rng = format!(
            "<element name='a' {}><element name='b'><text/></element><zeroOrMore><element name='c'><attribute name='x'/><empty/></element></zeroOrMore></element>",
            R
        );
        let (r, errs, _, e2) = check(&rng, "<a><b>t</b><c x='1'/><c x='2'/></a>");
        assert_eq!((r, errs), (0, 0), "{:?}", e2);
        let (r, errs, e1, e2) = check(&rng, "<a><b>t</b><c/><d/></a>");
        assert_eq!(r, 1);
        assert!(errs > 0);
        assert_eq!(
            e1,
            vec![
                "Element c failed to validate attributes\n".to_string(),
                "Did not expect element d there\n".to_string()
            ]
        );
        assert_eq!(
            e2,
            vec![
                "Element c failed to validate attributes\n".to_string(),
                "Did not expect element d there\n".to_string()
            ]
        );
        // non-compilable content (interleave) is validated as a full element
        let rng = format!(
            "<element name='a' {}><interleave><element name='b'><empty/></element><element name='c'><empty/></element></interleave></element>",
            R
        );
        let (r, errs, _, e2) = check(&rng, "<a><c/><b/></a>");
        assert_eq!((r, errs), (0, 0), "{:?}", e2);
        let (r, errs, _, e2) = check(&rng, "<a><c/></a>");
        assert_eq!(
            e2,
            vec![
                "Expecting an element b, got nothing\n".to_string(),
                "Invalid sequence in interleave\n".to_string(),
                "Element a failed to validate content\n".to_string()
            ]
        );
        assert_eq!(r, 1);
        assert!(errs > 0);
        // text where none is allowed
        let rng = format!(
            "<element name='a' {}><element name='b'><empty/></element></element>",
            R
        );
        let (_, errs, _, e2) = check(&rng, "<a>x<b/></a>");
        assert!(errs > 0);
        assert_eq!(
            e2,
            vec![
                "Did not expect text in element  TODO  content\n".to_string(),
                "Did not expect element b there\n".to_string()
            ]
        );
    }

    #[test]
    fn norm_ext_space_cases() {
        assert_eq!(norm_ext_space("abc"), "abc");
        assert_eq!(norm_ext_space("abc  "), "abc");
        assert_eq!(norm_ext_space("a b "), "a b");
        assert_eq!(norm_ext_space("  abc "), "abc");
        assert_eq!(norm_ext_space("  a b"), "ab");
    }
}
