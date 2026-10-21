//! X509_NAME: parsing, encoding, X509_NAME_oneline(), canonical hash and
//! php_openssl_add_assoc_name_entry().

use crate::asn1;
use crate::digest::Md;
use crate::obj;
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

pub const V_UTF8: u8 = 0x0c;
pub const V_NUMERIC: u8 = 0x12;
pub const V_PRINTABLE: u8 = 0x13;
pub const V_T61: u8 = 0x14;
pub const V_IA5: u8 = 0x16;
pub const V_VISIBLE: u8 = 0x1a;
pub const V_GENERAL: u8 = 0x1b;
pub const V_UNIVERSAL: u8 = 0x1c;
pub const V_BMP: u8 = 0x1e;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ava {
    pub oid: String,
    pub tag: u8,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Name {
    pub rdns: Vec<Vec<Ava>>,
}

impl Name {
    pub fn parse(t: &asn1::Tlv) -> Option<Name> {
        if t.tag != asn1::SEQUENCE {
            return None;
        }
        let mut rdns = Vec::new();
        for set in t.children()? {
            if set.tag != asn1::SET {
                return None;
            }
            let mut rdn = Vec::new();
            for atv in set.children()? {
                let c = atv.children()?;
                if c.len() != 2 {
                    return None;
                }
                rdn.push(Ava {
                    oid: c[0].oid()?,
                    tag: c[1].tag,
                    value: c[1].content.to_vec(),
                });
            }
            rdns.push(rdn);
        }
        Some(Name { rdns })
    }

    pub fn entries(&self) -> impl Iterator<Item = &Ava> {
        self.rdns.iter().flatten()
    }

    pub fn entry_count(&self) -> usize {
        self.rdns.iter().map(|r| r.len()).sum()
    }

    pub fn has_oid(&self, oid: &str) -> bool {
        self.entries().any(|e| e.oid == oid)
    }

    /// Append an entry as a new RDN (X509_NAME_add_entry(.., -1, 0)).
    pub fn push(&mut self, ava: Ava) {
        self.rdns.push(vec![ava]);
    }

    pub fn to_der(&self) -> Vec<u8> {
        let sets: Vec<Vec<u8>> = self
            .rdns
            .iter()
            .map(|rdn| {
                let items: Vec<Vec<u8>> = rdn
                    .iter()
                    .map(|a| asn1::seq(&[asn1::oid(&a.oid), asn1::tlv(a.tag, &a.value)]))
                    .collect();
                asn1::set_of(&items)
            })
            .collect();
        asn1::seq(&sets)
    }

    /// X509_NAME_oneline()
    pub fn oneline(&self) -> String {
        let mut out = String::new();
        for e in self.entries() {
            let sn = obj::by_oid(&e.oid)
                .map(|o| o.sn)
                .unwrap_or_else(|| e.oid.clone());
            out.push('/');
            out.push_str(&sn);
            out.push('=');
            let q = &e.value;
            let mut gs = [true; 4];
            if e.tag == V_GENERAL && q.len() % 4 == 0 {
                let mut nz = [false; 4];
                for (j, &b) in q.iter().enumerate() {
                    if b != 0 {
                        nz[j & 3] = true;
                    }
                }
                if nz[0] || nz[1] || nz[2] {
                    gs = [true; 4];
                } else {
                    gs = [false, false, false, true];
                }
            }
            for (j, &n) in q.iter().enumerate() {
                if !gs[j & 3] {
                    continue;
                }
                if !(b' '..=b'~').contains(&n) {
                    out.push_str(&format!("\\x{:02X}", n));
                } else {
                    out.push(n as char);
                }
            }
        }
        if out.is_empty() {
            out.push('/');
        }
        out
    }

    /// Canonical encoding used for X509_NAME_cmp() and X509_NAME_hash().
    pub fn canon(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for rdn in &self.rdns {
            let items: Vec<Vec<u8>> = rdn
                .iter()
                .map(|a| {
                    let (tag, val) = canon_string(a.tag, &a.value);
                    asn1::seq(&[asn1::oid(&a.oid), asn1::tlv(tag, &val)])
                })
                .collect();
            out.extend(asn1::set_of(&items));
        }
        out
    }

    /// X509_NAME_hash_ex() (SHA1 of the canonical encoding).
    pub fn hash(&self) -> u32 {
        let d = Md::Sha1.digest(&self.canon());
        u32::from_le_bytes([d[0], d[1], d[2], d[3]])
    }

    /// X509_NAME_cmp() == 0
    pub fn eq_canon(&self, other: &Name) -> bool {
        self.canon() == other.canon()
    }

    /// php_openssl_add_assoc_name_entry() into `arr`.
    pub fn add_to_array(&self, arr: &mut PhpArray, shortname: bool) {
        for e in self.entries() {
            let o = obj::by_oid(&e.oid);
            let sname = match &o {
                Some(o) => {
                    if shortname {
                        o.sn.clone()
                    } else {
                        o.ln.clone()
                    }
                }
                None => {
                    if shortname {
                        "UNDEF".to_string()
                    } else {
                        "undefined".to_string()
                    }
                }
            };
            let val = if e.tag == V_UTF8 {
                Some(e.value.clone())
            } else {
                to_utf8(e.tag, &e.value)
            };
            let Some(val) = val else {
                crate::err::push(crate::err::LIB_ASN1, 0x8010c & 0x7fffff, "unsupported");
                continue;
            };
            let key = ArrayKey::String(PhpString::from_bytes(sname.as_bytes()));
            let v = Value::String(PhpString::from_vec(val));
            match arr.get(&key).cloned() {
                Some(Value::Array(a)) => {
                    a.borrow_mut().push(v);
                }
                Some(Value::String(s)) => {
                    let mut tmp = PhpArray::new();
                    tmp.push(Value::String(s));
                    tmp.push(v);
                    arr.set(key, Value::Array(Rc::new(RefCell::new(tmp))));
                }
                _ => {
                    arr.set(key, v);
                }
            }
        }
    }
}

/// ASN1_STRING_to_UTF8()
pub fn to_utf8(tag: u8, v: &[u8]) -> Option<Vec<u8>> {
    let chars: Vec<u32> = match tag {
        V_UTF8 => return Some(v.to_vec()),
        V_NUMERIC | V_PRINTABLE | V_T61 | V_IA5 | V_VISIBLE => {
            v.iter().map(|&b| b as u32).collect()
        }
        V_BMP => {
            if !v.len().is_multiple_of(2) {
                return None;
            }
            v.chunks(2)
                .map(|c| ((c[0] as u32) << 8) | c[1] as u32)
                .collect()
        }
        V_UNIVERSAL => {
            if !v.len().is_multiple_of(4) {
                return None;
            }
            v.chunks(4)
                .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        }
        _ => return None,
    };
    let mut out = Vec::new();
    for c in chars {
        utf8_encode(c, &mut out);
    }
    Some(out)
}

pub fn utf8_encode(c: u32, out: &mut Vec<u8>) {
    if c < 0x80 {
        out.push(c as u8);
    } else if c < 0x800 {
        out.push(0xc0 | (c >> 6) as u8);
        out.push(0x80 | (c & 0x3f) as u8);
    } else if c < 0x10000 {
        out.push(0xe0 | (c >> 12) as u8);
        out.push(0x80 | ((c >> 6) & 0x3f) as u8);
        out.push(0x80 | (c & 0x3f) as u8);
    } else {
        out.push(0xf0 | (c >> 18) as u8);
        out.push(0x80 | ((c >> 12) & 0x3f) as u8);
        out.push(0x80 | ((c >> 6) & 0x3f) as u8);
        out.push(0x80 | (c & 0x3f) as u8);
    }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// asn1_string_canon()
fn canon_string(tag: u8, v: &[u8]) -> (u8, Vec<u8>) {
    if !matches!(
        tag,
        V_UTF8 | V_BMP | V_UNIVERSAL | V_PRINTABLE | V_T61 | V_IA5 | V_VISIBLE
    ) {
        return (tag, v.to_vec());
    }
    let s = to_utf8(tag, v).unwrap_or_default();
    let mut start = 0;
    let mut end = s.len();
    while start < end && is_space(s[start]) {
        start += 1;
    }
    while end > start && is_space(s[end - 1]) {
        end -= 1;
    }
    let from = &s[start..end];
    let mut out = Vec::with_capacity(from.len());
    let mut i = 0;
    while i < from.len() {
        let c = from[i];
        if c >= 0x80 {
            out.push(c);
            i += 1;
        } else if is_space(c) {
            out.push(b' ');
            while i < from.len() && is_space(from[i]) {
                i += 1;
            }
        } else {
            out.push(c.to_ascii_lowercase());
            i += 1;
        }
    }
    (V_UTF8, out)
}

// ---------------------------------------------------------------------------
// String type selection (ASN1_STRING_set_by_NID / ASN1_mbstring_ncopy)
// ---------------------------------------------------------------------------

use crate::req::{
    B_ASN1_BMPSTRING, B_ASN1_IA5STRING, B_ASN1_PRINTABLESTRING, B_ASN1_T61STRING,
    B_ASN1_UNIVERSALSTRING, B_ASN1_UTF8STRING,
};
pub const B_ASN1_NUMERICSTRING: u64 = 0x0001;
pub const DIRSTRING_TYPE: u64 =
    B_ASN1_PRINTABLESTRING | B_ASN1_T61STRING | B_ASN1_BMPSTRING | B_ASN1_UTF8STRING;
pub const PKCS9STRING_TYPE: u64 = DIRSTRING_TYPE | B_ASN1_IA5STRING;

/// (min chars, max chars, mask, no_global_mask) for the NID string table.
fn string_table(oid: &str) -> Option<(i64, i64, u64, bool)> {
    Some(match oid {
        "2.5.4.3" => (1, 64, DIRSTRING_TYPE, false), // commonName
        "2.5.4.6" => (2, 2, B_ASN1_PRINTABLESTRING, true), // countryName
        "2.5.4.7" => (1, 128, DIRSTRING_TYPE, false), // localityName
        "2.5.4.8" => (1, 128, DIRSTRING_TYPE, false), // stateOrProvinceName
        "2.5.4.10" => (1, 64, DIRSTRING_TYPE, false), // organizationName
        "2.5.4.11" => (1, 64, DIRSTRING_TYPE, false), // organizationalUnitName
        "1.2.840.113549.1.9.1" => (1, 128, B_ASN1_IA5STRING, true), // emailAddress
        "1.2.840.113549.1.9.2" => (1, -1, PKCS9STRING_TYPE, false), // unstructuredName
        "1.2.840.113549.1.9.7" => (1, -1, PKCS9STRING_TYPE, false), // challengePassword
        "1.2.840.113549.1.9.8" => (1, -1, DIRSTRING_TYPE, false), // unstructuredAddress
        "2.5.4.42" => (1, 32768, DIRSTRING_TYPE, false), // givenName
        "2.5.4.4" => (1, 32768, DIRSTRING_TYPE, false), // surname
        "2.5.4.43" => (1, 32768, DIRSTRING_TYPE, false), // initials
        "2.5.4.5" => (1, 64, B_ASN1_PRINTABLESTRING, true), // serialNumber
        "1.2.840.113549.1.9.20" => (-1, -1, B_ASN1_BMPSTRING, true), // friendlyName
        "2.5.4.41" => (1, 32768, DIRSTRING_TYPE, false), // name
        "2.5.4.46" => (-1, -1, B_ASN1_PRINTABLESTRING, true), // dnQualifier
        "0.9.2342.19200300.100.1.25" => (1, -1, B_ASN1_IA5STRING, true), // domainComponent
        "1.3.6.1.4.1.311.17.1" => (-1, -1, B_ASN1_BMPSTRING, true), // ms CSP name
        "1.3.6.1.4.1.311.60.2.1.3" => (2, 2, B_ASN1_PRINTABLESTRING, true), // jurisdictionC
        _ => return None,
    })
}

fn is_asn1_print(c: u32) -> bool {
    c < 0x80
        && ((c as u8).is_ascii_alphanumeric()
            || matches!(
                c as u8,
                b' ' | b'\'' | b'(' | b')' | b'+' | b',' | b'-' | b'.' | b'/' | b':' | b'=' | b'?'
            ))
}

pub enum StrErr {
    InvalidUtf8,
    TooShort,
    TooLong,
    IllegalChars,
}

/// Build the DER string (tag, content) for a UTF-8 value of attribute `oid`.
pub fn make_string(oid: &str, utf8: &[u8], global_mask: u64) -> Result<(u8, Vec<u8>), StrErr> {
    let (minsize, maxsize, mut mask) = match string_table(oid) {
        Some((mn, mx, m, nomask)) => (mn, mx, if nomask { m } else { m & global_mask }),
        None => (-1, -1, DIRSTRING_TYPE & global_mask),
    };
    let s = std::str::from_utf8(utf8).map_err(|_| StrErr::InvalidUtf8)?;
    let chars: Vec<u32> = s.chars().map(|c| c as u32).collect();
    let n = chars.len() as i64;
    if minsize > 0 && n < minsize {
        return Err(StrErr::TooShort);
    }
    if maxsize > 0 && n > maxsize {
        return Err(StrErr::TooLong);
    }
    for &c in &chars {
        if mask & B_ASN1_NUMERICSTRING != 0
            && !((c as u8).is_ascii_digit() && c < 0x80 || c == 0x20)
        {
            mask &= !B_ASN1_NUMERICSTRING;
        }
        if mask & B_ASN1_PRINTABLESTRING != 0 && !is_asn1_print(c) {
            mask &= !B_ASN1_PRINTABLESTRING;
        }
        if mask & B_ASN1_IA5STRING != 0 && c >= 0x80 {
            mask &= !B_ASN1_IA5STRING;
        }
        if mask & B_ASN1_T61STRING != 0 && c > 0xff {
            mask &= !B_ASN1_T61STRING;
        }
        if mask & B_ASN1_BMPSTRING != 0 && c > 0xffff {
            mask &= !B_ASN1_BMPSTRING;
        }
        if mask == 0 {
            return Err(StrErr::IllegalChars);
        }
    }
    let ascii = |chars: &[u32]| chars.iter().map(|&c| c as u8).collect::<Vec<u8>>();
    Ok(if mask & B_ASN1_NUMERICSTRING != 0 {
        (V_NUMERIC, ascii(&chars))
    } else if mask & B_ASN1_PRINTABLESTRING != 0 {
        (V_PRINTABLE, ascii(&chars))
    } else if mask & B_ASN1_IA5STRING != 0 {
        (V_IA5, ascii(&chars))
    } else if mask & B_ASN1_T61STRING != 0 {
        (V_T61, ascii(&chars))
    } else if mask & B_ASN1_BMPSTRING != 0 {
        (
            V_BMP,
            chars
                .iter()
                .flat_map(|&c| (c as u16).to_be_bytes())
                .collect(),
        )
    } else if mask & B_ASN1_UNIVERSALSTRING != 0 {
        (
            V_UNIVERSAL,
            chars.iter().flat_map(|&c| c.to_be_bytes()).collect(),
        )
    } else if mask & B_ASN1_UTF8STRING != 0 {
        (V_UTF8, utf8.to_vec())
    } else {
        return Err(StrErr::IllegalChars);
    })
}

pub fn push_str_err(e: &StrErr) {
    use crate::err::{LIB_ASN1, push};
    match e {
        StrErr::InvalidUtf8 => push(LIB_ASN1, 134, "invalid utf8string"),
        StrErr::TooShort => push(LIB_ASN1, 152, "string too short"),
        StrErr::TooLong => push(LIB_ASN1, 151, "string too long"),
        StrErr::IllegalChars => push(LIB_ASN1, 124, "illegal characters"),
    }
}
