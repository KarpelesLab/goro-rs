//! Prints what getimagesize(), exif_imagetype(), exif_read_data() and
//! exif_thumbnail() return for the given files, in var_dump() format, to
//! compare the library with recorded PHP 8.5 output.
//!
//! cargo run --example dump -- FILE...

use goro_ext_exif::Diag;
use goro_ext_exif::exif::{self, Charset, FileMeta, Ini, Node, ReadOptions};
use goro_ext_exif::imageinfo;
use std::io::Write;

struct Ucs2;

impl Charset for Ucs2 {
    fn convert(&self, data: &[u8], to: &[u8], from: &[u8]) -> Option<Vec<u8>> {
        let be = match from {
            b"UCS-2BE" => true,
            b"UCS-2LE" => false,
            _ => return None,
        };
        let mut out = Vec::new();
        for c in data.chunks(2) {
            if c.len() < 2 {
                break;
            }
            let u = if be {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from_le_bytes([c[0], c[1]])
            };
            let b = match (to, u) {
                (_, 0..=0x7F) => u as u8,
                (b"ISO-8859-1", 0x80..=0xFF) => u as u8,
                (b"ISO-8859-15", 0xA0..=0xFF)
                    if !matches!(u, 0xA4 | 0xA6 | 0xA8 | 0xB4 | 0xB8 | 0xBC | 0xBD | 0xBE) =>
                {
                    u as u8
                }
                (b"ISO-8859-15", 0x80..=0x9F) => u as u8,
                _ => b'?',
            };
            out.push(b);
        }
        Some(out)
    }
}

fn fmt_float(f: f64) -> String {
    if f.is_nan() {
        return "NAN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "INF".into() } else { "-INF".into() };
    }
    if f == f.trunc() && f.abs() < 1e15 {
        return format!("{}", f as i64);
    }
    let s = format!("{:e}", f);
    let (m, e) = s.split_once('e').unwrap();
    let e: i32 = e.parse().unwrap();
    if (-5..15).contains(&e) {
        return format!("{}", f);
    }
    let m = if m.contains('.') { m.to_string() } else { format!("{m}.0") };
    format!("{}E{}{}", m, if e < 0 { "-" } else { "+" }, e.abs())
}

fn dump(out: &mut Vec<u8>, n: &Node, ind: usize) {
    let pad = " ".repeat(ind);
    match n {
        Node::Null => out.extend_from_slice(b"NULL\n"),
        Node::Int(i) => out.extend_from_slice(format!("int({i})\n").as_bytes()),
        Node::Float(f) => out.extend_from_slice(format!("float({})\n", fmt_float(*f)).as_bytes()),
        Node::Str(s) => {
            out.extend_from_slice(format!("string({}) \"", s.len()).as_bytes());
            out.extend_from_slice(s);
            out.extend_from_slice(b"\"\n");
        }
        Node::List(l) => {
            out.extend_from_slice(format!("array({}) {{\n", l.len()).as_bytes());
            for (i, v) in l.iter().enumerate() {
                out.extend_from_slice(format!("{pad}  [{i}]=>\n{pad}  ").as_bytes());
                dump(out, v, ind + 2);
            }
            out.extend_from_slice(format!("{pad}}}\n").as_bytes());
        }
        Node::Map(m) => {
            out.extend_from_slice(format!("array({}) {{\n", m.len()).as_bytes());
            for (k, v) in m {
                out.extend_from_slice(format!("{pad}  [\"").as_bytes());
                out.extend_from_slice(k);
                out.extend_from_slice(format!("\"]=>\n{pad}  ").as_bytes());
                dump(out, v, ind + 2);
            }
            out.extend_from_slice(format!("{pad}}}\n").as_bytes());
        }
    }
}

fn diags(out: &mut Vec<u8>, func: &str, name: &str, d: &[Diag]) {
    for x in d {
        let (lvl, msg, bare) = match x {
            Diag::ReadError => ("Notice", format!("Error reading from {name}!"), true),
            Diag::Warning(m) => ("Warning", m.clone(), false),
            Diag::Notice(m) => ("Notice", m.clone(), false),
            Diag::WarningBare(m) => ("Warning", m.clone(), true),
            Diag::NoticeBare(m) => ("Notice", m.clone(), true),
        };
        let base = name.rsplit('/').next().unwrap();
        if bare || !func.starts_with("exif_r") && !func.starts_with("exif_t") {
            out.extend_from_slice(format!("\n{lvl}: {func}(): {msg} in X\n").as_bytes());
        } else {
            out.extend_from_slice(format!("\n{lvl}: {func}({base}): {msg} in X\n").as_bytes());
        }
    }
}

fn main() {
    let ini = Ini::default();
    let mut out = Vec::new();
    for p in std::env::args().skip(1) {
        let d = std::fs::read(&p).unwrap();
        let rel = p.replace("/tmp/claude-1000/php-tests/", "");
        out.extend_from_slice(format!("##### {rel}\n-- getimagesize\n").as_bytes());
        let mut dg = Vec::new();
        let (r, info) = imageinfo::image_size(&d, true, &mut dg);
        diags(&mut out, "getimagesize", &p, &dg);
        match r {
            None => out.extend_from_slice(b"bool(false)\n"),
            Some(s) => {
                let mut m = vec![];
                let mut l = vec![
                    Node::Int(s.width as i64),
                    Node::Int(s.height as i64),
                    Node::Int(s.itype),
                ];
                if let Some(h) = &s.html {
                    l.push(Node::Str(h.clone().into_bytes()));
                }
                for (i, v) in l.into_iter().enumerate() {
                    m.push((i.to_string().into_bytes(), v));
                }
                if s.bits != 0 {
                    m.push((b"bits".to_vec(), Node::Int(s.bits as i64)));
                }
                if s.channels != 0 {
                    m.push((b"channels".to_vec(), Node::Int(s.channels as i64)));
                }
                m.push((b"mime".to_vec(), Node::Str(imageinfo::mime_type(s.itype).into())));
                m.push((b"width_unit".to_vec(), Node::Str(s.width_unit.into())));
                m.push((b"height_unit".to_vec(), Node::Str(s.height_unit.into())));
                let mut tmp = Vec::new();
                dump(&mut tmp, &Node::Map(m), 0);
                // list keys print as [0]
                let t = String::from_utf8_lossy(&tmp)
                    .replace("[\"0\"]", "[0]")
                    .replace("[\"1\"]", "[1]")
                    .replace("[\"2\"]", "[2]")
                    .replace("[\"3\"]", "[3]");
                out.extend_from_slice(t.as_bytes());
            }
        }
        for (k, v) in info {
            out.extend_from_slice(
                format!("{k}: {} {:032x}\n", v.len(), md5(&v)).as_bytes(),
            );
        }
        out.extend_from_slice(b"-- exif_imagetype\n");
        let mut dg = Vec::new();
        let t = imageinfo::image_type(&d, &mut dg);
        diags(&mut out, "exif_imagetype", &p, &dg);
        if t == 0 {
            out.extend_from_slice(b"bool(false)\n");
        } else {
            out.extend_from_slice(format!("int({t})\n").as_bytes());
        }
        let meta = FileMeta {
            name: Some(p.rsplit('/').next().unwrap().as_bytes().to_vec()),
            mtime: std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64)
                .unwrap_or(0),
        };
        for (label, arrays, thumb) in [("arrays", true, true), ("flat", false, false)] {
            out.extend_from_slice(format!("-- exif_read_data {label}\n").as_bytes());
            let opts = ReadOptions {
                required: None,
                as_arrays: arrays,
                read_thumbnail: thumb,
            };
            let (r, dg) = exif::read_data(&d, &meta, &opts, &ini, &Ucs2);
            diags(&mut out, "exif_read_data", &p, &dg);
            match r {
                None => out.extend_from_slice(b"bool(false)\n"),
                Some(n) => dump(&mut out, &n, 0),
            }
        }
        out.extend_from_slice(b"-- exif_thumbnail\n");
        let (r, dg) = exif::thumbnail(&d, &ini, &Ucs2);
        diags(&mut out, "exif_thumbnail", &p, &dg);
        match r {
            None => out.extend_from_slice(b"bool(false)\nNULL\nNULL\nNULL\n"),
            Some(t) => out.extend_from_slice(
                format!(
                    "array(2) {{\n  [0]=>\n  int({})\n  [1]=>\n  string(32) \"{:032x}\"\n}}\nint({})\nint({})\nint({})\n",
                    t.data.len(),
                    md5(&t.data),
                    t.width,
                    t.height,
                    t.image_type
                )
                .as_bytes(),
            ),
        }
    }
    std::io::stdout().write_all(&out).unwrap();
}

/// MD5 (RFC 1321), for the APPn and thumbnail digests.
fn md5(data: &[u8]) -> u128 {
    let s: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64)
        .map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32)
        .collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = data.to_vec();
    let bitlen = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk
            .chunks(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f2 = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f2.rotate_left(s[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut r = [0u8; 16];
    r[..4].copy_from_slice(&a0.to_le_bytes());
    r[4..8].copy_from_slice(&b0.to_le_bytes());
    r[8..12].copy_from_slice(&c0.to_le_bytes());
    r[12..].copy_from_slice(&d0.to_le_bytes());
    u128::from_be_bytes(r)
}
