//! XInclude 1.0 (W3C XML Inclusions, Second Edition) processing for
//! `DOMDocument::xinclude()` and LIBXML_XINCLUDE.
//!
//! `xi:include` elements (namespaces `http://www.w3.org/2001/XInclude` and
//! the older `http://www.w3.org/2003/XInclude`) are replaced by the
//! referenced document (or the nodes its `xpointer(...)` fragment selects,
//! through the host's XPath), or by text with `parse="text"`, or by the
//! children of `xi:fallback` when loading fails. Included top-level
//! elements get an `xml:base` attribute with the `href` (observed on
//! PHP 8.5, test ext/dom/tests/dom_xinclude.phpt). No marker nodes remain.

use crate::error::{Domain, LEVEL_ERROR, LEVEL_WARNING, XmlError};
use crate::tree::{Arena, NodeData, NodeId, QName, XML_NS};
use crate::xml::{LoadError, ParseOptions, build};

/// The XInclude namespace.
pub const XINCLUDE_NS: &str = "http://www.w3.org/2001/XInclude";
/// The pre-recommendation XInclude namespace libxml also accepts.
pub const XINCLUDE_NS_OLD: &str = "http://www.w3.org/2003/XInclude";

/// What XInclude needs from its host.
pub trait XIncludeHost {
    /// Loads a resource (`href` resolved against `base` by the host).
    fn load(&mut self, href: &str, base: Option<&str>) -> Result<Vec<u8>, LoadError>;
    /// Evaluates an XPointer `xpointer(expr)` expression (an XPath) on a
    /// loaded document; `None` when unsupported or invalid.
    fn xpath(&mut self, arena: &mut Arena, doc: NodeId, expr: &str) -> Option<Vec<NodeId>>;
}

fn is_xi(arena: &Arena, n: NodeId, local: &str) -> bool {
    arena.element(n).is_some_and(|e| {
        &*e.name.local == local
            && matches!(
                e.name.ns.as_deref(),
                Some(XINCLUDE_NS) | Some(XINCLUDE_NS_OLD)
            )
    })
}

fn err(
    errors: &mut Vec<XmlError>,
    level: u8,
    code: u32,
    msg: String,
    line: u32,
    url: &Option<String>,
) {
    errors.push(XmlError {
        domain: Domain::XInclude,
        code,
        level,
        message: format!("{msg}\n"),
        file: url.clone(),
        line,
        column: 0,
    });
}

/// Processes the XIncludes of `doc` (or of the subtree at `root`).
/// Returns the number of substitutions and the warnings/errors.
pub fn process(
    arena: &mut Arena,
    root: NodeId,
    flags: u32,
    host: &mut dyn XIncludeHost,
) -> (usize, Vec<XmlError>) {
    let mut errors = Vec::new();
    let mut count = 0;
    process_inner(arena, root, flags, host, &mut errors, &mut count, 0);
    (count, errors)
}

fn process_inner(
    arena: &mut Arena,
    root: NodeId,
    flags: u32,
    host: &mut dyn XIncludeHost,
    errors: &mut Vec<XmlError>,
    count: &mut usize,
    depth: usize,
) {
    if depth > 40 {
        return;
    }
    let doc = arena.doc_of(root);
    let url = arena.doc(doc).url.clone();
    let includes: Vec<NodeId> = std::iter::once(root)
        .chain(arena.descendants(root))
        .filter(|&n| is_xi(arena, n, "include"))
        // Not inside a fallback of another include.
        .filter(|&n| {
            let mut p = arena.parent(n);
            while let Some(x) = p {
                if is_xi(arena, x, "fallback") {
                    return false;
                }
                p = arena.parent(x);
            }
            true
        })
        .collect();
    for inc in includes {
        if !arena.is_live(inc) || arena.parent(inc).is_none() {
            continue;
        }
        let line = arena.line(inc);
        let href = arena.get_attribute(inc, "href").unwrap_or_default();
        let parse = arena
            .get_attribute(inc, "parse")
            .unwrap_or_else(|| "xml".into());
        let (file, fragment) = match href.split_once('#') {
            Some((f, x)) => (f.to_string(), Some(x.to_string())),
            None => (href.clone(), None),
        };
        let xpointer = arena.get_attribute(inc, "xpointer").or(fragment);
        let doc_of_inc = arena.doc_of(inc);
        let mut replacement: Option<Vec<NodeId>> = None;
        match host.load(&file, url.as_deref()) {
            Ok(bytes) => {
                if parse == "text" {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    replacement = Some(vec![arena.new_text(doc_of_inc, text)]);
                } else {
                    let mut po = ParseOptions::with_flags(flags);
                    po.url = Some(file.clone());
                    let b = build(arena, &bytes, &po, None);
                    if b.ok {
                        process_inner(arena, b.doc, flags, host, errors, count, depth + 1);
                        let selected: Vec<NodeId> = match &xpointer {
                            Some(xp) => {
                                let expr = xp
                                    .strip_prefix("xpointer(")
                                    .and_then(|s| s.strip_suffix(')'));
                                match expr.and_then(|e| host.xpath(arena, b.doc, e)) {
                                    Some(v) => v,
                                    None => {
                                        err(
                                            errors,
                                            LEVEL_ERROR,
                                            1612,
                                            format!("XPointer evaluation failed: #{xp}"),
                                            line,
                                            &url,
                                        );
                                        Vec::new()
                                    }
                                }
                            }
                            None => arena
                                .children(b.doc)
                                .filter(|&c| !matches!(arena.data(c), NodeData::Doctype(_)))
                                .collect(),
                        };
                        let mut copies = Vec::new();
                        for s in selected {
                            let c = arena.clone_node(s, doc_of_inc, true);
                            if arena.is_element(c) && file != url.clone().unwrap_or_default() {
                                set_base(arena, c, &file);
                            }
                            copies.push(c);
                        }
                        replacement = Some(copies);
                    } else {
                        errors.extend(b.errors);
                    }
                    arena.free_subtree(b.doc);
                }
            }
            Err(LoadError::NotFound) => {
                err(
                    errors,
                    LEVEL_WARNING,
                    1549,
                    format!("I/O warning : failed to load \"{file}\""),
                    line,
                    &url,
                );
            }
            Err(LoadError::Refused) => {}
        }
        let nodes = match replacement {
            Some(v) => v,
            None => {
                // Fallback.
                let fb = arena.children(inc).find(|&c| is_xi(arena, c, "fallback"));
                match fb {
                    Some(f) => {
                        let kids: Vec<NodeId> = arena.children(f).collect();
                        for &k in &kids {
                            arena.detach(k);
                        }
                        kids
                    }
                    None => {
                        err(
                            errors,
                            LEVEL_ERROR,
                            1604,
                            format!("could not load {href}, and no fallback was found"),
                            line,
                            &url,
                        );
                        continue;
                    }
                }
            }
        };
        let parent = arena.parent(inc).unwrap();
        for n in nodes {
            arena.raw_insert_before(parent, n, Some(inc));
            arena.register_ids(n);
        }
        arena.detach(inc);
        arena.free_subtree(inc);
        *count += 1;
    }
}

fn set_base(arena: &mut Arena, el: NodeId, base: &str) {
    if arena.get_attr_node_ns(el, Some(XML_NS), "base").is_some() {
        return;
    }
    let doc = arena.doc_of(el);
    let a = arena.new_attribute(
        doc,
        QName {
            prefix: Some("xml".into()),
            local: "base".into(),
            ns: Some(XML_NS.into()),
        },
        base,
    );
    arena.raw_append_attr(el, a);
}
