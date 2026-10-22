//! php-src's ext/xsl: an XSLT 1.0 processor (a port of libxslt 1.1.45 and
//! libexslt) running on goro-ext-xml's tree, with its own XPath 1.0 engine
//! (a port of libxml2's xpath.c that knows about XSLT's extensions).

// The port follows libxslt's control flow closely (same-branch bodies,
// NaN-aware negated comparisons, C-style counters, unit errors).
#![allow(
    clippy::result_unit_err,
    clippy::if_same_then_else,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::explicit_counter_loop,
    clippy::needless_late_init,
    clippy::map_entry,
    clippy::question_mark
)]

use goro_core::vm::Vm;

pub mod apply;
pub mod document;
pub mod exslt;
pub mod functions;
pub mod numbers;
pub mod output;
pub mod pattern;
pub mod php;
pub mod profile;
pub mod security;
pub mod sort;
pub mod style;
pub mod transform;
pub mod util;
pub mod vars;
pub mod xpath;
pub mod xpointer;

/// The extension name (`extension_loaded('xsl')`).
pub const NAME: &[u8] = b"xsl";

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    // PHP_MINFO_FUNCTION(xsl)
    vm.phpinfo_hooks.push(|_| {
        "\nxsl\n\nXSL => enabled\nlibxslt Version => 1.1.45\nlibxslt compiled against libxml Version => 2.13.8\nEXSLT => enabled\nlibexslt Version => 0.8.25\n"
            .to_string()
    });
    php::register(vm);
}
