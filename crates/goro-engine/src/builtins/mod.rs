//! The engine's built-in modules: core classes and functions, variable
//! handling, output control, strings, arrays, math, files, json...

pub mod core_classes;
pub mod core_funcs;
pub mod enums;
pub mod fiber;
pub mod file;
pub mod json;
pub mod reflection;
pub mod output;
pub mod serialize;
pub mod var;

use crate::registry::Module;

/// The modules every engine has.
pub fn modules() -> Vec<&'static Module> {
    vec![
        &core_funcs::MODULE,
        &core_classes::MODULE,
        &var::MODULE,
        &output::MODULE,
        &fiber::MODULE,
        &file::MODULE,
        &serialize::MODULE,
        &json::MODULE,
        &reflection::MODULE,
    ]
}
