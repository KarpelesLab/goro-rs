//! ext/intl/common (common_enum.cpp): IntlIterator, the PHP iterator over
//! an internal source (an ICU StringEnumeration, the parts of a break
//! iterator...).
//!
//! The engine drives an iterator through its methods here, so the keys
//! are those foreach sees (zend_fe_fetch: the index restarts at 0 on
//! rewind, a source may give its own keys).

use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::Call;
use crate::error::{self, IntlError};
use crate::obj;
use crate::zpp::Obj;

/// What an IntlIterator iterates.
pub trait Source {
    /// Restart (before the first element is fetched with `next`).
    fn reset(&mut self, vm: &mut Vm);
    /// The next element and, when the source has its own keys, its key.
    fn next(&mut self, vm: &mut Vm) -> Option<(Option<i64>, Value)>;
    /// The object the source iterates (IntlPartsIterator::getBreakIterator()).
    fn object(&self) -> Option<Obj> {
        None
    }
}

/// A fixed list of values (a StringEnumeration).
struct Values {
    vals: Vec<Value>,
    pos: usize,
}

impl Source for Values {
    fn reset(&mut self, _vm: &mut Vm) {
        self.pos = 0;
    }
    fn next(&mut self, _vm: &mut Vm) -> Option<(Option<i64>, Value)> {
        let v = self.vals.get(self.pos).cloned()?;
        self.pos += 1;
        Some((None, v))
    }
}

/// IntlIterator_object
#[derive(Default)]
pub struct IntlIter {
    pub src: Option<Box<dyn Source>>,
    current: Option<Value>,
    key: i64,
    index: i64,
    pub err: IntlError,
}

pub static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(no_clone),
    ..NativeObjectHandlers::EMPTY
};

fn no_clone(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    obj::uncloneable(vm, src);
}

fn create(_vm: &mut Vm, o: &Obj) {
    obj::attach(o, IntlIter::default(), &HANDLERS);
}

crate::intl_functions! { pub FUNCTIONS: }

pub const METHODS: &[(&str, crate::Imp)] = &[
    ("IntlIterator::current", current),
    ("IntlIterator::key", key),
    ("IntlIterator::next", next),
    ("IntlIterator::rewind", rewind),
    ("IntlIterator::valid", valid),
];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[
    ("IntlIterator", Some(create)),
    ("IntlPartsIterator", Some(create)),
];

/// A new object of `class_lower` (IntlIterator or a subclass) over `src`.
pub fn new_iterator(vm: &mut Vm, class_lower: &[u8], src: Box<dyn Source>) -> Option<Obj> {
    let o = obj::object_init_ex(vm, class_lower)?;
    if obj::state::<IntlIter>(&o).is_none() {
        obj::attach(&o, IntlIter::default(), &HANDLERS);
    }
    let st = obj::state::<IntlIter>(&o)?;
    st.borrow_mut().src = Some(src);
    Some(o)
}

/// An IntlIterator over a fixed list of values, keys 0..n
/// (IntlIterator_from_StringEnumeration()).
pub fn new_array_iterator(vm: &mut Vm, values: Vec<Value>) -> Option<Obj> {
    let o = new_iterator(vm, b"intliterator", Box::new(Values { vals: values, pos: 0 }))?;
    wrap_self(&o);
    Some(o)
}

/// zoi_with_current's wrapping_obj: the iterator references its object,
/// a cycle only the collector frees (as in php-src).
pub fn wrap_self(o: &Obj) {
    let v = Value::Object(o.clone());
    o.borrow_mut()
        .set_property(b"__spl_intl_wrapping".to_vec(), v);
}

/// The source object of an IntlIterator (IntlPartsIterator's break iterator).
pub fn source_object(o: &Obj) -> Option<Obj> {
    let st = obj::state::<IntlIter>(o)?;
    let st = st.borrow();
    st.src.as_ref()?.object()
}

/// INTLITERATOR_METHOD_FETCH_OBJECT: the state, with its error reset.
pub fn fetch(vm: &mut Vm, c: &Call) -> Option<std::rc::Rc<std::cell::RefCell<IntlIter>>> {
    let o = c.obj();
    let st = obj::state::<IntlIter>(&o);
    match st {
        Some(st) if st.borrow().src.is_some() => {
            error::reset_both(&mut st.borrow_mut().err);
            Some(st)
        }
        _ => {
            crate::zpp::throw(vm, b"Error", "Found unconstructed IntlIterator");
            None
        }
    }
}

fn advance(vm: &mut Vm, st: &std::rc::Rc<std::cell::RefCell<IntlIter>>, first: bool) {
    // The source runs without the state borrowed (it may call PHP code).
    let mut src = st.borrow_mut().src.take();
    let r = match src.as_mut() {
        Some(s) => {
            if first {
                s.reset(vm);
            }
            s.next(vm)
        }
        None => None,
    };
    let mut b = st.borrow_mut();
    b.src = src;
    if first {
        b.index = 0;
    } else {
        b.index += 1;
    }
    match r {
        Some((k, v)) => {
            b.key = k.unwrap_or(b.index);
            b.current = Some(v);
        }
        None => {
            b.key = b.index;
            b.current = None;
        }
    }
}

/// IntlIterator::current()
fn current(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    st.borrow().current.clone().unwrap_or(Value::Null)
}

/// IntlIterator::key()
fn key(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    Value::Long(st.borrow().key)
}

/// IntlIterator::next()
fn next(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    advance(vm, &st, false);
    Value::Null
}

/// IntlIterator::rewind()
fn rewind(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    advance(vm, &st, true);
    Value::Null
}

/// IntlIterator::valid()
fn valid(vm: &mut Vm, c: &mut Call) -> Value {
    let Some(st) = fetch(vm, c) else {
        return Value::Null;
    };
    crate::bool_v(st.borrow().current.is_some())
}
