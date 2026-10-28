//! The engine's built-in modules: core classes and functions, variable
//! handling, output control, strings, arrays, math, files, json...

pub mod array;
pub mod core_classes;
pub mod core_funcs;
pub mod ctype;
pub mod enums;
pub mod exec;
pub mod fiber;
pub mod file;
pub mod hash;
pub mod highlight;
pub mod info;
mod info_text;
pub mod ini;
pub mod json;
pub mod math;
pub mod mbstring;
pub mod misc;
pub mod output;
pub mod password;
pub mod pcre;
pub mod reflection;
pub mod serialize;
pub mod spl;
pub mod string;
pub mod strmisc;
pub mod userstream;
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
        &hash::MODULE,
        &info::MODULE,
        &fiber::MODULE,
        &file::MODULE,
        &serialize::MODULE,
        &json::MODULE,
        &reflection::MODULE,
        &spl::MODULE,
        &weakmap::MODULE,
        &userstream::MODULE,
        &string::MODULE,
        &strmisc::MODULE,
        &password::MODULE,
        &misc::MODULE,
        &exec::MODULE,
        &highlight::MODULE,
        &ini::MODULE,
        &pcre::MODULE,
        &mbstring::MODULE,
        &ctype::MODULE,
        &array::MODULE,
        &math::MODULE,
    ]
}
