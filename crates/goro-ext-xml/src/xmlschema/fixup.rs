//! Schema component fixup: reference resolution, type fixup, attribute
//! uses, constraints and content model compilation
//! (xmlSchemaFixupComponents and friends).

use super::parse::PCtxt;
use super::simple::{
    self, ErrCtx, check_cvc_simple_type, is_derived_from_builtin, primitive_type,
    union_member_types,
};
use super::*;
use crate::xmlregexp::regexp_compile;
use crate::xmlschemastypes as T;

impl<'a> PCtxt<'a> {
    /// Run a simple type check with the parser as error context.
    pub fn pcheck_simple(
        &mut self,
        node: Option<NodeId>,
        typ: ItemId,
        value: &str,
        fire: bool,
    ) -> (i32, Option<simple::VList>) {
        let mut ec = ErrCtx::parser();
        let r = check_cvc_simple_type(&self.s, &mut ec, node, typ, value, true, fire, true, false);
        self.merge(ec);
        r
    }

    pub fn merge(&mut self, ec: ErrCtx) {
        self.nberrors += ec.nberrors;
        if ec.nberrors > 0 {
            self.err = ec.err;
        }
        self.errors.extend(ec.errors);
    }

    /// xmlSchemaAddComponents
    fn add_components(&mut self, b: usize) {
        if self.s.buckets[b].marked {
            return;
        }
        self.s.buckets[b].marked = true;
        let globals = self.s.buckets[b].globals.clone();
        for item in globals {
            let it = self.s.it(item);
            if matches!(it.typ, TYPE_COMPLEX | TYPE_SIMPLE) && it.flags & TYPE_REDEFINED != 0 {
                continue;
            }
            if it.typ == TYPE_GROUP && it.flags & MODEL_GROUP_DEF_REDEFINED != 0 {
                continue;
            }
            if it.typ == TYPE_ATTRIBUTEGROUP && it.flags & ATTRGROUP_REDEFINED != 0 {
                continue;
            }
            let name = it.name.clone().unwrap_or_default();
            let key = (kind_of(it.typ), name, it.target_ns.clone());
            if let std::collections::hash_map::Entry::Vacant(e) = self.s.globals.entry(key) {
                e.insert(item);
            } else {
                let m = format!(
                    "A global {} '{}' does already exist",
                    self.s.component_type_str(item),
                    self.s.component_qname(item)
                );
                let node = self.s.component_node(item);
                self.custom_err(E::SCHEMAP_REDEFINED_TYPE, node, Some(item), &m);
            }
        }
        let rels: Vec<usize> = self.s.buckets[b]
            .relations
            .iter()
            .filter_map(|r| r.bucket)
            .collect();
        for rb in rels {
            self.add_components(rb);
        }
    }

    /// xmlSchemaFixupComponents
    pub fn fixup_components(&mut self) -> i32 {
        if self.pending.is_empty() {
            return 0;
        }
        let mb = self.main_bucket;
        self.check_src_redefine_first();
        self.add_components(mb);
        self.ctxt_type = None;
        let items = self.pending.clone();
        macro_rules! check_err {
            () => {
                if self.err == E::SCHEMAP_INTERNAL {
                    return -1;
                }
                if self.nberrors != 0 {
                    return self.err;
                }
            };
        }
        for &item in &items {
            match self.s.it(item).typ {
                TYPE_ELEMENT => self.resolve_element_references(item),
                TYPE_COMPLEX | TYPE_SIMPLE => self.resolve_type_references(item),
                TYPE_ATTRIBUTE => {
                    self.resolve_attr_type_references(item);
                }
                TYPE_ATTRIBUTE_USE => {
                    self.resolve_attr_use_references(item);
                }
                EXTRA_QNAMEREF => {
                    if self.s.it(item).item_type == TYPE_ATTRIBUTEGROUP {
                        self.resolve_attr_group_references(item);
                    }
                }
                TYPE_SEQUENCE | TYPE_CHOICE | TYPE_ALL => {
                    self.resolve_model_group_particle_references(item)
                }
                TYPE_IDC_KEY | TYPE_IDC_UNIQUE | TYPE_IDC_KEYREF => {
                    self.resolve_idc_key_references(item);
                }
                EXTRA_ATTR_USE_PROHIB => {
                    self.resolve_attr_use_prohib_references(item);
                }
                _ => {}
            }
            if self.err == E::SCHEMAP_INTERNAL {
                return -1;
            }
        }
        check_err!();
        for &item in &items {
            match self.s.it(item).typ {
                TYPE_COMPLEX | TYPE_SIMPLE => self.check_type_def_circular(item),
                TYPE_GROUP => self.check_group_def_circular(item),
                TYPE_ATTRIBUTEGROUP => self.check_attr_group_circular(item),
                _ => {}
            }
            check_err!();
        }
        for &item in &items {
            if matches!(self.s.it(item).typ, TYPE_SEQUENCE | TYPE_CHOICE) {
                self.model_group_to_model_group_def_fixup(item);
            }
        }
        check_err!();
        for &item in &items {
            if self.s.it(item).typ == TYPE_ATTRIBUTEGROUP {
                let f = self.s.it(item).flags;
                if f & ATTRGROUP_WILDCARD_BUILDED == 0 && f & ATTRGROUP_HAS_REFS != 0 {
                    self.attribute_group_expand_refs(item);
                    if self.err == E::SCHEMAP_INTERNAL {
                        return -1;
                    }
                }
            }
        }
        check_err!();
        for &item in &items {
            if self.s.it(item).typ == TYPE_SIMPLE
                && self.s.it(item).flags & TYPE_FIXUP_1 == 0
                && self.fixup_simple_type_stage_one(item) == -1
            {
                return -1;
            }
        }
        check_err!();
        for &item in &items {
            if self.s.it(item).typ == TYPE_SIMPLE && !self.s.it(item).member_types.is_empty() {
                self.check_union_type_def_circular(item);
            }
        }
        check_err!();
        for &item in &items {
            if self.s.it(item).typ == TYPE_SIMPLE
                && self.type_not_fixed(item)
                && self.fixup_simple_type_stage_two(item) == -1
            {
                return -1;
            }
        }
        check_err!();
        for &item in &items {
            if self.s.it(item).typ == TYPE_ATTRIBUTE && self.check_attr_props_correct(item) == -1 {
                return -1;
            }
        }
        check_err!();
        for &item in &items {
            if self.s.it(item).typ == TYPE_ATTRIBUTE_USE
                && self.s.it(item).def_value.is_some()
                && self.check_attr_use_props_correct(item) == -1
            {
                return -1;
            }
        }
        check_err!();
        for &item in &items {
            if self.s.it(item).typ == TYPE_ATTRIBUTEGROUP && self.s.it(item).attr_uses.len() > 1 {
                self.check_ag_props_correct(item);
            }
        }
        check_err!();
        for &item in &items {
            if self.s.it(item).typ == TYPE_COMPLEX
                && self.type_not_fixed(item)
                && self.fixup_complex_type(item) == -1
            {
                return -1;
            }
        }
        check_err!();
        let items = self.pending.clone();
        for &item in &items {
            if self.s.it(item).typ == TYPE_ELEMENT
                && self.s.it(item).flags & ELEM_INTERNAL_CHECKED == 0
            {
                self.check_element_decl_component(item);
            }
        }
        check_err!();
        for &item in &items {
            if self.s.it(item).typ == TYPE_COMPLEX {
                self.build_content_model(item);
            }
        }
        check_err!();
        self.pending.clear();
        0
    }

    /// xmlSchemaFindRedefCompInGraph
    fn find_redef_comp(
        &self,
        b: usize,
        typ: i32,
        name: Option<&str>,
        ns: Option<&str>,
        marked: &mut Vec<usize>,
    ) -> Option<ItemId> {
        name?;
        for &g in &self.s.buckets[b].globals {
            let it = self.s.it(g);
            if it.typ == typ && it.name.as_deref() == name && it.target_ns.as_deref() == ns {
                return Some(g);
            }
        }
        marked.push(b);
        let rels: Vec<usize> = self.s.buckets[b]
            .relations
            .iter()
            .filter_map(|r| r.bucket)
            .collect();
        for rb in rels {
            if !marked.contains(&rb) {
                if let Some(r) = self.find_redef_comp(rb, typ, name, ns, marked) {
                    return Some(r);
                }
            }
        }
        marked.pop();
        None
    }

    /// xmlSchemaCheckSRCRedefineFirst
    fn check_src_redefine_first(&mut self) -> i32 {
        let mut err = 0;
        for i in 0..self.redefs.len() {
            let item = self.redefs[i].item;
            let ityp = self.s.it(item).typ;
            let prev = match self.redefs[i].target_bucket {
                Some(tb) => {
                    let mut marked = Vec::new();
                    let (n, ns) = (
                        self.redefs[i].ref_name.clone(),
                        self.redefs[i].ref_ns.clone(),
                    );
                    self.find_redef_comp(tb, ityp, n.as_deref(), ns.as_deref(), &mut marked)
                }
                None => None,
            };
            let node = match self.redefs[i].reference {
                Some(r) => self.s.component_node(r),
                None => self.s.component_node(item),
            };
            let prev = match prev {
                None => {
                    let m = format!(
                        "The {} '{}' to be redefined could not be found in the redefined schema",
                        Schema::item_type_to_str(ityp),
                        format_qname(
                            self.redefs[i].ref_ns.as_deref(),
                            self.redefs[i].ref_name.as_deref()
                        )
                    );
                    self.custom_err(E::SCHEMAP_SRC_REDEFINE, node, None, &m);
                    err = self.err;
                    continue;
                }
                Some(p) => p,
            };
            let mut was_redefined = false;
            match ityp {
                TYPE_COMPLEX | TYPE_SIMPLE => {
                    if self.s.it(prev).flags & TYPE_REDEFINED != 0 {
                        was_redefined = true;
                    } else {
                        self.s.itm(prev).flags |= TYPE_REDEFINED;
                        self.s.itm(item).base_type = Some(prev);
                    }
                }
                TYPE_GROUP => {
                    if self.s.it(prev).flags & MODEL_GROUP_DEF_REDEFINED != 0 {
                        was_redefined = true;
                    } else {
                        self.s.itm(prev).flags |= MODEL_GROUP_DEF_REDEFINED;
                        if let Some(r) = self.redefs[i].reference {
                            self.s.itm(r).children = Some(prev);
                        }
                        self.redefs[i].target = Some(prev);
                    }
                }
                TYPE_ATTRIBUTEGROUP => {
                    if self.s.it(prev).flags & ATTRGROUP_REDEFINED != 0 {
                        was_redefined = true;
                    } else {
                        self.s.itm(prev).flags |= ATTRGROUP_REDEFINED;
                        if let Some(r) = self.redefs[i].reference {
                            self.s.itm(r).item = Some(prev);
                            self.redefs[i].target = None;
                        } else {
                            self.redefs[i].target = Some(prev);
                        }
                    }
                }
                _ => {
                    self.internal_err(
                        "xmlSchemaResolveRedefReferences",
                        "Unexpected redefined component type",
                    );
                    return -1;
                }
            }
            if was_redefined {
                let m = format!(
                    "The referenced {} was already redefined. Multiple redefinition of the same component is not supported",
                    self.s.component_designation(prev)
                );
                self.custom_err(E::SCHEMAP_SRC_REDEFINE, node, None, &m);
                err = self.err;
            }
        }
        err
    }

    pub fn type_not_fixed(&self, t: ItemId) -> bool {
        let it = self.s.it(t);
        it.typ != TYPE_BASIC && it.flags & TYPE_INTERNAL_RESOLVED == 0
    }

    // -----------------------------------------------------------------
    // Reference resolution

    /// xmlSchemaResolveElementReferences
    fn resolve_element_references(&mut self, e: ItemId) {
        if self.s.it(e).flags & ELEM_INTERNAL_RESOLVED != 0 {
            return;
        }
        self.s.itm(e).flags |= ELEM_INTERNAL_RESOLVED;
        if self.s.it(e).subtypes.is_none() {
            if let Some(nt) = self.s.it(e).named_type.clone() {
                let ns = self.s.it(e).named_type_ns.clone();
                match self.s.get_type(&nt, ns.as_deref()) {
                    None => {
                        let node = self.s.it(e).node;
                        self.res_comp_attr_err(
                            E::SCHEMAP_SRC_RESOLVE,
                            Some(e),
                            node,
                            "type",
                            Some(&nt),
                            ns.as_deref(),
                            TYPE_BASIC,
                            Some("type definition"),
                        );
                    }
                    Some(t) => self.s.itm(e).subtypes = Some(t),
                }
            }
        }
        if let Some(sg) = self.s.it(e).subst_group.clone() {
            let ns = self.s.it(e).subst_group_ns.clone();
            match self.s.get_global(TYPE_ELEMENT, &sg, ns.as_deref()) {
                None => {
                    self.res_comp_attr_err(
                        E::SCHEMAP_SRC_RESOLVE,
                        Some(e),
                        None,
                        "substitutionGroup",
                        Some(&sg),
                        ns.as_deref(),
                        TYPE_ELEMENT,
                        None,
                    );
                }
                Some(head) => {
                    self.resolve_element_references(head);
                    self.s.itm(e).ref_decl = Some(head);
                    if self.s.it(e).subtypes.is_none() {
                        let t = match self.s.it(head).subtypes {
                            None => self.s.builtin(T::ANYTYPE),
                            Some(t) => t,
                        };
                        self.s.itm(e).subtypes = Some(t);
                    }
                }
            }
        }
        let it = self.s.it(e);
        if it.subtypes.is_none() && it.named_type.is_none() && it.subst_group.is_none() {
            let at = self.s.builtin(T::ANYTYPE);
            self.s.itm(e).subtypes = Some(at);
        }
    }

    /// xmlSchemaResolveUnionMemberTypes
    fn resolve_union_member_types(&mut self, t: ItemId) {
        let refs = std::mem::take(&mut self.s.itm(t).member_types);
        let mut members = Vec::new();
        for r in refs {
            let name = self.s.it(r).name.clone().unwrap_or_default();
            let ns = self.s.it(r).target_ns.clone();
            match self.s.get_type(&name, ns.as_deref()) {
                Some(m) if self.s.is_simple(m) => members.push(m),
                _ => {
                    let node = self.s.it(t).node;
                    self.res_comp_attr_err(
                        E::SCHEMAP_SRC_RESOLVE,
                        Some(t),
                        node,
                        "memberTypes",
                        Some(&name),
                        ns.as_deref(),
                        TYPE_SIMPLE,
                        None,
                    );
                }
            }
        }
        let mut sub = self.s.it(t).subtypes;
        while let Some(st) = sub {
            members.push(st);
            sub = self.s.it(st).next;
        }
        self.s.itm(t).member_types = members;
    }

    /// xmlSchemaResolveTypeReferences
    fn resolve_type_references(&mut self, t: ItemId) {
        if self.s.it(t).base_type.is_none() {
            let base = self.s.it(t).base.clone();
            let base_ns = self.s.it(t).base_ns.clone();
            let bt = base
                .as_deref()
                .and_then(|b| self.s.get_type(b, base_ns.as_deref()));
            match bt {
                None => {
                    let node = self.s.it(t).node;
                    self.res_comp_attr_err(
                        E::SCHEMAP_SRC_RESOLVE,
                        Some(t),
                        node,
                        "base",
                        base.as_deref(),
                        base_ns.as_deref(),
                        TYPE_SIMPLE,
                        None,
                    );
                    return;
                }
                Some(b) => self.s.itm(t).base_type = Some(b),
            }
        }
        if self.s.is_simple(t) {
            if self.s.is_union(t) {
                self.resolve_union_member_types(t);
            } else if self.s.is_list(t) && self.s.it(t).subtypes.is_none() {
                if let Some(base) = self.s.it(t).base.clone() {
                    let ns = self.s.it(t).base_ns.clone();
                    match self.s.get_type(&base, ns.as_deref()) {
                        Some(it) if self.s.is_simple(it) => self.s.itm(t).subtypes = Some(it),
                        _ => {
                            self.s.itm(t).subtypes = None;
                            let node = self.s.it(t).node;
                            self.res_comp_attr_err(
                                E::SCHEMAP_SRC_RESOLVE,
                                Some(t),
                                node,
                                "itemType",
                                Some(&base),
                                ns.as_deref(),
                                TYPE_SIMPLE,
                                None,
                            );
                        }
                    }
                }
            }
            return;
        }
        // complex type: a particle whose term is a QName ref (group ref)
        let p = match self.s.it(t).subtypes {
            Some(p) if self.s.it(p).typ == TYPE_PARTICLE => p,
            _ => return,
        };
        let r = match self.s.it(p).children {
            Some(r) if self.s.it(r).typ == EXTRA_QNAMEREF => r,
            _ => return,
        };
        self.s.itm(p).children = None;
        let name = self.s.it(r).name.clone().unwrap_or_default();
        let ns = self.s.it(r).target_ns.clone();
        let it_type = self.s.it(r).item_type;
        match self
            .s
            .get_global(it_type, &name, ns.as_deref())
            .filter(|&g| self.s.it(g).typ == TYPE_GROUP)
        {
            None => {
                let node = self.s.it(p).node;
                self.res_comp_attr_err(
                    E::SCHEMAP_SRC_RESOLVE,
                    None,
                    node,
                    "ref",
                    Some(&name),
                    ns.as_deref(),
                    it_type,
                    None,
                );
                self.s.itm(t).subtypes = None;
            }
            Some(g) => match self.s.it(g).children {
                None => self.s.itm(t).subtypes = None,
                Some(mg) => {
                    self.s.itm(p).children = Some(mg);
                    if self.s.it(mg).typ == TYPE_ALL && self.s.it(p).max_occurs != 1 {
                        let node = self.s.it(p).node;
                        self.custom_err(E::SCHEMAP_COS_ALL_LIMITED, node, None, "The particle's {max occurs} must be 1, since the reference resolves to an 'all' model group");
                    }
                }
            },
        }
    }

    /// xmlSchemaResolveAttrTypeReferences
    fn resolve_attr_type_references(&mut self, a: ItemId) -> i32 {
        if self.s.it(a).flags & ATTR_INTERNAL_RESOLVED != 0 {
            return 0;
        }
        self.s.itm(a).flags |= ATTR_INTERNAL_RESOLVED;
        if self.s.it(a).subtypes.is_some() {
            return 0;
        }
        if let Some(tn) = self.s.it(a).type_name.clone() {
            let ns = self.s.it(a).type_ns.clone();
            match self.s.get_type(&tn, ns.as_deref()) {
                Some(t) if self.s.is_simple(t) => self.s.itm(a).subtypes = Some(t),
                _ => {
                    let node = self.s.it(a).node;
                    self.res_comp_attr_err(
                        E::SCHEMAP_SRC_RESOLVE,
                        Some(a),
                        node,
                        "type",
                        Some(&tn),
                        ns.as_deref(),
                        TYPE_SIMPLE,
                        None,
                    );
                    return self.err;
                }
            }
        } else {
            let ast = self.s.builtin(T::ANYSIMPLETYPE);
            self.s.itm(a).subtypes = Some(ast);
        }
        0
    }

    /// xmlSchemaResolveAttrUseReferences
    fn resolve_attr_use_references(&mut self, u: ItemId) -> i32 {
        let r = match self.s.it(u).attr_decl {
            Some(r) if self.s.it(r).typ == EXTRA_QNAMEREF => r,
            _ => return 0,
        };
        let name = self.s.it(r).name.clone().unwrap_or_default();
        let ns = self.s.it(r).target_ns.clone();
        let d = self.s.get_global(TYPE_ATTRIBUTE, &name, ns.as_deref());
        self.s.itm(u).attr_decl = d;
        if d.is_none() {
            let node = self.s.it(u).node;
            self.res_comp_attr_err(
                E::SCHEMAP_SRC_RESOLVE,
                Some(u),
                node,
                "ref",
                Some(&name),
                ns.as_deref(),
                TYPE_ATTRIBUTE,
                None,
            );
            return self.err;
        }
        0
    }

    /// xmlSchemaResolveAttrGroupReferences
    fn resolve_attr_group_references(&mut self, r: ItemId) -> i32 {
        if self.s.it(r).item.is_some() {
            return 0;
        }
        let name = self.s.it(r).name.clone().unwrap_or_default();
        let ns = self.s.it(r).target_ns.clone();
        match self.s.get_global(TYPE_ATTRIBUTEGROUP, &name, ns.as_deref()) {
            None => {
                let node = self.s.it(r).node;
                let it_type = self.s.it(r).item_type;
                self.res_comp_attr_err(
                    E::SCHEMAP_SRC_RESOLVE,
                    None,
                    node,
                    "ref",
                    Some(&name),
                    ns.as_deref(),
                    it_type,
                    None,
                );
                self.err
            }
            Some(g) => {
                self.s.itm(r).item = Some(g);
                0
            }
        }
    }

    /// xmlSchemaResolveModelGroupParticleReferences
    fn resolve_model_group_particle_references(&mut self, mg: ItemId) {
        let mut p = self.s.it(mg).children;
        while let Some(part) = p {
            let next = self.s.it(part).next;
            let r = match self.s.it(part).children {
                Some(r) if self.s.it(r).typ == EXTRA_QNAMEREF => r,
                _ => {
                    p = next;
                    continue;
                }
            };
            self.s.itm(part).children = None;
            let name = self.s.it(r).name.clone().unwrap_or_default();
            let ns = self.s.it(r).target_ns.clone();
            let it_type = self.s.it(r).item_type;
            let ref_item = match it_type {
                TYPE_GROUP | TYPE_ELEMENT => self.s.get_global(it_type, &name, ns.as_deref()),
                _ => None,
            };
            match ref_item {
                None => {
                    let node = self.s.it(part).node;
                    self.res_comp_attr_err(
                        E::SCHEMAP_SRC_RESOLVE,
                        None,
                        node,
                        "ref",
                        Some(&name),
                        ns.as_deref(),
                        it_type,
                        None,
                    );
                }
                Some(ri) => {
                    if self.s.it(ri).typ == TYPE_GROUP {
                        match self.s.it(ri).children {
                            None => {}
                            Some(m) => {
                                if self.s.it(m).typ == TYPE_ALL {
                                    let node = self.s.it(part).node;
                                    self.custom_err(E::SCHEMAP_COS_ALL_LIMITED, node, None, "A model group definition is referenced, but it contains an 'all' model group, which cannot be contained by model groups");
                                } else {
                                    self.s.itm(part).children = Some(ri);
                                }
                            }
                        }
                    } else {
                        self.s.itm(part).children = Some(ri);
                    }
                }
            }
            p = next;
        }
    }

    /// xmlSchemaResolveIDCKeyReferences
    fn resolve_idc_key_references(&mut self, idc: ItemId) -> i32 {
        if self.s.it(idc).typ != TYPE_IDC_KEYREF {
            return 0;
        }
        let r = match self.s.it(idc).idc_ref {
            Some(r) => r,
            None => return 0,
        };
        let name = match self.s.it(r).name.clone() {
            Some(n) => n,
            None => return 0,
        };
        let ns = self.s.it(r).target_ns.clone();
        match self.s.get_global(TYPE_IDC_KEY, &name, ns.as_deref()) {
            None => {
                let node = self.s.it(idc).node;
                self.res_comp_attr_err(
                    E::SCHEMAP_SRC_RESOLVE,
                    Some(idc),
                    node,
                    "refer",
                    Some(&name),
                    ns.as_deref(),
                    TYPE_IDC_KEY,
                    None,
                );
                self.err
            }
            Some(k) => {
                if self.s.it(k).typ == TYPE_IDC_KEYREF {
                    self.custom_err(
                        E::SCHEMAP_C_PROPS_CORRECT,
                        None,
                        Some(idc),
                        "The keyref references a keyref",
                    );
                    return self.err;
                }
                self.s.itm(r).item = Some(k);
                if self.s.it(idc).fields.len() != self.s.it(k).fields.len() {
                    let m = format!(
                        "The cardinality of the keyref differs from the cardinality of the referenced key/unique '{}'",
                        format_qname(
                            self.s.it(k).target_ns.as_deref(),
                            self.s.it(k).name.as_deref()
                        )
                    );
                    self.custom_err(E::SCHEMAP_C_PROPS_CORRECT, None, Some(idc), &m);
                    return self.err;
                }
                0
            }
        }
    }

    /// xmlSchemaResolveAttrUseProhibReferences
    fn resolve_attr_use_prohib_references(&mut self, p: ItemId) -> i32 {
        let name = self.s.it(p).name.clone().unwrap_or_default();
        let ns = self.s.it(p).target_ns.clone();
        if self
            .s
            .get_global(TYPE_ATTRIBUTE, &name, ns.as_deref())
            .is_none()
        {
            let node = self.s.it(p).node;
            self.res_comp_attr_err(
                E::SCHEMAP_SRC_RESOLVE,
                None,
                node,
                "ref",
                Some(&name),
                ns.as_deref(),
                TYPE_ATTRIBUTE,
                None,
            );
            return E::SCHEMAP_SRC_RESOLVE;
        }
        0
    }

    // -----------------------------------------------------------------
    // Circularity

    fn check_type_def_circular(&mut self, t: ItemId) {
        let it = self.s.it(t);
        if it.typ == TYPE_BASIC || it.base_type.is_none() {
            return;
        }
        let mut anc = it.base_type;
        let mut visited = std::collections::HashSet::new();
        while let Some(a) = anc {
            if self.s.it(a).typ == TYPE_BASIC {
                return;
            }
            if a == t {
                let node = self.s.component_node(t);
                self.pcustom_err(
                    E::SCHEMAP_ST_PROPS_CORRECT_2,
                    Some(t),
                    node,
                    "The definition is circular",
                );
                return;
            }
            if !visited.insert(a) {
                return;
            }
            anc = self.s.it(a).base_type;
        }
    }

    fn circ_model_group_def_ref(
        &self,
        gdef: ItemId,
        particle: Option<ItemId>,
        marked: &mut Vec<ItemId>,
    ) -> Option<ItemId> {
        let mut p = particle;
        while let Some(part) = p {
            if let Some(term) = self.s.it(part).children {
                match self.s.it(term).typ {
                    TYPE_GROUP => {
                        if term == gdef {
                            return Some(part);
                        }
                        if !marked.contains(&term) {
                            if let Some(mg) = self.s.it(term).children {
                                marked.push(term);
                                let c = self.circ_model_group_def_ref(
                                    gdef,
                                    self.s.it(mg).children,
                                    marked,
                                );
                                marked.pop();
                                if c.is_some() {
                                    return c;
                                }
                            }
                        }
                    }
                    TYPE_SEQUENCE | TYPE_CHOICE | TYPE_ALL => {
                        let c =
                            self.circ_model_group_def_ref(gdef, self.s.it(term).children, marked);
                        if c.is_some() {
                            return c;
                        }
                    }
                    _ => {}
                }
            }
            p = self.s.it(part).next;
        }
        None
    }

    fn check_group_def_circular(&mut self, g: ItemId) {
        let mg = match self.s.it(g).children {
            Some(m) => m,
            None => return,
        };
        let first = self.s.it(mg).children;
        let mut marked = Vec::new();
        if let Some(circ) = self.circ_model_group_def_ref(g, first, &mut marked) {
            let m = format!(
                "Circular reference to the model group definition '{}' defined",
                format_qname(
                    self.s.it(g).target_ns.as_deref(),
                    self.s.it(g).name.as_deref()
                )
            );
            let node = self.s.component_node(circ);
            self.pcustom_err(E::SCHEMAP_MG_PROPS_CORRECT_2, None, node, &m);
            self.s.itm(circ).children = None;
        }
    }

    fn attr_group_circular_recur(
        &self,
        ctxt_gr: ItemId,
        list: &[ItemId],
        marked: &mut Vec<ItemId>,
    ) -> Option<ItemId> {
        for &r in list {
            let ri = self.s.it(r);
            if ri.typ == EXTRA_QNAMEREF && ri.item_type == TYPE_ATTRIBUTEGROUP {
                if let Some(gr) = ri.item {
                    if gr == ctxt_gr {
                        return Some(r);
                    }
                    if marked.contains(&gr) {
                        continue;
                    }
                    let g = self.s.it(gr);
                    if g.flags & ATTRGROUP_HAS_REFS != 0 {
                        marked.push(gr);
                        let c = self.attr_group_circular_recur(ctxt_gr, &g.attr_uses, marked);
                        marked.pop();
                        if c.is_some() {
                            return c;
                        }
                    }
                }
            }
        }
        None
    }

    fn check_attr_group_circular(&mut self, g: ItemId) {
        if self.s.it(g).flags & ATTRGROUP_HAS_REFS == 0 {
            return;
        }
        let list = self.s.it(g).attr_uses.clone();
        let mut marked = Vec::new();
        if let Some(circ) = self.attr_group_circular_recur(g, &list, &mut marked) {
            let m = format!(
                "Circular reference to the attribute group '{}' defined",
                self.s.component_qname(g)
            );
            let node = self.s.component_node(circ);
            self.pcustom_err(E::SCHEMAP_SRC_ATTRIBUTE_GROUP_3, None, node, &m);
            self.s.itm(circ).item = None;
        }
    }

    fn check_union_type_def_circular(&mut self, t: ItemId) {
        if !self.s.is_union(t) {
            return;
        }
        let members = self.s.it(t).member_types.clone();
        let mut marked = Vec::new();
        self.union_circ_recur(t, &members, &mut marked);
    }

    fn union_circ_recur(
        &mut self,
        ctx: ItemId,
        members: &[ItemId],
        marked: &mut Vec<ItemId>,
    ) -> i32 {
        for &m in members {
            let mut mt = Some(m);
            while let Some(x) = mt {
                if self.s.it(x).typ == TYPE_BASIC {
                    break;
                }
                if x == ctx {
                    self.pcustom_err(
                        E::SCHEMAP_SRC_SIMPLE_TYPE_4,
                        Some(ctx),
                        None,
                        "The union type definition is circular",
                    );
                    return E::SCHEMAP_SRC_SIMPLE_TYPE_4;
                }
                if self.s.is_union(x) && !marked.contains(&x) {
                    marked.push(x);
                    let sub = union_member_types(&self.s, x);
                    let r = self.union_circ_recur(ctx, &sub, marked);
                    marked.pop();
                    if r != 0 {
                        return r;
                    }
                }
                if self.s.it(x).base_type == Some(x) {
                    break;
                }
                mt = self.s.it(x).base_type;
            }
        }
        0
    }

    /// xmlSchemaModelGroupToModelGroupDefFixup
    fn model_group_to_model_group_def_fixup(&mut self, mg: ItemId) {
        let mut p = self.s.it(mg).children;
        while let Some(part) = p {
            if let Some(term) = self.s.it(part).children {
                if self.s.it(term).typ == TYPE_GROUP {
                    let m = self.s.it(term).children;
                    self.s.itm(part).children = m;
                }
            }
            p = self.s.it(part).next;
        }
    }

    // -----------------------------------------------------------------
    // Attribute groups and wildcards

    fn clone_wildcard_ns(&mut self, dest: ItemId, src: ItemId) {
        let (any, set, neg) = {
            let s = self.s.it(src);
            (s.any, s.ns_set.clone(), s.neg_ns_set.clone())
        };
        let d = self.s.itm(dest);
        d.any = any;
        d.ns_set = set;
        d.neg_ns_set = neg;
    }

    /// xmlSchemaUnionWildcards
    pub fn union_wildcards(&mut self, cw: ItemId, cur: ItemId) -> i32 {
        let c = self.s.it(cur).clone();
        let w = self.s.it(cw).clone();
        // 1 same value
        if w.any == c.any
            && w.ns_set.is_empty() == c.ns_set.is_empty()
            && w.neg_ns_set.is_none() == c.neg_ns_set.is_none()
            && (w.neg_ns_set.is_none() || w.neg_ns_set == c.neg_ns_set)
        {
            if !w.ns_set.is_empty() {
                if w.ns_set.iter().all(|v| c.ns_set.contains(v)) {
                    return 0;
                }
            } else {
                return 0;
            }
        }
        // 2
        if w.any != c.any {
            if !w.any {
                let d = self.s.itm(cw);
                d.any = true;
                d.ns_set.clear();
                d.neg_ns_set = None;
            }
            return 0;
        }
        // 3
        if !w.ns_set.is_empty() && !c.ns_set.is_empty() {
            for v in &c.ns_set {
                if !self.s.it(cw).ns_set.contains(v) {
                    self.s.itm(cw).ns_set.insert(0, v.clone());
                }
            }
            return 0;
        }
        // 4
        if let (Some(a), Some(b)) = (&w.neg_ns_set, &c.neg_ns_set) {
            if a != b {
                self.s.itm(cw).neg_ns_set = Some(None);
                return 0;
            }
        }
        // 5
        let five = (matches!(&w.neg_ns_set, Some(Some(_))) && !c.ns_set.is_empty())
            || (matches!(&c.neg_ns_set, Some(Some(_))) && !w.ns_set.is_empty());
        if five {
            let (set, neg) = if !w.ns_set.is_empty() {
                (w.ns_set.clone(), c.neg_ns_set.clone().unwrap())
            } else {
                (c.ns_set.clone(), w.neg_ns_set.clone().unwrap())
            };
            let mut ns_found = false;
            let mut absent_found = false;
            for v in &set {
                if v.is_none() {
                    absent_found = true;
                } else if *v == neg {
                    ns_found = true;
                }
                if ns_found && absent_found {
                    break;
                }
            }
            if ns_found && absent_found {
                let d = self.s.itm(cw);
                d.any = true;
                d.ns_set.clear();
                d.neg_ns_set = None;
            } else if ns_found && !absent_found {
                let d = self.s.itm(cw);
                d.ns_set.clear();
                d.neg_ns_set = Some(None);
            } else if !ns_found && absent_found {
                let node = self.s.it(cw).node;
                self.perr(
                    node,
                    E::SCHEMAP_UNION_NOT_EXPRESSIBLE,
                    "The union of the wildcard is not expressible.\n".into(),
                );
                return E::SCHEMAP_UNION_NOT_EXPRESSIBLE;
            } else if self.s.it(cw).neg_ns_set.is_none() {
                let d = self.s.itm(cw);
                d.ns_set.clear();
                d.neg_ns_set = c.neg_ns_set.clone();
            }
            return 0;
        }
        // 6
        let six = (matches!(&w.neg_ns_set, Some(None)) && !c.ns_set.is_empty())
            || (matches!(&c.neg_ns_set, Some(None)) && !w.ns_set.is_empty());
        if six {
            let set = if !w.ns_set.is_empty() {
                w.ns_set.clone()
            } else {
                c.ns_set.clone()
            };
            if set.iter().any(|v| v.is_none()) {
                let d = self.s.itm(cw);
                d.any = true;
                d.ns_set.clear();
                d.neg_ns_set = None;
                return 0;
            }
            if self.s.it(cw).neg_ns_set.is_none() {
                let d = self.s.itm(cw);
                d.ns_set.clear();
                d.neg_ns_set = Some(None);
            }
            return 0;
        }
        0
    }

    /// xmlSchemaIntersectWildcards
    pub fn intersect_wildcards(&mut self, cw: ItemId, cur: ItemId) -> i32 {
        let c = self.s.it(cur).clone();
        let w = self.s.it(cw).clone();
        if w.any == c.any
            && w.ns_set.is_empty() == c.ns_set.is_empty()
            && w.neg_ns_set.is_none() == c.neg_ns_set.is_none()
            && (w.neg_ns_set.is_none() || w.neg_ns_set == c.neg_ns_set)
        {
            if !w.ns_set.is_empty() {
                if w.ns_set.iter().all(|v| c.ns_set.contains(v)) {
                    return 0;
                }
            } else {
                return 0;
            }
        }
        if w.any != c.any && w.any {
            self.clone_wildcard_ns(cw, cur);
            return 0;
        }
        if (w.neg_ns_set.is_some() && !c.ns_set.is_empty())
            || (c.neg_ns_set.is_some() && !w.ns_set.is_empty())
        {
            let neg = if w.ns_set.is_empty() {
                let n = w.neg_ns_set.clone().unwrap();
                self.clone_wildcard_ns(cw, cur);
                n
            } else {
                c.neg_ns_set.clone().unwrap()
            };
            let d = self.s.itm(cw);
            if let Some(pos) = d.ns_set.iter().position(|v| v.is_none()) {
                d.ns_set.remove(pos);
            }
            if neg.is_some() {
                if let Some(pos) = d.ns_set.iter().position(|v| *v == neg) {
                    d.ns_set.remove(pos);
                }
            }
            return 0;
        }
        if !w.ns_set.is_empty() && !c.ns_set.is_empty() {
            let d = self.s.itm(cw);
            d.ns_set.retain(|v| c.ns_set.contains(v));
            return 0;
        }
        if let (Some(a), Some(b)) = (&w.neg_ns_set, &c.neg_ns_set) {
            if a != b && a.is_some() && b.is_some() {
                let node = self.s.it(cw).node;
                self.perr(
                    node,
                    E::SCHEMAP_INTERSECTION_NOT_EXPRESSIBLE,
                    "The intersection of the wildcard is not expressible.\n".into(),
                );
                return E::SCHEMAP_INTERSECTION_NOT_EXPRESSIBLE;
            }
            if a != b && a.is_none() {
                self.s.itm(cw).neg_ns_set = Some(b.clone());
            }
        }
        0
    }

    fn attr_use_decl_qname(&self, u: ItemId) -> (Option<String>, Option<String>) {
        match self.s.it(u).attr_decl {
            Some(d) => (self.s.it(d).name.clone(), self.s.it(d).target_ns.clone()),
            None => (None, None),
        }
    }

    /// xmlSchemaExpandAttributeGroupRefs: returns the new uses list, the
    /// complete wildcard and the prohibitions.
    fn expand_attribute_group_refs(
        &mut self,
        owner: ItemId,
        complete_wild: &mut Option<ItemId>,
        list: Vec<ItemId>,
        want_prohibs: bool,
    ) -> Result<(Vec<ItemId>, Vec<ItemId>), ()> {
        let mut created = complete_wild.is_some();
        let mut out: Vec<ItemId> = Vec::new();
        let mut prohibs: Vec<ItemId> = Vec::new();
        for u in list {
            let ut = self.s.it(u).typ;
            if ut == EXTRA_ATTR_USE_PROHIB {
                if !want_prohibs {
                    self.internal_err(
                        "xmlSchemaExpandAttributeGroupRefs",
                        "unexpected attr prohibition found",
                    );
                    return Err(());
                }
                prohibs.push(u);
                continue;
            }
            if ut == EXTRA_QNAMEREF && self.s.it(u).item_type == TYPE_ATTRIBUTEGROUP {
                let gr = match self.s.it(u).item {
                    Some(g) => g,
                    None => return Err(()),
                };
                if self.s.it(gr).flags & ATTRGROUP_WILDCARD_BUILDED == 0
                    && self.attribute_group_expand_refs(gr) == -1
                {
                    return Err(());
                }
                if let Some(gw) = self.s.it(gr).attribute_wildcard {
                    match *complete_wild {
                        None => *complete_wild = Some(gw),
                        Some(cw) => {
                            let mut cw = cw;
                            if !created {
                                let node = self.s.component_node(owner).unwrap_or(0);
                                let tmp = self.add_wildcard(TYPE_ANY_ATTRIBUTE, node);
                                self.clone_wildcard_ns(tmp, cw);
                                self.s.itm(tmp).process_contents = self.s.it(cw).process_contents;
                                cw = tmp;
                                *complete_wild = Some(tmp);
                                created = true;
                            }
                            if self.intersect_wildcards(cw, gw) == -1 {
                                return Err(());
                            }
                        }
                    }
                }
                let sub = self.s.it(gr).attr_uses.clone();
                out.extend(sub);
                continue;
            }
            out.push(u);
        }
        if want_prohibs && !prohibs.is_empty() && !out.is_empty() {
            let mut i = prohibs.len();
            while i > 0 {
                i -= 1;
                let p = prohibs[i];
                let (pn, pns) = (self.s.it(p).name.clone(), self.s.it(p).target_ns.clone());
                for &u in &out {
                    let (un, uns) = self.attr_use_decl_qname(u);
                    if un == pn && uns == pns {
                        let m = format!(
                            "Skipping pointless attribute use prohibition '{}', since a corresponding attribute use exists already in the type definition",
                            format_qname(pns.as_deref(), pn.as_deref())
                        );
                        let node = self.s.it(p).node;
                        self.custom_warning(E::SCHEMAP_WARN_ATTR_POINTLESS_PROH, node, &m);
                        prohibs.remove(i);
                        break;
                    }
                }
            }
        }
        Ok((out, prohibs))
    }

    /// xmlSchemaAttributeGroupExpandRefs
    fn attribute_group_expand_refs(&mut self, g: ItemId) -> i32 {
        if !self.s.it(g).has_attr_uses || self.s.it(g).flags & ATTRGROUP_WILDCARD_BUILDED != 0 {
            return 0;
        }
        self.s.itm(g).flags |= ATTRGROUP_WILDCARD_BUILDED;
        let list = self.s.it(g).attr_uses.clone();
        let mut w = self.s.it(g).attribute_wildcard;
        match self.expand_attribute_group_refs(g, &mut w, list, false) {
            Ok((uses, _)) => {
                self.s.itm(g).attr_uses = uses;
                self.s.itm(g).attribute_wildcard = w;
                0
            }
            Err(_) => -1,
        }
    }

    fn check_duplicate_uses(&mut self, owner: ItemId, ag: bool) {
        let mut uses = self.s.it(owner).attr_uses.clone();
        let mut has_id = false;
        let mut i = uses.len();
        while i > 0 {
            i -= 1;
            let u = uses[i];
            let (un, uns) = self.attr_use_decl_qname(u);
            let mut dup = false;
            for j in (0..i).rev() {
                let (tn, tns) = self.attr_use_decl_qname(uses[j]);
                if un == tn && uns == tns {
                    let m = format!("Duplicate {}", self.s.component_designation(u));
                    let node = if ag { self.s.it(owner).node } else { None };
                    self.custom_err(E::SCHEMAP_AG_PROPS_CORRECT, node, Some(owner), &m);
                    uses.remove(i);
                    dup = true;
                    break;
                }
            }
            if dup {
                continue;
            }
            let td = self.s.it(u).attr_decl.and_then(|d| self.s.it(d).subtypes);
            if td.is_some() && is_derived_from_builtin(&self.s, td, T::ID) {
                if has_id {
                    let m = format!(
                        "There must not exist more than one attribute declaration of type 'xs:ID' (or derived from 'xs:ID'). The {} violates this constraint",
                        self.s.component_designation(u)
                    );
                    let node = if ag { self.s.it(owner).node } else { None };
                    self.custom_err(E::SCHEMAP_AG_PROPS_CORRECT, node, Some(owner), &m);
                    uses.remove(i);
                }
                has_id = true;
            }
        }
        self.s.itm(owner).attr_uses = uses;
    }

    /// xmlSchemaCheckAGPropsCorrect
    fn check_ag_props_correct(&mut self, g: ItemId) {
        self.check_duplicate_uses(g, true);
    }

    // -----------------------------------------------------------------
    // Simple types

    fn fixup_simple_type_stage_one(&mut self, t: ItemId) -> i32 {
        if self.s.it(t).typ != TYPE_SIMPLE || self.s.it(t).flags & TYPE_FIXUP_1 != 0 {
            return 0;
        }
        self.s.itm(t).flags |= TYPE_FIXUP_1;
        if self.s.is_list(t) {
            if self.s.it(t).subtypes.is_none() {
                self.internal_err(
                    "xmlSchemaFixupSimpleTypeStageOne",
                    "list type has no item-type assigned",
                );
                return -1;
            }
        } else if self.s.is_union(t) {
            if self.s.it(t).member_types.is_empty() {
                self.internal_err(
                    "xmlSchemaFixupSimpleTypeStageOne",
                    "union type has no member-types assigned",
                );
                return -1;
            }
        } else {
            let base = match self.s.it(t).base_type {
                Some(b) => b,
                None => {
                    self.internal_err(
                        "xmlSchemaFixupSimpleTypeStageOne",
                        "type has no base-type assigned",
                    );
                    return -1;
                }
            };
            if self.s.it(base).typ != TYPE_BASIC
                && self.s.it(base).flags & TYPE_FIXUP_1 == 0
                && self.fixup_simple_type_stage_one(base) == -1
            {
                return -1;
            }
            if self.s.is_atomic(base) {
                self.s.itm(t).flags |= TYPE_VARIETY_ATOMIC;
            } else if self.s.is_list(base) {
                self.s.itm(t).flags |= TYPE_VARIETY_LIST;
                let st = self.s.it(base).subtypes;
                self.s.itm(t).subtypes = st;
            } else if self.s.is_union(base) {
                self.s.itm(t).flags |= TYPE_VARIETY_UNION;
            }
        }
        0
    }

    pub fn type_fixup(&mut self, t: ItemId) -> i32 {
        if !self.type_not_fixed(t) {
            return 0;
        }
        match self.s.it(t).typ {
            TYPE_COMPLEX => self.fixup_complex_type(t),
            TYPE_SIMPLE => self.fixup_simple_type_stage_two(t),
            _ => 0,
        }
    }

    fn final_contains(&self, t: ItemId, f: i32) -> bool {
        self.s.it(t).flags & f != 0
    }

    /// xmlSchemaCheckSTPropsCorrect
    fn check_st_props_correct(&mut self, t: ItemId) -> i32 {
        let base = match self.s.it(t).base_type {
            None => {
                self.pcustom_err(
                    E::SCHEMAP_ST_PROPS_CORRECT_1,
                    Some(t),
                    None,
                    "No base type existent",
                );
                return E::SCHEMAP_ST_PROPS_CORRECT_1;
            }
            Some(b) => b,
        };
        if !self.s.is_simple(base) {
            let m = format!(
                "The base type '{}' is not a simple type",
                self.s.component_qname(base)
            );
            self.pcustom_err(E::SCHEMAP_ST_PROPS_CORRECT_1, Some(t), None, &m);
            return E::SCHEMAP_ST_PROPS_CORRECT_1;
        }
        if (self.s.is_list(t) || self.s.is_union(t))
            && self.s.it(t).flags & TYPE_DERIVATION_METHOD_RESTRICTION == 0
            && !self.s.is_any_simple_type(base)
            && self.s.it(base).typ != TYPE_SIMPLE
        {
            let m = format!(
                "A type, derived by list or union, must have the simple ur-type definition as base type, not '{}'",
                self.s.component_qname(base)
            );
            self.pcustom_err(E::SCHEMAP_ST_PROPS_CORRECT_1, Some(t), None, &m);
            return E::SCHEMAP_ST_PROPS_CORRECT_1;
        }
        if !self.s.is_atomic(t) && !self.s.is_union(t) && !self.s.is_list(t) {
            self.pcustom_err(
                E::SCHEMAP_ST_PROPS_CORRECT_1,
                Some(t),
                None,
                "The variety is absent",
            );
            return E::SCHEMAP_ST_PROPS_CORRECT_1;
        }
        if self.final_contains(base, TYPE_FINAL_RESTRICTION) {
            let m = format!(
                "The 'final' of its base type '{}' must not contain 'restriction'",
                self.s.component_qname(base)
            );
            self.pcustom_err(E::SCHEMAP_ST_PROPS_CORRECT_3, Some(t), None, &m);
            return E::SCHEMAP_ST_PROPS_CORRECT_3;
        }
        0
    }

    /// xmlSchemaCheckCOSSTRestricts (the facet and variety checks)
    fn check_cos_st_restricts(&mut self, t: ItemId) -> i32 {
        let base = self.s.it(t).base_type.unwrap();
        let facet_types: Vec<i32> = self.s.it(t).facets.iter().map(|f| f.f.typ).collect();
        if self.s.is_atomic(t) {
            if !self.s.is_atomic(base) {
                let m = format!(
                    "The base type '{}' is not an atomic simple type",
                    self.s.component_qname(base)
                );
                self.pcustom_err(E::SCHEMAP_COS_ST_RESTRICTS_1_1, Some(t), None, &m);
                return E::SCHEMAP_COS_ST_RESTRICTS_1_1;
            }
            if self.final_contains(base, TYPE_FINAL_RESTRICTION) {
                let m = format!(
                    "The final of its base type '{}' must not contain 'restriction'",
                    self.s.component_qname(base)
                );
                self.pcustom_err(E::SCHEMAP_COS_ST_RESTRICTS_1_2, Some(t), None, &m);
                return E::SCHEMAP_COS_ST_RESTRICTS_1_2;
            }
            if !facet_types.is_empty() {
                let prim = match primitive_type(&self.s, t) {
                    Some(p) => p,
                    None => {
                        self.internal_err(
                            "xmlSchemaCheckCOSSTRestricts",
                            "failed to get primitive type",
                        );
                        return -1;
                    }
                };
                let mut ok = true;
                for ft in facet_types {
                    if !T::is_builtin_type_facet(self.s.it(prim).builtin_type, ft) {
                        ok = false;
                        self.illegal_facet_atomic_err(
                            E::SCHEMAP_COS_ST_RESTRICTS_1_3_1,
                            t,
                            prim,
                            ft,
                        );
                    }
                }
                if !ok {
                    return E::SCHEMAP_COS_ST_RESTRICTS_1_3_1;
                }
            }
        } else if self.s.is_list(t) {
            let item = match self.s.it(t).subtypes {
                Some(i) if self.s.is_simple(i) => i,
                _ => {
                    self.internal_err(
                        "xmlSchemaCheckCOSSTRestricts",
                        "failed to evaluate the item type",
                    );
                    return -1;
                }
            };
            if self.type_not_fixed(item) {
                self.type_fixup(item);
            }
            if !self.s.is_atomic(item) && !self.s.is_union(item) {
                let m = format!(
                    "The item type '{}' does not have a variety of atomic or union",
                    self.s.component_qname(item)
                );
                self.pcustom_err(E::SCHEMAP_COS_ST_RESTRICTS_2_1, Some(t), None, &m);
                return E::SCHEMAP_COS_ST_RESTRICTS_2_1;
            } else if self.s.is_union(item) {
                for m in self.s.it(item).member_types.clone() {
                    if !self.s.is_atomic(m) {
                        let msg = format!(
                            "The item type is a union type, but the member type '{}' of this item type is not atomic",
                            self.s.component_qname(m)
                        );
                        self.pcustom_err(E::SCHEMAP_COS_ST_RESTRICTS_2_1, Some(t), None, &msg);
                        return E::SCHEMAP_COS_ST_RESTRICTS_2_1;
                    }
                }
            }
            if self.s.is_any_simple_type(base) {
                if self.final_contains(item, TYPE_FINAL_LIST) {
                    let m = format!(
                        "The final of its item type '{}' must not contain 'list'",
                        self.s.component_qname(item)
                    );
                    self.pcustom_err(E::SCHEMAP_COS_ST_RESTRICTS_2_3_1_1, Some(t), None, &m);
                    return E::SCHEMAP_COS_ST_RESTRICTS_2_3_1_1;
                }
                for ft in facet_types {
                    if ft != T::FACET_WHITESPACE {
                        self.illegal_facet_list_union_err(
                            E::SCHEMAP_COS_ST_RESTRICTS_2_3_1_2,
                            t,
                            ft,
                        );
                        return E::SCHEMAP_COS_ST_RESTRICTS_2_3_1_2;
                    }
                }
            } else {
                if !self.s.is_list(base) {
                    let m = format!(
                        "The base type '{}' must be a list type",
                        self.s.component_qname(base)
                    );
                    self.pcustom_err(E::SCHEMAP_COS_ST_RESTRICTS_2_3_2_1, Some(t), None, &m);
                    return E::SCHEMAP_COS_ST_RESTRICTS_2_3_2_1;
                }
                if self.final_contains(base, TYPE_FINAL_RESTRICTION) {
                    let m = format!(
                        "The 'final' of the base type '{}' must not contain 'restriction'",
                        self.s.component_qname(base)
                    );
                    self.pcustom_err(E::SCHEMAP_COS_ST_RESTRICTS_2_3_2_2, Some(t), None, &m);
                    return E::SCHEMAP_COS_ST_RESTRICTS_2_3_2_2;
                }
                let mut ok = true;
                for ft in facet_types {
                    if !matches!(
                        ft,
                        T::FACET_LENGTH
                            | T::FACET_MINLENGTH
                            | T::FACET_MAXLENGTH
                            | T::FACET_WHITESPACE
                            | T::FACET_PATTERN
                            | T::FACET_ENUMERATION
                    ) {
                        self.illegal_facet_list_union_err(
                            E::SCHEMAP_COS_ST_RESTRICTS_2_3_2_4,
                            t,
                            ft,
                        );
                        ok = false;
                    }
                }
                if !ok {
                    return E::SCHEMAP_COS_ST_RESTRICTS_2_3_2_4;
                }
            }
        } else if self.s.is_union(t) {
            for m in self.s.it(t).member_types.clone() {
                if self.type_not_fixed(m) {
                    self.type_fixup(m);
                }
                if !self.s.is_atomic(m) && !self.s.is_list(m) {
                    let msg = format!(
                        "The member type '{}' is neither an atomic, nor a list type",
                        self.s.component_qname(m)
                    );
                    self.pcustom_err(E::SCHEMAP_COS_ST_RESTRICTS_3_1, Some(t), None, &msg);
                    return E::SCHEMAP_COS_ST_RESTRICTS_3_1;
                }
            }
            if self.s.it(base).builtin_type == T::ANYSIMPLETYPE {
                for m in self.s.it(t).member_types.clone() {
                    if self.final_contains(m, TYPE_FINAL_UNION) {
                        let msg = format!(
                            "The 'final' of member type '{}' contains 'union'",
                            self.s.component_qname(m)
                        );
                        self.pcustom_err(E::SCHEMAP_COS_ST_RESTRICTS_3_3_1, Some(t), None, &msg);
                        return E::SCHEMAP_COS_ST_RESTRICTS_3_3_1;
                    }
                }
                if !self.s.it(t).facet_set.is_empty() {
                    self.pcustom_err(
                        E::SCHEMAP_COS_ST_RESTRICTS_3_3_1_2,
                        Some(t),
                        None,
                        "No facets allowed",
                    );
                    return E::SCHEMAP_COS_ST_RESTRICTS_3_3_1_2;
                }
            } else {
                if !self.s.is_union(base) {
                    let m = format!(
                        "The base type '{}' is not a union type",
                        self.s.component_qname(base)
                    );
                    self.pcustom_err(E::SCHEMAP_COS_ST_RESTRICTS_3_3_2_1, Some(t), None, &m);
                    return E::SCHEMAP_COS_ST_RESTRICTS_3_3_2_1;
                }
                if self.final_contains(base, TYPE_FINAL_RESTRICTION) {
                    let m = format!(
                        "The 'final' of its base type '{}' must not contain 'restriction'",
                        self.s.component_qname(base)
                    );
                    self.pcustom_err(E::SCHEMAP_COS_ST_RESTRICTS_3_3_2_2, Some(t), None, &m);
                    return E::SCHEMAP_COS_ST_RESTRICTS_3_3_2_2;
                }
                let mut ok = true;
                for ft in facet_types {
                    if ft != T::FACET_PATTERN && ft != T::FACET_ENUMERATION {
                        self.illegal_facet_list_union_err(
                            E::SCHEMAP_COS_ST_RESTRICTS_3_3_2_4,
                            t,
                            ft,
                        );
                        ok = false;
                    }
                }
                if !ok {
                    return E::SCHEMAP_COS_ST_RESTRICTS_3_3_2_4;
                }
            }
        }
        0
    }

    /// xmlSchemaCheckFacet
    fn check_facet(&mut self, t: ItemId, fi: usize) -> i32 {
        let f = self.s.it(t).facets[fi].clone();
        let node = f.node;
        match f.f.typ {
            T::FACET_MININCLUSIVE
            | T::FACET_MINEXCLUSIVE
            | T::FACET_MAXINCLUSIVE
            | T::FACET_MAXEXCLUSIVE
            | T::FACET_ENUMERATION => {
                let base = if self.s.it(t).typ != TYPE_BASIC {
                    match self.s.it(t).base_type {
                        Some(b) => b,
                        None => {
                            self.internal_err(
                                "xmlSchemaCheckFacet",
                                "a type user derived type has no base type",
                            );
                            return -1;
                        }
                    }
                } else {
                    t
                };
                let (ret, val) = self.pcheck_simple(node, base, &f.f.value, true);
                if ret != 0 {
                    if ret < 0 {
                        let m = format!(
                            "Internal error: xmlSchemaCheckFacet, failed to validate the value '{}' of the facet '{}' against the base type",
                            f.f.value,
                            facet_type_to_string(f.f.typ)
                        );
                        self.custom_err(E::SCHEMAP_INTERNAL, node, None, &m);
                        return -1;
                    }
                    let m = format!(
                        "The value '{}' of the facet does not validate against the base type '{}'",
                        f.f.value,
                        format_qname(
                            self.s.it(base).target_ns.as_deref(),
                            self.s.it(base).name.as_deref()
                        )
                    );
                    self.custom_err(E::SCHEMAP_INVALID_FACET_VALUE, node, None, &m);
                    return E::SCHEMAP_INVALID_FACET_VALUE;
                }
                let sf = &mut self.s.itm(t).facets[fi];
                if let Some(v) = val {
                    sf.f.val = v.first().cloned();
                    sf.vals = Some(v);
                }
                0
            }
            T::FACET_PATTERN => match regexp_compile(&f.f.value) {
                Ok(re) => {
                    self.s.itm(t).facets[fi].f.regexp = Some(Rc::new(re));
                    0
                }
                Err(msgs) => {
                    // the regexp module raises its own (generic) errors
                    for m in msgs {
                        let mut e = XmlError::simple(
                            E::FROM_REGEXP,
                            E::REGEXP_COMPILE_ERROR,
                            crate::parser::ERR_FATAL,
                            format!("{}\n", m),
                        );
                        e.formatted = vec![m];
                        self.errors.push(e);
                    }
                    let m = format!(
                        "The value '{}' of the facet 'pattern' is not a valid regular expression",
                        f.f.value
                    );
                    self.custom_err(E::SCHEMAP_REGEXP_INVALID, node, Some(t), &m);
                    E::SCHEMAP_REGEXP_INVALID
                }
            },
            T::FACET_TOTALDIGITS
            | T::FACET_FRACTIONDIGITS
            | T::FACET_LENGTH
            | T::FACET_MAXLENGTH
            | T::FACET_MINLENGTH => {
                let vt = if f.f.typ == T::FACET_TOTALDIGITS {
                    T::PINTEGER
                } else {
                    T::NNINTEGER
                };
                let (ret, val) = T::val_predef_type_node(vt, &f.f.value, true, None);
                if ret != 0 {
                    if ret < 0 {
                        self.internal_err("xmlSchemaCheckFacet", "validating facet value");
                        return -1;
                    }
                    let m = format!(
                        "The value '{}' of the facet '{}' is not a valid '{}'",
                        f.f.value,
                        facet_type_to_string(f.f.typ),
                        if f.f.typ != T::FACET_TOTALDIGITS {
                            "nonNegativeInteger"
                        } else {
                            "positiveInteger"
                        }
                    );
                    self.custom_err(E::SCHEMAP_INVALID_FACET_VALUE, node, Some(t), &m);
                    return E::SCHEMAP_INVALID_FACET_VALUE;
                }
                self.s.itm(t).facets[fi].f.val = val;
                0
            }
            T::FACET_WHITESPACE => {
                let ws = match f.f.value.as_str() {
                    "preserve" => FACET_PRESERVE,
                    "replace" => FACET_REPLACE,
                    "collapse" => FACET_COLLAPSE,
                    _ => {
                        let m = format!(
                            "The value '{}' of the facet 'whitespace' is not valid",
                            f.f.value
                        );
                        self.custom_err(E::SCHEMAP_INVALID_FACET_VALUE, node, Some(t), &m);
                        return E::SCHEMAP_INVALID_FACET_VALUE;
                    }
                };
                self.s.itm(t).facets[fi].f.whitespace = ws;
                0
            }
            _ => 0,
        }
    }

    /// "facet 'x'" (xmlSchemaFormatItemForReport of a facet)
    fn facet_des(&self, f: (ItemId, usize)) -> String {
        format!(
            "facet '{}'",
            facet_type_to_string(self.s.it(f.0).facets[f.1].f.typ)
        )
    }

    fn facet_restr_err(&mut self, f: (ItemId, usize), msg: &str) {
        let node = self.s.it(f.0).facets[f.1].node;
        let m = format!("{}: {}.\n", self.facet_des(f), msg);
        self.perr(node, E::SCHEMAP_INVALID_FACET_VALUE, m);
    }

    /// xmlSchemaDeriveFacetErr
    fn derive_facet_err(
        &mut self,
        f1: (ItemId, usize),
        f2: (ItemId, usize),
        less_greater: i32,
        or_equal: bool,
        of_base: bool,
    ) {
        let t1 = self.s.it(f1.0).facets[f1.1].f.typ;
        let t2 = self.s.it(f2.0).facets[f2.1].f.typ;
        let mut msg = format!("'{}' has to be", facet_type_to_string(t1));
        if less_greater == 0 {
            msg.push_str(" equal to");
        }
        if less_greater == 1 {
            msg.push_str(" greater than");
        } else {
            msg.push_str(" less than");
        }
        if or_equal {
            msg.push_str(" or equal to");
        }
        msg.push_str(" '");
        msg.push_str(facet_type_to_string(t2));
        if of_base {
            msg.push_str("' of the base type");
        } else {
            msg.push('\'');
        }
        let m = format!("{}: {}.\n", self.facet_des(f1), msg);
        self.perr(None, E::SCHEMAP_INVALID_FACET_VALUE, m);
    }

    fn fcmp(&self, a: (ItemId, usize), b: (ItemId, usize)) -> i32 {
        let va = &self.s.it(a.0).facets[a.1].f.val;
        let vb = &self.s.it(b.0).facets[b.1].f.val;
        match (va, vb) {
            (Some(x), Some(y)) => T::compare_values(x, y),
            _ => -2,
        }
    }

    fn ffixed(&self, f: (ItemId, usize)) -> bool {
        self.s.it(f.0).facets[f.1].f.fixed
    }

    /// xmlSchemaDeriveAndValidateFacets
    fn derive_facets(&mut self, t: ItemId) -> i32 {
        let base = self.s.it(t).base_type.unwrap();
        let tset = self.s.it(t).facet_set.clone();
        let bset = self.s.it(base).facet_set.clone();
        if tset.is_empty() && bset.is_empty() {
            return 0;
        }
        let find = |s: &Schema, set: &[(ItemId, usize)], ft: i32| -> Option<(ItemId, usize)> {
            let mut r = None;
            for &(o, i) in set {
                if s.it(o).facets[i].f.typ == ft {
                    r = Some((o, i));
                }
            }
            r
        };
        let mut flength = find(&self.s, &tset, T::FACET_LENGTH);
        let mut fminlen = find(&self.s, &tset, T::FACET_MINLENGTH);
        let fmininc = find(&self.s, &tset, T::FACET_MININCLUSIVE);
        let fminexc = find(&self.s, &tset, T::FACET_MINEXCLUSIVE);
        let mut fmaxlen = find(&self.s, &tset, T::FACET_MAXLENGTH);
        let fmaxinc = find(&self.s, &tset, T::FACET_MAXINCLUSIVE);
        let fmaxexc = find(&self.s, &tset, T::FACET_MAXEXCLUSIVE);
        let mut ftotdig = find(&self.s, &tset, T::FACET_TOTALDIGITS);
        let mut ffracdig = find(&self.s, &tset, T::FACET_FRACTIONDIGITS);
        let bflength = find(&self.s, &bset, T::FACET_LENGTH);
        let bfminlen = find(&self.s, &bset, T::FACET_MINLENGTH);
        let bfmininc = find(&self.s, &bset, T::FACET_MININCLUSIVE);
        let bfminexc = find(&self.s, &bset, T::FACET_MINEXCLUSIVE);
        let bfmaxlen = find(&self.s, &bset, T::FACET_MAXLENGTH);
        let bfmaxinc = find(&self.s, &bset, T::FACET_MAXINCLUSIVE);
        let bfmaxexc = find(&self.s, &bset, T::FACET_MAXEXCLUSIVE);
        let bftotdig = find(&self.s, &bset, T::FACET_TOTALDIGITS);
        let bffracdig = find(&self.s, &bset, T::FACET_FRACTIONDIGITS);
        macro_rules! cmp {
            ($a:expr, $b:expr) => {{
                let r = self.fcmp($a, $b);
                if r == -2 {
                    self.internal_err("xmlSchemaDeriveAndValidateFacets", "an error occurred");
                    return -1;
                }
                r
            }};
        }
        const FIXED: &str = "The base type's facet is 'fixed', thus the value must not differ";
        if let Some(fl) = flength {
            if fminlen.is_some() || fmaxlen.is_some() {
                self.facet_restr_err(fl, "It is an error for both 'length' and either of 'minLength' or 'maxLength' to be specified on the same type definition");
            }
        }
        for (a, b) in [(fmaxinc, fmaxexc), (fmininc, fminexc)] {
            if let (Some(a), Some(b)) = (a, b) {
                let m = format!(
                    "It is an error for both '{}' and '{}' to be specified on the same type definition",
                    facet_type_to_string(self.s.it(a.0).facets[a.1].f.typ),
                    facet_type_to_string(self.s.it(b.0).facets[b.1].f.typ)
                );
                self.facet_restr_err(a, &m);
            }
        }
        if let (Some(f), Some(bf)) = (flength, bflength) {
            let res = cmp!(f, bf);
            if res != 0 {
                self.derive_facet_err(f, bf, 0, false, true);
            }
            if res != 0 && self.ffixed(bf) {
                self.facet_restr_err(f, FIXED);
            }
        }
        if let (Some(f), Some(bf)) = (fminlen, bfminlen) {
            let res = cmp!(f, bf);
            if res == -1 {
                self.derive_facet_err(f, bf, 1, true, true);
            }
            if res != 0 && self.ffixed(bf) {
                self.facet_restr_err(f, FIXED);
            }
        }
        if let (Some(f), Some(bf)) = (fmaxlen, bfmaxlen) {
            let res = cmp!(f, bf);
            if res == 1 {
                self.derive_facet_err(f, bf, -1, true, true);
            }
            if res != 0 && self.ffixed(bf) {
                self.facet_restr_err(f, FIXED);
            }
        }
        if flength.is_none() {
            flength = bflength;
        }
        if let Some(fl) = flength {
            if fminlen.is_none() {
                fminlen = bfminlen;
            }
            if let Some(fm) = fminlen {
                if cmp!(fl, fm) == -1 {
                    self.derive_facet_err(fl, fm, 1, true, false);
                }
            }
            if fmaxlen.is_none() {
                fmaxlen = bfmaxlen;
            }
            if let Some(fm) = fmaxlen {
                if cmp!(fl, fm) == 1 {
                    self.derive_facet_err(fl, fm, -1, true, false);
                }
            }
        }
        if let Some(f) = fmaxinc {
            if let Some(o) = fmininc {
                if cmp!(f, o) == -1 {
                    self.derive_facet_err(f, o, 1, true, false);
                }
            }
            if let Some(b) = bfmaxinc {
                let res = cmp!(f, b);
                if res == 1 {
                    self.derive_facet_err(f, b, -1, true, true);
                }
                if res != 0 && self.ffixed(b) {
                    self.facet_restr_err(f, FIXED);
                }
            }
            if let Some(b) = bfmaxexc {
                if cmp!(f, b) != -1 {
                    self.derive_facet_err(f, b, -1, false, true);
                }
            }
            if let Some(b) = bfmininc {
                if cmp!(f, b) == -1 {
                    self.derive_facet_err(f, b, 1, true, true);
                }
            }
            if let Some(b) = bfminexc {
                if cmp!(f, b) != 1 {
                    self.derive_facet_err(f, b, 1, false, true);
                }
            }
        }
        if let Some(f) = fmaxexc {
            if let Some(o) = fminexc {
                if cmp!(f, o) == -1 {
                    self.derive_facet_err(f, o, 1, true, false);
                }
            }
            if let Some(b) = bfmaxexc {
                let res = cmp!(f, b);
                if res == 1 {
                    self.derive_facet_err(f, b, -1, true, true);
                }
                if res != 0 && self.ffixed(b) {
                    self.facet_restr_err(f, FIXED);
                }
            }
            if let Some(b) = bfmaxinc {
                if cmp!(f, b) == 1 {
                    self.derive_facet_err(f, b, -1, true, true);
                }
            }
            if let Some(b) = bfmininc {
                if cmp!(f, b) != 1 {
                    self.derive_facet_err(f, b, 1, false, true);
                }
            }
            if let Some(b) = bfminexc {
                if cmp!(f, b) != 1 {
                    self.derive_facet_err(f, b, 1, false, true);
                }
            }
        }
        if let Some(f) = fminexc {
            if let Some(o) = fmaxinc {
                if cmp!(f, o) != -1 {
                    self.derive_facet_err(f, o, -1, false, false);
                }
            }
            if let Some(b) = bfminexc {
                let res = cmp!(f, b);
                if res == -1 {
                    self.derive_facet_err(f, b, 1, true, true);
                }
                if res != 0 && self.ffixed(b) {
                    self.facet_restr_err(f, FIXED);
                }
            }
            if let Some(b) = bfmaxinc {
                if cmp!(f, b) == 1 {
                    self.derive_facet_err(f, b, -1, true, true);
                }
            }
            if let Some(b) = bfmininc {
                if cmp!(f, b) == -1 {
                    self.derive_facet_err(f, b, 1, true, true);
                }
            }
            if let Some(b) = bfmaxexc {
                if cmp!(f, b) != -1 {
                    self.derive_facet_err(f, b, -1, false, true);
                }
            }
        }
        if let Some(f) = fmininc {
            if let Some(o) = fmaxexc {
                if cmp!(f, o) != -1 {
                    self.derive_facet_err(f, o, -1, false, false);
                }
            }
            if let Some(b) = bfmininc {
                let res = cmp!(f, b);
                if res == -1 {
                    self.derive_facet_err(f, b, 1, true, true);
                }
                if res != 0 && self.ffixed(b) {
                    self.facet_restr_err(f, FIXED);
                }
            }
            if let Some(b) = bfmaxinc {
                if cmp!(f, b) == 1 {
                    self.derive_facet_err(f, b, -1, true, true);
                }
            }
            if let Some(b) = bfminexc {
                if cmp!(f, b) != 1 {
                    self.derive_facet_err(f, b, 1, false, true);
                }
            }
            if let Some(b) = bfmaxexc {
                if cmp!(f, b) != -1 {
                    self.derive_facet_err(f, b, -1, false, true);
                }
            }
        }
        if let (Some(f), Some(b)) = (ftotdig, bftotdig) {
            let res = cmp!(f, b);
            if res == 1 {
                self.derive_facet_err(f, b, -1, true, true);
            }
            if res != 0 && self.ffixed(b) {
                self.facet_restr_err(f, FIXED);
            }
        }
        if let (Some(f), Some(b)) = (ffracdig, bffracdig) {
            let res = cmp!(f, b);
            if res == 1 {
                self.derive_facet_err(f, b, -1, true, true);
            }
            if res != 0 && self.ffixed(b) {
                self.facet_restr_err(f, FIXED);
            }
        }
        if ftotdig.is_none() {
            ftotdig = bftotdig;
        }
        if ffracdig.is_none() {
            ffracdig = bffracdig;
        }
        if let (Some(fd), Some(td)) = (ffracdig, ftotdig) {
            if cmp!(fd, td) == 1 {
                self.derive_facet_err(fd, td, -1, true, false);
            }
        }
        for (bt, bi) in bset {
            let bf = self.s.it(bt).facets[bi].f.clone();
            if bf.typ == T::FACET_PATTERN || bf.typ == T::FACET_ENUMERATION {
                continue;
            }
            let cur = self.s.it(t).facet_set.clone();
            let mut dup = false;
            for (ot, oi) in cur {
                let f = self.s.it(ot).facets[oi].f.clone();
                if f.typ == bf.typ {
                    if f.typ == T::FACET_WHITESPACE {
                        if f.whitespace < bf.whitespace {
                            self.facet_restr_err((ot, oi), "The 'whitespace' value has to be equal to or stronger than the 'whitespace' value of the base type");
                        }
                        if bf.fixed && f.whitespace != bf.whitespace {
                            self.facet_restr_err((ot, oi), FIXED);
                        }
                    }
                    dup = true;
                    break;
                }
            }
            if !dup {
                self.s.itm(t).facet_set.push((bt, bi));
            }
        }
        0
    }

    fn fixup_whitespace(&mut self, t: ItemId) {
        if self.s.is_list(t) {
            self.s.itm(t).flags |= TYPE_WHITESPACE_COLLAPSE;
            return;
        } else if self.s.is_union(t) {
            return;
        }
        for &(ft, fi) in &self.s.it(t).facet_set.clone() {
            let f = &self.s.it(ft).facets[fi].f;
            if f.typ == T::FACET_WHITESPACE {
                let fl = match f.whitespace {
                    FACET_PRESERVE => TYPE_WHITESPACE_PRESERVE,
                    FACET_REPLACE => TYPE_WHITESPACE_REPLACE,
                    _ => TYPE_WHITESPACE_COLLAPSE,
                };
                self.s.itm(t).flags |= fl;
                return;
            }
        }
        let mut anc = self.s.it(t).base_type;
        while let Some(a) = anc {
            if self.s.it(a).builtin_type == T::ANYTYPE {
                break;
            }
            if self.s.it(a).typ == TYPE_BASIC {
                let fl = match self.s.it(a).builtin_type {
                    T::NORMSTRING => TYPE_WHITESPACE_REPLACE,
                    T::STRING | T::ANYSIMPLETYPE => TYPE_WHITESPACE_PRESERVE,
                    _ => TYPE_WHITESPACE_COLLAPSE,
                };
                self.s.itm(t).flags |= fl;
                break;
            }
            anc = self.s.it(a).base_type;
        }
    }

    fn fixup_optim_facets(&mut self, t: ItemId) {
        let base = self.s.it(t).base_type.unwrap();
        let bf = self.s.it(base).flags;
        let mut has = bf & TYPE_HAS_FACETS != 0;
        let mut need_val = false;
        let mut norm_val = false;
        if has {
            need_val = bf & TYPE_FACETSNEEDVALUE != 0;
            norm_val = bf & TYPE_NORMVALUENEEDED != 0;
        }
        for f in &self.s.it(t).facets {
            match f.f.typ {
                T::FACET_WHITESPACE => {}
                T::FACET_PATTERN => {
                    norm_val = true;
                    has = true;
                }
                T::FACET_ENUMERATION => {
                    need_val = true;
                    norm_val = true;
                    has = true;
                }
                _ => has = true,
            }
        }
        if norm_val {
            self.s.itm(t).flags |= TYPE_NORMVALUENEEDED;
        }
        if need_val {
            self.s.itm(t).flags |= TYPE_FACETSNEEDVALUE;
        }
        if has {
            self.s.itm(t).flags |= TYPE_HAS_FACETS;
        }
        if has && !need_val && self.s.is_atomic(t) {
            if let Some(p) = primitive_type(&self.s, t) {
                let bt = self.s.it(p).builtin_type;
                if bt != T::ANYSIMPLETYPE && bt != T::STRING {
                    self.s.itm(t).flags |= TYPE_FACETSNEEDVALUE;
                }
            }
        }
    }

    /// xmlSchemaFinishMemberTypeDefinitionsProperty
    fn finish_member_types(&mut self, t: ItemId) {
        let members = self.s.it(t).member_types.clone();
        let mut out = Vec::new();
        for m in members {
            if self.type_not_fixed(m) {
                self.type_fixup(m);
            }
            if self.s.is_union(m) {
                let sub = union_member_types(&self.s, m);
                if sub.is_empty() {
                    out.push(m);
                } else {
                    out.extend(sub);
                }
            } else {
                out.push(m);
            }
        }
        self.s.itm(t).member_types = out;
    }

    /// xmlSchemaFixupSimpleTypeStageTwo
    fn fixup_simple_type_stage_two(&mut self, t: ItemId) -> i32 {
        let olderrs = self.nberrors;
        if self.s.it(t).typ != TYPE_SIMPLE {
            return -1;
        }
        if !self.type_not_fixed(t) {
            return 0;
        }
        self.s.itm(t).flags |= TYPE_INTERNAL_RESOLVED;
        self.s.itm(t).content_type = CONTENT_SIMPLE;
        let base = match self.s.it(t).base_type {
            None => {
                self.internal_err("xmlSchemaFixupSimpleTypeStageTwo", "missing baseType");
                return -1;
            }
            Some(b) => b,
        };
        if self.type_not_fixed(base) {
            self.type_fixup(base);
        }
        if !self.s.it(t).member_types.is_empty() {
            self.finish_member_types(t);
        }
        macro_rules! herr {
            ($r:expr) => {
                let r = $r;
                if r == -1 {
                    return -1;
                }
                if r != 0 {
                    if olderrs != self.nberrors {
                        return self.err;
                    }
                    return 0;
                }
            };
        }
        herr!(self.check_st_props_correct(t));
        herr!(self.check_cos_st_restricts(t));
        let n = self.s.it(t).facets.len();
        let fe = self.nberrors;
        for i in 0..n {
            if self.check_facet(t, i) == -1 {
                return -1;
            }
        }
        if fe != self.nberrors {
            return self.err;
        }
        if (!self.s.it(t).facet_set.is_empty() || !self.s.it(base).facet_set.is_empty())
            && self.derive_facets(t) == -1
        {
            return -1;
        }
        self.fixup_whitespace(t);
        self.fixup_optim_facets(t);
        if olderrs != self.nberrors {
            return self.err;
        }
        0
    }

    // -----------------------------------------------------------------
    // Attribute declarations and uses

    /// xmlSchemaCheckAttrPropsCorrect
    fn check_attr_props_correct(&mut self, a: ItemId) -> i32 {
        let td = match self.s.it(a).subtypes {
            Some(t) => t,
            None => return 0,
        };
        let dv = match self.s.it(a).def_value.clone() {
            Some(v) => v,
            None => return 0,
        };
        if is_derived_from_builtin(&self.s, Some(td), T::ID) {
            self.custom_err(E::SCHEMAP_A_PROPS_CORRECT_3, None, Some(a), "Value constraints are not allowed if the type definition is or is derived from xs:ID");
            return self.err;
        }
        let node = self.s.it(a).node;
        let (ret, val) = self.pcheck_simple(node, td, &dv, true);
        if ret != 0 {
            if ret < 0 {
                self.internal_err(
                    "xmlSchemaCheckAttrPropsCorrect",
                    "calling xmlSchemaVCheckCVCSimpleType()",
                );
                return -1;
            }
            self.custom_err(
                E::SCHEMAP_A_PROPS_CORRECT_2,
                None,
                Some(a),
                "The value of the value constraint is not valid",
            );
            return self.err;
        }
        self.s.itm(a).def_val = val.and_then(|v| v.into_iter().next());
        0
    }

    /// xmlSchemaCheckAttrUsePropsCorrect
    fn check_attr_use_props_correct(&mut self, u: ItemId) -> i32 {
        let decl = match self.s.it(u).attr_decl {
            Some(d) if self.s.it(d).typ == TYPE_ATTRIBUTE => d,
            _ => return 0,
        };
        if self.s.it(u).def_value.is_none() {
            return 0;
        }
        if self.s.it(decl).def_value.is_some()
            && self.s.it(decl).flags & ATTR_FIXED != 0
            && self.s.it(u).flags & ATTR_USE_FIXED == 0
        {
            self.pcustom_err(E::SCHEMAP_AU_PROPS_CORRECT_2, Some(u), None, "The attribute declaration has a 'fixed' value constraint , thus the attribute use must also have a 'fixed' value constraint");
            return self.err;
        }
        // NOTE: libxml2 checks use->defVal (never computed at this point),
        // so the value constraint of the use itself is not validated here.
        if let Some(_ddv) = &self.s.it(decl).def_val {
            if self.s.it(decl).flags & ATTR_USE_FIXED == 0 {
                // use->defVal is NULL: xmlSchemaAreValuesEqual(NULL, ...) is 0
                let m = format!(
                    "The 'fixed' value constraint of the attribute use must match the attribute declaration's value constraint '{}'",
                    self.s.it(decl).def_value.clone().unwrap_or_default()
                );
                self.pcustom_err(E::SCHEMAP_AU_PROPS_CORRECT_2, Some(u), None, &m);
                return self.err;
            }
        }
        0
    }

    // -----------------------------------------------------------------
    // Complex types

    /// xmlSchemaIsParticleEmptiable
    pub fn particle_emptiable(&self, p: Option<ItemId>) -> bool {
        is_particle_emptiable(&self.s, p)
    }

    /// xmlSchemaCheckSRCCT
    fn check_src_ct(&mut self, t: ItemId) -> i32 {
        let base = self.s.it(t).base_type.unwrap();
        let bq = format_qname(
            self.s.it(base).target_ns.as_deref(),
            self.s.it(base).name.as_deref(),
        );
        let mut ret = 0;
        if !self.s.has_simple_content(t) {
            if !self.s.is_complex(base) {
                let m = format!(
                    "If using <complexContent>, the base type is expected to be a complex type. The base type '{}' is a simple type",
                    bq
                );
                let node = self.s.it(t).node;
                self.pcustom_err(E::SCHEMAP_SRC_CT_1, Some(t), node, &m);
                return E::SCHEMAP_SRC_CT_1;
            }
        } else {
            let is_ext = self.s.it(t).flags & TYPE_DERIVATION_METHOD_EXTENSION != 0;
            let is_restr = self.s.it(t).flags & TYPE_DERIVATION_METHOD_RESTRICTION != 0;
            if self.s.is_simple(base) {
                if !is_ext {
                    let m = format!(
                        "If using <simpleContent> and <restriction>, the base type must be a complex type. The base type '{}' is a simple type",
                        bq
                    );
                    self.pcustom_err(E::SCHEMAP_SRC_CT_1, Some(t), None, &m);
                    return E::SCHEMAP_SRC_CT_1;
                }
            } else {
                let bct = self.s.it(base).content_type;
                if bct == CONTENT_SIMPLE || bct == CONTENT_BASIC {
                    if self.s.it(base).content_type_def.is_none() {
                        let m = format!(
                            "Internal error: xmlSchemaCheckSRCCT, '{}', base type has no content type",
                            self.s.it(t).name.clone().unwrap_or_default()
                        );
                        self.pcustom_err(E::SCHEMAP_INTERNAL, Some(t), None, &m);
                        return -1;
                    }
                } else if bct == CONTENT_MIXED && is_restr {
                    let bp = self.s.it(base).subtypes;
                    if !self.particle_emptiable(bp) {
                        ret = E::SCHEMAP_SRC_CT_1;
                    } else if self.s.it(t).content_type_def.is_none() {
                        let m = format!(
                            "A <simpleType> is expected among the children of <restriction>, if <simpleContent> is used and the base type '{}' is a complex type",
                            bq
                        );
                        self.pcustom_err(E::SCHEMAP_SRC_CT_1, Some(t), None, &m);
                        return E::SCHEMAP_SRC_CT_1;
                    }
                } else {
                    ret = E::SCHEMAP_SRC_CT_1;
                }
            }
            if ret > 0 {
                if is_restr {
                    let m = format!(
                        "If <simpleContent> and <restriction> is used, the base type must be a simple type or a complex type with mixed content and particle emptiable. The base type '{}' is none of those",
                        bq
                    );
                    self.pcustom_err(E::SCHEMAP_SRC_CT_1, Some(t), None, &m);
                } else {
                    let m = format!(
                        "If <simpleContent> and <extension> is used, the base type must be a simple type. The base type '{}' is a complex type",
                        bq
                    );
                    self.pcustom_err(E::SCHEMAP_SRC_CT_1, Some(t), None, &m);
                }
            }
        }
        ret
    }

    /// xmlSchemaFixupTypeAttributeUses
    fn fixup_type_attribute_uses(&mut self, t: ItemId) -> i32 {
        let base = self.s.it(t).base_type.unwrap();
        if self.type_not_fixed(base) && self.type_fixup(base) == -1 {
            return -1;
        }
        let is_restr = self.s.it(t).flags & TYPE_DERIVATION_METHOD_RESTRICTION != 0;
        let is_ext = self.s.it(t).flags & TYPE_DERIVATION_METHOD_EXTENSION != 0;
        let uses = self.s.it(t).attr_uses.clone();
        let mut prohibs: Vec<ItemId> = Vec::new();
        let mut new_uses = uses.clone();
        if !uses.is_empty() {
            let mut w = self.s.it(t).attribute_wildcard;
            match self.expand_attribute_group_refs(t, &mut w, uses, is_restr) {
                Ok((u, p)) => {
                    new_uses = u;
                    prohibs = p;
                }
                Err(_) => {
                    self.internal_err(
                        "xmlSchemaFixupTypeAttributeUses",
                        "failed to expand attributes",
                    );
                    return -1;
                }
            }
            self.s.itm(t).attribute_wildcard = w;
        }
        let base_uses = self.s.it(base).attr_uses.clone();
        if !base_uses.is_empty() {
            if is_restr {
                let uses_count = new_uses.len();
                for &u in &base_uses {
                    let (un, uns) = self.attr_use_decl_qname(u);
                    if prohibs
                        .iter()
                        .any(|&p| self.s.it(p).name == un && self.s.it(p).target_ns == uns)
                    {
                        continue;
                    }
                    if new_uses[..uses_count]
                        .iter()
                        .any(|&x| self.attr_use_decl_qname(x) == (un.clone(), uns.clone()))
                    {
                        continue;
                    }
                    new_uses.push(u);
                }
            } else {
                new_uses.extend(base_uses);
            }
        }
        self.s.itm(t).attr_uses = new_uses;
        if is_ext {
            if let Some(bw) = self.s.it(base).attribute_wildcard {
                match self.s.it(t).attribute_wildcard {
                    Some(tw) => {
                        if self.union_wildcards(tw, bw) == -1 {
                            return -1;
                        }
                    }
                    None => self.s.itm(t).attribute_wildcard = Some(bw),
                }
            }
        }
        0
    }

    /// xmlSchemaFixupComplexType
    pub fn fixup_complex_type(&mut self, t: ItemId) -> i32 {
        let olderrs = self.nberrors;
        if !self.type_not_fixed(t) {
            return 0;
        }
        self.s.itm(t).flags |= TYPE_INTERNAL_RESOLVED;
        let base = match self.s.it(t).base_type {
            None => {
                self.internal_err("xmlSchemaFixupComplexType", "missing baseType");
                self.s.itm(t).flags |= TYPE_INTERNAL_INVALID;
                return -1;
            }
            Some(b) => b,
        };
        if self.type_not_fixed(base) {
            self.type_fixup(base);
        }
        if self.s.it(base).flags & TYPE_INTERNAL_INVALID != 0 {
            return 0;
        }
        macro_rules! herr {
            ($r:expr) => {
                let r = $r;
                if r == -1 {
                    self.s.itm(t).flags |= TYPE_INTERNAL_INVALID;
                    return -1;
                }
                if r != 0 {
                    self.s.itm(t).flags |= TYPE_INTERNAL_INVALID;
                    return self.err;
                }
            };
        }
        herr!(self.check_src_ct(t));
        let is_restr = self.s.it(t).flags & TYPE_DERIVATION_METHOD_RESTRICTION != 0;
        let is_ext = self.s.it(t).flags & TYPE_DERIVATION_METHOD_EXTENSION != 0;
        if self.s.it(t).content_type == CONTENT_SIMPLE {
            if self.s.is_complex(base) && self.s.it(base).content_type_def.is_some() && is_restr {
                let content_base = match self.s.it(t).content_type_def {
                    Some(c) => {
                        self.s.itm(t).content_type_def = None;
                        c
                    }
                    None => self.s.it(base).content_type_def.unwrap(),
                };
                let tns = self.s.it(t).target_ns.clone();
                let node = self.s.it(t).node.unwrap_or(0);
                let content = self.add_type(TYPE_SIMPLE, None, tns, node, false);
                self.s.itm(content).base_type = Some(content_base);
                let facets = std::mem::take(&mut self.s.itm(t).facets);
                self.s.itm(t).facet_set.clear();
                let n = facets.len();
                self.s.itm(content).facets = facets;
                self.s.itm(content).facet_set = (0..n).map(|i| (content, i)).collect();
                self.s.itm(t).content_type_def = Some(content);
                if self.type_not_fixed(content_base) {
                    self.type_fixup(content_base);
                }
                herr!(self.fixup_simple_type_stage_one(content));
                herr!(self.fixup_simple_type_stage_two(content));
            } else if self.s.is_complex(base)
                && self.s.it(base).content_type == CONTENT_MIXED
                && is_restr
            {
                let ok = self
                    .s
                    .it(t)
                    .content_type_def
                    .map(|c| self.s.it(c).base_type.is_some())
                    .unwrap_or(false);
                if !ok {
                    let m = format!(
                        "Internal error: xmlSchemaTypeFixup, complex type '{}': the <simpleContent><restriction> is missing a <simpleType> child, but was not caught by xmlSchemaCheckSRCCT()",
                        self.s.it(t).name.clone().unwrap_or_default()
                    );
                    self.pcustom_err(E::SCHEMAP_INTERNAL, Some(t), None, &m);
                    self.s.itm(t).flags |= TYPE_INTERNAL_INVALID;
                    return -1;
                }
            } else if self.s.is_complex(base) && is_ext {
                match self.s.it(base).content_type_def {
                    None => {
                        let m = format!(
                            "Internal error: xmlSchemaTypeFixup, complex type '{}': the <extension>ed base type is a complex type with no simple content type",
                            self.s.it(t).name.clone().unwrap_or_default()
                        );
                        self.pcustom_err(E::SCHEMAP_INTERNAL, Some(t), None, &m);
                        self.s.itm(t).flags |= TYPE_INTERNAL_INVALID;
                        return -1;
                    }
                    Some(c) => self.s.itm(t).content_type_def = Some(c),
                }
            } else if self.s.is_simple(base) && is_ext {
                self.s.itm(t).content_type_def = Some(base);
            } else {
                let m = format!(
                    "Internal error: xmlSchemaTypeFixup, complex type '{}' with <simpleContent>: unhandled derivation case",
                    self.s.it(t).name.clone().unwrap_or_default()
                );
                self.pcustom_err(E::SCHEMAP_INTERNAL, Some(t), None, &m);
                self.s.itm(t).flags |= TYPE_INTERNAL_INVALID;
                return -1;
            }
        } else {
            let mut dummy_sequence = false;
            let mut particle = self.s.it(t).subtypes;
            let empty_effective = match particle {
                None => true,
                Some(p) => {
                    let pi = self.s.it(p);
                    match pi.children {
                        Some(term) => {
                            let tt = self.s.it(term);
                            pi.typ == TYPE_PARTICLE
                                && (tt.typ == TYPE_ALL
                                    || tt.typ == TYPE_SEQUENCE
                                    || (tt.typ == TYPE_CHOICE && pi.min_occurs == 0))
                                && tt.children.is_none()
                        }
                        None => false,
                    }
                }
            };
            if empty_effective {
                if self.s.it(t).flags & TYPE_MIXED != 0 {
                    let need_new = match particle {
                        None => true,
                        Some(p) => self
                            .s
                            .it(p)
                            .children
                            .map(|c| self.s.it(c).typ != TYPE_SEQUENCE)
                            .unwrap_or(true),
                    };
                    if need_new {
                        let node = self.s.it(t).node;
                        let p = self.add_particle(node, 1, 1);
                        let mg = self.add_model_group(TYPE_SEQUENCE, node.unwrap_or(0));
                        self.s.itm(p).children = Some(mg);
                        self.s.itm(t).subtypes = Some(p);
                        particle = Some(p);
                    }
                    dummy_sequence = true;
                    self.s.itm(t).content_type = CONTENT_ELEMENTS;
                } else {
                    self.s.itm(t).content_type = CONTENT_EMPTY;
                }
            } else {
                self.s.itm(t).content_type = CONTENT_ELEMENTS;
            }
            let mixed = self.s.it(t).flags & TYPE_MIXED != 0;
            if is_restr {
                if self.s.it(t).content_type != CONTENT_EMPTY && mixed {
                    self.s.itm(t).content_type = CONTENT_MIXED;
                }
            } else if self.s.it(t).content_type == CONTENT_EMPTY {
                let (ct, st, ctd) = (
                    self.s.it(base).content_type,
                    self.s.it(base).subtypes,
                    self.s.it(base).content_type_def,
                );
                let ti = self.s.itm(t);
                ti.content_type = ct;
                ti.subtypes = st;
                ti.content_type_def = ctd;
            } else if self.s.it(base).content_type == CONTENT_EMPTY {
                if mixed {
                    self.s.itm(t).content_type = CONTENT_MIXED;
                }
            } else {
                if mixed {
                    self.s.itm(t).content_type = CONTENT_MIXED;
                }
                let term_is_all = |s: &Schema, ty: ItemId| -> bool {
                    s.it(ty)
                        .subtypes
                        .and_then(|p| s.it(p).children)
                        .map(|c| s.it(c).typ == TYPE_ALL)
                        .unwrap_or(false)
                };
                if term_is_all(&self.s, t) {
                    let node = self.s.component_node(t);
                    self.custom_err(E::SCHEMAP_COS_ALL_LIMITED, node, None, "The type has an 'all' model group in its {content type} and thus cannot be derived from a non-empty type, since this would produce a 'sequence' model group containing the 'all' model group; 'all' model groups are not allowed to appear inside other model groups");
                } else if term_is_all(&self.s, base) {
                    let node = self.s.component_node(t);
                    self.custom_err(E::SCHEMAP_COS_ALL_LIMITED, node, None, "A type cannot be derived by extension from a type which has an 'all' model group in its {content type}, since this would produce a 'sequence' model group containing the 'all' model group; 'all' model groups are not allowed to appear inside other model groups");
                } else if !dummy_sequence && self.s.it(base).subtypes.is_some() {
                    let effective = self.s.it(t).subtypes;
                    let node = self.s.it(t).node;
                    let np = self.add_particle(node, 1, 1);
                    let mg = self.add_model_group(TYPE_SEQUENCE, node.unwrap_or(0));
                    self.s.itm(np).children = Some(mg);
                    self.s.itm(t).subtypes = Some(np);
                    let bp = self.s.it(base).subtypes.unwrap();
                    let (bmin, bmax, bterm) = (
                        self.s.it(bp).min_occurs,
                        self.s.it(bp).max_occurs,
                        self.s.it(bp).children,
                    );
                    let cp = self.add_particle(node, bmin, bmax);
                    self.s.itm(mg).children = Some(cp);
                    self.s.itm(cp).children = bterm;
                    self.s.itm(cp).next = effective;
                } else if let Some(p) = particle {
                    // an empty <sequence> already: add the base's content
                    if let Some(mg) = self.s.it(p).children {
                        let bs = self.s.it(base).subtypes;
                        self.s.itm(mg).children = bs;
                    }
                }
            }
        }
        herr!(self.fixup_type_attribute_uses(t));
        herr!(self.check_ct_component(t));
        if olderrs != self.nberrors {
            return self.err;
        }
        0
    }

    /// xmlSchemaCheckCTComponent (ct-props-correct; derivation checks are
    /// not implemented)
    fn check_ct_component(&mut self, t: ItemId) -> i32 {
        let base = self.s.it(t).base_type;
        if let Some(b) = base {
            if self.s.is_simple(b) && self.s.it(t).flags & TYPE_DERIVATION_METHOD_EXTENSION == 0 {
                self.custom_err(
                    E::SCHEMAP_SRC_CT_1,
                    None,
                    Some(t),
                    "If the base type is a simple type, the derivation method must be 'extension'",
                );
                return E::SCHEMAP_SRC_CT_1;
            }
        }
        if self.s.it(t).attr_uses.len() > 1 {
            self.check_duplicate_uses(t, false);
        }
        0
    }

    // -----------------------------------------------------------------
    // Element declarations

    /// xmlSchemaCheckElemPropsCorrect
    fn check_elem_props_correct(&mut self, e: ItemId) -> i32 {
        let mut ret = 0;
        let type_def = self.s.it(e).subtypes;
        if let Some(head) = self.s.it(e).ref_decl {
            self.check_element_decl_component(head);
            if self.s.it(e).flags & ELEM_GLOBAL == 0 {
                self.pcustom_err(
                    E::SCHEMAP_E_PROPS_CORRECT_3,
                    Some(e),
                    None,
                    "Only global element declarations can have a substitution group affiliation",
                );
                ret = E::SCHEMAP_E_PROPS_CORRECT_3;
            }
            let circ = if head == e {
                Some(head)
            } else if self.s.it(head).ref_decl.is_some() {
                // xmlSchemaCheckSubstGroupCircular(head, head)
                let mut cur = self.s.it(head).ref_decl;
                let mut seen = vec![head];
                let mut found = None;
                let mut prev = head;
                while let Some(c) = cur {
                    if c == head {
                        found = Some(prev);
                        break;
                    }
                    if seen.contains(&c) {
                        break;
                    }
                    seen.push(c);
                    prev = c;
                    cur = self.s.it(c).ref_decl;
                }
                found
            } else {
                None
            };
            if let Some(c) = circ {
                let m = format!(
                    "The element declaration '{}' defines a circular substitution group to element declaration '{}'",
                    self.s.component_qname(c),
                    self.s.component_qname(head)
                );
                self.pcustom_err(E::SCHEMAP_E_PROPS_CORRECT_6, Some(c), None, &m);
                ret = E::SCHEMAP_E_PROPS_CORRECT_6;
            }
            let head_type = self.s.it(head).subtypes;
            if let (Some(td), Some(ht)) = (type_def, head_type) {
                if td != ht {
                    let mut set = 0;
                    if self.s.it(head).flags & ELEM_FINAL_EXTENSION != 0 {
                        set |= 2;
                    }
                    if self.s.it(head).flags & ELEM_FINAL_RESTRICTION != 0 {
                        set |= 1;
                    }
                    if !self.s.cos_derived_ok(td, ht, set) {
                        let m = format!(
                            "The type definition '{}' was either rejected by the substitution group affiliation '{}', or not validly derived from its type definition '{}'",
                            self.s.component_qname(td),
                            self.s.component_qname(head),
                            self.s.component_qname(ht)
                        );
                        self.pcustom_err(E::SCHEMAP_E_PROPS_CORRECT_4, Some(e), None, &m);
                        ret = E::SCHEMAP_E_PROPS_CORRECT_4;
                    }
                }
            }
        }
        let value = self.s.it(e).value.clone();
        if let (Some(v), Some(td)) = (&value, type_def) {
            let id_type = (self.s.is_simple(td)
                && is_derived_from_builtin(&self.s, Some(td), T::ID))
                || (self.s.is_complex(td)
                    && self.s.has_simple_content(td)
                    && is_derived_from_builtin(&self.s, self.s.it(td).content_type_def, T::ID));
            if id_type {
                ret = E::SCHEMAP_E_PROPS_CORRECT_5;
                self.pcustom_err(E::SCHEMAP_E_PROPS_CORRECT_5, Some(e), None, "The type definition (or type definition's content type) is or is derived from ID; value constraints are not allowed in conjunction with such a type definition");
            } else {
                let node = self.s.it(e).node.and_then(|n| {
                    if self.s.it(e).flags & ELEM_FIXED != 0 {
                        get_prop_node(n, "fixed")
                    } else {
                        get_prop_node(n, "default")
                    }
                });
                // xmlSchemaParseCheckCOSValidDefault
                if self.s.is_complex(td)
                    && !self.s.has_simple_content(td)
                    && (self.s.it(td).content_type != CONTENT_MIXED
                        || !self.particle_emptiable(self.s.it(td).subtypes))
                {
                    let tn = self.s.it(td).node;
                    self.pcustom_err(E::SCHEMAP_COS_VALID_DEFAULT_2_1, Some(td), tn, "For a string to be a valid default, the type definition must be a simple type or a complex type with mixed content and a particle emptiable");
                    return E::SCHEMAP_COS_VALID_DEFAULT_2_1;
                }
                let st = if self.s.is_simple(td) {
                    Some(td)
                } else if self.s.has_simple_content(td) {
                    self.s.it(td).content_type_def
                } else {
                    None
                };
                if let Some(st) = st {
                    let (r, val) = self.pcheck_simple(node, st, v, true);
                    if r != 0 {
                        if r < 0 {
                            self.internal_err(
                                "xmlSchemaParseCheckCOSValidDefault",
                                "calling xmlSchemaVCheckCVCSimpleType()",
                            );
                            return -1;
                        }
                        return r;
                    }
                    self.s.itm(e).def_val = val.and_then(|v| v.into_iter().next());
                }
            }
        }
        ret
    }

    /// xmlSchemaCheckElemSubstGroup
    fn check_elem_subst_group(&mut self, e: ItemId) {
        if self.s.it(e).ref_decl.is_none() || self.s.it(e).flags & ELEM_ABSTRACT != 0 {
            return;
        }
        let mut head = self.s.it(e).ref_decl;
        let mut guard = 0;
        while let Some(h) = head {
            guard += 1;
            if guard > 1000 {
                break;
            }
            let next = self.s.it(h).ref_decl;
            if self.s.it(h).flags & ELEM_BLOCK_SUBSTITUTION != 0 {
                head = next;
                continue;
            }
            let head_type = self.s.it(h).subtypes;
            let typ = self.s.it(e).subtypes;
            let mut add = head_type == typ;
            if !add {
                let mut set = 0;
                let mut meth = 0;
                if self.s.it(h).flags & ELEM_BLOCK_RESTRICTION != 0 {
                    set |= TYPE_BLOCK_RESTRICTION;
                }
                if self.s.it(h).flags & ELEM_BLOCK_EXTENSION != 0 {
                    set |= TYPE_BLOCK_EXTENSION;
                }
                let mut ty = typ;
                while let Some(t) = ty {
                    if Some(t) == head_type || self.s.it(t).base_type == Some(t) {
                        break;
                    }
                    if self.s.it(t).flags & TYPE_DERIVATION_METHOD_EXTENSION != 0
                        && meth & TYPE_BLOCK_RESTRICTION == 0
                    {
                        meth |= TYPE_BLOCK_EXTENSION;
                    }
                    if self.s.it(t).flags & TYPE_DERIVATION_METHOD_RESTRICTION != 0
                        && meth & TYPE_BLOCK_RESTRICTION == 0
                    {
                        meth |= TYPE_BLOCK_RESTRICTION;
                    }
                    ty = self.s.it(t).base_type;
                }
                let mut ty = typ.and_then(|t| self.s.it(t).base_type);
                while let Some(t) = ty {
                    if self.s.is_complex(t) {
                        if self.s.it(t).flags & TYPE_BLOCK_EXTENSION != 0 {
                            set |= TYPE_BLOCK_EXTENSION;
                        }
                        if self.s.it(t).flags & TYPE_BLOCK_RESTRICTION != 0 {
                            set |= TYPE_BLOCK_RESTRICTION;
                        }
                    } else {
                        break;
                    }
                    if Some(t) == head_type || self.s.it(t).base_type == Some(t) {
                        break;
                    }
                    ty = self.s.it(t).base_type;
                }
                add = !(set != 0
                    && ((set & TYPE_BLOCK_EXTENSION != 0 && meth & TYPE_BLOCK_EXTENSION != 0)
                        || (set & TYPE_BLOCK_RESTRICTION != 0
                            && meth & TYPE_BLOCK_RESTRICTION != 0)));
            }
            if add {
                self.add_subst_member(h, e);
                self.s.itm(h).flags |= ELEM_SUBST_GROUP_HEAD;
            }
            head = next;
        }
    }

    /// xmlSchemaCheckElementDeclComponent
    fn check_element_decl_component(&mut self, e: ItemId) {
        if self.s.it(e).flags & ELEM_INTERNAL_CHECKED != 0 {
            return;
        }
        self.s.itm(e).flags |= ELEM_INTERNAL_CHECKED;
        if self.check_elem_props_correct(e) == 0 {
            self.check_elem_subst_group(e);
        }
    }

    // -----------------------------------------------------------------
    // Content models

    fn subst_members(&self, e: ItemId) -> Vec<ItemId> {
        let key = (
            self.s.it(e).name.clone().unwrap_or_default(),
            self.s.it(e).target_ns.clone(),
        );
        self.s.subst_groups.get(&key).cloned().unwrap_or_default()
    }

    /// xmlSchemaBuildContentModelForSubstGroup
    fn build_cm_subst_group(
        &mut self,
        am: &mut Automata,
        state: &mut StateId,
        particle: ItemId,
        counter: i32,
        end: Option<StateId>,
    ) -> bool {
        let e = self.s.it(particle).children.unwrap();
        let start = *state;
        let end = end.unwrap_or_else(|| am.new_state());
        let members = self.subst_members(e);
        let (name, ns) = (
            self.s.it(e).name.clone().unwrap_or_default(),
            self.s.it(e).target_ns.clone(),
        );
        let (min, max) = (
            self.s.it(particle).min_occurs,
            self.s.it(particle).max_occurs,
        );
        let mut ret = false;
        if counter >= 0 {
            let tmp = am.new_counted_trans(start, None, counter);
            am.new_transition2(tmp, Some(end), &name, ns.as_deref(), Some(e));
            for m in members {
                let (mn, mns) = (
                    self.s.it(m).name.clone().unwrap_or_default(),
                    self.s.it(m).target_ns.clone(),
                );
                am.new_transition2(tmp, Some(end), &mn, mns.as_deref(), Some(m));
            }
        } else if max == 1 {
            let s1 = am
                .new_transition2(start, None, &name, ns.as_deref(), Some(e))
                .unwrap();
            am.new_epsilon(s1, Some(end));
            for m in members {
                let (mn, mns) = (
                    self.s.it(m).name.clone().unwrap_or_default(),
                    self.s.it(m).target_ns.clone(),
                );
                let tmp = am
                    .new_transition2(start, None, &mn, mns.as_deref(), Some(m))
                    .unwrap();
                am.new_epsilon(tmp, Some(end));
            }
        } else {
            let maxo = if max == UNBOUNDED { UNBOUNDED } else { max - 1 };
            let mino = if min < 1 { 0 } else { min - 1 };
            let counter = am.new_counter(mino, maxo);
            let hop = am.new_state();
            let s1 = am
                .new_transition2(start, None, &name, ns.as_deref(), Some(e))
                .unwrap();
            am.new_epsilon(s1, Some(hop));
            for m in members {
                let (mn, mns) = (
                    self.s.it(m).name.clone().unwrap_or_default(),
                    self.s.it(m).target_ns.clone(),
                );
                let s2 = am
                    .new_transition2(start, None, &mn, mns.as_deref(), Some(m))
                    .unwrap();
                am.new_epsilon(s2, Some(hop));
            }
            am.new_counted_trans(hop, Some(start), counter);
            am.new_counter_trans(hop, Some(end), counter);
        }
        if min == 0 {
            am.new_epsilon(start, Some(end));
            ret = true;
        }
        *state = end;
        ret
    }

    /// xmlSchemaBuildContentModelForElement
    fn build_cm_element(
        &mut self,
        am: &mut Automata,
        state: &mut StateId,
        particle: ItemId,
    ) -> bool {
        let e = self.s.it(particle).children.unwrap();
        if self.s.it(e).flags & ELEM_SUBST_GROUP_HEAD != 0 {
            return self.build_cm_subst_group(am, state, particle, -1, None);
        }
        if self.s.it(e).flags & ELEM_ABSTRACT != 0 {
            return false;
        }
        let (name, ns) = (
            self.s.it(e).name.clone().unwrap_or_default(),
            self.s.it(e).target_ns.clone(),
        );
        let (min, max) = (
            self.s.it(particle).min_occurs,
            self.s.it(particle).max_occurs,
        );
        let start;
        if max == 1 {
            start = *state;
            *state = am
                .new_transition2(start, None, &name, ns.as_deref(), Some(e))
                .unwrap();
        } else if max >= UNBOUNDED && min < 2 {
            start = *state;
            *state = am
                .new_transition2(start, None, &name, ns.as_deref(), Some(e))
                .unwrap();
            let s = *state;
            *state = am
                .new_transition2(s, Some(s), &name, ns.as_deref(), Some(e))
                .unwrap();
        } else {
            let maxo = if max == UNBOUNDED { UNBOUNDED } else { max - 1 };
            let mino = if min < 1 { 0 } else { min - 1 };
            start = am.new_epsilon(*state, None);
            let counter = am.new_counter(mino, maxo);
            *state = am
                .new_transition2(start, None, &name, ns.as_deref(), Some(e))
                .unwrap();
            am.new_counted_trans(*state, Some(start), counter);
            *state = am.new_counter_trans(*state, None, counter);
        }
        if min == 0 {
            am.new_epsilon(start, Some(*state));
            return true;
        }
        false
    }

    fn sub_particles(&self, term: ItemId) -> Vec<ItemId> {
        let mut v = Vec::new();
        let mut p = self.s.it(term).children;
        while let Some(x) = p {
            v.push(x);
            p = self.s.it(x).next;
        }
        v
    }

    /// xmlSchemaBuildAContentModel: returns true if emptiable ("nillable")
    fn build_a_content_model(
        &mut self,
        am: &mut Automata,
        state: &mut StateId,
        particle: Option<ItemId>,
    ) -> bool {
        let particle = match particle {
            None => {
                self.internal_err("xmlSchemaBuildAContentModel", "particle is NULL");
                return true;
            }
            Some(p) => p,
        };
        let term = match self.s.it(particle).children {
            None => return true,
            Some(t) => t,
        };
        let (min, max) = (
            self.s.it(particle).min_occurs,
            self.s.it(particle).max_occurs,
        );
        let mut ret;
        match self.s.it(term).typ {
            TYPE_ANY => {
                let w = self.s.it(term).clone();
                let start = *state;
                let end = am.new_state();
                let target = if max == 1 { end } else { am.new_state() };
                let mut counter = -1;
                if max != 1 {
                    let maxo = if max == UNBOUNDED { UNBOUNDED } else { max - 1 };
                    let mino = if min < 1 { 0 } else { min - 1 };
                    counter = am.new_counter(mino, maxo);
                }
                if w.any {
                    let s1 = am
                        .new_transition2(start, None, "*", Some("*"), Some(term))
                        .unwrap();
                    am.new_epsilon(s1, Some(target));
                    let s2 = am
                        .new_transition2(start, None, "*", None, Some(term))
                        .unwrap();
                    am.new_epsilon(s2, Some(target));
                } else if !w.ns_set.is_empty() {
                    for ns in &w.ns_set {
                        let s1 = am
                            .new_transition2(start, None, "*", ns.as_deref(), Some(term))
                            .unwrap();
                        am.new_epsilon(s1, Some(target));
                    }
                } else if let Some(neg) = &w.neg_ns_set {
                    am.new_neg_trans(start, Some(target), "*", neg.as_deref(), Some(term));
                }
                if max != 1 {
                    am.new_counted_trans(target, Some(start), counter);
                    am.new_counter_trans(target, Some(end), counter);
                }
                ret = false;
                if min == 0 {
                    am.new_epsilon(start, Some(end));
                    ret = true;
                }
                *state = end;
            }
            TYPE_ELEMENT => ret = self.build_cm_element(am, state, particle),
            TYPE_SEQUENCE => {
                ret = true;
                let subs = self.sub_particles(term);
                if min == 1 && max == 1 {
                    for s in subs {
                        if !self.build_a_content_model(am, state, Some(s)) {
                            ret = false;
                        }
                    }
                } else {
                    let mut oldstate = *state;
                    if max >= UNBOUNDED {
                        if min > 1 {
                            *state = am.new_epsilon(oldstate, None);
                            oldstate = *state;
                            let counter = am.new_counter(min - 1, UNBOUNDED);
                            for s in subs {
                                if !self.build_a_content_model(am, state, Some(s)) {
                                    ret = false;
                                }
                            }
                            let tmp = *state;
                            am.new_counted_trans(tmp, Some(oldstate), counter);
                            *state = am.new_counter_trans(tmp, None, counter);
                            if ret {
                                am.new_epsilon(oldstate, Some(*state));
                            }
                        } else {
                            *state = am.new_epsilon(oldstate, None);
                            oldstate = *state;
                            for s in subs {
                                if !self.build_a_content_model(am, state, Some(s)) {
                                    ret = false;
                                }
                            }
                            am.new_epsilon(*state, Some(oldstate));
                            *state = am.new_epsilon(*state, None);
                            if min == 0 {
                                am.new_epsilon(oldstate, Some(*state));
                                ret = true;
                            }
                        }
                    } else if max > 1 || min > 1 {
                        *state = am.new_epsilon(oldstate, None);
                        oldstate = *state;
                        let counter = am.new_counter(min - 1, max - 1);
                        for s in subs {
                            if !self.build_a_content_model(am, state, Some(s)) {
                                ret = false;
                            }
                        }
                        let tmp = *state;
                        am.new_counted_trans(tmp, Some(oldstate), counter);
                        *state = am.new_counter_trans(tmp, None, counter);
                        if min == 0 || ret {
                            am.new_epsilon(oldstate, Some(*state));
                            ret = true;
                        }
                    } else {
                        for s in subs {
                            if !self.build_a_content_model(am, state, Some(s)) {
                                ret = false;
                            }
                        }
                        *state = am.new_epsilon(*state, None);
                        if min == 0 {
                            am.new_epsilon(oldstate, Some(*state));
                            ret = true;
                        }
                    }
                }
            }
            TYPE_CHOICE => {
                ret = false;
                let start = *state;
                let end = am.new_state();
                let subs = self.sub_particles(term);
                if max == 1 {
                    for s in subs {
                        *state = start;
                        if self.build_a_content_model(am, state, Some(s)) {
                            ret = true;
                        }
                        am.new_epsilon(*state, Some(end));
                    }
                } else {
                    let maxo = if max == UNBOUNDED { UNBOUNDED } else { max - 1 };
                    let mino = if min < 1 { 0 } else { min - 1 };
                    let counter = am.new_counter(mino, maxo);
                    let hop = am.new_state();
                    let base = am.new_state();
                    for s in subs {
                        *state = base;
                        if self.build_a_content_model(am, state, Some(s)) {
                            ret = true;
                        }
                        am.new_epsilon(*state, Some(hop));
                    }
                    am.new_epsilon(start, Some(base));
                    am.new_counted_trans(hop, Some(base), counter);
                    am.new_counter_trans(hop, Some(end), counter);
                    if ret {
                        am.new_epsilon(base, Some(end));
                    }
                }
                if min == 0 {
                    am.new_epsilon(start, Some(end));
                    ret = true;
                }
                *state = end;
            }
            TYPE_ALL => {
                let subs = self.sub_particles(term);
                if subs.is_empty() {
                    return true;
                }
                ret = false;
                let start = *state;
                let tmp = am.new_state();
                am.new_epsilon(*state, Some(tmp));
                for sub in subs {
                    *state = tmp;
                    let e = match self.s.it(sub).children {
                        Some(e) => e,
                        None => {
                            self.internal_err(
                                "xmlSchemaBuildAContentModel",
                                "<element> particle has no term",
                            );
                            return ret;
                        }
                    };
                    let (smin, smax) = (self.s.it(sub).min_occurs, self.s.it(sub).max_occurs);
                    if self.s.it(e).flags & ELEM_SUBST_GROUP_HEAD != 0 {
                        let counter = am.new_counter(smin, smax);
                        let st = *state;
                        self.build_cm_subst_group(am, state, sub, counter, Some(st));
                    } else {
                        let (name, ns) = (
                            self.s.it(e).name.clone().unwrap_or_default(),
                            self.s.it(e).target_ns.clone(),
                        );
                        if smin == 1 && smax == 1 {
                            am.new_once_trans2(
                                *state,
                                Some(*state),
                                &name,
                                ns.as_deref(),
                                1,
                                1,
                                Some(e),
                            );
                        } else if smin == 0 && smax == 1 {
                            am.new_count_trans2(
                                *state,
                                Some(*state),
                                &name,
                                ns.as_deref(),
                                0,
                                1,
                                Some(e),
                            );
                        }
                    }
                }
                *state = am.new_all_trans(*state, None, false);
                if min == 0 {
                    am.new_epsilon(start, Some(*state));
                    ret = true;
                }
            }
            TYPE_GROUP => ret = true,
            _ => {
                let m = format!(
                    "found unexpected term of type '{}' in content model",
                    self.s.component_type_str(term)
                );
                self.internal_err("xmlSchemaBuildAContentModel", &m);
                return false;
            }
        }
        ret
    }

    /// xmlSchemaBuildContentModel
    fn build_content_model(&mut self, t: ItemId) {
        let it = self.s.it(t);
        if it.typ != TYPE_COMPLEX
            || it.cont_model.is_some()
            || (it.content_type != CONTENT_ELEMENTS && it.content_type != CONTENT_MIXED)
        {
            return;
        }
        let mut am = Automata::new();
        let mut state = am.init_state();
        let p = self.s.it(t).subtypes;
        self.build_a_content_model(&mut am, &mut state, p);
        am.set_final_state(state);
        match am.compile() {
            None => {
                let node = self.s.it(t).node;
                self.pcustom_err(
                    E::SCHEMAP_INTERNAL,
                    Some(t),
                    node,
                    "Failed to compile the content model",
                );
            }
            Some(re) => {
                if !re.is_determinist() {
                    let node = self.s.it(t).node;
                    self.pcustom_err(
                        E::SCHEMAP_NOT_DETERMINISTIC,
                        Some(t),
                        node,
                        "The content model is not determinist",
                    );
                }
                self.s.itm(t).cont_model = Some(Rc::new(re));
            }
        }
    }
}

/// xmlSchemaIsParticleEmptiable
pub fn is_particle_emptiable(s: &Schema, p: Option<ItemId>) -> bool {
    let p = match p {
        None => return true,
        Some(p) => p,
    };
    let pi = s.it(p);
    if pi.min_occurs == 0 || pi.children.is_none() {
        return true;
    }
    let term = pi.children.unwrap();
    if matches!(s.it(term).typ, TYPE_SEQUENCE | TYPE_CHOICE | TYPE_ALL) {
        return get_particle_emptiable(s, p);
    }
    false
}

/// xmlSchemaGetParticleEmptiable
fn get_particle_emptiable(s: &Schema, p: ItemId) -> bool {
    let pi = s.it(p);
    let term = match pi.children {
        None => return true,
        Some(t) => t,
    };
    if pi.min_occurs == 0 {
        return true;
    }
    let is_choice = s.it(term).typ == TYPE_CHOICE;
    let mut part = s.it(term).children;
    if part.is_none() {
        return true;
    }
    while let Some(x) = part {
        let emptiable = match s.it(x).children {
            Some(c) if matches!(s.it(c).typ, TYPE_ELEMENT | TYPE_ANY) => s.it(x).min_occurs == 0,
            _ => get_particle_emptiable(s, x),
        };
        if is_choice {
            if emptiable {
                return true;
            }
        } else if !emptiable {
            return false;
        }
        part = s.it(x).next;
    }
    !is_choice
}

impl Schema {
    /// xmlSchemaCheckCOSCTDerivedOK (true when OK)
    pub fn cos_ct_derived_ok(&self, t: ItemId, base: ItemId, set: i32) -> bool {
        if t == base {
            return true;
        }
        let f = self.it(t).flags;
        if (set & 2 != 0 && f & TYPE_DERIVATION_METHOD_EXTENSION != 0)
            || (set & 1 != 0 && f & TYPE_DERIVATION_METHOD_RESTRICTION != 0)
        {
            return false;
        }
        let bt = match self.it(t).base_type {
            Some(b) => b,
            None => return false,
        };
        if bt == base {
            return true;
        }
        if self.is_anytype(bt) || bt == t {
            return false;
        }
        if self.is_complex(bt) {
            self.cos_ct_derived_ok(bt, base, set)
        } else {
            self.cos_st_derived_ok(bt, base, set)
        }
    }

    /// xmlSchemaCheckCOSSTDerivedOK (true when OK)
    pub fn cos_st_derived_ok(&self, t: ItemId, base: ItemId, subset: i32) -> bool {
        if t == base {
            return true;
        }
        let bt = match self.it(t).base_type {
            Some(b) => b,
            None => return false,
        };
        if subset & 1 != 0 || self.it(bt).flags & TYPE_FINAL_RESTRICTION != 0 {
            return false;
        }
        if bt == base {
            return true;
        }
        if !self.is_anytype(bt) && bt != t && self.cos_st_derived_ok(bt, base, subset) {
            return true;
        }
        if self.is_any_simple_type(base) && (self.is_list(t) || self.is_union(t)) {
            return true;
        }
        if self.is_union(base) {
            for m in &self.it(base).member_types {
                if self.cos_st_derived_ok(t, *m, subset) {
                    return true;
                }
            }
        }
        false
    }

    /// xmlSchemaCheckCOSDerivedOK (true when OK)
    pub fn cos_derived_ok(&self, t: ItemId, base: ItemId, set: i32) -> bool {
        if self.is_simple(t) {
            self.cos_st_derived_ok(t, base, set)
        } else {
            self.cos_ct_derived_ok(t, base, set)
        }
    }
}
