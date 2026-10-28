//! Checks every line of tests/fixtures/json_php85.txt, the output of
//! tests/fixtures/json_driver.php on PHP 8.5 (see the driver for the line
//! formats).

mod common;

use common::{Host, Kind, esc, serialize, unesc, unserialize};
use goro_ext_json::consts::*;
use goro_ext_json::{
    json_decode, json_encode, json_last_error, json_last_error_msg, json_validate,
};
use goro_runtime::{Key, PhpArray, PhpStr, ResRef, Value};

fn field<'a>(t: &'a str, name: &str) -> &'a str {
    t.strip_prefix(name)
        .and_then(|r| r.strip_prefix('='))
        .unwrap_or_else(|| panic!("field {name} in {t}"))
}

fn decode_line(host: &mut Host, t: &[&str]) -> String {
    let a = field(t[1], "a");
    let depth: i64 = field(t[2], "d").parse().unwrap();
    let flags: i64 = field(t[3], "f").parse().unwrap();
    let json = unesc(t[4]);
    let assoc = match a {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    };
    let r = json_decode(host, &json, assoc, depth, flags).expect("no exception");
    let e = json_last_error();
    let v =
        json_validate(host, &json, depth, flags & JSON_INVALID_UTF8_IGNORE).expect("no exception");
    let ve = json_last_error();
    format!(
        "D a={a} d={depth} f={flags} {} => {} {e} v={} {ve}",
        t[4],
        esc(&serialize(&r)),
        v as i32
    )
}

fn encode_line(host: &mut Host, t: &[&str]) -> String {
    let flags: i64 = field(t[1], "f").parse().unwrap();
    let depth: i64 = field(t[2], "d").parse().unwrap();
    let prec: i32 = field(t[3], "p").parse().unwrap();
    let v = unserialize(host, &unesc(t[4]));
    host.precision = prec;
    let r = json_encode(host, &v, flags, depth).expect("no exception");
    host.precision = -1;
    let e = json_last_error();
    let out = match &r {
        Value::Bool(false) => "!false".to_string(),
        Value::Str(s) => esc(s),
        v => panic!("unexpected {v:?}"),
    };
    format!("E f={flags} d={depth} p={prec} {} => {out} {e}", t[4])
}

/// `show($r)` of the driver.
fn show(host: &mut Host, r: Result<Value, String>) {
    match r {
        Ok(v) => {
            let mut s = serialize(&v);
            s.extend_from_slice(
                format!(" {} {}", json_last_error(), json_last_error_msg()).as_bytes(),
            );
            host.echo(&s);
        }
        Err(e) => host.echo(e.as_bytes()),
    }
}

fn s(v: &str) -> Value {
    Value::Str(PhpStr::from(v))
}

fn list(v: Vec<Value>) -> Value {
    Value::Array(PhpArray::from_values(v))
}

fn scenario(host: &mut Host, name: &str) {
    let js = host.class("JS", Kind::Js);
    let mk_js =
        |host: &Host, v: Value, n: &str| Value::Object(host.object(&js, &[("v", v), ("n", s(n))]));
    let nan = Value::Float(f64::NAN);
    let partial = JSON_PARTIAL_OUTPUT_ON_ERROR;
    let bad = Value::Str(PhpStr::new(b"\xff"));
    let resource = || Value::Resource(ResRef::new(PhpStr::from("stream"), Box::new(())));
    let bad_key = || {
        Value::Array(PhpArray::from_pairs([(
            Key::Str(PhpStr::new(b"\xff")),
            Value::Int(1),
        )]))
    };
    let r = match name {
        "js_chain" => {
            let z = mk_js(host, Value::Int(5), "z");
            let y = mk_js(host, z, "y");
            let x = mk_js(host, y, "x");
            json_encode(host, &x, 0, 512)
        }
        "js_array" => {
            let inner = mk_js(host, Value::Int(2), "in");
            let a = Value::Array(PhpArray::from_pairs([
                (Key::Int(0), Value::Int(1)),
                (Key::from("a"), inner),
            ]));
            let o = mk_js(host, a, "out");
            json_encode(host, &o, 0, 512)
        }
        "js_nan" => {
            let o = mk_js(host, nan, "n");
            json_encode(host, &o, partial, 512)
        }
        "js_nan_then" => {
            let v = list(vec![nan, mk_js(host, Value::Int(1), "after")]);
            json_encode(host, &v, 0, 512)
        }
        "js_utf8_stops" | "js_utf8_partial" => {
            let v = list(vec![bad, mk_js(host, Value::Int(1), "after")]);
            json_encode(
                host,
                &v,
                if name == "js_utf8_partial" {
                    partial
                } else {
                    0
                },
                512,
            )
        }
        "js_badkey_goes_on" => {
            let v = list(vec![bad_key(), mk_js(host, Value::Int(1), "after")]);
            json_encode(host, &v, 0, 512)
        }
        "js_depth_inner" => {
            let v = list(vec![list(vec![mk_js(host, Value::Int(1), "in")])]);
            json_encode(host, &v, 0, 1)
        }
        "js_depth_stops" => {
            let v = list(vec![
                list(vec![Value::Int(1)]),
                mk_js(host, Value::Int(1), "in"),
            ]);
            json_encode(host, &v, 0, 1)
        }
        "js_depth_partial" => {
            let v = list(vec![
                list(vec![list(vec![Value::Int(1)])]),
                mk_js(host, Value::Int(1), "b"),
                nan,
            ]);
            json_encode(host, &v, partial, 1)
        }
        "js_ret_depth" | "js_ret_depth2" => {
            let o = mk_js(
                host,
                list(vec![Value::Int(1), list(vec![Value::Int(2)])]),
                "d",
            );
            json_encode(host, &o, 0, if name == "js_ret_depth" { 1 } else { 2 })
        }
        "js_scalar_depth0" => {
            let o = mk_js(host, Value::Int(5), "d");
            json_encode(host, &o, 0, 0)
        }
        "js_self" | "js_self_partial" => {
            let o = host.object(&js, &[("v", Value::Null), ("n", s("self"))]);
            o.obj()
                .props
                .borrow_mut()
                .insert(Key::from("v"), Value::Object(o.clone()));
            let r = json_encode(
                host,
                &Value::Object(o.clone()),
                if name == "js_self" { 0 } else { partial },
                512,
            );
            o.obj()
                .props
                .borrow_mut()
                .insert(Key::from("v"), Value::Null);
            r
        }
        "js_in_own_array" => {
            let o = host.object(&js, &[("v", Value::Null), ("n", s("arr"))]);
            o.obj()
                .props
                .borrow_mut()
                .insert(Key::from("v"), list(vec![Value::Object(o.clone())]));
            let r = json_encode(host, &Value::Object(o.clone()), partial, 512);
            o.obj()
                .props
                .borrow_mut()
                .insert(Key::from("v"), Value::Null);
            r
        }
        "js_twice" => {
            let o = mk_js(host, Value::Int(1), "tw");
            let v = list(vec![o.clone(), o.clone(), list(vec![o])]);
            json_encode(host, &v, 0, 512)
        }
        "js_throw" => {
            let c = host.class("JSThrow", Kind::JsThrow);
            let v = list(vec![
                Value::Object(host.object(&c, &[])),
                Value::Object(host.object(&c, &[])),
            ]);
            json_encode(host, &v, 0, 512)
        }
        "js_self_encode" => {
            let c = host.class("JSSelfEncode", Kind::JsSelfEncode);
            let o = host.object(
                &c,
                &[
                    ("a", Value::Int(1)),
                    ("\0JSSelfEncode\0b", s("hide")),
                    ("\0*\0c", s("protect")),
                ],
            );
            json_encode(host, &Value::Object(o), 0, 512)
        }
        "js_inner_plain_guard" => {
            let c = host.class("JSInnerEncode", Kind::JsInnerEncode);
            let p = host.std_object(&[]);
            let n = host.object(&c, &[("p", Value::Object(p.clone()))]);
            p.obj()
                .props
                .borrow_mut()
                .insert(Key::from("n"), Value::Object(n.clone()));
            let r = json_encode(host, &Value::Object(p.clone()), 0, 512);
            p.obj().props.borrow_mut().make_mut().clear();
            r
        }
        "js_inner_array_guard" => {
            let c = host.class("JSInnerEncode", Kind::JsInnerEncode);
            let n = host.object(&c, &[]);
            let mut arr = Value::Array(PhpArray::from_pairs([(Key::from("x"), Value::Int(1))]));
            let r = arr.make_ref();
            n.obj()
                .props
                .borrow_mut()
                .insert(Key::from("p"), Value::Ref(r.clone()));
            r.borrow_mut()
                .as_array_mut()
                .unwrap()
                .insert(Key::from("n"), Value::Object(n.clone()));
            let res = json_encode(host, &r.get(), 0, 512);
            n.obj().props.borrow_mut().make_mut().clear();
            res
        }
        "js_pretty_nested" => {
            let inner =
                json_encode(host, &list(vec![Value::Int(1)]), JSON_PRETTY_PRINT, 512).unwrap();
            let v = list(vec![mk_js(host, inner, "p")]);
            json_encode(host, &v, 0, 512)
        }
        "obj_recursion" | "obj_recursion_partial" => {
            let a = host.std_object(&[]);
            a.obj()
                .props
                .borrow_mut()
                .insert(Key::from("prop"), Value::Object(a.clone()));
            let r = json_encode(
                host,
                &Value::Object(a.clone()),
                if name == "obj_recursion" { 0 } else { partial },
                512,
            );
            a.obj().props.borrow_mut().make_mut().clear();
            r
        }
        "obj_shared" => {
            let q = Value::Object(host.std_object(&[("x", Value::Int(1))]));
            let v = list(vec![q.clone(), q.clone(), list(vec![q])]);
            json_encode(host, &v, 0, 512)
        }
        "obj_in_array_cycle" => {
            let p = host.std_object(&[]);
            p.obj()
                .props
                .borrow_mut()
                .insert(Key::from("a"), list(vec![Value::Object(p.clone())]));
            let r = json_encode(host, &Value::Object(p.clone()), partial, 512);
            p.obj().props.borrow_mut().make_mut().clear();
            r
        }
        "arr_ref_recursion" | "arr_ref_recursion_partial" => {
            // $a = [1]; $a[] = &$a;
            let mut a = list(vec![Value::Int(1)]);
            let r = a.make_ref();
            let inner = Value::Ref(r.clone());
            r.borrow_mut().as_array_mut().unwrap().push(inner).unwrap();
            let res = json_encode(
                host,
                &r.get(),
                if name == "arr_ref_recursion" {
                    0
                } else {
                    partial
                },
                512,
            );
            r.borrow_mut()
                .as_array_mut()
                .unwrap()
                .remove(goro_runtime::KeyRef::Int(1));
            res
        }
        "arr_ref_siblings" => {
            let mut b = list(vec![Value::Int(1)]);
            let r = b.make_ref();
            let c = list(vec![Value::Ref(r.clone()), Value::Ref(r)]);
            json_encode(host, &c, 0, 512)
        }
        "arr_ref_elem" => {
            let mut a = PhpArray::from_values([Value::Int(1), Value::Int(2)]);
            let _r = a
                .make_mut()
                .get_mut(goro_runtime::KeyRef::Int(0))
                .unwrap()
                .make_ref();
            json_encode(host, &Value::Array(a), 0, 512)
        }
        "visibility" => {
            let c = host.class("Vis", Kind::Plain);
            let o = host.object(
                &c,
                &[
                    ("a", Value::Int(1)),
                    ("\0*\0b", Value::Int(2)),
                    ("\0Vis\0c", Value::Int(3)),
                    ("e", Value::Null),
                ],
            );
            json_encode(host, &Value::Object(o), 0, 512)
        }
        "enum_unit" | "enum_unit_partial" => {
            let c = host.class("Unit", Kind::UnitEnum);
            let e = Value::Object(host.object(&c, &[("name", s("A"))]));
            if name == "enum_unit" {
                json_encode(host, &e, 0, 512)
            } else {
                json_encode(host, &list(vec![e, Value::Int(1)]), partial, 512)
            }
        }
        "enum_backed" => {
            let cs = host.class("BStr", Kind::BackedEnum);
            let ci = host.class("BInt", Kind::BackedEnum);
            let a = Value::Object(host.object(&cs, &[("name", s("A")), ("value", s("x"))]));
            let q = Value::Object(host.object(&cs, &[("name", s("Q")), ("value", s("a\"b"))]));
            let i = Value::Object(host.object(&ci, &[("name", s("A")), ("value", Value::Int(5))]));
            let v = Value::Array(PhpArray::from_pairs([
                (Key::Int(0), a),
                (Key::Int(1), i),
                (Key::from("k"), q),
            ]));
            json_encode(host, &v, JSON_HEX_QUOT, 512)
        }
        "resource" => json_encode(host, &resource(), 0, 512),
        "resource_partial" => {
            let v = Value::Array(PhpArray::from_pairs([(Key::from("a"), resource())]));
            json_encode(host, &v, partial, 512)
        }
        "throw_encode" => json_encode(host, &Value::str(b"\x80"), JSON_THROW_ON_ERROR, 512),
        "throw_partial_encode" => json_encode(
            host,
            &Value::str(b"\x80"),
            JSON_THROW_ON_ERROR | partial,
            512,
        ),
        "throw_decode" => json_decode(host, b"{", Some(false), 512, JSON_THROW_ON_ERROR),
        "throw_decode_empty" => json_decode(host, b"", Some(false), 0, JSON_THROW_ON_ERROR),
        "state_throw_ok" => {
            let _ = json_decode(host, b"\xff", None, 512, 0);
            json_encode(host, &Value::Int(1), JSON_THROW_ON_ERROR, 512)
        }
        "state_throw_decode_ok" => {
            let _ = json_decode(host, b"\xff", None, 512, 0);
            json_decode(host, b"[1]", Some(false), 512, JSON_THROW_ON_ERROR)
        }
        "state_throw_fail" | "state_throw_decode_fail" => {
            let _ = json_decode(host, b"\xff", None, 512, 0);
            let r = if name == "state_throw_fail" {
                json_encode(host, &nan, JSON_THROW_ON_ERROR, 512)
            } else {
                json_decode(host, b"", Some(false), 512, JSON_THROW_ON_ERROR)
            };
            let e = r.unwrap_err();
            // "JsonException: <message> (<code>)" -> "<message><code> "
            let body = e.strip_prefix("JsonException: ").unwrap();
            let (msg, code) = body.rsplit_once(" (").unwrap();
            host.echo(format!("{msg}{} ", code.trim_end_matches(')')).as_bytes());
            Ok(Value::Int(1))
        }
        "state_decode_valueerror"
        | "state_decode_valueerror_throw"
        | "state_validate_valueerror" => {
            let _ = json_decode(host, b"\xff", None, 512, 0);
            let e = match name {
                "state_decode_valueerror" => {
                    json_decode(host, b"1", Some(false), 0, 0).unwrap_err()
                }
                "state_decode_valueerror_throw" => {
                    json_decode(host, b"1", Some(false), 0, JSON_THROW_ON_ERROR).unwrap_err()
                }
                _ => json_validate(host, b"1", 0, 0).unwrap_err(),
            };
            let msg = e
                .strip_prefix("ValueError: ")
                .unwrap()
                .strip_suffix(" (0)")
                .unwrap();
            host.echo(format!("{msg} ").as_bytes());
            Ok(Value::Int(1))
        }
        "state_encode_reset" => {
            let _ = json_decode(host, b"\xff", None, 512, 0);
            json_encode(host, &Value::Int(1), 0, -5)
        }
        "decode_depth_big" => json_decode(host, b"[1]", Some(false), 2147483648, 0),
        "decode_depth_max" => json_decode(host, b"[1]", Some(false), 2147483647, 0),
        "decode_depth_neg" => json_decode(host, b"[1]", Some(false), -1, 0),
        "decode_empty_depth0" => json_decode(host, b"", Some(false), 0, 0),
        "decode_flags_unknown" => json_decode(host, b"[1]", Some(true), 512, -1),
        "validate_empty_depth0" => json_validate(host, b"", 0, 0).map(Value::Bool),
        "validate_depth_big" => json_validate(host, b"1", 2147483648, 0).map(Value::Bool),
        "validate_flags" => json_validate(host, b"1", 512, JSON_BIGINT_AS_STRING).map(Value::Bool),
        "validate_flags_depth0" => {
            json_validate(host, b"1", 0, JSON_BIGINT_AS_STRING).map(Value::Bool)
        }
        "validate_empty_badflags" => json_validate(host, b"", 512, 2).map(Value::Bool),
        "validate_ignore" => {
            json_validate(host, b"[\"\xff\"]", 512, JSON_INVALID_UTF8_IGNORE).map(Value::Bool)
        }
        "encode_depth_intmax" => json_encode(host, &list(vec![Value::Int(1)]), 0, i64::MAX),
        "encode_depth_2p31" => json_encode(host, &list(vec![Value::Int(1)]), 0, 2147483648),
        "encode_depth_2p32p1" => json_encode(host, &list(vec![Value::Int(1)]), 0, 4294967297),
        "last_error_msgs" => {
            for j in [
                &b"[1"[..],
                b"[}",
                b"\"\x01\"",
                b"\xff",
                b"\"\\ud800\"",
                b"{\"\\u0000\":1}",
                b"[[1]]",
            ] {
                let _ = json_decode(host, j, Some(false), 2, 0);
                let line = format!("{}:{};", json_last_error(), json_last_error_msg());
                host.echo(line.as_bytes());
            }
            return;
        }
        "decode_object_numeric_keys" => {
            json_decode(host, br#"{"0":1,"1":2,"a":{"2":3}}"#, None, 512, 0)
        }
        "decode_object_dup" => json_decode(host, br#"{"a":1,"b":2,"a":3}"#, None, 512, 0),
        "encode_decoded" => {
            let v =
                json_decode(host, br#"{"0":1,"":2,"a":{"b":[]},"c":{}}"#, None, 512, 0).unwrap();
            json_encode(host, &v, 0, 512)
        }
        _ => panic!("unknown scenario {name}"),
    };
    show(host, r);
}

#[test]
fn php85_fixture() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/json_php85.txt");
    let text = std::fs::read_to_string(path).expect("fixture");
    let mut host = Host::new();
    let mut failures = Vec::new();
    let mut counts = [0usize; 3];
    for (n, line) in text.lines().enumerate() {
        let t: Vec<&str> = line.split(' ').collect();
        let got = match t[0] {
            "D" => {
                counts[0] += 1;
                decode_line(&mut host, &t)
            }
            "E" => {
                counts[1] += 1;
                encode_line(&mut host, &t)
            }
            "X" => {
                counts[2] += 1;
                host.take_echo();
                scenario(&mut host, t[1]);
                format!("X {} => {}", t[1], esc(&host.take_echo()))
            }
            _ => panic!("line {}: unknown kind", n + 1),
        };
        if got != line {
            failures.push(format!("line {}:\n  want {line}\n  got  {got}", n + 1));
        }
    }
    let total: usize = counts.iter().sum();
    if !failures.is_empty() {
        for f in failures.iter().take(60) {
            eprintln!("{f}");
        }
        panic!(
            "{} of {total} fixture lines differ (D {}, E {}, X {})",
            failures.len(),
            counts[0],
            counts[1],
            counts[2]
        );
    }
    eprintln!(
        "{total} fixture lines match (D {}, E {}, X {})",
        counts[0], counts[1], counts[2]
    );
}
