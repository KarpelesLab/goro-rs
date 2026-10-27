//! OBJ_* lookups over the generated OpenSSL object table, plus objects
//! created at runtime (OBJ_create from `oid_section`).

use crate::objects::OBJECTS;
use std::cell::RefCell;

#[derive(Clone, Debug)]
pub struct Obj {
    pub nid: i64,
    pub sn: String,
    pub ln: String,
    pub oid: String,
}

thread_local! {
    static DYNAMIC: RefCell<Vec<Obj>> = const { RefCell::new(Vec::new()) };
}

const FIRST_DYNAMIC_NID: i64 = 2000;

fn from_static(e: &(i64, &str, &str, &str)) -> Obj {
    Obj {
        nid: e.0,
        sn: e.1.to_string(),
        ln: e.2.to_string(),
        oid: e.3.to_string(),
    }
}

pub fn by_oid(oid: &str) -> Option<Obj> {
    if let Some(e) = OBJECTS.iter().find(|e| e.3 == oid && e.0 != 0) {
        return Some(from_static(e));
    }
    DYNAMIC.with(|d| d.borrow().iter().find(|o| o.oid == oid).cloned())
}

pub fn by_sn(sn: &str) -> Option<Obj> {
    if let Some(e) = OBJECTS.iter().find(|e| e.1 == sn) {
        return Some(from_static(e));
    }
    DYNAMIC.with(|d| d.borrow().iter().find(|o| o.sn == sn).cloned())
}

pub fn by_ln(ln: &str) -> Option<Obj> {
    if let Some(e) = OBJECTS.iter().find(|e| e.2 == ln) {
        return Some(from_static(e));
    }
    DYNAMIC.with(|d| d.borrow().iter().find(|o| o.ln == ln).cloned())
}

fn is_numeric_oid(s: &str) -> bool {
    !s.is_empty()
        && s.split('.').count() >= 2
        && s.split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// OBJ_txt2obj(s, 0) leaving OpenSSL's errors when it fails: an unknown
/// name, or a2d_ASN1_OBJECT()'s complaint about a dotted number.
#[inline(never)]
pub fn txt2obj_err(s: &str) -> Option<Obj> {
    let r = txt2obj(s);
    if r.is_none() {
        let first = s.bytes().next();
        if !first.is_some_and(|b| b.is_ascii_digit()) {
            crate::err::push(crate::err::LIB_OBJ, 103, "unknown object name");
        } else if !s.contains('.') {
            crate::err::push(crate::err::LIB_ASN1, 138, "missing second number");
        } else if s.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            && s.split('.')
                .next()
                .is_some_and(|f| f.parse::<u64>().map_or(true, |v| v > 2))
        {
            crate::err::push(crate::err::LIB_ASN1, 122, "first num too large");
        } else {
            crate::err::push(crate::err::LIB_ASN1, 130, "invalid digit");
        }
    }
    r
}

/// OBJ_txt2obj(s, 0): short name, long name or dotted OID.
pub fn txt2obj(s: &str) -> Option<Obj> {
    if let Some(o) = by_sn(s).or_else(|| by_ln(s)) {
        return Some(o);
    }
    if is_numeric_oid(s) {
        return Some(by_oid(s).unwrap_or(Obj {
            nid: 0,
            sn: s.to_string(),
            ln: s.to_string(),
            oid: s.to_string(),
        }));
    }
    None
}

/// OBJ_create(): returns false if the OID is invalid.
pub fn create(oid: &str, sn: &str, ln: &str) -> bool {
    if !is_numeric_oid(oid) {
        return false;
    }
    if by_sn(sn).is_some() || by_ln(ln).is_some() {
        return true;
    }
    DYNAMIC.with(|d| {
        let mut d = d.borrow_mut();
        let nid = FIRST_DYNAMIC_NID + d.len() as i64;
        d.push(Obj {
            nid,
            sn: sn.to_string(),
            ln: ln.to_string(),
            oid: oid.to_string(),
        });
    });
    true
}

pub fn oid_ln(oid: &str) -> String {
    by_oid(oid).map(|o| o.ln).unwrap_or_else(|| oid.to_string())
}
