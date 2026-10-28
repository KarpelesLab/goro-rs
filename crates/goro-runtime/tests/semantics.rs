//! Conversion, comparison and operator cases taken from php-src's .phpt
//! tests (expected output only) and from PHP 8.5 spot checks.

use std::rc::Rc;

use goro_runtime::compare::{compare, loose_eq, strict_eq};
use goro_runtime::convert::{to_array, to_int, to_str};
use goro_runtime::ops::{self, BinOp};
use goro_runtime::{
    ClassInfo, Diagnostics, ErrorKind, Key, Level, ObjRef, PhpArray, PhpStr, ResRef, Value,
};

fn v(s: &str) -> Value {
    Value::from(s)
}

fn eq(a: Value, b: Value) -> bool {
    loose_eq(&a, &b, &mut Diagnostics::new()).unwrap()
}

fn cmp(a: Value, b: Value) -> i32 {
    compare(&a, &b, &mut Diagnostics::new()).unwrap()
}

/// Zend/tests/string_to_number_comparison.phpt (the RFC's examples).
#[test]
fn rfc_string_to_number_comparison() {
    assert!(eq(Value::Int(42), v("000042")));
    assert!(eq(Value::Int(42), v("42.0")));
    assert!(eq(Value::Float(42.0), v("+42.0E0")));
    assert!(eq(Value::Int(0), v("0e214987142012")));
    assert!(eq(v("42"), v("000042")));
    assert!(eq(v("42"), v("42.0")));
    assert!(eq(v("42.0"), v("+42.0E0")));
    assert!(eq(v("0"), v("0e214987142012")));
    assert!(eq(Value::Int(42), v("   42")));
    assert!(eq(Value::Int(42), v("42   ")));
    assert!(!eq(Value::Int(42), v("42abc")));
    assert!(!eq(Value::Int(42), v("abc42")));
    assert!(!eq(Value::Int(0), v("abc42")));
    assert!(eq(Value::Float(f64::INFINITY), v("INF")));
    assert!(eq(Value::Float(f64::NEG_INFINITY), v("-INF")));
    assert!(!eq(Value::Float(f64::NAN), v("NAN")));
    assert!(eq(Value::Float(f64::INFINITY), v("1e1000")));
    assert!(eq(Value::Float(f64::NEG_INFINITY), v("-1e1000")));

    let f = Value::Float(1.75);
    let mut ctx = Diagnostics::new();
    assert_eq!(compare(&f, &v("1.75abc"), &mut ctx).unwrap(), -1);
    assert_eq!(compare(&v("1.75"), &v("1.75abc"), &mut ctx).unwrap(), -1);
    // With precision=0 the float is converted to "2".
    ctx.precision = Some(0);
    assert_eq!(compare(&f, &v("1.75abc"), &mut ctx).unwrap(), 1);
    let s = to_str(&f, &mut ctx).unwrap();
    assert_eq!(&*s, b"2");
    assert_eq!(compare(&Value::Str(s), &v("1.75abc"), &mut ctx).unwrap(), 1);
}

/// Zend/tests/int_conversion_exponents.phpt
#[test]
fn int_conversion_from_exponents() {
    let mut ctx = Diagnostics::new();
    assert_eq!(to_int(&v("1.2345e9"), &mut ctx), 1_234_500_000);
    assert_eq!(to_int(&v("-1.2345e9"), &mut ctx), -1_234_500_000);
    assert_eq!(to_int(&v(" 1.2345e9  abc"), &mut ctx), 1_234_500_000);
    let r = ops::modulo(&v("1.2345e9"), &Value::Int(i64::MAX), &mut ctx).unwrap();
    assert_eq!(r.as_int(), Some(1_234_500_000));
    let r = ops::bit_or(&v("-1.2345e9"), &Value::Int(0), &mut ctx).unwrap();
    assert_eq!(r.as_int(), Some(-1_234_500_000));
    assert!(ctx.take().is_empty());
    let r = ops::bit_or(&v(" 1.2345e9  abc"), &Value::Int(0), &mut ctx).unwrap();
    assert_eq!(r.as_int(), Some(1_234_500_000));
    let n = ctx.take();
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].level, Level::Warning);
    assert_eq!(n[0].message, "A non-numeric value encountered");
}

/// Zend/tests/numeric_strings/trailling_whitespaces.phpt
#[test]
fn whitespace_in_numeric_strings() {
    let strings = [
        "123",
        "123   ",
        "123 \t\n\r\x0b\x0c",
        "   123",
        " \t\n\r\x0b\x0c123",
        "   123   ",
        "123.0",
        "123.0 \t\n\r\x0b\x0c",
        " \t\n\r\x0b\x0c123.0",
        "123e0",
        " \t\n\r\x0b\x0c123e0 \t\n\r\x0b\x0c",
    ];
    let mut ctx = Diagnostics::new();
    for s in strings {
        let sv = v(s);
        assert!(eq(sv.clone(), Value::Int(123)), "{s:?}");
        let p = ops::plus(&sv, &mut ctx).unwrap();
        assert!(eq(p, Value::Int(123)));
        assert!(goro_runtime::numeric::is_numeric_str(s.as_bytes()));
        assert!(eq(ops::increment(&sv, &mut ctx).unwrap(), Value::Int(124)));
        assert!(eq(ops::decrement(&sv, &mut ctx).unwrap(), Value::Int(122)));
    }
    assert!(ctx.take().is_empty());
}

/// Zend/tests/numeric_strings/invalid_numeric_strings_must_generate_warning.phpt
#[test]
fn leading_numeric_strings_warn_non_numeric_throw() {
    let mut ctx = Diagnostics::new();
    let r = ops::add(&v("2 Lorem"), &v("3 ipsum"), &mut ctx).unwrap();
    assert_eq!(r.as_int(), Some(5));
    assert_eq!(ctx.take().len(), 2);
    let e = ops::add(&v("dolor"), &v("sit"), &mut ctx).unwrap_err();
    assert_eq!(e.kind, ErrorKind::TypeError);
    assert_eq!(e.message, "Unsupported operand types: string + string");
    let e = ops::modulo(&v("dolor"), &v("sit"), &mut ctx).unwrap_err();
    assert_eq!(e.message, "Unsupported operand types: string % string");
    // Observed on PHP 8.5: the left operand's warning comes first.
    let e = ops::add(&v("5 apples"), &v("abc"), &mut ctx).unwrap_err();
    assert_eq!(e.message, "Unsupported operand types: string + string");
    assert_eq!(ctx.take().len(), 1);
    let e = ops::add(&Value::Null, &v("abc"), &mut ctx).unwrap_err();
    assert_eq!(e.message, "Unsupported operand types: null + string");
}

/// Observed on PHP 8.5: assorted comparisons.
#[test]
fn loose_comparisons() {
    assert!(eq(v(" 1"), Value::Int(1)));
    assert!(eq(v("1 "), Value::Int(1)));
    assert!(eq(v("1\n"), Value::Int(1)));
    assert!(!eq(v("abc"), v("ABC")));
    assert!(eq(Value::Null, Value::Bool(false)));
    assert!(eq(Value::Array(PhpArray::new()), Value::Bool(false)));
    assert!(eq(v("0"), Value::Bool(false)));
    assert!(eq(v(""), Value::Null));
    assert!(!eq(v("a"), Value::Int(0)));
    assert!(!eq(v("1e1000"), v("1e1001")));
    assert_eq!(cmp(v("1e1000"), v("-1e1000")), 1);
    assert!(!eq(v("9223372036854775807"), v("9223372036854775808")));

    assert_eq!(cmp(Value::Int(1), v("1")), 0);
    assert_eq!(cmp(v("abc"), Value::Int(1)), 1);
    assert_eq!(cmp(Value::Int(1), v("abc")), -1);
    assert_eq!(cmp(Value::Null, v("a")), -1);
    assert_eq!(cmp(Value::Bool(true), v("a")), 0);
    assert_eq!(cmp(Value::Array(PhpArray::new()), Value::Null), 0);
    assert_eq!(cmp(Value::Float(1.5), v("1.5")), 0);
    assert_eq!(cmp(v("10"), v("9")), 1);
    assert_eq!(cmp(v("10"), v("9a")), -1);
}

fn arr(pairs: &[(i64, Value)]) -> Value {
    Value::Array(PhpArray::from_pairs(
        pairs.iter().map(|(k, v)| (Key::Int(*k), v.clone())),
    ))
}

/// PHP manual (Array Operators) and PHP 8.5: [1,2] == [1=>2, 0=>1] is
/// true, === is false; [1,2] <=> [1,2,3] is -1; ["a"=>1] == ["a"=>"1"];
/// [0] == [false].
#[test]
fn array_comparisons() {
    let a = arr(&[(0, Value::Int(1)), (1, Value::Int(2))]);
    let b = arr(&[(1, Value::Int(2)), (0, Value::Int(1))]);
    assert!(eq(a.clone(), b.clone()));
    assert!(!strict_eq(&a, &b));
    assert!(strict_eq(&a, &a.clone()));
    let c = arr(&[(0, Value::Int(1)), (1, Value::Int(2)), (2, Value::Int(3))]);
    assert_eq!(cmp(a.clone(), c), -1);
    let x = Value::Array(PhpArray::from_pairs([(Key::from("a"), Value::Int(1))]));
    let y = Value::Array(PhpArray::from_pairs([(Key::from("a"), v("1"))]));
    assert!(eq(x.clone(), y.clone()));
    assert!(!strict_eq(&x, &y));
    assert!(eq(
        arr(&[(0, Value::Int(0))]),
        arr(&[(0, Value::Bool(false))])
    ));
    // Uncomparable: neither < nor >.
    let p = Value::Array(PhpArray::from_pairs([(Key::from("a"), Value::Int(1))]));
    let q = Value::Array(PhpArray::from_pairs([(Key::from("b"), Value::Int(1))]));
    let mut ctx = Diagnostics::new();
    assert!(!goro_runtime::compare::less(&p, &q, &mut ctx).unwrap());
    assert!(!goro_runtime::compare::greater(&p, &q, &mut ctx).unwrap());
    // Arrays are greater than scalars.
    assert_eq!(cmp(a, Value::Int(100)), 1);
}

#[test]
fn strict_equality() {
    assert!(strict_eq(&Value::Float(0.0), &Value::Float(-0.0)));
    assert!(!strict_eq(&Value::Float(f64::NAN), &Value::Float(f64::NAN)));
    assert!(!strict_eq(&Value::Int(1), &Value::Float(1.0)));
    assert!(!strict_eq(&v("1e3"), &v("1000")));
    assert!(strict_eq(&v("abc"), &Value::Str(PhpStr::intern(b"abc"))));
}

struct Plain(PhpStr);
impl ClassInfo for Plain {
    fn name(&self) -> &PhpStr {
        &self.0
    }
}

/// Observed on PHP 8.5 with stdClass.
#[test]
fn object_conversions_default() {
    let class: Rc<dyn ClassInfo> = Rc::new(Plain(PhpStr::from("stdClass")));
    let o = Value::Object(ObjRef::new(class.clone(), vec![]));
    let mut ctx = Diagnostics::new();
    assert_eq!(to_int(&o, &mut ctx), 1);
    assert_eq!(
        ctx.take()[0].message,
        "Object of class stdClass could not be converted to int"
    );
    let e = to_str(&o, &mut ctx).unwrap_err();
    assert_eq!(
        e.message,
        "Object of class stdClass could not be converted to string"
    );
    let e = ops::add(&o, &Value::Int(1), &mut ctx).unwrap_err();
    assert_eq!(e.message, "Unsupported operand types: stdClass + int");
    // == 1: notice, then true
    assert!(loose_eq(&o, &Value::Int(1), &mut ctx).unwrap());
    let n = ctx.take();
    assert_eq!(n[0].level, Level::Notice);
    // == null is false, null < object
    assert!(!loose_eq(&o, &Value::Null, &mut ctx).unwrap());
    assert!(goro_runtime::compare::less(&Value::Null, &o, &mut ctx).unwrap());
    let e = ops::increment(&o, &mut ctx).unwrap_err();
    assert_eq!(e.message, "Cannot increment stdClass");
    // (array) of an object: its property table.
    if let Value::Object(obj) = &o {
        obj.obj()
            .props
            .borrow_mut()
            .insert(Key::from("a"), Value::Int(1));
        obj.obj()
            .props
            .borrow_mut()
            .insert(Key::from("5"), Value::Int(2));
    }
    let a = to_array(&o, &mut ctx);
    assert_eq!(a.len(), 2);
    assert_eq!(a.get_int(5).and_then(|v| v.as_int()), Some(2));
    // Two objects of one class with equal properties are ==.
    let p = ObjRef::new(class.clone(), vec![]);
    let q = ObjRef::new(class, vec![]);
    assert!(
        loose_eq(
            &Value::Object(p.clone()),
            &Value::Object(q.clone()),
            &mut ctx
        )
        .unwrap()
    );
    q.obj()
        .props
        .borrow_mut()
        .insert(Key::from("x"), Value::Int(1));
    assert!(!loose_eq(&Value::Object(p), &Value::Object(q), &mut ctx).unwrap());
}

/// Observed on PHP 8.5 with STDERR (resource id 3).
#[test]
fn resources() {
    let r = ResRef::new(PhpStr::from("stream"), Box::new(()));
    let id = r.id();
    let rv = Value::Resource(r.clone());
    let mut ctx = Diagnostics::new();
    assert_eq!(
        &*to_str(&rv, &mut ctx).unwrap(),
        format!("Resource id #{id}").as_bytes()
    );
    assert_eq!(to_int(&rv, &mut ctx), id);
    let e = ops::add(&rv, &Value::Int(1), &mut ctx).unwrap_err();
    assert_eq!(e.message, "Unsupported operand types: resource + int");
    assert!(loose_eq(&rv, &Value::Int(id), &mut ctx).unwrap());
    let k = goro_runtime::convert::to_key(&rv, "array", &mut ctx).unwrap();
    assert_eq!(k, Key::Int(id));
    assert_eq!(
        ctx.take()[0].message,
        format!("Resource ID#{id} used as offset, casting to integer ({id})")
    );
    let e = ops::increment(&rv, &mut ctx).unwrap_err();
    assert_eq!(e.message, "Cannot increment resource");
    assert_eq!(rv.debug_type(), "resource (stream)");
    r.close();
    assert_eq!(rv.debug_type(), "resource (closed)");
}

#[test]
fn nan_coercions() {
    let mut ctx = Diagnostics::new();
    assert!(!ops::not(&Value::Float(f64::NAN), &mut ctx));
    assert_eq!(
        ctx.take()[0].message,
        "unexpected NAN value was coerced to bool"
    );
}

/// Observed on PHP 8.5: 1.0 . "" is "1", 0.1 + 0.2 . "" is "0.3",
/// 1e100 . "" is "1.0E+100", -0.0 . "" is "-0", 7.0E-10 . "" is "7.0E-10".
#[test]
fn concatenation_of_floats() {
    let mut ctx = Diagnostics::new();
    for (f, s) in [
        (1.0, "1"),
        (0.1 + 0.2, "0.3"),
        (1e100, "1.0E+100"),
        (-0.0, "-0"),
        (7.0e-10, "7.0E-10"),
        (f64::INFINITY, "INF"),
        (f64::NEG_INFINITY, "-INF"),
    ] {
        let r = ops::concat(&Value::Float(f), &v(""), &mut ctx).unwrap();
        assert_eq!(&*r, s.as_bytes());
    }
}

#[test]
fn concat_assign_in_place() {
    // $s = "x"; $t = $s; $t .= "y";  ->  "x", "xy"
    let s = PhpStr::from("xx");
    let mut t = s.clone();
    let mut ctx = Diagnostics::new();
    ops::concat_assign(&mut t, &v("y"), &mut ctx).unwrap();
    assert_eq!(&*s, b"xx");
    assert_eq!(&*t, b"xxy");
    ops::concat_assign(&mut t, &Value::Int(5), &mut ctx).unwrap();
    assert_eq!(&*t, b"xxy5");
}

#[test]
fn array_union() {
    // [1, 2] + [5, 6, 7] is [1, 2, 7]; keys of the left win.
    let mut ctx = Diagnostics::new();
    let a = Value::Array(PhpArray::from_values([Value::Int(1), Value::Int(2)]));
    let b = Value::Array(PhpArray::from_values([
        Value::Int(5),
        Value::Int(6),
        Value::Int(7),
    ]));
    let r = ops::add(&a, &b, &mut ctx).unwrap();
    let vals: Vec<i64> = r
        .as_array()
        .unwrap()
        .iter()
        .map(|(_, v)| v.as_int().unwrap())
        .collect();
    assert_eq!(vals, [1, 2, 7]);
}

#[test]
fn integer_overflow_and_division() {
    let mut ctx = Diagnostics::new();
    let r = ops::add(&Value::Int(i64::MAX), &Value::Int(1), &mut ctx).unwrap();
    assert_eq!(r.as_float(), Some(9.223_372_036_854_776e18));
    let r = ops::mul(&Value::Int(i64::MIN), &Value::Int(-1), &mut ctx).unwrap();
    assert!(r.as_float().is_some());
    let r = ops::div(&Value::Int(i64::MIN), &Value::Int(-1), &mut ctx).unwrap();
    assert!(r.as_float().is_some());
    assert_eq!(
        ops::div(&Value::Int(6), &Value::Int(3), &mut ctx)
            .unwrap()
            .as_int(),
        Some(2)
    );
    assert_eq!(
        ops::div(&Value::Int(7), &Value::Int(2), &mut ctx)
            .unwrap()
            .as_float(),
        Some(3.5)
    );
    let e = ops::div(&Value::Int(1), &Value::Float(0.0), &mut ctx).unwrap_err();
    assert_eq!(
        (e.kind, e.message.as_str()),
        (ErrorKind::DivisionByZero, "Division by zero")
    );
    let e = ops::modulo(&Value::Int(1), &Value::Int(0), &mut ctx).unwrap_err();
    assert_eq!(e.message, "Modulo by zero");
    assert_eq!(
        ops::modulo(&Value::Int(i64::MIN), &Value::Int(-1), &mut ctx)
            .unwrap()
            .as_int(),
        Some(0)
    );
    assert_eq!(
        ops::modulo(&Value::Int(-7), &Value::Int(3), &mut ctx)
            .unwrap()
            .as_int(),
        Some(-1)
    );
    let e = ops::intdiv(i64::MIN, -1).unwrap_err();
    assert_eq!(e.kind, ErrorKind::ArithmeticError);
    assert_eq!(ops::intdiv(-7, 2).unwrap(), -3);
    assert_eq!(
        ops::pow(&Value::Int(2), &Value::Int(62), &mut ctx)
            .unwrap()
            .as_int(),
        Some(1 << 62)
    );
    assert!(
        ops::pow(&Value::Int(2), &Value::Int(63), &mut ctx)
            .unwrap()
            .as_float()
            .is_some()
    );
    assert_eq!(
        ops::pow(&Value::Int(2), &Value::Int(-1), &mut ctx)
            .unwrap()
            .as_float(),
        Some(0.5)
    );
    assert_eq!(
        ops::pow(&Value::Int(-3), &Value::Int(3), &mut ctx)
            .unwrap()
            .as_int(),
        Some(-27)
    );
    // Shifts
    assert_eq!(
        ops::shl(&Value::Int(1), &Value::Int(64), &mut ctx)
            .unwrap()
            .as_int(),
        Some(0)
    );
    assert_eq!(
        ops::shr(&Value::Int(-8), &Value::Int(100), &mut ctx)
            .unwrap()
            .as_int(),
        Some(-1)
    );
    let e = ops::shl(&Value::Int(1), &Value::Int(-1), &mut ctx).unwrap_err();
    assert_eq!(e.message, "Bit shift by negative number");
    assert!(ctx.take().is_empty());
}

#[test]
fn string_bitwise() {
    let mut ctx = Diagnostics::new();
    let r = ops::bit_and(&v("abc"), &v("  "), &mut ctx).unwrap();
    assert_eq!(&*r.as_str().unwrap().clone(), b"  ");
    let r = ops::bit_or(&v("a"), &v("  b"), &mut ctx).unwrap();
    assert_eq!(&*r.as_str().unwrap().clone(), b"a b");
    let r = ops::bit_xor(&v("abc"), &v("AB"), &mut ctx).unwrap();
    assert_eq!(&*r.as_str().unwrap().clone(), b"  ");
    let r = ops::bit_not(&Value::str(b"\x00\xff"), &mut ctx).unwrap();
    assert_eq!(&*r.as_str().unwrap().clone(), b"\xff\x00");
    let e = ops::bit_not(&Value::Bool(true), &mut ctx).unwrap_err();
    assert_eq!(e.message, "Cannot perform bitwise not on true");
}

#[test]
fn binary_op_dispatch_and_references() {
    let mut ctx = Diagnostics::new();
    let mut x = Value::Int(5);
    let r = Value::Ref(x.make_ref());
    for (op, expect) in [
        (BinOp::Add, 7),
        (BinOp::Sub, 3),
        (BinOp::Mul, 10),
        (BinOp::Mod, 1),
        (BinOp::Pow, 25),
        (BinOp::BitAnd, 0),
        (BinOp::BitOr, 7),
        (BinOp::BitXor, 7),
        (BinOp::Shl, 20),
        (BinOp::Shr, 1),
    ] {
        let got = ops::binary_op(op, &r, &Value::Int(2), &mut ctx).unwrap();
        assert_eq!(got.as_int(), Some(expect), "{op:?}");
    }
    let got = ops::binary_op(BinOp::Concat, &r, &Value::Int(2), &mut ctx).unwrap();
    assert_eq!(got.as_str().map(|s| s.to_vec()), Some(b"52".to_vec()));
    assert_eq!(BinOp::Pow.symbol(), "**");
}

#[test]
fn string_increment_table() {
    // Zend/tests/in-de-crement/string_increment_various.phpt
    for (from, to) in [
        ("Az", "Ba"),
        ("aZ", "bA"),
        ("A9", "B0"),
        ("a9", "b0"),
        ("Zz", "AAa"),
        ("zZ", "aaA"),
        ("9z", "10a"),
        ("9Z", "10A"),
        ("", "1"),
        ("-cc", "-cd"),
        ("Z ", "Z "),
        (" Z", " A"),
        ("foo1.txt", "foo1.txu"),
        ("1f.5", "1f.6"),
        ("5d9", "5e0"),
    ] {
        assert_eq!(
            ops::str_increment_bytes(from.as_bytes()),
            to.as_bytes(),
            "{from:?}"
        );
    }
    // "5e0" is numeric: the next increment gives float(6).
    let mut ctx = Diagnostics::new();
    assert_eq!(
        ops::increment(&v("5e0"), &mut ctx).unwrap().as_float(),
        Some(6.0)
    );
}
