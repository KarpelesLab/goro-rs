//! BcMath\Number (bcmath.c, PHP 8.4+).
//!
//! The class is declared from PHP source (prelude.php) and made internal,
//! as ext/zend_test does. Its `value` / `scale` properties are real
//! readonly properties: `value` is bc_num2str_ex(num, scale), from which
//! the number is parsed again when it is used (a Number's bc_num never has
//! trailing fraction zeros, so the round trip is exact). The object handlers
//! that have no PHP-level equivalent are native: do_operation (arithmetic
//! operators), compare and the bool cast.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::opcode::OpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm, VmError};

use crate::num::{self, Num};
use crate::{INT_MAX, R, throw};

const PRELUDE: &[u8] = include_bytes!("prelude.php");
const CLASS: &[u8] = b"BcMath\\Number";
const CLASS_LOWER: &[u8] = b"bcmath\\number";
const CLASS_DISPLAY: &str = "BcMath\\Number";
/// BC_MATH_NUMBER_EXPAND_SCALE
const EXPAND_SCALE: usize = 10;

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    cast: Some(cast),
    compare: Some(compare_handler),
    clone_obj: Some(clone_obj),
    ..NativeObjectHandlers::EMPTY
};

type DoOperation = fn(&mut Vm, &str, &Value, &Value) -> Option<Result<Value, String>>;
type IsOperatorType = fn(&Value) -> bool;

thread_local! {
    static PREV_DO_OPERATION: std::cell::Cell<Option<DoOperation>> = const { std::cell::Cell::new(None) };
    static PREV_IS_TYPE: std::cell::Cell<Option<IsOperatorType>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn register(vm: &mut Vm) {
    vm.register_function(b"__bcmath_number", number_method);
    let Some((op_array, classes)) = compile_prelude() else {
        return;
    };
    let saved_file = std::mem::take(&mut vm.current_file);
    let saved_line = vm.current_line;
    let _ = vm.run_compiled(&op_array, classes);
    vm.current_file = saved_file;
    vm.current_line = saved_line;
    vm.current_exception = None;
    make_internal(vm);
    PREV_DO_OPERATION.with(|p| p.set(vm.object_do_operation));
    PREV_IS_TYPE.with(|p| p.set(vm.object_is_operator_type));
    vm.object_do_operation = Some(do_operation);
    vm.object_is_operator_type = Some(is_operator_type);
}

fn compile_prelude() -> Option<(OpArray, Vec<ClassEntry>)> {
    let mut lexer = goro_parser::Lexer::new(PRELUDE);
    let tokens = lexer.tokenize();
    let _ = goro_parser::take_lexer_deprecations();
    let _ = goro_parser::take_lexer_warnings();
    let mut parser = goro_parser::Parser::new(tokens).with_source(PRELUDE);
    let program = parser.parse().ok()?;
    let compiler = goro_core::compiler::Compiler::new();
    let (mut op_array, mut classes, _) = compiler.compile(&program).ok()?;
    let _ = goro_core::compiler::take_pending_compile_warnings();
    clear_lines(&mut op_array);
    for c in &mut classes {
        for m in c.methods.values_mut() {
            clear_lines(Rc::make_mut(&mut m.op_array));
        }
    }
    Some((op_array, classes))
}

/// Internal code has no lines: the VM keeps reporting the caller's.
fn clear_lines(op: &mut OpArray) {
    for o in &mut op.ops {
        o.line = 0;
    }
    op.decl_line = 0;
    op.end_line = 0;
    op.filename.clear();
    for c in &mut op.child_functions {
        clear_lines(c);
    }
}

fn make_internal(vm: &mut Vm) {
    let Some(ce) = vm.classes.get_mut(CLASS_LOWER) else {
        return;
    };
    ce.filename = None;
    ce.start_line = 0;
    ce.end_line = 0;
    ce.doc_comment = None;
    let Some(sc) = goro_core::stubs::find_self_declared_class(CLASS_LOWER) else {
        return;
    };
    for m in ce.methods.values_mut() {
        let Some(f) = sc
            .methods
            .iter()
            .find(|f| f.name.as_bytes().eq_ignore_ascii_case(&m.name))
        else {
            continue;
        };
        let oa = Rc::make_mut(&mut m.op_array);
        oa.internal_sig = Some(f);
        oa.doc_comment = None;
        m.doc_comment = None;
    }
}

// ---------------------------------------------------------------------------
// object helpers

fn is_number(v: &Value) -> bool {
    match v {
        Value::Object(o) => o.borrow().class_name.eq_ignore_ascii_case(CLASS),
        Value::Reference(r) => is_number(&r.borrow()),
        _ => false,
    }
}

fn is_operator_type(v: &Value) -> bool {
    is_number(v) || PREV_IS_TYPE.with(|p| p.get()).is_some_and(|f| f(v))
}

fn attach(o: &Rc<RefCell<PhpObject>>) {
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(()),
        handlers: &HANDLERS,
    });
}

/// The number and scale of a Number object.
fn obj_num(o: &Rc<RefCell<PhpObject>>) -> (Num, usize) {
    let ob = o.borrow();
    let s = ob.get_property(b"value").to_php_string();
    let scale = ob.get_property(b"scale").to_long().max(0) as usize;
    drop(ob);
    let n = num::str2num(s.as_bytes(), 0, true)
        .0
        .unwrap_or_else(Num::zero);
    (n, scale)
}

fn set_props(o: &Rc<RefCell<PhpObject>>, n: &Num, scale: usize) {
    let mut ob = o.borrow_mut();
    ob.set_property(
        b"value".to_vec(),
        Value::String(PhpString::from_vec(num::num2str(n, scale))),
    );
    ob.set_property(b"scale".to_vec(), Value::Long(scale as i64));
}

/// bcmath_number_new_obj()
fn new_number(vm: &mut Vm, n: &Num, scale: usize) -> Value {
    let id = vm.alloc_object_id();
    let o = Rc::new(RefCell::new(PhpObject::new(CLASS.to_vec(), id)));
    set_props(&o, n, scale);
    attach(&o);
    Value::Object(o)
}

fn cast(o: &Rc<RefCell<PhpObject>>, kind: u8) -> Option<Value> {
    if kind != 0 {
        return None;
    }
    let (n, _) = obj_num(o);
    Some(if n.is_zero() {
        Value::False
    } else {
        Value::True
    })
}

fn clone_obj(_vm: &mut Vm, _src: &Rc<RefCell<PhpObject>>, dst: &Rc<RefCell<PhpObject>>) {
    attach(dst);
}

/// An operand of an operator or comparison (bcmath_number_parse_num() +
/// bc_num_from_obj_or_str_or_long()). Err(true): not a number operand,
/// Err(false): a string that is not well-formed.
fn operand(vm: &mut Vm, v: &Value) -> Result<(Num, usize), bool> {
    let v = v.deref();
    match &v {
        Value::Object(o) if is_number(&v) => Ok(obj_num(o)),
        Value::Long(l) => Ok((num::long2num(*l), 0)),
        Value::String(s) => match num::str2num(s.as_bytes(), 0, true) {
            (Some(n), fs) => Ok((n, fs)),
            _ => Err(false),
        },
        Value::Null | Value::Undef => Err(true),
        Value::True => Ok((num::long2num(1), 0)),
        Value::False => Ok((num::long2num(0), 0)),
        Value::Double(d) => {
            // zend_parse_arg_long_slow()
            if d.is_nan() || !goro_core::value::php_double_fits_long(*d) {
                return Err(true);
            }
            if d.fract() != 0.0 {
                let msg = format!(
                    "Implicit conversion from float {} to int loses precision",
                    goro_core::value::format_php_float(*d)
                );
                let line = vm.current_line;
                vm.emit_deprecated_at(&msg, line);
            }
            Ok((num::long2num(*d as i64), 0))
        }
        _ => Err(true),
    }
}

/// zend_zval_type_name()
fn type_name(v: &Value) -> String {
    match v.deref() {
        Value::Object(o) => goro_core::value::display_class_name(&o.borrow().class_name),
        Value::Null | Value::Undef => "null".to_string(),
        Value::True | Value::False => "bool".to_string(),
        Value::Long(_) => "int".to_string(),
        Value::Double(_) => "float".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Array(_) => "array".to_string(),
        Value::Resource(_) => "resource".to_string(),
        Value::Generator(_) => "Generator".to_string(),
        Value::Reference(_) => "reference".to_string(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

/// The error of a failed calculation: (class, message).
type CalcErr = (&'static [u8], String);

/// bcmath_number_{add,sub,mul,div,mod,pow}_internal(): (result, scale).
/// `scale` is used unless `auto_scale`. `arg_prefix` is the
/// "Number::m(): Argument #1 ($x)" prefix for methods (None: operator).
fn calc(
    op: Op,
    n1: &Num,
    n2: &Num,
    n1_full_scale: usize,
    n2_full_scale: usize,
    scale: usize,
    auto_scale: bool,
    arg_prefix: Option<&str>,
) -> Result<(Num, usize), CalcErr> {
    let too_large = || -> CalcErr {
        (
            b"ValueError",
            "scale of the result is too large".to_string(),
        )
    };
    let mut scale = scale;
    let ret = match op {
        Op::Add | Op::Sub => {
            if auto_scale {
                scale = n1_full_scale.max(n2_full_scale);
            }
            let mut r = if op == Op::Add {
                num::add(n1, n2, scale)
            } else {
                num::sub(n1, n2, scale)
            };
            r.trunc_scale(scale);
            r.rm_trailing_zeros();
            r
        }
        Op::Mul => {
            if auto_scale {
                scale = n1_full_scale + n2_full_scale;
                if scale as i64 > INT_MAX {
                    return Err(too_large());
                }
            }
            let mut r = num::multiply(n1, n2, scale);
            r.trunc_scale(scale);
            r.rm_trailing_zeros();
            r
        }
        Op::Div => {
            if auto_scale {
                scale = n1_full_scale + EXPAND_SCALE;
                if scale as i64 > INT_MAX {
                    return Err(too_large());
                }
            }
            let Some(mut r) = num::divide(n1, n2, scale) else {
                return Err((b"DivisionByZeroError", "Division by zero".to_string()));
            };
            r.rm_trailing_zeros();
            if auto_scale {
                let diff = scale - r.scale;
                scale -= diff.min(EXPAND_SCALE);
            }
            r
        }
        Op::Mod => {
            if auto_scale {
                scale = n1_full_scale.max(n2_full_scale);
            }
            let Some(mut r) = num::modulo(n1, n2, scale) else {
                return Err((b"DivisionByZeroError", "Modulo by zero".to_string()));
            };
            r.rm_trailing_zeros();
            r
        }
        Op::Pow => {
            let prefixed = |what: &str| -> String {
                match arg_prefix {
                    Some(p) => format!("{} {}", p, what),
                    None => what.to_string(),
                }
            };
            if n2.scale != 0 {
                return Err((
                    b"ValueError",
                    prefixed("exponent cannot have a fractional part"),
                ));
            }
            let exponent = num::num2long(n2);
            let mut scale_expand = false;
            if auto_scale {
                if exponent > 0 {
                    let s = (n1_full_scale as u128) * (exponent as u128);
                    if s > INT_MAX as u128 {
                        return Err(too_large());
                    }
                    scale = s as usize;
                } else if exponent < 0 {
                    scale = n1_full_scale + EXPAND_SCALE;
                    if scale as i64 > INT_MAX {
                        return Err(too_large());
                    }
                    scale_expand = true;
                } else {
                    scale = 0;
                }
            }
            if exponent == 0 && (n2.len > 1 || n2.v[0] != 0) {
                return Err((b"ValueError", prefixed("exponent is too large")));
            }
            let mut r = match num::raise(n1, exponent, scale) {
                Ok(r) => r,
                Err(num::RaiseError::DivideByZero) => {
                    return Err((b"DivisionByZeroError", "Negative power of zero".to_string()));
                }
                Err(_) => {
                    return Err((
                        b"ValueError",
                        prefixed("exponent is too large, the number of digits overflowed"),
                    ));
                }
            };
            r.rm_trailing_zeros();
            if scale_expand {
                let diff = scale - r.scale;
                scale -= diff.min(EXPAND_SCALE);
            }
            r
        }
    };
    Ok((ret, scale))
}

/// bcmath_number_do_operation()
fn do_operation(vm: &mut Vm, opstr: &str, a: &Value, b: &Value) -> Option<Result<Value, String>> {
    if !is_number(a) && !is_number(b) {
        return PREV_DO_OPERATION
            .with(|p| p.get())
            .and_then(|f| f(vm, opstr, a, b));
    }
    let op = match opstr {
        "+" => Op::Add,
        "-" => Op::Sub,
        "*" => Op::Mul,
        "/" => Op::Div,
        "%" => Op::Mod,
        "**" => Op::Pow,
        _ => return None,
    };
    let unsupported = || {
        Some(Err(format!(
            "Unsupported operand types: {} {} {}",
            type_name(a),
            opstr,
            type_name(b)
        )))
    };
    // Both operands are parsed (deprecations included) before either is
    // converted.
    let pa = operand_kind(a);
    let pb = operand_kind(b);
    if !pa || !pb {
        return unsupported();
    }
    let ra = operand(vm, a);
    let rb = operand(vm, b);
    let (n1, s1) = match ra {
        Ok(v) => v,
        Err(true) => return unsupported(),
        Err(false) => {
            return Some(Err(
                "\x01V\x01Left string operand cannot be converted to BcMath\\Number".to_string(),
            ));
        }
    };
    let (n2, s2) = match rb {
        Ok(v) => v,
        Err(true) => return unsupported(),
        Err(false) => {
            return Some(Err(
                "\x01V\x01Right string operand cannot be converted to BcMath\\Number".to_string(),
            ));
        }
    };
    if s1 as i64 > INT_MAX || s2 as i64 > INT_MAX {
        return Some(Err(format!(
            "\x01V\x01scale must be between 0 and {}",
            INT_MAX
        )));
    }
    match calc(op, &n1, &n2, s1, s2, 0, true, None) {
        Ok((r, scale)) => Some(Ok(new_number(vm, &r, scale))),
        Err((class, msg)) => {
            let tag = match class {
                b"DivisionByZeroError" => "\x01D\x01",
                _ => "\x01V\x01",
            };
            Some(Err(format!("{}{}", tag, msg)))
        }
    }
}

/// Whether bcmath_number_parse_num() accepts the type of `v`.
fn operand_kind(v: &Value) -> bool {
    match v.deref() {
        Value::Object(_) => is_number(v),
        Value::Long(_) | Value::String(_) | Value::True | Value::False => true,
        Value::Double(d) => !d.is_nan() && goro_core::value::php_double_fits_long(d),
        _ => false,
    }
}

/// bcmath_number_compare(): -1 / 0 / 1, or 2 (ZEND_UNCOMPARABLE).
fn compare_handler(a: &Value, b: &Value) -> Option<i64> {
    let parse = |v: &Value| -> Option<Num> {
        match v.deref() {
            Value::Object(o) if is_number(v) => Some(obj_num(&o).0),
            Value::Long(l) => Some(num::long2num(l)),
            Value::String(s) => num::str2num(s.as_bytes(), 0, true).0,
            Value::Double(d) if !d.is_nan() && goro_core::value::php_double_fits_long(d) => {
                Some(num::long2num(d as i64))
            }
            _ => None,
        }
    };
    if !is_number(a) && !is_number(b) {
        return None;
    }
    // zend_compare(): null compares as "less", bools through the bool
    // cast, before the compare handler runs.
    for v in [a, b] {
        if matches!(
            v.deref(),
            Value::Null | Value::Undef | Value::True | Value::False
        ) {
            return None;
        }
    }
    let (Some(n1), Some(n2)) = (parse(a), parse(b)) else {
        return Some(2);
    };
    Some(num::compare(&n1, &n2, n1.scale.max(n2.scale)) as i64)
}

// ---------------------------------------------------------------------------
// methods

/// The call arguments (the variadic `$args` of the PHP method): positional
/// ones first, named ones placed by parameter name. Err: the message of
/// the Error to throw.
fn margs(m: &Method, args: &[Value]) -> Result<Vec<Value>, String> {
    let mut list: Vec<Option<Value>> = Vec::new();
    let Some(Value::Array(a)) = args.get(2).map(|v| v.deref()) else {
        return Ok(Vec::new());
    };
    for (k, v) in a.borrow().iter() {
        match k {
            ArrayKey::Int(_) => list.push(Some(v.deref())),
            ArrayKey::String(name) => {
                let Some(pos) = m
                    .params
                    .iter()
                    .position(|p| p.as_bytes() == name.as_bytes())
                else {
                    return Err(format!(
                        "Unknown named parameter ${}",
                        name.to_string_lossy()
                    ));
                };
                if list.len() <= pos {
                    list.resize(pos + 1, None);
                } else if list[pos].is_some() {
                    return Err(format!(
                        "Named parameter ${} overwrites previous argument",
                        name.to_string_lossy()
                    ));
                }
                list[pos] = Some(v.deref());
            }
        }
    }
    // A skipped optional parameter takes its default; a skipped required
    // one is an error.
    let mut out = Vec::with_capacity(list.len());
    for (i, v) in list.into_iter().enumerate() {
        match v {
            Some(v) => out.push(v),
            None if i < m.required => {
                return Err(format!(
                    "{}::{}(): Argument #{} (${}) not passed",
                    CLASS_DISPLAY,
                    m.name,
                    i + 1,
                    m.params[i]
                ));
            }
            None => out.push(match (m.name, i) {
                ("round", 0) => Value::Long(0),
                ("round", 1) => Value::Undef,
                _ => Value::Null,
            }),
        }
    }
    Ok(out)
}

struct Method {
    name: &'static str,
    params: &'static [&'static str],
    required: usize,
}

fn method_info(name: &str) -> Option<Method> {
    let (n, params, required): (&'static str, &'static [&'static str], usize) = match name {
        "__construct" => ("__construct", &["num"], 1),
        "add" => ("add", &["num", "scale"], 1),
        "sub" => ("sub", &["num", "scale"], 1),
        "mul" => ("mul", &["num", "scale"], 1),
        "div" => ("div", &["num", "scale"], 1),
        "mod" => ("mod", &["num", "scale"], 1),
        "divmod" => ("divmod", &["num", "scale"], 1),
        "powmod" => ("powmod", &["exponent", "modulus", "scale"], 2),
        "pow" => ("pow", &["exponent", "scale"], 1),
        "sqrt" => ("sqrt", &["scale"], 0),
        "floor" => ("floor", &[], 0),
        "ceil" => ("ceil", &[], 0),
        "round" => ("round", &["precision", "mode"], 0),
        "compare" => ("compare", &["num", "scale"], 1),
        "__toString" => ("__toString", &[], 0),
        "__serialize" => ("__serialize", &[], 0),
        "__unserialize" => ("__unserialize", &["data"], 1),
        _ => return None,
    };
    Some(Method {
        name: n,
        params,
        required,
    })
}

impl Method {
    fn prefix(&self, i: usize) -> String {
        format!(
            "{}::{}(): Argument #{} (${})",
            CLASS_DISPLAY,
            self.name,
            i + 1,
            self.params.get(i).copied().unwrap_or("")
        )
    }
}

/// `__bcmath_number(method, $this, func_get_args())`: the native bodies of
/// the BcMath\Number methods.
fn number_method(vm: &mut Vm, args: &[Value]) -> R {
    let mname = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let Some(m) = method_info(&mname) else {
        return Ok(Value::Null);
    };
    let Some(Value::Object(obj)) = args.get(1).map(|v| v.deref()) else {
        return Ok(Value::Null);
    };
    let list = match margs(&m, args) {
        Ok(l) => l,
        Err(msg) => {
            let class: &[u8] = if msg.ends_with("not passed") {
                b"ArgumentCountError"
            } else {
                b"Error"
            };
            return throw(vm, class, &msg);
        }
    };
    // Argument count (zend_wrong_parameters_count_error)
    let given = list.len();
    let max = m.params.len();
    if given < m.required || given > max {
        let (kind, n) = if m.required == max {
            ("exactly", max)
        } else if given < m.required {
            ("at least", m.required)
        } else {
            ("at most", max)
        };
        let msg = format!(
            "{}::{}() expects {} {} argument{}, {} given",
            CLASS_DISPLAY,
            m.name,
            kind,
            n,
            if n == 1 { "" } else { "s" },
            given
        );
        return throw(vm, b"ArgumentCountError", &msg);
    }
    match m.name {
        "__construct" => construct(vm, &m, &obj, &list),
        "add" => calc_method(vm, &m, &obj, &list, Op::Add),
        "sub" => calc_method(vm, &m, &obj, &list, Op::Sub),
        "mul" => calc_method(vm, &m, &obj, &list, Op::Mul),
        "div" => calc_method(vm, &m, &obj, &list, Op::Div),
        "mod" => calc_method(vm, &m, &obj, &list, Op::Mod),
        "pow" => calc_method(vm, &m, &obj, &list, Op::Pow),
        "divmod" => divmod_method(vm, &m, &obj, &list),
        "powmod" => powmod_method(vm, &m, &obj, &list),
        "sqrt" => sqrt_method(vm, &m, &obj, &list),
        "floor" | "ceil" => {
            let (n, _) = obj_num(&obj);
            let r = num::floor_or_ceil(&n, m.name == "floor");
            Ok(new_number(vm, &r, 0))
        }
        "round" => round_method(vm, &m, &obj, &list),
        "compare" => compare_method(vm, &m, &obj, &list),
        "__toString" => Ok(obj.borrow().get_property(b"value")),
        "__serialize" => {
            let mut arr = PhpArray::new();
            arr.set(
                ArrayKey::String(PhpString::from_bytes(b"value")),
                obj.borrow().get_property(b"value"),
            );
            Ok(Value::Array(Rc::new(RefCell::new(arr))))
        }
        "__unserialize" => unserialize(vm, &m, &obj, &list),
        _ => Ok(Value::Null),
    }
}

fn strict(vm: &Vm) -> bool {
    let _ = vm;
    false
}

/// Z_PARAM_STR_OR_LONG / BCMATH_PARAM_NUMBER_OR_STR_OR_LONG for argument
/// `i`, then bc_num_from_obj_or_str_or_long_with_err(). Ok(None): an
/// exception was thrown.
fn num_arg(
    vm: &mut Vm,
    m: &Method,
    list: &[Value],
    i: usize,
    allow_number: bool,
) -> Result<Option<(Num, usize)>, VmError> {
    use goro_core::arginfo::{T_INT, T_STRING};
    let v = list.get(i).cloned().unwrap_or(Value::Null);
    if allow_number {
        if let Value::Object(o) = &v {
            if is_number(&v) {
                return Ok(Some(obj_num(o)));
            }
        }
    }
    let line = vm.current_line;
    let coerced = if matches!(v, Value::Null | Value::Undef) {
        let ty = if allow_number {
            "BcMath\\Number|string|int"
        } else {
            "string|int"
        };
        let msg = format!(
            "{}::{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
            CLASS_DISPLAY,
            m.name,
            i + 1,
            m.params[i],
            ty
        );
        vm.emit_deprecated_at(&msg, line);
        if vm.current_exception.is_some() {
            return Ok(None);
        }
        Some(Value::Long(0))
    } else {
        match vm.zpp_coerce_scalar(T_INT | T_STRING, &v, strict(vm), line) {
            Ok(c) => c,
            Err(()) => return Ok(None),
        }
    };
    let Some(c) = coerced else {
        let msg = if allow_number {
            format!(
                "{} must be of type int, string, or {}, {} given",
                m.prefix(i),
                CLASS_DISPLAY,
                Vm::value_type_name(&v)
            )
        } else {
            format!(
                "{} must be of type string|int, {} given",
                m.prefix(i),
                Vm::value_type_name(&v)
            )
        };
        throw(vm, b"TypeError", &msg)?;
        return Ok(None);
    };
    if vm.current_exception.is_some() {
        return Ok(None);
    }
    match c {
        Value::Long(l) => Ok(Some((num::long2num(l), 0))),
        other => {
            let s = other.to_php_string();
            match num::str2num(s.as_bytes(), 0, true) {
                (Some(n), fs) => {
                    if fs as i64 > INT_MAX {
                        let msg = format!("{} must be between 0 and {}", m.prefix(i), INT_MAX);
                        throw(vm, b"ValueError", &msg)?;
                        return Ok(None);
                    }
                    Ok(Some((n, fs)))
                }
                _ => {
                    let msg = format!("{} is not well-formed", m.prefix(i));
                    throw(vm, b"ValueError", &msg)?;
                    Ok(None)
                }
            }
        }
    }
}

/// Z_PARAM_LONG_OR_NULL for argument `i`: Ok(Some(None)) for null.
fn long_or_null_arg(
    vm: &mut Vm,
    m: &Method,
    list: &[Value],
    i: usize,
) -> Result<Option<Option<i64>>, VmError> {
    use goro_core::arginfo::{T_INT, T_NULL};
    let v = match list.get(i) {
        None | Some(Value::Null) | Some(Value::Undef) => return Ok(Some(None)),
        Some(v) => v.clone(),
    };
    let line = vm.current_line;
    match vm.zpp_coerce_scalar(T_INT | T_NULL, &v, strict(vm), line) {
        Ok(Some(c)) => {
            if vm.current_exception.is_some() {
                return Ok(None);
            }
            Ok(Some(Some(c.to_long())))
        }
        Ok(None) => {
            let msg = format!(
                "{} must be of type ?int, {} given",
                m.prefix(i),
                Vm::value_type_name(&v)
            );
            throw(vm, b"TypeError", &msg)?;
            Ok(None)
        }
        Err(()) => Ok(None),
    }
}

/// bcmath_check_scale()
fn check_scale(vm: &mut Vm, m: &Method, i: usize, s: i64) -> Result<bool, VmError> {
    if !(0..=INT_MAX).contains(&s) {
        let msg = format!("{} must be between 0 and {}", m.prefix(i), INT_MAX);
        throw(vm, b"ValueError", &msg)?;
        return Ok(false);
    }
    Ok(true)
}

macro_rules! tri {
    ($e:expr) => {
        match $e? {
            Some(v) => v,
            None => return Ok(Value::Null),
        }
    };
}

fn readonly_error(vm: &mut Vm) -> R {
    throw(
        vm,
        b"Error",
        &format!("Cannot modify readonly property {}::$value", CLASS_DISPLAY),
    )
}

fn construct(vm: &mut Vm, m: &Method, obj: &Rc<RefCell<PhpObject>>, list: &[Value]) -> R {
    use goro_core::arginfo::{T_INT, T_STRING};
    let v = list[0].clone();
    // Z_PARAM_STR_OR_LONG
    let line = vm.current_line;
    let c = if matches!(v, Value::Null | Value::Undef) {
        let msg = format!(
            "{}::__construct(): Passing null to parameter #1 ($num) of type string|int is deprecated",
            CLASS_DISPLAY
        );
        vm.emit_deprecated_at(&msg, line);
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
        Value::Long(0)
    } else {
        match vm.zpp_coerce_scalar(T_INT | T_STRING, &v, strict(vm), line) {
            Ok(Some(c)) => c,
            Ok(None) => {
                let msg = format!(
                    "{} must be of type string|int, {} given",
                    m.prefix(0),
                    Vm::value_type_name(&v)
                );
                return throw(vm, b"TypeError", &msg);
            }
            Err(()) => return Ok(Value::Null),
        }
    };
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    if !matches!(obj.borrow().get_property(b"value"), Value::Undef) {
        return readonly_error(vm);
    }
    let (n, scale) = match c {
        Value::Long(l) => (num::long2num(l), 0),
        other => {
            let s = other.to_php_string();
            match num::str2num(s.as_bytes(), 0, true) {
                (Some(n), fs) if fs as i64 <= INT_MAX => (n, fs),
                (Some(_), _) => {
                    let msg = format!("{} must be between 0 and {}", m.prefix(0), INT_MAX);
                    return throw(vm, b"ValueError", &msg);
                }
                _ => {
                    let msg = format!("{} is not well-formed", m.prefix(0));
                    return throw(vm, b"ValueError", &msg);
                }
            }
        }
    };
    set_props(obj, &n, scale);
    attach(obj);
    Ok(Value::Null)
}

fn calc_method(vm: &mut Vm, m: &Method, obj: &Rc<RefCell<PhpObject>>, list: &[Value], op: Op) -> R {
    let (n2, s2) = tri!(num_arg(vm, m, list, 0, true));
    let scale = tri!(long_or_null_arg(vm, m, list, 1));
    if !check_scale(vm, m, 1, scale.unwrap_or(0))? {
        return Ok(Value::Null);
    }
    let (n1, s1) = obj_num(obj);
    let prefix = m.prefix(0);
    match calc(
        op,
        &n1,
        &n2,
        s1,
        s2,
        scale.unwrap_or(0) as usize,
        scale.is_none(),
        Some(&prefix),
    ) {
        Ok((r, scale)) => Ok(new_number(vm, &r, scale)),
        Err((class, msg)) => throw(vm, class, &msg),
    }
}

fn divmod_method(vm: &mut Vm, m: &Method, obj: &Rc<RefCell<PhpObject>>, list: &[Value]) -> R {
    let (n2, s2) = tri!(num_arg(vm, m, list, 0, true));
    let scale = tri!(long_or_null_arg(vm, m, list, 1));
    if !check_scale(vm, m, 1, scale.unwrap_or(0))? {
        return Ok(Value::Null);
    }
    let (n1, s1) = obj_num(obj);
    let scale = match scale {
        Some(s) => s as usize,
        None => s1.max(s2),
    };
    let Some((mut q, mut r)) = num::divmod(&n1, &n2, scale) else {
        return throw(vm, b"DivisionByZeroError", "Division by zero");
    };
    q.rm_trailing_zeros();
    r.rm_trailing_zeros();
    let qv = new_number(vm, &q, 0);
    let rv = new_number(vm, &r, scale);
    let mut arr = PhpArray::new();
    arr.push(qv);
    arr.push(rv);
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn powmod_method(vm: &mut Vm, m: &Method, obj: &Rc<RefCell<PhpObject>>, list: &[Value]) -> R {
    let (e, _) = tri!(num_arg(vm, m, list, 0, true));
    let (md, _) = tri!(num_arg(vm, m, list, 1, true));
    let scale = tri!(long_or_null_arg(vm, m, list, 2));
    let scale = scale.unwrap_or(0);
    if !check_scale(vm, m, 2, scale)? {
        return Ok(Value::Null);
    }
    let scale = scale as usize;
    let (base, _) = obj_num(obj);
    use num::RaiseModError::*;
    match num::raisemod(&base, &e, &md, scale) {
        Ok(mut r) => {
            r.rm_trailing_zeros();
            Ok(new_number(vm, &r, scale))
        }
        Err(BaseHasFractional) => throw(
            vm,
            b"ValueError",
            "Base number cannot have a fractional part",
        ),
        Err(ExpoHasFractional) => throw(
            vm,
            b"ValueError",
            &format!("{} cannot have a fractional part", m.prefix(0)),
        ),
        Err(ExpoIsNegative) => throw(
            vm,
            b"ValueError",
            &format!("{} must be greater than or equal to 0", m.prefix(0)),
        ),
        Err(ModHasFractional) => throw(
            vm,
            b"ValueError",
            &format!("{} cannot have a fractional part", m.prefix(1)),
        ),
        Err(ModIsZero) => throw(vm, b"DivisionByZeroError", "Modulo by zero"),
    }
}

fn sqrt_method(vm: &mut Vm, m: &Method, obj: &Rc<RefCell<PhpObject>>, list: &[Value]) -> R {
    let scale_arg = tri!(long_or_null_arg(vm, m, list, 0));
    if !check_scale(vm, m, 0, scale_arg.unwrap_or(0))? {
        return Ok(Value::Null);
    }
    let (n, s) = obj_num(obj);
    let mut scale = match scale_arg {
        None => {
            let sc = s + EXPAND_SCALE;
            if sc as i64 > INT_MAX {
                return throw(vm, b"ValueError", "scale of the result is too large");
            }
            sc
        }
        Some(v) => v as usize,
    };
    let Some(mut r) = num::sqrt(&n, scale) else {
        return throw(
            vm,
            b"ValueError",
            "Base number must be greater than or equal to 0",
        );
    };
    r.trunc_scale(scale);
    r.rm_trailing_zeros();
    if scale_arg.is_none() {
        let diff = scale - r.scale;
        scale -= diff.min(EXPAND_SCALE);
    }
    Ok(new_number(vm, &r, scale))
}

fn round_method(vm: &mut Vm, m: &Method, obj: &Rc<RefCell<PhpObject>>, list: &[Value]) -> R {
    use goro_core::arginfo::T_INT;
    let precision = match list.first() {
        None => 0,
        Some(v) => {
            let line = vm.current_line;
            if matches!(v, Value::Null) {
                let msg = format!(
                    "{}::round(): Passing null to parameter #1 ($precision) of type int is deprecated",
                    CLASS_DISPLAY
                );
                vm.emit_deprecated_at(&msg, line);
                if vm.current_exception.is_some() {
                    return Ok(Value::Null);
                }
                0
            } else {
                match vm.zpp_coerce_scalar(T_INT, v, strict(vm), line) {
                    Ok(Some(c)) => c.to_long(),
                    Ok(None) => {
                        let msg = format!(
                            "{} must be of type int, {} given",
                            m.prefix(0),
                            Vm::value_type_name(v)
                        );
                        return throw(vm, b"TypeError", &msg);
                    }
                    Err(()) => return Ok(Value::Null),
                }
            }
        }
    };
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    let mode = match list.get(1) {
        None | Some(Value::Undef) => num::ROUND_HALF_UP,
        Some(v) => match crate::rounding_mode(v) {
            Ok(mo) => mo,
            Err(given) => {
                let msg = format!(
                    "{} must be of type RoundingMode, {} given",
                    m.prefix(1),
                    given
                );
                return throw(vm, b"TypeError", &msg);
            }
        },
    };
    if !(1..=8).contains(&mode) {
        return throw(
            vm,
            b"ValueError",
            &format!("{} is an unsupported rounding mode", m.prefix(1)),
        );
    }
    let (n, _) = obj_num(obj);
    let (mut r, scale) = num::round(&n, precision, mode);
    r.rm_trailing_zeros();
    Ok(new_number(vm, &r, scale))
}

fn compare_method(vm: &mut Vm, m: &Method, obj: &Rc<RefCell<PhpObject>>, list: &[Value]) -> R {
    let (n2, _) = tri!(num_arg(vm, m, list, 0, true));
    let scale = tri!(long_or_null_arg(vm, m, list, 1));
    if !check_scale(vm, m, 1, scale.unwrap_or(0))? {
        return Ok(Value::Null);
    }
    let (n1, _) = obj_num(obj);
    let scale = match scale {
        Some(s) => s as usize,
        None => n1.scale.max(n2.scale),
    };
    Ok(Value::Long(num::compare(&n1, &n2, scale) as i64))
}

fn unserialize(vm: &mut Vm, m: &Method, obj: &Rc<RefCell<PhpObject>>, list: &[Value]) -> R {
    let Value::Array(arr) = list[0].clone() else {
        let msg = format!(
            "{} must be of type array, {} given",
            m.prefix(0),
            Vm::value_type_name(&list[0])
        );
        return throw(vm, b"TypeError", &msg);
    };
    let fail = |vm: &mut Vm| {
        throw(
            vm,
            b"Exception",
            &format!("Invalid serialization data for {} object", CLASS_DISPLAY),
        )
    };
    let zv = arr
        .borrow()
        .get(&ArrayKey::String(PhpString::from_bytes(b"value")))
        .map(|v| v.deref());
    let s = match zv {
        Some(Value::String(s)) if !s.as_bytes().is_empty() => s,
        _ => return fail(vm),
    };
    if !matches!(obj.borrow().get_property(b"value"), Value::Undef) {
        return readonly_error(vm);
    }
    match num::str2num(s.as_bytes(), 0, true) {
        (Some(n), fs) if fs as i64 <= INT_MAX => {
            set_props(obj, &n, fs);
            attach(obj);
            Ok(Value::Null)
        }
        _ => fail(vm),
    }
}
