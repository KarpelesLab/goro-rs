//! W3C XML Conformance Test Suite runner (xmlts20130923).
//!
//! Ignored by default: download the suite and point `XMLCONF` at its
//! `xmlconf` directory, then run
//! `XMLCONF=/path/xmlconf cargo test -p goro-xml --test conformance -- --ignored --nocapture`.
//!
//! Well-formedness: `not-wf` documents must be rejected, `valid` and
//! `invalid` ones accepted (external entities loaded). Validity: with
//! DTD validation on, `valid` documents must produce no validity errors and
//! `invalid` ones at least one. XML 1.1-only tests are skipped.

use std::path::{Path, PathBuf};

use goro_xml::Arena;
use goro_xml::tree::NodeData;
use goro_xml::xml::{EntityLoader, LoadError, ParseOptions, build, opt, validate};

struct FileLoader;

impl EntityLoader for FileLoader {
    fn load(
        &mut self,
        _public: Option<&str>,
        system: &str,
        base: Option<&str>,
    ) -> Result<Vec<u8>, LoadError> {
        let p = goro_xml::xml::resolve_system_id(system, base);
        let p = p.strip_prefix("file://").unwrap_or(&p).to_string();
        std::fs::read(p).map_err(|_| LoadError::NotFound)
    }
}

struct Test {
    kind: String,
    uri: PathBuf,
    id: String,
    version: Option<String>,
    edition: Option<String>,
    namespace: bool,
}

fn list_tests(root: &Path) -> Vec<Test> {
    let conf = std::fs::read_to_string(root.join("xmlconf.xml")).unwrap();
    let mut files = Vec::new();
    for line in conf.lines() {
        if let Some(i) = line.find("SYSTEM \"") {
            let rest = &line[i + 8..];
            if let Some(j) = rest.find('"') {
                files.push(rest[..j].to_string());
            }
        }
    }
    let mut out = Vec::new();
    for f in files {
        let path = root.join(&f);
        let Ok(mut bytes) = std::fs::read(&path) else {
            continue;
        };
        if f.ends_with(".dtd") {
            continue;
        }
        // Some lists are external entities (no single root): wrap them.
        let s = String::from_utf8_lossy(&bytes).into_owned();
        let body = match s.find("?>") {
            Some(i) if s.starts_with("<?xml") => &s[i + 2..],
            _ => &s[..],
        };
        if !body.contains("<TESTCASES")
            || body.matches("<TESTCASES").count() > 1
            || !body.trim_start().starts_with('<')
        {
            let body = body.replace("<!DOCTYPE", "<!--DOCTYPE");
            let body = if body.contains("<!--DOCTYPE") {
                body.replacen(">", "-->", 1)
            } else {
                body
            };
            bytes = format!("<r>{body}</r>").into_bytes();
        }
        let mut arena = Arena::new();
        let b = build(&mut arena, &bytes, &ParseOptions::default(), None);
        if !b.ok {
            eprintln!("cannot parse test list {f}");
            continue;
        }
        let dir = path.parent().unwrap().to_path_buf();
        for n in arena.descendants(b.doc).collect::<Vec<_>>() {
            let Some(e) = arena.element(n) else { continue };
            if &*e.name.local != "TEST" {
                continue;
            }
            let get = |k: &str| arena.get_attribute(n, k);
            let base = get("xml:base");
            let mut d = dir.clone();
            if let Some(b) = base {
                d = d.join(b);
            }
            // xml:base on ancestors (TESTCASES).
            let mut p = arena.parent(n);
            while let Some(x) = p {
                if let Some(b) = arena.get_attribute(x, "xml:base") {
                    d = dir.join(b);
                    break;
                }
                p = arena.parent(x);
            }
            out.push(Test {
                kind: get("TYPE").unwrap_or_default(),
                uri: d.join(get("URI").unwrap_or_default()),
                id: get("ID").unwrap_or_default(),
                version: get("VERSION"),
                edition: get("EDITION"),
                namespace: get("NAMESPACE").as_deref() != Some("no"),
            });
        }
    }
    out
}

#[test]
#[ignore]
fn xml_conformance() {
    let Ok(root) = std::env::var("XMLCONF") else {
        eprintln!("XMLCONF not set");
        return;
    };
    let root = PathBuf::from(root);
    let tests = list_tests(&root);
    let (mut wf_pass, mut wf_total, mut v_pass, mut v_total) = (0, 0, 0, 0);
    let verbose = std::env::var("XMLCONF_VERBOSE").is_ok();
    for t in &tests {
        if t.version
            .as_deref()
            .is_some_and(|v| v.contains("1.1") && !v.contains("1.0"))
        {
            continue;
        }
        if t.edition
            .as_deref()
            .is_some_and(|e| !e.split_whitespace().any(|x| x == "5"))
        {
            continue;
        }
        if t.kind == "error" {
            continue;
        }
        let Ok(bytes) = std::fs::read(&t.uri) else {
            continue;
        };
        let url = t.uri.to_string_lossy().into_owned();
        let mut po = ParseOptions::with_flags(opt::DTDLOAD | opt::NOENT);
        po.url = Some(url.clone());
        po.namespaces = t.namespace;
        let mut arena = Arena::new();
        let mut loader = FileLoader;
        let b = build(&mut arena, &bytes, &po, Some(&mut loader));
        let fatal = !b.ok;
        let expect_fail = t.kind == "not-wf";
        wf_total += 1;
        if fatal == expect_fail {
            wf_pass += 1;
        } else if verbose {
            let first = b
                .errors
                .iter()
                .find(|e| e.level == 3)
                .map(|e| e.text().to_string());
            eprintln!("WF FAIL {} ({}): {:?} {}", t.id, t.kind, first, url);
        }
        if !fatal && (t.kind == "valid" || t.kind == "invalid") {
            let errs = validate::validate(&arena, b.doc);
            let has_dtd = matches!(
                arena.doc(b.doc).doctype.map(|d| arena.data(d)),
                Some(NodeData::Doctype(_))
            );
            v_total += 1;
            let invalid = !errs.is_empty() || (!has_dtd && t.kind == "invalid");
            if invalid == (t.kind == "invalid") {
                v_pass += 1;
            } else if verbose {
                eprintln!(
                    "VALID FAIL {} ({}): {:?}",
                    t.id,
                    t.kind,
                    errs.first().map(|e| e.text().to_string())
                );
            }
        }
    }
    eprintln!("well-formedness: {wf_pass}/{wf_total}; validity: {v_pass}/{v_total}");
}
