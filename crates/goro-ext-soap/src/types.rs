//! The data structures of php_sdl.h / php_encoding.h / php_soap.h: the
//! service description (SDL) built from a WSDL, the encoders, and the
//! request-wide SOAP globals.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::object::PhpObject;
use goro_core::value::Value;
use goro_core::vm::Vm;
use indexmap::IndexMap;

use crate::xml::Node;

pub type Obj = Rc<RefCell<PhpObject>>;
pub type Arr = Rc<RefCell<PhpArray>>;

/// A SOAP error was raised (zend_bailout() in C): an exception is pending,
/// the script is exiting, or a fatal error was emitted.
#[derive(Debug, Clone, Copy)]
pub struct Bail;

pub type SR<T> = Result<T, Bail>;

// ---------------------------------------------------------------------------
// HashTable

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum HKey {
    Int(i64),
    Str(String),
}

/// An ordered hash table with string and integer keys (zend HashTable as
/// the SOAP structures use it).
#[derive(Debug)]
pub struct Ht<V> {
    pub map: IndexMap<HKey, V>,
    next: i64,
}

impl<V> Default for Ht<V> {
    fn default() -> Self {
        Ht {
            map: IndexMap::new(),
            next: 0,
        }
    }
}

impl<V: Clone> Clone for Ht<V> {
    fn clone(&self) -> Self {
        Ht {
            map: self.map.clone(),
            next: self.next,
        }
    }
}

impl<V> Ht<V> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    pub fn find(&self, key: &str) -> Option<&V> {
        self.map.get(&HKey::Str(key.to_string()))
    }
    pub fn find_idx(&self, i: i64) -> Option<&V> {
        self.map.get(&HKey::Int(i))
    }
    pub fn exists(&self, key: &str) -> bool {
        self.map.contains_key(&HKey::Str(key.to_string()))
    }
    /// zend_hash_str_add: false when the key already exists.
    pub fn add(&mut self, key: &str, v: V) -> bool {
        let k = HKey::Str(key.to_string());
        if self.map.contains_key(&k) {
            return false;
        }
        self.map.insert(k, v);
        true
    }
    /// zend_hash_str_update
    pub fn update(&mut self, key: &str, v: V) {
        self.map.insert(HKey::Str(key.to_string()), v);
    }
    /// zend_hash_next_index_insert
    pub fn push(&mut self, v: V) {
        let k = self.next;
        self.next += 1;
        self.map.insert(HKey::Int(k), v);
    }
    pub fn del_idx(&mut self, i: i64) {
        self.map.shift_remove(&HKey::Int(i));
    }
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.map.values()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&HKey, &V)> {
        self.map.iter()
    }
    pub fn first(&self) -> Option<&V> {
        self.map.get_index(0).map(|(_, v)| v)
    }
    pub fn get_index(&self, i: usize) -> Option<(&HKey, &V)> {
        self.map.get_index(i)
    }
}

// ---------------------------------------------------------------------------
// Encoders

pub type EncRef = Rc<Encoder>;
pub type TypeRef = Rc<RefCell<SdlType>>;
pub type AttrRef = Rc<RefCell<SdlAttribute>>;
pub type ModelRef = Rc<RefCell<ContentModel>>;
pub type ParamRef = Rc<SdlParam>;
pub type FuncRef = Rc<SdlFunction>;
pub type BindingRef = Rc<SdlBinding>;
pub type HeaderRef = Rc<SoapBindingHeader>;
pub type SdlRef = Rc<RefCell<Sdl>>;

/// to_xml(type, data, style, parent): data None is a C NULL zval pointer.
pub type ToXml = fn(&mut Vm, &EncRef, Option<&Value>, i32, Node) -> SR<Node>;
/// to_zval(type, node)
pub type ToZval = fn(&mut Vm, &EncRef, Option<Node>) -> SR<Value>;

#[derive(Debug, Default)]
pub struct Mapping {
    pub to_xml: Option<Value>,
    pub to_zval: Option<Value>,
}

#[derive(Debug, Default, Clone)]
pub struct EncDetails {
    pub type_: i64,
    pub type_str: Option<String>,
    pub ns: Option<String>,
    pub clark: Option<String>,
    pub sdl_type: Option<TypeRef>,
    pub map: Option<Rc<Mapping>>,
}

pub struct Encoder {
    pub d: RefCell<EncDetails>,
    pub to_xml: Cell<ToXml>,
    pub to_zval: Cell<ToZval>,
}

impl std::fmt::Debug for Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let d = self.d.borrow();
        write!(f, "Encoder({}, {:?}, {:?})", d.type_, d.ns, d.type_str)
    }
}

impl Encoder {
    pub fn new(d: EncDetails, to_xml: ToXml, to_zval: ToZval) -> EncRef {
        Rc::new(Encoder {
            d: RefCell::new(d),
            to_xml: Cell::new(to_xml),
            to_zval: Cell::new(to_zval),
        })
    }
    pub fn type_(&self) -> i64 {
        self.d.borrow().type_
    }
    pub fn type_str(&self) -> Option<String> {
        self.d.borrow().type_str.clone()
    }
    pub fn ns(&self) -> Option<String> {
        self.d.borrow().ns.clone()
    }
    pub fn sdl_type(&self) -> Option<TypeRef> {
        self.d.borrow().sdl_type.clone()
    }
    pub fn map(&self) -> Option<Rc<Mapping>> {
        self.d.borrow().map.clone()
    }
}

pub fn same_enc(a: &EncRef, b: &EncRef) -> bool {
    Rc::ptr_eq(a, b)
}

pub fn opt_same_enc(a: Option<&EncRef>, b: Option<&EncRef>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// SDL

pub const XSD_TYPEKIND_SIMPLE: u8 = 0;
pub const XSD_TYPEKIND_LIST: u8 = 1;
pub const XSD_TYPEKIND_UNION: u8 = 2;
pub const XSD_TYPEKIND_COMPLEX: u8 = 3;
pub const XSD_TYPEKIND_RESTRICTION: u8 = 4;
pub const XSD_TYPEKIND_EXTENSION: u8 = 5;

pub const XSD_CONTENT_ELEMENT: u8 = 0;
pub const XSD_CONTENT_SEQUENCE: u8 = 1;
pub const XSD_CONTENT_ALL: u8 = 2;
pub const XSD_CONTENT_CHOICE: u8 = 3;
pub const XSD_CONTENT_GROUP_REF: u8 = 4;
pub const XSD_CONTENT_GROUP: u8 = 5;
pub const XSD_CONTENT_ANY: u8 = 6;

pub const XSD_USE_DEFAULT: u8 = 0;
pub const XSD_USE_OPTIONAL: u8 = 1;
pub const XSD_USE_PROHIBITED: u8 = 2;
pub const XSD_USE_REQUIRED: u8 = 3;

pub const XSD_FORM_DEFAULT: u8 = 0;
pub const XSD_FORM_QUALIFIED: u8 = 1;
pub const XSD_FORM_UNQUALIFIED: u8 = 2;

pub const BINDING_SOAP: i32 = 1;
pub const BINDING_HTTP: i32 = 2;

pub const SOAP_ENCODING_DEFAULT: i32 = 0;
pub const SOAP_ENCODING_1_1: i32 = 1;
pub const SOAP_ENCODING_1_2: i32 = 2;

#[derive(Debug, Clone, Default)]
pub struct RestrictionInt {
    pub value: i64,
    pub fixed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RestrictionChar {
    pub value: String,
    pub fixed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Restrictions {
    pub enumeration: Option<Ht<RestrictionChar>>,
    pub min_exclusive: Option<RestrictionInt>,
    pub min_inclusive: Option<RestrictionInt>,
    pub max_exclusive: Option<RestrictionInt>,
    pub max_inclusive: Option<RestrictionInt>,
    pub total_digits: Option<RestrictionInt>,
    pub fraction_digits: Option<RestrictionInt>,
    pub length: Option<RestrictionInt>,
    pub min_length: Option<RestrictionInt>,
    pub max_length: Option<RestrictionInt>,
    pub white_space: Option<RestrictionChar>,
    pub pattern: Option<RestrictionChar>,
}

#[derive(Debug, Clone)]
pub enum ModelU {
    None,
    Element(TypeRef),
    Group(TypeRef),
    Content(Vec<ModelRef>),
    GroupRef(String),
}

#[derive(Debug, Clone)]
pub struct ContentModel {
    pub kind: u8,
    pub min_occurs: i64,
    pub max_occurs: i64,
    pub u: ModelU,
}

impl ContentModel {
    pub fn new(kind: u8, u: ModelU) -> ModelRef {
        Rc::new(RefCell::new(ContentModel {
            kind,
            min_occurs: 1,
            max_occurs: 1,
            u,
        }))
    }
    pub fn content(&self) -> Vec<ModelRef> {
        match &self.u {
            ModelU::Content(v) => v.clone(),
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct SdlType {
    pub kind: u8,
    pub nillable: bool,
    pub name: Option<String>,
    pub namens: Option<String>,
    pub elements: Option<Ht<TypeRef>>,
    pub attributes: Option<Ht<AttrRef>>,
    pub restrictions: Option<Restrictions>,
    pub encode: Option<EncRef>,
    pub model: Option<ModelRef>,
    pub def: Option<String>,
    pub fixed: Option<String>,
    pub ref_: Option<String>,
    pub form: u8,
}

pub fn new_type(t: SdlType) -> TypeRef {
    Rc::new(RefCell::new(t))
}

#[derive(Debug, Clone, Default)]
pub struct ExtraAttr {
    pub ns: Option<String>,
    pub val: String,
}

#[derive(Debug, Clone, Default)]
pub struct SdlAttribute {
    pub name: Option<String>,
    pub namens: Option<String>,
    pub ref_: Option<String>,
    pub def: Option<String>,
    pub fixed: Option<String>,
    pub form: u8,
    pub use_: u8,
    pub extra: Option<Ht<ExtraAttr>>,
    pub encode: Option<EncRef>,
}

#[derive(Debug, Clone, Default)]
pub struct SdlParam {
    pub order: i64,
    pub element: Option<TypeRef>,
    pub encode: Option<EncRef>,
    pub param_name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SoapBindingFault {
    pub ns: Option<String>,
    pub use_: i32,
    pub encoding_style: i32,
}

#[derive(Debug, Clone, Default)]
pub struct SdlFault {
    pub name: String,
    pub details: Vec<ParamRef>,
    pub binding: Option<SoapBindingFault>,
}

#[derive(Debug, Default)]
pub struct SoapBindingHeader {
    pub name: String,
    pub ns: Option<String>,
    pub use_: i32,
    pub encoding_style: i32,
    pub element: Option<TypeRef>,
    pub encode: Option<EncRef>,
    pub headerfaults: Option<Ht<HeaderRef>>,
}

#[derive(Debug, Default, Clone)]
pub struct SoapBindingBody {
    pub ns: Option<String>,
    pub use_: i32,
    pub encoding_style: i32,
    pub headers: Option<Ht<HeaderRef>>,
}

#[derive(Debug, Default, Clone)]
pub struct SoapBindingFunction {
    pub soap_action: Option<String>,
    pub style: i32,
    pub input: SoapBindingBody,
    pub output: SoapBindingBody,
}

#[derive(Debug, Default, Clone)]
pub struct SdlFunction {
    pub function_name: String,
    pub request_name: Option<String>,
    pub response_name: Option<String>,
    pub request_params: Option<Vec<ParamRef>>,
    pub response_params: Option<Vec<ParamRef>>,
    pub binding: Option<BindingRef>,
    pub binding_attrs: Option<SoapBindingFunction>,
    pub faults: Option<Ht<Rc<SdlFault>>>,
}

impl SdlFunction {
    pub fn is_soap(&self) -> bool {
        self.binding
            .as_ref()
            .is_some_and(|b| b.binding_type == BINDING_SOAP)
    }
    /// The SOAP binding attributes of a function with a SOAP binding.
    pub fn soap(&self) -> Option<&SoapBindingFunction> {
        if self.is_soap() {
            self.binding_attrs.as_ref()
        } else {
            None
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct SoapBinding {
    pub style: i32,
    pub transport: i32,
}

#[derive(Debug, Default)]
pub struct SdlBinding {
    pub name: String,
    pub location: String,
    pub binding_type: i32,
    pub soap: Option<SoapBinding>,
}

#[derive(Debug, Default)]
pub struct Sdl {
    pub functions: Ht<FuncRef>,
    pub types: Option<Ht<TypeRef>>,
    pub elements: Option<Ht<TypeRef>>,
    pub encoders: Option<Ht<EncRef>>,
    pub bindings: Option<Ht<BindingRef>>,
    pub requests: Option<Ht<FuncRef>>,
    pub groups: Option<Ht<TypeRef>>,
    pub target_ns: Option<String>,
    pub source: String,
}

// ---------------------------------------------------------------------------
// Constants

pub const SOAP_1_1: i32 = 1;
pub const SOAP_1_2: i32 = 2;
pub const SOAP_RPC: i32 = 1;
pub const SOAP_DOCUMENT: i32 = 2;
pub const SOAP_ENCODED: i32 = 1;
pub const SOAP_LITERAL: i32 = 2;

pub const SOAP_CLASS: i32 = 1;
pub const SOAP_FUNCTIONS: i32 = 2;
pub const SOAP_OBJECT: i32 = 3;
pub const SOAP_FUNCTIONS_ALL: i64 = 999;

pub const SOAP_PERSISTENCE_SESSION: i64 = 1;
pub const SOAP_PERSISTENCE_REQUEST: i64 = 2;

pub const SOAP_ACTOR_NEXT: i64 = 1;
pub const SOAP_ACTOR_NONE: i64 = 2;
pub const SOAP_ACTOR_UNLIMATERECEIVER: i64 = 3;

pub const SOAP_1_1_ACTOR_NEXT: &str = "http://schemas.xmlsoap.org/soap/actor/next";
pub const SOAP_1_2_ACTOR_NEXT: &str = "http://www.w3.org/2003/05/soap-envelope/role/next";
pub const SOAP_1_2_ACTOR_NONE: &str = "http://www.w3.org/2003/05/soap-envelope/role/none";
pub const SOAP_1_2_ACTOR_UNLIMATERECEIVER: &str =
    "http://www.w3.org/2003/05/soap-envelope/role/ultimateReceiver";

pub const SOAP_COMPRESSION_ACCEPT: i64 = 0x20;
pub const SOAP_COMPRESSION_GZIP: i64 = 0x00;
pub const SOAP_COMPRESSION_DEFLATE: i64 = 0x10;

pub const SOAP_AUTHENTICATION_BASIC: i64 = 0;
pub const SOAP_AUTHENTICATION_DIGEST: i64 = 1;

pub const SOAP_SINGLE_ELEMENT_ARRAYS: i64 = 1;
pub const SOAP_WAIT_ONE_WAY_CALLS: i64 = 2;
pub const SOAP_USE_XSI_ARRAY_TYPE: i64 = 4;

pub const WSDL_CACHE_NONE: i64 = 0;
pub const WSDL_CACHE_DISK: i64 = 1;
pub const WSDL_CACHE_MEMORY: i64 = 2;
pub const WSDL_CACHE_BOTH: i64 = 3;

pub const SOAP_SSL_METHOD_TLS: i64 = 0;
pub const SOAP_SSL_METHOD_SSLV2: i64 = 1;
pub const SOAP_SSL_METHOD_SSLV3: i64 = 2;
pub const SOAP_SSL_METHOD_SSLV23: i64 = 3;

pub const XSD_1999_NAMESPACE: &str = "http://www.w3.org/1999/XMLSchema";
pub const XSD_1999_TIMEINSTANT: i64 = 401;
pub const SOAP_1_1_ENV_NAMESPACE: &str = "http://schemas.xmlsoap.org/soap/envelope/";
pub const SOAP_1_1_ENV_NS_PREFIX: &str = "SOAP-ENV";
pub const SOAP_1_2_ENV_NAMESPACE: &str = "http://www.w3.org/2003/05/soap-envelope";
pub const SOAP_1_2_ENV_NS_PREFIX: &str = "env";
pub const SOAP_1_1_ENC_NAMESPACE: &str = "http://schemas.xmlsoap.org/soap/encoding/";
pub const SOAP_1_1_ENC_NS_PREFIX: &str = "SOAP-ENC";
pub const SOAP_1_2_ENC_NAMESPACE: &str = "http://www.w3.org/2003/05/soap-encoding";
pub const SOAP_1_2_ENC_NS_PREFIX: &str = "enc";
pub const XSD_DRAFT_2000_NAMESPACE: &str = "http://www.w3.org/2000/10/XMLSchema";
pub const XSD_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema";
pub const XSD_NS_PREFIX: &str = "xsd";
pub const XSI_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema-instance";
pub const XSI_NS_PREFIX: &str = "xsi";
pub const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
pub const XML_NS_PREFIX: &str = "xml";
pub const APACHE_NAMESPACE: &str = "http://xml.apache.org/xml-soap";
pub const WSDL_NAMESPACE: &str = "http://schemas.xmlsoap.org/wsdl/";
pub const WSDL_SOAP11_NAMESPACE: &str = "http://schemas.xmlsoap.org/wsdl/soap/";
pub const WSDL_SOAP12_NAMESPACE: &str = "http://schemas.xmlsoap.org/wsdl/soap12/";
pub const RPC_SOAP12_NAMESPACE: &str = "http://www.w3.org/2003/05/soap-rpc";
pub const RPC_SOAP12_NS_PREFIX: &str = "rpc";
pub const WSDL_HTTP11_NAMESPACE: &str = "http://schemas.xmlsoap.org/wsdl/http/";
pub const WSDL_HTTP12_NAMESPACE: &str = "http://www.w3.org/2003/05/soap/bindings/HTTP/";
pub const WSDL_HTTP_TRANSPORT: &str = "http://schemas.xmlsoap.org/soap/http";

pub const XSD_STRING: i64 = 101;
pub const XSD_BOOLEAN: i64 = 102;
pub const XSD_DECIMAL: i64 = 103;
pub const XSD_FLOAT: i64 = 104;
pub const XSD_DOUBLE: i64 = 105;
pub const XSD_DURATION: i64 = 106;
pub const XSD_DATETIME: i64 = 107;
pub const XSD_TIME: i64 = 108;
pub const XSD_DATE: i64 = 109;
pub const XSD_GYEARMONTH: i64 = 110;
pub const XSD_GYEAR: i64 = 111;
pub const XSD_GMONTHDAY: i64 = 112;
pub const XSD_GDAY: i64 = 113;
pub const XSD_GMONTH: i64 = 114;
pub const XSD_HEXBINARY: i64 = 115;
pub const XSD_BASE64BINARY: i64 = 116;
pub const XSD_ANYURI: i64 = 117;
pub const XSD_QNAME: i64 = 118;
pub const XSD_NOTATION: i64 = 119;
pub const XSD_NORMALIZEDSTRING: i64 = 120;
pub const XSD_TOKEN: i64 = 121;
pub const XSD_LANGUAGE: i64 = 122;
pub const XSD_NMTOKEN: i64 = 123;
pub const XSD_NAME: i64 = 124;
pub const XSD_NCNAME: i64 = 125;
pub const XSD_ID: i64 = 126;
pub const XSD_IDREF: i64 = 127;
pub const XSD_IDREFS: i64 = 128;
pub const XSD_ENTITY: i64 = 129;
pub const XSD_ENTITIES: i64 = 130;
pub const XSD_INTEGER: i64 = 131;
pub const XSD_NONPOSITIVEINTEGER: i64 = 132;
pub const XSD_NEGATIVEINTEGER: i64 = 133;
pub const XSD_LONG: i64 = 134;
pub const XSD_INT: i64 = 135;
pub const XSD_SHORT: i64 = 136;
pub const XSD_BYTE: i64 = 137;
pub const XSD_NONNEGATIVEINTEGER: i64 = 138;
pub const XSD_UNSIGNEDLONG: i64 = 139;
pub const XSD_UNSIGNEDINT: i64 = 140;
pub const XSD_UNSIGNEDSHORT: i64 = 141;
pub const XSD_UNSIGNEDBYTE: i64 = 142;
pub const XSD_POSITIVEINTEGER: i64 = 143;
pub const XSD_NMTOKENS: i64 = 144;
pub const XSD_ANYTYPE: i64 = 145;
pub const XSD_UR_TYPE: i64 = 146;
pub const XSD_ANYXML: i64 = 147;
pub const APACHE_MAP: i64 = 200;
pub const SOAP_ENC_ARRAY: i64 = 300;
pub const SOAP_ENC_OBJECT: i64 = 301;
pub const UNKNOWN_TYPE: i64 = 999998;
pub const END_KNOWN_TYPES: i64 = 999999;

/// zval type codes (IS_*) used as encoder types.
pub const IS_UNDEF: i64 = 0;
pub const IS_NULL: i64 = 1;
pub const IS_FALSE: i64 = 2;
pub const IS_TRUE: i64 = 3;
pub const IS_LONG: i64 = 4;
pub const IS_DOUBLE: i64 = 5;
pub const IS_STRING: i64 = 6;
pub const IS_ARRAY: i64 = 7;
pub const IS_OBJECT: i64 = 8;
pub const IS_RESOURCE: i64 = 9;

/// Z_TYPE_P()
pub fn zval_type(v: &Value) -> i64 {
    match v {
        Value::Undef => IS_UNDEF,
        Value::Null => IS_NULL,
        Value::False => IS_FALSE,
        Value::True => IS_TRUE,
        Value::Long(_) => IS_LONG,
        Value::Double(_) => IS_DOUBLE,
        Value::String(_) => IS_STRING,
        Value::Array(_) => IS_ARRAY,
        Value::Object(_) | Value::Generator(_) => IS_OBJECT,
        Value::Resource(_) => IS_RESOURCE,
        Value::Reference(r) => zval_type(&r.borrow()),
    }
}

// ---------------------------------------------------------------------------
// Globals

/// The encoder ref map of encode_reset_ns()/encode_finish():
/// value identity -> node while encoding, node -> value while decoding.
#[derive(Default)]
pub struct RefMap {
    pub out: std::collections::HashMap<usize, Node>,
    pub inp: std::collections::HashMap<Node, Value>,
}

#[derive(Default)]
pub struct Globals {
    pub typemap: Option<Rc<Ht<EncRef>>>,
    pub cur_uniq_ns: i64,
    pub soap_version: i32,
    pub sdl: Option<SdlRef>,
    pub error_code: Option<&'static str>,
    pub error_object: Option<Obj>,
    pub use_soap_error_handler: bool,
    pub encoding: Option<String>,
    pub class_map: Option<Arr>,
    pub features: i64,
    pub cur_uniq_ref: i64,
    pub ref_map: Option<RefMap>,
    /// In-memory WSDL cache: uri -> (time, sdl)
    pub mem_cache: IndexMap<String, (i64, SdlRef)>,
}

thread_local! {
    pub static G: RefCell<Globals> = RefCell::new(Globals {
        soap_version: SOAP_1_1,
        ..Default::default()
    });
}

/// Access the globals.
pub fn g<R>(f: impl FnOnce(&mut Globals) -> R) -> R {
    G.with(|g| f(&mut g.borrow_mut()))
}

pub fn soap_version() -> i32 {
    g(|g| g.soap_version)
}

pub fn cur_sdl() -> Option<SdlRef> {
    g(|g| g.sdl.clone())
}
