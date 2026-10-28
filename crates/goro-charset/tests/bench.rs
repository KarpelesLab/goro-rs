//! Timings of the cases of tools/charset/bench/mb_bench.php (not a test):
//! `cargo test --release --test bench -- --ignored --nocapture`.

use goro_charset::{Encoding, Subst, check_encoding, convert, ops, scrub};
use std::hint::black_box;
use std::time::Instant;

/// Whole repetitions of `unit`, at least 1 MiB.
fn input(unit: &[u8]) -> Vec<u8> {
    unit.repeat((1usize << 20).div_ceil(unit.len()))
}

fn crc32(b: &[u8]) -> u32 {
    let mut c = !0u32;
    for &x in b {
        c ^= x as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

enum Out {
    Bytes(Vec<u8>),
    Text(String),
}

fn time(name: &str, f: impl Fn() -> Out) {
    let mut best = f64::INFINITY;
    let mut out = None;
    for _ in 0..20 {
        let t = Instant::now();
        let v = black_box(f());
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
        out = Some(v);
    }
    let sig = match out.unwrap() {
        Out::Bytes(b) => format!("{}:{}", b.len(), crc32(&b)),
        Out::Text(s) => s,
    };
    println!("{name:<40} {best:8.3} ms  {sig}");
}

#[test]
#[ignore]
fn bench() {
    use Encoding::*;
    let sub = Subst::default();
    let ascii = input(b"The quick brown fox jumps over the lazy dog. 0123456789\n");
    let mixed = input(
        "Hello, world! Déjà vu — naïve café. 日本語のテキスト。Ελληνικά κείμενα. ".as_bytes(),
    );
    let cjk = input("日本語のテキストです。漢字とかなカナ、".as_bytes());
    let sjis = convert(&cjk, Sjis, Utf8, &sub).bytes;
    let latin1 = convert(
        &input("Déjà vu, naïve café, Ærøskøbing. ".as_bytes()),
        Iso8859_1,
        Utf8,
        &sub,
    )
    .bytes;
    let utf16 = convert(&mixed, Utf16Le, Utf8, &sub).bytes;
    let bad = input(b"abc\xffdef\xe3\x81ghi");
    let b = |v: bool| Out::Text(if v { "true".into() } else { "false".into() });
    time("check_encoding UTF-8 ascii", || {
        b(check_encoding(&ascii, Utf8))
    });
    time("check_encoding UTF-8 mixed", || {
        b(check_encoding(&mixed, Utf8))
    });
    time("strlen UTF-8 mixed", || {
        Out::Text(ops::strlen(&mixed, Utf8).to_string())
    });
    time("substr UTF-8 mixed (middle 1000)", || {
        Out::Bytes(ops::substr(&mixed, 300000, Some(1000), Utf8, &sub))
    });
    time("strpos UTF-8 mixed (miss)", || {
        Out::Text(format!(
            "{:?}",
            ops::strpos(&mixed, b"zzz", 0, Utf8).unwrap()
        ))
    });
    time("strpos UTF-8 mixed (near end)", || {
        let needle = "κείμενα. ".as_bytes();
        Out::Text(format!(
            "{:?}",
            ops::strpos(&mixed, needle, 640000, Utf8).unwrap()
        ))
    });
    time("convert UTF-8 -> UTF-16LE", || {
        Out::Bytes(convert(&mixed, Utf16Le, Utf8, &sub).bytes)
    });
    time("convert UTF-16LE -> UTF-8", || {
        Out::Bytes(convert(&utf16, Utf8, Utf16Le, &sub).bytes)
    });
    time("convert SJIS -> UTF-8", || {
        Out::Bytes(convert(&sjis, Utf8, Sjis, &sub).bytes)
    });
    time("convert UTF-8 -> SJIS", || {
        Out::Bytes(convert(&cjk, Sjis, Utf8, &sub).bytes)
    });
    time("convert ISO-8859-1 -> UTF-8", || {
        Out::Bytes(convert(&latin1, Utf8, Iso8859_1, &sub).bytes)
    });
    time("scrub UTF-8 (errors)", || {
        Out::Bytes(scrub(&bad, Utf8, &sub))
    });
    time("strtoupper UTF-8 ascii", || {
        Out::Bytes(ops::strtoupper(&ascii, Utf8, &sub))
    });
    time("strtoupper UTF-8 mixed", || {
        Out::Bytes(ops::strtoupper(&mixed, Utf8, &sub))
    });
    time("strwidth UTF-8 mixed", || {
        Out::Text(ops::strwidth(&mixed, Utf8).to_string())
    });
    time("detect [ASCII,UTF-8,SJIS,EUC-JP] mixed", || {
        let r = ops::detect::detect_encoding(&mixed, &[Ascii, Utf8, Sjis, EucJp], false);
        Out::Text(r.map(|e| e.name()).unwrap_or("false").to_string())
    });
    time("detect [UTF-8,SJIS,EUC-JP] sjis", || {
        let r = ops::detect::detect_encoding(&sjis, &[Utf8, Sjis, EucJp], false);
        Out::Text(r.map(|e| e.name()).unwrap_or("false").to_string())
    });
}
