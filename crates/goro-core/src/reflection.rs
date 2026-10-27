// Reflection API implementation for goro-rs
// Extracted from vm.rs - provides PHP Reflection classes

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::{PhpObject, RuntimeAttribute, Visibility};
use crate::opcode::{OpArray, ParamType};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

/// Validate that the user-supplied argument count is `expected`.
/// `args` is the raw arg slice (includes $this at args[0]).
/// Throws ArgumentCountError on mismatch and returns false.
/// Returns true if the count is correct.
pub(crate) fn check_arg_count(
    vm: &mut Vm,
    args: &[Value],
    expected: usize,
    method_label: &str,
) -> bool {
    let user_arg_count = args.len().saturating_sub(1);
    if user_arg_count != expected {
        let plural = if expected == 1 {
            "argument"
        } else {
            "arguments"
        };
        let err = format!(
            "{}() expects exactly {} {}, {} given",
            method_label, expected, plural, user_arg_count
        );
        let exc = vm.create_exception(b"ArgumentCountError", &err, vm.current_line);
        vm.current_exception = Some(exc);
        return false;
    }
    true
}

/// Validate user-supplied arg count is in [min, max] range.
/// Returns true if the count is within bounds.
pub(crate) fn check_arg_count_range(
    vm: &mut Vm,
    args: &[Value],
    min: usize,
    max: usize,
    method_label: &str,
) -> bool {
    let user_arg_count = args.len().saturating_sub(1);
    if user_arg_count < min || user_arg_count > max {
        let err = if min == max {
            let plural = if min == 1 { "argument" } else { "arguments" };
            format!(
                "{}() expects exactly {} {}, {} given",
                method_label, min, plural, user_arg_count
            )
        } else if user_arg_count < min {
            let plural = if min == 1 { "argument" } else { "arguments" };
            format!(
                "{}() expects at least {} {}, {} given",
                method_label, min, plural, user_arg_count
            )
        } else {
            let plural = if max == 1 { "argument" } else { "arguments" };
            format!(
                "{}() expects at most {} {}, {} given",
                method_label, max, plural, user_arg_count
            )
        };
        let exc = vm.create_exception(b"ArgumentCountError", &err, vm.current_line);
        vm.current_exception = Some(exc);
        return false;
    }
    true
}

/// Validate that arg at `arg_index` (0-based, where 0 is the first user arg)
/// is a string (or stringable), or null with deprecation.
/// Returns Some(stringified) on success, None if exception was thrown.
pub(crate) fn require_string_arg(
    vm: &mut Vm,
    args: &[Value],
    arg_index: usize,
    method_label: &str,
    param_name: &str,
    type_descriptor: &str,
    allow_null_deprecated: bool,
) -> Option<String> {
    let arg = args.get(arg_index + 1)?;
    match arg {
        Value::String(s) => Some(s.to_msg_string()),
        Value::Long(n) => Some(n.to_string()),
        Value::Double(_d) => {
            // Match PHP's number formatting for floats
            Some(arg.to_php_string().to_msg_string())
        }
        Value::True => Some("1".to_string()),
        Value::False => Some("".to_string()),
        Value::Null => {
            if allow_null_deprecated {
                vm.emit_deprecated_at(
                    &format!(
                        "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                        method_label,
                        arg_index + 1,
                        param_name,
                        type_descriptor
                    ),
                    vm.current_line,
                );
                Some("".to_string())
            } else {
                let err = format!(
                    "{}(): Argument #{} (${}) must be of type {}, null given",
                    method_label,
                    arg_index + 1,
                    param_name,
                    type_descriptor
                );
                let exc = vm.create_exception(b"TypeError", &err, vm.current_line);
                vm.current_exception = Some(exc);
                None
            }
        }
        Value::Array(_) => {
            let err = format!(
                "{}(): Argument #{} (${}) must be of type {}, array given",
                method_label,
                arg_index + 1,
                param_name,
                type_descriptor
            );
            let exc = vm.create_exception(b"TypeError", &err, vm.current_line);
            vm.current_exception = Some(exc);
            None
        }
        Value::Object(o) => {
            let class_name = crate::string::msg_str(&o.borrow().class_name).to_string();
            let err = format!(
                "{}(): Argument #{} (${}) must be of type {}, {} given",
                method_label,
                arg_index + 1,
                param_name,
                type_descriptor,
                class_name
            );
            let exc = vm.create_exception(b"TypeError", &err, vm.current_line);
            vm.current_exception = Some(exc);
            None
        }
        _ => Some(arg.to_php_string().to_msg_string()),
    }
}

/// Whether `lower` names an internal class/interface the VM implements.
pub(crate) fn internal_class_known(vm: &Vm, lower: &[u8]) -> bool {
    vm.is_known_builtin_class(lower) || vm.native_class_handlers.contains_key(lower)
}

/// Reflection's view of a class: the user class, or for an internal class a
/// `ClassEntry` synthesized from php-src's stubs (see `crate::stubs`).
/// An extension class registered as a bare entry (its methods are
/// implemented natively, e.g. ext/dom's; it may still declare the stub's
/// properties, as XSLTProcessor does for its typed properties).
/// Non-public stub methods may be kept for the engine's visibility checks
/// (tidyNode's private constructor): the entry is still a bare one.
fn bare_entry(c: &crate::object::ClassEntry) -> bool {
    c.filename.is_none()
        && c.start_line == 0
        && !c.is_enum
        && (c.methods.is_empty() || only_stub_nonpublic_methods(c))
}

fn only_stub_nonpublic_methods(c: &crate::object::ClassEntry) -> bool {
    let Some(sc) = crate::stubs::find_class(&c.name.to_ascii_lowercase()) else {
        return false;
    };
    c.methods.values().all(|m| {
        m.visibility != Visibility::Public
            && sc.methods.iter().any(|f| {
                f.name.as_bytes().eq_ignore_ascii_case(&m.name) && f.visibility() == m.visibility
            })
    })
}

/// Reflection sees the class through its stub: an internal class, or one
/// an extension registers as a bare entry.
pub(crate) fn stub_backed_class(vm: &Vm, lower: &[u8]) -> bool {
    match vm.classes.get(lower) {
        None => true,
        Some(c) => bare_entry(c) && crate::stubs::find_class(lower).is_some(),
    }
}

fn lower_name(n: &[u8]) -> Vec<u8> {
    n.strip_prefix(b"\\").unwrap_or(n).to_ascii_lowercase()
}

/// The interfaces `ce` implements, each followed by the ones it extends
/// (zend_do_implement_interfaces()), without duplicates; lowercase.
fn flat_interfaces(vm: &Vm, ce: &crate::object::ClassEntry, out: &mut Vec<Vec<u8>>, depth: u32) {
    if depth > 64 {
        return;
    }
    for i in &ce.interfaces {
        let l = lower_name(i);
        if out.contains(&l) {
            continue;
        }
        out.push(l.clone());
        if let Some(ice) = rcls(vm, &l) {
            flat_interfaces(vm, ice, out, depth + 1);
        }
    }
}

/// zend_function.common.prototype of method `m` (lowercase) of class
/// `class_lower`, as inheritance sets it (do_inheritance_check_on_method()
/// for the parent's methods, then for each newly implemented interface's):
/// the lowercase name of the prototype's class.
fn linked_method_prototype(vm: &Vm, class_lower: &[u8], m: &[u8], depth: u32) -> Option<Vec<u8>> {
    if depth > 64 {
        return None;
    }
    let ce = rcls(vm, class_lower)?;
    // An enum's cases()/from()/tryFrom() implement UnitEnum's/BackedEnum's.
    if ce.is_enum && ce.get_method(m).is_none() {
        return match m {
            b"cases" => Some(b"unitenum".to_vec()),
            b"from" | b"tryfrom" if ce.enum_backing_type.is_some() => Some(b"backedenum".to_vec()),
            _ => None,
        };
    }
    let Some(meth) = ce.get_method(m) else {
        // (a method inherited from an internal class is not copied into
        // the user class's table: it is the parent's, with its prototype)
        let p = ce.parent.as_deref().map(lower_name)?;
        return linked_method_prototype(vm, &p, m, depth + 1);
    };
    // (interface methods have none, redeclared or inherited)
    if ce.is_interface {
        return None;
    }
    let decl = lower_name(&meth.declaring_class);
    let own = decl.as_slice() == class_lower;
    let parent = ce.parent.as_deref().map(lower_name);
    let pce = parent.as_deref().and_then(|p| rcls(vm, p));
    let pm = pce.and_then(|p| p.get_method(m));
    // The scope of `parent->common.prototype ?: parent`.
    let proto_of = |owner: &[u8], f: &crate::object::MethodDef| {
        linked_method_prototype(vm, owner, m, depth + 1)
            .unwrap_or_else(|| lower_name(&f.declaring_class))
    };
    let mut proto = match (parent.as_deref(), pm) {
        // An inherited method is the parent's function, with its prototype.
        (Some(p), Some(_)) if !own => linked_method_prototype(vm, p, m, depth + 1),
        (Some(p), Some(pm)) if pm.visibility != crate::object::Visibility::Private => {
            let pr = proto_of(p, pm);
            // Constructors only have an abstract (or interface) prototype.
            if m == b"__construct" {
                let abs = rcls(vm, &pr)
                    .map(|c| c.is_interface || c.get_method(m).is_some_and(|f| f.is_abstract))
                    .unwrap_or(false);
                abs.then_some(pr)
            } else {
                Some(pr)
            }
        }
        _ => None,
    };
    let mut inherited_ifaces = Vec::new();
    if let Some(pce) = pce {
        flat_interfaces(vm, pce, &mut inherited_ifaces, 0);
    }
    let mut ifaces = Vec::new();
    flat_interfaces(vm, ce, &mut ifaces, 0);
    // A class declaring __toString() implements Stringable (implicitly).
    if m == b"__tostring" && !ce.is_trait {
        let s = b"stringable".to_vec();
        if pce.is_some_and(|p| p.get_method(m).is_some()) && !inherited_ifaces.contains(&s) {
            inherited_ifaces.push(s.clone());
        }
        if !ifaces.contains(&s) {
            ifaces.push(s);
        }
    }
    for i in ifaces.iter().filter(|i| !inherited_ifaces.contains(i)) {
        let Some(im) = rcls(vm, i).and_then(|ice| ice.get_method(m)) else {
            continue;
        };
        let ip = proto_of(i, im);
        if proto.as_ref() != Some(&ip) {
            proto = Some(ip);
        }
    }
    proto
}

pub(crate) fn rcls<'a>(vm: &'a Vm, lower: &[u8]) -> Option<&'a crate::object::ClassEntry> {
    // A bare extension entry: the stub declares the class.
    let bare = bare_entry;
    if let Some(c) = vm.classes.get(lower) {
        if !bare(c) || crate::stubs::find_class(lower).is_none() {
            return Some(c);
        }
    }
    let owned;
    let l: &[u8] = if lower.iter().any(|b| b.is_ascii_uppercase()) {
        owned = lower.to_ascii_lowercase();
        if let Some(c) = vm.classes.get(&owned) {
            if !bare(c) || crate::stubs::find_class(&owned).is_none() {
                return Some(c);
            }
        }
        &owned
    } else {
        lower
    };
    if internal_class_known(vm, l) {
        crate::stubs::class_entry(vm, l)
    } else {
        lossy_named_class(vm, l)
    }
}

/// A class whose name is not UTF-8 (Symfony's `\xA9` value wrapper), looked
/// up by the lossy form a reflector keeps of it.
#[inline(never)]
fn lossy_named_class<'a>(vm: &'a Vm, lower: &[u8]) -> Option<&'a crate::object::ClassEntry> {
    if !lower.windows(3).any(|w| w == "\u{FFFD}".as_bytes()) {
        return None;
    }
    vm.classes
        .iter()
        .find(|(k, _)| crate::string::msg_str(k).as_bytes() == lower)
        .map(|(_, c)| c)
}

/// Whether the class is an internal one (no user declaration).
pub(crate) fn is_internal_class(vm: &Vm, lower: &[u8]) -> bool {
    match vm.classes.get(lower) {
        // Extension classes are registered without a declaring file
        // (userland classes get theirs from DeclareClass).
        Some(c) => c.filename.is_none() && c.start_line == 0,
        None => rcls(vm, lower).is_some(),
    }
}

/// Find a method of a class, including methods inherited from internal
/// parent classes and interfaces (which user `ClassEntry`s don't copy).
pub(crate) fn rmethod<'a>(
    vm: &'a Vm,
    lower: &[u8],
    method: &[u8],
) -> Option<&'a crate::object::MethodDef> {
    let ml = method.to_ascii_lowercase();
    let mut cur = lower.to_ascii_lowercase();
    let mut ifaces: Vec<Vec<u8>> = Vec::new();
    if let Some(m) = enum_builtin_method(vm, &cur, &ml) {
        return Some(m);
    }
    for _ in 0..64 {
        let c = rcls(vm, &cur)?;
        if let Some(m) = c.methods.get(&ml) {
            return Some(m);
        }
        for i in &c.interfaces {
            ifaces.push(i.to_ascii_lowercase());
        }
        match &c.parent {
            Some(p) => cur = p.to_ascii_lowercase(),
            None => break,
        }
    }
    let mut seen = 0;
    while let Some(i) = ifaces.pop() {
        seen += 1;
        if seen > 64 {
            break;
        }
        if let Some(c) = rcls(vm, &i) {
            if let Some(m) = c.methods.get(&ml) {
                return Some(m);
            }
            for j in &c.interfaces {
                ifaces.push(j.to_ascii_lowercase());
            }
        }
    }
    None
}

/// All methods of a class in function-table order, including those
/// inherited from internal ancestors/interfaces (user `ClassEntry`s only
/// carry methods inherited from user classes).
pub(crate) fn rmethods<'a>(vm: &'a Vm, lower: &[u8]) -> Vec<&'a crate::object::MethodDef> {
    let lower = lower.to_ascii_lowercase();
    let mut out: Vec<&'a crate::object::MethodDef> = Vec::new();
    let ce = match rcls(vm, &lower) {
        Some(c) => c,
        None => return out,
    };
    out.extend(ce.methods.values());
    if !vm.classes.contains_key(&lower) {
        return out;
    }
    for em in [&b"cases"[..], b"from", b"tryfrom"] {
        if let Some(m) = enum_builtin_method(vm, &lower, em) {
            out.push(m);
        }
    }
    let mut pending: Vec<Vec<u8>> = Vec::new();
    let mut cur: &crate::object::ClassEntry = ce;
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 64 {
            break;
        }
        for i in &cur.interfaces {
            pending.push(i.to_ascii_lowercase());
        }
        let p = match &cur.parent {
            Some(p) => p.to_ascii_lowercase(),
            None => break,
        };
        let pce = match rcls(vm, &p) {
            Some(c) => c,
            None => break,
        };
        // an internal parent (or a bare extension entry whose methods its
        // native handler serves) gives the methods the user class lacks
        if !vm.classes.contains_key(&p) || vm.classes.get(&p).is_some_and(bare_entry) {
            for m in pce.methods.values() {
                if !out.iter().any(|x| x.name.eq_ignore_ascii_case(&m.name)) {
                    out.push(m);
                }
            }
        }
        cur = pce;
    }
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut idx = 0;
    while idx < pending.len() && idx < 128 {
        let i = pending[idx].clone();
        idx += 1;
        if seen.contains(&i) {
            continue;
        }
        seen.push(i.clone());
        if let Some(ice) = rcls(vm, &i) {
            if !vm.classes.contains_key(&i) || vm.classes.get(&i).is_some_and(bare_entry) {
                for m in ice.methods.values() {
                    if !out.iter().any(|x| x.name.eq_ignore_ascii_case(&m.name)) {
                        out.push(m);
                    }
                }
            }
            for j in &ice.interfaces {
                pending.push(j.to_ascii_lowercase());
            }
        }
    }
    out
}

/// Weak-mode scalar coercion of an argument to an internal parameter type
/// (as the engine does before an internal function runs).
pub(crate) fn coerce_scalar_for_type(v: &Value, ty: &str) -> Value {
    if ty.is_empty() {
        return v.clone();
    }
    let parts: Vec<String> = ty
        .trim_start_matches('?')
        .split('|')
        .map(|p| p.to_ascii_lowercase())
        .collect();
    let has = |n: &str| parts.iter().any(|p| p == n || p == "mixed");
    let accepted = match v {
        Value::True | Value::False => {
            has("bool")
                || (matches!(v, Value::True) && has("true"))
                || (matches!(v, Value::False) && has("false"))
        }
        Value::Long(_) => has("int") || has("float"),
        Value::Double(_) => has("float"),
        Value::String(_) => has("string"),
        _ => true,
    };
    if accepted {
        return v.clone();
    }
    match v {
        Value::True | Value::False | Value::Long(_) | Value::Double(_) => {
            if has("string") {
                Value::String(v.to_php_string())
            } else if has("int") {
                Value::Long(v.to_long())
            } else if has("float") {
                Value::Double(v.to_double())
            } else if has("bool") {
                if v.is_truthy() {
                    Value::True
                } else {
                    Value::False
                }
            } else {
                v.clone()
            }
        }
        _ => v.clone(),
    }
}

/// Run an internal constructor with its own call-stack frame
/// (`Class->__construct(...)`), so exceptions it throws carry the frame in
/// their trace like PHP's internal functions do.
fn with_internal_ctor_frame(
    vm: &mut Vm,
    args: &[Value],
    line: u32,
    f: fn(&mut Vm, &[Value], u32) -> bool,
) -> bool {
    let cls = match args.first() {
        Some(Value::Object(o)) => o.borrow().class_name.to_ascii_lowercase(),
        _ => return f(vm, args, line),
    };
    // The frame names the internal class declaring the constructor.
    let mut cur = cls;
    let mut decl: Option<(&'static str, &'static crate::stubs::StubFunc)> = None;
    for _ in 0..64 {
        if !vm.classes.contains_key(&cur) {
            decl = crate::stubs::find_method(&cur, b"__construct").map(|(c, m)| (c.name, m));
            break;
        }
        match vm.classes.get(&cur).and_then(|c| c.parent.clone()) {
            Some(p) => cur = p.to_ascii_lowercase(),
            None => break,
        }
    }
    let (decl, sig) = match decl {
        Some((d, m)) => (d, Some(m)),
        None => return f(vm, args, line),
    };
    let frame_args: Vec<Value> = args
        .iter()
        .skip(1)
        .enumerate()
        .map(|(i, v)| match sig.and_then(|m| m.args.get(i)) {
            Some(a) => coerce_scalar_for_type(v, a.ty),
            None => v.clone(),
        })
        .collect();
    // (a constructor the engine runs from another one is not zpp'd)
    let nested = vm
        .call_stack
        .last()
        .is_some_and(|f| f.4 && f.0.ends_with("->__construct"));
    vm.call_stack.push((
        format!("{}->__construct", decl),
        vm.current_file.clone(),
        line,
        frame_args,
        true,
    ));
    if let Some(this) = args.first() {
        vm.note_frame_this(this);
    }
    // zpp's argument count, from the constructor's stub
    if let Some(m) = sig.filter(|_| !nested) {
        let n = args.len().saturating_sub(1);
        let min = m.required_args();
        let max = if m
            .args
            .last()
            .is_some_and(|a| a.flags & crate::stubs::A_VARIADIC != 0)
        {
            None
        } else {
            Some(m.args.len())
        };
        if n < min || max.is_some_and(|x| n > x) {
            let (kind, k) = if max == Some(min) {
                ("exactly", min)
            } else if n < min {
                ("at least", min)
            } else {
                ("at most", max.unwrap_or(min))
            };
            let msg = format!(
                "{}::__construct() expects {} {} argument{}, {} given",
                decl,
                kind,
                k,
                if k == 1 { "" } else { "s" },
                n
            );
            let exc = vm.create_exception(b"ArgumentCountError", &msg, line);
            vm.current_exception = Some(exc);
            vm.call_stack.pop();
            return false;
        }
    }
    let r = f(vm, args, line);
    vm.call_stack.pop();
    r
}

pub fn reflection_class_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    with_internal_ctor_frame(vm, args, line, reflection_class_construct_impl)
}

pub fn reflection_method_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    with_internal_ctor_frame(vm, args, line, reflection_method_construct_impl)
}

pub fn reflection_function_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    with_internal_ctor_frame(vm, args, line, reflection_function_construct_impl)
}

pub fn reflection_property_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    with_internal_ctor_frame(vm, args, line, reflection_property_construct_impl)
}

pub fn reflection_parameter_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let ok = with_internal_ctor_frame(vm, args, line, reflection_parameter_construct_impl);
    // [$closure, '__invoke']: a parameter of the Closure::__invoke trampoline.
    let invoke = matches!(args.get(1).map(|v| v.deref()), Some(Value::Array(a)) if {
        let a = a.borrow();
        a.get_int(0).is_some_and(|c| c.is_closure_obj())
            && a.get_int(1)
                .is_some_and(|m| m.to_php_string().as_bytes().eq_ignore_ascii_case(b"__invoke"))
    });
    if ok && invoke {
        if let Some(Value::Object(o)) = args.first() {
            o.borrow_mut()
                .set_property(b"__reflection_param_trampoline".to_vec(), Value::True);
        }
    }
    // A closure's parameter: getDeclaringFunction() reflects that closure.
    if ok && !invoke {
        if let (Some(Value::Object(o)), Some(c)) = (args.first(), args.get(1).map(|v| v.deref())) {
            if c.is_closure_obj() {
                o.borrow_mut()
                    .set_property(b"__reflection_param_closure".to_vec(), c);
            }
        }
    }
    ok
}

pub fn reflection_class_constant_construct(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    with_internal_ctor_frame(vm, args, line, reflection_class_constant_construct_impl)
}

/// Whether a stored class-constant value is still an unevaluated marker.
fn is_deferred_const_value(v: &Value) -> bool {
    match v {
        Value::String(s) => {
            let b = s.as_bytes();
            b.starts_with(b"__deferred_") || b.starts_with(b"__enum_case__::")
        }
        Value::Array(a) => a.borrow().iter().any(|(_, v)| is_deferred_const_value(v)),
        _ => false,
    }
}

/// Evaluate a class constant the way a `Class::NAME` fetch does (resolving
/// enum cases and deferred constant expressions). Returns None when the
/// evaluation threw (the exception is left in `vm.current_exception`).
pub(crate) fn resolve_class_constant(
    vm: &mut Vm,
    class_lower: &[u8],
    name: &[u8],
) -> Option<Value> {
    let (raw, cname) = {
        let ce = rcls(vm, class_lower)?;
        let raw = match ce.constants.get(name) {
            Some(v) => v.clone(),
            None => return reflection_class_get_constant(vm, class_lower, name),
        };
        (raw, ce.name.clone())
    };
    if !is_deferred_const_value(&raw) {
        return Some(raw);
    }
    // zend_update_class_constant() in the declaring class.
    match vm.class_constant_value(class_lower, name) {
        Some(Ok(v)) => return Some(v),
        Some(Err(())) => return None,
        None => {}
    }
    let cname_s = crate::string::msg_str(&cname).to_string();
    let simple = !cname_s.is_empty()
        && cname_s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '\\' || (c as u32) >= 0x80)
        && name
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c >= 0x80);
    if !simple {
        return Some(vm.resolve_deferred_value(&raw));
    }
    let source = format!(
        "<?php return \\{}::{};",
        cname_s.trim_start_matches('\\'),
        crate::string::msg_str(name)
    )
    .into_bytes();
    let mut lexer = goro_parser::Lexer::new(&source);
    let tokens = lexer.tokenize();
    let mut parser = goro_parser::Parser::new(tokens);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(_) => return Some(vm.resolve_deferred_value(&raw)),
    };
    let compiler = crate::compiler::Compiler::new();
    let op_array = match compiler.compile(&program) {
        Ok((op, _, _)) => op,
        Err(_) => return Some(vm.resolve_deferred_value(&raw)),
    };
    let cvs = vec![Value::Undef; op_array.cv_names.len()];
    vm.called_class_stack.push(class_lower.to_vec());
    vm.push_class_scope(class_lower.to_vec());
    let r = vm.execute_op_array_pub(&op_array, cvs);
    vm.called_class_stack.pop();
    vm.pop_class_scope();
    if vm.current_exception.is_some() {
        return None;
    }
    r.ok()
}

thread_local! {
    static ENUM_METHOD_CACHE: RefCell<std::collections::HashMap<(Vec<u8>, Vec<u8>), &'static crate::object::MethodDef>> =
        RefCell::new(std::collections::HashMap::new());
}

/// The engine-provided `cases()`/`from()`/`tryFrom()` of a user enum:
/// internal functions scoped to the enum (prototype UnitEnum/BackedEnum).
fn enum_builtin_method(
    vm: &Vm,
    enum_lower: &[u8],
    method_lower: &[u8],
) -> Option<&'static crate::object::MethodDef> {
    let ce = vm.classes.get(enum_lower)?;
    if !ce.is_enum {
        return None;
    }
    let iface: &[u8] = match method_lower {
        b"cases" => b"UnitEnum",
        b"from" | b"tryfrom" if ce.enum_backing_type.is_some() => b"BackedEnum",
        _ => return None,
    };
    if ce.methods.contains_key(method_lower) {
        return None;
    }
    let key = (enum_lower.to_vec(), method_lower.to_vec());
    if let Some(m) = ENUM_METHOD_CACHE.with(|c| c.borrow().get(&key).copied()) {
        return Some(m);
    }
    let (_, sig) = crate::stubs::find_method(iface, method_lower)?;
    let oa = crate::stubs::op_array_for(sig, Some(enum_lower), true);
    let m: &'static crate::object::MethodDef = Box::leak(Box::new(crate::object::MethodDef {
        name: sig.name.as_bytes().to_vec(),
        param_count: sig.args.len(),
        op_array: std::rc::Rc::new(oa),
        is_static: true,
        is_abstract: false,
        is_final: false,
        visibility: Visibility::Public,
        declaring_class: enum_lower.to_vec(),
        doc_comment: None,
        attributes: Vec::new(),
    }));
    ENUM_METHOD_CACHE.with(|c| c.borrow_mut().insert(key, m));
    Some(m)
}

thread_local! {
    static ENUM_PROP_CACHE: RefCell<std::collections::HashMap<Vec<u8>, &'static [crate::object::PropertyDef]>> =
        RefCell::new(std::collections::HashMap::new());
}

/// The implicit `name`/`value` properties of a user enum.
fn enum_builtin_props(vm: &Vm, lower: &[u8]) -> &'static [crate::object::PropertyDef] {
    let ce = match vm.classes.get(lower) {
        Some(c) if c.is_enum => c,
        _ => return &[],
    };
    if let Some(v) = ENUM_PROP_CACHE.with(|c| c.borrow().get(lower).copied()) {
        return v;
    }
    let mk = |name: &[u8], ty: &[u8]| crate::object::PropertyDef {
        name: name.to_vec(),
        default: Value::Undef,
        is_static: false,
        is_readonly: true,
        visibility: Visibility::Public,
        set_visibility: None,
        declaring_class: lower.to_vec(),
        property_type: Some(ParamType::Simple(ty.to_vec())),
        has_get_hook: false,
        has_set_hook: false,
        is_virtual: false,
        is_abstract: false,
        is_final: false,
        attributes: Vec::new(),
        doc_comment: None,
    };
    let mut v = Vec::new();
    if !ce.properties.iter().any(|p| p.name == b"name") {
        v.push(mk(b"name", b"string"));
    }
    if let Some(bt) = &ce.enum_backing_type {
        if !ce.properties.iter().any(|p| p.name == b"value") {
            v.push(mk(b"value", &bt.to_ascii_lowercase()));
        }
    }
    let leaked: &'static [crate::object::PropertyDef] = Box::leak(v.into_boxed_slice());
    ENUM_PROP_CACHE.with(|c| c.borrow_mut().insert(lower.to_vec(), leaked));
    leaked
}

/// The properties class `ce` (lowercase `lower`) declares itself, in
/// declaration order.
pub fn own_declared_props<'a>(
    ce: &'a crate::object::ClassEntry,
    lower: &[u8],
) -> Vec<&'a crate::object::PropertyDef> {
    let own = |p: &&crate::object::PropertyDef| p.declaring_class.eq_ignore_ascii_case(lower);
    if ce.own_prop_order.is_empty() {
        return ce.properties.iter().filter(own).collect();
    }
    let mut out: Vec<&crate::object::PropertyDef> = ce
        .own_prop_order
        .iter()
        .filter_map(|n| ce.properties.iter().filter(own).find(|p| &p.name == n))
        .collect();
    // (declared later, e.g. by a trait)
    for p in ce.properties.iter().filter(own) {
        if !out.iter().any(|x| std::ptr::eq(*x, p)) {
            out.push(p);
        }
    }
    out
}

/// All declared properties of a class, including the implicit enum
/// properties and those inherited from internal ancestors (which user
/// `ClassEntry`s don't copy).
pub(crate) fn rprops<'a>(vm: &'a Vm, lower: &[u8]) -> Vec<&'a crate::object::PropertyDef> {
    fn collect<'a>(vm: &'a Vm, lower: &[u8], depth: usize) -> Vec<&'a crate::object::PropertyDef> {
        let mut out: Vec<&'a crate::object::PropertyDef> = Vec::new();
        if depth > 64 {
            return out;
        }
        let ce = match rcls(vm, lower) {
            Some(c) => c,
            None => return out,
        };
        if stub_backed_class(vm, lower) {
            // Internal: the synthesized entry is already in Zend's order.
            out.extend(ce.properties.iter());
            return out;
        }
        if is_internal_class(vm, lower) {
            // An extension's internal class: it inherits its parent's
            // properties when registered, then declares its own (a
            // redeclared one moves to the end, zend_declare_typed_property()).
            // (an internal enum: zend_enum_register_props() first)
            out.extend(enum_builtin_props(vm, lower).iter());
            if let Some(parent) = &ce.parent {
                for pp in collect(vm, &parent.to_ascii_lowercase(), depth + 1) {
                    if pp.visibility == Visibility::Private
                        || ce.properties.iter().any(|x| {
                            x.name == pp.name && x.declaring_class.eq_ignore_ascii_case(lower)
                        })
                    {
                        continue;
                    }
                    let mine = ce.properties.iter().find(|x| x.name == pp.name);
                    out.push(mine.unwrap_or(pp));
                }
            }
            for p in &ce.properties {
                if !out.iter().any(|x| x.name == p.name) {
                    out.push(p);
                }
            }
            return out;
        }
        // Zend's properties_info order: the engine's enum props, the
        // class's own declarations, then inherited ones (parent's order).
        out.extend(enum_builtin_props(vm, lower).iter());
        for p in own_declared_props(ce, lower) {
            if !out.iter().any(|x| x.name == p.name) {
                out.push(p);
            }
        }
        if let Some(parent) = &ce.parent {
            for pp in collect(vm, &parent.to_ascii_lowercase(), depth + 1) {
                if out.iter().any(|x| x.name == pp.name) {
                    continue;
                }
                // Prefer the child's own copy (it carries inherited state).
                let mine = ce.properties.iter().find(|x| {
                    x.name == pp.name && x.declaring_class.eq_ignore_ascii_case(&pp.declaring_class)
                });
                out.push(mine.unwrap_or(pp));
            }
        }
        for p in &ce.properties {
            if !out.iter().any(|x| x.name == p.name) {
                out.push(p);
            }
        }
        out
    }
    collect(vm, &lower.to_ascii_lowercase(), 0)
}

/// Find a declared property (see `rprops`).
pub(crate) fn rprop<'a>(
    vm: &'a Vm,
    lower: &[u8],
    name: &[u8],
) -> Option<&'a crate::object::PropertyDef> {
    rprops(vm, lower).into_iter().find(|p| p.name == name)
}

/// Methods of a user or internal class in function-table order (including
/// methods inherited from internal ancestors), as
/// (name, visibility, declaring class lowercase). None if the class is unknown.
pub fn class_method_list(
    vm: &Vm,
    class_lower: &[u8],
) -> Option<Vec<(Vec<u8>, Visibility, Vec<u8>)>> {
    let lower = class_lower.to_ascii_lowercase();
    rcls(vm, &lower)?;
    Some(
        rmethods(vm, &lower)
            .into_iter()
            .filter(|m| !m.name.starts_with(b"__property_"))
            .map(|m| {
                (
                    m.name.clone(),
                    m.visibility,
                    m.declaring_class.to_ascii_lowercase(),
                )
            })
            .collect(),
    )
}

/// ReflectionClass::newInstance()/newInstanceArgs() of a class with a user
/// constructor (or none): object_init_ex(), then the constructor called
/// from internal code (zend_call_known_function: argument checks, no
/// "called in"). An object whose constructor failed is not destructed.
#[inline(never)]
fn reflection_new_instance(
    vm: &mut Vm,
    canonical: &[u8],
    class_lower: &[u8],
    args: Value,
) -> Value {
    let mut positional = Vec::new();
    let mut named: Vec<(Vec<u8>, Value)> = Vec::new();
    if let Value::Array(a) = &args {
        for (k, v) in a.borrow().iter() {
            match k {
                crate::array::ArrayKey::String(n) => named.push((n.as_bytes().to_vec(), v.clone())),
                _ => positional.push(v.clone()),
            }
        }
    }
    let ctor = rmethod(vm, class_lower, b"__construct").cloned();
    let has_dtor = rmethod(vm, class_lower, b"__destruct").is_some();
    let obj_id = vm.alloc_object_id();
    let mut new_obj = PhpObject::new(canonical.to_vec(), obj_id);
    if let Some(ce) = rcls(vm, class_lower) {
        let props: Vec<_> = ce
            .properties
            .iter()
            .filter(|p| !p.is_static)
            .map(|p| (p.name.clone(), p.default.clone()))
            .collect();
        for (name, default) in props {
            let resolved = vm.resolve_deferred_value(&default);
            new_obj.set_property(name, resolved);
        }
    }
    let obj = Rc::new(RefCell::new(new_obj));
    vm.run_object_create_hook(&obj);
    if has_dtor {
        vm.track_destructible(Value::Object(obj.clone()));
    }
    let Some(m) = ctor else {
        return Value::Object(obj);
    };
    if has_dtor {
        obj.borrow_mut()
            .set_property(b"__ctor_pending".to_vec(), Value::True);
    }
    // zend_missing_arg_error() for an internal caller
    let op = &m.op_array;
    let required = op.required_param_count as usize;
    if named.is_empty() && positional.len() < required {
        let implicit = usize::from(op.cv_names.first().map(|n| n.as_slice()) == Some(b"this"));
        let total = (op.param_count as usize)
            .saturating_sub(implicit)
            .saturating_sub(usize::from(op.variadic_param.is_some()));
        let q = if total > required {
            "at least"
        } else {
            "exactly"
        };
        let msg = format!(
            "Too few arguments to function {}::__construct(), {} passed and {} {} expected",
            crate::value::display_class_name(canonical),
            positional.len(),
            q,
            required
        );
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Value::Null;
    }
    vm.push_class_scope(m.declaring_class.to_ascii_lowercase());
    vm.called_class_stack.push(canonical.to_vec());
    let line = vm.current_line;
    let r = vm.execute_fn_with_named_args(
        &m.op_array,
        positional,
        named,
        Some(Value::Object(obj.clone())),
    );
    vm.current_line = line;
    vm.called_class_stack.pop();
    vm.pop_class_scope();
    if r.is_err() || vm.current_exception.is_some() {
        return Value::Null;
    }
    if has_dtor {
        obj.borrow_mut()
            .properties
            .retain(|(k, _)| k != b"__ctor_pending");
    }
    Value::Object(obj)
}

/// Compile and run a PHP snippet with the given local variables, inside the
/// scope of `scope_lower` (so private/protected access, hooks, magic
/// methods and type checks behave as for code in that class). Returns None
/// if it threw (the exception is left in `vm.current_exception`).
pub(crate) fn run_snippet_in_scope(
    vm: &mut Vm,
    src: &str,
    vars: &[(&str, Value)],
    scope_lower: &[u8],
) -> Option<Value> {
    run_snippet_in_scope_ex(vm, src, vars, scope_lower, false)
}

/// `run_snippet_in_scope`; with `internal_call`, the calls the snippet makes
/// are made from internal code ("[internal function]" frames, as
/// ReflectionMethod::invoke()'s call of the method).
fn run_snippet_in_scope_ex(
    vm: &mut Vm,
    src: &str,
    vars: &[(&str, Value)],
    scope_lower: &[u8],
    internal_call: bool,
) -> Option<Value> {
    let source = format!("<?php {}", src).into_bytes();
    let mut lexer = goro_parser::Lexer::new(&source);
    let tokens = lexer.tokenize();
    let mut parser = goro_parser::Parser::new(tokens);
    let program = parser.parse().ok()?;
    let compiler = crate::compiler::Compiler::new();
    let (mut op_array, _, _) = compiler.compile(&program).ok()?;
    // Errors raised by the snippet are reported at the caller's line.
    let line = vm.current_line;
    for op in op_array.ops.iter_mut() {
        op.line = if internal_call && op.opcode == crate::opcode::OpCode::DoFCall {
            0
        } else {
            line
        };
    }
    let mut cvs = vec![Value::Undef; op_array.cv_names.len()];
    for (name, val) in vars {
        if let Some(i) = op_array
            .cv_names
            .iter()
            .position(|n| n.as_slice() == name.as_bytes())
        {
            cvs[i] = val.clone();
        }
    }
    vm.called_class_stack.push(scope_lower.to_vec());
    vm.push_class_scope(scope_lower.to_vec());
    let r = vm.execute_op_array_pub(&op_array, cvs);
    vm.called_class_stack.pop();
    vm.pop_class_scope();
    if vm.current_exception.is_some() {
        return None;
    }
    r.ok()
}

/// Compile and run `src` as if it were code at `file`:`line` with the given
/// strict_types (the dummy frame of zend_get_attribute_object()).
pub(crate) fn run_snippet_at(
    vm: &mut Vm,
    src: &str,
    vars: &[(&str, Value)],
    file: Option<Vec<u8>>,
    line: u32,
    strict: bool,
) -> Option<Value> {
    let source = format!("<?php {}", src).into_bytes();
    let mut lexer = goro_parser::Lexer::new(&source);
    let tokens = lexer.tokenize();
    let mut parser = goro_parser::Parser::new(tokens);
    let program = parser.parse().ok()?;
    let compiler = crate::compiler::Compiler::new();
    let (mut op_array, _, _) = compiler.compile(&program).ok()?;
    for op in op_array.ops.iter_mut() {
        op.line = line;
    }
    op_array.strict_types = strict;
    if let Some(f) = &file {
        op_array.filename = f.clone();
    }
    let mut cvs = vec![Value::Undef; op_array.cv_names.len()];
    for (name, val) in vars {
        if let Some(i) = op_array
            .cv_names
            .iter()
            .position(|n| n.as_slice() == name.as_bytes())
        {
            cvs[i] = val.clone();
        }
    }
    let saved_file =
        file.map(|f| std::mem::replace(&mut vm.current_file, crate::string::msg_str(&f).into()));
    let saved_line = std::mem::replace(&mut vm.current_line, line);
    vm.called_class_stack.push(Vec::new());
    vm.push_class_scope(Vec::new());
    let r = vm.execute_op_array_pub(&op_array, cvs);
    vm.called_class_stack.pop();
    vm.pop_class_scope();
    if let Some(f) = saved_file {
        vm.current_file = f;
    }
    vm.current_line = saved_line;
    if vm.current_exception.is_some() {
        return None;
    }
    r.ok()
}

/// File, line and strict_types of the place an attribute is used.
fn attr_use_site(obj: &Rc<RefCell<PhpObject>>) -> (Option<Vec<u8>>, Option<u32>, bool) {
    let idx = obj
        .borrow()
        .get_property(b"__reflection_attr_args_op")
        .to_long() as usize;
    ATTR_ARG_OPS.with(|r| {
        r.borrow()
            .get(idx)
            .map(|op| {
                (
                    (!op.filename.is_empty()).then(|| op.filename.clone()),
                    (op.decl_line > 0).then_some(op.decl_line),
                    op.strict_types,
                )
            })
            .unwrap_or((None, None, false))
    })
}

/// Record a closure's bound `$this`, scope and called class on its
/// ReflectionFunction. A bound `$this` travels as the first captured value
/// when the closure's first CV is `$this`; it's removed from `captured`.
fn set_closure_scope_info(
    vm: &Vm,
    this: &Rc<RefCell<PhpObject>>,
    fn_name: &[u8],
    captured: &mut Vec<Value>,
) {
    let lower = fn_name.to_ascii_lowercase();
    let op = match vm.user_functions.get(&lower) {
        Some(op) => op,
        None => return,
    };
    let mut bound_this: Option<Value> = None;
    if op
        .cv_names
        .first()
        .map(|n| n.as_slice() == b"this")
        .unwrap_or(false)
    {
        if let Some(Value::Object(_)) = captured.first() {
            bound_this = Some(captured.remove(0));
        }
    }
    let scope = op.scope_class.clone();
    let mut ob = this.borrow_mut();
    if let Some(t) = &bound_this {
        ob.set_property(b"__reflection_closure_this".to_vec(), t.clone());
    }
    if let Some(sc) = scope {
        let scope_name = rcls(vm, &sc).map(|c| c.name.clone()).unwrap_or(sc);
        let called = match &bound_this {
            Some(Value::Object(o)) => o.borrow().class_name.clone(),
            _ => scope_name.clone(),
        };
        ob.set_property(
            b"__reflection_closure_scope".to_vec(),
            Value::String(PhpString::from_vec(scope_name)),
        );
        ob.set_property(
            b"__reflection_closure_called".to_vec(),
            Value::String(PhpString::from_vec(called)),
        );
    }
}

/// ReflectionClass constructor: sets up __reflection_target and name properties
fn reflection_class_construct_impl(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };
    // Determine which reflector class we're constructing for error messages.
    let reflector_class_name: String = {
        let ob = this.borrow();
        crate::string::msg_str(&ob.class_name).to_string()
    };

    // User-provided args (args[0] is $this)
    let user_arg_count = args.len().saturating_sub(1);
    if user_arg_count == 0 {
        let err_msg = format!(
            "{}::__construct() expects exactly 1 argument, 0 given",
            reflector_class_name
        );
        let exc = vm.create_exception(b"ArgumentCountError", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }
    if user_arg_count > 1 {
        let err_msg = format!(
            "{}::__construct() expects exactly 1 argument, {} given",
            reflector_class_name, user_arg_count
        );
        let exc = vm.create_exception(b"ArgumentCountError", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let arg = args.get(1).cloned().unwrap_or(Value::Null);

    // ReflectionObject::__construct(object $object)
    let rc_lower = reflector_class_name.to_ascii_lowercase();
    let is_reflection_object = rc_lower == "reflectionobject"
        || vm.class_extends(rc_lower.as_bytes(), b"reflectionobject");
    if is_reflection_object && !matches!(arg.deref(), Value::Object(_) | Value::Generator(_)) {
        let err_msg = format!(
            "ReflectionObject::__construct(): Argument #1 ($object) must be of type object, {} given",
            Vm::value_type_name(&arg.deref())
        );
        let exc = vm.throw_type_error(err_msg);
        vm.current_exception = Some(exc);
        return false;
    }

    // Reject array/non-stringable arguments before we try to stringify.
    match &arg {
        Value::Array(_) => {
            let err_msg = format!(
                "{}::__construct(): Argument #1 ($objectOrClass) must be of type object|string, array given",
                reflector_class_name
            );
            let exc = vm.create_exception(b"TypeError", &err_msg, line);
            vm.current_exception = Some(exc);
            return false;
        }
        Value::Null => {
            // Deprecated warning, then proceed with null -> empty string (-> "Class \"\" does not exist")
            vm.emit_deprecated_at(
                &format!("{}::__construct(): Passing null to parameter #1 ($objectOrClass) of type object|string is deprecated",
                    reflector_class_name), line);
        }
        _ => {}
    }

    let class_name = match &arg {
        Value::Object(obj) => {
            let ob = obj.borrow();
            crate::string::msg_str(&ob.class_name).to_string()
        }
        Value::String(s) if std::str::from_utf8(s.as_bytes()).is_err() => {
            // (a class name that is not UTF-8: looked up by its bytes)
            let raw = s.as_bytes();
            let raw = raw.strip_prefix(b"\\").unwrap_or(raw);
            let l = raw.to_ascii_lowercase();
            if !vm.classes.contains_key(&l) {
                vm.try_autoload_class(raw);
                if vm.current_exception.is_some() {
                    return false;
                }
            }
            if let Some(ce) = vm.classes.get(&l) {
                let name = Value::String(PhpString::from_vec(ce.name.clone()));
                let mut obj = this.borrow_mut();
                obj.set_property(b"name".to_vec(), name.clone());
                obj.set_property(b"__reflection_target".to_vec(), name);
                return true;
            }
            s.to_msg_string()
        }
        Value::String(s) => {
            let name = s.to_msg_string();
            // Closure strings should map to "Closure" class
            if name.starts_with("__closure_")
                || name.starts_with("__arrow_")
                || name.starts_with("__bound_closure_")
                || name.starts_with("__closure_fcc_")
            {
                "Closure".to_string()
            } else {
                name
            }
        }
        Value::Generator(_) => "Generator".to_string(),
        _ => arg.to_php_string().to_msg_string(),
    };

    // Strip leading backslash (the error message names the class as given)
    let given_name = class_name.clone();
    let class_name = if let Some(stripped) = class_name.strip_prefix('\\') {
        stripped.to_string()
    } else {
        class_name
    };
    let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&class_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();

    // Check if class exists
    let mut class_exists =
        vm.classes.contains_key(&class_lower) || vm.is_known_builtin_class(&class_lower);

    // If class is unknown, try autoloading
    if !class_exists {
        vm.try_autoload_class(&crate::errdisplay::raw_error_bytes(&class_name));
        // An exception thrown by the autoloader propagates (zend_lookup_class).
        if vm.current_exception.is_some() {
            return false;
        }
        class_exists =
            vm.classes.contains_key(&class_lower) || vm.is_known_builtin_class(&class_lower);
    }

    if !class_exists {
        let err_msg = format!(
            "Class \"{}\" does not exist",
            crate::string::msg_cstr(given_name.as_bytes())
        );
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    // ReflectionEnum requires the target class actually be an enum.
    if reflector_class_name.eq_ignore_ascii_case("ReflectionEnum") {
        let is_enum = rcls(vm, &class_lower).map(|c| c.is_enum).unwrap_or(false);
        if !is_enum {
            let canonical = rcls(vm, &class_lower)
                .map(|c| crate::string::msg_str(&c.name).to_string())
                .unwrap_or(class_name.clone());
            let err_msg = format!("Class \"{}\" is not an enum", canonical);
            let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
            vm.current_exception = Some(exc);
            return false;
        }
    }

    // Get canonical name
    let canonical = if let Some(ce) = rcls(vm, &class_lower) {
        crate::string::msg_str(&ce.name).to_string()
    } else {
        // Built-in class - use proper casing
        vm.builtin_canonical_name(&class_lower)
    };

    let mut obj = this.borrow_mut();
    obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_msg(&canonical.clone())),
    );
    obj.set_property(
        b"__reflection_target".to_vec(),
        Value::String(PhpString::from_msg(&canonical)),
    );
    // If constructed from an object, a ReflectionObject keeps the object
    // (intern->obj); a ReflectionClass keeps only its class.
    if let Value::Object(_) = &arg {
        if arg.is_closure_obj() {
            let (fname, _) = crate::closure::closure_parts(&closure_legacy_arg(&arg));
            obj.set_property(
                b"__reflection_closure_fn".to_vec(),
                Value::String(PhpString::from_vec(fname)),
            );
        }
        if is_reflection_object {
            obj.set_property(b"__reflection_object".to_vec(), arg);
        }
    } else if let Value::Generator(_) = &arg {
        if is_reflection_object {
            obj.set_property(b"__reflection_object".to_vec(), arg);
        }
    } else if let Value::String(s) = &arg {
        // Closure represented as a string: remember the underlying function.
        let name = s.to_msg_string();
        if name.starts_with("__closure_")
            || name.starts_with("__arrow_")
            || name.starts_with("__bound_closure_")
            || name.starts_with("__closure_fcc_")
        {
            obj.set_property(
                b"__reflection_closure_fn".to_vec(),
                Value::String(s.clone()),
            );
        }
    } else if let Value::Array(arr) = &arg {
        // Closure stored as [func_name, $this, ...]
        let first = arr.borrow().iter().next().map(|(_, v)| v.clone());
        if let Some(Value::String(s)) = first {
            let name = s.to_msg_string();
            if name.starts_with("__closure_")
                || name.starts_with("__arrow_")
                || name.starts_with("__bound_closure_")
                || name.starts_with("__closure_fcc_")
            {
                obj.set_property(b"__reflection_closure_fn".to_vec(), Value::String(s));
            }
        }
    }
    true
}

/// ReflectionMethod constructor
fn reflection_method_construct_impl(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };

    // Validate arg count (1 or 2 user args expected).
    let user_arg_count = args.len().saturating_sub(1);
    if user_arg_count == 0 {
        let err =
            "ReflectionMethod::__construct() expects at least 1 argument, 0 given".to_string();
        let exc = vm.create_exception(b"ArgumentCountError", &err, line);
        vm.current_exception = Some(exc);
        return false;
    }
    if user_arg_count > 2 {
        let err = format!(
            "ReflectionMethod::__construct() expects at most 2 arguments, {} given",
            user_arg_count
        );
        let exc = vm.create_exception(b"ArgumentCountError", &err, line);
        vm.current_exception = Some(exc);
        return false;
    }
    // (instantiate_reflection_method(): the 1-argument form's deprecation
    // comes before zpp)
    let one_arg = user_arg_count == 1 && !CLOSURE_INVOKE_CTOR.with(|c| c.get());
    if one_arg && !ONE_ARG_DEPRECATION_DONE.with(|c| c.replace(true)) {
        let dep_msg = "Calling ReflectionMethod::__construct() with 1 argument is deprecated, use ReflectionMethod::createFromMethodName() instead";
        vm.emit_deprecated_raw(dep_msg, line);
        if vm.current_exception.is_some() {
            ONE_ARG_DEPRECATION_DONE.with(|c| c.set(false));
            return false;
        }
    }
    let r = reflection_method_construct_args(vm, &this, args, line, one_arg);
    ONE_ARG_DEPRECATION_DONE.with(|c| c.set(false));
    r
}

thread_local! {
    /// The 1-argument deprecation of the ReflectionMethod construction in
    /// progress was raised (a nested construction does not raise it again).
    static ONE_ARG_DEPRECATION_DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn reflection_method_construct_args(
    vm: &mut Vm,
    this: &Rc<RefCell<PhpObject>>,
    args: &[Value],
    line: u32,
    one_arg: bool,
) -> bool {
    let this = this.clone();
    if one_arg {
        match args.get(1).map(|v| v.deref()) {
            Some(Value::Null) => {
                vm.emit_deprecated_at(
                    "ReflectionMethod::__construct(): Passing null to parameter #1 ($objectOrMethod) of type object|string is deprecated",
                    line,
                );
                if vm.current_exception.is_some() {
                    return false;
                }
            }
            Some(Value::Object(_)) => {
                let exc = vm.create_exception(
                    b"ValueError",
                    "ReflectionMethod::__construct(): Argument #2 ($method) cannot be null when argument #1 ($objectOrMethod) is an object",
                    line,
                );
                vm.current_exception = Some(exc);
                return false;
            }
            _ => {}
        }
    }

    if args.get(1).map(|a| a.is_closure_obj()).unwrap_or(false) {
        let invoke = args.get(2).is_some_and(|m| {
            m.to_php_string()
                .as_bytes()
                .eq_ignore_ascii_case(b"__invoke")
        });
        if let (true, Some((decl, m))) = (invoke, internal_method_closure(vm, &args[1])) {
            return internal_method_invoke_rm_construct(vm, &this, &decl, &m, line);
        }
        if let (true, Some(f)) = (invoke, internal_function_closure(vm, &args[1])) {
            let mut obj = this.borrow_mut();
            for (k, v) in [
                (&b"name"[..], &b"__invoke"[..]),
                (b"class", b"Closure"),
                (b"__reflection_class", b"Closure"),
                (b"__reflection_method", b"__invoke"),
            ] {
                obj.set_property(k.to_vec(), Value::String(PhpString::from_bytes(v)));
            }
            obj.set_property(
                b"__reflection_target".to_vec(),
                Value::String(PhpString::from_vec(f)),
            );
            obj.set_property(b"__reflection_invoke_internal".to_vec(), Value::True);
            obj.set_property(b"__reflection_invoke_fn".to_vec(), Value::True);
            return true;
        }
        let mut a = args.to_vec();
        a[1] = closure_legacy_arg(&args[1]);
        // The __invoke() of a closure of a function or method is
        // zend_get_closure_invoke_method(): an internal Closure::__invoke
        // with that function's signature.
        if let (Value::String(f), Some(m)) = (a[1].clone(), args.get(2)) {
            let fb = f.as_bytes();
            if m.to_php_string()
                .as_bytes()
                .eq_ignore_ascii_case(b"__invoke")
                && !fb.starts_with(b"__")
            {
                {
                    a[1] = Value::String(PhpString::from_bytes(b"Closure"));
                    CLOSURE_INVOKE_CTOR.with(|c| c.set(true));
                    let ok = reflection_method_construct(vm, &a, line);
                    CLOSURE_INVOKE_CTOR.with(|c| c.set(false));
                    if ok {
                        this.borrow_mut().set_property(
                            b"__reflection_closure_fn".to_vec(),
                            Value::String(PhpString::from_vec(fb.to_ascii_lowercase())),
                        );
                    }
                    return ok;
                }
            }
        }
        return reflection_method_construct(vm, &a, line);
    }
    // zpp: object|string $objectOrMethod, ?string $method.
    let is_closure_arr = |v: &Value| match v {
        Value::Array(a) => matches!(a.borrow().get_int(0), Some(Value::String(s))
            if s.as_bytes().starts_with(b"__closure_")
                || s.as_bytes().starts_with(b"__arrow_")
                || s.as_bytes().starts_with(b"__bound_closure_")),
        _ => false,
    };
    if let Some(a @ Value::Array(_)) = args.get(1) {
        if !is_closure_arr(a) {
            let exc = vm.create_exception(
                b"TypeError",
                "ReflectionMethod::__construct(): Argument #1 ($objectOrMethod) must be of type object|string, array given",
                line,
            );
            vm.current_exception = Some(exc);
            return false;
        }
    }
    if let Some(Value::Array(_)) = args.get(2) {
        let exc = vm.create_exception(
            b"TypeError",
            "ReflectionMethod::__construct(): Argument #2 ($method) must be of type ?string, array given",
            line,
        );
        vm.current_exception = Some(exc);
        return false;
    }
    // Extract the closure's underlying function name, if any, from the
    // first argument. Supports string-form closures and array callables of
    // the form [closure_obj, 'method'] that flatten to a string closure.
    let mut closure_fn: Option<PhpString> = None;
    if let Some(first) = args.get(1) {
        match first {
            Value::String(s) => {
                let name = s.to_msg_string();
                if name.starts_with("__closure_")
                    || name.starts_with("__arrow_")
                    || name.starts_with("__bound_closure_")
                    || name.starts_with("__closure_fcc_")
                {
                    closure_fn = Some(s.clone());
                }
            }
            Value::Array(arr) => {
                let af = arr.borrow().iter().next().map(|(_, v)| v.clone());
                if let Some(Value::String(s)) = af {
                    let name = s.to_msg_string();
                    if name.starts_with("__closure_")
                        || name.starts_with("__arrow_")
                        || name.starts_with("__bound_closure_")
                        || name.starts_with("__closure_fcc_")
                    {
                        closure_fn = Some(s);
                    }
                }
            }
            _ => {}
        }
    }

    let (class_name, method_name) = if args.len() >= 3 {
        // new ReflectionMethod($class, $method)
        let class_arg = &args[1];
        let method_arg = args[2].to_php_string().to_msg_string();
        let class_str = match class_arg {
            Value::Object(obj) => {
                let ob = obj.borrow();
                crate::string::msg_str(&ob.class_name).to_string()
            }
            Value::String(s) => {
                let name = s.to_msg_string();
                // Closure strings map to "Closure" class
                if name.starts_with("__closure_")
                    || name.starts_with("__arrow_")
                    || name.starts_with("__bound_closure_")
                    || name.starts_with("__closure_fcc_")
                {
                    "Closure".to_string()
                } else {
                    name
                }
            }
            Value::Array(arr) => {
                // Array callable: ['Class', 'method'] or [$obj, 'method']
                let first = arr.borrow().iter().next().map(|(_, v)| v.clone());
                match first {
                    Some(Value::Object(o)) => {
                        crate::string::msg_str(&o.borrow().class_name).to_string()
                    }
                    Some(Value::String(s)) => {
                        let name = s.to_msg_string();
                        if name.starts_with("__closure_")
                            || name.starts_with("__arrow_")
                            || name.starts_with("__bound_closure_")
                            || name.starts_with("__closure_fcc_")
                        {
                            "Closure".to_string()
                        } else {
                            name
                        }
                    }
                    _ => class_arg.to_php_string().to_msg_string(),
                }
            }
            _ => class_arg.to_php_string().to_msg_string(),
        };
        (class_str, method_arg)
    } else if args.len() >= 2 {
        // new ReflectionMethod('Class::method') (its deprecation was raised
        // first)
        let arg = args[1].to_php_string().to_msg_string();
        if let Some(pos) = arg.find("::") {
            (arg[..pos].to_string(), arg[pos + 2..].to_string())
        } else {
            let err_msg = "ReflectionMethod::__construct(): Argument #1 ($objectOrMethod) must be a valid method name".to_string();
            let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
            vm.current_exception = Some(exc);
            return false;
        }
    } else {
        return true;
    };

    // (zend_lookup_class(): a leading backslash is not part of the name)
    let lookup_name = class_name.strip_prefix('\\').unwrap_or(&class_name);
    let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&lookup_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    let method_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&method_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();

    // Check if class exists (user-defined or built-in); zend_lookup_class()
    // runs the autoloaders.
    let mut is_user_class = vm.classes.contains_key(&class_lower);
    let mut is_builtin_class = vm.is_known_builtin_class(&class_lower);
    if !is_user_class && !is_builtin_class {
        vm.try_autoload_class(&crate::errdisplay::raw_error_bytes(&lookup_name));
        if vm.current_exception.is_some() {
            return false;
        }
        is_user_class = vm.classes.contains_key(&class_lower);
        is_builtin_class = vm.is_known_builtin_class(&class_lower);
    }
    if !is_user_class && !is_builtin_class {
        let err_msg = format!("Class \"{}\" does not exist", class_name);
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let found = rmethod(vm, &class_lower, &method_lower).map(|m| {
        (
            crate::string::msg_str(&m.name).to_string(),
            m.declaring_class.clone(),
        )
    });
    // Internal classes without stub data accept any method name.
    let has_info = is_user_class || rcls(vm, &class_lower).is_some();
    // (Closure::__invoke exists only for a closure object)
    let closure_invoke = class_lower == b"closure"
        && method_lower == b"__invoke"
        && (closure_fn.is_some() || CLOSURE_INVOKE_CTOR.with(|c| c.get()));
    if found.is_none() && has_info && !closure_invoke {
        let canonical_class = rcls(vm, &class_lower)
            .map(|c| crate::string::msg_str(&c.name).to_string())
            .unwrap_or(class_name.clone());
        let err_msg = format!(
            "Method {}::{}() does not exist",
            canonical_class, method_name
        );
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let canonical_class = match &found {
        Some((_, decl)) => rcls(vm, decl)
            .map(|c| crate::string::msg_str(&c.name).to_string())
            .unwrap_or_else(|| crate::string::msg_str(decl).to_string()),
        None if is_user_class => rcls(vm, &class_lower)
            .map(|c| crate::string::msg_str(&c.name).to_string())
            .unwrap_or(class_name.clone()),
        None => vm.builtin_canonical_name(&class_lower),
    };
    let lookup_class = rcls(vm, &class_lower)
        .map(|c| crate::string::msg_str(&c.name).to_string())
        .unwrap_or_else(|| canonical_class.clone());

    let canonical_method = match &found {
        Some((n, _)) => n.clone(),
        None => method_name.clone(),
    };

    let mut obj = this.borrow_mut();
    obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_msg(&canonical_method)),
    );
    obj.set_property(
        b"class".to_vec(),
        Value::String(PhpString::from_msg(&canonical_class.clone())),
    );
    obj.set_property(
        b"__reflection_class".to_vec(),
        Value::String(PhpString::from_msg(&canonical_class.clone())),
    );
    obj.set_property(
        b"__reflection_method".to_vec(),
        Value::String(PhpString::from_vec(method_lower)),
    );
    if !lookup_class.eq_ignore_ascii_case(&canonical_class) {
        obj.set_property(
            b"__reflection_lookup_class".to_vec(),
            Value::String(PhpString::from_msg(&lookup_class)),
        );
    }
    if let Some(fn_str) = closure_fn {
        obj.set_property(b"__reflection_closure_fn".to_vec(), Value::String(fn_str));
    }
    true
}

/// The attributes (and their scope) of the function behind a closure
/// (`__reflection_closure_fn`: "class::method" or a function name).
fn closure_fn_attributes(vm: &Vm, fn_lower: &[u8]) -> (Vec<RuntimeAttribute>, Option<Vec<u8>>) {
    if let Some(p) = fn_lower.windows(2).position(|w| w == b"::") {
        if let Some(m) = rmethod(vm, &fn_lower[..p], &fn_lower[p + 2..]) {
            return (m.attributes.clone(), Some(m.declaring_class.clone()));
        }
    }
    match rfunc(vm, fn_lower) {
        Some(op) => (op.attributes.clone(), op.scope_class.clone()),
        None => (Vec::new(), None),
    }
}

thread_local! {
    /// Set while constructing the Closure::__invoke of a closure object.
    static CLOSURE_INVOKE_CTOR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The internal-callable form of a Closure object used by the reflection
/// code paths: user closures keep their `__closure_N` name (or capture
/// array); method closures become their "Class::method" name.
pub(crate) fn closure_legacy_arg(v: &Value) -> Value {
    let v = v.deref();
    // `[$closure, '__invoke']`
    if let Value::Array(arr) = &v {
        let a = arr.borrow();
        if a.len() == 2 {
            if let (Some(c), Some(Value::String(m))) = (a.get_int(0), a.get_int(1)) {
                if c.is_closure_obj() && m.as_bytes().eq_ignore_ascii_case(b"__invoke") {
                    let c = c.deref();
                    drop(a);
                    return closure_legacy_arg(&c);
                }
            }
        }
    }
    match v.closure_inner() {
        Some(Value::Array(a)) => {
            let first = a.borrow().get_int(0).cloned();
            match first {
                Some(Value::String(s))
                    if !s.as_bytes().starts_with(b"__")
                        && s.as_bytes().windows(2).any(|w| w == b"::") =>
                {
                    Value::String(s)
                }
                _ => Value::Array(a),
            }
        }
        Some(i) => i,
        None => v,
    }
}

/// The __call()/__callStatic() behind a ReflectionFunction of a trampoline.
fn magic_target(obj: &Rc<RefCell<PhpObject>>) -> Option<Vec<u8>> {
    match obj.borrow().get_property(b"__reflection_magic") {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

/// For a closure created from a method that only exists through
/// __call()/__callStatic() (zend_get_call_trampoline_func): the magic
/// method ("class::__call", lowercase) and the called method's name.
fn magic_call_trampoline(vm: &Vm, closure: &Value) -> Option<(Vec<u8>, Vec<u8>)> {
    // A first-class-callable thunk names its target "Class::method"; an
    // object among the captured values makes it an instance call.
    let op = vm.closure_op_array(closure);
    // `$obj->method(...)`: the thunk calls a literal method on the captured
    // object.
    let method_fcc = op.as_ref().and_then(|op| {
        let call = op
            .ops
            .iter()
            .find(|o| o.opcode == crate::opcode::OpCode::InitMethodCall)?;
        let crate::opcode::OperandType::Const(i) = call.op2 else {
            return None;
        };
        let m = op.literals.get(i as usize)?.to_php_string();
        let obj = match closure.closure_inner()? {
            Value::Array(a) => a.borrow().values().find_map(|v| match v.deref() {
                Value::Object(o) => Some(o.borrow().class_name.clone()),
                _ => None,
            })?,
            _ => return None,
        };
        let mut t = obj;
        t.extend_from_slice(b"::");
        t.extend_from_slice(m.as_bytes());
        Some(t)
    });
    let target_name = match op.and_then(|op| op.fcc_target_name.clone()).or(method_fcc) {
        Some(t) => t,
        None => match closure.closure_inner()? {
            Value::String(s) => s.as_bytes().to_vec(),
            // [$object or "Class", "method"]
            // ["Class::method", $this] (a method bound to an object)
            Value::Array(a)
                if matches!(a.borrow().get_int(0).map(|v| v.deref()), Some(Value::String(s))
                    if s.as_bytes().windows(2).any(|w| w == b"::")) =>
            {
                a.borrow()
                    .get_int(0)?
                    .deref()
                    .to_php_string()
                    .as_bytes()
                    .to_vec()
            }
            Value::Array(a) => {
                let a = a.borrow();
                let mut t = match a.get_int(0)?.deref() {
                    Value::Object(o) => o.borrow().class_name.clone(),
                    Value::String(s) => s.as_bytes().to_vec(),
                    _ => return None,
                };
                t.extend_from_slice(b"::");
                t.extend_from_slice(a.get_int(1)?.deref().to_php_string().as_bytes());
                t
            }
            _ => return None,
        },
    };
    let p = target_name.windows(2).position(|w| w == b"::")?;
    let (class, method) = (target_name[..p].to_vec(), target_name[p + 2..].to_vec());
    let is_static = !match closure.closure_inner() {
        Some(Value::Array(a)) => a
            .borrow()
            .values()
            .any(|v| matches!(v.deref(), Value::Object(_))),
        _ => false,
    };
    let cl = class
        .strip_prefix(b"\\")
        .unwrap_or(&class)
        .to_ascii_lowercase();
    let ce = vm.classes.get(&cl)?;
    if ce
        .methods
        .contains_key(method.to_ascii_lowercase().as_slice())
    {
        return None;
    }
    let magic: &[u8] = if !is_static && ce.methods.contains_key(b"__call".as_slice()) {
        b"__call"
    } else if ce.methods.contains_key(b"__callstatic".as_slice()) {
        b"__callstatic"
    } else {
        return None;
    };
    let mut target = cl;
    target.extend_from_slice(b"::");
    target.extend_from_slice(magic);
    Some((target, method))
}

/// The internal method (declaring class, method name) a closure is over:
/// `$obj->m(...)`, `C::m(...)` of an internal method.
pub(crate) fn internal_method_closure(vm: &Vm, closure: &Value) -> Option<(Vec<u8>, Vec<u8>)> {
    let inner = closure.closure_inner()?;
    let (name, _) = crate::closure::closure_parts(&inner);
    if name.starts_with(b"__") {
        return None;
    }
    let p = name.windows(2).position(|w| w == b"::")?;
    let cl = name[..p].to_ascii_lowercase();
    let (sc, sm) = vm.internal_method_of(&cl, &name[p + 2..].to_ascii_lowercase())?;
    Some((sc.name.as_bytes().to_vec(), sm.name.as_bytes().to_vec()))
}

/// The internal function (lowercase name) a closure is over (`strlen(...)`).
pub(crate) fn internal_function_closure(vm: &Vm, closure: &Value) -> Option<Vec<u8>> {
    match closure.closure_inner()? {
        Value::String(s) => {
            let l = s.as_bytes().to_ascii_lowercase();
            (!l.starts_with(b"__")
                && !l.contains(&b':')
                && vm.functions.contains_key(&l)
                && !vm.user_functions.contains_key(&l))
            .then_some(l)
        }
        _ => None,
    }
}

/// ReflectionFunction of a closure of an internal method: set up like a
/// ReflectionMethod of it (whose implementation answers the function
/// queries, see internal_method_rf), named after the method.
#[inline(never)]
fn internal_method_rf_construct(
    vm: &mut Vm,
    this: &Rc<RefCell<PhpObject>>,
    closure: Value,
    decl: &[u8],
    mname: &[u8],
    line: u32,
) -> bool {
    let a = [
        Value::Object(this.clone()),
        Value::String(PhpString::from_bytes(decl)),
        Value::String(PhpString::from_bytes(mname)),
    ];
    if !reflection_method_construct(vm, &a, line) {
        return false;
    }
    let mut obj = this.borrow_mut();
    obj.remove_property(b"class");
    obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_bytes(mname)),
    );
    obj.set_property(b"__reflection_internal_method".to_vec(), Value::True);
    obj.set_property(b"__reflection_is_closure".to_vec(), Value::True);
    obj.set_property(b"__reflection_closure_obj".to_vec(), closure);
    true
}

/// ReflectionClass of Closure without a closure object: getMethod() /
/// getMethods() make a temporary Closure for its `__invoke` (its handle,
/// taken before the ReflectionMethod's and freed after).
#[inline(never)]
fn reflected_closure_tmp_handle(vm: &mut Vm, refl: &Rc<RefCell<PhpObject>>) -> Option<u64> {
    let has_obj = matches!(
        refl.borrow().get_property(b"__reflection_object"),
        Value::Object(_)
    );
    (!has_obj).then(|| vm.alloc_object_id())
}

/// ReflectionMethod of the `__invoke` of a closure of an internal method
/// (zend_get_closure_invoke_method(): a public Closure::__invoke with the
/// method's signature): set up like a ReflectionMethod of the method, which
/// answers the signature queries (see internal_invoke_rm).
#[inline(never)]
fn internal_method_invoke_rm_construct(
    vm: &mut Vm,
    this: &Rc<RefCell<PhpObject>>,
    decl: &[u8],
    mname: &[u8],
    line: u32,
) -> bool {
    let a = [
        Value::Object(this.clone()),
        Value::String(PhpString::from_bytes(decl)),
        Value::String(PhpString::from_bytes(mname)),
    ];
    if !reflection_method_construct(vm, &a, line) {
        return false;
    }
    let mut obj = this.borrow_mut();
    obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_bytes(b"__invoke")),
    );
    obj.set_property(
        b"class".to_vec(),
        Value::String(PhpString::from_bytes(b"Closure")),
    );
    obj.set_property(b"__reflection_invoke_internal".to_vec(), Value::True);
    true
}

/// ReflectionMethod methods of a Closure::__invoke made by
/// internal_method_invoke_rm_construct(); None for the signature queries
/// (answered for the closure's method).
#[inline(never)]
fn internal_invoke_rm(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
    args: Option<&[Value]>,
) -> Option<Value> {
    let b = |v: bool| if v { Value::True } else { Value::False };
    // (the __invoke of a closure of an internal function: its signature
    // queries are ReflectionFunction's of it)
    let of_fn = obj.borrow().has_property(b"__reflection_invoke_fn");
    if of_fn {
        match method {
            b"getparameters"
            | b"getnumberofparameters"
            | b"getnumberofrequiredparameters"
            | b"getreturntype"
            | b"hasreturntype"
            | b"gettentativereturntype"
            | b"hastentativereturntype"
            | b"isvariadic"
            | b"returnsreference"
            | b"isdeprecated" => {
                return reflection_function_method(vm, method, obj).or_else(|| {
                    reflection_function_docall(vm, method, &[Value::Object(obj.clone())])
                });
            }
            _ => {}
        }
    }
    match method {
        b"getdeclaringclass" => Some(create_reflection_class(vm, "Closure")),
        b"isstatic" | b"isabstract" | b"isfinal" | b"isprivate" | b"isprotected"
        | b"isconstructor" | b"isdestructor" | b"isuserdefined" => Some(Value::False),
        b"ispublic" | b"isinternal" => Some(Value::True),
        b"getmodifiers" => Some(Value::Long(1)),
        b"getextension" => Some(Value::Null),
        b"getextensionname" => Some(Value::False),
        b"getdoccomment" | b"getfilename" | b"getstartline" | b"getendline" => Some(Value::False),
        b"hasprototype" => Some(b(false)),
        b"invoke" | b"invokeargs" => {
            let a = args?;
            let target = a.get(1).filter(|t| t.is_closure_obj())?.deref();
            let call: Vec<Value> = if method == b"invoke" {
                a.iter().skip(2).cloned().collect()
            } else {
                match a.get(2) {
                    Some(Value::Array(arr)) => arr.borrow().values().cloned().collect(),
                    _ => Vec::new(),
                }
            };
            Some(vm.call_callback(&target, &call).unwrap_or(Value::Null))
        }
        b"__tostring" => {
            obj.borrow_mut()
                .remove_property(b"__reflection_invoke_internal");
            let me = [Value::Object(obj.clone())];
            let s = if of_fn {
                reflection_function_method(vm, method, obj)
                    .or_else(|| reflection_function_docall(vm, method, &me))
            } else {
                reflection_method_method(vm, method, obj)
                    .or_else(|| reflection_method_docall(vm, method, &me))
            };
            obj.borrow_mut()
                .set_property(b"__reflection_invoke_internal".to_vec(), Value::True);
            let s = s?.to_php_string().to_msg_string();
            let (head, rest) = s.split_once('\n').unwrap_or((s.as_str(), ""));
            let proto = head
                .split_once('<')
                .and_then(|(_, h)| h.split_once('>'))
                .and_then(|(attrs, _)| {
                    attrs
                        .split(", ")
                        .find(|p| p.starts_with("prototype "))
                        .map(|p| p.to_string())
                });
            let mut out = String::from("Method [ <internal");
            if let Some(p) = proto {
                out.push_str(", ");
                out.push_str(&p);
            }
            out.push_str("> public method __invoke ] {\n");
            out.push_str(rest);
            Some(Value::String(PhpString::from_msg(&out)))
        }
        _ => None,
    }
}

/// ReflectionFunction methods of a closure of an internal method (see
/// internal_method_rf_construct): the function queries are
/// ReflectionMethod's; None for the closure ones ReflectionFunction answers.
#[inline(never)]
fn internal_method_rf(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
    args: Option<&[Value]>,
) -> Option<Value> {
    match method {
        b"getname"
        | b"isclosure"
        | b"getclosurethis"
        | b"getclosurescopeclass"
        | b"getclosurecalledclass"
        | b"getclosureusedvariables"
        | b"getclosure"
        | b"isanonymous"
        | b"invoke"
        | b"invokeargs"
        | b"isdisabled" => None,
        b"__tostring" => {
            let s = reflection_method_method(vm, method, obj)
                .or_else(|| reflection_method_docall(vm, method, &[Value::Object(obj.clone())]))?;
            let b = s.to_php_string();
            match b.as_bytes().strip_prefix(b"Method [") {
                Some(rest) => {
                    // The closure's function has no class context (no
                    // inherits/overwrites), and a fake closure's flags have
                    // ZEND_ACC_PUBLIC added to a non-public method's.
                    let text = crate::string::msg_str(rest).into_owned();
                    let (head, tail) = text.split_once('\n').unwrap_or((&text, ""));
                    let mut head = head.to_string();
                    for kw in [", inherits ", ", overwrites "] {
                        if let Some(i) = head.find(kw) {
                            let end = head[i + 2..]
                                .find([',', '>'])
                                .map(|j| i + 2 + j)
                                .unwrap_or(head.len());
                            head.replace_range(i..end, "");
                        }
                    }
                    for v in ["> protected method ", "> private method "] {
                        head = head.replacen(v, "> <visibility error> method ", 1);
                    }
                    let mut out = b"Closure [".to_vec();
                    out.extend_from_slice(head.as_bytes());
                    if text.contains('\n') {
                        out.push(b'\n');
                        out.extend_from_slice(tail.as_bytes());
                    }
                    Some(Value::String(PhpString::from_vec(out)))
                }
                None => Some(s),
            }
        }
        _ => match args {
            Some(a) => reflection_method_docall(vm, method, a),
            None => reflection_method_method(vm, method, obj)
                .or_else(|| reflection_method_docall(vm, method, &[Value::Object(obj.clone())])),
        },
    }
}

/// ReflectionFunction constructor
/// Z_PARAM_STR of a Reflection constructor's argument `n`: arrays and
/// non-Stringable objects are TypeErrors, null is deprecated (false when
/// it threw).
#[inline(never)]
fn ctor_string_arg_ok(
    vm: &mut Vm,
    class: &str,
    n: usize,
    name: &str,
    v: &Value,
    line: u32,
) -> bool {
    let v = v.deref();
    let bad = match &v {
        Value::Array(_) => true,
        Value::Object(o) => !vm
            .classes
            .get(&o.borrow().class_name.to_ascii_lowercase())
            .is_some_and(|c| c.methods.contains_key(&b"__tostring".to_vec())),
        Value::Null => {
            vm.emit_deprecated_at(
                &format!(
                    "{}::__construct(): Passing null to parameter #{} (${}) of type string is deprecated",
                    class, n, name
                ),
                line,
            );
            return vm.current_exception.is_none();
        }
        _ => false,
    };
    if bad {
        let msg = format!(
            "{}::__construct(): Argument #{} (${}) must be of type string, {} given",
            class,
            n,
            name,
            Vm::value_type_name(&v)
        );
        let exc = vm.create_exception(b"TypeError", &msg, line);
        vm.current_exception = Some(exc);
        return false;
    }
    true
}

fn reflection_function_construct_impl(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };
    if matches!(args.get(1).map(|v| v.deref()), Some(Value::Null)) {
        vm.emit_deprecated_at(
            "ReflectionFunction::__construct(): Passing null to parameter #1 ($function) of type Closure|string is deprecated",
            line,
        );
        if vm.current_exception.is_some() {
            return false;
        }
    }
    if let Some(arg) = args.get(1) {
        if arg.is_closure_obj() && args.len() == 2 {
            let closure = arg.deref();
            if let Some((decl, mname)) = internal_method_closure(vm, &closure) {
                return internal_method_rf_construct(vm, &this, closure, &decl, &mname, line);
            }
            let legacy = closure_legacy_arg(&closure);
            let ok = reflection_function_construct(vm, &[args[0].clone(), legacy], line);
            if ok {
                let display = vm.closure_op_array(&closure).and_then(|op| {
                    if crate::closure::is_closure_fn_name(&op.name)
                        && op.fcc_target_name.is_none()
                        && !op.display_name.is_empty()
                    {
                        return Some(op.display_name.clone());
                    }
                    // A function given a $this ("Closure::f"): named after it.
                    let t = op
                        .fcc_target_name
                        .as_ref()
                        .filter(|_| op.name.starts_with(b"__bound_closure_"))?;
                    let p = t.windows(2).position(|w| w == b"::")?;
                    Some(t[p + 2..].to_vec())
                });
                let magic = magic_call_trampoline(vm, &closure);
                let mut obj = this.borrow_mut();
                if let Some(d) = display {
                    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_vec(d)));
                } else if let Value::String(n) = obj.get_property(b"name") {
                    // A method's closure is named after the bare method.
                    let b = n.as_bytes();
                    if let Some(p) = b.windows(2).rposition(|w| w == b"::") {
                        let short = b[p + 2..].to_vec();
                        obj.set_property(
                            b"name".to_vec(),
                            Value::String(PhpString::from_vec(short)),
                        );
                    }
                }
                if let Some((target, name)) = magic {
                    // A closure of a __call()/__callStatic() trampoline: named
                    // after the called method, attributes of the magic one.
                    obj.set_property(b"name".to_vec(), Value::String(PhpString::from_vec(name)));
                    obj.set_property(
                        b"__reflection_magic".to_vec(),
                        Value::String(PhpString::from_vec(target)),
                    );
                }
                obj.set_property(b"__reflection_is_closure".to_vec(), Value::True);
                obj.set_property(b"__reflection_closure_obj".to_vec(), closure);
            }
            return ok;
        }
    }

    // Validate user arg count
    let user_arg_count = args.len().saturating_sub(1);
    if user_arg_count == 0 {
        let err =
            "ReflectionFunction::__construct() expects exactly 1 argument, 0 given".to_string();
        let exc = vm.create_exception(b"ArgumentCountError", &err, line);
        vm.current_exception = Some(exc);
        return false;
    }
    if user_arg_count > 1 {
        let err = format!(
            "ReflectionFunction::__construct() expects exactly 1 argument, {} given",
            user_arg_count
        );
        let exc = vm.create_exception(b"ArgumentCountError", &err, line);
        vm.current_exception = Some(exc);
        return false;
    }
    let arg = args.get(1).cloned().unwrap_or(Value::Null);

    // Handle Closure objects
    if let Value::Object(closure_obj) = &arg {
        let class_lower: Vec<u8> = closure_obj
            .borrow()
            .class_name
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        if class_lower == b"closure" {
            let mut obj = this.borrow_mut();
            obj.set_property(
                b"name".to_vec(),
                Value::String(PhpString::from_bytes(b"{closure}")),
            );
            obj.set_property(
                b"__reflection_target".to_vec(),
                Value::String(PhpString::from_bytes(b"")),
            );
            obj.set_property(b"__reflection_is_closure".to_vec(), Value::True);
            return true;
        }
        // Non-Closure object: throw TypeError
        let cls = crate::string::msg_str(&closure_obj.borrow().class_name).to_string();
        let err = format!(
            "ReflectionFunction::__construct(): Argument #1 ($function) must be of type Closure|string, {} given",
            cls
        );
        let exc = vm.create_exception(b"TypeError", &err, line);
        vm.current_exception = Some(exc);
        return false;
    }

    // Handle array callables like [ClassName, method] - these should throw for ReflectionFunction
    if let Value::Array(arr) = &arg {
        // Closures with captured `use ($var)` are internally represented
        // as an array whose first element is a `__closure_*` name. Treat
        // those as Closure objects.
        let first_is_closure_str = arr.borrow().values().next().and_then(|v| {
            if let Value::String(s) = v {
                let b = s.as_bytes();
                if b.starts_with(b"__closure_")
                    || b.starts_with(b"__arrow_")
                    || b.starts_with(b"__bound_closure_")
                    || b.starts_with(b"__closure_fcc_")
                {
                    Some(s.clone())
                } else {
                    None
                }
            } else {
                None
            }
        });
        if let Some(s) = first_is_closure_str {
            // Capture the trailing values (use-var bindings).
            let mut captured: Vec<Value> = arr.borrow().values().skip(1).cloned().collect();
            set_closure_scope_info(vm, &this, s.as_bytes(), &mut captured);
            let mut obj = this.borrow_mut();
            obj.set_property(
                b"name".to_vec(),
                Value::String(PhpString::from_bytes(b"{closure}")),
            );
            obj.set_property(b"__reflection_target".to_vec(), Value::String(s));
            obj.set_property(b"__reflection_is_closure".to_vec(), Value::True);
            if !captured.is_empty() {
                let mut arr_captured = PhpArray::new();
                for v in captured {
                    arr_captured.push(v);
                }
                obj.set_property(
                    b"__reflection_closure_captured".to_vec(),
                    Value::Array(Rc::new(RefCell::new(arr_captured))),
                );
            }
            return true;
        }
        let err_msg = "ReflectionFunction::__construct(): Argument #1 ($function) must be of type Closure|string, array given".to_string();
        let exc = vm.create_exception(b"TypeError", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let func_name = match &arg {
        Value::String(s) => {
            let name = s.to_msg_string();
            // Check for closures
            if name.starts_with("__closure_")
                || name.starts_with("__arrow_")
                || name.starts_with("__bound_closure_")
                || name.starts_with("__closure_fcc_")
            {
                set_closure_scope_info(vm, &this, s.as_bytes(), &mut Vec::new());
                let mut obj = this.borrow_mut();
                obj.set_property(
                    b"name".to_vec(),
                    Value::String(PhpString::from_bytes(b"{closure}")),
                );
                obj.set_property(b"__reflection_target".to_vec(), Value::String(s.clone()));
                obj.set_property(b"__reflection_is_closure".to_vec(), Value::True);
                return true;
            }
            name
        }
        _ => arg.to_php_string().to_msg_string(),
    };

    // Strip leading backslash
    let func_name = if let Some(stripped) = func_name.strip_prefix('\\') {
        stripped.to_string()
    } else {
        func_name
    };
    let func_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&func_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();

    // Check if function exists
    // (the engine's internal helpers, named "__...", are not functions)
    let exists = vm.user_functions.contains_key(&func_lower)
        || vm.functions.contains_key(&func_lower) && !func_lower.starts_with(b"__");
    if !exists {
        let err_msg = format!(
            "Function {}() does not exist",
            crate::string::msg_cstr(func_name.as_bytes())
        );
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }
    // `$name` is the function's own (declared / registered) name.
    let func_name = match vm.user_functions.get(&func_lower) {
        Some(op) if op.name.eq_ignore_ascii_case(&func_lower) => {
            crate::string::msg_str(&op.name).into_owned()
        }
        Some(_) => func_name,
        None => crate::stubs::find_function(&func_lower)
            .map(|f| f.name.to_string())
            .unwrap_or_else(|| crate::string::msg_str(&func_lower).into_owned()),
    };

    let mut obj = this.borrow_mut();
    obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_msg(&func_name.clone())),
    );
    obj.set_property(
        b"__reflection_target".to_vec(),
        Value::String(PhpString::from_vec(func_lower)),
    );
    true
}

/// ReflectionProperty constructor
fn reflection_property_construct_impl(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };

    // Validate user arg count (must be exactly 2)
    let user_arg_count = args.len().saturating_sub(1);
    if user_arg_count != 2 {
        let err = format!(
            "ReflectionProperty::__construct() expects exactly 2 arguments, {} given",
            user_arg_count
        );
        let exc = vm.create_exception(b"ArgumentCountError", &err, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let class_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let prop_arg = args.get(2).cloned().unwrap_or(Value::Null);
    if !ctor_string_arg_ok(vm, "ReflectionProperty", 2, "property", &prop_arg, line) {
        return false;
    }

    let class_name_raw = match &class_arg {
        Value::Object(obj) => {
            let ob = obj.borrow();
            crate::string::msg_str(&ob.class_name).to_string()
        }
        _ => class_arg.to_php_string().to_msg_string(),
    };
    // Strip a leading backslash from absolute class names (e.g. "\X").
    let class_name = class_name_raw.trim_start_matches('\\').to_string();
    let prop_name = prop_arg.to_php_string().to_msg_string();

    let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&class_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();

    let is_user_class = vm.classes.contains_key(&class_lower);
    let is_builtin = vm.is_known_builtin_class(&class_lower);

    // PHP triggers autoload when ReflectionProperty resolves an unknown class.
    let (is_user_class, is_builtin) = if !is_user_class && !is_builtin {
        vm.try_autoload_class(&crate::errdisplay::raw_error_bytes(&class_name));
        if vm.current_exception.is_some() {
            return false;
        }
        (
            vm.classes.contains_key(&class_lower),
            vm.is_known_builtin_class(&class_lower),
        )
    } else {
        (is_user_class, is_builtin)
    };

    if !is_user_class && !is_builtin {
        let err_msg = format!("Class \"{}\" does not exist", class_name);
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let canonical_class = if is_user_class {
        rcls(vm, &class_lower)
            .map(|c| crate::string::msg_str(&c.name).to_string())
            .unwrap_or(class_name.clone())
    } else {
        vm.builtin_canonical_name(&class_lower)
    };

    // For user classes, check property exists; for built-in classes, accept any property name.
    // When constructed from an object instance, accept dynamic properties even if the class
    // doesn't declare them (matches PHP behavior for #[AllowDynamicProperties] and stdClass).
    let mut is_dynamic_prop = false;
    // (internal classes known to the stubs declare their properties too)
    if is_user_class || rcls(vm, &class_lower).is_some() {
        // A parent's private property is not a property of the class.
        let prop_exists_decl = rcls(vm, &class_lower)
            .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()))
            .map(|p| {
                p.visibility != Visibility::Private
                    || p.declaring_class.eq_ignore_ascii_case(&class_lower)
            })
            .unwrap_or(false);

        if !prop_exists_decl {
            // Check if it exists as a dynamic property on the passed-in object.
            let dyn_exists = if let Value::Object(o) = &class_arg {
                let ob = o.borrow();
                ob.has_property(prop_name.as_bytes())
                    && !crate::foreach_obj::is_date_view_prop(&ob, prop_name.as_bytes())
                    && rprop(vm, &class_lower, prop_name.as_bytes()).is_none()
            } else {
                false
            };
            if dyn_exists {
                is_dynamic_prop = true;
            } else {
                let err_msg = format!(
                    "Property {}::${} does not exist",
                    canonical_class, prop_name
                );
                let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
                vm.current_exception = Some(exc);
                return false;
            }
        }
    }
    // `$class` is the declaring class (prop_info->ce), as getProperty() does.
    let canonical_class = if !is_dynamic_prop && rcls(vm, &class_lower).is_some() {
        rprop(vm, &class_lower, prop_name.as_bytes())
            .map(|p| p.declaring_class.to_ascii_lowercase())
            .and_then(|d| rcls(vm, &d).map(|c| crate::string::msg_str(&c.name).to_string()))
            .unwrap_or(canonical_class)
    } else {
        canonical_class
    };

    let mut obj = this.borrow_mut();
    obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_msg(&prop_name.clone())),
    );
    obj.set_property(
        b"class".to_vec(),
        Value::String(PhpString::from_msg(&canonical_class.clone())),
    );
    obj.set_property(
        b"__reflection_class".to_vec(),
        Value::String(PhpString::from_msg(&canonical_class)),
    );
    obj.set_property(
        b"__reflection_prop".to_vec(),
        Value::String(PhpString::from_msg(&prop_name)),
    );
    if is_dynamic_prop {
        obj.set_property(b"__reflection_prop_dynamic".to_vec(), Value::True);
        // Store the originating object so getValue/setValue and related can act on it.
        if let Value::Object(o) = &class_arg {
            obj.set_property(b"__reflection_prop_obj".to_vec(), Value::Object(o.clone()));
        }
    }
    true
}

/// ReflectionParameter constructor
fn reflection_parameter_construct_impl(vm: &mut Vm, args: &[Value], _line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };

    // PHP requires exactly 2 arguments to __construct (function + param)
    if args.len() < 3 {
        let provided = args.len().saturating_sub(1);
        let err_msg = format!(
            "ReflectionParameter::__construct() expects exactly 2 arguments, {} given",
            provided
        );
        let exc = vm.create_exception(b"ArgumentCountError", &err_msg, _line);
        vm.current_exception = Some(exc);
        return false;
    }

    let func_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let func_arg = if func_arg.is_closure_obj()
        || matches!(&func_arg, Value::Array(a) if a.borrow().get_int(0).map(|v| v.is_closure_obj()).unwrap_or(false))
    {
        closure_legacy_arg(&func_arg)
    } else {
        func_arg
    };
    let param_arg = args.get(2).cloned().unwrap_or(Value::Null);

    // Validate the function argument type: must be string, array, or callable object (closure/etc).
    match &func_arg {
        Value::String(_) | Value::Array(_) | Value::Object(_) => {}
        _ => {
            let tn = Vm::value_type_name(&func_arg);
            let err_msg = format!(
                "ReflectionParameter::__construct(): Argument #1 ($function) must be a string, an array(class, method), or a callable object, {} given",
                tn
            );
            let exc = vm.create_exception(b"ReflectionException", &err_msg, _line);
            vm.current_exception = Some(exc);
            return false;
        }
    }

    // If the function arg is an array, validate class/method existence and raise
    // appropriate ReflectionException messages.
    if let Value::Array(arr) = &func_arg {
        let items: Vec<Value> = arr.borrow().iter().map(|(_, v)| v.clone()).collect();
        if items.len() == 2 {
            let class_name = match &items[0] {
                Value::Object(o) => o.borrow().class_name.clone(),
                Value::String(s) => s.as_bytes().to_vec(),
                _ => Vec::new(),
            };
            // For objects with __toString, invoke it to obtain the method name.
            let method_name = vm.value_to_string(&items[1]).as_bytes().to_vec();
            // Detect closure (either as an object or as the internal string
            // representation used by goro-rs for closures stored in arrays).
            let is_closure_obj = class_name.starts_with(b"__closure_")
                || class_name.starts_with(b"__arrow_")
                || class_name.starts_with(b"__bound_closure_")
                || class_name.eq_ignore_ascii_case(b"Closure");
            if !class_name.is_empty() && !is_closure_obj {
                let class_lower: Vec<u8> =
                    class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                let method_lower: Vec<u8> =
                    method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                let class_str = crate::string::msg_str(&class_name).to_string();
                let method_str = crate::string::msg_str(&method_name).to_string();

                // If the class arg was a string (not an object) and class isn't known, raise.
                let class_known = vm.classes.contains_key(class_lower.as_slice())
                    || vm.is_known_builtin_class(class_lower.as_slice());
                let is_string_class = matches!(&items[0], Value::String(_));
                if is_string_class && !class_known {
                    let err_msg = format!("Class \"{}\" does not exist", class_str);
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, _line);
                    vm.current_exception = Some(exc);
                    return false;
                }

                if class_known {
                    let method_exists = rcls(vm, class_lower.as_slice())
                        .map(|c| c.methods.contains_key(method_lower.as_slice()))
                        .unwrap_or(false);
                    if !method_exists {
                        let err_msg =
                            format!("Method {}::{}() does not exist", class_str, method_str);
                        let exc = vm.create_exception(b"ReflectionException", &err_msg, _line);
                        vm.current_exception = Some(exc);
                        return false;
                    }
                }
            }
        }
    }

    // If the function arg is a plain string referring to an unknown function, raise.
    if let Value::String(s) = &func_arg {
        let raw = s.as_bytes();
        // Skip closures and method ('Class::method') forms; they go through other paths.
        let has_sep = raw.windows(2).any(|w| w == b"::");
        if !has_sep {
            let lower: Vec<u8> = raw.iter().map(|b| b.to_ascii_lowercase()).collect();
            let is_closure = lower.starts_with(b"__closure_")
                || lower.starts_with(b"__arrow_")
                || lower.starts_with(b"__bound_closure_")
                || lower.starts_with(b"__closure_fcc_");
            if !is_closure
                && !vm.user_functions.contains_key(&lower)
                && !vm.functions.contains_key(&lower)
            {
                let err_msg = format!("Function {}() does not exist", crate::string::msg_str(raw));
                let exc = vm.create_exception(b"ReflectionException", &err_msg, _line);
                vm.current_exception = Some(exc);
                return false;
            }
        }
    }

    // Resolve the function/method reference to a lowercase key in
    // vm.user_functions. Supports strings, closures (string form), arrays
    // ([class, method] or [closure_str]), and Closure objects.
    let func_lower: Vec<u8> = match &func_arg {
        Value::String(s) => s
            .as_bytes()
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect(),
        Value::Array(arr) => {
            let items: Vec<Value> = arr.borrow().iter().map(|(_, v)| v.clone()).collect();
            if items.len() == 2 {
                let class_part = match &items[0] {
                    Value::Object(o) => crate::string::msg_str(&o.borrow().class_name).to_string(),
                    Value::String(s) => s.to_msg_string(),
                    _ => String::new(),
                };
                let method_part = vm.value_to_string(&items[1]).to_msg_string();
                // If class is closure, use the closure string directly.
                let cl_low = class_part.to_ascii_lowercase();
                if cl_low == "closure"
                    || items[0]
                        .clone()
                        .to_php_string()
                        .to_msg_string()
                        .starts_with("__closure_")
                    || items[0]
                        .clone()
                        .to_php_string()
                        .to_msg_string()
                        .starts_with("__arrow_")
                    || items[0]
                        .clone()
                        .to_php_string()
                        .to_msg_string()
                        .starts_with("__bound_closure_")
                {
                    if let Value::String(s) = &items[0] {
                        let n = s.to_msg_string();
                        if n.starts_with("__closure_")
                            || n.starts_with("__arrow_")
                            || n.starts_with("__bound_closure_")
                            || n.starts_with("__closure_fcc_")
                        {
                            n.as_bytes()
                                .iter()
                                .map(|b| b.to_ascii_lowercase())
                                .collect()
                        } else {
                            crate::errdisplay::raw_error_bytes(&format!(
                                "{}::{}",
                                class_part, method_part
                            ))
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect()
                        }
                    } else {
                        crate::errdisplay::raw_error_bytes(&format!(
                            "{}::{}",
                            class_part, method_part
                        ))
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect()
                    }
                } else {
                    crate::errdisplay::raw_error_bytes(&format!("{}::{}", class_part, method_part))
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect()
                }
            } else if items.len() == 1 {
                items[0]
                    .to_php_string()
                    .as_bytes()
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect()
            } else {
                Vec::new()
            }
        }
        _ => func_arg
            .to_php_string()
            .as_bytes()
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect(),
    };

    // For methods, resolve to the class.method MethodDef so we can use cv_names
    // and skip the synthetic $this slot at index 0.
    let (method_class_lower, method_name_lower): (Option<Vec<u8>>, Option<Vec<u8>>) =
        if let Some(sep) = func_lower.windows(2).position(|w| w == b"::") {
            (
                Some(func_lower[..sep].to_vec()),
                Some(func_lower[sep + 2..].to_vec()),
            )
        } else {
            (None, None)
        };

    let method_def = match (&method_class_lower, &method_name_lower) {
        (Some(cl), Some(mn)) => rcls(vm, cl).and_then(|c| c.methods.get(mn).cloned()),
        _ => None,
    };
    let method_cv_offset: usize = method_def
        .as_ref()
        .map(|m| if m.is_static { 0 } else { 1 })
        .unwrap_or(0);

    // Parameter may be specified by index or by name.
    // For named lookup, search cv_names skipping the implicit `$this` for methods.
    let param_idx_opt: Option<usize> = match &param_arg {
        Value::String(s) => {
            let name = s.as_bytes();
            if let Some(m) = method_def.as_ref() {
                // Look up among visible parameter names (cv_names[cv_offset..param_count])
                let pc = m.op_array.param_count as usize;
                let names = &m.op_array.cv_names;
                let mut found: Option<usize> = None;
                for i in 0..(pc.saturating_sub(method_cv_offset)) {
                    let cv_idx = i + method_cv_offset;
                    if cv_idx < names.len() && names[cv_idx] == name {
                        found = Some(i);
                        break;
                    }
                }
                found
            } else {
                vm.user_functions
                    .get(&func_lower)
                    .and_then(|op| op.cv_names.iter().position(|n| n == name))
                    .or_else(|| {
                        // Internal functions: the stub's parameter names.
                        rfunc(vm, &func_lower).and_then(|op| {
                            op.cv_names
                                .iter()
                                .take(op.param_count as usize)
                                .position(|n| n == name)
                        })
                    })
            }
        }
        Value::Long(i) => {
            if *i < 0 {
                let err_msg = "ReflectionParameter::__construct(): Argument #2 ($param) must be greater than or equal to 0";
                let exc = vm.create_exception(b"ValueError", err_msg, _line);
                vm.current_exception = Some(exc);
                return false;
            }
            Some(*i as usize)
        }
        _ => {
            let val = param_arg.to_long();
            if val < 0 {
                let err_msg = "ReflectionParameter::__construct(): Argument #2 ($param) must be greater than or equal to 0";
                let exc = vm.create_exception(b"ValueError", err_msg, _line);
                vm.current_exception = Some(exc);
                return false;
            }
            Some(val as usize)
        }
    };

    // If a string-name lookup was requested but not found, raise.
    if matches!(&param_arg, Value::String(_)) && param_idx_opt.is_none() {
        let err_msg = "The parameter specified by its name could not be found";
        let exc = vm.create_exception(b"ReflectionException", err_msg, _line);
        vm.current_exception = Some(exc);
        return false;
    }

    // Look up the function or method
    if let Some(m) = method_def.as_ref() {
        let param_idx = param_idx_opt.unwrap_or(0);
        // Validate index range (for integer index).
        let pc = m.op_array.param_count as usize;
        let visible_count = pc.saturating_sub(method_cv_offset);
        if param_idx >= visible_count {
            let err_msg = "The parameter specified by its offset could not be found".to_string();
            let exc = vm.create_exception(b"ReflectionException", &err_msg, _line);
            vm.current_exception = Some(exc);
            return false;
        }
        let cv_idx = param_idx + method_cv_offset;
        let param_name = if cv_idx < m.op_array.cv_names.len() {
            crate::string::msg_str(&m.op_array.cv_names[cv_idx]).to_string()
        } else {
            format!("param{}", param_idx)
        };
        let mut obj = this.borrow_mut();
        obj.set_property(
            b"name".to_vec(),
            Value::String(PhpString::from_msg(&param_name)),
        );
        obj.set_property(
            b"__reflection_func".to_vec(),
            Value::String(PhpString::from_vec(func_lower)),
        );
        obj.set_property(
            b"__reflection_param_idx".to_vec(),
            Value::Long(param_idx as i64),
        );
        obj.set_property(
            b"__reflection_param_cv_offset".to_vec(),
            Value::Long(method_cv_offset as i64),
        );
        if let (Some(cl), Some(mn)) = (method_class_lower.as_ref(), method_name_lower.as_ref()) {
            obj.set_property(
                b"__reflection_param_class".to_vec(),
                Value::String(PhpString::from_vec(cl.clone())),
            );
            obj.set_property(
                b"__reflection_param_method".to_vec(),
                Value::String(PhpString::from_vec(mn.clone())),
            );
        }
    } else if let Some(op_array) = vm.user_functions.get(&func_lower).cloned() {
        let param_idx = param_idx_opt.unwrap_or(0);
        if param_idx >= op_array.param_count as usize {
            let err_msg = "The parameter specified by its offset could not be found".to_string();
            let exc = vm.create_exception(b"ReflectionException", &err_msg, _line);
            vm.current_exception = Some(exc);
            return false;
        }
        let param_name = if param_idx < op_array.cv_names.len() {
            crate::string::msg_str(&op_array.cv_names[param_idx]).to_string()
        } else {
            format!("param{}", param_idx)
        };

        let mut obj = this.borrow_mut();
        obj.set_property(
            b"name".to_vec(),
            Value::String(PhpString::from_msg(&param_name)),
        );
        obj.set_property(
            b"__reflection_func".to_vec(),
            Value::String(PhpString::from_vec(func_lower)),
        );
        obj.set_property(
            b"__reflection_param_idx".to_vec(),
            Value::Long(param_idx as i64),
        );
    } else if let Some(idx) = param_idx_opt {
        // No userland function - still set enough for later calls.
        let internal = rfunc(vm, &func_lower);
        // An internal function's parameter offset must exist too.
        if let Some(op) = &internal {
            if idx >= op.param_count as usize && op.variadic_param.is_none() {
                let err_msg = "The parameter specified by its offset could not be found";
                let exc = vm.create_exception(b"ReflectionException", err_msg, _line);
                vm.current_exception = Some(exc);
                return false;
            }
        }
        let internal_name = internal.and_then(|op| op.cv_names.get(idx).cloned());
        let mut obj = this.borrow_mut();
        if let Some(n) = internal_name {
            obj.set_property(b"name".to_vec(), Value::String(PhpString::from_vec(n)));
        }
        // Forget what an earlier __construct() call reflected.
        obj.properties.retain(|(k, _)| {
            !matches!(
                k.as_slice(),
                b"__reflection_param_class"
                    | b"__reflection_param_method"
                    | b"__reflection_param_cv_offset"
            )
        });
        obj.set_property(
            b"__reflection_func".to_vec(),
            Value::String(PhpString::from_vec(func_lower)),
        );
        obj.set_property(b"__reflection_param_idx".to_vec(), Value::Long(idx as i64));
    }
    true
}

/// ReflectionClassConstant constructor
fn reflection_class_constant_construct_impl(vm: &mut Vm, args: &[Value], line: u32) -> bool {
    let this = match args.first() {
        Some(Value::Object(o)) => o.clone(),
        _ => return true,
    };

    // Determine which reflector class we're constructing - for error messages.
    let reflector_name: String = {
        let ob = this.borrow();
        crate::string::msg_str(&ob.class_name).to_string()
    };

    let class_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let const_arg = args.get(2).cloned().unwrap_or(Value::Null);

    // Reject array first argument explicitly.
    if matches!(&class_arg, Value::Array(_)) {
        let err_msg = format!(
            "{}::__construct(): Argument #1 ($class) must be of type object|string, array given",
            reflector_name
        );
        let exc = vm.create_exception(b"TypeError", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }
    if !ctor_string_arg_ok(vm, &reflector_name, 2, "constant", &const_arg, line) {
        return false;
    }

    let class_name_raw = match &class_arg {
        Value::Object(obj) => {
            let ob = obj.borrow();
            crate::string::msg_str(&ob.class_name).to_string()
        }
        _ => class_arg.to_php_string().to_msg_string(),
    };
    // Strip a leading backslash from absolute class names (e.g. "\X").
    let class_name = class_name_raw.trim_start_matches('\\').to_string();
    let const_name = const_arg.to_php_string().to_msg_string();

    let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&class_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();

    // Check class exists (zend_lookup_class(): autoloaded)
    if !vm.classes.contains_key(&class_lower) && !vm.is_known_builtin_class(&class_lower) {
        vm.try_autoload_class(&crate::errdisplay::raw_error_bytes(&class_name));
        if vm.current_exception.is_some() {
            return false;
        }
    }
    if !vm.classes.contains_key(&class_lower) && !vm.is_known_builtin_class(&class_lower) {
        let err_msg = format!("Class \"{}\" does not exist", class_name);
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let canonical_class = rcls(vm, &class_lower)
        .map(|c| crate::string::msg_str(&c.name).to_string())
        .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower));

    // Check constant exists. PHP says "Constant {}::{} does not exist" in
    // both the ReflectionClassConstant and ReflectionEnumUnit/BackedCase paths.
    let const_val = reflection_class_get_constant(vm, &class_lower, const_name.as_bytes());
    if const_val.is_none() {
        let err_msg = format!(
            "Constant {}::{} does not exist",
            canonical_class, const_name
        );
        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
        vm.current_exception = Some(exc);
        return false;
    }

    let unwrapped = const_val.unwrap();

    // ReflectionEnumUnitCase / ReflectionEnumBackedCase: validate the
    // referenced constant is actually declared as an enum case (regular
    // constants whose value happens to be an enum case are not cases).
    if reflector_name.eq_ignore_ascii_case("ReflectionEnumUnitCase")
        || reflector_name.eq_ignore_ascii_case("ReflectionEnumBackedCase")
    {
        let is_declared_case = rcls(vm, &class_lower)
            .map(|c| {
                c.enum_cases
                    .iter()
                    .any(|(n, _)| n.eq_ignore_ascii_case(const_name.as_bytes()))
            })
            .unwrap_or(false);
        if !is_declared_case {
            let err_msg = format!("Constant {}::{} is not a case", canonical_class, const_name);
            let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
            vm.current_exception = Some(exc);
            return false;
        }
        if reflector_name.eq_ignore_ascii_case("ReflectionEnumBackedCase") {
            let is_backed = if let Value::Object(o) = &unwrapped {
                o.borrow().has_property(b"value")
            } else {
                false
            };
            if !is_backed {
                let err_msg = format!(
                    "Enum case {}::{} is not a backed case",
                    canonical_class, const_name
                );
                let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
                vm.current_exception = Some(exc);
                return false;
            }
        }
    }

    // The visible `class` property reflects the constant's declaring class,
    // not the class the reflection was constructed on. Walk up the parent
    // chain to find it (regular constants only — enum cases stay on the
    // enum they were declared on, which is also the class we were given).
    let declaring_class = if reflector_name.eq_ignore_ascii_case("ReflectionEnumUnitCase")
        || reflector_name.eq_ignore_ascii_case("ReflectionEnumBackedCase")
    {
        canonical_class.clone()
    } else {
        find_constant_declaring_class(vm, &class_lower, const_name.as_bytes())
            .unwrap_or_else(|| canonical_class.clone())
    };

    let mut obj = this.borrow_mut();
    obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_msg(&const_name.clone())),
    );
    obj.set_property(
        b"class".to_vec(),
        Value::String(PhpString::from_msg(&declaring_class)),
    );
    obj.set_property(b"__reflection_value".to_vec(), unwrapped);
    true
}

/// ReflectionClass no-arg method dispatch
pub fn reflection_class_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let target = ob
        .get_property(b"__reflection_target")
        .to_php_string()
        .to_msg_string();
    let class_lower: Vec<u8> = ob
        .get_property(b"__reflection_target")
        .to_php_string()
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    drop(ob);

    match method {
        b"getname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        b"getparentclass" => {
            if let Some(ce) = rcls(vm, &class_lower) {
                if let Some(ref parent) = ce.parent {
                    // Create a ReflectionClass for the parent
                    let parent_name = crate::string::msg_str(parent).to_string();
                    Some(create_reflection_class(vm, &parent_name))
                } else {
                    Some(Value::False)
                }
            } else {
                // Check built-in parent chain
                let parents = super::vm::builtin_parent_chain(&class_lower);
                if let Some(first_parent) = parents.first() {
                    let parent_name = vm.builtin_canonical_name(first_parent);
                    Some(create_reflection_class(vm, &parent_name))
                } else {
                    Some(Value::False)
                }
            }
        }
        b"isabstract" => {
            // ZEND_ACC_EXPLICIT_ABSTRACT_CLASS, or IMPLICIT_ABSTRACT_CLASS:
            // an interface declaring or inheriting a method.
            let is_abstract = rcls(vm, &class_lower)
                .map(|c| {
                    c.is_abstract || (c.is_interface && !rmethods(vm, &class_lower).is_empty())
                })
                .unwrap_or(false);
            Some(if is_abstract {
                Value::True
            } else {
                Value::False
            })
        }
        b"isfinal" => {
            // (user enums are final; internal ones are not flagged)
            let user_enum = !is_internal_class(vm, &class_lower);
            let is_final = rcls(vm, &class_lower)
                .map(|c| c.is_final || (c.is_enum && user_enum))
                .unwrap_or(false);
            Some(if is_final { Value::True } else { Value::False })
        }
        b"isinterface" => {
            let is_interface = rcls(vm, &class_lower)
                .map(|c| c.is_interface)
                .unwrap_or(false);
            Some(if is_interface {
                Value::True
            } else {
                Value::False
            })
        }
        b"istrait" => {
            let is_trait = rcls(vm, &class_lower).map(|c| c.is_trait).unwrap_or(false);
            Some(if is_trait { Value::True } else { Value::False })
        }
        b"isenum" => {
            let is_enum = rcls(vm, &class_lower).map(|c| c.is_enum).unwrap_or(false);
            Some(if is_enum { Value::True } else { Value::False })
        }
        b"isreadonly" => {
            let is_readonly = rcls(vm, &class_lower)
                .map(|c| c.is_readonly)
                .unwrap_or(false);
            Some(if is_readonly {
                Value::True
            } else {
                Value::False
            })
        }
        b"isinstantiable" => {
            if let Some(ce) = rcls(vm, &class_lower) {
                let mut instantiable =
                    !ce.is_abstract && !ce.is_interface && !ce.is_trait && !ce.is_enum;
                // A class with a non-public constructor isn't instantiable
                // from outside, so isInstantiable() reports false.
                if instantiable {
                    if let Some(ctor) = ce.get_method(b"__construct") {
                        if ctor.visibility != Visibility::Public {
                            instantiable = false;
                        }
                    }
                }
                Some(if instantiable {
                    Value::True
                } else {
                    Value::False
                })
            } else {
                // Built-in classes are generally instantiable
                Some(Value::True)
            }
        }
        b"iscloneable" => {
            // Traits, interfaces, abstract classes, and enums are not cloneable.
            let mut cloneable = if let Some(ce) = rcls(vm, &class_lower) {
                !ce.is_interface && !ce.is_trait && !ce.is_abstract && !ce.is_enum
            } else {
                !vm.builtin_is_interface(&class_lower)
            };
            // Objects whose handlers have no clone_obj.
            const UNCLONEABLE: &[&[u8]] = &[
                b"reflectionclass",
                b"reflectionfunctionabstract",
                b"reflectionproperty",
                b"reflectionparameter",
                b"reflectionclassconstant",
                b"reflectiontype",
                b"reflectionattribute",
                b"reflectionextension",
                b"reflectionzendextension",
                b"reflectionconstant",
                b"reflectiongenerator",
                b"reflectionfiber",
                b"reflectionreference",
                b"generator",
                b"fiber",
                b"weakreference",
                b"xmlwriter",
                b"xmlparser",
                b"splfileobject",
                b"globiterator",
                b"iteratoriterator",
                b"recursiveiteratoriterator",
                b"random\\engine\\secure",
                b"random\\randomizer",
                b"directory",
                b"finfo",
                b"mysqli",
                b"mysqli_result",
                b"mysqli_stmt",
                b"addressinfo",
                b"curlmultihandle",
                b"curlsharehandle",
                b"curlsharepersistenthandle",
                b"deflatecontext",
                b"dom\\dtdnamednodemap",
                b"dom\\htmlcollection",
                b"dom\\implementation",
                b"dom\\namednodemap",
                b"dom\\nodelist",
                b"dom\\tokenlist",
                b"dom\\xpath",
                b"domnamednodemap",
                b"domnodelist",
                b"domxpath",
                b"filter\\filterexception",
                b"filter\\filterfailedexception",
                b"ftp\\connection",
                b"inflatecontext",
                b"internaliterator",
                b"opensslasymmetrickey",
                b"opensslcertificate",
                b"opensslcertificatesigningrequest",
                b"pdo",
                b"pdorow",
                b"pdostatement",
                b"pharexception",
                b"shmop",
                b"soap\\sdl",
                b"soap\\url",
                b"soapfault",
                b"soapserver",
                b"socket",
                b"sodiumexception",
                b"sysvmessagequeue",
                b"sysvsemaphore",
                b"sysvsharedmemory",
                b"xmlreader",
                b"xsltprocessor",
            ];
            if cloneable
                && UNCLONEABLE
                    .iter()
                    .any(|b| class_lower == *b || vm.class_extends(&class_lower, b))
            {
                cloneable = false;
            }
            // A non-public __clone().
            if cloneable {
                if let Some(m) = rcls(vm, &class_lower).and_then(|c| c.get_method(b"__clone")) {
                    cloneable = m.visibility == Visibility::Public;
                }
            }
            Some(if cloneable { Value::True } else { Value::False })
        }
        b"isinternal" => {
            // User-defined classes are not internal
            let is_internal = is_internal_class(vm, &class_lower);
            Some(if is_internal {
                Value::True
            } else {
                Value::False
            })
        }
        b"isuserdefined" => {
            let is_user =
                vm.classes.contains_key(&class_lower) && !is_internal_class(vm, &class_lower);
            Some(if is_user { Value::True } else { Value::False })
        }
        b"isanonymous" => {
            // Anonymous class names contain a NUL byte separator
            let is_anon = if let Some(ce) = rcls(vm, &class_lower) {
                ce.name.contains(&b'\x00')
            } else {
                false
            };
            Some(if is_anon { Value::True } else { Value::False })
        }
        b"isiterable" | b"isiterateable" => {
            // Per PHP: interfaces themselves and abstract classes are not iterable
            // even if they extend Iterator. Only concrete (or non-interface,
            // non-abstract) classes that implement Iterator/IteratorAggregate are.
            let (is_interface, is_abstract) = rcls(vm, &class_lower)
                .map(|c| (c.is_interface, c.is_abstract))
                .unwrap_or((vm.builtin_is_interface(&class_lower), false));
            if is_interface || is_abstract {
                return Some(Value::False);
            }
            let is_iterable = vm.class_implements_interface(&class_lower, b"iterator")
                || vm.class_implements_interface(&class_lower, b"iteratoraggregate")
                || vm.builtin_implements_interface(&class_lower, b"iterator")
                || vm.builtin_implements_interface(&class_lower, b"iteratoraggregate");
            Some(if is_iterable {
                Value::True
            } else {
                Value::False
            })
        }
        b"getinterfacenames" => {
            // Collect all interfaces reachable via implements/extends chains.
            let collected: Vec<Vec<u8>> = class_all_interface_names(vm, &class_lower)
                .into_iter()
                .map(|n| n.into_bytes())
                .collect();
            let mut names = PhpArray::new();
            for iface in collected {
                names.push(Value::String(PhpString::from_vec(iface)));
            }
            // Add implicit Stringable interface for classes with __toString
            // (traits implement no interfaces)
            let is_trait = vm.classes.get(&class_lower).is_some_and(|c| c.is_trait);
            if !is_trait && vm.class_implements_interface(&class_lower, b"stringable") {
                let already_has = names.iter().any(|(_, v)| {
                    v.to_php_string()
                        .as_bytes()
                        .eq_ignore_ascii_case(b"Stringable")
                });
                if !already_has {
                    names.push(Value::String(PhpString::from_vec(b"Stringable".to_vec())));
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(names))))
        }
        b"getinterfaces" => {
            let mut collected: Vec<Vec<u8>> = class_all_interface_names(vm, &class_lower)
                .into_iter()
                .map(|n| n.into_bytes())
                .collect();
            if !vm.classes.get(&class_lower).is_some_and(|c| c.is_trait)
                && vm.class_implements_interface(&class_lower, b"stringable")
                && !collected
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(b"Stringable"))
            {
                collected.push(b"Stringable".to_vec());
            }
            let mut result = PhpArray::new();
            for iface in collected {
                let iface_name = crate::string::msg_str(&iface).to_string();
                let rc = create_reflection_class(vm, &iface_name);
                result.set(ArrayKey::String(PhpString::from_vec(iface.clone())), rc);
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"gettraitnames" => {
            let mut names = PhpArray::new();
            if let Some(ce) = rcls(vm, &class_lower) {
                for t in &ce.traits {
                    names.push(Value::String(PhpString::from_vec(t.clone())));
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(names))))
        }
        b"gettraits" => {
            let mut result = PhpArray::new();
            if let Some(ce) = rcls(vm, &class_lower) {
                for t in ce.traits.clone() {
                    let trait_name = crate::string::msg_str(&t).to_string();
                    let rc = create_reflection_class(vm, &trait_name);
                    result.set(ArrayKey::String(PhpString::from_vec(t.clone())), rc);
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"gettraitaliases" => {
            let mut result = PhpArray::new();
            if let Some(ce) = rcls(vm, &class_lower) {
                for adapt in &ce.trait_adaptations {
                    if let crate::object::TraitAdaptation::Alias {
                        trait_name,
                        method,
                        new_name,
                        ..
                    } = adapt
                    {
                        if let Some(alias) = new_name {
                            // An alias without a trait name names the used
                            // trait that declares the method.
                            let ml = method.to_ascii_lowercase();
                            let trait_str = trait_name
                                .clone()
                                .or_else(|| {
                                    ce.traits.iter().find_map(|t| {
                                        rcls(vm, &t.to_ascii_lowercase())
                                            .filter(|tc| tc.methods.contains_key(&ml))
                                            .map(|tc| tc.name.clone())
                                    })
                                })
                                .map(|t| crate::string::msg_str(&t).to_string())
                                .unwrap_or_default();
                            let method_str = crate::string::msg_str(method).to_string();
                            let alias_str = crate::string::msg_str(alias).to_string();
                            let source = if trait_str.is_empty() {
                                method_str
                            } else {
                                format!("{}::{}", trait_str, method_str)
                            };
                            result.set(
                                ArrayKey::String(PhpString::from_msg(&alias_str)),
                                Value::String(PhpString::from_msg(&source)),
                            );
                        }
                    }
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getconstructor" => {
            if rcls(vm, &class_lower).is_some() {
                if let Some(decl) =
                    rmethod(vm, &class_lower, b"__construct").map(|m| m.declaring_class.clone())
                {
                    let decl_name = rcls(vm, &decl)
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or_else(|| target.clone());
                    Some(create_reflection_method(vm, &decl_name, "__construct"))
                } else {
                    Some(Value::Null)
                }
            } else {
                Some(Value::Null)
            }
        }
        b"getmodifiers" => {
            let mut mods = 0i64;
            if let Some(ce) = rcls(vm, &class_lower) {
                if ce.is_abstract {
                    mods |= 0x40;
                } // IS_EXPLICIT_ABSTRACT
                if ce.is_final || (ce.is_enum && !is_internal_class(vm, &class_lower)) {
                    mods |= 0x20;
                } // IS_FINAL
                if ce.is_readonly {
                    mods |= 0x10000;
                } // IS_READONLY
            }
            Some(Value::Long(mods))
        }
        b"getdefaultproperties" => {
            if vm.update_class_constants(&class_lower).is_err() {
                return Some(Value::Null);
            }
            // add_class_vars(ce, statics) then add_class_vars(ce, instance),
            // over properties_info (an ancestor's private ones skipped).
            let mut result = PhpArray::new();
            let props: Vec<crate::object::PropertyDef> =
                rprops(vm, &class_lower).into_iter().cloned().collect();
            for statics in [true, false] {
                for prop in &props {
                    if prop.is_static != statics {
                        continue;
                    }
                    if prop.visibility == Visibility::Private
                        && !prop.declaring_class.eq_ignore_ascii_case(&class_lower)
                    {
                        continue;
                    }
                    // (default_static_members_table: the declared default,
                    // not the current value)
                    let val = if statics && is_deferred_const_value(&prop.default) {
                        vm.static_prop_raw(&prop.declaring_class.to_ascii_lowercase(), &prop.name)
                            .unwrap_or_else(|| prop.default.clone())
                    } else {
                        prop.default.clone()
                    };
                    // Uninitialized typed properties have no default.
                    if matches!(val, Value::Undef) || prop.is_virtual {
                        continue;
                    }
                    let key = ArrayKey::String(PhpString::from_vec(prop.name.clone()));
                    if result.get(&key).is_none() {
                        result.set(key, val);
                    }
                }
            }
            // Resolve deferred constant-expression markers in defaults.
            let keys: Vec<ArrayKey> = result.iter()
                .filter(|(_, v)| matches!(v, Value::String(s) if s.as_bytes().starts_with(b"__deferred_global_const__::")))
                .map(|(k, _)| k.clone())
                .collect();
            for k in keys {
                if let Some(v) = result.get(&k).cloned() {
                    let r = vm.resolve_deferred_value(&v);
                    result.set(k, r);
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getstaticproperties" => {
            if vm.update_class_constants(&class_lower).is_err() {
                return Some(Value::Null);
            }
            // Include own static props (including own private) and inherited
            // non-private statics. Properties stored on this class with
            // declaring_class != self are inherited; private inherited ones
            // must be excluded.
            let mut result = PhpArray::new();
            // properties_info order: own declarations, then the parent's.
            let mut chain: Vec<Vec<u8>> = vec![class_lower.clone()];
            while let Some(p) = rcls(vm, chain.last().unwrap()).and_then(|c| c.parent.clone()) {
                if chain.len() > 64 {
                    break;
                }
                chain.push(p.to_ascii_lowercase());
            }
            let mut entries: Vec<(Vec<u8>, Value)> = Vec::new();
            for (idx, link) in chain.iter().enumerate() {
                if let Some(lce) = rcls(vm, link) {
                    for prop in own_declared_props(lce, link) {
                        if !prop.is_static {
                            continue;
                        }
                        if idx > 0 && prop.visibility == Visibility::Private {
                            continue;
                        }
                        entries.push((prop.name.clone(), prop.default.clone()));
                    }
                }
            }
            for (name, default) in entries {
                let key = ArrayKey::String(PhpString::from_vec(name.clone()));
                if result.get(&key).is_none() {
                    // Static value is stored on the declaring class
                    let val = vm.static_prop_raw(&class_lower, &name).unwrap_or(default);
                    // (an uninitialized typed static is left out)
                    if matches!(val.deref(), Value::Undef) {
                        continue;
                    }
                    result.set(key, val);
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getfilename" => {
            if is_internal_class(vm, &class_lower) {
                Some(Value::False)
            } else if let Some(ce) = vm.classes.get(&class_lower) {
                if let Some(ref filename) = ce.filename {
                    Some(Value::String(PhpString::from_msg(&filename.clone())))
                } else {
                    Some(Value::String(PhpString::from_msg(
                        &vm.current_file.to_string(),
                    )))
                }
            } else {
                Some(Value::False)
            }
        }
        b"getstartline" => {
            if let Some(ce) = rcls(vm, &class_lower) {
                if ce.start_line > 0 {
                    Some(Value::Long(ce.start_line as i64))
                } else {
                    Some(Value::False)
                }
            } else {
                Some(Value::False)
            }
        }
        b"getendline" => {
            if let Some(ce) = rcls(vm, &class_lower) {
                if ce.end_line > 0 {
                    Some(Value::Long(ce.end_line as i64))
                } else {
                    Some(Value::False)
                }
            } else {
                Some(Value::False)
            }
        }
        b"getdoccomment" => {
            if let Some(ce) = rcls(vm, &class_lower) {
                if let Some(ref doc) = ce.doc_comment {
                    Some(Value::String(PhpString::from_bytes(doc.as_bytes())))
                } else {
                    Some(Value::False)
                }
            } else {
                Some(Value::False)
            }
        }
        b"newinstancewithoutconstructor" => {
            // Internal final classes cannot be created without their ctor.
            // (zend_class_entry: ZEND_INTERNAL_CLASS, ZEND_ACC_FINAL and a
            // create_object handler)
            let no_ctorless: &[&[u8]] = &[
                b"__php_incomplete_class",
                b"addressinfo",
                b"bcmath\\number",
                b"closure",
                b"curlhandle",
                b"curlmultihandle",
                b"curlsharehandle",
                b"curlsharepersistenthandle",
                b"deflatecontext",
                b"dom\\domexception",
                b"dom\\htmldocument",
                b"dom\\tokenlist",
                b"dom\\xmldocument",
                b"dom\\xpath",
                b"domexception",
                b"ffi",
                b"ffi\\cdata",
                b"ffi\\ctype",
                b"ffi\\parserexception",
                b"fiber",
                b"fibererror",
                b"ftp\\connection",
                b"generator",
                b"hashcontext",
                b"inflatecontext",
                b"internaliterator",
                b"opensslasymmetrickey",
                b"opensslcertificate",
                b"opensslcertificatesigningrequest",
                b"pdorow",
                b"random\\engine\\mt19937",
                b"random\\engine\\pcgoneseq128xslrr64",
                b"random\\engine\\secure",
                b"random\\engine\\xoshiro256starstar",
                b"random\\randomizer",
                b"reflectionfiber",
                b"reflectiongenerator",
                b"reflectionreference",
                b"shmop",
                b"soap\\sdl",
                b"soap\\url",
                b"socket",
                b"sysvmessagequeue",
                b"sysvsemaphore",
                b"sysvsharedmemory",
                b"uri\\rfc3986\\uri",
                b"uri\\whatwg\\url",
                b"weakmap",
                b"weakreference",
                b"xmlparser",
                b"gmp",
                b"dba\\connection",
                b"ldap\\connection",
                b"ldap\\result",
                b"ldap\\resultentry",
                b"pgsql\\connection",
                b"pgsql\\result",
                b"pgsql\\lob",
            ];
            if no_ctorless.contains(&class_lower.as_slice()) {
                let canonical = rcls(vm, &class_lower)
                    .map(|c| crate::string::msg_str(&c.name).to_string())
                    .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower));
                let err_msg = format!(
                    "Class {} is an internal class marked as final that cannot be instantiated without invoking its constructor",
                    canonical
                );
                let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            if let Some(err_msg) = instantiation_error(vm, &class_lower) {
                let exc = vm.create_exception(b"Error", &err_msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            // Create an instance without calling the constructor
            let obj_id = vm.alloc_object_id();
            let canonical = rcls(vm, &class_lower)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());
            let mut new_obj = PhpObject::new(canonical, obj_id);
            // Set default property values (the slots `new` gives: a
            // redeclared private keeps its own)
            if vm.classes.contains_key(&class_lower) {
                let _ = vm.update_class_constants(&class_lower);
                if vm.current_exception.is_some() {
                    return Some(Value::Null);
                }
                vm.init_declared_props(&class_lower, &mut new_obj);
                new_obj.mark_declared();
            } else if let Some(ce) = rcls(vm, &class_lower) {
                let props: Vec<_> = ce
                    .properties
                    .iter()
                    .filter(|p| !p.is_static && !p.is_virtual)
                    .map(|p| (p.name.clone(), p.default.clone()))
                    .collect();
                for (name, default) in props {
                    let resolved = vm.resolve_deferred_value(&default);
                    new_obj.set_property(name, resolved);
                }
            }
            // zend_default_exception_new(): file, line and trace are the
            // creation site's.
            if class_lower == b"exception"
                || class_lower == b"error"
                || crate::vm::is_builtin_subclass(&class_lower, b"exception")
                || crate::vm::is_builtin_subclass(&class_lower, b"error")
                || vm.class_extends_any(&class_lower, &[b"exception", b"error"])
            {
                if !new_obj.has_property(b"message") {
                    new_obj.set_property(b"message".to_vec(), Value::String(PhpString::empty()));
                }
                if !new_obj.has_property(b"code") {
                    new_obj.set_property(b"code".to_vec(), Value::Long(0));
                }
                new_obj.set_property(
                    b"file".to_vec(),
                    Value::String(PhpString::from_msg(&vm.current_file.to_string())),
                );
                new_obj.set_property(b"line".to_vec(), Value::Long(vm.current_line as i64));
                new_obj.throwable_set(b"previous", Value::Null);
                let trace = vm.build_exception_trace();
                new_obj.throwable_set(b"trace", trace);
                if vm.class_extends(&class_lower, b"errorexception") {
                    new_obj.set_property(b"severity".to_vec(), Value::Long(1));
                }
            }
            let rc = Rc::new(RefCell::new(new_obj));
            vm.run_object_create_hook(&rc);
            // (destructed once released, like an object made by `new`)
            vm.lazy_list_destructible(&rc);
            Some(Value::Object(rc))
        }
        b"getextension" | b"getextensionname" => {
            let ext = if is_internal_class(vm, &class_lower) {
                crate::stubs::find_class(&class_lower)
                    .or_else(|| crate::stubs::find_self_declared_class(&class_lower))
                    .map(|c| c.ext)
                    .or_else(|| {
                        // ext/dom has no stubs here.
                        (class_lower.starts_with(b"dom") && !class_lower.starts_with(b"domain"))
                            .then_some("dom")
                    })
            } else {
                None
            };
            Some(match (ext, method) {
                (Some(e), b"getextension") => make_reflection_extension(vm, e),
                (Some(e), _) => Value::String(PhpString::from_msg(&e.to_string())),
                (None, b"getextension") => Value::Null,
                (None, _) => Value::False,
            })
        }
        b"innamespace" => Some(if target.contains('\\') {
            Value::True
        } else {
            Value::False
        }),
        b"getnamespacename" => {
            if let Some(pos) = target.rfind('\\') {
                Some(Value::String(PhpString::from_msg(
                    &target[..pos].to_string(),
                )))
            } else {
                Some(Value::String(PhpString::empty()))
            }
        }
        b"getshortname" => {
            if let Some(pos) = target.rfind('\\') {
                Some(Value::String(PhpString::from_msg(
                    &target[pos + 1..].to_string(),
                )))
            } else {
                Some(Value::String(PhpString::from_msg(&target)))
            }
        }
        // ReflectionEnum-specific methods
        b"isbacked" => {
            let is_backed = rcls(vm, &class_lower)
                .map(|c| c.is_enum && c.enum_backing_type.is_some())
                .unwrap_or(false);
            Some(if is_backed { Value::True } else { Value::False })
        }
        b"getbackingtype" => {
            if let Some(ce) = rcls(vm, &class_lower) {
                if let Some(ref bt) = ce.enum_backing_type {
                    Some(create_reflection_type(
                        vm,
                        &crate::opcode::ParamType::Simple(bt.clone()),
                    ))
                } else {
                    Some(Value::Null)
                }
            } else {
                Some(Value::Null)
            }
        }
        b"getcases" => {
            let mut result = PhpArray::new();
            // Collect case info first, then create objects
            let case_info: Vec<(String, bool, String)> = if let Some(ce) = rcls(vm, &class_lower) {
                let canonical = crate::string::msg_str(&ce.name).to_string();
                let is_backed = ce.enum_backing_type.is_some();
                ce.enum_cases
                    .iter()
                    .map(|(case_name, _)| {
                        let case_str = crate::string::msg_str(case_name).to_string();
                        (case_str, is_backed, canonical.clone())
                    })
                    .collect()
            } else {
                vec![]
            };
            for (case_str, is_backed, canonical) in case_info {
                let class_type = if is_backed {
                    "ReflectionEnumBackedCase"
                } else {
                    "ReflectionEnumUnitCase"
                };
                let obj_id = vm.alloc_object_id();
                let mut case_obj = PhpObject::new(class_type.as_bytes().to_vec(), obj_id);
                case_obj.set_property(
                    b"name".to_vec(),
                    Value::String(PhpString::from_msg(&case_str.clone())),
                );
                case_obj.set_property(
                    b"class".to_vec(),
                    Value::String(PhpString::from_msg(&canonical)),
                );
                // The case value is evaluated on demand (getValue() /
                // getBackingValue()), as php-src does.
                result.push(Value::Object(Rc::new(RefCell::new(case_obj))));
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        // hasCase/getCase need args; let docall handle them.
        b"__tostring" => {
            // Build a detailed __toString representation for ReflectionClass
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_msg_string();
            // Detect which reflector this is: ReflectionClass, ReflectionObject,
            // or ReflectionEnum. Each has a slightly different header line.
            let reflector_class: Vec<u8> = ob
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            let wrapped = if reflector_class == b"reflectionobject" {
                match ob.get_property(b"__reflection_object") {
                    Value::Object(o) => Some(o.clone()),
                    _ => None,
                }
            } else {
                None
            };
            drop(ob);
            let kind = if reflector_class == b"reflectionobject" {
                ReflectionStringKind::Object
            } else if reflector_class == b"reflectionenum" {
                ReflectionStringKind::Enum
            } else {
                ReflectionStringKind::Class
            };
            // Evaluate constants first (zend_update_class_constant); an
            // evaluation error aborts the string conversion.
            let const_names: Vec<Vec<u8>> = rcls(vm, &class_lower)
                .map(|c| c.constants.keys().cloned().collect())
                .unwrap_or_default();
            let mut resolved: Vec<(Vec<u8>, Value)> = Vec::new();
            for cn in const_names {
                match resolve_class_constant(vm, &class_lower, &cn) {
                    Some(v) => resolved.push((cn, v)),
                    None => return Some(Value::Null),
                }
            }
            RESOLVED_CONSTS.with(|r| *r.borrow_mut() = Some(resolved));
            let out = reflection_class_to_string(vm, &name, &class_lower, kind, wrapped.as_ref());
            RESOLVED_CONSTS.with(|r| *r.borrow_mut() = None);
            Some(Value::String(PhpString::from_msg(&out)))
        }
        _ => None,
    }
}

/// ReflectionClass methods that need args (dispatched via handle_spl_docall)
pub fn reflection_class_docall(vm: &mut Vm, method: &[u8], args: &[Value]) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let target = ob
            .get_property(b"__reflection_target")
            .to_php_string()
            .to_msg_string();
        let class_lower: Vec<u8> = ob
            .get_property(b"__reflection_target")
            .to_php_string()
            .as_bytes()
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        drop(ob);

        // Z_PARAM_OBJ_OF_CLASS(object, intern->ce)
        let lazy_obj_method = match method {
            b"getlazyinitializer" => Some("getLazyInitializer"),
            b"initializelazyobject" => Some("initializeLazyObject"),
            b"isuninitializedlazyobject" => Some("isUninitializedLazyObject"),
            b"marklazyobjectasinitialized" => Some("markLazyObjectAsInitialized"),
            _ => None,
        };
        if let Some(m) = lazy_obj_method {
            let target = args.get(1).map(|v| v.deref()).unwrap_or(Value::Null);
            let ok = match &target {
                Value::Object(o) => {
                    let ol = o.borrow().class_name.to_ascii_lowercase();
                    ol == class_lower || vm.class_extends(&ol, &class_lower)
                }
                _ => false,
            };
            if !ok {
                let given = match &target {
                    Value::Object(o) => crate::value::display_class_name(&o.borrow().class_name),
                    other => Vm::value_type_name(other),
                };
                let msg = format!(
                    "ReflectionClass::{}(): Argument #1 ($object) must be of type {}, {} given",
                    m,
                    class_display(vm, &class_lower),
                    given
                );
                let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
        }
        // ReflectionClass|string arguments
        if matches!(method, b"implementsinterface" | b"issubclassof") {
            if let Some(Value::Array(_)) = args.get(1).map(|v| v.deref()) {
                let (m, p) = if method == b"implementsinterface" {
                    ("implementsInterface", "interface")
                } else {
                    ("isSubclassOf", "class")
                };
                let msg = format!(
                    "ReflectionClass::{}(): Argument #1 (${}) must be of type ReflectionClass|string, array given",
                    m, p
                );
                let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
        }

        match method {
            b"hasmethod" => {
                if !check_arg_count(vm, args, 1, "ReflectionClass::hasMethod") {
                    return Some(Value::Null);
                }
                let method_name = match require_string_arg(
                    vm,
                    args,
                    0,
                    "ReflectionClass::hasMethod",
                    "name",
                    "string",
                    true,
                ) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let method_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&method_name)
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                let has = rmethod(vm, &class_lower, &method_lower).is_some();
                // Closure::__invoke is implicitly defined.
                let closure_has = class_lower == b"closure"
                    && matches!(
                        method_lower.as_slice(),
                        b"__invoke" | b"bindto" | b"call" | b"bind" | b"fromcallable"
                    );
                Some(if has || closure_has {
                    Value::True
                } else {
                    Value::False
                })
            }
            b"hasproperty" => {
                if !check_arg_count(vm, args, 1, "ReflectionClass::hasProperty") {
                    return Some(Value::Null);
                }
                let prop_name_s = match require_string_arg(
                    vm,
                    args,
                    0,
                    "ReflectionClass::hasProperty",
                    "name",
                    "string",
                    true,
                ) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let prop_name = PhpString::from_msg(&prop_name_s);
                let has = rcls(vm, &class_lower)
                    .map(|_c| {
                        rprops(vm, &class_lower).into_iter().any(|p| {
                            if p.name != prop_name.as_bytes() {
                                return false;
                            }
                            // Inherited private props are not visible.
                            if p.visibility == Visibility::Private
                                && p.declaring_class != class_lower
                            {
                                return false;
                            }
                            true
                        })
                    })
                    .unwrap_or(false);
                // Also check dynamic properties on the reflected object,
                // but exclude properties that ARE declared on the class
                // hierarchy (including inherited private). Those are not
                // "dynamic" for purposes of hasProperty visibility.
                let has_dynamic = if !has {
                    let self_obj = reflected_instance(vm, obj);
                    if let Value::Object(inst) = &self_obj {
                        let io = inst.borrow();
                        let on_instance = io.has_property(prop_name.as_bytes())
                            && !crate::foreach_obj::is_date_view_prop(&io, prop_name.as_bytes());
                        if !on_instance {
                            false
                        } else {
                            // If any class in the hierarchy declares this property,
                            // it is not a true dynamic property.
                            let mut declared_anywhere = false;
                            let mut current = Some(class_lower.clone());
                            while let Some(cl) = current {
                                if let Some(ce) = rcls(vm, &cl) {
                                    if ce.properties.iter().any(|p| p.name == prop_name.as_bytes())
                                    {
                                        declared_anywhere = true;
                                        break;
                                    }
                                    current = ce.parent.as_ref().map(|p| {
                                        p.iter().map(|b| b.to_ascii_lowercase()).collect()
                                    });
                                } else {
                                    break;
                                }
                            }
                            !declared_anywhere
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                Some(if has || has_dynamic {
                    Value::True
                } else {
                    Value::False
                })
            }
            b"hasconstant" => {
                if !check_arg_count(vm, args, 1, "ReflectionClass::hasConstant") {
                    return Some(Value::Null);
                }
                let const_name_s = match require_string_arg(
                    vm,
                    args,
                    0,
                    "ReflectionClass::hasConstant",
                    "name",
                    "string",
                    true,
                ) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let const_name = PhpString::from_msg(&const_name_s);
                let has = reflection_class_has_constant(vm, &class_lower, const_name.as_bytes());
                Some(if has { Value::True } else { Value::False })
            }
            b"getconstant" => {
                if !check_arg_count(vm, args, 1, "ReflectionClass::getConstant") {
                    return Some(Value::Null);
                }
                let const_name_s = match require_string_arg(
                    vm,
                    args,
                    0,
                    "ReflectionClass::getConstant",
                    "name",
                    "string",
                    true,
                ) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let const_name = PhpString::from_msg(&const_name_s);
                let val = if reflection_class_has_constant(vm, &class_lower, const_name.as_bytes())
                {
                    resolve_class_constant(vm, &class_lower, const_name.as_bytes())
                } else {
                    None
                };
                if vm.current_exception.is_some() {
                    Some(Value::Null)
                } else if let Some(v) = val {
                    Some(v)
                } else {
                    // Emit deprecated warning
                    vm.emit_deprecated_at(
                        "ReflectionClass::getConstant() for a non-existent constant is deprecated, use ReflectionClass::hasConstant() to check if the constant exists",
                        vm.current_line,
                    );
                    Some(Value::False)
                }
            }
            b"getconstants" => {
                // Filter: IS_PUBLIC=1, IS_PROTECTED=2, IS_PRIVATE=4, IS_FINAL=0x20
                let filter = args
                    .get(1)
                    .map(|v| {
                        if matches!(v, Value::Null) {
                            -1
                        } else {
                            v.to_long()
                        }
                    })
                    .unwrap_or(-1);
                let mut result = PhpArray::new();
                if let Some(ce) = rcls(vm, &class_lower) {
                    for (name, val) in &ce.constants {
                        // A parent's private constant is not inherited.
                        if ce.constants_meta.get(name).is_some_and(|m| {
                            m.visibility == Visibility::Private
                                && !m.declaring_class.eq_ignore_ascii_case(&class_lower)
                                && !m.declaring_class.is_empty()
                        }) {
                            continue;
                        }
                        if filter != -1 {
                            let vis_flag = ce
                                .constants_meta
                                .get(name)
                                .map(|m| match m.visibility {
                                    Visibility::Public => 1i64,
                                    Visibility::Protected => 2,
                                    Visibility::Private => 4,
                                })
                                .unwrap_or(1);
                            let final_flag = ce
                                .constants_meta
                                .get(name)
                                .map(|m| if m.is_final { 0x20 } else { 0 })
                                .unwrap_or(0);
                            if (filter & vis_flag) == 0 && (filter & final_flag) == 0 {
                                continue;
                            }
                        }
                        result.set(
                            ArrayKey::String(PhpString::from_vec(name.clone())),
                            val.clone(),
                        );
                    }
                }
                // Evaluate pending constant expressions / enum cases.
                let keys: Vec<Vec<u8>> = result
                    .iter()
                    .filter(|(_, v)| is_deferred_const_value(v))
                    .map(|(k, _)| match k {
                        ArrayKey::String(s) => s.as_bytes().to_vec(),
                        ArrayKey::Int(n) => n.to_string().into_bytes(),
                    })
                    .collect();
                for k in keys {
                    // Evaluated in the declaring class (self:: there).
                    let decl = rcls(vm, &class_lower)
                        .and_then(|c| c.constants_meta.get(&k))
                        .map(|m| m.declaring_class.to_ascii_lowercase())
                        .filter(|d| !d.is_empty())
                        .unwrap_or_else(|| class_lower.clone());
                    match resolve_class_constant(vm, &decl, &k) {
                        Some(v) => result.set(ArrayKey::String(PhpString::from_vec(k)), v),
                        None => return Some(Value::Null),
                    }
                }
                // Also check parent constants (no filter applied to inherited
                // for simplicity; PHP does filter these too though).
                if filter == -1 && is_internal_class(vm, &class_lower) {
                    // An internal class inherits its parent's constants
                    // before declaring its own.
                    let mut merged = PhpArray::new();
                    reflection_collect_parent_constants(vm, &class_lower, &mut merged);
                    for (k, v) in result.iter() {
                        merged.set(k.clone(), v.clone());
                    }
                    result = merged;
                } else if filter == -1 {
                    reflection_collect_parent_constants(vm, &class_lower, &mut result);
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"getmethod" => {
                if !check_arg_count(vm, args, 1, "ReflectionClass::getMethod") {
                    return Some(Value::Null);
                }
                let method_name = match require_string_arg(
                    vm,
                    args,
                    0,
                    "ReflectionClass::getMethod",
                    "name",
                    "string",
                    true,
                ) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let method_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&method_name)
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                // Look up the method definition and, if found, use its
                // `declaring_class` for the ReflectionMethod->$class field.
                let method_meta = rmethod(vm, &class_lower, &method_lower).map(|m| {
                    (
                        crate::string::msg_str(&m.name).to_string(),
                        m.declaring_class.clone(),
                    )
                });
                if let Some((canonical_method, declaring_lower)) = method_meta {
                    let declaring_canonical = rcls(vm, declaring_lower.as_slice())
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or_else(|| crate::string::msg_str(&declaring_lower).to_string());
                    // Pass the lookup class so __toString can show
                    // "<user, inherits Parent>" when the method was fetched
                    // from a child class but is declared on a parent.
                    let lookup_canonical = rcls(vm, &class_lower)
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or_else(|| target.clone());
                    Some(create_reflection_method_with_lookup(
                        vm,
                        &declaring_canonical,
                        &canonical_method,
                        Some(&lookup_canonical),
                    ))
                } else if class_lower == b"closure" && method_lower == b"__invoke" {
                    // Synthesise ReflectionMethod for Closure::__invoke by
                    // pointing at the underlying function name captured when
                    // we constructed the ReflectionObject/Class from a closure.
                    let closure_fn: Option<String> = {
                        let ob = obj.borrow();
                        let direct = ob.get_property(b"__reflection_closure_fn");
                        match direct {
                            Value::String(s) => Some(s.to_msg_string()),
                            _ => None,
                        }
                    };
                    // (without a reflected closure, a temporary Closure
                    // object gives zend_get_closure_invoke_method() its
                    // method: it holds a handle meanwhile)
                    let tmp_closure = reflected_closure_tmp_handle(vm, obj);
                    let obj_id = vm.alloc_object_id();
                    if let Some(id) = tmp_closure {
                        crate::object::FREED_OBJECT_IDS.with(|f| f.borrow_mut().push(id));
                    }
                    let mut rm_obj = PhpObject::new(b"ReflectionMethod".to_vec(), obj_id);
                    rm_obj.set_property(
                        b"name".to_vec(),
                        Value::String(PhpString::from_bytes(b"__invoke")),
                    );
                    rm_obj.set_property(
                        b"class".to_vec(),
                        Value::String(PhpString::from_bytes(b"Closure")),
                    );
                    rm_obj.set_property(
                        b"__reflection_class".to_vec(),
                        Value::String(PhpString::from_bytes(b"Closure")),
                    );
                    rm_obj.set_property(
                        b"__reflection_method".to_vec(),
                        Value::String(PhpString::from_bytes(b"__invoke")),
                    );
                    if let Some(fn_n) = closure_fn {
                        rm_obj.set_property(
                            b"__reflection_closure_fn".to_vec(),
                            Value::String(PhpString::from_msg(&fn_n)),
                        );
                    }
                    Some(Value::Object(Rc::new(RefCell::new(rm_obj))))
                } else {
                    let canonical_class = rcls(vm, &class_lower)
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or(target.clone());
                    let err_msg = format!(
                        "Method {}::{}() does not exist",
                        canonical_class, method_name
                    );
                    let exc =
                        vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    Some(Value::Null)
                }
            }
            b"getmethods" => {
                let filter = args
                    .get(1)
                    .map(|v| {
                        if matches!(v, Value::Null) {
                            -1
                        } else {
                            v.to_long()
                        }
                    })
                    .unwrap_or(-1);
                let mut result = PhpArray::new();
                // Collect method info first to avoid borrow issues
                let method_info: Vec<(String, String)> = if rcls(vm, &class_lower).is_some() {
                    rmethods(vm, &class_lower)
                        .into_iter()
                        .filter_map(|method_def| {
                            if filter != -1 {
                                let method_mod = reflection_method_modifiers_static(method_def);
                                if method_mod & filter == 0 {
                                    return None;
                                }
                            }
                            // Inherited private methods are NOT visible.
                            if method_def.visibility == Visibility::Private
                                && method_def.declaring_class != class_lower
                            {
                                return None;
                            }
                            // Property hooks (compiler-synthesised __property_get_/set_
                            // methods) are not real methods.
                            if method_def.name.starts_with(b"__property_get_")
                                || method_def.name.starts_with(b"__property_set_")
                            {
                                return None;
                            }
                            let method_name = crate::string::msg_str(&method_def.name).to_string();
                            let declaring =
                                crate::string::msg_str(&method_def.declaring_class).to_string();
                            Some((declaring, method_name))
                        })
                        .collect()
                } else {
                    vec![]
                };
                for (declaring, method_name) in method_info {
                    let declaring_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&declaring)
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();
                    let declaring_canonical = rcls(vm, &declaring_lower)
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or(declaring);
                    result.push(create_reflection_method(
                        vm,
                        &declaring_canonical,
                        &method_name,
                    ));
                }
                // For Closure include an implicit __invoke method.
                if class_lower == b"closure" {
                    let closure_fn: Option<String> = {
                        let ob = obj.borrow();
                        let direct = ob.get_property(b"__reflection_closure_fn");
                        match direct {
                            Value::String(s) => Some(s.to_msg_string()),
                            _ => None,
                        }
                    };
                    // (without a reflected closure, a temporary Closure
                    // object gives zend_get_closure_invoke_method() its
                    // method: it holds a handle meanwhile)
                    let tmp_closure = reflected_closure_tmp_handle(vm, obj);
                    let obj_id = vm.alloc_object_id();
                    if let Some(id) = tmp_closure {
                        crate::object::FREED_OBJECT_IDS.with(|f| f.borrow_mut().push(id));
                    }
                    let mut rm_obj = PhpObject::new(b"ReflectionMethod".to_vec(), obj_id);
                    rm_obj.set_property(
                        b"name".to_vec(),
                        Value::String(PhpString::from_bytes(b"__invoke")),
                    );
                    rm_obj.set_property(
                        b"class".to_vec(),
                        Value::String(PhpString::from_bytes(b"Closure")),
                    );
                    rm_obj.set_property(
                        b"__reflection_class".to_vec(),
                        Value::String(PhpString::from_bytes(b"Closure")),
                    );
                    rm_obj.set_property(
                        b"__reflection_method".to_vec(),
                        Value::String(PhpString::from_bytes(b"__invoke")),
                    );
                    if let Some(fn_n) = closure_fn {
                        rm_obj.set_property(
                            b"__reflection_closure_fn".to_vec(),
                            Value::String(PhpString::from_msg(&fn_n)),
                        );
                    }
                    result.push(Value::Object(Rc::new(RefCell::new(rm_obj))));
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"getproperty" => {
                if !check_arg_count(vm, args, 1, "ReflectionClass::getProperty") {
                    return Some(Value::Null);
                }
                let prop_input = match require_string_arg(
                    vm,
                    args,
                    0,
                    "ReflectionClass::getProperty",
                    "name",
                    "string",
                    true,
                ) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                // Support fully qualified "ClassName::propName" syntax.
                let (qualified_class_lower, prop_name): (Option<Vec<u8>>, String) =
                    if let Some(pos) = prop_input.find("::") {
                        let qc = &prop_input[..pos];
                        let pn = &prop_input[pos + 2..];
                        (
                            Some(
                                qc.as_bytes()
                                    .iter()
                                    .map(|b| b.to_ascii_lowercase())
                                    .collect(),
                            ),
                            pn.to_string(),
                        )
                    } else {
                        (None, prop_input.clone())
                    };
                if let Some(ref qc_lower) = qualified_class_lower {
                    let is_self = *qc_lower == class_lower;
                    let is_ancestor = vm.class_extends(&class_lower, qc_lower);
                    if !is_self && !is_ancestor {
                        let qc_exists = vm.classes.contains_key(qc_lower)
                            || vm.is_known_builtin_class(qc_lower);
                        if !qc_exists {
                            // Give a registered autoloader a chance to load this class
                            // before reporting it as missing — PHP triggers autoload
                            // when resolving "Class::prop" qualifiers in getProperty().
                            vm.try_autoload_class(qc_lower);
                        }
                        let qc_exists = vm.classes.contains_key(qc_lower)
                            || vm.is_known_builtin_class(qc_lower);
                        if !qc_exists {
                            let qc_canonical = crate::string::msg_str(qc_lower).to_string();
                            let err = format!("Class \"{}\" does not exist", qc_canonical);
                            let exc =
                                vm.create_exception(b"ReflectionException", &err, vm.current_line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                        let qc_canonical = rcls(vm, qc_lower)
                            .map(|c| crate::string::msg_str(&c.name).to_string())
                            .unwrap_or_else(|| crate::string::msg_str(qc_lower).to_string());
                        let err = format!(
                            "Fully qualified property name {}::${} does not specify a base class of {}",
                            qc_canonical, prop_name, target
                        );
                        let exc =
                            vm.create_exception(b"ReflectionException", &err, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }
                let lookup_lower: Vec<u8> = qualified_class_lower
                    .clone()
                    .unwrap_or_else(|| class_lower.clone());
                // Inherited private (instance) props are NOT visible via
                // getProperty without an explicit qualifier; static private
                // props remain accessible.
                // Zend: a private property is only found on its declaring class.
                let prop_info = rprop(vm, &lookup_lower, prop_name.as_bytes())
                    .filter(|p| {
                        p.visibility != Visibility::Private
                            || p.declaring_class.eq_ignore_ascii_case(&lookup_lower)
                    })
                    .map(|p| p.declaring_class.clone());
                // For ReflectionObject, check the actual object for dynamic
                // properties. Inherited private props leave entries on the
                // instance, but those are NOT dynamic — only treat a property
                // as dynamic when it's not declared anywhere on the hierarchy.
                let has_dynamic = if prop_info.is_none() {
                    let self_obj = reflected_instance(vm, obj);
                    if let Value::Object(inst) = &self_obj {
                        let io = inst.borrow();
                        let on_inst = io.has_property(prop_name.as_bytes());
                        drop(io);
                        if !on_inst {
                            false
                        } else {
                            // Check whether declared anywhere in hierarchy
                            let mut declared_anywhere = false;
                            let mut current = Some(class_lower.clone());
                            while let Some(cl) = current {
                                if let Some(ce) = rcls(vm, &cl) {
                                    if ce.properties.iter().any(|p| p.name == prop_name.as_bytes())
                                    {
                                        declared_anywhere = true;
                                        break;
                                    }
                                    current = ce.parent.as_ref().map(|p| {
                                        p.iter().map(|b| b.to_ascii_lowercase()).collect()
                                    });
                                } else {
                                    break;
                                }
                            }
                            !declared_anywhere
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                if let Some(declaring_lower) = prop_info {
                    let declaring_canonical = rcls(vm, declaring_lower.as_slice())
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or_else(|| crate::string::msg_str(&declaring_lower).to_string());
                    Some(create_reflection_property(
                        vm,
                        &declaring_canonical,
                        &prop_name,
                    ))
                } else if has_dynamic {
                    Some(create_reflection_property(vm, &target, &prop_name))
                } else {
                    let err_msg = format!("Property {}::${} does not exist", target, prop_name);
                    let exc =
                        vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    Some(Value::Null)
                }
            }
            b"getproperties" => {
                let filter = args
                    .get(1)
                    .map(|v| {
                        if matches!(v, Value::Null) {
                            -1
                        } else {
                            v.to_long()
                        }
                    })
                    .unwrap_or(-1);
                let mut result = PhpArray::new();
                let mut declared_names: Vec<Vec<u8>> = Vec::new();
                // Collect declared properties (both static and instance) along
                // with their declaring class. Private props from a parent are
                // not visible on subclasses.
                let declared: Vec<(String, Vec<u8>)> = if rcls(vm, &class_lower).is_some() {
                    let mut v = Vec::new();
                    for prop in rprops(vm, &class_lower) {
                        if filter != -1 {
                            let prop_mod = reflection_property_modifiers_static(prop);
                            if prop_mod & filter == 0 {
                                continue;
                            }
                        }
                        if prop.visibility == Visibility::Private
                            && prop.declaring_class != class_lower
                        {
                            continue;
                        }
                        let name_str = crate::string::msg_str(&prop.name).to_string();
                        v.push((name_str, prop.declaring_class.clone()));
                        declared_names.push(prop.name.clone());
                    }
                    v
                } else {
                    Vec::new()
                };
                for (name_str, declaring_lower) in declared {
                    let declaring_canonical = rcls(vm, declaring_lower.as_slice())
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or_else(|| crate::string::msg_str(&declaring_lower).to_string());
                    result.push(create_reflection_property(
                        vm,
                        &declaring_canonical,
                        &name_str,
                    ));
                }
                // For ReflectionObject, append dynamic instance properties
                // (these are considered IS_PUBLIC). Collect names first.
                let include_dynamic = filter == -1 || (filter & 1) != 0;
                if include_dynamic {
                    // Collect props on the instance that are NOT declared on the
                    // runtime class. For inherited private props, the class.properties
                    // list will already include them — so we filter them by name AND by
                    // checking if there's a *visible* (non-private-from-parent) declared
                    // prop with that name.
                    let visible_declared_names: Vec<Vec<u8>> =
                        if let Some(ce) = rcls(vm, &class_lower) {
                            ce.properties
                                .iter()
                                .filter(|p| {
                                    !(p.visibility == Visibility::Private
                                        && p.declaring_class != class_lower)
                                })
                                .map(|p| p.name.clone())
                                .collect()
                        } else {
                            Vec::new()
                        };
                    let dyn_names: Vec<Vec<u8>> = {
                        let self_obj = reflected_instance(vm, obj);
                        if let Value::Object(inst) = &self_obj {
                            // The dynamic entries of its property table
                            // (a DateTime's date/timezone are only views).
                            let table = vm.object_prop_table(inst);
                            let ib = inst.borrow();
                            table
                                .into_iter()
                                .filter(|e| e.slot.is_none())
                                .map(|e| e.key)
                                .filter(|name| !name.starts_with(b"__") && name.first() != Some(&0))
                                .filter(|name| !crate::foreach_obj::is_date_view_prop(&ib, name))
                                .filter(|name| !visible_declared_names.iter().any(|n| n == name))
                                .collect()
                        } else {
                            Vec::new()
                        }
                    };
                    for dn in dyn_names {
                        let name_str = crate::string::msg_str(&dn).to_string();
                        let rp = create_reflection_property(vm, &target, &name_str);
                        // Mark this ReflectionProperty as referring to a dynamic
                        // (instance-only) property so visibility queries return
                        // public and declaring class reports the runtime class.
                        if let Value::Object(rp_obj) = &rp {
                            rp_obj
                                .borrow_mut()
                                .set_property(b"__reflection_prop_dynamic".to_vec(), Value::True);
                        }
                        result.push(rp);
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"issubclassof" => {
                let user_arg_count = args.len().saturating_sub(1);
                if user_arg_count == 0 {
                    let err_msg =
                        "ReflectionClass::isSubclassOf() expects exactly 1 argument, 0 given"
                            .to_string();
                    let exc = vm.create_exception(b"ArgumentCountError", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                if user_arg_count > 1 {
                    let err_msg = format!(
                        "ReflectionClass::isSubclassOf() expects exactly 1 argument, {} given",
                        user_arg_count
                    );
                    let exc = vm.create_exception(b"ArgumentCountError", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let parent_arg = args.get(1)?.clone();
                let Some(parent_name) =
                    reflection_class_or_str_arg(vm, &parent_arg, "isSubclassOf", "class")
                else {
                    return Some(Value::Null);
                };
                let parent_lower: Vec<u8> = parent_name
                    .as_bytes()
                    .strip_prefix(b"\\")
                    .unwrap_or(parent_name.as_bytes())
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                // zend_lookup_class(): the class is autoloaded first.
                if !vm.classes.contains_key(&parent_lower)
                    && !vm.is_known_builtin_class(&parent_lower)
                {
                    vm.try_autoload_class(&crate::errdisplay::raw_error_bytes(&parent_name));
                    if vm.current_exception.is_some() {
                        return Some(Value::Null);
                    }
                }
                // Validate that the parent class exists (PHP throws ReflectionException otherwise)
                let parent_exists = vm.classes.contains_key(&parent_lower)
                    || vm.is_known_builtin_class(&parent_lower);
                if !parent_exists {
                    let err_msg = format!("Class \"{}\" does not exist", parent_name);
                    let exc =
                        vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // A class is not a subclass of itself
                if class_lower == parent_lower {
                    return Some(Value::False);
                }
                let result = vm.class_extends(&class_lower, &parent_lower)
                    || vm.class_implements_interface(&class_lower, &parent_lower)
                    || super::vm::is_builtin_subclass(&class_lower, &parent_lower);
                Some(if result { Value::True } else { Value::False })
            }
            b"implementsinterface" => {
                let user_arg_count = args.len().saturating_sub(1);
                if user_arg_count == 0 {
                    let err_msg = "ReflectionClass::implementsInterface() expects exactly 1 argument, 0 given".to_string();
                    let exc = vm.create_exception(b"ArgumentCountError", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                if user_arg_count > 1 {
                    let err_msg = format!(
                        "ReflectionClass::implementsInterface() expects exactly 1 argument, {} given",
                        user_arg_count
                    );
                    let exc = vm.create_exception(b"ArgumentCountError", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let iface_arg = args.get(1)?.clone();
                let Some(iface_name) =
                    reflection_class_or_str_arg(vm, &iface_arg, "implementsInterface", "interface")
                else {
                    return Some(Value::Null);
                };
                let iface_lower: Vec<u8> = iface_name
                    .as_bytes()
                    .strip_prefix(b"\\")
                    .unwrap_or(iface_name.as_bytes())
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();

                // PHP triggers autoload before reporting an interface as
                // missing from implementsInterface().
                if !vm.classes.contains_key(&iface_lower)
                    && !vm.is_known_builtin_class(&iface_lower)
                {
                    vm.try_autoload_class(&crate::errdisplay::raw_error_bytes(&iface_name));
                }

                // Check if the target is actually an interface
                let is_interface = rcls(vm, &iface_lower)
                    .map(|c| c.is_interface)
                    .unwrap_or_else(|| vm.builtin_is_interface(&iface_lower));

                if !is_interface {
                    // Check if class exists
                    let exists = vm.classes.contains_key(&iface_lower)
                        || vm.is_known_builtin_class(&iface_lower);
                    if exists {
                        // (named as declared)
                        let shown = rcls(vm, &iface_lower)
                            .map(|c| crate::string::msg_str(&c.name).into_owned())
                            .unwrap_or_else(|| vm.builtin_canonical_name(&iface_lower));
                        let err_msg = format!("{} is not an interface", shown);
                        let exc =
                            vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    } else {
                        let err_msg = format!("Interface \"{}\" does not exist", iface_name);
                        let exc =
                            vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }

                // An interface "implements" itself per PHP semantics.
                let result = class_lower == iface_lower
                    || vm.class_implements_interface(&class_lower, &iface_lower)
                    || vm.builtin_implements_interface(&class_lower, &iface_lower);
                Some(if result { Value::True } else { Value::False })
            }
            b"isinstance" => {
                let instance = args.get(1)?;
                if let Value::Object(inst_obj) = instance {
                    let inst_class: Vec<u8> = inst_obj
                        .borrow()
                        .class_name
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();
                    let result = inst_class == class_lower
                        || vm.class_extends(&inst_class, &class_lower)
                        || vm.class_implements_interface(&inst_class, &class_lower)
                        || super::vm::is_builtin_subclass(&inst_class, &class_lower);
                    Some(if result { Value::True } else { Value::False })
                } else {
                    Some(Value::False)
                }
            }
            b"newinstance" => {
                // (extra named arguments, see spl_place_named_args())
                let fwd_named = std::mem::take(&mut vm.pending_named_args);
                if let Some(msg) = internal_get_constructor_error(&class_lower) {
                    let exc = vm.create_exception(b"Error", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // Create instance and call constructor with remaining args
                let canonical = rcls(vm, &class_lower)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());

                // object_init_ex: interfaces, traits, enums and abstract classes
                if let Some(err_msg) = instantiation_error(vm, &class_lower) {
                    let exc = vm.create_exception(b"Error", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }

                let has_constructor = rcls(vm, &class_lower)
                    .map(|c| c.get_method(b"__construct").is_some())
                    .unwrap_or(false)
                    || native_ctor_in_chain(vm, &class_lower);

                // If no constructor and args were passed, throw
                if !has_constructor && (args.len() > 1 || !fwd_named.is_empty()) {
                    let canonical_name = crate::string::msg_str(&canonical).to_string();
                    let err_msg = format!(
                        "Class {} does not have a constructor, so you cannot pass any constructor arguments",
                        canonical_name
                    );
                    // (object_init_ex() made the object first: freed after)
                    let made = vm.alloc_object_id();
                    let exc =
                        vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    crate::object::FREED_OBJECT_IDS.with(|f| f.borrow_mut().push(made));
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }

                // Check constructor visibility
                if has_constructor {
                    let ctor_visibility = rcls(vm, &class_lower)
                        .and_then(|c| c.get_method(b"__construct"))
                        .map(|m| m.visibility);
                    if matches!(
                        ctor_visibility,
                        Some(Visibility::Protected) | Some(Visibility::Private)
                    ) {
                        let canonical_name = crate::string::msg_str(&canonical).to_string();
                        let err_msg = format!(
                            "Access to non-public constructor of class {}",
                            canonical_name
                        );
                        // (object_init_ex() made the object first: freed after)
                        let made = vm.alloc_object_id();
                        let exc =
                            vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                        crate::object::FREED_OBJECT_IDS.with(|f| f.borrow_mut().push(made));
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }

                // An internal constructor: construct like `new` does.
                if rmethod(vm, &class_lower, b"__construct")
                    .is_some_and(|m| m.op_array.internal_sig.is_some())
                    || native_ctor_in_chain(vm, &class_lower)
                {
                    let mut a = PhpArray::new();
                    for arg in args.iter().skip(1) {
                        a.push(arg.clone());
                    }
                    for (n, v) in fwd_named {
                        a.set(ArrayKey::String(PhpString::from_vec(n)), v);
                    }
                    return Some(
                        run_snippet_in_scope(
                            vm,
                            "return new $__c(...$__a);",
                            &[
                                ("__c", Value::String(PhpString::from_vec(canonical.clone()))),
                                ("__a", Value::Array(Rc::new(RefCell::new(a)))),
                            ],
                            &class_lower,
                        )
                        .unwrap_or(Value::Null),
                    );
                }
                let mut a = PhpArray::new();
                for arg in args.iter().skip(1) {
                    a.push(arg.clone());
                }
                for (n, v) in fwd_named {
                    a.set(ArrayKey::String(PhpString::from_vec(n)), v);
                }
                Some(reflection_new_instance(
                    vm,
                    &canonical,
                    &class_lower,
                    Value::Array(Rc::new(RefCell::new(a))),
                ))
            }
            b"newinstanceargs" => {
                if let Some(msg) = internal_get_constructor_error(&class_lower) {
                    let exc = vm.create_exception(b"Error", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // Same as newInstance but takes an array of args
                // Validate the argument is an array (or null/missing)
                let args_arr = match args.get(1) {
                    Some(Value::Array(a)) => Value::Array(a.clone()),
                    Some(Value::Null) | None => {
                        Value::Array(Rc::new(RefCell::new(PhpArray::new())))
                    }
                    Some(other) => {
                        let got = match other {
                            Value::Long(_) => "int",
                            Value::Double(_) => "float",
                            Value::String(_) => "string",
                            Value::True | Value::False => "bool",
                            Value::Object(o) => {
                                let _ob = o.borrow();
                                "object"
                            }
                            _ => "unknown",
                        };
                        let err_msg = format!(
                            "ReflectionClass::newInstanceArgs(): Argument #1 ($args) must be of type array, {} given",
                            got
                        );
                        let exc = vm.create_exception(b"TypeError", &err_msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                };
                let ctor_args: Vec<Value> = if let Value::Array(arr) = &args_arr {
                    arr.borrow().iter().map(|(_, v)| v.clone()).collect()
                } else {
                    vec![]
                };

                let canonical = rcls(vm, &class_lower)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());

                // object_init_ex: interfaces, traits, enums and abstract classes
                if let Some(err_msg) = instantiation_error(vm, &class_lower) {
                    let exc = vm.create_exception(b"Error", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }

                let has_constructor = rcls(vm, &class_lower)
                    .map(|c| c.get_method(b"__construct").is_some())
                    .unwrap_or(false)
                    || native_ctor_in_chain(vm, &class_lower);

                // If no constructor and args were passed, throw
                if !has_constructor && !ctor_args.is_empty() {
                    let canonical_name = crate::string::msg_str(&canonical).to_string();
                    let err_msg = format!(
                        "Class {} does not have a constructor, so you cannot pass any constructor arguments",
                        canonical_name
                    );
                    // (object_init_ex() made the object first: freed after)
                    let made = vm.alloc_object_id();
                    let exc =
                        vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    crate::object::FREED_OBJECT_IDS.with(|f| f.borrow_mut().push(made));
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }

                // Check constructor visibility
                if has_constructor {
                    let ctor_visibility = rcls(vm, &class_lower)
                        .and_then(|c| c.get_method(b"__construct"))
                        .map(|m| m.visibility);
                    if matches!(
                        ctor_visibility,
                        Some(Visibility::Protected) | Some(Visibility::Private)
                    ) {
                        let canonical_name = crate::string::msg_str(&canonical).to_string();
                        let err_msg = format!(
                            "Access to non-public constructor of class {}",
                            canonical_name
                        );
                        // (object_init_ex() made the object first: freed after)
                        let made = vm.alloc_object_id();
                        let exc =
                            vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                        crate::object::FREED_OBJECT_IDS.with(|f| f.borrow_mut().push(made));
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }

                // An internal constructor: construct like `new` does.
                if rmethod(vm, &class_lower, b"__construct")
                    .is_some_and(|m| m.op_array.internal_sig.is_some())
                    || native_ctor_in_chain(vm, &class_lower)
                {
                    return Some(
                        run_snippet_in_scope(
                            vm,
                            "return new $__c(...$__a);",
                            &[
                                ("__c", Value::String(PhpString::from_vec(canonical.clone()))),
                                ("__a", args_arr.clone()),
                            ],
                            &class_lower,
                        )
                        .unwrap_or(Value::Null),
                    );
                }
                Some(reflection_new_instance(
                    vm,
                    &canonical,
                    &class_lower,
                    args_arr.clone(),
                ))
            }
            b"resetaslazyghost" | b"resetaslazyproxy" => {
                // zend_object_make_lazy() on an existing object.
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                let initializer = args.get(2).cloned().unwrap_or(Value::Null);
                let options = args.get(3).map(|v| v.to_long()).unwrap_or(0);
                let is_proxy = method == b"resetaslazyproxy";
                let method_name = if is_proxy {
                    "resetAsLazyProxy"
                } else {
                    "resetAsLazyGhost"
                };
                let Value::Object(obj) = &target else {
                    let ce_disp = vm
                        .classes
                        .get(&class_lower)
                        .map(|c| crate::value::display_class_name(&c.name))
                        .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower));
                    let err_msg = format!(
                        "ReflectionClass::{}(): Argument #1 ($object) must be of type {}, {} given",
                        method_name,
                        ce_disp,
                        Vm::value_type_name(&target)
                    );
                    let exc = vm.create_exception(b"TypeError", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                };
                let obj_class_lower: Vec<u8> = obj
                    .borrow()
                    .class_name
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
                if obj_class_lower != class_lower
                    && !vm.class_extends(&obj_class_lower, &class_lower)
                {
                    let ce_disp = vm
                        .classes
                        .get(&class_lower)
                        .map(|c| crate::value::display_class_name(&c.name))
                        .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower));
                    let given = crate::value::display_class_name(&obj.borrow().class_name);
                    let err_msg = format!(
                        "ReflectionClass::{}(): Argument #1 ($object) must be of type {}, {} given",
                        method_name, ce_disp, given
                    );
                    let exc = vm.create_exception(b"TypeError", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                if !lazy_check_initializer_arg(vm, &initializer, method_name, 2) {
                    return Some(Value::Null);
                }
                // ZEND_LAZY_OBJECT_USER_MASK = SKIP_INITIALIZATION_ON_SERIALIZE | SKIP_DESTRUCTOR
                if options & !(LAZY_SKIP_INIT_ON_SERIALIZE | LAZY_SKIP_DESTRUCTOR) != 0 {
                    let err_msg = format!(
                        "ReflectionClass::{}(): Argument #3 ($options) contains invalid flags",
                        method_name
                    );
                    let exc =
                        vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let (state, has_real) = {
                    let ob = obj.borrow();
                    (
                        ob.get_property(b"__lazy_state"),
                        ob.has_property(b"__lazy_real"),
                    )
                };
                let is_lazy = matches!(state, Value::String(_));
                let is_init_proxy =
                    matches!(&state, Value::String(s) if s.as_bytes() == b"proxy") && has_real;
                // While its initializer runs, an object is no longer lazy but
                // can't be reset either.
                if vm.is_lazy_init_in_progress(obj.borrow().object_id) {
                    let exc = vm.create_exception(
                        b"Error",
                        "Can not reset an object while it is being initialized",
                        vm.current_line,
                    );
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                if is_lazy && !is_init_proxy {
                    let exc = vm.create_exception(
                        b"ReflectionException",
                        "Object is already lazy",
                        vm.current_line,
                    );
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                if is_init_proxy {
                    let mut ob = obj.borrow_mut();
                    ob.remove_property(b"__lazy_real");
                    ob.remove_property(b"__lazy_state");
                    ob.remove_property(b"__lazy_initializer");
                    ob.remove_property(b"__lazy_skipped");
                    ob.remove_property(b"__lazy_flags");
                } else {
                    let oid = obj.borrow().object_id;
                    if vm.is_lazy_init_in_progress(oid) {
                        let exc = vm.create_exception(
                            b"Error",
                            "Can not reset an object while it is being initialized",
                            vm.current_line,
                        );
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    if options & LAZY_SKIP_DESTRUCTOR == 0 && !vm.call_destructor_now(obj) {
                        return Some(Value::Null);
                    }
                }
                obj.borrow_mut().remove_property(b"__destructed");
                // Unset dynamic properties, then the declared properties of the
                // reflected class (initialized readonly props of final
                // classes/props are kept and stay non-lazy).
                let obj_final = vm
                    .classes
                    .get(&obj_class_lower)
                    .map(|c| c.is_final)
                    .unwrap_or(false);
                let ce_defs = vm.ordered_instance_prop_defs(&class_lower);
                let obj_defs = vm.ordered_instance_prop_defs(&obj_class_lower);
                let mut removed: Vec<Value> = Vec::new();
                let mut skipped: Vec<Vec<u8>> = Vec::new();
                let mut lazy_count = 0usize;
                // (dynamic properties mean the object's properties table
                // was built: var_dump() then counts its declared slots)
                let had_dynamic;
                {
                    let mut ob = obj.borrow_mut();
                    let dyn_names: Vec<Vec<u8>> = ob
                        .properties
                        .iter()
                        .filter(|(n, _)| {
                            !n.starts_with(b"__") && !obj_defs.iter().any(|d| &d.name == n)
                        })
                        .map(|(n, _)| n.clone())
                        .collect();
                    had_dynamic = !dyn_names.is_empty();
                    for n in dyn_names.iter().rev() {
                        removed.push(ob.get_property(n));
                        ob.remove_property(n);
                    }
                    for d in &obj_defs {
                        if d.is_virtual && (d.has_get_hook || d.has_set_hook) {
                            continue;
                        }
                        let in_ce = ce_defs.iter().any(|c| c.name == d.name);
                        if !in_ce {
                            skipped.push(d.name.clone());
                            continue;
                        }
                        let cur = ob.get_property(&d.name);
                        if !matches!(cur, Value::Undef)
                            && d.is_readonly
                            && (obj_final || d.is_final)
                        {
                            skipped.push(d.name.clone());
                            continue;
                        }
                        removed.push(cur);
                        // The slot lets go of a reference it held (which
                        // loses this property as a type source).
                        match ob.properties.iter_mut().find(|(k, _)| *k == d.name) {
                            Some(slot) => {
                                removed.push(std::mem::replace(&mut slot.1, Value::Undef))
                            }
                            None => ob.set_property(d.name.clone(), Value::Undef),
                        }
                        lazy_count += 1;
                    }
                }
                // Release the old values now (may run nested destructors).
                drop(removed);
                vm.run_pending_destructors_pub();
                if lazy_count > 0 {
                    let mut ob = obj.borrow_mut();
                    ob.set_property(
                        b"__lazy_state".to_vec(),
                        Value::String(PhpString::from_bytes(if is_proxy {
                            b"proxy"
                        } else {
                            b"ghost"
                        })),
                    );
                    ob.set_property(b"__lazy_initializer".to_vec(), initializer);
                    if had_dynamic {
                        let slots = obj_defs
                            .iter()
                            .filter(|d| !(d.is_virtual && (d.has_get_hook || d.has_set_hook)))
                            .count();
                        ob.set_property(b"__lazy_htcount".to_vec(), Value::Long(slots as i64));
                    } else {
                        ob.remove_property(b"__lazy_htcount");
                    }
                    if !skipped.is_empty() {
                        ob.set_property(
                            b"__lazy_skipped".to_vec(),
                            Value::String(PhpString::from_vec(skipped.join(&0u8))),
                        );
                    } else {
                        ob.remove_property(b"__lazy_skipped");
                    }
                    if options & LAZY_SKIP_INIT_ON_SERIALIZE != 0 {
                        ob.set_property(b"__lazy_flags".to_vec(), Value::Long(options));
                    } else {
                        ob.remove_property(b"__lazy_flags");
                    }
                }
                Some(Value::Null)
            }
            b"newlazyghost" | b"newlazyproxy" => {
                let initializer = args.get(1).cloned().unwrap_or(Value::Null);
                let options = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                let is_proxy = method == b"newlazyproxy";
                let method_disp = if is_proxy {
                    "newLazyProxy"
                } else {
                    "newLazyGhost"
                };
                if !lazy_check_initializer_arg(vm, &initializer, method_disp, 1) {
                    return Some(Value::Null);
                }
                // Validate options: only SKIP_INITIALIZATION_ON_SERIALIZE (1)
                // is allowed for newLazyGhost/newLazyProxy. SKIP_DESTRUCTOR (2)
                // is only allowed on resetAsLazyProxy.
                if options & !LAZY_SKIP_INIT_ON_SERIALIZE != 0 {
                    let method_name = if is_proxy {
                        "newLazyProxy"
                    } else {
                        "newLazyGhost"
                    };
                    let err_msg = if options & LAZY_SKIP_DESTRUCTOR != 0
                        && options & !(LAZY_SKIP_INIT_ON_SERIALIZE | LAZY_SKIP_DESTRUCTOR) == 0
                    {
                        format!(
                            "ReflectionClass::{}(): Argument #2 ($options) does not accept ReflectionClass::SKIP_DESTRUCTOR",
                            method_name
                        )
                    } else {
                        format!(
                            "ReflectionClass::{}(): Argument #2 ($options) contains invalid flags",
                            method_name
                        )
                    };
                    let exc = vm.create_exception(b"ReflectionException", &err_msg, 0);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // PHP: internal classes cannot be initialized lazily.
                // stdClass is whitelisted as it is considered safe.
                if !vm.classes.contains_key(&class_lower) && class_lower != b"stdclass" {
                    let canonical = vm.builtin_canonical_name(&class_lower);
                    let err_msg = format!(
                        "Cannot make instance of internal class lazy: {} is internal",
                        canonical
                    );
                    let exc = vm.create_exception(b"Error", &err_msg, 0);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // PHP: sub-classes of internal classes (other than stdClass) cannot
                // be initialized lazily either.
                {
                    let mut cur = class_lower.clone();
                    let canonical_self = rcls(vm, &class_lower)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());
                    let self_disp = crate::string::msg_str(&canonical_self).to_string();
                    let mut bad_internal_ancestor: Option<String> = None;
                    while let Some(ce) = rcls(vm, &cur) {
                        if let Some(parent_lower) = &ce.parent {
                            let pl: Vec<u8> = parent_lower
                                .iter()
                                .map(|b| b.to_ascii_lowercase())
                                .collect();
                            if pl == b"stdclass" {
                                break;
                            }
                            if !vm.classes.contains_key(&pl) {
                                // Parent is internal
                                let parent_disp = vm.builtin_canonical_name(&pl);
                                bad_internal_ancestor = Some(parent_disp);
                                break;
                            }
                            cur = pl;
                        } else {
                            break;
                        }
                    }
                    if let Some(parent_name) = bad_internal_ancestor {
                        let err_msg = format!(
                            "Cannot make instance of internal class lazy: {} inherits internal class {}",
                            self_disp, parent_name
                        );
                        let exc = vm.create_exception(b"Error", &err_msg, 0);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }
                // object_init_ex()'s error for an uninstantiable class.
                if let Some(msg) = uninstantiable_class_error(vm, &class_lower) {
                    let exc = vm.create_exception(b"Error", &msg, 0);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // zend_object_make_lazy(): the class's constants and default
                // values are evaluated first.
                if vm.update_class_constants(&class_lower).is_err() {
                    return Some(Value::Null);
                }
                let obj_id = vm.alloc_object_id();
                let canonical = rcls(vm, &class_lower)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| vm.builtin_canonical_name(&class_lower).into_bytes());
                let mut new_obj = PhpObject::new(canonical, obj_id);
                // PHP: classes with no declared instance properties are never lazy
                // — they are returned as already-initialized objects (no initializer).
                let decl_prop_count = rcls(vm, &class_lower)
                    .map(|ce| {
                        ce.properties
                            .iter()
                            .filter(|p| !p.is_static && !p.is_virtual)
                            .count()
                    })
                    .unwrap_or(0);
                if decl_prop_count == 0 {
                    let obj = Rc::new(RefCell::new(new_obj));
                    vm.lazy_list_destructible(&obj);
                    return Some(Value::Object(obj));
                }
                // For lazy objects, all declared properties are seeded as Undef
                // in declaration order so later assignments preserve that order.
                // Typed properties will render as uninitialized() in var_dump
                // until the initializer runs.
                if let Some(ce) = rcls(vm, &class_lower) {
                    let decl_props: Vec<_> = ce
                        .properties
                        .iter()
                        .filter(|p| !p.is_static)
                        .map(|p| p.name.clone())
                        .collect();
                    for name in decl_props {
                        new_obj.set_property(name, Value::Undef);
                    }
                }
                new_obj.set_property(
                    b"__lazy_state".to_vec(),
                    Value::String(PhpString::from_bytes(if is_proxy {
                        b"proxy"
                    } else {
                        b"ghost"
                    })),
                );
                new_obj.set_property(b"__lazy_initializer".to_vec(), initializer);
                if options & LAZY_SKIP_INIT_ON_SERIALIZE != 0 {
                    new_obj.set_property(b"__lazy_flags".to_vec(), Value::Long(options));
                }
                Some(Value::Object(Rc::new(RefCell::new(new_obj))))
            }
            b"initializelazyobject" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Object(inner) = &target {
                    vm.maybe_run_lazy_init(inner);
                    // For proxies, return the real backing instance.
                    let ob = inner.borrow();
                    let state = ob.get_property(b"__lazy_state");
                    let is_proxy = matches!(&state, Value::String(s) if s.as_bytes() == b"proxy");
                    if is_proxy {
                        if let Value::Object(real) = ob.get_property(b"__lazy_real") {
                            return Some(Value::Object(real));
                        }
                    }
                }
                Some(target)
            }
            b"marklazyobjectasinitialized" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Object(inner) = &target {
                    if Vm::is_uninit_lazy(inner) {
                        // zend_lazy_object_mark_as_initialized(): the still
                        // lazy properties get their default values, and the
                        // object (even a proxy) becomes a regular one.
                        let obj_class_lower: Vec<u8> = inner
                            .borrow()
                            .class_name
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect();
                        let defaults: Vec<(Vec<u8>, Value)> = rcls(vm, &obj_class_lower)
                            .map(|ce| {
                                ce.properties
                                    .iter()
                                    .filter(|p| {
                                        !p.is_static
                                            && !p.is_virtual
                                            && !matches!(p.default, Value::Undef)
                                    })
                                    .filter(|p| !crate::vm::is_prop_skipped(inner, &p.name))
                                    .map(|p| (p.name.clone(), p.default.clone()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        {
                            let mut ob = inner.borrow_mut();
                            for (name, val) in defaults {
                                ob.set_property(name, val);
                            }
                        }
                        vm.lazy_realize(inner);
                    }
                    if let Value::Object(real) = inner.borrow().get_property(b"__lazy_real") {
                        return Some(Value::Object(real));
                    }
                }
                Some(target)
            }
            b"isuninitializedlazyobject" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Object(inner) = &target {
                    let ob = inner.borrow();
                    let state = ob.get_property(b"__lazy_state");
                    let is_ghost =
                        matches!(state, Value::String(ref s) if s.as_bytes() == b"ghost");
                    let is_uninit_proxy = matches!(state, Value::String(ref s) if s.as_bytes() == b"proxy")
                        && !ob.has_property(b"__lazy_real");
                    Some(if is_ghost || is_uninit_proxy {
                        Value::True
                    } else {
                        Value::False
                    })
                } else {
                    Some(Value::False)
                }
            }
            b"getlazyinitializer" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Object(inner) = &target {
                    let ob = inner.borrow();
                    let state = ob.get_property(b"__lazy_state");
                    let is_uninit = match &state {
                        Value::String(s) if s.as_bytes() == b"ghost" => true,
                        Value::String(s) if s.as_bytes() == b"proxy" => {
                            !ob.has_property(b"__lazy_real")
                        }
                        _ => false,
                    };
                    if is_uninit {
                        Some(ob.get_property(b"__lazy_initializer"))
                    } else {
                        Some(Value::Null)
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"getlazyproxyinstance" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if let Value::Object(inner) = &target {
                    let ob = inner.borrow();
                    let real = ob.get_property(b"__lazy_real");
                    if matches!(real, Value::Object(_)) {
                        Some(real)
                    } else {
                        Some(Value::Null)
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"getstaticpropertyvalue" => {
                if !check_arg_count_range(vm, args, 1, 2, "ReflectionClass::getStaticPropertyValue")
                {
                    return Some(Value::Null);
                }
                let prop_name_s = match require_string_arg(
                    vm,
                    args,
                    0,
                    "ReflectionClass::getStaticPropertyValue",
                    "name",
                    "string",
                    true,
                ) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let prop_name = PhpString::from_msg(&prop_name_s);
                let default = args.get(2);
                // zend_std_get_static_property() from the class's scope.
                let declared_static =
                    visible_static_prop(vm, &class_lower, prop_name.as_bytes()).is_some();
                if declared_static {
                    let val = find_static_property_value(vm, &class_lower, prop_name.as_bytes());
                    match val {
                        Some(v) if !matches!(v, Value::Undef) => return Some(v.deref()),
                        _ => {}
                    }
                    if let Some(d) = default {
                        return Some(d.clone());
                    }
                    let cname = rcls(vm, &class_lower)
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or_else(|| target.clone());
                    let err_msg = format!(
                        "Typed property {}::${} must not be accessed before initialization",
                        cname,
                        prop_name.to_msg_string()
                    );
                    let exc = vm.create_exception(b"Error", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // Property doesn't exist on the class. PHP: if a default
                // was provided, return it; otherwise throw.
                if let Some(d) = default {
                    return Some(d.clone());
                }
                let err_msg = format!(
                    "Property {}::${} does not exist",
                    target,
                    prop_name.to_msg_string()
                );
                let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                vm.current_exception = Some(exc);
                Some(Value::Null)
            }
            b"setstaticpropertyvalue" => {
                if !check_arg_count(vm, args, 2, "ReflectionClass::setStaticPropertyValue") {
                    return Some(Value::Null);
                }
                let prop_name_s = match require_string_arg(
                    vm,
                    args,
                    0,
                    "ReflectionClass::setStaticPropertyValue",
                    "name",
                    "string",
                    true,
                ) {
                    Some(s) => s,
                    None => return Some(Value::Null),
                };
                let prop_name = PhpString::from_msg(&prop_name_s.clone());
                let value = args.get(2).cloned().unwrap_or(Value::Null);
                // Validate that this static prop exists on the class (or an ancestor)
                let declared =
                    visible_static_prop(vm, &class_lower, prop_name.as_bytes()).is_some();
                if !declared {
                    let canonical = rcls(vm, &class_lower)
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or(target.clone());
                    let err_msg = format!(
                        "Class {} does not have a property named {}",
                        canonical, prop_name_s
                    );
                    let exc =
                        vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // Enforce property type if declared
                let static_def = visible_static_prop(vm, &class_lower, prop_name.as_bytes());
                let prop_type = static_def.as_ref().and_then(|p| p.property_type.clone());
                // (the TypeError names the declaring class)
                let decl_display = static_def
                    .as_ref()
                    .and_then(|p| rcls(vm, &p.declaring_class.to_ascii_lowercase()))
                    .map(|c| crate::string::msg_str(&c.name).to_string());
                let mut coerced = value;
                if let Some(pt) = prop_type.as_ref() {
                    let strict = false;
                    let original = coerced.clone();
                    crate::vm::Vm::coerce_value_to_type(&mut coerced, pt, strict);
                    if !vm.value_matches_type(&coerced, pt) {
                        let canonical = decl_display.clone().unwrap_or(target.clone());
                        let type_str = param_type_name(pt);
                        let value_type = match &original {
                            Value::Null => "null",
                            Value::True | Value::False => "bool",
                            Value::Long(_) => "int",
                            Value::Double(_) => "float",
                            Value::String(_) => "string",
                            Value::Array(_) => "array",
                            Value::Object(_) => "object",
                            _ => "unknown",
                        };
                        let err_msg = format!(
                            "Cannot assign {} to property {}::${} of type {}",
                            value_type, canonical, prop_name_s, type_str
                        );
                        let exc = vm.create_exception(b"TypeError", &err_msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }
                let _ = vm.static_prop_assign_typed(&class_lower, prop_name.as_bytes(), coerced);
                Some(Value::Null)
            }
            b"getreflectionconstant" | b"getreflectionconstants" => {
                // Return ReflectionClassConstant objects
                let mut result = PhpArray::new();
                if method == b"getreflectionconstant" {
                    let const_name = args.get(1)?.to_php_string();
                    let mut val =
                        reflection_class_get_constant(vm, &class_lower, const_name.as_bytes());
                    if val.is_none() {
                        // Fall back to built-in class constants.
                        val = vm.get_builtin_class_constant(&class_lower, const_name.as_bytes());
                    }
                    if let Some(v) = val {
                        return Some(create_reflection_class_constant(
                            vm,
                            &target,
                            &const_name.to_msg_string(),
                            v,
                        ));
                    } else {
                        return Some(Value::False);
                    }
                }
                // getReflectionConstants(?int $filter)
                let filter = match args.get(1) {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(v.to_long()),
                };
                let entries: Vec<(Vec<u8>, Value, i64)> = rcls(vm, &class_lower)
                    .map(|ce| {
                        ce.constants
                            .iter()
                            .map(|(name, val)| {
                                let meta = ce.constants_meta.get(name);
                                let mut flags = match meta.map(|m| m.visibility) {
                                    Some(Visibility::Protected) => 2,
                                    Some(Visibility::Private) => 4,
                                    _ => 1,
                                };
                                if meta.is_some_and(|m| m.is_final) {
                                    flags |= 0x20;
                                }
                                (name.clone(), val.clone(), flags)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                for (name, val, flags) in entries {
                    if filter.is_some_and(|f| flags & f == 0) {
                        continue;
                    }
                    let const_name = crate::string::msg_str(&name).to_string();
                    result.push(create_reflection_class_constant(
                        vm,
                        &target,
                        &const_name,
                        val,
                    ));
                }
                Some(Value::Array(Rc::new(RefCell::new(result))))
            }
            b"hascase" => {
                let case_name = args.get(1)?.to_php_string();
                let has = rcls(vm, &class_lower)
                    .map(|ce| {
                        ce.enum_cases
                            .iter()
                            .any(|(cn, _)| cn == case_name.as_bytes())
                    })
                    .unwrap_or(false);
                Some(if has { Value::True } else { Value::False })
            }
            b"getcase" => {
                let case_name = args.get(1)?.to_php_string().to_msg_string();
                if let Some(ce) = rcls(vm, &class_lower) {
                    let is_case = ce
                        .enum_cases
                        .iter()
                        .any(|(cn, _)| cn == case_name.as_bytes());
                    if is_case {
                        let canonical = crate::string::msg_str(&ce.name).to_string();
                        let is_backed = ce.enum_backing_type.is_some();
                        let class_type = if is_backed {
                            "ReflectionEnumBackedCase"
                        } else {
                            "ReflectionEnumUnitCase"
                        };
                        let obj_id = vm.alloc_object_id();
                        let mut case_obj = PhpObject::new(class_type.as_bytes().to_vec(), obj_id);
                        case_obj.set_property(
                            b"name".to_vec(),
                            Value::String(PhpString::from_msg(&case_name.clone())),
                        );
                        case_obj.set_property(
                            b"class".to_vec(),
                            Value::String(PhpString::from_msg(&canonical)),
                        );
                        let case_val =
                            reflection_class_get_constant(vm, &class_lower, case_name.as_bytes());
                        if let Some(cv) = case_val {
                            case_obj.set_property(b"__reflection_value".to_vec(), cv);
                        }
                        Some(Value::Object(Rc::new(RefCell::new(case_obj))))
                    } else {
                        // Check if it's a constant but not a case
                        let is_const = ce.constants.contains_key(case_name.as_bytes());
                        let canonical = crate::string::msg_str(&ce.name).to_string();
                        if is_const {
                            let err_msg = format!("{}::{} is not a case", canonical, case_name);
                            let exc = vm.create_exception(
                                b"ReflectionException",
                                &err_msg,
                                vm.current_line,
                            );
                            vm.current_exception = Some(exc);
                        } else {
                            let err_msg =
                                format!("Case {}::{} does not exist", canonical, case_name);
                            let exc = vm.create_exception(
                                b"ReflectionException",
                                &err_msg,
                                vm.current_line,
                            );
                            vm.current_exception = Some(exc);
                        }
                        Some(Value::Null)
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"getattributes" => {
                let mut attrs = rcls(vm, &class_lower)
                    .map(|c| c.attributes.clone())
                    .unwrap_or_default();
                if attrs.is_empty() && class_lower == b"stdclass" {
                    attrs.push(crate::stubs::plain_internal_attribute(
                        b"AllowDynamicProperties",
                    ));
                }
                let filter_name = args.get(1).and_then(|v| {
                    if matches!(v, Value::Null) {
                        None
                    } else {
                        Some(v.to_php_string().as_bytes().to_vec())
                    }
                });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                let list = create_reflection_attributes(
                    vm,
                    &attrs,
                    filter_name.as_deref(),
                    filter_flags,
                    1,
                );
                // newInstance() runs the class validators of delayed attributes.
                if let Value::Array(a) = &list {
                    for (_, v) in a.borrow().iter() {
                        if let Value::Object(o) = v {
                            o.borrow_mut().set_property(
                                b"__reflection_attr_class".to_vec(),
                                Value::String(PhpString::from_vec(class_lower.clone())),
                            );
                        }
                    }
                }
                Some(list)
            }
            _ => None,
        }
    } else {
        None
    }
}

/// ReflectionMethod no-arg method dispatch
pub fn reflection_method_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    if obj.borrow().has_property(b"__reflection_invoke_internal") {
        if let Some(r) = internal_invoke_rm(vm, method, obj, None) {
            return Some(r);
        }
    }
    let ob = obj.borrow();
    let class_name = ob
        .get_property(b"__reflection_class")
        .to_php_string()
        .to_msg_string();
    let method_lower_val = ob.get_property(b"__reflection_method");
    let method_lower = method_lower_val.to_php_string();
    let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&class_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    // Optional closure back-reference, set when this ReflectionMethod came
    // from a Closure::__invoke lookup.
    let closure_fn: Option<Vec<u8>> = match ob.get_property(b"__reflection_closure_fn") {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    };
    drop(ob);

    // For Closure::__invoke: delegate param/return/doc queries to the
    // underlying user-defined function by rewriting the class/method keys
    // to look up in vm.user_functions.
    if let Some(ref fn_lower) = closure_fn {
        // (a closure's method of its scope class, not Closure::__invoke)
        let scoped = !class_lower.eq_ignore_ascii_case(b"closure");
        if scoped {
            match method {
                b"getdoccomment" => {
                    return Some(
                        rfunc(vm, fn_lower)
                            .and_then(|op| op.doc_comment.as_ref())
                            .map(|d| Value::String(PhpString::from_bytes(d)))
                            .unwrap_or(Value::False),
                    );
                }
                b"isstatic" => {
                    let st = rfunc(vm, fn_lower).is_some_and(|op| op.is_static_closure);
                    return Some(if st { Value::True } else { Value::False });
                }
                _ => {}
            }
        }
        let closure = obj.borrow().get_property(b"__reflection_closure_obj");
        if matches!(closure, Value::Object(_)) {
            match method {
                b"getclosurethis" => return Some(vm.closure_this(&closure).unwrap_or(Value::Null)),
                b"getclosurescopeclass" | b"getclosurecalledclass" => {
                    let scope = if method == b"getclosurecalledclass" {
                        vm.closure_called_class(&closure)
                    } else {
                        vm.closure_scope(&closure)
                    };
                    return Some(match scope {
                        Some(n) => create_reflection_class(vm, &crate::string::msg_str(&n)),
                        None => Value::Null,
                    });
                }
                _ => {}
            }
        }
        // Closure::fromCallable() of a __call()/__callStatic() trampoline:
        // its __invoke() has the trampoline's (empty) arginfo.
        if fn_lower.starts_with(b"__closure_fcc_tramp_")
            && !fn_lower.starts_with(b"__closure_fcc_tramp_ip_")
            && !fn_lower.starts_with(b"__closure_fcc_tramp_sp_")
        {
            match method {
                b"getnumberofparameters" | b"getnumberofrequiredparameters" => {
                    return Some(Value::Long(0));
                }
                b"getparameters" => {
                    return Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                }
                b"__tostring" => {
                    return Some(Value::String(PhpString::from_msg(
                        &"Method [ <internal> public method __invoke ] {\n}\n".to_string(),
                    )));
                }
                _ => {}
            }
        }
        match method {
            // (ZEND_ACC_DEPRECATED is kept from the closure's function)
            b"isdeprecated" => {
                let dep = match fn_lower.windows(2).position(|w| w == b"::") {
                    Some(p) => rmethod(vm, &fn_lower[..p], &fn_lower[p + 2..])
                        .map(|m| op_array_is_deprecated(&m.op_array, &m.attributes)),
                    None => None,
                }
                .or_else(|| {
                    rfunc(vm, fn_lower).map(|op| op_array_is_deprecated(op, &op.attributes))
                })
                .unwrap_or(false);
                return Some(if dep { Value::True } else { Value::False });
            }
            b"getnumberofparameters" => {
                let count = rfunc(vm, fn_lower.as_slice())
                    .map(|op| {
                        (op.param_count as usize).saturating_sub(
                            method_this_hidden(fn_lower, op) + closure_hidden_params(op),
                        ) as i64
                    })
                    .unwrap_or(0);
                return Some(Value::Long(count));
            }
            b"getnumberofrequiredparameters" => {
                let count = rfunc(vm, fn_lower.as_slice())
                    .map(|op| op.required_param_count as i64)
                    .unwrap_or(0);
                return Some(Value::Long(count));
            }
            b"getparameters" => {
                if let Some(op_array) = rfunc(vm, fn_lower.as_slice()).cloned() {
                    let params = if method_this_hidden(fn_lower, &op_array) > 0 {
                        create_reflection_parameters_method(vm, &op_array, false)
                    } else {
                        create_reflection_parameters(vm, &op_array)
                    };
                    // Closure::__invoke is an internal trampoline with
                    // userland arginfo: defaults are not retrievable.
                    if let (false, Value::Array(a)) = (scoped, &params) {
                        for (_, p) in a.borrow().iter() {
                            if let Value::Object(po) = p {
                                po.borrow_mut().set_property(
                                    b"__reflection_param_trampoline".to_vec(),
                                    Value::True,
                                );
                            }
                        }
                    }
                    return Some(params);
                }
                return Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
            }
            b"getreturntype" => {
                let rt = rfunc(vm, fn_lower.as_slice())
                    .and_then(|op| op.return_type.as_ref())
                    .cloned();
                return Some(match rt {
                    Some(rt) => create_reflection_type(vm, &rt),
                    None => Value::Null,
                });
            }
            b"hasreturntype" => {
                let has = rfunc(vm, fn_lower.as_slice())
                    .and_then(|op| op.return_type.as_ref())
                    .is_some();
                return Some(if has { Value::True } else { Value::False });
            }
            b"isstatic" => {
                return Some(Value::False);
            }
            b"isvariadic" => {
                let is_var = rfunc(vm, fn_lower.as_slice())
                    .and_then(|op| op.variadic_param)
                    .is_some();
                return Some(if is_var { Value::True } else { Value::False });
            }
            b"getfilename" => {
                let fname = vm
                    .user_functions
                    .get(fn_lower.as_slice())
                    .map(|op| crate::string::msg_str(&op.filename).to_string())
                    .filter(|f| !f.is_empty())
                    .unwrap_or_else(|| vm.current_file.to_string());
                return Some(Value::String(PhpString::from_msg(&fname)));
            }
            b"getstartline" => {
                let line = vm
                    .user_functions
                    .get(fn_lower.as_slice())
                    .map(|op| op.decl_line as i64)
                    .unwrap_or(0);
                return Some(Value::Long(line));
            }
            _ => {}
        }
    }

    match method {
        b"getname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        // Method names never carry a namespace.
        b"innamespace" => Some(Value::False),
        b"getnamespacename" => Some(Value::String(PhpString::from_bytes(b""))),
        b"getshortname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        b"isclosure" => Some(Value::False),
        b"getclosurethis" | b"getclosurescopeclass" | b"getclosurecalledclass"
            if closure_fn.is_none() =>
        {
            Some(Value::Null)
        }
        b"getclosureusedvariables" if closure_fn.is_none() => {
            Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
        }
        b"getextension" | b"getextensionname" => {
            let ext = rmethod(vm, &class_lower, method_lower.as_bytes())
                .filter(|m| m.op_array.internal_sig.is_some())
                .and_then(internal_method_ext);
            Some(match (method, ext) {
                (b"getextensionname", Some(e)) => {
                    Value::String(PhpString::from_msg(&e.to_string()))
                }
                (b"getextensionname", None) => Value::False,
                (_, Some(e)) => {
                    let obj_id = vm.alloc_object_id();
                    let mut ext_obj = PhpObject::new(b"ReflectionExtension".to_vec(), obj_id);
                    ext_obj.set_property(
                        b"name".to_vec(),
                        Value::String(PhpString::from_msg(&e.to_string())),
                    );
                    Value::Object(Rc::new(RefCell::new(ext_obj)))
                }
                (_, None) => Value::Null,
            })
        }
        b"getdeclaringclass" => {
            let class = rcls(vm, &class_lower)
                .and_then(|c| {
                    c.get_method(method_lower.as_bytes())
                        .map(|m| crate::string::msg_str(&m.declaring_class).to_string())
                })
                .unwrap_or(class_name.clone());
            let declaring_canonical = rcls(vm, class.as_bytes())
                .map(|c| crate::string::msg_str(&c.name).to_string())
                .unwrap_or(class.clone());
            Some(create_reflection_class(vm, &declaring_canonical))
        }
        b"ispublic" => {
            let vis = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| m.visibility)
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Public {
                Value::True
            } else {
                Value::False
            })
        }
        b"isprotected" => {
            let vis = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| m.visibility)
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Protected {
                Value::True
            } else {
                Value::False
            })
        }
        b"isprivate" => {
            let vis = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| m.visibility)
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Private {
                Value::True
            } else {
                Value::False
            })
        }
        b"isstatic" => {
            let is_static = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| m.is_static)
                .unwrap_or(false);
            Some(if is_static { Value::True } else { Value::False })
        }
        b"isabstract" => {
            let is_abstract = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| m.is_abstract)
                .unwrap_or(false);
            Some(if is_abstract {
                Value::True
            } else {
                Value::False
            })
        }
        b"isfinal" => {
            let is_final = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| m.is_final)
                .unwrap_or(false);
            Some(if is_final { Value::True } else { Value::False })
        }
        b"isconstructor" => Some(if method_lower.as_bytes() == b"__construct" {
            Value::True
        } else {
            Value::False
        }),
        b"isdestructor" => Some(if method_lower.as_bytes() == b"__destruct" {
            Value::True
        } else {
            Value::False
        }),
        b"getmodifiers" => {
            let mods = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(reflection_method_modifiers)
                .unwrap_or(1); // default public
            Some(Value::Long(mods))
        }
        b"getnumberofparameters" => {
            let method_def = rmethod(vm, &class_lower, method_lower.as_bytes());
            // (internal methods declared from PHP source: their arginfo)
            let count = method_def
                .map(|m| match m.op_array.internal_sig {
                    Some(sig) => sig.args.len() as u32,
                    None => {
                        let skip = if !m.is_static { 1u32 } else { 0u32 };
                        m.op_array.param_count.saturating_sub(skip)
                    }
                })
                .unwrap_or(0);
            Some(Value::Long(count as i64))
        }
        b"getnumberofrequiredparameters" => {
            let method_def = rmethod(vm, &class_lower, method_lower.as_bytes());
            // The compiler stores `required_param_count` as the count of
            // user-declared parameters without defaults — it does NOT include
            // the implicit `$this` slot. So we should NOT subtract for
            // instance methods.
            let count = method_def
                .map(|m| match m.op_array.internal_sig {
                    Some(sig) => sig.required_args() as u32,
                    None => m.op_array.required_param_count,
                })
                .unwrap_or(0);
            Some(Value::Long(count as i64))
        }
        b"getparameters" => {
            let method_data = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| (m.op_array.clone(), m.is_static));
            let params = if let Some((oa, is_static)) = method_data {
                create_reflection_parameters_method(vm, &oa, is_static)
            } else {
                Value::Array(Rc::new(RefCell::new(PhpArray::new())))
            };
            Some(params)
        }
        b"getreturntype" => {
            // Tentative return types of internal methods are not "real".
            let ret = rmethod(vm, &class_lower, method_lower.as_bytes())
                .filter(|m| {
                    !m.op_array
                        .internal_sig
                        .map(|f| f.flags & crate::stubs::F_TENTATIVE != 0)
                        .unwrap_or(false)
                })
                .and_then(reflect_return_type);
            // Resolve `self` to the declaring class of the method, falling
            // back to the lookup class if declaring info is unavailable.
            let scope: Vec<u8> = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| {
                    let decl_lower: Vec<u8> = m
                        .declaring_class
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();
                    rcls(vm, &decl_lower)
                        .map(|dc| dc.name.clone())
                        .unwrap_or_else(|| m.declaring_class.clone())
                })
                .or_else(|| rcls(vm, &class_lower).map(|c| c.name.clone()))
                .unwrap_or_else(|| class_lower.clone());
            if let Some(rt) = ret {
                Some(create_reflection_type_scoped(vm, &rt, Some(&scope)))
            } else {
                Some(Value::Null)
            }
        }
        b"hasreturntype" => {
            let has = rmethod(vm, &class_lower, method_lower.as_bytes())
                .filter(|m| {
                    !m.op_array
                        .internal_sig
                        .map(|f| f.flags & crate::stubs::F_TENTATIVE != 0)
                        .unwrap_or(false)
                })
                .and_then(reflect_return_type)
                .is_some();
            Some(if has { Value::True } else { Value::False })
        }
        b"isuserdefined" | b"isinternal" => {
            // Internal methods (also when inherited by a user class) carry
            // stub signatures; a Closure::__invoke trampoline is internal.
            let internal = match rmethod(vm, &class_lower, method_lower.as_bytes()) {
                Some(m) => m.op_array.internal_sig.is_some(),
                None => !vm.classes.contains_key(class_lower.as_slice()),
            };
            let r = if method == b"isinternal" {
                internal
            } else {
                !internal
            };
            Some(if r { Value::True } else { Value::False })
        }
        b"returnsreference" => {
            let by_ref = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| m.op_array.returns_by_ref)
                .unwrap_or(false);
            Some(if by_ref { Value::True } else { Value::False })
        }
        b"getfilename" => {
            // For methods on internal/built-in classes, return false.
            let is_user = vm.classes.contains_key(class_lower.as_slice())
                && !rmethod(vm, &class_lower, method_lower.as_bytes())
                    .is_some_and(|m| m.op_array.internal_sig.is_some());
            if !is_user {
                return Some(Value::False);
            }
            // The file the method was declared in (a trait's file for an
            // imported method), else the class's.
            let filename = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| crate::string::msg_str(&m.op_array.filename).into_owned())
                .filter(|f| !f.is_empty())
                .or_else(|| rcls(vm, &class_lower).and_then(|c| c.filename.clone()))
                .unwrap_or_else(|| vm.current_file.to_string());
            Some(Value::String(PhpString::from_msg(&filename)))
        }
        b"getstartline" => {
            let is_user = vm.classes.contains_key(class_lower.as_slice())
                && !rmethod(vm, &class_lower, method_lower.as_bytes())
                    .is_some_and(|m| m.op_array.internal_sig.is_some());
            if !is_user {
                return Some(Value::False);
            }
            let line = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| m.op_array.decl_line)
                .unwrap_or(0);
            Some(Value::Long(line as i64))
        }
        b"getendline" => {
            let is_user = vm.classes.contains_key(class_lower.as_slice())
                && !rmethod(vm, &class_lower, method_lower.as_bytes())
                    .is_some_and(|m| m.op_array.internal_sig.is_some());
            if !is_user {
                return Some(Value::False);
            }
            let line = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| {
                    if m.op_array.end_line > 0 {
                        m.op_array.end_line as i64
                    } else {
                        m.op_array.decl_line as i64
                    }
                })
                .unwrap_or(0);
            if line > 0 {
                Some(Value::Long(line))
            } else {
                Some(Value::False)
            }
        }
        b"getdoccomment" => {
            let doc = rmethod(vm, &class_lower, method_lower.as_bytes())
                .and_then(|m| m.doc_comment.as_ref())
                .cloned();
            if let Some(doc) = doc {
                Some(Value::String(PhpString::from_bytes(doc.as_bytes())))
            } else {
                Some(Value::False)
            }
        }
        b"isvariadic" => {
            let is_variadic = rmethod(vm, &class_lower, method_lower.as_bytes())
                .and_then(|m| m.op_array.variadic_param)
                .is_some();
            Some(if is_variadic {
                Value::True
            } else {
                Value::False
            })
        }
        b"isdeprecated" => {
            // The #[\Deprecated] attribute on the method (an internal one's
            // ZEND_ACC_DEPRECATED).
            let has_dep = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| op_array_is_deprecated(&m.op_array, &m.attributes))
                .unwrap_or(false);
            Some(if has_dep { Value::True } else { Value::False })
        }
        b"getstaticvariables" => {
            // Static variables on user methods are keyed by op_array.name (which
            // is `Class::method` for methods). Mirror reflection_function_method.
            let mut result = PhpArray::new();
            // Build the lookup prefix using the method's op_array name. We need
            // the op_array's name to match what `StaticVarInit` keyed against.
            let prefix_opt = rcls(vm, &class_lower)
                .and_then(|ce| ce.methods.get(method_lower.as_bytes()))
                .map(|m| {
                    let mut k = Vec::new();
                    if let Some(sc) = &m.op_array.scope_class {
                        if !m.op_array.name.starts_with(b"{closure") {
                            k.extend_from_slice(sc);
                            k.extend_from_slice(b"::");
                        }
                    }
                    k.extend_from_slice(&m.op_array.name);
                    k.extend_from_slice(b"::");
                    k
                });
            // Also extract declared static variable names in source order by
            // scanning the op_array for `StaticVarInit` opcodes.
            if let Some(prefix) = prefix_opt {
                if let Some(m) =
                    rcls(vm, &class_lower).and_then(|ce| ce.methods.get(method_lower.as_bytes()))
                {
                    use crate::opcode::{OpCode, OperandType};
                    let op_array = m.op_array.clone();
                    for op in op_array.ops.iter() {
                        if op.opcode == OpCode::StaticVarInit {
                            if let OperandType::Const(key_idx) = op.result {
                                if let Some(Value::String(key_s)) =
                                    op_array.literals.get(key_idx as usize)
                                {
                                    let key_bytes = key_s.as_bytes();
                                    if key_bytes.len() > prefix.len()
                                        && key_bytes.starts_with(&prefix)
                                    {
                                        let var_name = &key_bytes[prefix.len()..];
                                        // Get current value (or default)
                                        let val = if let Some(v) = vm.static_vars().get(key_bytes) {
                                            match v {
                                                Value::Reference(r) if Rc::strong_count(r) > 1 => {
                                                    Value::Reference(r.clone())
                                                }
                                                v => v.deref(),
                                            }
                                        } else {
                                            // No invocation yet -> use the default literal.
                                            if let OperandType::Const(d_idx) = op.op2 {
                                                op_array
                                                    .literals
                                                    .get(d_idx as usize)
                                                    .cloned()
                                                    .unwrap_or(Value::Null)
                                            } else {
                                                eval_static_const_init(vm, &op_array, op)
                                                    .unwrap_or(Value::Null)
                                            }
                                        };
                                        result.set(
                                            ArrayKey::String(PhpString::from_bytes(var_name)),
                                            val,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"hastentativereturntype" | b"gettentativereturntype" => {
            let rt = rmethod(vm, &class_lower, method_lower.as_bytes())
                .filter(|m| {
                    m.op_array
                        .internal_sig
                        .map(|f| f.flags & crate::stubs::F_TENTATIVE != 0)
                        .unwrap_or(false)
                })
                .and_then(reflect_return_type);
            Some(match (method, rt) {
                (b"hastentativereturntype", r) => {
                    if r.is_some() {
                        Value::True
                    } else {
                        Value::False
                    }
                }
                (_, Some(rt)) => create_reflection_type(vm, &rt),
                (_, None) => Value::Null,
            })
        }
        b"hasprototype" => {
            let lookup = match obj.borrow().get_property(b"__reflection_lookup_class") {
                Value::String(s) => s.as_bytes().to_ascii_lowercase(),
                _ => class_lower.clone(),
            };
            let proto = linked_method_prototype(vm, &lookup, method_lower.as_bytes(), 0);
            Some(if proto.is_some() {
                Value::True
            } else {
                Value::False
            })
        }
        b"isgenerator" => {
            let is_gen = rmethod(vm, &class_lower, method_lower.as_bytes())
                .map(|m| m.op_array.is_generator)
                .unwrap_or(false);
            Some(if is_gen { Value::True } else { Value::False })
        }
        b"getprototype" => {
            let method_lower_bytes = method_lower.as_bytes();
            // (the method as the class it was looked up in has it: an
            // inherited method may have been given a new prototype there)
            let lookup = match obj.borrow().get_property(b"__reflection_lookup_class") {
                Value::String(s) => s.as_bytes().to_ascii_lowercase(),
                _ => class_lower.clone(),
            };
            let proto_class =
                linked_method_prototype(vm, &lookup, method_lower_bytes, 0).map(|p| {
                    rcls(vm, &p)
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or_else(|| crate::string::msg_str(&p).to_string())
                });
            if let Some(proto) = proto_class {
                let method_name = rcls(vm, &class_lower)
                    .and_then(|c| {
                        c.get_method(method_lower_bytes)
                            .map(|m| crate::string::msg_str(&m.name).to_string())
                    })
                    .unwrap_or_default();
                Some(create_reflection_method(vm, &proto, &method_name))
            } else {
                // No prototype found - throw
                let ob2 = obj.borrow();
                let method_display = ob2.get_property(b"name").to_php_string().to_msg_string();
                drop(ob2);
                let shown = rcls(vm, &lookup)
                    .map(|c| crate::string::msg_str(&c.name).to_string())
                    .unwrap_or_else(|| class_name.to_string());
                let err_msg = format!(
                    "Method {}::{} does not have a prototype",
                    shown, method_display
                );
                let exc = vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                vm.current_exception = Some(exc);
                Some(Value::Null)
            }
        }
        b"__tostring" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_msg_string();
            let class = ob.get_property(b"class").to_php_string().to_msg_string();
            // If the method was looked up via ReflectionClass::getMethod on a
            // child, the lookup class is recorded here. Use it for the
            // inheritance check; otherwise fall back to __reflection_class.
            let effective_class_lower: Vec<u8> = match ob.get_property(b"__reflection_lookup_class")
            {
                Value::String(s) => s
                    .as_bytes()
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect(),
                _ => class_lower.clone(),
            };
            drop(ob);

            if let Some(ref fn_lower) = closure_fn {
                if let Some(op) = vm.user_functions.get(fn_lower.as_slice()).cloned() {
                    let hidden = closure_hidden_params(&op) + method_this_hidden(fn_lower, &op);
                    let m = crate::object::MethodDef {
                        name: b"__invoke".to_vec(),
                        param_count: op.param_count as usize,
                        op_array: op,
                        is_static: false,
                        is_abstract: false,
                        is_final: false,
                        visibility: Visibility::Public,
                        declaring_class: b"closure".to_vec(),
                        doc_comment: None,
                        attributes: Vec::new(),
                    };
                    let ctx = FnStrCtx {
                        op: &m.op_array,
                        method: Some(&m),
                        scope: None,
                        is_closure: false,
                        internal_fn_ext: None,
                        display_name: "__invoke",
                        filename: "",
                        internal_user_arginfo: true,
                        // (the closure's leading $this / use variables, a
                        // method's $this)
                        params_skip: Some(hidden),
                    };
                    return Some(Value::String(PhpString::from_msg(&function_string(
                        vm, &ctx, "",
                    ))));
                }
            }
            let method_def = rmethod(vm, &class_lower, method_lower.as_bytes()).cloned();

            if let Some(m) = method_def {
                let filename = if m.op_array.filename.is_empty() {
                    vm.current_file.to_string()
                } else {
                    crate::string::msg_str(&m.op_array.filename).into_owned()
                };
                // (a property hook is named `$prop::get` / `$prop::set`)
                let display = if m.name.starts_with(b"__property_") && !m.op_array.name.is_empty() {
                    crate::string::msg_str(&m.op_array.name).to_string()
                } else {
                    crate::string::msg_str(&m.name).to_string()
                };
                let ctx = FnStrCtx {
                    op: &m.op_array,
                    method: Some(&m),
                    scope: Some(&effective_class_lower),
                    is_closure: false,
                    internal_fn_ext: None,
                    display_name: &display,
                    filename: &filename,
                    internal_user_arginfo: false,
                    params_skip: None,
                };
                let _ = &name;
                Some(Value::String(PhpString::from_msg(&function_string(
                    vm, &ctx, "",
                ))))
            } else {
                Some(Value::String(PhpString::from_msg(&format!(
                    "Method [ {} {} ]",
                    class, name
                ))))
            }
        }
        _ => None,
    }
}

/// ReflectionMethod methods that need args
pub fn reflection_method_docall(vm: &mut Vm, method: &[u8], args: &[Value]) -> Option<Value> {
    match deprecated_method_call(vm, b"reflectionmethod", method, args) {
        Some(true) => return Some(Value::Null),
        // setAccessible(): no effect
        Some(false) => return Some(Value::Null),
        None => {}
    }
    let this = args.first()?;
    if let Value::Object(obj) = this {
        if obj.borrow().has_property(b"__reflection_invoke_internal") {
            if let Some(r) = internal_invoke_rm(vm, method, obj, Some(args)) {
                return Some(r);
            }
        }
        let ob = obj.borrow();
        let class_name = ob
            .get_property(b"__reflection_class")
            .to_php_string()
            .to_msg_string();
        let method_lower_val = ob.get_property(b"__reflection_method");
        let method_lower_bytes = method_lower_val.to_php_string();
        let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&class_name)
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        drop(ob);

        // Closure::__invoke on a closure object: call / return the closure.
        if class_lower == b"closure" && method_lower_bytes.as_bytes() == b"__invoke" {
            if let Some(target) = args.get(1).filter(|t| t.is_closure_obj()).cloned() {
                match method {
                    b"invoke" => {
                        let a: Vec<Value> = args.iter().skip(2).cloned().collect();
                        return Some(vm.call_callback(&target, &a).unwrap_or(Value::Null));
                    }
                    b"invokeargs" => {
                        let a: Vec<Value> = match args.get(2) {
                            Some(Value::Array(arr)) => arr.borrow().values().cloned().collect(),
                            _ => Vec::new(),
                        };
                        return Some(vm.call_callback(&target, &a).unwrap_or(Value::Null));
                    }
                    b"getclosure" => return Some(target.deref()),
                    _ => {}
                }
            }
        }

        match method {
            b"invoke" => {
                // invoke($object, ...$args)
                // Require at least 1 user argument ($object).
                if args.len() < 2 {
                    let err = "ReflectionMethod::invoke() expects at least 1 argument, 0 given"
                        .to_string();
                    let exc = vm.create_exception(b"ArgumentCountError", &err, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let target_obj = args.get(1)?.clone();
                let invoke_args: Vec<Value> = args.iter().skip(2).cloned().collect();
                // (extra named arguments, see spl_place_named_args())
                let invoke_named = std::mem::take(&mut vm.pending_named_args);

                // Block direct instantiation of internal classes via invoke($obj, __construct)
                if method_lower_bytes.as_bytes() == b"__construct" {
                    let no_direct: &[&[u8]] = &[
                        b"reflectionattribute",
                        b"reflectiongenerator",
                        b"reflectionfiber",
                        b"reflectionreference",
                    ];
                    if no_direct.contains(&class_lower.as_slice()) {
                        let canonical = match class_lower.as_slice() {
                            b"reflectionattribute" => "ReflectionAttribute",
                            b"reflectiongenerator" => "ReflectionGenerator",
                            b"reflectionfiber" => "ReflectionFiber",
                            b"reflectionreference" => "ReflectionReference",
                            _ => "ReflectionAttribute",
                        };
                        let msg = format!("Cannot directly instantiate {}", canonical);
                        let exc =
                            vm.create_exception(b"ReflectionException", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }

                let method_def = rmethod(vm, &class_lower, method_lower_bytes.as_bytes()).cloned();

                if let Some(m) = method_def {
                    // Abstract methods cannot be invoked. Check this before
                    // the ?object type validation to match PHP's ordering.
                    if m.is_abstract {
                        let canonical = rcls(vm, &class_lower)
                            .map(|c| crate::string::msg_str(&c.name).to_string())
                            .unwrap_or_else(|| crate::string::msg_str(&class_lower).to_string());
                        let method_disp = crate::string::msg_str(&m.name).to_string();
                        let msg = format!(
                            "Trying to invoke abstract method {}::{}()",
                            canonical, method_disp
                        );
                        let exc =
                            vm.create_exception(b"ReflectionException", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    // Type-check the first arg: must be ?object (null or object).
                    if !matches!(&target_obj, Value::Object(_) | Value::Null) {
                        let type_name = Vm::value_type_name(&target_obj);
                        let msg = format!(
                            "ReflectionMethod::invoke(): Argument #1 ($object) must be of type ?object, {} given",
                            type_name
                        );
                        let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    // Validate target_obj for non-static methods. PHP uses
                    // the method's declaring class for the type check, not
                    // the class the reflection was created on. This lets
                    // (new ReflectionMethod(B::class, 'method'))->invoke(A)
                    // work when B inherits the method from A.
                    if !m.is_static {
                        match &target_obj {
                            Value::Object(o) => {
                                let obj_class_lower: Vec<u8> = o
                                    .borrow()
                                    .class_name
                                    .iter()
                                    .map(|b| b.to_ascii_lowercase())
                                    .collect();
                                let decl_lower: Vec<u8> = m
                                    .declaring_class
                                    .iter()
                                    .map(|b| b.to_ascii_lowercase())
                                    .collect();
                                let target_lower = if !decl_lower.is_empty() {
                                    decl_lower
                                } else {
                                    class_lower.clone()
                                };
                                let ok = obj_class_lower == target_lower
                                    || vm.class_extends(&obj_class_lower, &target_lower);
                                if !ok {
                                    let msg = "Given object is not an instance of the class this method was declared in";
                                    let exc = vm.create_exception(
                                        b"ReflectionException",
                                        msg,
                                        vm.current_line,
                                    );
                                    vm.current_exception = Some(exc);
                                    return Some(Value::Null);
                                }
                            }
                            Value::Null => {
                                let canonical = rcls(vm, &class_lower)
                                    .map(|c| crate::string::msg_str(&c.name).to_string())
                                    .unwrap_or_else(|| {
                                        crate::string::msg_str(&class_lower).to_string()
                                    });
                                let method_disp = crate::string::msg_str(&m.name).to_string();
                                let msg = format!(
                                    "Trying to invoke non static method {}::{}() without an object",
                                    canonical, method_disp
                                );
                                let exc = vm.create_exception(
                                    b"ReflectionException",
                                    &msg,
                                    vm.current_line,
                                );
                                vm.current_exception = Some(exc);
                                return Some(Value::Null);
                            }
                            _ => {
                                let msg = "Given object is not an instance of the class this method was declared in";
                                let exc = vm.create_exception(
                                    b"ReflectionException",
                                    msg,
                                    vm.current_line,
                                );
                                vm.current_exception = Some(exc);
                                return Some(Value::Null);
                            }
                        }
                    }
                    if m.op_array.internal_sig.is_some() {
                        return Some(invoke_internal_method(
                            vm,
                            &m,
                            &class_lower,
                            target_obj,
                            &invoke_args,
                        ));
                    }
                    {
                        // Internal callers report "N passed and exactly M expected".
                        let req = m.op_array.required_param_count as usize;
                        if invoke_args.len() < req && invoke_named.is_empty() {
                            let canonical = rcls(vm, &m.declaring_class.to_ascii_lowercase())
                                .map(|c| crate::string::msg_str(&c.name).to_string())
                                .unwrap_or_else(|| {
                                    crate::string::msg_str(&class_lower).to_string()
                                });
                            let skip = if !m.is_static { 1usize } else { 0usize };
                            let total = (m.op_array.param_count as usize)
                                .saturating_sub(skip)
                                .saturating_sub(usize::from(m.op_array.variadic_param.is_some()));
                            let descriptor = if total == req { "exactly" } else { "at least" };
                            let msg = format!(
                                "Too few arguments to function {}::{}(), {} passed and {} {} expected",
                                canonical,
                                hook_method_display(&m.name),
                                invoke_args.len(),
                                descriptor,
                                req
                            );
                            // Raised inside the callee, called from internal code.
                            vm.call_stack.push((
                                format!("{}->{}", canonical, crate::string::msg_str(&m.name)),
                                "[internal function]".into(),
                                0,
                                invoke_args.clone(),
                                !m.is_static,
                            ));
                            let exc = vm.create_exception(
                                b"ArgumentCountError",
                                &msg,
                                m.op_array.decl_line,
                            );
                            vm.call_stack.pop();
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                        if let Value::Object(o) = &target_obj {
                            if m.name.starts_with(b"__property_") && invoke_named.is_empty() {
                                return Some(vm.invoke_property_hook(o, &m, &invoke_args));
                            }
                        }
                        let mut arr = PhpArray::new();
                        for a in &invoke_args {
                            arr.push(a.clone());
                        }
                        for (n, v) in invoke_named {
                            arr.set(ArrayKey::String(PhpString::from_vec(n)), v);
                        }
                        let called_lower = reflection_called_class(obj, &class_lower);
                        if let Some(r) = invoke_user_method(
                            vm,
                            &m,
                            if m.is_static {
                                &called_lower
                            } else {
                                &class_lower
                            },
                            &target_obj,
                            Value::Array(Rc::new(RefCell::new(arr))),
                        ) {
                            return Some(r);
                        }
                    }
                    let mut func_key = class_lower.clone();
                    func_key.extend_from_slice(b"::");
                    func_key.extend_from_slice(method_lower_bytes.as_bytes());
                    vm.user_functions
                        .insert(func_key.clone(), m.op_array.clone());

                    let mut cvs = vec![Value::Undef; m.op_array.cv_names.len()];
                    if !m.is_static {
                        if !cvs.is_empty() {
                            cvs[0] = target_obj;
                        }
                        for (i, arg) in invoke_args.iter().enumerate() {
                            if i + 1 < cvs.len() {
                                cvs[i + 1] = arg.clone();
                            }
                        }
                    } else {
                        for (i, arg) in invoke_args.iter().enumerate() {
                            if i < cvs.len() {
                                cvs[i] = arg.clone();
                            }
                        }
                    }

                    // Push class scope
                    // (the called scope is the class the ReflectionMethod
                    // was made for: static:: of an inherited method)
                    let called = match obj.borrow().get_property(b"__reflection_lookup_class") {
                        Value::String(l) => l.as_bytes().to_ascii_lowercase(),
                        _ => class_lower.clone(),
                    };
                    vm.called_class_stack.push(called);
                    vm.push_class_scope(class_lower.clone());

                    let result = vm
                        .execute_op_array_pub(&m.op_array, cvs)
                        .unwrap_or(Value::Null);

                    vm.called_class_stack.pop();
                    vm.pop_class_scope();

                    Some(result)
                } else {
                    Some(Value::Null)
                }
            }
            b"invokeargs" => {
                // reflection_method_invoke(): the abstract check comes
                // before argument parsing.
                if let Some(m) = rmethod(vm, &class_lower, method_lower_bytes.as_bytes())
                    .filter(|m| m.is_abstract)
                    .cloned()
                {
                    let canonical = rcls(vm, &class_lower)
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or_else(|| crate::string::msg_str(&class_lower).to_string());
                    let msg = format!(
                        "Trying to invoke abstract method {}::{}()",
                        canonical,
                        crate::string::msg_str(&m.name)
                    );
                    let exc = vm.create_exception(b"ReflectionException", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let target_obj = args.get(1)?.clone();
                if !matches!(&target_obj, Value::Object(_) | Value::Null) {
                    let msg = format!(
                        "ReflectionMethod::invokeArgs(): Argument #1 ($object) must be of type ?object, {} given",
                        Vm::value_type_name(&target_obj)
                    );
                    let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let args_arr_val = args
                    .get(2)
                    .cloned()
                    .unwrap_or(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                // Type-check: args must be array (PHP throws TypeError otherwise).
                if !matches!(&args_arr_val, Value::Array(_)) {
                    let type_name = Vm::value_type_name(&args_arr_val);
                    let msg = format!(
                        "ReflectionMethod::invokeArgs(): Argument #2 ($args) must be of type array, {} given",
                        type_name
                    );
                    let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let invoke_args: Vec<Value> = if let Value::Array(arr) = &args_arr_val {
                    arr.borrow().iter().map(|(_, v)| v.clone()).collect()
                } else {
                    vec![]
                };

                let method_def = rmethod(vm, &class_lower, method_lower_bytes.as_bytes()).cloned();

                if let Some(m) = method_def {
                    // Abstract methods cannot be invoked.
                    if m.is_abstract {
                        let canonical = rcls(vm, &class_lower)
                            .map(|c| crate::string::msg_str(&c.name).to_string())
                            .unwrap_or_else(|| crate::string::msg_str(&class_lower).to_string());
                        let method_disp = crate::string::msg_str(&m.name).to_string();
                        let msg = format!(
                            "Trying to invoke abstract method {}::{}()",
                            canonical, method_disp
                        );
                        let exc =
                            vm.create_exception(b"ReflectionException", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    // For non-static methods, validate that target_obj is an instance of the
                    // declaring class (or subclass).
                    if !m.is_static {
                        match &target_obj {
                            Value::Object(o) => {
                                let obj_class_lower: Vec<u8> = o
                                    .borrow()
                                    .class_name
                                    .iter()
                                    .map(|b| b.to_ascii_lowercase())
                                    .collect();
                                let ok = obj_class_lower == class_lower
                                    || vm.class_extends(&obj_class_lower, &class_lower);
                                if !ok {
                                    let msg = "Given object is not an instance of the class this method was declared in";
                                    let exc = vm.create_exception(
                                        b"ReflectionException",
                                        msg,
                                        vm.current_line,
                                    );
                                    vm.current_exception = Some(exc);
                                    return Some(Value::Null);
                                }
                            }
                            _ => {
                                let msg = "Given object is not an instance of the class this method was declared in";
                                let exc = vm.create_exception(
                                    b"ReflectionException",
                                    msg,
                                    vm.current_line,
                                );
                                vm.current_exception = Some(exc);
                                return Some(Value::Null);
                            }
                        }
                    }
                    if m.op_array.internal_sig.is_some() {
                        return Some(invoke_internal_method(
                            vm,
                            &m,
                            &class_lower,
                            target_obj,
                            &invoke_args,
                        ));
                    }
                    // Check that enough args were provided (ArgumentCountError).
                    // `required_param_count` is the user-declared count
                    // (no $this), so it works for both static and non-static.
                    let req = m.op_array.required_param_count as usize;
                    if invoke_args.len() < req {
                        let canonical = rcls(vm, &class_lower)
                            .map(|c| crate::string::msg_str(&c.name).to_string())
                            .unwrap_or_else(|| crate::string::msg_str(&class_lower).to_string());
                        let method_disp = crate::string::msg_str(&m.name).to_string();
                        // Determine `exactly` vs `at least`.
                        let skip = if !m.is_static { 1usize } else { 0usize };
                        let total = (m.op_array.param_count as usize)
                            .saturating_sub(skip)
                            .saturating_sub(usize::from(m.op_array.variadic_param.is_some()));
                        let descriptor = if total == req { "exactly" } else { "at least" };
                        let plural = if req == 1 { "" } else { "s" };
                        let msg = format!(
                            "Too few arguments to function {}::{}(), {} passed and {} {} expected",
                            canonical,
                            method_disp,
                            invoke_args.len(),
                            descriptor,
                            req
                        );
                        let _ = plural;
                        let line = m.op_array.decl_line;
                        vm.call_stack.push((
                            format!("{}->{}", canonical, method_disp),
                            "[internal function]".into(),
                            0,
                            invoke_args.clone(),
                            !m.is_static,
                        ));
                        let exc = vm.create_exception(b"ArgumentCountError", &msg, line);
                        vm.call_stack.pop();
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    if let Some(r) = invoke_user_method(
                        vm,
                        &m,
                        &if m.is_static {
                            reflection_called_class(obj, &class_lower)
                        } else {
                            class_lower.clone()
                        },
                        &target_obj,
                        args_arr_val.clone(),
                    ) {
                        return Some(r);
                    }
                    let mut cvs = vec![Value::Undef; m.op_array.cv_names.len()];
                    if !m.is_static {
                        if !cvs.is_empty() {
                            cvs[0] = target_obj;
                        }
                        for (i, arg) in invoke_args.iter().enumerate() {
                            if i + 1 < cvs.len() {
                                cvs[i + 1] = arg.clone();
                            }
                        }
                    } else {
                        for (i, arg) in invoke_args.iter().enumerate() {
                            if i < cvs.len() {
                                cvs[i] = arg.clone();
                            }
                        }
                    }

                    // (the called scope is the class the ReflectionMethod
                    // was made for: static:: of an inherited method)
                    let called = match obj.borrow().get_property(b"__reflection_lookup_class") {
                        Value::String(l) => l.as_bytes().to_ascii_lowercase(),
                        _ => class_lower.clone(),
                    };
                    vm.called_class_stack.push(called);
                    vm.push_class_scope(class_lower.clone());

                    let result = vm
                        .execute_op_array_pub(&m.op_array, cvs)
                        .unwrap_or(Value::Null);

                    vm.called_class_stack.pop();
                    vm.pop_class_scope();

                    Some(result)
                } else {
                    Some(Value::Null)
                }
            }
            b"getattributes" => {
                let closure_fn = match obj.borrow().get_property(b"__reflection_closure_fn") {
                    Value::String(s) => Some(s.as_bytes().to_vec()),
                    _ => None,
                };
                // (a closure's Closure::__invoke carries its function's
                // attributes: zend_get_closure_invoke_method copies common)
                let (mut attrs, decl) = match closure_fn {
                    Some(f) => closure_fn_attributes(vm, &f),
                    None => rcls(vm, &class_lower)
                        .and_then(|c| {
                            c.get_method(method_lower_bytes.as_bytes())
                                .map(|m| (m.attributes.clone(), Some(m.declaring_class.clone())))
                        })
                        .unwrap_or_default(),
                };
                // (an internal method: its stub's #[\Deprecated])
                if attrs.is_empty() {
                    if let Some(d) = decl
                        .as_ref()
                        .filter(|d| rcls(vm, d).is_none_or(|c| c.filename.is_none()))
                    {
                        let key = format!(
                            "{}::{}",
                            crate::string::msg_str(d).to_ascii_lowercase(),
                            method_lower_bytes.to_msg_string().to_ascii_lowercase()
                        );
                        attrs.extend(stub_deprecated_attr(1, &key));
                    }
                }
                let filter_name = args.get(1).and_then(|v| {
                    if matches!(v, Value::Null) {
                        None
                    } else {
                        Some(v.to_php_string().as_bytes().to_vec())
                    }
                });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                let list = create_reflection_attributes(
                    vm,
                    &attrs,
                    filter_name.as_deref(),
                    filter_flags,
                    4,
                );
                set_attr_scope(&list, decl, b"__reflection_attr_scope");
                Some(list)
            }
            b"getclosure" => {
                if opt_object_arg_error(vm, "ReflectionMethod::getClosure", args.get(1)) {
                    return Some(Value::Null);
                }
                // Build a Closure value referring to this method. In this VM,
                // closures are represented as strings ("Class::method") or
                // arrays ([callable, $this]). For static methods return the
                // callable name; for instance methods bind to the given $obj.
                let is_static = rcls(vm, &class_lower)
                    .and_then(|c| c.get_method(method_lower_bytes.as_bytes()))
                    .map(|m| m.is_static)
                    .unwrap_or(false);
                // Canonicalize name and class for display
                let method_name = rcls(vm, &class_lower)
                    .and_then(|c| {
                        c.get_method(method_lower_bytes.as_bytes())
                            .map(|m| crate::string::msg_str(&m.name).to_string())
                    })
                    .unwrap_or_else(|| method_lower_bytes.to_msg_string());
                let class_canonical = rcls(vm, &class_lower)
                    .map(|c| crate::string::msg_str(&c.name).to_string())
                    .unwrap_or(class_name.clone());
                let callable = format!("{}::{}", class_canonical, method_name);
                if is_static {
                    Some(vm.make_closure_object(Value::String(PhpString::from_msg(&callable))))
                } else {
                    let target_obj = args.get(1).cloned().unwrap_or(Value::Null);
                    if matches!(target_obj, Value::Null | Value::Undef) {
                        let exc = vm.create_exception(b"ValueError", "ReflectionMethod::getClosure(): Argument #1 ($object) cannot be null for non-static methods", vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    let decl = rmethod(vm, &class_lower, method_lower_bytes.as_bytes())
                        .map(|m| m.declaring_class.to_ascii_lowercase())
                        .unwrap_or_else(|| class_lower.clone());
                    if let Value::Object(o) = &target_obj {
                        let ocl = o.borrow().class_name.to_ascii_lowercase();
                        if ocl != decl && !vm.class_extends(&ocl, &decl) {
                            let exc = vm.create_exception(b"ReflectionException", "Given object is not an instance of the class this method was declared in", vm.current_line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                    }
                    // An internal method: a closure over it, `[$obj, 'm']`
                    // (unless the object's class overrides it).
                    if let Value::Object(o) = &target_obj {
                        let ocl = o.borrow().class_name.to_ascii_lowercase();
                        let ml = method_lower_bytes.as_bytes();
                        if let (Some(_), Some((_, sm))) = (
                            vm.internal_method_of(&class_lower, ml),
                            vm.internal_method_of(&ocl, ml)
                                .filter(|(_, m)| m.visibility() == Visibility::Public),
                        ) {
                            let mut arr = PhpArray::new();
                            arr.push(target_obj.clone());
                            arr.push(Value::String(PhpString::from_bytes(sm.name.as_bytes())));
                            return Some(
                                vm.make_closure_object(Value::Array(Rc::new(RefCell::new(arr)))),
                            );
                        }
                    }
                    let key: Vec<u8> =
                        crate::errdisplay::raw_error_bytes(&callable).to_ascii_lowercase();
                    if !vm.user_functions.contains_key(&key) {
                        let op = vm
                            .classes
                            .get(&class_lower)
                            .and_then(|c| c.get_method(method_lower_bytes.as_bytes()))
                            .map(|m| {
                                let mut op = m.op_array.clone();
                                if op.attributes.is_empty() && !m.attributes.is_empty() {
                                    std::rc::Rc::make_mut(&mut op).attributes =
                                        m.attributes.clone();
                                }
                                op
                            });
                        if let Some(op) = op {
                            vm.user_functions.insert(key, op);
                        }
                    }
                    let mut arr = PhpArray::new();
                    arr.push(Value::String(PhpString::from_msg(&callable)));
                    arr.push(target_obj);
                    Some(vm.make_closure_object(Value::Array(Rc::new(RefCell::new(arr)))))
                }
            }
            _ => None,
        }
    } else {
        None
    }
}

/// Invoke a user method the way ReflectionMethod::invoke() does: a normal
/// call (argument checks, coercion, frames) made from the declaring class's
/// scope, with the reflected class as the called scope of static calls.
/// None when the class can't be named in source (anonymous classes).
/// A method's name as PHP shows it: a property hook's is `$prop::get` /
/// `$prop::set`.
fn hook_method_display(name: &[u8]) -> String {
    if let Some(p) = name.strip_prefix(b"__property_get_") {
        format!("${}::get", crate::string::msg_str(p))
    } else if let Some(p) = name.strip_prefix(b"__property_set_") {
        format!("${}::set", crate::string::msg_str(p))
    } else {
        crate::string::msg_str(name).to_string()
    }
}

/// The class a ReflectionMethod was made for (lowercase): static:: of the
/// inherited static method it invokes.
fn reflection_called_class(obj: &Rc<RefCell<PhpObject>>, class_lower: &[u8]) -> Vec<u8> {
    match obj.borrow().get_property(b"__reflection_lookup_class") {
        Value::String(l) => l.as_bytes().to_ascii_lowercase(),
        _ => class_lower.to_vec(),
    }
}

fn invoke_user_method(
    vm: &mut Vm,
    m: &crate::object::MethodDef,
    class_lower: &[u8],
    target_obj: &Value,
    args: Value,
) -> Option<Value> {
    let cname = rcls(vm, class_lower).map(|c| crate::string::msg_str(&c.name).to_string())?;
    let simple = |n: &str| {
        !n.is_empty()
            && n.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '\\' || (c as u32) >= 0x80)
    };
    if !simple(&cname) {
        return None;
    }
    let decl = m.declaring_class.to_ascii_lowercase();
    // invoke() calls this exact implementation; a virtual call would pick an
    // override in the object's class.
    if let Value::Object(o) = target_obj {
        if !m.is_static {
            let ocl = o.borrow().class_name.to_ascii_lowercase();
            match rmethod(vm, &ocl, &m.name) {
                Some(om) if om.declaring_class.eq_ignore_ascii_case(&decl) => {}
                _ => return None,
            }
        }
    }
    let raw_name = crate::errdisplay::raw_error_bytes(&cname);
    // (a name that is not UTF-8 cannot be spelled in the snippet's source)
    let by_var = std::str::from_utf8(&raw_name).is_err();
    let src = if by_var && (m.is_static || !matches!(target_obj, Value::Object(_))) {
        "return $__c::$__m(...$__a);".to_string()
    } else if m.is_static || !matches!(target_obj, Value::Object(_)) {
        format!(
            "return \\{}::$__m(...$__a);",
            cname.trim_start_matches('\\')
        )
    } else {
        "return $__o->$__m(...$__a);".to_string()
    };
    let r = run_snippet_in_scope_ex(
        vm,
        &src,
        &[
            ("__o", target_obj.clone()),
            ("__m", Value::String(PhpString::from_vec(m.name.clone()))),
            ("__a", args),
            (
                "__c",
                Value::String(PhpString::from_vec(raw_name.into_owned())),
            ),
        ],
        &decl,
        true,
    );
    Some(r.unwrap_or(Value::Null))
}

/// Invoke an internal (natively implemented) method through the VM's
/// regular callable dispatch.
fn invoke_internal_method(
    vm: &mut Vm,
    m: &crate::object::MethodDef,
    class_lower: &[u8],
    target_obj: Value,
    args: &[Value],
) -> Value {
    let callable = if m.is_static || !matches!(target_obj, Value::Object(_)) {
        let cname = rcls(vm, class_lower)
            .map(|c| crate::string::msg_str(&c.name).to_string())
            .unwrap_or_else(|| crate::string::msg_str(class_lower).to_string());
        Value::String(PhpString::from_msg(&format!(
            "{}::{}",
            cname,
            crate::string::msg_str(&m.name)
        )))
    } else {
        let mut arr = PhpArray::new();
        arr.push(target_obj.clone());
        arr.push(Value::String(PhpString::from_vec(m.name.clone())));
        Value::Array(Rc::new(RefCell::new(arr)))
    };
    // (ReflectionMethod::invoke() calls methods whatever their visibility)
    let prev = INVOKE_BYPASS.with(|b| b.replace(true));
    // The reflected internal method itself runs, not an override of it in
    // the object's (user) class.
    if let Value::Object(o) = &target_obj {
        if !m.is_static {
            if let Some(base) = overridden_internal_base(vm, o, &m.name.to_ascii_lowercase()) {
                let ov = target_obj.clone();
                let r =
                    vm.call_internal_method_of(o, &ov, &base, &m.name.to_ascii_lowercase(), args);
                if let Some(v) = r {
                    INVOKE_BYPASS.with(|b| b.set(prev));
                    return v;
                }
            }
        }
    }
    let r = vm.call_callback(&callable, args);
    INVOKE_BYPASS.with(|b| b.set(prev));
    match r {
        Ok(v) => v,
        Err(e) => {
            if vm.current_exception.is_none() {
                let exc = vm.create_exception(b"Error", &e.message, vm.current_line);
                vm.current_exception = Some(exc);
            }
            Value::Null
        }
    }
}

/// The internal class an object's user class overrides the internal
/// method `method_lower` of: the nearest internal ancestor.
#[inline(never)]
fn overridden_internal_base(
    vm: &Vm,
    obj: &Rc<RefCell<PhpObject>>,
    method_lower: &[u8],
) -> Option<Vec<u8>> {
    let mut cur = obj.borrow().class_name.to_ascii_lowercase();
    let def = vm.classes.get(&cur)?.get_method(method_lower)?;
    if def.op_array.internal_sig.is_some() || vm.is_known_builtin_class(&cur) {
        return None;
    }
    for _ in 0..64 {
        if vm.is_known_builtin_class(&cur) {
            return Some(cur);
        }
        cur = vm.classes.get(&cur)?.parent.as_ref()?.to_ascii_lowercase();
    }
    None
}

thread_local! {
    static INVOKE_BYPASS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// An internal method is being run by ReflectionMethod::invoke(), which
/// does not check its visibility (natively implemented private methods
/// then run their body).
pub fn invoke_bypasses_visibility() -> bool {
    INVOKE_BYPASS.with(|b| b.get())
}

thread_local! {
    static INTERNAL_FN_CACHE: RefCell<std::collections::HashMap<Vec<u8>, Option<&'static OpArray>>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Reflection's view of a function: the user function, or for an internal
/// function a signature-only `OpArray` synthesized from its stub.
/// 1 when `lower` names a method ("class::method") whose op array has
/// `$this` as its first CV (an instance method), else 0.
fn method_this_hidden(lower: &[u8], op: &OpArray) -> usize {
    (lower.windows(2).any(|w| w == b"::")
        && op.scope_class.is_some()
        && op.param_count > 0
        && op.cv_names.first().is_some_and(|n| n == b"this")) as usize
}

/// Whether "Class::method" names a static method.
#[inline(never)]
fn static_method_named(vm: &Vm, name: &[u8]) -> bool {
    let Some(p) = name.windows(2).position(|w| w == b"::") else {
        return false;
    };
    let class = name[..p]
        .strip_prefix(b"\\")
        .unwrap_or(&name[..p])
        .to_ascii_lowercase();
    let method = name[p + 2..].to_ascii_lowercase();
    rmethod(vm, &class, &method).is_some_and(|m| m.is_static)
}

pub(crate) fn rfunc<'a>(vm: &'a Vm, lower: &[u8]) -> Option<&'a OpArray> {
    if let Some(op) = vm.user_functions.get(lower) {
        return Some(op);
    }
    if !vm.functions.contains_key(lower) {
        // "class::method" (the __call/__callStatic behind a trampoline).
        let p = lower.windows(2).position(|w| w == b"::")?;
        return vm
            .classes
            .get(&lower[..p])
            .and_then(|c| c.methods.get(&lower[p + 2..]))
            .map(|m| &*m.op_array);
    }
    if let Some(v) = INTERNAL_FN_CACHE.with(|c| c.borrow().get(lower).copied()) {
        return v;
    }
    let built: Option<&'static OpArray> = crate::stubs::find_function(lower).map(|f| {
        let mut oa = crate::stubs::op_array_for(f, None, false);
        // What the stub declares beyond arginfo (attributes, an exposed doc
        // comment), from the extension's own declaration.
        if let Some(d) = vm.internal_function_decls.get(lower) {
            oa.attributes = d.attributes.clone();
            oa.param_attributes = d.param_attributes.clone();
            oa.doc_comment = d.doc_comment.clone();
        }
        if oa.attributes.is_empty() {
            oa.attributes
                .extend(stub_deprecated_attr(0, &f.name.to_ascii_lowercase()));
        }
        &*Box::leak(Box::new(oa))
    });
    // A builtin without a stub (registered from C code in php-src): its
    // declaration describes it.
    let built = built.or_else(|| {
        vm.internal_function_decls
            .get(lower)
            .map(|d| &*Box::leak(Box::new((**d).clone())))
    });
    INTERNAL_FN_CACHE.with(|c| c.borrow_mut().insert(lower.to_vec(), built));
    built
}

#[path = "stub_deprecations.rs"]
mod stub_deprecations;

/// The #[\Deprecated] attribute of an internal declaration, from its stub
/// (kind and key: see stub_deprecations.rs).
fn stub_deprecated_attr(kind: u8, key: &str) -> Option<crate::object::RuntimeAttribute> {
    let (_, _, args) = stub_deprecations::DEPRECATED
        .iter()
        .find(|(k, n, _)| *k == kind && *n == key)?;
    Some(deprecated_attr_with(args))
}

/// A #[\Deprecated] attribute with these named arguments.
fn deprecated_attr_with(args: &[(&str, &str)]) -> crate::object::RuntimeAttribute {
    let lit = |v: &str| format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"));
    let src = format!(
        "<?php return [{}];",
        args.iter()
            .map(|(k, v)| format!("{} => {}", lit(k), lit(v)))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let source = src.into_bytes();
    let tokens = goro_parser::Lexer::new(&source).tokenize();
    let args_op_array = goro_parser::Parser::new(tokens)
        .parse()
        .ok()
        .and_then(|p| crate::compiler::Compiler::new().compile(&p).ok())
        .map(|(oa, _, _)| oa)
        .unwrap_or_default();
    crate::object::RuntimeAttribute {
        name: b"Deprecated".to_vec(),
        args_op_array: std::rc::Rc::new(args_op_array),
        validation_error: None,
    }
}

/// The #[\Deprecated] of an internal class constant (its stub's, or the
/// one an extension registers it with), added to its metadata.
fn internal_const_deprecated_attr(
    vm: &Vm,
    const_name: &[u8],
    meta: &mut Option<crate::object::ConstantMeta>,
) {
    let Some(m) = meta.as_mut() else {
        return;
    };
    if !m.attributes.is_empty()
        || rcls(vm, &m.declaring_class).is_some_and(|c| c.filename.is_some())
    {
        return;
    }
    let key = format!(
        "{}::{}",
        crate::string::msg_str(&m.declaring_class),
        crate::string::msg_str(const_name)
    );
    if let Some(a) = stub_deprecated_attr(3, &key) {
        m.attributes.push(a);
    } else if let Some(msg) = vm.ext_class_constant_deprecation(&m.declaring_class, const_name) {
        // "Constant C::K is deprecated[ since V][, message]"
        let rest = msg.split_once(" is deprecated").map_or("", |(_, r)| r);
        let (since, message) = match rest.strip_prefix(" since ") {
            Some(r) => match r.split_once(", ") {
                Some((v, m)) => (Some(v), Some(m)),
                None => (Some(r), None),
            },
            None => (None, rest.strip_prefix(", ")),
        };
        let mut args = Vec::new();
        if let Some(v) = since {
            args.push(("since", v));
        }
        if let Some(v) = message {
            args.push(("message", v));
        }
        m.attributes.push(deprecated_attr_with(&args));
    }
}

/// Extension of an internal function, from its stub.
fn internal_function_ext(vm: &Vm, lower: &[u8]) -> Option<&'static str> {
    if vm.user_functions.contains_key(lower) || !vm.functions.contains_key(lower) {
        return None;
    }
    crate::stubs::find_function(lower).map(|f| f.ext)
}

/// For an internal parameter default expression, the constant name it
/// refers to (Zend: ZEND_AST_CONSTANT / ZEND_AST_CLASS_CONST), if any.
fn internal_default_constant_name(def: &str) -> Option<String> {
    let d = def.trim();
    let is_ident = |x: &str| {
        !x.is_empty()
            && x.chars()
                .next()
                .map(|c| c.is_ascii_alphabetic() || c == '_' || c == '\\')
                .unwrap_or(false)
            && x.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '\\')
    };
    if let Some((cls, name)) = d.split_once("::") {
        if is_ident(cls) && is_ident(name) && !name.eq_ignore_ascii_case("class") {
            return Some(format!("{}::{}", cls.trim_start_matches('\\'), name));
        }
        return None;
    }
    if is_ident(d) && !matches!(d.to_ascii_lowercase().as_str(), "true" | "false" | "null") {
        return Some(d.trim_start_matches('\\').to_string());
    }
    None
}

/// Evaluate an internal parameter's default value expression. Names in it
/// resolve in the namespace of the declaring class `scope` (stubs write
/// `IntervalBoundary::ClosedOpen` inside `namespace Random`).
fn eval_internal_default(vm: &mut Vm, def: &str, scope: Option<&[u8]>) -> Option<Value> {
    if let Some(v) = crate::stubs::literal_value(def) {
        return Some(v);
    }
    let ns = scope
        .and_then(|s| s.iter().rposition(|&b| b == b'\\').map(|p| &s[..p]))
        .map(|n| format!("namespace {};", crate::string::msg_str(n)))
        .unwrap_or_default();
    let source = format!("<?php {} return ({});", ns, def).into_bytes();
    let mut lexer = goro_parser::Lexer::new(&source);
    let tokens = lexer.tokenize();
    let mut parser = goro_parser::Parser::new(tokens);
    let program = parser.parse().ok()?;
    let compiler = crate::compiler::Compiler::new();
    let (mut op_array, _classes, _warnings) = compiler.compile(&program).ok()?;
    // (evaluated where getDefaultValue() is called: its diagnostics too)
    let line = vm.current_line;
    if line > 0 {
        for op in &mut op_array.ops {
            op.line = line;
        }
        op_array.filename = vm.current_file.as_bytes().to_vec();
    }
    let cvs = vec![Value::Undef; op_array.cv_names.len()];
    vm.execute_op_array_pub(&op_array, cvs).ok()
}

/// ReflectionReference::fromArrayElement(array $array, int|string $key): ?ReflectionReference
fn reflection_reference_from_array_element(vm: &mut Vm, args: &[Value], line: u32) -> Value {
    let arr = match args.first().map(|v| v.deref()) {
        Some(Value::Array(a)) => a,
        Some(other) => {
            let msg = format!(
                "ReflectionReference::fromArrayElement(): Argument #1 ($array) must be of type array, {} given",
                Vm::value_type_name(&other)
            );
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Value::Null;
        }
        None => return Value::Null,
    };
    let key = match args.get(1).map(|v| v.deref()) {
        Some(Value::Long(n)) => ArrayKey::Int(n),
        Some(Value::String(s)) => match std::str::from_utf8(s.as_bytes())
            .ok()
            .and_then(|t| t.parse::<i64>().ok())
        {
            Some(n) if n.to_string().as_bytes() == s.as_bytes() => ArrayKey::Int(n),
            _ => ArrayKey::String(s),
        },
        Some(other) => {
            let msg = format!(
                "ReflectionReference::fromArrayElement(): Argument #2 ($key) must be of type string|int, {} given",
                Vm::value_type_name(&other)
            );
            let exc = vm.create_exception(b"TypeError", &msg, line);
            vm.current_exception = Some(exc);
            return Value::Null;
        }
        None => return Value::Null,
    };
    let item = arr.borrow().get(&key).cloned();
    let item = match item {
        Some(v) => v,
        None => {
            let exc = vm.create_exception(b"ReflectionException", "Array key not found", line);
            vm.current_exception = Some(exc);
            return Value::Null;
        }
    };
    let rc = match item {
        Value::Reference(rc) => rc,
        _ => return Value::Null,
    };
    // A reference with a refcount of 1 is not a reference anymore, except
    // for a directly self-referential array (bug #78263).
    let self_ref = match &*rc.borrow() {
        Value::Array(inner) => Rc::ptr_eq(inner, &arr),
        _ => false,
    };
    // (`rc` itself is one more holder than the array slot.)
    if Rc::strong_count(&rc) <= 2 && !self_ref && !crate::vm::typed_ref::is_typed(&rc) {
        return Value::Null;
    }
    let id = Rc::as_ptr(&rc) as usize as u64;
    let obj_id = vm.alloc_object_id();
    let mut obj = PhpObject::new(b"ReflectionReference".to_vec(), obj_id);
    obj.set_property(b"__reflection_ref_id".to_vec(), Value::Long(id as i64));
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// ReflectionReference instance methods.
pub fn reflection_reference_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    match method {
        b"__construct" => {
            let scope = vm.current_class_scope().map(|c| {
                let cl = c.to_ascii_lowercase();
                rcls(vm, &cl)
                    .map(|ce| crate::string::msg_str(&ce.name).to_string())
                    .unwrap_or_else(|| crate::string::msg_str(&c).to_string())
            });
            let msg = match scope {
                Some(sc) => format!(
                    "Call to private ReflectionReference::__construct() from scope {}",
                    sc
                ),
                None => "Call to private ReflectionReference::__construct() from global scope"
                    .to_string(),
            };
            let exc = vm.create_exception(b"Error", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            Some(Value::Null)
        }
        b"getid" => {
            // PHP returns an opaque 20-byte binary id (a SHA1 of the
            // reference address and a per-request key).
            let id = obj.borrow().get_property(b"__reflection_ref_id").to_long() as u64;
            let key: u64 = REF_ID_KEY.with(|k| *k);
            let mut out = Vec::with_capacity(20);
            let mut x = id ^ key;
            for i in 0..20u64 {
                x = x
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407 ^ i);
                out.push((x >> 56) as u8);
            }
            Some(Value::String(PhpString::from_vec(out)))
        }
        _ => None,
    }
}

thread_local! {
    static REF_ID_KEY: u64 = {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
        t ^ 0x9E37_79B9_7F4A_7C15
    };
}

/// ReflectionFunction no-arg method dispatch
pub fn reflection_function_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    if obj.borrow().has_property(b"__reflection_internal_method") {
        if let Some(r) = internal_method_rf(vm, method, obj, None) {
            return Some(r);
        }
    }
    let ob = obj.borrow();
    let target = ob.get_property(b"__reflection_target").to_php_string();
    let func_lower = target.as_bytes().to_vec();
    drop(ob);
    if func_lower.starts_with(b"__closure_fcc_tramp_") {
        if let Some(r) = trampoline_function_method(vm, method, &func_lower) {
            return Some(r);
        }
    }
    // Closure::fromCallable() of a __call()/__callStatic() trampoline: the
    // fake closure's function has no arginfo.
    if func_lower.starts_with(b"__closure_fcc_tramp_")
        && !func_lower.starts_with(b"__closure_fcc_tramp_ip_")
        && !func_lower.starts_with(b"__closure_fcc_tramp_sp_")
    {
        match method {
            b"getparameters" => return Some(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
            b"getnumberofparameters" | b"getnumberofrequiredparameters" => {
                return Some(Value::Long(0));
            }
            b"isvariadic" => return Some(Value::False),
            _ => {}
        }
    }

    match method {
        b"getname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        b"isinternal" => {
            let is_internal = vm.functions.contains_key(func_lower.as_slice());
            Some(if is_internal {
                Value::True
            } else {
                Value::False
            })
        }
        b"isuserdefined" => {
            let is_user = vm.user_functions.contains_key(func_lower.as_slice());
            Some(if is_user { Value::True } else { Value::False })
        }
        b"getfilename" => {
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()) {
                if !op_array.filename.is_empty() {
                    Some(Value::String(PhpString::from_vec(
                        op_array.filename.clone(),
                    )))
                } else {
                    Some(Value::String(PhpString::from_msg(
                        &vm.current_file.to_string(),
                    )))
                }
            } else {
                Some(Value::False)
            }
        }
        b"getstartline" => {
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()) {
                Some(Value::Long(op_array.decl_line as i64))
            } else {
                Some(Value::False)
            }
        }
        b"getendline" => {
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()) {
                let l = if op_array.end_line > 0 {
                    op_array.end_line as i64
                } else {
                    op_array.decl_line as i64
                };
                if l > 0 {
                    Some(Value::Long(l))
                } else {
                    Some(Value::False)
                }
            } else {
                Some(Value::False)
            }
        }
        b"getdoccomment" => {
            if let Some(op_array) = rfunc(vm, func_lower.as_slice()) {
                if let Some(doc) = op_array.doc_comment.as_ref() {
                    Some(Value::String(PhpString::from_bytes(doc)))
                } else {
                    Some(Value::False)
                }
            } else {
                Some(Value::False)
            }
        }
        b"getstaticvariables" => {
            // Return static variables of the function. PHP returns them even
            // when the function hasn't yet been called, in which case the
            // values are the compile-time defaults. Walk the op_array looking
            // for `StaticVarInit` opcodes and merge with any runtime values.
            // A closure's `use` variables come first: Zend binds them into
            // its static_variables table.
            let closure = obj.borrow().get_property(b"__reflection_closure_obj");
            let mut result = if matches!(closure, Value::Object(_)) {
                // zval_add_ref(): a reference only the closure holds is
                // copied as its value.
                // (this reflector keeps a copy of the captured values)
                let held = match obj.borrow().get_property(b"__reflection_closure_captured") {
                    Value::Array(a) => a
                        .borrow()
                        .values()
                        .filter_map(|v| match v {
                            Value::Reference(r) => Some(Rc::as_ptr(r) as usize),
                            _ => None,
                        })
                        .collect::<Vec<usize>>(),
                    _ => Vec::new(),
                };
                let mut used = PhpArray::new();
                for (k, v) in vm.closure_used_vars(&closure).iter() {
                    let v = match v {
                        Value::Reference(r)
                            if Rc::strong_count(r)
                                <= 2 + held
                                    .iter()
                                    .filter(|p| **p == Rc::as_ptr(r) as usize)
                                    .count() =>
                        {
                            r.borrow().clone()
                        }
                        v => v.clone(),
                    };
                    used.set(k.clone(), v);
                }
                used
            } else {
                PhpArray::new()
            };
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()).cloned() {
                let func_prefix = {
                    let mut p = op_array.name.clone();
                    p.extend_from_slice(b"::");
                    p
                };
                for op in &op_array.ops {
                    if !matches!(op.opcode, crate::opcode::OpCode::StaticVarInit) {
                        continue;
                    }
                    let key_bytes = match op.result {
                        crate::opcode::OperandType::Const(idx) => {
                            match op_array.literals.get(idx as usize) {
                                Some(Value::String(s)) => s.as_bytes().to_vec(),
                                _ => continue,
                            }
                        }
                        _ => continue,
                    };
                    // Pull the variable name (after the "name::" prefix).
                    let var_name: Vec<u8> = if key_bytes.starts_with(&func_prefix) {
                        key_bytes[func_prefix.len()..].to_vec()
                    } else {
                        continue;
                    };
                    // Prefer runtime value; fall back to the literal default.
                    // A static bound by the running function stays a
                    // reference (zval_add_ref keeps refcount > 1 refs).
                    let val = if let Some(v) = vm.static_vars().get(&key_bytes) {
                        match v {
                            Value::Reference(r) if Rc::strong_count(r) > 1 => {
                                Value::Reference(r.clone())
                            }
                            v => v.deref(),
                        }
                    } else {
                        match op.op2 {
                            crate::opcode::OperandType::Const(idx) => op_array
                                .literals
                                .get(idx as usize)
                                .cloned()
                                .unwrap_or(Value::Null),
                            _ => eval_static_const_init(vm, &op_array, op).unwrap_or(Value::Null),
                        }
                    };
                    result.set(ArrayKey::String(PhpString::from_vec(var_name)), val);
                }
            }
            // Also include any runtime statics created later that we missed.
            let prefix = format!("{}::", crate::string::msg_str(&func_lower));
            for (key, val) in vm.static_vars() {
                let key_str = crate::string::msg_str(key);
                if key_str.starts_with(&prefix) {
                    let var_name = &key_str[prefix.len()..];
                    let key = ArrayKey::String(PhpString::from_msg(&var_name.to_string()));
                    if result.get(&key).is_none() {
                        result.set(key, val.deref());
                    }
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getnumberofparameters" => {
            if let Some(op_array) = rfunc(vm, &func_lower) {
                let hidden =
                    closure_hidden_params(op_array) + method_this_hidden(&func_lower, op_array);
                Some(Value::Long(
                    (op_array.param_count as usize).saturating_sub(hidden) as i64,
                ))
            } else if vm.functions.contains_key(func_lower.as_slice()) {
                // For built-in functions, check param names
                let count = vm
                    .builtin_param_names
                    .get(func_lower.as_slice())
                    .map(|p| p.len() as i64)
                    .unwrap_or(0);
                Some(Value::Long(count))
            } else {
                Some(Value::Long(0))
            }
        }
        b"getnumberofrequiredparameters" => {
            if let Some(op_array) = rfunc(vm, &func_lower) {
                Some(Value::Long(op_array.required_param_count as i64))
            } else {
                Some(Value::Long(0))
            }
        }
        b"returnsreference" => {
            let by_ref = rfunc(vm, &func_lower)
                .map(|op| op.returns_by_ref)
                .unwrap_or(false);
            Some(if by_ref { Value::True } else { Value::False })
        }
        b"getparameters" => {
            if let Some(op_array) = rfunc(vm, &func_lower).cloned() {
                // (a closure of a method: the method's parameters)
                let list = if func_lower.windows(2).any(|w| w == b"::")
                    && op_array.scope_class.is_some()
                    && !func_lower.starts_with(b"__closure_fcc_")
                {
                    let is_static = method_this_hidden(&func_lower, &op_array) == 0;
                    create_reflection_parameters_method(vm, &op_array, is_static)
                } else {
                    create_reflection_parameters(vm, &op_array)
                };
                let closure = obj.borrow().get_property(b"__reflection_closure_obj");
                if !matches!(closure, Value::Null | Value::Undef) {
                    let scope = vm.closure_scope(&closure);
                    set_attr_scope(&list, scope, b"__reflection_param_attr_scope");
                    // (getDeclaringFunction() reflects the closure itself)
                    if let Value::Array(a) = &list {
                        for (_, p) in a.borrow().iter() {
                            if let Value::Object(po) = p {
                                po.borrow_mut().set_property(
                                    b"__reflection_param_closure".to_vec(),
                                    closure.clone(),
                                );
                            }
                        }
                    }
                }
                Some(list)
            } else {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
        }
        b"getreturntype" => {
            if let Some(op_array) = rfunc(vm, &func_lower) {
                if let Some(ref rt) = op_array.return_type {
                    let rt = rt.clone();
                    Some(create_reflection_type(vm, &rt))
                } else {
                    Some(Value::Null)
                }
            } else {
                Some(Value::Null)
            }
        }
        b"hasreturntype" => {
            let has = rfunc(vm, &func_lower)
                .and_then(|op| op.return_type.as_ref())
                .is_some();
            Some(if has { Value::True } else { Value::False })
        }
        b"isclosure" => {
            let ob = obj.borrow();
            let is_closure = ob.has_property(b"__reflection_is_closure");
            Some(if is_closure {
                Value::True
            } else {
                Value::False
            })
        }
        b"isvariadic" => {
            let is_variadic = rfunc(vm, &func_lower)
                .and_then(|op| op.variadic_param)
                .is_some();
            Some(if is_variadic {
                Value::True
            } else {
                Value::False
            })
        }
        b"isdeprecated" => {
            // Check for the #[\Deprecated] attribute on the function.
            let fl = magic_target(obj).unwrap_or_else(|| func_lower.clone());
            let has_dep = rfunc(vm, &fl)
                .map(|op| {
                    op.attributes.iter().any(|a| {
                        let n = crate::string::msg_str(&a.name);
                        n.eq_ignore_ascii_case("deprecated")
                            || n.eq_ignore_ascii_case("\\deprecated")
                    })
                })
                .unwrap_or(false)
                // An internal function: ZEND_ACC_DEPRECATED from its stub.
                || (vm.functions.contains_key(fl.as_slice())
                    && crate::stubs::find_function(&fl)
                        .is_some_and(|f| f.flags & crate::stubs::F_DEPRECATED != 0));
            Some(if has_dep { Value::True } else { Value::False })
        }
        b"isgenerator" => {
            let is_gen = rfunc(vm, &func_lower)
                .map(|op| op.is_generator)
                .unwrap_or(false);
            Some(if is_gen { Value::True } else { Value::False })
        }
        b"getextension" => {
            let func_str = crate::string::msg_str(&func_lower);
            let ext_name = internal_function_ext(vm, &func_lower)
                .or_else(|| get_function_extension(&func_str));
            if let Some(ext) = ext_name {
                let obj_id = vm.alloc_object_id();
                let mut ext_obj = PhpObject::new(b"ReflectionExtension".to_vec(), obj_id);
                ext_obj.set_property(
                    b"name".to_vec(),
                    Value::String(PhpString::from_msg(&ext.to_string())),
                );
                Some(Value::Object(Rc::new(RefCell::new(ext_obj))))
            } else {
                Some(Value::Null)
            }
        }
        b"getextensionname" => {
            let func_str = crate::string::msg_str(&func_lower);
            let ext_name = internal_function_ext(vm, &func_lower)
                .or_else(|| get_function_extension(&func_str));
            if let Some(ext) = ext_name {
                Some(Value::String(PhpString::from_msg(&ext.to_string())))
            } else {
                Some(Value::False)
            }
        }
        b"isanonymous" => {
            let ob = obj.borrow();
            let is_closure = ob.has_property(b"__reflection_is_closure");
            // FCC closures (`strlen(...)`) wrap a named function — they are
            // NOT anonymous, even though the closure carries an internal
            // `__closure_fcc_*` name. Detect via the underlying op_array's
            // fcc_target_name.
            let is_fcc = if is_closure {
                vm.user_functions
                    .get(func_lower.as_slice())
                    .map(|op| {
                        op.fcc_target_name.is_some()
                            || !crate::closure::is_closure_fn_name(&op.name)
                    })
                    .unwrap_or(true)
            } else {
                false
            };
            Some(if is_closure && !is_fcc {
                Value::True
            } else {
                Value::False
            })
        }
        b"isstatic" => {
            // Closures declared `static function()` carry is_static_closure on
            // their OpArray. For free functions this is always false; a
            // closure of a static method (Class::m(...), fromCallable())
            // is static.
            let st = vm
                .user_functions
                .get(func_lower.as_slice())
                .is_some_and(|op| {
                    op.is_static_closure
                        || op
                            .fcc_target_name
                            .as_deref()
                            .is_some_and(|t| static_method_named(vm, t))
                })
                || static_method_named(vm, &func_lower);
            Some(if st { Value::True } else { Value::False })
        }
        b"innamespace" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_msg_string();
            Some(if name.contains('\\') && !name.starts_with("{closure") {
                Value::True
            } else {
                Value::False
            })
        }
        b"getnamespacename" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_msg_string();
            if name.starts_with("{closure") {
                return Some(Value::String(PhpString::from_bytes(b"")));
            }
            if let Some(pos) = name.rfind('\\') {
                Some(Value::String(PhpString::from_msg(&name[..pos].to_string())))
            } else {
                Some(Value::String(PhpString::from_bytes(b"")))
            }
        }
        b"getshortname" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_msg_string();
            if name.starts_with("{closure") {
                return Some(Value::String(PhpString::from_msg(&name)));
            }
            if let Some(pos) = name.rfind('\\') {
                Some(Value::String(PhpString::from_msg(
                    &name[pos + 1..].to_string(),
                )))
            } else {
                Some(Value::String(PhpString::from_msg(&name)))
            }
        }
        b"getclosurethis" if obj.borrow().has_property(b"__reflection_closure_obj") => {
            let c = obj.borrow().get_property(b"__reflection_closure_obj");
            Some(vm.closure_this(&c).unwrap_or(Value::Null))
        }
        b"getclosure" if obj.borrow().has_property(b"__reflection_closure_obj") => {
            Some(obj.borrow().get_property(b"__reflection_closure_obj"))
        }
        b"getclosurescopeclass" | b"getclosurecalledclass"
            if obj.borrow().has_property(b"__reflection_closure_obj") =>
        {
            let c = obj.borrow().get_property(b"__reflection_closure_obj");
            let scope = if method == b"getclosurecalledclass" {
                vm.closure_called_class(&c)
            } else {
                vm.closure_scope(&c)
            };
            match scope {
                Some(n) => Some(create_reflection_class(vm, &crate::string::msg_str(&n))),
                None => Some(Value::Null),
            }
        }
        b"getclosureusedvariables" if obj.borrow().has_property(b"__reflection_closure_obj") => {
            let c = obj.borrow().get_property(b"__reflection_closure_obj");
            Some(Value::Array(Rc::new(RefCell::new(
                vm.closure_used_vars(&c),
            ))))
        }
        b"getclosurethis" => {
            let ob = obj.borrow();
            if ob.has_property(b"__reflection_closure_this") {
                Some(ob.get_property(b"__reflection_closure_this"))
            } else {
                Some(Value::Null)
            }
        }
        b"getclosure" => {
            // Return a Closure wrapping the function. The function name itself
            // is a valid string callable in our VM.
            let ob = obj.borrow();
            if ob.has_property(b"__reflection_is_closure") {
                return Some(ob.get_property(b"__reflection_target"));
            }
            let name = ob.get_property(b"name").to_php_string();
            drop(ob);
            let func_lower: Vec<u8> = name
                .as_bytes()
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            if vm.user_functions.contains_key(func_lower.as_slice())
                || vm.functions.contains_key(func_lower.as_slice())
            {
                Some(vm.make_closure_object(Value::String(name)))
            } else {
                Some(Value::Null)
            }
        }
        b"getclosurescopeclass" => {
            let ob = obj.borrow();
            if ob.has_property(b"__reflection_closure_scope") {
                let name = ob
                    .get_property(b"__reflection_closure_scope")
                    .to_php_string()
                    .to_msg_string();
                drop(ob);
                Some(create_reflection_class(vm, &name))
            } else {
                Some(Value::Null)
            }
        }
        b"getclosurecalledclass" => {
            let ob = obj.borrow();
            if ob.has_property(b"__reflection_closure_called") {
                let name = ob
                    .get_property(b"__reflection_closure_called")
                    .to_php_string()
                    .to_msg_string();
                drop(ob);
                Some(create_reflection_class(vm, &name))
            } else {
                Some(Value::Null)
            }
        }
        b"getclosureusedvariables" => {
            let ob = obj.borrow();
            let captured = match ob.get_property(b"__reflection_closure_captured") {
                Value::Array(a) => Some(a),
                _ => None,
            };
            drop(ob);
            if let Some(captured_arr) = captured {
                // Map captured values onto the closure's first N cv_names —
                // those are the lexical `use` variables (in source order).
                let cv_names: Vec<Vec<u8>> = vm
                    .user_functions
                    .get(func_lower.as_slice())
                    .map(|op| op.cv_names.clone())
                    .unwrap_or_default();
                let values: Vec<Value> = captured_arr.borrow().values().cloned().collect();
                let mut out = PhpArray::new();
                // Skip leading `$this` slot if present.
                let mut name_offset = 0usize;
                if cv_names
                    .first()
                    .map(|n| n.as_slice() == b"this")
                    .unwrap_or(false)
                {
                    name_offset = 1;
                }
                for (i, v) in values.iter().enumerate() {
                    if let Some(n) = cv_names.get(name_offset + i) {
                        let key = ArrayKey::String(PhpString::from_vec(n.clone()));
                        out.set(key, v.clone());
                    }
                }
                Some(Value::Array(Rc::new(RefCell::new(out))))
            } else {
                Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
            }
        }
        b"hastentativereturntype" => {
            let t = rfunc(vm, &func_lower)
                .and_then(|op| op.internal_sig)
                .map(|f| f.flags & crate::stubs::F_TENTATIVE != 0 && !f.ret.is_empty())
                .unwrap_or(false);
            Some(if t { Value::True } else { Value::False })
        }
        b"gettentativereturntype" => {
            let rt = rfunc(vm, &func_lower)
                .filter(|op| {
                    op.internal_sig
                        .map(|f| f.flags & crate::stubs::F_TENTATIVE != 0)
                        .unwrap_or(false)
                })
                .and_then(|op| op.return_type.clone());
            Some(match rt {
                Some(rt) => create_reflection_type(vm, &rt),
                None => Value::Null,
            })
        }
        b"__tostring"
            if !vm.user_functions.contains_key(func_lower.as_slice())
                && rfunc(vm, &func_lower).is_some() =>
        {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_msg_string();
            // (strlen(...): a Closure of an internal function)
            let is_closure = ob.has_property(b"__reflection_is_closure");
            drop(ob);
            let op = rfunc(vm, &func_lower).unwrap();
            let ctx = FnStrCtx {
                op,
                method: None,
                scope: None,
                is_closure,
                internal_fn_ext: None,
                display_name: &name,
                filename: "",
                internal_user_arginfo: false,
                params_skip: None,
            };
            Some(Value::String(PhpString::from_msg(&function_string(
                vm, &ctx, "",
            ))))
        }
        b"__tostring" if vm.user_functions.contains_key(func_lower.as_slice()) => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_msg_string();
            let is_closure = ob.has_property(b"__reflection_is_closure");
            let captured: Vec<Value> = match ob.get_property(b"__reflection_closure_captured") {
                Value::Array(a) => a.borrow().values().cloned().collect(),
                _ => Vec::new(),
            };
            drop(ob);
            let op = vm.user_functions.get(func_lower.as_slice()).unwrap();
            let display = if !op.display_name.is_empty() {
                crate::string::msg_str(&op.display_name).to_string()
            } else if is_closure {
                name.clone()
            } else {
                crate::string::msg_str(&op.name).to_string()
            };
            let display = if display.is_empty() {
                name.clone()
            } else {
                display
            };
            let filename = if op.filename.is_empty() {
                vm.current_file.to_string()
            } else {
                crate::string::msg_str(&op.filename).into_owned()
            };
            // A closure's leading CVs are its captured values ($this
            // first), not parameters.
            let captures = if is_closure && crate::closure::is_closure_fn_name(&op.name) {
                op.closure_capture_names.len()
            } else if is_closure && op.cv_names.first().is_some_and(|n| n.as_slice() == b"this") {
                // (a method's closure, $obj->m(...): its $this)
                1
            } else {
                0
            };
            // A method's closure is named after the method.
            let display = match display.rsplit_once("::") {
                Some((_, m)) if is_closure && !display.starts_with("{closure") => m.to_string(),
                _ => display,
            };
            // The closure's static_variables: `use` variables, then statics.
            let mut bound: Vec<String> = if captured.is_empty() {
                Vec::new()
            } else {
                op.closure_capture_names
                    .iter()
                    .filter(|n| n.as_slice() != b"this")
                    .map(|n| crate::string::msg_str(n).into_owned())
                    .collect()
            };
            for n in op_static_var_names(op) {
                let n = crate::string::msg_str(&n).into_owned();
                if !bound.contains(&n) {
                    bound.push(n);
                }
            }
            let ctx = FnStrCtx {
                op,
                method: None,
                scope: None,
                is_closure,
                internal_fn_ext: None,
                display_name: &display,
                filename: &filename,
                internal_user_arginfo: false,
                params_skip: (captures > 0).then_some(captures),
            };
            let mut out = function_string(vm, &ctx, "");
            // Bound variables (closure `use` vars) go right after the @@ line.
            if is_closure {
                insert_bound_variables(&mut out, &bound);
            }
            Some(Value::String(PhpString::from_msg(&out)))
        }
        b"__tostring" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_msg_string();
            drop(ob);
            if let Some(op_array) = vm.user_functions.get(func_lower.as_slice()).cloned() {
                let mut s = String::new();
                if let Some(doc) = op_array.doc_comment.as_ref() {
                    s.push_str(&crate::string::msg_str(doc));
                    s.push('\n');
                }
                s.push_str(&format!("Function [ <user> function {} ] {{\n", name));
                let f_end = if op_array.end_line > 0 {
                    op_array.end_line
                } else {
                    op_array.decl_line
                };
                s.push_str(&format!(
                    "  @@ {} {} - {}\n",
                    vm.current_file, op_array.decl_line, f_end
                ));
                if op_array.param_count > 0 {
                    s.push_str(&format!("\n  - Parameters [{}] {{\n", op_array.param_count));
                    let variadic_cv = op_array.variadic_param;
                    for i in 0..op_array.param_count as usize {
                        if i < op_array.cv_names.len() {
                            let pname = crate::string::msg_str(&op_array.cv_names[i]);
                            let is_variadic = variadic_cv.map(|v| v as usize == i).unwrap_or(false);
                            let required =
                                !is_variadic && i < op_array.required_param_count as usize;
                            let default_suffix = if !required && !is_variadic {
                                if let Some(const_ref) =
                                    extract_param_default_constant(&op_array, i as u32)
                                {
                                    let display = if let Some(rest) =
                                        const_ref.strip_prefix("global:")
                                    {
                                        rest.trim_start_matches('\\').to_string()
                                    } else if let Some(rest) = const_ref.strip_prefix("class:") {
                                        rest.to_string()
                                    } else {
                                        const_ref
                                    };
                                    format!(" = {}", display)
                                } else if let Some(v) = extract_param_default(&op_array, i as u32) {
                                    format!(" = {}", format_reflection_default(&v))
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };
                            let variadic_prefix = if is_variadic { "..." } else { "" };
                            s.push_str(&format!(
                                "    Parameter #{} [ <{}> {}${}{} ]\n",
                                i,
                                if required { "required" } else { "optional" },
                                variadic_prefix,
                                pname,
                                default_suffix
                            ));
                        }
                    }
                    s.push_str("  }\n");
                }
                s.push_str("}\n");
                Some(Value::String(PhpString::from_msg(&s)))
            } else {
                Some(Value::String(PhpString::from_msg(&format!(
                    "Function [ <internal> function {} ]",
                    name
                ))))
            }
        }
        _ => None,
    }
}

/// ReflectionFunction methods with args
pub fn reflection_function_docall(vm: &mut Vm, method: &[u8], args: &[Value]) -> Option<Value> {
    match deprecated_method_call(vm, b"reflectionfunction", method, args) {
        Some(true) => return Some(Value::Null),
        // isDisabled(): a disabled function has no ReflectionFunction
        Some(false) => return Some(Value::False),
        None => {}
    }
    let this = args.first()?;
    if let Value::Object(obj) = this {
        if obj.borrow().has_property(b"__reflection_internal_method") {
            if let Some(r) = internal_method_rf(vm, method, obj, Some(args)) {
                return Some(r);
            }
        }
        let ob = obj.borrow();
        let target = ob.get_property(b"__reflection_target").to_php_string();
        let func_lower = target.as_bytes().to_vec();
        drop(ob);

        let closure_obj = obj.borrow().get_property(b"__reflection_closure_obj");
        // Named arguments (invoke()'s extra named ones, invokeArgs()'s string
        // keys) are bound by the call like call_user_func_array() does.
        if matches!(method, b"invoke" | b"invokeargs") {
            let mut arr = if method == b"invoke" {
                let mut a = PhpArray::new();
                for v in args.iter().skip(1) {
                    a.push(v.clone());
                }
                a
            } else {
                match args.get(1) {
                    Some(Value::Array(a)) => a.borrow().clone(),
                    _ => PhpArray::new(),
                }
            };
            if method == b"invoke" {
                for (n, v) in std::mem::take(&mut vm.pending_named_args) {
                    arr.set(ArrayKey::String(PhpString::from_vec(n)), v);
                }
            }
            if arr.iter().any(|(k, _)| matches!(k, ArrayKey::String(_))) {
                let callable = if closure_obj.is_closure_obj() {
                    closure_obj
                } else {
                    Value::String(PhpString::from_vec(func_lower.clone()))
                };
                let f = vm
                    .functions
                    .get(b"call_user_func_array".as_slice())
                    .copied()?;
                let r = f(vm, &[callable, Value::Array(Rc::new(RefCell::new(arr)))]);
                return Some(r.unwrap_or(Value::Null));
            }
        }
        if closure_obj.is_closure_obj() {
            match method {
                b"invoke" | b"invokeargs" => {
                    let invoke_args: Vec<Value> = if method == b"invoke" {
                        args.iter().skip(1).cloned().collect()
                    } else {
                        match args.get(1) {
                            Some(Value::Array(a)) => a.borrow().values().cloned().collect(),
                            _ => Vec::new(),
                        }
                    };
                    let r = vm
                        .call_callback(&closure_obj, &invoke_args)
                        .unwrap_or(Value::Null);
                    return Some(r);
                }
                b"getclosure" => return Some(closure_obj),
                _ => {}
            }
        }
        match method {
            b"invoke" => {
                let invoke_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                if let Some(op_array) = vm.user_functions.get(&func_lower).cloned() {
                    // (zend_call_function(): a deprecated function warns; the
                    // function gets its own frame, which func_get_args() reads)
                    let line = vm.current_line;
                    vm.check_user_function_deprecation(
                        &op_array,
                        &crate::string::msg_str(&func_lower),
                        line,
                    );
                    if vm.current_exception.is_some() {
                        return Some(Value::Null);
                    }
                    let result = vm
                        .execute_fn_with_named_args(&op_array, invoke_args, Vec::new(), None)
                        .unwrap_or(Value::Null);
                    Some(result)
                } else if let Some(func) = vm.functions.get(&func_lower).cloned() {
                    let result = vm
                        .call_builtin_from_internal(func, &func_lower, &invoke_args)
                        .unwrap_or(Value::Null);
                    Some(result)
                } else {
                    Some(Value::Null)
                }
            }
            b"invokeargs" => {
                let args_arr = args
                    .get(1)
                    .cloned()
                    .unwrap_or(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                let invoke_args: Vec<Value> = if let Value::Array(arr) = &args_arr {
                    arr.borrow().iter().map(|(_, v)| v.clone()).collect()
                } else {
                    vec![]
                };
                if let Some(op_array) = vm.user_functions.get(&func_lower).cloned() {
                    // (zend_call_function(): a deprecated function warns; the
                    // function gets its own frame, which func_get_args() reads)
                    let line = vm.current_line;
                    vm.check_user_function_deprecation(
                        &op_array,
                        &crate::string::msg_str(&func_lower),
                        line,
                    );
                    if vm.current_exception.is_some() {
                        return Some(Value::Null);
                    }
                    let result = vm
                        .execute_fn_with_named_args(&op_array, invoke_args, Vec::new(), None)
                        .unwrap_or(Value::Null);
                    Some(result)
                } else if let Some(func) = vm.functions.get(&func_lower).cloned() {
                    let result = vm
                        .call_builtin_from_internal(func, &func_lower, &invoke_args)
                        .unwrap_or(Value::Null);
                    Some(result)
                } else {
                    Some(Value::Null)
                }
            }
            b"getattributes" => {
                let fl = magic_target(obj).unwrap_or_else(|| func_lower.clone());
                let attrs = rfunc(vm, fl.as_slice())
                    .map(|op| op.attributes.clone())
                    .unwrap_or_default();
                let filter_name = args.get(1).and_then(|v| {
                    if matches!(v, Value::Null) {
                        None
                    } else {
                        Some(v.to_php_string().as_bytes().to_vec())
                    }
                });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                // A closure of a method validates against TARGET_METHOD.
                let target = if fl.windows(2).any(|w| w == b"::") {
                    4
                } else {
                    2
                };
                let list = create_reflection_attributes(
                    vm,
                    &attrs,
                    filter_name.as_deref(),
                    filter_flags,
                    target,
                );
                let closure = obj.borrow().get_property(b"__reflection_closure_obj");
                if !matches!(closure, Value::Null | Value::Undef) {
                    let scope = vm.closure_scope(&closure);
                    set_attr_scope(&list, scope, b"__reflection_attr_scope");
                }
                Some(list)
            }
            b"getclosure" => {
                // Return the function name as a callable string (treated as a
                // closure by Closure::__invoke).
                let name = {
                    let obj_r = obj.borrow();
                    obj_r.get_property(b"name").to_php_string().to_msg_string()
                };
                // If the underlying target is already a closure-ish string,
                // return it as-is.
                let target_str = crate::string::msg_str(&func_lower).to_string();
                if target_str.starts_with("__closure_")
                    || target_str.starts_with("__arrow_")
                    || target_str.starts_with("__bound_closure_")
                    || target_str.starts_with("__closure_fcc_")
                {
                    Some(Value::String(PhpString::from_msg(&target_str)))
                } else {
                    Some(Value::String(PhpString::from_msg(&name)))
                }
            }
            _ => None,
        }
    } else {
        None
    }
}

/// ReflectionProperty no-arg method dispatch
pub fn reflection_property_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let class_name = ob
        .get_property(b"__reflection_class")
        .to_php_string()
        .to_msg_string();
    let prop_name = ob
        .get_property(b"__reflection_prop")
        .to_php_string()
        .to_msg_string();
    let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&class_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    // Dynamic-property flag: when set, the ReflectionProperty refers to a
    // runtime-only instance property that doesn't exist in the class's
    // PropertyDef list. It is always considered public and declared by the
    // runtime class (PHP behavior for dynamic props with AllowDynamicProperties).
    let is_dynamic = matches!(ob.get_property(b"__reflection_prop_dynamic"), Value::True);
    drop(ob);

    match method {
        b"getname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        b"getdeclaringclass" => {
            if is_dynamic {
                // Dynamic props are owned by the runtime class.
                let canonical = rcls(vm, &class_lower)
                    .map(|c| crate::string::msg_str(&c.name).to_string())
                    .unwrap_or(class_name.clone());
                return Some(create_reflection_class(vm, &canonical));
            }
            let declaring = rcls(vm, &class_lower)
                .and_then(|_c| {
                    rprop(vm, &class_lower, prop_name.as_bytes())
                        .map(|p| crate::string::msg_str(&p.declaring_class).to_string())
                })
                .unwrap_or(class_name.clone());
            let declaring_canonical =
                rcls(vm, declaring.as_bytes().to_ascii_lowercase().as_slice())
                    .map(|c| crate::string::msg_str(&c.name).to_string())
                    .unwrap_or(declaring.clone());
            Some(create_reflection_class(vm, &declaring_canonical))
        }
        b"ispublic" => {
            if is_dynamic {
                return Some(Value::True);
            }
            let vis = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()).map(|p| p.visibility))
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Public {
                Value::True
            } else {
                Value::False
            })
        }
        b"isprotected" => {
            if is_dynamic {
                return Some(Value::False);
            }
            let vis = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()).map(|p| p.visibility))
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Protected {
                Value::True
            } else {
                Value::False
            })
        }
        b"isprivate" => {
            if is_dynamic {
                return Some(Value::False);
            }
            let vis = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()).map(|p| p.visibility))
                .unwrap_or(Visibility::Public);
            Some(if vis == Visibility::Private {
                Value::True
            } else {
                Value::False
            })
        }
        b"isstatic" => {
            let is_static = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()).map(|p| p.is_static))
                .unwrap_or(false);
            Some(if is_static { Value::True } else { Value::False })
        }
        b"isreadonly" => {
            let is_readonly = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()).map(|p| p.is_readonly))
                .unwrap_or(false);
            Some(if is_readonly {
                Value::True
            } else {
                Value::False
            })
        }
        b"isvirtual" => {
            let is_virtual = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()).map(prop_is_virtual))
                .unwrap_or(false);
            Some(if is_virtual {
                Value::True
            } else {
                Value::False
            })
        }
        b"isabstract" => {
            let is_abstract = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()))
                .is_some_and(|p| reflection_property_modifiers(vm, p) & 0x40 != 0);
            Some(if is_abstract {
                Value::True
            } else {
                Value::False
            })
        }
        b"isfinal" => {
            // Either explicitly final OR `private(set)` (implicit final
            // because the setter cannot be overridden).
            let is_final = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()))
                .map(|p| p.is_final || p.set_visibility == Some(Visibility::Private))
                .unwrap_or(false);
            Some(if is_final { Value::True } else { Value::False })
        }
        b"hashooks" => {
            let has_hooks = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()))
                .map(|p| p.has_get_hook || p.has_set_hook)
                .unwrap_or(false);
            Some(if has_hooks { Value::True } else { Value::False })
        }
        b"isdefault" => {
            let is_default = rcls(vm, &class_lower)
                .map(|_c| rprop(vm, &class_lower, prop_name.as_bytes()).is_some())
                .unwrap_or(false);
            Some(if is_default {
                Value::True
            } else {
                Value::False
            })
        }
        b"getdefaultvalue" => {
            // Per PHP 8.5: emit deprecation when called for a property that
            // has no default (Undef) — but only for *typed* properties.
            // Untyped properties implicitly default to NULL, so no deprecation.
            // Dynamic properties (looked up by object) also trigger the warning.
            let prop_info = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()).cloned());
            let (default_val, has_default, has_type) = match &prop_info {
                Some(p) => {
                    let has = !p.is_virtual && !matches!(p.default, Value::Undef);
                    // A virtual property has no default either.
                    (
                        p.default.clone(),
                        has,
                        p.property_type.is_some() || p.is_virtual,
                    )
                }
                None => {
                    // Dynamic property: counts as no-default + typed (warns)
                    (Value::Null, false, true)
                }
            };
            if !has_default && has_type {
                vm.emit_deprecated(
                    "ReflectionProperty::getDefaultValue() for a property without a default value is deprecated, use ReflectionProperty::hasDefaultValue() to check if the default value exists",
                );
            }
            let result = match default_val {
                Value::Undef => Value::Null,
                v @ Value::String(_) => vm.resolve_deferred_value(&v),
                v => v,
            };
            Some(result)
        }
        b"hasdefaultvalue" => {
            let has = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()))
                // (a virtual property has no default)
                .map(|p| !p.is_virtual && !matches!(p.default, Value::Undef))
                .unwrap_or(false);
            Some(if has { Value::True } else { Value::False })
        }
        b"getmodifiers" => {
            let mods = rcls(vm, &class_lower)
                .and_then(|_c| {
                    rprop(vm, &class_lower, prop_name.as_bytes())
                        .map(|p| reflection_property_modifiers(vm, p))
                })
                .unwrap_or(1);
            Some(Value::Long(mods))
        }
        b"getdoccomment" => {
            let doc = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()))
                .and_then(|p| p.doc_comment.as_ref())
                .cloned();
            if let Some(d) = doc {
                Some(Value::String(PhpString::from_bytes(d.as_bytes())))
            } else {
                Some(Value::False)
            }
        }
        b"hastype" => {
            let has = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()))
                .map(|p| p.property_type.is_some())
                .unwrap_or(false);
            Some(if has { Value::True } else { Value::False })
        }
        b"gettype" => {
            let pt = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()))
                .and_then(|p| p.property_type.as_ref())
                .cloned();
            if let Some(pt) = pt {
                Some(create_reflection_type(vm, &pt))
            } else {
                Some(Value::Null)
            }
        }
        b"ispromoted" => {
            let decl = rprop(vm, &class_lower, prop_name.as_bytes())
                .filter(|p| !p.is_static)
                .map(|p| p.declaring_class.to_ascii_lowercase());
            let promoted = decl
                .and_then(|d| {
                    let m = rcls(vm, &d)?.get_method(b"__construct")?;
                    m.declaring_class
                        .eq_ignore_ascii_case(&d)
                        .then(|| op_array_promotes(&m.op_array, prop_name.as_bytes()))
                })
                .unwrap_or(false);
            Some(if promoted { Value::True } else { Value::False })
        }
        b"getmangledname" => {
            // PHP: private properties -> "\0ClassName\0name",
            // protected properties -> "\0*\0name", public -> "name".
            let (vis, declaring_lower) = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()))
                .map(|p| (p.visibility, p.declaring_class.clone()))
                .unwrap_or((Visibility::Public, class_lower.clone()));
            let declaring_canonical = rcls(vm, &declaring_lower)
                .map(|c| crate::string::msg_str(&c.name).to_string())
                .unwrap_or_else(|| crate::string::msg_str(&declaring_lower).to_string());
            let mangled = match vis {
                Visibility::Public => prop_name.clone(),
                Visibility::Protected => format!("\0*\0{}", prop_name),
                Visibility::Private => format!("\0{}\0{}", declaring_canonical, prop_name),
            };
            Some(Value::String(PhpString::from_msg(&mangled)))
        }
        b"isdynamic" => {
            // Dynamic properties are those not declared on the class.
            let is_declared = rcls(vm, &class_lower)
                .map(|_c| rprop(vm, &class_lower, prop_name.as_bytes()).is_some())
                .unwrap_or(false);
            Some(if is_declared {
                Value::False
            } else {
                Value::True
            })
        }
        b"isprivateset" => {
            let is_priv_set = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()))
                .is_some_and(|p| reflection_property_modifiers(vm, p) & 0x1000 != 0);
            Some(if is_priv_set {
                Value::True
            } else {
                Value::False
            })
        }
        b"isprotectedset" => {
            // (a public readonly property is implicitly protected(set))
            let is_prot_set = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()))
                .is_some_and(|p| reflection_property_modifiers(vm, p) & 0x800 != 0);
            Some(if is_prot_set {
                Value::True
            } else {
                Value::False
            })
        }
        b"gethooks" => {
            let mut arr = PhpArray::new();
            for kind in ["get", "set"] {
                if let Some(m) =
                    reflection_property_hook_method(vm, &class_lower, prop_name.as_bytes(), kind)
                {
                    arr.set(
                        crate::array::ArrayKey::String(PhpString::from_msg(&kind.to_string())),
                        m,
                    );
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(arr))))
        }
        b"getsettabletype" => {
            // The set hook's parameter type; never for a virtual property
            // without a set hook; else the property type.
            let decl = rcls(vm, &class_lower)
                .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()))
                .cloned();
            let mut pt = decl.as_ref().and_then(|p| p.property_type.clone());
            if let Some(d) = &decl {
                let mut m = b"__property_set_".to_vec();
                m.extend(prop_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()));
                let set_hook = rcls(vm, &class_lower).and_then(|c| c.get_method(&m));
                match set_hook {
                    Some(h) => {
                        let op = &h.op_array;
                        let off = usize::from(op.cv_names.first().is_some_and(|n| n == b"this"));
                        if let Some(Some(pti)) = op.param_types.get(off) {
                            pt = Some(pti.param_type.clone());
                        }
                    }
                    None if d.is_virtual => {
                        pt = Some(ParamType::Simple(b"never".to_vec()));
                    }
                    None => {}
                }
            }
            if let Some(pt) = pt {
                Some(create_reflection_type(vm, &pt))
            } else {
                Some(Value::Null)
            }
        }
        b"__tostring" => {
            let prop = rprop(vm, &class_lower, prop_name.as_bytes()).cloned();
            if let Some(p) = prop {
                Some(Value::String(PhpString::from_msg(&property_string(
                    vm, &p, "",
                ))))
            } else {
                Some(Value::String(PhpString::from_msg(&format!(
                    "Property [ <dynamic> public ${} ]\n",
                    prop_name
                ))))
            }
        }
        _ => None,
    }
}

/// Whether constructor `op` promotes its parameter `name` to a property.
fn op_array_promotes(op: &OpArray, name: &[u8]) -> bool {
    let Some(cv) = op.cv_names.iter().position(|n| n == name) else {
        return false;
    };
    op.promoted_params.contains(&(cv as u32))
}

/// Find a property declaration (static or not) on `class_lower` or its
/// ancestors.
/// reflection_property_get_effective_prop(): the property of the object's
/// class when it redeclares one that is not private in the reflected class
/// (the redeclaration may add hooks or change flags).
fn effective_prop(
    vm: &Vm,
    class_lower: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
    prop: &[u8],
) -> Option<crate::object::PropertyDef> {
    let r = class_prop_info(vm, class_lower, prop);
    let ocl = obj.borrow().class_name.to_ascii_lowercase();
    if ocl.as_slice() != class_lower
        && !r
            .as_ref()
            .is_some_and(|d| d.visibility == Visibility::Private)
    {
        return class_prop_info(vm, &ocl, prop);
    }
    r
}

/// The properties_info entry `prop` of class `class_lower`: its own
/// declaration before an inherited private one of the same name.
fn class_prop_info(vm: &Vm, class_lower: &[u8], prop: &[u8]) -> Option<crate::object::PropertyDef> {
    let c = vm.classes.get(class_lower)?;
    let mut it = c.properties.iter().filter(|p| p.name == prop);
    let first = it.clone().next();
    it.find(|p| {
        p.declaring_class.eq_ignore_ascii_case(class_lower) || p.visibility != Visibility::Private
    })
    .or(first)
    .cloned()
    .or_else(|| find_prop_decl(vm, class_lower, prop))
}

/// The storage key of `prop` in `obj` as seen from class `scope`.
fn raw_prop_key(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>, prop: &[u8], scope: &[u8]) -> Vec<u8> {
    vm.class_scope_stack.push(scope.to_ascii_lowercase());
    let k = vm.prop_access_key(obj, prop);
    vm.class_scope_stack.pop();
    k.unwrap_or_else(|| prop.to_vec())
}

/// The storage key of declared property `d` in `obj` (a private one may be
/// stored under its mangled name).
fn backing_key(
    vm: &mut Vm,
    obj: &Rc<RefCell<PhpObject>>,
    prop: &[u8],
    d: &crate::object::PropertyDef,
) -> Vec<u8> {
    if d.visibility == Visibility::Private {
        let decl = rcls(vm, &d.declaring_class.to_ascii_lowercase())
            .map(|c| c.name.clone())
            .unwrap_or_else(|| d.declaring_class.clone());
        let mut mk = vec![0u8];
        mk.extend_from_slice(&decl);
        mk.push(0);
        mk.extend_from_slice(prop);
        if obj.borrow().has_property(&mk) {
            return mk;
        }
    }
    raw_prop_key(vm, obj, prop, &d.declaring_class)
}

/// Whether property `d` (of class `cl`) has a `kind` ("get"/"set") hook.
fn prop_has_hook(vm: &Vm, d: &crate::object::PropertyDef, kind: &str, prop: &str) -> bool {
    if if kind == "get" {
        d.has_get_hook
    } else {
        d.has_set_hook
    } {
        return true;
    }
    let mut m = format!("__property_{}_", kind).into_bytes();
    m.extend(prop.bytes().map(|b| b.to_ascii_lowercase()));
    rcls(vm, &d.declaring_class.to_ascii_lowercase()).is_some_and(|c| c.get_method(&m).is_some())
}

/// ReflectionProperty::getRawValue() on the effective property: None when
/// the plain read applies (no get hook).
#[inline(never)]
fn raw_value_effective_get(
    vm: &mut Vm,
    class_lower: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
    prop: &str,
) -> Option<Value> {
    let pn = prop.as_bytes();
    let eff = effective_prop(vm, class_lower, obj, pn);
    if eff.as_ref().is_some_and(|d| d.is_static) {
        let exc = vm.create_exception(
            b"ReflectionException",
            "May not use getRawValue on static properties",
            vm.current_line,
        );
        vm.current_exception = Some(exc);
        return Some(Value::Null);
    }
    let d = eff?;
    if !prop_has_hook(vm, &d, "get", prop) {
        // The reflected class's view of the slot (a private of an ancestor).
        if d.visibility == Visibility::Private
            && !d.declaring_class.eq_ignore_ascii_case(class_lower)
        {
            return None;
        }
        let key = raw_prop_key(vm, obj, pn, class_lower);
        if key.as_slice() == pn {
            return None;
        }
        return Some(obj.borrow().get_property(&key));
    }
    if d.is_virtual {
        let msg = format!(
            "Must not read from virtual property {}::${}",
            class_display(vm, &d.declaring_class),
            prop
        );
        let exc = vm.create_exception(b"Error", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Some(Value::Null);
    }
    // The get hook's own access: the backing value.
    let key = backing_key(vm, obj, pn, &d);
    Some(obj.borrow().get_property(&key))
}

/// ReflectionProperty::setRawValue() on the effective property: false when
/// the older path applies.
#[inline(never)]
fn raw_value_effective_set(
    vm: &mut Vm,
    class_lower: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
    prop: &str,
    value: &Value,
) -> bool {
    let pn = prop.as_bytes();
    let eff = effective_prop(vm, class_lower, obj, pn);
    if eff.as_ref().is_some_and(|d| d.is_static) {
        let exc = vm.create_exception(
            b"ReflectionException",
            "May not use setRawValue on static properties",
            vm.current_line,
        );
        vm.current_exception = Some(exc);
        return true;
    }
    let ocl = obj.borrow().class_name.to_ascii_lowercase();
    let hooked = eff
        .as_ref()
        .is_some_and(|d| prop_has_hook(vm, d, "set", prop));
    if ocl.as_slice() == class_lower && !hooked {
        return false;
    }
    match eff {
        Some(d) if prop_has_hook(vm, &d, "set", prop) => {
            if d.is_virtual {
                let msg = format!(
                    "Must not write to virtual property {}::${}",
                    class_display(vm, &d.declaring_class),
                    prop
                );
                let exc = vm.create_exception(b"Error", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return true;
            }
            let key = backing_key(vm, obj, pn, &d);
            obj.borrow_mut().set_property(key, value.clone());
            true
        }
        _ => {
            // write_property() with the reflected class as scope.
            let _ = run_snippet_in_scope(
                vm,
                "$__o->{$__n} = $__v;",
                &[
                    ("__o", Value::Object(obj.clone())),
                    ("__n", Value::String(PhpString::from_msg(&prop.to_string()))),
                    ("__v", value.clone()),
                ],
                class_lower,
            );
            true
        }
    }
}

fn find_prop_decl(vm: &Vm, class_lower: &[u8], prop: &[u8]) -> Option<crate::object::PropertyDef> {
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        let c = vm.classes.get(&cur)?;
        if let Some(p) = c.properties.iter().find(|p| p.name == prop) {
            return Some(p.clone());
        }
        cur = c
            .parent
            .as_ref()?
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
    }
    None
}

fn class_display(vm: &Vm, class_lower: &[u8]) -> String {
    vm.classes
        .get(class_lower)
        .map(|c| crate::value::display_class_name(&c.name))
        .unwrap_or_else(|| vm.builtin_canonical_name(class_lower))
}

/// Z_PARAM_OBJ_OF_CLASS(object, intern->ce) for ReflectionProperty methods.
fn reflection_property_object_arg(
    vm: &mut Vm,
    target: &Value,
    class_lower: &[u8],
    method: &str,
) -> Option<Rc<RefCell<PhpObject>>> {
    if let Value::Object(o) = target {
        let ol: Vec<u8> = o
            .borrow()
            .class_name
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        if ol == class_lower || vm.class_extends(&ol, class_lower) {
            return Some(o.clone());
        }
    }
    let given = match target {
        Value::Object(o) => crate::value::display_class_name(&o.borrow().class_name),
        other => crate::vm::Vm::value_type_name(other),
    };
    let msg = format!(
        "ReflectionProperty::{}(): Argument #1 ($object) must be of type {}, {} given",
        method,
        class_display(vm, class_lower),
        given
    );
    let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    None
}

/// reflection_property_check_lazy_compatible()
fn reflection_property_check_lazy_compatible(
    vm: &mut Vm,
    decl: &Option<crate::object::PropertyDef>,
    class_lower: &[u8],
    prop: &[u8],
    method: &str,
) -> bool {
    let prop_s = crate::string::msg_str(prop).to_string();
    let msg = match decl {
        None => Some(format!(
            "Can not use {} on dynamic property {}::${}",
            method,
            class_display(vm, class_lower),
            prop_s
        )),
        Some(d) if d.is_static => Some(format!(
            "Can not use {} on static property {}::${}",
            method,
            class_display(vm, &d.declaring_class),
            prop_s
        )),
        Some(d) if d.is_virtual && (d.has_get_hook || d.has_set_hook) => Some(format!(
            "Can not use {} on virtual property {}::${}",
            method,
            class_display(vm, &d.declaring_class),
            prop_s
        )),
        _ => None,
    };
    if let Some(m) = msg {
        let exc = vm.create_exception(b"ReflectionException", &m, vm.current_line);
        vm.current_exception = Some(exc);
        return false;
    }
    true
}

/// Add/remove `prop` from the object's non-lazy (`__lazy_skipped`) list.
fn set_prop_lazy_flag(obj: &Rc<RefCell<PhpObject>>, prop: &[u8], lazy: bool) {
    let mut ob = obj.borrow_mut();
    let mut names: Vec<Vec<u8>> = match ob.get_property(b"__lazy_skipped") {
        Value::String(s) => s
            .as_bytes()
            .split(|b| *b == 0)
            .filter(|n| !n.is_empty())
            .map(|n| n.to_vec())
            .collect(),
        _ => Vec::new(),
    };
    names.retain(|n| n != prop);
    if !lazy {
        names.push(prop.to_vec());
    }
    if names.is_empty() {
        ob.remove_property(b"__lazy_skipped");
    } else {
        ob.set_property(
            b"__lazy_skipped".to_vec(),
            Value::String(PhpString::from_vec(names.join(&0u8))),
        );
    }
}

/// True if `prop` of the (proxy-resolved) object is still lazy.
/// Z_PARAM_FUNC for the `$initializer`/`$factory` argument of the lazy
/// object methods: false (with a TypeError thrown) when it is not callable.
#[inline(never)]
fn lazy_check_initializer_arg(vm: &mut Vm, cb: &Value, method: &str, arg_num: usize) -> bool {
    let Some(err) = vm.callable_error(cb) else {
        return true;
    };
    if vm.current_exception.is_some() {
        return false;
    }
    let param = if method.contains("Proxy") {
        "factory"
    } else {
        "initializer"
    };
    let msg = format!(
        "ReflectionClass::{}(): Argument #{} (${}) must be a valid callback, {}",
        method, arg_num, param, err
    );
    let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    false
}

/// object_init_ex()'s error for an interface, trait, enum or abstract class.
fn uninstantiable_class_error(vm: &Vm, class_lower: &[u8]) -> Option<String> {
    let c = rcls(vm, class_lower)?;
    let kind = if c.is_interface {
        "interface"
    } else if c.is_trait {
        "trait"
    } else if c.is_enum {
        "enum"
    } else if c.is_abstract {
        "abstract class"
    } else {
        return None;
    };
    Some(format!(
        "Cannot instantiate {} {}",
        kind,
        crate::value::display_class_name(&c.name)
    ))
}

fn prop_is_lazy(obj: &Rc<RefCell<PhpObject>>, prop: &[u8]) -> bool {
    Vm::is_uninit_lazy(obj) && !crate::vm::is_prop_skipped(obj, prop)
}

/// ReflectionClass::SKIP_INITIALIZATION_ON_SERIALIZE (ZEND_LAZY_OBJECT_SKIP_INITIALIZATION_ON_SERIALIZE)
pub const LAZY_SKIP_INIT_ON_SERIALIZE: i64 = 1 << 3;
/// ReflectionClass::SKIP_DESTRUCTOR (ZEND_LAZY_OBJECT_SKIP_DESTRUCTOR)
pub const LAZY_SKIP_DESTRUCTOR: i64 = 1 << 4;

pub fn reflection_property_docall(vm: &mut Vm, method: &[u8], args: &[Value]) -> Option<Value> {
    match deprecated_method_call(vm, b"reflectionproperty", method, args) {
        Some(true) => return Some(Value::Null),
        // setAccessible(): no effect
        Some(false) => return Some(Value::Null),
        None => {}
    }
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let prop_name = ob
            .get_property(b"__reflection_prop")
            .to_php_string()
            .to_msg_string();
        let class_name = ob
            .get_property(b"__reflection_class")
            .to_php_string()
            .to_msg_string();
        let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&class_name)
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        drop(ob);

        // Check if this is a static property
        let is_static = rcls(vm, &class_lower)
            .and_then(|_c| rprop(vm, &class_lower, prop_name.as_bytes()).map(|p| p.is_static))
            .unwrap_or(false);

        match method {
            b"getvalue" => {
                if args.len() > 2 {
                    let msg = format!(
                        "ReflectionProperty::getValue() expects at most 1 argument, {} given",
                        args.len() - 1
                    );
                    let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                if opt_object_arg_error(vm, "ReflectionProperty::getValue", args.get(1)) {
                    return Some(Value::Null);
                }
                if !is_static {
                    match args.get(1) {
                        Some(Value::Object(o)) => {
                            let ol = o.borrow().class_name.to_ascii_lowercase();
                            let decl = rprop(vm, &class_lower, prop_name.as_bytes())
                                .map(|p| p.declaring_class.to_ascii_lowercase())
                                .unwrap_or_else(|| class_lower.clone());
                            if ol != decl && !vm.class_extends(&ol, &decl) {
                                let exc = vm.create_exception(
                                    b"ReflectionException",
                                    "Given object is not an instance of the class this property was declared in",
                                    vm.current_line,
                                );
                                vm.current_exception = Some(exc);
                                return Some(Value::Null);
                            }
                        }
                        None | Some(Value::Null) => {
                            let exc = vm.create_exception(
                                b"TypeError",
                                "ReflectionProperty::getValue(): Argument #1 ($object) must be provided for instance properties",
                                vm.current_line,
                            );
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                        _ => {}
                    }
                }
                if is_static {
                    // Static properties are stored on the class that declared
                    // them, even when reflected via a subclass. Walk up the
                    // parent chain to find the storage slot.
                    let val = find_static_property_value(vm, &class_lower, prop_name.as_bytes())
                        .unwrap_or(Value::Null);
                    if matches!(val, Value::Undef) {
                        let decl =
                            find_static_property_owner(vm, &class_lower, prop_name.as_bytes())
                                .unwrap_or_else(|| class_lower.clone());
                        let cname = rcls(vm, &decl)
                            .map(|c| crate::string::msg_str(&c.name).to_string())
                            .unwrap_or_else(|| class_name.clone());
                        let err_msg = format!(
                            "Typed static property {}::${} must not be accessed before initialization",
                            cname, prop_name
                        );
                        let exc = vm.create_exception(b"Error", &err_msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    Some(val.deref())
                } else if let Some(target) = args.get(1) {
                    if let Value::Object(o) = target {
                        // SensitiveParameterValue keeps $value natively.
                        if prop_name == "value"
                            && o.borrow()
                                .class_name
                                .eq_ignore_ascii_case(b"SensitiveParameterValue")
                        {
                            return Some(o.borrow().get_property(b"__sensitive_value"));
                        }
                        // zend_read_property_ex() in the declaring class's scope.
                        let decl = rprop(vm, &class_lower, prop_name.as_bytes())
                            .map(|p| p.declaring_class.to_ascii_lowercase())
                            .unwrap_or_else(|| class_lower.clone());
                        let v = run_snippet_in_scope(
                            vm,
                            "return $__o->{$__n};",
                            &[
                                ("__o", target.clone()),
                                (
                                    "__n",
                                    Value::String(PhpString::from_msg(&prop_name.clone())),
                                ),
                            ],
                            &decl,
                        );
                        Some(v.unwrap_or(Value::Null))
                    } else {
                        Some(Value::Null)
                    }
                } else {
                    Some(Value::Null)
                }
            }
            b"setvalue" => {
                if is_static {
                    // For static properties, ReflectionProperty::setValue accepts:
                    //   setValue($obj_or_null, $value)  — two args (canonical)
                    //   setValue($value)                — one arg (deprecated)
                    // (args[0] is the ReflectionProperty $this slot.)
                    if args.len() >= 3 && !matches!(args[1].deref(), Value::Null | Value::Object(_))
                    {
                        vm.emit_deprecated(
                            "Calling ReflectionProperty::setValue() with a 1st argument which is not null or an object is deprecated",
                        );
                        if vm.current_exception.is_some() {
                            return Some(Value::Null);
                        }
                    }
                    let value = if args.len() >= 3 {
                        args[2].clone()
                    } else if args.len() >= 2 {
                        // Deprecated single-arg form on a static property.
                        vm.emit_deprecated(
                            "Calling ReflectionProperty::setValue() with a single argument is deprecated",
                        );
                        args[1].clone()
                    } else {
                        Value::Null
                    };
                    // Store on the declaring class, not the reflected one.
                    let declaring =
                        find_static_property_owner(vm, &class_lower, prop_name.as_bytes())
                            .unwrap_or_else(|| class_lower.clone());
                    // Typed static properties: weak-mode coercion, else TypeError.
                    let mut value = value;
                    if let Some(pt) = rprop(vm, &class_lower, prop_name.as_bytes())
                        .and_then(|p| p.property_type.clone())
                    {
                        let mut coerced = value.clone();
                        Vm::coerce_value_to_type(&mut coerced, &pt, false);
                        if !vm.value_matches_type(&coerced, &pt) {
                            let cname = rcls(vm, &declaring)
                                .map(|c| crate::string::msg_str(&c.name).to_string())
                                .unwrap_or_else(|| class_name.clone());
                            let msg = format!(
                                "Cannot assign {} to property {}::${} of type {}",
                                Vm::value_type_name(&value),
                                cname,
                                prop_name,
                                zend_type_string(&pt)
                            );
                            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                        value = coerced;
                    }
                    let _ = vm.static_prop_assign_typed(
                        declaring.as_slice(),
                        prop_name.as_bytes(),
                        value,
                    );
                } else if args.len() >= 3 {
                    let target = args[1].clone();
                    let value = args[2].clone();
                    // (zpp "oz" of an instance property)
                    if args.len() > 3 {
                        let msg = format!(
                            "ReflectionProperty::setValue() expects exactly 2 arguments, {} given",
                            args.len() - 1
                        );
                        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    if !matches!(target.deref(), Value::Object(_)) {
                        let msg = format!(
                            "ReflectionProperty::setValue(): Argument #1 ($objectOrValue) must be of type object, {} given",
                            Vm::value_type_name(&target.deref())
                        );
                        let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    if let Value::Object(_) = &target {
                        // A Stringable object assigned to a string-typed
                        // property coerces to string (bug #79683).
                        let mut value = value;
                        let prop_type = rprop(vm, &class_lower, prop_name.as_bytes())
                            .and_then(|p| p.property_type.clone());
                        if let Some(pt) = &prop_type {
                            let inner = match pt {
                                crate::opcode::ParamType::Nullable(inner) => inner.as_ref(),
                                other => other,
                            };
                            if matches!(inner, crate::opcode::ParamType::Simple(n) if n.as_slice() == b"string")
                                && matches!(value, Value::Object(_))
                            {
                                let s = vm.value_to_string(&value);
                                if vm.current_exception.is_some() {
                                    return Some(Value::Null);
                                }
                                value = Value::String(s);
                            }
                        }
                        // zend_update_property_ex() in the declaring class's scope.
                        let decl = rprop(vm, &class_lower, prop_name.as_bytes())
                            .map(|p| p.declaring_class.to_ascii_lowercase())
                            .unwrap_or_else(|| class_lower.clone());
                        let _ = run_snippet_in_scope(
                            vm,
                            "$__o->{$__n} = $__v;",
                            &[
                                ("__o", target.clone()),
                                (
                                    "__n",
                                    Value::String(PhpString::from_msg(&prop_name.clone())),
                                ),
                                ("__v", value),
                            ],
                            &decl,
                        );
                    }
                } else {
                    // (an instance property needs the object and the value)
                    let msg = format!(
                        "ReflectionProperty::setValue() expects exactly 2 arguments, {} given",
                        args.len().saturating_sub(1)
                    );
                    let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
                    vm.current_exception = Some(exc);
                }
                Some(Value::Null)
            }
            b"getattributes" => {
                let attrs = rcls(vm, &class_lower)
                    .and_then(|_c| {
                        rprop(vm, &class_lower, prop_name.as_bytes()).map(|p| p.attributes.clone())
                    })
                    .unwrap_or_default();
                let filter_name = args.get(1).and_then(|v| {
                    if matches!(v, Value::Null) {
                        None
                    } else {
                        Some(v.to_php_string().as_bytes().to_vec())
                    }
                });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                Some(create_reflection_attributes(
                    vm,
                    &attrs,
                    filter_name.as_deref(),
                    filter_flags,
                    8,
                ))
            }
            b"getrawvalue" => {
                // getRawValue($obj): read the property without triggering lazy init.
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                // (the object must be an instance of the declaring class)
                if let Value::Object(o) = &target {
                    let ol = o.borrow().class_name.to_ascii_lowercase();
                    let decl = rprop(vm, &class_lower, prop_name.as_bytes())
                        .map(|p| p.declaring_class.to_ascii_lowercase())
                        .unwrap_or_else(|| class_lower.clone());
                    if ol != decl && !vm.class_extends(&ol, &decl) {
                        let exc = vm.create_exception(
                            b"ReflectionException",
                            "Given object is not an instance of the class this property was declared in",
                            vm.current_line,
                        );
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }
                if let Value::Object(o) = &target {
                    if let Some(v) = raw_value_effective_get(vm, &class_lower, o, &prop_name) {
                        return Some(v);
                    }
                }
                if let Some(d) = find_prop_decl(vm, &class_lower, prop_name.as_bytes()) {
                    if d.is_virtual && (d.has_get_hook || d.has_set_hook) {
                        let msg = format!(
                            "Must not read from virtual property {}::${}",
                            class_display(vm, &d.declaring_class),
                            prop_name
                        );
                        let exc = vm.create_exception(b"Error", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }
                // A hookless virtual property of an internal class is served
                // by its read handler (zend_read_property_ex()).
                let internal_virtual = rprop(vm, &class_lower, prop_name.as_bytes())
                    .map(|p| (p.is_virtual, p.declaring_class.to_ascii_lowercase()))
                    .filter(|(v, d)| *v && is_internal_class(vm, d))
                    .map(|(_, d)| d);
                if let Some(decl) = internal_virtual {
                    if matches!(target, Value::Object(_)) {
                        let v = run_snippet_in_scope(
                            vm,
                            "return $__o->{$__n};",
                            &[
                                ("__o", target.clone()),
                                (
                                    "__n",
                                    Value::String(PhpString::from_msg(&prop_name.clone())),
                                ),
                            ],
                            &decl,
                        );
                        return Some(v.unwrap_or(Value::Null));
                    }
                }
                if let Value::Object(target_obj) = &target {
                    // A dynamic property the object lacks: zend_read_property_ex()
                    // warns.
                    if find_prop_decl(vm, &class_lower, prop_name.as_bytes()).is_none()
                        && !target_obj.borrow().has_property(prop_name.as_bytes())
                    {
                        let cname =
                            crate::value::display_class_name(&target_obj.borrow().class_name);
                        vm.emit_warning(&format!("Undefined property: {}::${}", cname, prop_name));
                        return Some(Value::Null);
                    }
                    let ob = target_obj.borrow();
                    Some(ob.get_property(prop_name.as_bytes()))
                } else {
                    Some(Value::Null)
                }
            }
            b"setrawvaluewithoutlazyinitialization" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                let mut value = args.get(2).cloned().unwrap_or(Value::Null);
                let mname = "setRawValueWithoutLazyInitialization";
                let Some(tobj) = reflection_property_object_arg(vm, &target, &class_lower, mname)
                else {
                    return Some(Value::Null);
                };
                let tobj = vm.proxy_redirect(&tobj);
                let pn = prop_name.as_bytes();
                let decl = effective_prop(vm, &class_lower, &tobj, pn);
                if !reflection_property_check_lazy_compatible(vm, &decl, &class_lower, pn, mname) {
                    return Some(Value::Null);
                }
                let decl = decl.unwrap();
                // A Stringable object assigned to a string property is
                // converted first; __toString() may initialize the object.
                if let Some(pt) = decl.property_type.as_ref() {
                    let inner = match pt {
                        crate::opcode::ParamType::Nullable(inner) => inner.as_ref(),
                        other => other,
                    };
                    if matches!(inner, crate::opcode::ParamType::Simple(n) if n.eq_ignore_ascii_case(b"string"))
                        && matches!(&value, Value::Object(o) if vm
                            .classes
                            .get(&o.borrow().class_name.to_ascii_lowercase())
                            .is_some_and(|c| c.get_method(b"__tostring").is_some()))
                    {
                        let s = vm.value_to_string(&value);
                        if vm.current_exception.is_some() {
                            return Some(Value::Null);
                        }
                        value = Value::String(s);
                    }
                }
                let prop_was_lazy = prop_is_lazy(&tobj, pn);
                if prop_was_lazy {
                    set_prop_lazy_flag(&tobj, pn, false);
                }
                // Write with the reflected class as scope: readonly and type
                // checks apply, hooks and lazy initialization don't.
                let decl_disp = class_display(vm, &decl.declaring_class);
                let cur = tobj.borrow().get_property(pn);
                let mut err: Option<(&[u8], String)> = None;
                if decl.is_readonly && !matches!(cur, Value::Undef) {
                    err = Some((
                        b"Error",
                        format!(
                            "Cannot modify readonly property {}::${}",
                            decl_disp, prop_name
                        ),
                    ));
                } else if let Some(pt) = decl.property_type.as_ref() {
                    crate::vm::Vm::coerce_value_to_type(&mut value, pt, false);
                    if !vm.value_matches_type(&value, pt) {
                        err = Some((
                            b"TypeError",
                            format!(
                                "Cannot assign {} to property {}::${} of type {}",
                                crate::vm::Vm::value_type_name(&value),
                                decl_disp,
                                prop_name,
                                vm.param_type_name(pt)
                            ),
                        ));
                    }
                }
                if let Some((cls, m)) = err {
                    let exc = vm.create_exception(cls, &m, vm.current_line);
                    vm.current_exception = Some(exc);
                    // Mark property as lazy again if an exception prevented update
                    if prop_was_lazy
                        && matches!(tobj.borrow().get_property(pn), Value::Undef)
                        && Vm::is_uninit_lazy(&tobj)
                    {
                        set_prop_lazy_flag(&tobj, pn, true);
                    }
                } else {
                    let key = raw_prop_key(vm, &tobj, pn, &decl.declaring_class);
                    tobj.borrow_mut().set_property(key, value);
                }
                // Object becomes non-lazy if this was the last lazy prop
                if prop_was_lazy
                    && !prop_is_lazy(&tobj, pn)
                    && Vm::is_uninit_lazy(&tobj)
                    && vm.lazy_props_remaining(&tobj) == 0
                {
                    vm.lazy_realize(&tobj);
                }
                Some(Value::Null)
            }
            b"islazy" => {
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                let Some(tobj) =
                    reflection_property_object_arg(vm, &target, &class_lower, "isLazy")
                else {
                    return Some(Value::Null);
                };
                let pn = prop_name.as_bytes();
                match find_prop_decl(vm, &class_lower, pn) {
                    Some(d)
                        if !d.is_static
                            && !(d.is_virtual && (d.has_get_hook || d.has_set_hook)) =>
                    {
                        let tobj = vm.proxy_redirect(&tobj);
                        Some(if prop_is_lazy(&tobj, pn) {
                            Value::True
                        } else {
                            Value::False
                        })
                    }
                    _ => Some(Value::False),
                }
            }
            b"isinitialized" => {
                // isInitialized($obj): returns true if the property has been
                // assigned a value (or has a default) on the object.
                if opt_object_arg_error(vm, "ReflectionProperty::isInitialized", args.get(1)) {
                    return Some(Value::Null);
                }
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                if is_static {
                    let initialized = vm
                        .static_prop_raw(&class_lower, prop_name.as_bytes())
                        .is_some_and(|v| !matches!(v, Value::Undef));
                    Some(if initialized {
                        Value::True
                    } else {
                        Value::False
                    })
                } else if let Value::Object(target_obj) = &target {
                    let pn = prop_name.as_bytes();
                    let decl = find_prop_decl(vm, &class_lower, pn)
                        .map(|d| d.declaring_class.to_ascii_lowercase())
                        .unwrap_or_else(|| class_lower.clone());
                    let ol = target_obj.borrow().class_name.to_ascii_lowercase();
                    if ol != decl && !vm.class_extends(&ol, &decl) {
                        let exc = vm.create_exception(
                            b"ReflectionException",
                            "Given object is not an instance of the class this property was declared in",
                            vm.current_line,
                        );
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    // Hooked properties cannot be unset: virtual and untyped
                    // backed ones always exist.
                    let hooked = find_prop_decl(vm, &class_lower, pn)
                        .filter(|d| d.has_get_hook || d.has_set_hook)
                        .map(|d| d.is_virtual || d.property_type.is_none());
                    let ob = target_obj.borrow();
                    // (SensitiveParameterValue keeps $value natively)
                    let pn = if pn == b"value"
                        && ob
                            .class_name
                            .eq_ignore_ascii_case(b"SensitiveParameterValue")
                    {
                        b"__sensitive_value".as_slice()
                    } else {
                        pn
                    };
                    let has = hooked == Some(true)
                        || (ob.has_property(pn) && !matches!(ob.get_property(pn), Value::Undef));
                    Some(if has { Value::True } else { Value::False })
                } else {
                    let exc = vm.create_exception(
                        b"TypeError",
                        "ReflectionProperty::isInitialized(): Argument #1 ($object) must be provided for instance properties",
                        vm.current_line,
                    );
                    vm.current_exception = Some(exc);
                    Some(Value::Null)
                }
            }
            b"setrawvalue" => {
                // setRawValue($obj, $value): writes the property bypassing
                // hooks and type checks (best-effort in our VM).
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                let value = args.get(2).cloned().unwrap_or(Value::Null);
                if let Value::Object(o) = &target {
                    if raw_value_effective_set(vm, &class_lower, o, &prop_name, &value) {
                        return Some(Value::Null);
                    }
                }
                if let Some(d) = find_prop_decl(vm, &class_lower, prop_name.as_bytes()) {
                    // reflection_property_set_raw_value(): without a set
                    // hook on the effective property (the object class's,
                    // reflection_property_get_effective_prop()),
                    // write_property() in the reflected class's scope.
                    // (a set hook may be inherited by a redeclaration)
                    let mut set_m = b"__property_set_".to_vec();
                    set_m.extend(prop_name.bytes().map(|b| b.to_ascii_lowercase()));
                    let has_set = |vm: &Vm, cl: &[u8]| {
                        rprop(vm, cl, prop_name.as_bytes())
                            .filter(|p| !p.is_static)
                            .is_some_and(|p| p.has_set_hook)
                            || rcls(vm, cl).is_some_and(|c| c.get_method(&set_m).is_some())
                    };
                    let effective_set_hook = has_set(vm, &class_lower)
                        || match &target {
                            Value::Object(o) => {
                                let ocl = o.borrow().class_name.to_ascii_lowercase();
                                has_set(vm, &ocl)
                            }
                            _ => false,
                        };
                    if !is_static
                        && d.visibility != Visibility::Private
                        && !d.has_set_hook
                        && !effective_set_hook
                        && matches!(target, Value::Object(_))
                    {
                        let _ = run_snippet_in_scope(
                            vm,
                            "$__o->{$__n} = $__v;",
                            &[
                                ("__o", target.clone()),
                                (
                                    "__n",
                                    Value::String(PhpString::from_msg(&prop_name.clone())),
                                ),
                                ("__v", value),
                            ],
                            &class_lower,
                        );
                        return Some(Value::Null);
                    }
                    if d.is_virtual && (d.has_get_hook || d.has_set_hook) {
                        let msg = format!(
                            "Must not write to virtual property {}::${}",
                            class_display(vm, &d.declaring_class),
                            prop_name
                        );
                        let exc = vm.create_exception(b"Error", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                }
                if is_static {
                    vm.static_prop_put_raw(&class_lower, prop_name.as_bytes(), value);
                } else if let Value::Object(target_obj) = &target {
                    // Creating a dynamic property (zend_update_property_ex()).
                    if find_prop_decl(vm, &class_lower, prop_name.as_bytes()).is_none()
                        && !target_obj.borrow().has_property(prop_name.as_bytes())
                    {
                        let cl = target_obj.borrow().class_name.to_ascii_lowercase();
                        let allows = vm
                            .classes
                            .get(&cl)
                            .is_some_and(|c| c.allow_dynamic_properties)
                            || cl == b"stdclass";
                        if !allows {
                            let cname =
                                crate::value::display_class_name(&target_obj.borrow().class_name);
                            vm.emit_deprecated(&format!(
                                "Creation of dynamic property {}::${} is deprecated",
                                cname, prop_name
                            ));
                        }
                    }
                    let mut ob = target_obj.borrow_mut();
                    ob.set_property(prop_name.as_bytes().to_vec(), value);
                }
                Some(Value::Null)
            }
            b"gethook" | b"hashook" => {
                // Argument: a PropertyHookType case.
                let kind = match args.get(1) {
                    Some(Value::Object(o)) => {
                        let ob = o.borrow();
                        let v = ob
                            .get_property(b"value")
                            .to_php_string()
                            .to_msg_string()
                            .to_ascii_lowercase();
                        if v.is_empty() {
                            ob.get_property(b"name")
                                .to_php_string()
                                .to_msg_string()
                                .to_ascii_lowercase()
                        } else {
                            v
                        }
                    }
                    Some(other) => other.to_php_string().to_msg_string().to_ascii_lowercase(),
                    None => String::new(),
                };
                let m = if kind == "get" || kind == "set" {
                    reflection_property_hook_method(vm, &class_lower, prop_name.as_bytes(), &kind)
                } else {
                    None
                };
                if method == b"hashook" {
                    Some(if m.is_some() {
                        Value::True
                    } else {
                        Value::False
                    })
                } else {
                    Some(m.unwrap_or(Value::Null))
                }
            }
            b"skiplazyinitialization" => {
                // Mark property as non-lazy, and initialize to default value.
                let target = args.get(1).cloned().unwrap_or(Value::Null);
                let mname = "skipLazyInitialization";
                let Some(tobj) = reflection_property_object_arg(vm, &target, &class_lower, mname)
                else {
                    return Some(Value::Null);
                };
                let pn = prop_name.as_bytes();
                let decl = find_prop_decl(vm, &class_lower, pn);
                if !reflection_property_check_lazy_compatible(vm, &decl, &class_lower, pn, mname) {
                    return Some(Value::Null);
                }
                let decl = decl.unwrap();
                let tobj = vm.proxy_redirect(&tobj);
                if !prop_is_lazy(&tobj, pn) {
                    // No effect on non-lazy properties
                    return Some(Value::Null);
                }
                let default = vm.resolve_deferred_value(&decl.default);
                set_prop_lazy_flag(&tobj, pn, false);
                tobj.borrow_mut().set_property(pn.to_vec(), default);
                if vm.lazy_props_remaining(&tobj) == 0 {
                    vm.lazy_realize(&tobj);
                }
                Some(Value::Null)
            }
            _ => None,
        }
    } else {
        None
    }
}

/// Extract the literal default value for a given CV index by scanning the
/// function prologue opcodes the compiler emits for parameter defaults.
///
/// Pattern emitted by the compiler for each defaulted parameter:
///   Identical CV(cv), Const(Undef) -> Tmp(t)
///   JmpZ Tmp(t), JmpTarget(after)
///   Assign CV(cv), <default_val>
///
/// We look for an `Assign` targeting the requested CV whose op2 is a
/// `Const(idx)` and return that literal.  If the default is a constant
/// expression or `new ExprName()` initializer it may not be a simple
/// constant operand — in that case we currently give up and return None.
/// Format a Value for display in Reflection __toString output. Matches PHP's
/// format used in `Function [ ... ]` and `Method [ ... ]` representations:
///   string -> 'foo' (single quotes)
///   bool -> true|false (lowercase)
///   null -> NULL
///   array -> [a, b, c]
///   numeric -> as-is
fn format_reflection_default(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::True => "true".to_string(),
        Value::False => "false".to_string(),
        Value::Long(n) => n.to_string(),
        Value::Double(f) => crate::value::format_php_float(*f),
        Value::String(s) => {
            let raw = s.to_msg_string();
            format!("'{}'", raw.replace('\\', "\\\\").replace('\'', "\\'"))
        }
        Value::Array(a) => {
            let arr = a.borrow();
            let mut parts: Vec<String> = Vec::new();
            let mut last_key: i64 = -1;
            let mut all_packed = true;
            for (k, _) in arr.iter() {
                match k {
                    crate::array::ArrayKey::Int(n) => {
                        if *n == last_key + 1 {
                            last_key = *n;
                        } else {
                            all_packed = false;
                            break;
                        }
                    }
                    _ => {
                        all_packed = false;
                        break;
                    }
                }
            }
            for (k, v) in arr.iter() {
                if all_packed {
                    parts.push(format_reflection_default(v));
                } else {
                    let key_s = match k {
                        crate::array::ArrayKey::Int(n) => n.to_string(),
                        crate::array::ArrayKey::String(s) => {
                            format!("'{}'", crate::string::msg_str(s.as_bytes()))
                        }
                    };
                    parts.push(format!("{} => {}", key_s, format_reflection_default(v)));
                }
            }
            format!("[{}]", parts.join(", "))
        }
        _ => "?".to_string(),
    }
}

pub(crate) fn extract_param_default(
    op_array: &crate::opcode::OpArray,
    cv_idx: u32,
) -> Option<Value> {
    use crate::opcode::{OpCode, OperandType};
    // Scan only the first portion of the function (the default-value prelude
    // sits before any real body opcode that reads/writes other CVs in non-
    // trivial ways). We stop once we encounter an opcode outside the prelude
    // pattern that doesn't touch parameter defaults.
    for op in op_array.ops.iter() {
        if op.opcode == OpCode::Assign {
            if let OperandType::Cv(target_cv) = op.op1 {
                if target_cv == cv_idx {
                    if let OperandType::Const(const_idx) = op.op2 {
                        if let Some(v) = op_array.literals.get(const_idx as usize) {
                            return Some(v.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Evaluate the default value of the parameter at `cv_idx` by running its
/// compiled prelude (`if ($p === UNDEF) { <expr>; $p = <value>; }`) on its
/// own, in the scope of `scope` (lowercase class name), like
/// zend_get_default_from_internal_arg_info()/zval_update_constant_ex() do for
/// the RECV_INIT constant AST. None if no prelude was found; on an exception
/// (e.g. an undefined constant) the exception is left pending.
pub(crate) fn eval_param_default_prelude(
    vm: &mut Vm,
    op_array: &crate::opcode::OpArray,
    cv_idx: u32,
    scope: Option<&[u8]>,
) -> Option<Value> {
    use crate::opcode::{Op, OpCode, OperandType};
    let ops = &op_array.ops;
    let is_undef_lit = |o: &OperandType| match o {
        OperandType::Const(l) => matches!(op_array.literals.get(*l as usize), Some(Value::Undef)),
        _ => false,
    };
    for i in 0..ops.len().saturating_sub(1) {
        let op = &ops[i];
        if op.opcode != OpCode::Identical
            || op.op1 != OperandType::Cv(cv_idx)
            || !is_undef_lit(&op.op2)
        {
            continue;
        }
        let jz = &ops[i + 1];
        if jz.opcode != OpCode::JmpZ || jz.op1 != op.result {
            continue;
        }
        let after = match jz.op2 {
            OperandType::JmpTarget(t) => t as usize,
            _ => continue,
        };
        let start = i + 2;
        if after <= start || after > ops.len() {
            continue;
        }
        // The Assign is the last one to the CV before `after` (a
        // RecvDefaultCheck may follow it).
        let Some(assign_at) = (start..after)
            .rev()
            .find(|&j| ops[j].opcode == OpCode::Assign && ops[j].op1 == OperandType::Cv(cv_idx))
        else {
            continue;
        };
        let last = &ops[assign_at];
        let rebase = |o: OperandType| match o {
            OperandType::JmpTarget(t) if t as usize >= start => {
                OperandType::JmpTarget(t - start as u32)
            }
            o => o,
        };
        let mut sub = op_array.clone();
        sub.ops = ops[start..assign_at]
            .iter()
            .map(|o| Op {
                opcode: o.opcode,
                op1: rebase(o.op1),
                op2: rebase(o.op2),
                result: rebase(o.result),
                line: o.line,
            })
            .collect();
        let line = last.line;
        sub.ops.push(Op {
            opcode: OpCode::Return,
            op1: last.op2,
            op2: OperandType::Unused,
            result: OperandType::Unused,
            line,
        });
        sub.is_generator = false;
        sub.return_type = None;
        sub.returns_by_ref = false;
        let cvs = vec![Value::Undef; sub.cv_names.len()];
        let saved_line = vm.current_line;
        let pushed = if let Some(s) = scope {
            vm.called_class_stack.push(s.to_vec());
            vm.push_class_scope(s.to_vec());
            true
        } else {
            false
        };
        let r = vm.execute_op_array_pub(&sub, cvs);
        if pushed {
            vm.called_class_stack.pop();
            vm.pop_class_scope();
        }
        vm.current_line = saved_line;
        if vm.current_exception.is_some() {
            return Some(Value::Null);
        }
        return match r {
            Ok(Value::Reference(r)) => Some(r.borrow().clone()),
            Ok(Value::Undef) => Some(Value::Null),
            Ok(v) => Some(v),
            Err(e) => {
                let exc = vm.create_exception(b"Error", &e.message, e.line);
                vm.current_exception = Some(exc);
                Some(Value::Null)
            }
        };
    }
    None
}

/// The value a not-yet-run `static $v = <constant expression>;` reports
/// (php-src evaluates constant-expression initializers at compile time):
/// the guarded initializer ops are run on their own when they only build a
/// constant value. None for other initializers (they stay uninitialized).
fn eval_static_const_init(
    vm: &mut Vm,
    op_array: &crate::opcode::OpArray,
    init: &crate::opcode::Op,
) -> Option<Value> {
    use crate::opcode::{Op, OpCode, OperandType};
    let ops = &op_array.ops;
    let at = ops.iter().position(|o| std::ptr::eq(o, init))?;
    let pure_op = |o: &Op| {
        !matches!(o.op1, OperandType::Cv(_))
            && !matches!(o.op2, OperandType::Cv(_))
            && matches!(
                o.opcode,
                OpCode::ArrayNew
                    | OpCode::ArrayAppend
                    | OpCode::ArraySet
                    | OpCode::StaticPropGet
                    | OpCode::ConstLookup
                    | OpCode::QmAssign
                    | OpCode::Add
                    | OpCode::Sub
                    | OpCode::Mul
                    | OpCode::Div
                    | OpCode::Mod
                    | OpCode::Pow
                    | OpCode::Concat
                    | OpCode::Negate
                    | OpCode::UnaryPlus
                    | OpCode::BitwiseAnd
                    | OpCode::BitwiseOr
                    | OpCode::BitwiseXor
                    | OpCode::BitwiseNot
                    | OpCode::ShiftLeft
                    | OpCode::ShiftRight
                    | OpCode::BooleanNot
                    | OpCode::FreeTmps
                    | OpCode::Nop
            )
    };
    // A non-constant initializer is guarded by StaticVarInitOrJmp; a
    // constant one is compiled right before the StaticVarInit.
    let start = match (0..at).rev().find(|&j| {
        ops[j].opcode == OpCode::StaticVarInitOrJmp
            && ops[j].op1 == init.op1
            && ops[j].result == init.result
    }) {
        Some(g) => g + 1,
        None => {
            let mut s = at;
            while s > 0 && pure_op(&ops[s - 1]) {
                s -= 1;
            }
            s
        }
    };
    let body = &ops[start..at];
    if !body.iter().all(pure_op) {
        return None;
    }
    let mut sub = op_array.clone();
    sub.ops = body.to_vec();
    sub.ops.push(Op {
        opcode: OpCode::Return,
        op1: init.op2,
        op2: OperandType::Unused,
        result: OperandType::Unused,
        line: init.line,
    });
    sub.is_generator = false;
    sub.return_type = None;
    sub.returns_by_ref = false;
    let cvs = vec![Value::Undef; sub.cv_names.len()];
    let scope = op_array.scope_class.clone();
    if let Some(s) = &scope {
        vm.called_class_stack.push(s.clone());
        vm.push_class_scope(s.clone());
    }
    let saved_line = vm.current_line;
    let r = vm.execute_op_array_pub(&sub, cvs);
    vm.current_line = saved_line;
    if scope.is_some() {
        vm.called_class_stack.pop();
        vm.pop_class_scope();
    }
    if vm.current_exception.is_some() {
        vm.current_exception = None;
        return None;
    }
    r.ok().map(|v| v.deref())
}

/// Identify the kind of default value for the parameter at `cv_idx`.
/// Returns Some("global:<NAME>") for a global constant default
/// (emitted by ConstLookup), Some("class:<Class>::<CONST>") for a class
/// constant default (StaticPropGet with class+name), or None.
/// A default naming a global constant the compiler folded to its value
/// (PHP_INT_MAX, E_ALL, NAN...): the AST still is a ZEND_AST_CONSTANT.
#[inline(never)]
pub(crate) fn folded_default_const_name(
    op_array: &crate::opcode::OpArray,
    cv_idx: usize,
) -> Option<String> {
    let r = op_array.param_default_repr.get(cv_idx)?.as_ref()?;
    let r = r.trim_start_matches('\\');
    let first = r.chars().next()?;
    if !(first.is_ascii_alphabetic() || first == '_' || !first.is_ascii())
        || !r
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '\\' || !c.is_ascii())
    {
        return None;
    }
    if matches!(r.to_ascii_lowercase().as_str(), "true" | "false" | "null") {
        return None;
    }
    Some(r.to_string())
}

pub(crate) fn extract_param_default_constant(
    op_array: &crate::opcode::OpArray,
    cv_idx: u32,
) -> Option<String> {
    use crate::opcode::{OpCode, OperandType};
    // Find the Assign for this CV; the op2 should be a Tmp(t) that was
    // produced by the immediately preceding ConstLookup or StaticPropGet.
    for (i, op) in op_array.ops.iter().enumerate() {
        if op.opcode != OpCode::Assign {
            continue;
        }
        if let OperandType::Cv(target_cv) = op.op1 {
            if target_cv != cv_idx {
                continue;
            }
            let src_tmp = match op.op2 {
                OperandType::Tmp(t) => t,
                _ => continue,
            };
            // Search backwards for the op producing this Tmp.
            for j in (0..i).rev() {
                let prev = &op_array.ops[j];
                let writes_tmp = match prev.result {
                    OperandType::Tmp(t) => t == src_tmp,
                    _ => false,
                };
                if !writes_tmp {
                    continue;
                }
                match prev.opcode {
                    OpCode::ConstLookup => {
                        if let OperandType::Const(idx) = prev.op1 {
                            if let Some(Value::String(s)) = op_array.literals.get(idx as usize) {
                                return Some(format!("global:{}", s.to_msg_string()));
                            }
                        }
                    }
                    OpCode::StaticPropGet => {
                        if let (OperandType::Const(ci), OperandType::Const(ni)) =
                            (prev.op1, prev.op2)
                        {
                            let cls = match op_array.literals.get(ci as usize) {
                                Some(Value::String(s)) => s.to_msg_string(),
                                _ => return None,
                            };
                            let nm = match op_array.literals.get(ni as usize) {
                                Some(Value::String(s)) => s.to_msg_string(),
                                _ => return None,
                            };
                            return Some(format!("class:{}::{}", cls, nm));
                        }
                    }
                    _ => {}
                }
                break;
            }
            return None;
        }
    }
    None
}

/// Resolve the underlying OpArray for a ReflectionParameter by checking either
/// the `__reflection_func` property (free function/closure) or the class+method
/// properties (method parameters).
fn reflection_parameter_op_array(
    vm: &Vm,
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<crate::opcode::OpArray> {
    let ob = obj.borrow();
    let func_target = ob.get_property(b"__reflection_func").to_php_string();
    let func_bytes = func_target.as_bytes().to_vec();
    drop(ob);
    if !func_bytes.is_empty() {
        if let Some(oa) = rfunc(vm, &func_bytes) {
            return Some(oa.clone());
        }
    }
    // Fall back to class+method lookup for method parameters.
    let ob = obj.borrow();
    let class_name = ob.get_property(b"__reflection_param_class").to_php_string();
    let method_name = ob
        .get_property(b"__reflection_param_method")
        .to_php_string();
    drop(ob);
    let class_bytes = class_name.as_bytes().to_vec();
    let method_bytes = method_name.as_bytes().to_vec();
    if !class_bytes.is_empty() && !method_bytes.is_empty() {
        let class_lower: Vec<u8> = class_bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
        let method_lower: Vec<u8> = method_bytes
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        // (rmethod(): also an enum's implicit cases()/from()/tryFrom())
        if let Some(m) = rmethod(vm, &class_lower, &method_lower) {
            return Some(
                reflect_sig_op_array(&m.op_array).unwrap_or_else(|| (*m.op_array).clone()),
            );
        }
    }
    None
}

/// ReflectionParameter no-arg method dispatch
pub fn reflection_parameter_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    // (the deprecated methods check their arguments: reflection_parameter_docall())
    if deprecated_method(b"reflectionparameter", method).is_some() {
        return None;
    }
    reflection_parameter_method_body(vm, method, obj)
}

/// Deprecated methods of the Reflection classes: (class, method, message).
const DEPRECATED_METHODS: &[(&[u8], &[u8], &str)] = &[
    (
        b"reflectionmethod",
        b"setaccessible",
        "Method ReflectionMethod::setAccessible() is deprecated since 8.5, as it has no effect since PHP 8.1",
    ),
    (
        b"reflectionproperty",
        b"setaccessible",
        "Method ReflectionProperty::setAccessible() is deprecated since 8.5, as it has no effect since PHP 8.1",
    ),
    (
        b"reflectionfunction",
        b"isdisabled",
        "Method ReflectionFunction::isDisabled() is deprecated since 8.0, as ReflectionFunction can no longer be constructed for disabled functions",
    ),
    (
        b"reflectionparameter",
        b"getclass",
        "Method ReflectionParameter::getClass() is deprecated since 8.0, use ReflectionParameter::getType() instead",
    ),
    (
        b"reflectionparameter",
        b"isarray",
        "Method ReflectionParameter::isArray() is deprecated since 8.0, use ReflectionParameter::getType() instead",
    ),
    (
        b"reflectionparameter",
        b"iscallable",
        "Method ReflectionParameter::isCallable() is deprecated since 8.0, use ReflectionParameter::getType() instead",
    ),
];

fn deprecated_method(class_lower: &[u8], method_lower: &[u8]) -> Option<&'static str> {
    DEPRECATED_METHODS
        .iter()
        .find(|(c, m, _)| *c == class_lower && *m == method_lower)
        .map(|(_, _, msg)| *msg)
}

/// A call to a deprecated Reflection method (`args`: $this first): the
/// deprecation (from the call), then the argument checks. None when the
/// method is not deprecated; Some(false) to run it, Some(true) when the call
/// failed (exception set).
fn deprecated_method_call(
    vm: &mut Vm,
    class_lower: &[u8],
    method_lower: &[u8],
    args: &[Value],
) -> Option<bool> {
    let msg = deprecated_method(class_lower, method_lower)?;
    // "Method Class::name() is deprecated ...": the name as declared
    let name = msg[7..].split_once("::")?.1.split_once('(')?.0;
    if vm.emit_method_deprecation(msg, name) {
        return Some(true);
    }
    Some(
        vm.zpp_method_self_check(class_lower, method_lower, args.get(1..).unwrap_or(&[]))
            .is_err(),
    )
}

fn reflection_parameter_method_body(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let func_target = ob.get_property(b"__reflection_func").to_php_string();
    let param_idx = ob.get_property(b"__reflection_param_idx").to_long() as usize;
    let cv_offset = ob.get_property(b"__reflection_param_cv_offset").to_long() as usize;
    drop(ob);
    let resolved_op_array = reflection_parameter_op_array(vm, obj);
    // CV index for type-info lookups (cv_names / param_types arrays). For free
    // functions and static methods this equals `param_idx`; for non-static
    // methods we shift past the synthetic `$this` slot at index 0.
    let type_idx = param_idx + cv_offset;

    // Internal trampolines with userland arginfo (Closure::__invoke).
    if obj.borrow().has_property(b"__reflection_param_trampoline") {
        match method {
            b"hasdefaultvalue" | b"isdefaultvalueavailable" => return Some(Value::False),
            b"getdefaultvalue" | b"isdefaultvalueconstant" | b"getdefaultvalueconstantname" => {
                let exc = vm.create_exception(
                    b"ReflectionException",
                    "Internal error: Failed to retrieve the default value",
                    vm.current_line,
                );
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            b"__tostring" => {
                if let Some(op) = resolved_op_array.as_ref() {
                    let is_variadic = op
                        .variadic_param
                        .map(|v| v as usize == type_idx)
                        .unwrap_or(false);
                    let required = !is_variadic && param_idx < op.required_param_count as usize;
                    return Some(Value::String(PhpString::from_msg(
                        &parameter_string_user_arginfo(op, param_idx, cv_offset, required),
                    )));
                }
            }
            _ => {}
        }
    }
    // Internal functions: defaults come from the stub's arginfo text.
    let internal_default: Option<Option<&'static str>> = resolved_op_array
        .as_ref()
        .and_then(|op| op.internal_sig)
        .map(|f| f.args.get(param_idx).and_then(|a| a.default));
    if let Some(def) = internal_default {
        match method {
            b"hasdefaultvalue" | b"isdefaultvalueavailable" => {
                return Some(if def.is_some() {
                    Value::True
                } else {
                    Value::False
                });
            }
            b"getdefaultvalue" | b"isdefaultvalueconstant" | b"getdefaultvalueconstantname" => {
                let def = match def {
                    Some(d) => d,
                    None => {
                        let exc = vm.create_exception(
                            b"ReflectionException",
                            "Internal error: Failed to retrieve the default value",
                            vm.current_line,
                        );
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                };
                let const_name = internal_default_constant_name(def);
                return Some(match method {
                    b"isdefaultvalueconstant" => {
                        if const_name.is_some() {
                            Value::True
                        } else {
                            Value::False
                        }
                    }
                    b"getdefaultvalueconstantname" => match const_name {
                        Some(n) => Value::String(PhpString::from_msg(&n)),
                        None => Value::Null,
                    },
                    _ => {
                        let scope = resolved_op_array
                            .as_ref()
                            .and_then(|op| op.scope_class.clone());
                        eval_internal_default(vm, def, scope.as_deref()).unwrap_or(Value::Null)
                    }
                });
            }
            _ => {}
        }
    }

    match method {
        b"getname" => {
            let ob = obj.borrow();
            Some(ob.get_property(b"name"))
        }
        b"getposition" => Some(Value::Long(param_idx as i64)),
        b"isoptional" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                Some(if param_idx >= op_array.required_param_count as usize {
                    Value::True
                } else {
                    Value::False
                })
            } else {
                Some(Value::False)
            }
        }
        b"hasdefaultvalue" | b"isdefaultvalueavailable" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                // (a user parameter has one when it declares one and is
                // optional: a variadic has none, nor a defaulted parameter
                // before a required one)
                let has = if op_array.internal_sig.is_none() {
                    op_array
                        .param_has_default
                        .get(type_idx)
                        .copied()
                        .unwrap_or(false)
                        && param_idx >= op_array.required_param_count as usize
                } else {
                    param_idx >= op_array.required_param_count as usize
                };
                Some(if has { Value::True } else { Value::False })
            } else {
                Some(Value::False)
            }
        }
        b"getdefaultvalue" | b"isdefaultvalueconstant" | b"getdefaultvalueconstantname"
            if resolved_op_array
                .as_ref()
                .map(|op| {
                    op.internal_sig.is_none()
                        && !op.param_has_default.get(type_idx).copied().unwrap_or(false)
                })
                .unwrap_or(false) =>
        {
            let exc = vm.create_exception(
                b"ReflectionException",
                "Internal error: Failed to retrieve the default value",
                vm.current_line,
            );
            vm.current_exception = Some(exc);
            Some(Value::Null)
        }
        b"getdefaultvalue" => {
            // Try to extract default value from compiled opcode prelude.
            if let Some(op_array) = resolved_op_array.as_ref() {
                if param_idx >= op_array.required_param_count as usize {
                    if let Some(v) = extract_param_default(op_array, type_idx as u32) {
                        return Some(v);
                    }
                    // If the default is a constant reference, try to resolve it.
                    if let Some(name) = extract_param_default_constant(op_array, type_idx as u32) {
                        if let Some(rest) = name.strip_prefix("class:") {
                            // class:<ClassName>::<CONST>
                            if let Some(idx) = rest.find("::") {
                                let cls = &rest[..idx];
                                let const_name = &rest[idx + 2..];
                                // Handle self/static/parent
                                let resolved_class: Vec<u8> = match cls {
                                    "self" | "static" => {
                                        // Use declaring class of the param if known
                                        let ob = obj.borrow();
                                        let pc = ob
                                            .get_property(b"__reflection_param_class")
                                            .to_php_string();
                                        drop(ob);
                                        if pc.as_bytes().is_empty() {
                                            cls.as_bytes().to_vec()
                                        } else {
                                            pc.as_bytes().to_vec()
                                        }
                                    }
                                    "parent" => {
                                        let ob = obj.borrow();
                                        let pc = ob
                                            .get_property(b"__reflection_param_class")
                                            .to_php_string();
                                        drop(ob);
                                        let pcl: Vec<u8> = pc
                                            .as_bytes()
                                            .iter()
                                            .map(|b| b.to_ascii_lowercase())
                                            .collect();
                                        rcls(vm, &pcl)
                                            .and_then(|c| c.parent.clone())
                                            .unwrap_or_else(|| cls.as_bytes().to_vec())
                                    }
                                    _ => cls.as_bytes().to_vec(),
                                };
                                let cl: Vec<u8> = resolved_class
                                    .iter()
                                    .map(|b| b.to_ascii_lowercase())
                                    .collect();
                                // Walk class hierarchy for constant
                                let mut current = Some(cl);
                                while let Some(cl) = current {
                                    if let Some(ce) = rcls(vm, &cl) {
                                        if ce.constants.contains_key(const_name.as_bytes()) {
                                            // (enum cases and constant
                                            // expressions are evaluated)
                                            return Some(
                                                resolve_class_constant(
                                                    vm,
                                                    &cl,
                                                    const_name.as_bytes(),
                                                )
                                                .unwrap_or(Value::Null),
                                            );
                                        }
                                        current = ce.parent.as_ref().map(|p| {
                                            p.iter().map(|b| b.to_ascii_lowercase()).collect()
                                        });
                                    } else {
                                        break;
                                    }
                                }
                            }
                        } else if let Some(rest) = name.strip_prefix("global:") {
                            let const_name = rest.trim_start_matches('\\');
                            if let Some(v) = vm.constants.get(const_name.as_bytes()) {
                                return Some(v.clone());
                            }
                            // Case-insensitive built-ins
                            let lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&const_name)
                                .iter()
                                .map(|b| b.to_ascii_lowercase())
                                .collect();
                            match lower.as_slice() {
                                b"true" => return Some(Value::True),
                                b"false" => return Some(Value::False),
                                b"null" => return Some(Value::Null),
                                _ => {}
                            }
                        }
                    }
                    // Any other constant expression: evaluate its prelude.
                    let scope = op_array.scope_class.clone().or_else(|| {
                        let pc = obj
                            .borrow()
                            .get_property(b"__reflection_param_class")
                            .to_php_string();
                        (!pc.as_bytes().is_empty()).then(|| pc.as_bytes().to_ascii_lowercase())
                    });
                    if let Some(v) =
                        eval_param_default_prelude(vm, op_array, type_idx as u32, scope.as_deref())
                    {
                        return Some(v);
                    }
                }
            }
            Some(Value::Null)
        }
        b"allowsnull" => {
            // No type hint -> allows null (PHP default behavior).
            // Otherwise: depends on whether the type permits null.
            if let Some(op_array) = resolved_op_array.as_ref() {
                if type_idx < op_array.param_types.len() {
                    if let Some(pti) = &op_array.param_types[type_idx] {
                        return Some(if param_type_allows_null(&pti.param_type) {
                            Value::True
                        } else {
                            Value::False
                        });
                    }
                }
            }
            Some(Value::True)
        }
        b"isvariadic" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                if let Some(variadic_idx) = op_array.variadic_param {
                    Some(if type_idx == variadic_idx as usize {
                        Value::True
                    } else {
                        Value::False
                    })
                } else {
                    Some(Value::False)
                }
            } else {
                Some(Value::False)
            }
        }
        b"ispassedbyreference" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                if type_idx < op_array.by_ref_params.len() {
                    return Some(if op_array.by_ref_params[type_idx] {
                        Value::True
                    } else {
                        Value::False
                    });
                }
            }
            Some(Value::False)
        }
        b"hastype" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                let has = type_idx < op_array.param_types.len()
                    && op_array.param_types[type_idx].is_some();
                Some(if has { Value::True } else { Value::False })
            } else {
                Some(Value::False)
            }
        }
        b"gettype" => {
            let param_type = resolved_op_array.as_ref().and_then(|op_array| {
                if type_idx < op_array.param_types.len() {
                    op_array.param_types[type_idx]
                        .as_ref()
                        .map(|pti| pti.param_type.clone())
                } else {
                    None
                }
            });
            // Derive a scope class for self/parent resolution from the
            // parameter's declaring class (set when reflecting a method).
            let scope: Option<Vec<u8>> = {
                let ob = obj.borrow();
                let class_lower = ob.get_property(b"__reflection_param_class").to_php_string();
                drop(ob);
                let cb = class_lower.as_bytes();
                if cb.is_empty() {
                    None
                } else {
                    let cl: Vec<u8> = cb.iter().map(|b| b.to_ascii_lowercase()).collect();
                    Some(
                        rcls(vm, &cl)
                            .map(|c| c.name.clone())
                            .unwrap_or_else(|| cb.to_vec()),
                    )
                }
            };
            if let Some(pt) = param_type {
                Some(create_reflection_type_scoped(vm, &pt, scope.as_deref()))
            } else {
                Some(Value::Null)
            }
        }
        b"getdeclaringfunction" => {
            // Return a ReflectionFunction/ReflectionMethod for the declaring
            // function. If the parameter belongs to a method (class+method
            // were stored when the parameter was created), construct a
            // ReflectionMethod instead.
            let ob = obj.borrow();
            let class_opt = ob.get_property(b"__reflection_param_class").to_php_string();
            let method_opt = ob
                .get_property(b"__reflection_param_method")
                .to_php_string();
            // For functions, __reflection_func has the lowercase function name.
            let func_lower = ob.get_property(b"__reflection_func").to_php_string();
            drop(ob);

            let class_bytes = class_opt.as_bytes();
            let method_bytes = method_opt.as_bytes();
            if !class_bytes.is_empty() && !method_bytes.is_empty() {
                // Build a ReflectionMethod
                let class_str = crate::string::msg_str(class_bytes).to_string();
                // (the method's name as declared)
                let method_str = rmethod(vm, class_bytes, method_bytes)
                    .map(|m| crate::string::msg_str(&m.name).to_string())
                    .unwrap_or_else(|| crate::string::msg_str(method_bytes).to_string());
                let canonical_class = rcls(vm, class_bytes)
                    .map(|c| crate::string::msg_str(&c.name).to_string())
                    .unwrap_or_else(|| class_str.clone());
                let obj_id = vm.alloc_object_id();
                let mut rm_obj = PhpObject::new(b"ReflectionMethod".to_vec(), obj_id);
                rm_obj.set_property(
                    b"name".to_vec(),
                    Value::String(PhpString::from_msg(&method_str.clone())),
                );
                rm_obj.set_property(
                    b"class".to_vec(),
                    Value::String(PhpString::from_msg(&canonical_class)),
                );
                rm_obj.set_property(
                    b"__reflection_class".to_vec(),
                    Value::String(PhpString::from_vec(class_bytes.to_vec())),
                );
                rm_obj.set_property(
                    b"__reflection_method".to_vec(),
                    Value::String(PhpString::from_vec(method_bytes.to_vec())),
                );
                return Some(Value::Object(Rc::new(RefCell::new(rm_obj))));
            }

            // A closure's parameter: Closure::__invoke for the trampoline,
            // a method of the scope for a closure bound to a class, else
            // a function (reflection_method_factory / function_factory).
            if crate::closure::is_closure_fn_name(func_target.as_bytes()) {
                let trampoline = obj.borrow().has_property(b"__reflection_param_trampoline");
                let scope = vm
                    .user_functions
                    .get(func_target.as_bytes().to_ascii_lowercase().as_slice())
                    .map(|op| (op.scope_class.clone(), op.display_name.clone()));
                let target = if trampoline {
                    Some((b"Closure".to_vec(), b"__invoke".to_vec()))
                } else if let Some((Some(sc), display)) = scope {
                    let cname = rcls(vm, &sc.to_ascii_lowercase())
                        .map(|c| c.name.clone())
                        .unwrap_or(sc);
                    Some((cname, display))
                } else {
                    None
                };
                if let Some((class, name)) = target {
                    let obj_id = vm.alloc_object_id();
                    let mut rm = PhpObject::new(b"ReflectionMethod".to_vec(), obj_id);
                    rm.set_property(b"name".to_vec(), Value::String(PhpString::from_vec(name)));
                    rm.set_property(
                        b"class".to_vec(),
                        Value::String(PhpString::from_vec(class.clone())),
                    );
                    rm.set_property(
                        b"__reflection_class".to_vec(),
                        Value::String(PhpString::from_vec(class)),
                    );
                    rm.set_property(
                        b"__reflection_method".to_vec(),
                        Value::String(PhpString::from_bytes(b"__invoke")),
                    );
                    rm.set_property(
                        b"__reflection_closure_fn".to_vec(),
                        Value::String(func_target.clone()),
                    );
                    let closure = obj.borrow().get_property(b"__reflection_param_closure");
                    if matches!(closure, Value::Object(_)) {
                        rm.set_property(b"__reflection_closure_obj".to_vec(), closure);
                    }
                    return Some(Value::Object(Rc::new(RefCell::new(rm))));
                }
            }

            // Free function path. Resolve the canonical function name from
            // the user_functions table if available; otherwise fall back to
            // the stored lowercase name.
            let lower = func_lower.as_bytes();
            let canonical_func = if !lower.is_empty() {
                vm.user_functions
                    .get(lower)
                    .map(|op| {
                        // A closure is named {closure:file:line}.
                        if crate::closure::is_closure_fn_name(&op.name)
                            && !op.display_name.is_empty()
                        {
                            crate::string::msg_str(&op.display_name).to_string()
                        } else {
                            crate::string::msg_str(&op.name).to_string()
                        }
                    })
                    .unwrap_or_else(|| crate::string::msg_str(lower).to_string())
            } else {
                String::new()
            };
            let obj_id = vm.alloc_object_id();
            let mut rf_obj = PhpObject::new(b"ReflectionFunction".to_vec(), obj_id);
            rf_obj.set_property(
                b"name".to_vec(),
                Value::String(PhpString::from_msg(&canonical_func)),
            );
            rf_obj.set_property(
                b"__reflection_target".to_vec(),
                Value::String(func_target.clone()),
            );
            // (a closure's function: isAnonymous() & co)
            if crate::closure::is_closure_fn_name(func_target.as_bytes()) {
                rf_obj.set_property(b"__reflection_is_closure".to_vec(), Value::True);
                let closure = obj.borrow().get_property(b"__reflection_param_closure");
                if matches!(closure, Value::Object(_)) {
                    rf_obj.set_property(b"__reflection_closure_obj".to_vec(), closure);
                }
            }
            Some(Value::Object(Rc::new(RefCell::new(rf_obj))))
        }
        b"getdeclaringclass" => {
            // Returns null for functions, ReflectionClass for methods.
            let ob = obj.borrow();
            let class_lower = ob.get_property(b"__reflection_param_class").to_php_string();
            drop(ob);
            let bytes = class_lower.as_bytes();
            if !bytes.is_empty() {
                if let Some(ce) = rcls(vm, bytes) {
                    let name_str = crate::string::msg_str(&ce.name).to_string();
                    return Some(create_reflection_class(vm, &name_str));
                }
            }
            Some(Value::Null)
        }
        b"getclass" => {
            // Deprecated since 8.0: returns ReflectionClass if param type hints a class
            // Only a single class-name type (optionally nullable) qualifies.
            let class_name_opt = resolved_op_array
                .as_ref()
                .and_then(|op| op.param_types.get(type_idx).cloned().flatten())
                .and_then(|pti| {
                    // ZEND_TYPE_HAS_NAME(): one class name, plus any builtin
                    // types (`iterable` is Traversable|array).
                    fn single_name(pt: &crate::opcode::ParamType) -> Option<Vec<u8>> {
                        use crate::opcode::ParamType;
                        let is_builtin = |t: &ParamType| {
                            matches!(t, ParamType::Simple(n) if matches!(
                                n.to_ascii_lowercase().as_slice(),
                                b"int" | b"float" | b"string" | b"bool" | b"array"
                                    | b"object" | b"callable" | b"mixed" | b"null"
                                    | b"void" | b"false" | b"true" | b"never"
                            ))
                        };
                        match pt {
                            ParamType::Simple(n) if n.eq_ignore_ascii_case(b"iterable") => {
                                Some(b"Traversable".to_vec())
                            }
                            ParamType::Simple(n) => Some(n.clone()),
                            ParamType::Nullable(inner) => single_name(inner),
                            ParamType::Union(items) => {
                                let named: Vec<&ParamType> =
                                    items.iter().filter(|t| !is_builtin(t)).collect();
                                if named.len() == 1
                                    && !matches!(named[0], ParamType::Intersection(_))
                                {
                                    single_name(named[0])
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        }
                    }
                    single_name(&pti.param_type)
                });
            let name = match class_name_opt {
                Some(n) => n,
                None => return Some(Value::Null),
            };
            let lower = name.to_ascii_lowercase();
            let is_builtin_type = matches!(
                lower.as_slice(),
                b"int"
                    | b"float"
                    | b"string"
                    | b"bool"
                    | b"array"
                    | b"object"
                    | b"void"
                    | b"null"
                    | b"mixed"
                    | b"iterable"
                    | b"callable"
                    | b"false"
                    | b"true"
                    | b"never"
                    | b"static"
            );
            if is_builtin_type {
                return Some(Value::Null);
            }
            // The function's scope: a method's class or a closure's scope.
            let scope: Option<Vec<u8>> = {
                let pc = obj
                    .borrow()
                    .get_property(b"__reflection_param_class")
                    .to_php_string();
                if !pc.as_bytes().is_empty() {
                    Some(pc.as_bytes().to_ascii_lowercase())
                } else {
                    resolved_op_array
                        .as_ref()
                        .and_then(|op| op.scope_class.clone())
                }
            };
            let target: Vec<u8> = match lower.as_slice() {
                b"self" => {
                    match scope {
                        Some(s) => s,
                        None => {
                            let exc = vm.create_exception(b"ReflectionException", "Parameter uses \"self\" as type but function is not a class member", vm.current_line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                    }
                }
                b"parent" => {
                    let s = match scope {
                        Some(s) => s,
                        None => {
                            let exc = vm.create_exception(b"ReflectionException", "Parameter uses \"parent\" as type but function is not a class member", vm.current_line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                    };
                    match rcls(vm, &s).and_then(|c| c.parent.clone()) {
                        Some(p) => p.to_ascii_lowercase(),
                        None => {
                            let exc = vm.create_exception(b"ReflectionException", "Parameter uses \"parent\" as type although class does not have a parent", vm.current_line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                    }
                }
                _ => {
                    if rcls(vm, &lower).is_none() {
                        vm.try_autoload_class(&name);
                    }
                    if rcls(vm, &lower).is_none() {
                        let msg =
                            format!("Class \"{}\" does not exist", crate::string::msg_str(&name));
                        let exc =
                            vm.create_exception(b"ReflectionException", &msg, vm.current_line);
                        vm.current_exception = Some(exc);
                        return Some(Value::Null);
                    }
                    lower
                }
            };
            let name_str = rcls(vm, &target)
                .map(|c| crate::string::msg_str(&c.name).to_string())
                .unwrap_or_else(|| crate::string::msg_str(&target).to_string());
            Some(create_reflection_class(vm, &name_str))
        }
        b"isarray" => {
            // Deprecated since 8.0, returns true if param type is array.
            let is_array = resolved_op_array
                .as_ref()
                .and_then(|op| {
                    if type_idx < op.param_types.len() {
                        op.param_types[type_idx]
                            .as_ref()
                            .map(|pti| pti.param_type.clone())
                    } else {
                        None
                    }
                })
                .map(|pt| match &pt {
                    crate::opcode::ParamType::Simple(n) => n.as_slice() == b"array",
                    crate::opcode::ParamType::Nullable(inner) => matches!(inner.as_ref(),
                        crate::opcode::ParamType::Simple(n) if n.as_slice() == b"array"),
                    _ => false,
                })
                .unwrap_or(false);
            Some(if is_array { Value::True } else { Value::False })
        }
        b"iscallable" => {
            // Deprecated since 8.0, returns true if param type is callable.
            let is_callable = resolved_op_array
                .as_ref()
                .and_then(|op| {
                    if type_idx < op.param_types.len() {
                        op.param_types[type_idx]
                            .as_ref()
                            .map(|pti| pti.param_type.clone())
                    } else {
                        None
                    }
                })
                .map(|pt| match &pt {
                    crate::opcode::ParamType::Simple(n) => n.as_slice() == b"callable",
                    crate::opcode::ParamType::Nullable(inner) => matches!(inner.as_ref(),
                        crate::opcode::ParamType::Simple(n) if n.as_slice() == b"callable"),
                    _ => false,
                })
                .unwrap_or(false);
            Some(if is_callable {
                Value::True
            } else {
                Value::False
            })
        }
        b"isdefaultvalueconstant" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                if extract_param_default_constant(op_array, type_idx as u32).is_some()
                    || folded_default_const_name(op_array, type_idx).is_some()
                {
                    return Some(Value::True);
                }
            }
            Some(Value::False)
        }
        b"getdefaultvalueconstantname" => {
            if let Some(op_array) = resolved_op_array.as_ref() {
                if extract_param_default_constant(op_array, type_idx as u32).is_none() {
                    if let Some(n) = folded_default_const_name(op_array, type_idx) {
                        return Some(Value::String(PhpString::from_msg(&n)));
                    }
                }
                if let Some(name) = extract_param_default_constant(op_array, type_idx as u32) {
                    // The AST rendering keeps the name as written (self::X).
                    if let Some(Some(r)) = op_array.param_default_repr.get(type_idx) {
                        let r = r.trim_start_matches('\\');
                        if r.chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == '\\' || c == ':')
                        {
                            return Some(Value::String(PhpString::from_msg(&r.to_string())));
                        }
                    }
                    let display = if let Some(rest) = name.strip_prefix("global:") {
                        // Strip leading backslash from fully-qualified names
                        rest.trim_start_matches('\\').to_string()
                    } else if let Some(rest) = name.strip_prefix("class:") {
                        rest.to_string()
                    } else {
                        name
                    };
                    return Some(Value::String(PhpString::from_msg(&display)));
                }
            }
            Some(Value::Null)
        }
        b"ispromoted" => {
            let promoted = resolved_op_array.as_ref().is_some_and(|op| {
                op.name.to_ascii_lowercase().ends_with(b"__construct")
                    && op
                        .cv_names
                        .get(type_idx)
                        .is_some_and(|n| op_array_promotes(op, n))
            });
            Some(if promoted { Value::True } else { Value::False })
        }
        b"canbepassedbyvalue" => {
            if let Some(f) = resolved_op_array.as_ref().and_then(|op| op.internal_sig) {
                let by_ref_only = f
                    .args
                    .get(param_idx)
                    .map(|a| a.flags & crate::stubs::A_BY_REF != 0)
                    .unwrap_or(false);
                return Some(if by_ref_only {
                    Value::False
                } else {
                    Value::True
                });
            }
            if let Some(op_array) = resolved_op_array.as_ref() {
                if type_idx < op_array.by_ref_params.len() && op_array.by_ref_params[type_idx] {
                    return Some(Value::False);
                }
            }
            Some(Value::True)
        }
        b"__tostring" if resolved_op_array.is_some() => {
            let op = resolved_op_array.as_ref().unwrap();
            let is_variadic = op
                .variadic_param
                .map(|v| v as usize == type_idx)
                .unwrap_or(false);
            let required = !is_variadic && param_idx < op.required_param_count as usize;
            Some(Value::String(PhpString::from_msg(&parameter_string(
                op, param_idx, cv_offset, required,
            ))))
        }
        b"__tostring" => {
            let ob = obj.borrow();
            let name = ob.get_property(b"name").to_php_string().to_msg_string();
            drop(ob);
            let is_variadic = resolved_op_array
                .as_ref()
                .and_then(|op| op.variadic_param)
                .map(|v| v as usize == type_idx)
                .unwrap_or(false);
            // Variadic parameters are always considered "optional" in PHP's
            // ReflectionParameter::__toString (PHP renders them with the
            // <optional> tag and a ... prefix, no default-value suffix).
            let is_optional = is_variadic
                || if let Some(op_array) = resolved_op_array.as_ref() {
                    param_idx >= op_array.required_param_count as usize
                } else {
                    false
                };
            let kind = if is_optional { "optional" } else { "required" };
            let variadic_prefix = if is_variadic { "..." } else { "" };
            let default_suffix = if is_optional && !is_variadic {
                if let Some(op_array) = resolved_op_array.as_ref() {
                    // Prefer constant reference rendering when available
                    if let Some(const_ref) =
                        extract_param_default_constant(op_array, type_idx as u32)
                    {
                        let display = if let Some(rest) = const_ref.strip_prefix("global:") {
                            rest.trim_start_matches('\\').to_string()
                        } else if let Some(rest) = const_ref.strip_prefix("class:") {
                            rest.to_string()
                        } else {
                            const_ref
                        };
                        format!(" = {}", display)
                    } else if let Some(v) = extract_param_default(op_array, type_idx as u32) {
                        format!(" = {}", format_reflection_default(&v))
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            Some(Value::String(PhpString::from_msg(&format!(
                "Parameter #{} [ <{}> {}${}{} ]",
                param_idx, kind, variadic_prefix, name, default_suffix
            ))))
        }
        _ => None,
    }
}

/// ReflectionConstant method dispatch (PHP 8.3+)
pub fn reflection_constant_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let name = ob.get_property(b"name").to_php_string().to_msg_string();
    drop(ob);

    match method {
        // (the name the constant was registered with, const_->name; the
        // property keeps the name given)
        b"getname" | b"getshortname" => {
            let key = normalize_const_key(name.as_bytes());
            let registered = match vm.user_constant_names.iter().find(|(k, _)| *k == key) {
                Some((_, n)) => crate::string::msg_str(n).into_owned(),
                None => {
                    let n = name.strip_prefix('\\').unwrap_or(&name);
                    if matches!(n.to_ascii_lowercase().as_str(), "true" | "false" | "null") {
                        n.to_ascii_uppercase()
                    } else {
                        n.to_string()
                    }
                }
            };
            Some(Value::String(PhpString::from_msg(
                &if method == b"getshortname" {
                    registered
                        .rsplit_once('\\')
                        .map_or(registered.clone(), |(_, s)| s.to_string())
                } else {
                    registered
                },
            )))
        }
        b"getnamespacename" => {
            // Strip a leading backslash so the namespace prefix is reported
            // in canonical (unprefixed) form, matching PHP.
            let n = name.strip_prefix('\\').unwrap_or(&name);
            if let Some(pos) = n.rfind('\\') {
                Some(Value::String(PhpString::from_msg(&n[..pos].to_string())))
            } else {
                Some(Value::String(PhpString::empty()))
            }
        }
        b"getvalue" => Some(reflection_constant_value(vm, name.as_bytes())),
        b"isdefault" => Some(Value::False),
        b"isdeprecated" => {
            let user = constant_attrs_lookup(vm, name.as_bytes())
                .unwrap_or_default()
                .iter()
                .any(|a| attr_is_deprecated(&a.name));
            Some(
                if user || internal_constant_deprecated(vm, name.as_bytes()) {
                    Value::True
                } else {
                    Value::False
                },
            )
        }
        b"getfilename" => Some(
            match vm
                .constant_files
                .get(normalize_const_key(name.as_bytes()).as_slice())
            {
                Some(f) => Value::String(PhpString::from_msg(&f.clone())),
                None => Value::False,
            },
        ),
        b"getextension" | b"getextensionname" => {
            let key = normalize_const_key(name.as_bytes());
            let canon = reflection_constant_resolve(vm, name.as_bytes()).map(|(n, _)| n);
            let ext = if vm.constant_files.contains_key(key.as_slice()) {
                None
            } else if let Some(c) = canon.as_ref().and_then(|n| {
                crate::stubs::find_constant(n.as_bytes()).or_else(|| {
                    // (the stub table keeps a namespaced constant's short
                    // name: Dom\INDEX_SIZE_ERR is dom's INDEX_SIZE_ERR)
                    let (ns, short) = n.rsplit_once('\\')?;
                    let top = ns.split('\\').next()?;
                    crate::stub_sigs::CONSTANTS
                        .iter()
                        .find(|c| c.name == short && c.ext.eq_ignore_ascii_case(top))
                })
            }) {
                Some(c.ext)
            } else if canon.is_some() {
                Some("Core")
            } else {
                None
            };
            Some(match (ext, method) {
                (Some(e), b"getextension") => make_reflection_extension(vm, e),
                (Some(e), _) => Value::String(PhpString::from_msg(&e.to_string())),
                (None, b"getextension") => Value::Null,
                (None, _) => Value::False,
            })
        }
        b"__tostring" => {
            // _const_string()
            let key = normalize_const_key(name.as_bytes());
            let value = reflection_constant_value(vm, name.as_bytes());
            let user = vm.constant_files.contains_key(key.as_slice());
            let name = reflection_constant_resolve(vm, name.as_bytes())
                .map(|(n, _)| n)
                .unwrap_or(name);
            let deprecated = constant_attrs_lookup(vm, name.as_bytes())
                .unwrap_or_default()
                .iter()
                .any(|a| attr_is_deprecated(&a.name))
                || internal_constant_deprecated(vm, name.as_bytes());
            let mut flags = Vec::new();
            if !user {
                flags.push("persistent");
            }
            if deprecated {
                flags.push("deprecated");
            }
            let flags = if flags.is_empty() {
                String::new()
            } else {
                format!("<{}> ", flags.join(", "))
            };
            let ty = match &value {
                Value::Null | Value::Undef => "null",
                Value::True | Value::False => "bool",
                Value::Long(_) => "int",
                Value::Double(_) => "float",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Resource(_) => "resource",
                _ => "object",
            };
            let shown = match &value {
                Value::Array(_) => "Array".to_string(),
                v => v.to_php_string().to_msg_string(),
            };
            Some(Value::String(PhpString::from_msg(&format!(
                "Constant [ {}{} {} ] {{ {} }}\n",
                flags, ty, name, shown
            ))))
        }
        _ => None,
    }
}

/// Names of the `static` variables an op array declares, in order.
fn op_static_var_names(op: &OpArray) -> Vec<Vec<u8>> {
    let mut prefix = op.name.clone();
    prefix.extend_from_slice(b"::");
    let mut out: Vec<Vec<u8>> = Vec::new();
    for o in &op.ops {
        if !matches!(o.opcode, crate::opcode::OpCode::StaticVarInit) {
            continue;
        }
        if let crate::opcode::OperandType::Const(idx) = o.result {
            if let Some(Value::String(s)) = op.literals.get(idx as usize) {
                let b = s.as_bytes();
                // (a method's statics are keyed `Class::method::name`)
                let n = b.strip_prefix(prefix.as_slice()).or_else(|| {
                    let at = b
                        .windows(prefix.len())
                        .position(|w| w == prefix.as_slice())?;
                    (at >= 2 && &b[at - 2..at] == b"::" && !b[..at - 2].contains(&b':'))
                        .then(|| &b[at + prefix.len()..])
                });
                if let Some(n) = n {
                    if !out.iter().any(|x| x.as_slice() == n) {
                        out.push(n.to_vec());
                    }
                }
            }
        }
    }
    out
}

/// zend_get_constant_ptr() for ReflectionConstant: `name` with a leading
/// backslash stripped and the namespace part case-insensitive, then the
/// special true/false/null. Returns the registered name and the key of the
/// constant table entry (None for the special constants).
pub(crate) fn reflection_constant_resolve(
    vm: &Vm,
    name: &[u8],
) -> Option<(String, Option<Vec<u8>>)> {
    let stripped = name.strip_prefix(b"\\").unwrap_or(name);
    let key = normalize_const_key(stripped);
    let disp = crate::string::msg_str(stripped).to_string();
    if vm.constants.const_contains(stripped) {
        return Some((disp, Some(stripped.to_vec())));
    }
    if vm.constants.const_contains(key.as_slice()) {
        return Some((disp, Some(key)));
    }
    if stripped.contains(&b'\\') {
        // Extension constants registered with their namespace's own case.
        let found = vm
            .constants
            .consts()
            .keys()
            .find(|k| k.contains(&b'\\') && normalize_const_key(k) == key)
            .cloned();
        if let Some(k) = found {
            return Some((crate::string::msg_str(&k).to_string(), Some(k)));
        }
        return None;
    }
    let special = if stripped.eq_ignore_ascii_case(b"true") {
        "TRUE"
    } else if stripped.eq_ignore_ascii_case(b"false") {
        "FALSE"
    } else if stripped.eq_ignore_ascii_case(b"null") {
        "NULL"
    } else {
        return None;
    };
    Some((special.to_string(), None))
}

/// The value of a constant resolved by reflection_constant_resolve().
fn reflection_constant_value(vm: &Vm, name: &[u8]) -> Value {
    match reflection_constant_resolve(vm, name) {
        Some((_, Some(k))) => vm.constants.const_get(&k).cloned().unwrap_or(Value::Null),
        Some((n, None)) => match n.as_str() {
            "TRUE" => Value::True,
            "FALSE" => Value::False,
            _ => Value::Null,
        },
        None => Value::Null,
    }
}

/// A `ReflectionExtension` object for extension `name`.
fn make_reflection_extension(vm: &mut Vm, name: &str) -> Value {
    let obj_id = vm.alloc_object_id();
    let mut ext_obj = PhpObject::new(b"ReflectionExtension".to_vec(), obj_id);
    ext_obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_msg(&name.to_string())),
    );
    Value::Object(Rc::new(RefCell::new(ext_obj)))
}

/// Constant table key: the namespace part is case-insensitive.
fn normalize_const_key(name: &[u8]) -> Vec<u8> {
    let n = name.strip_prefix(b"\\").unwrap_or(name);
    match n.iter().rposition(|&b| b == b'\\') {
        Some(p) => {
            let mut k = n[..p].to_ascii_lowercase();
            k.extend_from_slice(&n[p..]);
            k
        }
        None => n.to_vec(),
    }
}

fn attr_is_deprecated(name: &[u8]) -> bool {
    name.strip_prefix(b"\\")
        .unwrap_or(name)
        .eq_ignore_ascii_case(b"Deprecated")
}

/// Internal constants flagged #[\Deprecated] in the stubs (or deprecated
/// by an extension at runtime).
fn internal_constant_deprecated(vm: &Vm, name: &[u8]) -> bool {
    if vm.internal_deprecated_constants.contains_key(name) {
        return true;
    }
    matches!(
        name,
        b"E_STRICT"
            | b"MT_RAND_PHP"
            | b"SUNFUNCS_RET_TIMESTAMP"
            | b"SUNFUNCS_RET_STRING"
            | b"SUNFUNCS_RET_DOUBLE"
            | b"DATE_RFC7231"
            | b"FILE_BINARY"
            | b"FILE_TEXT"
    )
}

/// The attributes of a global constant; internal deprecated constants
/// carry #[\Deprecated].
fn constant_attrs_or_internal(vm: &Vm, name: &[u8]) -> Vec<crate::object::RuntimeAttribute> {
    if let Some(a) = constant_attrs_lookup(vm, name) {
        return a;
    }
    if internal_constant_deprecated(vm, name) {
        return vec![crate::object::RuntimeAttribute {
            name: b"Deprecated".to_vec(),
            args_op_array: std::rc::Rc::new(OpArray::new()),
            validation_error: None,
        }];
    }
    Vec::new()
}

fn constant_attrs_lookup(vm: &Vm, name: &[u8]) -> Option<Vec<crate::object::RuntimeAttribute>> {
    if let Some(a) = vm.constant_attributes.get(name) {
        return Some(a.clone());
    }
    // Fall back to lowercase
    let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
    if let Some(a) = vm.constant_attributes.get(&lower[..]) {
        return Some(a.clone());
    }
    None
}

pub fn reflection_constant_docall(vm: &mut Vm, method: &[u8], args: &[Value]) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let name = ob.get_property(b"name").to_php_string().to_msg_string();
        drop(ob);
        match method {
            b"getattributes" => {
                let attrs = constant_attrs_or_internal(vm, name.as_bytes());
                let filter_name = args.get(1).and_then(|v| {
                    if matches!(v, Value::Null) {
                        None
                    } else {
                        Some(v.to_php_string().as_bytes().to_vec())
                    }
                });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                if filter_flags & !2 != 0 {
                    let exc = vm.create_exception(
                        b"ValueError",
                        "ReflectionConstant::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag",
                        vm.current_line,
                    );
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                // TARGET_CONSTANT = 64
                Some(create_reflection_attributes(
                    vm,
                    &attrs,
                    filter_name.as_deref(),
                    filter_flags,
                    64,
                ))
            }
            _ => None,
        }
    } else {
        None
    }
}

/// Line a suspended generator is executing (its current opline).
fn generator_exec_line(gb: &crate::generator::PhpGenerator) -> u32 {
    use crate::opcode::OpCode;
    if gb.state == crate::generator::GeneratorState::Created {
        return gb.op_array.decl_line;
    }
    let ops = &gb.op_array.ops;
    if gb.yield_from_source.is_some() {
        if let Some(op) = ops.get(gb.ip) {
            if op.opcode == OpCode::YieldFrom {
                return op.line;
            }
        }
    }
    if gb.ip > 0 {
        if let Some(op) = ops.get(gb.ip - 1) {
            return op.line;
        }
    }
    ops.get(gb.ip)
        .map(|o| o.line)
        .unwrap_or(gb.op_array.decl_line)
}

/// The chain of generators from `g` down to the innermost one it is
/// delegating to via `yield from`.
/// ReflectionGenerator methods on a generator that is running: the
/// generators from it to the innermost running one are the top of the
/// running stack (vm.running_generators, outermost first).
fn reflection_running_generator(
    vm: &mut Vm,
    method: &[u8],
    g: &Rc<RefCell<crate::generator::PhpGenerator>>,
) -> Value {
    let addr = RefCell::as_ptr(g) as usize;
    let start = vm
        .running_generators
        .iter()
        .rposition(|(a, _, _, _)| *a == addr)
        .unwrap_or(vm.running_generators.len().saturating_sub(1));
    let names: Vec<String> = vm.running_generators[start..]
        .iter()
        .map(|(_, n, _, _)| n.to_string())
        .collect();
    // (the executing generator's position: where it called the frames
    // running above it, if any)
    // (a generator delegating with `yield from` is at its YIELD_FROM: where
    // it runs the next one)
    let at = vm
        .running_generators
        .get(start)
        .and_then(|(_, _, _, d)| vm.call_stack.get(*d))
        .filter(|f| !f.1.is_empty() && f.2 > 0)
        .map(|f| (f.1.to_string(), f.2));
    match method {
        b"getexecutingfile" => Value::String(PhpString::from_msg(
            &at.map(|(f, _)| f)
                .unwrap_or_else(|| vm.current_file.to_string()),
        )),
        b"getexecutingline" => Value::Long(at.map_or(vm.current_line, |(_, l)| l) as i64),
        b"getexecutinggenerator" => {
            // zend_generator_get_current(): the leaf of its `yield from` tree
            let leaf = vm.running_generators.last().map(|e| e.0);
            match leaf.and_then(crate::gc::find_generator) {
                Some(l) => Value::Generator(l),
                None => Value::Generator(g.clone()),
            }
        }
        b"getthis" => Value::Null,
        b"getfunction" => {
            let name = names.first().cloned().unwrap_or_default();
            let obj_id = vm.alloc_object_id();
            let mut rf = PhpObject::new(b"ReflectionFunction".to_vec(), obj_id);
            rf.set_property(b"name".to_vec(), Value::String(PhpString::from_msg(&name)));
            rf.set_property(b"__reflection_is_closure".to_vec(), Value::True);
            Value::Object(Rc::new(RefCell::new(rf)))
        }
        b"gettrace" => {
            // Innermost first; each frame is located where its caller (the
            // next outer generator) resumed it.
            let mut frames: Vec<crate::vm::CallFrame> = Vec::new();
            for (i, name) in names.iter().enumerate() {
                let (file, line) = if i == 0 {
                    (crate::rcstr::RcStr::new(), 0)
                } else {
                    vm.call_stack
                        .iter()
                        .rev()
                        .find(|f| f.0 == *name)
                        .map(|f| (f.1.clone(), f.2))
                        .unwrap_or_default()
                };
                frames.push((name.clone(), file, line, Vec::new(), false));
            }
            // build_exception_trace() walks the stack top-down.
            let saved = std::mem::replace(&mut vm.call_stack, frames);
            let trace = vm.build_exception_trace();
            vm.call_stack = saved;
            trace
        }
        b"isclosed" => Value::False,
        _ => Value::Null,
    }
}

fn generator_chain(
    g: &Rc<RefCell<crate::generator::PhpGenerator>>,
) -> Vec<Rc<RefCell<crate::generator::PhpGenerator>>> {
    let mut chain = vec![g.clone()];
    for _ in 0..1024 {
        let next = match &chain.last().unwrap().borrow().yield_from_source {
            Some(Value::Generator(inner)) => Some(inner.clone()),
            _ => None,
        };
        match next {
            Some(n) if !chain.iter().any(|c| Rc::ptr_eq(c, &n)) => chain.push(n),
            _ => break,
        }
    }
    chain
}

pub fn reflection_generator_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    // Helper closure to read the underlying generator (if still set).
    let get_gen =
        |obj: &Rc<RefCell<PhpObject>>| -> Option<Rc<RefCell<crate::generator::PhpGenerator>>> {
            let ob = obj.borrow();
            match ob.get_property(b"__reflection_target") {
                Value::Generator(g) => Some(g),
                _ => None,
            }
        };
    // getTrace($options) runs with its arguments (reflection_trace_docall).
    let method: &[u8] = match method {
        b"gettrace" => return None,
        b"__gettrace" => b"gettrace",
        m => m,
    };

    // A generator that is executing right now (ReflectionGenerator used from
    // inside it): its state is in use, its frames are the running ones.
    if let Some(g) = get_gen(obj) {
        if g.try_borrow_mut().is_err() {
            return Some(reflection_running_generator(vm, method, &g));
        }
        // REFLECTION_CHECK_VALID_GENERATOR
        if matches!(method, b"getthis" | b"getexecutinggenerator") {
            let closed = {
                let gb = g.borrow();
                gb.state == crate::generator::GeneratorState::Completed && !gb.is_running
            };
            if closed {
                let exc = vm.create_exception(
                    b"ReflectionException",
                    "Cannot fetch information from a closed Generator",
                    vm.current_line,
                );
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
        }
    }
    match method {
        b"getexecutingfile" => {
            if let Some(g) = get_gen(obj) {
                let gb = g.borrow();
                if gb.state == crate::generator::GeneratorState::Completed && !gb.is_running {
                    let exc = vm.create_exception(
                        b"ReflectionException",
                        "Cannot fetch information from a closed Generator",
                        vm.current_line,
                    );
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                let fname = crate::string::msg_str(&gb.op_array.filename).to_string();
                if !fname.is_empty() {
                    return Some(Value::String(PhpString::from_msg(&fname)));
                }
            }
            Some(Value::String(PhpString::from_msg(
                &vm.current_file.to_string(),
            )))
        }
        b"getexecutingline" => {
            if let Some(g) = get_gen(obj) {
                let gb = g.borrow();
                if gb.state == crate::generator::GeneratorState::Completed && !gb.is_running {
                    let exc = vm.create_exception(
                        b"ReflectionException",
                        "Cannot fetch information from a closed Generator",
                        vm.current_line,
                    );
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
                return Some(Value::Long(generator_exec_line(&gb) as i64));
            }
            Some(Value::Long(vm.current_line as i64))
        }
        b"getfunction" => {
            // Return a ReflectionFunction for the generator's function.
            let target = {
                let ob = obj.borrow();
                ob.get_property(b"__reflection_target")
            };
            if let Value::Generator(g) = &target {
                let gb = g.borrow();
                let fn_name = crate::string::msg_str(&gb.op_array.name).to_string();
                let closure_display = if gb.op_array.display_name.is_empty() {
                    None
                } else {
                    Some(crate::string::msg_str(&gb.op_array.display_name).to_string())
                };
                // Methods yield a ReflectionMethod.
                let method_of = if gb.op_array.display_name.is_empty() {
                    gb.op_array.scope_class.clone()
                } else {
                    None
                };
                drop(gb);
                if let Some(cls) = method_of {
                    let mname = fn_name.rsplit("::").next().unwrap_or(&fn_name).to_string();
                    if rmethod(vm, &cls, mname.as_bytes()).is_some() {
                        let cname = rcls(vm, &cls)
                            .map(|c| crate::string::msg_str(&c.name).to_string())
                            .unwrap_or_else(|| crate::string::msg_str(&cls).to_string());
                        return Some(create_reflection_method(vm, &cname, &mname));
                    }
                }
                let obj_id = vm.alloc_object_id();
                let mut rf_obj = PhpObject::new(b"ReflectionFunction".to_vec(), obj_id);
                let display = match &closure_display {
                    Some(d) => d.clone(),
                    None if fn_name.is_empty() => "{closure}".to_string(),
                    None => fn_name.clone(),
                };
                rf_obj.set_property(
                    b"name".to_vec(),
                    Value::String(PhpString::from_msg(&display)),
                );
                rf_obj.set_property(
                    b"__reflection_target".to_vec(),
                    Value::String(PhpString::from_msg(&fn_name.to_ascii_lowercase())),
                );
                if closure_display.is_some() {
                    rf_obj.set_property(b"__reflection_is_closure".to_vec(), Value::True);
                }
                return Some(Value::Object(Rc::new(RefCell::new(rf_obj))));
            }
            Some(Value::Null)
        }
        b"getthis" => {
            if let Some(g) = get_gen(obj) {
                let gb = g.borrow();
                if !gb.cvs.is_empty() {
                    if let Value::Object(_) = &gb.cvs[0] {
                        return Some(gb.cvs[0].clone());
                    }
                }
            }
            Some(Value::Null)
        }
        b"getexecutinggenerator" => {
            // zend_generator_get_current(): the innermost delegated generator.
            match get_gen(obj) {
                Some(g) => Some(Value::Generator(generator_chain(&g).pop().unwrap())),
                None => Some(obj.borrow().get_property(b"__reflection_target")),
            }
        }
        b"gettrace" => {
            // A closed generator cannot produce a trace.
            if let Some(g) = get_gen(obj) {
                let gb = g.borrow();
                if gb.state == crate::generator::GeneratorState::Completed && !gb.is_running {
                    let exc = vm.create_exception(
                        b"ReflectionException",
                        "Cannot fetch information from a closed Generator",
                        vm.current_line,
                    );
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
            }
            // Frames from the innermost delegated generator out to this one;
            // each frame's file/line is its caller's current position.
            let g = match get_gen(obj) {
                Some(g) => g,
                None => return Some(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
            };
            let chain = generator_chain(&g);
            let mut frames: Vec<crate::vm::CallFrame> = Vec::new();
            let mut objects: Vec<Option<Value>> = Vec::new();
            for (i, genr) in chain.iter().enumerate() {
                let gb = genr.borrow();
                let mut fname = crate::string::msg_str(gb.op_array.effective_name()).to_string();
                let is_method = gb.op_array.scope_class.is_some() && !fname.starts_with("{closure");
                if is_method && !fname.contains("::") {
                    let cls = gb
                        .called_class
                        .clone()
                        .or_else(|| gb.op_array.scope_class.clone())
                        .unwrap_or_default();
                    let cname = rcls(vm, &cls)
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or_else(|| crate::string::msg_str(&cls).to_string());
                    fname = format!("{}::{}", cname, fname);
                }
                let has_this = matches!(gb.cvs.first(), Some(Value::Object(_)))
                    && gb
                        .op_array
                        .cv_names
                        .first()
                        .map(|n| n.as_slice() == b"this")
                        .unwrap_or(false);
                let (file, line) = if i == 0 {
                    (crate::rcstr::RcStr::new(), 0)
                } else {
                    let caller = chain[i - 1].borrow();
                    (caller.op_array.file_name(), generator_exec_line(&caller))
                };
                let mut fargs = gb.args.clone();
                if has_this {
                    if let (Some(Value::Object(a)), Some(Value::Object(t))) =
                        (fargs.first(), gb.cvs.first())
                    {
                        if Rc::ptr_eq(a, t) {
                            fargs.remove(0);
                        }
                    }
                }
                objects.push(if has_this {
                    gb.cvs.first().cloned()
                } else {
                    None
                });
                frames.push((fname, file, line, fargs, has_this));
            }
            // build_exception_trace() walks the stack top-down.
            let saved = std::mem::replace(&mut vm.call_stack, frames);
            let trace = vm.build_exception_trace();
            vm.call_stack = saved;
            // DEBUG_BACKTRACE_PROVIDE_OBJECT: add "object" after "class".
            objects.reverse();
            if let Value::Array(t) = &trace {
                let frames_v: Vec<(ArrayKey, Value)> = t
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (i, (k, fv)) in frames_v.into_iter().enumerate() {
                    if let (Some(Some(o)), Value::Array(fa)) = (objects.get(i), &fv) {
                        let mut nf = PhpArray::new();
                        for (fk, v) in fa.borrow().iter() {
                            nf.set(fk.clone(), v.clone());
                            if matches!(fk, ArrayKey::String(s) if s.as_bytes() == b"class") {
                                nf.set(
                                    ArrayKey::String(PhpString::from_bytes(b"object")),
                                    o.clone(),
                                );
                            }
                        }
                        t.borrow_mut()
                            .set(k, Value::Array(Rc::new(RefCell::new(nf))));
                    }
                }
            }
            Some(trace)
        }
        b"isclosed" => {
            if let Some(g) = get_gen(obj) {
                let gb = g.borrow();
                let closed =
                    gb.state == crate::generator::GeneratorState::Completed && !gb.is_running;
                return Some(if closed { Value::True } else { Value::False });
            }
            Some(Value::False)
        }
        _ => None,
    }
}

/// ReflectionGenerator::getTrace() / ReflectionFiber::getTrace() with
/// their `$options` (DEBUG_BACKTRACE_PROVIDE_OBJECT by default).
pub fn reflection_trace_docall(vm: &mut Vm, class: &[u8], args: &[Value]) -> Option<Value> {
    let Some(Value::Object(obj)) = args.first() else {
        return None;
    };
    let options = args.get(1).map(|v| v.to_long()).unwrap_or(1);
    let trace = if class == b"reflectionfiber" {
        reflection_fiber_method(vm, b"__gettrace", obj)
    } else {
        reflection_generator_method(vm, b"__gettrace", obj)
    }?;
    let Value::Array(t) = &trace else {
        return Some(trace);
    };
    if options & 3 == 1 {
        return Some(trace);
    }
    let mut out = PhpArray::new();
    for (k, fv) in t.borrow().iter() {
        let Value::Array(fa) = fv else {
            out.set(k.clone(), fv.clone());
            continue;
        };
        let mut nf = PhpArray::new();
        for (fk, v) in fa.borrow().iter() {
            let drop_it = match fk {
                ArrayKey::String(s) => {
                    (s.as_bytes() == b"args" && options & 2 != 0)
                        || (s.as_bytes() == b"object" && options & 1 == 0)
                }
                _ => false,
            };
            if !drop_it {
                nf.set(fk.clone(), v.clone());
            }
        }
        out.set(k.clone(), Value::Array(Rc::new(RefCell::new(nf))));
    }
    Some(Value::Array(Rc::new(RefCell::new(out))))
}

/// ReflectionFiber method dispatch (no args). `__reflection_target` is the
/// Fiber object.
pub fn reflection_fiber_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    use crate::fiber::FiberState;
    // getTrace($options) runs with its arguments (reflection_trace_docall).
    let method: &[u8] = match method {
        b"gettrace" => return None,
        b"__gettrace" => b"gettrace",
        m => m,
    };
    let fib = obj.borrow().get_property(b"__reflection_target");
    let fib_obj = match &fib {
        Value::Object(f) => Some(f.clone()),
        _ => None,
    };
    let state = fib_obj
        .as_ref()
        .map(|f| f.borrow().get_property(b"__fiber_state").to_long())
        .unwrap_or(-1);
    let fiber_id = fib_obj.as_ref().map(|f| f.borrow().object_id).unwrap_or(0);
    let is_running = state == FiberState::Running as i64;
    let invalid = state != FiberState::Running as i64 && state != FiberState::Suspended as i64;
    let invalid_error = |vm: &mut Vm| {
        let exc = vm.create_exception(
            b"Error",
            "Cannot fetch information from a fiber that has not been started or is terminated",
            vm.current_line,
        );
        vm.current_exception = Some(exc);
        Some(Value::Null)
    };
    match method {
        b"getfiber" => Some(fib),
        b"getcallable" => {
            if state == FiberState::Terminated as i64 {
                let exc = vm.create_exception(
                    b"Error",
                    "Cannot fetch the callable from a fiber that has terminated",
                    vm.current_line,
                );
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            Some(
                fib_obj
                    .map(|f| f.borrow().get_property(b"__fiber_callback"))
                    .unwrap_or(Value::Null),
            )
        }
        b"getexecutingfile" | b"getexecutingline" => {
            if invalid {
                return invalid_error(vm);
            }
            let pos = if is_running {
                Some((vm.current_file.clone(), vm.current_line))
            } else {
                // The innermost user frame of the fiber (the frame of the
                // last call made from user code): none when it suspended in
                // internal code only (`new Fiber(Fiber::suspend(...))`).
                vm.fiber_coroutines.get(&fiber_id).and_then(|f| {
                    if f.saved_call_stack.is_empty() {
                        return f.suspended_at.clone();
                    }
                    f.saved_call_stack
                        .iter()
                        .rev()
                        .find(|fr| {
                            !(fr.1.is_empty() || &*fr.1 == "[internal function]" || fr.2 == 0)
                        })
                        .map(|fr| (fr.1.clone(), fr.2))
                })
            };
            Some(match (method, pos) {
                (b"getexecutingfile", Some((f, _))) => {
                    Value::String(PhpString::from_msg(&f.to_string()))
                }
                (_, Some((_, l))) => Value::Long(l as i64),
                _ => Value::Null,
            })
        }
        b"gettrace" => {
            if invalid {
                return invalid_error(vm);
            }
            // zend_fetch_debug_backtrace() from the fiber's current frame
            // down to its bottom one (DEBUG_BACKTRACE_PROVIDE_OBJECT)
            if is_running {
                vm.sync_builtin_frames();
                let Some(at) = vm.fiber_bases.iter().rposition(|f| f.0 == fiber_id) else {
                    return Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
                };
                let base = vm.fiber_bases[at].1;
                let end = vm
                    .fiber_bases
                    .get(at + 1)
                    .map_or(vm.call_stack.len(), |f| f.1);
                let trace = vm.debug_backtrace_array(1, 0, Some((base, end)));
                return Some(Value::Array(Rc::new(RefCell::new(trace))));
            }
            let Some(mut fiber) = vm.fiber_coroutines.remove(&fiber_id) else {
                return Some(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
            };
            vm.sync_builtin_frames();
            // (the suspended fiber's frames, put back on top of the stack
            // for the time being)
            let base = vm.call_stack.len();
            vm.fiber_restore_cv_frames(&mut fiber.saved_cv_frames);
            vm.call_stack.append(&mut fiber.saved_call_stack);
            let end = vm.call_stack.len();
            let trace = vm.debug_backtrace_array(1, 0, Some((base, end)));
            fiber.saved_call_stack = vm.call_stack.split_off(base);
            fiber.saved_cv_frames = vm.fiber_take_cv_frames(base);
            vm.fiber_coroutines.insert(fiber_id, fiber);
            Some(Value::Array(Rc::new(RefCell::new(trace))))
        }
        _ => None,
    }
}

/// ReflectionExtension no-arg method dispatch
/// ReflectionZendExtension's methods (zend_extension's name, version,
/// author, URL and copyright).
pub fn reflection_zend_extension_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let name = obj.borrow().get_property(b"name");
    let s = |v: &str| Some(Value::String(PhpString::from_msg(&v.to_string())));
    match method {
        b"getname" => Some(name),
        b"getversion" => Some(
            vm.constants
                .get(b"PHP_VERSION".as_slice())
                .cloned()
                .unwrap_or(Value::Null),
        ),
        b"getauthor" => s("Zend Technologies"),
        b"geturl" => s("http://www.zend.com/"),
        b"getcopyright" => s("Copyright (c)"),
        _ => None,
    }
}

pub fn reflection_extension_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let name = ob.get_property(b"name").to_php_string().to_msg_string();
    drop(ob);

    let ext_lower: String = name.to_ascii_lowercase();

    match method {
        b"getname" => Some(Value::String(PhpString::from_msg(&name))),
        b"getversion" => Some(Value::String(PhpString::from_bytes(
            match ext_lower.as_str() {
                // PECL extensions have their own version
                "uuid" => b"1.3.0".as_slice(),
                "mailparse" => b"3.2.1-dev",
                "imagick" => b"3.8.1",
                _ => b"8.5.4",
            },
        ))),
        b"getfunctions" => {
            // Enumerate all registered internal and user functions that belong
            // to this extension (matched via get_function_extension()). Return
            // a string-keyed map of ReflectionFunction objects.
            let mut result = PhpArray::new();
            // The module's function table order, then the others goro
            // attributes to the extension.
            for fname in extension_function_names(vm, &ext_lower) {
                let rf = create_reflection_function(vm, &fname);
                result.set(ArrayKey::String(PhpString::from_msg(&fname)), rf);
            }
            let mut func_names: Vec<String> = vm
                .functions
                .keys()
                .map(|k| crate::string::msg_str(k).to_string())
                .filter(|k| function_listed_ext(k).is_none())
                .collect();
            func_names.sort();
            for fname in func_names {
                let ext = get_function_extension(&fname).unwrap_or("standard");
                if ext.eq_ignore_ascii_case(&ext_lower)
                    || (ext_lower == "standard" && get_function_extension(&fname).is_none())
                {
                    let rf = create_reflection_function(vm, &fname);
                    result.set(ArrayKey::String(PhpString::from_msg(&fname)), rf);
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getclasses" => {
            let mut result = PhpArray::new();
            let class_list = extension_class_names(&ext_lower);
            for cname in class_list {
                if vm.is_known_builtin_class(cname.to_ascii_lowercase().as_bytes())
                    || vm
                        .classes
                        .contains_key(cname.to_ascii_lowercase().as_bytes())
                {
                    let rc = create_reflection_class(vm, cname);
                    result.set(
                        ArrayKey::String(PhpString::from_msg(&cname.to_string())),
                        rc,
                    );
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getclassnames" => {
            let mut result = PhpArray::new();
            let class_list = extension_class_names(&ext_lower);
            for cname in class_list {
                if vm.is_known_builtin_class(cname.to_ascii_lowercase().as_bytes())
                    || vm
                        .classes
                        .contains_key(cname.to_ascii_lowercase().as_bytes())
                {
                    result.push(Value::String(PhpString::from_msg(&cname.to_string())));
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getconstants" => {
            let mut result = PhpArray::new();
            if ext_lower == "core" {
                for n in CORE_CONSTANT_ORDER {
                    if let Some(v) = vm.constants.get(n.as_bytes()) {
                        result.set(
                            ArrayKey::String(PhpString::from_bytes(n.as_bytes())),
                            v.clone(),
                        );
                    }
                }
            }
            if let Some((_, names)) = vm
                .module_constants
                .iter()
                .find(|(m, _)| m.as_slice() == ext_lower.as_bytes())
            {
                for n in names {
                    if let Some(v) = vm.constants.get(n) {
                        result.set(ArrayKey::String(PhpString::from_vec(n.clone())), v.clone());
                    }
                }
            }
            let const_list = get_extension_constants(&ext_lower);
            for (cname, val) in const_list {
                let v = vm.constants.get(cname.as_bytes()).cloned().unwrap_or(val);
                result.set(ArrayKey::String(PhpString::from_msg(&cname.to_string())), v);
            }
            // The constants the extension's stubs declare (that are defined).
            for c in crate::stub_sigs::CONSTANTS {
                if !c.ext.eq_ignore_ascii_case(&ext_lower) {
                    continue;
                }
                let key = ArrayKey::String(PhpString::from_bytes(c.name.as_bytes()));
                if result.get(&key).is_some() {
                    continue;
                }
                if let Some(v) = vm.constants.get(c.name.as_bytes()) {
                    result.set(key, v.clone());
                }
            }
            if ext_lower != "core" {
                crate::const_order::add_unlisted_module_constants(vm, &ext_lower, &mut result);
                crate::const_order::order_module_constants(&ext_lower, &mut result);
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getinientries" => {
            let mut result = PhpArray::new();
            let ini_list = get_extension_ini_entries(&ext_lower);
            for entry in ini_list {
                let v = vm
                    .constants
                    .get(entry.as_bytes())
                    .map(|v| match v {
                        // an entry without a value
                        Value::Null => Value::Null,
                        v => Value::String(v.to_php_string()),
                    })
                    .unwrap_or_else(|| Value::String(PhpString::from_bytes(b"")));
                result.set(ArrayKey::String(PhpString::from_msg(&entry.to_string())), v);
            }
            Some(Value::Array(Rc::new(RefCell::new(result))))
        }
        b"getdependencies" => {
            // The modules' zend_module_dep tables.
            let deps: &[(&str, &str)] = match ext_lower.as_str() {
                "dom" => &[
                    ("libxml", "Required"),
                    ("lexbor", "Required"),
                    ("domxml", "Conflicts"),
                ],
                "exif" => &[("standard", "Required"), ("mbstring", "Optional")],
                "libxml" | "mysqlnd" | "openssl" | "sodium" => &[("standard", "Required")],
                "mbstring" | "pgsql" | "zip" => &[("pcre", "Required")],
                "mysqli" => &[("spl", "Required"), ("mysqlnd", "Required")],
                "pdo" => &[("spl", "Required")],
                "pdo_mysql" => &[("pdo", "Required"), ("mysqlnd", "Required")],
                "pdo_firebird" | "pdo_odbc" | "pdo_pgsql" | "pdo_sqlite" | "pdo_dblib" => {
                    &[("pdo", "Required")]
                }
                "phar" => &[
                    ("apc", "Optional"),
                    ("bz2", "Optional"),
                    ("openssl", "Optional"),
                    ("zlib", "Optional"),
                    ("standard", "Optional"),
                    ("hash", "Required"),
                    ("spl", "Required"),
                ],
                "session" => &[("spl", "Optional")],
                "simplexml" => &[("libxml", "Required"), ("spl", "Required")],
                "snmp" => &[("spl", "Required")],
                "soap" => &[
                    ("date", "Required"),
                    ("libxml", "Required"),
                    ("session", "Optional"),
                ],
                "standard" => &[("uri", "Required"), ("session", "Optional")],
                "spl" => &[("json", "Required")],
                "uri" => &[("lexbor", "Required")],
                "xml" | "xmlwriter" => &[("libxml", "Required")],
                "xmlreader" => &[("dom", "Required"), ("libxml", "Required")],
                "xsl" => &[("libxml", "Required"), ("dom", "Required")],
                _ => &[],
            };
            let mut out = PhpArray::new();
            for (n, k) in deps {
                out.set(
                    ArrayKey::String(PhpString::from_bytes(n.as_bytes())),
                    Value::String(PhpString::from_bytes(k.as_bytes())),
                );
            }
            Some(Value::Array(Rc::new(RefCell::new(out))))
        }
        b"info" => {
            // Mimic PHP's per-extension MINFO block. Print the section banner,
            // then a minimal block of key=>value entries.
            let banner = match ext_lower.as_str() {
                "reflection" => "Reflection\n\nReflection => enabled\n\n".to_string(),
                "date" => {
                    let mut s = String::new();
                    s.push_str("date\n\n");
                    s.push_str("date/time support => enabled\n");
                    s.push_str("timelib version => 2025.01\n");
                    s.push_str("\"Olson\" Timezone Database Version => 0.system\n");
                    s.push_str("Timezone Database => internal\n");
                    let tz = vm
                        .constants
                        .get(b"__default_timezone".as_ref())
                        .map(|v| v.to_php_string().to_msg_string())
                        .unwrap_or_else(|| "UTC".to_string());
                    s.push_str(&format!("Default timezone => {}\n", tz));
                    s.push('\n');
                    s.push_str("Directive => Local Value => Master Value\n");
                    s.push_str(&format!("date.timezone => {} => {}\n", tz, tz));
                    s.push_str("date.default_latitude => 31.7667 => 31.7667\n");
                    s.push_str("date.default_longitude => 35.2333 => 35.2333\n");
                    s.push_str("date.sunset_zenith => 90.833333 => 90.833333\n");
                    s.push_str("date.sunrise_zenith => 90.833333 => 90.833333\n");
                    s
                }
                _ => format!("{}\n\n{} => enabled\n", name, name),
            };
            vm.write_output(banner.as_bytes());
            Some(Value::Null)
        }
        b"ispersistent" => Some(Value::True),
        b"istemporary" => Some(Value::False),
        b"__tostring" => Some(Value::String(PhpString::from_msg(&extension_string(
            vm, &name, &ext_lower,
        )))),
        _ => None,
    }
}

/// The order a module registers its constants and functions in (its stub's
/// declaration order), for the modules where it is known.
fn extension_declaration_order(ext: &str) -> (&'static [&'static str], &'static [&'static str]) {
    match ext {
        "pcre" => (
            &[
                "PREG_PATTERN_ORDER",
                "PREG_SET_ORDER",
                "PREG_OFFSET_CAPTURE",
                "PREG_UNMATCHED_AS_NULL",
                "PREG_SPLIT_NO_EMPTY",
                "PREG_SPLIT_DELIM_CAPTURE",
                "PREG_SPLIT_OFFSET_CAPTURE",
                "PREG_GREP_INVERT",
                "PREG_NO_ERROR",
                "PREG_INTERNAL_ERROR",
                "PREG_BACKTRACK_LIMIT_ERROR",
                "PREG_RECURSION_LIMIT_ERROR",
                "PREG_BAD_UTF8_ERROR",
                "PREG_BAD_UTF8_OFFSET_ERROR",
                "PREG_JIT_STACKLIMIT_ERROR",
                "PCRE_VERSION",
                "PCRE_VERSION_MAJOR",
                "PCRE_VERSION_MINOR",
                "PCRE_JIT_SUPPORT",
            ],
            &[
                "preg_match",
                "preg_match_all",
                "preg_replace",
                "preg_filter",
                "preg_replace_callback",
                "preg_replace_callback_array",
                "preg_split",
                "preg_quote",
                "preg_grep",
                "preg_last_error",
                "preg_last_error_msg",
            ],
        ),
        _ => (&[], &[]),
    }
}

/// _extension_string(): the module's INI entries, constants, functions and
/// classes (as php-src's stubs declare them).
#[inline(never)]
fn extension_string(vm: &mut Vm, name: &str, ext_lower: &str) -> String {
    let number = vm
        .loaded_extensions
        .iter()
        .position(|e| e.eq_ignore_ascii_case(ext_lower.as_bytes()))
        .map_or(0, |i| i + 1);
    let mut s = format!(
        "Extension [ <persistent> extension #{} {} version {} ] {{\n",
        number, name, "8.5.4"
    );
    let ini = get_extension_ini_entries(ext_lower);
    if !ini.is_empty() {
        s.push_str("\n  - INI {\n");
        for entry in ini {
            let access = ini_access(entry);
            let mut mods: Vec<&str> = Vec::new();
            if access == 7 {
                mods.push("ALL");
            } else {
                if access & 1 != 0 {
                    mods.push("USER");
                }
                if access & 2 != 0 {
                    mods.push("PERDIR");
                }
                if access & 4 != 0 {
                    mods.push("SYSTEM");
                }
            }
            let cur = vm
                .constants
                .get(entry.as_bytes())
                .map(|v| v.to_php_string().to_msg_string())
                .unwrap_or_default();
            s.push_str(&format!(
                "    Entry [ {} <{}> ]\n      Current = '{}'\n",
                entry,
                mods.join(","),
                cur
            ));
            if let Some(Some(orig)) = vm.ini_originals.get(entry.as_bytes()) {
                s.push_str(&format!(
                    "      Default = '{}'\n",
                    orig.to_php_string().to_msg_string()
                ));
            }
            s.push_str("    }\n");
        }
        s.push_str("  }\n");
    }
    // (the stubs are sorted by name: the declaration order where known)
    let (const_order, func_order) = extension_declaration_order(ext_lower);
    let rank = |order: &[&str], n: &str| order.iter().position(|o| *o == n).unwrap_or(usize::MAX);
    let mut stub_consts: Vec<&crate::stubs::StubConst> = crate::stub_sigs::CONSTANTS
        .iter()
        .filter(|c| c.ext.eq_ignore_ascii_case(ext_lower))
        .collect();
    stub_consts.sort_by_key(|c| rank(const_order, c.name));
    let mut consts = String::new();
    let mut nconsts = 0;
    for c in stub_consts {
        let Some(v) = vm.constants.get(c.name.as_bytes()) else {
            continue;
        };
        let shown = match v {
            Value::Array(_) => "Array".to_string(),
            v => v.to_php_string().to_msg_string(),
        };
        let ty = match v {
            Value::True | Value::False => "bool".to_string(),
            v => Vm::value_type_name(v),
        };
        consts.push_str(&format!(
            "    Constant [ <persistent> {} {} ] {{ {} }}\n",
            ty, c.name, shown
        ));
        nconsts += 1;
    }
    if nconsts > 0 {
        s.push_str(&format!(
            "\n  - Constants [{}] {{\n{}  }}\n",
            nconsts, consts
        ));
    }
    let mut funcs: Vec<&str> = crate::stub_sigs::FUNCTIONS
        .iter()
        .filter(|f| {
            f.ext.eq_ignore_ascii_case(ext_lower)
                && vm
                    .functions
                    .contains_key(f.name.to_ascii_lowercase().as_bytes())
        })
        .map(|f| f.name)
        .collect();
    funcs.sort_by_key(|f| rank(func_order, f));
    if !funcs.is_empty() {
        s.push_str("\n  - Functions {\n");
        for f in funcs {
            let rf = create_reflection_function(vm, f);
            if let Some(Value::String(t)) = vm.call_object_method(&rf, b"__toString", &[]) {
                for line in t.to_msg_string().split_inclusive('\n') {
                    if line != "\n" {
                        s.push_str("    ");
                    }
                    s.push_str(line);
                }
            }
        }
        s.push_str("  }\n");
    }
    let classes: Vec<&str> = crate::stub_sigs::CLASSES
        .iter()
        .filter(|c| {
            c.ext.eq_ignore_ascii_case(ext_lower)
                && internal_class_known(vm, c.name.to_ascii_lowercase().as_bytes())
        })
        .map(|c| c.name)
        .collect();
    if !classes.is_empty() {
        s.push_str(&format!("\n  - Classes [{}] {{", classes.len()));
        for c in classes {
            let rc = create_reflection_class(vm, c);
            if let Some(Value::String(t)) = vm.call_object_method(&rc, b"__toString", &[]) {
                s.push('\n');
                for line in t.to_msg_string().split_inclusive('\n') {
                    if line != "\n" {
                        s.push_str("    ");
                    }
                    s.push_str(line);
                }
            }
        }
        s.push_str("  }\n");
    }
    s.push_str("}\n");
    s
}

/// An extension's classes: the hand-kept list (in registration order),
/// then the other classes its stubs declare (by name).
fn extension_class_names(ext: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = crate::ext_order::EXT_CLASSES
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, l)| l.to_vec())
        .unwrap_or_default();
    for c in get_extension_classes(ext) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(c)) {
            out.push(c);
        }
    }
    for c in crate::stub_sigs::CLASSES {
        if c.ext.eq_ignore_ascii_case(ext) && !out.iter().any(|o| o.eq_ignore_ascii_case(c.name)) {
            out.push(c.name);
        }
    }
    out.retain(|c| !crate::stubs::is_pseudo_class(c));
    out
}

/// The extension (lowercase) whose registration-order list names function
/// `name` (lowercase), if any.
fn function_listed_ext(name: &str) -> Option<&'static str> {
    thread_local! {
        static MAP: std::collections::HashMap<String, &'static str> = {
            let mut m = std::collections::HashMap::new();
            for (e, l) in crate::ext_order::EXT_FUNCTIONS {
                for f in l.iter() {
                    m.entry(f.to_ascii_lowercase()).or_insert(*e);
                }
            }
            m
        };
    }
    MAP.with(|m| m.get(name).copied())
}

/// The defined functions of extension `ext` (lowercase), in the module's
/// registration order.
pub fn extension_function_names(vm: &Vm, ext: &str) -> Vec<String> {
    let Some((_, list)) = crate::ext_order::EXT_FUNCTIONS
        .iter()
        .find(|(e, _)| *e == ext)
    else {
        return Vec::new();
    };
    list.iter()
        .filter(|f| vm.functions.contains_key(f.to_ascii_lowercase().as_bytes()))
        .filter(|f| function_listed_ext(&f.to_ascii_lowercase()) == Some(ext))
        .map(|f| f.to_string())
        .collect()
}

fn get_extension_classes(ext: &str) -> &'static [&'static str] {
    match ext {
        "reflection" => &[
            "ReflectionException",
            "Reflection",
            "Reflector",
            "ReflectionFunctionAbstract",
            "ReflectionFunction",
            "ReflectionGenerator",
            "ReflectionParameter",
            "ReflectionType",
            "ReflectionNamedType",
            "ReflectionUnionType",
            "ReflectionIntersectionType",
            "ReflectionMethod",
            "ReflectionClass",
            "ReflectionObject",
            "ReflectionProperty",
            "ReflectionClassConstant",
            "ReflectionExtension",
            "ReflectionZendExtension",
            "ReflectionReference",
            "ReflectionAttribute",
            "ReflectionEnum",
            "ReflectionEnumUnitCase",
            "ReflectionEnumBackedCase",
            "ReflectionFiber",
            "ReflectionConstant",
            "PropertyHookType",
        ],
        "standard" => &[
            "AssertionError",
            "Directory",
            "RoundingMode",
            "StreamBucket",
            "__PHP_Incomplete_Class",
            "php_user_filter",
        ],
        "core" => &[
            "stdClass",
            "Exception",
            "Error",
            "TypeError",
            "ValueError",
            "ArgumentCountError",
            "ArithmeticError",
            "DivisionByZeroError",
            "Closure",
            "Generator",
            "WeakMap",
            "WeakReference",
            "UnhandledMatchError",
        ],
        "spl" => &[
            "ArrayObject",
            "ArrayIterator",
            "RecursiveArrayIterator",
            "SplStack",
            "SplQueue",
            "SplDoublyLinkedList",
            "SplPriorityQueue",
            "SplObjectStorage",
            "SplFixedArray",
            "SplHeap",
            "SplMinHeap",
            "SplMaxHeap",
            "SplFileInfo",
            "SplFileObject",
            "SplTempFileObject",
            "DirectoryIterator",
            "FilesystemIterator",
            "RecursiveDirectoryIterator",
            "GlobIterator",
            "AppendIterator",
            "MultipleIterator",
            "CachingIterator",
            "RecursiveCachingIterator",
            "RecursiveTreeIterator",
            "LimitIterator",
            "NoRewindIterator",
            "RegexIterator",
            "RecursiveRegexIterator",
            "RuntimeException",
            "OutOfBoundsException",
            "OutOfRangeException",
            "LengthException",
            "LogicException",
            "InvalidArgumentException",
            "RangeException",
            "UnderflowException",
            "OverflowException",
            "UnexpectedValueException",
            "BadMethodCallException",
            "BadFunctionCallException",
            "DomainException",
        ],
        "bcmath" => &["BcMath\\Number"],
        "dba" => &["Dba\\Connection"],
        "tidy" => &["tidy", "tidyNode"],
        "ftp" => &["FTP\\Connection"],
        "ldap" => &["LDAP\\Connection", "LDAP\\Result", "LDAP\\ResultEntry"],
        "mailparse" => &["mimemessage"],
        "imagick" => &[
            "Imagick",
            "ImagickDraw",
            "ImagickPixel",
            "ImagickPixelIterator",
            "ImagickKernel",
            "ImagickException",
            "ImagickDrawException",
            "ImagickPixelException",
            "ImagickPixelIteratorException",
            "ImagickKernelException",
        ],
        "xsl" => &["XSLTProcessor"],
        "shmop" => &["Shmop"],
        "sysvsem" => &["SysvSemaphore"],
        "sysvshm" => &["SysvSharedMemory"],
        "sysvmsg" => &["SysvMessageQueue"],
        "zip" => &["ZipArchive"],
        "gd" => &["GdImage", "GdFont"],
        "sqlite3" => &[
            "SQLite3Exception",
            "SQLite3",
            "SQLite3Stmt",
            "SQLite3Result",
        ],
        "fileinfo" => &["finfo"],
        "filter" => &["Filter\\FilterException", "Filter\\FilterFailedException"],
        "sodium" => &["SodiumException"],
        // PHP_MINIT(uri) registration order
        "uri" => &[
            "Uri\\Rfc3986\\Uri",
            "Uri\\WhatWg\\Url",
            "Uri\\UriComparisonMode",
            "Uri\\UriException",
            "Uri\\UriError",
            "Uri\\InvalidUriException",
            "Uri\\WhatWg\\InvalidUrlException",
            "Uri\\WhatWg\\UrlValidationError",
            "Uri\\WhatWg\\UrlValidationErrorType",
        ],
        "intl" => &[
            "Collator",
            "NumberFormatter",
            "IntlListFormatter",
            "Normalizer",
            "Locale",
            "MessageFormatter",
            "IntlDateFormatter",
            "IntlDatePatternGenerator",
            "ResourceBundle",
            "Transliterator",
            "IntlTimeZone",
            "IntlCalendar",
            "IntlGregorianCalendar",
            "Spoofchecker",
            "IntlException",
            "IntlIterator",
            "IntlBreakIterator",
            "IntlRuleBasedBreakIterator",
            "IntlCodePointBreakIterator",
            "IntlPartsIterator",
            "UConverter",
            "IntlChar",
        ],
        _ => &[],
    }
}

thread_local! {
    /// The access level (ZEND_INI_USER/PERDIR/SYSTEM bits) of INI entries
    /// that are not changeable everywhere, registered by their extension.
    static INI_ACCESS: std::cell::RefCell<std::collections::HashMap<String, i64>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Records the access level of an INI entry (ini_get_all()'s "access").
pub fn register_ini_access(name: &str, access: i64) {
    INI_ACCESS.with(|a| a.borrow_mut().insert(name.to_string(), access));
}

/// The access level of an INI entry (ZEND_INI_ALL unless registered).
pub fn ini_access(name: &str) -> i64 {
    INI_ACCESS
        .with(|a| a.borrow().get(name).copied())
        .unwrap_or_else(|| {
            DEFAULT_INI_ACCESS
                .iter()
                .find(|(n, _)| *n == name)
                .map_or(7, |&(_, a)| a)
        })
}

/// The modifiability of the entries not PHP_INI_ALL (1 user, 2 perdir,
/// 4 system), as PHP 8.5 registers them.
const DEFAULT_INI_ACCESS: &[(&str, i64)] = &[
    ("ldap.max_links", 4),
    ("sqlite3.defensive", 1),
    ("sqlite3.extension_dir", 4),
    ("allow_url_fopen", 4),
    ("allow_url_include", 4),
    ("arg_separator.input", 6),
    ("auto_append_file", 6),
    ("auto_globals_jit", 6),
    ("auto_prepend_file", 6),
    ("browscap", 4),
    ("curl.cainfo", 4),
    ("disable_functions", 4),
    ("doc_root", 4),
    ("enable_dl", 4),
    ("enable_post_data_reading", 6),
    ("expose_php", 4),
    ("extension_dir", 4),
    ("ffi.enable", 4),
    ("ffi.preload", 4),
    ("file_uploads", 4),
    ("filter.default", 6),
    ("filter.default_flags", 6),
    ("hard_timeout", 4),
    ("mail.add_x_header", 6),
    ("mail.cr_lf_mode", 6),
    ("mail.force_extra_parameters", 6),
    ("mail.log", 6),
    ("mail.mixed_lf_and_crlf", 6),
    ("max_file_uploads", 6),
    ("max_input_nesting_level", 6),
    ("max_input_time", 6),
    ("max_input_vars", 6),
    ("max_memory_limit", 4),
    ("max_multipart_body_parts", 6),
    ("mbstring.encoding_translation", 6),
    ("opcache.blacklist_filename", 4),
    ("opcache.enable_cli", 4),
    ("opcache.enable_file_override", 4),
    ("opcache.error_log", 4),
    ("opcache.file_cache", 4),
    ("opcache.file_cache_consistency_checks", 4),
    ("opcache.file_cache_only", 4),
    ("opcache.file_cache_read_only", 4),
    ("opcache.force_restart_timeout", 4),
    ("opcache.huge_code_pages", 4),
    ("opcache.interned_strings_buffer", 4),
    ("opcache.jit_buffer_size", 4),
    ("opcache.jit_hot_func", 4),
    ("opcache.jit_hot_loop", 4),
    ("opcache.jit_hot_return", 4),
    ("opcache.jit_max_exit_counters", 4),
    ("opcache.jit_max_root_traces", 4),
    ("opcache.jit_max_side_traces", 4),
    ("opcache.lockfile_path", 4),
    ("opcache.log_verbosity_level", 4),
    ("opcache.max_accelerated_files", 4),
    ("opcache.max_file_size", 4),
    ("opcache.max_wasted_percentage", 4),
    ("opcache.memory_consumption", 4),
    ("opcache.opt_debug_level", 4),
    ("opcache.optimization_level", 4),
    ("opcache.preferred_memory_model", 4),
    ("opcache.preload", 4),
    ("opcache.preload_user", 4),
    ("opcache.protect_memory", 4),
    ("opcache.record_warnings", 4),
    ("opcache.restrict_api", 4),
    ("opcache.save_comments", 4),
    ("opcache.use_cwd", 4),
    ("opcache.validate_permission", 4),
    ("opcache.validate_root", 4),
    ("openssl.cafile", 2),
    ("openssl.capath", 2),
    ("openssl.libctx", 2),
    ("output_buffering", 6),
    ("output_handler", 6),
    ("phar.cache_list", 4),
    ("post_max_size", 6),
    ("realpath_cache_size", 4),
    ("realpath_cache_ttl", 4),
    ("register_argc_argv", 6),
    ("request_order", 6),
    ("sendmail_path", 4),
    ("session.auto_start", 2),
    ("session.upload_progress.cleanup", 2),
    ("session.upload_progress.enabled", 2),
    ("session.upload_progress.freq", 2),
    ("session.upload_progress.min_freq", 2),
    ("session.upload_progress.name", 2),
    ("session.upload_progress.prefix", 2),
    ("short_open_tag", 6),
    ("sys_temp_dir", 4),
    ("syslog.facility", 4),
    ("syslog.ident", 4),
    ("upload_max_filesize", 6),
    ("upload_tmp_dir", 4),
    ("user_dir", 4),
    ("user_ini.cache_ttl", 4),
    ("user_ini.filename", 4),
    ("variables_order", 6),
    ("xmlrpc_errors", 4),
    ("zend.max_allowed_stack_size", 4),
    ("zend.multibyte", 2),
    ("zend.reserved_stack_size", 4),
    ("zend.signal_check", 4),
];

/// The INI entries a module registers (lowercase module name).
pub fn extension_ini_entries(ext: &str) -> &'static [&'static str] {
    get_extension_ini_entries(ext)
}

fn get_extension_ini_entries(ext: &str) -> &'static [&'static str] {
    match ext {
        // (main.c / zend.c: the engine's own entries)
        "core" => &[
            "allow_url_fopen",
            "allow_url_include",
            "arg_separator.input",
            "arg_separator.output",
            "auto_append_file",
            "auto_globals_jit",
            "auto_prepend_file",
            "browscap",
            "default_charset",
            "default_mimetype",
            "disable_functions",
            "display_errors",
            "display_startup_errors",
            "doc_root",
            "docref_ext",
            "docref_root",
            "enable_dl",
            "enable_post_data_reading",
            "error_append_string",
            "error_log",
            "error_log_mode",
            "error_prepend_string",
            "error_reporting",
            "expose_php",
            "extension_dir",
            "fatal_error_backtraces",
            "fiber.stack_size",
            "file_uploads",
            "hard_timeout",
            "highlight.comment",
            "highlight.default",
            "highlight.html",
            "highlight.keyword",
            "highlight.string",
            "html_errors",
            "ignore_repeated_errors",
            "ignore_repeated_source",
            "ignore_user_abort",
            "implicit_flush",
            "include_path",
            "input_encoding",
            "internal_encoding",
            "log_errors",
            "mail.add_x_header",
            "mail.cr_lf_mode",
            "mail.force_extra_parameters",
            "mail.log",
            "mail.mixed_lf_and_crlf",
            "max_execution_time",
            "max_file_uploads",
            "max_input_nesting_level",
            "max_input_time",
            "max_input_vars",
            "max_memory_limit",
            "max_multipart_body_parts",
            "memory_limit",
            "open_basedir",
            "output_buffering",
            "output_encoding",
            "output_handler",
            "post_max_size",
            "precision",
            "realpath_cache_size",
            "realpath_cache_ttl",
            "register_argc_argv",
            "report_memleaks",
            "report_zend_debug",
            "request_order",
            "sendmail_from",
            "sendmail_path",
            "serialize_precision",
            "short_open_tag",
            "SMTP",
            "smtp_port",
            "sys_temp_dir",
            "syslog.facility",
            "syslog.filter",
            "syslog.ident",
            "unserialize_callback_func",
            "upload_max_filesize",
            "upload_tmp_dir",
            "user_dir",
            "user_ini.cache_ttl",
            "user_ini.filename",
            "variables_order",
            "xmlrpc_error_number",
            "xmlrpc_errors",
            "zend.assertions",
            "zend.detect_unicode",
            "zend.enable_gc",
            "zend.exception_ignore_args",
            "zend.exception_string_param_max_len",
            "zend.max_allowed_stack_size",
            "zend.multibyte",
            "zend.reserved_stack_size",
            "zend.script_encoding",
            "zend.signal_check",
        ],
        "openssl" => &["openssl.cafile", "openssl.capath", "openssl.libctx"],
        "session" => &[
            "session.auto_start",
            "session.cache_expire",
            "session.cache_limiter",
            "session.cookie_domain",
            "session.cookie_httponly",
            "session.cookie_lifetime",
            "session.cookie_partitioned",
            "session.cookie_path",
            "session.cookie_samesite",
            "session.cookie_secure",
            "session.gc_divisor",
            "session.gc_maxlifetime",
            "session.gc_probability",
            "session.lazy_write",
            "session.name",
            "session.referer_check",
            "session.save_handler",
            "session.save_path",
            "session.serialize_handler",
            "session.sid_bits_per_character",
            "session.sid_length",
            "session.upload_progress.cleanup",
            "session.upload_progress.enabled",
            "session.upload_progress.freq",
            "session.upload_progress.min_freq",
            "session.upload_progress.name",
            "session.upload_progress.prefix",
            "session.use_cookies",
            "session.use_only_cookies",
            "session.use_strict_mode",
            "session.use_trans_sid",
        ],
        "zlib" => &[
            "zlib.output_compression",
            "zlib.output_compression_level",
            "zlib.output_handler",
        ],
        "curl" => &["curl.cainfo"],
        "phar" => &["phar.cache_list", "phar.readonly", "phar.require_hash"],
        "soap" => &[
            "soap.wsdl_cache",
            "soap.wsdl_cache_dir",
            "soap.wsdl_cache_enabled",
            "soap.wsdl_cache_limit",
            "soap.wsdl_cache_ttl",
        ],
        "bcmath" => &["bcmath.scale"],
        "date" => &[
            "date.timezone",
            "date.default_latitude",
            "date.default_longitude",
            "date.sunset_zenith",
            "date.sunrise_zenith",
        ],
        "pcre" => &["pcre.backtrack_limit", "pcre.recursion_limit", "pcre.jit"],
        "gd" => &["gd.jpeg_ignore_warning"],
        "dba" => &["dba.default_handler"],
        "zend opcache" => &[
            "opcache.enable",
            "opcache.use_cwd",
            "opcache.validate_timestamps",
            "opcache.validate_permission",
            "opcache.validate_root",
            "opcache.dups_fix",
            "opcache.revalidate_path",
            "opcache.log_verbosity_level",
            "opcache.memory_consumption",
            "opcache.interned_strings_buffer",
            "opcache.max_accelerated_files",
            "opcache.max_wasted_percentage",
            "opcache.force_restart_timeout",
            "opcache.revalidate_freq",
            "opcache.file_update_protection",
            "opcache.preferred_memory_model",
            "opcache.blacklist_filename",
            "opcache.max_file_size",
            "opcache.protect_memory",
            "opcache.save_comments",
            "opcache.record_warnings",
            "opcache.optimization_level",
            "opcache.opt_debug_level",
            "opcache.enable_file_override",
            "opcache.enable_cli",
            "opcache.error_log",
            "opcache.restrict_api",
            "opcache.lockfile_path",
            "opcache.file_cache",
            "opcache.file_cache_read_only",
            "opcache.file_cache_only",
            "opcache.file_cache_consistency_checks",
            "opcache.huge_code_pages",
            "opcache.preload",
            "opcache.preload_user",
            "opcache.jit",
            "opcache.jit_buffer_size",
            "opcache.jit_debug",
            "opcache.jit_bisect_limit",
            "opcache.jit_prof_threshold",
            "opcache.jit_max_root_traces",
            "opcache.jit_max_side_traces",
            "opcache.jit_max_exit_counters",
            "opcache.jit_hot_loop",
            "opcache.jit_hot_func",
            "opcache.jit_hot_return",
            "opcache.jit_hot_side_exit",
            "opcache.jit_blacklist_root_trace",
            "opcache.jit_blacklist_side_trace",
            "opcache.jit_max_loop_unrolls",
            "opcache.jit_max_recursive_calls",
            "opcache.jit_max_recursive_returns",
            "opcache.jit_max_polymorphic_calls",
            "opcache.jit_max_trace_length",
        ],
        "readline" => &["cli.pager", "cli.prompt"],
        "tidy" => &["tidy.default_config", "tidy.clean_output"],
        "sqlite3" => &["sqlite3.extension_dir", "sqlite3.defensive"],
        "ldap" => &["ldap.max_links"],
        "mailparse" => &["mailparse.def_charset"],
        "imagick" => &[
            "imagick.locale_fix",
            "imagick.skip_version_check",
            "imagick.progress_monitor",
            "imagick.set_single_thread",
            "imagick.shutdown_sleep_count",
            "imagick.allow_zero_dimension_images",
        ],
        "filter" => &["filter.default", "filter.default_flags"],
        "exif" => &[
            "exif.encode_unicode",
            "exif.decode_unicode_motorola",
            "exif.decode_unicode_intel",
            "exif.encode_jis",
            "exif.decode_jis_motorola",
            "exif.decode_jis_intel",
        ],
        "intl" => &[
            "intl.default_locale",
            "intl.error_level",
            "intl.use_exceptions",
        ],
        "iconv" => &[
            "iconv.input_encoding",
            "iconv.output_encoding",
            "iconv.internal_encoding",
        ],
        "mbstring" => &[
            "mbstring.language",
            "mbstring.detect_order",
            "mbstring.http_input",
            "mbstring.http_output",
            "mbstring.internal_encoding",
            "mbstring.substitute_character",
            "mbstring.encoding_translation",
            "mbstring.http_output_conv_mimetypes",
            "mbstring.strict_detection",
            "mbstring.regex_retry_limit",
            "mbstring.regex_stack_limit",
        ],
        "standard" => &[
            "assert.active",
            "assert.bail",
            "assert.callback",
            "assert.exception",
            "assert.warning",
            "auto_detect_line_endings",
            "default_socket_timeout",
            "from",
            "session.trans_sid_hosts",
            "session.trans_sid_tags",
            "unserialize_max_depth",
            "url_rewriter.hosts",
            "url_rewriter.tags",
            "user_agent",
        ],
        _ => &[],
    }
}

/// The constants of the Core module in registration order (Zend's,
/// main.c's, the output layer's, rfc1867's, then the SAPI's).
pub const CORE_CONSTANT_ORDER: &[&str] = &[
    "E_ERROR",
    "E_WARNING",
    "E_PARSE",
    "E_NOTICE",
    "E_CORE_ERROR",
    "E_CORE_WARNING",
    "E_COMPILE_ERROR",
    "E_COMPILE_WARNING",
    "E_USER_ERROR",
    "E_USER_WARNING",
    "E_USER_NOTICE",
    "E_STRICT",
    "E_RECOVERABLE_ERROR",
    "E_DEPRECATED",
    "E_USER_DEPRECATED",
    "E_ALL",
    "DEBUG_BACKTRACE_PROVIDE_OBJECT",
    "DEBUG_BACKTRACE_IGNORE_ARGS",
    "ZEND_THREAD_SAFE",
    "ZEND_DEBUG_BUILD",
    "ZEND_VM_KIND",
    "TRUE",
    "FALSE",
    "NULL",
    "PHP_VERSION",
    "PHP_MAJOR_VERSION",
    "PHP_MINOR_VERSION",
    "PHP_RELEASE_VERSION",
    "PHP_EXTRA_VERSION",
    "PHP_VERSION_ID",
    "PHP_BUILD_DATE",
    "PHP_ZTS",
    "PHP_DEBUG",
    "PHP_OS",
    "PHP_OS_FAMILY",
    "DEFAULT_INCLUDE_PATH",
    "PEAR_INSTALL_DIR",
    "PEAR_EXTENSION_DIR",
    "PHP_EXTENSION_DIR",
    "PHP_PREFIX",
    "PHP_BINDIR",
    "PHP_SBINDIR",
    "PHP_MANDIR",
    "PHP_LIBDIR",
    "PHP_DATADIR",
    "PHP_SYSCONFDIR",
    "PHP_LOCALSTATEDIR",
    "PHP_CONFIG_FILE_PATH",
    "PHP_CONFIG_FILE_SCAN_DIR",
    "PHP_SHLIB_SUFFIX",
    "PHP_EOL",
    "PHP_MAXPATHLEN",
    "PHP_INT_MAX",
    "PHP_INT_MIN",
    "PHP_INT_SIZE",
    "PHP_FD_SETSIZE",
    "PHP_FLOAT_DIG",
    "PHP_FLOAT_EPSILON",
    "PHP_FLOAT_MAX",
    "PHP_FLOAT_MIN",
    "PHP_OUTPUT_HANDLER_START",
    "PHP_OUTPUT_HANDLER_WRITE",
    "PHP_OUTPUT_HANDLER_FLUSH",
    "PHP_OUTPUT_HANDLER_CLEAN",
    "PHP_OUTPUT_HANDLER_FINAL",
    "PHP_OUTPUT_HANDLER_CONT",
    "PHP_OUTPUT_HANDLER_END",
    "PHP_OUTPUT_HANDLER_CLEANABLE",
    "PHP_OUTPUT_HANDLER_FLUSHABLE",
    "PHP_OUTPUT_HANDLER_REMOVABLE",
    "PHP_OUTPUT_HANDLER_STDFLAGS",
    "PHP_OUTPUT_HANDLER_STARTED",
    "PHP_OUTPUT_HANDLER_DISABLED",
    "PHP_OUTPUT_HANDLER_PROCESSED",
    "UPLOAD_ERR_OK",
    "UPLOAD_ERR_INI_SIZE",
    "UPLOAD_ERR_FORM_SIZE",
    "UPLOAD_ERR_PARTIAL",
    "UPLOAD_ERR_NO_FILE",
    "UPLOAD_ERR_NO_TMP_DIR",
    "UPLOAD_ERR_CANT_WRITE",
    "UPLOAD_ERR_EXTENSION",
    "PHP_SAPI",
    "PHP_BINARY",
    "PHP_CLI_PROCESS_TITLE",
    "STDIN",
    "STDOUT",
    "STDERR",
];

fn get_extension_constants(ext: &str) -> Vec<(&'static str, Value)> {
    match ext {
        "standard" => vec![
            ("CONNECTION_NORMAL", Value::Long(0)),
            ("CONNECTION_ABORTED", Value::Long(1)),
            ("CONNECTION_TIMEOUT", Value::Long(2)),
            ("INI_USER", Value::Long(1)),
            ("INI_PERDIR", Value::Long(2)),
            ("INI_SYSTEM", Value::Long(4)),
            ("INI_ALL", Value::Long(7)),
            ("PHP_URL_SCHEME", Value::Long(0)),
            ("PHP_URL_HOST", Value::Long(1)),
            ("PHP_URL_PORT", Value::Long(2)),
            ("PHP_URL_USER", Value::Long(3)),
            ("PHP_URL_PASS", Value::Long(4)),
            ("PHP_URL_PATH", Value::Long(5)),
            ("PHP_URL_QUERY", Value::Long(6)),
            ("PHP_URL_FRAGMENT", Value::Long(7)),
        ],
        _ => vec![],
    }
}

fn create_reflection_function(vm: &mut Vm, func_name: &str) -> Value {
    let obj_id = vm.alloc_object_id();
    let mut rf_obj = PhpObject::new(b"ReflectionFunction".to_vec(), obj_id);
    rf_obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_msg(&func_name.to_string())),
    );
    let lower = func_name.to_ascii_lowercase();
    rf_obj.set_property(
        b"__reflection_target".to_vec(),
        Value::String(PhpString::from_msg(&lower)),
    );
    Value::Object(Rc::new(RefCell::new(rf_obj)))
}

/// ReflectionNamedType no-arg method dispatch
pub fn reflection_named_type_method(
    _vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let type_name = ob
        .get_property(b"__type_name")
        .to_php_string()
        .to_msg_string();
    let allows_null = ob.get_property(b"__allows_null");
    let is_builtin_type = ob.get_property(b"__is_builtin");
    drop(ob);

    match method {
        b"getname" => Some(Value::String(PhpString::from_msg(&type_name))),
        b"allowsnull" => Some(if matches!(allows_null, Value::True) {
            Value::True
        } else {
            Value::False
        }),
        b"isbuiltin" => Some(if matches!(is_builtin_type, Value::True) {
            Value::True
        } else {
            Value::False
        }),
        b"__tostring" => {
            let ob = obj.borrow();
            let nullable = ob.get_property(b"__allows_null");
            let name = ob
                .get_property(b"__type_name")
                .to_php_string()
                .to_msg_string();
            drop(ob);
            if matches!(nullable, Value::True) && name != "null" && name != "mixed" {
                Some(Value::String(PhpString::from_msg(&format!("?{}", name))))
            } else {
                Some(Value::String(PhpString::from_msg(&name)))
            }
        }
        _ => None,
    }
}

/// ReflectionUnionType / ReflectionIntersectionType method dispatch
pub fn reflection_composite_type_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let allows_null = ob.get_property(b"__allows_null");
    drop(ob);

    match method {
        b"gettypes" => {
            // New objects on each call (zend_reflection_type_factory()).
            let types = obj.borrow().get_property(b"__types");
            let mut out = PhpArray::new();
            if let Value::Array(a) = types {
                let members: Vec<Value> = a.borrow().values().cloned().collect();
                for m in members {
                    out.push(attach_type_handle(vm, &m));
                }
            }
            Some(Value::Array(Rc::new(RefCell::new(out))))
        }
        b"allowsnull" => Some(if matches!(allows_null, Value::True) {
            Value::True
        } else {
            Value::False
        }),
        b"__tostring" => {
            // Recursive helper: render a ReflectionType into a string.
            // Named types -> their __type_name (with leading ? if implicitly nullable named type).
            // Union types  -> children joined with `|`.
            // Intersection types -> children joined with `&` and wrapped in parens
            // when the parent is a union (DNF rendering).
            fn render(v: &Value, parent_union: bool) -> String {
                if let Value::Object(t) = v {
                    let tb = t.borrow();
                    let cn: Vec<u8> = tb
                        .class_name
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect();
                    if cn == b"reflectionnamedtype" {
                        let name = tb
                            .get_property(b"__type_name")
                            .to_php_string()
                            .to_msg_string();
                        // Only union types include "null" as a separate member, so
                        // we don't emit `?` here.
                        name
                    } else if cn == b"reflectionuniontype" {
                        let types = tb.get_property(b"__types");
                        drop(tb);
                        if let Value::Array(arr) = types {
                            let parts: Vec<String> =
                                arr.borrow().iter().map(|(_, v)| render(v, true)).collect();
                            parts.join("|")
                        } else {
                            String::new()
                        }
                    } else if cn == b"reflectionintersectiontype" {
                        let types = tb.get_property(b"__types");
                        drop(tb);
                        if let Value::Array(arr) = types {
                            let parts: Vec<String> =
                                arr.borrow().iter().map(|(_, v)| render(v, false)).collect();
                            let joined = parts.join("&");
                            if parent_union {
                                format!("({})", joined)
                            } else {
                                joined
                            }
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            }

            let ob = obj.borrow();
            let class_lower: Vec<u8> = ob
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            drop(ob);
            // Render with the appropriate "parent is union" flag depending on
            // self class.
            let self_value = Value::Object(obj.clone());
            let _ = class_lower; // already encoded in render via class_name
            Some(Value::String(PhpString::from_msg(&render(
                &self_value,
                false,
            ))))
        }
        _ => None,
    }
}

/// ReflectionClassConstant method dispatch
pub fn reflection_class_constant_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    let const_name = ob.get_property(b"name").to_php_string().to_msg_string();
    let class_name = ob.get_property(b"class").to_php_string().to_msg_string();
    let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&class_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();

    // Look up constant metadata
    let mut meta =
        rcls(vm, &class_lower).and_then(|ce| ce.constants_meta.get(const_name.as_bytes()).cloned());
    internal_const_deprecated_attr(vm, const_name.as_bytes(), &mut meta);

    let visibility = meta
        .as_ref()
        .map(|m| m.visibility)
        .unwrap_or(Visibility::Public);
    let is_final = meta.as_ref().map(|m| m.is_final).unwrap_or(false);

    // `unset($r->name)`: the typed property is uninitialized.
    if matches!(method, b"getname" | b"__tostring")
        && matches!(ob.get_property(b"name"), Value::Undef)
    {
        drop(ob);
        let msg = format!(
            "Typed property {}::$name must not be accessed before initialization",
            crate::value::display_class_name(&obj.borrow().class_name)
        );
        let exc = vm.create_exception(b"Error", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Some(Value::Null);
    }
    match method {
        b"getname" => Some(ob.get_property(b"name")),
        b"getvalue" => {
            // Evaluate (and cache) the constant like a Class::NAME fetch.
            let cached = ob.get_property(b"__reflection_value");
            let has_cached = ob.has_property(b"__reflection_value");
            drop(ob);
            let decl = meta
                .as_ref()
                .map(|m| m.declaring_class.clone())
                .unwrap_or_else(|| class_lower.clone());
            if has_cached && !is_deferred_const_value(&cached) {
                return Some(cached);
            }
            match resolve_class_constant(vm, &decl, const_name.as_bytes()) {
                Some(v) => {
                    obj.borrow_mut()
                        .set_property(b"__reflection_value".to_vec(), v.clone());
                    Some(v)
                }
                None => Some(Value::Null),
            }
        }
        b"getdeclaringclass" => {
            let declaring = meta
                .as_ref()
                .map(|m| {
                    let dc_lower = &m.declaring_class;
                    rcls(vm, dc_lower.as_slice())
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or(class_name.clone())
                })
                .unwrap_or(class_name.clone());
            drop(ob);
            Some(create_reflection_class(vm, &declaring))
        }
        b"ispublic" => Some(if visibility == Visibility::Public {
            Value::True
        } else {
            Value::False
        }),
        b"isprotected" => Some(if visibility == Visibility::Protected {
            Value::True
        } else {
            Value::False
        }),
        b"isprivate" => Some(if visibility == Visibility::Private {
            Value::True
        } else {
            Value::False
        }),
        b"getmodifiers" => {
            let mut mods = 0i64;
            match visibility {
                Visibility::Public => mods |= 1,
                Visibility::Protected => mods |= 2,
                Visibility::Private => mods |= 4,
            }
            if is_final {
                mods |= 0x20;
            }
            Some(Value::Long(mods))
        }
        b"getdoccomment" => {
            if let Some(doc) = meta.as_ref().and_then(|m| m.doc_comment.clone()) {
                Some(Value::String(PhpString::from_bytes(doc.as_bytes())))
            } else {
                Some(Value::False)
            }
        }
        b"isfinal" => Some(if is_final { Value::True } else { Value::False }),
        b"isenumcase" => {
            // A ReflectionClassConstant is an enum case only when the
            // constant was declared as `case Foo;` on an enum — not when
            // it's a regular constant that *happens* to reference an enum
            // case value (e.g. `const Baz = self::Bar;`).
            let is_case = rcls(vm, &class_lower)
                .map(|ce| {
                    ce.enum_cases
                        .iter()
                        .any(|(cn, _)| cn == const_name.as_bytes())
                })
                .unwrap_or(false);
            drop(ob);
            Some(if is_case { Value::True } else { Value::False })
        }
        b"isdeprecated" => {
            let has_dep = meta
                .as_ref()
                .map(|m| {
                    m.attributes.iter().any(|a| {
                        let n = crate::string::msg_str(&a.name);
                        n.eq_ignore_ascii_case("deprecated")
                            || n.eq_ignore_ascii_case("\\deprecated")
                    })
                })
                .unwrap_or(false);
            Some(if has_dep { Value::True } else { Value::False })
        }
        b"hastype" => {
            let has = meta
                .as_ref()
                .map(|m| m.declared_type.is_some())
                .unwrap_or(false);
            Some(if has { Value::True } else { Value::False })
        }
        b"gettype" => {
            let pt = meta.as_ref().and_then(|m| m.declared_type.clone());
            if let Some(pt) = pt {
                Some(create_reflection_type(vm, &pt))
            } else {
                Some(Value::Null)
            }
        }
        // ReflectionEnumUnitCase/BackedCase methods
        b"getenum" => {
            drop(ob);
            Some(create_reflection_class(vm, &class_name))
        }
        b"getbackingvalue" => {
            // For backed enum cases, return the backing value
            let cached = ob.get_property(b"__reflection_value");
            let has_cached = ob.has_property(b"__reflection_value");
            drop(ob);
            let val = if has_cached && !is_deferred_const_value(&cached) {
                cached
            } else {
                let decl = meta
                    .as_ref()
                    .map(|m| m.declaring_class.clone())
                    .unwrap_or_else(|| class_lower.clone());
                match resolve_class_constant(vm, &decl, const_name.as_bytes()) {
                    Some(v) if vm.current_exception.is_none() => {
                        obj.borrow_mut()
                            .set_property(b"__reflection_value".to_vec(), v.clone());
                        v
                    }
                    _ => return Some(Value::Null),
                }
            };
            if let Value::Object(enum_obj) = &val {
                let eo = enum_obj.borrow();
                let backing = eo.get_property(b"value");
                if matches!(backing, Value::Null) && !eo.has_property(b"value") {
                    // Not a backed case
                    let case_name_str = const_name.clone();
                    let err_msg = format!(
                        "Enum case {}::{} is not a backed case",
                        class_name, case_name_str
                    );
                    let exc =
                        vm.create_exception(b"ReflectionException", &err_msg, vm.current_line);
                    vm.current_exception = Some(exc);
                    Some(Value::Null)
                } else {
                    Some(backing)
                }
            } else {
                Some(Value::Null)
            }
        }
        b"__tostring" => {
            let name = ob.get_property(b"name").to_php_string().to_msg_string();
            let cached = ob.get_property(b"__reflection_value");
            let has_cached = ob.has_property(b"__reflection_value");
            drop(ob);
            let _ = (visibility, is_final);
            // zend_update_class_constant() first; errors abort the cast.
            let val = if !has_cached || is_deferred_const_value(&cached) {
                let decl = meta
                    .as_ref()
                    .map(|m| m.declaring_class.clone())
                    .unwrap_or_else(|| class_lower.clone());
                match vm.class_constant_value(&decl.to_ascii_lowercase(), name.as_bytes()) {
                    Some(Ok(v)) => v,
                    _ => return Some(Value::Null),
                }
            } else {
                cached
            };
            Some(Value::String(PhpString::from_msg(&class_const_string(
                name.as_bytes(),
                &val,
                meta.as_ref(),
                "",
            ))))
        }
        _ => None,
    }
}

/// ReflectionParameter methods with args
pub fn reflection_parameter_docall(vm: &mut Vm, method: &[u8], args: &[Value]) -> Option<Value> {
    match deprecated_method_call(vm, b"reflectionparameter", method, args) {
        Some(true) => return Some(Value::Null),
        Some(false) => {
            return match args.first() {
                Some(Value::Object(obj)) => reflection_parameter_method_body(vm, method, obj),
                _ => None,
            };
        }
        None => {}
    }
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let func_target = ob.get_property(b"__reflection_func").to_php_string();
        let param_idx = ob.get_property(b"__reflection_param_idx").to_long() as usize;
        let param_class = ob.get_property(b"__reflection_param_class");
        let param_method = ob.get_property(b"__reflection_param_method");
        let position = ob.get_property(b"__reflection_param_position").to_long() as usize;
        drop(ob);

        match method {
            b"getattributes" => {
                let filter_name = args.get(1).and_then(|v| {
                    if matches!(v, Value::Null) {
                        None
                    } else {
                        Some(v.to_php_string().as_bytes().to_vec())
                    }
                });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);

                if !matches!(&param_class, Value::Null) {
                    let class_lower = param_class.to_php_string().as_bytes().to_vec();
                    let method_lower = param_method.to_php_string().as_bytes().to_vec();
                    let decl = rcls(vm, &class_lower)
                        .and_then(|c| c.get_method(&method_lower))
                        .map(|m| m.declaring_class.clone());
                    let attrs = rcls(vm, &class_lower)
                        .and_then(|c| c.get_method(&method_lower))
                        .and_then(|m| m.op_array.param_attributes.get(position))
                        .cloned()
                        .unwrap_or_default();
                    let list = create_reflection_attributes(
                        vm,
                        &attrs,
                        filter_name.as_deref(),
                        filter_flags,
                        32,
                    );
                    set_attr_scope(&list, decl, b"__reflection_attr_scope");
                    Some(list)
                } else {
                    let attrs = rfunc(vm, func_target.as_bytes())
                        .and_then(|op| op.param_attributes.get(param_idx))
                        .cloned()
                        .unwrap_or_default();
                    let list = create_reflection_attributes(
                        vm,
                        &attrs,
                        filter_name.as_deref(),
                        filter_flags,
                        32,
                    );
                    let scope = match obj.borrow().get_property(b"__reflection_param_attr_scope") {
                        Value::String(s) => Some(s.as_bytes().to_vec()),
                        _ => None,
                    };
                    set_attr_scope(&list, scope, b"__reflection_attr_scope");
                    Some(list)
                }
            }
            _ => None,
        }
    } else {
        None
    }
}

/// ReflectionClassConstant methods with args
pub fn reflection_class_constant_docall(
    vm: &mut Vm,
    method: &[u8],
    args: &[Value],
) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        let ob = obj.borrow();
        let const_name = ob.get_property(b"name").to_php_string().to_msg_string();
        let class_name = ob.get_property(b"class").to_php_string().to_msg_string();
        let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&class_name)
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        drop(ob);

        match method {
            b"getattributes" => {
                let mut meta = rcls(vm, &class_lower)
                    .and_then(|ce| ce.constants_meta.get(const_name.as_bytes()).cloned());
                internal_const_deprecated_attr(vm, const_name.as_bytes(), &mut meta);
                let attrs = meta
                    .as_ref()
                    .map(|m| m.attributes.clone())
                    .unwrap_or_default();
                let filter_name = args.get(1).and_then(|v| {
                    if matches!(v, Value::Null) {
                        None
                    } else {
                        Some(v.to_php_string().as_bytes().to_vec())
                    }
                });
                let filter_flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
                Some(create_reflection_attributes(
                    vm,
                    &attrs,
                    filter_name.as_deref(),
                    filter_flags,
                    16,
                ))
            }
            _ => None,
        }
    } else {
        None
    }
}

/// ReflectionAttribute methods with args
pub fn reflection_attribute_docall(vm: &mut Vm, method: &[u8], args: &[Value]) -> Option<Value> {
    let this = args.first()?;
    if let Value::Object(obj) = this {
        // Delegate to the no-arg handler, which handles newInstance
        reflection_attribute_method(vm, method, obj)
    } else {
        None
    }
}

/// Handle static method calls on Reflection classes
/// Wrapper that includes the original (case-preserved) method name for error messages.
/// _reflection_write_property(): the declared `$name` / `$class` properties
/// of reflection objects are read-only. False (with a ReflectionException
/// pending) when the write must not happen.
pub(crate) fn reflection_prop_write_allowed(
    vm: &mut Vm,
    obj: &Rc<RefCell<PhpObject>>,
    name: &[u8],
) -> bool {
    let (cl, display) = {
        let ob = obj.borrow();
        (ob.class_name.to_ascii_lowercase(), ob.class_name.clone())
    };
    let is = |vm: &Vm, base: &[u8]| cl == base || vm.class_extends(&cl, base);
    let declared = match name {
        b"name" => [
            &b"reflectionfunctionabstract"[..],
            b"reflectionclass",
            b"reflectionproperty",
            b"reflectionclassconstant",
            b"reflectionparameter",
            b"reflectionextension",
            b"reflectionzendextension",
            b"reflectionconstant",
        ]
        .iter()
        .any(|b| is(vm, b)),
        b"class" => [
            &b"reflectionmethod"[..],
            b"reflectionproperty",
            b"reflectionclassconstant",
        ]
        .iter()
        .any(|b| is(vm, b)),
        _ => false,
    };
    if !declared {
        return true;
    }
    let msg = format!(
        "Cannot set read-only property {}::${}",
        crate::value::display_class_name(&display),
        crate::string::msg_str(name)
    );
    let exc = vm.create_exception(b"ReflectionException", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    false
}

pub fn reflection_static_call_named(
    vm: &mut Vm,
    class_lower: &str,
    method_lower: &str,
    method_original: &str,
    args: &[Value],
    line: u32,
) -> Option<Value> {
    // A static method inherited by a userland subclass of a Reflection
    // class (MyReflectionMethod::createFromMethodName()): run the internal
    // one; a `static` return becomes an instance of the called class.
    if let Some(user) = rcls(vm, class_lower.as_bytes()).filter(|c| {
        !is_internal_class(vm, class_lower.as_bytes())
            && c.get_method(method_lower.as_bytes()).is_none()
    }) {
        let called = user.name.clone();
        let mut base = user.parent.clone().map(|p| p.to_ascii_lowercase());
        for _ in 0..64 {
            let Some(b) = base.clone() else { break };
            if is_internal_class(vm, &b) {
                break;
            }
            base = rcls(vm, &b)
                .and_then(|c| c.parent.clone())
                .map(|p| p.to_ascii_lowercase());
        }
        let base = base?;
        let base_s = crate::string::msg_str(&base).to_string();
        if !base_s.starts_with("reflection") {
            return None;
        }
        let r =
            reflection_static_call_named(vm, &base_s, method_lower, method_original, args, line);
        if method_lower == "createfrommethodname" {
            if let Some(Value::Object(o)) = &r {
                o.borrow_mut().set_class_name(called);
            }
        }
        return r;
    }
    // Forbid calling non-static instance methods statically. We list common
    // ReflectionClass/Method/Property/etc instance methods that have no
    // legitimate static dispatch.
    let is_instance_only = match class_lower {
        "reflectionclass" | "reflectionobject" | "reflectionenum" => matches!(
            method_lower,
            "getname"
                | "getshortname"
                | "getnamespacename"
                | "getfilename"
                | "getstartline"
                | "getendline"
                | "getdoccomment"
                | "isiterateable"
                | "isiterable"
                | "isabstract"
                | "isfinal"
                | "isinterface"
                | "istrait"
                | "isenum"
                | "isanonymous"
                | "isinstantiable"
                | "iscloneable"
                | "isuserdefined"
                | "isinternal"
                | "isreadonly"
                | "getmodifiers"
                | "getmethods"
                | "getproperties"
                | "getconstants"
                | "getreflectionconstants"
                | "getmethod"
                | "getproperty"
                | "hasmethod"
                | "hasproperty"
                | "hasconstant"
                | "getconstant"
                | "getclassconstant"
                | "getreflectionconstant"
                | "hasreflectionconstant"
                | "getinterfaces"
                | "getinterfacenames"
                | "getparentclass"
                | "implementsinterface"
                | "issubclassof"
                | "isinstance"
                | "newinstance"
                | "newinstanceargs"
                | "newinstancewithoutconstructor"
                | "getconstructor"
                | "getstaticproperties"
                | "getstaticpropertyvalue"
                | "setstaticpropertyvalue"
                | "getdefaultproperties"
                | "gettraits"
                | "gettraitnames"
                | "gettraitaliases"
                | "getattributes"
                | "getextension"
                | "getextensionname"
                | "inNamespace"
                | "innamespace"
                | "__tostring"
                | "getcases"
                | "iscase"
                | "hascase"
                | "getcase"
                | "getbackingtype"
        ),
        "reflectionmethod" => matches!(
            method_lower,
            "getname"
                | "getshortname"
                | "getnamespacename"
                | "getfilename"
                | "getstartline"
                | "getendline"
                | "getdoccomment"
                | "isuserdefined"
                | "isinternal"
                | "isabstract"
                | "isfinal"
                | "ispublic"
                | "isprotected"
                | "isprivate"
                | "isstatic"
                | "isconstructor"
                | "isdestructor"
                | "iscloneable"
                | "isvariadic"
                | "isgenerator"
                | "isclosure"
                | "isdeprecated"
                | "returnsreference"
                | "hasreturntype"
                | "getreturntype"
                | "hastentativereturntype"
                | "gettentativereturntype"
                | "getmodifiers"
                | "getparameters"
                | "getnumberofparameters"
                | "getnumberofrequiredparameters"
                | "getclosure"
                | "getclosurethis"
                | "getclosurescopeclass"
                | "getclosurecalledclass"
                | "getclosureusedvariables"
                | "getdeclaringclass"
                | "getprototype"
                | "hasprototype"
                | "getattributes"
                | "getextension"
                | "getextensionname"
                | "__tostring"
                | "invoke"
                | "invokeargs"
                | "settaccessible"
        ),
        "reflectionproperty" => matches!(
            method_lower,
            "getname"
                | "getvalue"
                | "setvalue"
                | "isinitialized"
                | "hasdefaultvalue"
                | "getdefaultvalue"
                | "getdeclaringclass"
                | "getmodifiers"
                | "ispublic"
                | "isprotected"
                | "isprivate"
                | "isstatic"
                | "isreadonly"
                | "ispromoted"
                | "isdefault"
                | "isvirtual"
                | "getdoccomment"
                | "gettype"
                | "hastype"
                | "getattributes"
                | "settaccessible"
                | "__tostring"
                | "gethooks"
                | "hashook"
                | "gethook"
                | "getrawvalue"
                | "setrawvalue"
                | "setrawvaluewithouthook"
                | "isdynamic"
                | "isabstract"
                | "isfinal"
        ),
        "reflectionfunction" => matches!(
            method_lower,
            "getname"
                | "getshortname"
                | "getnamespacename"
                | "getfilename"
                | "getstartline"
                | "getendline"
                | "getdoccomment"
                | "isuserdefined"
                | "isinternal"
                | "isdisabled"
                | "isclosure"
                | "isgenerator"
                | "isvariadic"
                | "isdeprecated"
                | "returnsreference"
                | "hasreturntype"
                | "getreturntype"
                | "hastentativereturntype"
                | "gettentativereturntype"
                | "getparameters"
                | "getnumberofparameters"
                | "getnumberofrequiredparameters"
                | "getclosure"
                | "getclosurethis"
                | "getclosurescopeclass"
                | "getclosurecalledclass"
                | "getclosureusedvariables"
                | "getattributes"
                | "getextension"
                | "getextensionname"
                | "__tostring"
                | "invoke"
                | "invokeargs"
        ),
        "reflectionparameter" => matches!(
            method_lower,
            "getname"
                | "getposition"
                | "ispassedbyreference"
                | "canbepassedbyvalue"
                | "getdeclaringfunction"
                | "getdeclaringclass"
                | "getclass"
                | "hastype"
                | "gettype"
                | "isarray"
                | "iscallable"
                | "allowsnull"
                | "isoptional"
                | "isdefaultvalueavailable"
                | "getdefaultvalue"
                | "isdefaultvalueconstant"
                | "getdefaultvalueconstantname"
                | "isvariadic"
                | "ispromoted"
                | "getattributes"
                | "__tostring"
        ),
        _ => false,
    };
    if is_instance_only {
        // Match PHP's casing: keep the original method name as written.
        // PHP uses canonical class names: ReflectionClass, ReflectionMethod etc.
        // For subclasses (ReflectionObject extends ReflectionClass), report the
        // declaring class name in the error message — that's how Zend reports
        // "Non-static method ReflectionClass::getName() cannot be called statically"
        // even when the user wrote ReflectionObject::getName().
        let class_canonical = match class_lower {
            "reflectionclass" => "ReflectionClass",
            "reflectionobject" => "ReflectionClass",
            "reflectionenum" => "ReflectionClass",
            "reflectionmethod" => "ReflectionMethod",
            "reflectionproperty" => "ReflectionProperty",
            "reflectionfunction" => "ReflectionFunction",
            "reflectionparameter" => "ReflectionParameter",
            _ => class_lower,
        };
        let err_msg = format!(
            "Non-static method {}::{}() cannot be called statically",
            class_canonical, method_original
        );
        let exc = vm.create_exception(b"Error", &err_msg, line);
        vm.current_exception = Some(exc);
        return Some(Value::Null);
    }
    reflection_static_call(vm, class_lower, method_lower, args, line)
}

pub fn reflection_static_call(
    vm: &mut Vm,
    class_lower: &str,
    method_lower: &str,
    args: &[Value],
    line: u32,
) -> Option<Value> {
    match class_lower {
        "reflection" => {
            match method_lower {
                "getmodifiernames" => {
                    let modifiers = args.first().map(|v| v.to_long()).unwrap_or(0);
                    let mut names = PhpArray::new();
                    // ZEND_METHOD(Reflection, getModifierNames)
                    let mut add = |n: &[u8]| names.push(Value::String(PhpString::from_bytes(n)));
                    if modifiers & 0x40 != 0 {
                        add(b"abstract");
                    }
                    if modifiers & 0x20 != 0 {
                        add(b"final");
                    }
                    if modifiers & 0x200 != 0 {
                        add(b"virtual");
                    }
                    match modifiers & 7 {
                        1 => add(b"public"),
                        4 => add(b"private"),
                        2 => add(b"protected"),
                        _ => {}
                    }
                    match modifiers & 0x1800 {
                        0x800 => add(b"protected(set)"),
                        0x1000 => add(b"private(set)"),
                        _ => {}
                    }
                    if modifiers & 0x10 != 0 {
                        add(b"static");
                    }
                    // ZEND_ACC_READONLY | ZEND_ACC_READONLY_CLASS
                    if modifiers & (0x80 | 0x10000) != 0 {
                        add(b"readonly");
                    }
                    Some(Value::Array(Rc::new(RefCell::new(names))))
                }
                _ => None,
            }
        }
        "reflectionmethod" => {
            match method_lower {
                "createfrommethodname" => {
                    let method_str = args
                        .first()
                        .map(|v| v.to_php_string().to_msg_string())
                        .unwrap_or_default();
                    if let Some(pos) = method_str.find("::") {
                        let class_name = &method_str[..pos];
                        let method_name = &method_str[pos + 2..];
                        // Create a ReflectionMethod
                        let class_lower_bytes: Vec<u8> =
                            crate::errdisplay::raw_error_bytes(&class_name)
                                .iter()
                                .map(|b| b.to_ascii_lowercase())
                                .collect();
                        let method_lower_bytes: Vec<u8> =
                            crate::errdisplay::raw_error_bytes(&method_name)
                                .iter()
                                .map(|b| b.to_ascii_lowercase())
                                .collect();

                        let _ = (&class_lower_bytes, &method_lower_bytes);
                        // (the constructor's two-argument form: internal
                        // classes included)
                        let obj_id = vm.alloc_object_id();
                        let obj = Rc::new(RefCell::new(PhpObject::new(
                            b"ReflectionMethod".to_vec(),
                            obj_id,
                        )));
                        let a = [
                            Value::Object(obj.clone()),
                            Value::String(PhpString::from_msg(&class_name.to_string())),
                            Value::String(PhpString::from_msg(&method_name.to_string())),
                        ];
                        if !reflection_method_construct(vm, &a, line) {
                            return Some(Value::Null);
                        }
                        Some(Value::Object(obj))
                    } else {
                        let err_msg = "ReflectionMethod::createFromMethodName(): Argument #1 ($method) must be a valid method name".to_string();
                        let exc = vm.create_exception(b"ReflectionException", &err_msg, line);
                        vm.current_exception = Some(exc);
                        Some(Value::Null)
                    }
                }
                _ => None,
            }
        }
        "reflectionreference" => match method_lower {
            "fromarrayelement" => Some(reflection_reference_from_array_element(vm, args, line)),
            _ => None,
        },
        "reflectionclass" | "reflectionobject" => {
            match method_lower {
                "export" => {
                    // Deprecated, return null
                    Some(Value::Null)
                }
                _ => None,
            }
        }
        "reflectionenum" => match method_lower {
            "export" => Some(Value::Null),
            _ => None,
        },
        _ => None,
    }
}

/// Create a ReflectionClass object for a given class name
pub fn create_reflection_class(vm: &mut Vm, class_name: &str) -> Value {
    let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&class_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    // If the target class is an enum, materialize a ReflectionEnum so that
    // callers like `getDeclaringClass()::class` see the right runtime type.
    let is_enum = rcls(vm, &class_lower).map(|c| c.is_enum).unwrap_or(false);
    let reflector = if is_enum {
        b"ReflectionEnum".to_vec()
    } else {
        b"ReflectionClass".to_vec()
    };
    let obj_id = vm.alloc_object_id();
    let mut obj = PhpObject::new(reflector, obj_id);
    obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_msg(&class_name.to_string())),
    );
    obj.set_property(
        b"__reflection_target".to_vec(),
        Value::String(PhpString::from_msg(&class_name.to_string())),
    );
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Create a ReflectionMethod object for a given class and method name
pub fn create_reflection_method(vm: &mut Vm, class_name: &str, method_name: &str) -> Value {
    create_reflection_method_with_lookup(vm, class_name, method_name, None)
}

/// Create a ReflectionMethod with an optional lookup-class override.
/// `class_name` is the declaring class (used for `$class` / `__reflection_class`).
/// `lookup_class` is the class the method was originally fetched from; it
/// surfaces as `<user, inherits Declaring>` in __toString output when it
/// differs from the declaring class.
pub fn create_reflection_method_with_lookup(
    vm: &mut Vm,
    class_name: &str,
    method_name: &str,
    lookup_class: Option<&str>,
) -> Value {
    let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&class_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    let method_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&method_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();

    let obj_id = vm.alloc_object_id();
    let mut obj = PhpObject::new(b"ReflectionMethod".to_vec(), obj_id);

    let canonical_method = rcls(vm, &class_lower)
        .and_then(|c| {
            c.get_method(&method_lower)
                .map(|m| crate::string::msg_str(&m.name).to_string())
        })
        .unwrap_or_else(|| method_name.to_string());

    let canonical_class = rcls(vm, &class_lower)
        .map(|c| crate::string::msg_str(&c.name).to_string())
        .unwrap_or_else(|| class_name.to_string());

    obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_msg(&canonical_method)),
    );
    obj.set_property(
        b"class".to_vec(),
        Value::String(PhpString::from_msg(&canonical_class.clone())),
    );
    obj.set_property(
        b"__reflection_class".to_vec(),
        Value::String(PhpString::from_msg(&canonical_class)),
    );
    obj.set_property(
        b"__reflection_method".to_vec(),
        Value::String(PhpString::from_vec(method_lower)),
    );
    if let Some(lookup) = lookup_class {
        let lookup_canonical = {
            let lookup_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&lookup)
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            rcls(vm, &lookup_lower)
                .map(|c| crate::string::msg_str(&c.name).to_string())
                .unwrap_or_else(|| lookup.to_string())
        };
        obj.set_property(
            b"__reflection_lookup_class".to_vec(),
            Value::String(PhpString::from_msg(&lookup_canonical)),
        );
    }
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// ReflectionMethod for the `kind` ("get"/"set") hook of a property, named
/// `$prop::get` / `$prop::set` and owned by the hook's declaring class.
fn reflection_property_hook_method(
    vm: &mut Vm,
    class_lower: &[u8],
    prop: &[u8],
    kind: &str,
) -> Option<Value> {
    let mut m = format!("__property_{}_", kind).into_bytes();
    m.extend(prop.iter().map(|b| b.to_ascii_lowercase()));
    let decl = vm
        .classes
        .get(class_lower)?
        .get_method(&m)?
        .declaring_class
        .clone();
    let decl_display = vm
        .classes
        .get(&decl)
        .map(|c| crate::string::msg_str(&c.name).to_string())
        .unwrap_or_else(|| crate::string::msg_str(&decl).to_string());
    let method_name = crate::string::msg_str(&m).to_string();
    let rm = create_reflection_method(vm, &decl_display, &method_name);
    if let Value::Object(o) = &rm {
        o.borrow_mut().set_property(
            b"name".to_vec(),
            Value::String(PhpString::from_msg(&format!(
                "${}::{}",
                crate::string::msg_str(prop),
                kind
            ))),
        );
    }
    Some(rm)
}

/// Create a ReflectionProperty object
pub fn create_reflection_property(vm: &mut Vm, class_name: &str, prop_name: &str) -> Value {
    let obj_id = vm.alloc_object_id();
    let mut obj = PhpObject::new(b"ReflectionProperty".to_vec(), obj_id);
    obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_msg(&prop_name.to_string())),
    );
    obj.set_property(
        b"class".to_vec(),
        Value::String(PhpString::from_msg(&class_name.to_string())),
    );
    obj.set_property(
        b"__reflection_class".to_vec(),
        Value::String(PhpString::from_msg(&class_name.to_string())),
    );
    obj.set_property(
        b"__reflection_prop".to_vec(),
        Value::String(PhpString::from_msg(&prop_name.to_string())),
    );
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Create a ReflectionClassConstant object
pub fn create_reflection_class_constant(
    vm: &mut Vm,
    class_name: &str,
    const_name: &str,
    value: Value,
) -> Value {
    let obj_id = vm.alloc_object_id();
    let mut obj = PhpObject::new(b"ReflectionClassConstant".to_vec(), obj_id);
    obj.set_property(
        b"name".to_vec(),
        Value::String(PhpString::from_msg(&const_name.to_string())),
    );
    // Walk up the parent chain to find which class actually declares this
    // constant. The visible `class` property and getDeclaringClass() both
    // return the declaring class, not the class the reflection was made on.
    let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&class_name)
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    let const_lower: Vec<u8> = const_name.as_bytes().to_vec();
    let declaring = find_constant_declaring_class(vm, &class_lower, &const_lower)
        .unwrap_or_else(|| class_name.to_string());
    obj.set_property(
        b"class".to_vec(),
        Value::String(PhpString::from_msg(&declaring)),
    );
    obj.set_property(b"__reflection_value".to_vec(), value);
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Whether the class or an ancestor is an extension class constructed by
/// an object_construct_handler (no user __construct overriding it).
/// ReflectionFunction of a __call()/__callStatic() trampoline closure
/// (zend_get_call_trampoline_func()): an internal function named after the
/// called method; `...$arguments` when made by `$o->m(...)`.
#[inline(never)]
fn trampoline_function_method(vm: &Vm, method: &[u8], func_lower: &[u8]) -> Option<Value> {
    let op = vm.user_functions.get(func_lower)?;
    let target = op.fcc_target_name.as_ref()?;
    let name = match target.windows(2).rposition(|w| w == b"::") {
        Some(p) => &target[p + 2..],
        None => target.as_slice(),
    };
    let fcc = func_lower.starts_with(b"__closure_fcc_tramp_ip_")
        || func_lower.starts_with(b"__closure_fcc_tramp_sp_");
    let is_static = func_lower.starts_with(b"__closure_fcc_tramp_s");
    Some(match method {
        b"getname" | b"getshortname" => Value::String(PhpString::from_vec(name.to_vec())),
        b"isinternal" => Value::True,
        b"isuserdefined" => Value::False,
        b"getfilename" | b"getstartline" | b"getendline" | b"getdoccomment" => Value::False,
        b"__tostring" => {
            let mut out = format!(
                "Closure [ <internal> {}public method {} ] {{\n",
                if is_static { "static " } else { "" },
                crate::string::msg_str(name)
            );
            if fcc {
                out.push_str(
                    "\n  - Parameters [1] {\n    Parameter #0 [ <optional> mixed ...$arguments ]\n  }\n",
                );
            }
            out.push_str("}\n");
            Value::String(PhpString::from_msg(&out))
        }
        _ => return None,
    })
}

/// intern->obj of a class reflector: the object a ReflectionObject
/// reflects (a ReflectionClass made from an object keeps none).
fn reflected_instance(vm: &Vm, refl: &Rc<RefCell<PhpObject>>) -> Value {
    let ob = refl.borrow();
    let cl = ob.class_name.to_ascii_lowercase();
    if cl == b"reflectionobject" || vm.class_extends(&cl, b"reflectionobject") {
        ob.get_property(b"__reflection_object")
    } else {
        Value::Null
    }
}

/// The get_constructor() handler errors of internal classes (thrown
/// before the constructor's visibility is checked).
#[inline(never)]
fn internal_get_constructor_error(class_lower: &[u8]) -> Option<String> {
    match class_lower {
        b"closure" => Some("Instantiation of class Closure is not allowed".to_string()),
        b"generator" | b"fibererror" => Some(format!(
            "The \"{}\" class is reserved for internal use and cannot be manually instantiated",
            if class_lower == b"generator" {
                "Generator"
            } else {
                "FiberError"
            }
        )),
        _ => crate::vm::no_direct_construct_msg(class_lower).map(str::to_string),
    }
}

fn native_ctor_in_chain(vm: &Vm, class_lower: &[u8]) -> bool {
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        if vm.object_construct_handlers.contains_key(cur.as_slice()) {
            return true;
        }
        // (an extension class whose native handler serves __construct)
        if vm
            .native_class_handlers
            .get(cur.as_slice())
            .is_some_and(|h| (h.has_method)(&cur, b"__construct", false))
        {
            return true;
        }
        let Some(c) = vm.classes.get(&cur) else {
            return false;
        };
        if c.methods.contains_key(b"__construct".as_slice()) {
            return false;
        }
        match &c.parent {
            Some(p) => cur = p.to_ascii_lowercase(),
            None => return false,
        }
    }
    false
}

/// "Cannot instantiate ..." for classes object_init_ex() refuses.
fn instantiation_error(vm: &Vm, class_lower: &[u8]) -> Option<String> {
    let c = rcls(vm, class_lower)?;
    let kind = if c.is_interface {
        "interface"
    } else if c.is_trait {
        "trait"
    } else if c.is_enum {
        "enum"
    } else if c.is_abstract {
        "abstract class"
    } else {
        return None;
    };
    Some(format!(
        "Cannot instantiate {} {}",
        kind,
        crate::value::display_class_name(&c.name)
    ))
}

/// Walk a class's parent chain to find the class that declares a given
/// static property. Static properties are stored on the declaring class,
/// even when accessed via a subclass.
fn find_static_property_owner(vm: &Vm, class_lower: &[u8], prop_name: &[u8]) -> Option<Vec<u8>> {
    let mut owner: Option<Vec<u8>> = None;
    let mut cur: Option<Vec<u8>> = Some(class_lower.to_vec());
    let mut visited: Vec<Vec<u8>> = Vec::new();
    while let Some(c) = cur {
        if visited.contains(&c) {
            break;
        }
        visited.push(c.clone());
        if let Some(ce) = rcls(vm, c.as_slice()) {
            // The nearest class declaring it owns the static slot.
            if ce.properties.iter().any(|p| {
                p.is_static && p.name == prop_name && p.declaring_class.eq_ignore_ascii_case(&c)
            }) {
                return Some(c.clone());
            }
            if ce
                .properties
                .iter()
                .any(|p| p.is_static && p.name == prop_name)
            {
                owner = Some(c.clone());
            }
            cur = ce
                .parent
                .as_ref()
                .map(|p| p.iter().map(|b| b.to_ascii_lowercase()).collect());
        } else {
            break;
        }
    }
    owner
}

/// The static property `name` as seen from `class_lower`
/// (zend_std_get_static_property_with_info): an ancestor's private one is
/// not a property of the class.
fn visible_static_prop(
    vm: &Vm,
    class_lower: &[u8],
    name: &[u8],
) -> Option<crate::object::PropertyDef> {
    rcls(vm, class_lower)?
        .properties
        .iter()
        .find(|p| {
            p.is_static
                && p.name == name
                && (p.visibility != Visibility::Private
                    || p.declaring_class.eq_ignore_ascii_case(class_lower))
        })
        .cloned()
}

/// Walk a class's parent chain to find a static property's current value.
fn find_static_property_value(vm: &Vm, class_lower: &[u8], prop_name: &[u8]) -> Option<Value> {
    vm.static_prop_raw(class_lower, prop_name)
}

/// The class that declares the constant `const_name` as seen from
/// `class_lower` (ReflectionClassConstant::$class / getDeclaringClass()):
/// a redeclaring child is its own declarer, an inherited constant (from a
/// parent or an interface) reports where it was declared.
fn find_constant_declaring_class(vm: &Vm, class_lower: &[u8], const_name: &[u8]) -> Option<String> {
    let owner = constant_owner_class(vm, class_lower, const_name)?;
    let ce = rcls(vm, &owner)?;
    let decl = ce
        .constants_meta
        .get(const_name)
        .map(|m| m.declaring_class.to_ascii_lowercase())
        .filter(|d| !d.is_empty() && *d != owner);
    if let Some(d) = decl {
        if let Some(dc) = rcls(vm, &d) {
            return Some(crate::string::msg_str(&dc.name).to_string());
        }
    }
    Some(crate::string::msg_str(&ce.name).to_string())
}

/// Create ReflectionParameter objects for a function's parameters
pub fn create_reflection_parameters(vm: &mut Vm, op_array: &OpArray) -> Value {
    let mut result = PhpArray::new();
    let _func_name = crate::string::msg_str(&op_array.name).to_string();
    let func_lower: Vec<u8> = op_array
        .name
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    // A closure's leading CVs ($this, the use variables) are not parameters.
    let mut hidden = closure_hidden_params(op_array);
    let mut count = (op_array.param_count as usize).saturating_sub(hidden);
    // A first-class callable's forwarding closure / call trampoline:
    // `...$arguments` after its captured value.
    if let (true, Some(v)) = (
        op_array.name.starts_with(b"__closure_fcc_"),
        op_array.variadic_param,
    ) {
        hidden = v as usize;
        count = 1;
    }
    for i in 0..count {
        let cv = i + hidden;
        let param_name = if cv < op_array.cv_names.len() {
            crate::string::msg_str(&op_array.cv_names[cv]).to_string()
        } else {
            format!("param{}", i)
        };
        let obj_id = vm.alloc_object_id();
        let mut obj = PhpObject::new(b"ReflectionParameter".to_vec(), obj_id);
        obj.set_property(
            b"name".to_vec(),
            Value::String(PhpString::from_msg(&param_name)),
        );
        obj.set_property(
            b"__reflection_func".to_vec(),
            Value::String(PhpString::from_vec(func_lower.clone())),
        );
        obj.set_property(b"__reflection_param_idx".to_vec(), Value::Long(i as i64));
        if hidden > 0 {
            obj.set_property(
                b"__reflection_param_cv_offset".to_vec(),
                Value::Long(hidden as i64),
            );
        }
        result.push(Value::Object(Rc::new(RefCell::new(obj))));
    }
    Value::Array(Rc::new(RefCell::new(result)))
}

/// The count of a closure's leading CVs that are not parameters ($this and
/// the use variables); 0 for other functions.
pub fn closure_hidden_params(op_array: &OpArray) -> usize {
    if crate::closure::is_closure_fn_name(&op_array.name) {
        (op_array.param_count as usize).saturating_sub(op_array.param_attributes.len())
    } else {
        0
    }
}

/// Create ReflectionParameter objects for a method's parameters (skipping $this for non-static methods)
/// The signature reflection shows for a method an extension declares from
/// PHP source taking its arguments as `...$args`: its stub's arginfo.
/// The return type reflection shows for method `m` (see
/// reflect_sig_op_array()).
fn reflect_return_type(m: &crate::object::MethodDef) -> Option<ParamType> {
    match m.op_array.internal_sig {
        Some(f) if !m.op_array.ops.is_empty() => crate::stubs::parse_type(f.ret),
        _ => m.op_array.return_type.clone(),
    }
}

pub(crate) fn reflect_sig_op_array(oa: &OpArray) -> Option<OpArray> {
    let sig = oa.internal_sig?;
    // (an op array built from the stub already has its signature)
    if oa.ops.is_empty() {
        return None;
    }
    let mut o = crate::stubs::op_array_for(sig, oa.scope_class.as_deref(), true);
    o.name = oa.name.clone();
    o.scope_class = oa.scope_class.clone();
    o.attributes = oa.attributes.clone();
    Some(o)
}

pub fn create_reflection_parameters_method(
    vm: &mut Vm,
    op_array: &OpArray,
    is_static: bool,
) -> Value {
    let sig_oa = reflect_sig_op_array(op_array);
    let op_array = sig_oa.as_ref().unwrap_or(op_array);
    let mut result = PhpArray::new();
    let skip = if !is_static { 1usize } else { 0usize };
    let param_count = (op_array.param_count as usize).saturating_sub(skip);
    for i in 0..param_count {
        let cv_idx = i + skip;
        let param_name = if cv_idx < op_array.cv_names.len() {
            crate::string::msg_str(&op_array.cv_names[cv_idx]).to_string()
        } else {
            format!("param{}", i)
        };
        let obj_id = vm.alloc_object_id();
        let mut obj = PhpObject::new(b"ReflectionParameter".to_vec(), obj_id);
        obj.set_property(
            b"name".to_vec(),
            Value::String(PhpString::from_msg(&param_name)),
        );
        // Store the user-visible parameter position (0-based, excludes $this).
        obj.set_property(b"__reflection_param_idx".to_vec(), Value::Long(i as i64));
        // Store the CV index offset (1 for non-static methods because of $this,
        // 0 for static methods). Used to map back to the OpArray's cv_names /
        // param_types arrays where $this occupies slot 0 for non-static methods.
        obj.set_property(
            b"__reflection_param_cv_offset".to_vec(),
            Value::Long(skip as i64),
        );
        // Store class and method info so the OpArray can be re-fetched at call
        // time. Required for isOptional / hasType / getType / etc. on methods.
        if let Some(ref scope) = op_array.scope_class {
            obj.set_property(
                b"__reflection_param_class".to_vec(),
                Value::String(PhpString::from_vec(scope.clone())),
            );
            // A property hook (`$prop::get`) is registered as `__property_get_prop`.
            let n = &op_array.name;
            let method: Vec<u8> = match n.strip_prefix(b"$").and_then(|r| {
                r.strip_suffix(b"::get")
                    .map(|p| (b"get", p))
                    .or_else(|| r.strip_suffix(b"::set").map(|p| (b"set", p)))
            }) {
                Some((kind, prop)) => [b"__property_".as_slice(), kind, b"_", prop].concat(),
                None => n.clone(),
            };
            obj.set_property(
                b"__reflection_param_method".to_vec(),
                Value::String(PhpString::from_vec(
                    method.iter().map(|b| b.to_ascii_lowercase()).collect(),
                )),
            );
        }
        // Store param position (0-based, adjusted for $this skip)
        obj.set_property(
            b"__reflection_param_position".to_vec(),
            Value::Long(i as i64),
        );
        result.push(Value::Object(Rc::new(RefCell::new(obj))));
    }
    Value::Array(Rc::new(RefCell::new(result)))
}

/// Determine whether a ParamType permits null.
fn param_type_allows_null(pt: &ParamType) -> bool {
    match pt {
        ParamType::Simple(name) => matches!(name.as_slice(), b"null" | b"mixed"),
        ParamType::Nullable(_) => true,
        ParamType::Union(items) => items.iter().any(param_type_allows_null),
        ParamType::Intersection(_) => false,
    }
}

/// Create a ReflectionType object from a ParamType
pub fn create_reflection_type(vm: &mut Vm, param_type: &ParamType) -> Value {
    create_reflection_type_scoped(vm, param_type, None)
}

/// Create a ReflectionType with optional scope class name (canonical) used to
/// resolve `self` references in PHP types. `static` is left as the literal
/// string "static" (it's a late-bound type).
pub fn create_reflection_type_scoped(
    vm: &mut Vm,
    param_type: &ParamType,
    scope_class: Option<&[u8]>,
) -> Value {
    // Helper: resolve a simple type name with `self`/`parent` substitution.
    // `self` resolves to the declaring class; `parent` resolves to that class's
    // direct parent (canonical name preserved). `static` is left literal.
    fn resolve_name(vm: &Vm, name: &[u8], scope: Option<&[u8]>) -> Vec<u8> {
        // zend_compile_typename() keeps self/parent of anonymous classes
        // (GH-18373).
        if scope.is_some_and(|s| {
            s.contains(&0) || s.starts_with(b"class@anonymous") || s.starts_with(b"__anonymous")
        }) {
            return name.to_vec();
        }
        if name == b"self" {
            if let Some(s) = scope {
                return s.to_vec();
            }
        }
        if name == b"parent" {
            if let Some(s) = scope {
                let scope_lower: Vec<u8> = s.iter().map(|b| b.to_ascii_lowercase()).collect();
                if let Some(ce) = rcls(vm, &scope_lower) {
                    if let Some(parent) = ce.parent.as_ref() {
                        // Try canonical name from class entry
                        let parent_lower: Vec<u8> =
                            parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                        if let Some(parent_ce) = rcls(vm, &parent_lower) {
                            return parent_ce.name.clone();
                        }
                        return parent.clone();
                    }
                }
            }
        }
        name.to_vec()
    }
    match param_type {
        ParamType::Simple(name) => {
            let resolved = resolve_name(vm, name, scope_class);
            let type_name = crate::string::msg_str(&resolved).to_string();
            let obj_id = vm.alloc_object_id();
            let mut obj = PhpObject::new(b"ReflectionNamedType".to_vec(), obj_id);
            let is_builtin = matches!(
                name.as_slice(),
                b"int"
                    | b"float"
                    | b"string"
                    | b"bool"
                    | b"array"
                    | b"callable"
                    | b"void"
                    | b"null"
                    | b"mixed"
                    | b"never"
                    | b"object"
                    | b"iterable"
                    | b"false"
                    | b"true"
            );
            // `null` and `mixed` implicitly allow null
            let allows_null = matches!(name.as_slice(), b"null" | b"mixed");
            obj.set_property(
                b"__type_name".to_vec(),
                Value::String(PhpString::from_msg(&type_name)),
            );
            obj.set_property(
                b"__allows_null".to_vec(),
                if allows_null {
                    Value::True
                } else {
                    Value::False
                },
            );
            obj.set_property(
                b"__is_builtin".to_vec(),
                if is_builtin {
                    Value::True
                } else {
                    Value::False
                },
            );
            Value::Object(Rc::new(RefCell::new(obj)))
        }
        ParamType::Nullable(inner) => {
            match inner.as_ref() {
                ParamType::Simple(name) => {
                    let resolved = resolve_name(vm, name, scope_class);
                    let type_name = crate::string::msg_str(&resolved).to_string();
                    let obj_id = vm.alloc_object_id();
                    let mut obj = PhpObject::new(b"ReflectionNamedType".to_vec(), obj_id);
                    let is_builtin = matches!(
                        name.as_slice(),
                        b"int"
                            | b"float"
                            | b"string"
                            | b"bool"
                            | b"array"
                            | b"callable"
                            | b"void"
                            | b"null"
                            | b"mixed"
                            | b"never"
                            | b"object"
                            | b"iterable"
                            | b"false"
                            | b"true"
                    );
                    obj.set_property(
                        b"__type_name".to_vec(),
                        Value::String(PhpString::from_msg(&type_name)),
                    );
                    obj.set_property(b"__allows_null".to_vec(), Value::True);
                    obj.set_property(
                        b"__is_builtin".to_vec(),
                        if is_builtin {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                    Value::Object(Rc::new(RefCell::new(obj)))
                }
                _ => {
                    // Nullable complex type becomes union with null
                    let inner_type = create_reflection_type_scoped(vm, inner, scope_class);
                    let null_type = create_reflection_type_scoped(
                        vm,
                        &ParamType::Simple(b"null".to_vec()),
                        scope_class,
                    );
                    detach_type_handle(&inner_type);
                    detach_type_handle(&null_type);
                    let mut types = PhpArray::new();
                    types.push(inner_type);
                    types.push(null_type);
                    let obj_id = vm.alloc_object_id();
                    let mut obj = PhpObject::new(b"ReflectionUnionType".to_vec(), obj_id);
                    obj.set_property(
                        b"__types".to_vec(),
                        Value::Array(Rc::new(RefCell::new(types))),
                    );
                    obj.set_property(b"__allows_null".to_vec(), Value::True);
                    Value::Object(Rc::new(RefCell::new(obj)))
                }
            }
        }
        ParamType::Union(types) => {
            // PHP expands `iterable` inside a union to `Traversable|array`.
            let mut expanded: Vec<ParamType> = Vec::new();
            for t in types {
                match t {
                    ParamType::Simple(n) if n == b"iterable" => {
                        expanded.push(ParamType::Simple(b"Traversable".to_vec()));
                        expanded.push(ParamType::Simple(b"array".to_vec()));
                    }
                    _ => expanded.push(t.clone()),
                }
            }
            // Zend orders union members: class types (as declared), then
            // static, callable, object, array, string, int, float, bool, null.
            expanded.sort_by_key(|t| match t {
                ParamType::Simple(n) => match n.to_ascii_lowercase().as_slice() {
                    b"static" => 1,
                    b"callable" => 2,
                    b"object" => 3,
                    b"array" => 4,
                    b"string" => 5,
                    b"int" => 6,
                    b"float" => 7,
                    b"bool" => 8,
                    b"false" => 9,
                    b"true" => 10,
                    b"void" => 11,
                    b"never" => 12,
                    b"null" => 13,
                    b"mixed" => 14,
                    _ => 0,
                },
                _ => 0,
            });
            let types: &Vec<ParamType> = &expanded;
            // PHP normalizes `T|null` to nullable `?T` and exposes it as a
            // single ReflectionNamedType (not a union).
            let non_null: Vec<&ParamType> = types
                .iter()
                .filter(|t| !matches!(t, ParamType::Simple(n) if n == b"null"))
                .collect();
            let has_null = types
                .iter()
                .any(|t| matches!(t, ParamType::Simple(n) if n == b"null"));
            if has_null && non_null.len() == 1 {
                if let ParamType::Simple(name) = non_null[0] {
                    let resolved = resolve_name(vm, name, scope_class);
                    let type_name = crate::string::msg_str(&resolved).to_string();
                    let obj_id = vm.alloc_object_id();
                    let mut obj = PhpObject::new(b"ReflectionNamedType".to_vec(), obj_id);
                    let is_builtin = matches!(
                        name.as_slice(),
                        b"int"
                            | b"float"
                            | b"string"
                            | b"bool"
                            | b"array"
                            | b"callable"
                            | b"void"
                            | b"null"
                            | b"mixed"
                            | b"never"
                            | b"object"
                            | b"iterable"
                            | b"false"
                            | b"true"
                    );
                    obj.set_property(
                        b"__type_name".to_vec(),
                        Value::String(PhpString::from_msg(&type_name)),
                    );
                    obj.set_property(b"__allows_null".to_vec(), Value::True);
                    obj.set_property(
                        b"__is_builtin".to_vec(),
                        if is_builtin {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                    return Value::Object(Rc::new(RefCell::new(obj)));
                }
            }
            let mut type_arr = PhpArray::new();
            let mut allows_null = false;
            for t in types {
                let rt = create_reflection_type_scoped(vm, t, scope_class);
                if param_type_allows_null(t) {
                    allows_null = true;
                }
                detach_type_handle(&rt);
                type_arr.push(rt);
            }
            let obj_id = vm.alloc_object_id();
            let mut obj = PhpObject::new(b"ReflectionUnionType".to_vec(), obj_id);
            obj.set_property(
                b"__types".to_vec(),
                Value::Array(Rc::new(RefCell::new(type_arr))),
            );
            obj.set_property(
                b"__allows_null".to_vec(),
                if allows_null {
                    Value::True
                } else {
                    Value::False
                },
            );
            Value::Object(Rc::new(RefCell::new(obj)))
        }
        ParamType::Intersection(types) => {
            let mut type_arr = PhpArray::new();
            for t in types {
                let rt = create_reflection_type_scoped(vm, t, scope_class);
                detach_type_handle(&rt);
                type_arr.push(rt);
            }
            let obj_id = vm.alloc_object_id();
            let mut obj = PhpObject::new(b"ReflectionIntersectionType".to_vec(), obj_id);
            obj.set_property(
                b"__types".to_vec(),
                Value::Array(Rc::new(RefCell::new(type_arr))),
            );
            obj.set_property(b"__allows_null".to_vec(), Value::False);
            Value::Object(Rc::new(RefCell::new(obj)))
        }
    }
}

/// The member types of a union or intersection type are held without an
/// object handle: getTypes() makes objects of them (attach_type_handle()).
fn detach_type_handle(v: &Value) {
    if let Value::Object(o) = v {
        let id = std::mem::replace(&mut o.borrow_mut().object_id, 0);
        if id != 0 {
            crate::object::FREED_OBJECT_IDS.with(|ids| ids.borrow_mut().push(id));
        }
    }
}

/// A new ReflectionType object for member type `v` (see detach_type_handle()).
fn attach_type_handle(vm: &mut Vm, v: &Value) -> Value {
    let Value::Object(o) = v else {
        return v.clone();
    };
    let (class, props) = {
        let ob = o.borrow();
        let props: Vec<(Vec<u8>, Value)> = [
            b"__type_name".as_slice(),
            b"__allows_null",
            b"__is_builtin",
            b"__types",
        ]
        .iter()
        .filter(|k| ob.has_property(k))
        .map(|k| (k.to_vec(), ob.get_property(k)))
        .collect();
        (ob.class_name.clone(), props)
    };
    let id = vm.alloc_object_id();
    let mut obj = PhpObject::new(class, id);
    for (k, v) in props {
        obj.set_property(k, v);
    }
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// The class entry (lowercase name) that a constant lookup on `class_lower`
/// resolves to: the class itself, else the nearest ancestor declaring it
/// non-private (private constants are not inherited).
fn constant_owner_class(vm: &Vm, class_lower: &[u8], const_name: &[u8]) -> Option<Vec<u8>> {
    let mut cur = class_lower.to_vec();
    let mut first = true;
    for _ in 0..64 {
        let ce = rcls(vm, &cur)?;
        if ce.constants.contains_key(const_name) {
            let private = ce
                .constants_meta
                .get(const_name)
                .is_some_and(|m| m.visibility == Visibility::Private);
            if first || !private {
                return Some(cur);
            }
        }
        first = false;
        cur = ce.parent.as_ref()?.to_ascii_lowercase();
    }
    None
}

/// Helper to check if a class has a constant (walks parent chain)
fn reflection_class_has_constant(vm: &Vm, class_lower: &[u8], const_name: &[u8]) -> bool {
    constant_owner_class(vm, class_lower, const_name).is_some()
}

/// Helper to get a constant from a class (walks parent chain). For enum
/// cases stored as the marker `__enum_case__::Name`, resolve to the actual
/// enum case object.
fn reflection_class_get_constant(
    vm: &mut Vm,
    class_lower: &[u8],
    const_name: &[u8],
) -> Option<Value> {
    let owner = constant_owner_class(vm, class_lower, const_name)?;
    let raw = rcls(vm, &owner)?.constants.get(const_name)?.clone();
    // Resolve the enum-case marker to the actual case object.
    if let Value::String(ref s) = raw {
        let sb = s.as_bytes();
        if sb.starts_with(b"__enum_case__::") {
            let case_name = &sb[b"__enum_case__::".len()..];
            if let Some(case_obj) = vm.get_enum_case(&owner, case_name) {
                return Some(case_obj);
            }
        }
    }
    Some(raw)
}

/// Collect parent constants into the result array
fn reflection_collect_parent_constants(vm: &Vm, class_lower: &[u8], result: &mut PhpArray) {
    if let Some(ce) = rcls(vm, class_lower) {
        if let Some(ref parent) = ce.parent {
            let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(pce) = rcls(vm, &parent_lower) {
                for (name, val) in &pce.constants {
                    // Private constants are not inherited.
                    if pce
                        .constants_meta
                        .get(name)
                        .is_some_and(|m| m.visibility == Visibility::Private)
                    {
                        continue;
                    }
                    let key = ArrayKey::String(PhpString::from_vec(name.clone()));
                    if result.get(&key).is_none() {
                        result.set(key, val.deref());
                    }
                }
            }
            reflection_collect_parent_constants(vm, &parent_lower, result);
        }
    }
}

/// Get modifier flags for a method
fn reflection_method_modifiers(method: &crate::object::MethodDef) -> i64 {
    reflection_method_modifiers_static(method)
}

/// Get modifier flags for a method (static version for use in closures)
fn reflection_method_modifiers_static(method: &crate::object::MethodDef) -> i64 {
    let mut mods = 0i64;
    match method.visibility {
        Visibility::Public => mods |= 1,    // IS_PUBLIC
        Visibility::Protected => mods |= 2, // IS_PROTECTED
        Visibility::Private => mods |= 4,   // IS_PRIVATE
    }
    if method.is_static {
        mods |= 0x10;
    } // IS_STATIC
    if method.is_abstract {
        mods |= 0x40;
    } // IS_ABSTRACT
    if method.is_final {
        mods |= 0x20;
    } // IS_FINAL
    mods
}

/// Whether a property is virtual: declared so, or an internal class's
/// property its stub marks virtual although goro gives it storage
/// (BcMath\\Number's value/scale).
fn prop_is_virtual(prop: &crate::object::PropertyDef) -> bool {
    prop.is_virtual
        || crate::stubs::find_class_lower(&prop.declaring_class)
            .or_else(|| crate::stubs::find_self_declared_class(&prop.declaring_class))
            .is_some_and(|c| {
                c.props.iter().any(|sp| {
                    sp.name.as_bytes() == prop.name.as_slice()
                        && sp.flags & crate::stubs::F_VIRTUAL != 0
                })
            })
}

/// Get modifier flags for a property
fn reflection_property_modifiers(vm: &Vm, prop: &crate::object::PropertyDef) -> i64 {
    let mut mods = reflection_property_modifiers_static(prop);
    // An interface's hooked property is abstract (zend_compile_prop_decl).
    if (prop.has_get_hook || prop.has_set_hook)
        && rcls(vm, &prop.declaring_class.to_ascii_lowercase()).is_some_and(|c| c.is_interface)
    {
        mods |= 0x40;
    }
    mods
}

/// Get modifier flags for a property (static version for use in closures)
fn reflection_property_modifiers_static(prop: &crate::object::PropertyDef) -> i64 {
    let mut mods = 0i64;
    match prop.visibility {
        Visibility::Public => mods |= 1,
        Visibility::Protected => mods |= 2,
        Visibility::Private => mods |= 4,
    }
    if prop.is_static {
        mods |= 0x10;
    }
    if prop.is_final {
        mods |= 0x20;
    }
    if prop.is_abstract {
        mods |= 0x40;
    }
    if prop_is_virtual(prop) {
        mods |= 0x200;
    }
    if prop.is_readonly {
        mods |= 0x80; // ZEND_ACC_READONLY
    }
    // Set visibility (zend_declare_typed_property): public readonly implies
    // protected(set); private(set) implies final.
    match prop.set_visibility {
        Some(Visibility::Private) if prop.visibility != Visibility::Private => {
            mods |= 0x1000 | 0x20;
        }
        Some(Visibility::Protected) if prop.visibility == Visibility::Public => mods |= 0x800,
        None if prop.is_readonly && prop.visibility == Visibility::Public => mods |= 0x800,
        _ => {}
    }
    mods
}

/// Selector for which reflector class is producing the __toString output.
/// Affects the header line: `Class [ ... ]`, `Object of class [ ... ]`,
/// or `Enum [ ... ]`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReflectionStringKind {
    Class,
    Object,
    Enum,
}

/// Build the complete __toString representation for a ReflectionClass
fn reflection_class_to_string(
    vm: &Vm,
    name: &str,
    class_lower: &[u8],
    kind: ReflectionStringKind,
    wrapped_obj: Option<&std::rc::Rc<std::cell::RefCell<PhpObject>>>,
) -> String {
    let _ = name;
    let obj = if kind == ReflectionStringKind::Object {
        wrapped_obj
    } else {
        None
    };
    class_string(vm, class_lower, obj, "")
}

/// Convert a ParamType to its string representation
fn param_type_name(pt: &crate::opcode::ParamType) -> String {
    use crate::opcode::ParamType;
    match pt {
        ParamType::Simple(name) => crate::string::msg_str(name).to_string(),
        ParamType::Nullable(inner) => format!("?{}", param_type_name(inner)),
        ParamType::Union(types) => {
            let names: Vec<String> = types.iter().map(param_type_name).collect();
            names.join("|")
        }
        ParamType::Intersection(types) => {
            let names: Vec<String> = types.iter().map(param_type_name).collect();
            names.join("&")
        }
    }
}

/// zend_wrong_parameters_count_error() of an internal instance method
/// called with `n` arguments, from its stub (true when it threw).
#[inline(never)]
pub fn stub_method_arity_error(vm: &mut Vm, class_lower: &[u8], method: &[u8], n: usize) -> bool {
    use crate::stubs::{A_VARIADIC, F_DEPRECATED, F_STATIC};
    let Some((c, f)) = crate::stubs::find_method(class_lower, method) else {
        return false;
    };
    // (a deprecated method's deprecation comes before: left to the method)
    if f.flags & (F_STATIC | F_DEPRECATED) != 0 {
        return false;
    }
    let min = f.required_args();
    let max = if f.args.last().is_some_and(|a| a.flags & A_VARIADIC != 0) {
        None
    } else {
        Some(f.args.len())
    };
    if n >= min && max.is_none_or(|m| n <= m) {
        return false;
    }
    let (kind, k) = if max == Some(min) {
        ("exactly", min)
    } else if n < min {
        ("at least", min)
    } else {
        ("at most", max.unwrap_or(min))
    };
    let msg = format!(
        "{}::{}() expects {} {} argument{}, {} given",
        c.name,
        f.name,
        kind,
        k,
        if k == 1 { "" } else { "s" },
        n
    );
    let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    true
}

/// zpp "o!" of a method's optional object argument: "must be of type
/// ?object" (true when it threw).
#[inline(never)]
fn opt_object_arg_error(vm: &mut Vm, method: &str, v: Option<&Value>) -> bool {
    let Some(v) = v.map(|v| v.deref()) else {
        return false;
    };
    if matches!(v, Value::Null | Value::Object(_)) {
        return false;
    }
    let msg = format!(
        "{}(): Argument #1 ($object) must be of type ?object, {} given",
        method,
        Vm::value_type_name(&v)
    );
    let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    true
}

/// Create a PHP array of ReflectionAttribute objects from a slice of RuntimeAttributes.
/// Optionally filter by attribute name and flags.
pub fn create_reflection_attributes(
    vm: &mut Vm,
    attrs: &[RuntimeAttribute],
    filter_name: Option<&[u8]>,
    filter_flags: i64,
    target: i64,
) -> Value {
    let mut result = PhpArray::new();
    // Validate flags: only 0 (default) and 2 (IS_INSTANCEOF) are allowed.
    if filter_flags != 0 && filter_flags != 2 {
        let line = vm.current_line;
        // (the class declaring the getAttributes() called)
        let class = match target {
            1 => "ReflectionClass",
            8 => "ReflectionProperty",
            16 => "ReflectionClassConstant",
            32 => "ReflectionParameter",
            64 => "ReflectionConstant",
            _ => "ReflectionFunctionAbstract",
        };
        let exc = vm.create_exception(
            b"ValueError",
            &format!(
                "{}::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag",
                class
            ),
            line,
        );
        vm.current_exception = Some(exc);
        return Value::Null;
    }
    // Resolve filter_name; if given and IS_INSTANCEOF specified, the class
    // must exist or throw an error.
    if filter_flags == 2 {
        if let Some(fname) = filter_name {
            // zend_lookup_class(name): autoloaded if needed.
            let bare = fname.strip_prefix(b"\\").unwrap_or(fname);
            let fl: Vec<u8> = bare.to_ascii_lowercase();
            if !vm.classes.contains_key(&fl) && !vm.is_known_builtin_class(&fl) {
                vm.try_autoload_class(bare);
                if vm.current_exception.is_some() {
                    return Value::Null;
                }
            }
            let exists = vm.classes.contains_key(&fl) || vm.is_known_builtin_class(&fl);
            if !exists {
                let display = crate::string::msg_str(fname);
                let line = vm.current_line;
                let exc = vm.create_exception(
                    b"Error",
                    &format!("Class \"{}\" not found", display),
                    line,
                );
                vm.current_exception = Some(exc);
                return Value::Null;
            }
        }
    }
    for attr in attrs {
        // If a filter name is specified, check it
        if let Some(fname) = filter_name {
            if filter_flags == 0 {
                // Exact match (default)
                if !attr.name.eq_ignore_ascii_case(fname) {
                    continue;
                }
            } else {
                // IS_INSTANCEOF = 2: check if attribute class is the given class
                // or any subclass/implementer (transitive).
                let fname = fname.strip_prefix(b"\\").unwrap_or(fname);
                if !attr.name.eq_ignore_ascii_case(fname) {
                    let attr_lower: Vec<u8> =
                        attr.name.iter().map(|b| b.to_ascii_lowercase()).collect();
                    // read_attributes(): zend_lookup_class_ex(attr->name):
                    // the attribute's class is autoloaded (and skipped when
                    // it does not exist).
                    if !vm.classes.contains_key(&attr_lower)
                        && !vm.is_known_builtin_class(&attr_lower)
                    {
                        vm.try_autoload_class(&attr.name);
                        if vm.current_exception.is_some() {
                            return Value::Null;
                        }
                    }
                    let filter_lower: Vec<u8> =
                        fname.iter().map(|b| b.to_ascii_lowercase()).collect();
                    // Walk attr class + ancestors checking class + interfaces (transitively).
                    let mut visited: std::collections::HashSet<Vec<u8>> =
                        std::collections::HashSet::new();
                    let mut queue: Vec<Vec<u8>> = vec![attr_lower];
                    let mut is_subclass = false;
                    while let Some(cur) = queue.pop() {
                        if !visited.insert(cur.clone()) {
                            continue;
                        }
                        if cur == filter_lower {
                            is_subclass = true;
                            break;
                        }
                        if let Some(ce) = rcls(vm, &cur) {
                            if let Some(p) = &ce.parent {
                                let pl: Vec<u8> =
                                    p.iter().map(|b| b.to_ascii_lowercase()).collect();
                                queue.push(pl);
                            }
                            for iface in &ce.interfaces {
                                let il: Vec<u8> =
                                    iface.iter().map(|b| b.to_ascii_lowercase()).collect();
                                queue.push(il);
                            }
                        } else if crate::vm::is_builtin_subclass(&cur, &filter_lower) {
                            is_subclass = true;
                            break;
                        }
                    }
                    if !is_subclass {
                        continue;
                    }
                }
            }
        }
        let obj_id = vm.alloc_object_id();
        let mut obj = PhpObject::new(b"ReflectionAttribute".to_vec(), obj_id);
        obj.set_property(
            b"name".to_vec(),
            Value::String(PhpString::from_vec(attr.name.clone())),
        );
        obj.set_property(
            b"__reflection_attr_name".to_vec(),
            Value::String(PhpString::from_vec(attr.name.clone())),
        );
        // Store the op_array for lazy argument evaluation
        obj.set_property(b"__reflection_attr_target".to_vec(), Value::Long(target));
        // Store args_op_array reference as a serialized form
        // We'll evaluate it lazily when getArguments() is called
        // For now, eagerly evaluate it
        // Push class scope if the attribute was defined in a class context
        // Arguments are evaluated lazily (getArguments()/newInstance()),
        // like PHP; __toString() renders them unevaluated.
        let idx = ATTR_ARG_OPS.with(|r| {
            let mut r = r.borrow_mut();
            r.push((*attr.args_op_array).clone());
            r.len() - 1
        });
        obj.set_property(
            b"__reflection_attr_args_op".to_vec(),
            Value::Long(idx as i64),
        );
        let mut reprs = PhpArray::new();
        for r in &attr.args_op_array.param_default_repr {
            reprs.push(match r {
                Some(s) => Value::String(PhpString::from_msg(&s.clone())),
                None => Value::Null,
            });
        }
        obj.set_property(
            b"__reflection_attr_reprs".to_vec(),
            Value::Array(Rc::new(RefCell::new(reprs))),
        );
        // Check if the attribute is repeated (count how many times this name appears)
        let repeat_count = attrs
            .iter()
            .filter(|a| a.name.eq_ignore_ascii_case(&attr.name))
            .count();
        obj.set_property(
            b"__reflection_attr_repeated".to_vec(),
            if repeat_count > 1 {
                Value::True
            } else {
                Value::False
            },
        );
        if let Some(e) = &attr.validation_error {
            obj.set_property(
                b"__reflection_attr_validation_error".to_vec(),
                Value::String(PhpString::from_msg(&e.clone())),
            );
        }
        result.push(Value::Object(Rc::new(RefCell::new(obj))));
    }
    Value::Array(Rc::new(RefCell::new(result)))
}

thread_local! {
    /// Argument op arrays of ReflectionAttribute objects, evaluated lazily.
    static ATTR_ARG_OPS: RefCell<Vec<OpArray>> = const { RefCell::new(Vec::new()) };
}

/// The (lazily evaluated) arguments array of a ReflectionAttribute.
/// None if evaluating them threw.
/// Set the class scope the attribute arguments of the ReflectionAttribute
/// (or ReflectionParameter) objects in `list` are evaluated in: a trait
/// method's attributes see the using class, a closure's its bound scope.
fn set_attr_scope(list: &Value, scope: Option<Vec<u8>>, prop: &[u8]) {
    let (Some(scope), Value::Array(a)) = (scope, list) else {
        return;
    };
    for v in a.borrow().values() {
        if let Value::Object(o) = v {
            o.borrow_mut().set_property(
                prop.to_vec(),
                Value::String(PhpString::from_vec(scope.to_ascii_lowercase())),
            );
        }
    }
}

fn attr_args_value(vm: &mut Vm, obj: &Rc<RefCell<PhpObject>>) -> Option<Value> {
    // Evaluated on every call: `new` in arguments yields fresh objects.
    let idx = obj
        .borrow()
        .get_property(b"__reflection_attr_args_op")
        .to_long() as usize;
    let mut op = ATTR_ARG_OPS.with(|r| r.borrow().get(idx).cloned())?;
    let scope_override = match obj.borrow().get_property(b"__reflection_attr_scope") {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    };
    if let Some(s) = &scope_override {
        op.scope_class = Some(s.clone());
    }
    let pushed_scope = if let Some(ref scope) = scope_override.or_else(|| op.scope_class.clone()) {
        vm.push_class_scope(scope.clone());
        vm.called_class_stack.push(scope.clone());
        true
    } else {
        false
    };
    let r = vm.execute_op_array_pub(&op, vec![]);
    if pushed_scope {
        vm.pop_class_scope();
        vm.called_class_stack.pop();
    }
    if vm.current_exception.is_some() {
        return None;
    }
    // (not kept: the arguments are the caller's)
    Some(r.unwrap_or_else(|_| Value::Array(Rc::new(RefCell::new(PhpArray::new())))))
}

/// Handle method calls on ReflectionAttribute objects
pub fn reflection_attribute_method(
    vm: &mut Vm,
    method: &[u8],
    obj: &Rc<RefCell<PhpObject>>,
) -> Option<Value> {
    let ob = obj.borrow();
    match method {
        b"__construct" => {
            drop(ob);
            let exc = vm.create_exception(
                b"ReflectionException",
                "Cannot directly instantiate ReflectionAttribute",
                vm.current_line,
            );
            vm.current_exception = Some(exc);
            Some(Value::Null)
        }
        b"getname" => {
            let name = ob.get_property(b"__reflection_attr_name");
            Some(name)
        }
        b"getarguments" => {
            drop(ob);
            let args = match attr_args_value(vm, obj) {
                Some(a) => a,
                None => return Some(Value::Null),
            };
            // Return the arguments array directly
            match &args {
                Value::Array(arr) => {
                    let borrowed = arr.borrow();
                    let mut result = PhpArray::new();
                    for (key, val) in borrowed.iter() {
                        result.set(key.clone(), val.clone());
                    }
                    Some(Value::Array(Rc::new(RefCell::new(result))))
                }
                _ => Some(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
            }
        }
        b"gettarget" => {
            let target = ob.get_property(b"__reflection_attr_target");
            Some(target)
        }
        b"isrepeated" => {
            let repeated = ob.get_property(b"__reflection_attr_repeated");
            Some(repeated)
        }
        b"newinstance" => {
            let attr_name = ob
                .get_property(b"__reflection_attr_name")
                .to_php_string()
                .to_msg_string();
            drop(ob);
            // zend_lookup_class(attr->name) comes first: the attribute's
            // class is autoloaded before its arguments are evaluated.
            {
                let lower = attr_name.to_ascii_lowercase().into_bytes();
                if !vm.classes.contains_key(&lower) && !vm.is_known_builtin_class(&lower) {
                    vm.try_autoload_class(&crate::errdisplay::raw_error_bytes(&attr_name));
                    if vm.current_exception.is_some() {
                        return Some(Value::Null);
                    }
                }
            }
            let args = match attr_args_value(vm, obj) {
                Some(a) => a,
                None => return Some(Value::Null),
            };
            // Capture the call-site line before any inner op-array execution
            // (e.g. evaluating #[Attribute(...)] args) overwrites self.current_line.
            let call_site_line = vm.current_line;
            // Validators of internal attributes whose validation was delayed
            // (#[\DelayedTargetValidation]) run now.
            if obj
                .borrow()
                .get_property(b"__reflection_attr_target")
                .to_long()
                == 1
            {
                let target_class = obj
                    .borrow()
                    .get_property(b"__reflection_attr_class")
                    .to_php_string()
                    .as_bytes()
                    .to_vec();
                let msg = rcls(vm, &target_class).and_then(|ce| {
                    Vm::class_attribute_validation_error(
                        ce,
                        &attr_name.to_ascii_lowercase().into_bytes(),
                    )
                });
                if let Some(msg) = msg {
                    let exc = vm.create_exception(b"Error", &msg, call_site_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
            }
            // Create a new instance of the attribute class
            let class_lower: Vec<u8> = crate::errdisplay::raw_error_bytes(&attr_name)
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            // The target of an internal attribute is validated now
            // (zend_get_attribute_object(): delayed with #[\DelayedTargetValidation]).
            let target_name = match obj
                .borrow()
                .get_property(b"__reflection_attr_target")
                .to_long()
            {
                1 => "class",
                2 => "function",
                4 => "method",
                8 => "property",
                16 => "class constant",
                32 => "parameter",
                64 => "constant",
                _ => "",
            };
            if !target_name.is_empty() {
                if let Some(msg) = crate::compiler::Compiler::validate_internal_attr(
                    attr_name.as_bytes(),
                    target_name,
                ) {
                    let exc = vm.create_exception(b"Error", &msg, call_site_line);
                    vm.current_exception = Some(exc);
                    return Some(Value::Null);
                }
            }
            // Built-in attribute classes: AllowDynamicProperties, Override,
            // SensitiveParameter, ReturnTypeWillChange, DelayedTargetValidation
            // are marker attributes that have no constructor body.
            // Their newInstance() returns an empty object of the right type.
            let builtin_marker_attrs: &[&[u8]] = &[
                b"allowdynamicproperties",
                b"override",
                b"sensitiveparameter",
                b"returntypewillchange",
                b"delayedtargetvalidation",
            ];
            if builtin_marker_attrs.contains(&class_lower.as_slice())
                && !vm.classes.contains_key(&class_lower)
            {
                let obj_id = vm.alloc_object_id();
                // Preserve canonical PHP class name casing
                let canonical_name: &[u8] = match class_lower.as_slice() {
                    b"allowdynamicproperties" => b"AllowDynamicProperties",
                    b"override" => b"Override",
                    b"sensitiveparameter" => b"SensitiveParameter",
                    b"returntypewillchange" => b"ReturnTypeWillChange",
                    b"delayedtargetvalidation" => b"DelayedTargetValidation",
                    _ => unreachable!(),
                };
                let new_obj = PhpObject::new(canonical_name.to_vec(), obj_id);
                return Some(Value::Object(Rc::new(RefCell::new(new_obj))));
            }
            // A validator error delayed by #[\DelayedTargetValidation].
            if let Value::String(e) = obj
                .borrow()
                .get_property(b"__reflection_attr_validation_error")
            {
                let exc = vm.create_exception(b"Error", &e.to_msg_string(), call_site_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            // Internal attribute classes (e.g. #[\Deprecated], #[\NoDiscard]).
            if vm
                .classes
                .get(&class_lower)
                .map(|c| c.filename.is_none())
                .unwrap_or(true)
                && internal_class_known(vm, &class_lower)
            {
                if let Some(sc) = crate::stubs::find_class(&class_lower)
                    .filter(|c| c.flags & crate::stubs::F_ATTRIBUTE != 0)
                {
                    // Map named arguments onto the stub constructor's positions.
                    let mut positional: Vec<Value> = Vec::new();
                    let mut named: Vec<(Vec<u8>, Value)> = Vec::new();
                    if let Value::Array(a) = &args {
                        for (k, v) in a.borrow().iter() {
                            match k {
                                ArrayKey::Int(_) => positional.push(v.clone()),
                                ArrayKey::String(n) => {
                                    named.push((n.as_bytes().to_vec(), v.clone()))
                                }
                            }
                        }
                    }
                    if let Some((_, ctor)) =
                        crate::stubs::find_method(sc.name.as_bytes(), b"__construct")
                    {
                        for (n, v) in named {
                            let pos = match ctor
                                .args
                                .iter()
                                .position(|a| a.name.as_bytes() == n.as_slice())
                            {
                                Some(p) => p,
                                None => {
                                    let msg = format!(
                                        "Unknown named parameter ${}",
                                        crate::string::msg_str(&n)
                                    );
                                    let exc = vm.create_exception(b"Error", &msg, call_site_line);
                                    vm.current_exception = Some(exc);
                                    return Some(Value::Null);
                                }
                            };
                            while positional.len() < pos {
                                let i = positional.len();
                                let d = ctor
                                    .args
                                    .get(i)
                                    .and_then(|a| a.default)
                                    .and_then(|d| eval_internal_default(vm, d, None))
                                    .unwrap_or(Value::Null);
                                positional.push(d);
                            }
                            if positional.len() == pos {
                                positional.push(v);
                            } else {
                                positional[pos] = v;
                            }
                        }
                    }
                    let mut list = PhpArray::new();
                    for v in positional {
                        list.push(v);
                    }
                    let src = format!("return new \\{}(...$__a);", sc.name);
                    let v = run_snippet_in_scope(
                        vm,
                        &src,
                        &[("__a", Value::Array(Rc::new(RefCell::new(list))))],
                        b"",
                    );
                    return Some(v.unwrap_or(Value::Null));
                }
            }
            if !vm.classes.contains_key(&class_lower) {
                let err_msg = format!("Attribute class \"{}\" not found", attr_name);
                let exc = vm.create_exception(b"Error", &err_msg, call_site_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            // Check if the class has #[Attribute] -- only attribute classes can be instantiated via newInstance
            let attribute_args_op = rcls(vm, &class_lower)
                .and_then(|ce| {
                    ce.attributes
                        .iter()
                        .find(|a| a.name.eq_ignore_ascii_case(b"attribute"))
                })
                .map(|a| a.args_op_array.clone());
            let is_attribute_class = attribute_args_op.is_some();
            if !is_attribute_class && class_lower != b"attribute" {
                let err_msg = format!(
                    "Attempting to use non-attribute class \"{}\" as attribute",
                    attr_name
                );
                let exc = vm.create_exception(b"Error", &err_msg, call_site_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            // Validate the #[Attribute(...)] flags on this attribute class.
            let mut attr_flags: i64 = 127; // Default TARGET_ALL
            if let Some(args_op) = attribute_args_op {
                let args_val = vm
                    .execute_op_array_pub(&args_op, vec![])
                    .unwrap_or(Value::Null);
                // Restore the call-site line after the inner op-array updated it.
                vm.current_line = call_site_line;
                if vm.current_exception.is_some() {
                    return Some(Value::Null);
                }
                if let Value::Array(arr) = &args_val {
                    let arr_b = arr.borrow();
                    if let Some((_, flag_val)) = arr_b.iter().next() {
                        // Type-check first: must be int
                        if !matches!(flag_val, Value::Long(_) | Value::Null) {
                            // zend_zval_value_name()
                            let actual_type = Vm::value_type_name(flag_val);
                            let err_msg = format!(
                                "Attribute::__construct(): Argument #1 ($flags) must be of type int, {} given",
                                actual_type
                            );
                            // (zend_get_attribute_target_names: zend_throw_error)
                            let exc = vm.create_exception(b"Error", &err_msg, call_site_line);
                            vm.current_exception = Some(exc);
                            return Some(Value::Null);
                        }
                        // Value range check: must be 0..=255 (TARGET_ALL=127 | IS_REPEATABLE=128)
                        if let Value::Long(n) = flag_val {
                            if *n < 0 || *n > (127 | 128) {
                                let err_msg = "Invalid attribute flags specified".to_string();
                                let exc =
                                    vm.create_exception(b"ValueError", &err_msg, call_site_line);
                                vm.current_exception = Some(exc);
                                return Some(Value::Null);
                            }
                            attr_flags = *n;
                        }
                    }
                }
            }
            // Check target compatibility. The attribute object stores
            // `__attr_target` as the active target bit (e.g. TARGET_CLASS=1).
            let active_target = {
                let ob2 = obj.borrow();
                ob2.get_property(b"__reflection_attr_target").to_long()
            };
            let target_mask = attr_flags & 127;
            if active_target != 0 && (target_mask & active_target) == 0 {
                let target_str = match active_target {
                    1 => "class",
                    2 => "function",
                    4 => "method",
                    8 => "property",
                    16 => "class constant",
                    32 => "parameter",
                    64 => "constant",
                    _ => "unknown",
                };
                let mut allowed: Vec<&str> = Vec::new();
                if target_mask & 1 != 0 {
                    allowed.push("class");
                }
                if target_mask & 2 != 0 {
                    allowed.push("function");
                }
                if target_mask & 4 != 0 {
                    allowed.push("method");
                }
                if target_mask & 8 != 0 {
                    allowed.push("property");
                }
                if target_mask & 16 != 0 {
                    allowed.push("class constant");
                }
                if target_mask & 32 != 0 {
                    allowed.push("parameter");
                }
                if target_mask & 64 != 0 {
                    allowed.push("constant");
                }
                let err_msg = format!(
                    "Attribute \"{}\" cannot target {} (allowed targets: {})",
                    attr_name,
                    target_str,
                    allowed.join(", ")
                );
                let exc = vm.create_exception(b"Error", &err_msg, call_site_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            // Check repetition: if the attribute is repeated but not flagged
            // as IS_REPEATABLE, throw an error.
            let is_repeated = {
                let ob2 = obj.borrow();
                matches!(ob2.get_property(b"__reflection_attr_repeated"), Value::True)
            };
            if is_repeated && (attr_flags & 128) == 0 {
                let err_msg = format!("Attribute \"{}\" must not be repeated", attr_name);
                let exc = vm.create_exception(b"Error", &err_msg, call_site_line);
                vm.current_exception = Some(exc);
                return Some(Value::Null);
            }
            // zend_get_attribute_object(): the constructor is called (with
            // the usual argument checks and named-argument handling) from a
            // frame at the attribute's file and line, with the use-site
            // strict_types.
            let site = attr_use_site(obj);
            let ce_name = rcls(vm, &class_lower)
                .map(|c| crate::string::msg_str(&c.name).to_string())
                .unwrap_or_else(|| attr_name.to_string());
            let v = run_snippet_at(
                vm,
                &format!("return new \\{}(...$__a);", ce_name),
                &[("__a", args)],
                site.0,
                site.1.unwrap_or(call_site_line),
                site.2,
            );
            Some(v.unwrap_or(Value::Null))
        }
        b"__tostring" => {
            let name = ob
                .get_property(b"__reflection_attr_name")
                .to_php_string()
                .to_msg_string();
            let reprs: Vec<Value> = match ob.get_property(b"__reflection_attr_reprs") {
                Value::Array(a) => a.borrow().values().cloned().collect(),
                _ => Vec::new(),
            };
            drop(ob);
            let mut out = format!("Attribute [ {} ]", name);
            if reprs.is_empty() {
                out.push('\n');
            } else {
                // Arguments without an AST rendering print their value.
                let evaluated: Vec<Value> = if reprs.iter().any(|r| matches!(r, Value::Null)) {
                    match attr_args_value(vm, obj) {
                        Some(Value::Array(a)) => a.borrow().values().cloned().collect(),
                        Some(_) => Vec::new(),
                        None => return Some(Value::Null),
                    }
                } else {
                    Vec::new()
                };
                out.push_str(" {\n");
                out.push_str(&format!("  - Arguments [{}] {{\n", reprs.len()));
                for (i, r) in reprs.iter().enumerate() {
                    let text = match r {
                        Value::String(s) => s.to_msg_string(),
                        _ => evaluated
                            .get(i)
                            .map(format_default_value)
                            .unwrap_or_default(),
                    };
                    out.push_str(&format!("    Argument #{} [ {} ]\n", i, text));
                }
                out.push_str("  }\n}\n");
            }
            Some(Value::String(PhpString::from_msg(&out)))
        }
        _ => None,
    }
}

/// Determine which extension a built-in function belongs to
fn get_function_extension(func_name: &str) -> Option<&'static str> {
    // Extensions whose functions are only known from their stubs.
    if let Some(f) = crate::stubs::find_function(func_name.as_bytes()) {
        if matches!(f.ext, "exif" | "zip" | "fileinfo" | "gd") {
            return Some(f.ext);
        }
    }
    // Standard library functions
    let standard = [
        "array_shift",
        "array_unshift",
        "array_pop",
        "array_push",
        "array_splice",
        "array_slice",
        "array_merge",
        "array_combine",
        "array_chunk",
        "array_unique",
        "array_flip",
        "array_reverse",
        "array_keys",
        "array_values",
        "array_search",
        "array_map",
        "array_filter",
        "array_reduce",
        "array_walk",
        "array_walk_recursive",
        "array_column",
        "array_fill",
        "array_fill_keys",
        "array_pad",
        "array_rand",
        "array_sum",
        "array_product",
        "array_count_values",
        "array_diff",
        "array_diff_key",
        "array_diff_assoc",
        "array_intersect",
        "array_intersect_key",
        "array_intersect_assoc",
        "array_key_exists",
        "array_key_first",
        "array_key_last",
        "array_multisort",
        "array_replace",
        "array_replace_recursive",
        "in_array",
        "compact",
        "extract",
        "sort",
        "rsort",
        "asort",
        "arsort",
        "ksort",
        "krsort",
        "usort",
        "uasort",
        "uksort",
        "natsort",
        "natcasesort",
        "count",
        "sizeof",
        "range",
        "shuffle",
        "list",
        "strlen",
        "strpos",
        "strrpos",
        "strstr",
        "stristr",
        "substr",
        "substr_count",
        "substr_replace",
        "str_replace",
        "str_ireplace",
        "str_repeat",
        "str_pad",
        "str_split",
        "str_word_count",
        "str_contains",
        "str_starts_with",
        "str_ends_with",
        "strtolower",
        "strtoupper",
        "ucfirst",
        "lcfirst",
        "ucwords",
        "trim",
        "ltrim",
        "rtrim",
        "nl2br",
        "wordwrap",
        "number_format",
        "sprintf",
        "printf",
        "fprintf",
        "sscanf",
        "vsprintf",
        "implode",
        "join",
        "explode",
        "chunk_split",
        "ord",
        "chr",
        "hex2bin",
        "bin2hex",
        "pack",
        "unpack",
        "md5",
        "sha1",
        "crc32",
        "base64_encode",
        "base64_decode",
        "htmlspecialchars",
        "htmlspecialchars_decode",
        "htmlentities",
        "html_entity_decode",
        "urlencode",
        "urldecode",
        "rawurlencode",
        "rawurldecode",
        "http_build_query",
        "parse_str",
        "parse_url",
        "intval",
        "floatval",
        "strval",
        "boolval",
        "settype",
        "gettype",
        "is_null",
        "is_int",
        "is_integer",
        "is_long",
        "is_float",
        "is_double",
        "is_string",
        "is_bool",
        "is_array",
        "is_object",
        "is_numeric",
        "is_callable",
        "is_resource",
        "is_finite",
        "is_infinite",
        "is_nan",
        "is_countable",
        "isset",
        "unset",
        "empty",
        "var_dump",
        "var_export",
        "print_r",
        "debug_zval_refs",
        "serialize",
        "unserialize",
        "abs",
        "ceil",
        "floor",
        "round",
        "max",
        "min",
        "pow",
        "sqrt",
        "log",
        "log2",
        "log10",
        "exp",
        "fmod",
        "intdiv",
        "fdiv",
        "rand",
        "mt_rand",
        "random_int",
        "random_bytes",
        "srand",
        "mt_srand",
        "sin",
        "cos",
        "tan",
        "asin",
        "acos",
        "atan",
        "atan2",
        "pi",
        "deg2rad",
        "rad2deg",
        "base_convert",
        "bindec",
        "octdec",
        "hexdec",
        "decoct",
        "dechex",
        "time",
        "microtime",
        "sleep",
        "usleep",
        "time_sleep_until",
        "fopen",
        "fclose",
        "fread",
        "fwrite",
        "fgets",
        "fgetc",
        "feof",
        "fseek",
        "ftell",
        "rewind",
        "fflush",
        "flock",
        "fstat",
        "ftruncate",
        "fgetcsv",
        "fputcsv",
        "file_get_contents",
        "file_put_contents",
        "file_exists",
        "file",
        "readfile",
        "copy",
        "rename",
        "unlink",
        "mkdir",
        "rmdir",
        "is_file",
        "is_dir",
        "is_link",
        "is_readable",
        "is_writable",
        "is_executable",
        "stat",
        "lstat",
        "realpath",
        "basename",
        "dirname",
        "pathinfo",
        "glob",
        "tempnam",
        "sys_get_temp_dir",
        "tmpfile",
        "chmod",
        "chown",
        "chgrp",
        "touch",
        "clearstatcache",
        "readlink",
        "symlink",
        "link",
        "opendir",
        "closedir",
        "readdir",
        "scandir",
        "preg_match",
        "preg_match_all",
        "preg_replace",
        "preg_replace_callback",
        "preg_split",
        "preg_quote",
        "preg_last_error",
        "class_exists",
        "interface_exists",
        "trait_exists",
        "function_exists",
        "method_exists",
        "property_exists",
        "get_class",
        "get_parent_class",
        "get_called_class",
        "get_object_vars",
        "get_class_vars",
        "get_class_methods",
        "defined",
        "define",
        "constant",
        "trigger_error",
        "user_error",
        "set_error_handler",
        "restore_error_handler",
        "set_exception_handler",
        "restore_exception_handler",
        "debug_backtrace",
        "debug_print_backtrace",
        "header",
        "headers_sent",
        "headers_list",
        "setcookie",
        "ini_get",
        "ini_set",
        "ini_restore",
        "get_cfg_var",
        "phpversion",
        "phpinfo",
        "php_uname",
        "php_sapi_name",
        "get_defined_vars",
        "get_defined_functions",
        "get_defined_constants",
        "getenv",
        "putenv",
        "call_user_func",
        "call_user_func_array",
        "array_is_list",
    ];
    if standard.contains(&func_name) {
        return Some("standard");
    }

    // Date functions
    let date = [
        "date",
        "time",
        "mktime",
        "gmmktime",
        "strtotime",
        "getdate",
        "localtime",
        "checkdate",
        "date_create",
        "date_create_immutable",
        "date_create_from_format",
        "date_format",
        "date_modify",
        "date_add",
        "date_sub",
        "date_diff",
        "date_timezone_get",
        "date_timezone_set",
        "date_offset_get",
        "date_time_set",
        "date_date_set",
        "date_isodate_set",
        "date_timestamp_get",
        "date_timestamp_set",
        "date_default_timezone_set",
        "date_default_timezone_get",
        "date_parse",
        "date_parse_from_format",
        "timezone_open",
        "timezone_name_get",
        "timezone_offset_get",
        "timezone_identifiers_list",
        "timezone_abbreviations_list",
        "strftime",
        "gmstrftime",
        "idate",
    ];
    if date.contains(&func_name) {
        return Some("date");
    }

    // JSON functions
    if func_name.starts_with("json_") {
        return Some("json");
    }

    // Ctype functions
    if func_name.starts_with("ctype_") {
        return Some("ctype");
    }

    // mbstring functions
    if func_name.starts_with("mb_") {
        return Some("mbstring");
    }

    // Otherwise, the extension its stub declares it in.
    crate::stubs::find_function(func_name.as_bytes()).map(|f| f.ext)
}

// ---------------------------------------------------------------------------
// Faithful ports of php_reflection.c's string builders.
// ---------------------------------------------------------------------------

/// Port of `zend_type_to_string()`: class names first (in declaration
/// order, intersections bracketed inside unions), then builtin types in
/// the engine's canonical order, `?T` for a single nullable type.
pub(crate) fn zend_type_string(pt: &ParamType) -> String {
    let mut classes: Vec<String> = Vec::new();
    let mut mask: u32 = 0;
    const STATIC: u32 = 1 << 0;
    const CALLABLE: u32 = 1 << 1;
    const OBJECT: u32 = 1 << 2;
    const ARRAY: u32 = 1 << 3;
    const STRING: u32 = 1 << 4;
    const INT: u32 = 1 << 5;
    const FLOAT: u32 = 1 << 6;
    const FALSE: u32 = 1 << 7;
    const TRUE: u32 = 1 << 8;
    const VOID: u32 = 1 << 9;
    const NEVER: u32 = 1 << 10;
    const NULL: u32 = 1 << 11;
    const MIXED: u32 = 1 << 12;
    fn walk(pt: &ParamType, classes: &mut Vec<String>, mask: &mut u32, in_union: bool) {
        match pt {
            ParamType::Simple(n) => {
                let l = n.to_ascii_lowercase();
                *mask |= match l.as_slice() {
                    b"static" => STATIC,
                    b"callable" => CALLABLE,
                    b"object" => OBJECT,
                    b"array" => ARRAY,
                    b"string" => STRING,
                    b"int" => INT,
                    b"float" => FLOAT,
                    b"bool" => FALSE | TRUE,
                    b"false" => FALSE,
                    b"true" => TRUE,
                    b"void" => VOID,
                    b"never" => NEVER,
                    b"null" => NULL,
                    b"mixed" => MIXED,
                    b"iterable" => {
                        if !classes.iter().any(|c| c == "Traversable") {
                            classes.push("Traversable".to_string());
                        }
                        ARRAY
                    }
                    _ => {
                        let s = crate::string::msg_str(n).to_string();
                        let s = s.strip_prefix('\\').map(|x| x.to_string()).unwrap_or(s);
                        classes.push(s);
                        0
                    }
                };
            }
            ParamType::Nullable(inner) => {
                *mask |= NULL;
                walk(inner, classes, mask, in_union);
            }
            ParamType::Union(items) => {
                for t in items {
                    walk(t, classes, mask, true);
                }
            }
            ParamType::Intersection(items) => {
                let parts: Vec<String> = items.iter().map(zend_type_string).collect();
                let s = parts.join("&");
                classes.push(if in_union { format!("({})", s) } else { s });
            }
        }
    }
    walk(pt, &mut classes, &mut mask, false);
    if mask & MIXED != 0 {
        classes.push("mixed".to_string());
        return classes.join("|");
    }
    let mut parts = classes;
    let order: [(u32, &str); 9] = [
        (STATIC, "static"),
        (CALLABLE, "callable"),
        (OBJECT, "object"),
        (ARRAY, "array"),
        (STRING, "string"),
        (INT, "int"),
        (FLOAT, "float"),
        (0, ""),
        (VOID, "void"),
    ];
    for (bit, name) in order.iter() {
        if *bit == 0 {
            if mask & (FALSE | TRUE) == (FALSE | TRUE) {
                parts.push("bool".to_string());
            } else if mask & FALSE != 0 {
                parts.push("false".to_string());
            } else if mask & TRUE != 0 {
                parts.push("true".to_string());
            }
            continue;
        }
        if mask & bit != 0 {
            parts.push(name.to_string());
        }
    }
    if mask & NEVER != 0 {
        parts.push("never".to_string());
    }
    if mask & NULL != 0 {
        if parts.len() == 1 && !parts[0].contains('&') {
            return format!("?{}", parts[0]);
        }
        parts.push("null".to_string());
    }
    parts.join("|")
}

/// Extension name of an internal method's class (from its stub).
fn internal_method_ext(m: &crate::object::MethodDef) -> Option<&'static str> {
    crate::stubs::find_class(&m.declaring_class)
        .map(|c| c.ext)
        // Classes extensions declare themselves: the ext is in the arginfo.
        .or_else(|| {
            m.op_array
                .internal_sig
                .map(|f| f.ext)
                .filter(|e| !e.is_empty())
        })
}

fn op_array_is_deprecated(op: &OpArray, attrs: &[RuntimeAttribute]) -> bool {
    if let Some(sig) = op.internal_sig {
        return sig.flags & crate::stubs::F_DEPRECATED != 0;
    }
    op.attributes.iter().chain(attrs.iter()).any(|a| {
        let n = crate::string::msg_str(&a.name);
        n.eq_ignore_ascii_case("deprecated") || n.eq_ignore_ascii_case("\\deprecated")
    })
}

/// Find the prototype class of a method (Zend's `fptr->common.prototype`):
/// the root-most ancestor declaration it overrides, with interface methods
/// taking precedence. Constructors only get one from abstract/interface
/// declarations.
fn method_prototype(vm: &Vm, m: &crate::object::MethodDef, scope: Option<&[u8]>) -> Option<String> {
    if m.visibility == Visibility::Private {
        return None;
    }
    let decl = m.declaring_class.to_ascii_lowercase();
    let ml = m.name.to_ascii_lowercase();
    let is_ctor = ml == b"__construct";
    let ce = rcls(vm, &decl)?;
    let mut proto: Option<(String, bool)> = None;
    // Parent chain.
    let mut visited: Vec<Vec<u8>> = vec![decl.clone()];
    let mut cur = ce.parent.clone();
    let mut chain_classes: Vec<Vec<u8>> = vec![decl.clone()];
    while let Some(p) = cur {
        let pl = p.to_ascii_lowercase();
        if visited.contains(&pl) {
            break;
        }
        visited.push(pl.clone());
        chain_classes.push(pl.clone());
        let pce = match rcls(vm, &pl) {
            Some(c) => c,
            None => break,
        };
        if let Some(pm) = rmethod(vm, &pl, &ml) {
            if pm.visibility != Visibility::Private {
                let dl = pm.declaring_class.to_ascii_lowercase();
                let name = rcls(vm, &dl)
                    .map(|c| crate::string::msg_str(&c.name).to_string())
                    .unwrap_or_else(|| crate::string::msg_str(&pce.name).to_string());
                proto = Some((name, pm.is_abstract));
            }
        }
        cur = pce.parent.clone();
    }
    // Interfaces of the class the method is looked up on (an inherited
    // method gets the prototypes of the interfaces it implements there)
    // and of its ancestors, implemented in order: the last one wins.
    let mut iface_chain: Vec<Vec<u8>> = Vec::new();
    match scope.map(|s| s.to_ascii_lowercase()) {
        Some(s) if s != decl => {
            let mut c = Some(s);
            while let Some(cl) = c {
                if iface_chain.contains(&cl) {
                    break;
                }
                c = rcls(vm, &cl).and_then(|ce| ce.parent.as_ref().map(|p| p.to_ascii_lowercase()));
                iface_chain.push(cl);
            }
        }
        _ => iface_chain = chain_classes.clone(),
    }
    let mut stack: Vec<Vec<u8>> = Vec::new();
    for c in iface_chain.iter().rev() {
        if let Some(cc) = rcls(vm, c) {
            for i in &cc.interfaces {
                stack.push(i.to_ascii_lowercase());
            }
        }
    }
    stack.reverse();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut iface_proto: Option<String> = None;
    while let Some(i) = stack.pop() {
        if seen.contains(&i) || seen.len() > 64 {
            continue;
        }
        seen.push(i.clone());
        if let Some(ice) = rcls(vm, &i) {
            if let Some(im) = ice.methods.get(&ml) {
                let dl = im.declaring_class.to_ascii_lowercase();
                let name = rcls(vm, &dl)
                    .map(|c| crate::string::msg_str(&c.name).to_string())
                    .unwrap_or_else(|| crate::string::msg_str(&ice.name).to_string());
                if dl != decl {
                    iface_proto = Some(name);
                }
            }
            for j in &ice.interfaces {
                stack.push(j.to_ascii_lowercase());
            }
        }
    }
    if let Some(ip) = iface_proto {
        return Some(ip);
    }
    // An (internal) enum's cases()/from()/tryFrom() implement UnitEnum's /
    // BackedEnum's.
    if ce.is_enum {
        match ml.as_slice() {
            b"cases" => return Some("UnitEnum".to_string()),
            b"from" | b"tryfrom" if ce.enum_backing_type.is_some() => {
                return Some("BackedEnum".to_string());
            }
            _ => {}
        }
    }
    // A class declaring __toString() implicitly implements Stringable: the
    // topmost declaration's prototype is Stringable::__toString().
    if ml == b"__tostring" && !ce.is_trait && !ce.is_interface && decl != b"stringable" {
        return Some("Stringable".to_string());
    }
    match proto {
        Some((name, is_abstract)) => {
            if is_ctor && !is_abstract {
                None
            } else {
                Some(name)
            }
        }
        None => None,
    }
}

/// Default-value suffix for a parameter of a user function (` = ...`).
fn user_param_default_suffix(op: &OpArray, cv_idx: usize) -> String {
    if let Some(Some(r)) = op.param_default_repr.get(cv_idx) {
        return format!(" = {}", r);
    }
    if let Some(const_ref) = extract_param_default_constant(op, cv_idx as u32) {
        let display = if let Some(rest) = const_ref.strip_prefix("global:") {
            rest.trim_start_matches('\\').to_string()
        } else if let Some(rest) = const_ref.strip_prefix("class:") {
            rest.to_string()
        } else {
            const_ref
        };
        format!(" = {}", display)
    } else if let Some(v) = extract_param_default(op, cv_idx as u32) {
        format!(" = {}", format_reflection_default(&v))
    } else {
        String::new()
    }
}

/// `- Bound Variables [n]` of a closure, right after the `@@` line.
fn insert_bound_variables(out: &mut String, bound: &[String]) {
    if bound.is_empty() {
        return;
    }
    let mut bv = format!("\n  - Bound Variables [{}] {{\n", bound.len());
    for (i, n) in bound.iter().enumerate() {
        bv.push_str(&format!("      Variable #{} [ ${} ]\n", i, n));
    }
    bv.push_str("  }\n");
    if let Some(pos) = out.find('\n') {
        if let Some(pos2) = out[pos + 1..].find('\n') {
            let at = pos + 1 + pos2 + 1;
            out.insert_str(at, &bv);
        }
    }
}

/// Port of `_parameter_string()`.
pub(crate) fn parameter_string(op: &OpArray, idx: usize, skip: usize, required: bool) -> String {
    let cv_idx = idx + skip;
    let mut s = format!("Parameter #{} [ ", idx);
    s.push_str(if required {
        "<required> "
    } else {
        "<optional> "
    });
    if let Some(Some(pti)) = op.param_types.get(cv_idx) {
        s.push_str(&zend_type_string(&pti.param_type));
        s.push(' ');
    }
    let by_ref = op.by_ref_params.get(cv_idx).copied().unwrap_or(false);
    if by_ref {
        s.push('&');
    }
    let is_variadic = op
        .variadic_param
        .map(|v| v as usize == cv_idx)
        .unwrap_or(false);
    if is_variadic {
        s.push_str("...");
    }
    let pname = op
        .cv_names
        .get(cv_idx)
        .map(|n| crate::string::msg_str(n).to_string())
        .unwrap_or_default();
    s.push('$');
    s.push_str(&pname);
    if !required && !is_variadic {
        if let Some(sig) = op.internal_sig {
            s.push_str(" = ");
            match sig.args.get(idx).and_then(|a| a.default) {
                Some(d) => s.push_str(&arginfo_default_text(op.scope_class.as_deref(), d)),
                None => s.push_str("<default>"),
            }
        } else {
            s.push_str(&user_param_default_suffix(op, cv_idx));
        }
    }
    s.push_str(" ]");
    s
}

/// A stub default as gen_stub.php writes it into the arginfo: class names
/// fully qualified without the leading backslash (resolved in the
/// namespace of the method's class), `|` with spaces, a NUL as `\x00`.
fn arginfo_default_text(scope_lower: Option<&[u8]>, d: &str) -> String {
    let ns: Option<String> = scope_lower
        .and_then(crate::stubs::find_class)
        .and_then(|c| c.name.rsplit_once('\\').map(|(n, _)| n.to_string()));
    let b = d.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(d.len() + 8);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\'' {
            // a string literal: only the \0 escape changes
            let q = c;
            let start = i;
            i += 1;
            while i < b.len() && b[i] != q {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            let lb = &b[start..i];
            if q == b'"' {
                let mut j = 0;
                while j < lb.len() {
                    if lb[j] == b'\\' && j + 1 < lb.len() {
                        if lb[j + 1] == b'0' && !lb.get(j + 2).is_some_and(|x| x.is_ascii_digit()) {
                            out.extend_from_slice(b"\\x00");
                        } else {
                            out.extend_from_slice(&lb[j..j + 2]);
                        }
                        j += 2;
                    } else {
                        out.push(lb[j]);
                        j += 1;
                    }
                }
            } else {
                out.extend_from_slice(lb);
            }
            continue;
        }
        if c == b'|' && out.last() != Some(&b' ') {
            out.extend_from_slice(b" | ");
            i += 1;
            while i < b.len() && b[i] == b' ' {
                i += 1;
            }
            continue;
        }
        if c == b'\\' || c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i] == b'\\' || b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let word = &d[start..i];
            if d[i..].starts_with("::") {
                let name = word.strip_prefix('\\').unwrap_or(word);
                let qualified = match &ns {
                    Some(n)
                        if !word.starts_with('\\')
                            && !name.contains('\\')
                            && !matches!(
                                name.to_ascii_lowercase().as_str(),
                                "self" | "static" | "parent"
                            )
                            && crate::stubs::find_class(name.as_bytes()).is_none() =>
                    {
                        format!("{}\\{}", n, name)
                    }
                    _ => name.to_string(),
                };
                out.extend_from_slice(qualified.as_bytes());
            } else {
                out.extend_from_slice(word.as_bytes());
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    crate::string::msg_str(&out).into_owned()
}

/// `_parameter_string()` for an internal function with userland arginfo
/// (Closure::__invoke): defaults can't be fetched and print `<default>`.
pub(crate) fn parameter_string_user_arginfo(
    op: &OpArray,
    idx: usize,
    skip: usize,
    required: bool,
) -> String {
    let cv_idx = idx + skip;
    let mut s = parameter_string(op, idx, skip, true);
    let is_variadic = op
        .variadic_param
        .map(|v| v as usize == cv_idx)
        .unwrap_or(false);
    if !required {
        s = s.replacen("<required> ", "<optional> ", 1);
        if !is_variadic {
            let end = s.len() - 2;
            s.insert_str(end, " = <default>");
        }
    }
    s
}

/// What kind of function `function_string` renders.
pub(crate) struct FnStrCtx<'a> {
    pub op: &'a OpArray,
    pub method: Option<&'a crate::object::MethodDef>,
    /// Lowercase name of the class the method was looked up on.
    pub scope: Option<&'a [u8]>,
    pub is_closure: bool,
    /// Internal function without stub info (no arginfo available).
    pub internal_fn_ext: Option<&'a str>,
    pub display_name: &'a str,
    pub filename: &'a str,
    /// Internal function with userland arginfo (e.g. Closure::__invoke):
    /// defaults render as `<default>`.
    pub internal_user_arginfo: bool,
    /// Override for the number of leading CVs that aren't parameters.
    pub params_skip: Option<usize>,
}

/// Port of `_function_string()`.
pub(crate) fn function_string(vm: &Vm, ctx: &FnStrCtx, indent: &str) -> String {
    let sig_oa = reflect_sig_op_array(ctx.op);
    let op = sig_oa.as_ref().unwrap_or(ctx.op);
    let is_internal =
        op.internal_sig.is_some() || ctx.internal_fn_ext.is_some() || ctx.internal_user_arginfo;
    let mut s = String::new();
    // (Internal functions only have a doc comment when their stub exposes
    // it.)
    {
        let doc = ctx.method.and_then(|m| m.doc_comment.clone()).or_else(|| {
            op.doc_comment
                .as_ref()
                .map(|d| crate::string::msg_str(d).into())
        });
        if let Some(d) = doc {
            s.push_str(indent);
            s.push_str(&d);
            s.push('\n');
        }
    }
    s.push_str(indent);
    s.push_str(if ctx.is_closure {
        "Closure [ "
    } else if ctx.method.is_some() {
        "Method [ "
    } else {
        "Function [ "
    });
    s.push_str(if is_internal { "<internal" } else { "<user" });
    let attrs: &[RuntimeAttribute] = ctx.method.map(|m| m.attributes.as_slice()).unwrap_or(&[]);
    if op_array_is_deprecated(op, attrs) {
        s.push_str(", deprecated");
    }
    if is_internal && !ctx.internal_user_arginfo {
        let ext = match ctx.method {
            Some(m) => internal_method_ext(m),
            None => op.internal_sig.map(|f| f.ext),
        };
        if let Some(e) = ext.or(ctx.internal_fn_ext) {
            if !e.is_empty() {
                s.push(':');
                s.push_str(e);
            }
        }
    }
    // (a closure's function has no class context here)
    if let (Some(m), Some(scope), false) = (ctx.method, ctx.scope, ctx.is_closure) {
        let decl = m.declaring_class.to_ascii_lowercase();
        if decl != scope.to_ascii_lowercase() {
            let dn = rcls(vm, &decl)
                .map(|c| crate::string::msg_str(&c.name).to_string())
                .unwrap_or_else(|| crate::string::msg_str(&m.declaring_class).to_string());
            s.push_str(&format!(", inherits {}", dn));
        } else if let Some(parent) = rcls(vm, &decl).and_then(|c| c.parent.clone()) {
            let pl = parent.to_ascii_lowercase();
            if let Some(ow) = rmethod(vm, &pl, &m.name) {
                let ow_decl = ow.declaring_class.to_ascii_lowercase();
                if ow_decl != decl && ow.visibility != Visibility::Private {
                    let on = rcls(vm, &ow_decl)
                        .map(|c| crate::string::msg_str(&c.name).to_string())
                        .unwrap_or_else(|| crate::string::msg_str(&ow.declaring_class).to_string());
                    s.push_str(&format!(", overwrites {}", on));
                }
            }
        }
    }
    if let Some(m) = ctx.method {
        if let Some(p) = method_prototype(vm, m, ctx.scope) {
            s.push_str(&format!(", prototype {}", p));
        }
        if m.name.eq_ignore_ascii_case(b"__construct") {
            s.push_str(", ctor");
        }
    }
    s.push_str("> ");
    if let Some(m) = ctx.method {
        if m.is_abstract {
            s.push_str("abstract ");
        }
        if m.is_final {
            s.push_str("final ");
        }
        if m.is_static {
            s.push_str("static ");
        }
        s.push_str(match m.visibility {
            // (a fake closure's flags have ZEND_ACC_PUBLIC added)
            _ if ctx.is_closure && m.visibility != Visibility::Public => "<visibility error> ",
            Visibility::Public => "public ",
            Visibility::Protected => "protected ",
            Visibility::Private => "private ",
        });
        s.push_str("method ");
    } else if ctx.is_closure && op.scope_class.is_some() {
        if op.is_static_closure {
            s.push_str("static ");
        }
        // A closure over a non-public method (`$this->m(...)`).
        let non_public = op.scope_class.as_ref().is_some_and(|sc| {
            !crate::closure::is_closure_fn_name(&op.name)
                && rmethod(vm, sc, &op.name).is_some_and(|m| {
                    m.visibility != Visibility::Public && m.op_array.name == op.name
                })
        });
        s.push_str(if non_public {
            "<visibility error> method "
        } else {
            "public method "
        });
    } else {
        if ctx.is_closure && op.is_static_closure {
            s.push_str("static ");
        }
        s.push_str("function ");
    }
    if op.returns_by_ref {
        s.push('&');
    }
    s.push_str(ctx.display_name);
    s.push_str(" ] {\n");
    if !is_internal {
        let end = if op.end_line > 0 {
            op.end_line
        } else {
            op.decl_line
        };
        s.push_str(&format!(
            "{}  @@ {} {} - {}\n",
            indent, ctx.filename, op.decl_line, end
        ));
    }
    let param_indent = format!("{}  ", indent);
    let skip = if let Some(k) = ctx.params_skip {
        k
    } else if ctx.method.map(|m| !m.is_static).unwrap_or(false)
        || (op
            .cv_names
            .first()
            .map(|n| n.as_slice() == b"this")
            .unwrap_or(false)
            && ctx.method.is_some())
    {
        1
    } else {
        0
    };
    let num_args = (op.param_count as usize).saturating_sub(skip);
    let has_arg_info =
        (is_internal && !ctx.internal_user_arginfo) || num_args > 0 || op.return_type.is_some();
    if has_arg_info {
        s.push('\n');
        s.push_str(&format!("{}- Parameters [{}] {{\n", param_indent, num_args));
        let req = op.required_param_count as usize;
        for i in 0..num_args {
            s.push_str(&format!("{}  ", param_indent));
            if ctx.internal_user_arginfo {
                s.push_str(&parameter_string_user_arginfo(op, i, skip, i < req));
            } else {
                s.push_str(&parameter_string(op, i, skip, i < req));
            }
            s.push('\n');
        }
        s.push_str(&format!("{}}}\n", param_indent));
    }
    if let Some(rt) = &op.return_type {
        let tentative = op
            .internal_sig
            .map(|f| f.flags & crate::stubs::F_TENTATIVE != 0)
            .unwrap_or(false);
        s.push_str(&format!(
            "  {}- {} [ {} ]\n",
            indent,
            if tentative {
                "Tentative return"
            } else {
                "Return"
            },
            zend_type_string(rt)
        ));
    }
    s.push_str(indent);
    s.push_str("}\n");
    s
}

/// Port of `smart_str_append_escaped()`.
fn zend_escape_str(s: &[u8]) -> String {
    let mut out = String::new();
    for &c in s {
        if c < 32 || c == b'\\' || c > 126 {
            out.push('\\');
            match c {
                b'\n' => out.push('n'),
                b'\r' => out.push('r'),
                b'\t' => out.push('t'),
                0x0c => out.push('f'),
                0x0b => out.push('v'),
                b'\\' => out.push('\\'),
                0x1b => out.push('e'),
                _ => out.push_str(&format!("x{:X}{:X}", c >> 4, c & 0xf)),
            }
        } else {
            out.push(c as char);
        }
    }
    out
}

/// Port of `format_default_value()` for already-evaluated values.
pub(crate) fn format_default_value(v: &Value) -> String {
    match v {
        Value::Undef | Value::Null => "NULL".to_string(),
        Value::True => "true".to_string(),
        Value::False => "false".to_string(),
        Value::Long(n) => n.to_string(),
        Value::Double(f) => {
            let s = crate::value::format_php_float(*f);
            if f.is_finite() && !s.contains('.') {
                format!("{}.0", s)
            } else {
                s
            }
        }
        Value::String(s) => format!("'{}'", zend_escape_str(s.as_bytes())),
        Value::Array(a) => {
            let arr = a.borrow();
            let mut is_list = true;
            let mut expect: i64 = 0;
            for (k, _) in arr.iter() {
                match k {
                    ArrayKey::Int(n) if *n == expect => expect += 1,
                    _ => {
                        is_list = false;
                        break;
                    }
                }
            }
            let mut parts: Vec<String> = Vec::new();
            for (k, v) in arr.iter() {
                let val = format_default_value(v);
                if is_list {
                    parts.push(val);
                } else {
                    let key = match k {
                        ArrayKey::Int(n) => n.to_string(),
                        ArrayKey::String(s) => format!("'{}'", zend_escape_str(s.as_bytes())),
                    };
                    parts.push(format!("{} => {}", key, val));
                }
            }
            format!("[{}]", parts.join(", "))
        }
        Value::Object(o) => {
            let ob = o.borrow();
            if ob.has_property(b"__enum_case") {
                format!(
                    "{}::{}",
                    crate::string::msg_str(&ob.class_name),
                    ob.get_property(b"name").to_php_string().to_msg_string()
                )
            } else {
                format!("object({})", crate::string::msg_str(&ob.class_name))
            }
        }
        other => other.to_php_string().to_msg_string(),
    }
}

/// Whether a class gets an object iterator (`ce->get_iterator`), shown as
/// `<iterateable>`: it implements Iterator/IteratorAggregate (not counting the
/// interfaces themselves) or has hooked properties.
fn class_is_iterateable(vm: &Vm, class_lower: &[u8]) -> bool {
    if class_lower == b"iterator"
        || class_lower == b"iteratoraggregate"
        || class_lower == b"traversable"
    {
        return false;
    }
    // (an interface has no get_iterator of Iterator's: only a user one
    // declaring hooked properties gets zend_hooked_object_get_iterator)
    if let Some(ce) = rcls(vm, class_lower).filter(|c| c.is_interface) {
        return !is_internal_class(vm, class_lower)
            && ce
                .properties
                .iter()
                .any(|p| p.has_get_hook || p.has_set_hook);
    }
    if vm.class_extends(class_lower, b"iterator")
        || vm.class_extends(class_lower, b"iteratoraggregate")
    {
        return true;
    }
    if is_internal_class(vm, class_lower) {
        let ifs = crate::stubs::class_interfaces(class_lower);
        if ifs.iter().any(|i| {
            i.eq_ignore_ascii_case("Iterator") || i.eq_ignore_ascii_case("IteratorAggregate")
        }) {
            return true;
        }
    }
    // User classes: walk interfaces (user ClassEntry interface lists may
    // include internal ones that extend Iterator).
    let mut stack: Vec<Vec<u8>> = vec![class_lower.to_vec()];
    let mut seen: Vec<Vec<u8>> = Vec::new();
    while let Some(c) = stack.pop() {
        if seen.contains(&c) || seen.len() > 128 {
            continue;
        }
        seen.push(c.clone());
        if let Some(ce) = rcls(vm, &c) {
            // (zend_hooked_object_get_iterator: user hooks; an internal
            // class's virtual properties have none)
            if !is_internal_class(vm, &c)
                && ce
                    .properties
                    .iter()
                    .any(|p| p.has_get_hook || p.has_set_hook)
            {
                return true;
            }
            for i in &ce.interfaces {
                let il = i.to_ascii_lowercase();
                if il == b"iterator" || il == b"iteratoraggregate" {
                    return true;
                }
                stack.push(il);
            }
            if let Some(p) = &ce.parent {
                stack.push(p.to_ascii_lowercase());
            }
        }
    }
    false
}

/// format_default_value() for a default that may still be an unevaluated
/// constant expression (a compiler marker): that is shown as its AST export,
/// like php-src does for an IS_CONSTANT_AST zval.
pub(crate) fn deferred_default_display(vm: &Vm, v: &Value, declaring_class: &[u8]) -> String {
    if let Value::String(s) = v {
        let b = s.as_bytes();
        if let Some(name) = b.strip_prefix(b"__deferred_thunk__::") {
            let decl = declaring_class.to_ascii_lowercase();
            let find = |c: &crate::object::ClassEntry| {
                c.const_closures
                    .iter()
                    .find(|op| op.name == name)
                    .map(|op| match op.param_default_repr.first() {
                        Some(Some(r)) => r.clone(),
                        _ => crate::string::msg_str(&op.display_name).to_string(),
                    })
            };
            if let Some(r) = rcls(vm, &decl)
                .and_then(find)
                .or_else(|| vm.classes.values().find_map(find))
            {
                return r;
            }
        } else if let Some(name) = b.strip_prefix(b"__deferred_global_const__::") {
            return crate::string::msg_str(name).to_string();
        }
    }
    format_default_value(v)
}

/// Port of `_property_string()` for a declared property.
pub(crate) fn property_string(vm: &Vm, p: &crate::object::PropertyDef, indent: &str) -> String {
    let mut s = String::new();
    if let Some(doc) = &p.doc_comment {
        s.push_str(indent);
        s.push_str(doc);
        s.push('\n');
    }
    s.push_str(indent);
    s.push_str("Property [ ");
    let set_vis = match p.set_visibility {
        Some(v) if v != p.visibility => Some(v),
        Some(_) => None,
        None if p.is_readonly && p.visibility == Visibility::Public => Some(Visibility::Protected),
        None => None,
    };
    // Interface properties (hooked) are implicitly abstract.
    let in_interface = rcls(vm, &p.declaring_class.to_ascii_lowercase())
        .map(|c| c.is_interface)
        .unwrap_or(false);
    if p.is_abstract || in_interface {
        s.push_str("abstract ");
    }
    if p.is_final || set_vis == Some(Visibility::Private) {
        s.push_str("final ");
    }
    s.push_str(match p.visibility {
        Visibility::Public => "public ",
        Visibility::Protected => "protected ",
        Visibility::Private => "private ",
    });
    match set_vis {
        Some(Visibility::Private) => s.push_str("private(set) "),
        Some(Visibility::Protected) => s.push_str("protected(set) "),
        _ => {}
    }
    if p.is_static {
        s.push_str("static ");
    }
    if p.is_readonly {
        s.push_str("readonly ");
    }
    // (or @virtual in the stub of an internal class implemented in PHP)
    if p.is_virtual
        || crate::stubs::find_self_declared_class(&p.declaring_class.to_ascii_lowercase())
            .is_some_and(|c| {
                c.props.iter().any(|sp| {
                    sp.name.as_bytes() == p.name.as_slice()
                        && sp.flags & crate::stubs::F_VIRTUAL != 0
                })
            })
    {
        s.push_str("virtual ");
    }
    if let Some(t) = &p.property_type {
        s.push_str(&zend_type_string(t));
        s.push(' ');
    }
    s.push('$');
    s.push_str(&crate::string::msg_str(&p.name));
    if !p.is_virtual && !matches!(p.default, Value::Undef) {
        s.push_str(" = ");
        s.push_str(&deferred_default_display(
            vm,
            &p.default,
            &p.declaring_class,
        ));
    }
    // (an internal class's virtual properties are read by its handlers:
    // they have no hooks to list)
    if (p.has_get_hook || p.has_set_hook)
        && !is_internal_class(vm, &p.declaring_class.to_ascii_lowercase())
    {
        s.push_str(" {");
        let decl = p.declaring_class.to_ascii_lowercase();
        let final_hook = |kind: &str| -> bool {
            let mname = format!("__property_{}_{}", kind, crate::string::msg_str(&p.name));
            rmethod(vm, &decl, mname.as_bytes())
                .map(|m| m.is_final)
                .unwrap_or(false)
        };
        if p.has_get_hook {
            s.push_str(if final_hook("get") {
                " final get;"
            } else {
                " get;"
            });
        }
        if p.has_set_hook {
            s.push_str(if final_hook("set") {
                " final set;"
            } else {
                " set;"
            });
        }
        s.push_str(" }");
    }
    s.push_str(" ]\n");
    s
}

/// Port of `_class_const_string()`.
fn class_const_string(
    name: &[u8],
    val: &Value,
    meta: Option<&crate::object::ConstantMeta>,
    indent: &str,
) -> String {
    let mut s = String::new();
    if let Some(doc) = meta.and_then(|m| m.doc_comment.as_ref()) {
        s.push_str(indent);
        s.push_str(doc);
        s.push('\n');
    }
    let vis = match meta.map(|m| m.visibility).unwrap_or(Visibility::Public) {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Private => "private",
    };
    let fin = if meta.map(|m| m.is_final).unwrap_or(false) {
        "final "
    } else {
        ""
    };
    let ty = match meta.and_then(|m| m.declared_type.as_ref()) {
        Some(t) => zend_type_string(t),
        // (an object constant, e.g. an enum case, shows its class)
        None => match val {
            Value::Object(o) => crate::value::display_class_name(&o.borrow().class_name),
            _ => zend_zval_type_name(val).to_string(),
        },
    };
    let v = match val {
        Value::Array(_) => "Array".to_string(),
        Value::Object(_) => "Object".to_string(),
        Value::Null => String::new(),
        Value::True => "1".to_string(),
        Value::False => String::new(),
        Value::Double(f) => crate::value::format_php_float(*f),
        other => other.to_php_string().to_msg_string(),
    };
    s.push_str(&format!(
        "{}Constant [ {}{} {} {} ] {{ {} }}\n",
        indent,
        fin,
        vis,
        ty,
        crate::string::msg_str(name),
        v
    ));
    s
}

/// `zend_zval_type_name()`.
fn zend_zval_type_name(v: &Value) -> &'static str {
    match v {
        Value::Undef | Value::Null => "null",
        Value::True | Value::False => "bool",
        Value::Long(_) => "int",
        Value::Double(_) => "float",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        _ => "mixed",
    }
}

thread_local! {
    /// Constant values pre-evaluated (with a mutable VM) for `class_string`.
    static RESOLVED_CONSTS: RefCell<Option<Vec<(Vec<u8>, Value)>>> = const { RefCell::new(None) };
}

/// Port of `_class_string()`.
pub(crate) fn class_string(
    vm: &Vm,
    class_lower: &[u8],
    obj: Option<&Rc<RefCell<PhpObject>>>,
    indent: &str,
) -> String {
    let ce = match rcls(vm, class_lower) {
        Some(c) => c,
        None => return String::new(),
    };
    let internal = is_internal_class(vm, class_lower);
    let sub_indent = format!("{}    ", indent);
    let mut s = String::new();
    if let Some(doc) = &ce.doc_comment {
        s.push_str(indent);
        s.push_str(doc);
        s.push('\n');
    }
    if obj.is_some() {
        s.push_str(&format!("{}Object of class [ ", indent));
    } else {
        let kind = if ce.is_interface {
            "Interface"
        } else if ce.is_trait {
            "Trait"
        } else if ce.is_enum {
            "Enum"
        } else {
            "Class"
        };
        s.push_str(&format!("{}{} [ ", indent, kind));
    }
    s.push_str(if internal { "<internal" } else { "<user" });
    if internal {
        if let Some(sc) = crate::stubs::find_class(class_lower)
            .or_else(|| crate::stubs::find_self_declared_class(class_lower))
        {
            s.push(':');
            s.push_str(sc.ext);
        }
    }
    s.push_str("> ");
    if class_is_iterateable(vm, class_lower) {
        s.push_str("<iterateable> ");
    }
    if ce.is_interface {
        s.push_str("interface ");
    } else if ce.is_trait {
        s.push_str("trait ");
    } else if ce.is_enum {
        s.push_str("enum ");
    } else {
        if ce.is_abstract {
            s.push_str("abstract ");
        }
        if ce.is_final {
            s.push_str("final ");
        }
        if ce.is_readonly {
            s.push_str("readonly ");
        }
        s.push_str("class ");
    }
    s.push_str(&crate::string::msg_str(&ce.name));
    // (a user interface keeps its first parent interface as `parent`: it
    // is listed with the others)
    if let Some(p) = ce.parent.as_ref().filter(|_| !ce.is_interface) {
        let pn = rcls(vm, &p.to_ascii_lowercase())
            .map(|c| c.name.clone())
            .unwrap_or_else(|| p.clone());
        s.push_str(&format!(" extends {}", crate::string::msg_str(&pn)));
    }
    if ce.is_enum {
        if let Some(bt) = &ce.enum_backing_type {
            s.push_str(if bt.eq_ignore_ascii_case(b"string") {
                ": string"
            } else {
                ": int"
            });
        }
    }
    let mut ifaces = class_all_interface_names(vm, class_lower);
    // (the Stringable a __toString() method adds at compile time)
    if !ce.is_trait
        && !ifaces.iter().any(|i| i.eq_ignore_ascii_case("Stringable"))
        && !class_lower.eq_ignore_ascii_case(b"stringable")
        && vm.class_implements_interface(class_lower, b"stringable")
    {
        ifaces.push("Stringable".to_string());
    }
    if !ifaces.is_empty() {
        s.push_str(if ce.is_interface {
            " extends "
        } else {
            " implements "
        });
        s.push_str(&ifaces.join(", "));
    }
    s.push_str(" ] {\n");
    if !internal {
        let fname = ce
            .filename
            .clone()
            .unwrap_or_else(|| vm.current_file.to_string());
        s.push_str(&format!(
            "{}  @@ {} {}-{}\n",
            indent, fname, ce.start_line, ce.end_line
        ));
    }

    // Constants & enum cases.
    let case_names: Vec<&[u8]> = ce.enum_cases.iter().map(|(n, _)| n.as_slice()).collect();
    let mut const_str = String::new();
    let mut case_str = String::new();
    let mut const_count = 0;
    let mut case_count = 0;
    let resolved: Option<Vec<(Vec<u8>, Value)>> = RESOLVED_CONSTS.with(|r| r.borrow().clone());
    for (cname, raw_val) in ce.constants.iter() {
        let resolved_val = resolved
            .as_ref()
            .and_then(|r| r.iter().find(|(n, _)| n == cname).map(|(_, v)| v.clone()));
        let cval = resolved_val.as_ref().unwrap_or(raw_val);
        if case_names.contains(&cname.as_slice()) {
            case_count += 1;
            let meta = ce.constants_meta.get(cname.as_slice());
            if let Some(doc) = meta.and_then(|m| m.doc_comment.as_ref()) {
                case_str.push_str(&sub_indent);
                case_str.push_str(doc);
                case_str.push('\n');
            }
            case_str.push_str(&format!(
                "{}Case {}",
                sub_indent,
                crate::string::msg_str(cname)
            ));
            if ce.enum_backing_type.is_some() {
                let bv = ce
                    .enum_cases
                    .iter()
                    .find(|(n, _)| n == cname)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(Value::Null);
                case_str.push_str(&format!(" = {}\n", bv.to_php_string().to_msg_string()));
            } else {
                case_str.push('\n');
            }
        } else {
            const_count += 1;
            const_str.push_str(&class_const_string(
                cname,
                cval,
                ce.constants_meta.get(cname.as_slice()),
                &sub_indent,
            ));
        }
    }
    // Enum cases not registered as constants.
    for (cn, cv) in &ce.enum_cases {
        if ce.constants.contains_key(cn) {
            continue;
        }
        case_count += 1;
        case_str.push_str(&format!(
            "{}Case {}",
            sub_indent,
            crate::string::msg_str(cn)
        ));
        if ce.enum_backing_type.is_some() {
            case_str.push_str(&format!(" = {}\n", cv.to_php_string().to_msg_string()));
        } else {
            case_str.push('\n');
        }
    }
    if case_count > 0 {
        s.push('\n');
        s.push_str(&format!("{}  - Enum cases [{}] {{\n", indent, case_count));
        s.push_str(&case_str);
        s.push_str(&format!("{}  }}\n", indent));
    }
    s.push('\n');
    s.push_str(&format!("{}  - Constants [{}] {{\n", indent, const_count));
    s.push_str(&const_str);
    s.push_str(&format!("{}  }}\n", indent));

    // Properties.
    let visible = |p: &&crate::object::PropertyDef| {
        !(p.visibility == Visibility::Private
            && p.declaring_class.to_ascii_lowercase() != class_lower)
    };
    let all_props = rprops(vm, class_lower);
    let static_props: Vec<&crate::object::PropertyDef> = all_props
        .iter()
        .copied()
        .filter(|p| p.is_static)
        .filter(visible)
        .collect();
    s.push_str(&format!(
        "\n{}  - Static properties [{}] {{\n",
        indent,
        static_props.len()
    ));
    for p in &static_props {
        s.push_str(&property_string(vm, p, &sub_indent));
    }
    s.push_str(&format!("{}  }}\n", indent));

    let methods = rmethods(vm, class_lower);
    let mvisible = |m: &&&crate::object::MethodDef| {
        !(m.visibility == Visibility::Private
            && m.declaring_class.to_ascii_lowercase() != class_lower)
            && !m.name.starts_with(b"__property_get_")
            && !m.name.starts_with(b"__property_set_")
    };
    let static_methods: Vec<&&crate::object::MethodDef> = methods
        .iter()
        .filter(|m| m.is_static)
        .filter(mvisible)
        .collect();
    s.push_str(&format!(
        "\n{}  - Static methods [{}] {{",
        indent,
        static_methods.len()
    ));
    if static_methods.is_empty() {
        s.push('\n');
    }
    for m in &static_methods {
        s.push('\n');
        s.push_str(&method_section_string(vm, m, class_lower, &sub_indent));
    }
    s.push_str(&format!("{}  }}\n", indent));

    let inst_props: Vec<&crate::object::PropertyDef> = all_props
        .iter()
        .copied()
        .filter(|p| !p.is_static)
        .filter(visible)
        .collect();
    s.push_str(&format!(
        "\n{}  - Properties [{}] {{\n",
        indent,
        inst_props.len()
    ));
    for p in &inst_props {
        s.push_str(&property_string(vm, p, &sub_indent));
    }
    s.push_str(&format!("{}  }}\n", indent));

    if let Some(o) = obj {
        let declared: Vec<Vec<u8>> = all_props.iter().map(|p| p.name.clone()).collect();
        let mut dyn_str = String::new();
        let mut count = 0;
        for (name, _) in o.borrow().properties.iter() {
            if name.is_empty() || name[0] == 0 || name.starts_with(b"__") {
                continue;
            }
            if declared.iter().any(|d| d == name) {
                continue;
            }
            count += 1;
            dyn_str.push_str(&format!(
                "{}Property [ <dynamic> public ${} ]\n",
                sub_indent,
                crate::string::msg_str(name)
            ));
        }
        s.push_str(&format!(
            "\n{}  - Dynamic properties [{}] {{\n",
            indent, count
        ));
        s.push_str(&dyn_str);
        s.push_str(&format!("{}  }}\n", indent));
    }

    let inst_methods: Vec<&&crate::object::MethodDef> = methods
        .iter()
        .filter(|m| !m.is_static)
        .filter(mvisible)
        .collect();
    if inst_methods.is_empty() {
        s.push_str(&format!("\n{}  - Methods [0] {{\n", indent));
    } else {
        s.push_str(&format!(
            "\n{}  - Methods [{}] {{",
            indent,
            inst_methods.len()
        ));
        for m in &inst_methods {
            s.push('\n');
            s.push_str(&method_section_string(vm, m, class_lower, &sub_indent));
        }
    }
    s.push_str(&format!("{}  }}\n", indent));
    s.push_str(&format!("{}}}\n", indent));
    s
}

fn method_section_string(
    vm: &Vm,
    m: &crate::object::MethodDef,
    scope_lower: &[u8],
    indent: &str,
) -> String {
    let filename = if m.op_array.filename.is_empty() {
        vm.current_file.to_string()
    } else {
        crate::string::msg_str(&m.op_array.filename).into_owned()
    };
    let display = crate::string::msg_str(&m.name).to_string();
    let ctx = FnStrCtx {
        op: &m.op_array,
        method: Some(m),
        scope: Some(scope_lower),
        is_closure: false,
        internal_fn_ext: None,
        display_name: &display,
        filename: &filename,
        internal_user_arginfo: false,
        params_skip: None,
    };
    function_string(vm, &ctx, indent)
}

/// `ce->interfaces` names in Zend's order: the parent's interfaces, then
/// the declared ones, then interfaces inherited from those (appended while
/// each is implemented).
pub fn class_all_interface_names(vm: &Vm, class_lower: &[u8]) -> Vec<String> {
    fn canon(vm: &Vm, n: &[u8]) -> String {
        rcls(vm, &n.to_ascii_lowercase())
            .map(|c| crate::string::msg_str(&c.name).to_string())
            .unwrap_or_else(|| crate::string::msg_str(n).to_string())
    }
    fn compute(vm: &Vm, lower: &[u8], depth: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if depth > 32 {
            return out;
        }
        // An internal class: linked as zend_register_internal_class_ex()
        // and zend_class_implements() do.
        if is_internal_class(vm, lower) && crate::stubs::find_class(lower).is_some() {
            return crate::stubs::class_interfaces(lower)
                .into_iter()
                .map(|n| canon(vm, n.as_bytes()))
                .collect();
        }
        let ce = match rcls(vm, lower) {
            Some(c) => c,
            None => return out,
        };
        // User interfaces keep their first extended interface as `parent`.
        let mut declared: Vec<Vec<u8>> = Vec::new();
        declared.extend(ce.interfaces.iter().cloned());
        // add_stringable_interface(): a class declaring __toString() gets
        // Stringable after its declared interfaces, at compile time.
        // (or using a trait's: zend_do_bind_traits())
        if !ce.is_trait
            && ce.methods.get(b"__tostring".as_slice()).is_some_and(|m| {
                m.declaring_class.eq_ignore_ascii_case(lower)
                    || rcls(vm, &m.declaring_class.to_ascii_lowercase()).is_some_and(|d| d.is_trait)
            })
            && !declared
                .iter()
                .any(|i| i.eq_ignore_ascii_case(b"stringable"))
        {
            declared.push(b"Stringable".to_vec());
        }
        if let Some(p) = &ce.parent {
            if ce.is_interface {
                declared.insert(0, p.clone());
            } else {
                out = compute(vm, &p.to_ascii_lowercase(), depth + 1);
                // The parent's interfaces are inherited backwards
                // (zend_do_inherit_interfaces()) by an internal class
                // (zend_do_inheritance_ex()) and by a user class declaring
                // none (zend_do_link_class()); a user class declaring
                // interfaces copies them in order.
                if declared.is_empty() {
                    out.reverse();
                }
            }
        }
        let n_parent = out.len();
        for i in &declared {
            let cn = canon(vm, i);
            if !out.iter().any(|x| x.eq_ignore_ascii_case(&cn)) {
                out.push(cn);
            }
        }
        // zend_do_inherit_interfaces(): each declared interface's own list
        // is appended (walked backwards), skipping ones already present.
        let n_declared = out.len();
        for idx in n_parent..n_declared {
            let il = out[idx].to_ascii_lowercase();
            for pi in compute(vm, il.as_bytes(), depth + 1).into_iter().rev() {
                if !out.iter().any(|x| x.eq_ignore_ascii_case(&pi)) {
                    out.push(pi);
                }
            }
        }
        out
    }
    compute(vm, class_lower, 0)
}

/// Z_PARAM_OBJ_OF_CLASS_OR_STR(reflection_class_ptr) of
/// ReflectionClass::isSubclassOf()/implementsInterface(): a ReflectionClass
/// (or subclass) gives its class name, anything else is parsed as a string.
/// None when an error (TypeError, an exception from __toString()) is pending.
#[inline(never)]
fn reflection_class_or_str_arg(
    vm: &mut Vm,
    arg: &Value,
    method: &str,
    pname: &str,
) -> Option<String> {
    let arg = arg.deref();
    if vm.obj_instanceof(&arg, b"reflectionclass") {
        if let Value::Object(o) = &arg {
            return Some(
                o.borrow()
                    .get_property(b"name")
                    .to_php_string()
                    .to_msg_string(),
            );
        }
    }
    let type_error = |vm: &mut Vm, v: &Value| {
        let msg = format!(
            "ReflectionClass::{}(): Argument #1 (${}) must be of type ReflectionClass|string, {} given",
            method,
            pname,
            Vm::value_type_name(v)
        );
        let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        None
    };
    match &arg {
        Value::Null | Value::Undef => {
            vm.emit_deprecated_at(
                &format!(
                    "ReflectionClass::{}(): Passing null to parameter #1 (${}) of type ReflectionClass|string is deprecated",
                    method, pname
                ),
                vm.current_line,
            );
            Some(String::new())
        }
        Value::String(s) => Some(s.to_msg_string()),
        Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
            vm.warn_nan_coercion(&arg, "string", vm.current_line);
            Some(arg.to_php_string().to_msg_string())
        }
        Value::Object(o) => {
            let cl = o.borrow().class_name.to_ascii_lowercase();
            if !vm.class_implements_interface(&cl, b"stringable") {
                return type_error(vm, &arg);
            }
            let s = vm.value_to_string(&arg);
            if vm.current_exception.is_some() {
                return None;
            }
            Some(s.to_msg_string())
        }
        other => type_error(vm, other),
    }
}
