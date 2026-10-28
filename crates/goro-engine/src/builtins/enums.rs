//! Enum methods: `cases()`, `from()`, `tryFrom()` (PHP manual:
//! Enumerations, Backed enumerations).

use crate::class::{DisplayName, TypeName};
use goro_runtime::{PhpArray, Value};

use crate::class::{ConstState, mflags};
use crate::func::Args;
use crate::vm::{R, Vm};

fn enum_class(vm: &Vm, a: &Args) -> std::rc::Rc<crate::class::Class> {
    let p = if a.static_class.is_null() {
        a.func.scope.get()
    } else {
        a.static_class
    };
    vm.rc_class(p)
}

pub fn enum_cases(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cls = enum_class(vm, a);
    let names: Vec<_> = cls
        .consts
        .borrow()
        .iter()
        .filter(|c| c.flags & mflags::CASE != 0)
        .map(|c| c.name.clone())
        .collect();
    let mut out = PhpArray::new();
    for n in names {
        let v = vm.enum_case(&cls, &n)?;
        let _ = out.push(v);
    }
    Ok(Value::Array(out))
}

fn lookup(vm: &mut Vm, a: &mut Args, try_: bool) -> R<Value> {
    let cls = enum_class(vm, a);
    let v = a.val(0);
    let int_backed = cls.enum_backing == Some(crate::bytecode::tmask::INT);
    // Coerce the argument to the backing type.
    let key = match (&v, int_backed) {
        (Value::Int(_), true) | (Value::Str(_), false) => v.clone(),
        (Value::Str(s), true) => {
            let p = goro_runtime::numeric::parse_numeric(s);
            if p.kind == goro_runtime::numeric::Numericness::Numeric
                && let goro_runtime::Number::Int(i) = p.num
                && !vm.caller_strict()
            {
                Value::Int(i)
            } else {
                return vm.type_error(&format!(
                    "{}::{}(): Argument #1 ($value) must be of type int, string given",
                    cls.name.dname(),
                    if try_ { "tryFrom" } else { "from" }
                ));
            }
        }
        (Value::Int(i), false) => Value::Str(goro_runtime::convert::int_to_string(*i)),
        (Value::Float(f), true) if f.fract() == 0.0 => Value::Int(*f as i64),
        (other, _) => {
            if let Value::Float(_) = other {
                return vm.type_error(&format!(
                    "{}::{}(): Argument #1 ($value) must be of type int, float given",
                    cls.name.dname(),
                    if try_ { "tryFrom" } else { "from" }
                ));
            }
            other.clone()
        }
    };
    let consts: Vec<_> = cls
        .consts
        .borrow()
        .iter()
        .filter(|c| c.flags & mflags::CASE != 0)
        .cloned()
        .collect();
    for c in consts {
        let cv = match c.value.borrow().clone() {
            ConstState::Value(v) => v,
            ConstState::Pending(ce) => {
                let v = vm.eval_const_expr(&ce, std::rc::Rc::as_ptr(&cls))?;
                *c.value.borrow_mut() = ConstState::Value(v.clone());
                v
            }
            ConstState::Evaluating => continue,
        };
        if goro_runtime::compare::strict_eq(&cv, &key) {
            return vm.enum_case(&cls, &c.name);
        }
    }
    if try_ {
        return Ok(Value::Null);
    }
    let shown = match &key {
        Value::Str(s) => format!("\"{}\"", s.to_string_lossy()),
        Value::Int(i) => i.to_string(),
        other => other.tname(),
    };
    vm.value_error(&format!(
        "{} is not a valid backing value for enum {}",
        shown,
        enum_display(&cls.name.dname())
    ))
}

fn enum_display(n: &str) -> String {
    n.to_string()
}

pub fn enum_from(vm: &mut Vm, a: &mut Args) -> R<Value> {
    lookup(vm, a, false)
}

pub fn enum_try_from(vm: &mut Vm, a: &mut Args) -> R<Value> {
    lookup(vm, a, true)
}
