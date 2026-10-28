//! The uri extension (PHP 8.5; RFC "Add RFC 3986 and WHATWG URL compliant
//! API"; PHP manual: URI): `Uri\Rfc3986\Uri`, `Uri\WhatWg\Url`, their
//! errors and enums. Parsing, normalization, resolution and the withers
//! are the goro-ext-uri crate's; this module wires them to PHP objects as
//! that crate's documentation describes (shapes from the ext/uri tests'
//! expected output and observations on PHP 8.5).

use std::rc::Rc;

use goro_ext_uri::ComparisonMode;
use goro_ext_uri::object::{serialize_array, unserialize_uri};
use goro_ext_uri::rfc3986::{self, Rfc3986Error};
use goro_ext_uri::whatwg::{self, UrlError, ValidationError};
use goro_runtime::{ObjRef, PhpArray, PhpStr, Value};

use crate::class::{Class, NativeHooks};
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "uri",
    classes: CLASSES,
    methods: METHODS,
    hooks: &[
        ("uri\\rfc3986\\uri", uri_hooks),
        ("uri\\whatwg\\url", url_hooks),
    ],
    ..Module::EMPTY
};

const CLASSES: &[(&str, &str)] = &[
    (
        "uri\\rfc3986\\uri",
        r#"<?php namespace Uri\Rfc3986;
use Uri\UriComparisonMode;
final readonly class Uri {
    public static function parse(string $uri, ?\Uri\Rfc3986\Uri $baseUrl = null): ?static {}
    public function __construct(string $uri, ?\Uri\Rfc3986\Uri $baseUrl = null) {}
    public function getScheme(): ?string {}
    public function getRawScheme(): ?string {}
    public function withScheme(?string $scheme): static {}
    public function getUserInfo(): ?string {}
    public function getRawUserInfo(): ?string {}
    public function withUserInfo(#[\SensitiveParameter] ?string $userinfo): static {}
    public function getUsername(): ?string {}
    public function getRawUsername(): ?string {}
    public function getPassword(): ?string {}
    public function getRawPassword(): ?string {}
    public function getHost(): ?string {}
    public function getRawHost(): ?string {}
    public function withHost(?string $host): static {}
    public function getPort(): ?int {}
    public function withPort(?int $port): static {}
    public function getPath(): string {}
    public function getRawPath(): string {}
    public function withPath(string $path): static {}
    public function getQuery(): ?string {}
    public function getRawQuery(): ?string {}
    public function withQuery(?string $query): static {}
    public function getFragment(): ?string {}
    public function getRawFragment(): ?string {}
    public function withFragment(?string $fragment): static {}
    public function equals(\Uri\Rfc3986\Uri $uri, \Uri\UriComparisonMode $comparisonMode = UriComparisonMode::ExcludeFragment): bool {}
    public function toString(): string {}
    public function toRawString(): string {}
    public function resolve(string $uri): static {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    public function __debugInfo(): array {}
}"#,
    ),
    (
        "uri\\whatwg\\url",
        r#"<?php namespace Uri\WhatWg;
use Uri\UriComparisonMode;
final readonly class Url {
    public static function parse(string $uri, ?\Uri\WhatWg\Url $baseUrl = null, &$errors = null): ?static {}
    public function __construct(string $uri, ?\Uri\WhatWg\Url $baseUrl = null, &$softErrors = null) {}
    public function getScheme(): string {}
    public function withScheme(string $scheme): static {}
    public function getUsername(): ?string {}
    public function withUsername(?string $username): static {}
    public function getPassword(): ?string {}
    public function withPassword(#[\SensitiveParameter] ?string $password): static {}
    public function getAsciiHost(): ?string {}
    public function getUnicodeHost(): ?string {}
    public function withHost(?string $host): static {}
    public function getPort(): ?int {}
    public function withPort(?int $port): static {}
    public function getPath(): string {}
    public function withPath(string $path): static {}
    public function getQuery(): ?string {}
    public function withQuery(?string $query): static {}
    public function getFragment(): ?string {}
    public function withFragment(?string $fragment): static {}
    public function equals(\Uri\WhatWg\Url $url, \Uri\UriComparisonMode $comparisonMode = UriComparisonMode::ExcludeFragment): bool {}
    public function toAsciiString(): string {}
    public function toUnicodeString(): string {}
    public function resolve(string $uri, &$softErrors = null): static {}
    public function __serialize(): array {}
    public function __unserialize(array $data): void {}
    public function __debugInfo(): array {}
}"#,
    ),
    (
        "uri\\uricomparisonmode",
        r#"<?php namespace Uri;
enum UriComparisonMode {
    case IncludeFragment;
    case ExcludeFragment;
}"#,
    ),
    (
        "uri\\uriexception",
        r#"<?php namespace Uri;
class UriException extends \Exception {}"#,
    ),
    (
        "uri\\urierror",
        r#"<?php namespace Uri;
class UriError extends \Error {}"#,
    ),
    (
        "uri\\invaliduriexception",
        r#"<?php namespace Uri;
class InvalidUriException extends \Uri\UriException {}"#,
    ),
    (
        "uri\\whatwg\\invalidurlexception",
        r#"<?php namespace Uri\WhatWg;
class InvalidUrlException extends \Uri\InvalidUriException {
    public protected(set) readonly array $errors;
    public function __construct(string $message = "", array $errors = [], int $code = 0, ?\Throwable $previous = null) {}
}"#,
    ),
    (
        "uri\\whatwg\\urlvalidationerror",
        r#"<?php namespace Uri\WhatWg;
final readonly class UrlValidationError {
    public protected(set) string $context;
    public protected(set) \Uri\WhatWg\UrlValidationErrorType $type;
    public protected(set) bool $failure;
    public function __construct(string $context, \Uri\WhatWg\UrlValidationErrorType $type, bool $failure) {
        $this->context = $context;
        $this->type = $type;
        $this->failure = $failure;
    }
}"#,
    ),
    (
        "uri\\whatwg\\urlvalidationerrortype",
        r#"<?php namespace Uri\WhatWg;
enum UrlValidationErrorType {
    case DomainToAscii;
    case DomainToUnicode;
    case DomainInvalidCodePoint;
    case HostInvalidCodePoint;
    case Ipv4EmptyPart;
    case Ipv4TooManyParts;
    case Ipv4NonNumericPart;
    case Ipv4NonDecimalPart;
    case Ipv4OutOfRangePart;
    case Ipv6Unclosed;
    case Ipv6InvalidCompression;
    case Ipv6TooManyPieces;
    case Ipv6MultipleCompression;
    case Ipv6InvalidCodePoint;
    case Ipv6TooFewPieces;
    case Ipv4InIpv6TooManyPieces;
    case Ipv4InIpv6InvalidCodePoint;
    case Ipv4InIpv6OutOfRangePart;
    case Ipv4InIpv6TooFewParts;
    case InvalidUrlUnit;
    case SpecialSchemeMissingFollowingSolidus;
    case MissingSchemeNonRelativeUrl;
    case InvalidReverseSoldius;
    case InvalidCredentials;
    case HostMissing;
    case PortOutOfRange;
    case PortInvalid;
    case FileInvalidWindowsDriveLetter;
    case FileInvalidWindowsDriveLetterHost;
}"#,
    ),
];

static METHODS: &[(&str, crate::func::NativeFn)] = &[
    ("uri\\rfc3986\\uri::parse", u_parse),
    ("uri\\rfc3986\\uri::__construct", u_construct),
    ("uri\\rfc3986\\uri::getscheme", u_get_scheme),
    ("uri\\rfc3986\\uri::getrawscheme", u_get_raw_scheme),
    ("uri\\rfc3986\\uri::withscheme", u_with_scheme),
    ("uri\\rfc3986\\uri::getuserinfo", u_get_userinfo),
    ("uri\\rfc3986\\uri::getrawuserinfo", u_get_raw_userinfo),
    ("uri\\rfc3986\\uri::withuserinfo", u_with_userinfo),
    ("uri\\rfc3986\\uri::getusername", u_get_username),
    ("uri\\rfc3986\\uri::getrawusername", u_get_raw_username),
    ("uri\\rfc3986\\uri::getpassword", u_get_password),
    ("uri\\rfc3986\\uri::getrawpassword", u_get_raw_password),
    ("uri\\rfc3986\\uri::gethost", u_get_host),
    ("uri\\rfc3986\\uri::getrawhost", u_get_raw_host),
    ("uri\\rfc3986\\uri::withhost", u_with_host),
    ("uri\\rfc3986\\uri::getport", u_get_port),
    ("uri\\rfc3986\\uri::withport", u_with_port),
    ("uri\\rfc3986\\uri::getpath", u_get_path),
    ("uri\\rfc3986\\uri::getrawpath", u_get_raw_path),
    ("uri\\rfc3986\\uri::withpath", u_with_path),
    ("uri\\rfc3986\\uri::getquery", u_get_query),
    ("uri\\rfc3986\\uri::getrawquery", u_get_raw_query),
    ("uri\\rfc3986\\uri::withquery", u_with_query),
    ("uri\\rfc3986\\uri::getfragment", u_get_fragment),
    ("uri\\rfc3986\\uri::getrawfragment", u_get_raw_fragment),
    ("uri\\rfc3986\\uri::withfragment", u_with_fragment),
    ("uri\\rfc3986\\uri::equals", u_equals),
    ("uri\\rfc3986\\uri::tostring", u_to_string),
    ("uri\\rfc3986\\uri::torawstring", u_to_raw_string),
    ("uri\\rfc3986\\uri::resolve", u_resolve),
    ("uri\\rfc3986\\uri::__serialize", u_serialize),
    ("uri\\rfc3986\\uri::__unserialize", u_unserialize),
    ("uri\\rfc3986\\uri::__debuginfo", u_debug_info),
    ("uri\\whatwg\\url::parse", w_parse),
    ("uri\\whatwg\\url::__construct", w_construct),
    ("uri\\whatwg\\url::getscheme", w_get_scheme),
    ("uri\\whatwg\\url::withscheme", w_with_scheme),
    ("uri\\whatwg\\url::getusername", w_get_username),
    ("uri\\whatwg\\url::withusername", w_with_username),
    ("uri\\whatwg\\url::getpassword", w_get_password),
    ("uri\\whatwg\\url::withpassword", w_with_password),
    ("uri\\whatwg\\url::getasciihost", w_get_ascii_host),
    ("uri\\whatwg\\url::getunicodehost", w_get_unicode_host),
    ("uri\\whatwg\\url::withhost", w_with_host),
    ("uri\\whatwg\\url::getport", w_get_port),
    ("uri\\whatwg\\url::withport", w_with_port),
    ("uri\\whatwg\\url::getpath", w_get_path),
    ("uri\\whatwg\\url::withpath", w_with_path),
    ("uri\\whatwg\\url::getquery", w_get_query),
    ("uri\\whatwg\\url::withquery", w_with_query),
    ("uri\\whatwg\\url::getfragment", w_get_fragment),
    ("uri\\whatwg\\url::withfragment", w_with_fragment),
    ("uri\\whatwg\\url::equals", w_equals),
    ("uri\\whatwg\\url::toasciistring", w_to_ascii_string),
    ("uri\\whatwg\\url::tounicodestring", w_to_unicode_string),
    ("uri\\whatwg\\url::resolve", w_resolve),
    ("uri\\whatwg\\url::__serialize", w_serialize),
    ("uri\\whatwg\\url::__unserialize", w_unserialize),
    ("uri\\whatwg\\url::__debuginfo", w_debug_info),
    ("uri\\whatwg\\invalidurlexception::__construct", e_construct),
];

const URI_CLASS: &str = "Uri\\Rfc3986\\Uri";
const URL_CLASS: &str = "Uri\\WhatWg\\Url";

// ---------------------------------------------------------------- helpers

fn peek<T: Clone + 'static>(o: &ObjRef) -> Option<T> {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<T>().cloned()
}

fn set<T: goro_runtime::NativeData + 'static>(o: &ObjRef, v: T) {
    *o.obj().native.borrow_mut() = Some(Box::new(v));
}

fn is_set<T: 'static>(o: &ObjRef) -> bool {
    let n = o.obj().native.borrow();
    n.as_ref().is_some_and(|b| {
        let any: &dyn std::any::Any = &**b;
        any.is::<T>()
    })
}

fn class(vm: &mut Vm, name: &str) -> R<Rc<Class>> {
    match vm.find_class(name.as_bytes(), false)? {
        Some(c) => Ok(c),
        None => Err(vm.fatal(&format!("Class \"{name}\" not found"))),
    }
}

/// The value of `$this`, or the "not correctly initialized" error for an
/// object made without its constructor (observed on PHP 8.5).
fn this<T: Clone + 'static>(vm: &mut Vm, a: &Args, cls: &str) -> R<T> {
    match peek::<T>(a.this_obj()) {
        Some(v) => Ok(v),
        None => vm.throw(
            "Error",
            &format!("{cls} object is not correctly initialized"),
        ),
    }
}

/// A new object of the class of `$this` holding `v` (the withers and
/// resolve() return `static`).
fn new_like<T: goro_runtime::NativeData + 'static>(vm: &mut Vm, o: &ObjRef, v: T) -> R<Value> {
    let cls = vm.class_of(o);
    let n = vm.instantiate_raw(&cls)?;
    set(&n, v);
    Ok(Value::Object(n))
}

/// The class a static method was called on.
fn static_class(vm: &Vm, a: &Args) -> Rc<Class> {
    let p = if a.static_class.is_null() {
        a.func.scope.get()
    } else {
        a.static_class
    };
    vm.rc_class(p)
}

/// Assigns to the by-reference argument `i` (through the type of a typed
/// property it is bound to: test 057.phpt).
fn set_out(vm: &mut Vm, a: &Args, i: usize, v: Value) -> R<()> {
    if let Some(r) = a.ref_at(i) {
        let v = vm.typed_ref_value(&r, v)?;
        drop(r.set(v));
    }
    Ok(())
}

fn opt_bytes(v: Option<&[u8]>) -> Value {
    v.map_or(Value::Null, Value::from)
}

fn opt_cow(v: Option<std::borrow::Cow<'_, [u8]>>) -> Value {
    v.map_or(Value::Null, |c| Value::from(&*c))
}

/// The `Uri\UriComparisonMode` argument `i` (the default when absent).
fn comparison_mode(vm: &Vm, a: &Args, i: usize) -> ComparisonMode {
    match a.obj(i) {
        Some(o) => match vm.get_prop_raw(&o, b"name") {
            Value::Str(s) if s.as_bytes() == b"IncludeFragment" => ComparisonMode::IncludeFragment,
            _ => ComparisonMode::ExcludeFragment,
        },
        None => ComparisonMode::ExcludeFragment,
    }
}

/// "Cannot modify readonly object of class X": `__construct()` on a
/// constructed object (test 065.phpt).
fn readonly_error<T>(vm: &mut Vm, cls: &str) -> R<T> {
    vm.throw(
        "Error",
        &format!("Cannot modify readonly object of class {cls}"),
    )
}

/// `Exception("Invalid serialization data for X object")` (test 065.phpt).
fn bad_data<T>(vm: &mut Vm, cls: &str) -> R<T> {
    vm.throw(
        "Exception",
        &format!("Invalid serialization data for {cls} object"),
    )
}

fn uri_hooks() -> NativeHooks {
    NativeHooks {
        clone: Some(|_, from, to| {
            if let Some(u) = peek::<rfc3986::Uri>(from) {
                set(to, u);
            }
        }),
        debug_props: Some(|_, o| {
            Some(peek::<rfc3986::Uri>(o).map_or_else(PhpArray::new, |u| u.debug_array()))
        }),
        props_for: Some(|_, _| Some(PhpArray::new())),
        compare: Some(|_, x, y| {
            let (Value::Object(a), Value::Object(b)) = (x, y) else {
                return None;
            };
            // no properties: two objects of the class are == (observed on
            // PHP 8.5)
            (is_set::<rfc3986::Uri>(a) && is_set::<rfc3986::Uri>(b)).then_some(Ok(0))
        }),
        ..Default::default()
    }
}

fn url_hooks() -> NativeHooks {
    NativeHooks {
        clone: Some(|_, from, to| {
            if let Some(u) = peek::<whatwg::Url>(from) {
                set(to, u);
            }
        }),
        debug_props: Some(|_, o| {
            Some(peek::<whatwg::Url>(o).map_or_else(PhpArray::new, |u| u.debug_array()))
        }),
        props_for: Some(|_, _| Some(PhpArray::new())),
        compare: Some(|_, x, y| {
            let (Value::Object(a), Value::Object(b)) = (x, y) else {
                return None;
            };
            (is_set::<whatwg::Url>(a) && is_set::<whatwg::Url>(b)).then_some(Ok(0))
        }),
        ..Default::default()
    }
}

// ---------------------------------------------------------------- Uri\Rfc3986\Uri

fn rfc_throw<T>(vm: &mut Vm, e: Rfc3986Error) -> R<T> {
    vm.throw("Uri\\InvalidUriException", e.message())
}

fn base_uri(vm: &mut Vm, a: &Args, i: usize) -> R<Option<rfc3986::Uri>> {
    match a.obj(i) {
        Some(o) => match peek::<rfc3986::Uri>(&o) {
            Some(u) => Ok(Some(u)),
            None => vm.throw(
                "Error",
                &format!("{URI_CLASS} object is not correctly initialized"),
            ),
        },
        None => Ok(None),
    }
}

fn u_parse(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let base = base_uri(vm, a, 1)?;
    match rfc3986::Uri::parse_with_base(a.bytes(0), base.as_ref()) {
        Ok(u) => {
            let cls = static_class(vm, a);
            let o = vm.instantiate_raw(&cls)?;
            set(&o, u);
            Ok(Value::Object(o))
        }
        Err(_) => Ok(Value::Null),
    }
}

fn u_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if is_set::<rfc3986::Uri>(&o) {
        return readonly_error(vm, URI_CLASS);
    }
    let base = base_uri(vm, a, 1)?;
    match rfc3986::Uri::parse_with_base(a.bytes(0), base.as_ref()) {
        Ok(u) => {
            set(&o, u);
            Ok(Value::Null)
        }
        Err(e) => rfc_throw(vm, e),
    }
}

macro_rules! uri_getter {
    ($name:ident, |$u:ident| $body:expr) => {
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            let $u: rfc3986::Uri = this(vm, a, URI_CLASS)?;
            Ok($body)
        }
    };
}

uri_getter!(u_get_scheme, |u| opt_cow(u.scheme()));
uri_getter!(u_get_raw_scheme, |u| opt_bytes(u.raw_scheme()));
uri_getter!(u_get_userinfo, |u| opt_cow(u.userinfo()));
uri_getter!(u_get_raw_userinfo, |u| opt_bytes(u.raw_userinfo()));
uri_getter!(u_get_username, |u| opt_cow(u.username()));
uri_getter!(u_get_raw_username, |u| opt_bytes(u.raw_username()));
uri_getter!(u_get_password, |u| opt_cow(u.password()));
uri_getter!(u_get_raw_password, |u| opt_bytes(u.raw_password()));
uri_getter!(u_get_host, |u| opt_cow(u.host()));
uri_getter!(u_get_raw_host, |u| opt_bytes(u.raw_host()));
uri_getter!(u_get_port, |u| u.port().map_or(Value::Null, Value::Int));
uri_getter!(u_get_path, |u| Value::from(&*u.path()));
uri_getter!(u_get_raw_path, |u| Value::from(u.raw_path()));
uri_getter!(u_get_query, |u| opt_cow(u.query()));
uri_getter!(u_get_raw_query, |u| opt_bytes(u.raw_query()));
uri_getter!(u_get_fragment, |u| opt_cow(u.fragment()));
uri_getter!(u_get_raw_fragment, |u| opt_bytes(u.raw_fragment()));
uri_getter!(u_to_string, |u| Value::from(
    u.to_normalized_string().as_slice()
));
uri_getter!(u_to_raw_string, |u| Value::from(
    u.to_raw_string().as_slice()
));
uri_getter!(u_debug_info, |u| Value::Array(u.debug_array()));
uri_getter!(u_serialize, |u| Value::Array(serialize_array(
    &u.serialized_uri()
)));

fn uri_result(vm: &mut Vm, a: &Args, r: Result<rfc3986::Uri, Rfc3986Error>) -> R<Value> {
    match r {
        Ok(u) => new_like(vm, a.this_obj(), u),
        Err(e) => rfc_throw(vm, e),
    }
}

macro_rules! uri_wither {
    ($name:ident, |$u:ident, $a:ident| $body:expr) => {
        fn $name(vm: &mut Vm, $a: &mut Args) -> R<Value> {
            let $u: rfc3986::Uri = this(vm, $a, URI_CLASS)?;
            let r = $body;
            uri_result(vm, $a, r)
        }
    };
}

uri_wither!(u_with_scheme, |u, a| u.with_scheme(a.opt_str(0).as_deref()));
uri_wither!(u_with_userinfo, |u, a| u
    .with_userinfo(a.opt_str(0).as_deref()));
uri_wither!(u_with_host, |u, a| u.with_host(a.opt_str(0).as_deref()));
uri_wither!(u_with_port, |u, a| u.with_port(a.opt_int(0)));
uri_wither!(u_with_path, |u, a| u.with_path(a.bytes(0)));
uri_wither!(u_with_query, |u, a| u.with_query(a.opt_str(0).as_deref()));
uri_wither!(u_with_fragment, |u, a| u
    .with_fragment(a.opt_str(0).as_deref()));
uri_wither!(u_resolve, |u, a| u.resolve(a.bytes(0)));

fn u_equals(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let u: rfc3986::Uri = this(vm, a, URI_CLASS)?;
    let Some(other) = base_uri(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    let mode = comparison_mode(vm, a, 1);
    Ok(Value::Bool(u.equals(&other, mode)))
}

fn u_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if is_set::<rfc3986::Uri>(&o) {
        return bad_data(vm, URI_CLASS);
    }
    let Some(s) = unserialize_uri(&a.array(0)) else {
        return bad_data(vm, URI_CLASS);
    };
    match rfc3986::Uri::parse(s.as_bytes()) {
        Ok(u) => {
            set(&o, u);
            Ok(Value::Null)
        }
        Err(_) => bad_data(vm, URI_CLASS),
    }
}

// ---------------------------------------------------------------- Uri\WhatWg\Url

/// The `Uri\WhatWg\UrlValidationError` objects of `errs`.
fn error_list(vm: &mut Vm, errs: &[ValidationError]) -> R<Value> {
    let mut out = PhpArray::new();
    if errs.is_empty() {
        return Ok(Value::Array(out));
    }
    let cls = class(vm, "Uri\\WhatWg\\UrlValidationError")?;
    let ty = class(vm, "Uri\\WhatWg\\UrlValidationErrorType")?;
    for e in errs {
        let o = vm.instantiate_raw(&cls)?;
        let case = vm.enum_case(&ty, &PhpStr::from(e.kind.name()))?;
        vm.set_prop_raw(&o, b"context", Value::from(e.context.as_slice()));
        vm.set_prop_raw(&o, b"type", case);
        vm.set_prop_raw(&o, b"failure", Value::Bool(e.failure()));
        let _ = out.push(Value::Object(o));
    }
    Ok(Value::Array(out))
}

/// Throws `Uri\WhatWg\InvalidUrlException` for `e`.
fn url_throw<T>(vm: &mut Vm, e: &UrlError) -> R<T> {
    let errors = error_list(vm, &e.errors)?;
    let ex = vm.create_exception("Uri\\WhatWg\\InvalidUrlException", &e.message(), 0)?;
    vm.set_prop_raw(&ex, b"errors", errors);
    Err(crate::vm::Unwind::Throw(ex))
}

fn base_url(vm: &mut Vm, a: &Args, i: usize) -> R<Option<whatwg::Url>> {
    match a.obj(i) {
        Some(o) => match peek::<whatwg::Url>(&o) {
            Some(u) => Ok(Some(u)),
            None => vm.throw(
                "Error",
                &format!("{URL_CLASS} object is not correctly initialized"),
            ),
        },
        None => Ok(None),
    }
}

fn w_parse(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let base = base_url(vm, a, 1)?;
    match whatwg::Url::parse(a.bytes(0), base.as_ref()) {
        Ok((u, errs)) => {
            let cls = static_class(vm, a);
            let o = vm.instantiate_raw(&cls)?;
            set(&o, u);
            let list = error_list(vm, &errs)?;
            set_out(vm, a, 2, list)?;
            Ok(Value::Object(o))
        }
        Err(e) => {
            let list = error_list(vm, &e.errors)?;
            set_out(vm, a, 2, list)?;
            Ok(Value::Null)
        }
    }
}

fn w_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if is_set::<whatwg::Url>(&o) {
        return readonly_error(vm, URL_CLASS);
    }
    let base = base_url(vm, a, 1)?;
    match whatwg::Url::parse(a.bytes(0), base.as_ref()) {
        Ok((u, errs)) => {
            set(&o, u);
            let list = error_list(vm, &errs)?;
            set_out(vm, a, 2, list)?;
            Ok(Value::Null)
        }
        Err(e) => url_throw(vm, &e),
    }
}

macro_rules! url_getter {
    ($name:ident, |$u:ident| $body:expr) => {
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            let $u: whatwg::Url = this(vm, a, URL_CLASS)?;
            Ok($body)
        }
    };
}

url_getter!(w_get_scheme, |u| Value::from(u.scheme()));
url_getter!(w_get_username, |u| opt_bytes(u.username()));
url_getter!(w_get_password, |u| opt_bytes(u.password()));
url_getter!(w_get_ascii_host, |u| opt_bytes(u.ascii_host()));
url_getter!(w_get_unicode_host, |u| u
    .unicode_host()
    .map_or(Value::Null, |h| Value::from(h.as_slice())));
url_getter!(w_get_port, |u| u
    .port()
    .map_or(Value::Null, |p| Value::Int(p as i64)));
url_getter!(w_get_path, |u| Value::from(u.path()));
url_getter!(w_get_query, |u| opt_bytes(u.query()));
url_getter!(w_get_fragment, |u| opt_bytes(u.fragment()));
url_getter!(w_to_ascii_string, |u| Value::from(u.to_ascii_string()));
url_getter!(w_to_unicode_string, |u| Value::from(
    u.to_unicode_string().as_slice()
));
url_getter!(w_debug_info, |u| Value::Array(u.debug_array()));
url_getter!(w_serialize, |u| Value::Array(serialize_array(
    u.serialized_uri()
)));

macro_rules! url_wither {
    ($name:ident, |$u:ident, $a:ident| $body:expr) => {
        fn $name(vm: &mut Vm, $a: &mut Args) -> R<Value> {
            let $u: whatwg::Url = this(vm, $a, URL_CLASS)?;
            match $body {
                Ok(n) => new_like(vm, $a.this_obj(), n),
                Err(e) => url_throw(vm, &e),
            }
        }
    };
}

url_wither!(w_with_scheme, |u, a| u.with_scheme(a.bytes(0)));
url_wither!(w_with_username, |u, a| u
    .with_username(a.opt_str(0).as_deref()));
url_wither!(w_with_password, |u, a| u
    .with_password(a.opt_str(0).as_deref()));
url_wither!(w_with_host, |u, a| u.with_host(a.opt_str(0).as_deref()));
url_wither!(w_with_port, |u, a| u.with_port(a.opt_int(0)));
url_wither!(w_with_path, |u, a| u.with_path(a.bytes(0)));
url_wither!(w_with_query, |u, a| u.with_query(a.opt_str(0).as_deref()));
url_wither!(w_with_fragment, |u, a| u
    .with_fragment(a.opt_str(0).as_deref()));

fn w_resolve(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let u: whatwg::Url = this(vm, a, URL_CLASS)?;
    match u.resolve(a.bytes(0)) {
        Ok((n, errs)) => {
            let list = error_list(vm, &errs)?;
            set_out(vm, a, 1, list)?;
            new_like(vm, a.this_obj(), n)
        }
        Err(e) => url_throw(vm, &e),
    }
}

fn w_equals(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let u: whatwg::Url = this(vm, a, URL_CLASS)?;
    let Some(other) = base_url(vm, a, 0)? else {
        return Ok(Value::Bool(false));
    };
    let mode = comparison_mode(vm, a, 1);
    Ok(Value::Bool(u.equals(&other, mode)))
}

fn w_unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if is_set::<whatwg::Url>(&o) {
        return bad_data(vm, URL_CLASS);
    }
    let Some(s) = unserialize_uri(&a.array(0)) else {
        return bad_data(vm, URL_CLASS);
    };
    match whatwg::Url::parse(s.as_bytes(), None) {
        Ok((u, _)) => {
            set(&o, u);
            Ok(Value::Null)
        }
        Err(_) => bad_data(vm, URL_CLASS),
    }
}

// ---------------------------------------------------------------- InvalidUrlException

/// `Uri\WhatWg\InvalidUrlException::__construct()`: the Exception
/// constructor's work, then the readonly `$errors`, which must be a list of
/// `Uri\WhatWg\UrlValidationError` (tests gh19780.phpt, 053.phpt: a second
/// call updates the message and a non-zero code or non-null previous, then
/// fails on `$errors`).
fn e_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let errors = a.array(1);
    let ve = class(vm, "Uri\\WhatWg\\UrlValidationError")?;
    let ok = errors.iter().enumerate().all(|(i, (k, v))| {
        k == goro_runtime::KeyRef::Int(i as i64)
            && matches!(v.deref(), Value::Object(e) if vm.class_of(&e).is_subclass_of(&ve))
    });
    if !ok {
        return vm.throw(
            "ValueError",
            "Uri\\WhatWg\\InvalidUrlException::__construct(): Argument #2 ($errors) must be a list of Uri\\WhatWg\\UrlValidationError",
        );
    }
    vm.set_prop_raw(&o, b"message", Value::Str(a.str(0)));
    let code = a.int_or(2, 0);
    if code != 0 {
        vm.set_prop_raw(&o, b"code", Value::Int(code));
    }
    if let Some(p) = a.obj(3) {
        vm.set_prop_raw(&o, b"previous", Value::Object(p));
    }
    if matches!(vm.get_prop_raw(&o, b"errors"), Value::Array(_)) {
        return vm.throw(
            "Error",
            "Cannot modify readonly property Uri\\WhatWg\\InvalidUrlException::$errors",
        );
    }
    vm.set_prop_raw(&o, b"errors", Value::Array(errors));
    Ok(Value::Null)
}
