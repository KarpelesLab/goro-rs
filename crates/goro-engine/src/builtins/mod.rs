//! The engine's built-in modules: core classes and functions, variable
//! handling, output control, strings, arrays, math, files, json...

pub mod array;
pub mod core_classes;
pub mod core_funcs;
pub mod enums;
pub mod math;
pub mod output;
pub mod string;
pub mod strmisc;
pub mod var;

use crate::registry::Module;

/// The modules every engine has.
pub fn modules() -> Vec<&'static Module> {
    vec![
        &core_funcs::MODULE,
        &core_classes::MODULE,
        &var::MODULE,
        &output::MODULE,
        &string::MODULE,
        &strmisc::MODULE,
        &array::MODULE,
        &math::MODULE,
    ]
}
