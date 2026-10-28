//! Replays `fixtures/sodium_php85.txt`, generated once on PHP 8.5
//! (libsodium 1.0.18) by `fixtures/sodium_driver.php` (see that file for
//! the line format). Results with random parts (`sodium_crypto_pwhash_str`
//! and the scrypt `_str`) are compared by format; the lines that follow
//! them check verification of PHP's strings.

use goro_ext_sodium::*;
use goro_runtime::{Diagnostics, PhpArray, PhpStr, Value};

#[derive(Clone, Debug)]
enum Arg {
    Str(Vec<u8>),
    Int(i64),
    Null,
    Array,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| {
            u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap_or_else(|_| panic!("bad hex {s}"))
        })
        .collect()
}

fn parse_arg(t: &str) -> Arg {
    match t.as_bytes()[0] {
        b's' => Arg::Str(unhex(&t[1..])),
        b'i' => Arg::Int(t[1..].parse().unwrap()),
        b'n' => Arg::Null,
        b'a' => Arg::Array,
        _ => panic!("bad token {t}"),
    }
}

fn to_value(a: &Arg) -> Value {
    match a {
        Arg::Str(s) => Value::Str(PhpStr::from(s.clone())),
        Arg::Int(i) => Value::Int(*i),
        Arg::Null => Value::Null,
        Arg::Array => Value::Array(PhpArray::new()),
    }
}

fn value_token(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("s{}", hex(s.as_bytes())),
        Value::Int(i) => format!("i{i}"),
        Value::Null => "n".into(),
        Value::Array(_) => "a".into(),
        _ => "?".into(),
    }
}

enum Out {
    Str(Vec<u8>),
    Bool(bool),
    Int(i64),
    Void,
    List(Vec<Out>),
}

fn enc(o: &Out) -> String {
    match o {
        Out::Str(s) => format!("S{}", hex(s)),
        Out::Bool(b) => {
            if *b {
                "B1".into()
            } else {
                "B0".into()
            }
        }
        Out::Int(i) => format!("I{i}"),
        Out::Void => "V".into(),
        Out::List(v) => format!("A{}", v.iter().map(enc).collect::<Vec<_>>().join(",")),
    }
}

fn opt(r: SodiumResult<Option<Vec<u8>>>) -> SodiumResult<Out> {
    r.map(|o| match o {
        Some(v) => Out::Str(v),
        None => Out::Bool(false),
    })
}

fn st(r: SodiumResult<Vec<u8>>) -> SodiumResult<Out> {
    r.map(Out::Str)
}

fn pair(r: SodiumResult<(Vec<u8>, Vec<u8>)>) -> SodiumResult<Out> {
    r.map(|(a, b)| Out::List(vec![Out::Str(a), Out::Str(b)]))
}

struct Call<'a> {
    args: &'a [Arg],
}

impl Call<'_> {
    fn s(&self, i: usize) -> &[u8] {
        match &self.args[i] {
            Arg::Str(s) => s,
            a => panic!("arg {i} not a string: {a:?}"),
        }
    }
    fn s_or<'b>(&'b self, i: usize, d: &'b [u8]) -> &'b [u8] {
        if i < self.args.len() { self.s(i) } else { d }
    }
    fn n(&self, i: usize) -> i64 {
        match &self.args[i] {
            Arg::Int(n) => *n,
            a => panic!("arg {i} not an int: {a:?}"),
        }
    }
    fn n_or(&self, i: usize, d: i64) -> i64 {
        if i < self.args.len() { self.n(i) } else { d }
    }
}

/// Runs a function; `refv` is the by-reference first argument, if any.
fn run(name: &str, c: &Call, refv: &mut Value, ctx: &mut Diagnostics) -> Option<SodiumResult<Out>> {
    Some(match name {
        "sodium_bin2hex" => Ok(Out::Str(sodium_bin2hex(c.s(0)))),
        "sodium_hex2bin" => st(sodium_hex2bin(c.s(0), c.s_or(1, b""))),
        "sodium_bin2base64" => st(sodium_bin2base64(c.s(0), c.n(1))),
        "sodium_base642bin" => st(sodium_base642bin(c.s(0), c.n(1), c.s_or(2, b""))),
        "sodium_compare" => sodium_compare(c.s(0), c.s(1)).map(Out::Int),
        "sodium_memcmp" => sodium_memcmp(c.s(0), c.s(1)).map(Out::Int),
        "sodium_increment" => sodium_increment(refv).map(|_| Out::Void),
        "sodium_add" => sodium_add(refv, &arg_string(&c.args[1])).map(|_| Out::Void),
        "sodium_memzero" => sodium_memzero(refv).map(|_| Out::Void),
        "sodium_pad" => st(sodium_pad(c.s(0), c.n(1))),
        "sodium_unpad" => st(sodium_unpad(c.s(0), c.n(1))),
        "sodium_crypto_generichash" => st(sodium_crypto_generichash(
            c.s(0),
            c.s_or(1, b""),
            c.n_or(2, 32),
        )),
        "sodium_crypto_generichash_init" => st(sodium_crypto_generichash_init(
            c.s_or(0, b""),
            c.n_or(1, 32),
        )),
        "sodium_crypto_generichash_update" => {
            sodium_crypto_generichash_update(refv, c.s(1)).map(Out::Bool)
        }
        "sodium_crypto_generichash_final" => {
            st(sodium_crypto_generichash_final(refv, c.n_or(1, 32)))
        }
        "sodium_crypto_shorthash" => st(sodium_crypto_shorthash(c.s(0), c.s(1))),
        "sodium_crypto_kdf_derive_from_key" => st(sodium_crypto_kdf_derive_from_key(
            c.n(0),
            c.n(1),
            c.s(2),
            c.s(3),
        )),
        "sodium_crypto_auth" => st(sodium_crypto_auth(c.s(0), c.s(1))),
        "sodium_crypto_auth_verify" => {
            sodium_crypto_auth_verify(c.s(0), c.s(1), c.s(2)).map(Out::Bool)
        }
        "sodium_crypto_secretbox" => st(sodium_crypto_secretbox(c.s(0), c.s(1), c.s(2))),
        "sodium_crypto_secretbox_open" => opt(sodium_crypto_secretbox_open(c.s(0), c.s(1), c.s(2))),
        "sodium_crypto_stream" => st(sodium_crypto_stream(c.n(0), c.s(1), c.s(2))),
        "sodium_crypto_stream_xor" => st(sodium_crypto_stream_xor(c.s(0), c.s(1), c.s(2))),
        "sodium_crypto_stream_xchacha20" => {
            st(sodium_crypto_stream_xchacha20(c.n(0), c.s(1), c.s(2)))
        }
        "sodium_crypto_stream_xchacha20_xor" => {
            st(sodium_crypto_stream_xchacha20_xor(c.s(0), c.s(1), c.s(2)))
        }
        "sodium_crypto_stream_xchacha20_xor_ic" => st(sodium_crypto_stream_xchacha20_xor_ic(
            c.s(0),
            c.s(1),
            c.n(2),
            c.s(3),
        )),
        "sodium_crypto_aead_chacha20poly1305_encrypt" => st(
            sodium_crypto_aead_chacha20poly1305_encrypt(c.s(0), c.s(1), c.s(2), c.s(3)),
        ),
        "sodium_crypto_aead_chacha20poly1305_decrypt" => opt(
            sodium_crypto_aead_chacha20poly1305_decrypt(c.s(0), c.s(1), c.s(2), c.s(3)),
        ),
        "sodium_crypto_aead_chacha20poly1305_ietf_encrypt" => st(
            sodium_crypto_aead_chacha20poly1305_ietf_encrypt(c.s(0), c.s(1), c.s(2), c.s(3)),
        ),
        "sodium_crypto_aead_chacha20poly1305_ietf_decrypt" => opt(
            sodium_crypto_aead_chacha20poly1305_ietf_decrypt(c.s(0), c.s(1), c.s(2), c.s(3)),
        ),
        "sodium_crypto_aead_xchacha20poly1305_ietf_encrypt" => st(
            sodium_crypto_aead_xchacha20poly1305_ietf_encrypt(c.s(0), c.s(1), c.s(2), c.s(3)),
        ),
        "sodium_crypto_aead_xchacha20poly1305_ietf_decrypt" => opt(
            sodium_crypto_aead_xchacha20poly1305_ietf_decrypt(c.s(0), c.s(1), c.s(2), c.s(3)),
        ),
        "sodium_crypto_aead_aes256gcm_encrypt" => st(sodium_crypto_aead_aes256gcm_encrypt(
            c.s(0),
            c.s(1),
            c.s(2),
            c.s(3),
        )),
        "sodium_crypto_aead_aes256gcm_decrypt" => opt(sodium_crypto_aead_aes256gcm_decrypt(
            c.s(0),
            c.s(1),
            c.s(2),
            c.s(3),
        )),
        "sodium_crypto_aead_aes256gcm_is_available" => {
            Ok(Out::Bool(sodium_crypto_aead_aes256gcm_is_available()))
        }
        "sodium_crypto_scalarmult" => st(sodium_crypto_scalarmult(c.s(0), c.s(1))),
        "sodium_crypto_scalarmult_base" => st(sodium_crypto_scalarmult_base(c.s(0))),
        "sodium_crypto_box_seed_keypair" => st(sodium_crypto_box_seed_keypair(c.s(0))),
        "sodium_crypto_box_secretkey" => st(sodium_crypto_box_secretkey(c.s(0))),
        "sodium_crypto_box_publickey" => st(sodium_crypto_box_publickey(c.s(0))),
        "sodium_crypto_box_publickey_from_secretkey" => {
            st(sodium_crypto_box_publickey_from_secretkey(c.s(0)))
        }
        "sodium_crypto_box_keypair_from_secretkey_and_publickey" => st(
            sodium_crypto_box_keypair_from_secretkey_and_publickey(c.s(0), c.s(1)),
        ),
        "sodium_crypto_box" => st(sodium_crypto_box(c.s(0), c.s(1), c.s(2))),
        "sodium_crypto_box_open" => opt(sodium_crypto_box_open(c.s(0), c.s(1), c.s(2))),
        "sodium_crypto_box_seal" => st(sodium_crypto_box_seal(c.s(0), c.s(1))),
        "sodium_crypto_box_seal_open" => opt(sodium_crypto_box_seal_open(c.s(0), c.s(1))),
        "sodium_crypto_kx_seed_keypair" => st(sodium_crypto_kx_seed_keypair(c.s(0))),
        "sodium_crypto_kx_publickey" => st(sodium_crypto_kx_publickey(c.s(0))),
        "sodium_crypto_kx_secretkey" => st(sodium_crypto_kx_secretkey(c.s(0))),
        "sodium_crypto_kx_client_session_keys" => {
            pair(sodium_crypto_kx_client_session_keys(c.s(0), c.s(1)))
        }
        "sodium_crypto_kx_server_session_keys" => {
            pair(sodium_crypto_kx_server_session_keys(c.s(0), c.s(1)))
        }
        "sodium_crypto_sign_seed_keypair" => st(sodium_crypto_sign_seed_keypair(c.s(0))),
        "sodium_crypto_sign_secretkey" => st(sodium_crypto_sign_secretkey(c.s(0))),
        "sodium_crypto_sign_publickey" => st(sodium_crypto_sign_publickey(c.s(0))),
        "sodium_crypto_sign_publickey_from_secretkey" => {
            st(sodium_crypto_sign_publickey_from_secretkey(c.s(0)))
        }
        "sodium_crypto_sign_keypair_from_secretkey_and_publickey" => st(
            sodium_crypto_sign_keypair_from_secretkey_and_publickey(c.s(0), c.s(1)),
        ),
        "sodium_crypto_sign_ed25519_pk_to_curve25519" => {
            st(sodium_crypto_sign_ed25519_pk_to_curve25519(c.s(0)))
        }
        "sodium_crypto_sign_ed25519_sk_to_curve25519" => {
            st(sodium_crypto_sign_ed25519_sk_to_curve25519(c.s(0)))
        }
        "sodium_crypto_sign" => st(sodium_crypto_sign(c.s(0), c.s(1))),
        "sodium_crypto_sign_open" => opt(sodium_crypto_sign_open(c.s(0), c.s(1))),
        "sodium_crypto_sign_detached" => st(sodium_crypto_sign_detached(c.s(0), c.s(1))),
        "sodium_crypto_sign_verify_detached" => {
            sodium_crypto_sign_verify_detached(c.s(0), c.s(1), c.s(2)).map(Out::Bool)
        }
        "sodium_crypto_core_ristretto255_from_hash" => {
            st(sodium_crypto_core_ristretto255_from_hash(c.s(0)))
        }
        "sodium_crypto_core_ristretto255_is_valid_point" => {
            sodium_crypto_core_ristretto255_is_valid_point(c.s(0)).map(Out::Bool)
        }
        "sodium_crypto_core_ristretto255_add" => {
            st(sodium_crypto_core_ristretto255_add(c.s(0), c.s(1)))
        }
        "sodium_crypto_core_ristretto255_sub" => {
            st(sodium_crypto_core_ristretto255_sub(c.s(0), c.s(1)))
        }
        "sodium_crypto_core_ristretto255_scalar_negate" => {
            st(sodium_crypto_core_ristretto255_scalar_negate(c.s(0)))
        }
        "sodium_crypto_core_ristretto255_scalar_complement" => {
            st(sodium_crypto_core_ristretto255_scalar_complement(c.s(0)))
        }
        "sodium_crypto_core_ristretto255_scalar_invert" => {
            st(sodium_crypto_core_ristretto255_scalar_invert(c.s(0)))
        }
        "sodium_crypto_core_ristretto255_scalar_add" => {
            st(sodium_crypto_core_ristretto255_scalar_add(c.s(0), c.s(1)))
        }
        "sodium_crypto_core_ristretto255_scalar_sub" => {
            st(sodium_crypto_core_ristretto255_scalar_sub(c.s(0), c.s(1)))
        }
        "sodium_crypto_core_ristretto255_scalar_mul" => {
            st(sodium_crypto_core_ristretto255_scalar_mul(c.s(0), c.s(1)))
        }
        "sodium_crypto_core_ristretto255_scalar_reduce" => {
            st(sodium_crypto_core_ristretto255_scalar_reduce(c.s(0)))
        }
        "sodium_crypto_scalarmult_ristretto255" => {
            st(sodium_crypto_scalarmult_ristretto255(c.s(0), c.s(1)))
        }
        "sodium_crypto_scalarmult_ristretto255_base" => {
            st(sodium_crypto_scalarmult_ristretto255_base(c.s(0)))
        }
        "sodium_crypto_secretstream_xchacha20poly1305_init_pull" => st(
            sodium_crypto_secretstream_xchacha20poly1305_init_pull(c.s(0), c.s(1)),
        ),
        "sodium_crypto_secretstream_xchacha20poly1305_init_push" => pair(
            sodium_crypto_secretstream_xchacha20poly1305_init_push(c.s(0)),
        ),
        "sodium_crypto_secretstream_xchacha20poly1305_push" => {
            st(sodium_crypto_secretstream_xchacha20poly1305_push(
                refv,
                &arg_string(&c.args[1]),
                c.s_or(2, b""),
                c.n_or(3, 0),
            ))
        }
        "sodium_crypto_secretstream_xchacha20poly1305_pull" => {
            sodium_crypto_secretstream_xchacha20poly1305_pull(
                refv,
                &arg_string(&c.args[1]),
                c.s_or(2, b""),
            )
            .map(|r| match r {
                Some((m, t)) => Out::List(vec![Out::Str(m), Out::Int(t)]),
                None => Out::Bool(false),
            })
        }
        "sodium_crypto_secretstream_xchacha20poly1305_rekey" => {
            sodium_crypto_secretstream_xchacha20poly1305_rekey(refv).map(|_| Out::Void)
        }
        "sodium_crypto_pwhash" => st(sodium_crypto_pwhash(
            ctx,
            c.n(0),
            c.s(1),
            c.s(2),
            c.n(3),
            c.n(4),
            c.n_or(5, 2),
        )),
        "sodium_crypto_pwhash_str" => st(sodium_crypto_pwhash_str(ctx, c.s(0), c.n(1), c.n(2))),
        "sodium_crypto_pwhash_str_verify" => {
            sodium_crypto_pwhash_str_verify(ctx, c.s(0), c.s(1)).map(Out::Bool)
        }
        "sodium_crypto_pwhash_str_needs_rehash" => {
            sodium_crypto_pwhash_str_needs_rehash(c.s(0), c.n(1), c.n(2)).map(Out::Bool)
        }
        "sodium_crypto_pwhash_scryptsalsa208sha256" => st(
            sodium_crypto_pwhash_scryptsalsa208sha256(ctx, c.n(0), c.s(1), c.s(2), c.n(3), c.n(4)),
        ),
        "sodium_crypto_pwhash_scryptsalsa208sha256_str" => st(
            sodium_crypto_pwhash_scryptsalsa208sha256_str(ctx, c.s(0), c.n(1), c.n(2)),
        ),
        "sodium_crypto_pwhash_scryptsalsa208sha256_str_verify" => {
            sodium_crypto_pwhash_scryptsalsa208sha256_str_verify(ctx, c.s(0), c.s(1)).map(Out::Bool)
        }
        "sodium_crypto_aead_chacha20poly1305_keygen"
        | "sodium_crypto_aead_chacha20poly1305_ietf_keygen"
        | "sodium_crypto_aead_xchacha20poly1305_ietf_keygen"
        | "sodium_crypto_aead_aes256gcm_keygen" => return None,
        _ => return None,
    })
}

/// A `string` parameter as the engine coerces it.
fn arg_string(a: &Arg) -> Vec<u8> {
    match a {
        Arg::Str(s) => s.clone(),
        Arg::Int(i) => i.to_string().into_bytes(),
        _ => panic!("not coercible"),
    }
}

fn len_of(name: &str, c: &Call) -> Option<String> {
    let r: SodiumResult<Vec<usize>> = match name {
        "sodium_crypto_generichash_keygen" => {
            sodium_crypto_generichash_keygen().map(|v| vec![v.len()])
        }
        "sodium_crypto_shorthash_keygen" => sodium_crypto_shorthash_keygen().map(|v| vec![v.len()]),
        "sodium_crypto_kdf_keygen" => sodium_crypto_kdf_keygen().map(|v| vec![v.len()]),
        "sodium_crypto_auth_keygen" => sodium_crypto_auth_keygen().map(|v| vec![v.len()]),
        "sodium_crypto_secretbox_keygen" => sodium_crypto_secretbox_keygen().map(|v| vec![v.len()]),
        "sodium_crypto_stream_keygen" => sodium_crypto_stream_keygen().map(|v| vec![v.len()]),
        "sodium_crypto_stream_xchacha20_keygen" => {
            sodium_crypto_stream_xchacha20_keygen().map(|v| vec![v.len()])
        }
        "sodium_crypto_aead_chacha20poly1305_keygen" => {
            sodium_crypto_aead_chacha20poly1305_keygen().map(|v| vec![v.len()])
        }
        "sodium_crypto_aead_chacha20poly1305_ietf_keygen" => {
            sodium_crypto_aead_chacha20poly1305_ietf_keygen().map(|v| vec![v.len()])
        }
        "sodium_crypto_aead_xchacha20poly1305_ietf_keygen" => {
            sodium_crypto_aead_xchacha20poly1305_ietf_keygen().map(|v| vec![v.len()])
        }
        "sodium_crypto_aead_aes256gcm_keygen" => {
            sodium_crypto_aead_aes256gcm_keygen().map(|v| vec![v.len()])
        }
        "sodium_crypto_box_keypair" => sodium_crypto_box_keypair().map(|v| vec![v.len()]),
        "sodium_crypto_kx_keypair" => sodium_crypto_kx_keypair().map(|v| vec![v.len()]),
        "sodium_crypto_sign_keypair" => sodium_crypto_sign_keypair().map(|v| vec![v.len()]),
        "sodium_crypto_box_seal" => sodium_crypto_box_seal(c.s(0), c.s(1)).map(|v| vec![v.len()]),
        "sodium_crypto_core_ristretto255_random" => {
            sodium_crypto_core_ristretto255_random().map(|v| vec![v.len()])
        }
        "sodium_crypto_core_ristretto255_scalar_random" => {
            sodium_crypto_core_ristretto255_scalar_random().map(|v| vec![v.len()])
        }
        "sodium_crypto_secretstream_xchacha20poly1305_init_push" => {
            sodium_crypto_secretstream_xchacha20poly1305_init_push(c.s(0))
                .map(|(a, b)| vec![a.len(), b.len()])
        }
        "sodium_crypto_secretstream_xchacha20poly1305_keygen" => {
            sodium_crypto_secretstream_xchacha20poly1305_keygen().map(|v| vec![v.len()])
        }
        _ => return None,
    };
    Some(match r {
        Ok(v) => v
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Err(e) => format!("ESodiumException:{e}"),
    })
}

#[test]
fn php85_fixture() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    let mut data = std::fs::read_to_string(format!("{dir}sodium_php85.txt")).unwrap();
    data.push_str(&std::fs::read_to_string(format!("{dir}sodium_php85_2.txt")).unwrap());
    let mut failures = Vec::new();
    let mut checked = 0;
    let mut skipped = 0;
    // Unoptimized builds take minutes over the password hashes (megabytes
    // of Argon2/scrypt memory per case): there, only the cheap ones run,
    // unless SODIUM_FULL_FIXTURE is set. Release builds run everything.
    let full = !cfg!(debug_assertions) || std::env::var_os("SODIUM_FULL_FIXTURE").is_some();
    for line in data.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if !full
            && (parts[0] == "#scrypt"
                || (parts[0].starts_with("sodium_crypto_pwhash")
                    && !parts[0].ends_with("_needs_rehash")))
        {
            skipped += 1;
            continue;
        }
        if parts[0] == "#scrypt" {
            // The parameter prefix of a `$7$` string for (opslimit, memlimit).
            let lim: Vec<i64> = parts[1].split(' ').map(|x| x.parse().unwrap()).collect();
            let mut ctx = Diagnostics::new();
            let h = sodium_crypto_pwhash_scryptsalsa208sha256_str(&mut ctx, b"pw", lim[0], lim[1])
                .unwrap();
            checked += 1;
            if &h[..14] != parts[3].as_bytes() {
                failures.push(format!(
                    "scrypt params {}: got {}",
                    parts[1],
                    String::from_utf8_lossy(&h[..14])
                ));
            }
            continue;
        }
        if parts[0] == "#len" {
            let args: Vec<Arg> = if parts[2].is_empty() {
                vec![]
            } else {
                parts[2].split(' ').map(parse_arg).collect()
            };
            let c = Call { args: &args };
            match len_of(parts[1], &c) {
                Some(got) => {
                    checked += 1;
                    if got != parts[4] {
                        failures.push(format!("{}: len got {got}, want {}", parts[1], parts[4]));
                    }
                }
                None => failures.push(format!("unhandled #len {}", parts[1])),
            }
            continue;
        }
        let (name, args_s, want) = (parts[0], parts[1], parts[3]);
        let args: Vec<Arg> = if args_s.is_empty() {
            vec![]
        } else {
            args_s.split(' ').map(parse_arg).collect()
        };
        let c = Call { args: &args };
        let mut refv = args.first().map(to_value).unwrap_or_default();
        let mut ctx = Diagnostics::new();
        let Some(res) = run(name, &c, &mut refv, &mut ctx) else {
            failures.push(format!("unhandled {name}"));
            continue;
        };
        let mut got = String::new();
        for n in ctx.take() {
            got.push('W');
            got.push_str(&n.message);
            got.push('|');
        }
        match &res {
            Ok(o) => got.push_str(&enc(o)),
            Err(SodiumError::Exception(m)) => got.push_str(&format!("ESodiumException:{m}")),
            Err(e) => got.push_str(&format!("E?{e}")),
        }
        let by_ref = matches!(
            name,
            "sodium_increment"
                | "sodium_add"
                | "sodium_memzero"
                | "sodium_crypto_generichash_update"
                | "sodium_crypto_generichash_final"
                | "sodium_crypto_secretstream_xchacha20poly1305_push"
                | "sodium_crypto_secretstream_xchacha20poly1305_pull"
                | "sodium_crypto_secretstream_xchacha20poly1305_rekey"
        );
        if by_ref {
            got.push_str(" R");
            got.push_str(&value_token(&refv));
        }
        checked += 1;
        let result_part = |s: &str| -> usize { s.rfind('|').map(|i| i + 1).unwrap_or(0) };
        let (wr, gr) = (result_part(want), result_part(&got));
        let ok = if name == "sodium_crypto_box_seal" && want.starts_with('S') {
            // Random ephemeral key: same length.
            got.len() == want.len() && got.starts_with('S')
        } else if (name == "sodium_crypto_pwhash_str"
            || name == "sodium_crypto_pwhash_scryptsalsa208sha256_str")
            && want[wr..].starts_with('S')
            && got[gr..].starts_with('S')
        {
            // Random salt: compare the parameter prefix and the length.
            let w = unhex(&want[wr + 1..]);
            let g = unhex(&got[gr + 1..]);
            let prefix = |s: &[u8]| -> Vec<u8> {
                if s.starts_with(b"$7$") {
                    s[..14].to_vec()
                } else {
                    let mut n = 0;
                    let mut out = Vec::new();
                    for &b in s {
                        out.push(b);
                        if b == b'$' {
                            n += 1;
                            if n == 4 {
                                break;
                            }
                        }
                    }
                    out
                }
            };
            w.len() == g.len() && prefix(&w) == prefix(&g) && want[..wr] == got[..gr]
        } else {
            got == want
        };
        if !ok {
            let short = |s: &str| {
                if s.len() > 300 {
                    format!("{}…", &s[..300])
                } else {
                    s.to_string()
                }
            };
            failures.push(format!(
                "{name}({}):\n   got  {}\n   want {}",
                short(args_s),
                short(&got),
                short(want)
            ));
        }
    }
    let total = failures.len();
    for f in failures.iter().take(60) {
        eprintln!("{f}");
    }
    if skipped > 0 {
        eprintln!("{skipped} password-hashing cases skipped in this unoptimized build");
    }
    assert!(total == 0, "{total} mismatches out of {checked} cases");
}
