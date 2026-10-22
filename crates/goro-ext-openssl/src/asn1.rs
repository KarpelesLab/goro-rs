//! Tiny DER reader/writer used by the key, certificate and PKCS code.

use crate::bn::Bn;

pub const BOOLEAN: u8 = 0x01;
pub const INTEGER: u8 = 0x02;
pub const BIT_STRING: u8 = 0x03;
pub const OCTET_STRING: u8 = 0x04;
pub const NULL: u8 = 0x05;
pub const OID: u8 = 0x06;
pub const UTF8_STRING: u8 = 0x0c;
pub const IA5_STRING: u8 = 0x16;
pub const UTC_TIME: u8 = 0x17;
pub const GENERALIZED_TIME: u8 = 0x18;
pub const SEQUENCE: u8 = 0x30;
pub const SET: u8 = 0x31;

/// A parsed TLV.
#[derive(Clone, Copy, Debug)]
pub struct Tlv<'a> {
    pub tag: u8,
    pub content: &'a [u8],
    /// The full encoding (tag + length + content).
    pub raw: &'a [u8],
}

impl<'a> Tlv<'a> {
    pub fn constructed(&self) -> bool {
        self.tag & 0x20 != 0
    }

    /// Context-specific tag number if this is [n].
    pub fn context(&self) -> Option<u8> {
        if self.tag & 0xc0 == 0x80 {
            Some(self.tag & 0x1f)
        } else {
            None
        }
    }

    pub fn children(&self) -> Option<Vec<Tlv<'a>>> {
        parse_all(self.content)
    }

    pub fn uint(&self) -> Bn {
        Bn::from_be(self.content)
    }

    pub fn oid(&self) -> Option<String> {
        if self.tag != OID {
            return None;
        }
        oid_to_string(self.content)
    }

    pub fn as_i64(&self) -> Option<i64> {
        if self.tag != INTEGER || self.content.is_empty() || self.content.len() > 8 {
            return None;
        }
        let mut v: i64 = if self.content[0] & 0x80 != 0 { -1 } else { 0 };
        for &b in self.content {
            v = (v << 8) | b as i64;
        }
        Some(v)
    }

    /// BIT STRING payload without the unused-bits byte.
    pub fn bits(&self) -> Option<&'a [u8]> {
        if self.tag != BIT_STRING || self.content.is_empty() {
            return None;
        }
        Some(&self.content[1..])
    }
}

/// Parse one TLV, returning it and the remaining bytes. Supports BER
/// indefinite length for constructed values.
pub fn parse(data: &[u8]) -> Option<(Tlv<'_>, &[u8])> {
    if data.len() < 2 {
        return None;
    }
    let tag = data[0];
    let mut i = 1;
    if tag & 0x1f == 0x1f {
        // high tag number form: skip
        while i < data.len() && data[i] & 0x80 != 0 {
            i += 1;
        }
        i += 1;
    }
    if i >= data.len() {
        return None;
    }
    let lb = data[i];
    i += 1;
    let len: usize;
    if lb < 0x80 {
        len = lb as usize;
    } else if lb == 0x80 {
        // indefinite length: find end-of-contents
        if tag & 0x20 == 0 {
            return None;
        }
        let mut rest = &data[i..];
        let start = i;
        loop {
            if rest.len() >= 2 && rest[0] == 0 && rest[1] == 0 {
                let clen = data.len() - start - rest.len();
                let total = start + clen + 2;
                return Some((
                    Tlv {
                        tag,
                        content: &data[start..start + clen],
                        raw: &data[..total],
                    },
                    &data[total..],
                ));
            }
            let (_, r) = parse(rest)?;
            rest = r;
        }
    } else {
        let n = (lb & 0x7f) as usize;
        if n > 8 || i + n > data.len() {
            return None;
        }
        let mut l = 0usize;
        for &b in &data[i..i + n] {
            l = l.checked_mul(256)?.checked_add(b as usize)?;
        }
        i += n;
        len = l;
    }
    let end = i.checked_add(len)?;
    if end > data.len() {
        return None;
    }
    Some((
        Tlv {
            tag,
            content: &data[i..end],
            raw: &data[..end],
        },
        &data[end..],
    ))
}

/// Parse a buffer that must contain exactly one TLV.
pub fn parse_one(data: &[u8]) -> Option<Tlv<'_>> {
    let (t, rest) = parse(data)?;
    if !rest.is_empty() {
        return None;
    }
    Some(t)
}

pub fn parse_all(mut data: &[u8]) -> Option<Vec<Tlv<'_>>> {
    let mut v = Vec::new();
    while !data.is_empty() {
        let (t, r) = parse(data)?;
        v.push(t);
        data = r;
    }
    Some(v)
}

pub fn oid_to_string(c: &[u8]) -> Option<String> {
    if c.is_empty() {
        return None;
    }
    let mut parts: Vec<u64> = Vec::new();
    let mut v: u64 = 0;
    let mut first = true;
    for &b in c {
        v = (v << 7) | (b & 0x7f) as u64;
        if b & 0x80 == 0 {
            if first {
                if v < 80 {
                    parts.push(v / 40);
                    parts.push(v % 40);
                } else {
                    parts.push(2);
                    parts.push(v - 80);
                }
                first = false;
            } else {
                parts.push(v);
            }
            v = 0;
        }
    }
    Some(
        parts
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join("."),
    )
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

pub fn len_bytes(len: usize) -> Vec<u8> {
    if len < 0x80 {
        vec![len as u8]
    } else {
        let mut b = Vec::new();
        let mut l = len;
        while l > 0 {
            b.push((l & 0xff) as u8);
            l >>= 8;
        }
        b.reverse();
        let mut out = vec![0x80 | b.len() as u8];
        out.extend(b);
        out
    }
}

pub fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend(len_bytes(content.len()));
    out.extend_from_slice(content);
    out
}

pub fn seq(items: &[Vec<u8>]) -> Vec<u8> {
    tlv(SEQUENCE, &items.concat())
}

pub fn set(items: &[Vec<u8>]) -> Vec<u8> {
    tlv(SET, &items.concat())
}

/// DER SET OF: elements sorted by their encodings.
pub fn set_of(items: &[Vec<u8>]) -> Vec<u8> {
    let mut v = items.to_vec();
    v.sort();
    tlv(SET, &v.concat())
}

/// INTEGER from unsigned big-endian bytes.
pub fn uint_bytes(b: &[u8]) -> Vec<u8> {
    let mut i = 0;
    while i + 1 < b.len() && b[i] == 0 {
        i += 1;
    }
    let mut c = Vec::new();
    if b.is_empty() {
        c.push(0);
    } else {
        if b[i] & 0x80 != 0 {
            c.push(0);
        }
        c.extend_from_slice(&b[i..]);
    }
    tlv(INTEGER, &c)
}

pub fn uint(n: &Bn) -> Vec<u8> {
    uint_bytes(n.be())
}

pub fn int_i64(v: i64) -> Vec<u8> {
    let bytes = v.to_be_bytes();
    let mut i = 0;
    while i < 7
        && ((bytes[i] == 0 && bytes[i + 1] & 0x80 == 0)
            || (bytes[i] == 0xff && bytes[i + 1] & 0x80 != 0))
    {
        i += 1;
    }
    tlv(INTEGER, &bytes[i..])
}

pub fn octet(b: &[u8]) -> Vec<u8> {
    tlv(OCTET_STRING, b)
}

pub fn bitstr(b: &[u8]) -> Vec<u8> {
    let mut c = vec![0u8];
    c.extend_from_slice(b);
    tlv(BIT_STRING, &c)
}

pub fn null() -> Vec<u8> {
    vec![NULL, 0]
}

pub fn boolean(v: bool) -> Vec<u8> {
    vec![BOOLEAN, 1, if v { 0xff } else { 0 }]
}

/// Context-specific constructed [n] EXPLICIT wrapper.
pub fn explicit(n: u8, content: &[u8]) -> Vec<u8> {
    tlv(0xa0 | n, content)
}

/// Context-specific primitive [n] IMPLICIT.
pub fn implicit(n: u8, content: &[u8]) -> Vec<u8> {
    tlv(0x80 | n, content)
}

pub fn oid_content(dotted: &str) -> Option<Vec<u8>> {
    let parts: Vec<u64> = dotted
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    if parts.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    let mut enc = |mut v: u64| {
        let mut tmp = vec![(v & 0x7f) as u8];
        v >>= 7;
        while v > 0 {
            tmp.push(0x80 | (v & 0x7f) as u8);
            v >>= 7;
        }
        tmp.reverse();
        out.extend(tmp);
    };
    enc(parts[0] * 40 + parts[1]);
    for &p in &parts[2..] {
        enc(p);
    }
    Some(out)
}

pub fn oid(dotted: &str) -> Vec<u8> {
    tlv(OID, &oid_content(dotted).unwrap_or_default())
}

/// AlgorithmIdentifier { oid, params }
pub fn algid(oid_str: &str, params: Option<Vec<u8>>) -> Vec<u8> {
    let mut v = vec![oid(oid_str)];
    if let Some(p) = params {
        v.push(p);
    }
    seq(&v)
}

// ---------------------------------------------------------------------------
// PEM
// ---------------------------------------------------------------------------

pub fn pem_encode(label: &str, der: &[u8]) -> String {
    let b64 = crate::base64_encode(der);
    let mut s = format!("-----BEGIN {}-----\n", label);
    for chunk in b64.as_bytes().chunks(64) {
        s.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        s.push('\n');
    }
    s.push_str(&format!("-----END {}-----\n", label));
    s
}

/// A PEM block found in some text.
#[derive(Clone, Debug)]
pub struct PemBlock {
    pub label: String,
    pub headers: Vec<(String, String)>,
    pub der: Vec<u8>,
}

/// Find all PEM blocks (in order). Blocks with invalid base64 are skipped.
pub fn pem_blocks(data: &[u8]) -> Vec<PemBlock> {
    let text = String::from_utf8_lossy(data);
    let mut out = Vec::new();
    let mut rest: &str = &text;
    while let Some(start) = rest.find("-----BEGIN ") {
        let after = &rest[start + 11..];
        let Some(lend) = after.find("-----") else {
            break;
        };
        let label = after[..lend].to_string();
        let body_start = &after[lend + 5..];
        let end_marker = format!("-----END {}-----", label);
        let Some(bend) = body_start.find(&end_marker) else {
            rest = body_start;
            continue;
        };
        let body = &body_start[..bend];
        let mut headers = Vec::new();
        let mut b64 = String::new();
        let mut in_headers = true;
        for line in body.lines() {
            let l = line.trim();
            if l.is_empty() {
                // the blank line after the BEGIN line is not the header separator
                if !headers.is_empty() || !b64.is_empty() {
                    in_headers = false;
                }
                continue;
            }
            if in_headers {
                if let Some(c) = l.find(':') {
                    headers.push((l[..c].trim().to_string(), l[c + 1..].trim().to_string()));
                    continue;
                }
                in_headers = false;
            }
            b64.push_str(l);
        }
        if let Some(der) = crate::base64_decode(&b64) {
            out.push(PemBlock {
                label,
                headers,
                der,
            });
        }
        rest = &body_start[bend + end_marker.len()..];
    }
    out
}

/// First PEM block with one of the given labels.
pub fn pem_find(data: &[u8], labels: &[&str]) -> Option<PemBlock> {
    pem_blocks(data)
        .into_iter()
        .find(|b| labels.contains(&b.label.as_str()))
}
