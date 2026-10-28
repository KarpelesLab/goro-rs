//! mb_convert_encoding() from a list of encodings (expected values observed
//! on PHP 8.5).

use goro_charset::ops::detect::convert_from_list;
use goro_charset::{Encoding, Subst};

fn conv(s: &[u8], to: &str, from: &[&str]) -> Result<Vec<u8>, String> {
    let to = Encoding::from_name(to.as_bytes()).unwrap();
    let from: Vec<Encoding> = from
        .iter()
        .map(|n| Encoding::from_name(n.as_bytes()).unwrap())
        .collect();
    convert_from_list(s, to, &from, &Subst::default())
        .map(|c| c.bytes)
        .map_err(|e| e.message("mb_convert_encoding"))
}

#[test]
fn from_list() {
    assert_eq!(
        conv(b"\xff", "UTF-8", &["ASCII", "UTF-8"]),
        Ok(b"?".to_vec())
    );
    assert_eq!(
        conv(b"h\xe9llo", "UTF-8", &["UTF-8", "ISO-8859-1"]),
        Ok(b"h\xc3\xa9llo".to_vec())
    );
    assert_eq!(
        conv(b"h\xe9llo", "UTF-16BE", &["UTF-8", "ASCII"]),
        Ok(b"\0h\0?\0l\0l\0o".to_vec())
    );
    assert_eq!(
        conv(b"\x82\xa0", "UTF-8", &["UTF-8", "SJIS"]),
        Ok(b"\xe3\x81\x82".to_vec())
    );
    assert_eq!(
        conv(b"\x82\xa0", "UTF-8", &["UTF-8", "BASE64"]),
        Ok(b"??".to_vec())
    );
    assert_eq!(conv(b"abc", "UTF-8", &["BASE64"]), Ok(b"\x69\xb7".to_vec()));
    assert_eq!(conv(b"", "UTF-8", &["UTF-8", "SJIS"]), Ok(b"".to_vec()));
    assert_eq!(
        conv(b"\xd6\x8a\x8a", "UTF-16BE", &["ASCII", "UTF-8"]),
        Ok(b"\x05\x8a\x00\x3f".to_vec())
    );
    assert_eq!(
        conv(b"abc", "UTF-8", &["BASE64", "UUENCODE"]),
        Err("mb_convert_encoding(): Argument #3 ($from_encoding) must specify at least one encoding".into())
    );
}
