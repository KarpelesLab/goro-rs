//! html5lib-tests tree-construction runner.
//!
//! The data files are not part of the repository. Download
//! https://github.com/html5lib/html5lib-tests (the tree-construction tests
//! were removed from its tip in 2026; use commit a9f4496 or the WPT copy)
//! and run:
//!
//! ```text
//! HTML5LIB_TREE_DIR=/path/to/tree-construction cargo test -p goro-xml \
//!     --test html5lib -- --ignored --nocapture
//! ```
//!
//! `#script-on` tests are skipped (PHP parses with scripting disabled).
//! The parser runs with `standard: true` (see `ParseOptions::standard`).

use goro_xml::html5::{self, FragmentOptions, ParseOptions};
use goro_xml::tree::{
    Arena, DocKind, HTML_NS, MATHML_NS, NodeData, NodeId, QName, SVG_NS, XLINK_NS, XML_NS, XMLNS_NS,
};

fn dump(arena: &Arena, n: NodeId, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    match arena.data(n) {
        NodeData::Element(e) => {
            let name = match e.name.ns.as_deref() {
                Some(SVG_NS) => format!("svg {}", e.name.local),
                Some(MATHML_NS) => format!("math {}", e.name.local),
                _ => e.name.local.to_string(),
            };
            out.push_str(&format!("| {pad}<{name}>\n"));
            let mut attrs: Vec<(String, String)> = arena
                .attrs(n)
                .map(|a| {
                    let d = arena.attr(a).unwrap();
                    let nm = match d.name.ns.as_deref() {
                        Some(XLINK_NS) => format!("xlink {}", d.name.local),
                        Some(XML_NS) => format!("xml {}", d.name.local),
                        Some(XMLNS_NS) => format!("xmlns {}", d.name.local),
                        _ => d.name.local.to_string(),
                    };
                    (nm, arena.attr_value(a))
                })
                .collect();
            attrs.sort();
            for (k, v) in attrs {
                out.push_str(&format!("| {pad}  {k}=\"{v}\"\n"));
            }
            if let Some(tc) = e.template_contents
                && e.name.ns.as_deref() == Some(HTML_NS)
            {
                out.push_str(&format!("| {pad}  content\n"));
                for c in arena.children(tc) {
                    dump(arena, c, depth + 2, out);
                }
            }
            for c in arena.children(n) {
                dump(arena, c, depth + 1, out);
            }
        }
        NodeData::Text(t) => out.push_str(&format!("| {pad}\"{t}\"\n")),
        NodeData::Comment(t) => out.push_str(&format!("| {pad}<!-- {t} -->\n")),
        NodeData::Doctype(d) => {
            let p = d.public_id.clone().unwrap_or_default();
            let s = d.system_id.clone().unwrap_or_default();
            if p.is_empty() && s.is_empty() {
                out.push_str(&format!("| {pad}<!DOCTYPE {}>\n", d.name));
            } else {
                out.push_str(&format!("| {pad}<!DOCTYPE {} \"{p}\" \"{s}\">\n", d.name));
            }
        }
        _ => {
            for c in arena.children(n) {
                dump(arena, c, depth, out);
            }
        }
    }
}

struct Case {
    data: String,
    fragment: Option<String>,
    script: Option<bool>,
    document: String,
}

fn parse_dat(text: &str) -> Vec<Case> {
    let mut cases = Vec::new();
    let mut lines = text.split('\n').peekable();
    while let Some(l) = lines.next() {
        if l != "#data" {
            continue;
        }
        let mut data = Vec::new();
        let mut fragment = None;
        let mut script = None;
        let mut document = Vec::new();
        let mut section = "data";
        while let Some(&l) = lines.peek() {
            if l == "#data" {
                break;
            }
            lines.next();
            if l.starts_with('#')
                && matches!(
                    l,
                    "#errors"
                        | "#new-errors"
                        | "#document-fragment"
                        | "#document"
                        | "#script-off"
                        | "#script-on"
                )
            {
                section = l;
                if l == "#script-off" {
                    script = Some(false);
                }
                if l == "#script-on" {
                    script = Some(true);
                }
                continue;
            }
            match section {
                "data" => data.push(l),
                "#document-fragment" => fragment = Some(l.to_string()),
                "#document" => document.push(l),
                _ => {}
            }
        }
        // The document section ends with an empty line.
        while document.last() == Some(&"") {
            document.pop();
        }
        let mut doc = String::new();
        for d in document {
            doc.push_str(d);
            doc.push('\n');
        }
        cases.push(Case {
            data: data.join("\n"),
            fragment,
            script,
            document: doc,
        });
    }
    cases
}

fn run_case(c: &Case) -> String {
    let mut arena = Arena::new();
    let mut out = String::new();
    match &c.fragment {
        None => {
            let r = html5::parse_document_str(
                &mut arena,
                c.data.clone(),
                &ParseOptions {
                    collect_errors: true,
                    standard: true,
                    ..Default::default()
                },
            );
            dump(&arena, r.doc, 0, &mut out);
        }
        Some(ctx) => {
            let doc = arena.new_document(DocKind::Html5);
            arena.doc_mut(doc).modern = true;
            let (ns, name) = match ctx.split_once(' ') {
                Some(("svg", n)) => (SVG_NS, n),
                Some(("math", n)) => (MATHML_NS, n),
                _ => (HTML_NS, ctx.as_str()),
            };
            let el = arena.new_element(
                doc,
                QName {
                    prefix: None,
                    local: name.into(),
                    ns: Some(ns.into()),
                },
            );
            arena.raw_append(doc, el);
            let r = html5::parse_fragment(
                &mut arena,
                el,
                &c.data,
                &FragmentOptions {
                    collect_errors: true,
                    standard: true,
                },
            );
            for k in arena.children(r.fragment).collect::<Vec<_>>() {
                dump(&arena, k, 0, &mut out);
            }
        }
    }
    out
}

#[test]
#[ignore]
fn html5lib_tree_construction() {
    let Ok(dir) = std::env::var("HTML5LIB_TREE_DIR") else {
        eprintln!("HTML5LIB_TREE_DIR not set");
        return;
    };
    let verbose = std::env::var("HTML5LIB_VERBOSE").is_ok();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "dat"))
        .collect();
    files.sort();
    let (mut pass, mut fail, mut skip) = (0, 0, 0);
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let (mut fp, mut ff) = (0, 0);
        for c in parse_dat(&text) {
            if c.script == Some(true) {
                skip += 1;
                continue;
            }
            let got = std::panic::catch_unwind(|| run_case(&c));
            let ok = matches!(&got, Ok(g) if *g == c.document);
            if ok {
                fp += 1;
            } else {
                ff += 1;
                if verbose {
                    eprintln!(
                        "FAIL {}\n--- data\n{}\n--- fragment {:?}\n--- expected\n{}--- got\n{}",
                        f.display(),
                        c.data,
                        c.fragment,
                        c.document,
                        match &got {
                            Ok(g) => g.clone(),
                            Err(_) => "PANIC\n".into(),
                        }
                    );
                }
            }
        }
        eprintln!(
            "{}: {fp} passed, {ff} failed",
            f.file_name().unwrap().to_string_lossy()
        );
        pass += fp;
        fail += ff;
    }
    eprintln!(
        "TOTAL: {pass} passed, {fail} failed, {skip} skipped (script-on); {:.2}%",
        100.0 * pass as f64 / (pass + fail).max(1) as f64
    );
}
