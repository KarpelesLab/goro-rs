//! DOMXPath and Dom\XPath on goro-xml's XPath 1.0 engine, with PHP
//! callbacks (`registerPhpFunctions()`, `php:function()`,
//! `php:functionString()`, `registerPhpFunctionNS()`) (PHP manual:
//! DOMXPath; observed on PHP 8.5).

use goro_runtime::{NativeData, ObjRef, PhpArray, PhpStr, Value};
use goro_xml::xpath::{self, XPathError};
use goro_xml::{Arena, NodeId};

use super::super::libxml;
use super::lists::{ListKind, new_list};
use super::xmlstore::{self, NodeHold};
use super::{arg_node, modern_fn, node_of, opt_arg_node, s8, sv, with_arena, wrap_node};
use crate::class::NativeHooks;
use crate::func::{Args, NativeFn};
use crate::vm::{R, Unwind, Vm};

pub const METHODS: &[(&str, NativeFn)] = &[
    ("domxpath::__construct", construct),
    ("domxpath::evaluate", evaluate),
    ("domxpath::query", query),
    ("domxpath::registernamespace", register_namespace),
    ("domxpath::registerphpfunctions", register_php_functions),
    ("domxpath::registerphpfunctionns", register_php_function_ns),
    ("domxpath::quote", quote),
    ("dom\\xpath::__construct", construct),
    ("dom\\xpath::evaluate", evaluate),
    ("dom\\xpath::query", query),
    ("dom\\xpath::registernamespace", register_namespace),
    ("dom\\xpath::registerphpfunctions", register_php_functions),
    ("dom\\xpath::registerphpfunctionns", register_php_function_ns),
    ("dom\\xpath::quote", quote),
];

const PHP_NS: &str = "http://php.net/xpath";

/// Which PHP functions XPath may call.
#[derive(Clone)]
enum Allowed {
    None,
    All,
    /// Names (lower case) with their callables.
    Some(Vec<(String, Value)>),
}

/// The native data of an XPath object.
pub struct XPathObj {
    doc: NodeHold,
    namespaces: Vec<(String, String)>,
    register_node_ns: bool,
    allowed: Allowed,
    /// `registerPhpFunctionNS()`: (namespace, name, callable).
    ns_functions: Vec<(String, String, Value)>,
}

impl NativeData for XPathObj {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        if let Allowed::Some(v) = &self.allowed {
            for (_, c) in v {
                visit(c);
            }
        }
        for (_, _, c) in &self.ns_functions {
            visit(c);
        }
    }
    fn clear(&mut self) {
        self.allowed = Allowed::None;
        self.ns_functions.clear();
    }
}

fn with_xp<T>(o: &ObjRef, f: impl FnOnce(&mut XPathObj) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<XPathObj>().map(f)
}

fn this_xp(vm: &mut Vm, a: &Args) -> R<()> {
    if with_xp(a.this_obj(), |_| ()).is_none() {
        let c = vm.class_of(a.this_obj()).name.to_string_lossy();
        return vm.throw("Error", &format!("Couldn't fetch {c}"));
    }
    Ok(())
}

fn construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = arg_node(vm, a, 0)?;
    let reg = a.bool_or(1, true);
    let doc = xmlstore::with(|s| s.hold(d));
    *a.this_obj().obj().native.borrow_mut() = Some(Box::new(XPathObj {
        doc,
        namespaces: Vec::new(),
        register_node_ns: reg,
        allowed: Allowed::None,
        ns_functions: Vec::new(),
    }));
    Ok(Value::Null)
}

fn register_namespace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    this_xp(vm, a)?;
    let p = s8(&a.str(0));
    let ns = s8(&a.str(1));
    with_xp(a.this_obj(), |x| {
        x.namespaces.retain(|(q, _)| *q != p);
        x.namespaces.push((p, ns));
    });
    Ok(Value::Bool(true))
}

fn register_php_functions(vm: &mut Vm, a: &mut Args) -> R<Value> {
    this_xp(vm, a)?;
    // Validation messages observed on PHP 8.5 (test
    // DOMXPath_callables_errors.phpt).
    let bad_names = "Argument #1 ($restrict) must be an array containing valid callback names";
    let new = match a.val(0) {
        Value::Null => Allowed::All,
        Value::Str(s) => {
            if s.is_empty() {
                return vm.fthrow(a, "ValueError", "Argument #1 ($restrict) must be a valid callback name");
            }
            if vm.resolve_callable(&Value::Str(s.clone()), false).is_err() {
                let why = vm.callable_error(&Value::Str(s.clone()));
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!("Argument #1 ($restrict) must be a callable, {why}"),
                );
            }
            Allowed::Some(vec![(s8(&s).to_ascii_lowercase(), Value::Str(s))])
        }
        Value::Array(arr) => {
            let mut v = Vec::new();
            for (k, val) in arr.iter() {
                let val = val.deref();
                match k {
                    goro_runtime::KeyRef::Str(name) => {
                        if name.is_empty() || name.as_bytes().contains(&0) {
                            return vm.fthrow(a, "ValueError", bad_names);
                        }
                        if vm.resolve_callable(&val, false).is_err() {
                            let why = vm.callable_error(&val);
                            return vm.fthrow(
                                a,
                                "TypeError",
                                &format!("Argument #1 ($restrict) must be an array with valid callbacks as values, {why}"),
                            );
                        }
                        v.push((s8(name).to_ascii_lowercase(), val));
                    }
                    _ => {
                        let s = vm.to_php_str(&val)?;
                        if s.is_empty() || s.as_bytes().contains(&0) {
                            return vm.fthrow(a, "ValueError", bad_names);
                        }
                        if vm.resolve_callable(&Value::Str(s.clone()), false).is_err() {
                            let why = vm.callable_error(&Value::Str(s.clone()));
                            return vm.fthrow(
                                a,
                                "TypeError",
                                &format!("Argument #1 ($restrict) must be an array with valid callbacks as values, {why}"),
                            );
                        }
                        v.push((s8(&s).to_ascii_lowercase(), Value::Str(s)));
                    }
                }
            }
            Allowed::Some(v)
        }
        _ => Allowed::All,
    };
    with_xp(a.this_obj(), |x| {
        x.allowed = match (&x.allowed, new) {
            (Allowed::Some(old), Allowed::Some(add)) => {
                let mut v = old.clone();
                for (k, c) in add {
                    v.retain(|(n, _)| *n != k);
                    v.push((k, c));
                }
                Allowed::Some(v)
            }
            (Allowed::All, Allowed::Some(_)) => Allowed::All,
            (_, n) => n,
        };
    });
    Ok(Value::Null)
}

fn register_php_function_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    this_xp(vm, a)?;
    let ns = s8(&a.str(0));
    let name = s8(&a.str(1));
    if ns == PHP_NS {
        return vm.fthrow(a, "ValueError", "Argument #1 ($namespaceURI) must not be \"http://php.net/xpath\" because it is reserved by PHP");
    }
    if name.is_empty() || name.contains(':') || !goro_xml::chars::is_ncname(&name) {
        return vm.fthrow(a, "ValueError", "Argument #2 ($name) must be a valid callback name");
    }
    let cb = a.val(2);
    with_xp(a.this_obj(), |x| {
        x.ns_functions.retain(|(n, l, _)| !(n == &ns && l == &name));
        x.ns_functions.push((ns, name, cb));
    });
    Ok(Value::Null)
}

fn quote(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if a.str(0).as_bytes().contains(&0) {
        return vm.fthrow(a, "ValueError", "Argument #1 ($str) must not contain any null bytes");
    }
    let s = s8(&a.str(0));
    let out = if !s.contains('\'') {
        format!("'{s}'")
    } else if !s.contains('"') {
        format!("\"{s}\"")
    } else {
        // Alternate quoting styles (observed on PHP 8.5, test
        // DOMXPath_quote.phpt).
        let mut parts = Vec::new();
        let mut rest = s.as_str();
        while !rest.is_empty() {
            let n = rest.find('\'').unwrap_or(rest.len());
            if n > 0 {
                parts.push(format!("'{}'", &rest[..n]));
                rest = &rest[n..];
            } else {
                let m = rest.find('"').unwrap_or(rest.len());
                parts.push(format!("\"{}\"", &rest[..m]));
                rest = &rest[m..];
            }
        }
        format!("concat({})", parts.join(","))
    };
    Ok(sv(&out))
}

/// The PHP-calling function handler. The arena is lent back to the store
/// while PHP code runs.
struct Handler {
    vm: *mut Vm,
    fname: String,
    allowed: Allowed,
    ns_functions: Vec<(String, String, Value)>,
    modern: bool,
}

/// What a PHP error inside XPath carries back.
struct Pending(Unwind);

impl Handler {
    fn call_php(
        &mut self,
        arena: &mut Arena,
        callable: &Value,
        args: Vec<xpath::Value>,
        as_string: bool,
    ) -> Result<xpath::Value, XPathError> {
        // SAFETY: the VM outlives the evaluation; no other borrow of it
        // is active while the XPath engine runs.
        let vm = unsafe { &mut *self.vm };
        xmlstore::with(|s| std::mem::swap(&mut s.arena, arena));
        let r = self.call_inner(vm, callable, args, as_string);
        xmlstore::with(|s| std::mem::swap(&mut s.arena, arena));
        r
    }

    fn call_inner(
        &mut self,
        vm: &mut Vm,
        callable: &Value,
        args: Vec<xpath::Value>,
        as_string: bool,
    ) -> Result<xpath::Value, XPathError> {
        let mut php_args = Vec::new();
        for v in args {
            let pv = match v {
                xpath::Value::NodeSet(ns) => {
                    if as_string {
                        let s = with_arena(|ar| {
                            ns.first().map_or(String::new(), |&n| xpath::string_value(ar, n))
                        });
                        sv(&s)
                    } else {
                        let mut arr = PhpArray::new();
                        for n in ns {
                            match wrap_node(vm, n) {
                                Ok(o) => {
                                    let _ = arr.push(o);
                                }
                                Err(u) => return Err(XPathError::callback("", Some(Box::new(Pending(u))))),
                            }
                        }
                        Value::Array(arr)
                    }
                }
                xpath::Value::Boolean(b) => {
                    if as_string {
                        sv(if b { "1" } else { "" })
                    } else {
                        Value::Bool(b)
                    }
                }
                xpath::Value::Number(f) => {
                    if as_string {
                        sv(&xpath::number_to_string(f))
                    } else {
                        Value::Float(f)
                    }
                }
                xpath::Value::String(s) => sv(&s),
            };
            php_args.push(pv);
        }
        let r = match vm.call_value(callable, php_args) {
            Ok(v) => v,
            Err(u) => return Err(XPathError::callback("", Some(Box::new(Pending(u))))),
        };
        match r.deref() {
            Value::Str(s) => Ok(xpath::Value::String(s8(&s))),
            Value::Int(i) => Ok(xpath::Value::Number(i as f64)),
            Value::Float(f) => Ok(xpath::Value::Number(f)),
            Value::Bool(b) => Ok(xpath::Value::Boolean(b)),
            Value::Null => Ok(xpath::Value::String(String::new())),
            Value::Object(o) => {
                if let Some(n) = node_of(&o) {
                    return Ok(xpath::Value::NodeSet(vec![n]));
                }
                let msg = "Only objects that are instances of DOMNode can be converted to an XPath expression".to_string();
                Err(XPathError::callback(msg, None))
            }
            Value::Array(_) => Err(XPathError::callback(
                "A PHP Object cannot be converted to a XPath-string",
                None,
            )),
            other => {
                let s = match vm.to_php_str(&other) {
                    Ok(s) => s,
                    Err(u) => return Err(XPathError::callback("", Some(Box::new(Pending(u))))),
                };
                Ok(xpath::Value::String(s8(&s)))
            }
        }
    }
}

impl xpath::FunctionHandler for Handler {
    fn has_function(&self, ns: &str, local: &str) -> bool {
        if ns == PHP_NS {
            return local == "function" || local == "functionString";
        }
        let _ = &self.fname;
        self.ns_functions
            .iter()
            .any(|(n, l, _)| n == ns && l == local)
    }

    fn call(
        &mut self,
        arena: &mut Arena,
        ns: &str,
        local: &str,
        mut args: Vec<xpath::Value>,
    ) -> Result<xpath::Value, XPathError> {
        if ns != PHP_NS {
            let Some((_, _, cb)) = self
                .ns_functions
                .iter()
                .find(|(n, l, _)| n == ns && l == local)
                .cloned()
            else {
                return Err(XPathError::callback("Unregistered function", None));
            };
            return self.call_php(arena, &cb, args, false);
        }
        let as_string = local == "functionString";
        if args.is_empty() {
            return Err(XPathError::callback(
                "Function name must be passed as the first argument",
                None,
            ));
        }
        let first = args.remove(0);
        let name = first.to_string_value(arena);
        let callable = match &self.allowed {
            Allowed::None => {
                return Err(XPathError::callback("No callbacks were registered", None));
            }
            Allowed::All => sv(&name),
            Allowed::Some(v) => {
                let l = name.to_ascii_lowercase();
                match v.iter().find(|(n, _)| *n == l) {
                    Some((_, c)) => c.clone(),
                    None => {
                        return Err(XPathError::callback(
                            format!("No callback handler \"{name}\" registered"),
                            None,
                        ));
                    }
                }
            }
        };
        // SAFETY: as in `call_php`.
        let vm = unsafe { &mut *self.vm };
        if matches!(self.allowed, Allowed::All) && vm.resolve_callable(&callable, false).is_err() {
            return Err(XPathError::callback(
                format!("Unable to call handler {name}()"),
                None,
            ));
        }
        let _ = (&self.fname, self.modern);
        self.call_php(arena, &callable, args, as_string)
    }
}

/// Evaluates an expression for C14N's `$xpath` option: the node-set.
pub(crate) fn eval_nodes(
    vm: &mut Vm,
    ctx_node: NodeId,
    expr: &str,
    ns: &[(String, String)],
) -> R<Vec<NodeId>> {
    let Ok(e) = xpath::compile(expr) else {
        return Ok(Vec::new());
    };
    let r = with_arena(|ar| {
        let mut c = xpath::Context::new();
        for (p, u) in ns {
            c.register_namespace(p, u);
        }
        xpath::evaluate(ar, &e, ctx_node, &mut c)
    });
    match r {
        Ok(xpath::Value::NodeSet(v)) => Ok(v),
        Ok(_) => vm.throw("TypeError", "XPath query did not return a nodeset"),
        Err(_) => Ok(Vec::new()),
    }
}

/// Runs an XPath evaluation for `query()` / `evaluate()`.
fn run(vm: &mut Vm, a: &Args) -> R<Option<xpath::Value>> {
    this_xp(vm, a)?;
    let o = a.this_obj().clone();
    let expr = s8(&a.str(0));
    let (doc, namespaces, reg_default, allowed, ns_functions) = with_xp(&o, |x| {
        (
            x.doc.id(),
            x.namespaces.clone(),
            x.register_node_ns,
            x.allowed.clone(),
            x.ns_functions.clone(),
        )
    })
    .unwrap();
    let ctx_node = match opt_arg_node(vm, a, 1)? {
        Some(n) => {
            if with_arena(|ar| ar.doc_of(n)) != doc {
                return super::throw_dom(vm, goro_xml::tree::DomError::WrongDocument);
            }
            n
        }
        None => doc,
    };
    let reg = if a.has(2) { a.bool(2) } else { reg_default };
    let fname = a.fname();
    let compiled = match xpath::compile(&expr) {
        Ok(e) => e,
        Err(e) => {
            report_xpath_error(vm, &fname, &e)?;
            return Ok(None);
        }
    };
    let modern = modern_fn(a);
    let mut handler = Handler {
        vm: vm as *mut Vm,
        fname: fname.clone(),
        allowed,
        ns_functions,
        modern,
    };
    let has_fns = !matches!(handler.allowed, Allowed::None)
        || !handler.ns_functions.is_empty()
        || expr.contains("php:");
    let (r, ns_nodes) = if has_fns {
        let mut arena = xmlstore::with(|s| std::mem::take(&mut s.arena));
        let mut c = xpath::Context::new();
        c.namespaces = namespaces;
        c.register_node_ns = reg;
        c.functions = Some(&mut handler);
        let r = xpath::evaluate(&mut arena, &compiled, ctx_node, &mut c);
        let nsn = std::mem::take(&mut c.namespace_nodes);
        drop(c);
        xmlstore::with(|s| s.arena = arena);
        (r, nsn)
    } else {
        with_arena(|ar| {
            let mut c = xpath::Context::new();
            c.namespaces = namespaces;
            c.register_node_ns = reg;
            let r = xpath::evaluate(ar, &compiled, ctx_node, &mut c);
            (r, std::mem::take(&mut c.namespace_nodes))
        })
    };
    match r {
        Ok(v) => {
            NS_NODES.with(|n| n.borrow_mut().extend(ns_nodes));
            Ok(Some(v))
        }
        Err(mut e) => {
            with_arena(|ar| {
                for n in ns_nodes {
                    ar.free_namespace_node(n);
                }
            });
            if let Some(p) = e.payload.take()
                && let Ok(p) = p.downcast::<Pending>()
            {
                return Err(p.0);
            }
            report_xpath_error(vm, &fname, &e)?;
            Ok(None)
        }
    }
}

thread_local! {
    /// Namespace nodes of the last results, freed once wrapped.
    static NS_NODES: std::cell::RefCell<Vec<NodeId>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Frees the namespace nodes no PHP object took.
fn free_ns_nodes() {
    let v = NS_NODES.with(|n| std::mem::take(&mut *n.borrow_mut()));
    if v.is_empty() {
        return;
    }
    with_arena(|ar| {
        for n in v {
            ar.free_namespace_node(n);
        }
    });
}

fn report_xpath_error(vm: &mut Vm, fname: &str, e: &XPathError) -> R<()> {
    if e.kind == xpath::XPathErrorKind::Callback {
        if e.message.is_empty() {
            return Ok(());
        }
        return vm.throw("Error", &e.message);
    }
    let err = goro_xml::error::XmlError {
        domain: goro_xml::error::Domain::XPath,
        code: e.code() as u32,
        level: goro_xml::error::LEVEL_ERROR,
        message: format!("{}\n", e.message),
        file: None,
        line: 0,
        column: 0,
    };
    if libxml::record(vm, err) {
        return Ok(());
    }
    vm.warn(&format!("{fname}(): {}", e.message))
}

/// A node-set result as a list object (namespace nodes wrapped first).
fn node_list(vm: &mut Vm, v: Vec<NodeId>, modern: bool) -> R<Value> {
    // Namespace nodes must get their objects before the list holds them.
    let r = new_list(vm, ListKind::Static(v), modern);
    free_ns_nodes();
    r
}

fn evaluate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let modern = modern_fn(a);
    let Some(v) = run(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    Ok(match v {
        xpath::Value::NodeSet(ns) => node_list(vm, ns, modern)?,
        xpath::Value::Boolean(b) => Value::Bool(b),
        xpath::Value::Number(f) => Value::Float(f),
        xpath::Value::String(s) => Value::Str(PhpStr::from(s)),
    })
}

fn query(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let modern = modern_fn(a);
    let Some(v) = run(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    match v {
        xpath::Value::NodeSet(ns) => node_list(vm, ns, modern),
        _ => {
            if modern {
                return vm.throw(
                    "TypeError",
                    "Dom\\XPath::query(): Argument #1 ($expression) must be an expression that evaluates to a node-set",
                );
            }
            node_list(vm, Vec::new(), modern)
        }
    }
}

/// `$document` and `$registerNodeNamespaces`.
pub fn xpath_prop(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (name, is_set) = super::props::hook_name(a);
    let o = a.this_obj().clone();
    if with_xp(&o, |_| ()).is_none() {
        let c = vm.class_of(&o).name.to_string_lossy();
        return vm.throw("Error", &format!("Couldn't fetch {c}"));
    }
    match (name.as_str(), is_set) {
        ("document", false) => {
            let d = with_xp(&o, |x| x.doc.id()).unwrap();
            wrap_node(vm, d)
        }
        ("registerNodeNamespaces", false) => {
            Ok(Value::Bool(with_xp(&o, |x| x.register_node_ns).unwrap()))
        }
        ("registerNodeNamespaces", true) => {
            let b = a.bool(0);
            with_xp(&o, |x| x.register_node_ns = b);
            Ok(Value::Null)
        }
        _ => {
            let c = vm.class_of(&o).name.to_string_lossy();
            vm.throw(
                "Error",
                &format!("Cannot modify readonly property {c}::${name}"),
            )
        }
    }
}

pub fn xpath_hooks() -> NativeHooks {
    NativeHooks {
        debug_props: Some(|vm, o| {
            let mut out = vm.plain_props_array(o, true);
            let d = with_xp(o, |x| x.doc.id())?;
            let _ = d;
            out.insert(
                goro_runtime::Key::Str(PhpStr::from("document")),
                Value::from(&b"(object value omitted)"[..]),
            );
            let r = with_xp(o, |x| x.register_node_ns)?;
            out.insert(
                goro_runtime::Key::Str(PhpStr::from("registerNodeNamespaces")),
                Value::Bool(r),
            );
            Some(out)
        }),
        props_for: Some(|vm, o| Some(vm.plain_props_array(o, true))),
        clone: Some(|_vm, from, to| {
            let data = with_xp(from, |x| {
                (
                    x.doc.id(),
                    x.namespaces.clone(),
                    x.register_node_ns,
                    x.allowed.clone(),
                    x.ns_functions.clone(),
                )
            });
            if let Some((d, ns, r, al, nf)) = data {
                let doc = xmlstore::with(|s| s.hold(d));
                *to.obj().native.borrow_mut() = Some(Box::new(XPathObj {
                    doc,
                    namespaces: ns,
                    register_node_ns: r,
                    allowed: al,
                    ns_functions: nf,
                }));
            }
        }),
        ..Default::default()
    }
}
