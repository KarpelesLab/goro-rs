//! EVP symmetric ciphers: name table (default + legacy providers, as php-src
//! 8.5 loads both) and one-shot encrypt/decrypt implementing OpenSSL's mode,
//! padding and AEAD semantics.

use crate::err;
use cipher::generic_array::GenericArray;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algo {
    Null,
    Aes,
    Aria,
    Camellia,
    Sm4,
    Seed,
    Des,
    DesEde,
    DesEde3,
    DesX,
    Bf,
    Cast5,
    Idea,
    /// RC2 with the given effective key bits.
    Rc2(usize),
    Rc4,
    ChaCha20,
    ChaCha20Poly1305,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Stream,
    Ecb,
    Cbc,
    Cfb,
    Cfb1,
    Cfb8,
    Ofb,
    Ctr,
    Gcm,
    Ccm,
    Ocb,
    Xts,
    Wrap,
    WrapPad,
}

#[derive(Clone, Debug)]
pub struct CipherSpec {
    /// Canonical (lower-case) name as listed by openssl_get_cipher_methods().
    pub name: String,
    /// Other accepted names (lower-case).
    pub aliases: Vec<String>,
    pub algo: Algo,
    pub mode: Mode,
    pub key_len: usize,
    pub iv_len: usize,
    /// EVP_CIPHER_get_block_size()
    #[allow(dead_code)]
    pub block_size: usize,
    pub var_key: bool,
    pub oid: Option<&'static str>,
}

impl CipherSpec {
    pub fn is_aead(&self) -> bool {
        matches!(self.mode, Mode::Gcm | Mode::Ccm | Mode::Ocb)
            || self.algo == Algo::ChaCha20Poly1305
    }

    /// `EVP_CIPHER_get_nid`-style OID for ASN.1 parameters (PKCS#7/CMS).
    pub fn oid(&self) -> Option<&'static str> {
        self.oid
    }
}

fn spec(
    name: &str,
    aliases: &[&str],
    algo: Algo,
    mode: Mode,
    key_len: usize,
    iv_len: usize,
    block_size: usize,
    var_key: bool,
    oid: Option<&'static str>,
) -> CipherSpec {
    CipherSpec {
        name: name.to_string(),
        aliases: aliases.iter().map(|s| s.to_ascii_lowercase()).collect(),
        algo,
        mode,
        key_len,
        iv_len,
        block_size,
        var_key,
        oid,
    }
}

fn build_table() -> Vec<CipherSpec> {
    let mut t = Vec::new();
    // AES
    let aes_oids: [(usize, [&'static str; 8]); 3] = [
        (
            128,
            [
                "2.16.840.1.101.3.4.1.1",
                "2.16.840.1.101.3.4.1.2",
                "2.16.840.1.101.3.4.1.3",
                "2.16.840.1.101.3.4.1.4",
                "2.16.840.1.101.3.4.1.6",
                "2.16.840.1.101.3.4.1.7",
                "2.16.840.1.101.3.4.1.5",
                "2.16.840.1.101.3.4.1.8",
            ],
        ),
        (
            192,
            [
                "2.16.840.1.101.3.4.1.21",
                "2.16.840.1.101.3.4.1.22",
                "2.16.840.1.101.3.4.1.23",
                "2.16.840.1.101.3.4.1.24",
                "2.16.840.1.101.3.4.1.26",
                "2.16.840.1.101.3.4.1.27",
                "2.16.840.1.101.3.4.1.25",
                "2.16.840.1.101.3.4.1.28",
            ],
        ),
        (
            256,
            [
                "2.16.840.1.101.3.4.1.41",
                "2.16.840.1.101.3.4.1.42",
                "2.16.840.1.101.3.4.1.43",
                "2.16.840.1.101.3.4.1.44",
                "2.16.840.1.101.3.4.1.46",
                "2.16.840.1.101.3.4.1.47",
                "2.16.840.1.101.3.4.1.45",
                "2.16.840.1.101.3.4.1.48",
            ],
        ),
    ];
    for (bits, oids) in aes_oids {
        let k = bits / 8;
        let n = |m: &str| format!("aes-{}-{}", bits, m);
        t.push(spec(
            &n("ecb"),
            &[],
            Algo::Aes,
            Mode::Ecb,
            k,
            0,
            16,
            false,
            Some(oids[0]),
        ));
        t.push(spec(
            &n("cbc"),
            &[&format!("aes{}", bits)],
            Algo::Aes,
            Mode::Cbc,
            k,
            16,
            16,
            false,
            Some(oids[1]),
        ));
        t.push(spec(
            &n("ofb"),
            &[],
            Algo::Aes,
            Mode::Ofb,
            k,
            16,
            1,
            false,
            Some(oids[2]),
        ));
        t.push(spec(
            &n("cfb"),
            &[],
            Algo::Aes,
            Mode::Cfb,
            k,
            16,
            1,
            false,
            Some(oids[3]),
        ));
        t.push(spec(
            &n("cfb1"),
            &[],
            Algo::Aes,
            Mode::Cfb1,
            k,
            16,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("cfb8"),
            &[],
            Algo::Aes,
            Mode::Cfb8,
            k,
            16,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("ctr"),
            &[],
            Algo::Aes,
            Mode::Ctr,
            k,
            16,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("gcm"),
            &[&format!("id-aes{}-gcm", bits)],
            Algo::Aes,
            Mode::Gcm,
            k,
            12,
            1,
            false,
            Some(oids[4]),
        ));
        t.push(spec(
            &n("ccm"),
            &[&format!("id-aes{}-ccm", bits)],
            Algo::Aes,
            Mode::Ccm,
            k,
            12,
            1,
            false,
            Some(oids[5]),
        ));
        t.push(spec(
            &n("ocb"),
            &[],
            Algo::Aes,
            Mode::Ocb,
            k,
            12,
            16,
            false,
            None,
        ));
        t.push(spec(
            &n("wrap"),
            &[
                &format!("id-aes{}-wrap", bits),
                &format!("aes{}-wrap", bits),
            ],
            Algo::Aes,
            Mode::Wrap,
            k,
            8,
            8,
            false,
            Some(oids[6]),
        ));
        t.push(spec(
            &n("wrap-pad"),
            &[
                &format!("id-aes{}-wrap-pad", bits),
                &format!("aes{}-wrap-pad", bits),
            ],
            Algo::Aes,
            Mode::WrapPad,
            k,
            4,
            8,
            false,
            Some(oids[7]),
        ));
    }
    t.push(spec(
        "aes-128-xts",
        &[],
        Algo::Aes,
        Mode::Xts,
        32,
        16,
        1,
        false,
        Some("1.3.111.2.1619.0.1.1"),
    ));
    t.push(spec(
        "aes-256-xts",
        &[],
        Algo::Aes,
        Mode::Xts,
        64,
        16,
        1,
        false,
        Some("1.3.111.2.1619.0.1.2"),
    ));
    // ARIA
    for bits in [128usize, 192, 256] {
        let k = bits / 8;
        let n = |m: &str| format!("aria-{}-{}", bits, m);
        t.push(spec(
            &n("ecb"),
            &[],
            Algo::Aria,
            Mode::Ecb,
            k,
            0,
            16,
            false,
            None,
        ));
        t.push(spec(
            &n("cbc"),
            &[&format!("aria{}", bits)],
            Algo::Aria,
            Mode::Cbc,
            k,
            16,
            16,
            false,
            None,
        ));
        t.push(spec(
            &n("ofb"),
            &[],
            Algo::Aria,
            Mode::Ofb,
            k,
            16,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("cfb"),
            &[],
            Algo::Aria,
            Mode::Cfb,
            k,
            16,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("cfb1"),
            &[],
            Algo::Aria,
            Mode::Cfb1,
            k,
            16,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("cfb8"),
            &[],
            Algo::Aria,
            Mode::Cfb8,
            k,
            16,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("ctr"),
            &[],
            Algo::Aria,
            Mode::Ctr,
            k,
            16,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("gcm"),
            &[],
            Algo::Aria,
            Mode::Gcm,
            k,
            12,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("ccm"),
            &[],
            Algo::Aria,
            Mode::Ccm,
            k,
            12,
            1,
            false,
            None,
        ));
    }
    // Camellia
    for bits in [128usize, 192, 256] {
        let k = bits / 8;
        let n = |m: &str| format!("camellia-{}-{}", bits, m);
        t.push(spec(
            &n("ecb"),
            &[],
            Algo::Camellia,
            Mode::Ecb,
            k,
            0,
            16,
            false,
            None,
        ));
        t.push(spec(
            &n("cbc"),
            &[&format!("camellia{}", bits)],
            Algo::Camellia,
            Mode::Cbc,
            k,
            16,
            16,
            false,
            None,
        ));
        t.push(spec(
            &n("ofb"),
            &[],
            Algo::Camellia,
            Mode::Ofb,
            k,
            16,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("cfb"),
            &[],
            Algo::Camellia,
            Mode::Cfb,
            k,
            16,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("cfb1"),
            &[],
            Algo::Camellia,
            Mode::Cfb1,
            k,
            16,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("cfb8"),
            &[],
            Algo::Camellia,
            Mode::Cfb8,
            k,
            16,
            1,
            false,
            None,
        ));
        t.push(spec(
            &n("ctr"),
            &[],
            Algo::Camellia,
            Mode::Ctr,
            k,
            16,
            1,
            false,
            None,
        ));
    }
    // SM4
    t.push(spec(
        "sm4-ecb",
        &[],
        Algo::Sm4,
        Mode::Ecb,
        16,
        0,
        16,
        false,
        None,
    ));
    t.push(spec(
        "sm4-cbc",
        &["sm4"],
        Algo::Sm4,
        Mode::Cbc,
        16,
        16,
        16,
        false,
        None,
    ));
    t.push(spec(
        "sm4-ofb",
        &["sm4-ofb128"],
        Algo::Sm4,
        Mode::Ofb,
        16,
        16,
        1,
        false,
        None,
    ));
    t.push(spec(
        "sm4-cfb",
        &["sm4-cfb128"],
        Algo::Sm4,
        Mode::Cfb,
        16,
        16,
        1,
        false,
        None,
    ));
    t.push(spec(
        "sm4-ctr",
        &[],
        Algo::Sm4,
        Mode::Ctr,
        16,
        16,
        1,
        false,
        None,
    ));
    // Triple DES
    t.push(spec(
        "des-ede3-ecb",
        &["des-ede3"],
        Algo::DesEde3,
        Mode::Ecb,
        24,
        0,
        8,
        false,
        None,
    ));
    t.push(spec(
        "des-ede3-cbc",
        &["des3"],
        Algo::DesEde3,
        Mode::Cbc,
        24,
        8,
        8,
        false,
        Some("1.2.840.113549.3.7"),
    ));
    t.push(spec(
        "des-ede3-ofb",
        &[],
        Algo::DesEde3,
        Mode::Ofb,
        24,
        8,
        1,
        false,
        None,
    ));
    t.push(spec(
        "des-ede3-cfb",
        &[],
        Algo::DesEde3,
        Mode::Cfb,
        24,
        8,
        1,
        false,
        None,
    ));
    t.push(spec(
        "des-ede3-cfb8",
        &[],
        Algo::DesEde3,
        Mode::Cfb8,
        24,
        8,
        1,
        false,
        None,
    ));
    t.push(spec(
        "des-ede3-cfb1",
        &[],
        Algo::DesEde3,
        Mode::Cfb1,
        24,
        8,
        1,
        false,
        None,
    ));
    t.push(spec(
        "des-ede-ecb",
        &["des-ede"],
        Algo::DesEde,
        Mode::Ecb,
        16,
        0,
        8,
        false,
        None,
    ));
    t.push(spec(
        "des-ede-cbc",
        &[],
        Algo::DesEde,
        Mode::Cbc,
        16,
        8,
        8,
        false,
        None,
    ));
    t.push(spec(
        "des-ede-ofb",
        &[],
        Algo::DesEde,
        Mode::Ofb,
        16,
        8,
        1,
        false,
        None,
    ));
    t.push(spec(
        "des-ede-cfb",
        &[],
        Algo::DesEde,
        Mode::Cfb,
        16,
        8,
        1,
        false,
        None,
    ));
    // ChaCha20
    t.push(spec(
        "chacha20",
        &[],
        Algo::ChaCha20,
        Mode::Stream,
        32,
        16,
        1,
        false,
        None,
    ));
    t.push(spec(
        "chacha20-poly1305",
        &[],
        Algo::ChaCha20Poly1305,
        Mode::Stream,
        32,
        12,
        1,
        false,
        None,
    ));
    t.push(spec(
        "null",
        &[],
        Algo::Null,
        Mode::Stream,
        0,
        0,
        1,
        false,
        None,
    ));
    // Legacy provider
    for (pfx, algo, klen, var) in [
        ("bf", Algo::Bf, 16usize, true),
        ("cast5", Algo::Cast5, 16, true),
        ("idea", Algo::Idea, 16, false),
        ("rc2", Algo::Rc2(128), 16, true),
        ("des", Algo::Des, 8, false),
    ] {
        let n = |m: &str| format!("{}-{}", pfx, m);
        let cbc_alias: Vec<&str> = match pfx {
            "bf" => vec!["bf", "blowfish"],
            "cast5" => vec!["cast", "cast-cbc"],
            "idea" => vec!["idea"],
            "rc2" => vec!["rc2", "rc2-128"],
            _ => vec!["des"],
        };
        let cbc_oid = match pfx {
            "rc2" => Some("1.2.840.113549.3.2"),
            "des" => Some("1.3.14.3.2.7"),
            _ => None,
        };
        t.push(spec(&n("ecb"), &[], algo, Mode::Ecb, klen, 0, 8, var, None));
        t.push(spec(
            &n("cbc"),
            &cbc_alias,
            algo,
            Mode::Cbc,
            klen,
            8,
            8,
            var,
            cbc_oid,
        ));
        t.push(spec(&n("ofb"), &[], algo, Mode::Ofb, klen, 8, 1, var, None));
        t.push(spec(&n("cfb"), &[], algo, Mode::Cfb, klen, 8, 1, var, None));
        if pfx == "des" {
            t.push(spec(
                &n("cfb1"),
                &[],
                algo,
                Mode::Cfb1,
                klen,
                8,
                1,
                var,
                None,
            ));
            t.push(spec(
                &n("cfb8"),
                &[],
                algo,
                Mode::Cfb8,
                klen,
                8,
                1,
                var,
                None,
            ));
        }
    }
    t.push(spec(
        "seed-ecb",
        &[],
        Algo::Seed,
        Mode::Ecb,
        16,
        0,
        16,
        false,
        None,
    ));
    t.push(spec(
        "seed-cbc",
        &["seed"],
        Algo::Seed,
        Mode::Cbc,
        16,
        16,
        16,
        false,
        Some("1.2.410.200004.1.4"),
    ));
    t.push(spec(
        "seed-ofb",
        &[],
        Algo::Seed,
        Mode::Ofb,
        16,
        16,
        1,
        false,
        None,
    ));
    t.push(spec(
        "seed-cfb",
        &[],
        Algo::Seed,
        Mode::Cfb,
        16,
        16,
        1,
        false,
        None,
    ));
    t.push(spec(
        "rc2-40-cbc",
        &["rc2-40"],
        Algo::Rc2(40),
        Mode::Cbc,
        5,
        8,
        8,
        true,
        Some("1.2.840.113549.3.2"),
    ));
    t.push(spec(
        "rc2-64-cbc",
        &["rc2-64"],
        Algo::Rc2(64),
        Mode::Cbc,
        8,
        8,
        8,
        true,
        Some("1.2.840.113549.3.2"),
    ));
    t.push(spec(
        "rc4",
        &[],
        Algo::Rc4,
        Mode::Stream,
        16,
        0,
        1,
        true,
        None,
    ));
    t.push(spec(
        "rc4-40",
        &[],
        Algo::Rc4,
        Mode::Stream,
        5,
        0,
        1,
        true,
        None,
    ));
    t.push(spec(
        "desx-cbc",
        &["desx"],
        Algo::DesX,
        Mode::Cbc,
        24,
        8,
        8,
        false,
        None,
    ));
    t
}

fn table() -> &'static [CipherSpec] {
    static T: OnceLock<Vec<CipherSpec>> = OnceLock::new();
    T.get_or_init(build_table)
}

/// Look up a cipher by name (case-insensitive, names or aliases).
pub fn by_name(name: &str) -> Option<&'static CipherSpec> {
    let n = name.to_ascii_lowercase();
    table()
        .iter()
        .find(|s| s.name == n || s.aliases.contains(&n))
}

/// Look up a cipher by its ASN.1 object identifier (first table entry).
pub fn by_oid(oid: &str) -> Option<&'static CipherSpec> {
    table().iter().find(|s| s.oid == Some(oid))
}

/// openssl_get_cipher_methods(): sorted names (and aliases).
pub fn method_names(aliases: bool) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for s in table() {
        v.push(s.name.clone());
        if aliases {
            v.extend(s.aliases.iter().cloned());
            if let Some(o) = s.oid {
                v.push(o.to_string());
            }
        }
    }
    v.sort();
    v.dedup();
    v
}

// ---------------------------------------------------------------------------
// Block cipher primitives
// ---------------------------------------------------------------------------

pub trait Block {
    fn bs(&self) -> usize;
    fn enc(&self, b: &mut [u8]);
    fn dec(&self, b: &mut [u8]);
}

/// RustCrypto block ciphers (only for algorithms purecrypto lacks).
struct Bc<C>(C);

impl<C: cipher::BlockEncrypt + cipher::BlockDecrypt> Block for Bc<C> {
    fn bs(&self) -> usize {
        C::block_size()
    }
    fn enc(&self, b: &mut [u8]) {
        self.0.encrypt_block(GenericArray::from_mut_slice(b));
    }
    fn dec(&self, b: &mut [u8]) {
        self.0.decrypt_block(GenericArray::from_mut_slice(b));
    }
}

/// purecrypto 128-bit block ciphers.
struct Pc16<C>(C);

impl<C: purecrypto::cipher::BlockCipher> Block for Pc16<C> {
    fn bs(&self) -> usize {
        16
    }
    fn enc(&self, b: &mut [u8]) {
        let blk: &mut [u8; 16] = b.try_into().expect("16-byte block");
        self.0.encrypt_block(blk);
    }
    fn dec(&self, b: &mut [u8]) {
        let blk: &mut [u8; 16] = b.try_into().expect("16-byte block");
        self.0.decrypt_block(blk);
    }
}

/// purecrypto 64-bit block ciphers.
struct Pc8<C>(C);

impl<C: purecrypto::cipher::BlockCipher64> Block for Pc8<C> {
    fn bs(&self) -> usize {
        8
    }
    fn enc(&self, b: &mut [u8]) {
        let blk: &mut [u8; 8] = b.try_into().expect("8-byte block");
        self.0.encrypt_block(blk);
    }
    fn dec(&self, b: &mut [u8]) {
        let blk: &mut [u8; 8] = b.try_into().expect("8-byte block");
        self.0.decrypt_block(blk);
    }
}

struct DesX {
    k: Pc8<purecrypto::cipher::Des>,
    pre: [u8; 8],
    post: [u8; 8],
}

fn xor8(b: &mut [u8], k: &[u8; 8]) {
    for (x, y) in b.iter_mut().zip(k) {
        *x ^= y;
    }
}

impl Block for DesX {
    fn bs(&self) -> usize {
        8
    }
    fn enc(&self, b: &mut [u8]) {
        xor8(b, &self.pre);
        self.k.enc(b);
        xor8(b, &self.post);
    }
    fn dec(&self, b: &mut [u8]) {
        xor8(b, &self.post);
        self.k.dec(b);
        xor8(b, &self.pre);
    }
}

struct Bf(blowfish::Blowfish);

impl Block for Bf {
    fn bs(&self) -> usize {
        8
    }
    fn enc(&self, b: &mut [u8]) {
        use cipher::BlockEncrypt;
        self.0.encrypt_block(GenericArray::from_mut_slice(b));
    }
    fn dec(&self, b: &mut [u8]) {
        use cipher::BlockDecrypt;
        self.0.decrypt_block(GenericArray::from_mut_slice(b));
    }
}

fn arr<const N: usize>(k: &[u8]) -> Option<[u8; N]> {
    k.try_into().ok()
}

/// Build the block primitive for `algo` with the exact key bytes.
pub fn make_block(algo: Algo, key: &[u8]) -> Option<Box<dyn Block>> {
    use purecrypto::cipher as pc;
    Some(match algo {
        Algo::Aes => match key.len() {
            16 => Box::new(Pc16(pc::Aes128::new(&arr(key)?))),
            24 => Box::new(Pc16(pc::Aes192::new(&arr(key)?))),
            32 => Box::new(Pc16(pc::Aes256::new(&arr(key)?))),
            _ => return None,
        },
        Algo::Aria => match key.len() {
            16 => Box::new(Pc16(pc::Aria128::new(&arr(key)?))),
            24 => Box::new(Pc16(pc::Aria192::new(&arr(key)?))),
            32 => Box::new(Pc16(pc::Aria256::new(&arr(key)?))),
            _ => return None,
        },
        Algo::Camellia => match key.len() {
            16 => Box::new(Pc16(pc::Camellia128::new(&arr(key)?))),
            24 => Box::new(Pc16(pc::Camellia192::new(&arr(key)?))),
            32 => Box::new(Pc16(pc::Camellia256::new(&arr(key)?))),
            _ => return None,
        },
        Algo::Sm4 => Box::new(Pc16(pc::Sm4::new(&arr(key)?))),
        Algo::Seed => Box::new(Pc16(pc::Seed::new(&arr(key)?))),
        Algo::Des => Box::new(Pc8(pc::Des::new(&arr(key)?))),
        Algo::DesEde => Box::new(Pc8(pc::TdesEde2::new(&arr(key)?))),
        Algo::DesEde3 => Box::new(Pc8(pc::TdesEde3::new(&arr(key)?))),
        Algo::DesX => {
            if key.len() != 24 {
                return None;
            }
            let mut pre = [0u8; 8];
            let mut post = [0u8; 8];
            pre.copy_from_slice(&key[8..16]);
            post.copy_from_slice(&key[16..24]);
            Box::new(DesX {
                k: Pc8(pc::Des::new(&arr(&key[..8])?)),
                pre,
                post,
            })
        }
        Algo::Bf => {
            if key.is_empty() {
                return None;
            }
            let k = &key[..key.len().min(72)];
            let mut bf = blowfish::Blowfish::bc_init_state();
            bf.bc_expand_key(k);
            Box::new(Bf(bf))
        }
        Algo::Cast5 => {
            use cipher::KeyInit;
            let mut k = key[..key.len().min(16)].to_vec();
            if k.len() < 5 {
                k.resize(5, 0);
            }
            Box::new(Bc(cast5::Cast5::new_from_slice(&k).ok()?))
        }
        Algo::Idea => {
            use cipher::KeyInit;
            Box::new(Bc(idea::Idea::new_from_slice(key).ok()?))
        }
        Algo::Rc2(ekb) => {
            if key.is_empty() {
                return None;
            }
            let k = &key[..key.len().min(128)];
            Box::new(Bc(rc2::Rc2::new_with_eff_key_len(k, ekb)))
        }
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Parameters and result of a one-shot cipher operation
// ---------------------------------------------------------------------------

/// A failure inside the EVP layer. Errors are already queued in `err`; the
/// variant tells php-level code which warning (if any) to emit.
#[derive(Debug, PartialEq, Eq)]
pub enum Fail {
    /// Silent failure (only the OpenSSL error queue is populated).
    Silent,
    /// Setting of data length failed (CCM).
    DataLength,
}

pub struct Op<'a> {
    pub spec: &'a CipherSpec,
    pub key: &'a [u8],
    pub iv: &'a [u8],
    pub enc: bool,
    pub padding: bool,
    pub aad: &'a [u8],
    /// AEAD tag length (encrypt: requested tag length; decrypt: tag len).
    pub tag_len: usize,
    /// AEAD tag to verify (decrypt).
    pub tag: Option<&'a [u8]>,
}

/// Run a full EVP_CipherInit/Update/Final sequence on `data`.
/// Returns the output and, for AEAD encryption, the computed tag (of
/// `tag_len` bytes, or the mode's maximum if that is out of range).
pub fn run(op: &Op, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Fail> {
    let spec = op.spec;
    match spec.algo {
        Algo::Null => return Ok((data.to_vec(), Vec::new())),
        Algo::Rc4 => return Ok((rc4(op.key, data), Vec::new())),
        Algo::ChaCha20 => return Ok((chacha20_xor(op.key, op.iv, data), Vec::new())),
        Algo::ChaCha20Poly1305 => return chacha20_poly1305(op, data),
        _ => {}
    }
    if spec.mode == Mode::Xts {
        return xts(op, data);
    }
    let blk = match make_block(spec.algo, op.key) {
        Some(b) => b,
        None => {
            err::evp_invalid_key_length();
            return Err(Fail::Silent);
        }
    };
    let bs = blk.bs();
    match spec.mode {
        Mode::Ecb | Mode::Cbc => {
            let cbc = spec.mode == Mode::Cbc;
            if op.enc {
                let mut buf = data.to_vec();
                if op.padding {
                    let p = bs - buf.len() % bs;
                    buf.extend(std::iter::repeat_n(p as u8, p));
                } else if !buf.len().is_multiple_of(bs) {
                    err::prov_wrong_final_block_length();
                    return Err(Fail::Silent);
                }
                let mut chain = op.iv[..bs.min(op.iv.len())].to_vec();
                chain.resize(bs, 0);
                for c in buf.chunks_mut(bs) {
                    if cbc {
                        for i in 0..bs {
                            c[i] ^= chain[i];
                        }
                    }
                    blk.enc(c);
                    if cbc {
                        chain.copy_from_slice(c);
                    }
                }
                Ok((buf, Vec::new()))
            } else {
                if !data.len().is_multiple_of(bs) || (data.is_empty() && op.padding) {
                    err::prov_wrong_final_block_length();
                    return Err(Fail::Silent);
                }
                let mut buf = data.to_vec();
                let mut chain = op.iv[..bs.min(op.iv.len())].to_vec();
                chain.resize(bs, 0);
                for c in buf.chunks_mut(bs) {
                    let saved = c.to_vec();
                    blk.dec(c);
                    if cbc {
                        for i in 0..bs {
                            c[i] ^= chain[i];
                        }
                        chain = saved;
                    }
                }
                if op.padding {
                    let p = *buf.last().unwrap() as usize;
                    if p == 0 || p > bs || buf[buf.len() - p..].iter().any(|&b| b as usize != p) {
                        err::prov_bad_decrypt();
                        return Err(Fail::Silent);
                    }
                    buf.truncate(buf.len() - p);
                }
                Ok((buf, Vec::new()))
            }
        }
        Mode::Cfb => {
            let mut reg = op.iv.to_vec();
            reg.resize(bs, 0);
            let mut ks = reg.clone();
            blk.enc(&mut ks);
            let mut out = Vec::with_capacity(data.len());
            let mut n = 0;
            for &b in data {
                if n == bs {
                    ks.copy_from_slice(&reg);
                    blk.enc(&mut ks);
                    n = 0;
                }
                let o = b ^ ks[n];
                reg[n] = if op.enc { o } else { b };
                out.push(o);
                n += 1;
            }
            Ok((out, Vec::new()))
        }
        Mode::Cfb8 => {
            let mut reg = op.iv.to_vec();
            reg.resize(bs, 0);
            let mut out = Vec::with_capacity(data.len());
            for &b in data {
                let mut ks = reg.clone();
                blk.enc(&mut ks);
                let o = b ^ ks[0];
                reg.remove(0);
                reg.push(if op.enc { o } else { b });
                out.push(o);
            }
            Ok((out, Vec::new()))
        }
        Mode::Cfb1 => {
            let mut reg = op.iv.to_vec();
            reg.resize(bs, 0);
            let mut out = Vec::with_capacity(data.len());
            for &b in data {
                let mut ob = 0u8;
                for bit in (0..8).rev() {
                    let mut ks = reg.clone();
                    blk.enc(&mut ks);
                    let ib = (b >> bit) & 1;
                    let o = ib ^ (ks[0] >> 7);
                    ob |= o << bit;
                    let fb = if op.enc { o } else { ib };
                    // shift register left by one bit, appending fb
                    for i in 0..bs {
                        let next = if i + 1 < bs { reg[i + 1] >> 7 } else { fb };
                        reg[i] = (reg[i] << 1) | next;
                    }
                }
                out.push(ob);
            }
            Ok((out, Vec::new()))
        }
        Mode::Ofb => {
            let mut ks = op.iv.to_vec();
            ks.resize(bs, 0);
            let mut out = Vec::with_capacity(data.len());
            let mut n = bs;
            for &b in data {
                if n == bs {
                    blk.enc(&mut ks);
                    n = 0;
                }
                out.push(b ^ ks[n]);
                n += 1;
            }
            Ok((out, Vec::new()))
        }
        Mode::Ctr => {
            let mut ctr = op.iv.to_vec();
            ctr.resize(bs, 0);
            Ok((ctr_xor(blk.as_ref(), &mut ctr, data, false), Vec::new()))
        }
        Mode::Gcm => gcm(blk.as_ref(), op, data),
        Mode::Ccm => ccm(blk.as_ref(), op, data),
        Mode::Ocb => ocb(blk.as_ref(), op, data),
        Mode::Wrap | Mode::WrapPad => wrap(blk.as_ref(), op, data),
        Mode::Stream | Mode::Xts => Err(Fail::Silent),
    }
}

/// CTR keystream XOR; `inc32` restricts the increment to the low 32 bits (GCM).
fn ctr_xor(blk: &dyn Block, ctr: &mut [u8], data: &[u8], inc32: bool) -> Vec<u8> {
    let bs = blk.bs();
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(bs) {
        let mut ks = ctr.to_vec();
        blk.enc(&mut ks);
        for (i, &b) in chunk.iter().enumerate() {
            out.push(b ^ ks[i]);
        }
        let lim = if inc32 { bs - 4 } else { 0 };
        for i in (lim..bs).rev() {
            ctr[i] = ctr[i].wrapping_add(1);
            if ctr[i] != 0 {
                break;
            }
        }
    }
    out
}

fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut s: [u8; 256] = [0; 256];
    for (i, v) in s.iter_mut().enumerate() {
        *v = i as u8;
    }
    if !key.is_empty() {
        let mut j: u8 = 0;
        for i in 0..256 {
            j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
            s.swap(i, j as usize);
        }
    }
    let (mut i, mut j) = (0u8, 0u8);
    data.iter()
        .map(|&b| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(s[i as usize]);
            s.swap(i as usize, j as usize);
            b ^ s[(s[i as usize].wrapping_add(s[j as usize])) as usize]
        })
        .collect()
}

// --- ChaCha20 -------------------------------------------------------------

fn chacha_block(key: &[u8], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let k: [u8; 32] = key.try_into().unwrap_or([0u8; 32]);
    purecrypto::cipher::ChaCha20::new(&k).block(nonce, counter)
}

/// ChaCha20 with a 16-byte IV = 32-bit LE counter || 96-bit nonce (OpenSSL).
fn chacha20_xor(key: &[u8], iv: &[u8], data: &[u8]) -> Vec<u8> {
    let mut ivb = [0u8; 16];
    ivb[..iv.len().min(16)].copy_from_slice(&iv[..iv.len().min(16)]);
    let counter = u32::from_le_bytes([ivb[0], ivb[1], ivb[2], ivb[3]]);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&ivb[4..]);
    chacha_stream(key, counter, &nonce, data)
}

fn chacha_stream(key: &[u8], mut counter: u32, nonce: &[u8; 12], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(64) {
        let ks = chacha_block(key, counter, nonce);
        for (i, &b) in chunk.iter().enumerate() {
            out.push(b ^ ks[i]);
        }
        counter = counter.wrapping_add(1);
    }
    out
}

fn poly1305_tag(otk: &[u8], aad: &[u8], ct: &[u8]) -> [u8; 16] {
    let k: [u8; 32] = otk[..32].try_into().unwrap_or([0u8; 32]);
    let mut mac = purecrypto::cipher::Poly1305::new(&k);
    let pad = |n: usize| vec![0u8; (16 - n % 16) % 16];
    mac.update(aad);
    mac.update(&pad(aad.len()));
    mac.update(ct);
    mac.update(&pad(ct.len()));
    mac.update(&(aad.len() as u64).to_le_bytes());
    mac.update(&(ct.len() as u64).to_le_bytes());
    mac.finish()
}

fn chacha20_poly1305(op: &Op, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Fail> {
    // The IV is right-aligned in the 12-byte nonce.
    let mut nonce = [0u8; 12];
    let n = op.iv.len().min(12);
    nonce[12 - n..].copy_from_slice(&op.iv[op.iv.len() - n..]);
    let otk = chacha_block(op.key, 0, &nonce);
    if op.enc {
        let ct = chacha_stream(op.key, 1, &nonce, data);
        let tag = poly1305_tag(&otk, op.aad, &ct);
        Ok((ct, tag.to_vec()))
    } else {
        let tag = poly1305_tag(&otk, op.aad, data);
        match op.tag {
            Some(t) if !t.is_empty() && t.len() <= 16 && tag[..t.len()] == *t => {}
            _ => return Err(Fail::Silent),
        }
        Ok((chacha_stream(op.key, 1, &nonce, data), Vec::new()))
    }
}

// --- GCM ------------------------------------------------------------------

fn gf_mul(x: u128, y: u128) -> u128 {
    let r: u128 = 0xE1 << 120;
    let mut z = 0u128;
    let mut v = y;
    for i in 0..128 {
        if (x >> (127 - i)) & 1 == 1 {
            z ^= v;
        }
        v = if v & 1 == 1 { (v >> 1) ^ r } else { v >> 1 };
    }
    z
}

fn ghash(h: u128, aad: &[u8], ct: &[u8]) -> u128 {
    let mut y = 0u128;
    for part in [aad, ct] {
        for chunk in part.chunks(16) {
            let mut b = [0u8; 16];
            b[..chunk.len()].copy_from_slice(chunk);
            y = gf_mul(y ^ u128::from_be_bytes(b), h);
        }
    }
    let lens = ((aad.len() as u128 * 8) << 64) | (ct.len() as u128 * 8);
    gf_mul(y ^ lens, h)
}

fn gcm(blk: &dyn Block, op: &Op, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Fail> {
    let mut hb = [0u8; 16];
    blk.enc(&mut hb);
    let h = u128::from_be_bytes(hb);
    let mut j0 = [0u8; 16];
    if op.iv.len() == 12 {
        j0[..12].copy_from_slice(op.iv);
        j0[15] = 1;
    } else {
        j0 = ghash(h, &[], op.iv).to_be_bytes();
    }
    let mut ctr = j0;
    for i in (12..16).rev() {
        ctr[i] = ctr[i].wrapping_add(1);
        if ctr[i] != 0 {
            break;
        }
    }
    let mut ek0 = j0;
    blk.enc(&mut ek0);
    if op.enc {
        let ct = ctr_xor(blk, &mut ctr, data, true);
        let s = ghash(h, op.aad, &ct) ^ u128::from_be_bytes(ek0);
        Ok((ct, s.to_be_bytes().to_vec()))
    } else {
        let s = (ghash(h, op.aad, data) ^ u128::from_be_bytes(ek0)).to_be_bytes();
        match op.tag {
            Some(t) if !t.is_empty() && t.len() <= 16 && s[..t.len()] == *t => {}
            _ => return Err(Fail::Silent),
        }
        Ok((ctr_xor(blk, &mut ctr, data, true), Vec::new()))
    }
}

// --- CCM ------------------------------------------------------------------

fn ccm(blk: &dyn Block, op: &Op, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Fail> {
    let n = op.iv.len();
    let l = 15 - n;
    let m = op.tag_len;
    if l < 8 && (data.len() as u64) >> (8 * l) != 0 {
        err::push(err::LIB_PROV, 0x6E, "invalid data length");
        return Err(Fail::DataLength);
    }
    let mut b0 = [0u8; 16];
    b0[0] =
        (if op.aad.is_empty() { 0 } else { 0x40 }) | ((((m - 2) / 2) as u8) << 3) | ((l - 1) as u8);
    b0[1..1 + n].copy_from_slice(op.iv);
    let mlen = data.len() as u64;
    for i in 0..l {
        b0[15 - i] = (mlen >> (8 * i)) as u8;
    }
    let cbc_mac = |payload: &[u8]| -> [u8; 16] {
        let mut x = b0;
        blk.enc(&mut x);
        if !op.aad.is_empty() {
            let mut a = Vec::new();
            let al = op.aad.len() as u64;
            if al < 0xFF00 {
                a.extend_from_slice(&(al as u16).to_be_bytes());
            } else if al <= 0xFFFF_FFFF {
                a.extend_from_slice(&[0xFF, 0xFE]);
                a.extend_from_slice(&(al as u32).to_be_bytes());
            } else {
                a.extend_from_slice(&[0xFF, 0xFF]);
                a.extend_from_slice(&al.to_be_bytes());
            }
            a.extend_from_slice(op.aad);
            for c in a.chunks(16) {
                for (i, &b) in c.iter().enumerate() {
                    x[i] ^= b;
                }
                blk.enc(&mut x);
            }
        }
        for c in payload.chunks(16) {
            for (i, &b) in c.iter().enumerate() {
                x[i] ^= b;
            }
            blk.enc(&mut x);
        }
        x
    };
    let mut a0 = [0u8; 16];
    a0[0] = (l - 1) as u8;
    a0[1..1 + n].copy_from_slice(op.iv);
    let mut s0 = a0;
    blk.enc(&mut s0);
    let mut ctr = a0;
    ctr[15] = 1;
    if op.enc {
        let t = cbc_mac(data);
        let ct = ctr_xor(blk, &mut ctr, data, false);
        let tag: Vec<u8> = (0..m).map(|i| t[i] ^ s0[i]).collect();
        Ok((ct, tag))
    } else {
        let pt = ctr_xor(blk, &mut ctr, data, false);
        let t = cbc_mac(&pt);
        let tag: Vec<u8> = (0..m).map(|i| t[i] ^ s0[i]).collect();
        match op.tag {
            Some(g) if g == tag.as_slice() => Ok((pt, Vec::new())),
            _ => Err(Fail::Silent),
        }
    }
}

// --- OCB (RFC 7253) -------------------------------------------------------

fn dbl(b: &[u8; 16]) -> [u8; 16] {
    let v = u128::from_be_bytes(*b);
    let r = (v << 1) ^ if v >> 127 == 1 { 0x87 } else { 0 };
    r.to_be_bytes()
}

fn xor16(a: &mut [u8; 16], b: &[u8; 16]) {
    for i in 0..16 {
        a[i] ^= b[i];
    }
}

fn ocb(blk: &dyn Block, op: &Op, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Fail> {
    let taglen = op.tag_len;
    let mut l_star = [0u8; 16];
    blk.enc(&mut l_star);
    let l_dollar = dbl(&l_star);
    let mut ls = vec![dbl(&l_dollar)];
    let l_at = |ls: &mut Vec<[u8; 16]>, i: usize| -> [u8; 16] {
        while ls.len() <= i {
            let n = dbl(ls.last().unwrap());
            ls.push(n);
        }
        ls[i]
    };
    // Nonce-dependent offset
    let mut nonce = [0u8; 16];
    nonce[0] = (((taglen * 8) % 128) as u8) << 1;
    let nl = op.iv.len();
    nonce[16 - nl..].copy_from_slice(op.iv);
    nonce[15 - nl] |= 1;
    let bottom = (nonce[15] & 0x3f) as usize;
    let mut ktop_in = nonce;
    ktop_in[15] &= 0xc0;
    blk.enc(&mut ktop_in);
    let ktop = ktop_in;
    let mut stretch = [0u8; 24];
    stretch[..16].copy_from_slice(&ktop);
    for i in 0..8 {
        stretch[16 + i] = ktop[i] ^ ktop[i + 1];
    }
    let mut offset = [0u8; 16];
    for (i, o) in offset.iter_mut().enumerate() {
        let bit = i * 8 + bottom;
        let byte = bit / 8;
        let sh = bit % 8;
        *o = if sh == 0 {
            stretch[byte]
        } else {
            (stretch[byte] << sh) | (stretch[byte + 1] >> (8 - sh))
        };
    }
    let mut checksum = [0u8; 16];
    let mut out = Vec::with_capacity(data.len());
    let full = data.len() / 16;
    for i in 0..full {
        let li = l_at(&mut ls, (i + 1).trailing_zeros() as usize);
        xor16(&mut offset, &li);
        let mut b = [0u8; 16];
        b.copy_from_slice(&data[i * 16..i * 16 + 16]);
        if op.enc {
            xor16(&mut checksum, &b);
        }
        xor16(&mut b, &offset);
        if op.enc {
            blk.enc(&mut b);
        } else {
            blk.dec(&mut b);
        }
        xor16(&mut b, &offset);
        if !op.enc {
            xor16(&mut checksum, &b);
        }
        out.extend_from_slice(&b);
    }
    let rest = &data[full * 16..];
    if !rest.is_empty() {
        xor16(&mut offset, &l_star);
        let mut pad = offset;
        blk.enc(&mut pad);
        let mut p = [0u8; 16];
        for (i, &b) in rest.iter().enumerate() {
            let o = b ^ pad[i];
            out.push(o);
            p[i] = if op.enc { b } else { o };
        }
        p[rest.len()] = 0x80;
        xor16(&mut checksum, &p);
    }
    // HASH(K, A)
    let mut sum = [0u8; 16];
    let mut aoff = [0u8; 16];
    let afull = op.aad.len() / 16;
    for i in 0..afull {
        let li = l_at(&mut ls, (i + 1).trailing_zeros() as usize);
        xor16(&mut aoff, &li);
        let mut b = [0u8; 16];
        b.copy_from_slice(&op.aad[i * 16..i * 16 + 16]);
        xor16(&mut b, &aoff);
        blk.enc(&mut b);
        xor16(&mut sum, &b);
    }
    let arest = &op.aad[afull * 16..];
    if !arest.is_empty() {
        xor16(&mut aoff, &l_star);
        let mut b = [0u8; 16];
        b[..arest.len()].copy_from_slice(arest);
        b[arest.len()] = 0x80;
        xor16(&mut b, &aoff);
        blk.enc(&mut b);
        xor16(&mut sum, &b);
    }
    let mut t = checksum;
    xor16(&mut t, &offset);
    xor16(&mut t, &l_dollar);
    blk.enc(&mut t);
    xor16(&mut t, &sum);
    let tag = t[..taglen.min(16)].to_vec();
    if op.enc {
        Ok((out, tag))
    } else {
        match op.tag {
            Some(g) if !g.is_empty() && g == tag.as_slice() => Ok((out, Vec::new())),
            _ => Err(Fail::Silent),
        }
    }
}

// --- XTS ------------------------------------------------------------------

fn xts(op: &Op, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Fail> {
    let half = op.key.len() / 2;
    let (k1, k2) = op.key.split_at(half);
    if op.enc && k1 == k2 {
        err::push(err::LIB_PROV, 149, "xts duplicated keys");
        return Err(Fail::Silent);
    }
    if data.len() < 16 {
        return Err(Fail::Silent);
    }
    let (c1, c2) = match (make_block(op.spec.algo, k1), make_block(op.spec.algo, k2)) {
        (Some(a), Some(b)) => (a, b),
        _ => return Err(Fail::Silent),
    };
    let mut t = [0u8; 16];
    t.copy_from_slice(&op.iv[..16]);
    c2.enc(&mut t);
    let mul_alpha = |t: &mut [u8; 16]| {
        let mut carry = 0u8;
        for b in t.iter_mut() {
            let nc = *b >> 7;
            *b = (*b << 1) | carry;
            carry = nc;
        }
        if carry == 1 {
            t[0] ^= 0x87;
        }
    };
    let process = |blk: &[u8], t: &[u8; 16]| -> [u8; 16] {
        let mut b = [0u8; 16];
        b.copy_from_slice(blk);
        xor16(&mut b, t);
        if op.enc {
            c1.enc(&mut b);
        } else {
            c1.dec(&mut b);
        }
        xor16(&mut b, t);
        b
    };
    let n = data.len() / 16;
    let r = data.len() % 16;
    let mut out = vec![0u8; data.len()];
    let full_normal = if r == 0 { n } else { n - 1 };
    for i in 0..full_normal {
        let b = process(&data[i * 16..i * 16 + 16], &t);
        out[i * 16..i * 16 + 16].copy_from_slice(&b);
        mul_alpha(&mut t);
    }
    if r != 0 {
        let i = n - 1;
        let last_full = &data[i * 16..i * 16 + 16];
        let tail = &data[n * 16..];
        if op.enc {
            let cc = process(last_full, &t);
            let mut pp = [0u8; 16];
            pp[..r].copy_from_slice(tail);
            pp[r..].copy_from_slice(&cc[r..]);
            mul_alpha(&mut t);
            let c_last = process(&pp, &t);
            out[i * 16..i * 16 + 16].copy_from_slice(&c_last);
            out[n * 16..].copy_from_slice(&cc[..r]);
        } else {
            let mut t2 = t;
            mul_alpha(&mut t2);
            let pp = process(last_full, &t2);
            let mut cc = [0u8; 16];
            cc[..r].copy_from_slice(tail);
            cc[r..].copy_from_slice(&pp[r..]);
            let p_last = process(&cc, &t);
            out[i * 16..i * 16 + 16].copy_from_slice(&p_last);
            out[n * 16..].copy_from_slice(&pp[..r]);
        }
    }
    Ok((out, Vec::new()))
}

// --- Key wrap (RFC 3394 / RFC 5649) ---------------------------------------

fn wrap_core(blk: &dyn Block, a: [u8; 8], r: &mut [[u8; 8]]) -> [u8; 8] {
    let n = r.len();
    let mut a = u64::from_be_bytes(a);
    for j in 0..6 {
        for (i, ri) in r.iter_mut().enumerate() {
            let mut b = [0u8; 16];
            b[..8].copy_from_slice(&a.to_be_bytes());
            b[8..].copy_from_slice(ri);
            blk.enc(&mut b);
            let t = (n * j + i + 1) as u64;
            a = u64::from_be_bytes(b[..8].try_into().unwrap()) ^ t;
            ri.copy_from_slice(&b[8..]);
        }
    }
    a.to_be_bytes()
}

fn unwrap_core(blk: &dyn Block, a: [u8; 8], r: &mut [[u8; 8]]) -> [u8; 8] {
    let n = r.len();
    let mut a = u64::from_be_bytes(a);
    for j in (0..6).rev() {
        for i in (0..n).rev() {
            let t = (n * j + i + 1) as u64;
            let mut b = [0u8; 16];
            b[..8].copy_from_slice(&(a ^ t).to_be_bytes());
            b[8..].copy_from_slice(&r[i]);
            blk.dec(&mut b);
            a = u64::from_be_bytes(b[..8].try_into().unwrap());
            r[i].copy_from_slice(&b[8..]);
        }
    }
    a.to_be_bytes()
}

fn wrap(blk: &dyn Block, op: &Op, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Fail> {
    if data.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let pad = op.spec.mode == Mode::WrapPad;
    if op.enc {
        let (aiv, mut plain) = if pad {
            let mut aiv = [0u8; 8];
            aiv[..4].copy_from_slice(&op.iv[..4]);
            aiv[4..].copy_from_slice(&(data.len() as u32).to_be_bytes());
            let mut p = data.to_vec();
            p.resize(data.len().div_ceil(8) * 8, 0);
            (aiv, p)
        } else {
            if !data.len().is_multiple_of(8) || data.len() < 16 {
                err::push(err::LIB_PROV, 230, "invalid input length");
                return Err(Fail::Silent);
            }
            let mut aiv = [0u8; 8];
            aiv.copy_from_slice(&op.iv[..8]);
            (aiv, data.to_vec())
        };
        if pad && plain.len() == 8 {
            let mut b = [0u8; 16];
            b[..8].copy_from_slice(&aiv);
            b[8..].copy_from_slice(&plain);
            blk.enc(&mut b);
            return Ok((b.to_vec(), Vec::new()));
        }
        let mut r: Vec<[u8; 8]> = plain.chunks(8).map(|c| c.try_into().unwrap()).collect();
        plain.clear();
        let a = wrap_core(blk, aiv, &mut r);
        let mut out = a.to_vec();
        for b in r {
            out.extend_from_slice(&b);
        }
        Ok((out, Vec::new()))
    } else {
        if !data.len().is_multiple_of(8) || data.len() < if pad { 16 } else { 24 } {
            err::push(err::LIB_PROV, 230, "invalid input length");
            return Err(Fail::Silent);
        }
        let (a, plain) = if data.len() == 16 {
            let mut b = [0u8; 16];
            b.copy_from_slice(data);
            blk.dec(&mut b);
            let mut a = [0u8; 8];
            a.copy_from_slice(&b[..8]);
            (a, b[8..].to_vec())
        } else {
            let mut a = [0u8; 8];
            a.copy_from_slice(&data[..8]);
            let mut r: Vec<[u8; 8]> = data[8..].chunks(8).map(|c| c.try_into().unwrap()).collect();
            let a = unwrap_core(blk, a, &mut r);
            (a, r.concat())
        };
        if pad {
            if a[..4] != op.iv[..4] {
                return Err(Fail::Silent);
            }
            let mli = u32::from_be_bytes(a[4..].try_into().unwrap()) as usize;
            if mli > plain.len() || mli + 8 <= plain.len() || plain[mli..].iter().any(|&b| b != 0) {
                return Err(Fail::Silent);
            }
            Ok((plain[..mli].to_vec(), Vec::new()))
        } else {
            if a[..] != op.iv[..8] {
                return Err(Fail::Silent);
            }
            Ok((plain, Vec::new()))
        }
    }
}

/// Convenience one-shot with PKCS#7 padding (used by PKCS#7/CMS code).
pub fn simple(name: &str, data: &[u8], key: &[u8], iv: &[u8], enc: bool) -> Option<Vec<u8>> {
    let spec = by_name(name)?;
    let op = Op {
        spec,
        key,
        iv,
        enc,
        padding: true,
        aad: &[],
        tag_len: 0,
        tag: None,
    };
    run(&op, data).ok().map(|(o, _)| o)
}
