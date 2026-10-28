//! Test host: stdClass and the classes of the fixture's named scenarios,
//! PHP's serialize() format for building and dumping values.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use goro_ext_json::{JsonError, JsonHost, ObjectKind};
use goro_runtime::{ClassInfo, Key, KeyRef, ObjRef, PhpArray, PhpError, PhpStr, Value, float_fmt};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Std,
    /// `jsonSerialize()` echoes "[js <n>]" and returns `$v`.
    Js,
    JsThrow,
    /// `jsonSerialize()` echoes serialize(json_encode($this)) and the error, returns `$this`.
    JsSelfEncode,
    /// `jsonSerialize()` echoes serialize(json_encode($this->p)) and the error, returns 1.
    JsInnerEncode,
    Plain,
    UnitEnum,
    BackedEnum,
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

pub struct Host {
    pub precision: i32,
    pub echo: Rc<RefCell<Vec<u8>>>,
    pub std: Rc<Class>,
}

impl Default for Host {
    fn default() -> Self {
        Host::new()
    }
}

impl Host {
    pub fn new() -> Host {
        Host {
            precision: -1,
            echo: Rc::new(RefCell::new(Vec::new())),
            std: Rc::new(Class {
                name: PhpStr::from("stdClass"),
                kind: Kind::Std,
            }),
        }
    }

    pub fn class(&self, name: &str, kind: Kind) -> Rc<Class> {
        Rc::new(Class {
            name: PhpStr::from(name),
            kind,
        })
    }

    pub fn object(&self, class: &Rc<Class>, props: &[(&str, Value)]) -> ObjRef {
        let o = ObjRef::new(class.clone() as Rc<dyn ClassInfo>, Vec::new());
        {
            let mut p = o.obj().props.borrow_mut();
            for (k, v) in props {
                p.insert(Key::from_bytes(k.as_bytes()), v.clone());
            }
        }
        o
    }

    pub fn std_object(&self, props: &[(&str, Value)]) -> ObjRef {
        let c = self.std.clone();
        self.object(&c, props)
    }

    pub fn echo(&self, b: &[u8]) {
        self.echo.borrow_mut().extend_from_slice(b);
    }

    pub fn take_echo(&self) -> Vec<u8> {
        std::mem::take(&mut *self.echo.borrow_mut())
    }
}

fn kind_of(o: &ObjRef) -> Kind {
    (&**o.class() as &dyn std::any::Any)
        .downcast_ref::<Class>()
        .map(|c| c.kind)
        .unwrap_or(Kind::Std)
}

fn prop(o: &ObjRef, name: &str) -> Value {
    o.obj()
        .props
        .borrow()
        .get_bytes(name.as_bytes())
        .map(|v| v.deref())
        .unwrap_or_default()
}

impl JsonHost for Host {
    type Exception = String;

    fn serialize_precision(&self) -> i32 {
        self.precision
    }

    fn object_kind(&mut self, obj: &ObjRef) -> ObjectKind {
        match kind_of(obj) {
            Kind::Js | Kind::JsThrow | Kind::JsSelfEncode | Kind::JsInnerEncode => {
                ObjectKind::Serializable
            }
            Kind::UnitEnum => ObjectKind::UnitEnum,
            Kind::BackedEnum => ObjectKind::BackedEnum(prop(obj, "value")),
            _ => ObjectKind::Plain,
        }
    }

    fn json_serialize(&mut self, obj: &ObjRef) -> Result<Value, String> {
        match kind_of(obj) {
            Kind::Js => {
                let n = prop(obj, "n");
                let mut line = b"[js ".to_vec();
                line.extend_from_slice(n.as_str().map(|s| s.as_bytes()).unwrap_or(b""));
                line.push(b']');
                self.echo(&line);
                Ok(prop(obj, "v"))
            }
            Kind::JsThrow => Err("Exception: Not implemented! (0)".to_string()),
            Kind::JsSelfEncode => {
                let r = goro_ext_json::json_encode(self, &Value::Object(obj.clone()), 0, 512)?;
                let mut s = serialize(&r);
                s.extend_from_slice(goro_ext_json::json_last_error().to_string().as_bytes());
                self.echo(&s);
                Ok(Value::Object(obj.clone()))
            }
            Kind::JsInnerEncode => {
                let p = prop(obj, "p");
                let r = goro_ext_json::json_encode(self, &p, 0, 512)?;
                let mut s = serialize(&r);
                s.extend_from_slice(goro_ext_json::json_last_error().to_string().as_bytes());
                self.echo(&s);
                Ok(Value::Int(1))
            }
            _ => Ok(Value::Null),
        }
    }

    fn object_properties(&mut self, obj: &ObjRef) -> Result<PhpArray, String> {
        Ok(obj.obj().props.borrow().clone())
    }

    fn new_object(&mut self, props: PhpArray) -> Value {
        let o = ObjRef::new(self.std.clone() as Rc<dyn ClassInfo>, Vec::new());
        *o.obj().props.borrow_mut() = props;
        Value::Object(o)
    }

    fn json_exception(&mut self, err: JsonError) -> String {
        format!("JsonException: {} ({})", err.message(), err.code())
    }

    fn throw_error(&mut self, err: PhpError) -> String {
        format!("{}: {} (0)", err.kind.class_name(), err.message)
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
            let props = obj.obj().props.borrow().clone();
            o.extend_from_slice(format!("O:{}:\"", name.len()).as_bytes());
            o.extend_from_slice(&name);
            o.extend_from_slice(format!("\":{}:{{", props.len()).as_bytes());
            for (k, v) in props.iter() {
                match k {
                    KeyRef::Int(i) => ser_str(i.to_string().as_bytes(), o),
                    KeyRef::Str(s) => ser_str(s, o),
                }
                ser_into(v, o);
            }
            o.push(b'}');
        }
        Value::Ref(r) => ser_into(&r.get(), o),
        Value::Resource(_) => o.extend_from_slice(b"i:0;"),
    }
}

/// Parses PHP's serialize() output (null, bool, int, float, string,
/// array, stdClass).
pub fn unserialize(host: &mut Host, s: &[u8]) -> Value {
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

fn unser(host: &mut Host, s: &[u8], p: &mut usize) -> Value {
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
            *p += 1;
            let mut a = PhpArray::new();
            for _ in 0..cnt {
                let k = unser(host, s, p);
                let v = unser(host, s, p);
                let Value::Str(k) = k else { panic!("bad prop") };
                a.insert(Key::from_php_str(&k), v);
            }
            *p += 1;
            host.new_object(a)
        }
        _ => panic!("unsupported serialize type {}", t as char),
    }
}
