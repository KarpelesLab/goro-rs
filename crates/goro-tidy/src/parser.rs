//! parser.c: the HTML/XML tree builder and its repair rules.
#![allow(unused_variables)]

use crate::doc::*;
use crate::enums::TagId;

/// CheckNodeIntegrity()
pub fn check_node_integrity(doc: &mut Doc, node: NodeId) -> bool {
    todo!("CheckNodeIntegrity")
}

/// IsNewNode(): node created by the user (CM_NEW)
pub fn is_new_node(doc: &mut Doc, node: NodeId) -> bool {
    todo!("IsNewNode")
}

/// CoerceNode()
pub fn coerce_node(doc: &mut Doc, node: NodeId, tid: TagId, obsolete: bool, unexpected: bool) {
    todo!("CoerceNode")
}

/// RemoveNode(): extract a node and its children from a markup tree
pub fn remove_node(doc: &mut Doc, node: NodeId) -> NodeId {
    todo!("RemoveNode")
}

/// DiscardElement(): remove node from markup tree and discard it; returns
/// the next node
pub fn discard_element(doc: &mut Doc, element: NodeId) -> Option<NodeId> {
    todo!("DiscardElement")
}

/// InsertNodeAtStart(): insert node into markup tree as the first element
/// of content of element
pub fn insert_node_at_start(doc: &mut Doc, element: NodeId, node: NodeId) {
    todo!("InsertNodeAtStart")
}

/// InsertNodeAtEnd()
pub fn insert_node_at_end(doc: &mut Doc, element: NodeId, node: NodeId) {
    todo!("InsertNodeAtEnd")
}

/// InsertNodeBeforeElement()
pub fn insert_node_before_element(doc: &mut Doc, element: NodeId, node: NodeId) {
    todo!("InsertNodeBeforeElement")
}

/// InsertNodeAfterElement()
pub fn insert_node_after_element(doc: &mut Doc, element: NodeId, node: NodeId) {
    todo!("InsertNodeAfterElement")
}

/// TrimEmptyElement(): returns the next node
pub fn trim_empty_element(doc: &mut Doc, element: NodeId) -> Option<NodeId> {
    todo!("TrimEmptyElement")
}

/// DropEmptyElements()
pub fn drop_empty_elements(doc: &mut Doc, node: Option<NodeId>) -> Option<NodeId> {
    todo!("DropEmptyElements")
}

/// IsBlank(): a text node with no content
pub fn is_blank(doc: &mut Doc, node: NodeId) -> bool {
    todo!("IsBlank")
}

/// IsJavaScript()
pub fn is_javascript(doc: &mut Doc, node: NodeId) -> bool {
    todo!("IsJavaScript")
}

/// ParseDocument(): HTML is the top level element
pub fn parse_document(doc: &mut Doc) {
    todo!("ParseDocument")
}

/// XMLPreserveWhiteSpace()
pub fn xml_preserve_white_space(doc: &mut Doc, element: NodeId) -> bool {
    todo!("XMLPreserveWhiteSpace")
}

/// ParseXMLDocument()
pub fn parse_xml_document(doc: &mut Doc) {
    todo!("ParseXMLDocument")
}

/// ParseHTML()
pub fn parse_html(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseHTML")
}

/// ParseHead()
pub fn parse_head(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseHead")
}

/// ParseTitle()
pub fn parse_title(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseTitle")
}

/// ParseScript()
pub fn parse_script(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseScript")
}

/// ParseFrameSet()
pub fn parse_frame_set(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseFrameSet")
}

/// ParseNoFrames()
pub fn parse_no_frames(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseNoFrames")
}

/// ParseBody()
pub fn parse_body(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseBody")
}

/// ParsePre()
pub fn parse_pre(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParsePre")
}

/// ParseList()
pub fn parse_list(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseList")
}

/// ParseDefList()
pub fn parse_def_list(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseDefList")
}

/// ParseBlock()
pub fn parse_block(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseBlock")
}

/// ParseInline()
pub fn parse_inline(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseInline")
}

/// ParseEmpty()
pub fn parse_empty(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseEmpty")
}

/// ParseTableTag()
pub fn parse_table_tag(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseTableTag")
}

/// ParseColGroup()
pub fn parse_col_group(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseColGroup")
}

/// ParseRowGroup()
pub fn parse_row_group(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseRowGroup")
}

/// ParseRow()
pub fn parse_row(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseRow")
}

/// ParseSelect()
pub fn parse_select(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseSelect")
}

/// ParseOptGroup()
pub fn parse_opt_group(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseOptGroup")
}

/// ParseText()
pub fn parse_text(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseText")
}

/// ParseDatalist()
pub fn parse_datalist(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseDatalist")
}

/// ParseNamespace()
pub fn parse_namespace(doc: &mut Doc, node: NodeId, mode: GetTokenMode) {
    todo!("ParseNamespace")
}
