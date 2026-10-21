//! HTML serialization, port of libxml2 2.13's HTMLtree.c
//! (htmlNodeDumpFormatOutput, htmlDocContentDumpFormatOutput,
//! htmlDocDumpMemoryFormat, htmlGetMetaEncoding/htmlSetMetaEncoding).

use super::parser::{entity_value_lookup, is_boolean_attr, tag_lookup};
use crate::tree::{NType, NodeId, Tree};

/// How the serialized (UTF-8) output is converted to bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutEnc {
    /// No encoder: raw UTF-8.
    Raw,
    /// libxml2's "HTML" output encoder: non-ASCII as named or numeric
    /// character references.
    Html,
    /// A named encoding (unrepresentable characters become `&#N;`).
    Named(String),
}

/// htmlFindOutputEncoder
pub fn find_output_encoder(encoding: Option<&str>) -> OutEnc {
    match encoding {
        None => OutEnc::Html,
        Some(e) => {
            if e.eq_ignore_ascii_case("UTF-8") || e.eq_ignore_ascii_case("UTF8") {
                OutEnc::Raw
            } else if e.eq_ignore_ascii_case("HTML") {
                OutEnc::Html
            } else if crate::parser::encoding_supported(e) {
                OutEnc::Named(e.to_string())
            } else {
                // unknown encoding: libxml2 reports an error and writes UTF-8
                OutEnc::Raw
            }
        }
    }
}

/// The name of the handler htmlFindOutputEncoder returns (handler->name),
/// None when there is no handler.
fn handler_name(encoding: Option<&str>) -> Option<String> {
    match encoding {
        None => Some("HTML".to_string()),
        Some(e) => {
            if e.eq_ignore_ascii_case("UTF-8")
                || e.eq_ignore_ascii_case("UTF8")
                || !crate::parser::encoding_supported(e)
            {
                return None;
            }
            const BUILTIN: &[&str] = &[
                "UTF-16LE",
                "UTF-16BE",
                "UTF-16",
                "ISO-8859-1",
                "ASCII",
                "US-ASCII",
                "HTML",
            ];
            for b in BUILTIN {
                if b.eq_ignore_ascii_case(e) {
                    return Some(b.to_string());
                }
            }
            Some(e.to_string())
        }
    }
}

/// UTF8ToHtml applied to a whole string.
fn utf8_to_html(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        let v = c as u32;
        if v < 0x80 {
            out.push(v as u8);
        } else {
            out.push(b'&');
            match entity_value_lookup(v) {
                Some(name) => out.extend_from_slice(name.as_bytes()),
                None => out.extend_from_slice(format!("#{}", v).as_bytes()),
            }
            out.push(b';');
        }
    }
    out
}

pub fn encode_output(s: &str, enc: &OutEnc) -> Vec<u8> {
    match enc {
        OutEnc::Raw => s.as_bytes().to_vec(),
        OutEnc::Html => utf8_to_html(s),
        OutEnc::Named(e) => {
            crate::parser::encode_from_utf8(s, e).unwrap_or_else(|| s.as_bytes().to_vec())
        }
    }
}

/// xmlEncodeEntitiesInternal (attr = attribute value mode)
fn encode_entities(out: &mut String, s: &str, html: bool, doc_has_encoding: bool, attr: bool) {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'<' {
            if html
                && attr
                && b.get(i + 1) == Some(&b'!')
                && b.get(i + 2) == Some(&b'-')
                && b.get(i + 3) == Some(&b'-')
            {
                if let Some(e) = s[i..].find("-->") {
                    let end = i + e + 3;
                    out.push_str(&s[i..end]);
                    i = end;
                    continue;
                }
            }
            out.push_str("&lt;");
        } else if c == b'>' {
            out.push_str("&gt;");
        } else if c == b'&' {
            if html && attr && b.get(i + 1) == Some(&b'{') {
                if let Some(e) = s[i..].find('}') {
                    let end = i + e + 1;
                    out.push_str(&s[i..end]);
                    i = end;
                    continue;
                }
            }
            out.push_str("&amp;");
        } else if (0x20..0x80).contains(&c) || c == b'\n' || c == b'\t' || (html && c == b'\r') {
            out.push(c as char);
        } else if c >= 0x80 {
            let ch = s[i..].chars().next().unwrap();
            if doc_has_encoding || html {
                out.push(ch);
            } else {
                let v = ch as u32;
                let v = if crate::parser::is_char(v) { v } else { 0xFFFD };
                out.push_str(&format!("&#x{:X};", v));
            }
            i += ch.len_utf8();
            continue;
        } else if c == 0x9 || c == 0xA || c == 0xD {
            out.push_str(&format!("&#{};", c));
        }
        i += 1;
    }
}

/// xmlOutputBufferWriteQuotedString
fn write_quoted(out: &mut String, s: &str) {
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

/// xmlURIEscapeStr(str, "\"#$%&+,/:;<=>?@[\\]^`{|}")
fn uri_escape(s: &str) -> String {
    const LIST: &[u8] = b"\"#$%&+,/:;<=>?@[\\]^`{|}";
    let mut out = String::new();
    for &ch in s.as_bytes() {
        let unreserved = ch.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&ch);
        if ch != b'@' && !unreserved && !LIST.contains(&ch) {
            out.push_str(&format!("%{:02X}", ch));
        } else {
            out.push(ch as char);
        }
    }
    out
}

struct DumpCtx {
    out: String,
    html: bool,
    doc_has_encoding: bool,
    format: bool,
}

fn write_prefixed_name(t: &Tree, out: &mut String, id: NodeId) {
    if let Some(p) = t.ns_prefix(t.nodes[id].ns) {
        out.push_str(p);
        out.push(':');
    }
    out.push_str(&t.nodes[id].name);
}

/// xmlNodeListGetString(doc, list, 0) for attribute children
fn attr_value_string(t: &Tree, ctx: &DumpCtx, attr: NodeId) -> String {
    let mut s = String::new();
    let mut c = t.nodes[attr].children;
    while let Some(x) = c {
        match t.nodes[x].t {
            NType::Text | NType::CData => encode_entities(
                &mut s,
                &t.nodes[x].content,
                ctx.html,
                ctx.doc_has_encoding,
                true,
            ),
            NType::EntityRef => {
                s.push('&');
                s.push_str(&t.nodes[x].name);
                s.push(';');
            }
            _ => {}
        }
        c = t.nodes[x].next;
    }
    s
}

/// htmlAttrDumpOutput
fn attr_dump(t: &Tree, ctx: &mut DumpCtx, attr: NodeId) {
    ctx.out.push(' ');
    write_prefixed_name(t, &mut ctx.out, attr);
    let n = &t.nodes[attr];
    if n.children.is_some() && !is_boolean_attr(&n.name) {
        let value = attr_value_string(t, ctx, attr);
        ctx.out.push('=');
        let parent = n.parent;
        let is_uri = n.ns.is_none()
            && parent.map(|p| t.nodes[p].ns.is_none()).unwrap_or(false)
            && (n.name.eq_ignore_ascii_case("href")
                || n.name.eq_ignore_ascii_case("action")
                || n.name.eq_ignore_ascii_case("src")
                || (n.name.eq_ignore_ascii_case("name")
                    && parent
                        .map(|p| t.nodes[p].name.eq_ignore_ascii_case("a"))
                        .unwrap_or(false)));
        if is_uri {
            let trimmed = value.trim_start_matches([' ', '\t', '\n', '\r']);
            let escaped = uri_escape(trimmed);
            write_quoted(&mut ctx.out, &escaped);
        } else {
            write_quoted(&mut ctx.out, &value);
        }
    }
}

fn ns_list_dump(t: &Tree, ctx: &mut DumpCtx, id: NodeId) {
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
        crate::serialize::escape_attr(&mut ctx.out, &n.href, true);
        ctx.out.push('"');
    }
}

/// htmlDtdDumpOutput
fn dtd_dump(t: &Tree, ctx: &mut DumpCtx, doc: NodeId) {
    let dtd = match t.int_subset(doc) {
        Some(d) => d,
        None => return,
    };
    let info = t.dtd_info(dtd).cloned().unwrap_or_default();
    ctx.out.push_str("<!DOCTYPE ");
    ctx.out.push_str(&t.nodes[dtd].name);
    if let Some(e) = &info.external_id {
        ctx.out.push_str(" PUBLIC ");
        write_quoted(&mut ctx.out, e);
        if let Some(s) = &info.system_id {
            ctx.out.push(' ');
            write_quoted(&mut ctx.out, s);
        }
    } else if let Some(s) = &info.system_id {
        if s != "about:legacy-compat" {
            ctx.out.push_str(" SYSTEM ");
            write_quoted(&mut ctx.out, s);
        }
    }
    ctx.out.push_str(">\n");
}

fn is_text_like(t: &Tree, id: NodeId) -> bool {
    matches!(t.nodes[id].t, NType::Text | NType::EntityRef)
}

/// `(parent != NULL) && (parent->name != NULL) && (parent->name[0] != 'p')`
/// (document and fragment nodes have no name)
fn parent_allows_newline(t: &Tree, id: Option<NodeId>) -> bool {
    match id {
        Some(p) => {
            !matches!(
                t.nodes[p].t,
                NType::Document | NType::HtmlDocument | NType::DocumentFrag
            ) && t.nodes[p].name.as_bytes().first() != Some(&b'p')
        }
        None => false,
    }
}

/// htmlNodeDumpFormatOutput (non-recursive walk, like libxml2)
fn node_dump(t: &Tree, ctx: &mut DumpCtx, root: NodeId) {
    let mut cur = root;
    let mut parent = t.nodes[cur].parent;
    let format = ctx.format;
    loop {
        let n = &t.nodes[cur];
        let mut descend = false;
        match n.t {
            NType::HtmlDocument | NType::Document => {
                dtd_dump(t, ctx, cur);
                if n.children.is_some() {
                    descend = true;
                } else {
                    ctx.out.push('\n');
                }
            }
            NType::Element => {
                let info = if n.ns.is_none() {
                    tag_lookup(&n.name)
                } else {
                    None
                };
                ctx.out.push('<');
                write_prefixed_name(t, &mut ctx.out, cur);
                if !n.ns_def.is_empty() {
                    ns_list_dump(t, ctx, cur);
                }
                let mut a = n.properties;
                while let Some(x) = a {
                    attr_dump(t, ctx, x);
                    a = t.nodes[x].next;
                }
                if info.map(|i| i.empty).unwrap_or(false) {
                    ctx.out.push('>');
                } else if n.children.is_none() {
                    if info
                        .map(|i| i.save_end_tag != 0 && i.name != "html" && i.name != "body")
                        .unwrap_or(false)
                    {
                        ctx.out.push('>');
                    } else {
                        ctx.out.push_str("></");
                        write_prefixed_name(t, &mut ctx.out, cur);
                        ctx.out.push('>');
                    }
                } else {
                    ctx.out.push('>');
                    let first = n.children.unwrap();
                    if format
                        && info.map(|i| !i.isinline).unwrap_or(false)
                        && !is_text_like(t, first)
                        && n.children != n.last
                        && n.name.as_bytes().first() != Some(&b'p')
                    {
                        ctx.out.push('\n');
                    }
                    descend = true;
                }
                if !descend
                    && format
                    && n.next.is_some()
                    && info.map(|i| !i.isinline).unwrap_or(false)
                {
                    let nx = n.next.unwrap();
                    if !is_text_like(t, nx) && parent_allows_newline(t, parent) {
                        ctx.out.push('\n');
                    }
                }
            }
            NType::Attribute => attr_dump(t, ctx, cur),
            NType::Text => {
                let raw_parent = parent
                    .map(|p| {
                        t.nodes[p].name.eq_ignore_ascii_case("script")
                            || t.nodes[p].name.eq_ignore_ascii_case("style")
                    })
                    .unwrap_or(false);
                if n.flags & 1 == 0 && !raw_parent {
                    encode_entities(
                        &mut ctx.out,
                        &n.content,
                        ctx.html,
                        ctx.doc_has_encoding,
                        false,
                    );
                } else {
                    ctx.out.push_str(&n.content);
                }
            }
            NType::Comment => {
                ctx.out.push_str("<!--");
                ctx.out.push_str(&n.content);
                ctx.out.push_str("-->");
            }
            NType::Pi => {
                ctx.out.push_str("<?");
                ctx.out.push_str(&n.name);
                if !n.content.is_empty() {
                    ctx.out.push(' ');
                    ctx.out.push_str(&n.content);
                }
                ctx.out.push('>');
            }
            NType::EntityRef => {
                ctx.out.push('&');
                ctx.out.push_str(&n.name);
                ctx.out.push(';');
            }
            NType::CData => {
                ctx.out.push_str(&n.content);
            }
            _ => {}
        }
        if descend {
            parent = Some(cur);
            cur = t.nodes[cur].children.unwrap();
            continue;
        }
        loop {
            if cur == root {
                return;
            }
            if let Some(nx) = t.nodes[cur].next {
                cur = nx;
                break;
            }
            cur = match parent {
                Some(p) => p,
                None => return,
            };
            parent = t.nodes[cur].parent;
            let n = &t.nodes[cur];
            if matches!(n.t, NType::HtmlDocument | NType::Document) {
                ctx.out.push('\n');
            } else {
                let info = if format && n.ns.is_none() {
                    tag_lookup(&n.name)
                } else {
                    None
                };
                let last = n.last.unwrap();
                if format
                    && info.map(|i| !i.isinline).unwrap_or(false)
                    && !is_text_like(t, last)
                    && n.children != n.last
                    && n.name.as_bytes().first() != Some(&b'p')
                {
                    ctx.out.push('\n');
                }
                ctx.out.push_str("</");
                write_prefixed_name(t, &mut ctx.out, cur);
                ctx.out.push('>');
                if format && info.map(|i| !i.isinline).unwrap_or(false) {
                    if let Some(nx) = n.next {
                        if !is_text_like(t, nx) && parent_allows_newline(t, parent) {
                            ctx.out.push('\n');
                        }
                    }
                }
            }
        }
    }
}

fn doc_has_encoding(t: &Tree, doc: Option<NodeId>) -> bool {
    doc.and_then(|d| t.doc_info(d))
        .map(|i| i.encoding.is_some())
        .unwrap_or(false)
}

/// htmlNodeDumpFormatOutput into a UTF-8 string. `doc` is the document
/// passed to libxml2 (decides HTML-style escaping).
pub fn node_dump_string(t: &Tree, doc: Option<NodeId>, node: NodeId, format: bool) -> String {
    let html = doc
        .map(|d| t.nodes[d].t == NType::HtmlDocument)
        .unwrap_or(false);
    let mut ctx = DumpCtx {
        out: String::new(),
        html,
        doc_has_encoding: doc_has_encoding(t, doc),
        format,
    };
    node_dump(t, &mut ctx, node);
    ctx.out
}

/// htmlDocContentDumpFormatOutput into a UTF-8 string (the document is
/// treated as an HTML document while dumping).
pub fn doc_content_dump_string(t: &Tree, doc: NodeId, format: bool) -> String {
    let mut ctx = DumpCtx {
        out: String::new(),
        html: true,
        doc_has_encoding: doc_has_encoding(t, Some(doc)),
        format,
    };
    node_dump(t, &mut ctx, doc);
    ctx.out
}

/// htmlGetMetaEncoding
pub fn get_meta_encoding(t: &Tree, doc: NodeId) -> Option<String> {
    let elem_named =
        |id: NodeId, name: &str| t.nodes[id].t == NType::Element && t.nodes[id].name == name;
    let mut cur = t.nodes[doc].children;
    // search the html element (or a head / meta directly)
    let mut state = 0; // 0: in doc, 1: found head, 2: found meta
    while let Some(c) = cur {
        if elem_named(c, "html") {
            break;
        }
        if elem_named(c, "head") {
            state = 1;
            break;
        }
        if elem_named(c, "meta") {
            state = 2;
            break;
        }
        cur = t.nodes[c].next;
    }
    let mut cur = cur?;
    if state == 0 {
        let mut c = t.nodes[cur].children;
        let mut found = None;
        while let Some(x) = c {
            if elem_named(x, "head") {
                found = Some((x, 1));
                break;
            }
            if elem_named(x, "meta") {
                found = Some((x, 2));
                break;
            }
            c = t.nodes[x].next;
        }
        let (x, s) = found?;
        cur = x;
        state = s;
    }
    let mut c = if state == 1 {
        t.nodes[cur].children
    } else {
        Some(cur)
    };
    while let Some(x) = c {
        if elem_named(x, "meta") {
            let mut http = false;
            let mut content: Option<String> = None;
            let mut a = t.nodes[x].properties;
            while let Some(at) = a {
                if let Some(ch) = t.nodes[at].children {
                    if t.nodes[ch].t == NType::Text && t.nodes[ch].next.is_none() {
                        let value = &t.nodes[ch].content;
                        let aname = &t.nodes[at].name;
                        if aname.eq_ignore_ascii_case("http-equiv")
                            && value.eq_ignore_ascii_case("Content-Type")
                        {
                            http = true;
                        } else if aname.eq_ignore_ascii_case("content") {
                            content = Some(value.clone());
                        }
                        if http {
                            if let Some(ct) = &content {
                                return charset_of_content(ct);
                            }
                        }
                    }
                }
                a = t.nodes[at].next;
            }
        }
        c = t.nodes[x].next;
    }
    None
}

fn charset_of_content(content: &str) -> Option<String> {
    let mut idx = None;
    for pat in ["charset=", "Charset=", "CHARSET="] {
        if let Some(i) = content.find(pat) {
            idx = Some(i + 8);
            break;
        }
    }
    if idx.is_none() {
        for pat in ["charset =", "Charset =", "CHARSET ="] {
            if let Some(i) = content.find(pat) {
                idx = Some(i + 9);
                break;
            }
        }
    }
    let i = idx?;
    Some(content[i..].trim_start_matches([' ', '\t']).to_string())
}

/// htmlSetMetaEncoding. Returns false on error (e.g. the "HTML" pseudo
/// encoding).
pub fn set_meta_encoding(t: &mut Tree, doc: NodeId, encoding: Option<&str>) -> bool {
    if let Some(e) = encoding {
        if e.eq_ignore_ascii_case("html") {
            return false;
        }
    }
    let newcontent = encoding.map(|e| {
        let mut s = format!("text/html; charset={}", e);
        s.truncate(99);
        s
    });
    let elem_named = |t: &Tree, id: NodeId, name: &str| {
        t.nodes[id].t == NType::Element && t.nodes[id].name.eq_ignore_ascii_case(name)
    };
    let mut head: Option<NodeId> = None;
    let mut meta: Option<NodeId> = None;
    let mut content: Option<String> = None;
    let mut cur = t.nodes[doc].children;
    // 0 = search html, 1 = found head, 2 = found meta
    let mut state = 0;
    while let Some(c) = cur {
        if elem_named(t, c, "html") {
            break;
        }
        if elem_named(t, c, "head") {
            state = 1;
            break;
        }
        if elem_named(t, c, "meta") {
            state = 2;
            break;
        }
        cur = t.nodes[c].next;
    }
    let c0 = match cur {
        None => return false,
        Some(c) => c,
    };
    let mut scan: Option<NodeId> = None;
    let mut create = false;
    match state {
        0 => {
            let mut c = t.nodes[c0].children;
            let mut found = None;
            while let Some(x) = c {
                if elem_named(t, x, "head") {
                    found = Some((x, 1));
                    break;
                }
                if elem_named(t, x, "meta") {
                    head = t.nodes[x].parent;
                    found = Some((x, 2));
                    break;
                }
                c = t.nodes[x].next;
            }
            match found {
                None => return false,
                Some((x, 1)) => {
                    head = Some(x);
                    if t.nodes[x].children.is_none() {
                        create = true;
                    } else {
                        scan = t.nodes[x].children;
                    }
                }
                Some((x, _)) => scan = Some(x),
            }
        }
        1 => {
            head = Some(c0);
            if t.nodes[c0].children.is_none() {
                create = true;
            } else {
                scan = t.nodes[c0].children;
            }
        }
        _ => scan = Some(c0),
    }
    if !create {
        let mut c = scan;
        while let Some(x) = c {
            if elem_named(t, x, "meta") {
                let mut http = false;
                content = None;
                let mut a = t.nodes[x].properties;
                while let Some(at) = a {
                    if let Some(ch) = t.nodes[at].children {
                        if t.nodes[ch].t == NType::Text && t.nodes[ch].next.is_none() {
                            let value = t.nodes[ch].content.clone();
                            let aname = t.nodes[at].name.clone();
                            if aname.eq_ignore_ascii_case("http-equiv")
                                && value.eq_ignore_ascii_case("Content-Type")
                            {
                                http = true;
                            } else if aname.eq_ignore_ascii_case("content") {
                                content = Some(value);
                            }
                            if http && content.is_some() {
                                break;
                            }
                        }
                    }
                    a = t.nodes[at].next;
                }
                if http && content.is_some() {
                    meta = Some(x);
                    break;
                }
            }
            c = t.nodes[x].next;
        }
    }
    match meta {
        None => {
            if let (Some(nc), Some(h)) = (&newcontent, head) {
                let m = t.new_element(Some(doc), "meta");
                match t.nodes[h].children {
                    None => {
                        t.add_child(h, m);
                    }
                    Some(first) => {
                        t.add_prev_sibling(first, m);
                    }
                }
                t.new_prop(m, None, "http-equiv", Some("Content-Type"));
                t.new_prop(m, None, "content", Some(nc));
            }
        }
        Some(m) => match (&newcontent, encoding) {
            (None, _) => t.unlink(m),
            (Some(nc), Some(e)) => {
                let has = content
                    .map(|c| c.to_ascii_lowercase().contains(&e.to_ascii_lowercase()))
                    .unwrap_or(false);
                if !has {
                    if let Some(a) = t.has_prop(m, "content") {
                        t.set_children_text(a, nc);
                    } else {
                        t.set_ns_prop(m, None, "content", nc);
                    }
                }
            }
            _ => {}
        },
    }
    true
}

/// htmlDocDumpMemoryFormat. `encoding` overrides the meta-declared encoding
/// (None: use htmlGetMetaEncoding like libxml2).
pub fn doc_dump_memory(t: &Tree, doc: NodeId, encoding: Option<&str>, format: bool) -> Vec<u8> {
    let enc = match encoding {
        Some(e) => Some(e.to_string()),
        None => get_meta_encoding(t, doc),
    };
    let oe = find_output_encoder(enc.as_deref());
    let s = doc_content_dump_string(t, doc, format);
    encode_output(&s, &oe)
}

/// htmlSaveFileFormat(filename, doc, htmlGetMetaEncoding(doc), format) as
/// DOMDocument::saveHTMLFile uses it: updates the meta element and returns
/// the bytes to write.
pub fn save_file_format(t: &mut Tree, doc: NodeId, format: bool) -> Vec<u8> {
    let enc = get_meta_encoding(t, doc);
    let oe = find_output_encoder(enc.as_deref());
    match handler_name(enc.as_deref()) {
        Some(h) => {
            set_meta_encoding(t, doc, Some(&h));
        }
        None => {
            set_meta_encoding(t, doc, Some("UTF-8"));
        }
    }
    let s = doc_content_dump_string(t, doc, format);
    encode_output(&s, &oe)
}
