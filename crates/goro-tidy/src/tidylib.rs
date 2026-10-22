//! tidylib.c: the public API (document creation, parse, clean and repair,
//! diagnostics, save, the node and option accessors).

use crate::attrs;
use crate::clean;
use crate::config::{self, OptionImpl, OptionType};
use crate::doc::*;
use crate::enums::code::*;
use crate::enums::{AttrId, OptId, TagId};
use crate::lexer;
use crate::message;
use crate::parser;
use crate::pprint::{self, NORMAL};
use crate::streamio::{self, StreamOut};
use crate::tags;

const EINVAL: i32 = 22;

impl Doc {
    /// tidyCreate() / tidyDocCreate()
    pub fn new() -> Doc {
        let mut doc = Doc {
            nodes: Vec::new(),
            attvals: Vec::new(),
            root: NodeId(0),
            lexer: None,
            config: Default::default(),
            tags: Default::default(),
            attribs: Default::default(),
            access: Default::default(),
            muted: Vec::new(),
            pprint: Default::default(),
            doc_in: None,
            doc_out: None,
            errout: StreamOut::stderr(),
            option_errors: 0,
            errors: 0,
            warnings: 0,
            access_errors: 0,
            info_messages: 0,
            doc_errors: 0,
            parse_status: 0,
            bad_access: 0,
            bad_layout: 0,
            bad_chars: 0,
            bad_form: 0,
            footnotes: 0,
            html5_mode: false,
            xml_detected: false,
            n_class_id: 0,
            input_had_bom: false,
            given_doctype: None,
            generation: 0,
        };
        // the root node (`doc->root`), cleared
        doc.nodes.push(Node::default());
        tags::init_tags(&mut doc);
        attrs::init_attrs(&mut doc);
        config::init_config(&mut doc);
        pprint::init_print_buf(&mut doc);
        doc
    }
}

impl Default for Doc {
    fn default() -> Self {
        Doc::new()
    }
}

/// tidyLoadConfig()
pub fn load_config(doc: &mut Doc, cfgfil: &[u8]) -> i32 {
    config::parse_config_file(doc, cfgfil)
}

/// tidyLoadConfigEnc()
pub fn load_config_enc(doc: &mut Doc, cfgfil: &[u8], charenc: &[u8]) -> i32 {
    config::parse_config_file_enc(doc, cfgfil, charenc)
}

/// tidySetCharEncoding()
pub fn set_char_encoding(doc: &mut Doc, encnam: &[u8]) -> i32 {
    let enc = config::char_encoding_id(doc, encnam);
    if enc >= 0 && config::adjust_char_encoding(doc, enc) {
        return 0;
    }
    message::report_bad_argument(doc, b"char-encoding");
    -EINVAL
}

/// tidySetInCharEncoding()
pub fn set_in_char_encoding(doc: &mut Doc, encnam: &[u8]) -> i32 {
    let enc = config::char_encoding_id(doc, encnam);
    if enc >= 0 && config::set_option_int(doc, OptId::InCharEncoding, enc as u64) {
        return 0;
    }
    message::report_bad_argument(doc, b"in-char-encoding");
    -EINVAL
}

/// tidySetOutCharEncoding()
pub fn set_out_char_encoding(doc: &mut Doc, encnam: &[u8]) -> i32 {
    let enc = config::char_encoding_id(doc, encnam);
    if enc >= 0 && config::set_option_int(doc, OptId::OutCharEncoding, enc as u64) {
        return 0;
    }
    message::report_bad_argument(doc, b"out-char-encoding");
    -EINVAL
}

/// tidyOptGetIdForName(): None for N_TIDY_OPTIONS
pub fn opt_get_id_for_name(optnam: &[u8]) -> Option<OptId> {
    config::lookup_option(optnam).map(|o| o.id)
}

/// tidyOptGetDefault()
pub fn opt_get_default(option: &'static OptionImpl) -> Option<&'static str> {
    if option.id == OptId::Doctype {
        let newopt = config::get_option(OptId::DoctypeMode);
        return config::get_pick_list_label_for_pick(OptId::DoctypeMode, newopt.dflt as u32);
    }
    if option.type_ == OptionType::String {
        return option.pdflt;
    }
    None
}

/// tidyOptIsReadOnly()
pub fn opt_is_read_only(option: &'static OptionImpl) -> bool {
    option.parser.is_none()
}

/// tidyOptGetValue()
pub fn opt_get_value(doc: &Doc, opt_id: OptId) -> Option<Vec<u8>> {
    if opt_id == OptId::Doctype {
        let pick = opt_get_int(doc, OptId::DoctypeMode);
        if pick as u32 != TIDY_DOCTYPE_USER {
            return config::get_pick_list_label_for_pick(OptId::DoctypeMode, pick as u32)
                .map(|s| s.as_bytes().to_vec());
        }
        return doc.cfg_str(opt_id).map(|s| s.to_vec());
    }
    doc.cfg_str(opt_id).map(|s| s.to_vec())
}

/// tidyOptSetValue()
pub fn opt_set_value(doc: &mut Doc, opt_id: OptId, val: &[u8]) -> bool {
    config::parse_config_value(doc, opt_id, val)
}

/// tidyOptParseValue()
pub fn opt_parse_value(doc: &mut Doc, optnam: &[u8], val: &[u8]) -> bool {
    config::parse_config_option(doc, optnam, val)
}

/// tidyOptGetInt()
pub fn opt_get_int(doc: &Doc, opt_id: OptId) -> u64 {
    if opt_id == OptId::Doctype {
        doc.config.value[OptId::DoctypeMode as usize].v
    } else {
        doc.config.value[opt_id as usize].v
    }
}

/// tidyOptSetInt()
pub fn opt_set_int(doc: &mut Doc, opt_id: OptId, val: u64) -> bool {
    if opt_id == OptId::Doctype {
        config::set_option_int(doc, OptId::DoctypeMode, val)
    } else {
        config::set_option_int(doc, opt_id, val)
    }
}

/// tidyOptGetBool()
pub fn opt_get_bool(doc: &Doc, opt_id: OptId) -> bool {
    doc.cfg_bool(opt_id)
}

/// tidyOptSetBool()
pub fn opt_set_bool(doc: &mut Doc, opt_id: OptId, val: bool) -> bool {
    config::set_option_bool(doc, opt_id, val)
}

/// tidyOptGetEncName()
pub fn opt_get_enc_name(doc: &Doc, opt_id: OptId) -> &'static str {
    config::char_encoding_opt_name(opt_get_int(doc, opt_id) as i32)
}

/// tidyOptGetCurrPick()
pub fn opt_get_curr_pick(doc: &Doc, opt_id: OptId) -> Option<&'static str> {
    config::get_pick_list_label_for_pick(opt_id, opt_get_int(doc, opt_id) as u32)
}

/// tidyOptGetDoc()
pub fn opt_get_doc(opt_id: OptId) -> Option<&'static str> {
    message::opt_get_doc(opt_id)
}

/// tidySetErrorBuffer(): messages go to a fresh buffer (read back with
/// `doc.errout.bytes()`)
pub fn set_error_buffer(doc: &mut Doc) -> i32 {
    let outenc = doc.cfg(OptId::OutCharEncoding);
    let nl = doc.cfg(OptId::Newline);
    doc.errout = StreamOut::buffer(outenc, nl);
    0
}

/// tidyStatus() / tidyDocStatus()
pub fn status(doc: &Doc) -> i32 {
    if doc.errors > 0 {
        return 2;
    }
    if doc.warnings > 0 || doc.access_errors > 0 {
        return 1;
    }
    0
}

/// tidyDetectedHtmlVersion() (the lexer must exist)
pub fn detected_html_version(doc: &Doc) -> u32 {
    lexer::html_version_number_from_code(doc.lxr().version_emitted)
}

/// tidyDetectedXhtml() (the lexer must exist)
pub fn detected_xhtml(doc: &Doc) -> bool {
    doc.lxr().isvoyager
}

/// tidyDetectedGenericXml()
pub fn detected_generic_xml(doc: &Doc) -> bool {
    doc.xml_detected
}

/// tidyErrorSummary()
pub fn error_summary(doc: &mut Doc) {
    message::error_summary(doc);
}

/// tidyGeneralInfo()
pub fn general_info(doc: &mut Doc) {
    message::dialogue(doc, TEXT_GENERAL_INFO, &[]);
    message::dialogue(doc, TEXT_GENERAL_INFO_PLEA, &[]);
}

/// tidyParseBuffer() / tidyParseString(): parse a document from memory
pub fn parse_buffer(doc: &mut Doc, content: Vec<u8>) -> i32 {
    let enc = doc.cfg(OptId::InCharEncoding);
    let input = streamio::buffer_input(content, enc);
    doc_parse_stream(doc, input)
}

/// tidyParseFile(): FILE_CANT_OPEN is reported when the file can't be read
pub fn parse_file(doc: &mut Doc, filnam: &[u8]) -> i32 {
    use std::os::unix::ffi::OsStrExt;
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(filnam));
    match std::fs::read(path) {
        Ok(data) => parse_buffer(doc, data),
        Err(_) => {
            message::report_file_error(doc, filnam, FILE_CANT_OPEN);
            -libc_enoent()
        }
    }
}

fn libc_enoent() -> i32 {
    2
}

/// DocParseStream()
pub fn doc_parse_stream(doc: &mut Doc, input: streamio::StreamIn) -> i32 {
    let xml_in = doc.cfg_bool(OptId::XmlTags);
    doc.doc_in = Some(input);

    tags::reset_tags(doc);
    config::take_config_snapshot(doc);
    config::adjust_config(doc);
    attrs::free_anchors(doc);

    // FreeNode(doc, &doc->root) + TidyClearMemory(&doc->root): a fresh
    // arena whose node 0 is the cleared root
    lexer::free_node(doc, Some(doc.root));
    doc.nodes.clear();
    doc.attvals.clear();
    doc.nodes.push(Node::default());
    doc.generation += 1;

    doc.given_doctype = None;
    lexer::free_lexer(doc);

    let lx = lexer::new_lexer(doc);
    doc.lexer = Some(lx);
    let (l, c) = (doc.lxr().lines, doc.lxr().columns);
    let root = doc.root;
    doc[root].line = l;
    doc[root].column = c;
    doc.input_had_bom = false;
    doc.xml_detected = false;

    let bom_enc = streamio::read_bom_encoding(doc, streamio::InSel::Doc);
    if bom_enc != -1 {
        if let Some(i) = doc.doc_in.as_mut() {
            i.encoding = bom_enc as u32;
        }
        config::set_option_int(doc, OptId::InCharEncoding, bom_enc as u64);
    }

    if xml_in {
        parser::parse_xml_document(doc);
    } else {
        doc.warnings = 0;
        parser::parse_document(doc);
    }
    let root = doc.root;
    if !parser::check_node_integrity(doc, root) {
        panic!("\nPanic - tree has lost its integrity\n");
    }

    doc.doc_in = None;
    status(doc)
}

/// tidyRunDiagnostics()
pub fn run_diagnostics(doc: &mut Doc) -> i32 {
    message::report_markup_version(doc);
    message::report_num_warnings(doc);
    if doc.errors > 0 && !doc.cfg_bool(OptId::ForceOutput) {
        message::dialogue(doc, STRING_NEEDS_INTERVENTION, &[]);
    }
    status(doc)
}

/// tidyReportDoctype()
pub fn report_doctype(doc: &mut Doc) -> i32 {
    message::report_markup_version(doc);
    0
}

const HTML5_REMOVED: [TagId; 12] = [
    TagId::ACRONYM,
    TagId::APPLET,
    TagId::BASEFONT,
    TagId::BIG,
    TagId::CENTER,
    TagId::DIR,
    TagId::FONT,
    TagId::FRAME,
    TagId::FRAMESET,
    TagId::NOFRAMES,
    TagId::STRIKE,
    TagId::TT,
];

const BAD_BODY5_ATTRIBS: [AttrId; 6] = [
    AttrId::BACKGROUND,
    AttrId::BGCOLOR,
    AttrId::TEXT,
    AttrId::LINK,
    AttrId::VLINK,
    AttrId::ALINK,
];

fn node_has_align_attr(doc: &Doc, node: NodeId) -> bool {
    doc.attr_list(node)
        .into_iter()
        .any(|a| doc.attr_is(Some(a), AttrId::ALIGN))
}

/// CheckHTML5()
fn check_html5(doc: &mut Doc, node: Option<NodeId>) {
    let clean = doc.cfg_bool(OptId::MakeClean);
    let already_strict = doc.cfg_bool(OptId::StrictTagsAttr);
    let body = lexer::find_body(doc);
    let warn = true;
    let mut node = node;
    while let Some(n) = node {
        if node_has_align_attr(doc, n) && !already_strict {
            let a = attrs::attr_get_by_id(doc, n, AttrId::ALIGN);
            message::report_attr_error(doc, n, a, MISMATCHED_ATTRIBUTE_WARN);
        }
        let tid = doc.tag_id(Some(n));
        if Some(n) == body {
            if !already_strict {
                for id in BAD_BODY5_ATTRIBS {
                    if let Some(attr) = attrs::attr_get_by_id(doc, n, id) {
                        message::report_attr_error(doc, n, Some(attr), MISMATCHED_ATTRIBUTE_WARN);
                    }
                }
            }
        } else if tid == TagId::ACRONYM {
            if clean {
                parser::coerce_node(doc, n, TagId::ABBR, warn, false);
            } else if !already_strict {
                message::report(doc, Some(n), Some(n), REMOVED_HTML5, &[]);
            }
        } else if tid == TagId::APPLET {
            if clean {
                parser::coerce_node(doc, n, TagId::OBJECT, warn, false);
            } else if !already_strict {
                message::report(doc, Some(n), Some(n), REMOVED_HTML5, &[]);
            }
        } else if tid == TagId::BASEFONT {
            if !already_strict {
                message::report(doc, Some(n), Some(n), REMOVED_HTML5, &[]);
            }
        } else if tid == TagId::BIG {
            if clean {
                clean::add_style_property(doc, n, b"font-size: larger");
                parser::coerce_node(doc, n, TagId::SPAN, warn, false);
            } else if !already_strict {
                message::report(doc, Some(n), Some(n), REMOVED_HTML5, &[]);
            }
        } else if tid == TagId::CENTER
            || tid == TagId::DIR
            || tid == TagId::FONT
            || tid == TagId::FRAME
            || tid == TagId::FRAMESET
            || tid == TagId::NOFRAMES
        {
            if !already_strict {
                message::report(doc, Some(n), Some(n), REMOVED_HTML5, &[]);
            }
        } else if tid == TagId::STRIKE {
            if clean {
                clean::add_style_property(doc, n, b"text-decoration: line-through");
                parser::coerce_node(doc, n, TagId::SPAN, warn, false);
            } else if !already_strict {
                message::report(doc, Some(n), Some(n), REMOVED_HTML5, &[]);
            }
        } else if tid == TagId::TT {
            if clean {
                clean::add_style_property(doc, n, b"font-family: monospace");
                parser::coerce_node(doc, n, TagId::SPAN, warn, false);
            } else if !already_strict {
                message::report(doc, Some(n), Some(n), REMOVED_HTML5, &[]);
            }
        } else if tags::node_is_element(doc, Some(n)) {
            if let Some(t) = doc[n].tag {
                let v = doc.dict_versions(t);
                let id = doc.dict_id(t);
                if ((v & VERS_HTML5) == 0 && (v & VERS_PROPRIETARY) == 0)
                    || HTML5_REMOVED.contains(&id)
                {
                    if !already_strict {
                        message::report(doc, Some(n), Some(n), REMOVED_HTML5, &[]);
                    }
                }
            }
        }
        if let Some(c) = doc[n].content {
            check_html5(doc, Some(c));
        }
        node = doc[n].next;
    }
}

/// CheckHTMLTagsAttribsVersions()
fn check_html_tags_attribs_versions(doc: &mut Doc, node: Option<NodeId>) {
    let version_emitted = doc.lxr().version_emitted;
    let declared = doc.lxr().doctype;
    let version = if version_emitted == 0 {
        declared
    } else {
        version_emitted
    };
    let tag_report_type = if VERS_STRICT & version != 0 {
        ELEMENT_VERS_MISMATCH_ERROR
    } else {
        ELEMENT_VERS_MISMATCH_WARN
    };
    let attr_report_type = if VERS_STRICT & version != 0 {
        MISMATCHED_ATTRIBUTE_ERROR
    } else {
        MISMATCHED_ATTRIBUTE_WARN
    };
    let check_versions = doc.cfg_bool(OptId::StrictTagsAttr);
    let html_is5 = (doc.lxr().doctype & VERS_HTML5) > 0;

    let mut node = node;
    while let Some(n) = node {
        if tags::node_is_element(doc, Some(n)) {
            if let Some(t) = doc[n].tag {
                if !doc.cfg_bool(OptId::XmlTags) {
                    let tv = doc.dict_versions(t);
                    if check_versions && (tv & version) == 0 {
                        message::report(doc, None, Some(n), tag_report_type, &[]);
                    } else if tv & VERS_PROPRIETARY != 0 {
                        let tid = doc.dict_id(t);
                        if !doc.cfg_bool(OptId::MakeClean)
                            || (tid != TagId::NOBR && tid != TagId::WBR)
                        {
                            let tag_looks_custom = tags::node_is_autonomous_custom_format(doc, n);
                            if (html_is5 && !tag_looks_custom) || !html_is5 {
                                message::report(doc, None, Some(n), PROPRIETARY_ELEMENT, &[]);
                            }
                            if tid == TagId::LAYER {
                                doc.bad_layout |= USING_LAYER;
                            } else if tid == TagId::SPACER {
                                doc.bad_layout |= USING_SPACER;
                            } else if tid == TagId::NOBR {
                                doc.bad_layout |= USING_NOBR;
                            }
                        }
                    }
                }
            }
        }
        if tags::node_is_element(doc, Some(n)) {
            let mut attval = doc[n].attributes;
            while let Some(a) = attval {
                let next_attr = doc[a].next;
                let attr_is_proprietary = attrs::attribute_is_proprietary(doc, n, a);
                let attr_is_mismatched = if check_versions || html_is5 {
                    attrs::attribute_is_mismatched(doc, n, a)
                } else {
                    false
                };
                if attr_is_proprietary {
                    if doc.cfg_bool(OptId::WarnPropAttrs) {
                        message::report_attr_error(doc, n, Some(a), PROPRIETARY_ATTRIBUTE);
                    }
                } else if attr_is_mismatched {
                    if html_is5 {
                        message::report_attr_error(
                            doc,
                            n,
                            Some(a),
                            if check_versions {
                                MISMATCHED_ATTRIBUTE_ERROR
                            } else {
                                MISMATCHED_ATTRIBUTE_WARN
                            },
                        );
                    } else {
                        message::report_attr_error(doc, n, Some(a), attr_report_type);
                    }
                }
                if (attr_is_proprietary || attr_is_mismatched) && doc.cfg_bool(OptId::DropPropAttrs)
                {
                    lexer::remove_attribute(doc, n, a);
                }
                attval = next_attr;
            }
        }
        if let Some(c) = doc[n].content {
            check_html_tags_attribs_versions(doc, Some(c));
        }
        node = doc[n].next;
    }
}

/// tidyCleanAndRepair()
pub fn clean_and_repair(doc: &mut Doc) -> i32 {
    let word2k = doc.cfg_bool(OptId::Word2000);
    let logical = doc.cfg_bool(OptId::LogicalEmphasis);
    let clean = doc.cfg_bool(OptId::MakeClean);
    let gdoc = doc.cfg_bool(OptId::GDocClean);
    let html_out = doc.cfg_bool(OptId::HtmlOut);
    let xml_out = doc.cfg_bool(OptId::XmlOut);
    let xhtml_out = doc.cfg_bool(OptId::XhtmlOut);
    let xml_decl = doc.cfg_bool(OptId::XmlDecl);
    let tidy_mark = doc.cfg_bool(OptId::Mark);
    let tidy_xml_tags = doc.cfg_bool(OptId::XmlTags);
    let want_name_attr = doc.cfg_bool(OptId::AnchorAsName);
    let merge_emphasis = doc.cfg_bool(OptId::MergeEmphasis);
    let root = Some(doc.root);

    if tidy_xml_tags {
        return status(doc);
    }

    clean::clean_style(doc, root);
    if merge_emphasis {
        clean::nested_emphasis(doc, root);
    }
    clean::list2bq(doc, root);
    clean::bq2div(doc, root);
    if logical {
        clean::em_from_i(doc, root);
    }
    if word2k && clean::is_word2000(doc) {
        clean::drop_sections(doc, root);
        clean::clean_word2000(doc, root);
        parser::drop_empty_elements(doc, root);
    }
    if clean {
        clean::clean_document(doc);
    }
    if gdoc {
        crate::gdoc::clean_google_document(doc);
    }
    clean::tidy_meta_charset(doc);

    if !parser::check_node_integrity(doc, doc.root) {
        panic!("\nPanic - tree has lost its integrity\n");
    }

    if let Some(node) = lexer::find_doc_type(doc) {
        let fpi = attrs::get_attr_by_name(doc, node, b"PUBLIC");
        if doc.attr_has_value(fpi) {
            doc.given_doctype = fpi.and_then(|a| doc[a].value.clone());
        }
    }

    if doc[doc.root].content.is_some() {
        if html_out && doc.lxr().isvoyager {
            if let Some(node) = lexer::find_doc_type(doc) {
                parser::remove_node(doc, node);
            }
        }
        if xhtml_out && !html_out {
            lexer::set_xhtml_doc_type(doc);
            clean::fix_anchors(doc, root, want_name_attr, true);
            clean::fix_xhtml_namespace(doc, true);
            clean::fix_language_information(doc, root, true, true);
        } else {
            lexer::fix_doc_type(doc);
            clean::fix_anchors(doc, root, want_name_attr, true);
            clean::fix_xhtml_namespace(doc, false);
            clean::fix_language_information(doc, root, false, true);
        }
        if tidy_mark {
            lexer::add_generator(doc);
        }
    }

    if xml_out && xml_decl {
        lexer::fix_xml_decl(doc);
    }

    if doc.lexer.is_some() {
        if doc.lxr().version_emitted & VERS_HTML5 != 0 {
            check_html5(doc, root);
        }
        check_html_tags_attribs_versions(doc, root);
        if !doc.lxr().isvoyager && doc.xml_detected {
            let xd = lexer::find_xml_decl(doc);
            message::report(doc, None, xd, XML_DECLARATION_DETECTED, &[]);
        }
    }

    clean::clean_head(doc);
    status(doc)
}

/// showBodyOnly()
fn show_body_only(doc: &mut Doc, body_only: u32) -> bool {
    match body_only {
        TIDY_NO_STATE => false,
        TIDY_YES_STATE => true,
        _ => {
            let node = lexer::find_body(doc);
            node.is_some_and(|n| doc[n].implicit)
        }
    }
}

/// tidyDocSaveStream()
pub fn save_stream(doc: &mut Doc, out: StreamOut) -> (i32, StreamOut) {
    let show_markup = doc.cfg_bool(OptId::ShowMarkup);
    let force_output = doc.cfg_bool(OptId::ForceOutput);
    let output_bom = doc.cfg_auto_bool(OptId::OutputBOM) == TIDY_YES_STATE;
    let smart_bom = doc.cfg_auto_bool(OptId::OutputBOM) == TIDY_AUTO_STATE;
    let xml_out = doc.cfg_bool(OptId::XmlOut);
    let xhtml_out = doc.cfg_bool(OptId::XhtmlOut);
    let body_only = doc.cfg_auto_bool(OptId::BodyOnly);
    let drop_comments = doc.cfg_bool(OptId::HideComments);
    let make_clean = doc.cfg_bool(OptId::MakeClean);
    let ascii_chars = doc.cfg_bool(OptId::AsciiChars);
    let make_bare = doc.cfg_bool(OptId::MakeBare);
    let escape_cdata = doc.cfg_bool(OptId::EscapeCdata);
    let pp_with_tabs = doc.cfg_bool(OptId::PPrintTabs);
    let sort_attr_strat = doc.cfg(OptId::SortAttributes);
    let root = Some(doc.root);
    let mut out = out;

    if pp_with_tabs {
        pprint::pprint_tabs();
    } else {
        pprint::pprint_spaces();
    }
    if escape_cdata {
        clean::convert_cdata_nodes(doc, root);
    }
    if drop_comments {
        clean::drop_comments(doc, root);
    }
    if make_clean {
        clean::drop_font_elements(doc, root);
    }
    if (make_clean && ascii_chars) || make_bare {
        clean::downgrade_typography(doc, root);
    }
    if make_bare {
        clean::normalize_spaces(doc, root);
    } else {
        clean::replace_preformatted_spaces(doc, root);
    }
    attrs::sort_attributes(doc, root, sort_attr_strat);

    if show_markup && (doc.errors == 0 || force_output) {
        if output_bom || (doc.input_had_bom && smart_bom) {
            streamio::out_bom(&mut out);
        }
        doc.doc_out = Some(out);
        if xml_out && !xhtml_out {
            pprint::pprint_xml_tree(doc, NORMAL, 0, root);
        } else if show_body_only(doc, body_only) {
            pprint::print_body(doc);
        } else {
            pprint::pprint_tree(doc, NORMAL, 0, root);
        }
        pprint::pflush_line(doc, 0);
        out = doc.doc_out.take().expect("tidy: output stream");
    }
    (status(doc), out)
}

/// tidySaveBuffer(): returns the status and the output
pub fn save_buffer(doc: &mut Doc) -> (i32, Vec<u8>) {
    let outenc = doc.cfg(OptId::OutCharEncoding);
    let nl = doc.cfg(OptId::Newline);
    let out = StreamOut::buffer(outenc, nl);
    let (st, out) = save_stream(doc, out);
    (st, out.bytes().to_vec())
}

/// tidyNodeGetText(): the pretty-printed markup of a node
pub fn node_get_text(doc: &mut Doc, node: NodeId) -> Vec<u8> {
    let outenc = doc.cfg(OptId::OutCharEncoding);
    let nl = doc.cfg(OptId::Newline);
    let xml_out = doc.cfg_bool(OptId::XmlOut);
    let xhtml_out = doc.cfg_bool(OptId::XhtmlOut);
    let saved = doc.doc_out.take();
    doc.doc_out = Some(StreamOut::buffer(outenc, nl));
    if xml_out && !xhtml_out {
        pprint::pprint_xml_tree(doc, NORMAL, 0, Some(node));
    } else {
        pprint::pprint_tree(doc, NORMAL, 0, Some(node));
    }
    pprint::pflush_line(doc, 0);
    let out = doc.doc_out.take().expect("tidy: output stream");
    doc.doc_out = saved;
    out.bytes().to_vec()
}

/// tidyNodeGetValue(): the text of text-like nodes
pub fn node_get_value(doc: &Doc, node: NodeId) -> Option<Vec<u8>> {
    match doc[node].type_ {
        NodeType::TextNode
        | NodeType::CDATATag
        | NodeType::CommentTag
        | NodeType::ProcInsTag
        | NodeType::SectionTag
        | NodeType::AspTag
        | NodeType::JsteTag
        | NodeType::PhpTag => Some(doc.node_text(node).to_vec()),
        _ => None,
    }
}

/// tidyNodeIsProp()
pub fn node_is_prop(doc: &Doc, node: NodeId) -> bool {
    match doc[node].type_ {
        NodeType::RootNode
        | NodeType::DocTypeTag
        | NodeType::CommentTag
        | NodeType::XmlDecl
        | NodeType::ProcInsTag
        | NodeType::TextNode
        | NodeType::CDATATag => false,
        NodeType::SectionTag | NodeType::AspTag | NodeType::JsteTag | NodeType::PhpTag => true,
        NodeType::StartTag | NodeType::EndTag | NodeType::StartEndTag => match doc[node].tag {
            Some(t) => (doc.dict_versions(t) & VERS_PROPRIETARY) != 0,
            None => true,
        },
    }
}

/// tidyNodeGetId()
pub fn node_get_id(doc: &Doc, node: NodeId) -> TagId {
    doc.tag_id(Some(node))
}

/// tidyGetHtml()
pub fn get_html(doc: &mut Doc) -> Option<NodeId> {
    lexer::find_html(doc)
}

/// tidyGetHead()
pub fn get_head(doc: &mut Doc) -> Option<NodeId> {
    lexer::find_head(doc)
}

/// tidyGetBody()
pub fn get_body(doc: &mut Doc) -> Option<NodeId> {
    lexer::find_body(doc)
}
