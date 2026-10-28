//! html.rs against PHP 8.5 output (tests/fixtures/html.txt, made by html.php).

mod common;

use common::*;
use goro_ext_standard::html::{self, AllowedTags, Charset, DEFAULT_FLAGS};

fn default_charset(c: &Case) -> Vec<u8> {
    match c.ctx.strip_prefix("default_charset=") {
        Some(d) => d.as_bytes().to_vec(),
        None => b"UTF-8".to_vec(),
    }
}

fn charset_arg(c: &Case, idx: usize, diags: &mut Vec<goro_ext_standard::error::Diag>) -> Charset {
    let name = match c.args.get(idx) {
        None | Some(V::N) => None,
        Some(v) => Some(v.s().to_vec()),
    };
    html::resolve_charset(name.as_deref(), &default_charset(c), diags)
}

fn flags_arg(c: &Case, idx: usize) -> i64 {
    c.args.get(idx).map(|v| v.i()).unwrap_or(DEFAULT_FLAGS)
}

fn run(c: &Case, func: &str) -> Option<(Out, Vec<goro_ext_standard::error::Diag>)> {
    if c.func != func {
        return None;
    }
    let mut d = Vec::new();
    let v = match func {
        "htmlspecialchars" | "htmlentities" => {
            let cs = charset_arg(c, 2, &mut d);
            let de = c.args.get(3).map(|v| v.b()).unwrap_or(true);
            let s = c.args[0].s();
            if func == "htmlentities" {
                V::S(html::htmlentities(s, flags_arg(c, 1), cs, de, &mut d))
            } else {
                V::S(html::htmlspecialchars(s, flags_arg(c, 1), cs, de))
            }
        }
        "html_entity_decode" => {
            let cs = charset_arg(c, 2, &mut d);
            V::S(html::html_entity_decode(c.args[0].s(), flags_arg(c, 1), cs))
        }
        "htmlspecialchars_decode" => V::S(html::htmlspecialchars_decode(
            c.args[0].s(),
            flags_arg(c, 1),
        )),
        "get_html_translation_table" => {
            let table = c
                .args
                .first()
                .map(|v| v.i())
                .unwrap_or(html::HTML_SPECIALCHARS);
            let cs = match c.args.get(2) {
                None => Charset::Utf8,
                Some(_) => charset_arg(c, 2, &mut d),
            };
            arr_str(html::get_html_translation_table(table, flags_arg(c, 1), cs))
        }
        "strip_tags" => {
            let list: Vec<Vec<u8>>;
            let al = match c.args.get(1) {
                None | Some(V::N) => AllowedTags::None,
                Some(V::S(s)) => AllowedTags::Str(s),
                Some(V::A(a)) => {
                    list = a.iter().map(|(_, v)| v.s().to_vec()).collect();
                    let refs: Vec<&[u8]> = list.iter().map(|v| &v[..]).collect();
                    return Some((
                        Out::V(V::S(html::strip_tags(
                            c.args[0].s(),
                            AllowedTags::List(&refs),
                        ))),
                        d,
                    ));
                }
                Some(v) => panic!("{v:?}"),
            };
            V::S(html::strip_tags(c.args[0].s(), al))
        }
        _ => unreachable!(),
    };
    Some((Out::V(v), d))
}

#[test]
fn fixture_htmlspecialchars() {
    check("html", |c| run(c, "htmlspecialchars"));
}

#[test]
fn fixture_htmlentities() {
    check("html", |c| run(c, "htmlentities"));
}

#[test]
fn fixture_html_entity_decode() {
    check("html", |c| run(c, "html_entity_decode"));
}

#[test]
fn fixture_htmlspecialchars_decode() {
    check("html", |c| run(c, "htmlspecialchars_decode"));
}

#[test]
fn fixture_get_html_translation_table() {
    check("html", |c| run(c, "get_html_translation_table"));
}

#[test]
fn fixture_strip_tags() {
    check("html", |c| run(c, "strip_tags"));
}
