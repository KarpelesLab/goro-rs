//! Parsing of type definitions and model groups (xmlschemas.c).

use super::parse::{PCtxt, first_child, next_sibling};
use super::parse_comp::pval_attr_block_final;
use super::*;
use crate::xmlschemastypes as T;

fn is_facet_elem(n: Option<NodeId>) -> bool {
    [
        "minInclusive",
        "minExclusive",
        "maxInclusive",
        "maxExclusive",
        "totalDigits",
        "fractionDigits",
        "pattern",
        "enumeration",
        "whiteSpace",
        "length",
        "maxLength",
        "minLength",
    ]
    .iter()
    .any(|f| is_schema(n, f))
}

impl<'a> PCtxt<'a> {
    /// xmlSchemaParseUnion
    pub fn parse_union(&mut self, node: NodeId) -> i32 {
        let typ = self.ctxt_type.unwrap();
        self.s.itm(typ).flags |= TYPE_VARIETY_UNION;
        let ast = self.s.builtin(T::ANYSIMPLETYPE);
        self.s.itm(typ).base_type = Some(ast);
        self.check_attrs(node, &["id", "memberTypes"]);
        self.pval_attr_id(node, "id");
        let attr = get_prop_node(node, "memberTypes");
        if let Some(a) = attr {
            let cur = node_content(a);
            self.s.itm(typ).base = Some(cur.clone());
            for tmp in cur.split([' ', '\t', '\n', '\r']).filter(|s| !s.is_empty()) {
                if let Ok((ns, local)) = self.pval_attr_node_qname_value(a, tmp) {
                    let r = self.new_qname_ref(TYPE_SIMPLE, Some(local), ns);
                    self.s.itm(typ).member_types.push(r);
                }
            }
        }
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        if is_schema(child, "simpleType") {
            let mut last: Option<ItemId> = None;
            while is_schema(child, "simpleType") {
                if let Some(sub) = self.parse_simple_type(child.unwrap(), false) {
                    match last {
                        None => self.s.itm(typ).subtypes = Some(sub),
                        Some(l) => self.s.itm(l).next = Some(sub),
                    }
                    last = Some(sub);
                    self.s.itm(sub).next = None;
                }
                child = next_sibling(child.unwrap());
            }
        }
        if child.is_some() {
            self.content_err(
                E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                None,
                Some(node),
                child,
                None,
                Some("(annotation?, simpleType*)"),
            );
        }
        if attr.is_none() && self.s.it(typ).subtypes.is_none() {
            self.pcustom_err(E::SCHEMAP_SRC_UNION_MEMBERTYPES_OR_SIMPLETYPES, None, Some(node), "Either the attribute 'memberTypes' or at least one <simpleType> child must be present");
        }
        0
    }

    /// xmlSchemaParseList
    pub fn parse_list(&mut self, node: NodeId) {
        let typ = self.ctxt_type.unwrap();
        self.s.itm(typ).flags |= TYPE_VARIETY_LIST;
        let ast = self.s.builtin(T::ANYSIMPLETYPE);
        self.s.itm(typ).base_type = Some(ast);
        self.check_attrs(node, &["id", "itemType"]);
        self.pval_attr_id(node, "id");
        match self.pval_attr_qname(node, "itemType") {
            Ok(Some((u, l))) => {
                self.s.itm(typ).base_ns = u;
                self.s.itm(typ).base = Some(l);
            }
            Ok(None) => {}
            Err((_, l)) => self.s.itm(typ).base = Some(l),
        }
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        if is_schema(child, "simpleType") {
            if self.s.it(typ).base.is_some() {
                self.pcustom_err(
                    E::SCHEMAP_SRC_SIMPLE_TYPE_1,
                    None,
                    Some(node),
                    "The attribute 'itemType' and the <simpleType> child are mutually exclusive",
                );
            } else {
                let st = self.parse_simple_type(child.unwrap(), false);
                self.s.itm(typ).subtypes = st;
            }
            child = next_sibling(child.unwrap());
        } else if self.s.it(typ).base.is_none() {
            self.pcustom_err(
                E::SCHEMAP_SRC_SIMPLE_TYPE_1,
                None,
                Some(node),
                "Either the attribute 'itemType' or the <simpleType> child must be present",
            );
        }
        if child.is_some() {
            self.content_err(
                E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                None,
                Some(node),
                child,
                None,
                Some("(annotation?, simpleType?)"),
            );
        }
        if self.s.it(typ).base.is_none()
            && self.s.it(typ).subtypes.is_none()
            && get_prop_node(node, "itemType").is_none()
        {
            self.pcustom_err(
                E::SCHEMAP_SRC_SIMPLE_TYPE_1,
                None,
                Some(node),
                "Either the attribute 'itemType' or the <simpleType> child must be present",
            );
        }
    }

    /// xmlSchemaParseSimpleType
    pub fn parse_simple_type(&mut self, node: NodeId, top_level: bool) -> Option<ItemId> {
        let mut attr_value = None;
        if top_level {
            match get_prop_node(node, "name") {
                None => {
                    self.missing_attr_err(
                        E::SCHEMAP_S4S_ATTR_MISSING,
                        None,
                        Some(node),
                        "name",
                        None,
                    );
                    return None;
                }
                Some(a) => {
                    let (r, v) = self.pval_attr_node(a, T::NCNAME);
                    if r != 0 {
                        return None;
                    }
                    attr_value = Some(v);
                }
            }
        }
        let tns = self.target_ns.clone();
        let typ;
        if !top_level {
            typ = self.add_type(TYPE_SIMPLE, None, tns.clone(), node, false);
            self.s.itm(typ).content_type = CONTENT_SIMPLE;
            self.check_attrs(node, &["id"]);
        } else {
            typ = self.add_type(TYPE_SIMPLE, attr_value, tns.clone(), node, true);
            self.s.itm(typ).content_type = CONTENT_SIMPLE;
            self.s.itm(typ).flags |= TYPE_GLOBAL;
            self.check_attrs(node, &["id", "name", "final"]);
            match get_prop_node(node, "final") {
                None => {
                    if self.s.flags & FINAL_DEFAULT_RESTRICTION != 0 {
                        self.s.itm(typ).flags |= TYPE_FINAL_RESTRICTION;
                    }
                    if self.s.flags & FINAL_DEFAULT_LIST != 0 {
                        self.s.itm(typ).flags |= TYPE_FINAL_LIST;
                    }
                    if self.s.flags & FINAL_DEFAULT_UNION != 0 {
                        self.s.itm(typ).flags |= TYPE_FINAL_UNION;
                    }
                }
                Some(a) => {
                    let v = node_content(a);
                    let mut fl = self.s.it(typ).flags;
                    if pval_attr_block_final(
                        &v,
                        &mut fl,
                        -1,
                        -1,
                        TYPE_FINAL_RESTRICTION,
                        -1,
                        TYPE_FINAL_LIST,
                        TYPE_FINAL_UNION,
                    ) != 0
                    {
                        self.psimple_type_err(
                            E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                            a,
                            None,
                            Some("(#all | List of (list | union | restriction)"),
                            Some(&v),
                            None,
                        );
                    }
                    self.s.itm(typ).flags = fl;
                }
            }
        }
        self.s.itm(typ).target_ns = tns;
        self.pval_attr_id(node, "id");
        let old = self.ctxt_type;
        self.ctxt_type = Some(typ);
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        let mut has_restriction = false;
        if child.is_none() {
            self.content_err(
                E::SCHEMAP_S4S_ELEM_MISSING,
                None,
                Some(node),
                child,
                None,
                Some("(annotation?, (restriction | list | union))"),
            );
        } else if is_schema(child, "restriction") {
            self.parse_restriction(child.unwrap(), TYPE_SIMPLE);
            has_restriction = true;
            child = next_sibling(child.unwrap());
        } else if is_schema(child, "list") {
            self.parse_list(child.unwrap());
            child = next_sibling(child.unwrap());
        } else if is_schema(child, "union") {
            self.parse_union(child.unwrap());
            child = next_sibling(child.unwrap());
        }
        if child.is_some() {
            self.content_err(
                E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                None,
                Some(node),
                child,
                None,
                Some("(annotation?, (restriction | list | union))"),
            );
        }
        if top_level && self.is_redefine && !has_restriction {
            self.pcustom_err(
                E::SCHEMAP_SRC_REDEFINE,
                None,
                Some(node),
                "This is a redefinition, thus the <simpleType> must have a <restriction> child",
            );
        }
        self.ctxt_type = old;
        Some(typ)
    }

    /// xmlSchemaParseModelGroupDefRef
    pub fn parse_model_group_def_ref(&mut self, node: NodeId) -> Option<ItemId> {
        let attr = match get_prop_node(node, "ref") {
            None => {
                self.missing_attr_err(E::SCHEMAP_S4S_ATTR_MISSING, None, Some(node), "ref", None);
                return None;
            }
            Some(a) => a,
        };
        let (ref_ns, rname) = match self.pval_attr_node_qname(attr) {
            Ok(v) => v,
            Err(_) => return None,
        };
        self.check_reference(node, Some(attr), ref_ns.as_deref());
        let min = self.get_min_occurs(node, 0, -1, 1, "xs:nonNegativeInteger");
        let max = self.get_max_occurs(node, 0, UNBOUNDED, 1, "(xs:nonNegativeInteger | unbounded)");
        self.check_attrs(node, &["ref", "id", "minOccurs", "maxOccurs"]);
        self.pval_attr_id(node, "id");
        let item = self.add_particle(Some(node), min, max);
        let r = self.new_qname_ref(TYPE_GROUP, Some(rname), ref_ns);
        self.s.itm(item).children = Some(r);
        self.check_particle_correct_2(node, min, max);
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
        Some(item)
    }

    /// xmlSchemaParseModelGroupDefinition
    pub fn parse_model_group_definition(&mut self, node: NodeId) -> Option<ItemId> {
        let attr = match get_prop_node(node, "name") {
            None => {
                self.missing_attr_err(E::SCHEMAP_S4S_ATTR_MISSING, None, Some(node), "name", None);
                return None;
            }
            Some(a) => a,
        };
        let (r, name) = self.pval_attr_node(attr, T::NCNAME);
        if r != 0 {
            return None;
        }
        let tns = self.target_ns.clone();
        let item = self.add_model_group_definition(&name, tns, node);
        self.check_attrs(node, &["name", "id"]);
        self.pval_attr_id(node, "id");
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        for (n, t) in [
            ("all", TYPE_ALL),
            ("choice", TYPE_CHOICE),
            ("sequence", TYPE_SEQUENCE),
        ] {
            if is_schema(child, n) {
                let g = self.parse_model_group(child.unwrap(), t, false);
                self.s.itm(item).children = g;
                child = next_sibling(child.unwrap());
                break;
            }
        }
        if child.is_some() {
            self.content_err(
                E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                None,
                Some(node),
                child,
                None,
                Some("(annotation?, (all | choice | sequence)?)"),
            );
        }
        Some(item)
    }

    /// xmlSchemaParseModelGroup
    pub fn parse_model_group(
        &mut self,
        node: NodeId,
        typ: i32,
        with_particle: bool,
    ) -> Option<ItemId> {
        let item = self.add_model_group(typ, node);
        let mut min = 1;
        let mut max = 1;
        let mut has_refs = 0;
        let mut particle = None;
        if with_particle {
            if typ == TYPE_ALL {
                min = self.get_min_occurs(node, 0, 1, 1, "(0 | 1)");
                max = self.get_max_occurs(node, 1, 1, 1, "1");
            } else {
                min = self.get_min_occurs(node, 0, -1, 1, "xs:nonNegativeInteger");
                max = self.get_max_occurs(
                    node,
                    0,
                    UNBOUNDED,
                    1,
                    "(xs:nonNegativeInteger | unbounded)",
                );
            }
            self.check_particle_correct_2(node, min, max);
            let p = self.add_particle(Some(node), min, max);
            self.s.itm(p).children = Some(item);
            particle = Some(p);
            self.check_attrs(node, &["id", "maxOccurs", "minOccurs"]);
        } else {
            self.check_attrs(node, &["id"]);
        }
        self.pval_attr_id(node, "id");
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        let mut last: Option<ItemId> = None;
        if typ == TYPE_ALL {
            while is_schema(child, "element") {
                let mut is_elem_ref = false;
                let part = self.parse_element(child.unwrap(), &mut is_elem_ref, false);
                if let Some(p) = part {
                    if is_elem_ref {
                        has_refs += 1;
                    }
                    if self.s.it(p).min_occurs > 1 {
                        self.pcustom_err(
                            E::SCHEMAP_COS_ALL_LIMITED,
                            None,
                            child,
                            "Invalid value for minOccurs (must be 0 or 1)",
                        );
                        self.s.itm(p).min_occurs = 1;
                    }
                    if self.s.it(p).max_occurs > 1 {
                        self.pcustom_err(
                            E::SCHEMAP_COS_ALL_LIMITED,
                            None,
                            child,
                            "Invalid value for maxOccurs (must be 0 or 1)",
                        );
                        self.s.itm(p).max_occurs = 1;
                    }
                    match last {
                        None => self.s.itm(item).children = Some(p),
                        Some(l) => self.s.itm(l).next = Some(p),
                    }
                    last = Some(p);
                }
                child = next_sibling(child.unwrap());
            }
            if child.is_some() {
                self.content_err(
                    E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                    None,
                    Some(node),
                    child,
                    None,
                    Some("(annotation?, (annotation?, element*)"),
                );
            }
        } else {
            while is_schema(child, "element")
                || is_schema(child, "group")
                || is_schema(child, "any")
                || is_schema(child, "choice")
                || is_schema(child, "sequence")
            {
                let c = child.unwrap();
                let part = if is_schema(child, "element") {
                    let mut is_elem_ref = false;
                    let p = self.parse_element(c, &mut is_elem_ref, false);
                    if p.is_some() && is_elem_ref {
                        has_refs += 1;
                    }
                    p
                } else if is_schema(child, "group") {
                    let mut p = self.parse_model_group_def_ref(c);
                    if p.is_some() {
                        has_refs += 1;
                    }
                    if let (true, Some(ri), Some(part)) = (self.is_redefine, self.cur_redef, p) {
                        let refi = self.s.it(part).children;
                        let rd_item = self.redefs[ri].item;
                        if self.s.it(rd_item).typ == TYPE_GROUP {
                            if let Some(r) = refi {
                                let (rn, rns) =
                                    (self.s.it(r).name.clone(), self.s.it(r).target_ns.clone());
                                if rn == self.redefs[ri].ref_name && rns == self.redefs[ri].ref_ns {
                                    let q = format_qname(
                                        self.redefs[ri].ref_ns.as_deref(),
                                        self.redefs[ri].ref_name.as_deref(),
                                    );
                                    if self.redef_counter != 0 {
                                        let m = format!(
                                            "The redefining model group definition '{}' must not contain more than one reference to the redefined definition",
                                            q
                                        );
                                        self.custom_err(E::SCHEMAP_SRC_REDEFINE, Some(c), None, &m);
                                        p = None;
                                    } else if self.s.it(part).min_occurs != 1
                                        || self.s.it(part).max_occurs != 1
                                    {
                                        let m = format!(
                                            "The redefining model group definition '{}' must not contain a reference to the redefined definition with a maxOccurs/minOccurs other than 1",
                                            q
                                        );
                                        self.custom_err(E::SCHEMAP_SRC_REDEFINE, Some(c), None, &m);
                                        p = None;
                                    }
                                    self.redefs[ri].reference = p;
                                    self.redef_counter += 1;
                                }
                            }
                        }
                    }
                    p
                } else if is_schema(child, "any") {
                    self.parse_any(c)
                } else if is_schema(child, "choice") {
                    self.parse_model_group(c, TYPE_CHOICE, true)
                } else {
                    self.parse_model_group(c, TYPE_SEQUENCE, true)
                };
                if let Some(p) = part {
                    match last {
                        None => self.s.itm(item).children = Some(p),
                        Some(l) => self.s.itm(l).next = Some(p),
                    }
                    last = Some(p);
                }
                child = next_sibling(c);
            }
            if child.is_some() {
                self.content_err(
                    E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                    None,
                    Some(node),
                    child,
                    None,
                    Some("(annotation?, (element | group | choice | sequence | any)*)"),
                );
            }
        }
        if max == 0 && min == 0 {
            return None;
        }
        if has_refs > 0 {
            self.add_pending(item);
        }
        if with_particle { particle } else { Some(item) }
    }

    fn parse_model_group_child(
        &mut self,
        child: &mut Option<NodeId>,
        with_group: bool,
    ) -> Option<Option<ItemId>> {
        for (n, t) in [
            ("all", TYPE_ALL),
            ("choice", TYPE_CHOICE),
            ("sequence", TYPE_SEQUENCE),
        ] {
            if is_schema(*child, n) {
                let g = self.parse_model_group(child.unwrap(), t, true);
                *child = next_sibling(child.unwrap());
                return Some(g);
            }
        }
        if with_group && is_schema(*child, "group") {
            let g = self.parse_model_group_def_ref(child.unwrap());
            *child = next_sibling(child.unwrap());
            return Some(g);
        }
        None
    }

    /// xmlSchemaParseRestriction
    pub fn parse_restriction(&mut self, node: NodeId, parent_type: i32) {
        let typ = self.ctxt_type.unwrap();
        self.s.itm(typ).flags |= TYPE_DERIVATION_METHOD_RESTRICTION;
        self.check_attrs(node, &["id", "base"]);
        self.pval_attr_id(node, "id");
        match self.pval_attr_qname(node, "base") {
            Ok(r) => {
                if let Some((u, l)) = r {
                    self.s.itm(typ).base_ns = u;
                    self.s.itm(typ).base = Some(l);
                }
                if self.s.it(typ).base.is_none() && self.s.it(typ).typ == TYPE_COMPLEX {
                    self.missing_attr_err(
                        E::SCHEMAP_S4S_ATTR_MISSING,
                        None,
                        Some(node),
                        "base",
                        None,
                    );
                } else if self.is_redefine && self.s.it(typ).flags & TYPE_GLOBAL != 0 {
                    if self.s.it(typ).base.is_none() {
                        self.missing_attr_err(
                            E::SCHEMAP_S4S_ATTR_MISSING,
                            None,
                            Some(node),
                            "base",
                            None,
                        );
                    } else if self.s.it(typ).base != self.s.it(typ).name
                        || self.s.it(typ).base_ns != self.s.it(typ).target_ns
                    {
                        let m = format!(
                            "This is a redefinition, but the QName value '{}' of the 'base' attribute does not match the type's designation '{}'",
                            format_qname(
                                self.s.it(typ).base_ns.as_deref(),
                                self.s.it(typ).base.as_deref()
                            ),
                            format_qname(
                                self.s.it(typ).target_ns.as_deref(),
                                self.s.it(typ).name.as_deref()
                            )
                        );
                        self.pcustom_err(E::SCHEMAP_SRC_REDEFINE, None, Some(node), &m);
                        self.s.itm(typ).base = None;
                        self.s.itm(typ).base_ns = None;
                    }
                }
            }
            Err((_, l)) => self.s.itm(typ).base = Some(l),
        }
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        if parent_type == TYPE_SIMPLE {
            if is_schema(child, "simpleType") {
                if self.s.it(typ).base.is_some() {
                    self.content_err(E::SCHEMAP_SRC_RESTRICTION_BASE_OR_SIMPLETYPE, None, Some(node), child, Some("The attribute 'base' and the <simpleType> child are mutually exclusive"), None);
                } else {
                    let st = self.parse_simple_type(child.unwrap(), false);
                    self.s.itm(typ).base_type = st;
                }
                child = next_sibling(child.unwrap());
            } else if self.s.it(typ).base.is_none() {
                self.content_err(
                    E::SCHEMAP_SRC_RESTRICTION_BASE_OR_SIMPLETYPE,
                    None,
                    Some(node),
                    child,
                    Some("Either the attribute 'base' or a <simpleType> child must be present"),
                    None,
                );
            }
        } else if parent_type == TYPE_COMPLEX_CONTENT {
            if let Some(g) = self.parse_model_group_child(&mut child, true) {
                self.s.itm(typ).subtypes = g;
            }
        } else if parent_type == TYPE_SIMPLE_CONTENT && is_schema(child, "simpleType") {
            let st = self.parse_simple_type(child.unwrap(), false);
            self.s.itm(typ).content_type_def = st;
            if st.is_none() {
                return;
            }
            child = next_sibling(child.unwrap());
        }
        if parent_type == TYPE_SIMPLE || parent_type == TYPE_SIMPLE_CONTENT {
            while is_facet_elem(child) {
                if let Some(f) = self.parse_facet(child.unwrap()) {
                    self.s.itm(typ).facets.push(f);
                }
                child = next_sibling(child.unwrap());
            }
            let n = self.s.it(typ).facets.len();
            for i in 0..n {
                self.s.itm(typ).facet_set.push((typ, i));
            }
        }
        if self.s.it(typ).typ == TYPE_COMPLEX {
            let mut list = std::mem::take(&mut self.s.itm(typ).attr_uses);
            let mut dummy = false;
            child = self.parse_local_attributes(child, &mut list, TYPE_RESTRICTION, &mut dummy);
            self.s.itm(typ).attr_uses = list;
            self.s.itm(typ).has_attr_uses = true;
            if is_schema(child, "anyAttribute") {
                let w = self.parse_any_attribute(child.unwrap());
                self.s.itm(typ).attribute_wildcard = w;
                child = next_sibling(child.unwrap());
            }
        }
        if child.is_some() {
            let c = if parent_type == TYPE_COMPLEX_CONTENT {
                "annotation?, (group | all | choice | sequence)?, ((attribute | attributeGroup)*, anyAttribute?))"
            } else if parent_type == TYPE_SIMPLE_CONTENT {
                "(annotation?, (simpleType?, (minExclusive | minInclusive | maxExclusive | maxInclusive | totalDigits | fractionDigits | length | minLength | maxLength | enumeration | whiteSpace | pattern)*)?, ((attribute | attributeGroup)*, anyAttribute?))"
            } else {
                "(annotation?, (simpleType?, (minExclusive | minInclusive | maxExclusive | maxInclusive | totalDigits | fractionDigits | length | minLength | maxLength | enumeration | whiteSpace | pattern)*))"
            };
            self.content_err(
                E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                None,
                Some(node),
                child,
                None,
                Some(c),
            );
        }
    }

    /// xmlSchemaParseExtension
    pub fn parse_extension(&mut self, node: NodeId, parent_type: i32) {
        let typ = self.ctxt_type.unwrap();
        self.s.itm(typ).flags |= TYPE_DERIVATION_METHOD_EXTENSION;
        self.check_attrs(node, &["id", "base"]);
        self.pval_attr_id(node, "id");
        match self.pval_attr_qname(node, "base") {
            Ok(Some((u, l))) => {
                self.s.itm(typ).base_ns = u;
                self.s.itm(typ).base = Some(l);
            }
            Ok(None) => {
                self.missing_attr_err(E::SCHEMAP_S4S_ATTR_MISSING, None, Some(node), "base", None);
            }
            Err((_, l)) => self.s.itm(typ).base = Some(l),
        }
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        if parent_type == TYPE_COMPLEX_CONTENT {
            if let Some(g) = self.parse_model_group_child(&mut child, true) {
                self.s.itm(typ).subtypes = g;
            }
        }
        if child.is_some() {
            let mut list = std::mem::take(&mut self.s.itm(typ).attr_uses);
            let mut dummy = false;
            child = self.parse_local_attributes(child, &mut list, TYPE_EXTENSION, &mut dummy);
            self.s.itm(typ).attr_uses = list;
            self.s.itm(typ).has_attr_uses = true;
            if is_schema(child, "anyAttribute") {
                let w = self.parse_any_attribute(child.unwrap());
                self.s.itm(typ).attribute_wildcard = w;
                child = next_sibling(child.unwrap());
            }
        }
        if child.is_some() {
            let c = if parent_type == TYPE_COMPLEX_CONTENT {
                "(annotation?, ((group | all | choice | sequence)?, ((attribute | attributeGroup)*, anyAttribute?)))"
            } else {
                "(annotation?, ((attribute | attributeGroup)*, anyAttribute?))"
            };
            self.content_err(
                E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                None,
                Some(node),
                child,
                None,
                Some(c),
            );
        }
    }

    /// xmlSchemaParseSimpleContent / xmlSchemaParseComplexContent
    fn parse_content(&mut self, node: NodeId, simple: bool) -> bool {
        let typ = self.ctxt_type.unwrap();
        if simple {
            self.s.itm(typ).content_type = CONTENT_SIMPLE;
            self.check_attrs(node, &["id"]);
        } else {
            self.check_attrs(node, &["id", "mixed"]);
        }
        self.pval_attr_id(node, "id");
        if !simple && self.get_boolean_prop(node, "mixed", false) {
            self.s.itm(typ).flags |= TYPE_MIXED;
        }
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        if child.is_none() {
            // reported twice by libxml2
            self.content_err(
                E::SCHEMAP_S4S_ELEM_MISSING,
                None,
                Some(node),
                None,
                None,
                Some("(annotation?, (restriction | extension))"),
            );
            self.content_err(
                E::SCHEMAP_S4S_ELEM_MISSING,
                None,
                Some(node),
                None,
                None,
                Some("(annotation?, (restriction | extension))"),
            );
        }
        let pt = if simple {
            TYPE_SIMPLE_CONTENT
        } else {
            TYPE_COMPLEX_CONTENT
        };
        let mut has = false;
        if is_schema(child, "restriction") {
            self.parse_restriction(child.unwrap(), pt);
            has = true;
            child = next_sibling(child.unwrap());
        } else if is_schema(child, "extension") {
            self.parse_extension(child.unwrap(), pt);
            has = true;
            child = next_sibling(child.unwrap());
        }
        if child.is_some() {
            self.content_err(
                E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                None,
                Some(node),
                child,
                None,
                Some("(annotation?, (restriction | extension))"),
            );
        }
        has
    }

    /// xmlSchemaParseComplexType
    pub fn parse_complex_type(&mut self, node: NodeId, top_level: bool) -> Option<ItemId> {
        let ctxt_type = self.ctxt_type;
        let mut name = None;
        if top_level {
            match get_prop_node(node, "name") {
                None => {
                    self.missing_attr_err(
                        E::SCHEMAP_S4S_ATTR_MISSING,
                        None,
                        Some(node),
                        "name",
                        None,
                    );
                    return None;
                }
                Some(a) => {
                    let (r, v) = self.pval_attr_node(a, T::NCNAME);
                    if r != 0 {
                        return None;
                    }
                    name = Some(v);
                }
            }
        }
        let tns = self.target_ns.clone();
        let typ = if !top_level {
            self.add_type(TYPE_COMPLEX, None, tns.clone(), node, false)
        } else {
            let t = self.add_type(TYPE_COMPLEX, name, tns.clone(), node, true);
            self.s.itm(t).flags |= TYPE_GLOBAL;
            t
        };
        self.s.itm(typ).target_ns = tns;
        let mut final_ = false;
        let mut block = false;
        for a in attrs_of(node) {
            match node_ns(a) {
                None => {
                    let an = node_name(a);
                    if an == "id" {
                        self.pval_attr_id(node, "id");
                    } else if an == "mixed" {
                        if self.pget_bool_node_value(a) {
                            self.s.itm(typ).flags |= TYPE_MIXED;
                        }
                    } else if top_level {
                        if an == "name" {
                        } else if an == "abstract" {
                            if self.pget_bool_node_value(a) {
                                self.s.itm(typ).flags |= TYPE_ABSTRACT;
                            }
                        } else if an == "final" {
                            let v = node_content(a);
                            let mut fl = self.s.it(typ).flags;
                            if pval_attr_block_final(
                                &v,
                                &mut fl,
                                -1,
                                TYPE_FINAL_EXTENSION,
                                TYPE_FINAL_RESTRICTION,
                                -1,
                                -1,
                                -1,
                            ) != 0
                            {
                                self.psimple_type_err(
                                    E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                                    a,
                                    None,
                                    Some("(#all | List of (extension | restriction))"),
                                    Some(&v),
                                    None,
                                );
                            } else {
                                final_ = true;
                            }
                            self.s.itm(typ).flags = fl;
                        } else if an == "block" {
                            let v = node_content(a);
                            let mut fl = self.s.it(typ).flags;
                            if pval_attr_block_final(
                                &v,
                                &mut fl,
                                -1,
                                TYPE_BLOCK_EXTENSION,
                                TYPE_BLOCK_RESTRICTION,
                                -1,
                                -1,
                                -1,
                            ) != 0
                            {
                                self.psimple_type_err(
                                    E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                                    a,
                                    None,
                                    Some("(#all | List of (extension | restriction)) "),
                                    Some(&v),
                                    None,
                                );
                            } else {
                                block = true;
                            }
                            self.s.itm(typ).flags = fl;
                        } else {
                            self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
                        }
                    } else {
                        self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
                    }
                }
                Some(h) => {
                    if h == XS_NS {
                        self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
                    }
                }
            }
        }
        if !block {
            if self.s.flags & BLOCK_DEFAULT_RESTRICTION != 0 {
                self.s.itm(typ).flags |= TYPE_BLOCK_RESTRICTION;
            }
            if self.s.flags & BLOCK_DEFAULT_EXTENSION != 0 {
                self.s.itm(typ).flags |= TYPE_BLOCK_EXTENSION;
            }
        }
        if !final_ {
            if self.s.flags & FINAL_DEFAULT_RESTRICTION != 0 {
                self.s.itm(typ).flags |= TYPE_FINAL_RESTRICTION;
            }
            if self.s.flags & FINAL_DEFAULT_EXTENSION != 0 {
                self.s.itm(typ).flags |= TYPE_FINAL_EXTENSION;
            }
        }
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        self.ctxt_type = Some(typ);
        let has_restr_or_ext =
            (is_schema(child, "simpleContent") || is_schema(child, "complexContent")) && {
                let mut g = first_child(child.unwrap());
                if is_schema(g, "annotation") {
                    g = next_sibling(g.unwrap());
                }
                is_schema(g, "restriction") || is_schema(g, "extension")
            };
        if is_schema(child, "simpleContent") {
            self.s.itm(typ).flags &= !TYPE_MIXED;
            self.parse_content(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        } else if is_schema(child, "complexContent") {
            self.s.itm(typ).content_type = CONTENT_EMPTY;
            self.parse_content(child.unwrap(), false);
            child = next_sibling(child.unwrap());
        } else {
            let at = self.s.builtin(T::ANYTYPE);
            self.s.itm(typ).base_type = Some(at);
            self.s.itm(typ).flags |= TYPE_DERIVATION_METHOD_RESTRICTION;
            if let Some(g) = self.parse_model_group_child(&mut child, true) {
                self.s.itm(typ).subtypes = g;
            }
            let mut list = std::mem::take(&mut self.s.itm(typ).attr_uses);
            let mut dummy = false;
            child = self.parse_local_attributes(child, &mut list, TYPE_RESTRICTION, &mut dummy);
            self.s.itm(typ).attr_uses = list;
            self.s.itm(typ).has_attr_uses = true;
            if is_schema(child, "anyAttribute") {
                let w = self.parse_any_attribute(child.unwrap());
                self.s.itm(typ).attribute_wildcard = w;
                child = next_sibling(child.unwrap());
            }
        }
        if child.is_some() {
            self.content_err(E::SCHEMAP_S4S_ELEM_NOT_ALLOWED, None, Some(node), child, None, Some("(annotation?, (simpleContent | complexContent | ((group | all | choice | sequence)?, ((attribute | attributeGroup)*, anyAttribute?))))"));
        }
        if top_level && self.is_redefine && !has_restr_or_ext {
            self.pcustom_err(E::SCHEMAP_SRC_REDEFINE, None, Some(node), "This is a redefinition, thus the <complexType> must have a <restriction> or <extension> grand-child");
        }
        self.ctxt_type = ctxt_type;
        Some(typ)
    }
}
