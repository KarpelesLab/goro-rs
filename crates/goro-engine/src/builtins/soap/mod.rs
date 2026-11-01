//! ext/soap (PHP manual: SOAP): `SoapClient`, `SoapServer`, `SoapFault`,
//! `SoapHeader`, `SoapParam`, `SoapVar`, `use_soap_error_handler()`,
//! `is_soap_fault()`, the `SOAP_*`/`XSD_*` constants and the `soap.*` ini
//! settings, written from the manual, the SOAP 1.1/1.2, WSDL 1.1 and XML
//! Schema specifications, the ext/soap tests' expected output and
//! observations on PHP 8.5.
//!
//! - [`sdl`]: the WSDL and its schemas.
//! - [`encode`] / [`decode`]: PHP values to SOAP XML and back.
//! - [`client`] / [`server`]: the two ends of a call.
//! - [`xmlout`] / [`xmlin`]: writing and reading XML.

pub mod client;
pub mod decode;
pub mod describe;
pub mod encode;
pub mod http;
pub mod sdl;
pub mod server;
pub mod types;
pub mod xmlin;
pub mod xmlout;

use std::cell::RefCell;
use std::rc::Rc;

use goro_runtime::{ObjRef, PhpStr, Value};

use crate::func::{Args, NativeFn};
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Unwind, Vm};
use encode::TypeMapEntry;

const CLASSES: &[(&str, &str)] = &[
    (
        "soapparam",
        r#"<?php class SoapParam {
    public string $param_name;
    public mixed $param_data;
    public function __construct(mixed $data, string $name) {}
}"#,
    ),
    (
        "soapheader",
        r#"<?php class SoapHeader {
    public string $namespace;
    public string $name;
    public mixed $data = null;
    public bool $mustUnderstand;
    public string|int|null $actor;
    public function __construct(string $namespace, string $name, mixed $data = null, bool $mustUnderstand = false, string|int|null $actor = null) {}
}"#,
    ),
    (
        "soapfault",
        r#"<?php class SoapFault extends Exception {
    public string $faultstring;
    public ?string $faultcode = null;
    public ?string $faultcodens = null;
    public ?string $faultactor = null;
    public mixed $detail = null;
    public ?string $_name = null;
    public mixed $headerfault = null;
    public string $lang = "";
    public function __construct(array|string|null $code, string $string, ?string $actor = null, mixed $details = null, ?string $name = null, mixed $headerFault = null, string $lang = "") {}
    public function __toString(): string {}
}"#,
    ),
    (
        "soapvar",
        r#"<?php class SoapVar {
    public int $enc_type;
    public mixed $enc_value = null;
    public ?string $enc_stype = null;
    public ?string $enc_ns = null;
    public ?string $enc_name = null;
    public ?string $enc_namens = null;
    public function __construct(mixed $data, ?int $encoding, ?string $typeName = null, ?string $typeNamespace = null, ?string $nodeName = null, ?string $nodeNamespace = null) {}
}"#,
    ),
    (
        "soapserver",
        r#"<?php class SoapServer {
    private ?SoapFault $__soap_fault = null;
    public function __construct(?string $wsdl, array $options = []) {}
    public function fault(string $code, string $string, string $actor = "", mixed $details = null, string $name = "", string $lang = ""): void {}
    public function addSoapHeader(SoapHeader $header): void {}
    public function setPersistence(int $mode): void {}
    public function setClass(string $class, mixed ...$args): void {}
    public function setObject(object $object): void {}
    public function getFunctions(): array {}
    public function addFunction($functions): void {}
    public function handle(?string $request = null): void {}
    public function __getLastResponse(): ?string {}
}"#,
    ),
    (
        "soapclient",
        r#"<?php class SoapClient {
    private ?string $uri = null;
    private ?int $style = null;
    private ?int $use = null;
    private ?string $location = null;
    private bool $trace = false;
    private ?int $compression = null;
    private ?Soap\Sdl $sdl = null;
    private ?array $typemap = null;
    private $httpsocket = null;
    private ?Soap\Url $httpurl = null;
    private ?string $_login = null;
    private ?string $_password = null;
    private bool $_use_digest = false;
    private ?string $_digest = null;
    private ?string $_proxy_host = null;
    private ?int $_proxy_port = null;
    private ?string $_proxy_login = null;
    private ?string $_proxy_password = null;
    private bool $_exceptions = true;
    private ?string $_encoding = null;
    private ?array $_classmap = null;
    private ?int $_features = null;
    private int $_connection_timeout = 0;
    private $_stream_context = null;
    private ?string $_user_agent = null;
    private bool $_keep_alive = true;
    private ?int $_ssl_method = null;
    private int $_soap_version;
    private ?int $_use_proxy = null;
    private array $_cookies = [];
    private ?array $__default_headers = null;
    private ?SoapFault $__soap_fault = null;
    private ?string $__last_request = null;
    private ?string $__last_response = null;
    private ?string $__last_request_headers = null;
    private ?string $__last_response_headers = null;
    public function __construct(?string $wsdl, array $options = []) {}
    public function __call(string $name, array $args): mixed {}
    public function __soapCall(string $name, array $args, ?array $options = null, $inputHeaders = null, &$outputHeaders = null): mixed {}
    public function __getFunctions(): ?array {}
    public function __getTypes(): ?array {}
    public function __getLastRequest(): ?string {}
    public function __getLastResponse(): ?string {}
    public function __getLastRequestHeaders(): ?string {}
    public function __getLastResponseHeaders(): ?string {}
    public function __doRequest(string $request, string $location, string $action, int $version, bool $oneWay = false, ?string $uriParserClass = null): ?string {}
    public function __setCookie(string $name, ?string $value = null): void {}
    public function __getCookies(): array {}
    public function __setSoapHeaders($headers = null): bool {}
    public function __setLocation(?string $location = null): ?string {}
}"#,
    ),
    (
        "soap\\url",
        r#"<?php namespace Soap; final class Url { public function __construct() {} }"#,
    ),
    (
        "soap\\sdl",
        r#"<?php namespace Soap; final class Sdl { public function __construct() {} }"#,
    ),
];

static METHODS: &[(&str, NativeFn)] = &[
    ("soapparam::__construct", soapparam_construct),
    ("soapheader::__construct", soapheader_construct),
    ("soapfault::__construct", soapfault_construct),
    ("soapfault::__tostring", soapfault_tostring),
    ("soapvar::__construct", soapvar_construct),
    ("soapserver::__construct", server::construct),
    ("soapserver::fault", server::fault),
    ("soapserver::addsoapheader", server::add_soap_header),
    ("soapserver::setpersistence", server::set_persistence),
    ("soapserver::setclass", server::set_class),
    ("soapserver::setobject", server::set_object),
    ("soapserver::getfunctions", server::get_functions),
    ("soapserver::addfunction", server::add_function),
    ("soapserver::handle", server::handle),
    ("soapserver::__getlastresponse", server::get_last_response),
    ("soapclient::__construct", client::construct),
    ("soapclient::__call", client::call),
    ("soapclient::__soapcall", client::soap_call),
    ("soapclient::__getfunctions", client::get_functions),
    ("soapclient::__gettypes", client::get_types),
    ("soapclient::__getlastrequest", client::get_last_request),
    ("soapclient::__getlastresponse", client::get_last_response),
    (
        "soapclient::__getlastrequestheaders",
        client::get_last_request_headers,
    ),
    (
        "soapclient::__getlastresponseheaders",
        client::get_last_response_headers,
    ),
    ("soapclient::__dorequest", client::do_request),
    ("soapclient::__setcookie", client::set_cookie),
    ("soapclient::__getcookies", client::get_cookies),
    ("soapclient::__setsoapheaders", client::set_soap_headers),
    ("soapclient::__setlocation", client::set_location),
    ("soap\\url::__construct", no_construct_url),
    ("soap\\sdl::__construct", no_construct_sdl),
];

const CONSTANTS: &[(&str, ConstVal)] = &[
    ("SOAP_1_1", ConstVal::Int(1)),
    ("SOAP_1_2", ConstVal::Int(2)),
    ("SOAP_PERSISTENCE_SESSION", ConstVal::Int(1)),
    ("SOAP_PERSISTENCE_REQUEST", ConstVal::Int(2)),
    ("SOAP_FUNCTIONS_ALL", ConstVal::Int(999)),
    ("SOAP_ENCODED", ConstVal::Int(1)),
    ("SOAP_LITERAL", ConstVal::Int(2)),
    ("SOAP_RPC", ConstVal::Int(1)),
    ("SOAP_DOCUMENT", ConstVal::Int(2)),
    ("SOAP_ACTOR_NEXT", ConstVal::Int(1)),
    ("SOAP_ACTOR_NONE", ConstVal::Int(2)),
    ("SOAP_ACTOR_UNLIMATERECEIVER", ConstVal::Int(3)),
    ("SOAP_COMPRESSION_ACCEPT", ConstVal::Int(32)),
    ("SOAP_COMPRESSION_GZIP", ConstVal::Int(0)),
    ("SOAP_COMPRESSION_DEFLATE", ConstVal::Int(16)),
    ("SOAP_AUTHENTICATION_BASIC", ConstVal::Int(0)),
    ("SOAP_AUTHENTICATION_DIGEST", ConstVal::Int(1)),
    ("UNKNOWN_TYPE", ConstVal::Int(999998)),
    ("XSD_STRING", ConstVal::Int(101)),
    ("XSD_BOOLEAN", ConstVal::Int(102)),
    ("XSD_DECIMAL", ConstVal::Int(103)),
    ("XSD_FLOAT", ConstVal::Int(104)),
    ("XSD_DOUBLE", ConstVal::Int(105)),
    ("XSD_DURATION", ConstVal::Int(106)),
    ("XSD_DATETIME", ConstVal::Int(107)),
    ("XSD_TIME", ConstVal::Int(108)),
    ("XSD_DATE", ConstVal::Int(109)),
    ("XSD_GYEARMONTH", ConstVal::Int(110)),
    ("XSD_GYEAR", ConstVal::Int(111)),
    ("XSD_GMONTHDAY", ConstVal::Int(112)),
    ("XSD_GDAY", ConstVal::Int(113)),
    ("XSD_GMONTH", ConstVal::Int(114)),
    ("XSD_HEXBINARY", ConstVal::Int(115)),
    ("XSD_BASE64BINARY", ConstVal::Int(116)),
    ("XSD_ANYURI", ConstVal::Int(117)),
    ("XSD_QNAME", ConstVal::Int(118)),
    ("XSD_NOTATION", ConstVal::Int(119)),
    ("XSD_NORMALIZEDSTRING", ConstVal::Int(120)),
    ("XSD_TOKEN", ConstVal::Int(121)),
    ("XSD_LANGUAGE", ConstVal::Int(122)),
    ("XSD_NMTOKEN", ConstVal::Int(123)),
    ("XSD_NAME", ConstVal::Int(124)),
    ("XSD_NCNAME", ConstVal::Int(125)),
    ("XSD_ID", ConstVal::Int(126)),
    ("XSD_IDREF", ConstVal::Int(127)),
    ("XSD_IDREFS", ConstVal::Int(128)),
    ("XSD_ENTITY", ConstVal::Int(129)),
    ("XSD_ENTITIES", ConstVal::Int(130)),
    ("XSD_INTEGER", ConstVal::Int(131)),
    ("XSD_NONPOSITIVEINTEGER", ConstVal::Int(132)),
    ("XSD_NEGATIVEINTEGER", ConstVal::Int(133)),
    ("XSD_LONG", ConstVal::Int(134)),
    ("XSD_INT", ConstVal::Int(135)),
    ("XSD_SHORT", ConstVal::Int(136)),
    ("XSD_BYTE", ConstVal::Int(137)),
    ("XSD_NONNEGATIVEINTEGER", ConstVal::Int(138)),
    ("XSD_UNSIGNEDLONG", ConstVal::Int(139)),
    ("XSD_UNSIGNEDINT", ConstVal::Int(140)),
    ("XSD_UNSIGNEDSHORT", ConstVal::Int(141)),
    ("XSD_UNSIGNEDBYTE", ConstVal::Int(142)),
    ("XSD_POSITIVEINTEGER", ConstVal::Int(143)),
    ("XSD_NMTOKENS", ConstVal::Int(144)),
    ("XSD_ANYTYPE", ConstVal::Int(145)),
    ("XSD_ANYXML", ConstVal::Int(147)),
    ("APACHE_MAP", ConstVal::Int(200)),
    ("SOAP_ENC_OBJECT", ConstVal::Int(301)),
    ("SOAP_ENC_ARRAY", ConstVal::Int(300)),
    ("XSD_1999_TIMEINSTANT", ConstVal::Int(401)),
    (
        "XSD_NAMESPACE",
        ConstVal::Str("http://www.w3.org/2001/XMLSchema"),
    ),
    (
        "XSD_1999_NAMESPACE",
        ConstVal::Str("http://www.w3.org/1999/XMLSchema"),
    ),
    ("SOAP_SINGLE_ELEMENT_ARRAYS", ConstVal::Int(1)),
    ("SOAP_WAIT_ONE_WAY_CALLS", ConstVal::Int(2)),
    ("SOAP_USE_XSI_ARRAY_TYPE", ConstVal::Int(4)),
    ("WSDL_CACHE_NONE", ConstVal::Int(0)),
    ("WSDL_CACHE_DISK", ConstVal::Int(1)),
    ("WSDL_CACHE_MEMORY", ConstVal::Int(2)),
    ("WSDL_CACHE_BOTH", ConstVal::Int(3)),
    ("SOAP_SSL_METHOD_TLS", ConstVal::Int(0)),
    ("SOAP_SSL_METHOD_SSLv2", ConstVal::Int(1)),
    ("SOAP_SSL_METHOD_SSLv3", ConstVal::Int(2)),
    ("SOAP_SSL_METHOD_SSLv23", ConstVal::Int(3)),
];

pub static MODULE: Module = Module {
    name: "soap",
    functions: &[
        (
            "function use_soap_error_handler(bool $enable = true): bool",
            use_soap_error_handler,
        ),
        ("function is_soap_fault(mixed $object): bool", is_soap_fault),
    ],
    classes: CLASSES,
    methods: METHODS,
    hooks: &[
        ("soapserver", server::hooks),
        ("soapclient", client::hooks),
        ("soap\\sdl", sdl_hooks),
        ("soap\\url", url_hooks),
    ],
    constants: CONSTANTS,
    ini: &[
        ("soap.wsdl_cache_enabled", "1"),
        ("soap.wsdl_cache_dir", "/tmp"),
        ("soap.wsdl_cache_ttl", "86400"),
        ("soap.wsdl_cache", "1"),
        ("soap.wsdl_cache_limit", "5"),
    ],
    shutdown: Some(shutdown),
    ..Module::EMPTY
};

fn shutdown(_vm: &mut Vm) {
    ERR_MODE.with(|m| m.borrow_mut().clear());
    SERVER_ERRORS.with(|m| m.set(false));
}

fn sdl_hooks() -> crate::class::NativeHooks {
    crate::class::NativeHooks {
        create: Some(|vm, o| {
            let cls = vm.class_of(o);
            cls.flags
                .set(cls.flags.get() | crate::class::cflags::NOT_CLONEABLE);
        }),
        ..Default::default()
    }
}

fn url_hooks() -> crate::class::NativeHooks {
    sdl_hooks()
}

// ------------------------------------------------------------ shared state

/// The options encoding and decoding read.
#[derive(Clone, Default)]
pub struct SoapState {
    pub features: i64,
    pub encoding: Option<String>,
    pub classmap: Vec<(String, String)>,
    pub typemap: Rc<Vec<TypeMapEntry>>,
}

/// How a SOAP error is reported (PHP manual: SoapClient `exceptions`,
/// use_soap_error_handler()).
#[derive(Clone, Debug)]
pub enum ErrMode {
    /// Thrown as a SoapFault with this fault code.
    Throw(&'static str),
    /// A fatal error.
    Fatal,
}

thread_local! {
    static ERR_MODE: RefCell<Vec<ErrMode>> = const { RefCell::new(Vec::new()) };
    /// The last fault raised for a SOAP error (by address).
    static LAST_INTERNAL: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// use_soap_error_handler()'s setting.
    static SERVER_ERRORS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn push_mode(m: ErrMode) {
    ERR_MODE.with(|s| s.borrow_mut().push(m));
}

pub fn pop_mode() {
    ERR_MODE.with(|s| {
        s.borrow_mut().pop();
    });
}

/// Reports "SOAP-ERROR: <msg>" the way the current context wants it.
pub fn soap_error(vm: &mut Vm, msg: &str) -> Unwind {
    let full = format!("SOAP-ERROR: {msg}");
    let mode = ERR_MODE.with(|s| s.borrow().last().cloned());
    match mode {
        Some(ErrMode::Throw(code)) => match make_fault(vm, code, &full) {
            Ok(f) => {
                LAST_INTERNAL.with(|l| l.set(f.obj() as *const _ as usize));
                Unwind::Throw(f)
            }
            Err(e) => e,
        },
        _ => vm.fatal(&full),
    }
}

/// Whether a fault object was raised for a SOAP error (not by user code).
pub fn is_internal_fault(f: &ObjRef) -> bool {
    LAST_INTERNAL.with(|l| l.get() == f.obj() as *const _ as usize)
}

/// A plain error ("Class::method(): msg") in the current context.
pub fn soap_php_error(vm: &mut Vm, a: &Args, msg: &str) -> Unwind {
    let full = format!("{}(): {}", a.fname(), msg);
    let mode = ERR_MODE.with(|s| s.borrow().last().cloned());
    match mode {
        Some(ErrMode::Throw(code)) => match make_fault(vm, code, &full) {
            Ok(f) => Unwind::Throw(f),
            Err(e) => e,
        },
        _ => vm.fatal(&full),
    }
}

/// Creates a SoapFault object as `new SoapFault($code, $string)` would.
pub fn make_fault(vm: &mut Vm, code: &str, string: &str) -> R<ObjRef> {
    let cls = vm
        .find_class(b"SoapFault", false)?
        .expect("SoapFault declared");
    let o = vm.instantiate(&cls)?;
    init_fault(
        vm,
        &o,
        Some((None, code.to_string())),
        string,
        None,
        Value::Null,
        None,
        Value::Null,
        "",
    );
    Ok(o)
}

/// Sets a SoapFault's properties.
#[allow(clippy::too_many_arguments)]
pub fn init_fault(
    vm: &mut Vm,
    o: &ObjRef,
    code: Option<(Option<String>, String)>,
    string: &str,
    actor: Option<String>,
    detail: Value,
    name: Option<String>,
    header_fault: Value,
    lang: &str,
) {
    let s = Value::Str(PhpStr::from(string));
    vm.set_prop_raw(o, b"message", s.clone());
    vm.set_prop_raw(o, b"faultstring", s);
    if let Some((ns, c)) = code {
        let ns = ns.or_else(|| {
            matches!(
                c.as_str(),
                "Client" | "Server" | "VersionMismatch" | "MustUnderstand"
            )
            .then(|| xmlout::NS_SOAP11_ENV.to_string())
        });
        vm.set_prop_raw(o, b"faultcode", Value::Str(PhpStr::from(c)));
        if let Some(ns) = ns {
            vm.set_prop_raw(o, b"faultcodens", Value::Str(PhpStr::from(ns)));
        }
    }
    // Every property is (re)set, so a second constructor call starts
    // over (test SoapFault/gh14586.phpt).
    vm.set_prop_raw(
        o,
        b"faultactor",
        actor
            .map(|a| Value::Str(PhpStr::from(a)))
            .unwrap_or(Value::Null),
    );
    vm.set_prop_raw(o, b"detail", detail);
    vm.set_prop_raw(
        o,
        b"_name",
        name.map(|n| Value::Str(PhpStr::from(n)))
            .unwrap_or(Value::Null),
    );
    vm.set_prop_raw(o, b"headerfault", header_fault);
    // The language defaults to "en" (observed on PHP 8.5).
    let l = if lang.is_empty() { "en" } else { lang };
    vm.set_prop_raw(o, b"lang", Value::Str(PhpStr::from(l)));
}

/// Sets a fault's string from bytes (any charset).
pub fn set_fault_string(vm: &mut Vm, o: &ObjRef, s: PhpStr) {
    vm.set_prop_raw(o, b"message", Value::Str(s.clone()));
    vm.set_prop_raw(o, b"faultstring", Value::Str(s));
}

pub fn is_fault_obj(vm: &mut Vm, v: &Value) -> bool {
    match v {
        Value::Object(o) => encode::instance_of(vm, o, "SoapFault"),
        _ => false,
    }
}

pub fn str_prop(vm: &Vm, o: &ObjRef, name: &str) -> Option<String> {
    match vm.get_prop_raw(o, name.as_bytes()) {
        Value::Str(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}

// ------------------------------------------------------------ helpers

/// Converts bytes in charset `enc` to UTF-8.
pub fn charset_to_utf8(enc: &str, s: &[u8]) -> String {
    match goro_xml::encoding::lookup(enc) {
        Some(e) => {
            let d = goro_xml::encoding::decode(s, Some(e));
            String::from_utf8_lossy(&d.text).into_owned()
        }
        None => String::from_utf8_lossy(s).into_owned(),
    }
}

/// Converts UTF-8 text to charset `enc`.
pub fn utf8_to_charset(enc: &str, s: &str) -> Vec<u8> {
    goro_xml::serialize::encode_output(s, Some(enc))
}

pub fn valid_encoding(enc: &str) -> bool {
    goro_xml::encoding::lookup(enc).is_some()
}

pub fn xml_escape(s: &str) -> String {
    let mut out = String::new();
    goro_xml::serialize::escape_text(s, &mut out);
    out
}

unsafe extern "C" {
    fn tzset();
}

/// Local time of a timestamp (TZ as the C library sees it): year, month,
/// day, hour, minute, second, UTC offset in seconds.
pub fn local_time(ts: i64) -> (i64, i64, i64, i64, i64, i64, i64) {
    // SAFETY: localtime_r writes the struct it is given.
    unsafe {
        let t: libc::time_t = ts as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        tzset();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return (1970, 1, 1, 0, 0, 0, 0);
        }
        (
            tm.tm_year as i64 + 1900,
            tm.tm_mon as i64 + 1,
            tm.tm_mday as i64,
            tm.tm_hour as i64,
            tm.tm_min as i64,
            tm.tm_sec as i64,
            tm.tm_gmtoff,
        )
    }
}

// ------------------------------------------------------------ small classes

// Observed on PHP 8.5: "Cannot directly construct Soap\\Sdl".
fn no_construct_url(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.throw("Error", "Cannot directly construct Soap\\Url")
}

fn no_construct_sdl(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.throw("Error", "Cannot directly construct Soap\\Sdl")
}

fn soapparam_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let name = a.str(1);
    if name.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #2 ($name) must not be empty");
    }
    vm.set_prop_raw(&this, b"param_name", Value::Str(name));
    vm.set_prop_raw(&this, b"param_data", a.val(0));
    Ok(Value::Null)
}

fn soapheader_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let ns = a.str(0);
    if ns.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($namespace) must not be empty",
        );
    }
    let name = a.str(1);
    if name.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #2 ($name) must not be empty");
    }
    vm.set_prop_raw(&this, b"namespace", Value::Str(ns));
    vm.set_prop_raw(&this, b"name", Value::Str(name));
    vm.set_prop_raw(&this, b"data", a.val(2));
    vm.set_prop_raw(&this, b"mustUnderstand", Value::Bool(a.bool(3)));
    match a.val(4) {
        Value::Null => {}
        Value::Int(n) => {
            if !(1..=3).contains(&n) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #5 ($actor) must be one of SOAP_ACTOR_NEXT, SOAP_ACTOR_NONE, or SOAP_ACTOR_UNLIMATERECEIVER",
                );
            }
            vm.set_prop_raw(&this, b"actor", Value::Int(n));
        }
        Value::Str(s) => {
            if s.is_empty() {
                return vm.fthrow(a, "ValueError", "Argument #5 ($actor) must not be empty");
            }
            vm.set_prop_raw(&this, b"actor", Value::Str(s));
        }
        other => {
            vm.set_prop_raw(&this, b"actor", other);
        }
    }
    Ok(Value::Null)
}

fn soapfault_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let code = match a.val(0) {
        Value::Null => None,
        Value::Str(s) => {
            if s.is_empty() {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #1 ($code) is not a valid fault code",
                );
            }
            Some((None, s.to_string_lossy()))
        }
        Value::Array(arr) => {
            let vals: Vec<Value> = arr.iter().map(|(_, v)| v.deref()).collect();
            if vals.len() != 2 {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #1 ($code) is not a valid fault code",
                );
            }
            let (Value::Str(ns), Value::Str(c)) = (&vals[0], &vals[1]) else {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #1 ($code) is not a valid fault code",
                );
            };
            Some((Some(ns.to_string_lossy()), c.to_string_lossy()))
        }
        _ => None,
    };
    let raw = a.str(1);
    let string = raw.to_string_lossy();
    let actor = a.opt_str(2).map(|s| s.to_string_lossy());
    let name = a.opt_str(4).map(|s| s.to_string_lossy());
    let lang = a.str(6).to_string_lossy();
    init_fault(
        vm,
        &this,
        code,
        &string,
        actor,
        a.val(3),
        name,
        a.val(5),
        &lang,
    );
    set_fault_string(vm, &this, raw);
    Ok(Value::Null)
}

fn soapfault_tostring(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let code = vm.get_prop_raw(&this, b"faultcode");
    let code = match code {
        Value::Null => String::new(),
        v => vm.to_php_str(&v)?.to_string_lossy(),
    };
    let string = vm.get_prop_raw(&this, b"faultstring");
    let string = vm.to_php_str(&string)?.to_string_lossy();
    let file = vm.get_prop_raw(&this, b"file");
    let file = vm.to_php_str(&file)?.to_string_lossy();
    let line = match vm.get_prop_raw(&this, b"line") {
        Value::Int(n) => n,
        _ => 0,
    };
    let trace = vm.call_method(&this, b"gettraceasstring", vec![])?;
    let trace = vm.to_php_str(&trace)?.to_string_lossy();
    Ok(Value::Str(PhpStr::from(format!(
        "SoapFault exception: [{code}] {string} in {file}:{line}\nStack trace:\n{trace}"
    ))))
}

fn soapvar_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let this = a.this_obj().clone();
    let code = match a.val(1) {
        Value::Null => types::UNKNOWN_TYPE,
        Value::Int(n) => {
            if !types::valid_code(n) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #2 ($encoding) is not a valid encoding",
                );
            }
            n
        }
        _ => types::UNKNOWN_TYPE,
    };
    vm.set_prop_raw(&this, b"enc_type", Value::Int(code));
    vm.set_prop_raw(&this, b"enc_value", a.val(0));
    for (i, p) in [
        (2, &b"enc_stype"[..]),
        (3, b"enc_ns"),
        (4, b"enc_name"),
        (5, b"enc_namens"),
    ] {
        if let Some(s) = a.opt_str(i) {
            vm.set_prop_raw(&this, p, Value::Str(s));
        }
    }
    Ok(Value::Null)
}

fn use_soap_error_handler(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let new = a.bool_or(0, true);
    let old = SERVER_ERRORS.with(|s| s.replace(new));
    Ok(Value::Bool(old))
}

pub fn server_errors_enabled() -> bool {
    SERVER_ERRORS.with(|s| s.get())
}

fn is_soap_fault(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    Ok(Value::Bool(is_fault_obj(vm, &v)))
}
