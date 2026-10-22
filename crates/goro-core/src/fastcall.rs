//! Fast paths of the object opcodes for plain user classes.
//!
//! NEW and INIT_METHOD_CALL run a long list of checks for the special
//! classes (internal classes and their subclasses, SPL iterators, fibers,
//! throwables, closures ...) and lowercase and look up the same names many
//! times. For an object of a class whose whole hierarchy is user code none
//! of those special cases apply: these helpers do what the general handlers
//! do then, and report when they did nothing (the general handler runs).

use std::cell::RefCell;
use std::rc::Rc;

use super::{CallTarget, PendingCall, Vm};
use crate::object::{PhpObject, Visibility};
use crate::opcode::{Op, OperandType};
use crate::string::PhpString;
use crate::value::Value;

/// Outcome of Vm::new_object_fast().
pub(crate) enum NewFast {
    /// Not a plain user class: nothing was done.
    No,
    /// The new object, for the result operand.
    Obj(Value),
    /// Evaluating the class's constant expressions threw.
    Failed,
}

/// Lowercase `s` into `buf` (None when it does not fit).
#[inline]
fn lower_into<'b>(s: &[u8], buf: &'b mut [u8]) -> Option<&'b [u8]> {
    let out = buf.get_mut(..s.len())?;
    for (o, c) in out.iter_mut().zip(s) {
        *o = c.to_ascii_lowercase();
    }
    Some(out)
}

impl Vm {
    /// Whether class `lower` and all its ancestors are classes declared in
    /// user code (none is, or derives from, an internal class): the special
    /// cases of NEW and INIT_METHOD_CALL for internal classes and their
    /// subclasses then do not apply.
    pub(crate) fn user_class_chain(&self, lower: &[u8]) -> bool {
        let mut cur = match self.classes.get(lower) {
            Some(ce) => ce,
            None => return false,
        };
        let mut cur_lower: Vec<u8>;
        for _ in 0..64 {
            if cur.filename.is_none() || cur.is_enum {
                return false;
            }
            let Some(p) = cur.parent.as_ref() else {
                // (the ancestor walks also ask the stubs about a root)
                return true;
            };
            let mut buf = [0u8; 128];
            let next = match lower_into(p, &mut buf) {
                Some(l) => self.classes.get(l),
                None => {
                    cur_lower = p.to_ascii_lowercase();
                    self.classes.get(cur_lower.as_slice())
                }
            };
            match next {
                Some(ce) => cur = ce,
                None => return false,
            }
        }
        false
    }

    /// NEW of a user class named by a constant (see NewFast): what the
    /// general handler does for one whose hierarchy is all user code.
    pub(crate) fn new_object_fast(
        &mut self,
        op: &Op,
        literals: &[Value],
        tmps: &[Value],
    ) -> NewFast {
        let OperandType::Const(ci) = op.op1 else {
            return NewFast::No;
        };
        let Some(Value::String(name)) = literals.get(ci as usize) else {
            return NewFast::No;
        };
        let name = name.as_bytes();
        if name.is_empty()
            || name[0] == b'\\'
            || name.eq_ignore_ascii_case(b"static")
            || name.eq_ignore_ascii_case(b"self")
            || name.eq_ignore_ascii_case(b"parent")
        {
            return NewFast::No;
        }
        let mut buf = [0u8; 128];
        let Some(name_lower) = lower_into(name, &mut buf) else {
            return NewFast::No;
        };
        let name_lower: &[u8] = name_lower;
        match self.classes.get(name_lower) {
            Some(ce)
                if ce.attributes.is_empty()
                    && !(ce.is_abstract || ce.is_interface || ce.is_enum || ce.is_trait) => {}
            _ => return NewFast::No,
        }
        if !self.user_class_chain(name_lower) {
            return NewFast::No;
        }
        // Recycle the old object ID from the result slot before allocating
        // a new one.
        if let OperandType::Tmp(idx) = &op.result {
            if let Some(old_val) = tmps.get(*idx as usize) {
                self.maybe_recycle_object_id(old_val);
            }
        }
        let obj_id = self.alloc_object_id();
        let canonical_name = match self.classes.get(name_lower) {
            Some(class) => class.name.clone(),
            None => name.to_vec(),
        };
        let mut obj = PhpObject::new(canonical_name, obj_id);
        // Constant-expression defaults are evaluated on first use.
        if self.update_class_constants(name_lower).is_err() {
            return NewFast::Failed;
        }
        self.init_declared_props(name_lower, &mut obj);
        // Properties added from here on are dynamic ones.
        obj.mark_declared();
        let rc = Rc::new(RefCell::new(obj));
        self.run_object_create_hook(&rc);
        let obj_value = Value::Object(rc);
        // Objects with a destructor are tracked; one with a constructor to
        // run is not destructed until the constructor completed.
        let has_destruct = self
            .classes
            .get(name_lower)
            .is_some_and(|c| c.methods.contains_key(b"__destruct".as_slice()));
        if has_destruct {
            if let Value::Object(ref rc) = obj_value {
                rc.borrow_mut()
                    .set_property(b"__ctor_pending".to_vec(), Value::True);
            }
            self.list_destructible(obj_value.clone());
        }
        NewFast::Obj(obj_value)
    }

    /// INIT_METHOD_CALL of a public user method named by a constant on an
    /// object of a class whose hierarchy is all user code: what the general
    /// handler does then (the method is registered as a user function and
    /// the call is set up with `$this`). Returns false, doing nothing,
    /// otherwise.
    pub(crate) fn init_method_call_fast(
        &mut self,
        op: &Op,
        cvs: &[Value],
        tmps: &mut [Value],
        literals: &[Value],
    ) -> bool {
        let OperandType::Const(mi) = op.op2 else {
            return false;
        };
        let Some(Value::String(method_name)) = literals.get(mi as usize) else {
            return false;
        };
        let m = method_name.as_bytes();
        let is_ctor = m.eq_ignore_ascii_case(b"__construct");
        if m.is_empty() || (m.starts_with(b"__") && !is_ctor) {
            return false;
        }
        let mut mbuf = [0u8; 64];
        let Some(method_lower) = lower_into(m, &mut mbuf) else {
            return false;
        };
        let method_lower: &[u8] = method_lower;
        // The object operand (as read_operand() reads it).
        let obj = match op.op1 {
            OperandType::Cv(i) => match cvs.get(i as usize) {
                Some(Value::Object(o)) => o.clone(),
                Some(Value::Reference(r)) => match r.try_borrow().as_deref() {
                    Ok(Value::Object(o)) => o.clone(),
                    _ => return false,
                },
                _ => return false,
            },
            OperandType::Tmp(i) => match tmps.get(i as usize) {
                Some(Value::Object(o)) => o.clone(),
                _ => return false,
            },
            _ => return false,
        };
        let mut cbuf = [0u8; 128];
        let (class_lower, class_name_orig) = {
            let Ok(ob) = obj.try_borrow() else {
                return false;
            };
            if ob.closure.is_some() {
                return false;
            }
            // (the SPL checks on objects with internal state)
            if !is_ctor && ob.has_property(b"__spl_rii_its") {
                return false;
            }
            let Some(l) = lower_into(&ob.class_name, &mut cbuf) else {
                return false;
            };
            (l as &[u8], ob.class_name.clone())
        };
        let Some(class) = self.classes.get(class_lower) else {
            return false;
        };
        let Some(method) = class.methods.get(method_lower) else {
            // `new` of a class without a constructor: the call names
            // `Class::__construct`, and $this.
            if !is_ctor || !self.user_class_chain(class_lower) {
                return false;
            }
            let mut func_name = class_name_orig;
            func_name.extend_from_slice(b"::__construct");
            let mut args = self.take_values(0);
            args.push(Value::Object(obj));
            let fn_gen = self.user_functions.len() + 1;
            self.pending_calls.push(PendingCall {
                name: PhpString::from_vec(func_name),
                args,
                named_args: Vec::new(),
                target: CallTarget {
                    no_ctor: true,
                    fn_gen,
                    ..CallTarget::NONE
                },
            });
            return true;
        };
        if method.visibility != Visibility::Public
            || !method.attributes.is_empty()
            || !method.op_array.attributes.is_empty()
        {
            return false;
        }
        if !self.user_class_chain(class_lower) {
            return false;
        }
        let is_static = method.is_static;
        let method_op_array = method.op_array.clone();
        let mut func_name = class_name_orig;
        let sep = func_name.len();
        func_name.extend_from_slice(b"::");
        func_name.extend_from_slice(&method.name);
        // INIT_METHOD_CALL consumes a temporary object operand (`new` keeps
        // its result for the constructor call).
        let obj_val = match op.op1 {
            OperandType::Tmp(t) if !is_ctor => {
                std::mem::replace(&mut tmps[t as usize], Value::Undef)
            }
            _ => Value::Object(obj),
        };
        // Register the method as a user function (usually it is already).
        if !self
            .user_fn_lookup(&func_name)
            .is_some_and(|f| Rc::ptr_eq(f, &method_op_array))
        {
            self.user_functions
                .insert(func_name.to_ascii_lowercase(), method_op_array.clone());
        }
        let mut args = self.take_values(0);
        if !is_static {
            args.push(obj_val.clone());
        }
        // What DO_FCALL's fast path looks up again (see fast_fcall_target;
        // it leaves PhpToken::tokenize() to the general path).
        let target = if method_lower == b"tokenize" || func_name[0] == b'_' {
            CallTarget::NONE
        } else {
            CallTarget {
                user: Some(method_op_array),
                builtin: None,
                has_builtin: false,
                method: Some((sep, is_static)),
                no_ctor: false,
                fn_gen: self.user_functions.len() + 1,
            }
        };
        self.pending_calls.push(PendingCall {
            name: PhpString::from_vec(func_name),
            args,
            named_args: Vec::new(),
            target,
        });
        true
    }
}
