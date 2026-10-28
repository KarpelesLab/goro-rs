//! Cases ported from the expected output of php-src's .phpt tests (named
//! per test); the broad comparison with PHP 8.5 is tests/html_fixture.rs.

use super::*;

fn hsc(s: &str, flags: i64) -> String {
    String::from_utf8(htmlspecialchars(s.as_bytes(), flags, Charset::Utf8, true)).unwrap()
}

fn he(s: &[u8], flags: i64, cs: Charset) -> Vec<u8> {
    htmlentities(s, flags, cs, true, &mut Vec::new())
}

fn st(s: &str, allowed: Option<&str>) -> String {
    let al = match allowed {
        Some(a) => AllowedTags::Str(a.as_bytes()),
        None => AllowedTags::None,
    };
    String::from_utf8(strip_tags(s.as_bytes(), al)).unwrap()
}

#[test]
fn htmlspecialchars_manual_examples() {
    // PHP manual, htmlspecialchars() examples #1 and #2.
    assert_eq!(
        hsc("<a href='test'>Test</a>", ENT_QUOTES),
        "&lt;a href=&#039;test&#039;&gt;Test&lt;/a&gt;"
    );
    assert_eq!(
        htmlspecialchars(b"atta\x80ck", ENT_QUOTES | ENT_IGNORE, Charset::Utf8, true),
        b"attack"
    );
    assert_eq!(
        htmlspecialchars(
            b"atta\x80ck",
            ENT_QUOTES | ENT_SUBSTITUTE,
            Charset::Utf8,
            true
        ),
        b"atta\xef\xbf\xbdck"
    );
    // Invalid input without ENT_IGNORE/ENT_SUBSTITUTE gives "".
    assert_eq!(
        htmlspecialchars(b"atta\x80ck", ENT_QUOTES, Charset::Utf8, true),
        b""
    );
}

#[test]
fn htmlentities24_quotes() {
    // test htmlentities24.phpt
    let s = "A 'quote' is <b>bold</b>";
    let f = |flags| String::from_utf8(he(s.as_bytes(), flags, Charset::Utf8)).unwrap();
    assert_eq!(
        f(DEFAULT_FLAGS),
        "A &#039;quote&#039; is &lt;b&gt;bold&lt;/b&gt;"
    );
    assert_eq!(f(ENT_NOQUOTES), "A 'quote' is &lt;b&gt;bold&lt;/b&gt;");
    assert_eq!(f(ENT_COMPAT), "A 'quote' is &lt;b&gt;bold&lt;/b&gt;");
    assert_eq!(
        he(b"\x82\x86\x99\x9f\x80\x82\x86\x84\x80\x89\x85\x83\x86\x84\x80\x91\x83\x91\x86\x87\x85\x86\x88\x82\x89\x92\x91\x83", ENT_QUOTES, Charset::Cp1252),
        b"&sbquo;&dagger;&trade;&Yuml;&euro;&sbquo;&dagger;&bdquo;&euro;&permil;&hellip;&fnof;&dagger;&bdquo;&euro;&lsquo;&fnof;&lsquo;&dagger;&Dagger;&hellip;&dagger;&circ;&sbquo;&permil;&rsquo;&lsquo;&fnof;"
    );
}

#[test]
fn htmlentities21_disallowed_single_byte() {
    // test htmlentities21.phpt (Windows-1251, ENT_DISALLOWED)
    let sub = b"&#xFFFD;".to_vec();
    let a = |b: u8, d: i64| he(&[b], d | ENT_DISALLOWED, Charset::Cp1251);
    let b = |b: u8, d: i64| htmlspecialchars(&[b], d | ENT_DISALLOWED, Charset::Cp1251, true);
    assert_eq!(a(0x00, ENT_HTML401), sub);
    assert_eq!(a(0x7F, ENT_HTML401), sub);
    assert_eq!(b(0x7F, ENT_HTML401), b"\x7f");
    assert_eq!(a(0xA0, ENT_HTML401), b"&nbsp;");
    assert_eq!(a(0x0C, ENT_HTML5), b"\x0c");
    assert_eq!(a(0x09, ENT_HTML5), b"&Tab;");
    assert_eq!(a(0x0A, ENT_HTML5), b"&NewLine;");
    assert_eq!(a(0x80, ENT_HTML5), b"&DJcy;");
    assert_eq!(a(0x9F, ENT_HTML5), b"&dzcy;");
    assert_eq!(a(0x7F, ENT_XHTML), b"\x7f");
    assert_eq!(a(0xA0, ENT_XML1), b"\xa0");
}

#[test]
fn htmlentities22_numeric_entities() {
    // test htmlentities22.phpt (double_encode = false with ENT_DISALLOWED)
    let check = |t: &str, d: i64, changed: bool| {
        let a = htmlentities(
            t.as_bytes(),
            d | ENT_DISALLOWED,
            Charset::Utf8,
            false,
            &mut Vec::new(),
        );
        assert_eq!(a != t.as_bytes(), changed, "{t} {d}");
    };
    for (t, h4, x, h5) in [
        ("&#0;", false, true, true),
        ("&#x09;", false, false, false),
        ("&#x0C;", false, true, false),
        ("&#x0D;", false, false, true),
        ("&#x7F;", false, false, true),
        ("&#x80;", false, false, true),
        ("&#xD800;", false, true, false),
        ("&#xFFFE;", false, true, true),
        ("&#xFDD0;", false, false, true),
        ("&#x2FFFF;", false, false, true),
        ("&#x110000;", true, true, true),
    ] {
        check(t, ENT_HTML401, h4);
        check(t, ENT_XHTML, x);
        check(t, ENT_XML1, x);
        check(t, ENT_HTML5, h5);
    }
    assert_eq!(
        htmlentities(
            b"&#0;",
            ENT_HTML5 | ENT_DISALLOWED,
            Charset::Utf8,
            false,
            &mut Vec::new()
        ),
        b"&amp;&num;0&semi;"
    );
}

#[test]
fn htmlentities23_euc_jp_invalid() {
    // test htmlentities23.phpt (EUC-JP, ENT_SUBSTITUTE)
    let f =
        |s: &[u8]| String::from_utf8(he(s, ENT_QUOTES | ENT_SUBSTITUTE, Charset::EucJp)).unwrap();
    let r = "&#xFFFD;";
    assert_eq!(f(b"\x8F\xA1\xFF"), r.repeat(2));
    assert_eq!(f(b"\x8F\xA1"), r.repeat(2));
    assert_eq!(f(b"\x8F"), r);
    assert_eq!(f(b"\x8F\xA0"), r);
    assert_eq!(f(b"\x8F\xA1\x21"), r.repeat(2) + "!");
    assert_eq!(f(b"\x8F\x21"), r.to_owned() + "!");
    assert_eq!(
        he(b"\x8E\xAE", ENT_QUOTES | ENT_SUBSTITUTE, Charset::EucJp),
        b"\x8E\xAE"
    );
    assert_eq!(f(b"\xB2\xFF"), r);
    assert_eq!(f(b"\xB2\x21"), r.to_owned() + "!");
    assert_eq!(f(b"\xA0"), r);
}

#[test]
fn htmlentities_notice_for_cjk() {
    let mut d = Vec::new();
    htmlentities(b"x", ENT_QUOTES, Charset::ShiftJis, true, &mut d);
    assert_eq!(
        d,
        [Diag::Notice(
            "Only basic entities substitution is supported for multi-byte encodings other than UTF-8; \
             functionality is equivalent to htmlspecialchars"
                .into()
        )]
    );
}

#[test]
fn charset_names() {
    // PHP manual list; "utf8" and "1251" are rejected (observed on PHP 8.5).
    let mut d = Vec::new();
    assert_eq!(
        resolve_charset(Some(b"sjis-WIN"), b"", &mut d),
        Charset::ShiftJis
    );
    assert_eq!(resolve_charset(Some(b"koi8r"), b"", &mut d), Charset::Koi8R);
    assert_eq!(resolve_charset(None, b"cp1251", &mut d), Charset::Cp1251);
    assert_eq!(resolve_charset(Some(b""), b"", &mut d), Charset::Utf8);
    assert!(d.is_empty());
    assert_eq!(resolve_charset(Some(b"utf8"), b"", &mut d), Charset::Utf8);
    assert_eq!(
        d,
        [Diag::Warning(
            "Charset \"utf8\" is not supported, assuming UTF-8".into()
        )]
    );
}

#[test]
fn translation_table_counts() {
    // tests get_html_translation_table_basic5/7/8/9.phpt
    let n = |t, f, cs| get_html_translation_table(t, f, cs).len();
    assert_eq!(
        n(HTML_ENTITIES, ENT_COMPAT | ENT_HTML5, Charset::Utf8),
        1510
    );
    assert_eq!(
        n(HTML_ENTITIES, ENT_QUOTES | ENT_HTML5, Charset::Utf8),
        1511
    );
    assert_eq!(
        n(HTML_ENTITIES, ENT_NOQUOTES | ENT_HTML5, Charset::Utf8),
        1509
    );
    assert_eq!(n(HTML_ENTITIES, ENT_QUOTES | ENT_XHTML, Charset::Utf8), 253);
    assert_eq!(n(HTML_ENTITIES, ENT_COMPAT | ENT_XHTML, Charset::Utf8), 252);
    assert_eq!(
        n(HTML_ENTITIES, ENT_NOQUOTES | ENT_XHTML, Charset::Utf8),
        251
    );
    assert_eq!(n(HTML_ENTITIES, ENT_QUOTES | ENT_XML1, Charset::Utf8), 5);
    assert_eq!(
        n(HTML_ENTITIES, ENT_QUOTES | ENT_HTML5, Charset::ShiftJis),
        5
    );
    assert_eq!(n(HTML_SPECIALCHARS, ENT_COMPAT, Charset::Utf8), 4);
    let t = get_html_translation_table(HTML_SPECIALCHARS, ENT_QUOTES | ENT_XML1, Charset::Utf8);
    assert!(t.contains(&(b"'".to_vec(), b"&apos;".to_vec())));
    let t = get_html_translation_table(HTML_ENTITIES, ENT_QUOTES | ENT_XHTML, Charset::Utf8);
    assert!(t.contains(&(b"'".to_vec(), b"&#039;".to_vec())));
}

#[test]
fn htmlspecialchars_decode_variation7() {
    // test htmlspecialchars_decode_variation7.phpt
    let tests = [
        "&quot;", "&#x22;", "&#34;", "&apos;", "&#39;", "&#x27;", "&amp;", "&#x26;", "&lt;",
        "&gt;", "&#x3C;", "&#60;", "&#x3E;", "&#62;", "&#63;",
    ];
    let decoded = |flags: i64| -> Vec<&str> {
        tests
            .iter()
            .copied()
            .filter(|t| htmlspecialchars_decode(t.as_bytes(), flags) != t.as_bytes())
            .collect()
    };
    let all_but_apos: Vec<&str> = tests
        .iter()
        .copied()
        .filter(|&t| t != "&apos;" && t != "&#63;")
        .collect();
    assert_eq!(decoded(ENT_QUOTES | ENT_HTML401), all_but_apos);
    let with_apos: Vec<&str> = tests.iter().copied().filter(|&t| t != "&#63;").collect();
    assert_eq!(decoded(ENT_QUOTES | ENT_XHTML), with_apos);
    assert_eq!(decoded(ENT_QUOTES | ENT_HTML5), with_apos);
    assert_eq!(decoded(ENT_QUOTES | ENT_XML1), with_apos);
    let noquotes: Vec<&str> = with_apos
        .iter()
        .copied()
        .filter(|t| {
            !t.contains("quot")
                && !t.contains("apos")
                && !["&#x22;", "&#34;", "&#39;", "&#x27;"].contains(t)
        })
        .collect();
    assert_eq!(decoded(ENT_NOQUOTES | ENT_HTML5), noquotes);
}

#[test]
fn html_entity_decode_basics() {
    let d = |s: &str, f: i64, cs: Charset| html_entity_decode(s.as_bytes(), f, cs);
    assert_eq!(
        d("&eacute;&lt;&#233;&#x80;&euro;", ENT_QUOTES, Charset::Utf8),
        "é<é&#x80;€".as_bytes()
    );
    assert_eq!(
        d(
            "&eacute;&lt;&#233;&#x80;&euro;",
            ENT_QUOTES,
            Charset::Iso8859_1
        ),
        b"\xe9<\xe9&#x80;&euro;"
    );
    assert_eq!(d("&euro;", ENT_QUOTES, Charset::Iso8859_15), b"\xa4");
    assert_eq!(d("&euro;", ENT_QUOTES, Charset::MacRoman), b"\xdb");
    assert_eq!(
        d("&NotEqualTilde;&fjlig;", ENT_HTML5, Charset::Utf8),
        "≂̸fj".as_bytes()
    );
    assert_eq!(d("&fjlig;", ENT_HTML5, Charset::Iso8859_1), b"&fjlig;");
    assert_eq!(d("&amp;lt;&amp", ENT_QUOTES, Charset::Utf8), b"&lt;&amp");
    assert_eq!(
        d("&lt;&#65;&eacute;", ENT_QUOTES, Charset::ShiftJis),
        b"<A&eacute;"
    );
}

#[test]
fn strip_tags_phpt() {
    // test strip_tags.phpt
    assert_eq!(st("NEAT <? cool < blah ?> STUFF", None), "NEAT  STUFF");
    assert_eq!(st("NEAT <? cool > blah ?> STUFF", None), "NEAT  STUFF");
    assert_eq!(st("NEAT <!-- cool < blah --> STUFF", None), "NEAT  STUFF");
    assert_eq!(st("NEAT <!-- cool > blah --> STUFF", None), "NEAT  STUFF");
    assert_eq!(st(r#"NEAT <? echo \"\\\"\"?> STUFF"#, None), "NEAT  STUFF");
    assert_eq!(st(r"NEAT <? echo \'\\\'\'?> STUFF", None), "NEAT  STUFF");
    assert_eq!(st("TESTS ?!!?!?!!!?!!", None), "TESTS ?!!?!?!!!?!!");
    // tests bug40637, bug40704, bug40432, bug46578, bug78003, bug78346
    assert_eq!(
        st(r#"<span title="Bug ' Trigger">Text</span>"#, None),
        "Text"
    );
    assert_eq!(
        st("<div>Bug ' Trigger</div> Missing Text", None),
        "Bug ' Trigger Missing Text"
    );
    assert_eq!(st(r#"<span title="test > all">this</span>"#, None), "this");
    assert_eq!(
        st("<!-- testing I've been to mars -->foobar", None),
        "foobar"
    );
    assert_eq!(
        st(r#"<a alt="foobar">foo<!-- foo! --></a>bar"#, None),
        "foobar"
    );
    assert_eq!(
        st(
            r#"<a alt="foobar"/>foo<?= foo! /* <!-- "cool" --> */ ?>bar"#,
            None
        ),
        "foobar"
    );
    assert_eq!(st("< ax", None), "< ax");
    assert_eq!(st("<! a>", None), "");
    assert_eq!(st("<? ax", None), "");
    assert_eq!(st("<foo<>bar>", None), "");
    assert_eq!(st("<foo<!>bar>", None), "");
    assert_eq!(st("<foo<?>bar>", None), "");
    assert_eq!(st("<?= '<?= 1 ?>' ?>2", None), "2");
    // tests bug70720, bug45485
    assert_eq!(
        st("<?php $dom->test(); ?> this is a test", None),
        " this is a test"
    );
    assert_eq!(
        st("<span class=sf-dump-> this is a test</span>", None),
        " this is a test"
    );
    assert_eq!(
        st(
            "This text is shown <?XML:NAMESPACE PREFIX = ST1 /><b>This Text disappears</b>",
            None
        ),
        "This text is shown This Text disappears"
    );
}

#[test]
fn strip_tags_allowed() {
    // tests strip_tags_basic2.phpt, bug53319.phpt, bug78814.phpt, bug23650.phpt
    let s = r##"<html><p>hello</p><b>world</b><a href="#fragment">Other text</a></html><?php echo hello ?>"##;
    assert_eq!(st(s, Some("<html>")), "<html>helloworldOther text</html>");
    assert_eq!(st(s, Some("<p>")), "<p>hello</p>worldOther text");
    assert_eq!(
        st(s, Some("<a>")),
        r##"helloworld<a href="#fragment">Other text</a>"##
    );
    assert_eq!(st(s, Some("<?php")), "helloworldOther text");
    assert_eq!(
        st(s, Some("<html><p><a><?php")),
        r##"<html><p>hello</p>world<a href="#fragment">Other text</a></html>"##
    );
    let s = r#"<br /><br  />USD<input type="text"/><br/>CDN<br><input type="text" />"#;
    assert_eq!(
        st(s, Some("<input>")),
        r#"USD<input type="text"/>CDN<input type="text" />"#
    );
    assert_eq!(st(s, Some("<br><input>")), s);
    assert_eq!(st(s, None), "USDCDN");
    assert_eq!(st("<a/b>", Some("<a>")), "");
    assert_eq!(st("<s/trong>b</strong>", Some("<strong>")), "b</strong>");
    let s = "1:<!-- abc -  -->\n2:<!doctype -- >\n3:\n4:<abc - def>\n5:abc - def\n6:</abc>\n";
    assert_eq!(st(s, None), "1:\n2:\n3:\n4:\n5:abc - def\n6:\n");
    assert_eq!(
        st(s, Some("<abc>")),
        "1:\n2:\n3:\n4:<abc - def>\n5:abc - def\n6:</abc>\n"
    );
}

#[test]
fn strip_tags_array_form() {
    // test strip_tags_array.phpt
    let s = br##"<p>foo <b>bar</b> <a href="#">foobar</a></p>"##;
    let l = |names: &[&[u8]]| String::from_utf8(strip_tags(s, AllowedTags::List(names))).unwrap();
    assert_eq!(l(&[b"a"]), r##"foo bar <a href="#">foobar</a>"##);
    assert_eq!(
        l(&[b"p", b"a"]),
        r##"<p>foo bar <a href="#">foobar</a></p>"##
    );
    assert_eq!(l(&[]), "foo bar foobar");
    assert_eq!(l(&[b"1", b""]), "foo bar foobar");
}
