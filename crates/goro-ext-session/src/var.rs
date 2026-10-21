//! php_var_serialize / php_var_unserialize with a shared var_hash, as used by
//! the session serializers (ext/standard/var.c, var_unserializer.re).
//!
//! The session encoders serialize every session variable with one shared
//! var_hash, so `r:`/`R:` back-references may point across variables. Objects
//! of internal classes with their own serialization are delegated to the
//! standard `serialize()` / `unserialize()` functions.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{PhpObject, Visibility};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

thread_local! {
    /// Set when unserialization hit a fatal error (zend_bailout).
    pub static BAILOUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn lower(b: &[u8]) -> Vec<u8> {
    b.iter().map(|c| c.to_ascii_lowercase()).collect()
}

fn is_internal_property(name: &[u8]) -> bool {
    name.starts_with(b"__spl_") || name.starts_with(b"__reflection_")
        || name.starts_with(b"__timestamp") || name.starts_with(b"__enum_")
        || name.starts_with(b"__fiber_") || name.starts_with(b"__ctor_")
        || name.starts_with(b"__clone_") || name.starts_with(b"__destructed")
        || name.starts_with(b"__weak_ref_id") || name.starts_with(b"__sxml_")
        || name.starts_with(b"__dir_") || name.starts_with(b"__pkey_id")
        || name.starts_with(b"__lazy_")
}

/// Whether objects of this class are plain user objects whose serialization
/// we handle ourselves (no internal ancestor with custom serialization).
fn is_user_class(vm: &Vm, class_lower: &[u8]) -> bool {
    if class_lower == b"stdclass" || class_lower == b"__php_incomplete_class" {
        return true;
    }
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        if vm.native_class_handlers.contains_key(&cur) {
            return false;
        }
        match vm.classes.get(&cur) {
            Some(ce) => {
                if ce.filename.is_none() && cur.as_slice() != b"stdclass" {
                    return false;
                }
                match &ce.parent {
                    Some(p) => cur = lower(p),
                    None => return true,
                }
            }
            None => return cur.as_slice() == b"stdclass",
        }
    }
    false
}

/// Find the declared (non-static) property `name` in the class chain.
fn find_prop_decl(vm: &Vm, class_lower: &[u8], name: &[u8]) -> Option<(Visibility, Vec<u8>)> {
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        let ce = vm.classes.get(&cur)?;
        if let Some(p) = ce.properties.iter().find(|p| p.name == name && !p.is_static) {
            let decl = if p.declaring_class.is_empty() { cur.clone() } else { p.declaring_class.clone() };
            let decl_name = vm.classes.get(&decl).map(|c| c.name.clone()).unwrap_or_else(|| ce.name.clone());
            return Some((p.visibility, decl_name));
        }
        cur = lower(ce.parent.as_ref()?);
    }
    None
}

fn mangle(vm: &Vm, class_lower: &[u8], name: &[u8]) -> Vec<u8> {
    if name.contains(&0) {
        return name.to_vec();
    }
    match find_prop_decl(vm, class_lower, name) {
        Some((Visibility::Private, decl)) => {
            let mut v = vec![0u8];
            v.extend_from_slice(&decl);
            v.push(0);
            v.extend_from_slice(name);
            v
        }
        Some((Visibility::Protected, _)) => {
            let mut v = b"\0*\0".to_vec();
            v.extend_from_slice(name);
            v
        }
        _ => name.to_vec(),
    }
}

fn unmangle(name: &[u8]) -> Vec<u8> {
    if name.first() == Some(&0) {
        if let Some(p) = name[1..].iter().position(|&c| c == 0) {
            return name[p + 2..].to_vec();
        }
    }
    name.to_vec()
}

#[derive(Default)]
pub struct SerializeState {
    n: i64,
    seen: HashMap<usize, i64>,
    /// Arrays currently being serialized (recursion protection).
    active: HashSet<usize>,
}

impl SerializeState {
    pub fn new() -> Self {
        Self::default()
    }
}

fn push_str(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(format!("s:{}:\"", s.len()).as_bytes());
    out.extend_from_slice(s);
    out.extend_from_slice(b"\";");
}

fn push_key(out: &mut Vec<u8>, k: &ArrayKey) {
    match k {
        ArrayKey::Int(n) => out.extend_from_slice(format!("i:{};", n).as_bytes()),
        ArrayKey::String(s) => push_str(out, s.as_bytes()),
    }
}

fn format_double(f: f64) -> String {
    if f.is_nan() {
        return "NAN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "INF".into() } else { "-INF".into() };
    }
    let sp = goro_core::value::get_php_serialize_precision();
    if sp < 0 {
        goro_core::value::format_php_float_repr(f)
    } else {
        goro_core::value::format_php_float_with_precision_pub(f, sp as usize)
    }
}

/// php_add_var_hash: returns the back-reference index if `val` was seen.
fn add_var_hash(st: &mut SerializeState, val: &Value) -> i64 {
    st.n += 1;
    let key = match val {
        Value::Reference(r) => match &*r.borrow() {
            Value::Object(o) => Rc::as_ptr(o) as *const u8 as usize,
            _ => Rc::as_ptr(r) as *const u8 as usize,
        },
        Value::Object(o) => Rc::as_ptr(o) as *const u8 as usize,
        _ => return 0,
    };
    if let Some(&idx) = st.seen.get(&key) {
        if matches!(val, Value::Reference(_)) && idx != -1 {
            st.n -= 1;
        }
        return idx;
    }
    st.seen.insert(key, st.n);
    0
}

/// php_var_serialize_intern. `fname` is the active function for warnings.
pub fn serialize(vm: &mut Vm, val: &Value, st: &mut SerializeState, out: &mut Vec<u8>) {
    if vm.current_exception.is_some() {
        return;
    }
    let already = add_var_hash(st, val);
    if already != 0 {
        if already == -1 {
            out.extend_from_slice(b"N;");
        } else if matches!(val, Value::Reference(_)) {
            out.extend_from_slice(format!("R:{};", already).as_bytes());
        } else {
            out.extend_from_slice(format!("r:{};", already).as_bytes());
        }
        return;
    }
    let inner = match val {
        Value::Reference(r) => r.borrow().clone(),
        v => v.clone(),
    };
    match &inner {
        Value::Null | Value::Undef => out.extend_from_slice(b"N;"),
        Value::True => out.extend_from_slice(b"b:1;"),
        Value::False => out.extend_from_slice(b"b:0;"),
        Value::Long(n) => out.extend_from_slice(format!("i:{};", n).as_bytes()),
        Value::Double(f) => out.extend_from_slice(format!("d:{};", format_double(*f)).as_bytes()),
        Value::String(s) => push_str(out, s.as_bytes()),
        Value::Resource(_) | Value::Reference(_) => out.extend_from_slice(b"i:0;"),
        Value::Generator(_) => {
            let exc = vm.create_exception(b"Exception", "Serialization of 'Generator' is not allowed", vm.current_line);
            vm.current_exception = Some(exc);
        }
        Value::Array(arr) => {
            let ptr = Rc::as_ptr(arr) as *const u8 as usize;
            st.active.insert(ptr);
            let entries: Vec<(ArrayKey, Value)> = arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            out.extend_from_slice(format!("a:{}:{{", entries.len()).as_bytes());
            serialize_nested(vm, &entries, st, out);
            out.push(b'}');
            st.active.remove(&ptr);
        }
        Value::Object(obj) => serialize_object(vm, &inner, obj, st, out),
    }
}

fn serialize_nested(vm: &mut Vm, entries: &[(ArrayKey, Value)], st: &mut SerializeState, out: &mut Vec<u8>) {
    for (k, v) in entries {
        push_key(out, k);
        let inner_arr = match v {
            Value::Array(a) => Some(a.clone()),
            _ => None,
        };
        if let Some(a) = inner_arr {
            if st.active.contains(&(Rc::as_ptr(&a) as *const u8 as usize)) {
                st.n += 1;
                out.extend_from_slice(b"N;");
                continue;
            }
        }
        serialize(vm, v, st, out);
        if vm.current_exception.is_some() {
            return;
        }
    }
}

fn class_header(out: &mut Vm, name: &[u8], buf: &mut Vec<u8>) {
    let _ = out;
    buf.extend_from_slice(format!("O:{}:\"", name.len()).as_bytes());
    buf.extend_from_slice(name);
    buf.extend_from_slice(b"\":");
}

fn serialize_object(vm: &mut Vm, val: &Value, obj: &Rc<RefCell<PhpObject>>, st: &mut SerializeState, out: &mut Vec<u8>) {
    let class_name = obj.borrow().class_name.clone();
    let cl = lower(&class_name);
    if Vm::is_enum_case(val) {
        let case = obj.borrow().get_property(b"name").to_php_string();
        let disp = goro_core::value::display_class_name(&class_name);
        let full = format!("{}:{}", disp, case.to_string_lossy());
        out.extend_from_slice(format!("E:{}:\"{}\";", full.len(), full).as_bytes());
        return;
    }
    if !is_user_class(vm, &cl) {
        // Internal class: let the standard serializer handle it.
        if let Some(f) = vm.functions.get(b"serialize".as_slice()).copied() {
            if let Ok(Value::String(s)) = f(vm, std::slice::from_ref(val)) {
                if vm.current_exception.is_none() {
                    out.extend_from_slice(s.as_bytes());
                }
            }
        }
        return;
    }
    let display = if cl == b"__php_incomplete_class" {
        obj.borrow().get_property(b"__PHP_Incomplete_Class_Name").to_php_string().as_bytes().to_vec()
    } else {
        goro_core::value::display_class_name(&class_name).into_bytes()
    };
    let has = |vm: &Vm, m: &[u8]| vm.classes.get(&cl).map(|c| c.get_method(m).is_some()).unwrap_or(false);
    if has(vm, b"__serialize") {
        let r = vm.call_object_method(val, b"__serialize", &[]);
        if vm.current_exception.is_some() {
            return;
        }
        match r {
            Some(Value::Array(a)) => {
                let entries: Vec<(ArrayKey, Value)> = a.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                class_header(vm, &display, out);
                out.extend_from_slice(format!("{}:{{", entries.len()).as_bytes());
                serialize_nested(vm, &entries, st, out);
                out.push(b'}');
            }
            _ => {
                let msg = format!("{}::__serialize() must return an array", String::from_utf8_lossy(&display));
                let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
            }
        }
        return;
    }
    if vm.class_implements_interface(&cl, b"serializable") && has(vm, b"serialize") {
        let r = vm.call_object_method(val, b"serialize", &[]);
        if vm.current_exception.is_some() {
            return;
        }
        match r {
            Some(Value::String(s)) => {
                out.extend_from_slice(format!("C:{}:\"", display.len()).as_bytes());
                out.extend_from_slice(&display);
                out.extend_from_slice(format!("\":{}:{{", s.len()).as_bytes());
                out.extend_from_slice(s.as_bytes());
                out.push(b'}');
            }
            _ => out.extend_from_slice(b"N;"),
        }
        return;
    }
    if cl != b"__php_incomplete_class" && has(vm, b"__sleep") {
        let r = vm.call_object_method(val, b"__sleep", &[]);
        if vm.current_exception.is_some() {
            return;
        }
        let names: Vec<Value> = match r {
            Some(Value::Array(a)) => a.borrow().values().cloned().collect(),
            _ => {
                vm.emit_warning("serialize(): __sleep should return an array only containing the names of instance-variables to serialize");
                out.extend_from_slice(b"N;");
                return;
            }
        };
        let mut entries: Vec<(ArrayKey, Value)> = Vec::new();
        for n in names {
            let name = n.to_php_string().as_bytes().to_vec();
            let found = obj.borrow().properties.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone());
            match found {
                Some(v) => {
                    let key = mangle(vm, &cl, &name);
                    entries.push((ArrayKey::String(PhpString::from_vec(key)), v));
                }
                None => {
                    vm.emit_warning(&format!(
                        "serialize(): \"{}\" returned as member variable from __sleep() but does not exist",
                        String::from_utf8_lossy(&name)
                    ));
                }
            }
        }
        class_header(vm, &display, out);
        out.extend_from_slice(format!("{}:{{", entries.len()).as_bytes());
        serialize_nested(vm, &entries, st, out);
        out.push(b'}');
        return;
    }
    let props: Vec<(Vec<u8>, Value)> = obj
        .borrow()
        .properties
        .iter()
        .filter(|(k, v)| !is_internal_property(k) && !matches!(v, Value::Undef) && k.as_slice() != b"__PHP_Incomplete_Class_Name")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let entries: Vec<(ArrayKey, Value)> = props
        .into_iter()
        .map(|(k, v)| (ArrayKey::String(PhpString::from_vec(mangle(vm, &cl, &k))), v))
        .collect();
    class_header(vm, &display, out);
    out.extend_from_slice(format!("{}:{{", entries.len()).as_bytes());
    serialize_nested(vm, &entries, st, out);
    out.push(b'}');
}

// ---------------------------------------------------------------------------
// Unserialization
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum Loc {
    Top(usize),
    Arr(Rc<RefCell<PhpArray>>, ArrayKey),
    Prop(Rc<RefCell<PhpObject>>, Vec<u8>),
}

pub struct UnserializeState {
    slots: Vec<(Loc, Value)>,
    /// Top-level values (session variables), addressed by `Loc::Top`.
    pub tops: Vec<Value>,
    /// Delayed __wakeup / __unserialize calls.
    delayed: Vec<(Value, Option<Value>)>,
}

impl UnserializeState {
    pub fn new() -> Self {
        UnserializeState { slots: Vec::new(), tops: Vec::new(), delayed: Vec::new() }
    }

    fn read_loc(&self, loc: &Loc) -> Option<Value> {
        match loc {
            Loc::Top(i) => self.tops.get(*i).cloned(),
            Loc::Arr(a, k) => a.borrow().get(k).cloned(),
            Loc::Prop(o, n) => o.borrow().properties.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()),
        }
    }

    fn write_loc(&mut self, loc: &Loc, v: Value) {
        match loc {
            Loc::Top(i) => {
                if let Some(s) = self.tops.get_mut(*i) {
                    *s = v;
                }
            }
            Loc::Arr(a, k) => a.borrow_mut().set(k.clone(), v),
            Loc::Prop(o, n) => {
                // Rebind the slot itself (not through an existing reference).
                let mut ob = o.borrow_mut();
                match ob.properties.iter_mut().find(|(k, _)| k == n) {
                    Some((_, slot)) => *slot = v,
                    None => ob.properties.push((n.clone(), v)),
                }
            }
        }
    }

    /// Store the parsed value `v` at `loc`, writing through a reference
    /// that a nested `R:` may have created at that location.
    fn place(&mut self, loc: &Loc, v: Value) {
        if let Some(Value::Reference(r)) = self.read_loc(loc) {
            if !matches!(v, Value::Reference(_)) {
                *r.borrow_mut() = v;
                return;
            }
        }
        self.write_loc(loc, v);
    }

    /// Run the delayed __unserialize / __wakeup calls (var_destroy).
    pub fn finish(&mut self, vm: &mut Vm) {
        let delayed = std::mem::take(&mut self.delayed);
        for (obj, data) in delayed {
            if vm.current_exception.is_some() {
                break;
            }
            match data {
                Some(d) => {
                    vm.call_object_method(&obj, b"__unserialize", &[d]);
                }
                None => {
                    vm.call_object_method(&obj, b"__wakeup", &[]);
                }
            }
        }
    }
}

struct P<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> P<'a> {
    fn peek(&self) -> Option<u8> {
        self.d.get(self.pos).copied()
    }
    fn expect(&mut self, c: u8) -> Option<()> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Some(())
        } else {
            None
        }
    }
    /// uiv = [0-9]+
    fn uiv(&mut self) -> Option<usize> {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if start == self.pos {
            return None;
        }
        let s = std::str::from_utf8(&self.d[start..self.pos]).ok()?;
        s.parse::<usize>().ok()
    }
    /// iv = [+-]? [0-9]+
    fn iv(&mut self) -> Option<i64> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+') | Some(b'-')) {
            self.pos += 1;
        }
        let ds = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if ds == self.pos {
            return None;
        }
        let s = std::str::from_utf8(&self.d[start..self.pos]).ok()?;
        Some(match s.parse::<i64>() {
            Ok(n) => n,
            Err(_) => {
                // Overflow: PHP's parse_iv saturates with a warning; keep it simple.
                if s.starts_with('-') { i64::MIN } else { i64::MAX }
            }
        })
    }
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.pos + n > self.d.len() {
            return None;
        }
        let r = &self.d[self.pos..self.pos + n];
        self.pos += n;
        Some(r)
    }
}

/// Skip one serialized value syntactically; returns false when malformed.
fn skip_value(p: &mut P) -> Option<()> {
    let c = p.peek()?;
    p.pos += 1;
    match c {
        b'N' => p.expect(b';'),
        b'b' | b'i' | b'r' | b'R' => {
            p.expect(b':')?;
            if c == b'i' { p.iv()?; } else { p.uiv()?; }
            p.expect(b';')
        }
        b'd' => {
            p.expect(b':')?;
            while p.peek()? != b';' {
                p.pos += 1;
            }
            p.expect(b';')
        }
        b's' | b'S' | b'E' => {
            p.expect(b':')?;
            let n = p.uiv()?;
            p.expect(b':')?;
            p.expect(b'"')?;
            p.bytes(n)?;
            p.expect(b'"')?;
            p.expect(b';')
        }
        b'a' => {
            p.expect(b':')?;
            let n = p.uiv()?;
            p.expect(b':')?;
            p.expect(b'{')?;
            for _ in 0..n {
                skip_value(p)?;
                skip_value(p)?;
            }
            p.expect(b'}')
        }
        b'O' | b'C' => {
            p.expect(b':')?;
            let n = p.uiv()?;
            p.expect(b':')?;
            p.expect(b'"')?;
            p.bytes(n)?;
            p.expect(b'"')?;
            p.expect(b':')?;
            let cnt = p.uiv()?;
            p.expect(b':')?;
            p.expect(b'{')?;
            if c == b'C' {
                p.bytes(cnt)?;
            } else {
                for _ in 0..cnt {
                    skip_value(p)?;
                    skip_value(p)?;
                }
            }
            p.expect(b'}')
        }
        _ => None,
    }
}

fn valid_class_name(name: &[u8]) -> bool {
    !name.is_empty()
        && name.iter().all(|&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'\\' || c >= 0x80)
}

/// Unserialize one value from `data[*pos..]` into `loc`. Returns false on
/// malformed input (the session decoders then fail).
pub fn unserialize_into(vm: &mut Vm, data: &[u8], pos: &mut usize, st: &mut UnserializeState, top: usize) -> bool {
    let mut p = P { d: data, pos: *pos };
    while st.tops.len() <= top {
        st.tops.push(Value::Null);
    }
    let ok = unser(vm, &mut p, st, Loc::Top(top), 0).is_some();
    *pos = p.pos;
    ok
}

fn unser(vm: &mut Vm, p: &mut P, st: &mut UnserializeState, loc: Loc, depth: usize) -> Option<()> {
    if depth > 4096 || p.pos >= p.d.len() {
        return None;
    }
    let c = p.peek()?;
    if c != b'R' {
        st.slots.push((loc.clone(), Value::Null));
    }
    let slot_idx = st.slots.len().wrapping_sub(1);
    let start = p.pos;
    p.pos += 1;
    let v = match c {
        b'N' => {
            p.expect(b';')?;
            Value::Null
        }
        b'b' => {
            p.expect(b':')?;
            let v = match p.peek()? {
                b'0' => Value::False,
                b'1' => Value::True,
                _ => return None,
            };
            p.pos += 1;
            p.expect(b';')?;
            v
        }
        b'i' => {
            p.expect(b':')?;
            let n = p.iv()?;
            p.expect(b';')?;
            Value::Long(n)
        }
        b'd' => {
            p.expect(b':')?;
            let s0 = p.pos;
            while p.peek()? != b';' {
                p.pos += 1;
            }
            let s = std::str::from_utf8(&p.d[s0..p.pos]).ok()?;
            p.expect(b';')?;
            let f = match s {
                "INF" => f64::INFINITY,
                "-INF" => f64::NEG_INFINITY,
                "NAN" => f64::NAN,
                _ => {
                    let ok = !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'));
                    if !ok {
                        return None;
                    }
                    s.parse::<f64>().ok()?
                }
            };
            Value::Double(f)
        }
        b's' => {
            p.expect(b':')?;
            let n = p.uiv()?;
            p.expect(b':')?;
            p.expect(b'"')?;
            let b = p.bytes(n)?;
            p.expect(b'"')?;
            p.expect(b';')?;
            Value::String(PhpString::from_vec(b.to_vec()))
        }
        b'S' => {
            p.expect(b':')?;
            let n = p.uiv()?;
            p.expect(b':')?;
            p.expect(b'"')?;
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                let ch = p.peek()?;
                p.pos += 1;
                if ch == b'\\' {
                    let h = p.bytes(2)?;
                    let s = std::str::from_utf8(h).ok()?;
                    out.push(u8::from_str_radix(s, 16).ok()?);
                } else {
                    out.push(ch);
                }
            }
            p.expect(b'"')?;
            p.expect(b';')?;
            Value::String(PhpString::from_vec(out))
        }
        b'R' => {
            p.expect(b':')?;
            let id = p.uiv()?;
            p.expect(b';')?;
            if id == 0 || id > st.slots.len() {
                return None;
            }
            let (sloc, sval) = st.slots[id - 1].clone();
            let cur = st.read_loc(&sloc).unwrap_or(sval);
            let r = match cur {
                Value::Reference(r) => r,
                other => {
                    let r = Rc::new(RefCell::new(other));
                    st.write_loc(&sloc, Value::Reference(r.clone()));
                    st.slots[id - 1].1 = Value::Reference(r.clone());
                    r
                }
            };
            let v = Value::Reference(r);
            st.write_loc(&loc, v);
            return Some(());
        }
        b'r' => {
            p.expect(b':')?;
            let id = p.uiv()?;
            p.expect(b';')?;
            if id == 0 || id > st.slots.len() || id - 1 == slot_idx {
                return None;
            }
            let (sloc, sval) = st.slots[id - 1].clone();
            let cur = st.read_loc(&sloc).unwrap_or(sval).deref();
            if !matches!(cur, Value::Object(_)) {
                return None;
            }
            cur
        }
        b'a' => {
            p.expect(b':')?;
            let n = p.uiv()?;
            p.expect(b':')?;
            p.expect(b'{')?;
            let arr = Rc::new(RefCell::new(PhpArray::new()));
            let av = Value::Array(arr.clone());
            st.slots[slot_idx].1 = av.clone();
            st.place(&loc, av.clone());
            for _ in 0..n {
                let key = parse_key(p)?;
                arr.borrow_mut().set(key.clone(), Value::Null);
                unser(vm, p, st, Loc::Arr(arr.clone(), key), depth + 1)?;
            }
            p.expect(b'}')?;
            return Some(());
        }
        b'E' => {
            p.expect(b':')?;
            let n = p.uiv()?;
            p.expect(b':')?;
            p.expect(b'"')?;
            p.bytes(n)?;
            p.expect(b'"')?;
            p.expect(b';')?;
            delegate(vm, &p.d[start..p.pos])?
        }
        b'O' | b'C' => {
            p.expect(b':')?;
            let n = p.uiv()?;
            p.expect(b':')?;
            p.expect(b'"')?;
            let name = p.bytes(n)?.to_vec();
            p.expect(b'"')?;
            p.expect(b':')?;
            if !valid_class_name(&name) {
                return None;
            }
            let mut cl = lower(&name);
            if cl.first() == Some(&b'\\') {
                cl.remove(0);
            }
            if !vm.classes.contains_key(&cl) && !vm.is_known_builtin_class(&cl) {
                let was_fatal = vm.fatal_emitted;
                vm.try_autoload_class(&name);
                if vm.fatal_emitted && !was_fatal {
                    // A fatal error inside the autoloader: bailout.
                    BAILOUT.with(|b| b.set(true));
                    return None;
                }
            }
            let known = vm.classes.contains_key(&cl) || vm.is_known_builtin_class(&cl);
            if known && !is_user_class(vm, &cl) {
                // Internal class: find the extent and delegate.
                let mut q = P { d: p.d, pos: start };
                skip_value(&mut q)?;
                p.pos = q.pos;
                let v = delegate(vm, &p.d[start..p.pos])?;
                st.slots[slot_idx].1 = v.clone();
                st.place(&loc, v);
                return Some(());
            }
            let count = p.uiv()?;
            p.expect(b':')?;
            p.expect(b'{')?;
            let obj = new_object(vm, &name, &cl, known);
            let ov = Value::Object(obj.clone());
            st.slots[slot_idx].1 = ov.clone();
            st.place(&loc, ov.clone());
            if c == b'C' {
                let payload = p.bytes(count)?.to_vec();
                p.expect(b'}')?;
                if known && vm.classes.get(&cl).map(|c| c.get_method(b"unserialize").is_some()).unwrap_or(false) {
                    vm.call_object_method(&ov, b"unserialize", &[Value::String(PhpString::from_vec(payload))]);
                    if vm.current_exception.is_some() {
                        return None;
                    }
                }
                return Some(());
            }
            let has = |vm: &Vm, m: &[u8]| known && vm.classes.get(&cl).map(|c| c.get_method(m).is_some()).unwrap_or(false);
            if has(vm, b"__unserialize") {
                let tmp = Rc::new(RefCell::new(PhpArray::new()));
                for _ in 0..count {
                    let key = parse_key(p)?;
                    tmp.borrow_mut().set(key.clone(), Value::Null);
                    unser(vm, p, st, Loc::Arr(tmp.clone(), key), depth + 1)?;
                }
                p.expect(b'}')?;
                st.delayed.push((ov, Some(Value::Array(tmp))));
                return Some(());
            }
            for _ in 0..count {
                let key = parse_key(p)?;
                let pname = match &key {
                    ArrayKey::String(s) => unmangle(s.as_bytes()),
                    ArrayKey::Int(n) => n.to_string().into_bytes(),
                };
                if !obj.borrow().properties.iter().any(|(k, _)| *k == pname) {
                    obj.borrow_mut().set_property(pname.clone(), Value::Null);
                }
                unser(vm, p, st, Loc::Prop(obj.clone(), pname), depth + 1)?;
            }
            p.expect(b'}')?;
            if has(vm, b"__wakeup") {
                st.delayed.push((ov, None));
            }
            return Some(());
        }
        _ => return None,
    };
    if slot_idx < st.slots.len() && c != b'R' {
        st.slots[slot_idx].1 = v.clone();
    }
    st.place(&loc, v);
    Some(())
}

fn parse_key(p: &mut P) -> Option<ArrayKey> {
    match p.peek()? {
        b'i' => {
            p.pos += 1;
            p.expect(b':')?;
            let n = p.iv()?;
            p.expect(b';')?;
            Some(ArrayKey::Int(n))
        }
        b's' => {
            p.pos += 1;
            p.expect(b':')?;
            let n = p.uiv()?;
            p.expect(b':')?;
            p.expect(b'"')?;
            let b = p.bytes(n)?.to_vec();
            p.expect(b'"')?;
            p.expect(b';')?;
            Some(Vm::value_to_array_key(Value::String(PhpString::from_vec(b))))
        }
        _ => None,
    }
}

fn delegate(vm: &mut Vm, s: &[u8]) -> Option<Value> {
    let f = vm.functions.get(b"unserialize".as_slice()).copied()?;
    let r = f(vm, &[Value::String(PhpString::from_vec(s.to_vec()))]).ok()?;
    if matches!(r, Value::False) && s != b"b:0;" {
        return None;
    }
    Some(r)
}

fn new_object(vm: &mut Vm, name: &[u8], cl: &[u8], known: bool) -> Rc<RefCell<PhpObject>> {
    let id = vm.alloc_object_id();
    if !known {
        let mut obj = PhpObject::new(b"__PHP_Incomplete_Class".to_vec(), id);
        obj.set_property(b"__PHP_Incomplete_Class_Name".to_vec(), Value::String(PhpString::from_vec(name.to_vec())));
        return Rc::new(RefCell::new(obj));
    }
    let canonical = match vm.classes.get(cl) {
        Some(ce) => ce.name.clone(),
        None if cl == b"stdclass" => b"stdClass".to_vec(),
        None => name.to_vec(),
    };
    let mut obj = PhpObject::new(canonical, id);
    if vm.classes.contains_key(cl) {
        vm.apply_class_property_defaults(&mut obj, cl);
    }
    let rc = Rc::new(RefCell::new(obj));
    let has_destruct = vm.classes.get(cl).map(|c| c.get_method(b"__destruct").is_some()).unwrap_or(false);
    if has_destruct {
        vm.register_destructible(Value::Object(rc.clone()));
    }
    rc
}
