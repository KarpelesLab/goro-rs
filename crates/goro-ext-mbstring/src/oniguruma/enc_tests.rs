//! Tests of the encoding layer. `diff_dump` (ignored by default) produces
//! the same output as a small C harness linked against libonig, for
//! differential testing: ONIG_DIFF_IN=<cmds> ONIG_DIFF_OUT=<file>.

use super::enc::*;
use std::fmt::Write as _;

fn enc_by_name(n: &str) -> &'static Encoding {
    Encoding::get(match n {
        "ASCII" => EncKind::Ascii,
        "UTF8" => EncKind::Utf8,
        "UTF16BE" => EncKind::Utf16Be,
        "UTF16LE" => EncKind::Utf16Le,
        "UTF32BE" => EncKind::Utf32Be,
        "UTF32LE" => EncKind::Utf32Le,
        "EUCJP" => EncKind::EucJp,
        "SJIS" => EncKind::Sjis,
        "BIG5" => EncKind::Big5,
        "EUCTW" => EncKind::EucTw,
        "EUCKR" => EncKind::EucKr,
        "EUCCN" => EncKind::EucCn,
        "KOI8" => EncKind::Koi8,
        "KOI8R" => EncKind::Koi8R,
        _ => EncKind::Iso8859(n[3..].parse().unwrap()),
    })
}

fn unhex(h: &str) -> Vec<u8> {
    (0..h.len() / 2)
        .map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}

fn items(o: &mut String, enc: &Encoding, flag: u32, s: &[u8], p: usize, end: usize) {
    let mut it = [CaseFoldCodeItem::default(); ONIGENC_GET_CASE_FOLD_CODES_MAX_NUM];
    let n = enc.get_case_fold_codes_by_str(flag, s, p, end, &mut it);
    write!(o, " I{n}").unwrap();
    for x in &it[..n as usize] {
        write!(o, " [{}", x.byte_len).unwrap();
        for c in &x.code[..x.code_len as usize] {
            write!(o, " {c:x}").unwrap();
        }
        o.push(']');
    }
}

fn run(cmds: &str) -> String {
    let mut o = String::new();
    for line in cmds.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let enc = enc_by_name(parts[0]);
        let arg = parts.get(2).copied().unwrap_or("");
        writeln!(o, "{} {} {}", parts[0], parts[1], arg).unwrap();
        match parts[1] {
            "ALLFOLD" => {
                for flag in [ONIGENC_CASE_FOLD_MIN, ONIGENC_CASE_FOLD_ASCII_ONLY] {
                    if flag == ONIGENC_CASE_FOLD_ASCII_ONLY {
                        o.push_str("ascii-only\n");
                    }
                    enc.apply_all_case_fold(flag, &mut |from, to| {
                        write!(o, "{from:x}:").unwrap();
                        for t in to {
                            write!(o, " {t:x}").unwrap();
                        }
                        o.push('\n');
                        0
                    });
                }
            }
            "CODES" => {
                let (lo, hi) = arg.split_once('-').unwrap();
                let (lo, hi) = (
                    u32::from_str_radix(lo, 16).unwrap(),
                    u32::from_str_radix(hi, 16).unwrap(),
                );
                for c in lo..=hi {
                    let l = enc.code_to_mbclen(c);
                    write!(o, "{c:x} l{l} ct").unwrap();
                    for t in 0..=14 {
                        if enc.is_code_ctype(c, t) {
                            write!(o, " {t}").unwrap();
                        }
                    }
                    if enc.is_unicode() {
                        for t in (15..629).step_by(7) {
                            if enc.is_code_ctype(c, t) {
                                write!(o, " {t}").unwrap();
                            }
                        }
                    }
                    if l > 0 {
                        let mut b = [0u8; ONIGENC_CODE_TO_MBC_MAXLEN];
                        let m = enc.code_to_mbc(c, &mut b);
                        write!(o, " m{m}").unwrap();
                        if m > 0 {
                            let b = &b[..m as usize];
                            for x in b {
                                write!(o, "{x:02x}").unwrap();
                            }
                            items(&mut o, enc, ONIGENC_CASE_FOLD_MIN, b, 0, b.len());
                            items(&mut o, enc, ONIGENC_CASE_FOLD_ASCII_ONLY, b, 0, b.len());
                            for (tag, flag) in [
                                ("F", ONIGENC_CASE_FOLD_MIN),
                                ("A", ONIGENC_CASE_FOLD_ASCII_ONLY),
                            ] {
                                let mut pp = 0;
                                let mut f = [0u8; ONIGENC_MBC_CASE_FOLD_MAXLEN];
                                let fl = enc.mbc_case_fold(flag, b, &mut pp, b.len(), &mut f);
                                o.push(' ');
                                o.push_str(tag);
                                for x in &f[..fl] {
                                    write!(o, "{x:02x}").unwrap();
                                }
                            }
                        }
                    }
                    o.push('\n');
                }
            }
            "PROP" => {
                writeln!(o, "{}", enc.property_name_to_ctype(&unhex(arg))).unwrap();
            }
            "RANGE" => {
                match enc.get_ctype_code_range(arg.parse().unwrap()) {
                    Ok((sb, r)) => {
                        write!(o, "0 sb{sb:x} n{}", r[0]).unwrap();
                        for x in r.iter().skip(1).take((r[0] as usize * 2).min(6)) {
                            write!(o, " {x:x}").unwrap();
                        }
                    }
                    Err(e) => write!(o, "{e}").unwrap(),
                }
                o.push('\n');
            }
            "STR" => {
                let s = unhex(arg);
                let len = s.len();
                writeln!(o, "valid {}", enc.is_valid_mbc_string(&s, 0, len) as i32).unwrap();
                o.push_str("adj");
                for p in 0..=len {
                    write!(o, " {}", enc.left_adjust_char_head(&s, 0, p)).unwrap();
                }
                o.push_str("\nrev");
                for p in 0..len {
                    write!(o, " {}", enc.is_allowed_reverse_match(&s, p, len) as i32).unwrap();
                }
                o.push('\n');
                let mut p = 0;
                while p < len {
                    let l = enc.mbc_enc_len(&s, p);
                    write!(
                        o,
                        "@{p} l{l} c{:x} nl{}",
                        enc.mbc_to_code(&s, p, len),
                        enc.is_mbc_newline(&s, p, len) as i32
                    )
                    .unwrap();
                    items(&mut o, enc, ONIGENC_CASE_FOLD_MIN, &s, p, len);
                    let mut pp = p;
                    let mut f = [0u8; ONIGENC_MBC_CASE_FOLD_MAXLEN];
                    let fl = enc.mbc_case_fold(ONIGENC_CASE_FOLD_MIN, &s, &mut pp, len, &mut f);
                    write!(o, " F{pp}:").unwrap();
                    for x in &f[..fl] {
                        write!(o, "{x:02x}").unwrap();
                    }
                    write!(
                        o,
                        " g{}",
                        egcb_is_break_position(enc, &s, p, None, 0, len) as i32
                    )
                    .unwrap();
                    if enc.is_unicode() {
                        write!(
                            o,
                            " w{}",
                            wb_is_break_position(enc, &s, p, None, 0, len) as i32
                        )
                        .unwrap();
                    }
                    o.push('\n');
                    p += l;
                }
            }
            _ => {}
        }
    }
    o
}

#[test]
#[ignore]
fn diff_dump() {
    let cmds = std::fs::read_to_string(std::env::var("ONIG_DIFF_IN").unwrap()).unwrap();
    std::fs::write(std::env::var("ONIG_DIFF_OUT").unwrap(), run(&cmds)).unwrap();
}

fn fold_items(enc: &Encoding, s: &[u8]) -> Vec<(i32, Vec<u32>)> {
    let mut it = [CaseFoldCodeItem::default(); ONIGENC_GET_CASE_FOLD_CODES_MAX_NUM];
    let n = enc.get_case_fold_codes_by_str(ONIGENC_CASE_FOLD_DEFAULT, s, 0, s.len(), &mut it);
    it[..n as usize]
        .iter()
        .map(|x| (x.byte_len, x.code[..x.code_len as usize].to_vec()))
        .collect()
}

fn fold(enc: &Encoding, s: &[u8]) -> Vec<u8> {
    let mut pp = 0;
    let mut f = [0u8; ONIGENC_MBC_CASE_FOLD_MAXLEN];
    let n = enc.mbc_case_fold(ONIGENC_CASE_FOLD_DEFAULT, s, &mut pp, s.len(), &mut f);
    f[..n].to_vec()
}

#[test]
fn unicode_decode_encode() {
    let cases: [(EncKind, &[u8], u32); 8] = [
        (EncKind::Utf8, "é".as_bytes(), 0xe9),
        (EncKind::Utf8, "😀".as_bytes(), 0x1f600),
        (EncKind::Utf16Be, &[0x00, 0xe9], 0xe9),
        (EncKind::Utf16Be, &[0xd8, 0x3d, 0xde, 0x00], 0x1f600),
        (EncKind::Utf16Le, &[0xe9, 0x00], 0xe9),
        (EncKind::Utf16Le, &[0x3d, 0xd8, 0x00, 0xde], 0x1f600),
        (EncKind::Utf32Be, &[0, 1, 0xf6, 0], 0x1f600),
        (EncKind::Utf32Le, &[0, 0xf6, 1, 0], 0x1f600),
    ];
    for (k, bytes, code) in cases {
        let enc = Encoding::get(k);
        assert_eq!(enc.mbc_enc_len(bytes, 0), bytes.len(), "{k:?}");
        assert_eq!(enc.mbc_to_code(bytes, 0, bytes.len()), code, "{k:?}");
        let mut b = [0u8; ONIGENC_CODE_TO_MBC_MAXLEN];
        let n = enc.code_to_mbc(code, &mut b);
        assert_eq!(&b[..n as usize], bytes, "{k:?}");
        assert!(enc.is_valid_mbc_string(bytes, 0, bytes.len()));
    }
    let u8e = Encoding::get(EncKind::Utf8);
    assert!(!u8e.is_valid_mbc_string(b"\x80", 0, 1));
    assert_eq!(u8e.code_to_mbclen(0x200000), -400);
}

#[test]
fn cjk_lengths() {
    let eucjp = Encoding::get(EncKind::EucJp);
    assert_eq!(eucjp.mbc_enc_len(b"\xa4\xa2", 0), 2);
    assert_eq!(eucjp.mbc_enc_len(b"\x8f\xa1\xa1", 0), 3);
    assert_eq!(eucjp.mbc_to_code(b"\xa4\xa2", 0, 2), 0xa4a2);
    let sjis = Encoding::get(EncKind::Sjis);
    assert_eq!(sjis.mbc_enc_len(b"\x82\xa0", 0), 2);
    assert_eq!(sjis.mbc_enc_len(b"\xb1", 0), 1);
    // "a\x82\xa0" : the trail byte 0xa0 adjusts back to the lead byte
    assert_eq!(sjis.left_adjust_char_head(b"a\x82\xa0", 0, 2), 1);
}

#[test]
fn case_folds() {
    let enc = Encoding::get(EncKind::Utf8);
    assert_eq!(fold(enc, "ß".as_bytes()), b"ss");
    assert_eq!(fold(enc, "ﬀ".as_bytes()), b"ff");
    assert_eq!(fold(enc, "İ".as_bytes()), "i\u{307}".as_bytes());
    assert_eq!(fold(enc, "ı".as_bytes()), "ı".as_bytes());
    assert_eq!(fold(enc, "\u{212a}".as_bytes()), b"k");
    assert_eq!(fold(enc, "ſ".as_bytes()), b"s");
    assert_eq!(fold(enc, b"S"), b"s");

    assert_eq!(
        fold_items(enc, "ß".as_bytes())[..3],
        [
            (2, vec![0x1e9e]),
            (2, vec![0x73, 0x73]),
            (2, vec![0x73, 0x53])
        ]
    );
    let ss = fold_items(enc, b"ss");
    assert_eq!(ss.len(), 10);
    assert_eq!(ss[0], (2, vec![0xdf]));
    assert_eq!(ss[1], (2, vec![0x1e9e]));
    let ff = fold_items(enc, "ﬀ".as_bytes());
    assert_eq!(ff[0], (3, vec![0x66, 0x66]));
    assert_eq!(ff[1], (3, vec![0x66, 0x46]));
    let fi = fold_items(enc, b"fi");
    assert_eq!(fi[0], (2, vec![0xfb01]));
    assert_eq!(fold_items(enc, b"k"), [(1, vec![0x4b]), (1, vec![0x212a])]);

    let mut n = 0;
    enc.apply_all_case_fold(ONIGENC_CASE_FOLD_DEFAULT, &mut |_, _| {
        n += 1;
        0
    });
    assert!(n > 2800, "{n}");
}

#[test]
fn property_names() {
    let enc = Encoding::get(EncKind::Utf8);
    assert_eq!(
        enc.property_name_to_ctype(b"Alpha"),
        ONIGENC_CTYPE_ALPHA as i32
    );
    assert_eq!(
        enc.property_name_to_ctype(b"alpha"),
        ONIGENC_CTYPE_ALPHA as i32
    );
    assert!(enc.property_name_to_ctype(b"Hiragana") > 14);
    assert!(enc.property_name_to_ctype(b"Greek") > 14);
    assert!(enc.property_name_to_ctype(b"In_Basic_Latin") > 14);
    assert!(enc.property_name_to_ctype(b"in basic-latin") > 14);
    assert!(enc.property_name_to_ctype(b"Emoji") > 14);
    assert!(enc.property_name_to_ctype(b"L") > 14);
    assert!(enc.property_name_to_ctype(b"Lu") > 14);
    assert_eq!(enc.property_name_to_ctype(b"^Lu"), -223);
    assert_eq!(enc.property_name_to_ctype(b"NoSuch"), -223);
    let hira = enc.property_name_to_ctype(b"Hiragana") as u32;
    assert!(enc.is_code_ctype(0x3042, hira));
    assert!(!enc.is_code_ctype(0x30a2, hira));
    let jp = Encoding::get(EncKind::EucJp);
    assert_eq!(jp.property_name_to_ctype(b"Hiragana"), 15);
    assert_eq!(jp.property_name_to_ctype(b"hiragana"), -223);
    assert_eq!(
        Encoding::get(EncKind::Ascii).property_name_to_ctype(b"Alpha"),
        1
    );
}

#[test]
fn grapheme_breaks() {
    let enc = Encoding::get(EncKind::Utf8);
    let brk = |s: &str, p: usize| egcb_is_break_position(enc, s.as_bytes(), p, None, 0, s.len());
    assert!(!brk("\r\n", 1));
    assert!(brk("a\n", 1));
    assert!(!brk("e\u{301}", 1));
    // 👨‍👩 : man ZWJ woman
    let fam = "\u{1f468}\u{200d}\u{1f469}";
    assert!(!brk(fam, 4));
    assert!(!brk(fam, 7));
    // Hangul L + V
    assert!(!brk("\u{1100}\u{1161}", 3));
    assert!(brk("\u{ac00}\u{ac00}", 3));
    // regional indicators pair up
    let ri = "\u{1f1ef}\u{1f1f5}\u{1f1fa}";
    assert!(!brk(ri, 4));
    assert!(brk(ri, 8));
    let wb = |s: &str, p: usize| wb_is_break_position(enc, s.as_bytes(), p, None, 0, s.len());
    assert!(!wb("ab", 1));
    assert!(wb("a b", 1));
    assert!(!wb("can't", 3));
    assert!(!wb("3.14", 1));
}
