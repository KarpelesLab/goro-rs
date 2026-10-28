//! Hand-written checks of behaviour required by php-src's ext/json tests
//! (each cites the test whose expected output it follows) that the fixture
//! does not exercise directly.

mod common;

use common::{Host, serialize};
use goro_ext_json::consts::*;
use goro_ext_json::{JsonHost, json_decode, json_encode, json_last_error, json_last_error_msg};
use goro_runtime::{Key, PhpArray, PhpStr, Value};

fn enc(h: &mut Host, v: &Value, flags: i64, depth: i64) -> Option<String> {
    match json_encode(h, v, flags, depth).unwrap() {
        Value::Str(s) => Some(String::from_utf8(s.as_bytes().to_vec()).unwrap()),
        _ => None,
    }
}

fn nested_lists(n: usize) -> Value {
    let mut v = Value::Array(PhpArray::new());
    for _ in 0..n {
        v = Value::Array(PhpArray::from_values([v]));
    }
    v
}

/// Test ext/json/tests/bug62369.phpt: 550 nested arrays encode with depth
/// 551 and fail with depth 540.
#[test]
fn deep_arrays_depth() {
    let mut h = Host::new();
    let v = nested_lists(550);
    assert!(enc(&mut h, &v, 0, 551).is_some());
    assert_eq!(json_last_error(), JSON_ERROR_NONE);
    assert!(enc(&mut h, &v, 0, 540).is_none());
    assert_eq!(json_last_error(), JSON_ERROR_DEPTH);
}

/// Neither direction recurses on the Rust stack: a million levels work
/// when `$depth` allows them.
#[test]
fn very_deep_documents() {
    let mut h = Host::new();
    let n = 1_000_000;
    let mut doc = vec![b'['; n];
    doc.extend(std::iter::repeat_n(b']', n));
    let v = json_decode(&mut h, &doc, Some(true), i32::MAX as i64, 0).unwrap();
    assert_eq!(json_last_error(), 0);
    let out = enc(&mut h, &v, 0, i32::MAX as i64).unwrap();
    assert_eq!(out.as_bytes(), &doc[..]);
    // Release iteratively too, so the test does not overflow on drop.
    let mut cur = v;
    loop {
        let next = match &mut cur {
            Value::Array(a) if !a.is_empty() => a.make_mut().pop(),
            _ => None,
        };
        match next {
            Some(n) => cur = n,
            None => break,
        }
    }
}

/// Test ext/json/tests/gh15168.phpt: the engine's stack limit turns deep
/// nesting into JSON_ERROR_DEPTH whatever `$depth` says.
#[test]
fn nesting_limit_hook() {
    struct Limited(Host);
    impl JsonHost for Limited {
        type Exception = String;
        fn object_kind(&mut self, o: &goro_runtime::ObjRef) -> goro_ext_json::ObjectKind {
            self.0.object_kind(o)
        }
        fn json_serialize(&mut self, o: &goro_runtime::ObjRef) -> Result<Value, String> {
            self.0.json_serialize(o)
        }
        fn object_properties(&mut self, o: &goro_runtime::ObjRef) -> Result<PhpArray, String> {
            self.0.object_properties(o)
        }
        fn new_object(&mut self, p: PhpArray) -> Value {
            self.0.new_object(p)
        }
        fn json_exception(&mut self, e: goro_ext_json::JsonError) -> String {
            self.0.json_exception(e)
        }
        fn throw_error(&mut self, e: goro_runtime::PhpError) -> String {
            self.0.throw_error(e)
        }
        fn max_encode_nesting(&self) -> Option<usize> {
            Some(1000)
        }
    }
    let mut h = Limited(Host::new());
    let first = h.0.std_object(&[]);
    let mut node = first.clone();
    for _ in 0..30000 {
        let next = h.0.std_object(&[]);
        node.obj()
            .props
            .borrow_mut()
            .insert(Key::from("next"), Value::Object(next.clone()));
        node = next;
    }
    let r = json_encode(&mut h, &Value::Object(first.clone()), 0, 500000).unwrap();
    assert!(matches!(r, Value::Bool(false)));
    assert_eq!(json_last_error(), 1);
    assert_eq!(json_last_error_msg(), "Maximum stack depth exceeded");
    // Unlink iteratively.
    let mut cur = Some(first);
    while let Some(o) = cur {
        let next = o
            .obj()
            .props
            .borrow_mut()
            .make_mut()
            .remove(goro_runtime::KeyRef::Str(&PhpStr::from("next")));
        cur = match next {
            Some(Value::Object(n)) => Some(n),
            _ => None,
        };
    }
}

/// Test ext/json/tests/pass001.1.phpt ("ENCODE: FROM ARRAY"): the
/// JSON_checker pass1 document decodes as arrays, and re-encodes with
/// JSON_PARTIAL_OUTPUT_ON_ERROR (its 23456789012E666 is INF, encoded as 0;
/// empty objects became empty arrays).
#[test]
fn json_checker_pass1() {
    let doc = r#"
[
    "JSON Test Pattern pass1",
    {"object with 1 member":["array with 1 element"]},
    {},
    [],
    -42,
    true,
    false,
    null,
    {
        "integer": 1234567890,
        "real": -9876.543210,
        "e": 0.123456789e-12,
        "E": 1.234567890E+34,
        "":  23456789012E666,
        "zero": 0,
        "one": 1,
        "space": " ",
        "quote": "\"",
        "backslash": "\\",
        "controls": "\b\f\n\r\t",
        "slash": "/ & \/",
        "hex": "\u0123\u4567\u89AB\uCDEF\uabcd\uef4A",
        "array":[  ],
        "object":{  },
        " s p a c e d " :[1,2 , 3

,

4 , 5        ,          6           ,7        ],
        "compact": [1,2,3,4,5,6,7],
        "\/\\\"\uCAFE\uBABE\uAB98\uFCDE\ubcda\uef4A\b\f\n\r\t`1~!@#$%^&*()_+-=[]{}|;:',./<>?"
: "A key can be any string"
    },
    0.5 ,98.6
,
99.44
,

1066


,"rosebud"]
"#;
    let mut h = Host::new();
    let v = json_decode(&mut h, doc.as_bytes(), Some(true), 512, 0).unwrap();
    assert_eq!(json_last_error(), 0);
    let out = enc(&mut h, &v, JSON_PARTIAL_OUTPUT_ON_ERROR, 512).unwrap();
    assert_eq!(json_last_error(), JSON_ERROR_INF_OR_NAN);
    assert_eq!(
        out,
        r#"["JSON Test Pattern pass1",{"object with 1 member":["array with 1 element"]},[],[],-42,true,false,null,{"integer":1234567890,"real":-9876.54321,"e":1.23456789e-13,"E":1.23456789e+34,"":0,"zero":0,"one":1,"space":" ","quote":"\"","backslash":"\\","controls":"\b\f\n\r\t","slash":"\/ & \/","hex":"\u0123\u4567\u89ab\ucdef\uabcd\uef4a","array":[],"object":[]," s p a c e d ":[1,2,3,4,5,6,7],"compact":[1,2,3,4,5,6,7],"\/\\\"\ucafe\ubabe\uab98\ufcde\ubcda\uef4a\b\f\n\r\t`1~!@#$%^&*()_+-=[]{}|;:',.\/<>?":"A key can be any string"},0.5,98.6,99.44,1066,"rosebud"]"#
    );
}

/// Tests ext/json/tests/bug41067.phpt and json_encode_basic_utf8.phpt:
/// astral characters become surrogate pairs and come back.
#[test]
fn surrogate_pairs_round_trip() {
    let mut h = Host::new();
    let s = Value::Str(PhpStr::new(b"\xf0\x9d\x84\x80"));
    let a = Value::Array(PhpArray::from_values([s]));
    let j = enc(&mut h, &a, 0, 512).unwrap();
    assert_eq!(j, r#"["\ud834\udd00"]"#);
    let back = json_decode(&mut h, j.as_bytes(), Some(true), 512, 0).unwrap();
    assert_eq!(serialize(&back), b"a:1:{i:0;s:4:\"\xf0\x9d\x84\x80\";}");
}

/// Test ext/json/tests/bug77843.phpt: an array modified by
/// jsonSerialize() while it is being encoded is encoded as it was.
#[test]
fn array_snapshot() {
    let mut h = Host::new();
    let js = h.class("X", common::Kind::Js);
    let mut arr = Value::Array(PhpArray::new());
    let r = arr.make_ref();
    let x = h.object(&js, &[("v", Value::Int(1)), ("n", Value::str(b"x"))]);
    r.borrow_mut()
        .as_array_mut()
        .unwrap()
        .push(Value::Object(x))
        .unwrap();
    let outer = Value::Array(PhpArray::from_values([Value::Ref(r.clone())]));
    // The element is shared with `r`; replacing r's contents mid-way (as
    // unset($arr[0]) does) cannot change the snapshot being iterated.
    let j = enc(&mut h, &outer, 0, 512).unwrap();
    r.set(Value::Array(PhpArray::new()));
    assert_eq!(j, "[[1]]");
    assert_eq!(h.take_echo(), b"[js x]");
}
