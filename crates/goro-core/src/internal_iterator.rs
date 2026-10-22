//! InternalIterator objects wrapping an iterator an extension implements
//! (zend_create_internal_iterator_zval() over a class's get_iterator).

use std::cell::RefCell;
use std::rc::Rc;

use crate::object::PhpObject;
use crate::value::Value;
use crate::vm::Vm;

/// A zend_object_iterator's funcs.
pub trait ObjectIterator {
    /// funcs->valid
    fn valid(&mut self, vm: &mut Vm) -> bool;
    /// funcs->get_current_data
    fn current(&mut self, vm: &mut Vm) -> Value;
    /// funcs->get_current_key (None: there is none, the index is used)
    fn key(&mut self, _vm: &mut Vm) -> Option<Value> {
        None
    }
    /// funcs->move_forward
    fn move_forward(&mut self, vm: &mut Vm);
    /// funcs->rewind; false if there is none.
    fn rewind(&mut self, vm: &mut Vm) -> bool;
    /// funcs->dtor: the InternalIterator is freed.
    fn dtor(&mut self, _vm: &mut Vm) {}
}

pub(crate) struct ExtIterator {
    it: Box<dyn ObjectIterator>,
    rewind_called: bool,
    index: i64,
}

impl Vm {
    /// zend_create_internal_iterator_zval(): an InternalIterator over `it`.
    pub fn create_internal_iterator(&mut self, it: Box<dyn ObjectIterator>) -> Value {
        // The zend_object_iterator is an object of its own (handle).
        let _ = self.alloc_object_id();
        let obj = self.spl_instantiate(b"InternalIterator");
        let id = obj.borrow().object_id;
        self.ext_iterators.insert(
            id,
            ExtIterator {
                it,
                rewind_called: false,
                index: 0,
            },
        );
        self.list_destructible(Value::Object(obj.clone()));
        Value::Object(obj)
    }

    /// An InternalIterator nothing references any more: free its iterator.
    pub(crate) fn ext_iterator_released(&mut self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let id = obj.borrow().object_id;
        match self.ext_iterators.remove(&id) {
            Some(mut e) => {
                e.it.dtor(self);
                true
            }
            None => false,
        }
    }

    /// InternalIterator's methods, for one wrapping an extension's iterator.
    pub(crate) fn ext_iterator_call(
        &mut self,
        method: &[u8],
        obj: &Rc<RefCell<PhpObject>>,
    ) -> Option<Value> {
        let id = obj.borrow().object_id;
        if !matches!(method, b"current" | b"key" | b"next" | b"valid" | b"rewind") {
            return None;
        }
        let mut e = self.ext_iterators.remove(&id)?;
        let r = self.ext_iterator_method(&mut e, method);
        self.ext_iterators.insert(id, e);
        Some(r)
    }

    fn ext_iterator_method(&mut self, e: &mut ExtIterator, method: &[u8]) -> Value {
        if method == b"rewind" {
            e.rewind_called = true;
            if !e.it.rewind(self) && e.index != 0 {
                let exc = self.create_exception(b"Error", "Iterator does not support rewinding", 0);
                self.current_exception = Some(exc);
                return Value::Null;
            }
            e.index = 0;
            return Value::Null;
        }
        // zend_internal_iterator_ensure_rewound()
        if !e.rewind_called {
            e.rewind_called = true;
            e.it.rewind(self);
            if self.current_exception.is_some() {
                return Value::Null;
            }
        }
        match method {
            b"current" => e.it.current(self).deref(),
            b"key" => e.it.key(self).unwrap_or(Value::Long(e.index)),
            b"next" => {
                e.index += 1;
                e.it.move_forward(self);
                Value::Null
            }
            _ => {
                if e.it.valid(self) {
                    Value::True
                } else {
                    Value::False
                }
            }
        }
    }
}
