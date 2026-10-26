//! `Closure` objects.
//!
//! A closure is a `Value::Object` of class `Closure` whose `PhpObject::closure`
//! payload holds the internal callable used for dispatch:
//! - `"__closure_N"` / `"__arrow_N"` / `"__bound_closure_N"` / `"__closure_fcc_N"`
//!   (no captures), or `[name, captured...]` where the captured values bind to
//!   the function's first CVs (`OpArray::closure_capture_names`);
//! - a plain function name (`Closure::fromCallable('strlen')`) or
//!   `"Class::method"` (static method closure);
//! - `["Class::method", $obj]` (bound-method closure).

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::PhpObject;
use crate::opcode::{OpArray, OpCode, OperandType};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

/// A function name the engine makes (a closure's, a bound closure's, a
/// first-class callable's trampoline...), not one user code declares
/// (`__()`, `__return_true()`).
#[inline]
pub fn is_engine_fn_name(name: &[u8]) -> bool {
    const PREFIXES: &[&[u8]] = &[
        b"__closure_",
        b"__arrow_",
        b"__bound_closure_",
        b"__const_thunk_",
        b"__declare_const",
        b"__deferred_",
        b"__property_",
        b"__anonymous",
    ];
    PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Whether `name` is an internal closure function name.
#[inline]
pub fn is_closure_fn_name(name: &[u8]) -> bool {
    (name.starts_with(b"__closure_") && !name.starts_with(b"__closure_fcc_"))
        || name.starts_with(b"__arrow_")
        || name.starts_with(b"__bound_closure_")
}

/// Split an internal closure callable into (function name, captured values).
pub fn closure_parts(inner: &Value) -> (Vec<u8>, Vec<Value>) {
    match inner {
        Value::Array(arr) => {
            let a = arr.borrow();
            // `[$obj, 'method']` (methods of builtin classes)
            if a.len() == 2 {
                if let (Some(Value::Object(o)), Some(Value::String(m))) =
                    (a.get_int(0), a.get_int(1))
                {
                    let mut name = o.borrow().class_name.clone();
                    name.extend_from_slice(b"::");
                    name.extend_from_slice(m.as_bytes());
                    return (name, vec![Value::Object(o.clone())]);
                }
            }
            let mut it = a.values();
            let name = it
                .next()
                .map(|v| v.to_php_string().as_bytes().to_vec())
                .unwrap_or_default();
            (name, it.cloned().collect())
        }
        other => (other.to_php_string().as_bytes().to_vec(), Vec::new()),
    }
}

/// Whether the function name of `closure_parts(inner)` is a closure's and
/// is `name` (case-insensitively), without building the parts.
pub fn closure_fn_named(inner: &Value, name: &[u8]) -> bool {
    let check = |n: &[u8]| is_closure_fn_name(n) && n.eq_ignore_ascii_case(name);
    match inner {
        Value::Array(arr) => {
            let a = arr.borrow();
            if a.len() == 2
                && matches!(
                    (a.get_int(0), a.get_int(1)),
                    (Some(Value::Object(_)), Some(Value::String(_)))
                )
            {
                drop(a);
                return check(&closure_parts(inner).0);
            }
            match a.values().next() {
                Some(Value::String(s)) => check(s.as_bytes()),
                Some(v) => check(v.to_php_string().as_bytes()),
                None => check(b""),
            }
        }
        Value::String(s) => check(s.as_bytes()),
        other => check(other.to_php_string().as_bytes()),
    }
}

/// `==` on two distinct Closure objects (zend_closure_compare): only fake
/// closures (created from callables) over the same function, with the same
/// bound `$this`, compare equal.
pub fn closures_equal(a: &PhpObject, b: &PhpObject) -> bool {
    let (x, y) = match (a.closure.as_deref(), b.closure.as_deref()) {
        (Some(x), Some(y)) => (x, y),
        _ => return false,
    };
    let (na, ca) = closure_parts(x);
    let (nb, cb) = closure_parts(y);
    if is_closure_fn_name(&na) || is_closure_fn_name(&nb) {
        return false;
    }
    if !na.eq_ignore_ascii_case(&nb) {
        return false;
    }
    match (ca.first().map(|v| v.deref()), cb.first().map(|v| v.deref())) {
        (None, None) => true,
        (Some(Value::Object(p)), Some(Value::Object(q))) => Rc::ptr_eq(&p, &q),
        _ => false,
    }
}

fn str_val(s: &[u8]) -> Value {
    Value::String(PhpString::from_vec(s.to_vec()))
}

impl Vm {
    /// zend_create_closure(): a non-static closure created in a frame with a
    /// `$this` is bound to it, also when the code declaring it had none (a
    /// closure declared in a closure that bindTo()/bind() gave a `$this`).
    pub fn closure_inherit_this(
        &mut self,
        inner: Value,
        frame: &crate::opcode::OpArray,
        cvs: &[Value],
    ) -> Value {
        if frame.cv_names.first().map(|n| n.as_slice()) != Some(b"this") {
            return inner;
        }
        let this = match cvs.first().map(|v| v.deref()) {
            Some(v @ Value::Object(_)) => v,
            _ => return inner,
        };
        self.closure_bind_frame_this(inner, this)
    }

    /// closure_inherit_this() with the frame's `$this` given.
    pub(crate) fn closure_bind_frame_this(&mut self, inner: Value, this: Value) -> Value {
        let (name, caps) = closure_parts(&inner);
        if !is_closure_fn_name(&name) {
            return inner;
        }
        let l = lower(&name);
        match self.user_functions.get(&l) {
            Some(op)
                if !op.is_static_closure
                    && op.closure_capture_names.first().map(|n| n.as_slice()) != Some(b"this") => {}
            _ => return inner,
        }
        // The bound variant's captures are [$this, ...use values].
        if let Some(bound) = self.closure_this_variants.get(&l) {
            let mut arr = PhpArray::new();
            arr.push(str_val(bound));
            arr.push(this);
            for c in caps {
                arr.push(c);
            }
            return Value::Array(Rc::new(RefCell::new(arr)));
        }
        let r = self.bind_closure_raw(&inner, this, Value::Null, false);
        if matches!(r, Value::Null) {
            return inner;
        }
        let (bound, _) = closure_parts(&r);
        self.closure_this_variants.insert(l, bound);
        r
    }

    /// Wrap an internal callable (closure name, capture array, function
    /// name, bound-method array) into a `Closure` object with its own object
    /// id. Closure objects and Null (failure) are returned unchanged.
    pub fn make_closure_object(&mut self, inner: Value) -> Value {
        if matches!(inner, Value::Null | Value::Undef) || inner.is_closure_obj() {
            return inner;
        }
        let id = self.alloc_object_id();
        let mut o = PhpObject::new(b"Closure".to_vec(), id);
        o.closure = Some(Box::new(inner));
        o.account();
        Value::Object(Rc::new(RefCell::new(o)))
    }

    /// `clone $closure`: a new Closure object with the same function, bound
    /// `$this` and captured values (zend_closure_clone).
    pub fn clone_closure(&mut self, closure: &Value) -> Value {
        let inner = match closure.closure_inner() {
            Some(i) => i,
            None => return closure.clone(),
        };
        let inner = match inner {
            Value::Array(a) => {
                let copy = a.borrow().clone();
                Value::Array(Rc::new(RefCell::new(copy)))
            }
            other => other,
        };
        let inner = self.closure_with_own_statics(inner, Some(b""));
        self.make_closure_object(inner)
    }

    /// The user OpArray backing a closure object (None for internal functions).
    pub fn closure_op_array(&self, closure: &Value) -> Option<&OpArray> {
        let inner = closure.closure_inner()?;
        let (name, _) = closure_parts(&inner);
        let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
        self.user_functions.get(&lower).map(|o| &**o)
    }

    /// Canonical "Class::method" / "function" display for a fake closure target.
    fn closure_target_display(&self, name: &[u8]) -> Vec<u8> {
        let name = name.strip_prefix(b"\\").unwrap_or(name);
        if let Some(p) = name.windows(2).position(|w| w == b"::") {
            let cls = &name[..p];
            let m = &name[p + 2..];
            let cl: Vec<u8> = cls.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some((sc, sm)) = self.internal_method_of(&cl, &lower(m)) {
                let mut out = sc.name.as_bytes().to_vec();
                out.extend_from_slice(b"::");
                out.extend_from_slice(sm.name.as_bytes());
                return out;
            }
            if let Some(ce) = self.classes.get(&cl) {
                let ml: Vec<u8> = m.iter().map(|b| b.to_ascii_lowercase()).collect();
                let md = ce.methods.get(&ml);
                // The scope of a method closure is its declaring class.
                let mut out = md
                    .and_then(|md| self.classes.get(&md.declaring_class.to_ascii_lowercase()))
                    .map(|dc| dc.name.clone())
                    .unwrap_or_else(|| ce.name.clone());
                out.extend_from_slice(b"::");
                out.extend_from_slice(md.map(|md| md.name.as_slice()).unwrap_or(m));
                return out;
            }
            return name.to_vec();
        }
        let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
        if let Some(op) = self.user_functions.get(&lower) {
            if !op.name.is_empty() {
                return op.name.clone();
            }
        }
        if self.functions.contains_key(&lower) {
            return lower;
        }
        name.to_vec()
    }

    /// `["parameter"]` entries (name => "<required>"/"<optional>") for a user function.
    fn closure_param_entries(op: &OpArray) -> Vec<(Vec<u8>, bool)> {
        // (display name, optional)
        let mut out = Vec::new();
        if is_closure_fn_name(&op.name) && op.fcc_target_name.is_none() {
            for (i, (n, by_ref)) in op.closure_params.iter().enumerate() {
                let mut d = if *by_ref {
                    b"&$".to_vec()
                } else {
                    b"$".to_vec()
                };
                d.extend_from_slice(n);
                out.push((d, i as u32 >= op.required_param_count));
            }
            return out;
        }
        // Regular function / method: params are the first `param_count` CVs
        // (methods may have `this` at CV 0).
        let skip = if op.cv_names.first().map(|n| n.as_slice()) == Some(b"this") {
            1
        } else {
            0
        };
        let count = op.param_count as usize;
        for i in 0..count {
            let cv = i + skip;
            if let Some(n) = op.cv_names.get(cv) {
                let by_ref = op.by_ref_params.get(cv).copied().unwrap_or(false);
                let mut d = if by_ref {
                    b"&$".to_vec()
                } else {
                    b"$".to_vec()
                };
                d.extend_from_slice(n);
                let optional =
                    (i as u32) >= op.required_param_count || op.variadic_param == Some(cv as u32);
                out.push((d, optional));
            }
        }
        out
    }

    /// Static variables declared in a function body (`static $x = ...;`).
    fn closure_static_vars(&self, op: &OpArray) -> Vec<(Vec<u8>, Value)> {
        let mut out = Vec::new();
        let mut prefix = op.name.clone();
        prefix.extend_from_slice(b"::");
        for o in &op.ops {
            if !matches!(o.opcode, OpCode::StaticVarInit) {
                continue;
            }
            let key = match o.result {
                OperandType::Const(idx) => match op.literals.get(idx as usize) {
                    Some(Value::String(s)) => s.as_bytes().to_vec(),
                    _ => continue,
                },
                _ => continue,
            };
            let var = match key.strip_prefix(prefix.as_slice()) {
                Some(v) => v.to_vec(),
                None => continue,
            };
            if out.iter().any(|(n, _): &(Vec<u8>, Value)| *n == var) {
                continue;
            }
            let val = if let Some(v) = self.static_vars().get(&key) {
                v.deref()
            } else {
                match o.op2 {
                    OperandType::Const(idx) => op
                        .literals
                        .get(idx as usize)
                        .cloned()
                        .unwrap_or(Value::Null),
                    _ => {
                        let key_idx = match o.result {
                            OperandType::Const(i) => i,
                            _ => u32::MAX,
                        };
                        op.static_defaults
                            .iter()
                            .flat_map(|d| d.iter())
                            .find(|(k, _)| *k == key_idx)
                            .map(|(_, v)| v.clone())
                            .unwrap_or(Value::Null)
                    }
                }
            };
            out.push((var, val));
        }
        out
    }

    /// The debug-info properties of a closure (zend_closure_get_debug_info):
    /// name/file/line (or function), static, this, parameter.
    pub fn closure_debug_info(&self, closure: &Value) -> Vec<(Vec<u8>, Value)> {
        let mut props: Vec<(Vec<u8>, Value)> = Vec::new();
        let inner = match closure.closure_inner() {
            Some(i) => i,
            None => return props,
        };
        let (name, captures) = closure_parts(&inner);
        let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
        let op = self.user_functions.get(&lower);
        let is_user_closure =
            is_closure_fn_name(&name) && op.map(|o| o.fcc_target_name.is_none()).unwrap_or(false);
        let mut this_val: Option<Value> = None;
        let mut params: Vec<(Vec<u8>, bool)> = Vec::new();
        if is_user_closure {
            let op = op.unwrap();
            let fname = if !op.display_name.is_empty() {
                op.display_name.clone()
            } else {
                format!(
                    "{{closure:{}:{}}}",
                    String::from_utf8_lossy(&op.filename),
                    op.decl_line
                )
                .into_bytes()
            };
            props.push((b"name".to_vec(), str_val(&fname)));
            props.push((b"file".to_vec(), str_val(&op.filename)));
            props.push((b"line".to_vec(), Value::Long(op.decl_line as i64)));
            let mut statics = PhpArray::new();
            for (i, v) in captures.iter().enumerate() {
                let n = op.closure_capture_names.get(i).cloned().unwrap_or_default();
                if n.as_slice() == b"this" {
                    if matches!(v.deref(), Value::Object(_)) {
                        this_val = Some(v.deref());
                    }
                    continue;
                }
                if n.is_empty() || matches!(v, Value::Undef) {
                    continue;
                }
                let v = match v {
                    Value::Reference(r) if Rc::strong_count(r) == 1 => r.borrow().clone(),
                    other => other.clone(),
                };
                statics.set(ArrayKey::String(PhpString::from_vec(n)), v);
            }
            for (n, v) in self.closure_static_vars(op) {
                let k = ArrayKey::String(PhpString::from_vec(n));
                if statics.get(&k).is_none() {
                    statics.set(k, v);
                }
            }
            if !statics.is_empty() {
                props.push((
                    b"static".to_vec(),
                    Value::Array(Rc::new(RefCell::new(statics))),
                ));
            }
            params = Self::closure_param_entries(op);
        } else {
            // Fake closure (first-class callable / fromCallable).
            let target: Vec<u8> = match op.and_then(|o| o.fcc_target_name.clone()) {
                Some(t) => t,
                None => name.clone(),
            };
            props.push((
                b"function".to_vec(),
                str_val(&self.closure_target_display(&target)),
            ));
            if let Some(Value::Object(o)) = captures.first().map(|v| v.deref()) {
                this_val = Some(Value::Object(o));
            }
            let tl: Vec<u8> = target
                .strip_prefix(b"\\")
                .unwrap_or(&target)
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            let internal = tl
                .windows(2)
                .position(|w| w == b"::")
                .and_then(|p| self.internal_method_of(&tl[..p], &tl[p + 2..]));
            let target_op: Option<OpArray> = if internal.is_some() {
                None
            } else {
                self.user_functions
                    .get(&tl)
                    .map(|o| (**o).clone())
                    .or_else(|| self.closure_method_op(&tl))
            };
            if let Some(top) = &target_op {
                let statics = self.closure_static_vars(top);
                if !statics.is_empty() {
                    let mut sa = PhpArray::new();
                    for (n, v) in statics {
                        sa.set(ArrayKey::String(PhpString::from_vec(n)), v);
                    }
                    props.push((b"static".to_vec(), Value::Array(Rc::new(RefCell::new(sa)))));
                }
            }
            if name.starts_with(b"__closure_fcc_tramp_ip_")
                || name.starts_with(b"__closure_fcc_tramp_sp_")
            {
                // (a __call()/__callStatic() trampoline: `mixed ...$arguments`)
                params = vec![(b"$arguments".to_vec(), true)];
            } else if let Some((_, sm)) = internal {
                let required = sm.required_args();
                params = sm
                    .args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let mut d = if a.flags & crate::stubs::A_BY_REF != 0 {
                            b"&$".to_vec()
                        } else {
                            b"$".to_vec()
                        };
                        d.extend_from_slice(a.name.as_bytes());
                        (d, i >= required)
                    })
                    .collect();
            } else if let Some(top) = &target_op {
                params = Self::closure_param_entries(top);
            } else if let Some(o) = op.filter(|o| o.name.starts_with(b"__bound_closure_")) {
                // (a function bound to a $this, see function_closure_with_this)
                params = o
                    .closure_params
                    .iter()
                    .enumerate()
                    .map(|(i, (n, by_ref))| {
                        let mut d = if *by_ref {
                            b"&$".to_vec()
                        } else {
                            b"$".to_vec()
                        };
                        d.extend_from_slice(n);
                        (
                            d,
                            i as u32 >= o.required_param_count
                                || o.variadic_param == Some(i as u32 + 1),
                        )
                    })
                    .collect();
            } else if let Some(fi) = crate::arginfo::lookup(&tl) {
                // zend_closure_get_debug_info() for an internal function.
                params = fi
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let mut d = if a.by_ref != 0 {
                            b"&$".to_vec()
                        } else {
                            b"$".to_vec()
                        };
                        d.extend_from_slice(a.name.as_bytes());
                        (d, a.variadic || i >= fi.required as usize)
                    })
                    .collect();
            } else if let Some(names) = self.builtin_param_names.get(&tl) {
                params = names
                    .iter()
                    .map(|n| {
                        let mut d = b"$".to_vec();
                        d.extend_from_slice(n);
                        (d, false)
                    })
                    .collect();
            }
        }
        if let Some(t) = this_val {
            props.push((b"this".to_vec(), t));
        }
        if !params.is_empty() {
            let mut pa = PhpArray::new();
            for (n, opt) in params {
                pa.set(
                    ArrayKey::String(PhpString::from_vec(n)),
                    str_val(if opt { b"<optional>" } else { b"<required>" }),
                );
            }
            props.push((
                b"parameter".to_vec(),
                Value::Array(Rc::new(RefCell::new(pa))),
            ));
        }
        props
    }

    /// The callable name of a closure (zend_get_callable_name_ex): the
    /// `{closure:...}` name for user closures, "Class::method" or "function"
    /// for closures created from callables.
    pub fn closure_function_name(&self, closure: &Value) -> Vec<u8> {
        let inner = match closure.closure_inner() {
            Some(i) => i,
            None => return b"Closure::__invoke".to_vec(),
        };
        let (name, _) = closure_parts(&inner);
        let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
        match self.user_functions.get(&lower) {
            Some(op) if is_closure_fn_name(&name) || name.starts_with(b"__closure_fcc_") => {
                if let Some(t) = &op.fcc_target_name {
                    return self.closure_target_display(t);
                }
                if !op.display_name.is_empty() {
                    return op.display_name.clone();
                }
                if name.starts_with(b"__closure_fcc_") {
                    return b"{closure}".to_vec();
                }
                format!(
                    "{{closure:{}:{}}}",
                    String::from_utf8_lossy(&op.filename),
                    op.decl_line
                )
                .into_bytes()
            }
            _ => self.closure_target_display(&name),
        }
    }

    /// The bound `$this` of a closure, if any.
    pub fn closure_this(&self, closure: &Value) -> Option<Value> {
        let inner = closure.closure_inner()?;
        let (name, caps) = closure_parts(&inner);
        if caps.is_empty() {
            return None;
        }
        let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
        let v = if is_closure_fn_name(&name) {
            let op = self.user_functions.get(&lower)?;
            let idx = op
                .closure_capture_names
                .iter()
                .position(|n| n.as_slice() == b"this")?;
            caps.get(idx)?.deref()
        } else if name.starts_with(b"__closure_fcc_") || name.windows(2).any(|w| w == b"::") {
            caps[0].deref()
        } else {
            return None;
        };
        if matches!(v, Value::Object(_)) {
            Some(v)
        } else {
            None
        }
    }

    /// The called class (static::) of a closure, if any.
    pub fn closure_called_class(&self, closure: &Value) -> Option<Vec<u8>> {
        if let Some(Value::Object(o)) = self.closure_this(closure) {
            return Some(o.borrow().class_name.clone());
        }
        let inner = closure.closure_inner()?;
        let (name, _) = closure_parts(&inner);
        let l = lower(&name);
        // (a static call trampoline: the class it was called on)
        if let Some(c) = self
            .user_functions
            .get(&l)
            .filter(|_| name.starts_with(b"__closure_fcc_tramp_"))
            .and_then(|o| o.closure_called_class.clone())
        {
            return Some(c);
        }
        let target = if name.starts_with(b"__closure_fcc_") {
            self.user_functions
                .get(&l)
                .and_then(|o| o.fcc_target_name.clone())
                .unwrap_or_default()
        } else {
            name.clone()
        };
        if let Some(p) = target.windows(2).position(|w| w == b"::") {
            let cl = lower(&target[..p]);
            return Some(
                self.classes
                    .get(&cl)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| target[..p].to_vec()),
            );
        }
        let op = self.user_functions.get(&l)?;
        if let Some(c) = &op.closure_called_class {
            return Some(c.clone());
        }
        self.closure_scope(closure)
    }

    /// The scope class (canonical name) of a closure, if any.
    pub fn closure_scope(&self, closure: &Value) -> Option<Vec<u8>> {
        let inner = closure.closure_inner()?;
        let (name, _) = closure_parts(&inner);
        let name = if name.starts_with(b"__closure_fcc_tramp_") {
            self.user_functions
                .get(&lower(&name))
                .and_then(|o| o.fcc_target_name.clone())
                .unwrap_or(name)
        } else {
            name
        };
        let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
        let scope_lower: Vec<u8> = if let Some(p) = lower.windows(2).position(|w| w == b"::") {
            let cl = lower[..p].to_vec();
            if let Some((sc, _)) = self.internal_method_of(&cl, &lower[p + 2..]) {
                return Some(sc.name.as_bytes().to_vec());
            }
            self.classes
                .get(&cl)
                .and_then(|c| c.methods.get(&lower[p + 2..]))
                .map(|m| m.declaring_class.to_ascii_lowercase())
                .unwrap_or(cl)
        } else {
            let op = self.user_functions.get(&lower)?;
            if !is_closure_fn_name(&name) && !name.starts_with(b"__closure_fcc_") {
                return None;
            }
            op.scope_class.clone()?
        };
        Some(
            self.classes
                .get(&scope_lower)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| self.builtin_canonical_name(&scope_lower).into_bytes()),
        )
    }

    /// `use` variables of a closure (name => value), excluding `$this`.
    pub fn closure_used_vars(&self, closure: &Value) -> PhpArray {
        let mut out = PhpArray::new();
        let inner = match closure.closure_inner() {
            Some(i) => i,
            None => return out,
        };
        let (name, caps) = closure_parts(&inner);
        if !is_closure_fn_name(&name) {
            return out;
        }
        let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
        if let Some(op) = self.user_functions.get(&lower) {
            for (i, v) in caps.iter().enumerate() {
                match op.closure_capture_names.get(i) {
                    Some(n) if n.as_slice() != b"this" && !matches!(v, Value::Undef) => {
                        // by-reference uses stay references (Z_TRY_ADDREF)
                        out.set(ArrayKey::String(PhpString::from_vec(n.clone())), v.clone());
                    }
                    _ => {}
                }
            }
        }
        out
    }

    /// The OpArray of a "class::method" name (lowercase), if user-defined.
    fn closure_method_op(&self, lower: &[u8]) -> Option<OpArray> {
        let p = lower.windows(2).position(|w| w == b"::")?;
        let ce = self.classes.get(&lower[..p])?;
        ce.methods
            .get(&lower[p + 2..])
            .map(|m| (*m.op_array).clone())
    }
}

type FccResult = Result<Value, (&'static [u8], String)>;

fn lower(b: &[u8]) -> Vec<u8> {
    b.iter().map(|c| c.to_ascii_lowercase()).collect()
}

impl Vm {
    /// First-class callable creation (`f(...)`, `$o->m(...)`, `A::m(...)`),
    /// like zend_closure_from_frame: resolves the target now, producing a
    /// fake closure over it. `wrapper` is the compiled forwarding closure,
    /// used for magic `__call`/`__callStatic` targets and builtin classes.
    /// The callable of a dynamic call names its class: self / parent /
    /// static are looked up as class names (zend_init_dynamic_call_*()).
    pub fn make_fcc(
        &mut self,
        wrapper: Value,
        desc: &[u8],
        this: Option<Value>,
        scope: Option<Vec<u8>>,
    ) -> FccResult {
        let (_, caps) = closure_parts(&wrapper);
        // Op-array scopes are lowercase; errors name the class as declared.
        let scope = scope.map(|s| {
            self.classes
                .get(&lower(&s))
                .map(|c| c.name.clone())
                .unwrap_or(s)
        });
        if let Some(name) = desc.strip_prefix(b"f:") {
            return self.fcc_function(name, true);
        }
        if let Some(name) = desc.strip_prefix(b"F:") {
            return self.fcc_function(name, false);
        }
        if desc == b"d:" {
            let callable = caps.first().map(|v| v.deref()).unwrap_or(Value::Null);
            return self.fcc_dynamic(callable, wrapper, scope);
        }
        // `$class::method(...)`: the class (name or object) is captured.
        if let Some(m) = desc.strip_prefix(b"D:") {
            let cls = match caps.first().map(|v| v.deref()) {
                Some(Value::Object(o)) => o.borrow().class_name.clone(),
                Some(Value::String(s)) => {
                    by_name_class_check(s.as_bytes())?;
                    s.as_bytes().to_vec()
                }
                _ => {
                    return Err((
                        b"Error",
                        "Class name must be a valid object or a string".to_string(),
                    ));
                }
            };
            return self.fcc_static(&cls, m, this, wrapper, scope);
        }
        if let Some(m) = desc.strip_prefix(b"m:") {
            let obj = caps.first().map(|v| v.deref()).unwrap_or(Value::Null);
            return self.fcc_method(obj, m, wrapper, scope, false);
        }
        // A method name computed at run time: the compiled forwarding
        // closure does not know it, a magic __call() gets a trampoline.
        if let Some(m) = desc.strip_prefix(b"M:") {
            let obj = caps.first().map(|v| v.deref()).unwrap_or(Value::Null);
            return self.fcc_method(obj, m, wrapper, scope, true);
        }
        // "S:" is a static method callable in a constant expression.
        let const_expr = desc.starts_with(b"S:");
        if let Some(rest) = desc
            .strip_prefix(b"s:")
            .or_else(|| desc.strip_prefix(b"S:"))
        {
            if let Some(p) = rest.windows(2).rposition(|w| w == b"::") {
                let (cls, m) = (rest[..p].to_vec(), rest[p + 2..].to_vec());
                if const_expr {
                    // zend_ast_evaluate(): a __callStatic() trampoline cannot
                    // back a constant-expression callable.
                    let magic = self.classes.get(&lower(&cls)).is_some_and(|c| {
                        c.get_method(&lower(&m)).is_none()
                            && c.get_method(b"__callstatic").is_some()
                    });
                    if magic {
                        return Err((
                            b"Error",
                            "Creating a callable for the magic __callStatic() method is not supported in constant expressions"
                                .to_string(),
                        ));
                    }
                }
                return self.fcc_static(&cls, &m, this, wrapper, scope);
            }
        }
        Ok(self.make_closure_object(wrapper))
    }

    fn fcc_function(&mut self, name: &[u8], ns_fallback: bool) -> FccResult {
        let name = name.strip_prefix(b"\\").unwrap_or(name);
        let mut cands = vec![name.to_vec()];
        if ns_fallback {
            if let Some(p) = name.iter().rposition(|&b| b == b'\\') {
                cands.push(name[p + 1..].to_vec());
            }
        }
        for c in &cands {
            let l = lower(c);
            if let Some(op) = self.user_functions.get(&l) {
                let canon = if op.name.is_empty() {
                    c.clone()
                } else {
                    op.name.clone()
                };
                return Ok(self.make_closure_object(str_val(&canon)));
            }
            if self.functions.contains_key(&l) {
                return Ok(self.make_closure_object(str_val(&l)));
            }
        }
        Err((
            b"Error",
            format!(
                "Call to undefined function {}()",
                String::from_utf8_lossy(name)
            ),
        ))
    }

    fn fcc_dynamic(
        &mut self,
        callable: Value,
        wrapper: Value,
        scope: Option<Vec<u8>>,
    ) -> FccResult {
        match &callable {
            v if v.is_closure_obj() => Ok(callable.clone()),
            Value::String(s) => {
                let b = s.as_bytes().to_vec();
                // (zend_init_dynamic_call_string() splits at the last "::")
                if let Some(p) = b.windows(2).rposition(|w| w == b"::") {
                    by_name_class_check(&b[..p])?;
                    return self.fcc_static(&b[..p], &b[p + 2..], None, wrapper, scope);
                }
                self.fcc_function(&b, false)
            }
            Value::Array(arr) => {
                let (t, m) = {
                    let a = arr.borrow();
                    (
                        a.get_int(0).map(|v| v.deref()),
                        a.get_int(1).map(|v| v.deref()),
                    )
                };
                match (t, m) {
                    (Some(o @ Value::Object(_)), Some(Value::String(m))) => {
                        self.fcc_method(o, m.as_bytes(), wrapper, scope, false)
                    }
                    (Some(Value::String(c)), Some(Value::String(m))) => {
                        by_name_class_check(c.as_bytes())?;
                        self.fcc_static(c.as_bytes(), m.as_bytes(), None, wrapper, scope)
                    }
                    _ => Err((
                        b"Error",
                        "Array callback must have exactly two elements".to_string(),
                    )),
                }
            }
            Value::Object(o) => {
                let cl = lower(&o.borrow().class_name);
                if self
                    .classes
                    .get(&cl)
                    .map(|c| c.get_method(b"__invoke").is_some())
                    .unwrap_or(false)
                {
                    self.fcc_method(callable.clone(), b"__invoke", wrapper, scope, false)
                } else {
                    Err((
                        b"Error",
                        format!(
                            "Object of type {} is not callable",
                            crate::value::display_class_name(&o.borrow().class_name)
                        ),
                    ))
                }
            }
            other => Err((
                b"Error",
                format!(
                    "Value of type {} is not callable",
                    Vm::zval_type_name(other)
                ),
            )),
        }
    }

    /// The internal method (stub class declaring it, stub method) that a
    /// call of `ml` on an object of class `cl` (both lowercase) runs: the
    /// class is internal or inherits the method from an internal ancestor.
    /// None when a user class declares it (or there is no such method).
    #[inline(never)]
    pub(crate) fn internal_method_of(
        &self,
        cl: &[u8],
        ml: &[u8],
    ) -> Option<(
        &'static crate::stubs::StubClass,
        &'static crate::stubs::StubFunc,
    )> {
        if let Some(c) = self.classes.get(cl).filter(|c| c.filename.is_some()) {
            if let Some(m) = c.get_method(ml) {
                let d = lower(&m.declaring_class);
                if d.is_empty() || self.classes.get(&d).is_some_and(|dc| dc.filename.is_some()) {
                    return None;
                }
            }
        }
        let base = self.internal_base_class(cl)?;
        crate::stubs::find_method(&base, ml)
    }

    /// `$obj->m(...)` (or Closure::fromCallable([$obj, 'm'])) of an internal
    /// method: a closure over the method itself, `[$obj, 'Method']` (or
    /// "Class::method" for a static one), like zend_closure_from_frame().
    /// None when `m` is no internal method of the object's class.
    #[inline(never)]
    fn fcc_internal_method(
        &mut self,
        obj: &Value,
        cl: &[u8],
        m: &[u8],
        scope: Option<&[u8]>,
    ) -> Option<FccResult> {
        let ml = lower(m);
        let (sc, sm) = self.internal_method_of(cl, &ml)?;
        let class_name = match obj {
            Value::Object(o) => o.borrow().class_name.clone(),
            _ => cl.to_vec(),
        };
        if sm.visibility() != crate::object::Visibility::Public {
            if let Some(msg) = self.check_visibility(
                sm.visibility(),
                sc.name.as_bytes(),
                &class_name,
                sm.name,
                false,
                scope,
            ) {
                return Some(Err((b"Error", msg)));
            }
            // (the closure is called as an array callable: from the scope
            // it was created in)
            if !sm.is_static() {
                let arr = Rc::new(RefCell::new(PhpArray::new()));
                arr.borrow_mut().push(obj.clone());
                arr.borrow_mut().push(str_val(sm.name.as_bytes()));
                self.fcc_scoped_arrays.retain(|(w, _)| w.strong_count() > 0);
                self.fcc_scoped_arrays
                    .push((Rc::downgrade(&arr), scope.map(lower).unwrap_or_default()));
                return Some(Ok(self.make_closure_object(Value::Array(arr))));
            }
            return None;
        }
        if sm.is_static() {
            let mut n = class_name;
            n.extend_from_slice(b"::");
            n.extend_from_slice(sm.name.as_bytes());
            // (when it can be called by name)
            self.internal_static_target(&str_val(&n))?;
            return Some(Ok(self.make_closure_object(str_val(&n))));
        }
        let mut arr = PhpArray::new();
        arr.push(obj.clone());
        arr.push(str_val(sm.name.as_bytes()));
        Some(Ok(self.make_closure_object(Value::Array(Rc::new(
            RefCell::new(arr),
        )))))
    }

    /// Closure::fromCallable('C::m') / (['C', 'm']) of a static method of
    /// the internal class `cl` (lowercase): a closure over "C::m".
    #[inline(never)]
    fn fcc_internal_static_named(&mut self, cl: &[u8], m: &[u8]) -> Option<FccResult> {
        let (_, sm) = self.internal_method_of(cl, &lower(m))?;
        if !sm.is_static() {
            return None;
        }
        let mut n = self.builtin_canonical_name(cl).into_bytes();
        n.extend_from_slice(b"::");
        n.extend_from_slice(sm.name.as_bytes());
        self.internal_static_target(&str_val(&n))?;
        Some(Ok(self.make_closure_object(str_val(&n))))
    }

    /// `parent::m(...)` (or `Internal::m(...)`) of an instance method of the
    /// internal ancestor `cl` (lowercase) with a compatible `$this`: a
    /// closure over the method bound to `$this`. None when the object's
    /// class redeclares `m` (or no such method).
    #[inline(never)]
    fn fcc_internal_parent_method(
        &mut self,
        cl: &[u8],
        m: &[u8],
        this: Option<&Value>,
        scope: &Option<Vec<u8>>,
    ) -> Option<FccResult> {
        let Some(t @ Value::Object(o)) = this else {
            return None;
        };
        let ocl = lower(&o.borrow().class_name);
        if !self.class_extends(&ocl, cl) {
            return None;
        }
        let ml = lower(m);
        // (a user redeclaration would be what [$this, 'm'] calls)
        let redeclared = self.classes.get(&ocl).is_some_and(|c| {
            c.get_method(&ml).is_some_and(|md| {
                self.classes
                    .get(&lower(&md.declaring_class))
                    .is_some_and(|dc| dc.filename.is_some())
            })
        });
        let (_, sm) = self.internal_method_of(cl, &ml)?;
        if redeclared || sm.is_static() {
            return None;
        }
        let t = t.clone();
        self.fcc_internal_method(&t, &ocl, m, scope.as_deref())
    }

    /// Whether the first internal class up the parent chain of the user
    /// class `cl` (lowercase) has the method `ml`.
    #[inline(never)]
    fn internal_ancestor_has_method(&self, cl: &[u8], ml: &[u8]) -> bool {
        let mut cur = cl.to_vec();
        for _ in 0..64 {
            let Some(c) = self.classes.get(&cur) else {
                return self.internal_class(&cur).is_some()
                    && crate::stubs::find_method(&cur, ml).is_some();
            };
            match &c.parent {
                Some(p) => cur = lower(p),
                None => return false,
            }
        }
        false
    }

    fn fcc_method(
        &mut self,
        obj: Value,
        m: &[u8],
        wrapper: Value,
        scope: Option<Vec<u8>>,
        dynamic: bool,
    ) -> FccResult {
        let o = match &obj {
            Value::Object(o) => o.clone(),
            // Generator methods: the forwarding closure calls them.
            Value::Generator(_) => {
                const GEN_METHODS: [&[u8]; 8] = [
                    b"current",
                    b"key",
                    b"next",
                    b"rewind",
                    b"send",
                    b"throw",
                    b"valid",
                    b"getreturn",
                ];
                if GEN_METHODS.contains(&lower(m).as_slice()) {
                    return Ok(self.make_closure_object(wrapper));
                }
                return Err((
                    b"Error",
                    format!(
                        "Call to undefined method Generator::{}()",
                        crate::string::msg_str(m)
                    ),
                ));
            }
            other => {
                return Err((
                    b"Error",
                    format!(
                        "Call to a member function {}() on {}",
                        String::from_utf8_lossy(m),
                        Vm::value_type_name(other)
                    ),
                ));
            }
        };
        if obj.is_closure_obj() {
            if m.eq_ignore_ascii_case(b"__invoke") {
                return Ok(obj);
            }
            return Ok(self.make_closure_object(wrapper));
        }
        let class_name = o.borrow().class_name.clone();
        let cl = lower(&class_name);
        if let Some(r) = self.fcc_internal_method(&obj, &cl, m, scope.as_deref()) {
            return r;
        }
        let ml = lower(m);
        let ce = match self.classes.get(&cl) {
            Some(c) => c,
            // stdClass has no methods.
            None if cl.as_slice() == b"stdclass" => {
                return Err((
                    b"Error",
                    format!(
                        "Call to undefined method stdClass::{}()",
                        crate::string::msg_str(m)
                    ),
                ));
            }
            // incomplete_class_get_method()
            None if cl.as_slice() == b"__php_incomplete_class" => {
                let mut a = PhpArray::new();
                a.push(obj.clone());
                a.push(str_val(m));
                let cb = Value::Array(Rc::new(RefCell::new(a)));
                return Err((
                    b"Error",
                    Self::incomplete_method_call_msg(&cb).unwrap_or_default(),
                ));
            }
            // Builtin classes: keep the forwarding closure (when the class
            // has that method).
            None => {
                if self.internal_class(&cl).is_some()
                    && crate::stubs::find_method(&cl, &ml).is_none()
                    && crate::stubs::find_method(&cl, b"__call").is_none()
                {
                    return Err((
                        b"Error",
                        format!(
                            "Call to undefined method {}::{}()",
                            crate::value::display_class_name(&class_name),
                            crate::string::msg_str(m)
                        ),
                    ));
                }
                return Ok(self.make_closure_object(wrapper));
            }
        };
        let method = ce.methods.get(&ml).map(|md| {
            (
                md.is_static,
                md.visibility,
                md.declaring_class.clone(),
                md.name.clone(),
            )
        });
        let has_call = ce.get_method(b"__call").is_some();
        let canon = ce.name.clone();
        // (a __call() trampoline: zend_get_call_trampoline_func())
        let _ = dynamic;
        let magic = |vm: &mut Vm| -> FccResult {
            Ok(vm.magic_call_closure(&canon, m, Some(obj.clone()), true))
        };
        match method {
            Some((is_static, vis, decl, mname)) => {
                if let Some(msg) = self.check_visibility(
                    vis,
                    &decl,
                    &class_name,
                    &String::from_utf8_lossy(&mname),
                    false,
                    scope.as_deref(),
                ) {
                    if has_call {
                        return magic(self);
                    }
                    return Err((b"Error", msg));
                }
                if is_static {
                    let mut n = class_name.clone();
                    n.extend_from_slice(b"::");
                    n.extend_from_slice(&mname);
                    return Ok(self.make_closure_object(str_val(&n)));
                }
                let mut arr = PhpArray::new();
                arr.push(obj.clone());
                arr.push(str_val(&mname));
                let line = self.current_line;
                let inner =
                    self.callable_to_closure_raw(Value::Array(Rc::new(RefCell::new(arr))), line);
                if self.current_exception.is_some() {
                    return Err((b"", String::new()));
                }
                Ok(self.make_closure_object(inner))
            }
            None if has_call => magic(self),
            // A method inherited from an internal ancestor: the forwarding
            // closure calls it.
            None if self.internal_ancestor_has_method(&cl, &ml) => {
                Ok(self.make_closure_object(wrapper))
            }
            None => Err((
                b"Error",
                format!(
                    "Call to undefined method {}::{}()",
                    crate::value::display_class_name(&class_name),
                    crate::string::msg_str(m)
                ),
            )),
        }
    }

    /// `C::m(...)` of an internal class `cl` (lowercase): a closure over the
    /// method itself when the VM implements it (its frame is the method's,
    /// as for zend_closure_from_frame()), else over the forwarding `wrapper`.
    #[inline(never)]
    fn fcc_internal_static(&mut self, cl: &[u8], m: &[u8], wrapper: Value) -> Value {
        let mut name = cl.to_vec();
        name.extend_from_slice(b"::");
        name.extend_from_slice(&lower(m));
        if self.internal_static_target(&str_val(&name)).is_none() {
            return self.make_closure_object(wrapper);
        }
        let mut n = self.builtin_canonical_name(cl).into_bytes();
        n.extend_from_slice(b"::");
        n.extend_from_slice(
            crate::stubs::find_method(cl, &lower(m))
                .map(|(_, md)| md.name.as_bytes())
                .unwrap_or(m),
        );
        self.make_closure_object(str_val(&n))
    }

    #[inline(never)]
    fn fcc_trait_self_scope(&self, cls: &[u8], scope: Option<&[u8]>) -> Option<Vec<u8>> {
        let t = self.classes.get(&lower(cls)).filter(|t| t.is_trait)?;
        let sc = self.classes.get(&lower(scope?)).filter(|c| !c.is_trait)?;
        let _ = t;
        Some(sc.name.clone())
    }

    fn fcc_static(
        &mut self,
        cls: &[u8],
        m: &[u8],
        this: Option<Value>,
        wrapper: Value,
        scope: Option<Vec<u8>>,
    ) -> FccResult {
        let cls: Vec<u8> = if let Some(p) = cls.strip_prefix(b"parent:") {
            p.to_vec()
        } else if cls.eq_ignore_ascii_case(b"static") {
            let called = self.called_class_stack.last().cloned().or_else(|| {
                this.as_ref().and_then(|t| match t {
                    Value::Object(o) => Some(o.borrow().class_name.clone()),
                    _ => None,
                })
            });
            match called {
                Some(c) => c,
                None => {
                    return Err((
                        b"Error",
                        "Cannot use \"static\" when no class scope is active".to_string(),
                    ));
                }
            }
        } else {
            cls.to_vec()
        };
        let cls = cls.strip_prefix(b"\\").map(|c| c.to_vec()).unwrap_or(cls);
        // `self::m(...)` compiled in a trait (self is the trait's name):
        // the class using the trait, the scope it runs in.
        let cls = match self.fcc_trait_self_scope(&cls, scope.as_deref()) {
            Some(c) => c,
            None => cls,
        };
        let cl = lower(&cls);
        if !self.classes.contains_key(&cl) {
            self.try_autoload_class(&cls);
        }
        let ce = match self.classes.get(&cl) {
            Some(c) => c,
            None => {
                if cl.as_slice() == b"stdclass" {
                    return Err((
                        b"Error",
                        format!(
                            "Call to undefined method stdClass::{}()",
                            crate::string::msg_str(m)
                        ),
                    ));
                }
                if self.is_known_builtin_class(&cl) {
                    if let Some(r) = self.fcc_internal_parent_method(&cl, m, this.as_ref(), &scope)
                    {
                        return r;
                    }
                    return Ok(self.fcc_internal_static(&cl, m, wrapper));
                }
                return Err((
                    b"Error",
                    format!("Class \"{}\" not found", crate::string::msg_str(&cls)),
                ));
            }
        };
        let canon = ce.name.clone();
        let ml = lower(m);
        let is_trait = ce.is_trait;
        let is_abstract = ce.methods.get(&ml).is_some_and(|md| md.is_abstract);
        let method = ce.methods.get(&ml).map(|md| {
            (
                md.is_static,
                md.visibility,
                md.declaring_class.clone(),
                md.name.clone(),
                md.op_array.clone(),
                md.attributes.clone(),
            )
        });
        let has_call_static = ce.get_method(b"__callstatic").is_some();
        let has_call = ce.get_method(b"__call").is_some();
        // A compatible $this is forwarded to non-static methods.
        let bound_this = this.filter(|t| match t {
            Value::Object(o) => {
                let ocl = lower(&o.borrow().class_name);
                ocl == cl || self.class_extends(&ocl, &cl)
            }
            _ => false,
        });
        match method {
            Some((is_static, vis, decl, mname, op, attrs)) => {
                if let Some(msg) = self.check_visibility(
                    vis,
                    &decl,
                    &canon,
                    &String::from_utf8_lossy(&mname),
                    false,
                    scope.as_deref(),
                ) {
                    if has_call && bound_this.is_some() {
                        return Ok(self.magic_call_closure(&canon, m, bound_this, true));
                    }
                    if has_call_static {
                        return Ok(self.magic_call_closure(&canon, m, None, true));
                    }
                    return Err((b"Error", msg));
                }
                let mut n = canon.clone();
                n.extend_from_slice(b"::");
                n.extend_from_slice(&mname);
                // zend_std_get_static_method(): abstract methods cannot be
                // called; static trait methods are deprecated.
                if is_abstract && (is_static || bound_this.is_none()) {
                    let decl_name = self
                        .classes
                        .get(&lower(&decl))
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| canon.clone());
                    return Err((
                        b"Error",
                        format!(
                            "Cannot call abstract method {}::{}()",
                            String::from_utf8_lossy(&decl_name),
                            String::from_utf8_lossy(&mname)
                        ),
                    ));
                }
                if is_static && is_trait {
                    let line = self.current_line;
                    self.emit_deprecated_at(
                        &format!(
                            "Calling static trait method {}::{} is deprecated, it should only be called on a class using the trait",
                            crate::string::msg_str(&canon),
                            crate::string::msg_str(&mname)
                        ),
                        line,
                    );
                    if self.current_exception.is_some() {
                        return Err((b"", String::new()));
                    }
                }
                if is_static {
                    return Ok(self.make_closure_object(str_val(&n)));
                }
                match bound_this {
                    Some(t) => {
                        self.user_functions.entry(lower(&n)).or_insert_with(|| {
                            let mut op = op;
                            if op.attributes.is_empty() && !attrs.is_empty() {
                                std::rc::Rc::make_mut(&mut op).attributes = attrs;
                            }
                            op
                        });
                        let mut arr = PhpArray::new();
                        arr.push(str_val(&n));
                        arr.push(t);
                        Ok(self.make_closure_object(Value::Array(Rc::new(RefCell::new(arr)))))
                    }
                    None => Err((
                        b"Error",
                        format!(
                            "Non-static method {}::{}() cannot be called statically",
                            String::from_utf8_lossy(&canon),
                            String::from_utf8_lossy(&mname)
                        ),
                    )),
                }
            }
            None if has_call && bound_this.is_some() => {
                Ok(self.magic_call_closure(&canon, m, bound_this, true))
            }
            None if has_call_static => Ok(self.magic_call_closure(&canon, m, None, true)),
            // An enum's from() / tryFrom() / cases()
            None if ce.is_enum
                && (ml == b"cases"
                    || (ce.enum_backing_type.is_some()
                        && matches!(ml.as_slice(), b"from" | b"tryfrom"))) =>
            {
                let mut n = canon.clone();
                n.extend_from_slice(b"::");
                n.extend_from_slice(if ml == b"tryfrom" { b"tryFrom" } else { &ml });
                Ok(self.make_closure_object(str_val(&n)))
            }
            None => Err((
                b"Error",
                format!(
                    "Call to undefined method {}::{}()",
                    crate::string::msg_str(&canon),
                    crate::string::msg_str(m)
                ),
            )),
        }
    }
}

impl Vm {
    /// A closure forwarding to `__call`/`__callStatic` for `method`
    /// (the closure PHP builds over a call trampoline).
    /// `fcc`: a first-class callable (whose debug info lists the
    /// trampoline's `$arguments`), not Closure::fromCallable().
    fn magic_call_closure(
        &mut self,
        class_canon: &[u8],
        method: &[u8],
        obj: Option<Value>,
        fcc: bool,
    ) -> Value {
        use crate::opcode::Op;
        let mut target = class_canon.to_vec();
        target.extend_from_slice(b"::");
        target.extend_from_slice(method);
        // One trampoline per (target, instance/static): equal closures compare equal.
        let name = format!(
            "__closure_fcc_tramp_{}{}_{}",
            if obj.is_some() { "i" } else { "s" },
            if fcc { "p" } else { "" },
            // (no "::": the name must not read as a "Class::method" callable)
            String::from_utf8_lossy(&lower(&target)).replace("::", ":")
        )
        .into_bytes();
        let has_obj = obj.is_some();
        // The trampoline is a method of the magic method's declaring class,
        // with its attributes (#[\Deprecated], #[\NoDiscard]).
        let magic: &[u8] = if has_obj { b"__call" } else { b"__callStatic" };
        let (decl_canon, magic_attrs) = self
            .classes
            .get(&lower(class_canon))
            .and_then(|c| c.get_method(&lower(magic)))
            .map(|m| {
                let decl = self
                    .classes
                    .get(&m.declaring_class)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| class_canon.to_vec());
                let attrs = if m.attributes.is_empty() {
                    m.op_array.attributes.clone()
                } else {
                    m.attributes.clone()
                };
                (decl, attrs)
            })
            .unwrap_or_else(|| (class_canon.to_vec(), Vec::new()));
        let mut display = decl_canon.clone();
        display.extend_from_slice(b"::");
        display.extend_from_slice(method);
        let mut op = OpArray::new();
        op.name = name.clone();
        op.fcc_target_name = Some(display.clone());
        op.display_name = display;
        op.attributes = magic_attrs;
        op.scope_class = Some(lower(&decl_canon));
        if !has_obj {
            op.closure_called_class = Some(class_canon.to_vec());
        }
        if has_obj {
            op.get_or_create_cv(b"__fcc_captured");
        }
        // (zend_get_call_trampoline_func(): `mixed ...$arguments`)
        let args_cv = op.get_or_create_cv(b"arguments");
        op.param_count = 1;
        op.required_param_count = 0;
        op.variadic_param = Some(args_cv);
        op.param_types = vec![None; args_cv as usize];
        op.param_types.push(Some(crate::opcode::ParamTypeInfo {
            param_name: b"arguments".to_vec(),
            param_type: crate::opcode::ParamType::Simple(b"mixed".to_vec()),
            implicitly_nullable: false,
        }));
        let zero = op.add_literal(Value::Long(0));
        if has_obj {
            let m = op.add_literal(str_val(method));
            op.emit(Op {
                opcode: OpCode::InitMethodCall,
                op1: OperandType::Cv(0),
                op2: OperandType::Const(m),
                result: OperandType::Unused,
                line: 0,
            });
        } else {
            let t = op.add_literal(str_val(&target));
            op.emit(Op {
                opcode: OpCode::InitFCall,
                op1: OperandType::Const(t),
                op2: OperandType::Const(zero),
                result: OperandType::Unused,
                line: 0,
            });
        }
        op.emit(Op {
            opcode: OpCode::SendUnpack,
            op1: OperandType::Cv(args_cv),
            op2: OperandType::Unused,
            result: OperandType::Unused,
            line: 0,
        });
        let tmp = op.alloc_temp();
        op.emit(Op {
            opcode: OpCode::DoFCall,
            op1: OperandType::Unused,
            op2: OperandType::Unused,
            result: OperandType::Tmp(tmp),
            line: 0,
        });
        op.emit(Op {
            opcode: OpCode::Return,
            op1: OperandType::Tmp(tmp),
            op2: OperandType::Unused,
            result: OperandType::Unused,
            line: 0,
        });
        self.user_functions.insert(name.clone(), Rc::new(op));
        let inner = match obj {
            Some(o) => {
                let mut arr = PhpArray::new();
                arr.push(str_val(&name));
                arr.push(o);
                Value::Array(Rc::new(RefCell::new(arr)))
            }
            None => str_val(&name),
        };
        self.make_closure_object(inner)
    }

    /// Resolve `self`/`parent`/`static` in a callable class name relative to
    /// `scope` (lowercase), emitting the PHP 8.2 deprecation.
    fn callable_class_relative(
        &mut self,
        cls: &[u8],
        scope: Option<&[u8]>,
    ) -> Result<Vec<u8>, String> {
        let l = lower(cls);
        let kw: &str = match l.as_slice() {
            b"self" => "self",
            b"parent" => "parent",
            b"static" => "static",
            _ => return Ok(cls.strip_prefix(b"\\").unwrap_or(cls).to_vec()),
        };
        let scope = match scope {
            Some(s) => s.to_vec(),
            None => {
                return Err(format!(
                    "cannot access \"{}\" when no class scope is active",
                    kw
                ));
            }
        };
        match kw {
            "self" => Ok(scope),
            "parent" => match self.classes.get(&scope).and_then(|c| c.parent.clone()) {
                Some(p) => Ok(p),
                None => Err(
                    "cannot access \"parent\" when current class scope has no parent".to_string(),
                ),
            },
            _ => Ok(self.called_class_stack.last().cloned().unwrap_or(scope)),
        }
    }

    /// Closure::fromCallable() (zend_create_closure_from_callable): resolve
    /// the callable from `scope` (lowercase class of the caller, with its
    /// `$this`), returning the closure or the zend_is_callable error text.
    pub fn closure_from_callable(
        &mut self,
        callable: &Value,
        scope: Option<Vec<u8>>,
        this: Option<Value>,
    ) -> Result<Value, String> {
        let callable = callable.deref();
        if callable.is_closure_obj() {
            return Ok(callable);
        }
        // zend_is_callable_ex() decides; its deprecations are raised while
        // the callable is resolved below.
        let check = self.zend_is_callable_ex(
            &callable,
            crate::callable::IS_CALLABLE_SUPPRESS_DEPRECATIONS,
        );
        if self.current_exception.is_some() {
            return Err(String::new());
        }
        // The check's deprecations ("self"/"parent"/"static", ["A",
        // "B::m"] forms) are raised before its error or the closure.
        let line = self.current_line;
        for d in self.is_callable_pure(&callable, 0).deprecations {
            self.emit_deprecated_at(&d, line);
        }
        if self.current_exception.is_some() {
            return Err(String::new());
        }
        if !check.ok {
            return Err(check.error.unwrap_or_default());
        }
        match &callable {
            Value::String(s) => {
                let b = s.as_bytes().to_vec();
                if let Some(p) = b.windows(2).position(|w| w == b"::") {
                    let cls = self.callable_class_relative(&b[..p], scope.as_deref())?;
                    return self.resolve_callable_method(&cls, &b[p + 2..], None, this, scope);
                }
                let name = b.strip_prefix(b"\\").unwrap_or(&b).to_vec();
                let l = lower(&name);
                if let Some(op) = self.user_functions.get(&l) {
                    if !crate::closure::is_closure_fn_name(&op.name) {
                        let canon = op.name.clone();
                        return Ok(self.make_closure_object(str_val(&canon)));
                    }
                }
                if self.functions.contains_key(&l) {
                    return Ok(self.make_closure_object(str_val(&l)));
                }
                Err(format!(
                    "function \"{}\" not found or invalid function name",
                    // (printed with %s: a NUL ends it)
                    crate::string::msg_str(b.split(|&c| c == 0).next().unwrap_or_default())
                ))
            }
            Value::Array(arr) => {
                let (len, t, m) = {
                    let a = arr.borrow();
                    (
                        a.len(),
                        a.get_int(0).map(|v| v.deref()),
                        a.get_int(1).map(|v| v.deref()),
                    )
                };
                if len != 2 {
                    return Err("array callback must have exactly two members".to_string());
                }
                let m = match m {
                    Some(Value::String(m)) => m.as_bytes().to_vec(),
                    _ => return Err("second array member is not a valid method".to_string()),
                };
                match t {
                    Some(c) if c.is_closure_obj() && m.eq_ignore_ascii_case(b"__invoke") => Ok(c),
                    Some(Value::Object(o)) => {
                        let ocls = o.borrow().class_name.clone();
                        let (cls, meth) = match m.windows(2).position(|w| w == b"::") {
                            Some(p) => (
                                self.callable_class_relative(
                                    &m[..p],
                                    scope.as_deref().or(Some(&lower(&ocls))),
                                )?,
                                m[p + 2..].to_vec(),
                            ),
                            None => (ocls, m.clone()),
                        };
                        self.resolve_callable_method(
                            &cls,
                            &meth,
                            Some(Value::Object(o)),
                            None,
                            scope,
                        )
                    }
                    Some(Value::String(c)) => {
                        let cls = self.callable_class_relative(c.as_bytes(), scope.as_deref())?;
                        // ["C", "self::m"] / ["C", "A::m"] (deprecated forms):
                        // the scoped class is relative to C
                        // (zend_is_callable_check_func()'s ce_org).
                        let (cls, meth) = match m.windows(2).position(|w| w == b"::") {
                            Some(p) => (
                                self.callable_class_relative(&m[..p], Some(&lower(&cls)))?,
                                m[p + 2..].to_vec(),
                            ),
                            None => (cls, m.clone()),
                        };
                        self.resolve_callable_method(&cls, &meth, None, this, scope)
                    }
                    _ => Err("first array member is not a valid class name or object".to_string()),
                }
            }
            Value::Object(o) => {
                let cn = o.borrow().class_name.clone();
                let has_invoke = self
                    .classes
                    .get(&lower(&cn))
                    .map(|c| c.get_method(b"__invoke").is_some())
                    .unwrap_or(false);
                if has_invoke {
                    // (the object's get_closure() handler: __invoke() whatever
                    // its visibility)
                    self.resolve_callable_method(
                        &cn,
                        b"__invoke",
                        Some(callable.clone()),
                        None,
                        Some(lower(&cn)),
                    )
                } else {
                    Err("no array or string given".to_string())
                }
            }
            _ => Err("no array or string given".to_string()),
        }
    }

    fn resolve_callable_method(
        &mut self,
        cls: &[u8],
        m: &[u8],
        obj: Option<Value>,
        this: Option<Value>,
        scope: Option<Vec<u8>>,
    ) -> Result<Value, String> {
        let cl = lower(cls);
        if !self.classes.contains_key(&cl) {
            self.try_autoload_class(cls);
        }
        let ce = match self.classes.get(&cl) {
            Some(c) => c,
            None => {
                if self.is_known_builtin_class(&cl) {
                    // A closure over the internal method itself.
                    if let Some(Ok(c)) = match &obj {
                        Some(o) => self.fcc_internal_method(o, &cl, m, scope.as_deref()),
                        None => self.fcc_internal_static_named(&cl, m),
                    } {
                        return Ok(c);
                    }
                    // Builtin class methods: keep the [obj|class, method] callable.
                    let mut arr = PhpArray::new();
                    arr.push(obj.unwrap_or_else(|| str_val(cls)));
                    arr.push(str_val(m));
                    return Ok(self.make_closure_object(Value::Array(Rc::new(RefCell::new(arr)))));
                }
                return Err(format!(
                    "class \"{}\" not found",
                    crate::string::msg_str(cls)
                ));
            }
        };
        let canon = ce.name.clone();
        let ml = lower(m);
        let method = ce.methods.get(&ml).map(|md| {
            (
                md.is_static,
                md.is_abstract,
                md.visibility,
                md.declaring_class.clone(),
                md.name.clone(),
                md.op_array.clone(),
                md.attributes.clone(),
            )
        });
        let has_call = ce.get_method(b"__call").is_some();
        let has_call_static = ce.get_method(b"__callstatic").is_some();
        // A compatible caller $this is used for non-static methods named statically.
        let compat_this = this.filter(|t| match t {
            Value::Object(o) => {
                let ocl = lower(&o.borrow().class_name);
                ocl == cl || self.class_extends(&ocl, &cl)
            }
            _ => false,
        });
        let obj = obj.or(compat_this.clone());
        match method {
            Some((is_static, is_abstract, vis, decl, mname, op, attrs)) => {
                if let Some(msg) = self.check_visibility(
                    vis,
                    &decl,
                    &canon,
                    &String::from_utf8_lossy(&mname),
                    false,
                    scope.as_deref(),
                ) {
                    if obj.is_some() && has_call {
                        return Ok(self.magic_call_closure(&canon, m, obj, false));
                    }
                    if obj.is_none() && has_call_static {
                        return Ok(self.magic_call_closure(&canon, m, None, false));
                    }
                    // "Call to private method A::m() from scope B" -> "cannot access private method A::m()"
                    let vis_s = match vis {
                        crate::object::Visibility::Private => "private",
                        crate::object::Visibility::Protected => "protected",
                        crate::object::Visibility::Public => "public",
                    };
                    let _ = msg;
                    return Err(format!(
                        "cannot access {} method {}::{}()",
                        vis_s,
                        crate::string::msg_str(&canon),
                        crate::string::msg_str(&mname)
                    ));
                }
                if is_abstract {
                    return Err(format!(
                        "cannot call abstract method {}::{}()",
                        crate::string::msg_str(&canon),
                        crate::string::msg_str(&mname)
                    ));
                }
                let mut n = canon.clone();
                n.extend_from_slice(b"::");
                n.extend_from_slice(&mname);
                if is_static {
                    return Ok(self.make_closure_object(str_val(&n)));
                }
                let o = match obj {
                    Some(o) => o,
                    None => {
                        return Err(format!(
                            "non-static method {}::{}() cannot be called statically",
                            crate::string::msg_str(&canon),
                            crate::string::msg_str(&mname)
                        ));
                    }
                };
                let key = lower(&n);
                let mut op = op;
                if op.attributes.is_empty() && !attrs.is_empty() {
                    std::rc::Rc::make_mut(&mut op).attributes = attrs;
                }
                self.user_functions.insert(key, op);
                let mut arr = PhpArray::new();
                arr.push(str_val(&n));
                arr.push(o);
                Ok(self.make_closure_object(Value::Array(Rc::new(RefCell::new(arr)))))
            }
            None => {
                // An enum's from() / tryFrom() / cases()
                let (is_enum, backed) = self.classes.get(&cl).map_or((false, false), |c| {
                    (c.is_enum, c.enum_backing_type.is_some())
                });
                if is_enum
                    && (ml == b"cases" || (backed && matches!(ml.as_slice(), b"from" | b"tryfrom")))
                {
                    let mut n = canon.clone();
                    n.extend_from_slice(b"::");
                    n.extend_from_slice(if ml == b"tryfrom" { b"tryFrom" } else { &ml });
                    return Ok(self.make_closure_object(str_val(&n)));
                }
                // A method inherited from an internal class.
                if obj.is_some() && self.internal_method_frame_name(&cl, &ml).is_some() {
                    let mut arr = PhpArray::new();
                    arr.push(obj.unwrap_or_else(|| str_val(cls)));
                    arr.push(str_val(m));
                    return Ok(self.make_closure_object(Value::Array(Rc::new(RefCell::new(arr)))));
                }
                if obj.is_some() && has_call {
                    return Ok(self.magic_call_closure(&canon, m, obj, false));
                }
                if has_call_static {
                    return Ok(self.magic_call_closure(&canon, m, None, false));
                }
                Err(format!(
                    "class {} does not have a method \"{}\"",
                    crate::string::msg_str(&canon),
                    crate::string::msg_str(m)
                ))
            }
        }
    }
}

impl Vm {
    /// Closure::bind()/bindTo() on a closure created from a callable
    /// (function or method): validates like zend_valid_closure_binding and
    /// returns the rebound closure (or NULL). None for user closures.
    pub fn bind_fake_closure(
        &mut self,
        inner: &Value,
        new_this: &Value,
        scope: &Value,
        scope_provided: bool,
    ) -> Option<Value> {
        let (name, caps) = closure_parts(inner);
        let is_tramp = name.starts_with(b"__closure_fcc_tramp_");
        // A function given a $this (function_closure_with_this): a method
        // of the dummy Closure scope now.
        let fn_this = name.starts_with(b"__bound_closure_")
            && self
                .user_functions
                .get(&lower(&name))
                .is_some_and(|o| o.fcc_target_name.is_some());
        if !fn_this
            && (is_closure_fn_name(&name) || (name.starts_with(b"__closure_fcc_") && !is_tramp))
        {
            return None;
        }
        // __call/__callStatic trampolines: the target is "Class::method".
        let target: Vec<u8> = if is_tramp || fn_this {
            self.user_functions
                .get(&lower(&name))
                .and_then(|o| o.fcc_target_name.clone())?
        } else {
            name.clone()
        };
        let lname = lower(&target);
        // Function scope / static-ness of the target.
        let (func_scope, is_static, mname): (Option<Vec<u8>>, bool, Vec<u8>) =
            match lname.windows(2).position(|w| w == b"::") {
                Some(p) => {
                    let cl = lname[..p].to_vec();
                    // (an internal method: its declaring class)
                    if let Some((sc, sm)) = self.internal_method_of(&cl, &lname[p + 2..]) {
                        (
                            Some(lower(sc.name.as_bytes())),
                            sm.is_static(),
                            sm.name.as_bytes().to_vec(),
                        )
                    } else {
                        let md = self
                            .classes
                            .get(&cl)
                            .and_then(|c| c.methods.get(&lname[p + 2..]));
                        match md {
                            Some(md) => (
                                Some(md.declaring_class.to_ascii_lowercase()),
                                md.is_static,
                                md.name.clone(),
                            ),
                            None => (Some(cl), caps.is_empty(), target[p + 2..].to_vec()),
                        }
                    }
                }
                None => (None, false, name.clone()),
            };
        let has_this = matches!(new_this, Value::Object(_));
        let requested: Option<Vec<u8>> = if scope_provided {
            match scope {
                Value::Null => None,
                Value::Object(o) => Some(lower(&o.borrow().class_name)),
                Value::String(s) if s.as_bytes().eq_ignore_ascii_case(b"static") => {
                    func_scope.clone()
                }
                Value::String(s) => {
                    let n = s
                        .as_bytes()
                        .strip_prefix(b"\\")
                        .unwrap_or(s.as_bytes())
                        .to_vec();
                    let l = lower(&n);
                    if !self.classes.contains_key(&l) && !self.is_known_builtin_class(&l) {
                        self.try_autoload_class(&n);
                    }
                    if !self.classes.contains_key(&l) && !self.is_known_builtin_class(&l) {
                        self.emit_warning(&format!(
                            "Class \"{}\" not found",
                            crate::string::msg_str(&n)
                        ));
                        return Some(Value::Null);
                    }
                    Some(l)
                }
                _ => func_scope.clone(),
            }
        } else {
            func_scope.clone()
        };
        let scope_display = |vm: &Vm, l: &[u8]| -> String {
            vm.classes
                .get(l)
                .map(|c| String::from_utf8_lossy(&c.name).to_string())
                .unwrap_or_else(|| vm.builtin_canonical_name(l))
        };
        if has_this {
            if is_static {
                self.emit_warning(
                    "Cannot bind an instance to a static closure, this will be an error in PHP 9",
                );
                return Some(Value::Null);
            }
            if let Some(fs) = &func_scope {
                let ocl = match new_this {
                    Value::Object(o) => lower(&o.borrow().class_name),
                    _ => Vec::new(),
                };
                if ocl != *fs && !self.class_extends(&ocl, fs) {
                    let oname = match new_this {
                        Value::Object(o) => {
                            crate::value::display_class_name(&o.borrow().class_name)
                        }
                        _ => String::new(),
                    };
                    let msg = format!(
                        "Cannot bind method {}::{}() to object of class {}, this will be an error in PHP 9",
                        scope_display(self, fs),
                        crate::string::msg_str(&mname),
                        oname
                    );
                    self.emit_warning(&msg);
                    return Some(Value::Null);
                }
            }
        } else if func_scope.is_some() && !is_static {
            self.emit_warning("Cannot unbind $this of method, this will be an error in PHP 9");
            return Some(Value::Null);
        }
        if let Some(sc) = &requested {
            if Some(sc) != func_scope.as_ref() && !self.classes.contains_key(sc) {
                let msg = format!(
                    "Cannot bind closure to scope of internal class {}, this will be an error in PHP 9",
                    scope_display(self, sc)
                );
                self.emit_warning(&msg);
                return Some(Value::Null);
            }
        }
        if requested != func_scope {
            if func_scope.is_none() {
                self.emit_warning("Cannot rebind scope of closure created from function, this will be an error in PHP 9");
            } else {
                self.emit_warning("Cannot rebind scope of closure created from method, this will be an error in PHP 9");
            }
            return Some(Value::Null);
        }
        let user_class = is_tramp
            || func_scope
                .as_ref()
                .map(|s| self.classes.contains_key(s))
                .unwrap_or(false);
        let new_inner = match new_this {
            Value::Object(_) if fn_this => {
                let f = target[b"Closure::".len()..].to_vec();
                self.function_closure_with_this(&f, new_this)
            }
            Value::Object(_) if func_scope.is_some() && !user_class => {
                // Builtin class method: keep the [$obj, 'method'] form.
                let mut arr = PhpArray::new();
                arr.push(new_this.clone());
                arr.push(str_val(&mname));
                Value::Array(Rc::new(RefCell::new(arr)))
            }
            Value::Object(_) if func_scope.is_some() => {
                let mut arr = PhpArray::new();
                arr.push(str_val(&name));
                arr.push(new_this.clone());
                Value::Array(Rc::new(RefCell::new(arr)))
            }
            // A user function given a `$this`: it runs with it, in the dummy
            // Closure scope (zend_create_closure_ex()).
            Value::Object(_) if self.user_functions.contains_key(&lname) && !is_tramp => {
                self.function_closure_with_this(&name, new_this)
            }
            _ => str_val(&name),
        };
        Some(self.make_closure_object(new_inner))
    }
}

impl Vm {
    /// The closure of the user function `name` bound to `this` (bindTo()
    /// of `f(...)`): a copy of the function taking `$this` (a bound
    /// closure's layout), still showing as the fake closure of
    /// "Closure::f" (the dummy scope).
    #[inline(never)]
    fn function_closure_with_this(&mut self, name: &[u8], this: &Value) -> Value {
        let r = self.bind_closure_raw(&str_val(name), this.clone(), Value::Null, false);
        let (bound, _) = closure_parts(&r);
        if let Some(op) = self.user_functions.get_mut(&lower(&bound)) {
            let mut t = b"Closure::".to_vec();
            t.extend_from_slice(name);
            let op = Rc::make_mut(op);
            op.fcc_target_name = Some(t);
            // (its parameters follow the $this it receives: see arg_cv_bound())
            // (one entry per declared parameter: the leading $this is hidden,
            // see closure_hidden_params())
            op.param_attributes
                .resize((op.param_count as usize).saturating_sub(1), Vec::new());
            if op.closure_params.is_empty() {
                op.closure_params = (1..op.param_count as usize)
                    .filter_map(|i| {
                        let n = op.cv_names.get(i)?.clone();
                        Some((n, op.by_ref_params.get(i).copied().unwrap_or(false)))
                    })
                    .collect();
            }
        }
        r
    }
}

impl Vm {
    /// Instantiate a closure compiled from a constant expression (class
    /// constant, property default, attribute argument).
    pub fn instantiate_const_closure(&mut self, op: OpArray) -> Value {
        let name = op.name.clone();
        let desc = op.fcc_desc.clone();
        let scope = op.scope_class.clone();
        self.register_user_function(&name, op);
        match desc {
            Some(d) => match self.make_fcc(str_val(&name), &d, None, scope) {
                Ok(c) => c,
                Err((cls, msg)) => {
                    if self.current_exception.is_none() && !cls.is_empty() {
                        let line = self.current_line;
                        let exc = self.create_exception(cls, &msg, line);
                        self.current_exception = Some(exc);
                    }
                    Value::Null
                }
            },
            None => self.make_closure_object(str_val(&name)),
        }
    }
}

impl Vm {
    /// A closure takes the scope of the function creating it (the op_array
    /// scope of a rebound closure or of an imported trait method may differ
    /// from the one it was compiled with).
    pub fn closure_scope_at_creation(
        &mut self,
        inner: Value,
        frame_scope: Option<&Vec<u8>>,
    ) -> Value {
        let (name, caps) = closure_parts(&inner);
        if !is_closure_fn_name(&name) {
            return inner;
        }
        let l = lower(&name);
        let op = match self.user_functions.get(&l) {
            Some(op) => op,
            None => return inner,
        };
        // Called class (static::) for closures without a bound $this.
        let has_this = op
            .closure_capture_names
            .first()
            .map(|n| n.as_slice() == b"this")
            .unwrap_or(false)
            && matches!(caps.first().map(|v| v.deref()), Some(Value::Object(_)));
        let called: Option<Vec<u8>> = if has_this || frame_scope.is_none() {
            None
        } else {
            self.called_class_stack
                .last()
                .cloned()
                .filter(|c| Some(&lower(c)) != frame_scope)
        };
        if op.scope_class.as_ref() == frame_scope
            && called.is_none()
            && op.closure_called_class.is_none()
        {
            return inner;
        }
        let base: Vec<u8> = match name.iter().position(|&b| b == b'@') {
            Some(p) => name[..p].to_vec(),
            None => name.clone(),
        };
        let mut new_name = base;
        new_name.extend_from_slice(b"@");
        new_name.extend_from_slice(frame_scope.map(|s| s.as_slice()).unwrap_or(b""));
        if let Some(c) = &called {
            new_name.extend_from_slice(b"@");
            new_name.extend_from_slice(&lower(c));
        }
        if new_name == name {
            return inner;
        }
        let nl = lower(&new_name);
        if !self.user_functions.contains_key(&nl) {
            let mut nop = (**op).clone();
            // A closure declared in a trait method, created in the class
            // using the trait: its self / __CLASS__ name that class (as the
            // imported method's do).
            let trait_using = op
                .scope_class
                .as_ref()
                .and_then(|sc| self.classes.get(sc))
                .filter(|t| t.is_trait)
                .map(|t| t.name.clone())
                .zip(
                    frame_scope
                        .and_then(|fs| self.classes.get(fs))
                        .filter(|c| !c.is_trait)
                        .map(|c| c.name.clone()),
                );
            if let Some((trait_name, class_name)) = trait_using {
                patch_trait_class_literals(&mut nop, &trait_name, &class_name);
            }
            nop.name = new_name.clone();
            nop.scope_class = frame_scope.cloned();
            nop.closure_called_class = called;
            self.user_functions.entry(nl).or_insert(Rc::new(nop));
        }
        if caps.is_empty() {
            return str_val(&new_name);
        }
        let mut arr = PhpArray::new();
        arr.push(str_val(&new_name));
        for c in caps {
            arr.push(c);
        }
        Value::Array(Rc::new(RefCell::new(arr)))
    }
}

/// A closure's op array rebound from its scope to `new_scope` (lowercase):
/// the literals compiled from `self` / `__CLASS__` name the new scope.
#[inline(never)]
pub(crate) fn retarget_self_literals(vm: &Vm, op: &mut OpArray, new_scope: Option<&[u8]>) {
    if op.class_const_literals.is_empty() || op.scope_class.as_deref() == new_scope {
        return;
    }
    let old = op
        .scope_class
        .as_ref()
        .and_then(|s| vm.classes.get(s))
        .map(|c| c.name.clone());
    let new = new_scope
        .and_then(|s| vm.classes.get(s))
        .map(|c| c.name.clone());
    if let (Some(old), Some(new)) = (old, new) {
        patch_trait_class_literals(op, &old, &new);
    }
}

/// The `Trait` / `Trait::member` literals (class_const_literals) of a
/// function compiled in trait `trait_name` retargeted to `class_name`.
#[inline(never)]
fn patch_trait_class_literals(op: &mut OpArray, trait_name: &[u8], class_name: &[u8]) {
    for i in op.class_const_literals.clone() {
        let Some(Value::String(s)) = op.literals.get(i as usize) else {
            continue;
        };
        let b = s.as_bytes();
        let n = trait_name.len();
        let new = if b.eq_ignore_ascii_case(trait_name) {
            class_name.to_vec()
        } else if b.len() > n + 2
            && b[..n].eq_ignore_ascii_case(trait_name)
            && &b[n..n + 2] == b"::"
        {
            // (a self::method() call: forwarding, in the using class's scope)
            let mut v = b"self".to_vec();
            v.extend_from_slice(&b[n..]);
            v
        } else {
            continue;
        };
        op.literals[i as usize] = Value::String(PhpString::from_vec(new));
    }
}

static CLOSURE_STATICS_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Whether a function declares `static` variables.
pub fn op_has_statics(op: &OpArray) -> bool {
    op.ops
        .iter()
        .any(|o| matches!(o.opcode, OpCode::StaticVarInit))
}

/// Rewrite the static-variable keys of `op` ("<old>::var") for a renamed copy.
fn retarget_static_keys(op: &mut OpArray, old_name: &[u8], new_name: &[u8]) {
    let mut old_prefix = old_name.to_vec();
    old_prefix.extend_from_slice(b"::");
    let mut new_prefix = new_name.to_vec();
    new_prefix.extend_from_slice(b"::");
    let idxs: Vec<u32> = op
        .ops
        .iter()
        .filter(|o| matches!(o.opcode, OpCode::StaticVarInit))
        .filter_map(|o| match o.result {
            OperandType::Const(i) => Some(i),
            _ => None,
        })
        .collect();
    for i in idxs {
        if let Some(Value::String(s)) = op.literals.get(i as usize) {
            if let Some(rest) = s.as_bytes().strip_prefix(old_prefix.as_slice()) {
                let mut k = new_prefix.clone();
                k.extend_from_slice(rest);
                op.literals[i as usize] = Value::String(PhpString::from_vec(k));
            }
        }
    }
}

impl Vm {
    /// Closures with `static` variables get their own copy of the function
    /// per closure object (PHP keeps static variables per closure).
    /// `copy_from`: the closure whose current static values are duplicated
    /// (bind/clone), None for a fresh closure.
    pub fn closure_with_own_statics(&mut self, inner: Value, copy_from: Option<&[u8]>) -> Value {
        let (name, caps) = closure_parts(&inner);
        if !is_closure_fn_name(&name) {
            return inner;
        }
        let l = lower(&name);
        let op = match self.user_functions.get(&l) {
            Some(op) if op_has_statics(op) => (**op).clone(),
            _ => return inner,
        };
        let id = CLOSURE_STATICS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base: Vec<u8> = match name.iter().position(|&b| b == b'#') {
            Some(p) => name[..p].to_vec(),
            None => name.clone(),
        };
        let new_name = format!("{}#{}", String::from_utf8_lossy(&base), id).into_bytes();
        // Keys are compiled from the original function name.
        let key_name = op
            .ops
            .iter()
            .find_map(|o| match (o.opcode, &o.result) {
                (OpCode::StaticVarInit, OperandType::Const(i)) => {
                    match op.literals.get(*i as usize) {
                        Some(Value::String(s)) => s
                            .as_bytes()
                            .windows(2)
                            .rposition(|w| w == b"::")
                            .map(|p| s.as_bytes()[..p].to_vec()),
                        _ => None,
                    }
                }
                _ => None,
            })
            .unwrap_or_else(|| op.name.clone());
        let mut nop = op;
        retarget_static_keys(&mut nop, &key_name, &new_name);
        nop.name = new_name.clone();
        if copy_from.is_some() {
            let mut old_prefix = key_name.clone();
            old_prefix.extend_from_slice(b"::");
            let entries: Vec<(Vec<u8>, Value)> = self
                .static_vars()
                .iter()
                .filter(|(k, _)| k.starts_with(&old_prefix))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (k, v) in entries {
                let mut nk = new_name.clone();
                nk.extend_from_slice(b"::");
                nk.extend_from_slice(&k[old_prefix.len()..]);
                let v = match v.deref() {
                    Value::Array(a) => Value::Array(Rc::new(RefCell::new(a.borrow().clone()))),
                    other => other,
                };
                self.set_static_var(nk, v);
            }
        }
        self.user_functions.insert(lower(&new_name), Rc::new(nop));
        if caps.is_empty() {
            return str_val(&new_name);
        }
        let mut arr = PhpArray::new();
        arr.push(str_val(&new_name));
        for c in caps {
            arr.push(c);
        }
        Value::Array(Rc::new(RefCell::new(arr)))
    }
}

impl Vm {
    /// DECLARE_FUNCTION of a closure's function (`op`, not hoisted) whose
    /// name is registered as that very code already (by the closure's
    /// previous creations): the general handler would only register it
    /// again (see run_fast_ops()).
    #[inline(never)]
    pub(crate) fn fast_declare_closure(&self, op_array: &OpArray, op: &crate::opcode::Op) -> bool {
        let (OperandType::Const(n), OperandType::Const(i), OperandType::Unused) =
            (op.op1, op.op2, op.result)
        else {
            return false;
        };
        let (Some(Value::String(name)), Some(Value::Long(i))) = (
            op_array.literals.get(n as usize),
            op_array.literals.get(i as usize),
        ) else {
            return false;
        };
        let name = name.as_bytes();
        let Some(f) = usize::try_from(*i)
            .ok()
            .and_then(|i| op_array.child_functions.get(i))
        else {
            return false;
        };
        is_closure_fn_name(name)
            && !name.iter().any(u8::is_ascii_uppercase)
            && f.attributes.is_empty()
            && self
                .user_functions
                .get(name)
                .is_some_and(|g| Rc::ptr_eq(f, g))
    }

    /// MAKE_CLOSURE (`op` of `op_array`, running with variables `cvs`) of a
    /// closure the general handler creates as it is: no other scope or
    /// called class to give its function (closure_scope_at_creation), no
    /// `$this` to bind (closure_inherit_this), no static variables of its
    /// own (closure_with_own_statics). `take`: its temporary operand (the
    /// captured values) is released right after it (see
    /// OpArray::free_mask()): the closure gets it. False: nothing was done.
    #[inline(never)]
    pub(crate) fn fast_make_closure(
        &mut self,
        op_array: &OpArray,
        op: &crate::opcode::Op,
        cvs: &[Value],
        tmps: &mut [Value],
        take: bool,
    ) -> bool {
        if !matches!(op.result, OperandType::Tmp(_))
            || !crate::fastpath::tmp_writable(&op.result, tmps)
        {
            return false;
        }
        // (an included file's code: the includer's scope and `$this`)
        if self
            .include_frame_of(op_array)
            .is_some_and(|(s, t)| s.is_some() || t.is_some())
        {
            return false;
        }
        let inner = match op.op1 {
            OperandType::Const(c) => match op_array.literals.get(c as usize) {
                Some(v @ Value::String(_)) => v,
                _ => return false,
            },
            OperandType::Tmp(t) if op.result != op.op1 => match tmps.get(t as usize) {
                Some(v @ Value::Array(_)) => v,
                _ => return false,
            },
            _ => return false,
        };
        {
            let arr;
            let (name, first_cap): (&[u8], Option<&Value>) = match inner {
                Value::String(s) => (s.as_bytes(), None),
                Value::Array(a) => {
                    arr = match a.try_borrow() {
                        Ok(a) => a,
                        Err(_) => return false,
                    };
                    let mut it = arr.values();
                    match it.next() {
                        Some(Value::String(s)) => (s.as_bytes(), it.next()),
                        _ => return false,
                    }
                }
                _ => return false,
            };
            if !is_closure_fn_name(name) || name.iter().any(u8::is_ascii_uppercase) {
                return false;
            }
            let Some(f) = self.user_functions.get(name) else {
                return false;
            };
            let frame_scope = op_array.scope_class.as_ref();
            let captures_this =
                f.closure_capture_names.first().map(|n| n.as_slice()) == Some(b"this");
            // (closure_scope_at_creation(): the scope it was compiled in, no
            // other called class)
            if f.scope_class.as_ref() != frame_scope || f.closure_called_class.is_some() {
                return false;
            }
            let has_this =
                captures_this && matches!(first_cap.map(|v| v.deref()), Some(Value::Object(_)));
            if let (false, Some(scope)) = (has_this, frame_scope) {
                if self.called_class_stack.last().is_some_and(|c| {
                    c.len() != scope.len()
                        || c.iter()
                            .zip(scope)
                            .any(|(a, b)| a.to_ascii_lowercase() != *b)
                }) {
                    return false;
                }
            }
            // (closure_inherit_this(): no `$this` to bind)
            if op_array.cv_names.first().map(|n| n.as_slice()) == Some(b"this")
                && matches!(cvs.first().map(|v| v.deref()), Some(Value::Object(_)))
                && !f.is_static_closure
                && !captures_this
            {
                return false;
            }
            // (closure_with_own_statics())
            if op_has_statics(f) {
                return false;
            }
        }
        let inner = match (take, op.op1) {
            (true, OperandType::Tmp(t)) => std::mem::replace(&mut tmps[t as usize], Value::Undef),
            _ => inner.clone(),
        };
        let obj = self.make_closure_object(inner);
        crate::fastpath::store_tmp(&op.result, obj, tmps)
    }
}

/// A class looked up by name for a dynamic call: "self", "parent" and
/// "static" are not found.
fn by_name_class_check(cls: &[u8]) -> Result<(), (&'static [u8], String)> {
    if matches!(lower(cls).as_slice(), b"self" | b"parent" | b"static") {
        return Err((
            b"Error",
            format!("Class \"{}\" not found", crate::string::msg_str(cls)),
        ));
    }
    Ok(())
}
