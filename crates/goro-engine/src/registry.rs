//! The extension API: modules registering native functions, classes,
//! constants and ini settings.
//!
//! A [`Module`] is a static description. Functions are declared with their
//! PHP signature (`"function str_pad(string $string, int $length, string
//! $pad_string = \" \", int $pad_type = STR_PAD_RIGHT): string"`), parsed
//! the first time it's needed: the engine checks argument counts, applies
//! PHP's parameter type coercion (with PHP's exact `TypeError` and
//! deprecation messages), fills defaults of parameters skipped by named
//! arguments, and makes by-reference parameters references, before the
//! handler runs. Reflection reads the same signature.
//!
//! Classes are declared with PHP source (class, interface, trait or enum
//! declarations), compiled when first used. Methods listed in
//! [`Module::methods`] (`"class::method"`, lower case) run natively; the
//! others run their PHP body. [`Module::hooks`] attach native behaviour
//! (object creation, cloning, comparison, casts, `count()`, `var_dump`
//! properties, iteration) to a class and its subclasses.
//!
//! ```ignore
//! fn my_strrev(vm: &mut Vm, a: &mut Args) -> R<Value> {
//!     let s = a.str(0);
//!     Ok(Value::from(s.iter().rev().copied().collect::<Vec<u8>>().as_slice()))
//! }
//! pub static MODULE: Module = Module {
//!     name: "mymod",
//!     functions: &[("function my_strrev(string $s): string", my_strrev)],
//!     ..Module::EMPTY
//! };
//! ```

use goro_runtime::Value;

use crate::class::NativeHooks;
use crate::func::NativeFn;
use crate::vm::{R, Vm};

/// A constant's value in a static table.
#[derive(Clone, Copy, Debug)]
pub enum ConstVal {
    Int(i64),
    Float(f64),
    Str(&'static str),
    Bool(bool),
    Null,
}

impl ConstVal {
    pub fn to_value(self) -> Value {
        match self {
            ConstVal::Int(i) => Value::Int(i),
            ConstVal::Float(f) => Value::Float(f),
            ConstVal::Str(s) => Value::Str(goro_runtime::PhpStr::intern(s.as_bytes())),
            ConstVal::Bool(b) => Value::Bool(b),
            ConstVal::Null => Value::Null,
        }
    }
}

/// An extension module.
pub struct Module {
    /// Name reported by `get_loaded_extensions()` / `php -m`.
    pub name: &'static str,
    /// `(signature, handler)`.
    pub functions: &'static [(&'static str, NativeFn)],
    /// `(lower-case class name, PHP declaration)`.
    pub classes: &'static [(&'static str, &'static str)],
    /// `("class::method" lower case, handler)`.
    pub methods: &'static [(&'static str, NativeFn)],
    /// `(lower-case class name, hooks)`.
    pub hooks: &'static [(&'static str, fn() -> NativeHooks)],
    pub constants: &'static [(&'static str, ConstVal)],
    /// Default ini settings `(name, value)`.
    pub ini: &'static [(&'static str, &'static str)],
    /// Called once at startup (dynamic constants, state).
    pub init: Option<fn(&mut Vm)>,
    /// Called at request end.
    pub shutdown: Option<fn(&mut Vm)>,
    /// Called once after the startup settings (`-d`, in order) are applied,
    /// for "PHP Startup:" diagnostics and state derived from them.
    pub startup_ini: Option<fn(&mut Vm, &[(Vec<u8>, Vec<u8>)])>,
    /// Called by `ini_set()` and `ini_restore()` for every setting change
    /// (any name), before the value is stored: `Ok(false)` refuses it
    /// (`ini_set()` returns false).
    pub on_ini_set: Option<fn(&mut Vm, &[u8], &[u8]) -> R<bool>>,
    /// Whether `php -m` lists it (core pieces are folded into "Core").
    pub listed: bool,
    /// The lines of its phpinfo() section ("name => value\n" each), in
    /// place of the default "<name> support => enabled".
    pub info: Option<fn(&mut Vm) -> String>,
}

impl Module {
    pub const EMPTY: Module = Module {
        name: "",
        functions: &[],
        classes: &[],
        methods: &[],
        hooks: &[],
        constants: &[],
        ini: &[],
        init: None,
        shutdown: None,
        startup_ini: None,
        on_ini_set: None,
        listed: true,
        info: None,
    };
}

/// The function name in a signature (`function name(` or `function &name(`).
pub fn sig_name(sig: &str) -> &str {
    let s = sig.trim_start();
    let s = s.strip_prefix("function").unwrap_or(s).trim_start();
    let s = s.strip_prefix('&').unwrap_or(s).trim_start();
    let end = s.find('(').unwrap_or(s.len());
    s[..end].trim()
}
