//! The on-disk compiled-script cache (opcache.file_cache): the compiled
//! form of an included file ([`Compiled`]) serialized to a compact binary
//! format, so a fresh process skips lexing, parsing and compiling the
//! files an earlier one compiled.
//!
//! A cache file is `<opcache.file_cache>/<system id><full path>.bin`:
//!
//! ```text
//! "GOROFC\0\0"  magic
//! u32           FORMAT_VERSION
//! u64           build id (the binary's identity, see build_id())
//! i64, u64, u64 the source file's mtime (seconds, nanoseconds) and size
//!               when it was compiled
//! u64, u64      payload length, payload checksum
//! payload       the full path, the op array, the classes, the diagnostics
//! ```
//!
//! Anything that does not match (another build, a changed file, a
//! truncated or foreign file) is ignored: the script is compiled again
//! and the entry rewritten (through a temporary file renamed over it).
//!
//! Code that holds what a compile cannot reproduce in another process is
//! not stored: values other than scalars, strings and arrays in literals,
//! internal signatures, or folded constants of user code from other
//! files (see compiler::ct_user_dependency()). The process-wide names a
//! compile hands out (closure function names `__closure_N`, the keys of
//! top-level static variables `\0main#N`, OpArray::origin_id) are given
//! fresh numbers on load, as a compile in this process would.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::{
    ClassEntry, ConstantMeta, MethodDef, PropertyDef, RuntimeAttribute, TraitAdaptation, Visibility,
};
use crate::opcode::{Op, OpArray, OpCode, OperandType, ParamType, ParamTypeInfo};
use crate::script_cache::{Compiled, Diag};
use crate::string::PhpString;
use crate::value::Value;

const MAGIC: &[u8; 8] = b"GOROFC\0\0";
/// Bumped whenever the encoding below changes.
const FORMAT_VERSION: u32 = 2;
const HEADER_LEN: usize = 8 + 4 + 8 + 8 + 8 + 8 + 8 + 8;

/// A value that cannot be stored (the script is then not cached).
#[derive(Debug)]
pub struct Unsupported;

type Res<T> = Result<T, Unsupported>;

/// How deeply arrays and op arrays may nest in what is stored (the
/// encoder and the decoder recurse).
const MAX_DEPTH: u32 = 128;

pub(crate) struct W {
    buf: Vec<u8>,
    depth: u32,
}

impl W {
    #[inline]
    fn enter(&mut self) -> Res<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Unsupported);
        }
        Ok(())
    }
    #[inline]
    fn leave(&mut self) {
        self.depth -= 1;
    }
    #[inline]
    fn u64(&mut self, mut v: u64) {
        while v >= 0x80 {
            self.buf.push(v as u8 | 0x80);
            v >>= 7;
        }
        self.buf.push(v as u8);
    }
    #[inline]
    fn i64(&mut self, v: i64) {
        self.u64(((v << 1) ^ (v >> 63)) as u64);
    }
    #[inline]
    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    /// A byte string: its length (shifted left, the low bit telling
    /// whether it holds process-wide names to renumber), its bytes.
    #[inline]
    fn bytes(&mut self, b: &[u8]) {
        let generated = b.len() >= 7 && find_generated_name(b, 0).is_some();
        self.u64(((b.len() as u64) << 1) | generated as u64);
        self.buf.extend_from_slice(b);
    }
}

/// Fresh numbers for the process-wide names of the file being loaded.
#[derive(Default)]
struct Remap {
    closures: HashMap<u64, u64>,
    mains: HashMap<u64, u64>,
    origins: HashMap<u32, u32>,
}

pub(crate) struct R<'a> {
    data: &'a [u8],
    pos: usize,
    remap: Remap,
    depth: u32,
}

impl<'a> R<'a> {
    #[inline]
    fn enter(&mut self) -> Res<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Unsupported);
        }
        Ok(())
    }
    #[inline]
    fn leave(&mut self) {
        self.depth -= 1;
    }
    #[inline]
    fn u8(&mut self) -> Res<u8> {
        let b = *self.data.get(self.pos).ok_or(Unsupported)?;
        self.pos += 1;
        Ok(b)
    }
    #[inline(always)]
    fn u64(&mut self) -> Res<u64> {
        // (most numbers take a byte)
        match self.data.get(self.pos) {
            Some(&b) if b < 0x80 => {
                self.pos += 1;
                Ok(b as u64)
            }
            _ => self.u64_long(),
        }
    }
    #[inline(never)]
    fn u64_long(&mut self) -> Res<u64> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            if shift >= 64 {
                return Err(Unsupported);
            }
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
        }
    }
    #[inline]
    fn i64(&mut self) -> Res<i64> {
        let v = self.u64()?;
        Ok(((v >> 1) as i64) ^ -((v & 1) as i64))
    }
    #[inline]
    fn len(&mut self) -> Res<usize> {
        let n = self.u64()? as usize;
        // (every element takes a byte at least: a bad length fails here)
        if n > self.data.len() - self.pos {
            return Err(Unsupported);
        }
        Ok(n)
    }
    /// A byte string and whether it holds generated names (see W::bytes).
    #[inline]
    fn raw_flagged(&mut self) -> Res<(&'a [u8], bool)> {
        let v = self.u64()?;
        let n = (v >> 1) as usize;
        if n > self.data.len() - self.pos {
            return Err(Unsupported);
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok((s, v & 1 != 0))
    }
    #[inline]
    fn raw(&mut self) -> Res<&'a [u8]> {
        Ok(self.raw_flagged()?.0)
    }
    /// A byte string, with the process-wide names in it renumbered.
    #[inline]
    fn bytes(&mut self) -> Res<Vec<u8>> {
        let (s, generated) = self.raw_flagged()?;
        if !generated {
            return Ok(s.to_vec());
        }
        Ok(self.remap_slow(s))
    }

    #[cold]
    #[inline(never)]
    fn remap_slow(&mut self, s: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(s.len() + 8);
        let mut i = 0;
        while let Some((at, pfx, main, digits)) = find_generated_name(s, i) {
            out.extend_from_slice(&s[i..at + pfx]);
            let id = std::str::from_utf8(&s[at + pfx..at + pfx + digits])
                .ok()
                .and_then(|d| d.parse::<u64>().ok())
                .unwrap_or(0);
            let new = if main {
                *self
                    .remap
                    .mains
                    .entry(id)
                    .or_insert_with(crate::compiler::next_main_static_id)
            } else {
                *self
                    .remap
                    .closures
                    .entry(id)
                    .or_insert_with(crate::compiler::next_closure_id)
            };
            out.extend_from_slice(new.to_string().as_bytes());
            i = at + pfx + digits;
        }
        out.extend_from_slice(&s[i..]);
        out
    }
}

/// The next process-wide name in `s` from `from` on (see the module
/// docs): its offset, the length of its prefix, whether it is a static
/// key (else a closure function name), the number of its digits.
fn find_generated_name(s: &[u8], from: usize) -> Option<(usize, usize, bool, usize)> {
    const CLOSURE_PREFIXES: &[&[u8]] = &[
        b"__closure_fcc_",
        b"__closure_",
        b"__arrow_",
        crate::compiler::CONST_THUNK_PREFIX.as_bytes(),
    ];
    let mut i = from;
    while i + 1 < s.len() {
        // (8 bytes at a time while none is a '_' or a NUL)
        if let Some(w) = s.get(i..i + 8) {
            let v = u64::from_le_bytes(w.try_into().unwrap());
            const ONES: u64 = 0x0101_0101_0101_0101;
            const HIGH: u64 = 0x8080_8080_8080_8080;
            let zero = |x: u64| x.wrapping_sub(ONES) & !x & HIGH;
            if zero(v) == 0 && zero(v ^ (ONES * b'_' as u64)) == 0 {
                i += 8;
                continue;
            }
        }
        let rest = &s[i..];
        let found = match (rest[0], rest[1]) {
            (0, b'm') if rest.starts_with(b"\0main#") => Some((6, true)),
            (b'_', b'_') => CLOSURE_PREFIXES
                .iter()
                .find(|p| rest.starts_with(p))
                .map(|p| (p.len(), false)),
            _ => None,
        };
        if let Some((pfx, main)) = found {
            let digits = rest[pfx..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count();
            if digits > 0 && digits <= 19 {
                return Some((i, pfx, main, digits));
            }
        }
        i += 1;
    }
    None
}

pub(crate) trait Enc {
    fn enc(&self, w: &mut W) -> Res<()>;
}

pub(crate) trait Dec: Sized {
    fn dec(r: &mut R) -> Res<Self>;
}

macro_rules! num {
    ($($t:ty),*) => {$(
        impl Enc for $t {
            #[inline]
            fn enc(&self, w: &mut W) -> Res<()> {
                w.u64(*self as u64);
                Ok(())
            }
        }
        impl Dec for $t {
            #[inline]
            fn dec(r: &mut R) -> Res<Self> {
                <$t>::try_from(r.u64()?).map_err(|_| Unsupported)
            }
        }
    )*};
}
num!(u16, u32, u64, usize);

impl Enc for i64 {
    fn enc(&self, w: &mut W) -> Res<()> {
        w.i64(*self);
        Ok(())
    }
}
impl Dec for i64 {
    fn dec(r: &mut R) -> Res<Self> {
        r.i64()
    }
}

impl Enc for bool {
    #[inline]
    fn enc(&self, w: &mut W) -> Res<()> {
        w.u8(*self as u8);
        Ok(())
    }
}
impl Dec for bool {
    #[inline]
    fn dec(r: &mut R) -> Res<Self> {
        match r.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Unsupported),
        }
    }
}

impl Enc for Vec<u8> {
    #[inline]
    fn enc(&self, w: &mut W) -> Res<()> {
        w.bytes(self);
        Ok(())
    }
}
impl Dec for Vec<u8> {
    #[inline]
    fn dec(r: &mut R) -> Res<Self> {
        r.bytes()
    }
}

impl Enc for String {
    fn enc(&self, w: &mut W) -> Res<()> {
        w.bytes(self.as_bytes());
        Ok(())
    }
}
impl Dec for String {
    fn dec(r: &mut R) -> Res<Self> {
        String::from_utf8(r.bytes()?).map_err(|_| Unsupported)
    }
}

impl Enc for Rc<str> {
    fn enc(&self, w: &mut W) -> Res<()> {
        w.bytes(self.as_bytes());
        Ok(())
    }
}
impl Dec for Rc<str> {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(Rc::from(String::dec(r)?))
    }
}

impl<T: Enc> Enc for Option<T> {
    #[inline]
    fn enc(&self, w: &mut W) -> Res<()> {
        match self {
            None => {
                w.u8(0);
                Ok(())
            }
            Some(v) => {
                w.u8(1);
                v.enc(w)
            }
        }
    }
}
impl<T: Dec> Dec for Option<T> {
    #[inline]
    fn dec(r: &mut R) -> Res<Self> {
        match r.u8()? {
            0 => Ok(None),
            1 => Ok(Some(T::dec(r)?)),
            _ => Err(Unsupported),
        }
    }
}

/// A sequence (Vec<u8> is a byte string, see above).
macro_rules! seq {
    ($($t:ty),*) => {$(
        impl Enc for Vec<$t> {
            fn enc(&self, w: &mut W) -> Res<()> {
                w.u64(self.len() as u64);
                for v in self {
                    v.enc(w)?;
                }
                Ok(())
            }
        }
        impl Dec for Vec<$t> {
            fn dec(r: &mut R) -> Res<Self> {
                let n = r.len()?;
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    out.push(<$t>::dec(r)?);
                }
                Ok(out)
            }
        }
    )*};
}
seq!(
    bool,
    u32,
    Vec<u8>,
    Value,
    Op,
    Option<ParamTypeInfo>,
    Option<String>,
    ParamType,
    Rc<OpArray>,
    OpArray,
    RuntimeAttribute,
    Vec<RuntimeAttribute>,
    (Vec<u8>, Vec<RuntimeAttribute>),
    (Vec<u8>, bool),
    (u32, Value),
    (Vec<u8>, Value),
    (Vec<u8>, Vec<u8>),
    ClassEntry,
    TraitAdaptation,
    PropertyDef,
    Diag
);

impl<A: Enc, B: Enc> Enc for (A, B) {
    fn enc(&self, w: &mut W) -> Res<()> {
        self.0.enc(w)?;
        self.1.enc(w)
    }
}
impl<A: Dec, B: Dec> Dec for (A, B) {
    fn dec(r: &mut R) -> Res<Self> {
        let a = A::dec(r)?;
        Ok((a, B::dec(r)?))
    }
}

impl<T: Enc> Enc for Box<T> {
    fn enc(&self, w: &mut W) -> Res<()> {
        (**self).enc(w)
    }
}
impl<T: Dec> Dec for Box<T> {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(Box::new(T::dec(r)?))
    }
}

impl<T: Enc> Enc for Rc<T> {
    fn enc(&self, w: &mut W) -> Res<()> {
        (**self).enc(w)
    }
}
impl<T: Dec> Dec for Rc<T> {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(Rc::new(T::dec(r)?))
    }
}

impl<K: Enc, V: Enc, S> Enc for indexmap::IndexMap<K, V, S> {
    fn enc(&self, w: &mut W) -> Res<()> {
        w.u64(self.len() as u64);
        for (k, v) in self {
            k.enc(w)?;
            v.enc(w)?;
        }
        Ok(())
    }
}
impl<K: Dec + std::hash::Hash + Eq, V: Dec, S: std::hash::BuildHasher + Default> Dec
    for indexmap::IndexMap<K, V, S>
{
    fn dec(r: &mut R) -> Res<Self> {
        let n = r.len()?;
        let mut m = indexmap::IndexMap::with_capacity_and_hasher(n, S::default());
        for _ in 0..n {
            let k = K::dec(r)?;
            let v = V::dec(r)?;
            m.insert(k, v);
        }
        Ok(m)
    }
}

impl Enc for PhpString {
    fn enc(&self, w: &mut W) -> Res<()> {
        w.u8(self.is_interned() as u8);
        w.bytes(self.as_bytes());
        Ok(())
    }
}
impl Dec for PhpString {
    fn dec(r: &mut R) -> Res<Self> {
        let interned = bool::dec(r)?;
        let s = PhpString::from_vec(r.bytes()?);
        if interned {
            s.mark_interned();
        }
        Ok(s)
    }
}

impl Enc for Value {
    fn enc(&self, w: &mut W) -> Res<()> {
        match self {
            Value::Undef => w.u8(0),
            Value::Null => w.u8(1),
            Value::False => w.u8(2),
            Value::True => w.u8(3),
            Value::Long(n) => {
                w.u8(4);
                w.i64(*n);
            }
            Value::Double(f) => {
                w.u8(5);
                w.buf.extend_from_slice(&f.to_bits().to_le_bytes());
            }
            Value::String(s) => {
                w.u8(6);
                s.enc(w)?;
            }
            Value::Array(a) => {
                w.u8(7);
                w.enter()?;
                let a = a.try_borrow().map_err(|_| Unsupported)?;
                w.u64(a.len() as u64);
                w.i64(a.next_free_key());
                for (k, v) in a.iter() {
                    match k {
                        ArrayKey::Int(n) => {
                            w.u8(0);
                            w.i64(*n);
                        }
                        ArrayKey::String(s) => {
                            w.u8(1);
                            s.enc(w)?;
                        }
                    }
                    v.enc(w)?;
                }
                w.leave();
            }
            _ => return Err(Unsupported),
        }
        Ok(())
    }
}
impl Dec for Value {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(match r.u8()? {
            0 => Value::Undef,
            1 => Value::Null,
            2 => Value::False,
            3 => Value::True,
            4 => Value::Long(r.i64()?),
            5 => {
                let b = r.data.get(r.pos..r.pos + 8).ok_or(Unsupported)?;
                r.pos += 8;
                Value::Double(f64::from_bits(u64::from_le_bytes(b.try_into().unwrap())))
            }
            6 => Value::String(PhpString::dec(r)?),
            7 => {
                r.enter()?;
                let n = r.len()?;
                let next = r.i64()?;
                let mut a = PhpArray::with_capacity(n);
                for _ in 0..n {
                    let k = match r.u8()? {
                        0 => ArrayKey::Int(r.i64()?),
                        1 => ArrayKey::String(PhpString::dec(r)?),
                        _ => return Err(Unsupported),
                    };
                    a.set(k, Value::dec(r)?);
                }
                a.set_next_free_key(next);
                r.leave();
                Value::Array(Rc::new(RefCell::new(a)))
            }
            _ => return Err(Unsupported),
        })
    }
}

impl Enc for OperandType {
    #[inline]
    fn enc(&self, w: &mut W) -> Res<()> {
        let (tag, v) = match *self {
            OperandType::Unused => (0, 0),
            OperandType::Cv(v) => (1, v),
            OperandType::Const(v) => (2, v),
            OperandType::Tmp(v) => (3, v),
            OperandType::JmpTarget(v) => (4, v),
        };
        // (STMT_END's u32::MAX: the tag byte and the value)
        w.u8(tag);
        if tag != 0 {
            w.u64(v as u64);
        }
        Ok(())
    }
}
impl Dec for OperandType {
    #[inline]
    fn dec(r: &mut R) -> Res<Self> {
        let tag = r.u8()?;
        if tag == 0 {
            return Ok(OperandType::Unused);
        }
        let v = u32::dec(r)?;
        Ok(match tag {
            1 => OperandType::Cv(v),
            2 => OperandType::Const(v),
            3 => OperandType::Tmp(v),
            4 => OperandType::JmpTarget(v),
            _ => return Err(Unsupported),
        })
    }
}

impl Enc for Op {
    #[inline]
    fn enc(&self, w: &mut W) -> Res<()> {
        let Op {
            opcode,
            op1,
            op2,
            result,
            line,
        } = self;
        w.u64(*opcode as u8 as u64);
        op1.enc(w)?;
        op2.enc(w)?;
        result.enc(w)?;
        line.enc(w)
    }
}
impl Dec for Op {
    #[inline]
    fn dec(r: &mut R) -> Res<Self> {
        let code = u8::try_from(r.u64()?).map_err(|_| Unsupported)?;
        let opcode = OpCode::from_u8(code).ok_or(Unsupported)?;
        Ok(Op {
            opcode,
            op1: OperandType::dec(r)?,
            op2: OperandType::dec(r)?,
            result: OperandType::dec(r)?,
            line: u32::dec(r)?,
        })
    }
}

impl Enc for ParamType {
    fn enc(&self, w: &mut W) -> Res<()> {
        match self {
            ParamType::Simple(n) => {
                w.u8(0);
                n.enc(w)
            }
            ParamType::Nullable(t) => {
                w.u8(1);
                t.enc(w)
            }
            ParamType::Union(ts) => {
                w.u8(2);
                ts.enc(w)
            }
            ParamType::Intersection(ts) => {
                w.u8(3);
                ts.enc(w)
            }
        }
    }
}
impl Dec for ParamType {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(match r.u8()? {
            0 => ParamType::Simple(Vec::dec(r)?),
            1 => ParamType::Nullable(Box::dec(r)?),
            2 => ParamType::Union(Vec::dec(r)?),
            3 => ParamType::Intersection(Vec::dec(r)?),
            _ => return Err(Unsupported),
        })
    }
}

impl Enc for ParamTypeInfo {
    fn enc(&self, w: &mut W) -> Res<()> {
        let ParamTypeInfo {
            param_type,
            param_name,
            implicitly_nullable,
        } = self;
        param_type.enc(w)?;
        param_name.enc(w)?;
        implicitly_nullable.enc(w)
    }
}
impl Dec for ParamTypeInfo {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(ParamTypeInfo {
            param_type: ParamType::dec(r)?,
            param_name: Vec::dec(r)?,
            implicitly_nullable: bool::dec(r)?,
        })
    }
}

impl Enc for Visibility {
    fn enc(&self, w: &mut W) -> Res<()> {
        w.u8(match self {
            Visibility::Public => 0,
            Visibility::Protected => 1,
            Visibility::Private => 2,
        });
        Ok(())
    }
}
impl Dec for Visibility {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(match r.u8()? {
            0 => Visibility::Public,
            1 => Visibility::Protected,
            2 => Visibility::Private,
            _ => return Err(Unsupported),
        })
    }
}

impl Enc for RuntimeAttribute {
    fn enc(&self, w: &mut W) -> Res<()> {
        let RuntimeAttribute {
            name,
            args_op_array,
            validation_error,
        } = self;
        name.enc(w)?;
        args_op_array.enc(w)?;
        validation_error.enc(w)
    }
}
impl Dec for RuntimeAttribute {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(RuntimeAttribute {
            name: Vec::dec(r)?,
            args_op_array: Rc::new(OpArray::dec(r)?),
            validation_error: Option::dec(r)?,
        })
    }
}

impl Enc for TraitAdaptation {
    fn enc(&self, w: &mut W) -> Res<()> {
        match self {
            TraitAdaptation::Alias {
                trait_name,
                method,
                new_name,
                new_visibility,
                new_final,
            } => {
                w.u8(0);
                trait_name.enc(w)?;
                method.enc(w)?;
                new_name.enc(w)?;
                new_visibility.enc(w)?;
                new_final.enc(w)
            }
            TraitAdaptation::Precedence {
                trait_name,
                method,
                instead_of,
            } => {
                w.u8(1);
                trait_name.enc(w)?;
                method.enc(w)?;
                instead_of.enc(w)
            }
        }
    }
}
impl Dec for TraitAdaptation {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(match r.u8()? {
            0 => TraitAdaptation::Alias {
                trait_name: Option::dec(r)?,
                method: Vec::dec(r)?,
                new_name: Option::dec(r)?,
                new_visibility: Option::dec(r)?,
                new_final: bool::dec(r)?,
            },
            1 => TraitAdaptation::Precedence {
                trait_name: Vec::dec(r)?,
                method: Vec::dec(r)?,
                instead_of: Vec::dec(r)?,
            },
            _ => return Err(Unsupported),
        })
    }
}

impl Enc for ConstantMeta {
    fn enc(&self, w: &mut W) -> Res<()> {
        let ConstantMeta {
            visibility,
            is_final,
            declaring_class,
            attributes,
            declared_type,
            doc_comment,
            referenced_constants,
            line,
        } = self;
        visibility.enc(w)?;
        is_final.enc(w)?;
        declaring_class.enc(w)?;
        attributes.enc(w)?;
        declared_type.enc(w)?;
        doc_comment.enc(w)?;
        referenced_constants.enc(w)?;
        line.enc(w)
    }
}
impl Dec for ConstantMeta {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(ConstantMeta {
            visibility: Visibility::dec(r)?,
            is_final: bool::dec(r)?,
            declaring_class: Vec::dec(r)?,
            attributes: Vec::dec(r)?,
            declared_type: Option::dec(r)?,
            doc_comment: Option::dec(r)?,
            referenced_constants: Vec::dec(r)?,
            line: u32::dec(r)?,
        })
    }
}

impl Enc for PropertyDef {
    fn enc(&self, w: &mut W) -> Res<()> {
        let PropertyDef {
            name,
            default,
            is_static,
            is_readonly,
            visibility,
            set_visibility,
            declaring_class,
            property_type,
            has_get_hook,
            has_set_hook,
            is_virtual,
            is_abstract,
            is_final,
            attributes,
            doc_comment,
        } = self;
        name.enc(w)?;
        default.enc(w)?;
        is_static.enc(w)?;
        is_readonly.enc(w)?;
        visibility.enc(w)?;
        set_visibility.enc(w)?;
        declaring_class.enc(w)?;
        property_type.enc(w)?;
        has_get_hook.enc(w)?;
        has_set_hook.enc(w)?;
        is_virtual.enc(w)?;
        is_abstract.enc(w)?;
        is_final.enc(w)?;
        attributes.enc(w)?;
        doc_comment.enc(w)
    }
}
impl Dec for PropertyDef {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(PropertyDef {
            name: Vec::dec(r)?,
            default: Value::dec(r)?,
            is_static: bool::dec(r)?,
            is_readonly: bool::dec(r)?,
            visibility: Visibility::dec(r)?,
            set_visibility: Option::dec(r)?,
            declaring_class: Vec::dec(r)?,
            property_type: Option::dec(r)?,
            has_get_hook: bool::dec(r)?,
            has_set_hook: bool::dec(r)?,
            is_virtual: bool::dec(r)?,
            is_abstract: bool::dec(r)?,
            is_final: bool::dec(r)?,
            attributes: Vec::dec(r)?,
            doc_comment: Option::dec(r)?,
        })
    }
}

impl Enc for MethodDef {
    fn enc(&self, w: &mut W) -> Res<()> {
        let MethodDef {
            name,
            op_array,
            param_count,
            is_static,
            is_abstract,
            is_final,
            visibility,
            declaring_class,
            doc_comment,
            attributes,
        } = self;
        name.enc(w)?;
        op_array.enc(w)?;
        param_count.enc(w)?;
        is_static.enc(w)?;
        is_abstract.enc(w)?;
        is_final.enc(w)?;
        visibility.enc(w)?;
        declaring_class.enc(w)?;
        doc_comment.enc(w)?;
        attributes.enc(w)
    }
}
impl Dec for MethodDef {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(MethodDef {
            name: Vec::dec(r)?,
            op_array: Rc::dec(r)?,
            param_count: usize::dec(r)?,
            is_static: bool::dec(r)?,
            is_abstract: bool::dec(r)?,
            is_final: bool::dec(r)?,
            visibility: Visibility::dec(r)?,
            declaring_class: Vec::dec(r)?,
            doc_comment: Option::dec(r)?,
            attributes: Vec::dec(r)?,
        })
    }
}

impl Enc for ClassEntry {
    fn enc(&self, w: &mut W) -> Res<()> {
        let ClassEntry {
            name,
            parent,
            interfaces,
            traits,
            trait_adaptations,
            properties,
            methods,
            constants,
            constants_meta,
            static_properties,
            is_abstract,
            is_final,
            is_readonly,
            is_interface,
            is_trait,
            is_enum,
            enum_backing_type,
            enum_cases,
            start_line,
            end_line,
            filename,
            doc_comment,
            attributes,
            const_closures,
            allow_dynamic_properties,
            new_template: _,
            own_prop_order,
        } = self;
        name.enc(w)?;
        parent.enc(w)?;
        interfaces.enc(w)?;
        traits.enc(w)?;
        trait_adaptations.enc(w)?;
        properties.enc(w)?;
        methods.enc(w)?;
        constants.enc(w)?;
        constants_meta.enc(w)?;
        static_properties.enc(w)?;
        is_abstract.enc(w)?;
        is_final.enc(w)?;
        is_readonly.enc(w)?;
        is_interface.enc(w)?;
        is_trait.enc(w)?;
        is_enum.enc(w)?;
        enum_backing_type.enc(w)?;
        enum_cases.enc(w)?;
        start_line.enc(w)?;
        end_line.enc(w)?;
        filename.enc(w)?;
        doc_comment.enc(w)?;
        attributes.enc(w)?;
        const_closures.enc(w)?;
        allow_dynamic_properties.enc(w)?;
        own_prop_order.enc(w)
    }
}
impl Dec for ClassEntry {
    fn dec(r: &mut R) -> Res<Self> {
        let mut c = ClassEntry::new(Vec::dec(r)?);
        c.parent = Option::dec(r)?;
        c.interfaces = Vec::dec(r)?;
        c.traits = Vec::dec(r)?;
        c.trait_adaptations = Vec::dec(r)?;
        c.properties = Vec::dec(r)?;
        c.methods = indexmap::IndexMap::dec(r)?;
        c.constants = indexmap::IndexMap::dec(r)?;
        c.constants_meta = indexmap::IndexMap::dec(r)?;
        c.static_properties = indexmap::IndexMap::dec(r)?;
        c.is_abstract = bool::dec(r)?;
        c.is_final = bool::dec(r)?;
        c.is_readonly = bool::dec(r)?;
        c.is_interface = bool::dec(r)?;
        c.is_trait = bool::dec(r)?;
        c.is_enum = bool::dec(r)?;
        c.enum_backing_type = Option::dec(r)?;
        c.enum_cases = Vec::dec(r)?;
        c.start_line = u32::dec(r)?;
        c.end_line = u32::dec(r)?;
        c.filename = Option::dec(r)?;
        c.doc_comment = Option::dec(r)?;
        c.attributes = Vec::dec(r)?;
        c.const_closures = Vec::dec(r)?;
        c.allow_dynamic_properties = bool::dec(r)?;
        c.own_prop_order = Vec::dec(r)?;
        Ok(c)
    }
}

impl Enc for OpArray {
    fn enc(&self, w: &mut W) -> Res<()> {
        // (the caches OpArray keeps for the VM are made again on use)
        let OpArray {
            ops,
            literals,
            cv_names,
            temp_count,
            temp_next,
            name,
            param_count,
            required_param_count,
            variadic_param,
            child_functions,
            is_generator,
            param_types,
            return_type,
            scope_class,
            is_static_closure,
            returns_by_ref,
            decl_line,
            end_line,
            doc_comment,
            class_const_literals,
            const_expr_const_names,
            strict_types,
            by_ref_params,
            param_has_default,
            attributes,
            param_attributes,
            filename,
            filename_rc: _,
            fcc_target_name,
            display_name,
            const_attribute_table,
            local_classes,
            internal_sig,
            param_default_repr,
            static_defaults,
            closure_capture_names,
            closure_params,
            fcc_desc,
            closure_called_class,
            origin_id,
            ..
        } = self;
        if internal_sig.is_some() {
            return Err(Unsupported);
        }
        w.enter()?;
        ops.enc(w)?;
        literals.enc(w)?;
        cv_names.enc(w)?;
        temp_count.enc(w)?;
        temp_next.enc(w)?;
        name.enc(w)?;
        param_count.enc(w)?;
        required_param_count.enc(w)?;
        variadic_param.enc(w)?;
        child_functions.enc(w)?;
        is_generator.enc(w)?;
        param_types.enc(w)?;
        return_type.enc(w)?;
        scope_class.enc(w)?;
        is_static_closure.enc(w)?;
        returns_by_ref.enc(w)?;
        decl_line.enc(w)?;
        end_line.enc(w)?;
        doc_comment.enc(w)?;
        class_const_literals.enc(w)?;
        const_expr_const_names.enc(w)?;
        strict_types.enc(w)?;
        by_ref_params.enc(w)?;
        param_has_default.enc(w)?;
        attributes.enc(w)?;
        param_attributes.enc(w)?;
        filename.enc(w)?;
        fcc_target_name.enc(w)?;
        display_name.enc(w)?;
        const_attribute_table.enc(w)?;
        local_classes.enc(w)?;
        param_default_repr.enc(w)?;
        static_defaults.enc(w)?;
        closure_capture_names.enc(w)?;
        closure_params.enc(w)?;
        fcc_desc.enc(w)?;
        closure_called_class.enc(w)?;
        origin_id.enc(w)?;
        w.leave();
        Ok(())
    }
}
impl Dec for OpArray {
    fn dec(r: &mut R) -> Res<Self> {
        r.enter()?;
        let mut o = OpArray::new();
        o.ops = Vec::dec(r)?;
        o.literals = Vec::dec(r)?;
        o.cv_names = Vec::dec(r)?;
        o.temp_count = u32::dec(r)?;
        o.temp_next = u32::dec(r)?;
        o.name = Vec::dec(r)?;
        o.param_count = u32::dec(r)?;
        o.required_param_count = u32::dec(r)?;
        o.variadic_param = Option::dec(r)?;
        o.child_functions = Vec::dec(r)?;
        o.is_generator = bool::dec(r)?;
        o.param_types = Vec::dec(r)?;
        o.return_type = Option::dec(r)?;
        o.scope_class = Option::dec(r)?;
        o.is_static_closure = bool::dec(r)?;
        o.returns_by_ref = bool::dec(r)?;
        o.decl_line = u32::dec(r)?;
        o.end_line = u32::dec(r)?;
        o.doc_comment = Option::dec(r)?;
        o.class_const_literals = Vec::dec(r)?;
        o.const_expr_const_names = Vec::dec(r)?;
        o.strict_types = bool::dec(r)?;
        o.by_ref_params = Vec::dec(r)?;
        o.param_has_default = Vec::dec(r)?;
        o.attributes = Vec::dec(r)?;
        o.param_attributes = Vec::dec(r)?;
        o.filename = Vec::dec(r)?;
        o.fcc_target_name = Option::dec(r)?;
        o.display_name = Vec::dec(r)?;
        o.const_attribute_table = Vec::dec(r)?;
        o.local_classes = Vec::dec(r)?;
        o.param_default_repr = Vec::dec(r)?;
        o.static_defaults = Option::dec(r)?;
        o.closure_capture_names = Vec::dec(r)?;
        o.closure_params = Vec::dec(r)?;
        o.fcc_desc = Option::dec(r)?;
        o.closure_called_class = Option::dec(r)?;
        // A function reached through several copies keeps one id; the id
        // OpArray::new() gave is fresh in this process.
        let old = u32::dec(r)?;
        o.origin_id = *r.remap.origins.entry(old).or_insert(o.origin_id);
        r.leave();
        Ok(o)
    }
}

impl Enc for Diag {
    fn enc(&self, w: &mut W) -> Res<()> {
        match self {
            Diag::Lex(d, m, l) => {
                w.u8(if *d { 1 } else { 0 });
                m.enc(w)?;
                l.enc(w)
            }
            Diag::Compile(m, l) => {
                w.u8(2);
                m.enc(w)?;
                l.enc(w)
            }
        }
    }
}
impl Dec for Diag {
    fn dec(r: &mut R) -> Res<Self> {
        Ok(match r.u8()? {
            0 => Diag::Lex(false, String::dec(r)?, u32::dec(r)?),
            1 => Diag::Lex(true, String::dec(r)?, u32::dec(r)?),
            2 => Diag::Compile(String::dec(r)?, u32::dec(r)?),
            _ => return Err(Unsupported),
        })
    }
}

/// A 64-bit checksum of the payload (FNV-1a over 8-byte words).
fn checksum(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let (words, rest) = data.as_chunks::<8>();
    for c in words {
        h ^= u64::from_le_bytes(*c);
        h = h.wrapping_mul(0x100000001b3);
    }
    for &b in rest {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^ (data.len() as u64)
}

/// The identity of this build of goro: the compiled form of scripts
/// changes with the code, so a cache file written by another binary is
/// not used (the running executable's path, size and mtime).
pub fn build_id() -> u64 {
    static ID: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *ID.get_or_init(|| {
        use std::os::unix::fs::MetadataExt;
        let mut s = format!("goro {} fc{}", env!("CARGO_PKG_VERSION"), FORMAT_VERSION);
        if let Ok(exe) = std::env::current_exe() {
            s.push_str(&exe.to_string_lossy());
            if let Ok(m) = std::fs::metadata(&exe) {
                s.push_str(&format!(":{}:{}:{}", m.len(), m.mtime(), m.mtime_nsec()));
            }
        }
        checksum(s.as_bytes())
    })
}

/// The settings a compile depends on besides the source: part of the
/// build id check (PHP_SAPI is folded at compile time, short_open_tag
/// changes the lexing).
fn settings_id() -> u64 {
    let mut s = crate::compiler::sapi_name_bytes();
    s.push(goro_parser::short_open_tag() as u8);
    build_id() ^ checksum(&s).rotate_left(17)
}

/// Serializes a compiled script (None if it holds what cannot be stored).
pub fn encode(
    path: &str,
    mtime: (i64, i64),
    size: u64,
    op_array: &OpArray,
    classes: &[ClassEntry],
    diags: &[Diag],
) -> Option<Vec<u8>> {
    let mut w = W {
        buf: Vec::with_capacity(64 * 1024),
        depth: 0,
    };
    w.buf.resize(HEADER_LEN, 0);
    let body = (|| -> Res<()> {
        w.bytes(path.as_bytes());
        op_array.enc(&mut w)?;
        w.u64(classes.len() as u64);
        for class in classes {
            class.enc(&mut w)?;
        }
        w.u64(diags.len() as u64);
        for d in diags {
            d.enc(&mut w)?;
        }
        Ok(())
    })();
    body.ok()?;
    let payload_len = (w.buf.len() - HEADER_LEN) as u64;
    let sum = checksum(&w.buf[HEADER_LEN..]);
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(MAGIC);
    h.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    h.extend_from_slice(&settings_id().to_le_bytes());
    h.extend_from_slice(&mtime.0.to_le_bytes());
    h.extend_from_slice(&mtime.1.to_le_bytes());
    h.extend_from_slice(&size.to_le_bytes());
    h.extend_from_slice(&payload_len.to_le_bytes());
    h.extend_from_slice(&sum.to_le_bytes());
    w.buf[..HEADER_LEN].copy_from_slice(&h);
    Some(w.buf)
}

/// The header of a cache file: ((mtime, its nanoseconds), size) of the source it was
/// compiled from, when it is a cache file of this build.
pub fn header(data: &[u8]) -> Option<((i64, i64), u64)> {
    if data.len() < HEADER_LEN || &data[..8] != MAGIC {
        return None;
    }
    let word = |at: usize| u64::from_le_bytes(data[at..at + 8].try_into().unwrap());
    let version = u32::from_le_bytes(data[8..12].try_into().unwrap());
    if version != FORMAT_VERSION || word(12) != settings_id() {
        return None;
    }
    Some(((word(20) as i64, word(28) as i64), word(36)))
}

/// Deserializes a cache file whose header() matched: None when it is
/// truncated, corrupt or not for `path`.
pub fn decode(path: &str, data: &[u8]) -> Option<Compiled> {
    header(data)?;
    let word = |at: usize| u64::from_le_bytes(data[at..at + 8].try_into().unwrap());
    let payload = &data[HEADER_LEN..];
    if word(44) != payload.len() as u64 || word(52) != checksum(payload) {
        return None;
    }
    let mut r = R {
        data: payload,
        pos: 0,
        remap: Remap::default(),
        depth: 0,
    };
    let r = &mut r;
    if r.raw().ok()? != path.as_bytes() {
        return None;
    }
    let op_array = OpArray::dec(r).ok()?;
    let n = r.len().ok()?;
    let mut classes = Vec::with_capacity(n);
    for _ in 0..n {
        classes.push(ClassEntry::dec(r).ok()?);
    }
    let diags = Vec::dec(r).ok()?;
    if r.pos != payload.len() {
        return None;
    }
    Some(Compiled {
        op_array,
        classes: Rc::new(classes),
        diags,
    })
}
