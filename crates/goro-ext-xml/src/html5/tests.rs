//! Tree construction tests in the html5lib "#document" format, checked
//! against html5lib and php-src (lexbor) behaviour.

use super::dom::LKind;
use super::*;

fn dump(p: &Parser, n: NodeId, depth: usize, out: &mut String) {
    for c in p.dom.children(n) {
        let x = &p.dom.nodes[c];
        let pad = "  ".repeat(depth);
        match x.kind {
            LKind::Element => {
                let name = match &x.qualified {
                    Some(q) => String::from_utf8_lossy(q).into_owned(),
                    None => String::from_utf8_lossy(p.tag_name(x.tag)).into_owned(),
                };
                let prefix = match x.ns {
                    NS_SVG => "svg ",
                    NS_MATH => "math ",
                    _ => "",
                };
                out.push_str(&format!("| {}<{}{}>\n", pad, prefix, name));
                let mut attrs: Vec<(String, String)> = x
                    .attrs
                    .iter()
                    .map(|a| {
                        let pre = match a.ns {
                            NS_XLINK => "xlink ",
                            NS_XML => "xml ",
                            NS_XMLNS => "xmlns ",
                            _ => "",
                        };
                        let local: &[u8] = if a.has_prefix { &a.local } else { a.qname() };
                        (
                            format!("{}{}", pre, String::from_utf8_lossy(local)),
                            String::from_utf8_lossy(a.value.as_deref().unwrap_or(b"")).into_owned(),
                        )
                    })
                    .collect();
                attrs.sort();
                for (k, v) in attrs {
                    out.push_str(&format!("| {}  {}=\"{}\"\n", pad, k, v));
                }
                if let Some(tc) = x.template_content {
                    out.push_str(&format!("| {}  content\n", pad));
                    dump(p, tc, depth + 2, out);
                }
                dump(p, c, depth + 1, out);
            }
            LKind::Text => out.push_str(&format!(
                "| {}\"{}\"\n",
                pad,
                String::from_utf8_lossy(&x.data)
            )),
            LKind::Comment => out.push_str(&format!(
                "| {}<!-- {} -->\n",
                pad,
                String::from_utf8_lossy(&x.data)
            )),
            LKind::Doctype => {
                if x.public_id.is_empty() && x.system_id.is_empty() {
                    out.push_str(&format!(
                        "| {}<!DOCTYPE {}>\n",
                        pad,
                        String::from_utf8_lossy(&x.data)
                    ));
                } else {
                    out.push_str(&format!(
                        "| {}<!DOCTYPE {} \"{}\" \"{}\">\n",
                        pad,
                        String::from_utf8_lossy(&x.data),
                        String::from_utf8_lossy(&x.public_id),
                        String::from_utf8_lossy(&x.system_id)
                    ));
                }
            }
            _ => {}
        }
    }
}

fn parse(input: &str) -> Parser {
    let mut p = Parser::new();
    p.begin_document();
    let (s, e) = p.push_chunk(input.as_bytes());
    p.feed(s, e);
    p.finish();
    p
}

fn check(input: &str, expected: &str) {
    let p = parse(input);
    let mut out = String::new();
    let d = p.document;
    dump(&p, d, 0, &mut out);
    assert_eq!(out.trim_end(), expected.trim_end(), "input: {:?}", input);
}

#[test]
fn simple_document() {
    check(
        "<!doctype html><p>One<p>Two",
        "| <!DOCTYPE html>\n| <html>\n|   <head>\n|   <body>\n|     <p>\n|       \"One\"\n|     <p>\n|       \"Two\"",
    );
}

#[test]
fn adoption_agency() {
    check(
        "<a><p>X<a>Y</a>Z</p></a>",
        "| <html>\n|   <head>\n|   <body>\n|     <a>\n|     <p>\n|       <a>\n|         \"X\"\n|       <a>\n|         \"Y\"\n|       \"Z\"",
    );
    check(
        "<b><p>x</b>y",
        "| <html>\n|   <head>\n|   <body>\n|     <b>\n|     <p>\n|       <b>\n|         \"x\"\n|       \"y\"",
    );
}

#[test]
fn foster_parenting() {
    check(
        "<table>a<b>b</b>c</table>",
        "| <html>\n|   <head>\n|   <body>\n|     \"a\"\n|     <b>\n|       \"b\"\n|     \"c\"\n|     <table>",
    );
}

#[test]
fn foreign_content() {
    check(
        "<svg viewbox=y xlink:href=z><foreignobject><div>a</div></foreignobject></svg>",
        "| <html>\n|   <head>\n|   <body>\n|     <svg svg>\n|       viewBox=\"y\"\n|       xlink href=\"z\"\n|       <svg foreignObject>\n|         <div>\n|           \"a\"",
    );
    check(
        "<math><annotation-xml encoding=\"text/html\"><div>a</div></annotation-xml></math>",
        "| <html>\n|   <head>\n|   <body>\n|     <math math>\n|       <math annotation-xml>\n|         encoding=\"text/html\"\n|         <div>\n|           \"a\"",
    );
}

#[test]
fn character_references() {
    check(
        "&lt;&notin;&noti&amp;&#x41;&#128;",
        "| <html>\n|   <head>\n|   <body>\n|     \"<\u{2209}\u{ac}i&A\u{20ac}\"",
    );
}

#[test]
fn template_contents() {
    check(
        "<template><tr><td>x</template>",
        "| <html>\n|   <head>\n|     <template>\n|       content\n|         <tr>\n|           <td>\n|             \"x\"\n|   <body>",
    );
}

#[test]
fn lexbor_quirks() {
    // lexbor drops a token that "in table" sends to "in body" for reprocessing
    check(
        "<table><image></table>",
        "| <html>\n|   <head>\n|   <body>\n|     <table>",
    );
    // and ignores </noscript> in "in head noscript"
    check(
        "<head><noscript><link></noscript><link></head>",
        "| <html>\n|   <head>\n|     <noscript>\n|       <link>\n|       <link>\n|   <body>",
    );
    // "--!" followed by anything else is dropped from comments
    check(
        "<!--a--!x-->",
        "| <!-- ax -->\n| <html>\n|   <head>\n|   <body>",
    );
}

#[test]
fn quirks_mode() {
    assert_eq!(parse("<p>").compat_mode, CMODE_QUIRKS);
    assert_eq!(parse("<!doctype html><p>").compat_mode, CMODE_NO_QUIRKS);
    assert_eq!(
        parse("<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\">").compat_mode,
        CMODE_LIMITED_QUIRKS
    );
}

#[test]
fn error_positions() {
    let p = parse("<title>foo</title>\n<p>error</>");
    let te: Vec<(&str, usize, usize, usize)> = p
        .tree_errors
        .iter()
        .map(|e| (e.msg, e.line, e.column, e.length))
        .collect();
    assert_eq!(te[0], ("unexpected-token-in-initial-mode", 0, 1, 5));
    assert_eq!(p.tok_errors[0].msg, "missing-end-tag-name");
}

#[test]
fn encoding_prescan() {
    assert_eq!(
        encoding::determine_encoding(b"<meta charset=\"windows-1251\">"),
        ("windows-1251", 0)
    );
    assert_eq!(
        encoding::determine_encoding(
            b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=Shift_JIS\">"
        ),
        ("Shift_JIS", 0)
    );
    assert_eq!(encoding::determine_encoding(b"\xEF\xBB\xBFx"), ("UTF-8", 3));
    assert_eq!(encoding::determine_encoding(b"x"), ("UTF-8", 0));
    assert_eq!(encoding::by_name(b"latin1"), Some("windows-1252"));
    assert_eq!(encoding::by_name(b" utf-8"), None);
}
