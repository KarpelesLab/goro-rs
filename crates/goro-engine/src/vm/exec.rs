//! The dispatch loop and operand access.

use goro_runtime::{PhpArray, PhpStr, Value};

use super::{R, Slot, Unwind, Vm, frame_flags};
use crate::bytecode::*;

/// A slot whose value needs no drop (empty, or a scalar): it can be
/// overwritten without running the destructor glue.
#[inline(always)]
pub(crate) fn holds_no_resources(s: &Slot) -> bool {
    matches!(
        s,
        None | Some(Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_))
    )
}

/// Arithmetic, bitwise and comparison operators on ints and floats without
/// side effects (no warnings, no errors): the result, or `None` for the
/// general path. PHP manual (Arithmetic Operators): int results that
/// overflow become floats; `/` of ints gives an int when exact; `%` is on
/// ints with the sign of the dividend.
#[inline(always)]
pub(crate) fn fast_arith(op: Bin, x: &Value, y: &Value) -> Option<Value> {
    use Value::{Bool, Float, Int};
    match (x, y) {
        (Int(a), Int(b)) => {
            let (a, b) = (*a, *b);
            Some(match op {
                Bin::Add => a
                    .checked_add(b)
                    .map(Int)
                    .unwrap_or(Float(a as f64 + b as f64)),
                Bin::Sub => a
                    .checked_sub(b)
                    .map(Int)
                    .unwrap_or(Float(a as f64 - b as f64)),
                Bin::Mul => a
                    .checked_mul(b)
                    .map(Int)
                    .unwrap_or(Float(a as f64 * b as f64)),
                Bin::Div => {
                    if b == 0 || (a == i64::MIN && b == -1) {
                        return None;
                    }
                    if a % b == 0 {
                        Int(a / b)
                    } else {
                        Float(a as f64 / b as f64)
                    }
                }
                Bin::Mod => {
                    if b == 0 {
                        return None;
                    }
                    if b == -1 { Int(0) } else { Int(a % b) }
                }
                Bin::BitAnd => Int(a & b),
                Bin::BitOr => Int(a | b),
                Bin::BitXor => Int(a ^ b),
                Bin::Shl if (0..64).contains(&b) => Int(a.wrapping_shl(b as u32)),
                Bin::Shr if (0..64).contains(&b) => Int(a >> b),
                Bin::Lt => Bool(a < b),
                Bin::Le => Bool(a <= b),
                Bin::Eq | Bin::Identical => Bool(a == b),
                Bin::Ne | Bin::NotIdentical => Bool(a != b),
                Bin::Spaceship => Int((a > b) as i64 - (a < b) as i64),
                _ => return None,
            })
        }
        (Float(_), Float(_)) | (Int(_), Float(_)) | (Float(_), Int(_)) => {
            let fa = match x {
                Int(i) => *i as f64,
                Float(f) => *f,
                _ => unreachable!(),
            };
            let fb = match y {
                Int(i) => *i as f64,
                Float(f) => *f,
                _ => unreachable!(),
            };
            Some(match op {
                Bin::Add => Float(fa + fb),
                Bin::Sub => Float(fa - fb),
                Bin::Mul => Float(fa * fb),
                Bin::Div if fb != 0.0 => Float(fa / fb),
                Bin::Lt => Bool(fa < fb),
                Bin::Le => Bool(fa <= fb),
                Bin::Eq => Bool(fa == fb),
                Bin::Ne => Bool(fa != fb),
                Bin::Identical if matches!((x, y), (Float(_), Float(_))) => Bool(fa == fb),
                Bin::NotIdentical if matches!((x, y), (Float(_), Float(_))) => Bool(fa != fb),
                _ => return None,
            })
        }
        _ => None,
    }
}

/// `x === null` on defined values (PHP manual, Comparison Operators: same
/// type and value); `None` when neither is null or a reference is involved.
#[inline]
fn null_identity(x: &Value, y: &Value) -> Option<bool> {
    match (x, y) {
        (Value::Null, v) | (v, Value::Null) if !matches!(v, Value::Ref(_)) => {
            Some(matches!(v, Value::Null))
        }
        _ => None,
    }
}

impl Vm {
    // ------------------------------------------------------------ operands

    #[inline(always)]
    pub(crate) fn code_ref(&self) -> &FuncCode {
        // SAFETY: the running frame keeps its code alive.
        unsafe { &*self.ex.top.code }
    }

    #[inline(always)]
    pub(crate) fn base(&self) -> *mut Slot {
        self.ex.top.base
    }

    #[inline(always)]
    pub(crate) fn slot_ptr(&self, r: Reg) -> *mut Slot {
        unsafe { self.base().add(slot_of(r)) }
    }

    /// Peeks at an operand without consuming it. `None`: an undefined
    /// variable.
    #[inline(always)]
    pub(crate) fn peek(&self, r: Reg) -> Option<&Value> {
        if r & CONST != 0 {
            let code = self.code_ref();
            Some(unsafe { code.consts.get_unchecked((r & !CONST) as usize) })
        } else {
            unsafe { (*self.slot_ptr(r)).as_ref() }
        }
    }

    /// The value of an operand, dereferenced: constants are cloned,
    /// temporaries moved out, variables copied (with PHP's warning for an
    /// undefined variable).
    #[inline]
    pub(crate) fn get(&mut self, r: Reg) -> R<Value> {
        if r & CONST != 0 {
            let code = self.code_ref();
            return Ok(unsafe { code.consts.get_unchecked((r & !CONST) as usize).clone() });
        }
        let p = self.slot_ptr(r);
        if r & TMP != 0 {
            return Ok(match unsafe { (*p).take() } {
                Some(Value::Ref(rf)) => rf.get(),
                Some(v) => v,
                None => Value::Null,
            });
        }
        match unsafe { &*p } {
            Some(Value::Ref(rf)) => Ok(rf.get()),
            Some(v) => Ok(v.clone()),
            None => {
                self.undefined_var(r)?;
                Ok(Value::Null)
            }
        }
    }

    /// Like [`get`](Self::get) but without the undefined-variable warning.
    #[inline]
    pub(crate) fn get_quiet(&mut self, r: Reg) -> Value {
        if r & CONST != 0 {
            let code = self.code_ref();
            return unsafe { code.consts.get_unchecked((r & !CONST) as usize).clone() };
        }
        let p = self.slot_ptr(r);
        if r & TMP != 0 {
            return match unsafe { (*p).take() } {
                Some(Value::Ref(rf)) => rf.get(),
                Some(v) => v,
                None => Value::Null,
            };
        }
        match unsafe { &*p } {
            Some(Value::Ref(rf)) => rf.get(),
            Some(v) => v.clone(),
            None => {
                if let Some(v) = self.attached_lookup(r) {
                    return v.deref();
                }
                Value::Null
            }
        }
    }

    /// The raw operand value (references kept), consuming temporaries.
    #[inline]
    pub(crate) fn get_raw(&mut self, r: Reg) -> Value {
        if r & CONST != 0 {
            let code = self.code_ref();
            return unsafe { code.consts.get_unchecked((r & !CONST) as usize).clone() };
        }
        let p = self.slot_ptr(r);
        if r & TMP != 0 {
            return unsafe { (*p).take() }.unwrap_or_default();
        }
        unsafe { (*p).clone() }.unwrap_or_default()
    }

    /// Releases a consumed temporary without reading it.
    #[inline(always)]
    pub(crate) fn drop_tmp(&mut self, r: Reg) {
        if r & (CONST | TMP) == TMP {
            let p = self.slot_ptr(r);
            unsafe {
                if let Some(v) = (*p).take() {
                    drop(v);
                }
            }
        }
    }

    /// Stores a result: through references for variables, plainly for
    /// temporaries.
    #[inline]
    pub(crate) fn put(&mut self, dst: Reg, v: Value) {
        if dst == NONE {
            return;
        }
        let p = self.slot_ptr(dst);
        unsafe {
            if dst & TMP != 0 {
                // Temporaries: no references, usually empty.
                if holds_no_resources(&*p) {
                    std::ptr::write(p, Some(v));
                } else {
                    let old = (*p).replace(v);
                    drop(old);
                }
                return;
            }
            match &mut *p {
                Some(Value::Ref(rf)) => {
                    let old = rf.set(v.into_deref());
                    drop(old);
                }
                slot => {
                    if slot.is_none() && self.frame().flags & frame_flags::ATTACHED != 0 {
                        self.put_attached(dst, v);
                        return;
                    }
                    let v = v.into_deref();
                    if holds_no_resources(slot) {
                        std::ptr::write(slot, Some(v));
                    } else {
                        let old = slot.replace(v);
                        drop(old);
                    }
                }
            }
        }
    }

    #[inline(always)]
    pub(crate) fn put_int(&mut self, dst: Reg, i: i64) {
        let p = self.slot_ptr(dst);
        unsafe {
            match &mut *p {
                Some(Value::Int(x)) => *x = i,
                _ => self.put(dst, Value::Int(i)),
            }
        }
    }

    #[inline(always)]
    pub(crate) fn put_bool(&mut self, dst: Reg, b: bool) {
        let p = self.slot_ptr(dst);
        unsafe {
            match &mut *p {
                Some(Value::Bool(x)) => *x = b,
                _ => self.put(dst, Value::Bool(b)),
            }
        }
    }

    /// The name of a variable slot (for messages).
    pub(crate) fn var_name(&self, r: Reg) -> PhpStr {
        let code = self.code_ref();
        code.cv_names
            .get(slot_of(r))
            .cloned()
            .unwrap_or_else(PhpStr::empty)
    }

    #[cold]
    pub(crate) fn undefined_var(&mut self, r: Reg) -> R<()> {
        // An attached frame (include/eval) may find the variable in its
        // owner scope.
        if self.frame().flags & frame_flags::ATTACHED != 0 && self.attached_bind(r) {
            return Ok(());
        }
        let n = self.var_name(r);
        self.warn(&format!("Undefined variable ${}", n.to_string_lossy()))
    }

    // ------------------------------------------------------------ the loop

    /// Runs frames until the entry frame on top of the stack when called
    /// returns; gives its return value.
    pub fn run(&mut self) -> R<Value> {
        let entry = self.ex.frames.len();
        self.run_depth += 1;
        let r = loop {
            match self.exec(entry) {
                Ok(v) => break Ok(v),
                Err(Unwind::Throw(ex)) => {
                    if let Err(e) = self.dispatch_exception(ex, entry) {
                        break Err(e);
                    }
                }
                Err(e) => {
                    self.unwind_frames(entry - 1);
                    break Err(e);
                }
            }
        };
        self.run_depth -= 1;
        r
    }

    /// The instruction loop. Returns when the frame at depth `entry`
    /// returns.
    fn exec(&mut self, entry: usize) -> R<Value> {
        let _ = entry;
        loop {
            if goro_runtime::object::has_pending_destructors() {
                self.run_pending_destructors()?;
            }
            let ip = self.ex.ip;
            let op = unsafe { *self.code_ref().ops.get_unchecked(ip) };
            self.ex.ip = ip + 1;
            match op {
                Op::Nop | Op::Ticks => {}
                Op::Assign { dst, src } => {
                    let v = self.get(src)?;
                    self.assign_var(dst, v)?;
                }
                Op::AssignRes { dst, src, res } => {
                    let v = self.get(src)?;
                    self.assign_var(dst, v)?;
                    let v = self.get_quiet(dst);
                    self.put(res, v);
                }
                Op::Copy { dst, src } => {
                    let v = if src & (CONST | TMP) == TMP {
                        self.get_raw(src)
                    } else {
                        self.get(src)?
                    };
                    let p = self.slot_ptr(dst);
                    let old = unsafe { (*p).replace(v) };
                    drop(old);
                }
                Op::AssignRef { dst, src } => self.assign_ref_vars(dst, src)?,
                Op::BindRef { dst, src } => {
                    let v = self.get_raw(src);
                    self.bind_ref_var(dst, v)?;
                }
                Op::UnsetVar { var } => self.unset_var(var),
                Op::Free { slot } => {
                    let p = self.slot_ptr(slot);
                    let old = unsafe { (*p).take() };
                    drop(old);
                }
                Op::Add { dst, a, b } => {
                    if let (Some(Value::Int(x)), Some(Value::Int(y))) = (self.peek(a), self.peek(b))
                    {
                        match x.checked_add(*y) {
                            Some(s) => self.put_int(dst, s),
                            None => {
                                let f = *x as f64 + *y as f64;
                                self.put(dst, Value::Float(f));
                            }
                        }
                    } else if let (Some(x), Some(y)) = (self.peek(a), self.peek(b))
                        && let Some(r) = fast_arith(Bin::Add, x, y)
                    {
                        self.drop_tmp(a);
                        self.drop_tmp(b);
                        self.put(dst, r);
                    } else {
                        self.binop_slow(Bin::Add, dst, a, b)?;
                    }
                }
                Op::Sub { dst, a, b } => {
                    if let (Some(Value::Int(x)), Some(Value::Int(y))) = (self.peek(a), self.peek(b))
                    {
                        match x.checked_sub(*y) {
                            Some(s) => self.put_int(dst, s),
                            None => {
                                let f = *x as f64 - *y as f64;
                                self.put(dst, Value::Float(f));
                            }
                        }
                    } else if let (Some(x), Some(y)) = (self.peek(a), self.peek(b))
                        && let Some(r) = fast_arith(Bin::Sub, x, y)
                    {
                        self.drop_tmp(a);
                        self.drop_tmp(b);
                        self.put(dst, r);
                    } else {
                        self.binop_slow(Bin::Sub, dst, a, b)?;
                    }
                }
                Op::Mul { dst, a, b } => {
                    if let (Some(Value::Int(x)), Some(Value::Int(y))) = (self.peek(a), self.peek(b))
                    {
                        match x.checked_mul(*y) {
                            Some(s) => self.put_int(dst, s),
                            None => {
                                let f = *x as f64 * *y as f64;
                                self.put(dst, Value::Float(f));
                            }
                        }
                    } else if let (Some(x), Some(y)) = (self.peek(a), self.peek(b))
                        && let Some(r) = fast_arith(Bin::Mul, x, y)
                    {
                        self.drop_tmp(a);
                        self.drop_tmp(b);
                        self.put(dst, r);
                    } else {
                        self.binop_slow(Bin::Mul, dst, a, b)?;
                    }
                }
                Op::Concat { dst, a, b } => self.op_concat(dst, a, b)?,
                Op::Lt { dst, a, b } => {
                    if let (Some(Value::Int(x)), Some(Value::Int(y))) = (self.peek(a), self.peek(b))
                    {
                        let r = x < y;
                        self.put_bool(dst, r);
                    } else if let (Some(x), Some(y)) = (self.peek(a), self.peek(b))
                        && let Some(Value::Bool(r)) = fast_arith(Bin::Lt, x, y)
                    {
                        self.drop_tmp(a);
                        self.drop_tmp(b);
                        self.put_bool(dst, r);
                    } else {
                        self.binop_slow(Bin::Lt, dst, a, b)?;
                    }
                }
                Op::Le { dst, a, b } => {
                    if let (Some(Value::Int(x)), Some(Value::Int(y))) = (self.peek(a), self.peek(b))
                    {
                        let r = x <= y;
                        self.put_bool(dst, r);
                    } else if let (Some(x), Some(y)) = (self.peek(a), self.peek(b))
                        && let Some(Value::Bool(r)) = fast_arith(Bin::Le, x, y)
                    {
                        self.drop_tmp(a);
                        self.drop_tmp(b);
                        self.put_bool(dst, r);
                    } else {
                        self.binop_slow(Bin::Le, dst, a, b)?;
                    }
                }
                Op::Eq { dst, a, b } => {
                    if let (Some(Value::Int(x)), Some(Value::Int(y))) = (self.peek(a), self.peek(b))
                    {
                        let r = x == y;
                        self.put_bool(dst, r);
                    } else {
                        self.binop_slow(Bin::Eq, dst, a, b)?;
                    }
                }
                Op::Identical { dst, a, b } => {
                    if let (Some(Value::Int(x)), Some(Value::Int(y))) = (self.peek(a), self.peek(b))
                    {
                        let r = x == y;
                        self.put_bool(dst, r);
                    } else {
                        self.binop_slow(Bin::Identical, dst, a, b)?;
                    }
                }
                Op::BinOp { op, dst, a, b } => {
                    if let (Some(x), Some(y)) = (self.peek(a), self.peek(b))
                        && let Some(r) = fast_arith(op, x, y)
                    {
                        self.drop_tmp(a);
                        self.drop_tmp(b);
                        self.put(dst, r);
                    } else {
                        self.binop_slow(op, dst, a, b)?
                    }
                }
                Op::Unary { op, dst, a } => self.op_unary(op, dst, a)?,
                Op::Cast { to, dst, a } => {
                    let v = self.get(a)?;
                    let r = self.cast(to, v)?;
                    self.put(dst, r);
                }
                Op::Rope { dst, list } => self.op_rope(dst, list)?,
                Op::AssignOpVar { op, var, src, res } => {
                    self.op_assign_op_var(op, var, src, res)?
                }
                Op::IncDecVar { op, var, res } => self.op_incdec_var(op, var, res)?,
                Op::Jmp { t } => self.ex.ip = t as usize,
                Op::JmpZ { c, t } => {
                    let b = match self.peek(c) {
                        Some(Value::Bool(b)) => {
                            let b = *b;
                            self.drop_tmp(c);
                            b
                        }
                        _ => {
                            let v = self.get(c)?;
                            self.to_bool(&v)
                        }
                    };
                    if !b {
                        self.ex.ip = t as usize;
                    }
                }
                Op::JmpNZ { c, t } => {
                    let b = match self.peek(c) {
                        Some(Value::Bool(b)) => {
                            let b = *b;
                            self.drop_tmp(c);
                            b
                        }
                        _ => {
                            let v = self.get(c)?;
                            self.to_bool(&v)
                        }
                    };
                    if b {
                        self.ex.ip = t as usize;
                    }
                }
                Op::JmpLt { a, b, t } => {
                    let r = if let (Some(Value::Int(x)), Some(Value::Int(y))) =
                        (self.peek(a), self.peek(b))
                    {
                        x < y
                    } else {
                        let x = self.get(a)?;
                        let y = self.get(b)?;
                        goro_runtime::compare::less(&x, &y, self).map_err(|e| self.php_error(e))?
                    };
                    self.check_deferred()?;
                    if r {
                        self.ex.ip = t as usize;
                    }
                }
                Op::JmpZEx { c, res, t } => {
                    let v = self.get(c)?;
                    let b = self.to_bool(&v);
                    self.put(res, Value::Bool(b));
                    if !b {
                        self.ex.ip = t as usize;
                    }
                }
                Op::JmpNZEx { c, res, t } => {
                    let v = self.get(c)?;
                    let b = self.to_bool(&v);
                    self.put(res, Value::Bool(b));
                    if b {
                        self.ex.ip = t as usize;
                    }
                }
                Op::JmpSet { c, res, t } => {
                    let v = self.get_quiet(c);
                    if !v.is_null() {
                        self.put(res, v);
                        self.ex.ip = t as usize;
                    }
                }
                Op::JmpNull { c, res, t } => {
                    let is_null = match self.peek(c) {
                        None => true,
                        Some(v) => v.with_deref(|v| v.is_null()),
                    };
                    if is_null {
                        if self.peek(c).is_none() && c & (CONST | TMP) == 0 {
                            self.undefined_var(c)?;
                        }
                        self.drop_tmp(c);
                        if res != NONE {
                            self.put(res, Value::Null);
                        }
                        self.ex.ip = t as usize;
                    }
                }
                Op::Switch { .. } => {
                    return Err(self.throw_error_msg("Error", "switch tables are not supported"));
                }
                Op::Echo { src } => {
                    match self.peek(src) {
                        Some(Value::Str(s)) => {
                            let s = s.clone();
                            self.drop_tmp(src);
                            self.echo(&s);
                        }
                        _ => {
                            let v = self.get(src)?;
                            self.echo_value(&v)?;
                        }
                    }
                    // An output handler run by a full chunk may have thrown.
                    self.check_deferred()?;
                }
                Op::Return { src } => {
                    let v = self.get(src)?;
                    if let Some(v) = self.do_return(v, false)? {
                        return Ok(v);
                    }
                }
                Op::ReturnRef { src } => {
                    let mut v = self.get_raw(src);
                    if !matches!(v, Value::Ref(_)) {
                        self.notice_msg(
                            "Only variable references should be returned by reference",
                        )?;
                        v = Value::Ref(goro_runtime::PhpRef::new(v));
                    }
                    if let Some(v) = self.do_return(v, true)? {
                        return Ok(v);
                    }
                }
                Op::RecvDefault { param, t } => {
                    let p = self.slot_ptr(param);
                    if unsafe { (*p).is_some() } {
                        self.ex.ip = t as usize;
                    }
                }
                Op::RecvCheck { param } => self.recv_check(param)?,
                Op::RecvVariadic { .. } => {}
                Op::VerifyReturn { src, implicit } => self.verify_return(src, implicit)?,
                Op::NewArray { dst, cap } => {
                    let a = if cap == 0 {
                        PhpArray::new()
                    } else {
                        PhpArray::with_capacity(cap as usize)
                    };
                    let p = self.slot_ptr(dst);
                    let old = unsafe { (*p).replace(Value::Array(a)) };
                    drop(old);
                }
                Op::AddElem { arr, key, val } => self.op_add_elem(arr, key, val, false)?,
                Op::AddElemRef { arr, key, val } => self.op_add_elem(arr, key, val, true)?,
                Op::AddSpread { arr, src } => self.op_add_spread(arr, src)?,
                Op::FetchDim { dst, base, key } => {
                    self.op_fetch_dim(dst, base, key, super::dims::DimMode::Read)?
                }
                Op::FetchDimIs { dst, base, key } => {
                    self.op_fetch_dim(dst, base, key, super::dims::DimMode::Is)?
                }
                Op::FetchList { dst, base, key } => {
                    self.op_fetch_dim(dst, base, key, super::dims::DimMode::List)?
                }
                Op::IssetDim {
                    dst,
                    base,
                    key,
                    empty,
                } => self.op_isset_dim(dst, base, key, empty)?,
                Op::AssignDim { var, key, val, res } => self.op_assign_dim(var, key, val, res)?,
                Op::Write { path, res } => self.op_write(path, res)?,
                Op::FetchProp {
                    dst,
                    obj,
                    name,
                    cache,
                } => self.op_fetch_prop(dst, obj, name, cache, false)?,
                Op::FetchPropIs {
                    dst,
                    obj,
                    name,
                    cache,
                } => self.op_fetch_prop(dst, obj, name, cache, true)?,
                Op::IssetProp {
                    dst,
                    obj,
                    name,
                    empty,
                } => self.op_isset_prop(dst, obj, name, empty)?,
                Op::AssignProp {
                    obj,
                    name,
                    val,
                    cache,
                } => self.op_assign_prop(obj, name, val, cache)?,
                Op::FetchStaticProp { dst, class, name } => {
                    let (r, _) = self.static_prop_ref(class, name, false)?;
                    let v = r.get();
                    self.put(dst, v);
                }
                Op::IssetStaticProp {
                    dst,
                    class,
                    name,
                    empty,
                } => self.op_isset_static_prop(dst, class, name, empty)?,
                Op::ClassConst {
                    dst,
                    class,
                    name,
                    cache,
                } => self.op_class_const(dst, class, name, cache)?,
                Op::ClassName { dst, class } => {
                    let c = self.resolve_class_name_spec(class)?;
                    let n = c.name.clone();
                    self.put(dst, Value::Str(n));
                }
                Op::InstanceOf { dst, a, class } => self.op_instanceof(dst, a, class)?,
                Op::New {
                    dst,
                    class,
                    nargs,
                    cache,
                } => self.op_new(dst, class, nargs, cache)?,
                Op::NewAnon { dst, class, nargs } => self.op_new_anon(dst, class, nargs)?,
                Op::Clone { dst, src } => {
                    let v = self.get(src)?;
                    let r = self.clone_value(v)?;
                    self.put(dst, r);
                }
                Op::CloneWith { dst, src, with } => {
                    let v = self.get(src)?;
                    let w = self.get(with)?;
                    let r = self.clone_with(v, w)?;
                    self.put(dst, r);
                }
                Op::InitFCall {
                    name,
                    nargs,
                    cache,
                    fallback,
                } => self.op_init_fcall(name, nargs, cache, fallback)?,
                Op::InitDynCall { callee, nargs } => self.op_init_dyn_call(callee, nargs)?,
                Op::InitMethod {
                    obj,
                    name,
                    nargs,
                    cache,
                } => self.op_init_method(obj, name, nargs, cache)?,
                Op::InitStaticCall {
                    class,
                    name,
                    nargs,
                    cache,
                } => self.op_init_static_call(class, name, nargs, cache)?,
                Op::InitParentHook { name, set } => self.op_init_parent_hook(name, set)?,
                Op::Send { src, arg } => {
                    let v = self.get(src)?;
                    self.send_val(arg, v, false)?;
                }
                Op::SendValRef { src, arg } => {
                    let v = self.get(src)?;
                    self.send_val(arg, v, true)?;
                }
                Op::SendVar { var, arg } => self.send_var(var, arg)?,
                Op::SendRef { src, arg } => {
                    let v = self.get_raw(src);
                    self.send_ref(arg, v)?;
                }
                Op::SendUnpack { src } => {
                    let v = self.get(src)?;
                    self.send_unpack(v)?;
                }
                Op::SendNamed { src, name } => {
                    let v = self.get_raw(src);
                    let n = self.get(name)?;
                    self.send_named(n, v, false)?;
                }
                Op::SendNamedVar { var, name } => {
                    let n = self.get(name)?;
                    self.send_named_var(n, var)?;
                }
                Op::JmpArgByRef { arg, t } => {
                    if self.pending_arg_by_ref(arg) {
                        self.ex.ip = t as usize;
                    }
                }
                Op::JmpNamedArgByRef { name, t } => {
                    let n = self.peek(name).cloned().unwrap_or_default();
                    if self.pending_named_by_ref(&n) {
                        self.ex.ip = t as usize;
                    }
                }
                Op::DoCall { dst } => {
                    if let Some(v) = self.do_call(dst)? {
                        // A native call returned a value.
                        let _ = v;
                    }
                }
                Op::CallableConvert { dst } => {
                    let c = self.pending_to_closure()?;
                    self.put(dst, c);
                }
                Op::DeclareFunction { func } => {
                    let code = self.code_ref().children[func as usize].clone();
                    self.declare_function(code)?;
                }
                Op::DeclareClass { class } => {
                    let decl = self.code_ref().classes[class as usize].clone();
                    if !decl.declared.get() {
                        self.declare_class(&decl)?;
                    }
                }
                Op::Tick { every } => self.tick(every)?,
                Op::DeclareConst { name, val } => {
                    let n = self.get(name)?;
                    let v = self.get(val)?;
                    self.declare_const_stmt(n, v)?;
                }
                Op::MakeClosure { dst, func } => {
                    let code = self.code_ref().children[func as usize].clone();
                    let c = self.make_closure(code)?;
                    self.put(dst, c);
                }
                Op::BindStatic { var, index, t } => {
                    if self.bind_static(var, index)? {
                        self.ex.ip = t as usize;
                    }
                }
                Op::StaticInit { index, src } => {
                    let v = self.get(src)?;
                    self.static_init(index, v)?;
                }
                Op::BindGlobal { var, name } => {
                    let n = self.get(name)?;
                    self.bind_global(var, n)?;
                }
                Op::FetchGlobal { dst, name, quiet } => {
                    let n = self.get(name)?;
                    let v = self.fetch_global(&n, quiet)?;
                    self.put(dst, v);
                }
                Op::FetchVarVar { dst, name, quiet } => {
                    let n = self.get(name)?;
                    let v = self.fetch_var_var(&n, quiet)?;
                    self.put(dst, v);
                }
                Op::IssetVarVar { dst, name, empty } => {
                    let n = self.get(name)?;
                    let v = self.fetch_var_var(&n, true)?;
                    let r = if empty {
                        !self.to_bool(&v)
                    } else {
                        !v.is_null()
                    };
                    self.put(dst, Value::Bool(r));
                }
                Op::AssignVarVar { name, val, res } => {
                    let n = self.get(name)?;
                    let v = self.get(val)?;
                    self.assign_var_var(&n, v.clone())?;
                    self.put(res, v);
                }
                Op::UnsetVarVar { name } => {
                    let n = self.get(name)?;
                    self.unset_var_var(&n)?;
                }
                Op::RefVar { dst, var } => {
                    let r = self.var_make_ref(var)?;
                    let p = self.slot_ptr(dst);
                    let old = unsafe { (*p).replace(Value::Ref(r)) };
                    drop(old);
                }
                Op::IssetVar { dst, var, empty } => {
                    let v = match self.peek(var) {
                        Some(v) => v.deref(),
                        None => self
                            .attached_lookup(var)
                            .map(|v| v.deref())
                            .unwrap_or_default(),
                    };
                    let r = if empty {
                        !self.to_bool(&v)
                    } else {
                        !v.is_null()
                    };
                    self.put(dst, Value::Bool(r));
                }
                Op::FetchConst {
                    dst,
                    name,
                    cache,
                    fallback,
                } => self.op_fetch_const(dst, name, cache, fallback)?,
                Op::Special { dst, what } => {
                    let v = self.special(what)?;
                    self.put(dst, v);
                }
                Op::Throw { src } => {
                    let v = self.get(src)?;
                    return Err(self.throw_value(v));
                }
                Op::Catch { list, var, next } => self.op_catch(list, var, next)?,
                Op::Rethrow => {
                    let ex = self
                        .cur_exception
                        .take()
                        .expect("rethrow without exception");
                    return Err(Unwind::Throw(ex));
                }
                Op::FastCall { t, ret } => {
                    let back = self.ex.ip as i64;
                    let p = self.slot_ptr(ret);
                    let old = unsafe { (*p).replace(Value::Int(back)) };
                    drop(old);
                    self.ex.ip = t as usize;
                }
                Op::FastRet { ret, try_index } => {
                    let p = self.slot_ptr(ret);
                    let v = unsafe { (*p).take() };
                    match v {
                        Some(Value::Int(back)) => self.ex.ip = back as usize,
                        _ => {
                            // An exception is pending: rethrow it.
                            let tr = self.code_ref().try_ranges[try_index as usize];
                            let p = self.slot_ptr(tr.exc_slot);
                            if let Some(Value::Object(ex)) = unsafe { (*p).take() } {
                                return Err(Unwind::Throw(ex));
                            }
                        }
                    }
                }
                Op::DiscardException { ret } => {
                    let p = self.slot_ptr(ret);
                    let old = unsafe { (*p).take() };
                    drop(old);
                }
                Op::FeReset {
                    iter,
                    src,
                    by_ref,
                    t,
                } => {
                    if !self.fe_reset(iter, src, by_ref)? {
                        self.ex.ip = t as usize;
                    }
                }
                Op::FeFetch { iter, val, key, t } => {
                    if !self.fe_fetch(iter, val, key)? {
                        self.ex.ip = t as usize;
                    }
                }
                Op::FeFree { iter } => self.fe_free(iter)?,
                Op::Yield { dst, val, key } => {
                    if let Some(v) = self.op_yield(dst, val, key)? {
                        return Ok(v);
                    }
                }
                Op::YieldFrom { dst, src } => {
                    if let Some(v) = self.op_yield_from(dst, src)? {
                        return Ok(v);
                    }
                }
                Op::GeneratorReturn { src } => {
                    let v = self.get(src)?;
                    if let Some(v) = self.generator_return(v)? {
                        return Ok(v);
                    }
                }
                Op::Include { dst, src, kind } => {
                    let v = self.get(src)?;
                    self.op_include(dst, v, kind)?;
                }
                Op::Eval { dst, src } => {
                    let v = self.get(src)?;
                    self.op_eval(dst, v)?;
                }
                Op::Exit { src } => {
                    let v = if src == NONE {
                        None
                    } else {
                        Some(self.get(src)?)
                    };
                    return Err(self.do_exit(v)?);
                }
                Op::BeginSilence { save } => {
                    let old = self.ini.error_reporting;
                    let p = self.slot_ptr(save);
                    unsafe { *p = Some(Value::Int(old)) };
                    self.err.silence.push(old);
                    // PHP 8: @ keeps fatal errors visible.
                    self.ini.error_reporting &= 4437;
                }
                Op::EndSilence { save } => {
                    let p = self.slot_ptr(save);
                    if let Some(Value::Int(old)) = unsafe { (*p).take() } {
                        // Only restore if the script didn't change it.
                        if self.ini.error_reporting == old & 4437 {
                            self.ini.error_reporting = old;
                        }
                        self.err.silence.pop();
                    }
                }
                Op::Isset { dst, src } => {
                    let v = self.get_quiet(src);
                    self.put(dst, Value::Bool(!v.is_null()));
                }
                Op::Error { msg, kind } => {
                    let m = self.code_ref().consts[msg as usize].clone();
                    let m = m.as_str().map(|s| s.to_string_lossy()).unwrap_or_default();
                    let _ = kind;
                    return Err(self.throw_error_msg("Error", &m));
                }
                Op::MatchError { src } => {
                    let v = self.get(src)?;
                    let desc = self.match_error_desc(&v);
                    return Err(self.throw_error_msg("UnhandledMatchError", &desc));
                }
                Op::Strlen { dst, src } => {
                    if let Some(Value::Str(s)) = self.peek(src) {
                        let n = s.len() as i64;
                        self.drop_tmp(src);
                        self.put_int(dst, n);
                    } else {
                        let v = self.get(src)?;
                        let r = self.strlen_slow(v)?;
                        self.put(dst, r);
                    }
                }
                Op::Count { dst, src } => {
                    let v = self.get(src)?;
                    let n = self.count_value(&v, false)?;
                    self.put(dst, Value::Int(n));
                }
                Op::TypeCheck { .. } => {}
            }
        }
    }

    // ------------------------------------------------------------ helpers for ops

    #[cold]
    pub(crate) fn binop_slow(&mut self, op: Bin, dst: Reg, a: Reg, b: Reg) -> R<()> {
        if matches!(op, Bin::Identical | Bin::NotIdentical)
            && let (Some(x), Some(y)) = (self.peek(a), self.peek(b))
            && let Some(same) = null_identity(x, y)
        {
            self.drop_tmp(a);
            self.drop_tmp(b);
            self.put_bool(dst, same == (op == Bin::Identical));
            return Ok(());
        }
        let x = self.get(a)?;
        let y = self.get(b)?;
        let r = self.binary_op(op, &x, &y)?;
        drop(x);
        drop(y);
        self.put(dst, r);
        Ok(())
    }

    /// `a op b` on values with PHP semantics.
    pub fn binary_op(&mut self, op: Bin, x: &Value, y: &Value) -> R<Value> {
        use goro_runtime::compare;
        use goro_runtime::ops::{self, BinOp};
        let r = match op {
            Bin::Add => self.arith(BinOp::Add, x, y),
            Bin::Sub => self.arith(BinOp::Sub, x, y),
            Bin::Mul => self.arith(BinOp::Mul, x, y),
            Bin::Div => self.arith(BinOp::Div, x, y),
            Bin::Mod => self.arith(BinOp::Mod, x, y),
            Bin::Pow => self.arith(BinOp::Pow, x, y),
            Bin::BitAnd => self.arith(BinOp::BitAnd, x, y),
            Bin::BitOr => self.arith(BinOp::BitOr, x, y),
            Bin::BitXor => self.arith(BinOp::BitXor, x, y),
            Bin::Shl => self.arith(BinOp::Shl, x, y),
            Bin::Shr => self.arith(BinOp::Shr, x, y),
            Bin::Concat => ops::concat(x, y, self)
                .map(Value::Str)
                .map_err(|e| self.php_error(e)),
            Bin::Eq => compare::loose_eq(x, y, self)
                .map(Value::Bool)
                .map_err(|e| self.php_error(e)),
            Bin::Ne => compare::loose_eq(x, y, self)
                .map(|b| Value::Bool(!b))
                .map_err(|e| self.php_error(e)),
            Bin::Identical => Ok(Value::Bool(compare::strict_eq(x, y))),
            Bin::NotIdentical => Ok(Value::Bool(!compare::strict_eq(x, y))),
            Bin::Lt => compare::less(x, y, self)
                .map(Value::Bool)
                .map_err(|e| self.php_error(e)),
            Bin::Le => compare::less_eq(x, y, self)
                .map(Value::Bool)
                .map_err(|e| self.php_error(e)),
            Bin::Spaceship => compare::compare(x, y, self)
                .map(|c| Value::Int(c as i64))
                .map_err(|e| self.php_error(e)),
            Bin::Coalesce => Ok(if x.is_null() { y.clone() } else { x.clone() }),
        };
        let r = r?;
        self.check_deferred()?;
        Ok(r)
    }

    fn arith(&mut self, op: goro_runtime::ops::BinOp, x: &Value, y: &Value) -> R<Value> {
        if (matches!(x, Value::Object(_) | Value::Ref(_))
            || matches!(y, Value::Object(_) | Value::Ref(_)))
            && let Some(r) = self.overloaded_op(op, &x.deref(), &y.deref())
        {
            return r;
        }
        goro_runtime::ops::binary_op(op, x, y, self).map_err(|e| self.php_error(e))
    }

    /// The `do_operation` hook of an internal class an operand belongs to.
    fn op_hook(
        &self,
        v: &Value,
    ) -> Option<fn(&mut Vm, goro_runtime::ops::BinOp, &Value, &Value) -> Option<R<Value>>> {
        let Value::Object(o) = v else { return None };
        let cls = self.class_of(o);
        let mut c = Some(&*cls);
        while let Some(k) = c {
            if let Some(h) = k.native.do_operation {
                return Some(h);
            }
            c = k.parent.as_deref();
        }
        None
    }

    /// `x op y` through an operand's operator overloading, if any.
    fn overloaded_op(
        &mut self,
        op: goro_runtime::ops::BinOp,
        x: &Value,
        y: &Value,
    ) -> Option<R<Value>> {
        use goro_runtime::ops::BinOp;
        if let Some(h) = self.op_hook(x)
            && let Some(r) = h(self, op, x, y)
        {
            return Some(r);
        }
        let h = self.op_hook(y)?;
        // PHP 8.5 reads a non-object left operand of these operators as an
        // int before the handler runs (crates/goro-ext-gmp docs; observed
        // on PHP 8.5).
        if !matches!(x, Value::Object(_))
            && matches!(
                op,
                BinOp::Mod | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
            )
        {
            let numeric = match x {
                Value::Str(s) => {
                    goro_runtime::numeric::parse_numeric(s).kind
                        != goro_runtime::numeric::Numericness::NonNumeric
                }
                Value::Array(_) | Value::Resource(_) => false,
                _ => true,
            };
            if !numeric {
                let Value::Object(o) = y else { return None };
                let msg = format!(
                    "Unsupported operand types: {} {} {}",
                    x.type_name_for_error(),
                    op.symbol(),
                    o.class_name().to_string_lossy()
                );
                return Some(self.type_error(&msg));
            }
            if let Err(e) = goro_runtime::ops::binary_op(op, x, &Value::Int(1), self) {
                return Some(Err(self.php_error(e)));
            }
            if let Err(e) = self.check_deferred() {
                return Some(Err(e));
            }
        }
        h(self, op, x, y)
    }

    /// A unary operator through the `do_unary` hook of an object's class.
    pub(crate) fn overloaded_unary(
        &mut self,
        op: crate::class::UnaryOp,
        v: &Value,
    ) -> Option<R<Value>> {
        let Value::Object(o) = v.deref() else {
            return None;
        };
        let cls = self.class_of(&o);
        let mut c = Some(&*cls);
        while let Some(k) = c {
            if let Some(h) = k.native.do_unary {
                return h(self, op, &o);
            }
            c = k.parent.as_deref();
        }
        None
    }

    fn op_concat(&mut self, dst: Reg, a: Reg, b: Reg) -> R<()> {
        // Fast path: two strings (or a string and an int).
        if let (Some(Value::Str(x)), Some(Value::Str(y))) = (self.peek(a), self.peek(b)) {
            // Append in place when the left operand is a temporary we own.
            if a & (CONST | TMP) == TMP && dst == a {
                let y = y.clone();
                let p = self.slot_ptr(a);
                if let Some(Value::Str(s)) = unsafe { &mut *p } {
                    s.push_bytes(&y);
                }
                self.drop_tmp(b);
                return Ok(());
            }
            let s = PhpStr::concat(x, y);
            self.drop_tmp(a);
            self.drop_tmp(b);
            self.put(dst, Value::Str(s));
            return Ok(());
        }
        let x = self.get(a)?;
        let y = self.get(b)?;
        let mut out = Vec::new();
        self.append_to_string(&mut out, &x)?;
        self.append_to_string(&mut out, &y)?;
        self.put(dst, Value::Str(PhpStr::from(out)));
        Ok(())
    }

    fn op_rope(&mut self, dst: Reg, list: u32) -> R<()> {
        let n = self.code_ref().lists[list as usize].len();
        let mut out: Vec<u8> = Vec::new();
        for i in 0..n {
            let r = self.code_ref().lists[list as usize][i];
            match self.peek(r) {
                Some(Value::Str(s)) => {
                    out.extend_from_slice(s);
                    self.drop_tmp(r);
                }
                Some(Value::Int(n)) => {
                    let n = *n;
                    goro_runtime::convert::push_int(&mut out, n);
                    self.drop_tmp(r);
                }
                _ => {
                    let v = self.get(r)?;
                    self.append_to_string(&mut out, &v)?;
                }
            }
        }
        self.put(dst, Value::Str(PhpStr::from(out)));
        Ok(())
    }

    fn op_unary(&mut self, op: Un, dst: Reg, a: Reg) -> R<()> {
        if let (Un::Not, Some(Value::Bool(b))) = (op, self.peek(a)) {
            let r = !*b;
            self.drop_tmp(a);
            self.put_bool(dst, r);
            return Ok(());
        }
        let v = self.get(a)?;
        let r = match op {
            Un::Not => Value::Bool(!self.to_bool(&v)),
            Un::Bool => Value::Bool(self.to_bool(&v)),
            Un::Neg => {
                if let Value::Int(i) = v
                    && i != i64::MIN
                {
                    Value::Int(-i)
                } else if let Some(r) = self.overloaded_unary(crate::class::UnaryOp::Neg, &v) {
                    r?
                } else {
                    goro_runtime::ops::neg(&v, self).map_err(|e| self.php_error(e))?
                }
            }
            Un::Plus => {
                if matches!(v, Value::Object(_))
                    && let Some(r) =
                        self.overloaded_op(goro_runtime::ops::BinOp::Mul, &v, &Value::Int(1))
                {
                    r?
                } else {
                    goro_runtime::ops::plus(&v, self).map_err(|e| self.php_error(e))?
                }
            }
            Un::BitNot => {
                if let Some(r) = self.overloaded_unary(crate::class::UnaryOp::BitNot, &v) {
                    r?
                } else {
                    goro_runtime::ops::bit_not(&v, self).map_err(|e| self.php_error(e))?
                }
            }
        };
        self.check_deferred()?;
        self.put(dst, r);
        Ok(())
    }

    fn op_assign_op_var(&mut self, op: Bin, var: Reg, src: Reg, res: Reg) -> R<()> {
        let p = self.slot_ptr(var);
        // `$s .= str` appends in place.
        if op == Bin::Concat {
            if let Some(Value::Str(y)) = self.peek(src) {
                let y = y.clone();
                if let Some(Value::Str(s)) = unsafe { &mut *p } {
                    s.push_bytes(&y);
                    self.drop_tmp(src);
                    if res != NONE {
                        let v = self.get_quiet(var);
                        self.put(res, v);
                    }
                    return Ok(());
                }
            }
        } else if let (Some(x @ (Value::Int(_) | Value::Float(_))), Some(y)) =
            (unsafe { &*p }.as_ref(), self.peek(src))
            && let Some(r) = fast_arith(op, x, y)
        {
            self.drop_tmp(src);
            if res != NONE {
                self.put(res, r.clone());
            }
            // The old value is a number: nothing to drop.
            unsafe { std::ptr::write(p, Some(r)) };
            return Ok(());
        }
        if op == Bin::Concat
            && let (Some(Value::Str(s)), Some(Value::Int(n))) = (unsafe { &mut *p }, self.peek(src))
        {
            let mut buf = [0u8; 20];
            let d = goro_runtime::convert::int_digits(*n, &mut buf);
            s.push_bytes(d);
            if res != NONE {
                let v = self.get_quiet(var);
                self.put(res, v);
            }
            return Ok(());
        }
        let cur = self.get(var)?;
        let y = self.get(src)?;
        let r = if op == Bin::Concat {
            let mut out = Vec::new();
            self.append_to_string(&mut out, &cur)?;
            self.append_to_string(&mut out, &y)?;
            Value::Str(PhpStr::from(out))
        } else {
            self.binary_op(op, &cur, &y)?
        };
        drop(cur);
        if res != NONE {
            self.put(res, r.clone());
        }
        self.assign_var(var, r)?;
        Ok(())
    }

    fn op_incdec_var(&mut self, op: IncDec, var: Reg, res: Reg) -> R<()> {
        let p = self.slot_ptr(var);
        if let Some(Value::Int(x)) = unsafe { &mut *p } {
            let old = *x;
            let new = if op.is_inc() {
                old.checked_add(1)
            } else {
                old.checked_sub(1)
            };
            if let Some(n) = new {
                *x = n;
                if res != NONE {
                    let v = if op.is_post() { old } else { n };
                    self.put_int(res, v);
                }
                return Ok(());
            }
        }
        let cur = match unsafe { &*p } {
            None => {
                self.undefined_var(var)?;
                self.get_quiet(var)
            }
            Some(v) => v.deref(),
        };
        let new = self.incdec_value(op, &cur)?;
        // An int typed reference can't overflow into a float.
        if let (Value::Int(_), Value::Float(_)) = (&cur, &new)
            && let Some(Value::Ref(rf)) = unsafe { &*p }
            && rf.is_typed()
        {
            let rf = rf.clone();
            let props = self.typed_ref_props(&rf);
            if let Some(pi) = props
                .iter()
                .find(|pi| pi.ty.as_ref().is_some_and(|t| t.mask & tmask::FLOAT == 0))
            {
                let t = pi.ty.as_ref().unwrap();
                return self.type_error(&format!(
                    "Cannot {} a reference held by property {}::${} of type {} past its {} value",
                    if op.is_inc() {
                        "increment"
                    } else {
                        "decrement"
                    },
                    unsafe { crate::class::DisplayName::dname(&(*pi.class).name) },
                    crate::class::DisplayName::dname(&pi.name),
                    t.display(),
                    if op.is_inc() { "maximal" } else { "minimal" }
                ));
            }
        }
        if res != NONE {
            let v = if op.is_post() {
                cur.clone()
            } else {
                new.clone()
            };
            self.put(res, v);
        }
        self.assign_var(var, new)?;
        Ok(())
    }

    /// `++`/`--` on a value.
    pub(crate) fn incdec_value(&mut self, op: IncDec, v: &Value) -> R<Value> {
        if matches!(v.deref(), Value::Object(_)) {
            let u = if op.is_inc() {
                crate::class::UnaryOp::Inc
            } else {
                crate::class::UnaryOp::Dec
            };
            if let Some(r) = self.overloaded_unary(u, v) {
                return r;
            }
        }
        let r = if op.is_inc() {
            goro_runtime::ops::increment(v, self)
        } else {
            goro_runtime::ops::decrement(v, self)
        };
        let r = r.map_err(|e| self.php_error(e))?;
        self.check_deferred()?;
        Ok(r)
    }

    // ------------------------------------------------------------ variables

    /// `$var = v` (through references).
    #[inline]
    pub(crate) fn assign_var(&mut self, var: Reg, v: Value) -> R<()> {
        let p = self.slot_ptr(var);
        unsafe {
            match &mut *p {
                Some(Value::Ref(rf)) => {
                    let v = if rf.is_typed() {
                        let rf = rf.clone();
                        self.typed_ref_value(&rf, v)?
                    } else {
                        v
                    };
                    let p = self.slot_ptr(var);
                    if let Some(Value::Ref(rf)) = &*p {
                        let old = rf.set(v);
                        drop(old);
                    }
                }
                slot @ Some(_) => {
                    let old = slot.replace(v);
                    drop(old);
                }
                slot @ None => {
                    if self.frame().flags & frame_flags::ATTACHED != 0 {
                        self.put_attached(var, v);
                    } else {
                        *slot = Some(v);
                    }
                }
            }
        }
        Ok(())
    }

    /// Makes the variable a reference, returning the cell.
    pub(crate) fn var_make_ref(&mut self, var: Reg) -> R<goro_runtime::PhpRef> {
        let p = self.slot_ptr(var);
        unsafe {
            if (*p).is_none() && self.frame().flags & frame_flags::ATTACHED != 0 {
                self.attached_bind_or_create(var);
            }
            let slot = &mut *p;
            match slot {
                Some(v) => Ok(v.make_ref()),
                None => {
                    let r = goro_runtime::PhpRef::new(Value::Null);
                    *slot = Some(Value::Ref(r.clone()));
                    Ok(r)
                }
            }
        }
    }

    fn assign_ref_vars(&mut self, dst: Reg, src: Reg) -> R<()> {
        let r = self.var_make_ref(src)?;
        self.bind_ref_var(dst, Value::Ref(r))
    }

    /// Binds variable `dst` to the reference in `v` (or a fresh one).
    pub(crate) fn bind_ref_var(&mut self, dst: Reg, v: Value) -> R<()> {
        let r = match v {
            Value::Ref(r) => r,
            other => goro_runtime::PhpRef::new(other),
        };
        if self.frame().flags & frame_flags::ATTACHED != 0 {
            self.attached_bind_ref(dst, &r);
        }
        let p = self.slot_ptr(dst);
        let old = unsafe { (*p).replace(Value::Ref(r)) };
        drop(old);
        Ok(())
    }

    pub(crate) fn unset_var(&mut self, var: Reg) {
        if self.frame().flags & frame_flags::ATTACHED != 0 {
            self.attached_unset(var);
        }
        let p = self.slot_ptr(var);
        let old = unsafe { (*p).take() };
        drop(old);
    }

    pub(crate) fn special(&mut self, what: Special) -> R<Value> {
        Ok(match what {
            Special::This => match self.this_obj() {
                Some(o) => Value::Object(o),
                None => {
                    return Err(
                        self.throw_error_msg("Error", "Using $this when not in object context")
                    );
                }
            },
            Special::ThisQuiet => match self.this_obj() {
                Some(o) => Value::Object(o),
                None => Value::Null,
            },
            Special::StaticClass | Special::CalledClass => {
                let p = self.static_class_ptr();
                if p.is_null() {
                    if what == Special::CalledClass {
                        return Err(self.throw_error_msg(
                            "Error",
                            "get_called_class() must be called from within a class",
                        ));
                    }
                    return Err(self.throw_error_msg(
                        "Error",
                        "Cannot use \"static\" when no class scope is active",
                    ));
                }
                Value::Str(unsafe { (*p).name.clone() })
            }
            Special::SelfClass | Special::TraitClass => {
                let p = self.scope_ptr();
                if p.is_null() {
                    Value::Str(PhpStr::empty())
                } else {
                    Value::Str(unsafe { (*p).name.clone() })
                }
            }
            Special::ParentClass => {
                let p = self.scope_ptr();
                match (!p.is_null())
                    .then(|| unsafe { (*p).parent.clone() })
                    .flatten()
                {
                    Some(c) => Value::Str(c.name.clone()),
                    None => Value::Bool(false),
                }
            }
            Special::FuncGetArgs => Value::Array(self.func_get_args()),
            Special::FuncNumArgs => Value::Int(self.frame().argc as i64),
            Special::Globals => Value::Array(self.globals_array()),
        })
    }

    pub(crate) fn func_get_args(&self) -> PhpArray {
        let f = self.frame();
        let code = f.code();
        let np = code.params.len();
        let mut a = PhpArray::new();
        let n = f.argc as usize;
        for i in 0..n.min(np) {
            let v = unsafe { (*f.base.add(i)).clone() }.unwrap_or_default();
            let _ = a.push(v.deref());
        }
        for v in &f.extra_args {
            let _ = a.push(v.deref());
        }
        a
    }

    pub(crate) fn run_pending_destructors(&mut self) -> R<()> {
        while let Some(o) = goro_runtime::object::pop_pending_destructor() {
            self.call_destructor(o)?;
        }
        Ok(())
    }

    /// Pops frames down to (and including) depth `to` (exclusive index:
    /// frames[to..] are removed), cleaning them up.
    pub(crate) fn unwind_frames(&mut self, to: usize) {
        while self.ex.frames.len() > to {
            self.pop_frame();
        }
    }
}
