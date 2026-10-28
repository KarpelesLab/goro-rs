//! Hand-written checks of what php-src's ext/zlib tests expect (each case
//! cites its test). Tests whose expectations are binary are read from the
//! test-suite checkout when it is present.

use goro_ext_zlib::filter::*;
use goro_ext_zlib::*;

fn warn(m: &str) -> Result<Vec<u8>, ZlibError> {
    Err(ZlibError::Warning(m.into()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

#[test]
fn gzencode_variation2() {
    // ext/zlib/tests/gzencode_variation2.phpt
    let d = b"A small string to encode\n";
    let gz = "1f8b0800000000000003735428ce4dccc951282e29cacc4b5728c95748cd4bce4f49e50200d7739de519000000";
    assert_eq!(hex(&gzencode(d, -1, ENCODING_GZIP).unwrap()), gz);
    assert_eq!(
        hex(&gzencode(d, -1, ENCODING_DEFLATE).unwrap()),
        "789c735428ce4dccc951282e29cacc4b5728c95748cd4bce4f49e50200735808cd"
    );
}

#[test]
fn gzinflate_length() {
    // ext/zlib/tests/gzinflate_length.phpt
    let packed = gzdeflate(b"aaaaaaaaaaaaaaa", -1, ENCODING_RAW).unwrap();
    assert_eq!(packed.len(), 5);
    assert_eq!(gzinflate(&packed, 15).unwrap(), b"aaaaaaaaaaaaaaa");
    assert_eq!(gzinflate(&packed, 150).unwrap(), b"aaaaaaaaaaaaaaa");
    assert_eq!(gzinflate(&packed, 1), warn("insufficient memory"));
}

#[test]
fn length_errors() {
    // ext/zlib/tests/gzinflate_error1.phpt, gzuncompress_error1.phpt
    let c = gzcompress(b"string_val", -1, ENCODING_DEFLATE).unwrap();
    assert_eq!(gzinflate(&c, 9), warn("data error"));
    assert_eq!(gzuncompress(&c, 9), warn("insufficient memory"));
    assert_eq!(gzuncompress(b"123", 0), warn("data error"));
    // ext/zlib/tests/gzinflate-bug42663.phpt
    let mut s = Vec::new();
    for i in 0..30000 {
        s.extend(format!("{i} ").bytes());
    }
    assert_eq!(s.len(), 168890);
    let d = gzdeflate(&s, 9, ENCODING_RAW).unwrap();
    assert!((66700..66800).contains(&d.len()));
    assert_eq!(gzinflate(&d[..65535], 0), warn("data error"));
}

#[test]
fn argument_errors() {
    // ext/zlib/tests/gzcompress_error1.phpt, gzencode_invalid.phpt
    assert_eq!(
        gzcompress(b"string_val", 99, ENCODING_DEFLATE),
        Err(ZlibError::ValueError(
            "gzcompress(): Argument #2 ($level) must be between -1 and 9".into()
        ))
    );
    assert_eq!(
        gzencode(b"", 1, 100),
        Err(ZlibError::ValueError(
            "gzencode(): Argument #3 ($encoding) must be one of ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP, or ZLIB_ENCODING_DEFLATE".into()
        ))
    );
    // ext/zlib/tests/deflate_init_error.phpt
    let msg = |r: Result<DeflateContext, ZlibError>| match r {
        Err(ZlibError::ValueError(m)) => m,
        _ => panic!(),
    };
    assert_eq!(
        msg(deflate_init(42, &DeflateOptions::default())),
        "deflate_init(): Argument #1 ($encoding) must be one of ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP, or ZLIB_ENCODING_DEFLATE"
    );
    for (opt, m) in [
        (
            DeflateOptions {
                level: Some(42),
                ..Default::default()
            },
            "deflate_init(): \"level\" option must be between -1 and 9",
        ),
        (
            DeflateOptions {
                level: Some(-2),
                ..Default::default()
            },
            "deflate_init(): \"level\" option must be between -1 and 9",
        ),
        (
            DeflateOptions {
                memory: Some(0),
                ..Default::default()
            },
            "deflate_init(): \"memory\" option must be between 1 and 9",
        ),
        (
            DeflateOptions {
                memory: Some(10),
                ..Default::default()
            },
            "deflate_init(): \"memory\" option must be between 1 and 9",
        ),
    ] {
        assert_eq!(msg(deflate_init(ENCODING_DEFLATE, &opt)), m);
    }
    // ext/zlib/tests/inflate_init_error.phpt
    assert!(matches!(
        inflate_init(42, &InflateOptions::default()),
        Err(ZlibError::ValueError(m)) if m == "Encoding mode must be ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP or ZLIB_ENCODING_DEFLATE"
    ));
    // ext/zlib/tests/deflate_add_error.phpt, inflate_add_error.phpt
    let mut d = deflate_init(ENCODING_DEFLATE, &DeflateOptions::default()).unwrap();
    assert_eq!(
        d.add(b"test", 6789),
        Err(ZlibError::ValueError("deflate_add(): Argument #3 ($flush_mode) must be one of ZLIB_NO_FLUSH, ZLIB_PARTIAL_FLUSH, ZLIB_SYNC_FLUSH, ZLIB_FULL_FLUSH, ZLIB_BLOCK, or ZLIB_FINISH".into()))
    );
    // ext/zlib/tests/gh16326.phpt
    let dict = |v: Vec<&[u8]>| DeflateOptions {
        dictionary: Some(DictOption::List(
            v.into_iter().map(|s| s.to_vec()).collect(),
        )),
        ..Default::default()
    };
    assert_eq!(
        msg(deflate_init(ENCODING_DEFLATE, &dict(vec![b" ", b""]))),
        "deflate_init(): Argument #2 ($options) must not contain empty strings"
    );
    assert_eq!(
        msg(deflate_init(
            ENCODING_DEFLATE,
            &dict(vec![b"hello", b"wor\0ld"])
        )),
        "deflate_init(): Argument #2 ($options) must not contain strings with null bytes"
    );
}

#[test]
fn dictionary_usage() {
    // ext/zlib/tests/dictionary_usage.phpt
    let az: Vec<Vec<u8>> = (b'a'..=b'z').map(|c| vec![c]).collect();
    let mut joined = Vec::new();
    for c in &az {
        joined.extend(c);
        joined.push(0);
    }
    let a = deflate_init(
        ENCODING_DEFLATE,
        &DeflateOptions {
            dictionary: Some(DictOption::List(az.clone())),
            ..Default::default()
        },
    )
    .unwrap()
    .add(b"abdcde", 4)
    .unwrap();
    let b = deflate_init(
        ENCODING_DEFLATE,
        &DeflateOptions {
            dictionary: Some(DictOption::Str(joined)),
            ..Default::default()
        },
    )
    .unwrap()
    .add(b"abdcde", 4)
    .unwrap();
    assert_eq!(a, b);
    let mut i = inflate_init(
        ENCODING_DEFLATE,
        &InflateOptions {
            dictionary: Some(DictOption::List(az.clone())),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(i.add(&a, 4).unwrap(), b"abdcde");
    let mut wrong = vec![b"8".to_vec()];
    wrong.extend(az[1..].iter().cloned());
    let mut i = inflate_init(
        ENCODING_DEFLATE,
        &InflateOptions {
            dictionary: Some(DictOption::List(wrong)),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        i.add(&a, 4),
        warn("Dictionary does not match expected dictionary (incorrect adler32 hash)")
    );
}

#[test]
fn inflate_status_and_read_len() {
    // ext/zlib/tests/inflate_get_status.phpt
    let mut c = zlib_encode(b"Hello world.", ENCODING_DEFLATE, -1).unwrap();
    assert_eq!(c.len(), 20);
    c.extend(b"qebsouesl".repeat(128));
    let mut ctx = inflate_init(ENCODING_DEFLATE, &InflateOptions::default()).unwrap();
    let mut buf = Vec::new();
    let mut i = 0;
    while ctx.status() == 0 {
        buf.extend(ctx.add(&c[i..i + 1], 2).unwrap());
        assert_eq!(ctx.status(), if i == 19 { 1 } else { 0 });
        i += 1;
    }
    assert_eq!(i, 20);
    assert_eq!(buf, b"Hello world.");
    ctx.add(&c[..12], 2).unwrap();
    assert_eq!(ctx.status(), 0);
    // ext/zlib/tests/inflate_get_read_len.phpt
    let mut ctx = inflate_init(ENCODING_DEFLATE, &InflateOptions::default()).unwrap();
    ctx.add(&c, 2).unwrap();
    assert_eq!(ctx.status(), 1);
    assert_eq!(ctx.read_len(), 20);
}

#[test]
fn deflate_add_buffer_full() {
    // ext/zlib/tests/deflate_add_buffer_full.phpt: every sync flush ends
    // with the empty stored block's 00 00 ff ff.
    let mut ctx = deflate_init(ENCODING_RAW, &DeflateOptions::default()).unwrap();
    for h in [
        "255044462d312e320a25c7ec8fa20a362030206f626a0a3c3c2f4c656e6774682037203020522f46696c746572202f466c6174654465636f64653e3e0a737472",
        "65616d0a789c7d53c16ed43010bde7c037f85824766a7bc6767c2ca8a00a016a1b2edcb2dbecaed1266937d98afe3d6327363794439437e3f17b6f5e242821e3",
        "b3be777df5525d3f90384cd58b50a9945fbb5e7c6cb8c89fca8156c688665f2de794504a81f75658a7c1d54a347d7575fb6e17ba617edffcae9c84da3aee6c9e",
    ] {
        let o = ctx.add(&unhex(h), 2).unwrap();
        assert_eq!(&o[o.len() - 4..], b"\x00\x00\xff\xff");
    }
}

#[test]
fn context_reuse_and_streaming() {
    // ext/zlib/tests/deflate_init_reuse.phpt, deflate_add_basic.phpt,
    // inflate_init_reuse.phpt, inflate_add_basic.phpt
    let az: Vec<u8> = (b'a'..=b'z').collect();
    let mut d = deflate_init(ENCODING_DEFLATE, &DeflateOptions::default()).unwrap();
    for c in &az {
        d.add(&[*c], 0).unwrap();
    }
    d.add(b"", 4).unwrap();
    let mut out = Vec::new();
    for c in &az {
        out.extend(d.add(&[*c], 0).unwrap());
    }
    out.extend(d.add(b"", 4).unwrap());
    assert_eq!(zlib_decode(&out, 0).unwrap(), az);
    for e in [ENCODING_RAW, ENCODING_GZIP, ENCODING_DEFLATE] {
        for f in [2, 1, 3, 0, 5] {
            for size in [1usize, 4, 32768] {
                let mut d = deflate_init(e, &DeflateOptions::default()).unwrap();
                let mut buf = Vec::new();
                let mut out = Vec::new();
                for c in &az {
                    buf.push(*c);
                    if buf.len() >= size {
                        out.extend(d.add(&buf, f).unwrap());
                        buf.clear();
                    }
                }
                out.extend(d.add(&buf, 4).unwrap());
                assert_eq!(zlib_decode(&out, 0).unwrap(), az, "{e} {f} {size}");
            }
        }
    }
    let gz = gzencode(&az, -1, ENCODING_GZIP).unwrap();
    let mut i = inflate_init(ENCODING_GZIP, &InflateOptions::default()).unwrap();
    for _ in 0..2 {
        let mut o = Vec::new();
        for b in &gz {
            o.extend(i.add(&[*b], 2).unwrap());
        }
        o.extend(i.add(b"", 4).unwrap());
        assert_eq!(o, az);
    }
}

#[test]
fn filters() {
    // ext/zlib/tests/bug48725.phpt: rewinding the stream flushes the filter.
    let mut diags = Vec::new();
    let mut f = deflate_filter(&FilterParams::None, &mut diags).unwrap();
    let mut o = f.filter(&b"0123456789abcdef".repeat(1000), FilterFlush::None);
    o.extend(f.filter(b"", FilterFlush::Inc));
    assert_eq!(
        hex(&o),
        "ecc7c901c0100000b09594bac641d97f840e22f9253c31bdb9d4d6c75cdf3ec1ddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddaffc0f0000ffff"
    );
    let text = b"I am the very model of a modern major general, I've information vegetable, animal, and mineral.";
    let mut f = deflate_filter(&FilterParams::None, &mut diags).unwrap();
    let mut c = f.filter(text, FilterFlush::None);
    c.extend(f.filter(b"", FilterFlush::Close));
    // zlib_filter_deflate.phpt: the base64 of the output starts "HctB".
    assert_eq!(&c[..3], &[0x1d, 0xcb, 0x41]);
    let mut i = inflate_filter(&FilterParams::None, &mut diags).unwrap();
    assert_eq!(i.filter(&c, FilterFlush::None).unwrap(), text);
    // ext/zlib/tests/zlib_filter_inflate2.phpt: a gzip file needs window 31
    // or 47; the default (raw) fails with a notice.
    let gz = gzencode(b"This is quite the thing ain't it\n", -1, ENCODING_GZIP).unwrap();
    let mut i = inflate_filter(&FilterParams::None, &mut diags).unwrap();
    assert_eq!(
        i.filter(&gz, FilterFlush::None),
        Err(Diag::Notice("zlib: data error".into()))
    );
    for w in [31, 47] {
        let p = FilterParams::Map {
            level: None,
            window: Some(w),
            memory: None,
        };
        let mut i = inflate_filter(&p, &mut diags).unwrap();
        assert_eq!(
            i.filter(&gz, FilterFlush::None).unwrap(),
            b"This is quite the thing ain't it\n"
        );
    }
    // ext/zlib/tests/bug_40189.phpt: an empty final block, then junk.
    let mut i = inflate_filter(&FilterParams::None, &mut diags).unwrap();
    let a = b"\x03\x00\x85\x46\x2f\x7c\xc2\xaa\x69\x2b\x6d\xe5\xdb\xfe\xe4\x21\x8f\x00\x97\x21\x1d\x02\x00\x00\x00\x47\x42\x4d\x42";
    assert_eq!(i.filter(a, FilterFlush::None).unwrap(), b"");
    assert!(diags.is_empty());
}

/// Reads a section of a php-src test when the checkout is available.
fn phpt_section(test: &str, section: &str) -> Option<Vec<u8>> {
    let path = format!("/tmp/claude-1000/php-tests/ext/zlib/tests/{test}");
    let data = std::fs::read(path).ok()?;
    let marker = format!("--{section}--\n");
    let start = data
        .windows(marker.len())
        .position(|w| w == marker.as_bytes())?
        + marker.len();
    let rest = &data[start..];
    let end = rest
        .windows(3)
        .position(|w| w == b"\n--")
        .unwrap_or(rest.len());
    Some(rest[..end].to_vec())
}

#[test]
fn zlib_filter_deflate2() {
    // ext/zlib/tests/zlib_filter_deflate2.phpt: level 9 raw output, byte for
    // byte (the expectation is binary, so it is read from the test).
    let Some(want) = phpt_section("zlib_filter_deflate2.phpt", "EXPECT") else {
        return;
    };
    let text = b"I am the very model of a modern major general, I've information vegetable, animal, and mineral.";
    let p = FilterParams::Map {
        level: Some(9),
        window: None,
        memory: None,
    };
    let mut f = deflate_filter(&p, &mut Vec::new()).unwrap();
    let mut c = f.filter(text, FilterFlush::None);
    c.extend(f.filter(b"", FilterFlush::Close));
    // run-tests trims trailing whitespace of both sides.
    let trim = |v: &[u8]| {
        let mut v = v.to_vec();
        while v.last().is_some_and(|b| b.is_ascii_whitespace()) {
            v.pop();
        }
        v
    };
    assert_eq!(trim(&c), trim(&want));
}
