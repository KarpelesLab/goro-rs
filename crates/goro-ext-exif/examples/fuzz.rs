//! Mutates the given image files (byte changes, truncation) and runs every
//! reader on them, to find panics and runaway loops.
//!
//! cargo run --example fuzz -- ROUNDS FILE...

use goro_ext_exif::exif::{self, Charset, FileMeta, Ini, ReadOptions};
use goro_ext_exif::{imageinfo, iptc};

struct NoConv;
impl Charset for NoConv {
    fn convert(&self, _: &[u8], _: &[u8], _: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let rounds: usize = args.next().and_then(|r| r.parse().ok()).unwrap_or(100);
    let files: Vec<Vec<u8>> = args.map(|p| std::fs::read(p).unwrap()).collect();
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let ini = Ini::default();
    let meta = FileMeta::default();
    for r in 0..rounds {
        for f in &files {
            let mut d = f.clone();
            if d.is_empty() {
                continue;
            }
            let n = 1 + (rnd() % 8) as usize;
            for _ in 0..n {
                let i = (rnd() as usize) % d.len();
                d[i] = match rnd() % 4 {
                    0 => 0xFF,
                    1 => 0,
                    _ => rnd() as u8,
                };
            }
            if rnd() % 4 == 0 {
                let l = (rnd() as usize) % d.len();
                d.truncate(l);
            }
            let mut diags = Vec::new();
            let _ = imageinfo::image_size(&d, true, &mut diags);
            for (arrays, thumb) in [(true, true), (false, false)] {
                let o = ReadOptions {
                    required: None,
                    as_arrays: arrays,
                    read_thumbnail: thumb,
                };
                let _ = exif::read_data(&d, &meta, &o, &ini, &NoConv);
            }
            let _ = exif::thumbnail(&d, &ini, &NoConv);
            let _ = iptc::parse(&d);
            let _ = iptc::embed(b"\x1C\x02\x78\x00\x01A", &d);
        }
        if r % 100 == 0 {
            eprintln!("round {r}");
        }
    }
}
