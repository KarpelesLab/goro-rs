//! C14N: cases from the expected output of ext/dom/tests/canonicalization.phpt
//! (those without an XPath node-set).

use goro_xml::Arena;
use goro_xml::c14n::{C14nOptions, Mode, canonicalize};
use goro_xml::xml::{ParseOptions, build};

const XML: &str = "<?xml version=\"1.0\" encoding=\"ISO-8859-1\" ?>\n<foo xmlns=\"http://www.example.com/ns/foo\"\n     xmlns:fubar=\"http://www.example.com/ns/fubar\" xmlns:test=\"urn::test\"><contain>\n  <bar><test1 /></bar>\n  <bar><test2 /></bar>\n  <fubar:bar xmlns:fubar=\"http://www.example.com/ns/fubar\"><test3 /></fubar:bar>\n  <fubar:bar><test4 /></fubar:bar>\n<!-- this is a comment -->\n</contain>\n</foo>";

const BODY: &str = "  <bar><test1></test1></bar>\n  <bar><test2></test2></bar>\n";

fn run(mode: Mode, comments: bool, prefixes: &[&str]) -> String {
    let mut a = Arena::new();
    let b = build(&mut a, XML.as_bytes(), &ParseOptions::default(), None);
    let contain = a.first_child(a.document_element(b.doc).unwrap()).unwrap();
    let opts = C14nOptions {
        mode,
        with_comments: comments,
        inclusive_prefixes: prefixes.iter().map(|s| s.to_string()).collect(),
        node_set: None,
    };
    canonicalize(&a, contain, &opts)
}

#[test]
fn inclusive() {
    let want = format!(
        "<contain xmlns=\"http://www.example.com/ns/foo\" xmlns:fubar=\"http://www.example.com/ns/fubar\" xmlns:test=\"urn::test\">\n{BODY}  <fubar:bar><test3></test3></fubar:bar>\n  <fubar:bar><test4></test4></fubar:bar>\n\n</contain>"
    );
    assert_eq!(run(Mode::Inclusive, false, &[]), want);
    let want_c = want.replace("\n\n</contain>", "\n<!-- this is a comment -->\n</contain>");
    assert_eq!(run(Mode::Inclusive, true, &[]), want_c);
}

#[test]
fn exclusive() {
    let f = "<fubar:bar xmlns:fubar=\"http://www.example.com/ns/fubar\">";
    let want = format!(
        "<contain xmlns=\"http://www.example.com/ns/foo\">\n{BODY}  {f}<test3></test3></fubar:bar>\n  {f}<test4></test4></fubar:bar>\n\n</contain>"
    );
    assert_eq!(run(Mode::Exclusive, false, &[]), want);
    let want_c = want.replace("\n\n</contain>", "\n<!-- this is a comment -->\n</contain>");
    assert_eq!(run(Mode::Exclusive, true, &[]), want_c);
    let want_p = want.replace(
        "<contain xmlns=\"http://www.example.com/ns/foo\">",
        "<contain xmlns=\"http://www.example.com/ns/foo\" xmlns:test=\"urn::test\">",
    );
    assert_eq!(run(Mode::Exclusive, false, &["test"]), want_p);
}
