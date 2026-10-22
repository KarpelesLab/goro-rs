//! clean.c: the clean-up passes (presentational markup to style rules,
//! Word 2000, emphasis nesting, anchors, xhtml namespace, ...).
#![allow(unused_variables)]

use crate::doc::*;

/// FixNodeLinks()
pub fn fix_node_links(doc: &mut Doc, node: NodeId) {
    todo!("FixNodeLinks")
}

/// FreeStyles()
pub fn free_styles(doc: &mut Doc) {
    todo!("FreeStyles")
}

/// AddStyleAsClass()
pub fn add_style_as_class(doc: &mut Doc, node: NodeId, stylevalue: &[u8]) {
    todo!("AddStyleAsClass")
}

/// AddStyleProperty()
pub fn add_style_property(doc: &mut Doc, node: NodeId, property: &[u8]) {
    todo!("AddStyleProperty")
}

/// CleanDocument()
pub fn clean_document(doc: &mut Doc) {
    todo!("CleanDocument")
}

/// NestedEmphasis()
pub fn nested_emphasis(doc: &mut Doc, node: Option<NodeId>) {
    todo!("NestedEmphasis")
}

/// EmFromI()
pub fn em_from_i(doc: &mut Doc, node: Option<NodeId>) {
    todo!("EmFromI")
}

/// List2BQ()
pub fn list2bq(doc: &mut Doc, node: Option<NodeId>) {
    todo!("List2BQ")
}

/// BQ2Div()
pub fn bq2div(doc: &mut Doc, node: Option<NodeId>) {
    todo!("BQ2Div")
}

/// DropSections()
pub fn drop_sections(doc: &mut Doc, node: Option<NodeId>) {
    todo!("DropSections")
}

/// CleanWord2000()
pub fn clean_word2000(doc: &mut Doc, node: Option<NodeId>) {
    todo!("CleanWord2000")
}

/// IsWord2000()
pub fn is_word2000(doc: &mut Doc) -> bool {
    todo!("IsWord2000")
}

/// BumpObject()
pub fn bump_object(doc: &mut Doc, html: Option<NodeId>) {
    todo!("BumpObject")
}

/// TidyMetaCharset()
pub fn tidy_meta_charset(doc: &mut Doc) -> bool {
    todo!("TidyMetaCharset")
}

/// DropComments()
pub fn drop_comments(doc: &mut Doc, node: Option<NodeId>) {
    todo!("DropComments")
}

/// DropFontElements()
pub fn drop_font_elements(doc: &mut Doc, node: Option<NodeId>) {
    todo!("DropFontElements")
}

/// WbrToSpace()
pub fn wbr_to_space(doc: &mut Doc, node: Option<NodeId>) {
    todo!("WbrToSpace")
}

/// DowngradeTypography()
pub fn downgrade_typography(doc: &mut Doc, node: Option<NodeId>) {
    todo!("DowngradeTypography")
}

/// ReplacePreformattedSpaces()
pub fn replace_preformatted_spaces(doc: &mut Doc, node: Option<NodeId>) {
    todo!("ReplacePreformattedSpaces")
}

/// NormalizeSpaces()
pub fn normalize_spaces(doc: &mut Doc, node: Option<NodeId>) {
    todo!("NormalizeSpaces")
}

/// ConvertCDATANodes()
pub fn convert_cdata_nodes(doc: &mut Doc, node: Option<NodeId>) {
    todo!("ConvertCDATANodes")
}

/// FixAnchors()
pub fn fix_anchors(doc: &mut Doc, node: Option<NodeId>, want_name: bool, want_id: bool) {
    todo!("FixAnchors")
}

/// FixXhtmlNamespace()
pub fn fix_xhtml_namespace(doc: &mut Doc, want_xmlns: bool) {
    todo!("FixXhtmlNamespace")
}

/// FixLanguageInformation()
pub fn fix_language_information(
    doc: &mut Doc,
    node: Option<NodeId>,
    want_xml_lang: bool,
    want_lang: bool,
) {
    todo!("FixLanguageInformation")
}

/// CleanStyle()
pub fn clean_style(doc: &mut Doc, html: Option<NodeId>) {
    todo!("CleanStyle")
}

/// CleanHead()
pub fn clean_head(doc: &mut Doc) {
    todo!("CleanHead")
}
