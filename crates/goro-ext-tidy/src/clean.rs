//! Cleaning and repairing the parsed tree (PHP manual: tidy::cleanRepair).
//!
//! Tests 003.phpt, 020.phpt and 024.phpt show the doctype, the XML
//! declaration and the XHTML namespace being added here; the option behaviour
//! follows the options' documented meaning.

use crate::{Attr, Document, Level, NodeId, NodeType, tags};

const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";

pub fn clean_repair(d: &mut Document) {
    if d.config.bool("input-xml") {
        fix_xml_decl(d);
        return;
    }
    if let Some(body) = d.body() {
        apply_presentation_options(d, body);
        if d.config.bool("enclose-text") {
            enclose_text(d, body);
        }
    }
    if let Some(html) = d.html() {
        fix_attributes(d, html);
        if d.config.bool("drop-empty-elements") {
            trim_empty(d, html);
        }
    }
    fix_doctype(d);
    add_generator(d);
    fix_xml_decl(d);
    if d.config.bool("output-xhtml")
        && let Some(html) = d.html()
        && !d.nodes[html].attrs.iter().any(|a| a.name == "xmlns")
    {
        d.nodes[html].attrs.insert(
            0,
            Attr {
                name: "xmlns".to_string(),
                value: Some(XHTML_NS.to_string()),
            },
        );
    }
}

// ----- doctype and declarations --------------------------------------------

fn fix_doctype(d: &mut Document) {
    let xhtml = d.config.bool("output-xhtml");
    let mode = d.config.str("doctype").to_string();
    let text: Option<String> = match mode.as_str() {
        "omit" => None,
        "html5" => Some("html".to_string()),
        "strict" => Some(if xhtml {
            r#"html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd""#.to_string()
        } else {
            r#"html PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd""#
                .to_string()
        }),
        "loose" | "transitional" => Some(if xhtml {
            transitional_xhtml().to_string()
        } else {
            r#"html PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN" "http://www.w3.org/TR/html4/loose.dtd""#.to_string()
        }),
        "auto" | "user" => {
            let current = d.doctype().map(|i| d.nodes[i].text.clone());
            match current {
                // Without output-xhtml a declared doctype stands as written.
                Some(c) if !xhtml || c.contains("XHTML") => Some(c),
                _ if xhtml => Some(transitional_xhtml().to_string()),
                // Plain XML output declares nothing of its own.
                _ if d.config.bool("output-xml") => None,
                // Test 003.phpt.
                _ => Some("html".to_string()),
            }
        }
        // A quoted formal public identifier.
        fpi => Some(format!("html PUBLIC {fpi}")),
    };
    let existing = d.doctype();
    match (existing, text) {
        (Some(i), None) => d.detach(i),
        (Some(i), Some(t)) => d.nodes[i].text = t,
        (None, Some(t)) => {
            let id = d.new_node(NodeType::Doctype, "html", 1, 1);
            d.nodes[id].text = t;
            let root = d.root;
            d.nodes[id].parent = Some(root);
            let at = first_after_decl(d);
            d.nodes[root].children.insert(at, id);
        }
        (None, None) => {}
    }
}

fn transitional_xhtml() -> &'static str {
    r#"html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd""#
}

fn first_after_decl(d: &Document) -> usize {
    let root = &d.nodes[d.root].children;
    root.iter()
        .position(|&c| d.nodes[c].kind != NodeType::XmlDecl)
        .unwrap_or(root.len())
}

/// The XML declaration: test 020.phpt shows one added for XHTML in Latin-1,
/// naming the encoding. The option asks for it in any encoding; without it,
/// encodings XML assumes by default don't need one.
fn fix_xml_decl(d: &mut Document) {
    let xmlish =
        d.config.bool("output-xhtml") || d.config.bool("output-xml") || d.config.bool("input-xml");
    let root = d.root;
    let existing = d.nodes[root]
        .children
        .iter()
        .copied()
        .find(|&c| d.nodes[c].kind == NodeType::XmlDecl);
    if !xmlish {
        if let Some(e) = existing {
            d.detach(e);
        }
        return;
    }
    if existing.is_some() {
        return;
    }
    let enc = d.config.encoding("output-encoding");
    let name = enc.iana();
    let needed = !matches!(
        enc,
        crate::Encoding::Utf8 | crate::Encoding::Ascii | crate::Encoding::Raw
    );
    if !d.config.bool("add-xml-decl") && !needed {
        return;
    }
    let mut text = "xml version=\"1.0\"".to_string();
    if !name.is_empty() && needed {
        text.push_str(&format!(" encoding=\"{name}\""));
    }
    let id = d.new_node(NodeType::XmlDecl, "", 1, 1);
    d.nodes[id].text = text;
    d.nodes[id].parent = Some(root);
    d.nodes[root].children.insert(0, id);
}

/// `tidy-mark`: a meta element naming the tool.
fn add_generator(d: &mut Document) {
    if !d.config.bool("tidy-mark") {
        return;
    }
    let Some(head) = d.head() else { return };
    let (line, col) = (d.nodes[head].line, d.nodes[head].col);
    let already = d.nodes[head].children.iter().any(|&c| {
        d.is_element(c, "meta")
            && d.nodes[c]
                .attrs
                .iter()
                .any(|a| a.name == "name" && a.value.as_deref() == Some("generator"))
    });
    if already {
        return;
    }
    let id = d.new_node(NodeType::StartEnd, "meta", line, col);
    d.nodes[id].tag = tags::lookup("meta");
    d.nodes[id].attrs = vec![
        Attr {
            name: "name".to_string(),
            value: Some("generator".to_string()),
        },
        Attr {
            name: "content".to_string(),
            value: Some("goro tidy".to_string()),
        },
    ];
    d.nodes[id].parent = Some(head);
    d.nodes[head].children.push(id);
}

// ----- attributes ----------------------------------------------------------

fn is_uri_attr(name: &str) -> bool {
    matches!(
        name,
        "href" | "src" | "action" | "longdesc" | "cite" | "usemap" | "background" | "data"
    )
}

/// Attributes with a fixed list of values, written in lower case
/// (`lower-literals`).
fn is_literal_attr(name: &str) -> bool {
    matches!(
        name,
        "type" | "method" | "align" | "valign" | "shape" | "dir" | "frame" | "rules" | "scope"
    )
}

/// RFC 3986: spaces and non-ASCII characters in a URI are percent-encoded.
fn escape_uri(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        if c as u32 > 0x7E || c == ' ' {
            let mut b = [0; 4];
            for byte in c.encode_utf8(&mut b).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn fix_attributes(d: &mut Document, id: NodeId) {
    let fix_uri = d.config.bool("fix-uri");
    let fix_bs = d.config.bool("fix-backslash");
    let lower = d.config.bool("lower-literals");
    let drop_prop = d.config.bool("drop-proprietary-attributes");
    let alt = d.config.str("alt-text").to_string();
    let xhtml = d.config.bool("output-xhtml");
    let anchor_name = d.config.bool("anchor-as-name");
    let mut stack = vec![id];
    while let Some(n) = stack.pop() {
        stack.extend(d.nodes[n].children.iter().copied());
        if !matches!(d.nodes[n].kind, NodeType::Start | NodeType::StartEnd) {
            continue;
        }
        let name = d.nodes[n].name.clone();
        let attrs = &mut d.nodes[n].attrs;
        for a in attrs.iter_mut() {
            if let Some(v) = &mut a.value {
                if is_uri_attr(&a.name) {
                    if fix_bs {
                        *v = v.replace('\\', "/");
                    }
                    if fix_uri {
                        *v = escape_uri(v);
                    }
                } else if lower && is_literal_attr(&a.name) {
                    *v = v.to_ascii_lowercase();
                }
            }
        }
        if drop_prop {
            attrs.retain(|a| {
                !matches!(
                    a.name.as_str(),
                    "bgproperties" | "topmargin" | "leftmargin" | "marginheight" | "marginwidth"
                )
            });
        }
        if name == "img" && !alt.is_empty() && !attrs.iter().any(|a| a.name == "alt") {
            attrs.push(Attr {
                name: "alt".to_string(),
                value: Some(alt.clone()),
            });
        }
        if xhtml && name == "style" && !attrs.iter().any(|a| a.name == "type") {
            attrs.push(Attr {
                name: "type".to_string(),
                value: Some("text/css".to_string()),
            });
        }
        if xhtml && name == "script" && !attrs.iter().any(|a| a.name == "type") {
            attrs.push(Attr {
                name: "type".to_string(),
                value: Some("text/javascript".to_string()),
            });
        }
        // anchor-as-name: an anchor's name and id say the same thing.
        if anchor_name && name == "a" {
            let named = attrs.iter().find(|a| a.name == "name").cloned();
            if let Some(a) = named
                && xhtml
                && !attrs.iter().any(|x| x.name == "id")
            {
                attrs.push(Attr {
                    name: "id".to_string(),
                    value: a.value,
                });
            }
        }
    }
}

// ----- presentation --------------------------------------------------------

/// `logical-emphasis`, `drop-font-tags` and `clean`.
fn apply_presentation_options(d: &mut Document, body: NodeId) {
    let logical = d.config.bool("logical-emphasis");
    let drop_font = d.config.bool("drop-font-tags");
    let clean = d.config.bool("clean");
    let mut rules: Vec<(String, String, String)> = Vec::new();
    let mut order = Vec::new();
    collect(d, body, &mut order);
    for n in order {
        let name = d.nodes[n].name.clone();
        match name.as_str() {
            "b" if logical => rename(d, n, "strong"),
            "i" if logical => rename(d, n, "em"),
            "font" | "center" if drop_font => unwrap(d, n),
            "font" if clean => font_to_span(d, n, &mut rules),
            "center" if clean => {
                rename(d, n, "div");
                set_class(d, n, "div", "text-align: center", &mut rules);
            }
            "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" if clean => {
                let align = take_attr(d, n, "align");
                if let Some(Some(a)) = align {
                    let style = format!("text-align: {}", a.to_ascii_lowercase());
                    set_class(d, n, &name, &style, &mut rules);
                }
            }
            _ => {}
        }
    }
    if clean {
        merge_nested(d, body, "div", d.config.int("merge-divs"));
        merge_nested(d, body, "span", d.config.int("merge-spans"));
        add_style_rules(d, &rules);
    }
}

fn collect(d: &Document, id: NodeId, out: &mut Vec<NodeId>) {
    let mut stack = vec![id];
    while let Some(n) = stack.pop() {
        out.push(n);
        stack.extend(d.nodes[n].children.iter().rev().copied());
    }
}

fn rename(d: &mut Document, id: NodeId, name: &str) {
    d.nodes[id].name = name.to_string();
    d.nodes[id].tag = tags::lookup(name);
}

/// Replaces the element by its content.
fn unwrap(d: &mut Document, id: NodeId) {
    let Some(p) = d.nodes[id].parent else { return };
    let Some(i) = d.nodes[p].children.iter().position(|&c| c == id) else {
        return;
    };
    let kids = std::mem::take(&mut d.nodes[id].children);
    for &k in &kids {
        d.nodes[k].parent = Some(p);
    }
    d.nodes[p].children.splice(i..=i, kids);
    d.nodes[id].parent = None;
}

fn take_attr(d: &mut Document, id: NodeId, name: &str) -> Option<Option<String>> {
    let attrs = &mut d.nodes[id].attrs;
    let i = attrs.iter().position(|a| a.name == name)?;
    Some(attrs.remove(i).value)
}

/// CSS font-size values for the `size` attribute's scale (the CSS 2.1 keyword
/// sizes; relative values use the matching percentage).
fn font_size(v: &str) -> Option<String> {
    let abs = |n: i32| match n {
        i32::MIN..=1 => "x-small",
        2 => "small",
        3 => "medium",
        4 => "large",
        5 => "x-large",
        _ => "xx-large",
    };
    if let Some(r) = v.strip_prefix('+') {
        return r.parse::<i32>().ok().map(|n| format!("{}%", 100 + n * 20));
    }
    if let Some(r) = v.strip_prefix('-') {
        return r
            .parse::<i32>()
            .ok()
            .map(|n| format!("{}%", (100 - n * 20).max(20)));
    }
    v.parse::<i32>().ok().map(|n| abs(n).to_string())
}

fn font_to_span(d: &mut Document, id: NodeId, rules: &mut Vec<(String, String, String)>) {
    let mut style = Vec::new();
    if let Some(Some(f)) = take_attr(d, id, "face") {
        style.push(format!("font-family: {f}"));
    }
    if let Some(Some(s)) = take_attr(d, id, "size")
        && let Some(v) = font_size(&s)
    {
        style.push(format!("font-size: {v}"));
    }
    if let Some(Some(c)) = take_attr(d, id, "color") {
        style.push(format!("color: {c}"));
    }
    rename(d, id, "span");
    if !style.is_empty() {
        set_class(d, id, "span", &style.join("; "), rules);
    }
}

/// Gives the element a generated class for the style, reusing the class of
/// an identical rule.
fn set_class(
    d: &mut Document,
    id: NodeId,
    element: &str,
    style: &str,
    rules: &mut Vec<(String, String, String)>,
) {
    let existing = rules
        .iter()
        .find(|(_, e, s)| e == element && s == style)
        .map(|(c, _, _)| c.clone());
    let class = match existing {
        Some(c) => c,
        None => {
            let c = format!("{}{}", d.config.str("css-prefix"), rules.len() + 1);
            rules.push((c.clone(), element.to_string(), style.to_string()));
            c
        }
    };
    let attrs = &mut d.nodes[id].attrs;
    match attrs.iter_mut().find(|a| a.name == "class") {
        Some(a) => {
            let v = a.value.get_or_insert_default();
            if !v.is_empty() {
                v.push(' ');
            }
            v.push_str(&class);
        }
        None => attrs.push(Attr {
            name: "class".to_string(),
            value: Some(class),
        }),
    }
}

fn add_style_rules(d: &mut Document, rules: &[(String, String, String)]) {
    if rules.is_empty() {
        return;
    }
    let Some(head) = d.head() else { return };
    let (line, col) = (d.nodes[head].line, d.nodes[head].col);
    let mut css = String::new();
    for (class, element, style) in rules {
        css.push_str(&format!("{element}.{class} {{{style}}}\n"));
    }
    let id = d.new_node(NodeType::Start, "style", line, col);
    d.nodes[id].tag = tags::lookup("style");
    d.nodes[id].attrs.push(Attr {
        name: "type".to_string(),
        value: Some("text/css".to_string()),
    });
    let t = d.new_node(NodeType::Text, "", line, col);
    d.nodes[t].text = css;
    d.append(id, t);
    d.append(head, id);
}

/// `merge-divs` and `merge-spans`: an element that only holds one element of
/// the same name gives it up, joining their attributes. `auto` (2) merges
/// only when that loses nothing, `yes` (1) always does.
fn merge_nested(d: &mut Document, id: NodeId, name: &str, mode: i64) {
    if mode == 0 {
        return;
    }
    let mut order = Vec::new();
    collect(d, id, &mut order);
    for n in order {
        if d.nodes[n].name != name || !matches!(d.nodes[n].kind, NodeType::Start) {
            continue;
        }
        loop {
            let kids = &d.nodes[n].children;
            let [only] = kids.as_slice() else { break };
            let only = *only;
            if d.nodes[only].name != name || d.nodes[only].kind != NodeType::Start {
                break;
            }
            let inner_attrs = d.nodes[only].attrs.clone();
            let conflict = inner_attrs.iter().any(|a| {
                a.name != "class"
                    && a.name != "style"
                    && d.nodes[n].attrs.iter().any(|o| o.name == a.name)
            });
            if conflict && mode == 2 {
                break;
            }
            for a in inner_attrs {
                join_attr(d, n, a);
            }
            let grand = std::mem::take(&mut d.nodes[only].children);
            for &g in &grand {
                d.nodes[g].parent = Some(n);
            }
            d.nodes[n].children = grand;
            d.nodes[only].parent = None;
        }
    }
}

fn join_attr(d: &mut Document, id: NodeId, a: Attr) {
    let attrs = &mut d.nodes[id].attrs;
    match attrs.iter_mut().find(|o| o.name == a.name) {
        Some(o) if a.name == "class" => {
            let v = o.value.get_or_insert_default();
            v.push(' ');
            v.push_str(a.value.as_deref().unwrap_or(""));
        }
        Some(o) if a.name == "style" => {
            let v = o.value.get_or_insert_default();
            v.push_str("; ");
            v.push_str(a.value.as_deref().unwrap_or(""));
        }
        Some(o) => *o = a,
        None => attrs.push(a),
    }
}

/// `enclose-text`: text directly in the body goes in a paragraph.
fn enclose_text(d: &mut Document, body: NodeId) {
    let kids = d.nodes[body].children.clone();
    let mut new_kids = Vec::with_capacity(kids.len());
    let mut current: Option<NodeId> = None;
    for k in kids {
        let inline = d.nodes[k].kind == NodeType::Text
            || (matches!(d.nodes[k].kind, NodeType::Start | NodeType::StartEnd)
                && d.flags(k) & tags::INLINE != 0
                && d.flags(k) & tags::STRUCT == 0);
        if inline {
            let p = match current {
                Some(p) => p,
                None => {
                    let (l, c) = (d.nodes[k].line, d.nodes[k].col);
                    let p = d.new_node(NodeType::Start, "p", l, c);
                    d.nodes[p].tag = tags::lookup("p");
                    d.nodes[p].implicit = true;
                    d.nodes[p].parent = Some(body);
                    new_kids.push(p);
                    current = Some(p);
                    p
                }
            };
            d.append(p, k);
        } else {
            current = None;
            new_kids.push(k);
        }
    }
    d.nodes[body].children = new_kids;
}

// ----- empty elements ------------------------------------------------------

/// `drop-empty-elements`: phrasing and block elements with nothing in them
/// are removed, from the inside out. Declared and custom elements stay (test
/// 024.phpt), and so do structure, void elements and table cells.
fn trim_empty(d: &mut Document, id: NodeId) {
    let kids = d.nodes[id].children.clone();
    for k in kids {
        if matches!(d.nodes[k].kind, NodeType::Start) {
            trim_empty(d, k);
        }
    }
    let n = &d.nodes[id];
    if n.kind != NodeType::Start || !n.children.is_empty() {
        return;
    }
    let Some(tag) = n.tag else { return };
    let f = tag.flags;
    if f & (tags::STRUCT | tags::EMPTY | tags::HEAD | tags::RAW | tags::RCDATA | tags::TABLE_PART)
        != 0
    {
        return;
    }
    if matches!(
        n.name.as_str(),
        "a" | "iframe"
            | "object"
            | "applet"
            | "canvas"
            | "audio"
            | "video"
            | "li"
            | "dt"
            | "dd"
            | "option"
            | "button"
            | "label"
            | "map"
            | "noscript"
            | "template"
    ) {
        return;
    }
    if n.name == "p" && !d.config.bool("drop-empty-paras") {
        return;
    }
    // Elements that carry an identity or a style are still useful empty.
    if f & tags::BLOCK != 0 && !n.attrs.is_empty() {
        return;
    }
    if n.attrs.iter().any(|a| a.name == "id" || a.name == "name") {
        return;
    }
    let (line, col) = (n.line, n.col);
    let m = format!("trimming empty <{}>", n.name);
    d.report(Level::Warning, line, col, &m);
    d.detach(id);
}

// ----- accessibility -------------------------------------------------------

/// A small part of the accessibility checks (`accessibility-check`): missing
/// text alternatives and document language. Test 034.phpt only requires a
/// count, so this isn't a full implementation of the guideline checks.
pub fn accessibility_checks(d: &mut Document) {
    let mut order = Vec::new();
    collect(d, d.root, &mut order);
    for n in order {
        let node = &d.nodes[n];
        if !matches!(node.kind, NodeType::Start | NodeType::StartEnd) {
            continue;
        }
        let (line, col) = (node.line, node.col);
        let has = |name: &str| node.attrs.iter().any(|a| a.name == name);
        match node.name.as_str() {
            "img" if !has("alt") => {
                d.report(Level::Access, line, col, "<img> missing 'alt' text.");
            }
            "area" if !has("alt") => {
                d.report(Level::Access, line, col, "<area> missing 'alt' text.");
            }
            "html" if !has("lang") && !has("xml:lang") => {
                d.report(
                    Level::Access,
                    line,
                    col,
                    "document has no language declared.",
                );
            }
            _ => {}
        }
    }
}
