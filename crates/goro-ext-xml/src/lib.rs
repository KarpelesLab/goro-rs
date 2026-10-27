pub mod c14n;
pub mod dom;
pub mod html;
pub mod html5;
mod image_svg;
pub mod io;
pub mod libxml;
pub mod parser;
pub mod pattern;
pub mod relaxng;
pub mod sax2;
pub mod serialize;
pub mod simplexml;
pub mod tree;
pub mod uri;
pub mod valid;
pub mod xinclude;
pub mod xmlerrors;
pub mod xmlregexp;
pub mod xmlschema;
pub mod xmlschemastypes;
pub mod xmlunicode;
pub mod xpath;

/// The XML node behind a DOM object (for simplexml_import_dom).
pub fn dom_node_of(
    o: &std::rc::Rc<std::cell::RefCell<goro_core::object::PhpObject>>,
) -> Option<tree::NodeId> {
    dom::node_of(o)
}

use goro_core::vm::Vm;

mod xml_sax;
mod xmlreader;
mod xmlwriter;

// ── Public registration ─────────────────────────────────────────────────────

pub fn register(vm: &mut Vm) {
    vm.register_extension(b"xml");
    xmlwriter::register(vm);
    xml_sax::register(vm);

    libxml::register(vm);
    simplexml::register(vm);
    dom::register(vm);
    xmlreader::register(vm);
}
