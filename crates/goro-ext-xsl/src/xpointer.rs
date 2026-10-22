//! XPointer evaluation for document() fragment identifiers (xmlXPtrEval).

use crate::transform::Transform;
use crate::xpath::{self, XObj};
use goro_ext_xml::tree::NodeId;

/// Evaluate an XPointer against `doc`: xpointer()/xpath1() schemes and
/// shorthand (ID) pointers.
pub fn eval(t: &mut Transform, doc: NodeId, frag: &str) -> Option<XObj> {
    let frag = frag.trim();
    if let Some(inner) = frag
        .strip_prefix("xpointer(")
        .or_else(|| frag.strip_prefix("xpath1("))
        .and_then(|r| r.strip_suffix(')'))
    {
        let comp = xpath::compile(inner).ok()?;
        let saved = t.xp.clone();
        t.xp.node = Some(doc);
        t.xp.doc = Some(doc);
        let r = xpath::eval(t, &comp);
        let d = t.xp.depth;
        t.xp = saved;
        t.xp.depth = d;
        return r;
    }
    let id = goro_ext_xml::tree::with(|tr| {
        tr.doc_info(doc)
            .and_then(|i| i.ids.get(frag).copied())
            .and_then(|a| tr.nodes[a].parent)
    });
    Some(XObj::NodeSet(id.into_iter().collect()))
}
