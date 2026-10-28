//! Decoding, errors, contexts and buffer handling compared with PHP 8.5
//! (tests/fixtures/behaviour_php85.txt and behaviour2_php85.txt, from the
//! drivers next to them). Each case re-enacts the driver's PHP code.

mod common;

use common::*;
use goro_ext_zlib::*;
use std::collections::BTreeMap;

type R = Result<Vec<u8>, ZlibError>;

fn val_str(b: &[u8]) -> String {
    format!("=> s:{}", out_repr(b))
}

/// The driver's run() output for a call returning string|false.
fn show(func: &str, r: R) -> String {
    match r {
        Ok(v) => val_str(&v),
        Err(ZlibError::Warning(m)) => format!("W:{func}(): {m} | => false"),
        Err(ZlibError::ValueError(m)) => format!("E:ValueError:{m}"),
        Err(ZlibError::TypeError(m)) => format!("E:TypeError:{m}"),
    }
}

fn crc32(b: &[u8]) -> u32 {
    let mut h = goro_ext_hash::Hasher::plain(goro_ext_hash::lookup(b"crc32b").unwrap());
    h.update(b);
    u32::from_be_bytes(h.finish()[..4].try_into().unwrap())
}

fn adler32(b: &[u8]) -> u32 {
    let mut h = goro_ext_hash::Hasher::plain(goro_ext_hash::lookup(b"adler32").unwrap());
    h.update(b);
    u32::from_be_bytes(h.finish()[..4].try_into().unwrap())
}

fn decoder(name: &str) -> fn(&[u8], i64) -> R {
    match name {
        "gzuncompress" => gzuncompress,
        "gzinflate" => gzinflate,
        "gzdecode" => gzdecode,
        "zlib_decode" => zlib_decode,
        _ => panic!("{name}"),
    }
}

const DECODERS: [&str; 4] = ["gzuncompress", "gzinflate", "gzdecode", "zlib_decode"];

fn enc(name: &str, d: &[u8]) -> Vec<u8> {
    match name {
        "raw" => gzdeflate(d, -1, ENCODING_RAW).unwrap(),
        "zlib" => gzcompress(d, -1, ENCODING_DEFLATE).unwrap(),
        "gzip" => gzencode(d, -1, ENCODING_GZIP).unwrap(),
        _ => panic!(),
    }
}

fn hexs(b: &[u8]) -> String {
    hex(b)
}

struct Checker {
    want: BTreeMap<String, String>,
    seen: usize,
    fails: Vec<String>,
}

impl Checker {
    fn new(files: &[&str]) -> Self {
        let mut want = BTreeMap::new();
        for f in files {
            for (k, v) in fixture(f) {
                want.insert(k, v);
            }
        }
        Checker {
            want,
            seen: 0,
            fails: Vec::new(),
        }
    }
    fn check(&mut self, key: &str, got: String) {
        let Some(w) = self.want.get(key) else {
            self.fails.push(format!("{key}: not in fixture"));
            return;
        };
        self.seen += 1;
        if *w != got {
            self.fails
                .push(format!("{key}\n   want {w}\n    got {got}"));
        }
    }
    fn finish(mut self, min_seen: usize) {
        if !self.fails.is_empty() {
            let n = self.fails.len();
            self.fails.truncate(40);
            panic!(
                "{n} mismatches of {}:\n{}",
                self.seen,
                self.fails.join("\n")
            );
        }
        assert!(self.seen >= min_seen, "only {} cases checked", self.seen);
    }
}

#[test]
fn one_shot_functions() {
    let mut c = Checker::new(&[
        "behaviour_php85.txt",
        "behaviour2_php85.txt",
        "behaviour3_php85.txt",
    ]);
    let inputs = inputs();
    type Enc = fn(&[u8], i64, i64) -> R;
    for (name, f) in [
        ("gzcompress", gzcompress as Enc),
        ("gzdeflate", gzdeflate),
        ("gzencode", gzencode),
    ] {
        let def = match name {
            "gzcompress" => ENCODING_DEFLATE,
            "gzdeflate" => ENCODING_RAW,
            _ => ENCODING_GZIP,
        };
        for l in [-2, -1, 10, 99] {
            c.check(&format!("{name}/level/{l}"), show(name, f(b"abc", l, def)));
        }
        for e in [-15, 15, 31, 0, 1, 99, -1, 16] {
            c.check(&format!("{name}/enc/{e}"), show(name, f(b"abc", 6, e)));
        }
    }
    for e in [-15, 15, 31, 0, 99] {
        c.check(
            &format!("zlib_encode/enc/{e}"),
            show("zlib_encode", zlib_encode(b"abc", e, -1)),
        );
    }
    for l in [-2, 10] {
        c.check(
            &format!("zlib_encode/level/{l}"),
            show("zlib_encode", zlib_encode(b"abc", 15, l)),
        );
    }
    for f in DECODERS {
        let d = decoder(f);
        c.check(&format!("{f}/maxlen/-1"), show(f, d(b"abc", -1)));
        c.check(&format!("{f}/empty"), show(f, d(b"", 0)));
        c.check(
            &format!("{f}/garbage"),
            show(f, d(b"this is not compressed", 0)),
        );
        c.check(&format!("{f}/one"), show(f, d(b"\x00", 0)));
    }

    for name in [
        "empty",
        "hello",
        "text5000",
        "mixed400k",
        "zeros100k",
        "a15",
    ] {
        let d = &inputs[name];
        for e in ["raw", "zlib", "gzip"] {
            let comp = enc(e, d);
            for f in DECODERS {
                let dec = decoder(f);
                c.check(&format!("decode/{name}/{e}/{f}"), show(f, dec(&comp, 0)));
                let n = d.len() as i64;
                for m in [1, n - 1, n, n + 1] {
                    if m < 1 {
                        continue;
                    }
                    c.check(
                        &format!("decode/{name}/{e}/{f}/max{m}"),
                        show(f, dec(&comp, m)),
                    );
                }
            }
            let cl = comp.len();
            for cut in [1, 2, 5, 10, cl / 2, cl.wrapping_sub(4), cl - 1] {
                if cut == 0 || cut >= cl {
                    continue;
                }
                for f in DECODERS {
                    c.check(
                        &format!("trunc/{name}/{e}/{cut}/{f}"),
                        show(f, decoder(f)(&comp[..cut], 0)),
                    );
                }
            }
            for (i, tail) in [&b"\x00"[..], b"junk", &comp].iter().enumerate() {
                let mut t = comp.clone();
                t.extend_from_slice(tail);
                for f in DECODERS {
                    c.check(
                        &format!("tail/{name}/{e}/{i}/{f}"),
                        show(f, decoder(f)(&t, 0)),
                    );
                }
            }
        }
    }

    for e in ["raw", "zlib", "gzip"] {
        let comp = enc(
            e,
            b"The quick brown fox jumps over the lazy dog. The quick brown fox.",
        );
        for i in 0..comp.len() {
            for x in [0x01u8, 0x80, 0xff] {
                let mut b = comp.clone();
                b[i] ^= x;
                for f in DECODERS {
                    if (e == "raw") != (f == "gzinflate")
                        && f != "zlib_decode"
                        && !(e == "gzip" && f == "gzdecode")
                        && !(e == "zlib" && f == "gzuncompress")
                    {
                        continue;
                    }
                    c.check(&format!("flip/{e}/{i}/{x}/{f}"), show(f, decoder(f)(&b, 0)));
                }
            }
        }
    }

    let hi = gzdeflate(b"hi", -1, ENCODING_RAW).unwrap();
    let gz = |h: &[u8], tail: &[u8]| -> Vec<u8> { [h, &hi, tail].concat() };
    let hi_trailer = [crc32(b"hi").to_le_bytes(), 2u32.to_le_bytes()].concat();
    let fhcrc_ok = {
        let h = unhex("1f8b0802000000000003");
        let c = (crc32(&h) & 0xffff) as u16;
        [&h[..], &c.to_le_bytes()].concat()
    };
    let streams: Vec<(&str, Vec<u8>)> = vec![
        ("stored-empty-final", b"\x01\x00\x00\xff\xff".to_vec()),
        (
            "stored-badlen",
            [&b"\x01\x05\x00\xfa\xfe"[..], b"hello"].concat(),
        ),
        (
            "stored-short",
            [&b"\x01\x05\x00\xfa\xff"[..], b"hel"].concat(),
        ),
        ("fixed-empty", b"\x03\x00".to_vec()),
        ("type3", b"\x07\x00".to_vec()),
        ("nonfinal-only", b"\x02\x00".to_vec()),
        ("dist-too-far", unhex("4b0402")),
        (
            "gz-fname",
            gz(
                &[&unhex("1f8b0808000000000003")[..], b"name\0"].concat(),
                &hi_trailer,
            ),
        ),
        (
            "gz-fcomment",
            gz(
                &[&unhex("1f8b0810000000000003")[..], b"comment\0"].concat(),
                &hi_trailer,
            ),
        ),
        (
            "gz-fextra",
            gz(
                &[&unhex("1f8b0804000000000003")[..], b"\x03\x00abc"].concat(),
                &hi_trailer,
            ),
        ),
        ("gz-fhcrc-ok", gz(&fhcrc_ok, &hi_trailer)),
        (
            "gz-fhcrc-bad",
            gz(&unhex("1f8b08020000000000030000"), &hi_trailer),
        ),
        (
            "gz-reserved-flag",
            gz(&unhex("1f8b0820000000000003"), &hi_trailer),
        ),
        (
            "gz-badmethod",
            gz(&unhex("1f8b0700000000000003"), &hi_trailer),
        ),
        (
            "gz-badcrc",
            gz(
                &unhex("1f8b0800000000000003"),
                &[0u32.to_le_bytes(), 2u32.to_le_bytes()].concat(),
            ),
        ),
        (
            "gz-badlen",
            gz(
                &unhex("1f8b0800000000000003"),
                &[crc32(b"hi").to_le_bytes(), 3u32.to_le_bytes()].concat(),
            ),
        ),
        (
            "gz-two-members",
            [enc("gzip", b"hello "), enc("gzip", b"world")].concat(),
        ),
        ("zlib-badcheck", gz(b"\x78\x9d", &0u32.to_be_bytes())),
        ("zlib-badmethod", gz(b"\x79\x9c", b"")),
        (
            "zlib-window16",
            gz(b"\x88\x98", &adler32(b"hi").to_be_bytes()),
        ),
        ("zlib-badadler", gz(b"\x78\x9c", &1u32.to_be_bytes())),
        (
            "zlib-fdict",
            gz(b"\x78\xbb\x00\x00\x00\x01", &1u32.to_be_bytes()),
        ),
        (
            "zlib-window9",
            gz(b"\x18\x95", &adler32(b"hi").to_be_bytes()),
        ),
    ];
    for (sname, s) in &streams {
        for f in DECODERS {
            c.check(&format!("stream/{sname}/{f}"), show(f, decoder(f)(s, 0)));
        }
    }
    for name in ["hello", "text5000", "a", "allbytes"] {
        let d = &inputs[name];
        c.check(
            &format!("zlib_decode/raw/{name}"),
            show("zlib_decode", zlib_decode(&enc("raw", d), 0)),
        );
        for l in [0, 1, 9] {
            c.check(
                &format!("zlib_decode/raw/{name}/{l}"),
                show(
                    "zlib_decode",
                    zlib_decode(&gzdeflate(d, l, ENCODING_RAW).unwrap(), 0),
                ),
            );
        }
    }

    // behaviour2: detection and max_length.
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "raw-looks-zlib",
            [&b"\x08\x1d\x00\xe2\xff"[..], &[b'A'; 29], b"\x03\x00"].concat(),
        ),
        ("raw-78", b"\x78\x9czz".to_vec()),
        ("zlib-garbage", b"\x78\x9c\xff\xff".to_vec()),
        ("gzip-garbage", b"\x1f\x8b\x08\x00garbage".to_vec()),
        ("gzip-magic-only", b"\x1f\x8b".to_vec()),
        ("one-1f", b"\x1f".to_vec()),
        ("raw-1f8b-start", b"\x1f\x8b\x00\x00".to_vec()),
        (
            "zlib-then-raw",
            [enc("zlib", b"x"), b"junk".to_vec()].concat(),
        ),
    ];
    for (cname, d) in &cases {
        for f in ["zlib_decode", "gzinflate", "gzuncompress", "gzdecode"] {
            c.check(&format!("detect/{cname}/{f}"), show(f, decoder(f)(d, 0)));
        }
    }
    let a100 = enc("raw", &[b'a'; 100]);
    c.check(
        "maxlen/exact-then-more",
        show("gzinflate", gzinflate(&a100[..a100.len() - 1], 100)),
    );
    c.check(
        "maxlen/data-error-after",
        show(
            "gzinflate",
            gzinflate(&[enc("raw", b"hello"), vec![0xff]].concat(), 5),
        ),
    );
    let m = enc("zlib", &inputs["mixed400k"]);
    c.check(
        "maxlen/large",
        format!("=> {}", gzuncompress(&m, 400000).unwrap().len()),
    );
    c.check(
        "maxlen/large-1",
        show("gzuncompress", gzuncompress(&m, 399999)),
    );
    c.check(
        "maxlen/zlib_decode",
        show("zlib_decode", zlib_decode(&enc("gzip", b"abcdef"), 3)),
    );
    c.finish(2600);
}

fn opts_case(name: &str) -> (DeflateOptions, InflateOptions) {
    let mut d = DeflateOptions::default();
    let mut i = InflateOptions::default();
    let dict = |v: DictOption, d: &mut DeflateOptions, i: &mut InflateOptions| {
        d.dictionary = Some(v.clone());
        i.dictionary = Some(v);
    };
    match name {
        "none" | "unknown" | "numeric-key" => {}
        "level-1" => d.level = Some(-1),
        "level9" => d.level = Some(9),
        "level10" => d.level = Some(10),
        "level-str" | "level-float" => d.level = Some(5),
        "level-null" | "level-arr" | "level-strbad" => d.level = Some(0),
        "level-bool" => d.level = Some(1),
        "memory0" => d.memory = Some(0),
        "memory1" => d.memory = Some(1),
        "memory9" => d.memory = Some(9),
        "memory10" => d.memory = Some(10),
        "strategy-1" => d.strategy = Some(-1),
        "strategy0" => d.strategy = Some(0),
        "strategy4" => d.strategy = Some(4),
        "strategy5" => d.strategy = Some(5),
        "dict-empty" => dict(DictOption::Str(vec![]), &mut d, &mut i),
        "dict-str" => dict(DictOption::Str(b"abc".to_vec()), &mut d, &mut i),
        "dict-arr" => dict(
            DictOption::List(vec![b"a".to_vec(), b"b".to_vec()]),
            &mut d,
            &mut i,
        ),
        "dict-arr-empty" => dict(DictOption::List(vec![]), &mut d, &mut i),
        "dict-arr-emptystr" => dict(
            DictOption::List(vec![b"a".to_vec(), vec![]]),
            &mut d,
            &mut i,
        ),
        "dict-arr-nul" => dict(DictOption::List(vec![b"a\0b".to_vec()]), &mut d, &mut i),
        "dict-int" => dict(DictOption::Other("int".into()), &mut d, &mut i),
        "dict-null" => dict(DictOption::Other("null".into()), &mut d, &mut i),
        "dict-str-nul" => dict(DictOption::Str(b"a\0b".to_vec()), &mut d, &mut i),
        "dict-arr-int" => dict(
            DictOption::List(vec![b"1".to_vec(), b"2".to_vec()]),
            &mut d,
            &mut i,
        ),
        w if w.starts_with("window") => {
            let v: i64 = w[6..].parse().unwrap();
            d.window = Some(v);
            i.window = Some(v);
        }
        _ => panic!("{name}"),
    }
    (d, i)
}

fn show_init<T>(func: &str, r: Result<T, ZlibError>, f: impl FnOnce(T) -> R) -> String {
    match r {
        Ok(c) => show(func, f(c)),
        Err(ZlibError::Warning(m)) => format!("W:{func}(): {m} | => false"),
        Err(ZlibError::ValueError(m)) => format!("E:ValueError:{m}"),
        Err(ZlibError::TypeError(m)) => format!("E:TypeError:{m}"),
    }
}

/// Runs a closure returning a driver-built string, with warnings raised by
/// the inner calls collected in order.
fn closure_str(diags: &[String], s: &str) -> String {
    let mut parts: Vec<String> = diags.to_vec();
    parts.push(val_str(s.as_bytes()));
    parts.join(" | ")
}

#[test]
fn contexts() {
    let mut c = Checker::new(&[
        "behaviour_php85.txt",
        "behaviour2_php85.txt",
        "behaviour3_php85.txt",
    ]);
    let inputs = inputs();
    let names = [
        "none",
        "level-1",
        "level9",
        "level10",
        "level-str",
        "level-float",
        "level-null",
        "level-arr",
        "level-bool",
        "level-strbad",
        "memory0",
        "memory1",
        "memory9",
        "memory10",
        "window7",
        "window8",
        "window9",
        "window15",
        "window16",
        "window-15",
        "window31",
        "strategy-1",
        "strategy0",
        "strategy4",
        "strategy5",
        "dict-empty",
        "dict-str",
        "dict-arr",
        "dict-arr-empty",
        "dict-arr-emptystr",
        "dict-arr-nul",
        "dict-int",
        "dict-null",
        "dict-str-nul",
        "dict-arr-int",
        "unknown",
        "numeric-key",
    ];
    for name in names {
        let (dopt, iopt) = opts_case(name);
        for e in [ENCODING_RAW, ENCODING_DEFLATE, ENCODING_GZIP] {
            let got = show_init("deflate_init", deflate_init(e, &dopt), |mut ctx| {
                ctx.add(b"hello hello", 4).map(|v| hexs(&v).into_bytes())
            });
            c.check(&format!("deflate_init/{name}/{e}"), got);
            let input = match e {
                ENCODING_RAW => enc("raw", b"hello"),
                ENCODING_GZIP => enc("gzip", b"hello"),
                _ => enc("zlib", b"hello"),
            };
            let got = match inflate_init(e, &iopt) {
                Ok(mut ctx) => show("inflate_add", ctx.add(&input, 4)),
                Err(ZlibError::ValueError(m)) => format!("E:ValueError:{m}"),
                Err(ZlibError::TypeError(m)) => format!("E:TypeError:{m}"),
                Err(ZlibError::Warning(m)) => format!("W:inflate_init(): {m} | => false"),
            };
            c.check(&format!("inflate_init/{name}/{e}"), got);
        }
    }
    for e in [0, 1, -1, 16, 47, -16, 999] {
        let d = match deflate_init(e, &DeflateOptions::default()) {
            Ok(_) => "=> o:DeflateContext".to_string(),
            Err(ZlibError::ValueError(m)) => format!("E:ValueError:{m}"),
            Err(x) => format!("{x:?}"),
        };
        c.check(&format!("deflate_init/enc/{e}"), d);
        let d = match inflate_init(e, &InflateOptions::default()) {
            Ok(_) => "=> o:InflateContext".to_string(),
            Err(ZlibError::ValueError(m)) => format!("E:ValueError:{m}"),
            Err(x) => format!("{x:?}"),
        };
        c.check(&format!("inflate_init/enc/{e}"), d);
    }

    // inflate_add incremental runs.
    for name in ["hello", "text5000", "mixed400k"] {
        let d = &inputs[name];
        for (e, ename) in [
            (ENCODING_RAW, "raw"),
            (ENCODING_DEFLATE, "zlib"),
            (ENCODING_GZIP, "gzip"),
        ] {
            let comp = [enc(ename, d), b"TRAILING".to_vec()].concat();
            for chunk in [1usize, 3, 64, 1000, 100000] {
                if chunk < 64 && comp.len() > 20000 {
                    continue;
                }
                for f in [0i64, 2, 4, 1, 3, 5] {
                    let mut ctx = inflate_init(e, &InflateOptions::default()).unwrap();
                    let mut out = Vec::new();
                    let mut st = Vec::new();
                    let mut lens = Vec::new();
                    let mut diags = Vec::new();
                    for ch in comp.chunks(chunk) {
                        match ctx.add(ch, f) {
                            Ok(r) => {
                                lens.push(r.len().to_string());
                                out.extend(r);
                                st.push(format!("{}/{}", ctx.status(), ctx.read_len()));
                            }
                            Err(ZlibError::Warning(m)) => {
                                diags.push(format!("W:inflate_add(): {m}"));
                                st.push("F".into());
                                break;
                            }
                            Err(x) => panic!("{x:?}"),
                        }
                    }
                    let s = format!(
                        "{} {} {} {}",
                        out_repr(&out),
                        md5_hex(st.join(",").as_bytes()),
                        md5_hex(lens.join(",").as_bytes()),
                        st.last().unwrap()
                    );
                    c.check(
                        &format!("inflate_add/{name}/{e}/{chunk}/{f}"),
                        closure_str(&diags, &s),
                    );
                }
            }
        }
    }

    // Call-by-call sequences.
    let hw = enc("zlib", b"Hello world.");
    let sync_parts = {
        let mut c1 = deflate_init(ENCODING_DEFLATE, &DeflateOptions::default()).unwrap();
        let a = c1.add(b"part one ", 2).unwrap();
        let b = c1.add(b"part two", 4).unwrap();
        vec![a, b]
    };
    let bytes = |s: &[u8]| s.iter().map(|&b| vec![b]).collect::<Vec<_>>();
    let seqs: Vec<(&str, Vec<Vec<u8>>, i64)> = vec![
        (
            "bytewise",
            bytes(&[hw.clone(), b"junk".to_vec()].concat()),
            0,
        ),
        ("bytewise-sync", bytes(&hw), 2),
        ("bytewise-finish", bytes(&hw), 4),
        ("half-finish", vec![hw[..8].to_vec(), hw[8..].to_vec()], 4),
        ("empty-first", vec![vec![], hw.clone(), vec![]], 0),
        ("empty-finish", vec![vec![], vec![], vec![]], 4),
        ("garbage", vec![b"garbage!".to_vec(), b"more".to_vec()], 0),
        ("garbage-finish", vec![b"garbage!".to_vec()], 4),
        (
            "restart",
            vec![
                enc("zlib", b"abc"),
                enc("zlib", b"def"),
                [enc("zlib", b"ghi"), enc("zlib", b"jkl")].concat(),
            ],
            0,
        ),
        (
            "restart-finish",
            vec![enc("zlib", b"abc"), enc("zlib", b"def")],
            4,
        ),
        (
            "after-error",
            vec![b"garbage!".to_vec(), enc("zlib", b"abc")],
            0,
        ),
        ("sync-stream", sync_parts, 2),
    ];
    for (sname, parts, f) in seqs {
        let mut ctx = inflate_init(ENCODING_DEFLATE, &InflateOptions::default()).unwrap();
        let mut res = Vec::new();
        let mut diags = Vec::new();
        for p in parts {
            let r = ctx.add(&p, f);
            let s = match r {
                Ok(v) => hexs(&v),
                Err(ZlibError::Warning(m)) => {
                    diags.push(format!("W:inflate_add(): {m}"));
                    "false".into()
                }
                Err(x) => panic!("{x:?}"),
            };
            res.push(format!("{s}/{}/{}", ctx.status(), ctx.read_len()));
        }
        c.check(
            &format!("inflate_seq/{sname}"),
            closure_str(&diags, &res.join(",")),
        );
    }

    // Dictionaries.
    let dict = b"hello world dictionary".to_vec();
    let dopt = DeflateOptions {
        dictionary: Some(DictOption::Str(dict.clone())),
        ..Default::default()
    };
    let iopt = |d: &[u8]| InflateOptions {
        dictionary: Some(DictOption::Str(d.to_vec())),
        ..Default::default()
    };
    let withdict = deflate_init(ENCODING_DEFLATE, &dopt)
        .unwrap()
        .add(b"hello world", 4)
        .unwrap();
    let rawdict = deflate_init(ENCODING_RAW, &dopt)
        .unwrap()
        .add(b"hello world", 4)
        .unwrap();
    let ia = |e, o: &InflateOptions, d: &[u8]| {
        show("inflate_add", inflate_init(e, o).unwrap().add(d, 4))
    };
    c.check(
        "dict/zlib/ok",
        ia(ENCODING_DEFLATE, &iopt(&dict), &withdict),
    );
    c.check(
        "dict/zlib/none",
        ia(ENCODING_DEFLATE, &InflateOptions::default(), &withdict),
    );
    c.check(
        "dict/zlib/wrong",
        ia(ENCODING_DEFLATE, &iopt(b"nope"), &withdict),
    );
    c.check(
        "dict/zlib/unneeded",
        ia(ENCODING_DEFLATE, &iopt(b"nope"), &enc("zlib", b"hello")),
    );
    c.check("dict/raw/ok", ia(ENCODING_RAW, &iopt(&dict), &rawdict));
    c.check(
        "dict/raw/none",
        ia(ENCODING_RAW, &InflateOptions::default(), &rawdict),
    );
    c.check(
        "dict/gzip/ok",
        ia(ENCODING_GZIP, &iopt(&dict), &enc("gzip", b"hello")),
    );
    c.check(
        "dict/gzuncompress",
        show("gzuncompress", gzuncompress(&withdict, 0)),
    );
    c.check(
        "dict/zlib_decode",
        show("zlib_decode", zlib_decode(&withdict, 0)),
    );
    {
        let mut ctx = inflate_init(ENCODING_DEFLATE, &iopt(&dict)).unwrap();
        let mut o = Vec::new();
        let mut failed = false;
        for b in &withdict {
            match ctx.add(&[*b], 2) {
                Ok(r) => o.extend(r),
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        let s = if failed {
            "false".to_string()
        } else {
            format!("{}/{}", String::from_utf8_lossy(&o), ctx.status())
        };
        c.check("dict/zlib/bytewise", closure_str(&[], &s));
    }
    {
        let mut ctx = inflate_init(ENCODING_DEFLATE, &InflateOptions::default()).unwrap();
        let mut o = Vec::new();
        let mut diags = Vec::new();
        for b in &withdict {
            let r = ctx.add(&[*b], 2);
            let v = match r {
                Ok(v) => format!("'{}'", String::from_utf8_lossy(&v)),
                Err(ZlibError::Warning(m)) => {
                    diags.push(format!("W:inflate_add(): {m}"));
                    "false".into()
                }
                Err(x) => panic!("{x:?}"),
            };
            o.push(format!("{v}{}", ctx.status()));
        }
        c.check("dict/zlib/none/bytewise", closure_str(&diags, &o.join(",")));
    }

    // deflate_add misc.
    let bad = |r: R| show("deflate_add", r);
    c.check(
        "deflate_add/badflush",
        bad(deflate_init(ENCODING_RAW, &DeflateOptions::default())
            .unwrap()
            .add(b"x", 99)),
    );
    c.check(
        "deflate_add/badflush-6",
        bad(deflate_init(ENCODING_RAW, &DeflateOptions::default())
            .unwrap()
            .add(b"x", 6)),
    );
    c.check(
        "inflate_add/badflush",
        show(
            "inflate_add",
            inflate_init(ENCODING_RAW, &InflateOptions::default())
                .unwrap()
                .add(b"x", 99),
        ),
    );
    {
        let mut ctx = deflate_init(ENCODING_RAW, &DeflateOptions::default()).unwrap();
        let parts: Vec<String> = [(&b"abc"[..], 2), (b"", 0), (b"", 2), (b"x", 2), (b"x", 2)]
            .iter()
            .map(|(d, f)| hexs(&ctx.add(d, *f).unwrap()))
            .collect();
        c.check(
            "deflate_add/sync-noflush-sync",
            closure_str(&[], &parts.join(",")),
        );
    }
    let level0 = |m: i64| DeflateOptions {
        level: Some(0),
        memory: Some(m),
        ..Default::default()
    };
    {
        let mut ctx = deflate_init(ENCODING_RAW, &level0(8)).unwrap();
        let mut o = ctx.add(&inputs["mixed400k"], 2).unwrap();
        o.extend(ctx.add(&inputs["rand70000"], 0).unwrap());
        o.extend(ctx.add(b"", 4).unwrap());
        c.check("deflate_add/big-level0", closure_str(&[], &out_repr(&o)));
    }
    c.check(
        "gzcompress/level0/big",
        closure_str(
            &[],
            &out_repr(&gzcompress(&inputs["mixed400k"], 0, ENCODING_DEFLATE).unwrap()),
        ),
    );
    for m in [1, 2, 3, 4, 9] {
        let mut ctx = deflate_init(ENCODING_RAW, &level0(m)).unwrap();
        let mut o = Vec::new();
        let mut lens = Vec::new();
        for p in inputs["text70k"].chunks(1000) {
            let r = ctx.add(p, 0).unwrap();
            lens.push(r.len().to_string());
            o.extend(r);
        }
        let r = ctx.add(b"", 4).unwrap();
        lens.push(r.len().to_string());
        o.extend(r);
        c.check(
            &format!("deflate_add/level0/mem{m}"),
            closure_str(&[], &format!("{} {}", out_repr(&o), lens.join(","))),
        );
    }
    c.check("inflate_get_status/fresh", "=> 0".into());

    // behaviour2: deflate_add's buffer sizes.
    let t = &inputs["text300k"];
    for n in [
        990usize, 1000, 1001, 1100, 1500, 2000, 3000, 5000, 9999, 10000, 20000, 31000,
    ] {
        for pre in [500usize, 30] {
            let mut ctx = deflate_init(ENCODING_RAW, &level0(2)).unwrap();
            let a = ctx.add(&t[..pre], 0).unwrap();
            let b = ctx.add(&t[1000..1000 + n], 0).unwrap();
            let d = ctx.add(b"", 4).unwrap();
            let s = format!(
                "{},{},{},{}",
                a.len(),
                b.len(),
                d.len(),
                md5_hex(&[a, b, d].concat())
            );
            c.check(&format!("guess/{n}/{pre}"), closure_str(&[], &s));
        }
    }
    for n in [1usize, 2, 3, 10, 50, 64, 100, 200] {
        for (key, level) in [("syncfill", 0), ("syncfill6", 6)] {
            let mut ctx = deflate_init(
                ENCODING_RAW,
                &DeflateOptions {
                    level: Some(level),
                    ..Default::default()
                },
            )
            .unwrap();
            let mut o = Vec::new();
            for k in 1..=70usize {
                let data: Vec<u8> = if level == 0 {
                    vec![b'x'; n + k]
                } else {
                    inputs["rand70000"][k * 100..k * 100 + n + k].to_vec()
                };
                o.push(hexs(&ctx.add(&data, 2).unwrap()));
            }
            let lens: Vec<String> = o.iter().map(|s| s.len().to_string()).collect();
            let s = format!("{} {}", md5_hex(o.join(",").as_bytes()), lens.join(","));
            c.check(&format!("{key}/{n}"), closure_str(&[], &s));
        }
    }
    {
        let mut r = Vec::new();
        let mx = &inputs["mixed400k"];
        for n in [
            0usize, 1, 100, 32767, 32768, 65535, 65536, 65540, 131070, 131071, 200000,
        ] {
            r.push(format!(
                "{}/{}",
                gzcompress(&mx[..n], 0, ENCODING_DEFLATE).unwrap().len(),
                md5_hex(&gzdeflate(&mx[..n], 0, ENCODING_RAW).unwrap())
            ));
        }
        c.check("gzcompress/level0/sizes", closure_str(&[], &r.join(",")));
    }

    // behaviour2: inflate_add traces.
    let streams: Vec<(&str, Vec<u8>)> = vec![
        (
            "raw",
            [enc("raw", b"hello world"), b"TRAILING".to_vec()].concat(),
        ),
        (
            "zlib",
            [enc("zlib", b"hello world"), b"TRAILING".to_vec()].concat(),
        ),
        (
            "gzip",
            [enc("gzip", b"hello world"), b"TRAILING".to_vec()].concat(),
        ),
        ("raw2", {
            let mut d = deflate_init(ENCODING_RAW, &DeflateOptions::default()).unwrap();
            [
                d.add(b"first block ", 3).unwrap(),
                d.add(b"second", 4).unwrap(),
            ]
            .concat()
        }),
        ("zlib2", {
            let mut d = deflate_init(ENCODING_DEFLATE, &DeflateOptions::default()).unwrap();
            [
                d.add(b"first block ", 2).unwrap(),
                d.add(b"second", 4).unwrap(),
            ]
            .concat()
        }),
    ];
    for (sname, s) in &streams {
        for (e, en) in [
            (ENCODING_RAW, "raw"),
            (ENCODING_DEFLATE, "zlib"),
            (ENCODING_GZIP, "gzip"),
        ] {
            if !sname.starts_with(en) {
                continue;
            }
            for chunk in [1usize, 2, 3, 5, 7, 100] {
                for f in [5i64, 0, 4, 2] {
                    let mut ctx = inflate_init(e, &InflateOptions::default()).unwrap();
                    let mut res = Vec::new();
                    let mut diags = Vec::new();
                    let mut step = |ctx: &mut InflateContext, d: &[u8]| {
                        let r = match ctx.add(d, f) {
                            Ok(v) => hexs(&v),
                            Err(ZlibError::Warning(m)) => {
                                diags.push(format!("W:inflate_add(): {m}"));
                                "F".into()
                            }
                            Err(x) => panic!("{x:?}"),
                        };
                        res.push(format!("{r}/{}/{}", ctx.status(), ctx.read_len()));
                    };
                    for ch in s.chunks(chunk) {
                        step(&mut ctx, ch);
                    }
                    for extra in [&b""[..], b"", b"x"] {
                        step(&mut ctx, extra);
                    }
                    c.check(
                        &format!("trace/{sname}/{chunk}/{f}"),
                        closure_str(&diags, &res.join(",")),
                    );
                }
            }
        }
    }
    {
        let s = enc("zlib", b"hello world, hello world, hello world");
        let mut ctx = inflate_init(ENCODING_DEFLATE, &InflateOptions::default()).unwrap();
        let mut res = Vec::new();
        let mut diags = Vec::new();
        for (o, l, f) in [(0usize, 3usize, 5i64), (3, 5, 0), (8, 4, 5), (12, 100, 2)] {
            let end = (o + l).min(s.len());
            let r = match ctx.add(&s[o.min(s.len())..end], f) {
                Ok(v) => hexs(&v),
                Err(ZlibError::Warning(m)) => {
                    diags.push(format!("W:inflate_add(): {m}"));
                    "F".into()
                }
                Err(x) => panic!("{x:?}"),
            };
            res.push(format!("{r}/{}/{}", ctx.status(), ctx.read_len()));
        }
        c.check("trace/mixed-modes", closure_str(&diags, &res.join(",")));
    }
    // behaviour3: which invalid option is reported first.
    let err = |r: Result<DeflateContext, ZlibError>| match r {
        Err(ZlibError::ValueError(m)) => format!("E:ValueError:{m}"),
        _ => "ok".into(),
    };
    let o = |level, memory, window, strategy, dictionary| DeflateOptions {
        level,
        memory,
        window,
        strategy,
        dictionary,
    };
    c.check(
        "deflate_init/all-bad",
        err(deflate_init(
            ENCODING_RAW,
            &o(Some(99), Some(99), Some(99), Some(99), None),
        )),
    );
    c.check(
        "deflate_init/mem-window-bad",
        err(deflate_init(
            ENCODING_RAW,
            &o(None, Some(99), Some(99), Some(99), None),
        )),
    );
    c.check(
        "deflate_init/window-strategy-bad",
        err(deflate_init(
            ENCODING_RAW,
            &o(None, None, Some(99), Some(99), None),
        )),
    );
    c.check(
        "deflate_init/strategy-dict-bad",
        err(deflate_init(
            ENCODING_RAW,
            &o(
                None,
                None,
                None,
                Some(99),
                Some(DictOption::Other("int".into())),
            ),
        )),
    );
    c.check(
        "deflate_init/enc-and-level-bad",
        err(deflate_init(99, &o(Some(99), None, None, None, None))),
    );
    c.check(
        "inflate_init/window-dict-bad",
        match inflate_init(
            ENCODING_RAW,
            &InflateOptions {
                window: Some(99),
                dictionary: Some(DictOption::Other("int".into())),
            },
        ) {
            Err(ZlibError::ValueError(m)) => format!("E:ValueError:{m}"),
            _ => "ok".into(),
        },
    );
    c.check(
        "gzcompress/both-bad",
        show("gzcompress", gzcompress(b"x", 99, 99)),
    );
    c.check(
        "zlib_encode/both-bad",
        show("zlib_encode", zlib_encode(b"x", 99, 99)),
    );
    c.finish(600);
}
