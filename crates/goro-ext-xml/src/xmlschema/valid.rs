//! Instance validation (xmlSchemaValidCtxt): element/attribute assessment,
//! content models, simple content, default attributes. Works on the DOM
//! tree (`validate_doc`, `validate_one_element`) or as a stream of events
//! (`start_element` / `characters` / `end_element`, for XMLReader).
//!
//! Identity constraints (xs:key/xs:unique/xs:keyref) are not evaluated.

use super::fixup::is_particle_emptiable;
use super::simple::{
    ErrCtx, NodeDesc, NsScope, VList, check_cvc_simple_type, is_derived_from_builtin,
    normalize_value,
};
use super::*;

// attribute info states (XML_SCHEMAS_ATTR_*)
const ATTR_UNKNOWN: i32 = 1;
const ATTR_ASSESSED: i32 = 2;
const ATTR_ERR_MISSING: i32 = 4;
const ATTR_INVALID_VALUE: i32 = 5;
const ATTR_ERR_NO_TYPE: i32 = 6;
const ATTR_ERR_FIXED_VALUE: i32 = 7;
const ATTR_DEFAULT: i32 = 8;
const ATTR_WILD_SKIP: i32 = 10;
const ATTR_WILD_LAX_NO_DECL: i32 = 11;
const ATTR_ERR_WILD_STRICT_NO_DECL: i32 = 12;
const ATTR_ERR_WILD_DUPLICATE_ID: i32 = 13;
const ATTR_ERR_WILD_AND_USE_ID: i32 = 14;
const ATTR_META: i32 = 15;

// meta attribute types (XML_SCHEMA_ATTR_INFO_META_*)
const META_XSI_TYPE: i32 = 1;
const META_XSI_NIL: i32 = 2;
const META_XSI_SCHEMA_LOC: i32 = 3;
const META_XSI_NO_NS_SCHEMA_LOC: i32 = 4;
const META_XMLNS: i32 = 5;

// node info flags
const ELEM_INFO_NILLED: i32 = 1 << 2;
const ELEM_INFO_LOCAL_TYPE: i32 = 1 << 3;
const NODE_INFO_VALUE_NEEDED: i32 = 1 << 4;
const ELEM_INFO_EMPTY: i32 = 1 << 5;
const ELEM_INFO_HAS_ELEM_CONTENT: i32 = 1 << 7;
const ELEM_INFO_ERR_BAD_CONTENT: i32 = 1 << 8;
const NODE_INFO_ERR_NOT_EXPECTED: i32 = 1 << 9;
const NODE_INFO_ERR_BAD_TYPE: i32 = 1 << 10;

/// Text node kinds for `characters`.
pub const TEXT_NODE: i32 = 3;
pub const CDATA_NODE: i32 = 4;

#[derive(Default)]
struct AttrInfo {
    node: Option<NodeId>,
    line: i32,
    local: String,
    ns: Option<String>,
    value: String,
    meta: i32,
    state: i32,
    flags: i32,
    use_: Option<ItemId>,
    decl: Option<ItemId>,
    type_def: Option<ItemId>,
    val: Option<VList>,
    vc_value: Option<String>,
}

#[derive(Default)]
struct ElemInfo {
    node: Option<NodeId>,
    line: i32,
    local: String,
    ns: Option<String>,
    flags: i32,
    decl: Option<ItemId>,
    type_def: Option<ItemId>,
    value: Option<String>,
    val: Option<VList>,
    regex: Option<ExecCtxt>,
    ns_bindings: Vec<(Option<String>, String)>,
}

/// An attribute pushed by a streaming producer.
pub struct StreamAttr {
    pub local: String,
    pub ns: Option<String>,
    pub value: String,
    /// the attribute node, if any (for IDs and error locations)
    pub node: Option<NodeId>,
}

/// Which info item is "active" for error reports (vctxt->inode).
#[derive(Clone, Copy, PartialEq)]
enum Active {
    None,
    Elem(usize),
    Attr(usize),
}

pub struct ValidCtxt {
    pub schema: Rc<Schema>,
    pub options: i32,
    /// file name used for error reports of streamed (tree-less) validation
    pub filename: Option<String>,
    pub ec: ErrCtx,
    elems: Vec<ElemInfo>,
    depth: i32,
    attrs: Vec<AttrInfo>,
    skip_depth: i32,
    validation_root: Option<NodeId>,
    active: Active,
    tree_mode: bool,
}

impl ValidCtxt {
    /// xmlSchemaNewValidCtxt
    pub fn new(schema: Rc<Schema>) -> ValidCtxt {
        ValidCtxt {
            schema,
            options: 0,
            filename: None,
            ec: ErrCtx {
                validator: true,
                ..Default::default()
            },
            elems: Vec::new(),
            depth: -1,
            attrs: Vec::new(),
            skip_depth: -1,
            validation_root: None,
            active: Active::None,
            tree_mode: false,
        }
    }

    /// xmlSchemaSetValidOptions
    pub fn set_options(&mut self, options: i32) {
        self.options = options;
    }

    /// Collected errors and warnings (in order).
    pub fn take_errors(&mut self) -> Vec<XmlError> {
        std::mem::take(&mut self.ec.errors)
    }

    pub fn errors(&self) -> &[XmlError] {
        &self.ec.errors
    }

    /// Number of validity errors so far (xmlSchemaIsValid is nberrors == 0).
    pub fn nb_errors(&self) -> i32 {
        self.ec.nberrors
    }

    /// Last error code (vctxt->err), -1 on internal error.
    pub fn err(&self) -> i32 {
        self.ec.err
    }

    // -----------------------------------------------------------------
    // error context handling

    fn ns_scope(&self) -> NsScope {
        if self.tree_mode {
            if self.depth >= 0 {
                if let Some(n) = self.elems[self.depth as usize].node {
                    return NsScope::Node(n);
                }
            }
            NsScope::None
        } else {
            let mut b = Vec::new();
            let d = if self.depth < 0 {
                0
            } else {
                self.depth as usize + 1
            };
            for e in self.elems.iter().take(d.min(self.elems.len())) {
                b.extend(e.ns_bindings.iter().cloned());
            }
            NsScope::Bindings(b)
        }
    }

    fn set_active(&mut self, a: Active) {
        self.active = a;
        self.ec.inode = match a {
            Active::None => None,
            Active::Elem(i) => {
                let e = &self.elems[i];
                Some(NodeDesc {
                    is_attr: false,
                    local: e.local.clone(),
                    ns: e.ns.clone(),
                    elem_local: String::new(),
                    elem_ns: None,
                    node: e.node,
                    line: e.line,
                })
            }
            Active::Attr(i) => {
                let at = &self.attrs[i];
                let (el, ens) = if self.depth >= 0 {
                    let e = &self.elems[self.depth as usize];
                    (e.local.clone(), e.ns.clone())
                } else {
                    (String::new(), None)
                };
                Some(NodeDesc {
                    is_attr: true,
                    local: at.local.clone(),
                    ns: at.ns.clone(),
                    elem_local: el,
                    elem_ns: ens,
                    node: at.node,
                    line: at.line,
                })
            }
        };
        self.ec.ns_scope = self.ns_scope();
    }

    fn activate_elem(&mut self) {
        if self.depth >= 0 {
            self.set_active(Active::Elem(self.depth as usize));
        } else {
            self.set_active(Active::None);
        }
    }

    fn activate_parent(&mut self) {
        self.set_active(Active::Elem(self.depth as usize - 1));
    }

    fn inode(&mut self) -> &mut ElemInfo {
        let d = self.depth as usize;
        &mut self.elems[d]
    }

    fn ino(&self) -> &ElemInfo {
        &self.elems[self.depth as usize]
    }

    /// VERROR
    fn verror(&mut self, code: i32, msg: &str) {
        let s = self.schema.clone();
        self.ec.custom_err(&s, code, None, None, msg);
    }

    fn verror_int(&mut self, func: &str, msg: &str) {
        self.ec.internal_err(func, msg);
    }

    /// xmlSchemaComplexTypeErr
    fn complex_type_err(
        &mut self,
        code: i32,
        message: &str,
        info: Option<(Vec<String>, usize, usize, bool)>,
    ) {
        let mut msg = self.ec.format_node(None);
        msg.push_str(message);
        msg.push('.');
        let (values, nbval, nbneg) = match info {
            Some((v, a, b, _)) => (v, a, b),
            None => (Vec::new(), 0, 0),
        };
        let total = nbval + nbneg;
        if total > 0 {
            let mut s = String::from(if total > 1 {
                " Expected is one of ( "
            } else {
                " Expected is ( "
            });
            for i in 0..total {
                let mut cur: &str = match values.get(i) {
                    Some(v) => v,
                    None => continue,
                };
                if let Some(r) = cur.strip_prefix("not ") {
                    cur = r;
                    s.push_str("##other");
                }
                let (local, rest): (&str, &str) = if let Some(r) = cur.strip_prefix('*') {
                    ("*", r)
                } else {
                    match cur.find('|') {
                        Some(p) => (&cur[..p], &cur[p..]),
                        None => (cur, ""),
                    }
                };
                if !rest.is_empty() {
                    let after = &rest[1..];
                    if nbneg == 0 || !after.starts_with('*') || !local.starts_with('*') {
                        if after.starts_with('*') {
                            s.push_str("{*}");
                        } else {
                            s.push_str(if i >= nbval { "{##other:" } else { "{" });
                            s.push_str(after);
                            s.push('}');
                        }
                    } else {
                        continue;
                    }
                }
                s.push_str(local);
                if i + 1 < total {
                    s.push_str(", ");
                }
            }
            s.push_str(" ).\n");
            msg.push_str(&s);
        } else {
            msg.push('\n');
        }
        self.ec.err(code, None, msg);
    }

    /// xmlSchemaIllegalAttrErr (on the active attribute info)
    fn illegal_attr_err(&mut self, code: i32, i: usize) {
        let mut msg = self.ec.format_node(None);
        let a = &self.attrs[i];
        msg.push_str(&format!(
            "The attribute '{}' is not allowed.\n",
            format_qname(a.ns.as_deref(), Some(&a.local))
        ));
        self.ec.err(code, None, msg);
    }

    /// xmlSchemaVCheckCVCSimpleType with fireErrors=1
    fn check_simple(
        &mut self,
        node: Option<NodeId>,
        typ: ItemId,
        value: &str,
        want_val: bool,
        normalize: bool,
    ) -> (i32, Option<VList>) {
        let s = self.schema.clone();
        check_cvc_simple_type(
            &s,
            &mut self.ec,
            node,
            typ,
            value,
            want_val,
            true,
            normalize,
            false,
        )
    }

    // -----------------------------------------------------------------
    // public entry points

    fn pre_run(&mut self) {
        self.ec.err = 0;
        self.ec.nberrors = 0;
        self.depth = -1;
        self.skip_depth = -1;
        self.elems.clear();
        self.attrs.clear();
        self.active = Active::None;
        self.ec.inode = None;
    }

    /// xmlSchemaValidateDoc: returns 0 if valid, a positive error code,
    /// or -1 on internal error.
    pub fn validate_doc(&mut self, doc: NodeId) -> i32 {
        let root = tree::with(|t| t.root_element(doc));
        match root {
            None => {
                self.pre_run();
                let s = self.schema.clone();
                self.ec.custom_err(
                    &s,
                    E::SCHEMAV_DOCUMENT_ELEMENT_MISSING,
                    Some(doc),
                    None,
                    "The document has no document element",
                );
                self.ec.err
            }
            Some(r) => self.validate_one_element(r),
        }
    }

    /// xmlSchemaValidateOneElement
    pub fn validate_one_element(&mut self, elem: NodeId) -> i32 {
        if node_type(elem) != NType::Element {
            return -1;
        }
        self.validation_root = Some(elem);
        self.tree_mode = true;
        self.pre_run();
        let ret = self.doc_walk();
        self.post_run();
        if ret == 0 { self.ec.err } else { ret }
    }

    fn post_run(&mut self) {
        self.elems.clear();
        self.attrs.clear();
        self.depth = -1;
        self.active = Active::None;
        self.ec.inode = None;
    }

    // -----------------------------------------------------------------
    // streaming interface

    /// Start streamed validation (call before the first `start_element`).
    pub fn stream_start(&mut self) {
        self.tree_mode = false;
        self.validation_root = None;
        self.pre_run();
    }

    /// An element start tag. `node` is the instance node when there is a
    /// tree (used for error locations and default attributes).
    /// Returns -1 on internal error.
    pub fn start_element(
        &mut self,
        local: &str,
        ns: Option<&str>,
        line: i32,
        node: Option<NodeId>,
        ns_bindings: Vec<(Option<String>, String)>,
        attrs: Vec<StreamAttr>,
    ) -> i32 {
        if self.skip_depth != -1 && self.depth >= self.skip_depth {
            self.depth += 1;
            self.push_elem();
            let e = self.inode();
            e.local = local.to_string();
            e.ns = ns.map(|s| s.to_string());
            e.node = node;
            e.line = line;
            return 0;
        }
        if let Some(p) = self.parent_info_mut() {
            if p.flags & ELEM_INFO_EMPTY != 0 {
                p.flags ^= ELEM_INFO_EMPTY;
            }
        }
        self.depth += 1;
        self.push_elem();
        {
            let e = self.inode();
            e.local = local.to_string();
            e.ns = ns.map(|s| s.to_string());
            e.node = node;
            e.line = line;
            e.flags |= ELEM_INFO_EMPTY;
            e.ns_bindings = ns_bindings;
        }
        self.activate_elem();
        for a in attrs {
            self.push_attribute(a.node, line, a.local, a.ns, a.value);
        }
        let r = self.validate_elem();
        if r == -1 {
            return -1;
        }
        0
    }

    fn parent_info_mut(&mut self) -> Option<&mut ElemInfo> {
        if self.depth >= 0 {
            let d = self.depth as usize;
            Some(&mut self.elems[d])
        } else {
            None
        }
    }

    /// Character data of the current element.
    pub fn characters(&mut self, node_type: i32, text: &str) -> i32 {
        if self.depth < 0 || (self.skip_depth != -1 && self.depth >= self.skip_depth) {
            return 0;
        }
        let e = self.inode();
        if e.flags & ELEM_INFO_EMPTY != 0 {
            e.flags ^= ELEM_INFO_EMPTY;
        }
        self.activate_elem();
        let r = self.push_text(node_type, text);
        if r < 0 {
            return -1;
        }
        0
    }

    /// The end tag of the current element.
    pub fn end_element(&mut self) -> i32 {
        if self.depth < 0 {
            return 0;
        }
        if self.skip_depth != -1 && self.depth > self.skip_depth {
            let d = self.depth as usize;
            self.elems[d] = ElemInfo::default();
            self.depth -= 1;
            return 0;
        }
        self.activate_elem();
        let r = self.pop_elem();
        if r < 0 {
            return -1;
        }
        0
    }

    /// Finish streamed validation: 0 if valid, else the last error code.
    pub fn stream_end(&mut self) -> i32 {
        self.post_run();
        self.ec.err
    }

    // -----------------------------------------------------------------
    // the walk

    fn push_elem(&mut self) {
        let d = self.depth as usize;
        if self.elems.len() <= d {
            self.elems.resize_with(d + 1, ElemInfo::default);
        }
        self.elems[d] = ElemInfo::default();
        self.attrs.clear();
    }

    /// xmlSchemaValidatorPushAttribute
    fn push_attribute(
        &mut self,
        node: Option<NodeId>,
        line: i32,
        local: String,
        ns: Option<String>,
        value: String,
    ) {
        let mut a = AttrInfo {
            node,
            line,
            state: ATTR_UNKNOWN,
            ..Default::default()
        };
        if let Some(n) = &ns {
            if local == "nil" {
                if n == XSI_NS {
                    a.meta = META_XSI_NIL;
                }
            } else if local == "type" {
                if n == XSI_NS {
                    a.meta = META_XSI_TYPE;
                }
            } else if local == "schemaLocation" {
                if n == XSI_NS {
                    a.meta = META_XSI_SCHEMA_LOC;
                }
            } else if local == "noNamespaceSchemaLocation" {
                if n == XSI_NS {
                    a.meta = META_XSI_NO_NS_SCHEMA_LOC;
                }
            } else if n == XMLNS_NS {
                a.meta = META_XMLNS;
            }
        }
        a.local = local;
        a.ns = ns;
        a.value = value;
        if a.meta != 0 {
            a.state = ATTR_META;
        }
        self.attrs.push(a);
    }

    /// xmlSchemaVDocWalk
    fn doc_walk(&mut self) -> i32 {
        let val_root = match self.validation_root {
            Some(r) => r,
            None => {
                self.verror(1, "The document has no document element");
                return 1;
            }
        };
        self.depth = -1;
        let mut node = Some(val_root);
        let mut ret = 0;
        let mut ielem: Option<usize> = None;
        'walk: while let Some(n) = node {
            let mut leave = false;
            let mut skip_to_next = false;
            if self.skip_depth != -1 && self.depth >= self.skip_depth {
                skip_to_next = true;
            } else {
                let (nt, line, name, ns, content) = tree::with(|t| {
                    let nd = &t.nodes[n];
                    (
                        nd.t,
                        nd.line as i32,
                        nd.name.clone(),
                        t.ns_href(nd.ns).map(|s| s.to_string()),
                        if matches!(nd.t, NType::Text | NType::CData) {
                            Some(nd.content.clone())
                        } else {
                            None
                        },
                    )
                });
                match nt {
                    NType::Element => {
                        self.depth += 1;
                        self.push_elem();
                        ielem = Some(self.depth as usize);
                        {
                            let e = self.inode();
                            e.node = Some(n);
                            e.line = line;
                            e.local = name;
                            e.ns = ns;
                            e.flags |= ELEM_INFO_EMPTY;
                        }
                        self.activate_elem();
                        let attrs: Vec<(NodeId, String, Option<String>, String)> =
                            tree::with(|t| {
                                t.attrs_of(n)
                                    .into_iter()
                                    .map(|a| {
                                        let an = &t.nodes[a];
                                        (
                                            a,
                                            an.name.clone(),
                                            t.ns_href(an.ns).map(|s| s.to_string()),
                                            t.list_get_string(an.children, true),
                                        )
                                    })
                                    .collect()
                            });
                        for (a, local, ans, v) in attrs {
                            self.push_attribute(Some(a), line, local, ans, v);
                        }
                        ret = self.validate_elem();
                        if ret != 0 {
                            if ret == -1 {
                                self.verror_int(
                                    "xmlSchemaDocWalk",
                                    "calling xmlSchemaValidateElem()",
                                );
                                return -1;
                            }
                            leave = true;
                        } else if self.skip_depth != -1 && self.depth >= self.skip_depth {
                            leave = true;
                        }
                    }
                    NType::Text | NType::CData => {
                        if let Some(i) = ielem {
                            if self.elems[i].flags & ELEM_INFO_EMPTY != 0 {
                                self.elems[i].flags ^= ELEM_INFO_EMPTY;
                            }
                        }
                        let code = if nt == NType::Text {
                            TEXT_NODE
                        } else {
                            CDATA_NODE
                        };
                        self.activate_elem();
                        if self.push_text(code, content.as_deref().unwrap_or("")) < 0 {
                            self.verror_int("xmlSchemaVDocWalk", "calling xmlSchemaVPushText()");
                            return -1;
                        }
                    }
                    NType::Entity | NType::EntityRef => {
                        self.verror_int("xmlSchemaVDocWalk", "there is at least one entity reference in the node-tree currently being validated. Processing of entities with this XML Schema processor is not supported (yet). Please substitute entities before validation.");
                        return -1;
                    }
                    _ => leave = true,
                }
                if !leave {
                    let first = tree::with(|t| t.nodes[n].children);
                    if first.is_some() {
                        node = first;
                        continue 'walk;
                    }
                }
            }
            // leave_node / next_sibling
            let mut cur = n;
            let mut do_leave = !skip_to_next;
            loop {
                if do_leave && node_type(cur) == NType::Element {
                    if self.depth < 0 || self.elems[self.depth as usize].node != Some(cur) {
                        self.verror_int("xmlSchemaVDocWalk", "element position mismatch");
                        return -1;
                    }
                    self.activate_elem();
                    let r = self.pop_elem();
                    if r < 0 {
                        self.verror_int("xmlSchemaVDocWalk", "calling xmlSchemaValidatorPopElem()");
                        return -1;
                    }
                    ret = r;
                    if cur == val_root {
                        break 'walk;
                    }
                }
                do_leave = true;
                let (next, parent) = tree::with(|t| (t.nodes[cur].next, t.nodes[cur].parent));
                if let Some(nx) = next {
                    node = Some(nx);
                    continue 'walk;
                }
                match parent {
                    Some(p) => cur = p,
                    None => break 'walk,
                }
            }
        }
        ret
    }

    fn get_meta_attr(&self, meta: i32) -> Option<usize> {
        self.attrs.iter().position(|a| a.meta == meta)
    }

    /// xmlSchemaValidateElem
    fn validate_elem(&mut self) -> i32 {
        let s = self.schema.clone();
        let mut ret;
        macro_rules! exit {
            ($r:expr) => {{
                let r = $r;
                if r != 0 {
                    self.skip_depth = self.depth;
                }
                return r;
            }};
        }
        if self.skip_depth != -1 && self.depth >= self.skip_depth {
            self.verror_int("xmlSchemaValidateElem", "in skip-state");
            return -1;
        }
        if self.depth > 0 {
            ret = self.validate_child_elem();
            if ret != 0 {
                if ret < 0 {
                    self.verror_int(
                        "xmlSchemaValidateElem",
                        "calling xmlSchemaStreamValidateChildElement()",
                    );
                    return -1;
                }
                exit!(ret);
            }
            if self.depth == self.skip_depth {
                exit!(0);
            }
            if self.ino().decl.is_none() && self.ino().type_def.is_none() {
                self.verror_int(
                    "xmlSchemaValidateElem",
                    "the child element was valid but neither the declaration nor the type was set",
                );
                return -1;
            }
        } else {
            let (l, n) = (self.ino().local.clone(), self.ino().ns.clone());
            let d = s.get_global(TYPE_ELEMENT, &l, n.as_deref());
            self.inode().decl = d;
            if d.is_none() {
                self.verror(
                    E::SCHEMAV_CVC_ELT_1,
                    "No matching global declaration available for the validation root",
                );
                exit!(E::SCHEMAV_CVC_ELT_1);
            }
        }
        let mut type_validation = self.ino().decl.is_none();
        if !type_validation {
            let decl = self.ino().decl.unwrap();
            if s.it(decl).typ == TYPE_ANY {
                let mut skip = false;
                ret = self.validate_elem_wildcard(&mut skip);
                if ret != 0 {
                    if ret < 0 {
                        self.verror_int(
                            "xmlSchemaValidateElem",
                            "calling xmlSchemaValidateElemWildcard()",
                        );
                        return -1;
                    }
                    exit!(ret);
                }
                if skip {
                    self.skip_depth = self.depth;
                    exit!(0);
                }
                let d = self.ino().decl.unwrap();
                if s.it(d).typ != TYPE_ELEMENT {
                    self.inode().decl = None;
                    type_validation = true;
                }
            }
        }
        if !type_validation {
            ret = self.validate_elem_decl();
            if ret != 0 {
                if ret < 0 {
                    self.verror_int(
                        "xmlSchemaValidateElem",
                        "calling xmlSchemaValidateElemDecl()",
                    );
                    return -1;
                }
                exit!(ret);
            }
        }
        // type_validation:
        let td = match self.ino().type_def {
            None => {
                self.inode().flags |= NODE_INFO_ERR_BAD_TYPE;
                self.verror(E::SCHEMAV_CVC_TYPE_1, "The type definition is absent");
                exit!(E::SCHEMAV_CVC_TYPE_1);
            }
            Some(t) => t,
        };
        if s.it(td).flags & TYPE_ABSTRACT != 0 {
            self.inode().flags |= NODE_INFO_ERR_BAD_TYPE;
            self.verror(E::SCHEMAV_CVC_TYPE_2, "The type definition is abstract");
            exit!(E::SCHEMAV_CVC_TYPE_2);
        }
        ret = 0;
        if s.is_complex(td) {
            if !self.attrs.is_empty() || !s.it(td).attr_uses.is_empty() {
                ret = self.attributes_complex();
            }
        } else if !self.attrs.is_empty() {
            ret = self.attributes_simple();
        }
        self.attrs.clear();
        if ret == -1 {
            self.verror_int("xmlSchemaValidateElem", "calling attributes validation");
            return -1;
        }
        0
    }

    /// xmlSchemaValidateChildElem
    fn validate_child_elem(&mut self) -> i32 {
        let s = self.schema.clone();
        if self.depth <= 0 {
            self.verror_int(
                "xmlSchemaValidateChildElem",
                "not intended for the validation root",
            );
            return -1;
        }
        let pd = self.depth as usize - 1;
        if self.elems[pd].flags & ELEM_INFO_EMPTY != 0 {
            self.elems[pd].flags ^= ELEM_INFO_EMPTY;
        }
        let ret;
        let unexpected = |me: &mut ValidCtxt, ret: i32| -> i32 {
            me.skip_depth = me.depth;
            me.inode().flags |= NODE_INFO_ERR_NOT_EXPECTED;
            me.elems[pd].flags |= ELEM_INFO_ERR_BAD_CONTENT;
            ret
        };
        if self.elems[pd].flags & ELEM_INFO_NILLED != 0 {
            self.activate_parent();
            ret = E::SCHEMAV_CVC_ELT_3_2_1;
            self.verror(ret, "Neither character nor element content is allowed, because the element was 'nilled'");
            self.activate_elem();
            return unexpected(self, ret);
        }
        let ptype = match self.elems[pd].type_def {
            Some(t) => t,
            None => return 0,
        };
        if s.it(ptype).builtin_type == T::ANYTYPE {
            let (l, n) = (self.ino().local.clone(), self.ino().ns.clone());
            let d = s.get_global(TYPE_ELEMENT, &l, n.as_deref());
            self.inode().decl = d;
            if d.is_none() {
                if let Some(ia) = self.get_meta_attr(META_XSI_TYPE) {
                    let mut lt = None;
                    ret = self.process_xsi_type(ia, &mut lt, None);
                    self.inode().type_def = lt;
                    if ret != 0 {
                        if ret == -1 {
                            self.verror_int("xmlSchemaValidateChildElem", "calling xmlSchemaProcessXSIType() to process the attribute 'xsi:nil'");
                            return -1;
                        }
                        return ret;
                    }
                } else {
                    let at = s.builtin(T::ANYTYPE);
                    self.inode().type_def = Some(at);
                }
            }
            return 0;
        }
        match s.it(ptype).content_type {
            CONTENT_EMPTY => {
                self.activate_parent();
                ret = E::SCHEMAV_CVC_COMPLEX_TYPE_2_1;
                self.verror(
                    ret,
                    "Element content is not allowed, because the content type is empty",
                );
                self.activate_elem();
                unexpected(self, ret)
            }
            CONTENT_MIXED | CONTENT_ELEMENTS => {
                let cm = match &s.it(ptype).cont_model {
                    None => {
                        self.verror_int(
                            "xmlSchemaValidateChildElem",
                            "type has elem content but no content model",
                        );
                        return -1;
                    }
                    Some(c) => c.clone(),
                };
                if self.elems[pd].flags & ELEM_INFO_ERR_BAD_CONTENT != 0 {
                    self.verror_int(
                        "xmlSchemaValidateChildElem",
                        "validating elem, but elem content is already invalid",
                    );
                    return -1;
                }
                if self.elems[pd].regex.is_none() {
                    self.elems[pd].regex = Some(ExecCtxt::new(cm));
                }
                let (l, n) = (self.ino().local.clone(), self.ino().ns.clone());
                let rx = self.elems[pd].regex.as_mut().unwrap();
                rx.callbacks.clear();
                let r = rx.push_string2(&l, n.as_deref(), Some(0));
                let cb = rx.callbacks.iter().rev().find_map(|&(td, _)| td);
                if let Some(d) = cb {
                    self.inode().decl = Some(d);
                }
                if self.ec.err == E::SCHEMAV_INTERNAL {
                    self.verror_int(
                        "xmlSchemaValidateChildElem",
                        "calling xmlRegExecPushString2()",
                    );
                    return -1;
                }
                if r < 0 {
                    let (_, info) = self.elems[pd].regex.as_ref().unwrap().err_info(10);
                    self.complex_type_err(
                        E::SCHEMAV_ELEMENT_CONTENT,
                        "This element is not expected",
                        info,
                    );
                    ret = self.ec.err;
                    return unexpected(self, ret);
                }
                0
            }
            CONTENT_SIMPLE | CONTENT_BASIC => {
                self.activate_parent();
                if s.is_complex(ptype) {
                    self.verror(E::SCHEMAV_CVC_COMPLEX_TYPE_2_2, "Element content is not allowed, because the content type is a simple type definition");
                } else {
                    self.verror(
                        E::SCHEMAV_CVC_TYPE_3_1_2,
                        "Element content is not allowed, because the type definition is simple",
                    );
                }
                self.activate_elem();
                ret = self.ec.err;
                unexpected(self, ret)
            }
            _ => 0,
        }
    }

    /// xmlSchemaValidateElemWildcard
    fn validate_elem_wildcard(&mut self, skip: &mut bool) -> i32 {
        let s = self.schema.clone();
        let w = self.ino().decl.unwrap();
        *skip = false;
        if s.it(w).process_contents == ANY_SKIP {
            *skip = true;
            return 0;
        }
        let (l, n) = (self.ino().local.clone(), self.ino().ns.clone());
        if let Some(d) = s.get_global(TYPE_ELEMENT, &l, n.as_deref()) {
            self.inode().decl = Some(d);
            return 0;
        }
        if s.it(w).process_contents == ANY_STRICT {
            self.verror(E::SCHEMAV_CVC_ELT_1, "No matching global element declaration available, but demanded by the strict wildcard");
            return self.ec.err;
        }
        if !self.attrs.is_empty() {
            if let Some(ia) = self.get_meta_attr(META_XSI_TYPE) {
                let mut lt = None;
                let r = self.process_xsi_type(ia, &mut lt, None);
                self.inode().type_def = lt;
                if r == -1 {
                    self.verror_int(
                        "xmlSchemaValidateElemWildcard",
                        "calling xmlSchemaProcessXSIType() to process the attribute 'xsi:nil'",
                    );
                    return -1;
                }
                return 0;
            }
        }
        let at = s.builtin(T::ANYTYPE);
        self.inode().type_def = Some(at);
        0
    }

    /// xmlSchemaVExpandQName
    fn expand_qname(&mut self, value: &str) -> Result<(Option<String>, String), i32> {
        let s = self.schema.clone();
        let r = T::validate_qname(value, true);
        if r < 0 {
            return Err(-1);
        }
        if r > 0 {
            let qt = s.builtin(T::QNAME);
            self.ec.simple_type_err(
                &s,
                E::SCHEMAV_CVC_DATATYPE_VALID_1_2_1,
                None,
                value,
                qt,
                true,
            );
            return Err(1);
        }
        let (prefix, local) = match T::split_qname2(value) {
            Some((p, l)) => (Some(p), l),
            None => (None, value.to_string()),
        };
        let ns = self.ec.ns_scope.lookup(prefix.as_deref());
        if prefix.is_some() && ns.is_none() {
            let qt = s.builtin(T::QNAME);
            let m = format!(
                "The QName value '{}' has no corresponding namespace declaration in scope",
                value
            );
            self.ec
                .custom_err(&s, E::SCHEMAV_CVC_DATATYPE_VALID_1_2_1, None, Some(qt), &m);
            return Err(2);
        }
        Ok((ns, local))
    }

    /// xmlSchemaProcessXSIType
    fn process_xsi_type(
        &mut self,
        ia: usize,
        local_type: &mut Option<ItemId>,
        elem_decl: Option<ItemId>,
    ) -> i32 {
        let s = self.schema.clone();
        *local_type = None;
        self.set_active(Active::Attr(ia));
        let value = self.attrs[ia].value.clone();
        let mut ret = 0;
        match self.expand_qname(&value) {
            Err(r) => {
                if r < 0 {
                    self.verror_int(
                        "xmlSchemaValidateElementByDeclaration",
                        "calling xmlSchemaQNameExpand() to validate the attribute 'xsi:type'",
                    );
                    self.activate_elem();
                    return -1;
                }
                ret = r;
            }
            Ok((ns, local)) => {
                *local_type = s.get_type(&local, ns.as_deref());
                match *local_type {
                    None => {
                        let qt = s.builtin(T::QNAME);
                        let m = format!(
                            "The QName value '{}' of the xsi:type attribute does not resolve to a type definition",
                            format_qname(ns.as_deref(), Some(&local))
                        );
                        self.ec
                            .custom_err(&s, E::SCHEMAV_CVC_ELT_4_2, None, Some(qt), &m);
                        ret = self.ec.err;
                    }
                    Some(lt) => {
                        if let Some(ed) = elem_decl {
                            let mut set = 0;
                            let et = s.it(ed).subtypes;
                            let ef = s.it(ed).flags;
                            let tf = et.map(|t| s.it(t).flags).unwrap_or(0);
                            if ef & ELEM_BLOCK_EXTENSION != 0 || tf & TYPE_BLOCK_EXTENSION != 0 {
                                set |= 2;
                            }
                            if ef & ELEM_BLOCK_RESTRICTION != 0 || tf & TYPE_BLOCK_RESTRICTION != 0
                            {
                                set |= 1;
                            }
                            let ok = match et {
                                Some(et) => s.cos_derived_ok(lt, et, set),
                                None => false,
                            };
                            if !ok {
                                let m = format!(
                                    "The type definition '{}', specified by xsi:type, is blocked or not validly derived from the type definition of the element declaration",
                                    format_qname(
                                        s.it(lt).target_ns.as_deref(),
                                        s.it(lt).name.as_deref()
                                    )
                                );
                                self.ec
                                    .custom_err(&s, E::SCHEMAV_CVC_ELT_4_3, None, None, &m);
                                ret = self.ec.err;
                                *local_type = None;
                            }
                        }
                    }
                }
            }
        }
        self.activate_elem();
        ret
    }

    /// xmlSchemaValidateElemDecl
    fn validate_elem_decl(&mut self) -> i32 {
        let s = self.schema.clone();
        let decl = match self.ino().decl {
            None => {
                self.verror(E::SCHEMAV_CVC_ELT_1, "No matching declaration available");
                return self.ec.err;
            }
            Some(d) => d,
        };
        let mut actual = s.it(decl).subtypes;
        if s.it(decl).flags & ELEM_ABSTRACT != 0 {
            self.verror(E::SCHEMAV_CVC_ELT_2, "The element declaration is abstract");
            return self.ec.err;
        }
        if actual.is_none() {
            self.verror(E::SCHEMAV_CVC_TYPE_1, "The type definition is absent");
            return E::SCHEMAV_CVC_TYPE_1;
        }
        if !self.attrs.is_empty() {
            if let Some(ia) = self.get_meta_attr(META_XSI_NIL) {
                self.set_active(Active::Attr(ia));
                let bt = s.builtin(T::BOOLEAN);
                let v = self.attrs[ia].value.clone();
                let (ret, val) = self.check_simple(None, bt, &v, true, false);
                self.activate_elem();
                if ret < 0 {
                    self.verror_int("xmlSchemaValidateElemDecl", "calling xmlSchemaVCheckCVCSimpleType() to validate the attribute 'xsi:nil'");
                    return -1;
                }
                if ret == 0 {
                    if s.it(decl).flags & ELEM_NILLABLE == 0 {
                        self.verror(E::SCHEMAV_CVC_ELT_3_1, "The element is not 'nillable'");
                    } else {
                        let b = matches!(
                            val.as_ref().and_then(|v| v.first()),
                            Some(Val {
                                data: T::ValData::Bool(true),
                                ..
                            })
                        );
                        if b {
                            if s.it(decl).flags & ELEM_FIXED != 0 && s.it(decl).value.is_some() {
                                self.verror(E::SCHEMAV_CVC_ELT_3_2_2, "The element cannot be 'nilled' because there is a fixed value constraint defined for it");
                            } else {
                                self.inode().flags |= ELEM_INFO_NILLED;
                            }
                        }
                    }
                }
                self.attrs[ia].val = val;
            }
            if let Some(ia) = self.get_meta_attr(META_XSI_TYPE) {
                let mut lt = None;
                let ret = self.process_xsi_type(ia, &mut lt, Some(decl));
                if ret == -1 {
                    self.verror_int(
                        "xmlSchemaValidateElemDecl",
                        "calling xmlSchemaProcessXSIType() to process the attribute 'xsi:type'",
                    );
                    return -1;
                }
                if lt.is_some() {
                    self.inode().flags |= ELEM_INFO_LOCAL_TYPE;
                    actual = lt;
                }
            }
        }
        if actual.is_none() {
            self.verror(E::SCHEMAV_CVC_TYPE_1, "The type definition is absent");
            return E::SCHEMAV_CVC_TYPE_1;
        }
        self.inode().type_def = actual;
        0
    }

    /// xmlSchemaVAttributesSimple
    fn attributes_simple(&mut self) -> i32 {
        let mut ret = 0;
        for i in 0..self.attrs.len() {
            if self.attrs[i].meta == 0 {
                self.set_active(Active::Attr(i));
                self.illegal_attr_err(E::SCHEMAV_CVC_TYPE_3_1_1, i);
                ret = E::SCHEMAV_CVC_TYPE_3_1_1;
            }
        }
        self.activate_elem();
        ret
    }

    /// xmlSchemaVAttributesComplex
    fn attributes_complex(&mut self) -> i32 {
        let s = self.schema.clone();
        let typ = self.ino().type_def.unwrap();
        let uses = s.it(typ).attr_uses.clone();
        let nb_attrs = self.attrs.len();
        let mut wild_ids = 0;
        for &u in &uses {
            let decl = match s.it(u).attr_decl {
                Some(d) => d,
                None => continue,
            };
            let (dn, dns) = (
                s.it(decl).name.clone().unwrap_or_default(),
                s.it(decl).target_ns.clone(),
            );
            let mut found = false;
            for j in 0..nb_attrs {
                let a = &mut self.attrs[j];
                if a.meta != 0 || a.local != dn || a.ns != dns {
                    continue;
                }
                found = true;
                a.state = ATTR_ASSESSED;
                a.use_ = Some(u);
                a.decl = Some(decl);
                a.type_def = s.it(decl).subtypes;
                break;
            }
            if found {
                continue;
            }
            let occ = s.it(u).occurs;
            if occ == ATTR_USE_REQUIRED {
                self.attrs.push(AttrInfo {
                    state: ATTR_ERR_MISSING,
                    use_: Some(u),
                    decl: Some(decl),
                    ..Default::default()
                });
            } else if occ == ATTR_USE_OPTIONAL
                && (s.it(u).def_value.is_some() || s.it(decl).def_value.is_some())
            {
                self.attrs.push(AttrInfo {
                    state: ATTR_DEFAULT,
                    use_: Some(u),
                    decl: Some(decl),
                    type_def: s.it(decl).subtypes,
                    local: dn,
                    ns: dns,
                    ..Default::default()
                });
            }
        }
        if self.attrs.is_empty() {
            return 0;
        }
        if let Some(w) = s.it(typ).attribute_wildcard {
            for i in 0..nb_attrs {
                if self.attrs[i].state != ATTR_UNKNOWN {
                    continue;
                }
                let ans = self.attrs[i].ns.clone();
                if wildcard_ns_ok(&s, w, ans.as_deref()) {
                    if s.it(w).process_contents == ANY_SKIP {
                        self.attrs[i].state = ATTR_WILD_SKIP;
                        continue;
                    }
                    let l = self.attrs[i].local.clone();
                    let d = s.get_global(TYPE_ATTRIBUTE, &l, ans.as_deref());
                    self.attrs[i].decl = d;
                    if let Some(d) = d {
                        self.attrs[i].state = ATTR_ASSESSED;
                        let td = s.it(d).subtypes;
                        self.attrs[i].type_def = td;
                        if is_derived_from_builtin(&s, td, T::ID) {
                            if wild_ids != 0 {
                                self.attrs[i].state = ATTR_ERR_WILD_DUPLICATE_ID;
                                continue;
                            }
                            wild_ids += 1;
                            for &u in &uses {
                                let ut = s.it(u).attr_decl.and_then(|d| s.it(d).subtypes);
                                if is_derived_from_builtin(&s, ut, T::ID) {
                                    self.attrs[i].state = ATTR_ERR_WILD_AND_USE_ID;
                                    break;
                                }
                            }
                        }
                    } else if s.it(w).process_contents == ANY_LAX {
                        self.attrs[i].state = ATTR_WILD_LAX_NO_DECL;
                    } else {
                        self.attrs[i].state = ATTR_ERR_WILD_STRICT_NO_DECL;
                    }
                }
            }
        }
        let def_owner = if self.options & VAL_VC_I_CREATE != 0 {
            self.ino().node
        } else {
            None
        };
        for i in 0..self.attrs.len() {
            let st = self.attrs[i].state;
            if st != ATTR_ASSESSED && st != ATTR_DEFAULT {
                continue;
            }
            let td = match self.attrs[i].type_def {
                None => {
                    self.attrs[i].state = ATTR_ERR_NO_TYPE;
                    continue;
                }
                Some(t) => t,
            };
            self.set_active(Active::Attr(i));
            let u = self.attrs[i].use_;
            let decl = self.attrs[i].decl.unwrap();
            if st == ATTR_DEFAULT {
                if def_owner.is_some() {
                    let (value, val) = match u.and_then(|u| s.it(u).def_value.clone()) {
                        Some(v) => (v, None::<Val>),
                        None => (
                            s.it(decl).def_value.clone().unwrap_or_default(),
                            s.it(decl).def_val.clone(),
                        ),
                    };
                    // use->defVal is never computed by libxml2
                    let has_val = if u.and_then(|u| s.it(u).def_value.clone()).is_some() {
                        false
                    } else {
                        val.is_some()
                    };
                    if !has_val {
                        self.verror_int(
                            "xmlSchemaVAttributesComplex",
                            "default/fixed value on an attribute use was not precomputed",
                        );
                        self.activate_elem();
                        return -1;
                    }
                    self.attrs[i].value = value;
                }
                if let Some(owner) = def_owner {
                    let value = self.attrs[i].value.clone();
                    let value = normalize_value(&s, td, &value).unwrap_or(value);
                    let (local, ans) = (self.attrs[i].local.clone(), self.attrs[i].ns.clone());
                    let root = self.validation_root;
                    let ok = tree::with(|t| {
                        match &ans {
                            None => {
                                t.new_prop(owner, None, &local, Some(&value));
                            }
                            Some(href) => {
                                let mut ns = t.search_ns_by_href(owner, href);
                                if ns.is_none() {
                                    let mut counter = 0;
                                    let mut prefix;
                                    loop {
                                        prefix = format!("p{}", counter);
                                        counter += 1;
                                        if t.search_ns(owner, Some(&prefix)).is_none() {
                                            break;
                                        }
                                        if counter > 1000 {
                                            return false;
                                        }
                                    }
                                    ns = t.new_ns(root, href, Some(&prefix));
                                }
                                t.new_prop(owner, ns, &local, Some(&value));
                            }
                        }
                        true
                    });
                    if !ok {
                        self.verror_int(
                            "xmlSchemaVAttributesComplex",
                            "could not compute a ns prefix for a default/fixed attribute",
                        );
                        self.activate_elem();
                        return -1;
                    }
                }
                continue;
            }
            let fixed = s.it(decl).flags & ATTR_FIXED != 0
                || u.map(|u| s.it(u).flags & ATTR_FIXED != 0).unwrap_or(false);
            let node = self.attrs[i].node;
            let value = self.attrs[i].value.clone();
            let (res, val) = if fixed {
                self.attrs[i].flags |= NODE_INFO_VALUE_NEEDED;
                self.check_simple(node, td, &value, true, true)
            } else {
                self.check_simple(node, td, &value, false, false)
            };
            if res != 0 {
                if res == -1 {
                    self.verror_int(
                        "xmlSchemaVAttributesComplex",
                        "calling xmlSchemaStreamValidateSimpleTypeValue()",
                    );
                    self.activate_elem();
                    return -1;
                }
                self.attrs[i].state = ATTR_INVALID_VALUE;
                continue;
            }
            self.attrs[i].val = val.clone();
            if fixed {
                let v = match &val {
                    None => continue,
                    Some(v) => v.clone(),
                };
                if let Some(udv) = u.and_then(|u| s.it(u).def_value.clone()) {
                    // use->defVal is never precomputed
                    let _ = udv;
                    continue;
                }
                let dv = match &s.it(decl).def_val {
                    None => continue,
                    Some(d) => vec![d.clone()],
                };
                self.attrs[i].vc_value = s.it(decl).def_value.clone();
                if super::simple::values_equal(&s, &v, &dv) == 0 {
                    self.attrs[i].state = ATTR_ERR_FIXED_VALUE;
                }
            }
        }
        for i in 0..self.attrs.len() {
            let st = self.attrs[i].state;
            if matches!(
                st,
                ATTR_META | ATTR_ASSESSED | ATTR_WILD_SKIP | ATTR_WILD_LAX_NO_DECL
            ) {
                continue;
            }
            self.set_active(Active::Attr(i));
            match st {
                ATTR_ERR_MISSING => {
                    self.activate_elem();
                    let d = self.attrs[i].decl.unwrap();
                    let m = format!("The attribute '{}' is required but missing", format_qname(s.it(d).target_ns.as_deref(), s.it(d).name.as_deref()));
                    self.ec.custom_err(&s, E::SCHEMAV_CVC_COMPLEX_TYPE_4, None, None, &m);
                }
                ATTR_ERR_NO_TYPE => self.verror(E::SCHEMAV_CVC_ATTRIBUTE_2, "The type definition is absent"),
                ATTR_ERR_FIXED_VALUE => {
                    let m = format!("The value '{}' does not match the fixed value constraint '{}'", self.attrs[i].value, self.attrs[i].vc_value.clone().unwrap_or_default());
                    self.ec.custom_err(&s, E::SCHEMAV_CVC_AU, None, None, &m);
                }
                ATTR_ERR_WILD_STRICT_NO_DECL => self.verror(E::SCHEMAV_CVC_WILDCARD, "No matching global attribute declaration available, but demanded by the strict wildcard"),
                ATTR_UNKNOWN => {
                    if self.attrs[i].meta != 0 {
                        continue;
                    }
                    if s.it(typ).attribute_wildcard.is_none() {
                        self.illegal_attr_err(E::SCHEMAV_CVC_COMPLEX_TYPE_3_2_1, i);
                    } else {
                        self.illegal_attr_err(E::SCHEMAV_CVC_COMPLEX_TYPE_3_2_2, i);
                    }
                }
                _ => {}
            }
        }
        self.activate_elem();
        0
    }

    /// xmlSchemaVPushText
    fn push_text(&mut self, node_type: i32, value: &str) -> i32 {
        let s = self.schema.clone();
        if self.ino().flags & ELEM_INFO_NILLED != 0 {
            self.verror(
                E::SCHEMAV_CVC_ELT_3_2_1,
                "Neither character nor element content is allowed because the element is 'nilled'",
            );
            return self.ec.err;
        }
        let td = match self.ino().type_def {
            Some(t) => t,
            None => return 0,
        };
        let ct = s.it(td).content_type;
        if ct == CONTENT_EMPTY {
            self.verror(
                E::SCHEMAV_CVC_COMPLEX_TYPE_2_1,
                "Character content is not allowed, because the content type is empty",
            );
            return self.ec.err;
        }
        if ct == CONTENT_ELEMENTS {
            if node_type != TEXT_NODE
                || !value
                    .bytes()
                    .all(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r'))
            {
                self.verror(E::SCHEMAV_CVC_COMPLEX_TYPE_2_3, "Character content other than whitespace is not allowed because the content type is 'element-only'");
                return self.ec.err;
            }
            return 0;
        }
        if value.is_empty() {
            return 0;
        }
        if ct == CONTENT_MIXED
            && self
                .ino()
                .decl
                .map(|d| s.it(d).value.is_none())
                .unwrap_or(true)
        {
            return 0;
        }
        let e = self.inode();
        match &mut e.value {
            None => e.value = Some(value.to_string()),
            Some(v) => v.push_str(value),
        }
        0
    }

    fn check_inode_data_type(&mut self, typ: ItemId, value: &str) -> i32 {
        let need = self.ino().flags & NODE_INFO_VALUE_NEEDED != 0;
        let (r, v) = self.check_simple(None, typ, value, need, need);
        if need {
            self.inode().val = v;
        }
        r
    }

    /// xmlSchemaCheckCOSValidDefault (validator)
    fn check_cos_valid_default(&mut self, value: &str) -> i32 {
        let s = self.schema.clone();
        let td = self.ino().type_def.unwrap();
        if s.is_complex(td)
            && !s.has_simple_content(td)
            && (s.it(td).content_type != CONTENT_MIXED
                || !is_particle_emptiable(&s, s.it(td).subtypes))
        {
            let ret = E::SCHEMAP_COS_VALID_DEFAULT_2_1;
            self.verror(ret, "For a string to be a valid default, the type definition must be a simple type or a complex type with simple content or mixed content and a particle emptiable");
            return ret;
        }
        let mut ret = 0;
        if s.is_simple(td) {
            let (r, v) = self.check_simple(None, td, value, true, true);
            ret = r;
            self.inode().val = v;
        } else if s.has_simple_content(td) {
            let ctd = s.it(td).content_type_def.unwrap();
            let (r, v) = self.check_simple(None, ctd, value, true, true);
            ret = r;
            self.inode().val = v;
        }
        if ret < 0 {
            self.verror_int(
                "xmlSchemaCheckCOSValidDefault",
                "calling xmlSchemaVCheckCVCSimpleType()",
            );
        }
        ret
    }

    /// xmlSchemaValidatorPopElem
    fn pop_elem(&mut self) -> i32 {
        let s = self.schema.clone();
        let mut ret = 0;
        self.attrs.clear();
        let flags = self.ino().flags;
        'end: {
            if flags & NODE_INFO_ERR_NOT_EXPECTED != 0 {
                self.skip_depth = self.depth - 1;
                break 'end;
            }
            let td = match self.ino().type_def {
                Some(t) if flags & NODE_INFO_ERR_BAD_TYPE == 0 => t,
                _ => break 'end,
            };
            let ct = s.it(td).content_type;
            let mut character_content = false;
            if ct == CONTENT_MIXED || ct == CONTENT_ELEMENTS {
                if s.it(td).builtin_type == T::ANYTYPE {
                    character_content = true;
                } else if self.ino().flags & ELEM_INFO_ERR_BAD_CONTENT == 0 {
                    if self.ino().regex.is_none() {
                        match &s.it(td).cont_model {
                            Some(c) => self.inode().regex = Some(ExecCtxt::new(c.clone())),
                            None => {
                                self.verror_int(
                                    "xmlSchemaValidatorPopElem",
                                    "failed to create a regex context",
                                );
                                self.ec.err = -1;
                                return -1;
                            }
                        }
                    }
                    if self.ino().flags & ELEM_INFO_NILLED == 0 {
                        let rx = self.inode().regex.as_mut().unwrap();
                        let info = rx.next_values(10);
                        rx.callbacks.clear();
                        let r = rx.push_string(None, None);
                        if r <= 0 {
                            ret = 1;
                            self.inode().flags |= ELEM_INFO_ERR_BAD_CONTENT;
                            self.complex_type_err(
                                E::SCHEMAV_ELEMENT_CONTENT,
                                "Missing child element(s)",
                                info,
                            );
                        } else {
                            ret = 0;
                        }
                    }
                }
            }
            if !character_content && ct == CONTENT_ELEMENTS {
                break 'end;
            }
            // character_content:
            let decl = self.ino().decl;
            let value = self.ino().value.clone();
            let nilled = self.ino().flags & ELEM_INFO_NILLED != 0;
            match decl {
                None => {
                    let v = value.unwrap_or_default();
                    if s.is_simple(td) {
                        ret = self.check_inode_data_type(td, &v);
                    } else if s.has_simple_content(td) {
                        let c = s.it(td).content_type_def.unwrap();
                        ret = self.check_inode_data_type(c, &v);
                    }
                    if ret < 0 {
                        self.verror_int(
                            "xmlSchemaValidatorPopElem",
                            "calling xmlSchemaVCheckCVCSimpleType()",
                        );
                        self.ec.err = -1;
                        return -1;
                    }
                    break 'end;
                }
                Some(d) => {
                    let dval = s.it(d).value.clone();
                    if let (Some(dv), true) = (
                        dval.clone(),
                        self.ino().flags & ELEM_INFO_EMPTY != 0 && !nilled,
                    ) {
                        if self.ino().flags & ELEM_INFO_LOCAL_TYPE != 0 {
                            ret = self.check_cos_valid_default(&dv);
                            if ret != 0 {
                                if ret < 0 {
                                    self.verror_int(
                                        "xmlSchemaValidatorPopElem",
                                        "calling xmlSchemaCheckCOSValidDefault()",
                                    );
                                    self.ec.err = -1;
                                    return -1;
                                }
                                break 'end;
                            }
                        } else {
                            if s.is_simple(td) {
                                ret = self.check_inode_data_type(td, &dv);
                            } else if s.has_simple_content(td) {
                                let c = s.it(td).content_type_def.unwrap();
                                ret = self.check_inode_data_type(c, &dv);
                            }
                            if ret != 0 {
                                if ret < 0 {
                                    self.verror_int(
                                        "xmlSchemaValidatorPopElem",
                                        "calling xmlSchemaVCheckCVCSimpleType()",
                                    );
                                    self.ec.err = -1;
                                    return -1;
                                }
                                break 'end;
                            }
                        }
                        // default_psvi:
                        if self.options & VAL_VC_I_CREATE != 0 {
                            if let Some(n) = self.ino().node {
                                let nv = normalize_value(&s, td, &dv).unwrap_or(dv);
                                tree::with(|t| {
                                    let doc = t.nodes[n].doc;
                                    let tx = t.new_text(doc, &nv);
                                    t.add_child(n, tx);
                                });
                            }
                        }
                    } else if !nilled {
                        let v = value.clone().unwrap_or_default();
                        if s.is_simple(td) {
                            ret = self.check_inode_data_type(td, &v);
                        } else if s.has_simple_content(td) {
                            let c = s.it(td).content_type_def.unwrap();
                            ret = self.check_inode_data_type(c, &v);
                        }
                        if ret != 0 {
                            if ret < 0 {
                                self.verror_int(
                                    "xmlSchemaValidatorPopElem",
                                    "calling xmlSchemaVCheckCVCSimpleType()",
                                );
                                self.ec.err = -1;
                                return -1;
                            }
                            break 'end;
                        }
                        if let Some(dv) = &dval {
                            if s.it(d).flags & ELEM_FIXED != 0 {
                                if self.ino().flags & ELEM_INFO_HAS_ELEM_CONTENT != 0 {
                                    ret = E::SCHEMAV_CVC_ELT_5_2_2_1;
                                    self.verror(ret, "The content must not contain element nodes since there is a fixed value constraint");
                                    break 'end;
                                }
                                let iv = value.clone().unwrap_or_default();
                                let mixed = s.it(td).content_type == CONTENT_MIXED;
                                if mixed {
                                    if iv != *dv {
                                        ret = E::SCHEMAV_CVC_ELT_5_2_2_2_1;
                                        let m = format!(
                                            "The initial value '{}' does not match the fixed value constraint '{}'",
                                            iv, dv
                                        );
                                        self.ec.custom_err(&s, ret, None, None, &m);
                                        break 'end;
                                    }
                                } else if s.has_simple_content(td) && iv != *dv {
                                    ret = E::SCHEMAV_CVC_ELT_5_2_2_2_2;
                                    let m = format!(
                                        "The actual value '{}' does not match the fixed value constraint '{}'",
                                        iv, dv
                                    );
                                    self.ec.custom_err(&s, ret, None, None, &m);
                                    break 'end;
                                }
                            }
                        }
                    }
                }
            }
        }
        // end_elem:
        if self.depth < 0 {
            return 0;
        }
        if self.depth == self.skip_depth {
            self.skip_depth = -1;
        }
        let d = self.depth as usize;
        self.elems[d] = ElemInfo::default();
        if self.depth == 0 {
            self.depth -= 1;
            self.set_active(Active::None);
            return 0;
        }
        self.depth -= 1;
        self.activate_elem();
        ret
    }
}

/// xmlSchemaCheckCVCWildcardNamespace (true when the namespace matches)
pub fn wildcard_ns_ok(s: &Schema, w: ItemId, ns: Option<&str>) -> bool {
    let wi = s.it(w);
    if wi.any {
        return true;
    }
    if !wi.ns_set.is_empty() {
        return wi.ns_set.iter().any(|v| v.as_deref() == ns);
    }
    if let Some(neg) = &wi.neg_ns_set {
        if ns.is_some() && neg.as_deref() != ns {
            return true;
        }
    }
    false
}

impl Schema {
    /// Compile a schema from a file/URL (xmlSchemaNewParserCtxt + xmlSchemaParse).
    pub fn parse_file(
        vm: &mut goro_core::vm::Vm,
        url: &str,
    ) -> (Option<Rc<Schema>>, Vec<XmlError>) {
        let (s, e) = super::parse_doc::schema_parse(vm, Some(url), None);
        (s.map(Rc::new), e)
    }

    /// Compile a schema from memory (xmlSchemaNewMemParserCtxt + xmlSchemaParse).
    pub fn parse_memory(
        vm: &mut goro_core::vm::Vm,
        data: &[u8],
    ) -> (Option<Rc<Schema>>, Vec<XmlError>) {
        let (s, e) = super::parse_doc::schema_parse(vm, None, Some(data));
        (s.map(Rc::new), e)
    }
}
