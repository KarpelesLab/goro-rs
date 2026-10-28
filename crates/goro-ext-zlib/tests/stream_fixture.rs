//! zlib.* stream filters and the gz file layer compared with PHP 8.5
//! (tests/fixtures/behaviour_php85.txt and behaviour2_php85.txt). The PHP
//! stream layer itself (read buffering, fgets, the eof flag) belongs to the
//! engine, so these cases re-enact only what passes through the filter or
//! the gz reader/writer.

mod common;

use common::*;
use goro_ext_zlib::filter::*;
use goro_ext_zlib::gzfile::*;
use goro_ext_zlib::*;
use std::collections::BTreeMap;
use std::io::Cursor;

fn want() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for f in [
        "behaviour_php85.txt",
        "behaviour2_php85.txt",
        "behaviour3_php85.txt",
    ] {
        for (k, v) in fixture(f) {
            // behaviour3 reuses some names for other cases: first wins.
            m.entry(k).or_insert(v);
        }
    }
    m
}

fn val_str(s: &[u8]) -> String {
    format!("=> s:{}", out_repr(s))
}

fn diag_prefix(d: &Diag, func: &str) -> String {
    match d {
        Diag::Warning(m) => format!("W:{func}(): {m}"),
        Diag::Notice(m) => format!("N:{func}(): {m}"),
    }
}

fn params(name: &str) -> FilterParams {
    let map = |level, window, memory| FilterParams::Map {
        level,
        window,
        memory,
    };
    match name {
        "none" => FilterParams::None,
        "level9" => map(Some(9), None, None),
        "level0" => map(Some(0), None, None),
        "level10" => map(Some(10), None, None),
        "level-2" => map(Some(-2), None, None),
        "memory1" => map(None, None, Some(1)),
        "memory10" => map(None, None, Some(10)),
        "int" => FilterParams::Scalar(5),
        "str" => FilterParams::Scalar(0),
        "strategy" => map(None, None, None),
        "obj" => map(Some(1), None, None),
        w if w.starts_with("window") => map(None, Some(w[6..].parse().unwrap()), None),
        _ => panic!("{name}"),
    }
}

/// filt_write() of the driver for zlib.deflate: each fwrite() is a bucket,
/// fflush() an incremental flush, stream_filter_remove() the close.
fn deflate_write(p: &FilterParams, chunks: &[&[u8]], flush: bool) -> (Vec<Diag>, Option<Vec<u8>>) {
    let mut diags = Vec::new();
    let Some(mut f) = deflate_filter(p, &mut diags) else {
        diags.push(Diag::Warning(
            "Unable to create or locate filter \"zlib.deflate\"".into(),
        ));
        return (diags, None);
    };
    let mut out = Vec::new();
    for c in chunks {
        out.extend(f.filter(c, FilterFlush::None));
        if flush {
            out.extend(f.filter(b"", FilterFlush::Inc));
        }
    }
    out.extend(f.filter(b"", FilterFlush::Close));
    (diags, Some(out))
}

fn show(diags: &[String], r: Option<String>) -> String {
    let mut parts = diags.to_vec();
    parts.push(match r {
        Some(s) => val_str(s.as_bytes()),
        None => "=> false".into(),
    });
    parts.join(" | ")
}

#[test]
fn filters() {
    let want = want();
    let inputs = inputs();
    let mut fails = Vec::new();
    let mut n = 0;
    let mut check = |k: String, got: String| {
        n += 1;
        match want.get(&k) {
            Some(w) if *w == got => {}
            Some(w) => fails.push(format!("{k}\n   want {w}\n    got {got}")),
            None => fails.push(format!("{k}: missing")),
        }
    };
    let names = [
        "none",
        "level9",
        "level0",
        "level10",
        "level-2",
        "window9",
        "window15",
        "window31",
        "window47",
        "window-15",
        "window7",
        "window50",
        "memory1",
        "memory10",
        "int",
        "str",
        "strategy",
        "obj",
    ];
    let parts: [&[u8]; 3] = [b"hello ", b"hello ", b"world"];
    for name in names {
        for flush in [false, true] {
            let (d, r) = deflate_write(&params(name), &parts, flush);
            let d: Vec<String> = d
                .iter()
                .map(|d| diag_prefix(d, "stream_filter_append"))
                .collect();
            let key = format!("filter/deflate/{name}{}", if flush { "/flush" } else { "" });
            check(key, show(&d, r.map(|v| hex(&v))));
        }
    }
    let big: Vec<&[u8]> = inputs["mixed400k"].chunks(10000).collect();
    check(
        "filter/deflate/big".into(),
        show(
            &[],
            deflate_write(&FilterParams::None, &big, false)
                .1
                .map(|v| out_repr(&v)),
        ),
    );
    let t: Vec<&[u8]> = inputs["text70k"].chunks(7000).collect();
    let l6 = FilterParams::Map {
        level: Some(6),
        window: None,
        memory: None,
    };
    check(
        "filter/deflate/big/flush".into(),
        show(&[], deflate_write(&l6, &t, true).1.map(|v| out_repr(&v))),
    );

    let text5000 = gzdeflate(&inputs["text5000"], -1, ENCODING_RAW).unwrap();
    let datas: Vec<(&str, Vec<u8>)> = vec![
        ("raw", gzdeflate(b"hello world", -1, ENCODING_RAW).unwrap()),
        (
            "zlib",
            gzcompress(b"hello world", -1, ENCODING_DEFLATE).unwrap(),
        ),
        ("gzip", gzencode(b"hello world", -1, ENCODING_GZIP).unwrap()),
        ("garbage", b"garbage data".to_vec()),
        (
            "rawtail",
            [
                gzdeflate(b"hello world", -1, ENCODING_RAW).unwrap(),
                b"tail".to_vec(),
            ]
            .concat(),
        ),
        ("trunc", text5000[..500].to_vec()),
    ];
    for pname in [
        "none",
        "window15",
        "window31",
        "window47",
        "window-15",
        "window7",
        "window9",
        "window50",
        "int",
    ] {
        for (dname, d) in &datas {
            for mode in ["read", "write"] {
                let mut diags = Vec::new();
                let created = inflate_filter(&params(pname), &mut diags);
                let mut ds: Vec<String> = diags
                    .iter()
                    .map(|d| diag_prefix(d, "stream_filter_append"))
                    .collect();
                let r = match created {
                    None => {
                        ds.push("W:stream_filter_append(): Unable to create or locate filter \"zlib.inflate\"".into());
                        None
                    }
                    Some(mut f) => {
                        let res = f.filter(d, FilterFlush::None).and_then(|mut o| {
                            o.extend(f.filter(b"", FilterFlush::Close)?);
                            Ok(o)
                        });
                        match res {
                            Ok(o) => Some(out_repr(&o)),
                            Err(e) => {
                                let func = if mode == "read" { "fread" } else { "fwrite" };
                                ds.push(diag_prefix(&e, func));
                                Some(if mode == "read" {
                                    "false".into()
                                } else {
                                    String::new()
                                })
                            }
                        }
                    }
                };
                // Small outputs come back in one fread().
                check(
                    format!("filter/inflate/{pname}/{dname}/{mode}"),
                    show(&ds, r),
                );
            }
        }
    }
    let mut f = inflate_filter(&FilterParams::None, &mut Vec::new()).unwrap();
    let two = [
        gzdeflate(b"abc", -1, ENCODING_RAW).unwrap(),
        gzdeflate(b"def", -1, ENCODING_RAW).unwrap(),
    ]
    .concat();
    check(
        "filter/inflate/two-streams".into(),
        show(
            &[],
            Some(out_repr(&f.filter(&two, FilterFlush::None).unwrap())),
        ),
    );
    let mut f = inflate_filter(&FilterParams::None, &mut Vec::new()).unwrap();
    let big = gzdeflate(&inputs["mixed400k"], -1, ENCODING_RAW).unwrap();
    let mut out = Vec::new();
    for ch in big.chunks(8192) {
        out.extend(f.filter(ch, FilterFlush::None).unwrap());
    }
    // fread() returns full chunks; feof() is known after a read at the end.
    let mut reads: Vec<String> = out.chunks(100000).map(out_repr).collect();
    if out.len() % 100000 == 0 {
        reads.push(String::new());
    }
    check(
        "filter/inflate/big/read".into(),
        show(&[], Some(reads.join(","))),
    );

    // behaviour3: level 0 and 6 through the filter, with the output size of
    // each write.
    for (chunk, fl) in [
        (1000usize, false),
        (1000, true),
        (40000, false),
        (40000, true),
        (100000, false),
        (70000, true),
    ] {
        for level in [0i64, 6] {
            let p = FilterParams::Map {
                level: Some(level),
                window: None,
                memory: None,
            };
            let mut f = deflate_filter(&p, &mut Vec::new()).unwrap();
            let mut out = Vec::new();
            let mut lens = Vec::new();
            for c in inputs["mixed400k"].chunks(chunk) {
                let mut o = f.filter(c, FilterFlush::None);
                if fl {
                    o.extend(f.filter(b"", FilterFlush::Inc));
                }
                lens.push(o.len().to_string());
                out.extend(o);
            }
            let o = f.filter(b"", FilterFlush::Close);
            lens.push(o.len().to_string());
            out.extend(o);
            let s = format!("{} {}", out_repr(&out), md5_hex(lens.join(",").as_bytes()));
            check(
                format!("filter0/{chunk}/{}/{level}", if fl { "f" } else { "n" }),
                show(&[], Some(s)),
            );
        }
    }
    let mut d = Vec::new();
    let r = deflate_filter(
        &FilterParams::Map {
            level: Some(99),
            window: Some(99),
            memory: Some(99),
        },
        &mut d,
    )
    .is_some();
    let mut ds: Vec<String> = d
        .iter()
        .map(|d| diag_prefix(d, "stream_filter_append"))
        .collect();
    ds.push(format!("=> {r}"));
    check("filter/all-bad".into(), ds.join(" | "));

    if !fails.is_empty() {
        panic!("{} mismatches of {n}:\n{}", fails.len(), fails.join("\n"));
    }
    assert!(n > 150);
}

/// gzw() of the driver: writes parts (or seeks), then gztell and close.
fn gzw(mode: &str, existing: &[u8], parts: &[Part]) -> Result<(Vec<String>, Vec<u8>), String> {
    let m = parse_mode(mode.as_bytes())
        .map_err(|e| format!("W:gzopen(): {} | => false", e.message()))?;
    let mut file = if m.append {
        existing.to_vec()
    } else {
        Vec::new()
    };
    let start = file.len();
    let mut diags = Vec::new();
    let mut r = Vec::new();
    {
        let mut sink = Cursor::new(&mut file);
        sink.set_position(start as u64);
        let mut w = GzWriter::from_mode(sink, &m);
        let mut wrote = false;
        for p in parts {
            match p {
                Part::Data(d) => {
                    r.push(w.write(d).unwrap().to_string());
                    wrote |= !d.is_empty();
                }
                Part::Seek(o, whence) => match whence {
                    None => r.push(w.seek(*o, Whence::Set).min(0).to_string()),
                    Some(Whence::Cur) => r.push(w.seek(*o, Whence::Cur).min(0).to_string()),
                    Some(Whence::Set) => {
                        diags.push("W:gzseek(): SEEK_END is not supported".to_string());
                        r.push("-1".into());
                    }
                },
            }
        }
        r.push(w.tell().to_string());
        if wrote {
            w.flush(Flush::Sync).unwrap();
        }
        w.close().unwrap();
    }
    Ok((
        diags,
        format!("{} {}", r.join(","), hex(&file)).into_bytes(),
    ))
}

enum Part {
    Data(Vec<u8>),
    /// Offset and whence; `Some(Set)` stands for SEEK_END here.
    Seek(i64, Option<Whence>),
}

fn show_gzw(r: Result<(Vec<String>, Vec<u8>), String>, md5: bool) -> String {
    match r {
        Err(e) => e,
        Ok((mut d, v)) => {
            let s = if md5 {
                md5_hex(&v)
            } else {
                String::from_utf8(v).unwrap()
            };
            d.push(val_str(s.as_bytes()));
            d.join(" | ")
        }
    }
}

fn read_all(data: &[u8]) -> Result<Vec<u8>, GzError> {
    let mut r = GzReader::new(Cursor::new(data.to_vec()));
    let mut out = Vec::new();
    let mut buf = vec![0u8; 8192];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..n]);
    }
}

#[test]
fn gz_files() {
    let want = want();
    let inputs = inputs();
    let mut fails = Vec::new();
    let mut n = 0;
    let mut check = |k: &str, got: String| {
        n += 1;
        match want.get(k) {
            Some(w) if *w == got => {}
            Some(w) => fails.push(format!("{k}\n   want {w}\n    got {got}")),
            None => fails.push(format!("{k}: missing")),
        }
    };
    let hw = || {
        vec![
            Part::Data(b"hello hello hello ".to_vec()),
            Part::Data(b"world".to_vec()),
        ]
    };
    for mode in [
        "w", "wb", "w0", "w1", "w9", "wf", "wh", "wR", "wF", "w9h", "wT", "a", "ab", "w+", "r+",
        "c", "w5e", "wb6f",
    ] {
        check(
            &format!("gzopen/w/{mode}"),
            show_gzw(gzw(mode, b"", &hw()), false),
        );
    }
    let first = gzw("w", b"", &[Part::Data(b"first ".to_vec())]).unwrap().1;
    let first_file = unhex(
        std::str::from_utf8(&first)
            .unwrap()
            .split(' ')
            .nth(1)
            .unwrap(),
    );
    check(
        "gzopen/append",
        show_gzw(
            gzw("a", &first_file, &[Part::Data(b"second".to_vec())]),
            false,
        ),
    );
    check(
        "gzopen/seek-write",
        show_gzw(
            gzw(
                "w",
                b"",
                &[
                    Part::Data(b"abc".to_vec()),
                    Part::Seek(10, None),
                    Part::Data(b"def".to_vec()),
                    Part::Seek(5, None),
                    Part::Seek(20, Some(Whence::Cur)),
                    Part::Data(b"g".to_vec()),
                    Part::Seek(0, Some(Whence::Set)),
                ],
            ),
            false,
        ),
    );
    let chunks = |d: &[u8], k: usize| {
        d.chunks(k)
            .map(|c| Part::Data(c.to_vec()))
            .collect::<Vec<_>>()
    };
    check(
        "gzopen/big-write",
        closure_md5(gzw("w", b"", &chunks(&inputs["mixed400k"], 3000))),
    );
    check(
        "gzopen/big-write-w0",
        closure_md5(gzw("w0", b"", &chunks(&inputs["mixed400k"], 3000))),
    );
    check(
        "gzopen/big-write-w1",
        closure_md5(gzw("w1", b"", &chunks(&inputs["text300k"], 70000))),
    );
    {
        let mut file = Vec::new();
        let mut w = GzWriter::new(Cursor::new(&mut file), -1, Strategy::Default, false);
        w.write(b"part1").unwrap();
        w.flush(Flush::Sync).unwrap();
        w.write(b"part2").unwrap();
        w.flush(Flush::Sync).unwrap();
        w.flush(Flush::Sync).unwrap();
        w.flush(Flush::Sync).unwrap();
        w.close().unwrap();
        check("gzopen/flush", val_str(hex(&file).as_bytes()));
    }
    {
        let mut file = Vec::new();
        let mut w = GzWriter::new(Cursor::new(&mut file), -1, Strategy::Default, false);
        let a = w.write(&b"hello world"[..5]).unwrap();
        let b = w.write(&b"xyz"[..0]).unwrap();
        let c = w.write(b"xyz").unwrap();
        let d = w.write(b"").unwrap();
        w.flush(Flush::Sync).unwrap();
        w.close().unwrap();
        let first =
            String::from_utf8(gzw("w", b"", &[Part::Data(b"hello".to_vec())]).unwrap().1).unwrap();
        let s = format!("{first}|{a},{b},{c},{d},{}", hex(&file));
        check("gzwrite/len", val_str(s.as_bytes()));
    }

    let files: Vec<(&str, Vec<u8>)> = vec![
        (
            "gz",
            gzencode(b"line one\nline two\nthird line here\n", -1, ENCODING_GZIP).unwrap(),
        ),
        ("plain", b"plain text\nnot compressed\n".to_vec()),
        ("empty", vec![]),
        (
            "two-members",
            [
                gzencode(b"first\n", -1, ENCODING_GZIP).unwrap(),
                gzencode(b"second\n", -1, ENCODING_GZIP).unwrap(),
            ]
            .concat(),
        ),
        (
            "member-garbage",
            [
                gzencode(b"first\n", -1, ENCODING_GZIP).unwrap(),
                b"garbage".to_vec(),
            ]
            .concat(),
        ),
        (
            "truncated",
            gzencode(&inputs["text5000"], -1, ENCODING_GZIP).unwrap()[..1000].to_vec(),
        ),
        ("corrupt", {
            let mut s = gzencode(
                b"The quick brown fox jumps over the lazy dog",
                -1,
                ENCODING_GZIP,
            )
            .unwrap();
            s[15] = b'X';
            s
        }),
        ("badcrc", {
            let mut s = gzencode(b"hello there", -1, ENCODING_GZIP).unwrap();
            let l = s.len();
            s[l - 6] = b'X';
            s
        }),
        ("one-byte", b"\x1f".to_vec()),
        ("magic-only", b"\x1f\x8b".to_vec()),
        (
            "header-only",
            gzencode(b"abc", -1, ENCODING_GZIP).unwrap()[..10].to_vec(),
        ),
        (
            "zlib",
            gzcompress(b"zlib data", -1, ENCODING_DEFLATE).unwrap(),
        ),
        (
            "big",
            gzencode(&inputs["mixed400k"], -1, ENCODING_GZIP).unwrap(),
        ),
    ];
    for (fname, data) in &files {
        let all = read_all(data);
        // readgzfile() prints the data and returns its length, -1 on error.
        let (len, content) = match &all {
            Ok(v) => (v.len() as i64, v.clone()),
            Err(_) => (-1, vec![]),
        };
        check(
            &format!("readgzfile/{fname}"),
            val_str(format!("{len}:{}", md5_hex(&content)).as_bytes()),
        );
        check(
            &format!("compress.zlib/{fname}"),
            val_str(format!("{}{}", md5_hex(&content), content.len()).as_bytes()),
        );
        // gzfile(): the lines joined back; PHP 8.5 cuts each line at its
        // first NUL byte (behaviour3_php85.txt).
        let lines: Vec<&[u8]> = if content.is_empty() {
            vec![]
        } else {
            content
                .split_inclusive(|&b| b == b'\n')
                .map(|l| &l[..l.iter().position(|&b| b == 0).unwrap_or(l.len())])
                .collect()
        };
        check(
            &format!("gzfile/{fname}"),
            val_str(format!("{}{}", md5_hex(&lines.join(&b'|')), lines.len()).as_bytes()),
        );
        if *fname != "big" {
            let r = match &all {
                Ok(v) => format!("r:{},e:true,r:,e:true,t:{}", hex(v), v.len()),
                Err(_) => "r:F,e:false,r:F,e:false,t:0".into(),
            };
            let r = if r.len() > 2000 {
                md5_hex(r.as_bytes())
            } else {
                r
            };
            if *fname != "truncated" {
                check(&format!("gzr/{fname}/readall"), val_str(r.as_bytes()));
            }
        }
    }
    for m in ["w", "w0", "wT"] {
        let mode = parse_mode(m.as_bytes()).unwrap();
        let mut f0 = Vec::new();
        GzWriter::from_mode(Cursor::new(&mut f0), &mode)
            .close()
            .unwrap();
        check(&format!("gzempty/{m}"), val_str(hex(&f0).as_bytes()));
        let mut f1 = Vec::new();
        let mut w = GzWriter::from_mode(Cursor::new(&mut f1), &mode);
        w.write(b"").unwrap();
        w.close().unwrap();
        check(&format!("gzempty-write0/{m}"), val_str(hex(&f1).as_bytes()));
        let mut f2 = Vec::new();
        let mut w = GzWriter::from_mode(Cursor::new(&mut f2), &mode);
        w.flush(Flush::Sync).unwrap();
        w.close().unwrap();
        check(&format!("gzempty-flush/{m}"), val_str(hex(&f2).as_bytes()));
    }
    let modes: Vec<String> = ["r", "rb", "rt", "r9", "rf", "rh", "re", "rx", "rT"]
        .iter()
        .map(|m| {
            format!(
                "{m}={}",
                match parse_mode(m.as_bytes()) {
                    Ok(_) => "abc",
                    Err(_) => "F",
                }
            )
        })
        .collect();
    check(
        "gzopen/modes-r",
        format!(
            "W:gzopen(): gzopen failed | {}",
            val_str(modes.join(",").as_bytes())
        ),
    );

    if !fails.is_empty() {
        panic!("{} mismatches of {n}:\n{}", fails.len(), fails.join("\n"));
    }
}

fn closure_md5(r: Result<(Vec<String>, Vec<u8>), String>) -> String {
    show_gzw(r, true)
}

#[test]
fn gz_reader_semantics() {
    // zlib manual: gzseek emulates seeking backwards by rewinding, and a
    // forward seek is done lazily (observed on PHP 8.5: seeking past the
    // end succeeds and gztell reports the requested position).
    let data = gzencode(b"line one\nline two\nthird line here\n", -1, ENCODING_GZIP).unwrap();
    let mut r = GzReader::new(Cursor::new(data));
    let mut b = [0u8; 4];
    assert_eq!(r.seek(5, Whence::Set), 5);
    assert_eq!(r.read(&mut b).unwrap(), 4);
    assert_eq!(&b, b"one\n");
    assert_eq!(r.seek(2, Whence::Set), 2);
    assert_eq!(r.read(&mut b[..3]).unwrap(), 3);
    assert_eq!(&b[..3], b"ne ");
    assert_eq!(r.seek(-100, Whence::Cur), -1);
    assert_eq!(r.seek(1000, Whence::Set), 1000);
    assert_eq!(r.tell(), 1000);
    assert_eq!(r.read(&mut b).unwrap(), 0);
    assert!(r.eof());
    // zlib's position stops at the end of the data; PHP's gztell() reports
    // the stream layer's position instead.
    assert_eq!(r.tell(), 34);
    r.rewind().unwrap();
    assert_eq!(r.tell(), 0);
    assert_eq!(r.read(&mut b).unwrap(), 4);
    assert_eq!(&b, b"line");

    // Plain files are read as they are, with real seeks.
    let mut r = GzReader::new(Cursor::new(b"plain text\nnot compressed\n".to_vec()));
    assert_eq!(r.read(&mut b).unwrap(), 4);
    assert_eq!(r.seek(2, Whence::Set), 2);
    assert_eq!(r.read(&mut b).unwrap(), 4);
    assert_eq!(&b, b"ain ");

    // Corrupt data is an error, truncated data is not.
    let mut s = gzencode(
        b"The quick brown fox jumps over the lazy dog",
        -1,
        ENCODING_GZIP,
    )
    .unwrap();
    s[15] = b'X';
    let mut r = GzReader::new(Cursor::new(s));
    assert!(r.read(&mut b).is_err());
    assert!(!r.eof());
    assert!(r.rewind().is_err());
    let t = gzencode(&text_data(3, 5000), -1, ENCODING_GZIP).unwrap()[..1000].to_vec();
    let mut r = GzReader::new(Cursor::new(t));
    let mut big = vec![0u8; 100000];
    let n = r.read(&mut big).unwrap();
    assert!(n > 2000 && n < 5000);
    assert!(r.truncated());
}
