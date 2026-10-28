//! Mode tests against published vectors (NIST SP 800-38A, SP 800-38C/D,
//! RFC 7253, RFC 8439, RFC 3394, RFC 5649) and the cipher list observed on
//! PHP 8.5 (`methods_php85.txt`, `lengths_php85.txt`: the output of
//! openssl_get_cipher_methods() and of openssl_cipher_iv_length()/
//! openssl_cipher_key_length() for every name).

use super::*;

fn h(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn info(n: &str) -> &'static CipherInfo {
    lookup(n).unwrap_or_else(|| panic!("{n}"))
}

const PHP85: &str = include_str!("methods_php85.txt");
const LENGTHS: &str = include_str!("lengths_php85.txt");

#[test]
fn methods_match_php85() {
    let lines: Vec<&str> = PHP85.lines().collect();
    assert_eq!(methods(false).join(","), lines[0]);
    assert_eq!(methods(true).join(","), lines[2]);
}

#[test]
fn lengths_match_php85() {
    for l in LENGTHS.lines() {
        let f: Vec<&str> = l.split(' ').collect();
        let c = info(f[0]);
        assert_eq!(c.iv_len().to_string(), f[1], "{}", f[0]);
        assert_eq!(c.key_len().to_string(), f[2], "{}", f[0]);
    }
}

#[test]
fn name_rules() {
    assert_eq!(info("AES-128-CBC").name(), "aes-128-cbc");
    assert_eq!(info("aes-128-cbc  ").name(), "aes-128-cbc");
    assert_eq!(
        lookup_bytes(b"aes-128-cbc\0zz").unwrap().name(),
        "aes-128-cbc"
    );
    assert!(lookup(" aes-128-cbc").is_none());
    assert!(lookup("aes-128-cbc\t").is_none());
    assert!(lookup("").is_none());
    assert_eq!(info("Id-Aes256-GCM").name(), "aes-256-gcm");
    assert_eq!(info("2.16.840.1.101.3.4.1.42").name(), "aes-256-cbc");
    assert_eq!(by_oid("1.2.840.113549.3.7").unwrap().name(), "des-ede3-cbc");
    assert_eq!(info("des3").oid(), Some("1.2.840.113549.3.7"));
    assert!(info("bf-ecb").is_legacy());
    assert_eq!(info("des").name(), "des-cbc");
    for n in ["aes-128-cbc", "aes-192-cbc", "aes-256-cbc", "des-ede3-cbc"] {
        assert_eq!(by_oid(info(n).oid().unwrap()).unwrap().name(), n);
    }
}

#[test]
fn all_listed_roundtrip() {
    for name in methods(false) {
        let c = info(name);
        let key: Vec<u8> = (0..c.key_len() as u8)
            .map(|i| i.wrapping_mul(7) + 1)
            .collect();
        let iv: Vec<u8> = (0..c.iv_len() as u8)
            .map(|i| i.wrapping_mul(7) + 4)
            .collect();
        for n in [16usize, 32, 48, 64] {
            let pt: Vec<u8> = (0..n as u8).map(|i| i ^ 0x5c).collect();
            if c.is_aead() {
                let (ct, tag) = aead_encrypt(c, &key, &iv, b"aad", &pt, 16).unwrap();
                let back = aead_decrypt(c, &key, &iv, b"aad", &ct, Some(&tag)).unwrap();
                assert_eq!(back, pt, "{name}");
                let mut bad = tag.clone();
                bad[0] ^= 1;
                assert!(aead_decrypt(c, &key, &iv, b"aad", &ct, Some(&bad)).is_err());
                continue;
            }
            let ct = encrypt(c, &key, &iv, &pt, true).unwrap();
            if matches!(c.mode(), Mode::Siv | Mode::GcmSiv) {
                assert_eq!(ct.len(), pt.len());
                continue;
            }
            let back = decrypt(c, &key, &iv, &ct, true).unwrap();
            assert_eq!(back, pt, "{name}");
        }
    }
}

// NIST SP 800-38A Appendix F (AES-128).
const K38A: &str = "2b7e151628aed2a6abf7158809cf4f3c";
const P38A: &str = "6bc1bee22e409f96e93d7e117393172a ae2d8a571e03ac9c9eb76fac45af8e51
                    30c81c46a35ce411e5fbc1191a0a52ef f69f2445df4f9b17ad2b417be66c3710";
const IV38A: &str = "000102030405060708090a0b0c0d0e0f";

fn nist(name: &str, iv: &str, pt: &[u8], ct: &str) {
    let c = info(name);
    let out = encrypt(c, &h(K38A), &h(iv), pt, false).unwrap();
    assert_eq!(out, h(ct), "{name}");
    assert_eq!(decrypt(c, &h(K38A), &h(iv), &out, false).unwrap(), pt);
}

#[test]
fn sp800_38a() {
    let p = h(P38A);
    nist(
        "aes-128-ecb",
        "",
        &p,
        "3ad77bb40d7a3660a89ecaf32466ef97 f5d3d58503b9699de785895a96fdbaaf
         43b1cd7f598ece23881b00e3ed030688 7b0c785e27e8ad3f8223207104725dd4",
    );
    nist(
        "aes-128-cbc",
        IV38A,
        &p,
        "7649abac8119b246cee98e9b12e9197d 5086cb9b507219ee95db113a917678b2
         73bed6b8e3c1743b7116e69e22229516 3ff1caa1681fac09120eca307586e1a7",
    );
    nist(
        "aes-128-cfb",
        IV38A,
        &p,
        "3b3fd92eb72dad20333449f8e83cfb4a c8a64537a0b3a93fcde3cdad9f1ce58b
         26751f67a3cbb140b1808cf187a4f4df c04b05357c5d1c0eeac4c66f9ff7f2e6",
    );
    nist(
        "aes-128-ofb",
        IV38A,
        &p,
        "3b3fd92eb72dad20333449f8e83cfb4a 7789508d16918f03f53c52dac54ed825
         9740051e9c5fecf64344f7a82260edcc 304c6528f659c77866a510d9c1d6ae5e",
    );
    nist(
        "aes-128-ctr",
        "f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff",
        &p,
        "874d6191b620e3261bef6864990db6ce 9806f66b7970fdff8617187bb9fffdff
         5ae4df3edbd5d35e5b4f09020db03eab 1e031dda2fbe03d1792170a0f3009cee",
    );
    nist(
        "aes-128-cfb8",
        IV38A,
        &p[..18],
        "3b79424c9c0dd436bace9e0ed4586a4f32b9",
    );
    // F.3.1 CFB1: the first 16 bits 0110101111000001 encrypt to
    // 0110100010110011.
    nist("aes-128-cfb1", IV38A, &p[..2], "68b3");
}

#[test]
fn padding() {
    let c = info("aes-128-cbc");
    let k = [1u8; 16];
    let iv = [2u8; 16];
    for n in 0..40 {
        let pt = vec![0xaa; n];
        let ct = encrypt(c, &k, &iv, &pt, true).unwrap();
        assert_eq!(ct.len(), (n / 16 + 1) * 16);
        assert_eq!(decrypt(c, &k, &iv, &ct, true).unwrap(), pt);
    }
    assert_eq!(
        encrypt(c, &k, &iv, b"abc", false),
        Err(CipherError::WrongFinalBlockLength)
    );
    assert_eq!(
        decrypt(c, &k, &iv, b"", true),
        Err(CipherError::WrongFinalBlockLength)
    );
    assert_eq!(
        decrypt(c, &k, &iv, &[0; 17], true),
        Err(CipherError::WrongFinalBlockLength)
    );
    // A block whose padding is wrong.
    let ct = encrypt(c, &k, &iv, &[17u8; 16], false).unwrap();
    assert_eq!(decrypt(c, &k, &iv, &ct, true), Err(CipherError::BadDecrypt));
    assert!(unpad(&[1, 2, 3, 4, 3, 4, 4, 4], 4).is_none());
    assert_eq!(unpad(&[1, 2, 3, 4, 4, 4, 4, 4], 4), Some(4));
    assert!(unpad(&[1, 2, 3, 0], 4).is_none());
    assert!(unpad(&[1, 2, 3, 5], 4).is_none());
}

#[test]
fn cts_cs1() {
    // Observed on PHP 8.5: aes-128-cbc-cts keeps the CBC order and truncates
    // the next-to-last block (CS1); whole blocks are plain CBC.
    let c = info("aes-128-cbc-cts");
    let cbc = info("aes-128-cbc");
    let k = [3u8; 16];
    let iv = [4u8; 16];
    let pt: Vec<u8> = (0..37u8).collect();
    let ct = encrypt(c, &k, &iv, &pt, true).unwrap();
    let mut padded = pt.clone();
    padded.resize(48, 0);
    let full = encrypt(cbc, &k, &iv, &padded, false).unwrap();
    assert_eq!(&ct[..16], &full[..16]);
    assert_eq!(&ct[16..21], &full[16..21]);
    assert_eq!(&ct[21..], &full[32..]);
    for n in 16..70 {
        let pt: Vec<u8> = (0..n as u8).collect();
        let ct = encrypt(c, &k, &iv, &pt, true).unwrap();
        assert_eq!(ct.len(), n);
        assert_eq!(decrypt(c, &k, &iv, &ct, true).unwrap(), pt);
    }
    assert_eq!(
        encrypt(c, &k, &iv, b"short", true),
        Err(CipherError::Silent)
    );
}

#[test]
fn bit_and_byte_cfb_roundtrip() {
    for name in [
        "aes-192-cfb1",
        "des-ede3-cfb1",
        "camellia-128-cfb8",
        "des-ede3-cfb8",
    ] {
        let c = info(name);
        let k = vec![9u8; c.key_len()];
        let iv = vec![8u8; c.iv_len()];
        let pt: Vec<u8> = (0..33u8).collect();
        let ct = encrypt(c, &k, &iv, &pt, true).unwrap();
        assert_eq!(decrypt(c, &k, &iv, &ct, true).unwrap(), pt);
    }
}

#[test]
fn gcm_sp800_38d() {
    // GCM test case 4 (the McGrew–Viega submission, reproduced in
    // ext/openssl/tests/cipher_tests.inc).
    let c = info("aes-128-gcm");
    let key = h("feffe9928665731c6d6a8f9467308308");
    let iv = h("cafebabefacedbaddecaf888");
    let aad = h("feedfacedeadbeeffeedfacedeadbeefabaddad2");
    let pt = h(
        "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72
                1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
    );
    let (ct, tag) = aead_encrypt(c, &key, &iv, &aad, &pt, 16).unwrap();
    assert_eq!(
        ct,
        h(
            "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e
           21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091"
        )
    );
    assert_eq!(tag, h("5bc94fbc3221a5db94fae95ae7121a47"));
    for l in 1..=16 {
        let (_, t) = aead_encrypt(c, &key, &iv, &aad, &pt, l).unwrap();
        assert_eq!(t, tag[..l]);
        assert_eq!(aead_decrypt(c, &key, &iv, &aad, &ct, Some(&t)).unwrap(), pt);
        let mut bad = t.clone();
        bad[l - 1] ^= 0x80;
        assert_eq!(
            aead_decrypt(c, &key, &iv, &aad, &ct, Some(&bad)),
            Err(CipherError::TagMismatch)
        );
    }
    assert_eq!(
        aead_decrypt(c, &key, &iv, &aad, &ct, None),
        Err(CipherError::TagMismatch)
    );
    assert_eq!(
        aead_encrypt(c, &key, &iv, &aad, &pt, 17),
        Err(CipherError::InvalidTagLength)
    );
}

#[test]
fn ccm_sp800_38c() {
    // NIST SP 800-38C Appendix C, examples 1 and 2.
    let c = info("aes-128-ccm");
    let key = h("404142434445464748494a4b4c4d4e4f");
    let (ct, tag) = aead_encrypt(
        c,
        &key,
        &h("10111213141516"),
        &h("0001020304050607"),
        &h("20212223"),
        4,
    )
    .unwrap();
    assert_eq!(ct, h("7162015b"));
    assert_eq!(tag, h("4dac255d"));
    let (ct, tag) = aead_encrypt(
        c,
        &key,
        &h("1011121314151617"),
        &h("000102030405060708090a0b0c0d0e0f"),
        &h("202122232425262728292a2b2c2d2e2f"),
        6,
    )
    .unwrap();
    assert_eq!(ct, h("d2a1f0e051ea5f62081a7792073d593d"));
    assert_eq!(tag, h("1fc64fbfaccd"));
    let back = aead_decrypt(
        c,
        &key,
        &h("1011121314151617"),
        &h("000102030405060708090a0b0c0d0e0f"),
        &ct,
        Some(&tag),
    )
    .unwrap();
    assert_eq!(back, h("202122232425262728292a2b2c2d2e2f"));
    assert!(!c.tag_len_ok(5));
    assert!(!c.aead_iv_ok(6));
    assert!(!c.aead_iv_ok(14));
}

#[test]
fn ocb_rfc7253() {
    // RFC 7253 Appendix A, the first sample results.
    let c = info("aes-128-ocb");
    let key = h("000102030405060708090A0B0C0D0E0F");
    let cases = [
        (
            "BBAA99887766554433221100",
            "",
            "",
            "",
            "785407BFFFC8AD9EDCC5520AC9111EE6",
        ),
        (
            "BBAA99887766554433221101",
            "0001020304050607",
            "0001020304050607",
            "6820B3657B6F615A",
            "5725BDA0D3B4EB3A257C9AF1F8F03009",
        ),
        (
            "BBAA99887766554433221102",
            "0001020304050607",
            "",
            "",
            "81017F8203F081277152FADE694A0A00",
        ),
        (
            "BBAA99887766554433221103",
            "",
            "0001020304050607",
            "45DD69F8F5AAE724",
            "14054CD1F35D82760B2CD00D2F99BFA9",
        ),
        (
            "BBAA99887766554433221104",
            "000102030405060708090A0B0C0D0E0F",
            "000102030405060708090A0B0C0D0E0F",
            "571D535B60B277188BE5147170A9A22C",
            "3AD7A4FF3835B8C5701C1CCEC8FC3358",
        ),
    ];
    for (n, a, p, ct, tag) in cases {
        let (out, t) = aead_encrypt(c, &key, &h(n), &h(a), &h(p), 16).unwrap();
        assert_eq!(out, h(ct), "{n}");
        assert_eq!(t, h(tag), "{n}");
        assert_eq!(
            aead_decrypt(c, &key, &h(n), &h(a), &out, Some(&t)).unwrap(),
            h(p)
        );
    }
    // Longer inputs (several batches, partial blocks) round-trip.
    for len in [0usize, 15, 16, 17, 500, 1100] {
        let pt: Vec<u8> = (0..len).map(|i| i as u8).collect();
        let aad: Vec<u8> = (0..len / 3).map(|i| (i * 3) as u8).collect();
        for tl in [1usize, 8, 12, 16] {
            let (ct, t) = aead_encrypt(c, &key, &h("0102030405"), &aad, &pt, tl).unwrap();
            assert_eq!(t.len(), tl);
            assert_eq!(
                aead_decrypt(c, &key, &h("0102030405"), &aad, &ct, Some(&t)).unwrap(),
                pt
            );
        }
    }
}

#[test]
fn chacha20_poly1305_rfc8439() {
    // RFC 8439 §2.8.2.
    let c = info("chacha20-poly1305");
    let key = h("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
    let nonce = h("070000004041424344454647");
    let aad = h("50515253c0c1c2c3c4c5c6c7");
    let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let (ct, tag) = aead_encrypt(c, &key, &nonce, &aad, pt, 16).unwrap();
    assert_eq!(&ct[..16], &h("d31a8d34648e60db7b86afbc53ef7ec2")[..]);
    assert_eq!(tag, h("1ae10b594f09e26a7e902ecbd0600691"));
    for l in [1usize, 5, 15] {
        assert_eq!(
            aead_decrypt(c, &key, &nonce, &aad, &ct, Some(&tag[..l])).unwrap(),
            pt
        );
        let mut bad = tag[..l].to_vec();
        bad[0] ^= 1;
        assert!(aead_decrypt(c, &key, &nonce, &aad, &ct, Some(&bad)).is_err());
    }
    // Observed on PHP 8.5: without a tag the ciphertext decrypts unverified.
    assert_eq!(aead_decrypt(c, &key, &nonce, &aad, &ct, None).unwrap(), pt);
}

#[test]
fn chacha20_rfc8439() {
    // RFC 8439 §2.4.2: counter 1, nonce 000000000000004a00000000.
    let c = info("chacha20");
    let key = h("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
    let iv = h("01000000 000000000000004a00000000");
    let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let ct = encrypt(c, &key, &iv, pt, true).unwrap();
    assert_eq!(&ct[..16], &h("6e2e359a2568f98041ba0728dd0d6981")[..]);
}

#[test]
fn key_wrap_rfc3394_rfc5649() {
    // RFC 3394 §4.1 (default IV A6A6A6A6A6A6A6A6).
    let c = info("aes-128-wrap");
    let kek = h("000102030405060708090A0B0C0D0E0F");
    let iv = h("A6A6A6A6A6A6A6A6");
    let ct = encrypt(c, &kek, &iv, &h("00112233445566778899AABBCCDDEEFF"), true).unwrap();
    assert_eq!(ct, h("1FA68B0A8112B447AEF34BD8FB5A7B829D3E862371D2CFE5"));
    assert_eq!(
        decrypt(c, &kek, &iv, &ct, true).unwrap(),
        h("00112233445566778899AABBCCDDEEFF")
    );
    let mut bad = ct.clone();
    bad[3] ^= 1;
    assert_eq!(
        decrypt(c, &kek, &iv, &bad, true),
        Err(CipherError::KwFailed)
    );
    assert_eq!(
        encrypt(c, &kek, &iv, &[0; 8], true),
        Err(CipherError::KwFailed)
    );
    assert_eq!(
        encrypt(c, &kek, &iv, &[0; 9], true),
        Err(CipherError::KwInvalidLength)
    );
    // RFC 5649 §6 (AIV high half A65959A6).
    let c = info("aes-192-wrap-pad");
    let kek = h("5840df6e29b02af1ab493b705bf16ea1ae8338f4dcc176a8");
    let aiv = h("A65959A6");
    let k20 = h("c37b7e6492584340bed12207808941155068f738");
    let ct = encrypt(c, &kek, &aiv, &k20, true).unwrap();
    assert_eq!(
        ct,
        h("138bdeaa9b8fa7fc61f97742e72248ee5ae6ae5360d1ae6a5f54f373fa543b6a")
    );
    assert_eq!(decrypt(c, &kek, &aiv, &ct, true).unwrap(), k20);
    let ct = encrypt(c, &kek, &aiv, &h("466f7250617369"), true).unwrap();
    assert_eq!(ct, h("afbeb0f07dfbf5419200f2ccb50bb24f"));
    assert_eq!(
        decrypt(c, &kek, &aiv, &ct, true).unwrap(),
        h("466f7250617369")
    );
    // Inverse-cipher variants round-trip.
    for n in ["aes-256-wrap-inv", "aes-128-wrap-pad-inv"] {
        let c = info(n);
        let k = vec![1u8; c.key_len()];
        let iv = vec![2u8; c.iv_len()];
        for len in [1usize, 8, 16, 40] {
            if c.mode() == Mode::WrapInv && len % 8 != 0 || len < 16 && c.mode() == Mode::WrapInv {
                continue;
            }
            let pt = vec![7u8; len];
            let ct = encrypt(c, &k, &iv, &pt, true).unwrap();
            assert_eq!(decrypt(c, &k, &iv, &ct, true).unwrap(), pt, "{n}");
        }
    }
}

#[test]
fn des3_wrap_rfc3217() {
    let c = info("des3-wrap");
    let kek: Vec<u8> = (0..24u8).collect();
    for len in [8usize, 16, 24] {
        let cek = vec![0x42u8; len];
        let ct = encrypt(c, &kek, b"", &cek, true).unwrap();
        assert_eq!(ct.len(), len + 16);
        assert_eq!(decrypt(c, &kek, b"", &ct, true).unwrap(), cek);
        let mut bad = ct.clone();
        bad[ct.len() - 1] ^= 1;
        assert_eq!(
            decrypt(c, &kek, b"", &bad, true),
            Err(CipherError::OperationFailed)
        );
    }
}

#[test]
fn xts_keys() {
    let c = info("aes-128-xts");
    let same = [5u8; 32];
    let iv = [0u8; 16];
    assert_eq!(
        encrypt(c, &same, &iv, &[0; 16], true),
        Err(CipherError::XtsDuplicatedKeys)
    );
    assert!(decrypt(c, &same, &iv, &[0; 16], true).is_ok());
    let mut k = [5u8; 32];
    k[31] = 6;
    assert_eq!(
        encrypt(c, &k, &iv, &[0; 15], true),
        Err(CipherError::OperationFailed)
    );
    let pt: Vec<u8> = (0..37u8).collect();
    let ct = encrypt(c, &k, &iv, &pt, true).unwrap();
    assert_eq!(decrypt(c, &k, &iv, &ct, true).unwrap(), pt);
}

#[test]
fn legacy_unsupported() {
    let c = info("bf-cbc");
    assert_eq!(
        encrypt(c, &[0; 16], &[0; 8], b"x", true),
        Err(CipherError::Unsupported)
    );
    // Single DES runs (FIPS 81 / the classic "Now is the time for all "
    // ECB example: key 0123456789abcdef).
    let d = info("des-ecb");
    let ct = encrypt(d, &h("0123456789abcdef"), b"", b"Now is t", false).unwrap();
    assert_eq!(ct, h("3fa40e8a984d4815"));
}

/// The probe scripts' byte pattern: byte i is i*7+1+s.
fn pattern(n: usize, s: usize) -> Vec<u8> {
    (0..n).map(|i| ((i * 7 + 1 + s) & 255) as u8).collect()
}

#[test]
fn sm4_xts_gb_php85() {
    // Observed on PHP 8.5: openssl_encrypt() with sm4-xts, key pattern(32),
    // IV pattern(16, 3), plaintext pattern(n, 11).
    let c = info("sm4-xts");
    let key = pattern(32, 0);
    let iv = pattern(16, 3);
    for (n, want) in [
        (17, "10d0d7844c19a76a85abfe3ccaf70b101a"),
        (
            37,
            "1af7b8b455425620fffed8ae41271dc9788a7c4d52092ee4670f0fb5b646493e5c6661c057",
        ),
        (
            64,
            "1af7b8b455425620fffed8ae41271dc95c6661c0578d69be5e981330eddfe451\
             ecd54072e80e1b545e2dd8c5466cd7a79ec6f07c35fbe88106374b5d5d70427c",
        ),
    ] {
        let pt = pattern(n, 11);
        let ct = encrypt(c, &key, &iv, &pt, true).unwrap();
        assert_eq!(ct, h(want), "{n}");
        assert_eq!(decrypt(c, &key, &iv, &ct, true).unwrap(), pt);
    }
}
