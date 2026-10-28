//! `BcMath\Number` (PHP 8.4+; PHP manual: The BcMath\Number class; RFC
//! "Support object type in BCMath").
//!
//! A [`Number`] is an immutable value with a canonical string `value` and a
//! `scale`. When a method's `$scale` is null, the result scale follows the
//! RFC and PHP 8.5's observed behaviour:
//!
//! | operation      | result scale                                              |
//! |----------------|-----------------------------------------------------------|
//! | add, sub, mod  | `max(s1, s2)`                                             |
//! | mul            | `s1 + s2`                                                 |
//! | div            | computed at `s1 + 10`, trailing zeros trimmed down to `s1` |
//! | pow, `e >= 0`  | `s1 * e`                                                  |
//! | pow, `e < 0`   | as div                                                    |
//! | sqrt           | as div (`s1 + 10`, trimmed down to `s1`)                  |
//! | divmod         | quotient 0, remainder `max(s1, s2)`                       |
//! | powmod         | 0                                                         |
//! | round          | `max(precision, 0)`; floor and ceil: 0                    |
//!
//! (tests ext/bcmath/tests/number/methods/*.phpt and operators/*.phpt.)
//! An explicit `$scale` gives exactly that scale, without trimming.

use crate::dec::{self, Dec, Num, RoundingMode};
use crate::functions::{self, DIV_ZERO, ExpError, MAX_SCALE, MOD_ZERO, PowmodError, ordering_int};
use crate::{BcError, arith};
use std::cmp::Ordering;

/// The `BcMath\Number` class name.
pub const CLASS_NAME: &str = "BcMath\\Number";

/// Extra fractional digits of an inexact `div`, `pow` or `sqrt` result
/// (RFC "Support object type in BCMath": "scale + 10").
const EXPAND_SCALE: usize = 10;

/// Values up to this many bytes are stored inline (no allocation).
const INLINE: usize = 46;

/// A `Number`'s canonical string: inline when short, else on the heap.
#[derive(Clone)]
enum Bytes {
    Inline { len: u8, buf: [u8; INLINE] },
    Heap(Box<[u8]>),
}

impl Bytes {
    #[inline]
    fn new(s: &[u8]) -> Bytes {
        if s.len() <= INLINE {
            let mut buf = [0u8; INLINE];
            dec::copy_small(&mut buf[..s.len()], s);
            Bytes::Inline {
                len: s.len() as u8,
                buf,
            }
        } else {
            Bytes::Heap(s.into())
        }
    }

    #[inline]
    fn as_slice(&self) -> &[u8] {
        match self {
            Bytes::Inline { len, buf } => &buf[..*len as usize],
            Bytes::Heap(b) => b,
        }
    }
}

impl PartialEq for Bytes {
    fn eq(&self, other: &Bytes) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl Eq for Bytes {}
impl std::hash::Hash for Bytes {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.as_slice().hash(h)
    }
}
impl std::fmt::Debug for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", String::from_utf8_lossy(self.as_slice()))
    }
}

thread_local! {
    /// Where results are built before they are stored in a `Number`.
    static SCRATCH: std::cell::Cell<Vec<u8>> = const { std::cell::Cell::new(Vec::new()) };
}

/// Runs `f` with an empty scratch buffer.
#[inline]
fn with_scratch<R>(f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
    SCRATCH.with(|c| {
        let mut v = c.take();
        v.clear();
        let r = f(&mut v);
        c.set(v);
        r
    })
}

/// A `BcMath\Number` value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Number {
    /// Canonical form: optional `-`, the integer part (at least one digit,
    /// no leading zeros), and `.` with `scale` digits when `scale > 0`.
    value: Bytes,
    scale: u32,
}

/// An operand of a `BcMath\Number` method or operator
/// (`BcMath\Number|string|int`).
#[derive(Clone, Copy, Debug)]
pub enum NumberArg<'a> {
    Int(i64),
    Str(&'a [u8]),
    Number(&'a Number),
}

impl From<i64> for NumberArg<'_> {
    fn from(v: i64) -> Self {
        NumberArg::Int(v)
    }
}
impl<'a> From<&'a [u8]> for NumberArg<'a> {
    fn from(v: &'a [u8]) -> Self {
        NumberArg::Str(v)
    }
}
impl<'a> From<&'a str> for NumberArg<'a> {
    fn from(v: &'a str) -> Self {
        NumberArg::Str(v.as_bytes())
    }
}
impl<'a> From<&'a Number> for NumberArg<'a> {
    fn from(v: &'a Number) -> Self {
        NumberArg::Number(v)
    }
}

/// The arithmetic operators `BcMath\Number` overloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

/// An operand's digits: an int is formatted on the stack, a `Number` is
/// already canonical (no validation needed).
enum Holder<'a> {
    Str(&'a [u8]),
    Int([u8; 20], usize),
    Num(&'a Number),
}

impl Holder<'_> {
    #[inline]
    fn parse(&self) -> Option<Num<'_>> {
        match self {
            Holder::Str(s) => dec::parse(s),
            Holder::Int(buf, start) => dec::parse(&buf[*start..]),
            Holder::Num(n) => Some(n.num()),
        }
    }
}

#[inline]
fn holder(arg: NumberArg<'_>) -> Holder<'_> {
    match arg {
        NumberArg::Int(i) => {
            // Decimal digits right-aligned in the buffer, sign before them.
            let mut buf = [0u8; 20];
            let mut v = i.unsigned_abs();
            let mut k = buf.len();
            loop {
                k -= 1;
                buf[k] = b'0' + (v % 10) as u8;
                v /= 10;
                if v == 0 {
                    break;
                }
            }
            if i < 0 {
                k -= 1;
                buf[k] = b'-';
            }
            Holder::Int(buf, k)
        }
        NumberArg::Str(s) => Holder::Str(s),
        NumberArg::Number(n) => Holder::Num(n),
    }
}

/// Removes trailing fractional zeros of `d` while its scale exceeds `min`.
fn trim_to(d: &mut Dec, min: usize) {
    while d.scale > min && d.digits.last() == Some(&b'0') {
        d.digits.pop();
        d.scale -= 1;
    }
}

fn method_err(method: &str, arg: u32, name: &str, what: &str) -> BcError {
    BcError::value(format!(
        "{CLASS_NAME}::{method}(): Argument #{arg} (${name}) {what}"
    ))
}

impl Number {
    fn from_num(n: Num, scale: usize) -> Result<Number, BcError> {
        if scale as u64 > MAX_SCALE as u64 {
            return Err(BcError::out_of_memory(scale));
        }
        with_scratch(|buf| {
            functions::output_into(buf, n, scale)?;
            Ok(Number::from_slice(buf, scale))
        })
    }

    #[inline]
    fn from_slice(v: &[u8], scale: usize) -> Number {
        Number {
            value: Bytes::new(v),
            scale: scale as u32,
        }
    }

    fn from_dec(d: &Dec, scale: usize) -> Result<Number, BcError> {
        Number::from_num(d.view(), scale)
    }

    /// A formatted result at `scale` with trailing fractional zeros
    /// dropped while the scale exceeds `min`.
    fn trimmed_from(v: &mut Vec<u8>, scale: usize, min: usize) -> Number {
        let mut s = scale;
        while s > min && v.last() == Some(&b'0') {
            v.pop();
            s -= 1;
        }
        if s == 0 && v.last() == Some(&b'.') {
            v.pop();
        }
        Number::from_slice(v, s)
    }

    /// The number as written, its scale being the count of fractional
    /// digits (trailing zeros included).
    fn parsed(n: Num) -> Number {
        let scale = n.frac.len();
        with_scratch(|buf| {
            dec::write_num(buf, n, scale);
            Number::from_slice(buf, scale)
        })
    }

    /// Parses a well-formed string (`None` otherwise) the way the
    /// constructor does: `"-0.0"` is `0.0`, `".5"` is `0.5`, `""` is `0`
    /// (test ext/bcmath/tests/number/construct_64bit.phpt).
    pub fn parse(s: &[u8]) -> Option<Number> {
        dec::parse(s).map(Number::parsed)
    }

    /// The value of an int.
    pub fn from_i64(v: i64) -> Number {
        match holder(NumberArg::Int(v)) {
            Holder::Int(buf, k) => Number::from_slice(&buf[k..], 0),
            _ => unreachable!(),
        }
    }

    /// `new BcMath\Number(string|int $num)`.
    pub fn new(num: NumberArg) -> Result<Number, BcError> {
        match num {
            NumberArg::Int(i) => Ok(Number::from_i64(i)),
            NumberArg::Number(n) => Ok(n.clone()),
            NumberArg::Str(s) => Number::parse(s)
                .ok_or_else(|| method_err("__construct", 1, "num", "is not well-formed")),
        }
    }

    /// The `value` property (and `__toString()`).
    pub fn value(&self) -> &[u8] {
        self.value.as_slice()
    }

    /// The `value` property as text.
    pub fn as_str(&self) -> &str {
        // The canonical form is ASCII.
        std::str::from_utf8(self.value.as_slice()).unwrap_or("0")
    }

    /// The `scale` property.
    pub fn scale(&self) -> i64 {
        self.scale as i64
    }

    /// Truthiness for `(bool)` casts: whether the value is non-zero
    /// (test ext/bcmath/tests/number/cast.phpt).
    pub fn to_bool(&self) -> bool {
        !self.num().is_zero()
    }

    pub(crate) fn num(&self) -> Num<'_> {
        let v = self.value.as_slice();
        let (neg, rest) = match v.first() {
            Some(b'-') => (true, &v[1..]),
            _ => (false, v),
        };
        let s = self.scale as usize;
        let (int, frac) = if s > 0 {
            (&rest[..rest.len() - s - 1], &rest[rest.len() - s..])
        } else {
            (rest, &rest[rest.len()..])
        };
        let int = if int == b"0" { &int[1..] } else { int };
        Num { neg, int, frac }
    }

    /// `__serialize()`: the array `['value' => $this->value]`; this is the
    /// value to store under `"value"` (test number/serialize.phpt).
    pub fn serialize_value(&self) -> &[u8] {
        self.value.as_slice()
    }

    /// `__unserialize()` from the `"value"` entry, `None` when it is missing
    /// or not a string. Errors with PHP's `Exception` (test
    /// number/unserialize_error.phpt).
    pub fn unserialize(value: Option<&[u8]>) -> Result<Number, BcError> {
        value
            .filter(|v| !v.is_empty())
            .and_then(Number::parse)
            .ok_or_else(|| {
                BcError::exception(format!(
                    "Invalid serialization data for {CLASS_NAME} object"
                ))
            })
    }

    fn arg<'h>(method: &str, index: u32, name: &str, h: &'h Holder) -> Result<Num<'h>, BcError> {
        h.parse()
            .ok_or_else(|| method_err(method, index, name, "is not well-formed"))
    }

    fn scale_arg(method: &str, index: u32, scale: Option<i64>) -> Result<Option<usize>, BcError> {
        match scale {
            None => Ok(None),
            Some(s) if (0..=MAX_SCALE).contains(&s) => Ok(Some(s as usize)),
            Some(_) => Err(method_err(
                method,
                index,
                "scale",
                "must be between 0 and 2147483647",
            )),
        }
    }

    /// `add(BcMath\Number|string|int $num, ?int $scale = null): BcMath\Number`
    pub fn add(&self, num: NumberArg, scale: Option<i64>) -> Result<Number, BcError> {
        let h = holder(num);
        let b = Number::arg("add", 1, "num", &h)?;
        let scale = Number::scale_arg("add", 2, scale)?;
        calc(BinOp::Add, self.num(), b, scale)
    }

    /// `sub(BcMath\Number|string|int $num, ?int $scale = null): BcMath\Number`
    pub fn sub(&self, num: NumberArg, scale: Option<i64>) -> Result<Number, BcError> {
        let h = holder(num);
        let b = Number::arg("sub", 1, "num", &h)?;
        let scale = Number::scale_arg("sub", 2, scale)?;
        calc(BinOp::Sub, self.num(), b, scale)
    }

    /// `mul(BcMath\Number|string|int $num, ?int $scale = null): BcMath\Number`
    pub fn mul(&self, num: NumberArg, scale: Option<i64>) -> Result<Number, BcError> {
        let h = holder(num);
        let b = Number::arg("mul", 1, "num", &h)?;
        let scale = Number::scale_arg("mul", 2, scale)?;
        calc(BinOp::Mul, self.num(), b, scale)
    }

    /// `div(BcMath\Number|string|int $num, ?int $scale = null): BcMath\Number`
    pub fn div(&self, num: NumberArg, scale: Option<i64>) -> Result<Number, BcError> {
        let h = holder(num);
        let b = Number::arg("div", 1, "num", &h)?;
        let scale = Number::scale_arg("div", 2, scale)?;
        calc(BinOp::Div, self.num(), b, scale)
    }

    /// `mod(BcMath\Number|string|int $num, ?int $scale = null): BcMath\Number`
    pub fn modulo(&self, num: NumberArg, scale: Option<i64>) -> Result<Number, BcError> {
        let h = holder(num);
        let b = Number::arg("mod", 1, "num", &h)?;
        let scale = Number::scale_arg("mod", 2, scale)?;
        calc(BinOp::Mod, self.num(), b, scale)
    }

    /// `divmod(BcMath\Number|string|int $num, ?int $scale = null): array`
    /// as `[quotient, remainder]`.
    pub fn divmod(&self, num: NumberArg, scale: Option<i64>) -> Result<(Number, Number), BcError> {
        let h = holder(num);
        let b = Number::arg("divmod", 1, "num", &h)?;
        let scale = Number::scale_arg("divmod", 2, scale)?;
        let a = self.num();
        if b.is_zero() {
            return Err(BcError::division_by_zero(DIV_ZERO));
        }
        let (q, r) = arith::divmod(&a, &b);
        let rs = scale.unwrap_or(a.frac.len().max(b.frac.len()));
        Ok((Number::from_dec(&q, 0)?, Number::from_dec(&r, rs)?))
    }

    /// `pow(BcMath\Number|string|int $exponent, ?int $scale = null): BcMath\Number`
    pub fn pow(&self, exponent: NumberArg, scale: Option<i64>) -> Result<Number, BcError> {
        let h = holder(exponent);
        let e = Number::arg("pow", 1, "exponent", &h)?;
        let scale = Number::scale_arg("pow", 2, scale)?;
        let e = functions::exponent(&e).map_err(|err| {
            method_err(
                "pow",
                1,
                "exponent",
                match err {
                    ExpError::Fractional => "exponent cannot have a fractional part",
                    ExpError::TooLarge => "exponent is too large",
                },
            )
        })?;
        pow_impl(self.num(), e, scale)
    }

    /// `powmod(BcMath\Number|string|int $exponent, BcMath\Number|string|int $modulus, ?int $scale = null): BcMath\Number`
    pub fn powmod(
        &self,
        exponent: NumberArg,
        modulus: NumberArg,
        scale: Option<i64>,
    ) -> Result<Number, BcError> {
        const M: &str = "powmod";
        let he = holder(exponent);
        let e = Number::arg(M, 1, "exponent", &he)?;
        let hm = holder(modulus);
        let m = Number::arg(M, 2, "modulus", &hm)?;
        let scale = Number::scale_arg(M, 3, scale)?;
        let r = functions::powmod_checked(&self.num(), &e, &m, |which| match which {
            PowmodError::BaseFraction => {
                BcError::value("Base number cannot have a fractional part")
            }
            PowmodError::ExpFraction => {
                method_err(M, 1, "exponent", "cannot have a fractional part")
            }
            PowmodError::ExpNegative => {
                method_err(M, 1, "exponent", "must be greater than or equal to 0")
            }
            PowmodError::ModFraction => {
                method_err(M, 2, "modulus", "cannot have a fractional part")
            }
        })?;
        Number::from_dec(&r, scale.unwrap_or(0))
    }

    /// `sqrt(?int $scale = null): BcMath\Number`
    pub fn sqrt(&self, scale: Option<i64>) -> Result<Number, BcError> {
        let scale = Number::scale_arg("sqrt", 1, scale)?;
        let a = self.num();
        if a.neg && !a.is_zero() {
            return Err(BcError::value(
                "Base number must be greater than or equal to 0",
            ));
        }
        match scale {
            Some(s) => Number::from_dec(&arith::sqrt(&a, s), s),
            None => {
                let s = a.frac.len();
                let mut d = arith::sqrt(&a, s + EXPAND_SCALE);
                trim_to(&mut d, s);
                let sc = d.scale;
                Number::from_dec(&d, sc)
            }
        }
    }

    /// `floor(): BcMath\Number`
    pub fn floor(&self) -> Result<Number, BcError> {
        let d = functions::round_num(&self.num(), 0, RoundingMode::NegativeInfinity)?;
        Number::from_dec(&d, 0)
    }

    /// `ceil(): BcMath\Number`
    pub fn ceil(&self) -> Result<Number, BcError> {
        let d = functions::round_num(&self.num(), 0, RoundingMode::PositiveInfinity)?;
        Number::from_dec(&d, 0)
    }

    /// `round(int $precision = 0, RoundingMode $mode = RoundingMode::HalfAwayFromZero): BcMath\Number`
    pub fn round(&self, precision: i64, mode: RoundingMode) -> Result<Number, BcError> {
        let d = functions::round_num(&self.num(), precision, mode)?;
        Number::from_dec(&d, precision.max(0) as usize)
    }

    /// `compare(BcMath\Number|string|int $num, ?int $scale = null): int`
    pub fn compare(&self, num: NumberArg, scale: Option<i64>) -> Result<i64, BcError> {
        let h = holder(num);
        let b = Number::arg("compare", 1, "num", &h)?;
        let scale = Number::scale_arg("compare", 2, scale)?;
        let a = self.num();
        let o = match scale {
            None => dec::cmp(&a, &b),
            Some(s) => dec::cmp(&a.truncated(s), &b.truncated(s)),
        };
        Ok(ordering_int(o))
    }

    /// An arithmetic operator with a `BcMath\Number` on at least one side
    /// (the engine handles operand types other than int, string and
    /// `BcMath\Number`, after converting floats and bools to int as PHP
    /// does). A malformed string fails with "Left/Right string operand
    /// cannot be converted to BcMath\Number" (test
    /// number/operators/calc_non_numeric_string.phpt).
    pub fn binary_op(op: BinOp, lhs: NumberArg, rhs: NumberArg) -> Result<Number, BcError> {
        let hl = holder(lhs);
        let hr = holder(rhs);
        let a = hl.parse().ok_or_else(|| {
            BcError::value(format!(
                "Left string operand cannot be converted to {CLASS_NAME}"
            ))
        })?;
        let b = hr.parse().ok_or_else(|| {
            BcError::value(format!(
                "Right string operand cannot be converted to {CLASS_NAME}"
            ))
        })?;
        calc(op, a, b, None)
    }

    /// `<=>` between operands with a `BcMath\Number` on at least one side;
    /// `None` when a string operand is malformed (the values are then
    /// uncomparable: every ordered comparison is false; test
    /// number/operators/compare_with_invalid_types.phpt).
    pub fn compare_operands(lhs: NumberArg, rhs: NumberArg) -> Option<Ordering> {
        let hl = holder(lhs);
        let hr = holder(rhs);
        let a = hl.parse()?;
        let b = hr.parse()?;
        Some(dec::cmp(&a, &b))
    }

    /// Unary minus (`-$n`, which PHP evaluates as `$n * -1`).
    pub fn negate(&self) -> Number {
        let n = self.num();
        Number::parsed(Num { neg: !n.neg, ..n })
    }

    /// `++$n` (`$n + 1`, scale kept; test number/operators/increment.phpt).
    pub fn increment(&self) -> Result<Number, BcError> {
        Number::binary_op(BinOp::Add, NumberArg::Number(self), NumberArg::Int(1))
    }

    /// `--$n`.
    pub fn decrement(&self) -> Result<Number, BcError> {
        Number::binary_op(BinOp::Sub, NumberArg::Number(self), NumberArg::Int(1))
    }
}

impl std::fmt::Display for Number {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A two-operand calculation with `BcMath\Number` scale rules.
#[inline]
fn calc(op: BinOp, a: Num, b: Num, scale: Option<usize>) -> Result<Number, BcError> {
    let (sa, sb) = (a.frac.len(), b.frac.len());
    #[inline(always)]
    fn run(
        a: &Num,
        b: &Num,
        s: usize,
        f: impl FnOnce(&mut Vec<u8>, &Num, &Num, usize),
    ) -> Result<Number, BcError> {
        if s as u64 > MAX_SCALE as u64 {
            return Err(BcError::out_of_memory(s));
        }
        functions::check_len(functions::result_bound(a, b, s))?;
        with_scratch(|buf| {
            f(buf, a, b, s);
            Ok(Number::from_slice(buf, s))
        })
    }
    let direct = |f: fn(&mut Vec<u8>, &Num, &Num, usize), s: usize| run(&a, &b, s, f);
    match op {
        BinOp::Add => run(&a, &b, scale.unwrap_or(sa.max(sb)), |o, x, y, s| {
            dec::add_into(o, x, y, s)
        }),
        BinOp::Sub => run(&a, &b, scale.unwrap_or(sa.max(sb)), |o, x, y, s| {
            dec::sub_into(o, x, y, s)
        }),
        BinOp::Mul => direct(arith::mul_into, scale.unwrap_or(sa + sb)),
        BinOp::Div => {
            if b.is_zero() {
                return Err(BcError::division_by_zero(DIV_ZERO));
            }
            match scale {
                Some(s) => direct(arith::div_into, s),
                None => with_scratch(|buf| {
                    let s = sa + EXPAND_SCALE;
                    functions::run_into(buf, &a, &b, s, arith::div_into)?;
                    Ok(Number::trimmed_from(buf, s, sa))
                }),
            }
        }
        BinOp::Mod => {
            if b.is_zero() {
                return Err(BcError::division_by_zero(MOD_ZERO));
            }
            let (_, r) = arith::divmod(&a, &b);
            Number::from_dec(&r, scale.unwrap_or(sa.max(sb)))
        }
        BinOp::Pow => {
            let e = functions::exponent(&b).map_err(|err| {
                BcError::value(match err {
                    ExpError::Fractional => "exponent cannot have a fractional part",
                    ExpError::TooLarge => "exponent is too large",
                })
            })?;
            pow_impl(a, e, scale)
        }
    }
}

fn pow_impl(a: Num, e: i64, scale: Option<usize>) -> Result<Number, BcError> {
    let sa = a.frac.len();
    let neg_scale = scale.unwrap_or(sa + EXPAND_SCALE);
    let Some(mut d) = functions::power(&a, e, neg_scale, scale)? else {
        return Err(BcError::division_by_zero("Negative power of zero"));
    };
    let out_scale = match scale {
        Some(s) => s,
        None if e >= 0 => {
            let s = sa as u128 * e as u128;
            if s > MAX_SCALE as u128 {
                return Err(BcError::out_of_memory(usize::MAX));
            }
            s as usize
        }
        None => {
            trim_to(&mut d, sa);
            d.scale
        }
    };
    Number::from_dec(&d, out_scale)
}
