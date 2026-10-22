//! php_schema.c: XML Schema parsing into SDL types.

use std::rc::Rc;

use goro_core::vm::Vm;

use crate::encoding::{
    conv, get_encoder, sdl_guess_convert_xml, sdl_guess_convert_zval, whitespace_collapse,
};
use crate::php::soap_error;
use crate::sdl::SdlCtx;
use crate::types::*;
use crate::xml::{self, Node};

fn node_is_equal_xsd(node: Node, name: &str) -> bool {
    xml::node_is_equal_ex_one_of(
        node,
        name,
        &[XSD_NAMESPACE, XSD_DRAFT_2000_NAMESPACE, XSD_1999_NAMESPACE],
    )
}

fn attr(n: Node, name: &str) -> Option<Node> {
    xml::get_attribute(xml::properties(n), name)
}

fn attr_val(n: Node, name: &str) -> Option<String> {
    attr(n, name).map(xml::attr_value)
}

fn new_enc(ns: &str, type_: &str, st: Option<TypeRef>) -> EncDetails {
    EncDetails {
        type_: 0,
        type_str: Some(type_.to_string()),
        ns: Some(ns.to_string()),
        clark: Some(format!("{{{}}}{}", ns, type_)),
        sdl_type: st,
        map: None,
    }
}

/// create_encoder(): (re)defines the encoder "ns:type" of the SDL.
fn create_encoder(sdl: &SdlRef, cur_type: Option<&TypeRef>, ns: &str, type_: &str) -> EncRef {
    let nscat = format!("{}:{}", ns, type_);
    let existing = sdl
        .borrow()
        .encoders
        .as_ref()
        .and_then(|e| e.find(&nscat).cloned());
    let (fx, fz) = (
        sdl_guess_convert_xml as ToXml,
        sdl_guess_convert_zval as ToZval,
    );
    match existing {
        Some(enc) => {
            *enc.d.borrow_mut() = new_enc(ns, type_, cur_type.cloned());
            enc.to_xml.set(fx);
            enc.to_zval.set(fz);
            enc
        }
        None => {
            let enc = Encoder::new(new_enc(ns, type_, cur_type.cloned()), fx, fz);
            sdl.borrow_mut()
                .encoders
                .get_or_insert_with(Ht::new)
                .update(&nscat, enc.clone());
            enc
        }
    }
}

fn get_create_encoder(sdl: &SdlRef, cur_type: Option<&TypeRef>, ns: &str, type_: &str) -> EncRef {
    match get_encoder(Some(sdl), Some(ns), type_) {
        Some(e) => e,
        None => create_encoder(sdl, cur_type, ns, type_),
    }
}

/// The anonymous encoder of a type declared inline (cur_type->encode).
fn anon_encoder(sdl: &SdlRef, ptr: &TypeRef) -> EncRef {
    let (ns, name) = {
        let p = ptr.borrow();
        (p.namens.clone(), p.name.clone())
    };
    let clark = ns
        .as_ref()
        .map(|n| format!("{{{}}}{}", n, name.clone().unwrap_or_default()));
    let enc = Encoder::new(
        EncDetails {
            type_: 0,
            type_str: name,
            ns,
            clark,
            sdl_type: Some(ptr.clone()),
            map: None,
        },
        sdl_guess_convert_xml,
        sdl_guess_convert_zval,
    );
    sdl.borrow_mut()
        .encoders
        .get_or_insert_with(Ht::new)
        .push(enc.clone());
    enc
}

fn push_type(sdl: &SdlRef, t: TypeRef) {
    sdl.borrow_mut().types.get_or_insert_with(Ht::new).push(t);
}

fn num_types(sdl: &SdlRef) -> usize {
    sdl.borrow().types.as_ref().map_or(0, |t| t.len())
}

fn schema_load_file(
    vm: &mut Vm,
    ctx: &mut SdlCtx,
    ns: Option<Node>,
    location: Option<String>,
    tns: Option<Node>,
    import: bool,
) -> SR<()> {
    let Some(location) = location else {
        return Ok(());
    };
    if ctx.docs.exists(&location) {
        return Ok(());
    }
    let doc = match crate::sdl::load_doc(vm, ctx, &location) {
        Ok(d) => d,
        Err(_) => {
            return Err(soap_error(
                vm,
                format!("Parsing Schema: can't import schema from '{}'", location),
            ));
        }
    };
    let Some(schema) = xml::get_node(xml::children(doc), "schema") else {
        return Err(soap_error(
            vm,
            format!("Parsing Schema: can't import schema from '{}'", location),
        ));
    };
    let new_tns = attr(schema, "targetNamespace");
    if import {
        if let Some(ns) = ns {
            let nsv = xml::attr_value(ns);
            match new_tns {
                None => {
                    return Err(soap_error(
                        vm,
                        format!(
                            "Parsing Schema: can't import schema from '{}', missing 'targetNamespace', expected '{}'",
                            location, nsv
                        ),
                    ));
                }
                Some(nt) if xml::attr_value(nt) != nsv => {
                    return Err(soap_error(
                        vm,
                        format!(
                            "Parsing Schema: can't import schema from '{}', unexpected 'targetNamespace'='{}', expected '{}'",
                            location,
                            xml::attr_value(nt),
                            nsv
                        ),
                    ));
                }
                _ => {}
            }
        }
        if ns.is_none() {
            if let Some(nt) = new_tns {
                return Err(soap_error(
                    vm,
                    format!(
                        "Parsing Schema: can't import schema from '{}', unexpected 'targetNamespace'='{}', expected no 'targetNamespace'",
                        location,
                        xml::attr_value(nt)
                    ),
                ));
            }
        }
    } else {
        match new_tns {
            None => {
                if let Some(t) = tns {
                    xml::set_prop(schema, "targetNamespace", &xml::attr_value(t));
                }
            }
            Some(nt) => {
                if let Some(t) = tns {
                    if xml::attr_value(t) != xml::attr_value(nt) {
                        return Err(soap_error(
                            vm,
                            format!(
                                "Parsing Schema: can't include schema from '{}', different 'targetNamespace'",
                                location
                            ),
                        ));
                    }
                }
            }
        }
    }
    ctx.docs.add(&location, doc);
    load_schema(vm, ctx, schema)
}

/// load_schema()
pub fn load_schema(vm: &mut Vm, ctx: &mut SdlCtx, schema: Node) -> SR<()> {
    {
        let mut s = ctx.sdl.borrow_mut();
        s.types.get_or_insert_with(Ht::new);
    }
    ctx.attributes.get_or_insert_with(Ht::new);
    ctx.attribute_groups.get_or_insert_with(Ht::new);
    let tns = match attr(schema, "targetNamespace") {
        Some(t) => t,
        None => {
            let t = xml::set_prop(schema, "targetNamespace", "");
            xml::new_ns(schema, "", None);
            t
        }
    };
    let sdl = ctx.sdl.clone();
    let mut trav = xml::children(schema);
    while let Some(tr) = trav {
        if node_is_equal_xsd(tr, "include") || node_is_equal_xsd(tr, "redefine") {
            let what = if node_is_equal_xsd(tr, "include") {
                "include"
            } else {
                "redefine"
            };
            match attr(tr, "schemaLocation") {
                None => {
                    return Err(soap_error(
                        vm,
                        format!("Parsing Schema: {} has no 'schemaLocation' attribute", what),
                    ));
                }
                Some(loc) => {
                    let uri = xml::construct_uri(loc);
                    schema_load_file(vm, ctx, None, Some(uri), Some(tns), false)?;
                }
            }
        } else if node_is_equal_xsd(tr, "import") {
            let ns = attr(tr, "namespace");
            let location = attr(tr, "schemaLocation");
            if let Some(n) = ns {
                if xml::attr_value(n) == xml::attr_value(tns) {
                    return Err(match location {
                        Some(l) => soap_error(
                            vm,
                            format!(
                                "Parsing Schema: can't import schema from '{}', namespace must not match the enclosing schema 'targetNamespace'",
                                xml::attr_value(l)
                            ),
                        ),
                        None => soap_error(
                            vm,
                            "Parsing Schema: can't import schema. Namespace must not match the enclosing schema 'targetNamespace'",
                        ),
                    });
                }
            }
            let uri = location.map(xml::construct_uri);
            schema_load_file(vm, ctx, ns, uri, Some(tns), true)?;
        } else if node_is_equal_xsd(tr, "annotation") {
        } else {
            break;
        }
        trav = xml::next(tr);
    }
    while let Some(tr) = trav {
        if node_is_equal_xsd(tr, "simpleType") {
            schema_simple_type(vm, &sdl, tns, tr, None)?;
        } else if node_is_equal_xsd(tr, "complexType") {
            schema_complex_type(vm, &sdl, tns, tr, None)?;
        } else if node_is_equal_xsd(tr, "group") {
            schema_group(vm, &sdl, tns, tr, None, None)?;
        } else if node_is_equal_xsd(tr, "attributeGroup") {
            schema_attribute_group(vm, &sdl, tns, tr, None, Some(ctx))?;
        } else if node_is_equal_xsd(tr, "element") {
            schema_element(vm, &sdl, tns, tr, None, None)?;
        } else if node_is_equal_xsd(tr, "attribute") {
            schema_attribute(vm, &sdl, tns, tr, None, Some(ctx))?;
        } else if node_is_equal_xsd(tr, "notation") || node_is_equal_xsd(tr, "annotation") {
        } else {
            return Err(soap_error(
                vm,
                format!("Parsing Schema: unexpected <{}> in schema", xml::name(tr)),
            ));
        }
        trav = xml::next(tr);
    }
    Ok(())
}

fn skip_annotation(trav: Option<Node>) -> Option<Node> {
    match trav {
        Some(t) if node_is_equal_xsd(t, "annotation") => xml::next(t),
        other => other,
    }
}

fn schema_simple_type(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    simple_type: Node,
    cur_type: Option<&TypeRef>,
) -> SR<()> {
    let ns = attr(simple_type, "targetNamespace").unwrap_or(tns);
    let name = attr(simple_type, "name");
    let cur: TypeRef;
    if let Some(ct) = cur_type {
        // anonymous type inside <element> or <restriction>
        let (nm, nns) = match name {
            Some(n) => (Some(xml::attr_value(n)), Some(xml::attr_value(ns))),
            None => {
                let c = ct.borrow();
                (c.name.clone(), c.namens.clone())
            }
        };
        let new_type = new_type(SdlType {
            kind: XSD_TYPEKIND_SIMPLE,
            name: nm,
            namens: nns,
            ..Default::default()
        });
        push_type(sdl, new_type.clone());
        let enc = anon_encoder(sdl, &new_type);
        ct.borrow_mut().encode = Some(enc);
        cur = new_type;
    } else if let Some(n) = name {
        let new_type = new_type(SdlType {
            kind: XSD_TYPEKIND_SIMPLE,
            name: Some(xml::attr_value(n)),
            namens: Some(xml::attr_value(ns)),
            ..Default::default()
        });
        push_type(sdl, new_type.clone());
        create_encoder(
            sdl,
            Some(&new_type),
            &xml::attr_value(ns),
            &xml::attr_value(n),
        );
        cur = new_type;
    } else {
        return Err(soap_error(
            vm,
            "Parsing Schema: simpleType has no 'name' attribute",
        ));
    }
    let trav = skip_annotation(xml::children(simple_type));
    let trav = match trav {
        Some(t) => {
            if node_is_equal_xsd(t, "restriction") {
                schema_restriction_simple_content(vm, sdl, tns, t, &cur, true)?;
            } else if node_is_equal_xsd(t, "list") {
                cur.borrow_mut().kind = XSD_TYPEKIND_LIST;
                schema_list(vm, sdl, tns, t, &cur)?;
            } else if node_is_equal_xsd(t, "union") {
                cur.borrow_mut().kind = XSD_TYPEKIND_UNION;
                schema_union(vm, sdl, tns, t, &cur)?;
            } else {
                return Err(soap_error(
                    vm,
                    format!(
                        "Parsing Schema: unexpected <{}> in simpleType",
                        xml::name(t)
                    ),
                ));
            }
            xml::next(t)
        }
        None => {
            return Err(soap_error(
                vm,
                "Parsing Schema: expected <restriction>, <list> or <union> in simpleType",
            ));
        }
    };
    if let Some(t) = trav {
        return Err(soap_error(
            vm,
            format!(
                "Parsing Schema: unexpected <{}> in simpleType",
                xml::name(t)
            ),
        ));
    }
    Ok(())
}

fn add_element_anon(cur: &TypeRef, t: TypeRef) {
    cur.borrow_mut()
        .elements
        .get_or_insert_with(Ht::new)
        .push(t);
}

fn schema_list(vm: &mut Vm, sdl: &SdlRef, tns: Node, list_type: Node, cur: &TypeRef) -> SR<()> {
    let item_type = attr(list_type, "itemType");
    if let Some(it) = item_type {
        let (typ, ns) = xml::parse_namespace(&xml::attr_value(it));
        if let Some(nsptr) = xml::search_ns(list_type, ns.as_deref()) {
            let href = xml::ns_href(nsptr);
            let nt = new_type(SdlType {
                name: Some(typ.clone()),
                namens: Some(href.clone()),
                ..Default::default()
            });
            let enc = get_create_encoder(sdl, Some(&nt), &href, &typ);
            nt.borrow_mut().encode = Some(enc);
            add_element_anon(cur, nt);
        }
    }
    let mut trav = skip_annotation(xml::children(list_type));
    if let Some(t) = trav {
        if node_is_equal_xsd(t, "simpleType") {
            if item_type.is_some() {
                return Err(soap_error(
                    vm,
                    "Parsing Schema: element has both 'itemType' attribute and subtype",
                ));
            }
            let nt = new_type(SdlType {
                name: Some(format!("anonymous{}", num_types(sdl))),
                namens: Some(xml::attr_value(tns)),
                ..Default::default()
            });
            add_element_anon(cur, nt.clone());
            schema_simple_type(vm, sdl, tns, t, Some(&nt))?;
            trav = xml::next(t);
        }
    }
    if let Some(t) = trav {
        return Err(soap_error(
            vm,
            format!("Parsing Schema: unexpected <{}> in list", xml::name(t)),
        ));
    }
    Ok(())
}

fn schema_union(vm: &mut Vm, sdl: &SdlRef, tns: Node, union_type: Node, cur: &TypeRef) -> SR<()> {
    if let Some(mt) = attr(union_type, "memberTypes") {
        let s = whitespace_collapse(&xml::attr_value(mt));
        for start in s.split(' ').filter(|p| !p.is_empty()) {
            let (typ, ns) = xml::parse_namespace(start);
            if let Some(nsptr) = xml::search_ns(union_type, ns.as_deref()) {
                let href = xml::ns_href(nsptr);
                let nt = new_type(SdlType {
                    name: Some(typ.clone()),
                    namens: Some(href.clone()),
                    ..Default::default()
                });
                let enc = get_create_encoder(sdl, Some(&nt), &href, &typ);
                nt.borrow_mut().encode = Some(enc);
                add_element_anon(cur, nt);
            }
        }
    }
    let mut trav = skip_annotation(xml::children(union_type));
    while let Some(t) = trav {
        if node_is_equal_xsd(t, "simpleType") {
            let nt = new_type(SdlType {
                name: Some(format!("anonymous{}", num_types(sdl))),
                namens: Some(xml::attr_value(tns)),
                ..Default::default()
            });
            add_element_anon(cur, nt.clone());
            schema_simple_type(vm, sdl, tns, t, Some(&nt))?;
        } else {
            return Err(soap_error(
                vm,
                format!("Parsing Schema: unexpected <{}> in union", xml::name(t)),
            ));
        }
        trav = xml::next(t);
    }
    Ok(())
}

fn schema_simple_content(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    node: Node,
    cur: &TypeRef,
) -> SR<()> {
    let trav = skip_annotation(xml::children(node));
    let trav = match trav {
        Some(t) => {
            if node_is_equal_xsd(t, "restriction") {
                cur.borrow_mut().kind = XSD_TYPEKIND_RESTRICTION;
                schema_restriction_simple_content(vm, sdl, tns, t, cur, false)?;
            } else if node_is_equal_xsd(t, "extension") {
                cur.borrow_mut().kind = XSD_TYPEKIND_EXTENSION;
                schema_extension_simple_content(vm, sdl, tns, t, cur)?;
            } else {
                return Err(soap_error(
                    vm,
                    format!(
                        "Parsing Schema: unexpected <{}> in simpleContent",
                        xml::name(t)
                    ),
                ));
            }
            xml::next(t)
        }
        None => {
            return Err(soap_error(
                vm,
                "Parsing Schema: expected <restriction> or <extension> in simpleContent",
            ));
        }
    };
    if let Some(t) = trav {
        return Err(soap_error(
            vm,
            format!(
                "Parsing Schema: unexpected <{}> in simpleContent",
                xml::name(t)
            ),
        ));
    }
    Ok(())
}

fn base_encoder(sdl: &SdlRef, node: Node, cur: &TypeRef, base: Node) {
    let (typ, ns) = xml::parse_namespace(&xml::attr_value(base));
    if let Some(nsptr) = xml::search_ns(node, ns.as_deref()) {
        let enc = get_create_encoder(sdl, Some(cur), &xml::ns_href(nsptr), &typ);
        cur.borrow_mut().encode = Some(enc);
    }
}

fn restriction_int(vm: &mut Vm, val: Node) -> SR<RestrictionInt> {
    let fixed = attr_val(val, "fixed").is_some_and(|f| f == "true" || f == "1");
    let Some(value) = attr_val(val, "value") else {
        return Err(soap_error(vm, "Parsing Schema: missing restriction value"));
    };
    Ok(RestrictionInt {
        value: atoi(&value),
        fixed,
    })
}

fn restriction_char(vm: &mut Vm, val: Node) -> SR<RestrictionChar> {
    let fixed = attr_val(val, "fixed").is_some_and(|f| f == "true" || f == "1");
    let Some(value) = attr_val(val, "value") else {
        return Err(soap_error(vm, "Parsing Schema: missing restriction value"));
    };
    Ok(RestrictionChar { value, fixed })
}

/// atoi()
pub fn atoi(s: &str) -> i64 {
    let s = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let b = s.as_bytes();
    let mut i = 0;
    let mut neg = false;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        neg = b[i] == b'-';
        i += 1;
    }
    let mut v: i64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        v = v.wrapping_mul(10).wrapping_add((b[i] - b'0') as i64);
        i += 1;
    }
    let v = if neg { -v } else { v };
    v as i32 as i64
}

fn schema_restriction_simple_content(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    rest: Node,
    cur: &TypeRef,
    simple_type: bool,
) -> SR<()> {
    if let Some(base) = attr(rest, "base") {
        base_encoder(sdl, rest, cur, base);
    } else if !simple_type {
        return Err(soap_error(
            vm,
            "Parsing Schema: restriction has no 'base' attribute",
        ));
    }
    cur.borrow_mut()
        .restrictions
        .get_or_insert_with(Restrictions::default);
    let mut trav = skip_annotation(xml::children(rest));
    if let Some(t) = trav {
        if node_is_equal_xsd(t, "simpleType") {
            schema_simple_type(vm, sdl, tns, t, Some(cur))?;
            trav = xml::next(t);
        }
    }
    while let Some(t) = trav {
        macro_rules! set_int {
            ($f:ident) => {{
                let r = restriction_int(vm, t)?;
                cur.borrow_mut().restrictions.as_mut().unwrap().$f = Some(r);
            }};
        }
        macro_rules! set_char {
            ($f:ident) => {{
                let r = restriction_char(vm, t)?;
                cur.borrow_mut().restrictions.as_mut().unwrap().$f = Some(r);
            }};
        }
        if node_is_equal_xsd(t, "minExclusive") {
            set_int!(min_exclusive)
        } else if node_is_equal_xsd(t, "minInclusive") {
            set_int!(min_inclusive)
        } else if node_is_equal_xsd(t, "maxExclusive") {
            set_int!(max_exclusive)
        } else if node_is_equal_xsd(t, "maxInclusive") {
            set_int!(max_inclusive)
        } else if node_is_equal_xsd(t, "totalDigits") {
            set_int!(total_digits)
        } else if node_is_equal_xsd(t, "fractionDigits") {
            set_int!(fraction_digits)
        } else if node_is_equal_xsd(t, "length") {
            set_int!(length)
        } else if node_is_equal_xsd(t, "minLength") {
            set_int!(min_length)
        } else if node_is_equal_xsd(t, "maxLength") {
            set_int!(max_length)
        } else if node_is_equal_xsd(t, "whiteSpace") {
            set_char!(white_space)
        } else if node_is_equal_xsd(t, "pattern") {
            set_char!(pattern)
        } else if node_is_equal_xsd(t, "enumeration") {
            let r = restriction_char(vm, t)?;
            let mut c = cur.borrow_mut();
            let e = c
                .restrictions
                .as_mut()
                .unwrap()
                .enumeration
                .get_or_insert_with(Ht::new);
            let k = r.value.clone();
            e.add(&k, r);
        } else {
            break;
        }
        trav = xml::next(t);
    }
    if !simple_type {
        while let Some(t) = trav {
            if node_is_equal_xsd(t, "attribute") {
                schema_attribute(vm, sdl, tns, t, Some(cur), None)?;
            } else if node_is_equal_xsd(t, "attributeGroup") {
                schema_attribute_group(vm, sdl, tns, t, Some(cur), None)?;
            } else if node_is_equal_xsd(t, "anyAttribute") {
                trav = xml::next(t);
                break;
            } else {
                return Err(soap_error(
                    vm,
                    format!(
                        "Parsing Schema: unexpected <{}> in restriction",
                        xml::name(t)
                    ),
                ));
            }
            trav = xml::next(t);
        }
    }
    if let Some(t) = trav {
        return Err(soap_error(
            vm,
            format!(
                "Parsing Schema: unexpected <{}> in restriction",
                xml::name(t)
            ),
        ));
    }
    Ok(())
}

fn content_particle(vm: &mut Vm, sdl: &SdlRef, tns: Node, t: Node, cur: &TypeRef) -> SR<bool> {
    if node_is_equal_xsd(t, "group") {
        schema_group(vm, sdl, tns, t, Some(cur), None)?;
    } else if node_is_equal_xsd(t, "all") {
        schema_all(vm, sdl, tns, t, cur, None)?;
    } else if node_is_equal_xsd(t, "choice") {
        schema_choice(vm, sdl, tns, t, cur, None)?;
    } else if node_is_equal_xsd(t, "sequence") {
        schema_sequence(vm, sdl, tns, t, cur, None)?;
    } else {
        return Ok(false);
    }
    Ok(true)
}

fn attribute_list(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    mut trav: Option<Node>,
    cur: &TypeRef,
    what: &str,
) -> SR<()> {
    while let Some(t) = trav {
        if node_is_equal_xsd(t, "attribute") {
            schema_attribute(vm, sdl, tns, t, Some(cur), None)?;
        } else if node_is_equal_xsd(t, "attributeGroup") {
            schema_attribute_group(vm, sdl, tns, t, Some(cur), None)?;
        } else if node_is_equal_xsd(t, "anyAttribute") {
            trav = xml::next(t);
            break;
        } else {
            return Err(soap_error(
                vm,
                format!("Parsing Schema: unexpected <{}> in {}", xml::name(t), what),
            ));
        }
        trav = xml::next(t);
    }
    if let Some(t) = trav {
        return Err(soap_error(
            vm,
            format!("Parsing Schema: unexpected <{}> in {}", xml::name(t), what),
        ));
    }
    Ok(())
}

fn schema_restriction_complex_content(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    rest: Node,
    cur: &TypeRef,
) -> SR<()> {
    match attr(rest, "base") {
        Some(base) => base_encoder(sdl, rest, cur, base),
        None => {
            return Err(soap_error(
                vm,
                "Parsing Schema: restriction has no 'base' attribute",
            ));
        }
    }
    let mut trav = skip_annotation(xml::children(rest));
    if let Some(t) = trav {
        if content_particle(vm, sdl, tns, t, cur)? {
            trav = xml::next(t);
        }
    }
    attribute_list(vm, sdl, tns, trav, cur, "restriction")
}

fn schema_extension_simple_content(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    ext: Node,
    cur: &TypeRef,
) -> SR<()> {
    match attr(ext, "base") {
        Some(base) => base_encoder(sdl, ext, cur, base),
        None => {
            return Err(soap_error(
                vm,
                "Parsing Schema: extension has no 'base' attribute",
            ));
        }
    }
    let trav = skip_annotation(xml::children(ext));
    attribute_list(vm, sdl, tns, trav, cur, "extension")
}

fn schema_extension_complex_content(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    ext: Node,
    cur: &TypeRef,
) -> SR<()> {
    match attr(ext, "base") {
        Some(base) => base_encoder(sdl, ext, cur, base),
        None => {
            return Err(soap_error(
                vm,
                "Parsing Schema: extension has no 'base' attribute",
            ));
        }
    }
    let mut trav = skip_annotation(xml::children(ext));
    if let Some(t) = trav {
        if content_particle(vm, sdl, tns, t, cur)? {
            trav = xml::next(t);
        }
    }
    attribute_list(vm, sdl, tns, trav, cur, "extension")
}

/// schema_min_max()
pub fn schema_min_max(node: Node, model: &ModelRef) {
    let min = match attr_val(node, "minOccurs") {
        Some(v) => atoi(&v),
        None => 1,
    };
    let max = match attr_val(node, "maxOccurs") {
        Some(v) => {
            if v == "unbounded" {
                -1
            } else {
                atoi(&v)
            }
        }
        None => 1,
    };
    let mut m = model.borrow_mut();
    m.min_occurs = min;
    m.max_occurs = max;
}

fn attach_model(cur: &TypeRef, model: Option<&ModelRef>, new_model: &ModelRef) {
    match model {
        None => cur.borrow_mut().model = Some(new_model.clone()),
        Some(m) => {
            if let ModelU::Content(c) = &mut m.borrow_mut().u {
                c.push(new_model.clone());
            }
        }
    }
}

fn schema_all(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    all: Node,
    cur: &TypeRef,
    model: Option<&ModelRef>,
) -> SR<()> {
    let nm = ContentModel::new(XSD_CONTENT_ALL, ModelU::Content(Vec::new()));
    attach_model(cur, model, &nm);
    schema_min_max(all, &nm);
    let mut trav = skip_annotation(xml::children(all));
    while let Some(t) = trav {
        if node_is_equal_xsd(t, "element") {
            schema_element(vm, sdl, tns, t, Some(cur), Some(&nm))?;
        } else {
            return Err(soap_error(
                vm,
                format!("Parsing Schema: unexpected <{}> in all", xml::name(t)),
            ));
        }
        trav = xml::next(t);
    }
    Ok(())
}

fn schema_group(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    group: Node,
    cur_type: Option<&TypeRef>,
    model: Option<&ModelRef>,
) -> SR<()> {
    let ns = attr(group, "targetNamespace").unwrap_or(tns);
    let mut name = attr(group, "name");
    let mut ref_ = None;
    if name.is_none() {
        ref_ = attr(group, "ref");
        name = ref_;
    }
    let Some(name) = name else {
        return Err(soap_error(
            vm,
            "Parsing Schema: group has no 'name' nor 'ref' attributes",
        ));
    };
    let new_model;
    let key;
    if let Some(r) = ref_ {
        let (typ, nsp) = xml::parse_namespace(&xml::attr_value(r));
        let mut k = String::new();
        match xml::search_ns(group, nsp.as_deref()) {
            Some(n) => k.push_str(&xml::ns_href(n)),
            None => {
                let ns2 = attr(group, "targetNamespace").unwrap_or(tns);
                k.push_str(&xml::attr_value(ns2));
            }
        }
        k.push(':');
        k.push_str(&typ);
        new_model = ContentModel::new(XSD_CONTENT_GROUP_REF, ModelU::GroupRef(k.clone()));
        key = k;
    } else {
        new_model = ContentModel::new(XSD_CONTENT_SEQUENCE, ModelU::Content(Vec::new()));
        key = format!("{}:{}", xml::attr_value(ns), xml::attr_value(name));
    }
    let cur: TypeRef = match cur_type {
        Some(c) => c.clone(),
        None => {
            let nt = new_type(SdlType::default());
            let added = sdl
                .borrow_mut()
                .groups
                .get_or_insert_with(Ht::new)
                .add(&key, nt.clone());
            if !added {
                return Err(soap_error(
                    vm,
                    format!("Parsing Schema: group '{}' already defined", key),
                ));
            }
            nt
        }
    };
    attach_model(&cur, model, &new_model);
    schema_min_max(group, &new_model);
    let mut trav = skip_annotation(xml::children(group));
    if let Some(t) = trav {
        let kind = if node_is_equal_xsd(t, "choice") {
            Some(XSD_CONTENT_CHOICE)
        } else if node_is_equal_xsd(t, "sequence") {
            Some(XSD_CONTENT_SEQUENCE)
        } else if node_is_equal_xsd(t, "all") {
            Some(XSD_CONTENT_ALL)
        } else {
            None
        };
        match kind {
            Some(k) => {
                if ref_.is_some() {
                    return Err(soap_error(
                        vm,
                        "Parsing Schema: group has both 'ref' attribute and subcontent",
                    ));
                }
                new_model.borrow_mut().kind = k;
                match k {
                    XSD_CONTENT_CHOICE => schema_choice(vm, sdl, tns, t, &cur, Some(&new_model))?,
                    XSD_CONTENT_SEQUENCE => {
                        schema_sequence(vm, sdl, tns, t, &cur, Some(&new_model))?
                    }
                    _ => schema_all(vm, sdl, tns, t, &cur, Some(&new_model))?,
                }
                trav = xml::next(t);
            }
            None => {
                return Err(soap_error(
                    vm,
                    format!("Parsing Schema: unexpected <{}> in group", xml::name(t)),
                ));
            }
        }
    }
    if let Some(t) = trav {
        return Err(soap_error(
            vm,
            format!("Parsing Schema: unexpected <{}> in group", xml::name(t)),
        ));
    }
    Ok(())
}

fn particles(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    node: Node,
    cur: &TypeRef,
    nm: &ModelRef,
    what: &str,
) -> SR<()> {
    let mut trav = skip_annotation(xml::children(node));
    while let Some(t) = trav {
        if node_is_equal_xsd(t, "element") {
            schema_element(vm, sdl, tns, t, Some(cur), Some(nm))?;
        } else if node_is_equal_xsd(t, "group") {
            schema_group(vm, sdl, tns, t, Some(cur), Some(nm))?;
        } else if node_is_equal_xsd(t, "choice") {
            schema_choice(vm, sdl, tns, t, cur, Some(nm))?;
        } else if node_is_equal_xsd(t, "sequence") {
            schema_sequence(vm, sdl, tns, t, cur, Some(nm))?;
        } else if node_is_equal_xsd(t, "any") {
            let am = ContentModel::new(XSD_CONTENT_ANY, ModelU::None);
            schema_min_max(t, &am);
            if let ModelU::Content(c) = &mut nm.borrow_mut().u {
                c.push(am);
            }
        } else {
            return Err(soap_error(
                vm,
                format!("Parsing Schema: unexpected <{}> in {}", xml::name(t), what),
            ));
        }
        trav = xml::next(t);
    }
    Ok(())
}

fn schema_choice(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    node: Node,
    cur: &TypeRef,
    model: Option<&ModelRef>,
) -> SR<()> {
    let nm = ContentModel::new(XSD_CONTENT_CHOICE, ModelU::Content(Vec::new()));
    attach_model(cur, model, &nm);
    schema_min_max(node, &nm);
    particles(vm, sdl, tns, node, cur, &nm, "choice")
}

fn schema_sequence(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    node: Node,
    cur: &TypeRef,
    model: Option<&ModelRef>,
) -> SR<()> {
    let nm = ContentModel::new(XSD_CONTENT_SEQUENCE, ModelU::Content(Vec::new()));
    attach_model(cur, model, &nm);
    schema_min_max(node, &nm);
    particles(vm, sdl, tns, node, cur, &nm, "sequence")
}

fn schema_complex_content(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    node: Node,
    cur: &TypeRef,
) -> SR<()> {
    let trav = skip_annotation(xml::children(node));
    let trav = match trav {
        Some(t) => {
            if node_is_equal_xsd(t, "restriction") {
                cur.borrow_mut().kind = XSD_TYPEKIND_RESTRICTION;
                schema_restriction_complex_content(vm, sdl, tns, t, cur)?;
            } else if node_is_equal_xsd(t, "extension") {
                cur.borrow_mut().kind = XSD_TYPEKIND_EXTENSION;
                schema_extension_complex_content(vm, sdl, tns, t, cur)?;
            } else {
                return Err(soap_error(
                    vm,
                    format!(
                        "Parsing Schema: unexpected <{}> in complexContent",
                        xml::name(t)
                    ),
                ));
            }
            xml::next(t)
        }
        None => {
            return Err(soap_error(
                vm,
                "Parsing Schema: <restriction> or <extension> expected in complexContent",
            ));
        }
    };
    if let Some(t) = trav {
        return Err(soap_error(
            vm,
            format!(
                "Parsing Schema: unexpected <{}> in complexContent",
                xml::name(t)
            ),
        ));
    }
    Ok(())
}

fn schema_complex_type(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    comp: Node,
    cur_type: Option<&TypeRef>,
) -> SR<()> {
    let ns = attr(comp, "targetNamespace").unwrap_or(tns);
    let name = attr(comp, "name");
    let cur: TypeRef;
    if let Some(ct) = cur_type {
        let (nm, nns) = match name {
            Some(n) => (Some(xml::attr_value(n)), Some(xml::attr_value(ns))),
            None => {
                let c = ct.borrow();
                (c.name.clone(), c.namens.clone())
            }
        };
        let nt = new_type(SdlType {
            kind: XSD_TYPEKIND_COMPLEX,
            name: nm,
            namens: nns,
            ..Default::default()
        });
        push_type(sdl, nt.clone());
        let enc = anon_encoder(sdl, &nt);
        ct.borrow_mut().encode = Some(enc);
        cur = nt;
    } else if let Some(n) = name {
        let nt = new_type(SdlType {
            kind: XSD_TYPEKIND_COMPLEX,
            name: Some(xml::attr_value(n)),
            namens: Some(xml::attr_value(ns)),
            ..Default::default()
        });
        push_type(sdl, nt.clone());
        create_encoder(sdl, Some(&nt), &xml::attr_value(ns), &xml::attr_value(n));
        cur = nt;
    } else {
        return Err(soap_error(
            vm,
            "Parsing Schema: complexType has no 'name' attribute",
        ));
    }
    let mut trav = skip_annotation(xml::children(comp));
    if let Some(t) = trav {
        if node_is_equal_xsd(t, "simpleContent") {
            schema_simple_content(vm, sdl, tns, t, &cur)?;
            trav = xml::next(t);
        } else if node_is_equal_xsd(t, "complexContent") {
            schema_complex_content(vm, sdl, tns, t, &cur)?;
            trav = xml::next(t);
        } else {
            if content_particle(vm, sdl, tns, t, &cur)? {
                trav = xml::next(t);
            }
            return attribute_list(vm, sdl, tns, trav, &cur, "complexType");
        }
    }
    if let Some(t) = trav {
        return Err(soap_error(
            vm,
            format!(
                "Parsing Schema: unexpected <{}> in complexType",
                xml::name(t)
            ),
        ));
    }
    Ok(())
}

/// The form of an element or attribute with no 'form' attribute, from the
/// enclosing schema's elementFormDefault / attributeFormDefault.
fn default_form(node: Node, attr_name: &str) -> u8 {
    let mut parent = xml::parent(node);
    while let Some(p) = parent {
        if xml::ntype(p) == goro_ext_xml::tree::NType::Element && node_is_equal_xsd(p, "schema") {
            return match attr_val(p, attr_name) {
                Some(v) if v == "qualified" => XSD_FORM_QUALIFIED,
                _ => XSD_FORM_UNQUALIFIED,
            };
        }
        parent = xml::parent(p);
    }
    XSD_FORM_UNQUALIFIED
}

fn schema_element(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    element: Node,
    cur_type: Option<&TypeRef>,
    model: Option<&ModelRef>,
) -> SR<()> {
    let ns = attr(element, "targetNamespace").unwrap_or(tns);
    let mut name = attr(element, "name");
    let mut ref_ = None;
    if name.is_none() {
        ref_ = attr(element, "ref");
        name = ref_;
    }
    if name.is_none() {
        return Err(soap_error(
            vm,
            "Parsing Schema: element has no 'name' nor 'ref' attributes",
        ));
    }
    let name = name.unwrap();
    let nt = new_type(SdlType::default());
    if let Some(r) = ref_ {
        let (typ, nsp) = xml::parse_namespace(&xml::attr_value(r));
        let mut nscat = String::new();
        match xml::search_ns(element, nsp.as_deref()) {
            Some(n) => {
                let h = xml::ns_href(n);
                nscat.push_str(&h);
                nt.borrow_mut().namens = Some(h);
            }
            None => {
                let ns2 = attr(element, "targetNamespace").unwrap_or(tns);
                nscat.push_str(&xml::attr_value(ns2));
            }
        }
        nscat.push(':');
        nscat.push_str(&typ);
        let mut t = nt.borrow_mut();
        t.name = Some(typ);
        t.ref_ = Some(nscat);
    } else {
        let mut t = nt.borrow_mut();
        t.name = Some(xml::attr_value(name));
        t.namens = Some(xml::attr_value(ns));
    }
    let (tname, tns_s) = {
        let t = nt.borrow();
        (
            t.name.clone().unwrap_or_default(),
            t.namens.clone().unwrap_or_default(),
        )
    };
    match cur_type {
        None => {
            let key = format!("{}:{}", tns_s, tname);
            let added = sdl
                .borrow_mut()
                .elements
                .get_or_insert_with(Ht::new)
                .add(&key, nt.clone());
            if !added {
                return Err(soap_error(
                    vm,
                    format!("Parsing Schema: element '{}' already defined", key),
                ));
            }
        }
        Some(ct) => {
            let mut c = ct.borrow_mut();
            let els = c.elements.get_or_insert_with(Ht::new);
            if !els.add(&tname, nt.clone()) {
                els.push(nt.clone());
            }
        }
    }
    if let Some(m) = model {
        let nm = ContentModel::new(XSD_CONTENT_ELEMENT, ModelU::Element(nt.clone()));
        schema_min_max(element, &nm);
        if let ModelU::Content(c) = &mut m.borrow_mut().u {
            c.push(nm);
        }
    }
    let cur = nt;
    // nillable
    if let Some(v) = attr_val(element, "nillable") {
        if ref_.is_some() {
            return Err(soap_error(
                vm,
                "Parsing Schema: element has both 'ref' and 'nillable' attributes",
            ));
        }
        cur.borrow_mut().nillable = v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("1");
    } else {
        cur.borrow_mut().nillable = false;
    }
    if let Some(v) = attr_val(element, "fixed") {
        if ref_.is_some() {
            return Err(soap_error(
                vm,
                "Parsing Schema: element has both 'ref' and 'fixed' attributes",
            ));
        }
        cur.borrow_mut().fixed = Some(v);
    }
    if let Some(v) = attr_val(element, "default") {
        if ref_.is_some() {
            return Err(soap_error(
                vm,
                "Parsing Schema: element has both 'default' and 'fixed' attributes",
            ));
        }
        cur.borrow_mut().def = Some(v);
    }
    let form = match attr_val(element, "form") {
        Some(v) if v == "qualified" => XSD_FORM_QUALIFIED,
        Some(v) if v == "unqualified" => XSD_FORM_UNQUALIFIED,
        _ => XSD_FORM_DEFAULT,
    };
    let form = if form == XSD_FORM_DEFAULT {
        default_form(element, "elementFormDefault")
    } else {
        form
    };
    cur.borrow_mut().form = form;
    let type_attr = attr(element, "type");
    if let Some(ta) = type_attr {
        if ref_.is_some() {
            return Err(soap_error(
                vm,
                "Parsing Schema: element has both 'ref' and 'type' attributes",
            ));
        }
        let (cptype, nsp) = xml::parse_namespace(&xml::attr_value(ta));
        if let Some(n) = xml::search_ns(element, nsp.as_deref()) {
            let enc = get_create_encoder(sdl, Some(&cur), &xml::ns_href(n), &cptype);
            cur.borrow_mut().encode = Some(enc);
        }
    }
    let mut trav = skip_annotation(xml::children(element));
    if let Some(t) = trav {
        let simple = node_is_equal_xsd(t, "simpleType");
        let complex = !simple && node_is_equal_xsd(t, "complexType");
        if simple || complex {
            if ref_.is_some() {
                return Err(soap_error(
                    vm,
                    "Parsing Schema: element has both 'ref' attribute and subtype",
                ));
            } else if type_attr.is_some() {
                return Err(soap_error(
                    vm,
                    "Parsing Schema: element has both 'type' attribute and subtype",
                ));
            }
            if simple {
                schema_simple_type(vm, sdl, tns, t, Some(&cur))?;
            } else {
                schema_complex_type(vm, sdl, tns, t, Some(&cur))?;
            }
            trav = xml::next(t);
        }
    }
    while let Some(t) = trav {
        if node_is_equal_xsd(t, "unique")
            || node_is_equal_xsd(t, "key")
            || node_is_equal_xsd(t, "keyref")
        {
        } else {
            return Err(soap_error(
                vm,
                format!("Parsing Schema: unexpected <{}> in element", xml::name(t)),
            ));
        }
        trav = xml::next(t);
    }
    Ok(())
}

fn schema_attribute(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    attr_type: Node,
    cur_type: Option<&TypeRef>,
    ctx: Option<&mut SdlCtx>,
) -> SR<()> {
    let mut name = attr(attr_type, "name");
    let mut ref_ = None;
    if name.is_none() {
        ref_ = attr(attr_type, "ref");
        name = ref_;
    }
    if name.is_none() {
        return Err(soap_error(
            vm,
            "Parsing Schema: attribute has no 'name' nor 'ref' attributes",
        ));
    }
    let name = name.unwrap();
    let new_attr: AttrRef = Rc::new(std::cell::RefCell::new(SdlAttribute::default()));
    let mut key = String::new();
    if let Some(r) = ref_ {
        let (attr_name, nsp) = xml::parse_namespace(&xml::attr_value(r));
        match xml::search_ns(attr_type, nsp.as_deref()) {
            Some(n) => {
                let h = xml::ns_href(n);
                key.push_str(&h);
                new_attr.borrow_mut().namens = Some(h);
            }
            None => {
                let ns2 = attr(attr_type, "targetNamespace").unwrap_or(tns);
                key.push_str(&xml::attr_value(ns2));
            }
        }
        key.push(':');
        key.push_str(&attr_name);
        new_attr.borrow_mut().ref_ = Some(key.clone());
    } else {
        let ns = attr(attr_type, "targetNamespace").unwrap_or(tns);
        let v = xml::attr_value(ns);
        key.push_str(&v);
        key.push(':');
        new_attr.borrow_mut().namens = Some(v);
        key.push_str(&xml::attr_value(name));
    }
    let added = match cur_type {
        None => ctx
            .expect("ctx")
            .attributes
            .get_or_insert_with(Ht::new)
            .add(&key, new_attr.clone()),
        Some(ct) => ct
            .borrow_mut()
            .attributes
            .get_or_insert_with(Ht::new)
            .add(&key, new_attr.clone()),
    };
    if !added {
        return Err(soap_error(
            vm,
            format!("Parsing Schema: attribute '{}' already defined", key),
        ));
    }
    let type_attr = attr(attr_type, "type");
    if let Some(ta) = type_attr {
        if ref_.is_some() {
            return Err(soap_error(
                vm,
                "Parsing Schema: attribute has both 'ref' and 'type' attributes",
            ));
        }
        let (cptype, nsp) = xml::parse_namespace(&xml::attr_value(ta));
        if let Some(n) = xml::search_ns(attr_type, nsp.as_deref()) {
            let enc = get_create_encoder(sdl, cur_type, &xml::ns_href(n), &cptype);
            new_attr.borrow_mut().encode = Some(enc);
        }
    }
    let mut a = xml::properties(attr_type);
    while let Some(at) = a {
        let v = xml::attr_value(at);
        if xml::attr_is_equal_ex(at, "default", None) {
            new_attr.borrow_mut().def = Some(v);
        } else if xml::attr_is_equal_ex(at, "fixed", None) {
            new_attr.borrow_mut().fixed = Some(v);
        } else if xml::attr_is_equal_ex(at, "form", None) {
            new_attr.borrow_mut().form = match v.as_str() {
                "qualified" => XSD_FORM_QUALIFIED,
                "unqualified" => XSD_FORM_UNQUALIFIED,
                _ => XSD_FORM_DEFAULT,
            };
        } else if xml::attr_is_equal_ex(at, "id", None) {
        } else if xml::attr_is_equal_ex(at, "name", None) {
            new_attr.borrow_mut().name = Some(v);
        } else if xml::attr_is_equal_ex(at, "ref", None) || xml::attr_is_equal_ex(at, "type", None)
        {
        } else if xml::attr_is_equal_ex(at, "use", None) {
            new_attr.borrow_mut().use_ = match v.as_str() {
                "prohibited" => XSD_USE_PROHIBITED,
                "required" => XSD_USE_REQUIRED,
                "optional" => XSD_USE_OPTIONAL,
                _ => XSD_USE_DEFAULT,
            };
        } else if let Some(href) = xml::ns_href_of(at) {
            if href != XSD_NAMESPACE {
                let (value, nsp) = xml::parse_namespace(&v);
                let parent = xml::parent(at).unwrap_or(attr_type);
                let ext = match xml::search_ns(parent, nsp.as_deref()) {
                    Some(n) => ExtraAttr {
                        ns: Some(xml::ns_href(n)),
                        val: value,
                    },
                    None => ExtraAttr { ns: None, val: v },
                };
                let k2 = format!("{}:{}", href, xml::name(at));
                new_attr
                    .borrow_mut()
                    .extra
                    .get_or_insert_with(Ht::new)
                    .add(&k2, ext);
            }
        }
        a = xml::next(at);
    }
    if new_attr.borrow().form == XSD_FORM_DEFAULT {
        new_attr.borrow_mut().form = default_form(attr_type, "attributeFormDefault");
    }
    let mut trav = skip_annotation(xml::children(attr_type));
    if let Some(t) = trav {
        if node_is_equal_xsd(t, "simpleType") {
            if ref_.is_some() {
                return Err(soap_error(
                    vm,
                    "Parsing Schema: attribute has both 'ref' attribute and subtype",
                ));
            } else if type_attr.is_some() {
                return Err(soap_error(
                    vm,
                    "Parsing Schema: attribute has both 'type' attribute and subtype",
                ));
            }
            let dummy = new_type(SdlType {
                name: Some(format!("anonymous{}", num_types(sdl))),
                namens: Some(xml::attr_value(tns)),
                ..Default::default()
            });
            schema_simple_type(vm, sdl, tns, t, Some(&dummy))?;
            new_attr.borrow_mut().encode = dummy.borrow().encode.clone();
            trav = xml::next(t);
        }
    }
    if let Some(t) = trav {
        return Err(soap_error(
            vm,
            format!("Parsing Schema: unexpected <{}> in attribute", xml::name(t)),
        ));
    }
    Ok(())
}

fn schema_attribute_group(
    vm: &mut Vm,
    sdl: &SdlRef,
    tns: Node,
    group: Node,
    cur_type: Option<&TypeRef>,
    ctx: Option<&mut SdlCtx>,
) -> SR<()> {
    let mut name = attr(group, "name");
    let mut ref_ = None;
    if name.is_none() {
        ref_ = attr(group, "ref");
        name = ref_;
    }
    let mut cur: Option<TypeRef> = cur_type.cloned();
    match name {
        Some(name) => {
            if let Some(ct) = cur_type.filter(|_| ref_.is_some()) {
                let r = ref_.unwrap();
                let (group_name, nsp) = xml::parse_namespace(&xml::attr_value(r));
                let mut key = String::new();
                if let Some(n) = xml::search_ns(group, nsp.as_deref()) {
                    key.push_str(&xml::ns_href(n));
                }
                key.push(':');
                key.push_str(&group_name);
                let new_attr = SdlAttribute {
                    ref_: Some(key),
                    ..Default::default()
                };
                ct.borrow_mut()
                    .attributes
                    .get_or_insert_with(Ht::new)
                    .push(Rc::new(std::cell::RefCell::new(new_attr)));
                cur = None;
            } else if cur_type.is_none() {
                let ns = attr(group, "targetNamespace").unwrap_or(tns);
                let nt = new_type(SdlType {
                    name: Some(xml::attr_value(name)),
                    namens: Some(xml::attr_value(ns)),
                    ..Default::default()
                });
                let key = format!("{}:{}", xml::attr_value(ns), xml::attr_value(name));
                let added = ctx
                    .expect("ctx")
                    .attribute_groups
                    .get_or_insert_with(Ht::new)
                    .add(&key, nt.clone());
                if !added {
                    return Err(soap_error(
                        vm,
                        format!("Parsing Schema: attributeGroup '{}' already defined", key),
                    ));
                }
                cur = Some(nt);
            }
        }
        None => {
            return Err(soap_error(
                vm,
                "Parsing Schema: attributeGroup has no 'name' nor 'ref' attributes",
            ));
        }
    }
    let mut trav = skip_annotation(xml::children(group));
    while let Some(t) = trav {
        if node_is_equal_xsd(t, "attribute") {
            if ref_.is_some() {
                return Err(soap_error(
                    vm,
                    "Parsing Schema: attributeGroup has both 'ref' attribute and subattribute",
                ));
            }
            schema_attribute(vm, sdl, tns, t, cur.as_ref(), None)?;
        } else if node_is_equal_xsd(t, "attributeGroup") {
            if ref_.is_some() {
                return Err(soap_error(
                    vm,
                    "Parsing Schema: attributeGroup has both 'ref' attribute and subattribute",
                ));
            }
            schema_attribute_group(vm, sdl, tns, t, cur.as_ref(), None)?;
        } else if node_is_equal_xsd(t, "anyAttribute") {
            if ref_.is_some() {
                return Err(soap_error(
                    vm,
                    "Parsing Schema: attributeGroup has both 'ref' attribute and subattribute",
                ));
            }
            trav = xml::next(t);
            break;
        } else {
            return Err(soap_error(
                vm,
                format!(
                    "Parsing Schema: unexpected <{}> in attributeGroup",
                    xml::name(t)
                ),
            ));
        }
        trav = xml::next(t);
    }
    if let Some(t) = trav {
        return Err(soap_error(
            vm,
            format!(
                "Parsing Schema: unexpected <{}> in attributeGroup",
                xml::name(t)
            ),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Pass 2: resolving references

fn find_by_ref<V: Clone>(ht: &Ht<V>, r: &str) -> Option<V> {
    if let Some(v) = ht.find(r) {
        return Some(v.clone());
    }
    if let Some(p) = r.rfind(':') {
        return ht.find(&r[p..]).cloned();
    }
    None
}

fn schema_attribute_fixup(ctx: &SdlCtx, attr: &AttrRef) {
    let r = attr.borrow().ref_.clone();
    let Some(r) = r else { return };
    if let Some(attrs) = &ctx.attributes {
        if let Some(tmp) = find_by_ref(attrs, &r) {
            if !Rc::ptr_eq(&tmp, attr) {
                schema_attribute_fixup(ctx, &tmp);
                let t = tmp.borrow().clone();
                let mut a = attr.borrow_mut();
                if t.name.is_some() && a.name.is_none() {
                    a.name = t.name.clone();
                }
                if t.namens.is_some() && a.namens.is_none() {
                    a.namens = t.namens.clone();
                }
                if t.def.is_some() && a.def.is_none() {
                    a.def = t.def.clone();
                }
                if t.fixed.is_some() && a.fixed.is_none() {
                    a.fixed = t.fixed.clone();
                }
                if a.form == XSD_FORM_DEFAULT {
                    a.form = t.form;
                }
                if a.use_ == XSD_USE_DEFAULT {
                    a.use_ = t.use_;
                }
                if t.extra.is_some() {
                    a.extra = t.extra.clone();
                }
                a.encode = t.encode.clone();
            }
        }
    }
    let mut a = attr.borrow_mut();
    if a.name.is_none() {
        a.name = Some(match r.rfind(':') {
            Some(p) => r[p + 1..].to_string(),
            None => r.clone(),
        });
    }
    a.ref_ = None;
}

fn schema_attributegroup_fixup(ctx: &SdlCtx, attr: &AttrRef, ht: &mut Ht<AttrRef>) {
    let r = attr.borrow().ref_.clone();
    let Some(r) = r else { return };
    if let Some(groups) = &ctx.attribute_groups {
        if let Some(tmp) = find_by_ref(groups, &r) {
            let entries: Vec<(HKey, AttrRef)> = {
                let t = tmp.borrow();
                t.attributes
                    .as_ref()
                    .map(|a| a.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default()
            };
            for (k, tmp_attr) in entries {
                match k {
                    HKey::Str(key) => {
                        schema_attribute_fixup(ctx, &tmp_attr);
                        let copy = tmp_attr.borrow().clone();
                        ht.add(&key, Rc::new(std::cell::RefCell::new(copy)));
                    }
                    HKey::Int(i) => {
                        schema_attributegroup_fixup(ctx, &tmp_attr, ht);
                        if let Some(a) = tmp.borrow_mut().attributes.as_mut() {
                            a.del_idx(i);
                        }
                    }
                }
            }
        }
    }
    attr.borrow_mut().ref_ = None;
}

fn schema_content_model_fixup(vm: &mut Vm, ctx: &SdlCtx, model: &ModelRef) -> SR<()> {
    let kind = model.borrow().kind;
    match kind {
        XSD_CONTENT_GROUP_REF => {
            let r = match &model.borrow().u {
                ModelU::GroupRef(r) => r.clone(),
                _ => String::new(),
            };
            let tmp = ctx
                .sdl
                .borrow()
                .groups
                .as_ref()
                .and_then(|g| g.find(&r).cloned());
            match tmp {
                Some(t) => {
                    schema_type_fixup(vm, ctx, &t)?;
                    let mut m = model.borrow_mut();
                    m.kind = XSD_CONTENT_GROUP;
                    m.u = ModelU::Group(t);
                }
                None => {
                    return Err(soap_error(
                        vm,
                        format!("Parsing Schema: unresolved group 'ref' attribute '{}'", r),
                    ));
                }
            }
        }
        XSD_CONTENT_CHOICE | XSD_CONTENT_SEQUENCE | XSD_CONTENT_ALL => {
            if kind == XSD_CONTENT_CHOICE {
                let max = model.borrow().max_occurs;
                if max != 1 {
                    for tmp in model.borrow().content() {
                        let mut t = tmp.borrow_mut();
                        t.min_occurs = 0;
                        t.max_occurs = max;
                    }
                    let mut m = model.borrow_mut();
                    m.kind = XSD_CONTENT_ALL;
                    m.min_occurs = 1;
                    m.max_occurs = 1;
                }
            }
            for tmp in model.borrow().content() {
                schema_content_model_fixup(vm, ctx, &tmp)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn schema_type_fixup(vm: &mut Vm, ctx: &SdlCtx, type_: &TypeRef) -> SR<()> {
    let r = type_.borrow().ref_.clone();
    if let Some(r) = r {
        let els = ctx.sdl.borrow().elements.clone();
        if let Some(els) = els {
            match find_by_ref(&els, &r) {
                Some(tmp) => {
                    if !Rc::ptr_eq(&tmp, type_) {
                        let t = tmp.borrow().clone();
                        let mut ty = type_.borrow_mut();
                        ty.kind = t.kind;
                        ty.encode = t.encode.clone();
                        if t.nillable {
                            ty.nillable = true;
                        }
                        if t.fixed.is_some() {
                            ty.fixed = t.fixed.clone();
                        }
                        if t.def.is_some() {
                            ty.def = t.def.clone();
                        }
                        ty.form = t.form;
                    }
                }
                None => {
                    if r == format!("{}:schema", XSD_NAMESPACE) {
                        type_.borrow_mut().encode = Some(conv(XSD_ANYXML));
                    } else {
                        return Err(soap_error(
                            vm,
                            format!("Parsing Schema: unresolved element 'ref' attribute '{}'", r),
                        ));
                    }
                }
            }
        }
        type_.borrow_mut().ref_ = None;
    }
    let els: Vec<TypeRef> = type_
        .borrow()
        .elements
        .as_ref()
        .map(|e| e.values().cloned().collect())
        .unwrap_or_default();
    for t in els {
        schema_type_fixup(vm, ctx, &t)?;
    }
    let model = type_.borrow().model.clone();
    if let Some(m) = model {
        schema_content_model_fixup(vm, ctx, &m)?;
    }
    if type_.borrow().attributes.is_some() {
        let mut ht = type_.borrow_mut().attributes.take().unwrap();
        let mut pos = 0;
        while pos < ht.len() {
            let (k, a) = {
                let (k, a) = ht.get_index(pos).unwrap();
                (k.clone(), a.clone())
            };
            match k {
                HKey::Str(_) => {
                    schema_attribute_fixup(ctx, &a);
                    pos += 1;
                }
                HKey::Int(i) => {
                    schema_attributegroup_fixup(ctx, &a, &mut ht);
                    ht.del_idx(i);
                }
            }
        }
        type_.borrow_mut().attributes = Some(ht);
    }
    Ok(())
}

/// schema_pass2()
pub fn schema_pass2(vm: &mut Vm, ctx: &mut SdlCtx) -> SR<()> {
    if let Some(attrs) = &ctx.attributes {
        let list: Vec<AttrRef> = attrs.values().cloned().collect();
        for a in list {
            schema_attribute_fixup(ctx, &a);
        }
    }
    let groups: Vec<TypeRef> = ctx
        .attribute_groups
        .as_ref()
        .map(|g| g.values().cloned().collect())
        .unwrap_or_default();
    for t in groups {
        schema_type_fixup(vm, ctx, &t)?;
    }
    let (elements, sgroups, types) = {
        let s = ctx.sdl.borrow();
        let f = |h: &Option<Ht<TypeRef>>| -> Vec<TypeRef> {
            h.as_ref()
                .map(|x| x.values().cloned().collect())
                .unwrap_or_default()
        };
        (f(&s.elements), f(&s.groups), f(&s.types))
    };
    for t in elements.iter().chain(sgroups.iter()).chain(types.iter()) {
        schema_type_fixup(vm, ctx, t)?;
    }
    ctx.attributes = None;
    ctx.attribute_groups = None;
    Ok(())
}
