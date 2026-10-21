#[path = "oracle_cases.rs"]
mod oracle_cases;

use super::*;
use crate::tree::{NType, NodeId, Tree};

fn show(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// PHP's json_encode of a string
fn json(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '/' => o.push_str("\\/"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) >= 0x80 => {
                let mut b = [0u16; 2];
                for u in c.encode_utf16(&mut b) {
                    o.push_str(&format!("\\u{:04x}", u));
                }
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// xmlGetLineNo
fn line_no(t: &Tree, id: NodeId) -> i64 {
    let n = &t.nodes[id];
    match n.t {
        NType::Element | NType::Text | NType::Comment | NType::Pi => n.line as i64,
        _ => {
            if let Some(p) = n.prev {
                if matches!(
                    t.nodes[p].t,
                    NType::Element | NType::Text | NType::Comment | NType::Pi
                ) {
                    return line_no(t, p);
                }
            }
            match n.parent {
                Some(p) if t.nodes[p].t == NType::Element => line_no(t, p),
                _ => -1,
            }
        }
    }
}

/// Same format as tdump() in the generator script.
fn tdump(t: &Tree, id: NodeId, d: usize, o: &mut String) {
    for c in t.children_of(id) {
        let n = &t.nodes[c];
        let (ty, name) = match n.t {
            NType::Element => (1, n.name.clone()),
            NType::Text => (3, "#text".to_string()),
            NType::CData => (4, "#cdata-section".to_string()),
            NType::Comment => (8, "#comment".to_string()),
            NType::Pi => (7, n.name.clone()),
            NType::Dtd => (10, n.name.clone()),
            NType::EntityRef => (5, n.name.clone()),
            _ => (0, n.name.clone()),
        };
        o.push_str(&format!(
            "{}{}:{}@{}",
            " ".repeat(d),
            ty,
            name,
            line_no(t, c)
        ));
        if n.t == NType::Element {
            for a in t.attrs_of(c) {
                let v = if t.nodes[a].children.is_some() {
                    json(&t.node_get_content(a))
                } else {
                    "-".to_string()
                };
                o.push_str(&format!(" {}={}", t.nodes[a].name, v));
            }
        }
        if matches!(n.t, NType::Text | NType::CData | NType::Comment) {
            o.push(' ');
            o.push_str(&json(&n.content));
        }
        o.push('\n');
        if n.t != NType::Dtd && n.children.is_some() {
            tdump(t, c, d + 1, o);
        }
    }
}

#[test]
fn oracle_cases() {
    let mut failures = Vec::new();
    let only: Option<usize> = std::env::var("HTML_CASE").ok().and_then(|s| s.parse().ok());
    for (i, c) in oracle_cases::CASES.iter().enumerate() {
        if let Some(o) = only {
            if o != i {
                continue;
            }
        }
        let mut t = Tree::default();
        let out = parse_html_in(&mut t, c.input, None, c.options, None);
        let doc = out.doc.expect("document");
        let mut errs = String::new();
        for e in &out.errors {
            errs.push_str(&format!(
                "{}/{} {}:{} {}\n",
                e.level,
                e.code,
                e.line,
                e.col,
                e.message.trim_end_matches('\n')
            ));
        }
        let mut tree = String::new();
        tdump(&t, doc, 0, &mut tree);
        let got_doc = html_doc_to_bytes(&t, doc, None, false);
        let got_doc_fmt = html_doc_to_bytes(&t, doc, None, true);
        let root = t.root_element(doc);
        let got_root = root
            .map(|r| html_node_to_bytes(&t, r, None, false))
            .unwrap_or_default();
        let got_root_fmt = root
            .map(|r| html_node_to_bytes(&t, r, None, true))
            .unwrap_or_default();
        let enc = t
            .doc_info(doc)
            .and_then(|d| d.encoding.clone())
            .unwrap_or_default();
        let checks: [(&str, &[u8], &[u8]); 7] = [
            ("tree", c.tree, tree.as_bytes()),
            ("doc", c.doc, &got_doc),
            ("doc_fmt", c.doc_fmt, &got_doc_fmt),
            ("root", c.root, &got_root),
            ("root_fmt", c.root_fmt, &got_root_fmt),
            ("errors", c.errors, errs.as_bytes()),
            ("encoding", c.encoding, enc.as_bytes()),
        ];
        for (what, exp, got) in checks {
            if exp != got {
                failures.push(format!(
                    "case {} {:?} [{}]\n  expected: {:?}\n  got:      {:?}",
                    i,
                    show(c.input),
                    what,
                    show(exp),
                    show(got)
                ));
            }
        }
    }
    let total = oracle_cases::CASES.len();
    if !failures.is_empty() {
        for f in &failures {
            eprintln!("{}", f);
        }
        panic!("{} mismatches over {} cases", failures.len(), total);
    }
}

#[path = "oracle_xml_cases.rs"]
mod oracle_xml_cases;

#[test]
fn oracle_xml_docs() {
    let mut failures = Vec::new();
    for (i, (input, doc_exp, docf_exp, root_exp)) in oracle_xml_cases::XML_CASES.iter().enumerate()
    {
        let out = crate::sax2::parse_doc(input, None, 0);
        let doc = out.doc.expect("xml doc");
        let (d, df, r) = tree::with(|t| {
            let root = t.root_element(doc).unwrap();
            (
                html_doc_to_bytes(t, doc, None, false),
                html_doc_to_bytes(t, doc, None, true),
                html_node_to_bytes(t, root, None, false),
            )
        });
        for (what, exp, got) in [
            ("doc", *doc_exp, &d),
            ("doc_fmt", *docf_exp, &df),
            ("root", *root_exp, &r),
        ] {
            if exp != got.as_slice() {
                failures.push(format!(
                    "xml case {} [{}]\n  expected: {:?}\n  got:      {:?}",
                    i,
                    what,
                    show(exp),
                    show(got)
                ));
            }
        }
    }
    if !failures.is_empty() {
        for f in &failures {
            eprintln!("{}", f);
        }
        panic!("{} mismatches", failures.len());
    }
}

#[test]
fn oracle_save_file() {
    let mut failures = Vec::new();
    for (i, (input, exp, expf)) in oracle_xml_cases::FILE_CASES.iter().enumerate() {
        for (fmt, exp) in [(false, *exp), (true, *expf)] {
            let mut t = Tree::default();
            let doc = parse_html_in(&mut t, input, None, 0, None).doc.unwrap();
            let got = html_save_file_bytes(&mut t, doc, fmt);
            if got != exp {
                failures.push(format!(
                    "file case {} fmt={}\n  expected: {:?}\n  got:      {:?}",
                    i,
                    fmt,
                    show(exp),
                    show(&got)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn save_file_adds_meta() {
    let mut t = Tree::default();
    let out = parse_html_in(&mut t, b"<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=iso-8859-1\"></head><body>\xe9</body></html>", None, 0, None);
    let doc = out.doc.unwrap();
    assert_eq!(
        html_get_meta_encoding(&t, doc).as_deref(),
        Some("iso-8859-1")
    );
    let b = html_save_file_bytes(&mut t, doc, false);
    assert!(b.ends_with(b"<body>\xe9</body></html>\n"), "{:?}", show(&b));
}
