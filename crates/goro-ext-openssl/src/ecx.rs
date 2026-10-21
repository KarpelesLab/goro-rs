//! X25519/X448 (RFC 7748) and Ed25519/Ed448 (RFC 8032) on big integers.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EcxKind {
    X25519,
    Ed25519,
    X448,
    Ed448,
}

impl EcxKind {
    pub fn key_len(self) -> usize {
        match self {
            EcxKind::X25519 | EcxKind::Ed25519 => 32,
            EcxKind::X448 => 56,
            EcxKind::Ed448 => 57,
        }
    }

    pub fn oid(self) -> &'static str {
        match self {
            EcxKind::X25519 => "1.3.101.110",
            EcxKind::X448 => "1.3.101.111",
            EcxKind::Ed25519 => "1.3.101.112",
            EcxKind::Ed448 => "1.3.101.113",
        }
    }

    pub fn from_oid(oid: &str) -> Option<EcxKind> {
        Some(match oid {
            "1.3.101.110" => EcxKind::X25519,
            "1.3.101.111" => EcxKind::X448,
            "1.3.101.112" => EcxKind::Ed25519,
            "1.3.101.113" => EcxKind::Ed448,
            _ => return None,
        })
    }

    /// EVP_PKEY_bits()
    pub fn bits(self) -> i64 {
        match self {
            EcxKind::X25519 => 253,
            EcxKind::Ed25519 => 256,
            EcxKind::X448 => 448,
            EcxKind::Ed448 => 456,
        }
    }

    pub fn is_signature(self) -> bool {
        matches!(self, EcxKind::Ed25519 | EcxKind::Ed448)
    }

    pub fn pub_from_priv(self, priv_key: &[u8]) -> Option<Vec<u8>> {
        if priv_key.len() != self.key_len() {
            return None;
        }
        Some(match self {
            EcxKind::X25519 => x25519(priv_key, &base_u(9, 32)),
            EcxKind::X448 => x448(priv_key, &base_u(5, 56)),
            EcxKind::Ed25519 => ed25519_public(priv_key),
            EcxKind::Ed448 => ed448_public(priv_key),
        })
    }
}

use purecrypto::ec::{
    Ed448PrivateKey, Ed448PublicKey, Ed448Signature, Ed25519PrivateKey, Ed25519PublicKey,
    Ed25519Signature,
};

fn base_u(v: u32, len: usize) -> Vec<u8> {
    let mut b = vec![0u8; len];
    b[0] = v as u8;
    b
}

/// X25519 scalar multiplication (RFC 7748).
pub fn x25519(k: &[u8], u: &[u8]) -> Vec<u8> {
    let (Ok(k), Ok(u)) = (<[u8; 32]>::try_from(k), <[u8; 32]>::try_from(u)) else {
        return Vec::new();
    };
    purecrypto::ec::x25519::x25519(&k, &u).to_vec()
}

/// X448 scalar multiplication (RFC 7748).
pub fn x448(k: &[u8], u: &[u8]) -> Vec<u8> {
    let (Ok(k), Ok(u)) = (<[u8; 56]>::try_from(k), <[u8; 56]>::try_from(u)) else {
        return Vec::new();
    };
    purecrypto::ec::x448::x448(&k, &u).to_vec()
}

pub fn ed25519_public(seed: &[u8]) -> Vec<u8> {
    let Ok(s) = <[u8; 32]>::try_from(seed) else {
        return Vec::new();
    };
    Ed25519PrivateKey::from_bytes(s)
        .public_key()
        .to_bytes()
        .to_vec()
}

pub fn ed448_public(seed: &[u8]) -> Vec<u8> {
    let Ok(s) = <[u8; 57]>::try_from(seed) else {
        return Vec::new();
    };
    Ed448PrivateKey::from_bytes(s)
        .public_key()
        .to_bytes()
        .to_vec()
}

/// Pure EdDSA signature (empty context).
pub fn sign(kind: EcxKind, seed: &[u8], msg: &[u8]) -> Option<Vec<u8>> {
    match kind {
        EcxKind::Ed25519 => {
            let k = Ed25519PrivateKey::from_bytes(seed.try_into().ok()?);
            Some(k.sign(msg).to_bytes().to_vec())
        }
        EcxKind::Ed448 => {
            let k = Ed448PrivateKey::from_bytes(seed.try_into().ok()?);
            Some(k.sign(msg).to_bytes().to_vec())
        }
        _ => None,
    }
}

pub fn verify(kind: EcxKind, pubk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    match kind {
        EcxKind::Ed25519 => {
            let (Ok(p), Ok(sg)) = (<[u8; 32]>::try_from(pubk), <[u8; 64]>::try_from(sig)) else {
                return false;
            };
            Ed25519PublicKey::from_bytes(p)
                .verify(msg, &Ed25519Signature::from_bytes(sg))
                .is_ok()
        }
        EcxKind::Ed448 => {
            let (Ok(p), Ok(sg)) = (<[u8; 57]>::try_from(pubk), <[u8; 114]>::try_from(sig)) else {
                return false;
            };
            Ed448PublicKey::from_bytes(p)
                .verify(msg, &Ed448Signature::from_bytes(sg))
                .is_ok()
        }
        _ => false,
    }
}

/// X25519/X448 key agreement.
pub fn derive(kind: EcxKind, priv_key: &[u8], peer: &[u8]) -> Option<Vec<u8>> {
    let out = match kind {
        EcxKind::X25519 if peer.len() == 32 => x25519(priv_key, peer),
        EcxKind::X448 if peer.len() == 56 => x448(priv_key, peer),
        _ => return None,
    };
    if out.is_empty() || out.iter().all(|&b| b == 0) {
        return None;
    }
    Some(out)
}
