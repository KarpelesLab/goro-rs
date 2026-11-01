//! ext/xsl (PHP manual: XSLTProcessor): the `XSLTProcessor` class on
//! goro-xml's XSLT engine ([`goro_xml::xslt`]), with `php:function()`
//! callbacks and security preferences. Messages come from the ext/xsl tests.

use std::rc::Rc;

use goro_runtime::{KeyRef, NativeData, ObjRef, PhpArray, PhpStr, Value};
use goro_xml::error::{Domain, LEVEL_WARNING, XmlError};
use goro_xml::xpath::{self, XPathError};
use goro_xml::xslt::{self, Stylesheet};
use goro_xml::{Arena, NodeId};

use super::libxml::{self, Style};
use super::xmlstore::{self, NodeHold};
use crate::class::{DisplayName, NativeHooks};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Unwind, Vm};

const PHP_NS: &str = "http://php.net/xsl";

const SEC_READ_FILE: i64 = 2;
const SEC_WRITE_FILE: i64 = 4;
const SEC_CREATE_DIRECTORY: i64 = 8;
const SEC_READ_NETWORK: i64 = 16;
const SEC_WRITE_NETWORK: i64 = 32;
const SEC_DEFAULT: i64 = SEC_WRITE_FILE | SEC_CREATE_DIRECTORY | SEC_WRITE_NETWORK;

const CLASS: &str = r#"<?php
class XSLTProcessor {
    public bool $doXInclude = false;
    public bool $cloneDocument = false;
    public int $maxTemplateDepth { get {} set {} }
    public int $maxTemplateVars { get {} set {} }
    public function importStylesheet(object $stylesheet): bool {}
    public function transformToDoc(object $document, ?string $returnClass = null): object|false {}
    public function transformToUri(object $document, string $uri): int {}
    public function transformToXml(object $document): string|null|false {}
    public function setParameter(string $namespace, array|string $name, ?string $value = null): bool {}
    public function getParameter(string $namespace, string $name): string|false {}
    public function removeParameter(string $namespace, string $name): bool {}
    public function hasExsltSupport(): bool {}
    public function registerPHPFunctions(array|string|null $functions = null): void {}
    public function registerPHPFunctionNS(string $namespaceURI, string $name, callable $callable): void {}
    public function setProfiling(?string $filename): true {}
    public function setSecurityPrefs(int $preferences): int {}
    public function getSecurityPrefs(): int {}
}"#;

pub static MODULE: Module = Module {
    name: "xsl",
    classes: &[("xsltprocessor", CLASS)],
    methods: &[
        ("xsltprocessor::importstylesheet", import_stylesheet),
        ("xsltprocessor::transformtodoc", transform_to_doc),
        ("xsltprocessor::transformtouri", transform_to_uri),
        ("xsltprocessor::transformtoxml", transform_to_xml),
        ("xsltprocessor::setparameter", set_parameter),
        ("xsltprocessor::getparameter", get_parameter),
        ("xsltprocessor::removeparameter", remove_parameter),
        ("xsltprocessor::hasexsltsupport", has_exslt_support),
        (
            "xsltprocessor::registerphpfunctions",
            register_php_functions,
        ),
        (
            "xsltprocessor::registerphpfunctionns",
            register_php_function_ns,
        ),
        ("xsltprocessor::setprofiling", set_profiling),
        ("xsltprocessor::setsecurityprefs", set_security_prefs),
        ("xsltprocessor::getsecurityprefs", get_security_prefs),
        ("xsltprocessor::$", limit_prop),
    ],
    hooks: &[("xsltprocessor", hooks)],
    constants: &[
        ("XSL_CLONE_AUTO", ConstVal::Int(0)),
        ("XSL_CLONE_NEVER", ConstVal::Int(-1)),
        ("XSL_CLONE_ALWAYS", ConstVal::Int(1)),
        ("XSL_SECPREF_NONE", ConstVal::Int(0)),
        ("XSL_SECPREF_READ_FILE", ConstVal::Int(SEC_READ_FILE)),
        ("XSL_SECPREF_WRITE_FILE", ConstVal::Int(SEC_WRITE_FILE)),
        (
            "XSL_SECPREF_CREATE_DIRECTORY",
            ConstVal::Int(SEC_CREATE_DIRECTORY),
        ),
        ("XSL_SECPREF_READ_NETWORK", ConstVal::Int(SEC_READ_NETWORK)),
        (
            "XSL_SECPREF_WRITE_NETWORK",
            ConstVal::Int(SEC_WRITE_NETWORK),
        ),
        ("XSL_SECPREF_DEFAULT", ConstVal::Int(SEC_DEFAULT)),
        ("LIBXSLT_VERSION", ConstVal::Int(10145)),
        ("LIBXSLT_DOTTED_VERSION", ConstVal::Str("1.1.45")),
        ("LIBEXSLT_VERSION", ConstVal::Int(824)),
        ("LIBEXSLT_DOTTED_VERSION", ConstVal::Str("0.8.24")),
    ],
    info: Some(|_| {
        "XSL => enabled\nlibxslt Version => 1.1.45\nlibxslt compiled against libxml Version => 2.15.2\nEXSLT => enabled\nlibexslt Version => 0.8.24\n"
            .to_string()
    }),
    ..Module::EMPTY
};

/// Which PHP functions a stylesheet may call.
#[derive(Clone)]
enum Allowed {
    None,
    All,
    /// Names (lower case) with their callables.
    Some(Vec<(String, Value)>),
}

/// The native data of an `XSLTProcessor`.
struct XslObj {
    sheet: Option<Rc<Stylesheet>>,
    /// Keeps the stylesheet's documents alive.
    holds: Vec<NodeHold>,
    /// By expanded name, in the order set.
    params: Vec<(String, String)>,
    max_depth: i64,
    max_vars: i64,
    prefs: i64,
    allowed: Allowed,
    ns_functions: Vec<(String, String, Value)>,
    profile: Option<String>,
}

impl Default for XslObj {
    fn default() -> XslObj {
        XslObj {
            sheet: None,
            holds: Vec::new(),
            params: Vec::new(),
            // The defaults of test maxTemplateDepth_modification_validation_bypass.phpt.
            max_depth: 3000,
            max_vars: 15000,
            prefs: SEC_DEFAULT,
            allowed: Allowed::None,
            ns_functions: Vec::new(),
            profile: None,
        }
    }
}

impl NativeData for XslObj {
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

fn with<T>(o: &ObjRef, f: impl FnOnce(&mut XslObj) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<XslObj>().map(f)
}

fn s8(s: &PhpStr) -> String {
    libxml::utf8(s)
}

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(XslObj::default()));
        }),
        clone: Some(|_, from, to| {
            let data = with(from, |x| {
                (
                    x.sheet.clone(),
                    x.params.clone(),
                    x.max_depth,
                    x.max_vars,
                    x.prefs,
                    x.allowed.clone(),
                    x.ns_functions.clone(),
                    x.profile.clone(),
                )
            });
            let mut obj = XslObj::default();
            if let Some((sheet, params, d, v, p, al, nf, pr)) = data {
                if let Some(s) = &sheet {
                    obj.holds =
                        xmlstore::with(|st| s.modules.iter().map(|m| st.hold(m.doc)).collect());
                }
                obj.sheet = sheet;
                obj.params = params;
                obj.max_depth = d;
                obj.max_vars = v;
                obj.prefs = p;
                obj.allowed = al;
                obj.ns_functions = nf;
                obj.profile = pr;
            }
            *to.obj().native.borrow_mut() = Some(Box::new(obj));
        }),
        debug_props: Some(|vm, o| {
            let out = vm.plain_props_array(o, true);
            let (d, v) = with(o, |x| (x.max_depth, x.max_vars))?;
            // Declared after the two plain properties, before dynamic ones.
            let mut ordered = PhpArray::new();
            let plain = |k: &KeyRef<'_>| {
                matches!(k, KeyRef::Str(n)
                    if n.as_bytes() == b"doXInclude" || n.as_bytes() == b"cloneDocument")
            };
            for (k, val) in out.iter() {
                if plain(&k)
                    && let KeyRef::Str(n) = k
                {
                    ordered.insert(
                        goro_runtime::Key::Str(PhpStr::from(n.as_bytes())),
                        val.clone(),
                    );
                }
            }
            ordered.insert(
                goro_runtime::Key::Str(PhpStr::from("maxTemplateDepth")),
                Value::Int(d),
            );
            ordered.insert(
                goro_runtime::Key::Str(PhpStr::from("maxTemplateVars")),
                Value::Int(v),
            );
            for (k, val) in out.iter() {
                if plain(&k) {
                    continue;
                }
                let key = match k {
                    KeyRef::Str(n) => goro_runtime::Key::Str(PhpStr::from(n.as_bytes())),
                    KeyRef::Int(i) => goro_runtime::Key::Int(i),
                };
                ordered.insert(key, val.clone());
            }
            Some(ordered)
        }),
        ..Default::default()
    }
}

fn limit_prop(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (name, is_set) = super::dom::props::hook_name(a);
    let o = a.this_obj().clone();
    let depth = name == "maxTemplateDepth";
    if !is_set {
        return Ok(Value::Int(
            with(&o, |x| if depth { x.max_depth } else { x.max_vars }).unwrap_or(0),
        ));
    }
    let v = a.int(0);
    if v < 0 {
        return vm.throw(
            "ValueError",
            &format!("XSLTProcessor::${name} must be greater than or equal to 0"),
        );
    }
    with(&o, |x| {
        if depth {
            x.max_depth = v;
        } else {
            x.max_vars = v;
        }
    });
    Ok(Value::Null)
}

fn read_bool(vm: &mut Vm, o: &ObjRef, name: &str) -> R<bool> {
    let v = vm.read_prop(o, &PhpStr::from(name), false, None)?;
    Ok(matches!(v.deref(), Value::Bool(true)))
}

/// The node an object wraps: DOM or SimpleXML (PHP manual:
/// XSLTProcessor::transformToDoc).
fn node_arg(vm: &mut Vm, a: &Args, i: usize, what: &str) -> R<NodeId> {
    let n = a
        .obj(i)
        .and_then(|o| super::dom::node_of(&o).or_else(|| super::simplexml::sxe_node_of(&o)));
    match n {
        Some(n) => Ok(n),
        None => vm.fthrow(
            a,
            "TypeError",
            &format!("Argument #{} (${what}) must be a valid XML node", i + 1),
        ),
    }
}

// ----- the host ----------------------------------------------------------------

struct Host {
    vm: *mut Vm,
    fname: String,
    allowed: Allowed,
    ns_functions: Vec<(String, String, Value)>,
    prefs: i64,
    do_xinclude: bool,
    pending: Option<Unwind>,
    keep: Vec<ObjRef>,
}

fn is_network(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("ftp://")
}

impl Host {
    /// Runs PHP-side code with the arena back in the store. The caller's
    /// `arena` is out of the store for the whole transformation.
    fn in_php<T>(&mut self, arena: &mut Arena, f: impl FnOnce(&mut Self, &mut Vm) -> T) -> T {
        // SAFETY: the VM outlives the transformation, and no other borrow
        // of it is active while the XSLT engine runs.
        let vm = unsafe { &mut *self.vm };
        xmlstore::with(|s| {
            std::mem::swap(&mut s.arena, arena);
            // The engine holds node ids PHP code could otherwise free.
            s.arena.begin_defer_free();
        });
        let r = f(self, vm);
        xmlstore::with(|s| {
            s.arena.end_defer_free();
            std::mem::swap(&mut s.arena, arena);
        });
        r
    }

    fn call_inner(
        &mut self,
        vm: &mut Vm,
        callable: &Value,
        args: Vec<xpath::Value>,
        as_string: bool,
    ) -> Result<xpath::Value, XPathError> {
        let mut php_args = Vec::with_capacity(args.len());
        for v in args {
            php_args.push(match v {
                xpath::Value::NodeSet(ns) => {
                    if as_string {
                        let s = xmlstore::with(|st| {
                            ns.first()
                                .map_or(String::new(), |&n| xpath::string_value(&st.arena, n))
                        });
                        Value::Str(PhpStr::from(s))
                    } else {
                        let mut arr = PhpArray::new();
                        for n in ns {
                            match super::dom::wrap_node(vm, n) {
                                Ok(o) => {
                                    let _ = arr.push(o);
                                }
                                Err(u) => return Err(self.pend(u)),
                            }
                        }
                        Value::Array(arr)
                    }
                }
                xpath::Value::Boolean(b) => {
                    if as_string {
                        Value::Str(PhpStr::from(if b { "1" } else { "" }))
                    } else {
                        Value::Bool(b)
                    }
                }
                xpath::Value::Number(f) => {
                    if as_string {
                        Value::Str(PhpStr::from(xpath::number_to_string(f)))
                    } else {
                        Value::Float(f)
                    }
                }
                xpath::Value::String(s) => Value::Str(PhpStr::from(s)),
            });
        }
        let r = match vm.call_value(callable, php_args) {
            Ok(v) => v,
            Err(u) => return Err(self.pend(u)),
        };
        match r.deref() {
            Value::Str(s) => Ok(xpath::Value::String(s8(&s))),
            Value::Int(i) => Ok(xpath::Value::Number(i as f64)),
            Value::Float(f) => Ok(xpath::Value::Number(f)),
            Value::Bool(b) => Ok(xpath::Value::Boolean(b)),
            Value::Null => Ok(xpath::Value::String(String::new())),
            Value::Object(o) => {
                if let Some(n) = super::dom::node_of(&o) {
                    self.keep.push(o.clone());
                    return Ok(xpath::Value::NodeSet(vec![n]));
                }
                // Test xslt_non_dom_node.phpt.
                let u = vm.throw_error_msg(
                    "TypeError",
                    "Only objects that are instances of DOM nodes can be converted to an XPath expression",
                );
                Err(self.pend(u))
            }
            Value::Array(_) => {
                let u = vm.throw_error_msg(
                    "TypeError",
                    "A PHP array cannot be converted to an XPath expression",
                );
                Err(self.pend(u))
            }
            other => match vm.to_php_str(&other) {
                Ok(s) => Ok(xpath::Value::String(s8(&s))),
                Err(u) => Err(self.pend(u)),
            },
        }
    }

    fn pend(&mut self, u: Unwind) -> XPathError {
        if self.pending.is_none() {
            self.pending = Some(u);
        }
        XPathError::callback("", None)
    }

    /// `php:function()` with every PHP function allowed: the callable must
    /// resolve. An exception from the autoloader becomes the previous of
    /// the "Invalid callback" error (test throw_in_autoload.phpt).
    fn checked_callable(&mut self, vm: &mut Vm, name: &str) -> Result<Value, XPathError> {
        let callable = Value::Str(PhpStr::from(name));
        let err = match vm.resolve_callable(&callable, false) {
            Ok(_) => return Ok(callable),
            Err(Unwind::Throw(e)) => e,
            Err(u) => return Err(self.pend(u)),
        };
        let why = match name.split_once("::") {
            Some((cls, _)) => match vm.find_class(cls.as_bytes(), false) {
                Ok(None) => format!("class \"{cls}\" not found"),
                _ => vm.callable_error(&callable),
            },
            None => vm.callable_error(&callable),
        };
        let u = vm.throw_error_msg("Error", &format!("Invalid callback {name}, {why}"));
        let from_autoloader = {
            let msg = vm.get_prop_raw(&err, b"message");
            !matches!(&msg, Value::Str(m) if m.as_bytes().starts_with(b"Class \""))
        };
        if from_autoloader && let Unwind::Throw(new) = &u {
            vm.chain_previous(new, err);
        }
        Err(self.pend(u))
    }
}

impl xslt::Host for Host {
    fn report(&mut self, arena: &mut Arena, message: &str) {
        let line = format!("{}(): {message}", self.fname);
        let text = message.to_string();
        self.in_php(arena, |h, vm| {
            let err = XmlError {
                domain: Domain::Xslt,
                code: 0,
                level: LEVEL_WARNING,
                message: format!("{text}\n"),
                file: None,
                line: 0,
                column: 0,
            };
            if libxml::record(vm, err) {
                return;
            }
            if let Err(u) = vm.warn(&line) {
                h.pend(u);
            }
        });
    }

    fn resolve(&mut self, href: &str, base: Option<&str>) -> String {
        // SAFETY: as in `in_php`.
        let vm = unsafe { &*self.vm };
        libxml::resolve(vm, href, base)
    }

    fn load(&mut self, arena: &mut Arena, url: &str, stylesheet: bool) -> Option<NodeId> {
        let url = url.to_string();
        self.in_php(arena, |h, vm| {
            let fname = h.fname.clone();
            let bytes = match libxml::load_document(vm, &fname, &url) {
                Ok(Ok(b)) => b,
                Ok(Err(msg)) => {
                    let r = match msg {
                        Some(m) => vm.warn(&format!("{fname}(): {m}")),
                        None => {
                            let e = libxml::io_error(&url);
                            libxml::report(vm, &fname, &[e], Style::Dom, 0, None)
                        }
                    };
                    if let Err(u) = r {
                        h.pend(u);
                    }
                    return None;
                }
                Err(u) => {
                    h.pend(u);
                    return None;
                }
            };
            // Entities substituted and DTD defaults applied, CDATA as text
            // (XSLT 1.0 section 3: the data model has neither).
            let flags = goro_xml::xml::opt::NOENT
                | goro_xml::xml::opt::DTDLOAD
                | goro_xml::xml::opt::DTDATTR
                | goro_xml::xml::opt::NOCDATA;
            let d = match super::dom::document::parse_xml(
                vm,
                &fname,
                &bytes,
                flags,
                Some(url.clone()),
                false,
                Style::Dom,
            ) {
                Ok(d) => d,
                Err(u) => {
                    h.pend(u);
                    return None;
                }
            };
            // Test xinclude/xinclude.phpt: documents read by document().
            if h.do_xinclude
                && !stylesheet
                && let Some(d) = d
                && let Err(u) = super::dom::document::run_xinclude(vm, &fname, d, flags)
            {
                h.pend(u);
            }
            d
        })
    }

    fn read_refusal(&mut self, url: &str) -> Option<String> {
        if is_network(url) {
            return (self.prefs & SEC_READ_NETWORK != 0)
                .then(|| format!("Network read for {url} refused"));
        }
        (self.prefs & SEC_READ_FILE != 0).then(|| format!("Local file read for {url} refused"))
    }

    fn write_refusal(&mut self, url: &str) -> Option<String> {
        if is_network(url) {
            return (self.prefs & SEC_WRITE_NETWORK != 0)
                .then(|| format!("Network write for {url} refused"));
        }
        // Test bug54446.phpt.
        if self.prefs & SEC_WRITE_FILE != 0 {
            return Some(format!("File write for {url} refused"));
        }
        if self.prefs & SEC_CREATE_DIRECTORY != 0 {
            // SAFETY: as in `in_php`.
            let vm = unsafe { &*self.vm };
            let path = vm.fs_path(url.strip_prefix("file://").unwrap_or(url).as_bytes());
            if let Some(dir) = path.parent()
                && !dir.as_os_str().is_empty()
                && !dir.exists()
            {
                return Some(format!("Directory creation for {url} refused"));
            }
        }
        None
    }

    fn write(&mut self, arena: &mut Arena, url: &str, data: &[u8]) -> bool {
        let url = url.to_string();
        let data = data.to_vec();
        self.in_php(arena, |h, vm| match write_uri(vm, &url, &data) {
            Ok(n) => n.is_some(),
            Err(u) => {
                h.pend(u);
                false
            }
        })
    }

    fn has_function(&self, ns: &str, local: &str) -> bool {
        if ns == PHP_NS {
            return local == "function" || local == "functionString";
        }
        self.ns_functions
            .iter()
            .any(|(n, l, _)| n == ns && l == local)
    }

    fn call_function(
        &mut self,
        arena: &mut Arena,
        ns: &str,
        local: &str,
        mut args: Vec<xpath::Value>,
    ) -> Result<xpath::Value, XPathError> {
        if ns != PHP_NS {
            let cb = self
                .ns_functions
                .iter()
                .find(|(n, l, _)| n == ns && l == local)
                .map(|(_, _, c)| c.clone());
            let Some(cb) = cb else {
                return Err(XPathError::callback("Unregistered function", None));
            };
            return self.in_php(arena, |h, vm| h.call_inner(vm, &cb, args, false));
        }
        let as_string = local == "functionString";
        let first = if args.is_empty() {
            None
        } else {
            Some(args.remove(0))
        };
        let name = first.as_ref().map(|f| f.to_string_value(arena));
        self.in_php(arena, |h, vm| {
            // Tests php_function_edge_cases.phpt, Handler name errors.
            let Some(first) = first else {
                let u = vm.throw_error_msg(
                    "Error",
                    "Function name must be passed as the first argument",
                );
                return Err(h.pend(u));
            };
            if matches!(first, xpath::Value::Number(_) | xpath::Value::Boolean(_)) {
                let u = vm.throw_error_msg("TypeError", "Handler name must be a string");
                return Err(h.pend(u));
            }
            let name = name.unwrap_or_default();
            let callable = match h.allowed.clone() {
                Allowed::None => {
                    let u = vm.throw_error_msg("Error", "No callbacks were registered");
                    return Err(h.pend(u));
                }
                Allowed::All => h.checked_callable(vm, &name)?,
                Allowed::Some(v) => {
                    let l = name.to_ascii_lowercase();
                    match v.iter().find(|(n, _)| *n == l) {
                        Some((_, c)) => c.clone(),
                        None => {
                            let u = vm.throw_error_msg(
                                "Error",
                                &format!("No callback handler \"{name}\" registered"),
                            );
                            return Err(h.pend(u));
                        }
                    }
                }
            };
            h.call_inner(vm, &callable, args, as_string)
        })
    }

    fn stop_requested(&self) -> bool {
        self.pending.is_some()
    }
}

/// Writes bytes through PHP's streams; `None` on failure.
fn write_uri(vm: &mut Vm, uri: &str, data: &[u8]) -> R<Option<i64>> {
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    let path = if uri.starts_with("file://") {
        path
    } else {
        uri
    };
    let r = vm.call_value(
        &Value::from(&b"file_put_contents"[..]),
        vec![Value::from(path.as_bytes()), Value::from(data)],
    )?;
    Ok(match r {
        Value::Int(n) => Some(n),
        _ => None,
    })
}

// ----- methods -----------------------------------------------------------------

fn make_host(vm: &mut Vm, fname: String, o: &ObjRef, do_xinclude: bool) -> Host {
    let (allowed, ns_functions, prefs) =
        with(o, |x| (x.allowed.clone(), x.ns_functions.clone(), x.prefs)).unwrap_or((
            Allowed::None,
            Vec::new(),
            SEC_DEFAULT,
        ));
    Host {
        vm: vm as *mut Vm,
        fname,
        allowed,
        ns_functions,
        prefs,
        do_xinclude,
        pending: None,
        keep: Vec::new(),
    }
}

fn import_stylesheet(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let node = node_arg(vm, a, 0, "stylesheet")?;
    let o = a.this_obj().clone();
    // Test special_operations_with_properties.phpt.
    read_bool(vm, &o, "cloneDocument")?;
    // A copy: later changes to the document don't reach the stylesheet (test
    // XSLTProcessor_namespace_mapper_lifetime.phpt drops it at once).
    let (clone, first_hold) = xmlstore::with(|s| {
        let d = s.arena.doc_of(node);
        let c = s.arena.clone_node(d, d, true);
        (c, s.hold(c))
    });
    let mut host = make_host(vm, a.fname(), &o, false);
    let mut arena = xmlstore::with(|s| std::mem::take(&mut s.arena));
    let compiled = xslt::compile::compile(&mut arena, clone, &mut host);
    xmlstore::with(|s| s.arena = arena);
    if let Some(u) = host.pending.take() {
        return Err(u);
    }
    let Some(sheet) = compiled else {
        return Ok(Value::Bool(false));
    };
    let mut holds = vec![first_hold];
    xmlstore::with(|s| {
        for m in sheet.modules.iter().skip(1) {
            holds.push(s.hold(m.doc));
        }
    });
    let old = with(&o, |x| {
        let h = std::mem::replace(&mut x.holds, holds);
        let sh = x.sheet.replace(Rc::new(sheet));
        (h, sh)
    });
    drop(old);
    Ok(Value::Bool(true))
}

/// Runs the transformation: the result document and the stylesheet.
fn run(vm: &mut Vm, a: &Args) -> R<Option<(NodeId, Rc<Stylesheet>)>> {
    let o = a.this_obj().clone();
    let Some(sheet) = with(&o, |x| x.sheet.clone()).flatten() else {
        return vm.throw(
            "Error",
            &format!(
                "{}() can only be called after a stylesheet has been imported",
                a.fname()
            ),
        );
    };
    let node = node_arg(vm, a, 0, "document")?;
    let do_xinclude = read_bool(vm, &o, "doXInclude")?;
    let clone = read_bool(vm, &o, "cloneDocument")?;
    let (params, max_depth, max_vars, profile) = with(&o, |x| {
        (x.params.clone(), x.max_depth, x.max_vars, x.profile.clone())
    })
    .unwrap_or((Vec::new(), 3000, 15000, None));
    let mut source = xmlstore::with(|s| s.arena.doc_of(node));
    let mut clone_hold = None;
    if clone {
        // Test cloneDocument.phpt.
        let (c, h) = xmlstore::with(|s| {
            let c = s.arena.clone_node(source, source, true);
            (c, s.hold(c))
        });
        source = c;
        clone_hold = Some(h);
    }
    let mut host = make_host(vm, a.fname(), &o, do_xinclude);
    let opts = xslt::transform::Options {
        params,
        max_depth: max_depth as usize,
        max_vars: max_vars as usize,
    };
    let mut arena = xmlstore::with(|s| std::mem::take(&mut s.arena));
    let outcome = xslt::transform::transform(&mut arena, &sheet, source, opts, &mut host);
    arena.flush_deferred_frees();
    xmlstore::with(|s| s.arena = arena);
    drop(clone_hold);
    let keep = std::mem::take(&mut host.keep);
    if let Some(u) = host.pending.take() {
        if let Some(d) = outcome.doc {
            xmlstore::with(|s| s.free_doc_if_unheld(d));
        }
        drop(keep);
        return Err(u);
    }
    drop(keep);
    if let Some(p) = profile {
        // The profile's header (per-template timings are not collected).
        let text =
            b"number               match                name      mode  Calls Tot 100us Avg\n\n";
        write_uri(vm, &p, text)?;
    }
    Ok(outcome.doc.map(|d| (d, sheet)))
}

fn transform_to_doc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let class = a.opt_str(1);
    let input = a.obj(0);
    // Tests transformToDoc_class_exceptions.phpt, gh19988.phpt.
    let mut return_class = None;
    if let (Some(cn), Some(input)) = (&class, &input) {
        let Some(c) = vm.find_class(cn.as_bytes(), true)? else {
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #2 ($returnClass) must be a valid class name or null, {} given",
                    cn.to_string_lossy()
                ),
            );
        };
        let ic = vm.class_of(input);
        if !Rc::ptr_eq(&c, &ic) && !c.is_subclass_of(&ic) && !ic.is_subclass_of(&c) {
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #2 ($returnClass) must be a class name compatible with {}, {} given",
                    ic.name.dname(),
                    cn.to_string_lossy()
                ),
            );
        }
        return_class = Some(c);
    }
    let Some((doc, _)) = run(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    if let Some(c) = return_class {
        let sxe = vm
            .find_class(b"SimpleXMLElement", false)?
            .is_some_and(|b| Rc::ptr_eq(&b, &c) || c.is_subclass_of(&b));
        if sxe {
            return super::simplexml::wrap_document(vm, &c, doc);
        }
        super::dom::with_settings(doc, |s| s.doc_class = Some(c));
    }
    super::dom::wrap_node(vm, doc)
}

fn result_bytes(vm: &mut Vm, a: &Args) -> R<Option<Vec<u8>>> {
    let Some((doc, sheet)) = run(vm, a)? else {
        return Ok(None);
    };
    Ok(Some(xmlstore::with(|s| {
        let has_output = s.arena.first_child(doc).is_some();
        let bytes = if has_output {
            xslt::output::serialize(&mut s.arena, doc, &sheet.output)
        } else {
            Vec::new()
        };
        s.free_doc_if_unheld(doc);
        bytes
    })))
}

fn transform_to_xml(vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match result_bytes(vm, a)? {
        Some(b) if b.is_empty() => Value::Null,
        Some(b) => Value::from(b.as_slice()),
        None => Value::Bool(false),
    })
}

fn transform_to_uri(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let uri = s8(&a.str(1));
    let Some(bytes) = result_bytes(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    Ok(match write_uri(vm, &uri, &bytes)? {
        Some(n) => Value::Int(n),
        None => Value::Bool(false),
    })
}

/// The stored name; errors for a namespaced name written in another form
/// (test req30622_errors.phpt).
fn param_key(vm: &mut Vm, a: &Args, ns: &str, name: &str) -> R<String> {
    if ns.is_empty() {
        return Ok(name.to_string());
    }
    if name.starts_with('{') {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($name) must not use clark notation when argument #1 ($namespace) is not empty",
        );
    }
    if !name.starts_with(':') && name.contains(':') {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($name) must not be a QName when argument #1 ($namespace) is not empty",
        );
    }
    Ok(format!("{{{ns}}}{name}"))
}

fn store_param(o: &ObjRef, key: String, value: String) {
    with(o, |x| match x.params.iter_mut().find(|(k, _)| *k == key) {
        Some(p) => p.1 = value,
        None => x.params.push((key, value)),
    });
}

fn set_parameter(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let ns = s8(&a.str(0));
    let o = a.this_obj().clone();
    match a.val(1) {
        Value::Array(arr) => {
            // Tests setParameter_exceptions_test.phpt, setParameter_null_bytes.phpt.
            let mut items = Vec::new();
            for (k, v) in arr.iter() {
                let KeyRef::Str(name) = k else {
                    return vm.fthrow(
                        a,
                        "TypeError",
                        "Argument #2 ($name) must contain only string keys",
                    );
                };
                if name.as_bytes().contains(&0) {
                    return vm.fthrow(
                        a,
                        "ValueError",
                        "Argument #3 ($value) must not contain keys with any null bytes",
                    );
                }
                let s = vm.to_php_str(&v.deref())?;
                if s.as_bytes().contains(&0) {
                    return vm.fthrow(
                        a,
                        "ValueError",
                        "Argument #3 ($value) must not contain values with any null bytes",
                    );
                }
                items.push((s8(name), s8(&s)));
            }
            for (name, value) in items {
                let key = param_key(vm, a, &ns, &name)?;
                store_param(&o, key, value);
            }
        }
        name => {
            let name = vm.to_php_str(&name)?;
            let Some(value) = a.opt_str(2) else {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #3 ($value) cannot be null when argument #2 ($name) is a string",
                );
            };
            if name.as_bytes().contains(&0) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #2 ($name) must not contain any null bytes",
                );
            }
            if value.as_bytes().contains(&0) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #3 ($value) must not contain any null bytes",
                );
            }
            let key = param_key(vm, a, &ns, &s8(&name))?;
            store_param(&o, key, s8(&value));
        }
    }
    Ok(Value::Bool(true))
}

fn get_parameter(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key = param_key(vm, a, &s8(&a.str(0)), &s8(&a.str(1)))?;
    let v = with(a.this_obj(), |x| {
        x.params
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
    })
    .flatten();
    Ok(match v {
        Some(s) => Value::Str(PhpStr::from(s)),
        None => Value::Bool(false),
    })
}

fn remove_parameter(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key = param_key(vm, a, &s8(&a.str(0)), &s8(&a.str(1)))?;
    let found = with(a.this_obj(), |x| {
        let n = x.params.len();
        x.params.retain(|(k, _)| *k != key);
        x.params.len() != n
    })
    .unwrap_or(false);
    Ok(Value::Bool(found))
}

fn has_exslt_support(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(true))
}

fn register_php_functions(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // Test XSLTProcessor_callables_errors.phpt.
    let bad_names = "Argument #1 ($functions) must be an array containing valid callback names";
    let new = match a.val(0) {
        Value::Null => Allowed::All,
        Value::Str(s) => {
            if s.is_empty() {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #1 ($functions) must be a valid callback name",
                );
            }
            if vm.resolve_callable(&Value::Str(s.clone()), false).is_err() {
                let why = vm.callable_error(&Value::Str(s.clone()));
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!("Argument #1 ($functions) must be a callable, {why}"),
                );
            }
            Allowed::Some(vec![(s8(&s).to_ascii_lowercase(), Value::Str(s))])
        }
        Value::Array(arr) => {
            let mut v = Vec::new();
            for (k, val) in arr.iter() {
                let val = val.deref();
                let (name, callable) = match k {
                    KeyRef::Str(name) => {
                        if name.is_empty() || name.as_bytes().contains(&0) {
                            return vm.fthrow(a, "ValueError", bad_names);
                        }
                        (s8(name), val)
                    }
                    _ => {
                        let s = vm.to_php_str(&val)?;
                        if s.is_empty() || s.as_bytes().contains(&0) {
                            return vm.fthrow(a, "ValueError", bad_names);
                        }
                        (s8(&s), Value::Str(s))
                    }
                };
                if vm.resolve_callable(&callable, false).is_err() {
                    let why = vm.callable_error(&callable);
                    return vm.fthrow(
                        a,
                        "TypeError",
                        &format!(
                            "Argument #1 ($functions) must be an array with valid callbacks as values, {why}"
                        ),
                    );
                }
                v.push((name.to_ascii_lowercase(), callable));
            }
            Allowed::Some(v)
        }
        _ => Allowed::All,
    };
    with(a.this_obj(), |x| {
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
    let ns = s8(&a.str(0));
    let name = s8(&a.str(1));
    // Test registerPHPFunctionNS_errors.phpt.
    if ns == PHP_NS {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($namespaceURI) must not be \"http://php.net/xsl\" because it is reserved by PHP",
        );
    }
    if name.contains('\0') {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($name) must not contain any null bytes",
        );
    }
    if ns.contains('\0') {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($namespaceURI) must not contain any null bytes",
        );
    }
    if name.is_empty() || name.contains(':') || !goro_xml::chars::is_ncname(&name) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($name) must be a valid callback name",
        );
    }
    let cb = a.val(2);
    with(a.this_obj(), |x| {
        x.ns_functions.retain(|(n, l, _)| !(n == &ns && l == &name));
        x.ns_functions.push((ns, name, cb));
    });
    Ok(Value::Null)
}

fn set_profiling(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.opt_str(0).map(|s| s8(&s));
    with(a.this_obj(), |x| x.profile = f);
    Ok(Value::Bool(true))
}

fn set_security_prefs(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let new = a.int(0);
    let old = with(a.this_obj(), |x| std::mem::replace(&mut x.prefs, new)).unwrap_or(SEC_DEFAULT);
    Ok(Value::Int(old))
}

fn get_security_prefs(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(
        with(a.this_obj(), |x| x.prefs).unwrap_or(SEC_DEFAULT),
    ))
}
