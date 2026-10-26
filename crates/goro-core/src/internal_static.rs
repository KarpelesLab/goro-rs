//! Static methods of internal classes implemented by the VM itself
//! (`WeakReference::create()`, `Closure::fromCallable()`, enum `from()`, ...),
//! and internal static methods invoked as callbacks.
//!
//! A direct `C::m()` call reaches these from the DoFCall handler. A string or
//! array callable naming one (`call_user_func('SplFixedArray::fromArray', $a)`,
//! `array_map([Suit::class, 'from'], $v)`) goes through
//! `call_internal_static()`, which does what zend_call_function() does for an
//! internal function: a frame, the zpp checks, the call.

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::PhpArray;
use crate::object::PhpObject;
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::{Vm, VmError};

/// An internal static method named by a callable.
pub struct InternalStaticTarget {
    /// The class the callable names (lowercase): the called class
    class_lower: Vec<u8>,
    /// The internal class implementing the method (lowercase)
    internal: Vec<u8>,
    method_lower: Vec<u8>,
    kind: TargetKind,
}

enum TargetKind {
    /// A method of a natively implemented (extension) class
    Native,
    /// A method the VM implements itself (`vm_static_builtin`)
    Vm,
    /// `from()` / `tryFrom()` / `cases()` of an enum
    Enum,
}

impl Vm {
    /// Whether `name_lower` ("class::method") is a static method of an
    /// internal class that `vm_static_builtin()` implements.
    pub(crate) fn is_vm_static_builtin(&self, name_lower: &[u8]) -> bool {
        self.vm_static_builtin_target(name_lower).is_some()
    }

    /// The name `vm_static_builtin()` runs for a call to `name_lower`
    /// ("class::method"): the method a user class inherits is its internal
    /// ancestor's. None when it is not such a method.
    pub(crate) fn vm_static_builtin_target(&self, name_lower: &[u8]) -> Option<Vec<u8>> {
        let sep = name_lower.windows(2).position(|w| w == b"::")?;
        let (class, method) = (&name_lower[..sep], &name_lower[sep + 2..]);
        match method {
            b"tokenize" => (class == b"phptoken" || self.class_extends(class, b"phptoken"))
                .then(|| name_lower.to_vec()),
            b"from" | b"tryfrom" | b"cases" => self
                .classes
                .get(class)
                .is_some_and(|c| c.is_enum)
                .then(|| name_lower.to_vec()),
            _ => {
                // The internal class: walk up from a user class that does
                // not declare the method itself.
                let mut cur = class.to_vec();
                for _ in 0..64 {
                    let Some(c) = self.classes.get(&cur).filter(|c| c.filename.is_some()) else {
                        break;
                    };
                    if let Some(m) = c.get_method(method) {
                        let decl = m.declaring_class.to_ascii_lowercase();
                        if decl.is_empty()
                            || self
                                .classes
                                .get(&decl)
                                .is_some_and(|d| d.filename.is_some())
                        {
                            return None;
                        }
                    }
                    cur = c.parent.as_ref()?.to_ascii_lowercase();
                }
                let known = matches!(
                    (cur.as_slice(), method),
                    (b"fiber", b"getcurrent")
                        | (b"fiber", b"suspend")
                        | (b"weakreference", b"create")
                        | (b"closure", b"getcurrent")
                        | (b"closure", b"fromcallable")
                        | (b"closure", b"bind")
                        | (b"closure", b"bindto")
                        | (b"splfixedarray", b"fromarray")
                );
                known.then(|| {
                    cur.extend_from_slice(b"::");
                    cur.extend_from_slice(method);
                    cur
                })
            }
        }
    }

    /// `Fiber::suspend()` called as a callback (call_user_func(), a
    /// ReflectionMethod, ...): its frame is already on the call stack. A
    /// FiberError, or an exception Fiber::throw() resumes it with, is left
    /// in `current_exception`.
    #[inline(never)]
    fn fiber_suspend_callback(&mut self, args: &[Value], line: u32) -> Value {
        let value = args.first().cloned().unwrap_or(Value::Null);
        let outside = self.fiber_depth == 0 && !crate::fiber::in_fiber();
        let msg = if self.current_fiber_force_closed() {
            Some("Cannot suspend in a force-closed fiber")
        } else if outside {
            Some("Cannot suspend outside of a fiber")
        } else if self.fiber_switch_blocking > 0 || !crate::fiber::in_fiber() {
            Some("Cannot switch fibers in current execution context")
        } else {
            None
        };
        if let Some(msg) = msg {
            let exc = self.create_exception(b"FiberError", msg, line);
            self.current_exception = Some(exc);
            return Value::Null;
        }
        match self.fiber_suspend_switch(value) {
            Some(crate::fiber::FiberResume::Value(v)) => v,
            Some(crate::fiber::FiberResume::Throw(exc)) => {
                self.current_exception = Some(exc);
                Value::Null
            }
            None => Value::Null,
        }
    }

    /// Run the VM-implemented static method `name_lower` ("class::method",
    /// see `is_vm_static_builtin()`) on `args`; `this` is the calling
    /// frame's `$this`, `strict` its strict_types. An exception it throws is left in
    /// `current_exception` (the result is then meaningless).
    pub(crate) fn vm_static_builtin(
        &mut self,
        name_lower: &[u8],
        args: &[Value],
        _named: &[(Vec<u8>, Value)],
        this: Option<Value>,
        strict: bool,
        line: u32,
    ) -> Value {
        match name_lower {
            b"fiber::getcurrent" => self.current_fiber.clone().unwrap_or(Value::Null),
            b"fiber::suspend" => self.fiber_suspend_callback(args, line),
            b"weakreference::create" => self.weakreference_create(args, line),
            b"closure::getcurrent" => {
                // The Closure of the calling frame.
                let depth = self.call_stack.len();
                while self.closure_frames.last().is_some_and(|f| f.0 > depth) {
                    self.closure_frames.pop();
                }
                let cur = self.closure_frames.last().and_then(|(d, name, c)| {
                    (*d == depth && self.call_stack.last().is_some_and(|f| f.0 == *name))
                        .then(|| c.upgrade().map(Value::Object))
                        .flatten()
                });
                match cur {
                    Some(c) => c,
                    None => {
                        let exc = self.create_exception(
                            b"Error",
                            "Current function is not a closure",
                            line,
                        );
                        self.current_exception = Some(exc);
                        Value::Null
                    }
                }
            }
            b"closure::fromcallable" => {
                let callable = args.first().cloned().unwrap_or(Value::Null);
                let scope = self.scope_top().cloned();
                match self.closure_from_callable(&callable, scope, this) {
                    Ok(c) => c,
                    Err(e) => {
                        if self.current_exception.is_none() {
                            // (thrown from the method's frame, unless a
                            // callback call pushed it)
                            let push = self
                                .call_stack
                                .last()
                                .is_none_or(|f| f.0 != "Closure::fromCallable");
                            if push {
                                self.call_stack.push((
                                    "Closure::fromCallable".to_string(),
                                    self.current_file.clone(),
                                    line,
                                    args.to_vec(),
                                    false,
                                ));
                            }
                            let exc = self.throw_type_error(format!(
                                "Failed to create closure from callable: {}",
                                e
                            ));
                            if push {
                                self.call_stack.pop();
                            }
                            self.current_exception = Some(exc);
                        }
                        Value::Null
                    }
                }
            }
            b"closure::bind" | b"closure::bindto" => {
                // Closure::bind($closure, $newThis, $newScope)
                let closure = args.first().cloned().unwrap_or(Value::Null);
                let new_this = args.get(1).cloned().unwrap_or(Value::Null);
                let scope = args
                    .get(2)
                    .cloned()
                    .unwrap_or(Value::String(PhpString::from_bytes(b"static")));
                let scope_provided = args.len() >= 3;
                self.bind_closure(&closure, new_this, scope, scope_provided)
            }
            b"splfixedarray::fromarray" => self.fa_from_array(args),
            _ => {
                let sep = name_lower
                    .windows(2)
                    .position(|w| w == b"::")
                    .unwrap_or(name_lower.len());
                let class = &name_lower[..sep];
                match name_lower.get(sep + 2..).unwrap_or(b"") {
                    b"tokenize" => self.phptoken_tokenize(class, args, line),
                    b"from" => self.enum_from(class, args, false, strict, line),
                    b"tryfrom" => self.enum_from(class, args, true, strict, line),
                    b"cases" => {
                        let cases: Vec<Vec<u8>> = match self.classes.get(class) {
                            Some(c) => c.enum_cases.iter().map(|(n, _)| n.clone()).collect(),
                            None => return Value::Null,
                        };
                        let mut arr = PhpArray::new();
                        for case_name in &cases {
                            arr.push(self.get_enum_case(class, case_name).unwrap_or(Value::Null));
                        }
                        Value::Array(Rc::new(RefCell::new(arr)))
                    }
                    _ => Value::Null,
                }
            }
        }
    }

    /// WeakReference::create(object $object): one WeakReference per object.
    fn weakreference_create(&mut self, args: &[Value], line: u32) -> Value {
        let arg = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
        let Value::Object(obj_rc) = &arg else {
            let type_name = match &arg {
                Value::Null => "null",
                Value::Long(_) => "int",
                Value::Double(_) => "float",
                Value::String(_) => "string",
                Value::True | Value::False => "bool",
                Value::Array(_) => "array",
                _ => "unknown",
            };
            let msg = format!(
                "WeakReference::create(): Argument #1 ($object) must be of type object, {} given",
                type_name
            );
            let exc = self.create_exception(b"TypeError", &msg, line);
            self.current_exception = Some(exc);
            return Value::Null;
        };
        let target_oid = obj_rc.borrow().object_id;
        if let Some((_, wr_val)) = self.weak_ref_cache.get(&target_oid) {
            return wr_val.clone();
        }
        let weak = Rc::downgrade(obj_rc);
        let weak_id = self.next_weak_ref_id;
        self.next_weak_ref_id += 1;
        self.weak_refs.insert(weak_id, weak);
        let wr_id = self.alloc_object_id();
        let mut wr_obj = PhpObject::new(b"WeakReference".to_vec(), wr_id);
        wr_obj.set_property(b"__weak_ref_id".to_vec(), Value::Long(weak_id as i64));
        let wr_val = Value::Object(Rc::new(RefCell::new(wr_obj)));
        self.weak_ref_cache
            .insert(target_oid, (weak_id, wr_val.clone()));
        wr_val
    }

    /// `from()` / `tryFrom()` of the backed enum `class_part` (lowercase).
    fn enum_from(
        &mut self,
        class_part: &[u8],
        args: &[Value],
        try_from: bool,
        strict: bool,
        line: u32,
    ) -> Value {
        let arg = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
        let Some(class) = self.classes.get(class_part).cloned() else {
            return Value::Null;
        };
        let class_name = String::from_utf8_lossy(&class.name).into_owned();
        let method = if try_from { "tryFrom" } else { "from" };
        let Some(bt) = class.enum_backing_type.as_ref() else {
            // A pure enum has no from()/tryFrom() (BackedEnum declares them).
            let msg = format!("Call to undefined method {}::{}()", class_name, method);
            let exc = self.create_exception(b"Error", &msg, line);
            self.current_exception = Some(exc);
            return Value::Null;
        };
        // Z_PARAM_STR_OR_LONG: null is deprecated in weak mode and becomes
        // the backing type's empty value.
        let arg = match arg {
            Value::Null | Value::Undef if !strict => {
                self.emit_deprecated_at(
                    &format!(
                        "{}::{}(): Passing null to parameter #1 ($value) of type string|int is deprecated",
                        class_name, method
                    ),
                    line,
                );
                if self.current_exception.is_some() {
                    return Value::Null;
                }
                if bt.eq_ignore_ascii_case(b"int") {
                    Value::Long(0)
                } else {
                    Value::String(crate::string::PhpString::empty())
                }
            }
            a => a,
        };
        // Z_PARAM_LONG / Z_PARAM_STR: weak mode coerces scalars.
        let arg = self.enum_from_weak_int(bt, arg, strict, line);
        let is_int_backed = bt.eq_ignore_ascii_case(b"int");
        if (is_int_backed && !matches!(arg, Value::Long(_)))
            || (!is_int_backed && strict && !matches!(arg, Value::String(_)))
        {
            let msg = format!(
                "{}::{}(): Argument #1 ($value) must be of type {}, {} given",
                class_name,
                if try_from { "tryFrom" } else { "from" },
                if is_int_backed { "int" } else { "string" },
                Self::value_type_name(&arg)
            );
            let exc = self.create_exception(b"TypeError", &msg, line);
            self.current_exception = Some(exc);
            return Value::Null;
        }
        if let Some((exc_class, err_msg)) = self.enum_backed_table_error(class_part) {
            // An empty class: the value expression's exception is pending.
            if !exc_class.is_empty() {
                let exc = self.create_exception(exc_class, &err_msg, line);
                self.current_exception = Some(exc);
            }
            return Value::Null;
        }
        if self.init_backed_enum_cases(class_part).is_none() {
            return Value::Null;
        }
        let is_string = bt.eq_ignore_ascii_case(b"string");
        let lookup_arg = if is_string && !matches!(arg, Value::String(_)) {
            Value::String(arg.to_php_string())
        } else {
            arg
        };
        let found = class
            .enum_cases
            .iter()
            .find(|(_, v)| self.resolve_deferred_value(v).identical(&lookup_arg))
            .map(|(n, _)| n.clone());
        if let Some(case_name) = found {
            return self
                .get_enum_case(class_part, &case_name)
                .unwrap_or(Value::Null);
        }
        if try_from {
            return Value::Null;
        }
        let arg_str = lookup_arg.to_php_string().to_string_lossy();
        let msg = if is_string {
            format!(
                "\"{}\" is not a valid backing value for enum {}",
                arg_str, class_name
            )
        } else {
            format!(
                "{} is not a valid backing value for enum {}",
                arg_str, class_name
            )
        };
        let exc = self.create_exception(b"ValueError", &msg, line);
        self.current_exception = Some(exc);
        Value::Null
    }

    /// PhpToken::tokenize(string $code, int $flags = 0): array, called on
    /// PhpToken or a subclass `class_part` (lowercase) whose instances it
    /// creates (see tokenizer.rs). An exception leaves NULL.
    fn phptoken_tokenize(&mut self, class_part: &[u8], args: &[Value], line: u32) -> Value {
        let _ = line;
        crate::tokenizer::phptoken_tokenize(self, class_part, args).unwrap_or(Value::Null)
    }

    /// The internal static method a string ("C::m") or array (["C", "m"])
    /// callable names, when it is one the callback machinery must dispatch
    /// itself (user methods, closures and relative class names are left to
    /// the callers).
    pub fn internal_static_target(&mut self, callback: &Value) -> Option<InternalStaticTarget> {
        let (class, method): (Vec<u8>, Vec<u8>) = match callback {
            Value::String(s) => {
                let b = s.as_bytes();
                let sep = b.windows(2).rposition(|w| w == b"::")?;
                (b[..sep].to_vec(), b[sep + 2..].to_vec())
            }
            Value::Array(a) => {
                let a = a.borrow();
                if a.len() != 2 {
                    return None;
                }
                let mut it = a.values();
                let (Value::String(c), Value::String(m)) = (it.next()?.deref(), it.next()?.deref())
                else {
                    return None;
                };
                if m.as_bytes().windows(2).any(|w| w == b"::") {
                    return None;
                }
                (c.as_bytes().to_vec(), m.as_bytes().to_vec())
            }
            _ => return None,
        };
        let class_lower = class
            .strip_prefix(b"\\")
            .unwrap_or(&class)
            .to_ascii_lowercase();
        let method_lower = method.to_ascii_lowercase();
        if class_lower.is_empty()
            || matches!(class_lower.as_slice(), b"self" | b"parent" | b"static")
            || class_lower.starts_with(b"__")
        {
            return None;
        }
        // A user method (or one a user class inherits from a user class)
        if let Some(c) = self.classes.get(&class_lower) {
            if let Some(m) = c.get_method(&method_lower) {
                let decl = m.declaring_class.to_ascii_lowercase();
                let user = decl.is_empty()
                    || self
                        .classes
                        .get(&decl)
                        .is_some_and(|d| d.filename.is_some());
                if user {
                    return None;
                }
            }
            if c.is_enum
                && (method_lower == b"cases"
                    || (c.enum_backing_type.is_some()
                        && matches!(method_lower.as_slice(), b"from" | b"tryfrom")))
            {
                return Some(InternalStaticTarget {
                    internal: class_lower.clone(),
                    class_lower,
                    method_lower,
                    kind: TargetKind::Enum,
                });
            }
        }
        if let Some((native, h)) = self.find_native_class_handler(&class_lower) {
            if (h.has_method)(&native, &method_lower, true) {
                return Some(InternalStaticTarget {
                    class_lower,
                    internal: native,
                    method_lower,
                    kind: TargetKind::Native,
                });
            }
            return None;
        }
        let internal = self
            .zpp_internal_class(&class_lower)
            .or_else(|| (!self.classes.contains_key(&class_lower)).then(|| class_lower.clone()))?;
        let mut name = if method_lower == b"tokenize" {
            class_lower.clone()
        } else {
            internal.clone()
        };
        name.extend_from_slice(b"::");
        name.extend_from_slice(&method_lower);
        if !self.is_vm_static_builtin(&name) {
            return None;
        }
        // Instance methods (Closure::bindTo) are not callable statically.
        if crate::stubs::find_method(&internal, &method_lower).is_some_and(|(_, m)| !m.is_static())
        {
            return None;
        }
        Some(InternalStaticTarget {
            class_lower,
            internal,
            method_lower,
            kind: TargetKind::Vm,
        })
    }

    /// Call the internal static method `t` as a callback with `args` and
    /// named arguments `named` (zend_call_function()): a frame for it, the
    /// zpp checks, the call.
    pub fn call_internal_static(
        &mut self,
        t: InternalStaticTarget,
        args: &[Value],
        mut named: Vec<(Vec<u8>, Value)>,
    ) -> Result<Value, VmError> {
        let line = self.current_line;
        let method_display: String = match t.kind {
            TargetKind::Enum => match t.method_lower.as_slice() {
                b"tryfrom" => "tryFrom".into(),
                m => crate::string::msg_str(m).into_owned(),
            },
            _ => crate::stubs::find_method(&t.internal, &t.method_lower)
                .map(|(_, m)| m.name.to_string())
                .unwrap_or_else(|| crate::string::msg_str(&t.method_lower).into_owned()),
        };
        let class_display = match t.kind {
            TargetKind::Native => self.builtin_canonical_name(&t.internal),
            _ => self.callable_class_name(&t.internal),
        };
        let frame_name = format!("{}::{}", class_display, method_display);
        // Located at the call site when called through a direct call_user_func()
        let ucf = self.ucf_call_line.take();
        let (file, fline) = match ucf {
            Some(l) => (self.current_file.clone(), l),
            None => (crate::rcstr::RcStr::new(), 0),
        };
        let mut args: Vec<Value> = args.to_vec();
        let had_exc = self.current_exception.is_some();
        let fail = |vm: &mut Vm| VmError {
            message: vm.uncaught_exception_message(),
            line,
        };
        // Closure::getCurrent() looks at its caller's frame.
        let own_frame = !(t.method_lower == b"getcurrent" && ucf.is_some());
        if own_frame {
            let shown: Vec<Value> = args.iter().map(|v| v.deref()).collect();
            self.call_stack
                .push((frame_name.clone(), file, fline, shown, false));
        }
        let pop = |vm: &mut Vm| {
            if own_frame {
                vm.call_stack.pop();
            }
        };
        // zpp
        let zpp_err = match t.kind {
            TargetKind::Enum => {
                let (want, given) = (usize::from(t.method_lower != b"cases"), args.len());
                if let Some((n, _)) = named.first() {
                    Some((
                        &b"Error"[..],
                        format!("Unknown named parameter ${}", String::from_utf8_lossy(n)),
                    ))
                } else if given != want {
                    Some((
                        &b"ArgumentCountError"[..],
                        format!(
                            "{}() expects exactly {} argument{}, {} given",
                            frame_name,
                            want,
                            if want == 1 { "" } else { "s" },
                            given
                        ),
                    ))
                } else {
                    None
                }
            }
            _ => {
                if self
                    .zpp_method_call(
                        &t.internal,
                        &t.method_lower,
                        &mut args,
                        0,
                        &mut named,
                        None,
                        false,
                        line,
                    )
                    .is_err()
                {
                    pop(self);
                    return Err(fail(self));
                }
                None
            }
        };
        if let Some((cls, msg)) = zpp_err {
            let exc = self.create_exception(cls, &msg, line);
            pop(self);
            self.current_exception = Some(exc);
            return Err(fail(self));
        }
        let result = match t.kind {
            TargetKind::Native => {
                let (native, h) = match self.find_native_class_handler(&t.class_lower) {
                    Some(x) => x,
                    None => {
                        pop(self);
                        return Ok(Value::Null);
                    }
                };
                match self.merge_native_named_args(&native, &t.method_lower, &mut args, 0, &named) {
                    Ok(()) => {
                        let called = self
                            .classes
                            .get(&t.class_lower)
                            .map(|c| c.name.clone())
                            .unwrap_or_else(|| self.builtin_canonical_name(&t.class_lower).into());
                        h.invoke(self, &native, &t.method_lower, &called, None, &args)
                    }
                    Err(n) => {
                        let msg =
                            format!("Unknown named parameter ${}", crate::string::msg_str(&n));
                        let exc = self.create_exception(b"Error", &msg, line);
                        self.current_exception = Some(exc);
                        Value::Null
                    }
                }
            }
            TargetKind::Vm | TargetKind::Enum => {
                let mut name =
                    if t.method_lower == b"tokenize" || matches!(t.kind, TargetKind::Enum) {
                        t.class_lower.clone()
                    } else {
                        t.internal.clone()
                    };
                name.extend_from_slice(b"::");
                name.extend_from_slice(&t.method_lower);
                // Arguments zpp did not consume by name
                if let Some((n, _)) = named
                    .iter()
                    .find(|(n, _)| !(name.ends_with(b"::parse") && n.as_slice() == b"errors"))
                {
                    let msg = format!("Unknown named parameter ${}", crate::string::msg_str(n));
                    let exc = self.create_exception(b"Error", &msg, line);
                    self.current_exception = Some(exc);
                    Value::Null
                } else {
                    let this = self.current_frame_this();
                    // (called from internal code: weak mode)
                    self.vm_static_builtin(&name, &args, &named, this, false, line)
                }
            }
        };
        pop(self);
        if !had_exc && self.current_exception.is_some() {
            return Err(fail(self));
        }
        Ok(result)
    }
}
