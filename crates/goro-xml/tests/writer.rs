//! XMLWriter output against fixtures recorded on PHP 8.5
//! (tests/fixtures/writer.php).

mod common;
use common::json::{Json, parse};
use common::*;
use goro_xml::writer::{WriterError, XmlWriter};

fn s(v: &Json) -> Option<&str> {
    v.as_str()
}

/// Runs one op; returns PHP's return value as the fixture records it.
fn op(w: &mut XmlWriter, m: &str, a: &[Json]) -> String {
    let r: Result<(), WriterError> = match m {
        "openMemory" => Ok(()),
        "setIndent" => {
            w.set_indent(a[0] == Json::Bool(true));
            Ok(())
        }
        "setIndentString" => {
            w.set_indent_string(s(&a[0]).unwrap());
            Ok(())
        }
        "startDocument" => w.start_document(
            a.first().and_then(s),
            a.get(1).and_then(s),
            a.get(2).and_then(s),
        ),
        "endDocument" => w.end_document(),
        "startElement" => w.start_element(s(&a[0]).unwrap()),
        "startElementNs" => w.start_element_ns(s(&a[0]), s(&a[1]).unwrap(), a.get(2).and_then(s)),
        "endElement" => w.end_element(),
        "fullEndElement" => w.full_end_element(),
        "writeAttribute" => w.write_attribute(s(&a[0]).unwrap(), s(&a[1]).unwrap()),
        "writeAttributeNs" => {
            w.write_attribute_ns(s(&a[0]), s(&a[1]).unwrap(), s(&a[2]), s(&a[3]).unwrap())
        }
        "startAttribute" => w.start_attribute(s(&a[0]).unwrap()),
        "startAttributeNs" => w.start_attribute_ns(s(&a[0]), s(&a[1]).unwrap(), s(&a[2])),
        "endAttribute" => w.end_attribute(),
        "text" => w.text(s(&a[0]).unwrap()),
        "writeRaw" => w.write_raw(s(&a[0]).unwrap()),
        "writeElement" => w.write_element(s(&a[0]).unwrap(), a.get(1).and_then(s)),
        "writeElementNs" => {
            w.write_element_ns(s(&a[0]), s(&a[1]).unwrap(), s(&a[2]), a.get(3).and_then(s))
        }
        "startCdata" => w.start_cdata(),
        "endCdata" => w.end_cdata(),
        "writeCdata" => w.write_cdata(s(&a[0]).unwrap()),
        "startComment" => w.start_comment(),
        "endComment" => w.end_comment(),
        "writeComment" => w.write_comment(s(&a[0]).unwrap()),
        "startPi" => w.start_pi(s(&a[0]).unwrap()),
        "endPi" => w.end_pi(),
        "writePi" => w.write_pi(s(&a[0]).unwrap(), s(&a[1]).unwrap()),
        "startDtd" => w.start_dtd(
            s(&a[0]).unwrap(),
            a.get(1).and_then(s),
            a.get(2).and_then(s),
        ),
        "endDtd" => w.end_dtd(),
        "writeDtd" => w.write_dtd(
            s(&a[0]).unwrap(),
            a.get(1).and_then(s),
            a.get(2).and_then(s),
            a.get(3).and_then(s),
        ),
        "writeDtdElement" => w.write_dtd_element(s(&a[0]).unwrap(), s(&a[1]).unwrap()),
        "writeDtdAttlist" => w.write_dtd_attlist(s(&a[0]).unwrap(), s(&a[1]).unwrap()),
        "writeDtdEntity" => w.write_dtd_entity(s(&a[0]).unwrap(), s(&a[1]).unwrap(), false),
        "startDtdEntity" => {
            w.start_dtd_entity(s(&a[0]).unwrap(), a.get(1) == Some(&Json::Bool(true)))
        }
        "endDtdEntity" => w.end_dtd_entity(),
        "flush" => {
            let empty = a.first() != Some(&Json::Bool(false));
            let out = w.flush(empty);
            return format!("s:{}", String::from_utf8_lossy(&out));
        }
        _ => panic!("op {m}"),
    };
    match r {
        Ok(()) => "true".into(),
        Err(WriterError::InvalidName) => "exc".into(),
        Err(_) => "false".into(),
    }
}

#[test]
fn writer_fixtures() {
    let text = fixture("writer.out");
    let mut lines = text.lines();
    let (mut total, mut fails) = (0, 0);
    while let Some(l) = lines.next() {
        let Some(ops) = l.strip_prefix("W ") else {
            continue;
        };
        let ops = parse(&String::from_utf8(b64(ops)).unwrap());
        let rets = parse(lines.next().unwrap().strip_prefix("R ").unwrap());
        let out = b64(lines.next().unwrap().strip_prefix("O ").unwrap());
        let Json::Arr(ops) = ops else { panic!() };
        let Json::Arr(rets) = rets else { panic!() };
        let mut w = XmlWriter::new();
        let mut got = Vec::new();
        let mut want = Vec::new();
        for (o, r) in ops.iter().zip(&rets) {
            let Json::Arr(o) = o else { panic!() };
            let m = o[0].as_str().unwrap();
            got.push(op(&mut w, m, &o[1..]));
            want.push(match r {
                Json::Bool(true) => "true".to_string(),
                Json::Bool(false) => "false".to_string(),
                Json::Str(x) if b64(x).starts_with(b"EXC") => "exc".to_string(),
                Json::Str(x) => format!("s:{}", String::from_utf8_lossy(&b64(x))),
                other => format!("{other:?}"),
            });
            if got.last() == Some(&"exc".to_string()) {
                break;
            }
        }
        want.truncate(got.len());
        let got_out = w.flush(true);
        total += 1;
        if got != want || got_out != out {
            fails += 1;
            eprintln!("--- {ops:?}");
            if got != want {
                eprintln!("  rets got {got:?}\n  rets want {want:?}");
            }
            if got_out != out {
                eprintln!(
                    "  out got  {:?}\n  out want {:?}",
                    show(&got_out),
                    show(&out)
                );
            }
        }
    }
    eprintln!("writer: {}/{} pass", total - fails, total);
    assert_eq!(fails, 0);
}
