//! Parsing of attribute and element declarations (xmlschemas.c).

use super::parse::{PCtxt, first_child, next_sibling};
use super::*;
use crate::xmlschemastypes as T;

const DEF_VAL_DEFAULT: i32 = 1;
const DEF_VAL_FIXED: i32 = 2;

/// xmlSchemaPValAttrFormDefault
pub fn pval_attr_form_default(value: &str, flags: &mut i32, flag_qualified: i32) -> i32 {
    if value == "qualified" {
        *flags |= flag_qualified;
    } else if value != "unqualified" {
        return 1;
    }
    0
}

/// xmlSchemaPValAttrBlockFinal
pub fn pval_attr_block_final(
    value: &str,
    flags: &mut i32,
    flag_all: i32,
    ext: i32,
    restr: i32,
    subst: i32,
    list: i32,
    union: i32,
) -> i32 {
    if value.is_empty() {
        return 0;
    }
    let mut ret = 0;
    if value == "#all" {
        if flag_all != -1 {
            *flags |= flag_all;
        } else {
            for f in [ext, restr, subst, list, union] {
                if f != -1 {
                    *flags |= f;
                }
            }
        }
    } else {
        for item in value
            .split([' ', '\t', '\n', '\r'])
            .filter(|s| !s.is_empty())
        {
            let f = match item {
                "extension" => ext,
                "restriction" => restr,
                "substitution" => subst,
                "list" => list,
                "union" => union,
                _ => -1,
            };
            if f != -1 {
                *flags |= f;
            } else {
                ret = 1;
            }
            if ret != 0 {
                break;
            }
        }
    }
    ret
}

impl<'a> PCtxt<'a> {
    /// xmlSchemaParseLocalAttribute
    pub fn parse_local_attribute(
        &mut self,
        node: NodeId,
        uses: &[ItemId],
        parent_type: i32,
    ) -> Option<ItemId> {
        let mut tmp_ns: Option<String> = None;
        let mut tmp_name: Option<String> = None;
        let mut is_ref = false;
        let mut occurs = ATTR_USE_OPTIONAL;
        let mut has_form = false;
        let mut def_value: Option<String> = None;
        let mut def_value_type = 0;
        let mut ns: Option<String> = None;
        if let Some(attr) = get_prop_node(node, "ref") {
            match self.pval_attr_node_qname(attr) {
                Ok((u, l)) => {
                    tmp_ns = u;
                    tmp_name = Some(l);
                }
                Err(_) => return None,
            }
            let tn = tmp_ns.clone();
            if self.check_reference(node, Some(attr), tn.as_deref()) != 0 {
                return None;
            }
            is_ref = true;
        }
        let nberrors = self.nberrors;
        for attr in attrs_of(node) {
            match node_ns(attr) {
                None => {
                    let an = node_name(attr);
                    if is_ref {
                        if an == "id" {
                            self.pval_attr_node_id(attr);
                            continue;
                        } else if an == "ref" {
                            continue;
                        }
                    } else if an == "name" {
                        continue;
                    } else if an == "id" {
                        self.pval_attr_node_id(attr);
                        continue;
                    } else if an == "type" {
                        match self.pval_attr_node_qname(attr) {
                            Ok((u, l)) => {
                                tmp_ns = u;
                                tmp_name = Some(l);
                            }
                            Err((_, l)) => {
                                tmp_name = Some(l);
                            }
                        }
                        continue;
                    } else if an == "form" {
                        has_form = true;
                        let v = node_content(attr);
                        if v == "qualified" {
                            ns = self.target_ns.clone();
                        } else if v != "unqualified" {
                            self.psimple_type_err(
                                E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                                attr,
                                None,
                                Some("(qualified | unqualified)"),
                                Some(&v),
                                None,
                            );
                        }
                        continue;
                    }
                    if an == "use" {
                        let v = node_content(attr);
                        match v.as_str() {
                            "optional" => occurs = ATTR_USE_OPTIONAL,
                            "prohibited" => occurs = ATTR_USE_PROHIBITED,
                            "required" => occurs = ATTR_USE_REQUIRED,
                            _ => self.psimple_type_err(
                                E::SCHEMAP_INVALID_ATTR_USE,
                                attr,
                                None,
                                Some("(optional | prohibited | required)"),
                                Some(&v),
                                None,
                            ),
                        }
                        continue;
                    } else if an == "default" {
                        if def_value.is_some() {
                            self.mutual_excl_attr_err(
                                E::SCHEMAP_SRC_ATTRIBUTE_1,
                                None,
                                attr,
                                "default",
                                "fixed",
                            );
                        } else {
                            def_value = Some(node_content(attr));
                            def_value_type = DEF_VAL_DEFAULT;
                        }
                        continue;
                    } else if an == "fixed" {
                        if def_value.is_some() {
                            self.mutual_excl_attr_err(
                                E::SCHEMAP_SRC_ATTRIBUTE_1,
                                None,
                                attr,
                                "default",
                                "fixed",
                            );
                        } else {
                            def_value = Some(node_content(attr));
                            def_value_type = DEF_VAL_FIXED;
                        }
                        continue;
                    }
                }
                Some(h) => {
                    if h != XS_NS {
                        continue;
                    }
                }
            }
            self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, attr);
        }
        if def_value_type == DEF_VAL_DEFAULT && occurs != ATTR_USE_OPTIONAL {
            self.psimple_type_err(E::SCHEMAP_SRC_ATTRIBUTE_2, node, None, Some("(optional | prohibited | required)"), None, Some("The value of the attribute 'use' must be 'optional' if the attribute 'default' is present"));
        }
        if nberrors != self.nberrors {
            return None;
        }
        let mut use_id: Option<ItemId> = None;
        let mut name: Option<String> = None;
        if !is_ref {
            if !has_form && self.s.flags & QUALIF_ATTR != 0 {
                ns = self.target_ns.clone();
            }
            if ns.as_deref() == Some(XSI_NS) {
                let m = format!("The target namespace must not match '{}'", XSI_NS);
                self.custom_err(E::SCHEMAP_NO_XSI, Some(node), None, &m);
            }
            let attr = match get_prop_node(node, "name") {
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
                Some(a) => a,
            };
            let (r, n) = self.pval_attr_node(attr, T::NCNAME);
            if r != 0 {
                return None;
            }
            if n == "xmlns" {
                let nt = self.s.builtin(T::NCNAME);
                self.psimple_type_err(
                    E::SCHEMAP_NO_XMLNS,
                    attr,
                    Some(nt),
                    None,
                    None,
                    Some("The value of the attribute must not match 'xmlns'"),
                );
                return None;
            }
            name = Some(n.clone());
            if occurs != ATTR_USE_PROHIBITED {
                let u = self.add_attribute_use(node);
                self.s.itm(u).occurs = occurs;
                let decl = self.add_attribute(Some(n), ns.clone(), node, false);
                if tmp_name.is_some() {
                    self.s.itm(decl).type_name = tmp_name.clone();
                    self.s.itm(decl).type_ns = tmp_ns.clone();
                }
                self.s.itm(u).attr_decl = Some(decl);
                if let Some(dv) = &def_value {
                    self.s.itm(decl).def_value = Some(dv.clone());
                    if def_value_type == DEF_VAL_FIXED {
                        self.s.itm(decl).flags |= ATTR_FIXED;
                    }
                }
                use_id = Some(u);
            }
        } else if occurs != ATTR_USE_PROHIBITED {
            let u = self.add_attribute_use(node);
            self.add_pending(u);
            self.s.itm(u).occurs = occurs;
            let r = self.new_qname_ref(TYPE_ATTRIBUTE, tmp_name.clone(), tmp_ns.clone());
            self.s.itm(u).attr_decl = Some(r);
            if let Some(dv) = &def_value {
                self.s.itm(u).def_value = Some(dv.clone());
            }
            if def_value_type == DEF_VAL_FIXED {
                self.s.itm(u).flags |= ATTR_USE_FIXED;
            }
            use_id = Some(u);
        }
        // children
        let mut child = first_child(node);
        if occurs == ATTR_USE_PROHIBITED {
            if is_schema(child, "annotation") {
                self.parse_annotation(child.unwrap(), false);
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
            if parent_type == TYPE_ATTRIBUTEGROUP {
                self.custom_warning(E::SCHEMAP_WARN_ATTR_POINTLESS_PROH, Some(node), "Skipping attribute use prohibition, since it is pointless inside an <attributeGroup>");
                return None;
            } else if parent_type == TYPE_EXTENSION {
                self.custom_warning(E::SCHEMAP_WARN_ATTR_POINTLESS_PROH, Some(node), "Skipping attribute use prohibition, since it is pointless when extending a type");
                return None;
            }
            if !is_ref {
                tmp_name = name.clone();
                tmp_ns = ns.clone();
            }
            for &u in uses {
                let it = self.s.it(u);
                if it.typ == EXTRA_ATTR_USE_PROHIB && it.name == tmp_name && it.target_ns == tmp_ns
                {
                    let m = format!(
                        "Skipping duplicate attribute use prohibition '{}'",
                        format_qname(tmp_ns.as_deref(), tmp_name.as_deref())
                    );
                    self.custom_warning(E::SCHEMAP_WARN_ATTR_POINTLESS_PROH, Some(node), &m);
                    return None;
                }
            }
            let p = self.add_attribute_use_prohib();
            {
                let it = self.s.itm(p);
                it.node = Some(node);
                it.name = tmp_name;
                it.target_ns = tmp_ns;
                it.is_ref = is_ref;
            }
            if is_ref {
                self.add_pending(p);
            }
            return Some(p);
        }
        let u = use_id?;
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        if is_ref {
            if child.is_some() {
                if is_schema(child, "simpleType") {
                    self.content_err(
                        E::SCHEMAP_SRC_ATTRIBUTE_3_2,
                        None,
                        Some(node),
                        child,
                        None,
                        Some("(annotation?)"),
                    );
                } else {
                    self.content_err(
                        E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                        None,
                        Some(node),
                        child,
                        None,
                        Some("(annotation?)"),
                    );
                }
            }
        } else {
            if is_schema(child, "simpleType") {
                let decl = self.s.it(u).attr_decl.unwrap();
                if self.s.it(decl).type_name.is_some() {
                    self.content_err(E::SCHEMAP_SRC_ATTRIBUTE_4, None, Some(node), child, Some("The attribute 'type' and the <simpleType> child are mutually exclusive"), None);
                } else {
                    let st = self.parse_simple_type(child.unwrap(), false);
                    self.s.itm(decl).subtypes = st;
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
                    Some("(annotation?, simpleType?)"),
                );
            }
        }
        Some(u)
    }

    /// xmlSchemaParseGlobalAttribute
    pub fn parse_global_attribute(&mut self, node: NodeId) -> Option<ItemId> {
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
        if name == "xmlns" {
            let nt = self.s.builtin(T::NCNAME);
            self.psimple_type_err(
                E::SCHEMAP_NO_XMLNS,
                attr,
                Some(nt),
                None,
                None,
                Some("The value of the attribute must not match 'xmlns'"),
            );
            return None;
        }
        if self.target_ns.as_deref() == Some(XSI_NS) {
            let m = format!("The target namespace must not match '{}'", XSI_NS);
            self.custom_err(E::SCHEMAP_NO_XSI, Some(node), None, &m);
        }
        let tns = self.target_ns.clone();
        let ret = self.add_attribute(Some(name), tns, node, true);
        self.s.itm(ret).flags |= ATTR_GLOBAL;
        self.check_attrs(node, &["id", "default", "fixed", "name", "type"]);
        match self.pval_attr_qname(node, "type") {
            Ok(Some((u, l))) => {
                self.s.itm(ret).type_ns = u;
                self.s.itm(ret).type_name = Some(l);
            }
            Ok(None) => {}
            Err((_, l)) => self.s.itm(ret).type_name = Some(l),
        }
        self.pval_attr_id(node, "id");
        let fixed = get_prop(node, "fixed");
        if fixed.is_some() {
            self.s.itm(ret).def_value = fixed;
            self.s.itm(ret).flags |= ATTR_FIXED;
        }
        if let Some(a) = get_prop_node(node, "default") {
            if self.s.it(ret).flags & ATTR_FIXED != 0 {
                self.mutual_excl_attr_err(
                    E::SCHEMAP_SRC_ATTRIBUTE_1,
                    Some(ret),
                    a,
                    "default",
                    "fixed",
                );
            } else {
                self.s.itm(ret).def_value = Some(node_content(a));
            }
        }
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        if is_schema(child, "simpleType") {
            if self.s.it(ret).type_name.is_some() {
                self.content_err(
                    E::SCHEMAP_SRC_ATTRIBUTE_4,
                    None,
                    Some(node),
                    child,
                    Some("The attribute 'type' and the <simpleType> child are mutually exclusive"),
                    None,
                );
            } else {
                let st = self.parse_simple_type(child.unwrap(), false);
                self.s.itm(ret).subtypes = st;
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
                Some("(annotation?, simpleType?)"),
            );
        }
        Some(ret)
    }

    /// xmlSchemaParseAttributeGroupRef
    pub fn parse_attribute_group_ref(&mut self, node: NodeId) -> Option<ItemId> {
        let attr = match get_prop_node(node, "ref") {
            None => {
                self.missing_attr_err(E::SCHEMAP_S4S_ATTR_MISSING, None, Some(node), "ref", None);
                return None;
            }
            Some(a) => a,
        };
        let (ref_ns, rname) = match self.pval_attr_node_qname(attr) {
            Ok((u, l)) => (u, Some(l)),
            Err((_, l)) => (None, Some(l)),
        };
        if self.check_reference(node, Some(attr), ref_ns.as_deref()) != 0 {
            return None;
        }
        self.check_attrs(node, &["ref", "id"]);
        self.pval_attr_id(node, "id");
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), false);
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
        let is_redef_ref = self.is_redefine
            && self
                .cur_redef
                .map(|i| {
                    let rd = &self.redefs[i];
                    self.s.it(rd.item).typ == TYPE_ATTRIBUTEGROUP
                        && rd.ref_name == rname
                        && rd.ref_ns == ref_ns
                })
                .unwrap_or(false);
        if is_redef_ref {
            if self.redef_counter != 0 {
                let m = format!(
                    "The redefining attribute group definition '{}' must not contain more than one reference to the redefined definition",
                    format_qname(ref_ns.as_deref(), rname.as_deref())
                );
                self.custom_err(E::SCHEMAP_SRC_REDEFINE, Some(node), None, &m);
                return None;
            }
            self.redef_counter += 1;
            let r = self.new_qname_ref(TYPE_ATTRIBUTEGROUP, rname, ref_ns);
            self.s.itm(r).node = Some(node);
            let i = self.cur_redef.unwrap();
            self.redefs[i].reference = Some(r);
            return Some(r);
        }
        let r = self.new_qname_ref(TYPE_ATTRIBUTEGROUP, rname, ref_ns);
        self.s.itm(r).node = Some(node);
        self.add_pending(r);
        Some(r)
    }

    /// xmlSchemaParseAttributeGroupDefinition
    pub fn parse_attribute_group_definition(&mut self, node: NodeId) -> Option<ItemId> {
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
        let ret = self.add_attribute_group_definition(&name, tns, node);
        self.check_attrs(node, &["name", "id"]);
        self.pval_attr_id(node, "id");
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        let mut list = Vec::new();
        let mut has_refs = false;
        child = self.parse_local_attributes(child, &mut list, TYPE_ATTRIBUTEGROUP, &mut has_refs);
        self.s.itm(ret).attr_uses = list;
        self.s.itm(ret).has_attr_uses = true;
        if has_refs {
            self.s.itm(ret).flags |= ATTRGROUP_HAS_REFS;
        }
        if is_schema(child, "anyAttribute") {
            let w = self.parse_any_attribute(child.unwrap());
            self.s.itm(ret).attribute_wildcard = w;
            child = next_sibling(child.unwrap());
        }
        if child.is_some() {
            self.content_err(
                E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                None,
                Some(node),
                child,
                None,
                Some("(annotation?, ((attribute | attributeGroup)*, anyAttribute?))"),
            );
        }
        Some(ret)
    }

    /// xmlSchemaParseIDCSelectorAndField
    fn parse_idc_selector_and_field(&mut self, idc: ItemId, node: NodeId) -> IdcSelect {
        let _ = idc;
        self.check_attrs(node, &["id", "xpath"]);
        let mut item = IdcSelect::default();
        match get_prop_node(node, "xpath") {
            None => {
                self.missing_attr_err(E::SCHEMAP_S4S_ATTR_MISSING, None, Some(node), "name", None)
            }
            Some(a) => item.xpath = node_content(a),
        }
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
        item
    }

    /// xmlSchemaParseIDC
    pub fn parse_idc(
        &mut self,
        node: NodeId,
        category: i32,
        tns: Option<String>,
    ) -> Option<ItemId> {
        for a in attrs_of(node) {
            match node_ns(a) {
                None => {
                    let n = node_name(a);
                    if n != "id" && n != "name" && (category != TYPE_IDC_KEYREF || n != "refer") {
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
        let item = self.add_idc(&name, tns, category, node);
        self.s.has_idcs = true;
        self.pval_attr_id(node, "id");
        if category == TYPE_IDC_KEYREF {
            match get_prop_node(node, "refer") {
                None => self.missing_attr_err(
                    E::SCHEMAP_S4S_ATTR_MISSING,
                    None,
                    Some(node),
                    "refer",
                    None,
                ),
                Some(a) => {
                    let r = self.new_qname_ref(TYPE_IDC_KEY, None, None);
                    self.s.itm(item).idc_ref = Some(r);
                    match self.pval_attr_node_qname(a) {
                        Ok((u, l)) => {
                            self.s.itm(r).target_ns = u;
                            self.s.itm(r).name = Some(l);
                        }
                        Err((_, l)) => self.s.itm(r).name = Some(l),
                    }
                    let u = self.s.it(r).target_ns.clone();
                    self.check_reference(node, Some(a), u.as_deref());
                }
            }
        }
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        if child.is_none() {
            self.content_err(
                E::SCHEMAP_S4S_ELEM_MISSING,
                None,
                Some(node),
                child,
                Some("A child element is missing"),
                Some("(annotation?, (selector, field+))"),
            );
        }
        if is_schema(child, "selector") {
            let sel = self.parse_idc_selector_and_field(item, child.unwrap());
            self.s.itm(item).selector = Some(sel);
            child = next_sibling(child.unwrap());
            if is_schema(child, "field") {
                loop {
                    let mut f = self.parse_idc_selector_and_field(item, child.unwrap());
                    f.index = self.s.it(item).fields.len() as i32;
                    self.s.itm(item).fields.push(f);
                    child = next_sibling(child.unwrap());
                    if !is_schema(child, "field") {
                        break;
                    }
                }
            } else {
                self.content_err(
                    E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                    None,
                    Some(node),
                    child,
                    None,
                    Some("(annotation?, (selector, field+))"),
                );
            }
        }
        if child.is_some() {
            self.content_err(
                E::SCHEMAP_S4S_ELEM_NOT_ALLOWED,
                None,
                Some(node),
                child,
                None,
                Some("(annotation?, (selector, field+))"),
            );
        }
        Some(item)
    }

    /// xmlSchemaParseElement: returns the declaration (top level) or the
    /// particle.
    pub fn parse_element(
        &mut self,
        node: NodeId,
        is_elem_ref: &mut bool,
        top_level: bool,
    ) -> Option<ItemId> {
        *is_elem_ref = false;
        let name_attr = get_prop_node(node, "name");
        let ref_attr = get_prop_node(node, "ref");
        let mut is_ref = false;
        if top_level || ref_attr.is_none() {
            if name_attr.is_none() {
                self.missing_attr_err(E::SCHEMAP_S4S_ATTR_MISSING, None, Some(node), "name", None);
                return None;
            }
        } else {
            is_ref = true;
        }
        self.pval_attr_id(node, "id");
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
            self.parse_annotation(child.unwrap(), true);
            child = next_sibling(child.unwrap());
        }
        let mut particle = None;
        if !top_level {
            let min = self.get_min_occurs(node, 0, -1, 1, "xs:nonNegativeInteger");
            let max =
                self.get_max_occurs(node, 0, UNBOUNDED, 1, "(xs:nonNegativeInteger | unbounded)");
            self.check_particle_correct_2(node, min, max);
            let p = self.add_particle(Some(node), min, max);
            particle = Some(p);
            if is_ref {
                *is_elem_ref = true;
                let attr = ref_attr.unwrap();
                let (ref_ns, rname) = match self.pval_attr_node_qname(attr) {
                    Ok((u, l)) => (u, Some(l)),
                    Err((_, l)) => (None, Some(l)),
                };
                self.check_reference(node, Some(attr), ref_ns.as_deref());
                if let Some(na) = name_attr {
                    self.mutual_excl_attr_err(E::SCHEMAP_SRC_ELEMENT_2_1, None, na, "ref", "name");
                }
                for a in attrs_of(node) {
                    match node_ns(a) {
                        None => {
                            let n = node_name(a);
                            if matches!(
                                n.as_str(),
                                "ref" | "name" | "id" | "maxOccurs" | "minOccurs"
                            ) {
                                continue;
                            }
                            self.custom_attr_err(E::SCHEMAP_SRC_ELEMENT_2_2, None, Some(a), "Only the attributes 'minOccurs', 'maxOccurs' and 'id' are allowed in addition to 'ref'");
                            break;
                        }
                        Some(h) => {
                            if h == XS_NS {
                                self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
                            }
                        }
                    }
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
                let r = self.new_qname_ref(TYPE_ELEMENT, rname, ref_ns);
                self.s.itm(p).children = Some(r);
                self.add_pending(p);
                return Some(p);
            }
        }
        // declaration part
        let name_attr = name_attr.unwrap();
        let (r, name) = self.pval_attr_node(name_attr, T::NCNAME);
        if r != 0 {
            return None;
        }
        let mut ns: Option<String> = None;
        if top_level {
            ns = self.target_ns.clone();
        } else if let Some(a) = get_prop_node(node, "form") {
            let v = node_content(a);
            if v == "qualified" {
                ns = self.target_ns.clone();
            } else if v != "unqualified" {
                self.psimple_type_err(
                    E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                    a,
                    None,
                    Some("(qualified | unqualified)"),
                    Some(&v),
                    None,
                );
            }
        } else if self.s.flags & QUALIF_ELEM != 0 {
            ns = self.target_ns.clone();
        }
        let decl = self.add_element(&name, ns, node, top_level);
        for a in attrs_of(node) {
            match node_ns(a) {
                None => {
                    let n = node_name(a);
                    if !matches!(
                        n.as_str(),
                        "name" | "type" | "id" | "default" | "fixed" | "block" | "nillable"
                    ) {
                        if !top_level {
                            if !matches!(n.as_str(), "maxOccurs" | "minOccurs" | "form") {
                                self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
                            }
                        } else if !matches!(n.as_str(), "final" | "abstract" | "substitutionGroup")
                        {
                            self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
                        }
                    }
                }
                Some(h) => {
                    if h == XS_NS {
                        self.illegal_attr_err(E::SCHEMAP_S4S_ATTR_NOT_ALLOWED, a);
                    }
                }
            }
        }
        if top_level {
            self.s.itm(decl).flags |= ELEM_GLOBAL | ELEM_TOPLEVEL;
            match self.pval_attr_qname(node, "substitutionGroup") {
                Ok(Some((u, l))) => {
                    self.s.itm(decl).subst_group_ns = u;
                    self.s.itm(decl).subst_group = Some(l);
                }
                Ok(None) => {}
                Err((_, l)) => self.s.itm(decl).subst_group = Some(l),
            }
            if self.get_boolean_prop(node, "abstract", false) {
                self.s.itm(decl).flags |= ELEM_ABSTRACT;
            }
            match get_prop_node(node, "final") {
                None => {
                    if self.s.flags & FINAL_DEFAULT_EXTENSION != 0 {
                        self.s.itm(decl).flags |= ELEM_FINAL_EXTENSION;
                    }
                    if self.s.flags & FINAL_DEFAULT_RESTRICTION != 0 {
                        self.s.itm(decl).flags |= ELEM_FINAL_RESTRICTION;
                    }
                }
                Some(a) => {
                    let v = node_content(a);
                    let mut fl = self.s.it(decl).flags;
                    if pval_attr_block_final(
                        &v,
                        &mut fl,
                        -1,
                        ELEM_FINAL_EXTENSION,
                        ELEM_FINAL_RESTRICTION,
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
                    }
                    self.s.itm(decl).flags = fl;
                }
            }
        }
        match get_prop_node(node, "block") {
            None => {
                if self.s.flags & BLOCK_DEFAULT_RESTRICTION != 0 {
                    self.s.itm(decl).flags |= ELEM_BLOCK_RESTRICTION;
                }
                if self.s.flags & BLOCK_DEFAULT_EXTENSION != 0 {
                    self.s.itm(decl).flags |= ELEM_BLOCK_EXTENSION;
                }
                if self.s.flags & BLOCK_DEFAULT_SUBSTITUTION != 0 {
                    self.s.itm(decl).flags |= ELEM_BLOCK_SUBSTITUTION;
                }
            }
            Some(a) => {
                let v = node_content(a);
                let mut fl = self.s.it(decl).flags;
                if pval_attr_block_final(
                    &v,
                    &mut fl,
                    -1,
                    ELEM_BLOCK_EXTENSION,
                    ELEM_BLOCK_RESTRICTION,
                    ELEM_BLOCK_SUBSTITUTION,
                    -1,
                    -1,
                ) != 0
                {
                    self.psimple_type_err(
                        E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                        a,
                        None,
                        Some("(#all | List of (extension | restriction | substitution))"),
                        Some(&v),
                        None,
                    );
                }
                self.s.itm(decl).flags = fl;
            }
        }
        if self.get_boolean_prop(node, "nillable", false) {
            self.s.itm(decl).flags |= ELEM_NILLABLE;
        }
        if let Some(a) = get_prop_node(node, "type") {
            match self.pval_attr_node_qname(a) {
                Ok((u, l)) => {
                    self.s.itm(decl).named_type_ns = u;
                    self.s.itm(decl).named_type = Some(l);
                }
                Err((_, l)) => self.s.itm(decl).named_type = Some(l),
            }
            let u = self.s.it(decl).named_type_ns.clone();
            self.check_reference(node, Some(a), u.as_deref());
        }
        self.s.itm(decl).value = get_prop(node, "default");
        if let Some(a) = get_prop_node(node, "fixed") {
            let fixed = node_content(a);
            if self.s.it(decl).value.is_some() {
                self.mutual_excl_attr_err(E::SCHEMAP_SRC_ELEMENT_1, None, a, "default", "fixed");
            } else {
                self.s.itm(decl).flags |= ELEM_FIXED;
                self.s.itm(decl).value = Some(fixed);
            }
        }
        if is_schema(child, "complexType") {
            if self.s.it(decl).named_type.is_some() {
                self.content_err(
                    E::SCHEMAP_SRC_ELEMENT_3,
                    None,
                    Some(node),
                    child,
                    Some("The attribute 'type' and the <complexType> child are mutually exclusive"),
                    None,
                );
            } else {
                let t = self.parse_complex_type(child.unwrap(), false);
                self.s.itm(decl).subtypes = t;
            }
            child = next_sibling(child.unwrap());
        } else if is_schema(child, "simpleType") {
            if self.s.it(decl).named_type.is_some() {
                self.content_err(
                    E::SCHEMAP_SRC_ELEMENT_3,
                    None,
                    Some(node),
                    child,
                    Some("The attribute 'type' and the <simpleType> child are mutually exclusive"),
                    None,
                );
            } else {
                let t = self.parse_simple_type(child.unwrap(), false);
                self.s.itm(decl).subtypes = t;
            }
            child = next_sibling(child.unwrap());
        }
        while is_schema(child, "unique") || is_schema(child, "key") || is_schema(child, "keyref") {
            let cat = if is_schema(child, "unique") {
                TYPE_IDC_UNIQUE
            } else if is_schema(child, "key") {
                TYPE_IDC_KEY
            } else {
                TYPE_IDC_KEYREF
            };
            let tns = self.s.it(decl).target_ns.clone();
            if let Some(idc) = self.parse_idc(child.unwrap(), cat, tns) {
                self.s.itm(decl).idcs.push(idc);
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
                Some("(annotation?, ((simpleType | complexType)?, (unique | key | keyref)*))"),
            );
        }
        if top_level {
            Some(decl)
        } else {
            let p = particle.unwrap();
            self.s.itm(p).children = Some(decl);
            Some(p)
        }
    }
}
