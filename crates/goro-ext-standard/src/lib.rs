pub mod math;
pub mod misc;
pub mod output;
pub mod strings;
pub mod type_funcs;

use goro_core::vm::Vm;

/// Register all standard extension functions
pub fn register_standard_functions(vm: &mut Vm) {
    output::register(vm);
    strings::register(vm);
    type_funcs::register(vm);
    math::register(vm);
    misc::register(vm);
}
