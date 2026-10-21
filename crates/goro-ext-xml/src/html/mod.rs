//! libxml2-compatible HTML support: the legacy (2.13) HTML parser and the
//! HTMLtree.c serializer, as used by DOMDocument::loadHTML/saveHTML.

pub mod parser;
pub mod save;
pub mod tables;

#[cfg(test)]
mod tests;

use crate::sax2::ParseOutcome;
use crate::tree::{self, NType, NodeId, Tree};

pub use parser::{ElemDesc, FROM_HTML, is_boolean_attr, tag_lookup};

/// Parse an HTML document (htmlCreateMemoryParserCtxt / htmlCreateFileParserCtxt
/// + htmlCtxtUseOptions + htmlParseDocument, the way PHP's DOM drives it).
///
/// * `url`: the input file name (used as the document URL and as the file
///   in error reports); None for in-memory strings.
/// * `options`: LIBXML_* / HTML_PARSE_* flags. As in PHP, zero options means
///   htmlCtxtUseOptions is not called (so node line numbers stay 0).
/// * `encoding`: force the input encoding (otherwise BOM / meta / ISO-8859-1
///   fallback detection applies).
///
/// `well_formed` is libxml2's ctxt->wellFormed (only fatal errors such as
/// encoding errors clear it); the document is returned regardless.
pub fn parse_html(
    data: &[u8],
    url: Option<&str>,
    options: i64,
    encoding: Option<&str>,
) -> ParseOutcome {
    tree::with(|t| parse_html_in(t, data, url, options, encoding))
}

/// Same as [`parse_html`] with an explicit tree (for callers already inside
/// `tree::with`).
pub fn parse_html_in(
    t: &mut Tree,
    data: &[u8],
    url: Option<&str>,
    options: i64,
    encoding: Option<&str>,
) -> ParseOutcome {
    let mut p = parser::HtmlParser::new(t, data, url, options, encoding);
    let wf = p.parse_document();
    let errors = std::mem::take(&mut p.errors);
    ParseOutcome {
        doc: p.doc,
        well_formed: wf,
        ns_well_formed: true,
        errors,
    }
}

/// Whether libxml2 had no error channel for `e` because of
/// LIBXML_NOERROR / LIBXML_NOWARNING. Such errors are still delivered to
/// a global structured handler, i.e. PHP records them for
/// libxml_get_errors() when libxml_use_internal_errors(true) is on, but
/// never emits a warning for them.
pub fn error_is_silenced(e: &crate::parser::XmlError, options: i64) -> bool {
    options & parser::HTML_PARSE_NOERROR != 0
        || (e.level == crate::parser::ERR_WARNING && options & parser::HTML_PARSE_NOWARNING != 0)
}

/// htmlDocDumpMemoryFormat: what DOMDocument::saveHTML() returns.
///
/// `encoding` None selects the output encoding like libxml2 does (the
/// charset of a `<meta http-equiv="Content-Type">`, else the "HTML" encoder
/// that writes non-ASCII characters as entities); Some(name) forces one
/// ("UTF-8" for raw output, "HTML" for the entity encoder).
pub fn html_doc_to_bytes(t: &Tree, doc: NodeId, encoding: Option<&str>, format: bool) -> Vec<u8> {
    save::doc_dump_memory(t, doc, encoding, format)
}

/// htmlNodeDumpFormatOutput of one node, as DOMDocument::saveHTML($node)
/// uses it (PHP writes into an output buffer without encoder, so the
/// result is UTF-8 unless `encoding` names one). A document fragment dumps
/// its children one after the other, like PHP does.
pub fn html_node_to_bytes(t: &Tree, node: NodeId, encoding: Option<&str>, format: bool) -> Vec<u8> {
    let doc = t.nodes[node].doc;
    let mut s = String::new();
    if t.nodes[node].t == NType::DocumentFrag {
        let mut c = t.nodes[node].children;
        while let Some(x) = c {
            s.push_str(&save::node_dump_string(t, doc, x, format));
            c = t.nodes[x].next;
        }
    } else {
        s = save::node_dump_string(t, doc, node, format);
    }
    let enc = match encoding {
        None => save::OutEnc::Raw,
        Some(e) => save::find_output_encoder(Some(e)),
    };
    save::encode_output(&s, &enc)
}

/// htmlGetMetaEncoding
pub fn html_get_meta_encoding(t: &Tree, doc: NodeId) -> Option<String> {
    save::get_meta_encoding(t, doc)
}

/// htmlSetMetaEncoding
pub fn html_set_meta_encoding(t: &mut Tree, doc: NodeId, encoding: Option<&str>) -> bool {
    save::set_meta_encoding(t, doc, encoding)
}

/// htmlSaveFileFormat(file, doc, htmlGetMetaEncoding(doc), format) as used by
/// DOMDocument::saveHTMLFile: may add/update the Content-Type meta element,
/// returns the bytes to write.
pub fn html_save_file_bytes(t: &mut Tree, doc: NodeId, format: bool) -> Vec<u8> {
    save::save_file_format(t, doc, format)
}
