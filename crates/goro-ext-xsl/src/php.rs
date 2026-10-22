//! The XSLTProcessor class (php_xsl.c, xsltprocessor.c) and the PHP
//! callbacks of php:function() / registerPHPFunctionNS() (xpath_callbacks.c).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, NativeObjectHandlers, Vm};
use goro_ext_xml::dom::{self, Args};
use goro_ext_xml::tree::{self, NType, NodeId};

use crate::style::{Compiler, Sheet};
use crate::transform::{PhpHost, Transform};
use crate::util::{XSLT_NAMESPACE, tw};
use crate::xpath::{self, PCtx, XObj};

type Obj = Rc<RefCell<PhpObject>>;
type R<T> = Result<T, ()>;

const CLASS: &[u8] = b"XSLTProcessor";
const CLASS_LOWER: &[u8] = b"xsltprocessor";
pub const PHP_XSL_NS: &str = "http://php.net/xsl";

/// XSL_SECPREF_DEFAULT
const SECPREF_DEFAULT: i64 = 44;
/// xsltMaxDepth / xsltMaxVars
const MAX_DEPTH: i64 = 3000;
const MAX_VARS: i64 = 15000;

#[derive(Clone, Default)]
pub struct CbNs {
    /// registerPHPFunctions() without restriction
    pub all: bool,
    pub funcs: Vec<(String, Value)>,
}

#[derive(Default)]
pub struct XslState {
    sheet: Option<Rc<Sheet>>,
    has_keys: bool,
    /// setParameter() values, by (clark notation) name, in insertion order
    params: Vec<(String, String)>,
    sec_prefs: i64,
    profiling: Option<String>,
    php_ns: Option<CbNs>,
    custom: Vec<(String, CbNs)>,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    write_property: Some(h_write_property),
    unset_property: Some(h_unset_property),
    ..NativeObjectHandlers::EMPTY
};

fn state_of(o: &Obj) -> Option<Rc<RefCell<XslState>>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<RefCell<XslState>>().ok()
}

fn attach(o: &Obj) {
    let st = XslState {
        sec_prefs: SECPREF_DEFAULT,
        ..Default::default()
    };
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(RefCell::new(st)),
        handlers: &HANDLERS,
    });
}

/// xsl_objects_new
fn create_object(_vm: &mut Vm, o: &Obj) {
    {
        let mut b = o.borrow_mut();
        b.set_property(b"maxTemplateDepth".to_vec(), Value::Long(MAX_DEPTH));
        b.set_property(b"maxTemplateVars".to_vec(), Value::Long(MAX_VARS));
    }
    attach(o);
}

fn is_validated(name: &[u8]) -> bool {
    name == b"maxTemplateDepth" || name == b"maxTemplateVars"
}

fn class_display(o: &Obj) -> String {
    goro_core::value::display_class_name(&o.borrow().class_name).to_string()
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    dom::throw_error(vm, class, msg);
}

/// xsl_objects_get_property_ptr_ptr + xsl_objects_read_property: the
/// validated properties cannot be fetched by reference.
fn ref_fetch(o: &Obj, name: &[u8]) -> Option<String> {
    if is_validated(name) {
        return Some(format!(
            "Indirect modification of {}::${} is not allowed",
            class_display(o),
            String::from_utf8_lossy(name)
        ));
    }
    None
}

/// xsl_objects_write_property_with_validation
fn h_write_property(vm: &mut Vm, o: &Obj, name: &[u8], v: &Value) -> bool {
    if !is_validated(name) {
        return false;
    }
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let n = match vm.zpp_coerce_scalar(goro_core::arginfo::T_INT, v, strict, line) {
        Ok(Some(Value::Long(n))) => n,
        Ok(_) => {
            let m = format!(
                "Cannot assign {} to property {}::${} of type int",
                Vm::value_type_name(v),
                class_display(o),
                String::from_utf8_lossy(name)
            );
            throw(vm, b"TypeError", &m);
            return true;
        }
        Err(()) => return true,
    };
    if n < 0 {
        let m = format!(
            "{}::${} must be greater than or equal to 0",
            class_display(o),
            String::from_utf8_lossy(name)
        );
        throw(vm, b"ValueError", &m);
        return true;
    }
    o.borrow_mut().set_property(name.to_vec(), Value::Long(n));
    true
}

fn h_unset_property(vm: &mut Vm, o: &Obj, name: &[u8]) -> bool {
    if is_validated(name) {
        let m = format!(
            "Cannot unset {}::${}",
            class_display(o),
            String::from_utf8_lossy(name)
        );
        throw(vm, b"Error", &m);
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Registration

const METHODS: &[&str] = &[
    "importStylesheet",
    "transformToDoc",
    "transformToUri",
    "transformToXml",
    "setParameter",
    "getParameter",
    "removeParameter",
    "hasExsltSupport",
    "registerPHPFunctions",
    "registerPHPFunctionNS",
    "setProfiling",
    "setSecurityPrefs",
    "getSecurityPrefs",
];

fn canon(method: &[u8]) -> Option<&'static str> {
    METHODS
        .iter()
        .copied()
        .find(|m| m.as_bytes().eq_ignore_ascii_case(method))
}

fn has_method(_class: &[u8], method: &[u8], _is_static: bool) -> bool {
    canon(method).is_some()
}

fn param_names(_class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    Some(match canon(method)? {
        "importStylesheet" => &["stylesheet"],
        "transformToDoc" => &["document", "returnClass"],
        "transformToUri" => &["document", "uri"],
        "transformToXml" => &["document"],
        "setParameter" => &["namespace", "name", "value"],
        "getParameter" | "removeParameter" => &["namespace", "name"],
        "registerPHPFunctions" => &["functions"],
        "registerPHPFunctionNS" => &["namespaceURI", "name", "callable"],
        "setProfiling" => &["filename"],
        "setSecurityPrefs" => &["preferences"],
        _ => &[],
    })
}

const CONSTANTS: &[(&str, i64)] = &[
    ("XSL_CLONE_AUTO", 0),
    ("XSL_CLONE_NEVER", -1),
    ("XSL_CLONE_ALWAYS", 1),
    ("XSL_SECPREF_NONE", 0),
    ("XSL_SECPREF_READ_FILE", 2),
    ("XSL_SECPREF_WRITE_FILE", 4),
    ("XSL_SECPREF_CREATE_DIRECTORY", 8),
    ("XSL_SECPREF_READ_NETWORK", 16),
    ("XSL_SECPREF_WRITE_NETWORK", 32),
    ("XSL_SECPREF_DEFAULT", SECPREF_DEFAULT),
    ("LIBXSLT_VERSION", 10145),
    ("LIBEXSLT_VERSION", 825),
];

pub fn register(vm: &mut Vm) {
    let mut ce = match goro_core::stubs::class_entry(vm, CLASS) {
        Some(stub) => {
            let mut ce = stub.clone();
            ce.methods.clear();
            ce
        }
        None => ClassEntry::new(CLASS.to_vec()),
    };
    ce.name = CLASS.to_vec();
    vm.classes.insert(CLASS_LOWER.to_vec(), ce);
    vm.native_class_handlers.insert(
        CLASS_LOWER.to_vec(),
        NativeClassHandler {
            has_method,
            call: call_method,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: Some(param_names),
        },
    );
    vm.object_create_handlers
        .insert(CLASS_LOWER.to_vec(), create_object);
    vm.ref_fetch_handlers
        .insert(CLASS_LOWER.to_vec(), ref_fetch);
    let mut names: Vec<Vec<u8>> = Vec::new();
    for &(n, v) in CONSTANTS {
        vm.constants.insert(n.as_bytes().to_vec(), Value::Long(v));
        names.push(n.as_bytes().to_vec());
    }
    for (n, v) in [
        ("LIBXSLT_DOTTED_VERSION", "1.1.45"),
        ("LIBEXSLT_DOTTED_VERSION", "0.8.25"),
    ] {
        vm.constants.insert(
            n.as_bytes().to_vec(),
            Value::String(PhpString::from_bytes(v.as_bytes())),
        );
        names.push(n.as_bytes().to_vec());
    }
    vm.module_constants.push((crate::NAME.to_vec(), names));
}

fn call_method(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let Some(m) = canon(method) else {
        return Value::Null;
    };
    let fname = format!("XSLTProcessor::{}", m);
    let Some(Value::Object(o)) = this.map(|t| t.deref()) else {
        throw(
            vm,
            b"Error",
            &format!("Non-static method {}() cannot be called statically", fname),
        );
        return Value::Null;
    };
    if state_of(&o).is_none() {
        attach(&o);
    }
    let a = Args::new(fname.clone(), args);
    let r = dom::with_active_fn(fname.clone(), || match m {
        "importStylesheet" => import_stylesheet(vm, &o, &a),
        "transformToDoc" => transform_to_doc(vm, &o, &a),
        "transformToUri" => transform_to_uri(vm, &o, &a),
        "transformToXml" => transform_to_xml(vm, &o, &a),
        "setParameter" => set_parameter(vm, &o, &a),
        "getParameter" => get_parameter(vm, &o, &a, false),
        "removeParameter" => get_parameter(vm, &o, &a, true),
        "hasExsltSupport" => a.count(vm, 0, 0).map(|_| Value::True),
        "registerPHPFunctions" => register_php_functions(vm, &o, &a),
        "registerPHPFunctionNS" => register_php_function_ns(vm, &o, &a),
        "setProfiling" => set_profiling(vm, &o, &a),
        "setSecurityPrefs" => a
            .count(vm, 1, 1)
            .and_then(|_| a.long(vm, 0, "preferences", 0))
            .map(|n| {
                let st = state_of(&o).unwrap();
                let old = std::mem::replace(&mut st.borrow_mut().sec_prefs, n);
                Value::Long(old)
            }),
        "getSecurityPrefs" => a
            .count(vm, 0, 0)
            .map(|_| Value::Long(state_of(&o).unwrap().borrow().sec_prefs)),
        _ => Ok(Value::Null),
    });
    r.unwrap_or(Value::Null)
}

// ---------------------------------------------------------------------------
// Helpers

/// php_libxml_import_node: the node of a DOM or SimpleXML object.
fn import_node(o: &Obj) -> Option<NodeId> {
    match goro_ext_xml::simplexml::sxe_of(o) {
        Some(sx) => tree::with(|t| goro_ext_xml::simplexml::first_node(t, &sx, sx.node.get())),
        None => dom::node_of(o),
    }
}

fn doc_of(n: NodeId) -> Option<NodeId> {
    tw(|t| if t.is_doc(n) { Some(n) } else { t.nodes[n].doc })
}

/// The object argument #1 (zpp "o").
fn obj_arg(vm: &mut Vm, a: &Args, name: &str) -> R<Obj> {
    match a.raw(0) {
        Some(Value::Object(o)) => Ok(o),
        Some(v) => {
            a.type_err(vm, 0, name, "object", &v);
            Err(())
        }
        None => {
            a.type_err(vm, 0, name, "object", &Value::Null);
            Err(())
        }
    }
}

/// zend_std_read_property() of a typed property (Error when it is not
/// initialized).
fn read_typed_bool(vm: &mut Vm, o: &Obj, name: &str) -> R<bool> {
    let v = o
        .borrow()
        .properties
        .iter()
        .find(|(k, _)| k == name.as_bytes())
        .map(|(_, v)| v.deref());
    match v {
        None | Some(Value::Undef) => {
            throw(
                vm,
                b"Error",
                &format!(
                    "Typed property {}::${} must not be accessed before initialization",
                    class_display(o),
                    name
                ),
            );
            Err(())
        }
        Some(v) => Ok(v.is_truthy()),
    }
}

fn read_long(o: &Obj, name: &str) -> i64 {
    o.borrow().get_property(name.as_bytes()).to_long()
}

// ---------------------------------------------------------------------------
// importStylesheet

fn import_stylesheet(vm: &mut Vm, o: &Obj, a: &Args) -> R<Value> {
    a.count(vm, 1, 1)?;
    let so = obj_arg(vm, a, "stylesheet")?;
    let doc = import_node(&so).and_then(doc_of);
    let Some(doc) = doc else {
        throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #1 ($stylesheet) must be a valid XML node",
                a.f
            ),
        );
        return Err(());
    };
    let newdoc = tw(|t| {
        let url = t.doc_info(doc).and_then(|i| i.url.clone());
        let nd = t.copy_doc(doc, true);
        if let Some(i) = t.doc_info_mut(nd) {
            i.url = url;
        }
        // xmlStaticCopyNode() only keeps the line numbers of elements
        let mut stack: Vec<NodeId> = t.nodes[nd].children.into_iter().collect();
        while let Some(n) = stack.pop() {
            if t.nodes[n].t != NType::Element {
                t.nodes[n].line = 0;
            }
            if let Some(nx) = t.nodes[n].next {
                stack.push(nx);
            }
            if t.nodes[n].t == NType::Element {
                if let Some(c) = t.nodes[n].children {
                    stack.push(c);
                }
            }
        }
        nd
    });
    let Some(mut sheet) = Compiler::parse_stylesheet_doc(vm, newdoc) else {
        return Ok(Value::False);
    };
    // xsl_apply_ns_hash_corrections: namespace declarations of a modern
    // document are attributes
    // (the copy may have declared the namespaces of elements, as
    // xmlStaticCopyNode() reconciles them)
    if dom::doc_props(Some(doc)).class_type == 2 {
        for (k, v) in build_ns_map(vm, doc) {
            sheet.styles[0].ns_hash.entry(k).or_insert(v);
        }
    }
    let clone = read_typed_bool(vm, o, "cloneDocument")?;
    let has_keys = clone
        || tw(|t| {
            let Some(root) = t.root_element(sheet.styles[0].doc) else {
                return false;
            };
            let mut c = t.nodes[root].children;
            while let Some(n) = c {
                if t.nodes[n].t == NType::Element
                    && t.nodes[n].name == "key"
                    && t.nodes[n]
                        .ns
                        .map(|ns| t.nss[ns].href == XSLT_NAMESPACE)
                        .unwrap_or(false)
                {
                    return true;
                }
                c = t.nodes[n].next;
            }
            false
        });
    let st = state_of(o).unwrap();
    let mut st = st.borrow_mut();
    st.has_keys = has_keys;
    st.sheet = Some(Rc::new(sheet));
    Ok(Value::True)
}

/// xsl_build_ns_map
fn build_ns_map(vm: &mut Vm, doc: NodeId) -> std::collections::HashMap<String, String> {
    const XMLNS: &str = "http://www.w3.org/2000/xmlns/";
    let mut map = std::collections::HashMap::new();
    let entries: Vec<(NodeId, String, String)> = tw(|t| {
        let mut out = Vec::new();
        let Some(root) = t.root_element(doc) else {
            return out;
        };
        let mut cur = Some(root);
        while let Some(n) = cur {
            if t.nodes[n].t == NType::Element {
                if let Some(ns) = t.nodes[n].ns {
                    if let Some(p) = t.nss[ns].prefix.clone() {
                        out.push((n, p, t.nss[ns].href.clone()));
                    }
                }
                for a in t.attrs_of(n) {
                    let Some(ns) = t.nodes[a].ns else { continue };
                    if t.nss[ns].prefix.is_none() || t.nss[ns].href != XMLNS {
                        continue;
                    }
                    if t.nodes[a].children.is_none() {
                        continue;
                    }
                    let v = t.node_get_content(a);
                    out.push((n, t.nodes[a].name.clone(), v));
                }
            }
            cur = goro_ext_xml::dom::modern::next_in_tree_order(t, n, Some(doc));
        }
        out
    });
    for (n, p, h) in entries {
        match map.get(&p) {
            Some(u) if *u != h => {
                crate::util::transform_error(
                    vm,
                    false,
                    true,
                    Some(n),
                    &format!("Namespaces prefix {} used for multiple namespaces\n", p),
                );
            }
            Some(_) => {}
            None => {
                map.insert(p, h);
            }
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Transformation

struct Host {
    php_ns: Option<CbNs>,
    custom: Vec<(String, CbNs)>,
    /// the document handed to user callbacks (intern->doc)
    doc: NodeId,
    /// objects returned by callbacks, kept alive for the transformation
    node_list: Vec<Value>,
    modern: bool,
}

impl PhpHost for Host {
    fn has_function(&self, name: &str, uri: &str) -> bool {
        if uri == PHP_XSL_NS {
            return name == "function" || name == "functionString";
        }
        self.custom
            .iter()
            .any(|(u, ns)| u == uri && ns.funcs.iter().any(|(n, _)| n == name))
    }

    fn call(&mut self, t: &mut Transform, p: &mut PCtx, name: &str, uri: &str, nargs: usize) {
        let ok = if uri == PHP_XSL_NS {
            self.call_php_ns(t, p, nargs, name == "functionString")
        } else {
            let params = self.fetch_args(t, p, nargs, false);
            let ns = self
                .custom
                .iter()
                .find(|(u, _)| u == uri)
                .map(|(_, n)| n.clone());
            self.dispatch(t, p, ns.as_ref(), name, params)
        };
        if !ok {
            p.stack.push(XObj::Str(String::new()));
        }
    }
}

impl Host {
    /// php_dom_xpath_callback_fetch_args
    fn fetch_args(
        &mut self,
        t: &mut Transform,
        p: &mut PCtx,
        n: usize,
        to_string: bool,
    ) -> Vec<Value> {
        let mut params = vec![Value::Null; n];
        for i in (0..n).rev() {
            let obj = p.stack.pop().unwrap_or(XObj::Str(String::new()));
            params[i] = match obj {
                XObj::Str(s) => Value::String(PhpString::from_string(s)),
                XObj::Bool(b) => {
                    if b {
                        Value::True
                    } else {
                        Value::False
                    }
                }
                XObj::Num(d) => Value::Double(d),
                XObj::NodeSet(nodes) => {
                    if to_string {
                        Value::String(PhpString::from_string(xpath::cast_to_string(
                            &XObj::NodeSet(nodes),
                        )))
                    } else {
                        let mut arr = PhpArray::new();
                        for node in nodes {
                            arr.push(self.proxy(t, node));
                        }
                        Value::Array(Rc::new(RefCell::new(arr)))
                    }
                }
                other => Value::String(PhpString::from_string(xpath::cast_to_string(&other))),
            };
        }
        params
    }

    /// xsl_proxy_factory: nodes of other documents than the source document
    /// are copied into it.
    fn proxy(&mut self, t: &mut Transform, node: NodeId) -> Value {
        let is_ns = tw(|tr| tr.nodes[node].t == NType::NamespaceDecl);
        let node = if is_ns {
            node
        } else {
            let nd = doc_of(node);
            if nd != Some(t.main_doc) {
                let target = self.doc;
                tw(|tr| tr.copy_node(node, Some(target), 1, None))
            } else {
                node
            }
        };
        dom::node_obj(t.vm, node)
    }

    /// php_dom_xpath_callbacks_call_php_ns
    fn call_php_ns(
        &mut self,
        t: &mut Transform,
        p: &mut PCtx,
        nargs: usize,
        to_string: bool,
    ) -> bool {
        if nargs == 0 {
            throw(
                t.vm,
                b"Error",
                "Function name must be passed as the first argument",
            );
            return false;
        }
        let params = self.fetch_args(t, p, nargs - 1, to_string);
        let fname = match p.stack.pop() {
            Some(XObj::Str(s)) => s,
            _ => {
                throw(t.vm, b"TypeError", "Handler name must be a string");
                return false;
            }
        };
        let ns = self.php_ns.clone();
        self.dispatch(t, p, ns.as_ref(), &fname, params)
    }

    /// php_dom_xpath_callback_dispatch
    fn dispatch(
        &mut self,
        t: &mut Transform,
        p: &mut PCtx,
        ns: Option<&CbNs>,
        fname: &str,
        params: Vec<Value>,
    ) -> bool {
        let vm = &mut *t.vm;
        let Some(ns) = ns else {
            throw(vm, b"Error", "No callbacks were registered");
            return false;
        };
        if vm.current_exception.is_some() {
            return false;
        }
        let r = if ns.all {
            let cb = Value::String(PhpString::from_string(fname.to_string()));
            // zend_call_function(): zend_is_callable_ex() first
            let chk = vm.zend_is_callable_ex(&cb, 0);
            if vm.exit_requested {
                return false;
            }
            if !chk.ok {
                let prev = vm.current_exception.take();
                let err = callable_error(vm, &cb);
                let msg = format!("Invalid callback {}, {}", fname, err);
                let e = vm.create_exception(b"Error", &msg, vm.current_line);
                if let Some(pv) = prev {
                    Vm::exception_set_previous(&e, pv);
                }
                vm.current_exception = Some(e);
                return false;
            }
            vm.call_callback(&cb, &params)
        } else {
            match ns.funcs.iter().find(|(n, _)| n == fname) {
                Some((_, cb)) => {
                    let cb = cb.clone();
                    vm.call_callback(&cb, &params)
                }
                None => {
                    throw(
                        vm,
                        b"Error",
                        &format!("No callback handler \"{}\" registered", fname),
                    );
                    return false;
                }
            }
        };
        let rv = match r {
            Ok(v) => v,
            Err(e) => {
                if vm.current_exception.is_none() {
                    throw(vm, b"Error", &e.message);
                }
                return false;
            }
        };
        if vm.current_exception.is_some() {
            return false;
        }
        match rv.deref() {
            Value::Undef => {}
            Value::Object(ob) => {
                let base = if self.modern { "Dom\\Node" } else { "DOMNode" };
                if dom::obj_is(vm, &ob, base) {
                    let n = dom::node_of(&ob);
                    self.node_list.push(Value::Object(ob));
                    p.stack.push(XObj::NodeSet(n.into_iter().collect()));
                } else {
                    throw(
                        vm,
                        b"TypeError",
                        "Only objects that are instances of DOM nodes can be converted to an XPath expression",
                    );
                    return false;
                }
            }
            Value::True => p.stack.push(XObj::Bool(true)),
            Value::False => p.stack.push(XObj::Bool(false)),
            other => {
                let s = vm.value_to_string(&other);
                if vm.current_exception.is_some() {
                    return false;
                }
                p.stack.push(XObj::Str(s.to_string_lossy()));
            }
        }
        true
    }
}

/// php_xsl_apply_stylesheet: the result document, or None.
fn apply_stylesheet(
    vm: &mut Vm,
    o: &Obj,
    a: &Args,
    docobj: &Obj,
) -> R<Option<(NodeId, Rc<Sheet>)>> {
    let doc = import_node(docobj).and_then(doc_of);
    let Some(mut doc) = doc else {
        throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #1 ($document) must be a valid XML node",
                a.f
            ),
        );
        return Err(());
    };
    let st = state_of(o).unwrap();
    let Some(sheet) = st.borrow().sheet.clone() else {
        throw(
            vm,
            b"Error",
            &format!(
                "{}() can only be called after a stylesheet has been imported",
                a.f
            ),
        );
        return Err(());
    };
    let profiling = st.borrow().profiling.clone();
    if let Some(p) = &profiling {
        // the profile is written by libxslt; create the file
        let _ = goro_ext_xml::io::write_resource(vm, p, b"");
    }
    let has_keys = st.borrow().has_keys;
    if has_keys {
        doc = tw(|t| t.copy_doc(doc, true));
    }
    let xinclude = read_typed_bool(vm, o, "doXInclude")?;
    let max_depth = read_long(o, "maxTemplateDepth");
    let max_vars = read_long(o, "maxTemplateVars");
    let (params, sec_prefs, php_ns, custom) = {
        let s = st.borrow();
        (
            s.params.clone(),
            s.sec_prefs,
            s.php_ns.clone(),
            s.custom.clone(),
        )
    };
    let modern = dom::doc_props(Some(doc)).class_type == 2;
    let mut host = Host {
        php_ns,
        custom,
        doc,
        node_list: Vec::new(),
        modern,
    };
    let res = {
        let mut t = Transform::new(vm, sheet.clone(), doc);
        t.xinclude = xinclude;
        t.max_template_depth = max_depth.min(i32::MAX as i64) as i32;
        t.max_template_vars = max_vars.min(i32::MAX as i64) as i32;
        t.sec_prefs = sec_prefs;
        t.main_doc = doc;
        let hp: *mut dyn PhpHost = &mut host;
        t.php = Some(hp);
        let r = t.apply_stylesheet(doc, &params);
        t.php = None;
        r
    };
    drop(host);
    Ok(res.map(|r| (r, sheet)))
}

fn transform_to_doc(vm: &mut Vm, o: &Obj, a: &Args) -> R<Value> {
    a.count(vm, 1, 2)?;
    let docobj = obj_arg(vm, a, "document")?;
    // "C!": the return class
    let ret_class: Option<Vec<u8>> = match a.raw(1) {
        None | Some(Value::Null) => None,
        Some(_) => {
            let name = a.str(vm, 1, "returnClass")?;
            let name = name.strip_prefix('\\').unwrap_or(&name).to_string();
            let lower = name.to_ascii_lowercase().into_bytes();
            if vm.lookup_class(&lower).is_none() {
                vm.try_autoload_class(name.as_bytes());
                if vm.current_exception.is_some() {
                    return Err(());
                }
            }
            match vm.lookup_class(&lower).map(|c| c.name.to_vec()) {
                Some(n) => Some(n),
                None => {
                    throw(
                        vm,
                        b"TypeError",
                        &format!(
                            "{}(): Argument #2 ($returnClass) must be a valid class name or null, {} given",
                            a.f, name
                        ),
                    );
                    return Err(());
                }
            }
        }
    };
    let Some((res, _)) = apply_stylesheet(vm, o, a, &docobj)? else {
        return Ok(Value::False);
    };
    if vm.current_exception.is_some() {
        return Err(());
    }
    match ret_class {
        Some(rc) => {
            let cur_name = docobj.borrow().class_name.clone();
            let mut root = cur_name.to_ascii_lowercase();
            for _ in 0..64 {
                match vm.classes.get(&root).and_then(|c| c.parent.clone()) {
                    Some(p) => root = p.to_ascii_lowercase(),
                    None => break,
                }
            }
            let rcl = rc.to_ascii_lowercase();
            if rcl != root && !vm.is_subclass_of(&rcl, &root) {
                throw(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): Argument #2 ($returnClass) must be a class name compatible with {}, {} given",
                        a.f,
                        goro_core::value::display_class_name(&cur_name),
                        String::from_utf8_lossy(&rc)
                    ),
                );
                return Err(());
            }
            if root == b"simplexmlelement" {
                return Ok(goro_ext_xml::simplexml::wrap_node(vm, &rc, res, res));
            }
            let ob = dom::new_object(vm, &rc);
            dom::attach(&ob, dom::Kind::Node(res));
            dom::set_wrapper(res, &ob);
            Ok(Value::Object(ob))
        }
        None => Ok(dom::node_obj(vm, res)),
    }
}

fn transform_to_uri(vm: &mut Vm, o: &Obj, a: &Args) -> R<Value> {
    a.count(vm, 2, 2)?;
    let docobj = obj_arg(vm, a, "document")?;
    let uri = a.path_str(vm, 1, "uri")?;
    let r = apply_stylesheet(vm, o, a, &docobj)?;
    let mut ret = -1i64;
    if let Some((res, sheet)) = r {
        if let Ok(bytes) = crate::output::save_result(vm, &sheet, res) {
            ret = goro_ext_xml::io::write_resource_len(vm, &uri, &bytes);
        }
    }
    if vm.current_exception.is_some() {
        return Err(());
    }
    Ok(Value::Long(ret))
}

fn transform_to_xml(vm: &mut Vm, o: &Obj, a: &Args) -> R<Value> {
    a.count(vm, 1, 1)?;
    let docobj = obj_arg(vm, a, "document")?;
    let r = apply_stylesheet(vm, o, a, &docobj)?;
    if vm.current_exception.is_some() {
        return Err(());
    }
    match r {
        Some((res, sheet)) => match crate::output::save_result(vm, &sheet, res) {
            Ok(b) if b.is_empty() => Ok(Value::Null),
            Ok(b) => Ok(Value::String(PhpString::from_vec(b))),
            Err(()) => Ok(Value::False),
        },
        None => Ok(Value::False),
    }
}

// ---------------------------------------------------------------------------
// Parameters

/// xsl_create_parameter_key
fn parameter_key(vm: &mut Vm, a: &Args, ns: &str, name: &str) -> R<String> {
    if ns.is_empty() {
        return Ok(name.to_string());
    }
    if name.starts_with('{') {
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($name) must not use clark notation when argument #1 ($namespace) is not empty",
                a.f
            ),
        );
        return Err(());
    }
    if !name.starts_with(':') && name.contains(':') {
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($name) must not be a QName when argument #1 ($namespace) is not empty",
                a.f
            ),
        );
        return Err(());
    }
    Ok(format!("{{{}}}{}", ns, name))
}

fn put_param(st: &mut XslState, k: String, v: String) {
    match st.params.iter_mut().find(|(x, _)| *x == k) {
        Some(e) => e.1 = v,
        None => st.params.push((k, v)),
    }
}

fn set_parameter(vm: &mut Vm, o: &Obj, a: &Args) -> R<Value> {
    a.count(vm, 2, 3)?;
    let ns = a.path_str(vm, 0, "namespace")?;
    let name_v = a.raw(1).unwrap_or(Value::Null);
    let value = a.opt_path_str(vm, 2, "value");
    let st = state_of(o).unwrap();
    if let Value::Array(arr) = &name_v {
        let value = value?;
        if value.is_some() {
            throw(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #3 ($value) must be null when argument #2 ($name) is an array",
                    a.f
                ),
            );
            return Err(());
        }
        let entries: Vec<(ArrayKey, Value)> = arr
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (k, v) in entries {
            let key = match k {
                ArrayKey::String(s) => s.to_string_lossy(),
                ArrayKey::Int(_) => {
                    throw(
                        vm,
                        b"TypeError",
                        &format!(
                            "{}(): Argument #2 ($name) must contain only string keys",
                            a.f
                        ),
                    );
                    return Err(());
                }
            };
            if key.contains('\0') {
                throw(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #3 ($value) must not contain keys with any null bytes",
                        a.f
                    ),
                );
                return Err(());
            }
            let hk = parameter_key(vm, a, &ns, &key)?;
            let v = v.deref();
            let s = if let Value::Array(_) = v {
                vm.emit_warning("Array to string conversion");
                PhpString::from_bytes(b"Array")
            } else {
                vm.value_to_string(&v)
            };
            if vm.current_exception.is_some() {
                return Err(());
            }
            let s = s.to_string_lossy();
            if s.contains('\0') {
                throw(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #3 ($value) must not contain values with any null bytes",
                        a.f
                    ),
                );
                return Err(());
            }
            put_param(&mut st.borrow_mut(), hk, s);
        }
        return Ok(Value::True);
    }
    let name = match &name_v {
        Value::String(s) => s.to_string_lossy(),
        Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
            name_v.to_php_string().to_string_lossy()
        }
        v if dom::is_closure(v) => {
            a.type_err(vm, 1, "name", "array|string", v);
            return Err(());
        }
        Value::Object(_) => match a.str(vm, 1, "name") {
            Ok(s) => s,
            Err(()) => {
                if vm.current_exception.is_some() {
                    // re-word as array|string
                    vm.current_exception = None;
                }
                a.type_err(vm, 1, "name", "array|string", &name_v);
                return Err(());
            }
        },
        Value::Null => {
            vm.emit_deprecated(&format!(
                "{}(): Passing null to parameter #2 ($name) of type array|string is deprecated",
                a.f
            ));
            String::new()
        }
        v => {
            a.type_err(vm, 1, "name", "array|string", v);
            return Err(());
        }
    };
    let value = value?;
    let Some(value) = value else {
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #3 ($value) cannot be null when argument #2 ($name) is a string",
                a.f
            ),
        );
        return Err(());
    };
    if name.contains('\0') {
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($name) must not contain any null bytes",
                a.f
            ),
        );
        return Err(());
    }
    let key = parameter_key(vm, a, &ns, &name)?;
    put_param(&mut st.borrow_mut(), key, value);
    Ok(Value::True)
}

fn get_parameter(vm: &mut Vm, o: &Obj, a: &Args, remove: bool) -> R<Value> {
    a.count(vm, 2, 2)?;
    let ns = a.path_str(vm, 0, "namespace")?;
    let name = a.path_str(vm, 1, "name")?;
    let key = parameter_key(vm, a, &ns, &name)?;
    let st = state_of(o).unwrap();
    let mut st = st.borrow_mut();
    let pos = st.params.iter().position(|(k, _)| *k == key);
    if remove {
        return Ok(match pos {
            Some(i) => {
                st.params.remove(i);
                Value::True
            }
            None => Value::False,
        });
    }
    Ok(match pos {
        Some(i) => Value::String(PhpString::from_string(st.params[i].1.clone())),
        None => Value::False,
    })
}

fn set_profiling(vm: &mut Vm, o: &Obj, a: &Args) -> R<Value> {
    a.count(vm, 1, 1)?;
    let f = a.opt_path_str(vm, 0, "filename")?;
    state_of(o).unwrap().borrow_mut().profiling = f;
    Ok(Value::True)
}

// ---------------------------------------------------------------------------
// PHP functions

fn is_callable(vm: &mut Vm, v: &Value) -> bool {
    vm.zend_is_callable_ex(v, 0).ok
}

fn callable_error(vm: &Vm, v: &Value) -> String {
    vm.is_callable_pure(v, goro_core::callable::IS_CALLABLE_SUPPRESS_DEPRECATIONS)
        .error
        .unwrap_or_default()
}

fn valid_cb_name(name: &str, ncname: bool) -> bool {
    if name.is_empty() || name.contains('\0') {
        return false;
    }
    if ncname && !goro_ext_xml::parser::is_valid_ncname(name) {
        return false;
    }
    true
}

fn register_php_functions(vm: &mut Vm, o: &Obj, a: &Args) -> R<Value> {
    a.count(vm, 0, 1)?;
    let st = state_of(o).unwrap();
    match a.raw(0) {
        None | Some(Value::Null) => {
            st.borrow_mut().php_ns.get_or_insert_with(CbNs::default).all = true;
        }
        Some(Value::Array(arr)) => {
            let entries: Vec<(ArrayKey, Value)> = arr
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let mut add: Vec<(String, Value)> = Vec::new();
            for (k, v) in entries {
                let v = v.deref();
                if !is_callable(vm, &v) {
                    let e = callable_error(vm, &v);
                    throw(
                        vm,
                        b"TypeError",
                        &format!(
                            "{}(): Argument #1 ($functions) must be an array with valid callbacks as values, {}",
                            a.f, e
                        ),
                    );
                    return Err(());
                }
                let name = match k {
                    ArrayKey::String(s) => s.to_string_lossy(),
                    ArrayKey::Int(_) => {
                        if dom::is_closure(&v) {
                            throw(
                                vm,
                                b"Error",
                                "Object of class Closure could not be converted to string",
                            );
                            return Err(());
                        }
                        let s = vm.value_to_string(&v);
                        if vm.current_exception.is_some() {
                            return Err(());
                        }
                        s.to_string_lossy()
                    }
                };
                if !valid_cb_name(&name, false) {
                    throw(
                        vm,
                        b"ValueError",
                        &format!(
                            "{}(): Argument #1 ($functions) must be an array containing valid callback names",
                            a.f
                        ),
                    );
                    return Err(());
                }
                add.push((name, v));
            }
            let mut s = st.borrow_mut();
            let ns = s.php_ns.get_or_insert_with(CbNs::default);
            for (n, v) in add {
                ns.funcs.retain(|(x, _)| *x != n);
                ns.funcs.push((n, v));
            }
            ns.all = false;
        }
        Some(v) => {
            let name = match &v {
                c if dom::is_closure(c) => {
                    a.type_err(vm, 0, "functions", "array|string|null", c);
                    return Err(());
                }
                Value::String(s) => s.to_string_lossy(),
                Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
                    v.to_php_string().to_string_lossy()
                }
                other => {
                    a.type_err(vm, 0, "functions", "array|string|null", other);
                    return Err(());
                }
            };
            if !valid_cb_name(&name, false) {
                throw(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #1 ($functions) must be a valid callback name",
                        a.f
                    ),
                );
                return Err(());
            }
            let cb = Value::String(PhpString::from_string(name.clone()));
            if !is_callable(vm, &cb) {
                let e = callable_error(vm, &cb);
                throw(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): Argument #1 ($functions) must be a callable, {}",
                        a.f, e
                    ),
                );
                return Err(());
            }
            let mut s = st.borrow_mut();
            let ns = s.php_ns.get_or_insert_with(CbNs::default);
            ns.funcs.retain(|(x, _)| *x != name);
            ns.funcs.push((name, cb));
            ns.all = false;
        }
    }
    Ok(Value::Null)
}

fn register_php_function_ns(vm: &mut Vm, o: &Obj, a: &Args) -> R<Value> {
    a.count(vm, 3, 3)?;
    let ns = a.path_str(vm, 0, "namespaceURI")?;
    let name = a.path_str(vm, 1, "name")?;
    let cb = a.raw(2).unwrap_or(Value::Null);
    if !is_callable(vm, &cb) {
        let e = callable_error(vm, &cb);
        throw(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #3 ($callable) must be a valid callback, {}",
                a.f, e
            ),
        );
        return Err(());
    }
    if ns == PHP_XSL_NS {
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($namespaceURI) must not be \"http://php.net/xsl\" because it is reserved by PHP",
                a.f
            ),
        );
        return Err(());
    }
    if !valid_cb_name(&name, true) {
        throw(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($name) must be a valid callback name",
                a.f
            ),
        );
        return Err(());
    }
    let st = state_of(o).unwrap();
    let mut s = st.borrow_mut();
    let idx = match s.custom.iter().position(|(u, _)| *u == ns) {
        Some(i) => i,
        None => {
            s.custom.push((ns.clone(), CbNs::default()));
            s.custom.len() - 1
        }
    };
    let e = &mut s.custom[idx].1;
    e.funcs.retain(|(x, _)| *x != name);
    e.funcs.push((name, cb));
    Ok(Value::Null)
}
