//! PKCS #12 (RFC 7292): reading and writing PFX files with a key, its
//! certificate and extra certificates.
//!
//! Export uses PBES2 (PBKDF2-HMAC-SHA256, AES-256-CBC, 2048 iterations)
//! for the key and certificate bags and an HMAC-SHA256 MAC — the
//! defaults PHP 8.5 with OpenSSL 3 writes (observed by decoding its
//! output).

use crate::der::{self, tag};
use crate::digest::{HashAlgorithm, Md};
use crate::key::Key;
use crate::pbe::{self, CbcCipher};
use crate::x509::Cert;

const OID_KEY_BAG: &str = "1.2.840.113549.1.12.10.1.1";
const OID_SHROUDED_KEY_BAG: &str = "1.2.840.113549.1.12.10.1.2";
const OID_CERT_BAG: &str = "1.2.840.113549.1.12.10.1.3";
const OID_DATA: &str = "1.2.840.113549.1.7.1";
const OID_ENCRYPTED_DATA: &str = "1.2.840.113549.1.7.6";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P12Error {
    Decode,
    MacVerify,
    Decrypt,
}

#[derive(Default)]
pub struct Contents {
    pub key: Option<Key>,
    pub certs: Vec<(Cert, Option<Vec<u8>>, Option<String>)>,
}

fn mac_ok(md: Md, pass: &[u8], salt: &[u8], iter: u32, data: &[u8], expected: &[u8]) -> bool {
    let key = pbe::pkcs12_kdf(md, pass, salt, 3, iter, md.output_len());
    let mac = md.hmac(&key, data);
    use purecrypto::ct::ConstantTimeEq;
    bool::from(mac.as_slice().ct_eq(expected))
}

/// Parses a PFX with `pass`.
pub fn read(d: &[u8], pass: &[u8]) -> Result<Contents, P12Error> {
    let e = |_| P12Error::Decode;
    let t = der::parse(d).map_err(e)?;
    let mut r = t.reader();
    let _ver = r.expect(tag::INTEGER).map_err(e)?;
    let auth = r.expect(tag::SEQUENCE).map_err(e)?;
    let (ctype, content) = content_info(&auth)?;
    if ctype != OID_DATA {
        return Err(P12Error::Decode);
    }
    let data = content.ok_or(P12Error::Decode)?;
    if !r.is_empty() {
        let mac = r.expect(tag::SEQUENCE).map_err(e)?;
        let mut mr = mac.reader();
        let di = mr.expect(tag::SEQUENCE).map_err(e)?;
        let mut dr = di.reader();
        let alg = dr.expect(tag::SEQUENCE).map_err(e)?;
        let digest = dr.expect(tag::OCTET_STRING).map_err(e)?.content.to_vec();
        let md_oid = alg.reader().next().and_then(|x| x.oid()).map_err(e)?;
        let md = Md::by_oid(&md_oid).ok_or(P12Error::Decode)?;
        let salt = mr.expect(tag::OCTET_STRING).map_err(e)?.content.to_vec();
        let iter = if mr.is_empty() {
            1
        } else {
            mr.next().and_then(|x| x.small_int()).map_err(e)? as u32
        };
        // An empty password is tried both as the empty BMPString and as
        // no password at all (RFC 7292 leaves this to implementations).
        let ok = mac_ok(md, pass, &salt, iter, &data, &digest)
            || (pass.is_empty() && mac_ok_nopass(md, &salt, iter, &data, &digest));
        if !ok {
            return Err(P12Error::MacVerify);
        }
    }
    let safe = der::parse(&data).map_err(e)?;
    let mut out = Contents::default();
    let mut sr = safe.reader();
    while !sr.is_empty() {
        let ci = sr.expect(tag::SEQUENCE).map_err(e)?;
        let (ct, c) = content_info(&ci)?;
        let bags = match ct.as_str() {
            OID_DATA => c.ok_or(P12Error::Decode)?,
            OID_ENCRYPTED_DATA => {
                let ed_raw = c.ok_or(P12Error::Decode)?;
                decrypt_encrypted_data(&ed_raw, pass)?
            }
            _ => continue,
        };
        parse_bags(&bags, pass, &mut out)?;
    }
    Ok(out)
}

fn mac_ok_nopass(md: Md, salt: &[u8], iter: u32, data: &[u8], expected: &[u8]) -> bool {
    // The KDF over an absent password (no BMPString at all).
    let u = md.output_len();
    let v = md.block_len();
    let dv = vec![3u8; v];
    let len = v * salt.len().div_ceil(v);
    let s: Vec<u8> = (0..len).map(|i| salt[i % salt.len().max(1)]).collect();
    let mut a = md.digest(&[&dv[..], &s[..]].concat());
    for _ in 1..iter {
        a = md.digest(&a);
    }
    a.truncate(u);
    let mac = md.hmac(&a, data);
    mac == expected
}

/// (contentType, content of [0]: the OCTET STRING's bytes for data, the
/// raw element otherwise).
fn content_info(t: &der::Tlv) -> Result<(String, Option<Vec<u8>>), P12Error> {
    let e = |_| P12Error::Decode;
    let mut r = t.reader();
    let ct = r.next().and_then(|x| x.oid()).map_err(e)?;
    let content = match r.next_if(der::CONTEXT, 0).map_err(e)? {
        Some(c) => {
            let inner = der::parse_prefix(c.content).map_err(e)?.0;
            if ct == OID_DATA {
                Some(inner.string_bytes().map_err(e)?)
            } else {
                Some(inner.raw.to_vec())
            }
        }
        None => None,
    };
    Ok((ct, content))
}

fn decrypt_encrypted_data(raw: &[u8], pass: &[u8]) -> Result<Vec<u8>, P12Error> {
    let e = |_| P12Error::Decode;
    let t = der::parse(raw).map_err(e)?;
    let mut r = t.reader();
    let _v = r.expect(tag::INTEGER).map_err(e)?;
    let eci = r.expect(tag::SEQUENCE).map_err(e)?;
    let mut er = eci.reader();
    let _ct = er.next().map_err(e)?;
    let alg = er.expect(tag::SEQUENCE).map_err(e)?;
    let enc = er.next().map_err(e)?;
    let data = enc.string_bytes().map_err(e)?;
    pbe::decrypt(&alg, &data, pass).map_err(|_| P12Error::Decrypt)
}

fn parse_bags(d: &[u8], pass: &[u8], out: &mut Contents) -> Result<(), P12Error> {
    let e = |_| P12Error::Decode;
    let t = der::parse(d).map_err(e)?;
    let mut r = t.reader();
    while !r.is_empty() {
        let bag = r.expect(tag::SEQUENCE).map_err(e)?;
        let mut br = bag.reader();
        let id = br.next().and_then(|x| x.oid()).map_err(e)?;
        let val = br.next().map_err(e)?;
        let inner = der::parse_prefix(val.content).map_err(e)?.0;
        let mut local_id = None;
        let mut friendly = None;
        if !br.is_empty() {
            let attrs = br.expect(tag::SET).map_err(e)?;
            let mut ar = attrs.reader();
            while !ar.is_empty() {
                let at = ar.expect(tag::SEQUENCE).map_err(e)?;
                let mut atr = at.reader();
                let o = atr.next().and_then(|x| x.oid()).map_err(e)?;
                let vals = atr.expect(tag::SET).map_err(e)?;
                let v = vals.reader().next().map_err(e)?;
                match o.as_str() {
                    "1.2.840.113549.1.9.21" => local_id = Some(v.content.to_vec()),
                    "1.2.840.113549.1.9.20" => {
                        friendly = Some(
                            String::from_utf8_lossy(&crate::x509::to_utf8(v.num, v.content))
                                .into_owned(),
                        )
                    }
                    _ => {}
                }
            }
        }
        match id.as_str() {
            OID_KEY_BAG => {
                out.key = Some(Key::from_pkcs8_der(inner.raw).map_err(|_| P12Error::Decode)?);
            }
            OID_SHROUDED_KEY_BAG => {
                let pk =
                    crate::key::decrypt_pkcs8(inner.raw, pass).map_err(|_| P12Error::Decrypt)?;
                out.key = Some(Key::from_pkcs8_der(&pk).map_err(|_| P12Error::Decode)?);
            }
            OID_CERT_BAG => {
                let mut cr = inner.reader();
                let ctype = cr.next().and_then(|x| x.oid()).map_err(e)?;
                if ctype != crate::oid::ids::X509_CERT_TYPE {
                    continue;
                }
                let cv = cr.next().map_err(e)?;
                let octets = der::parse_prefix(cv.content).map_err(e)?.0;
                let cder = octets.string_bytes().map_err(e)?;
                let c = Cert::from_der(&cder).map_err(|_| P12Error::Decode)?;
                out.certs.push((c, local_id, friendly));
            }
            _ => {}
        }
    }
    Ok(())
}

impl Contents {
    /// The certificate matching the key (by localKeyId, else by public
    /// key), and the others.
    pub fn split(self) -> (Option<Key>, Option<Cert>, Vec<Cert>) {
        let key = self.key;
        let mut main = None;
        let mut rest = Vec::new();
        for (c, _, _) in self.certs {
            let matches = key
                .as_ref()
                .is_some_and(|k| c.public_key().is_ok_and(|pk| pk.public_eq(k)));
            if matches && main.is_none() {
                main = Some(c);
            } else {
                rest.push(c);
            }
        }
        (key, main, rest)
    }
}

fn attrs(local_id: &[u8], friendly: Option<&str>) -> Vec<u8> {
    let mut v = Vec::new();
    if let Some(f) = friendly {
        let bmp: Vec<u8> = f.encode_utf16().flat_map(|c| c.to_be_bytes()).collect();
        v.push(der::seq_vec(&[
            der::oid("1.2.840.113549.1.9.20"),
            der::set(&[der::tlv(0x1e, &bmp)]),
        ]));
    }
    v.push(der::seq_vec(&[
        der::oid("1.2.840.113549.1.9.21"),
        der::set(&[der::octet_string(local_id)]),
    ]));
    der::set(&v)
}

fn cert_bag(c: &Cert, attrs: Option<Vec<u8>>) -> Vec<u8> {
    let inner = der::seq_vec(&[
        der::oid(crate::oid::ids::X509_CERT_TYPE),
        der::explicit(0, &der::octet_string(&c.der)),
    ]);
    let mut parts = vec![der::oid(OID_CERT_BAG), der::explicit(0, &inner)];
    if let Some(a) = attrs {
        parts.push(a);
    }
    der::seq_vec(&parts)
}

/// Writes a PFX.
pub fn export(
    cert: &Cert,
    key: &Key,
    extra: &[Cert],
    pass: &[u8],
    friendly: Option<&str>,
) -> Option<Vec<u8>> {
    let local_id = Md::H(HashAlgorithm::Sha1).digest(&cert.der);
    // Certificates: an encryptedData SafeContents.
    let mut bags = vec![cert_bag(cert, Some(attrs(&local_id, friendly)))];
    for c in extra {
        bags.push(cert_bag(c, None));
    }
    let certs_plain = der::seq_vec(&bags);
    let enc = pbe::pbes2_encrypt(CbcCipher::Aes256, pass, &certs_plain)?;
    // pbes2_encrypt gives EncryptedPrivateKeyInfo {alg, octets}.
    let et = der::parse(&enc).ok()?;
    let mut er = et.reader();
    let alg = er.next().ok()?.raw.to_vec();
    let ct = er.next().ok()?.content.to_vec();
    let encrypted_data = der::seq_vec(&[
        der::int(0),
        der::seq_vec(&[der::oid(OID_DATA), alg, der::implicit_primitive(0, &ct)]),
    ]);
    let ci_certs = der::seq_vec(&[
        der::oid(OID_ENCRYPTED_DATA),
        der::explicit(0, &encrypted_data),
    ]);
    // Key: a shrouded key bag in a data SafeContents.
    let pk = key.to_pkcs8_der()?;
    let shrouded = pbe::pbes2_encrypt(CbcCipher::Aes256, pass, &pk)?;
    let key_bag = der::seq_vec(&[
        der::oid(OID_SHROUDED_KEY_BAG),
        der::explicit(0, &shrouded),
        attrs(&local_id, friendly),
    ]);
    let key_safe = der::seq_vec(&[key_bag]);
    let ci_key = der::seq_vec(&[
        der::oid(OID_DATA),
        der::explicit(0, &der::octet_string(&key_safe)),
    ]);
    let auth_safe = der::seq_vec(&[ci_certs, ci_key]);
    // MAC (RFC 7292 appendix B with id 3).
    let md = Md::H(HashAlgorithm::Sha256);
    let mut salt = [0u8; 8];
    crate::random_bytes(&mut salt);
    let iter = 2048u32;
    let mkey = pbe::pkcs12_kdf(md, pass, &salt, 3, iter, md.output_len());
    let mac = md.hmac(&mkey, &auth_safe);
    let mac_data = der::seq_vec(&[
        der::seq_vec(&[
            der::seq_vec(&[der::oid("2.16.840.1.101.3.4.2.1"), der::null()]),
            der::octet_string(&mac),
        ]),
        der::octet_string(&salt),
        der::int(iter as i64),
    ]);
    Some(der::seq_vec(&[
        der::int(3),
        der::seq_vec(&[
            der::oid(OID_DATA),
            der::explicit(0, &der::octet_string(&auth_safe)),
        ]),
        mac_data,
    ]))
}
