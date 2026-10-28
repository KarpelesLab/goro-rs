//! Certificate extensions from configuration sections (OpenSSL
//! documentation: x509v3_config(5)): basicConstraints, keyUsage,
//! extendedKeyUsage, subjectKeyIdentifier, authorityKeyIdentifier,
//! subject/issuerAltName, the Netscape string and cert-type extensions,
//! crlDistributionPoints, authorityInfoAccess/subjectInfoAccess,
//! certificatePolicies (OIDs, CPS and user notices), nameConstraints,
//! policyConstraints, inhibitAnyPolicy, and arbitrary OIDs with
//! `DER:` / `ASN1:` values. Encodings per RFC 5280 §4.2.

use crate::conf::Conf;
use crate::der;
use crate::digest::Md;
use crate::oid;
use crate::x509::{Cert, Extension, Name};

/// What an extension value may refer to.
pub struct Ctx<'a> {
    pub subject_spki: &'a [u8],
    pub subject: Option<&'a Name>,
    /// The issuing certificate (none when self-signing).
    pub issuer: Option<&'a Cert>,
    pub issuer_name: Option<&'a Name>,
    pub serial: Option<&'a [u8]>,
    pub conf: &'a Conf,
}

#[derive(Debug, Clone)]
pub struct V3Error {
    pub name: String,
    pub value: String,
    /// The error-queue reason text (observed on PHP 8.5).
    pub reason: &'static str,
}

/// The extensions of a section, in order.
pub fn build_section(section: &str, ctx: &Ctx) -> Result<Vec<Extension>, V3Error> {
    let Some(entries) = ctx.conf.section(section) else {
        return Ok(Vec::new());
    };
    let mut out: Vec<Extension> = Vec::new();
    for (name, value) in entries {
        let e = build_one(name, value, ctx)?;
        if let Some(e) = e {
            if let Some(old) = out.iter_mut().find(|x| x.oid == e.oid) {
                *old = e;
            } else {
                out.push(e);
            }
        }
    }
    Ok(out)
}

fn split_list(v: &str) -> Vec<String> {
    v.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn ski_of(spki: &[u8]) -> Option<Vec<u8>> {
    // RFC 5280 §4.2.1.2 method (1): SHA-1 of the subjectPublicKey bits.
    let t = der::parse(spki).ok()?;
    let mut r = t.reader();
    r.next().ok()?;
    let bits = r.next().ok()?.bit_string().ok()?;
    Some(Md::H(purecrypto::hash::HashAlgorithm::Sha1).digest(&bits))
}

fn err(name: &str, value: &str, reason: &'static str) -> V3Error {
    V3Error {
        name: name.to_string(),
        value: value.to_string(),
        reason,
    }
}

pub fn build_one(name: &str, value: &str, ctx: &Ctx) -> Result<Option<Extension>, V3Error> {
    let mut v = value.trim();
    let mut critical = false;
    if let Some(rest) = v.strip_prefix("critical") {
        if let Some(r) = rest.trim_start().strip_prefix(',') {
            critical = true;
            v = r.trim();
        }
    }
    let oid_s = match oid::by_name(name) {
        Some(o) => o.oid.to_string(),
        None => {
            if der::oid_from_string(name).is_some() {
                name.to_string()
            } else {
                return Err(err(name, value, "unknown extension name"));
            }
        }
    };
    let e = |r| err(name, value, r);
    let body: Vec<u8> = match oid_s.as_str() {
        "2.5.29.19" => {
            let mut ca = false;
            let mut pl: Option<i64> = None;
            for item in split_list(v) {
                let (k, val) = item.split_once(':').ok_or(e("invalid value"))?;
                match k.trim() {
                    "CA" => {
                        ca = matches!(
                            val.trim().to_ascii_uppercase().as_str(),
                            "TRUE" | "YES" | "Y"
                        );
                    }
                    "pathlen" => pl = Some(val.trim().parse().map_err(|_| e("invalid value"))?),
                    _ => return Err(e("invalid value")),
                }
            }
            let mut p = Vec::new();
            if ca {
                p.push(der::boolean(true));
            }
            if let Some(n) = pl {
                p.push(der::int(n));
            }
            der::seq_vec(&p)
        }
        "2.5.29.15" => {
            const NAMES: [&str; 9] = [
                "digitalSignature",
                "nonRepudiation",
                "keyEncipherment",
                "dataEncipherment",
                "keyAgreement",
                "keyCertSign",
                "cRLSign",
                "encipherOnly",
                "decipherOnly",
            ];
            let mut bits = 0u16;
            for item in split_list(v) {
                let i = NAMES
                    .iter()
                    .position(|n| *n == item)
                    .ok_or(e("invalid value"))?;
                bits |= 1 << i;
            }
            bitstring_named(bits)
        }
        "2.16.840.1.113730.1.1" => {
            const NAMES: [&str; 8] = [
                "client", "server", "email", "objsign", "reserved", "sslCA", "emailCA", "objCA",
            ];
            let mut bits = 0u16;
            for item in split_list(v) {
                let i = NAMES
                    .iter()
                    .position(|n| *n == item)
                    .ok_or(e("invalid value"))?;
                bits |= 1 << i;
            }
            bitstring_named(bits)
        }
        "2.5.29.37" => {
            let mut p = Vec::new();
            for item in split_list(v) {
                let o = oid::resolve(&item).ok_or(e("invalid object identifier"))?;
                p.push(der::oid(&o));
            }
            der::seq_vec(&p)
        }
        "2.5.29.14" => {
            if v == "none" {
                return Ok(None);
            }
            let id = if v == "hash" {
                ski_of(ctx.subject_spki).ok_or(e("invalid value"))?
            } else {
                crate::pbe::hex_decode(&v.replace(':', "")).ok_or(e("invalid value"))?
            };
            der::octet_string(&id)
        }
        "2.5.29.35" => {
            let mut keyid = 0; // 0 none, 1 if available, 2 always
            let mut issuer = 0;
            for item in split_list(v) {
                match item.as_str() {
                    "keyid" => keyid = 1,
                    "keyid:always" => keyid = 2,
                    "issuer" => issuer = 1,
                    "issuer:always" => issuer = 2,
                    "none" => return Ok(None),
                    _ => return Err(e("unknown option")),
                }
            }
            let mut parts = Vec::new();
            let (kid, iname, iserial): (Option<Vec<u8>>, Option<Name>, Option<Vec<u8>>) =
                match ctx.issuer {
                    Some(c) => (
                        c.subject_key_id().or_else(|| ski_of(&c.spki)),
                        Some(c.subject.clone()),
                        Some(c.serial.clone()),
                    ),
                    None => (
                        // Self-signed: the subject's own key.
                        ski_of(ctx.subject_spki),
                        ctx.issuer_name.cloned(),
                        ctx.serial.map(|s| s.to_vec()),
                    ),
                };
            // Observed on PHP 8.5: a self-signed certificate gets its own
            // key identifier only with `keyid:always`.
            if keyid == 2 || (keyid == 1 && ctx.issuer.is_some()) {
                match &kid {
                    Some(k) => parts.push(der::implicit_primitive(0, k)),
                    None if keyid == 2 => return Err(e("unable to get issuer keyid")),
                    None => {}
                }
            }
            if issuer == 2 || (issuer == 1 && parts.is_empty()) {
                match (&iname, &iserial) {
                    (Some(n), Some(s)) => {
                        parts.push(der::implicit_constructed(1, &der::explicit(4, &n.der)));
                        parts.push(der::implicit_primitive(2, s));
                    }
                    _ if issuer == 2 => return Err(e("unable to get issuer details")),
                    _ => {}
                }
            }
            der::seq_vec(&parts)
        }
        "2.5.29.17" | "2.5.29.18" => {
            let mut names = Vec::new();
            for item in split_list(v) {
                if item == "email:copy" {
                    if let Some(s) = ctx.subject {
                        for a in s.find(oid::ids::EMAIL) {
                            names.push(der::implicit_primitive(1, &a.utf8()));
                        }
                    }
                    continue;
                }
                if item == "issuer:copy" {
                    if let Some(c) = ctx.issuer
                        && let Some(ian) = c.ext("2.5.29.17")
                        && let Ok(t) = der::parse(&ian.value)
                    {
                        names.push(t.content.to_vec());
                    }
                    continue;
                }
                names.push(general_name(&item, ctx).ok_or(e("bad value"))?);
            }
            der::seq_vec(&names)
        }
        "2.16.840.1.113730.1.13"
        | "2.16.840.1.113730.1.2"
        | "2.16.840.1.113730.1.3"
        | "2.16.840.1.113730.1.4"
        | "2.16.840.1.113730.1.7"
        | "2.16.840.1.113730.1.8"
        | "2.16.840.1.113730.1.12" => der::tlv(0x16, v.as_bytes()),
        "2.5.29.31" | "2.5.29.46" => {
            let mut dps = Vec::new();
            for item in split_list(v) {
                let g = general_name(&item, ctx).ok_or(e("bad value"))?;
                // DistributionPoint { distributionPoint [0] { fullName [0] GeneralNames } }
                dps.push(der::seq_vec(&[der::implicit_constructed(
                    0,
                    &der::implicit_constructed(0, &g),
                )]));
            }
            der::seq_vec(&dps)
        }
        "1.3.6.1.5.5.7.1.1" | "1.3.6.1.5.5.7.1.11" => {
            let mut ads = Vec::new();
            for item in split_list(v) {
                let (m, loc) = item.split_once(';').ok_or(e("invalid syntax"))?;
                let mo = oid::resolve(m.trim()).ok_or(e("bad object"))?;
                let g = general_name(loc.trim(), ctx).ok_or(e("bad value"))?;
                ads.push(der::seq_vec(&[der::oid(&mo), g]));
            }
            der::seq_vec(&ads)
        }
        "2.5.29.32" => {
            let mut pols = Vec::new();
            for item in split_list(v) {
                if let Some(sec) = item.strip_prefix('@') {
                    pols.push(policy_section(sec, ctx).ok_or(e("invalid policy"))?);
                } else if item == "ia5org" {
                    continue;
                } else {
                    let o = oid::resolve(&item).ok_or(e("invalid object identifier"))?;
                    pols.push(der::seq_vec(&[der::oid(&o)]));
                }
            }
            der::seq_vec(&pols)
        }
        "2.5.29.30" => {
            let mut perm = Vec::new();
            let mut excl = Vec::new();
            for item in split_list(v) {
                let (kind, rest) = item.split_once(';').ok_or(e("invalid syntax"))?;
                let g = if let Some(ip) = rest.strip_prefix("IP:") {
                    let (a, m) = ip.split_once('/').ok_or(e("bad value"))?;
                    let mut b = parse_ip(a).ok_or(e("bad value"))?;
                    b.extend(parse_ip(m).ok_or(e("bad value"))?);
                    der::implicit_primitive(7, &b)
                } else {
                    general_name(rest, ctx).ok_or(e("bad value"))?
                };
                let sub = der::seq_vec(&[g]);
                match kind.trim() {
                    "permitted" => perm.push(sub),
                    "excluded" => excl.push(sub),
                    _ => return Err(e("invalid syntax")),
                }
            }
            let mut p = Vec::new();
            if !perm.is_empty() {
                p.push(der::implicit_constructed(0, &perm.concat()));
            }
            if !excl.is_empty() {
                p.push(der::implicit_constructed(1, &excl.concat()));
            }
            der::seq_vec(&p)
        }
        "2.5.29.36" => {
            let mut p = Vec::new();
            for item in split_list(v) {
                let (k, n) = item.split_once(':').ok_or(e("invalid syntax"))?;
                let n: i64 = n.trim().parse().map_err(|_| e("invalid value"))?;
                let num = der::int(n);
                let tagn = match k.trim() {
                    "requireExplicitPolicy" => 0,
                    "inhibitPolicyMapping" => 1,
                    _ => return Err(e("invalid syntax")),
                };
                p.push(der::implicit_primitive(tagn, &num[2..]));
            }
            der::seq_vec(&p)
        }
        "2.5.29.54" => der::int(v.parse().map_err(|_| e("invalid value"))?),
        _ => {
            // Arbitrary extension: DER:hex or ASN1:type:value.
            if let Some(h) = v.strip_prefix("DER:") {
                return Ok(Some(Extension {
                    oid: oid_s,
                    critical,
                    value: crate::pbe::hex_decode(&h.replace(':', "")).ok_or(e("bad value"))?,
                }));
            }
            if let Some(a) = v.strip_prefix("ASN1:") {
                asn1_value(a).ok_or(e("bad value"))?
            } else {
                return Err(e("unsupported extension"));
            }
        }
    };
    Ok(Some(Extension {
        oid: oid_s,
        critical,
        value: body,
    }))
}

/// A named BIT STRING (DER drops trailing zero bits, X.690 §11.2.2).
fn bitstring_named(bits: u16) -> Vec<u8> {
    let mut bytes = [0u8; 2];
    for i in 0..16 {
        if bits & (1 << i) != 0 {
            bytes[i / 8] |= 0x80 >> (i % 8);
        }
    }
    let highest = (0..16).rev().find(|i| bits & (1 << i) != 0);
    let Some(h) = highest else {
        return vec![0x03, 0x01, 0x00];
    };
    let nbytes = h / 8 + 1;
    let unused = 7 - (h % 8);
    let mut c = vec![unused as u8];
    c.extend_from_slice(&bytes[..nbytes]);
    der::tlv(0x03, &c)
}

fn parse_ip(s: &str) -> Option<Vec<u8>> {
    if let Ok(a) = s.trim().parse::<std::net::Ipv4Addr>() {
        return Some(a.octets().to_vec());
    }
    if let Ok(a) = s.trim().parse::<std::net::Ipv6Addr>() {
        return Some(a.octets().to_vec());
    }
    None
}

/// A GeneralName from `TYPE:value` (DNS, IP, email, URI, RID, dirName,
/// otherName).
pub fn general_name(item: &str, ctx: &Ctx) -> Option<Vec<u8>> {
    let (t, val) = item.split_once(':')?;
    let val = val.trim();
    Some(match t.trim() {
        "DNS" => der::implicit_primitive(2, val.as_bytes()),
        "email" => der::implicit_primitive(1, val.as_bytes()),
        "URI" => der::implicit_primitive(6, val.as_bytes()),
        "IP" => der::implicit_primitive(7, &parse_ip(val)?),
        "RID" => der::implicit_primitive(8, &der::oid_from_string(&oid::resolve(val)?)?),
        "dirName" => {
            let sec = ctx.conf.section(val)?;
            let mut attrs = Vec::new();
            for (k, v) in sec {
                let k = k.split_once('.').map_or(k.as_str(), |(_, b)| b);
                let o = oid::resolve(k)?;
                attrs.push(
                    crate::csr::make_attr(&o, v.as_bytes(), crate::csr::StringMask(0x2000 | 0x2))
                        .ok()?,
                );
            }
            let n = Name::from_attrs(attrs);
            der::implicit_constructed(4, &n.der)
        }
        "otherName" => {
            let (o, rest) = val.split_once(';')?;
            let o = oid::resolve(o.trim())?;
            let inner = asn1_value(rest.trim())?;
            der::implicit_constructed(0, &[der::oid(&o), der::explicit(0, &inner)].concat())
        }
        _ => return None,
    })
}

/// A value of the ASN1: mini-language: `UTF8String:x`, `IA5STRING:x`,
/// `INTEGER:n`, `BOOLEAN:TRUE`, `OID:x`, `NULL`, `OCTETSTRING:x`,
/// `PRINTABLESTRING:x`.
pub fn asn1_value(a: &str) -> Option<Vec<u8>> {
    let (t, v) = a.split_once(':').unwrap_or((a, ""));
    let t = t.trim().to_ascii_uppercase();
    Some(match t.as_str() {
        "UTF8STRING" | "UTF8" => der::tlv(0x0c, v.as_bytes()),
        "IA5STRING" | "IA5" => der::tlv(0x16, v.as_bytes()),
        "PRINTABLESTRING" | "PRINTABLE" => der::tlv(0x13, v.as_bytes()),
        "VISIBLESTRING" | "VISIBLE" => der::tlv(0x1a, v.as_bytes()),
        "T61STRING" | "T61" | "TELETEXSTRING" => der::tlv(0x14, v.as_bytes()),
        "OCTETSTRING" | "OCT" => der::octet_string(v.as_bytes()),
        "INTEGER" | "INT" => der::int(v.trim().parse().ok()?),
        "BOOLEAN" | "BOOL" => der::boolean(matches!(
            v.trim().to_ascii_uppercase().as_str(),
            "TRUE" | "YES" | "Y" | "T"
        )),
        "NULL" => der::null(),
        "OBJECT" | "OID" => der::oid(&oid::resolve(v.trim())?),
        _ => return None,
    })
}

fn policy_section(sec: &str, ctx: &Ctx) -> Option<Vec<u8>> {
    let entries = ctx.conf.section(sec)?;
    let mut id = None;
    let mut quals = Vec::new();
    for (k, v) in entries {
        let base = k.split('.').next().unwrap_or(k);
        match base {
            "policyIdentifier" => id = Some(oid::resolve(v)?),
            "CPS" => quals.push(der::seq_vec(&[
                der::oid("1.3.6.1.5.5.7.2.1"),
                der::tlv(0x16, v.as_bytes()),
            ])),
            "userNotice" => {
                let nsec = v.strip_prefix('@')?;
                let ne = ctx.conf.section(nsec)?;
                let mut org = None;
                let mut nums = Vec::new();
                let mut text = None;
                for (nk, nv) in ne {
                    match nk.as_str() {
                        "organization" => org = Some(nv.clone()),
                        "noticeNumbers" => {
                            for n in split_list(nv) {
                                nums.push(der::int(n.parse().ok()?));
                            }
                        }
                        "explicitText" => text = Some(nv.clone()),
                        _ => {}
                    }
                }
                let mut un = Vec::new();
                if let Some(o) = org {
                    un.push(der::seq_vec(&[
                        der::tlv(0x0c, o.as_bytes()),
                        der::seq_vec(&nums),
                    ]));
                }
                if let Some(t) = text {
                    un.push(der::tlv(0x0c, t.as_bytes()));
                }
                quals.push(der::seq_vec(&[
                    der::oid("1.3.6.1.5.5.7.2.2"),
                    der::seq_vec(&un),
                ]));
            }
            _ => {}
        }
    }
    let id = id?;
    if quals.is_empty() {
        Some(der::seq_vec(&[der::oid(&id)]))
    } else {
        Some(der::seq_vec(&[der::oid(&id), der::seq_vec(&quals)]))
    }
}
