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

use super::{CallTarget, MethodCallInfo, PendingCall, Vm};
use crate::object::{InternalNewTemplate, NewTemplate, PhpObject, Visibility};
use crate::opcode::{Op, OpArray, OperandType};
use crate::string::PhpString;
use crate::value::Value;

/// Outcome of Vm::new_object_fast().
pub(crate) enum NewFast {
    /// Not a plain user class: nothing was done.
    No,
    /// The new object, for the result operand, and whether its class is
    /// known to have no constructor (see NewTemplate::no_ctor).
    Obj(Value, bool),
    /// Evaluating the class's constant expressions threw.
    Failed,
}

/// What an INIT_METHOD_CALL site found for the class of the object it saw
/// last (see Vm::init_method_call_fast): classes do not change once
/// declared.
#[derive(Debug, Clone)]
pub(crate) struct MethodSite {
    /// The object's class name.
    class_name: Vec<u8>,
    /// The calling scope (class_scope_ref()) it was resolved from: its own
    /// private method of that name may win (zend_get_parent_private_method()).
    scope: Option<Vec<u8>>,
    /// The method (None: a constructor call of a class without one).
    method: Option<std::rc::Weak<OpArray>>,
    /// The fast path does not apply to the class: the general handler runs.
    fallback: bool,
    /// The pending call's name, `Class::method` (shared by the calls; like
    /// PHP's function names, not counted by memory_get_usage()).
    func_name: PhpString,
    /// The call's frame name and scope (for a method).
    info: Option<Rc<MethodCallInfo>>,
    /// Where "::" is in it.
    sep: usize,
    is_static: bool,
    /// `user_functions.len() + 1` when the method was last seen registered
    /// under func_name (0: not checked).
    reg_gen: usize,
    /// What the site found for the other classes (and scopes) it met last
    /// (the first one met again is moved here, see take_method_site()).
    others: Vec<MethodSite>,
}

/// How many classes a method call site remembers besides the last one.
const METHOD_SITE_OTHERS: usize = 3;

/// What the INIT_METHOD_CALL sites resolved, for any site, by object class,
/// method name (as the site spells it) and calling scope: the answer of a
/// site meeting more classes than it remembers (a visitor loop calling
/// `enterNode()` on each of many classes), see init_method_call_fast().
/// Like the sites' answers, kept for the request (SITE_EPOCH).
#[derive(Default)]
struct MethodMemo {
    epoch: u32,
    /// By memo_slot(): the method name and the site's answer.
    slots: Vec<Option<Box<(Box<[u8]>, MethodSite)>>>,
}

/// log2 of the number of MethodMemo slots.
const METHOD_MEMO_BITS: u32 = 10;

thread_local! {
    static METHOD_MEMO: RefCell<MethodMemo> = RefCell::new(MethodMemo::default());
}

#[inline]
fn memo_slot(class_name: &[u8], method: &[u8], scope: Option<&[u8]>) -> usize {
    use std::hash::Hasher;
    let mut h = crate::fxhash::FxHasher::default();
    h.write(class_name);
    h.write_u8(0xff);
    h.write(method);
    if let Some(s) = scope {
        h.write_u8(0xfe);
        h.write(s);
    }
    (h.finish().wrapping_mul(0x9e37_79b9_7f4a_7c15) >> (64 - METHOD_MEMO_BITS)) as usize
}

impl MethodMemo {
    /// The answer for `class_name`, `method` and `scope`.
    #[inline]
    fn get(&self, class_name: &[u8], method: &[u8], scope: Option<&[u8]>) -> Option<&MethodSite> {
        if self.epoch != crate::opcode::site_epoch() {
            return None;
        }
        let e = self
            .slots
            .get(memo_slot(class_name, method, scope))?
            .as_deref()?;
        (crate::string::bytes_eq(&e.0, method) && e.1.is_for(class_name, scope)).then_some(&e.1)
    }

    /// Remember a site's answer (without its others) for `method`.
    #[inline(never)]
    fn put(&mut self, method: &[u8], site: &MethodSite) {
        let epoch = crate::opcode::site_epoch();
        if self.epoch != epoch || self.slots.is_empty() {
            self.slots.clear();
            self.slots.resize_with(1 << METHOD_MEMO_BITS, || None);
            self.epoch = epoch;
        }
        let i = memo_slot(&site.class_name, method, site.scope.as_deref());
        let mut s = site.clone();
        s.others = Vec::new();
        s.reg_gen = 0;
        self.slots[i] = Some(Box::new((method.into(), s)));
    }
}

impl MethodSite {
    #[inline]
    fn is_for(&self, class_name: &[u8], scope: Option<&[u8]>) -> bool {
        crate::string::bytes_eq(&self.class_name, class_name)
            && match (self.scope.as_deref(), scope) {
                (None, None) => true,
                (Some(a), Some(b)) => crate::string::bytes_eq(a, b),
                _ => false,
            }
    }
}

/// Remember `site` for the call at `ip`: the site's previous answers (for
/// other classes or scopes) are kept among its others.
fn store_method_site(op_array: &OpArray, ip: usize, site: MethodSite) {
    // (for the other sites calling the method by that name, see MethodMemo)
    if let Some(OperandType::Const(c)) = op_array.ops.get(ip).map(|o| o.op2) {
        if let Some(Value::String(m)) = op_array.literals.get(c as usize) {
            METHOD_MEMO.with(|mm| {
                if let Ok(mut mm) = mm.try_borrow_mut() {
                    mm.put(m.as_bytes(), &site);
                }
            });
        }
    }
    let mut sites = op_array.sites().borrow_mut();
    let map = &mut sites.get_or_insert_with(Default::default).method_sites;
    let Some(mut old) = map.insert(ip as u32, site) else {
        return;
    };
    let Some(new) = map.get_mut(&(ip as u32)) else {
        return;
    };
    let mut others = std::mem::take(&mut old.others);
    others.insert(0, old);
    others.retain(|o| !o.is_for(&new.class_name, new.scope.as_deref()));
    others.truncate(METHOD_SITE_OTHERS);
    new.others = others;
}

/// The site's answer for objects of `class_name` called from `scope`
/// among its others, made its current one (see MethodSite::others).
#[inline(never)]
fn other_method_site<'a>(
    s: &'a mut MethodSite,
    class_name: &[u8],
    scope: Option<&[u8]>,
) -> Option<&'a MethodSite> {
    let i = s.others.iter().position(|o| o.is_for(class_name, scope))?;
    let mut hit = s.others.remove(i);
    std::mem::swap(&mut hit.others, &mut s.others);
    std::mem::swap(s, &mut hit);
    // (`hit` is now the previous current answer, without others)
    s.others.insert(0, hit);
    Some(s)
}

/// What internal_ctor_probe() saw before the general INIT_METHOD_CALL
/// handler ran (see note_internal_ctor_site).
pub(crate) struct InternalCtorProbe {
    class_name: Vec<u8>,
    obj: *const RefCell<PhpObject>,
    root: bool,
    pending: usize,
    frames: usize,
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
    /// Whether class `lower` is `ancestor` (any case) or extends it.
    fn class_chain_has(&self, lower: &[u8], ancestor: &[u8]) -> bool {
        let mut cur = self.classes.get(lower);
        for _ in 0..64 {
            let Some(ce) = cur else {
                return false;
            };
            if ce.name.eq_ignore_ascii_case(ancestor) {
                return true;
            }
            cur = ce
                .parent
                .as_deref()
                .and_then(|p| self.classes.get(p.to_ascii_lowercase().as_slice()));
        }
        false
    }

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
        op_array: &OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &[Value],
    ) -> NewFast {
        let op = &op_array.ops[ip];
        let literals = &op_array.literals;
        // `new $name`: a class name in a variable or temporary (a leading
        // backslash is dropped)
        let dyn_name = match op.op1 {
            OperandType::Const(_) => None,
            OperandType::Cv(i) => match cvs.get(i as usize) {
                Some(Value::String(s)) => Some(s.clone()),
                Some(Value::Reference(r)) => match r.try_borrow().as_deref() {
                    Ok(Value::String(s)) => Some(s.clone()),
                    _ => return NewFast::No,
                },
                _ => return NewFast::No,
            },
            OperandType::Tmp(i) => match tmps.get(i as usize) {
                Some(Value::String(s)) => Some(s.clone()),
                _ => return NewFast::No,
            },
            _ => return NewFast::No,
        };
        if let Some(n) = &dyn_name {
            let n = n.as_bytes();
            let n = n.strip_prefix(b"\\").unwrap_or(n);
            if n.contains(&0) {
                return NewFast::No;
            }
            if let Some(t) = self.new_site_template(op_array, ip) {
                if n.eq_ignore_ascii_case(&t.lower) {
                    return NewFast::Obj(self.new_from_template(&t, op, tmps), t.no_ctor);
                }
            }
        } else {
            if let Some(t) = self.new_site_template(op_array, ip) {
                return NewFast::Obj(self.new_from_template(&t, op, tmps), t.no_ctor);
            }
            if let Some((obj, no_ctor)) = self.new_internal_fast(op_array, ip, op, tmps) {
                return NewFast::Obj(obj, no_ctor);
            }
        }
        let name = match &dyn_name {
            Some(n) => {
                let n = n.as_bytes();
                n.strip_prefix(b"\\").unwrap_or(n)
            }
            None => {
                let OperandType::Const(ci) = op.op1 else {
                    return NewFast::No;
                };
                let Some(Value::String(name)) = literals.get(ci as usize) else {
                    return NewFast::No;
                };
                name.as_bytes()
            }
        };
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
        let template = match self.classes.get(name_lower) {
            Some(ce)
                if ce.filename.is_some()
                    && !(ce.is_abstract || ce.is_interface || ce.is_enum || ce.is_trait) =>
            {
                ce.new_template
                    .borrow()
                    .as_ref()
                    .filter(|t| t.fits(ce))
                    .cloned()
            }
            None if name_lower == b"stdclass" => return self.new_std_object(op, tmps),
            _ => return NewFast::No,
        };
        if let Some(t) = template {
            Self::note_new_site(op_array, ip, &t);
            return NewFast::Obj(self.new_from_template(&t, op, tmps), t.no_ctor);
        }
        if !self.user_class_chain(name_lower) {
            return NewFast::No;
        }
        // Constant-expression defaults are evaluated on first use, before
        // the object is allocated.
        if self.update_class_constants(name_lower).is_err() {
            return NewFast::Failed;
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
        // What the next objects of the class get (see NewTemplate).
        if let Some(ce) = self
            .classes
            .get(name_lower)
            .filter(|_| self.consts_updated.contains(name_lower))
        {
            if let Some(mut slots) = self.plain_declared_slots(name_lower) {
                let mut arrays = Vec::new();
                for (i, (_, v)) in slots.iter_mut().enumerate() {
                    if let Value::Array(a) = v {
                        arrays.push((i, Rc::downgrade(a)));
                        *v = Value::Null;
                    }
                }
                let mut table = crate::object::PropTable::with_capacity(slots.len());
                for s in slots {
                    table.push(s);
                }
                let t = Rc::new(NewTemplate {
                    tag: crate::object::class_tag_of(&ce.name),
                    props_ptr: ce.properties.as_ptr(),
                    props_len: ce.properties.len(),
                    methods_len: ce.methods.len(),
                    class_name: ce.name.clone(),
                    lower: name_lower.to_vec(),
                    class_idx: std::cell::Cell::new(
                        self.classes.get_index_of(name_lower).unwrap_or(usize::MAX),
                    ),
                    slots: table,
                    arrays,
                    has_destruct,
                    no_ctor: !ce.methods.contains_key(b"__construct".as_slice()),
                    create_hook: self.object_create_hook_of(name_lower),
                });
                Self::note_new_site(op_array, ip, &t);
                *ce.new_template.borrow_mut() = Some(t);
            }
        }
        NewFast::Obj(obj_value, false)
    }

    /// NEW at `ip` of an internal class whose site made one before (see
    /// InternalNewTemplate): what the general handler does then. None:
    /// nothing was done.
    #[inline(never)]
    fn new_internal_fast(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        op: &Op,
        tmps: &[Value],
    ) -> Option<(Value, bool)> {
        let t = op_array
            .sites()
            .borrow()
            .as_ref()
            .and_then(|s| s.internal_new_sites.get(&(ip as u32)).cloned())?;
        if self.current_exception.is_some() || !self.internal_template_valid(&t) {
            return None;
        }
        // Recycle the old object ID from the result slot before allocating
        // a new one.
        if let OperandType::Tmp(idx) = &op.result {
            if let Some(old_val) = tmps.get(*idx as usize) {
                self.maybe_recycle_object_id(old_val);
            }
        }
        let obj_id = self.alloc_object_id();
        let mut props = t.slots.clone();
        for &i in &t.arrays {
            if let Some(slot) = props.get_mut(i) {
                slot.1 = Value::Array(Rc::new(RefCell::new(crate::array::PhpArray::new())));
            }
        }
        if let Some((fi, li, ti)) = t.throwable {
            if let Some(slot) = props.get_mut(fi) {
                slot.1 = Value::String(PhpString::from_string(self.current_file.to_string()));
            }
            if let Some(slot) = props.get_mut(li) {
                slot.1 = Value::Long(op.line as i64);
            }
            let trace = self.build_exception_trace();
            let (n, filter) = t.filter_trace;
            if filter || n != self.trace_without_args_classes.len() {
                self.filter_new_exception_trace(&t.lower, &trace);
            }
            if let Some(slot) = props.get_mut(ti) {
                slot.1 = trace;
            }
        }
        let obj = PhpObject::new_declared(t.class_name.clone(), obj_id, props, t.tag);
        let rc = Rc::new(RefCell::new(obj));
        if let Some(h) = t.create_hook {
            h(self, &rc);
        }
        Some((Value::Object(rc), t.no_ctor))
    }

    /// Whether the class an InternalNewTemplate was made for is still the
    /// one its name finds.
    #[inline]
    fn internal_template_valid(&self, t: &InternalNewTemplate) -> bool {
        if self.object_create_handlers.len() != t.hooks_len {
            return false;
        }
        let generation = self.classes.generation();
        if t.stamp.get() == generation {
            return true;
        }
        let ok = match self.classes.get(t.lower.as_slice()) {
            None => t.entry.is_null(),
            Some(ce) => std::ptr::eq(ce, t.entry),
        };
        if ok {
            t.stamp.set(generation);
        }
        ok
    }

    /// The general NEW handler at `ip` made `obj` (its properties set, not
    /// yet shared) of class `lower`: the site's InternalNewTemplate, when
    /// the class and all of its ancestors are internal and the site names
    /// it by a constant (see new_internal_fast).
    #[inline(never)]
    pub(crate) fn note_internal_new_site(
        &self,
        op_array: &OpArray,
        ip: usize,
        lower: &[u8],
        obj: &PhpObject,
        throwable: bool,
    ) {
        let op = &op_array.ops[ip];
        let OperandType::Const(ci) = op.op1 else {
            return;
        };
        let Some(Value::String(name)) = op_array.literals.get(ci as usize) else {
            return;
        };
        let name = name.as_bytes();
        let bare = name.strip_prefix(b"\\").unwrap_or(name);
        if bare.contains(&0)
            || !bare.eq_ignore_ascii_case(lower)
            || obj.native.is_some()
            || obj.closure.is_some()
        {
            return;
        }
        let entry: *const crate::object::ClassEntry = match self.classes.get(lower) {
            None => {
                if self.internal_class(lower).is_none() {
                    return;
                }
                std::ptr::null()
            }
            Some(ce) => {
                if ce.methods.contains_key(b"__destruct".as_slice()) {
                    return;
                }
                let mut cur = ce;
                for _ in 0..64 {
                    if cur.filename.is_some() || cur.is_enum || cur.is_abstract {
                        return;
                    }
                    let Some(p) = cur.parent.as_ref() else {
                        break;
                    };
                    match self.classes.get(p.to_ascii_lowercase().as_slice()) {
                        Some(pc) => cur = pc,
                        None => break,
                    }
                }
                ce
            }
        };
        let throwable = if throwable {
            let at = |k: &[u8]| obj.properties.iter().position(|(n, _)| n.as_slice() == k);
            let trace = obj.throwable_private_key(b"trace").or_else(|| at(b"trace"));
            match (at(b"file"), at(b"line"), trace) {
                (Some(f), Some(l), Some(t)) => Some((f, l, t)),
                _ => return,
            }
        } else {
            None
        };
        let mut arrays = Vec::new();
        for (i, (_, v)) in obj.properties.iter().enumerate() {
            // (a throwable's trace is made for each object)
            if throwable.is_some_and(|(_, _, t)| t == i) {
                continue;
            }
            match v {
                Value::Undef
                | Value::Null
                | Value::True
                | Value::False
                | Value::Long(_)
                | Value::Double(_)
                | Value::String(_) => {}
                Value::Array(a) if a.borrow().is_empty() => arrays.push(i),
                _ => return,
            }
        }
        // (the arrays are each object's own: not held here)
        let mut slots = obj.properties.clone();
        for &i in arrays.iter().chain(throwable.as_ref().map(|(_, _, t)| t)) {
            if let Some(slot) = slots.get_mut(i) {
                slot.1 = Value::Null;
            }
        }
        let t = InternalNewTemplate {
            class_name: obj.class_name.clone(),
            lower: lower.to_vec(),
            entry,
            stamp: std::cell::Cell::new(self.classes.generation()),
            slots,
            arrays,
            throwable,
            filter_trace: (self.trace_without_args_classes.len(), {
                let targets: Vec<&[u8]> = self
                    .trace_without_args_classes
                    .iter()
                    .map(|c| c.as_slice())
                    .collect();
                !targets.is_empty() && self.class_extends_any(lower, &targets)
            }),
            tag: crate::object::class_tag_of(&obj.class_name),
            no_ctor: self.internal_class_without_ctor(lower),
            create_hook: self.object_create_hook_of(lower),
            hooks_len: self.object_create_handlers.len(),
        };
        op_array
            .sites()
            .borrow_mut()
            .get_or_insert_with(Default::default)
            .internal_new_sites
            .insert(ip as u32, Rc::new(t));
    }

    /// The object the INIT_METHOD_CALL `op` calls a method of, when its
    /// operand holds one (a variable's reference is followed).
    #[inline]
    fn method_call_object(
        op: &Op,
        cvs: &[Value],
        tmps: &[Value],
    ) -> Option<Rc<RefCell<PhpObject>>> {
        match op.op1 {
            OperandType::Cv(i) => match cvs.get(i as usize)? {
                Value::Object(o) => Some(Rc::clone(o)),
                Value::Reference(r) => match &*r.try_borrow().ok()? {
                    Value::Object(o) => Some(Rc::clone(o)),
                    _ => None,
                },
                _ => None,
            },
            OperandType::Tmp(t) => match tmps.get(t as usize)? {
                Value::Object(o) => Some(Rc::clone(o)),
                _ => None,
            },
            _ => None,
        }
    }

    /// INIT_METHOD_CALL `__construct` at `ip` on an object of an internal
    /// class, where the general handler set up the same call for an object
    /// of that class before (see note_internal_ctor_site): that call.
    /// False: nothing was done.
    #[inline(never)]
    pub(crate) fn init_internal_ctor_fast(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &[Value],
    ) -> bool {
        if self.observer.is_some() || self.current_exception.is_some() {
            return false;
        }
        let op = &op_array.ops[ip];
        let Some(obj) = Self::method_call_object(op, cvs, tmps) else {
            return false;
        };
        let (name, root, kind) = {
            let sites = op_array.sites().borrow();
            let Some((class, name, root, kind)) =
                sites.as_ref().and_then(|s| s.ctor_sites.get(&(ip as u32)))
            else {
                return false;
            };
            if obj.borrow().class_name != *class {
                return false;
            }
            (name.clone(), *root, *kind)
        };
        let obj_val = Value::Object(obj);
        if root {
            Self::root_internal_call_this(op, op_array, &obj_val);
        }
        let mut args = self.take_values(0);
        args.push(obj_val);
        self.pending_calls.push(PendingCall {
            name,
            args,
            named_args: Vec::new(),
            target: CallTarget {
                internal_ctor: kind,
                ..CallTarget::NONE
            },
        });
        true
    }

    /// Before the general INIT_METHOD_CALL handler runs at `ip`: when it is
    /// a `__construct` call (by that name) on an object of an internal
    /// class without an observer watching, what note_internal_ctor_site()
    /// compares with what the handler did.
    #[inline(never)]
    pub(crate) fn internal_ctor_probe(
        &self,
        op_array: &OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &[Value],
    ) -> Option<InternalCtorProbe> {
        let op = &op_array.ops[ip];
        let OperandType::Const(mi) = op.op2 else {
            return None;
        };
        match op_array.literals.get(mi as usize) {
            Some(Value::String(m)) if m.as_bytes() == b"__construct" => {}
            _ => return None,
        }
        if self.observer.is_some() || self.current_exception.is_some() {
            return None;
        }
        let obj = Self::method_call_object(op, cvs, tmps)?;
        let class_name = {
            let ob = obj.try_borrow().ok()?;
            if ob.closure.is_some() {
                return None;
            }
            ob.class_name.clone()
        };
        let lower = class_name.to_ascii_lowercase();
        if matches!(
            lower.as_slice(),
            b"fiber" | b"weakreference" | b"weakmap" | b"closure" | b"generator"
        ) {
            return None;
        }
        match self.classes.get(lower.as_slice()) {
            None => {
                self.internal_class(&lower)?;
            }
            Some(ce) => {
                if ce.methods.contains_key(b"__construct".as_slice()) {
                    return None;
                }
                // (a user class: one deriving from an internal class)
                if ce.filename.is_some() {
                    if self.user_class_chain(&lower) {
                        return None;
                    }
                } else {
                    crate::stubs::find_class_lower(&lower)?;
                }
            }
        }
        Some(InternalCtorProbe {
            root: self.has_internal_ctor(&lower),
            class_name,
            obj: Rc::as_ptr(&obj),
            pending: self.pending_calls.len(),
            frames: self.call_stack.len(),
        })
    }

    /// After the general INIT_METHOD_CALL handler ran at `ip` (see
    /// internal_ctor_probe): when all it did was set up a call with the
    /// object as its only argument, the site's next calls for objects of
    /// the class are set up the same way (see init_internal_ctor_fast).
    #[inline(never)]
    pub(crate) fn note_internal_ctor_site(
        &self,
        op_array: &OpArray,
        ip: usize,
        p: InternalCtorProbe,
    ) {
        if self.current_exception.is_some()
            || self.pending_calls.len() != p.pending + 1
            || self.call_stack.len() != p.frames
        {
            return;
        }
        let Some(call) = self.pending_calls.last() else {
            return;
        };
        let t = &call.target;
        let plain_target = t.user.is_none()
            && t.builtin.is_none()
            && !t.has_builtin
            && t.method.is_none()
            && !t.no_ctor
            && !t.no_ns_fallback
            && !t.ns_global
            && !t.closure
            && t.forward.is_none()
            && !t.forward_this
            && t.method_info.is_none()
            && t.fn_gen == 0
            && t.call_flags == 0
            && !t.dynamic
            && t.internal_ctor == 0;
        let only_this = matches!(call.args.as_slice(), [Value::Object(o)] if std::ptr::eq(Rc::as_ptr(o), p.obj));
        if !plain_target || !only_this || !call.named_args.is_empty() {
            return;
        }
        let name = call.name.clone();
        let kind = self.internal_ctor_kind(&p.class_name, name.as_bytes());
        op_array
            .sites()
            .borrow_mut()
            .get_or_insert_with(Default::default)
            .ctor_sites
            .insert(ip as u32, (p.class_name, name, p.root, kind));
    }

    /// The CallTarget::internal_ctor of the constructor call `name` the
    /// general INIT_METHOD_CALL handler set up for an object of class
    /// `class_name`: what the general DO_FCALL handler does with it (see
    /// fast_internal_ctor_target), when it reaches its constructor branch
    /// for the call (no other branch takes the name) and then runs
    /// Exception's / Error's constructor (1: a throwable class, not an
    /// ErrorException, without hooked properties nor a construct handler)
    /// or the construct handler an extension registered for the class (2:
    /// an internal class whose nearest builtin ancestor is itself).
    #[inline(never)]
    fn internal_ctor_kind(&self, class_name: &[u8], name: &[u8]) -> u8 {
        let mut want = class_name.to_vec();
        want.extend_from_slice(b"::__construct");
        if name != want.as_slice() || name.starts_with(b"__") {
            return 0;
        }
        let lower_name = name.to_ascii_lowercase();
        let lower = class_name.to_ascii_lowercase();
        if self.vm_static_builtin_target(&lower_name).is_some()
            || self.functions.contains_key(&lower_name)
            || self.user_functions.contains_key(&lower_name)
        {
            return 0;
        }
        if let Some((native, h)) = self.find_native_class_handler(&lower) {
            if (h.has_method)(&native, b"__construct", true) {
                return 0;
            }
        }
        // The classes of the class chain: the user ones, then the stub
        // classes from the first internal one.
        let mut chain: Vec<Vec<u8>> = Vec::new();
        let mut cur = lower.clone();
        let mut stub_root = None;
        for _ in 0..64 {
            match self.classes.get(cur.as_slice()) {
                Some(ce) if ce.filename.is_some() => {
                    if ce.methods.contains_key(b"__construct".as_slice())
                        || ce
                            .properties
                            .iter()
                            .any(|p| p.has_get_hook || p.has_set_hook)
                    {
                        return 0;
                    }
                    chain.push(cur.clone());
                    match &ce.parent {
                        Some(p) => cur = p.to_ascii_lowercase(),
                        None => return 0,
                    }
                }
                _ => {
                    stub_root = Some(cur);
                    break;
                }
            }
        }
        let Some(root) = stub_root else {
            return 0;
        };
        let Some((_, h)) = crate::stubs::class_hierarchy(&root) else {
            return 0;
        };
        chain.push(root.clone());
        chain.extend(h.parents_lower.iter().cloned());
        let has = |n: &[u8]| chain.iter().any(|c| c.as_slice() == n);
        let handler = chain
            .iter()
            .any(|c| self.object_construct_handlers.contains_key(c.as_slice()));
        if has(b"exception") || has(b"error") {
            if has(b"errorexception") || handler {
                return 0;
            }
            return 1;
        }
        // A Reflection class's constructor (an internal class: the general
        // handler's nearest builtin ancestor is the one it runs the
        // constructor of)
        if chain.len() == 1 + h.parents_lower.len() && !handler {
            let k = match lower.as_slice() {
                b"reflectionclass" | b"reflectionobject" | b"reflectionenum" => 3,
                b"reflectionmethod" => 4,
                b"reflectionfunction" => 5,
                b"reflectionproperty" => 6,
                b"reflectionparameter" => 7,
                b"reflectionclassconstant"
                | b"reflectionenumunitcase"
                | b"reflectionenumbackedcase" => 8,
                _ => 0,
            };
            if k != 0 {
                return k;
            }
        }
        // (the general handler's nearest builtin ancestor of the class is
        // the class itself: it is none of the special ones, nor extends one)
        const SPECIAL: &[&[u8]] = &[
            b"arrayobject",
            b"arrayiterator",
            b"recursivearrayiterator",
            b"datetime",
            b"datetimeimmutable",
            b"datetimezone",
            b"dateinterval",
            b"dateperiod",
            b"iteratoriterator",
            b"splfileinfo",
            b"splfixedarray",
            b"spldoublylinkedlist",
            b"splobjectstorage",
            b"splpriorityqueue",
            b"splheap",
            b"appenditerator",
            b"multipleiterator",
        ];
        let special_ancestor = h.parents_lower.iter().any(|p| {
            SPECIAL.contains(&p.as_slice())
                || p.starts_with(b"reflection")
                || p.ends_with(b"iterator")
        });
        if chain.len() == 1 + h.parents_lower.len()
            && !special_ancestor
            && self
                .object_construct_handlers
                .contains_key(lower.as_slice())
        {
            return 2;
        }
        0
    }

    /// Whether the internal class `lower` has no constructor: neither its
    /// stub, nor an extension's construct handler or native class handler
    /// has one (the general DO_FCALL handler then does nothing for its
    /// constructor call).
    fn internal_class_without_ctor(&self, lower: &[u8]) -> bool {
        if crate::stubs::find_method(lower, b"__construct").is_some()
            || self
                .classes
                .get(lower)
                .is_some_and(|c| c.methods.contains_key(b"__construct".as_slice()))
        {
            return false;
        }
        let mut chain = vec![lower.to_vec()];
        if let Some((_, h)) = crate::stubs::class_hierarchy(lower) {
            chain.extend(h.parents_lower.iter().cloned());
        }
        let mut cur = self.classes.get(lower).and_then(|c| c.parent.clone());
        for _ in 0..64 {
            let Some(p) = cur else {
                break;
            };
            let pl = p.to_ascii_lowercase();
            cur = self
                .classes
                .get(pl.as_slice())
                .and_then(|c| c.parent.clone());
            chain.push(pl);
        }
        if chain
            .iter()
            .any(|c| self.object_construct_handlers.contains_key(c.as_slice()))
        {
            return false;
        }
        match self.find_native_class_handler(lower) {
            Some((native, h)) => {
                !(h.has_method)(&native, b"__construct", false)
                    && !(h.has_method)(&native, b"__construct", true)
            }
            None => true,
        }
    }

    /// `new stdClass`: an object without properties, of a class without a
    /// constructor.
    #[inline(never)]
    fn new_std_object(&mut self, op: &Op, tmps: &[Value]) -> NewFast {
        if let OperandType::Tmp(idx) = &op.result {
            if let Some(old_val) = tmps.get(*idx as usize) {
                self.maybe_recycle_object_id(old_val);
            }
        }
        let obj_id = self.alloc_object_id();
        let rc = Rc::new(RefCell::new(PhpObject::new(b"stdClass".to_vec(), obj_id)));
        self.run_object_create_hook(&rc);
        NewFast::Obj(Value::Object(rc), true)
    }

    /// The NewTemplate the NEW at `ip` used last, while its class still has
    /// it (new_from_template() then makes the object).
    #[inline]
    pub(crate) fn new_site_template(
        &self,
        op_array: &OpArray,
        ip: usize,
    ) -> Option<Rc<NewTemplate>> {
        self.new_site(op_array, ip).map(|(t, _)| t)
    }

    /// new_site_template(), and whether the NEW is followed by the
    /// constructor call new_skips_ctor_call() may skip (see
    /// ctor_call_shape()).
    #[inline]
    pub(crate) fn new_site(
        &self,
        op_array: &OpArray,
        ip: usize,
    ) -> Option<(Rc<NewTemplate>, bool)> {
        // (its class's name lowercased then)
        let (t, shape) = op_array
            .sites()
            .borrow()
            .as_ref()
            .and_then(|s| s.new_sites.get(&(ip as u32)).cloned())?;
        let ce = match self.classes.get_index(t.class_idx.get()) {
            Some((k, ce)) if k.as_slice() == t.lower.as_slice() => Some(ce),
            _ => self.classes.get_full(t.lower.as_slice()).map(|(i, _, ce)| {
                t.class_idx.set(i);
                ce
            }),
        };
        let same = ce.is_some_and(|ce| {
            ce.new_template
                .borrow()
                .as_ref()
                .is_some_and(|ct| Rc::ptr_eq(ct, &t) && t.fits(ce))
        });
        same.then_some((t, shape))
    }

    /// Whether the opcodes from `ip` (after a NEW) are its constructor
    /// call without arguments: INIT_METHOD_CALL `__construct` on the new
    /// object, then DO_FCALL (see new_skips_ctor_call()).
    fn ctor_call_shape(op_array: &OpArray, ip: usize) -> bool {
        let (Some(init), Some(fcall)) = (op_array.ops.get(ip), op_array.ops.get(ip + 1)) else {
            return false;
        };
        let Some(new_op) = ip.checked_sub(1).and_then(|i| op_array.ops.get(i)) else {
            return false;
        };
        if init.opcode != crate::opcode::OpCode::InitMethodCall
            || fcall.opcode != crate::opcode::OpCode::DoFCall
            || init.op1 != new_op.result
            || !matches!(new_op.result, OperandType::Tmp(_))
            || !matches!(fcall.op1, OperandType::Unused)
            || !matches!(fcall.result, OperandType::Tmp(_) | OperandType::Unused)
        {
            return false;
        }
        match init.op2 {
            OperandType::Const(c) => matches!(op_array.literals.get(c as usize),
                Some(Value::String(s)) if s.as_bytes().eq_ignore_ascii_case(b"__construct")),
            _ => false,
        }
    }

    /// Whether the NEW before `ip` (that made `obj`, of a class without a
    /// constructor) is followed by its constructor call without arguments:
    /// INIT_METHOD_CALL `__construct` on the new object, then DO_FCALL.
    /// ZEND_NEW then jumps over them (what DO_FCALL would do for the call of
    /// the missing constructor is done by the caller: a NULL result). Not
    /// with an observer (it watches the call), nor with an exception or an
    /// error handler's throw to dispatch. `has_destruct`: the class has a
    /// destructor (its new object is marked as awaiting its constructor).
    #[inline(never)]
    pub(crate) fn new_skips_ctor_call(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        obj: &Value,
        has_destruct: bool,
    ) -> bool {
        Self::ctor_call_shape(op_array, ip) && self.skip_ctor_call(obj, has_destruct)
    }

    /// new_skips_ctor_call() of a NEW whose ctor_call_shape() is known:
    /// the constructor call is skipped unless something watches or
    /// interrupts it.
    #[inline]
    pub(crate) fn skip_ctor_call(&mut self, obj: &Value, has_destruct: bool) -> bool {
        if self.observer.is_some() || self.current_exception.is_some() || self.error_handler_threw {
            return false;
        }
        // (what the DO_FCALL of the missing constructor does)
        self.last_call_by_ref = false;
        self.call_result_unused = None;
        if has_destruct {
            if let Value::Object(o) = obj {
                let mut ob = o.borrow_mut();
                if ob.has_property(b"__ctor_pending") {
                    ob.properties.retain(|(k, _)| k != b"__ctor_pending");
                }
            }
        }
        true
    }

    fn note_new_site(op_array: &OpArray, ip: usize, t: &Rc<NewTemplate>) {
        let shape = Self::ctor_call_shape(op_array, ip + 1);
        op_array
            .sites()
            .borrow_mut()
            .get_or_insert_with(Default::default)
            .new_sites
            .insert(ip as u32, (Rc::clone(t), shape));
    }

    /// new_object_fast() of a class whose NewTemplate is known: what it
    /// does then.
    pub(crate) fn new_from_template(&mut self, t: &NewTemplate, op: &Op, tmps: &[Value]) -> Value {
        // Recycle the old object ID from the result slot before allocating
        // a new one.
        if let OperandType::Tmp(idx) = &op.result {
            if let Some(old_val) = tmps.get(*idx as usize) {
                self.maybe_recycle_object_id(old_val);
            }
        }
        let obj_id = self.alloc_object_id();
        // (as init_declared_props() sets them)
        let mut props = t.slots.clone();
        // (an array default is shared copy-on-write; fits() saw it alive)
        for (i, a) in &t.arrays {
            if let (Some(a), Some(slot)) = (a.upgrade(), props.get_mut(*i)) {
                slot.1 = Value::Array(a);
            }
        }
        let obj = PhpObject::new_declared(t.class_name.clone(), obj_id, props, t.tag);
        let rc = Rc::new(RefCell::new(obj));
        if let Some(h) = t.create_hook {
            h(self, &rc);
        }
        let obj_value = Value::Object(rc);
        if t.has_destruct {
            if let Value::Object(ref rc) = obj_value {
                rc.borrow_mut()
                    .set_property(b"__ctor_pending".to_vec(), Value::True);
            }
            self.list_destructible(obj_value.clone());
        }
        obj_value
    }

    /// INIT_METHOD_CALL of a public user method named by a constant on an
    /// object of a class whose hierarchy is all user code: what the general
    /// handler does then (the method is registered as a user function and
    /// the call is set up with `$this`). The call site remembers what it
    /// found for the class it saw last (see MethodSite). Returns false,
    /// doing nothing, otherwise.
    pub(crate) fn init_method_call_fast(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &mut [Value],
    ) -> bool {
        let op = &op_array.ops[ip];
        let OperandType::Const(mi) = op.op2 else {
            return false;
        };
        let Some(Value::String(method_name)) = op_array.literals.get(mi as usize) else {
            return false;
        };
        let m = method_name.as_bytes();
        let is_ctor = m.eq_ignore_ascii_case(b"__construct");
        if m.is_empty() || (m.starts_with(b"__") && !is_ctor) {
            return false;
        }
        // (a call trampoline forwards to __call(): the general handler)
        if op_array.ops.len() <= 4 && op_array.name.starts_with(b"__closure_fcc_tramp_") {
            return false;
        }
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
        let site = {
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
            let scope = self.class_scope_ref();
            let Ok(mut sites) = op_array.sites().try_borrow_mut() else {
                return false;
            };
            match sites
                .as_mut()
                .and_then(|s| s.method_sites.get_mut(&(ip as u32)))
                .and_then(|s| {
                    if s.is_for(&ob.class_name, scope) {
                        Some(&*s)
                    } else if s.others.is_empty() {
                        None
                    } else {
                        other_method_site(s, &ob.class_name, scope)
                    }
                }) {
                Some(s) if s.fallback => return false,
                Some(s) => match &s.method {
                    Some(w) => w.upgrade().map(|m| {
                        (
                            Some(m),
                            s.func_name.clone(),
                            s.sep,
                            s.is_static,
                            s.reg_gen,
                            s.info.clone(),
                        )
                    }),
                    None => Some((None, s.func_name.clone(), 0, false, 0, None)),
                },
                _ => None,
            }
        };
        let Some((method, func_name, sep, is_static, reg_gen, info)) = site else {
            return self.init_method_call_resolve(op_array, ip, obj, tmps);
        };
        let Some(method_op_array) = method else {
            // `new` of a class without a constructor.
            self.push_noctor_call(func_name, obj);
            return true;
        };
        self.push_method_call(
            op_array,
            ip,
            obj,
            tmps,
            method_op_array,
            func_name,
            sep,
            is_static,
            reg_gen,
            info,
        );
        true
    }

    /// init_method_call_fast(): what the MethodMemo says of a call of method
    /// `m` on `obj` from the running scope (a fallback answer: no method and
    /// an empty name).
    #[inline(never)]
    #[allow(clippy::type_complexity)]
    fn memo_method_site(
        &self,
        obj: &Rc<RefCell<PhpObject>>,
        m: &[u8],
    ) -> Option<(
        Option<Rc<OpArray>>,
        PhpString,
        usize,
        bool,
        usize,
        Option<Rc<MethodCallInfo>>,
    )> {
        let ob = obj.try_borrow().ok()?;
        let scope = self.class_scope_ref();
        METHOD_MEMO.with(|mm| {
            let mm = mm.try_borrow().ok()?;
            let s = mm.get(&ob.class_name, m, scope)?;
            if s.fallback {
                return Some((None, PhpString::empty(), 0, false, 0, None));
            }
            let method = match &s.method {
                Some(w) => Some(w.upgrade()?),
                None => None,
            };
            Some((
                method,
                s.func_name.clone(),
                s.sep,
                s.is_static,
                0,
                s.info.clone(),
            ))
        })
    }

    /// init_method_call_fast() for a class its call site has not seen last:
    /// the checks, and what the call site remembers.
    fn init_method_call_resolve(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        obj: Rc<RefCell<PhpObject>>,
        tmps: &mut [Value],
    ) -> bool {
        let op = &op_array.ops[ip];
        let OperandType::Const(mi) = op.op2 else {
            return false;
        };
        let Some(Value::String(method_name)) = op_array.literals.get(mi as usize) else {
            return false;
        };
        let m = method_name.as_bytes();
        // (what another site found, see MethodMemo)
        if let Some((method, func_name, sep, is_static, _, info)) = self.memo_method_site(&obj, m) {
            return match method {
                Some(method_op_array) => {
                    self.push_method_call(
                        op_array,
                        ip,
                        obj,
                        tmps,
                        method_op_array,
                        func_name,
                        sep,
                        is_static,
                        0,
                        info,
                    );
                    true
                }
                // (a fallback answer: see memo_method_site)
                None if func_name.is_empty() => false,
                None => {
                    self.push_noctor_call(func_name, obj);
                    true
                }
            };
        }
        let is_ctor = m.eq_ignore_ascii_case(b"__construct");
        let mut mbuf = [0u8; 64];
        let Some(method_lower) = lower_into(m, &mut mbuf) else {
            return false;
        };
        let method_lower: &[u8] = method_lower;
        // (a long name: an anonymous class's)
        let mut cbuf = [0u8; 128];
        let long;
        let (class_lower, class_name_orig) = {
            let ob = obj.borrow();
            let l = match lower_into(&ob.class_name, &mut cbuf) {
                Some(l) => l as &[u8],
                None => {
                    long = ob.class_name.to_ascii_lowercase();
                    long.as_slice()
                }
            };
            (l, ob.class_name.clone())
        };
        let scope = self.class_scope_ref().map(|s| s.to_vec());
        // (what the call site remembers when the general handler is to run)
        let fallback = |class_name: Vec<u8>, scope: Option<Vec<u8>>| {
            store_method_site(
                op_array,
                ip,
                MethodSite {
                    class_name,
                    scope,
                    method: None,
                    fallback: true,
                    func_name: PhpString::empty(),
                    info: None,
                    sep: 0,
                    is_static: false,
                    reg_gen: 0,
                    others: Vec::new(),
                },
            );
            false
        };
        let Some(class) = self.classes.get(class_lower) else {
            return fallback(class_name_orig, scope);
        };
        let Some(method) = class.methods.get(method_lower) else {
            // `new` of a class without a constructor: the call names
            // `Class::__construct`, and $this.
            if !is_ctor || !self.user_class_chain(class_lower) {
                return fallback(class_name_orig, scope);
            }
            let mut func_name = class_name_orig.clone();
            func_name.extend_from_slice(b"::__construct");
            // (the call site's, not counted by memory_get_usage(), see MethodSite)
            let func_name = PhpString::from_bytes_untracked(&func_name);
            store_method_site(
                op_array,
                ip,
                MethodSite {
                    class_name: class_name_orig,
                    scope,
                    method: None,
                    fallback: false,
                    func_name: func_name.clone(),
                    info: None,
                    sep: 0,
                    is_static: false,
                    reg_gen: 0,
                    others: Vec::new(),
                },
            );
            self.push_noctor_call(func_name, obj);
            return true;
        };
        // A private method of the calling scope that the class redeclares
        // (or inherits) may be the one called: the general handler decides.
        let scope_private = scope.as_deref().is_some_and(|caller| {
            !method.declaring_class.eq_ignore_ascii_case(caller) && {
                let caller_lower = caller.to_ascii_lowercase();
                self.classes
                    .get(&caller_lower)
                    .and_then(|cc| cc.get_method(method_lower))
                    .is_some_and(|cm| cm.visibility == Visibility::Private)
            }
        });
        // A private method called from its class, a protected one from a
        // class of the object's (zend_check_protected(): the scope and the
        // method's root class are then both its ancestors, one extends the
        // other).
        let visible = match method.visibility {
            Visibility::Public => true,
            Visibility::Private => scope
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(&method.declaring_class)),
            Visibility::Protected => scope
                .as_deref()
                .is_some_and(|s| self.class_chain_has(class_lower, s)),
        };
        // (a class deriving from an internal one: a method a user class of
        // its declared, not of the internal ancestor's; not of the SPL
        // classes whose methods all check their object was constructed,
        // see the general handler)
        let user_method = || {
            method.op_array.internal_sig.is_none()
                && self
                    .spl_find_base(
                        class_lower,
                        &[
                            b"splfileobject",
                            b"globiterator",
                            b"recursiveiteratoriterator",
                        ],
                    )
                    .is_none()
                && !method.is_abstract
                && self
                    .classes
                    .get(method.declaring_class.to_ascii_lowercase().as_slice())
                    .is_some_and(|c| c.filename.is_some() && !c.is_enum)
                && self
                    .classes
                    .get(class_lower)
                    .is_some_and(|c| c.filename.is_some() && !c.is_enum)
        };
        if !visible
            || !crate::object::call_attributes_inert(&method.attributes)
            || !crate::object::call_attributes_inert(&method.op_array.attributes)
            || scope_private
            || !(self.user_class_chain(class_lower) || user_method())
        {
            return fallback(class_name_orig, scope);
        }
        let is_static = method.is_static;
        let method_op_array = method.op_array.clone();
        let mut func_name = class_name_orig.clone();
        let sep = func_name.len();
        func_name.extend_from_slice(b"::");
        func_name.extend_from_slice(&method.name);
        let func_name = PhpString::from_bytes_untracked(&func_name);
        let info = Rc::new(self.method_call_info(&method_op_array, &func_name, sep));
        store_method_site(
            op_array,
            ip,
            MethodSite {
                class_name: class_name_orig,
                scope,
                method: Some(Rc::downgrade(&method_op_array)),
                fallback: false,
                func_name: func_name.clone(),
                info: Some(info.clone()),
                sep,
                is_static,
                reg_gen: 0,
                others: Vec::new(),
            },
        );
        self.push_method_call(
            op_array,
            ip,
            obj,
            tmps,
            method_op_array,
            func_name,
            sep,
            is_static,
            0,
            Some(info),
        );
        true
    }

    /// The pending call of a `new` of a user class without a constructor.
    fn push_noctor_call(&mut self, func_name: PhpString, obj: Rc<RefCell<PhpObject>>) {
        let fn_gen = self.user_functions.len() + 1;
        // (built in place, see push_method_call)
        self.pending_calls
            .extend(std::iter::once_with(move || PendingCall {
                name: func_name,
                args: Vec::new(),
                named_args: Vec::new(),
                target: CallTarget::NONE,
            }));
        let Some(call) = self.pending_calls.last_mut() else {
            return;
        };
        if let Some(buf) = self.value_pool.pop() {
            super::fill_empty(&mut call.args, buf);
        }
        call.args
            .extend(std::iter::once_with(move || Value::Object(obj)));
        call.target.no_ctor = true;
        call.target.fn_gen = fn_gen;
    }

    /// The pending call of public user method `method_op_array` (named
    /// `func_name`, "::" at `sep`) on `obj`. `reg_gen`: the
    /// `user_functions.len() + 1` at which the call site last saw the method
    /// registered under its name.
    #[allow(clippy::too_many_arguments)]
    fn push_method_call(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        obj: Rc<RefCell<PhpObject>>,
        tmps: &mut [Value],
        method_op_array: Rc<OpArray>,
        func_name: PhpString,
        sep: usize,
        is_static: bool,
        reg_gen: usize,
        info: Option<Rc<MethodCallInfo>>,
    ) {
        let op = &op_array.ops[ip];
        let fname = func_name.as_bytes();
        let is_ctor = fname[sep + 2..].eq_ignore_ascii_case(b"__construct");
        // Register the method as a user function (usually it is already).
        if reg_gen != self.user_functions.len() + 1 {
            if !self
                .user_fn_lookup(fname)
                .is_some_and(|f| Rc::ptr_eq(f, &method_op_array))
            {
                self.user_functions
                    .insert(fname.to_ascii_lowercase(), method_op_array.clone());
            }
            let fn_gen = self.user_functions.len() + 1;
            if let Some(s) = op_array
                .sites()
                .borrow_mut()
                .get_or_insert_with(Default::default)
                .method_sites
                .get_mut(&(ip as u32))
                // (the site's answer for this method: not when the call is
                // one the MethodMemo answered)
                .filter(|s| s.func_name.ptr_eq(&func_name))
            {
                s.reg_gen = fn_gen;
            }
        }
        // (a name with one leading underscore is a plain one: the engine's
        // own start with two, see fast_fcall_target)
        let plain =
            !(fname[sep + 2..].eq_ignore_ascii_case(b"tokenize") || fname.starts_with(b"__"));
        let fn_gen = self.user_functions.len() + 1;
        // The call is built in place (see push_resolved_call), and $this
        // stored into its slot there: a value built apart and moved in is
        // copied by wide loads of its just stored narrow fields, which wait
        // for them.
        self.pending_calls
            .extend(std::iter::once_with(move || PendingCall {
                name: func_name,
                args: Vec::new(),
                named_args: Vec::new(),
                target: CallTarget::NONE,
            }));
        let Some(call) = self.pending_calls.last_mut() else {
            return;
        };
        if let Some(buf) = self.value_pool.pop() {
            super::fill_empty(&mut call.args, buf);
        }
        // INIT_METHOD_CALL consumes a temporary object operand (`new` keeps
        // its result for the constructor call; a static method's is
        // released with the rest).
        let mut _released = None;
        match op.op1 {
            OperandType::Tmp(t) if !is_ctor => {
                let v = std::mem::replace(&mut tmps[t as usize], Value::Undef);
                if is_static {
                    _released = Some(v);
                } else {
                    call.args.push(v);
                }
            }
            // (made in place, see above)
            _ if !is_static => call
                .args
                .extend(std::iter::once_with(move || Value::Object(obj))),
            _ => {}
        }
        // What DO_FCALL's fast path looks up again (see fast_fcall_target;
        // it leaves PhpToken::tokenize() to the general path).
        if plain {
            call.target.user = Some(method_op_array);
            call.target.method = Some((sep as u32, is_static));
            call.target.fn_gen = fn_gen;
            call.target.method_info = info;
        }
    }
}
