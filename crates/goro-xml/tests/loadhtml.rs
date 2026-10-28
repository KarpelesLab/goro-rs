//! DOMDocument::loadHTML()/saveHTML()/saveXML() against fixtures recorded
//! on PHP 8.5 (tests/fixtures/loadhtml.php).

mod common;
use common::*;
use goro_xml::Arena;
use goro_xml::html::{HtmlSaveOptions, parse_html, save_html};
use goro_xml::serialize::{SaveOptions, save_xml};

#[test]
fn loadhtml_fixtures() {
    let text = fixture("loadhtml.out");
    let mut cases: Vec<(Vec<u8>, u32, Vec<String>, Option<Vec<u8>>, Option<Vec<u8>>)> = Vec::new();
    for line in text.lines() {
        let (tag, rest) = line.split_once(' ').unwrap_or((line, ""));
        match tag {
            "C" => {
                let (i, o) = rest.split_once(' ').unwrap();
                cases.push((b64(i), o.parse().unwrap(), Vec::new(), None, None));
            }
            "E" => {
                let f: Vec<&str> = rest.split(' ').collect();
                let msg = String::from_utf8_lossy(&b64(f[4])).trim_end().to_string();
                cases
                    .last_mut()
                    .unwrap()
                    .2
                    .push(format!("{} {} {}", f[0], f[1], msg));
            }
            "H" => cases.last_mut().unwrap().3 = Some(b64(rest)),
            "X" => cases.last_mut().unwrap().4 = Some(b64(rest)),
            _ => {}
        }
    }
    let mut fails = 0;
    for (i, (input, flags, errs, h, x)) in cases.iter().enumerate() {
        let mut arena = Arena::new();
        let p = parse_html(&mut arena, input, *flags, None, None);
        let got_errs: Vec<String> = p
            .errors
            .iter()
            .map(|e| format!("{} {} {}", e.level, e.code, e.message.trim_end()))
            .collect();
        let gh = save_html(&arena, p.doc, HtmlSaveOptions::default());
        let gx = save_xml(&arena, p.doc, &SaveOptions::default());
        let ok = &got_errs == errs && Some(&gh) == h.as_ref() && Some(&gx) == x.as_ref();
        if !ok {
            fails += 1;
            eprintln!("--- case {i}: {:?} flags={flags}", show(input));
            if &got_errs != errs {
                eprintln!("  errs got  {got_errs:?}\n  errs want {errs:?}");
            }
            if Some(&gh) != h.as_ref() {
                eprintln!(
                    "  html got  {:?}\n  html want {:?}",
                    show(&gh),
                    h.as_deref().map(show)
                );
            }
            if Some(&gx) != x.as_ref() {
                eprintln!(
                    "  xml got  {:?}\n  xml want {:?}",
                    show(&gx),
                    x.as_deref().map(show)
                );
            }
        }
    }
    eprintln!("loadhtml: {}/{} pass", cases.len() - fails, cases.len());
    assert_eq!(fails, 0);
}
