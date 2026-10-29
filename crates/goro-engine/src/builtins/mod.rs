//! The engine's built-in modules: core classes and functions, variable
//! handling, output control, strings, arrays, math, files, json...

pub mod array;
pub mod bcmath;
pub mod browscap;
pub mod bz2;
pub mod calendar;
pub mod core_classes;
pub mod core_funcs;
pub mod ctype;
pub mod curl;
pub mod date;
pub mod dir;
pub mod dom;
pub mod enums;
pub mod exec;
pub mod exif;
pub mod fiber;
pub mod file;
pub mod filter;
pub mod gd;
pub mod gettext;
pub mod gmp;
pub mod hash;
pub mod headers;
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
pub mod mail;
pub mod math;
pub mod mbstring;
pub mod misc;
pub mod net;
pub mod opcache;
pub mod openssl;
pub mod output;
pub mod password;
pub mod pcntl;
pub mod pcre;
pub mod phar;
pub mod posix;
pub mod random;
pub mod readline;
pub mod reflection;
pub mod serialize;
pub mod session;
pub mod simplexml;
pub mod sockets;
pub mod sodium;
pub mod spl;
pub mod spl_autoload;
pub mod stdmisc;
pub mod streams;
pub mod string;
pub mod strmisc;
pub mod sysv;
pub mod tokenizer;
pub mod uri;
pub mod userstream;
pub mod var;
pub mod weakmap;
pub mod xml;
pub mod xmlreader;
pub mod xmlstore;
pub mod xmlwriter;
pub mod zip;
pub mod zlib;

use crate::registry::Module;

/// The modules every engine has.
pub fn modules() -> Vec<&'static Module> {
    let mut v = vec![
        &core_funcs::MODULE,
        &headers::MODULE,
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
        &posix::MODULE,
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
        &mail::MODULE,
        &net::MODULE,
        &dir::MODULE,
        &stdmisc::MODULE,
        &browscap::MODULE,
        &exec::MODULE,
        &highlight::MODULE,
        &ini::MODULE,
        &pcre::MODULE,
        &mbstring::MODULE,
        &iconv::MODULE,
        &zlib::MODULE,
        &zip::MODULE,
        &bz2::MODULE,
        &phar::MODULE,
        &ctype::MODULE,
        &calendar::MODULE,
        &tokenizer::MODULE,
        &gettext::MODULE,
        &readline::MODULE,
        &sysv::SHMOP,
        &sysv::SYSVSEM,
        &sysv::SYSVSHM,
        &sysv::SYSVMSG,
        &sysv::FTOK,
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
        &opcache::MODULE,
        &session::MODULE,
        &session::rewrite::MODULE,
        &gd::MODULE,
        &exif::MODULE,
        &curl::MODULE,
        &image::MODULE,
    ];
    v.extend_from_slice(intl::MODULES);
    v
}
