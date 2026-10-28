//! Array, string-offset and ArrayAccess reads and writes, and the
//! generic write-path walker (`$a[k][j]->p[] = v`).
//!
//! Behaviour from the PHP manual (Arrays, Strings: "String access and
//! modification by character", ArrayAccess) and observed on PHP 8.5.

use crate::class::{DisplayName, TypeName};
use std::rc::Rc;

use goro_runtime::{Key, ObjRef, PhpArray, PhpRef, PhpStr, Value};

use super::{R, Vm};
use crate::bytecode::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DimMode {
    Read,
    /// isset/`??` chains: no warnings.
    Is,
    /// `list()` destructuring.
    List,
}

/// Where a write path currently points.
pub(crate) enum Place {
    /// A value slot we may write (variable, element, property storage).
    Ptr(*mut Value),
    /// A temporary value (result of `offsetGet`/`__get`, function call):
    /// writes to it are lost, as in PHP.
    Temp(Box<Value>),
}

impl Place {
    fn ptr(&mut self) -> *mut Value {
        match self {
            Place::Ptr(p) => *p,
            Place::Temp(b) => &mut **b as *mut Value,
        }
    }
}

/// Operands of a write path being walked.
struct WalkCtx {
    keys: Vec<Option<Value>>,
    /// Keys in variables, read when the walk reaches them.
    lazy: Vec<Reg>,
    names: Vec<Option<PhpStr>>,
    value: Option<Value>,
    value_lazy: Reg,
    action: PathAction,
}

pub(crate) fn key_desc(k: &Key) -> String {
    match k {
        Key::Int(i) => i.to_string(),
        Key::Str(s) => format!("\"{}\"", s.to_string_lossy()),
    }
}

impl Vm {
    // ------------------------------------------------------------ reads

    pub(crate) fn op_fetch_dim(&mut self, dst: Reg, base: Reg, key: Reg, mode: DimMode) -> R<()> {
        // Fast path: array[int|string].
        if let Some(Value::Array(a)) = self.peek(base) {
            let found = match self.peek(key) {
                Some(Value::Int(i)) => Some(a.get_int(*i).cloned()),
                Some(Value::Str(s)) => Some(a.get_str(s).cloned()),
                _ => None,
            };
            if let Some(Some(v)) = found {
                let v = v.into_deref();
                self.drop_tmp(key);
                self.drop_tmp(base);
                self.put(dst, v);
                return Ok(());
            }
        }
        let b = if mode == DimMode::Read {
            self.get(base)?
        } else {
            self.get_quiet(base)
        };
        let k = self.get(key)?;
        let v = self.dim_read(&b, &k, mode)?;
        drop(b);
        self.put(dst, v);
        Ok(())
    }

    /// `container[key]` for reading.
    pub fn dim_read(&mut self, c: &Value, k: &Value, mode: DimMode) -> R<Value> {
        match c {
            Value::Array(a) => {
                let key = self.to_key(k, "array")?;
                match a.get(key.as_ref()) {
                    Some(v) => Ok(v.deref()),
                    None => {
                        if mode != DimMode::Is {
                            self.warn(&format!("Undefined array key {}", key_desc(&key)))?;
                        }
                        Ok(Value::Null)
                    }
                }
            }
            Value::Str(s) => {
                if mode == DimMode::List {
                    self.warn("Cannot use string as array")?;
                    return Ok(Value::Null);
                }
                let off = match self.string_offset(k, mode == DimMode::Is)? {
                    Some(o) => o,
                    None => return Ok(Value::Null),
                };
                let len = s.len() as i64;
                let i = if off < 0 { off + len } else { off };
                if i < 0 || i >= len {
                    if mode == DimMode::Read {
                        self.warn(&format!("Uninitialized string offset {}", off))?;
                        return Ok(Value::Str(PhpStr::empty()));
                    }
                    return Ok(Value::Null);
                }
                Ok(Value::Str(PhpStr::from_byte(s[i as usize])))
            }
            Value::Object(o) => self.array_access_get(o, k, mode),
            Value::Ref(r) => {
                let v = r.get();
                self.dim_read(&v, k, mode)
            }
            Value::Null => {
                if mode == DimMode::Read {
                    self.warn("Trying to access array offset on null")?;
                }
                Ok(Value::Null)
            }
            other => {
                match mode {
                    DimMode::Read => {
                        let t = match other {
                            Value::Bool(true) => "true".to_string(),
                            Value::Bool(false) => "false".to_string(),
                            v => v.tname(),
                        };
                        self.warn(&format!("Trying to access array offset on {}", t))?;
                    }
                    DimMode::List => {
                        self.warn(&format!("Cannot use {} as array", other.tname()))?;
                    }
                    DimMode::Is => {}
                }
                Ok(Value::Null)
            }
        }
    }

    /// The offset for a string read: `None` for non-numeric strings in
    /// isset context.
    fn string_offset(&mut self, k: &Value, quiet: bool) -> R<Option<i64>> {
        Ok(Some(match k {
            Value::Int(i) => *i,
            Value::Str(s) => match goro_runtime::array::canonical_int(s) {
                Some(i) => i,
                None => {
                    if quiet {
                        // "1.0" and friends are not integer offsets.
                        return Ok(None);
                    }
                    let p = goro_runtime::numeric::parse_numeric(s);
                    if p.kind == goro_runtime::numeric::Numericness::Numeric {
                        if let goro_runtime::Number::Int(i) = p.num {
                            i
                        } else {
                            self.warn("String offset cast occurred")?;
                            p.num.to_f64() as i64
                        }
                    } else if p.kind == goro_runtime::numeric::Numericness::Leading {
                        self.warn(&format!(
                            "Illegal string offset \"{}\"",
                            s.to_string_lossy()
                        ))?;
                        p.num.to_f64() as i64
                    } else {
                        return self.type_error("Cannot access offset of type string on string");
                    }
                }
            },
            Value::Float(f) => {
                if quiet {
                    if f.fract() != 0.0 {
                        self.deprecated(&format!(
                            "Implicit conversion from float {} to int loses precision",
                            goro_runtime::convert::float_repr(*f)
                        ))?;
                    }
                    return Ok(Some(*f as i64));
                }
                self.warn("String offset cast occurred")?;
                *f as i64
            }
            Value::Bool(b) => {
                if !quiet {
                    self.warn("String offset cast occurred")?;
                }
                *b as i64
            }
            Value::Null => {
                if !quiet {
                    self.warn("String offset cast occurred")?;
                }
                0
            }
            Value::Ref(r) => {
                let v = r.get();
                return self.string_offset(&v, quiet);
            }
            other => {
                if quiet {
                    return Ok(None);
                }
                return self.type_error(&format!(
                    "Cannot access offset of type {} on string",
                    other.tname()
                ));
            }
        }))
    }

    fn array_access_get(&mut self, o: &ObjRef, k: &Value, mode: DimMode) -> R<Value> {
        let cls = self.class_of(o);
        if !self.implements_array_access(&cls) {
            return self.throw(
                "Error",
                &format!("Cannot use object of type {} as array", cls.name.dname()),
            );
        }
        if mode == DimMode::Is {
            let e = self.call_method(o, b"offsetexists", vec![k.clone()])?;
            if !self.to_bool(&e) {
                return Ok(Value::Null);
            }
        }
        let v = self.call_method(o, b"offsetget", vec![k.clone()])?;
        Ok(v.into_deref())
    }

    pub(crate) fn implements_array_access(&self, cls: &crate::class::Class) -> bool {
        match &self.core.array_access {
            Some(a) => cls.is_subclass_of(a),
            None => false,
        }
    }

    pub(crate) fn op_isset_dim(&mut self, dst: Reg, base: Reg, key: Reg, empty: bool) -> R<()> {
        let b = self.get_quiet(base);
        let k = self.get(key)?;
        let r = self.dim_isset(&b, &k, empty)?;
        self.put(dst, Value::Bool(r));
        Ok(())
    }

    /// `isset(c[k])`, or `empty(c[k])` when `empty`.
    pub fn dim_isset(&mut self, c: &Value, k: &Value, empty: bool) -> R<bool> {
        match c {
            Value::Array(a) => {
                let key = match k {
                    Value::Int(i) => Key::Int(*i),
                    Value::Str(s) => Key::from_php_str(s),
                    Value::Array(_) | Value::Object(_) => {
                        return self.type_error(&format!(
                            "Cannot access offset of type {} in isset or empty",
                            k.tname()
                        ));
                    }
                    _ => self.to_key(k, "array")?,
                };
                let v = a.get(key.as_ref()).map(|v| v.deref());
                Ok(match v {
                    None => empty,
                    Some(v) => {
                        if empty {
                            !self.to_bool(&v)
                        } else {
                            !v.is_null()
                        }
                    }
                })
            }
            Value::Str(s) => {
                let off = match k {
                    Value::Int(i) => Some(*i),
                    Value::Str(ks) => goro_runtime::array::canonical_int(ks),
                    Value::Float(f) => Some(*f as i64),
                    Value::Bool(b) => Some(*b as i64),
                    Value::Null => Some(0),
                    _ => None,
                };
                let Some(off) = off else { return Ok(empty) };
                let len = s.len() as i64;
                let i = if off < 0 { off + len } else { off };
                if i < 0 || i >= len {
                    return Ok(empty);
                }
                if empty {
                    Ok(s[i as usize] == b'0')
                } else {
                    Ok(true)
                }
            }
            Value::Object(o) => {
                let cls = self.class_of(o);
                if !self.implements_array_access(&cls) {
                    return self.throw(
                        "Error",
                        &format!("Cannot use object of type {} as array", cls.name.dname()),
                    );
                }
                // ArrayObject/ArrayIterator: isset() is false for null
                // elements (php85: `isset(new ArrayObject(['n' => null])['n'])`).
                if cls.native.dim_storage.is_some()
                    && cls
                        .find_method(b"offsetexists")
                        .is_some_and(|f| f.is_native())
                {
                    let v = crate::builtins::spl::ao_isset(self, o, k)?;
                    return Ok(match v {
                        None => empty,
                        Some(v) if empty => !self.to_bool(&v),
                        Some(v) => !v.is_null(),
                    });
                }
                let e = self.call_method(o, b"offsetexists", vec![k.clone()])?;
                let exists = self.to_bool(&e);
                if !empty {
                    return Ok(exists);
                }
                if !exists {
                    return Ok(true);
                }
                let v = self.call_method(o, b"offsetget", vec![k.clone()])?;
                Ok(!self.to_bool(&v))
            }
            Value::Ref(r) => {
                let v = r.get();
                self.dim_isset(&v, k, empty)
            }
            _ => Ok(empty),
        }
    }

    // ------------------------------------------------------------ array literals

    pub(crate) fn op_add_elem(&mut self, arr: Reg, key: Reg, val: Reg, by_ref: bool) -> R<()> {
        let v = if by_ref {
            match self.get_raw(val) {
                Value::Ref(r) => Value::Ref(r),
                other => Value::Ref(PhpRef::new(other)),
            }
        } else {
            self.get(val)?
        };
        let k = if key == NONE {
            None
        } else {
            let kv = self.get(key)?;
            Some(self.to_key(&kv, "array")?)
        };
        let p = self.slot_ptr(arr);
        let Some(Value::Array(a)) = (unsafe { &mut *p }) else {
            unreachable!("AddElem on non-array")
        };
        match k {
            None => {
                if a.push(v).is_err() {
                    return self.throw(
                        "Error",
                        "Cannot add element to the array as the next element is already occupied",
                    );
                }
            }
            Some(k) => {
                let old = a.insert(k, v);
                drop(old);
            }
        }
        Ok(())
    }

    pub(crate) fn op_add_spread(&mut self, arr: Reg, src: Reg) -> R<()> {
        let v = self.get(src)?;
        let items: Vec<(Value, Value)> = match &v {
            Value::Array(a) => a.iter().map(|(k, v)| (k.to_value(), v.clone())).collect(),
            Value::Object(o) => {
                let cls = self.class_of(o);
                if !self.is_traversable(&cls) {
                    return self.throw("Error", "Only arrays and Traversables can be unpacked");
                }
                self.iterate_to_vec(&v, true)?
            }
            _ => return self.throw("Error", "Only arrays and Traversables can be unpacked"),
        };
        let p = self.slot_ptr(arr);
        let Some(Value::Array(a)) = (unsafe { &mut *p }) else {
            unreachable!()
        };
        for (k, x) in items {
            let x = match x {
                Value::Ref(r) if r.refcount() == 1 => r.get(),
                x => x,
            };
            match k {
                Value::Str(s) => {
                    a.insert(Key::from_php_str(&s), x);
                }
                Value::Int(_) => {
                    if a.push(x).is_err() {
                        return self.throw(
                            "Error",
                            "Cannot add element to the array as the next element is already occupied",
                        );
                    }
                }
                _ => {
                    return self.throw(
                        "Error",
                        "Keys must be of type int|string during array unpacking",
                    );
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ writes

    /// `$var[key] = val` (key NONE: append).
    pub(crate) fn op_assign_dim(&mut self, var: Reg, key: Reg, val: Reg, res: Reg) -> R<()> {
        // Fast path: the variable holds an array.
        let p = self.slot_ptr(var);
        if let Some(Value::Array(a)) = unsafe { &mut *p } {
            let k = if key == NONE {
                None
            } else {
                match self.peek(key) {
                    Some(Value::Int(i)) => Some(Key::Int(*i)),
                    Some(Value::Str(s)) => Some(Key::from_php_str(s)),
                    _ => {
                        return self.assign_dim_slow(var, key, val, res);
                    }
                }
            };
            let v = self.get(val)?;
            let rv = if res != NONE { Some(v.clone()) } else { None };
            // `get` may have run an error handler; re-fetch the slot.
            let p = self.slot_ptr(var);
            let Some(Value::Array(a)) = (unsafe { &mut *p }) else {
                return self.assign_dim_slow_value(var, k, rv.unwrap_or(v), res);
            };
            let _ = &a;
            match k {
                None => {
                    if a.push(v).is_err() {
                        return self.throw(
                            "Error",
                            "Cannot add element to the array as the next element is already occupied",
                        );
                    }
                }
                Some(k) => {
                    let old = a.assign(k, v);
                    drop(old);
                }
            }
            self.drop_tmp(key);
            if let Some(rv) = rv {
                self.put(res, rv);
            }
            return Ok(());
        }
        self.assign_dim_slow(var, key, val, res)
    }

    fn assign_dim_slow(&mut self, var: Reg, key: Reg, val: Reg, res: Reg) -> R<()> {
        let elem = if key == NONE {
            PathElem::Append
        } else {
            PathElem::Dim(key)
        };
        let path = WritePath {
            base: PathBase::Var(var),
            elems: Box::new([elem]),
            action: PathAction::Assign(val),
        };
        self.run_path(&path, res)
    }

    fn assign_dim_slow_value(&mut self, var: Reg, k: Option<Key>, v: Value, res: Reg) -> R<()> {
        let kv = match k {
            Some(k) => k.to_value(),
            None => Value::Null,
        };
        let mut place = Place::Ptr(self.var_place(var));
        let elem = if kv.is_null() { None } else { Some(kv) };
        let r = self.write_elem(&mut place, elem, PathAction::Assign(NONE), Some(v))?;
        self.put(res, r);
        Ok(())
    }

    pub(crate) fn op_write(&mut self, path: u32, res: Reg) -> R<()> {
        let code = self.code_ref();
        let p = code.paths[path as usize].clone();
        self.run_path(&p, res)
    }

    /// Pointer to a variable's value for writing (creating it as null).
    fn var_place(&mut self, var: Reg) -> *mut Value {
        let p = self.slot_ptr(var);
        unsafe {
            if (*p).is_none() {
                if self.frame().flags & super::frame_flags::ATTACHED != 0 {
                    self.attached_bind_or_create(var);
                }
                if (*p).is_none() {
                    *p = Some(Value::Null);
                }
            }
            (*p).as_mut().unwrap() as *mut Value
        }
    }

    /// `$o->p op= v` / `$o->p++` on a cached plain property holding a
    /// number: done in place. False when the general path is needed.
    fn prop_rmw_fast(&mut self, path: &WritePath, n: Reg, cache: u32, res: Reg) -> R<bool> {
        let o = match path.base {
            PathBase::This => match self.this_obj() {
                Some(o) => o,
                None => return Ok(false),
            },
            PathBase::Var(r) => match self.peek(r) {
                Some(Value::Object(o)) => o.clone(),
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        if o.is_lazy() {
            return Ok(false);
        }
        let mut c = self.code_ref().caches[cache as usize].get();
        let cp = self.class_ptr_of(&o) as usize;
        if c.key != cp {
            let name = match self.peek(n) {
                Some(Value::Str(s)) => s.clone(),
                _ => return Ok(false),
            };
            self.prop_ic_fill(&o, &name, cache);
            c = self.code_ref().caches[cache as usize].get();
            if c.key != cp {
                return Ok(false);
            }
        }
        if c.b & super::objects::IC_PLAIN == 0 {
            return Ok(false);
        }
        let rhs = match path.action {
            PathAction::AssignOp(_, r) => match self.peek(r) {
                Some(v @ (Value::Int(_) | Value::Float(_))) => Some(v.clone()),
                _ => return Ok(false),
            },
            _ => None,
        };
        let new = {
            let slots = o.obj().slots.borrow();
            let cur = &slots[c.a];
            match (&path.action, cur) {
                (PathAction::AssignOp(op, _), Value::Int(_) | Value::Float(_)) => {
                    match super::exec::fast_arith(*op, cur, rhs.as_ref().unwrap()) {
                        Some(v) => (v, None),
                        None => return Ok(false),
                    }
                }
                (PathAction::IncDec(op), Value::Int(i)) => {
                    let r = if op.is_inc() {
                        i.checked_add(1)
                    } else {
                        i.checked_sub(1)
                    };
                    match r {
                        Some(x) => (Value::Int(x), Some(*i)),
                        None => return Ok(false),
                    }
                }
                _ => return Ok(false),
            }
        };
        let (newv, old_int) = new;
        if c.b & super::objects::IC_TYPED != 0 && !super::objects::exact_type(&newv, c.b >> 8) {
            return Ok(false);
        }
        if let PathAction::AssignOp(_, r) = path.action {
            self.drop_tmp(r);
        }
        o.obj().slots.borrow_mut()[c.a] = newv.clone();
        if res != NONE {
            let out = match (&path.action, old_int) {
                (PathAction::IncDec(op), Some(old)) if op.is_post() => Value::Int(old),
                _ => newv,
            };
            self.put(res, out);
        }
        Ok(true)
    }

    fn run_path(&mut self, path: &WritePath, res: Reg) -> R<()> {
        if path.elems.len() == 1
            && let PathElem::Prop(n, cache) = path.elems[0]
            && n & CONST != 0
            && matches!(
                path.action,
                PathAction::AssignOp(..) | PathAction::IncDec(_)
            )
            && self.prop_rmw_fast(path, n, cache, res)?
        {
            return Ok(());
        }
        // Read-modify-write of an undefined variable warns first.
        if let PathBase::Var(r) = path.base
            && matches!(
                path.action,
                PathAction::AssignOp(..) | PathAction::IncDec(_)
            )
            && !path.elems.is_empty()
            && self.peek(r).is_none()
        {
            self.undefined_var(r)?;
        }
        // Operands: temporaries now (they were computed in order); variables
        // are read when the walk reaches them, as PHP does.
        let mut keys: Vec<Option<Value>> = Vec::with_capacity(path.elems.len());
        let mut lazy: Vec<Reg> = Vec::with_capacity(path.elems.len());
        let mut names: Vec<Option<PhpStr>> = Vec::with_capacity(path.elems.len());
        let mut base_tmp: Option<Value> = None;
        let mut base_name: Option<PhpStr> = None;
        let mut static_cell: Option<PhpRef> = None;
        let mut static_decl: Option<Rc<crate::class::StaticProp>> = None;
        let mut global_name: Option<PhpStr> = None;
        match path.base {
            PathBase::Tmp(r) => base_tmp = Some(self.get_raw(r)),
            PathBase::VarVar(r) => {
                let n = self.get(r)?;
                base_name = Some(self.to_php_str(&n)?);
            }
            PathBase::Global(r) => {
                let n = self.get(r)?;
                global_name = Some(self.to_php_str(&n)?);
            }
            PathBase::StaticProp(c, n) => {
                let unset = matches!(path.action, PathAction::Unset) && path.elems.is_empty();
                if unset {
                    return self.throw("Error", "Attempt to unset static property");
                }
                let (r, sp) = self.static_prop_ref(c, n)?;
                static_cell = Some(r);
                static_decl = Some(sp);
            }
            _ => {}
        }
        for e in path.elems.iter() {
            match e {
                PathElem::Dim(k) => {
                    if *k & (CONST | TMP) == 0 {
                        keys.push(None);
                        lazy.push(*k);
                    } else {
                        let v = self.get(*k)?;
                        keys.push(Some(v));
                        lazy.push(NONE);
                    }
                    names.push(None);
                }
                PathElem::Append => {
                    keys.push(None);
                    lazy.push(NONE);
                    names.push(None);
                }
                PathElem::Prop(n, _) => {
                    let v = self.get(*n)?;
                    let s = match v {
                        Value::Str(s) => s,
                        other => self.to_php_str(&other)?,
                    };
                    keys.push(None);
                    lazy.push(NONE);
                    names.push(Some(s));
                }
            }
        }
        // The value.
        let (value, value_lazy) = match path.action {
            PathAction::Assign(v) | PathAction::AssignOp(_, v) => (Some(self.get(v)?), NONE),
            PathAction::BindRef(v) => (Some(self.get_raw(v)), NONE),
            _ => (None, NONE),
        };
        let ctx = WalkCtx {
            keys,
            lazy,
            names,
            value,
            value_lazy,
            action: path.action,
        };
        // Resolve the base place.
        let mut place = match path.base {
            PathBase::Var(r) => {
                if path.elems.is_empty() {
                    let v = ctx.value.clone();
                    return self.var_action(r, path.action, v, res);
                }
                Place::Ptr(self.var_place(r))
            }
            PathBase::This => {
                let Some(t) = self.this_obj() else {
                    return self.throw("Error", "Using $this when not in object context");
                };
                Place::Temp(Box::new(Value::Object(t)))
            }
            PathBase::Tmp(_) => {
                let v = base_tmp.take().unwrap();
                if path.elems.is_empty()
                    && let PathAction::MakeRef = path.action
                {
                    // `&f()`: a reference to a function result.
                    let r = match v {
                        Value::Ref(r) => r,
                        other => {
                            self.notice_msg("Only variables should be assigned by reference")?;
                            PhpRef::new(other)
                        }
                    };
                    let p = self.slot_ptr(res);
                    let old = unsafe { (*p).replace(Value::Ref(r)) };
                    drop(old);
                    return Ok(());
                }
                match v {
                    Value::Ref(r) => Place::Ptr(r.borrow_mut_ptr()),
                    v => Place::Temp(Box::new(v)),
                }
            }
            PathBase::VarVar(_) => {
                let n = base_name.take().unwrap();
                if path.elems.is_empty() {
                    let v = ctx.value.clone();
                    return self.var_var_action(&n, path.action, v, res);
                }
                Place::Ptr(self.var_var_place(&n)?)
            }
            PathBase::Global(_) => {
                let n = global_name.take().unwrap();
                if path.elems.is_empty() {
                    let v = ctx.value.clone();
                    return self.global_action(&n, path.action, v, res);
                }
                Place::Ptr(self.global_place(&n))
            }
            PathBase::StaticProp(..) => {
                let r = static_cell.clone().unwrap();
                if path.elems.is_empty() {
                    let v = ctx.value.clone();
                    // `A::$x =& $y` rebinds the property's storage.
                    if let (PathAction::BindRef(_), Some(Value::Ref(src)), Some(sp)) =
                        (path.action, &v, &static_decl)
                    {
                        let src = src.clone();
                        if sp.ty.is_some() {
                            let cur = src.get();
                            let cur = self.typed_static_ref_value(sp, cur)?;
                            let old = src.set(cur);
                            drop(old);
                        }
                        *sp.cell.borrow_mut() = src.clone();
                        // Re-register a typed property's reference.
                        let _ = self.find_static_prop_entry(&self.rc_class(sp.class), &sp.name)?;
                        if res != NONE {
                            let p = self.slot_ptr(res);
                            let old = unsafe { (*p).replace(Value::Ref(src)) };
                            drop(old);
                        }
                        return Ok(());
                    }
                    return self.cell_action(&r, path.action, v, res);
                }
                Place::Ptr(r.borrow_mut_ptr())
            }
        };
        let mut ctx = ctx;
        self.err.defer_depth += 1;
        let r = self.walk(&mut place, &path.elems, &mut ctx, 0);
        self.err.defer_depth -= 1;
        if self.err.defer_depth == 0 && !self.err.queued.is_empty() {
            self.flush_deferred()?;
        }
        let r = r?;
        if res != NONE {
            let p = self.slot_ptr(res);
            let old = unsafe { (*p).replace(r) };
            drop(old);
        }
        Ok(())
    }

    fn walk_key(&mut self, ctx: &WalkCtx, i: usize) -> R<Option<Value>> {
        if ctx.lazy[i] != NONE {
            return Ok(Some(self.get(ctx.lazy[i])?));
        }
        Ok(ctx.keys[i].clone())
    }

    fn walk_value(&mut self, ctx: &mut WalkCtx) -> R<Option<Value>> {
        if ctx.value_lazy != NONE {
            let v = self.get(ctx.value_lazy)?;
            ctx.value_lazy = NONE;
            ctx.value = Some(v);
        }
        Ok(ctx.value.clone())
    }

    /// Walks the remaining path from `place`.
    fn walk(
        &mut self,
        place: &mut Place,
        elems: &[PathElem],
        ctx: &mut WalkCtx,
        i: usize,
    ) -> R<Value> {
        let last = i + 1 == elems.len();
        let action = ctx.action;
        match elems[i] {
            PathElem::Dim(_) | PathElem::Append => {
                let key = self.walk_key(ctx, i)?;
                if last {
                    let value = self.walk_value(ctx)?;
                    return self.write_elem(place, key, action, value);
                }
                // Intermediate: fetch the element for writing.
                let rmw = matches!(action, PathAction::AssignOp(..) | PathAction::IncDec(_));
                let mut next =
                    match self.dim_fetch_w(place, key, matches!(action, PathAction::Unset), rmw)? {
                        Some(p) => p,
                        None => return Ok(Value::Null),
                    };
                self.walk(&mut next, elems, ctx, i + 1)
            }
            PathElem::Prop(_, cache) => {
                let name = ctx.names[i].clone().unwrap();
                let cur = deref_ptr(place.ptr());
                let obj = match unsafe { &*cur } {
                    Value::Object(o) => o.clone(),
                    other => {
                        if last && matches!(action, PathAction::Unset) {
                            return Ok(Value::Null);
                        }
                        let t = match other {
                            Value::Null => "null".to_string(),
                            Value::Bool(true) => "true".to_string(),
                            Value::Bool(false) => "false".to_string(),
                            v => v.tname(),
                        };
                        let what = match (last, action) {
                            (true, PathAction::Assign(_)) | (true, PathAction::AssignOp(..)) => {
                                "assign"
                            }
                            (true, PathAction::IncDec(_)) => "increment/decrement",
                            _ => "modify",
                        };
                        if last {
                            let _ = self.walk_value(ctx)?;
                        }
                        return self.throw(
                            "Error",
                            &format!(
                                "Attempt to {} property \"{}\" on {}",
                                what,
                                name.to_string_lossy(),
                                t
                            ),
                        );
                    }
                };
                if last {
                    let value = self.walk_value(ctx)?;
                    // No array pointers are held here: handlers may run.
                    if !self.err.queued.is_empty() {
                        self.flush_deferred()?;
                    }
                    let saved = std::mem::replace(&mut self.err.defer_depth, 0);
                    let r = self.prop_action(&obj, &name, cache, action, value);
                    self.err.defer_depth = saved;
                    return r;
                }
                let mut next = match self.prop_fetch_w(&obj, &name, cache, elems.get(i + 1))? {
                    Some(p) => p,
                    None => return Ok(Value::Null),
                };
                self.walk(&mut next, elems, ctx, i + 1)
            }
        }
    }

    /// Prepares `place` as an array container for writing: auto-vivifies
    /// null and false. `None` when the write must stop (unset of something
    /// missing, or a user error handler replaced the container).
    fn array_container(&mut self, cur: *mut Value, for_unset: bool) -> R<Option<()>> {
        match unsafe { &*cur } {
            Value::Array(_) => Ok(Some(())),
            Value::Null => {
                if for_unset {
                    return Ok(None);
                }
                unsafe { *cur = Value::Array(PhpArray::new()) };
                Ok(Some(()))
            }
            Value::Bool(false) => {
                if for_unset {
                    return Ok(None);
                }
                self.deprecated("Automatic conversion of false to array is deprecated")?;
                // An error handler may have changed the variable.
                if !matches!(unsafe { &*cur }, Value::Bool(false)) {
                    return Ok(None);
                }
                unsafe { *cur = Value::Array(PhpArray::new()) };
                Ok(Some(()))
            }
            _ => Ok(Some(())),
        }
    }

    /// Fetches `place[key]` for a nested write, auto-vivifying arrays.
    /// `None` for unset paths that don't exist.
    fn dim_fetch_w(
        &mut self,
        place: &mut Place,
        key: Option<Value>,
        for_unset: bool,
        rmw: bool,
    ) -> R<Option<Place>> {
        let cur = deref_ptr(place.ptr());
        match unsafe { &*cur } {
            Value::Array(_) | Value::Null | Value::Bool(false) => {}
            Value::Str(s) => {
                if for_unset {
                    return self.throw("Error", "Cannot unset string offsets");
                }
                if !s.is_empty() {
                    return self.throw("Error", "Cannot use string offset as an array");
                }
                unsafe { *cur = Value::Array(PhpArray::new()) };
            }
            Value::Object(o) => {
                let o = o.clone();
                let cls = self.class_of(&o);
                if !self.implements_array_access(&cls) {
                    return self.throw(
                        "Error",
                        &format!("Cannot use object of type {} as array", cls.name.dname()),
                    );
                }
                if let Some(h) = cls.native.dim_storage
                    && cls.find_method(b"offsetget").is_some_and(|f| f.is_native())
                    && let Some(p) = h(self, &o)
                {
                    return self.dim_fetch_w(&mut Place::Ptr(p), key, for_unset, rmw);
                }
                let k = key.unwrap_or(Value::Null);
                let v = self.call_method_raw(&o, b"offsetget", vec![k])?;
                return Ok(Some(match v {
                    Value::Ref(r) => Place::Ptr(r.borrow_mut_ptr()),
                    Value::Object(_) => Place::Temp(Box::new(v)),
                    other => {
                        self.notice_msg(&format!(
                            "Indirect modification of overloaded element of {} has no effect",
                            cls.name.dname()
                        ))?;
                        Place::Temp(Box::new(other))
                    }
                }));
            }
            _ => {
                if for_unset {
                    return self.throw("Error", "Cannot unset offset in a non-array variable");
                }
                return self.throw("Error", "Cannot use a scalar value as an array");
            }
        }
        // The key first: its conversion may warn.
        let k = match key {
            None => None,
            Some(k) => Some(self.to_key(&k, "array")?),
        };
        if self.array_container(cur, for_unset)?.is_none() {
            return Ok(None);
        }
        let Value::Array(a) = (unsafe { &mut *cur }) else {
            return Ok(None);
        };
        let slot: *mut Value = match k {
            None => match a.make_mut().push_entry() {
                Ok(s) => s as *mut Value,
                Err(e) => return Err(self.php_error(e.to_error())),
            },
            Some(k) => {
                if a.get(k.as_ref()).is_none() {
                    if for_unset {
                        return Ok(None);
                    }
                    if rmw {
                        self.warn(&format!("Undefined array key {}", key_desc(&k)))?;
                    }
                }
                let Value::Array(a) = (unsafe { &mut *cur }) else {
                    return Ok(None);
                };
                a.entry(k) as *mut Value
            }
        };
        Ok(Some(Place::Ptr(slot)))
    }

    /// The final step `place[key] <action>`.
    fn write_elem(
        &mut self,
        place: &mut Place,
        key: Option<Value>,
        action: PathAction,
        value: Option<Value>,
    ) -> R<Value> {
        let cur = deref_ptr(place.ptr());
        let is_unset = matches!(action, PathAction::Unset);
        match unsafe { &*cur } {
            Value::Array(_) | Value::Null | Value::Bool(false) => {}
            Value::Str(_) => return self.string_offset_write(cur, key, action, value),
            Value::Object(o) => {
                let o = o.clone();
                return self.array_access_write(&o, key, action, value);
            }
            _ => {
                if is_unset {
                    return self.throw("Error", "Cannot unset offset in a non-array variable");
                }
                return self.throw("Error", "Cannot use a scalar value as an array");
            }
        }
        let k = match key {
            None => None,
            Some(k) => Some(self.to_key(&k, "array")?),
        };
        if self.array_container(cur, is_unset)?.is_none() {
            return Ok(Value::Null);
        }
        if !matches!(unsafe { &*cur }, Value::Array(_)) {
            return Ok(Value::Null);
        }
        macro_rules! arr {
            () => {
                match unsafe { &mut *cur } {
                    Value::Array(a) => a,
                    _ => return Ok(Value::Null),
                }
            };
        }
        match action {
            PathAction::Assign(_) => {
                let v = value.unwrap();
                let a = arr!();
                match k {
                    None => {
                        if a.push(v.clone()).is_err() {
                            return self.throw(
                                "Error",
                                "Cannot add element to the array as the next element is already occupied",
                            );
                        }
                    }
                    Some(k) => {
                        let old = a.assign(k, v.clone());
                        drop(old);
                    }
                }
                Ok(v)
            }
            PathAction::Unset => {
                if let Some(k) = k {
                    let old = arr!().remove(k.as_ref());
                    drop(old);
                }
                Ok(Value::Null)
            }
            PathAction::MakeRef | PathAction::FetchW => {
                let a = arr!();
                let slot = match k {
                    None => match a.make_mut().push_entry() {
                        Ok(s) => s,
                        Err(e) => return Err(self.php_error(e.to_error())),
                    },
                    Some(k) => a.entry(k),
                };
                Ok(Value::Ref(slot.make_ref()))
            }
            PathAction::BindRef(_) => {
                let r = match value.unwrap() {
                    Value::Ref(r) => r,
                    other => PhpRef::new(other),
                };
                let a = arr!();
                match k {
                    None => {
                        if a.push(Value::Ref(r.clone())).is_err() {
                            return self.throw(
                                "Error",
                                "Cannot add element to the array as the next element is already occupied",
                            );
                        }
                    }
                    Some(k) => {
                        let old = a.insert(k, Value::Ref(r.clone()));
                        drop(old);
                    }
                }
                Ok(Value::Ref(r))
            }
            PathAction::AssignOp(_, _) | PathAction::IncDec(_) => {
                if let Some(key) = &k {
                    let missing = arr!().get(key.as_ref()).is_none();
                    if missing {
                        self.warn(&format!("Undefined array key {}", key_desc(key)))?;
                    }
                }
                let a = arr!();
                let slot: *mut Value = match k {
                    None => match a.make_mut().push_entry() {
                        Ok(s) => s as *mut Value,
                        Err(e) => return Err(self.php_error(e.to_error())),
                    },
                    Some(key) => a.entry(key) as *mut Value,
                };
                match action {
                    PathAction::AssignOp(op, _) => self.rmw_slot(slot, op, value.unwrap()),
                    PathAction::IncDec(op) => self.incdec_slot(slot, op),
                    _ => unreachable!(),
                }
            }
        }
    }

    /// `slot op= rhs` on a value slot; returns the new value.
    pub(crate) fn rmw_slot(&mut self, slot: *mut Value, op: Bin, rhs: Value) -> R<Value> {
        let slot = deref_ptr(slot);
        if op == Bin::Concat
            && let (Value::Str(s), Value::Str(r)) = (unsafe { &mut *slot }, &rhs)
        {
            s.push_bytes(r);
            return Ok(unsafe { (*slot).clone() });
        }
        let old = unsafe { (*slot).clone() };
        let new = self.binary_op(op, &old, &rhs)?;
        drop(old);
        unsafe {
            let o = std::mem::replace(&mut *slot, new.clone());
            drop(o);
        }
        Ok(new)
    }

    pub(crate) fn incdec_slot(&mut self, slot: *mut Value, op: IncDec) -> R<Value> {
        let slot = deref_ptr(slot);
        let old = unsafe { (*slot).clone() };
        let new = self.incdec_value(op, &old)?;
        unsafe {
            let o = std::mem::replace(&mut *slot, new.clone());
            drop(o);
        }
        Ok(if op.is_post() { old } else { new })
    }

    fn string_offset_write(
        &mut self,
        cur: *mut Value,
        key: Option<Value>,
        action: PathAction,
        value: Option<Value>,
    ) -> R<Value> {
        match action {
            PathAction::Unset => return self.throw("Error", "Cannot unset string offsets"),
            PathAction::MakeRef | PathAction::BindRef(_) => {
                return self.throw("Error", "Cannot create references to/from string offsets");
            }
            PathAction::AssignOp(..) | PathAction::IncDec(_) => {
                return self.throw(
                    "Error",
                    "Cannot use assign-op operators with string offsets",
                );
            }
            _ => {}
        }
        let Some(k) = key else {
            return self.throw("Error", "[] operator not supported for strings");
        };
        let off = match &k {
            Value::Int(i) => *i,
            Value::Str(s) => match goro_runtime::array::canonical_int(s) {
                Some(i) => i,
                None => {
                    let p = goro_runtime::numeric::parse_numeric(s);
                    match p.kind {
                        goro_runtime::numeric::Numericness::Numeric => {
                            self.warn("String offset cast occurred")?;
                            p.num.to_f64() as i64
                        }
                        _ => {
                            return self
                                .type_error("Cannot access offset of type string on string");
                        }
                    }
                }
            },
            Value::Float(f) => {
                self.warn("String offset cast occurred")?;
                *f as i64
            }
            Value::Bool(b) => {
                self.warn("String offset cast occurred")?;
                *b as i64
            }
            Value::Null => {
                self.warn("String offset cast occurred")?;
                0
            }
            other => {
                return self.type_error(&format!(
                    "Cannot access offset of type {} on string",
                    other.tname()
                ));
            }
        };
        let v = value.unwrap();
        let vs = match &v {
            Value::Str(s) => s.clone(),
            other => self.to_php_str(other)?,
        };
        let Value::Str(s) = (unsafe { &mut *cur }) else {
            unreachable!()
        };
        let len = s.len() as i64;
        let idx = if off < 0 { off + len } else { off };
        if idx < 0 {
            self.warn(&format!("Illegal string offset {}", off))?;
            return Ok(Value::Null);
        }
        if vs.is_empty() {
            return self.throw("Error", "Cannot assign an empty string to a string offset");
        }
        if vs.len() > 1 {
            self.warn("Only the first byte will be assigned to the string offset")?;
        }
        let Value::Str(s) = (unsafe { &mut *cur }) else {
            unreachable!()
        };
        let idx = idx as usize;
        let mut bytes = s.to_vec();
        if idx >= bytes.len() {
            bytes.resize(idx, b' ');
            bytes.push(vs[0]);
        } else {
            bytes[idx] = vs[0];
        }
        *s = PhpStr::from(bytes);
        Ok(Value::Str(PhpStr::from_byte(vs[0])))
    }

    fn array_access_write(
        &mut self,
        o: &ObjRef,
        key: Option<Value>,
        action: PathAction,
        value: Option<Value>,
    ) -> R<Value> {
        let cls = self.class_of(o);
        if !self.implements_array_access(&cls) {
            return self.throw(
                "Error",
                &format!("Cannot use object of type {} as array", cls.name.dname()),
            );
        }
        let k = key.clone().unwrap_or(Value::Null);
        match action {
            PathAction::Assign(_) => {
                let v = value.unwrap();
                self.call_method(o, b"offsetset", vec![k, v.clone()])?;
                Ok(v)
            }
            PathAction::Unset => {
                self.call_method(o, b"offsetunset", vec![k])?;
                Ok(Value::Null)
            }
            PathAction::AssignOp(op, _) => {
                let old = self.call_method(o, b"offsetget", vec![k.clone()])?;
                let new = self.binary_op(op, &old.deref(), &value.unwrap())?;
                self.call_method(o, b"offsetset", vec![k, new.clone()])?;
                Ok(new)
            }
            PathAction::IncDec(op) => {
                // Internal ArrayAccess (ArrayObject...) updates its storage.
                let internal = cls.native.dim_storage.is_some()
                    && cls.find_method(b"offsetget").is_some_and(|f| f.is_native());
                let got = self.call_method_raw(o, b"offsetget", vec![k.clone()])?;
                if internal {
                    let old = got.into_deref();
                    let new = self.incdec_value(op, &old)?;
                    self.call_method(o, b"offsetset", vec![k, new.clone()])?;
                    return Ok(if op.is_post() { old } else { new });
                }
                // A user offsetGet: only a returned reference is modified
                // (test tests/classes/array_access_003.phpt).
                match got {
                    Value::Ref(r) => {
                        let old = r.get();
                        let new = self.incdec_value(op, &old)?;
                        let prev = r.set(new.clone());
                        drop(prev);
                        Ok(if op.is_post() { old } else { new })
                    }
                    old => {
                        self.notice_msg(&format!(
                            "Indirect modification of overloaded element of {} has no effect",
                            cls.name.dname()
                        ))?;
                        let new = self.incdec_value(op, &old)?;
                        Ok(if op.is_post() { old } else { new })
                    }
                }
            }
            PathAction::MakeRef | PathAction::FetchW => {
                let v = self.call_method(o, b"offsetget", vec![k])?;
                match v {
                    Value::Ref(r) => Ok(Value::Ref(r)),
                    other => {
                        self.notice_msg(&format!(
                            "Indirect modification of overloaded element of {} has no effect",
                            cls.name.dname()
                        ))?;
                        Ok(Value::Ref(PhpRef::new(other)))
                    }
                }
            }
            PathAction::BindRef(_) => self.throw(
                "Error",
                "Cannot assign by reference to an array dimension of an object",
            ),
        }
    }

    // ------------------------------------------------------------ whole-variable actions

    fn var_action(
        &mut self,
        var: Reg,
        action: PathAction,
        value: Option<Value>,
        res: Reg,
    ) -> R<()> {
        let r = match action {
            PathAction::Assign(_) => {
                let v = value.unwrap();
                self.assign_var(var, v.clone())?;
                v
            }
            PathAction::Unset => {
                self.unset_var(var);
                Value::Null
            }
            PathAction::MakeRef | PathAction::FetchW => Value::Ref(self.var_make_ref(var)?),
            PathAction::BindRef(_) => {
                let v = value.unwrap();
                self.bind_ref_var(var, v.clone())?;
                v
            }
            PathAction::AssignOp(op, _) => {
                let cur = self.get(var)?;
                let new = self.binary_op(op, &cur, &value.unwrap())?;
                self.assign_var(var, new.clone())?;
                new
            }
            PathAction::IncDec(op) => {
                let cur = self.get(var)?;
                let new = self.incdec_value(op, &cur)?;
                self.assign_var(var, new.clone())?;
                if op.is_post() { cur } else { new }
            }
        };
        if res != NONE {
            let p = self.slot_ptr(res);
            let old = unsafe { (*p).replace(r) };
            drop(old);
        }
        Ok(())
    }

    /// Actions on a reference cell (static properties, globals).
    pub(crate) fn cell_action(
        &mut self,
        cell: &PhpRef,
        action: PathAction,
        value: Option<Value>,
        res: Reg,
    ) -> R<()> {
        let r = match action {
            PathAction::Assign(_) => {
                let mut v = value.unwrap().into_deref();
                if cell.is_typed() {
                    v = self.typed_ref_value(cell, v)?;
                }
                let old = cell.set(v.clone());
                drop(old);
                v
            }
            PathAction::Unset => Value::Null,
            PathAction::MakeRef | PathAction::FetchW => Value::Ref(cell.clone()),
            PathAction::BindRef(_) => {
                // Static property bound by reference: replace its value.
                let v = value.unwrap();
                let nv = v.deref();
                let old = cell.set(nv);
                drop(old);
                v
            }
            PathAction::AssignOp(op, _) => {
                let cur = cell.get();
                let new = self.binary_op(op, &cur, &value.unwrap())?;
                let old = cell.set(new.clone());
                drop(old);
                new
            }
            PathAction::IncDec(op) => {
                let cur = cell.get();
                let new = self.incdec_value(op, &cur)?;
                let old = cell.set(new.clone());
                drop(old);
                if op.is_post() { cur } else { new }
            }
        };
        if res != NONE {
            let p = self.slot_ptr(res);
            let old = unsafe { (*p).replace(r) };
            drop(old);
        }
        Ok(())
    }
}

/// Follows a reference: the pointer to the referenced value.
#[inline]
pub(crate) fn deref_ptr(p: *mut Value) -> *mut Value {
    unsafe {
        match &*p {
            Value::Ref(r) => r.borrow_mut_ptr(),
            _ => p,
        }
    }
}

/// Raw access to a reference's value (no borrow guard held).
pub(crate) trait RefPtr {
    fn borrow_mut_ptr(&self) -> *mut Value;
}

impl RefPtr for PhpRef {
    fn borrow_mut_ptr(&self) -> *mut Value {
        let mut g = self.borrow_mut();
        let p = &mut *g as *mut Value;
        drop(g);
        p
    }
}

pub(crate) fn rc_ptr<T>(r: &Rc<T>) -> *const T {
    Rc::as_ptr(r)
}
