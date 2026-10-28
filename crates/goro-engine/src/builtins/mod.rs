//! The engine's built-in modules: core classes and functions, variable
//! handling, output control, strings, arrays, math, files, json...

pub mod array;
pub mod bcmath;
pub mod core_classes;
pub mod core_funcs;
pub mod ctype;
pub mod date;
pub mod dom;
pub mod enums;
pub mod exec;
pub mod exif;
pub mod fiber;
pub mod file;
pub mod filter;
pub mod gmp;
pub mod hash;
pub mod highlight;
pub mod http;
pub mod iconv;
pub mod image;
pub mod info;
mod info_text;
pub mod ini;
mod intl;
pub mod json;
pub mod libxml;
pub mod math;
pub mod mbstring;
pub mod misc;
pub mod openssl;
pub mod output;
pub mod password;
pub mod pcntl;
pub mod pcre;
pub mod random;
pub mod reflection;
pub mod serialize;
pub mod simplexml;
pub mod sockets;
pub mod sodium;
pub mod spl;
pub mod spl_autoload;
pub mod streams;
pub mod string;
pub mod strmisc;
pub mod uri;
pub mod userstream;
pub mod var;
pub mod weakmap;
pub mod xml;
pub mod xmlreader;
pub mod xmlstore;
pub mod xmlwriter;
pub mod zlib;

use crate::registry::Module;

/// The modules every engine has.
pub fn modules() -> Vec<&'static Module> {
    let mut v = vec![
        &core_funcs::MODULE,
        &core_classes::MODULE,
        &var::MODULE,
        &output::MODULE,
        &hash::MODULE,
        &info::MODULE,
        &fiber::MODULE,
        &file::MODULE,
        &sockets::MODULE,
        &http::MODULE,
        &pcntl::MODULE,
        &streams::MODULE,
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
        &iconv::MODULE,
        &zlib::MODULE,
        &ctype::MODULE,
        &date::MODULE,
        &array::MODULE,
        &math::MODULE,
        &random::MODULE,
        &bcmath::MODULE,
        &gmp::MODULE,
        &filter::MODULE,
        &sodium::MODULE,
        &uri::MODULE,
        &libxml::MODULE,
        &dom::MODULE,
        &xml::MODULE,
        &xmlwriter::MODULE,
        &xmlreader::MODULE,
        &simplexml::MODULE,
        &openssl::MODULE,
        &exif::MODULE,
        &image::MODULE,
    ];
    v.extend_from_slice(intl::MODULES);
    v
}
