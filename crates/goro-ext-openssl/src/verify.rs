//! Certificate path validation (RFC 5280 §6, basic path validation:
//! names, signatures, validity, CA flags, path lengths) with the purpose
//! checks of openssl_x509_checkpurpose() and the S/MIME and TLS callers.

use crate::x509::Cert;

/// Verification errors, numbered as the openssl-verify documentation's
/// X509_V_ERR_* list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyError {
    UnableToGetIssuerCert = 2,
    CertSignatureFailure = 7,
    CertNotYetValid = 9,
    CertHasExpired = 10,
    DepthZeroSelfSigned = 18,
    SelfSignedInChain = 19,
    UnableToGetIssuerCertLocally = 20,
    UnableToVerifyLeafSignature = 21,
    InvalidCa = 24,
    PathLengthExceeded = 25,
    InvalidPurpose = 26,
}

impl VerifyError {
    /// The reason text (openssl-verify documentation).
    pub fn text(self) -> &'static str {
        match self {
            VerifyError::UnableToGetIssuerCert => "unable to get issuer certificate",
            VerifyError::CertSignatureFailure => "certificate signature failure",
            VerifyError::CertNotYetValid => "certificate is not yet valid",
            VerifyError::CertHasExpired => "certificate has expired",
            VerifyError::DepthZeroSelfSigned => "self-signed certificate",
            VerifyError::SelfSignedInChain => "self-signed certificate in certificate chain",
            VerifyError::UnableToGetIssuerCertLocally => "unable to get local issuer certificate",
            VerifyError::UnableToVerifyLeafSignature => "unable to verify the first certificate",
            VerifyError::InvalidCa => "invalid CA certificate",
            VerifyError::PathLengthExceeded => "path length constraint exceeded",
            VerifyError::InvalidPurpose => "unsupported certificate purpose",
        }
    }
}

fn issued_by(c: &Cert, issuer: &Cert) -> bool {
    if !c.issuer.matches(&issuer.subject) {
        return false;
    }
    if let (Some(aki), Some(ski)) = (c.authority_key_id(), issuer.subject_key_id())
        && aki != ski
    {
        return false;
    }
    true
}

impl Cert {
    pub fn verify_signature_by(&self, issuer: &Cert) -> bool {
        match issuer.public_key() {
            Ok(k) => self.verify_signature(&k) == Some(true),
            Err(_) => false,
        }
    }
}

/// Builds and checks a chain from `leaf` to a certificate of `trusted`.
pub fn verify_chain(
    leaf: &Cert,
    trusted: &[Cert],
    untrusted: &[Cert],
    purpose: Option<i64>,
    now: i64,
) -> Result<Vec<Cert>, VerifyError> {
    let mut chain = vec![leaf.clone()];
    let mut anchored = false;
    loop {
        let cur = chain.last().expect("non-empty").clone();
        if trusted.iter().any(|t| t.der == cur.der) {
            anchored = true;
            break;
        }
        if cur.is_self_issued() && cur.verify_signature_by(&cur) {
            break;
        }
        let candidate = trusted
            .iter()
            .find(|t| issued_by(&cur, t) && cur.verify_signature_by(t))
            .cloned();
        if let Some(t) = candidate {
            chain.push(t);
            anchored = true;
            break;
        }
        let next = untrusted
            .iter()
            .find(|u| issued_by(&cur, u) && !chain.iter().any(|c| c.der == u.der))
            .cloned();
        match next {
            Some(u) if chain.len() < 32 => chain.push(u),
            _ => break,
        }
    }
    let top = chain.last().expect("non-empty");
    if !anchored {
        return Err(if chain.len() == 1 && top.is_self_issued() {
            VerifyError::DepthZeroSelfSigned
        } else if top.is_self_issued() {
            VerifyError::SelfSignedInChain
        } else if chain.len() == 1 {
            VerifyError::UnableToVerifyLeafSignature
        } else {
            VerifyError::UnableToGetIssuerCertLocally
        });
    }
    for i in 0..chain.len() {
        if i + 1 < chain.len() {
            if !chain[i].verify_signature_by(&chain[i + 1]) {
                return Err(VerifyError::CertSignatureFailure);
            }
        }
    }
    for c in &chain {
        if now < c.not_before.unix {
            return Err(VerifyError::CertNotYetValid);
        }
        if now > c.not_after.unix {
            return Err(VerifyError::CertHasExpired);
        }
    }
    for (i, c) in chain.iter().enumerate() {
        if i > 0 {
            if c.ca_check() == 0 {
                return Err(VerifyError::InvalidCa);
            }
            if let Some((_, Some(pl))) = c.basic_constraints()
                && (i as i64 - 1) > pl
            {
                return Err(VerifyError::PathLengthExceeded);
            }
        }
        if let Some(p) = purpose
            && c.check_purpose(p, i > 0) <= 0
        {
            return Err(VerifyError::InvalidPurpose);
        }
    }
    Ok(chain)
}
