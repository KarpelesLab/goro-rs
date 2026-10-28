//! encoding.rs against PHP 8.5 output (tests/fixtures/encoding.txt, made
//! by encoding.php).

mod common;

use common::*;
use goro_ext_standard::encoding as e;

fn opt(v: Option<Vec<u8>>) -> V {
    match v {
        Some(v) => V::S(v),
        None => V::B(false),
    }
}

fn run(c: &Case, func: &str) -> Option<(Out, Vec<goro_ext_standard::error::Diag>)> {
    if c.func != func {
        return None;
    }
    let a = c.args[0].s();
    let raw = || c.args.get(1).map(|v| v.b()).unwrap_or(false);
    let mut d = Vec::new();
    let v = match func {
        "bin2hex" => V::S(e::bin2hex(a)),
        "hex2bin" => opt(e::hex2bin(a, &mut d)),
        "crc32" => V::I(e::crc32(a) as i64),
        "md5" => V::S(e::md5(a, raw())),
        "sha1" => V::S(e::sha1(a, raw())),
        "str_rot13" => V::S(e::str_rot13(a)),
        "quoted_printable_encode" => V::S(e::quoted_printable_encode(a)),
        "quoted_printable_decode" => V::S(e::quoted_printable_decode(a)),
        "convert_uuencode" => V::S(e::convert_uuencode(a)),
        "convert_uudecode" => opt(e::convert_uudecode(a, &mut d)),
        _ => unreachable!(),
    };
    Some((Out::V(v), d))
}

#[test]
fn fixture_hex() {
    check("encoding", |c| run(c, "bin2hex"));
    check("encoding", |c| run(c, "hex2bin"));
}

#[test]
fn fixture_hashes() {
    check("encoding", |c| run(c, "crc32"));
    check("encoding", |c| run(c, "md5"));
    check("encoding", |c| run(c, "sha1"));
}

#[test]
fn fixture_str_rot13() {
    check("encoding", |c| run(c, "str_rot13"));
}

#[test]
fn fixture_quoted_printable() {
    check("encoding", |c| run(c, "quoted_printable_encode"));
    check("encoding", |c| run(c, "quoted_printable_decode"));
}

#[test]
fn fixture_uuencode() {
    check("encoding", |c| run(c, "convert_uuencode"));
    check("encoding", |c| run(c, "convert_uudecode"));
}
