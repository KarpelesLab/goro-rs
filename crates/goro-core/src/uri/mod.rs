//! ext/uri: Uri\Rfc3986\Uri (over the uriparser port in `rfc3986`) and
//! Uri\WhatWg\Url (over the lexbor URL parser port in `whatwg`), with their
//! exceptions, Uri\WhatWg\UrlValidationError and the parser registry other
//! extensions use (php_uri_get_parser()).
//!
//! This is a port of php_uri.c, php_uri_common.c, uri_parser_rfc3986.c,
//! uri_parser_whatwg.c and uri_parser_php_parse_url.c. The classes are
//! declared by the stubs; their methods are a native class handler and each
//! object carries its parsed URI as native data.

pub mod idna;
pub mod rfc3986;
pub mod whatwg;

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::{NativeData, PhpObject};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::{NativeClassHandler, NativeObjectHandlers, Vm};

type Obj = Rc<RefCell<PhpObject>>;

pub const RFC3986_CLASS: &str = "Uri\\Rfc3986\\Uri";
pub const WHATWG_CLASS: &str = "Uri\\WhatWg\\Url";
const RFC3986_LOWER: &[u8] = b"uri\\rfc3986\\uri";
const WHATWG_LOWER: &[u8] = b"uri\\whatwg\\url";
const INVALID_URI_EXCEPTION: &[u8] = b"Uri\\InvalidUriException";
const INVALID_URL_EXCEPTION: &[u8] = b"Uri\\WhatWg\\InvalidUrlException";
const URI_ERROR: &[u8] = b"Uri\\UriError";
const VALIDATION_ERROR_LOWER: &[u8] = b"uri\\whatwg\\urlvalidationerror";
const VALIDATION_ERROR_TYPE_LOWER: &[u8] = b"uri\\whatwg\\urlvalidationerrortype";

thread_local! {
    /// The request's lexbor URL parser log (lexbor_parser.log): emptied by
    /// each parse (lxb_url_parser_clean()) and by fill_errors(), but not by
    /// the setters, which append to it.
    static LEXBOR_LOG: RefCell<Vec<whatwg::LogEntry>> = const { RefCell::new(Vec::new()) };
}

/// php_uri_parser_rfc3986_uris
#[derive(Clone, Debug)]
pub struct RfcUris {
    pub uri: rfc3986::Uri,
    normalized: RefCell<Option<rfc3986::Uri>>,
}

impl RfcUris {
    fn new(uri: rfc3986::Uri) -> Self {
        RfcUris {
            uri,
            normalized: RefCell::new(None),
        }
    }

    /// get_normalized_uri()
    fn normalized(&self) -> std::cell::Ref<'_, rfc3986::Uri> {
        if self.normalized.borrow().is_none() {
            let mut n = self.uri.clone();
            n.normalize_syntax();
            *self.normalized.borrow_mut() = Some(n);
        }
        std::cell::Ref::map(self.normalized.borrow(), |n| n.as_ref().unwrap())
    }
}

/// The parsed URI of a parser.
#[derive(Clone, Debug)]
pub enum ParsedUri {
    Rfc3986(RfcUris),
    Whatwg(whatwg::Url),
    PhpUrl(PhpUrl),
}

/// php_uri_parser
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parser {
    Rfc3986,
    Whatwg,
    /// parse_url()
    PhpParseUrl,
}

/// php_uri_component_read_mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadMode {
    Raw,
    NormalizedAscii,
    NormalizedUnicode,
}

/// php_uri_recomposition_mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Recompose {
    RawAscii,
    RawUnicode,
    NormalizedAscii,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Component {
    Scheme,
    Username,
    Password,
    Host,
    Port,
    Path,
    Query,
    Fragment,
}

impl Component {
    fn name(self) -> &'static str {
        match self {
            Component::Scheme => "scheme",
            Component::Username => "username",
            Component::Password => "password",
            Component::Host => "host",
            Component::Port => "port",
            Component::Path => "path",
            Component::Query => "query",
            Component::Fragment => "fragment",
        }
    }
}

const ALL_COMPONENTS: [Component; 8] = [
    Component::Scheme,
    Component::Username,
    Component::Password,
    Component::Host,
    Component::Port,
    Component::Path,
    Component::Query,
    Component::Fragment,
];

/// php_uri_object: the parser of the class and the URI (None until the
/// object is initialized by its constructor / __unserialize()).
#[derive(Debug)]
struct UriObject {
    parser: Parser,
    uri: Option<ParsedUri>,
}

static URI_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    get_properties: Some(uri_get_properties),
    clone_obj: Some(uri_clone_obj),
    ..NativeObjectHandlers::EMPTY
};

fn s(v: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(v))
}

fn opt_s(v: Option<&[u8]>) -> Value {
    match v {
        Some(v) => s(v),
        None => Value::Null,
    }
}

fn key(k: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(k.as_bytes()))
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let exc = vm.create_exception(class, msg, 0);
    vm.chain_pending_exception(&exc);
    vm.current_exception = Some(exc);
}

fn state_of(o: &Obj) -> Option<Rc<RefCell<UriObject>>> {
    let data = o.borrow().native.as_ref()?.data.clone();
    data.downcast::<RefCell<UriObject>>().ok()
}

fn attach(o: &Obj, st: UriObject) {
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(RefCell::new(st)),
        handlers: &URI_HANDLERS,
    });
}

fn parser_of_class(class_lower: &[u8]) -> Option<Parser> {
    match class_lower {
        RFC3986_LOWER => Some(Parser::Rfc3986),
        WHATWG_LOWER => Some(Parser::Whatwg),
        _ => None,
    }
}

/// The class name of an object for messages.
fn class_name_of(o: &Obj) -> String {
    crate::value::display_class_name(&o.borrow().class_name)
}

// ----------------------------------------------------------------------
// Registration

/// PHP_MINIT(uri)
pub fn register(vm: &mut Vm) {
    let h = NativeClassHandler {
        has_method,
        call: call_method,
        prop_added: None,
        prop_write: None,
        compare_check: None,
        prop_read: None,
        param_names: Some(param_names),
    };
    for c in [RFC3986_LOWER, WHATWG_LOWER] {
        vm.native_class_handlers.insert(c.to_vec(), h);
        vm.object_create_handlers.insert(c.to_vec(), create_object);
        vm.object_construct_handlers
            .insert(c.to_vec(), construct_uri);
    }
    vm.object_construct_handlers.insert(
        b"uri\\whatwg\\invalidurlexception".to_vec(),
        construct_invalid_url_exception,
    );
    vm.object_construct_handlers
        .insert(VALIDATION_ERROR_LOWER.to_vec(), construct_validation_error);
    // The enums
    let error_types: Vec<&str> = ALL_ERROR_TYPES.iter().map(|t| t.php_case().0).collect();
    for (name, cases) in [
        (
            "Uri\\UriComparisonMode",
            vec!["IncludeFragment", "ExcludeFragment"],
        ),
        ("Uri\\WhatWg\\UrlValidationErrorType", error_types),
    ] {
        let mut ce = crate::object::ClassEntry::new(name.as_bytes().to_vec());
        ce.is_enum = true;
        ce.enum_backing_type = None;
        for c in cases {
            ce.enum_cases.push((c.as_bytes().to_vec(), Value::Null));
            let mut marker = b"__enum_case__::".to_vec();
            marker.extend_from_slice(c.as_bytes());
            ce.constants.insert(
                c.as_bytes().to_vec(),
                Value::String(PhpString::from_vec(marker)),
            );
        }
        vm.classes
            .insert(name.to_ascii_lowercase().into_bytes(), ce);
    }
}

/// UrlValidationErrorType's cases, in declaration order.
const ALL_ERROR_TYPES: [whatwg::ErrorType; 29] = {
    use whatwg::ErrorType::*;
    [
        DomainToAscii,
        DomainToUnicode,
        DomainInvalidCodePoint,
        HostInvalidCodePoint,
        Ipv4EmptyPart,
        Ipv4TooManyParts,
        Ipv4NonNumericPart,
        Ipv4NonDecimalPart,
        Ipv4OutOfRangePart,
        Ipv6Unclosed,
        Ipv6InvalidCompression,
        Ipv6TooManyPieces,
        Ipv6MultipleCompression,
        Ipv6InvalidCodePoint,
        Ipv6TooFewPieces,
        Ipv4InIpv6TooManyPieces,
        Ipv4InIpv6InvalidCodePoint,
        Ipv4InIpv6OutOfRangePart,
        Ipv4InIpv6TooFewParts,
        InvalidUrlUnit,
        SpecialSchemeMissingFollowingSolidus,
        MissingSchemeNonRelativeUrl,
        InvalidReverseSolidus,
        InvalidCredentials,
        HostMissing,
        PortOutOfRange,
        PortInvalid,
        FileInvalidWindowsDriveLetter,
        FileInvalidWindowsDriveLetterHost,
    ]
};

/// php_uri_object_create_rfc3986() / php_uri_object_create_whatwg()
fn create_object(_vm: &mut Vm, o: &Obj) {
    if o.borrow().native.is_some() {
        return;
    }
    let cls = o.borrow().class_name.to_ascii_lowercase();
    let parser = parser_of_class(&cls).unwrap_or(Parser::Rfc3986);
    attach(o, UriObject { parser, uri: None });
}

/// php_uri_object_handler_clone(): the clone gets a copy of the URI (but
/// not the cached normalized form).
fn uri_clone_obj(_vm: &mut Vm, src: &Obj, dst: &Obj) {
    let Some(st) = state_of(src) else {
        return;
    };
    let st = st.borrow();
    let uri = st.uri.as_ref().map(|u| match u {
        ParsedUri::Rfc3986(r) => ParsedUri::Rfc3986(RfcUris::new(r.uri.clone())),
        ParsedUri::Whatwg(w) => ParsedUri::Whatwg(w.clone()),
        ParsedUri::PhpUrl(u) => ParsedUri::PhpUrl(u.clone()),
    });
    attach(
        dst,
        UriObject {
            parser: st.parser,
            uri,
        },
    );
}

/// get_properties: var_dump()/print_r() show uri_get_debug_properties().
fn uri_get_properties(_vm: &mut Vm, o: &Obj, purpose: u8) -> Option<PhpArray> {
    if purpose != 0 {
        return None;
    }
    Some(debug_properties(o))
}

/// uri_get_debug_properties()
fn debug_properties(o: &Obj) -> PhpArray {
    let mut result = PhpArray::new();
    for (k, v) in o.borrow().properties.iter() {
        result.set(ArrayKey::String(PhpString::from_vec(k.clone())), v.clone());
    }
    let Some(st) = state_of(o) else {
        return result;
    };
    let st = st.borrow();
    let Some(uri) = &st.uri else {
        return result;
    };
    for c in ALL_COMPONENTS {
        result.set(key(c.name()), read_component(uri, c, ReadMode::Raw));
    }
    result
}

// ----------------------------------------------------------------------
// Property handlers (read)

/// The `read` property handler of a parser.
fn read_component(uri: &ParsedUri, c: Component, mode: ReadMode) -> Value {
    match uri {
        ParsedUri::Rfc3986(r) => {
            if mode == ReadMode::Raw {
                rfc_read(&r.uri, c)
            } else {
                rfc_read(&r.normalized(), c)
            }
        }
        ParsedUri::Whatwg(u) => whatwg_read(u, c, mode),
        ParsedUri::PhpUrl(u) => php_url_read(u, c, mode),
    }
}

/// uri_parser_php_parse_url_*_read()
fn php_url_read(u: &PhpUrl, c: Component, mode: ReadMode) -> Value {
    let v = match c {
        Component::Scheme => &u.scheme,
        Component::Username => &u.user,
        Component::Password => &u.pass,
        Component::Host => &u.host,
        Component::Port => {
            return if u.port != 0 {
                Value::Long(u.port as i64)
            } else {
                Value::Null
            };
        }
        Component::Path => &u.path,
        Component::Query => &u.query,
        Component::Fragment => &u.fragment,
    };
    match v {
        // decode_component()
        Some(v) if mode != ReadMode::Raw => s(&raw_url_decode(v)),
        Some(v) => s(v),
        None => Value::Null,
    }
}

/// php_raw_url_decode_ex()
fn raw_url_decode(v: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len());
    let mut i = 0;
    while i < v.len() {
        if v[i] == b'%' && i + 2 < v.len() {
            if let (Some(h), Some(l)) = (
                (v[i + 1] as char).to_digit(16),
                (v[i + 2] as char).to_digit(16),
            ) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(v[i]);
        i += 1;
    }
    out
}

/// php_url (ext/standard/url.h), as php_url_parse_ex2() gives it.
#[derive(Clone, Debug, Default)]
pub struct PhpUrl {
    pub scheme: Option<Vec<u8>>,
    pub user: Option<Vec<u8>>,
    pub pass: Option<Vec<u8>>,
    pub host: Option<Vec<u8>>,
    pub port: u16,
    pub path: Option<Vec<u8>>,
    pub query: Option<Vec<u8>>,
    pub fragment: Option<Vec<u8>>,
}

/// php_url_parse_ex2(), provided by ext/standard.
static PARSE_URL: std::sync::OnceLock<fn(&[u8]) -> Option<PhpUrl>> = std::sync::OnceLock::new();

/// Install the implementation of the parse_url() parser (php_url_parse_ex2()).
pub fn set_parse_url_impl(f: fn(&[u8]) -> Option<PhpUrl>) {
    let _ = PARSE_URL.set(f);
}

/// php_uri_parser_rfc3986_*_read()
fn rfc_read(u: &rfc3986::Uri, c: Component) -> Value {
    match c {
        Component::Scheme => opt_s(u.scheme.as_deref()),
        Component::Username => match &u.user_info {
            Some(ui) => match ui.iter().position(|&b| b == b':') {
                None if !ui.is_empty() => s(ui),
                Some(i) if i > 0 => s(&ui[..i]),
                _ => s(b""),
            },
            None => Value::Null,
        },
        Component::Password => match &u.user_info {
            Some(ui) => match ui.iter().position(|&b| b == b':') {
                Some(i) if ui.len() - i - 1 > 0 => s(&ui[i + 1..]),
                _ => s(b""),
            },
            None => Value::Null,
        },
        Component::Host => match &u.host_text {
            Some(h) => {
                if u.ip6.is_some() || u.ip_future {
                    let mut v = Vec::with_capacity(h.len() + 2);
                    v.push(b'[');
                    v.extend_from_slice(h);
                    v.push(b']');
                    s(&v)
                } else {
                    s(h)
                }
            }
            None => Value::Null,
        },
        Component::Port => match &u.port_text {
            Some(p) if !p.is_empty() => Value::Long(port_str_to_long_checked(p)),
            _ => Value::Null,
        },
        Component::Path => {
            if !u.path.is_empty() {
                let mut v = Vec::new();
                if u.absolute_path || u.has_host() {
                    v.push(b'/');
                }
                for (i, seg) in u.path.iter().enumerate() {
                    v.extend_from_slice(seg);
                    if i + 1 < u.path.len() {
                        v.push(b'/');
                    }
                }
                s(&v)
            } else if u.absolute_path {
                s(b"/")
            } else {
                s(b"")
            }
        }
        Component::Query => opt_s(u.query.as_deref()),
        Component::Fragment => opt_s(u.fragment.as_deref()),
    }
}

/// port_str_to_zend_long_checked()
fn port_str_to_long_checked(p: &[u8]) -> i64 {
    // MAX_LENGTH_OF_LONG on 64-bit platforms
    if p.len() > 20 {
        return -1;
    }
    let mut r: u64 = 0;
    for &c in p {
        if !c.is_ascii_digit() {
            break;
        }
        r = match r
            .checked_mul(10)
            .and_then(|r| r.checked_add((c - b'0') as u64))
        {
            Some(v) => v,
            None => return -1,
        };
    }
    if r > i64::MAX as u64 {
        return -1;
    }
    r as i64
}

/// php_uri_parser_whatwg_*_read()
fn whatwg_read(u: &whatwg::Url, c: Component, mode: ReadMode) -> Value {
    use whatwg::Host;
    match c {
        Component::Scheme => s(&u.scheme_name),
        Component::Username => {
            if u.includes_credentials() {
                s(&u.username)
            } else {
                Value::Null
            }
        }
        Component::Password => {
            if u.includes_credentials() {
                s(&u.password)
            } else {
                Value::Null
            }
        }
        Component::Host => match &u.host {
            Host::Ipv4(ip) => s(&whatwg::serialize_ipv4(*ip)),
            Host::Ipv6(ip) => {
                let mut v = vec![b'['];
                v.extend_from_slice(&whatwg::serialize_ipv6(ip));
                v.push(b']');
                s(&v)
            }
            Host::Empty => s(b""),
            Host::Undef => Value::Null,
            Host::Domain(d) | Host::Opaque(d) => {
                if mode == ReadMode::NormalizedUnicode {
                    let mut out = Vec::new();
                    let _ = whatwg::serialize_host_unicode(&u.host, &mut out);
                    s(&out)
                } else {
                    s(d)
                }
            }
        },
        Component::Port => {
            if u.has_port {
                Value::Long(u.port as i64)
            } else {
                Value::Null
            }
        }
        Component::Path => s(u.path.bytes()),
        Component::Query => opt_s(u.query.as_deref()),
        Component::Fragment => opt_s(u.fragment.as_deref()),
    }
}

/// The parser's to_string()
fn to_string(uri: &ParsedUri, mode: Recompose, exclude_fragment: bool) -> Vec<u8> {
    match uri {
        ParsedUri::Rfc3986(r) => {
            let mut out = match mode {
                Recompose::RawAscii | Recompose::RawUnicode => r.uri.to_string_bytes(),
                _ => r.normalized().to_string_bytes(),
            };
            if exclude_fragment {
                if let Some(pos) = out.iter().rposition(|&b| b == b'#') {
                    out.truncate(pos);
                }
            }
            out
        }
        ParsedUri::Whatwg(u) => {
            let unicode = mode == Recompose::RawUnicode;
            whatwg::serialize(u, unicode, exclude_fragment)
        }
        // The parse_url() parser has no to_string()
        ParsedUri::PhpUrl(_) => Vec::new(),
    }
}

// ----------------------------------------------------------------------
// Parsing

/// The reason a parse failed, for callers that report it themselves.
pub enum ParseFailure {
    /// An exception was thrown (not silent).
    Thrown,
    /// Silent failure.
    Failed,
}

/// php_uri_parser_rfc3986_parse_ex()
fn rfc_parse(vm: &mut Vm, input: &[u8], base: Option<&RfcUris>, silent: bool) -> Option<RfcUris> {
    let mut uri = match rfc3986::parse(input) {
        Ok(u) => u,
        Err(_) => {
            if !silent {
                throw(vm, INVALID_URI_EXCEPTION, "The specified URI is malformed");
            }
            return None;
        }
    };
    if let Some(b) = base {
        match rfc3986::Uri::add_base(&uri, &b.uri) {
            Ok(u) => uri = u,
            Err(_) => {
                if !silent {
                    throw(
                        vm,
                        INVALID_URI_EXCEPTION,
                        "The specified base URI must be absolute",
                    );
                }
                return None;
            }
        }
    }
    if let Some(p) = &uri.port_text {
        if !p.is_empty() && port_str_to_long_checked(p) == -1 {
            if !silent {
                throw(vm, INVALID_URI_EXCEPTION, "The port is out of range");
            }
            return None;
        }
    }
    Some(RfcUris::new(uri))
}

/// fill_errors(): drain the lexbor log into an array of
/// Uri\WhatWg\UrlValidationError (last logged first) and the name of the
/// first-logged failure.
fn fill_errors(vm: &mut Vm) -> (Value, Option<&'static str>) {
    let entries: Vec<whatwg::LogEntry> = LEXBOR_LOG.with(|l| std::mem::take(&mut *l.borrow_mut()));
    let mut arr = PhpArray::new();
    let mut result = None;
    for e in entries.into_iter().rev() {
        let (name, failure) = e.ty.php_case();
        if let Some(err) = new_validation_error(vm, &e.context, name, failure) {
            arr.push(err);
        }
        if failure {
            result = Some(name);
        }
    }
    (Value::Array(Rc::new(RefCell::new(arr))), result)
}

/// A Uri\WhatWg\UrlValidationError created as fill_errors() does.
fn new_validation_error(vm: &mut Vm, context: &[u8], case: &str, failure: bool) -> Option<Value> {
    let o = object_init_ex(vm, VALIDATION_ERROR_LOWER)?;
    let ty = vm
        .get_enum_case(VALIDATION_ERROR_TYPE_LOWER, case.as_bytes())
        .unwrap_or(Value::Null);
    {
        let mut b = o.borrow_mut();
        b.set_property(b"context".to_vec(), s(context));
        b.set_property(b"type".to_vec(), ty);
        b.set_property(
            b"failure".to_vec(),
            if failure { Value::True } else { Value::False },
        );
    }
    Some(Value::Object(o))
}

fn malformed_message(what: &str, reason: Option<&str>) -> String {
    match reason {
        Some(r) => format!("The specified {} is malformed ({})", what, r),
        None => format!("The specified {} is malformed", what),
    }
}

/// Throw Uri\WhatWg\InvalidUrlException with its errors.
fn throw_invalid_url(vm: &mut Vm, msg: &str, errors: Value) {
    let exc = vm.create_exception(INVALID_URL_EXCEPTION, msg, 0);
    if let Value::Object(o) = &exc {
        o.borrow_mut().set_property(b"errors".to_vec(), errors);
    }
    vm.chain_pending_exception(&exc);
    vm.current_exception = Some(exc);
}

/// php_uri_parser_whatwg_parse_ex(): `errors` receives the errors array
/// when requested (Some).
fn whatwg_parse(
    vm: &mut Vm,
    input: &[u8],
    base: Option<&whatwg::Url>,
    errors: Option<&mut Value>,
    silent: bool,
) -> Option<whatwg::Url> {
    let mut p = whatwg::Parser::default();
    let url = p.parse(base, input);
    LEXBOR_LOG.with(|l| *l.borrow_mut() = p.log);
    let want = errors.is_some();
    if (url.is_none() && !silent) || want {
        let (err, reason) = fill_errors(vm);
        if url.is_none() && !silent {
            throw_invalid_url(vm, &malformed_message("URI", reason), err.clone());
        }
        if let Some(e) = errors {
            *e = err;
        }
    }
    url
}

/// The parser's parse(): `errors` as for whatwg_parse().
fn parser_parse(
    vm: &mut Vm,
    parser: Parser,
    input: &[u8],
    base: Option<&ParsedUri>,
    errors: Option<&mut Value>,
    silent: bool,
) -> Option<ParsedUri> {
    match parser {
        Parser::Rfc3986 => {
            let b = match base {
                Some(ParsedUri::Rfc3986(r)) => Some(r),
                _ => None,
            };
            rfc_parse(vm, input, b, silent).map(ParsedUri::Rfc3986)
        }
        Parser::Whatwg => {
            let b = match base {
                Some(ParsedUri::Whatwg(u)) => Some(u),
                _ => None,
            };
            whatwg_parse(vm, input, b, errors, silent).map(ParsedUri::Whatwg)
        }
        Parser::PhpParseUrl => {
            // uri_parser_php_parse_url_parse()
            let url = PARSE_URL.get().and_then(|f| f(input));
            if url.is_none() && !silent {
                throw(vm, INVALID_URI_EXCEPTION, "The specified URI is malformed");
            }
            url.map(ParsedUri::PhpUrl)
        }
    }
}

/// object_init_ex(): a new object of the internal class `class_lower` with
/// its create_object hook run (no constructor call).
fn object_init_ex(vm: &mut Vm, class_lower: &[u8]) -> Option<Obj> {
    let name = vm
        .classes
        .get(class_lower)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| vm.builtin_canonical_name(class_lower).into_bytes());
    let id = vm.alloc_object_id();
    let mut obj = PhpObject::new(name, id);
    vm.apply_class_property_defaults(&mut obj, class_lower);
    if !vm.classes.contains_key(class_lower) {
        if let Some(sc) = crate::stubs::find_class(class_lower) {
            for p in sc.props {
                if p.flags & crate::stubs::F_STATIC != 0 || obj.has_property(p.name.as_bytes()) {
                    continue;
                }
                let v = p
                    .default
                    .and_then(crate::stubs::literal_value)
                    .unwrap_or(Value::Undef);
                obj.set_property(p.name.as_bytes().to_vec(), v);
            }
        }
    }
    let o = Rc::new(RefCell::new(obj));
    crate::gc::track_object(&o);
    vm.run_object_create_hook(&o);
    if vm.current_exception.is_some() {
        return None;
    }
    Some(o)
}

/// pass_errors_by_ref_and_free(): false when the assignment threw.
fn pass_errors_by_ref(vm: &mut Vm, errors_ref: Option<&Value>, errors: Value) -> bool {
    if matches!(errors, Value::Undef) {
        return true;
    }
    let Some(Value::Reference(r)) = errors_ref else {
        return true;
    };
    vm.try_assign_ref(r, errors).is_ok()
}

/// The URI of an initialized object of a URI class.
fn uri_of(o: &Obj) -> Option<(Parser, ParsedUri)> {
    let st = state_of(o)?;
    let st = st.borrow();
    Some((st.parser, st.uri.clone()?))
}

/// php_uri_instantiate_uri(). `this`: the object the method was called
/// on, or for a static call None (`called` names the class then).
#[allow(clippy::too_many_arguments)]
fn instantiate_uri(
    vm: &mut Vm,
    called: &[u8],
    this: Option<&Obj>,
    uri_str: &[u8],
    base: Option<&Obj>,
    should_throw: bool,
    should_update_this: bool,
    errors_ref: Option<&Value>,
) -> Value {
    let (target, parser) = if should_update_this {
        let Some(t) = this else {
            return Value::Null;
        };
        let st = match state_of(t) {
            Some(st) => st,
            None => {
                create_object(vm, t);
                state_of(t).expect("uri state")
            }
        };
        if st.borrow().uri.is_some() {
            let msg = format!(
                "Cannot modify readonly object of class {}",
                class_name_of(t)
            );
            throw(vm, b"Error", &msg);
            return Value::Null;
        }
        let parser = st.borrow().parser;
        (t.clone(), parser)
    } else {
        let class_lower = match this {
            Some(t) => t.borrow().class_name.to_ascii_lowercase(),
            None => called.to_ascii_lowercase(),
        };
        let Some(o) = object_init_ex(vm, &class_lower) else {
            return Value::Null;
        };
        let parser = state_of(&o)
            .map(|st| st.borrow().parser)
            .unwrap_or(Parser::Rfc3986);
        (o, parser)
    };
    let base_uri = base.and_then(uri_of).map(|(_, u)| u);
    let mut errors = Value::Undef;
    let want_errors = errors_ref.is_some_and(|v| !matches!(v, Value::Undef));
    let parsed = parser_parse(
        vm,
        parser,
        uri_str,
        base_uri.as_ref(),
        if want_errors { Some(&mut errors) } else { None },
        !should_throw,
    );
    let Some(parsed) = parsed else {
        if should_throw {
            return Value::Null;
        }
        if !pass_errors_by_ref(vm, errors_ref, errors) {
            return Value::Null;
        }
        return Value::Null;
    };
    if !pass_errors_by_ref(vm, errors_ref, errors) {
        return Value::Null;
    }
    if let Some(st) = state_of(&target) {
        st.borrow_mut().uri = Some(parsed);
    }
    if should_update_this {
        Value::Null
    } else {
        Value::Object(target)
    }
}

// ----------------------------------------------------------------------
// Constructors

/// new Uri\Rfc3986\Uri() / new Uri\WhatWg\Url()
fn construct_uri(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let Some(Value::Object(this)) = args.first().map(|v| v.deref()) else {
        return Ok(());
    };
    let cls = this.borrow().class_name.to_ascii_lowercase();
    let name = if cls == WHATWG_LOWER {
        "Uri\\WhatWg\\Url::__construct"
    } else {
        "Uri\\Rfc3986\\Uri::__construct"
    };
    let frame = name.replace("::", "->");
    let file = vm.current_file.clone();
    let line = vm.current_line;
    vm.call_stack
        .push((frame, file, line, args[1..].to_vec(), true));
    method_create(vm, &cls, &cls, Some(&this), &args[1..], true);
    vm.call_stack.pop();
    if vm.current_exception.is_some() {
        return Err("exception".to_string());
    }
    Ok(())
}

/// create_rfc3986_uri() / create_whatwg_uri()
fn method_create(
    vm: &mut Vm,
    class_lower: &[u8],
    called: &[u8],
    this: Option<&Obj>,
    args: &[Value],
    is_constructor: bool,
) -> Value {
    let uri_str = str_arg(args, 0);
    let base = match args.get(1).map(|v| v.deref()) {
        Some(Value::Object(o)) => Some(o),
        _ => None,
    };
    let errors_ref = if class_lower == WHATWG_LOWER {
        args.get(2)
    } else {
        None
    };
    instantiate_uri(
        vm,
        called,
        this,
        &uri_str,
        base.as_ref(),
        is_constructor,
        is_constructor,
        errors_ref,
    )
}

/// The InvalidUrlException::__construct() check is_list_of_whatwg_validation_errors()
fn is_list_of_validation_errors(a: &PhpArray) -> bool {
    for (next, (k, v)) in a.iter().enumerate() {
        if *k != ArrayKey::Int(next as i64) {
            return false;
        }
        match v {
            Value::Object(o) => {
                if !o
                    .borrow()
                    .class_name
                    .eq_ignore_ascii_case(b"Uri\\WhatWg\\UrlValidationError")
                {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn readonly_error(vm: &mut Vm, class: &str, prop: &str) {
    let msg = format!("Cannot modify readonly property {}::${}", class, prop);
    throw(vm, b"Error", &msg);
}

/// Whether a declared property has been initialized.
fn prop_initialized(o: &Obj, name: &[u8]) -> bool {
    o.borrow()
        .properties
        .iter()
        .find(|(k, _)| k == name)
        .is_some_and(|(_, v)| !matches!(v, Value::Undef))
}

/// Uri\WhatWg\InvalidUrlException::__construct()
fn construct_invalid_url_exception(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let Some(Value::Object(this)) = args.first().map(|v| v.deref()) else {
        return Ok(());
    };
    let a = &args[1..];
    let file = vm.current_file.clone();
    let line = vm.current_line;
    vm.call_stack.push((
        "Uri\\WhatWg\\InvalidUrlException->__construct".to_string(),
        file,
        line,
        a.to_vec(),
        true,
    ));
    let r = invalid_url_exception_ctor(vm, &this, a);
    vm.call_stack.pop();
    r
}

fn invalid_url_exception_ctor(vm: &mut Vm, this: &Obj, a: &[Value]) -> Result<(), String> {
    let class = class_name_of(this);
    // zend_update_exception_properties()
    if let Some(m) = a.first().map(|v| v.deref()) {
        if !matches!(m, Value::Undef) {
            let m = match m {
                Value::String(s) => s,
                v => v.to_php_string(),
            };
            this.borrow_mut()
                .set_property(b"message".to_vec(), Value::String(m));
        }
    }
    let code = match a.get(2).map(|v| v.deref()) {
        Some(Value::Long(n)) => n,
        Some(Value::Undef) | None => 0,
        Some(v) => v.to_long(),
    };
    if code != 0 {
        this.borrow_mut()
            .set_property(b"code".to_vec(), Value::Long(code));
    }
    if let Some(p @ Value::Object(_)) = a.get(3).map(|v| v.deref()) {
        this.borrow_mut().set_property(b"previous".to_vec(), p);
    }
    let errors = match a.get(1).map(|v| v.deref()) {
        None | Some(Value::Undef) => Value::Array(Rc::new(RefCell::new(PhpArray::new()))),
        Some(Value::Array(arr)) => {
            if !is_list_of_validation_errors(&arr.borrow()) {
                let msg = format!(
                    "{}::__construct(): Argument #2 ($errors) must be a list of Uri\\WhatWg\\UrlValidationError",
                    "Uri\\WhatWg\\InvalidUrlException"
                );
                throw(vm, b"ValueError", &msg);
                return Err("exception".to_string());
            }
            Value::Array(arr)
        }
        Some(v) => v,
    };
    if prop_initialized(this, b"errors") {
        readonly_error(vm, "Uri\\WhatWg\\InvalidUrlException", "errors");
        let _ = class;
        return Err("exception".to_string());
    }
    this.borrow_mut().set_property(b"errors".to_vec(), errors);
    Ok(())
}

/// Uri\WhatWg\UrlValidationError::__construct()
fn construct_validation_error(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let Some(Value::Object(this)) = args.first().map(|v| v.deref()) else {
        return Ok(());
    };
    let a = &args[1..];
    let file = vm.current_file.clone();
    let line = vm.current_line;
    vm.call_stack.push((
        "Uri\\WhatWg\\UrlValidationError->__construct".to_string(),
        file,
        line,
        a.to_vec(),
        true,
    ));
    let r = (|| {
        let context = str_arg(a, 0);
        let ty = a.get(1).map(|v| v.deref()).unwrap_or(Value::Null);
        let failure = a.get(2).map(|v| v.deref().is_truthy()).unwrap_or(false);
        for (name, v) in [
            ("context", s(&context)),
            ("type", ty),
            ("failure", if failure { Value::True } else { Value::False }),
        ] {
            if prop_initialized(&this, name.as_bytes()) {
                readonly_error(vm, "Uri\\WhatWg\\UrlValidationError", name);
                return Err("exception".to_string());
            }
            this.borrow_mut().set_property(name.as_bytes().to_vec(), v);
        }
        Ok(())
    })();
    vm.call_stack.pop();
    r
}

// ----------------------------------------------------------------------
// Methods

fn str_arg(args: &[Value], i: usize) -> Vec<u8> {
    match args.get(i).map(|v| v.deref()) {
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        Some(Value::Undef) | Some(Value::Null) | None => Vec::new(),
        Some(v) => v.to_php_string().as_bytes().to_vec(),
    }
}

/// A ?string argument
fn opt_str_arg(args: &[Value], i: usize) -> Option<Vec<u8>> {
    match args.get(i).map(|v| v.deref()) {
        Some(Value::Undef) | Some(Value::Null) | None => None,
        Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
        Some(v) => Some(v.to_php_string().as_bytes().to_vec()),
    }
}

/// A ?int argument
fn opt_long_arg(args: &[Value], i: usize) -> Option<i64> {
    match args.get(i).map(|v| v.deref()) {
        Some(Value::Undef) | Some(Value::Null) | None => None,
        Some(Value::Long(n)) => Some(n),
        Some(v) => Some(v.to_long()),
    }
}

/// The canonical name and parameter names of a method of the URI classes.
fn method_info(
    class: &[u8],
    method_lower: &[u8],
) -> Option<(&'static str, &'static [&'static str])> {
    let rfc = class == RFC3986_LOWER;
    let whatwg = class == WHATWG_LOWER;
    if !rfc && !whatwg {
        return None;
    }
    const NONE: &[&str] = &[];
    let r: (&str, &[&str]) = match method_lower {
        b"parse" => (
            "parse",
            if rfc {
                &["uri", "baseUrl"]
            } else {
                &["uri", "baseUrl", "errors"]
            },
        ),
        b"getscheme" => ("getScheme", NONE),
        b"withscheme" => ("withScheme", &["scheme"]),
        b"getusername" => ("getUsername", NONE),
        b"getpassword" => ("getPassword", NONE),
        b"withhost" => ("withHost", &["host"]),
        b"getport" => ("getPort", NONE),
        b"withport" => ("withPort", &["port"]),
        b"getpath" => ("getPath", NONE),
        b"withpath" => ("withPath", &["path"]),
        b"getquery" => ("getQuery", NONE),
        b"withquery" => ("withQuery", &["query"]),
        b"getfragment" => ("getFragment", NONE),
        b"withfragment" => ("withFragment", &["fragment"]),
        b"equals" => (
            "equals",
            if rfc {
                &["uri", "comparisonMode"]
            } else {
                &["url", "comparisonMode"]
            },
        ),
        b"resolve" => (
            "resolve",
            if rfc {
                &["uri"]
            } else {
                &["uri", "softErrors"]
            },
        ),
        b"__serialize" => ("__serialize", NONE),
        b"__unserialize" => ("__unserialize", &["data"]),
        b"__debuginfo" => ("__debugInfo", NONE),
        _ if rfc => match method_lower {
            b"getrawscheme" => ("getRawScheme", NONE),
            b"getuserinfo" => ("getUserInfo", NONE),
            b"getrawuserinfo" => ("getRawUserInfo", NONE),
            b"withuserinfo" => ("withUserInfo", &["userinfo"]),
            b"getrawusername" => ("getRawUsername", NONE),
            b"getrawpassword" => ("getRawPassword", NONE),
            b"gethost" => ("getHost", NONE),
            b"getrawhost" => ("getRawHost", NONE),
            b"getrawpath" => ("getRawPath", NONE),
            b"getrawquery" => ("getRawQuery", NONE),
            b"getrawfragment" => ("getRawFragment", NONE),
            b"tostring" => ("toString", NONE),
            b"torawstring" => ("toRawString", NONE),
            _ => return None,
        },
        _ => match method_lower {
            b"withusername" => ("withUsername", &["username"]),
            b"withpassword" => ("withPassword", &["password"]),
            b"getasciihost" => ("getAsciiHost", NONE),
            b"getunicodehost" => ("getUnicodeHost", NONE),
            b"toasciistring" => ("toAsciiString", NONE),
            b"tounicodestring" => ("toUnicodeString", NONE),
            _ => return None,
        },
    };
    Some(r)
}

fn has_method(class: &[u8], method: &[u8], is_static: bool) -> bool {
    let m = method.to_ascii_lowercase();
    if m == b"__construct" {
        return false;
    }
    match method_info(class, &m) {
        Some(_) => !is_static || m == b"parse",
        None => false,
    }
}

fn param_names(class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    let m = method.to_ascii_lowercase();
    if m == b"__construct" {
        return Some(if class == WHATWG_LOWER {
            &["uri", "baseUrl", "softErrors"]
        } else {
            &["uri", "baseUrl"]
        });
    }
    method_info(class, &m).map(|(_, p)| p)
}

fn this_obj(this: Option<&Value>) -> Option<Obj> {
    match this.map(|v| v.deref()) {
        Some(Value::Object(o)) => Some(o),
        _ => None,
    }
}

fn call_method(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let m = method.to_ascii_lowercase();
    let Some((canon, _)) = method_info(class, &m) else {
        return Value::Null;
    };
    let class_disp = if class == WHATWG_LOWER {
        WHATWG_CLASS
    } else {
        RFC3986_CLASS
    };
    if m == b"parse" {
        let called = if called.is_empty() { class } else { called };
        return method_create(vm, class, called, None, args, false);
    }
    let Some(o) = this_obj(this) else {
        let msg = format!(
            "Non-static method {}::{}() cannot be called statically",
            class_disp, canon
        );
        throw(vm, b"Error", &msg);
        return Value::Null;
    };
    match m.as_slice() {
        b"__unserialize" => return uri_unserialize(vm, &o, args),
        b"__debuginfo" => return Value::Array(Rc::new(RefCell::new(debug_properties(&o)))),
        _ => {}
    }
    let Some((parser, uri)) = uri_of(&o) else {
        // An object that was never initialized (ZEND_ASSERT(uri != NULL))
        let msg = format!("{} object is not initialized", class_name_of(&o));
        throw(vm, b"Error", &msg);
        return Value::Null;
    };
    let is_rfc = parser == Parser::Rfc3986;
    match m.as_slice() {
        // Readers
        b"getscheme" => read_helper(vm, &uri, Component::Scheme, ReadMode::NormalizedAscii),
        b"getrawscheme" => read_helper(vm, &uri, Component::Scheme, ReadMode::Raw),
        b"getuserinfo" | b"getrawuserinfo" => {
            let ParsedUri::Rfc3986(r) = &uri else {
                return Value::Null;
            };
            if m == b"getuserinfo" {
                opt_s(r.normalized().user_info.as_deref())
            } else {
                opt_s(r.uri.user_info.as_deref())
            }
        }
        b"getusername" => read_helper(vm, &uri, Component::Username, ReadMode::NormalizedAscii),
        b"getrawusername" => read_helper(vm, &uri, Component::Username, ReadMode::Raw),
        b"getpassword" => read_helper(vm, &uri, Component::Password, ReadMode::NormalizedAscii),
        b"getrawpassword" => read_helper(vm, &uri, Component::Password, ReadMode::Raw),
        b"gethost" | b"getasciihost" => {
            read_helper(vm, &uri, Component::Host, ReadMode::NormalizedAscii)
        }
        b"getunicodehost" => read_helper(vm, &uri, Component::Host, ReadMode::NormalizedUnicode),
        b"getrawhost" => read_helper(vm, &uri, Component::Host, ReadMode::Raw),
        b"getport" => read_helper(vm, &uri, Component::Port, ReadMode::Raw),
        b"getpath" => read_helper(vm, &uri, Component::Path, ReadMode::NormalizedAscii),
        b"getrawpath" => read_helper(vm, &uri, Component::Path, ReadMode::Raw),
        b"getquery" => read_helper(vm, &uri, Component::Query, ReadMode::NormalizedAscii),
        b"getrawquery" => read_helper(vm, &uri, Component::Query, ReadMode::Raw),
        b"getfragment" => {
            let mode = if is_rfc {
                ReadMode::NormalizedAscii
            } else {
                ReadMode::NormalizedUnicode
            };
            read_helper(vm, &uri, Component::Fragment, mode)
        }
        b"getrawfragment" => read_helper(vm, &uri, Component::Fragment, ReadMode::Raw),
        // Writers
        b"withscheme" => {
            let v = if is_rfc {
                opt_str_arg(args, 0)
            } else {
                Some(str_arg(args, 0))
            };
            write_helper(vm, &o, Component::Scheme, WriteValue::Str(v))
        }
        b"withuserinfo" => write_helper(
            vm,
            &o,
            Component::Username,
            WriteValue::UserInfo(opt_str_arg(args, 0)),
        ),
        b"withusername" => write_helper(
            vm,
            &o,
            Component::Username,
            WriteValue::Str(opt_str_arg(args, 0)),
        ),
        b"withpassword" => write_helper(
            vm,
            &o,
            Component::Password,
            WriteValue::Str(opt_str_arg(args, 0)),
        ),
        b"withhost" => write_helper(
            vm,
            &o,
            Component::Host,
            WriteValue::Str(opt_str_arg(args, 0)),
        ),
        b"withport" => write_helper(
            vm,
            &o,
            Component::Port,
            WriteValue::Long(opt_long_arg(args, 0)),
        ),
        b"withpath" => write_helper(
            vm,
            &o,
            Component::Path,
            WriteValue::Str(Some(str_arg(args, 0))),
        ),
        b"withquery" => write_helper(
            vm,
            &o,
            Component::Query,
            WriteValue::Str(opt_str_arg(args, 0)),
        ),
        b"withfragment" => write_helper(
            vm,
            &o,
            Component::Fragment,
            WriteValue::Str(opt_str_arg(args, 0)),
        ),
        b"equals" => uri_equals(vm, &o, &uri, args),
        b"tostring" => s(&to_string(&uri, Recompose::NormalizedAscii, false)),
        b"torawstring" | b"toasciistring" => s(&to_string(&uri, Recompose::RawAscii, false)),
        b"tounicodestring" => s(&to_string(&uri, Recompose::RawUnicode, false)),
        b"resolve" => {
            let uri_str = str_arg(args, 0);
            let errors_ref = if is_rfc { None } else { args.get(1) };
            instantiate_uri(
                vm,
                class,
                Some(&o),
                &uri_str,
                Some(&o),
                true,
                false,
                errors_ref,
            )
        }
        b"__serialize" => {
            let uri_str = to_string(&uri, Recompose::RawAscii, false);
            let mut first = PhpArray::new();
            first.set(key("uri"), s(&uri_str));
            let mut arr = PhpArray::new();
            arr.push(Value::Array(Rc::new(RefCell::new(first))));
            arr.push(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
            Value::Array(Rc::new(RefCell::new(arr)))
        }
        _ => Value::Null,
    }
}

/// php_uri_property_read_helper()
fn read_helper(_vm: &mut Vm, uri: &ParsedUri, c: Component, mode: ReadMode) -> Value {
    read_component(uri, c, mode)
}

enum WriteValue {
    Str(Option<Vec<u8>>),
    /// withUserInfo() (php_uri_parser_rfc3986_userinfo_write())
    UserInfo(Option<Vec<u8>>),
    Long(Option<i64>),
}

/// php_uri_property_write_helper(): the modified clone of `o`.
fn write_helper(vm: &mut Vm, o: &Obj, c: Component, v: WriteValue) -> Value {
    let line = vm.current_line;
    let Value::Object(new_obj) = vm.clone_value(&Value::Object(o.clone()), line) else {
        return Value::Null;
    };
    if vm.current_exception.is_some() {
        return Value::Null;
    }
    let Some(st) = state_of(&new_obj) else {
        return Value::Null;
    };
    let mut st = st.borrow_mut();
    let Some(uri) = st.uri.as_mut() else {
        return Value::Null;
    };
    let ok = match uri {
        ParsedUri::Rfc3986(r) => {
            // The cached normalized URI is not copied by the clone handler.
            *r.normalized.borrow_mut() = None;
            rfc_write(vm, &mut r.uri, c, v, &new_obj)
        }
        ParsedUri::Whatwg(u) => whatwg_write(vm, u, c, v),
        ParsedUri::PhpUrl(_) => false,
    };
    drop(st);
    if !ok {
        return Value::Null;
    }
    Value::Object(new_obj)
}

/// php_uri_parser_rfc3986_*_write()
fn rfc_write(vm: &mut Vm, u: &mut rfc3986::Uri, c: Component, v: WriteValue, o: &Obj) -> bool {
    use rfc3986::Error as E;
    let (res, what) = match (c, v) {
        (Component::Scheme, WriteValue::Str(v)) => (u.set_scheme(v.as_deref()), "scheme"),
        (Component::Username, WriteValue::UserInfo(v)) => {
            (u.set_user_info(v.as_deref()), "userinfo")
        }
        (Component::Host, WriteValue::Str(v)) => (u.set_host_auto(v.as_deref()), "host"),
        (Component::Port, WriteValue::Long(v)) => {
            let t = v.map(|n| n.to_string().into_bytes());
            (u.set_port_text(t.as_deref()), "port")
        }
        (Component::Path, WriteValue::Str(v)) => {
            let v = v.unwrap_or_default();
            (
                u.set_path(if v.is_empty() { None } else { Some(&v) }),
                "path",
            )
        }
        (Component::Query, WriteValue::Str(v)) => (u.set_query(v.as_deref()), "query"),
        (Component::Fragment, WriteValue::Str(v)) => (u.set_fragment(v.as_deref()), "fragment"),
        (c, _) => {
            // No write handler: zend_readonly_property_modification_error_ex()
            let msg = format!(
                "Cannot modify readonly property {}::${}",
                class_name_of(o),
                c.name()
            );
            throw(vm, b"Error", &msg);
            return false;
        }
    };
    let msg = match res {
        Ok(()) => return true,
        Err(E::Syntax) => format!("The specified {} is malformed", what),
        Err(E::SetUserInfoHostNotSet) => "Cannot set a userinfo without having a host".to_string(),
        Err(E::SetHostPortSet) => "Cannot remove the host from a URI that has a port".to_string(),
        Err(E::SetHostUserInfoSet) => {
            "Cannot remove the host from a URI that has a userinfo".to_string()
        }
        Err(E::SetPortHostNotSet) => "Cannot set a port without having a host".to_string(),
        Err(E::AddBaseRelBase) => {
            throw(vm, URI_ERROR, &format!("Failed to update the {}", what));
            return false;
        }
    };
    throw(vm, INVALID_URI_EXCEPTION, &msg);
    false
}

/// php_uri_parser_whatwg_*_write()
fn whatwg_write(vm: &mut Vm, u: &mut whatwg::Url, c: Component, v: WriteValue) -> bool {
    // zval_string_or_null_to_lexbor_str() / zval_long_or_null_to_lexbor_str()
    let data: Vec<u8> = match v {
        WriteValue::Str(v) | WriteValue::UserInfo(v) => v.unwrap_or_default(),
        WriteValue::Long(n) => n.map(|n| n.to_string().into_bytes()).unwrap_or_default(),
    };
    let mut p = whatwg::Parser {
        log: LEXBOR_LOG.with(|l| std::mem::take(&mut *l.borrow_mut())),
    };
    let (res, what) = match c {
        Component::Scheme => (p.protocol_set(u, &data), "scheme"),
        Component::Username => {
            whatwg::username_set(u, &data);
            (Ok(()), "username")
        }
        Component::Password => {
            whatwg::password_set(u, &data);
            (Ok(()), "password")
        }
        Component::Host => (p.hostname_set(u, &data), "host"),
        Component::Port => (p.port_set(u, &data), "port"),
        Component::Path => (p.pathname_set(u, &data), "path"),
        Component::Query => (p.search_set(u, &data), "query string"),
        Component::Fragment => (p.hash_set(u, &data), "fragment"),
    };
    LEXBOR_LOG.with(|l| *l.borrow_mut() = p.log);
    if res.is_err() {
        // throw_invalid_url_exception_during_write()
        let (err, reason) = fill_errors(vm);
        throw_invalid_url(vm, &malformed_message(what, reason), err);
        return false;
    }
    true
}

/// uri_equals()
fn uri_equals(vm: &mut Vm, o: &Obj, this_uri: &ParsedUri, args: &[Value]) -> Value {
    let Some(Value::Object(that)) = args.first().map(|v| v.deref()) else {
        return Value::False;
    };
    let this_cls = o.borrow().class_name.to_ascii_lowercase();
    let that_cls = that.borrow().class_name.to_ascii_lowercase();
    if this_cls != that_cls
        && !vm.class_extends(&this_cls, &that_cls)
        && !vm.class_extends(&that_cls, &this_cls)
    {
        return Value::False;
    }
    let exclude_fragment = match args.get(1).map(|v| v.deref()) {
        Some(Value::Object(mode)) => {
            let case = mode.borrow().get_property(b"name");
            !matches!(case, Value::String(ref s) if s.as_bytes() == b"IncludeFragment")
        }
        _ => true,
    };
    let Some((_, that_uri)) = uri_of(&that) else {
        return Value::False;
    };
    let a = to_string(this_uri, Recompose::NormalizedAscii, exclude_fragment);
    let b = to_string(&that_uri, Recompose::NormalizedAscii, exclude_fragment);
    if a == b { Value::True } else { Value::False }
}

/// uri_unserialize()
fn uri_unserialize(vm: &mut Vm, o: &Obj, args: &[Value]) -> Value {
    let class = class_name_of(o);
    let invalid = |vm: &mut Vm| {
        let msg = format!("Invalid serialization data for {} object", class);
        throw(vm, b"Exception", &msg);
        Value::Null
    };
    if state_of(o).is_none() {
        create_object(vm, o);
    }
    let Some(st) = state_of(o) else {
        return invalid(vm);
    };
    if st.borrow().uri.is_some() {
        let msg = format!("Cannot modify readonly object of class {}", class);
        throw(vm, b"Error", &msg);
        return invalid(vm);
    }
    let Some(Value::Array(data)) = args.first().map(|v| v.deref()) else {
        return invalid(vm);
    };
    let data = data.borrow().clone();
    if data.len() != 2 {
        return invalid(vm);
    }
    let Some(Value::Array(first)) = data.get(&ArrayKey::Int(0)).map(|v| v.deref()) else {
        return invalid(vm);
    };
    let first = first.borrow().clone();
    if first.len() != 1 {
        return invalid(vm);
    }
    let Some(Value::String(uri_str)) = first.get(&key("uri")).map(|v| v.deref()) else {
        return invalid(vm);
    };
    let parser = st.borrow().parser;
    let Some(parsed) = parser_parse(vm, parser, uri_str.as_bytes(), None, None, true) else {
        return invalid(vm);
    };
    st.borrow_mut().uri = Some(parsed);
    let Some(Value::Array(second)) = data.get(&ArrayKey::Int(1)).map(|v| v.deref()) else {
        return invalid(vm);
    };
    if !second.borrow().is_empty() {
        return invalid(vm);
    }
    Value::Null
}

// ----------------------------------------------------------------------
// The parser registry API (php_uri.h) for other extensions

/// php_uri_get_parser(): None for the parse_url() parser name "" handled
/// by the caller; Some for the URI classes' names (case-sensitive).
pub fn get_parser(name: Option<&[u8]>) -> Option<Parser> {
    match name {
        None => Some(Parser::PhpParseUrl),
        Some(b"Uri\\Rfc3986\\Uri") => Some(Parser::Rfc3986),
        Some(b"Uri\\WhatWg\\Url") => Some(Parser::Whatwg),
        Some(b"parse_url") => Some(Parser::PhpParseUrl),
        _ => None,
    }
}

/// php_uri_parse() + the component reads of php_uri_parse_to_struct():
/// the eight components in `mode` (None: the parse failed; an exception
/// was thrown unless `silent`).
pub fn parse_components(
    vm: &mut Vm,
    parser: Parser,
    input: &[u8],
    mode: ReadMode,
    silent: bool,
) -> Option<[Value; 8]> {
    let parsed = parser_parse(vm, parser, input, None, None, silent)?;
    Some(ALL_COMPONENTS.map(|c| read_component(&parsed, c, mode)))
}
