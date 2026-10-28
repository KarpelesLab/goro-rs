//! Test host for the filter functions: stdClass and string-convertible
//! classes, a few named callables, input arrays, and PHP's serialize()
//! format for building and dumping values.

#![allow(dead_code)]

use std::collections::HashMap;
use std::rc::Rc;

use goro_ext_filter::{FilterHost, InputType};
use goro_runtime::{
    ClassInfo, Ctx, Key, KeyRef, Level, ObjRef, PhpArray, PhpError, PhpStr, Value, float_fmt,
};

/// A closure's body.
pub type ClosureFn = Rc<dyn Fn(&mut Host, Value) -> Result<Value, Exc>>;

/// What a test class does.
#[derive(Clone)]
pub enum Kind {
    /// No `__toString()`.
    Plain,
    /// `__toString()` returns this.
    Str(PhpStr),
    /// `__toString()` throws an Exception with this message.
    Throws(String),
    /// A closure: called with the argument.
    Closure(ClosureFn),
}

pub struct Class {
    pub name: PhpStr,
    pub kind: Kind,
}

impl ClassInfo for Class {
    fn name(&self) -> &PhpStr {
        &self.name
    }
}

/// What the host throws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exc {
    /// TypeError, ValueError, ...
    Error(PhpError),
    /// Filter\FilterFailedException.
    Failed(Vec<u8>),
    /// A user exception (class, message).
    User(String, String),
}

impl Exc {
    /// "Class: message" as the fixture records it.
    pub fn describe(&self) -> (String, Vec<u8>) {
        match self {
            Exc::Error(e) => (
                e.kind.class_name().to_string(),
                e.message.as_bytes().to_vec(),
            ),
            Exc::Failed(m) => ("Filter\\FilterFailedException".to_string(), m.clone()),
            Exc::User(c, m) => (c.clone(), m.as_bytes().to_vec()),
        }
    }
}

pub struct Host {
    pub precision: i32,
    pub warnings: Vec<String>,
    pub std: Rc<Class>,
    pub inputs: HashMap<&'static str, PhpArray>,
    pub charset: Vec<u8>,
    pub echo: Vec<u8>,
}

impl Default for Host {
    fn default() -> Self {
        Host::new()
    }
}

fn input_name(t: InputType) -> &'static str {
    match t {
        InputType::Post => "post",
        InputType::Get => "get",
        InputType::Cookie => "cookie",
        InputType::Env => "env",
        InputType::Server => "server",
    }
}

impl Host {
    pub fn new() -> Host {
        Host {
            precision: 14,
            warnings: Vec::new(),
            std: Rc::new(Class {
                name: PhpStr::from("stdClass"),
                kind: Kind::Plain,
            }),
            inputs: HashMap::new(),
            charset: b"UTF-8".to_vec(),
            echo: Vec::new(),
        }
    }

    pub fn object(&self, name: &str, kind: Kind) -> Value {
        Value::Object(ObjRef::new(
            Rc::new(Class {
                name: PhpStr::from(name),
                kind,
            }) as Rc<dyn ClassInfo>,
            Vec::new(),
        ))
    }

    pub fn std_object(&self) -> Value {
        Value::Object(ObjRef::new(
            self.std.clone() as Rc<dyn ClassInfo>,
            Vec::new(),
        ))
    }

    pub fn closure(&self, f: impl Fn(&mut Host, Value) -> Result<Value, Exc> + 'static) -> Value {
        self.object("Closure", Kind::Closure(Rc::new(f)))
    }

    pub fn set_input(&mut self, t: InputType, a: PhpArray) {
        self.inputs.insert(input_name(t), a);
    }

    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}

fn class_kind(o: &ObjRef) -> Kind {
    (&**o.class() as &dyn std::any::Any)
        .downcast_ref::<Class>()
        .map(|c| c.kind.clone())
        .unwrap_or(Kind::Plain)
}

impl Ctx for Host {
    fn notice(&mut self, _level: Level, message: String) {
        self.warnings.push(message);
    }
    fn precision(&self) -> i32 {
        self.precision
    }
}

fn s(v: &Value) -> Vec<u8> {
    match v {
        Value::Str(s) => s.to_vec(),
        _ => Vec::new(),
    }
}

impl FilterHost for Host {
    type Exception = Exc;

    fn object_string(&mut self, obj: &ObjRef) -> Result<Option<PhpStr>, Exc> {
        match class_kind(obj) {
            Kind::Str(s) => Ok(Some(s)),
            Kind::Throws(m) => Err(Exc::User("Exception".into(), m)),
            _ => Ok(None),
        }
    }

    fn is_callable(&mut self, cb: &Value) -> bool {
        match cb {
            Value::Str(n) => matches!(
                &n[..],
                b"strtoupper"
                    | b"strtolower"
                    | b"test"
                    | b"myfunc"
                    | b"filter_cb"
                    | b"test1"
                    | b"callback"
            ),
            Value::Object(o) => matches!(class_kind(o), Kind::Closure(_)),
            Value::Array(a) => a.len() == 2,
            _ => false,
        }
    }

    fn call(&mut self, cb: &Value, arg: Value) -> Result<Value, Exc> {
        match cb {
            Value::Str(n) => Ok(match &n[..] {
                b"strtoupper" | b"test" => Value::Str(PhpStr::new(&s(&arg).to_ascii_uppercase())),
                b"strtolower" => Value::Str(PhpStr::new(&s(&arg).to_ascii_lowercase())),
                b"myfunc" => {
                    let mut v = s(&arg);
                    v.extend_from_slice(b"_callback");
                    Value::Str(PhpStr::new(&v))
                }
                b"filter_cb" => Value::Int(1),
                b"callback" => arg,
                _ => Value::Null,
            }),
            Value::Object(o) => match class_kind(o) {
                Kind::Closure(f) => f(self, arg),
                _ => unreachable!(),
            },
            // [class, method]: strtolower (test 029.phpt's test_class::test)
            Value::Array(_) => Ok(Value::Str(PhpStr::new(&s(&arg).to_ascii_lowercase()))),
            _ => unreachable!(),
        }
    }

    fn throw_error(&mut self, err: PhpError) -> Exc {
        Exc::Error(err)
    }

    fn filter_failed(&mut self, message: PhpStr) -> Exc {
        Exc::Failed(message.to_vec())
    }

    fn input(&mut self, ty: InputType) -> Option<PhpArray> {
        self.inputs.get(input_name(ty)).cloned()
    }

    fn default_charset(&self) -> Vec<u8> {
        self.charset.clone()
    }
}

// ---------------------------------------------------------------- escaping

/// The fixture's string escape: `'` then bytes, with `%XX` for bytes
/// <= 0x20, >= 0x7f and `%`.
pub fn esc(b: &[u8]) -> String {
    let mut o = String::from("'");
    for &c in b {
        if c <= 0x20 || c >= 0x7f || c == b'%' {
            o.push_str(&format!("%{c:02X}"));
        } else {
            o.push(c as char);
        }
    }
    o
}

pub fn unesc(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    assert_eq!(b.first(), Some(&b'\''), "escaped field: {s}");
    let mut o = Vec::new();
    let mut i = 1;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(&b[i + 1..i + 3]).unwrap();
            o.push(u8::from_str_radix(h, 16).unwrap());
            i += 3;
        } else {
            o.push(b[i]);
            i += 1;
        }
    }
    o
}

// ---------------------------------------------------------------- serialize

/// PHP's serialize() for the value kinds the fixture uses
/// (serialize_precision -1).
pub fn serialize(v: &Value) -> Vec<u8> {
    let mut o = Vec::new();
    ser_into(v, &mut o);
    o
}

fn ser_str(s: &[u8], o: &mut Vec<u8>) {
    o.extend_from_slice(format!("s:{}:\"", s.len()).as_bytes());
    o.extend_from_slice(s);
    o.extend_from_slice(b"\";");
}

fn ser_into(v: &Value, o: &mut Vec<u8>) {
    match v {
        Value::Null => o.extend_from_slice(b"N;"),
        Value::Bool(b) => o.extend_from_slice(if *b { b"b:1;" } else { b"b:0;" }),
        Value::Int(i) => o.extend_from_slice(format!("i:{i};").as_bytes()),
        Value::Float(f) => {
            o.extend_from_slice(b"d:");
            float_fmt::format_php_repr(*f, -1, b'E', o);
            o.push(b';');
        }
        Value::Str(s) => ser_str(s, o),
        Value::Array(a) => {
            o.extend_from_slice(format!("a:{}:{{", a.len()).as_bytes());
            for (k, v) in a.iter() {
                match k {
                    KeyRef::Int(i) => o.extend_from_slice(format!("i:{i};").as_bytes()),
                    KeyRef::Str(s) => ser_str(s, o),
                }
                ser_into(v, o);
            }
            o.push(b'}');
        }
        Value::Object(obj) => {
            let name = obj.class_name().as_bytes().to_vec();
            o.extend_from_slice(format!("O:{}:\"", name.len()).as_bytes());
            o.extend_from_slice(&name);
            o.extend_from_slice(b"\":0:{}");
        }
        Value::Ref(r) => ser_into(&r.get(), o),
        Value::Resource(_) => o.extend_from_slice(b"i:0;"),
    }
}

/// Parses PHP's serialize() output (null, bool, int, float, string, array,
/// stdClass without properties).
pub fn unserialize(host: &Host, s: &[u8]) -> Value {
    let mut p = 0;
    let v = unser(host, s, &mut p);
    assert_eq!(
        p,
        s.len(),
        "trailing data in {:?}",
        String::from_utf8_lossy(s)
    );
    v
}

fn read_until(s: &[u8], p: &mut usize, end: u8) -> String {
    let start = *p;
    while s[*p] != end {
        *p += 1;
    }
    let r = String::from_utf8(s[start..*p].to_vec()).unwrap();
    *p += 1;
    r
}

fn unser(host: &Host, s: &[u8], p: &mut usize) -> Value {
    let t = s[*p];
    *p += 2;
    match t {
        b'N' => Value::Null,
        b'b' => Value::Bool(read_until(s, p, b';') == "1"),
        b'i' => Value::Int(read_until(s, p, b';').parse().unwrap()),
        b'd' => {
            let t = read_until(s, p, b';');
            Value::Float(match t.as_str() {
                "INF" => f64::INFINITY,
                "-INF" => f64::NEG_INFINITY,
                "NAN" => f64::NAN,
                t => t.parse().unwrap(),
            })
        }
        b's' => {
            let n: usize = read_until(s, p, b':').parse().unwrap();
            *p += 1;
            let v = Value::Str(PhpStr::new(&s[*p..*p + n]));
            *p += n + 2;
            v
        }
        b'a' => {
            let n: usize = read_until(s, p, b':').parse().unwrap();
            *p += 1;
            let mut a = PhpArray::new();
            for _ in 0..n {
                let k = unser(host, s, p);
                let v = unser(host, s, p);
                let k = match k {
                    Value::Int(i) => Key::Int(i),
                    Value::Str(s) => Key::Str(s),
                    _ => panic!("bad key"),
                };
                a.insert(k, v);
            }
            *p += 1;
            Value::Array(a)
        }
        b'O' => {
            let n: usize = read_until(s, p, b':').parse().unwrap();
            *p += 1;
            assert_eq!(&s[*p..*p + n], b"stdClass");
            *p += n + 2;
            let cnt: usize = read_until(s, p, b':').parse().unwrap();
            assert_eq!(cnt, 0);
            *p += 2;
            host.std_object()
        }
        _ => panic!("unsupported serialize type {}", t as char),
    }
}
