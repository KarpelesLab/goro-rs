//! Object handlers of ext/zend_test's object_handlers.c classes:
//! DoOperationNoCast (do_operation for + and *, no casts) and
//! NumericCastableNoOperations (cast_object(_IS_NUMBER) only). The
//! Long/FloatCastableNoOperations casts (IS_LONG / IS_DOUBLE) are not
//! reachable from arithmetic, which is what the tests exercise.
//!
//! The VM has a single operator hook pair (used by GMP): the previous hooks
//! are kept and called for other objects.

use std::cell::Cell;
use std::rc::Rc;

use goro_core::object::PhpObject;
use goro_core::value::Value;
use goro_core::vm::Vm;

type DoOperation = fn(&mut Vm, &str, &Value, &Value) -> Option<Result<Value, String>>;
type IsOperatorType = fn(&Value) -> bool;

thread_local! {
    static PREV_DO_OPERATION: Cell<Option<DoOperation>> = const { Cell::new(None) };
    static PREV_IS_TYPE: Cell<Option<IsOperatorType>> = const { Cell::new(None) };
}

pub(crate) fn register(vm: &mut Vm) {
    PREV_DO_OPERATION.with(|p| p.set(vm.object_do_operation));
    PREV_IS_TYPE.with(|p| p.set(vm.object_is_operator_type));
    vm.object_do_operation = Some(do_operation);
    vm.object_is_operator_type = Some(is_operator_type);
}

fn class_is(v: &Value, class: &[u8]) -> bool {
    match v {
        Value::Object(o) => o.borrow().class_name.eq_ignore_ascii_case(class),
        Value::Reference(r) => class_is(&r.borrow(), class),
        _ => false,
    }
}

fn is_donc(v: &Value) -> bool {
    class_is(v, b"DoOperationNoCast")
}

fn is_numeric_castable(v: &Value) -> bool {
    class_is(v, b"NumericCastableNoOperations")
}

fn is_operator_type(v: &Value) -> bool {
    is_donc(v) || is_numeric_castable(v) || PREV_IS_TYPE.with(|p| p.get()).is_some_and(|f| f(v))
}

/// OBJ_PROP_NUM(obj, 0): the `val` property.
fn val_of(v: &Value) -> Value {
    match v {
        Value::Object(o) => o.borrow().get_property(b"val"),
        Value::Reference(r) => val_of(&r.borrow()),
        _ => Value::Null,
    }
}

fn do_operation(vm: &mut Vm, op: &str, a: &Value, b: &Value) -> Option<Result<Value, String>> {
    if is_donc(a) || is_donc(b) {
        // donc_do_operation(): ZEND_ADD and ZEND_MUL only.
        let long = |v: &Value| {
            if is_donc(v) {
                val_of(v).to_long()
            } else {
                v.to_long()
            }
        };
        let r = match op {
            "+" => long(a).wrapping_add(long(b)),
            "*" => long(a).wrapping_mul(long(b)),
            _ => return None,
        };
        return Some(Ok(donc_create(vm, r)));
    }
    if is_numeric_castable(a) || is_numeric_castable(b) {
        // No do_operation: the operands are cast to numbers
        // (zendi_try_convert_scalar_to_number(), cast_object(_IS_NUMBER)).
        let num = |v: &Value| {
            if is_numeric_castable(v) {
                val_of(v)
            } else {
                v.deref()
            }
        };
        let (x, y) = (num(a), num(b));
        if matches!(x, Value::Object(_)) || matches!(y, Value::Object(_)) {
            return None;
        }
        return match op {
            "+" => Some(Ok(x.add(&y))),
            "-" => Some(Ok(x.sub(&y))),
            "*" => Some(Ok(x.mul(&y))),
            _ => None,
        };
    }
    PREV_DO_OPERATION
        .with(|p| p.get())
        .and_then(|f| f(vm, op, a, b))
}

/// donc_create(): a new DoOperationNoCast holding `l`, without running
/// its constructor.
fn donc_create(vm: &mut Vm, l: i64) -> Value {
    let id = vm.alloc_object_id();
    let mut o = PhpObject::new(b"DoOperationNoCast".to_vec(), id);
    o.set_property(b"val".to_vec(), Value::Long(l));
    Value::Object(Rc::new(std::cell::RefCell::new(o)))
}

// ---------------------------------------------------------------------------
// DimensionHandlersNoArrayAccess: dimension handlers that record how they
// were called (properties read, write, has, unset, readType, hasOffset,
// checkEmpty, offset).

static DIMENSION_HANDLERS: goro_core::vm::NativeObjectHandlers =
    goro_core::vm::NativeObjectHandlers {
        read_dimension: Some(dim_read),
        write_dimension: Some(dim_write),
        has_dimension: Some(dim_has),
        unset_dimension: Some(dim_unset),
        ..goro_core::vm::NativeObjectHandlers::EMPTY
    };

pub(crate) fn register_dimension_handlers(vm: &mut Vm) {
    vm.object_create_handlers
        .insert(b"dimensionhandlersnoarrayaccess".to_vec(), dim_create);
}

type Obj = Rc<std::cell::RefCell<PhpObject>>;

/// dimension_handlers_no_ArrayAccess_object_create()
fn dim_create(_vm: &mut Vm, o: &Obj) {
    o.borrow_mut().native = Some(goro_core::object::NativeData {
        data: Rc::new(()),
        handlers: &DIMENSION_HANDLERS,
    });
}

/// dimension_common_helper()
fn dim_record(o: &Obj, offset: Option<&Value>, access: &[u8]) {
    let mut ob = o.borrow_mut();
    ob.set_property(access.to_vec(), Value::True);
    let has = if offset.is_some() {
        Value::True
    } else {
        Value::False
    };
    ob.set_property(b"hasOffset".to_vec(), has);
    if let Some(k) = offset {
        ob.set_property(b"offset".to_vec(), k.deref());
    }
}

fn dim_read(vm: &mut Vm, o: &Obj, offset: Option<&Value>, mode: u8) -> Option<Value> {
    dim_record(o, offset, b"read");
    let ty = vm.dim_fetch_type(mode);
    o.borrow_mut()
        .set_property(b"readType".to_vec(), Value::Long(ty as i64));
    Some(Value::True)
}

fn dim_write(_vm: &mut Vm, o: &Obj, offset: Option<&Value>, _value: &Value) -> bool {
    dim_record(o, offset, b"write");
    true
}

fn dim_has(_vm: &mut Vm, o: &Obj, offset: &Value, check_empty: u8) -> Option<bool> {
    o.borrow_mut()
        .set_property(b"checkEmpty".to_vec(), Value::Long(check_empty as i64));
    dim_record(o, Some(offset), b"has");
    Some(true)
}

fn dim_unset(_vm: &mut Vm, o: &Obj, offset: &Value) -> bool {
    dim_record(o, Some(offset), b"unset");
    true
}
