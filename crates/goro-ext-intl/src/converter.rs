//! ext/intl/converter: UConverter.
//!
//! The converter names, aliases, types, substitution characters and the
//! single-byte tables are ICU's (dumped from ICU 77, converter_tables.rs);
//! the Unicode encodings are implemented here and the multi-byte CJK
//! charsets come from the iconv extension's tables. Conversions go through
//! UTF-16 like ucnv_toUChars() / ucnv_fromUChars(), with ICU's callback
//! reasons; a UConverter subclass gets its toUCallback() / fromUCallback()
//! called (php_converter_set_callbacks()).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};
use goro_ext_iconv::charset::{self, Dec};

use crate::error::{self, IntlError};
use crate::zpp::Obj;
use crate::{Call, obj, s, sv};

#[path = "converter_tables.rs"]
mod tables;

const SBCS_BLOB: &[u8] = include_bytes!("converter_sbcs.bin");

pub const REASON_UNASSIGNED: i64 = 0;
pub const REASON_ILLEGAL: i64 = 1;
pub const REASON_RESET: i64 = 3;
pub const REASON_CLOSE: i64 = 4;

const U_INVALID_CHAR_FOUND: i32 = 10;
const U_TRUNCATED_CHAR_FOUND: i32 = 11;
const U_ILLEGAL_CHAR_FOUND: i32 = 12;
const U_FILE_ACCESS_ERROR: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Backend {
    Utf8,
    Cesu8,
    /// None: with BOM detection (default BE; version=2 writes LE)
    Utf16(Option<bool>, bool),
    Utf32(Option<bool>),
    Utf7,
    Ascii,
    Latin1,
    Sbcs(usize),
    Iconv(charset::Kind),
}

/// The backend of an ICU converter, by canonical name.
fn backend(name: &str, typ: i64, sbcs: i32) -> Option<Backend> {
    use charset::{Endian, Family, Kind};
    use goro_ext_mbstring::mbfl::EncNo;
    if sbcs >= 0 {
        return Some(Backend::Sbcs(sbcs as usize));
    }
    let _ = typ;
    Some(match name {
        "UTF-8" => Backend::Utf8,
        "CESU-8" => Backend::Cesu8,
        "UTF-16" | "UTF-16,version=1" => Backend::Utf16(None, false),
        "UTF-16,version=2" => Backend::Utf16(None, true),
        "UTF-16BE" | "UTF16_OppositeEndian" | "UTF-16BE,version=1" => Backend::Utf16(Some(true), false),
        "UTF-16LE" | "UTF16_PlatformEndian" | "UTF-16LE,version=1" => Backend::Utf16(Some(false), false),
        "UTF-32" => Backend::Utf32(None),
        "UTF-32BE" | "UTF32_OppositeEndian" => Backend::Utf32(Some(true)),
        "UTF-32LE" | "UTF32_PlatformEndian" => Backend::Utf32(Some(false)),
        "UTF-7" => Backend::Utf7,
        "US-ASCII" => Backend::Ascii,
        "ISO-8859-1" => Backend::Latin1,
        "ibm-943_P15A-2003" => Backend::Iconv(Kind::Mbfl(EncNo::Cp932, Family::Sjis)),
        "ibm-943_P130-1999" => Backend::Iconv(Kind::Mbfl(EncNo::Sjis, Family::Sjis)),
        "ibm-33722_P12A_P12A-2009_U2" | "ibm-33722_P120-1999" | "ibm-954_P101-2007"
        | "euc-jp-2007" => Backend::Iconv(Kind::Mbfl(EncNo::EucJp, Family::EucJp)),
        "windows-936-2000" | "ibm-1386_P100-2001" => {
            Backend::Iconv(Kind::Mbfl(EncNo::Cp936, Family::Dbcs))
        }
        "ibm-1383_P110-1999" => Backend::Iconv(Kind::Mbfl(EncNo::EucCn, Family::Dbcs)),
        "windows-950-2000" => Backend::Iconv(Kind::Mbfl(EncNo::Cp950, Family::Dbcs)),
        "ibm-1373_P100-2002" | "ibm-950_P110-1999" => {
            Backend::Iconv(Kind::Mbfl(EncNo::Big5, Family::Dbcs))
        }
        "ibm-949_P110-1999" | "ibm-949_P11A-1999" | "windows-949-2000" => {
            Backend::Iconv(Kind::Mbfl(EncNo::Uhc, Family::Dbcs))
        }
        "ibm-970_P110_P110-2006_U2" | "ibm-971_P100-1995" => {
            Backend::Iconv(Kind::Mbfl(EncNo::EucKr, Family::Dbcs))
        }
        "ISO_2022,locale=ja,version=0" => Backend::Iconv(Kind::Iso2022Jp),
        "ibm-1258_P100-1997" => Backend::Iconv(Kind::Cp1258),
        _ => {
            let _ = Endian::Be;
            return None;
        }
    })
}

/// A converter (UConverter*): its table entry, backend and substitution
/// characters.
#[derive(Clone)]
struct Cnv {
    idx: usize,
    be: Backend,
    subst: Vec<u8>,
}

impl Cnv {
    fn name(&self) -> &'static str {
        tables::CONVERTERS[self.idx].0
    }
    fn typ(&self) -> i64 {
        tables::CONVERTERS[self.idx].1
    }
    fn min_max(&self) -> (usize, usize) {
        let e = &tables::CONVERTERS[self.idx];
        (e.3 as usize, e.4 as usize)
    }
}

/// ucnv_compareNames() key: letters and digits only, lowercase, leading
/// zeros of numbers dropped.
fn name_key(n: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(n.len());
    let mut after_digit = false;
    for (i, &c) in n.iter().enumerate() {
        if c.is_ascii_alphanumeric() {
            if c == b'0' && !after_digit {
                // a leading zero: dropped if more digits / letters follow
                let next_digit = n[i + 1..]
                    .iter()
                    .find(|c| c.is_ascii_alphanumeric())
                    .is_some_and(|c| c.is_ascii_digit());
                if next_digit {
                    continue;
                }
            }
            after_digit = c.is_ascii_digit();
            out.push(c.to_ascii_lowercase());
        } else {
            after_digit = false;
        }
    }
    out
}

thread_local! {
    static ALIASES: RefCell<Option<HashMap<Vec<u8>, usize>>> = const { RefCell::new(None) };
    static SBCS_REV: RefCell<HashMap<usize, Rc<HashMap<u32, u8>>>> = RefCell::new(HashMap::new());
}

fn supported(i: usize) -> Option<Backend> {
    let e = &tables::CONVERTERS[i];
    backend(e.0, e.1, e.6)
}

/// The converter index of a name or alias.
fn find(name: &[u8]) -> Option<usize> {
    ALIASES.with(|a| {
        let mut a = a.borrow_mut();
        let map = a.get_or_insert_with(|| {
            let mut m = HashMap::new();
            for (i, e) in tables::CONVERTERS.iter().enumerate() {
                m.entry(name_key(e.0.as_bytes())).or_insert(i);
                for al in e.5 {
                    m.entry(name_key(al.as_bytes())).or_insert(i);
                }
            }
            m
        });
        let k = name_key(name);
        if k.is_empty() {
            return None;
        }
        map.get(&k).copied()
    })
}

/// Whether an alias names several converters (U_AMBIGUOUS_ALIAS_WARNING).
fn ambiguous(name: &[u8]) -> bool {
    thread_local! {
        static AMBIG: RefCell<Option<HashMap<Vec<u8>, usize>>> = const { RefCell::new(None) };
    }
    AMBIG.with(|a| {
        let mut a = a.borrow_mut();
        let map = a.get_or_insert_with(|| {
            let mut m: HashMap<Vec<u8>, usize> = HashMap::new();
            for e in tables::CONVERTERS.iter() {
                let mut keys: Vec<Vec<u8>> = e.5.iter().map(|al| name_key(al.as_bytes())).collect();
                keys.push(name_key(e.0.as_bytes()));
                keys.sort();
                keys.dedup();
                for k in keys {
                    *m.entry(k).or_insert(0) += 1;
                }
            }
            m
        });
        map.get(&name_key(name)).is_some_and(|&n| n > 1)
    })
}

/// ucnv_open(): Err(status).
fn open(name: &[u8]) -> Result<Cnv, i32> {
    // ICU names are invariant characters
    if name.iter().any(|&b| b >= 0x80) || name.is_empty() {
        return Err(U_FILE_ACCESS_ERROR);
    }
    let i = find(name).ok_or(U_FILE_ACCESS_ERROR)?;
    let be = supported(i).ok_or(U_FILE_ACCESS_ERROR)?;
    Ok(Cnv {
        idx: i,
        be,
        subst: tables::CONVERTERS[i].2.to_vec(),
    })
}

fn sbcs_table(k: usize) -> &'static [u8] {
    &SBCS_BLOB[k * 512..k * 512 + 512]
}

fn sbcs_decode(k: usize, b: u8) -> Option<u32> {
    let t = sbcs_table(k);
    let v = u16::from_le_bytes([t[b as usize * 2], t[b as usize * 2 + 1]]);
    match v {
        0xFFFF => None,
        0xFFFE => Some(0x1A),
        v => Some(v as u32),
    }
}

fn sbcs_rev(k: usize) -> Rc<HashMap<u32, u8>> {
    SBCS_REV.with(|m| {
        m.borrow_mut()
            .entry(k)
            .or_insert_with(|| {
                let mut r = HashMap::new();
                for b in (0..=255u8).rev() {
                    if let Some(c) = sbcs_decode(k, b) {
                        r.insert(c, b);
                    }
                }
                Rc::new(r)
            })
            .clone()
    })
}

/// A conversion failure that a callback (or the default substitution)
/// handles.
struct Bad {
    reason: i64,
    code: i32,
    len: usize,
}

/// One decoding step: Ok((code points, bytes consumed)) or Err(bad).
fn decode_step(be: Backend, s: &[u8], st: &mut DecState) -> Result<(Vec<u32>, usize), Bad> {
    let ill = |len: usize| Bad {
        reason: REASON_ILLEGAL,
        code: U_ILLEGAL_CHAR_FOUND,
        len,
    };
    let trunc = |len: usize| Bad {
        reason: REASON_ILLEGAL,
        code: U_TRUNCATED_CHAR_FOUND,
        len,
    };
    match be {
        Backend::Utf8 | Backend::Cesu8 => {
            let b = s[0];
            if b < 0x80 {
                return Ok((vec![b as u32], 1));
            }
            let cesu = be == Backend::Cesu8;
            let (n, init) = match b {
                0xC2..=0xDF => (2, (b & 0x1F) as u32),
                0xE0..=0xEF => (3, (b & 0x0F) as u32),
                0xF0..=0xF4 if !cesu => (4, (b & 0x07) as u32),
                _ => return Err(ill(1)),
            };
            let mut c = init;
            for i in 1..n {
                let Some(&t) = s.get(i) else {
                    return Err(trunc(i));
                };
                let ok = match (i, b) {
                    (1, 0xE0) => (0xA0..=0xBF).contains(&t),
                    (1, 0xED) if !cesu => (0x80..=0x9F).contains(&t),
                    (1, 0xF0) => (0x90..=0xBF).contains(&t),
                    (1, 0xF4) => (0x80..=0x8F).contains(&t),
                    _ => t & 0xC0 == 0x80,
                };
                if !ok {
                    return Err(ill(i));
                }
                c = (c << 6) | (t & 0x3F) as u32;
            }
            Ok((vec![c], n))
        }
        Backend::Utf16(endian, _) => {
            if s.len() < 2 {
                return Err(trunc(s.len()));
            }
            let mut be16 = endian.unwrap_or(st.bom_be.unwrap_or(true));
            if endian.is_none() && !st.bom_seen {
                st.bom_seen = true;
                if s[..2] == [0xFE, 0xFF] {
                    st.bom_be = Some(true);
                    return Ok((vec![], 2));
                }
                if s[..2] == [0xFF, 0xFE] {
                    st.bom_be = Some(false);
                    return Ok((vec![], 2));
                }
                be16 = true;
                st.bom_be = Some(true);
            }
            let u = |k: usize| -> u32 {
                if be16 {
                    ((s[k] as u32) << 8) | s[k + 1] as u32
                } else {
                    ((s[k + 1] as u32) << 8) | s[k] as u32
                }
            };
            let a = u(0);
            if (0xD800..0xDC00).contains(&a) {
                if s.len() < 4 {
                    return Err(trunc(s.len()));
                }
                let b = u(2);
                if (0xDC00..0xE000).contains(&b) {
                    return Ok((vec![0x10000 + ((a - 0xD800) << 10) + (b - 0xDC00)], 4));
                }
                return Err(ill(2));
            }
            if (0xDC00..0xE000).contains(&a) {
                return Err(ill(2));
            }
            Ok((vec![a], 2))
        }
        Backend::Utf32(endian) => {
            if s.len() < 4 {
                return Err(trunc(s.len()));
            }
            let mut be32 = endian.unwrap_or(st.bom_be.unwrap_or(true));
            if endian.is_none() && !st.bom_seen {
                st.bom_seen = true;
                if s[..4] == [0, 0, 0xFE, 0xFF] {
                    st.bom_be = Some(true);
                    return Ok((vec![], 4));
                }
                if s[..4] == [0xFF, 0xFE, 0, 0] {
                    st.bom_be = Some(false);
                    return Ok((vec![], 4));
                }
                be32 = true;
                st.bom_be = Some(true);
            }
            let v = if be32 {
                u32::from_be_bytes([s[0], s[1], s[2], s[3]])
            } else {
                u32::from_le_bytes([s[0], s[1], s[2], s[3]])
            };
            if v > 0x10FFFF || (0xD800..0xE000).contains(&v) {
                return Err(ill(4));
            }
            Ok((vec![v], 4))
        }
        Backend::Utf7 => utf7_step(s, st),
        Backend::Ascii => {
            if s[0] < 0x80 {
                Ok((vec![s[0] as u32], 1))
            } else {
                Err(ill(1))
            }
        }
        Backend::Latin1 => Ok((vec![s[0] as u32], 1)),
        Backend::Sbcs(k) => match sbcs_decode(k, s[0]) {
            Some(c) => Ok((vec![c], 1)),
            None => Err(Bad {
                reason: REASON_UNASSIGNED,
                code: U_INVALID_CHAR_FOUND,
                len: 1,
            }),
        },
        Backend::Iconv(kind) => {
            let d = st.iconv.get_or_insert_with(|| charset::Decoder::new(kind));
            match d.decode(s) {
                Dec::Char(c, n) => Ok((vec![c], n)),
                Dec::Shift(n) => Ok((vec![], n)),
                Dec::Illegal(n) => {
                    // a well-formed but unmapped double-byte character
                    let two = s.len() >= 2
                        && match kind {
                            charset::Kind::Mbfl(_, charset::Family::Sjis) => {
                                matches!(s[0], 0x81..=0x9F | 0xE0..=0xFC)
                                    && matches!(s[1], 0x40..=0x7E | 0x80..=0xFC)
                            }
                            charset::Kind::Mbfl(_, charset::Family::EucJp) => {
                                matches!(s[0], 0xA1..=0xFE) && matches!(s[1], 0xA1..=0xFE)
                            }
                            charset::Kind::Mbfl(_, charset::Family::Dbcs) => {
                                matches!(s[0], 0x81..=0xFE) && matches!(s[1], 0x40..=0xFE)
                            }
                            _ => false,
                        };
                    if two {
                        return Err(Bad {
                            reason: REASON_UNASSIGNED,
                            code: U_INVALID_CHAR_FOUND,
                            len: 2,
                        });
                    }
                    Err(ill(n.max(1)))
                }
                Dec::Incomplete => Err(trunc(s.len())),
            }
        }
    }
}

#[derive(Default)]
struct DecState {
    bom_seen: bool,
    bom_be: Option<bool>,
    iconv: Option<charset::Decoder>,
    /// UTF-7: inside a base64 run
    b64: bool,
    bits: u32,
    nbits: u32,
    pending_hi: Option<u32>,
}

fn b64val(c: u8) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => (c - b'A') as u32,
        b'a'..=b'z' => (c - b'a' + 26) as u32,
        b'0'..=b'9' => (c - b'0' + 52) as u32,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    })
}

const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn utf7_step(s: &[u8], st: &mut DecState) -> Result<(Vec<u32>, usize), Bad> {
    let c = s[0];
    if !st.b64 {
        if c == b'+' {
            if s.get(1) == Some(&b'-') {
                return Ok((vec![b'+' as u32], 2));
            }
            st.b64 = true;
            st.bits = 0;
            st.nbits = 0;
            return Ok((vec![], 1));
        }
        if c >= 0x80 {
            return Err(Bad {
                reason: REASON_ILLEGAL,
                code: U_ILLEGAL_CHAR_FOUND,
                len: 1,
            });
        }
        return Ok((vec![c as u32], 1));
    }
    match b64val(c) {
        Some(v) => {
            st.bits = (st.bits << 6) | v;
            st.nbits += 6;
            let mut out = Vec::new();
            if st.nbits >= 16 {
                st.nbits -= 16;
                let u = (st.bits >> st.nbits) & 0xFFFF;
                st.bits &= (1 << st.nbits) - 1;
                match st.pending_hi.take() {
                    Some(hi) if (0xDC00..0xE000).contains(&u) => {
                        out.push(0x10000 + ((hi - 0xD800) << 10) + (u - 0xDC00));
                    }
                    _ if (0xD800..0xDC00).contains(&u) => st.pending_hi = Some(u),
                    _ => out.push(u),
                }
            }
            Ok((out, 1))
        }
        None => {
            st.b64 = false;
            if c == b'-' {
                return Ok((vec![], 1));
            }
            if c >= 0x80 {
                return Err(Bad {
                    reason: REASON_ILLEGAL,
                    code: U_ILLEGAL_CHAR_FOUND,
                    len: 1,
                });
            }
            Ok((vec![c as u32], 1))
        }
    }
}

fn utf7_encode(cps: &[u32]) -> Vec<u8> {
    let direct = |c: u32| {
        c < 0x80
            && (c as u8).is_ascii_alphanumeric()
            || matches!(c, 0x20 | 0x09 | 0x0D | 0x0A)
            || b"'(),-./:?!\"#$%&*;<=>@[]^_`{|}".contains(&(c as u8)) && c < 0x80
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < cps.len() {
        let c = cps[i];
        if c == '+' as u32 {
            out.extend_from_slice(b"+-");
            i += 1;
            continue;
        }
        if direct(c) {
            out.push(c as u8);
            i += 1;
            continue;
        }
        out.push(b'+');
        let mut bits: u32 = 0;
        let mut nbits = 0;
        while i < cps.len() && !direct(cps[i]) && cps[i] != '+' as u32 {
            let mut units = [0u16; 2];
            let n = char::from_u32(cps[i])
                .map(|ch| ch.encode_utf16(&mut units).len())
                .unwrap_or(0);
            for &u in &units[..n] {
                bits = (bits << 16) | u as u32;
                nbits += 16;
                while nbits >= 6 {
                    nbits -= 6;
                    out.push(B64[((bits >> nbits) & 0x3F) as usize]);
                }
                bits &= (1 << nbits) - 1;
            }
            i += 1;
        }
        if nbits > 0 {
            out.push(B64[((bits << (6 - nbits)) & 0x3F) as usize]);
        }
        // '-' ends the run at the end and before a base64 character or '-'
        if i >= cps.len() || (cps[i] < 0x80 && (b64val(cps[i] as u8).is_some() || cps[i] == '-' as u32)) {
            out.push(b'-');
        }
    }
    out
}

/// Encode one code point: Ok(bytes) or Err(reason/code).
fn encode_cp(be: Backend, cp: u32, enc: &mut Option<charset::Encoder>, first: &mut bool) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    match be {
        Backend::Utf8 => crate::common::push_utf8(&mut out, cp),
        Backend::Cesu8 => {
            if cp > 0xFFFF {
                let v = cp - 0x10000;
                crate::common::push_utf8(&mut out, 0xD800 + (v >> 10));
                crate::common::push_utf8(&mut out, 0xDC00 + (v & 0x3FF));
            } else {
                crate::common::push_utf8(&mut out, cp);
            }
        }
        Backend::Utf16(endian, v2) => {
            // with a BOM: the platform endianness (version=2: big endian)
            let big = endian.unwrap_or(v2);
            if endian.is_none() && *first {
                *first = false;
                out.extend_from_slice(if big { &[0xFE, 0xFF] } else { &[0xFF, 0xFE] });
            }
            let mut units = [0u16; 2];
            let ch = char::from_u32(cp)?;
            for u in ch.encode_utf16(&mut units) {
                if big {
                    out.extend_from_slice(&u.to_be_bytes());
                } else {
                    out.extend_from_slice(&u.to_le_bytes());
                }
            }
        }
        Backend::Utf32(endian) => {
            let big = endian.unwrap_or(false);
            if endian.is_none() && *first {
                *first = false;
                out.extend_from_slice(&[0xFF, 0xFE, 0, 0]);
            }
            if big {
                out.extend_from_slice(&cp.to_be_bytes());
            } else {
                out.extend_from_slice(&cp.to_le_bytes());
            }
        }
        Backend::Utf7 => unreachable!(),
        Backend::Ascii => {
            if cp >= 0x80 {
                return None;
            }
            out.push(cp as u8);
        }
        Backend::Latin1 => {
            if cp >= 0x100 {
                return None;
            }
            out.push(cp as u8);
        }
        Backend::Sbcs(k) => out.push(*sbcs_rev(k).get(&cp)?),
        Backend::Iconv(kind) => {
            let e = enc.get_or_insert_with(|| charset::Encoder::new(kind));
            if !e.encode(cp, &mut out) {
                return None;
            }
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Object state

#[derive(Clone, Default)]
pub struct Conv {
    src: Option<Cnv>,
    dest: Option<Cnv>,
    pub err: IntlError,
}

pub static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(clone_obj),
    ..NativeObjectHandlers::EMPTY
};

fn create(vm: &mut Vm, o: &Obj) {
    obj::attach(o, Conv::default(), &HANDLERS);
    // A subclass's callbacks see the converters closed (free_obj).
    if !o.borrow().class_name.eq_ignore_ascii_case(b"UConverter") {
        vm.register_native_free_hook(&HANDLERS, free_obj);
        vm.track_destructible(Value::Object(o.clone()));
    }
}

/// php_converter_free_object(): ucnv_close() of the source then the
/// destination converter, each calling its toU then fromU callback with
/// UCNV_CLOSE.
fn free_obj(vm: &mut Vm, o: &Obj) {
    let Some(st) = obj::state::<Conv>(o) else {
        return;
    };
    let (src, dest) = {
        let b = st.borrow();
        (b.src.is_some(), b.dest.is_some())
    };
    let cb = Callbacks {
        obj: Value::Object(o.clone()),
    };
    for open in [src, dest] {
        if !open || vm.current_exception.is_some() {
            continue;
        }
        if cb
            .call(vm, b"toUCallback", [Value::Long(REASON_CLOSE), s(""), s("")], 0)
            .is_none()
        {
            return;
        }
        let args = [
            Value::Long(REASON_CLOSE),
            crate::arr(PhpArray::new()),
            Value::Long(0),
        ];
        if cb.call(vm, b"fromUCallback", args, 0).is_none() {
            return;
        }
    }
}

fn clone_obj(_vm: &mut Vm, src: &Obj, dst: &Obj) {
    obj::clone_state::<Conv>(src, dst, |c| Conv {
        src: c.src.clone(),
        dest: c.dest.clone(),
        err: IntlError::default(),
    });
}

crate::intl_functions! { pub FUNCTIONS: }

pub const METHODS: &[(&str, crate::Imp)] = &[
    ("UConverter::__construct", construct),
    ("UConverter::convert", convert),
    ("UConverter::fromUCallback", default_callback),
    ("UConverter::toUCallback", default_callback),
    ("UConverter::getAliases", get_aliases),
    ("UConverter::getAvailable", get_available),
    ("UConverter::getDestinationEncoding", get_dest_encoding),
    ("UConverter::getSourceEncoding", get_src_encoding),
    ("UConverter::getDestinationType", get_dest_type),
    ("UConverter::getSourceType", get_src_type),
    ("UConverter::getErrorCode", get_error_code),
    ("UConverter::getErrorMessage", get_error_message),
    ("UConverter::getStandards", get_standards),
    ("UConverter::getSubstChars", get_subst_chars),
    ("UConverter::setSubstChars", set_subst_chars),
    ("UConverter::reasonText", reason_text),
    ("UConverter::setDestinationEncoding", set_dest_encoding),
    ("UConverter::setSourceEncoding", set_src_encoding),
    ("UConverter::transcode", transcode),
];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[("UConverter", Some(create))];

fn state(c: &Call) -> Option<Rc<RefCell<Conv>>> {
    obj::state::<Conv>(&c.obj())
}

/// THROW_UFAILURE / php_converter_throw_failure()
fn throw_failure(vm: &mut Vm, c: &Call, st: Option<&Rc<RefCell<Conv>>>, code: i32, msg: &str) {
    match st {
        Some(st) => {
            let mut e = std::mem::take(&mut st.borrow_mut().err);
            error::set_both(vm, &c.name, &mut e, code, msg);
            st.borrow_mut().err = e;
        }
        None => error::set_global(vm, &c.name, code, msg),
    }
}

fn ufailure(vm: &mut Vm, c: &Call, st: Option<&Rc<RefCell<Conv>>>, code: i32) {
    let msg = format!("returned error {}: {}", code, error::error_name(code as i64));
    throw_failure(vm, c, st, code, &msg);
}

/// php_converter_set_encoding()
fn set_encoding(vm: &mut Vm, c: &Call, st: Option<&Rc<RefCell<Conv>>>, enc: &[u8]) -> Option<Cnv> {
    match open(enc) {
        Ok(cnv) => {
            if ambiguous(enc) {
                // U_AMBIGUOUS_ALIAS_WARNING
                let msg = format!("Ambiguous encoding specified, using {}", cnv.name());
                error::set_global(vm, &c.name, -122, &msg);
                if vm.current_exception.is_some() {
                    return None;
                }
            }
            Some(cnv)
        }
        Err(code) => {
            if st.is_some() {
                ufailure(vm, c, st, code);
            } else {
                let msg = format!(
                    "Error setting encoding: {} - {}",
                    code,
                    error::error_name(code as i64)
                );
                error::set_global(vm, &c.name, code, &msg);
            }
            None
        }
    }
}

fn set_ini(vm: &mut Vm, k: &[u8], v: Value) -> Value {
    vm.constants.insert(k.to_vec(), v).unwrap_or(Value::Null)
}

/// UConverter::__construct()
fn construct(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let dest = c.opt_str(0).unwrap_or_else(|| b"utf-8".to_vec());
    let src = c.opt_str(1).unwrap_or_else(|| b"utf-8".to_vec());
    let Some(st) = state(c) else {
        return Value::Null;
    };
    // the errors throw (use_exceptions forced on, error_level off)
    let old_ue = set_ini(vm, b"intl.use_exceptions", s("1"));
    let old_el = set_ini(vm, b"intl.error_level", s("0"));
    if let Some(cnv) = set_encoding(vm, c, Some(&st), &src) {
        st.borrow_mut().src = Some(cnv);
        if let Some(cnv) = set_encoding(vm, c, Some(&st), &dest) {
            st.borrow_mut().dest = Some(cnv);
        }
    }
    set_ini(vm, b"intl.use_exceptions", old_ue);
    set_ini(vm, b"intl.error_level", old_el);
    Value::Null
}

fn do_set_encoding(vm: &mut Vm, c: &mut Call, dest: bool) -> Value {
    let enc = c.str(0);
    let Some(st) = state(c) else {
        return Value::Null;
    };
    error::reset_both(&mut st.borrow_mut().err);
    match set_encoding(vm, c, Some(&st), &enc) {
        Some(cnv) => {
            let mut b = st.borrow_mut();
            if dest {
                b.dest = Some(cnv);
            } else {
                b.src = Some(cnv);
            }
            Value::True
        }
        None => Value::False,
    }
}

fn set_src_encoding(vm: &mut Vm, c: &mut Call) -> Value {
    do_set_encoding(vm, c, false)
}

fn set_dest_encoding(vm: &mut Vm, c: &mut Call) -> Value {
    do_set_encoding(vm, c, true)
}

fn do_get(c: &mut Call, dest: bool, f: impl Fn(&Cnv) -> Value) -> Value {
    let Some(st) = state(c) else {
        return Value::Null;
    };
    error::reset_both(&mut st.borrow_mut().err);
    let b = st.borrow();
    match if dest { &b.dest } else { &b.src } {
        Some(cnv) => f(cnv),
        None => Value::Null,
    }
}

fn get_src_encoding(_vm: &mut Vm, c: &mut Call) -> Value {
    do_get(c, false, |x| s(x.name()))
}

fn get_dest_encoding(_vm: &mut Vm, c: &mut Call) -> Value {
    do_get(c, true, |x| s(x.name()))
}

fn get_src_type(_vm: &mut Vm, c: &mut Call) -> Value {
    do_get(c, false, |x| Value::Long(x.typ()))
}

fn get_dest_type(_vm: &mut Vm, c: &mut Call) -> Value {
    do_get(c, true, |x| Value::Long(x.typ()))
}

fn get_error_code(_vm: &mut Vm, c: &mut Call) -> Value {
    match state(c) {
        Some(st) => Value::Long(st.borrow().err.code as i64),
        None => Value::Long(0),
    }
}

fn get_error_message(_vm: &mut Vm, c: &mut Call) -> Value {
    match state(c) {
        Some(st) => s(st.borrow().err.message()),
        None => Value::Null,
    }
}

/// ucnv_setSubstChars(): Err(U_ILLEGAL_ARGUMENT_ERROR) for a length out
/// of the converter's character byte range.
fn check_subst(cnv: &Cnv, chars: &[u8]) -> Result<(), i32> {
    let (mn, mx) = cnv.min_max();
    if chars.len() < mn || chars.len() > mx {
        return Err(error::U_ILLEGAL_ARGUMENT_ERROR);
    }
    Ok(())
}

fn set_subst_chars(vm: &mut Vm, c: &mut Call) -> Value {
    let chars = c.str(0);
    let Some(st) = state(c) else {
        return Value::Null;
    };
    error::reset_both(&mut st.borrow_mut().err);
    let mut ret = true;
    for dest in [false, true] {
        let cur = {
            let b = st.borrow();
            if dest { b.dest.clone() } else { b.src.clone() }
        };
        match cur {
            Some(mut cnv) => match check_subst(&cnv, &chars) {
                Ok(()) => {
                    cnv.subst = chars.clone();
                    let mut b = st.borrow_mut();
                    if dest {
                        b.dest = Some(cnv);
                    } else {
                        b.src = Some(cnv);
                    }
                }
                Err(code) => {
                    ufailure(vm, c, Some(&st), code);
                    ret = false;
                }
            },
            None => {
                let m = if dest {
                    "Destination Converter has not been initialized yet"
                } else {
                    "Source Converter has not been initialized yet"
                };
                throw_failure(vm, c, Some(&st), error::U_INVALID_STATE_ERROR, m);
                ret = false;
            }
        }
    }
    crate::bool_v(ret)
}

fn get_subst_chars(_vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = state(c) else {
        return Value::Null;
    };
    error::reset_both(&mut st.borrow_mut().err);
    match &st.borrow().src {
        Some(cnv) => sv(cnv.subst.clone()),
        None => Value::Null,
    }
}

fn reason_text(vm: &mut Vm, c: &mut Call) -> Value {
    error::reset_global();
    let r = c.long(0);
    let t = match r {
        0 => "REASON_UNASSIGNED",
        1 => "REASON_ILLEGAL",
        2 => "REASON_IRREGULAR",
        3 => "REASON_RESET",
        4 => "REASON_CLOSE",
        5 => "REASON_CLONE",
        _ => {
            crate::zpp::value_error(vm, &c.name, 1, "reason", "must be a UConverter::REASON_* constant");
            return Value::Null;
        }
    };
    s(t)
}

/// php_converter_default_callback(): the base class's toUCallback() /
/// fromUCallback().
fn default_callback(vm: &mut Vm, c: &mut Call) -> Value {
    let reason = c.long(0);
    if !matches!(reason, 0..=2) {
        return Value::Null;
    }
    let Some(st) = state(c) else {
        return Value::Null;
    };
    let src = st.borrow().src.clone();
    match src {
        None => {
            throw_failure(
                vm,
                c,
                Some(&st),
                error::U_INVALID_STATE_ERROR,
                "Source Converter has not been initialized yet",
            );
            c.set_ref(vm, 3, Value::Long(error::U_INVALID_STATE_ERROR as i64));
            s(b"\x1a")
        }
        Some(cnv) => {
            c.set_ref(vm, 3, Value::Long(0));
            sv(cnv.subst.clone())
        }
    }
}

/// The callbacks of a UConverter subclass.
struct Callbacks {
    obj: Value,
}

impl Callbacks {
    fn call(&self, vm: &mut Vm, method: &[u8], args: [Value; 3], code: i32) -> Option<(Value, i32)> {
        let r = Rc::new(RefCell::new(Value::Long(code as i64)));
        let mut a = args.to_vec();
        a.push(Value::Reference(r.clone()));
        let ret = vm.call_object_method(&self.obj, method, &a);
        if vm.current_exception.is_some() {
            return None;
        }
        let code = match &*r.borrow() {
            Value::Long(n) => *n as i32,
            _ => code,
        };
        Some((ret.unwrap_or(Value::Null), code))
    }
}

/// php_converter_append_toUnicode_target()
fn append_to_u(vm: &mut Vm, c: &Call, st: Option<&Rc<RefCell<Conv>>>, v: &Value, out: &mut Vec<u32>) {
    match v.deref() {
        Value::Null | Value::Undef => {}
        Value::Long(n) => {
            if !(0..=0x10FFFF).contains(&n) {
                let m = format!("Invalid codepoint U+{:04x}", n);
                throw_failure(vm, c, st, error::U_ILLEGAL_ARGUMENT_ERROR, &m);
                return;
            }
            out.push(n as u32);
        }
        Value::String(sv) => {
            for (_, cp) in crate::breakiter::decode_native(sv.as_bytes()) {
                out.push(cp);
            }
        }
        Value::Array(a) => {
            let vals: Vec<Value> = a.borrow().iter().map(|(_, v)| v.clone()).collect();
            for x in vals {
                append_to_u(vm, c, st, &x, out);
            }
        }
        _ => throw_failure(
            vm,
            c,
            st,
            error::U_ILLEGAL_ARGUMENT_ERROR,
            "toUCallback() specified illegal type for substitution character",
        ),
    }
}

/// php_converter_append_fromUnicode_target()
fn append_from_u(vm: &mut Vm, c: &Call, st: Option<&Rc<RefCell<Conv>>>, v: &Value, out: &mut Vec<u8>) {
    match v.deref() {
        Value::Null | Value::Undef => {}
        Value::Long(n) => out.push(n as u8),
        Value::String(sv) => out.extend_from_slice(sv.as_bytes()),
        Value::Array(a) => {
            let vals: Vec<Value> = a.borrow().iter().map(|(_, v)| v.clone()).collect();
            for x in vals {
                append_from_u(vm, c, st, &x, out);
            }
        }
        _ => throw_failure(
            vm,
            c,
            st,
            error::U_ILLEGAL_ARGUMENT_ERROR,
            "fromUCallback() specified illegal type for substitution character",
        ),
    }
}

/// ucnv_toUChars(): Err(status) when a callback left a failure.
fn to_unicode(
    vm: &mut Vm,
    c: &Call,
    st: Option<&Rc<RefCell<Conv>>>,
    cnv: &Cnv,
    input: &[u8],
    cb: Option<&Callbacks>,
) -> Result<Vec<u32>, i32> {
    if let Some(cb) = cb {
        cb.call(vm, b"toUCallback", [Value::Long(REASON_RESET), s(""), s("")], 0)
            .ok_or(0)?;
    }
    let mut out = Vec::with_capacity(input.len());
    let mut ds = DecState::default();
    let mut i = 0;
    while i < input.len() {
        match decode_step(cnv.be, &input[i..], &mut ds) {
            Ok((cps, n)) => {
                out.extend(cps);
                i += n;
            }
            Err(bad) => {
                let n = bad.len.clamp(1, input.len() - i);
                let units = &input[i..i + n];
                i += n;
                match cb {
                    Some(cb) => {
                        let args = [Value::Long(bad.reason), sv(input[i..].to_vec()), sv(units.to_vec())];
                        let (ret, code) = cb.call(vm, b"toUCallback", args, bad.code).ok_or(0)?;
                        append_to_u(vm, c, st, &ret, &mut out);
                        if error::failure(code) {
                            return Err(code);
                        }
                    }
                    None => out.push(0xFFFD),
                }
            }
        }
    }
    // UTF-16 code units: surrogate pairs (CESU-8) are one code point
    let mut cps = Vec::with_capacity(out.len());
    let mut k = 0;
    while k < out.len() {
        let u = out[k];
        if (0xD800..0xDC00).contains(&u) && out.get(k + 1).is_some_and(|l| (0xDC00..0xE000).contains(l)) {
            cps.push(0x10000 + ((u - 0xD800) << 10) + (out[k + 1] - 0xDC00));
            k += 2;
            continue;
        }
        cps.push(u);
        k += 1;
    }
    Ok(cps)
}

/// ucnv_fromUChars()
fn from_unicode(
    vm: &mut Vm,
    c: &Call,
    st: Option<&Rc<RefCell<Conv>>>,
    cnv: &Cnv,
    cps: &[u32],
    cb: Option<&Callbacks>,
) -> Result<Vec<u8>, i32> {
    if let Some(cb) = cb {
        cb.call(
            vm,
            b"fromUCallback",
            [Value::Long(REASON_RESET), crate::arr(PhpArray::new()), Value::Long(0)],
            0,
        )
        .ok_or(0)?;
    }
    if cnv.be == Backend::Utf7 {
        return Ok(utf7_encode(cps));
    }
    let mut out = Vec::with_capacity(cps.len());
    let mut enc = None;
    let mut first = true;
    for &cp in cps {
        let bad = if (0xD800..0xE000).contains(&cp) {
            Some((REASON_ILLEGAL, U_ILLEGAL_CHAR_FOUND))
        } else {
            match encode_cp(cnv.be, cp, &mut enc, &mut first) {
                Some(b) => {
                    out.extend(b);
                    None
                }
                None => Some((REASON_UNASSIGNED, U_INVALID_CHAR_FOUND)),
            }
        };
        if let Some((reason, code)) = bad {
            match cb {
                Some(cb) => {
                    let mut a = PhpArray::new();
                    a.push(Value::Long(cp as i64));
                    let args = [Value::Long(reason), crate::arr(a), Value::Long(cp as i64)];
                    let (ret, code) = cb.call(vm, b"fromUCallback", args, code).ok_or(0)?;
                    append_from_u(vm, c, st, &ret, &mut out);
                    if error::failure(code) {
                        return Err(code);
                    }
                }
                None => {
                    // a stateful encoding shifts back to its initial state
                    if let Some(e) = enc.as_mut() {
                        out.extend_from_slice(e.reset_bytes());
                        e.reset();
                    }
                    out.extend_from_slice(&cnv.subst)
                }
            }
        }
    }
    if let Some(e) = &enc {
        out.extend_from_slice(e.reset_bytes());
    }
    Ok(out)
}

/// php_converter_do_convert()
fn do_convert(
    vm: &mut Vm,
    c: &Call,
    st: Option<&Rc<RefCell<Conv>>>,
    dest: Option<&Cnv>,
    src: Option<&Cnv>,
    input: &[u8],
    cb: Option<&Callbacks>,
) -> Option<Vec<u8>> {
    let (Some(dest), Some(src)) = (dest, src) else {
        throw_failure(vm, c, st, error::U_INVALID_STATE_ERROR, "Internal converters not initialized");
        return None;
    };
    // the preflight and the conversion
    let mut units = Vec::new();
    for _ in 0..2 {
        match to_unicode(vm, c, st, src, input, cb) {
            Ok(u) => units = u,
            Err(code) => {
                if code != 0 {
                    ufailure(vm, c, st, code);
                }
                return None;
            }
        }
    }
    let mut out = Vec::new();
    for _ in 0..2 {
        match from_unicode(vm, c, st, dest, &units, cb) {
            Ok(o) => out = o,
            Err(code) => {
                if code != 0 {
                    ufailure(vm, c, st, code);
                }
                return None;
            }
        }
    }
    Some(out)
}

fn convert(vm: &mut Vm, c: &mut Call) -> Value {
    let input = c.str(0);
    let reverse = c.bool(1);
    let Some(st) = state(c) else {
        return Value::Null;
    };
    error::reset_both(&mut st.borrow_mut().err);
    let (src, dest) = {
        let b = st.borrow();
        (b.src.clone(), b.dest.clone())
    };
    let (d, s_) = if reverse { (src, dest) } else { (dest, src) };
    let o = c.obj();
    let sub = !o.borrow().class_name.eq_ignore_ascii_case(b"UConverter");
    let cb = sub.then(|| Callbacks {
        obj: Value::Object(o.clone()),
    });
    match do_convert(vm, c, Some(&st), d.as_ref(), s_.as_ref(), &input, cb.as_ref()) {
        Some(r) => sv(r),
        None => Value::False,
    }
}

fn transcode(vm: &mut Vm, c: &mut Call) -> Value {
    let input = c.str(0);
    let dest = c.str(1);
    let src = c.str(2);
    let opts = c.arg(3);
    error::reset_global();
    let Some(mut sc) = set_encoding(vm, c, None, &src) else {
        return Value::False;
    };
    let Some(mut dc) = set_encoding(vm, c, None, &dest) else {
        return Value::False;
    };
    let mut err = 0;
    if let Value::Array(a) = &opts {
        let get = |k: &str| -> Option<Vec<u8>> {
            let a = a.borrow();
            match a.get(&goro_core::array::ArrayKey::String(goro_core::string::PhpString::from_bytes(k.as_bytes())))?.deref() {
                Value::String(s) => Some(s.as_bytes().to_vec()),
                _ => None,
            }
        };
        if let Some(fs) = get("from_subst") {
            let fs = fs[..fs.len() & 0x7F].to_vec();
            match check_subst(&sc, &fs) {
                Ok(()) => sc.subst = fs,
                Err(e) => err = e,
            }
        }
        if err == 0 {
            if let Some(ts) = get("to_subst") {
                let ts = ts[..ts.len() & 0x7F].to_vec();
                match check_subst(&dc, &ts) {
                    Ok(()) => dc.subst = ts,
                    Err(e) => err = e,
                }
            }
        }
    }
    let mut ret = Value::False;
    if err == 0 {
        if let Some(r) = do_convert(vm, c, None, Some(&dc), Some(&sc), &input, None) {
            ret = sv(r);
        }
    }
    if err != 0 {
        ufailure(vm, c, None, err);
        ret = Value::False;
    }
    ret
}

fn get_available(_vm: &mut Vm, _c: &mut Call) -> Value {
    error::reset_global();
    let mut a = PhpArray::new();
    for (i, e) in tables::CONVERTERS.iter().enumerate() {
        if supported(i).is_some() {
            a.push(s(e.0));
        }
    }
    crate::arr(a)
}

fn get_aliases(_vm: &mut Vm, c: &mut Call) -> Value {
    let name = c.str(0);
    error::reset_global();
    // ucnv_countAliases(): an unknown name has no aliases
    let mut a = PhpArray::new();
    if let Some(i) = find(&name).filter(|&i| supported(i).is_some()) {
        for al in tables::CONVERTERS[i].5 {
            a.push(s(*al));
        }
    }
    crate::arr(a)
}

fn get_standards(_vm: &mut Vm, _c: &mut Call) -> Value {
    error::reset_global();
    let mut a = PhpArray::new();
    for st in tables::STANDARDS {
        a.push(s(*st));
    }
    crate::arr(a)
}
