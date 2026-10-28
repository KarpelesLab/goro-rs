//! Functions and methods: user code or native handlers, one type for both
//! so calls, callables and Reflection treat them alike.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use goro_runtime::{ObjRef, PhpRef, PhpStr, Value};

use crate::bytecode::{FuncCode, ParamInfo, TypeHint};
use crate::class::Class;
use crate::vm::{R, Vm};

/// A native function or method handler.
pub type NativeFn = fn(&mut Vm, &mut Args) -> R<Value>;

/// The parsed signature of a native function.
#[derive(Debug)]
pub struct NativeSig {
    pub params: Vec<ParamInfo>,
    pub ret: Option<TypeHint>,
    pub num_required: u32,
    pub by_ref_return: bool,
    pub variadic: bool,
    pub ret_text: Option<String>,
}

pub enum FuncKind {
    User(Rc<FuncCode>),
    Native {
        handler: NativeFn,
        /// PHP declaration text: `function name(type $param = default, ...): ret`.
        sig: &'static str,
        info: OnceCell<Rc<NativeSig>>,
    },
}

/// A function, method or closure body.
pub struct Func {
    pub name: PhpStr,
    pub lname: PhpStr,
    pub kind: FuncKind,
    /// Member flags (`mflags`) for methods.
    pub flags: u32,
    /// Declaring class for methods (null for functions).
    pub scope: Cell<*const Class>,
    /// For trait methods imported into a class: the trait.
    pub from_trait: Cell<*const Class>,
    /// Static variables (`static $x`), bound by reference.
    pub statics: RefCell<Vec<Option<PhpRef>>>,
    /// The method this one implements or overrides (for prototype checks).
    pub proto: RefCell<Option<Rc<Func>>>,
}

impl std::fmt::Debug for Func {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Func({})", self.name.to_string_lossy())
    }
}

impl Func {
    pub fn user(code: Rc<FuncCode>, flags: u32) -> Func {
        let name = code.name.clone();
        let lname = crate::class::lower(&name);
        let n = code.num_statics as usize;
        Func {
            name,
            lname,
            kind: FuncKind::User(code),
            flags,
            scope: Cell::new(std::ptr::null()),
            from_trait: Cell::new(std::ptr::null()),
            statics: RefCell::new(vec![None; n]),
            proto: RefCell::new(None),
        }
    }

    pub fn native(name: &str, sig: &'static str, handler: NativeFn, flags: u32) -> Func {
        Func {
            name: PhpStr::intern(name.as_bytes()),
            lname: crate::class::lower_intern(name.as_bytes()),
            kind: FuncKind::Native {
                handler,
                sig,
                info: OnceCell::new(),
            },
            flags,
            scope: Cell::new(std::ptr::null()),
            from_trait: Cell::new(std::ptr::null()),
            statics: RefCell::new(Vec::new()),
            proto: RefCell::new(None),
        }
    }

    pub fn code(&self) -> Option<&Rc<FuncCode>> {
        match &self.kind {
            FuncKind::User(c) => Some(c),
            _ => None,
        }
    }

    pub fn is_native(&self) -> bool {
        matches!(self.kind, FuncKind::Native { .. })
    }

    pub fn is_static(&self) -> bool {
        self.flags & crate::class::mflags::STATIC != 0
    }

    pub fn scope(&self) -> Option<&Class> {
        let p = self.scope.get();
        if p.is_null() {
            None
        } else {
            // SAFETY: classes live as long as the engine.
            Some(unsafe { &*p })
        }
    }

    /// `Class::name` or `name`, as PHP names functions in messages.
    pub fn display_name(&self) -> String {
        match self.scope() {
            Some(c) => format!(
                "{}::{}",
                c.name.to_string_lossy(),
                self.name.to_string_lossy()
            ),
            None => self.name.to_string_lossy(),
        }
    }
}

/// Arguments of a native call.
pub struct Args<'a> {
    /// `$this` for instance methods.
    pub this: Option<ObjRef>,
    /// The arguments, coerced to the declared parameter types. By-reference
    /// parameters hold a `Value::Ref`. Omitted optional parameters are
    /// absent (when trailing) or hold their default (when a later one was
    /// passed by name).
    pub vals: &'a mut Vec<Value>,
    pub func: &'a Rc<Func>,
    /// Late static binding class (`static::`) for static methods.
    pub static_class: *const Class,
    /// Named arguments collected by a variadic parameter.
    pub named: Option<goro_runtime::PhpArray>,
}

impl<'a> Args<'a> {
    #[inline]
    pub fn len(&self) -> usize {
        self.vals.len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.vals.is_empty()
    }
    #[inline]
    pub fn get(&self, i: usize) -> Option<&Value> {
        self.vals.get(i)
    }
    /// Argument `i` dereferenced (null if absent).
    #[inline]
    pub fn val(&self, i: usize) -> Value {
        match self.vals.get(i) {
            Some(v) => v.deref(),
            None => Value::Null,
        }
    }
    #[inline]
    pub fn has(&self, i: usize) -> bool {
        i < self.vals.len()
    }
    pub fn int(&self, i: usize) -> i64 {
        match self.vals.get(i) {
            Some(Value::Int(n)) => *n,
            Some(Value::Bool(b)) => *b as i64,
            Some(Value::Float(f)) => *f as i64,
            Some(Value::Ref(r)) => match &*r.borrow() {
                Value::Int(n) => *n,
                _ => 0,
            },
            _ => 0,
        }
    }
    pub fn int_or(&self, i: usize, d: i64) -> i64 {
        match self.vals.get(i) {
            None | Some(Value::Null) => d,
            _ => self.int(i),
        }
    }
    pub fn float(&self, i: usize) -> f64 {
        match self.vals.get(i) {
            Some(Value::Float(f)) => *f,
            Some(Value::Int(n)) => *n as f64,
            _ => 0.0,
        }
    }
    pub fn bool(&self, i: usize) -> bool {
        match self.vals.get(i) {
            Some(Value::Bool(b)) => *b,
            Some(Value::Int(n)) => *n != 0,
            Some(v) => goro_runtime::convert::to_bool_quiet(v),
            None => false,
        }
    }
    pub fn bool_or(&self, i: usize, d: bool) -> bool {
        if self.has(i) { self.bool(i) } else { d }
    }
    /// String argument (empty if absent or not a string).
    pub fn str(&self, i: usize) -> PhpStr {
        match self.vals.get(i) {
            Some(Value::Str(s)) => s.clone(),
            Some(Value::Ref(r)) => match &*r.borrow() {
                Value::Str(s) => s.clone(),
                _ => PhpStr::empty(),
            },
            _ => PhpStr::empty(),
        }
    }
    pub fn bytes(&self, i: usize) -> &[u8] {
        match self.vals.get(i) {
            Some(Value::Str(s)) => s.as_bytes(),
            _ => b"",
        }
    }
    /// Optional string (None when absent or null).
    pub fn opt_str(&self, i: usize) -> Option<PhpStr> {
        match self.vals.get(i) {
            Some(Value::Str(s)) => Some(s.clone()),
            _ => None,
        }
    }
    pub fn opt_int(&self, i: usize) -> Option<i64> {
        match self.vals.get(i) {
            None | Some(Value::Null) => None,
            _ => Some(self.int(i)),
        }
    }
    pub fn array(&self, i: usize) -> goro_runtime::PhpArray {
        match self.vals.get(i) {
            Some(Value::Array(a)) => a.clone(),
            Some(Value::Ref(r)) => match &*r.borrow() {
                Value::Array(a) => a.clone(),
                _ => goro_runtime::PhpArray::new(),
            },
            _ => goro_runtime::PhpArray::new(),
        }
    }
    pub fn obj(&self, i: usize) -> Option<ObjRef> {
        match self.vals.get(i) {
            Some(Value::Object(o)) => Some(o.clone()),
            _ => None,
        }
    }
    /// The reference cell of a by-reference argument.
    pub fn ref_at(&self, i: usize) -> Option<PhpRef> {
        match self.vals.get(i) {
            Some(Value::Ref(r)) => Some(r.clone()),
            _ => None,
        }
    }
    /// Assigns to a by-reference argument.
    pub fn set_ref(&self, i: usize, v: Value) {
        if let Some(Value::Ref(r)) = self.vals.get(i) {
            let old = r.set(v);
            drop(old);
        }
    }
    pub fn this_obj(&self) -> &ObjRef {
        self.this.as_ref().expect("method called without $this")
    }
}
