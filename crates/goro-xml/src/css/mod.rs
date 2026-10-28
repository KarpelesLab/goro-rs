//! CSS selectors (Selectors Level 4, as the WHATWG DOM "match a selector
//! against a tree" uses them) for `Dom\ParentNode::querySelector()`,
//! `querySelectorAll()`, `Dom\Element::matches()` and `closest()`.
//!
//! ```ignore
//! let sel = css::parse("div > p.note, :is(h1, h2)")?;
//! let found = css::query_selector_all(&arena, root, &sel);
//! ```
//!
//! Behaviour follows PHP 8.5 (observed on PHP 8.5, and the expected
//! output of ext/dom/tests/modern/css_selectors): in HTML documents, type
//! selectors and attribute names are matched case-insensitively against
//! HTML elements (the selector is lowercased), attribute values of the
//! attributes HTML lists as case-insensitive (`lang`, `type`, `charset`,
//! ...) compare case-insensitively, and in quirks mode ids and classes do
//! too. Namespace prefixes in type selectors are accepted and ignored;
//! attribute selectors match attributes without namespace. Interaction
//! pseudo-classes (`:hover`, `:focus`, `:active`) never match; others
//! PHP does not support raise "Not supported" errors.

mod parser;

use parser::{AttrOp, AttrSel, Combinator, Complex, Compound, Nth, Pseudo};

use crate::tree::{Arena, DocKind, HTML_NS, NodeData, NodeId, QuirksMode};

/// The kinds of selector errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssErrorKind {
    /// An invalid or unsupported selector: PHP throws a `DOMException`
    /// with code 12 (`SyntaxError`) and the message
    /// `Invalid selector (Selectors. DETAIL)`.
    Syntax,
    /// The column combinator `||`: PHP throws a `ValueError`
    /// "Argument #1 ($selectors) contains an unsupported selector".
    Unsupported,
    /// `:blank`: PHP throws a `DOMException` with the message as is.
    Blank,
}

/// A selector error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssError {
    /// The kind.
    pub kind: CssErrorKind,
    /// For `Syntax`: the detail (`Unexpected token: @invalid`,
    /// `Not supported: lang`, ...); otherwise the full message.
    pub message: String,
}

impl CssError {
    fn syntax(detail: String) -> CssError {
        CssError {
            kind: CssErrorKind::Syntax,
            message: detail,
        }
    }

    /// The exception message PHP uses (for `Syntax`:
    /// `Invalid selector (Selectors. DETAIL)`).
    pub fn exception_message(&self) -> String {
        match self.kind {
            CssErrorKind::Syntax => format!("Invalid selector (Selectors. {})", self.message),
            _ => self.message.clone(),
        }
    }
}

/// A parsed selector list.
#[derive(Clone, Debug)]
pub struct SelectorList {
    list: Vec<Complex>,
}

/// Parses a selector list.
pub fn parse(selectors: &str) -> Result<SelectorList, CssError> {
    Ok(SelectorList {
        list: parser::parse(selectors)?,
    })
}

impl SelectorList {
    /// Whether element `el` matches (`Element::matches()`).
    pub fn matches(&self, arena: &Arena, el: NodeId) -> bool {
        let m = Matcher::new(arena, el);
        self.list.iter().any(|c| m.complex(c, el, None))
    }
}

/// The first matching inclusive descendant... of `root` (descendants only,
/// in document order): `querySelector()`.
pub fn query_selector(arena: &Arena, root: NodeId, sel: &SelectorList) -> Option<NodeId> {
    let m = Matcher::new(arena, root);
    arena
        .descendants(root)
        .find(|&n| arena.is_element(n) && sel.list.iter().any(|c| m.complex(c, n, None)))
}

/// All matching descendants of `root` in document order:
/// `querySelectorAll()`.
pub fn query_selector_all(arena: &Arena, root: NodeId, sel: &SelectorList) -> Vec<NodeId> {
    let m = Matcher::new(arena, root);
    arena
        .descendants(root)
        .filter(|&n| arena.is_element(n) && sel.list.iter().any(|c| m.complex(c, n, None)))
        .collect()
}

/// The closest inclusive ancestor element of `el` that matches:
/// `closest()`.
pub fn closest(arena: &Arena, el: NodeId, sel: &SelectorList) -> Option<NodeId> {
    let m = Matcher::new(arena, el);
    let mut cur = Some(el);
    while let Some(n) = cur {
        if arena.is_element(n) && sel.list.iter().any(|c| m.complex(c, n, None)) {
            return Some(n);
        }
        cur = arena.parent(n);
    }
    None
}

/// Attributes whose values HTML compares case-insensitively in selectors
/// (Selectors Level 4 / HTML "Case-sensitivity of selectors").
const CI_ATTRS: &[&str] = &[
    "accept",
    "accept-charset",
    "align",
    "alink",
    "axis",
    "bgcolor",
    "charset",
    "checked",
    "clear",
    "codetype",
    "color",
    "compact",
    "declare",
    "defer",
    "dir",
    "direction",
    "disabled",
    "enctype",
    "face",
    "frame",
    "hreflang",
    "http-equiv",
    "lang",
    "language",
    "link",
    "media",
    "method",
    "multiple",
    "nohref",
    "noresize",
    "noshade",
    "nowrap",
    "readonly",
    "rel",
    "rev",
    "rules",
    "scope",
    "scrolling",
    "selected",
    "shape",
    "target",
    "text",
    "type",
    "valign",
    "valuetype",
    "vlink",
];

struct Matcher<'a> {
    arena: &'a Arena,
    html_doc: bool,
    quirks: bool,
}

impl<'a> Matcher<'a> {
    fn new(arena: &'a Arena, any: NodeId) -> Matcher<'a> {
        let doc = arena.doc_of(any);
        let (html_doc, quirks) = match arena.data(doc) {
            NodeData::Document(d) => (
                matches!(d.kind, DocKind::Html | DocKind::Html5),
                d.quirks == QuirksMode::Quirks,
            ),
            _ => (false, false),
        };
        Matcher {
            arena,
            html_doc,
            quirks,
        }
    }

    /// An HTML-namespace element (or a namespace-less element of an HTML
    /// document parsed with `HTML_NO_DEFAULT_NS`).
    fn is_html(&self, el: NodeId) -> bool {
        match self.arena.element(el) {
            Some(e) => match e.name.ns.as_deref() {
                Some(HTML_NS) => true,
                None => {
                    self.html_doc && self.arena.doc(self.arena.doc_of(el)).kind == DocKind::Html5
                }
                _ => false,
            },
            None => false,
        }
    }

    fn html_in_html_doc(&self, el: NodeId) -> bool {
        self.html_doc && self.is_html(el)
    }

    fn local(&self, el: NodeId) -> &str {
        &self.arena.element(el).unwrap().name.local
    }

    fn is_html_named(&self, el: NodeId, names: &[&str]) -> bool {
        self.is_html(el) && names.contains(&self.local(el))
    }

    fn attr(&self, el: NodeId, name: &str) -> Option<String> {
        let html = self.html_in_html_doc(el);
        self.arena
            .attrs(el)
            .find(|&a| {
                let d = self.arena.attr(a).unwrap();
                d.name.ns.is_none()
                    && d.name.prefix.is_none()
                    && if html {
                        d.name.local.eq_ignore_ascii_case(name)
                    } else {
                        &*d.name.local == name
                    }
                    || (d.name.ns.is_none()
                        && d.name.prefix.is_some()
                        && d.name.qualified() == name)
            })
            .map(|a| self.arena.attr_value(a))
    }

    fn has_attr(&self, el: NodeId, name: &str) -> bool {
        self.attr(el, name).is_some()
    }

    fn parent_element(&self, n: NodeId) -> Option<NodeId> {
        self.arena.parent(n).filter(|&p| self.arena.is_element(p))
    }

    fn prev_element(&self, n: NodeId) -> Option<NodeId> {
        let mut c = self.arena.prev_sibling(n);
        while let Some(x) = c {
            if self.arena.is_element(x) {
                return Some(x);
            }
            c = self.arena.prev_sibling(x);
        }
        None
    }

    fn next_element(&self, n: NodeId) -> Option<NodeId> {
        let mut c = self.arena.next_sibling(n);
        while let Some(x) = c {
            if self.arena.is_element(x) {
                return Some(x);
            }
            c = self.arena.next_sibling(x);
        }
        None
    }

    // ----- complex selectors -------------------------------------------------------

    /// Matches `c` with its rightmost compound at `el`. For relative
    /// selectors, `anchor` is the `:has()` subject.
    fn complex(&self, c: &Complex, el: NodeId, anchor: Option<NodeId>) -> bool {
        let last = c.parts.len() - 1;
        self.match_from(c, last, el, anchor)
    }

    fn match_from(&self, c: &Complex, i: usize, el: NodeId, anchor: Option<NodeId>) -> bool {
        if !self.compound(&c.parts[i], el) {
            return false;
        }
        if i == 0 {
            return match (anchor, c.lead) {
                (Some(a), Some(comb)) => self.related(comb, el, |x| x == a),
                _ => true,
            };
        }
        let comb = c.combs[i - 1];
        self.related(comb, el, |x| self.match_from(c, i - 1, x, anchor))
    }

    /// Whether some element in relation `comb` to the left of `el`
    /// satisfies `f`.
    fn related(&self, comb: Combinator, el: NodeId, f: impl Fn(NodeId) -> bool) -> bool {
        match comb {
            Combinator::Child => self.parent_element(el).is_some_and(f),
            Combinator::Descendant => {
                let mut cur = self.parent_element(el);
                while let Some(p) = cur {
                    if f(p) {
                        return true;
                    }
                    cur = self.parent_element(p);
                }
                false
            }
            Combinator::NextSibling => self.prev_element(el).is_some_and(f),
            Combinator::SubsequentSibling => {
                let mut cur = self.prev_element(el);
                while let Some(p) = cur {
                    if f(p) {
                        return true;
                    }
                    cur = self.prev_element(p);
                }
                false
            }
        }
    }

    // ----- compound selectors ------------------------------------------------------

    fn compound(&self, c: &Compound, el: NodeId) -> bool {
        let Some(e) = self.arena.element(el) else {
            return false;
        };
        if let Some(t) = &c.tag {
            let ok = if self.html_in_html_doc(el) {
                *e.name.local == *t.to_ascii_lowercase()
            } else {
                &*e.name.local == t
            };
            if !ok {
                return false;
            }
        }
        for id in &c.ids {
            match self.attr(el, "id") {
                Some(v) if self.ci_eq(&v, id) => {}
                _ => return false,
            }
        }
        if !c.classes.is_empty() {
            let Some(cls) = self.attr(el, "class") else {
                return false;
            };
            for want in &c.classes {
                if !cls.split_ascii_whitespace().any(|x| self.ci_eq(x, want)) {
                    return false;
                }
            }
        }
        for a in &c.attrs {
            if !self.attr_sel(a, el) {
                return false;
            }
        }
        c.pseudos.iter().all(|p| self.pseudo(p, el))
    }

    /// Id/class comparison (case-insensitive in quirks mode).
    fn ci_eq(&self, a: &str, b: &str) -> bool {
        if self.quirks {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    }

    fn attr_sel(&self, a: &AttrSel, el: NodeId) -> bool {
        let Some(v) = self.attr(el, &a.name) else {
            return false;
        };
        let ci = match a.case_insensitive {
            Some(x) => x,
            None => {
                self.html_in_html_doc(el)
                    && CI_ATTRS.contains(&a.name.to_ascii_lowercase().as_str())
            }
        };
        let (v, w) = if ci {
            (v.to_ascii_lowercase(), a.value.to_ascii_lowercase())
        } else {
            (v, a.value.clone())
        };
        match a.op {
            AttrOp::Exists => true,
            AttrOp::Equals => v == w,
            AttrOp::Includes => {
                !w.is_empty()
                    && !w.contains(|c: char| c.is_ascii_whitespace())
                    && v.split_ascii_whitespace().any(|x| x == w)
            }
            AttrOp::DashMatch => v == w || (v.starts_with(&w) && v[w.len()..].starts_with('-')),
            AttrOp::Prefix => !w.is_empty() && v.starts_with(&w),
            AttrOp::Suffix => !w.is_empty() && v.ends_with(&w),
            AttrOp::Substring => !w.is_empty() && v.contains(&w),
        }
    }

    // ----- pseudo-classes -----------------------------------------------------------

    fn any_of(&self, list: &[Complex], el: NodeId) -> bool {
        list.iter().any(|c| self.complex(c, el, None))
    }

    fn pseudo(&self, p: &Pseudo, el: NodeId) -> bool {
        match p {
            Pseudo::Not(l) => !self.any_of(l, el),
            Pseudo::Is(l) | Pseudo::Current(l) => self.any_of(l, el),
            Pseudo::Has(l) => self.has(l, el),
            Pseudo::NthChild(n) => self.nth(n, el, false, false),
            Pseudo::NthLastChild(n) => self.nth(n, el, true, false),
            Pseudo::NthOfType(n) => self.nth(n, el, false, true),
            Pseudo::NthLastOfType(n) => self.nth(n, el, true, true),
            Pseudo::FirstChild => self.prev_element(el).is_none(),
            Pseudo::LastChild => self.next_element(el).is_none(),
            Pseudo::OnlyChild => self.prev_element(el).is_none() && self.next_element(el).is_none(),
            Pseudo::FirstOfType => self.siblings_of_type(el, false) == 0,
            Pseudo::LastOfType => self.siblings_of_type(el, true) == 0,
            Pseudo::OnlyOfType => {
                self.siblings_of_type(el, false) == 0 && self.siblings_of_type(el, true) == 0
            }
            Pseudo::Root => self.arena.parent(el).is_some_and(|p| {
                matches!(
                    self.arena.data(p),
                    NodeData::Document(_) | NodeData::Fragment
                )
            }),
            Pseudo::Empty => self.arena.children(el).all(|c| {
                matches!(
                    self.arena.data(c),
                    NodeData::Comment(_) | NodeData::Pi { .. }
                )
            }),
            Pseudo::Checked => {
                (self.is_html_named(el, &["input"])
                    && self.attr(el, "type").is_some_and(|t| {
                        t.eq_ignore_ascii_case("checkbox") || t.eq_ignore_ascii_case("radio")
                    })
                    && self.has_attr(el, "checked"))
                    || (self.is_html_named(el, &["option"]) && self.has_attr(el, "selected"))
            }
            Pseudo::Disabled => self.disabled(el),
            Pseudo::Enabled => !self.disabled(el),
            Pseudo::Link => {
                self.is_html_named(el, &["a", "area", "link"]) && self.has_attr(el, "href")
            }
            Pseudo::Required => {
                self.is_html_named(el, &["input", "select", "textarea"])
                    && self.has_attr(el, "required")
            }
            Pseudo::Optional => {
                self.is_html_named(el, &["input", "select", "textarea"])
                    && !self.has_attr(el, "required")
            }
            Pseudo::PlaceholderShown => {
                self.is_html_named(el, &["input", "textarea"]) && self.has_attr(el, "placeholder")
            }
            Pseudo::ReadWrite => self.read_write(el),
            Pseudo::ReadOnly => !self.read_write(el),
            Pseudo::Never => false,
        }
    }

    fn read_write(&self, el: NodeId) -> bool {
        if self.is_html_named(el, &["input", "textarea"]) {
            return !self.has_attr(el, "readonly") && !self.has_attr(el, "disabled");
        }
        self.is_html(el)
            && self
                .attr(el, "contenteditable")
                .is_some_and(|v| !v.eq_ignore_ascii_case("false"))
    }

    /// `:disabled` as PHP evaluates it (observed on PHP 8.5, test
    /// pseudo_classes_disabled_enabled): form controls with a `disabled`
    /// attribute, and form controls inside a disabled `fieldset` except
    /// in its first element child.
    fn disabled(&self, el: NodeId) -> bool {
        if !self.is_html(el) {
            return false;
        }
        let name = self.local(el);
        if !matches!(
            name,
            "button" | "input" | "select" | "textarea" | "optgroup" | "option" | "fieldset"
        ) {
            return false;
        }
        if self.has_attr(el, "disabled") {
            return true;
        }
        if name == "option"
            && let Some(p) = self.parent_element(el)
            && self.is_html_named(p, &["optgroup"])
            && self.has_attr(p, "disabled")
        {
            return true;
        }
        if matches!(name, "optgroup" | "option") {
            return false;
        }
        let mut child = el;
        let mut cur = self.parent_element(el);
        while let Some(p) = cur {
            if self.is_html_named(p, &["fieldset"]) && self.has_attr(p, "disabled") {
                let first = self.arena.children(p).find(|&c| self.arena.is_element(c));
                if first != Some(child) {
                    return true;
                }
            }
            child = p;
            cur = self.parent_element(p);
        }
        false
    }

    fn has(&self, list: &[Complex], el: NodeId) -> bool {
        // Candidates: descendants of el, its following siblings and their
        // descendants.
        let mut cands: Vec<NodeId> = self
            .arena
            .descendants(el)
            .filter(|&n| self.arena.is_element(n))
            .collect();
        let mut s = self.next_element(el);
        while let Some(x) = s {
            cands.push(x);
            cands.extend(
                self.arena
                    .descendants(x)
                    .filter(|&n| self.arena.is_element(n)),
            );
            s = self.next_element(x);
        }
        cands
            .into_iter()
            .any(|c| list.iter().any(|sel| self.complex(sel, c, Some(el))))
    }

    fn same_type(&self, a: NodeId, b: NodeId) -> bool {
        let x = &self.arena.element(a).unwrap().name;
        let y = &self.arena.element(b).unwrap().name;
        x.local == y.local && x.ns == y.ns
    }

    /// Number of same-type element siblings before (or after) `el`.
    fn siblings_of_type(&self, el: NodeId, after: bool) -> usize {
        let mut n = 0;
        let mut cur = if after {
            self.next_element(el)
        } else {
            self.prev_element(el)
        };
        while let Some(x) = cur {
            if self.same_type(x, el) {
                n += 1;
            }
            cur = if after {
                self.next_element(x)
            } else {
                self.prev_element(x)
            };
        }
        n
    }

    fn nth(&self, n: &Nth, el: NodeId, last: bool, of_type: bool) -> bool {
        if let Some(of) = &n.of
            && !self.any_of(of, el)
        {
            return false;
        }
        // 1-based index among the counted siblings.
        let mut idx: i64 = 1;
        let mut cur = if last {
            self.next_element(el)
        } else {
            self.prev_element(el)
        };
        while let Some(x) = cur {
            let counted = if of_type {
                self.same_type(x, el)
            } else if let Some(of) = &n.of {
                self.any_of(of, x)
            } else {
                true
            };
            if counted {
                idx += 1;
            }
            cur = if last {
                self.next_element(x)
            } else {
                self.prev_element(x)
            };
        }
        if n.a == 0 {
            return idx == n.b;
        }
        let d = idx - n.b;
        d % n.a == 0 && d / n.a >= 0
    }
}
