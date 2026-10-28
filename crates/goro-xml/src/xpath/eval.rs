//! XPath 1.0 evaluation (W3C XPath 1.0 sections 2-5).

use std::collections::HashMap;

use super::number::string_to_number;
use super::parser::{ArOp, Axis, CmpOp, CoreFn, Expr, NodeTest, PathExpr, Start, Step};
use super::{Context, FunctionHandler, Value, XPathError, XPathErrorKind as K};
use crate::tree::{Arena, NodeData, NodeId, NsDecl, Str, XML_NS};

/// The context of an expression: node, position and size.
#[derive(Clone, Copy)]
struct C {
    node: NodeId,
    pos: usize,
    size: usize,
}

/// Document order keys of one tree (built on demand).
struct Order {
    /// Indexed by `NodeId::index()`; 0 = not in the tree.
    key: Vec<u64>,
}

struct Eval<'r> {
    arena: &'r mut Arena,
    ns: HashMap<String, String>,
    funcs: Option<&'r mut dyn FunctionHandler>,
    nsnodes: HashMap<(NodeId, Option<Str>), NodeId>,
    /// Creation index of namespace nodes (order among one element's).
    ns_index: HashMap<NodeId, u32>,
    created: &'r mut Vec<NodeId>,
    order: Option<Order>,
}

pub(super) fn evaluate(
    arena: &mut Arena,
    expr: &Expr,
    context: NodeId,
    ctx: &mut Context<'_>,
) -> Result<Value, XPathError> {
    let mut ns = HashMap::new();
    resolve(expr, arena, context, ctx, &mut ns)?;
    let funcs: Option<&mut dyn FunctionHandler> = match &mut ctx.functions {
        Some(f) => Some(&mut **f),
        None => None,
    };
    let mut ev = Eval {
        arena,
        ns,
        funcs,
        nsnodes: HashMap::new(),
        ns_index: HashMap::new(),
        created: &mut ctx.namespace_nodes,
        order: None,
    };
    ev.expr(
        expr,
        C {
            node: context,
            pos: 1,
            size: 1,
        },
    )
}

// ----- prefix resolution ---------------------------------------------------------

fn lookup_prefix(
    arena: &Arena,
    context: NodeId,
    ctx: &Context<'_>,
    prefix: &str,
) -> Option<String> {
    // The context node's in-scope declarations, registered after the
    // user's, win (observed on PHP 8.5, test ext/dom/tests/bug55700.phpt).
    if ctx.register_node_ns
        && let Some(u) = arena.lookup_ns(context, Some(prefix))
    {
        return Some(u.to_string());
    }
    if let Some((_, u)) = ctx.namespaces.iter().rev().find(|(p, _)| p == prefix) {
        return Some(u.clone());
    }
    if prefix == "xml" {
        return Some(XML_NS.into());
    }
    None
}

fn resolve(
    e: &Expr,
    arena: &Arena,
    context: NodeId,
    ctx: &Context<'_>,
    out: &mut HashMap<String, String>,
) -> Result<(), XPathError> {
    let need = |p: &str, out: &mut HashMap<String, String>| -> Result<(), XPathError> {
        if out.contains_key(p) {
            return Ok(());
        }
        match lookup_prefix(arena, context, ctx, p) {
            Some(u) => {
                out.insert(p.to_string(), u);
                Ok(())
            }
            None => Err(XPathError::with_name(K::UndefPrefix, p)),
        }
    };
    match e {
        Expr::Or(a, b)
        | Expr::And(a, b)
        | Expr::Cmp(_, a, b)
        | Expr::Arith(_, a, b)
        | Expr::Union(a, b) => {
            resolve(a, arena, context, ctx, out)?;
            resolve(b, arena, context, ctx, out)
        }
        Expr::Neg(a) => resolve(a, arena, context, ctx, out),
        Expr::Literal(_) | Expr::Number(_) => Ok(()),
        Expr::Fn(_, args) => {
            for a in args {
                resolve(a, arena, context, ctx, out)?;
            }
            Ok(())
        }
        Expr::ExtFn(p, l, args) => {
            need(p, out)?;
            let uri = out.get(p).cloned().unwrap_or_default();
            let known = ctx
                .functions
                .as_ref()
                .is_some_and(|f| f.has_function(&uri, l));
            if !known {
                return Err(XPathError::with_name(
                    K::UnknownFunction,
                    &format!("{p}:{l}"),
                ));
            }
            for a in args {
                resolve(a, arena, context, ctx, out)?;
            }
            Ok(())
        }
        Expr::Path(p) => {
            if let Start::Filter(f, preds) = &p.start {
                resolve(f, arena, context, ctx, out)?;
                for x in preds {
                    resolve(x, arena, context, ctx, out)?;
                }
            }
            for s in &p.steps {
                match &s.test {
                    NodeTest::NsAny(pr) | NodeTest::Name(Some(pr), _) => need(pr, out)?,
                    _ => {}
                }
                for x in &s.preds {
                    resolve(x, arena, context, ctx, out)?;
                }
            }
            Ok(())
        }
    }
}

// ----- data model helpers --------------------------------------------------------

/// Nodes that are part of the XPath tree (not doctype/DTD declarations).
fn in_model(arena: &Arena, n: NodeId) -> bool {
    !matches!(
        arena.data(n),
        NodeData::Doctype(_)
            | NodeData::EntityDecl(_)
            | NodeData::Notation(_)
            | NodeData::ElementDecl(_)
            | NodeData::AttributeDecl(_)
    )
}

fn is_attr_or_ns(arena: &Arena, n: NodeId) -> bool {
    matches!(
        arena.data(n),
        NodeData::Attribute(_) | NodeData::Namespace(_)
    )
}

/// Children in the XPath model (entity references are leaves).
fn children(arena: &Arena, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    let has = matches!(
        arena.data(n),
        NodeData::Document(_) | NodeData::Fragment | NodeData::Element(_)
    );
    let first = if has { arena.first_child(n) } else { None };
    std::iter::successors(first, move |&c| arena.next_sibling(c))
        .filter(move |&c| in_model(arena, c))
}

/// The XPath parent (owner element for attributes and namespace nodes).
fn parent(arena: &Arena, n: NodeId) -> Option<NodeId> {
    arena.parent(n)
}

/// Pre-order descendants (not including `n`).
fn push_descendants(
    arena: &Arena,
    n: NodeId,
    out: &mut Vec<NodeId>,
    test: &dyn Fn(NodeId) -> bool,
) {
    let mut stack: Vec<NodeId> = Vec::new();
    let mut kids: Vec<NodeId> = children(arena, n).collect();
    kids.reverse();
    stack.extend(kids);
    while let Some(c) = stack.pop() {
        if test(c) {
            out.push(c);
        }
        if matches!(arena.data(c), NodeData::Element(_)) {
            let start = stack.len();
            stack.extend(children(arena, c));
            stack[start..].reverse();
        }
    }
}

pub(super) fn string_value(arena: &Arena, n: NodeId) -> String {
    match arena.data(n) {
        NodeData::Text(s) | NodeData::CData(s) | NodeData::Comment(s) => s.clone(),
        NodeData::Pi { data, .. } => data.clone(),
        NodeData::Attribute(_) => arena.attr_value(n),
        NodeData::Namespace(d) => d.uri.to_string(),
        NodeData::Element(_) | NodeData::Document(_) | NodeData::Fragment => {
            let mut s = String::new();
            let mut stack = vec![n];
            while let Some(x) = stack.pop() {
                match arena.data(x) {
                    NodeData::Text(t) | NodeData::CData(t) => s.push_str(t),
                    NodeData::Element(_) | NodeData::Document(_) | NodeData::Fragment => {
                        let start = stack.len();
                        stack.extend(children(arena, x));
                        stack[start..].reverse();
                    }
                    NodeData::EntityRef(_) => s.push_str(&arena.text_content(x)),
                    _ => {}
                }
            }
            s
        }
        NodeData::EntityRef(_) => arena.text_content(n),
        _ => String::new(),
    }
}

fn local_name(arena: &Arena, n: NodeId) -> String {
    match arena.data(n) {
        NodeData::Element(e) => e.name.local.to_string(),
        NodeData::Attribute(a) => a.name.local.to_string(),
        NodeData::Pi { target, .. } => target.to_string(),
        NodeData::Namespace(d) => d.prefix.as_deref().unwrap_or("").to_string(),
        _ => String::new(),
    }
}

fn namespace_uri(arena: &Arena, n: NodeId) -> String {
    match arena.data(n) {
        NodeData::Element(e) => e.name.ns_str().to_string(),
        NodeData::Attribute(a) => a.name.ns_str().to_string(),
        _ => String::new(),
    }
}

fn qualified_name(arena: &Arena, n: NodeId) -> String {
    match arena.data(n) {
        NodeData::Element(e) => e.name.qualified(),
        NodeData::Attribute(a) => a.name.qualified(),
        NodeData::Pi { target, .. } => target.to_string(),
        NodeData::Namespace(d) => d.prefix.as_deref().unwrap_or("").to_string(),
        _ => String::new(),
    }
}

impl Eval<'_> {
    // ----- namespace nodes --------------------------------------------------------

    /// The namespace nodes of an element: `xml` first, then the in-scope
    /// declarations, the outermost first and in reverse declaration order
    /// within an element (observed on PHP 8.5: for `<r xmlns="d"
    /// xmlns:p="p">` the order is xml, p, default).
    fn namespace_nodes(&mut self, el: NodeId) -> Vec<NodeId> {
        let mut decls: Vec<NsDecl> = Vec::new();
        let mut seen: Vec<Option<Str>> = Vec::new();
        let mut cur = Some(el);
        while let Some(c) = cur {
            if let Some(e) = self.arena.element(c) {
                for d in &e.ns_decls {
                    if !seen.contains(&d.prefix) {
                        seen.push(d.prefix.clone());
                        if !d.uri.is_empty() && d.prefix.as_deref() != Some("xml") {
                            decls.push(d.clone());
                        }
                    }
                }
            }
            cur = self.arena.parent(c);
        }
        decls.reverse();
        let mut all = vec![NsDecl {
            prefix: Some("xml".into()),
            uri: XML_NS.into(),
        }];
        all.extend(decls);
        let mut out = Vec::with_capacity(all.len());
        for (i, d) in all.into_iter().enumerate() {
            let key = (el, d.prefix.clone());
            let node = match self.nsnodes.get(&key) {
                Some(&n) => n,
                None => {
                    let doc = self.arena.doc_of(el);
                    let n = self.arena.create(doc, NodeData::Namespace(d));
                    self.arena.node_mut(n).parent = Some(el);
                    self.nsnodes.insert(key, n);
                    self.ns_index.insert(n, i as u32);
                    self.created.push(n);
                    n
                }
            };
            out.push(node);
        }
        out
    }

    // ----- document order -----------------------------------------------------------

    fn build_order(&mut self, root: NodeId) {
        let mut key: Vec<u64> = Vec::new();
        let mut seq: u64 = 0;
        let arena = &*self.arena;
        let set = |key: &mut Vec<u64>, n: NodeId, seq: &mut u64| {
            *seq += 1;
            let i = n.index() as usize;
            if key.len() <= i {
                key.resize(i + 1, 0);
            }
            key[i] = *seq << 2;
        };
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            set(&mut key, n, &mut seq);
            if arena.is_element(n) {
                for a in arena.attrs(n) {
                    set(&mut key, a, &mut seq);
                }
            }
            if matches!(
                arena.data(n),
                NodeData::Element(_) | NodeData::Document(_) | NodeData::Fragment
            ) {
                let start = stack.len();
                stack.extend(children(arena, n));
                stack[start..].reverse();
            }
        }
        self.order = Some(Order { key });
    }

    /// Sort key, or None when the node is not in the indexed tree.
    fn key_of(&self, n: NodeId) -> Option<(u64, u32)> {
        let order = self.order.as_ref()?;
        let get = |m: NodeId| -> Option<u64> {
            order
                .key
                .get(m.index() as usize)
                .copied()
                .filter(|&k| k != 0)
        };
        if let NodeData::Namespace(_) = self.arena.data(n) {
            let owner = self.arena.parent(n)?;
            return Some((get(owner)? | 1, self.ns_index.get(&n).copied().unwrap_or(0)));
        }
        Some((get(n)?, 0))
    }

    /// Sorts into document order and removes duplicates.
    fn sort_nodes(&mut self, v: &mut Vec<NodeId>) {
        if v.len() <= 1 {
            return;
        }
        let root = self.arena.root(v[0]);
        if self.order.is_none() {
            self.build_order(root);
        }
        let keys: Option<Vec<(u64, u32)>> = v.iter().map(|&n| self.key_of(n)).collect();
        match keys {
            Some(keys) => {
                let mut pairs: Vec<((u64, u32), NodeId)> =
                    keys.into_iter().zip(v.iter().copied()).collect();
                if !pairs.windows(2).all(|w| w[0].0 < w[1].0) {
                    pairs.sort_by_key(|p| p.0);
                    pairs.dedup_by_key(|p| p.1);
                }
                v.clear();
                v.extend(pairs.into_iter().map(|p| p.1));
            }
            None => {
                let arena = &*self.arena;
                v.sort_by(|&a, &b| arena.cmp_order(a, b));
                v.dedup();
            }
        }
    }

    // ----- expressions --------------------------------------------------------------

    fn expr(&mut self, e: &Expr, c: C) -> Result<Value, XPathError> {
        Ok(match e {
            Expr::Or(a, b) => {
                Value::Boolean(self.expr(a, c)?.to_boolean() || self.expr(b, c)?.to_boolean())
            }
            Expr::And(a, b) => {
                Value::Boolean(self.expr(a, c)?.to_boolean() && self.expr(b, c)?.to_boolean())
            }
            Expr::Cmp(op, a, b) => {
                let x = self.expr(a, c)?;
                let y = self.expr(b, c)?;
                Value::Boolean(self.compare(*op, &x, &y))
            }
            Expr::Arith(op, a, b) => {
                let x = self.expr(a, c)?.to_number(self.arena);
                let y = self.expr(b, c)?.to_number(self.arena);
                Value::Number(match op {
                    ArOp::Add => x + y,
                    ArOp::Sub => x - y,
                    ArOp::Mul => x * y,
                    ArOp::Div => x / y,
                    ArOp::Mod => x % y,
                })
            }
            Expr::Neg(a) => Value::Number(-self.expr(a, c)?.to_number(self.arena)),
            Expr::Union(a, b) => {
                let x = self.expr(a, c)?;
                let y = self.expr(b, c)?;
                match (x, y) {
                    (Value::NodeSet(mut x), Value::NodeSet(y)) => {
                        x.extend(y);
                        self.sort_nodes(&mut x);
                        Value::NodeSet(x)
                    }
                    _ => return Err(XPathError::new(K::InvalidType)),
                }
            }
            Expr::Literal(s) => Value::String(s.clone()),
            Expr::Number(n) => Value::Number(*n),
            Expr::Fn(f, args) => self.core_fn(*f, args, c)?,
            Expr::ExtFn(p, l, args) => {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.expr(a, c)?);
                }
                let uri = self.ns.get(p).cloned().unwrap_or_default();
                let arena = &mut *self.arena;
                let Some(f) = self.funcs.as_deref_mut() else {
                    return Err(XPathError::with_name(
                        K::UnknownFunction,
                        &format!("{p}:{l}"),
                    ));
                };
                match f.call(arena, &uri, l, vals)? {
                    Value::NodeSet(mut v) => {
                        self.order = None;
                        self.sort_nodes(&mut v);
                        Value::NodeSet(v)
                    }
                    v => v,
                }
            }
            Expr::Path(p) => Value::NodeSet(self.path(p, c)?),
        })
    }

    fn path(&mut self, p: &PathExpr, c: C) -> Result<Vec<NodeId>, XPathError> {
        let mut nodes = match &p.start {
            Start::Root => {
                vec![self.arena.root(c.node)]
            }
            Start::Context => vec![c.node],
            Start::Filter(f, preds) => {
                let v = self.expr(f, c)?;
                let Value::NodeSet(mut v) = v else {
                    if preds.is_empty() && p.steps.is_empty() {
                        unreachable!("plain primary expressions are not paths");
                    }
                    return Err(XPathError::new(K::InvalidType));
                };
                for pred in preds {
                    v = self.filter(v, pred)?;
                }
                v
            }
        };
        for step in &p.steps {
            nodes = self.step(&nodes, step)?;
        }
        Ok(nodes)
    }

    /// Applies a predicate to `nodes` (in proximity order).
    fn filter(&mut self, nodes: Vec<NodeId>, pred: &Expr) -> Result<Vec<NodeId>, XPathError> {
        let size = nodes.len();
        // Fast path for numeric literals.
        if let Expr::Number(n) = pred {
            let n = *n;
            if n.fract() == 0.0 && n >= 1.0 && (n as usize) <= size {
                return Ok(vec![nodes[n as usize - 1]]);
            }
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for (i, &n) in nodes.iter().enumerate() {
            let v = self.expr(
                pred,
                C {
                    node: n,
                    pos: i + 1,
                    size,
                },
            )?;
            let keep = match v {
                Value::Number(x) => x == (i + 1) as f64,
                v => v.to_boolean(),
            };
            if keep {
                out.push(n);
            }
        }
        Ok(out)
    }

    fn step(&mut self, ctxs: &[NodeId], step: &Step) -> Result<Vec<NodeId>, XPathError> {
        let mut out: Vec<NodeId> = Vec::new();
        for &cn in ctxs {
            let mut v = self.axis(cn, step.axis, &step.test);
            for pred in &step.preds {
                v = self.filter(v, pred)?;
            }
            if step.axis.is_reverse() {
                v.reverse();
            }
            out.extend(v);
        }
        if ctxs.len() > 1 {
            self.sort_nodes(&mut out);
        }
        Ok(out)
    }

    fn matches(&self, n: NodeId, axis: Axis, test: &NodeTest) -> bool {
        let arena = &*self.arena;
        let data = arena.data(n);
        match test {
            NodeTest::Node => true,
            NodeTest::Text => matches!(data, NodeData::Text(_) | NodeData::CData(_)),
            NodeTest::Comment => matches!(data, NodeData::Comment(_)),
            NodeTest::Pi(t) => match data {
                NodeData::Pi { target, .. } => t.as_deref().is_none_or(|t| &**target == t),
                _ => false,
            },
            NodeTest::Any => match axis {
                Axis::Attribute => matches!(data, NodeData::Attribute(_)),
                Axis::Namespace => matches!(data, NodeData::Namespace(_)),
                _ => matches!(data, NodeData::Element(_)),
            },
            NodeTest::NsAny(p) => {
                let uri = self.ns.get(p).map(String::as_str).unwrap_or("");
                match (axis, data) {
                    (Axis::Attribute, NodeData::Attribute(a)) => a.name.ns_str() == uri,
                    (Axis::Namespace, _) => false,
                    (_, NodeData::Element(e)) => axis != Axis::Attribute && e.name.ns_str() == uri,
                    _ => false,
                }
            }
            NodeTest::Name(p, local) => {
                let uri = match p {
                    Some(p) => self.ns.get(p).map(String::as_str).unwrap_or(""),
                    None => "",
                };
                match (axis, data) {
                    (Axis::Attribute, NodeData::Attribute(a)) => {
                        &*a.name.local == local && a.name.ns_str() == uri
                    }
                    (Axis::Namespace, NodeData::Namespace(d)) => {
                        p.is_none() && d.prefix.as_deref().unwrap_or("") == local
                    }
                    (Axis::Attribute | Axis::Namespace, _) => false,
                    (_, NodeData::Element(e)) => &*e.name.local == local && e.name.ns_str() == uri,
                    _ => false,
                }
            }
        }
    }

    /// The nodes of `axis` from `n` matching `test`, in proximity order.
    fn axis(&mut self, n: NodeId, axis: Axis, test: &NodeTest) -> Vec<NodeId> {
        let mut out = Vec::new();
        match axis {
            Axis::Child => {
                let arena = &*self.arena;
                for c in children(arena, n) {
                    if self.matches(c, axis, test) {
                        out.push(c);
                    }
                }
            }
            Axis::Descendant | Axis::DescendantOrSelf => {
                if axis == Axis::DescendantOrSelf && self.matches(n, axis, test) {
                    out.push(n);
                }
                if !is_attr_or_ns(self.arena, n) {
                    let this = &*self;
                    push_descendants(this.arena, n, &mut out, &|c| this.matches(c, axis, test));
                }
            }
            Axis::Parent => {
                if let Some(p) = parent(self.arena, n)
                    && self.matches(p, axis, test)
                {
                    out.push(p);
                }
            }
            Axis::Ancestor | Axis::AncestorOrSelf => {
                if axis == Axis::AncestorOrSelf && self.matches(n, axis, test) {
                    out.push(n);
                }
                let mut cur = parent(self.arena, n);
                while let Some(p) = cur {
                    if self.matches(p, axis, test) {
                        out.push(p);
                    }
                    cur = parent(self.arena, p);
                }
            }
            Axis::FollowingSibling | Axis::PrecedingSibling => {
                if is_attr_or_ns(self.arena, n) {
                    return out;
                }
                let mut cur = n;
                loop {
                    let next = if axis == Axis::FollowingSibling {
                        self.arena.next_sibling(cur)
                    } else {
                        self.arena.prev_sibling(cur)
                    };
                    let Some(s) = next else { break };
                    cur = s;
                    if in_model(self.arena, s) && self.matches(s, axis, test) {
                        out.push(s);
                    }
                }
            }
            Axis::Following => {
                // Nodes after n in document order, excluding descendants;
                // for attributes and namespace nodes, everything after
                // their element (including its descendants).
                let arena = &*self.arena;
                let mut start = n;
                if is_attr_or_ns(arena, n) {
                    let Some(owner) = parent(arena, n) else {
                        return out;
                    };
                    let this = &*self;
                    push_descendants(arena, owner, &mut out, &|c| this.matches(c, axis, test));
                    start = owner;
                }
                let mut cur = start;
                loop {
                    let mut s = arena.next_sibling(cur);
                    while let Some(x) = s {
                        if in_model(arena, x) {
                            break;
                        }
                        s = arena.next_sibling(x);
                    }
                    match s {
                        Some(x) => {
                            if self.matches(x, axis, test) {
                                out.push(x);
                            }
                            let this = &*self;
                            push_descendants(arena, x, &mut out, &|c| this.matches(c, axis, test));
                            cur = x;
                        }
                        None => match arena.parent(cur) {
                            Some(p) => cur = p,
                            None => break,
                        },
                    }
                }
            }
            Axis::Preceding => {
                // Nodes before n in document order, excluding ancestors,
                // in reverse document order.
                let arena = &*self.arena;
                let mut cur = if is_attr_or_ns(arena, n) {
                    match parent(arena, n) {
                        Some(o) => o,
                        None => return out,
                    }
                } else {
                    n
                };
                loop {
                    let mut s = arena.prev_sibling(cur);
                    while let Some(x) = s {
                        if in_model(arena, x) {
                            break;
                        }
                        s = arena.prev_sibling(x);
                    }
                    match s {
                        Some(x) => {
                            // x's descendants in reverse document order,
                            // then x.
                            let mut d = Vec::new();
                            let this = &*self;
                            push_descendants(arena, x, &mut d, &|c| this.matches(c, axis, test));
                            d.reverse();
                            out.extend(d);
                            if self.matches(x, axis, test) {
                                out.push(x);
                            }
                            cur = x;
                        }
                        None => match arena.parent(cur) {
                            Some(p) => cur = p,
                            None => break,
                        },
                    }
                }
            }
            Axis::Attribute => {
                if self.arena.is_element(n) {
                    let attrs: Vec<NodeId> = self.arena.attrs(n).collect();
                    for a in attrs {
                        if self.matches(a, axis, test) {
                            out.push(a);
                        }
                    }
                }
            }
            Axis::Namespace => {
                if self.arena.is_element(n) {
                    for x in self.namespace_nodes(n) {
                        if self.matches(x, axis, test) {
                            out.push(x);
                        }
                    }
                }
            }
            Axis::Myself => {
                if self.matches(n, axis, test) {
                    out.push(n);
                }
            }
        }
        out
    }

    // ----- comparisons --------------------------------------------------------------

    fn compare(&mut self, op: CmpOp, x: &Value, y: &Value) -> bool {
        let arena = &*self.arena;
        match (x, y) {
            (Value::NodeSet(a), Value::NodeSet(b)) => {
                if matches!(op, CmpOp::Eq | CmpOp::Ne) {
                    let bs: Vec<String> = b.iter().map(|&n| string_value(arena, n)).collect();
                    a.iter().any(|&n| {
                        let s = string_value(arena, n);
                        bs.iter()
                            .any(|t| if op == CmpOp::Eq { s == *t } else { s != *t })
                    })
                } else {
                    let bn: Vec<f64> = b
                        .iter()
                        .map(|&n| string_to_number(&string_value(arena, n)))
                        .collect();
                    a.iter().any(|&n| {
                        let x = string_to_number(&string_value(arena, n));
                        bn.iter().any(|&y| num_cmp(op, x, y))
                    })
                }
            }
            (Value::NodeSet(a), other) | (other, Value::NodeSet(a)) => {
                let flipped = matches!(y, Value::NodeSet(_)) && !matches!(x, Value::NodeSet(_));
                let op = if flipped { flip(op) } else { op };
                match other {
                    Value::Number(m) => a
                        .iter()
                        .any(|&n| num_cmp(op, string_to_number(&string_value(arena, n)), *m)),
                    Value::String(s) => {
                        if matches!(op, CmpOp::Eq | CmpOp::Ne) {
                            a.iter().any(|&n| {
                                let v = string_value(arena, n);
                                if op == CmpOp::Eq { v == *s } else { v != *s }
                            })
                        } else {
                            let m = string_to_number(s);
                            a.iter()
                                .any(|&n| num_cmp(op, string_to_number(&string_value(arena, n)), m))
                        }
                    }
                    Value::Boolean(bv) => {
                        let nb = !a.is_empty();
                        if matches!(op, CmpOp::Eq | CmpOp::Ne) {
                            if op == CmpOp::Eq {
                                nb == *bv
                            } else {
                                nb != *bv
                            }
                        } else {
                            num_cmp(op, nb as u8 as f64, *bv as u8 as f64)
                        }
                    }
                    Value::NodeSet(_) => unreachable!(),
                }
            }
            _ => {
                if matches!(op, CmpOp::Eq | CmpOp::Ne) {
                    let eq = if matches!(x, Value::Boolean(_)) || matches!(y, Value::Boolean(_)) {
                        x.to_boolean() == y.to_boolean()
                    } else if matches!(x, Value::Number(_)) || matches!(y, Value::Number(_)) {
                        x.to_number(arena) == y.to_number(arena)
                    } else {
                        x.to_string_value(arena) == y.to_string_value(arena)
                    };
                    if op == CmpOp::Eq { eq } else { !eq }
                } else {
                    num_cmp(op, x.to_number(arena), y.to_number(arena))
                }
            }
        }
    }

    // ----- core functions ------------------------------------------------------------

    fn arg_nodes(&mut self, e: &Expr, c: C) -> Result<Vec<NodeId>, XPathError> {
        match self.expr(e, c)? {
            Value::NodeSet(v) => Ok(v),
            _ => Err(XPathError::new(K::InvalidType)),
        }
    }

    fn arg_string(&mut self, args: &[Expr], i: usize, c: C) -> Result<String, XPathError> {
        match args.get(i) {
            Some(e) => Ok(self.expr(e, c)?.to_string_value(self.arena)),
            None => Ok(string_value(self.arena, c.node)),
        }
    }

    fn arg_number(&mut self, e: &Expr, c: C) -> Result<f64, XPathError> {
        Ok(self.expr(e, c)?.to_number(self.arena))
    }

    fn core_fn(&mut self, f: CoreFn, args: &[Expr], c: C) -> Result<Value, XPathError> {
        use CoreFn::*;
        Ok(match f {
            Last => Value::Number(c.size as f64),
            Position => Value::Number(c.pos as f64),
            Count => Value::Number(self.arg_nodes(&args[0], c)?.len() as f64),
            Id => {
                let v = self.expr(&args[0], c)?;
                let mut tokens: Vec<std::string::String> = Vec::new();
                match &v {
                    Value::NodeSet(ns) => {
                        for &n in ns {
                            tokens.extend(
                                string_value(self.arena, n)
                                    .split_ascii_whitespace()
                                    .map(str::to_string),
                            );
                        }
                    }
                    v => tokens.extend(
                        v.to_string_value(self.arena)
                            .split_ascii_whitespace()
                            .map(str::to_string),
                    ),
                }
                let doc = self.arena.doc_of(c.node);
                let mut out = Vec::new();
                if matches!(self.arena.data(doc), NodeData::Document(_)) {
                    for t in tokens {
                        if let Some(e) = self.arena.get_element_by_id(doc, &t) {
                            out.push(e);
                        }
                    }
                }
                self.sort_nodes(&mut out);
                Value::NodeSet(out)
            }
            LocalName | NamespaceUri | Name => {
                let node = match args.first() {
                    Some(e) => {
                        let v = self.arg_nodes(e, c)?;
                        match v.first() {
                            Some(&n) => n,
                            None => return Ok(Value::String(std::string::String::new())),
                        }
                    }
                    None => c.node,
                };
                Value::String(match f {
                    LocalName => local_name(self.arena, node),
                    NamespaceUri => namespace_uri(self.arena, node),
                    _ => qualified_name(self.arena, node),
                })
            }
            String => Value::String(self.arg_string(args, 0, c)?),
            Concat => {
                let mut s = std::string::String::new();
                for a in args {
                    s.push_str(&self.expr(a, c)?.to_string_value(self.arena));
                }
                Value::String(s)
            }
            StartsWith | Contains | SubstringBefore | SubstringAfter => {
                let a = self.arg_string(args, 0, c)?;
                let b = self.arg_string(args, 1, c)?;
                match f {
                    StartsWith => Value::Boolean(a.starts_with(&b)),
                    Contains => Value::Boolean(a.contains(&b)),
                    SubstringBefore => Value::String(match a.find(&b) {
                        Some(i) => a[..i].to_string(),
                        None => std::string::String::new(),
                    }),
                    _ => Value::String(match a.find(&b) {
                        Some(i) => a[i + b.len()..].to_string(),
                        None => std::string::String::new(),
                    }),
                }
            }
            Substring => {
                let s = self.arg_string(args, 0, c)?;
                let start = xpath_round(self.arg_number(&args[1], c)?);
                let end = match args.get(2) {
                    Some(e) => start + xpath_round(self.arg_number(e, c)?),
                    None => f64::INFINITY,
                };
                let mut out = std::string::String::new();
                for (i, ch) in s.chars().enumerate() {
                    let p = (i + 1) as f64;
                    if p >= start && p < end {
                        out.push(ch);
                    }
                }
                Value::String(out)
            }
            StringLength => Value::Number(self.arg_string(args, 0, c)?.chars().count() as f64),
            NormalizeSpace => {
                let s = self.arg_string(args, 0, c)?;
                let parts: Vec<&str> = s
                    .split([' ', '\t', '\n', '\r'])
                    .filter(|p| !p.is_empty())
                    .collect();
                Value::String(parts.join(" "))
            }
            Translate => {
                let s = self.arg_string(args, 0, c)?;
                let from: Vec<char> = self.arg_string(args, 1, c)?.chars().collect();
                let to: Vec<char> = self.arg_string(args, 2, c)?.chars().collect();
                let mut out = std::string::String::new();
                for ch in s.chars() {
                    match from.iter().position(|&x| x == ch) {
                        Some(i) => {
                            if let Some(&r) = to.get(i) {
                                out.push(r);
                            }
                        }
                        None => out.push(ch),
                    }
                }
                Value::String(out)
            }
            Boolean => Value::Boolean(self.expr(&args[0], c)?.to_boolean()),
            Not => Value::Boolean(!self.expr(&args[0], c)?.to_boolean()),
            True => Value::Boolean(true),
            False => Value::Boolean(false),
            Lang => {
                let want = self.arg_string(args, 0, c)?.to_ascii_lowercase();
                let mut cur = Some(c.node);
                let mut found = None;
                while let Some(n) = cur {
                    if self.arena.is_element(n) {
                        let attr = self.arena.attrs(n).find(|&a| {
                            let d = self.arena.attr(a).unwrap();
                            &*d.name.local == "lang"
                                && (d.name.ns.as_deref() == Some(XML_NS)
                                    || d.name.prefix.as_deref() == Some("xml"))
                        });
                        if let Some(a) = attr {
                            found = Some(self.arena.attr_value(a).to_ascii_lowercase());
                            break;
                        }
                    }
                    cur = self.arena.parent(n);
                }
                Value::Boolean(found.is_some_and(|l| {
                    l == want
                        || (l.starts_with(&want) && l.as_bytes().get(want.len()) == Some(&b'-'))
                }))
            }
            Number => Value::Number(match args.first() {
                Some(e) => self.arg_number(e, c)?,
                None => string_to_number(&string_value(self.arena, c.node)),
            }),
            Sum => {
                let v = self.arg_nodes(&args[0], c)?;
                Value::Number(
                    v.iter()
                        .map(|&n| string_to_number(&string_value(self.arena, n)))
                        .sum(),
                )
            }
            Floor => Value::Number(self.arg_number(&args[0], c)?.floor()),
            Ceiling => Value::Number(self.arg_number(&args[0], c)?.ceil()),
            Round => Value::Number(xpath_round(self.arg_number(&args[0], c)?)),
        })
    }
}

/// XPath `round()`: halves go towards positive infinity; -0.5 <= x < 0
/// gives negative zero.
fn xpath_round(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() || x == 0.0 {
        return x;
    }
    if (-0.5..0.0).contains(&x) {
        return -0.0;
    }
    let r = (x + 0.5).floor();
    if r == 0.0 && x < 0.0 { -0.0 } else { r }
}

fn num_cmp(op: CmpOp, x: f64, y: f64) -> bool {
    match op {
        CmpOp::Eq => x == y,
        CmpOp::Ne => x != y,
        CmpOp::Lt => x < y,
        CmpOp::Le => x <= y,
        CmpOp::Gt => x > y,
        CmpOp::Ge => x >= y,
    }
}

fn flip(op: CmpOp) -> CmpOp {
    match op {
        CmpOp::Lt => CmpOp::Gt,
        CmpOp::Le => CmpOp::Ge,
        CmpOp::Gt => CmpOp::Lt,
        CmpOp::Ge => CmpOp::Le,
        o => o,
    }
}
