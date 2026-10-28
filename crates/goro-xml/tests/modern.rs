//! `Dom\` XML serialization: cases from the expected outputs of
//! ext/dom/tests/modern/spec/dom_parsing_gh*.phpt, serialize_element_unqualified.phpt,
//! xml_serialize_formatting.phpt and clone_conflicting_namespace_prefix.phpt.

use goro_xml::Arena;
use goro_xml::modern::{ModernOptions, to_string};
use goro_xml::tree::{NodeId, QName};
use goro_xml::xml::{ParseOptions, build};

fn parse(arena: &mut Arena, s: &str) -> NodeId {
    let po = ParseOptions {
        modern: true,
        ..Default::default()
    };
    let b = build(arena, s.as_bytes(), &po, None);
    assert!(b.ok, "{:?}", b.errors);
    arena.doc_mut(b.doc).modern = true;
    arena.doc_mut(b.doc).encoding = Some("UTF-8".into());
    b.doc
}

fn set_attr_ns(arena: &mut Arena, el: NodeId, ns: &str, qname: &str, v: &str) {
    let doc = arena.doc_of(el);
    let (prefix, local) = match qname.split_once(':') {
        Some((p, l)) => (Some(p.into()), l.into()),
        None => (None, qname.into()),
    };
    let a = arena.new_attribute(
        doc,
        QName {
            prefix,
            local,
            ns: Some(ns.into()),
        },
        v,
    );
    arena.raw_append_attr(el, a);
}

fn save(arena: &Arena, n: NodeId, format: bool) -> String {
    to_string(
        arena,
        n,
        ModernOptions {
            format,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn gh44_generated_prefix() {
    let mut a = Arena::new();
    let d = parse(
        &mut a,
        r#"<root xmlns:ns2="uri2"><child xmlns:ns1="uri1" xmlns:a0="uri1" xmlns:NS1="uri1"/></root>"#,
    );
    let root = a.document_element(d).unwrap();
    let child = a.first_child(root).unwrap();
    set_attr_ns(&mut a, child, "uri3", "attr1", "value1");
    assert_eq!(
        save(&a, d, false),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root xmlns:ns2=\"uri2\"><child xmlns:ns1=\"uri1\" xmlns:a0=\"uri1\" xmlns:NS1=\"uri1\" xmlns:ns2=\"uri3\" ns2:attr1=\"value1\"/></root>"
    );
}

#[test]
fn gh45_preferred_prefix() {
    let mut a = Arena::new();
    let d = parse(
        &mut a,
        r#"<el1 xmlns:p="u1" xmlns:q="u1"><el2 xmlns:q="u2"/></el1>"#,
    );
    let root = a.document_element(d).unwrap();
    let child = a.first_child(root).unwrap();
    set_attr_ns(&mut a, child, "u1", "name", "v");
    assert_eq!(
        save(&a, root, false),
        r#"<el1 xmlns:p="u1" xmlns:q="u1"><el2 xmlns:q="u2" p:name="v"/></el1>"#
    );
}

#[test]
fn gh47_null_namespaces() {
    let mut a = Arena::new();
    let d = parse(&mut a, r#"<root xmlns=""><child xmlns=""/></root>"#);
    assert_eq!(
        save(&a, d, false),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root xmlns=\"\"><child xmlns=\"\"/></root>"
    );
    let mut a = Arena::new();
    let d = parse(
        &mut a,
        r#"<root xmlns="urn:u1"><child xmlns="urn:u1"/></root>"#,
    );
    assert_eq!(
        save(&a, d, false),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root xmlns=\"urn:u1\"><child xmlns=\"urn:u1\"/></root>"
    );
    let mut a = Arena::new();
    let src = "<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\">\n  <head>\n    <people xmlns=\"\">\n      <person/>\n    </people>\n  </head>\n</html>";
    let d = parse(&mut a, src);
    assert_eq!(
        save(&a, d, false),
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{src}")
    );
}

#[test]
fn unqualified_nearest_prefix() {
    let mut a = Arena::new();
    let d = parse(
        &mut a,
        r#"<root xmlns:p1="u1"><child xmlns:p2="u1"><p1:child2/></child></root>"#,
    );
    assert_eq!(
        save(&a, d, false),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root xmlns:p1=\"u1\"><child xmlns:p2=\"u1\"><p1:child2/></child></root>"
    );
    let mut a = Arena::new();
    let d = parse(
        &mut a,
        r#"<root xmlns:p1="u1"><child xmlns:p2="u1"></child></root>"#,
    );
    let root = a.document_element(d).unwrap();
    let child = a.first_child(root).unwrap();
    let c2 = a.new_element(
        d,
        QName {
            prefix: None,
            local: "child2".into(),
            ns: Some("u1".into()),
        },
    );
    a.append_child(child, c2).unwrap();
    assert_eq!(
        save(&a, d, false),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root xmlns:p1=\"u1\"><child xmlns:p2=\"u1\"><p2:child2/></child></root>"
    );
}

#[test]
fn conflicting_prefix_attribute() {
    let mut a = Arena::new();
    let src = "<root>\n    <a:root1 xmlns:a=\"urn:a\">\n        <a:root2 xmlns:a=\"urn:b\">\n            <a:child xmlns:b=\"urn:b\">\n                <b:child1 xmlns:a=\"urn:x\">bar</b:child1>\n            </a:child>\n        </a:root2>\n    </a:root1>\n</root>";
    let d = parse(&mut a, src);
    let root = a.document_element(d).unwrap();
    let r1 = a.children(root).find(|&c| a.is_element(c)).unwrap();
    set_attr_ns(&mut a, r1, "urn:y", "a:foo", "bar");
    let want = src.replace(
        "<a:root1 xmlns:a=\"urn:a\">",
        "<a:root1 xmlns:a=\"urn:a\" xmlns:ns1=\"urn:y\" ns1:foo=\"bar\">",
    );
    assert_eq!(
        save(&a, d, false),
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{want}")
    );
}

#[test]
fn formatting() {
    let cases = [
        ("<root></root>", "<root/>"),
        ("<root> </root>", "<root> </root>"),
        (
            "<!DOCTYPE html><html><head/></html>",
            "<!DOCTYPE html>\n<html>\n  <head/>\n</html>",
        ),
        (
            "<root><child>text</child><!-- comment --><foo>&amp;</foo></root>",
            "<root>\n  <child>text</child>\n  <!-- comment -->\n  <foo>&amp;</foo>\n</root>",
        ),
        (
            "<root><child>text</child><![CDATA[x]]><foo><bar/></foo></root>",
            "<root><child>text</child><![CDATA[x]]><foo><bar/></foo></root>",
        ),
        (
            "<root><a/><foo><bar><?foo <x><y/></x> ?><baz/></bar></foo></root>",
            "<root>\n  <a/>\n  <foo>\n    <bar>\n      <?foo <x><y/></x> ?>\n      <baz/>\n    </bar>\n  </foo>\n</root>",
        ),
    ];
    for (src, want) in cases {
        let mut a = Arena::new();
        let d = parse(&mut a, src);
        assert_eq!(
            save(&a, d, true),
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{want}"),
            "{src}"
        );
    }
}
