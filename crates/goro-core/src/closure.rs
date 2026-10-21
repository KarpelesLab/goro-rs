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

/// Whether `name` is an internal closure function name.
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
        self.make_closure_object(inner)
    }

    /// The user OpArray backing a closure object (None for internal functions).
    pub fn closure_op_array(&self, closure: &Value) -> Option<&OpArray> {
        let inner = closure.closure_inner()?;
        let (name, _) = closure_parts(&inner);
        let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
        self.user_functions.get(&lower)
    }

    /// Canonical "Class::method" / "function" display for a fake closure target.
    fn closure_target_display(&self, name: &[u8]) -> Vec<u8> {
        let name = name.strip_prefix(b"\\").unwrap_or(name);
        if let Some(p) = name.windows(2).position(|w| w == b"::") {
            let cls = &name[..p];
            let m = &name[p + 2..];
            let cl: Vec<u8> = cls.iter().map(|b| b.to_ascii_lowercase()).collect();
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
                let by_ref = op.by_ref_params.get(i).copied().unwrap_or(false);
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
                    _ => Value::Null,
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
                if n.is_empty() {
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
            if statics.len() > 0 {
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
            let target_op: Option<OpArray> = self
                .user_functions
                .get(&tl)
                .cloned()
                .or_else(|| self.closure_method_op(&tl));
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
            if let Some(top) = &target_op {
                params = Self::closure_param_entries(top);
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

    /// The scope class (canonical name) of a closure, if any.
    pub fn closure_scope(&self, closure: &Value) -> Option<Vec<u8>> {
        let inner = closure.closure_inner()?;
        let (name, _) = closure_parts(&inner);
        let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
        let scope_lower: Vec<u8> = if let Some(p) = lower.windows(2).position(|w| w == b"::") {
            let cl = lower[..p].to_vec();
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
                .unwrap_or(scope_lower),
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
                    Some(n) if n.as_slice() != b"this" => {
                        out.set(ArrayKey::String(PhpString::from_vec(n.clone())), v.deref());
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
        ce.methods.get(&lower[p + 2..]).map(|m| m.op_array.clone())
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
    pub fn make_fcc(
        &mut self,
        wrapper: Value,
        desc: &[u8],
        this: Option<Value>,
        scope: Option<Vec<u8>>,
    ) -> FccResult {
        let (_, caps) = closure_parts(&wrapper);
        if let Some(name) = desc.strip_prefix(b"f:") {
            return self.fcc_function(name, true);
        }
        if desc == b"d:" {
            let callable = caps.first().map(|v| v.deref()).unwrap_or(Value::Null);
            return self.fcc_dynamic(callable, wrapper, scope);
        }
        if let Some(m) = desc.strip_prefix(b"m:") {
            let obj = caps.first().map(|v| v.deref()).unwrap_or(Value::Null);
            return self.fcc_method(obj, m, wrapper, scope);
        }
        if let Some(rest) = desc.strip_prefix(b"s:") {
            if let Some(p) = rest.windows(2).rposition(|w| w == b"::") {
                let (cls, m) = (rest[..p].to_vec(), rest[p + 2..].to_vec());
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
                if let Some(p) = b.windows(2).position(|w| w == b"::") {
                    return self.fcc_static(
                        &b[..p].to_vec(),
                        &b[p + 2..].to_vec(),
                        None,
                        wrapper,
                        scope,
                    );
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
                        self.fcc_method(o, m.as_bytes(), wrapper, scope)
                    }
                    (Some(Value::String(c)), Some(Value::String(m))) => self.fcc_static(
                        &c.as_bytes().to_vec(),
                        &m.as_bytes().to_vec(),
                        None,
                        wrapper,
                        scope,
                    ),
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
                    self.fcc_method(callable.clone(), b"__invoke", wrapper, scope)
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
                    Vm::value_type_name(other)
                ),
            )),
        }
    }

    fn fcc_method(
        &mut self,
        obj: Value,
        m: &[u8],
        wrapper: Value,
        scope: Option<Vec<u8>>,
    ) -> FccResult {
        let o = match &obj {
            Value::Object(o) => o.clone(),
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
        let ml = lower(m);
        let ce = match self.classes.get(&cl) {
            Some(c) => c,
            // stdClass has no methods.
            None if cl.as_slice() == b"stdclass" => {
                return Err((
                    b"Error",
                    format!(
                        "Call to undefined method stdClass::{}()",
                        String::from_utf8_lossy(m)
                    ),
                ));
            }
            // Builtin classes: keep the forwarding closure.
            None => return Ok(self.make_closure_object(wrapper)),
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
                        return Ok(self.make_closure_object(wrapper));
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
            None if has_call => Ok(self.make_closure_object(wrapper)),
            None => Err((
                b"Error",
                format!(
                    "Call to undefined method {}::{}()",
                    crate::value::display_class_name(&class_name),
                    String::from_utf8_lossy(m)
                ),
            )),
        }
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
                            String::from_utf8_lossy(m)
                        ),
                    ));
                }
                if self.is_known_builtin_class(&cl) {
                    return Ok(self.make_closure_object(wrapper));
                }
                return Err((
                    b"Error",
                    format!("Class \"{}\" not found", String::from_utf8_lossy(&cls)),
                ));
            }
        };
        let canon = ce.name.clone();
        let ml = lower(m);
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
                    if has_call_static || (has_call && bound_this.is_some()) {
                        return Ok(self.make_closure_object(wrapper));
                    }
                    return Err((b"Error", msg));
                }
                let mut n = canon.clone();
                n.extend_from_slice(b"::");
                n.extend_from_slice(&mname);
                if is_static {
                    return Ok(self.make_closure_object(str_val(&n)));
                }
                match bound_this {
                    Some(t) => {
                        let key = lower(&n);
                        if !self.user_functions.contains_key(&key) {
                            let mut op = op;
                            if op.attributes.is_empty() && !attrs.is_empty() {
                                op.attributes = attrs;
                            }
                            self.user_functions.insert(key, op);
                        }
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
            None if has_call_static || (has_call && bound_this.is_some()) => {
                Ok(self.make_closure_object(wrapper))
            }
            None => Err((
                b"Error",
                format!(
                    "Call to undefined method {}::{}()",
                    String::from_utf8_lossy(&canon),
                    String::from_utf8_lossy(m)
                ),
            )),
        }
    }
}

impl Vm {
    /// A closure forwarding to `__call`/`__callStatic` for `method`
    /// (the closure PHP builds over a call trampoline).
    fn magic_call_closure(
        &mut self,
        class_canon: &[u8],
        method: &[u8],
        obj: Option<Value>,
    ) -> Value {
        use crate::opcode::Op;
        let mut target = class_canon.to_vec();
        target.extend_from_slice(b"::");
        target.extend_from_slice(method);
        // One trampoline per (target, instance/static): equal closures compare equal.
        let name = format!(
            "__closure_fcc_tramp_{}_{}",
            if obj.is_some() { "i" } else { "s" },
            String::from_utf8_lossy(&lower(&target))
        )
        .into_bytes();
        let mut op = OpArray::new();
        op.name = name.clone();
        op.fcc_target_name = Some(target.clone());
        op.scope_class = Some(lower(class_canon));
        let has_obj = obj.is_some();
        if has_obj {
            op.get_or_create_cv(b"__fcc_captured");
        }
        let args_cv = op.get_or_create_cv(b"args");
        op.param_count = 1;
        op.required_param_count = 0;
        op.variadic_param = Some(args_cv);
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
        self.user_functions.insert(name.clone(), op);
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
        let line = self.current_line;
        self.emit_deprecated_at(
            &format!("Use of \"{}\" in callables is deprecated", kw),
            line,
        );
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
        match &callable {
            Value::String(s) => {
                let b = s.as_bytes().to_vec();
                if let Some(p) = b.windows(2).position(|w| w == b"::") {
                    let cls = self.callable_class_relative(&b[..p], scope.as_deref())?;
                    return self.from_callable_method(&cls, &b[p + 2..], None, this, scope);
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
                    String::from_utf8_lossy(&b)
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
                        self.from_callable_method(&cls, &meth, Some(Value::Object(o)), None, scope)
                    }
                    Some(Value::String(c)) => {
                        let cls = self.callable_class_relative(c.as_bytes(), scope.as_deref())?;
                        self.from_callable_method(&cls, &m, None, this, scope)
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
                    self.from_callable_method(&cn, b"__invoke", Some(callable.clone()), None, scope)
                } else {
                    Err("no array or string given".to_string())
                }
            }
            _ => Err("no array or string given".to_string()),
        }
    }

    fn from_callable_method(
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
                    // Builtin class methods: keep the [obj|class, method] callable.
                    let mut arr = PhpArray::new();
                    arr.push(obj.unwrap_or_else(|| str_val(cls)));
                    arr.push(str_val(m));
                    return Ok(self.make_closure_object(Value::Array(Rc::new(RefCell::new(arr)))));
                }
                return Err(format!(
                    "class \"{}\" not found",
                    String::from_utf8_lossy(cls)
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
                        return Ok(self.magic_call_closure(&canon, m, obj));
                    }
                    if obj.is_none() && has_call_static {
                        return Ok(self.magic_call_closure(&canon, m, None));
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
                        String::from_utf8_lossy(&canon),
                        String::from_utf8_lossy(&mname)
                    ));
                }
                if is_abstract {
                    return Err(format!(
                        "cannot call abstract method {}::{}()",
                        String::from_utf8_lossy(&canon),
                        String::from_utf8_lossy(&mname)
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
                            String::from_utf8_lossy(&canon),
                            String::from_utf8_lossy(&mname)
                        ));
                    }
                };
                let key = lower(&n);
                let mut op = op;
                if op.attributes.is_empty() && !attrs.is_empty() {
                    op.attributes = attrs;
                }
                self.user_functions.insert(key, op);
                let mut arr = PhpArray::new();
                arr.push(str_val(&n));
                arr.push(o);
                Ok(self.make_closure_object(Value::Array(Rc::new(RefCell::new(arr)))))
            }
            None => {
                if obj.is_some() && has_call {
                    return Ok(self.magic_call_closure(&canon, m, obj));
                }
                if has_call_static {
                    return Ok(self.magic_call_closure(&canon, m, None));
                }
                Err(format!(
                    "class {} does not have a method \"{}\"",
                    String::from_utf8_lossy(&canon),
                    String::from_utf8_lossy(m)
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
        if is_closure_fn_name(&name) || name.starts_with(b"__closure_fcc_") {
            return None;
        }
        let lname = lower(&name);
        // Function scope / static-ness of the target.
        let (func_scope, is_static, mname): (Option<Vec<u8>>, bool, Vec<u8>) =
            match lname.windows(2).position(|w| w == b"::") {
                Some(p) => {
                    let cl = lname[..p].to_vec();
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
                        None => (Some(cl), caps.is_empty(), name[p + 2..].to_vec()),
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
                            String::from_utf8_lossy(&n)
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
                        String::from_utf8_lossy(&mname),
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
        let new_inner = match new_this {
            Value::Object(_) if func_scope.is_some() => {
                let mut arr = PhpArray::new();
                arr.push(str_val(&name));
                arr.push(new_this.clone());
                Value::Array(Rc::new(RefCell::new(arr)))
            }
            _ => str_val(&name),
        };
        Some(self.make_closure_object(new_inner))
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
