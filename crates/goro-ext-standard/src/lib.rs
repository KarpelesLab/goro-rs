pub mod output;
pub mod strings;
pub mod type_funcs;
pub mod math;

use goro_core::vm::Vm;

/// Register all standard extension functions
pub fn register_standard_functions(vm: &mut Vm) {
    output::register(vm);
    strings::register(vm);
    type_funcs::register(vm);
    math::register(vm);
}
