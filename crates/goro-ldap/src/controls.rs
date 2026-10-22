//! Control value encoders/decoders for the known OIDs (pagectrl.c,
//! sortctrl.c, vlvctrl.c, assertion.c, RFC 4527 pre/post read, RFC 3876
//! ValuesReturnFilter).

use crate::ber::{BerError, Reader, TAG_OCTETSTRING, Writer};
use crate::consts::*;
use crate::filter;

/// ldap_create_page_control_value: SEQUENCE { INTEGER size, OCTET cookie }
pub fn page_value(size: i64, cookie: &[u8]) -> Vec<u8> {
    let mut w = Writer::new();
    w.start_seq().int(size).octets(cookie).end();
    w.finish()
}

/// A sort key for the sort request control.
pub struct SortKey {
    pub attr: Vec<u8>,
    pub oid: Option<Vec<u8>>,
    pub reverse: bool,
}

/// ldap_create_sort_control_value
pub fn sort_value(keys: &[SortKey]) -> Vec<u8> {
    let mut w = Writer::new();
    w.start_seq();
    for k in keys {
        w.start_seq();
        w.octets(&k.attr);
        if let Some(oid) = &k.oid {
            w.raw(LDAP_MATCHRULE_IDENTIFIER, oid);
        }
        if k.reverse {
            w.bool_tagged(LDAP_REVERSEORDER_IDENTIFIER, true);
        }
        w.end();
    }
    w.end();
    w.finish()
}

/// ldap_create_vlv_control_value: by index (offset/count) or by value.
#[allow(clippy::too_many_arguments)]
pub fn vlv_value(
    before: i64,
    after: i64,
    attrvalue: Option<&[u8]>,
    offset: i64,
    count: i64,
    context: Option<&[u8]>,
) -> Vec<u8> {
    let mut w = Writer::new();
    w.start_seq();
    w.int(before).int(after);
    match attrvalue {
        None => {
            w.start(LDAP_VLVBYINDEX_IDENTIFIER);
            w.int(offset).int(count);
            w.end();
        }
        Some(v) => {
            w.raw(LDAP_VLVBYVALUE_IDENTIFIER, v);
        }
    }
    if let Some(c) = context {
        w.raw(LDAP_VLVCONTEXT_IDENTIFIER, c);
    }
    w.end();
    w.finish()
}

/// ldap_create_assertion_control_value: a Filter.
pub fn assertion_value(filter: &[u8]) -> Option<Vec<u8>> {
    if filter.is_empty() {
        return None;
    }
    filter::encode(filter)
}

/// pre/post read control value: SEQUENCE OF attribute names (ber "{v}").
pub fn attrs_value(attrs: &[Vec<u8>]) -> Vec<u8> {
    let mut w = Writer::new();
    w.start_seq();
    for a in attrs {
        w.octets(a);
    }
    w.end();
    w.finish()
}

/// ldap_put_vrFilter: SEQUENCE OF SimpleFilterItem.
pub fn valuesreturnfilter_value(filter: &[u8]) -> Option<Vec<u8>> {
    let mut w = Writer::new();
    w.start_seq();
    // the string is a sequence of parenthesized simple items
    let mut s: &[u8] = filter;
    loop {
        s = skip_spaces(s);
        if s.is_empty() {
            break;
        }
        if s.first() == Some(&b'(') {
            // find matching ')'
            let end = right_paren(&s[1..])?;
            let item = &s[1..1 + end];
            filter::put_simple(&mut w, item)?;
            s = &s[1 + end + 1..];
        } else {
            // bare simple
            filter::put_simple(&mut w, s)?;
            s = &[];
        }
    }
    w.end();
    Some(w.finish())
}

fn skip_spaces(s: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    &s[i..]
}
fn right_paren(s: &[u8]) -> Option<usize> {
    let mut balance = 1;
    let mut escape = false;
    let mut i = 0;
    while i < s.len() && balance != 0 {
        if !escape {
            if s[i] == b'(' {
                balance += 1;
            } else if s[i] == b')' {
                balance -= 1;
            }
        }
        escape = s[i] == b'\\' && !escape;
        if balance != 0 {
            i += 1;
        }
    }
    if i < s.len() && balance == 0 {
        Some(i)
    } else {
        None
    }
}

// ---- response control value parsers ----

/// paged results response: SEQUENCE { INTEGER size, OCTET cookie }
pub fn parse_page(val: &[u8]) -> Result<(i64, Vec<u8>), BerError> {
    let mut r = Reader::new(val);
    let mut s = r.seq()?;
    let size = s.int()?;
    let cookie = s.octets()?.to_vec();
    Ok((size, cookie))
}

/// sort response: SEQUENCE { ENUMERATED errcode, [0] OCTET attribute OPTIONAL }
pub fn parse_sortresponse(val: &[u8]) -> Result<(i64, Option<Vec<u8>>), BerError> {
    let mut r = Reader::new(val);
    let mut s = r.seq()?;
    let code = s.enumerated()?;
    let attr = s.optional(LDAP_ATTRTYPES_IDENTIFIER)?.map(|v| v.to_vec());
    Ok((code, attr))
}

/// vlv response: SEQUENCE { INTEGER target, INTEGER count, ENUMERATED errcode,
/// [0] OCTET context OPTIONAL }
pub fn parse_vlvresponse(val: &[u8]) -> Result<(i64, i64, i64, Option<Vec<u8>>), BerError> {
    let mut r = Reader::new(val);
    let mut s = r.seq()?;
    let target = s.int()?;
    let count = s.int()?;
    let err = s.enumerated()?;
    let context = s.optional(LDAP_VLVCONTEXT_IDENTIFIER)?.map(|v| v.to_vec());
    Ok((target, count, err, context))
}

/// pre/post read response: SEQUENCE { OCTET dn, SEQUENCE OF PartialAttribute }
/// PartialAttribute ::= SEQUENCE { type OCTET, vals SET OF OCTET }
#[allow(clippy::type_complexity)]
pub fn parse_readentry(val: &[u8]) -> Result<(Vec<u8>, Vec<(Vec<u8>, Vec<Vec<u8>>)>), BerError> {
    let mut r = Reader::new(val);
    let mut s = r.seq()?;
    let dn = s.octets()?.to_vec();
    let mut attrs = Vec::new();
    while !s.is_empty() {
        let mut a = s.seq()?;
        let name = a.octets()?.to_vec();
        let mut set = a.enter(crate::ber::TAG_SET)?;
        let mut vals = Vec::new();
        while !set.is_empty() {
            vals.push(set.any()?.to_vec());
        }
        attrs.push((name, vals));
    }
    let _ = TAG_OCTETSTRING;
    Ok((dn, attrs))
}
