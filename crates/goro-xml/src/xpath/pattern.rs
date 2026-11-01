//! XSLT 1.0 patterns (section 5.2): the subset of XPath location paths a
//! node can be tested against. A pattern is read with the XPath parser and
//! tested from its last step upwards: a node matches when some
//! ancestor-or-self context would select it.

use std::rc::Rc;

use super::parser::{self, Axis, CoreFn, Expr, NodeTest, Start, Step};
use super::{FunctionHandler, NsMap, XPathError, XPathErrorKind as K, eval};
use crate::tree::{Arena, NodeData, NodeId};

/// Which nodes a pattern alternative can match, by its last step (the
/// template index of XSLT).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PatternKey {
    /// Elements of this local name.
    Element(Rc<str>),
    /// Any element.
    AnyElement,
    /// Attributes of this local name.
    Attribute(Rc<str>),
    /// Any attribute.
    AnyAttribute,
    /// Text nodes.
    Text,
    /// Comments.
    Comment,
    /// Processing instructions.
    Pi,
    /// Root nodes.
    Root,
    /// Anything (`node()`, `id(...)`, `key(...)`).
    Any,
}

#[derive(Clone, Debug)]
enum PatStart {
    Relative,
    Root,
    Function(Expr),
}

#[derive(Clone, Debug)]
struct PatStep {
    axis: Axis,
    test: NodeTest,
    preds: Vec<Expr>,
    /// `//` comes before this step.
    any_ancestor: bool,
}

/// One alternative of a pattern (section 5.5 treats each separately).
#[derive(Clone, Debug)]
pub struct Pattern {
    start: PatStart,
    steps: Vec<PatStep>,
    ns: NsMap,
    /// The default priority (XSLT 1.0 section 5.5).
    pub priority: f64,
    /// The first prefix not bound.
    unbound: Option<String>,
}

fn bad() -> XPathError {
    XPathError::new(K::Expr)
}

fn alternatives(e: Expr, out: &mut Vec<Expr>) {
    match e {
        Expr::Union(a, b) => {
            alternatives(*a, out);
            alternatives(*b, out);
        }
        e => out.push(e),
    }
}

fn is_ancestor_step(s: &Step) -> bool {
    s.axis == Axis::DescendantOrSelf && s.test == NodeTest::Node && s.preds.is_empty()
}

fn is_key_or_id(e: &Expr) -> bool {
    match e {
        Expr::Fn(CoreFn::Id, _) => true,
        Expr::HostFn(n, _) => n == "key",
        _ => false,
    }
}

fn one(e: Expr, ns: &NsMap) -> Result<Pattern, XPathError> {
    let Expr::Path(p) = e else {
        // A bare function call is not parsed as a path.
        if !is_key_or_id(&e) {
            return Err(bad());
        }
        let mut pat = Pattern {
            start: PatStart::Function(e),
            steps: Vec::new(),
            ns: ns.clone(),
            priority: 0.5,
            unbound: None,
        };
        pat.unbound = pat.find_unbound();
        return Ok(pat);
    };
    let p = *p;
    let start = match p.start {
        Start::Root => PatStart::Root,
        Start::Context => PatStart::Relative,
        Start::Filter(f, preds) => {
            if !preds.is_empty() || !is_key_or_id(&f) {
                return Err(bad());
            }
            PatStart::Function(f)
        }
    };
    let mut steps: Vec<PatStep> = Vec::with_capacity(p.steps.len());
    let mut pending = false;
    for s in p.steps {
        if is_ancestor_step(&s) {
            if pending {
                return Err(bad());
            }
            pending = true;
            continue;
        }
        if !matches!(s.axis, Axis::Child | Axis::Attribute) {
            return Err(bad());
        }
        steps.push(PatStep {
            axis: s.axis,
            test: s.test,
            preds: s.preds,
            any_ancestor: pending,
        });
        pending = false;
    }
    if pending {
        return Err(bad());
    }
    let priority = match (&start, steps.as_slice()) {
        (PatStart::Relative, [s]) if !s.any_ancestor && s.preds.is_empty() => match &s.test {
            NodeTest::Name(..) => 0.0,
            NodeTest::Pi(Some(_)) => 0.0,
            NodeTest::NsAny(_) => -0.25,
            _ => -0.5,
        },
        _ => 0.5,
    };
    let mut pat = Pattern {
        start,
        steps,
        ns: ns.clone(),
        priority,
        unbound: None,
    };
    pat.unbound = pat.find_unbound();
    Ok(pat)
}

/// Compiles a pattern into its alternatives.
pub fn compile(pattern: &str, ns: &NsMap) -> Result<Vec<Pattern>, XPathError> {
    let e = parser::parse_raw(pattern, true)?;
    let mut alts = Vec::new();
    alternatives(e, &mut alts);
    alts.into_iter().map(|a| one(a, ns)).collect()
}

impl Pattern {
    fn find_unbound(&self) -> Option<String> {
        let check = |e: &Expr| eval::unbound_prefix(e, &self.ns);
        if let PatStart::Function(f) = &self.start
            && let Some(x) = check(f)
        {
            return Some(x);
        }
        for s in &self.steps {
            match &s.test {
                NodeTest::NsAny(p) | NodeTest::Name(Some(p), _)
                    if p != "xml" && !self.ns.contains_key(p) =>
                {
                    return Some(p.clone());
                }
                _ => {}
            }
            if let Some(x) = s.preds.iter().find_map(check) {
                return Some(x);
            }
        }
        None
    }

    /// The first prefix used but not bound.
    pub fn unbound_prefix(&self) -> Option<&str> {
        self.unbound.as_deref()
    }

    /// The index key of this alternative.
    pub fn key(&self) -> PatternKey {
        let Some(last) = self.steps.last() else {
            return match self.start {
                PatStart::Root => PatternKey::Root,
                _ => PatternKey::Any,
            };
        };
        let attr = last.axis == Axis::Attribute;
        match &last.test {
            NodeTest::Name(_, l) => {
                if attr {
                    PatternKey::Attribute(l.as_str().into())
                } else {
                    PatternKey::Element(l.as_str().into())
                }
            }
            NodeTest::Any | NodeTest::NsAny(_) => {
                if attr {
                    PatternKey::AnyAttribute
                } else {
                    PatternKey::AnyElement
                }
            }
            NodeTest::Text if !attr => PatternKey::Text,
            NodeTest::Comment if !attr => PatternKey::Comment,
            NodeTest::Pi(_) if !attr => PatternKey::Pi,
            _ => PatternKey::Any,
        }
    }

    /// Whether `node` matches.
    pub fn matches(
        &self,
        arena: &mut Arena,
        node: NodeId,
        functions: &mut dyn FunctionHandler,
        namespace_nodes: &mut Vec<NodeId>,
    ) -> Result<bool, XPathError> {
        if let Some(p) = &self.unbound {
            return Err(XPathError::with_name(K::UndefPrefix, p));
        }
        let mut ev = eval::bound(arena, self.ns.clone(), functions, namespace_nodes);
        if self.steps.is_empty() {
            return self.at_start(&mut ev, node, false);
        }
        self.chain(&mut ev, self.steps.len() - 1, node)
    }

    fn at_start(
        &self,
        ev: &mut eval::Eval<'_>,
        n: NodeId,
        any_ancestor: bool,
    ) -> Result<bool, XPathError> {
        match &self.start {
            PatStart::Relative => Ok(true),
            PatStart::Root => {
                let mut cur = Some(n);
                while let Some(c) = cur {
                    if ev.arena.parent(c).is_none()
                        && matches!(ev.arena.data(c), NodeData::Document(_) | NodeData::Fragment)
                    {
                        return Ok(true);
                    }
                    if !any_ancestor {
                        return Ok(false);
                    }
                    cur = ev.arena.parent(c);
                }
                Ok(false)
            }
            PatStart::Function(f) => {
                let v = ev.expr(
                    f,
                    eval::C {
                        node: n,
                        pos: 1,
                        size: 1,
                    },
                )?;
                let super::Value::NodeSet(v) = v else {
                    return Ok(false);
                };
                let mut cur = Some(n);
                while let Some(c) = cur {
                    if v.contains(&c) {
                        return Ok(true);
                    }
                    if !any_ancestor {
                        return Ok(false);
                    }
                    cur = ev.arena.parent(c);
                }
                Ok(false)
            }
        }
    }

    /// Tests step `i` on `n`, then the steps before it on `n`'s ancestors.
    fn chain(&self, ev: &mut eval::Eval<'_>, i: usize, n: NodeId) -> Result<bool, XPathError> {
        let step = &self.steps[i];
        let kind_ok = match (step.axis, ev.arena.data(n)) {
            (Axis::Attribute, NodeData::Attribute(_)) => true,
            (Axis::Attribute, _) => false,
            (_, NodeData::Attribute(_) | NodeData::Namespace(_) | NodeData::Document(_)) => false,
            _ => true,
        };
        if !kind_ok || !ev.matches(n, step.axis, &step.test) {
            return Ok(false);
        }
        let parent = ev.arena.parent(n);
        if !step.preds.is_empty() {
            // The predicates see positions among the nodes the step selects
            // from the parent.
            let Some(p) = parent else {
                return Ok(false);
            };
            let mut set = ev.axis(p, step.axis, &step.test);
            for pred in &step.preds {
                set = ev.filter(set, pred)?;
            }
            if !set.contains(&n) {
                return Ok(false);
            }
        }
        if i == 0 {
            return match parent {
                Some(p) => self.at_start(ev, p, step.any_ancestor),
                None => Ok(matches!(self.start, PatStart::Relative)),
            };
        }
        let mut cur = parent;
        while let Some(p) = cur {
            if self.chain(ev, i - 1, p)? {
                return Ok(true);
            }
            if !step.any_ancestor {
                return Ok(false);
            }
            cur = ev.arena.parent(p);
        }
        Ok(false)
    }
}
