//! parser.c: the HTML/XML tree builder and its repair rules.

use crate::attrs;
use crate::clean;
use crate::config;
use crate::doc::*;
use crate::enums::code::*;
use crate::enums::{AttrId, OptId, TagId};
use crate::istack;
use crate::lexer;
use crate::message;
use crate::pprint;
use crate::tags::{self, ParserFn};
use crate::tmbstr;

use GetTokenMode::*;
use NodeType::*;

// ---------------------------------------------------------------------------
// helpers

/// `showingBodyOnly(doc)`
fn showing_body_only(doc: &Doc) -> bool {
    doc.cfg_auto_bool(OptId::BodyOnly) == TIDY_YES_STATE
}

/// `mode & Preformatted` (the C code tests the enum value as a bit mask)
fn pre_bit(mode: GetTokenMode) -> bool {
    (mode as u32) & (Preformatted as u32) != 0
}

/// `lexer->lexbuf[i]` (0 past the end, like the NUL terminator)
fn lb(doc: &Doc, i: u32) -> u8 {
    doc.lxr().lexbuf.get(i as usize).copied().unwrap_or(0)
}

/// `lexer->lexbuf[i] = c`
fn lb_set(doc: &mut Doc, i: u32, c: u8) {
    let b = &mut doc.lx().lexbuf;
    let i = i as usize;
    if i < b.len() {
        b[i] = c;
    } else {
        b.resize(i, 0);
        b.push(c);
    }
}

fn rep(doc: &mut Doc, element: Option<NodeId>, node: Option<NodeId>, code: u32) {
    message::report(doc, element, node, code, &[]);
}

fn free(doc: &mut Doc, node: NodeId) {
    lexer::free_node(doc, Some(node));
}

fn is_text(doc: &Doc, node: Option<NodeId>) -> bool {
    tags::node_is_text(doc, node)
}

fn is_elem(doc: &Doc, node: Option<NodeId>) -> bool {
    tags::node_is_element(doc, node)
}

fn has_cm(doc: &Doc, node: Option<NodeId>, cm: u32) -> bool {
    tags::node_has_cm(doc, node, cm)
}

fn same_parser(p: Option<ParserFn>, f: ParserFn) -> bool {
    p.is_some_and(|p| std::ptr::fn_addr_eq(p, f))
}

fn istacksize(doc: &Doc) -> usize {
    doc.lxr().istack.len()
}

fn pop_to_base(doc: &mut Doc) {
    while doc.lxr().istack.len() > doc.lxr().istackbase {
        istack::pop_inline(doc, None);
    }
}

fn get_token(doc: &mut Doc, mode: GetTokenMode) -> Option<NodeId> {
    lexer::get_token(doc, mode)
}

fn unget(doc: &mut Doc) {
    lexer::unget_token(doc);
}

// ---------------------------------------------------------------------------

/// CheckNodeIntegrity()
pub fn check_node_integrity(doc: &mut Doc, node: NodeId) -> bool {
    check_integrity(doc, node)
}

fn check_integrity(doc: &Doc, node: NodeId) -> bool {
    let n = &doc[node];
    if let Some(p) = n.prev {
        if doc[p].next != Some(node) {
            return false;
        }
    }
    if let Some(nx) = n.next {
        if nx == node || doc[nx].prev != Some(node) {
            return false;
        }
    }
    if let Some(par) = n.parent {
        if n.prev.is_none() && doc[par].content != Some(node) {
            return false;
        }
        if n.next.is_none() && doc[par].last != Some(node) {
            return false;
        }
    }
    let mut child = n.content;
    while let Some(c) = child {
        if doc[c].parent != Some(node) || !check_integrity(doc, c) {
            return false;
        }
        child = doc[c].next;
    }
    true
}

/// IsNewNode(): used to determine how attributes without values should be
/// printed (user defined tags)
pub fn is_new_node(doc: &mut Doc, node: NodeId) -> bool {
    match doc[node].tag {
        Some(t) => doc.dict_model(t) & CM_NEW != 0,
        None => true,
    }
}

/// CoerceNode()
pub fn coerce_node(doc: &mut Doc, node: NodeId, tid: TagId, obsolete: bool, unexpected: bool) {
    let tag = tags::lookup_tag_def(tid).expect("tidy: CoerceNode of an unknown tag");
    let tmp = lexer::inferred_tag(doc, doc.dict_id(tag));
    if obsolete {
        rep(doc, Some(node), Some(tmp), OBSOLETE_ELEMENT);
    } else if unexpected {
        rep(doc, Some(node), Some(tmp), REPLACING_UNEX_ELEMENT);
    } else {
        rep(doc, Some(node), Some(tmp), REPLACING_ELEMENT);
    }
    let name = doc.dict_name(tag).map(|s| s.to_vec());
    let n = &mut doc[node];
    n.was = n.tag;
    n.tag = Some(tag);
    n.type_ = StartTag;
    n.implicit = true;
    n.element = name;
}

/// RemoveNode(): extract a node and its children from a markup tree
pub fn remove_node(doc: &mut Doc, node: NodeId) -> NodeId {
    let (prev, next, parent) = (doc[node].prev, doc[node].next, doc[node].parent);
    if let Some(p) = prev {
        doc[p].next = next;
    }
    if let Some(n) = next {
        doc[n].prev = prev;
    }
    if let Some(par) = parent {
        if doc[par].content == Some(node) {
            doc[par].content = next;
        }
        if doc[par].last == Some(node) {
            doc[par].last = prev;
        }
    }
    let n = &mut doc[node];
    n.parent = None;
    n.prev = None;
    n.next = None;
    node
}

/// DiscardElement(): remove node from markup tree and discard it; returns
/// the next node
pub fn discard_element(doc: &mut Doc, element: NodeId) -> Option<NodeId> {
    let next = doc[element].next;
    remove_node(doc, element);
    free(doc, element);
    next
}

/// InsertNodeAtStart(): insert node into markup tree as the first element
/// of content of element
pub fn insert_node_at_start(doc: &mut Doc, element: NodeId, node: NodeId) {
    doc[node].parent = Some(element);
    match doc[element].content {
        None => doc[element].last = Some(node),
        Some(c) => doc[c].prev = Some(node),
    }
    doc[node].next = doc[element].content;
    doc[node].prev = None;
    doc[element].content = Some(node);
}

/// InsertNodeAtEnd()
pub fn insert_node_at_end(doc: &mut Doc, element: NodeId, node: NodeId) {
    doc[node].parent = Some(element);
    doc[node].prev = doc[element].last;
    match doc[element].last {
        Some(l) => doc[l].next = Some(node),
        None => doc[element].content = Some(node),
    }
    doc[element].last = Some(node);
}

/// InsertNodeAsParent(): insert "node" into markup tree in place of
/// "element" which is moved to become the child of the node
fn insert_node_as_parent(doc: &mut Doc, element: NodeId, node: NodeId) {
    doc[node].content = Some(element);
    doc[node].last = Some(element);
    doc[node].parent = doc[element].parent;
    doc[element].parent = Some(node);
    let par = doc[node]
        .parent
        .expect("tidy: InsertNodeAsParent without parent");
    if doc[par].content == Some(element) {
        doc[par].content = Some(node);
    }
    if doc[par].last == Some(element) {
        doc[par].last = Some(node);
    }
    doc[node].prev = doc[element].prev;
    doc[element].prev = None;
    if let Some(p) = doc[node].prev {
        doc[p].next = Some(node);
    }
    doc[node].next = doc[element].next;
    doc[element].next = None;
    if let Some(n) = doc[node].next {
        doc[n].prev = Some(node);
    }
}

/// InsertNodeBeforeElement()
pub fn insert_node_before_element(doc: &mut Doc, element: NodeId, node: NodeId) {
    let parent = doc[element].parent;
    doc[node].parent = parent;
    doc[node].next = Some(element);
    doc[node].prev = doc[element].prev;
    doc[element].prev = Some(node);
    if let Some(p) = doc[node].prev {
        doc[p].next = Some(node);
    }
    let parent = parent.expect("tidy: InsertNodeBeforeElement without parent");
    if doc[parent].content == Some(element) {
        doc[parent].content = Some(node);
    }
}

/// InsertNodeAfterElement()
pub fn insert_node_after_element(doc: &mut Doc, element: NodeId, node: NodeId) {
    let parent = doc[element].parent;
    doc[node].parent = parent;
    if parent.is_some_and(|p| doc[p].last == Some(element)) {
        doc[parent.unwrap()].last = Some(node);
    } else {
        doc[node].next = doc[element].next;
        if let Some(n) = doc[node].next {
            doc[n].prev = Some(node);
        }
    }
    doc[element].next = Some(node);
    doc[node].prev = Some(element);
}

fn can_prune(doc: &Doc, element: NodeId) -> bool {
    if !doc.cfg_bool(OptId::DropEmptyElems) {
        return false;
    }
    if is_text(doc, Some(element)) {
        return true;
    }
    let e = &doc[element];
    if e.content.is_some() {
        return false;
    }
    let Some(tag) = e.tag else {
        return false;
    };
    let model = doc.dict_model(tag);
    let has_attrs = e.attributes.is_some();
    if model & CM_BLOCK != 0 && has_attrs {
        return false;
    }
    if doc.is(element, TagId::A) && has_attrs {
        return false;
    }
    if doc.is(element, TagId::P) && !doc.cfg_bool(OptId::DropEmptyParas) {
        return false;
    }
    if model & CM_ROW != 0 {
        return false;
    }
    if model & CM_EMPTY != 0 {
        return false;
    }
    if doc.is(element, TagId::APPLET) {
        return false;
    }
    if doc.is(element, TagId::OBJECT) {
        return false;
    }
    if doc.is(element, TagId::SCRIPT) && attrs::attr_get_by_id(doc, element, AttrId::SRC).is_some()
    {
        return false;
    }
    if doc.is(element, TagId::TITLE) {
        return false;
    }
    if doc.is(element, TagId::IFRAME) {
        return false;
    }
    if doc.is(element, TagId::TEXTAREA) {
        return false;
    }
    if doc.is(element, TagId::CANVAS) {
        return false;
    }
    if doc.is(element, TagId::PROGRESS) {
        return false;
    }
    if attrs::attr_get_by_id(doc, element, AttrId::ID).is_some()
        || attrs::attr_get_by_id(doc, element, AttrId::NAME).is_some()
    {
        return false;
    }
    if attrs::attr_get_by_id(doc, element, AttrId::DATAFLD).is_some() {
        return false;
    }
    if doc.dict_id(tag) == TagId::UNKNOWN {
        return false;
    }
    if doc.is(element, TagId::BODY) {
        return false;
    }
    if doc.is(element, TagId::COLGROUP) {
        return false;
    }
    if doc.is(element, TagId::OPTION) && has_attrs {
        return false;
    }
    if doc.is(element, TagId::DD) {
        return false;
    }
    true
}

/// TrimEmptyElement(): returns the next node
pub fn trim_empty_element(doc: &mut Doc, element: NodeId) -> Option<NodeId> {
    if can_prune(doc, element) {
        if doc[element].type_ != TextNode {
            doc.footnotes |= FN_TRIM_EMPTY_ELEMENT;
            rep(doc, Some(element), None, TRIM_EMPTY_ELEMENT);
        }
        return discard_element(doc, element);
    }
    doc[element].next
}

/// DropEmptyElements()
pub fn drop_empty_elements(doc: &mut Doc, node: Option<NodeId>) -> Option<NodeId> {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        if let Some(c) = doc[n].content {
            drop_empty_elements(doc, Some(c));
        }
        if !is_elem(doc, Some(n)) && !(is_text(doc, Some(n)) && doc[n].start >= doc[n].end) {
            node = next;
            continue;
        }
        node = trim_empty_element(doc, n);
    }
    node
}

/// BadForm(): errors in positioning of form start or end tags
fn bad_form(doc: &mut Doc) {
    doc.bad_form |= FLG_BAD_FORM;
}

/// TrimTrailingSpace()
fn trim_trailing_space(doc: &mut Doc, element: NodeId, last: Option<NodeId>) {
    let Some(last) = last else { return };
    if is_text(doc, Some(last)) && doc[last].end > doc[last].start {
        let c = lb(doc, doc[last].end - 1);
        if c == b' ' {
            doc[last].end -= 1;
            let model = doc.model(element);
            if model & CM_INLINE != 0 && model & CM_FIELD == 0 {
                doc.lx().insertspace = true;
            }
        }
    }
}

/// IsBlank(): only true for text nodes
pub fn is_blank(doc: &mut Doc, node: NodeId) -> bool {
    is_blank_ro(doc, Some(node))
}

fn is_blank_ro(doc: &Doc, node: Option<NodeId>) -> bool {
    match node {
        Some(n) if is_text(doc, node) => {
            let (s, e) = (doc[n].start, doc[n].end);
            e == s || (e == s + 1 && lb(doc, s) == b' ')
        }
        _ => false,
    }
}

/// TrimInitialSpace()
fn trim_initial_space(doc: &mut Doc, element: NodeId, text: Option<NodeId>) {
    let Some(text) = text else { return };
    if is_text(doc, Some(text))
        && lb(doc, doc[text].start) == b' '
        && doc[text].start < doc[text].end
    {
        let model = doc.model(element);
        if model & CM_INLINE != 0 && model & CM_FIELD == 0 {
            let prev = doc[element].prev;
            if is_text(doc, prev) {
                let prev = prev.unwrap();
                let pe = doc[prev].end;
                if pe == 0 || lb(doc, pe - 1) != b' ' {
                    lb_set(doc, pe, b' ');
                    doc[prev].end += 1;
                }
                doc[element].start += 1;
            } else {
                let node = lexer::new_node(doc);
                let es = doc[element].start;
                doc[node].start = es;
                doc[element].start += 1;
                doc[node].end = doc[element].start;
                lb_set(doc, es, b' ');
                insert_node_before_element(doc, element, node);
            }
        }
        doc[text].start += 1;
    }
}

fn is_pre_descendant(doc: &Doc, node: NodeId) -> bool {
    let mut parent = doc[node].parent;
    while let Some(p) = parent {
        if same_parser(doc.parser_of(p), parse_pre) {
            return true;
        }
        parent = doc[p].parent;
    }
    false
}

fn clean_trailing_whitespace(doc: &Doc, node: NodeId) -> bool {
    if !is_text(doc, Some(node)) {
        return false;
    }
    let Some(parent) = doc[node].parent else {
        return false;
    };
    if doc[parent].type_ == DocTypeTag {
        return false;
    }
    if is_pre_descendant(doc, node) {
        return false;
    }
    if same_parser(doc.parser_of(parent), parse_script) {
        return false;
    }
    let next = doc[node].next;
    if next.is_none() && !has_cm(doc, Some(parent), CM_INLINE) {
        return true;
    }
    if next.is_none() && doc[parent].next.is_some() && !has_cm(doc, doc[parent].next, CM_INLINE) {
        return true;
    }
    let Some(next) = next else {
        return false;
    };
    if doc.is(next, TagId::BR) {
        return true;
    }
    if has_cm(doc, Some(next), CM_INLINE) {
        return false;
    }
    if doc[next].type_ == StartTag {
        return true;
    }
    if doc[next].type_ == StartEndTag {
        return true;
    }
    if is_text(doc, Some(next))
        && doc[next].start < doc[next].end
        && lexer::is_white(lb(doc, doc[next].start) as u32)
    {
        return true;
    }
    false
}

fn clean_leading_whitespace(doc: &Doc, node: NodeId) -> bool {
    if !is_text(doc, Some(node)) {
        return false;
    }
    let Some(parent) = doc[node].parent else {
        return false;
    };
    if doc[parent].type_ == DocTypeTag {
        return false;
    }
    if is_pre_descendant(doc, node) {
        return false;
    }
    if same_parser(doc.parser_of(parent), parse_script) {
        return false;
    }
    let prev = doc[node].prev;
    if doc.is_o(prev, TagId::BR) {
        return true;
    }
    if prev.is_none() && !has_cm(doc, Some(parent), CM_INLINE) {
        return true;
    }
    if prev.is_some() && !has_cm(doc, prev, CM_INLINE) && is_elem(doc, prev) {
        return true;
    }
    if prev.is_none() && doc[parent].prev.is_none() && !has_cm(doc, doc[parent].parent, CM_INLINE) {
        return true;
    }
    false
}

fn clean_spaces(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        if is_text(doc, Some(n)) && clean_leading_whitespace(doc, n) {
            while doc[n].start < doc[n].end && lexer::is_white(lb(doc, doc[n].start) as u32) {
                doc[n].start += 1;
            }
        }
        if is_text(doc, Some(n)) && clean_trailing_whitespace(doc, n) {
            while doc[n].end > doc[n].start && lexer::is_white(lb(doc, doc[n].end - 1) as u32) {
                doc[n].end -= 1;
            }
        }
        if is_text(doc, Some(n)) && doc[n].start >= doc[n].end {
            remove_node(doc, n);
            free(doc, n);
            node = next;
            continue;
        }
        if let Some(c) = doc[n].content {
            clean_spaces(doc, Some(c));
        }
        node = next;
    }
}

/// TrimSpaces(): move initial and trailing space out
fn trim_spaces(doc: &mut Doc, element: NodeId) {
    if doc.is(element, TagId::PRE) || is_pre_descendant(doc, element) {
        return;
    }
    let text = doc[element].content;
    if is_text(doc, text) {
        trim_initial_space(doc, element, text);
    }
    let text = doc[element].last;
    if is_text(doc, text) {
        trim_trailing_space(doc, element, text);
    }
}

fn descendant_of(doc: &Doc, element: NodeId, tid: TagId) -> bool {
    let mut parent = doc[element].parent;
    while let Some(p) = parent {
        if doc.is(p, tid) {
            return true;
        }
        parent = doc[p].parent;
    }
    false
}

fn insert_misc(doc: &mut Doc, element: NodeId, node: NodeId) -> bool {
    let t = doc[node].type_;
    if matches!(
        t,
        CommentTag | ProcInsTag | CDATATag | SectionTag | AspTag | JsteTag | PhpTag
    ) {
        insert_node_at_end(doc, element, node);
        return true;
    }
    if t == XmlDecl {
        let mut root = element;
        while let Some(p) = doc[root].parent {
            root = p;
        }
        let has_decl = doc[root].content.is_some_and(|c| doc[c].type_ == XmlDecl);
        if !has_decl {
            insert_node_at_start(doc, root, node);
            return true;
        }
    }
    if let Some(tag) = doc[node].tag {
        if is_elem(doc, Some(node))
            && tags::node_cm_is_empty(doc, Some(node))
            && doc.tag_id(Some(node)) == TagId::UNKNOWN
            && (doc.dict_versions(tag) & VERS_PROPRIETARY) != 0
        {
            insert_node_at_end(doc, element, node);
            return true;
        }
    }
    false
}

fn parse_tag(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    let Some(tag) = doc[node].tag else { return };
    let model = doc.dict_model(tag);
    let parser = doc.dict_parser(tag);
    if model & CM_EMPTY != 0 {
        doc.lx().waswhite = false;
        if parser.is_none() {
            return;
        }
    } else if model & CM_INLINE == 0 {
        doc.lx().insertspace = false;
    }
    let Some(parser) = parser else { return };
    if doc[node].type_ == StartEndTag {
        return;
    }
    doc.lx().parent = Some(node);
    parser(doc, node, mode);
}

/// InsertDocType(): the doctype has been found after other tags, and needs
/// moving to before the html element
fn insert_doc_type(doc: &mut Doc, element: NodeId, doctype: NodeId) {
    let existing = lexer::find_doc_type(doc);
    if existing.is_some() {
        rep(doc, Some(element), Some(doctype), DISCARDING_UNEXPECTED);
        free(doc, doctype);
    } else {
        rep(doc, Some(element), Some(doctype), DOCTYPE_AFTER_TAGS);
        let mut element = element;
        while !doc.is(element, TagId::HTML) {
            element = doc[element]
                .parent
                .expect("tidy: InsertDocType without html");
        }
        insert_node_before_element(doc, element, doctype);
    }
}

/// MoveToHead(): move node to the head
fn move_to_head(doc: &mut Doc, element: NodeId, node: NodeId) {
    remove_node(doc, node);
    if is_elem(doc, Some(node)) {
        rep(doc, Some(element), Some(node), TAG_NOT_ALLOWED_IN);
        let head = lexer::find_head(doc).expect("tidy: MoveToHead without head");
        insert_node_at_end(doc, head, node);
        if doc.parser_of(node).is_some() {
            parse_tag(doc, node, IgnoreWhitespace);
        }
    } else {
        rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
        free(doc, node);
    }
}

/// MoveNodeToBody(): moves given node to end of body element
fn move_node_to_body(doc: &mut Doc, node: NodeId) {
    if let Some(body) = lexer::find_body(doc) {
        remove_node(doc, node);
        insert_node_at_end(doc, body, node);
    }
}

fn add_class_no_indent(doc: &mut Doc, node: NodeId) {
    let sprop: &[u8] = b"padding-left: 2ex; margin-left: 0ex; margin-top: 0ex; margin-bottom: 0ex";
    if !doc.cfg_bool(OptId::DecorateInferredUL) {
        return;
    }
    if doc.cfg_bool(OptId::MakeClean) {
        clean::add_style_as_class(doc, node, sprop);
    } else {
        clean::add_style_property(doc, node, sprop);
    }
}

// ---------------------------------------------------------------------------
// ParseBlock

/// ParseBlock(): element is node created by the lexer upon seeing the start
/// tag, or by the parser when the start tag is inferred
pub fn parse_block(doc: &mut Doc, element: NodeId, mode: GetTokenMode) {
    let mut mode = mode;
    let mut checkstack = true;
    let mut istackbase = 0usize;

    let emodel = doc.model(element);
    if emodel & CM_EMPTY != 0 {
        return;
    }

    if doc.is(element, TagId::FORM) && descendant_of(doc, element, TagId::FORM) {
        rep(doc, Some(element), None, ILLEGAL_NESTING);
    }

    if emodel & CM_OBJECT != 0 {
        istackbase = doc.lxr().istackbase;
        let sz = istacksize(doc);
        doc.lx().istackbase = sz;
    }

    if emodel & CM_MIXED == 0 {
        istack::inline_dup(doc, None);
    }

    if emodel & CM_INLINE == 0 || emodel & CM_FIELD != 0 {
        mode = IgnoreWhitespace;
    } else if mode == IgnoreWhitespace {
        mode = MixedContent;
    }

    let mut cur: Option<NodeId>;
    loop {
        cur = get_token(doc, mode);
        let Some(mut node) = cur else { break };
        let emodel = doc.model(element);

        // end tag for this element
        if doc[node].type_ == EndTag
            && doc[node].tag.is_some()
            && (doc[node].tag == doc[element].tag || doc[element].was == doc[node].tag)
        {
            free(doc, node);
            if emodel & CM_OBJECT != 0 {
                pop_to_base(doc);
                doc.lx().istackbase = istackbase;
            }
            doc[element].closed = true;
            trim_spaces(doc, element);
            return;
        }

        if doc.is(node, TagId::HTML) || doc.is(node, TagId::HEAD) || doc.is(node, TagId::BODY) {
            if is_elem(doc, Some(node)) {
                rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
            }
            free(doc, node);
            continue;
        }

        if doc[node].type_ == EndTag {
            if doc[node].tag.is_none() {
                rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            } else if doc.is(node, TagId::BR) {
                doc[node].type_ = StartTag;
            } else if doc.is(node, TagId::P) {
                doc[node].type_ = StartEndTag;
                doc[node].implicit = true;
            } else if descendant_of(doc, element, doc.tag_id(Some(node))) {
                unget(doc);
                cur = Some(node);
                break;
            } else if doc.lxr().exiled
                && (has_cm(doc, Some(node), CM_TABLE) || doc.is(node, TagId::TABLE))
            {
                unget(doc);
                trim_spaces(doc, element);
                return;
            }
        }

        // mixed content model permits text
        if is_text(doc, Some(node)) {
            if checkstack {
                checkstack = false;
                if emodel & CM_MIXED == 0 && istack::inline_dup(doc, Some(node)) > 0 {
                    continue;
                }
            }
            insert_node_at_end(doc, element, node);
            mode = MixedContent;
            if doc.is(element, TagId::BODY)
                || doc.is(element, TagId::MAP)
                || doc.is(element, TagId::BLOCKQUOTE)
                || doc.is(element, TagId::FORM)
                || doc.is(element, TagId::NOSCRIPT)
            {
                lexer::constrain_version(doc, !VERS_HTML40_STRICT);
            }
            continue;
        }

        if insert_misc(doc, element, node) {
            continue;
        }

        // allow PARAM elements?
        if doc.is(node, TagId::PARAM) {
            if has_cm(doc, Some(element), CM_PARAM) && is_elem(doc, Some(node)) {
                insert_node_at_end(doc, element, node);
                continue;
            }
            rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // allow AREA elements?
        if doc.is(node, TagId::AREA) {
            if doc.is(element, TagId::MAP) && is_elem(doc, Some(node)) {
                insert_node_at_end(doc, element, node);
                continue;
            }
            rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // ignore unknown start/end tags
        if doc[node].tag.is_none() {
            rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        if !has_cm(doc, Some(node), CM_INLINE) {
            if !is_elem(doc, Some(node)) {
                if doc.is(node, TagId::FORM) {
                    bad_form(doc);
                }
                rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            if doc.is(element, TagId::LI)
                && (doc.is(node, TagId::FRAME)
                    || doc.is(node, TagId::FRAMESET)
                    || doc.is(node, TagId::OPTGROUP)
                    || doc.is(node, TagId::OPTION))
            {
                rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            if doc.is(element, TagId::TD) || doc.is(element, TagId::TH) {
                // if parent is a table cell, avoid inferring the end of the cell
                if has_cm(doc, Some(node), CM_HEAD) {
                    move_to_head(doc, element, node);
                    continue;
                }
                if has_cm(doc, Some(node), CM_LIST) {
                    unget(doc);
                    node = lexer::inferred_tag(doc, TagId::UL);
                    add_class_no_indent(doc, node);
                    doc.lx().exclude_blocks = true;
                } else if has_cm(doc, Some(node), CM_DEFLIST) {
                    unget(doc);
                    node = lexer::inferred_tag(doc, TagId::DL);
                    doc.lx().exclude_blocks = true;
                }
                // infer end of current table cell
                if !has_cm(doc, Some(node), CM_BLOCK) {
                    unget(doc);
                    trim_spaces(doc, element);
                    return;
                }
            } else if has_cm(doc, Some(node), CM_BLOCK) {
                if doc.lxr().exclude_blocks {
                    if !has_cm(doc, Some(element), CM_OPT) {
                        rep(doc, Some(element), Some(node), MISSING_ENDTAG_BEFORE);
                    }
                    unget(doc);
                    if has_cm(doc, Some(element), CM_OBJECT) {
                        doc.lx().istackbase = istackbase;
                    }
                    trim_spaces(doc, element);
                    return;
                }
            } else {
                // things like list items
                if doc.model(node) & CM_HEAD != 0 {
                    move_to_head(doc, element, node);
                    continue;
                }

                // special case where a form start tag occurs in a tr and is
                // followed by td or th
                let eparent = doc[element].parent;
                if doc.is(element, TagId::FORM)
                    && doc.is_o(eparent, TagId::TD)
                    && doc[eparent.unwrap()].implicit
                {
                    if doc.is(node, TagId::TD) {
                        rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
                        free(doc, node);
                        continue;
                    }
                    if doc.is(node, TagId::TH) {
                        rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
                        free(doc, node);
                        let p = eparent.unwrap();
                        doc[p].element = Some(b"th".to_vec());
                        doc[p].tag = tags::lookup_tag_def(TagId::TH);
                        continue;
                    }
                }

                if !has_cm(doc, Some(element), CM_OPT) && !doc[element].implicit {
                    rep(doc, Some(element), Some(node), MISSING_ENDTAG_BEFORE);
                }

                // #521, warn on missing optional end-tags if not omitting them
                if !doc.cfg_bool(OptId::OmitOptionalTags) && has_cm(doc, Some(element), CM_OPT) {
                    rep(doc, Some(element), Some(node), MISSING_ENDTAG_OPTIONAL);
                }

                unget(doc);

                if has_cm(doc, Some(node), CM_LIST) {
                    let ep = doc[element].parent;
                    if ep.is_some_and(|p| same_parser(doc.parser_of(p), parse_list)) {
                        trim_spaces(doc, element);
                        return;
                    }
                    node = lexer::inferred_tag(doc, TagId::UL);
                    add_class_no_indent(doc, node);
                } else if has_cm(doc, Some(node), CM_DEFLIST) {
                    if doc.is_o(doc[element].parent, TagId::DL) {
                        trim_spaces(doc, element);
                        return;
                    }
                    node = lexer::inferred_tag(doc, TagId::DL);
                } else if has_cm(doc, Some(node), CM_TABLE) || has_cm(doc, Some(node), CM_ROW) {
                    // In exiled mode, return so table processing can continue.
                    if doc.lxr().exiled {
                        return;
                    }
                    node = lexer::inferred_tag(doc, TagId::TABLE);
                } else if has_cm(doc, Some(element), CM_OBJECT) {
                    pop_to_base(doc);
                    doc.lx().istackbase = istackbase;
                    trim_spaces(doc, element);
                    return;
                } else {
                    trim_spaces(doc, element);
                    return;
                }
            }
        }

        // Issue #307 - an <A> tag to ends any open <A> element
        if doc.is(node, TagId::A)
            && !doc[node].implicit
            && (doc.is(element, TagId::A) || descendant_of(doc, element, TagId::A))
        {
            if doc[node].type_ != EndTag
                && doc[node].attributes.is_none()
                && doc.cfg_bool(OptId::CoerceEndTags)
            {
                doc[node].type_ = EndTag;
                rep(doc, Some(element), Some(node), COERCE_TO_ENDTAG);
                unget(doc);
                continue;
            }

            if doc.is(element, TagId::A) {
                rep(doc, Some(element), Some(node), MISSING_ENDTAG_BEFORE);
                unget(doc);
            } else {
                rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
            }

            if !pre_bit(mode) {
                trim_spaces(doc, element);
            }
            return;
        }

        // parse known element
        if is_elem(doc, Some(node)) {
            if doc.model(node) & CM_INLINE != 0 {
                if checkstack && !doc[node].implicit {
                    checkstack = false;
                    if doc.model(element) & CM_MIXED == 0 && istack::inline_dup(doc, Some(node)) > 0
                    {
                        continue;
                    }
                }
                mode = MixedContent;
            } else {
                checkstack = true;
                mode = IgnoreWhitespace;
            }

            // trim white space before <br>
            if doc.is(node, TagId::BR) {
                trim_spaces(doc, element);
            }

            insert_node_at_end(doc, element, node);

            if doc[node].implicit {
                rep(doc, Some(element), Some(node), INSERTING_TAG);
            }

            parse_tag(doc, node, IgnoreWhitespace);
            continue;
        }

        // discard unexpected tags
        if doc[node].type_ == EndTag {
            istack::pop_inline(doc, Some(node));
        }
        rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
        free(doc, node);
    }

    let emodel = doc.model(element);
    if emodel & CM_OPT == 0 {
        rep(doc, Some(element), cur, MISSING_ENDTAG_FOR);
    }

    if emodel & CM_OBJECT != 0 {
        pop_to_base(doc);
        doc.lx().istackbase = istackbase;
    }

    trim_spaces(doc, element);
}

// ---------------------------------------------------------------------------
// svg / math

/// FindMatchingDescendant(): search the parent chain (from 'parent' upwards
/// up to the root) for a node matching the given 'node'; returns the match
/// and whether the search passed beyond 'marker_node'.
fn find_matching_descendant(
    doc: &Doc,
    parent: Option<NodeId>,
    node: NodeId,
    marker_node: NodeId,
) -> (Option<NodeId>, bool) {
    let matching = doc.tag_id(Some(node));
    let mut passed = false;
    let mut cur = parent;
    while let Some(n) = cur {
        if doc.tag_id(Some(n)) == matching {
            let ok = matching != TagId::UNKNOWN
                || match (&doc[n].element, &doc[node].element) {
                    (Some(a), Some(b)) => tmbstr::tmbstrcmp(b, a) == 0,
                    _ => false,
                };
            if ok {
                return (Some(n), passed);
            }
        }
        if n == marker_node {
            passed = true;
        }
        cur = doc[n].parent;
    }
    (None, passed)
}

/// ParseNamespace(): act as a generic XML (sub)tree parser
pub fn parse_namespace(doc: &mut Doc, basenode: NodeId, mode: GetTokenMode) {
    let _ = mode;
    let mut parent = basenode;

    // a la <table>: defer popping elements off the inline stack
    istack::defer_dup(doc);
    let istackbase = doc.lxr().istackbase;
    let sz = istacksize(doc);
    doc.lx().istackbase = sz;

    let mode = OtherNamespace;

    while let Some(node) = get_token(doc, mode) {
        if doc[node].type_ == EndTag {
            let (mp, outside) = find_matching_descendant(doc, Some(parent), node, basenode);
            if let Some(mp) = mp {
                let base_parent = doc[basenode].parent;
                let mut n = Some(parent);
                while let Some(nn) = n {
                    if Some(nn) == base_parent || nn == mp {
                        break;
                    }
                    doc[nn].closed = true;
                    let np = doc[nn].parent;
                    rep(doc, np, Some(nn), MISSING_ENDTAG_BEFORE);
                    n = doc[nn].parent;
                }

                let cur;
                if !outside {
                    // EndTag for a node within the basenode subtree. Roll on...
                    let nn = n.expect("tidy: ParseNamespace lost its node");
                    doc[nn].closed = true;
                    free(doc, node);
                    cur = nn;
                    parent = doc[cur].parent.unwrap_or(parent);
                } else {
                    // EndTag for a node outside the basenode subtree
                    unget(doc);
                    cur = basenode;
                    parent = doc[cur].parent.unwrap_or(parent);
                }

                if cur == basenode {
                    doc.lx().istackbase = istackbase;
                    return;
                }
            } else {
                rep(doc, Some(parent), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
            }
        } else if doc[node].type_ == StartTag {
            for a in doc.attr_list(node) {
                doc[a].dict = None;
            }
            insert_node_at_end(doc, parent, node);
            parent = node;
        } else {
            for a in doc.attr_list(node) {
                doc[a].dict = None;
            }
            insert_node_at_end(doc, parent, node);
        }
    }

    let bp = doc[basenode].parent;
    rep(doc, bp, Some(basenode), MISSING_ENDTAG_FOR);
}

// ---------------------------------------------------------------------------
// ParseInline

/// ParseInline()
pub fn parse_inline(doc: &mut Doc, element: NodeId, mode: GetTokenMode) {
    let mut element = element;
    let mut mode = mode;

    if doc.model(element) & CM_EMPTY != 0 {
        return;
    }

    if (has_cm(doc, Some(element), CM_BLOCK) || doc.is(element, TagId::DT))
        && !has_cm(doc, Some(element), CM_MIXED)
    {
        istack::inline_dup(doc, None);
    } else if has_cm(doc, Some(element), CM_INLINE) {
        istack::push_inline(doc, element);
    }

    if doc.is(element, TagId::NOBR) {
        doc.bad_layout |= USING_NOBR;
    } else if doc.is(element, TagId::FONT) {
        doc.bad_layout |= USING_FONT;
    }

    // Inline elements may or may not be within a preformatted element
    if mode != Preformatted {
        mode = MixedContent;
    }

    let mut cur: Option<NodeId>;
    loop {
        cur = get_token(doc, mode);
        let Some(mut node) = cur else { break };

        // end tag for current element
        if doc[node].tag == doc[element].tag && doc[node].type_ == EndTag {
            if doc.model(element) & CM_INLINE != 0 {
                istack::pop_inline(doc, Some(node));
            }
            free(doc, node);

            if !pre_bit(mode) {
                trim_spaces(doc, element);
            }

            // if a font element wraps an anchor and nothing else then move
            // the font element inside the anchor
            if doc.is(element, TagId::FONT)
                && doc[element].content.is_some()
                && doc[element].content == doc[element].last
            {
                let child = doc[element].content.unwrap();
                if doc.is(child, TagId::A) {
                    doc[child].parent = doc[element].parent;
                    doc[child].next = doc[element].next;
                    doc[child].prev = doc[element].prev;

                    doc[element].next = None;
                    doc[element].prev = None;
                    doc[element].parent = Some(child);

                    doc[element].content = doc[child].content;
                    doc[element].last = doc[child].last;
                    doc[child].content = Some(element);

                    clean::fix_node_links(doc, child);
                    clean::fix_node_links(doc, element);
                }
            }

            doc[element].closed = true;
            trim_spaces(doc, element);
            return;
        }

        // <u>...<u>  map 2nd <u> to </u> if 1st is explicit
        if doc[node].type_ == StartTag
            && doc[node].tag == doc[element].tag
            && istack::is_pushed(doc, node)
            && !doc[node].implicit
            && !doc[element].implicit
            && doc[node].tag.is_some()
            && doc.model(node) & CM_INLINE != 0
            && !doc.is(node, TagId::A)
            && !doc.is(node, TagId::FONT)
            && !doc.is(node, TagId::BIG)
            && !doc.is(node, TagId::SMALL)
            && !doc.is(node, TagId::SUB)
            && !doc.is(node, TagId::SUP)
            && !doc.is(node, TagId::Q)
            && !doc.is(node, TagId::SPAN)
            && doc.cfg_bool(OptId::CoerceEndTags)
        {
            let last = doc[element].last;
            if doc[element].content.is_some()
                && doc[node].attributes.is_none()
                && is_text(doc, last)
                && !pprint::text_node_end_with_space(doc, last)
            {
                rep(doc, Some(element), Some(node), COERCE_TO_ENDTAG);
                doc[node].type_ = EndTag;
                unget(doc);
                continue;
            }

            if doc[node].attributes.is_none() || doc[element].attributes.is_none() {
                rep(doc, Some(element), Some(node), NESTED_EMPHASIS);
            }
        } else if istack::is_pushed(doc, node)
            && doc[node].type_ == StartTag
            && doc.is(node, TagId::Q)
        {
            // Issue #215 - nested quotes are not a problem in HTML5
            if lexer::html_version(doc) != HT50 {
                rep(doc, Some(element), Some(node), NESTED_QUOTATION);
            }
        }

        if is_text(doc, Some(node)) {
            // only called for 1st child
            if doc[element].content.is_none() && !pre_bit(mode) {
                trim_spaces(doc, element);
            }
            if doc[node].start >= doc[node].end {
                free(doc, node);
                continue;
            }
            insert_node_at_end(doc, element, node);
            continue;
        }

        // mixed content model so allow text
        if insert_misc(doc, element, node) {
            continue;
        }

        // deal with HTML tags
        if doc.is(node, TagId::HTML) {
            if is_elem(doc, Some(node)) {
                rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            // otherwise infer end of inline element
            unget(doc);
            if !pre_bit(mode) {
                trim_spaces(doc, element);
            }
            return;
        }

        // within <dt> or <pre> map <p> to <br>
        if doc.is(node, TagId::P)
            && doc[node].type_ == StartTag
            && (pre_bit(mode)
                || doc.is(element, TagId::DT)
                || descendant_of(doc, element, TagId::DT))
        {
            doc[node].tag = tags::lookup_tag_def(TagId::BR);
            doc[node].element = Some(b"br".to_vec());
            trim_spaces(doc, element);
            insert_node_at_end(doc, element, node);
            continue;
        }

        // <p> allowed within <address> in HTML 4.01 Transitional
        if doc.is(node, TagId::P) && doc[node].type_ == StartTag && doc.is(element, TagId::ADDRESS)
        {
            lexer::constrain_version(doc, !VERS_HTML40_STRICT);
            insert_node_at_end(doc, element, node);
            if let Some(p) = doc.parser_of(node) {
                p(doc, node, mode);
            }
            continue;
        }

        // ignore unknown and PARAM tags
        if doc[node].tag.is_none() || doc.is(node, TagId::PARAM) {
            rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        if doc.is(node, TagId::BR) && doc[node].type_ == EndTag {
            doc[node].type_ = StartTag;
        }

        if doc[node].type_ == EndTag {
            // coerce </br> to <br>
            if doc.is(node, TagId::BR) {
                doc[node].type_ = StartTag;
            } else if doc.is(node, TagId::P) {
                // coerce unmatched </p> to <br><br>
                if !descendant_of(doc, element, TagId::P) {
                    coerce_node(doc, node, TagId::BR, false, false);
                    trim_spaces(doc, element);
                    insert_node_at_end(doc, element, node);
                    node = lexer::inferred_tag(doc, TagId::BR);
                    insert_node_at_end(doc, element, node);
                    continue;
                }
            } else if has_cm(doc, Some(node), CM_INLINE)
                && !doc.is(node, TagId::A)
                && !has_cm(doc, Some(node), CM_OBJECT)
                && has_cm(doc, Some(element), CM_INLINE)
            {
                // allow any inline end tag to end current element
                if !doc.is(element, TagId::A)
                    && doc[node].tag != doc[element].tag
                    && istack::is_pushed(doc, node)
                    && istack::is_pushed(doc, element)
                {
                    // we have something like <b>bold <i>bold and italic</b> italics</i>
                    if istack::switch_inline(doc, element, node) {
                        rep(doc, Some(element), Some(node), NON_MATCHING_ENDTAG);
                        unget(doc);
                        // InlineDup1(doc, NULL, element)
                        istack::inline_dup1(doc, None, element);
                        if !pre_bit(mode) {
                            trim_spaces(doc, element);
                        }
                        return;
                    }
                }
                istack::pop_inline(doc, Some(element));

                if !doc.is(element, TagId::A) {
                    if doc.is(node, TagId::A) && doc[node].tag != doc[element].tag {
                        rep(doc, Some(element), Some(node), MISSING_ENDTAG_BEFORE);
                        unget(doc);
                    } else {
                        rep(doc, Some(element), Some(node), NON_MATCHING_ENDTAG);
                        free(doc, node);
                    }
                    if !pre_bit(mode) {
                        trim_spaces(doc, element);
                    }
                    return;
                }

                // if parent is <a> then discard unexpected inline end tag
                rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            } else if doc.lxr().exiled
                && (has_cm(doc, Some(node), CM_TABLE) || doc.is(node, TagId::TABLE))
            {
                unget(doc);
                trim_spaces(doc, element);
                return;
            }
        }

        // allow any header tag to end current header
        if has_cm(doc, Some(node), CM_HEADING) && has_cm(doc, Some(element), CM_HEADING) {
            if doc[node].tag == doc[element].tag {
                rep(doc, Some(element), Some(node), NON_MATCHING_ENDTAG);
                free(doc, node);
            } else {
                rep(doc, Some(element), Some(node), MISSING_ENDTAG_BEFORE);
                unget(doc);
            }
            if !pre_bit(mode) {
                trim_spaces(doc, element);
            }
            return;
        }

        // an <A> tag to ends any open <A> element but <A href=...> is mapped
        // to </A><A href=...>
        if doc.is(node, TagId::A)
            && !doc[node].implicit
            && (doc.is(element, TagId::A) || descendant_of(doc, element, TagId::A))
        {
            if doc[node].type_ != EndTag
                && doc[node].attributes.is_none()
                && doc.cfg_bool(OptId::CoerceEndTags)
            {
                doc[node].type_ = EndTag;
                rep(doc, Some(element), Some(node), COERCE_TO_ENDTAG);
                unget(doc);
                continue;
            }

            unget(doc);
            rep(doc, Some(element), Some(node), MISSING_ENDTAG_BEFORE);
            if !pre_bit(mode) {
                trim_spaces(doc, element);
            }
            return;
        }

        if doc.model(element) & CM_HEADING != 0 {
            if doc.is(node, TagId::CENTER) || doc.is(node, TagId::DIV) {
                if !is_elem(doc, Some(node)) {
                    rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
                    free(doc, node);
                    continue;
                }

                rep(doc, Some(element), Some(node), TAG_NOT_ALLOWED_IN);

                // insert center as parent if heading is empty
                if doc[element].content.is_none() {
                    insert_node_as_parent(doc, element, node);
                    continue;
                }

                // split heading and make center parent of 2nd part
                insert_node_after_element(doc, element, node);
                if !pre_bit(mode) {
                    trim_spaces(doc, element);
                }
                element = lexer::clone_node(doc, Some(element));
                insert_node_at_end(doc, node, element);
                continue;
            }

            if doc.is(node, TagId::HR) {
                if !is_elem(doc, Some(node)) {
                    rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
                    free(doc, node);
                    continue;
                }

                rep(doc, Some(element), Some(node), TAG_NOT_ALLOWED_IN);

                // insert hr before heading if heading is empty
                if doc[element].content.is_none() {
                    insert_node_before_element(doc, element, node);
                    continue;
                }

                // split heading and insert hr before 2nd part
                insert_node_after_element(doc, element, node);
                if !pre_bit(mode) {
                    trim_spaces(doc, element);
                }
                element = lexer::clone_node(doc, Some(element));
                insert_node_after_element(doc, node, element);
                continue;
            }
        }

        if doc.is(element, TagId::DT) && doc.is(node, TagId::HR) {
            if !is_elem(doc, Some(node)) {
                rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            rep(doc, Some(element), Some(node), TAG_NOT_ALLOWED_IN);
            let dd = lexer::inferred_tag(doc, TagId::DD);

            // insert hr within dd before dt if dt is empty
            if doc[element].content.is_none() {
                insert_node_before_element(doc, element, dd);
                insert_node_at_end(doc, dd, node);
                continue;
            }

            // split dt and insert hr within dd before 2nd part
            insert_node_after_element(doc, element, dd);
            insert_node_at_end(doc, dd, node);
            if !pre_bit(mode) {
                trim_spaces(doc, element);
            }
            element = lexer::clone_node(doc, Some(element));
            insert_node_after_element(doc, dd, element);
            continue;
        }

        // if this is the end tag for an ancestor element then infer end tag
        // for this element
        if doc[node].type_ == EndTag {
            let mut parent = doc[element].parent;
            while let Some(p) = parent {
                if doc[node].tag == doc[p].tag {
                    if doc.model(element) & CM_OPT == 0 && !doc[element].implicit {
                        rep(doc, Some(element), Some(node), MISSING_ENDTAG_BEFORE);
                    }
                    if istack::is_pushed_last(doc, element, node) {
                        istack::pop_inline(doc, Some(element));
                    }
                    unget(doc);
                    if !pre_bit(mode) {
                        trim_spaces(doc, element);
                    }
                    return;
                }
                parent = doc[p].parent;
            }
        }

        // block level tags end this element
        if doc.model(node) & CM_INLINE == 0
            && doc.model(element) & CM_MIXED == 0
            && !(doc.is(element, TagId::SPAN) && doc.is(node, TagId::META))
        {
            if !is_elem(doc, Some(node)) {
                rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }
            // HTML5
            if doc.is(element, TagId::DATALIST) {
                lexer::constrain_version(doc, !VERS_HTML5);
            } else if doc.model(element) & CM_OPT == 0 {
                rep(doc, Some(element), Some(node), MISSING_ENDTAG_BEFORE);
            }

            let nmodel = doc.model(node);
            if nmodel & CM_HEAD != 0 && nmodel & CM_BLOCK == 0 {
                move_to_head(doc, element, node);
                continue;
            }

            // prevent anchors from propagating into block tags except for
            // headings h1 to h6
            if doc.is(element, TagId::A) {
                if doc[node].tag.is_some() && doc.model(node) & CM_HEADING == 0 {
                    istack::pop_inline(doc, Some(element));
                } else if doc[element].content.is_none() {
                    discard_element(doc, element);
                    unget(doc);
                    return;
                }
            }

            unget(doc);
            if !pre_bit(mode) {
                trim_spaces(doc, element);
            }
            return;
        }

        // parse inline element
        if is_elem(doc, Some(node)) {
            if doc[node].implicit {
                rep(doc, Some(element), Some(node), INSERTING_TAG);
            }
            // trim white space before <br>
            if doc.is(node, TagId::BR) {
                trim_spaces(doc, element);
            }
            insert_node_at_end(doc, element, node);
            parse_tag(doc, node, mode);
            continue;
        }

        // discard unexpected tags
        rep(doc, Some(element), Some(node), DISCARDING_UNEXPECTED);
        free(doc, node);
    }

    if doc.model(element) & CM_OPT == 0 {
        rep(doc, Some(element), cur, MISSING_ENDTAG_FOR);
    }
}

/// ParseEmpty()
pub fn parse_empty(doc: &mut Doc, element: NodeId, mode: GetTokenMode) {
    if doc.lxr().isvoyager {
        if let Some(node) = get_token(doc, mode) {
            if !(doc[node].type_ == EndTag && doc[node].tag == doc[element].tag) {
                unget(doc);
            } else {
                free(doc, node);
            }
        }
    }
}

/// ParseDefList()
pub fn parse_def_list(doc: &mut Doc, list: NodeId, mode: GetTokenMode) {
    let mut list = list;
    if doc.model(list) & CM_EMPTY != 0 {
        return;
    }

    doc.lx().insert = None; // defer implicit inline start tags

    let mut cur: Option<NodeId>;
    loop {
        cur = get_token(doc, IgnoreWhitespace);
        let Some(mut node) = cur else { break };

        if doc[node].tag == doc[list].tag && doc[node].type_ == EndTag {
            free(doc, node);
            doc[list].closed = true;
            return;
        }

        // deal with comments etc.
        if insert_misc(doc, list, node) {
            continue;
        }

        if is_text(doc, Some(node)) {
            unget(doc);
            node = lexer::inferred_tag(doc, TagId::DT);
            rep(doc, Some(list), Some(node), MISSING_STARTTAG);
        }

        if doc[node].tag.is_none() {
            rep(doc, Some(list), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // if this is the end tag for an ancestor element then infer end tag
        // for this element
        if doc[node].type_ == EndTag {
            let mut discard_it = false;
            if doc.is(node, TagId::FORM) {
                bad_form(doc);
                rep(doc, Some(list), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            let mut parent = doc[list].parent;
            while let Some(p) = parent {
                // Do not match across BODY to avoid infinite loop
                if doc.is(p, TagId::BODY) {
                    discard_it = true;
                    break;
                }
                if doc[node].tag == doc[p].tag {
                    rep(doc, Some(list), Some(node), MISSING_ENDTAG_BEFORE);
                    unget(doc);
                    return;
                }
                parent = doc[p].parent;
            }
            if discard_it {
                rep(doc, Some(list), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }
        }

        // center in a dt or a dl breaks the dl list in two
        if doc.is(node, TagId::CENTER) {
            if doc[list].content.is_some() {
                insert_node_after_element(doc, list, node);
            } else {
                insert_node_before_element(doc, list, node);
            }

            // ParseTag can destroy node, if it finds that this <center> is
            // followed immediately by </center>.
            let parent = doc[node].parent;

            // and parse contents of center
            doc.lx().exclude_blocks = false;
            parse_tag(doc, node, mode);
            doc.lx().exclude_blocks = true;

            // now create a new dl element, unless node has been blown away
            if parent.is_some_and(|p| doc[p].last == Some(node)) {
                list = lexer::inferred_tag(doc, TagId::DL);
                insert_node_after_element(doc, node, list);
            }
            continue;
        }

        if !(doc.is(node, TagId::DT) || doc.is(node, TagId::DD)) {
            unget(doc);

            let nmodel = doc.model(node);
            if nmodel & (CM_BLOCK | CM_INLINE) == 0 {
                rep(doc, Some(list), Some(node), TAG_NOT_ALLOWED_IN);
                return;
            }

            // if DD appeared directly in BODY then exclude blocks
            if nmodel & CM_INLINE == 0 && doc.lxr().exclude_blocks {
                return;
            }

            node = lexer::inferred_tag(doc, TagId::DD);
            rep(doc, Some(list), Some(node), MISSING_STARTTAG);
        }

        if doc[node].type_ == EndTag {
            rep(doc, Some(list), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // node should be <DT> or <DD>
        insert_node_at_end(doc, list, node);
        parse_tag(doc, node, IgnoreWhitespace);
    }

    rep(doc, Some(list), cur, MISSING_ENDTAG_FOR);
}

/// FindLastLI()
fn find_last_li(doc: &Doc, list: NodeId) -> Option<NodeId> {
    let mut lastli = None;
    let mut node = doc[list].content;
    while let Some(n) = node {
        if doc.is(n, TagId::LI) && doc[n].type_ == StartTag {
            lastli = Some(n);
        }
        node = doc[n].next;
    }
    lastli
}

/// ParseList()
pub fn parse_list(doc: &mut Doc, list: NodeId, mode: GetTokenMode) {
    let _ = mode;
    let nodeis_ol = doc.is(list, TagId::OL);

    if doc.model(list) & CM_EMPTY != 0 {
        return;
    }
    doc.lx().insert = None; // defer implicit inline start tags

    let mut cur: Option<NodeId>;
    loop {
        cur = get_token(doc, IgnoreWhitespace);
        let Some(mut node) = cur else { break };
        let mut found_li = false;

        if doc[node].tag == doc[list].tag && doc[node].type_ == EndTag {
            free(doc, node);
            doc[list].closed = true;
            return;
        }

        // deal with comments etc.
        if insert_misc(doc, list, node) {
            continue;
        }

        if doc[node].type_ != TextNode && doc[node].tag.is_none() {
            rep(doc, Some(list), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }
        if doc[node].type_ == TextNode {
            let mut ix = doc[node].start;
            // Issue #572 - Skip whitespace.
            while ix < doc[node].end {
                let ch = lb(doc, ix);
                if ch != 0 && (ch == b' ' || ch == b'\t' || ch == b'\r' || ch == b'\n') {
                    ix += 1;
                } else {
                    break;
                }
            }
            if ix >= doc[node].end {
                // Issue #572 - Discard if ALL whitespace.
                free(doc, node);
                continue;
            }
        }

        // if this is the end tag for an ancestor element then infer end tag
        // for this element
        if doc[node].type_ == EndTag {
            if doc.is(node, TagId::FORM) {
                bad_form(doc);
                rep(doc, Some(list), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            if has_cm(doc, Some(node), CM_INLINE) {
                rep(doc, Some(list), Some(node), DISCARDING_UNEXPECTED);
                istack::pop_inline(doc, Some(node));
                free(doc, node);
                continue;
            }

            let mut parent = doc[list].parent;
            while let Some(p) = parent {
                // Do not match across BODY to avoid infinite loop
                if doc.is(p, TagId::BODY) {
                    break;
                }
                if doc[node].tag == doc[p].tag {
                    rep(doc, Some(list), Some(node), MISSING_ENDTAG_BEFORE);
                    unget(doc);
                    return;
                }
                parent = doc[p].parent;
            }

            rep(doc, Some(list), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        if !doc.is(node, TagId::LI) && nodeis_ol {
            // Issue #572 - A <ol><li> can have nested <ol> elements
            found_li = find_last_li(doc, list).is_some();
        }

        if doc.is(node, TagId::LI) || (tags::is_html5_mode(doc) && !found_li) {
            // node is <LI> OR Issue #396 - A <ul> can have Zero or more <li>
            insert_node_at_end(doc, list, node);
        } else {
            unget(doc);

            if has_cm(doc, Some(node), CM_BLOCK) && doc.lxr().exclude_blocks {
                rep(doc, Some(list), Some(node), MISSING_ENDTAG_BEFORE);
                return;
            } else if doc.lxr().exiled
                && (has_cm(doc, Some(node), CM_TABLE | CM_ROWGRP | CM_ROW)
                    || doc.is(node, TagId::TABLE))
            {
                // In exiled mode, return so table processing can continue.
                return;
            }
            // If "list" is an unordered list, insert the next tag within the
            // last <li> to preserve the numbering
            let lastli = if doc.is(list, TagId::OL) {
                find_last_li(doc, list)
            } else {
                None
            };
            if let Some(lastli) = lastli {
                // Create a node for error reporting
                node = lexer::inferred_tag(doc, TagId::LI);
                rep(doc, Some(list), Some(node), MISSING_STARTTAG);
                free(doc, node);
                node = lastli;
            } else {
                // Add an inferred <li>
                let wasblock = has_cm(doc, Some(node), CM_BLOCK);
                node = lexer::inferred_tag(doc, TagId::LI);
                clean::add_style_property(
                    doc,
                    node,
                    if wasblock {
                        b"list-style: none; display: inline"
                    } else {
                        b"list-style: none"
                    },
                );
                rep(doc, Some(list), Some(node), MISSING_STARTTAG);
                insert_node_at_end(doc, list, node);
            }
        }

        parse_tag(doc, node, IgnoreWhitespace);
    }

    rep(doc, Some(list), cur, MISSING_ENDTAG_FOR);
}

/// MoveBeforeTable(): unexpected content in table row is moved to just
/// before the table
fn move_before_table(doc: &mut Doc, row: NodeId, node: NodeId) {
    let mut table = doc[row].parent;
    while let Some(t) = table {
        if doc.is(t, TagId::TABLE) {
            insert_node_before_element(doc, t, node);
            return;
        }
        table = doc[t].parent;
    }
    // No table element
    let p = doc[row]
        .parent
        .expect("tidy: MoveBeforeTable without parent");
    insert_node_before_element(doc, p, node);
}

/// FixEmptyRow(): if a table row is empty then insert an empty cell
fn fix_empty_row(doc: &mut Doc, row: NodeId) {
    if doc[row].content.is_none() {
        let cell = lexer::inferred_tag(doc, TagId::TD);
        insert_node_at_end(doc, row, cell);
        rep(doc, Some(row), Some(cell), MISSING_STARTTAG);
    }
}

/// ParseRow()
pub fn parse_row(doc: &mut Doc, row: NodeId, mode: GetTokenMode) {
    let _ = mode;
    if doc.model(row) & CM_EMPTY != 0 {
        return;
    }

    while let Some(mut node) = get_token(doc, IgnoreWhitespace) {
        if doc[node].tag == doc[row].tag {
            if doc[node].type_ == EndTag {
                free(doc, node);
                doc[row].closed = true;
                fix_empty_row(doc, row);
                return;
            }
            // New row start implies end of current row
            unget(doc);
            fix_empty_row(doc, row);
            return;
        }

        // if this is the end tag for an ancestor element then infer end tag
        // for this element
        if doc[node].type_ == EndTag {
            if (has_cm(doc, Some(node), CM_HTML | CM_TABLE) || doc.is(node, TagId::TABLE))
                && descendant_of(doc, row, doc.tag_id(Some(node)))
            {
                unget(doc);
                return;
            }

            if doc.is(node, TagId::FORM) || has_cm(doc, Some(node), CM_BLOCK | CM_INLINE) {
                if doc.is(node, TagId::FORM) {
                    bad_form(doc);
                }
                rep(doc, Some(row), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            if doc.is(node, TagId::TD) || doc.is(node, TagId::TH) {
                rep(doc, Some(row), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }
        }

        // deal with comments etc.
        if insert_misc(doc, row, node) {
            continue;
        }

        // discard unknown tags
        if doc[node].tag.is_none() && doc[node].type_ != TextNode {
            rep(doc, Some(row), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // discard unexpected <table> element
        if doc.is(node, TagId::TABLE) {
            rep(doc, Some(row), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // THEAD, TFOOT or TBODY
        if has_cm(doc, Some(node), CM_ROWGRP) {
            unget(doc);
            return;
        }

        if doc[node].type_ == EndTag {
            rep(doc, Some(row), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // if text or inline or block move before table; if head content move
        // to head
        if doc[node].type_ != EndTag {
            if doc.is(node, TagId::FORM) {
                unget(doc);
                node = lexer::inferred_tag(doc, TagId::TD);
                rep(doc, Some(row), Some(node), MISSING_STARTTAG);
            } else if is_text(doc, Some(node)) || has_cm(doc, Some(node), CM_BLOCK | CM_INLINE) {
                move_before_table(doc, row, node);
                rep(doc, Some(row), Some(node), TAG_NOT_ALLOWED_IN);
                doc.lx().exiled = true;
                let exclude_state = doc.lxr().exclude_blocks;
                doc.lx().exclude_blocks = false;

                if doc[node].type_ != TextNode {
                    parse_tag(doc, node, IgnoreWhitespace);
                }

                doc.lx().exiled = false;
                doc.lx().exclude_blocks = exclude_state;
                continue;
            } else if doc.model(node) & CM_HEAD != 0 {
                rep(doc, Some(row), Some(node), TAG_NOT_ALLOWED_IN);
                move_to_head(doc, row, node);
                continue;
            }
        }

        if !(doc.is(node, TagId::TD) || doc.is(node, TagId::TH)) {
            rep(doc, Some(row), Some(node), TAG_NOT_ALLOWED_IN);
            free(doc, node);
            continue;
        }

        // node should be <TD> or <TH>
        insert_node_at_end(doc, row, node);
        let exclude_state = doc.lxr().exclude_blocks;
        doc.lx().exclude_blocks = false;
        parse_tag(doc, node, IgnoreWhitespace);
        doc.lx().exclude_blocks = exclude_state;

        // pop inline stack
        pop_to_base(doc);
    }
}

/// ParseRowGroup()
pub fn parse_row_group(doc: &mut Doc, rowgroup: NodeId, mode: GetTokenMode) {
    let _ = mode;
    if doc.model(rowgroup) & CM_EMPTY != 0 {
        return;
    }

    while let Some(mut node) = get_token(doc, IgnoreWhitespace) {
        if doc[node].tag == doc[rowgroup].tag {
            if doc[node].type_ == EndTag {
                doc[rowgroup].closed = true;
                free(doc, node);
                return;
            }
            unget(doc);
            return;
        }

        // if </table> infer end tag
        if doc.is(node, TagId::TABLE) && doc[node].type_ == EndTag {
            unget(doc);
            return;
        }

        // deal with comments etc.
        if insert_misc(doc, rowgroup, node) {
            continue;
        }

        // discard unknown tags
        if doc[node].tag.is_none() && doc[node].type_ != TextNode {
            rep(doc, Some(rowgroup), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // if TD or TH then infer <TR>; if text or inline or block move before
        // table; if head content move to head
        if doc[node].type_ != EndTag {
            if doc.is(node, TagId::TD) || doc.is(node, TagId::TH) {
                unget(doc);
                node = lexer::inferred_tag(doc, TagId::TR);
                rep(doc, Some(rowgroup), Some(node), MISSING_STARTTAG);
            } else if is_text(doc, Some(node)) || has_cm(doc, Some(node), CM_BLOCK | CM_INLINE) {
                move_before_table(doc, rowgroup, node);
                rep(doc, Some(rowgroup), Some(node), TAG_NOT_ALLOWED_IN);
                doc.lx().exiled = true;

                if doc[node].type_ != TextNode {
                    parse_tag(doc, node, IgnoreWhitespace);
                }

                doc.lx().exiled = false;
                continue;
            } else if doc.model(node) & CM_HEAD != 0 {
                rep(doc, Some(rowgroup), Some(node), TAG_NOT_ALLOWED_IN);
                move_to_head(doc, rowgroup, node);
                continue;
            }
        }

        // if this is the end tag for ancestor element then infer end tag for
        // this element
        if doc[node].type_ == EndTag {
            if doc.is(node, TagId::FORM) || has_cm(doc, Some(node), CM_BLOCK | CM_INLINE) {
                if doc.is(node, TagId::FORM) {
                    bad_form(doc);
                }
                rep(doc, Some(rowgroup), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            if doc.is(node, TagId::TR) || doc.is(node, TagId::TD) || doc.is(node, TagId::TH) {
                rep(doc, Some(rowgroup), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            let mut parent = doc[rowgroup].parent;
            while let Some(p) = parent {
                if doc[node].tag == doc[p].tag {
                    unget(doc);
                    return;
                }
                parent = doc[p].parent;
            }
        }

        // if THEAD, TFOOT or TBODY then implied end tag
        if doc.model(node) & CM_ROWGRP != 0 && doc[node].type_ != EndTag {
            unget(doc);
            return;
        }

        if doc[node].type_ == EndTag {
            rep(doc, Some(rowgroup), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        if !doc.is(node, TagId::TR) {
            node = lexer::inferred_tag(doc, TagId::TR);
            rep(doc, Some(rowgroup), Some(node), MISSING_STARTTAG);
            unget(doc);
        }

        // node should be <TR>
        insert_node_at_end(doc, rowgroup, node);
        parse_tag(doc, node, IgnoreWhitespace);
    }
}

/// ParseColGroup()
pub fn parse_col_group(doc: &mut Doc, colgroup: NodeId, mode: GetTokenMode) {
    let _ = mode;
    if doc.model(colgroup) & CM_EMPTY != 0 {
        return;
    }

    while let Some(node) = get_token(doc, IgnoreWhitespace) {
        if doc[node].tag == doc[colgroup].tag && doc[node].type_ == EndTag {
            free(doc, node);
            doc[colgroup].closed = true;
            return;
        }

        // if this is the end tag for an ancestor element then infer end tag
        // for this element
        if doc[node].type_ == EndTag {
            if doc.is(node, TagId::FORM) {
                bad_form(doc);
                rep(doc, Some(colgroup), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            let mut parent = doc[colgroup].parent;
            while let Some(p) = parent {
                if doc[node].tag == doc[p].tag {
                    unget(doc);
                    return;
                }
                parent = doc[p].parent;
            }
        }

        if is_text(doc, Some(node)) {
            unget(doc);
            return;
        }

        // deal with comments etc.
        if insert_misc(doc, colgroup, node) {
            continue;
        }

        // discard unknown tags
        if doc[node].tag.is_none() {
            rep(doc, Some(colgroup), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        if !doc.is(node, TagId::COL) {
            unget(doc);
            return;
        }

        if doc[node].type_ == EndTag {
            rep(doc, Some(colgroup), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // node should be <COL>
        insert_node_at_end(doc, colgroup, node);
        parse_tag(doc, node, IgnoreWhitespace);
    }
}

/// ParseTableTag()
pub fn parse_table_tag(doc: &mut Doc, table: NodeId, mode: GetTokenMode) {
    let _ = mode;
    istack::defer_dup(doc);
    let istackbase = doc.lxr().istackbase;
    let sz = istacksize(doc);
    doc.lx().istackbase = sz;

    let mut cur: Option<NodeId>;
    loop {
        cur = get_token(doc, IgnoreWhitespace);
        let Some(mut node) = cur else { break };

        if doc[node].tag == doc[table].tag {
            if doc[node].type_ == EndTag {
                free(doc, node);
            } else {
                // Issue #498 - If a <table> in a <table> just close the
                // current table, and issue a warning.
                unget(doc);
                rep(doc, Some(table), Some(node), TAG_NOT_ALLOWED_IN);
            }
            doc.lx().istackbase = istackbase;
            doc[table].closed = true;
            return;
        }

        // deal with comments etc.
        if insert_misc(doc, table, node) {
            continue;
        }

        // discard unknown tags
        if doc[node].tag.is_none() && doc[node].type_ != TextNode {
            rep(doc, Some(table), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // if TD or TH or text or inline or block then infer <TR>
        if doc[node].type_ != EndTag {
            if doc.is(node, TagId::TD) || doc.is(node, TagId::TH) || doc.is(node, TagId::TABLE) {
                unget(doc);
                node = lexer::inferred_tag(doc, TagId::TR);
                rep(doc, Some(table), Some(node), MISSING_STARTTAG);
            } else if is_text(doc, Some(node)) || has_cm(doc, Some(node), CM_BLOCK | CM_INLINE) {
                insert_node_before_element(doc, table, node);
                rep(doc, Some(table), Some(node), TAG_NOT_ALLOWED_IN);
                doc.lx().exiled = true;

                if doc[node].type_ != TextNode {
                    parse_tag(doc, node, IgnoreWhitespace);
                }

                doc.lx().exiled = false;
                continue;
            } else if doc.model(node) & CM_HEAD != 0 {
                move_to_head(doc, table, node);
                continue;
            }
        }

        // if this is the end tag for an ancestor element then infer end tag
        // for this element
        if doc[node].type_ == EndTag {
            if doc.is(node, TagId::FORM) {
                bad_form(doc);
                rep(doc, Some(table), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            // best to discard unexpected block/inline end tags
            if has_cm(doc, Some(node), CM_TABLE | CM_ROW)
                || has_cm(doc, Some(node), CM_BLOCK | CM_INLINE)
            {
                rep(doc, Some(table), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            let mut parent = doc[table].parent;
            while let Some(p) = parent {
                if doc[node].tag == doc[p].tag {
                    rep(doc, Some(table), Some(node), MISSING_ENDTAG_BEFORE);
                    unget(doc);
                    doc.lx().istackbase = istackbase;
                    return;
                }
                parent = doc[p].parent;
            }
        }

        if doc.model(node) & CM_TABLE == 0 {
            unget(doc);
            rep(doc, Some(table), Some(node), TAG_NOT_ALLOWED_IN);
            doc.lx().istackbase = istackbase;
            return;
        }

        if is_elem(doc, Some(node)) {
            insert_node_at_end(doc, table, node);
            parse_tag(doc, node, IgnoreWhitespace);
            continue;
        }

        // discard unexpected text nodes and end tags
        rep(doc, Some(table), Some(node), DISCARDING_UNEXPECTED);
        free(doc, node);
    }

    rep(doc, Some(table), cur, MISSING_ENDTAG_FOR);
    doc.lx().istackbase = istackbase;
}

/// PreContent(): acceptable content for pre elements
fn pre_content(doc: &Doc, node: NodeId) -> bool {
    // p is coerced to br's, Text OK too
    if doc.is(node, TagId::P) || is_text(doc, Some(node)) {
        return true;
    }
    if doc[node].tag.is_none()
        || doc.is(node, TagId::PARAM)
        || !has_cm(doc, Some(node), CM_INLINE | CM_NEW)
    {
        return false;
    }
    true
}

/// ParsePre()
pub fn parse_pre(doc: &mut Doc, pre: NodeId, mode: GetTokenMode) {
    let _ = mode;
    let mut pre = pre;
    if doc.model(pre) & CM_EMPTY != 0 {
        return;
    }

    istack::inline_dup(doc, None); // tell lexer to insert inlines if needed

    let mut cur: Option<NodeId>;
    loop {
        cur = get_token(doc, Preformatted);
        let Some(node) = cur else { break };

        if doc[node].type_ == EndTag
            && (doc[node].tag == doc[pre].tag || descendant_of(doc, pre, doc.tag_id(Some(node))))
        {
            if doc.is(node, TagId::BODY) || doc.is(node, TagId::HTML) {
                rep(doc, Some(pre), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }
            if doc[node].tag == doc[pre].tag {
                free(doc, node);
            } else {
                rep(doc, Some(pre), Some(node), MISSING_ENDTAG_BEFORE);
                unget(doc);
            }
            doc[pre].closed = true;
            trim_spaces(doc, pre);
            return;
        }

        if is_text(doc, Some(node)) {
            insert_node_at_end(doc, pre, node);
            continue;
        }

        // deal with comments etc.
        if insert_misc(doc, pre, node) {
            continue;
        }

        if doc[node].tag.is_none() {
            rep(doc, Some(pre), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // strip unexpected tags
        if !pre_content(doc, node) {
            if doc[node].type_ == EndTag {
                if doc.lxr().exiled
                    && (has_cm(doc, Some(node), CM_TABLE) || doc.is(node, TagId::TABLE))
                {
                    unget(doc);
                    trim_spaces(doc, pre);
                    return;
                }

                rep(doc, Some(pre), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            } else if has_cm(doc, Some(node), CM_TABLE | CM_ROW) || doc.is(node, TagId::TABLE) {
                if !doc.lxr().exiled {
                    // No missing close warning if exiled.
                    rep(doc, Some(pre), Some(node), MISSING_ENDTAG_BEFORE);
                }
                unget(doc);
                return;
            }

            insert_node_after_element(doc, pre, node);
            rep(doc, Some(pre), Some(node), MISSING_ENDTAG_BEFORE);
            parse_tag(doc, node, IgnoreWhitespace);

            let newnode = lexer::inferred_tag(doc, TagId::PRE);
            rep(doc, Some(pre), Some(newnode), INSERTING_TAG);
            pre = newnode;
            insert_node_after_element(doc, node, pre);
            continue;
        }

        if doc.is(node, TagId::P) {
            if doc[node].type_ == StartTag {
                rep(doc, Some(pre), Some(node), USING_BR_INPLACE_OF);

                // trim white space before <p> in <pre>
                trim_spaces(doc, pre);

                // coerce both <p> and </p> to <br>
                coerce_node(doc, node, TagId::BR, false, false);
                lexer::free_attrs(doc, node); // discard align attribute etc.
                insert_node_at_end(doc, pre, node);
            } else {
                rep(doc, Some(pre), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
            }
            continue;
        }

        if is_elem(doc, Some(node)) {
            // trim white space before <br>
            if doc.is(node, TagId::BR) {
                trim_spaces(doc, pre);
            }
            insert_node_at_end(doc, pre, node);
            parse_tag(doc, node, Preformatted);
            continue;
        }

        // discard unexpected tags
        rep(doc, Some(pre), Some(node), DISCARDING_UNEXPECTED);
        free(doc, node);
    }

    rep(doc, Some(pre), cur, MISSING_ENDTAG_FOR);
}

/// ParseOptGroup()
pub fn parse_opt_group(doc: &mut Doc, field: NodeId, mode: GetTokenMode) {
    let _ = mode;
    doc.lx().insert = None; // defer implicit inline start tags

    while let Some(node) = get_token(doc, IgnoreWhitespace) {
        if doc[node].tag == doc[field].tag && doc[node].type_ == EndTag {
            free(doc, node);
            doc[field].closed = true;
            trim_spaces(doc, field);
            return;
        }

        // deal with comments etc.
        if insert_misc(doc, field, node) {
            continue;
        }

        if doc[node].type_ == StartTag
            && (doc.is(node, TagId::OPTION) || doc.is(node, TagId::OPTGROUP))
        {
            if doc.is(node, TagId::OPTGROUP) {
                rep(doc, Some(field), Some(node), CANT_BE_NESTED);
            }
            insert_node_at_end(doc, field, node);
            parse_tag(doc, node, MixedContent);
            continue;
        }

        // discard unexpected tags
        rep(doc, Some(field), Some(node), DISCARDING_UNEXPECTED);
        free(doc, node);
    }
}

/// ParseSelect() and ParseDatalist() (identical in tidy)
fn parse_select_like(doc: &mut Doc, field: NodeId) {
    doc.lx().insert = None; // defer implicit inline start tags

    let mut cur: Option<NodeId>;
    loop {
        cur = get_token(doc, IgnoreWhitespace);
        let Some(node) = cur else { break };

        if doc[node].tag == doc[field].tag && doc[node].type_ == EndTag {
            free(doc, node);
            doc[field].closed = true;
            trim_spaces(doc, field);
            return;
        }

        // deal with comments etc.
        if insert_misc(doc, field, node) {
            continue;
        }

        if doc[node].type_ == StartTag
            && (doc.is(node, TagId::OPTION)
                || doc.is(node, TagId::OPTGROUP)
                || doc.is(node, TagId::DATALIST)
                || doc.is(node, TagId::SCRIPT))
        {
            insert_node_at_end(doc, field, node);
            parse_tag(doc, node, IgnoreWhitespace);
            continue;
        }

        // discard unexpected tags
        rep(doc, Some(field), Some(node), DISCARDING_UNEXPECTED);
        free(doc, node);
    }

    rep(doc, Some(field), cur, MISSING_ENDTAG_FOR);
}

/// ParseSelect()
pub fn parse_select(doc: &mut Doc, field: NodeId, mode: GetTokenMode) {
    let _ = mode;
    parse_select_like(doc, field);
}

/// ParseDatalist() (HTML5)
pub fn parse_datalist(doc: &mut Doc, field: NodeId, mode: GetTokenMode) {
    let _ = mode;
    parse_select_like(doc, field);
}

/// ParseText()
pub fn parse_text(doc: &mut Doc, field: NodeId, mode: GetTokenMode) {
    doc.lx().insert = None; // defer implicit inline start tags

    let mode = if doc.is(field, TagId::TEXTAREA) {
        Preformatted
    } else {
        let _ = mode;
        MixedContent // kludge for font tags
    };

    let mut cur: Option<NodeId>;
    loop {
        cur = get_token(doc, mode);
        let Some(node) = cur else { break };

        if doc[node].tag == doc[field].tag && doc[node].type_ == EndTag {
            free(doc, node);
            doc[field].closed = true;
            trim_spaces(doc, field);
            return;
        }

        // deal with comments etc.
        if insert_misc(doc, field, node) {
            continue;
        }

        if is_text(doc, Some(node)) {
            // only called for 1st child
            if doc[field].content.is_none() && !pre_bit(mode) {
                trim_spaces(doc, field);
            }
            if doc[node].start >= doc[node].end {
                free(doc, node);
                continue;
            }
            insert_node_at_end(doc, field, node);
            continue;
        }

        // discard inline tags e.g. font
        if doc[node].tag.is_some()
            && doc.model(node) & CM_INLINE != 0
            && doc.model(node) & CM_FIELD == 0
        {
            rep(doc, Some(field), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // terminate element on other tags
        if doc.model(field) & CM_OPT == 0 {
            rep(doc, Some(field), Some(node), MISSING_ENDTAG_BEFORE);
        }
        unget(doc);
        trim_spaces(doc, field);
        return;
    }

    if doc.model(field) & CM_OPT == 0 {
        rep(doc, Some(field), cur, MISSING_ENDTAG_FOR);
    }
}

/// ParseTitle()
pub fn parse_title(doc: &mut Doc, title: NodeId, mode: GetTokenMode) {
    let _ = mode;
    let mut cur: Option<NodeId>;
    loop {
        cur = get_token(doc, MixedContent);
        let Some(node) = cur else { break };

        if doc[node].tag == doc[title].tag
            && doc[node].type_ == StartTag
            && doc.cfg_bool(OptId::CoerceEndTags)
        {
            rep(doc, Some(title), Some(node), COERCE_TO_ENDTAG);
            doc[node].type_ = EndTag;
            unget(doc);
            continue;
        } else if doc[node].tag == doc[title].tag && doc[node].type_ == EndTag {
            free(doc, node);
            doc[title].closed = true;
            trim_spaces(doc, title);
            return;
        }

        if is_text(doc, Some(node)) {
            // only called for 1st child
            if doc[title].content.is_none() {
                trim_initial_space(doc, title, Some(node));
            }
            if doc[node].start >= doc[node].end {
                free(doc, node);
                continue;
            }
            insert_node_at_end(doc, title, node);
            continue;
        }

        // deal with comments etc.
        if insert_misc(doc, title, node) {
            continue;
        }

        // discard unknown tags
        if doc[node].tag.is_none() {
            rep(doc, Some(title), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // pushback unexpected tokens
        rep(doc, Some(title), Some(node), MISSING_ENDTAG_BEFORE);
        unget(doc);
        trim_spaces(doc, title);
        return;
    }

    rep(doc, Some(title), cur, MISSING_ENDTAG_FOR);
}

/// ParseScript(): this isn't quite right for CDATA content as it recognises
/// tags within the content and parses them accordingly.
pub fn parse_script(doc: &mut Doc, script: NodeId, mode: GetTokenMode) {
    let _ = mode;
    doc.lx().parent = Some(script);
    let node = get_token(doc, CdataContent);
    doc.lx().parent = None;

    match node {
        Some(n) => insert_node_at_end(doc, script, n),
        None => {
            // handle e.g. a document like "<script>"
            rep(doc, Some(script), None, MISSING_ENDTAG_FOR);
            return;
        }
    }

    let node = get_token(doc, IgnoreWhitespace);
    let ok = node.is_some_and(|n| {
        doc[n].type_ == EndTag
            && doc[n].tag.is_some()
            && doc.tag_id(Some(n)) == doc.tag_id(Some(script))
    });
    if !ok {
        rep(doc, Some(script), node, MISSING_ENDTAG_FOR);
        if node.is_some() {
            unget(doc);
        }
    } else {
        free(doc, node.unwrap());
    }
}

/// IsJavaScript()
pub fn is_javascript(doc: &mut Doc, node: NodeId) -> bool {
    if doc[node].attributes.is_none() {
        return true;
    }
    for attr in doc.attr_list(node) {
        let a = Some(attr);
        if (doc.attr_is(a, AttrId::LANGUAGE) || doc.attr_is(a, AttrId::TYPE))
            && doc.attr_contains(a, b"javascript")
        {
            return true;
        }
    }
    false
}

/// ParseHead()
pub fn parse_head(doc: &mut Doc, head: NodeId, mode: GetTokenMode) {
    let _ = mode;
    let mut has_title = 0;
    let mut has_base = 0;

    while let Some(node) = get_token(doc, IgnoreWhitespace) {
        if doc[node].tag == doc[head].tag && doc[node].type_ == EndTag {
            free(doc, node);
            doc[head].closed = true;
            break;
        }

        // find and discard multiple <head> elements
        // find and discard <html> in <head> elements
        if (doc[node].tag == doc[head].tag || doc.is(node, TagId::HTML))
            && doc[node].type_ == StartTag
        {
            rep(doc, Some(head), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        if is_text(doc, Some(node)) {
            // Issue #132 - avoid warning for missing body tag, if configured
            // to --omit-otpional-tags yes; Issue #314 - and if --show-body-only
            if !doc.cfg_bool(OptId::OmitOptionalTags) && !showing_body_only(doc) {
                rep(doc, Some(head), Some(node), TAG_NOT_ALLOWED_IN);
            }
            unget(doc);
            break;
        }

        if doc[node].type_ == ProcInsTag
            && doc[node]
                .element
                .as_deref()
                .is_some_and(|e| tmbstr::tmbstrcmp(e, b"xml-stylesheet") == 0)
        {
            rep(doc, Some(head), Some(node), TAG_NOT_ALLOWED_IN);
            if let Some(html) = lexer::find_html(doc) {
                insert_node_before_element(doc, html, node);
            }
            continue;
        }

        // deal with comments etc.
        if insert_misc(doc, head, node) {
            continue;
        }

        if doc[node].type_ == DocTypeTag {
            insert_doc_type(doc, head, node);
            continue;
        }

        // discard unknown tags
        if doc[node].tag.is_none() {
            rep(doc, Some(head), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // if it doesn't belong in the head then treat as implicit end of
        // head and deal with as part of the body
        if doc.model(node) & CM_HEAD == 0 {
            // #545067 Implicit closing of head broken - warn only for XHTML input
            if doc.lxr().isvoyager {
                rep(doc, Some(head), Some(node), TAG_NOT_ALLOWED_IN);
            }
            unget(doc);
            break;
        }

        if is_elem(doc, Some(node)) {
            if doc.is(node, TagId::TITLE) {
                has_title += 1;
                if has_title > 1 {
                    rep(doc, Some(head), Some(node), TOO_MANY_ELEMENTS_IN);
                }
            } else if doc.is(node, TagId::BASE) {
                has_base += 1;
                if has_base > 1 {
                    rep(doc, Some(head), Some(node), TOO_MANY_ELEMENTS_IN);
                }
            }

            insert_node_at_end(doc, head, node);
            parse_tag(doc, node, IgnoreWhitespace);
            continue;
        }

        // discard unexpected text nodes and end tags
        rep(doc, Some(head), Some(node), DISCARDING_UNEXPECTED);
        free(doc, node);
    }
}

/// FindNodeWithId(): Issue #166 - repeated <main> element
fn find_node_with_id(doc: &Doc, node: Option<NodeId>, tid: TagId) -> bool {
    let mut node = node;
    while let Some(n) = node {
        if doc.is(n, tid) {
            return true;
        }
        if let Some(c) = doc[n].content {
            if find_node_with_id(doc, Some(c), tid) {
                return true;
            }
        }
        node = doc[n].next;
    }
    false
}

/// FindNodeById(): do a global search for an element
fn find_node_by_id(doc: &Doc, tid: TagId) -> bool {
    find_node_with_id(doc, doc[doc.root].content, tid)
}

/// ParseBody()
pub fn parse_body(doc: &mut Doc, body: NodeId, mode: GetTokenMode) {
    let _ = mode;
    let mut mode = IgnoreWhitespace;
    let mut checkstack = true;

    let bp = doc[body].parent;
    clean::bump_object(doc, bp);

    while let Some(mut node) = get_token(doc, mode) {
        // find and discard multiple <body> elements
        if doc[node].tag == doc[body].tag && doc[node].type_ == StartTag {
            rep(doc, Some(body), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // #538536 Extra endtags not detected
        if doc.is(node, TagId::HTML) {
            if is_elem(doc, Some(node)) || doc.lxr().seen_end_html {
                rep(doc, Some(body), Some(node), DISCARDING_UNEXPECTED);
            } else {
                doc.lx().seen_end_html = true;
            }
            free(doc, node);
            continue;
        }

        if doc.lxr().seen_end_body && matches!(doc[node].type_, StartTag | EndTag | StartEndTag) {
            rep(doc, Some(body), Some(node), CONTENT_AFTER_BODY);
        }

        if doc[node].tag == doc[body].tag && doc[node].type_ == EndTag {
            doc[body].closed = true;
            trim_spaces(doc, body);
            free(doc, node);
            doc.lx().seen_end_body = true;
            mode = IgnoreWhitespace;

            if doc.is_o(doc[body].parent, TagId::NOFRAMES) {
                break;
            }
            continue;
        }

        if doc.is(node, TagId::NOFRAMES) {
            if doc[node].type_ == StartTag {
                insert_node_at_end(doc, body, node);
                parse_block(doc, node, mode);
                continue;
            }

            if doc[node].type_ == EndTag && doc.is_o(doc[body].parent, TagId::NOFRAMES) {
                trim_spaces(doc, body);
                unget(doc);
                break;
            }
        }

        if (doc.is(node, TagId::FRAME) || doc.is(node, TagId::FRAMESET))
            && doc.is_o(doc[body].parent, TagId::NOFRAMES)
        {
            trim_spaces(doc, body);
            unget(doc);
            break;
        }

        let iswhitenode = is_text(doc, Some(node))
            && doc[node].end <= doc[node].start + 1
            && lb(doc, doc[node].start) == b' ';

        // deal with comments etc.
        if insert_misc(doc, body, node) {
            continue;
        }

        // mixed content model permits text
        if is_text(doc, Some(node)) {
            if iswhitenode && mode == IgnoreWhitespace {
                free(doc, node);
                continue;
            }

            // HTML 2 and HTML4 strict don't allow text here
            lexer::constrain_version(doc, !(VERS_HTML40_STRICT | VERS_HTML20));

            if checkstack {
                checkstack = false;
                if istack::inline_dup(doc, Some(node)) > 0 {
                    continue;
                }
            }

            insert_node_at_end(doc, body, node);
            mode = MixedContent;
            continue;
        }

        if doc[node].type_ == DocTypeTag {
            insert_doc_type(doc, body, node);
            continue;
        }
        // discard unknown and PARAM tags
        if doc[node].tag.is_none() || doc.is(node, TagId::PARAM) {
            rep(doc, Some(body), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // Netscape allows LI and DD directly in BODY
        doc.lx().exclude_blocks = false;

        if (doc.is(node, TagId::INPUT)
            || (!has_cm(doc, Some(node), CM_BLOCK) && !has_cm(doc, Some(node), CM_INLINE)))
            && !tags::is_html5_mode(doc)
        {
            let nmodel = doc.model(node);
            // avoid this error message being issued twice
            if nmodel & CM_HEAD == 0 {
                rep(doc, Some(body), Some(node), TAG_NOT_ALLOWED_IN);
            }

            if nmodel & CM_HTML != 0 {
                // copy body attributes if current body was inferred
                if doc.is(node, TagId::BODY) && doc[body].implicit && doc[body].attributes.is_none()
                {
                    doc[body].attributes = doc[node].attributes;
                    doc[node].attributes = None;
                }
                free(doc, node);
                continue;
            }

            if nmodel & CM_HEAD != 0 {
                move_to_head(doc, body, node);
                continue;
            }

            if nmodel & CM_LIST != 0 {
                unget(doc);
                node = lexer::inferred_tag(doc, TagId::UL);
                add_class_no_indent(doc, node);
                doc.lx().exclude_blocks = true;
            } else if nmodel & CM_DEFLIST != 0 {
                unget(doc);
                node = lexer::inferred_tag(doc, TagId::DL);
                doc.lx().exclude_blocks = true;
            } else if nmodel & (CM_TABLE | CM_ROWGRP | CM_ROW) != 0 {
                // http://tidy.sf.net/issue/2855621
                if doc[node].type_ != EndTag {
                    unget(doc);
                    node = lexer::inferred_tag(doc, TagId::TABLE);
                }
                doc.lx().exclude_blocks = true;
            } else if doc.is(node, TagId::INPUT) {
                unget(doc);
                node = lexer::inferred_tag(doc, TagId::FORM);
                doc.lx().exclude_blocks = true;
            } else {
                if !has_cm(doc, Some(node), CM_ROW | CM_FIELD) {
                    unget(doc);
                    return;
                }
                // ignore </td> </th> <option> etc.
                free(doc, node);
                continue;
            }
        }

        if doc[node].type_ == EndTag {
            if doc.is(node, TagId::BR) {
                doc[node].type_ = StartTag;
            } else if doc.is(node, TagId::P) {
                doc[node].type_ = StartEndTag;
                doc[node].implicit = true;
            } else if has_cm(doc, Some(node), CM_INLINE) {
                istack::pop_inline(doc, Some(node));
            }
        }

        if is_elem(doc, Some(node)) {
            if doc.is(node, TagId::MAIN) {
                // Issue #166 - repeated <main> element
                if find_node_by_id(doc, TagId::MAIN) {
                    doc.bad_form |= FLG_BAD_MAIN;
                    rep(doc, Some(body), Some(node), DISCARDING_UNEXPECTED);
                    free(doc, node);
                    continue;
                }
            }
            if has_cm(doc, Some(node), CM_INLINE) {
                // HTML4 strict doesn't allow inline content here, but HTML2
                // does allow img elements as children of body
                if doc.is(node, TagId::IMG) {
                    lexer::constrain_version(doc, !VERS_HTML40_STRICT);
                } else {
                    lexer::constrain_version(doc, !(VERS_HTML40_STRICT | VERS_HTML20));
                }

                if checkstack && !doc[node].implicit {
                    checkstack = false;
                    if istack::inline_dup(doc, Some(node)) > 0 {
                        continue;
                    }
                }
                mode = MixedContent;
            } else {
                checkstack = true;
                mode = IgnoreWhitespace;
            }

            if doc[node].implicit {
                rep(doc, Some(body), Some(node), INSERTING_TAG);
            }

            insert_node_at_end(doc, body, node);
            parse_tag(doc, node, mode);
            continue;
        }

        // discard unexpected tags
        rep(doc, Some(body), Some(node), DISCARDING_UNEXPECTED);
        free(doc, node);
    }
}

/// ParseNoFrames()
pub fn parse_no_frames(doc: &mut Doc, noframes: NodeId, mode: GetTokenMode) {
    let _ = mode;
    if doc.cfg(OptId::AccessibilityCheckLevel) == 0 {
        doc.bad_access |= BA_USING_NOFRAMES;
    }
    let mode = IgnoreWhitespace;

    let mut cur: Option<NodeId>;
    loop {
        cur = get_token(doc, mode);
        let Some(mut node) = cur else { break };

        if doc[node].tag == doc[noframes].tag && doc[node].type_ == EndTag {
            free(doc, node);
            doc[noframes].closed = true;
            trim_spaces(doc, noframes);
            return;
        }

        if doc.is(node, TagId::FRAME) || doc.is(node, TagId::FRAMESET) {
            trim_spaces(doc, noframes);
            if doc[node].type_ == EndTag {
                rep(doc, Some(noframes), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
            } else {
                rep(doc, Some(noframes), Some(node), MISSING_ENDTAG_BEFORE);
                unget(doc);
            }
            return;
        }

        if doc.is(node, TagId::HTML) {
            if is_elem(doc, Some(node)) {
                rep(doc, Some(noframes), Some(node), DISCARDING_UNEXPECTED);
            }
            free(doc, node);
            continue;
        }

        // deal with comments etc.
        if insert_misc(doc, noframes, node) {
            continue;
        }

        if doc.is(node, TagId::BODY) && doc[node].type_ == StartTag {
            let seen_body = doc.lxr().seen_end_body;
            insert_node_at_end(doc, noframes, node);
            parse_tag(doc, node, IgnoreWhitespace);

            // fix for bug http://tidy.sf.net/bug/887259
            if seen_body && lexer::find_body(doc) != Some(node) {
                coerce_node(doc, node, TagId::DIV, false, false);
                move_node_to_body(doc, node);
            }
            continue;
        }

        // implicit body element inferred
        if is_text(doc, Some(node)) || (doc[node].tag.is_some() && doc[node].type_ != EndTag) {
            let body = lexer::find_body(doc);
            if body.is_some() || doc.lxr().seen_end_body {
                let Some(body) = body else {
                    rep(doc, Some(noframes), Some(node), DISCARDING_UNEXPECTED);
                    free(doc, node);
                    continue;
                };
                if is_text(doc, Some(node)) {
                    unget(doc);
                    node = lexer::inferred_tag(doc, TagId::P);
                    rep(doc, Some(noframes), Some(node), CONTENT_AFTER_BODY);
                }
                insert_node_at_end(doc, body, node);
            } else {
                unget(doc);
                node = lexer::inferred_tag(doc, TagId::BODY);
                if doc.cfg_bool(OptId::XmlOut) {
                    rep(doc, Some(noframes), Some(node), INSERTING_TAG);
                }
                insert_node_at_end(doc, noframes, node);
            }

            parse_tag(doc, node, IgnoreWhitespace);
            continue;
        }

        // discard unexpected end tags
        rep(doc, Some(noframes), Some(node), DISCARDING_UNEXPECTED);
        free(doc, node);
    }

    rep(doc, Some(noframes), cur, MISSING_ENDTAG_FOR);
}

/// ParseFrameSet()
pub fn parse_frame_set(doc: &mut Doc, frameset: NodeId, mode: GetTokenMode) {
    let _ = mode;
    if doc.cfg(OptId::AccessibilityCheckLevel) == 0 {
        doc.bad_access |= BA_USING_FRAMES;
    }

    let mut cur: Option<NodeId>;
    loop {
        cur = get_token(doc, IgnoreWhitespace);
        let Some(mut node) = cur else { break };

        if doc[node].tag == doc[frameset].tag && doc[node].type_ == EndTag {
            free(doc, node);
            doc[frameset].closed = true;
            trim_spaces(doc, frameset);
            return;
        }

        // deal with comments etc.
        if insert_misc(doc, frameset, node) {
            continue;
        }

        if doc[node].tag.is_none() {
            rep(doc, Some(frameset), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        if is_elem(doc, Some(node)) && doc.model(node) & CM_HEAD != 0 {
            move_to_head(doc, frameset, node);
            continue;
        }

        if doc.is(node, TagId::BODY) {
            unget(doc);
            node = lexer::inferred_tag(doc, TagId::NOFRAMES);
            rep(doc, Some(frameset), Some(node), INSERTING_TAG);
        }

        if doc[node].type_ == StartTag && doc.model(node) & CM_FRAMES != 0 {
            insert_node_at_end(doc, frameset, node);
            doc.lx().exclude_blocks = false;
            parse_tag(doc, node, MixedContent);
            continue;
        } else if doc[node].type_ == StartEndTag && doc.model(node) & CM_FRAMES != 0 {
            insert_node_at_end(doc, frameset, node);
            continue;
        }

        // discard unexpected tags
        // WAI [6.5.1.4] link is being discarded outside of NOFRAME
        if doc.is(node, TagId::A) {
            doc.bad_access |= BA_INVALID_LINK_NOFRAMES;
        }

        rep(doc, Some(frameset), Some(node), DISCARDING_UNEXPECTED);
        free(doc, node);
    }

    rep(doc, Some(frameset), cur, MISSING_ENDTAG_FOR);
}

/// ParseHTML()
pub fn parse_html(doc: &mut Doc, html: NodeId, mode: GetTokenMode) {
    let mut frameset: Option<NodeId> = None;
    let mut noframes: Option<NodeId> = None;

    config::set_option_bool(doc, OptId::XmlTags, false);

    let mut node;
    loop {
        let Some(n) = get_token(doc, IgnoreWhitespace) else {
            node = lexer::inferred_tag(doc, TagId::HEAD);
            break;
        };
        node = n;

        if doc.is(node, TagId::HEAD) {
            break;
        }

        if doc[node].tag == doc[html].tag && doc[node].type_ == EndTag {
            rep(doc, Some(html), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // find and discard multiple <html> elements
        if doc[node].tag == doc[html].tag && doc[node].type_ == StartTag {
            rep(doc, Some(html), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        // deal with comments etc.
        if insert_misc(doc, html, node) {
            continue;
        }

        unget(doc);
        node = lexer::inferred_tag(doc, TagId::HEAD);
        break;
    }

    let head = node;
    insert_node_at_end(doc, html, head);
    parse_head(doc, head, mode);

    loop {
        let Some(n) = get_token(doc, IgnoreWhitespace) else {
            if frameset.is_none() {
                // implied body
                let node = lexer::inferred_tag(doc, TagId::BODY);
                insert_node_at_end(doc, html, node);
                parse_body(doc, node, mode);
            }
            return;
        };
        node = n;

        // robustly handle html tags
        if doc[node].tag == doc[html].tag {
            if doc[node].type_ != StartTag && frameset.is_none() {
                rep(doc, Some(html), Some(node), DISCARDING_UNEXPECTED);
            }
            free(doc, node);
            continue;
        }

        // deal with comments etc.
        if insert_misc(doc, html, node) {
            continue;
        }

        // if frameset document coerce <body> to <noframes>
        if doc.is(node, TagId::BODY) {
            if doc[node].type_ != StartTag {
                rep(doc, Some(html), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            if doc.cfg(OptId::AccessibilityCheckLevel) == 0 {
                if let Some(fs) = frameset {
                    unget(doc);

                    let nf = match noframes {
                        None => {
                            let nf = lexer::inferred_tag(doc, TagId::NOFRAMES);
                            noframes = Some(nf);
                            insert_node_at_end(doc, fs, nf);
                            rep(doc, Some(html), Some(nf), INSERTING_TAG);
                            nf
                        }
                        Some(nf) => {
                            if doc[nf].type_ == StartEndTag {
                                doc[nf].type_ = StartTag;
                            }
                            nf
                        }
                    };

                    parse_tag(doc, nf, mode);
                    continue;
                }
            }

            lexer::constrain_version(doc, !VERS_FRAMESET);
            break; // to parse body
        }

        // flag an error if we see more than one frameset
        if doc.is(node, TagId::FRAMESET) {
            if doc[node].type_ != StartTag {
                rep(doc, Some(html), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            if frameset.is_some() {
                rep(doc, Some(html), Some(node), DUPLICATE_FRAMESET);
            } else {
                frameset = Some(node);
            }

            insert_node_at_end(doc, html, node);
            parse_tag(doc, node, mode);

            // see if it includes a noframes element so that we can merge
            // subsequent noframes elements
            let mut c = doc[frameset.unwrap()].content;
            while let Some(cn) = c {
                if doc.is(cn, TagId::NOFRAMES) {
                    noframes = Some(cn);
                }
                c = doc[cn].next;
            }
            continue;
        }

        // if not a frameset document coerce <noframes> to <body>
        if doc.is(node, TagId::NOFRAMES) {
            if doc[node].type_ != StartTag {
                rep(doc, Some(html), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }

            let Some(fs) = frameset else {
                rep(doc, Some(html), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                node = lexer::inferred_tag(doc, TagId::BODY);
                break;
            };

            let nf = match noframes {
                None => {
                    noframes = Some(node);
                    insert_node_at_end(doc, fs, node);
                    node
                }
                Some(nf) => {
                    free(doc, node);
                    nf
                }
            };

            parse_tag(doc, nf, mode);
            continue;
        }

        if is_elem(doc, Some(node)) {
            if doc[node].tag.is_some() && doc.model(node) & CM_HEAD != 0 {
                move_to_head(doc, html, node);
                continue;
            }

            // discard illegal frame element following a frameset
            if frameset.is_some() && doc.is(node, TagId::FRAME) {
                rep(doc, Some(html), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }
        }

        unget(doc);

        // insert other content into noframes element
        if let Some(fs) = frameset {
            let nf = match noframes {
                None => {
                    let nf = lexer::inferred_tag(doc, TagId::NOFRAMES);
                    noframes = Some(nf);
                    insert_node_at_end(doc, fs, nf);
                    nf
                }
                Some(nf) => {
                    rep(doc, Some(html), Some(node), NOFRAMES_CONTENT);
                    if doc[nf].type_ == StartEndTag {
                        doc[nf].type_ = StartTag;
                    }
                    nf
                }
            };

            lexer::constrain_version(doc, VERS_FRAMESET);
            parse_tag(doc, nf, mode);
            continue;
        }

        node = lexer::inferred_tag(doc, TagId::BODY);
        // Issue #132 - disable inserting BODY tag warning BUT only if NOT
        // --show-body-only yes
        if !showing_body_only(doc) {
            rep(doc, Some(html), Some(node), INSERTING_TAG);
        }
        lexer::constrain_version(doc, !VERS_FRAMESET);
        break;
    }

    // node must be body
    insert_node_at_end(doc, html, node);
    parse_tag(doc, node, mode);
}

fn node_cm_is_only_inline(doc: &Doc, node: Option<NodeId>) -> bool {
    has_cm(doc, node, CM_INLINE) && !has_cm(doc, node, CM_BLOCK)
}

fn enclose_body_text(doc: &mut Doc) {
    let Some(body) = lexer::find_body(doc) else {
        return;
    };
    let mut node = doc[body].content;
    while let Some(n) = node {
        if (is_text(doc, Some(n)) && !is_blank_ro(doc, Some(n)))
            || (is_elem(doc, Some(n)) && node_cm_is_only_inline(doc, Some(n)))
        {
            let p = lexer::inferred_tag(doc, TagId::P);
            insert_node_before_element(doc, n, p);
            let mut cur = Some(n);
            while let Some(c) = cur {
                if is_elem(doc, Some(c)) && !node_cm_is_only_inline(doc, Some(c)) {
                    break;
                }
                let next = doc[c].next;
                remove_node(doc, c);
                insert_node_at_end(doc, p, c);
                cur = next;
            }
            trim_spaces(doc, p);
            node = cur;
            continue;
        }
        node = doc[n].next;
    }
}

/// EncloseBlockText(): <form>, <blockquote> and <noscript> do not allow
/// #PCDATA in HTML 4.01 Strict
fn enclose_block_text(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;

        if let Some(c) = doc[n].content {
            enclose_block_text(doc, Some(c));
        }

        if !(doc.is(n, TagId::FORM) || doc.is(n, TagId::NOSCRIPT) || doc.is(n, TagId::BLOCKQUOTE))
            || doc[n].content.is_none()
        {
            node = next;
            continue;
        }

        let block = doc[n].content.unwrap();

        if (is_text(doc, Some(block)) && !is_blank_ro(doc, Some(block)))
            || (is_elem(doc, Some(block)) && node_cm_is_only_inline(doc, Some(block)))
        {
            let p = lexer::inferred_tag(doc, TagId::P);
            insert_node_before_element(doc, block, p);
            let mut cur = Some(block);
            while let Some(b) = cur {
                if is_elem(doc, Some(b)) && !node_cm_is_only_inline(doc, Some(b)) {
                    break;
                }
                let temp_next = doc[b].next;
                remove_node(doc, b);
                insert_node_at_end(doc, p, b);
                cur = temp_next;
            }
            trim_spaces(doc, p);
            continue;
        }

        node = next;
    }
}

fn replace_obsolete_elements(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;

        // HTML5 - <menu ... > is no longer obsolete
        if doc.is(n, TagId::DIR) {
            coerce_node(doc, n, TagId::UL, true, true);
        }

        if doc.is(n, TagId::XMP) || doc.is(n, TagId::LISTING) || doc.is(n, TagId::PLAINTEXT) {
            coerce_node(doc, n, TagId::PRE, true, true);
        }

        if let Some(c) = doc[n].content {
            replace_obsolete_elements(doc, Some(c));
        }

        node = next;
    }
}

fn attribute_checks(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;

        if is_elem(doc, Some(n)) {
            match doc[n].tag.and_then(|t| doc.dict_chkattrs(t)) {
                Some(chk) => chk(doc, n),
                None => tags::check_attributes(doc, n),
            }
        }

        if let Some(c) = doc[n].content {
            attribute_checks(doc, Some(c));
        }

        node = next;
    }
}

/// ParseDocument(): HTML is the top level element
pub fn parse_document(doc: &mut Doc) {
    let root = doc.root;
    let mut doctype: Option<NodeId> = None;

    while let Some(node) = get_token(doc, IgnoreWhitespace) {
        if doc[node].type_ == XmlDecl {
            doc.xml_detected = true;

            if lexer::find_xml_decl(doc).is_some() && doc[root].content.is_some() {
                rep(doc, Some(root), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
                continue;
            }
            if doc[node].line > 1 || doc[node].column != 1 {
                rep(doc, Some(root), Some(node), SPACE_PRECEDING_XMLDECL);
            }
        }

        // deal with comments etc.
        if insert_misc(doc, root, node) {
            continue;
        }

        if doc[node].type_ == DocTypeTag {
            if doctype.is_none() {
                insert_node_at_end(doc, root, node);
                doctype = Some(node);
            } else {
                rep(doc, Some(root), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
            }
            continue;
        }

        if doc[node].type_ == EndTag {
            rep(doc, Some(root), Some(node), DISCARDING_UNEXPECTED);
            free(doc, node);
            continue;
        }

        if doc[node].type_ == StartTag && doc.is(node, TagId::HTML) {
            let xmlns = attrs::attr_get_by_id(doc, node, AttrId::XMLNS);
            if doc.attr_value_is(xmlns, XHTML_NAMESPACE) {
                let html_out = doc.cfg_bool(OptId::HtmlOut);
                doc.lx().isvoyager = true; // Unless plain HTML
                config::set_option_bool(doc, OptId::XhtmlOut, !html_out); // is specified, output
                config::set_option_bool(doc, OptId::XmlOut, !html_out); // will be XHTML.

                // adjust other config options, just as in config.c
                if !html_out {
                    config::set_option_bool(doc, OptId::UpperCaseTags, false);
                    config::set_option_int(doc, OptId::UpperCaseAttrs, 0);
                }
            }
        }

        let html = if doc[node].type_ != StartTag || !doc.is(node, TagId::HTML) {
            unget(doc);
            lexer::inferred_tag(doc, TagId::HTML)
        } else {
            node
        };

        // #72, avoid MISSING_DOCTYPE if show-body-only.
        // #191, also if --doctype omit, that is TidyDoctypeOmit
        // #342, adjust tags to html4-- if not 'auto' or 'html5'
        if lexer::find_doc_type(doc).is_none() {
            let dtmode = doc.cfg(OptId::DoctypeMode);
            if dtmode != TIDY_DOCTYPE_OMIT && !showing_body_only(doc) {
                rep(doc, None, None, MISSING_DOCTYPE);
            }
            if dtmode != TIDY_DOCTYPE_AUTO && dtmode != TIDY_DOCTYPE_HTML5 {
                // Issue #342 - if not doctype 'auto', or 'html5' then reset
                // mode htm4-- parsing
                tags::adjust_tags(doc);
            }
        }
        insert_node_at_end(doc, root, html);
        parse_html(doc, html, IgnoreWhitespace);
        break;
    }

    // do this before any more document fixes
    if doc.cfg(OptId::AccessibilityCheckLevel) > 0 {
        crate::access::accessibility_checks(doc);
    }

    if lexer::find_html(doc).is_none() {
        // a later check should complain if <body> is empty
        let html = lexer::inferred_tag(doc, TagId::HTML);
        insert_node_at_end(doc, root, html);
        parse_html(doc, html, IgnoreWhitespace);
    }

    match lexer::find_title(doc) {
        None => {
            let head = lexer::find_head(doc);
            // #72, avoid MISSING_TITLE_ELEMENT if show-body-only
            if !showing_body_only(doc) {
                rep(doc, head, None, MISSING_TITLE_ELEMENT);
            }
            let title = lexer::inferred_tag(doc, TagId::TITLE);
            if let Some(head) = head {
                insert_node_at_end(doc, head, title);
            }
        }
        Some(node) => {
            if doc[node].content.is_none() && !showing_body_only(doc) {
                // Is #839 - warn node is blank in HTML5
                if tags::is_html5_mode(doc) {
                    rep(doc, Some(node), None, BLANK_TITLE_ELEMENT);
                }
            }
        }
    }

    attribute_checks(doc, Some(root));
    replace_obsolete_elements(doc, Some(root));
    drop_empty_elements(doc, Some(root));
    clean_spaces(doc, Some(root));

    if doc.cfg_bool(OptId::EncloseBodyText) {
        enclose_body_text(doc);
    }
    if doc.cfg_bool(OptId::EncloseBlockText) {
        enclose_block_text(doc, Some(root));
    }
}

/// XMLPreserveWhiteSpace()
pub fn xml_preserve_white_space(doc: &mut Doc, element: NodeId) -> bool {
    // search attributes for xml:space
    for attribute in doc.attr_list(element) {
        let a = Some(attribute);
        if doc.attr_is(a, AttrId::XML_SPACE) {
            return doc.attr_value_is(a, b"preserve");
        }
    }

    let Some(el) = doc[element].element.clone() else {
        return false;
    };

    // kludge for html docs without explicit xml:space attribute
    if doc.is(element, TagId::PRE)
        || doc.is(element, TagId::SCRIPT)
        || doc.is(element, TagId::STYLE)
        || same_parser(tags::find_parser(doc, element), parse_pre)
    {
        return true;
    }

    // kludge for XSL docs
    tmbstr::tmbstrcasecmp(&el, b"xsl:text") == 0
}

/// ParseXMLElement(): XML documents
fn parse_xml_element(doc: &mut Doc, element: NodeId, mode: GetTokenMode) {
    let mut mode = mode;

    // if node is pre or has xml:space="preserve" then do so
    if xml_preserve_white_space(doc, element) {
        mode = Preformatted;
    }

    while let Some(node) = get_token(doc, mode) {
        if doc[node].type_ == EndTag {
            let same = match (&doc[node].element, &doc[element].element) {
                (Some(a), Some(b)) => tmbstr::tmbstrcmp(a, b) == 0,
                _ => false,
            };
            if same {
                free(doc, node);
                doc[element].closed = true;
                break;
            }
        }

        // discard unexpected end tags
        if doc[node].type_ == EndTag {
            rep(doc, Some(element), Some(node), UNEXPECTED_ENDTAG_IN);
            free(doc, node);
            continue;
        }

        // parse content on seeing start tag
        if doc[node].type_ == StartTag {
            parse_xml_element(doc, node, mode);
        }

        insert_node_at_end(doc, element, node);
    }

    // if first child is text then trim initial space and delete text node
    // if it is empty.
    let node = doc[element].content;
    if is_text(doc, node) && mode != Preformatted {
        let n = node.unwrap();
        if lb(doc, doc[n].start) == b' ' {
            doc[n].start += 1;
            if doc[n].start >= doc[n].end {
                discard_element(doc, n);
            }
        }
    }

    // if last child is text then trim final space and delete the text node
    // if it is empty
    let node = doc[element].last;
    if is_text(doc, node) && mode != Preformatted {
        let n = node.unwrap();
        let e = doc[n].end;
        let c = if e == 0 { 0 } else { lb(doc, e - 1) };
        if c == b' ' {
            doc[n].end -= 1;
            if doc[n].start >= doc[n].end {
                discard_element(doc, n);
            }
        }
    }
}

/// ParseXMLDocument()
pub fn parse_xml_document(doc: &mut Doc) {
    let root = doc.root;
    let mut doctype: Option<NodeId> = None;

    config::set_option_bool(doc, OptId::XmlTags, true);

    doc.xml_detected = true;

    while let Some(node) = get_token(doc, IgnoreWhitespace) {
        // discard unexpected end tags
        if doc[node].type_ == EndTag {
            rep(doc, None, Some(node), UNEXPECTED_ENDTAG);
            free(doc, node);
            continue;
        }

        // deal with comments etc.
        if insert_misc(doc, root, node) {
            continue;
        }

        if doc[node].type_ == DocTypeTag {
            if doctype.is_none() {
                insert_node_at_end(doc, root, node);
                doctype = Some(node);
            } else {
                rep(doc, Some(root), Some(node), DISCARDING_UNEXPECTED);
                free(doc, node);
            }
            continue;
        }

        if doc[node].type_ == StartEndTag {
            insert_node_at_end(doc, root, node);
            continue;
        }

        // if start tag then parse element's content
        if doc[node].type_ == StartTag {
            insert_node_at_end(doc, root, node);
            parse_xml_element(doc, node, IgnoreWhitespace);
            continue;
        }

        rep(doc, Some(root), Some(node), DISCARDING_UNEXPECTED);
        free(doc, node);
    }

    // ensure presence of initial <?xml version="1.0"?>
    if doc.cfg_bool(OptId::XmlDecl) {
        lexer::fix_xml_decl(doc);
    }
}
