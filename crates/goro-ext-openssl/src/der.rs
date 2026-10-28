//! ASN.1 BER/DER reading and DER writing (ITU-T X.690).
//!
//! The reader accepts BER as well (indefinite lengths, constructed string
//! encodings), since S/MIME producers stream PKCS#7 in BER (RFC 5652 §1.1
//! allows it); the writer always produces DER.

/// Universal tag numbers (X.680 §8.6).
pub mod tag {
    pub const BOOLEAN: u32 = 1;
    pub const INTEGER: u32 = 2;
    pub const BIT_STRING: u32 = 3;
    pub const OCTET_STRING: u32 = 4;
    pub const NULL: u32 = 5;
    pub const OID: u32 = 6;
    pub const ENUMERATED: u32 = 10;
    pub const UTF8_STRING: u32 = 12;
    pub const SEQUENCE: u32 = 16;
    pub const SET: u32 = 17;
    pub const NUMERIC_STRING: u32 = 18;
    pub const PRINTABLE_STRING: u32 = 19;
    pub const T61_STRING: u32 = 20;
    pub const IA5_STRING: u32 = 22;
    pub const UTC_TIME: u32 = 23;
    pub const GENERALIZED_TIME: u32 = 24;
    pub const VISIBLE_STRING: u32 = 26;
    pub const UNIVERSAL_STRING: u32 = 28;
    pub const BMP_STRING: u32 = 30;
}

/// Tag classes.
pub const UNIVERSAL: u8 = 0;
pub const APPLICATION: u8 = 1;
pub const CONTEXT: u8 = 2;
pub const PRIVATE: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerError;

impl std::fmt::Display for DerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("malformed ASN.1")
    }
}

pub type Result<T> = std::result::Result<T, DerError>;

/// One decoded element.
#[derive(Debug, Clone, Copy)]
pub struct Tlv<'a> {
    pub class: u8,
    pub constructed: bool,
    pub num: u32,
    /// The content octets. For an indefinite-length element this excludes
    /// the end-of-contents marker.
    pub content: &'a [u8],
    /// The whole encoding (identifier, length, content).
    pub raw: &'a [u8],
    /// Whether the length was indefinite (BER).
    pub indefinite: bool,
}

impl<'a> Tlv<'a> {
    pub fn is(&self, class: u8, num: u32) -> bool {
        self.class == class && self.num == num
    }
    pub fn is_universal(&self, num: u32) -> bool {
        self.is(UNIVERSAL, num)
    }
    pub fn is_context(&self, num: u32) -> bool {
        self.is(CONTEXT, num)
    }
    /// A reader over the content of a constructed element.
    pub fn reader(&self) -> Reader<'a> {
        Reader::new(self.content)
    }
    /// The content of a string type, joining the segments of a BER
    /// constructed encoding.
    pub fn string_bytes(&self) -> Result<Vec<u8>> {
        if !self.constructed {
            return Ok(self.content.to_vec());
        }
        let mut out = Vec::new();
        let mut r = self.reader();
        while !r.is_empty() {
            let t = r.next()?;
            out.extend_from_slice(&t.string_bytes()?);
        }
        Ok(out)
    }
    /// An OBJECT IDENTIFIER in dotted form.
    pub fn oid(&self) -> Result<String> {
        if !self.is_universal(tag::OID) {
            return Err(DerError);
        }
        oid_to_string(self.content)
    }
    /// An INTEGER's two's-complement content.
    pub fn integer(&self) -> Result<&'a [u8]> {
        if !self.is_universal(tag::INTEGER) || self.content.is_empty() {
            return Err(DerError);
        }
        Ok(self.content)
    }
    /// A non-negative INTEGER as unsigned big-endian bytes (leading zeros
    /// stripped, at least one byte).
    pub fn uint(&self) -> Result<&'a [u8]> {
        let c = self.integer()?;
        let mut i = 0;
        while i + 1 < c.len() && c[i] == 0 {
            i += 1;
        }
        Ok(&c[i..])
    }
    /// A small INTEGER as i64.
    pub fn small_int(&self) -> Result<i64> {
        let c = self.integer()?;
        if c.len() > 8 {
            return Err(DerError);
        }
        let mut v: i64 = if c[0] & 0x80 != 0 { -1 } else { 0 };
        for b in c {
            v = (v << 8) | *b as i64;
        }
        Ok(v)
    }
    /// A BIT STRING's bytes (the unused-bits count dropped).
    pub fn bit_string(&self) -> Result<Vec<u8>> {
        if !self.is_universal(tag::BIT_STRING) {
            return Err(DerError);
        }
        let b = self.string_bytes()?;
        if b.is_empty() {
            return Err(DerError);
        }
        Ok(b[1..].to_vec())
    }
    pub fn boolean(&self) -> Result<bool> {
        if !self.is_universal(tag::BOOLEAN) || self.content.len() != 1 {
            return Err(DerError);
        }
        Ok(self.content[0] != 0)
    }
}

/// A sequential reader of elements.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }
    pub fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }
    pub fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }
    /// The next element.
    pub fn next(&mut self) -> Result<Tlv<'a>> {
        let (t, n) = parse_one(&self.data[self.pos..], 0)?;
        self.pos += n;
        Ok(t)
    }
    /// The next element when it has this class and number.
    pub fn next_if(&mut self, class: u8, num: u32) -> Result<Option<Tlv<'a>>> {
        if self.is_empty() {
            return Ok(None);
        }
        let save = self.pos;
        let t = self.next()?;
        if t.is(class, num) {
            Ok(Some(t))
        } else {
            self.pos = save;
            Ok(None)
        }
    }
    /// The next element, which must be this universal type.
    pub fn expect(&mut self, num: u32) -> Result<Tlv<'a>> {
        let t = self.next()?;
        if t.is_universal(num) {
            Ok(t)
        } else {
            Err(DerError)
        }
    }
    pub fn peek(&self) -> Option<Tlv<'a>> {
        parse_one(&self.data[self.pos..], 0).ok().map(|x| x.0)
    }
}

fn parse_one(d: &[u8], depth: usize) -> Result<(Tlv<'_>, usize)> {
    if depth > 64 || d.is_empty() {
        return Err(DerError);
    }
    let id = d[0];
    let class = id >> 6;
    let constructed = id & 0x20 != 0;
    let mut i = 1;
    let mut num = (id & 0x1f) as u32;
    if num == 0x1f {
        num = 0;
        loop {
            let b = *d.get(i).ok_or(DerError)?;
            i += 1;
            if num > (u32::MAX >> 7) {
                return Err(DerError);
            }
            num = (num << 7) | (b & 0x7f) as u32;
            if b & 0x80 == 0 {
                break;
            }
        }
    }
    let lb = *d.get(i).ok_or(DerError)?;
    i += 1;
    if lb == 0x80 {
        if !constructed {
            return Err(DerError);
        }
        // Indefinite: children until the end-of-contents octets.
        let start = i;
        loop {
            if d.len() < i + 2 {
                return Err(DerError);
            }
            if d[i] == 0 && d[i + 1] == 0 {
                let t = Tlv {
                    class,
                    constructed,
                    num,
                    content: &d[start..i],
                    raw: &d[..i + 2],
                    indefinite: true,
                };
                return Ok((t, i + 2));
            }
            let (_, n) = parse_one(&d[i..], depth + 1)?;
            i += n;
        }
    }
    let len = if lb & 0x80 == 0 {
        lb as usize
    } else {
        let n = (lb & 0x7f) as usize;
        if n == 0 || n > 8 {
            return Err(DerError);
        }
        let mut l: usize = 0;
        for _ in 0..n {
            let b = *d.get(i).ok_or(DerError)?;
            i += 1;
            l = l.checked_mul(256).ok_or(DerError)? | b as usize;
        }
        l
    };
    let end = i.checked_add(len).ok_or(DerError)?;
    if end > d.len() {
        return Err(DerError);
    }
    Ok((
        Tlv {
            class,
            constructed,
            num,
            content: &d[i..end],
            raw: &d[..end],
            indefinite: false,
        },
        end,
    ))
}

/// Parses exactly one element covering all of `d`.
pub fn parse(d: &[u8]) -> Result<Tlv<'_>> {
    let (t, n) = parse_one(d, 0)?;
    if n != d.len() {
        return Err(DerError);
    }
    Ok(t)
}

/// Parses one element at the start of `d`, returning it and the length
/// it used (trailing data allowed).
pub fn parse_prefix(d: &[u8]) -> Result<(Tlv<'_>, usize)> {
    parse_one(d, 0)
}

/// Dotted text of an OID's content octets.
pub fn oid_to_string(c: &[u8]) -> Result<String> {
    if c.is_empty() {
        return Err(DerError);
    }
    let mut arcs: Vec<u128> = Vec::new();
    let mut v: u128 = 0;
    let mut in_arc = false;
    for &b in c {
        if !in_arc && b == 0x80 {
            return Err(DerError);
        }
        if v > (u128::MAX >> 7) {
            return Err(DerError);
        }
        v = (v << 7) | (b & 0x7f) as u128;
        in_arc = true;
        if b & 0x80 == 0 {
            arcs.push(v);
            v = 0;
            in_arc = false;
        }
    }
    if in_arc {
        return Err(DerError);
    }
    let mut s = String::new();
    let first = arcs[0];
    let (a, b) = if first < 40 {
        (0, first)
    } else if first < 80 {
        (1, first - 40)
    } else {
        (2, first - 80)
    };
    s.push_str(&format!("{}.{}", a, b));
    for x in &arcs[1..] {
        s.push_str(&format!(".{}", x));
    }
    Ok(s)
}

/// Content octets of a dotted OID; none when malformed.
pub fn oid_from_string(s: &str) -> Option<Vec<u8>> {
    let parts: Vec<u128> = s
        .split('.')
        .map(|p| {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                p.parse().ok()
            }
        })
        .collect::<Option<_>>()?;
    if parts.len() < 2 || parts[0] > 2 || (parts[0] < 2 && parts[1] >= 40) {
        return None;
    }
    let mut out = Vec::new();
    let mut push = |mut v: u128| {
        let mut tmp = vec![(v & 0x7f) as u8];
        v >>= 7;
        while v > 0 {
            tmp.push(((v & 0x7f) as u8) | 0x80);
            v >>= 7;
        }
        tmp.reverse();
        out.extend_from_slice(&tmp);
    };
    push(parts[0] * 40 + parts[1]);
    for p in &parts[2..] {
        push(*p);
    }
    Some(out)
}

// ------------------------------------------------------------------ writer

/// The DER length octets of `n`.
pub fn len_bytes(n: usize) -> Vec<u8> {
    if n < 0x80 {
        vec![n as u8]
    } else {
        let b = (n as u64).to_be_bytes();
        let skip = b.iter().position(|&x| x != 0).unwrap_or(7);
        let mut v = vec![0x80 | (8 - skip) as u8];
        v.extend_from_slice(&b[skip..]);
        v
    }
}

/// An element with identifier octet `id` (single-byte tags only).
pub fn tlv(id: u8, content: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(content.len() + 6);
    v.push(id);
    v.extend_from_slice(&len_bytes(content.len()));
    v.extend_from_slice(content);
    v
}

pub fn seq(parts: &[&[u8]]) -> Vec<u8> {
    tlv(0x30, &parts.concat())
}

pub fn seq_vec(parts: &[Vec<u8>]) -> Vec<u8> {
    tlv(0x30, &parts.concat())
}

/// A SET OF, its elements sorted as DER requires (X.690 §11.6).
pub fn set_of(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut p: Vec<&Vec<u8>> = parts.iter().collect();
    p.sort();
    let c: Vec<u8> = p.into_iter().flatten().copied().collect();
    tlv(0x31, &c)
}

/// A SET with the elements in the given order.
pub fn set(parts: &[Vec<u8>]) -> Vec<u8> {
    tlv(0x31, &parts.concat())
}

/// An INTEGER from unsigned big-endian bytes.
pub fn uint(be: &[u8]) -> Vec<u8> {
    let mut i = 0;
    while i + 1 < be.len() && be[i] == 0 {
        i += 1;
    }
    let b = &be[i..];
    let mut c = Vec::with_capacity(b.len() + 1);
    if b.is_empty() {
        c.push(0);
    } else {
        if b[0] & 0x80 != 0 {
            c.push(0);
        }
        c.extend_from_slice(b);
    }
    tlv(0x02, &c)
}

/// An INTEGER from an i64.
pub fn int(v: i64) -> Vec<u8> {
    let b = v.to_be_bytes();
    let mut i = 0;
    while i < 7 && ((b[i] == 0 && b[i + 1] & 0x80 == 0) || (b[i] == 0xff && b[i + 1] & 0x80 != 0)) {
        i += 1;
    }
    tlv(0x02, &b[i..])
}

/// An INTEGER from two's-complement content.
pub fn int_raw(c: &[u8]) -> Vec<u8> {
    tlv(0x02, c)
}

pub fn oid(dotted: &str) -> Vec<u8> {
    tlv(0x06, &oid_from_string(dotted).unwrap_or_default())
}

pub fn null() -> Vec<u8> {
    vec![0x05, 0x00]
}

pub fn octet_string(b: &[u8]) -> Vec<u8> {
    tlv(0x04, b)
}

pub fn bit_string(b: &[u8]) -> Vec<u8> {
    let mut c = Vec::with_capacity(b.len() + 1);
    c.push(0);
    c.extend_from_slice(b);
    tlv(0x03, &c)
}

pub fn boolean(v: bool) -> Vec<u8> {
    vec![0x01, 0x01, if v { 0xff } else { 0 }]
}

/// `[n] EXPLICIT`.
pub fn explicit(n: u8, inner: &[u8]) -> Vec<u8> {
    tlv(0xa0 | n, inner)
}

/// `[n] IMPLICIT` of a constructed type: the content of `inner` under a
/// context tag.
pub fn implicit_constructed(n: u8, inner_content: &[u8]) -> Vec<u8> {
    tlv(0xa0 | n, inner_content)
}

/// `[n] IMPLICIT` of a primitive type.
pub fn implicit_primitive(n: u8, content: &[u8]) -> Vec<u8> {
    tlv(0x80 | n, content)
}

/// An AlgorithmIdentifier with optional parameters.
pub fn algorithm_id(dotted: &str, params: Option<&[u8]>) -> Vec<u8> {
    match params {
        Some(p) => seq(&[&oid(dotted), p]),
        None => seq(&[&oid(dotted)]),
    }
}

/// UTCTime or GeneralizedTime for a Unix time (RFC 5280 §4.1.2.5: UTCTime
/// through 2049).
pub fn time(t: i64) -> Vec<u8> {
    let (y, mo, d, h, mi, s) = crate::time::civil_from_unix(t);
    if (1950..2050).contains(&y) {
        tlv(
            0x17,
            format!("{:02}{:02}{:02}{:02}{:02}{:02}Z", y % 100, mo, d, h, mi, s).as_bytes(),
        )
    } else {
        tlv(
            0x18,
            format!("{:04}{:02}{:02}{:02}{:02}{:02}Z", y, mo, d, h, mi, s).as_bytes(),
        )
    }
}

/// Re-encodes a BER element as DER where it matters for signatures and
/// hashing: indefinite lengths become definite, recursively for
/// constructed elements. Constructed strings are left as they are.
pub fn to_der(t: &Tlv) -> Result<Vec<u8>> {
    if !t.indefinite && !t.constructed {
        return Ok(t.raw.to_vec());
    }
    if !t.constructed {
        return Ok(t.raw.to_vec());
    }
    let mut content = Vec::new();
    let mut r = t.reader();
    while !r.is_empty() {
        let c = r.next()?;
        content.extend_from_slice(&to_der(&c)?);
    }
    let mut out = identifier(t);
    out.extend_from_slice(&len_bytes(content.len()));
    out.extend_from_slice(&content);
    Ok(out)
}

/// The identifier octets of an element.
pub fn identifier(t: &Tlv) -> Vec<u8> {
    let first = (t.class << 6) | if t.constructed { 0x20 } else { 0 };
    if t.num < 31 {
        vec![first | t.num as u8]
    } else {
        let mut v = vec![first | 0x1f];
        let mut tmp = vec![(t.num & 0x7f) as u8];
        let mut n = t.num >> 7;
        while n > 0 {
            tmp.push(((n & 0x7f) as u8) | 0x80);
            n >>= 7;
        }
        tmp.reverse();
        v.extend_from_slice(&tmp);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oid_round_trip() {
        for s in [
            "1.2.840.113549.1.1.11",
            "2.5.4.3",
            "2.999.3",
            "1.3.6.1.4.1.311.60.2.1.3",
        ] {
            let c = oid_from_string(s).unwrap();
            assert_eq!(oid_to_string(&c).unwrap(), s);
        }
    }

    #[test]
    fn ints() {
        assert_eq!(int(0), vec![2, 1, 0]);
        assert_eq!(int(128), vec![2, 2, 0, 128]);
        assert_eq!(int(-1), vec![2, 1, 0xff]);
        assert_eq!(uint(&[0, 0, 0x80]), vec![2, 2, 0, 0x80]);
    }

    #[test]
    fn indefinite() {
        let d = [0x30, 0x80, 0x02, 0x01, 0x05, 0x00, 0x00];
        let t = parse(&d).unwrap();
        assert!(t.indefinite);
        assert_eq!(to_der(&t).unwrap(), vec![0x30, 3, 2, 1, 5]);
    }
}
