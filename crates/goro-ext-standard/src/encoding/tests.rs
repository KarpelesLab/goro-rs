//! Cases ported from the expected output of php-src's .phpt tests (named
//! per test); the broad comparison with PHP 8.5 is tests/encoding_fixture.rs.

use super::*;

#[test]
fn hex() {
    // test bin2hex.phpt
    let all: Vec<u8> = (0..=255).collect();
    let h = bin2hex(&all);
    assert_eq!(&h[..8], b"00010203");
    assert_eq!(&h[h.len() - 4..], b"feff");
    assert_eq!(bin2hex(b"abc"), b"616263");
    let mut d = Vec::new();
    assert_eq!(hex2bin(&h, &mut d).unwrap(), all);
    assert_eq!(hex2bin(b"6A6b", &mut d).unwrap(), b"jk");
    assert!(d.is_empty());
    // test hex2bin_error.phpt
    assert_eq!(hex2bin(b"AH", &mut d), None);
    assert_eq!(hex2bin(b"HA", &mut d), None);
    assert_eq!(
        d,
        vec![Diag::Warning("Input string must be hexadecimal string".into()); 2]
    );
    d.clear();
    // test hex2bin_basic.phpt
    assert_eq!(hex2bin(b"abc", &mut d), None);
    assert_eq!(
        d,
        [Diag::Warning(
            "Hexadecimal input string must have an even length".into()
        )]
    );
}

#[test]
fn crc32_phpt() {
    // test crc32.phpt
    for (s, v) in [
        ("foo", 2356372769u32),
        ("bar", 1996459178),
        ("baz", 2015626392),
        ("grldsajkopallkjasd", 824412087),
        ("1234567890123456", 509595063),
        ("1234567890123456abc", 1890929711),
        ("12345678901234561234567890123456", 156217630),
        ("12345678901234561234567890123456abc", 941689929),
    ] {
        assert_eq!(crc32(s.as_bytes()), v, "{s}");
    }
    assert_eq!(crc32_update(crc32(b"foo"), b"bar"), crc32(b"foobar"));
}

#[test]
fn digests() {
    // test md5.phpt (the RFC 1321 test suite)
    for (s, h) in [
        ("", "d41d8cd98f00b204e9800998ecf8427e"),
        ("a", "0cc175b9c0f1b6a831c399e269772661"),
        ("abc", "900150983cd24fb0d6963f7d28e17f72"),
        ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
        (
            "abcdefghijklmnopqrstuvwxyz",
            "c3fcd3d76192e4007dfb496cca67e13b",
        ),
        (
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
            "d174ab98d277d9f5a5611c2c9f419d9f",
        ),
    ] {
        assert_eq!(md5(s.as_bytes(), false), h.as_bytes());
        assert_eq!(bin2hex(&md5(s.as_bytes(), true)), h.as_bytes());
    }
    // test sha1.phpt
    for (s, h) in [
        ("abc", "a9993e364706816aba3e25717850c26c9cd0d89d"),
        (
            "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1",
        ),
        ("a", "86f7e437faa5a7fce15d1ddcb9eaeaea377667b8"),
        ("", "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
    ] {
        assert_eq!(sha1(s.as_bytes(), false), h.as_bytes());
        assert_eq!(sha1(s.as_bytes(), true).len(), 20);
    }
}

#[test]
fn rot13() {
    // test str_rot13_basic.phpt
    assert_eq!(
        str_rot13(b"str_rot13() tests starting"),
        b"fge_ebg13() grfgf fgnegvat"
    );
    assert_eq!(
        str_rot13(b"abcdefghijklmnopqrstuvwxyz"),
        b"nopqrstuvwxyzabcdefghijklm"
    );
    assert_eq!(str_rot13(b"0123456789!%^&*()"), b"0123456789!%^&*()");
}

#[test]
fn quoted_printable() {
    // test quoted_printable_decode_basic.phpt
    let s = b"=FAwow-factor=C1=d0=D5=DD=C5=CE=CE=D9=C5=0A=\n=20=D4=cf=D2=C7=CF=D7=D9=C5=\n=20=\n=D0=\n=D2=CF=C5=CB=D4=D9";
    assert_eq!(
        bin2hex(&quoted_printable_decode(s)),
        b"fa776f772d666163746f72c1d0d5ddc5ceced9c50a20d4cfd2c7cfd7d9c520d0d2cfc5cbd4d9"
    );
    // test quoted_printable_encode_002.phpt
    let z = quoted_printable_encode(&[0u8; 200]);
    assert!(z.starts_with(b"=00=00"));
    assert_eq!(quoted_printable_decode(&z), vec![0u8; 200]);
    let line = z.split(|&c| c == b'\n').next().unwrap();
    assert_eq!(line, [&b"=00".repeat(25)[..], b"=\r"].concat());
    let cyr = "строка в юникоде".repeat(50);
    assert_eq!(
        quoted_printable_decode(&quoted_printable_encode(cyr.as_bytes())),
        cyr.as_bytes()
    );
    // observed on PHP 8.5: tabs, CR and a space before CR are escaped.
    assert_eq!(
        quoted_printable_encode(b"a\tb \rc \r\nd "),
        b"a=09b=20=0Dc=20\r\nd "
    );
}

#[test]
fn uuencode_phpt() {
    // test uuencode.phpt
    let mut d = Vec::new();
    assert_eq!(convert_uuencode(b""), b"`\n");
    assert_eq!(convert_uudecode(b"`\n", &mut d).unwrap(), b"");
    let s = b"~!@#$%^&*()_}{POIUYTREWQQSDFGHJKL:<MNBVCXZ";
    let e = convert_uuencode(s);
    assert_eq!(
        e,
        &b"J?B%`(R0E7B8J*\"E??7M03TE5651215=145-$1D=(2DM,.CQ-3D)60UA:\n`\n"[..]
    );
    assert_eq!(convert_uudecode(&e, &mut d).unwrap(), s);
    assert_eq!(
        convert_uudecode(b"!@#$%^YUGFDFGHJKLUYTFBNMLOYT", &mut d)
            .unwrap()
            .len(),
        1
    );
    let e = convert_uuencode(b"not very sophisticated");
    assert_eq!(e, b"6;F]T('9E<GD@<V]P:&ES=&EC871E9```\n`\n");
    assert_eq!(
        convert_uudecode(&e, &mut d).unwrap(),
        b"not very sophisticated"
    );
    assert!(d.is_empty());
    assert_eq!(convert_uudecode(b"", &mut d), None);
    assert_eq!(convert_uudecode(&e[..e.len() - 10], &mut d), None);
    assert_eq!(
        d,
        vec![Diag::Warning("Argument #1 ($data) is not a valid uuencoded string".into()); 2]
    );
    // test convert_uudecode_basic.phpt: round trips
    for s in [
        &b"123"[..],
        b"Here is a simple string to test convert_uuencode/decode",
        b"\x90\x91\x00\x93\x94\x90\x91\x95",
    ] {
        assert_eq!(convert_uudecode(&convert_uuencode(s), &mut d).unwrap(), s);
    }
    let long: Vec<u8> = (0..200u32).map(|i| (i * 7) as u8).collect();
    assert_eq!(
        convert_uudecode(&convert_uuencode(&long), &mut d).unwrap(),
        long
    );
}
