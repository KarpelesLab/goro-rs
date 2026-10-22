//! php-src's ext/intl over the pure-Rust `intl` crate (KarpelesLab/intlrs,
//! CLDR 48 / Unicode 17 data, so the extension reports ICU 78.1), with
//! ICU4X for the few services `intl` lacks.
//!
//! The classes are natively implemented internal classes described by the
//! stubs (`goro_core::stub_sigs`): their methods and the procedural
//! functions share one implementation per php-src handler (an [`Imp`]),
//! reached through the `@alias` links of the stubs. Arguments are parsed
//! from the stub signatures ([`zpp`]); each module keeps its object state
//! as native data on the PHP object ([`obj`]).

#![allow(clippy::result_unit_err)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::stubs::StubFunc;
use goro_core::value::Value;
use goro_core::vm::{BuiltinFn, NativeClassHandler, Vm, VmError};

pub mod common;
pub mod consts;
pub mod datecore;
pub mod error;
pub mod obj;
pub mod zpp;
// misc area helpers (break iterator rules, UnicodeSet patterns)
pub mod breakiter_rules;
pub mod translit_data;
pub mod translit_rules;
pub mod uset;

pub use zpp::Obj;

/// The modules implementing the classes and functions: each defines
/// FUNCTIONS (with [`intl_functions!`]), METHODS (the methods that are not
/// an `@alias` of a function: "Class::method" => implementation) and
/// CLASSES (the classes it implements, with their create_object hook).
macro_rules! intl_modules {
    ($($m:ident),* $(,)?) => {
        $(pub mod $m;)*

        /// All the procedural functions.
        fn all_functions() -> Vec<&'static (&'static str, BuiltinFn, Imp)> {
            let mut v: Vec<&'static (&'static str, BuiltinFn, Imp)> = CORE_FUNCTIONS.iter().collect();
            $(v.extend($m::FUNCTIONS.iter());)*
            v
        }

        /// The methods not implemented as an alias of a function.
        fn all_methods() -> Vec<&'static (&'static str, Imp)> {
            let mut v: Vec<&'static (&'static str, Imp)> = Vec::new();
            $(v.extend($m::METHODS.iter());)*
            v
        }

        /// The intl classes with a native handler, with their create_object
        /// hooks.
        fn all_classes() -> Vec<&'static (&'static str, Option<obj::CreateFn>)> {
            let mut v: Vec<&'static (&'static str, Option<obj::CreateFn>)> = Vec::new();
            $(v.extend($m::CLASSES.iter());)*
            v
        }
    };
}

intl_modules!(
    normalizer,
    grapheme,
    idn,
    uchar,
    locale,
    collator,
    formatter,
    msgformat,
    dateformat,
    datepatterngen,
    calendar,
    timezone,
    breakiter,
    iterator,
    transliterator,
    converter,
    spoofchecker,
    resourcebundle,
    listformatter,
);

/// The extension name (`extension_loaded('intl')`).
pub const NAME: &[u8] = b"intl";

/// A call of an intl function or method.
pub struct Call {
    /// The active function or method ("collator_compare",
    /// "Collator::compare"), for messages.
    pub name: String,
    /// The object: `$this`, or the object argument of a procedural function.
    pub this: Option<Obj>,
    /// The parsed arguments (the object argument excluded).
    pub a: zpp::Args,
    /// The class a static method was called on (lowercase).
    pub called: Vec<u8>,
    /// Called as a method (not as a procedural function).
    pub is_method: bool,
    /// The first argument of a procedural function taking an object or a
    /// string (transliterator_transliterate()), when it was not an object.
    pub first: Option<Value>,
}

impl Call {
    pub fn arg(&self, i: usize) -> Value {
        self.a.get(i).deref()
    }
    /// The argument as given (references kept).
    pub fn raw(&self, i: usize) -> Value {
        self.a.get(i)
    }
    pub fn passed(&self, i: usize) -> bool {
        self.a.passed(i)
    }
    pub fn str(&self, i: usize) -> Vec<u8> {
        match self.arg(i) {
            Value::String(s) => s.as_bytes().to_vec(),
            Value::Undef | Value::Null => Vec::new(),
            v => v.to_php_string().as_bytes().to_vec(),
        }
    }
    /// A ?string argument: None for null / omitted.
    pub fn opt_str(&self, i: usize) -> Option<Vec<u8>> {
        match self.arg(i) {
            Value::Undef | Value::Null => None,
            Value::String(s) => Some(s.as_bytes().to_vec()),
            v => Some(v.to_php_string().as_bytes().to_vec()),
        }
    }
    pub fn long(&self, i: usize) -> i64 {
        match self.arg(i) {
            Value::Long(n) => n,
            Value::Undef | Value::Null => 0,
            v => v.to_long(),
        }
    }
    pub fn opt_long(&self, i: usize) -> Option<i64> {
        match self.arg(i) {
            Value::Undef | Value::Null => None,
            Value::Long(n) => Some(n),
            v => Some(v.to_long()),
        }
    }
    pub fn double(&self, i: usize) -> f64 {
        match self.arg(i) {
            Value::Double(d) => d,
            Value::Long(n) => n as f64,
            Value::Undef | Value::Null => 0.0,
            v => v.to_double(),
        }
    }
    pub fn bool(&self, i: usize) -> bool {
        self.arg(i).is_truthy()
    }
    /// Store into a by-reference argument (ZEND_TRY_ASSIGN_REF_*): false
    /// when that threw (a typed reference refusing the value).
    pub fn set_ref(&self, vm: &mut Vm, i: usize, v: Value) -> bool {
        if let Value::Reference(r) = self.a.get(i) {
            return vm.try_assign_ref(&r, v).is_ok();
        }
        true
    }
    /// The object, which the dispatcher guarantees for instance methods
    /// and object-taking functions.
    pub fn obj(&self) -> Obj {
        self.this
            .clone()
            .unwrap_or_else(|| Rc::new(RefCell::new(PhpObject::new(Vec::new(), 0))))
    }
    /// "func(): Argument #n ($name)" numbering: the procedural form counts
    /// the object argument.
    pub fn argn(&self, i: usize) -> usize {
        if self.is_method || self.this.is_none() && self.first.is_none() {
            i + 1
        } else {
            i + 2
        }
    }
}

/// One php-src handler (PHP_FUNCTION / PHP_METHOD).
pub type Imp = fn(&mut Vm, &mut Call) -> Value;

pub fn s(v: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(v.as_ref()))
}

pub fn sv(v: Vec<u8>) -> Value {
    Value::String(PhpString::from_vec(v))
}

pub fn bool_v(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

pub fn arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

/// Defines the procedural functions: `name => implementation` pairs become
/// a table of (name, builtin wrapper, implementation).
#[macro_export]
macro_rules! intl_functions {
    ($vis:vis $table:ident: $( $name:literal => $imp:path ),* $(,)?) => {
        $vis const $table: &[(&str, goro_core::vm::BuiltinFn, $crate::Imp)] = &[
            $( ($name, {
                fn w(
                    vm: &mut goro_core::vm::Vm,
                    args: &[goro_core::value::Value],
                ) -> Result<goro_core::value::Value, goro_core::vm::VmError> {
                    $crate::call_function(vm, $name, $imp, args)
                }
                w
            }, $imp) ),*
        ];
    };
}

intl_functions! { CORE_FUNCTIONS:
    "intl_get_error_code" => intl_get_error_code,
    "intl_get_error_message" => intl_get_error_message,
    "intl_is_failure" => intl_is_failure,
    "intl_error_name" => intl_error_name,
}

struct Tables {
    /// lowercase function name -> (implementation, takes the object first)
    functions: HashMap<String, (Imp, bool)>,
    /// lowercase "class::method" (declaring class) -> implementation
    methods: HashMap<String, Imp>,
}

thread_local! {
    static TABLES: RefCell<Option<Rc<Tables>>> = const { RefCell::new(None) };
}

fn tables() -> Rc<Tables> {
    TABLES.with(|t| {
        if let Some(t) = t.borrow().as_ref() {
            return t.clone();
        }
        let mut functions = HashMap::new();
        for (name, _, imp) in all_functions() {
            functions.insert(name.to_ascii_lowercase(), (*imp, false));
        }
        let mut methods = HashMap::new();
        for (name, imp) in all_methods() {
            methods.insert(name.to_ascii_lowercase(), *imp);
        }
        // Methods implemented by a function (@alias): the procedural form
        // of an instance method takes the object first.
        for (m, f) in goro_core::stub_sigs::ALIASES {
            let Some((cls, meth)) = m.split_once("::") else {
                continue;
            };
            let Some((_, ms)) = goro_core::stubs::find_method(cls.as_bytes(), meth.as_bytes())
            else {
                continue;
            };
            let key = m.to_ascii_lowercase();
            if methods.contains_key(&key) {
                continue;
            }
            if let Some((imp, _)) = functions.get(&f.to_ascii_lowercase()).copied() {
                methods.insert(key, imp);
                if !ms.is_static() {
                    functions.insert(f.to_ascii_lowercase(), (imp, true));
                }
            }
        }
        // Method aliases of methods
        for (m, f) in goro_core::stub_sigs::ALIASES {
            let key = m.to_ascii_lowercase();
            if !methods.contains_key(&key) && f.contains("::") {
                if let Some(imp) = methods.get(&f.to_ascii_lowercase()).copied() {
                    methods.insert(key, imp);
                }
            }
        }
        let rc = Rc::new(Tables { functions, methods });
        *t.borrow_mut() = Some(rc.clone());
        rc
    })
}

/// Run a procedural function.
pub fn call_function(vm: &mut Vm, name: &str, imp: Imp, args: &[Value]) -> Result<Value, VmError> {
    let skipped = std::mem::take(&mut vm.builtin_named_skipped);
    let Some(stub) = goro_core::stubs::find_function(name.as_bytes()) else {
        return Ok(Value::Null);
    };
    let obj_first = tables().functions.get(name).map(|f| f.1).unwrap_or(false);
    let owned: Vec<Value>;
    let args = if skipped.is_empty() {
        args
    } else {
        let mut v = args.to_vec();
        for i in skipped {
            if let Some(a) = v.get_mut(i) {
                *a = Value::Undef;
            }
        }
        owned = v;
        &owned[..]
    };
    let Some(mut a) = zpp::parse(vm, name, stub, args) else {
        return Ok(Value::Null);
    };
    let mut this = None;
    let mut first = None;
    if obj_first && !a.v.is_empty() {
        let v = a.v.remove(0).deref();
        a.given = a.given.saturating_sub(1);
        match v {
            Value::Object(o) => this = Some(o),
            v => first = Some(v),
        }
    }
    let mut call = Call {
        name: name.to_string(),
        this,
        a,
        called: Vec::new(),
        is_method: false,
        first,
    };
    Ok(imp(vm, &mut call))
}

/// The implementation and stub of method `method` of the intl class
/// `class_lower` (declared there or inherited).
fn resolve_method(
    class_lower: &[u8],
    method: &[u8],
) -> Option<(Imp, &'static StubFunc, &'static str)> {
    let (decl, m) = goro_core::stubs::find_method(class_lower, method)?;
    let key = format!("{}::{}", decl.name, m.name).to_ascii_lowercase();
    let imp = tables().methods.get(&key).copied()?;
    Some((imp, m, decl.name))
}

fn has_method(class: &[u8], method: &[u8], is_static: bool) -> bool {
    if method.eq_ignore_ascii_case(b"__construct") {
        return false;
    }
    match resolve_method(class, method) {
        Some((_, m, _)) => !is_static || m.is_static(),
        None => false,
    }
}

fn param_names(class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    let (_, m) = goro_core::stubs::find_method(class, method)?;
    thread_local! {
        static CACHE: RefCell<HashMap<(Vec<u8>, Vec<u8>), &'static [&'static str]>> =
            RefCell::new(HashMap::new());
    }
    let key = (class.to_vec(), method.to_ascii_lowercase());
    if let Some(v) = CACHE.with(|c| c.borrow().get(&key).copied()) {
        return Some(v);
    }
    let names: Vec<&'static str> = m.args.iter().map(|a| a.name).collect();
    let leaked: &'static [&'static str] = Box::leak(names.into_boxed_slice());
    CACHE.with(|c| c.borrow_mut().insert(key, leaked));
    Some(leaked)
}

fn this_obj(this: Option<&Value>) -> Option<Obj> {
    match this.map(|v| v.deref()) {
        Some(Value::Object(o)) => Some(o),
        _ => None,
    }
}

/// Run a method implementation with its frame.
fn run_method(
    vm: &mut Vm,
    imp: Imp,
    stub: &'static StubFunc,
    decl: &'static str,
    this: Option<Obj>,
    called: &[u8],
    args: &[Value],
) -> Value {
    let name = format!("{}::{}", decl, stub.name);
    let frame = if this.is_some() && !stub.is_static() {
        format!("{}->{}", decl, stub.name)
    } else {
        name.clone()
    };
    let line = vm.current_line;
    let file = vm.current_file.clone();
    vm.call_stack
        .push((frame, file, line, args.to_vec(), this.is_some()));
    let r = match zpp::parse(vm, &name, stub, args) {
        Some(a) => {
            let mut call = Call {
                name,
                this: if stub.is_static() { None } else { this },
                a,
                called: called.to_ascii_lowercase(),
                is_method: true,
                first: None,
            };
            imp(vm, &mut call)
        }
        None => Value::Null,
    };
    vm.call_stack.pop();
    r
}

fn call_method(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let Some((imp, stub, decl)) = resolve_method(class, method) else {
        return Value::Null;
    };
    let this_o = this_obj(this);
    if !stub.is_static() && this_o.is_none() {
        let msg = format!(
            "Non-static method {}::{}() cannot be called statically",
            decl, stub.name
        );
        zpp::throw(vm, b"Error", &msg);
        return Value::Null;
    }
    run_method(vm, imp, stub, decl, this_o, called, args)
}

/// new Class(...) of an intl class (or a subclass): its __construct.
fn construct(vm: &mut Vm, args: &[Value], _line: u32) -> Result<(), String> {
    let Some(Value::Object(this)) = args.first().map(|v| v.deref()) else {
        return Ok(());
    };
    let cls = this.borrow().class_name.to_ascii_lowercase();
    // The nearest intl ancestor declaring __construct
    let Some((decl, m)) = native_ctor(vm, &cls) else {
        return Ok(());
    };
    let key = format!("{}::__construct", decl).to_ascii_lowercase();
    let Some(imp) = tables().methods.get(&key).copied() else {
        return Ok(());
    };
    run_method(vm, imp, m, decl, Some(this), &cls, &args[1..]);
    if vm.current_exception.is_some() {
        return Err("exception".to_string());
    }
    Ok(())
}

fn native_ctor(vm: &Vm, class_lower: &[u8]) -> Option<(&'static str, &'static StubFunc)> {
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        if let Some(c) = goro_core::stubs::find_class(&cur) {
            if c.ext == "intl" {
                return goro_core::stubs::find_method(&cur, b"__construct")
                    .map(|(d, m)| (d.name, m));
            }
        }
        cur = vm.classes.get(&cur)?.parent.as_ref()?.to_ascii_lowercase();
    }
    None
}

// ---------------------------------------------------------------------------
// INI

/// OnUpdateErrorLevel
fn on_update_error_level(vm: &mut Vm, value: &[u8], startup: bool) -> bool {
    let (v, err) = goro_core::ini_quantity::parse_quantity(value, true);
    if let Some(e) = err {
        vm.emit_warning(&format!("Invalid \"intl.error_level\" setting. {}", e));
    }
    if v as i64 != 0 {
        // php_error_docref(): prefixed with the active function (ini_set())
        vm.emit_deprecated(&format!(
            "{}Using a value different than 0 for intl.error_level is deprecated, as the intl.error_level INI setting is deprecated. Instead the intl.use_exceptions INI setting should be enabled to throw exceptions on errors or intl_get_error_code()/intl_get_error_message() should be used to manually deal with errors",
            if startup { "" } else { "ini_set(): " }
        ));
    }
    true
}

/// OnUpdateStringUnempty
fn on_update_default_locale(_vm: &mut Vm, value: &[u8], _startup: bool) -> bool {
    !value.is_empty()
}

/// PHP_MINFO(intl)
fn minfo(_vm: &mut Vm) -> String {
    let ver = |n: &str| match consts::GLOBAL.iter().find(|(k, _)| *k == n) {
        Some((_, consts::V::S(s))) => s.to_string(),
        _ => String::new(),
    };
    format!(
        "\nintl\n\nInternationalization support => enabled\nICU version => {}\nICU Data version => {}\nICU TZData version => 2025b\nICU Unicode version => 17.0\n\nDirective => Local Value => Master Value\nintl.default_locale => no value => no value\nintl.error_level => 0 => 0\nintl.use_exceptions => Off => Off\n",
        ver("INTL_ICU_VERSION"),
        ver("INTL_ICU_DATA_VERSION")
    )
}

fn const_value(v: consts::V) -> Value {
    match v {
        consts::V::I(n) => Value::Long(n),
        consts::V::D(d) => Value::Double(d),
        consts::V::S(s) => self::s(s),
        consts::V::N => Value::Null,
    }
}

/// The value of an intl class constant.
pub fn class_const(class: &str, name: &str) -> i64 {
    consts::CLASSES
        .iter()
        .find(|(c, _)| *c == class)
        .and_then(|(_, l)| l.iter().find(|(k, _)| *k == name))
        .map(|(_, v)| match v {
            consts::V::I(n) => *n,
            _ => 0,
        })
        .unwrap_or(0)
}

fn rshutdown(_vm: &mut Vm) {
    error::reset_global();
}

/// PHP_MINIT(intl)
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    // INI
    vm.constants.insert(b"intl.default_locale".to_vec(), s(b""));
    vm.constants.insert(b"intl.error_level".to_vec(), s(b"0"));
    vm.constants
        .insert(b"intl.use_exceptions".to_vec(), s(b"0"));
    vm.ini_validators
        .insert(b"intl.error_level".to_vec(), on_update_error_level);
    vm.ini_validators
        .insert(b"intl.default_locale".to_vec(), on_update_default_locale);
    vm.phpinfo_hooks.push(minfo);
    vm.request_shutdown_hooks.push(rshutdown);
    // Constants
    let mut names = Vec::new();
    for (k, v) in consts::GLOBAL {
        vm.constants.insert(k.as_bytes().to_vec(), const_value(*v));
        names.push(k.as_bytes().to_vec());
    }
    vm.module_constants.push((NAME.to_vec(), names));
    for (c, list) in consts::CLASSES {
        let l: Vec<(Vec<u8>, Value, Option<String>)> = list
            .iter()
            .map(|(k, v)| (k.as_bytes().to_vec(), const_value(*v), None))
            .collect();
        vm.ext_class_constants
            .insert(c.to_ascii_lowercase().into_bytes(), l);
    }
    // Functions
    for (name, w, _) in all_functions() {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), *w, &params);
    }
    // Classes
    let h = NativeClassHandler {
        has_method,
        call: call_method,
        prop_added: None,
        prop_write: None,
        compare_check: None,
        prop_read: None,
        param_names: Some(param_names),
    };
    for &(c, create) in all_classes() {
        let lower = c.to_ascii_lowercase().into_bytes();
        vm.native_class_handlers.insert(lower.clone(), h);
        if let Some(f) = create {
            vm.object_create_handlers.insert(lower.clone(), f);
        }
        if goro_core::stubs::find_class(&lower)
            .is_some_and(|sc| sc.methods.iter().any(|m| m.name == "__construct"))
        {
            vm.object_construct_handlers.insert(lower, construct);
        }
    }
}

// ---------------------------------------------------------------------------
// php_intl.c

/// intl_get_error_code()
fn intl_get_error_code(_vm: &mut Vm, _c: &mut Call) -> Value {
    Value::Long(error::global_code() as i64)
}

/// intl_get_error_message()
fn intl_get_error_message(_vm: &mut Vm, _c: &mut Call) -> Value {
    s(error::global_message())
}

/// intl_is_failure()
fn intl_is_failure(_vm: &mut Vm, c: &mut Call) -> Value {
    bool_v(c.long(0) > 0)
}

/// intl_error_name()
fn intl_error_name(_vm: &mut Vm, c: &mut Call) -> Value {
    s(error::error_name(c.long(0)))
}
