//! XMLReader node stream against fixtures recorded on PHP 8.5
//! (tests/fixtures/reader.php).

mod common;
use common::*;
use goro_xml::reader::{XmlReader, node_type};

fn e(s: &str) -> String {
    // base64 of the fixture, compared as decoded text.
    String::from_utf8_lossy(&b64(s)).into_owned()
}

#[test]
fn reader_fixtures() {
    let text = fixture("reader.out");
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    let (mut total, mut fails) = (0, 0);
    while i < lines.len() {
        let Some(spec) = lines[i].strip_prefix("C ") else {
            i += 1;
            continue;
        };
        i += 1;
        let (input, props) = spec.split_once(' ').unwrap();
        let input = b64(input);
        let mut want = Vec::new();
        while i < lines.len() && !lines[i].starts_with("C ") && !lines[i].starts_with("NEXT") {
            let l = lines[i];
            let (tag, rest) = l.split_once(' ').unwrap_or((l, ""));
            match tag {
                "N" => {
                    let f: Vec<&str> = rest.split(' ').collect();
                    let mut s = f[..5].join(" ");
                    for x in &f[5..] {
                        s.push(' ');
                        s.push_str(&format!("{:?}", e(x)));
                    }
                    want.push(s);
                }
                "A" => {
                    let f: Vec<&str> = rest.split(' ').collect();
                    want.push(format!("A {:?} {:?} {:?}", e(f[0]), e(f[1]), e(f[2])));
                }
                "E" => {
                    let f: Vec<&str> = rest.split(' ').collect();
                    want.push(format!("E {} {} {}", f[0], f[1], e(f[4]).trim_end()));
                }
                "F" => want.push("F".into()),
                _ => {}
            }
            i += 1;
        }
        let mut r = XmlReader::new(input.clone(), None, 0, None);
        r.props.subst_entities = props.contains("\"4\":true");
        r.props.default_attrs = props.contains("\"2\":true");
        let mut got = Vec::new();
        for _ in 0..100 {
            let ok = r.read();
            for er in r.take_errors() {
                got.push(format!("E {} {} {}", er.level, er.code, er.text()));
            }
            if !ok {
                got.push("F".into());
                break;
            }
            let inner = r.read_inner_xml();
            let outer = r.read_outer_xml();
            let st = r.read_string();
            got.push(format!(
                "{} {} {} {} {} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}",
                r.node_type(),
                r.depth(),
                r.is_empty_element() as i32,
                r.has_value() as i32,
                r.attribute_count(),
                r.name(),
                r.local_name(),
                r.prefix(),
                r.namespace_uri(),
                r.value(),
                inner,
                outer,
                st
            ));
            if r.node_type() == node_type::ELEMENT && r.move_to_first_attribute() {
                loop {
                    got.push(format!(
                        "A {:?} {:?} {:?}",
                        r.name(),
                        r.value(),
                        r.namespace_uri()
                    ));
                    if !r.move_to_next_attribute() {
                        break;
                    }
                }
                r.move_to_element();
            }
        }
        total += 1;
        if got != want {
            fails += 1;
            eprintln!("--- {:?}", show(&input));
            for (k, (g, w)) in got.iter().zip(want.iter()).enumerate() {
                if g != w {
                    eprintln!("  first diff at {k}:\n    got  {g}\n    want {w}");
                    break;
                }
            }
            if got.len() != want.len() {
                eprintln!("  lengths {} vs {}", got.len(), want.len());
            }
        }
    }
    // next()
    let mut r = XmlReader::new(
        b"<r><a><x/></a><b/><a/><c>t</c></r>".to_vec(),
        None,
        0,
        None,
    );
    r.read();
    r.read();
    let mut s = format!("NEXT {}", r.name());
    while r.next(None) {
        s.push_str(&format!(" {}:{}", r.name(), r.node_type()));
    }
    let mut r = XmlReader::new(
        b"<r><a><x/></a><b/><a/><c>t</c></r>".to_vec(),
        None,
        0,
        None,
    );
    r.read();
    r.read();
    let mut s2 = format!("NEXTA {}", r.name());
    while r.next(Some("a")) {
        s2.push_str(&format!(" {}:{}", r.name(), r.node_type()));
    }
    total += 2;
    for want in lines.iter().filter(|l| l.starts_with("NEXT")) {
        if *want != s && *want != s2 {
            fails += 1;
            eprintln!("next: got {s:?} / {s2:?}, want {want:?}");
        }
    }
    eprintln!("reader: {}/{} pass", total - fails, total);
    assert_eq!(fails, 0);
}
