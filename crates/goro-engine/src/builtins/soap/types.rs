//! The built-in encodings of ext/soap: the XML Schema datatypes (XML
//! Schema Part 2), their SOAP-ENC and 1999 Schema twins, SOAP-ENC Array
//! and Struct, the Apache map, and the type codes PHP exposes as the
//! `XSD_*`, `SOAP_ENC_*`, `APACHE_MAP`, `UNKNOWN_TYPE` constants (PHP
//! manual: SOAP constants; codes from ReflectionExtension on PHP 8.5).

use super::xmlout::{NS_APACHE, NS_SOAP11_ENC, NS_SOAP12_ENC, NS_XSD, NS_XSD1999};

pub const UNKNOWN_TYPE: i64 = 999998;
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
pub const XSD_1999_TIMEINSTANT: i64 = 401;

/// How a built-in encoding converts values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conv {
    String,
    Boolean,
    /// Integers that fit PHP's int (int, short, byte, unsigned short/byte).
    Long,
    /// Integers that may not fit (long, integer, unsigned long...): ints
    /// when they fit, floats otherwise.
    BigInt,
    Double,
    Decimal,
    Base64,
    Hex,
    DateTime,
    Time,
    Date,
    GYearMonth,
    GYear,
    GMonthDay,
    GDay,
    GMonth,
    Duration,
    /// Whitespace-collapsed strings (token, NMTOKEN, Name, QName...).
    Token,
    /// normalizedString: whitespace replaced.
    Normalized,
    AnyType,
    AnyXml,
    Array,
    Struct,
    Map,
}

/// A built-in encoding.
#[derive(Debug)]
pub struct Builtin {
    pub code: i64,
    pub ns: &'static str,
    pub name: &'static str,
    pub conv: Conv,
}

macro_rules! b {
    ($code:expr, $ns:expr, $name:expr, $conv:ident) => {
        Builtin {
            code: $code,
            ns: $ns,
            name: $name,
            conv: Conv::$conv,
        }
    };
}

/// The XML Schema datatypes (first: the encoding a type code names).
pub static BUILTINS: &[Builtin] = &[
    b!(XSD_STRING, NS_XSD, "string", String),
    b!(XSD_BOOLEAN, NS_XSD, "boolean", Boolean),
    b!(XSD_DECIMAL, NS_XSD, "decimal", Decimal),
    b!(XSD_FLOAT, NS_XSD, "float", Double),
    b!(XSD_DOUBLE, NS_XSD, "double", Double),
    b!(XSD_DURATION, NS_XSD, "duration", Duration),
    b!(XSD_DATETIME, NS_XSD, "dateTime", DateTime),
    b!(XSD_TIME, NS_XSD, "time", Time),
    b!(XSD_DATE, NS_XSD, "date", Date),
    b!(XSD_GYEARMONTH, NS_XSD, "gYearMonth", GYearMonth),
    b!(XSD_GYEAR, NS_XSD, "gYear", GYear),
    b!(XSD_GMONTHDAY, NS_XSD, "gMonthDay", GMonthDay),
    b!(XSD_GDAY, NS_XSD, "gDay", GDay),
    b!(XSD_GMONTH, NS_XSD, "gMonth", GMonth),
    b!(XSD_HEXBINARY, NS_XSD, "hexBinary", Hex),
    b!(XSD_BASE64BINARY, NS_XSD, "base64Binary", Base64),
    b!(XSD_ANYURI, NS_XSD, "anyURI", Token),
    b!(XSD_QNAME, NS_XSD, "QName", Token),
    b!(XSD_NOTATION, NS_XSD, "NOTATION", Token),
    b!(XSD_NORMALIZEDSTRING, NS_XSD, "normalizedString", Normalized),
    b!(XSD_TOKEN, NS_XSD, "token", Token),
    b!(XSD_LANGUAGE, NS_XSD, "language", Token),
    b!(XSD_NMTOKEN, NS_XSD, "NMTOKEN", Token),
    b!(XSD_NAME, NS_XSD, "Name", Token),
    b!(XSD_NCNAME, NS_XSD, "NCName", Token),
    b!(XSD_ID, NS_XSD, "ID", Token),
    b!(XSD_IDREF, NS_XSD, "IDREF", Token),
    b!(XSD_IDREFS, NS_XSD, "IDREFS", Token),
    b!(XSD_ENTITY, NS_XSD, "ENTITY", Token),
    b!(XSD_ENTITIES, NS_XSD, "ENTITIES", Token),
    b!(XSD_INTEGER, NS_XSD, "integer", BigInt),
    b!(XSD_NONPOSITIVEINTEGER, NS_XSD, "nonPositiveInteger", BigInt),
    b!(XSD_NEGATIVEINTEGER, NS_XSD, "negativeInteger", BigInt),
    b!(XSD_LONG, NS_XSD, "long", BigInt),
    b!(XSD_INT, NS_XSD, "int", Long),
    b!(XSD_SHORT, NS_XSD, "short", Long),
    b!(XSD_BYTE, NS_XSD, "byte", Long),
    b!(XSD_NONNEGATIVEINTEGER, NS_XSD, "nonNegativeInteger", BigInt),
    b!(XSD_UNSIGNEDLONG, NS_XSD, "unsignedLong", BigInt),
    b!(XSD_UNSIGNEDINT, NS_XSD, "unsignedInt", BigInt),
    b!(XSD_UNSIGNEDSHORT, NS_XSD, "unsignedShort", Long),
    b!(XSD_UNSIGNEDBYTE, NS_XSD, "unsignedByte", Long),
    b!(XSD_POSITIVEINTEGER, NS_XSD, "positiveInteger", BigInt),
    b!(XSD_NMTOKENS, NS_XSD, "NMTOKENS", Token),
    b!(XSD_ANYTYPE, NS_XSD, "anyType", AnyType),
    b!(XSD_UR_TYPE, NS_XSD, "ur-type", AnyType),
    b!(XSD_ANYXML, NS_XSD, "anyXML", AnyXml),
    b!(SOAP_ENC_ARRAY, NS_SOAP11_ENC, "Array", Array),
    b!(SOAP_ENC_OBJECT, NS_SOAP11_ENC, "Struct", Struct),
    b!(APACHE_MAP, NS_APACHE, "Map", Map),
    b!(XSD_1999_TIMEINSTANT, NS_XSD1999, "timeInstant", DateTime),
    // The 1999 schema and the SOAP-ENC twins.
    b!(XSD_STRING, NS_XSD1999, "string", String),
    b!(XSD_BOOLEAN, NS_XSD1999, "boolean", Boolean),
    b!(XSD_DECIMAL, NS_XSD1999, "decimal", Decimal),
    b!(XSD_FLOAT, NS_XSD1999, "float", Double),
    b!(XSD_DOUBLE, NS_XSD1999, "double", Double),
    b!(XSD_LONG, NS_XSD1999, "long", BigInt),
    b!(XSD_INT, NS_XSD1999, "int", Long),
    b!(XSD_SHORT, NS_XSD1999, "short", Long),
    b!(XSD_BYTE, NS_XSD1999, "byte", Long),
    b!(XSD_UR_TYPE, NS_XSD1999, "ur-type", AnyType),
    b!(XSD_STRING, NS_SOAP11_ENC, "string", String),
    b!(XSD_BOOLEAN, NS_SOAP11_ENC, "boolean", Boolean),
    b!(XSD_DECIMAL, NS_SOAP11_ENC, "decimal", Decimal),
    b!(XSD_FLOAT, NS_SOAP11_ENC, "float", Double),
    b!(XSD_DOUBLE, NS_SOAP11_ENC, "double", Double),
    b!(XSD_DATETIME, NS_SOAP11_ENC, "dateTime", DateTime),
    b!(XSD_TIME, NS_SOAP11_ENC, "time", Time),
    b!(XSD_DATE, NS_SOAP11_ENC, "date", Date),
    b!(XSD_HEXBINARY, NS_SOAP11_ENC, "hexBinary", Hex),
    b!(XSD_BASE64BINARY, NS_SOAP11_ENC, "base64Binary", Base64),
    b!(XSD_BASE64BINARY, NS_SOAP11_ENC, "base64", Base64),
    b!(XSD_INTEGER, NS_SOAP11_ENC, "integer", BigInt),
    b!(XSD_LONG, NS_SOAP11_ENC, "long", BigInt),
    b!(XSD_INT, NS_SOAP11_ENC, "int", Long),
    b!(XSD_SHORT, NS_SOAP11_ENC, "short", Long),
    b!(XSD_BYTE, NS_SOAP11_ENC, "byte", Long),
    b!(XSD_UNSIGNEDLONG, NS_SOAP11_ENC, "unsignedLong", BigInt),
    b!(XSD_UNSIGNEDINT, NS_SOAP11_ENC, "unsignedInt", BigInt),
    b!(XSD_UNSIGNEDSHORT, NS_SOAP11_ENC, "unsignedShort", Long),
    b!(XSD_UNSIGNEDBYTE, NS_SOAP11_ENC, "unsignedByte", Long),
    b!(XSD_ANYURI, NS_SOAP11_ENC, "anyURI", Token),
    b!(XSD_QNAME, NS_SOAP11_ENC, "QName", Token),
    b!(XSD_STRING, NS_SOAP12_ENC, "string", String),
    b!(XSD_BOOLEAN, NS_SOAP12_ENC, "boolean", Boolean),
    b!(XSD_INT, NS_SOAP12_ENC, "int", Long),
    b!(XSD_FLOAT, NS_SOAP12_ENC, "float", Double),
    b!(XSD_DOUBLE, NS_SOAP12_ENC, "double", Double),
    b!(SOAP_ENC_ARRAY, NS_SOAP12_ENC, "Array", Array),
    b!(SOAP_ENC_OBJECT, NS_SOAP12_ENC, "Struct", Struct),
];

pub type Bi = &'static Builtin;

/// The encoding named by a type code (`SoapVar`, `XSD_*`).
pub fn by_code(code: i64) -> Option<Bi> {
    BUILTINS.iter().find(|b| b.code == code)
}

/// The built-in encoding of a qualified type name.
pub fn by_name(ns: &str, name: &str) -> Option<Bi> {
    BUILTINS.iter().find(|b| b.ns == ns && b.name == name)
}

pub fn string() -> Bi {
    &BUILTINS[0]
}

pub fn get(code: i64) -> Bi {
    by_code(code).expect("built-in encoding")
}

/// Whether a type code is one `SoapVar` accepts.
pub fn valid_code(code: i64) -> bool {
    code == UNKNOWN_TYPE || by_code(code).is_some()
}
