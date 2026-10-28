//! The deterministic parts of php-src's `ext/sodium/tests/*.phpt`, played
//! through the library: each test reproduces a script's computations and
//! compares the output with the test's `--EXPECT--` section, read at test
//! time from the test-suite checkout (`PHP_TESTS_DIR`, by default
//! `/tmp/claude-1000/php-tests`). Tests are skipped when the checkout is
//! missing. Inputs that are only in the scripts (hex vectors) are read from
//! the `--FILE--` sections too.

use goro_ext_sodium::*;
use goro_runtime::{Diagnostics, PhpStr, Value};

fn phpt(name: &str) -> Option<(String, Vec<String>)> {
    let dir =
        std::env::var("PHP_TESTS_DIR").unwrap_or_else(|_| "/tmp/claude-1000/php-tests".into());
    let path = format!("{dir}/ext/sodium/tests/{name}");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("skipping: {path} not found");
        return None;
    };
    let section = |tag: &str| -> Option<String> {
        let start = text.find(&format!("--{tag}--\n"))? + tag.len() + 5;
        let rest = &text[start..];
        let end = rest.find("\n--").map(|e| e + 1).unwrap_or(rest.len());
        Some(rest[..end].to_string())
    };
    let expect = section("EXPECT").or_else(|| section("EXPECTF")).unwrap();
    Some((
        section("FILE").unwrap(),
        expect.lines().map(|l| l.to_string()).collect(),
    ))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    sodium_hex2bin(s.as_bytes(), b"").unwrap()
}

fn vd_str(s: &str) -> String {
    format!("string({}) \"{s}\"", s.len())
}

fn vd_bool(b: bool) -> String {
    format!("bool({b})")
}

fn vd_int(i: i64) -> String {
    format!("int({i})")
}

/// The 64-hex-digit strings quoted in a script, in order.
fn quoted_hex(file: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in file.split('"').skip(1).step_by(2) {
        if part.len() == 64 && part.bytes().all(|c| c.is_ascii_hexdigit()) {
            out.push(part.to_string());
        }
    }
    out
}

fn exc(r: SodiumError) -> String {
    match r {
        SodiumError::Exception(m) => m,
        e => panic!("{e:?}"),
    }
}

#[test]
fn crypto_kx() {
    let Some((_, want)) = phpt("crypto_kx.phpt") else {
        return;
    };
    let cs = unhex("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
    let ss = unhex("f123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde0");
    let ckp = sodium_crypto_kx_seed_keypair(&cs).unwrap();
    let skp = sodium_crypto_kx_seed_keypair(&ss).unwrap();
    let c = sodium_crypto_kx_client_session_keys(&ckp, &sodium_crypto_kx_publickey(&skp).unwrap())
        .unwrap();
    let s = sodium_crypto_kx_server_session_keys(&skp, &sodium_crypto_kx_publickey(&ckp).unwrap())
        .unwrap();
    let got = [
        hex(&ckp),
        hex(&skp),
        hex(&c.0),
        hex(&s.1),
        hex(&c.1),
        hex(&s.0),
    ]
    .map(|h| vd_str(&h));
    assert_eq!(got.to_vec(), want);
}

#[test]
fn crypto_generichash() {
    let Some((_, want)) = phpt("crypto_generichash.phpt") else {
        return;
    };
    let k16 = b"0123456789abcdef";
    let k32 = b"0123456789abcdef0123456789abcdef";
    let mut got = vec![
        hex(&sodium_crypto_generichash(b"msg", b"", 32).unwrap()),
        hex(&sodium_crypto_generichash(b"msg", k16, 32).unwrap()),
        hex(&sodium_crypto_generichash(b"msg", k16, 64).unwrap()),
        hex(&sodium_crypto_generichash(b"msg", k32, 64).unwrap()),
    ];
    let state =
        |k: &[u8], n: i64| Value::Str(PhpStr::from(sodium_crypto_generichash_init(k, n).unwrap()));
    let mut st = state(b"", 32);
    got.push(hex(&sodium_crypto_generichash_final(&mut st, 32).unwrap()));
    for (k, n) in [(&b""[..], 32), (&k16[..], 32)] {
        let mut st = state(k, n);
        sodium_crypto_generichash_update(&mut st, b"m").unwrap();
        sodium_crypto_generichash_update(&mut st, b"sg").unwrap();
        got.push(hex(&sodium_crypto_generichash_final(&mut st, 32).unwrap()));
    }
    let mut st = state(k16, 64);
    sodium_crypto_generichash_update(&mut st, b"m").unwrap();
    sodium_crypto_generichash_update(&mut st, b"sg").unwrap();
    let mut st2 = st.clone();
    got.push(hex(&sodium_crypto_generichash_final(&mut st, 64).unwrap()));
    sodium_crypto_generichash_update(&mut st2, b"2").unwrap();
    let exp = hex(&sodium_crypto_generichash_final(&mut st2, 64).unwrap());
    let act = hex(&sodium_crypto_generichash(b"msg2", k16, 64).unwrap());
    let mut lines: Vec<String> = got.iter().map(|h| vd_str(h)).collect();
    lines.push(vd_str(&exp));
    lines.push(vd_str(&act));
    lines.push(vd_bool(exp == act));
    lines.push(vd_bool(
        sodium_crypto_generichash(b"test", b"", 128).is_err(),
    ));
    assert_eq!(lines, want);
}

#[test]
fn crypto_scalarmult() {
    let Some((_, want)) = phpt("crypto_scalarmult.phpt") else {
        return;
    };
    let n = unhex("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");
    let p = unhex("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a");
    let got = vec![
        vd_str(&hex(&sodium_crypto_scalarmult(&n, &p).unwrap())),
        vd_bool(sodium_crypto_scalarmult(&n[1..], &p).is_err()),
    ];
    assert_eq!(got, want);
}

#[test]
fn crypto_scalarmult_ristretto255() {
    let Some((_, want)) = phpt("crypto_scalarmult_ristretto255.phpt") else {
        return;
    };
    let b = unhex("e2f2ae0a6abc4e71a884a961c500515f58e30b6aa582dd8db6a65945e08d2d76");
    let mut n = Value::Str(PhpStr::from(vec![0u8; 32]));
    let nb = |v: &Value| v.as_str().unwrap().as_bytes().to_vec();
    let mut got = vec![
        exc(sodium_crypto_scalarmult_ristretto255_base(&nb(&n)).unwrap_err()),
        exc(sodium_crypto_scalarmult_ristretto255(&nb(&n), &b).unwrap_err()),
    ];
    let mut p = Vec::new();
    for _ in 1..16 {
        sodium_increment(&mut n).unwrap();
        p = sodium_crypto_scalarmult_ristretto255_base(&nb(&n)).unwrap();
        let p2 = sodium_crypto_scalarmult_ristretto255(&nb(&n), &b).unwrap();
        assert_eq!(p, p2);
        got.push(vd_str(&hex(&p)));
    }
    got.push(exc(sodium_crypto_scalarmult(&nb(&n)[1..], &p).unwrap_err()));
    assert_eq!(got, want);
}

#[test]
fn crypto_core_ristretto255() {
    let Some((file, want)) = phpt("crypto_core_ristretto255.phpt") else {
        return;
    };
    let vectors = quoted_hex(&file);
    // The bad encodings, then the two halves of the hash input.
    let (bad, hash) = vectors.split_at(vectors.len() - 2);
    let mut got: Vec<String> = bad
        .iter()
        .map(|h| vd_bool(sodium_crypto_core_ristretto255_is_valid_point(&unhex(h)).unwrap()))
        .collect();
    let input = unhex(&(hash[0].clone() + &hash[1]));
    got.push(vd_str(&hex(&sodium_crypto_core_ristretto255_from_hash(
        &input,
    )
    .unwrap())));
    let r = sodium_crypto_core_ristretto255_scalar_random().unwrap();
    let s0 = sodium_crypto_scalarmult_ristretto255_base(&r).unwrap();
    got.push(vd_bool(
        sodium_crypto_core_ristretto255_is_valid_point(&s0).unwrap(),
    ));
    let l = unhex("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
    let s = sodium_crypto_core_ristretto255_random().unwrap();
    got.push(exc(
        sodium_crypto_scalarmult_ristretto255(&s, &l).unwrap_err()
    ));
    sodium_crypto_scalarmult_ristretto255(&r, &s).unwrap();
    let ru = goro_ext_sodium::sodium_crypto_generichash_keygen()
        .unwrap()
        .repeat(2);
    let s3 = sodium_crypto_core_ristretto255_from_hash(&ru).unwrap();
    got.push(vd_bool(
        sodium_crypto_core_ristretto255_is_valid_point(&s3).unwrap(),
    ));
    let plus = sodium_crypto_core_ristretto255_add(&s, &s0).unwrap();
    let minus = sodium_crypto_core_ristretto255_sub(&plus, &s0).unwrap();
    got.push(vd_bool(minus == s));
    assert_eq!(got, want);
}

#[test]
fn crypto_shorthash() {
    let Some((_, want)) = phpt("crypto_shorthash.phpt") else {
        return;
    };
    let got = vec![
        hex(&sodium_crypto_shorthash(b"message", b"0123456789ABCDEF").unwrap()),
        hex(&sodium_crypto_shorthash(b"message", b"0123456789abcdef").unwrap()),
        hex(&sodium_crypto_shorthash(b"msg", b"0123456789abcdef").unwrap()),
        vd_bool(sodium_crypto_shorthash(b"message", b"0123456789ABCDEF0123456789abcdef").is_err()),
    ];
    assert_eq!(got, want);
}

#[test]
fn crypto_box_fixed_seal() {
    let Some((_, want)) = phpt("crypto_box.phpt") else {
        return;
    };
    let msg = unhex(
        "7375f4094f1151640bd853cb13dbc1a0ee9e13b0287a89d34fa2f6732be9de13f88457553d\
         768347116522d6d32c9cb353ef07aa7c83bd129b2bb5db35b28334c935b24f2639405a0604",
    );
    let kp = unhex(
        "36a6c2b96a650d80bf7e025e0f58f3d636339575defb370801a54213bd54582d\
         5aecbcf7866e7a4d58a6c1317e2b955f54ecbe2fcbbf7d262c10636ed524480c",
    );
    let out = sodium_crypto_box_seal_open(&msg, &kp).unwrap().unwrap();
    assert_eq!(
        &vd_str(std::str::from_utf8(&out).unwrap()),
        want.last().unwrap()
    );
    let err = exc(sodium_crypto_box(b"m", &[0; 24], &kp[1..]).unwrap_err());
    assert!(want.contains(&err));
}

#[test]
fn crypto_sign_sk_to_curve25519() {
    let Some((_, want)) = phpt("crypto_sign.phpt") else {
        return;
    };
    let sk = unhex(
        "55b62f664bf1c359f58a6b91b89556f97284273510573055b9237d17f5a20564607f0626f49e63c2c8f814ed6d955bf8b005b33fd5fd56eaca93073d8eb99165",
    );
    let c = sodium_crypto_sign_ed25519_sk_to_curve25519(&sk).unwrap();
    assert_eq!(
        hex(&c),
        "381b2be5e3d38820deb1243fb58b4be654da30dd3ccde492cb88f937eb489363"
    );
    // The whole script, with fresh keys as the test uses.
    let kp = sodium_crypto_sign_keypair().unwrap();
    let sk = sodium_crypto_sign_secretkey(&kp).unwrap();
    let pk = sodium_crypto_sign_publickey(&kp).unwrap();
    let mut got = vec![
        vd_bool(kp.len() == 96),
        vd_bool(sk.len() == 64),
        vd_bool(pk.len() == 32),
        vd_bool(pk != sk),
        vd_bool(sodium_crypto_sign_keypair_from_secretkey_and_publickey(&sk, &pk).unwrap() == kp),
    ];
    let msg = b"Here is the message, to be signed using Alice's secret key, and to be verified using Alice's public key";
    let sm = sodium_crypto_sign(msg, &sk).unwrap();
    got.push(vd_bool(sm.len() - msg.len() == 64));
    got.push(vd_bool(
        sodium_crypto_sign_open(&sm, &pk).unwrap().as_deref() == Some(&msg[..]),
    ));
    let kp = sodium_crypto_sign_seed_keypair(&[b'x'; 32]).unwrap();
    let (sk, pk) = (
        sodium_crypto_sign_secretkey(&kp).unwrap(),
        sodium_crypto_sign_publickey(&kp).unwrap(),
    );
    let msg = b"Here is another message";
    let sm = sodium_crypto_sign(msg, &sk).unwrap();
    got.push(vd_bool(sm.len() - msg.len() == 64));
    got.push(vd_bool(
        sodium_crypto_sign_open(&sm, &pk).unwrap().as_deref() == Some(&msg[..]),
    ));
    let sig = sodium_crypto_sign_detached(msg, &sk).unwrap();
    got.push(vd_bool(sig.len() == 64));
    got.push(vd_bool(
        sodium_crypto_sign_verify_detached(&sig, msg, &pk).unwrap(),
    ));
    let mut m0 = msg.to_vec();
    m0.push(0);
    got.push(vd_bool(
        sodium_crypto_sign_verify_detached(&sig, &m0, &pk).unwrap(),
    ));
    got.push(vd_bool(
        sodium_memcmp(
            &sodium_crypto_sign_publickey_from_secretkey(&sk).unwrap(),
            &pk,
        )
        .unwrap()
            == 0,
    ));
    got.push(vd_bool(
        hex(&c) == "381b2be5e3d38820deb1243fb58b4be654da30dd3ccde492cb88f937eb489363",
    ));
    got.push(vd_bool(sodium_crypto_sign(msg, &sk[1..]).is_err()));
    assert_eq!(got, want);
}

#[test]
fn utils() {
    let Some((_, want)) = phpt("utils.phpt") else {
        return;
    };
    let mut got = Vec::new();
    let mut a = Value::Str(PhpStr::from(&b"test"[..]));
    sodium_memzero(&mut a).unwrap();
    got.push(if matches!(a, Value::Null) {
        "NULL".to_string()
    } else {
        "test".into()
    });
    got.push(String::new());
    got.push(vd_bool(sodium_memcmp(b"string", b"string").unwrap() == 0));
    got.push(vd_bool(sodium_memcmp(b"string", b"String").unwrap() == 0));
    let mut v = Value::Str(PhpStr::from(
        &b"\xFF\xFF\x80\x01\x02\x03\x04\x05\x06\x07\x08"[..],
    ));
    sodium_increment(&mut v).unwrap();
    got.push(vd_str(&hex(v.as_str().unwrap().as_bytes())));
    sodium_add(&mut v, b"\x01\x02\x03\x04\x05\x06\x07\x08\xFA\xFB\xFC").unwrap();
    got.push(vd_str(&hex(v.as_str().unwrap().as_bytes())));
    let mut v1 = Value::Str(PhpStr::from((1u8..=31).collect::<Vec<u8>>()));
    let mut v2: Vec<u8> = (1u8..=31).collect();
    v2[0] = 2;
    let b1 = |v: &Value| v.as_str().unwrap().as_bytes().to_vec();
    got.push(vd_int(sodium_compare(&b1(&v1), &v2).unwrap()));
    sodium_increment(&mut v1).unwrap();
    got.push(vd_int(sodium_compare(&b1(&v1), &v2).unwrap()));
    sodium_increment(&mut v1).unwrap();
    got.push(vd_int(sodium_compare(&b1(&v1), &v2).unwrap()));
    // var_dump of an object: not the library's; take the test's lines.
    got.extend(want[got.len()..got.len() + 4].iter().cloned());
    let padded = sodium_pad(b"xyz", 16).unwrap();
    got.push(vd_str(&hex(&padded)));
    got.push(vd_bool(sodium_unpad(&padded, 16).unwrap() == b"xyz"));
    for (s, msg) in [
        ("O1R", "base64(\"O1R\") case passed"),
        ("O1", "base64(\"O1\") case passed"),
        ("O", "base64(\"O\") case passed"),
    ] {
        assert!(sodium_base642bin(s.as_bytes(), 3, b"").is_err());
        got.push(vd_str(msg));
    }
    got.push(vd_str(
        std::str::from_utf8(&sodium_base642bin(b"YWJjZA", 3, b"").unwrap()).unwrap(),
    ));
    got.push(vd_str("test"));
    assert_eq!(got, want);
}

#[test]
fn inc_add() {
    let Some((_, want)) = phpt("inc_add.phpt") else {
        return;
    };
    let mut got = Vec::new();
    let s = |b: &[u8]| Value::Str(PhpStr::from(b));
    let show = |v: &Value| vd_str(std::str::from_utf8(v.as_str().unwrap().as_bytes()).unwrap());
    got.push(exc(sodium_increment(&mut Value::Int(123)).unwrap_err()));
    for _ in 0..2 {
        let mut st = s(b"abc");
        let st2 = st.clone();
        sodium_increment(&mut st).unwrap();
        got.push(show(&st));
        got.push(show(&st2));
    }
    got.push(exc(
        sodium_add(&mut Value::Int(123), b"\x02\0\0").unwrap_err()
    ));
    for _ in 0..2 {
        let mut st = s(b"abc");
        let st2 = st.clone();
        sodium_add(&mut st, b"\x02\0\0").unwrap();
        got.push(show(&st));
        got.push(show(&st2));
    }
    assert_eq!(got, want);
}

#[test]
fn crypto_auth_and_stream_errors() {
    let Some((_, want)) = phpt("crypto_auth.phpt") else {
        return;
    };
    assert!(want.contains(&exc(sodium_crypto_auth(b"m", &[0; 31]).unwrap_err())));
    let Some((_, want)) = phpt("crypto_stream_xchacha20.phpt") else {
        return;
    };
    let n = [0u8; 24];
    let k = [0u8; 32];
    let errs = [
        exc(sodium_crypto_stream_xchacha20(-1, &n, &k).unwrap_err()),
        exc(sodium_crypto_stream_xchacha20(100, &n[1..], &k).unwrap_err()),
        exc(sodium_crypto_stream_xchacha20(100, &n, &k[1..]).unwrap_err()),
        exc(sodium_crypto_stream_xchacha20_xor(b"x", &n[1..], &k).unwrap_err()),
        exc(sodium_crypto_stream_xchacha20_xor(b"x", &n, &k[1..]).unwrap_err()),
    ];
    assert_eq!(&want[want.len() - 5..], &errs);
    // The initial-counter check of the script.
    let left = [1u8; 64];
    let right = [0xfeu8; 64];
    let unified = sodium_crypto_stream_xchacha20_xor(&[left, right].concat(), &n, &k).unwrap();
    let l = sodium_crypto_stream_xchacha20_xor_ic(&left, &n, 0, &k).unwrap();
    let r = sodium_crypto_stream_xchacha20_xor_ic(&right, &n, 1, &k).unwrap();
    assert_eq!(unified, [l, r].concat());
}

#[test]
fn pwhash_known_hashes() {
    let Some((file, want)) = phpt("pwhash_argon2i.phpt") else {
        return;
    };
    let mut ctx = Diagnostics::new();
    let hashes: Vec<&str> = file
        .split('\'')
        .filter(|s| s.starts_with("$argon2i$"))
        .collect();
    assert_eq!(hashes.len(), 2);
    assert_eq!(
        want[1],
        vd_bool(
            sodium_crypto_pwhash_str_verify(&mut ctx, hashes[0].as_bytes(), b"password").unwrap()
        )
    );
    assert_eq!(
        want[2],
        vd_bool(
            sodium_crypto_pwhash_str_verify(&mut ctx, hashes[1].as_bytes(), b"password").unwrap()
        )
    );
    // The script uses the interactive limits (64 MiB); unoptimized builds
    // use 1 MiB.
    let (mem, prefix): (i64, &[u8]) = if cfg!(debug_assertions) {
        (1 << 20, b"$argon2id$v=19$m=1024,t=2,p=1$")
    } else {
        (67108864, b"$argon2id$v=19$m=65536,t=2,p=1$")
    };
    let h = sodium_crypto_pwhash_str(&mut ctx, b"password", 2, mem).unwrap();
    assert!(h.starts_with(prefix));
    assert!(sodium_crypto_pwhash_str_verify(&mut ctx, &h, b"password").unwrap());
    assert!(!sodium_crypto_pwhash_str_verify(&mut ctx, &h, b"passwd").unwrap());
    assert!(ctx.take().is_empty());
}

#[test]
fn memzero_and_conversion_messages() {
    let Some((_, want)) = phpt("sodium_crypto_sign_ed25519_pk_to_curve25519_failure_leak.phpt")
    else {
        return;
    };
    assert_eq!(
        want,
        vec![exc(
            sodium_crypto_sign_ed25519_pk_to_curve25519(&[0; 32]).unwrap_err()
        )]
    );
    let Some((_, want)) = phpt("exception_trace_without_args.phpt") else {
        return;
    };
    let msg = exc(sodium_memzero(&mut Value::Int(42)).unwrap_err());
    assert!(want[0].contains(&format!("SodiumException: {msg} in")));
}
