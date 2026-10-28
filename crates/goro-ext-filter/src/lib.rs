//! PHP's filter extension (PHP manual: Data Filtering) over goro-runtime
//! values: `filter_var`, `filter_var_array`, `filter_input`,
//! `filter_input_array`, `filter_has_var`, `filter_list` and `filter_id`,
//! with every validation and sanitizing filter of PHP 8.5.
//!
//! Behaviour follows the PHP manual (filter functions, "Types of filters",
//! the flag and constant lists), the expected output of php-src's
//! `ext/filter/tests/*.phpt`, public specifications (RFC 5321/5322 for
//! e-mail addresses, RFC 4291 for IPv6, the IANA special-purpose address
//! registries for the range flags), and observations of PHP 8.5: the
//! fixture in `tests/fixtures` was generated once from the reference binary
//! by `tests/fixtures/filter_driver.php`.
//!
//! # Engine wiring
//!
//! The engine implements [`FilterHost`] (it extends goro-runtime's
//! [`Ctx`], which carries warnings, ini `precision` and object
//! conversions) and calls [`filter_var`], [`filter_var_array`],
//! [`filter_input`], [`filter_input_array`], [`filter_has_var`],
//! [`filter_list`] and [`filter_id`] after its usual argument coercion:
//! `$options` (`array|int`) arrives as [`Options`], `$filter` and
//! `$type` as ints. The host supplies:
//!
//! - `__toString()` ([`FilterHost::object_string`]), `is_callable()` and
//!   calls for `FILTER_CALLBACK` ([`FilterHost::is_callable`],
//!   [`FilterHost::call`]);
//! - the request's input arrays for `INPUT_*` ([`FilterHost::input`]):
//!   the values as the SAPI received them, before `filter.default` was
//!   applied to the superglobals (test ext/filter/tests/041.phpt), and
//!   not affected by later changes to `$_GET` and friends;
//! - the exceptions it throws: `TypeError`/`ValueError`
//!   ([`FilterHost::throw_error`]) and `Filter\FilterFailedException`
//!   ([`FilterHost::filter_failed`]; PHP 8.5 class hierarchy:
//!   `Filter\FilterFailedException extends Filter\FilterException extends
//!   Exception`, code 0);
//! - ini `default_charset` for `FILTER_SANITIZE_FULL_SPECIAL_CHARS`, the
//!   PCRE limits for `FILTER_VALIDATE_REGEXP`, and optionally the ext/uri
//!   parsers for the `uri_parser_class` option of `FILTER_VALIDATE_URL`.
//!
//! [`consts`] lists the constants (two of them deprecated). For the SAPI,
//! [`InputFilter`] applies ini `filter.default`/`filter.default_flags` to
//! incoming request variables.
//!
//! # Approximations
//!
//! - PHP's e-mail check gives up on local parts with many quoted words
//!   (apparently its regular expression hits the PCRE backtracking limit);
//!   that limit is modelled by a cost estimate fitted to observations (see
//!   `email.rs`), exact on every case tried but not derived from PCRE.
//! - With the `uri_parser_class` option FILTER_VALIDATE_URL needs the
//!   engine's ext/uri parsers ([`FilterHost::parse_uri`]); without them it
//!   falls back to the parse_url()-based check.

pub mod consts;

mod apply;
mod email;
mod ip;
mod sanitize;
mod url;
mod validate;

use consts::*;
use goro_runtime::{Ctx, ObjRef, PhpArray, PhpError, PhpStr, Value};

pub use apply::{
    filter_has_var, filter_id, filter_input, filter_input_array, filter_list, filter_var,
    filter_var_array,
};
pub use email::valid_email;
pub use ip::{Ip, parse_ip};
pub use sanitize::InputFilter;
pub use validate::valid_domain;

/// A filter (PHP manual: "Types of filters").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Filter {
    Int,
    Bool,
    Float,
    Regexp,
    Domain,
    Url,
    Email,
    Ip,
    Mac,
    /// `FILTER_SANITIZE_STRING` / `FILTER_SANITIZE_STRIPPED`.
    String,
    Encoded,
    SpecialChars,
    FullSpecialChars,
    /// `FILTER_UNSAFE_RAW` / `FILTER_DEFAULT`.
    UnsafeRaw,
    SanitizeEmail,
    SanitizeUrl,
    NumberInt,
    NumberFloat,
    AddSlashes,
    Callback,
}

/// `filter_list()` order with ids (test ext/filter/tests/008.phpt,
/// 009.phpt; "stripped" is a second name of 513).
const LIST: [(&str, i64); 21] = [
    ("int", FILTER_VALIDATE_INT),
    ("boolean", FILTER_VALIDATE_BOOL),
    ("float", FILTER_VALIDATE_FLOAT),
    ("validate_regexp", FILTER_VALIDATE_REGEXP),
    ("validate_domain", FILTER_VALIDATE_DOMAIN),
    ("validate_url", FILTER_VALIDATE_URL),
    ("validate_email", FILTER_VALIDATE_EMAIL),
    ("validate_ip", FILTER_VALIDATE_IP),
    ("validate_mac", FILTER_VALIDATE_MAC),
    ("string", FILTER_SANITIZE_STRING),
    ("stripped", FILTER_SANITIZE_STRIPPED),
    ("encoded", FILTER_SANITIZE_ENCODED),
    ("special_chars", FILTER_SANITIZE_SPECIAL_CHARS),
    ("full_special_chars", FILTER_SANITIZE_FULL_SPECIAL_CHARS),
    ("unsafe_raw", FILTER_UNSAFE_RAW),
    ("email", FILTER_SANITIZE_EMAIL),
    ("url", FILTER_SANITIZE_URL),
    ("number_int", FILTER_SANITIZE_NUMBER_INT),
    ("number_float", FILTER_SANITIZE_NUMBER_FLOAT),
    ("add_slashes", FILTER_SANITIZE_ADD_SLASHES),
    ("callback", FILTER_CALLBACK),
];

impl Filter {
    /// The filter with this id, if any.
    pub fn from_id(id: i64) -> Option<Filter> {
        Some(match id {
            FILTER_VALIDATE_INT => Filter::Int,
            FILTER_VALIDATE_BOOL => Filter::Bool,
            FILTER_VALIDATE_FLOAT => Filter::Float,
            FILTER_VALIDATE_REGEXP => Filter::Regexp,
            FILTER_VALIDATE_DOMAIN => Filter::Domain,
            FILTER_VALIDATE_URL => Filter::Url,
            FILTER_VALIDATE_EMAIL => Filter::Email,
            FILTER_VALIDATE_IP => Filter::Ip,
            FILTER_VALIDATE_MAC => Filter::Mac,
            FILTER_SANITIZE_STRING => Filter::String,
            FILTER_SANITIZE_ENCODED => Filter::Encoded,
            FILTER_SANITIZE_SPECIAL_CHARS => Filter::SpecialChars,
            FILTER_SANITIZE_FULL_SPECIAL_CHARS => Filter::FullSpecialChars,
            FILTER_UNSAFE_RAW => Filter::UnsafeRaw,
            FILTER_SANITIZE_EMAIL => Filter::SanitizeEmail,
            FILTER_SANITIZE_URL => Filter::SanitizeUrl,
            FILTER_SANITIZE_NUMBER_INT => Filter::NumberInt,
            FILTER_SANITIZE_NUMBER_FLOAT => Filter::NumberFloat,
            FILTER_SANITIZE_ADD_SLASHES => Filter::AddSlashes,
            FILTER_CALLBACK => Filter::Callback,
            _ => return None,
        })
    }

    /// The `FILTER_*` id.
    pub fn id(self) -> i64 {
        match self {
            Filter::Int => FILTER_VALIDATE_INT,
            Filter::Bool => FILTER_VALIDATE_BOOL,
            Filter::Float => FILTER_VALIDATE_FLOAT,
            Filter::Regexp => FILTER_VALIDATE_REGEXP,
            Filter::Domain => FILTER_VALIDATE_DOMAIN,
            Filter::Url => FILTER_VALIDATE_URL,
            Filter::Email => FILTER_VALIDATE_EMAIL,
            Filter::Ip => FILTER_VALIDATE_IP,
            Filter::Mac => FILTER_VALIDATE_MAC,
            Filter::String => FILTER_SANITIZE_STRING,
            Filter::Encoded => FILTER_SANITIZE_ENCODED,
            Filter::SpecialChars => FILTER_SANITIZE_SPECIAL_CHARS,
            Filter::FullSpecialChars => FILTER_SANITIZE_FULL_SPECIAL_CHARS,
            Filter::UnsafeRaw => FILTER_UNSAFE_RAW,
            Filter::SanitizeEmail => FILTER_SANITIZE_EMAIL,
            Filter::SanitizeUrl => FILTER_SANITIZE_URL,
            Filter::NumberInt => FILTER_SANITIZE_NUMBER_INT,
            Filter::NumberFloat => FILTER_SANITIZE_NUMBER_FLOAT,
            Filter::AddSlashes => FILTER_SANITIZE_ADD_SLASHES,
            Filter::Callback => FILTER_CALLBACK,
        }
    }

    /// The first `filter_list()` name of the filter, as used in the
    /// FILTER_THROW_ON_FAILURE message ("filter int not satisfied by ...",
    /// tests ext/filter/tests/throw-on-failure/*.phpt).
    pub fn name(self) -> &'static str {
        let id = self.id();
        LIST.iter()
            .find(|(_, i)| *i == id)
            .map(|(n, _)| *n)
            .unwrap()
    }
}

/// An `INPUT_*` source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputType {
    Post,
    Get,
    Cookie,
    Env,
    Server,
}

impl InputType {
    /// The source for an `INPUT_*` value (3 and 6, once INPUT_REQUEST and
    /// INPUT_SESSION, are not valid in PHP 8.5: observed).
    pub fn from_i64(v: i64) -> Option<InputType> {
        Some(match v {
            INPUT_POST => InputType::Post,
            INPUT_GET => InputType::Get,
            INPUT_COOKIE => InputType::Cookie,
            INPUT_ENV => InputType::Env,
            INPUT_SERVER => InputType::Server,
            _ => return None,
        })
    }
}

/// The `$options` argument (`array|int`) after the engine's argument
/// coercion: an int is the flags (for `filter_var`/`filter_input`) or the
/// filter (for the `_array` functions).
#[derive(Clone, Copy, Debug)]
pub enum Options<'a> {
    Int(i64),
    Array(&'a PhpArray),
}

/// The ext/uri parsers the `uri_parser_class` option of FILTER_VALIDATE_URL
/// names (PHP 8.5; test ext/filter/tests/062.phpt).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UriParser {
    /// `Uri\Rfc3986\Uri`.
    Rfc3986,
    /// `Uri\WhatWg\Url`.
    WhatWg,
}

/// What FILTER_VALIDATE_URL needs from a URI parsed by an ext/uri parser.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UriParts {
    pub scheme: Option<Vec<u8>>,
    pub host: Option<Vec<u8>>,
    pub has_path: bool,
    pub has_query: bool,
}

/// What the filter functions need from the engine.
pub trait FilterHost: Ctx {
    /// What the engine throws: an exception object, or whatever it uses to
    /// carry one while unwinding.
    type Exception;

    /// `(string)$obj` for an object being filtered: `Ok(None)` when the
    /// class has no `__toString()` (the filter then fails; tests
    /// ext/filter/tests/bug49274.phpt, bug73054.phpt), an exception from
    /// `__toString()` propagates.
    fn object_string(&mut self, obj: &ObjRef) -> Result<Option<PhpStr>, Self::Exception>;

    /// `is_callable($callback)` for FILTER_CALLBACK's option.
    fn is_callable(&mut self, callback: &Value) -> bool;

    /// Calls the FILTER_CALLBACK callable with one argument, by value
    /// (test ext/filter/tests/029.phpt: a by-reference parameter gets the
    /// engine's "must be passed by reference, value given" warning).
    fn call(&mut self, callback: &Value, arg: Value) -> Result<Value, Self::Exception>;

    /// Turns a `TypeError`/`ValueError` into the engine's exception.
    fn throw_error(&mut self, err: PhpError) -> Self::Exception;

    /// A `Filter\FilterFailedException` with this message (bytes: it
    /// quotes the filtered value as is).
    fn filter_failed(&mut self, message: PhpStr) -> Self::Exception;

    /// The request's input array for an `INPUT_*` source, `None` when the
    /// SAPI registered none (the CLI has no GET/POST/COOKIE arrays; tests
    /// ext/filter/tests/012.phpt, filter_input_array_001.phpt).
    fn input(&mut self, _ty: InputType) -> Option<PhpArray> {
        None
    }

    /// ini `default_charset` (FILTER_SANITIZE_FULL_SPECIAL_CHARS).
    fn default_charset(&self) -> Vec<u8> {
        b"UTF-8".to_vec()
    }

    /// ini `pcre.backtrack_limit`/`pcre.recursion_limit`/`pcre.jit` for
    /// FILTER_VALIDATE_REGEXP. Default: PHP's defaults with the JIT.
    fn pcre_limits(&self) -> goro_regex::Limits {
        goro_regex::Limits {
            match_limit: 1_000_000,
            depth_limit: 100_000,
            jit_stack: Some(goro_regex::Limits::PHP_JIT_STACK),
        }
    }

    /// Parses `uri` with an ext/uri parser for the `uri_parser_class`
    /// option: `Some(None)` when the parser rejects it, `None` when the
    /// engine has no such parser (the built-in parse_url()-based
    /// validation is used then).
    fn parse_uri(&mut self, _parser: UriParser, _uri: &[u8]) -> Option<Option<UriParts>> {
        None
    }
}

/// Which function is running, for messages ("filter_var(): ...") and the
/// `$options` argument number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Func {
    Var,
    VarArray,
    Input,
    InputArray,
}

impl Func {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Func::Var => "filter_var",
            Func::VarArray => "filter_var_array",
            Func::Input => "filter_input",
            Func::InputArray => "filter_input_array",
        }
    }

    /// The `$options` argument number.
    pub(crate) fn options_arg(self) -> u32 {
        match self {
            Func::Var => 3,
            Func::VarArray | Func::InputArray => 2,
            Func::Input => 4,
        }
    }
}

/// Bytes up to the first NUL, as PHP's C-string formatting of a value in a
/// message shows them (observed on PHP 8.5: the FILTER_THROW_ON_FAILURE
/// message for "a\0b" ends after "a").
pub(crate) fn c_str(b: &[u8]) -> &[u8] {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    &b[..end]
}
