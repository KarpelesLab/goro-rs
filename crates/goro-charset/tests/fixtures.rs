//! Exhaustive comparison with PHP 8.5's mbstring.
//!
//! The fixtures come from one php85 run of tools/charset/fixtures/mb_decode.php
//! and mb_encode.php each, compacted by tools/charset/compact_fixture.py:
//!
//! - decode: every 1- and 2-byte string (and sampled 3/4-byte prefixes) in
//!   every encoding, converted to UTF-32BE with substitute character `%`,
//!   with `mb_check_encoding`, `mb_strlen` and the `illegal_chars` count;
//! - encode: every block of 256 code points converted from UTF-32BE into
//!   every encoding.
//!
//! Environment knobs for debugging:
//! - `GORO_CHARSET_ENC=A,B`: only these encodings;
//! - `GORO_CHARSET_FULL=DIR`: DIR holds the uncompacted dec.txt/enc.txt, and
//!   the mismatching probes are listed;
//! - `GORO_CHARSET_DUMP=DIR`: write our records to DIR/<enc>.dec/.enc in the
//!   drivers' dump format.

use goro_charset::{Encoding, Subst, check_encoding, convert, ops};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;

fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *e = c;
        }
        t
    });
    let mut c = !0u32;
    for &b in data {
        c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

fn hex(s: &[u8]) -> String {
    let mut o = String::with_capacity(s.len() * 2);
    for b in s {
        write!(o, "{b:02x}").unwrap();
    }
    o
}

fn enc_by_name(n: &str) -> Encoding {
    *goro_charset::encoding::ALL
        .iter()
        .find(|e| e.name() == n)
        .unwrap_or_else(|| panic!("unknown encoding {n}"))
}

/// The probe strings of the decode driver, keyed as it keys them.
fn dec_probes(name: &str) -> Vec<(String, Vec<Vec<u8>>)> {
    let all: Vec<u8> = (0..=255u8).collect();
    let mut v = Vec::new();
    v.push(("1".to_string(), all.iter().map(|&b| vec![b]).collect()));
    for &b1 in &all {
        v.push((
            format!("{b1:02x}"),
            all.iter().map(|&b2| vec![b1, b2]).collect(),
        ));
    }
    let utf8 = [
        "UTF-8",
        "UTF-8-Mobile#DOCOMO",
        "UTF-8-Mobile#KDDI-A",
        "UTF-8-Mobile#KDDI-B",
        "UTF-8-Mobile#SOFTBANK",
    ];
    if utf8.contains(&name) {
        let mut b2s: Vec<u8> = (0x80..=0xBF).collect();
        b2s.extend([0, 0x7F, 0xC0, 0xFF]);
        for b1 in 0xE0..=0xEFu8 {
            for &b2 in &b2s {
                v.push((
                    format!("{b1:02x}{b2:02x}"),
                    all.iter().map(|&b3| vec![b1, b2, b3]).collect(),
                ));
            }
        }
        for b1 in 0xF0..=0xF4u8 {
            for &b2 in &b2s {
                let mut strs = Vec::new();
                for b3 in [0x80, 0x9F, 0xBF, 0x41] {
                    for &b4 in &all {
                        strs.push(vec![b1, b2, b3, b4]);
                    }
                }
                v.push((format!("{b1:02x}{b2:02x}"), strs));
            }
        }
    }
    if ["EUC-JP", "eucJP-win", "EUC-JP-2004", "CP51932"].contains(&name) {
        for &b2 in &all {
            v.push((
                format!("8f{b2:02x}"),
                all.iter().map(|&b3| vec![0x8F, b2, b3]).collect(),
            ));
        }
    }
    if name == "EUC-TW" {
        let mut b3s: Vec<u8> = (0xA0..=0xFF).collect();
        b3s.extend([0, 0x41, 0x80]);
        for b2 in 0xA0..=0xB1u8 {
            for &b3 in &b3s {
                v.push((
                    format!("8e{b2:02x}{b3:02x}"),
                    all.iter().map(|&b4| vec![0x8E, b2, b3, b4]).collect(),
                ));
            }
        }
    }
    if ["GB18030", "GB18030-2022"].contains(&name) {
        let mut b4s: Vec<u8> = (0x30..=0x39).collect();
        b4s.extend([0, 0x2F, 0x3A, 0x81, 0xFF]);
        for b1 in [
            0x81u8, 0x82, 0x83, 0x84, 0x85, 0x8F, 0x90, 0x95, 0xA0, 0xE3, 0xE4, 0xFD, 0xFE,
        ] {
            for b2 in 0x30..=0x39u8 {
                let mut strs = Vec::new();
                for &b3 in &all {
                    for &b4 in &b4s {
                        strs.push(vec![b1, b2, b3, b4]);
                    }
                }
                v.push((format!("{b1:02x}{b2:02x}"), strs));
            }
        }
    }
    let wide = [
        "UTF-16", "UTF-16BE", "UTF-16LE", "UTF-32", "UTF-32BE", "UTF-32LE", "UCS-4", "UCS-4BE",
        "UCS-4LE", "UCS-2", "UCS-2BE", "UCS-2LE",
    ];
    if wide.contains(&name) {
        for b1 in [0x00u8, 0x41, 0xD7, 0xD8, 0xDB, 0xDC, 0xDF, 0xE0, 0xFE, 0xFF] {
            for b2 in [0x00u8, 0x41, 0xD8, 0xDC, 0xFE, 0xFF] {
                for b3 in [0x00u8, 0x10, 0x41, 0xD8, 0xDC, 0xDF, 0xFE, 0xFF] {
                    v.push((
                        format!("{b1:02x}{b2:02x}{b3:02x}"),
                        all.iter().map(|&b4| vec![b1, b2, b3, b4]).collect(),
                    ));
                }
            }
        }
    }
    v
}

fn dec_record(s: &[u8], e: Encoding, sub: &Subst, r: &mut String) {
    let c = convert(s, Encoding::Utf32Be, e, sub);
    let _ = write!(
        r,
        "{}{}:{}:{},",
        if check_encoding(s, e) { '1' } else { '0' },
        ops::strlen(s, e),
        c.illegal,
        hex(&c.bytes)
    );
}

/// Per-encoding probe lines ("KEY CRC") for one direction.
fn lines(name: &str, encode: bool, dump: Option<&std::path::Path>) -> Vec<String> {
    let e = enc_by_name(name);
    let sub = Subst::char(0x25);
    let mut out = Vec::new();
    let file = dump.map(|d| {
        std::fs::File::create(d.join(format!(
            "{}.{}",
            name.replace('#', "_"),
            if encode { "enc" } else { "dec" }
        )))
        .unwrap()
    });
    let mut file = file.map(std::io::BufWriter::new);
    if encode {
        let pct: Vec<u8> = [0u8, 0, 0, b'%'].repeat(256);
        let pct = convert(&pct, e, Encoding::Utf32Be, &sub).bytes;
        let mut skipped = 0;
        for blk in 0..0x1100u32 {
            let mut s = Vec::with_capacity(1024);
            for i in 0..256 {
                s.extend_from_slice(&(blk * 256 + i).to_be_bytes());
            }
            let c = convert(&s, e, Encoding::Utf32Be, &sub);
            if c.bytes == pct && c.illegal == 256 {
                skipped += 1;
                continue;
            }
            let mut rec = format!("{}:", c.illegal).into_bytes();
            rec.extend_from_slice(&c.bytes);
            out.push(format!("{blk:04x} {:08x}", crc32(&rec)));
            if let Some(f) = file.as_mut() {
                writeln!(f, "{blk:04x} {} {}", c.illegal, hex(&c.bytes)).unwrap();
            }
        }
        out.push(format!("empty {skipped}"));
    } else {
        for (key, strs) in dec_probes(name) {
            let mut r = String::new();
            for s in &strs {
                dec_record(s, e, &sub, &mut r);
            }
            out.push(format!("{key} {:08x}", crc32(r.as_bytes())));
            if let Some(f) = file.as_mut() {
                writeln!(f, "{key} {r}").unwrap();
            }
        }
    }
    out
}

/// Groups lines by 16 as compact_fixture.py does.
fn compact(lines: &[String]) -> Vec<String> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("empty ") {
            v.push(lines[i].clone());
            i += 1;
            continue;
        }
        let first = lines[i].split(' ').next().unwrap().to_string();
        let mut buf = String::new();
        let mut n = 0;
        while i < lines.len() && n < 16 && !lines[i].starts_with("empty ") {
            buf.push_str(&lines[i]);
            buf.push('\n');
            i += 1;
            n += 1;
        }
        v.push(format!("{first} {:08x}", crc32(buf.as_bytes())));
    }
    v
}

fn run(fixture: &str, full_name: &str, encode: bool) {
    let mut expected: Vec<(String, Vec<String>)> = Vec::new();
    for line in fixture.lines() {
        let (enc, rest) = line.split_once(' ').unwrap();
        match expected.last_mut() {
            Some((e, v)) if e == enc => v.push(rest.to_string()),
            _ => expected.push((enc.to_string(), vec![rest.to_string()])),
        }
    }
    let only: Option<Vec<String>> = std::env::var("GORO_CHARSET_ENC")
        .ok()
        .map(|s| s.split(',').map(str::to_string).collect());
    let full: Option<HashMap<String, Vec<String>>> =
        std::env::var("GORO_CHARSET_FULL").ok().map(|d| {
            let text = std::fs::read_to_string(std::path::Path::new(&d).join(full_name)).unwrap();
            let mut m: HashMap<String, Vec<String>> = HashMap::new();
            for l in text.lines() {
                let (e, rest) = l.split_once(' ').unwrap();
                m.entry(e.to_string()).or_default().push(rest.to_string());
            }
            m
        });
    let dump = std::env::var("GORO_CHARSET_DUMP").ok();
    let dump = dump.as_deref().map(std::path::Path::new);
    let mut failed = Vec::new();
    for (enc, exp) in &expected {
        if let Some(o) = &only
            && !o.contains(enc)
        {
            continue;
        }
        let got = lines(enc, encode, dump);
        let got_c = compact(&got);
        let bad = got_c.iter().zip(exp).filter(|(a, b)| a != b).count()
            + got_c.len().abs_diff(exp.len());
        if bad > 0 {
            let mut msg = format!("{enc}: {bad} of {} groups differ", exp.len());
            if let Some(full) = &full {
                let fl = &full[enc];
                let diffs: Vec<&String> = got
                    .iter()
                    .zip(fl)
                    .filter(|(a, b)| a != b)
                    .map(|(a, _)| a)
                    .collect();
                let keys: Vec<&str> = diffs
                    .iter()
                    .take(40)
                    .map(|l| l.split(' ').next().unwrap())
                    .collect();
                msg += &format!(" ({} probes: {})", diffs.len(), keys.join(" "));
            }
            failed.push(msg);
        }
    }
    if !failed.is_empty() {
        panic!(
            "{} encodings differ from PHP 8.5:\n{}",
            failed.len(),
            failed.join("\n")
        );
    }
}

#[test]
fn decode_matches_php() {
    run(include_str!("fixtures/mb_decode.txt"), "dec.txt", false);
}

#[test]
fn encode_matches_php() {
    run(include_str!("fixtures/mb_encode.txt"), "enc.txt", true);
}
