//! attrs.c: attribute dictionary, lookups, value checks and the anchor
//! list.
#![allow(unused_variables)]

use crate::doc::*;
use crate::enums::AttrId;

/// `AttrCheck`
pub type AttrCheckFn = fn(&mut Doc, NodeId, AttId);

/// _Attribute
#[derive(Debug)]
pub struct Attribute {
    pub id: AttrId,
    pub name: &'static [u8],
    pub attrchk: Option<AttrCheckFn>,
}

/// _Anchor
#[derive(Clone, Debug)]
pub struct Anchor {
    pub node: NodeId,
    pub name: Vec<u8>,
}

/// TidyAttribImpl
#[derive(Clone, Debug, Default)]
pub struct AttribImpl {
    /// anchor/node lookup: ANCHOR_HASH_SIZE buckets, each in list order
    pub anchor_hash: Vec<Vec<Anchor>>,
    /// Prioritized list of attributes to write
    pub priority_attribs: Vec<Vec<u8>>,
}

/// CheckAttribute(): check the attribute value; returns its dict
pub fn check_attribute(doc: &mut Doc, node: NodeId, attval: AttId) -> Option<&'static Attribute> {
    todo!("CheckAttribute")
}

/// FindAttribute(): the dict of an attribute (by name)
pub fn find_attribute(doc: &mut Doc, attval: AttId) -> Option<&'static Attribute> {
    todo!("FindAttribute")
}

/// GetAttrByName()
pub fn get_attr_by_name(doc: &Doc, node: NodeId, name: &[u8]) -> Option<AttId> {
    todo!("GetAttrByName")
}

/// DropAttrByName()
pub fn drop_attr_by_name(doc: &mut Doc, node: NodeId, name: &[u8]) {
    todo!("DropAttrByName")
}

/// AddAttribute(): add an attribute at the end of the node's list
pub fn add_attribute(doc: &mut Doc, node: NodeId, name: &[u8], value: &[u8]) -> AttId {
    todo!("AddAttribute")
}

/// RepairAttrValue(): set or add an attribute
pub fn repair_attr_value(doc: &mut Doc, node: NodeId, name: &[u8], value: Option<&[u8]>) -> AttId {
    todo!("RepairAttrValue")
}

/// DefinePriorityAttribute()
pub fn define_priority_attribute(doc: &mut Doc, name: &[u8]) {
    todo!("DefinePriorityAttribute")
}

/// getPriorityAttrList() / getNextPriorityAttr()
pub fn get_priority_attrs(doc: &Doc) -> Vec<Vec<u8>> {
    todo!("getPriorityAttrList")
}

/// IsUrl()
pub fn is_url(doc: &mut Doc, attrname: &[u8]) -> bool {
    todo!("IsUrl")
}

/// IsScript()
pub fn is_script(doc: &mut Doc, attrname: &[u8]) -> bool {
    todo!("IsScript")
}

/// IsAnchorElement(): may this element have an id / name anchor?
pub fn is_anchor_element(doc: &mut Doc, node: NodeId) -> bool {
    todo!("IsAnchorElement")
}

/// IsCSS1Selector()
pub fn is_css1_selector(buf: &[u8]) -> bool {
    todo!("IsCSS1Selector")
}

/// IsValidHTMLID()
pub fn is_valid_html_id(id: &[u8]) -> bool {
    todo!("IsValidHTMLID")
}

/// IsValidXMLID()
pub fn is_valid_xml_id(id: &[u8]) -> bool {
    todo!("IsValidXMLID")
}

/// RemoveAnchorByNode()
pub fn remove_anchor_by_node(doc: &mut Doc, name: &[u8], node: NodeId) {
    todo!("RemoveAnchorByNode")
}

/// FreeAnchors()
pub fn free_anchors(doc: &mut Doc) {
    todo!("FreeAnchors")
}

/// InitAttrs()
pub fn init_attrs(doc: &mut Doc) {
    todo!("InitAttrs")
}

/// FreeAttrTable()
pub fn free_attr_table(doc: &mut Doc) {
    todo!("FreeAttrTable")
}

/// FreeAttrPriorityList()
pub fn free_attr_priority_list(doc: &mut Doc) {
    todo!("FreeAttrPriorityList")
}

/// AppendToClassAttr()
pub fn append_to_class_attr(doc: &mut Doc, classattr: AttId, classname: &[u8]) {
    todo!("AppendToClassAttr")
}

/// RepairDuplicateAttributes()
pub fn repair_duplicate_attributes(doc: &mut Doc, node: NodeId, is_xml: bool) {
    todo!("RepairDuplicateAttributes")
}

/// SortAttributes()
pub fn sort_attributes(doc: &mut Doc, node: Option<NodeId>, strat: u32) {
    todo!("SortAttributes")
}

/// IsBoolAttribute()
pub fn is_bool_attribute(doc: &Doc, attval: AttId) -> bool {
    todo!("IsBoolAttribute")
}

/// attrIsEvent()
pub fn attr_is_event(doc: &Doc, attval: AttId) -> bool {
    todo!("attrIsEvent")
}

/// AttrGetById()
pub fn attr_get_by_id(doc: &Doc, node: NodeId, id: AttrId) -> Option<AttId> {
    todo!("AttrGetById")
}

/// NodeAttributeVersions()
pub fn node_attribute_versions(doc: &Doc, node: NodeId, id: AttrId) -> u32 {
    todo!("NodeAttributeVersions")
}

/// AttributeIsProprietary()
pub fn attribute_is_proprietary(doc: &Doc, node: NodeId, attval: AttId) -> bool {
    todo!("AttributeIsProprietary")
}

/// AttributeIsMismatched()
pub fn attribute_is_mismatched(doc: &Doc, node: NodeId, attval: AttId) -> bool {
    todo!("AttributeIsMismatched")
}

/// check_action()
pub fn check_action(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_action")
}

/// check_align()
pub fn check_align(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_align")
}

/// check_bool()
pub fn check_bool(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_bool")
}

/// check_clear()
pub fn check_clear(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_clear")
}

/// check_color()
pub fn check_color(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_color")
}

/// check_fsubmit()
pub fn check_fsubmit(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_fsubmit")
}

/// check_id()
pub fn check_id(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_id")
}

/// check_is()
pub fn check_is(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_is")
}

/// check_lang()
pub fn check_lang(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_lang")
}

/// check_length()
pub fn check_length(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_length")
}

/// check_loading()
pub fn check_loading(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_loading")
}

/// check_name()
pub fn check_name(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_name")
}

/// check_number()
pub fn check_number(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_number")
}

/// check_rd_fa_prefix()
pub fn check_rd_fa_prefix(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_rd_fa_prefix")
}

/// check_rd_fa_safe_curie()
pub fn check_rd_fa_safe_curie(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_rd_fa_safe_curie")
}

/// check_rd_fa_term()
pub fn check_rd_fa_term(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_rd_fa_term")
}

/// check_scope()
pub fn check_scope(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_scope")
}

/// check_script()
pub fn check_script(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_script")
}

/// check_scroll()
pub fn check_scroll(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_scroll")
}

/// check_shape()
pub fn check_shape(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_shape")
}

/// check_svg_attr()
pub fn check_svg_attr(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_svg_attr")
}

/// check_target()
pub fn check_target(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_target")
}

/// check_text_dir()
pub fn check_text_dir(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_text_dir")
}

/// check_type()
pub fn check_type(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_type")
}

/// check_url()
pub fn check_url(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_url")
}

/// check_v_type()
pub fn check_v_type(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_v_type")
}

/// check_valign()
pub fn check_valign(doc: &mut Doc, node: NodeId, attval: AttId) {
    todo!("check_valign")
}
