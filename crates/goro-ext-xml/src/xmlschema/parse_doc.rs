//! Schema documents: loading, <schema>, <include>, <import>, <redefine>
//! (xmlSchemaAddSchemaDoc, xmlSchemaParseNewDoc*, xmlSchemaParse).

use super::parse::{PCtxt, first_child, next_sibling};
use super::parse_comp::{pval_attr_block_final, pval_attr_form_default};
use super::*;
use crate::parser::{FROM_IO, XML_PARSE_NOENT};
use crate::xmlschemastypes as T;

impl<'a> PCtxt<'a> {
    /// xmlSchemaCleanupDoc: remove blank text nodes (unless xml:space is
    /// "preserve"), comments, PIs and other non element/text nodes.
    pub fn cleanup_doc(&mut self, root: NodeId) {
        tree::with(|t| {
            let mut stack = vec![root];
            while let Some(n) = stack.pop() {
                let kids = t.children_of(n);
                for k in kids {
                    match t.nodes[k].t {
                        NType::Text => {
                            if super::parse::blank_str(&t.nodes[k].content) && !space_preserve(t, k)
                            {
                                t.unlink(k);
                            }
                        }
                        NType::Element => {
                            let skip = matches!(
                                t.nodes[k].children.map(|c| t.nodes[c].t),
                                Some(NType::EntityDecl)
                                    | Some(NType::EntityRef)
                                    | Some(NType::Entity)
                            );
                            if !skip {
                                stack.push(k);
                            }
                        }
                        NType::CData => {}
                        _ => t.unlink(k),
                    }
                }
            }
        })
    }

    /// xmlSchemaParseSchemaElement
    pub fn parse_schema_element(&mut self, node: NodeId) -> i32 {
        let old_errs = self.nberrors;
        let mut res = self.pval_attr_id(node, "id");
        if let Some(a) = get_prop_node(node, "targetNamespace") {
            let (r, _) = self.pval_attr_node(a, T::ANYURI);
            res = r;
            if r == -1 {
                return -1;
            }
            if r != 0 {
                self.stop = E::SCHEMAP_S4S_ATTR_INVALID_VALUE;
                if old_errs != self.nberrors {
                    return self.err;
                }
                return res;
            }
        }
        if let Some(a) = get_prop_node(node, "elementFormDefault") {
            let v = node_content(a);
            let mut fl = self.s.flags;
            res = pval_attr_form_default(&v, &mut fl, QUALIF_ELEM);
            self.s.flags = fl;
            if res != 0 {
                self.psimple_type_err(
                    E::SCHEMAP_ELEMFORMDEFAULT_VALUE,
                    a,
                    None,
                    Some("(qualified | unqualified)"),
                    Some(&v),
                    None,
                );
            }
        }
        if let Some(a) = get_prop_node(node, "attributeFormDefault") {
            let v = node_content(a);
            let mut fl = self.s.flags;
            res = pval_attr_form_default(&v, &mut fl, QUALIF_ATTR);
            self.s.flags = fl;
            if res != 0 {
                self.psimple_type_err(
                    E::SCHEMAP_ATTRFORMDEFAULT_VALUE,
                    a,
                    None,
                    Some("(qualified | unqualified)"),
                    Some(&v),
                    None,
                );
            }
        }
        if let Some(a) = get_prop_node(node, "finalDefault") {
            let v = node_content(a);
            let mut fl = self.s.flags;
            res = pval_attr_block_final(
                &v,
                &mut fl,
                -1,
                FINAL_DEFAULT_EXTENSION,
                FINAL_DEFAULT_RESTRICTION,
                -1,
                FINAL_DEFAULT_LIST,
                FINAL_DEFAULT_UNION,
            );
            self.s.flags = fl;
            if res != 0 {
                self.psimple_type_err(
                    E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                    a,
                    None,
                    Some("(#all | List of (extension | restriction | list | union))"),
                    Some(&v),
                    None,
                );
            }
        }
        if let Some(a) = get_prop_node(node, "blockDefault") {
            let v = node_content(a);
            let mut fl = self.s.flags;
            res = pval_attr_block_final(
                &v,
                &mut fl,
                -1,
                BLOCK_DEFAULT_EXTENSION,
                BLOCK_DEFAULT_RESTRICTION,
                BLOCK_DEFAULT_SUBSTITUTION,
                -1,
                -1,
            );
            self.s.flags = fl;
            if res != 0 {
                self.psimple_type_err(
                    E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                    a,
                    None,
                    Some("(#all | List of (extension | restriction | substitution))"),
                    Some(&v),
                    None,
                );
            }
        }
        if old_errs != self.nberrors {
            res = self.err;
        }
        res
    }

    /// xmlSchemaParseSchemaTopLevel
    pub fn parse_schema_top_level(&mut self, nodes: Option<NodeId>) -> i32 {
        let old_errs = self.nberrors;
        let mut child = nodes;
        let mut res = 0;
        while is_schema(child, "include")
            || is_schema(child, "import")
            || is_schema(child, "redefine")
            || is_schema(child, "annotation")
        {
            let c = child.unwrap();
            if is_schema(child, "annotation") {
                self.parse_annotation(c, true);
            } else {
                let tmp_old = self.nberrors;
                res = if is_schema(child, "import") {
                    self.parse_import(c)
                } else if is_schema(child, "include") {
                    self.parse_include_or_redefine(c, SCHEMA_INCLUDE)
                } else {
                    self.parse_include_or_redefine(c, SCHEMA_REDEFINE)
                };
                if res == -1 {
                    return -1;
                }
                if self.stop != 0 || tmp_old != self.nberrors {
                    self.ctxt_type = None;
                    if old_errs != self.nberrors {
                        res = self.err;
                    }
                    return res;
                }
            }
            child = next_sibling(c);
        }
        while let Some(c) = child {
            if is_schema(child, "complexType") {
                self.parse_complex_type(c, true);
            } else if is_schema(child, "simpleType") {
                self.parse_simple_type(c, true);
            } else if is_schema(child, "element") {
                let mut r = false;
                self.parse_element(c, &mut r, true);
            } else if is_schema(child, "attribute") {
                self.parse_global_attribute(c);
            } else if is_schema(child, "attributeGroup") {
                self.parse_attribute_group_definition(c);
            } else if is_schema(child, "group") {
                self.parse_model_group_definition(c);
            } else if is_schema(child, "notation") {
                self.parse_notation(c);
            } else {
                let parent = node_parent(c);
                self.content_err(E::SCHEMAP_S4S_ELEM_NOT_ALLOWED, None, parent, Some(c), None, Some("((include | import | redefine | annotation)*, (((simpleType | complexType | group | attributeGroup) | element | attribute | notation), annotation*)*)"));
            }
            child = next_sibling(c);
            while is_schema(child, "annotation") {
                self.parse_annotation(child.unwrap(), true);
                child = next_sibling(child.unwrap());
            }
        }
        self.ctxt_type = None;
        if old_errs != self.nberrors {
            res = self.err;
        }
        res
    }

    /// xmlSchemaParseImport
    pub fn parse_import(&mut self, node: NodeId) -> i32 {
        self.check_attrs(node, &["id", "namespace", "schemaLocation"]);
        let (r, namespace_name) = self.pval_attr(node, "namespace", T::ANYURI);
        if r != 0 {
            let at = self.s.builtin(T::ANYURI);
            self.psimple_type_err(
                E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                node,
                Some(at),
                None,
                namespace_name.as_deref(),
                None,
            );
            return self.err;
        }
        let (r, schema_location) = self.pval_attr(node, "schemaLocation", T::ANYURI);
        if r != 0 {
            let at = self.s.builtin(T::ANYURI);
            self.psimple_type_err(
                E::SCHEMAP_S4S_ATTR_INVALID_VALUE,
                node,
                Some(at),
                None,
                schema_location.as_deref(),
                None,
            );
            return self.err;
        }
        let mut child = first_child(node);
        if is_schema(child, "annotation") {
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
        let this_tns = self.s.buckets[self.bucket].orig_target_ns.clone();
        if let Some(ns) = &namespace_name {
            if this_tns.as_deref() == Some(ns.as_str()) {
                let m = format!(
                    "The value of the attribute 'namespace' must not match the target namespace '{}' of the importing schema",
                    this_tns.as_deref().unwrap_or("(null)")
                );
                self.pcustom_err(E::SCHEMAP_SRC_IMPORT_1_1, None, Some(node), &m);
                return self.err;
            }
        } else if this_tns.is_none() {
            self.pcustom_err(E::SCHEMAP_SRC_IMPORT_1_2, None, Some(node), "The attribute 'namespace' must be existent if the importing schema has no target namespace");
            return self.err;
        }
        let schema_location = schema_location.map(|l| build_absolute_uri(&l, node));
        let (ret, bucket) = self.add_schema_doc(
            SCHEMA_IMPORT,
            schema_location.as_deref(),
            None,
            Some(node),
            this_tns.as_deref(),
            namespace_name.as_deref(),
        );
        if ret != 0 {
            return ret;
        }
        if bucket.is_none() {
            if let Some(l) = &schema_location {
                let m = format!(
                    "Failed to locate a schema at location '{}'. Skipping the import",
                    l
                );
                self.custom_warning(E::SCHEMAP_WARN_UNLOCATED_SCHEMA, Some(node), &m);
            }
        }
        if let Some(b) = bucket {
            if self.s.buckets[b].doc.is_some() && !self.s.buckets[b].parsed {
                return self.parse_new_doc(b);
            }
        }
        ret
    }

    /// xmlSchemaParseIncludeOrRedefineAttrs: Ok(location)
    fn parse_include_or_redefine_attrs(&mut self, node: NodeId, typ: i32) -> Result<String, i32> {
        self.check_attrs(node, &["id", "schemaLocation"]);
        self.pval_attr_id(node, "id");
        let attr = match get_prop_node(node, "schemaLocation") {
            None => {
                self.missing_attr_err(
                    E::SCHEMAP_S4S_ATTR_MISSING,
                    None,
                    Some(node),
                    "schemaLocation",
                    None,
                );
                return Err(self.err);
            }
            Some(a) => a,
        };
        let (r, loc) = self.pval_attr_node(attr, T::ANYURI);
        if r != 0 {
            return Err(self.err);
        }
        let loc = build_absolute_uri(&loc, node);
        if Some(loc.as_str()) == self.url.as_deref() {
            if typ == SCHEMA_REDEFINE {
                let m = format!("The schema document '{}' cannot redefine itself.", loc);
                self.pcustom_err(E::SCHEMAP_SRC_REDEFINE, None, Some(node), &m);
            } else {
                let m = format!("The schema document '{}' cannot include itself.", loc);
                self.pcustom_err(E::SCHEMAP_SRC_INCLUDE, None, Some(node), &m);
            }
            return Err(self.err);
        }
        Ok(loc)
    }

    /// xmlSchemaParseIncludeOrRedefine
    pub fn parse_include_or_redefine(&mut self, node: NodeId, typ: i32) -> i32 {
        let loc = match self.parse_include_or_redefine_attrs(node, typ) {
            Ok(l) => l,
            Err(e) => return e,
        };
        let tns = self.target_ns.clone();
        let (res, bucket) =
            self.add_schema_doc(typ, Some(&loc), None, Some(node), tns.as_deref(), None);
        if res != 0 {
            return res;
        }
        let mut res = 0;
        let mut is_chameleon = false;
        let ok_bucket = bucket.filter(|&b| self.s.buckets[b].doc.is_some());
        match ok_bucket {
            None => {
                if typ == SCHEMA_INCLUDE {
                    res = E::SCHEMAP_SRC_INCLUDE;
                    let m = format!("Failed to load the document '{}' for inclusion", loc);
                    self.custom_err(res, Some(node), None, &m);
                } else {
                    res = E::SCHEMAP_SRC_REDEFINE;
                    let m = format!("Failed to load the document '{}' for redefinition", loc);
                    self.custom_err(res, Some(node), None, &m);
                }
            }
            Some(b) => {
                let orig = self.s.buckets[b].orig_target_ns.clone();
                if let Some(o) = orig {
                    match &self.target_ns {
                        None => {
                            let m = format!(
                                "The target namespace of the included/redefined schema '{}' has to be absent, since the including/redefining schema has no target namespace",
                                loc
                            );
                            self.custom_err(E::SCHEMAP_SRC_INCLUDE, Some(node), None, &m);
                            return self.err;
                        }
                        Some(t) if *t != o => {
                            let m = format!(
                                "The target namespace '{}' of the included/redefined schema '{}' differs from '{}' of the including/redefining schema",
                                o, loc, t
                            );
                            self.pcustom_err(E::SCHEMAP_SRC_INCLUDE, None, Some(node), &m);
                            return self.err;
                        }
                        _ => {}
                    }
                } else if self.target_ns.is_some() {
                    is_chameleon = true;
                    self.s.buckets[b].target_ns = self.target_ns.clone();
                }
            }
        }
        if let Some(b) = ok_bucket {
            if !self.s.buckets[b].parsed {
                let mut was_chameleon = false;
                if is_chameleon {
                    if self.s.flags & INCLUDING_CONVERT_NS == 0 {
                        self.s.flags |= INCLUDING_CONVERT_NS;
                    } else {
                        was_chameleon = true;
                    }
                }
                self.parse_new_doc(b);
                if is_chameleon && !was_chameleon {
                    self.s.flags &= !INCLUDING_CONVERT_NS;
                }
            }
        }
        let mut child = first_child(node);
        if typ == SCHEMA_REDEFINE {
            self.redefined = bucket;
            self.is_redefine = true;
            while is_schema(child, "annotation")
                || is_schema(child, "simpleType")
                || is_schema(child, "complexType")
                || is_schema(child, "group")
                || is_schema(child, "attributeGroup")
            {
                let c = child.unwrap();
                if is_schema(child, "simpleType") {
                    self.parse_simple_type(c, true);
                } else if is_schema(child, "complexType") {
                    self.parse_complex_type(c, true);
                } else if is_schema(child, "group") {
                    self.parse_model_group_definition(c);
                } else if is_schema(child, "attributeGroup") {
                    self.parse_attribute_group_definition(c);
                }
                child = next_sibling(c);
            }
            self.redefined = None;
            self.is_redefine = false;
        } else if is_schema(child, "annotation") {
            child = next_sibling(child.unwrap());
        }
        if child.is_some() {
            res = E::SCHEMAP_S4S_ELEM_NOT_ALLOWED;
            let c = if typ == SCHEMA_REDEFINE {
                "(annotation | (simpleType | complexType | group | attributeGroup))*"
            } else {
                "(annotation?)"
            };
            self.content_err(res, None, Some(node), child, None, Some(c));
        }
        res
    }

    fn get_bucket_by_location(&self, loc: &str) -> Option<usize> {
        self.s
            .buckets
            .iter()
            .position(|b| b.schema_location.as_deref() == Some(loc))
    }

    /// xmlSchemaAddSchemaDoc: (result, bucket)
    pub fn add_schema_doc(
        &mut self,
        typ: i32,
        schema_location: Option<&str>,
        buffer: Option<&[u8]>,
        invoking: Option<NodeId>,
        source_tns: Option<&str>,
        import_ns: Option<&str>,
    ) -> (i32, Option<usize>) {
        let err = match typ {
            SCHEMA_IMPORT | SCHEMA_MAIN => E::SCHEMAP_SRC_IMPORT,
            SCHEMA_INCLUDE => E::SCHEMAP_SRC_INCLUDE,
            _ => E::SCHEMAP_SRC_REDEFINE,
        };
        let mut bkt: Option<usize> = None;
        let mut relation: Option<usize> = None;
        let mut schema_location = schema_location.map(|s| s.to_string());
        let is_impmain = typ == SCHEMA_MAIN || typ == SCHEMA_IMPORT;
        if typ != SCHEMA_MAIN && !self.s.buckets.is_empty() {
            if let Some(l) = &schema_location {
                bkt = self.get_bucket_by_location(l);
                if let Some(b) = bkt {
                    if b == self.bucket {
                        self.custom_err(
                            err,
                            invoking,
                            None,
                            "The schema must not import/include/redefine itself",
                        );
                        return (0, None);
                    }
                }
            }
            let cur = self.bucket;
            self.s.buckets[cur].relations.push(Relation {
                typ,
                import_ns: None,
                bucket: None,
            });
            let ridx = self.s.buckets[cur].relations.len() - 1;
            relation = Some(ridx);
            if is_impmain {
                self.s.buckets[cur].relations[ridx].import_ns = import_ns.map(|s| s.to_string());
                if schema_location.is_none() {
                    return (0, None);
                }
            }
            if let Some(b) = bkt {
                if is_impmain && self.s.buckets[b].imported == 0 {
                    let l = schema_location
                        .clone()
                        .unwrap_or_else(|| "in_memory_buffer".into());
                    if Some(l.as_str()) != self.s.buckets[b].schema_location.as_deref() {
                        let m = format!(
                            "The schema document '{}' cannot be imported, since it was already included or redefined",
                            l
                        );
                        self.custom_err(err, invoking, None, &m);
                        return (0, None);
                    }
                } else if !is_impmain && self.s.buckets[b].imported != 0 {
                    let l = schema_location
                        .clone()
                        .unwrap_or_else(|| "in_memory_buffer".into());
                    if Some(l.as_str()) != self.s.buckets[b].schema_location.as_deref() {
                        let m = format!(
                            "The schema document '{}' cannot be included or redefined, since it was already imported",
                            l
                        );
                        self.custom_err(err, invoking, None, &m);
                        return (0, None);
                    }
                }
            }
            if is_impmain {
                if let Some(b) = bkt {
                    self.s.buckets[cur].relations[ridx].bucket = Some(b);
                    return (0, Some(b));
                }
                let found = self.s.buckets.iter().position(|b| {
                    !(b.doc.is_none() && b.schema_location.is_some())
                        && b.orig_target_ns.as_deref() == import_ns
                        && b.imported != 0
                });
                if let Some(b) = found {
                    self.s.buckets[cur].relations[ridx].bucket = Some(b);
                    if self.s.buckets[b].schema_location.is_none() {
                        self.s.buckets[b].schema_location = schema_location.clone();
                    } else if schema_location.as_deref()
                        != self.s.buckets[b].schema_location.as_deref()
                    {
                        let l = schema_location
                            .clone()
                            .unwrap_or_else(|| "in_memory_buffer".into());
                        let m = format!(
                            "Skipping import of schema located at '{}' for the namespace '{}', since this namespace was already imported with the schema located at '{}'",
                            l,
                            import_ns.unwrap_or("(null)"),
                            self.s.buckets[b]
                                .schema_location
                                .clone()
                                .unwrap_or_default()
                        );
                        self.custom_warning(E::SCHEMAP_WARN_SKIP_SCHEMA, invoking, &m);
                    }
                    return (0, Some(b));
                }
            } else if let Some(b) = bkt {
                if self.s.buckets[b].orig_target_ns.is_none()
                    && self.s.buckets[b].target_ns.as_deref() != source_tns
                {
                    let chamel = self.s.buckets.iter().position(|x| {
                        x.orig_target_ns.is_none()
                            && x.schema_location == schema_location
                            && x.target_ns.as_deref() == source_tns
                    });
                    if let Some(c) = chamel {
                        self.s.buckets[cur].relations[ridx].bucket = Some(c);
                        return (0, Some(c));
                    }
                    bkt = None;
                } else {
                    self.s.buckets[cur].relations[ridx].bucket = Some(b);
                    return (0, Some(b));
                }
            }
        }
        // doc_load
        let mut located = false;
        let mut doc: Option<NodeId> = None;
        if schema_location.is_some() || buffer.is_some() {
            let data: Option<Vec<u8>>;
            let url: Option<String>;
            if let Some(l) = &schema_location {
                url = Some(l.clone());
                data = crate::io::read_resource(self.vm, l);
                if data.is_none() {
                    if !crate::io::last_failure_reported() {
                        let msg = format!(
                            "I/O warning : failed to load \"{}\": No such file or directory",
                            l
                        );
                        let mut e = XmlError::simple(
                            FROM_IO,
                            1524,
                            ERR_WARNING,
                            format!("failed to load \"{}\": No such file or directory\n", l),
                        );
                        e.formatted = vec![msg];
                        self.errors.push(e);
                    }
                    if self.vm.current_exception.is_some() {
                        return (-1, None);
                    }
                }
            } else {
                url = None;
                data = buffer.map(|b| b.to_vec());
            }
            if let Some(d) = data {
                let out = crate::sax2::parse_doc(&d, url.as_deref(), XML_PARSE_NOENT);
                self.errors.extend(out.errors);
                if out.well_formed {
                    doc = out.doc;
                }
                if schema_location.is_none() {
                    schema_location = Some("in_memory_buffer".into());
                    if let Some(dd) = doc {
                        tree::with(|t| {
                            if let Some(i) = t.doc_info_mut(dd) {
                                i.url = Some("in_memory_buffer".into());
                            }
                        });
                    }
                }
                if doc.is_none() {
                    let m = format!(
                        "Failed to parse the XML resource '{}'",
                        schema_location.as_deref().unwrap_or("")
                    );
                    self.custom_err(E::SCHEMAP_SRC_IMPORT_2_1, invoking, None, &m);
                    return (self.err, None);
                }
            }
        } else {
            self.perr(
                None,
                E::SCHEMAP_NOTHING_TO_PARSE,
                "No information for parsing was provided with the given schema parser context.\n"
                    .into(),
            );
            return (-1, None);
        }
        let mut target_ns = None;
        if let Some(d) = doc {
            located = true;
            let root = tree::with(|t| t.root_element(d));
            let root = match root {
                None => {
                    let m = format!(
                        "The document '{}' has no document element",
                        schema_location.as_deref().unwrap_or("")
                    );
                    self.custom_err(E::SCHEMAP_NOROOT, invoking, None, &m);
                    return (self.err, None);
                }
                Some(r) => r,
            };
            self.cleanup_doc(root);
            if !is_schema(Some(root), "schema") {
                let m = format!(
                    "The XML document '{}' is not a schema document",
                    schema_location.as_deref().unwrap_or("")
                );
                self.custom_err(E::SCHEMAP_NOT_SCHEMA, invoking, None, &m);
                return (self.err, None);
            }
            target_ns = get_prop(root, "targetNamespace");
        }
        if bkt.is_none() && located {
            self.s.buckets.push(Bucket {
                typ,
                ..Default::default()
            });
            bkt = Some(self.s.buckets.len() - 1);
        }
        if let Some(b) = bkt {
            let bb = &mut self.s.buckets[b];
            bb.schema_location = schema_location.clone();
            bb.located = located;
            if doc.is_some() {
                bb.doc = doc;
                bb.target_ns = target_ns.clone();
                bb.orig_target_ns = target_ns;
            }
            if is_impmain {
                bb.imported += 1;
            }
            if let Some(r) = relation {
                let cur = self.bucket;
                self.s.buckets[cur].relations[r].bucket = Some(b);
            }
        }
        (0, bkt)
    }

    /// xmlSchemaParseNewDoc / xmlSchemaParseNewDocWithContext
    pub fn parse_new_doc(&mut self, b: usize) -> i32 {
        let old_flags = self.s.flags;
        // xmlSchemaClearSchemaDefaults
        self.s.flags &= INCLUDING_CONVERT_NS;
        let old_tns = self.target_ns.clone();
        let old_bucket = self.bucket;
        let old_url = self.url.clone();
        self.url = self.s.buckets[b].schema_location.clone();
        self.target_ns = self.s.buckets[b].target_ns.clone();
        self.bucket = b;
        self.s.buckets[b].parsed = true;
        let doc = self.s.buckets[b].doc.unwrap();
        let node = tree::with(|t| t.root_element(doc)).unwrap();
        let mut ret = self.parse_schema_element(node);
        if ret == 0 && first_child(node).is_some() {
            let old_errs = self.nberrors;
            ret = self.parse_schema_top_level(first_child(node));
            if ret == 0 && old_errs != self.nberrors {
                ret = self.err;
            }
        }
        self.bucket = old_bucket;
        self.target_ns = old_tns;
        self.url = old_url;
        self.s.flags = old_flags;
        ret
    }
}

fn space_preserve(t: &tree::Tree, n: NodeId) -> bool {
    // xmlNodeGetSpacePreserve
    let mut cur = Some(n);
    while let Some(c) = cur {
        if t.nodes[c].t == NType::Element {
            for a in t.attrs_of(c) {
                if t.nodes[a].name == "space" && t.ns_prefix(t.nodes[a].ns) == Some("xml") {
                    let v = t.node_get_content(a);
                    if v == "preserve" {
                        return true;
                    }
                    if v == "default" {
                        return false;
                    }
                }
            }
        }
        cur = t.nodes[c].parent;
    }
    false
}

/// xmlSchemaBuildAbsoluteURI / xmlBuildURI with the node's base
pub fn build_absolute_uri(location: &str, ctxt_node: NodeId) -> String {
    let base = tree::with(|t| {
        let d = t.nodes[ctxt_node].doc?;
        t.doc_info(d).and_then(|i| i.url.clone())
    });
    crate::uri::build_uri(location, base.as_deref()).unwrap_or_else(|| location.to_string())
}

/// xmlSchemaParse: compile a schema from a location or a memory buffer.
pub fn schema_parse(
    vm: &mut Vm,
    url: Option<&str>,
    buffer: Option<&[u8]>,
) -> (Option<Schema>, Vec<XmlError>) {
    let mut p = PCtxt::new(vm);
    p.url = url.map(|s| s.to_string());
    let (res, bucket) = p.add_schema_doc(SCHEMA_MAIN, url, buffer, None, None, None);
    if res == -1 || res != 0 {
        return (None, p.errors);
    }
    let b = match bucket {
        None => {
            match url {
                Some(u) => {
                    let m = format!("Failed to locate the main schema resource at '{}'", u);
                    p.custom_err(E::SCHEMAP_FAILED_LOAD, None, None, &m);
                }
                None => p.custom_err(
                    E::SCHEMAP_FAILED_LOAD,
                    None,
                    None,
                    "Failed to locate the main schema resource",
                ),
            }
            return (None, p.errors);
        }
        Some(b) => b,
    };
    p.main_bucket = b;
    p.bucket = b;
    if p.parse_new_doc(b) == -1 {
        return (None, p.errors);
    }
    if p.nberrors != 0 {
        return (None, p.errors);
    }
    p.s.target_ns = p.s.buckets[b].target_ns.clone();
    if p.fixup_components() == -1 || p.nberrors != 0 {
        return (None, p.errors);
    }
    let PCtxt { s, errors, .. } = p;
    (Some(s), errors)
}

use goro_core::vm::Vm;
