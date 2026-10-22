//! pprint.c: the pretty printer (indentation, wrapping, entities), writing
//! to `doc.doc_out`.
#![allow(unused_variables)]

use crate::doc::*;

pub const NORMAL: u32 = 0;
pub const PREFORMATTED: u32 = 1;
pub const COMMENT: u32 = 2;
pub const ATTRIBVALUE: u32 = 4;
pub const NOWRAP: u32 = 8;
pub const CDATA: u32 = 16;

/// TidyIndent
#[derive(Clone, Copy, Debug, Default)]
pub struct TidyIndent {
    pub spaces: i32,
    pub attr_val_start: i32,
    pub attr_string_start: i32,
}

/// TidyPrintImpl
#[derive(Clone, Debug, Default)]
pub struct PrintImpl {
    /// `linebuf` (`lbufsize` is its capacity, `linelen` the used length)
    pub linebuf: Vec<u32>,
    pub linelen: u32,
    pub wraphere: u32,
    pub line: u32,
    pub ix_ind: u32,
    pub indent: [TidyIndent; 2],
}

/// InitPrintBuf()
pub fn init_print_buf(doc: &mut Doc) {
    todo!("InitPrintBuf")
}

/// FreePrintBuf()
pub fn free_print_buf(doc: &mut Doc) {
    todo!("FreePrintBuf")
}

/// PFlushLine()
pub fn pflush_line(doc: &mut Doc, indent: u32) {
    todo!("PFlushLine")
}

/// PFlushLineSmart()
pub fn pflush_line_smart(doc: &mut Doc, indent: u32) {
    todo!("PFlushLineSmart")
}

/// PrintBody(): print just the content of the body element
pub fn print_body(doc: &mut Doc) {
    todo!("PrintBody")
}

/// PPrintTree()
pub fn pprint_tree(doc: &mut Doc, mode: u32, indent: u32, node: Option<NodeId>) {
    todo!("PPrintTree")
}

/// PPrintXMLTree()
pub fn pprint_xml_tree(doc: &mut Doc, mode: u32, indent: u32, node: Option<NodeId>) {
    todo!("PPrintXMLTree")
}

/// TextNodeEndWithSpace()
pub fn text_node_end_with_space(doc: &mut Doc, node: Option<NodeId>) -> bool {
    todo!("TextNodeEndWithSpace")
}

/// PPrintTabs(): indent with tabs (process-wide, like tidy's static)
pub fn pprint_tabs() {
    todo!("PPrintTabs")
}

/// PPrintSpaces(): indent with spaces (process-wide, like tidy's static)
pub fn pprint_spaces() {
    todo!("PPrintSpaces")
}
