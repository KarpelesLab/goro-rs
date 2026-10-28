//! Cases ported from the expected output of php-src's .phpt tests (named
//! per test); the broad comparison with PHP 8.5 is tests/url_fixture.rs.

use super::*;

fn u(scheme: Option<&str>, host: Option<&str>, port: Option<i64>, user: Option<&str>) -> Url {
    Url {
        scheme: scheme.map(|s| s.into()),
        host: host.map(|s| s.into()),
        port,
        user: user.map(|s| s.into()),
        ..Url::default()
    }
}

fn with(
    mut url: Url,
    pass: Option<&str>,
    path: Option<&str>,
    query: Option<&str>,
    fragment: Option<&str>,
) -> Url {
    url.pass = pass.map(|s| s.into());
    url.path = path.map(|s| s.into());
    url.query = query.map(|s| s.into());
    url.fragment = fragment.map(|s| s.into());
    url
}

#[test]
fn urlencode_family() {
    // test bug53248.phpt
    assert_eq!(rawurlencode(b"A1_-.~"), b"A1_-.~");
    assert_eq!(rawurldecode(b"%41%31%5F%2D%2E%7E"), b"A1_-.~");
    // PHP manual: urlencode() encodes spaces as "+" and "~".
    assert_eq!(urlencode(b"a b~c"), b"a+b%7Ec");
    assert_eq!(rawurlencode(b"a b~c"), b"a%20b~c");
    assert_eq!(urldecode(b"a+b%7ec%zz%4"), b"a b~c%zz%4");
    assert_eq!(rawurldecode(b"a+b"), b"a+b");
}

/// Walks a list of (key, value) pairs like the wiring layer; `None` values
/// are nested lists.
enum Q<'a> {
    S(&'a str),
    A(Vec<(QueryKey<'a>, Q<'a>)>),
}

fn hbq(items: &[(QueryKey, Q)], prefix: &str, sep: &str, enc: QueryEncoding) -> String {
    fn walk(b: &mut QueryBuilder, parent: Option<&[u8]>, items: &[(QueryKey, Q)], prefix: &str) {
        for (k, v) in items {
            let name = match parent {
                None => b.top_key(*k, prefix.as_bytes()),
                Some(p) => b.child_key(p, *k),
            };
            match v {
                Q::S(s) => b.push(&name, s.as_bytes()),
                Q::A(a) => walk(b, Some(&name), a, prefix),
            }
        }
    }
    let mut b = QueryBuilder::new(sep.as_bytes(), enc);
    walk(&mut b, None, items, prefix);
    String::from_utf8(b.finish()).unwrap()
}

#[test]
fn http_build_query_phpt() {
    use QueryKey::{Int, Str};
    // test http_build_query.phpt (without the float)
    let a = vec![
        (Str(b"foo"), Q::S("bar")),
        (Str(b"baz"), Q::S("1")),
        (Str(b"test"), Q::S("a ' \" ")),
        (Int(0), Q::S("abc")),
        (Str(b"true"), Q::S("1")),
        (Str(b"false"), Q::S("0")),
    ];
    assert_eq!(
        hbq(&a, "", "&", QueryEncoding::Rfc1738),
        "foo=bar&baz=1&test=a+%27+%22+&0=abc&true=1&false=0"
    );
    assert_eq!(
        hbq(&a, "foo", ";", QueryEncoding::Rfc1738),
        "foo=bar;baz=1;test=a+%27+%22+;foo0=abc;true=1;false=0"
    );
    // test http_build_query_variation2.phpt
    let m = vec![
        (Int(0), Q::S("20")),
        (Int(5), Q::S("13")),
        (
            Int(9),
            Q::A(vec![
                (Int(1), Q::S("val1")),
                (Int(3), Q::S("val2")),
                (Str(b"string"), Q::S("string")),
            ]),
        ),
        (Str(b"name"), Q::S("homepage")),
        (Str(b"page"), Q::S("10")),
        (
            Str(b"sort"),
            Q::A(vec![
                (Int(0), Q::S("desc")),
                (
                    Str(b"admin"),
                    Q::A(vec![
                        (Int(0), Q::S("admin1")),
                        (
                            Str(b"admin2"),
                            Q::A(vec![(Str(b"who"), Q::S("admin2")), (Int(2), Q::S("test"))]),
                        ),
                    ]),
                ),
            ]),
        ),
    ];
    assert_eq!(
        hbq(&m, "prefix_", "&", QueryEncoding::Rfc1738),
        "prefix_0=20&prefix_5=13&prefix_9%5B1%5D=val1&prefix_9%5B3%5D=val2&prefix_9%5Bstring%5D=string&name=homepage\
         &page=10&sort%5B0%5D=desc&sort%5Badmin%5D%5B0%5D=admin1&sort%5Badmin%5D%5Badmin2%5D%5Bwho%5D=admin2\
         &sort%5Badmin%5D%5Badmin2%5D%5B2%5D=test"
    );
    // test gh12745.phpt
    let h = vec![(Str(b"hello"), Q::S("world")), (Str(b"foo"), Q::S("bar"))];
    assert_eq!(
        hbq(&h, "", "&", QueryEncoding::from_php(2)),
        "hello=world&foo=bar"
    );
    let sp = vec![(Str(b"a b"), Q::S("c d~"))];
    assert_eq!(hbq(&sp, "", "&", QueryEncoding::Rfc3986), "a%20b=c%20d~");
}

#[test]
fn parse_url_basic() {
    // test parse_url_basic_001.phpt (a selection)
    let p = |s: &str| parse_url(s.as_bytes());
    assert_eq!(
        p("64.246.30.37"),
        Some(with(Url::default(), None, Some("64.246.30.37"), None, None))
    );
    assert_eq!(
        p("64.246.30.37:80/"),
        Some(with(
            u(None, Some("64.246.30.37"), Some(80), None),
            None,
            Some("/"),
            None,
            None
        ))
    );
    assert_eq!(
        p("www.php.net:80"),
        Some(u(None, Some("www.php.net"), Some(80), None))
    );
    assert_eq!(
        p(
            "http://secret:hid:out@www.php.net:80/index.php?test=1&test2=char&test3=mixesCI#some_page_ref123"
        ),
        Some(with(
            u(Some("http"), Some("www.php.net"), Some(80), Some("secret")),
            Some("hid:out"),
            Some("/index.php"),
            Some("test=1&test2=char&test3=mixesCI"),
            Some("some_page_ref123")
        ))
    );
    assert_eq!(
        p("http://user:@pass@host/path?argument?value#etc"),
        Some(with(
            u(Some("http"), Some("host"), None, Some("user")),
            Some("@pass"),
            Some("/path"),
            Some("argument?value"),
            Some("etc")
        ))
    );
    assert_eq!(
        p("zlib:http://foo@bar"),
        Some(with(
            u(Some("zlib"), None, None, None),
            None,
            Some("http://foo@bar"),
            None,
            None
        ))
    );
    assert_eq!(
        p("gg:9130731"),
        Some(with(
            u(Some("gg"), None, None, None),
            None,
            Some("9130731"),
            None,
            None
        ))
    );
    assert_eq!(
        p("file:///a:/"),
        Some(with(
            u(Some("file"), None, None, None),
            None,
            Some("a:/"),
            None,
            None
        ))
    );
    assert_eq!(
        p("file:///ab:/"),
        Some(with(
            u(Some("file"), None, None, None),
            None,
            Some("/ab:/"),
            None,
            None
        ))
    );
    assert_eq!(p("x://::6.5"), Some(u(Some("x"), Some(":"), Some(6), None)));
    assert_eq!(
        p("http://[x:80]/"),
        Some(with(
            u(Some("http"), Some("[x:80]"), None, None),
            None,
            Some("/"),
            None,
            None
        ))
    );
    assert_eq!(p("scheme:"), Some(u(Some("scheme"), None, None, None)));
    assert_eq!(
        p(""),
        Some(with(Url::default(), None, Some(""), None, None))
    );
    assert_eq!(
        p("%:x"),
        Some(with(Url::default(), None, Some("%:x"), None, None))
    );
    for bad in [
        "x://::abc/?",
        "http://?:/",
        "http:///blah.com",
        "http://:80",
        "http://user@:80",
        "http://@/",
        "http://#",
        "http://blah.com:123456",
        "http://blah.com:abcdef",
        // test bug55399.phpt
        ":",
    ] {
        assert_eq!(p(bad), None, "{bad}");
    }
}

#[test]
fn parse_url_bugs() {
    let p = |s: &str| parse_url(s.as_bytes());
    // test parse_url_relative_scheme.phpt
    assert_eq!(
        p("//example.org"),
        Some(u(None, Some("example.org"), None, None))
    );
    // test bug68917.phpt
    assert_eq!(
        p("//example.org:81/hi?a=b#c=d"),
        Some(with(
            u(None, Some("example.org"), Some(81), None),
            None,
            Some("/hi"),
            Some("a=b"),
            Some("c=d")
        ))
    );
    // test bug73192.phpt
    assert_eq!(
        p("http://example.com:80#@google.com/"),
        Some(with(
            u(Some("http"), Some("example.com"), Some(80), None),
            None,
            None,
            None,
            Some("@google.com/")
        ))
    );
    // test bug74780.phpt
    assert_eq!(
        p("//username@php.net/path?query=1:2"),
        Some(with(
            u(None, Some("php.net"), None, Some("username")),
            None,
            Some("/path"),
            Some("query=1:2"),
            None
        ))
    );
    // test bug63162.phpt
    assert_eq!(
        p("//user@host"),
        Some(u(None, Some("host"), None, Some("user")))
    );
    // test bug69976.phpt
    assert_eq!(
        p("/busca/?fq=B:200013"),
        Some(with(
            Url::default(),
            None,
            Some("/busca/"),
            Some("fq=B:200013"),
            None
        ))
    );
    // test bug54180.phpt
    assert_eq!(
        p("http://example.com/path/script.html#fragment?data"),
        Some(with(
            u(Some("http"), Some("example.com"), None, None),
            None,
            Some("/path/script.html"),
            None,
            Some("fragment?data")
        ))
    );
}

#[test]
fn parse_url_components() {
    // test parse_url_error_002.phpt and parse_url_basic_00*.phpt
    let url = b"http://secret:hideout@www.php.net:80/index.php?test=1&test2=char&test3=mixesCI#some_page_ref123";
    assert!(matches!(
        parse_url_component(url, -1),
        Ok(UrlResult::Array(_))
    ));
    assert_eq!(
        parse_url_component(url, 99),
        Err(StdError::ValueError(
            "parse_url(): Argument #2 ($component) must be a valid URL component identifier, 99 given".into()
        ))
    );
    assert_eq!(
        parse_url_component(url, PHP_URL_PORT),
        Ok(UrlResult::Part(Some(UrlPart::Int(80))))
    );
    assert_eq!(
        parse_url_component(url, PHP_URL_PASS),
        Ok(UrlResult::Part(Some(UrlPart::Str(b"hideout".to_vec()))))
    );
    assert_eq!(
        parse_url_component(b"64.246.30.37", PHP_URL_SCHEME),
        Ok(UrlResult::Part(None))
    );
    assert_eq!(
        parse_url_component(b"http://:80", PHP_URL_HOST),
        Ok(UrlResult::False)
    );
}

#[test]
fn base64() {
    // tests base64_encode_basic_001.phpt-style values (RFC 4648 section 10)
    for (plain, enc) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64_encode(plain.as_bytes()), enc.as_bytes());
        assert_eq!(
            base64_decode(enc.as_bytes(), true).unwrap(),
            plain.as_bytes()
        );
    }
    // test base64_decode_basic_002.phpt
    let ws = b"a GVs   bG8gd2\n                        9ybGQh";
    assert_eq!(base64_decode(ws, true).unwrap(), b"hello world!");
    assert_eq!(
        base64_decode(b"aGVsbG8gd29ybGQh*", false).unwrap(),
        b"hello world!"
    );
    assert_eq!(base64_decode(b"aGVsbG8gd29ybGQh*", true), None);
    // test bug52327.phpt
    let s =
        b"=VGhl=ICc9=JyBz=eW1i=b2xz=IGFy=ZW4n=dCBh=bGxv=d2Vk=IHdo=ZXJl=IGkg=cHV0=IHRo=ZW0g=by5P";
    assert_eq!(
        base64_decode(s, false).unwrap(),
        b"The '=' symbols aren't allowed where i put them o.O"
    );
    assert_eq!(base64_decode(s, true), None);
    // test bug55273.phpt
    assert_eq!(base64_decode(b"UEg=\n", true).unwrap(), b"PH");
    assert_eq!(base64_decode(b"UA==\n", true).unwrap(), b"P");
    // test bug47174.phpt
    assert_eq!(base64_decode(b"Zm9v==YmFy", false).unwrap(), b"foobar");
    assert_ne!(
        base64_decode(b"dGVzdA==", false),
        base64_decode(b"dGVzdA==CRAP", false)
    );
    // observed on PHP 8.5
    assert_eq!(base64_decode(b"====", true), None);
    assert_eq!(base64_decode(b"UA===", true), None);
}
