//! Behaviour from php-src's ext/filter/tests/*.phpt expected output that
//! the fixture can't express (input arrays, callbacks, objects with
//! `__toString()`, references), each case citing its test.

mod common;

use common::{Exc, Host, Kind, serialize};
use goro_ext_filter::consts::*;
use goro_ext_filter::{
    InputFilter, InputType, Options, filter_has_var, filter_id, filter_input, filter_input_array,
    filter_list, filter_var, filter_var_array,
};
use goro_runtime::{Key, PhpArray, PhpRef, PhpStr, Value};

fn s(v: &str) -> Value {
    Value::Str(PhpStr::from(v))
}

fn assoc(pairs: &[(&str, Value)]) -> PhpArray {
    PhpArray::from_pairs(
        pairs
            .iter()
            .map(|(k, v)| (Key::from_bytes(k.as_bytes()), v.clone())),
    )
}

fn ser(v: &Value) -> String {
    String::from_utf8(serialize(v)).unwrap()
}

fn ok(r: Result<Value, Exc>) -> String {
    ser(&r.expect("no exception"))
}

fn err(r: Result<Value, Exc>) -> String {
    let (c, m) = r.expect_err("exception").describe();
    format!("{c}: {}", String::from_utf8(m).unwrap())
}

fn get_post_host() -> Host {
    let mut h = Host::new();
    h.set_input(
        InputType::Get,
        assoc(&[
            ("a", s("<b>test</b>")),
            ("b", s("http://example.com")),
            (
                "ar",
                Value::Array(assoc(&[("elm1", s("1234")), ("elm2", s("0660"))])),
            ),
        ]),
    );
    h.set_input(
        InputType::Post,
        assoc(&[("c", s("<p>string</p>")), ("d", s("12345.7"))]),
    );
    h
}

#[test]
fn filter_list_and_id() {
    // tests 008.phpt, 009.phpt
    let l = filter_list();
    assert_eq!(l.len(), 21);
    assert_eq!(ser(&Value::Array(l)).matches("s:").count(), 21);
    assert_eq!(filter_id(b"stripped"), Some(513));
    assert_eq!(filter_id(b"string"), Some(513));
    assert_eq!(filter_id(b"url"), Some(518));
    assert_eq!(filter_id(b"int"), Some(257));
    assert_eq!(filter_id(b"none"), None);
    assert_eq!(filter_id(b"-1"), None);
    assert_eq!(filter_id(b"int\0x"), Some(257));
}

#[test]
fn filter_input_basics() {
    // test 011.phpt
    let mut h = get_post_host();
    assert_eq!(
        ok(filter_input(
            &mut h,
            INPUT_GET,
            b"b",
            FILTER_SANITIZE_URL,
            Options::Int(0)
        )),
        r#"s:18:"http://example.com";"#
    );
    let opts = PhpArray::from_values([1, 2, 3, 4, 5].map(Value::Int));
    assert_eq!(
        ok(filter_input(
            &mut h,
            INPUT_GET,
            b"a",
            FILTER_SANITIZE_SPECIAL_CHARS,
            Options::Array(&opts)
        )),
        r#"s:27:"&#60;b&#62;test&#60;/b&#62;";"#
    );
    assert_eq!(
        ok(filter_input(
            &mut h,
            INPUT_POST,
            b"d",
            FILTER_VALIDATE_FLOAT,
            Options::Int(0)
        )),
        "d:12345.7;"
    );
    assert_eq!(
        ok(filter_input(
            &mut h,
            INPUT_POST,
            b"d",
            FILTER_VALIDATE_INT,
            Options::Int(0)
        )),
        "b:0;"
    );
    // test 035.phpt
    let o = assoc(&[(
        "flags",
        Value::Int(FILTER_FLAG_ALLOW_OCTAL | FILTER_REQUIRE_ARRAY),
    )]);
    assert_eq!(
        ok(filter_input(
            &mut h,
            INPUT_GET,
            b"ar",
            FILTER_VALIDATE_INT,
            Options::Array(&o)
        )),
        r#"a:2:{s:4:"elm1";i:1234;s:4:"elm2";i:432;}"#
    );
    // test 012.phpt: missing variables
    let mut cli = Host::new();
    assert_eq!(
        ok(filter_input(
            &mut cli,
            INPUT_GET,
            b"test",
            FILTER_DEFAULT,
            Options::Int(0)
        )),
        "N;"
    );
    assert_eq!(
        ok(filter_input(
            &mut cli,
            INPUT_COOKIE,
            b"",
            FILTER_DEFAULT,
            Options::Int(FILTER_NULL_ON_FAILURE)
        )),
        "b:0;"
    );
    // test bug50632.phpt
    let o = assoc(&[
        ("flags", Value::Int(FILTER_REQUIRE_SCALAR)),
        (
            "options",
            Value::Array(assoc(&[("default", Value::Int(23))])),
        ),
    ]);
    assert_eq!(
        ok(filter_input(
            &mut cli,
            INPUT_GET,
            b"foo",
            FILTER_VALIDATE_INT,
            Options::Array(&o)
        )),
        "i:23;"
    );
}

#[test]
fn filter_input_errors() {
    let mut h = Host::new();
    // observed on PHP 8.5: the filter id is checked before the type
    assert_eq!(
        ok(filter_input(&mut h, 99, b"a", 9999, Options::Int(0))),
        "b:0;"
    );
    assert_eq!(
        h.take_warnings(),
        ["filter_input(): Unknown filter with ID 9999"]
    );
    assert_eq!(
        err(filter_input(
            &mut h,
            3,
            b"a",
            FILTER_DEFAULT,
            Options::Int(0)
        )),
        "ValueError: filter_input(): Argument #1 ($type) must be an INPUT_* constant"
    );
    // test throw-on-failure/throw-and-null-error.phpt
    let both = FILTER_THROW_ON_FAILURE | FILTER_NULL_ON_FAILURE;
    assert_eq!(
        err(filter_input(
            &mut h,
            INPUT_GET,
            b"b",
            FILTER_DEFAULT,
            Options::Int(both)
        )),
        "ValueError: filter_input(): Argument #4 ($options) cannot use both FILTER_NULL_ON_FAILURE and FILTER_THROW_ON_FAILURE"
    );
    // test 040.phpt
    assert_eq!(
        err(Ok(Value::Null).and(filter_has_var(&mut h, -1, b"cc").map(Value::Bool))),
        "ValueError: filter_has_var(): Argument #1 ($input_type) must be an INPUT_* constant"
    );
}

#[test]
fn filter_has_var_cases() {
    // tests 007.phpt, 040.phpt
    let mut h = Host::new();
    h.set_input(
        InputType::Get,
        assoc(&[("a", s("1")), ("b", s("2")), ("c", s("0"))]),
    );
    h.set_input(
        InputType::Post,
        assoc(&[
            ("ap", Value::Array(PhpArray::from_values([s("1")]))),
            ("bp", s("test")),
            ("cp", s("")),
        ]),
    );
    assert!(!filter_has_var(&mut h, INPUT_GET, b"").unwrap());
    assert!(filter_has_var(&mut h, INPUT_POST, b"ap").unwrap());
    assert!(filter_has_var(&mut h, INPUT_POST, b"cp").unwrap());
    assert!(filter_has_var(&mut h, INPUT_GET, b"c").unwrap());
    assert!(!filter_has_var(&mut h, INPUT_GET, b"abc").unwrap());
    assert!(!filter_has_var(&mut h, INPUT_POST, b"cc").unwrap());
}

#[test]
fn throw_on_failure() {
    // tests throw-on-failure/*.phpt
    let mut h = Host::new();
    h.set_input(InputType::Get, assoc(&[("a", s("1"))]));
    let t = Options::Int(FILTER_THROW_ON_FAILURE);
    assert_eq!(
        err(filter_input(&mut h, INPUT_GET, b"b", FILTER_DEFAULT, t)),
        "Filter\\FilterFailedException: input value 'b' not found"
    );
    assert_eq!(
        err(filter_input(
            &mut h,
            INPUT_GET,
            b"a",
            FILTER_DEFAULT,
            Options::Int(FILTER_REQUIRE_ARRAY | FILTER_THROW_ON_FAILURE)
        )),
        "Filter\\FilterFailedException: filter validation failed: not an array (got string)"
    );
    assert_eq!(
        err(filter_input(
            &mut h,
            INPUT_GET,
            b"a",
            FILTER_VALIDATE_EMAIL,
            t
        )),
        "Filter\\FilterFailedException: filter validation failed: filter validate_email not satisfied by '1'"
    );
    let spec = assoc(&[(
        "a",
        Value::Array(assoc(&[
            ("filter", Value::Int(FILTER_VALIDATE_EMAIL)),
            ("flags", Value::Int(FILTER_THROW_ON_FAILURE)),
        ])),
    )]);
    assert_eq!(
        err(filter_input_array(
            &mut h,
            INPUT_GET,
            Options::Array(&spec),
            true
        )),
        "Filter\\FilterFailedException: filter validation failed: filter validate_email not satisfied by '1'"
    );
    let std = h.std_object();
    assert_eq!(
        err(filter_var(&mut h, &std, FILTER_VALIDATE_EMAIL, t)),
        "Filter\\FilterFailedException: filter validation failed: object of type stdClass has no __toString() method"
    );
    // filter_success.phpt: an object with __toString()
    let my = h.object(
        "MyString",
        Kind::Str(PhpStr::from("daniel.e.scherzer@gmail.com")),
    );
    assert_eq!(
        ok(filter_var(&mut h, &my, FILTER_VALIDATE_EMAIL, t)),
        r#"s:27:"daniel.e.scherzer@gmail.com";"#
    );
    let data = assoc(&[("a", h.object("MyString", Kind::Str(PhpStr::from("bar"))))]);
    let spec = assoc(&[(
        "a",
        Value::Array(assoc(&[("flags", Value::Int(FILTER_THROW_ON_FAILURE))])),
    )]);
    assert_eq!(
        ok(filter_var_array(&mut h, &data, Options::Array(&spec), true)),
        r#"a:1:{s:1:"a";s:3:"bar";}"#
    );
    // a __toString() exception propagates
    let thrower = h.object("E", Kind::Throws("boom".into()));
    assert_eq!(
        err(filter_var(
            &mut h,
            &thrower,
            FILTER_DEFAULT,
            Options::Int(0)
        )),
        "Exception: boom"
    );
}

#[test]
fn callbacks() {
    // test 029.phpt
    let mut h = Host::new();
    let o = assoc(&[("options", s("test"))]);
    assert_eq!(
        ok(filter_var(
            &mut h,
            &s("data"),
            FILTER_CALLBACK,
            Options::Array(&o)
        )),
        r#"s:4:"DATA";"#
    );
    let bad = assoc(&[("options", s("no such func"))]);
    assert_eq!(
        err(filter_var(
            &mut h,
            &s("qwe"),
            FILTER_CALLBACK,
            Options::Array(&bad)
        )),
        "TypeError: filter_var(): Option must be a valid callback"
    );
    assert_eq!(
        err(filter_var(
            &mut h,
            &s("qwe"),
            FILTER_CALLBACK,
            Options::Int(0)
        )),
        "TypeError: filter_var(): Option must be a valid callback"
    );
    let m = assoc(&[(
        "options",
        Value::Array(PhpArray::from_values([s("test_class"), s("test")])),
    )]);
    assert_eq!(
        ok(filter_var(
            &mut h,
            &s("dAtA"),
            FILTER_CALLBACK,
            Options::Array(&m)
        )),
        r#"s:4:"data";"#
    );
    let t1 = assoc(&[("options", s("test1"))]);
    assert_eq!(
        ok(filter_var(
            &mut h,
            &s("data"),
            FILTER_CALLBACK,
            Options::Array(&t1)
        )),
        "N;"
    );
    // the callback's exception propagates
    let thrower = h.closure(|_, _| Err(Exc::User("Exception".into(), "test".into())));
    let o = assoc(&[("options", thrower)]);
    assert_eq!(
        err(filter_var(
            &mut h,
            &s("data"),
            FILTER_CALLBACK,
            Options::Array(&o)
        )),
        "Exception: test"
    );
    // observed on PHP 8.5: the callback gets the string form of scalars,
    // arrays are filtered element by element (test 052.phpt), an object
    // without __toString() fails before the callback is looked at
    let seen = h.closure(|h, v| {
        h.echo.extend_from_slice(&serialize(&v));
        Ok(v)
    });
    let o = assoc(&[("options", seen)]);
    let nested = Value::Array(PhpArray::from_values([
        Value::Int(1),
        Value::Array(PhpArray::from_values([
            Value::Float(2.5),
            Value::Bool(true),
        ])),
    ]));
    assert_eq!(
        ok(filter_var(
            &mut h,
            &nested,
            FILTER_CALLBACK,
            Options::Array(&o)
        )),
        r#"a:2:{i:0;s:1:"1";i:1;a:2:{i:0;s:3:"2.5";i:1;s:1:"1";}}"#
    );
    assert_eq!(
        String::from_utf8(std::mem::take(&mut h.echo)).unwrap(),
        r#"s:1:"1";s:3:"2.5";s:1:"1";"#
    );
    let std = h.std_object();
    assert_eq!(
        ok(filter_var(
            &mut h,
            &std,
            FILTER_CALLBACK,
            Options::Array(&bad)
        )),
        "b:0;"
    );
    assert_eq!(
        ok(filter_var(
            &mut h,
            &Value::Array(PhpArray::new()),
            FILTER_CALLBACK,
            Options::Array(&bad)
        )),
        "a:0:{}"
    );
    // test filter_callback_require_scalar.phpt
    let id = h.closure(|_, v| Ok(v));
    let o = assoc(&[
        ("options", id),
        ("flags", Value::Int(FILTER_REQUIRE_SCALAR)),
    ]);
    assert_eq!(
        ok(filter_var(
            &mut h,
            &s("test"),
            FILTER_CALLBACK,
            Options::Array(&o)
        )),
        r#"s:4:"test";"#
    );
    assert_eq!(
        ok(filter_var(
            &mut h,
            &Value::Array(PhpArray::from_values([s("test")])),
            FILTER_CALLBACK,
            Options::Array(&o)
        )),
        "b:0;"
    );
    // test 037.phpt
    let mut g = Host::new();
    g.set_input(InputType::Get, assoc(&[("a", s("1")), ("b", s("2"))]));
    let spec = assoc(&[
        (
            "a",
            Value::Array(assoc(&[
                ("filter", Value::Int(FILTER_CALLBACK)),
                ("options", s("myfunc")),
            ])),
        ),
        ("b", Value::Int(FILTER_VALIDATE_INT)),
    ]);
    assert_eq!(
        ok(filter_input_array(
            &mut g,
            INPUT_GET,
            Options::Array(&spec),
            true
        )),
        r#"a:2:{s:1:"a";s:10:"1_callback";s:1:"b";i:2;}"#
    );
}

#[test]
fn objects_and_resources() {
    // tests 014.phpt, bug49274.phpt, bug67167.01.phpt, bug73054.phpt
    let mut h = Host::new();
    let t = h.object("test", Kind::Str(PhpStr::from("blah")));
    assert_eq!(
        ok(filter_var(
            &mut h,
            &t,
            FILTER_VALIDATE_BOOL,
            Options::Int(0)
        )),
        "b:0;"
    );
    let std = h.std_object();
    assert_eq!(
        ok(filter_var(
            &mut h,
            &std,
            FILTER_VALIDATE_EMAIL,
            Options::Int(0)
        )),
        "b:0;"
    );
    assert_eq!(
        ok(filter_var(
            &mut h,
            &std,
            FILTER_VALIDATE_BOOL,
            Options::Int(FILTER_NULL_ON_FAILURE)
        )),
        "N;"
    );
    let o = assoc(&[(
        "options",
        Value::Array(assoc(&[("default", Value::Int(2))])),
    )]);
    assert_eq!(
        ok(filter_var(
            &mut h,
            &std,
            FILTER_VALIDATE_INT,
            Options::Array(&o)
        )),
        "i:2;"
    );
    let o = assoc(&[
        (
            "options",
            Value::Array(assoc(&[("default", Value::Int(2))])),
        ),
        ("flags", Value::Int(FILTER_NULL_ON_FAILURE)),
    ]);
    assert_eq!(
        ok(filter_var(
            &mut h,
            &std,
            FILTER_VALIDATE_INT,
            Options::Array(&o)
        )),
        "i:2;"
    );
}

#[test]
fn references() {
    // test 036.phpt: a referenced element named in the spec is copied
    let mut h = Host::new();
    let var = PhpRef::new(s("1"));
    let data = assoc(&[("test1", Value::Int(1)), ("test2", Value::Ref(var.clone()))]);
    let spec = assoc(&[
        ("test1", Value::Int(FILTER_VALIDATE_INT)),
        ("test2", Value::Int(FILTER_VALIDATE_INT)),
    ]);
    assert_eq!(
        ok(filter_var_array(&mut h, &data, Options::Array(&spec), true)),
        r#"a:2:{s:5:"test1";i:1;s:5:"test2";i:1;}"#
    );
    assert_eq!(ser(&var.get()), r#"s:1:"1";"#);
    // test filter_var_array_with_ref.phpt: an int filter changes
    // referenced arrays in place
    let inner = PhpRef::new(Value::Array(PhpArray::from_values([s("123foo")])));
    let outer = PhpArray::from_values([Value::Ref(inner.clone())]);
    let r = filter_var_array(&mut h, &outer, Options::Int(FILTER_VALIDATE_INT), true).unwrap();
    assert_eq!(ser(&r), "a:1:{i:0;a:1:{i:0;b:0;}}");
    let Value::Array(ra) = &r else { panic!() };
    assert!(ra.get_int(0).unwrap().is_ref());
    assert_eq!(ser(&inner.get()), "a:1:{i:0;b:0;}");
    // observed on PHP 8.5: a reference cycle is visited once
    let cyc = PhpRef::new(Value::Null);
    cyc.set(Value::Array(PhpArray::from_values([
        s("1"),
        Value::Ref(cyc.clone()),
    ])));
    let top = PhpArray::from_values([s("1"), Value::Ref(cyc.clone())]);
    let r = filter_var(
        &mut h,
        &Value::Array(top),
        FILTER_VALIDATE_INT,
        Options::Int(FILTER_REQUIRE_ARRAY),
    )
    .unwrap();
    let Value::Array(ra) = &r else { panic!() };
    assert_eq!(ser(ra.get_int(0).unwrap()), "i:1;");
    let Value::Array(c) = cyc.get() else { panic!() };
    assert_eq!(ser(c.get_int(0).unwrap()), "i:1;");
    // break the cycle for the test's own cleanup
    cyc.set(Value::Null);
    // bug76366.phpt: options reached through a reference
    let opts = PhpRef::new(Value::Array(assoc(&[
        ("min_range", Value::Int(1)),
        ("max_range", Value::Int(5)),
    ])));
    let spec = assoc(&[(
        "foo",
        Value::Array(assoc(&[
            ("filter", Value::Int(FILTER_VALIDATE_INT)),
            ("flags", Value::Int(FILTER_FORCE_ARRAY)),
            ("options", Value::Ref(opts)),
        ])),
    )]);
    let data = assoc(&[("foo", Value::Int(6))]);
    assert_eq!(
        ok(filter_var_array(&mut h, &data, Options::Array(&spec), true)),
        r#"a:1:{s:3:"foo";a:1:{i:0;b:0;}}"#
    );
}

#[test]
fn var_array_errors() {
    // test 039.phpt
    let mut h = Host::new();
    let empty = PhpArray::new();
    assert_eq!(
        ok(filter_var_array(&mut h, &empty, Options::Int(-1), true)),
        "b:0;"
    );
    assert_eq!(
        h.take_warnings(),
        ["filter_var_array(): Unknown filter with ID -1"]
    );
    let data = assoc(&[("var_name", s(""))]);
    let spec = assoc(&[
        ("var_name", Value::Int(-1)),
        ("asdas", s("asdasd")),
        ("qwe", s("rty")),
        ("", s("")),
    ]);
    assert_eq!(
        err(filter_var_array(&mut h, &data, Options::Array(&spec), true)),
        "ValueError: filter_var_array(): Argument #2 ($options) cannot contain empty keys"
    );
    assert_eq!(
        h.take_warnings(),
        ["filter_var_array(): Unknown filter with ID -1"]
    );
    // test 054.phpt
    let data = assoc(&[("foo", Value::Int(123))]);
    let spec = assoc(&[
        (
            "foo",
            Value::Array(assoc(&[("filter", Value::Int(FILTER_DEFAULT))])),
        ),
        (
            "bar",
            Value::Array(assoc(&[("filter", Value::Int(FILTER_DEFAULT))])),
        ),
    ]);
    assert_eq!(
        ok(filter_var_array(
            &mut h,
            &data,
            Options::Array(&spec),
            false
        )),
        r#"a:1:{s:3:"foo";s:3:"123";}"#
    );
    assert_eq!(
        ok(filter_var_array(&mut h, &data, Options::Array(&spec), true)),
        r#"a:2:{s:3:"foo";s:3:"123";s:3:"bar";N;}"#
    );
    // filter_input_array_001.phpt
    let spec = assoc(&[(
        "c",
        Value::Array(assoc(&[("flags", Value::Int(FILTER_NULL_ON_FAILURE))])),
    )]);
    assert_eq!(
        ok(filter_input_array(
            &mut h,
            INPUT_GET,
            Options::Array(&spec),
            true
        )),
        "N;"
    );
}

#[test]
fn float_precision_and_nan() {
    // observed on PHP 8.5: floats convert with ini precision; NAN warns
    let mut h = Host::new();
    let f = Value::Float(0.1 + 0.2);
    assert_eq!(
        ok(filter_var(&mut h, &f, FILTER_DEFAULT, Options::Int(0))),
        r#"s:3:"0.3";"#
    );
    h.precision = 17;
    assert_eq!(
        ok(filter_var(&mut h, &f, FILTER_DEFAULT, Options::Int(0))),
        r#"s:19:"0.30000000000000004";"#
    );
    h.precision = 14;
    assert_eq!(
        ok(filter_var(
            &mut h,
            &Value::Float(f64::NAN),
            FILTER_VALIDATE_INT,
            Options::Int(0)
        )),
        "b:0;"
    );
    assert_eq!(
        h.take_warnings(),
        ["unexpected NAN value was coerced to string"]
    );
}

#[test]
fn full_special_chars_charset() {
    // observed on PHP 8.5: ini default_charset picks the charset
    let mut h = Host::new();
    let v = s("é<'\"&amp;");
    assert_eq!(
        ok(filter_var(
            &mut h,
            &v,
            FILTER_SANITIZE_FULL_SPECIAL_CHARS,
            Options::Int(0)
        )),
        r#"s:29:"&eacute;&lt;&#039;&quot;&amp;";"#
    );
    h.charset = b"ISO-8859-1".to_vec();
    let v = Value::Str(PhpStr::new(b"\xff<\xe9"));
    assert_eq!(
        ok(filter_var(
            &mut h,
            &v,
            FILTER_SANITIZE_FULL_SPECIAL_CHARS,
            Options::Int(0)
        )),
        r#"s:18:"&yuml;&lt;&eacute;";"#
    );
}

#[test]
fn regexp_warnings() {
    // observed on PHP 8.5: compile errors warn for every value
    let mut h = Host::new();
    let o = assoc(&[
        ("options", Value::Array(assoc(&[("regexp", s("/(/"))]))),
        ("flags", Value::Int(FILTER_REQUIRE_ARRAY)),
    ]);
    let v = Value::Array(PhpArray::from_values([s("a"), s("b")]));
    assert_eq!(
        ok(filter_var(
            &mut h,
            &v,
            FILTER_VALIDATE_REGEXP,
            Options::Array(&o)
        )),
        "a:2:{i:0;b:0;i:1;b:0;}"
    );
    assert_eq!(
        h.take_warnings(),
        [
            "filter_var(): Compilation failed: missing closing parenthesis at offset 1",
            "filter_var(): Compilation failed: missing closing parenthesis at offset 1"
        ]
    );
    // test 017.phpt
    assert_eq!(
        err(filter_var(
            &mut h,
            &s("data"),
            FILTER_VALIDATE_REGEXP,
            Options::Int(0)
        )),
        "ValueError: filter_var(): \"regexp\" option is missing"
    );
}

#[test]
fn sapi_input_filter() {
    // tests 004.phpt, 041.phpt; PHP manual: filter.default_flags defaults
    // to FILTER_FLAG_NO_ENCODE_QUOTES
    let mut h = Host::new();
    let f = InputFilter::from_ini(b"special_chars", None);
    assert_eq!(
        ok(f.apply(&mut h, &PhpStr::from("O'Henry"))),
        r#"s:11:"O&#39;Henry";"#
    );
    let f = InputFilter::from_ini(b"stripped", Some(0));
    assert_eq!(
        ok(f.apply(&mut h, &PhpStr::from("\"foo bar\""))),
        r#"s:17:"&#34;foo bar&#34;";"#
    );
    let f = InputFilter::from_ini(b"stripped", None);
    assert_eq!(
        ok(f.apply(
            &mut h,
            &PhpStr::from("http://fifaworldcup.yahoo.com/03/en/photozone/index.html")
        )),
        r#"s:56:"http://fifaworldcup.yahoo.com/03/en/photozone/index.html";"#
    );
    assert!(InputFilter::from_ini(b"unsafe_raw", None).is_raw());
    assert!(InputFilter::deprecated(b"special_chars"));
    assert!(!InputFilter::deprecated(b"unsafe_raw"));
}

#[test]
fn test_033_all_filters() {
    // test 033.phpt: every filter of filter_list() on the same data
    let data = [
        "PHP",
        "1",
        "foo@bar.com",
        "http://a.b.c",
        "1.2.3.4",
        "123",
        "123abc<>()",
        "O'Henry",
        "하퍼",
        "aa:bb:cc:dd:ee:ff",
    ];
    let expected = [
        ("int", ["", "1", "", "", "", "123", "", "", "", ""]),
        (
            "validate_mac",
            ["", "", "", "", "", "", "", "", "", "aa:bb:cc:dd:ee:ff"],
        ),
        (
            "encoded",
            [
                "PHP",
                "1",
                "foo%40bar.com",
                "http%3A%2F%2Fa.b.c",
                "1.2.3.4",
                "123",
                "123abc%3C%3E%28%29",
                "O%27Henry",
                "%ED%95%98%ED%8D%BC",
                "aa%3Abb%3Acc%3Add%3Aee%3Aff",
            ],
        ),
        (
            "full_special_chars",
            [
                "PHP",
                "1",
                "foo@bar.com",
                "http://a.b.c",
                "1.2.3.4",
                "123",
                "123abc&lt;&gt;()",
                "O&#039;Henry",
                "하퍼",
                "aa:bb:cc:dd:ee:ff",
            ],
        ),
        (
            "email",
            [
                "PHP",
                "1",
                "foo@bar.com",
                "httpa.b.c",
                "1.2.3.4",
                "123",
                "123abc",
                "O'Henry",
                "",
                "aabbccddeeff",
            ],
        ),
        (
            "callback",
            [
                "PHP",
                "1",
                "FOO@BAR.COM",
                "HTTP://A.B.C",
                "1.2.3.4",
                "123",
                "123ABC<>()",
                "O'HENRY",
                "하퍼",
                "AA:BB:CC:DD:EE:FF",
            ],
        ),
    ];
    let mut h = Host::new();
    let o = assoc(&[("options", s("test"))]);
    for (name, want) in expected {
        let id = filter_id(name.as_bytes()).unwrap();
        for (d, w) in data.iter().zip(want) {
            let r = filter_var(&mut h, &s(d), id, Options::Array(&o)).unwrap();
            let shown = match r {
                Value::Str(x) => String::from_utf8(x.to_vec()).unwrap(),
                Value::Int(i) => i.to_string(),
                Value::Bool(false) => String::new(),
                v => panic!("{name}: {}", ser(&v)),
            };
            assert_eq!(shown, w, "{name} on {d}");
        }
    }
}
