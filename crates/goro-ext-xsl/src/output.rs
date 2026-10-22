//! Result serialization (xsltutils.c xsltSaveResultTo & co.)

use crate::style::Sheet;
use crate::util::*;
use goro_core::vm::Vm;
use goro_ext_xml::serialize::{self, SaveCtx};
use goro_ext_xml::tree::{NType, NodeId};

/// Encode a UTF-8 string with the output buffer encoder for `encoding`
/// (characters it cannot represent become character references).
fn encode(s: &str, encoding: Option<&str>) -> Vec<u8> {
    match encoding {
        None => s.as_bytes().to_vec(),
        Some(e) if e.eq_ignore_ascii_case("UTF-8") || e.eq_ignore_ascii_case("UTF8") => {
            s.as_bytes().to_vec()
        }
        Some(e) => serialize::encode_output(s, Some(e)),
    }
}

/// The output properties used for serialization.
#[derive(Debug, Clone, Default)]
pub struct OutProps {
    pub method: Option<String>,
    pub method_uri: Option<String>,
    pub encoding: Option<String>,
    pub indent: i32,
    pub omit_xml_declaration: i32,
    pub standalone: i32,
}

impl OutProps {
    /// XSLT_GET_IMPORT_* over the compiled stylesheet
    pub fn from_sheet(sheet: &Sheet) -> Self {
        OutProps {
            method: sheet.get_import_str(|s| s.method.as_ref()),
            method_uri: sheet.styles[0].method_uri.clone(),
            encoding: sheet.get_import_str(|s| s.encoding.as_ref()),
            indent: sheet.get_import_int(|s| s.indent),
            omit_xml_declaration: sheet.get_import_int(|s| s.omit_xml_declaration),
            standalone: sheet.get_import_int(|s| s.standalone),
        }
    }
}

/// xsltSaveResultTo into a byte buffer. Err(()) for the -1 return.
pub fn save_result(vm: &mut Vm, sheet: &Sheet, result: NodeId) -> Result<Vec<u8>, ()> {
    let mut props = OutProps::from_sheet(sheet);
    props.method_uri = if sheet.styles[0].method.as_deref() == Some("xhtml") {
        None
    } else {
        sheet.styles[0].method_uri.clone()
    };
    save_result_props(vm, &props, result)
}

pub fn save_result_props(vm: &mut Vm, props: &OutProps, result: NodeId) -> Result<Vec<u8>, ()> {
    let (no_children, only_dtd, rtype) = tw(|t| {
        let ch = t.nodes[result].children;
        let only_dtd = ch
            .map(|c| t.nodes[c].t == NType::Dtd && t.nodes[c].next.is_none())
            .unwrap_or(false);
        (ch.is_none(), only_dtd, t.nodes[result].t)
    });
    if no_children || only_dtd {
        return Ok(Vec::new());
    }
    if props.method_uri.is_some() {
        php_error(vm, "xsltSaveResultTo : unknown output method\n");
        return Err(());
    }
    let mut method = props.method.clone();
    let encoding = props.encoding.clone();
    let mut indent = props.indent;
    if method.is_none() && rtype == NType::HtmlDocument {
        method = Some("html".into());
    }
    match method.as_deref() {
        Some("html") => {
            tw(|t| {
                goro_ext_xml::html::save::set_meta_encoding(
                    t,
                    result,
                    Some(encoding.as_deref().unwrap_or("UTF-8")),
                )
            });
            if indent == -1 {
                indent = 1;
            }
            let s =
                tw(|t| goro_ext_xml::html::save::doc_content_dump_string(t, result, indent == 1));
            Ok(encode_html(&s, encoding.as_deref()))
        }
        Some("xhtml") => {
            tw(|t| {
                goro_ext_xml::html::save::set_meta_encoding(
                    t,
                    result,
                    Some(encoding.as_deref().unwrap_or("UTF-8")),
                )
            });
            let s = tw(|t| goro_ext_xml::html::save::doc_content_dump_string(t, result, false));
            Ok(encode_html(&s, encoding.as_deref()))
        }
        Some("text") => {
            let s = tw(|t| {
                let mut out = String::new();
                let mut cur = t.nodes[result].children;
                while let Some(c) = cur {
                    if t.nodes[c].t == NType::Text {
                        out.push_str(&t.nodes[c].content);
                    }
                    if let Some(ch) = t.nodes[c].children {
                        if !matches!(
                            t.nodes[ch].t,
                            NType::EntityDecl | NType::EntityRef | NType::Entity
                        ) {
                            cur = Some(ch);
                            continue;
                        }
                    }
                    if let Some(n) = t.nodes[c].next {
                        cur = Some(n);
                        continue;
                    }
                    let mut x = c;
                    cur = None;
                    loop {
                        match t.nodes[x].parent {
                            None => break,
                            Some(p) => {
                                if p == result {
                                    break;
                                }
                                x = p;
                                if let Some(n) = t.nodes[p].next {
                                    cur = Some(n);
                                    break;
                                }
                            }
                        }
                    }
                }
                out
            });
            Ok(encode(&s, encoding.as_deref()))
        }
        _ => {
            let omit = props.omit_xml_declaration;
            let standalone = props.standalone;
            let mut out = String::new();
            let (version, doc_enc) = tw(|t| {
                let i = t.doc_info(result);
                (
                    i.and_then(|i| i.version.clone()),
                    i.and_then(|i| i.encoding.clone()),
                )
            });
            let mut enc = encoding.clone();
            if omit != 1 {
                out.push_str("<?xml version=");
                match &version {
                    Some(v) => {
                        out.push('"');
                        out.push_str(v);
                        out.push('"');
                    }
                    None => out.push_str("\"1.0\""),
                }
                if enc.is_none() {
                    enc = doc_enc;
                }
                if let Some(e) = &enc {
                    out.push_str(" encoding=\"");
                    out.push_str(e);
                    out.push('"');
                }
                match standalone {
                    0 => out.push_str(" standalone=\"no\""),
                    1 => out.push_str(" standalone=\"yes\""),
                    _ => {}
                }
                out.push_str("?>\n");
            }
            let children = tw(|t| t.children_of(result));
            let dump_enc = enc.clone().or_else(|| encoding.clone());
            tw(|t| {
                for &child in &children {
                    let mut ctx =
                        SaveCtx::new(Some(dump_enc.as_deref().unwrap_or("UTF-8")), indent == 1);
                    ctx.level = 0;
                    serialize::dump_node(t, &mut ctx, child);
                    out.push_str(&ctx.out);
                    let ct = t.nodes[child].t;
                    if indent != 0
                        && (ct == NType::Dtd
                            || (ct == NType::Comment && t.nodes[child].next.is_some()))
                    {
                        out.push('\n');
                    }
                }
            });
            if indent != 0 {
                out.push('\n');
            }
            Ok(encode(&out, encoding.as_deref()))
        }
    }
}

fn encode_html(s: &str, encoding: Option<&str>) -> Vec<u8> {
    match encoding {
        None => s.as_bytes().to_vec(),
        Some(e) => {
            let oe = goro_ext_xml::html::save::find_output_encoder(Some(e));
            goro_ext_xml::html::save::encode_output(s, &oe)
        }
    }
}
