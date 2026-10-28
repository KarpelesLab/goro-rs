//! CSS selectors: behaviour from ext/dom/tests/modern/css_selectors and
//! tests/fixtures/css/probe.php (observed on PHP 8.5).

use goro_xml::css::{self, CssErrorKind};
use goro_xml::html5::{self, ParseOptions};
use goro_xml::tree::{Arena, HTML_NS, NodeId, QName};

fn doc(a: &mut Arena, html: &str) -> NodeId {
    html5::parse_document(
        a,
        html.as_bytes(),
        &ParseOptions {
            ..Default::default()
        },
    )
    .doc
}

fn names(a: &Arena, root: NodeId, sel: &str) -> String {
    let s = css::parse(sel).unwrap_or_else(|e| panic!("{sel}: {e:?}"));
    let v = css::query_selector_all(a, root, &s);
    // querySelector and matches agree with querySelectorAll.
    assert_eq!(
        css::query_selector(a, root, &s),
        v.first().copied(),
        "{sel}"
    );
    for n in a.descendants(root) {
        if a.is_element(n) {
            assert_eq!(s.matches(a, n), v.contains(&n), "{sel}");
        }
    }
    v.iter()
        .map(|&n| a.element(n).unwrap().name.local.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn err(sel: &str) -> String {
    match css::parse(sel) {
        Ok(_) => "ok".into(),
        Err(e) => e.exception_message(),
    }
}

const PROBE: &str = "<!doctype html><html><body><div id=\"Main\" class=\"A b\"><p lang=\"en-US\" title=\"x\">t</p><svg><foreignObject/><circle viewBox=\"1\"/></svg><input type=\"checkbox\" checked><select><option selected>a</option><option>b</option></select><a href=\"x\">l</a><a>nl</a></div></body></html>";

#[test]
fn php85_probe() {
    let mut a = Arena::new();
    let d = doc(&mut a, PROBE);
    let body = a
        .descendants(d)
        .find(|&n| a.element(n).is_some_and(|e| &*e.name.local == "body"))
        .unwrap();
    let div2 = a.new_element(
        d,
        QName {
            prefix: None,
            local: "Div".into(),
            ns: Some(HTML_NS.into()),
        },
    );
    a.raw_append(body, div2);
    for (sel, want) in [
        ("div", "div"),
        ("DIV", "div"),
        ("Div", "div"),
        (
            "*",
            "html,head,body,div,p,svg,foreignObject,circle,input,select,option,option,a,a,Div",
        ),
        ("p[LANG]", "p"),
        ("p[lang=\"en-us\"]", "p"),
        ("p[title=\"X\"]", ""),
        ("p[lang|=en]", "p"),
        ("circle", "circle"),
        ("CIRCLE", ""),
        ("foreignObject", "foreignObject"),
        ("foreignobject", ""),
        ("[viewBox]", "circle"),
        ("[viewbox]", ""),
        ("#Main", "div"),
        ("#main", ""),
        (".A", "div"),
        (".a", ""),
        ("div.b", "div"),
        (":checked", "input,option"),
        ("option:checked", "option"),
        (":link", "a"),
        ("a:not([href])", "a"),
        ("p:first-child", "p"),
        (":nth-child(2n of p)", ""),
        ("p:only-child", ""),
        (":root", "html"),
        ("div > p", "p"),
        ("div>p", "p"),
        ("div  p", "p"),
        ("p ~ svg", "svg"),
        ("p + svg", "svg"),
        (":is(p, svg)", "p,svg"),
        (":where(p)", "p"),
        (":has(> p)", "div"),
        ("div:has(p, a)", "div"),
        (
            ":not(p, div)",
            "html,head,body,svg,foreignObject,circle,input,select,option,option,a,a,Div",
        ),
        ("*|div", "div"),
        ("svg|circle", "circle"),
        ("|p", "p"),
        (
            ":nth-child( 2n + 1 )",
            "html,head,div,p,foreignObject,input,option,a",
        ),
        (
            ":nth-child(odd)",
            "html,head,div,p,foreignObject,input,option,a",
        ),
        (
            ":nth-child(-n+3)",
            "html,head,body,div,p,svg,foreignObject,circle,input,option,option,Div",
        ),
        (":nth-child(+5)", "a"),
        ("#1", ""),
        ("a b c", ""),
        ("div:hover", ""),
        (":focus", ""),
        ("a:not(:hover)", "a,a"),
        ("p:nth-of-type(1)", "p"),
        (":empty", "head,foreignObject,circle,input,Div"),
        ("[|a]", ""),
        (":NTH-CHILD(1)", "html,head,div,p,foreignObject,option"),
        (":FIRST-CHILD", "html,head,div,p,foreignObject,option"),
        ("DIV:not(P)", "div"),
    ] {
        assert_eq!(names(&a, d, sel), want, "{sel}");
    }
}

#[test]
fn php85_errors() {
    for (sel, want) in [
        (
            "",
            "Invalid selector (Selectors. Unexpected token: END-OF-FILE)",
        ),
        (
            " ",
            "Invalid selector (Selectors. Unexpected token: END-OF-FILE)",
        ),
        (",", "Invalid selector (Selectors. Unexpected token: ,)"),
        (
            "div,",
            "Invalid selector (Selectors. Unexpected token: END-OF-FILE)",
        ),
        (",div", "Invalid selector (Selectors. Unexpected token: ,)"),
        (
            "div[",
            "Invalid selector (Selectors. Unexpected token: END-OF-FILE)",
        ),
        (
            "div[a",
            "Invalid selector (Selectors. Unexpected token: END-OF-FILE)",
        ),
        (
            "div[a=]",
            "Invalid selector (Selectors. Unexpected token: ])",
        ),
        (
            "div[a=\"b\" x]",
            "Invalid selector (Selectors. Unexpected token: x)",
        ),
        (
            "div[a=\"b\" I]",
            "Invalid selector (Selectors. Unexpected token: I)",
        ),
        (
            "div >",
            "Invalid selector (Selectors. Unexpected token: END-OF-FILE)",
        ),
        ("> div", "Invalid selector (Selectors. Unexpected token: >)"),
        (
            "div >> p",
            "Invalid selector (Selectors. Unexpected token: >)",
        ),
        (
            ":unknown",
            "Invalid selector (Selectors. Unexpected token: unknown)",
        ),
        (
            "::unknown",
            "Invalid selector (Selectors. Unexpected token: unknown)",
        ),
        (
            ":nth-child(foo)",
            "Invalid selector (Selectors. Unexpected token: foo)",
        ),
        (
            ":nth-child()",
            "Invalid selector (Selectors. Pseudo function can't be empty: nth-child())",
        ),
        (
            ":not()",
            "Invalid selector (Selectors. Pseudo function can't be empty: not())",
        ),
        (
            ":has()",
            "Invalid selector (Selectors. Pseudo function can't be empty: has())",
        ),
        ("#", "Invalid selector (Selectors. Unexpected token: #)"),
        (
            ".",
            "Invalid selector (Selectors. Unexpected token: END-OF-FILE)",
        ),
        (".1", "Invalid selector (Selectors. Unexpected token: 0.1)"),
        (
            "p::before",
            "Invalid selector (Selectors. Not supported: before)",
        ),
        (
            "::before",
            "Invalid selector (Selectors. Not supported: before)",
        ),
        (
            ":first-line",
            "Invalid selector (Selectors. Unexpected token: first-line)",
        ),
        (
            "div ! p",
            "Invalid selector (Selectors. Unexpected token: !)",
        ),
        ("div{", "Invalid selector (Selectors. Unexpected token: {)"),
        (
            "p::",
            "Invalid selector (Selectors. Unexpected token: END-OF-FILE)",
        ),
        (
            ":",
            "Invalid selector (Selectors. Unexpected token: END-OF-FILE)",
        ),
        ("]", "Invalid selector (Selectors. Unexpected token: ])"),
        ("(", "Invalid selector (Selectors. Unexpected token: ()"),
        ("a)", "Invalid selector (Selectors. Unexpected token: ))"),
        (
            "\"a\"",
            "Invalid selector (Selectors. Unexpected token: \"a\")",
        ),
        (
            "p:lang(en)",
            "Invalid selector (Selectors. Not supported: lang)",
        ),
        (
            ":dir(rtl)",
            "Invalid selector (Selectors. Not supported: dir)",
        ),
        (
            ":nth-col(1)",
            "Invalid selector (Selectors. Not supported: nth-col)",
        ),
        (
            ":host",
            "Invalid selector (Selectors. Unexpected token: host)",
        ),
        (
            ":scope > p",
            "Invalid selector (Selectors. Not supported: scope)",
        ),
        ("[a~=]", "Invalid selector (Selectors. Unexpected token: ])"),
        ("[*|a]", "Invalid selector (Selectors. Unexpected token: *)"),
        (
            "container[* | bar]",
            "Invalid selector (Selectors. Unexpected token: *)",
        ),
        (
            "@invalid",
            "Invalid selector (Selectors. Unexpected token: @invalid)",
        ),
        (
            ":visited",
            "Invalid selector (Selectors. Not supported: visited)",
        ),
        (
            "a::marker",
            "Invalid selector (Selectors. Not supported: marker)",
        ),
    ] {
        assert_eq!(err(sel), want, "{sel:?}");
    }
    assert_eq!(
        css::parse("col.selected||td").unwrap_err().kind,
        CssErrorKind::Unsupported
    );
    let b = css::parse(":blank").unwrap_err();
    assert_eq!(b.kind, CssErrorKind::Blank);
    assert!(b.message.starts_with(":blank selector is not implemented"));
}

#[test]
fn quirks_mode() {
    // quirks_mode.phpt
    let html = "<html>\n    <div class=\"HElLoWorLD\"/>\n    <div id=\"hI\"/>\n</html>";
    let mut a = Arena::new();
    let d = doc(&mut a, html);
    assert_eq!(names(&a, d, "div.helloworld"), "div");
    assert_eq!(names(&a, d, "#hi"), "div");
    let d = doc(&mut a, &format!("<!DOCTYPE html>\n{html}"));
    assert_eq!(names(&a, d, "div.helloworld"), "");
    assert_eq!(names(&a, d, "div.HElLoWorLD"), "div");
    assert_eq!(names(&a, d, "#hi"), "");
}

#[test]
fn combinators_and_nth() {
    // combinators.phpt, pseudo_classes_nth_child(_of).phpt,
    // pseudo_classes_has.phpt, gh18877.phpt, lexbor274.phpt
    let mut a = Arena::new();
    let d = doc(
        &mut a,
        "<!DOCTYPE html><html><body><p>1</p><p>2</p><img src=1><div class=\"\"><p>3</p><img src=2><img src=3><div class=\"foo bar baz\"><p>4</p></div></div><article title=\"foo\" class=\"bar\"><p class=\"bar\">5</p></article></body></html>",
    );
    assert_eq!(names(&a, d, "nonsense"), "");
    assert_eq!(names(&a, d, "body div p"), "p,p");
    assert_eq!(names(&a, d, "div > *"), "p,img,img,div,p");
    assert_eq!(names(&a, d, "div > p + img"), "img");
    assert_eq!(names(&a, d, "div > p ~ img"), "img,img");
    assert_eq!(names(&a, d, "body > img"), "img");
    assert_eq!(names(&a, d, "div.bar.baz > p"), "p");
    assert_eq!(names(&a, d, "article[title].bar p"), "p");
    assert_eq!(names(&a, d, "div:has(div) ~ *"), "article");
    assert_eq!(names(&a, d, "a:has(+ b)"), "");
    let d = doc(
        &mut a,
        "<!DOCTYPE html><div><h2 class=hi>1</h2><h2>2</h2><h2 class=hi>3</h2><h2 class=hi>4</h2><h2>5</h2><h2 class=hi>6</h2></div>",
    );
    let text = |a: &Arena, sel: &str| -> String {
        let s = css::parse(sel).unwrap();
        css::query_selector_all(a, d, &s)
            .iter()
            .map(|&n| a.text_content(n))
            .collect::<Vec<_>>()
            .join(",")
    };
    assert_eq!(text(&a, "h2:nth-child(even of .hi)"), "3,6");
    assert_eq!(text(&a, "h2.hi:nth-child(even)"), "4,6");
    assert_eq!(text(&a, "h2:nth-child(odd of .hi)"), "1,4");
    assert_eq!(text(&a, "h2:nth-last-child(even of .hi)"), "1,4");
    assert_eq!(text(&a, "h2:nth-last-child(odd)"), "2,4,6");
    assert_eq!(text(&a, "h2:nth-child(3n - 2)"), "1,4");
    assert_eq!(text(&a, "h2:nth-last-child(3n - 2)"), "3,6");
    assert_eq!(
        text(&a, "h2:nth-of-type(n+2):nth-last-of-type(n+2)"),
        "2,3,4,5"
    );
    assert_eq!(
        text(&a, "h2:not(:first-of-type):not(:last-of-type)"),
        "2,3,4,5"
    );
}

#[test]
fn form_pseudo_classes() {
    // pseudo_classes_disabled_enabled.phpt (HTML version)
    let mut a = Arena::new();
    let d = doc(
        &mut a,
        "<!DOCTYPE html><button></button><button disabled></button><input disabled><fieldset disabled><fieldset id=1></fieldset></fieldset><fieldset disabled><!-- foo --><legend><div><fieldset id=2></fieldset></div></legend><div><fieldset id=3></fieldset></div></fieldset>",
    );
    let s = css::parse(":disabled").unwrap();
    let ids: Vec<String> = css::query_selector_all(&a, d, &s)
        .iter()
        .map(|&n| {
            format!(
                "{}{}",
                a.element(n).unwrap().name.local,
                a.get_attribute(n, "id").unwrap_or_default()
            )
        })
        .collect();
    assert_eq!(
        ids,
        ["button", "input", "fieldset", "fieldset", "fieldset3"]
    );
    // pseudo_classes_read_write_read_only, optional_required, placeholder_shown
    let d = doc(
        &mut a,
        "<!DOCTYPE html><input type=text readonly><textarea></textarea><p contenteditable></p><p contenteditable=false></p><select required></select><input placeholder=\"\">",
    );
    assert_eq!(names(&a, d, ":read-write"), "textarea,p,input");
    assert_eq!(names(&a, d, ":required"), "select");
    assert_eq!(names(&a, d, ":optional"), "input,textarea,input");
    assert_eq!(names(&a, d, ":placeholder-shown"), "input");
}

#[test]
fn closest_and_attrs() {
    // closest.phpt (HTML version), gh17802.phpt
    let mut a = Arena::new();
    let d = doc(
        &mut a,
        "<!DOCTYPE html><main><a></a><div class=foo id=div1><div id=div2><div class=bar id=div3></div></div></div></main><meta charset=\"Windows-1252\">",
    );
    let div3 = a.get_element_by_id(d, "div3");
    let div3 = div3.unwrap_or_else(|| {
        a.descendants(d)
            .find(|&n| a.get_attribute(n, "id").as_deref() == Some("div3"))
            .unwrap()
    });
    let c = |a: &Arena, sel: &str| {
        css::closest(a, div3, &css::parse(sel).unwrap()).and_then(|n| a.get_attribute(n, "id"))
    };
    assert_eq!(c(&a, "div").as_deref(), Some("div3"));
    assert_eq!(c(&a, "[class=\"foo\"]").as_deref(), Some("div1"));
    assert_eq!(c(&a, ":not(div)"), None);
    assert_eq!(c(&a, "main :not(div[class])").as_deref(), Some("div2"));
    assert_eq!(c(&a, "main > :not(div[class])"), None);
    assert_eq!(names(&a, d, "meta[charseT=\"windows-1252\"]"), "meta");
}
