//! Simple type validation (xmlSchemaVCheckCVCSimpleType and friends) and
//! the error reporting shared by the schema parser and the validator.

use super::*;
use crate::xmlschemastypes::{self as T, NodeCtx};

/// Description of the node currently validated (xmlSchemaNodeInfo as
/// far as error reports are concerned).
#[derive(Clone, Debug, Default)]
pub struct NodeDesc {
    pub is_attr: bool,
    pub local: String,
    pub ns: Option<String>,
    /// the owner element, for attributes
    pub elem_local: String,
    pub elem_ns: Option<String>,
    pub node: Option<NodeId>,
    pub line: i32,
}

/// Namespace context for QName/NOTATION values.
#[derive(Clone, Debug, Default)]
pub enum NsScope {
    #[default]
    None,
    Node(NodeId),
    Bindings(Vec<(Option<String>, String)>),
}

impl NsScope {
    pub fn lookup(&self, prefix: Option<&str>) -> Option<String> {
        match self {
            NsScope::None => None,
            NsScope::Node(n) => search_ns(*n, prefix),
            NsScope::Bindings(_) if prefix == Some("xml") => {
                Some(super::parse::XML_XML_NAMESPACE.to_string())
            }
            NsScope::Bindings(b) => b
                .iter()
                .rev()
                .find(|(p, _)| p.as_deref() == prefix)
                .map(|(_, h)| h.clone()),
        }
    }
}

/// Abstract error context (xmlSchemaAbstractCtxt).
#[derive(Default)]
pub struct ErrCtx {
    pub validator: bool,
    pub errors: Vec<XmlError>,
    pub nberrors: i32,
    pub err: i32,
    pub inode: Option<NodeDesc>,
    pub ns_scope: NsScope,
    /// file name for streaming validation (no node tree)
    pub filename: Option<String>,
}

impl ErrCtx {
    pub fn parser() -> ErrCtx {
        ErrCtx {
            validator: false,
            ..Default::default()
        }
    }

    fn domain(&self) -> i32 {
        if self.validator {
            E::FROM_SCHEMASV
        } else {
            E::FROM_SCHEMASP
        }
    }

    /// xmlSchemaErr4Line
    pub fn err4line(
        &mut self,
        level: i32,
        code: i32,
        node: Option<NodeId>,
        line: i32,
        msg: String,
    ) {
        if level != ERR_WARNING {
            self.nberrors += 1;
            self.err = code;
        }
        let dom = self.domain();
        if self.validator {
            let mut node = node;
            let mut file = None;
            let mut line = line;
            if line == 0 {
                if node.is_none() {
                    node = self.inode.as_ref().and_then(|i| i.node);
                }
                if node.is_none() {
                    file = self.filename.clone();
                    line = self.inode.as_ref().map(|i| i.line).unwrap_or(0);
                }
            } else {
                file = self.filename.clone();
                node = None;
            }
            self.errors
                .push(mk_error(dom, code, level, msg, node, file, line));
        } else {
            self.errors
                .push(mk_error(dom, code, level, msg, node, None, 0));
        }
    }

    pub fn err(&mut self, code: i32, node: Option<NodeId>, msg: String) {
        self.err4line(ERR_ERROR, code, node, 0, msg);
    }

    /// xmlSchemaFormatNodeForError
    pub fn format_node(&self, node: Option<NodeId>) -> String {
        if let Some(n) = node {
            return super::parse::format_node_for_error(n);
        }
        if self.validator {
            if let Some(i) = &self.inode {
                if i.is_attr {
                    return format!(
                        "Element '{}', attribute '{}': ",
                        format_qname(i.elem_ns.as_deref(), Some(&i.elem_local)),
                        format_qname(i.ns.as_deref(), Some(&i.local))
                    );
                }
                return format!(
                    "Element '{}': ",
                    format_qname(i.ns.as_deref(), Some(&i.local))
                );
            }
        }
        String::new()
    }

    /// xmlSchemaEvalErrorNodeType == XML_ATTRIBUTE_NODE
    pub fn error_node_is_attr(&self, node: Option<NodeId>) -> bool {
        match node {
            Some(n) => node_type(n) == NType::Attribute,
            None => self.validator && self.inode.as_ref().map(|i| i.is_attr).unwrap_or(false),
        }
    }

    pub fn internal_err(&mut self, func: &str, msg: &str) {
        let code = if self.validator {
            E::SCHEMAV_INTERNAL
        } else {
            E::SCHEMAP_INTERNAL
        };
        self.err(code, None, format!("Internal error: {}, {}.\n", func, msg));
    }

    /// xmlSchemaCustomErr4
    pub fn custom_err(
        &mut self,
        s: &Schema,
        code: i32,
        node: Option<NodeId>,
        item: Option<ItemId>,
        message: &str,
    ) {
        let mut node = node;
        let mut msg;
        if let (None, Some(it), false) = (node, item, self.validator) {
            node = s.component_node(it);
            msg = s.format_item_for_report(None, item, None);
            msg.push_str(": ");
        } else {
            msg = self.format_node(node);
        }
        msg.push_str(message);
        msg.push_str(".\n");
        self.err(code, node, msg);
    }

    /// xmlSchemaSimpleTypeErr
    pub fn simple_type_err(
        &mut self,
        s: &Schema,
        code: i32,
        node: Option<NodeId>,
        value: &str,
        typ: ItemId,
        display_value: bool,
    ) {
        let mut msg = self.format_node(node);
        let show = display_value || self.error_node_is_attr(node);
        if show {
            msg.push_str(&format!("'{}' is not a valid value of ", value));
        } else {
            msg.push_str("The character content is not a valid value of ");
        }
        msg.push_str(&s.simple_type_designation(typ));
        msg.push_str(".\n");
        self.err(code, node, msg);
    }

    /// xmlSchemaFacetErr (default messages)
    pub fn facet_err(
        &mut self,
        s: &Schema,
        code: i32,
        node: Option<NodeId>,
        value: &str,
        length: u64,
        typ: ItemId,
        facet: Option<&Facet>,
    ) {
        let is_attr = self.error_node_is_attr(node);
        let mut msg = self.format_node(node);
        let ftype = if code == E::SCHEMAV_CVC_ENUMERATION_VALID {
            T::FACET_ENUMERATION
        } else {
            facet.map(|f| f.typ).unwrap_or(0)
        };
        msg.push_str("[facet '");
        msg.push_str(facet_type_to_string(ftype));
        msg.push_str("'] ");
        let fval = facet.map(|f| f.value.clone()).unwrap_or_default();
        match ftype {
            T::FACET_LENGTH | T::FACET_MINLENGTH | T::FACET_MAXLENGTH => {
                let len = facet.map(|f| f.as_ulong()).unwrap_or(0);
                if is_attr {
                    msg.push_str(&format!(
                        "The value '{}' has a length of '{}'; ",
                        value, length
                    ));
                } else {
                    msg.push_str(&format!("The value has a length of '{}'; ", length));
                }
                match ftype {
                    T::FACET_LENGTH => msg.push_str(&format!(
                        "this differs from the allowed length of '{}'.\n",
                        len
                    )),
                    T::FACET_MAXLENGTH => msg.push_str(&format!(
                        "this exceeds the allowed maximum length of '{}'.\n",
                        len
                    )),
                    _ => msg.push_str(&format!(
                        "this underruns the allowed minimum length of '{}'.\n",
                        len
                    )),
                }
            }
            T::FACET_ENUMERATION => msg.push_str(&format!(
                "The value '{}' is not an element of the set {{{}}}.\n",
                value,
                s.format_facet_enum_set(typ)
            )),
            T::FACET_PATTERN => msg.push_str(&format!(
                "The value '{}' is not accepted by the pattern '{}'.\n",
                value, fval
            )),
            T::FACET_MININCLUSIVE => msg.push_str(&format!(
                "The value '{}' is less than the minimum value allowed ('{}').\n",
                value, fval
            )),
            T::FACET_MAXINCLUSIVE => msg.push_str(&format!(
                "The value '{}' is greater than the maximum value allowed ('{}').\n",
                value, fval
            )),
            T::FACET_MINEXCLUSIVE => msg.push_str(&format!(
                "The value '{}' must be greater than '{}'.\n",
                value, fval
            )),
            T::FACET_MAXEXCLUSIVE => msg.push_str(&format!(
                "The value '{}' must be less than '{}'.\n",
                value, fval
            )),
            T::FACET_TOTALDIGITS => msg.push_str(&format!(
                "The value '{}' has more digits than are allowed ('{}').\n",
                value, fval
            )),
            T::FACET_FRACTIONDIGITS => msg.push_str(&format!(
                "The value '{}' has more fractional digits than are allowed ('{}').\n",
                value, fval
            )),
            _ => {
                if is_attr {
                    msg.push_str(&format!("The value '{}' is not facet-valid.\n", value));
                } else {
                    msg.push_str("The value is not facet-valid.\n");
                }
            }
        }
        self.err(code, node, msg);
    }
}

/// A computed simple type value (lists have several items).
pub type VList = Vec<Val>;

/// xmlSchemaGetPrimitiveType
pub fn primitive_type(s: &Schema, t: ItemId) -> Option<ItemId> {
    let mut cur = Some(t);
    while let Some(c) = cur {
        let it = s.it(c);
        if it.builtin_type == T::ANYSIMPLETYPE || it.flags & TYPE_BUILTIN_PRIMITIVE != 0 {
            return Some(c);
        }
        if it.base_type == Some(c) {
            return None;
        }
        cur = it.base_type;
    }
    None
}

/// xmlSchemaGetUnionSimpleTypeMemberTypes
pub fn union_member_types(s: &Schema, t: ItemId) -> Vec<ItemId> {
    let mut cur = Some(t);
    while let Some(c) = cur {
        let it = s.it(c);
        if it.typ != TYPE_SIMPLE {
            break;
        }
        if !it.member_types.is_empty() {
            return it.member_types.clone();
        }
        cur = it.base_type;
    }
    Vec::new()
}

/// xmlSchemaIsDerivedFromBuiltInType
pub fn is_derived_from_builtin(s: &Schema, t: Option<ItemId>, vt: i32) -> bool {
    let mut cur = t;
    while let Some(c) = cur {
        if s.is_complex(c) {
            return false;
        }
        let it = s.it(c);
        if it.typ == TYPE_BASIC {
            if it.builtin_type == vt {
                return true;
            }
            if it.builtin_type == T::ANYSIMPLETYPE || it.builtin_type == T::ANYTYPE {
                return false;
            }
        }
        cur = it.subtypes;
    }
    false
}

/// xmlSchemaAreValuesEqual: 1 equal, 0 not, -1 error
pub fn values_equal(s: &Schema, x: &[Val], y: &[Val]) -> i32 {
    let mut i = 0;
    while i < x.len() {
        let xv = &x[i];
        let yv = match y.get(i) {
            Some(v) => v,
            None => return 0,
        };
        let tx = s.builtins.get(&xv.typ).copied();
        let ty = s.builtins.get(&yv.typ).copied();
        let ptx = tx.and_then(|t| primitive_type(s, t));
        let pty = ty.and_then(|t| primitive_type(s, t));
        if ptx != pty {
            return 0;
        }
        let pt = match ptx {
            Some(p) => p,
            None => return 0,
        };
        if s.it(pt).builtin_type == T::STRING || s.is_any_simple_type(pt) {
            if xv.str_value() != yv.str_value() {
                return 0;
            }
        } else {
            let r = T::compare_values_whtsp(xv, T::WS_PRESERVE, yv, T::WS_PRESERVE);
            if r == -2 {
                return -1;
            }
            if r != 0 {
                return 0;
            }
        }
        i += 1;
        if i < x.len() {
            if i >= y.len() {
                return 0;
            }
        } else if i < y.len() {
            return 0;
        } else {
            return 1;
        }
    }
    0
}

/// Node context for the built-in type checks (xmlNodePtr `node` param).
struct NodeArg<'n> {
    node: Option<NodeId>,
    scope: &'n NsScope,
}

impl NodeCtx for NodeArg<'_> {
    fn search_ns(&self, prefix: Option<&str>) -> Option<String> {
        match self.node {
            Some(n) => search_ns(n, prefix),
            None => self.scope.lookup(prefix),
        }
    }
    fn has_doc(&self) -> bool {
        match self.node {
            Some(n) => T::TreeNode(n).has_doc(),
            None => false,
        }
    }
    fn unparsed_entity(&self, name: &str) -> bool {
        self.node
            .map(|n| T::TreeNode(n).unparsed_entity(name))
            .unwrap_or(false)
    }
    fn notation_declared(&self, name: &str) -> bool {
        self.node
            .map(|n| T::TreeNode(n).notation_declared(name))
            .unwrap_or(false)
    }
    fn add_id(&self, value: &str) -> Option<i32> {
        self.node.and_then(|n| T::TreeNode(n).add_id(value))
    }
    fn set_attr_type(&self, atype: u8) {
        if let Some(n) = self.node {
            T::TreeNode(n).set_attr_type(atype)
        }
    }
}

/// xmlSchemaNormalizeValue
pub fn normalize_value(s: &Schema, t: ItemId, value: &str) -> Option<String> {
    match s.whitespace_facet_value(t) {
        T::WS_COLLAPSE => T::collapse_string(value),
        T::WS_REPLACE => T::whitespace_replace(value),
        _ => None,
    }
}

/// xmlSchemaValidateFacets
pub fn validate_facets(
    s: &Schema,
    ec: &mut ErrCtx,
    node: Option<NodeId>,
    typ: ItemId,
    val_type: i32,
    value: &str,
    val: Option<&[Val]>,
    length: u64,
    fire: bool,
) -> i32 {
    let it = s.it(typ);
    if it.typ == TYPE_BASIC {
        return 0;
    }
    let mut error = 0;
    let first = val.and_then(|v| v.first());
    if !it.facet_set.is_empty() && s.is_atomic(typ) {
        let pt = primitive_type(s, typ);
        let ws = match pt {
            Some(p) if s.it(p).builtin_type == T::STRING || s.is_any_simple_type(p) => {
                s.whitespace_facet_value(typ)
            }
            _ => T::WS_COLLAPSE,
        };
        let vt = first.map(|v| v.typ).unwrap_or(val_type);
        for &(ft, fi) in &it.facet_set {
            let f = &s.it(ft).facets[fi].f;
            let mut len = 0u64;
            let ret = match f.typ {
                T::FACET_WHITESPACE | T::FACET_PATTERN | T::FACET_ENUMERATION => continue,
                T::FACET_LENGTH | T::FACET_MINLENGTH | T::FACET_MAXLENGTH => {
                    let (r, l) = T::validate_length_facet_whtsp(f, vt, Some(value), first, ws);
                    len = l;
                    r
                }
                _ => T::validate_facet_whtsp(f, ws, vt, value, first, ws),
            };
            if ret < 0 {
                ec.internal_err(
                    "xmlSchemaValidateFacets",
                    "validating against a atomic type facet",
                );
                return -1;
            } else if ret > 0 {
                if fire {
                    ec.facet_err(s, ret, node, value, len, typ, Some(f));
                } else {
                    return ret;
                }
                if error == 0 {
                    error = ret;
                }
            }
        }
    }
    if !it.facet_set.is_empty() && s.is_list(typ) {
        for &(ft, fi) in &it.facet_set {
            let f = &s.it(ft).facets[fi].f;
            let ret = match f.typ {
                T::FACET_LENGTH | T::FACET_MINLENGTH | T::FACET_MAXLENGTH => {
                    T::validate_list_simple_type_facet(f, value, length).0
                }
                _ => continue,
            };
            if ret < 0 {
                ec.internal_err(
                    "xmlSchemaValidateFacets",
                    "validating against a list type facet",
                );
                return -1;
            } else if ret > 0 {
                if fire {
                    ec.facet_err(s, ret, node, value, length, typ, Some(f));
                } else {
                    return ret;
                }
                if error == 0 {
                    error = ret;
                }
            }
        }
    }
    // enumerations
    let mut found = false;
    let mut ret = 0;
    let mut tmp = Some(typ);
    let empty: Vec<Val> = Vec::new();
    let vals = val.unwrap_or(&empty);
    while let Some(t) = tmp {
        if s.it(t).typ == TYPE_BASIC {
            break;
        }
        for f in &s.it(t).facets {
            if f.f.typ != T::FACET_ENUMERATION {
                continue;
            }
            found = true;
            let fvals: Vec<Val> = match &f.vals {
                Some(v) => v.clone(),
                None => f.f.val.clone().map(|v| vec![v]).unwrap_or_default(),
            };
            ret = values_equal(s, &fvals, vals);
            if ret == 1 {
                break;
            } else if ret < 0 {
                ec.internal_err(
                    "xmlSchemaValidateFacets",
                    "validating against an enumeration facet",
                );
                return -1;
            }
        }
        if ret != 0 || found {
            break;
        }
        tmp = s.it(t).base_type;
    }
    if found && ret == 0 {
        let r = E::SCHEMAV_CVC_ENUMERATION_VALID;
        if fire {
            ec.facet_err(s, r, node, value, 0, typ, None);
        } else {
            return r;
        }
        if error == 0 {
            error = r;
        }
    }
    // patterns
    let mut tmp = Some(typ);
    while let Some(t) = tmp {
        if s.it(t).typ == TYPE_BASIC {
            break;
        }
        let mut found = false;
        let mut ret = 0;
        let mut last: Option<&Facet> = None;
        for &(ft, fi) in &s.it(t).facet_set {
            let f = &s.it(ft).facets[fi].f;
            if f.typ != T::FACET_PATTERN {
                continue;
            }
            found = true;
            ret = match &f.regexp {
                Some(re) => re.exec(value),
                None => -1,
            };
            if ret == 1 {
                break;
            } else if ret < 0 {
                ec.internal_err(
                    "xmlSchemaValidateFacets",
                    "validating against a pattern facet",
                );
                return -1;
            } else {
                last = Some(f);
            }
        }
        if found && ret != 1 {
            let r = E::SCHEMAV_CVC_PATTERN_VALID;
            if fire {
                let lf = last.cloned();
                ec.facet_err(s, r, node, value, 0, typ, lf.as_ref());
            } else {
                return r;
            }
            if error == 0 {
                error = r;
            }
            break;
        }
        tmp = s.it(t).base_type;
    }
    error
}

/// xmlSchemaVCheckCVCSimpleType: (ret, computed value when valid)
#[allow(unused_assignments)]
pub fn check_cvc_simple_type(
    s: &Schema,
    ec: &mut ErrCtx,
    node: Option<NodeId>,
    typ: ItemId,
    value: &str,
    want_val: bool,
    fire: bool,
    normalize: bool,
    is_normalized: bool,
) -> (i32, Option<VList>) {
    let mut val_needed = want_val;
    let mut value: String = value.to_string();
    let mut is_normalized = is_normalized;
    let flags = s.it(typ).flags;
    if !val_needed && flags & TYPE_FACETSNEEDVALUE != 0 {
        val_needed = true;
    }
    macro_rules! normalize {
        ($t:expr, $norm:expr) => {
            if !is_normalized && ($norm || flags & TYPE_NORMVALUENEEDED != 0) {
                if let Some(n) = normalize_value(s, $t, &value) {
                    value = n;
                }
                is_normalized = true;
            }
        };
    }
    let mut ret;
    let mut val: Option<VList> = None;
    if s.is_any_simple_type(typ) || s.is_atomic(typ) {
        normalize!(typ, normalize);
        let bi = if s.it(typ).typ != TYPE_BASIC {
            let mut b = s.it(typ).base_type;
            while let Some(x) = b {
                if s.it(x).typ == TYPE_BASIC {
                    break;
                }
                b = s.it(x).base_type;
            }
            match b {
                Some(b) => b,
                None => {
                    ec.internal_err(
                        "xmlSchemaVCheckCVCSimpleType",
                        "could not get the built-in type",
                    );
                    return (-1, None);
                }
            }
        } else {
            typ
        };
        let bvt = s.it(bi).builtin_type;
        let (r, v) = if bvt == T::NOTATION {
            validate_notation(s, ec, node, &value, val_needed)
        } else if bvt == T::QNAME && ec.validator {
            validate_qname(s, ec, &value, val_needed)
        } else {
            let na = NodeArg {
                node,
                scope: &ec.ns_scope,
            };
            T::val_predef_type_node_no_norm(bvt, &value, val_needed, Some(&na))
        };
        ret = r;
        val = v.map(|x| vec![x]);
        if ret != 0 {
            if ret < 0 {
                ec.internal_err(
                    "xmlSchemaVCheckCVCSimpleType",
                    "validating against a built-in type",
                );
                return (-1, None);
            }
            ret = if s.is_list(typ) {
                E::SCHEMAV_CVC_DATATYPE_VALID_1_2_2
            } else {
                E::SCHEMAV_CVC_DATATYPE_VALID_1_2_1
            };
        }
        if ret == 0 && flags & TYPE_HAS_FACETS != 0 {
            ret = validate_facets(s, ec, node, typ, bvt, &value, val.as_deref(), 0, fire);
            if ret != 0 {
                if ret < 0 {
                    ec.internal_err(
                        "xmlSchemaVCheckCVCSimpleType",
                        "validating facets of atomic simple type",
                    );
                    return (-1, None);
                }
                ret = if s.is_list(typ) {
                    E::SCHEMAV_CVC_DATATYPE_VALID_1_2_2
                } else {
                    E::SCHEMAV_CVC_DATATYPE_VALID_1_2_1
                };
            }
        } else if fire && ret > 0 {
            ec.simple_type_err(s, ret, node, &value, typ, true);
        }
    } else if s.is_list(typ) {
        normalize!(typ, normalize);
        let item_type = match s.it(typ).subtypes {
            Some(t) => t,
            None => {
                ec.internal_err("xmlSchemaVCheckCVCSimpleType", "list type has no item type");
                return (-1, None);
            }
        };
        let mut len = 0u64;
        let mut vals: VList = Vec::new();
        ret = 0;
        let v2 = value.clone();
        for tok in v2.split([' ', '\t', '\n', '\r']).filter(|t| !t.is_empty()) {
            len += 1;
            let (r, cv) =
                check_cvc_simple_type(s, ec, node, item_type, tok, val_needed, fire, false, true);
            if let Some(mut cv) = cv {
                vals.append(&mut cv);
            }
            if r != 0 {
                if r < 0 {
                    ec.internal_err(
                        "xmlSchemaVCheckCVCSimpleType",
                        "validating an item of list simple type",
                    );
                    return (-1, None);
                }
                ret = E::SCHEMAV_CVC_DATATYPE_VALID_1_2_2;
                break;
            }
        }
        if !vals.is_empty() {
            val = Some(vals);
        }
        if ret == 0 && flags & TYPE_HAS_FACETS != 0 {
            ret = validate_facets(
                s,
                ec,
                node,
                typ,
                T::UNKNOWN,
                &value,
                val.as_deref(),
                len,
                fire,
            );
            if ret != 0 {
                if ret < 0 {
                    ec.internal_err(
                        "xmlSchemaVCheckCVCSimpleType",
                        "validating facets of list simple type",
                    );
                    return (-1, None);
                }
                ret = E::SCHEMAV_CVC_DATATYPE_VALID_1_2_2;
            }
        }
        if fire && ret > 0 {
            normalize!(typ, true);
            ec.simple_type_err(s, ret, node, &value, typ, true);
        }
    } else if s.is_union(typ) {
        let members = union_member_types(s, typ);
        if members.is_empty() {
            ec.internal_err(
                "xmlSchemaVCheckCVCSimpleType",
                "union simple type has no member types",
            );
            return (-1, None);
        }
        ret = 0;
        let mut matched = None;
        for &m in &members {
            let (r, v) =
                check_cvc_simple_type(s, ec, node, m, &value, val_needed, false, true, false);
            ret = r;
            if r <= 0 {
                val = v;
                matched = Some(m);
                break;
            }
        }
        if ret != 0 {
            if ret < 0 {
                ec.internal_err(
                    "xmlSchemaVCheckCVCSimpleType",
                    "validating members of union simple type",
                );
                return (-1, None);
            }
            ret = E::SCHEMAV_CVC_DATATYPE_VALID_1_2_3;
        }
        if ret == 0 && flags & TYPE_HAS_FACETS != 0 {
            if let Some(m) = matched {
                if !is_normalized && (normalize || flags & TYPE_NORMVALUENEEDED != 0) {
                    if let Some(n) = normalize_value(s, m, &value) {
                        value = n;
                    }
                }
            }
            ret = validate_facets(
                s,
                ec,
                node,
                typ,
                T::UNKNOWN,
                &value,
                val.as_deref(),
                0,
                fire,
            );
            if ret != 0 {
                if ret < 0 {
                    ec.internal_err(
                        "xmlSchemaVCheckCVCSimpleType",
                        "validating facets of union simple type",
                    );
                    return (-1, None);
                }
                ret = E::SCHEMAV_CVC_DATATYPE_VALID_1_2_3;
            }
        }
        if fire && ret > 0 {
            ec.simple_type_err(s, ret, node, &value, typ, true);
        }
    } else {
        ret = 0;
    }
    if ret == 0 {
        (0, if want_val || val_needed { val } else { None })
    } else {
        (ret, None)
    }
}

/// xmlSchemaValidateQName (validator)
fn validate_qname(
    s: &Schema,
    ec: &mut ErrCtx,
    value: &str,
    val_needed: bool,
) -> (i32, Option<Val>) {
    if T::validate_qname(value, true) != 0 {
        return (E::SCHEMAV_CVC_DATATYPE_VALID_1_2_1, None);
    }
    let stripped = T::collapse_string(value).unwrap_or_else(|| value.to_string());
    let (prefix, local) = match T::split_qname2(&stripped) {
        Some((p, l)) => (Some(p), l),
        None => (None, value.to_string()),
    };
    let ns = ec.ns_scope.lookup(prefix.as_deref());
    if prefix.is_some() && ns.is_none() {
        let ret = E::SCHEMAV_CVC_DATATYPE_VALID_1_2_1;
        let qt = s.builtin(T::QNAME);
        let m = format!(
            "The QName value '{}' has no corresponding namespace declaration in scope",
            value
        );
        ec.custom_err(s, ret, None, Some(qt), &m);
        return (ret, None);
    }
    if val_needed {
        return (
            0,
            Some(Val {
                typ: T::QNAME,
                data: T::ValData::QName {
                    name: local,
                    uri: ns,
                },
            }),
        );
    }
    (0, None)
}

/// xmlSchemaValidateNotation
fn validate_notation(
    s: &Schema,
    ec: &mut ErrCtx,
    node: Option<NodeId>,
    value: &str,
    val_needed: bool,
) -> (i32, Option<Val>) {
    let r = T::validate_qname(value, true);
    if r != 0 {
        return (r, None);
    }
    match T::split_qname2(value) {
        Some((prefix, local)) => {
            let ns = if ec.validator {
                ec.ns_scope.lookup(Some(&prefix))
            } else if let Some(n) = node {
                search_ns(n, Some(&prefix))
            } else {
                return (1, None);
            };
            let ns = match ns {
                None => return (1, None),
                Some(n) => n,
            };
            if s.get_global(TYPE_NOTATION, &local, Some(&ns)).is_some() {
                if val_needed {
                    return (
                        0,
                        Some(Val {
                            typ: T::NOTATION,
                            data: T::ValData::QName {
                                name: local,
                                uri: Some(ns),
                            },
                        }),
                    );
                }
                (0, None)
            } else {
                (1, None)
            }
        }
        None => {
            if s.get_global(TYPE_NOTATION, value, None).is_some() {
                if val_needed {
                    return (
                        0,
                        Some(Val {
                            typ: T::NOTATION,
                            data: T::ValData::QName {
                                name: value.to_string(),
                                uri: None,
                            },
                        }),
                    );
                }
                (0, None)
            } else {
                (1, None)
            }
        }
    }
}
