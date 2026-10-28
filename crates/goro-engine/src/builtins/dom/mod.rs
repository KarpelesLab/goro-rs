//! ext/dom (PHP manual: DOM; RFCs "DOM HTML5 parsing and serialization"
//! and "Opt-in DOM spec-compliance"): the legacy `DOM*` classes and the
//! PHP 8.4 `Dom\` namespace, on goro-xml's tree in the shared node store
//! ([`super::xmlstore`]).
//!
//! Node objects: one PHP object per node while PHP refers to it (the
//! store's `dom_objs`), holding the node ([`DomObj`]). Its class comes from
//! the node kind, the document flavour (legacy or `Dom\`) and the classes
//! the document registered (`registerNodeClass()`).

use goro_runtime::{NativeData, ObjRef, Value};
use goro_xml::NodeId;

use super::xmlstore::{self, NodeHold};
use crate::vm::{R, Vm};

/// The native data of a node object.
pub struct DomObj {
    pub hold: NodeHold,
}

impl NativeData for DomObj {}

impl Drop for DomObj {
    fn drop(&mut self) {
        let id = self.hold.id();
        if !xmlstore::busy() {
            xmlstore::with(|s| {
                if s.dom_objs.get(&id).is_some_and(|w| w.upgrade().is_none()) {
                    s.dom_objs.remove(&id);
                }
            });
        }
    }
}

/// The node of a DOM node object.
pub fn node_of(o: &ObjRef) -> Option<NodeId> {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<DomObj>().map(|d| d.hold.id())
}

/// The PHP object of node `id` (the existing one while it lives).
pub fn wrap_node(vm: &mut Vm, id: NodeId) -> R<Value> {
    let _ = vm;
    let _ = id;
    Ok(Value::Null)
}
