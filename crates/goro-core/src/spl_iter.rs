//! SPL iterator classes: the "dual iterator" family (IteratorIterator,
//! FilterIterator, LimitIterator, CachingIterator, AppendIterator, ...) and
//! RecursiveIteratorIterator / RecursiveTreeIterator, following
//! php-src ext/spl/spl_iterators.c.
//!
//! Methods are dispatched per declaring class (like PHP's method tables):
//! `spl_iter_dispatch(class, method)` only handles methods that `class`
//! itself declares, so inherited methods resolve through the class chain.
//!
//! Hidden state:
//!   __spl_dit        dual iterator type (set by the constructor)
//!   __spl_inner      inner iterator (Object or Generator)
//!   __spl_has_cur / __spl_cur_data / __spl_cur_key / __spl_pos
//!   __spl_limit_offset / __spl_limit_count
//!   __spl_cit_flags / __spl_cache / __spl_cit_str / __spl_cit_children
//!   __spl_append_it  (ArrayIterator of appended iterators)
//!   __regex_pattern / __regex_mode / __regex_flags / __regex_preg_flags
//!   __spl_callback
//!   __spl_rii_its / __spl_rii_st / __spl_rii_mode / __spl_rii_flags /
//!   __spl_rii_maxd / __spl_rii_initer / __spl_rii_hooks
//!   __spl_rti_prefix / __spl_rti_postfix

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::PhpObject;
use crate::spl::{obj_class_lower, val_arr};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

// dual_it_type
const DIT_DEFAULT: i64 = 0; // Filter / RecursiveFilter / Parent
const DIT_LIMIT: i64 = 1;
const DIT_CACHING: i64 = 2;
const DIT_RCACHING: i64 = 3;
const DIT_ITERATOR: i64 = 4;
const DIT_NOREWIND: i64 = 5;
const DIT_INFINITE: i64 = 6;
const DIT_APPEND: i64 = 7;
const DIT_REGEX: i64 = 8;
const DIT_RREGEX: i64 = 9;
const DIT_CALLBACK: i64 = 10;
const DIT_RCALLBACK: i64 = 11;

const CIT_CALL_TOSTRING: i64 = 1;
const CIT_TOSTRING_USE_KEY: i64 = 2;
const CIT_TOSTRING_USE_CURRENT: i64 = 4;
const CIT_TOSTRING_USE_INNER: i64 = 8;
const CIT_CATCH_GET_CHILD: i64 = 16;
const CIT_FULL_CACHE: i64 = 256;
const CIT_PUBLIC: i64 = 0xFFFF;
const CIT_VALID: i64 = 0x10000;

const REGIT_USE_KEY: i64 = 1;
const REGIT_INVERTED: i64 = 2;

const RIT_LEAVES_ONLY: i64 = 0;
const RIT_SELF_FIRST: i64 = 1;
const RIT_CHILD_FIRST: i64 = 2;
const RIT_CATCH_GET_CHILD: i64 = 16;
const RTIT_BYPASS_CURRENT: i64 = 4;
const RTIT_BYPASS_KEY: i64 = 8;

// RecursiveIteratorIterator sub-iterator states
const RS_NEXT: i64 = 0;
const RS_TEST: i64 = 1;
const RS_SELF: i64 = 2;
const RS_CHILD: i64 = 3;
const RS_START: i64 = 4;

// Overridden RecursiveIteratorIterator hooks
const H_BEGIN_ITER: i64 = 1;
const H_END_ITER: i64 = 2;
const H_CALL_HAS: i64 = 4;
const H_CALL_GET: i64 = 8;
const H_BEGIN_CH: i64 = 16;
const H_END_CH: i64 = 32;
const H_NEXT_EL: i64 = 64;

/// Built-in classes implemented here (lowercase).
pub const ITER_CLASSES: &[&[u8]] = &[
    b"iteratoriterator", b"filteriterator", b"callbackfilteriterator",
    b"recursivecallbackfilteriterator", b"recursivefilteriterator", b"parentiterator",
    b"limititerator", b"cachingiterator", b"recursivecachingiterator",
    b"norewinditerator", b"appenditerator", b"infiniteiterator",
    b"regexiterator", b"recursiveregexiterator",
    b"recursiveiteratoriterator", b"recursivetreeiterator",
];

fn prop(obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> Value {
    obj.borrow().get_property(name)
}

fn set_prop(obj: &Rc<RefCell<PhpObject>>, name: &[u8], v: Value) {
    obj.borrow_mut().set_property(name.to_vec(), v);
}

fn lprop(obj: &Rc<RefCell<PhpObject>>, name: &[u8], default: i64) -> i64 {
    match prop(obj, name) {
        Value::Long(n) => n,
        _ => default,
    }
}

fn bprop(obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> bool {
    matches!(prop(obj, name), Value::True)
}

fn bv(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

fn sv(s: &str) -> Value {
    Value::String(PhpString::from_string(s.to_string()))
}

impl Vm {
    // ------------------------------------------------------------------
    // Inner iterator protocol (zend_object_iterator funcs)
    // ------------------------------------------------------------------

    fn exc(&self) -> bool {
        self.current_exception.is_some()
    }

    fn gen_prime(&mut self, g: &Rc<RefCell<crate::generator::PhpGenerator>>) {
        let created = matches!(g.try_borrow().map(|b| b.state.clone()), Ok(crate::generator::GeneratorState::Created));
        if created {
            if let Ok(mut b) = g.try_borrow_mut() {
                let _ = b.resume(self);
            }
        }
    }

    pub(crate) fn it_rewind(&mut self, it: &Value) {
        match it {
            Value::Object(_) => {
                self.spl_call_method(it, b"rewind", &[]);
            }
            Value::Generator(g) => {
                let (created, advanced) = match g.try_borrow() {
                    Ok(b) => (b.state == crate::generator::GeneratorState::Created, b.advanced_past_first),
                    Err(_) => (false, false),
                };
                if created {
                    self.gen_prime(g);
                } else if advanced {
                    self.spl_throw(b"Exception", "Cannot rewind a generator that was already run");
                }
            }
            _ => {}
        }
    }

    pub(crate) fn it_valid(&mut self, it: &Value) -> bool {
        match it {
            Value::Object(_) => self.spl_call_method(it, b"valid", &[]).map(|v| v.is_truthy()).unwrap_or(false),
            Value::Generator(g) => {
                self.gen_prime(g);
                !matches!(g.try_borrow().map(|b| b.state.clone()), Ok(crate::generator::GeneratorState::Completed))
            }
            _ => false,
        }
    }

    pub(crate) fn it_current(&mut self, it: &Value) -> Value {
        match it {
            Value::Object(_) => self.spl_call_method(it, b"current", &[]).unwrap_or(Value::Null),
            Value::Generator(g) => {
                self.gen_prime(g);
                g.try_borrow().map(|b| b.current_value.clone()).unwrap_or(Value::Null)
            }
            _ => Value::Null,
        }
    }

    pub(crate) fn it_key(&mut self, it: &Value) -> Value {
        match it {
            Value::Object(_) => self.spl_call_method(it, b"key", &[]).unwrap_or(Value::Null),
            Value::Generator(g) => {
                self.gen_prime(g);
                g.try_borrow().map(|b| b.current_key.clone()).unwrap_or(Value::Null)
            }
            _ => Value::Null,
        }
    }

    pub(crate) fn it_next(&mut self, it: &Value) {
        match it {
            Value::Object(_) => {
                self.spl_call_method(it, b"next", &[]);
            }
            Value::Generator(g) => {
                self.gen_prime(g);
                if let Ok(mut b) = g.try_borrow_mut() {
                    if b.state != crate::generator::GeneratorState::Completed {
                        b.advanced_past_first = true;
                        b.write_send_value();
                        let _ = b.resume(self);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn obj_instanceof(&self, v: &Value, iface_lower: &[u8]) -> bool {
        match v {
            Value::Object(o) => {
                // Same walk as the `instanceof` operator.
                let cl = obj_class_lower(o);
                if cl == iface_lower {
                    return true;
                }
                let mut cur = cl.clone();
                for _ in 0..64 {
                    match self.classes.get(&cur) {
                        Some(cd) => {
                            if cd.interfaces.iter().any(|i| i.eq_ignore_ascii_case(iface_lower)) {
                                return true;
                            }
                            match &cd.parent {
                                Some(p) => {
                                    let pl: Vec<u8> = p.iter().map(|b| b.to_ascii_lowercase()).collect();
                                    if pl == iface_lower {
                                        return true;
                                    }
                                    cur = pl;
                                }
                                None => break,
                            }
                        }
                        None => {
                            if crate::vm::is_builtin_subclass(&cur, iface_lower) {
                                return true;
                            }
                            break;
                        }
                    }
                }
                self.class_implements_interface(&cl, iface_lower)
            }
            Value::Generator(_) => matches!(iface_lower, b"traversable" | b"iterator" | b"generator"),
            _ => false,
        }
    }

    /// Resolve IteratorAggregate::getIterator() chains (zend_user_it /
    /// ce->get_iterator) to the object or generator actually iterated.
    /// Throws LogicException like spl_get_iterator_from_aggregate when the
    /// result is not Traversable.
    pub fn spl_resolve_traversable(&mut self, v: &Value) -> Value {
        let mut cur = v.clone();
        for _ in 0..32 {
            if !matches!(cur, Value::Object(_)) {
                return cur;
            }
            if self.obj_instanceof(&cur, b"iterator") || !self.obj_instanceof(&cur, b"iteratoraggregate") {
                return cur;
            }
            let cls = match &cur {
                Value::Object(o) => crate::value::display_class_name(&o.borrow().class_name).to_string(),
                _ => String::new(),
            };
            let r = self.spl_call_method_at(&cur, b"getIterator", &[], true).unwrap_or(Value::Null);
            if self.exc() {
                return Value::Null;
            }
            if !self.obj_instanceof(&r, b"traversable") {
                let msg = format!("{}::getIterator() must return an object that implements Traversable", cls);
                self.spl_throw(b"Exception", &msg);
                return Value::Null;
            }
            cur = r;
        }
        cur
    }

    /// zval_get_string() with the array warning.
    fn spl_zval_get_string(&mut self, v: &Value) -> PhpString {
        let v = v.deref();
        if matches!(v, Value::Array(_)) {
            self.emit_warning("Array to string conversion");
            return PhpString::from_bytes(b"Array");
        }
        // convert_to_string(): objects without __toString() throw.
        self.value_to_string_checked(&v).unwrap_or_else(|_| PhpString::empty())
    }

    // ------------------------------------------------------------------
    // Dual iterator core
    // ------------------------------------------------------------------

    fn dit_type(obj: &Rc<RefCell<PhpObject>>) -> Option<i64> {
        match prop(obj, b"__spl_dit") {
            Value::Long(n) => Some(n),
            _ => None,
        }
    }

    fn dit_check(&mut self, obj: &Rc<RefCell<PhpObject>>) -> Option<i64> {
        match Self::dit_type(obj) {
            Some(t) => Some(t),
            None => {
                self.spl_throw(b"Error", "The object is in an invalid state as the parent constructor was not called");
                None
            }
        }
    }

    fn dit_inner(obj: &Rc<RefCell<PhpObject>>) -> Value {
        prop(obj, b"__spl_inner")
    }

    fn dit_has_cur(obj: &Rc<RefCell<PhpObject>>) -> bool {
        bprop(obj, b"__spl_has_cur")
    }

    fn dit_free(obj: &Rc<RefCell<PhpObject>>) {
        let t = Self::dit_type(obj).unwrap_or(-1);
        let mut ob = obj.borrow_mut();
        ob.set_property(b"__spl_has_cur".to_vec(), Value::False);
        ob.set_property(b"__spl_cur_data".to_vec(), Value::Null);
        ob.set_property(b"__spl_has_key".to_vec(), Value::False);
        ob.set_property(b"__spl_cur_key".to_vec(), Value::Null);
        if t == DIT_CACHING || t == DIT_RCACHING {
            ob.set_property(b"__spl_cit_str".to_vec(), Value::Null);
            ob.set_property(b"__spl_cit_children".to_vec(), Value::Null);
        }
    }

    fn dit_rewind(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        Self::dit_free(obj);
        set_prop(obj, b"__spl_pos", Value::Long(0));
        let inner = Self::dit_inner(obj);
        if !matches!(inner, Value::Null | Value::Undef) {
            self.it_rewind(&inner);
        }
    }

    fn dit_valid(&mut self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let inner = Self::dit_inner(obj);
        if matches!(inner, Value::Null | Value::Undef) {
            return false;
        }
        self.it_valid(&inner)
    }

    fn dit_fetch(&mut self, obj: &Rc<RefCell<PhpObject>>, check_more: bool) -> bool {
        Self::dit_free(obj);
        if !check_more || self.dit_valid(obj) {
            if self.exc() {
                return false;
            }
            let inner = Self::dit_inner(obj);
            let data = self.it_current(&inner);
            if self.exc() {
                return false;
            }
            {
                let mut ob = obj.borrow_mut();
                ob.set_property(b"__spl_cur_data".to_vec(), data.deref());
                ob.set_property(b"__spl_has_cur".to_vec(), Value::True);
            }
            let key = self.it_key(&inner);
            if !self.exc() {
                let mut ob = obj.borrow_mut();
                ob.set_property(b"__spl_cur_key".to_vec(), key.deref());
                ob.set_property(b"__spl_has_key".to_vec(), Value::True);
            }
            return !self.exc();
        }
        false
    }

    fn dit_next(&mut self, obj: &Rc<RefCell<PhpObject>>, do_free: bool) {
        if do_free {
            Self::dit_free(obj);
        }
        let inner = Self::dit_inner(obj);
        if matches!(inner, Value::Null | Value::Undef) {
            if !do_free {
                self.spl_throw(b"Error", "The inner constructor wasn't initialized with an iterator instance");
            }
            return;
        }
        self.it_next(&inner);
        let p = lprop(obj, b"__spl_pos", 0);
        set_prop(obj, b"__spl_pos", Value::Long(p + 1));
    }

    fn dit_cur(obj: &Rc<RefCell<PhpObject>>) -> Value {
        if Self::dit_has_cur(obj) { prop(obj, b"__spl_cur_data") } else { Value::Null }
    }

    fn dit_key(obj: &Rc<RefCell<PhpObject>>) -> Value {
        if bprop(obj, b"__spl_has_key") { prop(obj, b"__spl_cur_key") } else { Value::Null }
    }

    // Filter
    fn filter_fetch(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        let this = Value::Object(obj.clone());
        while self.dit_fetch(obj, true) {
            let r = self.spl_call_method(&this, b"accept", &[]);
            if let Some(r) = r {
                if !self.exc() && r.is_truthy() {
                    return;
                }
            }
            if self.exc() {
                return;
            }
            let inner = Self::dit_inner(obj);
            self.it_next(&inner);
        }
        Self::dit_free(obj);
    }

    // Limit
    fn limit_rel(obj: &Rc<RefCell<PhpObject>>, pos: i64) -> i64 {
        pos - lprop(obj, b"__spl_limit_offset", 0)
    }

    fn limit_valid(&mut self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let count = lprop(obj, b"__spl_limit_count", -1);
        let pos = lprop(obj, b"__spl_pos", 0);
        if count != -1 && Self::limit_rel(obj, pos) >= count {
            return false;
        }
        self.dit_valid(obj)
    }

    fn limit_seek(&mut self, obj: &Rc<RefCell<PhpObject>>, pos: i64) {
        Self::dit_free(obj);
        let offset = lprop(obj, b"__spl_limit_offset", 0);
        let count = lprop(obj, b"__spl_limit_count", -1);
        if pos < offset {
            let msg = format!("Cannot seek to {} which is below the offset {}", pos, offset);
            self.spl_throw(b"OutOfBoundsException", &msg);
            return;
        }
        if Self::limit_rel(obj, pos) >= count && count != -1 {
            let msg = format!("Cannot seek to {} which is behind offset {} plus count {}", pos, offset, count);
            self.spl_throw(b"OutOfBoundsException", &msg);
            return;
        }
        let inner = Self::dit_inner(obj);
        let cur = lprop(obj, b"__spl_pos", 0);
        if pos != cur && self.obj_instanceof(&inner, b"seekableiterator") {
            Self::dit_free(obj);
            self.spl_call_method(&inner, b"seek", &[Value::Long(pos)]);
            if !self.exc() {
                set_prop(obj, b"__spl_pos", Value::Long(pos));
                if self.limit_valid(obj) {
                    self.dit_fetch(obj, false);
                }
            }
        } else {
            if pos < cur {
                self.dit_rewind(obj);
            }
            while pos > lprop(obj, b"__spl_pos", 0) && self.dit_valid(obj) {
                if self.exc() {
                    return;
                }
                self.dit_next(obj, true);
                if self.exc() {
                    return;
                }
            }
            if self.dit_valid(obj) {
                self.dit_fetch(obj, true);
            }
        }
    }

    // Caching
    fn cit_flags(obj: &Rc<RefCell<PhpObject>>) -> i64 {
        lprop(obj, b"__spl_cit_flags", CIT_CALL_TOSTRING)
    }

    fn cit_set_flags(obj: &Rc<RefCell<PhpObject>>, f: i64) {
        set_prop(obj, b"__spl_cit_flags", Value::Long(f));
    }

    fn cit_next(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        if self.dit_fetch(obj, true) {
            let mut flags = Self::cit_flags(obj) | CIT_VALID;
            Self::cit_set_flags(obj, flags);
            if flags & CIT_FULL_CACHE != 0 {
                let key = Self::dit_key(obj);
                let data = Self::dit_cur(obj);
                if let Value::Array(c) = prop(obj, b"__spl_cache") {
                    let k = Self::value_to_array_key(key);
                    c.borrow_mut().set(k, data);
                }
            }
            if Self::dit_type(obj) == Some(DIT_RCACHING) {
                let inner = Self::dit_inner(obj);
                let r = self.spl_call_method(&inner, b"hasChildren", &[]);
                if self.exc() {
                    if flags & CIT_CATCH_GET_CHILD != 0 {
                        self.current_exception = None;
                    } else {
                        return;
                    }
                } else if r.map(|v| v.is_truthy()).unwrap_or(false) {
                    let ch = self.spl_call_method(&inner, b"getChildren", &[]).unwrap_or(Value::Null);
                    if self.exc() {
                        if flags & CIT_CATCH_GET_CHILD != 0 {
                            self.current_exception = None;
                        } else {
                            return;
                        }
                    } else {
                        let child = self.spl_instantiate(b"RecursiveCachingIterator");
                        let ok = self.dit_construct(&child, &[ch, Value::Long(flags & CIT_PUBLIC)]);
                        if !ok {
                            if flags & CIT_CATCH_GET_CHILD != 0 {
                                self.current_exception = None;
                            } else {
                                return;
                            }
                        } else {
                            set_prop(obj, b"__spl_cit_children", Value::Object(child));
                        }
                    }
                }
            }
            flags = Self::cit_flags(obj);
            if flags & (CIT_TOSTRING_USE_INNER | CIT_CALL_TOSTRING) != 0 {
                let s = if flags & CIT_TOSTRING_USE_INNER != 0 {
                    let inner = Self::dit_inner(obj);
                    self.spl_zval_get_string(&inner)
                } else {
                    let d = Self::dit_cur(obj);
                    self.spl_zval_get_string(&d)
                };
                set_prop(obj, b"__spl_cit_str", Value::String(s));
            }
            self.dit_next(obj, false);
        } else {
            let f = Self::cit_flags(obj) & !CIT_VALID;
            Self::cit_set_flags(obj, f);
        }
    }

    fn cit_rewind(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        self.dit_rewind(obj);
        set_prop(obj, b"__spl_cache", val_arr(PhpArray::new()));
        self.cit_next(obj);
    }

    fn cit_check_flags(flags: i64) -> bool {
        let mut cnt = 0;
        for f in [CIT_CALL_TOSTRING, CIT_TOSTRING_USE_KEY, CIT_TOSTRING_USE_CURRENT, CIT_TOSTRING_USE_INNER] {
            if flags & f != 0 {
                cnt += 1;
            }
        }
        cnt <= 1
    }

    fn cit_full_cache_check(&mut self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        if Self::cit_flags(obj) & CIT_FULL_CACHE == 0 {
            let msg = format!(
                "{} does not use a full cache (see CachingIterator::__construct)",
                crate::value::display_class_name(&obj.borrow().class_name)
            );
            self.spl_throw(b"BadMethodCallException", &msg);
            return false;
        }
        true
    }

    // Append
    fn append_it(obj: &Rc<RefCell<PhpObject>>) -> Value {
        prop(obj, b"__spl_append_it")
    }

    fn append_next_iterator(&mut self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        Self::dit_free(obj);
        set_prop(obj, b"__spl_inner", Value::Null);
        let ai = Self::append_it(obj);
        if self.it_valid(&ai) {
            let it = self.it_current(&ai);
            set_prop(obj, b"__spl_inner", it);
            self.dit_rewind(obj);
            true
        } else {
            false
        }
    }

    fn append_fetch(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        while !self.dit_valid(obj) {
            if self.exc() {
                return;
            }
            let ai = Self::append_it(obj);
            self.it_next(&ai);
            if !self.append_next_iterator(obj) {
                return;
            }
        }
        self.dit_fetch(obj, false);
    }

    // ------------------------------------------------------------------
    // Construction
    // ------------------------------------------------------------------

    fn dit_type_for(base: &[u8]) -> i64 {
        match base {
            b"limititerator" => DIT_LIMIT,
            b"cachingiterator" => DIT_CACHING,
            b"recursivecachingiterator" => DIT_RCACHING,
            b"iteratoriterator" => DIT_ITERATOR,
            b"norewinditerator" => DIT_NOREWIND,
            b"infiniteiterator" => DIT_INFINITE,
            b"appenditerator" => DIT_APPEND,
            b"regexiterator" => DIT_REGEX,
            b"recursiveregexiterator" => DIT_RREGEX,
            b"callbackfilteriterator" => DIT_CALLBACK,
            b"recursivecallbackfilteriterator" => DIT_RCALLBACK,
            _ => DIT_DEFAULT,
        }
    }

    pub(crate) fn arity_check(&mut self, func: &str, given: usize, min: usize, max: usize) -> bool {
        if given < min || given > max {
            let (word, n) = if min == max {
                ("exactly", min)
            } else if given < min {
                ("at least", min)
            } else {
                ("at most", max)
            };
            let msg = format!("{}() expects {} {} argument{}, {} given", func, word, n, if n == 1 { "" } else { "s" }, given);
            self.spl_throw(b"ArgumentCountError", &msg);
            return false;
        }
        true
    }

    /// spl_dual_it_construct(). `args` excludes $this.
    pub(crate) fn dit_construct(&mut self, obj: &Rc<RefCell<PhpObject>>, args: &[Value]) -> bool {
        let cl = obj_class_lower(obj);
        let base = match self.spl_find_base(&cl, ITER_CLASSES) {
            Some(b) => b,
            None => return true,
        };
        if base == b"recursiveiteratoriterator" || base == b"recursivetreeiterator" {
            return self.rii_construct(obj, base, args);
        }
        let base_disp = self.builtin_canonical_name(base);
        let func = format!("{}::__construct", base_disp);
        let dit = Self::dit_type_for(base);
        if Self::dit_type(obj).is_some() {
            let msg = format!("{}::getIterator() must be called exactly once per instance", base_disp);
            self.spl_throw(b"BadMethodCallException", &msg);
            return false;
        }
        let (min, max) = match dit {
            DIT_APPEND => (0, 0),
            DIT_LIMIT => (1, 3),
            DIT_CACHING | DIT_RCACHING | DIT_ITERATOR => (1, 2),
            DIT_REGEX | DIT_RREGEX => (2, 5),
            DIT_CALLBACK | DIT_RCALLBACK => (2, 2),
            _ => (1, 1),
        };
        if !self.arity_check(&func, args.len(), min, max) {
            return false;
        }
        if dit == DIT_APPEND {
            let ai = self.spl_instantiate(b"ArrayIterator");
            {
                let mut ob = ai.borrow_mut();
                ob.set_property(b"__spl_array".to_vec(), val_arr(PhpArray::new()));
                ob.set_property(b"__spl_pos".to_vec(), Value::Long(0));
                ob.set_property(b"__spl_flags".to_vec(), Value::Long(0));
            }
            set_prop(obj, b"__spl_append_it", Value::Object(ai));
            set_prop(obj, b"__spl_dit", Value::Long(DIT_APPEND));
            set_prop(obj, b"__spl_pos", Value::Long(0));
            Self::dit_free(obj);
            return true;
        }
        // Inner iterator type check.
        let inner_iface: &[u8] = match dit {
            DIT_ITERATOR => b"traversable",
            DIT_RCACHING | DIT_RREGEX | DIT_RCALLBACK => b"recursiveiterator",
            DIT_DEFAULT if base != b"filteriterator" => b"recursiveiterator",
            _ => b"iterator",
        };
        let mut inner = args[0].deref();
        if !self.obj_instanceof(&inner, inner_iface) {
            let msg = format!(
                "{}(): Argument #1 ($iterator) must be of type {}, {} given",
                func,
                match inner_iface {
                    b"traversable" => "Traversable",
                    b"recursiveiterator" => "RecursiveIterator",
                    _ => "Iterator",
                },
                Self::value_type_name(&inner)
            );
            self.spl_throw(b"TypeError", &msg);
            return false;
        }
        match dit {
            DIT_LIMIT => {
                let offset = match args.get(1) {
                    Some(v) => match self.spl_arg_long(&func, 2, "offset", v) {
                        Some(n) => n,
                        None => return false,
                    },
                    None => 0,
                };
                let count = match args.get(2) {
                    Some(v) => match self.spl_arg_long(&func, 3, "limit", v) {
                        Some(n) => n,
                        None => return false,
                    },
                    None => -1,
                };
                if offset < 0 {
                    let msg = format!("{}(): Argument #2 ($offset) must be greater than or equal to 0", func);
                    self.spl_throw(b"ValueError", &msg);
                    return false;
                }
                if count < -1 {
                    let msg = format!("{}(): Argument #3 ($limit) must be greater than or equal to -1", func);
                    self.spl_throw(b"ValueError", &msg);
                    return false;
                }
                set_prop(obj, b"__spl_limit_offset", Value::Long(offset));
                set_prop(obj, b"__spl_limit_count", Value::Long(count));
            }
            DIT_CACHING | DIT_RCACHING => {
                let flags = match args.get(1) {
                    Some(v) => match self.spl_arg_long(&func, 2, "flags", v) {
                        Some(n) => n,
                        None => return false,
                    },
                    None => CIT_CALL_TOSTRING,
                };
                if !Self::cit_check_flags(flags) {
                    let msg = format!(
                        "{}(): Argument #2 ($flags) must contain only one of CachingIterator::CALL_TOSTRING, CachingIterator::TOSTRING_USE_KEY, CachingIterator::TOSTRING_USE_CURRENT, or CachingIterator::TOSTRING_USE_INNER",
                        func
                    );
                    self.spl_throw(b"ValueError", &msg);
                    return false;
                }
                Self::cit_set_flags(obj, flags & CIT_PUBLIC);
                set_prop(obj, b"__spl_cache", val_arr(PhpArray::new()));
            }
            DIT_ITERATOR => {
                let is_iter = self.obj_instanceof(&inner, b"iterator");
                if !is_iter {
                    if let Some(cv) = args.get(1) {
                        if !matches!(cv.deref(), Value::Null) {
                            let cname = cv.to_php_string();
                            let cname_lower: Vec<u8> = cname.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                            let known = self.classes.contains_key(&cname_lower) || self.is_known_builtin_class(&cname_lower);
                            if !known || !self.obj_instanceof(&inner, &cname_lower) {
                                self.spl_throw(
                                    b"LogicException",
                                    "Class to downcast to not found or not base class or does not implement Traversable",
                                );
                                return false;
                            }
                        }
                    }
                    if self.obj_instanceof(&inner, b"iteratoraggregate") {
                        let agg_cls = match &inner {
                            Value::Object(o) => crate::value::display_class_name(&o.borrow().class_name).to_string(),
                            _ => String::new(),
                        };
                        let r = self.spl_call_method(&inner, b"getIterator", &[]);
                        if self.exc() {
                            return false;
                        }
                        let r = r.unwrap_or(Value::Null);
                        if !self.obj_instanceof(&r, b"traversable") {
                            let msg = format!("{}::getIterator() must return an object that implements Traversable", agg_cls);
                            self.spl_throw(b"LogicException", &msg);
                            return false;
                        }
                        inner = r;
                    }
                }
            }
            DIT_REGEX | DIT_RREGEX => {
                let pattern = match self.spl_arg_string(&func, 2, "pattern", &args[1]) {
                    Some(s) => s,
                    None => return false,
                };
                let mode = match args.get(2) {
                    Some(v) => match self.spl_arg_long(&func, 3, "mode", v) {
                        Some(n) => n,
                        None => return false,
                    },
                    None => 0,
                };
                let flags = match args.get(3) {
                    Some(v) => match self.spl_arg_long(&func, 4, "flags", v) {
                        Some(n) => n,
                        None => return false,
                    },
                    None => 0,
                };
                let preg_flags = match args.get(4) {
                    Some(v) => match self.spl_arg_long(&func, 5, "pregFlags", v) {
                        Some(n) => n,
                        None => return false,
                    },
                    None => 0,
                };
                if !(0..5).contains(&mode) {
                    let msg = format!(
                        "{}(): Argument #3 ($mode) must be RegexIterator::MATCH, RegexIterator::GET_MATCH, RegexIterator::ALL_MATCHES, RegexIterator::SPLIT, or RegexIterator::REPLACE",
                        func
                    );
                    self.spl_throw(b"ValueError", &msg);
                    return false;
                }
                // Compile check: warnings become InvalidArgumentException.
                let prev = self.warning_capture.replace(Vec::new());
                self.spl_call_builtin(b"preg_match", &[Value::String(pattern.clone()), Value::String(PhpString::empty())]);
                let captured = std::mem::replace(&mut self.warning_capture, prev).unwrap_or_default();
                if let Some(w) = captured.first() {
                    let msg = match w.find("(): ") {
                        Some(i) => format!("{}(): {}", func, &w[i + 4..]),
                        None => w.clone(),
                    };
                    self.spl_throw(b"InvalidArgumentException", &msg);
                    return false;
                }
                let mut ob = obj.borrow_mut();
                ob.set_property(b"__regex_pattern".to_vec(), Value::String(pattern));
                ob.set_property(b"__regex_mode".to_vec(), Value::Long(mode));
                ob.set_property(b"__regex_flags".to_vec(), Value::Long(flags));
                ob.set_property(b"__regex_preg_flags".to_vec(), Value::Long(preg_flags));
                if !ob.has_property(b"replacement") {
                    ob.set_property(b"replacement".to_vec(), Value::Null);
                }
            }
            DIT_CALLBACK | DIT_RCALLBACK => {
                let cb = args[1].deref();
                let detail: Option<String> = match &cb {
                    Value::Null | Value::True | Value::False | Value::Long(_) | Value::Double(_) => {
                        Some("no array or string given".to_string())
                    }
                    Value::Array(a) if a.borrow().len() != 2 => Some("array callback must have exactly two members".to_string()),
                    Value::String(s) => {
                        let b = s.as_bytes();
                        let lower: Vec<u8> = b.iter().map(|c| c.to_ascii_lowercase()).collect();
                        let is_closure_name = b.starts_with(b"__closure_") || b.starts_with(b"__arrow_") || b.starts_with(b"__bound_closure_");
                        if is_closure_name || b.windows(2).any(|w| w == b"::") || self.functions.contains_key(&lower) || self.user_functions_contains(&lower) {
                            None
                        } else {
                            Some(format!("function \"{}\" not found or invalid function name", s.to_string_lossy()))
                        }
                    }
                    _ => None,
                };
                if let Some(d) = detail {
                    let msg = format!("{}(): Argument #2 ($callback) must be a valid callback, {}", func, d);
                    self.spl_throw(b"TypeError", &msg);
                    return false;
                }
                set_prop(obj, b"__spl_callback", cb);
            }
            _ => {}
        }
        set_prop(obj, b"__spl_inner", inner);
        set_prop(obj, b"__spl_dit", Value::Long(dit));
        set_prop(obj, b"__spl_pos", Value::Long(0));
        Self::dit_free(obj);
        true
    }

    fn user_functions_contains(&self, lower: &[u8]) -> bool {
        self.user_functions.contains_key(lower)
    }

    // ------------------------------------------------------------------
    // Dual iterator method tables
    // ------------------------------------------------------------------

    /// Whether `class_lower` is one of the classes implemented here.
    pub(crate) fn spl_iter_class(class_lower: &[u8]) -> bool {
        ITER_CLASSES.contains(&class_lower)
    }

    /// Dispatch `method` as declared by `class_lower` (one of ITER_CLASSES).
    /// Returns None when that class does not declare the method.
    pub(crate) fn spl_iter_dispatch(
        &mut self,
        class_lower: &[u8],
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: &[Value],
    ) -> Option<Value> {
        if !Self::spl_iter_class(class_lower) {
            return None;
        }
        if method == b"__construct" {
            // parent::__construct() from a userland constructor (the
            // __spl:: call path already pushed a frame).
            let has_frame = self.call_stack.last().map(|f| f.0.ends_with("->__construct")).unwrap_or(false);
            if !has_frame {
                self.spl_push_ctor_frame(obj, args);
            }
            self.dit_construct(obj, args);
            if !has_frame {
                self.call_stack.pop();
            }
            return Some(Value::Null);
        }
        if crate::spl_stubs::spl_stub_method(class_lower, method).is_none() && method != b"__tostring" {
            return None;
        }
        if class_lower == b"recursiveiteratoriterator" || class_lower == b"recursivetreeiterator" {
            return self.rii_dispatch(class_lower, method, obj, args);
        }
        let t = match self.dit_check(obj) {
            Some(t) => t,
            None => return Some(Value::Null),
        };
        let r = match (class_lower, method) {
            (b"iteratoriterator", b"getinneriterator") => Self::dit_inner(obj),
            (b"iteratoriterator", b"rewind") => {
                self.dit_rewind(obj);
                if !self.exc() {
                    self.dit_fetch(obj, true);
                }
                Value::Null
            }
            (b"iteratoriterator", b"valid") | (b"appenditerator", b"valid") => bv(Self::dit_has_cur(obj)),
            (b"iteratoriterator", b"key") => Self::dit_key(obj),
            (b"iteratoriterator", b"current") => Self::dit_cur(obj),
            (b"iteratoriterator", b"next") => {
                self.dit_next(obj, true);
                if !self.exc() {
                    self.dit_fetch(obj, true);
                }
                Value::Null
            }
            (b"filteriterator", b"rewind") => {
                self.dit_rewind(obj);
                if !self.exc() {
                    self.filter_fetch(obj);
                }
                Value::Null
            }
            (b"filteriterator", b"next") => {
                self.dit_next(obj, true);
                if !self.exc() {
                    self.filter_fetch(obj);
                }
                Value::Null
            }
            (b"callbackfilteriterator", b"accept") => {
                if !Self::dit_has_cur(obj) || !bprop(obj, b"__spl_has_key") {
                    return Some(Value::False);
                }
                let cb = prop(obj, b"__spl_callback");
                let data = Self::dit_cur(obj);
                let key = Self::dit_key(obj);
                let inner = Self::dit_inner(obj);
                match self.call_callback(&cb, &[data, key, inner]) {
                    Ok(v) => v.deref(),
                    Err(_) => Value::False,
                }
            }
            (b"recursivefilteriterator", b"haschildren")
            | (b"recursivecallbackfilteriterator", b"haschildren")
            | (b"parentiterator", b"accept")
            | (b"recursiveregexiterator", b"haschildren") => {
                let inner = Self::dit_inner(obj);
                self.spl_call_method(&inner, b"hasChildren", &[]).unwrap_or(Value::Null)
            }
            (b"recursivefilteriterator", b"getchildren") => {
                let inner = Self::dit_inner(obj);
                let ch = self.spl_call_method(&inner, b"getChildren", &[]);
                if self.exc() {
                    return Some(Value::Null);
                }
                let ch = ch.unwrap_or(Value::Null);
                return Some(self.spl_new_same_class(obj, &[ch]));
            }
            (b"recursivecallbackfilteriterator", b"getchildren") => {
                let inner = Self::dit_inner(obj);
                let ch = self.spl_call_method(&inner, b"getChildren", &[]);
                if self.exc() {
                    return Some(Value::Null);
                }
                let ch = ch.unwrap_or(Value::Null);
                let cb = prop(obj, b"__spl_callback");
                return Some(self.spl_new_same_class(obj, &[ch, cb]));
            }
            (b"recursiveregexiterator", b"getchildren") => {
                let inner = Self::dit_inner(obj);
                let ch = self.spl_call_method(&inner, b"getChildren", &[]);
                if self.exc() {
                    return Some(Value::Null);
                }
                let ch = ch.unwrap_or(Value::Null);
                let a = [
                    ch,
                    prop(obj, b"__regex_pattern"),
                    prop(obj, b"__regex_mode"),
                    prop(obj, b"__regex_flags"),
                    prop(obj, b"__regex_preg_flags"),
                ];
                return Some(self.spl_new_same_class(obj, &a));
            }
            (b"limititerator", b"rewind") => {
                self.dit_rewind(obj);
                if !self.exc() {
                    let off = lprop(obj, b"__spl_limit_offset", 0);
                    self.limit_seek(obj, off);
                }
                Value::Null
            }
            (b"limititerator", b"valid") => {
                let count = lprop(obj, b"__spl_limit_count", -1);
                let pos = lprop(obj, b"__spl_pos", 0);
                bv((count == -1 || Self::limit_rel(obj, pos) < count) && Self::dit_has_cur(obj))
            }
            (b"limititerator", b"next") => {
                self.dit_next(obj, true);
                if self.exc() {
                    return Some(Value::Null);
                }
                let count = lprop(obj, b"__spl_limit_count", -1);
                let pos = lprop(obj, b"__spl_pos", 0);
                if count == -1 || Self::limit_rel(obj, pos) < count {
                    self.dit_fetch(obj, true);
                }
                Value::Null
            }
            (b"limititerator", b"seek") => {
                let pos = match args.first() {
                    Some(v) => match self.spl_arg_long("LimitIterator::seek", 1, "offset", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                self.limit_seek(obj, pos);
                Value::Long(lprop(obj, b"__spl_pos", 0))
            }
            (b"limititerator", b"getposition") => Value::Long(lprop(obj, b"__spl_pos", 0)),
            (b"cachingiterator", b"rewind") => {
                self.cit_rewind(obj);
                Value::Null
            }
            (b"cachingiterator", b"valid") => bv(Self::cit_flags(obj) & CIT_VALID != 0),
            (b"cachingiterator", b"next") => {
                self.cit_next(obj);
                Value::Null
            }
            (b"cachingiterator", b"hasnext") => bv(self.dit_valid(obj)),
            (b"cachingiterator", b"__tostring") => {
                let flags = Self::cit_flags(obj);
                if flags & (CIT_CALL_TOSTRING | CIT_TOSTRING_USE_KEY | CIT_TOSTRING_USE_CURRENT | CIT_TOSTRING_USE_INNER) == 0 {
                    let msg = format!(
                        "{} does not fetch string value (see CachingIterator::__construct)",
                        crate::value::display_class_name(&obj.borrow().class_name)
                    );
                    return self.spl_throw(b"BadMethodCallException", &msg);
                }
                if flags & CIT_TOSTRING_USE_KEY != 0 {
                    let k = Self::dit_key(obj);
                    return Some(Value::String(self.spl_zval_get_string(&k)));
                } else if flags & CIT_TOSTRING_USE_CURRENT != 0 {
                    let d = Self::dit_cur(obj);
                    return Some(Value::String(self.spl_zval_get_string(&d)));
                }
                match prop(obj, b"__spl_cit_str") {
                    Value::String(s) => Value::String(s),
                    _ => Value::String(PhpString::empty()),
                }
            }
            (b"cachingiterator", b"offsetset") => {
                let k = match self.spl_arg_string("CachingIterator::offsetSet", 1, "key", args.first().unwrap_or(&Value::Null)) {
                    Some(k) => k,
                    None => return Some(Value::Null),
                };
                if !self.cit_full_cache_check(obj) {
                    return Some(Value::Null);
                }
                let v = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Array(c) = prop(obj, b"__spl_cache") {
                    c.borrow_mut().set(Self::value_to_array_key(Value::String(k)), v);
                }
                Value::Null
            }
            (b"cachingiterator", b"offsetget") => {
                let k = match self.spl_arg_string("CachingIterator::offsetGet", 1, "key", args.first().unwrap_or(&Value::Null)) {
                    Some(k) => k,
                    None => return Some(Value::Null),
                };
                if !self.cit_full_cache_check(obj) {
                    return Some(Value::Null);
                }
                let got = match prop(obj, b"__spl_cache") {
                    Value::Array(c) => c.borrow().get(&Self::value_to_array_key(Value::String(k.clone()))).cloned(),
                    _ => None,
                };
                match got {
                    Some(v) => v.deref(),
                    None => {
                        let msg = format!("Undefined array key \"{}\"", k.to_string_lossy());
                        self.emit_warning(&msg);
                        Value::Null
                    }
                }
            }
            (b"cachingiterator", b"offsetunset") => {
                let k = match self.spl_arg_string("CachingIterator::offsetUnset", 1, "key", args.first().unwrap_or(&Value::Null)) {
                    Some(k) => k,
                    None => return Some(Value::Null),
                };
                if !self.cit_full_cache_check(obj) {
                    return Some(Value::Null);
                }
                if let Value::Array(c) = prop(obj, b"__spl_cache") {
                    c.borrow_mut().remove(&Self::value_to_array_key(Value::String(k)));
                }
                Value::Null
            }
            (b"cachingiterator", b"offsetexists") => {
                let k = match self.spl_arg_string("CachingIterator::offsetExists", 1, "key", args.first().unwrap_or(&Value::Null)) {
                    Some(k) => k,
                    None => return Some(Value::Null),
                };
                if !self.cit_full_cache_check(obj) {
                    return Some(Value::Null);
                }
                match prop(obj, b"__spl_cache") {
                    Value::Array(c) => bv(c.borrow().get(&Self::value_to_array_key(Value::String(k))).is_some()),
                    _ => Value::False,
                }
            }
            (b"cachingiterator", b"getcache") => {
                if !self.cit_full_cache_check(obj) {
                    return Some(Value::Null);
                }
                match prop(obj, b"__spl_cache") {
                    Value::Array(c) => val_arr(c.borrow().clone()),
                    _ => val_arr(PhpArray::new()),
                }
            }
            (b"cachingiterator", b"count") => {
                if !self.cit_full_cache_check(obj) {
                    return Some(Value::Null);
                }
                match prop(obj, b"__spl_cache") {
                    Value::Array(c) => Value::Long(c.borrow().len() as i64),
                    _ => Value::Long(0),
                }
            }
            (b"cachingiterator", b"getflags") => Value::Long(Self::cit_flags(obj)),
            (b"cachingiterator", b"setflags") => {
                let flags = match args.first() {
                    Some(v) => match self.spl_arg_long("CachingIterator::setFlags", 1, "flags", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                if !Self::cit_check_flags(flags) {
                    return self.spl_throw(b"ValueError", "CachingIterator::setFlags(): Argument #1 ($flags) must contain only one of CachingIterator::CALL_TOSTRING, CachingIterator::TOSTRING_USE_KEY, CachingIterator::TOSTRING_USE_CURRENT, or CachingIterator::TOSTRING_USE_INNER");
                }
                let cur = Self::cit_flags(obj);
                if cur & CIT_CALL_TOSTRING != 0 && flags & CIT_CALL_TOSTRING == 0 {
                    return self.spl_throw(b"InvalidArgumentException", "Unsetting flag CALL_TO_STRING is not possible");
                }
                if cur & CIT_TOSTRING_USE_INNER != 0 && flags & CIT_TOSTRING_USE_INNER == 0 {
                    return self.spl_throw(b"InvalidArgumentException", "Unsetting flag TOSTRING_USE_INNER is not possible");
                }
                if flags & CIT_FULL_CACHE != 0 && cur & CIT_FULL_CACHE == 0 {
                    set_prop(obj, b"__spl_cache", val_arr(PhpArray::new()));
                }
                Self::cit_set_flags(obj, (cur & !CIT_PUBLIC) | (flags & CIT_PUBLIC));
                Value::Null
            }
            (b"recursivecachingiterator", b"haschildren") => {
                bv(matches!(prop(obj, b"__spl_cit_children"), Value::Object(_)))
            }
            (b"recursivecachingiterator", b"getchildren") => match prop(obj, b"__spl_cit_children") {
                v @ Value::Object(_) => v,
                _ => Value::Null,
            },
            (b"norewinditerator", b"rewind") => Value::Null,
            (b"norewinditerator", b"valid") => {
                let inner = Self::dit_inner(obj);
                bv(self.it_valid(&inner))
            }
            (b"norewinditerator", b"key") => {
                let inner = Self::dit_inner(obj);
                self.it_key(&inner)
            }
            (b"norewinditerator", b"current") => {
                let inner = Self::dit_inner(obj);
                self.it_current(&inner).deref()
            }
            (b"norewinditerator", b"next") => {
                let inner = Self::dit_inner(obj);
                self.it_next(&inner);
                Value::Null
            }
            (b"infiniteiterator", b"next") => {
                self.dit_next(obj, true);
                if self.exc() {
                    return Some(Value::Null);
                }
                if self.dit_valid(obj) {
                    self.dit_fetch(obj, false);
                } else if !self.exc() {
                    self.dit_rewind(obj);
                    if !self.exc() && self.dit_valid(obj) {
                        self.dit_fetch(obj, false);
                    }
                }
                Value::Null
            }
            (b"appenditerator", b"append") => {
                let it = args.first().cloned().unwrap_or(Value::Null).deref();
                if !self.obj_instanceof(&it, b"iterator") {
                    let msg = format!(
                        "AppendIterator::append(): Argument #1 ($iterator) must be of type Iterator, {} given",
                        Self::value_type_name(&it)
                    );
                    return self.spl_throw(b"TypeError", &msg);
                }
                let ai = Self::append_it(obj);
                let ai_valid = self.it_valid(&ai);
                let inner_valid = self.dit_valid(obj);
                self.spl_array_append_value(&ai, it.clone());
                if ai_valid && !inner_valid {
                    self.it_next(&ai);
                }
                let inner = Self::dit_inner(obj);
                if matches!(inner, Value::Null | Value::Undef) || !self.dit_valid(obj) {
                    if !self.it_valid(&ai) {
                        self.it_rewind(&ai);
                    }
                    for _ in 0..100000 {
                        self.append_next_iterator(obj);
                        let cur = Self::dit_inner(obj);
                        let same = match (&cur, &it) {
                            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
                            (Value::Null, _) => true,
                            _ => true,
                        };
                        if same || self.exc() {
                            break;
                        }
                    }
                    self.append_fetch(obj);
                }
                Value::Null
            }
            (b"appenditerator", b"current") => {
                self.dit_fetch(obj, true);
                Self::dit_cur(obj)
            }
            (b"appenditerator", b"rewind") => {
                let ai = Self::append_it(obj);
                self.it_rewind(&ai);
                if self.append_next_iterator(obj) {
                    self.append_fetch(obj);
                }
                Value::Null
            }
            (b"appenditerator", b"next") => {
                if self.dit_valid(obj) {
                    self.dit_next(obj, true);
                }
                self.append_fetch(obj);
                Value::Null
            }
            (b"appenditerator", b"getiteratorindex") => {
                let ai = Self::append_it(obj);
                if self.it_valid(&ai) { self.it_key(&ai) } else { Value::Null }
            }
            (b"appenditerator", b"getarrayiterator") => Self::append_it(obj),
            (b"regexiterator", b"accept") => return Some(self.regex_accept(obj)),
            (b"recursiveregexiterator", b"accept") => {
                if !Self::dit_has_cur(obj) {
                    return Some(Value::False);
                }
                if let Value::Array(a) = Self::dit_cur(obj) {
                    return Some(bv(a.borrow().len() > 0));
                }
                return Some(self.regex_accept(obj));
            }
            (b"regexiterator", b"getregex") => prop(obj, b"__regex_pattern"),
            (b"regexiterator", b"getmode") => Value::Long(lprop(obj, b"__regex_mode", 0)),
            (b"regexiterator", b"setmode") => {
                let m = match args.first() {
                    Some(v) => match self.spl_arg_long("RegexIterator::setMode", 1, "mode", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                if !(0..5).contains(&m) {
                    return self.spl_throw(b"ValueError", "RegexIterator::setMode(): Argument #1 ($mode) must be RegexIterator::MATCH, RegexIterator::GET_MATCH, RegexIterator::ALL_MATCHES, RegexIterator::SPLIT, or RegexIterator::REPLACE");
                }
                set_prop(obj, b"__regex_mode", Value::Long(m));
                Value::Null
            }
            (b"regexiterator", b"getflags") => Value::Long(lprop(obj, b"__regex_flags", 0)),
            (b"regexiterator", b"setflags") => {
                let f = match args.first() {
                    Some(v) => match self.spl_arg_long("RegexIterator::setFlags", 1, "flags", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                set_prop(obj, b"__regex_flags", Value::Long(f));
                Value::Null
            }
            (b"regexiterator", b"getpregflags") => Value::Long(lprop(obj, b"__regex_preg_flags", 0)),
            (b"regexiterator", b"setpregflags") => {
                let f = match args.first() {
                    Some(v) => match self.spl_arg_long("RegexIterator::setPregFlags", 1, "pregFlags", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                set_prop(obj, b"__regex_preg_flags", Value::Long(f));
                Value::Null
            }
            _ => {
                let _ = t;
                return None;
            }
        };
        Some(r)
    }

    /// Append to an ArrayIterator's storage (spl_array_iterator_append).
    fn spl_array_append_value(&mut self, ai: &Value, v: Value) {
        if let Value::Object(o) = ai {
            if let Value::Array(a) = o.borrow().get_property(b"__spl_array") {
                a.borrow_mut().push(v);
            }
        }
    }

    /// object_init_with_constructor(Z_OBJCE_P(ZEND_THIS), args)
    fn spl_new_same_class(&mut self, obj: &Rc<RefCell<PhpObject>>, args: &[Value]) -> Value {
        let class = obj.borrow().class_name.clone();
        let cl: Vec<u8> = class.iter().map(|b| b.to_ascii_lowercase()).collect();
        let new_obj = self.spl_instantiate(&class);
        if self.spl_has_user_ctor(&cl) {
            self.call_object_method(&Value::Object(new_obj.clone()), b"__construct", args);
        } else {
            self.dit_construct(&new_obj, args);
        }
        if self.exc() {
            return Value::Null;
        }
        Value::Object(new_obj)
    }

    fn regex_accept(&mut self, obj: &Rc<RefCell<PhpObject>>) -> Value {
        if !Self::dit_has_cur(obj) {
            return Value::False;
        }
        let flags = lprop(obj, b"__regex_flags", 0);
        let subject = if flags & REGIT_USE_KEY != 0 {
            let k = Self::dit_key(obj);
            self.spl_zval_get_string(&k)
        } else {
            let d = Self::dit_cur(obj);
            if matches!(d, Value::Array(_)) {
                return Value::False;
            }
            self.spl_zval_get_string(&d)
        };
        if self.exc() {
            return Value::Null;
        }
        let pattern = prop(obj, b"__regex_pattern");
        let preg_flags = lprop(obj, b"__regex_preg_flags", 0);
        let subj = Value::String(subject.clone());
        let mut result = match lprop(obj, b"__regex_mode", 0) {
            1 | 2 => {
                let all = lprop(obj, b"__regex_mode", 0) == 2;
                let out = Value::Reference(Rc::new(RefCell::new(Value::Null)));
                let f: &[u8] = if all { b"preg_match_all" } else { b"preg_match" };
                let mut a = vec![pattern, subj, out.clone()];
                if preg_flags != 0 || all {
                    a.push(Value::Long(if all && preg_flags == 0 { 1 } else { preg_flags }));
                }
                let cnt = self.spl_call_builtin(f, &a).unwrap_or(Value::Long(0));
                let m = out.deref();
                set_prop(obj, b"__spl_cur_data", m);
                cnt.to_long() > 0
            }
            3 => {
                let r = self
                    .spl_call_builtin(b"preg_split", &[pattern, subj, Value::Long(-1), Value::Long(preg_flags)])
                    .unwrap_or(Value::False);
                let n = match &r {
                    Value::Array(a) => a.borrow().len(),
                    _ => 0,
                };
                set_prop(obj, b"__spl_cur_data", r);
                n > 1
            }
            4 => {
                let repl = obj.borrow().get_property(b"replacement");
                let repl_s = match repl {
                    Value::Null | Value::Undef => PhpString::empty(),
                    other => self.value_to_string(&other),
                };
                let count = Value::Reference(Rc::new(RefCell::new(Value::Long(0))));
                let r = self.spl_call_builtin(
                    b"preg_replace",
                    &[pattern, Value::String(repl_s), subj, Value::Long(-1), count.clone()],
                );
                match r {
                    Some(v @ Value::String(_)) => {
                        if flags & REGIT_USE_KEY != 0 {
                            set_prop(obj, b"__spl_cur_key", v);
                        } else {
                            set_prop(obj, b"__spl_cur_data", v);
                        }
                        count.deref().to_long() > 0
                    }
                    _ => return Value::False,
                }
            }
            _ => self
                .spl_call_builtin(b"preg_match", &[pattern, subj])
                .map(|v| v.to_long() > 0)
                .unwrap_or(false),
        };
        if flags & REGIT_INVERTED != 0 {
            result = !result;
        }
        bv(result)
    }

    // ------------------------------------------------------------------
    // RecursiveIteratorIterator / RecursiveTreeIterator
    // ------------------------------------------------------------------

    fn rii_its(obj: &Rc<RefCell<PhpObject>>) -> Option<Rc<RefCell<PhpArray>>> {
        match prop(obj, b"__spl_rii_its") {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    fn rii_level(obj: &Rc<RefCell<PhpObject>>) -> i64 {
        Self::rii_its(obj).map(|a| a.borrow().len() as i64 - 1).unwrap_or(-1)
    }

    fn rii_it_at(obj: &Rc<RefCell<PhpObject>>, level: i64) -> Value {
        Self::rii_its(obj)
            .and_then(|a| a.borrow().get(&ArrayKey::Int(level)).cloned())
            .unwrap_or(Value::Null)
    }

    fn rii_state(obj: &Rc<RefCell<PhpObject>>, level: i64) -> i64 {
        match prop(obj, b"__spl_rii_st") {
            Value::Array(a) => match a.borrow().get(&ArrayKey::Int(level)) {
                Some(Value::Long(n)) => *n,
                _ => RS_START,
            },
            _ => RS_START,
        }
    }

    fn rii_set_state(obj: &Rc<RefCell<PhpObject>>, level: i64, st: i64) {
        if let Value::Array(a) = prop(obj, b"__spl_rii_st") {
            a.borrow_mut().set(ArrayKey::Int(level), Value::Long(st));
        }
    }

    fn rii_push(obj: &Rc<RefCell<PhpObject>>, it: Value) {
        let lvl = Self::rii_level(obj) + 1;
        if let Some(a) = Self::rii_its(obj) {
            a.borrow_mut().set(ArrayKey::Int(lvl), it);
        }
        Self::rii_set_state(obj, lvl, RS_START);
    }

    fn rii_pop(obj: &Rc<RefCell<PhpObject>>) {
        let lvl = Self::rii_level(obj);
        if lvl <= 0 {
            return;
        }
        if let Some(a) = Self::rii_its(obj) {
            a.borrow_mut().remove(&ArrayKey::Int(lvl));
        }
        if let Value::Array(a) = prop(obj, b"__spl_rii_st") {
            a.borrow_mut().remove(&ArrayKey::Int(lvl));
        }
    }

    fn rii_hook(obj: &Rc<RefCell<PhpObject>>, h: i64) -> bool {
        lprop(obj, b"__spl_rii_hooks", 0) & h != 0
    }

    fn rii_call_hook(&mut self, obj: &Rc<RefCell<PhpObject>>, name: &[u8]) -> Option<Value> {
        self.spl_call_method(&Value::Object(obj.clone()), name, &[])
    }

    fn rii_construct(&mut self, obj: &Rc<RefCell<PhpObject>>, base: &[u8], args: &[Value]) -> bool {
        let is_tree = base == b"recursivetreeiterator";
        let func = if is_tree { "RecursiveTreeIterator::__construct" } else { "RecursiveIteratorIterator::__construct" };
        if !self.arity_check(func, args.len(), 1, if is_tree { 4 } else { 3 }) {
            return false;
        }
        let mut iterator = args[0].deref();
        if !matches!(iterator, Value::Object(_)) {
            let msg = format!(
                "{}(): Argument #1 ($iterator) must be of type Traversable, {} given",
                func,
                Self::value_type_name(&iterator)
            );
            self.spl_throw(b"TypeError", &msg);
            return false;
        }
        let (mode, flags);
        if is_tree {
            flags = match args.get(1) {
                Some(v) => match self.spl_arg_long(func, 2, "flags", v) {
                    Some(n) => n,
                    None => return false,
                },
                None => RTIT_BYPASS_KEY,
            };
            let cit_flags = match args.get(2) {
                Some(v) => match self.spl_arg_long(func, 3, "cachingIteratorFlags", v) {
                    Some(n) => n,
                    None => return false,
                },
                None => CIT_CATCH_GET_CHILD,
            };
            mode = match args.get(3) {
                Some(v) => match self.spl_arg_long(func, 4, "mode", v) {
                    Some(n) => n,
                    None => return false,
                },
                None => RIT_SELF_FIRST,
            };
            if self.obj_instanceof(&iterator, b"iteratoraggregate") {
                match self.rii_from_aggregate(&iterator) {
                    Some(v) => iterator = v,
                    None => return false,
                }
            }
            let rci = self.spl_instantiate(b"RecursiveCachingIterator");
            if !self.dit_construct(&rci, &[iterator, Value::Long(cit_flags)]) {
                return false;
            }
            iterator = Value::Object(rci);
        } else {
            mode = match args.get(1) {
                Some(v) => match self.spl_arg_long(func, 2, "mode", v) {
                    Some(n) => n,
                    None => return false,
                },
                None => RIT_LEAVES_ONLY,
            };
            flags = match args.get(2) {
                Some(v) => match self.spl_arg_long(func, 3, "flags", v) {
                    Some(n) => n,
                    None => return false,
                },
                None => 0,
            };
            if self.obj_instanceof(&iterator, b"iteratoraggregate") {
                match self.rii_from_aggregate(&iterator) {
                    Some(v) => iterator = v,
                    None => return false,
                }
            }
        }
        if !self.obj_instanceof(&iterator, b"recursiveiterator") {
            self.spl_throw(b"InvalidArgumentException", "An instance of RecursiveIterator or IteratorAggregate creating it is required");
            return false;
        }
        // Overridden hooks
        let cl = obj_class_lower(obj);
        let mut hooks = 0;
        for (name, bit) in [
            (&b"beginiteration"[..], H_BEGIN_ITER),
            (b"enditeration", H_END_ITER),
            (b"callhaschildren", H_CALL_HAS),
            (b"callgetchildren", H_CALL_GET),
            (b"beginchildren", H_BEGIN_CH),
            (b"endchildren", H_END_CH),
            (b"nextelement", H_NEXT_EL),
        ] {
            if self.classes.get(&cl).map(|c| c.get_method(name).is_some()).unwrap_or(false) {
                hooks |= bit;
            }
        }
        let mut its = PhpArray::new();
        its.set(ArrayKey::Int(0), iterator);
        let mut st = PhpArray::new();
        st.set(ArrayKey::Int(0), Value::Long(RS_START));
        let mut ob = obj.borrow_mut();
        ob.set_property(b"__spl_rii_its".to_vec(), val_arr(its));
        ob.set_property(b"__spl_rii_st".to_vec(), val_arr(st));
        ob.set_property(b"__spl_rii_mode".to_vec(), Value::Long(mode));
        ob.set_property(b"__spl_rii_flags".to_vec(), Value::Long(flags));
        ob.set_property(b"__spl_rii_maxd".to_vec(), Value::Long(-1));
        ob.set_property(b"__spl_rii_initer".to_vec(), Value::False);
        ob.set_property(b"__spl_rii_hooks".to_vec(), Value::Long(hooks));
        if is_tree {
            let mut p = PhpArray::new();
            for s in ["", "| ", "  ", "|-", "\\-", ""] {
                p.push(sv(s));
            }
            ob.set_property(b"__spl_rti_prefix".to_vec(), val_arr(p));
            ob.set_property(b"__spl_rti_postfix".to_vec(), sv(""));
        }
        true
    }

    fn rii_from_aggregate(&mut self, agg: &Value) -> Option<Value> {
        let cls = match agg {
            Value::Object(o) => crate::value::display_class_name(&o.borrow().class_name).to_string(),
            _ => String::new(),
        };
        let r = self.spl_call_method(agg, b"getIterator", &[]);
        if self.exc() {
            return None;
        }
        let r = r.unwrap_or(Value::Null);
        if !self.obj_instanceof(&r, b"traversable") {
            let msg = format!("{}::getIterator() must return an object that implements Traversable", cls);
            self.spl_throw(b"LogicException", &msg);
            return None;
        }
        Some(r)
    }

    fn rii_catch(obj: &Rc<RefCell<PhpObject>>) -> bool {
        lprop(obj, b"__spl_rii_flags", 0) & RIT_CATCH_GET_CHILD != 0
    }

    fn rii_valid_ex(&mut self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        if Self::rii_its(obj).is_none() {
            return false;
        }
        let mut level = Self::rii_level(obj);
        while level >= 0 {
            let it = Self::rii_it_at(obj, level);
            if self.it_valid(&it) {
                return true;
            }
            if self.exc() {
                return false;
            }
            level -= 1;
        }
        if Self::rii_hook(obj, H_END_ITER) && bprop(obj, b"__spl_rii_initer") {
            self.rii_call_hook(obj, b"endIteration");
        }
        set_prop(obj, b"__spl_rii_initer", Value::False);
        false
    }

    fn rii_move_forward_ex(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        if Self::rii_its(obj).is_none() {
            self.spl_throw(b"Error", "The object is in an invalid state as the parent constructor was not called");
            return;
        }
        let mode = lprop(obj, b"__spl_rii_mode", RIT_LEAVES_ONLY);
        let max_depth = lprop(obj, b"__spl_rii_maxd", -1);
        let catch = Self::rii_catch(obj);
        let mut guard = 0u64;
        'outer: while !self.exc() {
            guard += 1;
            if guard > 10_000_000 {
                return;
            }
            let level = Self::rii_level(obj);
            let it = Self::rii_it_at(obj, level);
            let mut st = Self::rii_state(obj, level);
            // RS_NEXT / RS_START / RS_TEST fallthrough chain
            if st == RS_NEXT {
                self.it_next(&it);
                if self.exc() {
                    if !catch {
                        return;
                    }
                    self.current_exception = None;
                }
                st = RS_START;
            }
            if st == RS_START {
                if !self.it_valid(&it) {
                    if self.exc() {
                        return;
                    }
                    // no more elements at this level
                    if level > 0 {
                        if Self::rii_hook(obj, H_END_CH) {
                            self.rii_call_hook(obj, b"endchildren");
                            if self.exc() {
                                if !catch {
                                    return;
                                }
                                self.current_exception = None;
                            }
                        }
                        if Self::rii_level(obj) > 0 {
                            Self::rii_pop(obj);
                        }
                        continue 'outer;
                    } else {
                        return;
                    }
                }
                Self::rii_set_state(obj, level, RS_TEST);
                st = RS_TEST;
            }
            match st {
                RS_TEST => {
                    let r = if Self::rii_hook(obj, H_CALL_HAS) {
                        self.rii_call_hook(obj, b"callHasChildren")
                    } else {
                        self.spl_call_method(&it, b"hasChildren", &[])
                    };
                    if self.exc() {
                        if !catch {
                            Self::rii_set_state(obj, level, RS_NEXT);
                            return;
                        }
                        self.current_exception = None;
                    }
                    if let Some(r) = r {
                        if r.is_truthy() {
                            if max_depth == -1 || max_depth > level {
                                match mode {
                                    RIT_LEAVES_ONLY | RIT_CHILD_FIRST => {
                                        Self::rii_set_state(obj, level, RS_CHILD);
                                        continue 'outer;
                                    }
                                    _ => {
                                        Self::rii_set_state(obj, level, RS_SELF);
                                        continue 'outer;
                                    }
                                }
                            } else if mode == RIT_LEAVES_ONLY {
                                Self::rii_set_state(obj, level, RS_NEXT);
                                continue 'outer;
                            }
                        }
                    }
                    if Self::rii_hook(obj, H_NEXT_EL) {
                        self.rii_call_hook(obj, b"nextelement");
                    }
                    Self::rii_set_state(obj, level, RS_NEXT);
                    if self.exc() {
                        if !catch {
                            return;
                        }
                        self.current_exception = None;
                    }
                    return;
                }
                RS_SELF => {
                    if Self::rii_hook(obj, H_NEXT_EL) && (mode == RIT_SELF_FIRST || mode == RIT_CHILD_FIRST) {
                        self.rii_call_hook(obj, b"nextelement");
                    }
                    if mode == RIT_SELF_FIRST {
                        Self::rii_set_state(obj, level, RS_CHILD);
                    } else {
                        Self::rii_set_state(obj, level, RS_NEXT);
                    }
                    return;
                }
                RS_CHILD => {
                    let child = if Self::rii_hook(obj, H_CALL_GET) {
                        self.rii_call_hook(obj, b"callGetChildren")
                    } else {
                        self.spl_call_method(&it, b"getChildren", &[])
                    };
                    if self.exc() {
                        if !catch {
                            return;
                        }
                        self.current_exception = None;
                        Self::rii_set_state(obj, level, RS_NEXT);
                        continue 'outer;
                    }
                    let child = child.unwrap_or(Value::Null);
                    if !self.obj_instanceof(&child, b"recursiveiterator") {
                        self.spl_throw(
                            b"UnexpectedValueException",
                            "Objects returned by RecursiveIterator::getChildren() must implement RecursiveIterator",
                        );
                        return;
                    }
                    if mode == RIT_CHILD_FIRST {
                        Self::rii_set_state(obj, level, RS_SELF);
                    } else {
                        Self::rii_set_state(obj, level, RS_NEXT);
                    }
                    Self::rii_push(obj, child.clone());
                    self.it_rewind(&child);
                    if Self::rii_hook(obj, H_BEGIN_CH) {
                        self.rii_call_hook(obj, b"beginchildren");
                        if self.exc() {
                            if !catch {
                                return;
                            }
                            self.current_exception = None;
                        }
                    }
                    continue 'outer;
                }
                _ => return,
            }
        }
    }

    fn rii_rewind_ex(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        if Self::rii_its(obj).is_none() {
            self.spl_throw(b"Error", "The object is in an invalid state as the parent constructor was not called");
            return;
        }
        while Self::rii_level(obj) > 0 {
            Self::rii_pop(obj);
            if !self.exc() && Self::rii_hook(obj, H_END_CH) {
                self.rii_call_hook(obj, b"endchildren");
            }
        }
        Self::rii_set_state(obj, 0, RS_START);
        let it = Self::rii_it_at(obj, 0);
        self.it_rewind(&it);
        if !self.exc() && Self::rii_hook(obj, H_BEGIN_ITER) && !bprop(obj, b"__spl_rii_initer") {
            self.rii_call_hook(obj, b"beginIteration");
        }
        set_prop(obj, b"__spl_rii_initer", Value::True);
        self.rii_move_forward_ex(obj);
    }

    fn rti_prefix(&mut self, obj: &Rc<RefCell<PhpObject>>) -> PhpString {
        let parts: Vec<Vec<u8>> = match prop(obj, b"__spl_rti_prefix") {
            Value::Array(a) => a.borrow().values().map(|v| v.to_php_string().as_bytes().to_vec()).collect(),
            _ => vec![Vec::new(); 6],
        };
        let part = |i: usize| parts.get(i).cloned().unwrap_or_default();
        let mut s = part(0);
        let level = Self::rii_level(obj);
        for l in 0..level {
            let it = Self::rii_it_at(obj, l);
            let hn = self.spl_call_method(&it, b"hasNext", &[]);
            if let Some(hn) = hn {
                if matches!(hn, Value::True) {
                    s.extend_from_slice(&part(1));
                } else {
                    s.extend_from_slice(&part(2));
                }
            }
        }
        let it = Self::rii_it_at(obj, level);
        let hn = self.spl_call_method(&it, b"hasNext", &[]);
        if let Some(hn) = hn {
            if matches!(hn, Value::True) {
                s.extend_from_slice(&part(3));
            } else {
                s.extend_from_slice(&part(4));
            }
        }
        s.extend_from_slice(&part(5));
        PhpString::from_vec(s)
    }

    fn rti_entry(&mut self, obj: &Rc<RefCell<PhpObject>>) -> Option<PhpString> {
        let it = Self::rii_it_at(obj, Self::rii_level(obj));
        let data = self.it_current(&it).deref();
        if self.exc() {
            return None;
        }
        if matches!(data, Value::Array(_)) {
            return Some(PhpString::from_bytes(b"Array"));
        }
        let s = self.spl_zval_get_string(&data);
        if self.exc() {
            return None;
        }
        Some(s)
    }

    fn rii_check(&mut self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        if Self::rii_its(obj).is_none() {
            self.spl_throw(b"Error", "The object is in an invalid state as the parent constructor was not called");
            return false;
        }
        true
    }

    fn rii_dispatch(
        &mut self,
        class_lower: &[u8],
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
        args: &[Value],
    ) -> Option<Value> {
        let r = match (class_lower, method) {
            (b"recursiveiteratoriterator", b"rewind") => {
                self.rii_rewind_ex(obj);
                Value::Null
            }
            (b"recursiveiteratoriterator", b"valid") => bv(self.rii_valid_ex(obj)),
            (b"recursiveiteratoriterator", b"key") => {
                if !self.rii_check(obj) {
                    return Some(Value::Null);
                }
                let it = Self::rii_it_at(obj, Self::rii_level(obj));
                self.it_key(&it)
            }
            (b"recursiveiteratoriterator", b"current") => {
                if !self.rii_check(obj) {
                    return Some(Value::Null);
                }
                let it = Self::rii_it_at(obj, Self::rii_level(obj));
                self.it_current(&it).deref()
            }
            (b"recursiveiteratoriterator", b"next") => {
                self.rii_move_forward_ex(obj);
                Value::Null
            }
            (b"recursiveiteratoriterator", b"getdepth") => Value::Long(Self::rii_level(obj).max(0)),
            (b"recursiveiteratoriterator", b"getsubiterator") => {
                let lvl = Self::rii_level(obj);
                let level = match args.first().map(|v| v.deref()) {
                    None | Some(Value::Null) | Some(Value::Undef) => lvl,
                    Some(v) => match self.spl_arg_long("RecursiveIteratorIterator::getSubIterator", 1, "level", &v) {
                        Some(n) => {
                            if n < 0 || n > lvl {
                                return Some(Value::Null);
                            }
                            n
                        }
                        None => return Some(Value::Null),
                    },
                };
                if !self.rii_check(obj) {
                    return Some(Value::Null);
                }
                Self::rii_it_at(obj, level)
            }
            (b"recursiveiteratoriterator", b"getinneriterator") => {
                if !self.rii_check(obj) {
                    return Some(Value::Null);
                }
                Self::rii_it_at(obj, Self::rii_level(obj))
            }
            (b"recursiveiteratoriterator", b"beginiteration")
            | (b"recursiveiteratoriterator", b"enditeration")
            | (b"recursiveiteratoriterator", b"beginchildren")
            | (b"recursiveiteratoriterator", b"endchildren")
            | (b"recursiveiteratoriterator", b"nextelement") => Value::Null,
            (b"recursiveiteratoriterator", b"callhaschildren") => {
                if Self::rii_its(obj).is_none() {
                    return Some(Value::False);
                }
                let it = Self::rii_it_at(obj, Self::rii_level(obj));
                if matches!(it, Value::Null | Value::Undef) {
                    return Some(Value::False);
                }
                self.spl_call_method(&it, b"hasChildren", &[]).unwrap_or(Value::False)
            }
            (b"recursiveiteratoriterator", b"callgetchildren") => {
                if !self.rii_check(obj) {
                    return Some(Value::Null);
                }
                let it = Self::rii_it_at(obj, Self::rii_level(obj));
                if matches!(it, Value::Null | Value::Undef) {
                    return Some(Value::Null);
                }
                self.spl_call_method(&it, b"getChildren", &[]).unwrap_or(Value::Null)
            }
            (b"recursiveiteratoriterator", b"setmaxdepth") => {
                let d = match args.first() {
                    Some(v) => match self.spl_arg_long("RecursiveIteratorIterator::setMaxDepth", 1, "maxDepth", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => -1,
                };
                if d < -1 {
                    return self.spl_throw(b"ValueError", "RecursiveIteratorIterator::setMaxDepth(): Argument #1 ($maxDepth) must be greater than or equal to -1");
                }
                set_prop(obj, b"__spl_rii_maxd", Value::Long(d.min(i32::MAX as i64)));
                Value::Null
            }
            (b"recursiveiteratoriterator", b"getmaxdepth") => {
                let d = lprop(obj, b"__spl_rii_maxd", -1);
                if d == -1 { Value::False } else { Value::Long(d) }
            }
            (b"recursivetreeiterator", b"setprefixpart") => {
                let part = match args.first() {
                    Some(v) => match self.spl_arg_long("RecursiveTreeIterator::setPrefixPart", 1, "part", v) {
                        Some(n) => n,
                        None => return Some(Value::Null),
                    },
                    None => 0,
                };
                let value = match args.get(1) {
                    Some(v) => match self.spl_arg_string("RecursiveTreeIterator::setPrefixPart", 2, "value", v) {
                        Some(s) => s,
                        None => return Some(Value::Null),
                    },
                    None => PhpString::empty(),
                };
                if !(0..=5).contains(&part) {
                    return self.spl_throw(b"ValueError", "RecursiveTreeIterator::setPrefixPart(): Argument #1 ($part) must be a RecursiveTreeIterator::PREFIX_* constant");
                }
                if let Value::Array(a) = prop(obj, b"__spl_rti_prefix") {
                    a.borrow_mut().set(ArrayKey::Int(part), Value::String(value));
                }
                Value::Null
            }
            (b"recursivetreeiterator", b"getprefix") => {
                if !self.rii_check(obj) {
                    return Some(Value::Null);
                }
                Value::String(self.rti_prefix(obj))
            }
            (b"recursivetreeiterator", b"setpostfix") => {
                let value = match args.first() {
                    Some(v) => match self.spl_arg_string("RecursiveTreeIterator::setPostfix", 1, "postfix", v) {
                        Some(s) => s,
                        None => return Some(Value::Null),
                    },
                    None => PhpString::empty(),
                };
                set_prop(obj, b"__spl_rti_postfix", Value::String(value));
                Value::Null
            }
            (b"recursivetreeiterator", b"getentry") => {
                if !self.rii_check(obj) {
                    return Some(Value::Null);
                }
                match self.rti_entry(obj) {
                    Some(s) => Value::String(s),
                    None => Value::Null,
                }
            }
            (b"recursivetreeiterator", b"getpostfix") => {
                if !self.rii_check(obj) {
                    return Some(Value::Null);
                }
                prop(obj, b"__spl_rti_postfix")
            }
            (b"recursivetreeiterator", b"current") => {
                if !self.rii_check(obj) {
                    return Some(Value::Null);
                }
                if lprop(obj, b"__spl_rii_flags", 0) & RTIT_BYPASS_CURRENT != 0 {
                    let it = Self::rii_it_at(obj, Self::rii_level(obj));
                    return Some(self.it_current(&it).deref());
                }
                let entry = match self.rti_entry(obj) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let prefix = self.rti_prefix(obj);
                let postfix = prop(obj, b"__spl_rti_postfix").to_php_string();
                let mut out = prefix.as_bytes().to_vec();
                out.extend_from_slice(entry.as_bytes());
                out.extend_from_slice(postfix.as_bytes());
                Value::String(PhpString::from_vec(out))
            }
            (b"recursivetreeiterator", b"key") => {
                if !self.rii_check(obj) {
                    return Some(Value::Null);
                }
                let it = Self::rii_it_at(obj, Self::rii_level(obj));
                let key = self.it_key(&it);
                if lprop(obj, b"__spl_rii_flags", 0) & RTIT_BYPASS_KEY != 0 {
                    return Some(key);
                }
                let ks = self.spl_zval_get_string(&key);
                let prefix = self.rti_prefix(obj);
                let postfix = prop(obj, b"__spl_rti_postfix").to_php_string();
                let mut out = prefix.as_bytes().to_vec();
                out.extend_from_slice(ks.as_bytes());
                out.extend_from_slice(postfix.as_bytes());
                Value::String(PhpString::from_vec(out))
            }
            _ => return None,
        };
        Some(r)
    }
}

/// Native constructor for the iterator classes implemented here.
pub fn spl_iter_construct_native(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let obj = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return Ok(()),
    };
    vm.spl_push_ctor_frame(&obj, &args[1..]);
    let ok = vm.dit_construct(&obj, &args[1..]);
    vm.call_stack.pop();
    if ok {
        Ok(())
    } else {
        let msg = match &vm.current_exception {
            Some(Value::Object(e)) => e.borrow().get_property(b"message").to_php_string().to_string_lossy(),
            _ => String::new(),
        };
        Err(msg)
    }
}
