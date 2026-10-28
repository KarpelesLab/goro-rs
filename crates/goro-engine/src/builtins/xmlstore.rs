//! The node store the XML extensions share (ext/dom, simplexml, xmlreader's
//! `expand()`): one goro-xml [`Arena`] per thread holding every document,
//! so nodes move between documents and between extensions
//! (`dom_import_simplexml`) by handle.
//!
//! Lifetime follows PHP's: a document lives while a PHP value refers to any
//! of its nodes, and a detached subtree lives while a PHP value refers to a
//! node in it (PHP manual: DOMNode, "ownerDocument"; observed on PHP 8.5).
//! Every PHP-side reference is a [`NodeHold`], a hold on the arena node;
//! dropping the last hold frees what became unreachable.
//!
//! Holds can be dropped while the store is borrowed (a PHP object freed in
//! the middle of an operation): their release is queued and done at the
//! next access.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;

use goro_runtime::WeakObj;
use goro_xml::{Arena, NodeId};

/// Per-document data of the extensions (DOMDocument's parser flags, its
/// registered node classes...), freed with the document.
#[derive(Default)]
pub struct DocExt {
    items: Vec<(TypeId, Box<dyn Any>)>,
}

impl DocExt {
    /// The `T` of this document, created on first use.
    pub fn get<T: Default + 'static>(&mut self) -> &mut T {
        let id = TypeId::of::<T>();
        let i = match self.items.iter().position(|(t, _)| *t == id) {
            Some(i) => i,
            None => {
                self.items.push((id, Box::new(T::default())));
                self.items.len() - 1
            }
        };
        self.items[i].1.downcast_mut::<T>().unwrap()
    }

    /// The `T` of this document, if set.
    pub fn peek<T: 'static>(&self) -> Option<&T> {
        let id = TypeId::of::<T>();
        self.items
            .iter()
            .find(|(t, _)| *t == id)
            .and_then(|(_, b)| b.downcast_ref::<T>())
    }
}

/// The store.
pub struct Store {
    pub arena: Arena,
    /// Extension data of documents, by document node.
    pub docs: HashMap<NodeId, DocExt>,
    /// ext/dom's node objects: one PHP object per node while it lives
    /// (PHP keeps `$a->firstChild === $a->firstChild`).
    pub dom_objs: HashMap<NodeId, WeakObj>,
}

impl Store {
    fn new() -> Store {
        Store {
            arena: Arena::new(),
            docs: HashMap::new(),
            dom_objs: HashMap::new(),
        }
    }

    /// Extension data of the document owning `n`.
    pub fn doc_ext(&mut self, n: NodeId) -> &mut DocExt {
        let d = self.arena.doc_of(n);
        self.docs.entry(d).or_default()
    }

    /// Takes a hold on `id`.
    pub fn hold(&mut self, id: NodeId) -> NodeHold {
        self.arena.hold(id);
        NodeHold(id)
    }

    fn release(&mut self, id: NodeId) {
        if !self.arena.is_live(id) {
            return;
        }
        for d in self.arena.release(id) {
            self.docs.remove(&d);
        }
    }

    fn flush_deferred(&mut self) {
        loop {
            let ids = DEFERRED.with(|d| std::mem::take(&mut *d.borrow_mut()));
            if ids.is_empty() {
                break;
            }
            for id in ids {
                self.release(id);
            }
        }
    }

    /// Frees documents left without holds after adoptions.
    pub fn collect(&mut self) {
        for d in self.arena.collect() {
            self.docs.remove(&d);
        }
    }

    /// Frees the subtree at `n` if it is a detached root nothing holds
    /// (after an operation unlinked it).
    pub fn free_if_unheld(&mut self, n: NodeId) {
        if self.arena.is_live(n)
            && self.arena.parent(n).is_none()
            && !matches!(
                self.arena.node_type(n),
                goro_xml::NodeType::Document | goro_xml::NodeType::HtmlDocument
            )
            && !self.arena.subtree_held(n)
        {
            self.arena.free_subtree(n);
        }
    }

    /// Frees a document nothing holds (a failed parse).
    pub fn free_doc_if_unheld(&mut self, d: NodeId) {
        if self.arena.free_document_if_unheld(d) {
            self.docs.remove(&d);
        }
    }
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::new());
    static DEFERRED: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
}

/// Runs `f` with the store. Must not be nested (`f` must not call back into
/// PHP code or into another `with`).
pub fn with<T>(f: impl FnOnce(&mut Store) -> T) -> T {
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        s.flush_deferred();
        let r = f(&mut s);
        s.flush_deferred();
        r
    })
}

/// True if the store is borrowed (code running inside [`with`]).
pub fn busy() -> bool {
    STORE.with(|s| s.try_borrow_mut().is_err())
}

/// A hold on a node: the node (its document, its detached subtree) stays
/// alive while this exists.
pub struct NodeHold(NodeId);

impl NodeHold {
    #[inline]
    pub fn id(&self) -> NodeId {
        self.0
    }

    /// A new hold on the same node (needs the store: not inside [`with`]).
    pub fn dup(&self, s: &mut Store) -> NodeHold {
        s.hold(self.0)
    }
}

impl Drop for NodeHold {
    fn drop(&mut self) {
        let id = self.0;
        let done = STORE
            .try_with(|s| match s.try_borrow_mut() {
                Ok(mut s) => {
                    s.flush_deferred();
                    s.release(id);
                    true
                }
                Err(_) => false,
            })
            .unwrap_or(true);
        if !done {
            let _ = DEFERRED.try_with(|d| d.borrow_mut().push(id));
        }
    }
}

impl std::fmt::Debug for NodeHold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NodeHold({})", self.0.index())
    }
}
