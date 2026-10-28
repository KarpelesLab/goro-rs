//! Published test vectors of the algorithms, through the PHP-level
//! functions. AEGIS in particular has no reference output here (the PHP 8.5
//! reference build's libsodium 1.0.18 predates it), so its draft's vectors
//! are the check.

use goro_ext_sodium::*;

fn unhex(s: &str) -> Vec<u8> {
    sodium_hex2bin(s.as_bytes(), b" \n").unwrap()
}

fn hex(b: &[u8]) -> String {
    String::from_utf8(sodium_bin2hex(b)).unwrap()
}

#[test]
fn aegis_draft_vectors() {
    // draft-irtf-cfrg-aegis-aead, appendix A: AEGIS-128L test vectors 1
    // and 2, AEGIS-256 test vector 1 (256-bit tags, as libsodium uses).
    let key = unhex("10010000000000000000000000000000");
    let nonce = unhex("10000200000000000000000000000000");
    let c = sodium_crypto_aead_aegis128l_encrypt(&[0; 16], b"", &nonce, &key).unwrap();
    assert_eq!(
        hex(&c),
        "c1c0e58bd913006feba00f4b3cc3594e25835bfbb21632176cf03840687cb968cace4617af1bd0f7d064c639a5c79ee4"
    );
    assert_eq!(
        sodium_crypto_aead_aegis128l_decrypt(&c, b"", &nonce, &key)
            .unwrap()
            .unwrap(),
        vec![0; 16]
    );
    let c = sodium_crypto_aead_aegis128l_encrypt(b"", b"", &nonce, &key).unwrap();
    assert_eq!(
        hex(&c),
        "1360dc9db8ae42455f6e5b6a9d488ea4f2184c4e12120249335c4ee84bafe25d"
    );
    let key = unhex("1001000000000000000000000000000000000000000000000000000000000000");
    let nonce = unhex("1000020000000000000000000000000000000000000000000000000000000000");
    let c = sodium_crypto_aead_aegis256_encrypt(&[0; 16], b"", &nonce, &key).unwrap();
    assert_eq!(
        hex(&c),
        "754fc3d8c973246dcc6d741412a4b2361181a1d18091082bf0266f66297d167d2e68b845f61a3b0527d31fc7b7b89f13"
    );
    let mut bad = c.clone();
    bad[40] ^= 1;
    assert_eq!(
        sodium_crypto_aead_aegis256_decrypt(&bad, b"", &nonce, &key).unwrap(),
        None
    );
    assert_eq!(
        sodium_crypto_aead_aegis256_decrypt(&c[..31], b"", &nonce, &key).unwrap(),
        None
    );
    // Argument checks, as for the other AEADs.
    let e = sodium_crypto_aead_aegis128l_encrypt(b"", b"", &[0; 15], &[0; 16]).unwrap_err();
    assert_eq!(
        e,
        SodiumError::Exception(
            "sodium_crypto_aead_aegis128l_encrypt(): Argument #3 ($nonce) must be SODIUM_CRYPTO_AEAD_AEGIS128L_NPUBBYTES bytes long".into()
        )
    );
    let e = sodium_crypto_aead_aegis256_decrypt(b"", b"", &[0; 32], &[0; 31]).unwrap_err();
    assert_eq!(
        e,
        SodiumError::Exception(
            "sodium_crypto_aead_aegis256_decrypt(): Argument #4 ($key) must be SODIUM_CRYPTO_AEAD_AEGIS256_KEYBYTES bytes long".into()
        )
    );
    assert_eq!(sodium_crypto_aead_aegis128l_keygen().unwrap().len(), 16);
    assert_eq!(sodium_crypto_aead_aegis256_keygen().unwrap().len(), 32);
}

const SUNSCREEN: &[u8] = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";

#[test]
fn chacha20poly1305_ietf_rfc8439() {
    // RFC 8439 §2.8.2.
    let key = unhex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
    let nonce = unhex("070000004041424344454647");
    let ad = unhex("50515253c0c1c2c3c4c5c6c7");
    let c = sodium_crypto_aead_chacha20poly1305_ietf_encrypt(SUNSCREEN, &ad, &nonce, &key).unwrap();
    assert_eq!(
        hex(&c),
        "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b61161ae10b594f09e26a7e902ecbd0600691"
    );
}

#[test]
fn xchacha20poly1305_draft() {
    // draft-irtf-cfrg-xchacha-03 §A.3.1.
    let key = unhex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
    let nonce = unhex("404142434445464748494a4b4c4d4e4f5051525354555657");
    let ad = unhex("50515253c0c1c2c3c4c5c6c7");
    let c =
        sodium_crypto_aead_xchacha20poly1305_ietf_encrypt(SUNSCREEN, &ad, &nonce, &key).unwrap();
    assert_eq!(
        hex(&c),
        "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b4522f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff921f9664c97637da9768812f615c68b13b52ec0875924c1c7987947deafd8780acf49"
    );
}

#[test]
fn ed25519_rfc8032() {
    // RFC 8032 §7.1, tests 1 and 2.
    for (seed, pk, msg, sig) in [
        (
            "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
            "",
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        ),
        (
            "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
            "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
            "72",
            "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
        ),
    ] {
        let kp = sodium_crypto_sign_seed_keypair(&unhex(seed)).unwrap();
        assert_eq!(hex(&sodium_crypto_sign_publickey(&kp).unwrap()), pk);
        let sk = sodium_crypto_sign_secretkey(&kp).unwrap();
        let s = sodium_crypto_sign_detached(&unhex(msg), &sk).unwrap();
        assert_eq!(hex(&s), sig);
        assert!(sodium_crypto_sign_verify_detached(&s, &unhex(msg), &unhex(pk)).unwrap());
    }
}

#[test]
fn blake2b_keyed_kat() {
    // BLAKE2 reference known-answer test: BLAKE2b-512, key 00..3f, empty input.
    let key: Vec<u8> = (0..64).collect();
    assert_eq!(
        hex(&sodium_crypto_generichash(b"", &key, 64).unwrap()),
        "10ebb67700b1868efb4417987acf4690ae9d972fb7a590c2f02871799aaa4786b5e996e8f0f4eb981fc214b005f42d2ff4233499391653df7aefcbc13fc51568"
    );
}

#[test]
fn siphash24_paper() {
    // SipHash paper, appendix A: key 00..0f, message 00..0e, result
    // 0xa129ca6149be45e5 (little-endian output).
    let key: Vec<u8> = (0..16).collect();
    let msg: Vec<u8> = (0..15).collect();
    assert_eq!(
        hex(&sodium_crypto_shorthash(&msg, &key).unwrap()),
        "e545be4961ca29a1"
    );
}

#[test]
fn x25519_nacl_box() {
    // RFC 7748 §6.1 key pair; crypto_box of the two parties agrees.
    let alice_sk = unhex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
    let bob_sk = unhex("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");
    let alice_pk = sodium_crypto_box_publickey_from_secretkey(&alice_sk).unwrap();
    let bob_pk = sodium_crypto_scalarmult_base(&bob_sk).unwrap();
    assert_eq!(
        hex(&alice_pk),
        "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
    );
    assert_eq!(
        hex(&bob_pk),
        "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f"
    );
    let ab = sodium_crypto_box_keypair_from_secretkey_and_publickey(&alice_sk, &bob_pk).unwrap();
    let ba = sodium_crypto_box_keypair_from_secretkey_and_publickey(&bob_sk, &alice_pk).unwrap();
    let nonce = [7u8; 24];
    let c = sodium_crypto_box(b"hello bob", &nonce, &ab).unwrap();
    assert_eq!(
        sodium_crypto_box_open(&c, &nonce, &ba).unwrap().unwrap(),
        b"hello bob"
    );
    let sealed = sodium_crypto_box_seal(b"anonymous", &bob_pk).unwrap();
    let bob_kp = sodium_crypto_box_keypair_from_secretkey_and_publickey(&bob_sk, &bob_pk).unwrap();
    assert_eq!(
        sodium_crypto_box_seal_open(&sealed, &bob_kp)
            .unwrap()
            .unwrap(),
        b"anonymous"
    );
}

#[test]
fn secretstream_roundtrip_with_rekey() {
    use goro_runtime::{PhpStr, Value};
    let key = sodium_crypto_secretstream_xchacha20poly1305_keygen().unwrap();
    let (st, header) = sodium_crypto_secretstream_xchacha20poly1305_init_push(&key).unwrap();
    let mut push = Value::Str(PhpStr::from(st));
    let mut pull = Value::Str(PhpStr::from(
        sodium_crypto_secretstream_xchacha20poly1305_init_pull(&header, &key).unwrap(),
    ));
    for (i, tag) in [0i64, 2, 1, 0, 2, 3].into_iter().enumerate() {
        let msg = vec![i as u8; i * 37];
        let c =
            sodium_crypto_secretstream_xchacha20poly1305_push(&mut push, &msg, b"ad", tag).unwrap();
        let (m, t) = sodium_crypto_secretstream_xchacha20poly1305_pull(&mut pull, &c, b"ad")
            .unwrap()
            .unwrap();
        assert_eq!((m, t), (msg, tag));
        if i == 3 {
            sodium_crypto_secretstream_xchacha20poly1305_rekey(&mut push).unwrap();
            sodium_crypto_secretstream_xchacha20poly1305_rekey(&mut pull).unwrap();
        }
    }
}

#[test]
fn string_length_cap() {
    set_max_string_len(100);
    let n = [0u8; 24];
    let k = [0u8; 32];
    assert_eq!(
        sodium_crypto_stream(101, &n, &k),
        Err(SodiumError::OutOfMemory(101))
    );
    assert_eq!(sodium_crypto_stream(100, &n, &k).unwrap().len(), 100);
    assert_eq!(sodium_pad(b"abc", 128), Err(SodiumError::OutOfMemory(128)));
    assert_eq!(
        sodium_pad(b"", i64::MAX),
        Err(SodiumError::OutOfMemory(i64::MAX as usize))
    );
    set_max_string_len(usize::MAX);
}

#[test]
fn argon2_for_password_hash() {
    // Several lanes, as password_hash() allows (`threads`).
    let h = argon2_password_hash(true, b"secret", 64, 2, 3).unwrap();
    assert!(h.starts_with(b"$argon2id$v=19$m=64,t=2,p=3$"));
    assert_eq!(argon2_string_params(&h), Some(("argon2id", 64, 2, 3)));
    assert!(argon2_password_verify(&h, b"secret"));
    assert!(!argon2_password_verify(&h, b"Secret"));
    let h = argon2_password_hash(false, b"", 8, 1, 1).unwrap();
    assert_eq!(argon2_string_params(&h), Some(("argon2i", 8, 1, 1)));
    assert!(argon2_password_verify(&h, b""));
    // Fewer than 8 KiB per lane is rejected by Argon2.
    assert!(argon2_password_hash(true, b"x", 15, 1, 2).is_none());
}
