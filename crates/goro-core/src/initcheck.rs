//! The lookups INIT_FCALL / INIT_STATIC_METHOD_CALL / INIT_METHOD_CALL make
//! before the arguments of a call are evaluated.
//!
//! goro resolves most calls at DO_FCALL; php-src fails a call of an
//! undefined function, of a class that does not exist or of an undefined
//! method of a user class at the INIT_* opcode, before any argument is
//! evaluated (`nofunc(f())` never calls f()). These checks tell, for the
//! pending call just set up, whether DO_FCALL is bound to fail that way:
//! only for what is sure to (plain names, classes made of user code only),
//! leaving everything else to DO_FCALL.

use crate::value::Value;
use crate::vm::Vm;

impl Vm {
    /// The top pending call was set up without finding its function (see
    /// CallTarget): only then may init_call_undefined() have something to
    /// say.
    #[inline]
    pub(crate) fn top_call_unresolved(&self) -> bool {
        self.pending_calls
            .last()
            .is_some_and(|c| c.target.user.is_none() && !c.target.has_builtin)
    }

    /// The Error message INIT_* raises for the pending call just set up
    /// (the top one), when its function or method is sure not to exist:
    /// "Call to undefined function f()", "Class "C" not found", "Call to
    /// undefined method C::m()". None: the call may go ahead.
    #[inline(never)]
    pub(crate) fn init_call_undefined(&mut self) -> Option<String> {
        if self.current_exception.is_some() {
            return None;
        }
        let call = self.pending_calls.last()?;
        let fn_gen = self.user_functions.len() + 1;
        if call.target.fn_gen == fn_gen && (call.target.user.is_some() || call.target.has_builtin) {
            return None;
        }
        if call.target.closure || call.target.forward.is_some() || !call.named_args.is_empty() {
            return None;
        }
        let name = call.name.as_bytes();
        if name.is_empty() || name.contains(&0) {
            return None;
        }
        let bare = name.strip_prefix(b"\\").unwrap_or(name);
        let Some(sep) = crate::string::find_double_colon(bare) else {
            // A plain function name.
            if bare.starts_with(b"__") || bare.contains(&b':') {
                return None;
            }
            let lower = bare.to_ascii_lowercase();
            if self.user_functions.contains_key(&lower) || self.functions.contains_key(&lower) {
                return None;
            }
            // (INIT_NS_FCALL_BY_NAME: the global function of an
            // unqualified namespaced name)
            if !call.target.no_ns_fallback {
                if let Some(p) = lower.iter().rposition(|&b| b == b'\\') {
                    let gl = &lower[p + 1..];
                    if self.user_functions.contains_key(gl) || self.functions.contains_key(gl) {
                        return None;
                    }
                }
            }
            return Some(format!(
                "Call to undefined function {}()",
                call.name.to_msg_string()
            ));
        };
        let (cls, method) = (&bare[..sep], &bare[sep + 2..]);
        if cls.is_empty() || method.is_empty() || method.contains(&b':') {
            return None;
        }
        // An instance call ($this leads the arguments) is the method call's.
        if matches!(call.args.first(), Some(Value::Object(o))
            if o.borrow().class_name.eq_ignore_ascii_case(cls))
        {
            return None;
        }
        let mut cl = cls.to_ascii_lowercase();
        let keyword = matches!(cl.as_slice(), b"self" | b"static" | b"parent");
        match cl.as_slice() {
            b"self" => cl = self.scope_top()?.to_ascii_lowercase(),
            b"static" => {
                cl = self
                    .called_class_stack
                    .last()
                    .filter(|s| !s.is_empty())?
                    .to_ascii_lowercase()
            }
            b"parent" => {
                let scope = self.scope_top()?;
                cl = self
                    .classes
                    .get(scope.as_slice())?
                    .parent
                    .as_ref()?
                    .to_ascii_lowercase();
            }
            _ => {}
        }
        if !self.classes.contains_key(&cl) {
            if keyword || self.class_aliases.contains_key(&cl) {
                return None;
            }
            // (an internal class: the methods of its stub)
            if self.is_known_builtin_class(&cl)
                || self.native_class_handlers.contains_key(&cl)
                || crate::stubs::find_class_lower(&cl).is_some()
            {
                return self.user_method_missing(&cl, method, false);
            }
            return Some(format!(
                "Class \"{}\" not found",
                crate::string::msg_cstr(cls)
            ));
        }
        self.user_method_missing(&cl, method, false)
    }

    /// INIT_STATIC_METHOD_CALL of `C::m()` (C a user class named as is)
    /// finding the method: zend_std_get_static_method()'s visibility error
    /// (unless __callStatic() / __call() may take over), an abstract method,
    /// or a non-static one called from a frame without $this. None: the call
    /// may go ahead (DO_FCALL then decides).
    #[inline(never)]
    pub(crate) fn init_static_call_error(&self, has_this: bool) -> Option<String> {
        if self.current_exception.is_some() {
            return None;
        }
        let call = self.pending_calls.last()?;
        if call.target.closure || !call.named_args.is_empty() {
            return None;
        }
        // (no argument is sent yet: an object is the frame's $this the
        // call carries)
        let with_this = match call.args.as_slice() {
            [] => false,
            [Value::Object(_)] => true,
            _ => return None,
        };
        let name = call.name.as_bytes();
        if name.contains(&0) {
            return None;
        }
        let bare = name.strip_prefix(b"\\").unwrap_or(name);
        let sep = crate::string::find_double_colon(bare)?;
        let (cls, method) = (&bare[..sep], &bare[sep + 2..]);
        if cls.is_empty() || method.is_empty() || method.contains(&b':') {
            return None;
        }
        let mut cl = cls.to_ascii_lowercase();
        match cl.as_slice() {
            b"self" => cl = self.scope_top()?.to_ascii_lowercase(),
            b"static" => {
                cl = self
                    .called_class_stack
                    .last()
                    .filter(|s| !s.is_empty())?
                    .to_ascii_lowercase()
            }
            b"parent" => {
                let scope = self.scope_top()?;
                cl = self
                    .classes
                    .get(scope.as_slice())?
                    .parent
                    .as_ref()?
                    .to_ascii_lowercase();
            }
            _ => {}
        }
        // (a keyword names a user class: its frame's $this decides)
        let has_this = has_this || with_this;
        let ml = method.to_ascii_lowercase();
        // (constructors and magic methods have errors of their own)
        if ml.starts_with(b"__") {
            return None;
        }
        let Some(c) = self
            .classes
            .get(cl.as_slice())
            .filter(|c| c.filename.is_some())
        else {
            // An internal class's public non-static method, from no $this.
            if has_this || self.classes.contains_key(cl.as_slice()) {
                return None;
            }
            crate::stubs::find_class_lower(&cl)?;
            // (the error names the method's declaring class)
            let (owner, sm) = crate::stubs::find_method(&cl, &ml)?;
            if sm.flags & (crate::stubs::F_STATIC | crate::stubs::F_ABSTRACT) != 0
                || sm.flags & crate::stubs::F_PUBLIC == 0
            {
                return None;
            }
            return Some(format!(
                "Non-static method {}::{}() cannot be called statically",
                owner.name, sm.name
            ));
        };
        let meth = c.get_method(&ml)?;
        if meth.op_array.ops.is_empty() && !meth.is_abstract {
            return None;
        }
        let decl = meth.declaring_class.to_ascii_lowercase();
        if !self
            .classes
            .get(decl.as_slice())
            .is_some_and(|d| d.filename.is_some())
        {
            return None;
        }
        if meth.visibility != crate::object::Visibility::Public {
            if c.get_method(b"__callstatic").is_some() || c.get_method(b"__call").is_some() {
                return None;
            }
            let scope = self.current_class_scope();
            if let Some(e) = self.check_visibility(
                meth.visibility,
                &meth.declaring_class,
                &c.name,
                &crate::string::lossy(&meth.name),
                false,
                scope.as_deref(),
            ) {
                return Some(e);
            }
        }
        if meth.is_abstract {
            let decl = self.class_name_for_display(&decl);
            return Some(format!(
                "Cannot call abstract method {}::{}()",
                crate::string::msg_str(decl.as_deref().unwrap_or(&meth.declaring_class)),
                crate::string::msg_str(&meth.name)
            ));
        }
        if !meth.is_static && !has_this {
            // (zend_non_static_method_call(): the declaring class)
            let decl = self.class_name_for_display(&decl);
            return Some(format!(
                "Non-static method {}::{}() cannot be called statically",
                crate::string::msg_str(decl.as_deref().unwrap_or(&c.name)),
                crate::string::msg_str(&meth.name)
            ));
        }
        None
    }

    /// "Call to undefined method C::m()" when class `class_lower` and all
    /// of its ancestors are user classes (no internal method, handler or
    /// forwarding is involved) declaring neither `method` nor `__call()` /
    /// `__callStatic()` (an `instance` call: no `__call()`; zend_std_get_method()
    /// does not consider `__callStatic()`). None otherwise.
    #[inline(never)]
    pub(crate) fn user_method_missing(
        &self,
        class_lower: &[u8],
        method: &[u8],
        instance: bool,
    ) -> Option<String> {
        if method.is_empty() || method.contains(&0) {
            return None;
        }
        let ml = method.to_ascii_lowercase();
        // (NEW's constructor call: a class without one has none; property
        // hook calls `parent::$p::get()` are `__property_get_p` ones, of a
        // plain property too: the call handles those)
        if ml.starts_with(b"__") {
            return None;
        }
        let class = self.classes.get(class_lower);
        let display = match class {
            Some(c) => crate::value::display_class_name(&c.name),
            None => crate::stubs::find_class(class_lower)?.name.to_string(),
        };
        let has = |methods: &crate::fxhash::FxIndexMap<Vec<u8>, crate::object::MethodDef>| {
            methods.contains_key(&ml)
                || methods.contains_key(b"__call".as_slice())
                || (!instance && methods.contains_key(b"__callstatic".as_slice()))
        };
        let missing = || {
            Some(format!(
                "Call to undefined method {}::{}()",
                display,
                crate::string::msg_str(method)
            ))
        };
        // An internal class: the methods its stub declares, but for those
        // whose get_method handler finds more (the dual iterators forward
        // to their inner iterator).
        let stub_missing_uncached = |name: &[u8]| {
            let mut stub = crate::stubs::find_class(name);
            for _ in 0..16 {
                let Some(s) = stub else {
                    return false;
                };
                // (the classes whose get_method handler is not
                // zend_std_get_method())
                const OWN_GET_METHOD: [&str; 9] = [
                    "IteratorIterator",
                    "RecursiveIteratorIterator",
                    "Closure",
                    "PDO",
                    "PDOStatement",
                    "SplFileInfo",
                    "__PHP_Incomplete_Class",
                    "XMLReader",
                    "_ZendTestClass",
                ];
                if OWN_GET_METHOD
                    .iter()
                    .any(|n| s.name.eq_ignore_ascii_case(n))
                    || s.methods.iter().any(|m| {
                        m.name.eq_ignore_ascii_case("__call")
                            || m.name.eq_ignore_ascii_case("__callStatic")
                            || m.name.as_bytes().eq_ignore_ascii_case(&ml)
                    })
                {
                    return false;
                }
                match s.parent {
                    None => return true,
                    Some(p) => stub = crate::stubs::find_class(p.as_bytes()),
                }
            }
            false
        };
        // (what the stubs answer does not change: remembered)
        let stub_missing = |name: &[u8]| {
            let mut key = name.to_ascii_lowercase();
            key.extend_from_slice(b"::");
            key.extend_from_slice(&ml);
            if let Some(&r) = self.stub_method_missing_memo.borrow().get(&key) {
                return r;
            }
            let r = stub_missing_uncached(name);
            self.stub_method_missing_memo.borrow_mut().insert(key, r);
            r
        };
        let Some(mut cur) = class else {
            return stub_missing(class_lower).then(missing).flatten();
        };
        for _ in 0..64 {
            if has(&cur.methods) {
                return None;
            }
            // (an enum's cases() / from() / tryFrom() are internal)
            if cur.is_enum && matches!(ml.as_slice(), b"cases" | b"from" | b"tryfrom") {
                return None;
            }
            if cur.filename.is_none() {
                return stub_missing(&cur.name).then(missing).flatten();
            }
            for t in &cur.traits {
                if has(&self.classes.get(t.to_ascii_lowercase().as_slice())?.methods) {
                    return None;
                }
            }
            match &cur.parent {
                None => return missing(),
                Some(p) => match self.classes.get(p.to_ascii_lowercase().as_slice()) {
                    Some(c) => cur = c,
                    None => return stub_missing(p).then(missing).flatten(),
                },
            }
        }
        None
    }

    /// The parameter names of the function the top pending call is sure to
    /// call, when it takes no extra named arguments (not variadic): a named
    /// argument SEND_UNPACK sends is then checked against them. None: the
    /// callee is not known (or collects any name).
    #[inline(never)]
    pub(crate) fn unpack_known_params(&self) -> Option<Vec<Vec<u8>>> {
        let call = self.pending_calls.last()?;
        let name = call.name.as_bytes();
        let fname = match name.windows(2).position(|w| w == b"::") {
            Some(p) => &name[p + 2..],
            None => name,
        };
        if let Some(f) = self.call_user_fn(call) {
            if f.variadic_param.is_some() {
                return None;
            }
            let own = f.name.as_slice();
            let own = match own.windows(2).position(|w| w == b"::") {
                Some(p) => &own[p + 2..],
                None => own,
            };
            // (the function the call names, not a __call() trampoline)
            if !own.eq_ignore_ascii_case(fname)
                || own.eq_ignore_ascii_case(b"__call")
                || own.eq_ignore_ascii_case(b"__callstatic")
            {
                return None;
            }
            let base = crate::internal_frames::implicit_args(f);
            return Some(
                f.cv_names
                    .iter()
                    .take(f.param_count as usize)
                    .skip(base)
                    .cloned()
                    .collect(),
            );
        }
        if name.contains(&b':') || name.contains(&b'\\') || !call.target.has_builtin {
            return None;
        }
        let info = crate::arginfo::lookup(&name.to_ascii_lowercase())?;
        if info.params.iter().any(|p| p.variadic) {
            return None;
        }
        Some(
            info.params
                .iter()
                .map(|p| p.name.as_bytes().to_vec())
                .collect(),
        )
    }

    /// zend_handle_named_arg() for a named argument sent to the top pending
    /// call: the error it raises at that send (an unknown name, or a
    /// parameter a positional argument or an earlier named one already
    /// passed), when the callee is known.
    #[inline(never)]
    pub(crate) fn named_send_error(&self, name: &[u8]) -> Option<String> {
        let call = self.pending_calls.last()?;
        // (a class without a constructor takes no arguments by name)
        if call.target.no_ctor {
            return Some(format!(
                "Unknown named parameter ${}",
                crate::string::msg_str(name)
            ));
        }
        if call.named_args.iter().any(|(n, _)| n.as_slice() == name) {
            return Some(format!(
                "Named parameter ${} overwrites previous argument",
                crate::string::msg_str(name)
            ));
        }
        let params = self.unpack_known_params()?;
        match params.iter().position(|p| p.as_slice() == name) {
            None => Some(format!(
                "Unknown named parameter ${}",
                crate::string::msg_str(name)
            )),
            Some(i) if i < self.pending_positional_args(call) => Some(format!(
                "Named parameter ${} overwrites previous argument",
                crate::string::msg_str(name)
            )),
            Some(_) => None,
        }
    }

    /// The parameter names of the plain function the top pending call is
    /// sure to call, when named arguments were already sent to it (see
    /// place_named_before_positional()).
    #[inline(never)]
    pub(crate) fn unpack_positional_after_named_params(&self) -> Option<Vec<Vec<u8>>> {
        let call = self.pending_calls.last()?;
        if call.named_args.is_empty() {
            return None;
        }
        let f = self.call_user_fn(call)?;
        if crate::internal_frames::implicit_args(f) != 0 {
            return None;
        }
        // (not the variadic parameter: names it does not declare are
        // collected by it)
        let declared = f
            .variadic_param
            .map_or(f.param_count as usize, |v| v as usize);
        Some(f.cv_names.iter().take(declared).cloned().collect())
    }

    /// zend_handle_named_arg() raised ZEND_CALL_NUM_ARGS to cover the
    /// declared parameters the named arguments passed (the skipped ones
    /// left undefined): a positional argument SEND_UNPACK sends after them
    /// takes the next slot. Moves those named arguments to their slots.
    #[inline(never)]
    pub(crate) fn place_named_before_positional(
        call: &mut crate::vm::PendingCall,
        names: &[Vec<u8>],
    ) {
        let last = call
            .named_args
            .iter()
            .filter_map(|(n, _)| names.iter().position(|p| p == n))
            .max();
        let Some(last) = last else {
            return;
        };
        if last < call.args.len() {
            return;
        }
        call.args.resize(last + 1, crate::value::Value::Undef);
        let mut i = 0;
        while i < call.named_args.len() {
            match names.iter().position(|p| *p == call.named_args[i].0) {
                Some(pos) if matches!(call.args[pos], crate::value::Value::Undef) => {
                    let (_, v) = call.named_args.remove(i);
                    call.args[pos] = v;
                }
                _ => i += 1,
            }
        }
    }

    /// The positional arguments sent so far to `call` (without an implicit
    /// `$this`).
    fn pending_positional_args(&self, call: &crate::vm::PendingCall) -> usize {
        match self.call_user_fn(call) {
            Some(f) => call
                .args
                .len()
                .saturating_sub(crate::internal_frames::implicit_args(f)),
            None => call.args.len(),
        }
    }

    /// Whether the argument the top pending call takes next is known to be
    /// by value (a builtin's by-reference `array &$array` is passed the
    /// variable's array itself).
    pub(crate) fn next_arg_by_value(&self) -> bool {
        let Some(call) = self.pending_calls.last() else {
            return false;
        };
        let idx = call.args.len();
        if let Some(f) = self.call_user_fn(call) {
            return !crate::vm::arg_by_ref(f, idx);
        }
        let name = call.name.as_bytes();
        if name.contains(&b':') || name.is_empty() {
            return false;
        }
        let name = name.strip_prefix(b"\\").unwrap_or(name);
        let lname = match name.iter().rposition(|&b| b == b'\\') {
            Some(p) if !call.target.no_ns_fallback || call.target.ns_global => &name[p + 1..],
            Some(_) => return false,
            None => name,
        };
        let Some(info) = crate::arginfo::lookup(&lname.to_ascii_lowercase()) else {
            return false;
        };
        match info
            .params
            .get(idx)
            .or_else(|| info.params.last().filter(|p| p.variadic))
        {
            Some(p) => p.by_ref == 0,
            None => true,
        }
    }

    /// SEND_VAR of a variable's array at `ip` of `op_array`: whether code
    /// runs between it and the call (evaluating the next arguments), which
    /// may write the variable. The value sent is then a copy of its own
    /// (copy-on-write: SEND_VAR's zval copy), not the variable's array the
    /// write would change in place.
    #[inline]
    pub(crate) fn send_needs_snapshot(op_array: &crate::opcode::OpArray, ip: usize) -> bool {
        use crate::opcode::OpCode;
        for op in op_array.ops.iter().skip(ip + 1).take(32) {
            match op.opcode {
                OpCode::SendVal | OpCode::SendValChecked | OpCode::SendValRaw => {}
                OpCode::DoFCall => return false,
                _ => return true,
            }
        }
        true
    }

    /// INSTANCEOF of a constant class name at `ip` of `op_array` answered
    /// `result` for an object of `o`'s class: the site remembers it, when
    /// the class named exists (an object's ancestors and interfaces do not
    /// change, nor does whether a declared class is one of them).
    #[inline(never)]
    pub(crate) fn note_instanceof(
        &self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        o: &std::rc::Rc<std::cell::RefCell<crate::object::PhpObject>>,
        class_lower: &[u8],
        result: &Value,
    ) {
        // (`instanceof static` & co. depend on the scope or called class)
        let named = match op_array.ops.get(ip).map(|o| o.op2) {
            Some(crate::opcode::OperandType::Const(c)) => op_array.literals.get(c as usize),
            _ => None,
        };
        let Some(Value::String(named)) = named else {
            return;
        };
        let named = named.as_bytes();
        if named.eq_ignore_ascii_case(b"self")
            || named.eq_ignore_ascii_case(b"parent")
            || named.eq_ignore_ascii_case(b"static")
            || !(self.classes.contains_key(class_lower)
                || self.class_aliases.contains_key(class_lower))
        {
            return;
        }
        let Ok(ob) = o.try_borrow() else {
            return;
        };
        if ob.native.is_some() {
            return;
        }
        self.instanceof_site_memo.borrow_mut().set(
            self.classes.link_generation(),
            &ob.class_name,
            named,
            matches!(result, Value::True),
        );
        let mut sites = op_array.sites().borrow_mut();
        if let Some(c) = sites
            .get_or_insert_with(Default::default)
            .prop_slot(&op_array.ops, ip)
        {
            // (two classes: the slot's `scope` holds the second one, its
            // `slot` the answer for it; a third replaces the first)
            let answer = u8::from(matches!(result, Value::True));
            if c.set && c.class != ob.class_name && c.scope.is_none() {
                c.scope = Some(ob.class_name.clone());
                c.slot = u32::from(answer);
            } else {
                c.class.clear();
                c.class.extend_from_slice(&ob.class_name);
                c.plain = answer;
                if !c.set {
                    c.scope = None;
                    c.slot = 0;
                }
                c.set = true;
            }
        }
    }

    /// What the INSTANCEOF at `ip` of `op_array` answered for an object
    /// of the class of the one `op1` holds (see note_instanceof).
    #[inline(never)]
    pub(crate) fn cached_instanceof(
        &self,
        op_array: &crate::opcode::OpArray,
        ip: usize,
        cvs: &[Value],
        tmps: &mut [Value],
    ) -> crate::fastpath::Fast {
        use crate::fastpath::Fast;
        use crate::opcode::OperandType;
        let op = &op_array.ops[ip];
        if !matches!(op.op2, OperandType::Const(_))
            || !crate::fastpath::tmp_writable(&op.result, tmps)
        {
            return Fast::No;
        }
        let o = match op.op1 {
            OperandType::Cv(i) => match cvs.get(i as usize) {
                Some(Value::Object(o)) => o.clone(),
                Some(Value::Reference(r)) => match r.try_borrow().as_deref() {
                    Ok(Value::Object(o)) => o.clone(),
                    _ => return Fast::No,
                },
                _ => return Fast::No,
            },
            OperandType::Tmp(i) => match tmps.get(i as usize) {
                Some(Value::Object(o)) => o.clone(),
                _ => return Fast::No,
            },
            _ => return Fast::No,
        };
        let Ok(ob) = o.try_borrow() else {
            return Fast::No;
        };
        let r = {
            let Ok(mut sites) = op_array.sites().try_borrow_mut() else {
                return Fast::No;
            };
            match sites.as_mut().and_then(|s| s.prop_slot(&op_array.ops, ip)) {
                Some(c) if c.set && c.class == ob.class_name => c.plain != 0,
                Some(c) if c.set && c.scope.as_deref() == Some(ob.class_name.as_slice()) => {
                    c.slot != 0
                }
                // (a class the site does not remember: what another answer
                // of a site said of it, see note_instanceof)
                Some(c) if c.set => {
                    let named = match op.op2 {
                        OperandType::Const(k) => match op_array.literals.get(k as usize) {
                            Some(Value::String(n)) => n,
                            _ => return Fast::No,
                        },
                        _ => return Fast::No,
                    };
                    match self.instanceof_site_memo.borrow_mut().get(
                        self.classes.link_generation(),
                        &ob.class_name,
                        named.as_bytes(),
                    ) {
                        Some(r) => r,
                        None => return Fast::No,
                    }
                }
                _ => return Fast::No,
            }
        };
        drop(ob);
        let v = if r { Value::True } else { Value::False };
        if crate::fastpath::store_tmp(&op.result, v, tmps) {
            Fast::Done
        } else {
            Fast::No
        }
    }
}
