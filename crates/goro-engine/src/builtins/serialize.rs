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
                if let Some(&n) = s.refs.get(&r.addr()) {
                    s.n += 1;
                    s.out.extend_from_slice(format!("R:{};", n).as_bytes());
                    return Ok(());
                }
                s.refs.insert(r.addr(), s.n + 1);
            }
            let inner = r.get();
            return self.ser(s, &inner, _in_array);
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
                Value::Null => s.out.extend_from_slice(b"N;"),
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
        let props = if skip_init {
            self.plain_props_array(o, true)
        } else {
            let t = self.lazy_realize(o)?;
            self.object_props_array(&t, true)
        };
        let selected: PhpArray = if let Some(f) = cls.find_method(b"__sleep") {
            let r = self.call_func_this(&f, Some(o.clone()), Vec::new())?;
            let Value::Array(names) = r else {
                self.warn(&format!(
                    "serialize(): __sleep should return an array only containing the names of instance-variables to serialize"
                ))?;
                s.out.extend_from_slice(b"N;");
                return Ok(());
            };
            let mut sel = PhpArray::new();
            for (_, n) in names.iter() {
                let n = match n.deref() {
                    Value::Str(st) => st,
                    other => self.to_php_str(&other)?,
                };
                // The name, or its mangled forms.
                let cands = mangled_forms(&cls, &n);
                let mut found = false;
                for c in cands {
                    if let Some(v) = props.get_str(&c) {
                        sel.insert(Key::Str(c), v.clone());
                        found = true;
                        break;
                    }
                }
                if !found {
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
    let mut p = b"\0*\0".to_vec();
    p.extend_from_slice(n);
    v.push(PhpStr::from(p));
    let mut c = Some(cls);
    while let Some(k) = c {
        let mut q = vec![0u8];
        q.extend_from_slice(&k.name);
        q.push(0);
        q.extend_from_slice(n);
        v.push(PhpStr::from(q));
        c = k.parent.as_deref();
    }
    v
}

fn serialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.vals.first().cloned().unwrap_or_default();
    let out = vm.serialize_value(&v)?;
    Ok(Value::Str(PhpStr::from(out)))
}

// ---------------------------------------------------------------- unserialize

struct Unser<'a> {
    d: &'a [u8],
    pos: usize,
    /// Values by number (for R: and r:).
    vals: Vec<Value>,
    allowed: Option<Vec<PhpStr>>,
    max_depth: i64,
    depth: i64,
    wakeups: Vec<ObjRef>,
    unserializes: Vec<(ObjRef, PhpArray)>,
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
    fn int_until(&mut self, end: u8) -> UR<i64> {
        let start = self.pos;
        while self.pos < self.d.len() && self.d[self.pos] != end {
            self.pos += 1;
        }
        let s = &self.d[start..self.pos];
        let txt = std::str::from_utf8(s).map_err(|_| UErr::At(start))?;
        let txt = txt.strip_prefix('+').unwrap_or(txt);
        let v = txt.parse::<i64>().map_err(|_| UErr::At(start))?;
        self.expect(end)?;
        Ok(v)
    }
    fn uint_until(&mut self, end: u8) -> UR<usize> {
        let start = self.pos;
        let v = self.int_until(end)?;
        if v < 0 {
            return Err(UErr::At(start));
        }
        Ok(v as usize)
    }
    fn string_body(&mut self) -> UR<PhpStr> {
        let start = self.pos;
        let len = self.uint_until(b':')?;
        self.expect(b'"')?;
        if self.pos + len > self.d.len() {
            return Err(UErr::At(start - 2));
        }
        let s = PhpStr::new(&self.d[self.pos..self.pos + len]);
        self.pos += len;
        self.expect(b'"')?;
        Ok(s)
    }
}

impl Vm {
    fn unser_value(&mut self, u: &mut Unser, as_key: bool) -> UR<Value> {
        let start = self.pos_of(u);
        let Some(t) = u.peek() else {
            return Err(UErr::At(start));
        };
        u.pos += 1;
        if t == b'N' {
            u.expect(b';')?;
            if !as_key {
                u.vals.push(Value::Null);
            }
            return Ok(Value::Null);
        }
        u.expect(b':').map_err(|_| UErr::At(start))?;
        let v = match t {
            b'b' => {
                let v = u.int_until(b';')?;
                if v != 0 && v != 1 {
                    return Err(UErr::At(start));
                }
                Value::Bool(v == 1)
            }
            b'i' => Value::Int(u.int_until(b';')?),
            b'd' => {
                let s = u.pos;
                while u.pos < u.d.len() && u.d[u.pos] != b';' {
                    u.pos += 1;
                }
                let txt = &u.d[s..u.pos];
                u.expect(b';')?;
                let f = match txt {
                    b"INF" => f64::INFINITY,
                    b"-INF" => f64::NEG_INFINITY,
                    b"NAN" => f64::NAN,
                    _ => {
                        let t = std::str::from_utf8(txt).map_err(|_| UErr::At(s))?;
                        t.parse::<f64>().map_err(|_| UErr::At(start))?
                    }
                };
                Value::Float(f)
            }
            b's' => {
                let st = u.string_body()?;
                u.expect(b';')?;
                Value::Str(st)
            }
            b'S' => {
                // Escaped string.
                let len = u.uint_until(b':')?;
                u.expect(b'"')?;
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
                Value::Str(PhpStr::from(out))
            }
            b'a' if !as_key => return self.unser_array(u, start),
            b'O' | b'C' if !as_key => return self.unser_object(u, t, start),
            b'E' if !as_key => {
                let full = u.string_body()?;
                u.expect(b';')?;
                let v = self.unser_enum(&full, start)?;
                u.vals.push(v.clone());
                return Ok(v);
            }
            b'R' | b'r' if !as_key => {
                let n = u.uint_until(b';')?;
                if n == 0 || n > u.vals.len() {
                    return Err(UErr::At(start));
                }
                let target = u.vals[n - 1].clone();
                if t == b'R' {
                    // A reference to an earlier value.
                    let v = match &target {
                        Value::Ref(r) => Value::Ref(r.clone()),
                        other => {
                            let r = PhpRef::new(other.clone());
                            u.vals[n - 1] = Value::Ref(r.clone());
                            Value::Ref(r)
                        }
                    };
                    return Ok(v);
                }
                u.vals.push(target.deref());
                return Ok(target.deref());
            }
            _ => return Err(UErr::At(start)),
        };
        if !as_key {
            u.vals.push(v.clone());
        }
        Ok(v)
    }

    fn pos_of(&self, u: &Unser) -> usize {
        u.pos
    }

    fn unser_array(&mut self, u: &mut Unser, start: usize) -> UR<Value> {
        let n = u.uint_until(b':')?;
        u.expect(b'{')?;
        u.depth += 1;
        if u.max_depth > 0 && u.depth > u.max_depth {
            self.warn(&format!(
                "unserialize(): Maximum depth of {} exceeded. The depth limit can be changed using the max_depth unserialize() option or the unserialize_max_depth ini setting",
                u.max_depth
            ))?;
            return Err(UErr::At(start));
        }
        let idx = u.vals.len();
        u.vals.push(Value::Null);
        let mut arr = PhpArray::new();
        for _ in 0..n {
            let ks = u.pos;
            let k = self.unser_value(u, true)?;
            let key = match k {
                Value::Int(i) => Key::Int(i),
                Value::Str(s) => Key::from_php_str(&s),
                _ => return Err(UErr::At(ks)),
            };
            let v = self.unser_value(u, false)?;
            arr.insert(key, v);
        }
        u.expect(b'}')?;
        u.depth -= 1;
        // References into this array point at copies; fix up the slot.
        let v = Value::Array(arr);
        u.vals[idx] = v.clone();
        let _ = start;
        Ok(v)
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
                self.call_value(&Value::Str(cb), vec![Value::Str(name.clone())])?;
                return self.find_class(name, false);
            }
        }
        Ok(c)
    }

    fn unser_object(&mut self, u: &mut Unser, t: u8, start: usize) -> UR<Value> {
        let name = u.string_body()?;
        u.expect(b':')?;
        let cls = self.unser_class(u, &name)?;
        if t == b'C' {
            let len = u.uint_until(b':')?;
            u.expect(b'{')?;
            if u.pos + len > u.d.len() {
                return Err(UErr::At(start));
            }
            let data = PhpStr::new(&u.d[u.pos..u.pos + len]);
            u.pos += len;
            u.expect(b'}')?;
            let Some(cls) = cls else {
                return Err(UErr::At(start));
            };
            if cls.has_flag(cflags::NOT_SERIALIZABLE) {
                return Err(UErr::Php(
                    self.throw::<()>(
                        "Exception",
                        &format!("Unserialization of '{}' is not allowed", cls.name.dname()),
                    )
                    .unwrap_err(),
                ));
            }
            let o = self.instantiate(&cls)?;
            u.vals.push(Value::Object(o.clone()));
            if cls.find_method(b"unserialize").is_none() {
                self.warn(&format!("Class {} has no unserializer", cls.name.dname()))?;
            } else {
                self.call_method(&o, b"unserialize", vec![Value::Str(data)])?;
            }
            return Ok(Value::Object(o));
        }
        let n = u.uint_until(b':')?;
        u.expect(b'{')?;
        let (o, incomplete) = match &cls {
            Some(c) => {
                if c.is_enum() {
                    return Err(UErr::At(start));
                }
                if c.has_flag(cflags::NOT_SERIALIZABLE)
                    || matches!(c.lname.as_bytes(), b"closure" | b"generator")
                {
                    return Err(UErr::Php(self.throw_error_msg(
                        "Exception",
                        &format!("Unserialization of '{}' is not allowed", c.name.dname()),
                    )));
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
        u.vals.push(Value::Object(o.clone()));
        u.depth += 1;
        let uses_unserialize = !incomplete
            && cls
                .as_ref()
                .unwrap()
                .find_method(b"__unserialize")
                .is_some();
        let mut data = PhpArray::new();
        for _ in 0..n {
            let ks = u.pos;
            let k = self.unser_value(u, true)?;
            let v = self.unser_value(u, false)?;
            let key = match k {
                Value::Int(i) => Key::Int(i),
                Value::Str(s) => Key::from_php_str(&s),
                _ => return Err(UErr::At(ks)),
            };
            if uses_unserialize {
                data.insert(key, v);
                continue;
            }
            self.unser_set_prop(&o, key, v)?;
        }
        u.expect(b'}')?;
        u.depth -= 1;
        if uses_unserialize {
            u.unserializes.push((o.clone(), data));
        } else if !incomplete && cls.as_ref().unwrap().find_method(b"__wakeup").is_some() {
            u.wakeups.push(o.clone());
        }
        Ok(Value::Object(o))
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
        if let Some(p) = idx {
            if let Some(t) = &p.ty {
                let vv = v.deref();
                match self.check_type(&vv, t, true, p.class, Rc::as_ptr(&cls))? {
                    crate::vm::types::Check::Fail => {
                        return self.type_error(&format!(
                            "Cannot assign {} to property {}::${} of type {}",
                            crate::class::TypeName::tname(&vv),
                            unsafe { (*p.class).name.dname() },
                            p.name.dname(),
                            t.display()
                        ));
                    }
                    _ => {}
                }
            }
            let mut slots = o.obj().slots.borrow_mut();
            let old = std::mem::replace(&mut slots[p.slot as usize], v);
            drop(slots);
            drop(old);
            return Ok(());
        }
        if !cls.has_flag(cflags::ALLOW_DYNAMIC)
            && cls.lname.as_bytes() != b"__php_incomplete_class"
            && !self.class_allows_dynamic(&cls)
        {
            self.deprecated(&format!(
                "Creation of dynamic property {}::${} is deprecated",
                cls.name.dname(),
                bare.dname()
            ))?;
        }
        let old = o
            .obj()
            .props
            .borrow_mut()
            .insert(Key::from_php_str(&bare), v);
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
                    match x.deref() {
                        Value::Str(s) => v.push(s),
                        _ => {
                            return self.type_error("unserialize(): Option \"allowed_classes\" must be an array of class names, int given");
                        }
                    }
                }
                Some(v)
            }
            Some(other) => {
                return self.type_error(&format!(
                    "unserialize(): Option \"allowed_classes\" must be an array or of type bool, {} given",
                    crate::class::TypeName::tname(&other.deref())
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
        let mut u = Unser {
            d: data,
            pos: 0,
            vals: Vec::new(),
            allowed,
            max_depth,
            depth: 0,
            wakeups: Vec::new(),
            unserializes: Vec::new(),
        };
        let r = self.unser_value(&mut u, false);
        // __unserialize / __wakeup run after the whole value is built.
        let unserializes = std::mem::take(&mut u.unserializes);
        let wakeups = std::mem::take(&mut u.wakeups);
        match r {
            Ok(v) => {
                for (o, d) in unserializes {
                    self.call_method(&o, b"__unserialize", vec![Value::Array(d)])?;
                }
                for o in wakeups {
                    self.call_method(&o, b"__wakeup", Vec::new())?;
                }
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
                if !data.is_empty() {
                    self.warn(&format!(
                        "unserialize(): Error at offset {} of {} bytes",
                        off.min(data.len()),
                        data.len()
                    ))?;
                }
                Ok(Value::Bool(false))
            }
            Err(UErr::Php(e)) => Err(e),
        }
    }
}

impl Vm {
    /// Unserializes the value at the start of `data`: the value and the
    /// number of bytes it took (no warnings; `None` on errors).
    pub fn unserialize_prefix(&mut self, data: &[u8]) -> Option<(Value, usize)> {
        let mut u = Unser {
            d: data,
            pos: 0,
            vals: Vec::new(),
            allowed: None,
            max_depth: 4096,
            depth: 0,
            wakeups: Vec::new(),
            unserializes: Vec::new(),
        };
        let r = self.unser_value(&mut u, false);
        let unserializes = std::mem::take(&mut u.unserializes);
        let wakeups = std::mem::take(&mut u.wakeups);
        let v = r.ok()?;
        for (o, d) in unserializes {
            self.call_method(&o, b"__unserialize", vec![Value::Array(d)])
                .ok()?;
        }
        for o in wakeups {
            self.call_method(&o, b"__wakeup", Vec::new()).ok()?;
        }
        Some((v.into_deref(), u.pos))
    }

    /// The whole of `data` as one serialized value.
    pub fn unserialize_quiet(&mut self, data: &[u8]) -> Result<Value, ()> {
        match self.unserialize_prefix(data) {
            Some((v, n)) if n == data.len() => Ok(v),
            _ => Err(()),
        }
    }
}

fn unserialize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = a.str(0);
    let opts = a.array(1);
    vm.unserialize_bytes(&d, &opts)
}
