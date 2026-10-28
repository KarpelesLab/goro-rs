//! Schema validation: W3C XML Schema ([`xsd`]) and RELAX NG
//! ([`relaxng`]), loaded from bytes with includes going through an
//! [`EntityLoader`], with errors in the form PHP reports them (observed on
//! PHP 8.5; tests ext/dom/tests/DOMDocument_schemaValidate*.phpt and
//! DOMDocument_relaxNGValidate*.phpt).

pub mod relaxng;
pub mod xsd;

pub use xsd::Invalid;

use crate::error::{Domain, LEVEL_ERROR, XmlError};
use crate::tree::{Arena, NodeId};
use crate::xml::{EntityLoader, ParseOptions};

/// A compiled schema of either language.
#[derive(Debug)]
pub enum Compiled {
    /// W3C XML Schema.
    Xsd(xsd::Schema),
    /// RELAX NG.
    RelaxNg(relaxng::Grammar),
}

impl Compiled {
    /// Validates the document element `root`; `create` adds defaulted
    /// attributes (XML Schema, LIBXML_SCHEMA_CREATE).
    pub fn validate_root(&self, a: &mut Arena, root: NodeId, create: bool) -> Vec<XmlError> {
        let (errs, domain) = match self {
            Compiled::Xsd(s) => (s.validate_root(a, root, create), Domain::SchemasV),
            Compiled::RelaxNg(g) => (g.validate_root(a, root), Domain::RelaxNgV),
        };
        to_errors(a, errs, domain)
    }

    /// Validates document `doc`.
    pub fn validate(&self, a: &mut Arena, doc: NodeId, create: bool) -> Vec<XmlError> {
        match a.document_element(doc) {
            Some(r) => self.validate_root(a, r, create),
            None => {
                let errs = match self {
                    Compiled::Xsd(s) => s.validate(a, doc, create),
                    Compiled::RelaxNg(g) => g.validate(a, doc),
                };
                to_errors(a, errs, Domain::SchemasV)
            }
        }
    }
}

fn to_errors(a: &Arena, errs: Vec<Invalid>, domain: Domain) -> Vec<XmlError> {
    errs.into_iter()
        .map(|e| {
            let doc = a.doc_of(e.node);
            XmlError {
                domain,
                code: e.code,
                level: LEVEL_ERROR,
                message: format!("{}\n", e.message),
                file: a.doc(doc).url.clone(),
                line: a.line(e.node),
                column: 0,
            }
        })
        .collect()
}

/// A schema error without a position.
pub fn error(domain: Domain, code: u32, msg: &str) -> XmlError {
    XmlError {
        domain,
        code,
        level: LEVEL_ERROR,
        message: format!("{msg}\n"),
        file: None,
        line: 0,
        column: 0,
    }
}

/// The error for a main schema file that could not be read.
pub fn xsd_not_found(url: &str) -> XmlError {
    error(
        Domain::SchemasP,
        1767,
        &format!("Failed to locate the main schema resource at '{url}'."),
    )
}

/// The error for a main RELAX NG file that could not be read.
pub fn rng_not_found(url: &str) -> XmlError {
    error(
        Domain::RelaxNgP,
        1018,
        &format!("xmlRelaxNGParse: could not load {url}"),
    )
}

/// Parses a schema document into `a`: its document element, or the
/// errors (the parser's, then `failed`).
fn parse_doc(
    a: &mut Arena,
    input: &[u8],
    url: Option<&str>,
    loader: &mut dyn EntityLoader,
    failed: impl FnOnce() -> XmlError,
) -> Result<NodeId, Vec<XmlError>> {
    let mut opts = ParseOptions::with_flags(0);
    opts.url = url.map(str::to_string);
    let b = crate::xml::build(a, input, &opts, Some(loader));
    if !b.ok {
        let mut errs = b.errors;
        errs.push(failed());
        return Err(errs);
    }
    match a.document_element(b.doc) {
        Some(r) => Ok(r),
        None => Err(vec![failed()]),
    }
}

/// Compiles an XML Schema document (`url`: its location, `None` for a
/// string source). Included and imported schemas load through `loader`.
pub fn compile_xsd(
    input: &[u8],
    url: Option<&str>,
    loader: &mut dyn EntityLoader,
) -> Result<xsd::Schema, Vec<XmlError>> {
    let mut a = Arena::new();
    let what = url.unwrap_or("in_memory_buffer").to_string();
    let root = parse_doc(&mut a, input, url, loader, || {
        error(
            Domain::SchemasP,
            1761,
            &format!("Failed to parse the XML resource '{what}'."),
        )
    })?;
    let mut roots = vec![root];
    let mut errors = Vec::new();
    let mut i = 0;
    while i < roots.len() && roots.len() < 64 {
        let r = roots[i];
        i += 1;
        if !xsd::is_schema_root(&a, r) {
            continue;
        }
        let base = a.doc(a.doc_of(r)).url.clone();
        for (kind, loc) in xsd::includes(&a, r) {
            let full = crate::xml::resolve_system_id(&loc, base.as_deref());
            let already = roots
                .iter()
                .any(|&x| a.doc(a.doc_of(x)).url.as_deref() == Some(full.as_str()));
            if already {
                continue;
            }
            let loaded = loader.load(None, &loc, base.as_deref()).ok().and_then(|bytes| {
                parse_doc(&mut a, &bytes, Some(&full), loader, || {
                    error(Domain::SchemasP, 1761, "")
                })
                .ok()
            });
            match loaded {
                Some(nr) => roots.push(nr),
                // A failed import is only a warning in libxml; a failed
                // include makes the schema invalid.
                None if kind == "import" => {}
                None => errors.push(error(
                    Domain::SchemasP,
                    3050,
                    &format!(
                        "Element '{{{}}}{kind}': Failed to load the document '{loc}' for inclusion.",
                        xsd::XS_NS
                    ),
                )),
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    xsd::parse(&a, &roots).map_err(|v| {
        v.iter()
            .map(|m| error(Domain::SchemasP, 1761, m))
            .collect()
    })
}

/// Compiles a RELAX NG schema (`url`: its location, `None` for a string
/// source). Included grammars load through `loader`.
pub fn compile_rng(
    input: &[u8],
    url: Option<&str>,
    loader: &mut dyn EntityLoader,
) -> Result<relaxng::Grammar, Vec<XmlError>> {
    let mut a = Arena::new();
    let what = url.unwrap_or("in_memory_buffer").to_string();
    let root = parse_doc(&mut a, input, url, loader, || {
        error(
            Domain::RelaxNgP,
            1018,
            &format!("xmlRelaxNGParse: could not parse {what}"),
        )
    })?;
    relaxng::compile(&mut a, root, loader).map_err(|v| {
        v.iter()
            .map(|m| error(Domain::RelaxNgP, 1018, m))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::LoadError;

    struct NoLoad;
    impl EntityLoader for NoLoad {
        fn load(&mut self, _: Option<&str>, _: &str, _: Option<&str>) -> Result<Vec<u8>, LoadError> {
            Err(LoadError::NotFound)
        }
    }

    fn check(c: &Compiled, xml: &str) -> Vec<String> {
        let mut a = Arena::new();
        let b = crate::xml::build(&mut a, xml.as_bytes(), &ParseOptions::default(), None);
        c.validate(&mut a, b.doc, false)
            .iter()
            .map(|e| e.text().to_string())
            .collect()
    }

    // Messages observed on PHP 8.5.
    #[test]
    fn xsd_messages() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="r"><xs:complexType><xs:sequence><xs:element name="a" type="xs:int"/><xs:element name="b" minOccurs="0"/></xs:sequence><xs:attribute name="q" type="xs:int" use="required"/></xs:complexType></xs:element></xs:schema>"#;
        let c = Compiled::Xsd(compile_xsd(xsd.as_bytes(), None, &mut NoLoad).unwrap());
        assert!(check(&c, r#"<r q="1"><a>1</a></r>"#).is_empty());
        assert_eq!(
            check(&c, "<r><a>1</a></r>"),
            ["Element 'r': The attribute 'q' is required but missing."]
        );
        assert_eq!(
            check(&c, r#"<r q="1"><b/></r>"#),
            ["Element 'b': This element is not expected. Expected is ( a )."]
        );
        assert_eq!(
            check(&c, r#"<r q="1"></r>"#),
            ["Element 'r': Missing child element(s). Expected is ( a )."]
        );
        assert_eq!(
            check(&c, r#"<r q="1" z="2"><a>1</a><b/><c/></r>"#),
            [
                "Element 'r', attribute 'z': The attribute 'z' is not allowed.",
                "Element 'c': This element is not expected."
            ]
        );
        assert_eq!(
            check(&c, "<x/>"),
            ["Element 'x': No matching global declaration available for the validation root."]
        );
    }

    #[test]
    fn relaxng_messages() {
        let rng = r#"<grammar ns="" xmlns="http://relaxng.org/ns/structure/1.0" datatypeLibrary="http://www.w3.org/2001/XMLSchema-datatypes"><start><element name="apple"><attribute name="a"><data type="int"/></attribute><element name="pear"><data type="NCName"/></element><zeroOrMore><element name="x"><empty/></element></zeroOrMore></element></start></grammar>"#;
        let c = Compiled::RelaxNg(compile_rng(rng.as_bytes(), None, &mut NoLoad).unwrap());
        assert!(check(&c, r#"<apple a="1"><pear>P</pear><x/><x/></apple>"#).is_empty());
        assert_eq!(check(&c, "<foo/>"), ["Expecting element apple, got foo"]);
        assert_eq!(
            check(&c, r#"<apple a="1"><pear>P</pear><y/></apple>"#),
            ["Did not expect element y there"]
        );
        assert_eq!(
            check(&c, r#"<apple a="1" b="2"><pear>P</pear></apple>"#),
            ["Invalid attribute b for element apple"]
        );
        assert_eq!(
            check(&c, r#"<apple a="1">t<pear>P</pear></apple>"#),
            ["Did not expect text in element apple content"]
        );
        let bad = r#"<grammar xmlns="http://relaxng.org/ns/structure/1.0"><start><element name="a"></element></start></grammar>"#;
        assert!(compile_rng(bad.as_bytes(), None, &mut NoLoad).is_err());
    }
}
