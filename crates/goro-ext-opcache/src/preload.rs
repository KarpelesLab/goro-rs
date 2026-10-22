//! Preloading (opcache.preload, accel_preload() in ZendAccelerator.c).

use goro_core::value::Value;
use goro_core::vm::Vm;

pub(crate) fn run(_vm: &mut Vm, _script: &str) {}

pub(crate) fn active() -> bool {
    false
}

pub(crate) fn compile_file(_vm: &mut Vm, _path: &str, _name: &str) -> bool {
    false
}

pub(crate) fn statistics() -> Option<Value> {
    None
}

pub(crate) fn compiled_in_file_cache(_path: &str) -> bool {
    false
}
