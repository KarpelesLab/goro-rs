use goro_core::vm::Vm;

/// Register the Reflection extension.
///
/// The actual reflection implementation lives in `goro_core::reflection`.
/// This crate provides the extension entry point for consistency with
/// other goro-ext-* crates, and registers any top-level reflection
/// functions (currently none -- all reflection functionality is accessed
/// through Reflection class method dispatch in the VM).
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"reflection");
    // Reflection classes are handled internally by the VM's class dispatch.
    // Class constants (e.g. ReflectionMethod::IS_PUBLIC) are resolved via
    // Vm::get_builtin_class_constant().
    //
    // No top-level functions need to be registered for the reflection
    // extension at this time.
}
