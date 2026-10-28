//! `foreach` and generators (PHP manual: foreach, Generators, Iterator,
//! IteratorAggregate).

use crate::class::{DisplayName, TypeName};
use std::rc::Rc;

use goro_runtime::{Key, KeyRef, ObjRef, PhpArray, PhpRef, Value};

use super::{Frame, Pending, R, Slot, Vm, frame_flags};
use crate::bytecode::*;

const MODE_SHIFT: u32 = 56;
const M_ARRAY: i64 = 0;
const M_ARRAY_REF: i64 = 1;
const M_ITER_FIRST: i64 = 2;
const M_ITER: i64 = 3;
const M_GEN_FIRST: i64 = 4;
const M_GEN: i64 = 5;
const M_PROPS_REF: i64 = 6;
/// Plain object by value: [object, snapshot of its visible properties]
/// (the object stays alive while iterated).
const M_PROPS_VAL: i64 = 7;

#[inline]
fn enc(mode: i64, data: i64) -> Value {
    Value::Int((mode << MODE_SHIFT) | (data & ((1 << MODE_SHIFT) - 1)))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenState {
    Created,
    Suspended,
    Running,
    Done,
}

pub enum Delegate {
    Array { arr: PhpArray, pos: u32 },
    Gen(ObjRef),
    Iter(ObjRef),
}

/// Native data of Generator objects.
pub struct GenData {
    pub frame: Option<Frame>,
    pub slots: Box<[Slot]>,
    pub state: GenState,
    pub key: Value,
    pub value: Value,
    pub ret: Option<Value>,
    pub auto_key: i64,
    pub delegate: Option<Delegate>,
    /// The delegate was just set (its first element not fetched yet).
    pub delegate_fresh: bool,
    pub yield_dst: Reg,
    /// Pending calls open across a yield (`f(yield $x)`), with the values
    /// of their reserved slots.
    pub saved_pending: Vec<(Pending, Vec<Slot>)>,
    /// Advanced past the first yield (rewind no longer allowed).
    pub advanced: bool,
}

impl goro_runtime::NativeData for GenData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        for v in self.slots.iter().flatten() {
            visit(v);
        }
        visit(&self.key);
        visit(&self.value);
        if let Some(r) = &self.ret {
            visit(r);
        }
        if let Some(f) = &self.frame {
            if let Some(t) = &f.this {
                visit(&Value::Object(t.clone()));
            }
            if let Some(c) = &f.closure {
                visit(&Value::Object(c.clone()));
            }
            for v in &f.extra_args {
                visit(v);
            }
        }
        match &self.delegate {
            Some(Delegate::Array { arr, .. }) => visit(&Value::Array(arr.clone())),
            Some(Delegate::Gen(o)) | Some(Delegate::Iter(o)) => visit(&Value::Object(o.clone())),
            None => {}
        }
    }
    fn clear(&mut self) {
        for s in self.slots.iter_mut() {
            *s = None;
        }
        self.frame = None;
        self.delegate = None;
        self.key = Value::Null;
        self.value = Value::Null;
        self.ret = None;
        self.state = GenState::Done;
    }
}

impl Vm {
    // ------------------------------------------------------------ foreach

    fn iter_slots(&self, iter: Reg) -> (*mut Slot, *mut Slot) {
        let a = self.slot_ptr(iter);
        let b = self.slot_ptr(iter + 1);
        (a, b)
    }

    /// Prepares iteration; false when there is nothing to iterate.
    pub(crate) fn fe_reset(&mut self, iter: Reg, src: Reg, by_ref: bool) -> R<bool> {
        let raw = if by_ref {
            self.get_raw(src)
        } else {
            self.get(src)?
        };
        let (sa, sb) = self.iter_slots(iter);
        let v = raw.deref();
        match v {
            Value::Array(a) => {
                if by_ref {
                    let r = match raw {
                        Value::Ref(r) => r,
                        other => PhpRef::new(other),
                    };
                    let id = PhpArray::new_iter_id();
                    {
                        let mut b = r.borrow_mut();
                        if let Value::Array(arr) = &mut *b {
                            let d = arr.make_mut();
                            d.set_iter_pos(id, 0);
                        }
                    }
                    unsafe {
                        *sa = Some(Value::Ref(r));
                        *sb = Some(enc(M_ARRAY_REF, id as i64));
                    }
                    return Ok(true);
                }
                if a.is_empty() {
                    return Ok(false);
                }
                unsafe {
                    *sa = Some(Value::Array(a));
                    *sb = Some(enc(M_ARRAY, 0));
                }
                Ok(true)
            }
            Value::Object(o) => {
                let cls = self.class_of(&o);
                if self.is_generator_obj(&o) {
                    if by_ref && !self.gen_by_ref(&o) {
                        return self.throw("Exception", "You can only iterate a generator by-reference if it declared that it yields by-reference");
                    }
                    self.gen_rewind(&o)?;
                    unsafe {
                        *sa = Some(Value::Object(o));
                        *sb = Some(enc(M_GEN_FIRST, 0));
                    }
                    return Ok(true);
                }
                if self.is_traversable(&cls) {
                    if by_ref {
                        return self.throw(
                            "Error",
                            "An iterator cannot be used with foreach by reference",
                        );
                    }
                    let it = self.get_iterator_of(&o)?;
                    if self.is_generator_obj(&it) {
                        self.gen_rewind(&it)?;
                        unsafe {
                            *sa = Some(Value::Object(it));
                            *sb = Some(enc(M_GEN_FIRST, 0));
                        }
                        return Ok(true);
                    }
                    self.call_method(&it, b"rewind", Vec::new())?;
                    unsafe {
                        *sa = Some(Value::Object(it));
                        *sb = Some(enc(M_ITER_FIRST, 0));
                    }
                    return Ok(true);
                }
                // Plain object: its visible properties.
                if by_ref {
                    let names: Vec<Value> =
                        self.visible_props(&o).into_iter().map(|(k, _)| k).collect();
                    self.check_deferred()?;
                    if names.is_empty() {
                        return Ok(false);
                    }
                    let pack = PhpArray::from_values([
                        Value::Object(o),
                        Value::Array(PhpArray::from_values(names)),
                    ]);
                    unsafe {
                        *sa = Some(Value::Array(pack));
                        *sb = Some(enc(M_PROPS_REF, 0));
                    }
                    return Ok(true);
                }
                let props = self.visible_props(&o);
                self.check_deferred()?;
                if props.is_empty() {
                    return Ok(false);
                }
                let mut arr = PhpArray::new();
                for (k, v) in props {
                    let key = match k {
                        Value::Int(i) => Key::Int(i),
                        Value::Str(s) => Key::Str(s),
                        _ => continue,
                    };
                    arr.insert(key, v.deref());
                }
                let pack = PhpArray::from_values([Value::Object(o), Value::Array(arr)]);
                unsafe {
                    *sa = Some(Value::Array(pack));
                    *sb = Some(enc(M_PROPS_VAL, 0));
                }
                Ok(true)
            }
            other => {
                self.warn(&format!(
                    "foreach() argument must be of type array|object, {} given",
                    other.tname()
                ))?;
                Ok(false)
            }
        }
    }

    /// The Iterator of a Traversable (following IteratorAggregate).
    pub fn get_iterator_of(&mut self, o: &ObjRef) -> R<ObjRef> {
        let mut cur = o.clone();
        for _ in 0..32 {
            let cls = self.class_of(&cur);
            if let Some(h) = cls.native.get_iterator {
                cur = h(self, &cur, false)?;
                continue;
            }
            let iter_cls = self.core.iterator.clone().unwrap();
            if cls.is_subclass_of(&iter_cls) {
                return Ok(cur);
            }
            let agg = self.core.iterator_aggregate.clone().unwrap();
            if cls.is_subclass_of(&agg) {
                let r = self.call_method(&cur, b"getiterator", Vec::new())?;
                match r {
                    Value::Object(n) if self.is_traversable(&self.class_of(&n)) => {
                        cur = n;
                        continue;
                    }
                    _ => {
                        return self.throw(
                            "Exception",
                            &format!(
                                "Objects returned by {}::getIterator() must be traversable or implement interface Iterator",
                                cls.name.dname()
                            ),
                        );
                    }
                }
            }
            return Ok(cur);
        }
        Ok(cur)
    }

    /// Next element; false at the end.
    pub(crate) fn fe_fetch(&mut self, iter: Reg, val: Reg, key: Reg) -> R<bool> {
        let (sa, sb) = self.iter_slots(iter);
        let Some(Value::Int(st)) = (unsafe { (*sb).clone() }) else {
            return Ok(false);
        };
        let mode = st >> MODE_SHIFT;
        let data = st & ((1 << MODE_SHIFT) - 1);
        match mode {
            M_ARRAY => {
                let Some(Value::Array(a)) = (unsafe { &*sa }) else {
                    return Ok(false);
                };
                let Some((p, k, v)) = a.data().next_from(data as u32) else {
                    return Ok(false);
                };
                let k = k.to_value();
                let v = v.deref();
                unsafe { *sb = Some(enc(M_ARRAY, p as i64 + 1)) };
                self.put(val, v);
                if key != NONE {
                    self.put(key, k);
                }
                Ok(true)
            }
            M_PROPS_VAL => {
                let Some(Value::Array(pack)) = (unsafe { &*sa }) else {
                    return Ok(false);
                };
                let Some(Value::Array(a)) = pack.get_int(1) else {
                    return Ok(false);
                };
                let Some((p, k, v)) = a.data().next_from(data as u32) else {
                    return Ok(false);
                };
                let k = k.to_value();
                let v = v.deref();
                unsafe { *sb = Some(enc(M_PROPS_VAL, p as i64 + 1)) };
                self.put(val, v);
                if key != NONE {
                    self.put(key, k);
                }
                Ok(true)
            }
            M_ARRAY_REF => {
                let Some(Value::Ref(r)) = (unsafe { (*sa).clone() }) else {
                    return Ok(false);
                };
                let next = {
                    let mut b = r.borrow_mut();
                    match &mut *b {
                        Value::Array(a) => a.make_mut().iter_next_ref(data as u32),
                        _ => None,
                    }
                };
                let Some((k, er)) = next else {
                    return Ok(false);
                };
                let p = self.slot_ptr(val);
                let old = unsafe { (*p).replace(Value::Ref(er)) };
                drop(old);
                if key != NONE {
                    self.put(key, k.to_value());
                }
                Ok(true)
            }
            M_ITER_FIRST | M_ITER => {
                let Some(Value::Object(it)) = (unsafe { (*sa).clone() }) else {
                    return Ok(false);
                };
                if mode == M_ITER {
                    self.call_method(&it, b"next", Vec::new())?;
                }
                unsafe { *sb = Some(enc(M_ITER, 0)) };
                let valid = self.call_method(&it, b"valid", Vec::new())?;
                if !self.to_bool(&valid) {
                    return Ok(false);
                }
                let v = self.call_method(&it, b"current", Vec::new())?;
                self.put(val, v.into_deref());
                if key != NONE {
                    let k = self.call_method(&it, b"key", Vec::new())?;
                    self.put(key, k.into_deref());
                }
                Ok(true)
            }
            M_GEN_FIRST | M_GEN => {
                let Some(Value::Object(g)) = (unsafe { (*sa).clone() }) else {
                    return Ok(false);
                };
                if mode == M_GEN {
                    self.gen_next(&g)?;
                }
                unsafe { *sb = Some(enc(M_GEN, 0)) };
                if !self.gen_valid(&g)? {
                    return Ok(false);
                }
                let (k, v) = self.gen_current(&g)?;
                let byref = matches!(v, Value::Ref(_));
                if byref {
                    let p = self.slot_ptr(val);
                    let old = unsafe { (*p).replace(v) };
                    drop(old);
                } else {
                    self.put(val, v);
                }
                if key != NONE {
                    self.put(key, k);
                }
                Ok(true)
            }
            M_PROPS_REF => {
                let Some(Value::Array(pack)) = (unsafe { (*sa).clone() }) else {
                    return Ok(false);
                };
                let o = match pack.get_int(0) {
                    Some(Value::Object(o)) => o.clone(),
                    _ => return Ok(false),
                };
                let Some(Value::Array(names)) = pack.get_int(1).cloned() else {
                    return Ok(false);
                };
                let mut pos = data as u32;
                loop {
                    let Some((p, _, n)) = names.data().next_from(pos) else {
                        return Ok(false);
                    };
                    pos = p + 1;
                    let name = match n {
                        Value::Str(s) => s.clone(),
                        Value::Int(i) => goro_runtime::PhpStr::from(i.to_string()),
                        _ => continue,
                    };
                    unsafe { *sb = Some(enc(M_PROPS_REF, pos as i64)) };
                    let r = self.prop_action(&o, &name, 0, PathAction::MakeRef, None)?;
                    let p = self.slot_ptr(val);
                    let old = unsafe { (*p).replace(r) };
                    drop(old);
                    if key != NONE {
                        self.put(key, n.clone());
                    }
                    return Ok(true);
                }
            }
            _ => Ok(false),
        }
    }

    pub(crate) fn fe_free(&mut self, iter: Reg) -> R<()> {
        let (sa, sb) = self.iter_slots(iter);
        if let Some(Value::Int(st)) = unsafe { (*sb).clone() }
            && st >> MODE_SHIFT == M_ARRAY_REF
            && let Some(Value::Ref(r)) = unsafe { (*sa).clone() }
        {
            let id = (st & ((1 << MODE_SHIFT) - 1)) as u32;
            let mut b = r.borrow_mut();
            if let Value::Array(a) = &mut *b
                && a.refcount() > 0
            {
                a.make_mut().iter_release(id);
            }
        }
        let a = unsafe { (*sa).take() };
        let b = unsafe { (*sb).take() };
        drop(a);
        drop(b);
        Ok(())
    }

    /// All (key, value) pairs of an iterable (iterator_to_array, unpacking).
    pub fn iterate_to_vec(&mut self, v: &Value, with_keys: bool) -> R<Vec<(Value, Value)>> {
        let _ = with_keys;
        match v {
            Value::Array(a) => Ok(a.iter().map(|(k, v)| (k.to_value(), v.deref())).collect()),
            Value::Object(o) => {
                let mut out = Vec::new();
                if self.is_generator_obj(o) {
                    self.gen_rewind(o)?;
                    while self.gen_valid(o)? {
                        let (k, v) = self.gen_current(o)?;
                        out.push((k, v.into_deref()));
                        self.gen_next(o)?;
                    }
                    return Ok(out);
                }
                let it = self.get_iterator_of(o)?;
                if self.is_generator_obj(&it) {
                    return self.iterate_to_vec(&Value::Object(it), with_keys);
                }
                self.call_method(&it, b"rewind", Vec::new())?;
                loop {
                    let valid = self.call_method(&it, b"valid", Vec::new())?;
                    if !self.to_bool(&valid) {
                        break;
                    }
                    let v = self.call_method(&it, b"current", Vec::new())?;
                    let k = if with_keys {
                        self.call_method(&it, b"key", Vec::new())?
                    } else {
                        Value::Null
                    };
                    out.push((k.into_deref(), v.into_deref()));
                    self.call_method(&it, b"next", Vec::new())?;
                }
                Ok(out)
            }
            _ => Ok(Vec::new()),
        }
    }

    // ------------------------------------------------------------ generators

    pub fn is_generator_obj(&self, o: &ObjRef) -> bool {
        let n = o.obj().native.borrow();
        match n.as_ref() {
            Some(n) => {
                let any: &dyn std::any::Any = &**n;
                any.is::<GenData>()
            }
            None => false,
        }
    }

    fn gen_by_ref(&self, o: &ObjRef) -> bool {
        self.with_gen(o, |g| {
            g.frame
                .as_ref()
                .is_some_and(|f| f.code().flags & fflags::BY_REF != 0)
        })
        .unwrap_or(false)
    }

    pub(crate) fn with_gen<T>(&self, o: &ObjRef, f: impl FnOnce(&mut GenData) -> T) -> Option<T> {
        let mut n = o.obj().native.borrow_mut();
        let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
        let g = any.downcast_mut::<GenData>()?;
        Some(f(g))
    }

    /// Turns an activated generator function frame into a Generator.
    pub(crate) fn create_generator(&mut self, mut frame: Frame) -> R<ObjRef> {
        let n = frame.code().num_slots as usize;
        let mut slots: Box<[Slot]> = (0..n).map(|_| None).collect();
        for (i, s) in slots.iter_mut().enumerate() {
            *s = unsafe { (*frame.base.add(i)).take() };
        }
        self.ex.stack.release(frame.mark);
        frame.base = slots.as_mut_ptr();
        frame.flags =
            (frame.flags | frame_flags::GENERATOR | frame_flags::OWN_SLOTS | frame_flags::ENTRY)
                & !super::calls::RET_VAR;
        frame.ret = std::ptr::null_mut();
        frame.ip = 0;
        let cls = self.core.generator.clone().expect("Generator class");
        let o = ObjRef::new(cls, Vec::new());
        let data = GenData {
            frame: Some(frame),
            slots,
            state: GenState::Created,
            key: Value::Null,
            value: Value::Null,
            ret: None,
            auto_key: 0,
            delegate: None,
            delegate_fresh: false,
            yield_dst: NONE,
            saved_pending: Vec::new(),
            advanced: false,
        };
        *o.obj().native.borrow_mut() = Some(Box::new(data));
        Ok(o)
    }

    /// Resumes a generator (sending `send` to the paused yield, or throwing
    /// `throw` there) until it yields again or finishes.
    pub fn gen_resume(
        &mut self,
        g: &ObjRef,
        mut send: Option<Value>,
        mut throw: Option<ObjRef>,
    ) -> R<()> {
        loop {
            let state = self.with_gen(g, |d| d.state).unwrap();
            match state {
                GenState::Done => {
                    if let Some(e) = throw {
                        return Err(super::Unwind::Throw(e));
                    }
                    return Ok(());
                }
                GenState::Running => {
                    return self.throw("Error", "Cannot resume an already running generator");
                }
                _ => {}
            }
            // Delegation (`yield from`).
            let has_delegate = self.with_gen(g, |d| d.delegate.is_some()).unwrap();
            if has_delegate {
                let fresh = self
                    .with_gen(g, |d| std::mem::replace(&mut d.delegate_fresh, false))
                    .unwrap();
                // Running while its delegate runs (it cannot be resumed or
                // force-closed meanwhile).
                self.with_gen(g, |d| d.state = GenState::Running);
                let r = self.delegate_step(g, fresh, send.take(), throw.take());
                self.with_gen(g, |d| {
                    if d.state == GenState::Running {
                        d.state = GenState::Suspended;
                    }
                });
                match r? {
                    true => return Ok(()),
                    false => {
                        // Delegate finished: its result went to yield_dst.
                    }
                }
            }
            // Resume the frame.
            let (frame, saved, dst) = self
                .with_gen(g, |d| {
                    d.state = GenState::Running;
                    (
                        d.frame.take(),
                        std::mem::take(&mut d.saved_pending),
                        d.yield_dst,
                    )
                })
                .unwrap();
            let Some(mut frame) = frame else {
                self.with_gen(g, |d| d.state = GenState::Done);
                return Ok(());
            };
            if dst != NONE {
                let v = send.take().unwrap_or_default();
                unsafe {
                    let p = frame.base.add(slot_of(dst));
                    let old = (*p).replace(v);
                    drop(old);
                }
            }
            frame.generator = Some(g.clone());
            frame.pending_base = self.ex.pending.len() as u32;
            frame.native_base = self.ex.native_calls.len() as u32;
            let ip = frame.ip as usize;
            self.push_frame(frame);
            self.ex.ip = ip;
            // Restore calls that were open across the yield.
            for (mut p, vals) in saved {
                if !vals.is_empty() {
                    let (b, m) = self.ex.stack.alloc(vals.len());
                    for (i, v) in vals.into_iter().enumerate() {
                        unsafe { *b.add(i) = v };
                    }
                    p.base = b;
                    p.mark = m;
                }
                self.ex.pending.push(p);
            }
            let entry = self.ex.frames.len();
            let r = if let Some(e) = throw.take() {
                // Thrown at the paused yield.
                match self.dispatch_exception(e, entry) {
                    Ok(()) => self.run_entered(entry),
                    Err(e) => Err(e),
                }
            } else {
                self.run_entered(entry)
            };
            match r {
                Ok(_) => {
                    let again = self
                        .with_gen(g, |d| {
                            d.state == GenState::Suspended
                                && d.delegate.is_some()
                                && d.delegate_fresh
                        })
                        .unwrap();
                    if again {
                        continue;
                    }
                    return Ok(());
                }
                Err(e) => {
                    self.with_gen(g, |d| {
                        d.state = GenState::Done;
                        d.frame = None;
                        for s in d.slots.iter_mut() {
                            *s = None;
                        }
                        d.value = Value::Null;
                        d.key = Value::Null;
                    });
                    return Err(e);
                }
            }
        }
    }

    /// Runs the already-pushed entry frame at depth `entry`.
    fn run_entered(&mut self, entry: usize) -> R<Value> {
        let _ = entry;
        self.run()
    }

    /// One step of a `yield from` delegate. True if it produced a value.
    fn delegate_step(
        &mut self,
        g: &ObjRef,
        fresh: bool,
        send: Option<Value>,
        throw: Option<ObjRef>,
    ) -> R<bool> {
        let d = self.with_gen(g, |d| d.delegate.take()).unwrap();
        let Some(d) = d else { return Ok(false) };
        match d {
            Delegate::Array { arr, pos } => {
                if let Some(e) = throw {
                    self.with_gen(g, |d| d.delegate = None);
                    return self.gen_throw_at_frame(g, e);
                }
                let start = if fresh { 0 } else { pos + 1 };
                match arr.data().next_from(start) {
                    Some((p, k, v)) => {
                        let (k, v) = (k.to_value(), v.deref());
                        self.with_gen(g, |d| {
                            d.key = k;
                            d.value = v;
                            d.delegate = Some(Delegate::Array {
                                arr: arr.clone(),
                                pos: p,
                            });
                            d.state = GenState::Suspended;
                        });
                        Ok(true)
                    }
                    None => {
                        self.delegate_done(g, Value::Null);
                        Ok(false)
                    }
                }
            }
            Delegate::Gen(inner) => {
                let r = if fresh {
                    self.gen_ensure_started(&inner)
                } else if let Some(e) = throw {
                    self.gen_resume(&inner, None, Some(e))
                } else {
                    self.gen_resume(&inner, Some(send.unwrap_or_default()), None)
                };
                if let Err(e) = r {
                    self.with_gen(g, |d| d.delegate = None);
                    if let super::Unwind::Throw(ex) = e {
                        return self.gen_throw_at_frame(g, ex);
                    }
                    return Err(e);
                }
                let (st, k, v, ret) = self
                    .with_gen(&inner, |d| {
                        (d.state, d.key.clone(), d.value.clone(), d.ret.clone())
                    })
                    .unwrap();
                if st == GenState::Done {
                    self.delegate_done(g, ret.unwrap_or_default());
                    return Ok(false);
                }
                self.with_gen(g, |d| {
                    d.key = k;
                    d.value = v;
                    d.delegate = Some(Delegate::Gen(inner.clone()));
                    d.state = GenState::Suspended;
                });
                Ok(true)
            }
            Delegate::Iter(it) => {
                if let Some(e) = throw {
                    return self.gen_throw_at_frame(g, e);
                }
                let r = (|| -> R<Option<(Value, Value)>> {
                    if fresh {
                        self.call_method(&it, b"rewind", Vec::new())?;
                    } else {
                        self.call_method(&it, b"next", Vec::new())?;
                    }
                    let valid = self.call_method(&it, b"valid", Vec::new())?;
                    if !self.to_bool(&valid) {
                        return Ok(None);
                    }
                    let v = self.call_method(&it, b"current", Vec::new())?;
                    let k = self.call_method(&it, b"key", Vec::new())?;
                    Ok(Some((k, v)))
                })();
                match r {
                    Ok(Some((k, v))) => {
                        self.with_gen(g, |d| {
                            d.key = k.into_deref();
                            d.value = v.into_deref();
                            d.delegate = Some(Delegate::Iter(it.clone()));
                            d.state = GenState::Suspended;
                        });
                        Ok(true)
                    }
                    Ok(None) => {
                        self.delegate_done(g, Value::Null);
                        Ok(false)
                    }
                    Err(super::Unwind::Throw(ex)) => self.gen_throw_at_frame(g, ex),
                    Err(e) => Err(e),
                }
            }
        }
    }

    /// An exception from a delegate continues in the delegating frame.
    fn gen_throw_at_frame(&mut self, g: &ObjRef, e: ObjRef) -> R<bool> {
        self.with_gen(g, |d| {
            d.delegate = None;
            d.yield_dst = NONE;
            if d.state == GenState::Running {
                d.state = GenState::Suspended;
            }
        });
        self.gen_resume(g, None, Some(e))?;
        Ok(true)
    }

    fn delegate_done(&mut self, g: &ObjRef, result: Value) {
        self.with_gen(g, |d| {
            d.delegate = None;
            if d.yield_dst != NONE
                && let Some(f) = &d.frame
            {
                unsafe {
                    let p = f.base.add(slot_of(d.yield_dst));
                    *p = Some(result);
                }
            }
            d.yield_dst = NONE;
        });
    }

    /// `yield`: suspends the generator frame.
    pub(crate) fn op_yield(&mut self, dst: Reg, val: Reg, key: Reg) -> R<Option<Value>> {
        let by_ref = self.code_ref().flags & fflags::BY_REF != 0;
        let v = if val == NONE {
            Value::Null
        } else if by_ref {
            match self.get_raw(val) {
                Value::Ref(r) => Value::Ref(r),
                other => other,
            }
        } else {
            self.get(val)?
        };
        let g = self
            .frame()
            .generator
            .clone()
            .expect("yield outside generator");
        let k = if key == NONE {
            let k = self.with_gen(&g, |d| {
                let k = d.auto_key;
                d.auto_key += 1;
                k
            });
            Value::Int(k.unwrap())
        } else {
            let k = self.get(key)?;
            if let Value::Int(i) = k {
                self.with_gen(&g, |d| {
                    if i >= d.auto_key {
                        d.auto_key = i + 1;
                    }
                });
            }
            k
        };
        self.suspend_generator(&g, dst, Some((k, v)));
        Ok(Some(Value::Null))
    }

    fn suspend_generator(&mut self, g: &ObjRef, dst: Reg, kv: Option<(Value, Value)>) {
        let ip = self.ex.ip;
        // Calls open across the yield.
        let pb = self.frame().pending_base as usize;
        let mut saved = Vec::new();
        while self.ex.pending.len() > pb {
            let mut p = self.ex.pending.remove(pb);
            let mut vals = Vec::new();
            if !p.base.is_null()
                && let Some(code) = p.func.code()
            {
                for i in 0..code.num_slots as usize {
                    vals.push(unsafe { (*p.base.add(i)).take() });
                }
                p.base = std::ptr::null_mut();
            }
            saved.push((p, vals));
        }
        // Release the stack of saved pendings (LIFO: lowest mark first).
        if let Some((p, _)) = saved.first()
            && !p.func.is_native()
        {
            self.ex.stack.release(p.mark);
        }
        let mut f = self.ex.frames.pop().unwrap();
        self.ex.sync_top();
        if let Some(top) = self.ex.frames.last() {
            self.ex.ip = top.ip as usize;
        }
        f.ip = ip as u32;
        f.generator = None;
        self.ex.native_calls.truncate(f.native_base as usize);
        self.with_gen(g, |d| {
            if let Some((k, v)) = kv {
                d.key = k;
                d.value = v;
            }
            d.yield_dst = dst;
            d.frame = Some(f);
            d.saved_pending = saved;
            d.state = GenState::Suspended;
        });
    }

    pub(crate) fn op_yield_from(&mut self, dst: Reg, src: Reg) -> R<Option<Value>> {
        let v = self.get(src)?;
        let g = self
            .frame()
            .generator
            .clone()
            .expect("yield from outside generator");
        let delegate = match v {
            Value::Array(a) => {
                if a.is_empty() {
                    self.put(dst, Value::Null);
                    return Ok(None);
                }
                Delegate::Array { arr: a, pos: 0 }
            }
            Value::Object(o) => {
                if self.is_generator_obj(&o) {
                    if ObjRef::ptr_eq(&o, &g) {
                        return self.throw(
                            "Error",
                            "Impossible to yield from the Generator being currently run",
                        );
                    }
                    let (st, ret) = self.with_gen(&o, |d| (d.state, d.ret.clone())).unwrap();
                    if st == GenState::Done {
                        if ret.is_none() {
                            return self.throw("Error", "Generator passed to yield from was aborted without proper return and is unable to continue");
                        }
                        self.put(dst, ret.unwrap_or_default());
                        return Ok(None);
                    }
                    if st == GenState::Running {
                        return self.throw(
                            "Error",
                            "Impossible to yield from the Generator being currently run",
                        );
                    }
                    Delegate::Gen(o)
                } else {
                    let cls = self.class_of(&o);
                    if !self.is_traversable(&cls) {
                        return self.throw(
                            "Error",
                            "Can use \"yield from\" only with arrays and Traversables",
                        );
                    }
                    let it = self.get_iterator_of(&o)?;
                    if self.is_generator_obj(&it) {
                        Delegate::Gen(it)
                    } else {
                        Delegate::Iter(it)
                    }
                }
            }
            _ => {
                return self.throw(
                    "Error",
                    "Can use \"yield from\" only with arrays and Traversables",
                );
            }
        };
        self.with_gen(&g, |d| {
            d.delegate = Some(delegate);
            d.delegate_fresh = true;
        });
        self.suspend_generator(&g, dst, None);
        Ok(Some(Value::Null))
    }

    /// `return` in a generator.
    pub(crate) fn generator_return(&mut self, v: Value) -> R<Option<Value>> {
        let g = self.frame().generator.clone().expect("generator frame");
        let f = self.ex.frames.pop().unwrap();
        self.ex.sync_top();
        if let Some(top) = self.ex.frames.last() {
            self.ex.ip = top.ip as usize;
        }
        while self.ex.pending.len() > f.pending_base as usize {
            let p = self.ex.pending.pop().unwrap();
            self.discard_pending(p);
        }
        drop(f);
        let slots = self.with_gen(&g, |d| {
            d.state = GenState::Done;
            d.ret = Some(v.into_deref());
            d.value = Value::Null;
            d.key = Value::Null;
            std::mem::take(&mut d.slots)
        });
        // Locals are released when the generator finishes.
        drop(slots);
        Ok(Some(Value::Null))
    }

    pub fn gen_ensure_started(&mut self, g: &ObjRef) -> R<()> {
        let st = self.with_gen(g, |d| d.state).unwrap();
        if st == GenState::Created {
            self.gen_resume(g, None, None)?;
        }
        Ok(())
    }

    pub fn gen_rewind(&mut self, g: &ObjRef) -> R<()> {
        self.gen_ensure_started(g)?;
        let adv = self.with_gen(g, |d| d.advanced).unwrap();
        if adv {
            return self.throw(
                "Exception",
                "Cannot rewind a generator that was already run",
            );
        }
        Ok(())
    }

    pub fn gen_valid(&mut self, g: &ObjRef) -> R<bool> {
        self.gen_ensure_started(g)?;
        Ok(self.with_gen(g, |d| d.state != GenState::Done).unwrap())
    }

    pub fn gen_current(&mut self, g: &ObjRef) -> R<(Value, Value)> {
        self.gen_ensure_started(g)?;
        Ok(self
            .with_gen(g, |d| {
                if d.state == GenState::Done {
                    (Value::Null, Value::Null)
                } else {
                    (d.key.clone(), d.value.clone())
                }
            })
            .unwrap())
    }

    pub fn gen_next(&mut self, g: &ObjRef) -> R<()> {
        let st = self.with_gen(g, |d| d.state).unwrap();
        if st == GenState::Created {
            self.gen_resume(g, None, None)?;
        }
        self.with_gen(g, |d| d.advanced = true);
        self.gen_resume(g, None, None)
    }

    pub fn gen_send(&mut self, g: &ObjRef, v: Value) -> R<Value> {
        let st = self.with_gen(g, |d| d.state).unwrap();
        if st == GenState::Created {
            self.gen_resume(g, None, None)?;
        }
        self.with_gen(g, |d| d.advanced = true);
        self.gen_resume(g, Some(v), None)?;
        let (_, v) = self.gen_current(g)?;
        Ok(v.into_deref())
    }

    pub fn gen_throw(&mut self, g: &ObjRef, e: ObjRef) -> R<Value> {
        let st = self.with_gen(g, |d| d.state).unwrap();
        if st == GenState::Created {
            // Thrown before the first yield: the generator is closed.
            self.with_gen(g, |d| {
                d.state = GenState::Done;
                d.frame = None;
            });
            return Err(super::Unwind::Throw(e));
        }
        self.with_gen(g, |d| d.advanced = true);
        self.gen_resume(g, None, Some(e))?;
        let (_, v) = self.gen_current(g)?;
        Ok(v.into_deref())
    }

    pub fn gen_return_value(&mut self, g: &ObjRef) -> R<Value> {
        let (st, ret) = self.with_gen(g, |d| (d.state, d.ret.clone())).unwrap();
        match (st, ret) {
            (GenState::Done, Some(v)) => Ok(v),
            (GenState::Done, None) => self.throw(
                "Exception",
                "Cannot get return value of a generator that hasn't returned",
            ),
            _ => self.throw(
                "Exception",
                "Cannot get return value of a generator that hasn't returned",
            ),
        }
    }

    pub(crate) fn rc_unused(_: Rc<()>) {}

    #[allow(dead_code)]
    fn key_ref_unused(_: KeyRef<'_>) {}
}
