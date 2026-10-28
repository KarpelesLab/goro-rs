//! The WHATWG HTML parser and serializer for `Dom\HTMLDocument`.
//!
//! * [`parse_document`] / [`parse_document_str`]: `Dom\HTMLDocument::
//!   createFromString` and `createFromFile` (encoding sniffing, tokenizer,
//!   tree construction).
//! * [`parse_fragment`]: the HTML fragment parsing algorithm with a context
//!   element (`innerHTML`/`outerHTML` setters, `insertAdjacentHTML`).
//! * [`serialize`]: the HTML fragment serialization algorithm (`saveHtml`,
//!   `innerHTML`/`outerHTML` getters).
//!
//! Documents are built in a [`tree::Arena`](crate::tree::Arena) with
//! [`DocKind::Html5`](crate::tree::DocKind) and `modern = true`. HTML
//! elements are in the HTML namespace (or in no namespace with
//! [`ParseOptions::no_default_ns`], `Dom\HTML_NO_DEFAULT_NS`), SVG and
//! MathML elements in theirs; prefixes are always `None`. `xmlns`
//! attributes are plain attributes (in the XMLNS namespace when adjusted on
//! foreign elements); nothing is added to `ElementData::ns_decls`.
//!
//! Line numbers: `Node::line` holds the 0-based line of the token that
//! created the node, which is what PHP's `getLineNo()` reports for these
//! documents (observed on PHP 8.5: the `<html>` start tag on the second
//! line gives 1; implied `html`/`head`/`body` elements give 0). Attributes
//! get their element's line.

pub mod encoding;
mod encoding_tables;
mod entities;
pub mod serialize;
mod tags;
mod tokenizer;
mod treebuilder;

use crate::tree::{Arena, DocKind, NodeData, NodeId, QuirksMode, Str};
pub use encoding::WhatwgEncoding;
pub use serialize::{serialize_children, serialize_node};
use tags::T;
use treebuilder::{BuildOptions, Context, Ns, TreeBuilder};

/// Which stage reported a parse error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The tokenizer (WHATWG error codes such as `missing-end-tag-name`).
    Tokenizer,
    /// Tree construction (PHP's codes such as `unexpected-token`).
    Tree,
}

/// A parse error as PHP reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Tokenizer or tree error.
    pub kind: ErrorKind,
    /// The error code.
    pub code: &'static str,
    /// 1-based line.
    pub line: u32,
    /// 1-based column (in characters) of the start of the offending token
    /// (tree errors) or of the current character (tokenizer errors).
    pub column: u32,
    /// 1-based column of the end of the token (equal to `column` for
    /// tokenizer errors and one-character spans).
    pub column_end: u32,
}

impl ParseError {
    /// The warning text PHP prints, e.g. `tree error unexpected-token in
    /// Entity, line: 8, column: 7-10` (`source` is `Entity` for
    /// `createFromString`, the path for `createFromFile`).
    pub fn message(&self, source: &str) -> String {
        let kind = match self.kind {
            ErrorKind::Tokenizer => "tokenizer",
            ErrorKind::Tree => "tree",
        };
        if self.column_end != self.column {
            format!(
                "{kind} error {} in {source}, line: {}, column: {}-{}",
                self.code, self.line, self.column, self.column_end
            )
        } else {
            format!(
                "{kind} error {} in {source}, line: {}, column: {}",
                self.code, self.line, self.column
            )
        }
    }
}

/// Options of [`parse_document`].
#[derive(Clone, Debug, Default)]
pub struct ParseOptions {
    /// `$overrideEncoding` (already validated with
    /// [`encoding::encoding_for_label`]).
    pub override_encoding: Option<WhatwgEncoding>,
    /// The charset of the transport layer (`Content-Type` of an HTTP
    /// stream), if any.
    pub transport_encoding: Option<WhatwgEncoding>,
    /// `Dom\HTML_NO_DEFAULT_NS`: HTML elements get no namespace.
    pub no_default_ns: bool,
    /// `LIBXML_HTML_NOIMPLIED`: implied `html`, `head` and `body` elements
    /// are removed (their children take their place) and the missing
    /// doctype is not reported.
    pub no_implied: bool,
    /// Collect parse errors (false for `LIBXML_NOERROR`, which also makes
    /// parsing a bit faster).
    pub collect_errors: bool,
    /// Follow the current WHATWG standard where PHP 8.5 behaves
    /// differently (`<search>` element, `</p>` and `</br>` in foreign
    /// content, `xml:lang` on foreign elements, comments with `--!`).
    /// Off by default; the html5lib-tests runner turns it on.
    pub standard: bool,
}

/// The result of [`parse_document`].
#[derive(Debug)]
pub struct ParsedDocument {
    /// The new document node.
    pub doc: NodeId,
    /// Tokenizer errors first, then tree errors, each in source order (the
    /// order PHP emits its warnings in).
    pub errors: Vec<ParseError>,
    /// The encoding the input was decoded with (`$charset`).
    pub encoding: WhatwgEncoding,
}

/// Parses an HTML document from bytes, determining the encoding as
/// `Dom\HTMLDocument::createFromString` does.
pub fn parse_document(arena: &mut Arena, input: &[u8], opts: &ParseOptions) -> ParsedDocument {
    let sn = encoding::sniff(input, opts.override_encoding, opts.transport_encoding);
    let text = encoding::decode(&input[sn.bom_len..], sn.encoding);
    let mut r = parse_document_str(arena, text, opts);
    r.encoding = sn.encoding;
    arena.doc_mut(r.doc).encoding = Some(sn.encoding.name().to_string());
    r
}

/// Parses an HTML document from text that is already decoded.
pub fn parse_document_str(arena: &mut Arena, input: String, opts: &ParseOptions) -> ParsedDocument {
    let doc = arena.new_document(DocKind::Html5);
    {
        let d = arena.doc_mut(doc);
        d.modern = true;
        d.encoding = Some("UTF-8".into());
        d.content_type = Some("text/html".into());
    }
    let mut tok = tokenizer::Tokenizer::new(tokenizer::preprocess(input));
    tok.collect_errors = opts.collect_errors;
    tok.standard = opts.standard;
    let bopts = BuildOptions {
        no_default_ns: opts.no_default_ns,
        collect_errors: opts.collect_errors,
        scripting: false,
        no_implied: opts.no_implied,
        standard: opts.standard,
    };
    let mut tb = TreeBuilder::new(arena, doc, doc, tok, bopts);
    tb.run();
    let quirks = tb.quirks;
    let explicit = std::mem::take(&mut tb.explicit);
    let errors = collect_errors(&tb);
    drop(tb);
    arena.doc_mut(doc).quirks = quirks;
    if opts.no_implied {
        remove_implied(arena, doc, &explicit);
    }
    ParsedDocument {
        doc,
        errors,
        encoding: WhatwgEncoding::Utf8,
    }
}

/// Unwraps the implied `html`, `head` and `body` elements
/// (LIBXML_HTML_NOIMPLIED, observed on PHP 8.5).
fn remove_implied(arena: &mut Arena, doc: NodeId, explicit: &[NodeId]) {
    let Some(html) = arena.document_element(doc) else {
        return;
    };
    let kids: Vec<NodeId> = arena.children(html).collect();
    for k in kids {
        if !explicit.contains(&k)
            && arena
                .element(k)
                .is_some_and(|e| matches!(&*e.name.local, "head" | "body"))
        {
            unwrap_node(arena, k);
        }
    }
    if !explicit.contains(&html) {
        unwrap_node(arena, html);
    }
}

fn unwrap_node(arena: &mut Arena, n: NodeId) {
    let parent = arena.parent(n).expect("parent");
    while let Some(c) = arena.first_child(n) {
        arena.detach(c);
        arena.raw_insert_before(parent, c, Some(n));
    }
    arena.detach(n);
    arena.free_subtree(n);
}

fn collect_errors(tb: &TreeBuilder<'_>) -> Vec<ParseError> {
    let input = tb.tok.input();
    let mut offs: Vec<usize> = Vec::new();
    for e in &tb.tok.errors {
        offs.push(e.pos);
    }
    for e in &tb.errors {
        offs.push(e.first);
    }
    offs.sort_unstable();
    offs.dedup();
    // One sweep: (offset -> (line, column)).
    let mut pos: Vec<(u32, u32)> = Vec::with_capacity(offs.len());
    let (mut line, mut col, mut at) = (1u32, 1u32, 0usize);
    for &o in &offs {
        let o = o.min(input.len());
        for &b in &input[at..o] {
            if b == b'\n' {
                line += 1;
                col = 1;
            } else if (b & 0xC0) != 0x80 {
                col += 1;
            }
        }
        at = o;
        pos.push((line, col));
    }
    let lookup = |o: usize| pos[offs.binary_search(&o).unwrap()];
    let mut out = Vec::with_capacity(offs.len());
    for e in &tb.tok.errors {
        let (l, c) = lookup(e.pos);
        out.push(ParseError {
            kind: ErrorKind::Tokenizer,
            code: e.code,
            line: l,
            column: c,
            column_end: c,
        });
    }
    for e in &tb.errors {
        let (l, c) = lookup(e.first);
        // PHP's end column is the start column plus the length of the
        // token in bytes, minus one (observed on PHP 8.5: the tag name
        // `a\u{100000}` at column 2 is reported as 2-6).
        let len = e.end.saturating_sub(e.first) as u32;
        out.push(ParseError {
            kind: ErrorKind::Tree,
            code: e.code,
            line: l,
            column: c,
            column_end: (c + len.saturating_sub(1)).max(c),
        });
    }
    out
}

/// Options of [`parse_fragment`].
#[derive(Clone, Debug, Default)]
pub struct FragmentOptions {
    /// Collect parse errors.
    pub collect_errors: bool,
    /// See [`ParseOptions::standard`].
    pub standard: bool,
}

/// The result of [`parse_fragment`].
#[derive(Debug)]
pub struct ParsedFragment {
    /// A detached document fragment (owned by the context element's
    /// document) holding the parsed nodes.
    pub fragment: NodeId,
    /// Parse errors.
    pub errors: Vec<ParseError>,
}

/// The HTML fragment parsing algorithm (WHATWG HTML "13.4 Parsing HTML
/// fragments") with `context` as the context element. The nodes are
/// created in `context`'s document; they are returned as the children of
/// a new, detached fragment node.
///
/// The context element's namespace decides how it is treated: HTML
/// namespace (or no namespace in a document parsed with
/// `HTML_NO_DEFAULT_NS`) means an HTML element, SVG/MathML their own.
pub fn parse_fragment(
    arena: &mut Arena,
    context: NodeId,
    input: &str,
    opts: &FragmentOptions,
) -> ParsedFragment {
    let owner = arena.doc_of(context);
    let quirks = match &arena.node(owner).data {
        NodeData::Document(d) => d.quirks,
        _ => QuirksMode::NoQuirks,
    };
    let no_default_ns = arena.element(context).is_some_and(|e| e.name.ns.is_none());
    let (ns, local): (Ns, Str) = match arena.element(context) {
        Some(e) => {
            let ns = match e.name.ns.as_deref() {
                Some(crate::tree::SVG_NS) => Ns::Svg,
                Some(crate::tree::MATHML_NS) => Ns::Math,
                _ => Ns::Html,
            };
            (ns, e.name.local.clone())
        }
        None => (Ns::Html, Str::from("body")),
    };
    let t = if ns == Ns::Html {
        match T::from_name(&local.to_ascii_lowercase()) {
            T::Search if !opts.standard => T::Other,
            t => t,
        }
    } else {
        T::Other
    };
    let html_ip = ns == Ns::Math
        && &*local == "annotation-xml"
        && arena.get_attribute(context, "encoding").is_some_and(|v| {
            v.eq_ignore_ascii_case("text/html") || v.eq_ignore_ascii_case("application/xhtml+xml")
        });
    // Nearest form ancestor (inclusive) of the context.
    let mut form = None;
    let mut cur = Some(context);
    while let Some(c) = cur {
        if let Some(e) = arena.element(c)
            && &*e.name.local == "form"
            && e.name
                .ns
                .as_deref()
                .is_none_or(|n| n == crate::tree::HTML_NS)
        {
            form = Some(c);
            break;
        }
        cur = arena.parent(c);
    }
    let scratch = arena.new_document(DocKind::Html5);
    let mut tok = tokenizer::Tokenizer::new(tokenizer::preprocess(input.to_string()));
    tok.collect_errors = opts.collect_errors;
    tok.standard = opts.standard;
    let bopts = BuildOptions {
        no_default_ns,
        collect_errors: opts.collect_errors,
        scripting: false,
        no_implied: false,
        standard: opts.standard,
    };
    let mut tb = TreeBuilder::new(arena, scratch, owner, tok, bopts);
    tb.init_fragment(
        Context {
            node: context,
            ns,
            t,
            name: if ns == Ns::Html {
                Str::from(local.to_ascii_lowercase())
            } else {
                local
            },
            html_ip,
        },
        form,
        quirks,
    );
    tb.run();
    let root = tb.fragment_root().expect("fragment root");
    let errors = collect_errors(&tb);
    drop(tb);
    let frag = arena.create(owner, NodeData::Fragment);
    while let Some(c) = arena.first_child(root) {
        arena.detach(c);
        arena.raw_append(frag, c);
    }
    arena.free_subtree(scratch);
    ParsedFragment {
        fragment: frag,
        errors,
    }
}
