//! url.rs against PHP 8.5 output (tests/fixtures/url.txt, made by url.php).

mod common;

use common::*;
use goro_ext_standard::url::{self, QueryBuilder, QueryEncoding, QueryKey, UrlPart, UrlResult};

/// Walks an array as the wiring layer will.
fn build(b: &mut QueryBuilder, parent: Option<&[u8]>, arr: &[(V, V)], prefix: &[u8]) {
    for (k, v) in arr {
        let key = match k {
            V::I(i) => QueryKey::Int(*i),
            V::S(s) => QueryKey::Str(s),
            _ => unreachable!(),
        };
        let name = match parent {
            None => b.top_key(key, prefix),
            Some(p) => b.child_key(p, key),
        };
        match v {
            V::A(a) => build(b, Some(&name), a, prefix),
            V::N => {}
            V::S(s) => b.push(&name, s),
            V::I(i) => b.push(&name, i.to_string().as_bytes()),
            V::B(t) => b.push(&name, if *t { b"1" } else { b"0" }),
            _ => unreachable!(),
        }
    }
}

fn url_value(u: &url::Url) -> V {
    let mut a = Vec::new();
    let mut put = |k: &str, v: &Option<Vec<u8>>| {
        if let Some(v) = v {
            a.push((s(k.as_bytes()), V::S(v.clone())));
        }
    };
    put("scheme", &u.scheme);
    put("host", &u.host);
    if let Some(p) = u.port {
        a.push((s(b"port"), V::I(p)));
    }
    let mut put = |k: &str, v: &Option<Vec<u8>>| {
        if let Some(v) = v {
            a.push((s(k.as_bytes()), V::S(v.clone())));
        }
    };
    put("user", &u.user);
    put("pass", &u.pass);
    put("path", &u.path);
    put("query", &u.query);
    put("fragment", &u.fragment);
    V::A(a)
}

fn run(c: &Case, func: &str) -> Option<(Out, Vec<goro_ext_standard::error::Diag>)> {
    if c.func != func {
        return None;
    }
    let a0 = || c.args[0].s();
    let out: Out = match func {
        "urlencode" => V::S(url::urlencode(a0())).into(),
        "rawurlencode" => V::S(url::rawurlencode(a0())).into(),
        "urldecode" => V::S(url::urldecode(a0())).into(),
        "rawurldecode" => V::S(url::rawurldecode(a0())).into(),
        "base64_encode" => V::S(url::base64_encode(a0())).into(),
        "base64_decode" => {
            let strict = c.args.get(1).map(|v| v.b()).unwrap_or(false);
            match url::base64_decode(a0(), strict) {
                Some(v) => V::S(v).into(),
                None => V::B(false).into(),
            }
        }
        "http_build_query" => {
            let V::A(arr) = &c.args[0] else {
                unreachable!()
            };
            let prefix = c.args.get(1).map(|v| v.s().to_vec()).unwrap_or_default();
            let sep = match c.args.get(2) {
                None | Some(V::N) => b"&".to_vec(),
                Some(v) => v.s().to_vec(),
            };
            let enc = QueryEncoding::from_php(c.args.get(3).map(|v| v.i()).unwrap_or(1));
            let mut b = QueryBuilder::new(&sep, enc);
            build(&mut b, None, arr, &prefix);
            V::S(b.finish()).into()
        }
        "parse_url" => {
            let comp = c.args.get(1).map(|v| v.i()).unwrap_or(-1);
            if comp == -1 {
                match url::parse_url(a0()) {
                    Some(u) => url_value(&u).into(),
                    None => V::B(false).into(),
                }
            } else {
                match url::parse_url_component(a0(), comp) {
                    Err(e) => Out::X(e),
                    Ok(UrlResult::False) => V::B(false).into(),
                    Ok(UrlResult::Array(u)) => url_value(&u).into(),
                    Ok(UrlResult::Part(None)) => V::N.into(),
                    Ok(UrlResult::Part(Some(UrlPart::Str(s)))) => V::S(s).into(),
                    Ok(UrlResult::Part(Some(UrlPart::Int(i)))) => V::I(i).into(),
                }
            }
        }
        _ => unreachable!(),
    };
    Some((out, Vec::new()))
}

#[test]
fn fixture_urlencode() {
    check("url", |c| run(c, "urlencode"));
    check("url", |c| run(c, "rawurlencode"));
}

#[test]
fn fixture_urldecode() {
    check("url", |c| run(c, "urldecode"));
    check("url", |c| run(c, "rawurldecode"));
}

#[test]
fn fixture_http_build_query() {
    check("url", |c| run(c, "http_build_query"));
}

#[test]
fn fixture_parse_url() {
    check("url", |c| run(c, "parse_url"));
}

#[test]
fn fixture_base64() {
    check("url", |c| run(c, "base64_encode"));
    check("url", |c| run(c, "base64_decode"));
}
