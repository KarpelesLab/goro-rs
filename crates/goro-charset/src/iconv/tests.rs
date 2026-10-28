//! Tests: expected values come from the ext/iconv .phpt files and from
//! runs of PHP 8.5 (glibc 2.43 iconv) recorded as fixtures under
//! crates/goro-charset/tests/fixtures/.

use super::*;

/// `iconv()` result as the fixture drivers print it: hex, "e" for an empty
/// string, "x" / "i" for the illegal / incomplete notices, "W" for a warning.
fn k(from: &str, to: &str, s: &[u8]) -> String {
    let mut d = Vec::new();
    match iconv(from.as_bytes(), to.as_bytes(), s, &mut d) {
        Some(v) if v.is_empty() => "e".into(),
        Some(v) => hex(&v),
        None => match d.first() {
            Some(Diag::Notice(m)) if m.contains("incomplete") => "i".into(),
            Some(Diag::Notice(m)) if m.contains("illegal") => "x".into(),
            _ => "W".into(),
        },
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut c = !0u32;
    for &b in data {
        c = table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

fn block(lo: u32, hi: u32) -> Vec<u8> {
    let mut s = Vec::new();
    for cp in lo..hi {
        if !(0xD800..0xE000).contains(&cp) {
            s.extend_from_slice(&cp.to_be_bytes());
        }
    }
    s
}

const TABLES: &str = include_str!("../../tests/fixtures/iconv_tables.txt");

/// Fixture checks that differ from PHP 8.5 on purpose (glibc quirks not
/// reproduced, see the module documentation):
/// - CP949: A2 E8 followed by a byte, with //IGNORE, is reported as an
///   incomplete character;
/// - ISO-2022-JP//TRANSLIT of U+3395 writes JIS X 0208 bytes without their
///   escape sequence.
const KNOWN_DIFFERENCES: &[&str] = &["DECI CP949", "ISO-2022-JP@3000"];

fn fixture_lines(kind: &str) -> Vec<Vec<&'static str>> {
    TABLES
        .lines()
        .map(|l| l.split(' ').collect::<Vec<_>>())
        .filter(|p| p[0] == kind)
        .collect()
}

fn check_decode(cs: &str) -> (u32, u32) {
    let mut dec = Vec::new();
    let mut deci = Vec::new();
    let mut queue: Vec<Vec<u8>> = vec![Vec::new()];
    let mut q = 0;
    let mut l1 = 0;
    while q < queue.len() {
        let pre = queue[q].clone();
        if q == 1 {
            l1 = queue.len() - 1;
        }
        q += 1;
        for b in 0..=255u8 {
            let mut s = pre.clone();
            s.push(b);
            let r = k(cs, "UTF-32BE", &s);
            let mut si = s.clone();
            si.push(b'A');
            deci.push(k(cs, "UTF-32BE//IGNORE", &si));
            if r == "i" && (s.len() == 1 || (s.len() == 2 && l1 <= 128)) {
                queue.push(s);
            }
            dec.push(r);
        }
    }
    (
        crc32(dec.join(",").as_bytes()),
        crc32(deci.join(",").as_bytes()),
    )
}

/// Every charset decodes every short byte sequence as PHP does.
#[test]
fn fixture_decode() {
    let dec = fixture_lines("DEC");
    let deci = fixture_lines("DECI");
    let mut bad = Vec::new();
    for (d, di) in dec.iter().zip(deci.iter()) {
        let cs = d[1];
        let (a, b) = check_decode(cs);
        if format!("{a:08x}") != d[2] {
            bad.push(format!("DEC {cs}"));
        }
        if format!("{b:08x}") != di[2] {
            bad.push(format!("DECI {cs}"));
        }
    }
    bad.retain(|b| !KNOWN_DIFFERENCES.contains(&b.as_str()));
    assert!(bad.is_empty(), "{} mismatches: {bad:?}", bad.len());
}

/// Every charset encodes every code point as PHP does.
#[test]
fn fixture_encode() {
    let blocks: Vec<Vec<u8>> = (0..16).map(|b| block(b << 12, (b + 1) << 12)).collect();
    let planes: Vec<Vec<u8>> = [1u32, 2, 14, 15]
        .iter()
        .flat_map(|&p| {
            (0..16u32).map(move |b| block((p << 16) + (b << 12), (p << 16) + ((b + 1) << 12)))
        })
        .collect();
    let mut bad = Vec::new();
    for l in fixture_lines("ENC") {
        let cs = l[1];
        let to = format!("{cs}//IGNORE");
        let enc: Vec<String> = blocks.iter().map(|b| k("UTF-32BE", &to, b)).collect();
        if format!("{:08x}", crc32(enc.join(",").as_bytes())) != l[2] {
            bad.push(format!("ENC {cs}"));
        }
    }
    for l in fixture_lines("ENCS") {
        let cs = l[1];
        let to = format!("{cs}//IGNORE");
        let enc: Vec<String> = planes.iter().map(|b| k("UTF-32BE", &to, b)).collect();
        if format!("{:08x}", crc32(enc.join(",").as_bytes())) != l[2] {
            bad.push(format!("ENCS {cs}"));
        }
    }
    bad.retain(|b| !KNOWN_DIFFERENCES.contains(&b.as_str()));
    assert!(bad.is_empty(), "{} mismatches: {bad:?}", bad.len());
}

/// //TRANSLIT for a range of targets.
#[test]
fn fixture_translit() {
    let mut bad = Vec::new();
    for l in fixture_lines("TR") {
        let cs = l[1];
        let lo = u32::from_str_radix(l[2], 16).unwrap();
        let hi = lo + 0x1000;
        let r = k(
            "UTF-32BE",
            &format!("{cs}//TRANSLIT//IGNORE"),
            &block(lo.max(0x80), hi),
        );
        if format!("{:08x}", crc32(r.as_bytes())) != l[3] {
            bad.push(format!("{cs}@{lo:X}"));
        }
    }
    bad.retain(|b| !KNOWN_DIFFERENCES.contains(&b.as_str()));
    assert!(bad.is_empty(), "{} mismatches: {bad:?}", bad.len());
}

#[test]
fn basics() {
    // test ext/iconv/tests/iconv001.phpt
    assert_eq!(
        k("ISO-8859-1", "UTF-8", b"\xe6\xf8\xe5"),
        hex("æøå".as_bytes())
    );
    assert_eq!(k("UTF-8", "UTF-16", b"ab"), "fffe61006200");
    assert_eq!(unhex("6162"), b"ab");
}

fn diags_of(f: impl FnOnce(&mut Vec<Diag>)) -> Vec<Diag> {
    let mut d = Vec::new();
    f(&mut d);
    d
}

fn notice(m: &str) -> Diag {
    Diag::Notice(m.into())
}

fn warning(m: &str) -> Diag {
    Diag::Warning(m.into())
}

const ILLEGAL: &str = "Detected an illegal character in input string";
const INCOMPLETE: &str = "Detected an incomplete multibyte character in input string";

/// Tests from ext/iconv/tests: iconv().
#[test]
fn phpt_iconv() {
    // bug48147.phpt
    let mut d = Vec::new();
    assert_eq!(
        iconv(b"UTF-8", b"UTF-8", b"aa\xC3\xC3\xC3\xB8aa", &mut d),
        None
    );
    assert_eq!(d, [notice(ILLEGAL)]);
    let mut d = Vec::new();
    assert_eq!(
        iconv(b"UTF-8", b"UTF-8//IGNORE", b"aa\xC3\xC3\xC3\xB8aa", &mut d).unwrap(),
        b"aa\xc3\xb8aa"
    );
    assert!(d.is_empty());
    let mut d = Vec::new();
    assert_eq!(iconv(b"UTF-8", b"UTF-8//IGNORE", b"\xC3", &mut d), None);
    assert_eq!(d, [notice(INCOMPLETE)]);
    assert_eq!(
        k("UTF-8", "UTF-8//IGNORE", b"\xC3\xC3\xC3\xB8aa"),
        "c3b86161"
    );
    assert_eq!(k("UTF-8", "UTF-8//IGNORE", b"aa\xC3\xC3\xC3"), "i");
    // bug52211.phpt: 0xC9 is not in CP850's... it is; Ã‰ decodes but has no
    // ISO-8859-1 form.
    assert_eq!(
        k(
            "CP850",
            "ISO-8859-1",
            b"PATHOLOGIES M\xc3\x89DICO-CHIRUR. ADUL. PL"
        ),
        "x"
    );
    // bug74230.phpt: surrogates are illegal in UTF-8.
    for s in [
        &b"\xED\xa1\x92"[..],
        b"\xED\xBD\xA2",
        b"\xED\xa1\x92\xED\xBD\xA2",
    ] {
        assert_eq!(k("UTF-8", "UTF-8", s), "x");
    }
    // iconv002.phpt
    assert_eq!(
        k("ascii", "UCS-4LE", b"abcd"),
        "61000000620000006300000064000000"
    );
    assert_eq!(
        k(
            "UCS-4BE",
            "ascii",
            &unhex("00000061000000620000006300000064")
        ),
        "61626364"
    );
    // iconv003.phpt
    assert_eq!(k("blah", "blah", b"blah"), "W");
    let d = diags_of(|d| {
        iconv(b"blah", b"blah", b"blah", d);
    });
    assert_eq!(
        d,
        [warning(
            "Wrong encoding, conversion from \"blah\" to \"blah\" is not allowed"
        )]
    );
    // iconv-charset-length-cve-2007-4840.phpt
    let long = vec![b'/'; 9000];
    let d = diags_of(|d| assert_eq!(iconv(&long, b"b", b"test", d), None));
    assert_eq!(
        d,
        [warning(
            "Encoding parameter exceeds the maximum allowed length of 64 characters"
        )]
    );
    // iconv_basic.phpt
    let sjis = unhex("93fa967b8cea8365834c8358836782c582b781423031323334825482558256825782588142");
    let euc = unhex("c6fccbdcb8eca5c6a5ada5b9a5c8a4c7a4b9a1a33031323334a3b5a3b6a3b7a3b8a3b9a1a3");
    let utf8 = unhex(
        "e697a5e69cace8aa9ee38386e382ade382b9e38388e381a7e38199e380823031323334efbc95efbc96efbc97efbc98efbc99e38082",
    );
    assert_eq!(k("SJIS", "EUC-JP", &sjis), hex(&euc));
    assert_eq!(k("UTF-8", "EUC-JP", &utf8), hex(&euc));
    assert_eq!(k("EUC-JP", "SJIS", &euc), hex(&sjis));
    assert_eq!(k("UTF-8", "SJIS", &utf8), hex(&sjis));
    assert_eq!(k("SJIS", "UTF-8", &sjis), hex(&utf8));
    assert_eq!(k("EUC-JP", "UTF-8", &euc), hex(&utf8));
    // eucjp2sjis.phpt, eucjp2utf8.phpt, eucjp2iso2022jp.phpt
    let line_euc = unhex("c6fccbdcb8eca5c6a5ada5b9a5c8a4c8456e676c69736820546578740a");
    let line_sjis = unhex("93fa967b8cea8365834c8358836782c6456e676c69736820546578740a");
    let line_utf8 = "日本語テキストとEnglish Text\n".as_bytes();
    let mut euc_text = b"\n".to_vec();
    let mut sjis_text = b"\n".to_vec();
    let mut utf8_text = b"\n".to_vec();
    for _ in 0..36 {
        euc_text.extend_from_slice(&line_euc);
        sjis_text.extend_from_slice(&line_sjis);
        utf8_text.extend_from_slice(line_utf8);
    }
    assert_eq!(k("EUC-JP", "SJIS", &euc_text), hex(&sjis_text));
    assert_eq!(k("EUC-JP", "UTF-8", &euc_text), hex(&utf8_text));
    let unit = unhex("c6fccbdcb8eca5c6a5ada5b9a5c8a4c820456e676c6973682074657874");
    let mut src = unit.repeat(30);
    src.extend_from_slice(&unhex("c6fccbdcb8ec"));
    let unit_jp = "1b2442467c4b5c386c2546252d2539254824481b284220456e676c69736820746578741b";
    let mut want = String::new();
    for _ in 0..30 {
        want.push_str(&unit_jp[..unit_jp.len() - 2]);
    }
    want.push_str("1b2442467c4b5c386c1b2842");
    assert_eq!(k("EUC-JP", "ISO-2022-JP", &src), want);
    // translit-utf8.phpt's input (the expectations are libiconv's; these are
    // glibc's, observed on PHP 8.5).
    let quotes = unhex(
        "e2809c48656c6c6fe2809d0ae280984865 6c6c6fe280990ae2809e48656c6c6fe2809d0ae2809a48656c6c6fe280990a"
            .replace(' ', "")
            .as_str(),
    );
    assert_eq!(
        String::from_utf8(unhex(&k("UTF-8", "ASCII//TRANSLIT", &quotes))).unwrap(),
        "\"Hello\"\n'Hello'\n,,Hello\"\n,Hello'\n"
    );
    // translit-failure.phpt's input (glibc result).
    assert_eq!(
        k("ISO-8859-1", "ASCII//TRANSLIT", b"\xc9crit par %s."),
        hex(b"Ecrit par %s.")
    );
    // iconv_basic_001.phpt
    assert_eq!(
        k("UTF-8", "ASCII//TRANSLIT", "Žluťoučký kůň\\n".as_bytes()),
        hex(b"Zlutoucky kun\\n")
    );
}

/// Observed on PHP 8.5 (glibc 2.43 iconv): error handling and suffixes.
#[test]
fn observed_iconv_flags() {
    // Unencodable characters: illegal; //IGNORE drops them; //TRANSLIT.
    assert_eq!(k("UTF-8", "ISO-8859-1", b"a\xE2\x82\xACb"), "x");
    assert_eq!(k("UTF-8", "ISO-8859-1//IGNORE", b"a\xE2\x82\xACb"), "6162");
    assert_eq!(
        k("UTF-8", "ISO-8859-1//TRANSLIT", b"a\xE2\x82\xACb"),
        "6145555262"
    );
    assert_eq!(
        k(
            "UTF-8",
            "ASCII//TRANSLIT",
            b"a\xE2\x82\xACb\xC3\xA9\xC5\xBD\xE4\xB8\x80z"
        ),
        "6145555262655a3f7a"
    );
    assert_eq!(
        k("UTF-8", "ASCII//TRANSLIT//IGNORE", b"a\xE4\xB8\x80z"),
        "613f7a"
    );
    assert_eq!(
        k("UTF-8", "ASCII//IGNORE//TRANSLIT", b"a\xE4\xB8\x80z"),
        "613f7a"
    );
    // Suffix parsing.
    let s = b"a\xc3\xa9\xe4\xb8\x80b";
    assert_eq!(k("UTF-8", "ASCII//IGNORE", s), "6162");
    assert_eq!(k("UTF-8", "ASCII//ignore", s), "x");
    assert_eq!(k("UTF-8", "ASCII//IGNORE,", s), "x");
    assert_eq!(k("UTF-8", "ASCII//IGNORE/", s), "x");
    assert_eq!(k("UTF-8", "ASCII//X//IGNORE", s), "6162");
    assert_eq!(k("UTF-8", "ASCII//IGNORE//X", s), "x");
    assert_eq!(k("UTF-8", "ASCII///IGNORE", s), "6162");
    assert_eq!(k("UTF-8", "ASCII//translit", s), "61653f62");
    assert_eq!(k("UTF-8", "ASCII//TRANSLIT,IGNORE", s), "61653f62");
    assert_eq!(k("UTF-8", "ASCII//TRANSLIT,IGNORE", b"a\xffb"), "x");
    assert_eq!(k("UTF-8", "ASCII // TRANSLIT", s), "x");
    assert_eq!(k("UTF-8//IGNORE", "ASCII", b"a\xffb"), "x");
    // Names: case, junk characters, a trailing '/', NUL, the empty name.
    for n in [
        "utf8",
        "Utf-8",
        "UTF 8",
        " UTF-8",
        "(UTF-8)",
        "UTF-8/",
        "UTF-8\0junk",
        "",
        "//",
    ] {
        assert_eq!(k(n, "UTF-16BE", b"a"), "0061", "{n:?}");
    }
    for n in ["UTF_8", "UTF.8", "UTF-8/X", "/UTF-8", "UTF--8"] {
        assert_eq!(k(n, "UTF-16BE", b"a"), "W", "{n:?}");
    }
    assert_eq!(k("NF_Z_62-010_(1973)", "UTF-16BE", b"a"), "0061");
    assert_eq!(k("ISO-10646/UCS2/", "UTF-8", b"a\0"), "61");
    // Charset length limit: 63 is fine, 64 is too long.
    let n63 = format!("UTF-8{}", "/".repeat(58));
    assert_eq!(k(&n63, "ASCII", b"a"), "61");
    assert_eq!(k(&format!("{n63}/"), "ASCII", b"a"), "W");
    // Tag characters vanish.
    assert_eq!(
        k(
            "UTF-32BE",
            "ASCII",
            &unhex("00000061000e00010000006200")[..12]
        ),
        "6162"
    );
}

/// Observed on PHP 8.5 (glibc 2.43 iconv): the Unicode forms.
#[test]
fn observed_unicode() {
    // UTF-8 decoding.
    for (h, want) in [
        ("c080", "x"),
        ("e08080", "x"),
        ("eda080", "x"),
        ("f4908080", "x"),
        ("ff", "x"),
        ("c2", "i"),
        ("e282", "i"),
        ("e080", "i"),
        ("f5", "i"),
        ("fc8080", "i"),
        ("e28241", "x"),
        ("e0a080", "0800"),
        ("f0908080", "d800dc00"),
        ("efbfbe", "fffe"),
    ] {
        assert_eq!(k("UTF-8", "UTF-16BE", &unhex(h)), want, "{h}");
    }
    assert_eq!(k("UTF-8", "UCS-4BE", &unhex("fdbfbfbfbfbf")), "7fffffff");
    assert_eq!(k("UTF-8", "UCS-4BE", &unhex("f4908080")), "00110000");
    assert_eq!(k("UCS-4BE", "UTF-8", &unhex("7fffffff")), "fdbfbfbfbfbf");
    assert_eq!(k("UCS-4BE", "UTF-8", &unhex("80000000")), "x");
    assert_eq!(k("UTF-32BE", "UTF-8", &unhex("00110000")), "x");
    assert_eq!(k("UCS-4BE", "UCS-4LE", &unhex("0000d800")), "00d80000");
    assert_eq!(k("UCS-4BE", "UTF-8", &unhex("0000d800")), "x");
    // BOMs.
    for (cs, h, want) in [
        ("UTF-16", "feff0061", "61"),
        ("UTF-16", "fffe6100", "61"),
        ("UTF-16", "6100", "61"),
        ("UTF-16", "0061", "e68480"),
        ("UTF-16", "fffefffe6100", "efbbbf61"),
        ("UTF-16LE", "fffe6100", "efbbbf61"),
        ("UTF-16BE", "feff0061", "efbbbf61"),
        ("UCS-2", "fffe6100", "efbbbf61"),
        ("UNICODE", "feff0061", "61"),
        ("UTF-32", "0000feff00000061", "61"),
        ("UTF-32", "61000000", "61"),
        ("UTF-32", "00000061", "x"),
        ("UCS-4", "00000061", "61"),
        ("UCS-4", "0000feff00000061", "efbbbf61"),
        ("WCHAR_T", "61000000", "61"),
        ("UTF-16LE", "00d8", "i"),
        ("UTF-16LE", "00d84100", "x"),
        ("UTF-16LE", "00dc", "x"),
        ("UTF-16LE", "3dd800de", "f09f9880"),
        ("UCS-2", "3dd8", "x"),
        ("UTF-16", "610062", "i"),
    ] {
        assert_eq!(k(cs, "UTF-8", &unhex(h)), want, "{cs} {h}");
    }
    for (cs, want) in [
        ("UTF-16", "fffe61003dd800defffe"),
        ("UTF-16LE", "61003dd800defffe"),
        ("UTF-16BE", "0061d83dde00feff"),
        ("UTF-32", "fffe00006100000000f60100fffe0000"),
        ("UCS-2", "x"),
        ("UCS-4", "000000610001f6000000feff"),
        ("UNICODE", "x"),
        ("WCHAR_T", "6100000000f60100fffe0000"),
        ("UTF-7", "612b324433654150372f2d"),
        ("UTF-7-IMAP", "6126324433654150372c2d"),
    ] {
        assert_eq!(
            k("UTF-8", cs, b"a\xf0\x9f\x98\x80\xef\xbb\xbf"),
            want,
            "{cs}"
        );
    }
    assert_eq!(k("UTF-8", "UTF-16", b""), "e");
    assert_eq!(k("UTF-8", "UTF-16//IGNORE", b"\xff"), "e");
    assert_eq!(k("UCS-4BE", "UNICODE//IGNORE", &unhex("00010000")), "fffe");
}

/// Observed on PHP 8.5 (glibc 2.43 iconv): UTF-7 and UTF-7-IMAP.
#[test]
fn observed_utf7() {
    for (s, want, imap) in [
        ("Hi Mom -☺-!", "Hi Mom -+Jjo--+ACE-", "Hi Mom -&Jjo--!"),
        ("A≢Α.", "A+ImIDkQ.", "A&ImIDkQ-."),
        ("日本語", "+ZeVnLIqe-", "&ZeVnLIqe-"),
        ("~\\+-&a&b", "+AH4AXAAr--+ACY-a+ACY-b", "~\\+-&-a&-b"),
        ("a+b", "a+-b", "a+b"),
        ("😀x", "+2D3eAA-x", "&2D3eAA-x"),
        ("aé", "a+AOk-", "a&AOk-"),
        ("\u{feff}a", "+/v8-a", "&,v8-a"),
    ] {
        assert_eq!(
            unhex(&k("UTF-8", "UTF-7", s.as_bytes())),
            want.as_bytes(),
            "{s}"
        );
        assert_eq!(
            unhex(&k("UTF-8", "UTF-7-IMAP", s.as_bytes())),
            imap.as_bytes(),
            "{s}"
        );
    }
    for (s, want) in [
        ("+AGEAYgBj-", "006100620063"),
        ("+AGEAYgBj", "006100620063"),
        ("+-", "002b"),
        ("+", "i"),
        ("a+Z", "0061"),
        ("+AGEA-", "x"),
        ("+2D3eAA-", "d83dde00"),
        ("+2D3-", "x"),
        ("+AGEAYgBjAAA-", "0061006200630000"),
        ("+AGF-", "x"),
        ("~\\", "x"),
        ("+AGE-x", "00610078"),
        ("+AGE.x", "0061002e0078"),
        ("+AGE+AGI-", "x"),
        ("+3gA-", "x"),
    ] {
        assert_eq!(k("UTF-7", "UTF-16BE", s.as_bytes()), want, "{s}");
    }
    for (s, want) in [
        ("&AGEAYgBj-", "006100620063"),
        ("&-", "0026"),
        ("&", "i"),
        ("&AGEA-", "x"),
        ("&,,8-", "ffff"),
        ("&/v8-", "x"),
        ("+AGE-", "002b004100470045002d"),
    ] {
        assert_eq!(k("UTF-7-IMAP", "UTF-16BE", s.as_bytes()), want, "{s}");
    }
    assert_eq!(
        k("UTF-7", "UTF-16BE//IGNORE", b"+AGE\\AGI-x"),
        "0061004100470049002d0078"
    );
}

/// Observed on PHP 8.5 (glibc 2.43 iconv): ISO-2022-JP and ISO-2022-KR.
#[test]
fn observed_iso2022() {
    for (s, want) in [
        ("aあa", "611b244224221b284261"),
        ("¥", "1b284a5c1b2842"),
        ("¥a\\", "1b284a5c611b28425c"),
        ("‾", "1b284a7e1b2842"),
        ("ｱ", "x"),
        ("あ¥あ", "1b244224221b284a5c1b244224221b2842"),
        ("あ\nあ", "1b244224221b28420a1b244224221b2842"),
        ("", "e"),
    ] {
        assert_eq!(k("UTF-8", "ISO-2022-JP", s.as_bytes()), want, "{s}");
    }
    assert_eq!(
        k("UTF-8", "ISO-2022-JP//TRANSLIT", "ｱ".as_bytes()),
        "1b244225221b2842"
    );
    assert_eq!(
        k("UTF-8", "ISO-2022-JP//TRANSLIT", "あé".as_bytes()),
        "1b244224221b284265"
    );
    assert_eq!(
        k("UTF-8", "ISO-2022-JP//IGNORE", "あéあ".as_bytes()),
        "1b2442242224221b2842"
    );
    for (s, want) in [
        (&b"\x1b$B$\"\n$\"\x1b(B"[..], "e381820ae38182"),
        (b"\x1b$B$\"", "e38182"),
        (b"\x1b$B$", "i"),
        (b"\x1b", "i"),
        (b"\x1bA", "i"),
        (b"\x1b(Z", "1b285a"),
        (b"a\x0eb", "610e62"),
        (b"a\xa4\xa2b", "x"),
        (b"\x1b(J\\~a\x1b(B", "c2a5e280be61"),
        (b"\x1b$@\x30\x21\x1b(B", "e4ba9c"),
    ] {
        assert_eq!(k("ISO-2022-JP", "UTF-8", s), want, "{s:?}");
    }
    assert_eq!(k("ISO-2022-JP", "UTF-8//IGNORE", b"a\xa4\xa2b"), "6162");
    for (s, want) in [
        ("a", "1b24294361"),
        ("", "1b242943"),
        ("a가b", "1b242943610e30210f62"),
        ("가\n가", "1b2429430e30210f0a0e30210f"),
        ("₩", "x"),
    ] {
        assert_eq!(k("UTF-8", "ISO-2022-KR", s.as_bytes()), want, "{s}");
    }
    for (s, want) in [
        (&b"\x0e\x30\x21\x0fa"[..], "eab08061"),
        (b"a\x1b$)C\x0e\x30\x21\x0fa", "61eab08061"),
        (b"\x1b$)C\x0e\x30\x21", "eab080"),
        (b"\x1b$)C\x0e\x30\x21\n\x30\x21", "x"),
        (b"\x1b$", "i"),
        (b"\x1bA", "1b41"),
        (b"\x0e", "e"),
        (b"\x0eA", "i"),
    ] {
        assert_eq!(k("ISO-2022-KR", "UTF-8", s), want, "{s:?}");
    }
}

/// Observed on PHP 8.5 (glibc 2.43 iconv): table charsets' corners.
#[test]
fn observed_tables() {
    // Composition on decode, decomposition on encode.
    assert_eq!(k("UTF-8", "CP1258", "a\u{301}".as_bytes()), "61ec");
    assert_eq!(k("CP1258", "UTF-8", b"a\xec"), "c3a1");
    assert_eq!(k("CP1258", "UTF-8", b"a\xecb"), "c3a162");
    assert_eq!(k("UTF-8", "CP1258", "Ạ".as_bytes()), "41f2");
    assert_eq!(k("CP1255", "UTF-8", b"\xe0\xc8"), "efacaf");
    assert_eq!(k("UTF-8", "CP1255", "\u{fb2a}".as_bytes()), "f9d1");
    assert_eq!(k("UTF-8", "TCVN", "a\u{301}".as_bytes()), "61b3");
    assert_eq!(k("TCVN", "UTF-8", b"a\xb3"), "c3a1");
    assert_eq!(k("CP1258", "UTF-8", b"\xec\xec"), "cc81cc81");
    // Big5-HKSCS pairs.
    assert_eq!(k("UTF-8", "BIG5-HKSCS", "Ê\u{304}".as_bytes()), "8862");
    assert_eq!(k("UTF-8", "BIG5-HKSCS", "Ê".as_bytes()), "8866");
    assert_eq!(k("UTF-8", "BIG5-HKSCS", "Êx".as_bytes()), "886678");
    // GB18030 four-byte sequences.
    for (h, want, ign) in [
        ("8130", "i", "0042"),
        ("813020", "i", "00410042"),
        ("81308120", "x", "00410042"),
        ("8431a530", "x", "00410042"),
        ("8431a439", "ffff", "ffff00410042"),
        ("e3329a35", "dbffdfff", "dbffdfff00410042"),
        ("90308130", "d800dc00", "d800dc0000410042"),
        ("81308130", "0080", "008000410042"),
    ] {
        assert_eq!(k("GB18030", "UTF-16BE", &unhex(h)), want, "{h}");
        let mut s = unhex(h);
        s.extend_from_slice(b"AB");
        assert_eq!(k("GB18030", "UTF-16BE//IGNORE", &s), ign, "{h}");
    }
    // Shift_JIS: JIS X 0201 Roman in the single bytes; //IGNORE skips one or
    // two bytes of an illegal pair.
    assert_eq!(k("SJIS", "UTF-8", b"\\~"), "c2a5e280be");
    assert_eq!(k("UTF-8", "SJIS", b"\\~"), "5c7e");
    assert_eq!(k("SJIS", "UTF-8//IGNORE", b"\x81\x20A"), "2041");
    assert_eq!(k("SJIS", "UTF-8//IGNORE", b"\x81\x7fA"), "41");
    // EUC-JP-MS writes U+FFFD for bad half-width katakana under //IGNORE.
    assert_eq!(k("EUC-JP-MS", "UTF-16BE//IGNORE", b"\x8e\xe0A"), "fffd0041");
    assert_eq!(k("EUC-JP-MS", "UTF-16BE", b"\x8e\xe0A"), "x");
}

/// Tests from ext/iconv/tests: the string functions.
#[test]
fn phpt_strings() {
    let mut d = Vec::new();
    // bug37176.phpt
    assert_eq!(strpos(b"11--", b"1-", 0, b"UTF-8", &mut d), Ok(Some(1)));
    assert_eq!(strpos(b"-11--", b"1-", 0, b"UTF-8", &mut d), Ok(Some(2)));
    // bug37773.phpt, bug72320.phpt
    assert_eq!(substr(b"x", 0, Some(1), b"UTF-8", &mut d).unwrap(), b"x");
    assert_eq!(substr(b"", 0, Some(10), b"UTF-8", &mut d).unwrap(), b"");
    assert_eq!(substr(b"foo", 3, Some(10), b"UTF-8", &mut d).unwrap(), b"");
    // bug69840.phpt
    assert_eq!(
        substr(b"a\x00b\x00", 0, Some(1), b"UTF-16LE", &mut d).unwrap(),
        b"a\0"
    );
    assert_eq!(
        substr(b"\x00a\x00b", 0, Some(1), b"UTF-16BE", &mut d).unwrap(),
        b"\0a"
    );
    // bug79200.phpt
    assert_eq!(strlen(b"test test", b"WINDOWS-1258", &mut d), Some(9));
    assert_eq!(
        strpos(b"test test", b"test test", 0, b"WINDOWS-1258", &mut d),
        Ok(Some(0))
    );
    assert_eq!(
        substr(b"test test", 0, Some(9), b"WINDOWS-1258", &mut d).unwrap(),
        b"test test"
    );
    // iconv_strlen.phpt, iconv_strlen_basic.phpt
    assert_eq!(strlen(b"abc", b"ASCII", &mut d), Some(3));
    assert_eq!(
        strlen(&unhex("c6fccbdcb8ec204555432d4a50"), b"EUC-JP", &mut d),
        Some(10)
    );
    let mb = unhex(
        "e697a5e69cace8aa9ee38386e382ade382b9e38388e381a7e38199e380823031323334efbc95efbc96efbc97efbc98efbc99e38082",
    );
    assert_eq!(strlen(&mb, b"UTF-8", &mut d), Some(21));
    assert!(d.is_empty());
    // iconv_strlen_error2.phpt and friends
    let d = diags_of(|d| assert_eq!(strlen(b"abcdef", b"unknown-encoding", d), None));
    assert_eq!(
        d,
        [warning(
            "Wrong encoding, conversion from \"unknown-encoding\" to \"UCS-4LE\" is not allowed"
        )]
    );
    let d = diags_of(|d| assert_eq!(substr(b"Hello", 1, Some(5), b"unknown-encoding", d), None));
    assert_eq!(d.len(), 1);
    let d = diags_of(|d| assert_eq!(strrpos(b"Hello", b"l", b"unknown-encoding", d), None));
    assert_eq!(d.len(), 1);
    let d = diags_of(|d| assert_eq!(strpos(b"Hello", b"l", 2, b"unknown-encoding", d), Ok(None)));
    assert_eq!(d.len(), 1);
    let d = diags_of(|d| assert_eq!(strlen(b"1", &[b'/'; 100], d), None));
    assert_eq!(
        d,
        [warning(
            "Encoding parameter exceeds the maximum allowed length of 64 characters"
        )]
    );
    let mut d = Vec::new();
    // iconv_strpos.phpt
    let oob = Err(IconvError::ValueError(
        "iconv_strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)".into(),
    ));
    assert_eq!(
        strpos(b"abecdbcdabef", b"bcd", -1, b"ISO-8859-1", &mut d),
        Ok(None)
    );
    assert_eq!(
        strpos(b"abecdbcdabef", b"bcd", -7, b"ISO-8859-1", &mut d),
        Ok(Some(5))
    );
    assert_eq!(
        strpos(b"abecdbcdabef", b"bcd", 12, b"ISO-8859-1", &mut d),
        Ok(None)
    );
    assert_eq!(
        strpos(b"abecdbcdabef", b"bcd", 100000, b"ISO-8859-1", &mut d),
        oob
    );
    assert_eq!(
        strpos(b"abcabcabcdabcababcdabc", b"bcd", 10, b"ISO-8859-1", &mut d),
        Ok(Some(16))
    );
    let mut h = b"abcab".repeat(60);
    h.extend_from_slice(b"abcdb");
    h.extend_from_slice(&b"adabc".repeat(60));
    assert_eq!(strpos(&h, b"abcd", 0, b"ISO-8859-1", &mut d), Ok(Some(300)));
    let a = unhex("a4a2a4a4a4a6a4a8a4aa");
    let mut h = a.repeat(30);
    h.extend_from_slice(&unhex("a4a4a4a6a4aaa4a8a4a2"));
    h.extend_from_slice(&unhex("a4a2a4a4a4a8a4aaa4a6").repeat(30));
    assert_eq!(
        strpos(&h, &unhex("a4a6a4aa"), 0, b"EUC-JP", &mut d),
        Ok(Some(151))
    );
    let mut h = unhex("a4a2a4a4a4a6a4a8a4aa").repeat(60);
    h.push(b'$');
    h.extend_from_slice(&unhex("a4a2a4a4a4a8a4aaa4a6").repeat(60));
    let jp = iconv(b"EUC-JP", b"ISO-2022-JP", &h, &mut d).unwrap();
    assert_eq!(strpos(&jp, b"$", 0, b"ISO-2022-JP", &mut d), Ok(Some(300)));
    assert_eq!(strpos(b"string", b"", 0, b"ISO-8859-1", &mut d), Ok(None));
    assert_eq!(strpos(b"", b"string", 0, b"ISO-8859-1", &mut d), Ok(None));
    // iconv_strpos_variation5.phpt
    for (off, want) in [
        (-30, None),
        (-20, Some(9)),
        (-10, Some(20)),
        (0, Some(9)),
        (20, Some(20)),
        (30, None),
    ] {
        let r = strpos(&mb, "。".as_bytes(), off, b"UTF-8", &mut d);
        match want {
            Some(w) => assert_eq!(r, Ok(Some(w)), "{off}"),
            None => assert_eq!(r, oob, "{off}"),
        }
    }
    // iconv_strrpos.phpt
    assert_eq!(
        strrpos(b"abecdbcdabcdef", b"bcd", b"ISO-8859-1", &mut d),
        Some(9)
    );
    let mut h = a.repeat(30);
    h.extend_from_slice(&unhex("a4a4a4a6a4aaa4a8a4a2"));
    h.extend_from_slice(&unhex("a4a2a4a4a4a8a4aaa4a6").repeat(30));
    assert_eq!(
        strrpos(&h, &unhex("a4a6a4aa"), b"EUC-JP", &mut d),
        Some(151)
    );
    for i in 0..=6usize {
        let mut h = a.repeat(60);
        h.extend_from_slice(&b"$".repeat(i));
        h.extend_from_slice(&unhex("a4a2a4a4a4a8a4aaa4a6").repeat(60));
        let jp = iconv(b"EUC-JP", b"ISO-2022-JP", &h, &mut d).unwrap();
        assert_eq!(strlen(&jp, b"ISO-2022-JP", &mut d), Some(600 + i));
        assert_eq!(
            strrpos(&jp, b"$", b"ISO-2022-JP", &mut d),
            if i == 0 { None } else { Some(299 + i) }
        );
    }
    assert_eq!(strrpos(b"string", b"", b"ISO-8859-1", &mut d), None);
    assert_eq!(strrpos(b"", b"string", b"ISO-8859-1", &mut d), None);
    // iconv_substr.phpt
    let az = b"abcdefghijklmnopqrstuvwxyz";
    assert_eq!(
        substr(az, 5, Some(7), b"ASCII", &mut d).unwrap(),
        b"fghijkl"
    );
    let kana = unhex("a4a2a4a4a4a6a4a8a4aaa4aba4ada4afa4b1a4b3a4b5a4b7a4b9");
    assert_eq!(
        hex(&substr(&kana, 5, Some(7), b"EUC-JP", &mut d).unwrap()),
        "a4aba4ada4afa4b1a4b3a4b5a4b7"
    );
    let t = b"This is a test";
    assert_eq!(substr(t, 100000, None, b"ISO-8859-1", &mut d).unwrap(), b"");
    assert_eq!(
        substr(t, 0, Some(100000), b"ISO-8859-1", &mut d).unwrap(),
        t
    );
    assert_eq!(substr(t, -3, None, b"ISO-8859-1", &mut d).unwrap(), b"est");
    assert_eq!(
        substr(t, 0, Some(-9), b"ISO-8859-1", &mut d).unwrap(),
        b"This "
    );
    assert_eq!(
        substr(t, 0, Some(-100000), b"ISO-8859-1", &mut d).unwrap(),
        b""
    );
    assert_eq!(
        substr(t, -9, Some(-100000), b"ISO-8859-1", &mut d).unwrap(),
        b""
    );
    let src = unhex("a4b3a4f3a4cba4c1a4cf2049534f2d323032322d4a50");
    let jp = iconv(b"EUC-JP", b"ISO-2022-JP", &src, &mut d).unwrap();
    let sub = substr(&jp, 3, Some(8), b"ISO-2022-JP", &mut d).unwrap();
    assert_eq!(
        hex(&iconv(b"ISO-2022-JP", b"EUC-JP", &sub, &mut d).unwrap()),
        "a4c1a4cf2049534f2d32"
    );
    // iconv_substr_basic.phpt
    assert_eq!(
        hex(&substr(&mb, 2, Some(7), b"ISO-8859-1", &mut d).unwrap()),
        "a5e69cace8aa9e"
    );
    assert_eq!(
        hex(&substr(&mb, 2, Some(7), b"utf-8", &mut d).unwrap()),
        "e8aa9ee38386e382ade382b9e38388e381a7e38199"
    );
    // iconv_substr_out_of_bounds.phpt
    let u = "äöü".as_bytes();
    for (s, off, len, want) in [
        (&b"foo"[..], 3, None, &b""[..]),
        (b"foo", -3, None, b"foo"),
        (b"foo", 4, None, b""),
        (b"foo", -4, None, b"foo"),
        (u, 3, None, b""),
        (u, -3, None, u),
        (u, 4, None, b""),
        (u, -4, None, u),
        (b"foo", 0, Some(3), b"foo"),
        (b"foo", 0, Some(-3), b""),
        (b"foo", 0, Some(4), b"foo"),
        (b"foo", 0, Some(-4), b""),
        (u, 0, Some(3), u),
        (u, 0, Some(-3), b""),
        (u, 0, Some(4), u),
        (u, 0, Some(-4), b""),
        (u, -4, Some(1), "ä".as_bytes()),
        (u, -4, Some(-1), "äö".as_bytes()),
        (u, 2, Some(-2), b""),
    ] {
        assert_eq!(
            substr(s, off, len, b"UTF-8", &mut d).unwrap(),
            want,
            "{off} {len:?}"
        );
    }
    assert!(d.is_empty(), "{d:?}");
}

/// test ext/iconv/tests/iconv_strpos_error_001.phpt: an ini default is not
/// length-checked.
#[test]
fn phpt_default_charset_not_length_checked() {
    let x = vec![b'X'; 256];
    let d = diags_of(|d| {
        assert_eq!(
            strpos(b"Hello, world", b"world", -2, CharsetArg::Default(&x), d),
            Ok(None)
        );
    });
    assert!(
        matches!(&d[..], [Diag::Warning(m)] if m.starts_with("Wrong encoding, conversion from \"XXXX"))
    );
    let d = diags_of(|d| assert_eq!(strlen(b"a", CharsetArg::Given(&x), d), None));
    assert!(matches!(&d[..], [Diag::Warning(m)] if m.starts_with("Encoding parameter exceeds")));
    assert!(!check_charset_len(&[b'/'; 64], &mut Vec::new()));
}

/// Observed on PHP 8.5: the string functions' errors and encodings.
#[test]
fn observed_strings() {
    let d = diags_of(|d| assert_eq!(strlen(b"ab\xffcd", b"UTF-8", d), None));
    assert_eq!(d, [notice(ILLEGAL)]);
    let d = diags_of(|d| assert_eq!(strlen(b"ab\xc3", b"UTF-8", d), None));
    assert_eq!(d, [notice(INCOMPLETE)]);
    let mut d = Vec::new();
    assert_eq!(
        strlen(b"\x1b$B$\"\x1b(Bab", b"ISO-2022-JP", &mut d),
        Some(3)
    );
    assert_eq!(strlen(b"\xff\xfea\0b\0", b"UTF-16", &mut d), Some(2));
    assert_eq!(strlen(b"\xff\xfea\0b\0", b"UTF-16LE", &mut d), Some(3));
    assert_eq!(strlen(b"abc", b"UTF-8//TRANSLIT", &mut d), Some(3));
    assert_eq!(
        hex(&substr(b"\x1b$B$\"$$\x1b(Bab", 1, Some(2), b"ISO-2022-JP", &mut d).unwrap()),
        "1b244224241b284261"
    );
    assert_eq!(
        hex(&substr(b"\xff\xfea\0b\0c\0", 1, Some(1), b"UTF-16", &mut d).unwrap()),
        "fffe6200"
    );
    assert_eq!(
        hex(&substr("+ZeVnLIqe-".as_bytes(), 1, Some(1), b"UTF-7", &mut d).unwrap()),
        "2b5a79772d"
    );
    assert_eq!(
        hex(&substr(b"a\0\0\0b\0\0\0", 1, Some(1), b"UTF-32", &mut d).unwrap()),
        "fffe000062000000"
    );
    assert_eq!(
        strpos(b"\xff\xfea\0b\0", b"b\0", 0, b"UTF-16", &mut d),
        Ok(Some(1))
    );
    assert_eq!(strpos(b"aaab", b"aab", 0, b"UTF-8", &mut d), Ok(Some(1)));
    assert_eq!(strrpos(b"aaaa", b"aa", b"UTF-8", &mut d), Some(2));
    assert!(d.is_empty());
    // Illegal input anywhere fails, even after the substring.
    let d = diags_of(|d| assert_eq!(substr(b"abcd\xff", 0, Some(2), b"UTF-8", d), None));
    assert_eq!(d, [notice(ILLEGAL)]);
    let d = diags_of(|d| assert_eq!(strpos(b"ab", b"\xff", 0, b"UTF-8", d), Ok(None)));
    assert_eq!(d, [notice(ILLEGAL)]);
}

#[test]
#[ignore]
fn time_small_calls() {
    for (f, t, s) in [
        ("SHIFT_JIS", "UTF-32BE", &b"\x81\x40"[..]),
        ("ISO-8859-1", "UTF-32BE", b"a"),
        ("UTF-8", "UTF-32BE", b"a"),
        ("UTF-8", "SHIFT_JIS", b"a"),
    ] {
        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            std::hint::black_box(k(f, t, s));
        }
        eprintln!("{f}->{t}: {:?} per call", start.elapsed() / 100_000);
    }
}

/// Debugging aid: `ICONV_DEBUG_TR=<target>:<hex block>,...`.
#[test]
#[ignore]
fn dump_debug_translit() {
    let Ok(list) = std::env::var("ICONV_DEBUG_TR") else {
        return;
    };
    let mut lines = Vec::new();
    for spec in list.split(',') {
        let (t, b) = spec.split_once(':').unwrap();
        let lo = u32::from_str_radix(b, 16).unwrap();
        for cp in lo.max(0x80)..lo + 0x1000 {
            if (0xD800..0xE000).contains(&cp) {
                continue;
            }
            lines.push(format!(
                "{t} {cp:X} {}",
                k(
                    "UTF-32BE",
                    &format!("{t}//TRANSLIT//IGNORE"),
                    &cp.to_be_bytes()
                )
            ));
        }
    }
    std::fs::write(
        "/tmp/claude-1000/goro-work/charset-iconv/rust-tr.txt",
        lines.join("\n") + "\n",
    )
    .unwrap();
}

/// Debugging aid: `ICONV_DEBUG=<charset> cargo test dump_debug -- --ignored`
/// writes the DEC/DECI/ENC entries to /tmp/claude-1000/goro-work/charset-iconv/rust-<charset>.txt.
#[test]
#[ignore]
fn dump_debug() {
    let Ok(list) = std::env::var("ICONV_DEBUG") else {
        return;
    };
    for cs in list.split(',') {
        let mut lines = Vec::new();
        let mut queue: Vec<Vec<u8>> = vec![Vec::new()];
        let mut q = 0;
        while q < queue.len() {
            let pre = queue[q].clone();
            q += 1;
            for b in 0..=255u8 {
                let mut s = pre.clone();
                s.push(b);
                let r = k(cs, "UTF-32BE", &s);
                let mut si = s.clone();
                si.push(b'A');
                lines.push(format!(
                    "{} {} {}",
                    hex(&s),
                    r,
                    k(cs, "UTF-32BE//IGNORE", &si)
                ));
                if r == "i" && s.len() < 3 {
                    queue.push(s);
                }
            }
        }
        for b in 0..16u32 {
            lines.push(format!(
                "ENC {b:X} {}",
                k(
                    "UTF-32BE",
                    &format!("{cs}//IGNORE"),
                    &block(b << 12, (b + 1) << 12)
                )
            ));
        }
        let path = format!("/tmp/claude-1000/goro-work/charset-iconv/rust-{cs}.txt");
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }
}

/// test ext/iconv/tests/iconv_stream_filter.phpt: convert.iconv.ISO-2022-JP/EUC-JP
/// gives the same bytes whatever the pieces the input comes in.
#[test]
fn phpt_stream_filter() {
    let data = unhex("1b244224332473244b2441244f1b284269636f6e760a");
    let (from, _) = parse_charset(b"ISO-2022-JP").unwrap();
    let (to, flags) = parse_charset(b"EUC-JP").unwrap();
    for piece in 1..=data.len() {
        let mut s = StreamConverter::new(from, to, flags);
        let mut out = Vec::new();
        for chunk in data.chunks(piece) {
            s.feed(chunk, &mut out).unwrap();
        }
        s.finish(&mut out).unwrap();
        assert_eq!(hex(&out), "a4b3a4f3a4cba4c1a4cf69636f6e760a", "{piece}");
    }
    // bug76249.phpt: an odd byte left at the end of UCS-2 input.
    let (from, _) = parse_charset(b"ucs-2").unwrap();
    let (to, flags) = parse_charset(b"utf-8//IGNORE").unwrap();
    let mut s = StreamConverter::new(from, to, flags);
    let mut out = Vec::new();
    s.feed(b"abc", &mut out).unwrap();
    assert!(s.finish(&mut out).is_err());
    // gh17047.phpt: UTF-16BE input "test" decodes; the second filter then sees
    // UTF-8 bytes as UTF-16BE and fails on an odd length.
    let (from, _) = parse_charset(b"UTF-16BE").unwrap();
    let (to, flags) = parse_charset(b"UTF-8").unwrap();
    let mut s = StreamConverter::new(from, to, flags);
    let mut out = Vec::new();
    s.feed(b"test", &mut out).unwrap();
    s.finish(&mut out).unwrap();
    assert_eq!(hex(&out), "e791a5e78db4");
}

/// Benchmark against PHP 8.5's iconv() (same inputs as the PHP loop in the
/// crate's report): `cargo test --release -p goro-charset bench_iconv -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_iconv() {
    fn mk(unit: &str) -> Vec<u8> {
        unit.repeat((1 << 20) / unit.len() + 1).into_bytes()
    }
    let latin = mk("Pâté façon grand-mère, élégance française à Zürich, déjà vu. ");
    let ascii = mk("The quick brown fox jumps over the lazy dog, 0123456789. ");
    let cjk = mk("日本語のテキストです。漢字とひらがなとカタカナ。");
    let mut d = Vec::new();
    let euc = iconv(b"UTF-8", b"EUC-JP", &cjk, &mut d).unwrap();
    let latin1 = iconv(b"UTF-8", b"ISO-8859-1", &latin, &mut d).unwrap();
    let cases: [(&str, &str, &[u8], &str); 9] = [
        ("UTF-8", "ISO-8859-1", &latin, "utf8->latin1 (accented)"),
        ("UTF-8", "ISO-8859-1", &ascii, "utf8->latin1 (ascii)"),
        ("UTF-8", "UTF-16LE", &latin, "utf8->utf16le (accented)"),
        ("UTF-8", "UTF-16LE", &cjk, "utf8->utf16le (cjk)"),
        ("EUC-JP", "UTF-8", &euc, "eucjp->utf8"),
        ("UTF-8", "EUC-JP", &cjk, "utf8->eucjp"),
        ("ISO-8859-1", "UTF-8", &latin1, "latin1->utf8"),
        ("UTF-8", "UTF-8", &cjk, "utf8->utf8 (cjk)"),
        ("UTF-8", "ASCII//TRANSLIT", &latin, "utf8->ascii//translit"),
    ];
    for (f, t, s, label) in cases {
        let mut r = iconv(f.as_bytes(), t.as_bytes(), s, &mut d).unwrap();
        // Best of 5 rounds of 20.
        let mut dt = f64::MAX;
        for _ in 0..5 {
            let t0 = std::time::Instant::now();
            for _ in 0..20 {
                r = iconv(f.as_bytes(), t.as_bytes(), s, &mut d).unwrap();
            }
            dt = dt.min(t0.elapsed().as_secs_f64() * 1000.0 / 20.0);
        }
        eprintln!(
            "{label:<28} in {:7} B out {:7} B  {dt:8.3} ms  crc {:08x}",
            s.len(),
            r.len(),
            crc32(&r)
        );
    }
}

/// The fast paths give what the general loop gives, for every charset.
#[test]
fn fast_paths_match_generic() {
    use super::charset::Kind;
    let generic = |from: Charset, to: Charset, s: &[u8]| {
        let mut c = super::convert::Converter::new(from.decoder(), to.encoder(), Flags::default());
        let mut out = Vec::new();
        c.run(s, &mut out).map(|()| out).map_err(|e| e.kind)
    };
    let utf8 = Charset::lookup("UTF-8").unwrap();
    for info in super::tables::names::CHARSETS.iter() {
        let cs = Charset(info);
        // Every BMP character the charset encodes, and some others.
        let enc = cs.encoder();
        let mut text = String::new();
        for cp in 0..0x1_0400u32 {
            if let Some(ch) = char::from_u32(cp)
                && enc.can_encode(cp)
            {
                text.push(ch);
            }
        }
        let fast = super::fast::convert(utf8, cs, text.as_bytes());
        let slow = generic(utf8, cs, text.as_bytes());
        if let Some(f) = &fast {
            assert_eq!(Ok(f.clone()), slow, "UTF-8 -> {}", info.name);
        }
        if let Ok(bytes) = &slow
            && matches!(info.kind, Kind::Table(_))
        {
            let back_slow = generic(cs, utf8, bytes);
            if let Some(f) = super::fast::convert(cs, utf8, bytes) {
                assert_eq!(Ok(f), back_slow, "{} -> UTF-8", info.name);
            }
        }
        text.push('\u{10ffff}');
        if super::fast::convert(utf8, cs, text.as_bytes()).is_some() {
            assert!(enc.can_encode(0x10_FFFF), "{}", info.name);
        }
    }
}
