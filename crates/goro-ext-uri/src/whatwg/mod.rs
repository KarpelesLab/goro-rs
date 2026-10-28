//! `Uri\WhatWg\Url`: the WHATWG URL Standard parser with PHP 8.5's
//! behaviour (see [`parser`] for where PHP departs from the current
//! standard), its validation errors, getters, withers and serializations.

pub mod errors;
mod fast;
mod host;
mod idna;
mod idna_table;
mod parser;

pub use errors::{ErrorType, ValidationError};
pub use host::HostKind;

use std::cell::RefCell;
use std::rc::Rc;

use errors::Errors;
use host::Host;
use parser::{Parser, Record, Scheme, State};

use crate::object::DebugValue;
use crate::rfc3986::ComparisonMode;

/// Which part an error is about (the start of the exception message).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Component {
    Uri,
    Scheme,
    Username,
    Password,
    Host,
    Port,
    Path,
    Query,
    Fragment,
}

/// A failed parse or modification: thrown as
/// `Uri\WhatWg\InvalidUrlException` with [`UrlError::message`] and
/// `$errors` = [`UrlError::errors`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UrlError {
    pub component: Component,
    /// The validation errors in PHP's order (the last one met first).
    pub errors: Vec<ValidationError>,
    /// How many errors at the end of [`UrlError::errors`] a failing wither
    /// took over from the URL's earlier operations. PHP reads their
    /// contexts from memory it has freed (so it shows garbage once the
    /// memory is reused); goro gives the contexts they had.
    pub inherited: usize,
    /// The first failure-type error met, which names the message.
    first_failure: Option<ErrorType>,
}

impl UrlError {
    fn new(component: Component, errs: Errors) -> UrlError {
        UrlError::with_inherited(component, errs, 0)
    }

    fn with_inherited(component: Component, errs: Errors, inherited: usize) -> UrlError {
        let first_failure = errs.first_failure();
        let mut errors = errs.list;
        errors.reverse();
        UrlError {
            component,
            errors,
            inherited,
            first_failure,
        }
    }

    /// "The specified URI is malformed (PortInvalid)": the type is the
    /// first failure-type error the parser met, if any (observed on
    /// PHP 8.5; tests 007.phpt, 026.phpt, WPT `http://2001::1`).
    pub fn message(&self) -> String {
        let what = match self.component {
            Component::Uri => "URI",
            Component::Scheme => "scheme",
            Component::Username => "username",
            Component::Password => "password",
            Component::Host => "host",
            Component::Port => "port",
            Component::Path => "path",
            Component::Query => "query",
            Component::Fragment => "fragment",
        };
        match self.first_failure {
            Some(t) => format!("The specified {what} is malformed ({})", t.name()),
            None => format!("The specified {what} is malformed"),
        }
    }
}

impl std::fmt::Display for UrlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Span {
    start: u32,
    end: u32,
}

impl Span {
    fn get(self, b: &[u8]) -> &[u8] {
        &b[self.start as usize..self.end as usize]
    }
}

/// A `Uri\WhatWg\Url` value: the ASCII serialization with the component
/// offsets.
#[derive(Clone, Debug)]
pub struct Url {
    href: Box<[u8]>,
    scheme: Span,
    username: Span,
    password: Span,
    host: Option<Span>,
    host_kind: HostKind,
    port: Option<u16>,
    path: Span,
    query: Option<Span>,
    fragment: Option<Span>,
    opaque: bool,
    kind: Scheme,
    /// The validation errors of the parse that made this URL and of the
    /// successful withers since, in order, shared with every clone and
    /// wither result; a failing wither reports and empties it (observed on
    /// PHP 8.5: after `$v = $u->withPath(..)`, a failing `$v->withScheme()`
    /// reports `$u`'s parse errors and a failing `$u->withScheme()` then
    /// reports none).
    errors: Rc<RefCell<Vec<ValidationError>>>,
}

impl goro_runtime::NativeData for Url {}

/// A successful parse: the URL and its (soft) validation errors in PHP's
/// order.
pub type Parsed = (Url, Vec<ValidationError>);

impl Url {
    /// `Uri\WhatWg\Url::parse($uri, $baseUrl, $errors)` and the
    /// constructor: the URL with its validation errors, or the error to
    /// throw (`parse()` returns null and sets `$errors` to
    /// [`UrlError::errors`]).
    pub fn parse(input: &[u8], base: Option<&Url>) -> Result<Parsed, UrlError> {
        if let Some(r) = base.is_none().then(|| fast::parse(input)).flatten() {
            return Ok(r);
        }
        Url::parse_full(input, base)
    }

    /// [`Url::parse`] without the fast path.
    #[doc(hidden)]
    pub fn parse_full(input: &[u8], base: Option<&Url>) -> Result<Parsed, UrlError> {
        thread_local! {
            // A record kept between parses for its buffers.
            static SCRATCH: RefCell<Record> = RefCell::new(Record::new());
        }
        let base_rec = base.map(Url::to_record);
        let mut rec = SCRATCH
            .try_with(|r| {
                r.try_borrow_mut()
                    .map(|mut r| std::mem::replace(&mut *r, Record::new()))
                    .ok()
            })
            .ok()
            .flatten()
            .unwrap_or_else(Record::new);
        rec.reset();
        let res = Url::parse_into(input, base_rec.as_ref(), &mut rec);
        let _ = SCRATCH.try_with(|r| r.try_borrow_mut().map(|mut r| *r = rec));
        res
    }

    fn parse_into(
        input: &[u8],
        base_rec: Option<&Record>,
        rec: &mut Record,
    ) -> Result<Parsed, UrlError> {
        let mut errs = Errors::default();
        match parse_record(input, base_rec, rec, &mut errs) {
            Ok(()) => {
                if errs.ipv4.is_some() {
                    let mut strings: Vec<&[u8]> = vec![&rec.path];
                    strings.extend(rec.query.as_deref());
                    strings.extend(rec.fragment.as_deref());
                    errs.fix_ipv4(&strings);
                }
                let mut url = Url::from_record(rec);
                url.errors = Rc::new(RefCell::new(errs.list.clone()));
                let mut list = errs.list;
                list.reverse();
                Ok((url, list))
            }
            Err(()) => Err(UrlError::new(Component::Uri, errs)),
        }
    }

    /// `resolve($uri, $softErrors)`: `$uri` parsed against this URL.
    pub fn resolve(&self, input: &[u8]) -> Result<Parsed, UrlError> {
        Url::parse(input, Some(self))
    }

    // ---- getters ----

    pub fn scheme(&self) -> &[u8] {
        self.scheme.get(&self.href)
    }

    fn has_credentials(&self) -> bool {
        self.username.start != self.username.end || self.password.start != self.password.end
    }

    /// `getUsername()`: null when the URL has neither a username nor a
    /// password (tests whatwg/modification/username_*.phpt).
    pub fn username(&self) -> Option<&[u8]> {
        self.has_credentials()
            .then(|| self.username.get(&self.href))
    }

    /// `getPassword()`: null when the URL has neither a username nor a
    /// password.
    pub fn password(&self) -> Option<&[u8]> {
        self.has_credentials()
            .then(|| self.password.get(&self.href))
    }

    /// `getAsciiHost()`.
    pub fn ascii_host(&self) -> Option<&[u8]> {
        self.host.map(|h| h.get(&self.href))
    }

    pub fn host_kind(&self) -> Option<HostKind> {
        self.host.map(|_| self.host_kind)
    }

    /// `getUnicodeHost()`.
    pub fn unicode_host(&self) -> Option<Vec<u8>> {
        let h = self.ascii_host()?;
        Some(match self.host_kind {
            HostKind::Domain | HostKind::Opaque => idna::to_unicode(h).unwrap_or_default(),
            _ => h.to_vec(),
        })
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub fn path(&self) -> &[u8] {
        self.path.get(&self.href)
    }

    pub fn query(&self) -> Option<&[u8]> {
        self.query.map(|q| q.get(&self.href))
    }

    pub fn fragment(&self) -> Option<&[u8]> {
        self.fragment.map(|f| f.get(&self.href))
    }

    /// Whether the path is opaque (`mailto:x`).
    pub fn has_opaque_path(&self) -> bool {
        self.opaque
    }

    // ---- serializations ----

    /// `toAsciiString()` (the href).
    pub fn to_ascii_string(&self) -> &[u8] {
        &self.href
    }

    /// `toUnicodeString()`: the href with the host in Unicode. When a host
    /// label does not decode, PHP stops the string before the host
    /// (observed on PHP 8.5 with `foo://xn--zz/`).
    pub fn to_unicode_string(&self) -> Vec<u8> {
        let Some(h) = self.host else {
            return self.href.to_vec();
        };
        let text = h.get(&self.href);
        let uni = match self.host_kind {
            HostKind::Domain | HostKind::Opaque => {
                if !text.windows(4).any(|w| w.eq_ignore_ascii_case(b"xn--"))
                    && !text.iter().any(u8::is_ascii_uppercase)
                {
                    return self.href.to_vec();
                }
                match idna::to_unicode(text) {
                    Some(u) => u,
                    None => return self.href[..h.start as usize].to_vec(),
                }
            }
            _ => return self.href.to_vec(),
        };
        let mut out = Vec::with_capacity(self.href.len() + uni.len());
        out.extend_from_slice(&self.href[..h.start as usize]);
        out.extend_from_slice(&uni);
        out.extend_from_slice(&self.href[h.end as usize..]);
        out
    }

    /// `equals()`: the serializations are compared (test
    /// whatwg/equivalence/equals_true_normalization3.phpt).
    pub fn equals(&self, other: &Url, mode: ComparisonMode) -> bool {
        match mode {
            ComparisonMode::IncludeFragment => self.href == other.href,
            ComparisonMode::ExcludeFragment => {
                self.href_without_fragment() == other.href_without_fragment()
            }
        }
    }

    fn href_without_fragment(&self) -> &[u8] {
        match self.fragment {
            Some(f) => &self.href[..f.start as usize - 1],
            None => &self.href,
        }
    }

    /// The `"uri"` entry of `__serialize()`: the ASCII serialization
    /// (test 031.phpt).
    pub fn serialized_uri(&self) -> &[u8] {
        &self.href
    }

    /// `__debugInfo()` (also what `var_dump()` and `print_r()` show):
    /// scheme, username, password, host (ASCII), port, path, query and
    /// fragment (test 003.phpt).
    pub fn debug_info(&self) -> [(&'static str, DebugValue<'_>); 8] {
        fn s(v: Option<&[u8]>) -> DebugValue<'_> {
            v.map_or(DebugValue::Null, DebugValue::Str)
        }
        [
            ("scheme", DebugValue::Str(self.scheme())),
            ("username", s(self.username())),
            ("password", s(self.password())),
            ("host", s(self.ascii_host())),
            (
                "port",
                self.port
                    .map_or(DebugValue::Null, |p| DebugValue::Int(p as i64)),
            ),
            ("path", DebugValue::Str(self.path())),
            ("query", s(self.query())),
            ("fragment", s(self.fragment())),
        ]
    }

    /// [`Url::debug_info`] as the PHP array.
    pub fn debug_array(&self) -> goro_runtime::PhpArray {
        crate::object::debug_array(self.debug_info())
    }

    // ---- withers ----

    /// Runs a setter on a copy of this URL (see the `errors` field for
    /// what happens to validation errors).
    fn modify(
        &self,
        component: Component,
        f: impl FnOnce(&mut Record, &mut Errors) -> Result<(), ()>,
    ) -> Result<Url, UrlError> {
        let mut errs = Errors::default();
        let mut rec = self.to_record();
        match f(&mut rec, &mut errs) {
            Ok(()) => {
                let mut u = Url::from_record(&rec);
                self.errors.borrow_mut().extend(errs.list);
                u.errors = self.errors.clone();
                Ok(u)
            }
            Err(()) => {
                let mut all = std::mem::take(&mut *self.errors.borrow_mut());
                let inherited = all.len();
                all.extend(errs.list);
                Err(UrlError::with_inherited(
                    component,
                    Errors {
                        list: all,
                        ipv4: None,
                    },
                    inherited,
                ))
            }
        }
    }

    /// `withScheme(string $scheme)` (URL Standard, protocol setter): a
    /// change between a special and a non-special scheme is ignored.
    pub fn with_scheme(&self, scheme: &[u8]) -> Result<Url, UrlError> {
        self.modify(Component::Scheme, |rec, errs| {
            // The value is cleaned (tabs and newlines) before the ':' goes
            // on (observed on PHP 8.5: the error context stops before it).
            let (cleaned, len) = parser::clean(scheme, errs, false);
            let mut v = cleaned[..len].to_vec();
            v.push(b':');
            let mut p = Parser {
                input: &v,
                ctx: &v,
                base: None,
                url: rec,
                errs,
                over: Some(State::SchemeStart),
            };
            p.run(State::SchemeStart)
        })
    }

    /// `withUsername(?string $username)` (null is the empty string).
    pub fn with_username(&self, username: Option<&[u8]>) -> Result<Url, UrlError> {
        self.modify(Component::Username, |rec, _| {
            if rec.cannot_have_credentials_or_port() {
                return Ok(());
            }
            rec.username.clear();
            parser::encode_into(
                &mut rec.username,
                username.unwrap_or(b""),
                parser::Set::Userinfo,
            );
            Ok(())
        })
    }

    /// `withPassword(?string $password)` (null is the empty string).
    pub fn with_password(&self, password: Option<&[u8]>) -> Result<Url, UrlError> {
        self.modify(Component::Password, |rec, _| {
            if rec.cannot_have_credentials_or_port() {
                return Ok(());
            }
            rec.password.clear();
            parser::encode_into(
                &mut rec.password,
                password.unwrap_or(b""),
                parser::Set::Userinfo,
            );
            Ok(())
        })
    }

    /// `withHost(?string $host)` (URL Standard, hostname setter; null is
    /// the empty string).
    pub fn with_host(&self, host: Option<&[u8]>) -> Result<Url, UrlError> {
        self.modify(Component::Host, |rec, errs| {
            if rec.opaque {
                return Ok(());
            }
            run_setter(rec, host.unwrap_or(b""), State::Hostname, errs)
        })
    }

    /// `withPort(?int $port)` (URL Standard, port setter).
    pub fn with_port(&self, port: Option<i64>) -> Result<Url, UrlError> {
        self.modify(Component::Port, |rec, errs| {
            if rec.cannot_have_credentials_or_port() {
                return Ok(());
            }
            match port {
                None => {
                    rec.port = None;
                    Ok(())
                }
                Some(v) if v < 0 => Err(()),
                Some(v) => run_setter(rec, v.to_string().as_bytes(), State::Port, errs),
            }
        })
    }

    /// `withPath(string $path)` (URL Standard, pathname setter).
    pub fn with_path(&self, path: &[u8]) -> Result<Url, UrlError> {
        self.modify(Component::Path, |rec, errs| {
            if rec.opaque {
                return Ok(());
            }
            rec.clear_path();
            run_setter(rec, path, State::PathStart, errs)
        })
    }

    /// `withQuery(?string $query)` (URL Standard, search setter; a leading
    /// `?` is dropped, the empty string removes the query).
    pub fn with_query(&self, query: Option<&[u8]>) -> Result<Url, UrlError> {
        self.modify(Component::Query, |rec, errs| match query {
            None | Some(b"") => {
                rec.query = None;
                Ok(())
            }
            Some(q) => {
                let q = q.strip_prefix(b"?").unwrap_or(q);
                rec.query = Some(Vec::new());
                run_setter(rec, q, State::Query, errs)
            }
        })
    }

    /// `withFragment(?string $fragment)` (URL Standard, hash setter).
    pub fn with_fragment(&self, fragment: Option<&[u8]>) -> Result<Url, UrlError> {
        self.modify(Component::Fragment, |rec, errs| match fragment {
            None | Some(b"") => {
                rec.fragment = None;
                Ok(())
            }
            Some(f) => {
                let f = f.strip_prefix(b"#").unwrap_or(f);
                rec.fragment = Some(Vec::new());
                run_setter(rec, f, State::Fragment, errs)
            }
        })
    }

    // ---- conversion to and from the record ----

    fn from_record(r: &Record) -> Url {
        let mut href = Vec::with_capacity(
            r.scheme.len()
                + r.username.len()
                + r.password.len()
                + r.host.as_ref().map_or(0, |h| h.text.len())
                + r.path.len()
                + r.query.as_ref().map_or(0, |q| q.len())
                + r.fragment.as_ref().map_or(0, |f| f.len())
                + 16,
        );
        let sp = |href: &Vec<u8>, start: usize| Span {
            start: start as u32,
            end: href.len() as u32,
        };
        href.extend_from_slice(&r.scheme);
        let scheme = sp(&href, 0);
        href.push(b':');
        let mut username = Span::default();
        let mut password = Span::default();
        let mut host = None;
        let mut host_kind = HostKind::Empty;
        if let Some(h) = &r.host {
            href.extend_from_slice(b"//");
            let at = href.len();
            username = Span {
                start: at as u32,
                end: at as u32,
            };
            password = username;
            if r.includes_credentials() {
                href.extend_from_slice(&r.username);
                username = sp(&href, at);
                if !r.password.is_empty() {
                    href.push(b':');
                    let ps = href.len();
                    href.extend_from_slice(&r.password);
                    password = sp(&href, ps);
                } else {
                    password = Span {
                        start: href.len() as u32,
                        end: href.len() as u32,
                    };
                }
                href.push(b'@');
            }
            let hs = href.len();
            href.extend_from_slice(&h.text);
            host = Some(sp(&href, hs));
            host_kind = h.kind;
            if let Some(p) = r.port {
                href.push(b':');
                let mut digits = [0u8; 5];
                let mut i = digits.len();
                let mut v = p;
                loop {
                    i -= 1;
                    digits[i] = b'0' + (v % 10) as u8;
                    v /= 10;
                    if v == 0 {
                        break;
                    }
                }
                href.extend_from_slice(&digits[i..]);
            }
        } else if !r.opaque && r.segs.len() > 1 && r.seg(0).is_empty() {
            href.extend_from_slice(b"/.");
        }
        let ps = href.len();
        href.extend_from_slice(&r.path);
        let path = sp(&href, ps);
        let query = r.query.as_ref().map(|q| {
            href.push(b'?');
            let s = href.len();
            href.extend_from_slice(q);
            sp(&href, s)
        });
        let fragment = r.fragment.as_ref().map(|f| {
            href.push(b'#');
            let s = href.len();
            href.extend_from_slice(f);
            sp(&href, s)
        });
        Url {
            href: href.into_boxed_slice(),
            scheme,
            username,
            password,
            host,
            host_kind,
            port: r.port,
            path,
            query,
            fragment,
            opaque: r.opaque,
            kind: r.kind,
            errors: Rc::default(),
        }
    }

    fn to_record(&self) -> Record {
        let mut r = Record::new();
        r.set_scheme(self.scheme());
        r.username = self.username.get(&self.href).to_vec();
        r.password = self.password.get(&self.href).to_vec();
        r.host = self.host.map(|h| Host {
            kind: self.host_kind,
            text: h.get(&self.href).to_vec(),
        });
        r.port = self.port;
        r.opaque = self.opaque;
        let path = self.path();
        if self.opaque {
            r.path = path.to_vec();
        } else {
            r.path = path.to_vec();
            for (i, &c) in path.iter().enumerate() {
                if c == b'/' {
                    r.segs.push(i as u32);
                }
            }
        }
        r.query = self.query().map(<[u8]>::to_vec);
        r.fragment = self.fragment().map(<[u8]>::to_vec);
        let _ = self.kind;
        r
    }
}

fn parse_record(
    input: &[u8],
    base: Option<&Record>,
    rec: &mut Record,
    errs: &mut Errors,
) -> Result<(), ()> {
    let (cleaned, len) = parser::clean(input, errs, true);
    let mut p = Parser {
        input: &cleaned[..len],
        ctx: &cleaned,
        base,
        url: rec,
        errs,
        over: None,
    };
    p.run(State::SchemeStart)
}

fn run_setter(rec: &mut Record, value: &[u8], state: State, errs: &mut Errors) -> Result<(), ()> {
    let (cleaned, len) = parser::clean(value, errs, false);
    let mut p = Parser {
        input: &cleaned[..len],
        ctx: &cleaned,
        base: None,
        url: rec,
        errs,
        over: Some(state),
    };
    p.run(state)
}
