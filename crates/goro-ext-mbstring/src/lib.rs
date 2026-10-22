//! PHP mbstring extension for goro-rs: a port of php-src's ext/mbstring
//! (libmbfl encodings, mbstring.c functions, php_unicode.c case mapping and
//! php_mbregex.c with an Oniguruma-compatible regex engine).

pub mod arginfo;
pub mod entities;
pub mod gpc;
pub mod mail;
pub mod mbfl;
pub mod mbregex;
pub mod mbstring;
pub mod mime;
pub mod misc;
pub mod onig;
pub mod state;
pub mod strfuncs;
pub mod unicode;
pub mod zend_multibyte;
pub mod zpp;

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

pub fn register(vm: &mut Vm) {
    vm.register_extension(b"mbstring");
    mbstring::register(vm);
    vm.startup_hooks.push(zend_multibyte::startup);

    vm.constants
        .insert(b"MB_CASE_UPPER".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"MB_CASE_LOWER".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"MB_CASE_TITLE".to_vec(), Value::Long(2));
    vm.constants
        .insert(b"MB_CASE_FOLD".to_vec(), Value::Long(3));
    vm.constants
        .insert(b"MB_CASE_UPPER_SIMPLE".to_vec(), Value::Long(4));
    vm.constants
        .insert(b"MB_CASE_LOWER_SIMPLE".to_vec(), Value::Long(5));
    vm.constants
        .insert(b"MB_CASE_TITLE_SIMPLE".to_vec(), Value::Long(6));
    vm.constants
        .insert(b"MB_CASE_FOLD_SIMPLE".to_vec(), Value::Long(7));
    vm.constants.insert(
        b"MB_ONIGURUMA_VERSION".to_vec(),
        Value::String(PhpString::from_bytes(b"6.9.10")),
    );

    // Default mbstring.* INI keys to empty strings so ini_get() returns "".
    // Only set if not already configured.
    for key in [
        "mbstring.http_input",
        "mbstring.http_output",
        "mbstring.internal_encoding",
        "mbstring.encoding_translation",
        "mbstring.detect_order",
        "mbstring.substitute_character",
        "mbstring.func_overload",
        "mbstring.regex_retry_limit",
        "mbstring.regex_stack_limit",
        "mbstring.strict_detection",
        "mbstring.language",
        "mbstring.http_output_conv_mimetypes",
    ] {
        if !vm.constants.contains_key(key.as_bytes()) {
            vm.constants
                .insert(key.as_bytes().to_vec(), Value::String(PhpString::empty()));
        }
    }
}
