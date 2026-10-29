//! A small DNS stub resolver's message handling (RFC 1035 message format,
//! RFC 3596 AAAA, RFC 2782 SRV, RFC 3403 NAPTR, RFC 8659 CAA, RFC 2874
//! A6) for dns_get_record(), dns_check_record() and getmxrr() (PHP manual
//! pages of these functions). The transport lives in the engine.

/// A query for `name` with type `qtype`, class IN, recursion desired.
pub fn build_query(id: u16, name: &[u8], qtype: u16) -> Option<Vec<u8>> {
    let mut q = Vec::with_capacity(name.len() + 18);
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    let name = name.strip_suffix(b".").unwrap_or(name);
    if !name.is_empty() {
        for label in name.split(|&c| c == b'.') {
            if label.is_empty() || label.len() > 63 {
                return None;
            }
            q.push(label.len() as u8);
            q.extend_from_slice(label);
        }
    }
    q.push(0);
    q.extend_from_slice(&qtype.to_be_bytes());
    q.extend_from_slice(&1u16.to_be_bytes());
    Some(q)
}

/// The data of one resource record.
#[derive(Debug, Clone)]
pub enum RData {
    A([u8; 4]),
    Aaaa([u8; 16]),
    Name(Vec<u8>),
    Mx(u16, Vec<u8>),
    Txt(Vec<Vec<u8>>),
    Hinfo(Vec<u8>, Vec<u8>),
    Soa {
        mname: Vec<u8>,
        rname: Vec<u8>,
        serial: u32,
        refresh: u32,
        retry: u32,
        expire: u32,
        minimum: u32,
    },
    Srv {
        pri: u16,
        weight: u16,
        port: u16,
        target: Vec<u8>,
    },
    Naptr {
        order: u16,
        pref: u16,
        flags: Vec<u8>,
        services: Vec<u8>,
        regex: Vec<u8>,
        replacement: Vec<u8>,
    },
    Caa {
        flags: u8,
        tag: Vec<u8>,
        value: Vec<u8>,
    },
    A6 {
        masklen: u8,
        addr: [u8; 16],
        chain: Vec<u8>,
    },
    Raw(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct Record {
    pub host: Vec<u8>,
    pub rtype: u16,
    pub class: u16,
    pub ttl: u32,
    pub data: RData,
}

#[derive(Debug, Default)]
pub struct Response {
    pub id: u16,
    pub truncated: bool,
    pub rcode: u8,
    pub answers: Vec<Record>,
    pub authority: Vec<Record>,
    pub additional: Vec<Record>,
}

fn u16_at(m: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(m.get(i..i + 2)?.try_into().ok()?))
}
fn u32_at(m: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(m.get(i..i + 4)?.try_into().ok()?))
}

/// A possibly compressed domain name at `i`: (name, offset after it).
fn name_at(m: &[u8], mut i: usize) -> Option<(Vec<u8>, usize)> {
    let mut out = Vec::new();
    let mut end = None;
    let mut hops = 0;
    loop {
        let len = *m.get(i)? as usize;
        if len & 0xC0 == 0xC0 {
            let ptr = (u16_at(m, i)? & 0x3FFF) as usize;
            end.get_or_insert(i + 2);
            hops += 1;
            if hops > 64 {
                return None;
            }
            i = ptr;
            continue;
        }
        if len == 0 {
            return Some((out, end.unwrap_or(i + 1)));
        }
        if !out.is_empty() {
            out.push(b'.');
        }
        out.extend_from_slice(m.get(i + 1..i + 1 + len)?);
        i += 1 + len;
    }
}

fn char_string(m: &[u8], i: usize) -> Option<(Vec<u8>, usize)> {
    let len = *m.get(i)? as usize;
    Some((m.get(i + 1..i + 1 + len)?.to_vec(), i + 1 + len))
}

fn rdata(m: &[u8], rtype: u16, s: usize, e: usize) -> Option<RData> {
    let d = m.get(s..e)?;
    Some(match rtype {
        1 => RData::A(d.try_into().ok()?),
        28 => RData::Aaaa(d.try_into().ok()?),
        2 | 5 | 12 => RData::Name(name_at(m, s)?.0),
        15 => RData::Mx(u16_at(m, s)?, name_at(m, s + 2)?.0),
        16 => {
            let mut parts = Vec::new();
            let mut i = s;
            while i < e {
                let (p, n) = char_string(m, i)?;
                parts.push(p);
                i = n;
            }
            RData::Txt(parts)
        }
        13 => {
            let (cpu, n) = char_string(m, s)?;
            let (os, _) = char_string(m, n)?;
            RData::Hinfo(cpu, os)
        }
        6 => {
            let (mname, n) = name_at(m, s)?;
            let (rname, n) = name_at(m, n)?;
            RData::Soa {
                mname,
                rname,
                serial: u32_at(m, n)?,
                refresh: u32_at(m, n + 4)?,
                retry: u32_at(m, n + 8)?,
                expire: u32_at(m, n + 12)?,
                minimum: u32_at(m, n + 16)?,
            }
        }
        33 => RData::Srv {
            pri: u16_at(m, s)?,
            weight: u16_at(m, s + 2)?,
            port: u16_at(m, s + 4)?,
            target: name_at(m, s + 6)?.0,
        },
        35 => {
            let order = u16_at(m, s)?;
            let pref = u16_at(m, s + 2)?;
            let (flags, n) = char_string(m, s + 4)?;
            let (services, n) = char_string(m, n)?;
            let (regex, n) = char_string(m, n)?;
            let (replacement, _) = name_at(m, n)?;
            RData::Naptr {
                order,
                pref,
                flags,
                services,
                regex,
                replacement,
            }
        }
        257 => {
            let flags = *d.first()?;
            let tl = *d.get(1)? as usize;
            RData::Caa {
                flags,
                tag: d.get(2..2 + tl)?.to_vec(),
                value: d.get(2 + tl..)?.to_vec(),
            }
        }
        38 => {
            let masklen = *d.first()?;
            let nbytes = (128 - masklen as usize).div_ceil(8);
            let mut addr = [0u8; 16];
            let suffix = d.get(1..1 + nbytes)?;
            addr[16 - nbytes..].copy_from_slice(suffix);
            let chain = if masklen > 0 {
                name_at(m, s + 1 + nbytes)?.0
            } else {
                Vec::new()
            };
            RData::A6 {
                masklen,
                addr,
                chain,
            }
        }
        _ => RData::Raw(d.to_vec()),
    })
}

/// Parses a response message.
pub fn parse_response(m: &[u8]) -> Option<Response> {
    let id = u16_at(m, 0)?;
    let flags = u16_at(m, 2)?;
    let qd = u16_at(m, 4)? as usize;
    let counts = [
        u16_at(m, 6)? as usize,
        u16_at(m, 8)? as usize,
        u16_at(m, 10)? as usize,
    ];
    let mut r = Response {
        id,
        truncated: flags & 0x0200 != 0,
        rcode: (flags & 0xF) as u8,
        ..Response::default()
    };
    let mut i = 12;
    for _ in 0..qd {
        i = name_at(m, i)?.1 + 4;
    }
    for (sec, &n) in counts.iter().enumerate() {
        for _ in 0..n {
            let (host, j) = name_at(m, i)?;
            let rtype = u16_at(m, j)?;
            let class = u16_at(m, j + 2)?;
            let ttl = u32_at(m, j + 4)?;
            let len = u16_at(m, j + 8)? as usize;
            let s = j + 10;
            let data = rdata(m, rtype, s, s + len)?;
            let rec = Record {
                host,
                rtype,
                class,
                ttl,
                data,
            };
            match sec {
                0 => r.answers.push(rec),
                1 => r.authority.push(rec),
                _ => r.additional.push(rec),
            }
            i = s + len;
        }
    }
    Some(r)
}

/// The name of a record type as dns_get_record() reports it.
pub fn type_name(t: u16) -> &'static str {
    match t {
        1 => "A",
        2 => "NS",
        5 => "CNAME",
        6 => "SOA",
        12 => "PTR",
        13 => "HINFO",
        15 => "MX",
        16 => "TXT",
        28 => "AAAA",
        33 => "SRV",
        35 => "NAPTR",
        38 => "A6",
        257 => "CAA",
        255 => "ANY",
        _ => "",
    }
}

/// The record type of a dns_check_record() type name.
pub fn type_by_name(n: &[u8]) -> Option<u16> {
    Some(match n.to_ascii_uppercase().as_slice() {
        b"A" => 1,
        b"NS" => 2,
        b"CNAME" => 5,
        b"SOA" => 6,
        b"PTR" => 12,
        b"MX" => 15,
        b"TXT" => 16,
        b"AAAA" => 28,
        b"SRV" => 33,
        b"NAPTR" => 35,
        b"A6" => 38,
        b"CAA" => 257,
        b"ANY" => 255,
        _ => return None,
    })
}

/// The DNS_* constant bits and their record types, in query order.
pub const DNS_TYPES: &[(i64, u16)] = &[
    (1, 1),          // DNS_A
    (2, 2),          // DNS_NS
    (16, 5),         // DNS_CNAME
    (32, 6),         // DNS_SOA
    (2048, 12),      // DNS_PTR
    (4096, 13),      // DNS_HINFO
    (8192, 257),     // DNS_CAA
    (16384, 15),     // DNS_MX
    (32768, 16),     // DNS_TXT
    (16777216, 38),  // DNS_A6
    (33554432, 33),  // DNS_SRV
    (67108864, 35),  // DNS_NAPTR
    (134217728, 28), // DNS_AAAA
];
pub const DNS_ANY: i64 = 268435456;
pub const DNS_ALL: i64 = 251721779;

/// The nameservers of a resolv.conf.
pub fn nameservers(conf: &[u8]) -> Vec<std::net::IpAddr> {
    let mut out = Vec::new();
    for line in conf.split(|&c| c == b'\n') {
        let mut w = line
            .split(|c| c.is_ascii_whitespace())
            .filter(|x| !x.is_empty());
        if w.next() == Some(b"nameserver")
            && let Some(a) = w.next()
            && let Ok(s) = std::str::from_utf8(a)
        {
            // a scoped IPv6 address keeps its zone out
            let s = s.split('%').next().unwrap_or(s);
            if let Ok(ip) = s.parse() {
                out.push(ip);
            }
        }
    }
    out
}

/// inet_ntop() of an IPv6 address: the longest run of two or more zero
/// groups (the first on a tie) becomes "::"; an address whose first 96
/// bits are zero, or 80 zero bits then ffff, ends in dotted IPv4.
pub fn ipv6_text(a: &[u8; 16]) -> String {
    let w: Vec<u16> = (0..8)
        .map(|i| u16::from_be_bytes([a[2 * i], a[2 * i + 1]]))
        .collect();
    let (mut best, mut best_len) = (usize::MAX, 0usize);
    let mut i = 0;
    while i < 8 {
        if w[i] == 0 {
            let s = i;
            while i < 8 && w[i] == 0 {
                i += 1;
            }
            if i - s > best_len {
                best = s;
                best_len = i - s;
            }
        } else {
            i += 1;
        }
    }
    if best_len < 2 {
        best = usize::MAX;
    }
    let v4 = best == 0 && (best_len == 6 || (best_len == 5 && w[5] == 0xffff));
    let mut out = String::new();
    let groups = if v4 { 6 } else { 8 };
    let mut i = 0;
    while i < groups {
        if i == best {
            out.push_str("::");
            i += best_len;
            continue;
        }
        if !out.is_empty() && !out.ends_with(':') {
            out.push(':');
        }
        out.push_str(&format!("{:x}", w[i]));
        i += 1;
    }
    if v4 {
        if !out.ends_with(':') {
            out.push(':');
        }
        out.push_str(&format!("{}.{}.{}.{}", a[12], a[13], a[14], a[15]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v6_text() {
        let p = |s: &str| s.parse::<std::net::Ipv6Addr>().unwrap().octets();
        assert_eq!(ipv6_text(&p("::1")), "::1");
        assert_eq!(ipv6_text(&p("::")), "::");
        assert_eq!(ipv6_text(&p("::ffff:1.2.3.4")), "::ffff:1.2.3.4");
        assert_eq!(ipv6_text(&p("::1.2.3.4")), "::1.2.3.4");
        assert_eq!(ipv6_text(&p("1:0:0:1::1")), "1:0:0:1::1");
        assert_eq!(ipv6_text(&p("1:0:1:0:1:0:1:1")), "1:0:1:0:1:0:1:1");
        assert_eq!(ipv6_text(&p("2001:db8:1234::")), "2001:db8:1234::");
    }

    #[test]
    fn query_roundtrip() {
        let q = build_query(7, b"example.com.", 15).unwrap();
        let r = parse_response(&q).unwrap();
        assert_eq!(r.id, 7);
        assert!(r.answers.is_empty());
    }
}
