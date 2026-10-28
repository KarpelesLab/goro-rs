//! Netscape SignedPublicKeyAndChallenge (SPKAC; HTML5 keygen element,
//! WHATWG HTML 4.10.12 historical spec, and the OpenSSL spkac(1)
//! documentation): `SEQUENCE { SEQUENCE { spki, challenge IA5String },
//! signatureAlgorithm, signature BIT STRING }`, base64 without newlines.

use crate::der::{self, tag};
use crate::digest::Md;
use crate::key::Key;

pub struct Spkac {
    pub spki: Vec<u8>,
    pub challenge: Vec<u8>,
    pub pkac: Vec<u8>,
    pub sig_alg: String,
    pub sig_params: Option<Vec<u8>>,
    pub signature: Vec<u8>,
}

/// A new SPKAC string (`SPKAC=` prefix and base64).
pub fn new(key: &Key, challenge: &[u8], md: Md) -> Option<String> {
    let pkac = der::seq_vec(&[key.to_spki_der(), der::tlv(0x16, challenge)]);
    let (alg, sig) = crate::csr::sign_tbs(key, Some(md), &pkac)?;
    let d = der::seq_vec(&[pkac, alg, der::bit_string(&sig)]);
    Some(format!("SPKAC={}", crate::pem::base64_encode(&d)))
}

/// Parses base64 SPKAC text (an `SPKAC=` prefix tolerated).
pub fn parse(text: &[u8]) -> Option<Spkac> {
    let t = text.strip_prefix(b"SPKAC=").unwrap_or(text);
    let mut clean: Vec<u8> = t
        .iter()
        .copied()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    while clean.last() == Some(&0) {
        clean.pop();
    }
    let d = crate::pem::base64_decode(&clean)?;
    let top = der::parse(&d).ok()?;
    let mut r = top.reader();
    let pkac = r.expect(tag::SEQUENCE).ok()?;
    let alg = r.expect(tag::SEQUENCE).ok()?;
    let sig = r.next().ok()?.bit_string().ok()?;
    let (sig_alg, sig_params) = crate::x509::alg_id(&alg).ok()?;
    let mut pr = pkac.reader();
    let spki = pr.expect(tag::SEQUENCE).ok()?.raw.to_vec();
    let ch = pr.next().ok()?;
    Some(Spkac {
        spki,
        challenge: ch.string_bytes().ok()?,
        pkac: pkac.raw.to_vec(),
        sig_alg,
        sig_params,
        signature: sig,
    })
}

impl Spkac {
    pub fn public_key(&self) -> Option<Key> {
        Key::from_spki_der(&self.spki).ok()
    }
    pub fn verify(&self) -> bool {
        let Some(k) = self.public_key() else {
            return false;
        };
        crate::x509::verify_sig(
            &self.sig_alg,
            self.sig_params.as_deref(),
            &self.pkac,
            &self.signature,
            &k,
        ) == Some(true)
    }
}
