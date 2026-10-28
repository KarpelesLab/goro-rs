//! Self-consistency: everything compressed decompresses back, whatever the
//! parameters, flush pattern or input chunking, and damaged input never
//! panics the decoder.

mod common;

use common::*;
use goro_ext_zlib::*;

fn wraps() -> [(Wrap, InflateWrap); 3] {
    [
        (Wrap::Raw, InflateWrap::Raw),
        (Wrap::Zlib, InflateWrap::Zlib),
        (Wrap::Gzip, InflateWrap::Gzip),
    ]
}

/// Decodes with the given input chunk size and output limit per call.
fn inflate_chunked(
    data: &[u8],
    wrap: InflateWrap,
    chunk: usize,
    out_step: usize,
) -> Option<Vec<u8>> {
    let mut inf = Inflater::new(wrap, 15);
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let end = (pos + chunk).min(data.len());
        let (n, st) = inf.inflate(&data[pos..end], &mut out, out_step, InflateFlush::None);
        pos += n;
        match st {
            InflateStatus::StreamEnd => return Some(out),
            InflateStatus::DataError(_) | InflateStatus::NeedDict => return None,
            _ => {
                if pos >= data.len() && n == 0 && st == InflateStatus::BufError {
                    return None;
                }
            }
        }
    }
}

#[test]
fn roundtrip_parameters() {
    let inputs = inputs();
    let names = [
        "empty",
        "hello",
        "abc100",
        "allbytes",
        "rand1000",
        "text5000",
        "runs200k",
        "period32507",
        "fib",
        "x263",
    ];
    for name in names {
        // Keep unoptimised test runs short.
        let full = &inputs[name];
        let data = if cfg!(debug_assertions) {
            &full[..full.len().min(8000)]
        } else {
            &full[..]
        };
        for (w, iw) in wraps() {
            for level in [0, 1, 3, 4, 6, 9] {
                for strategy in [
                    Strategy::Default,
                    Strategy::Filtered,
                    Strategy::HuffmanOnly,
                    Strategy::Rle,
                    Strategy::Fixed,
                ] {
                    for mem in [1, 8] {
                        for window in [9, 15] {
                            let mut d = Deflater::new(DeflateParams {
                                level,
                                window_bits: window,
                                mem_level: mem,
                                strategy,
                                wrap: w,
                            })
                            .unwrap();
                            let mut c = Vec::new();
                            assert_eq!(
                                d.deflate(data, Flush::Finish, &mut c),
                                DeflateStatus::StreamEnd
                            );
                            let back = inflate_chunked(&c, iw, usize::MAX, usize::MAX);
                            assert!(
                                back.as_deref() == Some(data),
                                "{name} {w:?} level {level} {strategy:?} mem {mem} window {window}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn roundtrip_flush_patterns() {
    let inputs = inputs();
    let data = &inputs["mixed400k"][..if cfg!(debug_assertions) {
        30_000
    } else {
        120_000
    }];
    let mut r = Lcg::new(99);
    for (w, iw) in wraps() {
        for level in [0, 1, 6] {
            for _ in 0..6 {
                let mut d = Deflater::new(DeflateParams {
                    level,
                    wrap: w,
                    ..Default::default()
                })
                .unwrap();
                let mut c = Vec::new();
                let mut pos = 0;
                while pos < data.len() {
                    let n = (r.next() as usize * 7) % 9000 + 1;
                    let end = (pos + n).min(data.len());
                    let f = Flush::from_i64((r.next() % 6) as i64).unwrap();
                    let f = if f == Flush::Finish { Flush::Sync } else { f };
                    // Limited output space per call, as PHP gives zlib.
                    let mut input = &data[pos..end];
                    loop {
                        let mut avail = 64 + (r.next() as usize % 500);
                        d.deflate_step(&mut input, f, &mut c, &mut avail);
                        if input.is_empty() && avail > 0 {
                            break;
                        }
                    }
                    pos = end;
                }
                d.deflate(b"", Flush::Finish, &mut c);
                for (chunk, step) in [(usize::MAX, usize::MAX), (1, 3), (7, 1), (1000, 300)] {
                    let back = inflate_chunked(&c, iw, chunk, step);
                    assert!(
                        back.as_deref() == Some(data),
                        "{w:?} level {level} chunk {chunk}"
                    );
                }
            }
        }
    }
}

#[test]
fn damaged_input_never_panics() {
    let inputs = inputs();
    let mut r = Lcg::new(1234);
    let sources: Vec<Vec<u8>> = ["text5000", "rand1000", "runs200k", "fib", "hello"]
        .iter()
        .flat_map(|n| {
            let d = &inputs[*n];
            [
                gzdeflate(d, 6, ENCODING_RAW).unwrap(),
                gzcompress(d, 1, ENCODING_DEFLATE).unwrap(),
                gzencode(d, 9, ENCODING_GZIP).unwrap(),
                gzdeflate(d, 0, ENCODING_RAW).unwrap(),
            ]
        })
        .collect();
    for src in &sources {
        for _ in 0..if cfg!(debug_assertions) { 60 } else { 300 } {
            let mut b = src.clone();
            let flips = 1 + r.next() as usize % 4;
            for _ in 0..flips {
                let i = (((r.next() << 15) | r.next()) as usize) % b.len();
                b[i] ^= 1 << (r.next() % 8);
            }
            if r.next().is_multiple_of(4) {
                let cut = (((r.next() << 15) | r.next()) as usize) % b.len();
                b.truncate(cut);
            }
            for f in [gzinflate, gzuncompress, gzdecode, zlib_decode] {
                let _ = f(&b, 0);
                let _ = f(&b, 100);
            }
            for e in [ENCODING_RAW, ENCODING_DEFLATE, ENCODING_GZIP] {
                let mut ctx = inflate_init(e, &InflateOptions::default()).unwrap();
                for ch in b.chunks(1 + r.next() as usize % 50) {
                    let _ = ctx.add(ch, (r.next() % 6) as i64);
                }
            }
            for wrap in [InflateWrap::Raw, InflateWrap::Auto] {
                for wbits in [9, 15] {
                    let mut inf = Inflater::new(wrap, wbits);
                    let mut out = Vec::new();
                    for ch in b.chunks(13) {
                        let (_, st) = inf.inflate(ch, &mut out, 17, InflateFlush::None);
                        if matches!(st, InflateStatus::DataError(_)) {
                            break;
                        }
                    }
                }
            }
        }
    }
    // Arbitrary bytes.
    for len in [0usize, 1, 2, 3, 5, 10, 100, 1000] {
        for _ in 0..200 {
            let b: Vec<u8> = (0..len).map(|_| r.next() as u8).collect();
            for f in [gzinflate, gzuncompress, gzdecode, zlib_decode] {
                let _ = f(&b, 0);
            }
        }
    }
}
