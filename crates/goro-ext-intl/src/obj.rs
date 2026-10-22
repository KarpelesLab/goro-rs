//! Object state of the intl classes: native data attached by the
//! create_object hooks, and object instantiation (object_init_ex()).

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use goro_core::object::{NativeData, PhpObject};
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::zpp::Obj;

/// A create_object hook.
pub type CreateFn = fn(&mut Vm, &Obj);

/// Attach native state `st` to `o`.
pub fn attach<T: 'static>(o: &Obj, st: T, handlers: &'static NativeObjectHandlers) {
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(RefCell::new(st)),
        handlers,
    });
}

/// The native state of `o`, if it has state of type T.
pub fn state<T: 'static>(o: &Obj) -> Option<Rc<RefCell<T>>> {
    let data: Rc<dyn Any> = o.borrow().native.as_ref()?.data.clone();
    data.downcast::<RefCell<T>>().ok()
}

/// Copy the state of `src` (type T) to `dst` with `f`, keeping the handlers.
pub fn clone_state<T: 'static>(src: &Obj, dst: &Obj, f: impl FnOnce(&T) -> T) {
    let Some(st) = state::<T>(src) else {
        return;
    };
    let handlers = match src.borrow().native.as_ref() {
        Some(n) => n.handlers,
        None => return,
    };
    let copy = f(&st.borrow());
    attach(dst, copy, handlers);
}

/// object_init_ex(): a new object of `class_lower` (display name taken from
/// the class table), with its create_object hook run, without calling the
/// constructor. None if an exception was thrown.
pub fn object_init_ex(vm: &mut Vm, class_lower: &[u8]) -> Option<Obj> {
    let name = vm
        .classes
        .get(class_lower)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| vm.builtin_canonical_name(class_lower).into_bytes());
    if let Some(c) = vm.classes.get(class_lower) {
        let kind = if c.is_interface {
            Some("interface")
        } else if c.is_trait {
            Some("trait")
        } else if c.is_enum {
            Some("enum")
        } else if c.is_abstract {
            Some("abstract class")
        } else {
            None
        };
        if let Some(k) = kind {
            let m = format!(
                "Cannot instantiate {} {}",
                k,
                String::from_utf8_lossy(&c.name)
            );
            crate::zpp::throw(vm, b"Error", &m);
            return None;
        }
    }
    let id = vm.alloc_object_id();
    let mut obj = PhpObject::new(name, id);
    vm.apply_class_property_defaults(&mut obj, class_lower);
    if !vm.classes.contains_key(class_lower) {
        if let Some(sc) = goro_core::stubs::find_class(class_lower) {
            for p in sc.props {
                if p.flags & goro_core::stubs::F_STATIC != 0 || obj.has_property(p.name.as_bytes())
                {
                    continue;
                }
                let v = p
                    .default
                    .and_then(goro_core::stubs::literal_value)
                    .unwrap_or(Value::Undef);
                obj.set_property(p.name.as_bytes().to_vec(), v);
            }
        }
    }
    let o = Rc::new(RefCell::new(obj));
    goro_core::gc::track_object(&o);
    let has_dtor = vm
        .classes
        .get(class_lower)
        .is_some_and(|c| c.get_method(b"__destruct").is_some());
    if has_dtor {
        vm.track_destructible(Value::Object(o.clone()));
    }
    vm.run_object_create_hook(&o);
    if vm.current_exception.is_some() {
        return None;
    }
    Some(o)
}

/// "Trying to clone an uncloneable object of class X" (a clone handler of
/// a class whose objects cannot be cloned).
pub fn uncloneable(vm: &mut Vm, o: &Obj) {
    let cn = goro_core::value::display_class_name(&o.borrow().class_name);
    let msg = format!("Trying to clone an uncloneable object of class {}", cn);
    crate::zpp::throw(vm, b"Error", &msg);
}
