//! Arginfo of internal functions and methods, from php-src's `*.stub.php`
//! (tools/gen_stub_sigs.php -> stub_sigs.rs), and the generic
//! zend_parse_parameters-style layer built on it: argument count errors,
//! weak/strict scalar parameter checks and coercion, null-to-non-nullable
//! deprecations, and named arguments (incl. default values of skipped
//! parameters).
//!
//! The checks are enabled per function / method only where
//! tools/verify_arginfo.py confirmed against a real PHP and php-src's C
//! argument parsing that the stub reproduces its zpp diagnostics exactly
//! (arginfo_verified.rs). Methods are checked where the VM dispatches
//! internal methods: deferred SPL calls, argument-taking native methods
//! (`__spl::`), methods run when the call is initialized (argument count
//! only, `__builtin_return:`), internal constructors, static methods and
//! the Generator/Fiber/Closure/WeakMap pseudo-calls.

use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

#[path = "arginfo_verified.rs"]
mod verified;

// Type mask bits (keep in sync with tools/verify_arginfo.py)
pub const T_NULL: u16 = 1;
pub const T_FALSE: u16 = 2;
pub const T_TRUE: u16 = 4;
pub const T_BOOL: u16 = T_FALSE | T_TRUE;
pub const T_INT: u16 = 8;
pub const T_FLOAT: u16 = 16;
pub const T_STRING: u16 = 32;
pub const T_ARRAY: u16 = 64;
pub const T_OBJECT: u16 = 128;
pub const T_CALLABLE: u16 = 256;
pub const T_MIXED: u16 = 1024;
pub const T_VOID: u16 = 2048;
pub const T_NEVER: u16 = 4096;
pub const T_STATIC: u16 = 8192;
pub const T_CLASS: u16 = 16384;

/// Verified flag: argument count errors match php-src.
pub const CHECK_ARITY: u8 = 1;
/// Verified flag: parameter type errors / coercions match php-src.
pub const CHECK_TYPES: u8 = 2;

/// One parameter of an internal function, derived from its stub
/// (`crate::stubs::StubArg`) in the form the zpp layer needs.
#[derive(Debug)]
pub struct ArgInfo {
    pub name: &'static str,
    /// zend_type_to_string() of the declared type ("" when untyped)
    pub ty: String,
    pub mask: u16,
    /// Default value as written in the stub (None: required, or `= UNKNOWN`)
    pub default: Option<&'static str>,
    /// 0 = by value, 1 = by reference, 2 = prefer-ref
    pub by_ref: u8,
    pub variadic: bool,
    pub sensitive: bool,
}

#[derive(Debug)]
pub struct FuncInfo {
    /// Declared name
    pub name: &'static str,
    /// Module ("extension") name
    pub module: &'static str,
    pub params: Vec<ArgInfo>,
    pub required: u8,
    pub stub: &'static crate::stubs::StubFunc,
}

impl FuncInfo {
    fn from_stub(f: &'static crate::stubs::StubFunc) -> FuncInfo {
        use crate::stubs::{A_BY_REF, A_PREFER_REF, A_SENSITIVE, A_VARIADIC};
        let params = f
            .args
            .iter()
            .map(|a| ArgInfo {
                name: a.name,
                ty: type_display(a.ty),
                mask: type_mask(a.ty),
                default: a.default,
                by_ref: if a.flags & A_PREFER_REF != 0 {
                    2
                } else if a.flags & A_BY_REF != 0 {
                    1
                } else {
                    0
                },
                variadic: a.flags & A_VARIADIC != 0,
                sensitive: a.flags & A_SENSITIVE != 0,
            })
            .collect();
        FuncInfo {
            name: f.name,
            module: f.ext,
            params,
            required: f.required_args() as u8,
            stub: f,
        }
    }

    pub fn is_variadic(&self) -> bool {
        self.params.last().is_some_and(|p| p.variadic)
    }

    /// Number of declared parameters, excluding a variadic one (num_args).
    pub fn num_args(&self) -> usize {
        self.params.len() - self.is_variadic() as usize
    }

    /// The arginfo describing argument `i` (the variadic one for extra args).
    pub fn param(&self, i: usize) -> Option<&ArgInfo> {
        match self.params.get(i) {
            Some(p) if !p.variadic => Some(p),
            _ => self.params.last().filter(|p| p.variadic),
        }
    }

    /// Argument name as shown in errors: variadic arguments have none
    /// (get_function_arg_name()).
    pub fn arg_name(&self, i: usize) -> Option<&'static str> {
        if i < self.num_args() {
            Some(self.params[i].name)
        } else {
            None
        }
    }

    /// zend_wrong_parameters_count_error() for `given` arguments, if out of range.
    pub fn arity_error(&self, given: usize) -> Option<String> {
        let min = self.required as usize;
        let max = if self.is_variadic() {
            None
        } else {
            Some(self.num_args())
        };
        if given >= min && max.is_none_or(|m| given <= m) {
            return None;
        }
        let (kind, n) = if max == Some(min) {
            ("exactly", min)
        } else if given < min {
            ("at least", min)
        } else {
            ("at most", max.unwrap_or(min))
        };
        Some(format!(
            "{}() expects {} {} argument{}, {} given",
            self.name,
            kind,
            n,
            if n == 1 { "" } else { "s" },
            given
        ))
    }

    /// "f(): Argument #N ($name)" prefix of zend_argument_error().
    pub fn arg_prefix(&self, i: usize) -> String {
        match self.arg_name(i) {
            Some(n) => format!("{}(): Argument #{} (${})", self.name, i + 1, n),
            None => format!("{}(): Argument #{}", self.name, i + 1),
        }
    }
}

/// Type mask of a stub type (`iterable` is Traversable|array, as zend_compile makes it).
pub fn type_mask(t: &str) -> u16 {
    let mut t = t.trim();
    if t.is_empty() {
        return 0;
    }
    let mut mask = 0;
    if let Some(r) = t.strip_prefix('?') {
        mask |= T_NULL;
        t = r;
    }
    for part in t.split('|') {
        let part = part.trim().trim_matches(|c| c == '(' || c == ')');
        mask |= match part.to_ascii_lowercase().as_str() {
            _ if part.contains('&') => T_CLASS,
            "null" => T_NULL,
            "false" => T_FALSE,
            "true" => T_TRUE,
            "bool" => T_BOOL,
            "int" => T_INT,
            "float" => T_FLOAT,
            "string" => T_STRING,
            "array" => T_ARRAY,
            "object" => T_OBJECT,
            "callable" => T_CALLABLE,
            "mixed" => T_MIXED | T_NULL,
            "void" => T_VOID,
            "never" => T_NEVER,
            "static" | "self" => T_STATIC,
            "iterable" => T_ARRAY | T_CLASS,
            _ => T_CLASS,
        };
    }
    mask
}

/// zend_type_to_string() of a stub type.
pub fn type_display(t: &str) -> String {
    let orig = t.trim();
    let mut t = orig;
    if t.is_empty() {
        return String::new();
    }
    if let Some(r) = t.strip_prefix('?') {
        t = r;
    }
    let mut parts: Vec<String> = Vec::new();
    for part in t.split('|') {
        let part = part.trim().trim_matches(|c| c == '(' || c == ')');
        let lp = part.to_ascii_lowercase();
        if part.contains('&') {
            parts.push(part.to_string());
        } else if lp == "iterable" {
            parts.push("Traversable".to_string());
        } else if !matches!(
            lp.as_str(),
            "null"
                | "false"
                | "true"
                | "bool"
                | "int"
                | "float"
                | "string"
                | "array"
                | "object"
                | "callable"
                | "mixed"
                | "void"
                | "never"
                | "static"
                | "self"
        ) {
            parts.push(part.trim_start_matches('\\').to_string());
        }
    }
    let mask = type_mask(orig);
    if mask & T_MIXED != 0 {
        parts.push("mixed".to_string());
        return parts.join("|");
    }
    for (bit, name) in [
        (T_STATIC, "static"),
        (T_CALLABLE, "callable"),
        (T_OBJECT, "object"),
        (T_ARRAY, "array"),
        (T_STRING, "string"),
        (T_INT, "int"),
        (T_FLOAT, "float"),
    ] {
        if mask & bit != 0 {
            parts.push(name.to_string());
        }
    }
    if mask & T_BOOL == T_BOOL {
        parts.push("bool".to_string());
    } else if mask & T_FALSE != 0 {
        parts.push("false".to_string());
    } else if mask & T_TRUE != 0 {
        parts.push("true".to_string());
    }
    if mask & T_VOID != 0 {
        parts.push("void".to_string());
    }
    if mask & T_NEVER != 0 {
        parts.push("never".to_string());
    }
    if mask & T_NULL != 0 {
        if parts.len() == 1 && !parts[0].contains('&') {
            return format!("?{}", parts[0]);
        }
        parts.push("null".to_string());
    }
    parts.join("|")
}

/// Arginfo of the internal function `lname` (lowercase), from its stub.
pub fn lookup(lname: &[u8]) -> Option<&'static FuncInfo> {
    static MAP: std::sync::OnceLock<crate::fxhash::FxHashMap<&'static str, FuncInfo>> =
        std::sync::OnceLock::new();
    let map = MAP.get_or_init(|| {
        crate::stub_sigs::FUNCTIONS
            .iter()
            .map(|f| (f.name, FuncInfo::from_stub(f)))
            .collect()
    });
    let s = std::str::from_utf8(lname).ok()?;
    match map.get(s) {
        Some(f) => Some(f),
        None => {
            let f = crate::stubs::find_function(lname)?;
            map.get(f.name)
        }
    }
}

/// CHECK_* flags verified for `lname`.
pub fn verified_flags(lname: &[u8]) -> u8 {
    // implode() calls are frameless (ZEND_FRAMELESS_FUNCTION(implode, n)),
    // whose argument parsing differs from the stub's: implode() checks its
    // argument types itself.
    if lname == b"implode" {
        return CHECK_ARITY;
    }
    let t = verified::VERIFIED;
    t.binary_search_by(|(n, _)| n.as_bytes().cmp(lname))
        .map(|i| t[i].1)
        .unwrap_or(0)
}

/// Arginfo and verified flags of the internal method `key` (lowercase
/// "declaring class::method"), if it is in the verified list.
fn verified_method(key: &str) -> Option<(&'static FuncInfo, u8)> {
    static MAP: std::sync::OnceLock<std::collections::HashMap<&'static str, (FuncInfo, u8)>> =
        std::sync::OnceLock::new();
    let map = MAP.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        for &(k, flags) in verified::VERIFIED_METHODS {
            let Some((cls, m)) = k.split_once("::") else {
                continue;
            };
            let Some(c) = crate::stubs::find_class(cls.as_bytes()) else {
                continue;
            };
            let Some(f) = c.methods.iter().find(|f| f.name.eq_ignore_ascii_case(m)) else {
                continue;
            };
            let mut info = FuncInfo::from_stub(f);
            info.name = Box::leak(format!("{}::{}", c.name, f.name).into_boxed_str());
            info.module = c.ext;
            map.insert(k, (info, flags));
        }
        map
    });
    map.get(key).map(|(i, f)| (i, *f))
}

/// Parameters declared `float` in their stub that the C function parses
/// with Z_PARAM_NUMBER: ints are kept, numeric strings become int or float
/// and type errors name int|float (the null deprecation still names the
/// arginfo type).
fn zpp_number_param(info: &FuncInfo, i: usize) -> bool {
    i == 0 && info.name == "number_format"
}

/// Why a call to an internal function failed before it ran.
pub enum ZppError {
    /// Throw a new `class` exception with this message.
    Throw(&'static [u8], String),
    /// An exception is already pending (a user error handler threw).
    Pending,
}

/// The T_* bit of a (dereferenced) value's type.
fn value_type_bit(v: &Value) -> u16 {
    match v {
        Value::Undef | Value::Null => T_NULL,
        Value::False => T_FALSE,
        Value::True => T_TRUE,
        Value::Long(_) => T_INT,
        Value::Double(_) => T_FLOAT,
        Value::String(_) => T_STRING,
        Value::Array(_) => T_ARRAY,
        Value::Resource(_) => 0,
        _ => T_OBJECT,
    }
}

/// zend_zval_value_name()
fn value_name(v: &Value) -> String {
    Vm::value_type_name(v)
}

fn arg_type_display(p: &ArgInfo) -> &str {
    &p.ty
}

/// is_numeric_string() without errors allowed (whitespace around is fine).
fn numeric(s: &PhpString) -> Option<Value> {
    match crate::value::parse_numeric_str(s.as_bytes(), false) {
        Some((crate::value::NumStr::Long(n), false)) => Some(Value::Long(n)),
        Some((crate::value::NumStr::Double(d), false)) => Some(Value::Double(d)),
        _ => None,
    }
}

impl Vm {
    /// "Uncaught Class: message" for the pending exception (VmError text).
    pub fn uncaught_exception_message(&self) -> String {
        match &self.current_exception {
            Some(Value::Object(o)) => {
                let ob = o.borrow();
                let class = crate::value::display_class_name(&ob.class_name);
                format!(
                    "Uncaught {}: {}",
                    class,
                    ob.get_property(b"message")
                        .to_php_string()
                        .to_string_lossy()
                )
            }
            _ => "Uncaught exception".to_string(),
        }
    }

    /// The generic zpp checks (argument count and types, from the stub) of
    /// builtin `lname`, run by the builtin itself: a deprecated function
    /// raises its deprecation first. The coerced arguments, or Err with the
    /// exception set.
    pub fn zpp_self_check(
        &mut self,
        lname: &[u8],
        args: &[Value],
    ) -> Result<Vec<Value>, crate::vm::VmError> {
        self.zpp_self_check_flags(lname, args, CHECK_ARITY | CHECK_TYPES)
    }

    /// zpp_self_check() with only the `flags` (CHECK_*) checks.
    pub fn zpp_self_check_flags(
        &mut self,
        lname: &[u8],
        args: &[Value],
        flags: u8,
    ) -> Result<Vec<Value>, crate::vm::VmError> {
        let line = self.current_line;
        let mut owned = args.to_vec();
        // (the error handler threw on the deprecation: the function is not run)
        if self.current_exception.is_some() {
            return Err(crate::vm::VmError {
                message: self.uncaught_exception_message(),
                line,
            });
        }
        let Some(info) = lookup(lname) else {
            return Ok(owned);
        };
        let mut orig = None;
        let strict = self.caller_strict_types;
        if let Err(e) = self.zpp_check(info, flags, &mut owned, &mut orig, strict, line) {
            if let ZppError::Throw(cls, msg) = e {
                let exc = self.create_exception(cls, &msg, line);
                self.current_exception = Some(exc);
            }
            return Err(crate::vm::VmError {
                message: self.uncaught_exception_message(),
                line,
            });
        }
        Ok(owned)
    }

    /// Arginfo and verified flags of a registered builtin, if the generic
    /// zpp layer is enabled for it. Cheap: one hash lookup.
    #[inline]
    pub(crate) fn zpp_info(&mut self, lname: &[u8]) -> Option<(&'static FuncInfo, u8)> {
        if self.zpp_map_fn_count != self.functions.len() {
            self.zpp_rebuild_map();
        }
        self.zpp_map.get(lname).copied()
    }

    /// Arginfo and verified flags of internal method `method_lower` called on
    /// internal class `class_lower` (declared there or inherited), if the
    /// generic zpp layer is enabled for it. Cached per class and method.
    pub(crate) fn zpp_method_info(
        &mut self,
        class_lower: &[u8],
        method_lower: &[u8],
    ) -> Option<(&'static FuncInfo, u8)> {
        if let Some(r) = self
            .zpp_method_map
            .get(class_lower)
            .and_then(|m| m.get(method_lower))
        {
            return *r;
        }
        let r = crate::stubs::find_method(class_lower, method_lower).and_then(|(c, m)| {
            verified_method(&format!("{}::{}", c.name, m.name).to_ascii_lowercase())
        });
        self.zpp_method_map
            .entry(class_lower.to_vec())
            .or_default()
            .insert(method_lower.to_vec(), r);
        r
    }

    /// The generic zpp checks for a call to internal method `class_lower::
    /// method_lower` with `args[skip..]` (`args[..skip]`: $this), if they are
    /// enabled for it: named arguments are resolved first (taken from `named`)
    /// when given. On failure the exception is created and left in
    /// current_exception: Err(()). `frame`: whether to give the method a frame
    /// while it is created (Some(is_instance); None when the caller pushed
    /// one). Ok(true) when the layer checked the arguments.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn zpp_method_call(
        &mut self,
        class_lower: &[u8],
        method_lower: &[u8],
        args: &mut Vec<Value>,
        skip: usize,
        named: &mut Vec<(Vec<u8>, Value)>,
        frame: Option<bool>,
        strict: bool,
        line: u32,
    ) -> Result<bool, ()> {
        let Some((info, flags)) = self.zpp_method_info(class_lower, method_lower) else {
            return Ok(false);
        };
        let skip = skip.min(args.len());
        let mut err = None;
        if !named.is_empty() {
            let n = std::mem::take(named);
            let mut margs = args.split_off(skip);
            if let Err(e) = self.zpp_resolve_named(info, &mut margs, n) {
                err = Some((e, false));
            }
            args.append(&mut margs);
        }
        // Arguments as passed, when zpp coerced some (for the frame)
        let mut orig = None;
        if err.is_none() {
            if let Err(e) = self.zpp_check(info, flags, &mut args[skip..], &mut orig, strict, line)
            {
                err = Some((e, true));
            }
        }
        let Some((e, in_call)) = err else {
            return Ok(true);
        };
        if let ZppError::Throw(cls, msg) = e {
            let push = in_call && frame.is_some();
            if push {
                // ("[internal function]" when internal code calls it)
                let (file, fline) = if self.caller_is_internal() {
                    (crate::rcstr::RcStr::new(), 0)
                } else {
                    (self.current_file.clone(), line)
                };
                let is_instance = frame == Some(true);
                let shown = orig.unwrap_or_else(|| args[skip..].to_vec());
                self.call_stack
                    .push((info.name.to_string(), file, fline, shown, is_instance));
            }
            let exc = self.create_exception(cls, &msg, line);
            if push {
                self.call_stack.pop();
            }
            self.current_exception = Some(exc);
        }
        Err(())
    }

    /// zpp_method_call() on call arguments led by $this, for a method whose
    /// frame the caller has pushed. True when the call failed (exception set).
    pub(crate) fn zpp_spl_args(
        &mut self,
        class_lower: &[u8],
        method_lower: &[u8],
        args: &mut Vec<Value>,
        named: &mut Vec<(Vec<u8>, Value)>,
        strict: bool,
        line: u32,
    ) -> bool {
        self.zpp_method_call(
            class_lower,
            method_lower,
            args,
            1,
            named,
            None,
            strict,
            line,
        )
        .is_err()
    }

    /// The internal class `class_lower` is, or the nearest internal ancestor
    /// of the user class `class_lower` (lowercase).
    pub(crate) fn zpp_internal_class(&mut self, class_lower: &[u8]) -> Option<Vec<u8>> {
        // Internal classes and user classes can't change once declared, but a
        // user class may be declared later under a name looked up before.
        if let Some((n, r)) = self.zpp_internal_class_cache.get(class_lower) {
            if *n == self.classes.len() {
                return r.clone();
            }
        }
        let r = self.zpp_internal_class_uncached(class_lower);
        let n = self.classes.len();
        self.zpp_internal_class_cache
            .insert(class_lower.to_vec(), (n, r.clone()));
        r
    }

    fn zpp_internal_class_uncached(&self, class_lower: &[u8]) -> Option<Vec<u8>> {
        let mut cur = class_lower.to_vec();
        for _ in 0..64 {
            match self.classes.get(&cur) {
                // A user class (internal ones registered here have no file)
                Some(c) if c.filename.is_some() => cur = c.parent.as_ref()?.to_ascii_lowercase(),
                _ => return crate::stubs::find_class(&cur).map(|_| cur),
            }
        }
        None
    }

    /// Name of the pending call returning the result of internal method
    /// `method_lower`, run on an object of `class_lower` when the call was
    /// initialized (it takes no arguments): "__builtin_return", with
    /// ":<class>::<method>" appended when the method's argument count is
    /// verified, for zpp_builtin_return_check() once the arguments are known.
    pub(crate) fn zpp_builtin_return_name(
        &mut self,
        class_lower: &[u8],
        method_lower: &[u8],
    ) -> PhpString {
        if let Some(r) = self
            .zpp_return_names
            .get(class_lower)
            .and_then(|m| m.get(method_lower))
        {
            return r.clone();
        }
        let r = self.zpp_builtin_return_name_uncached(class_lower, method_lower);
        self.zpp_return_names
            .entry(class_lower.to_vec())
            .or_default()
            .insert(method_lower.to_vec(), r.clone());
        r
    }

    fn zpp_builtin_return_name_uncached(
        &mut self,
        class_lower: &[u8],
        method_lower: &[u8],
    ) -> PhpString {
        if let Some(c) = self.zpp_internal_class(class_lower) {
            if let Some((info, f)) = self.zpp_method_info(&c, method_lower) {
                if f & CHECK_ARITY == 0 {
                    return PhpString::from_bytes(b"__builtin_return");
                }
                // "__builtin_return:<min>:<max>:class::method", so that calls
                // in range need no lookup
                let max = if info.is_variadic() {
                    "v".to_string()
                } else {
                    info.num_args().to_string()
                };
                let mut n = format!("__builtin_return:{}:{}:", info.required, max).into_bytes();
                n.extend_from_slice(&c);
                n.extend_from_slice(b"::");
                n.extend_from_slice(method_lower);
                return PhpString::from_vec(n);
            }
        }
        PhpString::from_bytes(b"__builtin_return")
    }

    /// Argument checks of a call named by zpp_builtin_return_name(): `args`
    /// are the result followed by the arguments passed. On error the
    /// exception is left in current_exception.
    pub(crate) fn zpp_builtin_return_check(
        &mut self,
        name_lower: &[u8],
        args: &[Value],
        named: &[(Vec<u8>, Value)],
        line: u32,
    ) {
        let Some(rest) = name_lower.strip_prefix(b"__builtin_return:") else {
            return;
        };
        if self.current_exception.is_some() {
            return;
        }
        let mut parts = rest.splitn(3, |&c| c == b':');
        let (Some(min), Some(max), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
            return;
        };
        let given = args.len().saturating_sub(1);
        let num = |b: &[u8]| {
            std::str::from_utf8(b)
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
        };
        if named.is_empty()
            && num(min).is_some_and(|m| given >= m)
            && (max == b"v" || num(max).is_some_and(|m| given <= m))
        {
            return;
        }
        let Some(sep) = rest.windows(2).position(|w| w == b"::") else {
            return;
        };
        let Some((info, _)) = self.zpp_method_info(&rest[..sep], &rest[sep + 2..]) else {
            return;
        };
        let (cls, msg): (&[u8], String) = if let Some((n, _)) = named.first() {
            (
                b"Error",
                format!("Unknown named parameter ${}", String::from_utf8_lossy(n)),
            )
        } else {
            match info.arity_error(args.len().saturating_sub(1)) {
                Some(m) => (b"ArgumentCountError", m),
                None => return,
            }
        };
        let push = named.is_empty();
        if push {
            let file = self.current_file.clone();
            let shown = args.get(1..).unwrap_or(&[]).to_vec();
            self.call_stack
                .push((info.name.to_string(), file, line, shown, true));
        }
        let exc = self.create_exception(cls, &msg, line);
        if push {
            self.call_stack.pop();
        }
        self.current_exception = Some(exc);
    }

    /// Generic zpp for a call to the internal constructor an object inherits
    /// (`args[0]` is the object). True when the call failed (exception set).
    pub(crate) fn zpp_internal_ctor(
        &mut self,
        args: &mut Vec<Value>,
        named: &mut Vec<(Vec<u8>, Value)>,
        strict: bool,
        line: u32,
    ) -> bool {
        let class = match args.first() {
            Some(Value::Object(o)) => o.borrow().class_name.to_ascii_lowercase(),
            _ => return false,
        };
        let Some(internal) = self.zpp_internal_class(&class) else {
            return false;
        };
        if self.zpp_method_info(&internal, b"__construct").is_none() {
            return false;
        }
        self.zpp_method_call(
            &internal,
            b"__construct",
            args,
            1,
            named,
            Some(true),
            strict,
            line,
        )
        .is_err()
    }

    /// Generic zpp for a call `name_lower` ("class::method") to a static
    /// method of an internal class. True when the call failed (exception set).
    pub(crate) fn zpp_internal_static(
        &mut self,
        name_lower: &[u8],
        args: &mut Vec<Value>,
        named: &mut Vec<(Vec<u8>, Value)>,
        strict: bool,
        line: u32,
    ) -> bool {
        // Instance methods dispatched through internal pseudo-calls, led by $this
        let this_call: Option<(&[u8], &[u8])> = match name_lower {
            b"__closure_call" => Some((b"closure", b"call")),
            b"__closure_bindto" => Some((b"closure", b"bindto")),
            _ => {
                if let Some(m) = name_lower.strip_prefix(b"__fiber::") {
                    Some((b"fiber", m))
                } else if let Some(m) = name_lower.strip_prefix(b"__genm::") {
                    Some((b"generator", m))
                } else if let Some(m) = name_lower.strip_prefix(b"__weakmap::") {
                    Some((b"weakmap", m))
                } else {
                    None
                }
            }
        };
        if let Some((class, method)) = this_call {
            if name_lower == b"__closure_call" {
                // Closure::call(object $newThis, mixed ...$args): extra named
                // arguments are the closure's (Z_PARAM_VARIADIC_WITH_NAMED).
                let (mine, extra): (Vec<_>, Vec<_>) = std::mem::take(named)
                    .into_iter()
                    .partition(|(n, _)| n == b"newThis");
                *named = mine;
                let failed = self
                    .zpp_method_call(class, method, args, 1, named, Some(true), strict, line)
                    .is_err();
                *named = extra;
                return failed;
            }
            return self
                .zpp_method_call(class, method, args, 1, named, Some(true), strict, line)
                .is_err();
        }
        let Some(sep) = name_lower.windows(2).position(|w| w == b"::") else {
            return false;
        };
        let (class, method) = (&name_lower[..sep], &name_lower[sep + 2..]);
        // User classes (declared ones have a file)
        if self
            .classes
            .get(class)
            .is_some_and(|c| c.filename.is_some())
        {
            return false;
        }
        match self.zpp_method_info(class, method) {
            Some((info, _)) if info.stub.is_static() => {}
            _ => return false,
        }
        self.zpp_method_call(class, method, args, 0, named, Some(false), strict, line)
            .is_err()
    }

    fn zpp_rebuild_map(&mut self) {
        self.zpp_map.clear();
        for name in self.functions.keys() {
            let flags =
                verified_flags(name) | self.zpp_extension_verified.get(name).copied().unwrap_or(0);
            if flags != 0 {
                // (a deprecated function checks its arguments itself, after
                // its deprecation: zpp_self_check())
                if let Some(info) =
                    lookup(name).filter(|i| i.stub.flags & crate::stubs::F_DEPRECATED == 0)
                {
                    self.zpp_map.insert(name.clone(), (info, flags));
                }
            }
        }
        self.zpp_map_fn_count = self.functions.len();
    }

    /// Evaluate a stub default value (zend_get_default_from_internal_arg_info).
    /// None for UNKNOWN or an expression this evaluator does not handle.
    pub fn arginfo_default_value(&mut self, src: &str) -> Option<Value> {
        let src = src.trim();
        if src == "UNKNOWN" {
            return None;
        }
        // Bitwise-or of constants (e.g. ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401)
        if src.contains('|') && !src.starts_with('"') && !src.starts_with('\'') {
            let mut acc = 0i64;
            for part in src.split('|') {
                acc |= self.arginfo_default_value(part)?.to_long();
            }
            return Some(Value::Long(acc));
        }
        match src {
            "null" | "NULL" => return Some(Value::Null),
            "true" => return Some(Value::True),
            "false" => return Some(Value::False),
            "[]" => {
                return Some(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
                    crate::array::PhpArray::new(),
                ))));
            }
            _ => {}
        }
        let b = src.as_bytes();
        if b[0] == b'"' || b[0] == b'\'' {
            return Some(Value::String(PhpString::from_vec(unquote(src)?)));
        }
        if b[0].is_ascii_digit() || b[0] == b'-' || b[0] == b'+' {
            let (neg, digits) = match b[0] {
                b'-' => (true, &src[1..]),
                b'+' => (false, &src[1..]),
                _ => (false, src),
            };
            let v = if let Some(o) = digits
                .strip_prefix("0o")
                .or_else(|| digits.strip_prefix("0O"))
            {
                Value::Long(i64::from_str_radix(o, 8).ok()?)
            } else if let Some(h) = digits
                .strip_prefix("0x")
                .or_else(|| digits.strip_prefix("0X"))
            {
                Value::Long(i64::from_str_radix(h, 16).ok()?)
            } else if digits.len() > 1
                && digits.starts_with('0')
                && digits.bytes().all(|c| c.is_ascii_digit())
            {
                Value::Long(i64::from_str_radix(&digits[1..], 8).ok()?)
            } else if let Ok(n) = digits.parse::<i64>() {
                Value::Long(n)
            } else {
                match self.arginfo_default_value(digits)? {
                    Value::Double(d) => Value::Double(d),
                    Value::Long(n) => Value::Long(n),
                    _ => Value::Double(digits.parse::<f64>().ok()?),
                }
            };
            return Some(match (neg, v) {
                (true, Value::Long(n)) => Value::Long(n.wrapping_neg()),
                (true, Value::Double(d)) => Value::Double(-d),
                (_, v) => v,
            });
        }
        if let Ok(d) = src.parse::<f64>() {
            return Some(Value::Double(d));
        }
        if let Some(cls) = src.strip_suffix("::class") {
            return Some(Value::String(PhpString::from_string(
                cls.trim_start_matches('\\').to_string(),
            )));
        }
        if src.contains("::") {
            // Class constant / enum case: resolve like constant()
            let f = self.functions.get(b"constant".as_slice()).copied()?;
            let had = self.current_exception.is_some();
            let r = f(
                self,
                &[Value::String(PhpString::from_string(
                    src.trim_start_matches('\\').to_string(),
                ))],
            )
            .ok();
            if !had && self.current_exception.is_some() {
                self.current_exception = None;
                return None;
            }
            return r;
        }
        self.constants
            .get(src.trim_start_matches('\\').as_bytes())
            .cloned()
    }

    /// Map named arguments of a call to builtin `info` onto positions, the
    /// way zend_handle_named_arg + zend_handle_undef_args do for internal
    /// functions: skipped optional parameters get their stub default value.
    /// Returns the positions that were filled with defaults.
    pub(crate) fn zpp_resolve_named(
        &mut self,
        info: &'static FuncInfo,
        args: &mut Vec<Value>,
        named: Vec<(Vec<u8>, Value)>,
    ) -> Result<Vec<usize>, ZppError> {
        let positional = args.len();
        let mut set = vec![false; positional];
        set.iter_mut().for_each(|s| *s = true);
        let num_args = info.num_args();
        // Unknown names given to a variadic function are collected as extra
        // named params; zpp rejects them once it has checked the argument
        // count (that the named ones placed so far are part of).
        let mut extra_named = false;
        for (name, val) in named {
            let idx = info.params[..num_args]
                .iter()
                .position(|p| p.name.as_bytes() == name.as_slice());
            let Some(idx) = idx else {
                if info.is_variadic() {
                    extra_named = true;
                    continue;
                }
                return Err(ZppError::Throw(
                    b"Error",
                    format!("Unknown named parameter ${}", crate::string::msg_str(&name)),
                ));
            };
            if idx < set.len() && set[idx] {
                return Err(ZppError::Throw(
                    b"Error",
                    format!(
                        "Named parameter ${} overwrites previous argument",
                        crate::string::msg_str(&name)
                    ),
                ));
            }
            while args.len() <= idx {
                args.push(Value::Undef);
                set.push(false);
            }
            args[idx] = val;
            set[idx] = true;
        }
        let mut defaulted = Vec::new();
        for i in positional..args.len() {
            if set[i] {
                continue;
            }
            let p = &info.params[i];
            if i < info.required as usize {
                return Err(ZppError::Throw(
                    b"ArgumentCountError",
                    format!("{} not passed", info.arg_prefix(i)),
                ));
            }
            let dv = match p.default {
                Some(d) => self.arginfo_default_value(d),
                None => None,
            };
            if self.current_exception.is_some() {
                return Err(ZppError::Pending);
            }
            let Some(dv) = dv else {
                return Err(ZppError::Throw(
                    b"ArgumentCountError",
                    format!(
                        "{} must be passed explicitly, because the default value is not known",
                        info.arg_prefix(i)
                    ),
                ));
            };
            args[i] = if p.by_ref != 0 {
                Value::Reference(std::rc::Rc::new(std::cell::RefCell::new(dv)))
            } else {
                dv
            };
            defaulted.push(i);
        }
        if extra_named {
            if let Some(msg) = info.arity_error(args.len()) {
                return Err(ZppError::Throw(b"ArgumentCountError", msg));
            }
            return Err(ZppError::Throw(
                b"ArgumentCountError",
                format!("{}() does not accept unknown named parameters", info.name),
            ));
        }
        Ok(defaulted)
    }

    /// zend_parse_parameters for a builtin whose arginfo was verified:
    /// argument count, then each scalar/array typed by-value parameter is
    /// checked and coerced in place (weak or strict mode).
    ///
    /// Like zpp, coercions to string (and int|float) replace the argument in
    /// the call frame while int/float/bool ones only change the value the
    /// function sees: when one of the latter happens, `orig` receives the
    /// arguments as passed, for the frame shown in backtraces.
    pub(crate) fn zpp_check(
        &mut self,
        info: &'static FuncInfo,
        flags: u8,
        args: &mut [Value],
        orig: &mut Option<Vec<Value>>,
        strict: bool,
        line: u32,
    ) -> Result<(), ZppError> {
        if flags & CHECK_ARITY != 0 {
            if let Some(msg) = info.arity_error(args.len()) {
                return Err(ZppError::Throw(b"ArgumentCountError", msg));
            }
        }
        if flags & CHECK_TYPES == 0 {
            return Ok(());
        }
        for i in 0..args.len() {
            let Some(p) = info.param(i) else { break };
            let number = zpp_number_param(info, i);
            let m = if number { T_INT | T_FLOAT } else { p.mask };
            if m & T_MIXED != 0 || (p.by_ref != 0 && m == 0) {
                continue;
            }
            // The common case, without a copy of the argument: a plain value
            // (not a reference) of a type a scalar/array-typed by-value
            // parameter accepts as is.
            if p.by_ref == 0 && m != 0 && m & (T_CLASS | T_OBJECT | T_CALLABLE) == 0 {
                let vt = match &args[i] {
                    Value::Null => T_NULL,
                    Value::False => T_FALSE,
                    Value::True => T_TRUE,
                    Value::Long(_) => T_INT,
                    Value::Double(_) => T_FLOAT,
                    Value::String(_) => T_STRING,
                    Value::Array(_) => T_ARRAY,
                    _ => 0,
                };
                if m & vt != 0 {
                    continue;
                }
            }
            // A typed by-reference parameter (Z_PARAM_ARRAY_EX(.., 0, 1),
            // `array &$array`): the referenced value is checked, never
            // coerced; a scalar-typed one is left to the function.
            if p.by_ref != 0 && m & (T_BOOL | T_INT | T_FLOAT | T_STRING | T_CALLABLE) == 0 {
                let v = args[i].deref();
                if m & value_type_bit(&v) != 0
                    || matches!(v, Value::Object(_) | Value::Generator(_))
                        && m & (T_OBJECT | T_CLASS) != 0
                {
                    continue;
                }
                let ty = if m == T_ARRAY | T_OBJECT {
                    "array"
                } else {
                    arg_type_display(p)
                };
                return Err(ZppError::Throw(
                    b"TypeError",
                    format!(
                        "{} must be of type {}, {} given",
                        info.arg_prefix(i),
                        ty,
                        value_name(&v)
                    ),
                ));
            }
            // (Closure::fromCallable() takes its callable as a plain zval.)
            if p.by_ref == 0
                && m & T_CALLABLE != 0
                && m & !(T_CALLABLE | T_NULL) == 0
                && info.name != "Closure::fromCallable"
            {
                // Z_PARAM_FUNC / Z_PARAM_FUNC_OR_NULL (zend_fcall_info_init).
                // Deprecations are left to the function's own check.
                let v = args[i].deref();
                if !(m & T_NULL != 0 && matches!(v, Value::Null | Value::Undef)) {
                    let r = self.zend_is_callable_ex(
                        &v,
                        crate::callable::IS_CALLABLE_SUPPRESS_DEPRECATIONS,
                    );
                    if self.current_exception.is_some() {
                        return Err(ZppError::Pending);
                    }
                    if !r.ok {
                        let msg = format!(
                            "{}(): Argument #{} (${}) must be a valid callback{}, {}",
                            info.name,
                            i + 1,
                            p.name,
                            if m & T_NULL != 0 { " or null" } else { "" },
                            r.error.unwrap_or_default()
                        );
                        return Err(ZppError::Throw(b"TypeError", msg));
                    }
                }
                break;
            }
            let mut v = args[i].deref();
            if p.by_ref == 0
                && m & T_CALLABLE == 0
                && m & (T_CLASS | T_OBJECT) != 0
                && !matches!(v, Value::Object(_) | Value::Generator(_))
            {
                // An object or class typed parameter given a non-object: only
                // the scalar/array part of the type can accept it (e.g.
                // key("abc"), iterator_to_array(true)); the rest is left to
                // the function.
                if m & value_type_bit(&v) != 0 {
                    // array|object (Z_PARAM_ARRAY_OR_OBJECT) given an
                    // array: fully checked, go on with the next ones.
                    if m & !(T_ARRAY | T_OBJECT | T_NULL) == 0 {
                        continue;
                    }
                    break;
                }
                if m & (T_BOOL | T_INT | T_FLOAT | T_STRING) != 0 {
                    break;
                }
                // Z_PARAM_ARRAY_OR_OBJECT expects "array".
                let ty = if m == T_ARRAY | T_OBJECT {
                    "array"
                } else if m == T_ARRAY | T_OBJECT | T_NULL {
                    "?array"
                } else {
                    arg_type_display(p)
                };
                return Err(ZppError::Throw(
                    b"TypeError",
                    format!(
                        "{} must be of type {}, {} given",
                        info.arg_prefix(i),
                        ty,
                        value_name(&v)
                    ),
                ));
            }
            // An object given to a class-typed parameter (Z_PARAM_OBJ_OF_CLASS,
            // Z_PARAM_ITERABLE...): it must be an instance of one of them.
            if p.by_ref == 0
                && m & T_CLASS != 0
                && m & (T_OBJECT | T_CALLABLE | T_MIXED) == 0
                && matches!(v, Value::Object(_) | Value::Generator(_))
            {
                let ok =
                    p.ty.split('|')
                        .map(|t| t.trim_start_matches('?'))
                        .filter(|t| t.bytes().next().is_some_and(|c| c.is_ascii_uppercase()))
                        .any(|cn| {
                            self.value_matches_type_mode(
                                &v,
                                &crate::opcode::ParamType::Simple(cn.as_bytes().to_vec()),
                                true,
                            )
                        });
                // Z_PARAM_OBJ_OF_CLASS_OR_STR: a Stringable object is a string
                // (in weak mode, zend_parse_arg_str_weak()).
                if !ok && !(!strict && m & T_STRING != 0 && self.zpp_object_has_tostring(&v)) {
                    return Err(self.zpp_type_error_n(info, i, p, &v, number));
                }
                // An instance is fully checked: the next parameters are
                // parsed too (hash_final($ctx, []) is a TypeError).
                if ok {
                    continue;
                }
                break;
            }
            // An object given to an `object` / `array|object` parameter
            // (Z_PARAM_OBJECT, Z_PARAM_ARRAY_OR_OBJECT): fully checked.
            if p.by_ref == 0
                && m & T_OBJECT != 0
                && m & !(T_ARRAY | T_OBJECT | T_NULL) == 0
                && matches!(v, Value::Object(_) | Value::Generator(_))
            {
                continue;
            }
            if p.by_ref != 0 || m == 0 || m & (T_CLASS | T_OBJECT | T_CALLABLE) != 0 {
                // Validated by the function itself; leave the rest to it too
                // so errors keep zpp's left-to-right order.
                break;
            }
            let vt = match &v {
                Value::Undef | Value::Null => T_NULL,
                Value::False => T_FALSE,
                Value::True => T_TRUE,
                Value::Long(_) => T_INT,
                Value::Double(_) => T_FLOAT,
                Value::String(_) => T_STRING,
                Value::Array(_) => T_ARRAY,
                _ => T_OBJECT,
            };
            if m & vt != 0 {
                if matches!(args[i], Value::Reference(_) | Value::Undef) {
                    args[i] = if vt == T_NULL { Value::Null } else { v };
                }
                continue;
            }
            if strict {
                if vt == T_INT && m & T_FLOAT != 0 {
                    if let Value::Long(n) = v {
                        self.zpp_set_arg(args, orig, i, m, Value::Double(n as f64));
                    }
                    continue;
                }
                return Err(self.zpp_type_error_n(info, i, p, &v, number));
            }
            if vt == T_NULL {
                if m & (T_BOOL | T_INT | T_FLOAT | T_STRING) == 0 {
                    return Err(self.zpp_type_error_n(info, i, p, &v, number));
                }
                let msg = match info.arg_name(i) {
                    Some(n) => format!(
                        "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
                        info.name,
                        i + 1,
                        n,
                        arg_type_display(p)
                    ),
                    None => format!(
                        "{}(): Passing null to parameter #{} of type {} is deprecated",
                        info.name,
                        i + 1,
                        arg_type_display(p)
                    ),
                };
                self.emit_deprecated_at(&msg, line);
                if self.current_exception.is_some() {
                    return Err(ZppError::Pending);
                }
                let nv = if m & T_INT != 0 {
                    Value::Long(0)
                } else if m & T_FLOAT != 0 {
                    Value::Double(0.0)
                } else if m & T_STRING != 0 {
                    Value::String(PhpString::empty())
                } else {
                    Value::False
                };
                self.zpp_set_arg(args, orig, i, m, nv);
                continue;
            }
            match self.zpp_weak_coerce(m, &mut v, line) {
                Ok(true) => self.zpp_set_arg(args, orig, i, m, v),
                Ok(false) => return Err(self.zpp_type_error_n(info, i, p, &v, number)),
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// The scalar parameter coercion of zend_parse_parameters for
    /// extensions that parse their arguments themselves: `mask` is a
    /// combination of the T_* type bits, `v` a non-null value. Ok(Some) is
    /// the accepted (possibly coerced) value, Ok(None) a type error, Err(())
    /// an exception raised meanwhile (e.g. by __toString or an error
    /// handler).
    #[allow(clippy::result_unit_err)]
    pub fn zpp_coerce_scalar(
        &mut self,
        mask: u16,
        v: &Value,
        strict: bool,
        line: u32,
    ) -> Result<Option<Value>, ()> {
        let mut v = v.deref();
        let vt = match &v {
            Value::Undef | Value::Null => T_NULL,
            Value::False => T_FALSE,
            Value::True => T_TRUE,
            Value::Long(_) => T_INT,
            Value::Double(_) => T_FLOAT,
            Value::String(_) => T_STRING,
            Value::Array(_) => T_ARRAY,
            _ => T_OBJECT,
        };
        if mask & vt != 0 {
            return Ok(Some(v));
        }
        if strict {
            if vt == T_INT && mask & T_FLOAT != 0 {
                if let Value::Long(n) = v {
                    return Ok(Some(Value::Double(n as f64)));
                }
            }
            return Ok(None);
        }
        match self.zpp_weak_coerce(mask, &mut v, line) {
            Ok(true) => Ok(Some(v)),
            Ok(false) => Ok(None),
            Err(_) => Err(()),
        }
    }

    /// Store a coerced argument (see zpp_check about `orig`).
    fn zpp_set_arg(
        &self,
        args: &mut [Value],
        orig: &mut Option<Vec<Value>>,
        i: usize,
        m: u16,
        v: Value,
    ) {
        let in_frame = matches!(v, Value::String(_)) || m & (T_INT | T_FLOAT) == (T_INT | T_FLOAT);
        if !in_frame && orig.is_none() {
            *orig = Some(args.to_vec());
        }
        if in_frame {
            if let Some(o) = orig.as_mut() {
                o[i] = v.clone();
            }
        }
        args[i] = v;
    }

    /// The TypeError for argument `i`; `number`: it is parsed with
    /// Z_PARAM_NUMBER, whose error names int|float whatever the arginfo says.
    fn zpp_type_error_n(
        &self,
        info: &FuncInfo,
        i: usize,
        p: &ArgInfo,
        arg: &Value,
        number: bool,
    ) -> ZppError {
        ZppError::Throw(
            b"TypeError",
            format!(
                "{} must be of type {}, {} given",
                info.arg_prefix(i),
                if number {
                    "int|float"
                } else {
                    arg_type_display(p)
                },
                value_name(arg)
            ),
        )
    }

    /// zend_verify_weak_scalar_type_hint(): int -> float -> string -> bool.
    /// Ok(false) when the value cannot be coerced (TypeError).
    fn zpp_weak_coerce(&mut self, m: u16, arg: &mut Value, line: u32) -> Result<bool, ZppError> {
        if m & T_INT != 0 {
            if m & T_FLOAT != 0 && matches!(arg, Value::String(_)) {
                if let Value::String(s) = arg {
                    if let Some(n) = numeric(s) {
                        *arg = n;
                        return Ok(true);
                    }
                }
            } else if let Some(n) = self.zpp_long_weak(arg, line)? {
                *arg = Value::Long(n);
                return Ok(true);
            }
        }
        if m & T_FLOAT != 0 {
            let d = match arg {
                Value::Long(n) => Some(*n as f64),
                Value::String(s) => match numeric(s) {
                    Some(Value::Long(n)) => Some(n as f64),
                    Some(Value::Double(d)) => Some(d),
                    _ => None,
                },
                Value::False => Some(0.0),
                Value::True => Some(1.0),
                _ => None,
            };
            if let Some(d) = d {
                *arg = Value::Double(d);
                return Ok(true);
            }
        }
        if m & T_STRING != 0 {
            match arg {
                Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
                    // zend_double_to_str()
                    if matches!(arg, Value::Double(d) if d.is_nan()) {
                        self.emit_warning_at("unexpected NAN value was coerced to string", line);
                        if self.current_exception.is_some() {
                            return Err(ZppError::Pending);
                        }
                    }
                    *arg = Value::String(arg.to_php_string());
                    return Ok(true);
                }
                Value::Object(_) => {
                    if !self.zpp_object_has_tostring(arg) {
                        return Ok(false);
                    }
                    let had = self.current_exception.is_some();
                    let s = self.value_to_string(arg);
                    if !had && self.current_exception.is_some() {
                        return Err(ZppError::Pending);
                    }
                    *arg = Value::String(s);
                    return Ok(true);
                }
                _ => {}
            }
        }
        if m & T_BOOL == T_BOOL
            && matches!(arg, Value::Long(_) | Value::Double(_) | Value::String(_))
        {
            // zend_parse_arg_bool_weak()
            if matches!(arg, Value::Double(d) if d.is_nan()) {
                self.emit_warning_at("unexpected NAN value was coerced to bool", line);
                if self.current_exception.is_some() {
                    return Err(ZppError::Pending);
                }
            }
            *arg = if arg.is_truthy() {
                Value::True
            } else {
                Value::False
            };
            return Ok(true);
        }
        Ok(false)
    }

    /// zend_parse_arg_long_weak() for a non-null, non-int value.
    fn zpp_long_weak(&mut self, arg: &Value, line: u32) -> Result<Option<i64>, ZppError> {
        let fits = |d: f64| !d.is_nan() && crate::value::php_double_fits_long(d);
        let r = match arg {
            Value::Double(d) => {
                if !fits(*d) {
                    return Ok(None);
                }
                if d.fract() != 0.0 {
                    let msg = format!(
                        "Implicit conversion from float {} to int loses precision",
                        crate::value::format_php_float_repr(*d)
                    );
                    self.emit_deprecated_at(&msg, line);
                    if self.current_exception.is_some() {
                        return Err(ZppError::Pending);
                    }
                }
                Some(*d as i64)
            }
            Value::String(s) => match numeric(s) {
                Some(Value::Long(n)) => Some(n),
                Some(Value::Double(d)) => {
                    if !fits(d) {
                        return Ok(None);
                    }
                    if d.fract() != 0.0 {
                        let msg = format!(
                            "Implicit conversion from float-string \"{}\" to int loses precision",
                            s.to_msg_string()
                        );
                        self.emit_deprecated_at(&msg, line);
                        if self.current_exception.is_some() {
                            return Err(ZppError::Pending);
                        }
                    }
                    Some(d as i64)
                }
                _ => None,
            },
            Value::False => Some(0),
            Value::True => Some(1),
            _ => None,
        };
        Ok(r)
    }

    fn zpp_object_has_tostring(&self, v: &Value) -> bool {
        let Value::Object(o) = v else { return false };
        let lower: Vec<u8> = o.borrow().class_name.to_ascii_lowercase();
        if self
            .classes
            .get(&lower)
            .is_some_and(|c| c.methods.contains_key(b"__tostring".as_slice()))
        {
            return true;
        }
        // Internal classes with a cast handler / __toString (as in
        // value_to_string_checked)
        lower == b"exception"
            || lower == b"error"
            || crate::vm::is_builtin_subclass(&lower, b"exception")
            || crate::vm::is_builtin_subclass(&lower, b"error")
            || self.class_extends(&lower, b"exception")
            || self.class_extends(&lower, b"error")
            || lower.starts_with(b"reflection")
            || matches!(
                lower.as_slice(),
                b"gmp"
                    | b"simplexmlelement"
                    | b"phptoken"
            )
            || self.spl_has_builtin_tostring(&lower)
            // SimpleXMLIterator and user subclasses keep the cast handler
            || self.class_extends(&lower, b"simplexmlelement")
            // extension classes whose native handler serves __toString()
            || self
                .find_native_class_handler(&lower)
                .is_some_and(|(c, h)| (h.has_method)(&c, b"__tostring", false))
    }
}

/// Unquote a PHP string literal from a stub ('..' or ".." with escapes).
fn unquote(src: &str) -> Option<Vec<u8>> {
    let b = src.as_bytes();
    let q = b[0];
    if b.len() < 2 || b[b.len() - 1] != q {
        return None;
    }
    let inner = &b[1..b.len() - 1];
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        let c = inner[i];
        if c == b'\\' && i + 1 < inner.len() {
            let n = inner[i + 1];
            if q == b'\'' {
                if n == b'\'' || n == b'\\' {
                    out.push(n);
                    i += 2;
                    continue;
                }
                out.push(c);
                i += 1;
                continue;
            }
            let rep = match n {
                b'n' => Some(b'\n'),
                b't' => Some(b'\t'),
                b'r' => Some(b'\r'),
                b'v' => Some(0x0b),
                b'f' => Some(0x0c),
                b'e' => Some(0x1b),
                b'0' => Some(0),
                b'\\' => Some(b'\\'),
                b'"' => Some(b'"'),
                b'$' => Some(b'$'),
                _ => None,
            };
            match rep {
                Some(r) => {
                    out.push(r);
                    i += 2;
                }
                None => {
                    out.push(c);
                    i += 1;
                }
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    Some(out)
}
