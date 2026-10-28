//! Timing of the operations `tests/fixtures/bench.php` measures on the
//! reference PHP binary: `cargo run --release -p goro-ext-sodium --example bench`.
//! Each line is "name  microseconds-per-call" (best of 5 runs).

use goro_ext_sodium::*;
use goro_runtime::{Diagnostics, PhpStr, Value};
use std::hint::black_box;
use std::time::Instant;

fn bench<F: FnMut()>(name: &str, iters: usize, mut f: F) {
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        for _ in 0..iters {
            f();
        }
        best = best.min(t.elapsed().as_secs_f64() * 1e6 / iters as f64);
    }
    println!("{name:<40} {best:12.2}");
}

fn main() {
    let filter = std::env::args().nth(1);
    let on = |n: &str| filter.as_deref().is_none_or(|f| n.contains(f));
    let mib = vec![0x5au8; 1 << 20];
    let k = [b'k'; 32];
    let n24 = [b'n'; 24];
    let n12 = [b'n'; 12];
    let n8 = [b'n'; 8];
    let m64 = b"0123456789012345678901234567890123456789012345678901234567890123";
    macro_rules! b {
        ($name:expr, $iters:expr, $e:expr) => {
            if on($name) {
                bench($name, $iters, || {
                    black_box($e);
                });
            }
        };
    }
    b!(
        "secretbox 1MiB",
        50,
        sodium_crypto_secretbox(&mib, &n24, &k).unwrap()
    );
    let c = sodium_crypto_secretbox(&mib, &n24, &k).unwrap();
    b!(
        "secretbox_open 1MiB",
        50,
        sodium_crypto_secretbox_open(&c, &n24, &k).unwrap()
    );
    b!(
        "stream_xor (xsalsa20) 1MiB",
        50,
        sodium_crypto_stream_xor(&mib, &n24, &k).unwrap()
    );
    b!(
        "stream_xchacha20_xor 1MiB",
        50,
        sodium_crypto_stream_xchacha20_xor(&mib, &n24, &k).unwrap()
    );
    b!(
        "aead chacha20poly1305 1MiB",
        50,
        sodium_crypto_aead_chacha20poly1305_encrypt(&mib, b"ad", &n8, &k).unwrap()
    );
    b!(
        "aead chacha20poly1305_ietf 1MiB",
        50,
        sodium_crypto_aead_chacha20poly1305_ietf_encrypt(&mib, b"ad", &n12, &k).unwrap()
    );
    b!(
        "aead xchacha20poly1305_ietf 1MiB",
        50,
        sodium_crypto_aead_xchacha20poly1305_ietf_encrypt(&mib, b"ad", &n24, &k).unwrap()
    );
    let c = sodium_crypto_aead_xchacha20poly1305_ietf_encrypt(&mib, b"ad", &n24, &k).unwrap();
    b!(
        "aead xchacha20poly1305_ietf dec 1MiB",
        50,
        sodium_crypto_aead_xchacha20poly1305_ietf_decrypt(&c, b"ad", &n24, &k).unwrap()
    );
    b!(
        "aead aes256gcm 1MiB",
        50,
        sodium_crypto_aead_aes256gcm_encrypt(&mib, b"ad", &n12, &k).unwrap()
    );
    b!(
        "aead aegis128l 1MiB",
        50,
        sodium_crypto_aead_aegis128l_encrypt(&mib, b"ad", &[b'n'; 16], &[b'k'; 16]).unwrap()
    );
    b!(
        "aead aegis256 1MiB",
        50,
        sodium_crypto_aead_aegis256_encrypt(&mib, b"ad", &k, &k).unwrap()
    );
    b!(
        "generichash 1MiB",
        50,
        sodium_crypto_generichash(&mib, b"", 32).unwrap()
    );
    b!(
        "generichash 64B",
        100000,
        sodium_crypto_generichash(m64, b"", 32).unwrap()
    );
    if on("generichash_update 1MiB") {
        let mut st = Value::Str(PhpStr::from(
            sodium_crypto_generichash_init(b"", 32).unwrap(),
        ));
        bench("generichash_update 1MiB", 50, || {
            sodium_crypto_generichash_update(&mut st, &mib).unwrap();
        });
    }
    b!("auth 1MiB", 50, sodium_crypto_auth(&mib, &k).unwrap());
    b!(
        "shorthash 64B",
        100000,
        sodium_crypto_shorthash(m64, b"0123456789abcdef").unwrap()
    );
    let kp = sodium_crypto_sign_seed_keypair(&k).unwrap();
    let sk = sodium_crypto_sign_secretkey(&kp).unwrap();
    let pk = sodium_crypto_sign_publickey(&kp).unwrap();
    b!(
        "sign_detached 64B",
        5000,
        sodium_crypto_sign_detached(b"message", &sk).unwrap()
    );
    let sig = sodium_crypto_sign_detached(b"message", &sk).unwrap();
    b!(
        "sign_verify_detached 64B",
        5000,
        sodium_crypto_sign_verify_detached(&sig, b"message", &pk).unwrap()
    );
    b!(
        "sign_seed_keypair",
        5000,
        sodium_crypto_sign_seed_keypair(&k).unwrap()
    );
    let a = sodium_crypto_box_seed_keypair(&k).unwrap();
    let mut seed_b = n24.to_vec();
    seed_b.extend_from_slice(b"xxxxxxxx");
    let bk = sodium_crypto_box_seed_keypair(&seed_b).unwrap();
    let ab = sodium_crypto_box_keypair_from_secretkey_and_publickey(&a[..32], &bk[32..]).unwrap();
    b!("box 64B", 5000, sodium_crypto_box(m64, &n24, &ab).unwrap());
    b!(
        "box_seal 64B",
        5000,
        sodium_crypto_box_seal(m64, &a[32..]).unwrap()
    );
    b!(
        "scalarmult",
        5000,
        sodium_crypto_scalarmult(&k, &bk[32..]).unwrap()
    );
    b!(
        "scalarmult_base",
        5000,
        sodium_crypto_scalarmult_base(&k).unwrap()
    );
    b!(
        "scalarmult_ristretto255",
        5000,
        sodium_crypto_scalarmult_ristretto255(
            &k,
            &sodium_crypto_scalarmult_ristretto255_base(&k).unwrap()
        )
        .unwrap()
    );
    let mut ctx = Diagnostics::new();
    b!(
        "pwhash argon2id interactive",
        3,
        sodium_crypto_pwhash(&mut ctx, 32, b"password", &[b's'; 16], 2, 67108864, 2).unwrap()
    );
    b!(
        "pwhash argon2i interactive",
        3,
        sodium_crypto_pwhash(&mut ctx, 32, b"password", &[b's'; 16], 4, 67108864, 1).unwrap()
    );
    b!(
        "pwhash scrypt interactive",
        3,
        sodium_crypto_pwhash_scryptsalsa208sha256(&mut ctx, 32, b"password", &k, 524288, 16777216)
            .unwrap()
    );
    b!("bin2hex 1MiB", 50, sodium_bin2hex(&mib));
    let hx = sodium_bin2hex(&mib);
    b!("hex2bin 1MiB", 50, sodium_hex2bin(&hx, b"").unwrap());
    b!("bin2base64 1MiB", 50, sodium_bin2base64(&mib, 1).unwrap());
    let bx = sodium_bin2base64(&mib, 1).unwrap();
    b!(
        "base642bin 1MiB",
        50,
        sodium_base642bin(&bx, 1, b"").unwrap()
    );
}
