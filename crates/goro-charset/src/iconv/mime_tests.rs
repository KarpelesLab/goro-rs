//! MIME header tests: expectations from ext/iconv/tests/*.phpt and from
//! runs of PHP 8.5 (glibc 2.43 iconv).

use super::*;

const STRICT: i64 = MIME_DECODE_STRICT;
const CONT: i64 = MIME_DECODE_CONTINUE_ON_ERROR;

fn dec(s: &[u8], mode: i64, cs: &str) -> Result<Vec<u8>, Diag> {
    let mut d = Vec::new();
    match mime_decode(s, mode, cs.as_bytes(), &mut d) {
        Some(v) => Ok(v),
        None => Err(d.remove(0)),
    }
}

fn dec_str(s: &str, mode: i64) -> Result<String, Diag> {
    dec(s.as_bytes(), mode, "UTF-8").map(|v| String::from_utf8(v).unwrap())
}

fn headers(s: &str, mode: i64) -> Vec<(String, Vec<String>)> {
    let mut d = Vec::new();
    mime_decode_headers(s.as_bytes(), mode, b"UTF-8", &mut d)
        .unwrap()
        .into_iter()
        .map(|(k, v)| {
            (
                String::from_utf8(k).unwrap(),
                v.into_iter()
                    .map(|x| String::from_utf8(x).unwrap())
                    .collect(),
            )
        })
        .collect()
}

fn enc(
    name: &str,
    value: &[u8],
    scheme: MimeScheme,
    inc: &str,
    outc: &str,
    ll: i64,
    lf: &str,
) -> Result<String, Diag> {
    let opts = MimeEncodeOptions {
        scheme,
        input_charset: Some(inc.as_bytes().to_vec()),
        output_charset: Some(outc.as_bytes().to_vec()),
        line_length: ll,
        line_break_chars: lf.as_bytes().to_vec(),
    };
    let mut d = Vec::new();
    match mime_encode(name.as_bytes(), value, &opts, b"ISO-8859-1", &mut d) {
        Some(v) => Ok(String::from_utf8(v).unwrap()),
        None => Err(d.remove(0)),
    }
}

fn malformed() -> Diag {
    Diag::Warning("Malformed string".into())
}

fn illegal() -> Diag {
    Diag::Notice("Detected an illegal character in input string".into())
}

#[test]
fn phpt_mime_decode() {
    // bug51250.phpt
    let b = "Illegal encoded-word: =?utf-8?B?\u{a1}?= .".replace('\u{a1}', "\u{0}");
    let b = b.replace('\0', "");
    let _ = b;
    let mut ill_b = b"Illegal encoded-word: =?utf-8?B?".to_vec();
    ill_b.push(0xA1);
    ill_b.extend_from_slice(b"?= .");
    let mut ill_q = b"Illegal encoded-word: =?utf-8?Q?".to_vec();
    ill_q.push(0xA1);
    ill_q.extend_from_slice(b"?= .");
    assert_eq!(
        dec_str("Legal encoded-word: =?utf-8?B?Kg==?= .", CONT).unwrap(),
        "Legal encoded-word: * ."
    );
    assert_eq!(
        dec_str("Legal encoded-word: =?utf-8?Q?*?= .", CONT).unwrap(),
        "Legal encoded-word: * ."
    );
    assert_eq!(
        dec(&ill_b, CONT, "UTF-8").unwrap(),
        b"Illegal encoded-word:  ."
    );
    assert_eq!(
        dec(&ill_q, CONT, "UTF-8").unwrap(),
        b"Illegal encoded-word: ."
    );
    assert_eq!(
        dec_str("Legal encoded-word: =?utf-8?B?Kg==?= .", 0).unwrap(),
        "Legal encoded-word: * ."
    );
    assert_eq!(
        dec_str("Legal encoded-word: =?utf-8?Q?*?= .", 0).unwrap(),
        "Legal encoded-word: * ."
    );
    assert_eq!(
        dec(&ill_b, 0, "UTF-8").unwrap(),
        b"Illegal encoded-word:  ."
    );
    assert_eq!(dec(&ill_q, 0, "UTF-8"), Err(illegal()));
    // bug53304.phpt
    for s in [
        "=?utf-8?Q?Nachricht_=c3=bcber_Kontaktformular_www.inexio.net?=",
        "=?utf-8?Q?Nachricht_=C3=BCber_Kontaktformular_www.inexio.net?=",
    ] {
        assert_eq!(
            dec_str(s, 0).unwrap(),
            "Nachricht über Kontaktformular www.inexio.net"
        );
    }
    // bug60494.phpt
    for s in ["ä", "ö", "ß"] {
        assert_eq!(dec_str(s, 0), Err(illegal()));
    }
    // bug68180.phpt
    let s = "=?UTF-8?Q?=E3=80=8E=E3=80=90=E5=A4=96=E8=B3=87=E7=B3=BB=E6=88=A6=E7=95=A5=E3=82=B3=E3=83=B3=E3=82=B5=E3=83=AB=E3=81=8C=E9=9B=86=E7=B5=90=E3=80=91=E3=83=88=E3=83=83=E3=83=97=E3=82=B3=E3=83=B3=E3=82=B5=E3=83=AB=E3=82=BF=E3=83=B3=E3=83=88=E3=81=A8=E8=A9=B1=E3=81=9B=E3=82=8B=E3=82=B3=E3=83=B3=E3=82=B5=E3=83=AB=E6=A5=AD=E7=95=8C=E7=A0=94=E7=A9=B6=E3=82=BB=E3=83=9F=E3=83=8A=E3=83=BC=E3=80=8F=E3=81=B8=E3=81=AE=E3=82=A8=E3=83=B3=E3=83=88=E3=83=AA=E3=83=BC=E3=81=82=E3=82=8A=E3=81=8C=E3=81=A8=E3=81=86=E3=81=94=E3=81=96=E3=81=84=E3=81=BE=E3=81=97=E3=81=9F=E3=80=82?=";
    assert_eq!(
        dec_str(s, STRICT).unwrap(),
        "『【外資系戦略コンサルが集結】トップコンサルタントと話せるコンサル業界研究セミナー』へのエントリーありがとうございました。"
    );
    // gh7980.phpt, bug79200.phpt
    assert_eq!(
        dec_str(
            "=?windows-1258?Q?DSI_Charg=E9_de_Formation_Jean_Dupont?= <jdupont@example.fr>",
            STRICT
        )
        .unwrap(),
        "DSI Chargé de Formation Jean Dupont <jdupont@example.fr>"
    );
    assert_eq!(
        dec_str("=?windows-1258?Q?test=20test?=", 0).unwrap(),
        "test test"
    );
    // iconv_mime_decode.phpt (iconv.internal_charset only matters for the
    // test's own iconv_strlen calls)
    let cases = [
        "Subject: =?ISO-8859-1?Q?Pr=FCfung?=\n    =?ISO-8859-1*de_DE?Q?Pr=FCfung?=\t\n     =?ISO-8859-2?Q?k=F9=D4=F1=D3let?=",
        "Subject: =?ISO-8859-1?Q?Pr=FCfung?= =?ISO-8859-1*de_DE?Q?=20Pr=FCfung?= \t  =?ISO-8859-2?Q?k=F9=D4=F1=D3let?=",
        "Subject: =?ISO-8859-1?Q?Pr=FCfung?==?ISO-8859-1*de_DE?Q?Pr=FCfung?==?ISO-8859-2?Q?k=F9=D4=F1=D3let?=",
        "Subject: =?ISO-8859-1?Q?Pr=FCfung?= =?ISO-8859-1*de_DE?Q?Pr=FCfung??   =?ISO-8859-2?X?k=F9=D4=F1=D3let?=",
        "From: =?ISO-2022-JP?B?GyRCJTUbKEI=?=\n =?ISO-2022-JP?B?GyRCJXMlVxsoQg==?=\n =?ISO-2022-JP?B?GyRCJWtKOBsoQg==?=\n =?ISO-2022-JP?B?GyRCO3pOcxsoQg==?=\n =?ISO-2022-JP?B?GyRCJTUlcxsoQg==?=\n =?ISO-2022-JP?B?GyRCJVclaxsoQg==?=\n =?ISO-2022-JP?B?GyRCSjg7ehsoQg==?=\n =?ISO-2022-JP?B?GyRCTnNGfBsoQg==?=\n =?ISO-2022-JP?B?GyRCS1w4bBsoQg==?=\n =?ISO-2022-JP?B?GyRCJUYlLRsoQg==?=\n =?ISO-2022-JP?B?GyRCJTklSBsoQg==?=",
    ];
    let ok1 = "Subject: PrüfungPrüfungkůÔńÓlet";
    let ok2 = "Subject: Prüfung PrüfungkůÔńÓlet";
    let jp = "From: サンプル文字列サンプル文字列日本語テキスト";
    let raw3 = cases[2];
    let cont4 =
        "Subject: Prüfung=?ISO-8859-1*de_DE?Q?Pr=FCfung??   =?ISO-8859-2?X?k=F9=D4=F1=D3let?=";
    let expected: [[Result<&str, Diag>; 5]; 4] = [
        [Ok(ok1), Ok(ok2), Ok(ok1), Err(malformed()), Ok(jp)],
        [Ok(ok1), Ok(ok2), Ok(raw3), Err(malformed()), Ok(jp)],
        [Ok(ok1), Ok(ok2), Ok(ok1), Ok(cont4), Ok(jp)],
        [Ok(ok1), Ok(ok2), Ok(raw3), Ok(cont4), Ok(jp)],
    ];
    for (mode, row) in expected.iter().enumerate() {
        for (case, want) in cases.iter().zip(row) {
            let got = dec_str(case, mode as i64);
            assert_eq!(
                got.as_deref().map_err(Clone::clone),
                want.clone(),
                "mode {mode} {case:?}"
            );
        }
    }
    // The charset-length checks.
    let mut d = Vec::new();
    assert_eq!(mime_decode(b"a", 0, &[b'/'; 100], &mut d), None);
    assert_eq!(
        d,
        [Diag::Warning(
            "Encoding parameter exceeds the maximum allowed length of 64 characters".into()
        )]
    );
}

#[test]
fn phpt_mime_decode_headers() {
    // iconv_mime_decode_headers.phpt
    let h = "Return-Path: <internals-return-5651-***=***.example.com@lists.php.net>\nReceived: from pb1.pair.com (pb1.pair.com [16.92.131.4]) by ***.example.com\n    (8.12.10/8.12.10/1970-09-30) with SMTP id hALLmpea023899 for\n    <***@***.example.com>; Sat, 22 Jan 1970 06:48:51 +0900 (JST)\n    (envelope-from\n    internals-return-5651-***=***.example.com@lists.php.net)\nReceived: (qmail 63472 invoked by uid 1010); 1 Jan 1970 0:00:00 -0000\nMailing-List: contact internals-help@lists.php.net; run by ezmlm\nPrecedence: bulk\nList-Help: <mailto:internals-help@lists.php.net>\nList-Unsubscribe: <mailto:internals-unsubscribe@lists.php.net>\nList-Post: <mailto:internals@lists.php.net>\nDelivered-To: mailing list internals@lists.php.net\nReceived: (qmail 63459 invoked by uid 1010); 1 Jan 1970 0:00:00 -0000\nDelivered-To: ezmlm-scan-internals@lists.php.net\nDelivered-To: ezmlm-internals@lists.php.net\nDate: Thu, 1 Jan 1970 00:00:00 -0000 (GMT)\nFrom: *** *** *** <***@***.example.com>\nX-X-Sender: ***@***.example.com\nTo: internals@lists.php.net\nMessage-Id: <Pine.LNX.4.58.************@***.example.com>\nMIME-Version: 1.0\nContent-Type: TEXT/PLAIN; charset=US-ASCII\nSubject: [PHP-DEV] [ICONV] test for =?US-ASCII?Q?iconv_mime_decode_headers=28=29?=\nX-UIDL: @eH!!h2:!!EOS!!A_c\"!";
    let got = headers(h, 0);
    assert_eq!(got.len(), 17);
    assert_eq!(
        got[0],
        (
            "Return-Path".into(),
            vec!["<internals-return-5651-***=***.example.com@lists.php.net>".into()]
        )
    );
    assert_eq!(got[1].0, "Received");
    assert_eq!(got[1].1.len(), 3);
    assert_eq!(
        got[1].1[0],
        "from pb1.pair.com (pb1.pair.com [16.92.131.4]) by ***.example.com (8.12.10/8.12.10/1970-09-30) with SMTP id hALLmpea023899 for <***@***.example.com>; Sat, 22 Jan 1970 06:48:51 +0900 (JST) (envelope-from internals-return-5651-***=***.example.com@lists.php.net)"
    );
    assert_eq!(got[1].1[0].len(), 259);
    let delivered = got.iter().find(|(n, _)| n == "Delivered-To").unwrap();
    assert_eq!(delivered.1.len(), 3);
    assert_eq!(
        got[15],
        (
            "Subject".into(),
            vec!["[PHP-DEV] [ICONV] test for iconv mime decode headers()".into()]
        )
    );
    assert_eq!(
        got[16],
        ("X-UIDL".into(), vec!["@eH!!h2:!!EOS!!A_c\"!".into()])
    );
    // bug52941.phpt
    let h = "From: =?UTF-8?B?PGZvb0BleGFtcGxlLmNvbT4=?=\nSubject: =?ks_c_5601-1987?B?UkU6odk=?=\nX-Foo: =?ks_c_5601-1987?B?UkU6odk=?= Foo\nX-Bar: =?ks_c_5601-1987?B?UkU6odk=?= =?UTF-8?Q?Foo?=\nTo: <test@example.com>";
    for mode in [CONT, CONT | STRICT] {
        let got = headers(h, mode);
        assert_eq!(got[0].1, ["<foo@example.com>"]);
        // glibc doesn't know ks_c_5601-1987: the words stay as they are
        // (the test expects "%s", "%sFoo").
        assert_eq!(got[1].1, ["=?ks_c_5601-1987?B?UkU6odk=?="]);
        assert_eq!(got[2].1, ["=?ks_c_5601-1987?B?UkU6odk=?= Foo"]);
        assert_eq!(got[3].1, ["=?ks_c_5601-1987?B?UkU6odk=?=Foo"]);
        assert_eq!(got[4].1, ["<test@example.com>"]);
    }
    // bug55146.phpt
    let got = headers(
        "X-Header-One: H4sIAAAAAAAAA+NgFlsCAAA=\nX-Header-Two: XtLePq6GTMn8G68F0",
        CONT,
    );
    assert_eq!(got[0].1, ["H4sIAAAAAAAAA+NgFlsCAAA="]);
    assert_eq!(got[1].1, ["XtLePq6GTMn8G68F0"]);
    let got = headers("X-Header-One: =\nX-Header-Two: XtLePq6GTMn8G68F0", STRICT);
    assert_eq!(got[0].1, ["="]);
    assert_eq!(got[1].1, ["XtLePq6GTMn8G68F0"]);
    // bug63839.phpt
    let h = "From: \"xyz\" <xyz@xyz.com>\nTo: <xyz@xyz.com>\nSubject: Reply Is? white side-LED =? in Help\nDate: Sat, 22 Dec 2012\nMessage-ID: <006f01cde00e$d9f79da0$8de6d8e0>\nMIME-Version: 1.0\nContent-Type: multipart/alternative;\n    boundary=\"----=_NextPart_000_0070_01CDE03C.F3AFD9A0\"\nX-Mailer: Microsoft Office Outlook 12.0\nThread-Index: Ac3gDtcH2huHjzYcQVmFJPPoWjJogA==\nContent-Language: en-us\n\n";
    for mode in [CONT, STRICT] {
        let got = headers(h, mode);
        assert_eq!(got.len(), 10, "{mode}");
        assert_eq!(got[2].1, ["Reply Is? white side-LED =? in Help"]);
        assert_eq!(
            got[6].1,
            ["multipart/alternative; boundary=\"----=_NextPart_000_0070_01CDE03C.F3AFD9A0\""]
        );
        assert_eq!(got[8].1, ["Ac3gDtcH2huHjzYcQVmFJPPoWjJogA=="]);
    }
    // bug77147.phpt
    let mut h = b"Feedback-ID: 014a93e3-1f5e-4df6-b347-6b59f0f746b8:b5891053-55d1-45bc-a0b5-47a7a9b59687:email:epslh1".to_vec();
    h.extend_from_slice("\u{fffd}".as_bytes());
    let mut d = Vec::new();
    let got = mime_decode_headers(&h, CONT, b"UTF-8", &mut d).unwrap();
    assert_eq!(got[0].1[0].len(), 86);
    // bug78069.phpt
    let data: &[u8] = b"SuLt;  0Jpaa\0\0\r\r\r\r\r\r\r\r\r\rect: =?is\r\r\r\r\r\r\r\r\r\r\r\r\r\r\r\rect: =?isuLt;  0Jpaa\0\0\r\r\r\r\r\r\r\r\r\r\rect: =?o-8859-1?q?<3F3F=3F?=\xa2";
    let mut d = Vec::new();
    assert_eq!(
        mime_decode_headers(data, CONT, b"UTF-8", &mut d)
            .unwrap()
            .len(),
        1
    );
}

fn hdr_str(s: &str, mode: i64) -> String {
    let mut d = Vec::new();
    match mime_decode_headers(s.as_bytes(), mode, b"UTF-8", &mut d) {
        None => format!("false {d:?}"),
        Some(h) => h
            .iter()
            .map(|(k, v)| {
                let vals: Vec<String> = v
                    .iter()
                    .map(|x| String::from_utf8_lossy(x).into_owned())
                    .collect();
                format!("{}={}", String::from_utf8_lossy(k), vals.join("|"))
            })
            .collect::<Vec<_>>()
            .join(";"),
    }
}

/// Observed on PHP 8.5: header splitting.
#[test]
fn observed_mime_headers() {
    for (s, m, want) in [
        ("A: 1\nB: 2", 0, "A=1;B=2"),
        ("A: 1\r\nB: 2\r\n", 0, "A=1;B=2"),
        ("A: 1\n\nB: 2", 0, "A=1"),
        ("A:1", 0, "A=1"),
        ("A:  1  ", 0, "A=1"),
        ("A :1", 0, "A =1"),
        (" A: 1", 0, " A=1"),
        ("A: 1\n 2\nB: 3", 0, "A=1 2;B=3"),
        ("A: 1\n\t2", 0, "A=1 2"),
        ("noheader", 0, ""),
        ("noheader\nA: 1", 0, "A=1"),
        ("A: 1\nnocolon\nB: 2", 0, "A=1;B=2"),
        ("A: 1\nA: 2\nA: 3", 0, "A=1|2|3"),
        ("A: 1\na: 2", 0, "A=1;a=2"),
        ("A:", 0, "A="),
        ("A:\nB: 2", 0, "A=;B=2"),
        (": x", 0, "=x"),
        ("A: x: y", 0, "A=x: y"),
        ("A: 1\r", 2, "A=1"),
        ("A: =?bogus?B?YQ==?=\nB: 2", 2, "A==?bogus?B?YQ==?=;B=2"),
        ("A: =\nB: 2", 0, "A==;B=2"),
        ("A: =?UTF-8?B?w6?=\nB: 2", 2, "A==?UTF-8?B?w6?=;B=2"),
        ("A: é\nB: 2", 1, "A=;B=2"),
        ("\nA: 1", 0, ""),
        ("A: 1\n \n", 0, "A=1 "),
        ("A: a\n =?UTF-8?B?w6k=?=", 0, "A=a é"),
        ("A: =?UTF-8?B?w6k=?=\n =?UTF-8?B?w6k=?=", 0, "A=éé"),
        ("X-A: 1\nx-a: 2\nX-A: 3", 0, "X-A=1|3;x-a=2"),
        ("A: 1\rB: 2", 0, "A=1\rB: 2"),
        ("A: 1\nB", 0, "A=1"),
        ("A: =?\nB: 2", 0, "A==?;B=2"),
        ("A: =?UTF-8?B?w6k=\nB: 2", 2, "A="),
    ] {
        assert_eq!(hdr_str(s, m), want, "{s:?} mode {m}");
    }
    assert!(hdr_str("A: 1\r", 0).starts_with("false"));
    assert!(hdr_str("A: é\nB: 2", 0).starts_with("false"));
}

#[test]
fn observed_mime_decode() {
    for (s, m, want) in [
        ("plain text", 0, Ok("plain text")),
        ("trail  ", 0, Ok("trail")),
        ("a  b", 0, Ok("a  b")),
        ("a\r\n b", 0, Ok("a b")),
        ("a \r\n  b", 0, Ok("a b")),
        ("a\nb", 0, Ok("a")),
        ("abc\r\n ", 0, Ok("abc ")),
        ("x=?UTF-8?B?w6k=?=y", 0, Ok("xéy")),
        ("x=?UTF-8?B?w6k=?=y", STRICT, Ok("x=?UTF-8?B?w6k=?=y")),
        ("=?UTF-8?B?w6k=?=\r\n =?UTF-8?B?w6k=?=", 0, Ok("éé")),
        ("=?UTF-8?Q?a_b?= =?UTF-8?Q?c?=", 0, Ok("a bc")),
        ("=?UTF-8?Q?a?= b =?UTF-8?Q?c?=", 0, Ok("a b c")),
        ("=??B?w6k=?=", 0, Ok("é")),
        ("=?UTF-8*?Q?a?=", 0, Ok("a")),
        ("=?UTF-8?B?w!6k=?=", 0, Ok("é")),
        ("=?UTF-8?Q?a=?=", 0, Ok("a")),
        ("=", 0, Err(malformed())),
        ("a=", STRICT, Ok("a=")),
        ("a =", STRICT, Err(malformed())),
        ("a= ", 0, Ok("a= ")),
        ("=?UTF-8?X?w6k=?=", 0, Err(malformed())),
        ("=?UTF-8?X?w6k=?=", CONT, Ok("=?UTF-8?X?w6k")),
        ("=?UTF-8??w6k=?=", CONT, Ok("=?UTF-8??w6k")),
        ("=?UTF-8?B?w6?=", CONT, Ok("=?UTF-8?B?w6?")),
        ("=?UTF-8?B?w6?= y", CONT, Ok("=?UTF-8?B?w6?= y")),
        (
            "=?UTF-8?Q?=G?=",
            0,
            Err(Diag::Notice("Unknown error (0)".into())),
        ),
        ("=?UTF-8?Q?=G?=", CONT, Ok("=?UTF-8?Q?=G?=")),
        (
            "=?bogus?B?w6k=?=",
            0,
            Err(Diag::Warning(
                "Wrong encoding, conversion from \"???\" to \"UTF-8\" is not allowed".into(),
            )),
        ),
        ("=?bogus?B?w6k=?=", CONT, Ok("=?bogus?B?w6k=?=")),
        ("=?a?", CONT, Ok("=?a?")),
        (
            "=?UTF-8?B?w6k=?==?UTF-8?Q?=FF?=",
            CONT,
            Ok("é=?UTF-8?B?w6k=?==?UTF-8?Q?=FF?"),
        ),
        (
            "=?UTF-8?B?w6k=?= =?UTF-8?Q?=FF?= y",
            CONT,
            Ok("é=?UTF-8?Q?=FF?= y"),
        ),
        (
            "=?UTF-8?Q?=G?==?UTF-8?B?w6k=?=",
            CONT,
            Ok("=?UTF-8?Q?=G?==?UTF-8?B?w6k"),
        ),
        ("=?UTF-8?B?w6k=?==a", 0, Ok("é=?UTF-8?B?w6k=?==a")),
    ] {
        let got = dec_str(s, m);
        assert_eq!(got.as_deref().map_err(Clone::clone), want, "{s:?} mode {m}");
    }
    // Text outside words is converted from ASCII, words with a fresh
    // converter each.
    assert_eq!(
        hex(&dec(b"a=?UTF-8?B?w6k=?=b", 0, "UTF-16").unwrap()),
        "fffe6100fffee9006200"
    );
    assert_eq!(
        hex(&dec(b"=?UTF-8?B?4oKs?=", 0, "ISO-8859-1//TRANSLIT").unwrap()),
        "455552"
    );
    assert_eq!(
        dec(b"=?UTF-8?B?4oKs?=", 0, "ISO-8859-1//IGNORE"),
        Err(illegal())
    );
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn phpt_mime_encode() {
    use MimeScheme::{B, Q};
    // bug48289.phpt
    assert_eq!(
        enc(
            "Subject",
            "テストテスト".as_bytes(),
            Q,
            "UTF-8",
            "UTF-8",
            30,
            "\r\n"
        )
        .unwrap(),
        "Subject: =?UTF-8?Q?=E3=83=86?=\r\n =?UTF-8?Q?=E3=82=B9?=\r\n =?UTF-8?Q?=E3=83=88?=\r\n =?UTF-8?Q?=E3=83=86?=\r\n =?UTF-8?Q?=E3=82=B9?=\r\n =?UTF-8?Q?=E3=83=88?="
    );
    // bug53891.phpt
    assert_eq!(
        enc(
            "subject",
            "d obeybiubrsfqllpdtpge…".as_bytes(),
            Q,
            "utf-8",
            "utf-8",
            74,
            "\r\n"
        )
        .unwrap(),
        "subject: =?utf-8?Q?d=20obeybiubrsfqllpdtpge=E2=80=A6?="
    );
    // bug66828.phpt
    assert_eq!(
        enc(
            "Subject",
            b"Test Test Test Test Test Test Test Test",
            Q,
            "ISO-8859-1",
            "UTF-8",
            76,
            "\n"
        )
        .unwrap(),
        "Subject: =?UTF-8?Q?Test=20Test=20Test=20Test=20Test=20Test=20Test=20Test?="
    );
    // iconv004.phpt
    assert_eq!(
        enc("", b"", B, "ISO-8859-1", "ISO-8859-1", 76, "\r\n").unwrap(),
        ": =?ISO-8859-1?B??="
    );
    assert_eq!(
        enc("", b"", B, "ISO-8859-1", "ISO-8859-1", 76, "1").unwrap(),
        ": =?ISO-8859-1?B??="
    );
    // iconv_mime_encode.phpt: EUC-JP input, ISO-2022-JP output.
    let euc = [
        0xa5, 0xb5, 0xa5, 0xf3, 0xa5, 0xd7, 0xa5, 0xeb, 0xca, 0xb8, 0xbb, 0xfa, 0xce, 0xf3, 0xa5,
        0xb5, 0xa5, 0xf3, 0xa5, 0xd7, 0xa5, 0xeb, 0xca, 0xb8, 0xbb, 0xfa, 0xce, 0xf3, 0xc6, 0xfc,
        0xcb, 0xdc, 0xb8, 0xec, 0xa5, 0xc6, 0xa5, 0xad, 0xa5, 0xb9, 0xa5, 0xc8,
    ];
    let lens = |r: &str| r.split('\n').map(str::len).max().unwrap();
    for ll in 0..36 {
        assert!(
            enc("From", &euc, B, "EUC-JP", "ISO-2022-JP", ll, "\n").is_err(),
            "{ll}"
        );
    }
    let r36 = enc("From", &euc, B, "EUC-JP", "ISO-2022-JP", 36, "\n").unwrap();
    assert_eq!(r36.len(), 396);
    assert!(r36.starts_with(
        "From: =?ISO-2022-JP?B?GyRCJTUbKEI=?=\n =?ISO-2022-JP?B?GyRCJXMlVxsoQg==?=\n"
    ));
    for (ll, len, first) in [
        (39, 236, "From: =?ISO-2022-JP?B?GyRCJTUbKEI=?="),
        (40, 236, "From: =?ISO-2022-JP?B?GyRCJTUlcxsoQg==?="),
        (43, 212, "From: =?ISO-2022-JP?B?GyRCJTUlcxsoQg==?="),
        (44, 212, "From: =?ISO-2022-JP?B?GyRCJTUlcyVXJWsbKEI=?="),
        (47, 176, "From: =?ISO-2022-JP?B?GyRCJTUlcyVXJWsbKEI=?="),
        (48, 180, "From: =?ISO-2022-JP?B?GyRCJTUlcyVXJWtKOBsoQg==?="),
        (51, 152, "From: =?ISO-2022-JP?B?GyRCJTUlcyVXJWtKOBsoQg==?="),
        (
            52,
            148,
            "From: =?ISO-2022-JP?B?GyRCJTUlcyVXJWtKODt6TnMbKEI=?=",
        ),
        (
            60,
            120,
            "From: =?ISO-2022-JP?B?GyRCJTUlcyVXJWtKODt6TnMlNSVzJVcbKEI=?=",
        ),
        (
            76,
            120,
            "From: =?ISO-2022-JP?B?GyRCJTUlcyVXJWtKODt6TnMlNSVzJVcla0o4O3pOc0Z8S1wbKEI=?=",
        ),
    ] {
        let r = enc("From", &euc, B, "EUC-JP", "ISO-2022-JP", ll, "\n").unwrap();
        assert_eq!(r.len(), len, "{ll}");
        assert!(r.starts_with(&format!("{first}\n")), "{ll}: {r}");
        assert!(lens(&r) <= ll as usize);
    }
    for ll in 36..80 {
        let r = enc("From", &euc, B, "EUC-JP", "ISO-2022-JP", ll, "\n").unwrap();
        assert!(lens(&r) <= ll as usize, "{ll}");
    }
    // iconv_mime_encode_001.phpt
    let long = format!("ISO-8859-1{}", "x".repeat(256));
    let e = enc("Subject", "Prüfung".as_bytes(), B, &long, "UTF-8", 76, "\n").unwrap_err();
    assert_eq!(
        e,
        Diag::Warning(
            "Encoding parameter exceeds the maximum allowed length of 64 characters".into()
        )
    );
}

#[test]
fn observed_mime_encode() {
    use MimeScheme::{B, Q};
    let all: Vec<u8> = (32u8..127).collect();
    assert_eq!(
        enc("Subject", &all, Q, "UTF-8", "UTF-8", 1000, "\r\n").unwrap(),
        "Subject: =?UTF-8?Q?=20!\"#$%&'()*+,-./0123456789:;<=3D>=3F@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^=5F`abcdefghijklmnopqrstuvwxyz{|}~?="
    );
    let buf = Diag::Warning("Buffer length exceeded".into());
    for (ll, want) in [
        (20, Err(buf.clone())),
        (21, Ok("Subject: \r\n =?UTF-8?B?w6k=?=")),
        (27, Ok("Subject: \r\n =?UTF-8?B?w6k=?=")),
        (28, Err(buf.clone())),
        (29, Ok("Subject: =?UTF-8?B?w6k=?=")),
    ] {
        assert_eq!(
            enc("Subject", "é".as_bytes(), B, "UTF-8", "UTF-8", ll, "\r\n"),
            want.map(String::from),
            "{ll}"
        );
    }
    for (ll, want) in [
        (17, Err(buf.clone())),
        (18, Err(Diag::Notice("Unknown error (7)".into()))),
        (20, Ok("Subject: \r\n =?UTF-8?Q?=C3=A9?=")),
        (27, Ok("Subject: =?UTF-8?Q?=C3=A9?=")),
    ] {
        assert_eq!(
            enc("Subject", "é".as_bytes(), Q, "UTF-8", "UTF-8", ll, "\r\n"),
            want.map(String::from),
            "{ll}"
        );
    }
    assert_eq!(
        enc(
            "Subject",
            &"é".repeat(40).into_bytes(),
            B,
            "UTF-8",
            "UTF-8",
            40,
            "\n"
        )
        .unwrap(),
        "Subject: =?UTF-8?B?w6nDqcOpw6k=?=\n =?UTF-8?B?w6nDqcOpw6nDqcOpw6k=?=\n =?UTF-8?B?w6nDqcOpw6nDqcOpw6k=?=\n =?UTF-8?B?w6nDqcOpw6nDqcOpw6k=?=\n =?UTF-8?B?w6nDqcOpw6nDqcOpw6k=?=\n =?UTF-8?B?w6nDqcOpw6nDqcOpw6k=?=\n =?UTF-8?B?w6k=?="
    );
    assert_eq!(
        enc("Subject", b"ab", B, "UTF-8", "UTF-16", 76, "\n").unwrap(),
        "Subject: =?UTF-16?B?//5hAGIA?="
    );
    assert_eq!(
        enc(
            "Subject",
            "a€".as_bytes(),
            B,
            "UTF-8",
            "ISO-8859-1",
            76,
            "\n"
        ),
        Err(illegal())
    );
    assert_eq!(
        enc(
            "Subject",
            "a€".as_bytes(),
            B,
            "UTF-8",
            "ISO-8859-1//TRANSLIT",
            76,
            "\n"
        )
        .unwrap(),
        "Subject: =?ISO-8859-1//TRANSLIT?B?YUVVUg==?="
    );
}

fn fmt_result(r: Result<Vec<u8>, Diag>) -> String {
    match r {
        Ok(v) => format!("S:{}", String::from_utf8_lossy(&v)),
        Err(Diag::Warning(m)) => format!("F:2:{}", m.replace(' ', "_")),
        Err(Diag::Notice(m)) => format!("F:8:{}", m.replace(' ', "_")),
        Err(d) => format!("{d:?}"),
    }
}

fn fmt_want(w: &str) -> String {
    match w.strip_prefix("S:") {
        Some(b) => format!(
            "S:{}",
            String::from_utf8_lossy(&mime::b64_decode(b.as_bytes()))
        ),
        None => w.to_string(),
    }
}

/// Replays iconv_mime_decode() cases (tools/charset/iconv_obs_mime.php
/// format); returns the mismatches.
fn decode_corpus(text: &str, clean_only: bool) -> (usize, Vec<String>) {
    let mut ok = 0;
    let mut bad = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let mut parts = line.split(' ');
        let input = mime::b64_decode(parts.next().unwrap().as_bytes());
        // Inputs with lone CRs or 8-bit bytes exercise quirks that are not
        // reproduced exactly.
        let quirky = input.iter().any(|&b| b >= 0x80)
            || input.windows(2).any(|w| w[0] == b'\r' && w[1] != b'\n')
            || input.last() == Some(&b'\r');
        if clean_only && quirky {
            continue;
        }
        for (m, want) in parts.enumerate() {
            let got = fmt_result(dec(&input, m as i64, "UTF-8"));
            let want = fmt_want(want);
            if got == want {
                ok += 1;
            } else {
                bad.push(format!(
                    "m{m} {:?}\n   want {want:?}\n   got  {got:?}",
                    String::from_utf8_lossy(&input)
                ));
            }
        }
    }
    (ok, bad)
}

/// Replays iconv_mime_encode() cases (tools/charset/iconv_obs_mime_encode.php
/// format); returns the mismatches.
fn encode_corpus(text: &str) -> (usize, Vec<String>) {
    let mut ok = 0;
    let mut bad = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let p: Vec<&str> = line.split(' ').collect();
        let b = |s: &str| mime::b64_decode(s.as_bytes());
        let name = String::from_utf8(b(p[0])).unwrap();
        let value = b(p[1]);
        let scheme = if p[2] == "B" {
            MimeScheme::B
        } else {
            MimeScheme::Q
        };
        let ll: i64 = p[4].parse().unwrap();
        let lf = String::from_utf8(b(p[5])).unwrap();
        let got =
            fmt_result(enc(&name, &value, scheme, "UTF-8", p[3], ll, &lf).map(String::into_bytes));
        let want = fmt_want(p[6]);
        if got == want {
            ok += 1;
        } else {
            bad.push(format!(
                "{name} {:?} {} {} {ll} {lf:?}\n  want {want:?}\n  got  {got:?}",
                String::from_utf8_lossy(&value),
                p[2],
                p[3]
            ));
        }
    }
    (ok, bad)
}

/// iconv_mime_decode() on a sample of the observed cases (about 3200
/// decodings). Some CONTINUE_ON_ERROR recoveries, "Unknown error" errno
/// values and lone CR / 8-bit text quirks are not reproduced: 22 of the
/// cases without lone CRs and 8-bit bytes differ, 196 of all.
#[test]
fn fixture_mime_decode() {
    let text = include_str!("../../tests/fixtures/iconv_mime_decode.txt");
    let (ok, bad) = decode_corpus(text, true);
    assert!(ok > 2000);
    assert!(
        bad.len() <= KNOWN_DECODE_CLEAN,
        "{} mismatches:\n{}",
        bad.len(),
        bad.join("\n")
    );
    let (_, bad) = decode_corpus(text, false);
    assert!(bad.len() <= KNOWN_DECODE_ALL, "{} mismatches", bad.len());
}

/// Mismatches in the committed sample (CONTINUE_ON_ERROR recovery corners
/// and lone CR / 8-bit text quirks); a regression raises them.
const KNOWN_DECODE_CLEAN: usize = 22;
const KNOWN_DECODE_ALL: usize = 196;

/// iconv_mime_encode() on a sample of the observed cases. The ones that
/// differ use Q with a stateful charset (ISO-2022-JP, UTF-7), where PHP
/// splits one converter's output across words.
#[test]
fn fixture_mime_encode() {
    let text = include_str!("../../tests/fixtures/iconv_mime_encode.txt");
    let (ok, bad) = encode_corpus(text);
    assert!(ok > 500);
    let unexpected: Vec<&String> = bad
        .iter()
        .filter(|b| !(b.contains(" Q ISO-2022-JP ") || b.contains(" Q UTF-7 ")))
        .collect();
    assert!(unexpected.is_empty(), "{unexpected:?}");
}

/// Development aid: the full corpora, when present in the scratch directory.
#[test]
#[ignore]
fn mime_corpora() {
    let dir = "/tmp/claude-1000/goro-work/charset-iconv/obs/";
    if let Ok(text) = std::fs::read_to_string(format!("{dir}mime_decode2.txt")) {
        let clean = std::env::var("MIME_CLEAN").is_ok();
        let (ok, bad) = decode_corpus(&text, clean);
        for b in bad.iter().take(80) {
            eprintln!("{b}");
        }
        eprintln!("decode: ok {ok} bad {}", bad.len());
    }
    if let Ok(text) = std::fs::read_to_string(format!("{dir}mime_encode2.txt")) {
        let (ok, bad) = encode_corpus(&text);
        for b in bad.iter().take(30) {
            eprintln!("{b}");
        }
        eprintln!("encode: ok {ok} bad {}", bad.len());
    }
}
