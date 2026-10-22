//! clean.c: the clean-up passes (presentational markup to style rules,
//! Word 2000, emphasis nesting, anchors, xhtml namespace, ...).

use crate::attrs;
use crate::doc::*;
use crate::enums::code::*;
use crate::enums::{AttrId, OptId, TagId};
use crate::lexer;
use crate::message;
use crate::parser;
use crate::streamio;
use crate::tags::{self, ParserFn};
use crate::tmbstr;
use crate::utf8;

/// A StyleProp list entry (the list is a Vec in list order).
struct Prop {
    name: Vec<u8>,
    value: Option<Vec<u8>>,
}

fn is(doc: &Doc, node: NodeId, tid: TagId) -> bool {
    doc.is(node, tid)
}

fn attr(doc: &Doc, node: NodeId, id: AttrId) -> Option<AttId> {
    attrs::attr_get_by_id(doc, node, id)
}

fn has_parser(doc: &Doc, node: NodeId, f: ParserFn) -> bool {
    doc.parser_of(node)
        .is_some_and(|p| std::ptr::fn_addr_eq(p, f))
}

/// `tmbsnprintf(buf, size, ...)`: vsnprintf(buf, size - 1) keeps size - 2 bytes.
fn trunc(mut s: Vec<u8>, size: usize) -> Vec<u8> {
    s.truncate(size - 2);
    s
}

/// `"%s"` of a possibly NULL string (glibc prints "(null)").
fn s_or_null(v: Option<&[u8]>) -> &[u8] {
    v.unwrap_or(b"(null)")
}

/// RenameElem()
fn rename_elem(doc: &mut Doc, node: NodeId, tid: TagId) {
    let def = tags::builtin_def(tid);
    doc[node].element = Some(def.name.to_vec());
    doc[node].tag = tags::lookup_tag_def(tid);
}

/// InsertProperty()
fn insert_property(props: &mut Vec<Prop>, name: &[u8], value: &[u8]) {
    for i in 0..props.len() {
        let cmp = tmbstr::tmbstrcmp(&props[i].name, name);
        if cmp == 0 {
            // this property is already defined, ignore new value
            return;
        }
        if cmp > 0 {
            props.insert(
                i,
                Prop {
                    name: tmbstr::cstr(name).to_vec(),
                    value: Some(tmbstr::cstr(value).to_vec()),
                },
            );
            return;
        }
    }
    props.push(Prop {
        name: tmbstr::cstr(name).to_vec(),
        value: Some(tmbstr::cstr(value).to_vec()),
    });
}

/// CreateProps(): sorted list of properties from a style string
fn create_props(props: &mut Vec<Prop>, style: &[u8]) {
    let line = tmbstr::cstr(style);
    let at = |i: usize| line.get(i).copied().unwrap_or(0);
    let mut name = 0usize;
    while at(name) != 0 {
        while at(name) == b' ' {
            name += 1;
        }
        let mut name_end = name;
        let mut value = 0usize;
        while at(name_end) != 0 {
            if at(name_end) == b':' {
                value = name_end + 1;
                break;
            }
            name_end += 1;
        }
        if at(name_end) != b':' {
            break;
        }
        while at(value) == b' ' {
            value += 1;
        }
        let mut value_end = value;
        let mut more = false;
        while at(value_end) != 0 {
            if at(value_end) == b';' {
                more = true;
                break;
            }
            value_end += 1;
        }
        insert_property(props, &line[name..name_end], &line[value..value_end]);
        if more {
            name = value_end + 1;
            continue;
        }
        break;
    }
}

/// CreatePropString()
fn create_prop_string(props: &[Prop]) -> Vec<u8> {
    let mut style = Vec::new();
    for (i, prop) in props.iter().enumerate() {
        style.extend_from_slice(&prop.name);
        if let Some(v) = &prop.value {
            style.extend_from_slice(b": ");
            style.extend_from_slice(v);
        }
        if i + 1 == props.len() {
            break;
        }
        style.extend_from_slice(b"; ");
    }
    style
}

/// FreeStyles()
pub fn free_styles(doc: &mut Doc) {
    if let Some(lx) = doc.lexer.as_mut() {
        lx.styles.clear();
    }
}

/// GensymClass()
fn gensym_class(doc: &mut Doc) -> Vec<u8> {
    let pfx = match doc.cfg_str(OptId::CSSPrefix) {
        Some(p) if !p.is_empty() && p[0] != 0 => tmbstr::cstr(p).to_vec(),
        _ => b"c".to_vec(),
    };
    doc.n_class_id = doc.n_class_id.wrapping_add(1);
    let mut buf = pfx;
    buf.extend_from_slice(doc.n_class_id.to_string().as_bytes());
    trunc(buf, 512)
}

/// FindStyle()
fn find_style(doc: &mut Doc, tag: &[u8], properties: &[u8]) -> Vec<u8> {
    for style in &doc.lxr().styles {
        if tmbstr::tmbstrcmp(&style.tag, tag) == 0
            && tmbstr::tmbstrcmp(&style.properties, properties) == 0
        {
            return style.tag_class.clone();
        }
    }
    let tag_class = gensym_class(doc);
    let style = TagStyle {
        tag: tmbstr::cstr(tag).to_vec(),
        tag_class: tag_class.clone(),
        properties: tmbstr::cstr(properties).to_vec(),
    };
    // style->next = lexer->styles; lexer->styles = style;
    doc.lx().styles.insert(0, style);
    tag_class
}

/// AddClass(): add class="foo" to node
fn add_class(doc: &mut Doc, node: NodeId, classname: &[u8]) {
    match attr(doc, node, AttrId::CLASS) {
        Some(classattr) => attrs::append_to_class_attr(doc, classattr, classname),
        None => {
            attrs::add_attribute(doc, node, b"class", classname);
        }
    }
}

/// AddStyleAsClass()
pub fn add_style_as_class(doc: &mut Doc, node: NodeId, stylevalue: &[u8]) {
    let el = doc[node].element.clone().unwrap_or_default();
    let classname = find_style(doc, &el, stylevalue);
    add_class(doc, node, &classname);
}

/// Style2Rule(): replace the style attribute by a class attribute
fn style2rule(doc: &mut Doc, node: NodeId) {
    if let Some(styleattr) = attr(doc, node, AttrId::STYLE) {
        let Some(value) = doc[styleattr].value.clone() else {
            lexer::remove_attribute(doc, node, styleattr);
            return;
        };
        let el = doc[node].element.clone().unwrap_or_default();
        let classname = find_style(doc, &el, &value);
        match attr(doc, node, AttrId::CLASS) {
            Some(classattr) => {
                attrs::append_to_class_attr(doc, classattr, &classname);
                lexer::remove_attribute(doc, node, styleattr);
            }
            None => {
                doc[styleattr].attribute = Some(b"class".to_vec());
                doc[styleattr].value = Some(classname);
            }
        }
    }
}

/// AddColorRule()
fn add_color_rule(doc: &mut Doc, selector: &[u8], color: Option<&[u8]>) {
    if let Some(color) = color {
        lexer::add_string_literal(doc, selector);
        lexer::add_string_literal(doc, b" { color: ");
        lexer::add_string_literal(doc, color);
        lexer::add_string_literal(doc, b" }\n");
    }
}

/// Take an attribute's value and remove the attribute.
fn take_attr(doc: &mut Doc, body: NodeId, id: AttrId) -> Option<Vec<u8>> {
    let a = attr(doc, body, id)?;
    let v = doc[a].value.take();
    lexer::remove_attribute(doc, body, a);
    v
}

/// CleanBodyAttrs(): move presentation attribs from body to style element
fn clean_body_attrs(doc: &mut Doc, body: NodeId) {
    let bgurl = take_attr(doc, body, AttrId::BACKGROUND);
    let bgcolor = take_attr(doc, body, AttrId::BGCOLOR);
    let color = take_attr(doc, body, AttrId::TEXT);

    if bgurl.is_some() || bgcolor.is_some() || color.is_some() {
        lexer::add_string_literal(doc, b" body {\n");
        if let Some(u) = bgurl {
            lexer::add_string_literal(doc, b"  background-image: url(");
            lexer::add_string_literal(doc, &u);
            lexer::add_string_literal(doc, b");\n");
        }
        if let Some(c) = bgcolor {
            lexer::add_string_literal(doc, b"  background-color: ");
            lexer::add_string_literal(doc, &c);
            lexer::add_string_literal(doc, b";\n");
        }
        if let Some(c) = color {
            lexer::add_string_literal(doc, b"  color: ");
            lexer::add_string_literal(doc, &c);
            lexer::add_string_literal(doc, b";\n");
        }
        lexer::add_string_literal(doc, b" }\n");
    }

    for (id, sel) in [
        (AttrId::LINK, &b" :link"[..]),
        (AttrId::VLINK, &b" :visited"[..]),
        (AttrId::ALINK, &b" :active"[..]),
    ] {
        if let Some(a) = attr(doc, body, id) {
            let v = doc[a].value.clone();
            add_color_rule(doc, sel, v.as_deref());
            lexer::remove_attribute(doc, body, a);
        }
    }
}

/// NiceBody()
fn nice_body(doc: &mut Doc) -> bool {
    if let Some(node) = lexer::find_body(doc) {
        if [
            AttrId::BACKGROUND,
            AttrId::BGCOLOR,
            AttrId::TEXT,
            AttrId::LINK,
            AttrId::VLINK,
            AttrId::ALINK,
        ]
        .iter()
        .any(|&id| attr(doc, node, id).is_some())
        {
            doc.bad_layout |= USING_BODY;
            return false;
        }
    }
    true
}

/// CreateStyleElement(): create style element using rules from dictionary
fn create_style_element(doc: &mut Doc) {
    if doc.lxr().styles.is_empty() && nice_body(doc) {
        return;
    }
    let node = lexer::new_node(doc);
    doc[node].type_ = NodeType::StartTag;
    doc[node].implicit = true;
    doc[node].element = Some(b"style".to_vec());
    tags::find_tag(doc, node);

    let av = lexer::new_attribute_ex(doc, b"type", Some(b"text/css"), '"' as u32);
    lexer::insert_attribute_at_start(doc, node, av);

    let body = lexer::find_body(doc);
    let sz = doc.lxr().lexsize();
    doc.lx().txtstart = sz;
    if let Some(body) = body {
        clean_body_attrs(doc, body);
    }

    let styles = doc.lxr().styles.clone();
    for style in &styles {
        lexer::add_char_to_lexer(doc, ' ' as u32);
        lexer::add_string_literal(doc, &style.tag);
        lexer::add_char_to_lexer(doc, '.' as u32);
        lexer::add_string_literal(doc, &style.tag_class);
        lexer::add_char_to_lexer(doc, ' ' as u32);
        lexer::add_char_to_lexer(doc, '{' as u32);
        lexer::add_string_literal(doc, &style.properties);
        lexer::add_char_to_lexer(doc, '}' as u32);
        lexer::add_char_to_lexer(doc, '\n' as u32);
    }

    let sz = doc.lxr().lexsize();
    doc.lx().txtend = sz;

    let text = lexer::text_token(doc);
    parser::insert_node_at_end(doc, node, text);

    if let Some(head) = lexer::find_head(doc) {
        parser::insert_node_at_end(doc, head, node);
    }
}

/// FixNodeLinks(): ensure bidirectional links are consistent
pub fn fix_node_links(doc: &mut Doc, node: NodeId) {
    let parent = doc[node].parent;
    match doc[node].prev {
        Some(p) => doc[p].next = Some(node),
        None => {
            if let Some(par) = parent {
                doc[par].content = Some(node);
            }
        }
    }
    match doc[node].next {
        Some(n) => doc[n].prev = Some(node),
        None => {
            if let Some(par) = parent {
                doc[par].last = Some(node);
            }
        }
    }
    let mut child = doc[node].content;
    while let Some(c) = child {
        doc[c].parent = Some(node);
        child = doc[c].next;
    }
}

/// StripOnlyChild(): strip child of node when the node has one and only
/// one child
fn strip_only_child(doc: &mut Doc, node: NodeId) {
    let Some(child) = doc[node].content else {
        return;
    };
    doc[node].content = doc[child].content;
    doc[node].last = doc[child].last;
    doc[child].content = None;
    lexer::free_node(doc, Some(child));

    let mut c = doc[node].content;
    while let Some(ch) = c {
        doc[ch].parent = Some(node);
        c = doc[ch].next;
    }
}

/// DiscardContainer(): extricate "element", replace it by its content and
/// delete it; returns the node to continue with.
fn discard_container(doc: &mut Doc, element: NodeId) -> Option<NodeId> {
    if let Some(content) = doc[element].content {
        let parent = doc[element].parent;
        let last = doc[element].last;
        let enext = doc[element].next;
        if let Some(l) = last {
            doc[l].next = enext;
        }
        match enext {
            Some(n) => doc[n].prev = last,
            None => {
                if let Some(p) = parent {
                    doc[p].last = last;
                }
            }
        }
        match doc[element].prev {
            Some(prev) => {
                doc[content].prev = Some(prev);
                doc[prev].next = Some(content);
            }
            None => {
                if let Some(p) = parent {
                    doc[p].content = Some(content);
                }
            }
        }
        let mut n = Some(content);
        while let Some(x) = n {
            doc[x].parent = parent;
            n = doc[x].next;
        }
        doc[element].next = None;
        doc[element].content = None;
        lexer::free_node(doc, Some(element));
        Some(content)
    } else {
        parser::discard_element(doc, element)
    }
}

/// MergeProperties()
fn merge_properties(s1: &[u8], s2: &[u8]) -> Vec<u8> {
    let mut props = Vec::new();
    create_props(&mut props, s1);
    create_props(&mut props, s2);
    create_prop_string(&props)
}

/// AddStyleProperty(): add style property to element, creating style
/// attribute as needed and adding ; delimiter
pub fn add_style_property(doc: &mut Doc, node: NodeId, property: &[u8]) {
    match attr(doc, node, AttrId::STYLE) {
        Some(av) => match doc[av].value.clone() {
            Some(v) => {
                let s = merge_properties(&v, property);
                doc[av].value = Some(s);
            }
            None => doc[av].value = Some(tmbstr::cstr(property).to_vec()),
        },
        None => {
            let av = lexer::new_attribute_ex(doc, b"style", Some(property), '"' as u32);
            lexer::insert_attribute_at_start(doc, node, av);
        }
    }
}

/// The first attribute with the given id, by walking the list.
fn first_attr_with(doc: &Doc, node: NodeId, id: AttrId) -> Option<AttId> {
    doc.attr_list(node)
        .into_iter()
        .find(|&a| doc.attr_is(Some(a), id))
}

/// MergeClasses()
fn merge_classes(doc: &mut Doc, node: NodeId, child: NodeId) {
    let s2 = first_attr_with(doc, child, AttrId::CLASS).and_then(|a| doc[a].value.clone());
    let av = first_attr_with(doc, node, AttrId::CLASS);
    let s1 = av.and_then(|a| doc[a].value.clone());
    if let Some(s1) = s1 {
        if let Some(s2) = s2 {
            let mut names = tmbstr::cstr(&s1).to_vec();
            names.push(b' ');
            names.extend_from_slice(tmbstr::cstr(&s2));
            doc[av.unwrap()].value = Some(names);
        }
    } else if let Some(s2) = s2 {
        let a = lexer::new_attribute_ex(doc, b"class", Some(&s2), '"' as u32);
        lexer::insert_attribute_at_start(doc, node, a);
    }
}

/// MergeStyles()
fn merge_styles(doc: &mut Doc, node: NodeId, child: NodeId) {
    merge_classes(doc, node, child);
    let s2 = first_attr_with(doc, child, AttrId::STYLE).and_then(|a| doc[a].value.clone());
    let av = first_attr_with(doc, node, AttrId::STYLE);
    let s1 = av.and_then(|a| doc[a].value.clone());
    if let Some(s1) = s1 {
        if let Some(s2) = s2 {
            let style = merge_properties(&s1, &s2);
            doc[av.unwrap()].value = Some(style);
        }
    } else if let Some(s2) = s2 {
        let a = lexer::new_attribute_ex(doc, b"style", Some(&s2), '"' as u32);
        lexer::insert_attribute_at_start(doc, node, a);
    }
}

/// FontSize2Name()
fn font_size2name(size: &[u8]) -> Option<&'static [u8]> {
    const SIZES: [Option<&[u8]>; 7] = [
        Some(b"60%"),
        Some(b"70%"),
        Some(b"80%"),
        None,
        Some(b"120%"),
        Some(b"150%"),
        Some(b"200%"),
    ];
    const MINUSSIZES: [&[u8]; 7] = [b"100%", b"80%", b"64%", b"51%", b"40%", b"32%", b"26%"];
    const PLUSSIZES: [&[u8]; 7] = [
        b"100%", b"120%", b"144%", b"172%", b"207%", b"248%", b"298%",
    ];
    let at = |i: usize| size.get(i).copied().unwrap_or(0);
    if at(0) == 0 {
        return None;
    }
    if (b'0'..=b'6').contains(&at(0)) {
        return SIZES[(at(0) - b'0') as usize];
    }
    if at(0) == b'-' {
        if (b'0'..=b'6').contains(&at(1)) {
            return Some(MINUSSIZES[(at(1) - b'0') as usize]);
        }
        return Some(b"smaller");
    }
    if (b'0'..=b'6').contains(&at(1)) {
        return Some(PLUSSIZES[(at(1) - b'0') as usize]);
    }
    Some(b"larger")
}

/// AddFontFace()
fn add_font_face(doc: &mut Doc, node: NodeId, face: &[u8]) {
    let mut buf = b"font-family: ".to_vec();
    buf.extend_from_slice(tmbstr::cstr(face));
    add_style_property(doc, node, &trunc(buf, 256));
}

/// AddFontSize()
fn add_font_size(doc: &mut Doc, node: NodeId, size: &[u8]) {
    if is(doc, node, TagId::P) {
        let value: Option<&[u8]> = if tmbstr::tmbstrcmp(size, b"6") == 0 {
            Some(b"h1")
        } else if tmbstr::tmbstrcmp(size, b"5") == 0 {
            Some(b"h2")
        } else if tmbstr::tmbstrcmp(size, b"4") == 0 {
            Some(b"h3")
        } else {
            None
        };
        if let Some(v) = value {
            doc[node].element = Some(v.to_vec());
            tags::find_tag(doc, node);
            return;
        }
    }
    if let Some(value) = font_size2name(size) {
        let mut buf = b"font-size: ".to_vec();
        buf.extend_from_slice(value);
        add_style_property(doc, node, &trunc(buf, 64));
    }
}

/// AddFontColor()
fn add_font_color(doc: &mut Doc, node: NodeId, color: &[u8]) {
    let mut buf = b"color: ".to_vec();
    buf.extend_from_slice(tmbstr::cstr(color));
    add_style_property(doc, node, &trunc(buf, 128));
}

/// AddAlign(): force alignment value to lower case
fn add_align(doc: &mut Doc, node: NodeId, align: &[u8]) {
    let mut buf = b"text-align: ".to_vec();
    for &c in tmbstr::cstr(align) {
        if buf.len() >= 127 {
            break;
        }
        buf.push(c.to_ascii_lowercase());
    }
    add_style_property(doc, node, &buf);
}

/// AddFontStyles(): add style properties corresponding to the font face,
/// size and color attributes
fn add_font_styles(doc: &mut Doc, node: NodeId, av: Option<AttId>) {
    let mut av = av;
    while let Some(a) = av {
        if let Some(v) = doc[a].value.clone() {
            if doc.attr_is(Some(a), AttrId::FACE) {
                add_font_face(doc, node, &v);
            } else if doc.attr_is(Some(a), AttrId::SIZE) {
                add_font_size(doc, node, &v);
            } else if doc.attr_is(Some(a), AttrId::COLOR) {
                add_font_color(doc, node, &v);
            }
        }
        av = doc[a].next;
    }
}

/// TextAlign(): <p align=center> to <p style="text-align: center">
fn text_align(doc: &mut Doc, node: NodeId) {
    let mut prev: Option<AttId> = None;
    let mut av = doc[node].attributes;
    while let Some(a) = av {
        if doc.attr_is(Some(a), AttrId::ALIGN) {
            let next = doc[a].next;
            match prev {
                Some(p) => doc[p].next = next,
                None => doc[node].attributes = next,
            }
            if let Some(v) = doc[a].value.clone() {
                add_align(doc, node, &v);
            }
            lexer::free_attribute(doc, a);
            break;
        }
        prev = Some(a);
        av = doc[a].next;
    }
}

/// TableBgColor(): <table bgcolor="red"> to style="background-color: red"
fn table_bg_color(doc: &mut Doc, node: NodeId) {
    if let Some(a) = attr(doc, node, AttrId::BGCOLOR) {
        let mut buf = b"background-color: ".to_vec();
        let v = doc[a].value.clone();
        buf.extend_from_slice(tmbstr::cstr(s_or_null(v.as_deref())));
        let buf = trunc(buf, 256);
        lexer::remove_attribute(doc, node, a);
        add_style_property(doc, node, &buf);
    }
}

/// Dir2Div(): coerce <dir> <li> to <div> with indent
fn dir2div(doc: &mut Doc, node: NodeId) -> bool {
    if is(doc, node, TagId::DIR) || is(doc, node, TagId::UL) || is(doc, node, TagId::OL) {
        let Some(child) = doc[node].content else {
            return false;
        };
        if doc[child].next.is_some() {
            return false;
        }
        if !is(doc, child, TagId::LI) {
            return false;
        }
        if !doc[child].implicit {
            return false;
        }
        doc[node].tag = tags::lookup_tag_def(TagId::DIV);
        doc[node].element = Some(b"div".to_vec());
        add_style_property(doc, node, b"margin-left: 2em");
        strip_only_child(doc, node);
        return true;
    }
    false
}

/// Center2Div(): replace <center> by <div style="text-align: center">
fn center2div(doc: &mut Doc, node: NodeId) -> bool {
    if is(doc, node, TagId::CENTER) {
        rename_elem(doc, node, TagId::DIV);
        add_style_property(doc, node, b"text-align: center");
        return true;
    }
    false
}

/// CopyAttrs(): copy child attributes to node
fn copy_attrs(doc: &mut Doc, node: NodeId, child: NodeId) -> bool {
    if attr(doc, child, AttrId::ID).is_some() && attr(doc, node, AttrId::ID).is_some() {
        return false;
    }
    let mut av2 = doc[child].attributes;
    while let Some(a2) = av2 {
        if doc.attr_is(Some(a2), AttrId::STYLE) || doc.attr_is(Some(a2), AttrId::CLASS) {
            av2 = doc[a2].next;
            continue;
        }
        let id = doc.attr_id(a2);
        if id != AttrId::UNKNOWN {
            if let Some(a1) = attr(doc, node, id) {
                lexer::remove_attribute(doc, node, a1);
            }
        }
        lexer::detach_attribute(doc, child, a2);
        let a1 = a2;
        av2 = doc[a2].next;
        doc[a1].next = None;
        lexer::insert_attribute_at_end(doc, node, a1);
    }
    true
}

/// MergeNestedElements(): merge <XX><XX>...</XX></XX>
fn merge_nested_elements(doc: &mut Doc, id: TagId, state: u32, node: NodeId) -> bool {
    if state == TIDY_NO_STATE || !is(doc, node, id) {
        return false;
    }
    let Some(child) = doc[node].content else {
        return false;
    };
    if doc[child].next.is_some() || !is(doc, child, id) {
        return false;
    }
    if state == TIDY_AUTO_STATE && !copy_attrs(doc, node, child) {
        return false;
    }
    merge_styles(doc, node, child);
    strip_only_child(doc, node);
    true
}

/// NestedList(): <ul><li><ul>...</ul></li></ul> discards the outer list;
/// returns the node to resume with.
fn nested_list(doc: &mut Doc, node: NodeId) -> Option<Option<NodeId>> {
    if is(doc, node, TagId::UL) || is(doc, node, TagId::OL) {
        let child = doc[node].content?;
        if doc[child].next.is_some() {
            return None;
        }
        let mut list = doc[child].content?;
        if doc[list].tag != doc[node].tag {
            return None;
        }
        if doc[list].next.is_some() {
            return None;
        }
        let pnode = Some(list);

        doc[list].prev = doc[node].prev;
        doc[list].next = doc[node].next;
        doc[list].parent = doc[node].parent;
        fix_node_links(doc, list);

        doc[child].content = None;
        lexer::free_node(doc, Some(child));
        doc[node].content = None;
        doc[node].next = None;
        lexer::free_node(doc, Some(node));

        if let Some(lprev) = doc[list].prev {
            if (is(doc, lprev, TagId::UL) || is(doc, lprev, TagId::OL)) && doc[lprev].last.is_some()
            {
                let node = list;
                list = lprev;
                let child = doc[list].last.unwrap();
                doc[list].next = doc[node].next;
                fix_node_links(doc, list);

                doc[node].parent = Some(child);
                doc[node].next = None;
                doc[node].prev = doc[child].last;
                fix_node_links(doc, node);
                clean_node(doc, node);
            }
        }
        return Some(pnode);
    }
    None
}

/// FindCSSSpanEq()
fn find_css_span_eq(doc: &Doc, node: NodeId, deprecated_only: bool) -> Option<&'static [u8]> {
    const CSS_SPAN_EQ: [(TagId, &[u8], bool); 5] = [
        (TagId::B, b"font-weight: bold", false),
        (TagId::I, b"font-style: italic", false),
        (TagId::S, b"text-decoration: line-through", true),
        (TagId::STRIKE, b"text-decoration: line-through", true),
        (TagId::U, b"text-decoration: underline", true),
    ];
    for (id, css, dep) in CSS_SPAN_EQ {
        if (!deprecated_only || dep) && is(doc, node, id) {
            return Some(css);
        }
    }
    None
}

/// CanApplyBlockStyle()
fn can_apply_block_style(doc: &Doc, node: NodeId) -> bool {
    tags::node_has_cm(doc, Some(node), CM_BLOCK | CM_LIST | CM_DEFLIST | CM_TABLE)
        && !is(doc, node, TagId::DIV)
        && !is(doc, node, TagId::P)
        && !is(doc, node, TagId::TABLE)
        && !is(doc, node, TagId::TR)
        && !is(doc, node, TagId::LI)
}

/// BlockStyle()
fn block_style(doc: &mut Doc, node: NodeId) -> bool {
    if is(doc, node, TagId::TABLE)
        || is(doc, node, TagId::TD)
        || is(doc, node, TagId::TH)
        || is(doc, node, TagId::TR)
    {
        table_bg_color(doc, node);
    }
    if can_apply_block_style(doc, node) {
        if !is(doc, node, TagId::CAPTION) {
            text_align(doc, node);
        }
        let Some(child) = doc[node].content else {
            return false;
        };
        if doc[child].next.is_some() {
            return false;
        }
        if let Some(css) = find_css_span_eq(doc, child, false) {
            merge_styles(doc, node, child);
            add_style_property(doc, node, css);
            strip_only_child(doc, node);
            return true;
        } else if is(doc, child, TagId::FONT) {
            merge_styles(doc, node, child);
            let a = doc[child].attributes;
            add_font_styles(doc, node, a);
            strip_only_child(doc, node);
            return true;
        }
    }
    false
}

/// CanApplyInlineStyle()
fn can_apply_inline_style(doc: &Doc, node: NodeId) -> bool {
    !is(doc, node, TagId::FONT) && tags::node_has_cm(doc, Some(node), CM_INLINE | CM_ROW)
}

/// InlineStyle(): the only child of table cell or an inline element
fn inline_style(doc: &mut Doc, node: NodeId) -> bool {
    if can_apply_inline_style(doc, node) {
        let Some(child) = doc[node].content else {
            return false;
        };
        if doc[child].next.is_some() {
            return false;
        }
        if let Some(css) = find_css_span_eq(doc, child, false) {
            merge_styles(doc, node, child);
            add_style_property(doc, node, css);
            strip_only_child(doc, node);
            return true;
        } else if is(doc, child, TagId::FONT) {
            merge_styles(doc, node, child);
            let a = doc[child].attributes;
            add_font_styles(doc, node, a);
            strip_only_child(doc, node);
            return true;
        }
    }
    false
}

/// InlineElementToCSS()
fn inline_element_to_css(doc: &mut Doc, node: NodeId) -> bool {
    if let Some(parent) = doc[node].parent {
        if doc[parent].content == Some(node)
            && doc[node].next.is_none()
            && (can_apply_block_style(doc, parent) || can_apply_inline_style(doc, parent))
        {
            return false;
        }
    }
    if let Some(css) = find_css_span_eq(doc, node, true) {
        rename_elem(doc, node, TagId::SPAN);
        add_style_property(doc, node, css);
        return true;
    }
    false
}

/// Font2Span(): replace font elements by span elements
fn font2span(doc: &mut Doc, node: NodeId) -> bool {
    if is(doc, node, TagId::FONT) {
        if let Some(parent) = doc[node].parent {
            if doc[parent].content == Some(node)
                && doc[node].next.is_none()
                && can_apply_block_style(doc, parent)
            {
                return false;
            }
        }
        let a = doc[node].attributes;
        add_font_styles(doc, node, a);

        let mut av = doc[node].attributes;
        let mut style = None;
        while let Some(a) = av {
            let next = doc[a].next;
            if doc.attr_is(Some(a), AttrId::STYLE) {
                doc[a].next = None;
                style = Some(a);
            } else {
                lexer::free_attribute(doc, a);
            }
            av = next;
        }
        doc[node].attributes = style;
        rename_elem(doc, node, TagId::SPAN);
        return true;
    }
    false
}

/// CleanNode(): applies all matching rules to a node
fn clean_node(doc: &mut Doc, node: NodeId) -> Option<NodeId> {
    let merge_divs = doc.cfg_auto_bool(OptId::MergeDivs);
    let merge_spans = doc.cfg_auto_bool(OptId::MergeSpans);
    let next = Some(node);
    while tags::node_is_element(doc, Some(node)) {
        if dir2div(doc, node) {
            continue;
        }
        if let Some(n) = nested_list(doc, node) {
            return n;
        }
        if center2div(doc, node) {
            continue;
        }
        if merge_nested_elements(doc, TagId::DIV, merge_divs, node) {
            continue;
        }
        if merge_nested_elements(doc, TagId::SPAN, merge_spans, node) {
            continue;
        }
        if block_style(doc, node) {
            continue;
        }
        if inline_style(doc, node) {
            continue;
        }
        if inline_element_to_css(doc, node) {
            continue;
        }
        if font2span(doc, node) {
            continue;
        }
        break;
    }
    next
}

/// CleanTree()
fn clean_tree(doc: &mut Doc, node: NodeId) -> Option<NodeId> {
    if doc[node].content.is_some() {
        let mut child = doc[node].content;
        while let Some(c) = child {
            match clean_tree(doc, c) {
                Some(c2) => child = doc[c2].next,
                None => break,
            }
        }
    }
    clean_node(doc, node)
}

/// DefineStyleRules()
fn define_style_rules(doc: &mut Doc, node: NodeId) {
    let mut child = doc[node].content;
    while let Some(c) = child {
        define_style_rules(doc, c);
        child = doc[c].next;
    }
    style2rule(doc, node);
}

/// CleanDocument()
pub fn clean_document(doc: &mut Doc) {
    let root = doc.root;
    clean_tree(doc, root);
    if doc.cfg_bool(OptId::MakeClean) {
        define_style_rules(doc, root);
        create_style_element(doc);
    }
}

/// NestedEmphasis(): simplifies <b><b> ... </b> ...</b> etc.
pub fn nested_emphasis(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        if (is(doc, n, TagId::B) || is(doc, n, TagId::I))
            && doc[n].parent.is_some_and(|p| doc[p].tag == doc[n].tag)
        {
            node = discard_container(doc, n);
            continue;
        }
        if let Some(c) = doc[n].content {
            nested_emphasis(doc, Some(c));
        }
        node = next;
    }
}

/// EmFromI(): replace i by em and b by strong
pub fn em_from_i(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        if is(doc, n, TagId::I) {
            rename_elem(doc, n, TagId::EM);
        } else if is(doc, n, TagId::B) {
            rename_elem(doc, n, TagId::STRONG);
        }
        if let Some(c) = doc[n].content {
            em_from_i(doc, Some(c));
        }
        node = doc[n].next;
    }
}

/// HasOneChild()
fn has_one_child(doc: &Doc, node: NodeId) -> bool {
    doc[node].content.is_some_and(|c| doc[c].next.is_none())
}

/// List2BQ(): lists with a single implicit li become implicit blockquotes
pub fn list2bq(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        if let Some(c) = doc[n].content {
            list2bq(doc, Some(c));
        }
        if doc[n].tag.is_some()
            && has_parser(doc, n, parser::parse_list)
            && has_one_child(doc, n)
            && doc[n].content.is_some_and(|c| doc[c].implicit)
        {
            strip_only_child(doc, n);
            rename_elem(doc, n, TagId::BLOCKQUOTE);
            doc[n].implicit = true;
        }
        node = doc[n].next;
    }
}

/// BQ2Div(): replace implicit blockquote by div with an indent
pub fn bq2div(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        if is(doc, n, TagId::BLOCKQUOTE) && doc[n].implicit {
            let mut indent: u32 = 1;
            while has_one_child(doc, n)
                && doc.is_o(doc[n].content, TagId::BLOCKQUOTE)
                && doc[n].implicit
            {
                indent += 1;
                strip_only_child(doc, n);
            }
            if let Some(c) = doc[n].content {
                bq2div(doc, Some(c));
            }
            let buf = trunc(
                format!("margin-left: {}em", 2 * indent as i32).into_bytes(),
                32,
            );
            rename_elem(doc, n, TagId::DIV);
            add_style_property(doc, n, &buf);
        } else if let Some(c) = doc[n].content {
            bq2div(doc, Some(c));
        }
        node = doc[n].next;
    }
}

/// FindEnclosingCell()
fn find_enclosing_cell(doc: &Doc, node: NodeId) -> Option<NodeId> {
    let mut check = Some(node);
    while let Some(c) = check {
        if is(doc, c, TagId::TD) {
            return Some(c);
        }
        check = doc[c].parent;
    }
    None
}

/// `tmbstrncmp(lexer->lexbuf + node->start, s, n)`
fn lexbuf_ncmp(doc: &Doc, node: NodeId, s: &[u8], n: usize) -> i32 {
    let lb = &doc.lxr().lexbuf;
    let start = (doc[node].start as usize).min(lb.len());
    tmbstr::tmbstrncmp(&lb[start..], s, n)
}

/// PruneSection(): node is <![if ...]> prune up to <![endif]>
fn prune_section(doc: &mut Doc, node: Option<NodeId>) -> Option<NodeId> {
    let mut node = node;
    loop {
        let n = node?;
        if lexbuf_ncmp(doc, n, b"if !supportEmptyParas", 21) == 0 {
            if find_enclosing_cell(doc, n).is_some() {
                let nbsp = lexer::new_literal_text_node(doc, b"\xa0");
                parser::insert_node_before_element(doc, n, nbsp);
            }
        }
        node = if doc[n].type_ == NodeType::TextNode {
            doc[n].next
        } else {
            parser::discard_element(doc, n)
        };
        let n = node?;
        if doc[n].type_ == NodeType::SectionTag {
            if lexbuf_ncmp(doc, n, b"if", 2) == 0 {
                node = prune_section(doc, Some(n));
                continue;
            }
            if lexbuf_ncmp(doc, n, b"endif", 5) == 0 {
                node = parser::discard_element(doc, n);
                break;
            }
        }
    }
    node
}

/// DropSections()
pub fn drop_sections(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        if doc[n].type_ == NodeType::SectionTag {
            if lexbuf_ncmp(doc, n, b"if", 2) == 0 && lexbuf_ncmp(doc, n, b"if !vml", 7) != 0 {
                node = prune_section(doc, Some(n));
                continue;
            }
            node = parser::discard_element(doc, n);
            continue;
        }
        if let Some(c) = doc[n].content {
            drop_sections(doc, Some(c));
        }
        node = doc[n].next;
    }
}

/// PurgeWord2000Attributes()
fn purge_word2000_attributes(doc: &mut Doc, node: NodeId) {
    let mut prev: Option<AttId> = None;
    let mut attr_ = doc[node].attributes;
    while let Some(a) = attr_ {
        let next = doc[a].next;
        if doc.attr_is(Some(a), AttrId::CLASS) {
            let keep = doc.attr_value_is(Some(a), b"Code")
                || match doc[a].value.as_deref() {
                    Some(v) => tmbstr::tmbstrncmp(v, b"Mso", 3) != 0,
                    None => true,
                };
            if keep {
                prev = Some(a);
                attr_ = next;
                continue;
            }
        }
        let cell = is(doc, node, TagId::TD) || is(doc, node, TagId::TR) || is(doc, node, TagId::TH);
        if doc.attr_is(Some(a), AttrId::CLASS)
            || doc.attr_is(Some(a), AttrId::STYLE)
            || doc.attr_is(Some(a), AttrId::LANG)
            || ((doc.attr_is(Some(a), AttrId::HEIGHT) || doc.attr_is(Some(a), AttrId::WIDTH))
                && cell)
            || doc[a]
                .attribute
                .as_deref()
                .is_some_and(|n| tmbstr::tmbstrncmp(n, b"x:", 2) == 0)
        {
            match prev {
                Some(p) => doc[p].next = next,
                None => doc[node].attributes = next,
            }
            lexer::free_attribute(doc, a);
        } else {
            prev = Some(a);
        }
        attr_ = next;
    }
}

/// StripSpan(): Word2000 uses span excessively, so we strip span out
fn strip_span(doc: &mut Doc, span: NodeId) -> Option<NodeId> {
    let c = doc[span].content;
    clean_word2000(doc, c);
    let mut content = doc[span].content;
    let mut prev: Option<NodeId> = None;

    if let Some(p) = doc[span].prev {
        prev = Some(p);
    } else if let Some(c) = content {
        let node = c;
        content = doc[c].next;
        parser::remove_node(doc, node);
        parser::insert_node_before_element(doc, span, node);
        prev = Some(node);
    }

    while let Some(c) = content {
        let node = c;
        content = doc[c].next;
        parser::remove_node(doc, node);
        if let Some(p) = prev {
            parser::insert_node_after_element(doc, p, node);
        }
        prev = Some(node);
    }

    if doc[span].next.is_none() {
        if let Some(par) = doc[span].parent {
            doc[par].last = prev;
        }
    }

    let node = doc[span].next;
    doc[span].content = None;
    parser::discard_element(doc, span);
    node
}

/// In-place re-encoding of a text node's UTF-8 text through `map`, like
/// the C loops writing with PutUTF8 while reading with GetUTF8.
fn recode_text(doc: &mut Doc, node: NodeId, map: fn(u32) -> u32) {
    let (start, end) = (doc[node].start as usize, doc[node].end as usize);
    let lb = &mut doc.lx().lexbuf;
    let mut p = start;
    let mut i = start;
    while i < end {
        let mut c = lb.get(i).copied().unwrap_or(0) as u32;
        if c > 0x7F {
            let (n, ch) = utf8::get_utf8(&lb[i.min(lb.len())..]);
            i += n;
            c = ch;
        }
        c = map(c);
        let mut tmp = Vec::with_capacity(4);
        utf8::put_utf8(&mut tmp, c);
        for b in tmp {
            if p < lb.len() {
                lb[p] = b;
            } else {
                lb.push(b);
            }
            p += 1;
        }
        i += 1;
    }
    doc[node].end = p as u32;
}

/// NormalizeSpaces(): map non-breaking spaces to regular spaces
pub fn normalize_spaces(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        if let Some(c) = doc[n].content {
            normalize_spaces(doc, Some(c));
        }
        if tags::node_is_text(doc, Some(n)) {
            recode_text(doc, n, |c| if c == 160 { ' ' as u32 } else { c });
        }
        node = doc[n].next;
    }
}

/// NoMargins(): used to hunt for hidden preformatted sections
fn no_margins(doc: &Doc, node: NodeId) -> bool {
    let attval = attr(doc, node, AttrId::STYLE);
    let Some(v) = attval.and_then(|a| doc[a].value.as_deref()) else {
        return false;
    };
    tmbstr::tmbsubstr(v, b"margin-top: 0").is_some()
        && tmbstr::tmbsubstr(v, b"margin-bottom: 0").is_some()
}

/// SingleSpace(): does element have a single space as its content?
fn single_space(doc: &Doc, node: NodeId) -> bool {
    if let Some(n) = doc[node].content {
        if doc[n].next.is_some() {
            return false;
        }
        if doc[n].type_ != NodeType::TextNode {
            return false;
        }
        let lb = &doc.lxr().lexbuf;
        let (s, e) = (doc[n].start, doc[n].end);
        if e.wrapping_sub(s) == 1 && lb.get(s as usize) == Some(&b' ') {
            return true;
        }
        if e.wrapping_sub(s) == 2 {
            let (_, c) = utf8::get_utf8(&lb[(s as usize).min(lb.len())..]);
            if c == 160 {
                return true;
            }
        }
    }
    false
}

/// CleanWord2000(): strip out all the extra stuff you get when you save as
/// web page from Word 2000
pub fn clean_word2000(doc: &mut Doc, node: Option<NodeId>) {
    let mut list: Option<NodeId> = None;
    let mut node = node;
    while let Some(mut n) = node {
        if is(doc, n, TagId::HTML) {
            if !is_word2000(doc) {
                return;
            }
            let mut attval = doc[n].attributes;
            while let Some(a) = attval {
                let next_attr = doc[a].next;
                let is_xmlns = doc[a].attribute.as_deref().is_some_and(|s| s == b"xmlns");
                if !is_xmlns {
                    message::report_attr_error(doc, n, Some(a), PROPRIETARY_ATTRIBUTE);
                }
                attval = next_attr;
            }
            lexer::free_attrs(doc, n);
        }

        if is(doc, n, TagId::P) && no_margins(doc, n) {
            parser::coerce_node(doc, n, TagId::PRE, false, true);
            purge_word2000_attributes(doc, n);
            if let Some(c) = doc[n].content {
                clean_word2000(doc, Some(c));
            }
            let pre = n;
            let mut cur = doc[n].next;
            while let Some(c) = cur {
                if !(is(doc, c, TagId::P) && no_margins(doc, c)) {
                    break;
                }
                let next = doc[c].next;
                parser::remove_node(doc, c);
                let nl = lexer::new_line_node(doc);
                parser::insert_node_at_end(doc, pre, nl);
                parser::insert_node_at_end(doc, pre, c);
                strip_span(doc, c);
                cur = next;
            }
            match cur {
                None => break,
                Some(c) => n = c,
            }
        }

        if doc[n]
            .tag
            .is_some_and(|t| doc.dict_model(t) & CM_BLOCK != 0)
            && single_space(doc, n)
        {
            node = strip_span(doc, n);
            continue;
        }
        if is(doc, n, TagId::STYLE)
            || is(doc, n, TagId::META)
            || doc[n].type_ == NodeType::CommentTag
        {
            node = parser::discard_element(doc, n);
            continue;
        }
        if is(doc, n, TagId::SPAN) || is(doc, n, TagId::FONT) {
            node = strip_span(doc, n);
            continue;
        }
        if is(doc, n, TagId::LINK) {
            let a = attr(doc, n, AttrId::REL);
            if doc.attr_value_is(a, b"File-List") {
                node = parser::discard_element(doc, n);
                continue;
            }
        }
        if let Some(t) = doc[n].tag {
            if doc
                .dict_name(t)
                .is_some_and(|name| tmbstr::tmbstrcmp(name, b"o:p") == 0)
            {
                message::report(doc, None, Some(n), PROPRIETARY_ELEMENT, &[]);
                node = discard_container(doc, n);
                continue;
            }
        }
        if doc[n].content.is_none() && is(doc, n, TagId::P) {
            node = parser::trim_empty_element(doc, n);
            continue;
        }

        let mut cur = Some(n);
        if is(doc, n, TagId::P) {
            let a = attr(doc, n, AttrId::CLASS);
            let atr_style = attr(doc, n, AttrId::STYLE);
            if doc.attr_value_is(a, b"MsoListBullet")
                || doc.attr_value_is(a, b"MsoListNumber")
                || doc.attr_contains(atr_style, b"mso-list:")
            {
                let mut list_type = TagId::UL;
                if doc.attr_value_is(a, b"MsoListNumber") {
                    list_type = TagId::OL;
                }
                parser::coerce_node(doc, n, TagId::LI, false, true);
                if list.is_none() || doc.tag_id(list) != list_type {
                    let l = lexer::inferred_tag(doc, list_type);
                    parser::insert_node_before_element(doc, n, l);
                    list = Some(l);
                }
                purge_word2000_attributes(doc, n);
                if let Some(c) = doc[n].content {
                    clean_word2000(doc, Some(c));
                }
                parser::remove_node(doc, n);
                parser::insert_node_at_end(doc, list.unwrap(), n);
                cur = list;
            } else if doc.attr_value_is(a, b"Code") {
                let br = lexer::new_line_node(doc);
                let c = doc[n].content;
                normalize_spaces(doc, c);
                if list.is_none() || doc.tag_id(list) != TagId::PRE {
                    let l = lexer::inferred_tag(doc, TagId::PRE);
                    parser::insert_node_before_element(doc, n, l);
                    list = Some(l);
                }
                let l = list.unwrap();
                parser::remove_node(doc, n);
                parser::insert_node_at_end(doc, l, n);
                strip_span(doc, n);
                parser::insert_node_at_end(doc, l, br);
                cur = doc[l].next;
            } else {
                list = None;
            }
        } else {
            list = None;
        }

        let Some(n) = cur else {
            return;
        };
        if tags::node_is_element(doc, Some(n)) {
            purge_word2000_attributes(doc, n);
        }
        if let Some(c) = doc[n].content {
            clean_word2000(doc, Some(c));
        }
        node = doc[n].next;
    }
}

/// IsWord2000()
pub fn is_word2000(doc: &mut Doc) -> bool {
    let html = lexer::find_html(doc);
    if let Some(h) = html {
        if attrs::get_attr_by_name(doc, h, b"xmlns:o").is_some() {
            return true;
        }
    }
    if let Some(head) = lexer::find_head(doc) {
        let mut node = doc[head].content;
        while let Some(n) = node {
            node = doc[n].next;
            if !is(doc, n, TagId::META) {
                continue;
            }
            let a = attr(doc, n, AttrId::NAME);
            if !doc.attr_value_is(a, b"generator") {
                continue;
            }
            let a = attr(doc, n, AttrId::CONTENT);
            if doc.attr_contains(a, b"Microsoft") {
                return true;
            }
        }
    }
    false
}

/// BumpObject(): where appropriate move object elements from head to body
pub fn bump_object(doc: &mut Doc, html: Option<NodeId>) {
    let Some(html) = html else {
        return;
    };
    let mut head = None;
    let mut body = None;
    let mut node = doc[html].content;
    while let Some(n) = node {
        if is(doc, n, TagId::HEAD) {
            head = Some(n);
        }
        if is(doc, n, TagId::BODY) {
            body = Some(n);
        }
        node = doc[n].next;
    }
    if let (Some(head), Some(body)) = (head, body) {
        let mut node = doc[head].content;
        while let Some(n) = node {
            let next = doc[n].next;
            if is(doc, n, TagId::OBJECT) {
                let mut bump = false;
                let mut child = doc[n].content;
                while let Some(c) = child {
                    if (tags::node_is_text(doc, Some(c)) && !parser::is_blank(doc, n))
                        || !is(doc, c, TagId::PARAM)
                    {
                        bump = true;
                        break;
                    }
                    child = doc[c].next;
                }
                if bump {
                    parser::remove_node(doc, n);
                    parser::insert_node_at_start(doc, body, n);
                }
            }
            node = next;
        }
    }
}

/// Report DISCARDING_UNEXPECTED, discard the node and return its previous
/// sibling (the node the meta charset loop continues from).
fn discard_meta(doc: &mut Doc, head: NodeId, current: NodeId) -> Option<NodeId> {
    let prev = doc[current].prev;
    message::report(doc, Some(head), Some(current), DISCARDING_UNEXPECTED, &[]);
    parser::discard_element(doc, current);
    prev
}

/// TidyMetaCharset(): check / add / fix the meta charset
pub fn tidy_meta_charset(doc: &mut Doc) -> bool {
    let mut charset_found = false;
    let outenc = doc.cfg(OptId::OutCharEncoding);
    let enc = streamio::get_encoding_name_from_tidy_id(outenc);
    let head = lexer::find_head(doc);
    let add_meta = doc.cfg_bool(OptId::MetaCharset);

    let (Some(head), Some(enc)) = (head, enc) else {
        return false;
    };
    if enc.is_empty() {
        return false;
    }
    let enc = enc.as_bytes();
    if outenc == RAW {
        return false;
    }
    if outenc == ISO2022 {
        return false;
    }
    if doc.cfg_auto_bool(OptId::BodyOnly) == TIDY_YES_STATE {
        return false;
    }

    let mut charset_string = b"charset=".to_vec();
    charset_string.extend_from_slice(enc);

    let mut current = doc[head].content;
    while let Some(cur) = current {
        if !is(doc, cur, TagId::META) {
            current = doc[cur].next;
            continue;
        }
        let charset_attr = attr(doc, cur, AttrId::CHARSET);
        let http_equiv_attr = attr(doc, cur, AttrId::HTTP_EQUIV);
        if charset_attr.is_none() && http_equiv_attr.is_none() {
            current = doc[cur].next;
            continue;
        }
        // 1. <meta charset="value">
        if let (Some(ca), None) = (charset_attr, http_equiv_attr) {
            if charset_found || doc[ca].value.is_none() {
                let prev = discard_meta(doc, head, cur);
                current = prev.and_then(|p| doc[p].next);
                continue;
            }
            charset_found = true;
            let v = doc[ca].value.clone().unwrap_or_default();
            if tmbstr::tmbstrcasecmp(&v, enc) != 0 {
                message::report_attr_error(doc, cur, Some(ca), ATTRIBUTE_VALUE_REPLACED);
                doc[ca].value = Some(enc.to_vec());
            }
            // Make sure it's the first element (sic: compares to the 2nd).
            let second = doc[head].content.and_then(|c| doc[c].next);
            if Some(cur) != second {
                parser::remove_node(doc, cur);
                parser::insert_node_at_start(doc, head, cur);
            }
            current = doc[cur].next;
            continue;
        }
        // 2. <meta http-equiv="content-type" content="text/html; charset=UTF-8">
        if let (Some(he), None) = (http_equiv_attr, charset_attr) {
            let Some(content_attr) = attr(doc, cur, AttrId::CONTENT) else {
                current = doc[cur].next;
                continue;
            };
            let Some(hv) = doc[he].value.clone() else {
                let prev = discard_meta(doc, head, cur);
                current = prev.and_then(|p| doc[p].next);
                continue;
            };
            if tmbstr::tmbstrcasecmp(&hv, b"content-type") != 0 {
                current = doc[cur].next;
                continue;
            }
            let Some(cv) = doc[content_attr].value.clone() else {
                current = doc[cur].next;
                continue;
            };
            if tmbstr::tmbstrcasecmp(&cv, &charset_string) == 0 {
                if charset_found {
                    let prev = discard_meta(doc, head, cur);
                    current = prev.and_then(|p| doc[p].next);
                    continue;
                }
                charset_found = true;
            } else if charset_found {
                let prev = discard_meta(doc, head, cur);
                current = prev.and_then(|p| doc[p].next);
                continue;
            } else {
                let mut new_value = b"text/html; charset=".to_vec();
                new_value.extend_from_slice(enc);
                if doc.cfg_bool(OptId::ShowMetaChange) {
                    message::report_attr_error(
                        doc,
                        cur,
                        Some(content_attr),
                        ATTRIBUTE_VALUE_REPLACED,
                    );
                }
                doc[content_attr].value = Some(new_value);
                charset_found = true;
            }
            current = doc[cur].next;
            continue;
        }
        // 3. <meta charset="utf-8" http-equiv="Content-Type" content="...">
        let prev = discard_meta(doc, head, cur);
        current = prev.and_then(|p| doc[p].next);
    }

    if add_meta && !charset_found {
        let meta_tag = lexer::inferred_tag(doc, TagId::META);
        match lexer::html_version(doc) {
            HT50 | XH50 => {
                attrs::add_attribute(doc, meta_tag, b"charset", enc);
            }
            _ => {
                let mut buf = b"text/html; ".to_vec();
                buf.extend_from_slice(&charset_string);
                attrs::add_attribute(doc, meta_tag, b"http-equiv", b"Content-Type");
                attrs::add_attribute(doc, meta_tag, b"content", &buf);
            }
        }
        parser::insert_node_at_start(doc, head, meta_tag);
        message::report(doc, Some(meta_tag), Some(head), ADDED_MISSING_CHARSET, &[]);
    }
    true
}

/// DropComments()
pub fn drop_comments(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        if doc[n].type_ == NodeType::CommentTag {
            parser::remove_node(doc, n);
            lexer::free_node(doc, Some(n));
            node = next;
            continue;
        }
        if let Some(c) = doc[n].content {
            drop_comments(doc, Some(c));
        }
        node = next;
    }
}

/// DropFontElements() (the unused `pnode` argument of the C version is
/// dropped)
pub fn drop_font_elements(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        if is(doc, n, TagId::FONT) {
            node = discard_container(doc, n);
            continue;
        }
        if let Some(c) = doc[n].content {
            drop_font_elements(doc, Some(c));
        }
        node = next;
    }
}

/// WbrToSpace()
pub fn wbr_to_space(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        if is(doc, n, TagId::WBR) {
            let text = lexer::new_literal_text_node(doc, b" ");
            parser::insert_node_after_element(doc, n, text);
            parser::remove_node(doc, n);
            lexer::free_node(doc, Some(n));
            node = next;
            continue;
        }
        if let Some(c) = doc[n].content {
            wbr_to_space(doc, Some(c));
        }
        node = next;
    }
}

/// DowngradeTypography(): smart quotes and dashes to ASCII
pub fn downgrade_typography(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        if tags::node_is_text(doc, Some(n)) {
            recode_text(doc, n, |c| match c {
                0x2013 | 0x2014 => '-' as u32,
                0x2018..=0x201A => '\'' as u32,
                0x201C..=0x201E => '"' as u32,
                _ => c,
            });
        }
        if let Some(c) = doc[n].content {
            downgrade_typography(doc, Some(c));
        }
        node = next;
    }
}

/// ReplacePreformattedSpaces()
pub fn replace_preformatted_spaces(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        if doc[n].tag.is_some() && has_parser(doc, n, parser::parse_pre) {
            let c = doc[n].content;
            normalize_spaces(doc, c);
            node = next;
            continue;
        }
        if let Some(c) = doc[n].content {
            replace_preformatted_spaces(doc, Some(c));
        }
        node = next;
    }
}

/// ConvertCDATANodes()
pub fn convert_cdata_nodes(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        if doc[n].type_ == NodeType::CDATATag {
            doc[n].type_ = NodeType::TextNode;
        }
        if let Some(c) = doc[n].content {
            convert_cdata_nodes(doc, Some(c));
        }
        node = next;
    }
}

/// FixLanguageInformation(): keep (only) the language attributes the
/// output document type wants
pub fn fix_language_information(
    doc: &mut Doc,
    node: Option<NodeId>,
    want_xml_lang: bool,
    want_lang: bool,
) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        if tags::node_is_element(doc, Some(n)) {
            let lang = attr(doc, n, AttrId::LANG);
            let xml_lang = attr(doc, n, AttrId::XML_LANG);
            if lang.is_some() && xml_lang.is_some() {
                // nothing
            } else if let (Some(l), true) = (lang, want_xml_lang) {
                let emitted = doc.lxr().version_emitted;
                if attrs::node_attribute_versions(doc, n, AttrId::XML_LANG) & emitted != 0 {
                    let v = doc[l].value.clone();
                    attrs::repair_attr_value(doc, n, b"xml:lang", v.as_deref());
                }
            } else if let (Some(x), true) = (xml_lang, want_lang) {
                let emitted = doc.lxr().version_emitted;
                if attrs::node_attribute_versions(doc, n, AttrId::LANG) & emitted != 0 {
                    let v = doc[x].value.clone();
                    attrs::repair_attr_value(doc, n, b"lang", v.as_deref());
                }
            }
            if let Some(l) = lang {
                if !want_lang {
                    lexer::remove_attribute(doc, n, l);
                }
            }
            if let Some(x) = xml_lang {
                if !want_xml_lang {
                    lexer::remove_attribute(doc, n, x);
                }
            }
        }
        if let Some(c) = doc[n].content {
            fix_language_information(doc, Some(c), want_xml_lang, want_lang);
        }
        node = next;
    }
}

/// FixXhtmlNamespace(): set/fix/remove <html xmlns='...'>
pub fn fix_xhtml_namespace(doc: &mut Doc, want_xmlns: bool) {
    let Some(html) = lexer::find_html(doc) else {
        return;
    };
    let xmlns = attr(doc, html, AttrId::XMLNS);
    if want_xmlns {
        if !doc.attr_value_is(xmlns, XHTML_NAMESPACE) {
            attrs::repair_attr_value(doc, html, b"xmlns", Some(XHTML_NAMESPACE));
        }
    } else if let Some(x) = xmlns {
        lexer::remove_attribute(doc, html, x);
    }
}

/// FixAnchors()
pub fn fix_anchors(doc: &mut Doc, node: Option<NodeId>, want_name: bool, want_id: bool) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        if attrs::is_anchor_element(doc, n) {
            let name = attr(doc, n, AttrId::NAME);
            let id = attr(doc, n, AttrId::ID);
            let had_name = name.is_some();
            let had_id = id.is_some();
            let mut id_emitted = false;
            let mut name_emitted = false;

            if let (Some(nm), Some(i)) = (name, id) {
                let nv = doc[nm].value.clone();
                let iv = doc[i].value.clone();
                let name_has_value = nv.is_some();
                let id_has_value = iv.is_some();
                if name_has_value != id_has_value
                    || (name_has_value
                        && id_has_value
                        && tmbstr::tmbstrcmp(nv.as_deref().unwrap(), iv.as_deref().unwrap()) != 0)
                {
                    message::report_attr_error(doc, n, Some(nm), ID_NAME_MISMATCH);
                }
            } else if let (Some(nm), true) = (name, want_id) {
                let emitted = doc.lxr().version_emitted;
                if attrs::node_attribute_versions(doc, n, AttrId::ID) & emitted != 0 {
                    let nv = doc[nm].value.clone();
                    if nv.as_deref().is_some_and(attrs::is_valid_html_id) {
                        attrs::repair_attr_value(doc, n, b"id", nv.as_deref());
                        id_emitted = true;
                    } else {
                        message::report_attr_error(doc, n, Some(nm), INVALID_XML_ID);
                    }
                }
            } else if let (Some(i), true) = (id, want_name) {
                let emitted = doc.lxr().version_emitted;
                if attrs::node_attribute_versions(doc, n, AttrId::NAME) & emitted != 0 {
                    let iv = doc[i].value.clone();
                    attrs::repair_attr_value(doc, n, b"name", iv.as_deref());
                    name_emitted = true;
                }
            }

            if let Some(i) = id {
                if !want_id && (had_name || !want_name || name_emitted) {
                    if !want_id && !want_name {
                        let v = doc[i].value.clone().unwrap_or_default();
                        attrs::remove_anchor_by_node(doc, &v, n);
                    }
                    lexer::remove_attribute(doc, n, i);
                }
            }
            if let Some(nm) = name {
                if !want_name && (had_id || !want_id || id_emitted) {
                    if !want_id && !want_name {
                        let v = doc[nm].value.clone().unwrap_or_default();
                        attrs::remove_anchor_by_node(doc, &v, n);
                    }
                    lexer::remove_attribute(doc, n, nm);
                }
            }
        }
        if let Some(c) = doc[n].content {
            fix_anchors(doc, Some(c), want_name, want_id);
        }
        node = next;
    }
}

/// StyleToHead(): move style elements from body to head
fn style_to_head(doc: &mut Doc, head: NodeId, node: Option<NodeId>, fix: bool) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        if is(doc, n, TagId::STYLE) {
            if fix {
                parser::remove_node(doc, n);
                parser::insert_node_at_end(doc, head, n);
                message::report(doc, Some(n), Some(head), MOVED_STYLE_TO_HEAD, &[]);
            } else {
                message::report(doc, Some(n), Some(head), FOUND_STYLE_IN_BODY, &[]);
            }
        } else if let Some(c) = doc[n].content {
            style_to_head(doc, head, Some(c), fix);
        }
        node = next;
    }
}

/// CleanStyle(): Issue #567 - move style elements from body to head
pub fn clean_style(doc: &mut Doc, html: Option<NodeId>) {
    let fix = doc.cfg_bool(OptId::StyleTags);
    if html.is_none() {
        return;
    }
    let head = lexer::find_head(doc);
    let body = lexer::find_body(doc);
    if let (Some(head), Some(body)) = (head, body) {
        style_to_head(doc, head, Some(body), fix);
    }
}

/// CleanHead(): Issue #692 - remove multiple title elements
pub fn clean_head(doc: &mut Doc) {
    if doc.cfg_auto_bool(OptId::BodyOnly) == TIDY_YES_STATE {
        return;
    }
    let Some(head) = lexer::find_head(doc) else {
        return;
    };
    let mut titles = 0u32;
    let mut node = doc[head].content;
    while let Some(n) = node {
        let next = doc[n].next;
        if is(doc, n, TagId::TITLE) {
            titles += 1;
            if titles > 1 {
                message::report(doc, Some(head), Some(n), DISCARDING_UNEXPECTED, &[]);
                parser::discard_element(doc, n);
            }
        }
        node = next;
    }
}
