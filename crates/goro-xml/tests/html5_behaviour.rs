//! HTML5 parser/serializer behaviour required by php-src's ext/dom tests
//! (inputs and expected strings from tests under
//! ext/dom/tests/modern/html/, used as black boxes).

use goro_xml::html5::{self, FragmentOptions, ParseOptions, WhatwgEncoding, encoding};
use goro_xml::tree::{Arena, HTML_NS, NodeData, NodeId, QName, XLINK_NS, XMLNS_NS};

fn parse(arena: &mut Arena, s: &str, opts: ParseOptions) -> html5::ParsedDocument {
    html5::parse_document(arena, s.as_bytes(), &opts)
}

fn errs(s: &str) -> Vec<String> {
    let mut a = Arena::new();
    let r = parse(
        &mut a,
        s,
        ParseOptions {
            collect_errors: true,
            ..Default::default()
        },
    );
    r.errors.iter().map(|e| e.message("Entity")).collect()
}

fn html(s: &str, opts: ParseOptions) -> String {
    let mut a = Arena::new();
    let r = parse(&mut a, s, opts);
    html5::serialize_node(&a, r.doc)
}

#[test]
fn parser_warning_01() {
    // parser/HTMLDocument_fromString_parser_warning_01.phpt
    let input = "<title>foo</title>\n<datalist id=\"fruits\">\n    <option value=\"Apple\">\n    <option value=\"Banana\">\n    <option value=\"Pear\">\n</datalist>\n<p>error</>";
    assert_eq!(
        errs(input),
        [
            "tokenizer error missing-end-tag-name in Entity, line: 7, column: 11",
            "tree error unexpected-token-in-initial-mode in Entity, line: 1, column: 2-6",
        ]
    );
    assert_eq!(
        html(input, ParseOptions::default()),
        "<html><head><title>foo</title>\n</head><body><datalist id=\"fruits\">\n    <option value=\"Apple\">\n    </option><option value=\"Banana\">\n    </option><option value=\"Pear\">\n</option></datalist>\n<p>error</p></body></html>"
    );
}

#[test]
fn parser_warning_02() {
    // parser/HTMLDocument_fromString_parser_warning_02.phpt
    let input = "<!DOCTYPE HTML>\n<html>\n    <body>\n        <p\0>foo</p>\n        <p id=\"foo\"class=\"bar\"/>\n        <!ELEMENT br EMPTY>\n        <!-- <!-- nested --> -->\n        <!doctype html>\n    </body>\n</html>";
    assert_eq!(
        errs(input),
        [
            "tokenizer error unexpected-null-character in Entity, line: 4, column: 11",
            "tokenizer error missing-whitespace-between-attributes in Entity, line: 5, column: 20",
            "tokenizer error incorrectly-opened-comment in Entity, line: 6, column: 11",
            "tokenizer error nested-comment in Entity, line: 7, column: 18",
            "tree error unexpected-closed-token in Entity, line: 4, column: 18",
            "tree error doctype-token-in-body-mode in Entity, line: 8, column: 11-17",
        ]
    );
    assert_eq!(
        html(input, ParseOptions::default()),
        "<!DOCTYPE html><html><head></head><body>\n        <p\u{FFFD}>foo<p></p>\n        <p id=\"foo\" class=\"bar\">\n        <!--ELEMENT br EMPTY-->\n        <!-- <!-- nested --> --&gt;\n        \n    \n</p></p\u{FFFD}></body></html>"
    );
}

#[test]
fn internal_error_positions() {
    // parser/HTMLDocument_fromString_parser_warning_internal_error.phpt
    assert_eq!(
        errs("<>x</> <!doctype html>"),
        [
            "tokenizer error invalid-first-character-of-tag-name in Entity, line: 1, column: 2",
            "tokenizer error missing-end-tag-name in Entity, line: 1, column: 6",
            "tree error unexpected-token-in-initial-mode in Entity, line: 1, column: 1-7",
            "tree error doctype-token-in-body-mode in Entity, line: 1, column: 10-16",
        ]
    );
    // parser/user_error_handler.phpt, interactions/HTMLDocument_clone.phpt
    assert_eq!(
        errs("<html></html>"),
        ["tree error unexpected-token-in-initial-mode in Entity, line: 1, column: 2-5"]
    );
    assert_eq!(
        errs("<p>foo</p>"),
        ["tree error unexpected-token-in-initial-mode in Entity, line: 1, column: 2"]
    );
}

#[test]
fn old_doctype() {
    // parser/HTMLDocument_fromString_old_dtd.phpt
    let input = "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01//EN\" \"http://www.w3.org/TR/html4/strict.dtd\">\n<html>\n<head>\n</head>\n<body>\n</body>\n</html>";
    assert_eq!(
        errs(input),
        ["tree error bad-doctype-token-in-initial-mode in Entity, line: 1, column: 3-9"]
    );
    let mut a = Arena::new();
    let r = parse(&mut a, input, ParseOptions::default());
    assert_eq!(
        html5::serialize_node(&a, r.doc),
        "<!DOCTYPE html><html><head>\n</head>\n<body>\n\n</body></html>"
    );
    let dt = a.doc(r.doc).doctype.unwrap();
    let d = a.dtd(dt).unwrap();
    assert_eq!(d.public_id.as_deref(), Some("-//W3C//DTD HTML 4.01//EN"));
    assert_eq!(
        d.system_id.as_deref(),
        Some("http://www.w3.org/TR/html4/strict.dtd")
    );
}

fn elements(a: &Arena, root: NodeId) -> Vec<NodeId> {
    a.descendants(root).filter(|&n| a.is_element(n)).collect()
}

#[test]
fn line_numbers() {
    // parser/HTMLDocument_fromString_line_column.phpt
    let input = "<!doctype html>\n<html>\n    <head>\n        <title>foo</title>\n    </head>\n    <body>\n        <div id=\"mydiv\" x=\"foo\">\n            <p>\n                <strong>This is my paragraph</strong>\n                <!-- my comment -->\n            </p>\n        </div>\n    </body>\n</html>";
    let mut a = Arena::new();
    let r = parse(&mut a, input, ParseOptions::default());
    let lines: Vec<(String, u32)> = elements(&a, r.doc)
        .into_iter()
        .map(|n| (a.element(n).unwrap().name.local.to_string(), a.line(n)))
        .collect();
    assert_eq!(
        lines,
        [
            ("html".to_string(), 1),
            ("head".into(), 2),
            ("title".into(), 3),
            ("body".into(), 5),
            ("div".into(), 6),
            ("p".into(), 7),
            ("strong".into(), 8)
        ]
    );
    let strong = elements(&a, r.doc)[6];
    assert_eq!(a.line(a.first_child(strong).unwrap()), 8);
    let div = elements(&a, r.doc)[4];
    for at in a.attrs(div) {
        assert_eq!(a.line(at), 6);
    }
    let comment = a
        .descendants(r.doc)
        .find(|&n| matches!(a.data(n), NodeData::Comment(_)))
        .unwrap();
    assert_eq!(a.line(comment), 9);
    // interactions/getLineNo_65536.phpt
    let input = format!(
        "{}<!doctype html>\n<body>\n    <p>hello</p>\n</body>",
        "\n".repeat(65536)
    );
    let r = parse(&mut a, &input, ParseOptions::default());
    let p = elements(&a, r.doc)
        .into_iter()
        .find(|&n| &*a.element(n).unwrap().name.local == "p")
        .unwrap();
    assert_eq!(a.line(p), 65538);
}

#[test]
fn no_implied() {
    // parser/HTMLDocument_fromString_fromFile_LIBXML_HTML_NOIMPLIED.phpt
    let o = || ParseOptions {
        no_implied: true,
        ..Default::default()
    };
    let cases = [
        ("", ""),
        ("foobarbaz", "foobarbaz"),
        ("<p>foo</p>", "<p>foo</p>"),
        ("<body><p>foo</p></body>", "<body><p>foo</p></body>"),
        ("<title>x</title><p>foo</p>", "<title>x</title><p>foo</p>"),
        (
            "<!-- bar --><head><title>x</title><!-- foo --></head><p>foo</p><!-- bar -->",
            "<!-- bar --><head><title>x</title><!-- foo --></head><p>foo</p><!-- bar -->",
        ),
        ("<html>foobar</html>", "<html>foobar</html>"),
        (
            "<html><!-- foo --><title>a</title><p>foo</p></html><!-- bar -->",
            "<html><!-- foo --><title>a</title><p>foo</p></html><!-- bar -->",
        ),
        (
            "<!-- start --><body><head><html>foo</html></head></body><!-- end -->",
            "<!-- start --><body>foo</body><!-- end -->",
        ),
    ];
    for (i, e) in cases {
        assert_eq!(html(i, o()), e, "{i:?}");
    }
    // parser/HTMLDocument_fromString_LIBXML_HTML_NOIMPLIED_error.phpt
    let mut a = Arena::new();
    let r = html5::parse_document(
        &mut a,
        b"<div>foo</div>",
        &ParseOptions {
            no_implied: true,
            collect_errors: true,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty());
}

#[test]
fn no_default_ns() {
    // parser/HTMLDocument_fromString_DOM_HTML_NO_DEFAULT_NS.phpt,
    // common/template_no_default_ns.phpt
    let mut a = Arena::new();
    let r = parse(
        &mut a,
        "<!DOCTYPE html><html><body><template>a<div>foo</div>b</template><svg/></body></html>",
        ParseOptions {
            no_default_ns: true,
            ..Default::default()
        },
    );
    let els = elements(&a, r.doc);
    let html_el = a.element(els[0]).unwrap();
    assert_eq!(html_el.name.ns, None);
    let tpl = els
        .iter()
        .copied()
        .find(|&n| &*a.element(n).unwrap().name.local == "template")
        .unwrap();
    assert!(a.element(tpl).unwrap().template_contents.is_none());
    assert_eq!(html5::serialize_children(&a, tpl), "a<div>foo</div>b");
    let svg = els
        .iter()
        .copied()
        .find(|&n| &*a.element(n).unwrap().name.local == "svg")
        .unwrap();
    assert_eq!(
        a.element(svg).unwrap().name.ns.as_deref(),
        Some(goro_xml::tree::SVG_NS)
    );
}

#[test]
fn templates() {
    // common/template_participation.phpt, common/template_nested.phpt
    let mut a = Arena::new();
    let r = parse(
        &mut a,
        "<!DOCTYPE html>\n<html>\n    <body>\n        <template>foo<template>bar</template></template>\n    </body>\n</html>",
        ParseOptions::default(),
    );
    let tpl = elements(&a, r.doc)
        .into_iter()
        .find(|&n| &*a.element(n).unwrap().name.local == "template")
        .unwrap();
    assert!(a.first_child(tpl).is_none());
    assert_eq!(
        html5::serialize_children(&a, tpl),
        "foo<template>bar</template>"
    );
    assert_eq!(
        html5::serialize_node(&a, r.doc),
        "<!DOCTYPE html><html><head></head><body>\n        <template>foo<template>bar</template></template>\n    \n</body></html>"
    );
}

#[test]
fn namespaced_attributes() {
    // parser/HTMLDocument_createFromString_namespaced_attributes.phpt
    let mut a = Arena::new();
    let r = parse(
        &mut a,
        "<!DOCTYPE html>\n<html>\n    <svg width=\"1\" xmlns:xlink='http://www.w3.org/1999/xlink'>\n        <use xlink:href='#test' foo=\"bar\"></use>\n    </svg>\n    <math>\n        <mo accent=\"true\"></mo>\n    </math>\n</html>",
        ParseOptions::default(),
    );
    let mut seen = Vec::new();
    for n in elements(&a, r.doc) {
        for at in a.attrs(n) {
            let d = a.attr(at).unwrap();
            seen.push((
                d.name.qualified(),
                a.attr_value(at),
                d.name.ns.as_deref().map(str::to_string),
            ));
        }
    }
    assert_eq!(
        seen,
        [
            ("width".to_string(), "1".to_string(), None),
            (
                "xmlns:xlink".into(),
                XLINK_NS.into(),
                Some(XMLNS_NS.to_string())
            ),
            (
                "xlink:href".into(),
                "#test".into(),
                Some(XLINK_NS.to_string())
            ),
            ("foo".into(), "bar".into(), None),
            ("accent".into(), "true".into(), None),
        ]
    );
}

#[test]
fn serializer() {
    // serializer/HTMLDocument_escape_attribute.phpt, _escape_nbsp.phpt,
    // _serialize_text_02.phpt
    assert_eq!(
        html(
            "<p>these must transform: \u{a0}\u{a0} but these not: \u{fffd}|</p><br>",
            ParseOptions::default()
        ),
        "<html><head></head><body><p>these must transform: &nbsp;&nbsp; but these not: \u{fffd}|</p><br></body></html>"
    );
    let mut a = Arena::new();
    let doc = a.new_document(goro_xml::tree::DocKind::Html5);
    a.doc_mut(doc).modern = true;
    let mk = |a: &mut Arena, ns: &str, name: &str| {
        a.new_element(
            doc,
            QName {
                prefix: None,
                local: name.into(),
                ns: Some(ns.into()),
            },
        )
    };
    let body = mk(&mut a, HTML_NS, "body");
    a.raw_append(doc, body);
    for tag in ["style", "script", "noscript"] {
        let e = mk(&mut a, HTML_NS, tag);
        a.raw_append(body, e);
        a.set_text_content(e, "&\"<>\u{a0} foobar");
    }
    let p = mk(&mut a, HTML_NS, "p");
    a.raw_append(body, p);
    a.set_attribute(p, "foo", "<bar>\"'&");
    let x = a.new_element(
        doc,
        QName {
            prefix: Some("x".into()),
            local: "br".into(),
            ns: Some("http://php.net/foo".into()),
        },
    );
    a.raw_append(body, x);
    assert_eq!(
        html5::serialize_node(&a, doc),
        "<body><style>&\"<>\u{a0} foobar</style><script>&\"<>\u{a0} foobar</script><noscript>&amp;\"&lt;&gt;&nbsp; foobar</noscript><p foo=\"<bar>&quot;'&amp;\"></p><x:br></x:br></body>"
    );
}

#[test]
fn fragments() {
    // parser/Element_innerHTML.phpt style: fragment parsing with a context.
    let mut a = Arena::new();
    let r = parse(
        &mut a,
        "<!DOCTYPE html><table><tr><td>x</td></tr></table>",
        ParseOptions::default(),
    );
    let td = elements(&a, r.doc)
        .into_iter()
        .find(|&n| &*a.element(n).unwrap().name.local == "tr")
        .unwrap();
    let f = html5::parse_fragment(&mut a, td, "<td>a<td>b", &FragmentOptions::default());
    assert_eq!(
        html5::serialize_node(&a, f.fragment),
        "<td>a</td><td>b</td>"
    );
    let body = elements(&a, r.doc)[2];
    let f = html5::parse_fragment(
        &mut a,
        body,
        "<p>x<svg><circle/></svg>",
        &FragmentOptions::default(),
    );
    assert_eq!(
        html5::serialize_node(&a, f.fragment),
        "<p>x<svg><circle></circle></svg></p>"
    );
    // Element_outerHTML_invalid_tree.phpt: an <a> inside <a> via fragment.
    let f = html5::parse_fragment(
        &mut a,
        body,
        "<a href=\"#two\">Another Link</a>",
        &FragmentOptions::default(),
    );
    assert_eq!(
        html5::serialize_node(&a, f.fragment),
        "<a href=\"#two\">Another Link</a>"
    );
}

#[test]
fn encodings() {
    assert_eq!(
        encoding::encoding_for_label(" Latin1 "),
        Some(WhatwgEncoding::Windows1252)
    );
    assert_eq!(encoding::encoding_for_label("nonexistent"), None);
    assert_eq!(
        encoding::prescan(b"<!doctype html><head><meta charset=\"gb18030\"></head>"),
        Some(WhatwgEncoding::Gb18030)
    );
    assert_eq!(
        encoding::prescan(
            b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1251\">"
        ),
        Some(WhatwgEncoding::Windows1251)
    );
    // fallback_encoding.html: unknown charset label, UTF-8.
    let mut a = Arena::new();
    let r = html5::parse_document(
        &mut a,
        b"<!doctype html>\n<head>\n<meta charset=\"I don't exist\">\n</head>\n<body>\n",
        &ParseOptions::default(),
    );
    assert_eq!(r.encoding.name(), "UTF-8");
    // BOMs.
    let r = html5::parse_document(&mut a, b"\xFF\xFE<\0p\0>\0\xE9\0", &ParseOptions::default());
    assert_eq!(r.encoding, WhatwgEncoding::Utf16le);
    assert_eq!(
        html5::serialize_node(&a, r.doc),
        "<html><head></head><body><p>\u{e9}</p></body></html>"
    );
    let r = html5::parse_document(
        &mut a,
        b"<p>\xE9</p>",
        &ParseOptions {
            override_encoding: Some(WhatwgEncoding::Windows1252),
            ..Default::default()
        },
    );
    assert_eq!(r.encoding.name(), "windows-1252");
    assert_eq!(
        html5::serialize_node(&a, r.doc),
        "<html><head></head><body><p>\u{e9}</p></body></html>"
    );
}

#[test]
fn html5lib_smoke() {
    // A few cases from html5lib-tests tree-construction (MIT).
    for (input, out) in [
        (
            "<a><p>X<a>Y</a>Z</p></a>",
            "<html><head></head><body><a></a><p><a>X</a><a>Y</a>Z</p></body></html>",
        ),
        (
            "<table><tr><td><svg><desc><td></desc><circle>",
            "<html><head></head><body><table><tbody><tr><td><svg><desc></desc></svg></td><td><circle></circle></td></tr></tbody></table></body></html>",
        ),
        (
            "<b><i><b><i><b><i><b><i>x</b>",
            "<html><head></head><body><b><i><b><i><b><i><b><i>x</i></b></i></b></i></b></i></b></body></html>",
        ),
        (
            "<!DOCTYPE html><select><option>A<optgroup>B</select>C",
            "<!DOCTYPE html><html><head></head><body><select><option>A</option><optgroup>B</optgroup></select>C</body></html>",
        ),
    ] {
        assert_eq!(html(input, ParseOptions::default()), out, "{input:?}");
    }
}
