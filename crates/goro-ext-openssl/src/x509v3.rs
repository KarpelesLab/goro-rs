//! X509v3 extensions: printing (X509V3_EXT_print), creation from a config
//! section (X509V3_EXT_add_nconf) and purpose checks (v3_purp.c).

use crate::asn1::{self, Tlv};
use crate::cert::{Cert, Ext};
use crate::conf::Conf;
use crate::digest::Md;
use crate::err;
use crate::name::{self, Ava, Name};
use crate::obj;

pub const OID_SKID: &str = "2.5.29.14";
pub const OID_KU: &str = "2.5.29.15";
pub const OID_SAN: &str = "2.5.29.17";
pub const OID_IAN: &str = "2.5.29.18";
pub const OID_BC: &str = "2.5.29.19";
pub const OID_CRLDP: &str = "2.5.29.31";
pub const OID_CERTPOL: &str = "2.5.29.32";
pub const OID_AKID: &str = "2.5.29.35";
pub const OID_EKU: &str = "2.5.29.37";
pub const OID_AIA: &str = "1.3.6.1.5.5.7.1.1";
pub const OID_NS_CERT_TYPE: &str = "2.16.840.1.113730.1.1";

const KU_NAMES: &[(&str, &str)] = &[
    ("Digital Signature", "digitalSignature"),
    ("Non Repudiation", "nonRepudiation"),
    ("Key Encipherment", "keyEncipherment"),
    ("Data Encipherment", "dataEncipherment"),
    ("Key Agreement", "keyAgreement"),
    ("Certificate Sign", "keyCertSign"),
    ("CRL Sign", "cRLSign"),
    ("Encipher Only", "encipherOnly"),
    ("Decipher Only", "decipherOnly"),
];

const NS_NAMES: &[(&str, &str)] = &[
    ("SSL Client", "client"),
    ("SSL Server", "server"),
    ("S/MIME", "email"),
    ("Object Signing", "objsign"),
    ("Unused", "reserved"),
    ("SSL CA", "sslCA"),
    ("S/MIME CA", "emailCA"),
    ("Object Signing CA", "objCA"),
];

pub fn hex_colon(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{:02X}", x))
        .collect::<Vec<_>>()
        .join(":")
}

fn bit_set(bits: &[u8], i: usize) -> bool {
    bits.get(i / 8)
        .map(|b| b & (0x80 >> (i % 8)) != 0)
        .unwrap_or(false)
}

fn bitstring_names(t: &Tlv, table: &[(&str, &str)]) -> Option<String> {
    let bits = t.bits()?;
    let names: Vec<&str> = table
        .iter()
        .enumerate()
        .filter(|(i, _)| bit_set(bits, *i))
        .map(|(_, n)| n.0)
        .collect();
    Some(names.join(", "))
}

fn ip_text(b: &[u8]) -> String {
    if b.len() == 4 {
        format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
    } else if b.len() == 16 {
        b.chunks(2)
            .map(|c| format!("{:X}", ((c[0] as u16) << 8) | c[1] as u16))
            .collect::<Vec<_>>()
            .join(":")
    } else {
        "<invalid>".to_string()
    }
}

fn ip_mask_text(b: &[u8]) -> String {
    if b.len() == 8 {
        format!("{}/{}", ip_text(&b[..4]), ip_text(&b[4..]))
    } else if b.len() == 32 {
        format!("{}/{}", ip_text(&b[..16]), ip_text(&b[16..]))
    } else {
        "IP Address:<invalid>".to_string()
    }
}

/// X509_NAME_print() (XN_FLAG_COMPAT): the X509_NAME_oneline() form with
/// a slash turned into ", " only before an "X=" or "XY=" (upper case) type.
pub fn name_print_compat(n: &Name) -> String {
    let b = n.oneline().into_bytes();
    let s = b.get(1..).unwrap_or(&[]);
    let mut out = Vec::new();
    let mut c = 0;
    let up = |i: usize| s.get(i).is_some_and(|x| x.is_ascii_uppercase());
    let at = |i: usize| s.get(i).copied();
    for i in 0..=s.len() {
        let end = i == s.len();
        let split = !end
            && s[i] == b'/'
            && up(i + 1)
            && (at(i + 2) == Some(b'=') || (up(i + 2) && at(i + 3) == Some(b'=')));
        if split || end {
            out.extend_from_slice(&s[c..i]);
            c = i + 1;
            if !end {
                out.extend_from_slice(b", ");
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// X509_NAME_print_ex(.., XN_FLAG_ONELINE)
pub fn name_print_oneline(n: &Name) -> String {
    let mut parts = Vec::new();
    for rdn in &n.rdns {
        let mut sub = Vec::new();
        for e in rdn {
            let sn = obj::by_oid(&e.oid)
                .map(|o| o.sn)
                .unwrap_or_else(|| e.oid.clone());
            let v = name::to_utf8(e.tag, &e.value).unwrap_or_else(|| e.value.clone());
            sub.push(format!("{} = {}", sn, escape_rfc2253(&v)));
        }
        parts.push(sub.join(" + "));
    }
    parts.join(", ")
}

fn escape_rfc2253(v: &[u8]) -> String {
    let mut out = String::new();
    let n = v.len();
    for (i, &c) in v.iter().enumerate() {
        let special = matches!(c, b',' | b'+' | b'"' | b'\\' | b'<' | b'>' | b';')
            || ((i == 0) && (c == b'#' || c == b' '))
            || (i == n - 1 && c == b' ');
        if special {
            out.push('\\');
            out.push(c as char);
        } else if !(0x20..0x7f).contains(&c) {
            out.push_str(&format!("\\{:02X}", c));
        } else {
            out.push(c as char);
        }
    }
    out
}

/// The otherName forms OpenSSL prints: (type, label, value) for
/// SmtpUTF8Mailbox, XmppAddr, SRVName, UPN and NAIRealm whose value has the
/// expected string type (IA5String for SRVName, UTF8String otherwise).
fn other_name(g: &Tlv) -> Option<(&'static str, Vec<u8>)> {
    let parts = asn1::parse_all(g.content)?;
    let oid = parts.first()?.oid()?;
    let val = asn1::parse_one(parts.get(1)?.content)?;
    let (label, tag) = match oid.as_str() {
        "1.3.6.1.5.5.7.8.9" => ("SmtpUTF8Mailbox", 0x0c),
        "1.3.6.1.5.5.7.8.5" => ("XmppAddr", 0x0c),
        "1.3.6.1.5.5.7.8.7" => ("SRVName", 0x16),
        "1.3.6.1.4.1.311.20.2.3" => ("UPN", 0x0c),
        "1.3.6.1.5.5.7.8.8" => ("NAIRealm", 0x0c),
        _ => return None,
    };
    if val.tag != tag {
        return None;
    }
    Some((label, val.content.to_vec()))
}

/// GENERAL_NAME_print()
pub fn general_name_print(g: &Tlv) -> String {
    let raw = |c: &[u8]| String::from_utf8_lossy(c).to_string();
    match g.tag & 0x1f {
        0 => match other_name(g) {
            Some((label, v)) => format!("othername:{}:{}", label, raw(&v)),
            None => "othername:<unsupported>".to_string(),
        },
        1 => format!("email:{}", raw(g.content)),
        2 => format!("DNS:{}", raw(g.content)),
        3 => "X400Name:<unsupported>".to_string(),
        4 => match asn1::parse_one(g.content).and_then(|t| Name::parse(&t)) {
            Some(n) => format!("DirName:{}", name_print_oneline(&n)),
            None => "DirName:<invalid>".to_string(),
        },
        5 => "EdiPartyName:<unsupported>".to_string(),
        6 => format!("URI:{}", raw(g.content)),
        7 => format!("IP Address:{}", ip_text(g.content)),
        8 => format!(
            "Registered ID:{}",
            asn1::oid_to_string(g.content)
                .map(|o| obj::oid_ln(&o))
                .unwrap_or_default()
        ),
        _ => String::new(),
    }
}

/// i2v_GENERAL_NAME() rendering ("name:value").
fn general_name_v(g: &Tlv) -> String {
    match g.tag & 0x1f {
        4 => match asn1::parse_one(g.content).and_then(|t| Name::parse(&t)) {
            Some(n) => format!("DirName:{}", n.oneline()),
            None => "DirName:<invalid>".to_string(),
        },
        7 => format!("IP Address:{}", ip_text(g.content)),
        // i2v_GENERAL_NAME(): X509V3_add_value("othername: UPN", ...)
        0 => match other_name(g) {
            Some((label, v)) => {
                format!("othername: {}:{}", label, String::from_utf8_lossy(&v))
            }
            None => "othername:<unsupported>".to_string(),
        },
        _ => general_name_print(g),
    }
}

/// php-src openssl_x509v3_subjectAltName()
pub fn subject_alt_name(value: &[u8]) -> Option<Vec<u8>> {
    let t = asn1::parse_one(value)?;
    if t.tag != asn1::SEQUENCE {
        return None;
    }
    let names = t.children()?;
    let mut out = Vec::new();
    let n = names.len();
    for (i, g) in names.iter().enumerate() {
        match g.tag & 0x1f {
            1 => {
                out.extend_from_slice(b"email:");
                out.extend_from_slice(g.content);
            }
            2 => {
                out.extend_from_slice(b"DNS:");
                out.extend_from_slice(g.content);
            }
            6 => {
                out.extend_from_slice(b"URI:");
                out.extend_from_slice(g.content);
            }
            _ => out.extend_from_slice(general_name_print(g).as_bytes()),
        }
        if i + 1 < n {
            out.extend_from_slice(b", ");
        }
    }
    Some(out)
}

fn print_policies(value: &[u8]) -> Option<String> {
    let t = asn1::parse_one(value)?;
    let mut out = String::new();
    for p in t.children()? {
        let c = p.children()?;
        let oid = c.first()?.oid()?;
        out.push_str(&format!("Policy: {}\n", obj::oid_ln(&oid)));
        if let Some(quals) = c.get(1) {
            for q in quals.children()? {
                let qc = q.children()?;
                let qid = qc.first()?.oid()?;
                match qid.as_str() {
                    "1.3.6.1.5.5.7.2.1" => {
                        out.push_str(&format!(
                            "  CPS: {}\n",
                            String::from_utf8_lossy(qc.get(1)?.content)
                        ));
                    }
                    "1.3.6.1.5.5.7.2.2" => {
                        out.push_str("  User Notice:\n");
                        if let Some(un) = qc.get(1) {
                            for x in un.children()? {
                                if x.tag == asn1::SEQUENCE {
                                    let nr = x.children()?;
                                    out.push_str(&format!(
                                        "    Organization: {}\n",
                                        String::from_utf8_lossy(nr.first()?.content)
                                    ));
                                    if let Some(nums) = nr.get(1) {
                                        let ns: Vec<String> = nums
                                            .children()?
                                            .iter()
                                            .map(|n| n.uint().to_dec())
                                            .collect();
                                        out.push_str(&format!(
                                            "    Number{}: {}\n",
                                            if ns.len() > 1 { "s" } else { "" },
                                            ns.join(", ")
                                        ));
                                    }
                                } else {
                                    // print_notice(): "%.*s" of the raw string
                                    // bytes, so a BMPString stops at its
                                    // first NUL byte
                                    let raw = x.content;
                                    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                                    out.push_str(&format!(
                                        "    Explicit Text: {}\n",
                                        String::from_utf8_lossy(&raw[..end])
                                    ));
                                }
                            }
                        }
                    }
                    _ => {
                        out.push_str(&format!("  Unknown Qualifier: {}\n", obj::oid_ln(&qid)));
                    }
                }
            }
        }
    }
    Some(out)
}

fn print_crldp(value: &[u8]) -> Option<String> {
    let t = asn1::parse_one(value)?;
    let mut out = String::new();
    for (i, dp) in t.children()?.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        for f in dp.children()? {
            match f.context() {
                Some(0) => {
                    let dpn = asn1::parse_one(f.content)?;
                    match dpn.context() {
                        Some(0) => {
                            out.push_str("Full Name:\n");
                            for g in asn1::parse_all(dpn.content)? {
                                out.push_str(&format!("  {}\n", general_name_print(&g)));
                            }
                        }
                        Some(1) => {
                            out.push_str("Relative Name:\n");
                            let set = asn1::tlv(asn1::SET, dpn.content);
                            let n = Name::parse(&asn1::parse_one(&asn1::seq(&[set]))?)?;
                            out.push_str(&format!("  {}\n", name_print_oneline(&n)));
                        }
                        _ => {}
                    }
                }
                Some(1) => {
                    let names = [
                        "Unused",
                        "Key Compromise",
                        "CA Compromise",
                        "Affiliation Changed",
                        "Superseded",
                        "Cessation Of Operation",
                        "Certificate Hold",
                        "Privilege Withdrawn",
                        "AA Compromise",
                    ];
                    let bits = if f.content.is_empty() {
                        &[][..]
                    } else {
                        &f.content[1..]
                    };
                    let r: Vec<&str> = names
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| bit_set(bits, *i))
                        .map(|(_, n)| *n)
                        .collect();
                    out.push_str(&format!("Reasons: {}\n", r.join(", ")));
                }
                Some(2) => {
                    out.push_str("CRL Issuer:\n");
                    for g in asn1::parse_all(f.content)? {
                        out.push_str(&format!("  {}\n", general_name_print(&g)));
                    }
                }
                _ => {}
            }
        }
    }
    Some(out)
}

/// X509V3_EXT_print(out, ext, 0, 0). None = no printer (use raw data).
pub fn ext_print(e: &Ext) -> Option<Vec<u8>> {
    let v = &e.value;
    let s: String = match e.oid.as_str() {
        OID_BC => {
            let t = asn1::parse_one(v)?;
            let c = t.children()?;
            let mut ca = false;
            let mut pathlen = None;
            for x in &c {
                if x.tag == asn1::BOOLEAN {
                    ca = x.content.first().copied().unwrap_or(0) != 0;
                } else if x.tag == asn1::INTEGER {
                    pathlen = Some(x.uint().to_dec());
                }
            }
            let mut s = format!("CA:{}", if ca { "TRUE" } else { "FALSE" });
            if let Some(p) = pathlen {
                s.push_str(&format!(", pathlen:{}", p));
            }
            s
        }
        OID_KU => bitstring_names(&asn1::parse_one(v)?, KU_NAMES)?,
        OID_NS_CERT_TYPE => bitstring_names(&asn1::parse_one(v)?, NS_NAMES)?,
        OID_EKU => {
            let t = asn1::parse_one(v)?;
            let names: Option<Vec<String>> = t
                .children()?
                .iter()
                .map(|o| o.oid().map(|s| obj::oid_ln(&s)))
                .collect();
            names?.join(", ")
        }
        OID_SKID => {
            let t = asn1::parse_one(v)?;
            if t.tag != asn1::OCTET_STRING {
                return None;
            }
            hex_colon(t.content)
        }
        OID_AKID => {
            let t = asn1::parse_one(v)?;
            let mut lines = Vec::new();
            let fields = t.children()?;
            let only_keyid = fields.iter().all(|f| f.context() == Some(0));
            for f in fields {
                match f.context() {
                    Some(0) if only_keyid => lines.push(hex_colon(f.content)),
                    Some(0) => lines.push(format!("keyid:{}", hex_colon(f.content))),
                    Some(1) => {
                        for g in asn1::parse_all(f.content)? {
                            lines.push(general_name_v(&g));
                        }
                    }
                    Some(2) => {
                        // ASN1_INTEGER data holds the magnitude without sign padding
                        let mut c = f.content;
                        while c.len() > 1 && c[0] == 0 {
                            c = &c[1..];
                        }
                        lines.push(format!("serial:{}", hex_colon(c)))
                    }
                    _ => {}
                }
            }
            // i2v_AUTHORITY_KEYID() of an empty AKID (self-signed) returns
            // no list: X509V3_EXT_print() fails, the raw value is used
            if lines.is_empty() {
                return None;
            }
            lines.join("\n")
        }
        OID_SAN | OID_IAN => {
            let t = asn1::parse_one(v)?;
            let names: Vec<String> = t.children()?.iter().map(general_name_v).collect();
            names.join(", ")
        }
        OID_CRLDP => print_crldp(v)?,
        OID_CERTPOL => {
            let mut p = print_policies(v)?;
            if p.ends_with('\n') {
                p.pop();
            }
            p
        }
        OID_AIA => {
            let t = asn1::parse_one(v)?;
            let mut lines = Vec::new();
            for ad in t.children()? {
                let c = ad.children()?;
                let m = c.first()?.oid()?;
                let g = c.get(1)?;
                lines.push(format!("{} - {}", obj::oid_ln(&m), general_name_v(g)));
            }
            lines.join("\n")
        }
        "2.16.840.1.113730.1.13"
        | "2.16.840.1.113730.1.2"
        | "2.16.840.1.113730.1.3"
        | "2.16.840.1.113730.1.4"
        | "2.16.840.1.113730.1.7"
        | "2.16.840.1.113730.1.8"
        | "2.16.840.1.113730.1.12" => {
            let t = asn1::parse_one(v)?;
            if t.tag != asn1::IA5_STRING {
                return None;
            }
            return Some(t.content.to_vec());
        }
        "2.5.29.54" => {
            let t = asn1::parse_one(v)?;
            t.uint().to_dec()
        }
        "2.5.29.36" => {
            let t = asn1::parse_one(v)?;
            let mut parts = Vec::new();
            for f in t.children()? {
                match f.context() {
                    Some(0) => parts.push(format!(
                        "Require Explicit Policy:{}",
                        crate::bn::Bn::from_be(f.content).to_dec()
                    )),
                    Some(1) => parts.push(format!(
                        "Inhibit Policy Mapping:{}",
                        crate::bn::Bn::from_be(f.content).to_dec()
                    )),
                    _ => {}
                }
            }
            parts.join(", ")
        }
        "2.5.29.30" => {
            let t = asn1::parse_one(v)?;
            // i2r_NAME_CONSTRAINTS(): subtrees separated (not terminated)
            // by newlines, a newline between the two lists
            let mut sections = Vec::new();
            for f in t.children()? {
                let label = match f.context() {
                    Some(0) => "Permitted",
                    Some(1) => "Excluded",
                    _ => continue,
                };
                let mut lines = Vec::new();
                for st in asn1::parse_all(f.content)? {
                    let sc = st.children()?;
                    let g = sc.first()?;
                    if g.tag & 0x1f == 7 {
                        lines.push(format!("  IP:{}", ip_mask_text(g.content)));
                    } else {
                        lines.push(format!("  {}", general_name_print(g)));
                    }
                }
                if !lines.is_empty() {
                    sections.push(format!("{}:\n{}", label, lines.join("\n")));
                }
            }
            sections.join("\n")
        }
        // ct_precert_scts / ct_cert_scts: i2r_SCT_LIST()
        "1.3.6.1.4.1.11129.2.4.2" | "1.3.6.1.4.1.11129.2.4.5" => {
            let t = asn1::parse_one(v)?;
            if t.tag != asn1::OCTET_STRING {
                return None;
            }
            sct_list_print(t.content)?
        }
        // ct_precert_poison: i2s_poison()
        "1.3.6.1.4.1.11129.2.4.3" => "NULL".to_string(),
        "1.3.6.1.5.5.7.1.24" => {
            let t = asn1::parse_one(v)?;
            let f: Vec<String> = t
                .children()?
                .iter()
                .map(|x| match x.as_i64() {
                    Some(5) => "status_request".to_string(),
                    Some(17) => "status_request_v2".to_string(),
                    Some(n) => n.to_string(),
                    None => String::new(),
                })
                .collect();
            f.join(", ")
        }
        "2.5.29.16" => {
            let t = asn1::parse_one(v)?;
            let mut out = String::new();
            for f in t.children()? {
                let label = match f.context() {
                    Some(0) => "Not Before: ",
                    Some(1) => "Not After: ",
                    _ => continue,
                };
                out.push_str(label);
                out.push_str(&gentime_print(f.content));
                if f.context() == Some(0) {
                    out.push_str(", ");
                }
            }
            out
        }
        _ => return None,
    };
    Some(s.into_bytes())
}

/// BIO_hex_string(): colon-separated hex, `width` bytes per line, later
/// lines indented by `indent`.
fn bio_hex_string(indent: usize, width: usize, d: &[u8]) -> String {
    let mut out = String::new();
    let Some((last, head)) = d.split_last() else {
        return out;
    };
    let mut j = 0;
    for (i, b) in head.iter().enumerate() {
        if i > 0 && j == 0 {
            out.push_str(&" ".repeat(indent));
        }
        out.push_str(&format!("{:02X}:", b));
        j = (j + 1) % width;
        if j == 0 {
            out.push('\n');
        }
    }
    if !head.is_empty() && j == 0 {
        out.push_str(&" ".repeat(indent));
    }
    out.push_str(&format!("{:02X}", last));
    out
}

/// SCT_LIST_print(list, out, 0, "\n", NULL) over the TLS-encoded list
/// (o2i_SCT_LIST); None when it does not decode.
#[inline(never)]
fn sct_list_print(d: &[u8]) -> Option<String> {
    let u16at = |b: &[u8], i: usize| -> Option<usize> {
        Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]) as usize)
    };
    let total = u16at(d, 0)?;
    if total + 2 != d.len() {
        return None;
    }
    let mut scts = Vec::new();
    let mut i = 2;
    while i < d.len() {
        let l = u16at(d, i)?;
        let sct = d.get(i + 2..i + 2 + l)?;
        if sct.is_empty() {
            return None;
        }
        scts.push(sct);
        i += 2 + l;
    }
    if scts.is_empty() {
        return None;
    }
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut out = Vec::new();
    for sct in scts {
        let mut s = String::from("Signed Certificate Timestamp:\n    Version   : ");
        if sct[0] != 0 {
            s.push_str("unknown\n");
            s.push_str(&" ".repeat(16));
            s.push_str(&bio_hex_string(16, 16, sct));
            out.push(s);
            continue;
        }
        // v1: log id, timestamp, extensions, signature
        let log_id = sct.get(1..33)?;
        let ts = u64::from_be_bytes(sct.get(33..41)?.try_into().ok()?);
        let el = u16at(sct, 41)?;
        let ext = sct.get(43..43 + el)?;
        let p = 43 + el;
        let (hash_alg, sig_alg) = (*sct.get(p)?, *sct.get(p + 1)?);
        let sl = u16at(sct, p + 2)?;
        let sig = sct.get(p + 4..p + 4 + sl)?;
        if p + 4 + sl != sct.len() {
            return None;
        }
        s.push_str("v1 (0x0)\n    Log ID    : ");
        s.push_str(&bio_hex_string(16, 16, log_id));
        let secs = (ts / 1000) as i64;
        let (y, m, dd) = crate::cert::civil_from_days(secs.div_euclid(86400));
        let r = secs.rem_euclid(86400);
        s.push_str(&format!(
            "\n    Timestamp : {} {:2} {:02}:{:02}:{:02}.{:03} {} GMT",
            months[(m - 1) as usize],
            dd,
            r / 3600,
            (r / 60) % 60,
            r % 60,
            ts % 1000,
            y
        ));
        s.push_str("\n    Extensions: ");
        if ext.is_empty() {
            s.push_str("none");
        } else {
            s.push_str(&bio_hex_string(16, 16, ext));
        }
        s.push_str("\n    Signature : ");
        // SCT_get_signature_nid(): only SHA-256 with ECDSA or RSA is named
        match (hash_alg, sig_alg) {
            (4, 3) => s.push_str("ecdsa-with-SHA256"),
            (4, 1) => s.push_str("sha256WithRSAEncryption"),
            _ => s.push_str(&format!("{:02X}{:02X}", hash_alg, sig_alg)),
        }
        s.push_str("\n                ");
        s.push_str(&bio_hex_string(16, 16, sig));
        out.push(s);
    }
    Some(out.join("\n"))
}

/// ASN1_TIME_print() style "Mon DD HH:MM:SS YYYY GMT".
pub fn time_print(tag: u8, v: &[u8]) -> String {
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    match crate::cert::asn1_time_to_unix(tag, v) {
        Some(t) => {
            let (y, m, d) = crate::cert::civil_from_days(t.div_euclid(86400));
            let s = t.rem_euclid(86400);
            format!(
                "{} {:2} {:02}:{:02}:{:02} {} GMT",
                months[(m - 1) as usize],
                d,
                s / 3600,
                (s / 60) % 60,
                s % 60,
                y
            )
        }
        None => "Bad time value".to_string(),
    }
}

fn gentime_print(v: &[u8]) -> String {
    time_print(asn1::GENERALIZED_TIME, v)
}

// ---------------------------------------------------------------------------
// Extension creation from configuration (v3_conf.c)
// ---------------------------------------------------------------------------

pub struct V3Ctx<'a> {
    pub test: bool,
    pub issuer: Option<&'a Cert>,
    /// Subject public key BIT STRING content (for subjectKeyIdentifier=hash).
    pub subject_pubkey_bits: Option<Vec<u8>>,
    pub subject_name: Option<&'a Name>,
    /// Extensions already added to the subject cert.
    pub subject_exts: &'a [Ext],
    /// Issuer is the certificate being built (self-signed).
    pub self_signed: bool,
    /// Issuer name and serial of the cert being built (when self-signed).
    pub self_issuer_name: Option<&'a Name>,
    pub self_serial: Option<&'a [u8]>,
}

fn x509v3_err(reason: u32, s: &str) {
    err::push(err::LIB_X509V3, reason, s);
}

/// X509V3_parse_list()
pub fn parse_list(s: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for item in s.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        match item.find(':') {
            Some(i) => out.push((
                item[..i].trim().to_string(),
                Some(item[i + 1..].trim().to_string()),
            )),
            None => out.push((item.to_string(), None)),
        }
    }
    out
}

fn get_bool(v: &str) -> Option<bool> {
    match v {
        "TRUE" | "true" | "Y" | "y" | "YES" | "yes" => Some(true),
        "FALSE" | "false" | "N" | "n" | "NO" | "no" => Some(false),
        _ => None,
    }
}

fn encode_bits(bits: &[usize]) -> Vec<u8> {
    let max = bits.iter().copied().max().map(|m| m + 1).unwrap_or(0);
    let nbytes = max.div_ceil(8);
    let mut b = vec![0u8; nbytes];
    for &i in bits {
        b[i / 8] |= 0x80 >> (i % 8);
    }
    // trim trailing zero bytes and compute unused bits
    while b.last() == Some(&0) {
        b.pop();
    }
    let unused = match b.last() {
        Some(&last) => last.trailing_zeros() as u8,
        None => 0,
    };
    let mut c = vec![unused];
    c.extend(b);
    asn1::tlv(asn1::BIT_STRING, &c)
}

fn hexstr2buf(s: &str) -> Option<Vec<u8>> {
    let clean: String = s.chars().filter(|c| *c != ':').collect();
    if !clean.len().is_multiple_of(2) || clean.is_empty() {
        return None;
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).ok())
        .collect()
}

fn parse_ip(s: &str) -> Option<Vec<u8>> {
    if let Ok(v4) = s.parse::<std::net::Ipv4Addr>() {
        return Some(v4.octets().to_vec());
    }
    if let Ok(v6) = s.parse::<std::net::Ipv6Addr>() {
        return Some(v6.octets().to_vec());
    }
    None
}

/// v2i_GENERAL_NAME_ex(): one GeneralName from "type:value".
fn make_general_name(
    conf: &Conf,
    ctx: &V3Ctx,
    name: &str,
    value: &str,
    out: &mut Vec<Vec<u8>>,
) -> bool {
    // ossl_v3_name_cmp(): case-sensitive, an optional ".suffix" allowed
    // (the keys of an @section: DNS.1, IP.2, ...)
    let base = name.split('.').next().unwrap_or(name);
    match base {
        "email" => {
            if value == "copy" || value == "move" {
                if let Some(n) = ctx.subject_name {
                    for e in n.entries() {
                        if e.oid == "1.2.840.113549.1.9.1" {
                            out.push(asn1::implicit(1, &e.value));
                        }
                    }
                }
                return true;
            }
            out.push(asn1::implicit(1, value.as_bytes()));
        }
        "DNS" => out.push(asn1::implicit(2, value.as_bytes())),
        "URI" => out.push(asn1::implicit(6, value.as_bytes())),
        "IP" => match parse_ip(value) {
            Some(b) => out.push(asn1::implicit(7, &b)),
            None => {
                x509v3_err(118, "bad ip address");
                return false;
            }
        },
        "RID" => match obj::txt2obj(value) {
            Some(o) => out.push(asn1::implicit(
                8,
                &asn1::oid_content(&o.oid).unwrap_or_default(),
            )),
            None => {
                x509v3_err(119, "bad object");
                return false;
            }
        },
        "dirName" => {
            let Some(sect) = conf.section(value) else {
                x509v3_err(150, "section not found");
                return false;
            };
            let mut n = Name::default();
            for (k, v) in sect {
                let key = k.rsplit(['.', ':', ',']).next().unwrap_or(k);
                let Some(o) = obj::txt2obj(key) else {
                    x509v3_err(119, "bad object");
                    return false;
                };
                match name::make_string(&o.oid, v.as_bytes(), crate::req::global_string_mask()) {
                    Ok((tag, val)) => n.push(Ava {
                        oid: o.oid.clone(),
                        tag,
                        value: val,
                    }),
                    Err(e) => {
                        name::push_str_err(&e);
                        return false;
                    }
                }
            }
            out.push(asn1::explicit(4, &n.to_der()));
        }
        "otherName" => {
            // OID;TYPE:value  (UTF8 only)
            let Some((oidpart, rest)) = value.split_once(';') else {
                x509v3_err(147, "invalid othername");
                return false;
            };
            let Some(o) = obj::txt2obj(oidpart.trim()) else {
                x509v3_err(147, "invalid othername");
                return false;
            };
            let v = rest.split_once(':').map(|(_, v)| v).unwrap_or(rest);
            let inner = asn1::seq(&[
                asn1::oid(&o.oid),
                asn1::explicit(0, &asn1::tlv(asn1::UTF8_STRING, v.as_bytes())),
            ]);
            // OtherName is [0] IMPLICIT SEQUENCE
            let c = asn1::parse_one(&inner)
                .map(|t| t.content.to_vec())
                .unwrap_or_default();
            out.push(asn1::tlv(0xa0, &c));
        }
        _ => {
            x509v3_err(117, "unsupported option");
            return false;
        }
    }
    true
}

fn general_names(
    conf: &Conf,
    ctx: &V3Ctx,
    list: &[(String, Option<String>)],
    ian: bool,
) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (n, v) in list {
        if ian && n == "issuer" && v.as_deref() == Some("copy") {
            if let Some(iss) = ctx.issuer {
                if let Some(e) = iss.ext(OID_SAN) {
                    if let Some(t) = asn1::parse_one(&e.value) {
                        out.push(t.content.to_vec());
                    }
                }
            }
            continue;
        }
        if !make_general_name(conf, ctx, n, v.as_deref().unwrap_or(""), &mut out) {
            return None;
        }
    }
    Some(asn1::seq(&out))
}

fn asn1_gen_err(reason: u32, text: &str) {
    crate::err::push(crate::err::LIB_ASN1, reason, text);
}

/// ASN1_generate_v3(): the "[modifier,]...TYPE:value" mini-language of
/// generic configuration extensions (primitive types, SEQUENCE/SET from a
/// section, EXPLICIT/IMPLICIT tagging, OCTWRAP/SEQWRAP/SETWRAP/BITWRAP and
/// FORMAT:ASCII|UTF8|HEX|BITLIST).
#[inline(never)]
fn asn1_generate(conf: &Conf, s: &str, depth: usize) -> Option<Vec<u8>> {
    if depth > 50 {
        asn1_gen_err(181, "illegal nested tagging");
        return None;
    }
    // modifiers, then the type
    // EXPLICIT tags and *WRAPs in order (the last listed is innermost);
    // a single IMPLICIT tag replaces the base type's
    let mut outer: Vec<Result<u8, &str>> = Vec::new();
    let mut implicit: Option<u8> = None;
    let mut format = "ASCII";
    let mut rest = s.trim_start();
    let parse_tag = |v: &str| -> Option<u8> {
        let v = v.trim();
        let (num, class) = match v.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
            Some((i, _)) => (&v[..i], &v[i..]),
            None => (v, ""),
        };
        let n: u8 = num.parse().ok().filter(|n| *n < 31)?;
        let cls = match class {
            "" | "C" => 0x80,
            "A" => 0x40,
            "P" => 0xc0,
            "U" => 0x00,
            _ => return None,
        };
        Some(cls | n)
    };
    let (ty, value) = loop {
        let (item, tail) = match rest.find(',') {
            Some(i)
                if rest[..i].split(':').next().is_some_and(|k| {
                    matches!(
                        k.trim(),
                        "EXP"
                            | "EXPLICIT"
                            | "IMP"
                            | "IMPLICIT"
                            | "OCTWRAP"
                            | "SEQWRAP"
                            | "SETWRAP"
                            | "BITWRAP"
                            | "FORM"
                            | "FORMAT"
                    )
                }) =>
            {
                (&rest[..i], Some(&rest[i + 1..]))
            }
            _ => (rest, None),
        };
        let (k, v) = match item.split_once(':') {
            Some((k, v)) => (k.trim(), Some(v)),
            None => (item.trim(), None),
        };
        match k {
            "EXP" | "EXPLICIT" | "IMP" | "IMPLICIT" => {
                let Some(t) = v.and_then(parse_tag) else {
                    asn1_gen_err(186, "invalid modifier");
                    return None;
                };
                if k.starts_with("EXP") {
                    outer.push(Ok(t));
                } else if implicit.replace(t).is_some() {
                    asn1_gen_err(179, "illegal implicit tag");
                    return None;
                }
            }
            "OCTWRAP" | "SEQWRAP" | "SETWRAP" | "BITWRAP" => outer.push(Err(k)),
            "FORM" | "FORMAT" => match v.map(str::trim) {
                Some(f @ ("ASCII" | "UTF8" | "HEX" | "BITLIST")) => format = f,
                _ => {
                    asn1_gen_err(177, "illegal format");
                    return None;
                }
            },
            _ => break (k, v.unwrap_or("")),
        }
        match tail {
            Some(t) => rest = t.trim_start(),
            None => {
                asn1_gen_err(194, "unknown tag");
                return None;
            }
        }
    };
    let str_tag = |t: &str| -> Option<u8> {
        Some(match t {
            "UNIVERSALSTRING" | "UNIV" => 0x1c,
            "IA5" | "IA5STRING" => asn1::IA5_STRING,
            "UTF8" | "UTF8String" => asn1::UTF8_STRING,
            "BMP" | "BMPSTRING" => 0x1e,
            "VISIBLESTRING" | "VISIBLE" => 0x1a,
            "PRINTABLESTRING" | "PRINTABLE" => 0x13,
            "T61" | "T61STRING" | "TELETEXSTRING" => 0x14,
            "GeneralString" | "GENSTR" => 0x1b,
            "NUMERIC" | "NUMERICSTRING" => 0x12,
            _ => return None,
        })
    };
    let text = |v: &str| -> Option<Vec<u8>> {
        match format {
            "HEX" => {
                let b = hexstr2buf(v);
                if b.is_none() {
                    asn1_gen_err(178, "illegal hex");
                }
                b
            }
            _ => Some(v.as_bytes().to_vec()),
        }
    };
    let mut der = match ty {
        "BOOL" | "BOOLEAN" => match get_bool(value.trim()) {
            Some(b) if format == "ASCII" => asn1::boolean(b),
            r => {
                if format == "ASCII" && r.is_none() {
                    // X509V3_bool_from_string()
                    x509v3_err(104, "invalid boolean string");
                }
                asn1_gen_err(176, "illegal boolean");
                return None;
            }
        },
        "NULL" => {
            if !value.is_empty() {
                asn1_gen_err(182, "illegal null value");
                return None;
            }
            asn1::null()
        }
        "INT" | "INTEGER" | "ENUM" | "ENUMERATED" => {
            if format != "ASCII" {
                asn1_gen_err(185, "integer not ascii format");
                return None;
            }
            let v = value.trim();
            let (neg, digits) = match v.strip_prefix('-') {
                Some(d) => (true, d),
                None => (false, v),
            };
            let n = match digits
                .strip_prefix("0x")
                .or_else(|| digits.strip_prefix("0X"))
            {
                Some(h) => i64::from_str_radix(h, 16).ok(),
                None => digits.parse::<i64>().ok(),
            };
            let Some(n) = n else {
                // s2i_ASN1_INTEGER()
                x509v3_err(100, "bn dec2bn error");
                asn1_gen_err(180, "illegal integer");
                return None;
            };
            let mut d = asn1::int_i64(if neg { -n } else { n });
            if ty.starts_with("ENUM") {
                d[0] = 0x0a;
            }
            d
        }
        "OID" | "OBJECT" => match obj::txt2obj(value.trim()) {
            Some(o) if format == "ASCII" => asn1::oid(&o.oid),
            _ => {
                asn1_gen_err(183, "illegal object");
                return None;
            }
        },
        "UTCTIME" | "UTC" | "GENERALIZEDTIME" | "GENTIME" => {
            let t = if ty.starts_with('U') {
                asn1::UTC_TIME
            } else {
                asn1::GENERALIZED_TIME
            };
            if format != "ASCII" {
                asn1_gen_err(193, "time not ascii format");
                return None;
            }
            if crate::cert::asn1_time_to_unix(t, value.as_bytes()).is_none() {
                asn1_gen_err(184, "illegal time value");
                return None;
            }
            asn1::tlv(t, value.as_bytes())
        }
        "OCT" | "OCTETSTRING" => asn1::octet(&text(value)?),
        "BITSTR" | "BITSTRING" => {
            if format == "BITLIST" {
                let mut bits = Vec::new();
                for b in value.split(',') {
                    match b.trim().parse::<usize>() {
                        Ok(i) => bits.push(i),
                        Err(_) => {
                            asn1_gen_err(187, "invalid number");
                            return None;
                        }
                    }
                }
                encode_bits(&bits)
            } else {
                asn1::bitstr(&text(value)?)
            }
        }
        "SEQUENCE" | "SEQ" | "SET" => {
            let mut items = Vec::new();
            if !value.trim().is_empty() {
                let Some(sect) = conf.section(value.trim()) else {
                    return None;
                };
                for (_, v) in sect {
                    items.push(asn1_generate(conf, v, depth + 1)?);
                }
            }
            if ty == "SET" {
                asn1::set_of(&items)
            } else {
                asn1::seq(&items)
            }
        }
        t => match str_tag(t) {
            Some(tag) => {
                let raw = text(value)?;
                let content = match (tag, format) {
                    (0x1e, "ASCII" | "UTF8") => String::from_utf8_lossy(&raw)
                        .encode_utf16()
                        .flat_map(|u| u.to_be_bytes())
                        .collect(),
                    (0x1c, "ASCII" | "UTF8") => String::from_utf8_lossy(&raw)
                        .chars()
                        .flat_map(|c| (c as u32).to_be_bytes())
                        .collect(),
                    _ => raw,
                };
                asn1::tlv(tag, &content)
            }
            None => {
                // asn1_cb() and then ASN1_generate_v3() both report it
                asn1_gen_err(194, "unknown tag");
                asn1_gen_err(194, "unknown tag");
                return None;
            }
        },
    };
    if let Some(t) = implicit {
        der[0] = t | (der[0] & 0x20);
    }
    for op in outer.iter().rev() {
        der = match op {
            Ok(t) => asn1::tlv(*t | 0x20, &der),
            Err("OCTWRAP") => asn1::octet(&der),
            Err("SEQWRAP") => asn1::tlv(asn1::SEQUENCE, &der),
            Err("SETWRAP") => asn1::tlv(asn1::SET, &der),
            Err(_) => asn1::bitstr(&der),
        };
    }
    Some(der)
}

/// ossl_v3_name_cmp(): `name` is `cmp` or `cmp.<anything>`.
fn v3_name_is(name: &str, cmp: &str) -> bool {
    name == cmp
        || name
            .strip_prefix(cmp)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// policy_section(): one PolicyInformation from an @section.
fn policy_section(conf: &Conf, sect: &[(String, String)], ia5org: bool) -> Result<Vec<u8>, ()> {
    let mut pid = None;
    let mut quals = Vec::new();
    for (k, v) in sect {
        if k == "policyIdentifier" {
            match obj::txt2obj(v) {
                Some(o) => pid = Some(o.oid),
                None => {
                    x509v3_err(110, "invalid object identifier");
                    return Err(());
                }
            }
        } else if v3_name_is(k, "CPS") {
            quals.push(asn1::seq(&[asn1::oid("1.3.6.1.5.5.7.2.1"), ia5(v)]));
        } else if v3_name_is(k, "userNotice") {
            let Some(name) = v.strip_prefix('@') else {
                x509v3_err(137, "expected a section name");
                return Err(());
            };
            let Some(unot) = conf.section(name) else {
                x509v3_err(135, "invalid section");
                return Err(());
            };
            quals.push(notice_section(unot, ia5org)?);
        } else {
            x509v3_err(138, "invalid option");
            return Err(());
        }
    }
    let Some(pid) = pid else {
        // no NULL check: the absent policyIdentifier fails at i2d time
        crate::err::push(crate::err::LIB_ASN1, 222, "illegal zero content");
        x509v3_err(crate::err::RFLAG_COMMON | 13, "ASN1 lib");
        return Err(());
    };
    let mut p = vec![asn1::oid(&pid)];
    if !quals.is_empty() {
        p.push(asn1::seq(&quals));
    }
    Ok(asn1::seq(&p))
}

/// notice_section(): a UserNotice policy qualifier.
fn notice_section(unot: &[(String, String)], ia5org: bool) -> Result<Vec<u8>, ()> {
    let mut exptext = None;
    let mut org: Option<Vec<u8>> = None;
    let mut nos: Option<Vec<Vec<u8>>> = None;
    for (k, v) in unot {
        match k.as_str() {
            "explicitText" => {
                // displaytext_get_tag_len(): an optional string type prefix
                let (tag, text) = match v.split_once(':') {
                    Some(("UTF8" | "UTF8String", t)) => (asn1::UTF8_STRING, t),
                    Some(("BMP" | "BMPString", t)) => (0x1e, t),
                    Some(("VISIBLE" | "VisibleString", t)) => (0x1a, t),
                    _ => (0x1a, v.as_str()),
                };
                let content = if tag == 0x1e {
                    text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
                } else {
                    text.as_bytes().to_vec()
                };
                exptext = Some(asn1::tlv(tag, &content));
            }
            "organization" => {
                let tag = if ia5org { asn1::IA5_STRING } else { 0x1a };
                org = Some(asn1::tlv(tag, v.as_bytes()));
            }
            "noticeNumbers" => {
                let list = parse_list(v);
                if list.is_empty() {
                    x509v3_err(141, "invalid numbers");
                    return Err(());
                }
                let mut ints = nos.take().unwrap_or_default();
                for (n, _) in list {
                    match n.trim().parse::<i64>() {
                        Ok(i) => ints.push(asn1::int_i64(i)),
                        Err(_) => {
                            x509v3_err(141, "invalid numbers");
                            return Err(());
                        }
                    }
                }
                nos = Some(ints);
            }
            _ => {
                x509v3_err(138, "invalid option");
                return Err(());
            }
        }
    }
    let mut un = Vec::new();
    if org.is_some() || nos.is_some() {
        let tag = if ia5org { asn1::IA5_STRING } else { 0x1a };
        let o = org.unwrap_or_else(|| asn1::tlv(tag, b""));
        un.push(asn1::seq(&[o, asn1::seq(&nos.unwrap_or_default())]));
    }
    if let Some(t) = exptext {
        un.push(t);
    }
    Ok(asn1::seq(&[asn1::oid("1.3.6.1.5.5.7.2.2"), asn1::seq(&un)]))
}

fn ia5(v: &str) -> Vec<u8> {
    asn1::tlv(asn1::IA5_STRING, v.as_bytes())
}

/// Build one extension (X509V3_EXT_nconf). Ok(None) = skipped ("none").
pub fn make_ext(conf: &Conf, ctx: &V3Ctx, name: &str, value: &str) -> Result<Option<Ext>, ()> {
    let mut value = value.trim();
    let mut critical = false;
    if let Some(rest) = value.strip_prefix("critical,") {
        critical = true;
        value = rest.trim_start();
    }
    if let Some(rest) = value.strip_prefix("DER:") {
        let Some(o) = obj::txt2obj(name) else {
            x509v3_err(129, "extension name error");
            return Err(());
        };
        // OPENSSL_hexstr2buf()
        let digits = rest.chars().filter(|c| *c != ':').count();
        let Some(b) = hexstr2buf(rest) else {
            if rest.chars().any(|c| c != ':' && !c.is_ascii_hexdigit()) {
                crate::err::push(crate::err::LIB_CRYPTO, 102, "illegal hex digit");
            } else if digits % 2 == 1 {
                crate::err::push(crate::err::LIB_CRYPTO, 103, "odd number of digits");
            }
            x509v3_err(116, "extension value error");
            return Err(());
        };
        return Ok(Some(Ext {
            oid: o.oid,
            critical,
            value: b,
        }));
    }
    if let Some(rest) = value.strip_prefix("ASN1:") {
        // v3_generic_extension() with ASN1_generate_v3()
        let Some(o) = obj::txt2obj(name) else {
            x509v3_err(115, "extension name error");
            return Err(());
        };
        let Some(b) = asn1_generate(conf, rest, 0) else {
            x509v3_err(116, "extension value error");
            return Err(());
        };
        return Ok(Some(Ext {
            oid: o.oid,
            critical,
            value: b,
        }));
    }
    let Some(o) = obj::by_sn(name) else {
        x509v3_err(130, "unknown extension name");
        return Err(());
    };
    let list_or_section = |v: &str| -> Option<Vec<(String, Option<String>)>> {
        if let Some(s) = v.strip_prefix('@') {
            conf.section(s).map(|sect| {
                sect.iter()
                    .map(|(k, v)| (k.clone(), Some(v.clone())))
                    .collect()
            })
        } else {
            let l = parse_list(v);
            if l.is_empty() { None } else { Some(l) }
        }
    };
    let invalid = || x509v3_err(115, "invalid extension string");
    let der = match o.oid.as_str() {
        OID_BC => {
            let Some(list) = list_or_section(value) else {
                invalid();
                return Err(());
            };
            let mut ca = false;
            let mut pathlen: Option<i64> = None;
            for (n, v) in &list {
                match n.as_str() {
                    "CA" => match v.as_deref().and_then(get_bool) {
                        Some(b) => ca = b,
                        None => {
                            x509v3_err(104, "invalid boolean string");
                            return Err(());
                        }
                    },
                    "pathlen" => match v.as_deref().and_then(|s| s.parse::<i64>().ok()) {
                        Some(p) => pathlen = Some(p),
                        None => {
                            x509v3_err(110, "invalid number");
                            return Err(());
                        }
                    },
                    _ => {
                        x509v3_err(106, "invalid name");
                        return Err(());
                    }
                }
            }
            let mut items = Vec::new();
            if ca {
                items.push(asn1::boolean(true));
            }
            if let Some(p) = pathlen {
                items.push(asn1::int_i64(p));
            }
            asn1::seq(&items)
        }
        OID_KU | OID_NS_CERT_TYPE => {
            let table = if o.oid == OID_KU { KU_NAMES } else { NS_NAMES };
            let Some(list) = list_or_section(value) else {
                invalid();
                return Err(());
            };
            let mut bits = Vec::new();
            for (n, _) in &list {
                match table.iter().position(|(l, s)| n == s || n == l) {
                    Some(i) => bits.push(i),
                    None => {
                        x509v3_err(101, "unknown bit string argument");
                        return Err(());
                    }
                }
            }
            encode_bits(&bits)
        }
        OID_EKU => {
            let Some(list) = list_or_section(value) else {
                invalid();
                return Err(());
            };
            let mut oids = Vec::new();
            for (n, v) in &list {
                let t = v.as_deref().unwrap_or(n);
                match obj::txt2obj(t) {
                    Some(x) => oids.push(asn1::oid(&x.oid)),
                    None => {
                        x509v3_err(110, "invalid object identifier");
                        return Err(());
                    }
                }
            }
            asn1::seq(&oids)
        }
        OID_SKID => {
            if value == "none" {
                return Ok(None);
            }
            if value == "hash" {
                if ctx.test {
                    asn1::octet(&[])
                } else {
                    match &ctx.subject_pubkey_bits {
                        Some(b) => asn1::octet(&Md::Sha1.digest(b)),
                        None => {
                            x509v3_err(114, "no public key");
                            return Err(());
                        }
                    }
                }
            } else {
                match hexstr2buf(value) {
                    Some(b) => asn1::octet(&b),
                    None => {
                        invalid();
                        return Err(());
                    }
                }
            }
        }
        OID_AKID => {
            let list = parse_list(value);
            if list.len() == 1 && list[0].0 == "none" {
                return Ok(None);
            }
            let mut keyid = 0;
            let mut issuer = 0;
            for (n, v) in &list {
                if let Some(v) = v {
                    if v != "always" {
                        x509v3_err(164, "unknown option");
                        return Err(());
                    }
                }
                if n == "keyid" && keyid == 0 {
                    keyid = if v.is_some() { 2 } else { 1 };
                } else if n == "issuer" && issuer == 0 {
                    issuer = if v.is_some() { 2 } else { 1 };
                } else {
                    x509v3_err(110, "bad value");
                    return Err(());
                }
            }
            if ctx.test {
                asn1::seq(&[])
            } else {
                let ss = ctx.self_signed;
                let mut ikeyid: Option<Vec<u8>> = None;
                if ctx.issuer.is_none() && !ss {
                    x509v3_err(127, "no issuer certificate");
                    return Err(());
                }
                if keyid == 2 || (keyid == 1 && !ss) {
                    if ss {
                        ikeyid = ctx
                            .subject_exts
                            .iter()
                            .find(|e| e.oid == OID_SKID)
                            .and_then(|e| asn1::parse_one(&e.value).map(|t| t.content.to_vec()));
                    } else if let Some(iss) = ctx.issuer {
                        ikeyid = iss.skid();
                    }
                    if keyid == 2 && ikeyid.is_none() {
                        x509v3_err(119, "unable to get issuer keyid");
                        return Err(());
                    }
                }
                let mut items = Vec::new();
                if let Some(k) = &ikeyid {
                    items.push(asn1::implicit(0, k));
                }
                if issuer == 2 || (issuer == 1 && !ss && ikeyid.is_none()) {
                    let (iname, serial): (Name, Vec<u8>) = if ss {
                        (
                            ctx.self_issuer_name.cloned().unwrap_or_default(),
                            ctx.self_serial.map(|s| s.to_vec()).unwrap_or_default(),
                        )
                    } else {
                        let iss = ctx.issuer.unwrap();
                        (iss.issuer.clone(), iss.serial.clone())
                    };
                    let gn = asn1::explicit(4, &iname.to_der());
                    items.push(asn1::tlv(0xa1, &gn));
                    items.push(asn1::implicit(2, &serial));
                }
                asn1::seq(&items)
            }
        }
        OID_SAN | OID_IAN => {
            let Some(list) = list_or_section(value) else {
                invalid();
                return Err(());
            };
            match general_names(conf, ctx, &list, o.oid == OID_IAN) {
                Some(d) => d,
                None => return Err(()),
            }
        }
        OID_CRLDP => {
            let Some(list) = list_or_section(value) else {
                invalid();
                return Err(());
            };
            let mut dps = Vec::new();
            for (n, v) in &list {
                let mut gns = Vec::new();
                if !make_general_name(conf, ctx, n, v.as_deref().unwrap_or(""), &mut gns) {
                    return Err(());
                }
                let full = asn1::tlv(0xa0, &gns.concat());
                dps.push(asn1::seq(&[asn1::tlv(0xa0, &full)]));
            }
            asn1::seq(&dps)
        }
        OID_AIA => {
            let Some(list) = list_or_section(value) else {
                invalid();
                return Err(());
            };
            let mut ads = Vec::new();
            for (n, v) in &list {
                let Some((method, gtype)) = n.split_once(';') else {
                    x509v3_err(143, "invalid syntax");
                    return Err(());
                };
                let Some(m) = obj::txt2obj(method.trim()) else {
                    x509v3_err(119, "bad object");
                    return Err(());
                };
                let mut gns = Vec::new();
                if !make_general_name(
                    conf,
                    ctx,
                    gtype.trim(),
                    v.as_deref().unwrap_or(""),
                    &mut gns,
                ) {
                    return Err(());
                }
                ads.push(asn1::seq(&[asn1::oid(&m.oid), gns.concat()]));
            }
            asn1::seq(&ads)
        }
        OID_CERTPOL => {
            // r2i_certpol()
            let mut pols = Vec::new();
            let mut ia5org = false;
            for (n, v) in parse_list(value) {
                if v.is_some() {
                    x509v3_err(134, "invalid policy identifier");
                    return Err(());
                }
                if n == "ia5org" {
                    ia5org = true;
                    continue;
                }
                if let Some(sect) = n.strip_prefix('@') {
                    let Some(s) = conf.section(sect) else {
                        x509v3_err(135, "invalid section");
                        return Err(());
                    };
                    pols.push(policy_section(conf, s, ia5org)?);
                } else {
                    match obj::txt2obj(&n) {
                        Some(o) => pols.push(asn1::seq(&[asn1::oid(&o.oid)])),
                        None => {
                            x509v3_err(110, "invalid object identifier");
                            return Err(());
                        }
                    }
                }
            }
            asn1::seq(&pols)
        }
        "2.16.840.1.113730.1.13"
        | "2.16.840.1.113730.1.2"
        | "2.16.840.1.113730.1.3"
        | "2.16.840.1.113730.1.4"
        | "2.16.840.1.113730.1.7"
        | "2.16.840.1.113730.1.8"
        | "2.16.840.1.113730.1.12" => ia5(value),
        _ => {
            x509v3_err(129, "unknown extension");
            return Err(());
        }
    };
    Ok(Some(Ext {
        oid: o.oid,
        critical,
        value: der,
    }))
}

/// X509V3_EXT_add_nconf(): build all extensions of a section.
pub fn add_section(conf: &Conf, ctx: &mut V3Ctx, section: &str, out: &mut Vec<Ext>) -> bool {
    let Some(items) = conf.section(section).cloned() else {
        return false;
    };
    for (n, v) in items {
        let exts_now = out.clone();
        let c = V3Ctx {
            test: ctx.test,
            issuer: ctx.issuer,
            subject_pubkey_bits: ctx.subject_pubkey_bits.clone(),
            subject_name: ctx.subject_name,
            subject_exts: &exts_now,
            self_signed: ctx.self_signed,
            self_issuer_name: ctx.self_issuer_name,
            self_serial: ctx.self_serial,
        };
        match make_ext(conf, &c, &n, &v) {
            Ok(Some(e)) => out.push(e),
            Ok(None) => {}
            Err(()) => {
                // ERR_raise_data(..., "name=%s, value=%s"): the data is not
                // part of ERR_error_string_n()'s text. Generic DER:/ASN1:
                // extensions (v3_generic_extension()) add nothing.
                let v = v.trim();
                let v = v
                    .strip_prefix("critical,")
                    .map(str::trim_start)
                    .unwrap_or(v);
                if !v.starts_with("DER:") && !v.starts_with("ASN1:") {
                    x509v3_err(128, "error in extension");
                }
                return false;
            }
        }
    }
    true
}

/// php_openssl_config_check_syntax(): can the extension section be loaded?
pub fn check_section_syntax(conf: &Conf, section: &str) -> bool {
    let mut out = Vec::new();
    let mut ctx = V3Ctx {
        test: true,
        issuer: None,
        subject_pubkey_bits: None,
        subject_name: None,
        subject_exts: &[],
        self_signed: false,
        self_issuer_name: None,
        self_serial: None,
    };
    add_section(conf, &mut ctx, section, &mut out)
}

// ---------------------------------------------------------------------------
// Purposes (v3_purp.c)
// ---------------------------------------------------------------------------

const KU_DIGITAL_SIGNATURE: u32 = 0x0080;
const KU_NON_REPUDIATION: u32 = 0x0040;
const KU_KEY_ENCIPHERMENT: u32 = 0x0020;
const KU_KEY_AGREEMENT: u32 = 0x0008;
const KU_KEY_CERT_SIGN: u32 = 0x0004;
const KU_CRL_SIGN: u32 = 0x0002;

const NS_SSL_CLIENT: u32 = 0x80;
const NS_SSL_SERVER: u32 = 0x40;
const NS_SMIME: u32 = 0x20;
const NS_SSL_CA: u32 = 0x04;
const NS_SMIME_CA: u32 = 0x02;
const NS_ANY_CA: u32 = 0x07;

const XKU_SSL_SERVER: u32 = 0x1;
const XKU_SSL_CLIENT: u32 = 0x2;
const XKU_SMIME: u32 = 0x4;
const XKU_CODE_SIGN: u32 = 0x8;
const XKU_SGC: u32 = 0x10;
const XKU_OCSP_SIGN: u32 = 0x20;
const XKU_TIMESTAMP: u32 = 0x40;
const XKU_DVCS: u32 = 0x80;
const XKU_ANYEKU: u32 = 0x100;

#[derive(Default, Clone, Debug)]
pub struct ExFlags {
    pub v1: bool,
    pub bcons: bool,
    pub ca: bool,
    pub pathlen: Option<i64>,
    pub kusage: Option<u32>,
    pub xkusage: Option<u32>,
    pub nscert: Option<u32>,
    pub self_signed: bool,
    pub self_issued: bool,
    pub invalid: bool,
}

/// ossl_x509v3_cache_extensions()
pub fn ex_flags(c: &Cert) -> ExFlags {
    let mut f = ExFlags {
        v1: c.version == 0,
        ..Default::default()
    };
    for e in &c.extensions {
        match e.oid.as_str() {
            OID_BC => match asn1::parse_one(&e.value).and_then(|t| t.children()) {
                Some(ch) => {
                    f.bcons = true;
                    for x in ch {
                        if x.tag == asn1::BOOLEAN {
                            f.ca = x.content.first().copied().unwrap_or(0) != 0;
                        } else if x.tag == asn1::INTEGER {
                            f.pathlen = x.as_i64();
                        }
                    }
                }
                None => f.invalid = true,
            },
            OID_KU => match asn1::parse_one(&e.value).and_then(|t| t.bits().map(|b| b.to_vec())) {
                Some(b) => {
                    let v = b.first().copied().unwrap_or(0) as u32
                        | ((b.get(1).copied().unwrap_or(0) as u32) << 8);
                    f.kusage = Some(v);
                }
                None => f.invalid = true,
            },
            OID_EKU => match asn1::parse_one(&e.value).and_then(|t| t.children()) {
                Some(ch) => {
                    let mut x = 0;
                    for o in ch {
                        match o.oid().as_deref() {
                            Some("1.3.6.1.5.5.7.3.1") => x |= XKU_SSL_SERVER,
                            Some("1.3.6.1.5.5.7.3.2") => x |= XKU_SSL_CLIENT,
                            Some("1.3.6.1.5.5.7.3.4") => x |= XKU_SMIME,
                            Some("1.3.6.1.5.5.7.3.3") => x |= XKU_CODE_SIGN,
                            Some("1.3.6.1.4.1.311.10.3.3") | Some("2.16.840.1.113730.4.1") => {
                                x |= XKU_SGC
                            }
                            Some("1.3.6.1.5.5.7.3.9") => x |= XKU_OCSP_SIGN,
                            Some("1.3.6.1.5.5.7.3.8") => x |= XKU_TIMESTAMP,
                            Some("1.3.6.1.5.5.7.3.10") => x |= XKU_DVCS,
                            Some("2.5.29.37.0") => x |= XKU_ANYEKU,
                            _ => {}
                        }
                    }
                    f.xkusage = Some(x);
                }
                None => f.invalid = true,
            },
            OID_NS_CERT_TYPE => {
                match asn1::parse_one(&e.value).and_then(|t| t.bits().map(|b| b.to_vec())) {
                    Some(b) => f.nscert = Some(b.first().copied().unwrap_or(0) as u32),
                    None => f.invalid = true,
                }
            }
            _ => {}
        }
    }
    if c.subject.eq_canon(&c.issuer) {
        f.self_issued = true;
        // AKID must match SKID (if both present)
        let akid_ok = match (c.ext(OID_AKID), c.skid()) {
            (Some(a), Some(s)) => asn1::parse_one(&a.value)
                .and_then(|t| t.children())
                .map(|ch| {
                    ch.iter()
                        .find(|x| x.context() == Some(0))
                        .map(|k| k.content == s.as_slice())
                        .unwrap_or(true)
                })
                .unwrap_or(true),
            _ => true,
        };
        if akid_ok {
            f.self_signed = true;
        }
    }
    f
}

fn ku_reject(f: &ExFlags, usage: u32) -> bool {
    f.kusage.map(|k| k & usage == 0).unwrap_or(false)
}

fn xku_reject(f: &ExFlags, usage: u32) -> bool {
    f.xkusage.map(|k| k & usage == 0).unwrap_or(false)
}

fn ns_reject(f: &ExFlags, usage: u32) -> bool {
    f.nscert.map(|k| k & usage == 0).unwrap_or(false)
}

fn check_ca(f: &ExFlags) -> i32 {
    if ku_reject(f, KU_KEY_CERT_SIGN) {
        return 0;
    }
    if f.bcons {
        return if f.ca { 1 } else { 0 };
    }
    if f.v1 && f.self_signed {
        3
    } else if f.kusage.is_some() {
        4
    } else if f.nscert.map(|n| n & NS_ANY_CA != 0).unwrap_or(false) {
        5
    } else {
        0
    }
}

fn check_ssl_ca(f: &ExFlags) -> i32 {
    let r = check_ca(f);
    if r == 0 {
        return 0;
    }
    if r != 5 || f.nscert.unwrap_or(0) & NS_SSL_CA != 0 {
        r
    } else {
        0
    }
}

fn purpose_smime(f: &ExFlags, ca: bool) -> i32 {
    if xku_reject(f, XKU_SMIME) {
        return 0;
    }
    if ca {
        let r = check_ca(f);
        if r == 0 {
            return 0;
        }
        return if r != 5 || f.nscert.unwrap_or(0) & NS_SMIME_CA != 0 {
            r
        } else {
            0
        };
    }
    if let Some(ns) = f.nscert {
        if ns & NS_SMIME != 0 {
            return 1;
        }
        return if ns & NS_SSL_CLIENT != 0 { 2 } else { 0 };
    }
    1
}

/// X509_check_purpose(x, id, ca)
pub fn check_purpose(c: &Cert, id: i64, ca: bool) -> i32 {
    let f = ex_flags(c);
    if f.invalid {
        return -1;
    }
    let ktls = KU_DIGITAL_SIGNATURE | KU_KEY_ENCIPHERMENT | KU_KEY_AGREEMENT;
    match id {
        1 => {
            if xku_reject(&f, XKU_SSL_CLIENT) {
                return 0;
            }
            if ca {
                return check_ssl_ca(&f);
            }
            if ku_reject(&f, KU_DIGITAL_SIGNATURE | KU_KEY_AGREEMENT)
                || ns_reject(&f, NS_SSL_CLIENT)
            {
                return 0;
            }
            1
        }
        2 | 3 => {
            if xku_reject(&f, XKU_SSL_SERVER | XKU_SGC) {
                return 0;
            }
            if ca {
                return check_ssl_ca(&f);
            }
            if ns_reject(&f, NS_SSL_SERVER) || ku_reject(&f, ktls) {
                return 0;
            }
            if id == 3 && ku_reject(&f, KU_KEY_ENCIPHERMENT) {
                return 0;
            }
            1
        }
        4 => {
            let r = purpose_smime(&f, ca);
            if r == 0 || ca {
                return r;
            }
            if ku_reject(&f, KU_DIGITAL_SIGNATURE | KU_NON_REPUDIATION) {
                0
            } else {
                r
            }
        }
        5 => {
            let r = purpose_smime(&f, ca);
            if r == 0 || ca {
                return r;
            }
            if ku_reject(&f, KU_KEY_ENCIPHERMENT) {
                0
            } else {
                r
            }
        }
        6 => {
            if ca {
                let r = check_ca(&f);
                return if r == 2 { 0 } else { r };
            }
            if ku_reject(&f, KU_CRL_SIGN) { 0 } else { 1 }
        }
        7 => 1,
        8 => {
            if ca {
                check_ca(&f)
            } else {
                1
            }
        }
        9 => {
            if ca {
                return check_ca(&f);
            }
            if let Some(k) = f.kusage {
                let allowed = KU_NON_REPUDIATION | KU_DIGITAL_SIGNATURE;
                if k & !allowed != 0 || k & allowed == 0 {
                    return 0;
                }
            }
            if f.xkusage != Some(XKU_TIMESTAMP) {
                return 0;
            }
            if let Some(e) = c.ext(OID_EKU) {
                if !e.critical {
                    return 0;
                }
            }
            1
        }
        // check_purpose_code_sign() (OpenSSL 3.4+)
        10 => {
            if ca {
                return check_ca(&f);
            }
            let Some(k) = f.kusage else { return 0 };
            if k & KU_DIGITAL_SIGNATURE == 0 || k & (KU_KEY_CERT_SIGN | KU_CRL_SIGN) != 0 {
                return 0;
            }
            if !c.ext(OID_KU).is_some_and(|e| e.critical) {
                return 0;
            }
            let Some(x) = f.xkusage else { return 0 };
            if x & XKU_CODE_SIGN == 0 || x & (XKU_ANYEKU | XKU_SSL_SERVER) != 0 {
                return 0;
            }
            1
        }
        _ => -1,
    }
}

/// (id, short name, long name) of the X509 purposes.
pub const PURPOSES: &[(i64, &str, &str)] = &[
    (1, "sslclient", "SSL client"),
    (2, "sslserver", "SSL server"),
    (3, "nssslserver", "Netscape SSL server"),
    (4, "smimesign", "S/MIME signing"),
    (5, "smimeencrypt", "S/MIME encryption"),
    (6, "crlsign", "CRL signing"),
    (7, "any", "Any Purpose"),
    (8, "ocsphelper", "OCSP helper"),
    (9, "timestampsign", "Time Stamp signing"),
    (10, "codesign", "Code signing"),
];
