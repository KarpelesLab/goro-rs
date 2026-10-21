//! The minimal lexbor-like DOM the tree builder works on. Node 0 is the
//! active formatting marker (an element with tag/namespace UNDEF).

pub type NodeId = usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LKind {
    Element,
    Text,
    Comment,
    Doctype,
    Document,
    Fragment,
}

#[derive(Clone, Debug, Default)]
pub struct LAttr {
    /// local name (lowercased by the tokenizer, or adjusted)
    pub local: Vec<u8>,
    /// lexbor's qualified name when it differs (SVG camelCase, xlink:href)
    pub qualified: Option<Vec<u8>>,
    pub has_prefix: bool,
    pub ns: u8,
    pub value: Option<Vec<u8>>,
}

impl LAttr {
    /// lxb_dom_attr_qualified_name
    pub fn qname(&self) -> &[u8] {
        self.qualified.as_deref().unwrap_or(&self.local)
    }
}

#[derive(Clone, Debug)]
pub struct LNode {
    pub kind: LKind,
    pub tag: u32,
    pub ns: u8,
    pub parent: Option<NodeId>,
    pub first: Option<NodeId>,
    pub last: Option<NodeId>,
    pub prev: Option<NodeId>,
    pub next: Option<NodeId>,
    pub attrs: Vec<LAttr>,
    /// element qualified name override (SVG fixed-case names)
    pub qualified: Option<Vec<u8>>,
    /// text / comment data; doctype name
    pub data: Vec<u8>,
    pub public_id: Vec<u8>,
    pub system_id: Vec<u8>,
    pub line: usize,
    pub template_content: Option<NodeId>,
}

impl LNode {
    fn new(kind: LKind, tag: u32, ns: u8) -> LNode {
        LNode {
            kind,
            tag,
            ns,
            parent: None,
            first: None,
            last: None,
            prev: None,
            next: None,
            attrs: Vec::new(),
            qualified: None,
            data: Vec::new(),
            public_id: Vec::new(),
            system_id: Vec::new(),
            line: 0,
            template_content: None,
        }
    }
}

pub struct LDom {
    pub nodes: Vec<LNode>,
}

impl Default for LDom {
    fn default() -> Self {
        LDom::new()
    }
}

impl LDom {
    pub fn new() -> LDom {
        LDom {
            nodes: vec![LNode::new(LKind::Element, 0, 0)],
        }
    }

    fn alloc(&mut self, n: LNode) -> NodeId {
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    pub fn new_document(&mut self) -> NodeId {
        self.alloc(LNode::new(
            LKind::Document,
            super::_DOCUMENT,
            super::NS_HTML,
        ))
    }

    /// lxb_html_interface_create for an element (templates get content).
    pub fn new_element(&mut self, tag: u32, ns: u8) -> NodeId {
        let id = self.alloc(LNode::new(LKind::Element, tag, ns));
        if tag == super::TEMPLATE && ns == super::NS_HTML {
            let frag = self.alloc(LNode::new(LKind::Fragment, 0, 0));
            self.nodes[id].template_content = Some(frag);
        }
        id
    }

    pub fn new_text(&mut self, data: Vec<u8>) -> NodeId {
        let mut n = LNode::new(LKind::Text, super::_TEXT, super::NS_HTML);
        n.data = data;
        self.alloc(n)
    }

    pub fn new_comment(&mut self, ns: u8, data: Vec<u8>) -> NodeId {
        let mut n = LNode::new(LKind::Comment, super::_EM_COMMENT, ns);
        n.data = data;
        self.alloc(n)
    }

    pub fn new_doctype(&mut self) -> NodeId {
        self.alloc(LNode::new(
            LKind::Doctype,
            super::_EM_DOCTYPE,
            super::NS_HTML,
        ))
    }

    /// lxb_dom_node_remove_wo_events
    pub fn remove(&mut self, n: NodeId) {
        let (parent, prev, next) = {
            let x = &self.nodes[n];
            (x.parent, x.prev, x.next)
        };
        if let Some(p) = parent {
            if self.nodes[p].first == Some(n) {
                self.nodes[p].first = next;
            }
            if self.nodes[p].last == Some(n) {
                self.nodes[p].last = prev;
            }
        }
        if let Some(nx) = next {
            self.nodes[nx].prev = prev;
        }
        if let Some(pv) = prev {
            self.nodes[pv].next = next;
        }
        let x = &mut self.nodes[n];
        x.parent = None;
        x.prev = None;
        x.next = None;
    }

    /// lxb_dom_node_insert_child_wo_events: append `n` to `to`
    pub fn append_child(&mut self, to: NodeId, n: NodeId) {
        let last = self.nodes[to].last;
        if let Some(l) = last {
            self.nodes[l].next = Some(n);
        } else {
            self.nodes[to].first = Some(n);
        }
        let x = &mut self.nodes[n];
        x.parent = Some(to);
        x.next = None;
        x.prev = last;
        self.nodes[to].last = Some(n);
    }

    /// lxb_dom_node_insert_before_wo_events: insert `n` before `to`
    pub fn insert_before(&mut self, to: NodeId, n: NodeId) {
        let (parent, prev) = (self.nodes[to].parent, self.nodes[to].prev);
        if let Some(p) = prev {
            self.nodes[p].next = Some(n);
        } else if let Some(par) = parent {
            self.nodes[par].first = Some(n);
        }
        let x = &mut self.nodes[n];
        x.parent = parent;
        x.next = Some(to);
        x.prev = prev;
        self.nodes[to].prev = Some(n);
    }

    pub fn children(&self, n: NodeId) -> Vec<NodeId> {
        let mut v = Vec::new();
        let mut c = self.nodes[n].first;
        while let Some(x) = c {
            v.push(x);
            c = self.nodes[x].next;
        }
        v
    }

    /// lxb_dom_element_attr_is_exist / attr_by_local_name_data
    pub fn attr_find(&self, el: NodeId, name: &[u8]) -> Option<usize> {
        self.nodes[el]
            .attrs
            .iter()
            .position(|a| a.local == name || a.qualified.as_deref() == Some(name))
    }

    /// lxb_dom_element_compare (Noah's Ark clause)
    pub fn element_compare(&self, a: NodeId, b: NodeId) -> bool {
        let (x, y) = (&self.nodes[a], &self.nodes[b]);
        if x.tag != y.tag || x.ns != y.ns || x.qualified != y.qualified {
            return false;
        }
        if x.attrs.len() != y.attrs.len() {
            return false;
        }
        x.attrs.iter().all(|fa| {
            y.attrs.iter().any(|sa| {
                fa.local == sa.local
                    && fa.ns == sa.ns
                    && fa.qualified == sa.qualified
                    && fa.value == sa.value
            })
        })
    }
}
