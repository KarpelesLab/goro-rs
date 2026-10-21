//! DOMXPath (xpath.c, xpath_callbacks.c)

use super::node::{args, b};
use super::*;
use crate::xpath::{self, XErr, XValue};

const PHP_XPATH_NS: &str = "http://php.net/xpath";

#[derive(Clone, Default)]
pub struct CbNs {
    /// registerPhpFunctions() with no restriction
    pub all: bool,
    pub funcs: Vec<(String, Value)>,
}

pub struct XPathState {
    pub doc: NodeId,
    pub register_node_ns: bool,
    /// registerNamespace() prefixes
    pub ns: Vec<(String, String)>,
    pub php_ns: Option<CbNs>,
    pub custom: Vec<(String, CbNs)>,
}

fn with_state<R>(o: &Obj, f: impl FnOnce(&mut XPathState) -> R) -> Option<R> {
    let d = data_of(o)?;
    let k = d.kind.borrow();
    match &*k {
        Kind::XPath(st) => Some(f(&mut st.borrow_mut())),
        _ => None,
    }
}

fn invalid_ctx(vm: &mut Vm) -> R<Value> {
    throw_error(vm, b"Error", "Invalid XPath Context");
    Err(())
}

pub fn construct(vm: &mut Vm, this: &Obj, a: &[Value]) -> R<()> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let dobj = a.obj_of(vm, 0, "document", "DOMDocument", false)?.unwrap();
    let reg = a.bool(vm, 1, "registerNodeNS", true)?;
    let d = need_node(vm, &dobj)?;
    attach(
        this,
        Kind::XPath(RefCell::new(XPathState {
            doc: d,
            register_node_ns: reg,
            ns: Vec::new(),
            php_ns: None,
            custom: Vec::new(),
        })),
    );
    Ok(())
}

fn r_document(vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    match with_state(o, |s| s.doc) {
        Some(d) => Ok(node_obj(vm, d)),
        None => {
            throw_dom(vm, INVALID_STATE_ERR, true);
            Err(())
        }
    }
}

fn r_register_node_ns(_vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    Ok(b(with_state(o, |s| s.register_node_ns).unwrap_or(true)))
}

fn w_register_node_ns(_vm: &mut Vm, o: &Obj, _n: NodeId, v: &Value) -> R<()> {
    let on = v.is_truthy();
    with_state(o, |s| s.register_node_ns = on);
    Ok(())
}

pub fn xpath_props() -> Vec<Prop> {
    vec![
        p("document", r_document, None, "mixed"),
        p(
            "registerNodeNamespaces",
            r_register_node_ns,
            Some(w_register_node_ns),
            "bool",
        ),
    ]
}

fn m_register_namespace(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let prefix = a.str(vm, 0, "prefix")?;
    let uri = a.str(vm, 1, "namespace")?;
    match with_state(o, |s| {
        if prefix.is_empty() {
            return false;
        }
        s.ns.retain(|(p, _)| *p != prefix);
        s.ns.push((prefix.clone(), uri.clone()));
        true
    }) {
        Some(r) => Ok(b(r)),
        None => invalid_ctx(vm),
    }
}

/// Convert an XPath value to the PHP callback argument.
fn arg_to_php(vm: &mut Vm, v: XValue, to_string: bool) -> Value {
    match v {
        XValue::Str(s) => Value::String(PhpString::from_string(s)),
        XValue::Bool(b_) => b(b_),
        XValue::Num(n) => Value::Double(n),
        XValue::NodeSet(nodes) => {
            if to_string {
                let s = nodes
                    .first()
                    .map(|&n| tree::with(|t| xpath::string_value(t, n)))
                    .unwrap_or_default();
                Value::String(PhpString::from_string(s))
            } else {
                let mut arr = PhpArray::new();
                for n in nodes {
                    arr.push(node_obj(vm, n));
                }
                Value::Array(Rc::new(RefCell::new(arr)))
            }
        }
    }
}

fn dispatch(
    vm: &mut Vm,
    ns: Option<&CbNs>,
    fname: &str,
    params: Vec<Value>,
) -> Result<XValue, XErr> {
    let ns = match ns {
        Some(n) => n,
        None => {
            throw_error(vm, b"Error", "No callbacks were registered");
            return Err(XErr::Ext);
        }
    };
    let r = if ns.all {
        vm.call_callback(
            &Value::String(PhpString::from_string(fname.to_string())),
            &params,
        )
    } else {
        match ns.funcs.iter().find(|(n, _)| n == fname) {
            Some((_, cb)) => {
                let cb = cb.clone();
                vm.call_callback(&cb, &params)
            }
            None => {
                throw_error(
                    vm,
                    b"Error",
                    &format!("No callback handler \"{}\" registered", fname),
                );
                return Err(XErr::Ext);
            }
        }
    };
    let rv = match r {
        Ok(v) => v,
        Err(_) => return Err(XErr::Ext),
    };
    if vm.current_exception.is_some() {
        return Err(XErr::Ext);
    }
    match rv.deref() {
        Value::Object(o) => {
            if obj_is(vm, &o, "DOMNode") {
                match node_of(&o) {
                    Some(n) => Ok(XValue::NodeSet(vec![n])),
                    None => Ok(XValue::NodeSet(Vec::new())),
                }
            } else {
                throw_error(
                    vm,
                    b"TypeError",
                    "Only objects that are instances of DOM nodes can be converted to an XPath expression",
                );
                Err(XErr::Ext)
            }
        }
        Value::True => Ok(XValue::Bool(true)),
        Value::False => Ok(XValue::Bool(false)),
        Value::Undef => Ok(XValue::Str(String::new())),
        other => {
            let s = vm.value_to_string(&other);
            if vm.current_exception.is_some() {
                return Err(XErr::Ext);
            }
            Ok(XValue::Str(s.to_string_lossy()))
        }
    }
}

fn eval(vm: &mut Vm, o: &Obj, a: &[Value], query: bool) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 3)?;
    let expr = a.str(vm, 0, "expression")?;
    let ctxo = a.obj_of(vm, 1, "contextNode", "DOMNode", true)?;
    let st = with_state(o, |s| {
        (
            s.doc,
            s.register_node_ns,
            s.ns.clone(),
            s.php_ns.clone(),
            s.custom.clone(),
        )
    });
    let (doc, reg_default, regns, php_ns, custom) = match st {
        Some(x) => x,
        None => return invalid_ctx(vm),
    };
    let reg = a.bool(vm, 2, "registerNodeNS", reg_default)?;
    let mut nodep = match &ctxo {
        Some(co) => Some(need_node(vm, co)?),
        None => None,
    };
    if nodep.is_none() {
        nodep = tree::with(|t| t.root_element(doc));
    }
    if let Some(n) = nodep {
        if doc_of_node(n) != Some(doc) {
            throw_error(vm, b"Error", "Node from wrong document");
            return Err(());
        }
    }
    let mut nslist: Vec<(String, String)> = Vec::new();
    if reg {
        if let Some(n) = nodep {
            nslist = tree::with(|t| xpath::ns_list(t, n));
        }
    }
    for (p_, u) in &regns {
        if !nslist.iter().any(|(x, _)| x == p_) {
            nslist.push((p_.clone(), u.clone()));
        }
    }
    let fname = a.f.clone();
    let start = nodep.unwrap_or(doc);
    let res = {
        let vmr: &mut Vm = vm;
        let mut hook =
            |uri: Option<&str>, name: &str, vals: Vec<XValue>| -> Option<Result<XValue, XErr>> {
                let uri = uri?;
                if uri == PHP_XPATH_NS && (name == "function" || name == "functionString") {
                    let to_string = name == "functionString";
                    if vals.is_empty() {
                        throw_error(
                            vmr,
                            b"Error",
                            "Function name must be passed as the first argument",
                        );
                        return Some(Err(XErr::Ext));
                    }
                    let mut it = vals.into_iter();
                    let first = it.next().unwrap();
                    let params: Vec<Value> = it.map(|v| arg_to_php(vmr, v, to_string)).collect();
                    let fname = match first {
                        XValue::Str(s) => s,
                        _ => {
                            throw_error(vmr, b"TypeError", "Handler name must be a string");
                            return Some(Err(XErr::Ext));
                        }
                    };
                    return Some(dispatch(vmr, php_ns.as_ref(), &fname, params));
                }
                let ns = custom.iter().find(|(u, _)| u == uri).map(|(_, n)| n)?;
                if !ns.funcs.iter().any(|(n, _)| n == name) {
                    return None;
                }
                let params: Vec<Value> = vals
                    .into_iter()
                    .map(|v| arg_to_php(vmr, v, false))
                    .collect();
                Some(dispatch(vmr, Some(ns), name, params))
            };
        let mut ctx = xpath::Ctx {
            ns: &nslist,
            vars: &[],
            ext: Some(&mut hook),
            doc: Some(doc),
        };
        xpath::eval_ctx(&expr, start, &mut ctx)
    };
    let v = match res {
        Ok(v) => v,
        Err(e) => {
            if vm.current_exception.is_some() {
                return Err(());
            }
            xpath::report_error(vm, &fname, &e, &expr);
            return Ok(Value::False);
        }
    };
    if vm.current_exception.is_some() {
        return Err(());
    }
    match v {
        XValue::NodeSet(nodes) => {
            let mut items = Vec::new();
            for n in nodes {
                items.push(node_obj(vm, n));
            }
            Ok(list_obj(vm, ListKind::Static(items)))
        }
        _ if query => Ok(list_obj(vm, ListKind::Static(Vec::new()))),
        XValue::Bool(x) => Ok(b(x)),
        XValue::Num(n) => Ok(Value::Double(n)),
        XValue::Str(s) => Ok(Value::String(PhpString::from_string(s))),
    }
}

fn m_query(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    eval(vm, o, a, true)
}

fn m_evaluate(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    eval(vm, o, a, false)
}

fn is_callable(vm: &mut Vm, v: &Value) -> bool {
    match vm.functions.get(&b"is_callable".to_vec()).copied() {
        Some(f) => matches!(f(vm, &[v.clone()]), Ok(Value::True)),
        None => true,
    }
}

fn callable_error(v: &Value) -> String {
    match v {
        Value::String(s) => {
            let st = s.to_string_lossy();
            if let Some(i) = st.find("::") {
                format!("class \"{}\" not found", &st[..i])
            } else {
                format!("function \"{}\" not found or invalid function name", st)
            }
        }
        Value::Array(_) => "array callback must have exactly two members".to_string(),
        _ => "no array or string given".to_string(),
    }
}

fn valid_cb_name(name: &str, ncname: bool) -> bool {
    if name.is_empty() || name.contains('\0') {
        return false;
    }
    if ncname && !crate::parser::is_valid_ncname(name) {
        return false;
    }
    true
}

fn m_register_php_functions(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 1)?;
    let restrict = a.raw(0);
    if with_state(o, |_| ()).is_none() {
        return invalid_ctx(vm);
    }
    match restrict {
        None | Some(Value::Null) => {
            with_state(o, |s| {
                let ns = s.php_ns.get_or_insert_with(CbNs::default);
                ns.all = true;
            });
        }
        Some(Value::Array(arr)) => {
            let entries: Vec<(goro_core::array::ArrayKey, Value)> = arr
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let mut add: Vec<(String, Value)> = Vec::new();
            for (k, v) in entries {
                let v = v.deref();
                if !is_callable(vm, &v) {
                    throw_error(
                        vm,
                        b"TypeError",
                        &format!(
                            "{}(): Argument #1 ($restrict) must be an array with valid callbacks as values, {}",
                            a.f,
                            callable_error(&v)
                        ),
                    );
                    return Err(());
                }
                let name = match k {
                    goro_core::array::ArrayKey::String(s) => s.to_string_lossy(),
                    goro_core::array::ArrayKey::Int(_) => {
                        let s = vm.value_to_string(&v);
                        s.to_string_lossy()
                    }
                };
                if !valid_cb_name(&name, false) {
                    throw_error(
                        vm,
                        b"ValueError",
                        &format!(
                            "{}(): Argument #1 ($restrict) must be an array containing valid callback names",
                            a.f
                        ),
                    );
                    return Err(());
                }
                add.push((name, v));
            }
            with_state(o, |s| {
                let ns = s.php_ns.get_or_insert_with(CbNs::default);
                for (n, v) in add {
                    ns.funcs.retain(|(x, _)| *x != n);
                    ns.funcs.push((n, v));
                }
                ns.all = false;
            });
        }
        Some(v) => {
            let name = match &v {
                Value::String(s) => s.to_string_lossy(),
                Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
                    v.to_php_string().to_string_lossy()
                }
                other => {
                    a.type_err(vm, 0, "restrict", "array|string|null", other);
                    return Err(());
                }
            };
            if !valid_cb_name(&name, false) {
                throw_error(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #1 ($restrict) must be a valid callback name",
                        a.f
                    ),
                );
                return Err(());
            }
            let cb = Value::String(PhpString::from_string(name.clone()));
            if !is_callable(vm, &cb) {
                throw_error(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): Argument #1 ($restrict) must be a callable, {}",
                        a.f,
                        callable_error(&cb)
                    ),
                );
                return Err(());
            }
            with_state(o, |s| {
                let ns = s.php_ns.get_or_insert_with(CbNs::default);
                ns.funcs.retain(|(x, _)| *x != name);
                ns.funcs.push((name, cb));
                ns.all = false;
            });
        }
    }
    Ok(Value::Null)
}

fn m_register_php_function_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 3, 3)?;
    let ns = a.str(vm, 0, "namespaceURI")?;
    let name = a.str(vm, 1, "name")?;
    let cb = a.raw(2).unwrap_or(Value::Null);
    if ns.contains('\0') {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($namespaceURI) must not contain any null bytes",
                a.f
            ),
        );
        return Err(());
    }
    if name.contains('\0') {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($name) must not contain any null bytes",
                a.f
            ),
        );
        return Err(());
    }
    if !is_callable(vm, &cb) {
        throw_error(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #3 ($callable) must be a valid callback, {}",
                a.f,
                callable_error(&cb)
            ),
        );
        return Err(());
    }
    if ns == PHP_XPATH_NS {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($namespaceURI) must not be \"http://php.net/xpath\" because it is reserved by PHP",
                a.f
            ),
        );
        return Err(());
    }
    if !valid_cb_name(&name, true) {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($name) must be a valid callback name",
                a.f
            ),
        );
        return Err(());
    }
    if with_state(o, |s| {
        let idx = match s.custom.iter().position(|(u, _)| *u == ns) {
            Some(i) => i,
            None => {
                s.custom.push((ns.clone(), CbNs::default()));
                s.custom.len() - 1
            }
        };
        let e = &mut s.custom[idx].1;
        e.funcs.retain(|(x, _)| *x != name);
        e.funcs.push((name.clone(), cb.clone()));
        e.all = false;
    })
    .is_none()
    {
        return invalid_ctx(vm);
    }
    Ok(Value::Null)
}

pub fn quote(vm: &mut Vm, a: &[Value]) -> R<Value> {
    let a = Args::new("DOMXPath::quote".to_string(), a);
    a.count(vm, 1, 1)?;
    let input = a.str_bytes(vm, 0, "str")?;
    if input.contains(&0) {
        throw_error(
            vm,
            b"ValueError",
            "DOMXPath::quote(): Argument #1 ($str) must not contain any null bytes",
        );
        return Err(());
    }
    let out: Vec<u8> = if !input.contains(&b'\'') {
        let mut v = vec![b'\''];
        v.extend(&input);
        v.push(b'\'');
        v
    } else if !input.contains(&b'"') {
        let mut v = vec![b'"'];
        v.extend(&input);
        v.push(b'"');
        v
    } else {
        let mut v = b"concat(".to_vec();
        let mut i = 0;
        while i < input.len() {
            let rest = &input[i..];
            let ds = rest.iter().position(|&c| c == b'\'').unwrap_or(rest.len());
            let dd = rest.iter().position(|&c| c == b'"').unwrap_or(rest.len());
            let until = ds.max(dd);
            let q = if ds > dd { b'\'' } else { b'"' };
            v.push(q);
            v.extend(&rest[..until]);
            v.push(q);
            i += until;
            v.push(b',');
        }
        let l = v.len();
        v[l - 1] = b')';
        v
    };
    Ok(Value::String(PhpString::from_vec(out)))
}

pub fn xpath_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("registerNamespace", m_register_namespace),
        ("query", m_query),
        ("evaluate", m_evaluate),
        ("registerPhpFunctions", m_register_php_functions),
        ("registerPhpFunctionNS", m_register_php_function_ns),
    ]
}
