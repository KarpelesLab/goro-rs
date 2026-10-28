//! PKCS #7 (RFC 2315) and CMS (RFC 5652, RFC 5083) messages: SignedData
//! creation and verification, EnvelopedData with key transport (RSA
//! PKCS #1 v1.5, RFC 3370 §4.2.1), AuthEnvelopedData with AES-GCM
//! (RFC 5084), and the certificate lists of degenerate SignedData.
//!
//! The attribute set written with signatures (contentType, signingTime,
//! messageDigest, SMIMECapabilities with the AES-256/192/128-CBC,
//! DES-EDE3-CBC, RC2-128/64, DES-CBC, RC2-40 list) is the one PHP 8.5
//! produces (observed by decoding its output).

use crate::der::{self, Tlv, tag};
use crate::digest::{HashAlgorithm, Md};
use crate::key::{Key, KeyType};
use crate::oid::ids;
use crate::x509::{self, Cert, Name};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmsError {
    Decode,
    /// No signer / recipient certificate.
    NoSigner,
    /// A signature or digest didn't verify.
    BadSignature,
    /// A chain didn't verify.
    CertVerify(crate::verify::VerifyError),
    /// Decryption failed (wrong key, bad padding, tag mismatch).
    Decrypt,
    /// The cipher or algorithm is not available.
    Unsupported,
    /// Not the expected content type.
    WrongType,
}

pub type CResult<T> = Result<T, CmsError>;

fn d<T>(_: T) -> CmsError {
    CmsError::Decode
}

/// A parsed SignerInfo.
#[derive(Debug, Clone)]
pub struct SignerInfo {
    pub issuer: Option<Name>,
    pub serial: Option<Vec<u8>>,
    pub ski: Option<Vec<u8>>,
    pub digest_alg: String,
    /// The signed attributes' content (to be re-tagged as a SET).
    pub signed_attrs: Option<Vec<u8>>,
    pub sig_alg: String,
    pub sig_params: Option<Vec<u8>>,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct SignedData {
    pub content_type: String,
    pub content: Option<Vec<u8>>,
    pub certs: Vec<Cert>,
    pub signers: Vec<SignerInfo>,
}

/// The type and content of a ContentInfo.
pub fn content_info(dd: &[u8]) -> CResult<(String, Vec<u8>)> {
    let t = der::parse(dd).map_err(d)?;
    let mut r = t.reader();
    let o = r.next().and_then(|x| x.oid()).map_err(d)?;
    let c = r
        .next_if(der::CONTEXT, 0)
        .map_err(d)?
        .ok_or(CmsError::Decode)?;
    let inner = der::parse_prefix(c.content).map_err(d)?.0;
    Ok((o, der::to_der(&inner).map_err(d)?))
}

pub fn parse_signed(inner: &[u8]) -> CResult<SignedData> {
    let t = der::parse(inner).map_err(d)?;
    let mut r = t.reader();
    let _v = r.expect(tag::INTEGER).map_err(d)?;
    let _algs = r.expect(tag::SET).map_err(d)?;
    let eci = r.expect(tag::SEQUENCE).map_err(d)?;
    let mut er = eci.reader();
    let content_type = er.next().and_then(|x| x.oid()).map_err(d)?;
    let content = match er.next_if(der::CONTEXT, 0).map_err(d)? {
        Some(c) => {
            let inner = der::parse_prefix(c.content).map_err(d)?.0;
            if inner.is_universal(tag::OCTET_STRING) {
                Some(inner.string_bytes().map_err(d)?)
            } else {
                Some(der::to_der(&inner).map_err(d)?)
            }
        }
        None => None,
    };
    let mut certs = Vec::new();
    let mut signers = Vec::new();
    while !r.is_empty() {
        let x = r.next().map_err(d)?;
        if x.is_context(0) {
            let mut cr = x.reader();
            while !cr.is_empty() {
                let c = cr.next().map_err(d)?;
                if c.is_universal(tag::SEQUENCE)
                    && let Ok(cert) = Cert::from_der(&der::to_der(&c).map_err(d)?)
                {
                    certs.push(cert);
                }
            }
        } else if x.is_context(1) {
            // CRLs: ignored.
        } else if x.is_universal(tag::SET) {
            let mut sr = x.reader();
            while !sr.is_empty() {
                signers.push(parse_signer(&sr.next().map_err(d)?)?);
            }
        }
    }
    Ok(SignedData {
        content_type,
        content,
        certs,
        signers,
    })
}

fn parse_signer(t: &Tlv) -> CResult<SignerInfo> {
    let mut r = t.reader();
    let _v = r.expect(tag::INTEGER).map_err(d)?;
    let sid = r.next().map_err(d)?;
    let (issuer, serial, ski) = if sid.is_universal(tag::SEQUENCE) {
        let mut ir = sid.reader();
        let n = Name::parse(&ir.next().map_err(d)?).map_err(d)?;
        let s = ir
            .next()
            .and_then(|x| x.integer().map(|v| v.to_vec()))
            .map_err(d)?;
        (Some(n), Some(s), None)
    } else {
        (None, None, Some(sid.content.to_vec()))
    };
    let dalg = r.expect(tag::SEQUENCE).map_err(d)?;
    let digest_alg = dalg.reader().next().and_then(|x| x.oid()).map_err(d)?;
    let mut signed_attrs = None;
    let mut next = r.next().map_err(d)?;
    if next.is_context(0) {
        signed_attrs = Some(next.content.to_vec());
        next = r.next().map_err(d)?;
    }
    let (sig_alg, sig_params) = x509::alg_id(&next).map_err(d)?;
    let signature = r
        .expect(tag::OCTET_STRING)
        .map_err(d)?
        .string_bytes()
        .map_err(d)?;
    Ok(SignerInfo {
        issuer,
        serial,
        ski,
        digest_alg,
        signed_attrs,
        sig_alg,
        sig_params,
        signature,
    })
}

impl SignerInfo {
    pub fn matches(&self, c: &Cert) -> bool {
        if let (Some(i), Some(s)) = (&self.issuer, &self.serial) {
            return c.issuer.matches(i) && c.serial == *s;
        }
        if let Some(k) = &self.ski {
            return c.subject_key_id().as_deref() == Some(k.as_slice());
        }
        false
    }

    fn attr(&self, oid: &str) -> Option<Vec<u8>> {
        let a = self.signed_attrs.as_ref()?;
        let mut r = der::Reader::new(a);
        while !r.is_empty() {
            let at = r.next().ok()?;
            let mut ar = at.reader();
            let o = ar.next().ok()?.oid().ok()?;
            if o == oid {
                let set = ar.next().ok()?;
                return set.reader().next().ok().map(|v| v.raw.to_vec());
            }
        }
        None
    }

    /// Verifies the signature over `content` with `key`.
    pub fn verify(&self, content: &[u8], key: &Key) -> bool {
        let Some(md) = Md::by_oid(&self.digest_alg) else {
            return false;
        };
        let h = md.digest(content);
        let tbs = match &self.signed_attrs {
            Some(a) => {
                let Some(mdv) = self.attr(ids::MESSAGE_DIGEST) else {
                    return false;
                };
                let Ok(t) = der::parse(&mdv) else {
                    return false;
                };
                if t.content != h.as_slice() {
                    return false;
                }
                der::tlv(0x31, a)
            }
            None => {
                // No attributes: the signature covers the content.
                return self.verify_raw(content, md, key);
            }
        };
        self.verify_raw(&tbs, md, key)
    }

    fn verify_raw(&self, data: &[u8], md: Md, key: &Key) -> bool {
        // The signature algorithm may be the bare key algorithm
        // (rsaEncryption) or a combined one (sha256WithRSAEncryption).
        match self.sig_alg.as_str() {
            ids::RSA | ids::EC | ids::DSA => key
                .verify(Some(md), data, &self.signature, false)
                .unwrap_or(false),
            ids::ED25519 | ids::ED448 => key
                .verify(None, data, &self.signature, false)
                .unwrap_or(false),
            other => {
                x509::verify_sig(
                    other,
                    self.sig_params.as_deref(),
                    data,
                    &self.signature,
                    key,
                ) == Some(true)
            }
        }
    }
}

/// Options for [`sign`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SignOpts {
    pub detached: bool,
    pub no_attrs: bool,
    pub no_certs: bool,
    pub no_smime_cap: bool,
    /// CMS (digest AlgorithmIdentifiers without NULL parameters).
    pub cms: bool,
}

fn smime_caps() -> Vec<u8> {
    let rc2 = |bits: i64| der::seq_vec(&[der::oid("1.2.840.113549.3.2"), der::int(bits)]);
    der::seq_vec(&[
        der::seq_vec(&[der::oid("2.16.840.1.101.3.4.1.42")]),
        der::seq_vec(&[der::oid("2.16.840.1.101.3.4.1.22")]),
        der::seq_vec(&[der::oid("2.16.840.1.101.3.4.1.2")]),
        der::seq_vec(&[der::oid("1.2.840.113549.3.7")]),
        rc2(128),
        rc2(64),
        der::seq_vec(&[der::oid("1.3.14.3.2.7")]),
        rc2(40),
    ])
}

fn sig_alg_id_for(key: &Key, md: Md) -> Option<Vec<u8>> {
    Some(match key.key_type() {
        KeyType::Rsa => der::algorithm_id(ids::RSA, Some(&der::null())),
        KeyType::Ed25519 => der::algorithm_id(ids::ED25519, None),
        KeyType::Ed448 => der::algorithm_id(ids::ED448, None),
        _ => x509::sig_alg_for(key, Some(md))?,
    })
}

/// Creates a SignedData ContentInfo.
pub fn sign(
    content: &[u8],
    signer: &Cert,
    key: &Key,
    extra: &[Cert],
    md: Md,
    opts: SignOpts,
    now: i64,
) -> Option<Vec<u8>> {
    let md_oid = md.oid()?;
    let dalg = if opts.cms {
        der::algorithm_id(md_oid, None)
    } else {
        der::algorithm_id(md_oid, Some(&der::null()))
    };
    let h = md.digest(content);
    let eddsa = matches!(key.key_type(), KeyType::Ed25519 | KeyType::Ed448);
    let (attrs_field, signature) = if opts.no_attrs {
        let sig = key
            .sign(if eddsa { None } else { Some(md) }, content, false)
            .ok()?;
        (None, sig)
    } else {
        let mut attrs = vec![
            der::seq_vec(&[
                der::oid(ids::CONTENT_TYPE),
                der::set(&[der::oid(ids::DATA)]),
            ]),
            der::seq_vec(&[der::oid(ids::SIGNING_TIME), der::set(&[der::time(now)])]),
            der::seq_vec(&[
                der::oid(ids::MESSAGE_DIGEST),
                der::set(&[der::octet_string(&h)]),
            ]),
        ];
        if !opts.no_smime_cap {
            attrs.push(der::seq_vec(&[
                der::oid(ids::SMIME_CAPS),
                der::set(&[smime_caps()]),
            ]));
        }
        let set = der::set_of(&attrs);
        let sig = key
            .sign(if eddsa { None } else { Some(md) }, &set, false)
            .ok()?;
        let content = der::parse(&set).ok()?.content.to_vec();
        (Some(der::implicit_constructed(0, &content)), sig)
    };
    let mut si = vec![
        der::int(1),
        der::seq_vec(&[signer.issuer.der.clone(), der::int_raw(&signer.serial)]),
        dalg.clone(),
    ];
    if let Some(a) = attrs_field {
        si.push(a);
    }
    si.push(sig_alg_id_for(key, md)?);
    si.push(der::octet_string(&signature));
    let signer_info = der::seq_vec(&si);
    let mut eci = vec![der::oid(ids::DATA)];
    if !opts.detached {
        eci.push(der::explicit(0, &der::octet_string(content)));
    }
    let mut sd = vec![der::int(1), der::set(&[dalg]), der::seq_vec(&eci)];
    if !opts.no_certs {
        let mut cs = vec![signer.der.clone()];
        for c in extra {
            cs.push(c.der.clone());
        }
        sd.push(der::implicit_constructed(0, &cs.concat()));
    }
    sd.push(der::set(&[signer_info]));
    Some(der::seq_vec(&[
        der::oid(ids::SIGNED_DATA),
        der::explicit(0, &der::seq_vec(&sd)),
    ]))
}

/// A degenerate SignedData carrying only certificates (PKCS #7
/// "certs-only").
pub fn certs_only(certs: &[Cert]) -> Vec<u8> {
    let cs: Vec<u8> = certs.iter().flat_map(|c| c.der.clone()).collect();
    der::seq_vec(&[
        der::oid(ids::SIGNED_DATA),
        der::explicit(
            0,
            &der::seq_vec(&[
                der::int(1),
                der::set(&[]),
                der::seq_vec(&[der::oid(ids::DATA)]),
                der::implicit_constructed(0, &cs),
                der::set(&[]),
            ]),
        ),
    ])
}

/// Verification options.
#[derive(Debug, Clone, Copy, Default)]
pub struct VerifyOpts {
    pub no_verify: bool,
    pub no_intern: bool,
    pub no_sigs: bool,
    pub no_chain: bool,
}

/// Verifies a SignedData; returns the signer certificates.
pub fn verify(
    sd: &SignedData,
    detached: Option<&[u8]>,
    trusted: &[Cert],
    extra_certs: &[Cert],
    opts: VerifyOpts,
    now: i64,
) -> CResult<Vec<Cert>> {
    let content: &[u8] = match (&sd.content, detached) {
        (_, Some(c)) => c,
        (Some(c), None) => c,
        (None, None) => return Err(CmsError::Decode),
    };
    let mut pool: Vec<Cert> = extra_certs.to_vec();
    if !opts.no_intern {
        pool.extend(sd.certs.iter().cloned());
    }
    if sd.signers.is_empty() {
        return Err(CmsError::NoSigner);
    }
    let mut out = Vec::new();
    for s in &sd.signers {
        let Some(cert) = pool.iter().find(|c| s.matches(c)).cloned() else {
            return Err(CmsError::NoSigner);
        };
        if !opts.no_verify {
            let untrusted: Vec<Cert> = if opts.no_chain {
                Vec::new()
            } else {
                sd.certs.clone()
            };
            crate::verify::verify_chain(&cert, trusted, &untrusted, Some(4), now)
                .map_err(CmsError::CertVerify)?;
        }
        if !opts.no_sigs {
            let k = cert.public_key().map_err(|_| CmsError::BadSignature)?;
            if !s.verify(content, &k) {
                return Err(CmsError::BadSignature);
            }
        }
        out.push(cert);
    }
    Ok(out)
}

// ------------------------------------------------------------ enveloped data

/// Content encryption algorithms for EnvelopedData.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentCipher {
    Cbc(crate::pbe::CbcCipher),
    /// AES-GCM with this key length (AuthEnvelopedData).
    Gcm(usize),
}

impl ContentCipher {
    pub fn by_name(n: &str) -> Option<ContentCipher> {
        if let Some(c) = crate::pbe::CbcCipher::by_name(n) {
            return Some(ContentCipher::Cbc(c));
        }
        Some(match n.to_ascii_lowercase().as_str() {
            "aes-128-gcm" | "id-aes128-gcm" => ContentCipher::Gcm(16),
            "aes-192-gcm" | "id-aes192-gcm" => ContentCipher::Gcm(24),
            "aes-256-gcm" | "id-aes256-gcm" => ContentCipher::Gcm(32),
            _ => return None,
        })
    }
    fn key_len(self) -> usize {
        match self {
            ContentCipher::Cbc(c) => c.key_len(),
            ContentCipher::Gcm(n) => n,
        }
    }
    fn gcm_oid(n: usize) -> &'static str {
        match n {
            16 => "2.16.840.1.101.3.4.1.6",
            24 => "2.16.840.1.101.3.4.1.26",
            _ => "2.16.840.1.101.3.4.1.46",
        }
    }
}

fn recipient_infos(certs: &[Cert], cek: &[u8]) -> Option<Vec<u8>> {
    let mut ris = Vec::new();
    for c in certs {
        let k = c.public_key().ok()?;
        let ek = crate::key::rsa_public_encrypt(
            &k,
            cek,
            crate::key::PAD_PKCS1,
            Md::H(HashAlgorithm::Sha1),
        )
        .ok()?;
        ris.push(der::seq_vec(&[
            der::int(0),
            der::seq_vec(&[c.issuer.der.clone(), der::int_raw(&c.serial)]),
            der::algorithm_id(ids::RSA, Some(&der::null())),
            der::octet_string(&ek),
        ]));
    }
    Some(der::set(&ris))
}

/// EnvelopedData (or AuthEnvelopedData for GCM) for `certs`.
pub fn envelope(content: &[u8], certs: &[Cert], cipher: ContentCipher) -> Option<Vec<u8>> {
    let mut cek = vec![0u8; cipher.key_len()];
    crate::random_bytes(&mut cek);
    let ris = recipient_infos(certs, &cek)?;
    match cipher {
        ContentCipher::Cbc(c) => {
            let mut iv = vec![0u8; c.block()];
            crate::random_bytes(&mut iv);
            let ct = c.encrypt(&cek, &iv, content)?;
            let eci = der::seq_vec(&[
                der::oid(ids::DATA),
                der::algorithm_id(c.oid(), Some(&der::octet_string(&iv))),
                der::implicit_primitive(0, &ct),
            ]);
            Some(der::seq_vec(&[
                der::oid(ids::ENVELOPED_DATA),
                der::explicit(0, &der::seq_vec(&[der::int(0), ris, eci])),
            ]))
        }
        ContentCipher::Gcm(n) => {
            let mut nonce = [0u8; 12];
            crate::random_bytes(&mut nonce);
            let (ct, tag) = gcm_seal(&cek, &nonce, content)?;
            let params = der::seq_vec(&[der::octet_string(&nonce), der::int(16)]);
            let eci = der::seq_vec(&[
                der::oid(ids::DATA),
                der::algorithm_id(ContentCipher::gcm_oid(n), Some(&params)),
                der::implicit_primitive(0, &ct),
            ]);
            Some(der::seq_vec(&[
                der::oid(ids::AUTH_ENVELOPED_DATA),
                der::explicit(
                    0,
                    &der::seq_vec(&[der::int(0), ris, eci, der::octet_string(&tag)]),
                ),
            ]))
        }
    }
}

fn gcm_seal(key: &[u8], nonce: &[u8], data: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    use purecrypto::cipher::{Aes128, Aes192, Aes256, Gcm};
    let mut buf = data.to_vec();
    let tag = match key.len() {
        16 => Gcm::new(Aes128::new(key.try_into().ok()?))
            .try_encrypt(nonce, &[], &mut buf)
            .ok()?,
        24 => Gcm::new(Aes192::new(key.try_into().ok()?))
            .try_encrypt(nonce, &[], &mut buf)
            .ok()?,
        32 => Gcm::new(Aes256::new(key.try_into().ok()?))
            .try_encrypt(nonce, &[], &mut buf)
            .ok()?,
        _ => return None,
    };
    Some((buf, tag.to_vec()))
}

fn gcm_open(key: &[u8], nonce: &[u8], data: &[u8], tag: &[u8]) -> Option<Vec<u8>> {
    use purecrypto::cipher::{Aes128, Aes192, Aes256, Gcm};
    let mut buf = data.to_vec();
    let t: &[u8; 16] = tag.try_into().ok()?;
    let ok = match key.len() {
        16 => Gcm::new(Aes128::new(key.try_into().ok()?))
            .try_decrypt(nonce, &[], &mut buf, t)
            .is_ok(),
        24 => Gcm::new(Aes192::new(key.try_into().ok()?))
            .try_decrypt(nonce, &[], &mut buf, t)
            .is_ok(),
        32 => Gcm::new(Aes256::new(key.try_into().ok()?))
            .try_decrypt(nonce, &[], &mut buf, t)
            .is_ok(),
        _ => false,
    };
    if ok { Some(buf) } else { None }
}

/// Decrypts EnvelopedData / AuthEnvelopedData with `key` (the recipient
/// matching `cert` when given, else every key-transport recipient).
pub fn decrypt(ctype: &str, inner: &[u8], cert: Option<&Cert>, key: &Key) -> CResult<Vec<u8>> {
    let auth = match ctype {
        ids::ENVELOPED_DATA => false,
        ids::AUTH_ENVELOPED_DATA => true,
        _ => return Err(CmsError::WrongType),
    };
    let t = der::parse(inner).map_err(d)?;
    let mut r = t.reader();
    let _v = r.expect(tag::INTEGER).map_err(d)?;
    if r.next_if(der::CONTEXT, 0).map_err(d)?.is_some() {
        // originatorInfo
    }
    let ris = r.expect(tag::SET).map_err(d)?;
    let eci = r.expect(tag::SEQUENCE).map_err(d)?;
    let mac = if auth {
        let mut tagv = None;
        while !r.is_empty() {
            let x = r.next().map_err(d)?;
            if x.is_universal(tag::OCTET_STRING) {
                tagv = Some(x.content.to_vec());
            }
        }
        tagv
    } else {
        None
    };
    let mut er = eci.reader();
    let _ct = er.next().map_err(d)?;
    let alg = er.expect(tag::SEQUENCE).map_err(d)?;
    let enc = match er.next_if(der::CONTEXT, 0).map_err(d)? {
        Some(e) => e.string_bytes().map_err(d)?,
        None => return Err(CmsError::Decode),
    };
    let mut ar = alg.reader();
    let coid = ar.next().and_then(|x| x.oid()).map_err(d)?;
    let cparams = if ar.is_empty() {
        None
    } else {
        Some(ar.next().map_err(d)?)
    };
    // Recipient: key transport with RSA.
    let mut rr = ris.reader();
    let mut found = false;
    let mut last_err = CmsError::NoSigner;
    while !rr.is_empty() {
        let ri = rr.next().map_err(d)?;
        if !ri.is_universal(tag::SEQUENCE) {
            continue;
        }
        let mut rir = ri.reader();
        let _v = rir.expect(tag::INTEGER).map_err(d)?;
        let rid = rir.next().map_err(d)?;
        let matches = match cert {
            None => true,
            Some(c) => {
                if rid.is_universal(tag::SEQUENCE) {
                    let mut ir = rid.reader();
                    let n = Name::parse(&ir.next().map_err(d)?).map_err(d)?;
                    let s = ir
                        .next()
                        .and_then(|x| x.integer().map(|v| v.to_vec()))
                        .map_err(d)?;
                    c.issuer.matches(&n) && c.serial == s
                } else {
                    c.subject_key_id().as_deref() == Some(rid.content)
                }
            }
        };
        if !matches {
            continue;
        }
        found = true;
        let _kalg = rir.expect(tag::SEQUENCE).map_err(d)?;
        let ek = rir.expect(tag::OCTET_STRING).map_err(d)?.content.to_vec();
        let Some(sk) = key.rsa_private_key() else {
            return Err(CmsError::Decrypt);
        };
        let klen = match crate::pbe::CbcCipher::by_oid(&coid) {
            Some(c) => c.key_len(),
            None => match coid.as_str() {
                "2.16.840.1.101.3.4.1.6" => 16,
                "2.16.840.1.101.3.4.1.26" => 24,
                "2.16.840.1.101.3.4.1.46" => 32,
                _ => return Err(CmsError::Unsupported),
            },
        };
        // Implicit rejection with the known key length (RFC 3218 §2.3.2
        // mitigation): a wrong key yields a random CEK and then a
        // padding/tag failure, never a distinguishable RSA error.
        let cek = match sk.decrypt_pkcs1v15_session(&ek, klen) {
            Ok(c) => purecrypto::zeroize::Zeroizing::new(c),
            Err(_) => {
                last_err = CmsError::Decrypt;
                continue;
            }
        };
        let res = if let Some(c) = crate::pbe::CbcCipher::by_oid(&coid) {
            let iv = cparams
                .as_ref()
                .filter(|p| p.is_universal(tag::OCTET_STRING))
                .map(|p| p.content.to_vec())
                .ok_or(CmsError::Decode)?;
            c.decrypt(&cek, &iv, &enc)
        } else {
            let p = cparams.as_ref().ok_or(CmsError::Decode)?;
            let nonce = p.reader().next().map_err(d)?.content.to_vec();
            let tagv = mac.clone().ok_or(CmsError::Decode)?;
            gcm_open(&cek, &nonce, &enc, &tagv)
        };
        match res {
            Some(pt) => return Ok(pt),
            None => last_err = CmsError::Decrypt,
        }
    }
    if !found {
        return Err(CmsError::NoSigner);
    }
    Err(last_err)
}

// ------------------------------------------------------------ S/MIME

/// Canonical text (RFC 8551 §3.1.1): line ends as CRLF.
pub fn crlf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 32);
    let mut prev = 0u8;
    for &b in data {
        if b == b'\n' && prev != b'\r' {
            out.push(b'\r');
        }
        out.push(b);
        prev = b;
    }
    out
}

fn boundary() -> String {
    let mut b = [0u8; 16];
    crate::random_bytes(&mut b);
    b.iter().map(|x| format!("{:02X}", x)).collect()
}

/// The multipart/signed S/MIME form of a detached signature.
pub fn smime_signed_detached(content: &[u8], p7: &[u8], micalg: &str, old_mime: bool) -> Vec<u8> {
    let bnd = boundary();
    let sigtype = if old_mime {
        "application/x-pkcs7-signature"
    } else {
        "application/pkcs7-signature"
    };
    let mut s = Vec::new();
    s.extend_from_slice(b"MIME-Version: 1.0\n");
    s.extend_from_slice(
        format!(
            "Content-Type: multipart/signed; protocol=\"{}\"; micalg=\"{}\"; boundary=\"----{}\"\n\n",
            sigtype, micalg, bnd
        )
        .as_bytes(),
    );
    s.extend_from_slice(b"This is an S/MIME signed message\n\n");
    s.extend_from_slice(format!("------{}\n", bnd).as_bytes());
    s.extend_from_slice(content);
    s.extend_from_slice(format!("\n------{}\n", bnd).as_bytes());
    s.extend_from_slice(
        format!(
            "Content-Type: {}; name=\"smime.p7s\"\nContent-Transfer-Encoding: base64\nContent-Disposition: attachment; filename=\"smime.p7s\"\n\n",
            sigtype
        )
        .as_bytes(),
    );
    s.extend_from_slice(crate::pem::base64_lines(p7, 64).as_bytes());
    s.extend_from_slice(format!("\n------{}--\n\n", bnd).as_bytes());
    s
}

/// The application/pkcs7-mime form (`smime-type` signed-data or
/// enveloped-data).
pub fn smime_opaque(p7: &[u8], smime_type: &str, old_mime: bool) -> Vec<u8> {
    let ctype = if old_mime {
        "application/x-pkcs7-mime"
    } else {
        "application/pkcs7-mime"
    };
    let mut s = String::new();
    s.push_str("MIME-Version: 1.0\n");
    s.push_str("Content-Disposition: attachment; filename=\"smime.p7m\"\n");
    s.push_str(&format!(
        "Content-Type: {}; smime-type={}; name=\"smime.p7m\"\n",
        ctype, smime_type
    ));
    s.push_str("Content-Transfer-Encoding: base64\n\n");
    s.push_str(&crate::pem::base64_lines(p7, 64));
    s.push('\n');
    s.into_bytes()
}

fn find_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Splits a MIME entity into headers (unfolded) and body.
fn split_entity(data: &[u8]) -> (Vec<(String, String)>, &[u8]) {
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    loop {
        let rest = &data[i..];
        let le = rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len());
        let mut line = &rest[..le];
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        let next = (i + le + 1).min(data.len());
        if line.is_empty() {
            return (headers, &data[next..]);
        }
        if (line[0] == b' ' || line[0] == b'\t') && !headers.is_empty() {
            let l = headers.len() - 1;
            headers[l].1.push(' ');
            headers[l].1.push_str(String::from_utf8_lossy(line).trim());
        } else if let Some(c) = line.iter().position(|&x| x == b':') {
            headers.push((
                String::from_utf8_lossy(&line[..c]).trim().to_string(),
                String::from_utf8_lossy(&line[c + 1..]).trim().to_string(),
            ));
        }
        i = next;
        if i >= data.len() {
            return (headers, &data[data.len()..]);
        }
    }
}

fn param(v: &str, name: &str) -> Option<String> {
    for part in v.split(';').skip(1) {
        let (k, val) = part.split_once('=')?;
        if k.trim().eq_ignore_ascii_case(name) {
            return Some(val.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// A parsed S/MIME message: the PKCS #7 DER and, for multipart/signed,
/// the signed content (exactly as it appears, before canonicalisation).
pub struct Smime {
    pub p7: Vec<u8>,
    pub content: Option<Vec<u8>>,
}

pub fn smime_read(data: &[u8]) -> Option<Smime> {
    let (headers, body) = split_entity(data);
    let ctype = find_header(&headers, "Content-Type")
        .unwrap_or("")
        .to_string();
    let lower = ctype.to_ascii_lowercase();
    if lower.starts_with("multipart/signed") {
        let b = param(&ctype, "boundary")?;
        let delim = format!("--{}", b);
        let parts = split_multipart(body, delim.as_bytes())?;
        if parts.len() < 2 {
            return None;
        }
        let content = parts[0].to_vec();
        let (sh, sbody) = split_entity(parts[1]);
        let p7 = decode_body(&sh, sbody)?;
        return Some(Smime {
            p7,
            content: Some(content),
        });
    }
    if lower.contains("pkcs7-mime") || lower.contains("pkcs7-signature") {
        let p7 = decode_body(&headers, body)?;
        return Some(Smime { p7, content: None });
    }
    None
}

fn decode_body(headers: &[(String, String)], body: &[u8]) -> Option<Vec<u8>> {
    let enc = find_header(headers, "Content-Transfer-Encoding").unwrap_or("base64");
    if enc.eq_ignore_ascii_case("base64") {
        crate::pem::base64_decode(body)
    } else {
        Some(body.to_vec())
    }
}

/// The parts of a multipart body (without the CRLF before each
/// delimiter, RFC 2046 §5.1.1).
fn split_multipart<'a>(body: &'a [u8], delim: &[u8]) -> Option<Vec<&'a [u8]>> {
    let mut parts = Vec::new();
    let mut starts = Vec::new();
    let mut i = 0;
    while i + delim.len() <= body.len() {
        let at_line_start = i == 0 || body[i - 1] == b'\n';
        if at_line_start && &body[i..i + delim.len()] == delim {
            starts.push(i);
            i += delim.len();
        } else {
            i += 1;
        }
    }
    for w in starts.windows(2) {
        let (a, b) = (w[0], w[1]);
        // Skip the delimiter line itself.
        let mut s = a;
        while s < b && body[s] != b'\n' {
            s += 1;
        }
        s += 1;
        let mut e = b;
        // Remove the line break before the next delimiter.
        if e > s && body[e - 1] == b'\n' {
            e -= 1;
            if e > s && body[e - 1] == b'\r' {
                e -= 1;
            }
        }
        if s <= e {
            parts.push(&body[s..e]);
        }
    }
    Some(parts)
}

/// The micalg name of a digest (RFC 5751 §3.4.3.2).
pub fn micalg(md: Md) -> &'static str {
    match md.name() {
        "md5" => "md5",
        "sha1" => "sha1",
        "sha224" => "sha-224",
        "sha256" => "sha-256",
        "sha384" => "sha-384",
        "sha512" => "sha-512",
        _ => "unknown",
    }
}
