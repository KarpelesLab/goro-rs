//! The streaming-state interface every algorithm implements, and the
//! field layout used to expose that state through `serialize()`.
//!
//! PHP serializes a `HashContext` as `[algo, flags, state, magic, members]`
//! where `state` is a flat list of integers and byte strings. Each element
//! corresponds to one field of the algorithm's internal state; 64-bit
//! words appear as two 32-bit integers (low half first), and every 32-bit
//! value is printed as a signed integer. When unserializing, a wrong or
//! missing element is reported with the code `-(1000 + offset)`, where
//! `offset` is the field's byte offset in PHP's state record, and a state
//! that fails the algorithm's own consistency checks with `-2000`. These
//! layouts and codes were observed on PHP 8.5 (tests
//! ext/hash/tests/hash_serialize_003.phpt and hash_serialize_004.phpt, and
//! the probes in tests/fixtures/hash_serialize.php).

/// One field kind of a serialized state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A 32-bit word, one integer element.
    U32,
    /// A 64-bit word, two integer elements (low half, then high half).
    U64,
    /// A byte-wide counter, one integer element (truncated to 8 bits).
    U8,
    /// A byte string of exactly this length, one string element.
    Bytes(usize),
}

/// A run of `count` fields of one kind, starting at byte `offset` of PHP's
/// state record. Consecutive fields of the run follow at the kind's width.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Field {
    pub kind: Kind,
    pub count: usize,
    pub offset: usize,
}

pub(crate) const fn f(kind: Kind, count: usize, offset: usize) -> Field {
    Field {
        kind,
        count,
        offset,
    }
}

/// A state field's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Val {
    U32(u32),
    U64(u64),
    U8(u8),
    Bytes(Vec<u8>),
}

impl Val {
    pub(crate) fn u32(&self) -> u32 {
        match self {
            Val::U32(v) => *v,
            Val::U64(v) => *v as u32,
            Val::U8(v) => *v as u32,
            Val::Bytes(_) => 0,
        }
    }
    pub(crate) fn u64(&self) -> u64 {
        match self {
            Val::U64(v) => *v,
            other => other.u32() as u64,
        }
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        match self {
            Val::Bytes(b) => b,
            _ => &[],
        }
    }
}

/// The streaming state of one algorithm.
pub(crate) trait Engine: Send + Sync {
    /// Absorbs `data`.
    fn update(&mut self, data: &[u8]);
    /// Writes the digest into `out` (exactly the algorithm's digest length).
    /// The state is consumed: it must not be used afterwards.
    fn finish(&mut self, out: &mut [u8]);
    /// A copy of this state (`hash_copy()`).
    fn clone_box(&self) -> Box<dyn Engine>;
    /// The field values in serialization order, one per field (a run of
    /// `count` fields contributes `count` values).
    fn export(&self) -> Vec<Val>;
    /// Restores the state from field values in serialization order. Returns
    /// `false` when the values are inconsistent (PHP's code -2000).
    fn import(&mut self, vals: &[Val]) -> bool;
}

/// The number of field values a layout has.
pub(crate) fn value_count(layout: &[Field]) -> usize {
    layout.iter().map(|f| f.count).sum()
}

/// The words of a serialized state, a vector of PHP integers and strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SerVal {
    /// An integer element.
    Int(i64),
    /// A string element.
    Str(Vec<u8>),
}

/// One element of a state array handed back to `__unserialize()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SerInput<'a> {
    /// An integer.
    Int(i64),
    /// A string.
    Str(&'a [u8]),
    /// Any other type (float, bool, null, array, object).
    Other,
}

/// The state array of a serialized context, indexed by integer key.
pub trait StateArray {
    /// The element at integer key `index`, or `None` when the array has no
    /// such key.
    fn get(&self, index: usize) -> Option<SerInput<'_>>;
}

impl StateArray for [Option<SerInput<'_>>] {
    fn get(&self, index: usize) -> Option<SerInput<'_>> {
        self.get(index).copied().flatten()
    }
}

impl StateArray for Vec<SerVal> {
    fn get(&self, index: usize) -> Option<SerInput<'_>> {
        <[SerVal]>::get(self, index).map(|v| match v {
            SerVal::Int(i) => SerInput::Int(*i),
            SerVal::Str(s) => SerInput::Str(s),
        })
    }
}

/// Flattens field values into PHP's element list.
pub(crate) fn encode(layout: &[Field], vals: &[Val]) -> Vec<SerVal> {
    let mut out = Vec::with_capacity(value_count(layout) * 2);
    let mut it = vals.iter();
    for fl in layout {
        for _ in 0..fl.count {
            let v = it.next().expect("export matches layout");
            match (fl.kind, v) {
                (Kind::U32, v) => out.push(SerVal::Int(v.u32() as i32 as i64)),
                (Kind::U8, v) => out.push(SerVal::Int((v.u32() & 0xff) as i64)),
                (Kind::U64, v) => {
                    let w = v.u64();
                    out.push(SerVal::Int(w as u32 as i32 as i64));
                    out.push(SerVal::Int((w >> 32) as u32 as i32 as i64));
                }
                (Kind::Bytes(n), v) => {
                    let mut b = v.bytes().to_vec();
                    b.resize(n, 0);
                    out.push(SerVal::Str(b));
                }
            }
        }
    }
    out
}

/// Parses PHP's element list back into field values, or returns the error
/// code of the first bad element (`-(1000 + offset)`).
pub(crate) fn decode(layout: &[Field], state: &dyn StateArray) -> Result<Vec<Val>, i32> {
    let mut out = Vec::with_capacity(value_count(layout));
    let mut idx = 0usize;
    let int_at = |i: usize| match state.get(i) {
        Some(SerInput::Int(v)) => Some(v),
        _ => None,
    };
    for fl in layout {
        for k in 0..fl.count {
            match fl.kind {
                Kind::U32 => {
                    let off = fl.offset + 4 * k;
                    let v = int_at(idx).ok_or(-(1000 + off as i32))?;
                    idx += 1;
                    out.push(Val::U32(v as u32));
                }
                Kind::U8 => {
                    let off = fl.offset + k;
                    let v = int_at(idx).ok_or(-(1000 + off as i32))?;
                    idx += 1;
                    out.push(Val::U8(v as u8));
                }
                Kind::U64 => {
                    let off = fl.offset + 8 * k;
                    let lo = int_at(idx).ok_or(-(1000 + off as i32))?;
                    let hi = int_at(idx + 1).ok_or(-(1000 + off as i32))?;
                    idx += 2;
                    out.push(Val::U64((lo as u32 as u64) | ((hi as u32 as u64) << 32)));
                }
                Kind::Bytes(n) => {
                    let off = fl.offset + n * k;
                    match state.get(idx) {
                        Some(SerInput::Str(s)) if s.len() == n => out.push(Val::Bytes(s.to_vec())),
                        _ => return Err(-(1000 + off as i32)),
                    }
                    idx += 1;
                }
            }
        }
    }
    Ok(out)
}

/// Block buffering shared by the Merkle-Damgård style algorithms.
///
/// PHP's algorithms differ in what their buffer holds beyond the pending
/// bytes, which shows in `serialize()` output (observed on PHP 8.5). With
/// `copy_first` (the RFC 1321 reference scheme) the first block of an
/// update is always copied into the buffer before it is compressed, even
/// when the buffer was empty; without it, whole blocks are compressed
/// straight from the input when the buffer is empty. Either way the
/// remainder is copied to the start of the buffer and older bytes past it
/// are left in place.
#[derive(Clone)]
pub(crate) struct BlockBuf<const B: usize> {
    pub buf: [u8; B],
}

impl<const B: usize> BlockBuf<B> {
    pub(crate) fn new() -> Self {
        BlockBuf { buf: [0; B] }
    }

    /// Absorbs `data` given `used` pending bytes; calls `compress` for each
    /// complete block. Returns the new pending count.
    #[inline]
    pub(crate) fn absorb(
        &mut self,
        used: usize,
        mut data: &[u8],
        copy_first: bool,
        mut compress: impl FnMut(&[u8; B]),
    ) -> usize {
        let mut used = used % B;
        if used != 0 || (copy_first && data.len() >= B) {
            let take = B - used;
            if data.len() < take {
                self.buf[used..used + data.len()].copy_from_slice(data);
                return used + data.len();
            }
            self.buf[used..].copy_from_slice(&data[..take]);
            compress(&self.buf);
            data = &data[take..];
            used = 0;
        }
        let (chunks, chunks_rest) = data.as_chunks::<B>();
        for block in chunks {
            compress(block);
        }
        let rest = chunks_rest;
        self.buf[used..used + rest.len()].copy_from_slice(rest);
        used + rest.len()
    }
}

#[inline]
pub(crate) fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes(b[..4].try_into().unwrap())
}
#[inline]
pub(crate) fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes(b[..4].try_into().unwrap())
}
#[inline]
pub(crate) fn le64(b: &[u8]) -> u64 {
    u64::from_le_bytes(b[..8].try_into().unwrap())
}
#[inline]
pub(crate) fn be64(b: &[u8]) -> u64 {
    u64::from_be_bytes(b[..8].try_into().unwrap())
}

/// Implements `clone_box` for a `Clone` engine.
macro_rules! clone_box {
    () => {
        fn clone_box(&self) -> Box<dyn $crate::engine::Engine> {
            Box::new(self.clone())
        }
    };
}
pub(crate) use clone_box;
