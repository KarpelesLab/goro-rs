//! DTD validation (XML 1.0 validity constraints), with the messages and
//! codes PHP 8.5 reports through libxml (fixture
//! `tests/fixtures/validate.php`).

use std::collections::{HashMap, HashSet};

use crate::chars::{is_name, is_nmtoken};
use crate::error::{Domain, LEVEL_ERROR, XmlError};
use crate::tree::{
    Arena, AttrDecl, AttrDefault, AttrType, ContentModel, DtdData, ElementDecl, EntityKind,
    NodeData, NodeId, Occur, Particle,
};

/// When validation runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timing {
    /// `DOMDocument::validate()` on a built tree.
    AfterParse,
    /// LIBXML_DTDVALID: errors in parse order.
    DuringParse,
}

struct V<'a> {
    arena: &'a Arena,
    doc: NodeId,
    subsets: Vec<&'a DtdData>,
    out: Vec<XmlError>,
    timing: Timing,
    ids: HashSet<String>,
    idrefs: Vec<String>,
    url: Option<String>,
}

impl V<'_> {
    fn err(&mut self, code: u32, msg: String, line: i64) {
        let line = match self.timing {
            Timing::AfterParse => line,
            Timing::DuringParse => line.max(0),
        };
        self.out.push(XmlError {
            domain: Domain::Valid,
            code,
            level: LEVEL_ERROR,
            message: format!("{msg}\n"),
            file: self.url.clone(),
            line: line.max(0) as u32,
            column: 0,
        });
    }

    fn element_decl(&self, name: &str) -> Option<&ElementDecl> {
        self.subsets.iter().find_map(|d| d.elements.get(name))
    }

    fn attr_decls(&self, el: &str) -> Vec<&AttrDecl> {
        let mut v: Vec<&AttrDecl> = Vec::new();
        for d in &self.subsets {
            if let Some(l) = d.attlists.get(el) {
                for a in l {
                    if !v.iter().any(|x| x.name == a.name) {
                        v.push(a);
                    }
                }
            }
        }
        v
    }

    fn entity(&self, name: &str) -> Option<&crate::tree::EntityDecl> {
        self.subsets.iter().find_map(|d| d.entities.get(name))
    }

    /// Child element names and "CDATA" for non-blank text, as libxml lists
    /// them in content errors.
    fn child_names(&self, el: NodeId) -> Vec<String> {
        let mut v = Vec::new();
        for c in self.arena.children(el) {
            match self.arena.data(c) {
                NodeData::Element(e) => v.push(e.name.qualified()),
                NodeData::Text(t) | NodeData::CData(t)
                    if !t.bytes().all(crate::chars::is_space) =>
                {
                    v.push("CDATA".into());
                }
                _ => {}
            }
        }
        v
    }

    /// Start-of-element checks (declaration, attributes).
    fn start_checks(&mut self, el: NodeId) {
        let e = self.arena.element(el).unwrap();
        let name = e.name.qualified();
        let line = self.arena.line(el) as i64;
        if self.element_decl(&name).is_none() {
            self.err(534, format!("No declaration for element {name}"), line);
        }
        let decls: Vec<AttrDecl> = self.attr_decls(&name).into_iter().cloned().collect();
        let attrs: Vec<NodeId> = self.arena.attrs(el).collect();
        for &a in &attrs {
            let an = self.arena.attr(a).unwrap().name.qualified();
            let v = self.arena.attr_value(a);
            let Some(d) = decls.iter().find(|d| *d.name == *an) else {
                self.err(
                    533,
                    format!("No declaration for attribute {an} of element {name}"),
                    line,
                );
                continue;
            };
            self.check_value(d, &an, &name, &v, line);
            if d.default == AttrDefault::Fixed
                && let Some(fv) = &d.value
                && *fv != v
            {
                self.err(
                    500,
                    format!(
                        "Value for attribute {an} of {name} is different from default \"{fv}\""
                    ),
                    line,
                );
                self.err(
                    502,
                    format!("Value for attribute {an} of {name} must be \"{fv}\""),
                    line,
                );
            }
        }
        // Namespace declarations count as declared attributes when listed.
        for d in &decls {
            if d.default == AttrDefault::Required
                && !attrs
                    .iter()
                    .any(|&a| *self.arena.attr(a).unwrap().name.qualified() == *d.name)
            {
                let an = d.name.clone();
                self.err(
                    518,
                    format!("Element {name} does not carry attribute {an}"),
                    line,
                );
            }
        }
    }

    fn check_value(&mut self, d: &AttrDecl, an: &str, el: &str, v: &str, line: i64) {
        let bad_syntax = match d.atype {
            AttrType::Cdata => false,
            AttrType::Id | AttrType::IdRef | AttrType::Entity => !is_name(v),
            AttrType::IdRefs | AttrType::Entities => {
                v.split(' ').any(|t| !is_name(t)) || v.is_empty()
            }
            AttrType::NmToken => !is_nmtoken(v),
            AttrType::NmTokens => v.split(' ').any(|t| !is_nmtoken(t)) || v.is_empty(),
            AttrType::Enumeration | AttrType::Notation => false,
        };
        if bad_syntax {
            self.err(
                502,
                format!("Syntax of value for attribute {an} of {el} is not valid"),
                line,
            );
            return;
        }
        match d.atype {
            AttrType::Enumeration | AttrType::Notation => {
                if !d.values.iter().any(|x| x == v) {
                    self.err(502, format!("Value \"{v}\" for attribute {an} of {el} is not among the enumerated set"), line);
                }
            }
            AttrType::Id => {
                if !self.ids.insert(v.to_string()) && self.timing == Timing::AfterParse {
                    self.err(513, format!("ID {v} already defined"), line);
                }
            }
            AttrType::IdRef => self.idrefs.push(format!("{an}\0{v}")),
            AttrType::IdRefs => {
                for t in v.split(' ') {
                    self.idrefs.push(format!("{an}\0{t}"));
                }
            }
            AttrType::Entity | AttrType::Entities => {
                for t in v.split(' ') {
                    let ok = self
                        .entity(t)
                        .is_some_and(|e| e.kind == EntityKind::ExternalGeneralUnparsed);
                    if !ok {
                        let l = if self.timing == Timing::AfterParse {
                            -1
                        } else {
                            line
                        };
                        self.err(
                            535,
                            format!("ENTITY attribute {an} reference an unknown entity \"{t}\""),
                            l,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// End-of-element checks (content model).
    fn content_checks(&mut self, el: NodeId) {
        let e = self.arena.element(el).unwrap();
        let name = e.name.qualified();
        let line = self.arena.line(el) as i64;
        let Some(decl) = self.element_decl(&name).cloned() else {
            return;
        };
        match &decl.content {
            ContentModel::Any => {}
            ContentModel::Empty => {
                if self.arena.first_child(el).is_some() {
                    self.err(
                        528,
                        format!("Element {name} was declared EMPTY this one has content"),
                        line,
                    );
                }
            }
            ContentModel::Mixed(names) => {
                if names.is_empty() {
                    if self.arena.children(el).any(|c| self.arena.is_element(c)) {
                        self.err(
                            529,
                            format!(
                                "Element {name} was declared #PCDATA but contains non text nodes"
                            ),
                            line,
                        );
                    }
                } else {
                    for c in self.arena.children(el).collect::<Vec<_>>() {
                        if let Some(ce) = self.arena.element(c) {
                            let cn = ce.name.qualified();
                            if !names.iter().any(|n| **n == *cn) {
                                self.err(515, format!("Element {cn} is not declared in {name} list of possible children"), line);
                            }
                        }
                    }
                }
            }
            ContentModel::Children(p) => {
                let kids = self.child_names(el);
                let ends = match_particle(p, &kids, 0);
                if !ends.contains(&kids.len()) {
                    let mut model = String::new();
                    crate::serialize::content_model(&decl.content, &mut model);
                    // Each name followed by a space, as the php-src tests
                    // expect (DOMDocument_loadXML_variation2.phpt).
                    let got = if kids.is_empty() {
                        String::new()
                    } else {
                        format!("({} )", kids.join(" "))
                    };
                    self.err(504, format!("Element {name} content does not follow the DTD, expecting {model}, got {got}"), line);
                }
            }
        }
    }

    fn walk(&mut self, el: NodeId) {
        match self.timing {
            Timing::DuringParse => {
                self.start_checks(el);
                for c in self.arena.children(el).collect::<Vec<_>>() {
                    if self.arena.is_element(c) {
                        self.walk(c);
                    }
                }
                self.content_checks(el);
            }
            Timing::AfterParse => {
                self.content_checks(el);
                self.start_checks(el);
                for c in self.arena.children(el).collect::<Vec<_>>() {
                    if self.arena.is_element(c) {
                        self.walk(c);
                    }
                }
            }
        }
    }
}

/// Positions in `kids` reachable after matching `p` from `at`.
fn match_particle(p: &Particle, kids: &[String], at: usize) -> Vec<usize> {
    let once = |pos: usize| -> Vec<usize> {
        match p {
            Particle::Name(n, _) => {
                if kids.get(pos).is_some_and(|k| **k == **n) {
                    vec![pos + 1]
                } else {
                    vec![]
                }
            }
            Particle::Seq(items, _) => {
                let mut cur = vec![pos];
                for it in items {
                    let mut next = Vec::new();
                    for &c in &cur {
                        for e in match_particle(it, kids, c) {
                            if !next.contains(&e) {
                                next.push(e);
                            }
                        }
                    }
                    cur = next;
                    if cur.is_empty() {
                        break;
                    }
                }
                cur
            }
            Particle::Choice(items, _) => {
                let mut out = Vec::new();
                for it in items {
                    for e in match_particle(it, kids, pos) {
                        if !out.contains(&e) {
                            out.push(e);
                        }
                    }
                }
                out
            }
        }
    };
    let occ = match p {
        Particle::Name(_, o) | Particle::Seq(_, o) | Particle::Choice(_, o) => *o,
    };
    match occ {
        Occur::One => once(at),
        Occur::Opt => {
            let mut v = vec![at];
            for e in once(at) {
                if !v.contains(&e) {
                    v.push(e);
                }
            }
            v
        }
        Occur::Star | Occur::Plus => {
            let mut seen: Vec<usize> = Vec::new();
            let mut frontier = vec![at];
            let mut first = true;
            while !frontier.is_empty() {
                let mut next = Vec::new();
                for &f in &frontier {
                    for e in once(f) {
                        if !seen.contains(&e) && e != f {
                            seen.push(e);
                            next.push(e);
                        }
                    }
                }
                frontier = next;
                first = false;
            }
            let _ = first;
            if occ == Occur::Star && !seen.contains(&at) {
                seen.push(at);
            }
            seen
        }
    }
}

fn run(arena: &Arena, doc: NodeId, timing: Timing) -> Vec<XmlError> {
    let d = arena.doc(doc);
    let mut subsets = Vec::new();
    for s in [d.doctype, d.ext_subset].into_iter().flatten() {
        if let Some(x) = arena.dtd(s) {
            subsets.push(x);
        }
    }
    let mut v = V {
        arena,
        doc,
        subsets,
        out: Vec::new(),
        timing,
        ids: HashSet::new(),
        idrefs: Vec::new(),
        url: d.url.clone(),
    };
    let Some(root) = arena.document_element(doc) else {
        return v.out;
    };
    if v.subsets.is_empty() {
        match timing {
            Timing::AfterParse => v.err(522, "no DTD found!".into(), 0),
            Timing::DuringParse => {
                // Recorded without a newline and not displayed by PHP
                // (observed on PHP 8.5; test
                // ext/dom/tests/modern/xml/XMLDocument_fromString_03.phpt).
                v.err(522, "Validation failed: no DTD found !".into(), 1);
                if let Some(e) = v.out.last_mut() {
                    e.message.pop();
                }
            }
        }
        return v.out;
    }
    let dt_name = v.subsets[0].name.to_string();
    let rn = arena.element(root).unwrap().name.qualified();
    if dt_name != rn {
        let line = arena.line(root) as i64;
        v.err(
            531,
            format!("root and DTD name do not match '{rn}' and '{dt_name}'"),
            line,
        );
    }
    v.walk(root);
    let ids = v.ids.clone();
    let refs = std::mem::take(&mut v.idrefs);
    let mut seen: HashMap<String, ()> = HashMap::new();
    for r in refs {
        let (an, val) = r.split_once('\0').unwrap();
        if !ids.contains(val) && seen.insert(r.clone(), ()).is_none() {
            v.err(
                536,
                format!("IDREF attribute {an} references an unknown ID \"{val}\""),
                1,
            );
        }
    }
    let _ = v.doc;
    v.out
}

/// `DOMDocument::validate()`: validates a document against its DTD and
/// returns the validity errors (empty when valid).
pub fn validate(arena: &Arena, doc: NodeId) -> Vec<XmlError> {
    run(arena, doc, Timing::AfterParse)
}

/// The errors LIBXML_DTDVALID reports while parsing (in parse order).
pub fn validate_during_parse(arena: &Arena, doc: NodeId) -> Vec<XmlError> {
    run(arena, doc, Timing::DuringParse)
}
