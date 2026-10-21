//! serialize() / unserialize() — a port of php-src `ext/standard/var.c`
//! (php_var_serialize_*) and `ext/standard/var_unserializer.re`.
//!
//! Notable semantics mirrored from PHP:
//! * back-references (`R:` for PHP references, `r:` for repeated objects),
//!   numbered by the order in which values are visited;
//! * `__serialize`/`__unserialize`, `Serializable` (`C:`), `__sleep`/`__wakeup`;
//! * delayed `__wakeup`/`__unserialize` calls (run once the whole payload has
//!   been parsed), shared state between nested unserialize() calls made from
//!   `Serializable::unserialize()`, and the `serialize_lock` that isolates
//!   nested calls made from magic methods;
//! * exact error offsets, `allowed_classes`, `max_depth`.

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{PhpObject, Visibility};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    /// BG(serialize_lock): non-zero while a magic method invoked by the
    /// (un)serializer runs, so nested calls get a fresh context.
    static SERIALIZE_LOCK: Cell<u32> = const { Cell::new(0) };
    static SER_LEVEL: Cell<u32> = const { Cell::new(0) };
    static SER_DATA: RefCell<Option<Rc<RefCell<SerHash>>>> = const { RefCell::new(None) };
    static UNSER_LEVEL: Cell<u32> = const { Cell::new(0) };
    static UNSER_DATA: RefCell<Option<Rc<RefCell<UnserData>>>> = const { RefCell::new(None) };
}

fn with_lock<R>(f: impl FnOnce() -> R) -> R {
    SERIALIZE_LOCK.with(|l| l.set(l.get() + 1));
    let r = f();
    SERIALIZE_LOCK.with(|l| l.set(l.get() - 1));
    r
}

fn locked() -> bool {
    SERIALIZE_LOCK.with(|l| l.get()) > 0
}

const INCOMPLETE_CLASS: &[u8] = b"__PHP_Incomplete_Class";
const MAGIC_MEMBER: &[u8] = b"__PHP_Incomplete_Class_Name";

fn lower(b: &[u8]) -> Vec<u8> {
    b.iter().map(|c| c.to_ascii_lowercase()).collect()
}

fn is_closure_name(b: &[u8]) -> bool {
    b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_")
}

/// Internal bookkeeping properties that must never show up in serialized data.
pub(crate) fn is_internal_property(name: &[u8]) -> bool {
    crate::output::is_internal_property(name)
}

/// zend_is_valid_class_name()
fn is_valid_class_name(name: &[u8]) -> bool {
    name.iter().all(|&c| c >= 0x80 || c.is_ascii_alphanumeric() || c == b'_' || c == b'\\')
}

/// Classes flagged ZEND_ACC_NOT_SERIALIZABLE (inherited by subclasses).
fn not_serializable_builtin(cl: &[u8]) -> bool {
    matches!(cl,
        b"closure" | b"generator" | b"fiber" | b"weakreference" | b"weakmap"
        | b"xmlparser" | b"internaliterator" | b"random\\engine\\secure"
        | b"simplexmlelement" | b"splfileinfo" | b"directoryiterator" | b"filesystemiterator"
        | b"recursivedirectoryiterator" | b"globiterator" | b"splfileobject" | b"spltempfileobject"
        | b"sensitiveparametervalue" | b"socket" | b"addressinfo" | b"directory" | b"finfo"
        | b"inflatecontext" | b"deflatecontext" | b"curlfile" | b"curlhandle" | b"curlmultihandle"
        | b"curlsharehandle" | b"curlsharepersistenthandle"
        | b"opensslcertificate" | b"opensslcertificatesigningrequest" | b"opensslasymmetrickey"
        | b"domxpath" | b"hashcontext_never"
    ) || (cl.starts_with(b"reflection") && cl != b"reflectionexception")
}

/// Walk the class hierarchy (user and builtin) of `class_lower`, including itself.
fn class_chain(vm: &Vm, class_lower: &[u8]) -> Vec<Vec<u8>> {
    let mut out = vec![class_lower.to_vec()];
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        let next: Option<Vec<u8>> = vm.classes.get(&cur)
            .and_then(|c| c.parent.as_ref().map(|p| lower(p)))
            .or_else(|| goro_core::vm::get_builtin_parent(&cur).map(lower));
        match next {
            Some(p) if !out.contains(&p) => {
                out.push(p.clone());
                cur = p;
            }
            _ => break,
        }
    }
    out
}

fn is_not_serializable(vm: &Vm, class_name: &[u8]) -> bool {
    if class_name.windows(10).any(|w| w == b"@anonymous") {
        return true;
    }
    class_chain(vm, &lower(class_name)).iter().any(|c| not_serializable_builtin(c))
        || goro_core::stubs::class_is_not_serializable(vm, &lower(class_name))
}

/// Builtin classes that provide native __serialize/__unserialize handlers.
fn builtin_has_magic_serialize(cl: &[u8]) -> bool {
    matches!(cl,
        b"spldoublylinkedlist" | b"splstack" | b"splqueue"
        | b"splfixedarray" | b"splobjectstorage"
        | b"splheap" | b"splminheap" | b"splmaxheap"
        | b"splpriorityqueue"
        | b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator"
        | b"random\\engine\\mt19937"
        | b"random\\engine\\xoshiro256starstar"
        | b"random\\engine\\pcgoneseq128xslrr64"
        | b"random\\randomizer"
        | b"datetime" | b"datetimeimmutable" | b"datetimezone" | b"dateinterval" | b"dateperiod"
        | b"gmp"
    )
}

fn has_user_method(vm: &Vm, class_lower: &[u8], m: &[u8]) -> bool {
    vm.classes.get(class_lower).map(|c| c.get_method(m).is_some()).unwrap_or(false)
}

fn has_magic_serialize(vm: &Vm, class_lower: &[u8]) -> bool {
    has_user_method(vm, class_lower, b"__serialize")
        || class_chain(vm, class_lower).iter().any(|c| builtin_has_magic_serialize(c))
}

fn has_magic_unserialize(vm: &Vm, class_lower: &[u8]) -> bool {
    has_user_method(vm, class_lower, b"__unserialize")
        || class_chain(vm, class_lower).iter().any(|c| builtin_has_magic_serialize(c))
}

/// Whether the class uses the Serializable interface (ce->serialize != NULL).
fn is_serializable_iface(vm: &Vm, class_lower: &[u8]) -> bool {
    vm.class_implements_interface(class_lower, b"serializable")
        && has_user_method(vm, class_lower, b"serialize")
}

fn canonical_class_name(vm: &Vm, class_lower: &[u8], fallback: &[u8]) -> Vec<u8> {
    if let Some(ce) = vm.classes.get(class_lower) {
        return ce.name.clone();
    }
    if class_lower == b"stdclass" {
        return b"stdClass".to_vec();
    }
    let builtin = vm.builtin_canonical_name(class_lower).into_bytes();
    if builtin.as_slice() != class_lower {
        return builtin;
    }
    // No canonical spelling known: keep the spelling we were given unless it
    // is all-lowercase and a better one exists in the static tables.
    fallback.to_vec()
}

/// Name of the builtin throwable base (Exception/Error) for a class, if any.
fn throwable_base(vm: &Vm, class_lower: &[u8]) -> Option<&'static [u8]> {
    for c in class_chain(vm, class_lower) {
        if c == b"exception" {
            return Some(b"Exception");
        }
        if c == b"error" {
            return Some(b"Error");
        }
        if !vm.classes.contains_key(&c) {
            if goro_core::vm::is_builtin_subclass(&c, b"exception") {
                return Some(b"Exception");
            }
            if goro_core::vm::is_builtin_subclass(&c, b"error") {
                return Some(b"Error");
            }
        }
    }
    None
}

fn mangle(cls: &[u8], name: &[u8]) -> Vec<u8> {
    let mut k = Vec::with_capacity(cls.len() + name.len() + 2);
    k.push(0);
    k.extend_from_slice(cls);
    k.push(0);
    k.extend_from_slice(name);
    k
}

/// Split a mangled property name into (class, name). Unmangled → (None, name).
pub(crate) fn unmangle(key: &[u8]) -> Option<(Option<&[u8]>, &[u8])> {
    if key.first() != Some(&0) {
        return Some((None, key));
    }
    let rest = &key[1..];
    let sep = rest.iter().position(|&b| b == 0)?;
    Some((Some(&rest[..sep]), &rest[sep + 1..]))
}

/// One entry of an object's property table, keyed the way PHP's
/// properties HashTable is keyed (with private/protected mangling).
pub(crate) struct PropEntry {
    pub(crate) php_key: Vec<u8>,
    pub(crate) storage_key: Vec<u8>,
    pub(crate) value: Value,
}

/// Build the mangled property table of an object (zend_std_get_properties).
pub(crate) fn mangled_props(vm: &Vm, obj: &PhpObject) -> Vec<PropEntry> {
    let class_lower = lower(&obj.class_name);
    let ce = vm.classes.get(&class_lower);
    let exc_base = throwable_base(vm, &class_lower);
    let mut out: Vec<PropEntry> = Vec::new();
    if let Some(base) = exc_base {
        // Builtin Exception/Error properties, in declaration order.
        let order: [(&[u8], bool); 7] = [
            (b"message", false), (b"string", true), (b"code", false), (b"file", false),
            (b"line", false), (b"trace", true), (b"previous", true),
        ];
        for (name, private) in order {
            let user_override = ce.map(|c| c.properties.iter().any(|p| p.name == name && !p.is_static
                && lower(&p.declaring_class) != b"exception" && lower(&p.declaring_class) != b"error"))
                .unwrap_or(false);
            if user_override {
                continue;
            }
            let value = if obj.has_property(name) {
                obj.get_property(name)
            } else if name == b"string" {
                Value::String(PhpString::empty())
            } else if name == b"trace" {
                Value::Array(Rc::new(RefCell::new(PhpArray::new())))
            } else {
                continue;
            };
            let php_key = if private { mangle(base, name) } else { mangle(b"*", name) };
            out.push(PropEntry { php_key, storage_key: name.to_vec(), value });
        }
    }
    for (key, value) in obj.properties.iter() {
        if is_internal_property(key) {
            continue;
        }
        if exc_base.is_some() && out.iter().any(|e| e.storage_key == *key) {
            continue;
        }
        if key.first() == Some(&0) {
            out.push(PropEntry { php_key: key.clone(), storage_key: key.clone(), value: value.clone() });
            continue;
        }
        let php_key = match ce {
            Some(c) => {
                let defs: Vec<_> = c.properties.iter().filter(|p| p.name == *key && !p.is_static).collect();
                let def = if defs.len() > 1 {
                    defs.iter().find(|p| p.visibility != Visibility::Private).copied()
                } else {
                    defs.first().copied()
                };
                match def {
                    Some(p) if p.visibility == Visibility::Private => {
                        let decl = canonical_class_name(vm, &lower(&p.declaring_class), &p.declaring_class);
                        mangle(&decl, key)
                    }
                    Some(p) if p.visibility == Visibility::Protected => mangle(b"*", key),
                    _ => key.clone(),
                }
            }
            None => key.clone(),
        };
        out.push(PropEntry { php_key, storage_key: key.clone(), value: value.clone() });
    }
    out
}

// ---------------------------------------------------------------------------
// serialize()
// ---------------------------------------------------------------------------

struct SerHash {
    n: i64,
    map: HashMap<usize, i64>,
    /// Keeps visited values alive so their addresses are not reused.
    keep: Vec<Value>,
}

fn ser_init() -> Rc<RefCell<SerHash>> {
    let level = SER_LEVEL.with(|l| l.get());
    if locked() || level == 0 {
        let d = Rc::new(RefCell::new(SerHash { n: 0, map: HashMap::new(), keep: Vec::new() }));
        if !locked() {
            SER_DATA.with(|s| *s.borrow_mut() = Some(d.clone()));
            SER_LEVEL.with(|l| l.set(1));
        }
        d
    } else {
        SER_LEVEL.with(|l| l.set(level + 1));
        SER_DATA.with(|s| s.borrow().clone()).expect("serialize data")
    }
}

fn ser_destroy() {
    if !locked() {
        let level = SER_LEVEL.with(|l| {
            let v = l.get().saturating_sub(1);
            l.set(v);
            v
        });
        if level == 0 {
            SER_DATA.with(|s| *s.borrow_mut() = None);
        }
    }
}

struct Serializer {
    buf: Vec<u8>,
    hash: Rc<RefCell<SerHash>>,
    depth: usize,
    line: u32,
}

fn push_long(buf: &mut Vec<u8>, n: i64) {
    buf.extend_from_slice(b"i:");
    buf.extend_from_slice(n.to_string().as_bytes());
    buf.push(b';');
}

fn push_str(buf: &mut Vec<u8>, s: &[u8]) {
    buf.extend_from_slice(b"s:");
    buf.extend_from_slice(s.len().to_string().as_bytes());
    buf.extend_from_slice(b":\"");
    buf.extend_from_slice(s);
    buf.extend_from_slice(b"\";");
}

fn push_key(buf: &mut Vec<u8>, k: &ArrayKey) {
    match k {
        ArrayKey::Int(n) => push_long(buf, *n),
        ArrayKey::String(s) => push_str(buf, s.as_bytes()),
    }
}

fn push_class_name(buf: &mut Vec<u8>, name: &[u8]) {
    buf.extend_from_slice(b"O:");
    buf.extend_from_slice(name.len().to_string().as_bytes());
    buf.extend_from_slice(b":\"");
    buf.extend_from_slice(name);
    buf.extend_from_slice(b"\":");
}

/// Format a double like zend_gcvt(value, serialize_precision, '.', 'E').
pub(crate) fn format_serialize_double(f: f64) -> String {
    if f.is_nan() {
        return "NAN".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "INF".to_string() } else { "-INF".to_string() };
    }
    let sp = goro_core::value::get_php_serialize_precision();
    if sp < 0 {
        goro_core::value::format_php_float_shortest_pub(f)
    } else {
        goro_core::value::format_php_float_with_precision_pub(f, if sp == 0 { 1 } else { sp as usize })
    }
}

/// Take an element out of a container: a reference with refcount 1 is
/// treated as a plain value (`Z_ISREF_P(data) && Z_REFCOUNT_P(data) == 1`).
fn element_value(v: &Value) -> Value {
    if let Value::Reference(r) = v {
        if Rc::strong_count(r) == 1 {
            return r.borrow().clone();
        }
    }
    v.clone()
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
}

impl Serializer {
    /// php_add_var_hash(): returns 0 for a new value, -1 for an object whose
    /// serialization failed, or the back-reference index.
    fn add_var_hash(&mut self, var: &Value) -> i64 {
        let mut h = self.hash.borrow_mut();
        h.n += 1;
        let (is_ref, key, keep) = match var {
            Value::Reference(r) => {
                let inner = r.borrow();
                if let Value::Object(o) = &*inner {
                    (true, Rc::as_ptr(o) as *const u8 as usize, Value::Object(o.clone()))
                } else {
                    (true, Rc::as_ptr(r) as *const u8 as usize, var.clone())
                }
            }
            Value::Object(o) => (false, Rc::as_ptr(o) as *const u8 as usize, var.clone()),
            _ => return 0,
        };
        if let Some(&idx) = h.map.get(&key) {
            if is_ref && idx != -1 {
                h.n -= 1;
            }
            return idx;
        }
        let n = h.n;
        h.map.insert(key, n);
        h.keep.push(keep);
        0
    }

    fn mark_failed(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        let key = Rc::as_ptr(obj) as *const u8 as usize;
        if let Some(v) = self.hash.borrow_mut().map.get_mut(&key) {
            *v = -1;
        }
    }

    fn serialize(&mut self, vm: &mut Vm, val: &Value, is_root: bool) {
        if vm.current_exception.is_some() {
            return;
        }
        if self.depth > 3000 {
            throw(vm, b"Error", "Maximum call stack size of 8339456 bytes (zend.max_allowed_stack_size - zend.reserved_stack_size) reached. Infinite recursion?");
            return;
        }
        let already = self.add_var_hash(val);
        if already != 0 {
            if already == -1 {
                self.buf.extend_from_slice(b"N;");
                return;
            }
            if matches!(val, Value::Reference(_)) {
                self.buf.extend_from_slice(format!("R:{};", already).as_bytes());
                return;
            }
            if matches!(val, Value::Object(_)) {
                self.buf.extend_from_slice(format!("r:{};", already).as_bytes());
                return;
            }
        }
        let v = match val {
            Value::Reference(r) => r.borrow().clone(),
            other => other.clone(),
        };
        match &v {
            Value::False => self.buf.extend_from_slice(b"b:0;"),
            Value::True => self.buf.extend_from_slice(b"b:1;"),
            Value::Null | Value::Undef => self.buf.extend_from_slice(b"N;"),
            Value::Long(n) => push_long(&mut self.buf, *n),
            Value::Double(f) => {
                self.buf.extend_from_slice(b"d:");
                self.buf.extend_from_slice(format_serialize_double(*f).as_bytes());
                self.buf.push(b';');
            }
            Value::String(s) => {
                if is_closure_name(s.as_bytes()) || s.as_bytes().starts_with(b"__closure_fcc_") {
                    throw(vm, b"Exception", "Serialization of 'Closure' is not allowed");
                    return;
                }
                push_str(&mut self.buf, s.as_bytes())
            }
            Value::Generator(_) => {
                throw(vm, b"Exception", "Serialization of 'Generator' is not allowed");
            }
            Value::Array(arr) => {
                self.buf.extend_from_slice(b"a:");
                let items: Vec<(ArrayKey, Value)> = {
                    let a = arr.borrow();
                    a.iter().map(|(k, v)| (k.clone(), element_value(v))).collect()
                };
                self.depth += 1;
                self.buf.extend_from_slice(format!("{}:{{", items.len()).as_bytes());
                for (k, item) in &items {
                    push_key(&mut self.buf, k);
                    if let Value::Array(inner) = item {
                        if Rc::ptr_eq(inner, arr) {
                            self.add_var_hash(&v);
                            self.buf.extend_from_slice(b"N;");
                            continue;
                        }
                    }
                    self.serialize(vm, item, false);
                }
                self.buf.push(b'}');
                self.depth -= 1;
            }
            Value::Object(obj) => {
                self.depth += 1;
                self.serialize_object(vm, &v, obj);
                self.depth -= 1;
            }
            Value::Resource(_) => self.buf.extend_from_slice(b"i:0;"),
            Value::Reference(_) => self.buf.extend_from_slice(b"N;"),
        }
        let _ = is_root;
    }

    fn serialize_object(&mut self, vm: &mut Vm, val: &Value, obj_in: &Rc<RefCell<PhpObject>>) {
        // Lazy objects: an initialized proxy serializes its instance;
        // otherwise the object is initialized first, unless the class has
        // __serialize() (which runs on the lazy object) or the object was
        // created with SKIP_INITIALIZATION_ON_SERIALIZE.
        let mut obj = obj_in.clone();
        let (lazy_state, has_real, flags) = {
            let ob = obj.borrow();
            (ob.get_property(b"__lazy_state"), ob.has_property(b"__lazy_real"), ob.get_property(b"__lazy_flags").to_long())
        };
        if let Value::String(st) = &lazy_state {
            if st.as_bytes() == b"proxy" && has_real {
                obj = vm.proxy_redirect(&obj);
            } else if st.as_bytes() == b"ghost" || st.as_bytes() == b"proxy" {
                let cl = lower(&obj.borrow().class_name);
                if !has_magic_serialize(vm, &cl) && flags & 8 == 0 {
                    vm.maybe_run_lazy_init(&obj);
                    if vm.current_exception.is_some() {
                        return;
                    }
                    obj = vm.proxy_redirect(&obj);
                }
            }
        }
        let val = &Value::Object(obj.clone());
        let class_name = obj.borrow().class_name.clone();
        let class_lower = lower(&class_name);
        let display = canonical_class_name(vm, &class_lower, &class_name);

        if is_not_serializable(vm, &class_name) {
            let msg = format!("Serialization of '{}' is not allowed", goro_core::value::display_class_name(&display));
            throw(vm, b"Exception", &msg);
            return;
        }

        if Vm::is_enum_case(val) {
            let case_name = obj.borrow().get_property(b"name").to_php_string();
            let mut full = display.clone();
            full.push(b':');
            full.extend_from_slice(case_name.as_bytes());
            self.buf.extend_from_slice(format!("E:{}:\"", full.len()).as_bytes());
            self.buf.extend_from_slice(&full);
            self.buf.extend_from_slice(b"\";");
            return;
        }

        let incomplete = class_lower == lower(INCOMPLETE_CLASS);

        if !incomplete && has_magic_serialize(vm, &class_lower) {
            let saved_line = vm.current_line;
            let result = with_lock(|| vm.call_object_method(val, b"__serialize", &[]));
            vm.current_line = saved_line;
            if vm.current_exception.is_some() {
                return;
            }
            let result = result.map(|r| r.deref());
            let arr = match result {
                Some(Value::Array(a)) => a,
                _ => {
                    let msg = format!("{}::__serialize() must return an array", goro_core::value::display_class_name(&display));
                    throw(vm, b"TypeError", &msg);
                    return;
                }
            };
            push_class_name(&mut self.buf, &display);
            let items: Vec<(ArrayKey, Value)> = {
                let a = arr.borrow();
                a.iter().map(|(k, v)| (k.clone(), element_value(v))).collect()
            };
            self.buf.extend_from_slice(format!("{}:{{", items.len()).as_bytes());
            for (k, item) in &items {
                push_key(&mut self.buf, k);
                self.serialize(vm, item, false);
            }
            self.buf.push(b'}');
            return;
        }

        if !incomplete && is_serializable_iface(vm, &class_lower) {
            let saved_line = vm.current_line;
            let result = vm.call_object_method(val, b"serialize", &[]);
            vm.current_line = saved_line;
            let ok = match result.map(|r| r.deref()) {
                Some(Value::String(s)) if vm.current_exception.is_none() => Some(s),
                Some(Value::Null) => None,
                _ => {
                    if vm.current_exception.is_none() {
                        let msg = format!("{}::serialize() must return a string or NULL", goro_core::value::display_class_name(&display));
                        throw(vm, b"Exception", &msg);
                    }
                    None
                }
            };
            match ok {
                Some(s) => {
                    self.buf.extend_from_slice(format!("C:{}:\"", display.len()).as_bytes());
                    self.buf.extend_from_slice(&display);
                    self.buf.extend_from_slice(format!("\":{}:{{", s.len()).as_bytes());
                    self.buf.extend_from_slice(s.as_bytes());
                    self.buf.push(b'}');
                }
                None => {
                    self.mark_failed(&obj);
                    self.buf.extend_from_slice(b"N;");
                }
            }
            return;
        }

        if !incomplete && has_user_method(vm, &class_lower, b"__sleep") {
            let saved_line = vm.current_line;
            let result = with_lock(|| vm.call_object_method(val, b"__sleep", &[]));
            vm.current_line = saved_line;
            if vm.current_exception.is_some() {
                return;
            }
            let names = match result.map(|r| r.deref()) {
                Some(Value::Array(a)) => a,
                _ => {
                    vm.emit_warning_at(&format!(
                        "serialize(): {}::__sleep() should return an array only containing the names of instance-variables to serialize",
                        goro_core::value::display_class_name(&display)), saved_line);
                    self.buf.extend_from_slice(b"N;");
                    return;
                }
            };
            let names: Vec<Value> = names.borrow().values().map(|v| v.deref()).collect();
            let props = mangled_props(vm, &obj.borrow());
            let mut chosen: Vec<(Vec<u8>, Value)> = Vec::new();
            for name_val in &names {
                if !matches!(name_val, Value::String(_)) {
                    vm.emit_warning_at(&format!(
                        "serialize(): {}::__sleep() should return an array only containing the names of instance-variables to serialize",
                        goro_core::value::display_class_name(&display)), saved_line);
                }
                let name = name_val.to_php_string().as_bytes().to_vec();
                let candidates = [name.clone(), mangle(&display, &name), mangle(b"*", &name)];
                let mut found = false;
                for cand in candidates.iter() {
                    if let Some(e) = props.iter().find(|e| e.php_key == *cand) {
                        found = true;
                        if matches!(e.value, Value::Undef) {
                            break;
                        }
                        if chosen.iter().any(|(k, _)| k == cand) {
                            vm.emit_warning_at(&format!(
                                "serialize(): \"{}\" is returned from __sleep() multiple times",
                                String::from_utf8_lossy(&name)), saved_line);
                        } else {
                            chosen.push((cand.clone(), e.value.clone()));
                        }
                        break;
                    }
                }
                if !found {
                    vm.emit_warning_at(&format!(
                        "serialize(): \"{}\" returned as member variable from __sleep() but does not exist",
                        String::from_utf8_lossy(&name)), saved_line);
                }
            }
            push_class_name(&mut self.buf, &display);
            self.buf.extend_from_slice(format!("{}:{{", chosen.len()).as_bytes());
            for (k, v) in &chosen {
                push_str(&mut self.buf, k);
                let item = element_value(v);
                self.serialize(vm, &item, false);
            }
            self.buf.push(b'}');
            return;
        }

        // Default: all initialized properties.
        let props = mangled_props(vm, &obj.borrow());
        let out_name = if incomplete {
            match obj.borrow().get_property(MAGIC_MEMBER) {
                Value::String(s) => s.as_bytes().to_vec(),
                _ => INCOMPLETE_CLASS.to_vec(),
            }
        } else {
            display.clone()
        };
        let mut skip_magic = incomplete;
        let items: Vec<(Vec<u8>, Value)> = props.into_iter()
            .filter(|e| !matches!(e.value, Value::Undef))
            .filter(|e| {
                if skip_magic && e.php_key == MAGIC_MEMBER {
                    skip_magic = false;
                    return false;
                }
                true
            })
            .map(|e| (e.php_key, element_value(&e.value)))
            .collect();
        push_class_name(&mut self.buf, &out_name);
        self.buf.extend_from_slice(format!("{}:{{", items.len()).as_bytes());
        for (k, item) in &items {
            push_str(&mut self.buf, k);
            self.serialize(vm, item, false);
        }
        self.buf.push(b'}');
    }
}

/// Serialize a value with a (possibly shared) serialization context.
/// On exception returns None (the exception is left in vm.current_exception).
pub fn php_serialize(vm: &mut Vm, val: &Value) -> Option<Vec<u8>> {
    let hash = ser_init();
    let mut s = Serializer { buf: Vec::new(), hash, depth: 0, line: vm.current_line };
    s.serialize(vm, val, true);
    let _ = s.line;
    ser_destroy();
    if vm.current_exception.is_some() {
        None
    } else {
        Some(s.buf)
    }
}

fn exception_err(vm: &Vm) -> VmError {
    let msg = if let Some(Value::Object(o)) = &vm.current_exception {
        let ob = o.borrow();
        format!("Uncaught {}: {}", goro_core::value::display_class_name(&ob.class_name),
            ob.get_property(b"message").to_php_string().to_string_lossy())
    } else {
        "Uncaught exception".to_string()
    };
    VmError { message: msg, line: vm.current_line }
}

pub fn serialize_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let val = args.first().cloned().unwrap_or(Value::Null);
    let val = match val {
        Value::Reference(r) => r.borrow().clone(),
        v => v,
    };
    vm.call_stack.push(("serialize".to_string(), vm.current_file.clone(), vm.current_line, vec![val.clone()], false));
    let line = vm.current_line;
    let r = php_serialize(vm, &val);
    vm.current_line = line;
    vm.call_stack.pop();
    match r {
        Some(buf) => Ok(Value::String(PhpString::from_vec(buf))),
        None => Err(exception_err(vm)),
    }
}

// ---------------------------------------------------------------------------
// unserialize()
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum Slot {
    Tmp(usize),
    Arr(Rc<RefCell<PhpArray>>, ArrayKey),
    Prop(Rc<RefCell<PhpObject>>, Vec<u8>),
}

impl Slot {
    fn same(&self, other: &Slot) -> bool {
        match (self, other) {
            (Slot::Tmp(a), Slot::Tmp(b)) => a == b,
            (Slot::Arr(a, k1), Slot::Arr(b, k2)) => Rc::ptr_eq(a, b) && k1 == k2,
            (Slot::Prop(a, k1), Slot::Prop(b, k2)) => Rc::ptr_eq(a, b) && k1 == k2,
            _ => false,
        }
    }
}

struct Delayed {
    obj: Value,
    /// Some(data) → __unserialize(data); None → __wakeup()
    data: Option<Value>,
}

struct UnserData {
    entries: Vec<Option<Slot>>,
    tmps: Vec<Value>,
    delayed: Vec<Delayed>,
    allowed_classes: Option<Vec<Vec<u8>>>,
    max_depth: i64,
    cur_depth: i64,
}

impl UnserData {
    fn get(&self, s: &Slot) -> Value {
        match s {
            Slot::Tmp(i) => self.tmps.get(*i).cloned().unwrap_or(Value::Null),
            Slot::Arr(a, k) => a.borrow().get(k).cloned().unwrap_or(Value::Null),
            Slot::Prop(o, k) => o.borrow().get_property(k),
        }
    }
}

fn slot_set(d: &Rc<RefCell<UnserData>>, s: &Slot, v: Value) {
    match s {
        Slot::Tmp(i) => {
            let mut dd = d.borrow_mut();
            if *i < dd.tmps.len() {
                dd.tmps[*i] = v;
            }
        }
        Slot::Arr(a, k) => a.borrow_mut().set(k.clone(), v),
        Slot::Prop(o, k) => o.borrow_mut().set_property(k.clone(), v),
    }
}

fn unser_init(vm: &Vm) -> Rc<RefCell<UnserData>> {
    let level = UNSER_LEVEL.with(|l| l.get());
    if locked() || level == 0 {
        let max_depth = vm.constants.get(b"unserialize_max_depth".as_ref())
            .map(|v| v.to_long())
            .unwrap_or(4096);
        let d = Rc::new(RefCell::new(UnserData {
            entries: Vec::new(),
            tmps: Vec::new(),
            delayed: Vec::new(),
            allowed_classes: None,
            max_depth,
            cur_depth: 0,
        }));
        if !locked() {
            UNSER_DATA.with(|s| *s.borrow_mut() = Some(d.clone()));
            UNSER_LEVEL.with(|l| l.set(1));
        }
        d
    } else {
        UNSER_LEVEL.with(|l| l.set(level + 1));
        UNSER_DATA.with(|s| s.borrow().clone()).expect("unserialize data")
    }
}

/// PHP_VAR_UNSERIALIZE_DESTROY: on the outermost level, run the delayed
/// __wakeup()/__unserialize() calls.
fn unser_destroy(vm: &mut Vm, d: &Rc<RefCell<UnserData>>) {
    let was_locked = locked();
    let level = UNSER_LEVEL.with(|l| l.get());
    let outermost = was_locked || level == 1;
    if !was_locked {
        let nl = level.saturating_sub(1);
        UNSER_LEVEL.with(|l| l.set(nl));
        if nl == 0 {
            UNSER_DATA.with(|s| *s.borrow_mut() = None);
        }
    }
    if !outermost {
        return;
    }
    let delayed = {
        let mut dd = d.borrow_mut();
        dd.entries.clear();
        dd.tmps.clear();
        std::mem::take(&mut dd.delayed)
    };
    let saved_line = vm.current_line;
    let mut failed = vm.current_exception.is_some();
    for item in delayed {
        if failed {
            continue;
        }
        match item.data {
            None => {
                with_lock(|| vm.call_object_method(&item.obj, b"__wakeup", &[]));
            }
            Some(data) => {
                with_lock(|| vm.call_object_method(&item.obj, b"__unserialize", &[data]));
            }
        }
        vm.current_line = saved_line;
        if vm.current_exception.is_some() {
            failed = true;
        }
    }
}

struct Parser<'a> {
    data: &'a [u8],
    d: Rc<RefCell<UnserData>>,
    line: u32,
}

/// parse_iv2(): returns (value, end position), warning on overflow.
fn parse_iv2(vm: &mut Vm, data: &[u8], mut p: usize, line: u32) -> (i64, usize) {
    let at = |i: usize| -> u8 { data.get(i).copied().unwrap_or(0) };
    let mut neg = false;
    if at(p) == b'-' {
        neg = true;
        p += 1;
    } else if at(p) == b'+' {
        p += 1;
    }
    while at(p) == b'0' {
        p += 1;
    }
    let start = p;
    let mut result: u64 = 0;
    while at(p).is_ascii_digit() {
        result = result.wrapping_mul(10).wrapping_add((at(p) - b'0') as u64);
        p += 1;
    }
    let max = i64::MAX as u64 + if neg { 1 } else { 0 };
    if p - start > 19 || result > max {
        vm.emit_warning_at("unserialize(): Numerical result out of range", line);
        return (if neg { i64::MIN } else { i64::MAX }, p);
    }
    let v = if neg { (result as i64).wrapping_neg() } else { result as i64 };
    (v, p)
}

fn parse_uiv(data: &[u8], mut p: usize) -> u64 {
    let mut r: u64 = 0;
    while let Some(&c) = data.get(p) {
        if !c.is_ascii_digit() {
            break;
        }
        r = r.wrapping_mul(10).wrapping_add((c - b'0') as u64);
        p += 1;
    }
    r
}

impl<'a> Parser<'a> {
    fn at(&self, i: usize) -> u8 {
        self.data.get(i).copied().unwrap_or(0)
    }

    fn max(&self) -> usize {
        self.data.len()
    }

    fn warn(&self, vm: &mut Vm, msg: &str) {
        vm.emit_warning_at(&format!("unserialize(): {}", msg), self.line);
    }

    fn warn_raw(&self, vm: &mut Vm, msg: &str) {
        vm.emit_warning_at(msg, self.line);
    }

    /// Match `[0-9]+` at p; returns end.
    fn digits(&self, p: usize) -> Option<usize> {
        let mut e = p;
        while self.at(e).is_ascii_digit() {
            e += 1;
        }
        if e == p { None } else { Some(e) }
    }

    /// Match `[+-]?[0-9]+`
    fn iv(&self, p: usize) -> Option<usize> {
        let mut s = p;
        if self.at(s) == b'+' || self.at(s) == b'-' {
            s += 1;
        }
        self.digits(s)
    }

    /// Match the double grammar `iv | nv | nvexp` followed by ';'. Returns end (at ';').
    fn double_token(&self, p: usize) -> Option<usize> {
        let mut s = p;
        if self.at(s) == b'+' || self.at(s) == b'-' {
            s += 1;
        }
        let int_end = {
            let mut e = s;
            while self.at(e).is_ascii_digit() {
                e += 1;
            }
            e
        };
        let mut e = int_end;
        let has_int = int_end > s;
        if self.at(e) == b'.' {
            let frac_start = e + 1;
            let mut fe = frac_start;
            while self.at(fe).is_ascii_digit() {
                fe += 1;
            }
            let has_frac = fe > frac_start;
            if !has_int && !has_frac {
                return None;
            }
            e = fe;
        } else if !has_int {
            return None;
        }
        // optional exponent
        if self.at(e) == b'e' || self.at(e) == b'E' {
            if let Some(ee) = self.iv(e + 1) {
                if self.at(ee) == b';' {
                    return Some(ee);
                }
            }
        }
        if self.at(e) == b';' {
            Some(e)
        } else {
            None
        }
    }

    fn push_var(&self, slot: &Slot) {
        self.d.borrow_mut().entries.push(Some(slot.clone()));
    }

    /// php_var_unserialize(): parse one value into `slot`, nulling the
    /// entries added on failure.
    fn unserialize(&mut self, vm: &mut Vm, p: &mut usize, slot: &Slot) -> bool {
        let orig = self.d.borrow().entries.len();
        let ok = self.unserialize_internal(vm, p, Some(slot)).is_some();
        if !ok {
            let mut d = self.d.borrow_mut();
            for e in d.entries.iter_mut().skip(orig) {
                *e = None;
            }
        }
        ok
    }

    /// Parse a key (no var_hash): returns the scalar.
    fn parse_key(&mut self, vm: &mut Vm, p: &mut usize) -> Option<Value> {
        self.unserialize_internal(vm, p, None)
    }

    fn set(&self, slot: Option<&Slot>, v: Value) -> Option<Value> {
        if let Some(s) = slot {
            slot_set(&self.d, s, v.clone());
        }
        Some(v)
    }

    fn unserialize_internal(&mut self, vm: &mut Vm, p: &mut usize, slot: Option<&Slot>) -> Option<Value> {
        let start = *p;
        if start >= self.max() {
            return None;
        }
        let c0 = self.at(start);
        if let Some(s) = slot {
            if c0 != b'R' {
                self.push_var(s);
            }
        }
        let c1 = self.at(start + 1);
        match (c0, c1) {
            (b'R', b':') | (b'r', b':') => {
                let e = self.digits(start + 2)?;
                if self.at(e) != b';' {
                    return None;
                }
                *p = e + 1;
                let slot = slot?;
                let id = parse_uiv(self.data, start + 2) as i64 - 1;
                if id < 0 {
                    return None;
                }
                let target = self.d.borrow().entries.get(id as usize).cloned().flatten()?;
                if target.same(slot) {
                    return None;
                }
                let cur = self.d.borrow().get(&target);
                if c0 == b'R' {
                    let r = match cur {
                        Value::Reference(r) => r,
                        other => {
                            let r = Rc::new(RefCell::new(other));
                            slot_set(&self.d, &target, Value::Reference(r.clone()));
                            r
                        }
                    };
                    self.set(Some(slot), Value::Reference(r))
                } else {
                    let v = cur.deref();
                    if !matches!(v, Value::Object(_)) {
                        return None;
                    }
                    self.set(Some(slot), v)
                }
            }
            (b'N', b';') => {
                *p = start + 2;
                self.set(slot, Value::Null)
            }
            (b'b', b':') => {
                let c = self.at(start + 2);
                if (c == b'0' || c == b'1') && self.at(start + 3) == b';' {
                    *p = start + 4;
                    self.set(slot, if c == b'1' { Value::True } else { Value::False })
                } else {
                    None
                }
            }
            (b'i', b':') => {
                let e = self.iv(start + 2)?;
                if self.at(e) != b';' {
                    return None;
                }
                *p = e + 1;
                let (v, _) = parse_iv2(vm, self.data, start + 2, self.line);
                self.set(slot, Value::Long(v))
            }
            (b'd', b':') => {
                let rest = &self.data[(start + 2).min(self.max())..];
                for (tok, val) in [(&b"NAN;"[..], f64::NAN), (&b"INF;"[..], f64::INFINITY), (&b"-INF;"[..], f64::NEG_INFINITY)] {
                    if rest.starts_with(tok) {
                        *p = start + 2 + tok.len();
                        return self.set(slot, Value::Double(val));
                    }
                }
                let e = self.double_token(start + 2)?;
                *p = e + 1;
                let s = String::from_utf8_lossy(&self.data[start + 2..e]).to_string();
                let f = s.parse::<f64>().unwrap_or(0.0);
                self.set(slot, Value::Double(f))
            }
            (b's', b':') | (b'S', b':') => {
                let e = self.digits(start + 2)?;
                if self.at(e) != b':' || self.at(e + 1) != b'"' {
                    return None;
                }
                let mut cursor = e + 2;
                let len = parse_uiv(self.data, start + 2);
                let maxlen = (self.max() - cursor.min(self.max())) as u64;
                if maxlen < len {
                    *p = start + 2;
                    return None;
                }
                let len = len as usize;
                let bytes: Vec<u8> = if c0 == b's' {
                    let b = self.data[cursor..cursor + len].to_vec();
                    cursor += len;
                    b
                } else {
                    // unserialize_str(): \xx hex escapes
                    let end = self.max();
                    let mut out = Vec::with_capacity(len);
                    for _ in 0..len {
                        if cursor >= end {
                            return None;
                        }
                        if self.at(cursor) != b'\\' {
                            out.push(self.at(cursor));
                        } else {
                            let mut ch: u8 = 0;
                            for _ in 0..2 {
                                cursor += 1;
                                let c = self.at(cursor);
                                let d = match c {
                                    b'0'..=b'9' => c - b'0',
                                    b'a'..=b'f' => c - b'a' + 10,
                                    b'A'..=b'F' => c - b'A' + 10,
                                    _ => return None,
                                };
                                ch = (ch << 4) + d;
                            }
                            out.push(ch);
                        }
                        cursor += 1;
                    }
                    out
                };
                if self.at(cursor) != b'"' {
                    *p = cursor;
                    return None;
                }
                if self.at(cursor + 1) != b';' {
                    *p = cursor + 1;
                    return None;
                }
                *p = cursor + 2;
                if c0 == b'S' {
                    vm.emit_deprecated_at("unserialize(): Unserializing the 'S' format is deprecated", self.line);
                }
                self.set(slot, Value::String(PhpString::from_vec(bytes)))
            }
            (b'a', b':') => {
                let e = self.digits(start + 2)?;
                if self.at(e) != b':' || self.at(e + 1) != b'{' {
                    return None;
                }
                let (elements, _) = parse_iv2(vm, self.data, start + 2, self.line);
                *p = e + 2;
                let slot = slot?;
                let remaining = (self.max() - (*p).min(self.max())) as i64;
                if elements < 0 || elements >= 0x40000000 || elements > remaining / 2 {
                    return None;
                }
                let arr = Rc::new(RefCell::new(PhpArray::new()));
                slot_set(&self.d, slot, Value::Array(arr.clone()));
                if elements > 0 && !self.nested_array(vm, p, &arr, elements) {
                    return None;
                }
                if !self.finish_nested(p) {
                    return None;
                }
                Some(Value::Null)
            }
            (b'O', b':') | (b'C', b':') => {
                let e = self.digits(start + 2)?;
                if self.at(e) != b':' || self.at(e + 1) != b'"' {
                    return None;
                }
                let slot = slot?.clone();
                self.parse_object(vm, p, start, e + 2, c0 == b'C', &slot)
            }
            (b'E', b':') => {
                let e = self.digits(start + 2)?;
                if self.at(e) != b':' || self.at(e + 1) != b'"' {
                    return None;
                }
                let slot = slot?.clone();
                let mut cursor = e + 2;
                let len = parse_uiv(self.data, start + 2);
                let maxlen = (self.max() - cursor.min(self.max())) as u64;
                if maxlen < len || len == 0 {
                    *p = start + 2;
                    return None;
                }
                let s = self.data[cursor..cursor + len as usize].to_vec();
                cursor += len as usize;
                if self.at(cursor) != b'"' {
                    *p = cursor;
                    return None;
                }
                if self.at(cursor + 1) != b';' {
                    *p = cursor + 1;
                    return None;
                }
                let colon = match s.iter().position(|&b| b == b':') {
                    Some(c) => c,
                    None => {
                        self.warn(vm, &format!("Invalid enum name '{}' (missing colon)", String::from_utf8_lossy(&s)));
                        return None;
                    }
                };
                let enum_name = &s[..colon];
                let case_name = &s[colon + 1..];
                if !is_valid_class_name(enum_name) {
                    return None;
                }
                let en = String::from_utf8_lossy(enum_name).to_string();
                let cl = lower(enum_name);
                if !vm.classes.contains_key(&cl) {
                    with_lock(|| vm.try_autoload_class(enum_name));
                    vm.current_line = self.line;
                    if vm.current_exception.is_some() {
                        return None;
                    }
                }
                let (is_enum, has_case, has_const, canon) = match vm.classes.get(&cl) {
                    None => {
                        self.warn(vm, &format!("Class '{}' not found", en));
                        return None;
                    }
                    Some(ce) => (
                        ce.is_enum,
                        ce.enum_cases.iter().any(|(n, _)| n == case_name),
                        ce.constants.contains_key(case_name),
                        String::from_utf8_lossy(&ce.name).to_string(),
                    ),
                };
                if !is_enum {
                    self.warn(vm, &format!("Class '{}' is not an enum", en));
                    return None;
                }
                *p = cursor + 2;
                let cn = String::from_utf8_lossy(case_name).to_string();
                if !has_case {
                    if has_const {
                        self.warn(vm, &format!("{}::{} is not an enum case", en, cn));
                    } else {
                        self.warn(vm, &format!("Undefined constant {}::{}", en, cn));
                    }
                    return None;
                }
                let _ = canon;
                let v = vm.get_enum_case(&cl, case_name)?;
                self.set(Some(&slot), v)
            }
            (b'}', _) => {
                self.warn(vm, "Unexpected end of serialized data");
                None
            }
            _ => None,
        }
    }

    fn depth_enter(&self, vm: &mut Vm) -> bool {
        let (max, cur) = {
            let d = self.d.borrow();
            (d.max_depth, d.cur_depth)
        };
        if max > 0 && cur >= max {
            self.warn(vm, &format!(
                "Maximum depth of {} exceeded. The depth limit can be changed using the max_depth unserialize() option or the unserialize_max_depth ini setting",
                max));
            return false;
        }
        self.d.borrow_mut().cur_depth += 1;
        true
    }

    fn depth_leave(&self) {
        self.d.borrow_mut().cur_depth -= 1;
    }

    fn last_char_ok(&self, p: &mut usize) -> bool {
        let c = if *p > 0 { self.at(*p - 1) } else { 0 };
        if c != b';' && c != b'}' {
            *p -= 1;
            return false;
        }
        true
    }

    fn nested_array(&mut self, vm: &mut Vm, p: &mut usize, arr: &Rc<RefCell<PhpArray>>, elements: i64) -> bool {
        if !self.depth_enter(vm) {
            return false;
        }
        let mut left = elements;
        let ok = loop {
            if left <= 0 {
                break true;
            }
            left -= 1;
            let key = match self.parse_key(vm, p) {
                Some(k) => k,
                None => break false,
            };
            let key = match key {
                Value::Long(n) => ArrayKey::Int(n),
                Value::String(_) => Vm::value_to_array_key(key),
                _ => break false,
            };
            arr.borrow_mut().set(key.clone(), Value::Null);
            let slot = Slot::Arr(arr.clone(), key);
            if !self.unserialize(vm, p, &slot) {
                break false;
            }
            if left > 0 && !self.last_char_ok(p) {
                break false;
            }
        };
        self.depth_leave();
        ok
    }

    fn finish_nested(&self, p: &mut usize) -> bool {
        if *p >= self.max() || self.at(*p) != b'}' {
            return false;
        }
        *p += 1;
        true
    }

    /// Resolve the class for an O:/C: entry. Returns (class_lower, canonical, incomplete).
    fn resolve_class(&mut self, vm: &mut Vm, name: &[u8]) -> Result<(Vec<u8>, Vec<u8>, bool), ()> {
        let lc = lower(name);
        let allowed = {
            let d = self.d.borrow();
            match &d.allowed_classes {
                None => true,
                Some(list) => list.iter().any(|c| *c == lc),
            }
        };
        if !allowed {
            if !is_valid_class_name(name) {
                return Err(());
            }
            return Ok((lower(INCOMPLETE_CLASS), INCOMPLETE_CLASS.to_vec(), true));
        }
        let exists = |vm: &Vm| -> bool {
            vm.classes.get(&lc).map(|c| !c.name.windows(10).any(|w| w == b"@anonymous")).unwrap_or(false)
                || (vm.is_known_builtin_class(&lc) && !vm.classes.contains_key(&lc))
                || not_serializable_builtin(&lc)
                || (!vm.classes.contains_key(&lc)
                    && (lc == b"exception" || lc == b"error"
                        || goro_core::vm::is_builtin_subclass(&lc, b"exception")
                        || goro_core::vm::is_builtin_subclass(&lc, b"error")))
        };
        if exists(vm) {
            return Ok((lc.clone(), canonical_class_name(vm, &lc, name), false));
        }
        if !is_valid_class_name(name) {
            return Err(());
        }
        with_lock(|| vm.try_autoload_class(name));
        vm.current_line = self.line;
        if vm.current_exception.is_some() {
            return Err(());
        }
        if exists(vm) {
            return Ok((lc.clone(), canonical_class_name(vm, &lc, name), false));
        }
        let cb = vm.constants.get(b"unserialize_callback_func".as_ref())
            .map(|v| v.to_php_string().as_bytes().to_vec())
            .unwrap_or_default();
        if cb.is_empty() {
            return Ok((lower(INCOMPLETE_CLASS), INCOMPLETE_CLASS.to_vec(), true));
        }
        let cb_lower = lower(&cb);
        let fn_exists = vm.user_functions.contains_key(&cb_lower) || vm.functions.contains_key(&cb_lower);
        if !fn_exists {
            let cbs = String::from_utf8_lossy(&cb).to_string();
            throw(vm, b"Error", &format!("Invalid callback {}, function \"{}\" not found or invalid function name", cbs, cbs));
            return Err(());
        }
        let arg = Value::String(PhpString::from_bytes(name));
        let _ = with_lock(|| vm.call_callback(&Value::String(PhpString::from_vec(cb.clone())), &[arg]));
        vm.current_line = self.line;
        if vm.current_exception.is_some() {
            return Err(());
        }
        if !exists(vm) {
            with_lock(|| vm.try_autoload_class(name));
            vm.current_line = self.line;
            if vm.current_exception.is_some() {
                return Err(());
            }
        }
        if exists(vm) {
            return Ok((lc.clone(), canonical_class_name(vm, &lc, name), false));
        }
        self.warn(vm, &format!("Function {}() hasn't defined the class it was called for", String::from_utf8_lossy(&cb)));
        Ok((lower(INCOMPLETE_CLASS), INCOMPLETE_CLASS.to_vec(), true))
    }

    /// object_init_ex(): create an instance with default properties.
    fn instantiate(&mut self, vm: &mut Vm, class_lower: &[u8], canonical: &[u8]) -> Option<Rc<RefCell<PhpObject>>> {
        if !vm.classes.contains_key(class_lower)
            && matches!(class_lower, b"filteriterator" | b"recursivefilteriterator" | b"splheap" | b"reflectionfunctionabstract")
        {
            let msg = format!("Cannot instantiate abstract class {}", goro_core::value::display_class_name(canonical));
            throw(vm, b"Error", &msg);
            return None;
        }
        if let Some(ce) = vm.classes.get(class_lower) {
            let kind = if ce.is_interface {
                Some("interface")
            } else if ce.is_trait {
                Some("trait")
            } else if ce.is_enum {
                Some("enum")
            } else if ce.is_abstract {
                Some("abstract class")
            } else {
                None
            };
            if let Some(kind) = kind {
                let msg = format!("Cannot instantiate {} {}", kind, goro_core::value::display_class_name(canonical));
                throw(vm, b"Error", &msg);
                return None;
            }
        }
        let id = vm.alloc_object_id();
        let mut obj = PhpObject::new(canonical.to_vec(), id);
        let props: Vec<(Vec<u8>, Value, bool, bool)> = match vm.classes.get(class_lower) {
            Some(class) => {
                let mut counts: HashMap<&[u8], usize> = HashMap::new();
                for p in class.properties.iter().filter(|p| !p.is_static) {
                    *counts.entry(p.name.as_slice()).or_insert(0) += 1;
                }
                class.properties.iter().filter(|p| !p.is_static).map(|p| {
                    let conflicted = counts.get(p.name.as_slice()).copied().unwrap_or(0) > 1;
                    let key = if conflicted && p.visibility == Visibility::Private {
                        let decl = canonical_class_name(vm, &lower(&p.declaring_class), &p.declaring_class);
                        mangle(&decl, &p.name)
                    } else {
                        p.name.clone()
                    };
                    (key, p.default.clone(), p.is_readonly, p.is_virtual)
                }).collect()
            }
            None => Vec::new(),
        };
        for (name, default, is_readonly, is_virtual) in props {
            if is_virtual {
                continue;
            }
            if is_readonly || matches!(default, Value::Undef) {
                obj.set_property(name, Value::Undef);
            } else {
                let resolved = match vm.resolve_deferred_value(&default) {
                    Value::Array(a) => Value::Array(Rc::new(RefCell::new(a.borrow().clone()))),
                    v => v,
                };
                obj.set_property(name, resolved);
            }
        }
        if let Some(base) = throwable_base(vm, class_lower) {
            let _ = base;
            for (k, v) in [
                (&b"message"[..], Value::String(PhpString::empty())),
                (b"code", Value::Long(0)),
                (b"file", Value::String(PhpString::empty())),
                (b"line", Value::Long(0)),
                (b"previous", Value::Null),
                (b"trace", Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
            ] {
                if !obj.has_property(k) {
                    obj.set_property(k.to_vec(), v);
                }
            }
        }
        if goro_core::spl::spl_class_needs_storage(class_lower)
            && !matches!(obj.get_property(b"__spl_array"), Value::Array(_))
        {
            obj.set_property(b"__spl_array".to_vec(), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
        }
        Some(Rc::new(RefCell::new(obj)))
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_object(&mut self, vm: &mut Vm, p: &mut usize, start: usize, name_start: usize, custom: bool, slot: &Slot) -> Option<Value> {
        let len = parse_uiv(self.data, start + 2);
        let maxlen = (self.max() - name_start.min(self.max())) as u64;
        if maxlen < len || len == 0 {
            *p = start + 2;
            return None;
        }
        let name = self.data[name_start..name_start + len as usize].to_vec();
        let mut cursor = name_start + len as usize;
        if self.at(cursor) != b'"' {
            *p = cursor;
            return None;
        }
        if self.at(cursor + 1) != b':' {
            *p = cursor + 1;
            return None;
        }
        if name[0] == 0 || name[0] == b'\\' {
            return None;
        }
        let (class_lower, canonical, incomplete) = self.resolve_class(vm, &name).ok()?;
        *p = cursor;

        if !incomplete && is_not_serializable(vm, &canonical) {
            let msg = format!("Unserialization of '{}' is not allowed", goro_core::value::display_class_name(&canonical));
            throw(vm, b"Exception", &msg);
            return None;
        }

        if custom {
            return self.object_custom(vm, p, &class_lower, &canonical, incomplete, &name, slot);
        }

        if *p + 2 >= self.max() {
            self.warn_raw(vm, "Bad unserialize data");
            return None;
        }
        let (elements, e) = parse_iv2(vm, self.data, *p + 2, self.line);
        *p = e;
        let remaining = (self.max() - name_start.min(self.max())) as i64;
        if elements < 0 || elements > remaining / 2 {
            return None;
        }
        cursor = *p;
        if self.at(cursor) != b':' {
            return None;
        }
        if self.at(cursor + 1) != b'{' {
            *p = cursor + 1;
            return None;
        }
        *p += 2;

        let has_unserialize = !incomplete && has_magic_unserialize(vm, &class_lower);
        if !incomplete && !has_unserialize && is_serializable_iface(vm, &class_lower) {
            self.warn_raw(vm, &format!("Erroneous data format for unserializing '{}'", goro_core::value::display_class_name(&canonical)));
            return None;
        }

        let obj = self.instantiate(vm, &class_lower, &canonical)?;
        if incomplete {
            obj.borrow_mut().set_property(MAGIC_MEMBER.to_vec(), Value::String(PhpString::from_vec(name.clone())));
        }
        let obj_val = Value::Object(obj.clone());
        slot_set(&self.d, slot, obj_val.clone());

        if has_unserialize {
            if elements >= 0x40000000 {
                return None;
            }
            let ary = Rc::new(RefCell::new(PhpArray::new()));
            if !self.nested_array(vm, p, &ary, elements) {
                return None;
            }
            self.d.borrow_mut().delayed.push(Delayed { obj: obj_val.clone(), data: Some(Value::Array(ary)) });
            if !self.finish_nested(p) {
                return None;
            }
            return Some(obj_val);
        }

        let has_wakeup = !incomplete && has_user_method(vm, &class_lower, b"__wakeup");
        if !self.nested_object(vm, p, &obj, &class_lower, elements) {
            return None;
        }
        if has_wakeup {
            self.d.borrow_mut().delayed.push(Delayed { obj: obj_val.clone(), data: None });
        }
        if !self.finish_nested(p) {
            return None;
        }
        Some(obj_val)
    }

    #[allow(clippy::too_many_arguments)]
    fn object_custom(&mut self, vm: &mut Vm, p: &mut usize, class_lower: &[u8], canonical: &[u8], incomplete: bool, name: &[u8], slot: &Slot) -> Option<Value> {
        let (datalen, e) = parse_iv2(vm, self.data, *p + 2, self.line);
        *p = e;
        if self.max() < *p + 2 {
            return None;
        }
        if self.at(*p) != b':' {
            return None;
        }
        if self.at(*p + 1) != b'{' {
            *p += 1;
            return None;
        }
        *p += 2;
        let present = self.max() as i64 - *p as i64;
        if datalen < 0 || present <= datalen {
            self.warn_raw(vm, &format!("Insufficient data for unserializing - {} required, {} present", datalen, present));
            return None;
        }
        let datalen = datalen as usize;
        if self.at(*p + datalen) != b'}' {
            *p += datalen;
            return None;
        }
        let payload = self.data[*p..*p + datalen].to_vec();
        let user_serializable = !incomplete && vm.class_implements_interface(class_lower, b"serializable");
        let builtin_unser = !incomplete && class_chain(vm, class_lower).iter().any(|c| matches!(c.as_slice(),
            b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" | b"splobjectstorage"
            | b"spldoublylinkedlist" | b"splqueue" | b"splstack"));
        if !incomplete && class_lower == b"gmp" {
            let v = self.gmp_custom(vm, &payload)?;
            slot_set(&self.d, slot, v.clone());
            *p += datalen + 1;
            return Some(v);
        }
        if !user_serializable && !builtin_unser {
            self.warn_raw(vm, &format!("Class {} has no unserializer", goro_core::value::display_class_name(canonical)));
            let obj = self.instantiate(vm, class_lower, canonical)?;
            if incomplete {
                obj.borrow_mut().set_property(MAGIC_MEMBER.to_vec(), Value::String(PhpString::from_bytes(name)));
            }
            let v = Value::Object(obj);
            slot_set(&self.d, slot, v.clone());
            *p += datalen + 1;
            return Some(v);
        }
        let obj = self.instantiate(vm, class_lower, canonical)?;
        let v = Value::Object(obj);
        slot_set(&self.d, slot, v.clone());
        let saved_line = vm.current_line;
        vm.call_object_method(&v, b"unserialize", &[Value::String(PhpString::from_vec(payload))]);
        vm.current_line = saved_line;
        if vm.current_exception.is_some() {
            return None;
        }
        *p += datalen + 1;
        Some(v)
    }

    /// Legacy GMP `C:3:"GMP":N:{s:N:"num";a:N:{props}}` format.
    fn gmp_custom(&mut self, vm: &mut Vm, payload: &[u8]) -> Option<Value> {
        let mut sub = Parser { data: payload, d: self.d.clone(), line: self.line };
        let mut p2 = 0usize;
        let num_idx = {
            let mut d = self.d.borrow_mut();
            d.tmps.push(Value::Null);
            d.tmps.len() - 1
        };
        let ok = sub.unserialize(vm, &mut p2, &Slot::Tmp(num_idx));
        if vm.current_exception.is_some() {
            return None;
        }
        let num_val = self.d.borrow().tmps.get(num_idx).cloned().unwrap_or(Value::Null).deref();
        let num = match (ok, num_val) {
            (true, Value::String(s)) if {
                let b = s.as_bytes();
                let digits = if b.first() == Some(&b'-') || b.first() == Some(&b'+') { &b[1..] } else { b };
                !digits.is_empty() && digits.iter().all(|c| c.is_ascii_digit())
            } => s.to_string_lossy(),
            _ => {
                throw(vm, b"Exception", "Could not unserialize number");
                return None;
            }
        };
        let id = vm.alloc_object_id();
        let obj = Rc::new(RefCell::new(PhpObject::new(b"GMP".to_vec(), id)));
        obj.borrow_mut().set_property(b"num".to_vec(), Value::String(PhpString::from_string(num.clone())));
        let obj_val = Value::Object(obj.clone());
        let hex = match num.parse::<i128>() {
            Ok(n) if n >= 0 => format!("{:x}", n),
            Ok(n) => format!("-{:x}", -n),
            Err(_) => String::new(),
        };
        if !hex.is_empty() {
            if let Some(setter) = vm.gmp_from_hex {
                setter(&obj_val, &hex);
            }
        }
        let tmp_idx = {
            let mut d = self.d.borrow_mut();
            d.tmps.push(Value::Null);
            d.tmps.len() - 1
        };
        let ok = sub.unserialize(vm, &mut p2, &Slot::Tmp(tmp_idx));
        let props = self.d.borrow().tmps.get(tmp_idx).cloned().unwrap_or(Value::Null).deref();
        let props = match (ok, props) {
            (true, Value::Array(a)) => a,
            _ => {
                if vm.current_exception.is_none() {
                    throw(vm, b"Exception", "Could not unserialize properties");
                }
                return None;
            }
        };
        let items: Vec<(ArrayKey, Value)> = props.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if !items.is_empty() {
            let mut ob = obj.borrow_mut();
            ob.remove_property(b"num");
            for (k, v) in items {
                let name: Vec<u8> = match k {
                    ArrayKey::String(s) => s.as_bytes().to_vec(),
                    ArrayKey::Int(n) => n.to_string().into_bytes(),
                };
                if name == b"num" {
                    continue;
                }
                ob.set_property(name, v);
            }
            ob.set_property(b"num".to_vec(), Value::String(PhpString::from_string(num)));
        }
        Some(obj_val)
    }

    /// Map a serialized (mangled) property name to the object's storage key.
    /// Returns Err(()) on failure (warning already emitted).
    fn resolve_prop(&mut self, vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>, class_lower: &[u8], key: &[u8])
        -> Result<(Vec<u8>, Option<goro_core::opcode::ParamType>, Vec<u8>), ()>
    {
        // Declared properties, keyed by their PHP mangled name.
        let decl: Vec<(Vec<u8>, Vec<u8>, Option<goro_core::opcode::ParamType>, bool, Vec<u8>, Visibility, Vec<u8>)> = {
            match vm.classes.get(class_lower) {
                Some(ce) => {
                    let mut counts: HashMap<&[u8], usize> = HashMap::new();
                    for p in ce.properties.iter().filter(|p| !p.is_static) {
                        *counts.entry(p.name.as_slice()).or_insert(0) += 1;
                    }
                    ce.properties.iter().filter(|p| !p.is_static).map(|p| {
                        let decl_name = canonical_class_name(vm, &lower(&p.declaring_class), &p.declaring_class);
                        let php_key = match p.visibility {
                            Visibility::Private => mangle(&decl_name, &p.name),
                            Visibility::Protected => mangle(b"*", &p.name),
                            _ => p.name.clone(),
                        };
                        let conflicted = counts.get(p.name.as_slice()).copied().unwrap_or(0) > 1;
                        let storage = if conflicted && p.visibility == Visibility::Private {
                            mangle(&decl_name, &p.name)
                        } else {
                            p.name.clone()
                        };
                        (php_key, storage, p.property_type.clone(), p.is_virtual, p.name.clone(), p.visibility, lower(&p.declaring_class))
                    }).collect()
                }
                None => Vec::new(),
            }
        };
        // Builtin throwable properties
        if let Some(base) = throwable_base(vm, class_lower) {
            if let Some((cls, name)) = unmangle(key) {
                let is_priv = matches!(name, b"string" | b"trace" | b"previous");
                let is_prot = matches!(name, b"message" | b"code" | b"file" | b"line");
                let user_decl = decl.iter().any(|d| d.4 == name);
                if !user_decl && (is_priv || is_prot) {
                    let matches = match cls {
                        None => true,
                        Some(b"*") => true,
                        Some(c) => lower(c) == lower(base) || lower(c) == class_lower,
                    };
                    if matches {
                        use goro_core::opcode::ParamType;
                        let ty = match name {
                            b"string" | b"file" => Some(ParamType::Simple(b"string".to_vec())),
                            b"line" => Some(ParamType::Simple(b"int".to_vec())),
                            b"trace" => Some(ParamType::Simple(b"array".to_vec())),
                            b"previous" => Some(ParamType::Nullable(Box::new(ParamType::Simple(b"Throwable".to_vec())))),
                            _ => None,
                        };
                        return Ok((name.to_vec(), ty, base.to_vec()));
                    }
                }
            }
        }
        if let Some(d) = decl.iter().find(|d| d.0 == key) {
            if d.3 {
                return self.virtual_prop(vm, class_lower, &d.4);
            }
            return Ok((d.1.clone(), d.2.clone(), canonical_class_name(vm, &d.6, &d.6)));
        }
        // Existing dynamic property with this exact key?
        if obj.borrow().has_property(key) && !decl.iter().any(|d| d.1 == key) {
            // is_property_visibility_changed may still remap it below.
        }
        // is_property_visibility_changed()
        if !decl.is_empty() {
            let (ucls, uname) = match unmangle(key) {
                Some(x) => x,
                None => return Err(()),
            };
            let class_display = canonical_class_name(vm, class_lower, class_lower);
            let candidate = match ucls {
                None => true,
                Some(c) => c == b"*" || lower(c) == lower(&class_display),
            };
            if candidate {
                // properties_info lookup: the class's own view of `uname`
                let found = decl.iter().find(|d| d.4 == uname && (d.6 == class_lower || d.5 != Visibility::Private))
                    .or_else(|| decl.iter().find(|d| d.4 == uname && d.6 == class_lower));
                if let Some(d) = found {
                    if d.3 {
                        return self.virtual_prop(vm, class_lower, &d.4);
                    }
                    return Ok((d.1.clone(), d.2.clone(), canonical_class_name(vm, &d.6, &d.6)));
                }
            }
        }
        // Dynamic property
        if obj.borrow().has_property(key) {
            return Ok((key.to_vec(), None, Vec::new()));
        }
        let (readonly, allow_dynamic, is_enum, display) = {
            let chain = class_chain(vm, class_lower);
            let ce = vm.classes.get(class_lower);
            let allow = class_lower == b"stdclass" || class_lower == lower(INCOMPLETE_CLASS)
                || chain.iter().any(|c| vm.classes.get(c).map(|ce| ce.allow_dynamic_properties).unwrap_or(false))
                || ce.is_none();
            (ce.map(|c| c.is_readonly).unwrap_or(false), allow, ce.map(|c| c.is_enum).unwrap_or(false),
             goro_core::value::display_class_name(&canonical_class_name(vm, class_lower, class_lower)))
        };
        let uname = unmangle(key).map(|(_, n)| n.to_vec()).unwrap_or_else(|| key.to_vec());
        if readonly || is_enum {
            throw(vm, b"Error", &format!("Cannot create dynamic property {}::${}", display, String::from_utf8_lossy(&uname)));
            return Err(());
        }
        if !allow_dynamic {
            vm.emit_deprecated_at(&format!("Creation of dynamic property {}::${} is deprecated", display, String::from_utf8_lossy(&uname)), self.line);
            if vm.current_exception.is_some() {
                return Err(());
            }
        }
        Ok((key.to_vec(), None, Vec::new()))
    }

    fn virtual_prop(&self, vm: &mut Vm, class_lower: &[u8], name: &[u8])
        -> Result<(Vec<u8>, Option<goro_core::opcode::ParamType>, Vec<u8>), ()>
    {
        let display = goro_core::value::display_class_name(&canonical_class_name(vm, class_lower, class_lower));
        self.warn(vm, &format!("Cannot unserialize value for virtual property {}::${}", display, String::from_utf8_lossy(name)));
        Err(())
    }

    fn nested_object(&mut self, vm: &mut Vm, p: &mut usize, obj: &Rc<RefCell<PhpObject>>, class_lower: &[u8], elements: i64) -> bool {
        if !self.depth_enter(vm) {
            return false;
        }
        let mut left = elements;
        let ok = loop {
            if left <= 0 {
                break true;
            }
            left -= 1;
            let key = match self.parse_key(vm, p) {
                Some(k) => k,
                None => break false,
            };
            let key: Vec<u8> = match key {
                Value::String(s) => s.as_bytes().to_vec(),
                Value::Long(n) => n.to_string().into_bytes(),
                _ => break false,
            };
            let (storage, ptype, decl_name) = match self.resolve_prop(vm, obj, class_lower, &key) {
                Ok(x) => x,
                Err(()) => break false,
            };
            obj.borrow_mut().set_property(storage.clone(), Value::Null);
            let slot = Slot::Prop(obj.clone(), storage.clone());
            if !self.unserialize(vm, p, &slot) {
                break false;
            }
            if let Some(pt) = ptype {
                let v = obj.borrow().get_property(&storage);
                let inner = v.deref();
                if !vm.value_matches_type_strict_pub(&inner, &pt) {
                    let display = if decl_name.is_empty() { goro_core::value::display_class_name(&obj.borrow().class_name) } else { goro_core::value::display_class_name(&decl_name) };
                    let prop_name = unmangle(&storage).map(|(_, n)| n.to_vec()).unwrap_or(storage.clone());
                    let msg = format!("Cannot assign {} to property {}::${} of type {}",
                        Vm::value_type_name(&inner), display, String::from_utf8_lossy(&prop_name), vm.param_type_name(&pt));
                    obj.borrow_mut().set_property(storage.clone(), Value::Undef);
                    throw(vm, b"TypeError", &msg);
                    break false;
                }
            }
            if left > 0 && !self.last_char_ok(p) {
                break false;
            }
        };
        self.depth_leave();
        ok
    }
}

/// zval_try_get_tmp_string-ish conversion for allowed_classes entries.
fn option_class_name(vm: &mut Vm, v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        Value::Object(_) => {
            let r = vm.call_object_method(v, b"__tostring", &[]);
            if vm.current_exception.is_some() {
                return None;
            }
            match r {
                Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
                _ => {
                    let cn = Vm::value_type_name(v);
                    throw(vm, b"Error", &format!("Object of class {} could not be converted to string", cn));
                    None
                }
            }
        }
        _ => None,
    }
}

/// php_unserialize_with_options(). Returns Err when an exception is pending.
pub fn php_unserialize_with_options(vm: &mut Vm, buf: &[u8], options: Option<&Value>, function_name: &str) -> Result<Value, ()> {
    if buf.is_empty() {
        return Ok(Value::False);
    }
    let line = vm.current_line;
    let d = unser_init(vm);
    let (prev_classes, prev_max, prev_cur) = {
        let dd = d.borrow();
        (dd.allowed_classes.clone(), dd.max_depth, dd.cur_depth)
    };
    let restore = |d: &Rc<RefCell<UnserData>>| {
        let mut dd = d.borrow_mut();
        dd.allowed_classes = prev_classes.clone();
        dd.max_depth = prev_max;
        dd.cur_depth = prev_cur;
    };
    if let Some(Value::Array(opts)) = options {
        let classes = opts.borrow().get_str(b"allowed_classes").map(|v| v.deref());
        let max_depth = opts.borrow().get_str(b"max_depth").map(|v| v.deref());
        let mut err: Option<(&[u8], String)> = None;
        if let Some(c) = &classes {
            match c {
                Value::True => {}
                Value::False => d.borrow_mut().allowed_classes = Some(Vec::new()),
                Value::Array(list) => {
                    let entries: Vec<Value> = list.borrow().values().map(|v| v.deref()).collect();
                    let mut names = Vec::new();
                    for e in entries {
                        if !matches!(e, Value::String(_) | Value::Object(_)) {
                            err = Some((b"TypeError", format!("{}(): Option \"allowed_classes\" must be an array of class names, {} given", function_name, Vm::value_type_name(&e))));
                            break;
                        }
                        let name = match option_class_name(vm, &e) {
                            Some(n) => n,
                            None => break,
                        };
                        if !is_valid_class_name(&name) {
                            // %s in the C message stops at a NUL byte
                            let shown = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
                            err = Some((b"ValueError", format!("{}(): Option \"allowed_classes\" must be an array of class names, \"{}\" given", function_name, String::from_utf8_lossy(shown))));
                            break;
                        }
                        names.push(lower(&name));
                    }
                    d.borrow_mut().allowed_classes = Some(names);
                }
                other => {
                    err = Some((b"TypeError", format!("{}(): Option \"allowed_classes\" must be of type array|bool, {} given", function_name, Vm::value_type_name(other))));
                }
            }
        }
        if err.is_none() && vm.current_exception.is_none() {
            if let Some(md) = &max_depth {
                match md {
                    Value::Long(n) if *n < 0 => {
                        err = Some((b"ValueError", format!("{}(): Option \"max_depth\" must be greater than or equal to 0", function_name)));
                    }
                    Value::Long(n) => {
                        let mut dd = d.borrow_mut();
                        dd.max_depth = *n;
                        dd.cur_depth = 0;
                    }
                    other => {
                        err = Some((b"TypeError", format!("{}(): Option \"max_depth\" must be of type int, {} given", function_name, Vm::value_type_name(other))));
                    }
                }
            }
        }
        if let Some((cls, msg)) = err {
            throw(vm, cls, &msg);
        }
        if vm.current_exception.is_some() {
            restore(&d);
            unser_destroy(vm, &d);
            return Err(());
        }
    }

    let tmp_idx = {
        let mut dd = d.borrow_mut();
        dd.tmps.push(Value::Null);
        dd.tmps.len() - 1
    };
    let mut parser = Parser { data: buf, d: d.clone(), line };
    let mut p = 0usize;
    let ok = parser.unserialize(vm, &mut p, &Slot::Tmp(tmp_idx));
    let mut result = if ok {
        if p < buf.len() && vm.current_exception.is_none() {
            vm.emit_warning_at(&format!("{}(): Extra data starting at offset {} of {} bytes", function_name, p, buf.len()), line);
        }
        d.borrow().tmps.get(tmp_idx).cloned().unwrap_or(Value::False)
    } else {
        if vm.current_exception.is_none() {
            vm.emit_warning_at(&format!("{}(): Error at offset {} of {} bytes", function_name, p, buf.len()), line);
        }
        Value::False
    };
    restore(&d);
    unser_destroy(vm, &d);
    vm.current_line = line;
    if let Value::Reference(r) = &result {
        let inner = r.borrow().clone();
        result = inner;
    }
    if vm.current_exception.is_some() {
        return Err(());
    }
    Ok(result)
}

pub fn unserialize_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data_val = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let data = match &data_val {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Array(_) | Value::Object(_) => {
            let msg = format!("unserialize(): Argument #1 ($data) must be of type string, {} given", Vm::value_type_name(&data_val));
            throw(vm, b"TypeError", &msg);
            return Err(exception_err(vm));
        }
        v => v.to_php_string().as_bytes().to_vec(),
    };
    let options = args.get(1).map(|v| v.deref());
    if let Some(o) = &options {
        if !matches!(o, Value::Array(_)) {
            let msg = format!("unserialize(): Argument #2 ($options) must be of type array, {} given", Vm::value_type_name(o));
            throw(vm, b"TypeError", &msg);
            return Err(exception_err(vm));
        }
    }
    vm.call_stack.push((
        "unserialize".to_string(),
        vm.current_file.clone(),
        vm.current_line,
        vec![Value::String(PhpString::from_bytes(&data))],
        false,
    ));
    let r = php_unserialize_with_options(vm, &data, options.as_ref(), "unserialize");
    vm.call_stack.pop();
    match r {
        Ok(v) => Ok(v),
        Err(()) => Err(exception_err(vm)),
    }
}

/// Hook used by core SPL code to unserialize one embedded value at `pos`
/// (shares the active var hash, like php_var_unserialize on a nested call).
pub(crate) fn unserialize_value_hook(vm: &mut Vm, data: &[u8], pos: &mut usize) -> Option<Value> {
    let line = vm.current_line;
    let d = unser_init(vm);
    let tmp_idx = {
        let mut dd = d.borrow_mut();
        dd.tmps.push(Value::Null);
        dd.tmps.len() - 1
    };
    let mut parser = Parser { data, d: d.clone(), line };
    let ok = parser.unserialize(vm, pos, &Slot::Tmp(tmp_idx));
    let result = if ok { d.borrow().tmps.get(tmp_idx).cloned() } else { None };
    unser_destroy(vm, &d);
    result.map(|v| match v {
        Value::Reference(r) => r.borrow().clone(),
        v => v,
    })
}
