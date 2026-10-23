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
use crate::object::{NewTemplate, PhpObject, Visibility};
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
        tmps: &[Value],
    ) -> NewFast {
        let op = &op_array.ops[ip];
        let literals = &op_array.literals;
        if let Some(t) = self.new_site_template(op_array, ip) {
            return NewFast::Obj(self.new_from_template(&t, op, tmps), t.no_ctor);
        }
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
        let template = match self.classes.get(name_lower) {
            Some(ce)
                if ce.filename.is_some()
                    && ce.attributes.is_empty()
                    && !(ce.is_abstract || ce.is_interface || ce.is_enum || ce.is_trait) =>
            {
                ce.new_template
                    .borrow()
                    .as_ref()
                    .filter(|t| t.fits(ce))
                    .cloned()
            }
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
                let t = Rc::new(NewTemplate {
                    props_ptr: ce.properties.as_ptr(),
                    props_len: ce.properties.len(),
                    methods_len: ce.methods.len(),
                    class_name: ce.name.clone(),
                    lower: name_lower.to_vec(),
                    slots,
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

    /// The NewTemplate the NEW at `ip` used last, while its class still has
    /// it (new_from_template() then makes the object).
    #[inline]
    pub(crate) fn new_site_template(
        &self,
        op_array: &OpArray,
        ip: usize,
    ) -> Option<Rc<NewTemplate>> {
        // (its class's name lowercased then)
        let t = op_array
            .sites()
            .borrow()
            .as_ref()
            .and_then(|s| s.new_sites.get(&(ip as u32)).cloned())?;
        let same = self.classes.get(t.lower.as_slice()).is_some_and(|ce| {
            ce.new_template
                .borrow()
                .as_ref()
                .is_some_and(|ct| Rc::ptr_eq(ct, &t) && t.fits(ce))
        });
        same.then_some(t)
    }

    /// Whether the NEW before `ip` (that made `obj`, of a class without a
    /// constructor) is followed by its constructor call without arguments:
    /// INIT_METHOD_CALL `__construct` on the new object, then DO_FCALL.
    /// ZEND_NEW then jumps over them (what DO_FCALL would do for the call of
    /// the missing constructor is done by the caller: a NULL result). Not
    /// with an observer (it watches the call), nor with an exception or an
    /// error handler's throw to dispatch.
    #[inline(never)]
    pub(crate) fn new_skips_ctor_call(
        &mut self,
        op_array: &OpArray,
        ip: usize,
        obj: &Value,
    ) -> bool {
        let (Some(init), Some(fcall)) = (op_array.ops.get(ip), op_array.ops.get(ip + 1)) else {
            return false;
        };
        let new_op = &op_array.ops[ip - 1];
        let ctor_named = match init.op2 {
            OperandType::Const(c) => matches!(op_array.literals.get(c as usize),
                Some(Value::String(s)) if s.as_bytes().eq_ignore_ascii_case(b"__construct")),
            _ => false,
        };
        if init.opcode != crate::opcode::OpCode::InitMethodCall
            || !ctor_named
            || init.op1 != new_op.result
            || !matches!(new_op.result, OperandType::Tmp(_))
            || fcall.opcode != crate::opcode::OpCode::DoFCall
            || !matches!(fcall.op1, OperandType::Unused)
            || !matches!(fcall.result, OperandType::Tmp(_) | OperandType::Unused)
            || self.observer.is_some()
            || self.current_exception.is_some()
            || self.error_handler_threw
        {
            return false;
        }
        // (what the DO_FCALL of the missing constructor does)
        self.last_call_by_ref = false;
        self.call_result_unused = None;
        if let Value::Object(o) = obj {
            let mut ob = o.borrow_mut();
            if ob.has_property(b"__ctor_pending") {
                ob.properties.retain(|(k, _)| k != b"__ctor_pending");
            }
        }
        true
    }

    fn note_new_site(op_array: &OpArray, ip: usize, t: &Rc<NewTemplate>) {
        op_array
            .sites()
            .borrow_mut()
            .get_or_insert_with(Default::default)
            .new_sites
            .insert(ip as u32, Rc::clone(t));
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
        let mut obj = PhpObject::new(t.class_name.clone(), obj_id);
        // (as init_declared_props() sets them, one by one)
        obj.properties.reserve(t.slots.len());
        for (name, v) in &t.slots {
            obj.properties.push((name.clone(), v.clone()));
            obj.account();
        }
        // (an array default is shared copy-on-write; fits() saw it alive)
        for (i, a) in &t.arrays {
            if let (Some(a), Some(slot)) = (a.upgrade(), obj.properties.get_mut(*i)) {
                slot.1 = Value::Array(a);
            }
        }
        obj.mark_declared();
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
            let sites = op_array.sites().borrow();
            match sites
                .as_ref()
                .and_then(|s| s.method_sites.get(&(ip as u32)))
                .filter(|s| s.class_name == ob.class_name && s.scope.as_deref() == scope)
            {
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
            op_array
                .sites()
                .borrow_mut()
                .get_or_insert_with(Default::default)
                .method_sites
                .insert(
                    ip as u32,
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
            op_array
                .sites()
                .borrow_mut()
                .get_or_insert_with(Default::default)
                .method_sites
                .insert(
                    ip as u32,
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
        if !visible
            || !method.attributes.is_empty()
            || !method.op_array.attributes.is_empty()
            || scope_private
            || !self.user_class_chain(class_lower)
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
        op_array
            .sites()
            .borrow_mut()
            .get_or_insert_with(Default::default)
            .method_sites
            .insert(
                ip as u32,
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
            call.args = buf;
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
            {
                s.reg_gen = fn_gen;
            }
        }
        let plain = !(fname[sep + 2..].eq_ignore_ascii_case(b"tokenize") || fname[0] == b'_');
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
            call.args = buf;
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
