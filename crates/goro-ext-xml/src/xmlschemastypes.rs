//! XML Schema built-in datatypes (libxml2 xmlschemastypes.c): lexical
//! checking and value computation for the built-in simple types, value
//! comparison, facet checking and canonical representations.
//!
//! Shared by the XSD validator (xmlschema.rs) and the RelaxNG XML Schema
//! datatype library (relaxng.rs).

use crate::tree::{self, NType, NodeId};
use crate::xmlerrors as E;
use crate::xmlregexp::Regexp;
use crate::xmlunicode;
use std::rc::Rc;

pub const XS_NS: &str = "http://www.w3.org/2001/XMLSchema";

// xmlSchemaValType
pub const UNKNOWN: i32 = 0;
pub const STRING: i32 = 1;
pub const NORMSTRING: i32 = 2;
pub const DECIMAL: i32 = 3;
pub const TIME: i32 = 4;
pub const GDAY: i32 = 5;
pub const GMONTH: i32 = 6;
pub const GMONTHDAY: i32 = 7;
pub const GYEAR: i32 = 8;
pub const GYEARMONTH: i32 = 9;
pub const DATE: i32 = 10;
pub const DATETIME: i32 = 11;
pub const DURATION: i32 = 12;
pub const FLOAT: i32 = 13;
pub const DOUBLE: i32 = 14;
pub const BOOLEAN: i32 = 15;
pub const TOKEN: i32 = 16;
pub const LANGUAGE: i32 = 17;
pub const NMTOKEN: i32 = 18;
pub const NMTOKENS: i32 = 19;
pub const NAME: i32 = 20;
pub const QNAME: i32 = 21;
pub const NCNAME: i32 = 22;
pub const ID: i32 = 23;
pub const IDREF: i32 = 24;
pub const IDREFS: i32 = 25;
pub const ENTITY: i32 = 26;
pub const ENTITIES: i32 = 27;
pub const NOTATION: i32 = 28;
pub const ANYURI: i32 = 29;
pub const INTEGER: i32 = 30;
pub const NPINTEGER: i32 = 31;
pub const NINTEGER: i32 = 32;
pub const NNINTEGER: i32 = 33;
pub const PINTEGER: i32 = 34;
pub const INT: i32 = 35;
pub const UINT: i32 = 36;
pub const LONG: i32 = 37;
pub const ULONG: i32 = 38;
pub const SHORT: i32 = 39;
pub const USHORT: i32 = 40;
pub const BYTE: i32 = 41;
pub const UBYTE: i32 = 42;
pub const HEXBINARY: i32 = 43;
pub const BASE64BINARY: i32 = 44;
pub const ANYTYPE: i32 = 45;
pub const ANYSIMPLETYPE: i32 = 46;

// xmlSchemaTypeType facet kinds
pub const FACET_MININCLUSIVE: i32 = 1000;
pub const FACET_MINEXCLUSIVE: i32 = 1001;
pub const FACET_MAXINCLUSIVE: i32 = 1002;
pub const FACET_MAXEXCLUSIVE: i32 = 1003;
pub const FACET_TOTALDIGITS: i32 = 1004;
pub const FACET_FRACTIONDIGITS: i32 = 1005;
pub const FACET_PATTERN: i32 = 1006;
pub const FACET_ENUMERATION: i32 = 1007;
pub const FACET_WHITESPACE: i32 = 1008;
pub const FACET_LENGTH: i32 = 1009;
pub const FACET_MAXLENGTH: i32 = 1010;
pub const FACET_MINLENGTH: i32 = 1011;

// xmlSchemaWhitespaceValueType
pub const WS_UNKNOWN: i32 = 0;
pub const WS_PRESERVE: i32 = 1;
pub const WS_REPLACE: i32 = 2;
pub const WS_COLLAPSE: i32 = 3;

/// The built-in types: (name, value type, base type value type).
/// The order is the one of xmlSchemaInitTypes.
pub const BUILTINS: &[(&str, i32, i32)] = &[
    ("anyType", ANYTYPE, ANYTYPE),
    ("anySimpleType", ANYSIMPLETYPE, ANYTYPE),
    ("string", STRING, ANYSIMPLETYPE),
    ("decimal", DECIMAL, ANYSIMPLETYPE),
    ("date", DATE, ANYSIMPLETYPE),
    ("dateTime", DATETIME, ANYSIMPLETYPE),
    ("time", TIME, ANYSIMPLETYPE),
    ("gYear", GYEAR, ANYSIMPLETYPE),
    ("gYearMonth", GYEARMONTH, ANYSIMPLETYPE),
    ("gMonth", GMONTH, ANYSIMPLETYPE),
    ("gMonthDay", GMONTHDAY, ANYSIMPLETYPE),
    ("gDay", GDAY, ANYSIMPLETYPE),
    ("duration", DURATION, ANYSIMPLETYPE),
    ("float", FLOAT, ANYSIMPLETYPE),
    ("double", DOUBLE, ANYSIMPLETYPE),
    ("boolean", BOOLEAN, ANYSIMPLETYPE),
    ("anyURI", ANYURI, ANYSIMPLETYPE),
    ("hexBinary", HEXBINARY, ANYSIMPLETYPE),
    ("base64Binary", BASE64BINARY, ANYSIMPLETYPE),
    ("NOTATION", NOTATION, ANYSIMPLETYPE),
    ("QName", QNAME, ANYSIMPLETYPE),
    ("integer", INTEGER, DECIMAL),
    ("nonPositiveInteger", NPINTEGER, INTEGER),
    ("negativeInteger", NINTEGER, NPINTEGER),
    ("long", LONG, INTEGER),
    ("int", INT, LONG),
    ("short", SHORT, INT),
    ("byte", BYTE, SHORT),
    ("nonNegativeInteger", NNINTEGER, INTEGER),
    ("unsignedLong", ULONG, NNINTEGER),
    ("unsignedInt", UINT, ULONG),
    ("unsignedShort", USHORT, UINT),
    ("unsignedByte", UBYTE, USHORT),
    ("positiveInteger", PINTEGER, NNINTEGER),
    ("normalizedString", NORMSTRING, STRING),
    ("token", TOKEN, NORMSTRING),
    ("language", LANGUAGE, TOKEN),
    ("Name", NAME, TOKEN),
    ("NMTOKEN", NMTOKEN, TOKEN),
    ("NCName", NCNAME, NAME),
    ("ID", ID, NCNAME),
    ("IDREF", IDREF, NCNAME),
    ("ENTITY", ENTITY, NCNAME),
    ("ENTITIES", ENTITIES, ANYSIMPLETYPE),
    ("IDREFS", IDREFS, ANYSIMPLETYPE),
    ("NMTOKENS", NMTOKENS, ANYSIMPLETYPE),
];

/// xmlSchemaGetPredefinedType: the value type of a built-in type name in
/// the XML Schema namespace.
pub fn predefined_type(name: &str, ns: &str) -> Option<i32> {
    if ns != XS_NS {
        return None;
    }
    BUILTINS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, t, _)| *t)
}

pub fn builtin_name(vt: i32) -> &'static str {
    BUILTINS
        .iter()
        .find(|(_, t, _)| *t == vt)
        .map(|(n, _, _)| *n)
        .unwrap_or("???")
}

pub fn builtin_base(vt: i32) -> i32 {
    BUILTINS
        .iter()
        .find(|(_, t, _)| *t == vt)
        .map(|(_, _, b)| *b)
        .unwrap_or(ANYTYPE)
}

/// XML_SCHEMAS_TYPE_BUILTIN_PRIMITIVE
pub fn is_primitive(vt: i32) -> bool {
    matches!(
        vt,
        STRING
            | DECIMAL
            | DATE
            | DATETIME
            | TIME
            | GYEAR
            | GYEARMONTH
            | GMONTH
            | GMONTHDAY
            | GDAY
            | DURATION
            | FLOAT
            | DOUBLE
            | BOOLEAN
            | ANYURI
            | HEXBINARY
            | BASE64BINARY
            | QNAME
            | NOTATION
    )
}

/// Built-in list types (variety list).
pub fn is_list(vt: i32) -> bool {
    matches!(vt, IDREFS | NMTOKENS | ENTITIES)
}

/// xmlSchemaGetBuiltInListSimpleTypeItemType
pub fn list_item_type(vt: i32) -> Option<i32> {
    match vt {
        NMTOKENS => Some(NMTOKEN),
        IDREFS => Some(IDREF),
        ENTITIES => Some(ENTITY),
        _ => None,
    }
}

/// xmlSchemaIsBuiltInTypeFacet
pub fn is_builtin_type_facet(vt: i32, facet: i32) -> bool {
    match vt {
        BOOLEAN => facet == FACET_PATTERN || facet == FACET_WHITESPACE,
        STRING | NOTATION | QNAME | ANYURI | BASE64BINARY | HEXBINARY => matches!(
            facet,
            FACET_LENGTH
                | FACET_MINLENGTH
                | FACET_MAXLENGTH
                | FACET_PATTERN
                | FACET_ENUMERATION
                | FACET_WHITESPACE
        ),
        DECIMAL => matches!(
            facet,
            FACET_TOTALDIGITS
                | FACET_FRACTIONDIGITS
                | FACET_PATTERN
                | FACET_WHITESPACE
                | FACET_ENUMERATION
                | FACET_MAXINCLUSIVE
                | FACET_MAXEXCLUSIVE
                | FACET_MININCLUSIVE
                | FACET_MINEXCLUSIVE
        ),
        TIME | GDAY | GMONTH | GMONTHDAY | GYEAR | GYEARMONTH | DATE | DATETIME | DURATION
        | FLOAT | DOUBLE => matches!(
            facet,
            FACET_PATTERN
                | FACET_ENUMERATION
                | FACET_WHITESPACE
                | FACET_MAXINCLUSIVE
                | FACET_MAXEXCLUSIVE
                | FACET_MININCLUSIVE
                | FACET_MINEXCLUSIVE
        ),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Values

#[derive(Clone, Debug, PartialEq)]
pub struct Decimal {
    /// "+123.45" / "-0.5" / "+7.0": sign, integral digits, '.', fraction
    pub s: String,
    pub integral: u32,
    pub fractional: u32,
}

impl Decimal {
    fn frac_part(&self) -> &str {
        &self.s[2 + self.integral as usize..]
    }
    pub fn is_integer(&self) -> bool {
        self.fractional == 1 && self.frac_part().starts_with('0')
    }
    fn significant_digits(&self) -> u64 {
        let fractional = if self.is_integer() {
            0
        } else {
            self.fractional
        };
        let mut integral = self.integral;
        if integral == 1 && self.s.as_bytes()[1] == b'0' {
            integral = 0;
        }
        if integral + fractional == 0 {
            return 1;
        }
        (integral + fractional) as u64
    }
    fn from_int(v: i64) -> Decimal {
        let s = format!("{:+}.0", v);
        Decimal {
            integral: (s.len() - 3) as u32,
            s,
            fractional: 1,
        }
    }
    /// strtoul of the digits after the sign
    pub fn as_ulong(&self) -> u64 {
        let digits: String = self.s[1..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        digits.parse::<u64>().unwrap_or(u64::MAX)
    }
}

fn decimal_compare(l: &Decimal, r: &Decimal) -> i32 {
    if &l.s[1..] == "0.0" && &r.s[1..] == "0.0" {
        return 0;
    }
    let (ls, rs) = (l.s.as_bytes()[0], r.s.as_bytes()[0]);
    if ls != rs {
        return rs as i32 - ls as i32;
    }
    let sign = if ls == b'-' { -1 } else { 1 };
    if l.integral != r.integral {
        return (l.integral as i32 - r.integral as i32) * sign;
    }
    let c = match l.s[1..].cmp(&r.s[1..]) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    c * sign
}

fn decimal_compare_int(l: &Decimal, r: i64) -> i32 {
    decimal_compare(l, &Decimal::from_int(r))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DateVal {
    pub year: i64,
    pub mon: u32,
    pub day: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: f64,
    pub tz_flag: bool,
    pub tzo: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DurVal {
    pub mon: i64,
    pub day: i64,
    pub sec: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ValData {
    None,
    Str(String),
    Decimal(Decimal),
    Date(DateVal),
    Dur(DurVal),
    QName { name: String, uri: Option<String> },
    Hex { s: String, total: u32 },
    Base64 { s: String, total: u32 },
    Float(f32),
    Double(f64),
    Bool(bool),
}

/// A computed value (xmlSchemaVal).
#[derive(Clone, Debug, PartialEq)]
pub struct Val {
    pub typ: i32,
    pub data: ValData,
}

impl Val {
    fn new(typ: i32, data: ValData) -> Val {
        Val { typ, data }
    }
    pub fn str_value(&self) -> Option<&str> {
        match &self.data {
            ValData::Str(s) => Some(s),
            _ => None,
        }
    }
    fn date(&self) -> DateVal {
        match &self.data {
            ValData::Date(d) => *d,
            _ => DateVal::default(),
        }
    }
    fn decimal(&self) -> Option<&Decimal> {
        match &self.data {
            ValData::Decimal(d) => Some(d),
            _ => None,
        }
    }
}

/// Node context of a value being checked (xmlNodePtr argument): used to
/// resolve QName prefixes, look up entities and notations, and register
/// IDs.
pub trait NodeCtx {
    /// xmlSearchNs: the namespace name bound to `prefix` (None if not
    /// bound).
    fn search_ns(&self, prefix: Option<&str>) -> Option<String>;
    /// Whether the node belongs to a document.
    fn has_doc(&self) -> bool {
        true
    }
    /// Is `name` a declared unparsed entity of the document?
    fn unparsed_entity(&self, _name: &str) -> bool {
        false
    }
    /// xmlValidateNotationUse
    fn notation_declared(&self, _name: &str) -> bool {
        false
    }
    /// For attribute nodes: register an ID (xmlAddIDSafe); 1 added, 0
    /// already declared, None when the node is not an attribute.
    fn add_id(&self, _value: &str) -> Option<i32> {
        None
    }
    /// For attribute nodes: set the attribute type (IDREF(S), ENTITY...).
    fn set_attr_type(&self, _atype: u8) {}
}

/// A node of the global tree as value context.
pub struct TreeNode(pub NodeId);

impl NodeCtx for TreeNode {
    fn search_ns(&self, prefix: Option<&str>) -> Option<String> {
        tree::with(|t| {
            let ns = t.search_ns_ro(self.0, prefix)?;
            Some(t.nss[ns].href.clone())
        })
    }
    fn has_doc(&self) -> bool {
        tree::with(|t| t.nodes[self.0].doc.is_some())
    }
    fn unparsed_entity(&self, name: &str) -> bool {
        tree::with(|t| {
            let d = match t.nodes[self.0].doc {
                Some(d) => d,
                None => return false,
            };
            match t.get_doc_entity(d, name) {
                Some(e) => t
                    .entity_info(e)
                    .map(|i| i.etype == tree::EntityType::ExternalGeneralUnparsed)
                    .unwrap_or(false),
                None => false,
            }
        })
    }
    fn notation_declared(&self, name: &str) -> bool {
        tree::with(|t| {
            let d = match t.nodes[self.0].doc {
                Some(d) => d,
                None => return false,
            };
            let info = match t.doc_info(d) {
                Some(i) => i,
                None => return false,
            };
            for sub in [info.int_subset, info.ext_subset].into_iter().flatten() {
                if let Some(dd) = t.dtd_info(sub) {
                    if dd.notations.iter().any(|(n, _, _)| n == name) {
                        return true;
                    }
                }
            }
            false
        })
    }
    fn add_id(&self, value: &str) -> Option<i32> {
        tree::with(|t| {
            if t.nodes[self.0].t != NType::Attribute {
                return None;
            }
            if t.nodes[self.0].atype == 2 {
                // the IDness might have already been declared in the DTD
                return Some(1);
            }
            let at = self.0;
            if value.is_empty() {
                return Some(0);
            }
            let d = t.nodes[at].doc?;
            let di = t.doc_info_mut(d)?;
            if di.ids.contains_key(value) {
                return Some(0);
            }
            di.ids.insert(value.to_string(), at);
            t.nodes[at].atype = 2;
            Some(1)
        })
    }
    fn set_attr_type(&self, atype: u8) {
        tree::with(|t| {
            if t.nodes[self.0].t == NType::Attribute {
                t.nodes[self.0].atype = atype;
            }
        })
    }
}

impl TreeNode {
    pub fn is_attr(&self) -> bool {
        tree::with(|t| t.nodes[self.0].t == NType::Attribute)
    }
}

// ---------------------------------------------------------------------------
// Whitespace helpers

fn is_blank(c: u8) -> bool {
    c == 0x20 || c == 0x9 || c == 0xA || c == 0xD
}

/// xmlSchemaStrip: None if no change is needed.
pub fn strip(value: &str) -> Option<String> {
    let t = value.trim_matches([' ', '\t', '\n', '\r']);
    if t.len() == value.len() {
        None
    } else {
        Some(t.to_string())
    }
}

/// xmlSchemaWhiteSpaceReplace: None if no change is needed.
pub fn whitespace_replace(value: &str) -> Option<String> {
    if !value.bytes().any(|c| c == 0xd || c == 0x9 || c == 0xa) {
        return None;
    }
    Some(
        value
            .chars()
            .map(|c| {
                if c == '\r' || c == '\t' || c == '\n' {
                    ' '
                } else {
                    c
                }
            })
            .collect(),
    )
}

/// xmlSchemaCollapseString: None if no change is needed.
pub fn collapse_string(value: &str) -> Option<String> {
    let b = value.as_bytes();
    let mut start = 0;
    while start < b.len() && is_blank(b[start]) {
        start += 1;
    }
    let mut end = start;
    let mut col = 0usize;
    let mut found = false;
    while end < b.len() {
        if (b[end] == b' ' && end + 1 < b.len() && is_blank(b[end + 1]))
            || b[end] == 0xa
            || b[end] == 0x9
            || b[end] == 0xd
        {
            col = end - start;
            found = true;
            break;
        }
        end += 1;
    }
    if !found {
        let f = end;
        let mut e = end;
        while e > start && is_blank(b[e - 1]) {
            e -= 1;
        }
        if start == 0 && f == e {
            return None;
        }
        return Some(value[start..e].to_string());
    }
    // general case
    let mut out = String::with_capacity(b.len());
    out.push_str(&value[start..start + col]);
    let rest = &b[start + col..];
    let mut i = 0;
    let mut tmp: Vec<u8> = Vec::new();
    while i < rest.len() {
        if is_blank(rest[i]) {
            i += 1;
            while i < rest.len() && is_blank(rest[i]) {
                i += 1;
            }
            if i < rest.len() {
                tmp.push(b' ');
            }
        } else {
            tmp.push(rest[i]);
            i += 1;
        }
    }
    out.push_str(&String::from_utf8_lossy(&tmp));
    Some(out)
}

fn norm_len(value: &str) -> i64 {
    let b = value.as_bytes();
    let mut i = 0;
    let mut ret = 0;
    while i < b.len() && is_blank(b[i]) {
        i += 1;
    }
    while i < b.len() {
        if b[i] & 0x80 != 0 {
            let l = if b[i] & 0xf0 == 0xf0 {
                4
            } else if b[i] & 0xe0 == 0xe0 {
                3
            } else {
                2
            };
            i += l;
        } else if is_blank(b[i]) {
            while i < b.len() && is_blank(b[i]) {
                i += 1;
            }
            if i >= b.len() {
                break;
            }
        } else {
            i += 1;
        }
        ret += 1;
    }
    ret
}

fn utf8_len(value: &str) -> i64 {
    value.chars().count() as i64
}

// ---------------------------------------------------------------------------
// Name checks (tree.c, XML 1.0 4th edition character classes)

fn name_start(c: u32, colon: bool) -> bool {
    xmlunicode::is_letter(c) || c == '_' as u32 || (colon && c == ':' as u32)
}

fn name_char(c: u32, colon: bool) -> bool {
    xmlunicode::is_letter(c)
        || xmlunicode::is_digit(c)
        || c == '.' as u32
        || c == '-' as u32
        || c == '_' as u32
        || xmlunicode::is_combining(c)
        || xmlunicode::is_extender(c)
        || (colon && c == ':' as u32)
}

fn trim_blanks(value: &str, space: bool) -> &str {
    if space {
        value.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r')
    } else {
        value
    }
}

/// xmlValidateNCName: 0 if valid
pub fn validate_ncname(value: &str, space: bool) -> i32 {
    let v = trim_blanks(value, space);
    let mut it = v.chars();
    match it.next() {
        Some(c) if name_start(c as u32, false) => {}
        _ => return 1,
    }
    if it.all(|c| name_char(c as u32, false)) {
        0
    } else {
        1
    }
}

/// xmlValidateQName
pub fn validate_qname(value: &str, space: bool) -> i32 {
    let v = trim_blanks(value, space);
    let mut it = v.chars().peekable();
    match it.next() {
        Some(c) if name_start(c as u32, false) => {}
        _ => return 1,
    }
    while let Some(&c) = it.peek() {
        if name_char(c as u32, false) {
            it.next();
        } else {
            break;
        }
    }
    if it.peek() == Some(&':') {
        it.next();
        match it.next() {
            Some(c) if name_start(c as u32, false) => {}
            _ => return 1,
        }
        while let Some(&c) = it.peek() {
            if name_char(c as u32, false) {
                it.next();
            } else {
                break;
            }
        }
    }
    if it.next().is_some() { 1 } else { 0 }
}

/// xmlValidateName
pub fn validate_name(value: &str, space: bool) -> i32 {
    let v = trim_blanks(value, space);
    let mut it = v.chars();
    match it.next() {
        Some(c) if name_start(c as u32, true) => {}
        _ => return 1,
    }
    if it.all(|c| name_char(c as u32, true)) {
        0
    } else {
        1
    }
}

/// xmlValidateNMToken
pub fn validate_nmtoken(value: &str, space: bool) -> i32 {
    let v = trim_blanks(value, space);
    if v.is_empty() {
        return 1;
    }
    if v.chars().all(|c| name_char(c as u32, true)) {
        0
    } else {
        1
    }
}

/// xmlSplitQName2: (prefix, local) when there is a prefix.
pub fn split_qname2(name: &str) -> Option<(String, String)> {
    if name.starts_with(':') {
        return None;
    }
    let i = name.find(':')?;
    if i + 1 >= name.len() {
        return None;
    }
    Some((name[..i].to_string(), name[i + 1..].to_string()))
}

fn check_language(value: &str) -> bool {
    let mut first = true;
    let mut len = 0;
    for c in value.bytes() {
        if !(c.is_ascii_alphabetic() || c == b'-' || (!first && c.is_ascii_digit())) {
            return false;
        }
        if c == b'-' {
            if !(1..=8).contains(&len) {
                return false;
            }
            len = 0;
            first = false;
        } else {
            len += 1;
        }
    }
    (1..=8).contains(&len)
}

// ---------------------------------------------------------------------------
// Dates and durations

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

const DAYS_IN_MONTH: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const DAYS_IN_MONTH_LEAP: [u32; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn max_day_in_month(yr: i64, mon: u32) -> u32 {
    let m = ((mon as usize).max(1) - 1).min(11);
    if is_leap(yr) {
        DAYS_IN_MONTH_LEAP[m]
    } else {
        DAYS_IN_MONTH[m]
    }
}

fn valid_mday(d: &DateVal) -> bool {
    if d.mon < 1 || d.mon > 12 {
        return false;
    }
    d.day <= max_day_in_month(d.year, d.mon)
}

fn valid_date(d: &DateVal) -> bool {
    d.year != 0 && (1..=12).contains(&d.mon) && valid_mday(d)
}

fn valid_time(d: &DateVal) -> bool {
    let end_of_day = d.hour == 24 && d.min == 0 && d.sec == 0.0;
    ((d.hour <= 23 && d.min <= 59 && d.sec >= 0.0 && d.sec < 60.0) || end_of_day)
        && (-840..=840).contains(&d.tzo)
}

struct Cur<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Cur<'a> {
    fn c(&self) -> u8 {
        self.at(0)
    }
    fn at(&self, k: usize) -> u8 {
        self.b.get(self.i + k).copied().unwrap_or(0)
    }
}

fn parse_2_digits(c: &mut Cur) -> Option<u32> {
    let (a, b) = (c.at(0), c.at(1));
    c.i += 2;
    if !a.is_ascii_digit() || !b.is_ascii_digit() {
        return None;
    }
    Some(((a - b'0') * 10 + (b - b'0')) as u32)
}

fn parse_gyear(dt: &mut DateVal, c: &mut Cur) -> i32 {
    let ch = c.c();
    if !ch.is_ascii_digit() && ch != b'-' && ch != b'+' {
        return -1;
    }
    let mut isneg = false;
    if ch == b'-' {
        isneg = true;
        c.i += 1;
    }
    let first = c.c();
    let mut digcnt = 0;
    while c.c().is_ascii_digit() {
        let digit = (c.c() - b'0') as i64;
        if dt.year > i64::MAX / 10 {
            return 2;
        }
        dt.year *= 10;
        if dt.year > i64::MAX - digit {
            return 2;
        }
        dt.year += digit;
        c.i += 1;
        digcnt += 1;
    }
    if digcnt < 4 || (digcnt > 4 && first == b'0') {
        return 1;
    }
    if isneg {
        dt.year = -dt.year;
    }
    if dt.year == 0 {
        return 2;
    }
    0
}

fn parse_gmonth(dt: &mut DateVal, c: &mut Cur) -> i32 {
    let save = c.i;
    match parse_2_digits(c) {
        None => {
            c.i = save;
            1
        }
        Some(v) => {
            if !(1..=12).contains(&v) {
                c.i = save;
                return 2;
            }
            dt.mon = v;
            0
        }
    }
}

fn parse_gday(dt: &mut DateVal, c: &mut Cur) -> i32 {
    let save = c.i;
    match parse_2_digits(c) {
        None => {
            c.i = save;
            1
        }
        Some(v) => {
            if !(1..=31).contains(&v) {
                c.i = save;
                return 2;
            }
            dt.day = v;
            0
        }
    }
}

fn parse_time(dt: &mut DateVal, c: &mut Cur) -> i32 {
    let save = c.i;
    let v = match parse_2_digits(c) {
        None => {
            c.i = save;
            return 1;
        }
        Some(v) => v,
    };
    if c.c() != b':' {
        c.i = save;
        return 1;
    }
    if v > 23 && v != 24 {
        c.i = save;
        return 2;
    }
    c.i += 1;
    dt.hour = v;
    let v = match parse_2_digits(c) {
        None => {
            c.i = save;
            return 1;
        }
        Some(v) => v,
    };
    if v > 59 {
        c.i = save;
        return 2;
    }
    dt.min = v;
    if c.c() != b':' {
        c.i = save;
        return 1;
    }
    c.i += 1;
    // PARSE_FLOAT
    let v = match parse_2_digits(c) {
        None => {
            c.i = save;
            return 1;
        }
        Some(v) => v,
    };
    dt.sec = v as f64;
    if c.c() == b'.' {
        let mut mult = 1.0;
        c.i += 1;
        if !c.c().is_ascii_digit() {
            c.i = save;
            return 1;
        }
        while c.c().is_ascii_digit() {
            mult /= 10.0;
            dt.sec += (c.c() - b'0') as f64 * mult;
            c.i += 1;
        }
    }
    if !valid_time(dt) {
        c.i = save;
        return 2;
    }
    0
}

fn parse_timezone(dt: &mut DateVal, c: &mut Cur) -> i32 {
    let save = c.i;
    match c.c() {
        0 => {
            dt.tz_flag = false;
            dt.tzo = 0;
        }
        b'Z' => {
            dt.tz_flag = true;
            dt.tzo = 0;
            c.i += 1;
        }
        b'+' | b'-' => {
            let isneg = c.c() == b'-';
            c.i += 1;
            let tmp = match parse_2_digits(c) {
                None => {
                    c.i = save;
                    return 1;
                }
                Some(v) => v as i32,
            };
            if tmp > 23 {
                c.i = save;
                return 2;
            }
            if c.c() != b':' {
                c.i = save;
                return 1;
            }
            c.i += 1;
            dt.tzo = tmp * 60;
            let tmp = match parse_2_digits(c) {
                None => {
                    c.i = save;
                    return 1;
                }
                Some(v) => v as i32,
            };
            if tmp > 59 {
                c.i = save;
                return 2;
            }
            dt.tzo += tmp;
            if isneg {
                dt.tzo = -dt.tzo;
            }
            if !(-840..=840).contains(&dt.tzo) {
                c.i = save;
                return 2;
            }
            dt.tz_flag = true;
        }
        _ => return 1,
    }
    0
}

fn is_tzo_char(c: u8) -> bool {
    c == 0 || c == b'Z' || c == b'+' || c == b'-'
}

/// xmlSchemaValidateDates
fn validate_dates(typ: i32, value: &str, collapse: bool) -> Result<Val, i32> {
    let b = value.as_bytes();
    let mut c = Cur { b, i: 0 };
    if collapse {
        while is_blank(c.c()) {
            c.i += 1;
        }
    }
    let mut dt = DateVal::default();
    let start = c.i;
    macro_rules! ret_if_valid {
        ($t:expr) => {
            if is_tzo_char(c.c()) {
                let r = parse_timezone(&mut dt, &mut c);
                if r == 0 {
                    if c.c() != 0 {
                        return Err(1);
                    }
                    return finish(typ, $t, dt);
                }
            }
        };
    }
    fn finish(typ: i32, t: i32, dt: DateVal) -> Result<Val, i32> {
        if typ != UNKNOWN && typ != t {
            return Err(1);
        }
        Ok(Val::new(t, ValData::Date(dt)))
    }
    if c.c() == b'-' && c.at(1) == b'-' {
        c.i += 2;
        if c.c() == b'-' {
            if typ == GMONTH {
                return Err(1);
            }
            c.i += 1;
            if parse_gday(&mut dt, &mut c) != 0 {
                return Err(1);
            }
            ret_if_valid!(GDAY);
            return Err(1);
        }
        if parse_gmonth(&mut dt, &mut c) != 0 {
            return Err(1);
        }
        if c.c() == b'-' {
            let rewnd = c.i;
            c.i += 1;
            let r = parse_gday(&mut dt, &mut c);
            if r == 0 && (c.c() == 0 || c.c() != b':') {
                if valid_mday(&dt) {
                    ret_if_valid!(GMONTHDAY);
                    return Err(1);
                }
            }
            c.i = rewnd;
        }
        ret_if_valid!(GMONTH);
        return Err(1);
    }
    if c.c().is_ascii_digit() {
        let r = parse_time(&mut dt, &mut c);
        if r == 0 {
            ret_if_valid!(TIME);
        }
    }
    // fallback on date parsing (from the very beginning, as libxml2 does)
    let _ = start;
    c.i = 0;
    if parse_gyear(&mut dt, &mut c) != 0 {
        return Err(1);
    }
    ret_if_valid!(GYEAR);
    if c.c() != b'-' {
        return Err(1);
    }
    c.i += 1;
    if parse_gmonth(&mut dt, &mut c) != 0 {
        return Err(1);
    }
    ret_if_valid!(GYEARMONTH);
    if c.c() != b'-' {
        return Err(1);
    }
    c.i += 1;
    let r = parse_gday(&mut dt, &mut c);
    if r != 0 || !valid_date(&dt) {
        return Err(1);
    }
    ret_if_valid!(DATE);
    if c.c() != b'T' {
        return Err(1);
    }
    c.i += 1;
    if parse_time(&mut dt, &mut c) != 0 {
        return Err(1);
    }
    let r = parse_timezone(&mut dt, &mut c);
    if collapse {
        while is_blank(c.c()) {
            c.i += 1;
        }
    }
    if r != 0 || c.c() != 0 || !(valid_date(&dt) && valid_time(&dt)) {
        return Err(1);
    }
    finish(typ, DATETIME, dt)
}

/// xmlSchemaValidateDuration
fn validate_duration(value: &str, collapse: bool) -> Result<Val, i32> {
    let b = value.as_bytes();
    let mut c = Cur { b, i: 0 };
    if collapse {
        while is_blank(c.c()) {
            c.i += 1;
        }
    }
    let mut isneg = false;
    if c.c() == b'-' {
        isneg = true;
        c.i += 1;
    }
    if c.c() != b'P' {
        return Err(1);
    }
    c.i += 1;
    if c.c() == 0 {
        return Err(1);
    }
    let mut dur = DurVal::default();
    let mut seq = 0usize;
    let mut secs: i64 = 0;
    let mut sec_frac = 0.0f64;
    let desig = b"YMDHMS";
    while c.c() != 0 {
        let mut num: i64 = 0;
        let mut has_digits = false;
        let mut has_frac = false;
        if seq >= desig.len() {
            return Err(1);
        }
        if c.c() == b'T' {
            if seq > 3 {
                return Err(1);
            }
            c.i += 1;
            seq = 3;
        } else if seq == 3 {
            return Err(1);
        }
        while c.c().is_ascii_digit() {
            let digit = (c.c() - b'0') as i64;
            if num > i64::MAX / 10 {
                return Err(1);
            }
            num *= 10;
            if num > i64::MAX - digit {
                return Err(1);
            }
            num += digit;
            has_digits = true;
            c.i += 1;
        }
        if c.c() == b'.' {
            let mut mult = 1.0;
            c.i += 1;
            has_frac = true;
            while c.c().is_ascii_digit() {
                mult /= 10.0;
                sec_frac += (c.c() - b'0') as f64 * mult;
                has_digits = true;
                c.i += 1;
            }
        }
        while c.c() != desig[seq] {
            seq += 1;
            if seq == 3 || seq == desig.len() {
                return Err(1);
            }
        }
        c.i += 1;
        if !has_digits || (has_frac && seq != 5) {
            return Err(1);
        }
        match seq {
            0 => {
                if num > i64::MAX / 12 {
                    return Err(1);
                }
                dur.mon = num * 12;
            }
            1 => {
                if dur.mon > i64::MAX - num {
                    return Err(1);
                }
                dur.mon += num;
            }
            2 => dur.day = num,
            3 => {
                let days = num / 24;
                if dur.day > i64::MAX - days {
                    return Err(1);
                }
                dur.day += days;
                secs = (num % 24) * 3600;
            }
            4 => {
                let days = num / 1440;
                if dur.day > i64::MAX - days {
                    return Err(1);
                }
                dur.day += days;
                secs += (num % 1440) * 60;
            }
            5 => {
                let days = num / 86400;
                if dur.day > i64::MAX - days {
                    return Err(1);
                }
                dur.day += days;
                secs += num % 86400;
            }
            _ => {}
        }
        seq += 1;
    }
    let days = secs / 86400;
    if dur.day > i64::MAX - days {
        return Err(1);
    }
    dur.day += days;
    dur.sec = (secs % 86400) as f64 + sec_frac;
    if isneg {
        dur.mon = -dur.mon;
        dur.day = -dur.day;
        dur.sec = -dur.sec;
    }
    Ok(Val::new(DURATION, ValData::Dur(dur)))
}

fn b64_decode(ch: u8) -> i32 {
    match ch {
        b'A'..=b'Z' => (ch - b'A') as i32,
        b'a'..=b'z' => (ch - b'a') as i32 + 26,
        b'0'..=b'9' => (ch - b'0') as i32 + 52,
        b'+' => 62,
        b'/' => 63,
        b'=' => 64,
        _ => -1,
    }
}

/// xmlSchemaParseUInt: digits of an unsigned integer (leading zeroes
/// skipped) into a decimal; returns the digit count or -2.
fn parse_uint(c: &mut Cur) -> Result<Decimal, i32> {
    if !c.c().is_ascii_digit() {
        return Err(-2);
    }
    while c.c() == b'0' {
        c.i += 1;
    }
    if !c.c().is_ascii_digit() {
        c.i -= 1;
    }
    let start = c.i;
    while c.c().is_ascii_digit() {
        c.i += 1;
    }
    let digits = std::str::from_utf8(&c.b[start..c.i]).unwrap_or("0");
    Ok(Decimal {
        s: format!("+{}.0", digits),
        integral: digits.len() as u32,
        fractional: 1,
    })
}

/// Parameters of xmlSchemaValAtomicType.
#[derive(Clone, Copy)]
pub struct ValOpts {
    /// `flags`: 1 = don't normalize the value first
    pub flags: i32,
    pub ws: i32,
    pub norm_on_the_fly: bool,
    pub apply_norm: bool,
    pub create_string_value: bool,
}

/// xmlSchemaValAtomicType: 0 if valid, a positive error code otherwise
/// and -1 on internal errors. The computed value is returned when
/// `want_val` is set.
pub fn val_atomic_type(
    vt: i32,
    value: &str,
    want_val: bool,
    node: Option<&dyn NodeCtx>,
    o: ValOpts,
) -> (i32, Option<Val>) {
    let mut value: String = value.to_string();
    let mut normalized = false;
    if o.flags == 0 && vt != STRING && vt != ANYTYPE && vt != ANYSIMPLETYPE {
        let n = if vt == NORMSTRING {
            whitespace_replace(&value)
        } else {
            collapse_string(&value)
        };
        if let Some(n) = n {
            value = n;
            normalized = true;
        }
    }
    let ok = |v: Option<Val>| (0, if want_val { v } else { None });
    let bad = (1, None);
    match vt {
        UNKNOWN => (-1, None),
        ANYTYPE | ANYSIMPLETYPE => {
            if o.create_string_value && want_val {
                return (0, Some(Val::new(ANYSIMPLETYPE, ValData::Str(value))));
            }
            (0, None)
        }
        STRING => {
            if !o.norm_on_the_fly {
                let b = value.as_bytes();
                if o.ws == WS_REPLACE {
                    if b.iter().any(|&c| c == 0xd || c == 0xa || c == 0x9) {
                        return bad;
                    }
                } else if o.ws == WS_COLLAPSE {
                    let mut i = 0;
                    while i < b.len() {
                        let ch = b[i];
                        if ch == 0xd || ch == 0xa || ch == 0x9 {
                            return bad;
                        } else if ch == 0x20 {
                            i += 1;
                            if i < b.len() && b[i] == 0x20 {
                                return bad;
                            }
                        } else {
                            i += 1;
                        }
                    }
                }
            }
            if o.create_string_value && want_val {
                if o.apply_norm {
                    let n = if o.ws == WS_COLLAPSE {
                        collapse_string(&value)
                    } else if o.ws == WS_REPLACE {
                        whitespace_replace(&value)
                    } else {
                        None
                    };
                    if let Some(n) = n {
                        value = n;
                    }
                }
                return (0, Some(Val::new(STRING, ValData::Str(value))));
            }
            (0, None)
        }
        NORMSTRING => {
            if o.norm_on_the_fly {
                if o.apply_norm {
                    let n = if o.ws == WS_COLLAPSE {
                        collapse_string(&value)
                    } else {
                        whitespace_replace(&value)
                    };
                    if let Some(n) = n {
                        value = n;
                    }
                }
            } else if value.bytes().any(|c| c == 0xd || c == 0xa || c == 0x9) {
                return bad;
            }
            ok(Some(Val::new(NORMSTRING, ValData::Str(value))))
        }
        DECIMAL => {
            let b = value.as_bytes();
            if b.is_empty() {
                return bad;
            }
            let mut c = Cur { b, i: 0 };
            if o.norm_on_the_fly {
                while is_blank(c.c()) {
                    c.i += 1;
                }
            }
            let mut sign = b'+';
            if c.c() == b'-' {
                sign = b'-';
                c.i += 1;
            } else if c.c() == b'+' {
                c.i += 1;
            }
            if c.c() == 0 {
                return bad;
            }
            while c.c() == b'0' {
                c.i += 1;
            }
            let num_start = c.i;
            let mut integral = 0u32;
            let mut fractional = 0u32;
            while c.c().is_ascii_digit() {
                c.i += 1;
                integral += 1;
            }
            let int_end = c.i;
            if c.c() == b'.' {
                c.i += 1;
            }
            let frac_start = c.i;
            while c.c().is_ascii_digit() {
                c.i += 1;
                fractional += 1;
            }
            if fractional == 0 && integral == 0 && (num_start == 0 || b[num_start - 1] != b'0') {
                return bad;
            }
            let mut num_end = c.i;
            while num_end > num_start && fractional > 0 && b[num_end - 1] == b'0' {
                num_end -= 1;
                fractional -= 1;
            }
            if o.norm_on_the_fly {
                while is_blank(c.c()) {
                    c.i += 1;
                }
            }
            if c.c() != 0 {
                return bad;
            }
            if !want_val {
                return (0, None);
            }
            let mut ipart = std::str::from_utf8(&b[num_start..int_end])
                .unwrap_or("")
                .to_string();
            let mut fpart = std::str::from_utf8(&b[frac_start..frac_start + fractional as usize])
                .unwrap_or("")
                .to_string();
            let _ = num_end;
            if integral == 0 {
                ipart = "0".to_string();
                integral = 1;
            }
            if fractional == 0 {
                fpart = "0".to_string();
                fractional = 1;
            }
            let s = format!("{}{}.{}", sign as char, ipart, fpart);
            (
                0,
                Some(Val::new(
                    DECIMAL,
                    ValData::Decimal(Decimal {
                        s,
                        integral,
                        fractional,
                    }),
                )),
            )
        }
        TIME | GDAY | GMONTH | GMONTHDAY | GYEAR | GYEARMONTH | DATE | DATETIME => {
            match validate_dates(vt, &value, o.norm_on_the_fly) {
                Ok(v) => ok(Some(v)),
                Err(e) => (e, None),
            }
        }
        DURATION => match validate_duration(&value, o.norm_on_the_fly) {
            Ok(v) => ok(Some(v)),
            Err(e) => (e, None),
        },
        FLOAT | DOUBLE => {
            let b = value.as_bytes();
            let mut c = Cur { b, i: 0 };
            if o.norm_on_the_fly {
                while is_blank(c.c()) {
                    c.i += 1;
                }
            }
            let mk = |f: f64| -> Val {
                if vt == FLOAT {
                    Val::new(FLOAT, ValData::Float(f as f32))
                } else {
                    Val::new(DOUBLE, ValData::Double(f))
                }
            };
            if c.at(0) == b'N' && c.at(1) == b'a' && c.at(2) == b'N' {
                c.i += 3;
                if c.c() != 0 {
                    return bad;
                }
                return ok(Some(mk(f64::NAN)));
            }
            let mut neg = false;
            if c.c() == b'-' {
                neg = true;
                c.i += 1;
            }
            if c.at(0) == b'I' && c.at(1) == b'N' && c.at(2) == b'F' {
                c.i += 3;
                if c.c() != 0 {
                    return bad;
                }
                return ok(Some(mk(if neg {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                })));
            }
            if !neg && c.c() == b'+' {
                c.i += 1;
            }
            if c.c() == 0 || c.c() == b'+' || c.c() == b'-' {
                return bad;
            }
            let mut before = 0;
            let mut after = 0;
            while c.c().is_ascii_digit() {
                c.i += 1;
                before += 1;
            }
            if c.c() == b'.' {
                c.i += 1;
                while c.c().is_ascii_digit() {
                    c.i += 1;
                    after += 1;
                }
            }
            if before == 0 && after == 0 {
                return bad;
            }
            if c.c() == b'e' || c.c() == b'E' {
                c.i += 1;
                if c.c() == b'-' || c.c() == b'+' {
                    c.i += 1;
                }
                while c.c().is_ascii_digit() {
                    c.i += 1;
                }
            }
            if o.norm_on_the_fly {
                while is_blank(c.c()) {
                    c.i += 1;
                }
            }
            if c.c() != 0 {
                return bad;
            }
            if !want_val {
                return (0, None);
            }
            // sscanf("%f" / "%lf") on the longest numeric prefix
            let num = match scan_number(&value) {
                Some(n) => n,
                None => return bad,
            };
            if vt == FLOAT {
                match num.parse::<f32>() {
                    Ok(f) => (0, Some(Val::new(FLOAT, ValData::Float(f)))),
                    Err(_) => bad,
                }
            } else {
                match num.parse::<f64>() {
                    Ok(f) => (0, Some(Val::new(DOUBLE, ValData::Double(f)))),
                    Err(_) => bad,
                }
            }
        }
        BOOLEAN => {
            let b = value.as_bytes();
            let r = if o.norm_on_the_fly {
                let t = value.trim_matches([' ', '\t', '\n', '\r']);
                match t {
                    "0" | "false" => false,
                    "1" | "true" => true,
                    _ => return bad,
                }
            } else {
                match b {
                    b"0" | b"false" => false,
                    b"1" | b"true" => true,
                    _ => return bad,
                }
            };
            ok(Some(Val::new(BOOLEAN, ValData::Bool(r))))
        }
        TOKEN => {
            if !o.norm_on_the_fly {
                let b = value.as_bytes();
                let mut i = 0;
                while i < b.len() {
                    let ch = b[i];
                    if ch == 0xd || ch == 0xa || ch == 0x9 {
                        return bad;
                    } else if ch == b' ' {
                        i += 1;
                        if i >= b.len() || b[i] == b' ' {
                            return bad;
                        }
                    } else {
                        i += 1;
                    }
                }
            }
            ok(Some(Val::new(TOKEN, ValData::Str(value))))
        }
        LANGUAGE => {
            if !normalized && o.norm_on_the_fly {
                if let Some(n) = collapse_string(&value) {
                    value = n;
                }
            }
            if check_language(&value) {
                return ok(Some(Val::new(LANGUAGE, ValData::Str(value))));
            }
            bad
        }
        NMTOKEN => {
            if validate_nmtoken(&value, true) == 0 {
                return ok(Some(Val::new(NMTOKEN, ValData::Str(value))));
            }
            bad
        }
        NMTOKENS => {
            let r = val_atomic_list(NMTOKEN, &value, node);
            (if r > 0 { 0 } else { 1 }, None)
        }
        NAME => {
            let r = validate_name(&value, true);
            if r == 0 && want_val {
                let t = value.trim_start_matches([' ', '\t', '\n', '\r']);
                let end = t.find([' ', '\t', '\n', '\r']).unwrap_or(t.len());
                return (0, Some(Val::new(NAME, ValData::Str(t[..end].to_string()))));
            }
            (r, None)
        }
        QNAME => {
            let r = validate_qname(&value, true);
            if r != 0 {
                return (r, None);
            }
            let mut uri = None;
            let mut local = None;
            if let Some(n) = node {
                let (prefix, l) = match split_qname2(&value) {
                    Some((p, l)) => (Some(p), Some(l)),
                    None => (None, None),
                };
                local = l;
                let ns = n.search_ns(prefix.as_deref());
                if ns.is_none() && prefix.is_some() {
                    return bad;
                }
                uri = ns;
            }
            if want_val {
                let name = local.unwrap_or_else(|| value.clone());
                return (0, Some(Val::new(QNAME, ValData::QName { name, uri })));
            }
            (0, None)
        }
        NCNAME => {
            let r = validate_ncname(&value, true);
            if r == 0 && want_val {
                return (0, Some(Val::new(NCNAME, ValData::Str(value))));
            }
            (r, None)
        }
        ID => {
            let mut r = validate_ncname(&value, true);
            let v = if r == 0 && want_val {
                Some(Val::new(ID, ValData::Str(value.clone())))
            } else {
                None
            };
            if r == 0 {
                if let Some(n) = node {
                    let s = strip(&value).unwrap_or_else(|| value.clone());
                    if let Some(res) = n.add_id(&s) {
                        if res < 0 {
                            return (-1, None);
                        } else if res == 0 {
                            r = 2;
                        }
                    }
                }
            }
            (r, v)
        }
        IDREF => {
            let r = validate_ncname(&value, true);
            let v = if r == 0 && want_val {
                Some(Val::new(IDREF, ValData::Str(value.clone())))
            } else {
                None
            };
            if r == 0 {
                if let Some(n) = node {
                    n.set_attr_type(3);
                }
            }
            (r, v)
        }
        IDREFS => {
            let r = val_atomic_list(IDREF, &value, node);
            let r = if r < 0 { 2 } else { 0 };
            if r == 0 {
                if let Some(n) = node {
                    n.set_attr_type(4);
                }
            }
            (r, None)
        }
        ENTITY => {
            let mut r = validate_ncname(&value, true);
            if node.map(|n| !n.has_doc()).unwrap_or(true) {
                r = 3;
            }
            if r == 0 {
                let s = strip(&value).unwrap_or_else(|| value.clone());
                if !node.unwrap().unparsed_entity(&s) {
                    r = 4;
                }
            }
            if r == 0 {
                node.unwrap().set_attr_type(5);
            }
            (r, None)
        }
        ENTITIES => {
            if node.map(|n| !n.has_doc()).unwrap_or(true) {
                return (3, None);
            }
            let r = val_atomic_list(ENTITY, &value, node);
            let r = if r <= 0 { 1 } else { 0 };
            if r == 0 {
                node.unwrap().set_attr_type(6);
            }
            (r, None)
        }
        NOTATION => {
            let mut r = validate_qname(&value, true);
            let mut uri = None;
            let mut local = None;
            if r == 0 {
                if let Some(n) = node {
                    if let Some((p, l)) = split_qname2(&value) {
                        match n.search_ns(Some(&p)) {
                            None => r = 1,
                            Some(h) => uri = Some(h),
                        }
                        local = Some(l);
                    }
                }
            }
            if node.map(|n| !n.has_doc()).unwrap_or(true) {
                r = 3;
            }
            if r == 0 {
                r = if node.unwrap().notation_declared(&value) {
                    0
                } else {
                    1
                };
            }
            if r == 0 && want_val {
                let name = local.unwrap_or_else(|| value.clone());
                return (0, Some(Val::new(NOTATION, ValData::QName { name, uri })));
            }
            (r, None)
        }
        ANYURI => {
            if !value.is_empty() {
                if !normalized && o.norm_on_the_fly {
                    if let Some(n) = collapse_string(&value) {
                        value = n;
                    }
                }
                let tmp: String = value
                    .bytes()
                    .map(|c| {
                        if !(32..127).contains(&c) || b" <>\"{}|\\^`'".contains(&c) {
                            '_'
                        } else {
                            c as char
                        }
                    })
                    .collect();
                if crate::uri::parse_uri(&tmp).is_none() {
                    return bad;
                }
            }
            ok(Some(Val::new(ANYURI, ValData::Str(value))))
        }
        HEXBINARY => {
            let b = value.as_bytes();
            let mut c = Cur { b, i: 0 };
            if o.norm_on_the_fly {
                while is_blank(c.c()) {
                    c.i += 1;
                }
            }
            let start = c.i;
            while c.c().is_ascii_hexdigit() {
                c.i += 1;
            }
            let n = c.i - start;
            if o.norm_on_the_fly {
                while is_blank(c.c()) {
                    c.i += 1;
                }
            }
            if c.c() != 0 || !n.is_multiple_of(2) {
                return bad;
            }
            let s = value[start..start + n].to_ascii_uppercase();
            ok(Some(Val::new(
                HEXBINARY,
                ValData::Hex {
                    s,
                    total: (n / 2) as u32,
                },
            )))
        }
        BASE64BINARY => {
            let b = value.as_bytes();
            let mut i = 0usize;
            let mut cnt = 0i64;
            let mut pad = 0;
            while i < b.len() {
                let d = b64_decode(b[i]);
                if d < 0 {
                } else if d < 64 {
                    cnt += 1;
                } else {
                    break;
                }
                i += 1;
            }
            while i < b.len() {
                let d = b64_decode(b[i]);
                if d < 0 {
                } else if d < 64 {
                    return bad;
                }
                if d == 64 {
                    pad += 1;
                }
                i += 1;
            }
            let mut total = 3 * (cnt / 4);
            // the last significant character before the padding
            let last_sig = || -> i32 {
                let mut k = b.len();
                while k > 0 {
                    k -= 1;
                    let d = b64_decode(b[k]);
                    if (0..=63).contains(&d) {
                        return d;
                    }
                }
                0
            };
            if pad == 0 {
                if cnt % 4 != 0 {
                    return bad;
                }
            } else if pad == 1 {
                if cnt % 4 != 3 {
                    return bad;
                }
                if last_sig() & !0x3c != 0 {
                    return bad;
                }
                total += 2;
            } else if pad == 2 {
                if cnt % 4 != 2 {
                    return bad;
                }
                if last_sig() & !0x30 != 0 {
                    return bad;
                }
                total += 1;
            } else {
                return bad;
            }
            let s: String = value
                .chars()
                .filter(|&ch| ch.is_ascii() && b64_decode(ch as u8) >= 0)
                .collect();
            ok(Some(Val::new(
                BASE64BINARY,
                ValData::Base64 {
                    s,
                    total: total as u32,
                },
            )))
        }
        INTEGER | PINTEGER | NPINTEGER | NINTEGER | NNINTEGER | LONG | BYTE | SHORT | INT
        | UINT | ULONG | USHORT | UBYTE => {
            let b = value.as_bytes();
            let mut c = Cur { b, i: 0 };
            if o.norm_on_the_fly {
                while is_blank(c.c()) {
                    c.i += 1;
                }
            }
            let mut sign = '+';
            if c.c() == b'-' {
                sign = '-';
                c.i += 1;
            } else if c.c() == b'+' {
                c.i += 1;
            }
            let mut d = match parse_uint(&mut c) {
                Ok(d) => d,
                Err(_) => return bad,
            };
            d.s.replace_range(0..1, &sign.to_string());
            if o.norm_on_the_fly {
                while is_blank(c.c()) {
                    c.i += 1;
                }
            }
            if c.c() != 0 {
                return bad;
            }
            let dec = |s: &str, ip: u32| Decimal {
                s: s.to_string(),
                integral: ip,
                fractional: 1,
            };
            let out = match vt {
                NPINTEGER => decimal_compare_int(&d, 0) > 0,
                PINTEGER => sign == '-' || decimal_compare_int(&d, 0) <= 0,
                NINTEGER => decimal_compare_int(&d, 0) >= 0,
                NNINTEGER => decimal_compare_int(&d, 0) < 0,
                LONG => {
                    decimal_compare(&d, &dec("+9223372036854775807.0", 19)) > 0
                        || decimal_compare(&d, &dec("-9223372036854775808.0", 19)) < 0
                }
                ULONG => {
                    decimal_compare(&d, &dec("+18446744073709551615.0", 20)) > 0
                        || decimal_compare_int(&d, 0) < 0
                }
                INT => {
                    decimal_compare_int(&d, 0x7fffffff) > 0
                        || decimal_compare_int(&d, -0x80000000) < 0
                }
                SHORT => {
                    decimal_compare_int(&d, 0x7fff) > 0 || decimal_compare_int(&d, -0x8000) < 0
                }
                BYTE => decimal_compare_int(&d, 0x7f) > 0 || decimal_compare_int(&d, -0x80) < 0,
                UINT => {
                    decimal_compare(&d, &dec("+4294967295.0", 10)) > 0
                        || decimal_compare_int(&d, 0) < 0
                }
                USHORT => decimal_compare_int(&d, 0xffff) > 0 || decimal_compare_int(&d, 0) < 0,
                UBYTE => decimal_compare_int(&d, 0xff) > 0 || decimal_compare_int(&d, 0) < 0,
                _ => false,
            };
            if out {
                return bad;
            }
            ok(Some(Val::new(vt, ValData::Decimal(d))))
        }
        _ => (0, None),
    }
}

/// The numeric prefix sscanf("%lf") would consume.
fn scan_number(s: &str) -> Option<&str> {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r']);
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let ds = i;
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 || i == ds {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    Some(&t[..i])
}

/// xmlSchemaValAtomicListNode: number of items, or -1.
fn val_atomic_list(item: i32, value: &str, node: Option<&dyn NodeCtx>) -> i32 {
    let items: Vec<&str> = value
        .split([' ', '\t', '\n', '\r'])
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() {
        return 0;
    }
    for it in &items {
        let (r, _) = val_predef_type_node(item, it, false, node);
        if r != 0 {
            return -1;
        }
    }
    items.len() as i32
}

/// xmlSchemaValPredefTypeNode
pub fn val_predef_type_node(
    vt: i32,
    value: &str,
    want_val: bool,
    node: Option<&dyn NodeCtx>,
) -> (i32, Option<Val>) {
    val_atomic_type(
        vt,
        value,
        want_val,
        node,
        ValOpts {
            flags: 0,
            ws: WS_UNKNOWN,
            norm_on_the_fly: true,
            apply_norm: true,
            create_string_value: false,
        },
    )
}

/// xmlSchemaValPredefTypeNodeNoNorm
pub fn val_predef_type_node_no_norm(
    vt: i32,
    value: &str,
    want_val: bool,
    node: Option<&dyn NodeCtx>,
) -> (i32, Option<Val>) {
    val_atomic_type(
        vt,
        value,
        want_val,
        node,
        ValOpts {
            flags: 1,
            ws: WS_UNKNOWN,
            norm_on_the_fly: true,
            apply_norm: false,
            create_string_value: true,
        },
    )
}

// ---------------------------------------------------------------------------
// Comparisons

fn compare_durations(x: &DurVal, y: &DurVal) -> i32 {
    const DAY_RANGE: [[i64; 12]; 2] = [
        [0, 28, 59, 89, 120, 150, 181, 212, 242, 273, 303, 334],
        [0, 31, 62, 92, 123, 153, 184, 215, 245, 276, 306, 337],
    ];
    let mon = x.mon - y.mon;
    let mut sec = x.sec - y.sec;
    let carry = (sec / 86400.0) as i64;
    sec -= carry as f64 * 86400.0;
    let day = x.day - y.day + carry;
    if mon == 0 {
        if day == 0 {
            if sec == 0.0 {
                return 0;
            } else if sec < 0.0 {
                return -1;
            } else {
                return 1;
            }
        } else if day < 0 {
            return -1;
        } else {
            return 1;
        }
    }
    let (xmon, xday, invert);
    if mon > 0 {
        if day >= 0 && sec >= 0.0 {
            return 1;
        }
        xmon = mon;
        xday = -day;
        invert = 1;
    } else if day <= 0 && sec <= 0.0 {
        return -1;
    } else {
        invert = -1;
        xmon = -mon;
        xday = day;
    }
    let myear = xmon / 12;
    let (mut minday, mut maxday);
    if myear == 0 {
        minday = 0;
        maxday = 0;
    } else {
        if myear > i64::MAX / 366 {
            return -2;
        }
        maxday = 365 * myear + (myear + 3) / 4;
        minday = maxday - 1;
    }
    let xm = (xmon % 12) as usize;
    minday += DAY_RANGE[0][xm];
    maxday += DAY_RANGE[1][xm];
    if maxday == minday && maxday == xday {
        return 0;
    }
    if maxday < xday {
        return -invert;
    }
    if minday > xday {
        return invert;
    }
    2
}

fn fquotient(a: f64, b: f64) -> f64 {
    (a / b).floor()
}
fn modulo(a: f64, b: f64) -> f64 {
    a - fquotient(a, b) * b
}
fn modulo_range(a: f64, low: f64, high: f64) -> f64 {
    modulo(a - low, high - low) + low
}
fn fquotient_range(a: f64, low: f64, high: f64) -> f64 {
    fquotient(a - low, high - low)
}

/// _xmlSchemaDateAdd
fn date_add(typ: i32, dt: &DateVal, dur: &DurVal) -> (i32, DateVal) {
    let mut d = *dt;
    let mut u = *dur;
    let mut r = DateVal::default();
    if d.mon == 0 {
        d.mon = 1;
    }
    u.sec -= d.tzo as f64 * 60.0;
    d.tzo = 0;
    if d.day == 0 {
        d.day = 1;
    }
    let mut carry = d.mon as i64 + u.mon;
    r.mon = modulo_range(carry as f64, 1.0, 13.0) as u32;
    carry = fquotient_range(carry as f64, 1.0, 13.0) as i64;
    r.year = d.year + carry;
    if r.year == 0 {
        if d.year > 0 {
            r.year -= 1;
        } else {
            r.year += 1;
        }
    }
    r.tzo = d.tzo;
    r.tz_flag = d.tz_flag;
    r.sec = d.sec + u.sec;
    carry = fquotient((r.sec as i64) as f64, 60.0) as i64;
    if r.sec != 0.0 {
        r.sec = modulo(r.sec, 60.0);
    }
    carry += d.min as i64;
    r.min = modulo(carry as f64, 60.0) as u32;
    carry = fquotient(carry as f64, 60.0) as i64;
    carry += d.hour as i64;
    r.hour = modulo(carry as f64, 24.0) as u32;
    carry = fquotient(carry as f64, 24.0) as i64;
    let mut tempdays: i64 =
        if r.year != 0 && (1..=12).contains(&r.mon) && d.day > max_day_in_month(r.year, r.mon) {
            max_day_in_month(r.year, r.mon) as i64
        } else if d.day < 1 {
            1
        } else {
            d.day as i64
        };
    tempdays += u.day + carry;
    loop {
        if tempdays < 1 {
            let mut tmon = modulo_range(r.mon as f64 - 1.0, 1.0, 13.0) as i64;
            let mut tyr = r.year + fquotient_range(r.mon as f64 - 1.0, 1.0, 13.0) as i64;
            if tyr == 0 {
                tyr -= 1;
            }
            tmon = tmon.clamp(1, 12);
            tempdays += max_day_in_month(tyr, tmon as u32) as i64;
            carry = -1;
        } else if r.year != 0
            && (1..=12).contains(&r.mon)
            && tempdays > max_day_in_month(r.year, r.mon) as i64
        {
            tempdays -= max_day_in_month(r.year, r.mon) as i64;
            carry = 1;
        } else {
            break;
        }
        let temp = r.mon as i64 + carry;
        r.mon = modulo_range(temp as f64, 1.0, 13.0) as u32;
        r.year += fquotient_range(temp as f64, 1.0, 13.0) as i64;
        if r.year == 0 {
            if temp < 1 {
                r.year -= 1;
            } else {
                r.year += 1;
            }
        }
    }
    r.day = (tempdays as u32) & 0x1f;
    let mut t = typ;
    if t != DATETIME {
        if r.hour != 0 || r.min != 0 || r.sec != 0.0 {
            t = DATETIME;
        } else if t != DATE {
            if r.mon != 1 && r.day != 1 {
                t = DATE;
            } else if t != GYEARMONTH && r.mon != 1 {
                t = GYEARMONTH;
            }
        }
    }
    (t, r)
}

/// xmlSchemaDateNormalize
fn date_normalize(typ: i32, dt: &DateVal, offset: f64) -> (i32, DateVal) {
    if (typ != TIME && typ != DATETIME && typ != DATE) || dt.tzo == 0 {
        return (typ, *dt);
    }
    let dur = DurVal {
        mon: 0,
        day: 0,
        sec: -offset,
    };
    date_add(typ, dt, &dur)
}

fn cast_ym_to_days(dt: &DateVal) -> i64 {
    let mon = if dt.mon == 0 { 1 } else { dt.mon };
    let y = dt.year;
    let diy = |m: u32, yr: i64| -> i64 {
        const D: [i64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
        const DL: [i64; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];
        let i = (m as usize - 1).min(11);
        if is_leap(yr) { DL[i] } else { D[i] }
    };
    if y <= 0 {
        y * 365 + ((y + 1) / 4 - (y + 1) / 100 + (y + 1) / 400) + diy(mon, y)
    } else {
        (y - 1) * 365 + ((y - 1) / 4 - (y - 1) / 100 + (y - 1) / 400) + diy(mon, y)
    }
}

fn time_to_number(d: &DateVal) -> f64 {
    (d.hour as i64 * 3600 + d.min as i64 * 60 + d.tzo as i64 * 60) as f64 + d.sec
}

fn compare_dates(xt: i32, x: &DateVal, yt: i32, y: &DateVal) -> i32 {
    if x.year > i64::MAX / 366
        || x.year < i64::MIN / 366
        || y.year > i64::MAX / 366
        || y.year < i64::MIN / 366
    {
        return -2;
    }
    if x.tz_flag {
        if !y.tz_flag {
            let (_, p1) = date_normalize(xt, x, 0.0);
            let p1d = cast_ym_to_days(&p1) + p1.day as i64;
            let (_, q1) = date_normalize(yt, y, 14.0 * 3600.0);
            let q1d = cast_ym_to_days(&q1) + q1.day as i64;
            if p1d < q1d {
                return -1;
            } else if p1d == q1d {
                let sec = time_to_number(&p1) - time_to_number(&q1);
                if sec < 0.0 {
                    return -1;
                } else {
                    let mut ret = 0;
                    let (_, q2) = date_normalize(yt, y, -(14.0 * 3600.0));
                    let q2d = cast_ym_to_days(&q2) + q2.day as i64;
                    if p1d > q2d {
                        ret = 1;
                    } else if p1d == q2d {
                        let sec = time_to_number(&p1) - time_to_number(&q2);
                        ret = if sec > 0.0 { 1 } else { 2 };
                    }
                    if ret != 0 {
                        return ret;
                    }
                }
            }
        }
    } else if y.tz_flag {
        let (_, q1) = date_normalize(yt, y, 0.0);
        let q1d = cast_ym_to_days(&q1) + q1.day as i64;
        let (_, p1) = date_normalize(xt, x, -(14.0 * 3600.0));
        let p1d = cast_ym_to_days(&p1) + p1.day as i64;
        if p1d < q1d {
            return -1;
        } else if p1d == q1d {
            let sec = time_to_number(&p1) - time_to_number(&q1);
            if sec < 0.0 {
                return -1;
            } else {
                let mut ret = 0;
                let (_, p2) = date_normalize(xt, x, 14.0 * 3600.0);
                let p2d = cast_ym_to_days(&p2) + p2.day as i64;
                if p2d > q1d {
                    ret = 1;
                } else if p2d == q1d {
                    let sec = time_to_number(&p2) - time_to_number(&q1);
                    ret = if sec > 0.0 { 1 } else { 2 };
                }
                if ret != 0 {
                    return ret;
                }
            }
        }
    }
    if xt == yt {
        let (_, q1) = date_normalize(yt, y, 0.0);
        let q1d = cast_ym_to_days(&q1) + q1.day as i64;
        let (_, p1) = date_normalize(xt, x, 0.0);
        let p1d = cast_ym_to_days(&p1) + p1.day as i64;
        if p1d < q1d {
            return -1;
        } else if p1d > q1d {
            return 1;
        }
        let sec = time_to_number(&p1) - time_to_number(&q1);
        if sec < 0.0 {
            return -1;
        } else if sec > 0.0 {
            return 1;
        }
        return 0;
    }
    let mask = |t: i32| -> u8 {
        match t {
            DATETIME => 0xf,
            DATE => 0x7,
            GYEAR => 0x1,
            GMONTH => 0x2,
            GDAY => 0x3,
            GYEARMONTH => 0x3,
            GMONTHDAY => 0x6,
            TIME => 0x8,
            _ => 0,
        }
    };
    let (xm, ym) = (mask(xt), mask(yt));
    let xor = xm ^ ym;
    let and = xm & ym;
    if xor & 1 != 0 {
        return 2;
    } else if and & 1 != 0 {
        if x.year < y.year {
            return -1;
        } else if x.year > y.year {
            return 1;
        }
    }
    if xor & 2 != 0 {
        return 2;
    } else if and & 2 != 0 {
        if x.mon < y.mon {
            return -1;
        } else if x.mon > y.mon {
            return 1;
        }
    }
    if xor & 4 != 0 {
        return 2;
    } else if and & 4 != 0 {
        if x.day < y.day {
            return -1;
        } else if x.day > y.day {
            return 1;
        }
    }
    if xor & 8 != 0 {
        return 2;
    } else if and & 8 != 0 {
        if x.hour < y.hour {
            return -1;
        } else if x.hour > y.hour {
            return 1;
        } else if x.min < y.min {
            return -1;
        } else if x.min > y.min {
            return 1;
        } else if x.sec < y.sec {
            return -1;
        } else if x.sec > y.sec {
            return 1;
        }
    }
    0
}

fn sgn(v: i32) -> i32 {
    v.signum()
}

fn cmp_preserve_replace(x: &[u8], y: &[u8], invert: bool) -> i32 {
    let (p, n) = if invert { (-1, 1) } else { (1, -1) };
    let mut i = 0;
    while i < x.len() && i < y.len() {
        if y[i] == 0x9 || y[i] == 0xa || y[i] == 0xd {
            if x[i] != 0x20 {
                return if (x[i] as i32 - 0x20) < 0 { n } else { p };
            }
        } else {
            let tmp = x[i] as i32 - y[i] as i32;
            if tmp < 0 {
                return n;
            }
            if tmp > 0 {
                return p;
            }
        }
        i += 1;
    }
    if i < x.len() {
        return p;
    }
    if i < y.len() {
        return n;
    }
    0
}

fn cmp_preserve_collapse(x: &[u8], y: &[u8], invert: bool) -> i32 {
    let (p, n) = if invert { (-1, 1) } else { (1, -1) };
    let (mut i, mut j) = (0, 0);
    while j < y.len() && is_blank(y[j]) {
        j += 1;
    }
    while i < x.len() && j < y.len() {
        if is_blank(y[j]) {
            if x[i] != 0x20 {
                return if (x[i] as i32 - 0x20) < 0 { n } else { p };
            }
            i += 1;
            j += 1;
            while j < y.len() && is_blank(y[j]) {
                j += 1;
            }
        } else {
            let tmp = x[i] as i32 - y[j] as i32;
            i += 1;
            j += 1;
            if tmp < 0 {
                return n;
            }
            if tmp > 0 {
                return p;
            }
        }
    }
    if i < x.len() {
        return p;
    }
    if j < y.len() {
        while j < y.len() && is_blank(y[j]) {
            j += 1;
        }
        if j < y.len() {
            return n;
        }
    }
    0
}

fn cmp_replace_collapse(x: &[u8], y: &[u8], invert: bool) -> i32 {
    let (p, n) = if invert { (-1, 1) } else { (1, -1) };
    let (mut i, mut j) = (0, 0);
    while j < y.len() && is_blank(y[j]) {
        j += 1;
    }
    while i < x.len() && j < y.len() {
        if is_blank(y[j]) {
            if !is_blank(x[i]) {
                return if (x[i] as i32 - 0x20) < 0 { n } else { p };
            }
            i += 1;
            j += 1;
            while j < y.len() && is_blank(y[j]) {
                j += 1;
            }
        } else {
            if is_blank(x[i]) {
                return if (0x20 - y[j] as i32) < 0 { n } else { p };
            }
            let tmp = x[i] as i32 - y[j] as i32;
            i += 1;
            j += 1;
            if tmp < 0 {
                return -1;
            }
            if tmp > 0 {
                return 1;
            }
        }
    }
    if i < x.len() {
        return p;
    }
    if j < y.len() {
        while j < y.len() && is_blank(y[j]) {
            j += 1;
        }
        if j < y.len() {
            return n;
        }
    }
    0
}

fn cmp_replaced(x: &[u8], y: &[u8]) -> i32 {
    let mut i = 0;
    while i < x.len() && i < y.len() {
        if is_blank(y[i]) {
            if !is_blank(x[i]) {
                return if (x[i] as i32 - 0x20) < 0 { -1 } else { 1 };
            }
        } else {
            if is_blank(x[i]) {
                return if (0x20 - y[i] as i32) < 0 { -1 } else { 1 };
            }
            let tmp = x[i] as i32 - y[i] as i32;
            if tmp != 0 {
                return sgn(tmp);
            }
        }
        i += 1;
    }
    if i < x.len() {
        return 1;
    }
    if i < y.len() {
        return -1;
    }
    0
}

fn cmp_norm(x: &[u8], y: &[u8]) -> i32 {
    let (mut i, mut j) = (0, 0);
    while i < x.len() && is_blank(x[i]) {
        i += 1;
    }
    while j < y.len() && is_blank(y[j]) {
        j += 1;
    }
    while i < x.len() && j < y.len() {
        if is_blank(x[i]) {
            if !is_blank(y[j]) {
                return x[i] as i32 - y[j] as i32;
            }
            while i < x.len() && is_blank(x[i]) {
                i += 1;
            }
            while j < y.len() && is_blank(y[j]) {
                j += 1;
            }
        } else {
            let tmp = x[i] as i32 - y[j] as i32;
            i += 1;
            j += 1;
            if tmp != 0 {
                return sgn(tmp);
            }
        }
    }
    if i < x.len() {
        while i < x.len() && is_blank(x[i]) {
            i += 1;
        }
        if i < x.len() {
            return 1;
        }
    }
    if j < y.len() {
        while j < y.len() && is_blank(y[j]) {
            j += 1;
        }
        if j < y.len() {
            return -1;
        }
    }
    0
}

fn compare_floats(x: &Val, y: &Val) -> i32 {
    let d = |v: &Val| -> Option<f64> {
        match v.data {
            ValData::Double(d) => Some(d),
            ValData::Float(f) => Some(f as f64),
            _ => None,
        }
    };
    let (d1, d2) = match (d(x), d(y)) {
        (Some(a), Some(b)) => (a, b),
        _ => return -2,
    };
    if d1.is_nan() {
        return if d2.is_nan() { 0 } else { 1 };
    }
    if d2.is_nan() {
        return -1;
    }
    if d1 == f64::INFINITY {
        return if d2 == f64::INFINITY { 0 } else { 1 };
    }
    if d2 == f64::INFINITY {
        return -1;
    }
    if d1 == f64::NEG_INFINITY {
        return if d2 == f64::NEG_INFINITY { 0 } else { -1 };
    }
    if d2 == f64::NEG_INFINITY {
        return 1;
    }
    if d1 < d2 {
        -1
    } else if d1 > d2 {
        1
    } else {
        0
    }
}

fn is_decimal_type(t: i32) -> bool {
    matches!(
        t,
        DECIMAL
            | INTEGER
            | NPINTEGER
            | NINTEGER
            | NNINTEGER
            | PINTEGER
            | INT
            | UINT
            | LONG
            | ULONG
            | SHORT
            | USHORT
            | BYTE
            | UBYTE
    )
}

fn is_date_type(t: i32) -> bool {
    matches!(
        t,
        DATETIME | TIME | GDAY | GMONTH | GMONTHDAY | GYEAR | DATE | GYEARMONTH
    )
}

fn is_string_type(t: i32) -> bool {
    matches!(
        t,
        ANYSIMPLETYPE
            | STRING
            | NORMSTRING
            | TOKEN
            | LANGUAGE
            | NMTOKEN
            | NAME
            | NCNAME
            | ID
            | IDREF
            | ENTITY
            | ANYURI
    )
}

/// xmlSchemaCompareValuesInternal: -1, 0, 1, 2 (incomparable order) or -2
pub fn compare_values_ext(
    xtype: i32,
    x: Option<&Val>,
    xvalue: Option<&str>,
    xws: i32,
    ytype: i32,
    y: Option<&Val>,
    yvalue: Option<&str>,
    yws: i32,
) -> i32 {
    match xtype {
        UNKNOWN | ANYTYPE => -2,
        t if is_decimal_type(t) => {
            let (x, y) = match (x.and_then(|v| v.decimal()), y.and_then(|v| v.decimal())) {
                (Some(a), Some(b)) => (a, b),
                _ => return -2,
            };
            if ytype == xtype || is_decimal_type(ytype) {
                return sgn(decimal_compare(x, y));
            }
            -2
        }
        DURATION => match (x, y) {
            (
                Some(Val {
                    data: ValData::Dur(a),
                    ..
                }),
                Some(Val {
                    data: ValData::Dur(b),
                    ..
                }),
            ) if ytype == DURATION => compare_durations(a, b),
            _ => -2,
        },
        t if is_date_type(t) => {
            let (x, y) = match (x, y) {
                (Some(a), Some(b)) => (a, b),
                _ => return -2,
            };
            if is_date_type(ytype) {
                return compare_dates(x.typ, &x.date(), y.typ, &y.date());
            }
            -2
        }
        t if is_string_type(t) => {
            let xv = match x {
                Some(v) => v.str_value().unwrap_or(""),
                None => xvalue.unwrap_or(""),
            };
            let yv = match y {
                Some(v) => v.str_value().unwrap_or(""),
                None => yvalue.unwrap_or(""),
            };
            if ytype == QNAME {
                return -2;
            }
            if is_string_type(ytype) {
                let (xb, yb) = (xv.as_bytes(), yv.as_bytes());
                match xws {
                    WS_PRESERVE => match yws {
                        WS_PRESERVE => return if xv == yv { 0 } else { 2 },
                        WS_REPLACE => return cmp_preserve_replace(xb, yb, false),
                        WS_COLLAPSE => return cmp_preserve_collapse(xb, yb, false),
                        _ => {}
                    },
                    WS_REPLACE => match yws {
                        WS_PRESERVE => return cmp_preserve_replace(yb, xb, true),
                        WS_REPLACE => return cmp_replaced(xb, yb),
                        WS_COLLAPSE => return cmp_replace_collapse(xb, yb, false),
                        _ => {}
                    },
                    WS_COLLAPSE => match yws {
                        WS_PRESERVE => return cmp_preserve_collapse(yb, xb, true),
                        WS_REPLACE => return cmp_replace_collapse(yb, xb, true),
                        WS_COLLAPSE => return cmp_norm(xb, yb),
                        _ => {}
                    },
                    _ => return -2,
                }
            }
            -2
        }
        QNAME | NOTATION => match (x, y) {
            (
                Some(Val {
                    data: ValData::QName { name: n1, uri: u1 },
                    ..
                }),
                Some(Val {
                    data: ValData::QName { name: n2, uri: u2 },
                    ..
                }),
            ) if ytype == QNAME || ytype == NOTATION => {
                if n1 == n2 && u1 == u2 {
                    0
                } else {
                    2
                }
            }
            _ => -2,
        },
        FLOAT | DOUBLE => match (x, y) {
            (Some(a), Some(b)) if ytype == FLOAT || ytype == DOUBLE => compare_floats(a, b),
            _ => -2,
        },
        BOOLEAN => match (x, y) {
            (
                Some(Val {
                    data: ValData::Bool(a),
                    ..
                }),
                Some(Val {
                    data: ValData::Bool(b),
                    ..
                }),
            ) if ytype == BOOLEAN => {
                if a == b {
                    0
                } else if !a {
                    -1
                } else {
                    1
                }
            }
            _ => -2,
        },
        HEXBINARY => match (x, y) {
            (
                Some(Val {
                    data: ValData::Hex { s: s1, total: t1 },
                    ..
                }),
                Some(Val {
                    data: ValData::Hex { s: s2, total: t2 },
                    ..
                }),
            ) if ytype == HEXBINARY => {
                if t1 == t2 {
                    match s1.cmp(s2) {
                        std::cmp::Ordering::Greater => 1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Less => -1,
                    }
                } else if t1 > t2 {
                    1
                } else {
                    -1
                }
            }
            _ => -2,
        },
        BASE64BINARY => match (x, y) {
            (
                Some(Val {
                    data: ValData::Base64 { s: s1, total: t1 },
                    ..
                }),
                Some(Val {
                    data: ValData::Base64 { s: s2, total: t2 },
                    ..
                }),
            ) if ytype == BASE64BINARY => {
                if t1 == t2 {
                    match s1.cmp(s2) {
                        std::cmp::Ordering::Greater => 1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Less => -1,
                    }
                } else if t1 > t2 {
                    1
                } else {
                    -1
                }
            }
            _ => -2,
        },
        _ => -2,
    }
}

fn val_ws(v: &Val) -> i32 {
    if v.typ == STRING {
        WS_PRESERVE
    } else if v.typ == NORMSTRING {
        WS_REPLACE
    } else {
        WS_COLLAPSE
    }
}

/// xmlSchemaCompareValues
pub fn compare_values(x: &Val, y: &Val) -> i32 {
    compare_values_ext(
        x.typ,
        Some(x),
        None,
        val_ws(x),
        y.typ,
        Some(y),
        None,
        val_ws(y),
    )
}

/// xmlSchemaCompareValuesWhtsp
pub fn compare_values_whtsp(x: &Val, xws: i32, y: &Val, yws: i32) -> i32 {
    compare_values_ext(x.typ, Some(x), None, xws, y.typ, Some(y), None, yws)
}

// ---------------------------------------------------------------------------
// Facets

/// A facet (xmlSchemaFacet) as far as value checking is concerned.
#[derive(Clone, Debug)]
pub struct Facet {
    pub typ: i32,
    pub value: String,
    pub val: Option<Val>,
    pub regexp: Option<Rc<Regexp>>,
    pub fixed: bool,
    pub whitespace: i32,
}

impl Facet {
    pub fn new(typ: i32, value: &str) -> Facet {
        Facet {
            typ,
            value: value.to_string(),
            val: None,
            regexp: None,
            fixed: false,
            whitespace: WS_UNKNOWN,
        }
    }

    /// The minLength 1 facet of the built-in list types.
    pub fn min_length_one() -> Facet {
        let mut f = Facet::new(FACET_MINLENGTH, "1");
        f.val = Some(Val::new(NNINTEGER, ValData::Decimal(Decimal::from_int(1))));
        f
    }

    /// xmlSchemaGetFacetValueAsULong
    pub fn as_ulong(&self) -> u64 {
        self.val
            .as_ref()
            .and_then(|v| v.decimal())
            .map(|d| d.as_ulong())
            .unwrap_or(0)
    }

    fn int_val(&self, types: &[i32]) -> Option<&Decimal> {
        let v = self.val.as_ref()?;
        if !types.contains(&v.typ) {
            return None;
        }
        let d = v.decimal()?;
        if !d.is_integer() {
            return None;
        }
        Some(d)
    }
}

/// xmlSchemaValidateListSimpleTypeFacet: (ret, expected length)
pub fn validate_list_simple_type_facet(facet: &Facet, value: &str, actual_len: u64) -> (i32, u64) {
    match facet.typ {
        FACET_LENGTH => {
            if actual_len != facet.as_ulong() {
                return (E::SCHEMAV_CVC_LENGTH_VALID, facet.as_ulong());
            }
        }
        FACET_MINLENGTH => {
            if actual_len < facet.as_ulong() {
                return (E::SCHEMAV_CVC_MINLENGTH_VALID, facet.as_ulong());
            }
        }
        FACET_MAXLENGTH => {
            if actual_len > facet.as_ulong() {
                return (E::SCHEMAV_CVC_MAXLENGTH_VALID, facet.as_ulong());
            }
        }
        _ => {
            return (
                validate_facet_internal(facet, WS_UNKNOWN, UNKNOWN, Some(value), None, WS_UNKNOWN),
                0,
            );
        }
    }
    (0, 0)
}

fn value_length(valtype: i32, value: Option<&str>, val: Option<&Val>, ws: i32) -> Option<u64> {
    if let Some(v) = val {
        match &v.data {
            ValData::Hex { total, .. } => return Some(*total as u64),
            ValData::Base64 { total, .. } => return Some(*total as u64),
            _ => {}
        }
    }
    let len = match valtype {
        STRING | NORMSTRING => {
            let value = value.unwrap_or("");
            if ws == WS_UNKNOWN {
                if valtype == STRING {
                    utf8_len(value)
                } else {
                    norm_len(value)
                }
            } else if ws == WS_COLLAPSE {
                norm_len(value)
            } else {
                utf8_len(value)
            }
        }
        IDREF | TOKEN | LANGUAGE | NMTOKEN | NAME | NCNAME | ID | ANYURI => {
            value.map(norm_len).unwrap_or(0)
        }
        QNAME | NOTATION => return None,
        _ => 0,
    };
    Some(len.max(0) as u64)
}

/// xmlSchemaValidateLengthFacetWhtsp: (ret, actual length)
pub fn validate_length_facet_whtsp(
    facet: &Facet,
    valtype: i32,
    value: Option<&str>,
    val: Option<&Val>,
    ws: i32,
) -> (i32, u64) {
    if !matches!(facet.typ, FACET_LENGTH | FACET_MAXLENGTH | FACET_MINLENGTH) {
        return (-1, 0);
    }
    if facet.int_val(&[DECIMAL, NNINTEGER]).is_none() {
        return (-1, 0);
    }
    let len = match value_length(valtype, value, val, ws) {
        Some(l) => l,
        None => return (0, 0),
    };
    let fv = facet.as_ulong();
    let r = match facet.typ {
        FACET_LENGTH if len != fv => E::SCHEMAV_CVC_LENGTH_VALID,
        FACET_MINLENGTH if len < fv => E::SCHEMAV_CVC_MINLENGTH_VALID,
        FACET_MAXLENGTH if len > fv => E::SCHEMAV_CVC_MAXLENGTH_VALID,
        _ => 0,
    };
    (r, len)
}

/// xmlSchemaValidateFacetInternal
pub fn validate_facet_internal(
    facet: &Facet,
    fws: i32,
    valtype: i32,
    value: Option<&str>,
    val: Option<&Val>,
    ws: i32,
) -> i32 {
    match facet.typ {
        FACET_PATTERN => {
            let mut value = match value {
                Some(v) => v,
                None => return -1,
            };
            if let Some(v) = val {
                if let ValData::Str(s) = &v.data {
                    if (STRING..=NORMSTRING).contains(&v.typ)
                        || ((TOKEN..=ENTITIES).contains(&v.typ) && v.typ != QNAME)
                    {
                        value = s;
                    }
                }
            }
            let re = match &facet.regexp {
                Some(r) => r,
                None => return -1,
            };
            let r = re.exec(value);
            if r == 1 {
                0
            } else if r == 0 {
                E::SCHEMAV_CVC_PATTERN_VALID
            } else {
                r
            }
        }
        FACET_MAXEXCLUSIVE | FACET_MAXINCLUSIVE | FACET_MINEXCLUSIVE | FACET_MININCLUSIVE => {
            let (v, f) = match (val, facet.val.as_ref()) {
                (Some(a), Some(b)) => (a, b),
                _ => return -1,
            };
            let r = compare_values(v, f);
            if r == -2 {
                return -1;
            }
            match facet.typ {
                FACET_MAXEXCLUSIVE => {
                    if r == -1 {
                        0
                    } else {
                        E::SCHEMAV_CVC_MAXEXCLUSIVE_VALID
                    }
                }
                FACET_MAXINCLUSIVE => {
                    if r == -1 || r == 0 {
                        0
                    } else {
                        E::SCHEMAV_CVC_MAXINCLUSIVE_VALID
                    }
                }
                FACET_MINEXCLUSIVE => {
                    if r == 1 {
                        0
                    } else {
                        E::SCHEMAV_CVC_MINEXCLUSIVE_VALID
                    }
                }
                _ => {
                    if r == 1 || r == 0 {
                        0
                    } else {
                        E::SCHEMAV_CVC_MININCLUSIVE_VALID
                    }
                }
            }
        }
        FACET_WHITESPACE => 0,
        FACET_ENUMERATION => {
            if ws == WS_UNKNOWN {
                if Some(facet.value.as_str()) == value {
                    return 0;
                }
            } else {
                let fv = match &facet.val {
                    Some(v) => v,
                    None => return -1,
                };
                let r = compare_values_ext(
                    fv.typ,
                    Some(fv),
                    Some(&facet.value),
                    fws,
                    valtype,
                    val,
                    value,
                    ws,
                );
                if r == -2 {
                    return -1;
                }
                if r == 0 {
                    return 0;
                }
            }
            E::SCHEMAV_CVC_ENUMERATION_VALID
        }
        FACET_LENGTH | FACET_MAXLENGTH | FACET_MINLENGTH => {
            if valtype == QNAME || valtype == NOTATION {
                return 0;
            }
            if facet.int_val(&[DECIMAL, NNINTEGER]).is_none() {
                return -1;
            }
            let len = value_length(valtype, value, val, ws).unwrap_or(0);
            let fv = facet.as_ulong();
            match facet.typ {
                FACET_LENGTH if len != fv => E::SCHEMAV_CVC_LENGTH_VALID,
                FACET_MINLENGTH if len < fv => E::SCHEMAV_CVC_MINLENGTH_VALID,
                FACET_MAXLENGTH if len > fv => E::SCHEMAV_CVC_MAXLENGTH_VALID,
                _ => 0,
            }
        }
        FACET_TOTALDIGITS | FACET_FRACTIONDIGITS => {
            if facet.int_val(&[PINTEGER, NNINTEGER]).is_none() {
                return -1;
            }
            let d = match val {
                Some(v) if is_decimal_type(v.typ) => match v.decimal() {
                    Some(d) => d,
                    None => return -1,
                },
                _ => return -1,
            };
            let fv = facet.as_ulong();
            if facet.typ == FACET_TOTALDIGITS {
                if d.significant_digits() > fv {
                    return E::SCHEMAV_CVC_TOTALDIGITS_VALID;
                }
            } else {
                let fd = if d.is_integer() {
                    0
                } else {
                    d.fractional as u64
                };
                if fd > fv {
                    return E::SCHEMAV_CVC_FRACTIONDIGITS_VALID;
                }
            }
            0
        }
        _ => 0,
    }
}

/// xmlSchemaValidateFacet
pub fn validate_facet(base: Option<i32>, facet: &Facet, value: &str, val: Option<&Val>) -> i32 {
    if let Some(v) = val {
        return validate_facet_internal(facet, WS_UNKNOWN, v.typ, Some(value), Some(v), WS_UNKNOWN);
    }
    if let Some(b) = base {
        return validate_facet_internal(facet, WS_UNKNOWN, b, Some(value), None, WS_UNKNOWN);
    }
    -1
}

/// xmlSchemaValidateFacetWhtsp
pub fn validate_facet_whtsp(
    facet: &Facet,
    fws: i32,
    valtype: i32,
    value: &str,
    val: Option<&Val>,
    ws: i32,
) -> i32 {
    validate_facet_internal(facet, fws, valtype, Some(value), val, ws)
}

// ---------------------------------------------------------------------------
// Canonical values

/// "%.14g"
pub fn fmt_g14(v: f64) -> String {
    fmt_g(v, 14)
}

/// printf("%.<prec>g")
pub fn fmt_g(v: f64, prec: usize) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    let p = if prec == 0 { 1 } else { prec };
    let e = format!("{:.*e}", p - 1, v);
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if exp < -4 || exp >= p as i32 {
        let mant = trim_zeros(mant);
        format!(
            "{}e{}{:02}",
            mant,
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        trim_zeros(&format!("{:.*}", decimals, v))
    }
}

fn trim_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

/// printf("%02.14g")
fn fmt_sec(v: f64) -> String {
    let s = fmt_g14(v);
    if s.len() < 2 { format!("{:0>2}", s) } else { s }
}

/// "%01.14e"
fn fmt_e14(v: f64) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let e = format!("{:.14e}", v);
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    format!(
        "{}e{}{:02}",
        mant,
        if exp < 0 { '-' } else { '+' },
        exp.abs()
    )
}

/// xmlSchemaGetCanonValue: (0 ok / 1 unsupported, value)
pub fn get_canon_value(val: &Val) -> (i32, String) {
    match (&val.data, val.typ) {
        (ValData::Str(s), STRING) => (0, s.clone()),
        (ValData::Str(s), NORMSTRING) => (0, whitespace_replace(s).unwrap_or_else(|| s.clone())),
        (
            ValData::Str(s),
            TOKEN | LANGUAGE | NMTOKEN | NAME | NCNAME | ID | IDREF | ENTITY | ANYURI,
        ) => (0, collapse_string(s).unwrap_or_else(|| s.clone())),
        (ValData::QName { name, .. }, NOTATION) => {
            (0, collapse_string(name).unwrap_or_else(|| name.clone()))
        }
        (ValData::QName { name, uri }, QNAME) => match uri {
            None => (0, name.clone()),
            // libxml2 appends the URI twice here
            Some(u) => (0, format!("{{{}}}{}", u, u)),
        },
        (ValData::Decimal(d), DECIMAL) => (0, d.s.strip_prefix('+').unwrap_or(&d.s).to_string()),
        (ValData::Decimal(d), _) => {
            let s = d.s.strip_prefix('+').unwrap_or(&d.s);
            let n = d.integral as usize + if d.s.starts_with('-') { 1 } else { 0 };
            (0, s.chars().take(n).collect())
        }
        (ValData::Bool(b), _) => (0, if *b { "true".into() } else { "false".into() }),
        (ValData::Dur(d), _) => {
            let year = (d.mon.abs() as f64 / 12.0).floor() as u64;
            let mon = d.mon.unsigned_abs() - 12 * year;
            let day = (d.sec.abs() / 86400.0).floor() as u64;
            let mut left = d.sec.abs() - day as f64 * 86400.0;
            let (mut hour, mut min, mut sec) = (0u64, 0u64, 0.0f64);
            if left > 0.0 {
                hour = (left / 3600.0).floor() as u64;
                left -= hour as f64 * 3600.0;
                if left > 0.0 {
                    min = (left / 60.0).floor() as u64;
                    sec = left - min as f64 * 60.0;
                }
            }
            let neg = if d.mon < 0 || d.sec < 0.0 { "" } else { "-" };
            (
                0,
                format!(
                    "{}P{}Y{}M{}DT{}H{}M{}S",
                    neg,
                    year,
                    mon,
                    day,
                    hour,
                    min,
                    fmt_g14(sec)
                ),
            )
        }
        (ValData::Date(d), GYEAR) => (0, format!("{:04}", d.year)),
        (ValData::Date(d), GMONTH) => (0, format!("--{:02}", d.mon)),
        (ValData::Date(d), GDAY) => (0, format!("---{:02}", d.day)),
        (ValData::Date(d), GMONTHDAY) => (0, format!("--{:02}-{:02}", d.mon, d.day)),
        (ValData::Date(d), GYEARMONTH) => {
            if d.year < 0 {
                (0, format!("-{:04}-{:02}", d.year.abs(), d.mon))
            } else {
                (0, format!("{:04}-{:02}", d.year, d.mon))
            }
        }
        (ValData::Date(d), TIME) => {
            if d.tz_flag {
                let (_, n) = date_normalize(TIME, d, 0.0);
                (0, format!("{:02}:{:02}:{}Z", n.hour, n.min, fmt_sec(n.sec)))
            } else {
                (0, format!("{:02}:{:02}:{}", d.hour, d.min, fmt_sec(d.sec)))
            }
        }
        (ValData::Date(d), DATE) => {
            if d.tz_flag {
                let (_, n) = date_normalize(DATE, d, 0.0);
                (0, format!("{:04}-{:02}-{:02}Z", n.year, n.mon, n.day))
            } else {
                (0, format!("{:04}-{:02}-{:02}", d.year, d.mon, d.day))
            }
        }
        (ValData::Date(d), DATETIME) => {
            if d.tz_flag {
                let (_, n) = date_normalize(DATETIME, d, 0.0);
                (
                    0,
                    format!(
                        "{:04}-{:02}-{:02}T{:02}:{:02}:{}Z",
                        n.year,
                        n.mon,
                        n.day,
                        n.hour,
                        n.min,
                        fmt_sec(n.sec)
                    ),
                )
            } else {
                (
                    0,
                    format!(
                        "{:04}-{:02}-{:02}T{:02}:{:02}:{}",
                        d.year,
                        d.mon,
                        d.day,
                        d.hour,
                        d.min,
                        fmt_sec(d.sec)
                    ),
                )
            }
        }
        (ValData::Hex { s, .. }, _) => (0, s.clone()),
        (ValData::Base64 { s, .. }, _) => (0, s.clone()),
        (ValData::Float(f), _) => (0, fmt_e14(*f as f64)),
        (ValData::Double(f), _) => (0, fmt_e14(*f)),
        _ => (1, "???".into()),
    }
}

/// xmlSchemaGetCanonValueWhtsp
pub fn get_canon_value_whtsp(val: &Val, ws: i32) -> (i32, String) {
    if ws == WS_UNKNOWN || ws > WS_COLLAPSE {
        return (-1, String::new());
    }
    match (&val.data, val.typ) {
        (ValData::Str(s), STRING) => {
            let r = if ws == WS_COLLAPSE {
                collapse_string(s)
            } else if ws == WS_REPLACE {
                whitespace_replace(s)
            } else {
                None
            };
            (0, r.unwrap_or_else(|| s.clone()))
        }
        (ValData::Str(s), NORMSTRING) => {
            let r = if ws == WS_COLLAPSE {
                collapse_string(s)
            } else {
                whitespace_replace(s)
            };
            (0, r.unwrap_or_else(|| s.clone()))
        }
        _ => get_canon_value(val),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(vt: i32, v: &str) -> i32 {
        val_predef_type_node(vt, v, true, None).0
    }

    #[test]
    fn lexical() {
        assert_eq!(ok(INTEGER, "123"), 0);
        assert_eq!(ok(INTEGER, " 123 "), 0);
        assert_eq!(ok(INTEGER, "abc"), 1);
        assert_eq!(ok(INTEGER, "1.5"), 1);
        assert_eq!(ok(BYTE, "128"), 1);
        assert_eq!(ok(BYTE, "-128"), 0);
        assert_eq!(ok(DECIMAL, "1.50"), 0);
        assert_eq!(ok(DECIMAL, "."), 1);
        assert_eq!(ok(DECIMAL, "0."), 0);
        assert_eq!(ok(BOOLEAN, "true"), 0);
        assert_eq!(ok(BOOLEAN, "yes"), 1);
        assert_eq!(ok(DATE, "2004-02-29"), 0);
        assert_eq!(ok(DATE, "2003-02-29"), 1);
        assert_eq!(ok(DATETIME, "2004-02-29T12:00:00Z"), 0);
        assert_eq!(ok(TIME, "24:00:00"), 0);
        assert_eq!(ok(DURATION, "P1Y2M3DT10H30M"), 0);
        assert_eq!(ok(DURATION, "P1Y2MT"), 1);
        assert_eq!(ok(DOUBLE, "1e10"), 0);
        assert_eq!(ok(DOUBLE, "INF"), 0);
        assert_eq!(ok(FLOAT, "1.2.3"), 1);
        assert_eq!(ok(NCNAME, "a:b"), 1);
        assert_eq!(ok(NAME, "a:b"), 0);
        assert_eq!(ok(LANGUAGE, "en-US"), 0);
        assert_eq!(ok(HEXBINARY, "0FB7"), 0);
        assert_eq!(ok(HEXBINARY, "0FB"), 1);
        assert_eq!(ok(BASE64BINARY, "SGVsbG8="), 0);
        assert_eq!(ok(NMTOKENS, "a b c"), 0);
        assert_eq!(ok(NMTOKENS, "   "), 1);
    }

    #[test]
    fn compare() {
        let v = |vt: i32, s: &str| val_predef_type_node(vt, s, true, None).1.unwrap();
        assert_eq!(compare_values(&v(INTEGER, "10"), &v(INTEGER, "9")), 1);
        assert_eq!(compare_values(&v(DECIMAL, "-1.5"), &v(INTEGER, "-1")), -1);
        assert_eq!(
            compare_values(&v(DATE, "2004-01-01"), &v(DATE, "2004-01-02")),
            -1
        );
        assert_eq!(compare_values(&v(DURATION, "P1M"), &v(DURATION, "P30D")), 2);
        assert_eq!(get_canon_value(&v(DECIMAL, "001.500")).1, "1.5");
        assert_eq!(get_canon_value(&v(INTEGER, "+007")).1, "7");
        assert_eq!(get_canon_value(&v(INTEGER, "-007")).1, "-7");
    }
}
