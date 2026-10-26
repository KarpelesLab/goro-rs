//! Dimension fetches in a write context (php-src zend_fetch_dimension_address
//! for BP_VAR_W / BP_VAR_RW / BP_VAR_UNSET), used for the containers of a
//! nested write such as `$a[k][j] = v`, `$a[k][j] .= v` or `unset($a[k][j])`.
//!
//! The fetched element is returned as a value the next opcode writes into: an
//! array `Rc` that is the element's own storage (vivified and separated), an
//! object, or a plain value (scalars are rejected by the consumer).

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::PhpArray;
use crate::value::Value;
use crate::vm::Vm;

/// Which write context a dimension container is fetched for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DimFetch {
    /// FETCH_DIM_W: missing elements are created silently.
    W,
    /// FETCH_DIM_RW: missing variables / keys are warned about, then created.
    Rw,
    /// FETCH_DIM_UNSET: nothing is created.
    Unset,
    /// FETCH_DIM_W for the container of an object write (`$a[k]->p = v`):
    /// a missing element is created as null, not vivified to an array.
    Obj,
    /// FETCH_DIM_RW for the container of an object read-modify-write
    /// (`$a[k]->p += v`): a missing element is warned about, then created
    /// as null.
    ObjRw,
}

/// Where a string handed out by a write fetch lives. Strings are values, so
/// the fetch result is a copy; a string offset write into it is stored back
/// here (php-src writes through the INDIRECT zval instead).
pub(crate) enum StrWb {
    /// An array element.
    Elem(Rc<RefCell<PhpArray>>, crate::array::ArrayKey),
    /// A reference cell (a referenced element or property).
    Ref(Rc<RefCell<Value>>),
    /// An object property slot.
    Prop(Rc<RefCell<crate::object::PhpObject>>, Vec<u8>),
    /// A static property (lowercase class key, name).
    Static(Vec<u8>, Vec<u8>),
}

/// Outcome of zend_assign_to_string_offset().
pub(crate) enum StrOffsetAssign {
    /// The new string and the expression's value (the byte written).
    Written(Vec<u8>, Value),
    /// Nothing written (a warning was raised); the expression is null.
    Skipped,
    /// An exception is pending.
    Threw,
}

impl Vm {
    /// zend_assign_to_string_offset(): `s[dim] = value`.
    pub(crate) fn assign_string_offset(
        &mut self,
        s: &[u8],
        dim: &Value,
        value: &Value,
        line: u32,
    ) -> StrOffsetAssign {
        let dim = dim.deref();
        let offset = match &dim {
            Value::Long(n) => *n,
            other => {
                if !self.check_string_offset_w(other, DimFetch::W, line) {
                    return StrOffsetAssign::Threw;
                }
                if self.current_exception.is_some() {
                    return StrOffsetAssign::Threw;
                }
                match other {
                    Value::String(k) => {
                        let t = k.as_bytes();
                        let t = t
                            .iter()
                            .position(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
                            .map(|p| &t[p..])
                            .unwrap_or(b"");
                        let end = t
                            .iter()
                            .enumerate()
                            .position(|(i, b)| {
                                !(b.is_ascii_digit() || (i == 0 && matches!(b, b'-' | b'+')))
                            })
                            .unwrap_or(t.len());
                        std::str::from_utf8(&t[..end])
                            .ok()
                            .and_then(|x| x.parse::<i64>().ok())
                            .unwrap_or(0)
                    }
                    Value::Double(d) => {
                        if d.is_finite() && crate::value::php_double_fits_long(*d) {
                            *d as i64
                        } else {
                            0
                        }
                    }
                    Value::True => 1,
                    _ => 0,
                }
            }
        };
        let len = s.len() as i64;
        if offset < -len {
            self.emit_warning_at(&format!("Illegal string offset {}", offset), line);
            return if self.current_exception.is_some() {
                StrOffsetAssign::Threw
            } else {
                StrOffsetAssign::Skipped
            };
        }
        let offset = if offset < 0 { offset + len } else { offset } as usize;
        let value = value.deref();
        let bytes = match &value {
            Value::String(v) => v.as_bytes().to_vec(),
            // (zval_try_get_string_func())
            Value::Array(_) => {
                self.emit_warning_at("Array to string conversion", line);
                if self.current_exception.is_some() {
                    return StrOffsetAssign::Threw;
                }
                b"Array".to_vec()
            }
            other => match self.value_to_string_checked(other) {
                Ok(v) => v.as_bytes().to_vec(),
                Err(_) => return StrOffsetAssign::Threw,
            },
        };
        if self.current_exception.is_some() {
            return StrOffsetAssign::Threw;
        }
        if bytes.is_empty() {
            self.throw_error_msg(b"Error", "Cannot assign an empty string to a string offset");
            return StrOffsetAssign::Threw;
        }
        if bytes.len() > 1 {
            self.emit_warning_at(
                "Only the first byte will be assigned to the string offset",
                line,
            );
            if self.current_exception.is_some() {
                return StrOffsetAssign::Threw;
            }
        }
        let c = bytes[0];
        let mut out = s.to_vec();
        if offset >= out.len() {
            out.resize(offset, b' ');
            out.push(c);
        } else {
            out[offset] = c;
        }
        StrOffsetAssign::Written(
            out,
            Value::String(crate::string::PhpString::from_vec(vec![c])),
        )
    }

    /// Store a string back where a write fetch found it.
    pub(crate) fn str_wb_store(&mut self, loc: &StrWb, v: Value) {
        match loc {
            StrWb::Elem(arr, k) => {
                let mut a = arr.borrow_mut();
                match a.get_mut(k) {
                    Some(Value::Reference(r)) => *r.borrow_mut() = v,
                    Some(slot) => *slot = v,
                    None => a.set(k.clone(), v),
                }
            }
            StrWb::Ref(r) => *r.borrow_mut() = v,
            StrWb::Static(class_key, name) => {
                self.static_prop_put_raw(class_key, name, v);
            }
            StrWb::Prop(o, name) => {
                let mut ob = o.borrow_mut();
                if let Some(slot) = ob
                    .properties
                    .iter_mut()
                    .find(|(n, _)| n.as_slice() == name.as_slice())
                {
                    match &mut slot.1 {
                        Value::Reference(r) => *r.borrow_mut() = v,
                        s => *s = v,
                    }
                }
            }
        }
    }

    /// Fetch `slot[key]` (`key == None` is `slot[]`) for a nested write.
    /// `slot` is the container: a CV slot (possibly holding a reference) or a
    /// tmp holding an earlier fetch. `undef_cv` names the CV for the
    /// "Undefined variable" warning. Returns `None` when an exception is
    /// pending.
    pub(crate) fn fetch_dim_for_write(
        &mut self,
        slot: &mut Value,
        key: Option<Value>,
        mode: DimFetch,
        undef_cv: Option<&[u8]>,
        line: u32,
    ) -> Option<Value> {
        let cell = match slot {
            Value::Reference(r) => Some(r.clone()),
            _ => None,
        };
        let cur = match &cell {
            Some(r) => r.borrow().clone(),
            None => std::mem::replace(slot, Value::Undef),
        };
        let restore = |slot: &mut Value, v: Value| {
            if cell.is_none() {
                *slot = v;
            }
        };
        match cur {
            Value::Array(rc) => {
                // Copy-on-write: an array the slot shares with another holder
                // (through a reference cell: the cell holds one count, `cur`
                // the other) is separated before it is written.
                // (a plain CV owns its array: arrays are copied when assigned)
                let rc = if cell.is_some()
                    && crate::array::shared_beyond(&rc, 2)
                    && !Rc::ptr_eq(&rc, &self.globals)
                {
                    let sep = Rc::new(RefCell::new(rc.borrow().clone()));
                    if let Some(r) = &cell {
                        *r.borrow_mut() = Value::Array(sep.clone());
                    }
                    sep
                } else {
                    rc
                };
                restore(slot, Value::Array(rc.clone()));
                self.fetch_dim_from_array(&rc, key, mode, line)
            }
            Value::Undef | Value::Null | Value::False => {
                if mode != DimFetch::W && matches!(cur, Value::Undef) && cell.is_none() {
                    if let Some(name) = undef_cv {
                        self.emit_warning_at(
                            &format!("Undefined variable ${}", crate::string::msg_str(name)),
                            line,
                        );
                    }
                }
                // (FETCH_DIM_W converts a reference to false silently)
                let quiet = match &cell {
                    Some(_) if undef_cv.is_some() => self.dim_w_cv_ref,
                    Some(_) => true,
                    None => false,
                };
                if matches!(cur, Value::False) && !quiet {
                    self.emit_deprecated_at(
                        "Automatic conversion of false to array is deprecated",
                        line,
                    );
                }
                if mode == DimFetch::Unset {
                    restore(slot, cur);
                    return Some(Value::Null);
                }
                // A reference bound to typed properties must accept the array.
                if let Some(r) = &cell {
                    if let Err(msg) =
                        self.typed_ref_auto_init_check(Some(&Value::Reference(r.clone())))
                    {
                        self.throw_error_msg(b"TypeError", &msg);
                        return None;
                    }
                }
                let rc = Rc::new(RefCell::new(PhpArray::new()));
                match &cell {
                    Some(r) => *r.borrow_mut() = Value::Array(rc.clone()),
                    None => *slot = Value::Array(rc.clone()),
                }
                self.fetch_dim_from_array(&rc, key, mode, line)
            }
            Value::Object(_) => {
                let obj = cur.clone();
                restore(slot, cur);
                self.fetch_dim_from_object(&obj, key, mode, line)
            }
            Value::String(s) => {
                restore(slot, Value::String(s));
                match key {
                    None => self.throw_error_msg(b"Error", "[] operator not supported for strings"),
                    Some(k) => {
                        if self.check_string_offset_w(&k, mode, line) {
                            let msg = if std::mem::take(&mut self.str_wb_ref_ctx) {
                                "Cannot create references to/from string offsets"
                            } else if matches!(mode, DimFetch::Obj | DimFetch::ObjRw)
                                || self.str_wb_obj_ctx
                            {
                                "Cannot use string offset as an object"
                            } else {
                                "Cannot use string offset as an array"
                            };
                            self.throw_error_msg(b"Error", msg);
                        }
                    }
                }
                None
            }
            other => {
                restore(slot, other);
                if mode == DimFetch::Unset {
                    self.throw_error_msg(b"Error", "Cannot unset offset in a non-array variable");
                } else {
                    self.throw_error_msg(b"Error", "Cannot use a scalar value as an array");
                }
                None
            }
        }
    }

    /// Whether read_dimension() of `obj` hands out the element itself in a
    /// write context (ArrayObject storage, WeakMap values), so writes through
    /// it are not "indirect modifications".
    pub(crate) fn dim_fetch_native_ref(&self, obj: &Rc<RefCell<crate::object::PhpObject>>) -> bool {
        let cl: Vec<u8> = obj
            .borrow()
            .class_name
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        if cl == b"weakmap" {
            return true;
        }
        self.spl_find_base(&cl, &[b"arrayobject", b"arrayiterator"])
            .is_some()
            && !self.ao_user_has(obj, b"offsetget")
    }

    /// zend_std_read_property() in a write context (FETCH_OBJ_W/RW/UNSET)
    /// for a `__get()` result: a by-value non-object result is a copy the
    /// write cannot reach.
    pub(crate) fn magic_get_w_result(
        &mut self,
        result: Value,
        class_name: &[u8],
        prop: &[u8],
        line: u32,
    ) -> Value {
        if self.last_call_by_ref {
            return result.deref();
        }
        match result.deref() {
            Value::Object(o) => Value::Object(o),
            other => {
                self.emit_notice_at(
                    &format!(
                        "Indirect modification of overloaded property {}::${} has no effect",
                        crate::value::display_class_name(class_name),
                        crate::string::msg_str(prop)
                    ),
                    line,
                );
                match other {
                    Value::Array(rc) => {
                        let copy = rc.borrow().clone();
                        Value::Array(Rc::new(RefCell::new(copy)))
                    }
                    v => v,
                }
            }
        }
    }

    /// A get hook result fetched for writing: a by-value non-object result
    /// throws "Indirect modification of C::$p is not allowed". Returns false
    /// when it threw.
    pub(crate) fn hook_get_w_check(&mut self, val: &Value, class_name: &[u8], prop: &[u8]) -> bool {
        if self.last_call_by_ref || matches!(val.deref(), Value::Object(_)) {
            return true;
        }
        let msg = format!(
            "Indirect modification of {}::${} is not allowed",
            crate::value::display_class_name(class_name),
            crate::string::msg_str(prop)
        );
        self.throw_error_msg(b"Error", &msg);
        false
    }

    /// The property's hooks `(has_get, has_set)` when accessed from outside
    /// its own hooks; `None` for a plain property or from inside a hook.
    pub(crate) fn prop_hooks_outside(
        &self,
        class_lower: &[u8],
        prop: &[u8],
    ) -> Option<(bool, bool)> {
        let c = self.classes.get(class_lower)?;
        let p = c
            .properties
            .iter()
            .find(|p| p.name == prop && !p.is_static)?;
        let (g, st) = (p.has_get_hook, p.has_set_hook);
        if !g && !st {
            return None;
        }
        // (an inaccessible hooked property takes the plain path's error)
        if !self.hook_runs_for_scope(class_lower, &c.name, prop, &p.declaring_class) {
            return None;
        }
        let mut base = class_lower.to_vec();
        base.extend_from_slice(b"::");
        base.extend_from_slice(prop);
        if self.in_prop_hook(&base, None) {
            return None;
        }
        Some((g, st))
    }

    /// Run the get hook of `prop` (outside of it) and return its result.
    pub(crate) fn call_prop_get_hook(
        &mut self,
        obj_val: &Value,
        class_lower: &[u8],
        prop: &[u8],
    ) -> Value {
        let mut key = class_lower.to_vec();
        key.extend_from_slice(b"::");
        key.extend_from_slice(prop);
        key.extend_from_slice(b"::get");
        let key = Self::prop_hook_key(key, obj_val);
        let mut m = b"__property_get_".to_vec();
        m.extend_from_slice(prop);
        // A `&get` hook hands out the reference it returns.
        let by_ref = self
            .classes
            .get(class_lower)
            .and_then(|c| c.get_method(&m.to_ascii_lowercase()))
            .is_some_and(|md| md.op_array.returns_by_ref);
        self.last_call_by_ref = false;
        self.property_hook_stack.push(key);
        if by_ref {
            self.keep_ref_depth = Some(self.call_depth + 1);
        }
        let r = self.call_object_method(obj_val, &m, &[]);
        if by_ref {
            self.keep_ref_depth = None;
        }
        self.property_hook_stack.pop();
        let r = r.unwrap_or(Value::Null);
        if by_ref && matches!(r, Value::Reference(_)) {
            self.last_call_by_ref = true;
        }
        r
    }

    /// The (lowercase) internal function a pending call goes to, if any:
    /// an unqualified call in a namespace falls back to the global one.
    pub(crate) fn pending_builtin_name(&self, call: &crate::vm::PendingCall) -> Option<Vec<u8>> {
        let name = call.name.as_bytes();
        // A call resolved to a user function of its name when it was set up
        // (the name it looked up: no leading backslash).
        if call.target.user.is_some()
            && call.target.fn_gen == self.user_functions.len() + 1
            && name.first() != Some(&b'\\')
        {
            return None;
        }
        let name = name.strip_prefix(b"\\").unwrap_or(name);
        if name.contains(&b':') || self.user_fn_lookup(name).is_some() {
            return None;
        }
        let lower = name.to_ascii_lowercase();
        if self.functions.contains_key(&lower) {
            return Some(lower);
        }
        let short = &lower[lower.iter().rposition(|&b| b == b'\\')? + 1..];
        (self.user_fn_lookup(short).is_none() && self.functions.contains_key(short))
            .then(|| short.to_vec())
    }

    /// Whether parameter `arg_index` of the internal function `call` is
    /// taken by reference (not prefer-ref).
    pub(crate) fn builtin_arg_must_be_ref(
        &self,
        call: &crate::vm::PendingCall,
        arg_index: usize,
    ) -> bool {
        // (the arginfo the call target resolved when the call was set up)
        if call.target.fn_gen == self.user_functions.len() + 1 && call.target.user.is_none() {
            if let Some((_, Some((fi, _)))) = &call.target.builtin {
                let p = fi
                    .params
                    .get(arg_index)
                    .or_else(|| fi.params.last().filter(|p| p.variadic));
                return p.is_some_and(|p| p.by_ref == 1);
            }
        }
        let Some(lower) = self.pending_builtin_name(call) else {
            return false;
        };
        crate::arginfo::lookup(&lower).is_some_and(|fi| {
            let p = fi
                .params
                .get(arg_index)
                .or_else(|| fi.params.last().filter(|p| p.variadic));
            p.is_some_and(|p| p.by_ref == 1)
        })
    }

    /// Whether argument `arg_index` of the dynamic call `call` (`$f(...)`)
    /// goes to a by-reference parameter of an internal function (which
    /// the compiler could not know of). The builtins taking `array
    /// &$array` share the array handle instead (as their direct calls do).
    pub(crate) fn dynamic_builtin_arg_by_ref(
        &self,
        call: &crate::vm::PendingCall,
        arg_index: usize,
    ) -> bool {
        if !call.target.dynamic {
            return false;
        }
        // `$f()` with $f an [object|class, method] array (dispatched by
        // call_user_func, the callable first): the method's parameters.
        if call.name.as_bytes() == b"call_user_func" {
            // (an instance method's parameters follow its $this)
            return arg_index > 0
                && self
                    .array_callable_method(&call.args[0])
                    .is_some_and(|(f, this)| crate::vm::arg_by_ref(&f, arg_index - 1 + this));
        }
        self.builtin_arg_must_be_ref(call, arg_index)
            && !crate::compiler::Compiler::is_array_ref_builtin(Some(call.name.as_bytes()))
    }

    /// The user method an `[object|class, method]` callable names.
    fn array_callable_method(
        &self,
        callable: &Value,
    ) -> Option<(std::rc::Rc<crate::opcode::OpArray>, usize)> {
        let Value::Array(a) = callable else {
            return None;
        };
        let a = a.borrow();
        let (target, method) = (a.get_int(0)?.deref(), a.get_int(1)?.deref());
        let Value::String(m) = method else {
            return None;
        };
        let class = match &target {
            Value::Object(o) => o.borrow().class_name.to_ascii_lowercase(),
            Value::String(c) => c.as_bytes().to_ascii_lowercase(),
            _ => return None,
        };
        let class = class.strip_prefix(b"\\").unwrap_or(&class);
        let ce = self.classes.get(class)?;
        let m = ce.get_method(&m.as_bytes().to_ascii_lowercase())?;
        Some((m.op_array.clone(), usize::from(!m.is_static)))
    }

    /// zend_cannot_pass_by_reference() message for argument `arg_index`
    /// (0-based) of the pending call.
    pub(crate) fn cannot_pass_by_ref_msg(&self, arg_index: usize) -> String {
        if let Some(call) = self.pending_calls.last() {
            if self.builtin_arg_must_be_ref(call, arg_index) {
                let lower = self.pending_builtin_name(call).unwrap_or_default();
                if let Some(fi) = crate::arginfo::lookup(&lower) {
                    return match fi.params.get(arg_index).filter(|p| !p.variadic) {
                        Some(p) => format!(
                            "{}(): Argument #{} (${}) could not be passed by reference",
                            fi.name,
                            arg_index + 1,
                            p.name
                        ),
                        None => format!(
                            "{}(): Argument #{} could not be passed by reference",
                            fi.name,
                            arg_index + 1
                        ),
                    };
                }
            }
        }
        // An extension's native method ("__spl::class::method", $this first)
        if let Some(call) = self.pending_calls.last() {
            if let Some(rest) = call.name.as_bytes().strip_prefix(b"__spl::") {
                if let Some(sep) = rest.windows(2).position(|w| w == b"::") {
                    let (class, method) = (&rest[..sep], &rest[sep + 2..]);
                    if self.native_class_handlers.contains_key(class) {
                        if let Some((c, m)) = crate::stubs::find_method(class, method) {
                            let p = arg_index.saturating_sub(1);
                            let pname = m
                                .args
                                .get(p)
                                .filter(|a| a.flags & crate::stubs::A_VARIADIC == 0)
                                .map(|a| a.name);
                            return match pname {
                                Some(pn) => format!(
                                    "{}::{}(): Argument #{} (${}) could not be passed by reference",
                                    c.name, m.name, arg_index, pn
                                ),
                                None => format!(
                                    "{}::{}(): Argument #{} could not be passed by reference",
                                    c.name, m.name, arg_index
                                ),
                            };
                        }
                    }
                }
            }
        }
        self.cannot_pass_by_ref_user_msg(arg_index)
    }

    /// The pending user call's display name ("f", "C::m") and whether its
    /// first argument is $this (1) or not (0).
    pub(crate) fn pending_callee_display(&self) -> (String, usize) {
        let Some(call) = self.pending_calls.last() else {
            return (String::new(), 0);
        };
        let lower: Vec<u8> = call.name.as_bytes().to_ascii_lowercase();
        match self.user_functions.get(&lower) {
            Some(f) => {
                let is_method = f.cv_names.first().map(|n| n.as_slice()) == Some(b"this")
                    && matches!(call.args.first(), Some(Value::Object(_)));
                let disp =
                    if call.name.as_bytes().windows(2).any(|w| w == b"::") || f.name.is_empty() {
                        call.name.to_string_lossy().to_string()
                    } else {
                        String::from_utf8_lossy(f.effective_name()).to_string()
                    };
                (disp, usize::from(is_method))
            }
            None => (call.name.to_string_lossy().to_string(), 0),
        }
    }

    fn cannot_pass_by_ref_user_msg(&self, arg_index: usize) -> String {
        let (fname, pname, num) = self
            .pending_calls
            .last()
            .and_then(|call| {
                let lower: Vec<u8> = call.name.as_bytes().to_ascii_lowercase();
                self.user_functions.get(&lower).map(|f| {
                    // Methods carry $this as their first argument.
                    let is_method = f.cv_names.first().map(|n| n.as_slice()) == Some(b"this")
                        && matches!(call.args.first(), Some(Value::Object(_)));
                    let disp = if call.name.as_bytes().windows(2).any(|w| w == b"::")
                        || f.name.is_empty()
                    {
                        call.name.to_string_lossy().to_string()
                    } else {
                        String::from_utf8_lossy(f.effective_name()).to_string()
                    };
                    let num = if is_method { arg_index } else { arg_index + 1 };
                    // (a variadic parameter is not named)
                    let pname = f
                        .cv_names
                        .get(arg_index)
                        .filter(|_| f.variadic_param.is_none_or(|v| (arg_index as u32) < v))
                        .cloned();
                    (disp, pname, num)
                })
            })
            .unwrap_or_default();
        match pname {
            Some(p) => format!(
                "{}(): Argument #{} (${}) could not be passed by reference",
                fname,
                num,
                String::from_utf8_lossy(&p)
            ),
            None => format!(
                "{}(): Argument #{} could not be passed by reference",
                fname, num
            ),
        }
    }

    /// FETCH_OBJ_IS on an overloaded property: true when the class has an
    /// (unguarded) __isset() that reports the property as not set, so the
    /// fetch yields null without calling __get().
    pub(crate) fn magic_isset_denies(
        &mut self,
        obj_val: &Value,
        class_lower: &[u8],
        prop: &crate::string::PhpString,
    ) -> bool {
        let Value::Object(o) = obj_val else {
            return false;
        };
        let has_isset = self
            .classes
            .get(class_lower)
            .map(|c| c.get_method(b"__isset").is_some())
            .unwrap_or(false);
        if !has_isset {
            return false;
        }
        let key = (o.borrow().object_id, prop.as_bytes().to_vec(), 1u8);
        if self.magic_prop_guards.contains(&key) {
            return false;
        }
        self.magic_prop_guards.insert(key.clone());
        let r = self.call_object_method(obj_val, b"__isset", &[Value::String(prop.clone())]);
        self.magic_prop_guards.remove(&key);
        !r.map(|v| v.is_truthy()).unwrap_or(false)
    }

    /// The Error a dimension write on `container` throws, for containers
    /// other than arrays and null-ish values (`append`: `$c[] = v`).
    pub(crate) fn dim_write_container_error(
        &self,
        container: &Value,
        append: bool,
    ) -> Option<String> {
        match container {
            Value::True | Value::Long(_) | Value::Double(_) | Value::Resource(_) => {
                Some("Cannot use a scalar value as an array".to_string())
            }
            Value::String(_) if append => Some("[] operator not supported for strings".to_string()),
            Value::Object(o) => {
                let cl: Vec<u8> = o.borrow().class_name.to_ascii_lowercase();
                let array_access = cl == b"weakmap"
                    || self
                        .classes
                        .get(&cl)
                        .map(|c| c.get_method(b"offsetset").is_some())
                        .unwrap_or(false)
                    || self.class_implements_interface(&cl, b"arrayaccess")
                    || self
                        .spl_find_base(
                            &cl,
                            &[
                                b"arrayobject",
                                b"arrayiterator",
                                b"splfixedarray",
                                b"splobjectstorage",
                                b"multipleiterator",
                                b"simplexmlelement",
                                b"spldoublylinkedlist",
                            ],
                        )
                        .is_some()
                    || self.native_class_handlers.contains_key(&cl)
                    || crate::vm::native_handlers(o).is_some_and(|h| {
                        h.write_dimension.is_some() || h.unset_dimension.is_some()
                    })
                    || Self::spl_iter_class(&cl)
                    || crate::vm::get_builtin_parent(&cl).is_some();
                if array_access {
                    None
                } else {
                    Some(format!(
                        "Cannot use object of type {} as array",
                        crate::value::display_class_name(&o.borrow().class_name)
                    ))
                }
            }
            _ => None,
        }
    }

    /// zend_asymmetric_visibility_property_modification_error(): the
    /// message when the current scope may not write a `set_vis` property.
    pub(crate) fn asym_set_denied(
        &self,
        set_vis: Option<crate::object::Visibility>,
        is_readonly: bool,
        declaring_class: &[u8],
        class_name: &[u8],
        prop: &[u8],
        operation: &str,
    ) -> Option<String> {
        let sv = set_vis?;
        if sv == crate::object::Visibility::Public {
            return None;
        }
        // zend_std_read_property(W): a readonly property reports the
        // readonly error before its set visibility.
        if is_readonly && operation == "indirectly modify" {
            return None;
        }
        let prop_str = String::from_utf8_lossy(prop).to_string();
        let caller_scope = self.current_class_scope();
        self.check_visibility(
            sv,
            declaring_class,
            class_name,
            &prop_str,
            true,
            caller_scope.as_deref(),
        )?;
        let vis_str = match (sv, is_readonly) {
            (crate::object::Visibility::Private, _) => "private(set)",
            (_, true) => "protected(set) readonly",
            _ => "protected(set)",
        };
        let decl_lower = declaring_class.to_ascii_lowercase();
        let class_display = self
            .class_name_for_display(&decl_lower)
            .map(|n| crate::value::display_class_name(&n))
            .unwrap_or_else(|| crate::value::display_class_name(declaring_class));
        let scope_str = self.aviz_error_scope();
        Some(format!(
            "Cannot {} {} property {}::${} from {}",
            operation, vis_str, class_display, prop_str, scope_str
        ))
    }

    /// zend_asymmetric_visibility_property_modification_error(): the scope
    /// named is the called scope (`$this`'s class in a bound closure).
    #[inline(never)]
    pub(crate) fn aviz_error_scope(&self) -> String {
        let Some(scope) = self.current_class_scope() else {
            return "global scope".to_string();
        };
        let called = self
            .called_class_stack
            .last()
            .filter(|c| !c.is_empty())
            .cloned()
            .unwrap_or(scope);
        let name = self
            .classes
            .get(&called.to_ascii_lowercase())
            .map(|c| c.name.clone())
            .unwrap_or(called);
        format!("scope {}", crate::value::display_class_name(&name))
    }

    /// `$obj[k] = &$v` / `$obj[] = &$v` on an object (ZEND_ASSIGN_REF on a
    /// FETCH_DIM_W result): offsetGet() is called for the write fetch; unless
    /// it returned a reference there is no slot to bind and an Error is
    /// thrown. Returns false when an exception is pending.
    pub(crate) fn assign_ref_to_object_dim(
        &mut self,
        obj: &Value,
        key: Option<Value>,
        line: u32,
    ) -> bool {
        self.last_call_by_ref = false;
        let r = self.fetch_dim_from_object(obj, key, DimFetch::W, line);
        if r.is_none() || self.current_exception.is_some() {
            return false;
        }
        if !self.last_call_by_ref {
            self.throw_error_msg(
                b"Error",
                "Cannot assign by reference to an array dimension of an object",
            );
            return false;
        }
        true
    }

    fn throw_error_msg(&mut self, class: &[u8], msg: &str) {
        let line = self.current_line;
        let exc = self.create_exception(class, msg, line);
        self.current_exception = Some(exc);
    }

    /// zend_check_string_offset(): false when it threw.
    pub(crate) fn check_string_offset_w(&mut self, key: &Value, mode: DimFetch, line: u32) -> bool {
        match key.deref() {
            Value::Long(_) => true,
            Value::String(s) => {
                let (numeric, trailing) = Vm::is_numeric_string_with_trailing(s.as_bytes());
                if numeric
                    && s.as_bytes()
                        .iter()
                        .all(|b| !matches!(b, b'.' | b'e' | b'E'))
                {
                    if trailing && mode != DimFetch::Unset {
                        self.emit_warning_at(
                            &format!("Illegal string offset \"{}\"", s.to_msg_string()),
                            line,
                        );
                    }
                    true
                } else {
                    self.illegal_string_offset(&Value::String(s), mode);
                    false
                }
            }
            Value::Double(_) | Value::Null | Value::Undef | Value::False | Value::True => {
                self.emit_warning_at("String offset cast occurred", line);
                true
            }
            other => {
                self.illegal_string_offset(&other, mode);
                false
            }
        }
    }

    fn illegal_string_offset(&mut self, key: &Value, mode: DimFetch) {
        if mode == DimFetch::Unset {
            self.throw_error_msg(b"Error", "Cannot unset string offsets");
        } else {
            let t = Self::offset_type_name(key);
            self.throw_error_msg(
                b"TypeError",
                &format!("Cannot access offset of type {} on string", t),
            );
        }
    }

    fn offset_type_name(key: &Value) -> String {
        match key {
            Value::Object(o) => crate::value::display_class_name(&o.borrow().class_name),
            Value::Array(_) => "array".to_string(),
            Value::String(_) => "string".to_string(),
            Value::Resource(_) => "resource".to_string(),
            Value::Double(_) => "float".to_string(),
            Value::Long(_) => "int".to_string(),
            Value::True | Value::False => "bool".to_string(),
            _ => "null".to_string(),
        }
    }

    /// zend_fetch_dimension_address_inner() + vivification for W/RW/UNSET.
    fn fetch_dim_from_array(
        &mut self,
        arr: &Rc<RefCell<PhpArray>>,
        key: Option<Value>,
        mode: DimFetch,
        line: u32,
    ) -> Option<Value> {
        // An element is fetched for writing: the array's storage is its own
        // first (an element array in storage shared with copies is held by
        // all of them, whatever its count says).
        if let Ok(mut a) = arr.try_borrow_mut() {
            a.make_unique();
        }
        let Some(key_val) = key else {
            // `$a[]->p = v`: the new element is null (the property write
            // on it then fails).
            if mode == DimFetch::Obj {
                if let Err(msg) = arr.borrow_mut().try_push(Value::Null) {
                    self.throw_error_msg(b"Error", msg);
                    return None;
                }
                return Some(Value::Null);
            }
            let rc = Rc::new(RefCell::new(PhpArray::new()));
            if let Err(msg) = arr.borrow_mut().try_push(Value::Array(rc.clone())) {
                self.throw_error_msg(b"Error", msg);
                return None;
            }
            return Some(Value::Array(rc));
        };
        let key_inner = key_val.deref();
        if matches!(key_inner, Value::Array(_) | Value::Object(_)) {
            let t = Self::offset_type_name(&key_inner);
            // zend_illegal_array_offset_access(): FETCH_DIM_UNSET of an
            // outer dimension too (only UNSET_DIM says "Cannot unset").
            let msg = format!("Cannot access offset of type {} on array", t);
            self.throw_error_msg(b"TypeError", &msg);
            return None;
        }
        let k = self.value_to_array_key_checked(&key_inner, line);
        let existing = arr.borrow().get(&k).cloned();
        let vivify = |arr: &Rc<RefCell<PhpArray>>, k| {
            let rc = Rc::new(RefCell::new(PhpArray::new()));
            arr.borrow_mut().set(k, Value::Array(rc.clone()));
            Value::Array(rc)
        };
        if matches!(mode, DimFetch::Obj | DimFetch::ObjRw) {
            return Some(match existing {
                None | Some(Value::Undef) => {
                    if mode == DimFetch::ObjRw {
                        self.emit_undefined_key_warning(&k, line);
                    }
                    arr.borrow_mut().set(k, Value::Null);
                    Value::Null
                }
                Some(v) => v.deref(),
            });
        }
        // (sort($a[k]): the builtin gets the null element)
        if std::mem::take(&mut self.dim_w_array_ref_send) && mode == DimFetch::W {
            match existing {
                None | Some(Value::Undef) => {
                    arr.borrow_mut().set(k, Value::Null);
                    return Some(Value::Null);
                }
                Some(Value::Null) => return Some(Value::Null),
                Some(Value::Reference(r)) if matches!(&*r.borrow(), Value::Null | Value::Undef) => {
                    return Some(Value::Null);
                }
                _ => {}
            }
        }
        match existing {
            None | Some(Value::Undef) => {
                if mode == DimFetch::Unset {
                    return Some(Value::Null);
                }
                if mode == DimFetch::Rw {
                    self.emit_undefined_key_warning(&k, line);
                }
                Some(vivify(arr, k))
            }
            Some(Value::Null) => {
                if mode == DimFetch::Unset {
                    return Some(Value::Null);
                }
                Some(vivify(arr, k))
            }
            Some(Value::False) if mode != DimFetch::Unset => {
                let held = Rc::strong_count(arr);
                let v = vivify(arr, k);
                self.emit_deprecated_at(
                    "Automatic conversion of false to array is deprecated",
                    line,
                );
                if Rc::strong_count(arr) < held && self.current_exception.is_none() {
                    // The error handler released the container: the
                    // assignment into the new array is abandoned
                    // (ZEND_ASSIGN_DIM's assign_dim_error).
                    self.dim_rw_abort = Some(self.call_depth);
                }
                Some(v)
            }
            Some(Value::Reference(r)) => {
                let inner = r.borrow().clone();
                match inner {
                    Value::Null | Value::Undef => {
                        if mode == DimFetch::Unset {
                            return Some(Value::Null);
                        }
                        let rc = Rc::new(RefCell::new(PhpArray::new()));
                        *r.borrow_mut() = Value::Array(rc.clone());
                        Some(Value::Array(rc))
                    }
                    Value::False if mode != DimFetch::Unset => {
                        // (the next write fetch's container is a reference:
                        // FETCH_DIM_W converts it silently, ASSIGN_DIM not)
                        if !self.dim_w_chain {
                            self.emit_deprecated_at(
                                "Automatic conversion of false to array is deprecated",
                                line,
                            );
                        }
                        let rc = Rc::new(RefCell::new(PhpArray::new()));
                        *r.borrow_mut() = Value::Array(rc.clone());
                        Some(Value::Array(rc))
                    }
                    Value::Array(rc) => {
                        // The reference holds one count, `inner` the other.
                        // (unset() leaves a referenced array in place: a builtin may
                        // be working on it through the reference.)
                        if crate::array::shared_beyond(&rc, 2) && mode != DimFetch::Unset {
                            let sep = Rc::new(RefCell::new(rc.borrow().clone()));
                            *r.borrow_mut() = Value::Array(sep.clone());
                            Some(Value::Array(sep))
                        } else {
                            Some(Value::Array(rc))
                        }
                    }
                    other => {
                        if matches!(other, Value::String(_)) && mode != DimFetch::Unset {
                            self.str_wb_pending = Some(StrWb::Ref(r.clone()));
                        }
                        Some(other)
                    }
                }
            }
            Some(Value::Array(rc)) => {
                // Copy-on-write: separate an element array shared elsewhere
                // (the container holds one count, `existing` the other).
                if Rc::strong_count(&rc) > 2 {
                    let sep = Rc::new(RefCell::new(rc.borrow().clone()));
                    arr.borrow_mut().set(k, Value::Array(sep.clone()));
                    Some(Value::Array(sep))
                } else {
                    Some(Value::Array(rc))
                }
            }
            Some(other) => {
                if matches!(other, Value::String(_)) && mode != DimFetch::Unset {
                    self.str_wb_pending = Some(StrWb::Elem(arr.clone(), k));
                }
                Some(other)
            }
        }
    }

    /// `&$weakmap[$obj]`: zend_weakmap_read_dimension(BP_VAR_W) hands out the
    /// stored zval, which the reference assignment turns into a reference in
    /// place. None when it threw.
    #[inline(never)]
    pub(crate) fn weakmap_fetch_dim_ref(
        &mut self,
        obj: &Rc<RefCell<crate::object::PhpObject>>,
        key: Value,
    ) -> Option<Value> {
        self.weakmap_fetch_dim_w(obj, Some(key.clone()), DimFetch::W)?;
        let Value::Object(key_obj) = key.deref() else {
            return None;
        };
        let wm_id = obj.borrow().object_id;
        let key_id = key_obj.borrow().object_id;
        let (_, _, v) = self
            .weak_map_entries
            .get_mut(&wm_id)?
            .iter_mut()
            .find(|(oid, weak, _)| *oid == key_id && weak.upgrade().is_some())?;
        if !matches!(v, Value::Reference(_)) {
            let cur = std::mem::replace(v, Value::Null);
            *v = Value::Reference(Rc::new(RefCell::new(cur)));
        }
        Some(v.clone())
    }

    /// zend_weakmap_read_dimension() for W/RW/UNSET: the stored value itself.
    fn weakmap_fetch_dim_w(
        &mut self,
        obj: &Rc<RefCell<crate::object::PhpObject>>,
        key: Option<Value>,
        mode: DimFetch,
    ) -> Option<Value> {
        let Some(key) = key else {
            self.throw_error_msg(b"Error", "Cannot append to WeakMap");
            return None;
        };
        let Value::Object(key_obj) = key.deref() else {
            self.throw_error_msg(b"TypeError", "WeakMap key must be an object");
            return None;
        };
        let wm_id = obj.borrow().object_id;
        let key_id = key_obj.borrow().object_id;
        let entry = self.weak_map_entries.get_mut(&wm_id).and_then(|entries| {
            entries
                .iter_mut()
                .find(|(oid, weak, _)| *oid == key_id && weak.upgrade().is_some())
        });
        let Some((_, _, v)) = entry else {
            let msg = format!(
                "Object {}#{} not contained in WeakMap",
                crate::value::display_class_name(&key_obj.borrow().class_name),
                key_id
            );
            self.throw_error_msg(b"Error", &msg);
            return None;
        };
        let target: &mut Value = v;
        let fetched = match &*target {
            Value::Reference(r) => r.borrow().clone(),
            other => other.clone(),
        };
        let store = |target: &mut Value, nv: Value| match target {
            Value::Reference(r) => *r.borrow_mut() = nv,
            t => *t = nv,
        };
        match fetched {
            Value::Null if mode != DimFetch::Unset => {
                let arr = Value::Array(Rc::new(RefCell::new(PhpArray::new())));
                store(target, arr.clone());
                Some(arr)
            }
            Value::Array(rc) if Rc::strong_count(&rc) > 2 => {
                let sep = Value::Array(Rc::new(RefCell::new(rc.borrow().clone())));
                store(target, sep.clone());
                Some(sep)
            }
            other => Some(other),
        }
    }

    fn emit_undefined_key_warning(&mut self, k: &crate::array::ArrayKey, line: u32) {
        let msg = match k {
            crate::array::ArrayKey::Int(i) => format!("Undefined array key {}", i),
            crate::array::ArrayKey::String(s) => {
                format!("Undefined array key \"{}\"", s.to_msg_string())
            }
        };
        self.emit_warning_at(&msg, line);
    }

    /// read_dimension() of an object in a write context, then the
    /// "Indirect modification" check of zend_fetch_dimension_address().
    pub(crate) fn fetch_dim_from_object(
        &mut self,
        obj_val: &Value,
        key: Option<Value>,
        mode: DimFetch,
        line: u32,
    ) -> Option<Value> {
        self.fetch_dim_from_object_ex(obj_val, key, mode, line, false)
    }

    /// fetch_dim_from_object(); `want_ref` hands out the reference a
    /// by-reference offsetGet() returned (`$x = &$obj[k]`, `[&$x] = $obj`).
    pub(crate) fn fetch_dim_from_object_ex(
        &mut self,
        obj_val: &Value,
        key: Option<Value>,
        mode: DimFetch,
        line: u32,
        want_ref: bool,
    ) -> Option<Value> {
        let Value::Object(obj) = obj_val else {
            return Some(Value::Null);
        };
        let (class_name, class_lower) = {
            let ob = obj.borrow();
            let cl: Vec<u8> = ob
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            (ob.class_name.clone(), cl)
        };
        let indirect_notice = |vm: &mut Vm| {
            let disp = crate::value::display_class_name(&class_name);
            vm.emit_notice_at(
                &format!(
                    "Indirect modification of overloaded element of {} has no effect",
                    disp
                ),
                line,
            );
        };
        let is_ao = self
            .spl_find_base(&class_lower, &[b"arrayobject", b"arrayiterator"])
            .is_some();
        if is_ao && !self.ao_user_has(obj, b"offsetget") {
            let m = match mode {
                DimFetch::W | DimFetch::Obj => b'w',
                DimFetch::Rw | DimFetch::ObjRw => b'r',
                DimFetch::Unset => b'u',
            };
            let r = self.ao_fetch_dim_w(obj, key.as_ref(), m);
            if self.current_exception.is_some() {
                // An illegal offset in an unset fetch throws and hands out
                // uninitialized_zval (zend_fetch_dimension_address()).
                if m == b'u' && r.is_none() {
                    indirect_notice(self);
                }
                return None;
            }
            return match r {
                Some(v) => Some(v),
                None => {
                    indirect_notice(self);
                    Some(Value::Null)
                }
            };
        }
        if class_lower == b"weakmap" {
            return self.weakmap_fetch_dim_w(obj, key, mode);
        }
        // An extension object's read_dimension handler (BP_VAR_W / RW / UNSET)
        if let Some(f) = crate::vm::native_handlers(obj).and_then(|h| h.read_dimension) {
            let k = key.as_ref().map(|k| k.deref());
            let bp = match mode {
                DimFetch::Rw | DimFetch::ObjRw => crate::vm::BP_VAR_RW,
                DimFetch::Unset => crate::vm::BP_VAR_UNSET,
                DimFetch::W | DimFetch::Obj => crate::vm::BP_VAR_W,
            };
            let r = crate::vm::with_dim_fetch_type(bp, || f(self, obj, k.as_ref(), 1));
            if let Some(v) = r {
                if self.current_exception.is_some() {
                    return None;
                }
                if !matches!(v, Value::Object(_) | Value::Reference(_)) {
                    indirect_notice(self);
                }
                return Some(v);
            }
            if self.current_exception.is_some() {
                return None;
            }
        }
        // spl_fixedarray_object_read_dimension() without an offset.
        if key.is_none()
            && self
                .spl_find_base(&class_lower, &[b"splfixedarray"])
                .is_some()
            && !self.ao_user_has(obj, b"offsetget")
        {
            self.spl_throw(b"Error", "[] operator not supported for SplFixedArray");
            return None;
        }
        // spl_array_read_dimension_ex(): an overriding offsetGet() of an
        // ArrayObject gets an undefined offset for `$ao[]`.
        if key.is_none() && is_ao {
            let m = self
                .classes
                .get(&class_lower)
                .and_then(|c| c.get_method(b"offsetget"))
                .map(|m| {
                    (
                        m.declaring_class.clone(),
                        m.op_array
                            .cv_names
                            .iter()
                            .find(|n| n.as_slice() != b"this")
                            .cloned()
                            .unwrap_or_default(),
                    )
                });
            if let Some((decl, pname)) = m {
                let decl = self
                    .class_name_for_display(&decl.to_ascii_lowercase())
                    .unwrap_or(decl);
                let msg = format!(
                    "{}::offsetGet(): Argument #1 (${}) not passed",
                    crate::value::display_class_name(&decl),
                    crate::string::msg_str(&pname)
                );
                self.throw_error_msg(b"ArgumentCountError", &msg);
                indirect_notice(self);
                return None;
            }
        }
        let key_arg = key.map(|k| k.deref()).unwrap_or(Value::Null);
        self.last_call_by_ref = false;
        let args = [obj_val.clone(), key_arg.clone()];
        // A by-reference offsetGet() hands out the element's reference, which
        // the write goes through.
        let by_ref = self
            .classes
            .get(&class_lower)
            .and_then(|c| c.get_method(b"offsetget"))
            .map(|m| m.op_array.returns_by_ref)
            .unwrap_or(false);
        let result = match self.handle_spl_docall(&class_lower, b"offsetget", &args) {
            Some(v) => Some(v),
            None => {
                if by_ref {
                    self.keep_ref_depth = Some(self.call_depth + 1);
                }
                let r = self.call_object_method(obj_val, b"offsetGet", &[key_arg]);
                self.keep_ref_depth = None;
                r
            }
        };
        if let Some(Value::Reference(r)) = &result {
            self.last_call_by_ref = true;
            if want_ref {
                return Some(Value::Reference(r.clone()));
            }
            let cur = r.borrow().clone();
            return Some(match cur {
                Value::Null | Value::Undef if matches!(mode, DimFetch::W | DimFetch::Rw) => {
                    let a = Value::Array(Rc::new(RefCell::new(PhpArray::new())));
                    *r.borrow_mut() = a.clone();
                    a
                }
                // The reference holds one count, `cur` the other.
                Value::Array(a) if crate::array::shared_beyond(&a, 2) => {
                    let sep = Value::Array(Rc::new(RefCell::new(a.borrow().clone())));
                    *r.borrow_mut() = sep.clone();
                    sep
                }
                other => other,
            });
        }
        if self.current_exception.is_some() {
            return None;
        }
        let Some(result) = result else {
            let disp = crate::value::display_class_name(&class_name);
            self.throw_error_msg(
                b"Error",
                &format!("Cannot use object of type {} as array", disp),
            );
            return None;
        };
        if self.last_call_by_ref {
            self.last_call_by_ref = false;
            return Some(result.deref());
        }
        let result = result.deref();
        match result {
            Value::Object(_) => Some(result),
            Value::Array(rc) => {
                indirect_notice(self);
                // The write lands on a copy (offsetGet returned by value).
                let copy = rc.borrow().clone();
                Some(Value::Array(Rc::new(RefCell::new(copy))))
            }
            other => {
                indirect_notice(self);
                Some(other)
            }
        }
    }
}
