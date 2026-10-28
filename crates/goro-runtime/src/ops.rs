//! Arithmetic, bitwise, string and increment/decrement operators (PHP
//! manual: Arithmetic Operators, Bitwise Operators, String Operators,
//! Incrementing/Decrementing Operators; RFCs "Saner numeric strings",
//! "Arithmetic/bitwise operators more strictly", "Path to Saner
//! Increment/Decrement operators").
//!
//! Every operator takes dereferenced or reference values, reports
//! diagnostics through the [`Ctx`] and returns `Err` for what PHP throws.

use crate::array::PhpArray;
use crate::convert::{
    append_str, float_repr, float_to_int_implicit, str_to_number_operand, to_bool, to_str,
};
use crate::ctx::Ctx;
use crate::error::{ErrorKind, Level, PhpError, PhpResult};
use crate::numeric::{Number, Numericness, parse_numeric};
use crate::string::PhpStr;
use crate::value::Value;

/// Binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

impl BinOp {
    /// The operator as written in PHP (used in error messages).
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Pow => "**",
            BinOp::Concat => ".",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
        }
    }
}

/// Applies a binary operator.
pub fn binary_op(op: BinOp, a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    match op {
        BinOp::Add => add(a, b, ctx),
        BinOp::Sub => sub(a, b, ctx),
        BinOp::Mul => mul(a, b, ctx),
        BinOp::Div => div(a, b, ctx),
        BinOp::Mod => modulo(a, b, ctx),
        BinOp::Pow => pow(a, b, ctx),
        BinOp::Concat => concat(a, b, ctx).map(Value::Str),
        BinOp::BitAnd => bit_and(a, b, ctx),
        BinOp::BitOr => bit_or(a, b, ctx),
        BinOp::BitXor => bit_xor(a, b, ctx),
        BinOp::Shl => shl(a, b, ctx),
        BinOp::Shr => shr(a, b, ctx),
    }
}

fn unsupported(op: BinOp, a: &Value, b: &Value) -> PhpError {
    PhpError::type_error(format!(
        "Unsupported operand types: {} {} {}",
        a.type_name_for_error(),
        op.symbol(),
        b.type_name_for_error()
    ))
}

/// Dereferences both operands (cheap clones) when either is a reference.
macro_rules! deref2 {
    ($f:ident, $a:ident, $b:ident, $ctx:ident) => {
        if $a.is_ref() || $b.is_ref() {
            let (x, y) = ($a.deref(), $b.deref());
            return $f(&x, &y, $ctx);
        }
    };
}

fn overloaded(op: BinOp, a: &Value, b: &Value, ctx: &mut dyn Ctx) -> Option<PhpResult<Value>> {
    if matches!(a, Value::Object(_)) || matches!(b, Value::Object(_)) {
        ctx.object_binop(op, a, b)
    } else {
        None
    }
}

/// Numeric value of an arithmetic operand; `None` for unsupported types.
fn num_operand(v: &Value, ctx: &mut dyn Ctx) -> Option<Number> {
    match v {
        Value::Int(i) => Some(Number::Int(*i)),
        Value::Float(f) => Some(Number::Float(*f)),
        Value::Null => Some(Number::Int(0)),
        Value::Bool(b) => Some(Number::Int(*b as i64)),
        Value::Str(s) => str_to_number_operand(s, ctx).map(|(n, _)| n),
        _ => None,
    }
}

fn num_operands(op: BinOp, a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<(Number, Number)> {
    let x = num_operand(a, ctx).ok_or_else(|| unsupported(op, a, b))?;
    let y = num_operand(b, ctx).ok_or_else(|| unsupported(op, a, b))?;
    Ok((x, y))
}

#[inline]
fn num_value(n: Number) -> Value {
    match n {
        Number::Int(i) => Value::Int(i),
        Number::Float(f) => Value::Float(f),
    }
}

macro_rules! overload_first {
    ($op:expr, $a:ident, $b:ident, $ctx:ident) => {
        if let Some(r) = overloaded($op, $a, $b, $ctx) {
            return r;
        }
    };
}

/// `$a + $b` (array union for two arrays).
pub fn add(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            return Ok(match x.checked_add(*y) {
                Some(r) => Value::Int(r),
                None => Value::Float(*x as f64 + *y as f64),
            });
        }
        (Value::Float(x), Value::Float(y)) => return Ok(Value::Float(x + y)),
        (Value::Int(x), Value::Float(y)) => return Ok(Value::Float(*x as f64 + y)),
        (Value::Float(x), Value::Int(y)) => return Ok(Value::Float(x + *y as f64)),
        (Value::Array(x), Value::Array(y)) => return Ok(Value::Array(array_union(x, y))),
        _ => {}
    }
    deref2!(add, a, b, ctx);
    overload_first!(BinOp::Add, a, b, ctx);
    let (x, y) = num_operands(BinOp::Add, a, b, ctx)?;
    Ok(match (x, y) {
        (Number::Int(x), Number::Int(y)) => match x.checked_add(y) {
            Some(r) => Value::Int(r),
            None => Value::Float(x as f64 + y as f64),
        },
        (x, y) => Value::Float(x.to_f64() + y.to_f64()),
    })
}

/// Array union: `$a + $b` keeps `$a`'s entries and adds `$b`'s missing keys.
pub fn array_union(a: &PhpArray, b: &PhpArray) -> PhpArray {
    if b.is_empty() {
        return a.clone();
    }
    if a.is_empty() {
        return b.clone();
    }
    let mut r = a.clone();
    for (k, v) in b.iter() {
        if r.get(k).is_none() {
            r.insert(k.to_owned(), v.clone());
        }
    }
    r
}

/// `$a - $b`.
pub fn sub(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            return Ok(match x.checked_sub(*y) {
                Some(r) => Value::Int(r),
                None => Value::Float(*x as f64 - *y as f64),
            });
        }
        (Value::Float(x), Value::Float(y)) => return Ok(Value::Float(x - y)),
        (Value::Int(x), Value::Float(y)) => return Ok(Value::Float(*x as f64 - y)),
        (Value::Float(x), Value::Int(y)) => return Ok(Value::Float(x - *y as f64)),
        _ => {}
    }
    deref2!(sub, a, b, ctx);
    overload_first!(BinOp::Sub, a, b, ctx);
    let (x, y) = num_operands(BinOp::Sub, a, b, ctx)?;
    Ok(match (x, y) {
        (Number::Int(x), Number::Int(y)) => match x.checked_sub(y) {
            Some(r) => Value::Int(r),
            None => Value::Float(x as f64 - y as f64),
        },
        (x, y) => Value::Float(x.to_f64() - y.to_f64()),
    })
}

/// `$a * $b`.
pub fn mul(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            return Ok(match x.checked_mul(*y) {
                Some(r) => Value::Int(r),
                None => Value::Float(*x as f64 * *y as f64),
            });
        }
        (Value::Float(x), Value::Float(y)) => return Ok(Value::Float(x * y)),
        (Value::Int(x), Value::Float(y)) => return Ok(Value::Float(*x as f64 * y)),
        (Value::Float(x), Value::Int(y)) => return Ok(Value::Float(x * *y as f64)),
        _ => {}
    }
    deref2!(mul, a, b, ctx);
    overload_first!(BinOp::Mul, a, b, ctx);
    let (x, y) = num_operands(BinOp::Mul, a, b, ctx)?;
    Ok(match (x, y) {
        (Number::Int(x), Number::Int(y)) => match x.checked_mul(y) {
            Some(r) => Value::Int(r),
            None => Value::Float(x as f64 * y as f64),
        },
        (x, y) => Value::Float(x.to_f64() * y.to_f64()),
    })
}

fn div_by_zero() -> PhpError {
    PhpError::new(ErrorKind::DivisionByZero, "Division by zero")
}

/// `$a / $b`: int when both are ints and the division is exact, float
/// otherwise; dividing by zero throws DivisionByZeroError.
pub fn div(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    deref2!(div, a, b, ctx);
    let (x, y) = match (a, b) {
        (Value::Int(x), Value::Int(y)) => (Number::Int(*x), Number::Int(*y)),
        (Value::Float(x), Value::Float(y)) => (Number::Float(*x), Number::Float(*y)),
        _ => {
            overload_first!(BinOp::Div, a, b, ctx);
            num_operands(BinOp::Div, a, b, ctx)?
        }
    };
    Ok(match (x, y) {
        (_, Number::Int(0)) => return Err(div_by_zero()),
        (_, Number::Float(0.0)) => return Err(div_by_zero()),
        (Number::Int(x), Number::Int(y)) => {
            if y == -1 && x == i64::MIN {
                Value::Float(x as f64 / -1.0)
            } else if x % y == 0 {
                Value::Int(x / y)
            } else {
                Value::Float(x as f64 / y as f64)
            }
        }
        (x, y) => Value::Float(x.to_f64() / y.to_f64()),
    })
}

/// Int value of an operand of `%`, the bitwise and the shift operators.
/// `None` for unsupported types.
fn int_operand(v: &Value, ctx: &mut dyn Ctx) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Null => Some(0),
        Value::Bool(b) => Some(*b as i64),
        Value::Float(f) => Some(float_to_int_implicit(*f, ctx)),
        Value::Str(s) => {
            let (n, _) = str_to_number_operand(s, ctx)?;
            Some(match n {
                Number::Int(i) => i,
                Number::Float(f) => float_string_to_int(s, f, ctx),
            })
        }
        _ => None,
    }
}

/// Implicit conversion of a float-like numeric string to int (observed on
/// PHP 8.5: `Implicit conversion from float-string "1.5" to int loses
/// precision`).
fn float_string_to_int(s: &[u8], f: f64, ctx: &mut dyn Ctx) -> i64 {
    if f.is_finite() && (-crate::convert::TWO63..crate::convert::TWO63).contains(&f) {
        let i = f as i64;
        if i as f64 != f {
            ctx.notice(
                Level::Deprecated,
                format!(
                    "Implicit conversion from float-string \"{}\" to int loses precision",
                    String::from_utf8_lossy(s)
                ),
            );
        }
        i
    } else {
        float_to_int_implicit(f, ctx)
    }
}

fn int_operands(op: BinOp, a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<(i64, i64)> {
    let x = int_operand(a, ctx).ok_or_else(|| unsupported(op, a, b))?;
    let y = int_operand(b, ctx).ok_or_else(|| unsupported(op, a, b))?;
    Ok((x, y))
}

/// `$a % $b`: integer remainder with the sign of `$a`; modulo by zero
/// throws DivisionByZeroError.
pub fn modulo(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    deref2!(modulo, a, b, ctx);
    let (x, y) = match (a, b) {
        (Value::Int(x), Value::Int(y)) => (*x, *y),
        _ => {
            overload_first!(BinOp::Mod, a, b, ctx);
            int_operands(BinOp::Mod, a, b, ctx)?
        }
    };
    if y == 0 {
        return Err(PhpError::new(ErrorKind::DivisionByZero, "Modulo by zero"));
    }
    if y == -1 {
        return Ok(Value::Int(0));
    }
    Ok(Value::Int(x % y))
}

/// `intdiv($a, $b)`.
pub fn intdiv(a: i64, b: i64) -> PhpResult<i64> {
    if b == 0 {
        return Err(div_by_zero());
    }
    if b == -1 && a == i64::MIN {
        return Err(PhpError::new(
            ErrorKind::ArithmeticError,
            "Division of PHP_INT_MIN by -1 is not an integer",
        ));
    }
    Ok(a / b)
}

/// `int ** int` for a non-negative exponent by square-and-multiply; on
/// overflow the remaining steps continue in floating point, which keeps the
/// sign of odd powers of negative bases (observed on PHP 8.5:
/// `PHP_INT_MIN ** PHP_INT_MAX` is `-INF`).
fn int_pow(base: i64, mut exp: i64) -> Number {
    let mut result: i64 = 1;
    let mut b = base;
    loop {
        if exp & 1 == 1 {
            match result.checked_mul(b) {
                Some(r) => result = r,
                None => return float_pow_rest(result as f64 * b as f64, b as f64, exp >> 1, true),
            }
        }
        exp >>= 1;
        if exp == 0 {
            return Number::Int(result);
        }
        match b.checked_mul(b) {
            Some(x) => b = x,
            None => return float_pow_rest(result as f64, b as f64 * b as f64, exp, false),
        }
    }
}

fn float_pow_rest(mut result: f64, mut b: f64, mut exp: i64, squared_next: bool) -> Number {
    if squared_next && exp != 0 {
        b *= b;
    }
    while exp != 0 {
        if exp & 1 == 1 {
            result *= b;
        }
        exp >>= 1;
        if exp != 0 {
            b *= b;
        }
    }
    Number::Float(result)
}

/// `$a ** $b`.
pub fn pow(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    deref2!(pow, a, b, ctx);
    let (x, y) = match (a, b) {
        (Value::Int(x), Value::Int(y)) => (Number::Int(*x), Number::Int(*y)),
        (Value::Float(x), Value::Float(y)) => (Number::Float(*x), Number::Float(*y)),
        _ => {
            overload_first!(BinOp::Pow, a, b, ctx);
            num_operands(BinOp::Pow, a, b, ctx)?
        }
    };
    // PHP 8.4: "Power of base 0 and negative exponent is deprecated"
    // (RFC "Raising zero to the power of negative number").
    if x.to_f64() == 0.0 && y.to_f64() < 0.0 {
        ctx.notice(
            Level::Deprecated,
            "Power of base 0 and negative exponent is deprecated".into(),
        );
    }
    Ok(match (x, y) {
        (Number::Int(x), Number::Int(y)) if y >= 0 => num_value(int_pow(x, y)),
        (x, y) => Value::Float(x.to_f64().powf(y.to_f64())),
    })
}

/// `$a . $b`.
pub fn concat(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<PhpStr> {
    if let (Value::Str(x), Value::Str(y)) = (a, b) {
        if y.is_empty() {
            return Ok(x.clone());
        }
        if x.is_empty() {
            return Ok(y.clone());
        }
        return Ok(PhpStr::concat(x, y));
    }
    let mut out = Vec::new();
    append_str(&mut out, a, ctx)?;
    append_str(&mut out, b, ctx)?;
    Ok(PhpStr::new(&out))
}

/// `$a .= $b` on a string slot: appends in place when the string is not
/// shared.
pub fn concat_assign(target: &mut PhpStr, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<()> {
    match b {
        Value::Str(s) => target.push_bytes(s),
        v => {
            let s = to_str(v, ctx)?;
            target.push_bytes(&s);
        }
    }
    Ok(())
}

fn bytewise(a: &[u8], b: &[u8], op: BinOp) -> PhpStr {
    match op {
        BinOp::BitOr => {
            let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
            let mut out = long.to_vec();
            for (o, s) in out.iter_mut().zip(short) {
                *o |= s;
            }
            PhpStr::new(&out)
        }
        BinOp::BitAnd => {
            let out: Vec<u8> = a.iter().zip(b).map(|(x, y)| x & y).collect();
            PhpStr::new(&out)
        }
        _ => {
            let out: Vec<u8> = a.iter().zip(b).map(|(x, y)| x ^ y).collect();
            PhpStr::new(&out)
        }
    }
}

fn bitwise(op: BinOp, a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    if a.is_ref() || b.is_ref() {
        let (x, y) = (a.deref(), b.deref());
        return bitwise(op, &x, &y, ctx);
    }
    if let (Value::Str(x), Value::Str(y)) = (a, b) {
        return Ok(Value::Str(bytewise(x, y, op)));
    }
    let (x, y) = match (a, b) {
        (Value::Int(x), Value::Int(y)) => (*x, *y),
        _ => {
            overload_first!(op, a, b, ctx);
            int_operands(op, a, b, ctx)?
        }
    };
    Ok(Value::Int(match op {
        BinOp::BitAnd => x & y,
        BinOp::BitOr => x | y,
        _ => x ^ y,
    }))
}

/// `$a & $b` (byte-wise for two strings).
pub fn bit_and(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    bitwise(BinOp::BitAnd, a, b, ctx)
}

/// `$a | $b` (byte-wise for two strings).
pub fn bit_or(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    bitwise(BinOp::BitOr, a, b, ctx)
}

/// `$a ^ $b` (byte-wise for two strings).
pub fn bit_xor(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    bitwise(BinOp::BitXor, a, b, ctx)
}

fn shift(op: BinOp, a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    if a.is_ref() || b.is_ref() {
        let (x, y) = (a.deref(), b.deref());
        return shift(op, &x, &y, ctx);
    }
    let (x, y) = match (a, b) {
        (Value::Int(x), Value::Int(y)) => (*x, *y),
        _ => {
            overload_first!(op, a, b, ctx);
            int_operands(op, a, b, ctx)?
        }
    };
    if y < 0 {
        return Err(PhpError::new(
            ErrorKind::ArithmeticError,
            "Bit shift by negative number",
        ));
    }
    Ok(Value::Int(if op == BinOp::Shl {
        if y >= 64 { 0 } else { x.wrapping_shl(y as u32) }
    } else if y >= 64 {
        if x < 0 { -1 } else { 0 }
    } else {
        x >> y
    }))
}

/// `$a << $b`.
pub fn shl(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    shift(BinOp::Shl, a, b, ctx)
}

/// `$a >> $b`.
pub fn shr(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    shift(BinOp::Shr, a, b, ctx)
}

/// Name of a value in "Cannot perform bitwise not on X" style messages.
fn value_name(v: &Value) -> String {
    match v {
        Value::Bool(true) => "true".into(),
        Value::Bool(false) => "false".into(),
        v => v.type_name_for_error(),
    }
}

/// `~$a`.
pub fn bit_not(a: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    Ok(match a {
        Value::Int(i) => Value::Int(!i),
        Value::Float(f) => Value::Int(!float_to_int_implicit(*f, ctx)),
        Value::Str(s) => {
            let out: Vec<u8> = s.iter().map(|c| !c).collect();
            Value::Str(PhpStr::new(&out))
        }
        Value::Ref(r) => {
            let v = r.get();
            return bit_not(&v, ctx);
        }
        v => {
            return Err(PhpError::type_error(format!(
                "Cannot perform bitwise not on {}",
                value_name(v)
            )));
        }
    })
}

/// `-$a` (PHP evaluates it as `$a * -1`).
pub fn neg(a: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    match a {
        Value::Int(i) => Ok(match i.checked_neg() {
            Some(r) => Value::Int(r),
            None => Value::Float(-(*i as f64)),
        }),
        Value::Float(f) => Ok(Value::Float(-f)),
        _ => mul(a, &Value::Int(-1), ctx),
    }
}

/// `+$a` (PHP evaluates it as `$a * 1`).
pub fn plus(a: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    match a {
        Value::Int(_) | Value::Float(_) => Ok(a.clone()),
        _ => mul(a, &Value::Int(1), ctx),
    }
}

/// `!$a`.
pub fn not(a: &Value, ctx: &mut dyn Ctx) -> bool {
    !to_bool(a, ctx)
}

/// `$a xor $b`.
pub fn xor(a: &Value, b: &Value, ctx: &mut dyn Ctx) -> bool {
    to_bool(a, ctx) ^ to_bool(b, ctx)
}

// ---------------------------------------------------------------- inc/dec

/// The alphanumeric string increment ("a" → "b", "Az" → "Ba", "zz" →
/// "aaa", "a9" → "b0"); characters other than ASCII letters and digits stop
/// the carry (PHP manual, Incrementing/Decrementing Operators; tests
/// Zend/tests/in-de-crement/string_increment_various.phpt).
pub fn str_increment_bytes(s: &[u8]) -> Vec<u8> {
    let mut out = s.to_vec();
    if out.is_empty() {
        return b"1".to_vec();
    }
    let mut i = out.len();
    loop {
        if i == 0 {
            // Carry out of the first character: prepend based on its kind.
            let first = out[0];
            let lead = match first {
                b'a' => b'a',
                b'A' => b'A',
                _ => b'1',
            };
            out.insert(0, lead);
            return out;
        }
        i -= 1;
        let c = out[i];
        match c {
            b'a'..=b'y' | b'A'..=b'Y' | b'0'..=b'8' => {
                out[i] = c + 1;
                return out;
            }
            b'z' => out[i] = b'a',
            b'Z' => out[i] = b'A',
            b'9' => out[i] = b'0',
            _ => return out,
        }
        if i > 0 && !out[i - 1].is_ascii_alphanumeric() {
            return out;
        }
    }
}

/// `++$a`: the new value.
pub fn increment(a: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    Ok(match a {
        Value::Int(i) => match i.checked_add(1) {
            Some(r) => Value::Int(r),
            None => Value::Float(*i as f64 + 1.0),
        },
        Value::Float(f) => Value::Float(f + 1.0),
        Value::Null => Value::Int(1),
        Value::Bool(_) => {
            ctx.notice(
                Level::Warning,
                "Increment on type bool has no effect, this will change in the next major version of PHP".into(),
            );
            a.clone()
        }
        Value::Str(s) => {
            let p = parse_numeric(s);
            if p.kind == Numericness::Numeric {
                return increment(&num_value(p.num), ctx);
            }
            ctx.notice(
                Level::Deprecated,
                "Increment on non-numeric string is deprecated, use str_increment() instead".into(),
            );
            Value::Str(PhpStr::new(&str_increment_bytes(s)))
        }
        Value::Ref(r) => {
            let v = r.get();
            return increment(&v, ctx);
        }
        v => {
            return Err(PhpError::type_error(format!(
                "Cannot increment {}",
                v.type_name_for_error()
            )));
        }
    })
}

/// `--$a`: the new value.
pub fn decrement(a: &Value, ctx: &mut dyn Ctx) -> PhpResult<Value> {
    Ok(match a {
        Value::Int(i) => match i.checked_sub(1) {
            Some(r) => Value::Int(r),
            None => Value::Float(*i as f64 - 1.0),
        },
        Value::Float(f) => Value::Float(f - 1.0),
        Value::Null => {
            ctx.notice(
                Level::Warning,
                "Decrement on type null has no effect, this will change in the next major version of PHP".into(),
            );
            Value::Null
        }
        Value::Bool(_) => {
            ctx.notice(
                Level::Warning,
                "Decrement on type bool has no effect, this will change in the next major version of PHP".into(),
            );
            a.clone()
        }
        Value::Str(s) => {
            if s.is_empty() {
                ctx.notice(
                    Level::Deprecated,
                    "Decrement on empty string is deprecated as non-numeric".into(),
                );
                return Ok(Value::Int(-1));
            }
            let p = parse_numeric(s);
            if p.kind == Numericness::Numeric {
                return decrement(&num_value(p.num), ctx);
            }
            ctx.notice(
                Level::Deprecated,
                "Decrement on non-numeric string has no effect and is deprecated".into(),
            );
            a.clone()
        }
        Value::Ref(r) => {
            let v = r.get();
            return decrement(&v, ctx);
        }
        v => {
            return Err(PhpError::type_error(format!(
                "Cannot decrement {}",
                v.type_name_for_error()
            )));
        }
    })
}

/// Formats a float as diagnostics do (shortest round-trip form).
pub fn float_for_message(f: f64) -> String {
    float_repr(f)
}
