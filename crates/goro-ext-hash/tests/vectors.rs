//! Published test vectors of the algorithms' specifications, and the
//! input/digest pairs and behaviours of php-src's ext/hash tests.

use goro_ext_hash::{
    Diag, HASH_HMAC, HashContext, HashError, HashOptions, Hasher, SECRET_DEPRECATION, Seed, SerVal,
    StateSlot, algos, digest, hash, hash_equals, hash_hkdf, hash_hmac, hash_pbkdf2, hmac,
    hmac_algos, lookup, to_hex,
};

#[path = "data/phpt_vectors.rs"]
mod phpt_vectors_data;

fn hx(b: &[u8]) -> String {
    String::from_utf8(to_hex(b)).unwrap()
}

fn h(algo: &str, data: &[u8]) -> String {
    hx(&hash(
        algo.as_bytes(),
        data,
        &HashOptions::default(),
        &mut Vec::new(),
    )
    .unwrap())
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Every digest, through `hash()` and through a context fed one byte at
/// a time.
fn check(algo: &str, data: &[u8], want: &str) {
    assert_eq!(
        h(algo, data),
        want,
        "{algo} of {:?}",
        String::from_utf8_lossy(&data[..data.len().min(40)])
    );
    if data.len() <= 4096 {
        let mut c = HashContext::new(
            algo.as_bytes(),
            0,
            None,
            &HashOptions::default(),
            &mut Vec::new(),
        )
        .unwrap();
        for b in data {
            c.update(std::slice::from_ref(b));
        }
        assert_eq!(hx(&c.finalize().unwrap()), want, "{algo} bytewise");
    }
}

const MILLION_A: usize = 1_000_000;

#[test]
fn phpt_vectors() {
    for (file, algo, parts, want) in phpt_vectors_data::VECTORS {
        let mut data = Vec::new();
        for (p, n) in parts.iter() {
            for _ in 0..*n {
                data.extend_from_slice(p);
            }
        }
        assert_eq!(h(algo, &data), *want, "{file}: {algo}");
    }
}

/// RFC 1319, 1320, 1321 appendix A.5 test suites.
#[test]
fn md_rfc() {
    let inputs: [&[u8]; 7] = [
        b"",
        b"a",
        b"abc",
        b"message digest",
        b"abcdefghijklmnopqrstuvwxyz",
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
        b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
    ];
    let md2 = [
        "8350e5a3e24c153df2275c9f80692773",
        "32ec01ec4a6dac72c0ab96fb34c0b5d1",
        "da853b0d3f88d99b30283a69e6ded6bb",
        "ab4f496bfb2a530b219ff33031fe06b0",
        "4e8ddff3650292ab5a4108c3aa47940b",
        "da33def2a42df13975352846c30338cd",
        "d5976f79d83d3a0dc9806c3c66f3efd8",
    ];
    let md4 = [
        "31d6cfe0d16ae931b73c59d7e0c089c0",
        "bde52cb31de33e46245e05fbdbd6fb24",
        "a448017aaf21d8525fc10ae87aa6729d",
        "d9130a8164549fe818874806e1c7014b",
        "d79e1c308aa5bbcdeea8ed63df412da9",
        "043f8582f241db351ce627e153e7f0e4",
        "e33b4ddc9c38f2199c3e7b164fcc0536",
    ];
    let md5 = [
        "d41d8cd98f00b204e9800998ecf8427e",
        "0cc175b9c0f1b6a831c399e269772661",
        "900150983cd24fb0d6963f7d28e17f72",
        "f96b697d7cb7938d525a2f31aaf161d0",
        "c3fcd3d76192e4007dfb496cca67e13b",
        "d174ab98d277d9f5a5611c2c9f419d9f",
        "57edf4a22be3c955ac49da2e2107b67a",
    ];
    for i in 0..7 {
        check("md2", inputs[i], md2[i]);
        check("md4", inputs[i], md4[i]);
        check("md5", inputs[i], md5[i]);
    }
}

/// FIPS 180-4 example vectors (NIST CSRC "Examples with intermediate
/// values"): "abc", the two-block messages and one million 'a'.
#[test]
fn sha2_fips() {
    let a = vec![b'a'; MILLION_A];
    let two = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    let two512 = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";
    check("sha1", b"abc", "a9993e364706816aba3e25717850c26c9cd0d89d");
    check("sha1", two, "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
    check("sha1", &a, "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
    check(
        "sha224",
        b"abc",
        "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7",
    );
    check(
        "sha224",
        two,
        "75388b16512776cc5dba5da1fd890150b0c6455cb4f58b1952522525",
    );
    check(
        "sha256",
        b"abc",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
    check(
        "sha256",
        two,
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
    );
    check(
        "sha256",
        &a,
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
    );
    check(
        "sha384",
        b"abc",
        "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7",
    );
    check(
        "sha384",
        two512,
        "09330c33f71147e83d192fc782cd1b4753111b173b3b05d22fa08086e3b0f712fcc7c71a557e2db966c3e9fa91746039",
    );
    check(
        "sha512",
        b"abc",
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
    );
    check(
        "sha512",
        two512,
        "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909",
    );
    check(
        "sha512/224",
        b"abc",
        "4634270f707b6a54daae7530460842e20e37ed265ceee9a43e8924aa",
    );
    check(
        "sha512/256",
        b"abc",
        "53048e2681941ef99b2e29b76b4c7dabe4c2d0c634fc6d46e0e2f13107e7af23",
    );
    check(
        "sha512",
        &a,
        "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973ebde0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b",
    );
}

/// FIPS 202 / NIST example values, plus test ext/hash/tests/sha3.phpt.
#[test]
fn sha3() {
    check(
        "sha3-224",
        b"",
        "6b4e03423667dbb73b6e15454f0eb1abd4597f9a1b078e3f5b5a6bc7",
    );
    check(
        "sha3-256",
        b"abc",
        "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532",
    );
    check(
        "sha3-384",
        b"abc",
        "ec01498288516fc926459f58e2c6ad8df9b473cb0fc08c2596da7cf0e49be4b298d88cea927ac7f539f1edf228376d25",
    );
    check(
        "sha3-512",
        b"abc",
        "b751850b1a57168a5693cd924b6b096e08f621827444f70d884f5d0240d2712e10e116e9192af3c91a7ec57647e3934057340b4cf408d5a56592f8274eec53f0",
    );
    let a257 = vec![b'a'; 257];
    check(
        "sha3-224",
        &a257,
        "10dd422d71c42ee102a0c4bd398b5b85470341a0794702c954b022ba",
    );
    check(
        "sha3-256",
        &a257,
        "6d115e8744deef792419e8bdb8567d74844e0fa5c2d5e474a19de87ac001449f",
    );
    let a3 = vec![0xa3; 200];
    check(
        "sha3-256",
        &a3,
        "79f38adec5c20307a98ef76e8324afbfd46cfd81b22e3973c65fa1bd9de31787",
    );
    check(
        "sha3-512",
        &a3,
        "e76dfad22084a8b1467fcf2ffa58361bec7628edf5f3fdc0e4805dc48caeeca81b7c13c30adf52a3659584739a2df46be589c51ca1a4a8416df6545a1ce8ba00",
    );
    check(
        "sha3-512",
        b"The quick brown fox jumps over the lazy dog.",
        "18f4f4bd419603f95538837003d9d254c26c23765565162247483f65c50303597bc9ce4d289f21d1c2f1f458828e33dc442100331b35e7eb031b5d38ba6460f8",
    );
}

/// The RIPEMD page's test vectors (homes.esat.kuleuven.be/~bosselae).
#[test]
fn ripemd_page() {
    let a = vec![b'a'; MILLION_A];
    check("ripemd128", b"", "cdf26213a150dc3ecb610f18f6b38b46");
    check(
        "ripemd128",
        b"message digest",
        "9e327b3d6e523062afc1132d7df9d1b8",
    );
    check("ripemd128", &a, "4a7f5723f954eba1216c9d8f6320431f");
    check(
        "ripemd160",
        b"abc",
        "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc",
    );
    check("ripemd160", &a, "52783243c1697bdbe16d37f97f68f08325dc1528");
    check(
        "ripemd256",
        b"a",
        "f9333e45d857f5d90a91bab70a1eba0cfb1be4b0783c9acfcd883a9134692925",
    );
    check(
        "ripemd256",
        &a,
        "ac953744e10e31514c150d4d8d7b677342e33399788296e43ae4850ce4f97978",
    );
    check(
        "ripemd320",
        b"abc",
        "de4c01b3054f8930a79d09ae738e92301e5a17085beffdc1b8d116713e74f82fa942d64cdbc4682d",
    );
    check(
        "ripemd320",
        &a,
        "bdee37f4371e20646b8b0d862dda16292ae36f40965e8c8509e63d1dbddecc503e2b63eb9245bb66",
    );
}

/// The Whirlpool reference's ISO test vectors, plus whirlpool.phpt.
#[test]
fn whirlpool() {
    check(
        "whirlpool",
        b"",
        "19fa61d75522a4669b44e39c1d2e1726c530232130d407f89afee0964997f7a73e83be698b288febcf88e3e03c4f0757ea8964e59b63d93708b138cc42a66eb3",
    );
    check(
        "whirlpool",
        b"abc",
        "4e2448a4c6f486bb16b6562c73b4020bf3043e3a731bce721ae1b303d97e6d4c7181eebdb6c57e277d0e34957114cbd6c797fc9d95d8b582d225292076d4eef5",
    );
    let s = b"---qwertzuiopasdfghjklyxcvbnm------qwertzuiopasdfghjklyxcvbnm---";
    check(
        "whirlpool",
        s,
        "916ce6431d2f384be68d96bcaba800c21b82e9cc2f07076554c9557f85476b5d8f2b263951121fa955e34b31a4cdc857bdf076b123c2252543dcef34f84a7ef3",
    );
    let mut big = Vec::new();
    for _ in 0..1000 {
        big.extend_from_slice(s);
        big.push(b'0');
    }
    check(
        "whirlpool",
        &big,
        "b51984710d11893ac08e10529519f9801d82ea534629d14bc8c810307934496017ccdf23bfcb62c7e1259664e84c9388ff646b0b46688b0a6c32e5571234dd95",
    );
}

/// The Tiger authors' test results (testresults.html), 3 passes, as
/// printed by PHP (test tiger.phpt).
#[test]
fn tiger() {
    check(
        "tiger192,3",
        b"",
        "3293ac630c13f0245f92bbb1766e16167a4e58492dde73f3",
    );
    check(
        "tiger192,3",
        b"abc",
        "2aab1484e8c158f2bfb8c5ff41b57a525129131c957b5f93",
    );
    check(
        "tiger192,3",
        b"Tiger",
        "dd00230799f5009fec6debc838bb6a27df2b9d6f110c7937",
    );
    check(
        "tiger192,3",
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ=abcdefghijklmnopqrstuvwxyz+0123456789",
        "48ceeb6308b87d46e95d656112cdf18d97915f9765658957",
    );
    check(
        "tiger192,3",
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+-",
        "f71c8583902afb879edfe610f82c0d4786a3a534504486b5",
    );
    check("tiger128,3", b"abc", "2aab1484e8c158f2bfb8c5ff41b57a52");
    check(
        "tiger160,3",
        b"abc",
        "2aab1484e8c158f2bfb8c5ff41b57a525129131c",
    );
}

/// Test ext/hash/tests/haval.phpt and bug70312.phpt.
#[test]
fn haval() {
    let long = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMOPQRSTUVWXYZ0123456789".repeat(3);
    let cases: [(&str, &str, &str, &str); 15] = [
        (
            "haval128,3",
            "c68f39913f901f3ddf44c707357a7d70",
            "9e40ed883fb63e985d299b40cda2b8f2",
            "ddf4304cc5ffa3db8aab60d4f8fc2a00",
        ),
        (
            "haval160,3",
            "d353c3ae22a25401d257643836d7231a9a95f953",
            "b21e876c4d391e2a897661149d83576b5530a089",
            "e709559359b15917623050e41d27a306c6c3a9db",
        ),
        (
            "haval192,3",
            "e9c48d7903eaf2a91c5b350151efcb175c0fc82de2289a4e",
            "a7b14c9ef3092319b0e75e3b20b957d180bf20745629e8de",
            "51e25280ad356c06f4b913b3cdb3abaaac5879dda0a4fea4",
        ),
        (
            "haval224,3",
            "c5aae9d47bffcaaf84a8c6e7ccacd60a0dd1932be7b1a192b9214b6d",
            "5bc955220ba2346a948d2848eca37bdd5eca6ecca7b594bd32923fab",
            "28aa2c164e10bb3076574cc8aa8584fd6d04f6d82c37ea5c21e451b3",
        ),
        (
            "haval256,3",
            "4f6938531f0bc8991f62da7bbd6f7de3fad44562b8c6f4ebf146d5b4e46f7c17",
            "8699f1e3384d05b2a84b032693e2b6f46df85a13a50d93808d6874bb8fb9e86c",
            "5537364e3d75174b846d21adf9b113f9d8f97e4750df64d428c01e782f9ade4d",
        ),
        (
            "haval128,4",
            "ee6bbf4d6a46a679b3a856c88538bb98",
            "6f2132867c9648419adcd5013e532fa2",
            "c7d981e8270e39888ba96cafe8745636",
        ),
        (
            "haval160,4",
            "1d33aae1be4146dbaaca0b6e70d7a11f10801525",
            "77aca22f5b12cc09010afc9c0797308638b1cb9b",
            "3444e38cc2a132b818b554ced8f7d9592df28f57",
        ),
        (
            "haval192,4",
            "4a8372945afa55c7dead800311272523ca19d42ea47b72da",
            "7e29881ed05c915903dd5e24a8e81cde5d910142ae66207c",
            "0ca58f140ed92828a27913ce5636611abcada220fccf3af7",
        ),
        (
            "haval224,4",
            "3e56243275b3b81561750550e36fcd676ad2f5dd9e15f2e89e6ed78e",
            "124c43d2ba4884599d013e8c872bfea4c88b0b6bf6303974cbe04e68",
            "a9d0571d0857773e71363e4e9dfcca4696dba3e5019e7225e65e0cb1",
        ),
        (
            "haval256,4",
            "c92b2e23091e80e375dadce26982482d197b1a2521be82da819f8ca2c579b99b",
            "8f409f1bb6b30c5016fdce55f652642261575bedca0b9533f32f5455459142b5",
            "1858d106bdc2fc787445364a163cfc6027597a45a58a2490d14203c8b9bdd268",
        ),
        (
            "haval128,5",
            "184b8482a0c050dca54b59c7f05bf5dd",
            "d054232fe874d9c6c6dc8e6a853519ea",
            "d41e927ea041d2f0c255352b1a9f6195",
        ),
        (
            "haval160,5",
            "255158cfc1eed1a7be7c55ddd64d9790415b933b",
            "ae646b04845e3351f00c5161d138940e1fa0c11c",
            "f3245e222e6581d0c3077bd7af322af4b4fedab7",
        ),
        (
            "haval192,5",
            "4839d0626f95935e17ee2fc4509387bbe2cc46cb382ffe85",
            "d12091104555b00119a8d07808a3380bf9e60018915b9025",
            "fc45dc17a7b19adfed2a6485921f7af7951d70703b9357c1",
        ),
        (
            "haval224,5",
            "4a0513c032754f5582a758d35917ac9adf3854219b39e3ac77d1837e",
            "8081027a500147c512e5f1055986674d746d92af4841abeb89da64ad",
            "29687958a6f0d54d495105df00dbda0153ee0f5708408db68a5bbea5",
        ),
        (
            "haval256,5",
            "be417bb4dd5cfb76c7126f4f8eeb1553a449039307b1a3cd451dbfdc0fbbe330",
            "976cd6254c337969e5913b158392a2921af16fca51f5601d486e0a9de01156e7",
            "f93421623f852ac877584d1e4bba5d9345a95f81bfd277fe36dfeed1815f83d5",
        ),
    ];
    for (algo, e, abc, l) in cases {
        check(algo, b"", e);
        check(algo, b"abc", abc);
        check(algo, &long, l);
    }
    let s = b"1234567890123456789012345678901234567890123456789012345678901234";
    check("haval128,5", s, "f3f0d23819b87228b4b70ee350afaa9d");
    check(
        "haval256,5",
        s,
        "fb73c19300b14d5cb393d929bf005e6c2d459a4c9c009e9813af1d2d3637ee8f",
    );
}

/// Check values of the CRCs ("123456789"), FNV-1/FNV-1a, xxHash and the
/// Adler-32 example of RFC 1950's definition.
#[test]
fn checksums() {
    check("crc32b", b"123456789", "cbf43926");
    check("crc32c", b"123456789", "e3069283");
    // The MSB-first CRC (check value 0xfc891918), printed low byte first.
    check("crc32", b"123456789", "181989fc");
    check("adler32", b"Wikipedia", "11e60398");
    check("fnv132", b"a", "050c5d7e");
    check("fnv1a32", b"a", "e40c292c");
    check("fnv164", b"a", "af63bd4c8601b7be");
    check("fnv1a64", b"a", "af63dc4c8601ec8c");
    check("xxh32", b"", "02cc5d05");
    check("xxh64", b"", "ef46db3751d8e999");
    check("xxh3", b"", "2d06800538d394c2");
    check("xxh128", b"", "99aa06d3014798d86001c324468d497f");
    check("joaat", b"hello world", "3e4a5a57");
}

/// Test ext/hash/tests/murmurhash3.phpt and murmurhash3_seed.phpt.
#[test]
fn murmur() {
    check("murmur3a", b"foo", "f6a5c420");
    check(
        "murmur3c",
        b"Two hashes meet in a bar",
        "8036c2707453c6f37348142be7eaf75c",
    );
    check("murmur3c", b"hash me!", "c7009299985a5627a9280372a9280372");
    check(
        "murmur3f",
        b"Two hashes meet in a bar",
        "40256ed26fa6ece7785092ed33c8b659",
    );
    check("murmur3f", b"hash me!", "c43668294e89db0ba5772846e5804467");
    check("murmur3a", b"hello there", "6440964d");
    let seeded = |algo: &str, seed: i64, chunks: &[&[u8]]| {
        let opts = HashOptions {
            seed: Seed::Int(seed),
            secret: None,
        };
        let mut c = HashContext::new(algo.as_bytes(), 0, None, &opts, &mut Vec::new()).unwrap();
        for ch in chunks {
            c.update(ch);
        }
        let streamed = hx(&c.finalize().unwrap());
        let whole: Vec<u8> = chunks.concat();
        let once = hx(&hash(algo.as_bytes(), &whole, &opts, &mut Vec::new()).unwrap());
        assert_eq!(streamed, once);
        once
    };
    let bar: &[&[u8]] = &[b"Two", b" hashes", b" meet", b" in", b" a", b" bar."];
    assert_eq!(
        seeded("murmur3f", 42, bar),
        "95855f9be0db784a5c37e878c4a4dcee"
    );
    assert_eq!(
        seeded("murmur3c", 106, bar),
        "f64c9eb40287fa686575163893e283b2"
    );
    assert_eq!(seeded("murmur3a", 2345, bar), "7f7ec59b");
    // Test xxhash_seed.phpt.
    let lorem: &[&[u8]] = &[
        b"Lorem",
        b" ipsum dolor",
        b" sit amet,",
        b" consectetur adipiscing elit.",
    ];
    assert_eq!(seeded("xxh32", 42, lorem), "3d0cc7e5");
    assert_eq!(seeded("xxh64", 42, lorem), "9c9aa071b5d22a15");
    assert_eq!(seeded("xxh3", 42, lorem), "366409913c16b70d");
    assert_eq!(
        seeded("xxh128", 42, lorem),
        "f87856a7589354e92aeca886c71ed7fb"
    );
}

/// Test ext/hash/tests/xxhash_secret.phpt and xxh3_convert_secret_to_string.phpt.
#[test]
fn xxh3_secret() {
    let secret = vec![b'a'; 256];
    for (algo, want) in [
        ("xxh3", "8028aa834c03557a"),
        ("xxh128", "54279097795e7218093a05d4d781cbb9"),
    ] {
        let both = HashOptions {
            seed: Seed::Int(24),
            secret: Some(secret.clone()),
        };
        let err = HashContext::new(algo.as_bytes(), 0, None, &both, &mut Vec::new())
            .err()
            .unwrap();
        assert_eq!(
            err,
            HashError::Error(format!(
                "{algo}: Only one of seed or secret is to be passed for initialization"
            ))
        );
        let short = HashOptions {
            seed: Seed::Absent,
            secret: Some(vec![b'a'; 17]),
        };
        let err = HashContext::new(algo.as_bytes(), 0, None, &short, &mut Vec::new())
            .err()
            .unwrap();
        assert_eq!(
            err,
            HashError::Error(format!(
                "{algo}: Secret length must be >= 136 bytes, 17 bytes passed"
            ))
        );
        let opts = HashOptions {
            seed: Seed::Absent,
            secret: Some(secret.clone()),
        };
        let mut c = HashContext::new(algo.as_bytes(), 0, None, &opts, &mut Vec::new()).unwrap();
        for p in [
            &b"Lorem"[..],
            b" ipsum dolor",
            b" sit amet,",
            b" consectetur adipiscing elit.",
        ] {
            c.update(p);
        }
        assert_eq!(hx(&c.finalize().unwrap()), want);
        let mut d = Vec::new();
        let one = hash(
            algo.as_bytes(),
            b"Lorem ipsum dolor sit amet, consectetur adipiscing elit.",
            &opts,
            &mut d,
        )
        .unwrap();
        assert_eq!(hx(&one), want);
        assert!(d.is_empty());
        assert!(lookup(algo.as_bytes()).unwrap().uses_secret());
    }
    assert!(
        SECRET_DEPRECATION
            .starts_with("Passing a secret of a type other than string is deprecated")
    );
    // Secrets past 256 bytes are cut, with a warning (observed on PHP 8.5).
    let mut d = Vec::new();
    let long = HashOptions {
        seed: Seed::Absent,
        secret: Some(vec![b'x'; 300]),
    };
    let cut = HashOptions {
        seed: Seed::Absent,
        secret: Some(vec![b'x'; 256]),
    };
    let a = hash(b"xxh3", &[b'q'; 5000], &long, &mut d).unwrap();
    assert_eq!(
        d,
        vec![Diag::Warning(
            "xxh3: Secret content exceeding 256 bytes discarded".into()
        )]
    );
    assert_eq!(
        a,
        hash(b"xxh3", &[b'q'; 5000], &cut, &mut Vec::new()).unwrap()
    );
}

/// Tests murmur_seed_deprecation.phpt and xxhash_seed_deprecation.phpt.
#[test]
fn seed_deprecations() {
    let opts = HashOptions {
        seed: Seed::NonInt,
        secret: None,
    };
    for (algo, why) in [
        ("murmur3a", "the same as setting the seed to 0"),
        ("murmur3c", "the same as setting the seed to 0"),
        ("murmur3f", "the same as setting the seed to 0"),
        ("xxh32", "the same as setting the seed to 0"),
        ("xxh64", "the same as setting the seed to 0"),
        ("xxh3", "ignored"),
        ("xxh128", "ignored"),
    ] {
        let mut d = Vec::new();
        let got = hash(algo.as_bytes(), b"abc", &opts, &mut d).unwrap();
        assert_eq!(
            d,
            vec![Diag::Deprecated(format!(
                "Passing a seed of a type other than int is deprecated because it is {why}"
            ))]
        );
        assert_eq!(
            got,
            hash(
                algo.as_bytes(),
                b"abc",
                &HashOptions::default(),
                &mut Vec::new()
            )
            .unwrap()
        );
    }
    // Other algorithms ignore the options silently.
    let mut d = Vec::new();
    assert_eq!(
        h("md5", b"abc"),
        hx(&hash(b"md5", b"abc", &opts, &mut d).unwrap())
    );
    assert!(d.is_empty());
}

/// RFC 2104 (test hmac-md5.phpt), RFC 4231 and RFC 2286.
#[test]
fn hmac_rfc() {
    let mac = |a: &str, key: &[u8], data: &[u8]| hx(&hash_hmac(a.as_bytes(), data, key).unwrap());
    assert_eq!(
        mac("md5", &[0x0b; 16], b"Hi There"),
        "9294727a3638bb1c13f48ef8158bfc9d"
    );
    assert_eq!(
        mac("md5", b"Jefe", b"what do ya want for nothing?"),
        "750c783e6ab0b503eaa86e310a5db738"
    );
    assert_eq!(
        mac("md5", &[0xaa; 16], &[0xdd; 50]),
        "56be34521d144c88dbb8c733f0e8b3f6"
    );
    assert_eq!(
        mac("sha256", &[0x0b; 20], b"Hi There"),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert_eq!(
        mac("sha512", b"Jefe", b"what do ya want for nothing?"),
        "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737"
    );
    // RFC 4231 test case 6: a key longer than the block.
    assert_eq!(
        mac(
            "sha256",
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First"
        ),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
    assert_eq!(
        mac("ripemd160", &[0x0b; 20], b"Hi There"),
        "24cb4bd67d20fc1a5d2ed7732dcc39377f0a5668"
    );
    // Streaming HMAC contexts agree with hash_hmac().
    let mut c = HashContext::new(
        b"sha1",
        HASH_HMAC,
        Some(b"Jefe"),
        &HashOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    c.update(b"what do ya ");
    let mut d = c.copy().unwrap();
    c.update(b"want for nothing?");
    d.update(b"want for nothing?");
    let want = mac("sha1", b"Jefe", b"what do ya want for nothing?");
    assert_eq!(hx(&c.finalize().unwrap()), want);
    assert_eq!(hx(&d.finalize().unwrap()), want);
    assert_eq!(want, "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79");
    let algo = lookup(b"md5").unwrap();
    assert_eq!(
        hx(&hmac(algo, b"Jefe", b"what do ya want for nothing?")),
        "750c783e6ab0b503eaa86e310a5db738"
    );
}

/// RFC 5869 appendix A (test hash_hkdf_rfc5869.phpt) and hash_hkdf_edges.phpt.
#[test]
fn hkdf() {
    let k = |a: &str, ikm: &[u8], l: i64, info: &[u8], salt: &[u8]| {
        hx(&hash_hkdf(a.as_bytes(), ikm, l, info, salt).unwrap())
    };
    let r = |a: u8, b: u8| (a..=b).collect::<Vec<u8>>();
    assert_eq!(
        k("sha256", &[0x0b; 22], 42, &r(0xf0, 0xf9), &r(0x00, 0x0c)),
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
    );
    assert_eq!(
        k("sha256", &r(0x00, 0x4f), 82, &r(0xb0, 0xff), &r(0x60, 0xaf)),
        "b11e398dc80327a1c8e7f78c596a49344f012eda2d4efad8a050cc4c19afa97c59045a99cac7827271cb41c65e590e09da3275600c2f09b8367793a9aca3db71cc30c58179ec3e87c14c01d5c1f3434f1d87"
    );
    assert_eq!(
        k("sha256", &[0x0b; 22], 42, b"", b""),
        "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
    );
    assert_eq!(
        k("sha1", &[0x0b; 11], 42, &r(0xf0, 0xf9), &r(0x00, 0x0c)),
        "085a01ea1b10f36933068b56efa5ad81a4f14b822f5b091568a9cdd4f155fda2c22e422478d305f3f896"
    );
    assert_eq!(
        k("sha1", &[0x0c; 22], 42, b"", b""),
        "2c91117204d745f3500d636a62f64f0ab3bae548aa53d423b0d1f27ebba6f5e5673a081d70cce7acfc48"
    );
    let ikm = b"input key material";
    assert_eq!(k("md5", ikm, 7, b"", b""), "98b16391063ece");
    assert_eq!(k("Md5", ikm, 7, b"", b""), "98b16391063ece");
    assert_eq!(
        k("md5", ikm, 17, b"", b""),
        "98b16391063ecee006a3ca8ee5776b1e5f"
    );
    // Errors, test hash_hkdf_error.phpt.
    let e = |a: &str, ikm: &[u8], l: i64| hash_hkdf(a.as_bytes(), ikm, l, b"", b"").err().unwrap();
    let bad = HashError::ValueError(
        "hash_hkdf(): Argument #1 ($algo) must be a valid cryptographic hashing algorithm".into(),
    );
    for a in [
        "foo", "adler32", "crc32", "crc32b", "fnv132", "fnv1a32", "fnv164", "fnv1a64", "joaat",
        "jOaAt",
    ] {
        assert_eq!(e(a, ikm, 0), bad);
    }
    assert_eq!(
        e("sha1", b"", 0),
        HashError::ValueError("hash_hkdf(): Argument #2 ($key) must not be empty".into())
    );
    assert_eq!(
        e("sha1", ikm, -1),
        HashError::ValueError(
            "hash_hkdf(): Argument #3 ($length) must be greater than or equal to 0".into()
        )
    );
    assert_eq!(
        e("sha1", ikm, 20 * 255 + 1),
        HashError::ValueError(
            "hash_hkdf(): Argument #3 ($length) must be less than or equal to 5100".into()
        )
    );
    assert_eq!(hash_hkdf(b"sha1", ikm, 5100, b"", b"").unwrap().len(), 5100);
}

/// RFC 6070 and test hash_pbkdf2_basic.phpt / hash_pbkdf2_error.phpt.
#[test]
fn pbkdf2() {
    let p = |a: &str, pw: &[u8], salt: &[u8], it: i64, len: i64, raw: bool| {
        let out = hash_pbkdf2(a.as_bytes(), pw, salt, it, len, raw).unwrap();
        if raw {
            hx(&out)
        } else {
            String::from_utf8(out).unwrap()
        }
    };
    assert_eq!(
        p("sha1", b"password", b"salt", 1, 20, true),
        "0c60c80f961f0e71f3a9b524af6012062fe037a6"
    );
    assert_eq!(
        p("sha1", b"password", b"salt", 2, 20, true),
        "ea6c014dc72d6f8ccd1ed92ace1d41f0d8de8957"
    );
    assert_eq!(
        p("sha1", b"password", b"salt", 4096, 20, true),
        "4b007901b765489abead49d926f721d065a429c1"
    );
    assert_eq!(
        p(
            "sha1",
            b"passwordPASSWORDpassword",
            b"saltSALTsaltSALTsaltSALTsaltSALTsalt",
            4096,
            25,
            true
        ),
        "3d2eec4fe41c849b80c8d83662c0e44a8b291a964cf2f07038"
    );
    assert_eq!(
        p("sha1", b"pass\0word", b"sa\0lt", 4096, 16, true),
        "56fa6aa75548099dcc37d7f03425e0c3"
    );
    assert_eq!(
        p("sha1", b"password", b"salt", 1, 20, false),
        "0c60c80f961f0e71f3a9"
    );
    assert_eq!(
        p("sha1", b"password", b"salt", 1, 0, false),
        "0c60c80f961f0e71f3a9b524af6012062fe037a6"
    );
    assert_eq!(
        p(
            "sha1",
            b"passwordPASSWORDpassword",
            b"saltSALTsaltSALTsaltSALTsaltSALTsalt",
            4096,
            25,
            false
        ),
        "3d2eec4fe41c849b80c8d8366"
    );
    assert_eq!(
        p("sha256", b"password", b"salt", 1, 20, false),
        "120fb6cffcf8b32c43e7"
    );
    let e = |a: &str, it: i64, len: i64| {
        hash_pbkdf2(a.as_bytes(), b"password", b"salt", it, len, false)
            .err()
            .unwrap()
    };
    let bad = HashError::ValueError(
        "hash_pbkdf2(): Argument #1 ($algo) must be a valid cryptographic hashing algorithm".into(),
    );
    assert_eq!(e("foo", 1, 0), bad);
    assert_eq!(e("crc32", 1, 0), bad);
    let it = HashError::ValueError(
        "hash_pbkdf2(): Argument #4 ($iterations) must be greater than 0".into(),
    );
    assert_eq!(e("md5", 0, 0), it);
    assert_eq!(e("md5", -1, 0), it);
    assert_eq!(
        e("md5", 1, -1),
        HashError::ValueError(
            "hash_pbkdf2(): Argument #5 ($length) must be greater than or equal to 0".into()
        )
    );
}

/// Tests hash_algos.phpt, hash_hmac_algos.phpt, hash_error.phpt,
/// hash_hmac_error.phpt, hash_init_error.phpt, gh12186_*.phpt and
/// HashContext_debugInfo.phpt.
#[test]
fn registry_and_errors() {
    let all: Vec<&str> = algos().collect();
    assert_eq!(all.len(), 60);
    assert_eq!(all[0], "md2");
    assert_eq!(all[59], "haval256,5");
    let hm: Vec<&str> = hmac_algos().collect();
    assert_eq!(hm.len(), 44);
    assert!(!hm.contains(&"crc32") && !hm.contains(&"xxh128") && hm.contains(&"gost-crypto"));
    assert_eq!(
        hash(b"foo", b"", &HashOptions::default(), &mut Vec::new())
            .err()
            .unwrap(),
        HashError::ValueError(
            "hash(): Argument #1 ($algo) must be a valid hashing algorithm".into()
        )
    );
    assert_eq!(h("MD5", b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    for name in ["foo", "crc32"] {
        assert_eq!(
            hash_hmac(name.as_bytes(), b"x", b"secret").err().unwrap(),
            HashError::ValueError(
                "hash_hmac(): Argument #1 ($algo) must be a valid cryptographic hashing algorithm"
                    .into()
            )
        );
    }
    let init = |name: &str, flags: i64, key: Option<&[u8]>| {
        HashContext::new(
            name.as_bytes(),
            flags,
            key,
            &HashOptions::default(),
            &mut Vec::new(),
        )
        .err()
        .unwrap()
    };
    assert_eq!(
        init("dummy", 0, None),
        HashError::ValueError(
            "hash_init(): Argument #1 ($algo) must be a valid hashing algorithm".into()
        )
    );
    assert_eq!(
        init("crc32", HASH_HMAC, None),
        HashError::ValueError(
            "hash_init(): Argument #1 ($algo) must be a cryptographic hashing algorithm if HMAC is requested".into()
        )
    );
    assert_eq!(
        init("md5", HASH_HMAC, None),
        HashError::ValueError(
            "hash_init(): Argument #3 ($key) must not be empty when HMAC is requested".into()
        )
    );
    let mut c =
        HashContext::new(b"sha256", 0, None, &HashOptions::default(), &mut Vec::new()).unwrap();
    assert_eq!(c.algo_name(), "sha256");
    assert!(c.finalize().is_some());
    assert!(c.is_finalized() && c.copy().is_none() && !c.update(b"x") && c.finalize().is_none());
    assert_eq!(
        goro_ext_hash::finalized_error("hash_copy").message(),
        "hash_copy(): Argument #1 ($context) must be a valid, non-finalized HashContext"
    );
    let s = HashContext::new(
        b"snefru256",
        0,
        None,
        &HashOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(s.algo_name(), "snefru");
}

#[test]
fn equals() {
    assert!(hash_equals(b"abc", b"abc"));
    assert!(!hash_equals(b"abc", b"abd"));
    assert!(!hash_equals(b"abc", b"ab"));
    assert!(hash_equals(b"", b""));
}

/// Base64 decoding for the serialization strings of the tests.
fn b64(s: &str) -> Vec<u8> {
    let v = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    } as u32;
    let s = s.trim_end_matches('=').as_bytes();
    let mut out = Vec::new();
    for c in s.chunks(4) {
        let mut n = 0u32;
        for (i, &x) in c.iter().enumerate() {
            n |= v(x) << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if c.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if c.len() > 3 {
            out.push(n as u8);
        }
    }
    out
}

/// Test ext/hash/tests/hash_serialize_003.phpt (MD5 entry): the state
/// after "I can't remember anything".
#[test]
fn serialize_003_md5() {
    let mut c =
        HashContext::new(b"md5", 0, None, &HashOptions::default(), &mut Vec::new()).unwrap();
    c.update(b"I can't remember anything");
    let s = c.serialize().unwrap();
    assert_eq!((s.algo, s.flags, s.magic), ("md5", 0, 2));
    assert_eq!(s.state.len(), 23);
    assert_eq!(s.state[0], SerVal::Int(25));
    assert_eq!(s.state[2], SerVal::Int(1732584193));
    assert_eq!(s.state[3], SerVal::Int(-271733879));
    let mut buf = b"I can't remember anything".to_vec();
    buf.resize(64, 0);
    assert_eq!(s.state[6], SerVal::Str(buf));
    // The serialized string of the test, decoded.
    let ser = b64(concat!(
        "TzoxMToiSGFzaENvbnRleHQiOjU6e2k6MDtzOjM6Im1kNSI7aToxO2k6MDtpOjI7YToyMzp7aTowO2k6MjU7aToxO2k6MDtpOjI7aToxNzMyNTg0",
        "MTkzO2k6MztpOi0yNzE3MzM4Nzk7aTo0O2k6LTE3MzI1ODQxOTQ7aTo1O2k6MjcxNzMzODc4O2k6NjtzOjY0OiJJIGNhbid0IHJlbWVtYmVyIGFu",
        "eXRoaW5nAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIjtpOjc7aTowO2k6ODtpOjA7aTo5O2k6MDtpOjEwO2k6MDtpOjEx",
        "O2k6MDtpOjEyO2k6MDtpOjEzO2k6MDtpOjE0O2k6MDtpOjE1O2k6MDtpOjE2O2k6MDtpOjE3O2k6MDtpOjE4O2k6MDtpOjE5O2k6MDtpOjIwO2k6",
        "MDtpOjIxO2k6MDtpOjIyO2k6MDt9aTozO2k6MjtpOjQ7YTowOnt9fQ=="
    ));
    assert!(
        ser.starts_with(b"O:11:\"HashContext\":5:{i:0;s:3:\"md5\";i:1;i:0;i:2;a:23:{i:0;i:25;")
    );
    // Unserialize, continue, finish (test_serialization() of the test).
    let mut u = HashContext::unserialize(
        Some(b"md5"),
        Some(0),
        StateSlot::Array(&s.state),
        Some(2),
        true,
    )
    .unwrap();
    u.update("Can’t tell if this is true or dream".as_bytes());
    assert_eq!(
        hx(&u.finalize().unwrap()),
        "f255c114bd6ce94aad092b5141c00d46"
    );
}

/// SHA-3 states use magic 100, and the position must stay below the rate
/// (observed on PHP 8.5).
#[test]
fn serialize_sha3() {
    let mut c = HashContext::new(
        b"sha3-256",
        0,
        None,
        &HashOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    c.update(b"I can't remember anything");
    let s = c.serialize().unwrap();
    assert_eq!(s.magic, 100);
    let SerVal::Str(st) = &s.state[0] else {
        panic!()
    };
    assert_eq!(&st[..8], b"I can't ");
    assert_eq!(st[8], b'r' ^ 0xff);
    assert_eq!(s.state[1], SerVal::Int(25));
    let mut u = HashContext::unserialize(
        Some(b"sha3-256"),
        Some(0),
        StateSlot::Array(&s.state),
        Some(100),
        true,
    )
    .unwrap();
    u.update("Can’t tell if this is true or dream".as_bytes());
    assert_eq!(
        hx(&u.finalize().unwrap()),
        "57aa7a90f29b5ab66592760592780da247fd39b4c911773687450f9df8cc8ed0"
    );
    let mut bad = s.state.clone();
    bad[1] = SerVal::Int(136);
    let e = HashContext::unserialize(
        Some(b"sha3-256"),
        Some(0),
        StateSlot::Array(&bad),
        Some(100),
        true,
    )
    .err()
    .unwrap();
    assert_eq!(
        e.message(),
        "Incomplete or ill-formed serialization data (\"sha3-256\" code -2000)"
    );
    let e = HashContext::unserialize(
        Some(b"sha3-256"),
        Some(0),
        StateSlot::Array(&s.state),
        Some(2),
        true,
    )
    .err()
    .unwrap();
    assert_eq!(
        e.message(),
        "Incomplete or ill-formed serialization data (\"sha3-256\" code -1)"
    );
}

/// Tests hash_serialize_002.phpt, hash_serialize_004.phpt and
/// xxhash_unserialize_memsize.phpt.
#[test]
fn serialize_errors() {
    let o = HashOptions::default();
    for a in hmac_algos() {
        let c = HashContext::new(
            a.as_bytes(),
            HASH_HMAC,
            Some(b"This is the key that I have"),
            &o,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(
            c.serialize().err().unwrap(),
            HashError::Exception("HashContext with HASH_HMAC option cannot be serialized".into())
        );
    }
    for a in ["xxh3", "xxh128"] {
        let c = HashContext::new(a.as_bytes(), 0, None, &o, &mut Vec::new()).unwrap();
        assert_eq!(
            c.serialize().err().unwrap().message(),
            format!("HashContext for algorithm \"{a}\" cannot be serialized")
        );
    }
    let mut c = HashContext::new(b"sha1", 0, None, &o, &mut Vec::new()).unwrap();
    c.update(b"I can't remember anything");
    let st = c.serialize().unwrap().state;
    let un = |algo: Option<&[u8]>, flags: Option<i64>, state: StateSlot<'_>, magic: Option<i64>| {
        HashContext::unserialize(algo, flags, state, magic, true)
            .err()
            .unwrap()
            .message()
            .to_string()
    };
    let ill = "Incomplete or ill-formed serialization data";
    assert_eq!(un(None, None, StateSlot::Missing, None), ill);
    assert_eq!(un(None, Some(0), StateSlot::Array(&st), Some(2)), ill);
    assert_eq!(un(Some(b"sha1"), None, StateSlot::Array(&st), Some(2)), ill);
    assert_eq!(
        un(Some(b"sha1"), Some(1), StateSlot::Array(&st), Some(2)),
        "HashContext with HASH_HMAC option cannot be serialized"
    );
    assert_eq!(
        un(Some(b"sha1"), Some(0), StateSlot::NotArray, Some(2)),
        format!("{ill} (\"sha1\" code -1)")
    );
    let mut bad = st.clone();
    bad[6] = SerVal::Str(Vec::new());
    assert_eq!(
        un(Some(b"sha1"), Some(0), StateSlot::Array(&bad), Some(2)),
        format!("{ill} (\"sha1\" code -1024)")
    );
    assert_eq!(
        un(Some(b"sha1"), Some(0), StateSlot::Array(&st), Some(0)),
        format!("{ill} (\"sha1\" code -1)")
    );
    assert_eq!(
        un(Some(b"xxx1"), Some(0), StateSlot::Array(&st), Some(2)),
        "Unknown hash algorithm"
    );
    // A state cut short reports the missing field's offset.
    let short = st[..st.len() - 1].to_vec();
    assert_eq!(
        un(Some(b"sha1"), Some(0), StateSlot::Array(&short), Some(2)),
        format!("{ill} (\"sha1\" code -1028)")
    );
    let x32: Vec<SerVal> = [
        0,
        0,
        606290984,
        -2048144777,
        0,
        1640531535,
        0,
        0,
        0,
        0,
        30,
        0,
    ]
    .map(SerVal::Int)
    .to_vec();
    assert_eq!(
        un(Some(b"xxh32"), Some(0), StateSlot::Array(&x32), Some(2)),
        format!("{ill} (\"xxh32\" code -2000)")
    );
    let x64: Vec<SerVal> = [
        0, 0, 6, 2, 8, 9, 0, 0, 1, 5, 0, 0, 0, 0, 0, 0, 0, 0, 70, 0, 0, 0,
    ]
    .map(SerVal::Int)
    .to_vec();
    assert_eq!(
        un(Some(b"xxh64"), Some(0), StateSlot::Array(&x64), Some(2)),
        format!("{ill} (\"xxh64\" code -2000)")
    );
    assert_eq!(
        un(Some(b"xxh3"), Some(0), StateSlot::Array(&st), Some(2)),
        format!("{ill} (\"xxh3\" code -1)")
    );
}

/// Tests hash_copy_001.phpt / hash-clone.phpt for every algorithm, plus
/// round trips of every serializable state.
#[test]
fn copy_and_round_trip() {
    let first = b"I can't remember anything";
    let second = "Can’t tell if this is true or dream".as_bytes();
    let o = HashOptions::default();
    for a in algos() {
        let algo = lookup(a.as_bytes()).unwrap();
        let mut orig = HashContext::new(a.as_bytes(), 0, None, &o, &mut Vec::new()).unwrap();
        orig.update(first);
        let mut copy = orig.copy().unwrap();
        copy.update(second);
        let whole = [&first[..], second].concat();
        let want = digest(algo, &whole, &o, &mut Vec::new()).unwrap();
        assert_eq!(copy.finalize().unwrap(), want, "{a}");
        if algo.serializable() {
            let s = orig.serialize().unwrap();
            let mut u = HashContext::unserialize(
                Some(s.algo.as_bytes()),
                Some(s.flags),
                StateSlot::Array(&s.state),
                Some(s.magic),
                true,
            )
            .unwrap();
            u.update(second);
            assert_eq!(u.finalize().unwrap(), want, "{a} after unserialize");
        }
        let mut hs = Hasher::plain(algo);
        hs.update(&whole);
        assert_eq!(hs.finish(), want);
        assert_eq!(
            orig.finalize().unwrap(),
            digest(algo, first, &o, &mut Vec::new()).unwrap()
        );
    }
}

#[test]
fn unhex_helper() {
    assert_eq!(unhex("00ff"), vec![0, 255]);
}
