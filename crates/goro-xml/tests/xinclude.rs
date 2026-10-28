//! XInclude: the substitution and fallback behaviours of
//! ext/dom/tests/dom_xinclude.phpt and
//! DOMDocument_getElementsByTagName_liveness_xinclude.phpt.

use goro_xml::Arena;
use goro_xml::serialize::{SaveOptions, save_xml};
use goro_xml::tree::NodeId;
use goro_xml::xinclude::{XIncludeHost, process};
use goro_xml::xml::{LoadError, ParseOptions, build};

struct Host;

const BOOKS: &str = "<?xml version=\"1.0\"?>\n<books>\n <book>\n  <title>The Grapes of Wrath</title>\n </book>\n <book>\n  <title>The Pearl</title>\n </book>\n</books>\n";

impl XIncludeHost for Host {
    fn load(&mut self, href: &str, _base: Option<&str>) -> Result<Vec<u8>, LoadError> {
        if href == "books.xml" {
            Ok(BOOKS.as_bytes().to_vec())
        } else {
            Err(LoadError::NotFound)
        }
    }
    fn xpath(&mut self, arena: &mut Arena, doc: NodeId, expr: &str) -> Option<Vec<NodeId>> {
        // Enough for "/books/book".
        assert_eq!(expr, "/books/book");
        let root = arena.document_element(doc)?;
        Some(
            arena
                .children(root)
                .filter(|&c| arena.is_element(c))
                .collect(),
        )
    }
}

#[test]
fn xpointer_include() {
    let mut a = Arena::new();
    let src = "<?xml version=\"1.0\"?>\n<foo xmlns:xi=\"http://www.w3.org/2001/XInclude\">\n    <xi:include href=\"books.xml#xpointer(/books/book)\"/>\n </foo>";
    let b = build(&mut a, src.as_bytes(), &ParseOptions::default(), None);
    let (n, errs) = process(&mut a, b.doc, 0, &mut Host);
    assert_eq!(n, 1);
    assert!(errs.is_empty(), "{errs:?}");
    let out = String::from_utf8(save_xml(&a, b.doc, &SaveOptions::default())).unwrap();
    assert_eq!(
        out,
        "<?xml version=\"1.0\"?>\n<foo xmlns:xi=\"http://www.w3.org/2001/XInclude\">\n    <book xml:base=\"books.xml\">\n  <title>The Grapes of Wrath</title>\n </book><book xml:base=\"books.xml\">\n  <title>The Pearl</title>\n </book>\n </foo>\n"
    );
}

#[test]
fn fallback() {
    let mut a = Arena::new();
    let src = "<?xml version=\"1.0\" ?>\n<chapter xmlns:xi=\"http://www.w3.org/2001/XInclude\">\n <p>Hello</p>\n <para>\n  <xi:include href=\"book.xml\">\n   <xi:fallback>\n    <p>xinclude: book.xml not found</p>\n   </xi:fallback>\n  </xi:include>\n </para>\n</chapter>";
    let b = build(&mut a, src.as_bytes(), &ParseOptions::default(), None);
    let (n, _) = process(&mut a, b.doc, 0, &mut Host);
    assert_eq!(n, 1);
    let out = String::from_utf8(save_xml(&a, b.doc, &SaveOptions::default())).unwrap();
    assert_eq!(
        out,
        "<?xml version=\"1.0\"?>\n<chapter xmlns:xi=\"http://www.w3.org/2001/XInclude\">\n <p>Hello</p>\n <para>\n  \n    <p>xinclude: book.xml not found</p>\n   \n </para>\n</chapter>\n"
    );
}
