//! Library-level timings of the key operations behind openssl_sign() and
//! friends: `cargo run --release -p goro-ext-openssl --example openssl_bench -- KEY.pem`.

use goro_ext_openssl::digest::{HashAlgorithm, Md};
use goro_ext_openssl::key;
use std::time::Instant;

fn time<T>(name: &str, n: u32, mut f: impl FnMut() -> T) {
    let t = Instant::now();
    for _ in 0..n {
        std::hint::black_box(f());
    }
    println!(
        "{:<28} {:>10.1} us",
        name,
        t.elapsed().as_secs_f64() * 1e6 / n as f64
    );
}

fn main() {
    let path = std::env::args().nth(1).expect("key path");
    let pem = std::fs::read(path).expect("read key");
    time("load private pem", 50, || {
        key::load_private_pem(&pem, None).unwrap()
    });
    let k = key::load_private_pem(&pem, None).unwrap();
    let md = Md::H(HashAlgorithm::Sha256);
    let data = vec![b'x'; 1024];
    time("rsa sign", 100, || k.sign(Some(md), &data, false).unwrap());
    let sig = k.sign(Some(md), &data, false).unwrap();
    let pk = k.public_only();
    time("rsa verify", 1000, || {
        pk.verify(Some(md), &data, &sig, false).unwrap()
    });
    let sk = k.rsa_private_key().unwrap();
    let block = vec![1u8; 200];
    time("raw_private", 100, || sk.raw_private(&block).unwrap());
    time("raw_public", 1000, || {
        pk.rsa_public_key().unwrap().raw_public(&sig).unwrap()
    });
}
