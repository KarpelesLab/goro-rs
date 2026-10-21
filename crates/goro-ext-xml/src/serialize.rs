//! XML serialization (libxml2 xmlsave.c behaviour, 2.13 flavour).

use crate::tree::{Extra, NType, NodeId, Tree};

pub const SAVE_FORMAT: u32 = 1;
pub const SAVE_NO_DECL: u32 = 2;
pub const SAVE_NO_EMPTY: u32 = 4;

pub struct SaveCtx {
    pub out: String,
    /// Output encoding (None = escape non-ASCII as char refs).
    pub encoding: Option<String>,
    pub format: bool,
    pub level: i32,
    pub no_empty: bool,
    pub indent: String,
}

impl SaveCtx {
    pub fn new(encoding: Option<&str>, format: bool) -> Self {
        SaveCtx {
            out: String::new(),
            encoding: encoding.map(|s| s.to_string()),
            format,
            level: 0,
            no_empty: false,
            indent: "  ".to_string(),
        }
    }

    fn write_indent(&mut self) {
        let level = self.level.min(60).max(0);
        for _ in 0..level {
            let i = self.indent.clone();
            self.out.push_str(&i);
        }
    }
}

fn hex_ref(out: &mut String, v: u32) {
    out.push_str(&format!("&#x{:X};", v));
}

/// Escape text content.
pub fn escape_text(out: &mut String, s: &str, no_encoding: bool) {
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '\r' => {
                if no_encoding {
                    out.push_str("&#xD;")
                } else {
                    out.push_str("&#13;")
                }
            }
            '\n' | '\t' => out.push(c),
            c if (c as u32) < 0x20 => {
                if no_encoding {
                    out.push_str("&#xFFFD;")
                } else {
                    out.push(c)
                }
            }
            c if (c as u32) >= 0x80 && no_encoding => hex_ref(out, c as u32),
            c => out.push(c),
        }
    }
}

/// Escape an attribute value (xmlBufAttrSerializeTxtContent).
pub fn escape_attr(out: &mut String, s: &str, no_encoding: bool) {
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c if (c as u32) >= 0x80 && no_encoding => hex_ref(out, c as u32),
            c => out.push(c),
        }
    }
}

/// xmlBufWriteQuotedString
fn quoted(out: &mut String, s: &str) {
    if s.contains('"') {
        if s.contains('\'') {
            out.push('"');
            out.push_str(&s.replace('"', "&quot;"));
            out.push('"');
        } else {
            out.push('\'');
            out.push_str(s);
            out.push('\'');
        }
    } else {
        out.push('"');
        out.push_str(s);
        out.push('"');
    }
}

fn write_qname(t: &Tree, out: &mut String, id: NodeId) {
    if let Some(p) = t.ns_prefix(t.nodes[id].ns) {
        out.push_str(p);
        out.push(':');
    }
    out.push_str(&t.nodes[id].name);
}

fn write_ns_list(t: &Tree, ctx: &mut SaveCtx, id: NodeId) {
    let no_enc = ctx.encoding.is_none();
    for &ns in &t.nodes[id].ns_def {
        let n = &t.nss[ns];
        if n.prefix.as_deref() == Some("xml") {
            continue;
        }
        ctx.out.push(' ');
        match &n.prefix {
            Some(p) => {
                ctx.out.push_str("xmlns:");
                ctx.out.push_str(p);
            }
            None => ctx.out.push_str("xmlns"),
        }
        ctx.out.push_str("=\"");
        escape_attr(&mut ctx.out, &n.href, no_enc);
        ctx.out.push('"');
    }
}

pub fn write_attr(t: &Tree, ctx: &mut SaveCtx, a: NodeId) {
    ctx.out.push(' ');
    write_qname(t, &mut ctx.out, a);
    ctx.out.push_str("=\"");
    write_attr_content(t, ctx, a);
    ctx.out.push('"');
}

fn write_attr_content(t: &Tree, ctx: &mut SaveCtx, a: NodeId) {
    let no_enc = ctx.encoding.is_none();
    let mut c = t.nodes[a].children;
    while let Some(x) = c {
        match t.nodes[x].t {
            NType::Text | NType::CData => escape_attr(&mut ctx.out, &t.nodes[x].content, no_enc),
            NType::EntityRef => {
                ctx.out.push('&');
                ctx.out.push_str(&t.nodes[x].name);
                ctx.out.push(';');
            }
            _ => {}
        }
        c = t.nodes[x].next;
    }
}

pub fn dump_dtd(t: &Tree, ctx: &mut SaveCtx, dtd: NodeId) {
    let info = t.dtd_info(dtd).cloned().unwrap_or_default();
    ctx.out.push_str("<!DOCTYPE ");
    ctx.out.push_str(&t.nodes[dtd].name);
    if let Some(e) = &info.external_id {
        ctx.out.push_str(" PUBLIC ");
        quoted(&mut ctx.out, e);
        ctx.out.push(' ');
        quoted(&mut ctx.out, info.system_id.as_deref().unwrap_or(""));
    } else if let Some(s) = &info.system_id {
        ctx.out.push_str(" SYSTEM ");
        quoted(&mut ctx.out, s);
    }
    let has_decls = !info.entities.is_empty()
        || !info.pentities.is_empty()
        || !info.notations.is_empty()
        || t.children_of(dtd).iter().any(|c| {
            matches!(
                t.nodes[*c].t,
                NType::ElementDecl | NType::AttributeDecl | NType::EntityDecl
            )
        });
    if !has_decls {
        ctx.out.push('>');
        return;
    }
    ctx.out.push_str(" [\n");
    let is_int =
        t.nodes[dtd].doc.and_then(|d| t.int_subset(d)) == Some(dtd) || t.nodes[dtd].doc.is_none();
    if is_int {
        for (name, public, system) in &info.notations {
            ctx.out.push_str("<!NOTATION ");
            ctx.out.push_str(name);
            if let Some(p) = public {
                ctx.out.push_str(" PUBLIC ");
                quoted(&mut ctx.out, p);
                if let Some(s) = system {
                    ctx.out.push(' ');
                    quoted(&mut ctx.out, s);
                }
            } else {
                ctx.out.push_str(" SYSTEM ");
                quoted(&mut ctx.out, system.as_deref().unwrap_or(""));
            }
            ctx.out.push_str(" >\n");
        }
    }
    let (fmt, lvl) = (ctx.format, ctx.level);
    ctx.format = false;
    ctx.level = -1;
    for c in t.children_of(dtd) {
        dump_node(t, ctx, c);
    }
    ctx.format = fmt;
    ctx.level = lvl;
    ctx.out.push_str("]>");
}

fn dump_entity_decl(t: &Tree, ctx: &mut SaveCtx, e: NodeId) {
    let info = match t.entity_info(e) {
        Some(i) => i.clone(),
        None => return,
    };
    use crate::tree::EntityType::*;
    if matches!(info.etype, InternalParameter | ExternalParameter) {
        ctx.out.push_str("<!ENTITY % ");
    } else {
        ctx.out.push_str("<!ENTITY ");
    }
    ctx.out.push_str(&t.nodes[e].name);
    ctx.out.push(' ');
    if matches!(
        info.etype,
        ExternalGeneralParsed | ExternalGeneralUnparsed | ExternalParameter
    ) {
        if let Some(p) = &info.external_id {
            ctx.out.push_str("PUBLIC ");
            quoted(&mut ctx.out, p);
            ctx.out.push(' ');
        } else {
            ctx.out.push_str("SYSTEM ");
        }
        quoted(&mut ctx.out, info.system_id.as_deref().unwrap_or(""));
    }
    if info.etype == ExternalGeneralUnparsed {
        if let Some(n) = &info.notation {
            ctx.out.push_str(" NDATA ");
            ctx.out.push_str(n);
        }
    }
    if matches!(info.etype, InternalGeneral | InternalParameter) {
        if let Some(o) = &info.orig {
            quoted(&mut ctx.out, o);
        } else {
            ctx.out.push('"');
            for c in t.nodes[e].content.chars() {
                match c {
                    '"' => ctx.out.push_str("&quot;"),
                    '%' => ctx.out.push_str("&#x25;"),
                    c => ctx.out.push(c),
                }
            }
            ctx.out.push('"');
        }
    }
    ctx.out.push_str(">\n");
}

/// xmlNodeDumpOutputInternal (recursive form)
pub fn dump_node(t: &Tree, ctx: &mut SaveCtx, id: NodeId) {
    let n = &t.nodes[id];
    match n.t {
        NType::Document | NType::HtmlDocument => {
            dump_doc(t, ctx, id, ctx.encoding.clone().as_deref(), false);
        }
        NType::Dtd => dump_dtd(t, ctx, id),
        NType::DocumentFrag => {
            for c in t.children_of(id) {
                dump_node(t, ctx, c);
            }
        }
        NType::ElementDecl => {
            ctx.out.push_str("<!ELEMENT ");
            ctx.out.push_str(&n.name);
            ctx.out.push(' ');
            if let Extra::ElementDecl(c) = &n.extra {
                ctx.out.push_str(c);
            }
            ctx.out.push_str(">\n");
        }
        NType::AttributeDecl => {
            if let Extra::AttributeDecl(ad) = &n.extra {
                ctx.out.push_str("<!ATTLIST ");
                ctx.out.push_str(&ad.elem);
                ctx.out.push(' ');
                if let Some(p) = &ad.prefix {
                    ctx.out.push_str(p);
                    ctx.out.push(':');
                }
                ctx.out.push_str(&ad.name);
                ctx.out.push(' ');
                ctx.out.push_str(&ad.atype);
                if !ad.def.is_empty() {
                    ctx.out.push(' ');
                    ctx.out.push_str(&ad.def);
                }
                if let Some(d) = &ad.default {
                    ctx.out.push_str(" \"");
                    let no_enc = ctx.encoding.is_none();
                    escape_attr(&mut ctx.out, d, no_enc);
                    ctx.out.push('"');
                }
                ctx.out.push_str(">\n");
            }
        }
        NType::EntityDecl | NType::Entity => dump_entity_decl(t, ctx, id),
        NType::Element => dump_element(t, ctx, id, true),
        NType::Text => {
            let no_enc = ctx.encoding.is_none();
            if n.flags & 1 != 0 {
                ctx.out.push_str(&n.content);
            } else {
                escape_text(&mut ctx.out, &n.content, no_enc);
            }
        }
        NType::Pi => {
            ctx.out.push_str("<?");
            ctx.out.push_str(&n.name);
            if !n.content.is_empty() {
                ctx.out.push(' ');
                ctx.out.push_str(&n.content);
            }
            ctx.out.push_str("?>");
        }
        NType::Comment => {
            ctx.out.push_str("<!--");
            ctx.out.push_str(&n.content);
            ctx.out.push_str("-->");
        }
        NType::EntityRef => {
            ctx.out.push('&');
            ctx.out.push_str(&n.name);
            ctx.out.push(';');
        }
        NType::CData => {
            if n.content.is_empty() {
                ctx.out.push_str("<![CDATA[]]>");
            } else {
                let s = &n.content;
                let b = s.as_bytes();
                let mut start = 0;
                let mut i = 0;
                while i < b.len() {
                    if b[i] == b']' && i + 2 < b.len() && b[i + 1] == b']' && b[i + 2] == b'>' {
                        let end = i + 2;
                        ctx.out.push_str("<![CDATA[");
                        ctx.out.push_str(&s[start..end]);
                        ctx.out.push_str("]]>");
                        start = end;
                        i = end;
                        continue;
                    }
                    i += 1;
                }
                if start < b.len() {
                    ctx.out.push_str("<![CDATA[");
                    ctx.out.push_str(&s[start..]);
                    ctx.out.push_str("]]>");
                }
            }
        }
        NType::Attribute => write_attr(t, ctx, id),
        _ => {}
    }
}

fn dump_element(t: &Tree, ctx: &mut SaveCtx, id: NodeId, _root: bool) {
    let n = &t.nodes[id];
    ctx.out.push('<');
    write_qname(t, &mut ctx.out, id);
    write_ns_list(t, ctx, id);
    let mut a = n.properties;
    while let Some(x) = a {
        write_attr(t, ctx, x);
        a = t.nodes[x].next;
    }
    if n.children.is_none() {
        if !ctx.no_empty {
            ctx.out.push_str("/>");
        } else {
            ctx.out.push_str("></");
            write_qname(t, &mut ctx.out, id);
            ctx.out.push('>');
        }
        return;
    }
    let saved_format = ctx.format;
    if ctx.format {
        let mut c = n.children;
        while let Some(x) = c {
            if matches!(t.nodes[x].t, NType::Text | NType::CData | NType::EntityRef) {
                ctx.format = false;
                break;
            }
            c = t.nodes[x].next;
        }
    }
    ctx.out.push('>');
    if ctx.format {
        ctx.out.push('\n');
    }
    if ctx.level >= 0 {
        ctx.level += 1;
    }
    let mut c = n.children;
    while let Some(x) = c {
        if ctx.format && matches!(t.nodes[x].t, NType::Element | NType::Pi | NType::Comment) {
            ctx.write_indent();
        }
        dump_node(t, ctx, x);
        if ctx.format {
            ctx.out.push('\n');
        }
        c = t.nodes[x].next;
    }
    if ctx.level > 0 {
        ctx.level -= 1;
    }
    if ctx.format {
        ctx.write_indent();
    }
    ctx.out.push_str("</");
    write_qname(t, &mut ctx.out, id);
    ctx.out.push('>');
    ctx.format = saved_format;
}

/// xmlSaveDocInternal for XML documents.
pub fn dump_doc(t: &Tree, ctx: &mut SaveCtx, doc: NodeId, encoding: Option<&str>, no_decl: bool) {
    let info = t.doc_info(doc).cloned().unwrap_or_default();
    let encoding: Option<String> = encoding
        .map(|s| s.to_string())
        .or_else(|| info.encoding.clone());
    let saved_enc = ctx.encoding.clone();
    if encoding.is_some() {
        ctx.encoding = encoding.clone();
    }
    if !no_decl {
        ctx.out.push_str("<?xml version=\"");
        ctx.out.push_str(info.version.as_deref().unwrap_or("1.0"));
        ctx.out.push('"');
        if let Some(e) = &encoding {
            ctx.out.push_str(" encoding=\"");
            ctx.out.push_str(e);
            ctx.out.push('"');
        }
        match info.standalone {
            0 => ctx.out.push_str(" standalone=\"no\""),
            1 => ctx.out.push_str(" standalone=\"yes\""),
            _ => {}
        }
        ctx.out.push_str("?>\n");
    }
    for c in t.children_of(doc) {
        ctx.level = 0;
        dump_node(t, ctx, c);
        ctx.out.push('\n');
    }
    ctx.encoding = saved_enc;
}

/// Serialize a whole document to bytes (xmlDocDumpFormatMemoryEnc).
pub fn doc_to_bytes(
    t: &Tree,
    doc: NodeId,
    encoding: Option<&str>,
    format: bool,
    no_decl: bool,
    no_empty: bool,
) -> Vec<u8> {
    let enc = encoding
        .map(|s| s.to_string())
        .or_else(|| t.doc_info(doc).and_then(|d| d.encoding.clone()));
    let mut ctx = SaveCtx::new(enc.as_deref(), format);
    ctx.no_empty = no_empty;
    dump_doc(t, &mut ctx, doc, enc.as_deref(), no_decl);
    encode_output(&ctx.out, enc.as_deref())
}

/// Serialize a node (xmlNodeDumpOutput with doc encoding).
pub fn node_to_bytes(
    t: &Tree,
    node: NodeId,
    encoding: Option<&str>,
    format: bool,
    no_empty: bool,
) -> Vec<u8> {
    let mut ctx = SaveCtx::new(encoding, format);
    ctx.no_empty = no_empty;
    dump_node(t, &mut ctx, node);
    encode_output(&ctx.out, encoding)
}

pub fn encode_output(s: &str, enc: Option<&str>) -> Vec<u8> {
    match enc {
        None => s.as_bytes().to_vec(),
        Some(e) => crate::parser::encode_from_utf8(s, e).unwrap_or_else(|| s.as_bytes().to_vec()),
    }
}
