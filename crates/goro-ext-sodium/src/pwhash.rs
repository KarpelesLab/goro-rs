//! libsodium's crypto_pwhash (Argon2i / Argon2id v1.3, with the reference
//! PHC string encoding of argon2-encoding.c) and
//! crypto_pwhash_scryptsalsa208sha256 (escrypt's "$7$" strings), over
//! purecrypto's argon2() and scrypt().

use crate::codecs;
use purecrypto::kdf::argon2::{Argon2Params, Argon2Type, argon2};

pub const ALG_ARGON2I13: i64 = 1;
pub const ALG_ARGON2ID13: i64 = 2;
pub const ALG_DEFAULT: i64 = ALG_ARGON2ID13;
pub const SALTBYTES: usize = 16;
pub const STRBYTES: usize = 128;
pub const STRPREFIX: &[u8] = b"$argon2id$";
pub const BYTES_MIN: u64 = 16;
pub const BYTES_MAX: u64 = u32::MAX as u64;
pub const PASSWD_MAX: u64 = u32::MAX as u64;
pub const OPSLIMIT_MAX: u64 = u32::MAX as u64;
pub const MEMLIMIT_MIN: u64 = 8192;
pub const MEMLIMIT_MAX: u64 = 4398046510080;
pub const OPSLIMIT_INTERACTIVE: i64 = 2;
pub const MEMLIMIT_INTERACTIVE: i64 = 67108864;
pub const OPSLIMIT_MODERATE: i64 = 3;
pub const MEMLIMIT_MODERATE: i64 = 268435456;
pub const OPSLIMIT_SENSITIVE: i64 = 4;
pub const MEMLIMIT_SENSITIVE: i64 = 1073741824;
/// crypto_pwhash_argon2id_OPSLIMIT_MIN (the crypto_pwhash_* default)
pub const OPSLIMIT_MIN: i64 = 1;
const ARGON2I_OPSLIMIT_MIN: u64 = 3;
/// crypto_pwhash_STRBYTES - 1 bytes of salt for the encoded hashes, and the
/// raw hash length (crypto_pwhash_argon2id_STRHASHBYTES)
const STR_HASHBYTES: usize = 32;

#[derive(Clone, Copy, PartialEq)]
pub enum Variant {
    I,
    Id,
}

impl Variant {
    fn of_alg(alg: i64) -> Option<Variant> {
        match alg {
            ALG_ARGON2I13 => Some(Variant::I),
            ALG_ARGON2ID13 => Some(Variant::Id),
            _ => None,
        }
    }

    fn opslimit_min(self) -> u64 {
        match self {
            Variant::I => ARGON2I_OPSLIMIT_MIN,
            Variant::Id => 1,
        }
    }

    fn prefix(self) -> &'static [u8] {
        match self {
            Variant::I => b"$argon2i",
            Variant::Id => b"$argon2id",
        }
    }

    fn ty(self) -> Argon2Type {
        match self {
            Variant::I => Argon2Type::Argon2i,
            Variant::Id => Argon2Type::Argon2id,
        }
    }
}

/// validate_inputs() of argon2-core.c
fn valid_inputs(outlen: usize, saltlen: usize, t_cost: u32, m_cost: u32, lanes: u32) -> bool {
    outlen >= 16
        && saltlen >= 8
        && m_cost >= 8
        && (m_cost as u64) >= 8 * lanes as u64
        && t_cost >= 1
        && (1..=0xff_ffff).contains(&lanes)
}

/// argon2_hash()
fn argon2_raw(
    v: Variant,
    t_cost: u32,
    m_cost: u32,
    lanes: u32,
    pwd: &[u8],
    salt: &[u8],
    out: &mut [u8],
) -> Result<(), ()> {
    if !valid_inputs(out.len(), salt.len(), t_cost, m_cost, lanes) {
        return Err(());
    }
    let params = Argon2Params {
        t_cost,
        m_cost_kib: m_cost,
        parallelism: lanes,
        variant: v.ty(),
        version: 0x13,
    };
    argon2(&params, pwd, salt, &[], &[], out).map_err(|_| ())
}

fn check_limits(v: Variant, passwdlen: usize, opslimit: u64, memlimit: u64) -> Result<(), ()> {
    if passwdlen as u64 > PASSWD_MAX || opslimit > OPSLIMIT_MAX || memlimit > MEMLIMIT_MAX {
        return Err(());
    }
    if opslimit < v.opslimit_min() || memlimit < MEMLIMIT_MIN {
        return Err(());
    }
    Ok(())
}

/// crypto_pwhash() (crypto_pwhash_argon2i / crypto_pwhash_argon2id)
pub fn pwhash(
    outlen: u64,
    passwd: &[u8],
    salt: &[u8],
    opslimit: u64,
    memlimit: u64,
    alg: i64,
) -> Result<Vec<u8>, ()> {
    let v = Variant::of_alg(alg).ok_or(())?;
    if !(BYTES_MIN..=BYTES_MAX).contains(&outlen) {
        return Err(());
    }
    check_limits(v, passwd.len(), opslimit, memlimit)?;
    let mut out = vec![0u8; outlen as usize];
    argon2_raw(
        v,
        opslimit as u32,
        (memlimit / 1024) as u32,
        1,
        passwd,
        &salt[..SALTBYTES],
        &mut out,
    )?;
    Ok(out)
}

fn b64(b: &[u8]) -> Vec<u8> {
    codecs::bin2base64(b, 3)
}

/// crypto_pwhash_str_alg()
pub fn pwhash_str(passwd: &[u8], opslimit: u64, memlimit: u64, alg: i64) -> Result<Vec<u8>, ()> {
    let v = Variant::of_alg(alg).ok_or(())?;
    check_limits(v, passwd.len(), opslimit, memlimit)?;
    let mut salt = [0u8; SALTBYTES];
    crate::random_bytes(&mut salt);
    let t_cost = opslimit as u32;
    let m_cost = (memlimit / 1024) as u32;
    let mut hash = [0u8; STR_HASHBYTES];
    argon2_raw(v, t_cost, m_cost, 1, passwd, &salt, &mut hash)?;
    // encode_string()
    let mut out = v.prefix().to_vec();
    out.extend_from_slice(format!("$v=19$m={},t={},p=1$", m_cost, t_cost).as_bytes());
    out.extend_from_slice(&b64(&salt));
    out.push(b'$');
    out.extend_from_slice(&b64(&hash));
    if out.len() >= STRBYTES {
        return Err(());
    }
    Ok(out)
}

/// The fields of a PHC string decode_string() accepts.
struct Decoded {
    m_cost: u32,
    t_cost: u32,
    lanes: u32,
    salt: Vec<u8>,
    out: Vec<u8>,
}

/// decode_decimal()
fn decode_decimal(s: &[u8], pos: &mut usize) -> Option<u64> {
    let orig = *pos;
    let mut acc: u64 = 0;
    while *pos < s.len() && s[*pos].is_ascii_digit() {
        let c = (s[*pos] - b'0') as u64;
        acc = acc.checked_mul(10)?.checked_add(c)?;
        *pos += 1;
    }
    if *pos == orig || (s[orig] == b'0' && *pos != orig + 1) {
        return None;
    }
    Some(acc)
}

/// decode_string(): `maxlen` bounds the salt and hash (the buffers the
/// caller provides, as long as the string itself).
fn decode_string(s: &[u8], v: Variant, maxlen: usize) -> Option<Decoded> {
    let mut pos = 0usize;
    let cc = |pos: &mut usize, p: &[u8]| -> Option<()> {
        if s[*pos..].starts_with(p) {
            *pos += p.len();
            Some(())
        } else {
            None
        }
    };
    let dec_u32 = |pos: &mut usize| -> Option<u32> {
        let d = decode_decimal(s, pos)?;
        u32::try_from(d).ok()
    };
    let bin = |pos: &mut usize| -> Option<Vec<u8>> {
        let (ok, b, end) = codecs::base642bin(&s[*pos..], None, 3, maxlen);
        if !ok {
            return None;
        }
        *pos += end;
        Some(b)
    };
    cc(&mut pos, v.prefix())?;
    cc(&mut pos, b"$v=")?;
    let version = dec_u32(&mut pos)?;
    if version != 0x13 {
        return None;
    }
    cc(&mut pos, b"$m=")?;
    let m_cost = dec_u32(&mut pos)?;
    cc(&mut pos, b",t=")?;
    let t_cost = dec_u32(&mut pos)?;
    cc(&mut pos, b",p=")?;
    let lanes = dec_u32(&mut pos)?;
    cc(&mut pos, b"$")?;
    let salt = bin(&mut pos)?;
    cc(&mut pos, b"$")?;
    let out = bin(&mut pos)?;
    if !valid_inputs(out.len(), salt.len(), t_cost, m_cost, lanes) || pos != s.len() {
        return None;
    }
    Some(Decoded {
        m_cost,
        t_cost,
        lanes,
        salt,
        out,
    })
}

/// The C string view of a PHP string.
fn cstr(s: &[u8]) -> &[u8] {
    match s.iter().position(|&c| c == 0) {
        Some(p) => &s[..p],
        None => s,
    }
}

/// crypto_pwhash_str_verify()
pub fn str_verify(s: &[u8], passwd: &[u8]) -> bool {
    let s = cstr(s);
    let v = if s.starts_with(b"$argon2id$") {
        Variant::Id
    } else if s.starts_with(b"$argon2i$") {
        Variant::I
    } else {
        return false;
    };
    if passwd.len() as u64 > PASSWD_MAX {
        return false;
    }
    // argon2_verify()
    let Some(d) = decode_string(s, v, s.len()) else {
        return false;
    };
    let mut out = vec![0u8; d.out.len()];
    if argon2_raw(v, d.t_cost, d.m_cost, d.lanes, passwd, &d.salt, &mut out).is_err() {
        return false;
    }
    codecs::memcmp(&out, &d.out) == 0
}

/// crypto_pwhash_str_needs_rehash(): 1, 0, or -1 for an invalid string.
pub fn str_needs_rehash(s: &[u8], opslimit: u64, memlimit: u64) -> i32 {
    let s = cstr(s);
    let v = if s.starts_with(b"$argon2id$") {
        Variant::Id
    } else if s.starts_with(b"$argon2i$") {
        Variant::I
    } else {
        return -1;
    };
    if s.len() >= STRBYTES {
        return -1;
    }
    let Some(d) = decode_string(s, v, s.len()) else {
        return -1;
    };
    if d.t_cost != opslimit as u32 || d.m_cost != (memlimit / 1024) as u32 {
        return 1;
    }
    0
}

// ---------------------------------------------------------------------------
// scrypt (crypto_pwhash_scryptsalsa208sha256)

pub mod scrypt {
    use super::cstr;
    use crate::codecs;

    pub const SALTBYTES: usize = 32;
    pub const STRBYTES: usize = 102;
    pub const STRPREFIX: &[u8] = b"$7$";
    pub const OPSLIMIT_INTERACTIVE: i64 = 524288;
    pub const MEMLIMIT_INTERACTIVE: i64 = 16777216;
    pub const OPSLIMIT_SENSITIVE: i64 = 33554432;
    pub const MEMLIMIT_SENSITIVE: i64 = 1073741824;
    const BYTES_MIN: u64 = 16;
    const BYTES_MAX: u64 = 0x1f_ffff_ffe0;
    const STRSALTBYTES: usize = 32;
    const STRHASHBYTES: usize = 32;

    const ITOA64: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

    /// pickparams(): (N_log2, p, r)
    fn pickparams(opslimit: u64, memlimit: u64) -> (u32, u32, u32) {
        let opslimit = opslimit.max(32768);
        let r: u32 = 8;
        let mut n_log2: u32 = 1;
        let p: u32;
        if opslimit < memlimit / 32 {
            p = 1;
            let max_n = opslimit / (r as u64 * 4);
            while n_log2 < 63 {
                if (1u64 << n_log2) > max_n / 2 {
                    break;
                }
                n_log2 += 1;
            }
        } else {
            let max_n = memlimit / (r as u64 * 128);
            while n_log2 < 63 {
                if (1u64 << n_log2) > max_n / 2 {
                    break;
                }
                n_log2 += 1;
            }
            let mut maxrp = (opslimit / 4) / (1u64 << n_log2);
            if maxrp > 0x3fff_ffff {
                maxrp = 0x3fff_ffff;
            }
            p = (maxrp as u32) / r;
        }
        (n_log2, p, r)
    }

    /// escrypt_kdf() / crypto_pwhash_scryptsalsa208sha256_ll()
    fn kdf(
        passwd: &[u8],
        salt: &[u8],
        n_log2: u32,
        r: u32,
        p: u32,
        out: &mut [u8],
    ) -> Result<(), ()> {
        if n_log2 == 0 || n_log2 > 31 || r == 0 || p == 0 || (r as u64) * (p as u64) >= 1 << 30 {
            return Err(());
        }
        purecrypto::kdf::scrypt::scrypt(passwd, salt, n_log2 as u8, r, p, out).map_err(|_| ())
    }

    /// crypto_pwhash_scryptsalsa208sha256()
    pub fn pwhash(
        outlen: u64,
        passwd: &[u8],
        salt: &[u8],
        opslimit: u64,
        memlimit: u64,
    ) -> Result<Vec<u8>, ()> {
        if !(BYTES_MIN..=BYTES_MAX).contains(&outlen) {
            return Err(());
        }
        let (n_log2, p, r) = pickparams(opslimit, memlimit);
        let mut out = vec![0u8; outlen as usize];
        kdf(passwd, &salt[..SALTBYTES], n_log2, r, p, &mut out)?;
        Ok(out)
    }

    fn encode64_uint32(dst: &mut Vec<u8>, mut src: u32, srcbits: u32) {
        let mut bit = 0;
        while bit < srcbits {
            dst.push(ITOA64[(src & 0x3f) as usize]);
            src >>= 6;
            bit += 6;
        }
    }

    fn encode64(dst: &mut Vec<u8>, src: &[u8]) {
        let mut i = 0;
        while i < src.len() {
            let mut value: u32 = 0;
            let mut bits = 0;
            loop {
                value |= (src[i] as u32) << bits;
                i += 1;
                bits += 8;
                if bits >= 24 || i >= src.len() {
                    break;
                }
            }
            encode64_uint32(dst, value, bits);
        }
    }

    fn decode64_one(c: u8) -> Option<u32> {
        ITOA64.iter().position(|&x| x == c).map(|p| p as u32)
    }

    fn decode64_uint32(s: &[u8], pos: &mut usize, dstbits: u32) -> Option<u32> {
        let mut value = 0u32;
        let mut bit = 0;
        while bit < dstbits {
            let one = decode64_one(*s.get(*pos)?)?;
            *pos += 1;
            value |= one << bit;
            bit += 6;
        }
        Some(value)
    }

    /// escrypt_r(): the "$7$" string for `setting` (whose salt is used as
    /// text, up to the last '$').
    fn escrypt_r(passwd: &[u8], setting: &[u8]) -> Option<Vec<u8>> {
        if !setting.starts_with(b"$7$") {
            return None;
        }
        let mut pos = 3;
        let n_log2 = decode64_one(*setting.get(pos)?)?;
        pos += 1;
        let r = decode64_uint32(setting, &mut pos, 30)?;
        let p = decode64_uint32(setting, &mut pos, 30)?;
        let prefixlen = pos;
        let rest = &setting[prefixlen..];
        let saltlen = match rest.iter().rposition(|&c| c == b'$') {
            Some(i) => i,
            None => rest.len(),
        };
        let salt = &rest[..saltlen];
        // need = prefixlen + saltlen + 1 + HASH_ENCODED + 1 <= STRBYTES
        if prefixlen + saltlen + 1 + 43 + 1 > STRBYTES {
            return None;
        }
        let mut hash = [0u8; STRHASHBYTES];
        kdf(passwd, salt, n_log2, r, p, &mut hash).ok()?;
        let mut out = setting[..prefixlen + saltlen].to_vec();
        out.push(b'$');
        encode64(&mut out, &hash);
        if out.len() >= STRBYTES {
            return None;
        }
        Some(out)
    }

    /// crypto_pwhash_scryptsalsa208sha256_str()
    pub fn pwhash_str(passwd: &[u8], opslimit: u64, memlimit: u64) -> Result<Vec<u8>, ()> {
        let (n_log2, p, r) = pickparams(opslimit, memlimit);
        let mut salt = [0u8; STRSALTBYTES];
        crate::random_bytes(&mut salt);
        // escrypt_gensalt_r()
        let mut setting = b"$7$".to_vec();
        setting.push(ITOA64[n_log2 as usize]);
        encode64_uint32(&mut setting, r, 30);
        encode64_uint32(&mut setting, p, 30);
        encode64(&mut setting, &salt);
        escrypt_r(passwd, &setting).ok_or(())
    }

    /// crypto_pwhash_scryptsalsa208sha256_str_verify()
    pub fn str_verify(s: &[u8], passwd: &[u8]) -> bool {
        let s = cstr(s);
        if s.len() != STRBYTES - 1 {
            return false;
        }
        match escrypt_r(passwd, s) {
            Some(wanted) => {
                let mut a = [0u8; STRBYTES];
                let mut b = [0u8; STRBYTES];
                a[..wanted.len()].copy_from_slice(&wanted);
                b[..s.len()].copy_from_slice(s);
                codecs::memcmp(&a, &b) == 0
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2i_known_hash() {
        let h = b"$argon2i$v=19$m=4096,t=3,p=1$MzE4ODFiZWFlMjAzOWUAAA$FWUV6tsyJ32qThiLi1cCsLIbf3dIOG/RwXcTzt536KY";
        assert!(str_verify(h, b"password"));
        assert!(!str_verify(h, b"passwd"));
        let bad = b"$argon2i$v=19$m=4096,t=0,p=1$c29tZXNhbHQAAAAAAAAAAA$JTBozgKQiCn5yKAm3Hz0vUSX/XgfqhZloNCxDWmeDr0";
        assert!(!str_verify(bad, b"password"));
        assert_eq!(str_needs_rehash(h, 3, 4096 * 1024), 0);
        assert_eq!(str_needs_rehash(h, 4, 4096 * 1024), 1);
    }

    #[test]
    fn str_roundtrip() {
        let s = pwhash_str(b"pw", 2, 8192 * 1024, ALG_ARGON2ID13).unwrap();
        assert!(s.starts_with(b"$argon2id$v=19$m=8192,t=2,p=1$"));
        assert!(str_verify(&s, b"pw"));
        let s = scrypt::pwhash_str(b"pw", 32768, 1 << 20).unwrap();
        assert_eq!(s.len(), scrypt::STRBYTES - 1);
        assert!(scrypt::str_verify(&s, b"pw"));
        assert!(!scrypt::str_verify(&s, b"px"));
    }
}
