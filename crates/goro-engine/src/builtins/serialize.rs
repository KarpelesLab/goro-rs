//! serialize() / unserialize() (PHP manual: serialize, unserialize,
//! Serializable, __serialize/__unserialize, __sleep/__wakeup; format
//! observed on PHP 8.5).

use std::collections::HashMap;
use std::rc::Rc;

use goro_runtime::{Key, KeyRef, ObjRef, PhpArray, PhpRef, PhpStr, Value};

use crate::class::{Class, DisplayName, cflags, mflags};
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Unwind, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        ("function serialize(mixed $value): string", serialize),
        (
            "function unserialize(string $data, array $options = []): mixed",
            unserialize,
        ),
    ],
    classes: &[(
        "__php_incomplete_class",
        "<?php #[AllowDynamicProperties] final class __PHP_Incomplete_Class {}",
    )],
    listed: false,
    ..Module::EMPTY
};

struct Ser {
    out: Vec<u8>,
    /// Value numbers of objects (by handle) and references (by cell).
    objs: HashMap<u32, usize>,
    refs: HashMap<usize, usize>,
    n: usize,
}

impl Vm {
    pub fn serialize_value(&mut self, v: &Value) -> R<Vec<u8>> {
        let mut s = Ser {
            out: Vec::new(),
            objs: HashMap::new(),
            refs: HashMap::new(),
            n: 0,
        };
        self.ser(&mut s, v, false)?;
        Ok(s.out)
    }

    fn ser(&mut self, s: &mut Ser, v: &Value, _in_array: bool) -> R<()> {
        // References: the second time a cell is seen, `R:n;`.
        if let Value::Ref(r) = v {
            if r.refcount() > 1 {
                // A reference to an object already written points at the
                // object (serialization_objects_015: R:1).
                if let Value::Object(o) = &r.get()
                    && let Some(&n) = s.objs.get(&o.handle())
                    && !s.refs.contains_key(&r.addr())
                {
                    s.refs.insert(r.addr(), n);
                    s.out.extend_from_slice(format!("R:{};", n).as_bytes());
                    return Ok(());
                }
                if let Some(&n) = s.refs.get(&r.addr()) {
                    // `R:` takes no value number (counting_of_references).
                    s.out.extend_from_slice(format!("R:{};", n).as_bytes());
                    return Ok(());
                }
                s.refs.insert(r.addr(), s.n + 1);
            }
            let inner = r.get();
            self.ser(s, &inner, _in_array)?;
            if let Value::Object(o) = &inner
                && !s.objs.contains_key(&o.handle())
            {
                // Serializable::serialize() gave null: not registered
                // either (bug80411).
                s.refs.remove(&r.addr());
            }
            return Ok(());
        }
        s.n += 1;
        match v {
            Value::Null => s.out.extend_from_slice(b"N;"),
            Value::Bool(b) => s.out.extend_from_slice(if *b { b"b:1;" } else { b"b:0;" }),
            Value::Int(i) => s.out.extend_from_slice(format!("i:{};", i).as_bytes()),
            Value::Float(f) => {
                s.out.extend_from_slice(b"d:");
                if f.is_nan() {
                    s.out.extend_from_slice(b"NAN");
                } else if f.is_infinite() {
                    s.out
                        .extend_from_slice(if *f > 0.0 { b"INF" } else { b"-INF" });
                } else {
                    self.repr_float(*f, &mut s.out);
                }
                s.out.push(b';');
            }
            Value::Str(st) => {
                s.out
                    .extend_from_slice(format!("s:{}:\"", st.len()).as_bytes());
                s.out.extend_from_slice(st);
                s.out.extend_from_slice(b"\";");
            }
            Value::Array(a) => {
                s.out
                    .extend_from_slice(format!("a:{}:{{", a.len()).as_bytes());
                for (k, x) in a.iter() {
                    match k {
                        KeyRef::Int(i) => s.out.extend_from_slice(format!("i:{};", i).as_bytes()),
                        KeyRef::Str(st) => {
                            s.out
                                .extend_from_slice(format!("s:{}:\"", st.len()).as_bytes());
                            s.out.extend_from_slice(st);
                            s.out.extend_from_slice(b"\";");
                        }
                    }
                    self.ser(s, x, true)?;
                }
                s.out.push(b'}');
            }
            Value::Object(o) => return self.ser_object(s, o),
            Value::Resource(_) => s.out.extend_from_slice(b"i:0;"),
            Value::Ref(_) => unreachable!(),
        }
        Ok(())
    }

    fn ser_object(&mut self, s: &mut Ser, o: &ObjRef) -> R<()> {
        if let Some(&n) = s.objs.get(&o.handle()) {
            s.out.extend_from_slice(format!("r:{};", n).as_bytes());
            return Ok(());
        }
        s.objs.insert(o.handle(), s.n);
        let cls = self.class_of(o);
        let name = cls.name.clone();
        if cls.is_enum() {
            let case = self.get_prop_raw(o, b"name");
            let case = case.as_str().cloned().unwrap_or_default();
            let full = format!("{}:{}", name.to_string_lossy(), case.to_string_lossy());
            s.out
                .extend_from_slice(format!("E:{}:\"{}\";", full.len(), full).as_bytes());
            return Ok(());
        }
        if cls.has_flag(cflags::NOT_SERIALIZABLE)
            || cls.has_flag(cflags::ANON)
            || matches!(
                cls.lname.as_bytes(),
                b"closure" | b"generator" | b"fiber" | b"weakreference" | b"weakmap"
            )
        {
            let n = if cls.has_flag(cflags::ANON) {
                "class@anonymous".to_string()
            } else {
                cls.name.dname()
            };
            return self.throw(
                "Exception",
                &format!("Serialization of '{}' is not allowed", n),
            );
        }
        // __serialize(): an array of data.
        if let Some(f) = cls.find_method(b"__serialize") {
            let data = self.call_func_this(&f, Some(o.clone()), Vec::new())?;
            let Value::Array(a) = data else {
                return self.type_error(&format!(
                    "{}::__serialize() must return an array",
                    name.dname()
                ));
            };
            s.out
                .extend_from_slice(format!("O:{}:\"", name.len()).as_bytes());
            s.out.extend_from_slice(&name);
            s.out
                .extend_from_slice(format!("\":{}:{{", a.len()).as_bytes());
            for (k, x) in a.iter() {
                self.ser_key(s, k);
                self.ser(s, x, false)?;
            }
            s.out.push(b'}');
            return Ok(());
        }
        // Serializable::serialize(): opaque string.
        if self
            .find_class(b"Serializable", false)?
            .is_some_and(|ser| cls.is_subclass_of(&ser))
        {
            let data = self.call_method(o, b"serialize", Vec::new())?;
            match data {
                Value::Str(d) => {
                    s.out
                        .extend_from_slice(format!("C:{}:\"", name.len()).as_bytes());
                    s.out.extend_from_slice(&name);
                    s.out
                        .extend_from_slice(format!("\":{}:{{", d.len()).as_bytes());
                    s.out.extend_from_slice(&d);
                    s.out.push(b'}');
                }
                Value::Null => {
                    // Not registered: a repeat serializes again
                    // (ref_to_failed_serialize).
                    s.objs.remove(&o.handle());
                    s.out.extend_from_slice(b"N;")
                }
                _ => {
                    return self.throw(
                        "Exception",
                        &format!("{}::serialize() must return a string or NULL", name.dname()),
                    );
                }
            }
            return Ok(());
        }
        // Properties (all of them, mangled), or those __sleep() names. A
        // lazy object flagged SKIP_INITIALIZATION_ON_SERIALIZE is
        // serialized uninitialized.
        let skip_init = self.is_lazy_uninit(o)
            && crate::vm::lazy::with_lazy(o, |d| {
                d.options & crate::vm::lazy::SKIP_INITIALIZATION_ON_SERIALIZE != 0
            })
            .unwrap_or(false);
        // __sleep() runs before the properties are read (it may set some:
        // gh12265b).
        let sleep = match cls.find_method(b"__sleep") {
            Some(f) => Some(self.call_func_this(&f, Some(o.clone()), Vec::new())?),
            None => None,
        };
        let props = if skip_init {
            self.plain_props_array(o, true)
        } else {
            let t = self.lazy_realize(o)?;
            self.object_props_array(&t, true)
        };
        // An incomplete object keeps its original class name and data
        // (bug27469).
        let (name, props) = if cls.lname.as_bytes() == b"__php_incomplete_class" {
            let mut rest = PhpArray::new();
            let mut orig = name.clone();
            for (k, x) in props.iter() {
                match k {
                    KeyRef::Str(n) if n.as_bytes() == b"__PHP_Incomplete_Class_Name" => {
                        if let Value::Str(o) = x.deref() {
                            orig = o;
                        }
                    }
                    _ => {
                        rest.insert(k.to_owned(), x.clone());
                    }
                }
            }
            (orig, rest)
        } else {
            (name, props)
        };
        let selected: PhpArray = if let Some(r) = sleep {
            let bad_sleep = format!(
                "serialize(): {}::__sleep() should return an array only containing the names of instance-variables to serialize",
                cls.name.dname()
            );
            let Value::Array(names) = r else {
                self.warn(&bad_sleep)?;
                s.out.extend_from_slice(b"N;");
                return Ok(());
            };
            let mut sel = PhpArray::new();
            for (_, n) in names.iter() {
                let n = match n.deref() {
                    Value::Str(st) => st,
                    other => {
                        self.warn(&bad_sleep)?;
                        self.to_php_str(&other)?
                    }
                };
                // The name, or its mangled forms (observed on PHP 8.5:
                // only the class's own private properties).
                let cands = mangled_forms(&cls, &n);
                let mut found = false;
                for c in cands {
                    if let Some(v) = props.get_str(&c) {
                        if sel.get_str(&c).is_some() {
                            self.warn(&format!(
                                "serialize(): \"{}\" is returned from __sleep() multiple times",
                                n.to_string_lossy()
                            ))?;
                        } else {
                            sel.insert(Key::Str(c), v.clone());
                        }
                        found = true;
                        break;
                    }
                }
                // Declared but uninitialized typed properties are skipped
                // silently (sleep_uninitialized_typed_prop).
                let declared = {
                    let bare = match n.first() {
                        Some(0) => n
                            .iter()
                            .skip(1)
                            .position(|&c| c == 0)
                            .map(|p| PhpStr::new(&n[p + 2..]))
                            .unwrap_or(n.clone()),
                        _ => n.clone(),
                    };
                    let map = cls.prop_map.borrow();
                    let props = cls.props.borrow();
                    map.get(&bare).is_some_and(|idxs| {
                        idxs.iter().any(|&i| {
                            let p = &props[i as usize];
                            p.ty.is_some()
                                && (p.flags & mflags::PRIVATE == 0
                                    || std::ptr::eq(p.class, Rc::as_ptr(&cls)))
                        })
                    })
                };
                if !found && !declared {
                    self.warn(&format!(
                        "serialize(): \"{}\" returned as member variable from __sleep() but does not exist",
                        n.to_string_lossy()
                    ))?;
                }
            }
            sel
        } else {
            props
        };
        s.out
            .extend_from_slice(format!("O:{}:\"", name.len()).as_bytes());
        s.out.extend_from_slice(&name);
        s.out
            .extend_from_slice(format!("\":{}:{{", selected.len()).as_bytes());
        for (k, x) in selected.iter() {
            self.ser_key(s, k);
            self.ser(s, x, false)?;
        }
        s.out.push(b'}');
        Ok(())
    }

    fn ser_key(&mut self, s: &mut Ser, k: KeyRef<'_>) {
        match k {
            KeyRef::Int(i) => s.out.extend_from_slice(format!("i:{};", i).as_bytes()),
            KeyRef::Str(st) => {
                s.out
                    .extend_from_slice(format!("s:{}:\"", st.len()).as_bytes());
                s.out.extend_from_slice(st);
                s.out.extend_from_slice(b"\";");
            }
        }
    }
}

/// A property name and its mangled private/protected forms.
fn mangled_forms(cls: &Class, n: &PhpStr) -> Vec<PhpStr> {
    let mut v = vec![n.clone()];
    if n.first() == Some(&0) {
        return v;
    }
    let mut q = vec![0u8];
    q.extend_from_slice(&cls.name);
    q.push(0);
    q.extend_from_slice(n);
    v.push(PhpStr::from(q));
    let mut p = b"\0*\0".to_vec();
    p.extend_from_slice(n);
    v.push(PhpStr::from(p));
    v
}

fn serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.vals.first().cloned().unwrap_or_default();
    let out = vm.serialize_value(&v)?;
    Ok(Value::Str(PhpStr::from(out)))
}

// ---------------------------------------------------------------- unserialize
//
// Values are numbered as PHP numbers them (every value but array keys;
// `r:` takes a number, `R:` does not). Each value is built in a cell so
// that a later `R:n` can bind to it; cells nobody bound are unwrapped once
// the whole value is read. Error offsets follow PHP's scanner (observed on
// PHP 8.5): a malformed token is reported at its start, a bad back
// reference or key after its token, a missing closing at the position
// where it was expected.

struct Unser<'a> {
    d: &'a [u8],
    pos: usize,
    /// The cells of values by number (for `R:` and `r:`).
    vals: Vec<PhpRef>,
    /// Cells bound by `R:` (kept as references).
    shared: std::collections::HashSet<usize>,
    allowed: Option<Vec<PhpStr>>,
    max_depth: i64,
    depth: i64,
    /// __unserialize() (with its data) and __wakeup() calls, in the order
    /// objects were completed (innermost first, as PHP runs them).
    deferred: Vec<(ObjRef, Option<PhpArray>)>,
}

enum UErr {
    At(usize),
    Php(Unwind),
}

impl From<Unwind> for UErr {
    fn from(u: Unwind) -> UErr {
        UErr::Php(u)
    }
}

type UR<T> = Result<T, UErr>;

impl Unser<'_> {
    fn new<'a>(d: &'a [u8], allowed: Option<Vec<PhpStr>>, max_depth: i64) -> Unser<'a> {
        Unser {
            d,
            pos: 0,
            vals: Vec::new(),
            shared: std::collections::HashSet::new(),
            allowed,
            max_depth,
            depth: 0,
            deferred: Vec::new(),
        }
    }
    fn peek(&self) -> Option<u8> {
        self.d.get(self.pos).copied()
    }
    fn expect(&mut self, c: u8) -> UR<()> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(UErr::At(self.pos))
        }
    }
    /// Digits (with an optional sign when `signed`) followed by `end`; on
    /// failure, the error is at `tok` (the token start).
    fn num(&mut self, end: u8, signed: bool, tok: usize) -> UR<(bool, Vec<u8>)> {
        let mut neg = false;
        if signed && matches!(self.peek(), Some(b'+') | Some(b'-')) {
            neg = self.peek() == Some(b'-');
            self.pos += 1;
        }
        let st = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == st || self.peek() != Some(end) {
            return Err(UErr::At(tok));
        }
        let digits = self.d[st..self.pos].to_vec();
        self.pos += 1;
        Ok((neg, digits))
    }
    fn len(&mut self, end: u8, tok: usize) -> UR<usize> {
        let (_, d) = self.num(end, false, tok)?;
        std::str::from_utf8(&d)
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .ok_or(UErr::At(tok))
    }
    /// `len:"bytes"` after the type letter and colon.
    fn quoted(&mut self, tok: usize) -> UR<PhpStr> {
        let len = self.len(b':', tok)?;
        if self.peek() != Some(b'"') {
            return Err(UErr::At(tok));
        }
        self.pos += 1;
        // A length past the data is reported at the length (bug25378).
        if self.pos + len > self.d.len() {
            return Err(UErr::At(tok + 2));
        }
        let s = PhpStr::new(&self.d[self.pos..self.pos + len]);
        self.pos += len;
        self.expect(b'"')?;
        Ok(s)
    }
    /// A count read inside an object header (`O:..:"name":N:{`, `C:..:N:{`):
    /// errors are reported where the parse stopped (observed on PHP 8.5).
    fn count(&mut self) -> UR<(bool, usize)> {
        let mut neg = false;
        if matches!(self.peek(), Some(b'+') | Some(b'-')) {
            neg = self.peek() == Some(b'-');
            self.pos += 1;
        }
        let st = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == st {
            return Err(UErr::At(if self.peek() == Some(b':') {
                self.pos + 1
            } else {
                self.pos
            }));
        }
        let n = std::str::from_utf8(&self.d[st..self.pos])
            .ok()
            .and_then(|t| t.parse::<usize>().ok())
            .ok_or(UErr::At(self.pos))?;
        if self.peek() != Some(b':') {
            return Err(UErr::At(self.pos));
        }
        Ok((neg, n))
    }

    fn cell(&mut self, v: Value) -> PhpRef {
        let c = PhpRef::new(v);
        self.vals.push(c.clone());
        c
    }
}

/// An array key or value read by [`Vm::unser_value`].
enum Item {
    Key(Key),
    Val(Value),
}

impl Vm {
    /// One value (in a cell) or, with `as_key`, an array key.
    fn unser_value(&mut self, u: &mut Unser, as_key: bool) -> UR<Item> {
        let start = u.pos;
        let Some(t) = u.peek() else {
            return Err(UErr::At(start));
        };
        if t == b'N' {
            if u.d.get(start + 1) != Some(&b';') {
                return Err(UErr::At(start));
            }
            u.pos += 2;
            if as_key {
                return Err(UErr::At(u.pos));
            }
            return Ok(Item::Val(Value::Ref(u.cell(Value::Null))));
        }
        if u.d.get(start + 1) != Some(&b':') {
            return Err(UErr::At(start));
        }
        u.pos += 2;
        let v = match t {
            b'b' => {
                let v = match u.peek() {
                    Some(c @ (b'0' | b'1')) if u.d.get(u.pos + 1) == Some(&b';') => c == b'1',
                    _ => return Err(UErr::At(start)),
                };
                u.pos += 2;
                Value::Bool(v)
            }
            b'i' => {
                let (neg, d) = u.num(b';', true, start)?;
                let txt = std::str::from_utf8(&d).unwrap_or("0");
                let full = if neg {
                    format!("-{}", txt)
                } else {
                    txt.to_string()
                };
                match full.parse::<i64>() {
                    Ok(i) => Value::Int(i),
                    Err(_) => {
                        self.warn("unserialize(): Numerical result out of range")?;
                        Value::Int(if neg { i64::MIN } else { i64::MAX })
                    }
                }
            }
            b'd' => {
                let st = u.pos;
                while u.peek().is_some_and(|c| c != b';') {
                    u.pos += 1;
                }
                if u.peek() != Some(b';') {
                    return Err(UErr::At(start));
                }
                let txt = &u.d[st..u.pos];
                u.pos += 1;
                let ok_chars = txt
                    .iter()
                    .all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'));
                let f = match txt {
                    b"INF" => f64::INFINITY,
                    b"-INF" => f64::NEG_INFINITY,
                    b"NAN" => f64::NAN,
                    _ if ok_chars => std::str::from_utf8(txt)
                        .ok()
                        .and_then(|t| t.parse::<f64>().ok())
                        .ok_or(UErr::At(start))?,
                    _ => return Err(UErr::At(start)),
                };
                Value::Float(f)
            }
            b's' => {
                let s = u.quoted(start)?;
                u.expect(b';')?;
                Value::Str(s)
            }
            b'S' => {
                let len = u.len(b':', start)?;
                u.expect(b'"').map_err(|_| UErr::At(start))?;
                let mut out = Vec::new();
                while out.len() < len {
                    let c = u.peek().ok_or(UErr::At(u.pos))?;
                    if c == b'\\' {
                        let h = u.d.get(u.pos + 1..u.pos + 3).ok_or(UErr::At(u.pos))?;
                        let hs = std::str::from_utf8(h).map_err(|_| UErr::At(u.pos))?;
                        out.push(u8::from_str_radix(hs, 16).map_err(|_| UErr::At(u.pos))?);
                        u.pos += 3;
                    } else {
                        out.push(c);
                        u.pos += 1;
                    }
                }
                u.expect(b'"')?;
                u.expect(b';')?;
                self.deprecated("unserialize(): Unserializing the 'S' format is deprecated")?;
                Value::Str(PhpStr::from(out))
            }
            b'a' if as_key => {
                // An array as a key: refused after its header.
                u.len(b':', start)?;
                u.expect(b'{').map_err(|_| UErr::At(start))?;
                return Err(UErr::At(u.pos));
            }
            b'R' | b'r' => {
                let n = u.len(b';', start)?;
                if as_key {
                    return Err(UErr::At(u.pos));
                }
                if n == 0 || n > u.vals.len() {
                    return Err(UErr::At(u.pos));
                }
                let target = u.vals[n - 1].clone();
                if t == b'R' {
                    u.shared.insert(target.addr());
                    return Ok(Item::Val(Value::Ref(target)));
                }
                let v = target.get();
                return Ok(Item::Val(Value::Ref(u.cell(v))));
            }
            _ if as_key => return Err(UErr::At(start)),
            b'a' => return self.unser_array(u, start).map(Item::Val),
            b'O' | b'C' => return self.unser_object(u, t, start).map(Item::Val),
            b'E' => {
                let full = u.quoted(start)?;
                u.expect(b';')?;
                let v = self.unser_enum(&full, start)?;
                return Ok(Item::Val(Value::Ref(u.cell(v))));
            }
            _ => return Err(UErr::At(start)),
        };
        if as_key {
            return match v {
                Value::Int(i) => Ok(Item::Key(Key::Int(i))),
                Value::Str(s) => Ok(Item::Key(Key::from_php_str(&s))),
                _ => Err(UErr::At(u.pos)),
            };
        }
        Ok(Item::Val(Value::Ref(u.cell(v))))
    }

    fn unser_key(&mut self, u: &mut Unser) -> UR<Key> {
        match self.unser_value(u, true)? {
            Item::Key(k) => Ok(k),
            Item::Val(_) => Err(UErr::At(u.pos)),
        }
    }

    fn unser_elem(&mut self, u: &mut Unser) -> UR<Value> {
        match self.unser_value(u, false)? {
            Item::Val(v) => Ok(v),
            Item::Key(_) => Err(UErr::At(u.pos)),
        }
    }

    fn depth_check(&mut self, u: &mut Unser, start: usize) -> UR<()> {
        u.depth += 1;
        if u.max_depth > 0 && u.depth > u.max_depth {
            self.warn(&format!(
                "unserialize(): Maximum depth of {} exceeded. The depth limit can be changed using the max_depth unserialize() option or the unserialize_max_depth ini setting",
                u.max_depth
            ))?;
            return Err(UErr::At(start));
        }
        Ok(())
    }

    fn unser_array(&mut self, u: &mut Unser, start: usize) -> UR<Value> {
        let n = u.len(b':', start)?;
        u.expect(b'{').map_err(|_| UErr::At(start))?;
        self.depth_check(u, start)?;
        let cell = u.cell(Value::Null);
        let mut arr = PhpArray::new();
        for _ in 0..n {
            let key = self.unser_key(u)?;
            let v = self.unser_elem(u)?;
            arr.insert(key, v);
        }
        u.expect(b'}')?;
        u.depth -= 1;
        cell.set(Value::Array(arr));
        Ok(Value::Ref(cell))
    }

    fn unser_class(&mut self, u: &Unser, name: &PhpStr) -> R<Option<Rc<Class>>> {
        if let Some(al) = &u.allowed
            && !al.iter().any(|a| a.eq_ignore_ascii_case(name))
        {
            return Ok(None);
        }
        let c = self.find_class(name, true)?;
        if c.is_none() {
            let cb = self
                .get_ini(b"unserialize_callback_func")
                .unwrap_or_default();
            if !cb.is_empty() {
                self.call_value(&Value::Str(cb.clone()), vec![Value::Str(name.clone())])?;
                let c = self.find_class(name, true)?;
                if c.is_none() {
                    self.warn(&format!(
                        "unserialize(): Function {}() hasn't defined the class it was called for",
                        cb.to_string_lossy()
                    ))?;
                }
                return Ok(c);
            }
        }
        Ok(c)
    }

    fn not_allowed(&mut self, c: &Class) -> UErr {
        UErr::Php(self.throw_error_msg(
            "Exception",
            &format!("Unserialization of '{}' is not allowed", c.name.dname()),
        ))
    }

    fn unser_object(&mut self, u: &mut Unser, t: u8, start: usize) -> UR<Value> {
        let name = u.quoted(start)?;
        u.expect(b':')?;
        let cls = self.unser_class(u, &name)?;
        if t == b'C' {
            let (_, len) = u.count()?;
            let at = u.pos;
            u.pos += 1;
            if len > u.d.len() {
                return Err(UErr::At(at));
            }
            u.expect(b'{')?;
            if u.pos + len > u.d.len() {
                return Err(UErr::At(start));
            }
            let data = PhpStr::new(&u.d[u.pos..u.pos + len]);
            u.pos += len;
            u.expect(b'}')?;
            let Some(cls) = cls else {
                // Observed on PHP 8.5: an incomplete object, which has no
                // unserializer either.
                let ic = self.find_class(b"__PHP_Incomplete_Class", false)?.unwrap();
                let o = self.instantiate_raw(&ic)?;
                o.obj().props.borrow_mut().insert(
                    Key::from("__PHP_Incomplete_Class_Name"),
                    Value::Str(name.clone()),
                );
                let cell = u.cell(Value::Object(o));
                self.warn("Class __PHP_Incomplete_Class has no unserializer")?;
                return Ok(Value::Ref(cell));
            };
            if cls.has_flag(cflags::NOT_SERIALIZABLE) {
                return Err(self.not_allowed(&cls));
            }
            let o = self.instantiate(&cls)?;
            let cell = u.cell(Value::Object(o.clone()));
            if cls.find_method(b"unserialize").is_none() {
                self.warn(&format!("Class {} has no unserializer", cls.name.dname()))?;
            } else {
                self.call_method(&o, b"unserialize", vec![Value::Str(data)])?;
            }
            return Ok(Value::Ref(cell));
        }
        let (neg, n) = u.count()?;
        let at = u.pos;
        if neg && n != 0 {
            return Err(UErr::At(at));
        }
        // More properties than bytes left (unserialize_large).
        if n > u.d.len() - u.pos {
            return Err(UErr::At(at));
        }
        u.pos += 1;
        u.expect(b'{')?;
        let (o, incomplete) = match &cls {
            Some(c) => {
                if c.is_enum() {
                    return Err(UErr::At(start));
                }
                if c.has_flag(cflags::NOT_SERIALIZABLE)
                    || matches!(c.lname.as_bytes(), b"closure" | b"generator")
                {
                    return Err(self.not_allowed(c));
                }
                if c.kind != crate::class::ClassKind::Class || c.has_flag(cflags::ABSTRACT) {
                    let o = self.instantiate(c);
                    return Err(UErr::Php(o.err().unwrap()));
                }
                (self.instantiate_raw(c)?, false)
            }
            None => {
                let ic = self.find_class(b"__PHP_Incomplete_Class", false)?.unwrap();
                let o = self.instantiate_raw(&ic)?;
                o.obj().props.borrow_mut().insert(
                    Key::from("__PHP_Incomplete_Class_Name"),
                    Value::Str(name.clone()),
                );
                (o, true)
            }
        };
        let cell = u.cell(Value::Object(o.clone()));
        if let Some(c) = &cls
            && c.find_method(b"__unserialize").is_none()
            && c.interfaces
                .borrow()
                .iter()
                .any(|i| i.lname.as_bytes() == b"serializable")
        {
            self.warn(&format!(
                "Erroneous data format for unserializing '{}'",
                c.name.dname()
            ))?;
            return Err(UErr::At(u.pos));
        }
        self.depth_check(u, start)?;
        let uses_unserialize = !incomplete
            && cls
                .as_ref()
                .unwrap()
                .find_method(b"__unserialize")
                .is_some();
        let mut data = PhpArray::new();
        for _ in 0..n {
            let key = self.unser_key(u)?;
            let v = self.unser_elem(u)?;
            if uses_unserialize {
                data.insert(key, v);
                continue;
            }
            self.unser_set_prop(&o, key, v)?;
        }
        u.expect(b'}')?;
        u.depth -= 1;
        if uses_unserialize {
            u.deferred.push((o.clone(), Some(data)));
        } else if !incomplete && cls.as_ref().unwrap().find_method(b"__wakeup").is_some() {
            u.deferred.push((o.clone(), None));
        }
        Ok(Value::Ref(cell))
    }

    /// Unwraps the cells nobody bound with `R:` (see [`Unser`]).
    fn unser_finish(
        &mut self,
        v: Value,
        shared: &std::collections::HashSet<usize>,
        seen: &mut std::collections::HashSet<usize>,
    ) -> Value {
        match v {
            Value::Ref(c) => {
                if shared.contains(&c.addr()) {
                    if seen.insert(c.addr()) {
                        let inner = c.get();
                        let inner = self.unser_finish(inner, shared, seen);
                        drop(c.set(inner));
                    }
                    Value::Ref(c)
                } else {
                    let inner = c.get();
                    drop(c);
                    self.unser_finish(inner, shared, seen)
                }
            }
            Value::Array(a) => {
                if !a
                    .iter()
                    .any(|(_, x)| matches!(x, Value::Ref(_) | Value::Array(_) | Value::Object(_)))
                {
                    return Value::Array(a);
                }
                let mut out = PhpArray::new();
                for (k, x) in a.iter() {
                    let x = self.unser_finish(x.clone(), shared, seen);
                    out.insert(k.to_owned(), x);
                }
                Value::Array(out)
            }
            Value::Object(o) => {
                let key = (o.handle() as usize) | (1 << 62);
                if seen.insert(key) {
                    let slots: Vec<Value> = o.obj().slots.borrow().clone();
                    for (i, s) in slots.into_iter().enumerate() {
                        if matches!(s, Value::Ref(_) | Value::Array(_)) {
                            let s = self.unser_finish(s, shared, seen);
                            let old = std::mem::replace(&mut o.obj().slots.borrow_mut()[i], s);
                            drop(old);
                        }
                    }
                    let props: Vec<(Key, Value)> = o
                        .obj()
                        .props
                        .borrow()
                        .iter()
                        .map(|(k, x)| (k.to_owned(), x.clone()))
                        .collect();
                    for (k, x) in props {
                        if matches!(x, Value::Ref(_) | Value::Array(_)) {
                            let x = self.unser_finish(x, shared, seen);
                            o.obj().props.borrow_mut().insert(k, x);
                        }
                    }
                }
                Value::Object(o)
            }
            other => other,
        }
    }

    /// Runs a parse, then finishes the values and calls __unserialize()
    /// and __wakeup() (in that order, after the whole value is built).
    fn unser_run(&mut self, u: &mut Unser) -> UR<Value> {
        let r = self.unser_elem(u);
        let shared = std::mem::take(&mut u.shared);
        let mut seen = std::collections::HashSet::new();
        let v = {
            let v = r?;
            self.unser_finish(v, &shared, &mut seen)
        };
        // Every value is finished before any of the calls runs.
        let deferred: Vec<(ObjRef, Option<Value>)> = std::mem::take(&mut u.deferred)
            .into_iter()
            .map(|(o, d)| {
                let d = d.map(|d| self.unser_finish(Value::Array(d), &shared, &mut seen));
                (o, d)
            })
            .collect();
        for (o, d) in deferred {
            match d {
                Some(d) => {
                    self.call_method(&o, b"__unserialize", vec![d])?;
                }
                None => {
                    self.call_method(&o, b"__wakeup", Vec::new())?;
                }
            }
        }
        Ok(v)
    }

    /// Sets an unserialized property (mangled names select private and
    /// protected properties).
    pub(crate) fn unser_set_prop(&mut self, o: &ObjRef, key: Key, v: Value) -> R<()> {
        let cls = self.class_of(o);
        let name = match &key {
            Key::Str(s) => s.clone(),
            Key::Int(i) => PhpStr::from(i.to_string()),
        };
        let (bare, owner): (PhpStr, Option<Vec<u8>>) = if name.first() == Some(&0) {
            match name[1..].iter().position(|&c| c == 0) {
                Some(p) => (PhpStr::new(&name[p + 2..]), Some(name[1..p + 1].to_vec())),
                None => (name.clone(), None),
            }
        } else {
            (name.clone(), None)
        };
        // A declared property: its slot.
        let idx = {
            let map = cls.prop_map.borrow();
            let props = cls.props.borrow();
            map.get(&bare).and_then(|idxs| {
                idxs.iter()
                    .map(|&i| props[i as usize].clone())
                    .find(|p| match &owner {
                        Some(o) if o == b"*" => p.flags & mflags::PROTECTED != 0,
                        Some(o) => {
                            p.flags & mflags::PRIVATE != 0
                                && unsafe { (*p.class).name.as_bytes() == o.as_slice() }
                        }
                        None => p.flags & mflags::PUBLIC != 0 || true,
                    })
            })
        };
        // A property whose visibility changed since the data was written
        // takes it anyway (bug49649).
        let idx = idx.or_else(|| {
            let map = cls.prop_map.borrow();
            let props = cls.props.borrow();
            map.get(&bare)
                .and_then(|idxs| idxs.first().map(|&i| props[i as usize].clone()))
                .filter(|p| {
                    owner.is_none()
                        || p.flags & mflags::PRIVATE == 0
                        || std::ptr::eq(p.class, Rc::as_ptr(&cls))
                })
        });
        if let Some(p) = idx {
            if let Some(t) = &p.ty {
                let vv = v.deref();
                if let crate::vm::types::Check::Fail =
                    self.check_type(&vv, t, true, p.class, Rc::as_ptr(&cls))?
                {
                    return self.type_error(&format!(
                        "Cannot assign {} to property {}::${} of type {}",
                        crate::class::TypeName::tname(&vv),
                        unsafe { (*p.class).name.dname() },
                        p.name.dname(),
                        t.display()
                    ));
                }
            }
            let mut slots = o.obj().slots.borrow_mut();
            let old = std::mem::replace(&mut slots[p.slot as usize], v);
            drop(slots);
            drop(old);
            return Ok(());
        }
        // Undeclared properties keep their (mangled) names, as string keys
        // (observed on PHP 8.5: "\0*\0x" stays protected; bug55798).
        let pkey = if owner.is_some() {
            name.clone()
        } else {
            bare.clone()
        };
        let exists = o.obj().props.borrow().get_str(&pkey).is_some();
        if !exists
            && !cls.has_flag(cflags::ALLOW_DYNAMIC)
            && cls.lname.as_bytes() != b"__php_incomplete_class"
            && !self.class_allows_dynamic(&cls)
        {
            self.deprecated(&format!(
                "Creation of dynamic property {}::${} is deprecated",
                cls.name.dname(),
                bare.dname()
            ))?;
        }
        let old = o.obj().props.borrow_mut().insert(Key::Str(pkey), v);
        drop(old);
        Ok(())
    }

    fn class_allows_dynamic(&self, cls: &Class) -> bool {
        let mut c = Some(cls);
        while let Some(k) = c {
            if k.has_flag(cflags::ALLOW_DYNAMIC) {
                return true;
            }
            c = k.parent.as_deref();
        }
        false
    }

    fn unser_enum(&mut self, full: &PhpStr, start: usize) -> UR<Value> {
        let Some(p) = full.iter().position(|&c| c == b':') else {
            return Err(UErr::At(start));
        };
        let cname = &full[..p];
        let case = PhpStr::new(&full[p + 1..]);
        let Some(cls) = self.find_class(cname, true)? else {
            self.warn(&format!(
                "unserialize(): Class '{}' not found",
                String::from_utf8_lossy(cname)
            ))?;
            return Err(UErr::At(start));
        };
        if !cls.is_enum() {
            self.warn(&format!(
                "unserialize(): Class '{}' is not an enum",
                String::from_utf8_lossy(cname)
            ))?;
            return Err(UErr::At(start));
        }
        match cls.find_const(&case) {
            Some(c) if c.flags & mflags::CASE != 0 => Ok(self.enum_case(&cls, &case)?),
            _ => {
                self.warn(&format!(
                    "unserialize(): Undefined constant {}::{}",
                    String::from_utf8_lossy(cname),
                    case.to_string_lossy()
                ))?;
                Err(UErr::At(start))
            }
        }
    }

    pub fn unserialize_bytes(&mut self, data: &[u8], options: &PhpArray) -> R<Value> {
        let allowed = match options.get_str(&PhpStr::from("allowed_classes")) {
            None => None,
            Some(Value::Bool(true)) => None,
            Some(Value::Bool(false)) => Some(Vec::new()),
            Some(Value::Array(a)) => {
                let mut v = Vec::new();
                for (_, x) in a.iter() {
                    let x = x.deref();
                    let name = match &x {
                        Value::Str(s) => s.clone(),
                        Value::Object(_) => self.to_php_str(&x)?,
                        other => {
                            return self.type_error(&format!(
                                "unserialize(): Option \"allowed_classes\" must be an array of class names, {} given",
                                Vm::type_name_given(other)
                            ));
                        }
                    };
                    // Observed on PHP 8.5: names must look like class names.
                    let valid = name.iter().all(|&c| {
                        c.is_ascii_alphanumeric() || c == b'_' || c == b'\\' || c >= 0x80
                    });
                    if !valid {
                        // The name is shown up to a NUL byte.
                        let shown = name.split(|&c| c == 0).next().unwrap_or(&[]);
                        return self.value_error(&format!(
                            "unserialize(): Option \"allowed_classes\" must be an array of class names, \"{}\" given",
                            String::from_utf8_lossy(shown)
                        ));
                    }
                    v.push(name);
                }
                Some(v)
            }
            Some(other) => {
                return self.type_error(&format!(
                    "unserialize(): Option \"allowed_classes\" must be of type array|bool, {} given",
                    Vm::type_name_given(&other.deref())
                ));
            }
        };
        let max_depth = match options.get_str(&PhpStr::from("max_depth")) {
            Some(Value::Int(n)) => {
                if *n < 0 {
                    return self.value_error(
                        "unserialize(): Option \"max_depth\" must be greater than or equal to 0",
                    );
                }
                *n
            }
            Some(other) => {
                return self.type_error(&format!(
                    "unserialize(): Option \"max_depth\" must be of type int, {} given",
                    crate::class::TypeName::tname(&other.deref())
                ));
            }
            None => self
                .get_ini(b"unserialize_max_depth")
                .map(|s| goro_runtime::convert::str_to_int(&s))
                .unwrap_or(4096),
        };
        let mut u = Unser::new(data, allowed, max_depth);
        let r = self.unser_run(&mut u);
        match r {
            Ok(v) => {
                if u.pos < data.len() {
                    self.warn(&format!(
                        "unserialize(): Extra data starting at offset {} of {} bytes",
                        u.pos,
                        data.len()
                    ))?;
                }
                Ok(v.into_deref())
            }
            Err(UErr::At(off)) => {
                // Objects of a failed unserialize are not destructed
                // (bug72663).
                for c in &u.vals {
                    if let Value::Object(o) = c.get() {
                        o.mark_destructor_called();
                    }
                }
                if !data.is_empty() {
                    self.warn(&format!(
                        "unserialize(): Error at offset {} of {} bytes",
                        off.min(data.len()),
                        data.len()
                    ))?;
                }
                Ok(Value::Bool(false))
            }
            Err(UErr::Php(e)) => {
                for c in &u.vals {
                    if let Value::Object(o) = c.get() {
                        o.mark_destructor_called();
                    }
                }
                Err(e)
            }
        }
    }
}

impl Vm {
    /// Unserializes the value at the start of `data`: the value and the
    /// number of bytes it took (no warnings; `None` on errors).
    pub fn unserialize_prefix(&mut self, data: &[u8]) -> Option<(Value, usize)> {
        let mut u = Unser::new(data, None, 4096);
        let v = self.unser_run(&mut u).ok()?;
        Some((v.into_deref(), u.pos))
    }

    /// The whole of `data` as one serialized value.
    pub fn unserialize_quiet(&mut self, data: &[u8]) -> Option<Value> {
        match self.unserialize_prefix(data) {
            Some((v, n)) if n == data.len() => Some(v),
            _ => None,
        }
    }
}

fn unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
    let opts = a.array(1);
    vm.unserialize_bytes(&d, &opts)
}
