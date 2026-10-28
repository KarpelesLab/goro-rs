//! The engine's built-in modules: core classes and functions, variable
//! handling, output control, strings, arrays, math, files, json...

pub mod array;
pub mod core_classes;
pub mod core_funcs;
pub mod ctype;
pub mod enums;
pub mod fiber;
pub mod file;
pub mod ini;
pub mod json;
pub mod math;
pub mod mbstring;
pub mod exec;
pub mod misc;
pub mod output;
pub mod password;
pub mod pcre;
pub mod reflection;
pub mod serialize;
pub mod spl;
pub mod string;
pub mod strmisc;
pub mod var;
pub mod weakmap;

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
        &spl::MODULE,
        &weakmap::MODULE,
        &string::MODULE,
        &strmisc::MODULE,
        &password::MODULE,
        &misc::MODULE,
        &exec::MODULE,
        &ini::MODULE,
        &pcre::MODULE,
        &mbstring::MODULE,
        &ctype::MODULE,
        &array::MODULE,
        &math::MODULE,
    ]
}
