//! Compressed output compared byte for byte with PHP 8.5
//! (tests/fixtures/deflate_php85.txt, from deflate_driver.php).

mod common;

use common::*;
use goro_ext_zlib::{DeflateParams, Deflater, Flush, Strategy, Wrap};

fn wrap_of(e: i64) -> Wrap {
    match e {
        -15 => Wrap::Raw,
        15 => Wrap::Zlib,
        31 => Wrap::Gzip,
        _ => panic!("encoding {e}"),
    }
}

fn compress(
    data: &[u8],
    level: i32,
    wrap: Wrap,
    strategy: Strategy,
    mem: u32,
    window: u32,
) -> Option<Vec<u8>> {
    let mut d = Deflater::new(DeflateParams {
        level,
        window_bits: window,
        mem_level: mem,
        strategy,
        wrap,
    })
    .ok()?;
    let mut out = Vec::new();
    d.deflate(data, Flush::Finish, &mut out);
    Some(out)
}

/// deflate_add() as PHP calls it: empty input without FINISH does nothing,
/// and a finished stream starts over.
struct Ctx {
    d: Deflater,
}

impl Ctx {
    fn add(&mut self, data: &[u8], f: Flush) -> Vec<u8> {
        let mut out = Vec::new();
        if data.is_empty() && f != Flush::Finish {
            return out;
        }
        let st = self.d.deflate(data, f, &mut out);
        if st == goro_ext_zlib::DeflateStatus::StreamEnd {
            self.d.reset();
        }
        out
    }
}

fn ctx(e: i64, level: i32, dict: Option<&[u8]>) -> Ctx {
    let mut d = Deflater::new(DeflateParams {
        level,
        wrap: wrap_of(e),
        ..Default::default()
    })
    .unwrap();
    if let Some(dict) = dict {
        let _ = d.set_dictionary(dict);
    }
    Ctx { d }
}

#[test]
fn deflate_matches_php85() {
    let inputs = inputs();
    let mut fails = Vec::new();
    let mut n = 0;
    for (key, want) in fixture("deflate_php85.txt") {
        let p: Vec<&str> = key.split('/').collect();
        let data = |name: &str| inputs.get(name).unwrap_or_else(|| panic!("{name}"));
        let got: String = match p[0] {
            "gzdeflate" | "gzcompress" | "gzencode" => {
                let w = match p[0] {
                    "gzdeflate" => Wrap::Raw,
                    "gzcompress" => Wrap::Zlib,
                    _ => Wrap::Gzip,
                };
                out_repr(
                    &compress(
                        data(p[1]),
                        p[2].parse().unwrap(),
                        w,
                        Strategy::Default,
                        9,
                        15,
                    )
                    .unwrap(),
                )
            }
            "strategy" => {
                let s = Strategy::from_i64(p[2].parse().unwrap()).unwrap();
                out_repr(&compress(data(p[1]), p[3].parse().unwrap(), Wrap::Raw, s, 8, 15).unwrap())
            }
            "memory" => out_repr(
                &compress(
                    data(p[1]),
                    p[3].parse().unwrap(),
                    Wrap::Raw,
                    Strategy::Default,
                    p[2].parse().unwrap(),
                    15,
                )
                .unwrap(),
            ),
            "window" => {
                let w = wrap_of(p[3].parse().unwrap());
                match compress(
                    data(p[1]),
                    p[4].parse().unwrap(),
                    w,
                    Strategy::Default,
                    8,
                    p[2].parse().unwrap(),
                ) {
                    Some(o) => out_repr(&o),
                    None => "false".into(),
                }
            }
            "incr" => {
                let d = data(p[1]);
                let chunk: usize = p[2].parse().unwrap();
                let f = Flush::from_i64(p[3].parse().unwrap()).unwrap();
                let mut c = ctx(p[5].parse().unwrap(), p[4].parse().unwrap(), None);
                let mut out = Vec::new();
                let mut lens = Vec::new();
                for ch in d.chunks(chunk) {
                    let o = c.add(ch, f);
                    lens.push(o.len().to_string());
                    out.extend(o);
                }
                let o = c.add(b"", Flush::Finish);
                lens.push(o.len().to_string());
                out.extend(o);
                format!("{} {}", out_repr(&out), md5_hex(lens.join(",").as_bytes()))
            }
            "dict" => {
                let dict: Vec<u8> = match p[1] {
                    "az" => {
                        let mut v = Vec::new();
                        for c in b'a'..=b'z' {
                            v.push(c);
                            v.push(0);
                        }
                        v
                    }
                    "str" => b"hello world compression".to_vec(),
                    "text" => text_data(3, 2000),
                    "big" => text_data(17, 40000),
                    _ => panic!(),
                };
                let mut c = ctx(p[3].parse().unwrap(), p[4].parse().unwrap(), Some(&dict));
                out_repr(&c.add(data(p[2]), Flush::Finish))
            }
            "seq" => continue, // in phpt_like tests
            _ => panic!("{key}"),
        };
        n += 1;
        if got != want {
            fails.push(format!("{key}\n   want {want}\n    got {got}"));
        }
    }
    if !fails.is_empty() {
        let total = fails.len();
        fails.truncate(40);
        panic!("{total}/{n} mismatches:\n{}", fails.join("\n"));
    }
}
