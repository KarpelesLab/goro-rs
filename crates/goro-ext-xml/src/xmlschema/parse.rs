//! Schema document parsing (the xmlSchemaParse* functions of xmlschemas.c).

use super::*;
use crate::xmlschemastypes as T;
use goro_core::vm::Vm;

/// The schema parser context (xmlSchemaParserCtxt + construction context).
pub struct PCtxt<'a> {
    pub vm: &'a mut Vm,
    pub s: Schema,
    pub errors: Vec<XmlError>,
    pub nberrors: i32,
    pub err: i32,
    pub stop: i32,
    pub target_ns: Option<String>,
    pub bucket: usize,
    pub main_bucket: usize,
    pub pending: Vec<ItemId>,
    pub ctxt_type: Option<ItemId>,
    pub attr_prohibs: Vec<ItemId>,
    pub counter: i32,
    pub url: Option<String>,
    pub is_redefine: bool,
    /// the bucket being redefined (pctxt->redefined)
    pub redefined: Option<usize>,
    /// redefinitions (constructor->redefs) and the current one
    pub redefs: Vec<Redef>,
    pub cur_redef: Option<usize>,
    pub redef_counter: i32,
}

/// xmlSchemaRedef
pub struct Redef {
    pub item: ItemId,
    pub target_bucket: Option<usize>,
    pub ref_name: Option<String>,
    pub ref_ns: Option<String>,
    pub reference: Option<ItemId>,
    pub target: Option<ItemId>,
}

pub fn blank_str(s: &str) -> bool {
    s.bytes()
        .all(|c| c == 0x20 || c == 0x9 || c == 0xA || c == 0xD)
}

fn is_blank_ch(c: u8) -> bool {
    c == 0x20 || c == 0x9 || c == 0xA || c == 0xD
}

impl<'a> PCtxt<'a> {
    pub fn new(vm: &'a mut Vm) -> PCtxt<'a> {
        let mut s = Schema::default();
        s.init_builtins();
        PCtxt {
            vm,
            s,
            errors: Vec::new(),
            nberrors: 0,
            err: 0,
            stop: 0,
            target_ns: None,
            bucket: 0,
            main_bucket: 0,
            pending: Vec::new(),
            ctxt_type: None,
            attr_prohibs: Vec::new(),
            counter: 0,
            url: None,
            is_redefine: false,
            redefined: None,
            redefs: Vec::new(),
            cur_redef: None,
            redef_counter: 0,
        }
    }

    // -----------------------------------------------------------------
    // Errors

    /// xmlSchemaPErrFull
    pub fn perr_full(&mut self, node: Option<NodeId>, code: i32, level: i32, msg: String) {
        if level != ERR_WARNING {
            self.nberrors += 1;
            self.err = code;
        }
        self.errors
            .push(mk_error(E::FROM_SCHEMASP, code, level, msg, node, None, 0));
    }

    /// xmlSchemaPErr
    pub fn perr(&mut self, node: Option<NodeId>, code: i32, msg: String) {
        self.perr_full(node, code, ERR_ERROR, msg);
    }

    /// xmlSchemaPErr2
    pub fn perr2(&mut self, node: Option<NodeId>, child: Option<NodeId>, code: i32, msg: String) {
        self.perr(child.or(node), code, msg);
    }

    /// xmlSchemaFormatNodeForError for the parser
    pub fn format_node(&self, node: Option<NodeId>) -> String {
        match node {
            None => String::new(),
            Some(n) => format_node_for_error(n),
        }
    }

    /// xmlSchemaInternalErr
    pub fn internal_err(&mut self, func: &str, msg: &str) {
        self.perr(
            None,
            E::SCHEMAP_INTERNAL,
            format!("Internal error: {}, {}.\n", func, msg),
        );
    }

    /// xmlSchemaCustomErr4 (parser)
    pub fn custom_err(
        &mut self,
        code: i32,
        node: Option<NodeId>,
        item: Option<ItemId>,
        message: &str,
    ) {
        let mut node = node;
        let mut msg;
        if let (None, Some(it)) = (node, item) {
            node = self.s.component_node(it);
            msg = self.s.format_item_for_report(None, item, None);
            msg.push_str(": ");
        } else {
            msg = self.format_node(node);
        }
        msg.push_str(message);
        msg.push_str(".\n");
        self.perr(node, code, msg);
    }

    /// xmlSchemaCustomWarning (parser)
    pub fn custom_warning(&mut self, code: i32, node: Option<NodeId>, message: &str) {
        let mut msg = self.format_node(node);
        msg.push_str(message);
        msg.push_str(".\n");
        self.perr_full(node, code, ERR_WARNING, msg);
    }

    /// xmlSchemaPMissingAttrErr
    pub fn missing_attr_err(
        &mut self,
        code: i32,
        owner_item: Option<ItemId>,
        owner_elem: Option<NodeId>,
        name: &str,
        message: Option<&str>,
    ) {
        let des = self.s.format_item_for_report(None, owner_item, owner_elem);
        let msg = match message {
            Some(m) => format!("{}: {}.\n", des, m),
            None => format!(
                "{}: The attribute '{}' is required but missing.\n",
                des, name
            ),
        };
        self.perr(owner_elem, code, msg);
    }

    /// xmlSchemaPResCompAttrErr
    pub fn res_comp_attr_err(
        &mut self,
        code: i32,
        owner_item: Option<ItemId>,
        owner_elem: Option<NodeId>,
        name: &str,
        ref_name: Option<&str>,
        ref_uri: Option<&str>,
        ref_type: i32,
        ref_type_str: Option<&str>,
    ) {
        let des = self.s.format_item_for_report(None, owner_item, owner_elem);
        let rts = ref_type_str.unwrap_or_else(|| Schema::item_type_to_str(ref_type));
        let msg = format!(
            "{}, attribute '{}': The QName value '{}' does not resolve to a(n) {}.\n",
            des,
            name,
            format_qname(ref_uri, ref_name),
            rts
        );
        self.perr(owner_elem, code, msg);
    }

    /// xmlSchemaPCustomAttrErr
    pub fn custom_attr_err(
        &mut self,
        code: i32,
        owner_item: Option<ItemId>,
        attr: Option<NodeId>,
        msg: &str,
    ) {
        match attr {
            None => {
                let des = self.s.format_item_for_report(None, owner_item, None);
                self.perr(
                    None,
                    code,
                    format!("{}, attribute '{}': {}.\n", des, "Unknown", msg),
                );
            }
            Some(a) => {
                let parent = node_parent(a);
                let des = self.s.format_item_for_report(None, owner_item, parent);
                let name = node_name(a);
                self.perr(
                    Some(a),
                    code,
                    format!("{}, attribute '{}': {}.\n", des, name, msg),
                );
            }
        }
    }

    /// xmlSchemaPIllegalAttrErr
    pub fn illegal_attr_err(&mut self, code: i32, attr: NodeId) {
        let parent = node_parent(attr);
        let str_a = self.format_node(parent);
        let qn = node_qname(attr);
        self.perr(
            Some(attr),
            code,
            format!("{}The attribute '{}' is not allowed.\n", str_a, qn),
        );
    }

    /// xmlSchemaPCustomErrExt
    pub fn pcustom_err(
        &mut self,
        code: i32,
        item: Option<ItemId>,
        item_elem: Option<NodeId>,
        message: &str,
    ) {
        let des = self.s.format_item_for_report(None, item, item_elem);
        let msg = format!("{}: {}.\n", des, message);
        let mut elem = item_elem;
        if elem.is_none() {
            if let Some(i) = item {
                elem = self.s.component_node(i);
            }
        }
        self.perr(elem, code, msg);
    }

    /// xmlSchemaPAttrUseErr4
    pub fn attr_use_err(
        &mut self,
        code: i32,
        node: Option<NodeId>,
        owner_item: Option<ItemId>,
        attruse: ItemId,
        message: &str,
    ) {
        let mut msg = self.s.format_item_for_report(None, owner_item, None);
        msg.push_str(", ");
        msg.push_str(&self.s.format_item_for_report(None, Some(attruse), None));
        msg.push_str(": ");
        msg.push_str(message);
        msg.push_str(".\n");
        self.perr(node, code, msg);
    }

    /// xmlSchemaPIllegalFacetAtomicErr
    pub fn illegal_facet_atomic_err(
        &mut self,
        code: i32,
        typ: ItemId,
        base: ItemId,
        facet_type: i32,
    ) {
        let node = self.s.it(typ).node;
        let des = self.s.format_item_for_report(None, Some(typ), node);
        let strt = self.s.format_item_for_report(None, Some(base), None);
        self.perr(
            node,
            code,
            format!(
                "{}: The facet '{}' is not allowed on types derived from the type {}.\n",
                des,
                facet_type_to_string(facet_type),
                strt
            ),
        );
    }

    /// xmlSchemaPIllegalFacetListUnionErr
    pub fn illegal_facet_list_union_err(&mut self, code: i32, typ: ItemId, facet_type: i32) {
        let node = self.s.it(typ).node;
        let des = self.s.format_item_for_report(None, Some(typ), node);
        self.perr(
            node,
            code,
            format!(
                "{}: The facet '{}' is not allowed.\n",
                des,
                facet_type_to_string(facet_type)
            ),
        );
    }

    /// xmlSchemaPMutualExclAttrErr
    pub fn mutual_excl_attr_err(
        &mut self,
        code: i32,
        owner_item: Option<ItemId>,
        attr: NodeId,
        name1: &str,
        name2: &str,
    ) {
        let parent = node_parent(attr);
        let des = self.s.format_item_for_report(None, owner_item, parent);
        self.perr(
            Some(attr),
            code,
            format!(
                "{}: The attributes '{}' and '{}' are mutually exclusive.\n",
                des, name1, name2
            ),
        );
    }

    /// xmlSchemaPSimpleTypeErr
    pub fn psimple_type_err(
        &mut self,
        code: i32,
        node: NodeId,
        typ: Option<ItemId>,
        expected: Option<&str>,
        value: Option<&str>,
        message: Option<&str>,
    ) {
        let mut msg = self.format_node(Some(node));
        let is_attr = node_type(node) == NType::Attribute;
        match message {
            None => {
                if let Some(t) = typ {
                    if is_attr {
                        msg.push_str(&format!(
                            "'{}' is not a valid value of ",
                            value.unwrap_or("")
                        ));
                    } else {
                        msg.push_str("The character content is not a valid value of ");
                    }
                    msg.push_str(&self.s.simple_type_designation(t));
                    if self.s.is_global_item(t) {
                        msg.push('.');
                    }
                } else if is_attr {
                    msg.push_str(&format!(
                        "The value '{}' is not valid.",
                        value.unwrap_or("")
                    ));
                } else {
                    msg.push_str("The character content is not valid.");
                }
                if let Some(e) = expected {
                    msg.push_str(" Expected is '");
                    msg.push_str(e);
                    msg.push_str("'.\n");
                } else {
                    msg.push('\n');
                }
                self.perr(Some(node), code, msg);
            }
            Some(m) => {
                msg.push_str(m);
                msg.push_str(".\n");
                self.perr(Some(node), code, msg);
            }
        }
    }

    /// xmlSchemaPContentErr
    pub fn content_err(
        &mut self,
        code: i32,
        owner_item: Option<ItemId>,
        owner_elem: Option<NodeId>,
        child: Option<NodeId>,
        message: Option<&str>,
        content: Option<&str>,
    ) {
        let des = self.s.format_item_for_report(None, owner_item, owner_elem);
        let msg = match (message, content) {
            (Some(m), _) => format!("{}: {}.\n", des, m),
            (None, Some(c)) => format!("{}: The content is not valid. Expected is {}.\n", des, c),
            (None, None) => format!("{}: The content is not valid.\n", des),
        };
        self.perr2(owner_elem, child, code, msg);
    }

    // -----------------------------------------------------------------
    // Item creation

    fn add_global(&mut self, id: ItemId) {
        let b = self.bucket;
        self.s.buckets[b].globals.push(id);
    }

    fn add_local(&mut self, id: ItemId) {
        let b = self.bucket;
        self.s.buckets[b].locals.push(id);
    }

    pub fn add_pending(&mut self, id: ItemId) {
        self.pending.push(id);
    }

    pub fn new_item(&mut self, typ: i32) -> Item {
        Item {
            typ,
            min_occurs: 1,
            max_occurs: 1,
            ..Default::default()
        }
    }

    pub fn add_notation(&mut self, name: &str, ns: Option<String>) -> ItemId {
        let mut it = self.new_item(TYPE_NOTATION);
        it.name = Some(name.to_string());
        it.target_ns = ns;
        let id = self.s.add_item(it);
        self.add_global(id);
        id
    }

    pub fn add_attribute(
        &mut self,
        name: Option<String>,
        ns: Option<String>,
        node: NodeId,
        top: bool,
    ) -> ItemId {
        let mut it = self.new_item(TYPE_ATTRIBUTE);
        it.node = Some(node);
        it.name = name;
        it.target_ns = ns;
        let id = self.s.add_item(it);
        if top {
            self.add_global(id);
        } else {
            self.add_local(id);
        }
        self.add_pending(id);
        id
    }

    pub fn add_attribute_use(&mut self, node: NodeId) -> ItemId {
        let mut it = self.new_item(TYPE_ATTRIBUTE_USE);
        it.node = Some(node);
        let id = self.s.add_item(it);
        self.add_local(id);
        id
    }

    /// xmlSchemaAddRedef
    pub fn add_redef(&mut self, item: ItemId) {
        let (n, ns) = (
            self.s.it(item).name.clone(),
            self.s.it(item).target_ns.clone(),
        );
        self.redefs.push(Redef {
            item,
            target_bucket: self.redefined,
            ref_name: n,
            ref_ns: ns,
            reference: None,
            target: None,
        });
        self.cur_redef = Some(self.redefs.len() - 1);
        self.redef_counter = 0;
    }

    pub fn add_attribute_group_definition(
        &mut self,
        name: &str,
        ns: Option<String>,
        node: NodeId,
    ) -> ItemId {
        let mut it = self.new_item(TYPE_ATTRIBUTEGROUP);
        it.name = Some(name.to_string());
        it.target_ns = ns;
        it.node = Some(node);
        it.flags |= ATTRGROUP_GLOBAL;
        let id = self.s.add_item(it);
        if self.is_redefine {
            self.add_redef(id);
        }
        self.add_global(id);
        self.add_pending(id);
        id
    }

    pub fn add_element(
        &mut self,
        name: &str,
        ns: Option<String>,
        node: NodeId,
        top: bool,
    ) -> ItemId {
        let mut it = self.new_item(TYPE_ELEMENT);
        it.name = Some(name.to_string());
        it.target_ns = ns;
        it.node = Some(node);
        let id = self.s.add_item(it);
        if top {
            self.add_global(id);
        } else {
            self.add_local(id);
        }
        self.add_pending(id);
        id
    }

    pub fn add_type(
        &mut self,
        typ: i32,
        name: Option<String>,
        ns: Option<String>,
        node: NodeId,
        top: bool,
    ) -> ItemId {
        let mut it = self.new_item(typ);
        it.name = name;
        it.target_ns = ns;
        it.node = Some(node);
        let id = self.s.add_item(it);
        if top {
            if self.is_redefine {
                self.add_redef(id);
            }
            self.add_global(id);
        } else {
            self.add_local(id);
        }
        self.add_pending(id);
        id
    }

    pub fn new_qname_ref(
        &mut self,
        ref_type: i32,
        name: Option<String>,
        ns: Option<String>,
    ) -> ItemId {
        let mut it = self.new_item(EXTRA_QNAMEREF);
        it.name = name;
        it.target_ns = ns;
        it.item_type = ref_type;
        let id = self.s.add_item(it);
        self.add_local(id);
        id
    }

    pub fn add_attribute_use_prohib(&mut self) -> ItemId {
        let it = self.new_item(EXTRA_ATTR_USE_PROHIB);
        let id = self.s.add_item(it);
        self.add_local(id);
        id
    }

    pub fn add_model_group(&mut self, typ: i32, node: NodeId) -> ItemId {
        let mut it = self.new_item(typ);
        it.node = Some(node);
        let id = self.s.add_item(it);
        self.add_local(id);
        if typ == TYPE_SEQUENCE || typ == TYPE_CHOICE {
            self.add_pending(id);
        }
        id
    }

    pub fn add_particle(&mut self, node: Option<NodeId>, min: i32, max: i32) -> ItemId {
        let mut it = self.new_item(TYPE_PARTICLE);
        it.node = node;
        it.min_occurs = min;
        it.max_occurs = max;
        let id = self.s.add_item(it);
        self.add_local(id);
        id
    }

    pub fn add_model_group_definition(
        &mut self,
        name: &str,
        ns: Option<String>,
        node: NodeId,
    ) -> ItemId {
        let mut it = self.new_item(TYPE_GROUP);
        it.name = Some(name.to_string());
        it.node = Some(node);
        it.target_ns = ns;
        let id = self.s.add_item(it);
        if self.is_redefine {
            self.add_redef(id);
        }
        self.add_global(id);
        self.add_pending(id);
        id
    }

    pub fn add_idc(
        &mut self,
        name: &str,
        ns: Option<String>,
        category: i32,
        node: NodeId,
    ) -> ItemId {
        let mut it = self.new_item(category);
        it.target_ns = ns;
        it.name = Some(name.to_string());
        it.node = Some(node);
        let id = self.s.add_item(it);
        self.add_global(id);
        if category == TYPE_IDC_KEYREF {
            self.add_pending(id);
        }
        id
    }

    pub fn add_wildcard(&mut self, typ: i32, node: NodeId) -> ItemId {
        let mut it = self.new_item(typ);
        it.node = Some(node);
        let id = self.s.add_item(it);
        self.add_local(id);
        id
    }

    /// xmlSchemaAddElementSubstitutionMember
    pub fn add_subst_member(&mut self, head: ItemId, member: ItemId) {
        let key = (
            self.s.it(head).name.clone().unwrap_or_default(),
            self.s.it(head).target_ns.clone(),
        );
        self.s.subst_groups.entry(key).or_default().push(member);
    }

    // -----------------------------------------------------------------
    // Attribute value utilities

    /// xmlSchemaPValAttrNodeQNameValue: Ok((uri, local)) or Err(code) with
    /// `local` set to the value.
    pub fn pval_attr_node_qname_value(
        &mut self,
        attr: NodeId,
        value: &str,
    ) -> Result<(Option<String>, String), (i32, String)> {
        let ret = T::validate_qname(value, true);
        if ret > 0 {
            let qt = self.s.builtin(T::QNAME);
            self.psimple_type_err(
                E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                attr,
                Some(qt),
                None,
                Some(value),
                None,
            );
            return Err((self.err, value.to_string()));
        }
        let parent = node_parent(attr).unwrap_or(attr);
        if !value.contains(':') {
            let ns = search_ns(parent, None);
            let uri = match ns {
                Some(h) if !h.is_empty() => Some(h),
                _ => {
                    if self.s.flags & INCLUDING_CONVERT_NS != 0 {
                        self.target_ns.clone()
                    } else {
                        None
                    }
                }
            };
            return Ok((uri, value.to_string()));
        }
        // xmlSplitQName3
        let i = value.find(':').unwrap();
        let local = value[i + 1..].to_string();
        let pref = &value[..i];
        match search_ns(parent, Some(pref)) {
            None => {
                let qt = self.s.builtin(T::QNAME);
                let m = format!(
                    "The value '{}' of simple type 'xs:QName' has no corresponding namespace declaration in scope",
                    value
                );
                self.psimple_type_err(
                    E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                    attr,
                    Some(qt),
                    None,
                    Some(value),
                    Some(&m),
                );
                Err((self.err, local))
            }
            Some(h) => Ok((Some(h), local)),
        }
    }

    /// xmlSchemaPValAttrNodeQName
    pub fn pval_attr_node_qname(
        &mut self,
        attr: NodeId,
    ) -> Result<(Option<String>, String), (i32, String)> {
        let v = node_content(attr);
        self.pval_attr_node_qname_value(attr, &v)
    }

    /// xmlSchemaPValAttrQName: Ok(None) if absent
    pub fn pval_attr_qname(
        &mut self,
        owner: NodeId,
        name: &str,
    ) -> Result<Option<(Option<String>, String)>, (i32, String)> {
        match get_prop_node(owner, name) {
            None => Ok(None),
            Some(a) => self.pval_attr_node_qname(a).map(Some),
        }
    }

    /// xmlSchemaPValAttrNodeID
    pub fn pval_attr_node_id(&mut self, attr: NodeId) -> i32 {
        let value = node_content(attr);
        let mut ret = T::validate_ncname(&value, true);
        if ret == 0 {
            let is_id = tree::with(|t| t.nodes[attr].atype == 2);
            if !is_id {
                let v = T::collapse_string(&value).unwrap_or_else(|| value.clone());
                let res = T::TreeNode(attr).add_id(&v);
                use crate::xmlschemastypes::NodeCtx;
                let _ = <T::TreeNode as NodeCtx>::has_doc;
                if res == Some(0) {
                    ret = E::SCHEMAP_S4S_ATTR_INVALID_VALUE;
                    let idt = self.s.builtin(T::ID);
                    let m = format!("Duplicate value '{}' of simple type 'xs:ID'", v);
                    self.psimple_type_err(
                        E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                        attr,
                        Some(idt),
                        None,
                        None,
                        Some(&m),
                    );
                }
            }
        } else if ret > 0 {
            ret = E::SCHEMAP_S4S_ATTR_INVALID_VALUE;
            let idt = self.s.builtin(T::ID);
            let m = format!(
                "The value '{}' of simple type 'xs:ID' is not a valid 'xs:NCName'",
                value
            );
            self.psimple_type_err(
                E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                attr,
                Some(idt),
                None,
                None,
                Some(&m),
            );
        }
        ret
    }

    /// xmlSchemaPValAttrID
    pub fn pval_attr_id(&mut self, owner: NodeId, name: &str) -> i32 {
        match get_prop_node(owner, name) {
            None => 0,
            Some(a) => self.pval_attr_node_id(a),
        }
    }

    fn get_occurs(
        &mut self,
        node: NodeId,
        aname: &str,
        min: i32,
        max: i32,
        def: i32,
        expected: &str,
        allow_unbounded: bool,
    ) -> i32 {
        let attr = match get_prop_node(node, aname) {
            None => return def,
            Some(a) => a,
        };
        let val = node_content(attr);
        if allow_unbounded && val == "unbounded" {
            if max != UNBOUNDED {
                self.psimple_type_err(
                    E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                    attr,
                    None,
                    Some(expected),
                    Some(&val),
                    None,
                );
                return def;
            }
            return UNBOUNDED;
        }
        let b = val.as_bytes();
        let mut i = 0;
        while i < b.len() && is_blank_ch(b[i]) {
            i += 1;
        }
        if i >= b.len() {
            self.psimple_type_err(
                E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                attr,
                None,
                Some(expected),
                Some(&val),
                None,
            );
            return def;
        }
        let mut ret: i32 = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            if ret > i32::MAX / 10 {
                ret = i32::MAX;
            } else {
                let digit = (b[i] - b'0') as i32;
                ret *= 10;
                if ret > i32::MAX - digit {
                    ret = i32::MAX;
                } else {
                    ret += digit;
                }
            }
            i += 1;
        }
        while i < b.len() && is_blank_ch(b[i]) {
            i += 1;
        }
        if i < b.len() || ret < min || (max != -1 && ret > max) {
            self.psimple_type_err(
                E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                attr,
                None,
                Some(expected),
                Some(&val),
                None,
            );
            return def;
        }
        ret
    }

    /// xmlGetMaxOccurs
    pub fn get_max_occurs(
        &mut self,
        node: NodeId,
        min: i32,
        max: i32,
        def: i32,
        expected: &str,
    ) -> i32 {
        self.get_occurs(node, "maxOccurs", min, max, def, expected, true)
    }

    /// xmlGetMinOccurs
    pub fn get_min_occurs(
        &mut self,
        node: NodeId,
        min: i32,
        max: i32,
        def: i32,
        expected: &str,
    ) -> i32 {
        self.get_occurs(node, "minOccurs", min, max, def, expected, false)
    }

    /// xmlSchemaPGetBoolNodeValue
    pub fn pget_bool_node_value(&mut self, node: NodeId) -> bool {
        let v = node_content(node);
        match v.as_str() {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => {
                let bt = self.s.builtin(T::BOOLEAN);
                self.psimple_type_err(
                    E::SCHEMAP_INVALID_BOOLEAN,
                    node,
                    Some(bt),
                    None,
                    Some(&v),
                    None,
                );
                false
            }
        }
    }

    /// xmlGetBooleanProp
    pub fn get_boolean_prop(&mut self, node: NodeId, name: &str, def: bool) -> bool {
        let v = match get_prop(node, name) {
            None => return def,
            Some(v) => v,
        };
        match v.as_str() {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => {
                let bt = self.s.builtin(T::BOOLEAN);
                let a = get_prop_node(node, name).unwrap();
                self.psimple_type_err(
                    E::SCHEMAP_INVALID_BOOLEAN,
                    a,
                    Some(bt),
                    None,
                    Some(&v),
                    None,
                );
                def
            }
        }
    }

    /// xmlSchemaPValAttrNodeValue
    pub fn pval_attr_node_value(&mut self, attr: NodeId, value: &str, vt: i32) -> i32 {
        let ret = match vt {
            T::NCNAME | T::QNAME | T::ANYURI | T::TOKEN | T::LANGUAGE => {
                let n = T::TreeNode(attr);
                T::val_predef_type_node(vt, value, false, Some(&n)).0
            }
            _ => {
                self.internal_err(
                    "xmlSchemaPValAttrNodeValue",
                    "validation using the given type is not supported while parsing a schema",
                );
                return -1;
            }
        };
        if ret < 0 {
            self.internal_err(
                "xmlSchemaPValAttrNodeValue",
                "failed to validate a schema attribute value",
            );
            return -1;
        } else if ret > 0 {
            let t = self.s.builtin(vt);
            let code = if self.s.is_list(t) {
                E::SCHEMAV_CVC_DATATYPE_VALID_1_2_2
            } else {
                E::SCHEMAV_CVC_DATATYPE_VALID_1_2_1
            };
            self.psimple_type_err(code, attr, Some(t), None, Some(value), None);
            return code;
        }
        ret
    }

    /// xmlSchemaPValAttrNode
    pub fn pval_attr_node(&mut self, attr: NodeId, vt: i32) -> (i32, String) {
        let v = node_content(attr);
        let r = self.pval_attr_node_value(attr, &v, vt);
        (r, v)
    }

    /// xmlSchemaPValAttr
    pub fn pval_attr(&mut self, owner: NodeId, name: &str, vt: i32) -> (i32, Option<String>) {
        match get_prop_node(owner, name) {
            None => (0, None),
            Some(a) => {
                let (r, v) = self.pval_attr_node(a, vt);
                (r, Some(v))
            }
        }
    }

    /// xmlSchemaCheckReference
    pub fn check_reference(&mut self, node: NodeId, attr: Option<NodeId>, ns: Option<&str>) -> i32 {
        if self.target_ns.as_deref() == ns {
            return 0;
        }
        if ns == Some(XS_NS) {
            return 0;
        }
        let b = self.bucket;
        for rel in &self.s.buckets[b].relations {
            if (rel.typ == SCHEMA_MAIN || rel.typ == SCHEMA_IMPORT)
                && rel.import_ns.as_deref() == ns
            {
                return 0;
            }
        }
        let n = attr.unwrap_or(node);
        match ns {
            None => self.custom_err(E::SCHEMAP_SRC_RESOLVE, Some(n), None, "References from this schema to components in no namespace are not allowed, since not indicated by an import statement"),
            Some(u) => {
                let m = format!("References from this schema to components in the namespace '{}' are not allowed, since not indicated by an import statement", u);
                self.custom_err(E::SCHEMAP_SRC_RESOLVE, Some(n), None, &m)
            }
        }
        E::SCHEMAP_SRC_RESOLVE
    }

    /// xmlSchemaParseLocalAttributes: parses attribute uses / attribute
    /// group references starting at `child`; returns the next child.
    pub fn parse_local_attributes(
        &mut self,
        child: Option<NodeId>,
        list: &mut Vec<ItemId>,
        parent_type: i32,
        has_refs: &mut bool,
    ) -> Option<NodeId> {
        let mut child = child;
        while is_schema(child, "attribute") || is_schema(child, "attributeGroup") {
            let c = child.unwrap();
            let item = if is_schema(child, "attribute") {
                self.parse_local_attribute(c, list, parent_type)
            } else {
                let r = self.parse_attribute_group_ref(c);
                if r.is_some() {
                    *has_refs = true;
                }
                r
            };
            if let Some(i) = item {
                list.push(i);
            }
            child = next_sibling(c);
        }
        child
    }

    /// Check attributes: no-namespace attributes must be in `allowed`, and
    /// no attribute may be in the XML Schema namespace.
    pub fn check_attrs(&mut self, node: NodeId, allowed: &[&str]) {
        for a in attrs_of(node) {
            match node_ns(a) {
                None => {
                    let n = node_name(a);
                    if !allowed.contains(&n.as_str()) {
                        self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
                    }
                }
                Some(ns) => {
                    if ns == XS_NS {
                        self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
                    }
                }
            }
        }
    }

    /// xmlSchemaParseAnnotation
    pub fn parse_annotation(&mut self, node: NodeId, _needed: bool) {
        for a in attrs_of(node) {
            let ns = node_ns(a);
            if (ns.is_none() && node_name(a) != "id") || ns.as_deref() == Some(XS_NS) {
                self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
            }
        }
        self.pval_attr_id(node, "id");
        let mut barked = false;
        for child in node_children(node) {
            if is_schema(Some(child), "appinfo") {
                for a in attrs_of(child) {
                    let ns = node_ns(a);
                    if (ns.is_none() && node_name(a) != "source") || ns.as_deref() == Some(XS_NS) {
                        self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
                    }
                }
                self.pval_attr(child, "source", T::ANYURI);
            } else if is_schema(Some(child), "documentation") {
                for a in attrs_of(child) {
                    match node_ns(a) {
                        None => {
                            if node_name(a) != "source" {
                                self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
                            }
                        }
                        Some(ns) => {
                            if ns == XS_NS || (node_name(a) == "lang" && ns != XML_XML_NAMESPACE) {
                                self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
                            }
                        }
                    }
                }
                if let Some(a) = get_prop_node_ns(child, XML_XML_NAMESPACE, "lang") {
                    self.pval_attr_node(a, T::LANGUAGE);
                }
            } else {
                if !barked {
                    self.content_err(
                        E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                        None,
                        Some(node),
                        Some(child),
                        None,
                        Some("(appinfo | documentation)*"),
                    );
                }
                barked = true;
            }
        }
    }

    /// xmlSchemaParseFacet
    pub fn parse_facet(&mut self, node: NodeId) -> Option<SFacet> {
        let value = match get_prop(node, "value") {
            None => {
                let n = node_name(node);
                self.perr2(
                    Some(node),
                    None,
                    E::SCHEMAP_FACET_NO_VALUE,
                    format!("Facet {} has no value\n", n),
                );
                return None;
            }
            Some(v) => v,
        };
        let typ = match node_name(node).as_str() {
            "minInclusive" if is_schema(Some(node), "minInclusive") => T::FACET_MININCLUSIVE,
            "minExclusive" if is_schema(Some(node), "minExclusive") => T::FACET_MINEXCLUSIVE,
            "maxInclusive" if is_schema(Some(node), "maxInclusive") => T::FACET_MAXINCLUSIVE,
            "maxExclusive" if is_schema(Some(node), "maxExclusive") => T::FACET_MAXEXCLUSIVE,
            "totalDigits" if is_schema(Some(node), "totalDigits") => T::FACET_TOTALDIGITS,
            "fractionDigits" if is_schema(Some(node), "fractionDigits") => T::FACET_FRACTIONDIGITS,
            "pattern" if is_schema(Some(node), "pattern") => T::FACET_PATTERN,
            "enumeration" if is_schema(Some(node), "enumeration") => T::FACET_ENUMERATION,
            "whiteSpace" if is_schema(Some(node), "whiteSpace") => T::FACET_WHITESPACE,
            "length" if is_schema(Some(node), "length") => T::FACET_LENGTH,
            "maxLength" if is_schema(Some(node), "maxLength") => T::FACET_MAXLENGTH,
            "minLength" if is_schema(Some(node), "minLength") => T::FACET_MINLENGTH,
            _ => {
                let n = node_name(node);
                self.perr2(
                    Some(node),
                    None,
                    E::SCHEMAP_UNKNOWN_FACET_TYPE,
                    format!("Unknown facet type {}\n", n),
                );
                return None;
            }
        };
        self.pval_attr_id(node, "id");
        let mut f = Facet::new(typ, &value);
        if typ != T::FACET_PATTERN && typ != T::FACET_ENUMERATION {
            if let Some(fixed) = get_prop(node, "fixed") {
                if fixed == "true" {
                    f.fixed = true;
                }
            }
        }
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        if child.is_some() {
            let n = node_name(node);
            self.perr2(
                Some(node),
                child,
                E::SCHEMAP_UNKNOWN_FACET_CHILD,
                format!("Facet {} has unexpected child content\n", n),
            );
        }
        Some(SFacet {
            f,
            node: Some(node),
            vals: None,
        })
    }

    /// xmlSchemaParseWildcardNs
    pub fn parse_wildcard_ns(&mut self, wild: ItemId, node: NodeId) -> i32 {
        let mut ret = 0;
        let pc = get_prop(node, "processContents");
        let p = match pc.as_deref() {
            None | Some("strict") => ANY_STRICT,
            Some("skip") => ANY_SKIP,
            Some("lax") => ANY_LAX,
            Some(other) => {
                let o = other.to_string();
                self.psimple_type_err(
                    E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                    node,
                    None,
                    Some("(strict | skip | lax)"),
                    Some(&o),
                    None,
                );
                ret = E::SCHEMAP_S4S_ATTR_INVALID_VALUE;
                ANY_STRICT
            }
        };
        self.s.itm(wild).process_contents = p;
        let attr = get_prop_node(node, "namespace");
        let ns = attr.map(node_content).unwrap_or_default();
        let attr_n = attr.unwrap_or(node);
        if attr.is_none() || ns == "##any" {
            self.s.itm(wild).any = true;
        } else if ns == "##other" {
            let tns = self.target_ns.clone();
            self.s.itm(wild).neg_ns_set = Some(tns);
        } else {
            for item in ns.split([' ', '\t', '\n', '\r']).filter(|s| !s.is_empty()) {
                if item == "##other" || item == "##any" {
                    self.psimple_type_err(E::SCHEMAP_WILDCARD_INVALID_NS_MEMBER, attr_n, None, Some("((##any | ##other) | List of (xs:anyURI | (##targetNamespace | ##local)))"), Some(item), None);
                    ret = E::SCHEMAP_WILDCARD_INVALID_NS_MEMBER;
                } else {
                    let v = if item == "##targetNamespace" {
                        self.target_ns.clone()
                    } else if item == "##local" {
                        None
                    } else {
                        self.pval_attr_node_value(attr_n, item, T::ANYURI);
                        Some(item.to_string())
                    };
                    if !self.s.it(wild).ns_set.contains(&v) {
                        self.s.itm(wild).ns_set.push(v);
                    }
                }
            }
        }
        ret
    }

    /// xmlSchemaPCheckParticleCorrect_2
    pub fn check_particle_correct_2(&mut self, node: NodeId, min: i32, max: i32) -> i32 {
        if max == 0 && min == 0 {
            return 0;
        }
        if max != UNBOUNDED {
            if max < 1 {
                let a = get_prop_node(node, "maxOccurs");
                self.custom_attr_err(
                    E::SCHEMAP_P_PROPS_CORRECT_2_2,
                    None,
                    a,
                    "The value must be greater than or equal to 1",
                );
                return E::SCHEMAP_P_PROPS_CORRECT_2_2;
            } else if min > max {
                let a = get_prop_node(node, "minOccurs");
                self.custom_attr_err(
                    E::SCHEMAP_P_PROPS_CORRECT_2_1,
                    None,
                    a,
                    "The value must not be greater than the value of 'maxOccurs'",
                );
                return E::SCHEMAP_P_PROPS_CORRECT_2_1;
            }
        }
        0
    }

    /// xmlSchemaParseAny
    pub fn parse_any(&mut self, node: NodeId) -> Option<ItemId> {
        self.check_attrs(
            node,
            &[
                "id",
                "minOccurs",
                "maxOccurs",
                "namespace",
                "processContents",
            ],
        );
        self.pval_attr_id(node, "id");
        let max = self.get_max_occurs(node, 0, UNBOUNDED, 1, "(xs:nonNegativeInteger | unbounded)");
        let min = self.get_min_occurs(node, 0, -1, 1, "xs:nonNegativeInteger");
        self.check_particle_correct_2(node, min, max);
        let wild = self.add_wildcard(TYPE_ANY, node);
        self.parse_wildcard_ns(wild, node);
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        if child.is_some() {
            self.content_err(
                E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                None,
                Some(node),
                child,
                None,
                Some("(annotation?)"),
            );
        }
        if min == 0 && max == 0 {
            return None;
        }
        let p = self.add_particle(Some(node), min, max);
        self.s.itm(p).children = Some(wild);
        Some(p)
    }

    /// xmlSchemaParseNotation
    pub fn parse_notation(&mut self, node: NodeId) -> Option<ItemId> {
        let name = match get_prop(node, "name") {
            None => {
                self.perr2(
                    Some(node),
                    None,
                    E::SCHEMAP_NOTATION_NO_NAME,
                    "Notation has no name\n".into(),
                );
                return None;
            }
            Some(n) => n,
        };
        let tns = self.target_ns.clone();
        let ret = self.add_notation(&name, tns);
        self.pval_attr_id(node, "id");
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        if child.is_some() {
            self.content_err(
                E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                None,
                Some(node),
                child,
                None,
                Some("(annotation?)"),
            );
        }
        Some(ret)
    }

    /// xmlSchemaParseAnyAttribute
    pub fn parse_any_attribute(&mut self, node: NodeId) -> Option<ItemId> {
        let ret = self.add_wildcard(TYPE_ANY_ATTRIBUTE, node);
        self.check_attrs(node, &["id", "namespace", "processContents"]);
        self.pval_attr_id(node, "id");
        if self.parse_wildcard_ns(ret, node) != 0 {
            return None;
        }
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        if child.is_some() {
            self.content_err(
                E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                None,
                Some(node),
                child,
                None,
                Some("(annotation?)"),
            );
        }
        Some(ret)
    }
}

pub const XML_XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

pub fn first_child(n: NodeId) -> Option<NodeId> {
    tree::with(|t| t.nodes[n].children)
}

pub fn next_sibling(n: NodeId) -> Option<NodeId> {
    tree::with(|t| t.nodes[n].next)
}

/// xmlSchemaFormatNodeForError for tree nodes ("Element '...': " or
/// "Element '...', attribute '...': ").
pub fn format_node_for_error(n: NodeId) -> String {
    match node_type(n) {
        NType::Attribute => {
            let elem = node_parent(n).unwrap_or(n);
            format!(
                "Element '{}', attribute '{}': ",
                node_qname(elem),
                node_qname(n)
            )
        }
        NType::Element => format!("Element '{}': ", node_qname(n)),
        _ => String::new(),
    }
}
