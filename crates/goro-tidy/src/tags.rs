//! tags.c: the tag dictionary (built-in tags, user-declared tags) and the
//! element attribute checks.

use std::cell::Cell;

use crate::attrdict::AttrVersion;
use crate::attrs;
use crate::doc::*;
use crate::enums::code::*;
use crate::enums::{AttrId, N_TIDY_TAGS, OptId, TagId};
use crate::lexer;
use crate::message;
use crate::tagdefs::TAG_DEFS;

/// `Parser`
pub type ParserFn = fn(&mut Doc, NodeId, GetTokenMode);
/// `CheckAttribs`
pub type CheckAttribsFn = fn(&mut Doc, NodeId);

/// UserTagType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UserTagType {
    Null = 0,
    Empty = 1,
    Inline = 2,
    Block = 4,
    Pre = 8,
}

/// A built-in `Dict` (tag_defs[] entry).
pub struct TagDef {
    pub id: TagId,
    pub name: &'static [u8],
    pub versions: u32,
    pub attrvers: &'static [AttrVersion],
    pub model: u32,
    pub parser: Option<ParserFn>,
    pub chkattrs: Option<CheckAttribsFn>,
}

/// A user-declared `Dict`.
#[derive(Clone, Debug)]
pub struct DeclaredDict {
    pub name: Vec<u8>,
    pub versions: u32,
    pub model: u32,
    pub parser: Option<ParserFn>,
    pub chkattrs: Option<CheckAttribsFn>,
}

/// TidyTagImpl
#[derive(Clone, Debug, Default)]
pub struct TagsImpl {
    /// The declared dictionaries (`None` once freed).
    pub declared: Vec<Option<DeclaredDict>>,
    /// `declared_tag_list`, head first.
    pub list: Vec<u32>,
}

thread_local! {
    /// tidy's static tag_defs[] is modified in place by AdjustTags() (HTML4
    /// mode) and ResetTags() (HTML5 mode); this records which state the
    /// process-wide table is in.
    static TAGS_HTML5: Cell<bool> = const { Cell::new(true) };
}

fn tag_index(tid: TagId) -> usize {
    thread_local! {
        static INDEX: [u16; N_TIDY_TAGS] = {
            let mut ix = [0u16; N_TIDY_TAGS];
            for (i, d) in TAG_DEFS.iter().enumerate() {
                ix[d.id as usize] = i as u16;
            }
            ix
        };
    }
    INDEX.with(|ix| ix[tid as usize] as usize)
}

/// The static definition of a built-in tag.
pub fn builtin_def(tid: TagId) -> &'static TagDef {
    &TAG_DEFS[tag_index(tid)]
}

/// `np->model` of a built-in tag, as adjusted by AdjustTags().
pub fn builtin_model(tid: TagId) -> u32 {
    let d = builtin_def(tid);
    if TAGS_HTML5.with(|c| c.get()) {
        return d.model;
    }
    match tid {
        TagId::A => CM_INLINE,
        TagId::OBJECT => d.model | CM_HEAD,
        _ => d.model,
    }
}

/// `np->parser` of a built-in tag, as adjusted by AdjustTags().
pub fn builtin_parser(tid: TagId) -> Option<ParserFn> {
    let d = builtin_def(tid);
    if TAGS_HTML5.with(|c| c.get()) {
        return d.parser;
    }
    match tid {
        TagId::A | TagId::CAPTION => Some(crate::parser::parse_inline as ParserFn),
        TagId::BUTTON => Some(crate::parser::parse_block as ParserFn),
        _ => d.parser,
    }
}

fn same_parser(a: Option<ParserFn>, b: ParserFn) -> bool {
    a.is_some_and(|f| std::ptr::fn_addr_eq(f, b))
}

/// tagsLookup()
fn tags_lookup(doc: &Doc, s: Option<&[u8]>) -> Option<DictRef> {
    let s = s?;
    for d in TAG_DEFS.iter().skip(1) {
        if crate::tmbstr::tmbstrcmp(s, d.name) == 0 {
            return Some(DictRef::Builtin(d.id));
        }
    }
    for &i in &doc.tags.list {
        if let Some(d) = &doc.tags.declared[i as usize] {
            if crate::tmbstr::tmbstrcmp(s, &d.name) == 0 {
                return Some(DictRef::Declared(i));
            }
        }
    }
    None
}

/// declare()
fn declare(
    doc: &mut Doc,
    name: &[u8],
    versions: u32,
    model: u32,
    parser: Option<ParserFn>,
    chkattrs: Option<CheckAttribsFn>,
) {
    let np = match tags_lookup(doc, Some(name)) {
        Some(r) => r,
        None => {
            let i = doc.tags.declared.len() as u32;
            doc.tags.declared.push(Some(DeclaredDict {
                name: name.to_vec(),
                versions: VERS_UNKNOWN,
                model: CM_UNKNOWN,
                parser: None,
                chkattrs: None,
            }));
            doc.tags.list.insert(0, i);
            DictRef::Declared(i)
        }
    };
    if let DictRef::Declared(i) = np {
        if let Some(d) = doc.tags.declared[i as usize].as_mut() {
            d.versions = versions;
            d.model |= model;
            d.parser = parser;
            d.chkattrs = chkattrs;
        }
    }
}

/// DeclareUserTag(): coordinates config update and tags data
pub fn declare_user_tag(doc: &mut Doc, opt: OptId, name: &[u8]) {
    let tag_type = match opt {
        OptId::InlineTags => UserTagType::Inline,
        OptId::BlockTags => UserTagType::Block,
        OptId::EmptyTags => UserTagType::Empty,
        OptId::PreTags => UserTagType::Pre,
        OptId::CustomTags => match doc.cfg(OptId::UseCustomTags) {
            TIDY_CUSTOM_BLOCKLEVEL => UserTagType::Block,
            TIDY_CUSTOM_EMPTY => UserTagType::Empty,
            TIDY_CUSTOM_INLINE => UserTagType::Inline,
            TIDY_CUSTOM_PRE => UserTagType::Pre,
            _ => {
                let n = crate::config::get_option(opt).name;
                message::report_unknown_option(doc, n.as_bytes());
                return;
            }
        },
        _ => {
            let n = crate::config::get_option(opt).name;
            message::report_unknown_option(doc, n.as_bytes());
            return;
        }
    };
    define_tag(doc, tag_type, name);
}

/// FindTag(): public interface for finding tag by name
pub fn find_tag(doc: &mut Doc, node: NodeId) -> bool {
    if doc.cfg_bool(OptId::XmlTags) {
        doc[node].tag = Some(DictRef::Xml);
        return true;
    }
    let el = doc[node].element.clone();
    if el.is_some() {
        if let Some(np) = tags_lookup(doc, el.as_deref()) {
            doc[node].tag = Some(np);
            return true;
        }
    }
    if node_is_autonomous_custom_tag(doc, node) {
        let el = el.unwrap_or_default();
        declare_user_tag(doc, OptId::CustomTags, &el);
        doc[node].tag = tags_lookup(doc, Some(&el));
        message::report(doc, Some(node), Some(node), CUSTOM_TAG_DETECTED, &[]);
        return true;
    }
    false
}

/// LookupTagDef()
pub fn lookup_tag_def(tid: TagId) -> Option<DictRef> {
    if tid == TagId::UNKNOWN {
        None
    } else {
        Some(DictRef::Builtin(tid))
    }
}

/// FindParser()
pub fn find_parser(doc: &Doc, node: NodeId) -> Option<ParserFn> {
    let np = tags_lookup(doc, doc[node].element.as_deref())?;
    doc.dict_parser(np)
}

/// DefineTag()
pub fn define_tag(doc: &mut Doc, tag_type: UserTagType, name: &[u8]) {
    let mut parser: Option<ParserFn> = None;
    let mut cm = CM_UNKNOWN;
    let vers = VERS_PROPRIETARY;
    match tag_type {
        UserTagType::Empty => {
            cm = CM_EMPTY | CM_NO_INDENT | CM_NEW;
            parser = Some(crate::parser::parse_block);
        }
        UserTagType::Inline => {
            cm = CM_INLINE | CM_NO_INDENT | CM_NEW;
            parser = Some(crate::parser::parse_inline);
        }
        UserTagType::Block => {
            cm = CM_BLOCK | CM_NO_INDENT | CM_NEW;
            parser = Some(crate::parser::parse_block);
        }
        UserTagType::Pre => {
            cm = CM_BLOCK | CM_NO_INDENT | CM_NEW;
            parser = Some(crate::parser::parse_pre);
        }
        UserTagType::Null => {}
    }
    if cm != 0 && parser.is_some() {
        declare(doc, name, vers, cm, parser, None);
    }
}

fn declared_matches(d: &DeclaredDict, tag_type: UserTagType) -> bool {
    match tag_type {
        UserTagType::Empty => (d.model & CM_EMPTY) != 0,
        UserTagType::Inline => (d.model & CM_INLINE) != 0,
        UserTagType::Block => {
            (d.model & CM_BLOCK) != 0 && same_parser(d.parser, crate::parser::parse_block)
        }
        UserTagType::Pre => {
            (d.model & CM_BLOCK) != 0 && same_parser(d.parser, crate::parser::parse_pre)
        }
        UserTagType::Null => false,
    }
}

/// GetDeclaredTagList() + GetNextDeclaredTag(): the names of the declared
/// tags of a type, in list order.
pub fn get_declared_tags(doc: &Doc, tag_type: UserTagType) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    for &i in &doc.tags.list {
        if let Some(d) = &doc.tags.declared[i as usize] {
            if declared_matches(d, tag_type) {
                v.push(d.name.clone());
            }
        }
    }
    v
}

/// InitTags()
pub fn init_tags(doc: &mut Doc) {
    doc.tags = TagsImpl::default();
}

/// FreeDeclaredTags(): by default, zap all of them
pub fn free_declared_tags(doc: &mut Doc, tag_type: UserTagType) {
    let list = std::mem::take(&mut doc.tags.list);
    let mut keep = Vec::new();
    for i in list {
        let delete_it = match &doc.tags.declared[i as usize] {
            Some(d) => tag_type == UserTagType::Null || declared_matches(d, tag_type),
            None => true,
        };
        if delete_it {
            doc.tags.declared[i as usize] = None;
        } else {
            keep.push(i);
        }
    }
    doc.tags.list = keep;
}

/// AdjustTags(): switch the (process-wide) tag table to HTML4 mode
pub fn adjust_tags(doc: &mut Doc) {
    TAGS_HTML5.with(|c| c.set(false));
    doc.html5_mode = false;
}

/// IsHTML5Mode()
pub fn is_html5_mode(doc: &Doc) -> bool {
    doc.html5_mode
}

/// ResetTags(): reset the table to the default HTML5 mode
pub fn reset_tags(doc: &mut Doc) {
    TAGS_HTML5.with(|c| c.set(true));
    doc.html5_mode = true;
}

/// FreeTags()
pub fn free_tags(doc: &mut Doc) {
    free_declared_tags(doc, UserTagType::Null);
    doc.tags = TagsImpl::default();
}

/// CheckAttributes(): default method for checking an element's attributes
pub fn check_attributes(doc: &mut Doc, node: NodeId) {
    let mut attval = doc[node].attributes;
    while let Some(a) = attval {
        let next = doc[a].next;
        attrs::check_attribute(doc, node, a);
        attval = next;
    }
}

fn has(doc: &Doc, node: NodeId, id: AttrId) -> bool {
    attrs::attr_get_by_id(doc, node, id).is_some()
}

/// CheckIMG()
pub fn check_img(doc: &mut Doc, node: NodeId) {
    let has_alt = has(doc, node, AttrId::ALT);
    let has_src = has(doc, node, AttrId::SRC);
    let has_use_map = has(doc, node, AttrId::USEMAP);
    let has_is_map = has(doc, node, AttrId::ISMAP);
    let has_data_fld = has(doc, node, AttrId::DATAFLD);

    check_attributes(doc, node);

    if !has_alt {
        let alttext = doc.cfg_str(OptId::AltText).map(|s| s.to_vec());
        if doc.cfg(OptId::AccessibilityCheckLevel) == 0 && alttext.is_none() {
            doc.bad_access |= BA_MISSING_IMAGE_ALT;
            message::report_missing_attr(doc, node, b"alt");
        }
        if let Some(alt) = alttext {
            let attval = attrs::add_attribute(doc, node, b"alt", &alt);
            message::report_attr_error(doc, node, Some(attval), INSERTING_AUTO_ATTRIBUTE);
        }
    }
    if !has_src && !has_data_fld {
        message::report_missing_attr(doc, node, b"src");
    }
    if doc.cfg(OptId::AccessibilityCheckLevel) == 0 && has_is_map && !has_use_map {
        message::report_attr_error(doc, node, None, MISSING_IMAGEMAP);
        doc.bad_access |= BA_MISSING_IMAGE_MAP;
    }
}

/// CheckCaption()
pub fn check_caption(doc: &mut Doc, node: NodeId) {
    check_attributes(doc, node);
    let attval = attrs::attr_get_by_id(doc, node, AttrId::ALIGN);
    if !doc.attr_has_value(attval) {
        return;
    }
    if doc.attr_value_is(attval, b"left") || doc.attr_value_is(attval, b"right") {
        lexer::constrain_version(doc, VERS_HTML40_LOOSE);
    } else if doc.attr_value_is(attval, b"top") || doc.attr_value_is(attval, b"bottom") {
        lexer::constrain_version(doc, !(VERS_HTML20 | VERS_HTML32));
    } else {
        message::report_attr_error(doc, node, attval, BAD_ATTRIBUTE_VALUE);
    }
}

/// CheckHTML()
pub fn check_html(doc: &mut Doc, node: NodeId) {
    check_attributes(doc, node);
}

/// CheckAREA()
pub fn check_area(doc: &mut Doc, node: NodeId) {
    let has_alt = has(doc, node, AttrId::ALT);
    let has_href = has(doc, node, AttrId::HREF);
    let has_nohref = has(doc, node, AttrId::NOHREF);

    check_attributes(doc, node);

    if !has_alt && doc.cfg(OptId::AccessibilityCheckLevel) == 0 {
        doc.bad_access |= BA_MISSING_LINK_ALT;
        message::report_missing_attr(doc, node, b"alt");
    }
    if !has_href && !has_nohref {
        message::report_missing_attr(doc, node, b"href");
    }
}

/// CheckTABLE()
pub fn check_table(doc: &mut Doc, node: NodeId) {
    let has_summary = has(doc, node, AttrId::SUMMARY);
    let vers = lexer::html_version(doc) as u32;
    let is_html5 = vers == HT50 || vers == XH50;

    check_attributes(doc, node);

    if doc.cfg(OptId::AccessibilityCheckLevel) == 0 {
        if has_summary && is_html5 {
            message::report(doc, Some(node), Some(node), BAD_SUMMARY_HTML5, &[]);
        } else if !has_summary && !is_html5 {
            doc.bad_access |= BA_MISSING_SUMMARY;
            message::report_missing_attr(doc, node, b"summary");
        }
    }

    if doc.cfg_bool(OptId::XmlOut) {
        if let Some(attval) = attrs::attr_get_by_id(doc, node, AttrId::BORDER) {
            if doc[attval].value.is_none() {
                doc[attval].value = Some(b"1".to_vec());
            }
        }
    }
}

/// CheckLINK(): report missing href attribute; report missing rel attribute
pub fn check_link(doc: &mut Doc, node: NodeId) {
    let has_href = has(doc, node, AttrId::HREF);
    let has_rel = has(doc, node, AttrId::REL);
    let has_itemprop = has(doc, node, AttrId::ITEMPROP);
    if !has_href {
        message::report_missing_attr(doc, node, b"href");
    }
    if !has_itemprop && !has_rel {
        message::report_missing_attr(doc, node, b"rel");
    }
}

/// nodeIsText()
pub fn node_is_text(doc: &Doc, node: Option<NodeId>) -> bool {
    node.is_some_and(|n| doc[n].type_ == NodeType::TextNode)
}

/// nodeHasText()
pub fn node_has_text(doc: &Doc, node: Option<NodeId>) -> bool {
    if let Some(n) = node {
        let lb = &doc.lxr().lexbuf;
        for ix in doc[n].start..doc[n].end {
            let c = lb.get(ix as usize).copied().unwrap_or(0);
            if !lexer::is_white(c as u32) {
                return true;
            }
        }
    }
    false
}

/// nodeIsElement()
pub fn node_is_element(doc: &Doc, node: Option<NodeId>) -> bool {
    node.is_some_and(|n| {
        let t = doc[n].type_;
        t == NodeType::StartTag || t == NodeType::StartEndTag
    })
}

/// elementIsAutonomousCustomFormat(): must contain a hyphen not in first
/// character
pub fn element_is_autonomous_custom_format(element: Option<&[u8]>) -> bool {
    match element {
        Some(e) => {
            let e = crate::tmbstr::cstr(e);
            matches!(e.iter().position(|&c| c == b'-'), Some(p) if p > 0)
        }
        None => false,
    }
}

/// nodeIsAutonomousCustomFormat()
pub fn node_is_autonomous_custom_format(doc: &Doc, node: NodeId) -> bool {
    element_is_autonomous_custom_format(doc[node].element.as_deref())
}

/// nodeIsAutonomousCustomTag()
pub fn node_is_autonomous_custom_tag(doc: &Doc, node: NodeId) -> bool {
    node_is_autonomous_custom_format(doc, node) && doc.cfg(OptId::UseCustomTags) != TIDY_CUSTOM_NO
}

/// nodeHasCM(): true if any of the bits requested are set
pub fn node_has_cm(doc: &Doc, node: Option<NodeId>, content_model: u32) -> bool {
    match node {
        Some(n) => match doc[n].tag {
            Some(t) => (doc.dict_model(t) & content_model) != 0,
            None => false,
        },
        None => false,
    }
}

pub fn node_cm_is_block(doc: &Doc, node: Option<NodeId>) -> bool {
    node_has_cm(doc, node, CM_BLOCK)
}

pub fn node_cm_is_inline(doc: &Doc, node: Option<NodeId>) -> bool {
    node_has_cm(doc, node, CM_INLINE)
}

pub fn node_cm_is_empty(doc: &Doc, node: Option<NodeId>) -> bool {
    node_has_cm(doc, node, CM_EMPTY)
}

/// nodeIsHeader()
pub fn node_is_header(doc: &Doc, node: Option<NodeId>) -> bool {
    matches!(
        doc.tag_id(node),
        TagId::H1 | TagId::H2 | TagId::H3 | TagId::H4 | TagId::H5 | TagId::H6
    )
}

/// nodeHeaderLevel()
pub fn node_header_level(doc: &Doc, node: Option<NodeId>) -> u32 {
    match doc.tag_id(node) {
        TagId::H1 => 1,
        TagId::H2 => 2,
        TagId::H3 => 3,
        TagId::H4 => 4,
        TagId::H5 => 5,
        TagId::H6 => 6,
        _ => 0,
    }
}

/// TraverseNodeTree(): generic node tree traversal
pub fn traverse_node_tree(
    doc: &mut Doc,
    node: Option<NodeId>,
    cb: &mut dyn FnMut(&mut Doc, NodeId) -> NodeTraversalSignal,
) -> NodeTraversalSignal {
    let mut node = node;
    while let Some(n) = node {
        let mut s = cb(doc, n);
        if let Some(c) = doc[n].content {
            if s == NodeTraversalSignal::ContinueTraversal || s == NodeTraversalSignal::SkipSiblings
            {
                s = traverse_node_tree(doc, Some(c), cb);
            }
        }
        match s {
            NodeTraversalSignal::ExitTraversal => return NodeTraversalSignal::ExitTraversal,
            NodeTraversalSignal::VisitParent => {
                node = doc[n].parent;
                continue;
            }
            NodeTraversalSignal::SkipSiblings | NodeTraversalSignal::SkipChildrenAndSiblings => {
                return NodeTraversalSignal::ContinueTraversal;
            }
            _ => node = doc[n].next,
        }
    }
    NodeTraversalSignal::ContinueTraversal
}
