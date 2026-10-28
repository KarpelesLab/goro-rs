//! The certificate text dump of openssl_x509_export($c, $o, false)
//! (observed on PHP 8.5).

use crate::der;
use crate::key::Key;
use crate::oid;
use crate::x509::{
    Cert, Extension, GeneralName, extension_text, general_name_text, parse_general_names,
};

fn month(m: i64) -> &'static str {
    [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][(m.clamp(1, 12) - 1) as usize]
}

/// `Oct 28 14:31:49 2026 GMT`.
pub fn time_text(t: i64) -> String {
    let (y, mo, d, h, mi, s) = crate::time::civil_from_unix(t);
    format!(
        "{} {:2} {:02}:{:02}:{:02} {} GMT",
        month(mo),
        d,
        h,
        mi,
        s,
        y
    )
}

fn hex_block(b: &[u8], per_line: usize, indent: usize) -> String {
    let mut s = String::new();
    let chunks: Vec<&[u8]> = b.chunks(per_line).collect();
    for (i, c) in chunks.iter().enumerate() {
        s.push_str(&" ".repeat(indent));
        let line: Vec<String> = c.iter().map(|x| format!("{:02x}", x)).collect();
        s.push_str(&line.join(":"));
        if i + 1 < chunks.len() {
            s.push(':');
        }
        s.push('\n');
    }
    s
}

fn int_with_sign_byte(b: &[u8]) -> Vec<u8> {
    let s = crate::key::strip(b);
    if s[0] & 0x80 != 0 {
        let mut v = vec![0];
        v.extend_from_slice(&s);
        v
    } else {
        s
    }
}

fn public_key_text(spki: &[u8]) -> String {
    let mut s = String::new();
    let alg = der::parse(spki)
        .ok()
        .and_then(|t| t.reader().next().ok().and_then(|a| a.reader().next().ok()))
        .and_then(|o| o.oid().ok())
        .unwrap_or_default();
    s.push_str(&format!(
        "            Public Key Algorithm: {}\n",
        oid::ln(&alg)
    ));
    let Ok(k) = Key::from_spki_der(spki) else {
        return s;
    };
    use crate::key::KeyData;
    match &k.data {
        KeyData::Rsa { n, e, .. } => {
            s.push_str(&format!("                Public-Key: ({} bit)\n", k.bits()));
            s.push_str("                Modulus:\n");
            s.push_str(&hex_block(&int_with_sign_byte(n), 15, 20));
            let ev = e.iter().fold(0u128, |a, &b| (a << 8) | b as u128);
            s.push_str(&format!("                Exponent: {} (0x{:x})\n", ev, ev));
        }
        KeyData::Ec { curve, point, .. } => {
            s.push_str(&format!("                Public-Key: ({} bit)\n", k.bits()));
            s.push_str("                pub:\n");
            s.push_str(&hex_block(point, 15, 20));
            if let Some(o) = curve.oid {
                s.push_str(&format!("                ASN1 OID: {}\n", oid::sn(o)));
                let nist = match o {
                    "1.2.840.10045.3.1.7" => Some("P-256"),
                    "1.3.132.0.34" => Some("P-384"),
                    "1.3.132.0.35" => Some("P-521"),
                    "1.3.132.0.33" => Some("P-224"),
                    "1.2.840.10045.3.1.1" => Some("P-192"),
                    _ => None,
                };
                if let Some(n) = nist {
                    s.push_str(&format!("                NIST CURVE: {}\n", n));
                }
            }
        }
        _ => {
            s.push_str(&format!("                Public-Key: ({} bit)\n", k.bits()));
            s.push_str("                pub:\n");
            s.push_str(&hex_block(&k.public_bits(), 15, 20));
        }
    }
    s
}

fn ext_text_for_cert(e: &Extension) -> String {
    if (e.oid == "2.5.29.17" || e.oid == "2.5.29.18")
        && let Ok(t) = der::parse(&e.value)
        && let Ok(names) = parse_general_names(&t)
    {
        return names
            .iter()
            .map(|g| match g {
                GeneralName::Other(o, v) => {
                    let val = der::parse(v)
                        .ok()
                        .map(|i| String::from_utf8_lossy(i.content).into_owned())
                        .unwrap_or_default();
                    format!("othername: {}:{}", oid::ln(o), val)
                }
                other => general_name_text(other, false),
            })
            .collect::<Vec<_>>()
            .join(", ");
    }
    match extension_text(e) {
        Some(t) => t,
        None => e
            .value
            .iter()
            .map(|&b| {
                if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect(),
    }
}

/// The request text dump of openssl_csr_export($c, $o, false).
pub fn csr_text(c: &crate::csr::Csr) -> String {
    let mut s = String::from("Certificate Request:\n    Data:\n        Version: 1 (0x0)\n");
    s.push_str(&format!("        Subject: {}\n", c.subject.print_compat()));
    s.push_str("        Subject Public Key Info:\n");
    s.push_str(&public_key_text(&c.spki));
    s.push_str("        Attributes:\n");
    let exts = c.requested_extensions();
    let others: Vec<_> = c
        .attributes
        .iter()
        .filter(|(o, _)| o != crate::oid::ids::EXT_REQ)
        .collect();
    if others.is_empty() && exts.is_empty() {
        s.push_str("            (none)\n");
    }
    for (o, vals) in others {
        let v = vals
            .first()
            .and_then(|v| der::parse(v).ok())
            .map(|t| String::from_utf8_lossy(t.content).into_owned())
            .unwrap_or_default();
        s.push_str(&format!("            {:<24}:{}\n", oid::sn(o), v));
    }
    if !exts.is_empty() {
        s.push_str("        Requested Extensions:\n");
        for e in &exts {
            s.push_str(&format!(
                "            {}: {}\n",
                oid::ln(&e.oid),
                if e.critical { "critical" } else { "" }
            ));
            for line in ext_text_for_cert(e).split('\n') {
                s.push_str(&format!("                {}\n", line));
            }
        }
    }
    s.push_str(&format!(
        "    Signature Algorithm: {}\n",
        oid::ln(&c.sig_alg)
    ));
    s.push_str("    Signature Value:\n");
    s.push_str(&hex_block(&c.signature, 18, 8));
    s
}

/// The text dump before the PEM block.
pub fn cert_text(c: &Cert) -> String {
    let mut s = String::from("Certificate:\n    Data:\n");
    s.push_str(&format!(
        "        Version: {} (0x{:x})\n",
        c.version + 1,
        c.version
    ));
    let serial = crate::key::strip(&c.serial);
    if serial.len() <= 8 && c.serial.first().is_none_or(|b| b & 0x80 == 0) {
        let v = serial.iter().fold(0u64, |a, &b| (a << 8) | b as u64);
        s.push_str(&format!("        Serial Number: {} (0x{:x})\n", v, v));
    } else {
        s.push_str("        Serial Number:\n");
        let hex: Vec<String> = c.serial.iter().map(|x| format!("{:02x}", x)).collect();
        s.push_str(&format!("            {}\n", hex.join(":")));
    }
    s.push_str(&format!(
        "        Signature Algorithm: {}\n",
        oid::ln(&c.sig_alg)
    ));
    s.push_str(&format!("        Issuer: {}\n", c.issuer.print_compat()));
    s.push_str("        Validity\n");
    s.push_str(&format!(
        "            Not Before: {}\n",
        time_text(c.not_before.unix)
    ));
    s.push_str(&format!(
        "            Not After : {}\n",
        time_text(c.not_after.unix)
    ));
    s.push_str(&format!("        Subject: {}\n", c.subject.print_compat()));
    s.push_str("        Subject Public Key Info:\n");
    s.push_str(&public_key_text(&c.spki));
    if !c.extensions.is_empty() {
        s.push_str("        X509v3 extensions:\n");
        for e in &c.extensions {
            s.push_str(&format!(
                "            {}: {}\n",
                oid::ln(&e.oid),
                if e.critical { "critical" } else { "" }
            ));
            let t = ext_text_for_cert(e);
            for line in t.split('\n') {
                s.push_str(&format!("                {}\n", line));
            }
        }
    }
    s.push_str(&format!(
        "    Signature Algorithm: {}\n",
        oid::ln(&c.sig_alg)
    ));
    s.push_str("    Signature Value:\n");
    s.push_str(&hex_block(&c.signature, 18, 8));
    s
}
