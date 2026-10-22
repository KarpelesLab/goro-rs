//! lexer.c: the lexer (GetToken), node/attribute construction, doctype
//! and version handling.
#![allow(unused_variables)]

use crate::doc::*;
use crate::enums::TagId;

/// HTMLVersion(): choose what version to use for new doctype
pub fn html_version(doc: &mut Doc) -> u32 {
    todo!("HTMLVersion")
}

/// ConstrainVersion()
pub fn constrain_version(doc: &mut Doc, vers: u32) {
    todo!("ConstrainVersion")
}

pub fn is_white(c: u32) -> bool {
    todo!("IsWhite")
}

pub fn is_digit(c: u32) -> bool {
    todo!("IsDigit")
}

pub fn is_letter(c: u32) -> bool {
    todo!("IsLetter")
}

pub fn is_html_space(c: u32) -> bool {
    todo!("IsHTMLSpace")
}

pub fn is_newline(c: u32) -> bool {
    todo!("IsNewline")
}

pub fn is_namechar(c: u32) -> bool {
    todo!("IsNamechar")
}

pub fn is_xml_letter(c: u32) -> bool {
    todo!("IsXMLLetter")
}

pub fn is_xml_namechar(c: u32) -> bool {
    todo!("IsXMLNamechar")
}

pub fn is_upper(c: u32) -> bool {
    todo!("IsUpper")
}

pub fn to_lower(c: u32) -> u32 {
    todo!("ToLower")
}

pub fn to_upper(c: u32) -> u32 {
    todo!("ToUpper")
}

/// NewLexer()
pub fn new_lexer(doc: &mut Doc) -> Box<Lexer> {
    todo!("NewLexer")
}

/// FreeLexer()
pub fn free_lexer(doc: &mut Doc) {
    todo!("FreeLexer")
}

/// AddCharToLexer(): store character c as UTF-8 encoded byte stream
pub fn add_char_to_lexer(doc: &mut Doc, c: u32) {
    todo!("AddCharToLexer")
}

/// NewNode(): a TextNode at the lexer's position (if there is a lexer)
pub fn new_node(doc: &mut Doc) -> NodeId {
    todo!("NewNode")
}

/// CloneNode(): used to clone heading nodes when split by an <HR>
pub fn clone_node(doc: &mut Doc, element: Option<NodeId>) -> NodeId {
    todo!("CloneNode")
}

/// FreeAttrs()
pub fn free_attrs(doc: &mut Doc, node: NodeId) {
    todo!("FreeAttrs")
}

/// FreeAttribute(): doesn't repair attribute list linkage
pub fn free_attribute(doc: &mut Doc, av: AttId) {
    todo!("FreeAttribute")
}

/// DetachAttribute(): detach attribute from node
pub fn detach_attribute(doc: &mut Doc, node: NodeId, attr: AttId) {
    todo!("DetachAttribute")
}

/// RemoveAttribute(): detach attribute from node then free it
pub fn remove_attribute(doc: &mut Doc, node: NodeId, attr: AttId) {
    todo!("RemoveAttribute")
}

/// FreeNode(): frees the node, its children and its following siblings
pub fn free_node(doc: &mut Doc, node: Option<NodeId>) {
    todo!("FreeNode")
}

/// TextToken()
pub fn text_token(doc: &mut Doc) -> NodeId {
    todo!("TextToken")
}

/// NewLineNode(): used for creating preformatted text from Word2000
pub fn new_line_node(doc: &mut Doc) -> NodeId {
    todo!("NewLineNode")
}

/// NewLiteralTextNode(): used for adding a &nbsp; for Word2000
pub fn new_literal_text_node(doc: &mut Doc, txt: &[u8]) -> NodeId {
    todo!("NewLiteralTextNode")
}

/// AddStringLiteral()
pub fn add_string_literal(doc: &mut Doc, s: &[u8]) {
    todo!("AddStringLiteral")
}

pub fn find_doc_type(doc: &mut Doc) -> Option<NodeId> {
    todo!("FindDocType")
}

pub fn find_html(doc: &mut Doc) -> Option<NodeId> {
    todo!("FindHTML")
}

pub fn find_head(doc: &mut Doc) -> Option<NodeId> {
    todo!("FindHEAD")
}

pub fn find_title(doc: &mut Doc) -> Option<NodeId> {
    todo!("FindTITLE")
}

pub fn find_body(doc: &mut Doc) -> Option<NodeId> {
    todo!("FindBody")
}

pub fn find_xml_decl(doc: &mut Doc) -> Option<NodeId> {
    todo!("FindXmlDecl")
}

/// FindContainer(): returns containing block element, if any
pub fn find_container(doc: &mut Doc, node: Option<NodeId>) -> Option<NodeId> {
    todo!("FindContainer")
}

/// AddGenerator(): add meta element for Tidy
pub fn add_generator(doc: &mut Doc) -> bool {
    todo!("AddGenerator")
}

pub fn apparent_version(doc: &mut Doc) -> u32 {
    todo!("ApparentVersion")
}

pub fn html_version_name_from_code(vers: u32, is_xhtml: bool) -> Option<&'static str> {
    todo!("HTMLVersionNameFromCode")
}

pub fn html_version_number_from_code(vers: u32) -> u32 {
    todo!("HTMLVersionNumberFromCode")
}

pub fn warn_missing_si_in_emitted_doc_type(doc: &mut Doc) -> bool {
    todo!("WarnMissingSIInEmittedDocType")
}

pub fn set_xhtml_doc_type(doc: &mut Doc) -> bool {
    todo!("SetXHTMLDocType")
}

/// FixDocType(): fixup doctype if missing
pub fn fix_doc_type(doc: &mut Doc) -> bool {
    todo!("FixDocType")
}

/// FixXmlDecl(): ensure XML document starts with <?xml version="1.0"?>
pub fn fix_xml_decl(doc: &mut Doc) -> bool {
    todo!("FixXmlDecl")
}

pub fn inferred_tag(doc: &mut Doc, id: TagId) -> NodeId {
    todo!("InferredTag")
}

pub fn unget_token(doc: &mut Doc) {
    todo!("UngetToken")
}

pub fn get_token(doc: &mut Doc, mode: GetTokenMode) -> Option<NodeId> {
    todo!("GetToken")
}

/// NewAttribute(): create a new attribute
pub fn new_attribute(doc: &mut Doc) -> AttId {
    todo!("NewAttribute")
}

/// NewAttributeEx(): create a new attribute with given name and value
pub fn new_attribute_ex(doc: &mut Doc, name: &[u8], value: Option<&[u8]>, delim: u32) -> AttId {
    todo!("NewAttributeEx")
}

/// InsertAttributeAtEnd()
pub fn insert_attribute_at_end(doc: &mut Doc, node: NodeId, av: AttId) {
    todo!("InsertAttributeAtEnd")
}

/// InsertAttributeAtStart()
pub fn insert_attribute_at_start(doc: &mut Doc, node: NodeId, av: AttId) {
    todo!("InsertAttributeAtStart")
}
