//! PHP's uri extension (PHP 8.5; RFC "Add RFC 3986 and WHATWG URL compliant
//! API"; RFC 3986; WHATWG URL Standard): `Uri\Rfc3986\Uri`,
//! `Uri\WhatWg\Url` and their errors, as a library for the engine.
//!
//! Behaviour follows the PHP manual, the RFC, RFC 3986, the URL Standard,
//! UTS #46, the expected output of php-src's `ext/uri/tests/*.phpt`, and
//! observations of PHP 8.5.4 recorded as fixtures (`tests/fixtures/`, made by
//! the drivers next to them). Where PHP 8.5 differs from the current URL
//! Standard (it implements an earlier state of it), goro follows PHP; see
//! [`whatwg`] and `tests/wpt.rs` for the list.
//!
//! # Engine wiring
//!
//! The extension is `uri` (`ReflectionExtension::getVersion()` is the PHP
//! version; `getDependencies()` lists `lexbor => Required`; no ini entries
//! or constants). Its classes, as reflection shows them on PHP 8.5:
//!
//! * `final readonly class Uri\Rfc3986\Uri` (no properties): native data
//!   [`rfc3986::Uri`]. Methods, with the library call:
//!   - `static parse(string $uri, ?Uri\Rfc3986\Uri $baseUrl = null): ?static`
//!     → [`rfc3986::Uri::parse_with_base`], null on any error;
//!   - `__construct(string $uri, ?Uri\Rfc3986\Uri $baseUrl = null)` → the
//!     same, throwing `Uri\InvalidUriException` with
//!     [`rfc3986::Rfc3986Error::message`];
//!   - `getScheme/getRawScheme(): ?string`, `getUserInfo/getRawUserInfo`,
//!     `getUsername/getRawUsername`, `getPassword/getRawPassword`,
//!     `getHost/getRawHost` (all `?string`), `getPort(): ?int`,
//!     `getPath/getRawPath(): string`, `getQuery/getRawQuery`,
//!     `getFragment/getRawFragment` (`?string`) → the getters of the same
//!     name (the normalized ones without `raw_`);
//!   - `withScheme(?string $scheme)`, `withUserInfo(?string $userinfo)`,
//!     `withHost(?string $host)`, `withPort(?int $port)`,
//!     `withPath(string $path)`, `withQuery(?string $query)`,
//!     `withFragment(?string $fragment)`: `static` → `with_*`, throwing
//!     `Uri\InvalidUriException`;
//!   - `equals(Uri\Rfc3986\Uri $uri, Uri\UriComparisonMode $comparisonMode
//!     = Uri\UriComparisonMode::ExcludeFragment): bool`,
//!     `toString(): string` ([`rfc3986::Uri::to_normalized_string`]),
//!     `toRawString(): string`, `resolve(string $uri): static`;
//!   - `__serialize(): array`, `__unserialize(array $data): void`,
//!     `__debugInfo(): array` (see below).
//! * `final readonly class Uri\WhatWg\Url` (no properties): native data
//!   [`whatwg::Url`]. Methods:
//!   - `static parse(string $uri, ?Uri\WhatWg\Url $baseUrl = null, &$errors
//!     = null): ?static` → [`whatwg::Url::parse`]; `$errors` is always set
//!     to the list of `Uri\WhatWg\UrlValidationError` objects (an empty
//!     array when there are none; test gh19892.phpt), from
//!     [`whatwg::ValidationError`] in the order given;
//!   - `__construct(string $uri, ?Uri\WhatWg\Url $baseUrl = null,
//!     &$softErrors = null)`: on failure throws
//!     `Uri\WhatWg\InvalidUrlException` with [`whatwg::UrlError::message`]
//!     and `$errors` = [`whatwg::UrlError::errors`] (and leaves
//!     `$softErrors` alone); on success sets `$softErrors`;
//!   - `getScheme(): string`, `getUsername/getPassword(): ?string`,
//!     `getAsciiHost/getUnicodeHost(): ?string`, `getPort(): ?int`,
//!     `getPath(): string`, `getQuery/getFragment(): ?string`;
//!   - `withScheme(string $scheme)`, `withUsername(?string)`,
//!     `withPassword(?string)`, `withHost(?string)`, `withPort(?int)`,
//!     `withPath(string)`, `withQuery(?string)`, `withFragment(?string)`:
//!     `static`, throwing `Uri\WhatWg\InvalidUrlException`;
//!   - `equals(Uri\WhatWg\Url $url, Uri\UriComparisonMode $comparisonMode =
//!     ExcludeFragment): bool`, `toAsciiString(): string`,
//!     `toUnicodeString(): string`, `resolve(string $uri, &$softErrors =
//!     null): static`;
//!   - `__serialize`, `__unserialize`, `__debugInfo` as for the other class.
//!   - a `clone` of a `Url` must clone the [`whatwg::Url`] (cheap; it
//!     shares the validation-error list as PHP does, see [`whatwg::Url`]).
//! * `final readonly class Uri\WhatWg\UrlValidationError` with
//!   `public protected(set) readonly string $context`,
//!   `public protected(set) readonly Uri\WhatWg\UrlValidationErrorType $type`
//!   and `public protected(set) readonly bool $failure`, and
//!   `__construct(string $context, Uri\WhatWg\UrlValidationErrorType $type,
//!   bool $failure)` (calling it again: `Error: Cannot modify readonly
//!   property Uri\WhatWg\UrlValidationError::$context`, test 052.phpt).
//! * `enum Uri\WhatWg\UrlValidationErrorType` (pure, 29 cases in the order
//!   of [`whatwg::ErrorType::ALL`]) and `enum Uri\UriComparisonMode`
//!   (`IncludeFragment`, `ExcludeFragment`; [`ComparisonMode`]).
//! * `class Uri\UriException extends Exception`, `class Uri\UriError
//!   extends Error`, `class Uri\InvalidUriException extends
//!   Uri\UriException`, and `class Uri\WhatWg\InvalidUrlException extends
//!   Uri\InvalidUriException` with `public protected(set) readonly array
//!   $errors` and `__construct(string $message = "", array $errors = [],
//!   int $code = 0, ?Throwable $previous = null)`. `$errors` must be a list
//!   of `UrlValidationError` objects, else `ValueError:
//!   Uri\WhatWg\InvalidUrlException::__construct(): Argument #2 ($errors)
//!   must be a list of Uri\WhatWg\UrlValidationError` (gh19780.phpt);
//!   calling the constructor again throws `Error: Cannot modify readonly
//!   property Uri\WhatWg\InvalidUrlException::$errors` (053.phpt).
//!
//! Object behaviour the engine provides (tests 015, 021, 022, 032-034, 065
//! .phpt; observed on PHP 8.5):
//!
//! * both URI classes are final: extending them is the compile error
//!   "Class X cannot extend final class Uri\Rfc3986\Uri";
//!   `ReflectionClass::newInstanceWithoutConstructor()` throws
//!   `ReflectionException: Class Uri\Rfc3986\Uri is an internal class marked
//!   as final that cannot be instantiated without invoking its constructor`;
//! * calling `__construct()` on a constructed object throws `Error: Cannot
//!   modify readonly object of class Uri\Rfc3986\Uri` (resp. `Url`);
//! * `__serialize()` is [`object::serialize_array`] of
//!   [`rfc3986::Uri::serialized_uri`] / [`whatwg::Url::serialized_uri`];
//!   `__unserialize()` takes [`object::unserialize_uri`] and parses it
//!   without a base; bad data, a string that fails to parse, or an object
//!   that is already initialized throw `Exception: Invalid serialization
//!   data for Uri\Rfc3986\Uri object` (resp. `Uri\WhatWg\Url`);
//! * `var_dump()` and `print_r()` show the `__debugInfo()` array
//!   (`debug_array()`); `var_export()` writes `\Uri\Rfc3986\Uri::__set_state(
//!   array(\n))`, `(array)` and `get_object_vars()` give `[]`,
//!   `json_encode()` gives `{}`, and `==` between two objects of one of
//!   these classes is true (no properties to compare).
//!
//! The `uri_parser_class` option of FILTER_VALIDATE_URL
//! (`goro_ext_filter::FilterHost::parse_uri`) is served by
//! [`filter_parts`]:
//!
//! ```ignore
//! fn parse_uri(&mut self, p: UriParser, uri: &[u8]) -> Option<Option<UriParts>> {
//!     let parser = match p {
//!         UriParser::Rfc3986 => goro_ext_uri::FilterParser::Rfc3986,
//!         UriParser::WhatWg => goro_ext_uri::FilterParser::WhatWg,
//!     };
//!     Some(goro_ext_uri::filter_parts(parser, uri).map(|v| UriParts {
//!         scheme: v.scheme,
//!         host: v.host,
//!         has_path: v.has_path,
//!         has_query: v.has_query,
//!     }))
//! }
//! ```

pub mod object;
pub mod rfc3986;
pub mod whatwg;

pub use rfc3986::ComparisonMode;

/// The extension's classes, interfaces and enums (for registration).
pub const CLASS_NAMES: &[&str] = &[
    "Uri\\Rfc3986\\Uri",
    "Uri\\WhatWg\\Url",
    "Uri\\UriComparisonMode",
    "Uri\\UriException",
    "Uri\\UriError",
    "Uri\\InvalidUriException",
    "Uri\\WhatWg\\InvalidUrlException",
    "Uri\\WhatWg\\UrlValidationError",
    "Uri\\WhatWg\\UrlValidationErrorType",
];

/// Which ext/uri parser FILTER_VALIDATE_URL's `uri_parser_class` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterParser {
    /// `Uri\Rfc3986\Uri`
    Rfc3986,
    /// `Uri\WhatWg\Url`
    WhatWg,
}

/// What FILTER_VALIDATE_URL needs from a parsed URI.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterParts {
    pub scheme: Option<Vec<u8>>,
    pub host: Option<Vec<u8>>,
    /// Always true: with either parser, FILTER_FLAG_PATH_REQUIRED accepts
    /// `http://h` (observed on PHP 8.5).
    pub has_path: bool,
    /// The query is not null (`http://h?` passes FILTER_FLAG_QUERY_REQUIRED;
    /// observed on PHP 8.5).
    pub has_query: bool,
}

/// Parses `uri` for FILTER_VALIDATE_URL's `uri_parser_class` option:
/// `None` when the parser rejects it. The components are the raw ones
/// (RFC 3986) or the WHATWG URL's (ASCII host).
pub fn filter_parts(parser: FilterParser, uri: &[u8]) -> Option<FilterParts> {
    match parser {
        FilterParser::Rfc3986 => {
            let u = rfc3986::Uri::parse(uri).ok()?;
            Some(FilterParts {
                scheme: u.raw_scheme().map(<[u8]>::to_vec),
                host: u.raw_host().map(<[u8]>::to_vec),
                has_path: true,
                has_query: u.raw_query().is_some(),
            })
        }
        FilterParser::WhatWg => {
            let (u, _) = whatwg::Url::parse(uri, None).ok()?;
            Some(FilterParts {
                scheme: Some(u.scheme().to_vec()),
                host: u.ascii_host().map(<[u8]>::to_vec),
                has_path: true,
                has_query: u.query().is_some(),
            })
        }
    }
}
