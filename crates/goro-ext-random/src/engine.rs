//! The engine interface: one `generate()` result at a time.

use crate::{
    MSG_EMPTY_STRING, Mt19937, MtMode, PcgOneseq128XslRr64, RandomError, Secure, Xoshiro256StarStar,
};

/// One `generate()` result: up to 8 bytes, read little-endian into `value`.
///
/// PHP keeps at most 8 bytes of a result: longer strings returned by user
/// engines are truncated (test 03_randomizer/methods/getBytes_expansion.phpt),
/// shorter ones are combined with further calls where a method needs more
/// bits (tests getInt_expansion_32.phpt, getInt_expansion_64.phpt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Generated {
    /// The bytes, little-endian; bits above `size * 8` are zero.
    pub value: u64,
    /// The number of bytes, 1 to 8.
    pub size: u32,
}

impl Generated {
    /// A 4-byte result.
    #[inline(always)]
    pub fn u32(v: u32) -> Generated {
        Generated {
            value: v as u64,
            size: 4,
        }
    }

    /// An 8-byte result.
    #[inline(always)]
    pub fn u64(v: u64) -> Generated {
        Generated { value: v, size: 8 }
    }

    /// The result of a user engine's `generate()` returning `bytes`.
    ///
    /// An empty string is a `Random\BrokenRandomEngineError` ("A random
    /// engine must return a non-empty string", test
    /// 03_randomizer/engine_unsafe_empty_string.phpt); bytes past the eighth
    /// are dropped.
    pub fn from_bytes(bytes: &[u8]) -> Result<Generated, RandomError> {
        if bytes.is_empty() {
            return Err(RandomError::BrokenEngine(MSG_EMPTY_STRING));
        }
        let n = bytes.len().min(8);
        let mut b = [0u8; 8];
        b[..n].copy_from_slice(&bytes[..n]);
        Ok(Generated {
            value: u64::from_le_bytes(b),
            size: n as u32,
        })
    }

    /// The result as `generate()` returns it to PHP code.
    pub fn bytes(&self) -> Vec<u8> {
        self.value.to_le_bytes()[..self.size as usize].to_vec()
    }
}

/// A random engine, native or user-defined.
///
/// The VM implements this for user classes implementing `Random\Engine`:
/// `generate()` calls the PHP method and converts its string with
/// [`Generated::from_bytes`]; an exception or `exit()` inside it is
/// reported as [`RandomError::Propagate`] (test
/// 03_randomizer/engine_unsafe_throws.phpt).
pub trait Engine {
    /// One result.
    fn generate(&mut self) -> Result<Generated, RandomError>;

    /// Whether this is a native `Mt19937` in `MT_RAND_PHP` mode, whose
    /// `Randomizer::getInt()` keeps `mt_rand()`'s pre-7.1 range scaling
    /// (test 03_randomizer/compatibility_mt_rand.phpt). Only the native
    /// engine does: a user engine wrapping it uses the standard reduction.
    fn mt_php_scaling(&self) -> bool {
        false
    }

    /// Fills `out` directly when the output of the engine is unobservable
    /// byte-for-byte (the CSPRNG), so `getBytes()` can take it in one go.
    fn fill_unobservable(&mut self, _out: &mut [u8]) -> Option<Result<(), RandomError>> {
        None
    }

    /// Fills `out` with the concatenated results, as `getBytes()` would
    /// from repeated [`Engine::generate`] calls (a fast path for native
    /// engines; `None` to use `generate()`).
    fn fill_bytes(&mut self, _out: &mut [u8]) -> Option<Result<(), RandomError>> {
        None
    }

    /// This engine as a native `Mt19937`, for [`Engine::mt_php_scaling`]
    /// engines (`None` for any other engine).
    fn as_mt19937(&mut self) -> Option<&mut Mt19937> {
        None
    }
}

impl<E: Engine + ?Sized> Engine for &mut E {
    #[inline(always)]
    fn generate(&mut self) -> Result<Generated, RandomError> {
        (**self).generate()
    }
    fn mt_php_scaling(&self) -> bool {
        (**self).mt_php_scaling()
    }
    fn fill_unobservable(&mut self, out: &mut [u8]) -> Option<Result<(), RandomError>> {
        (**self).fill_unobservable(out)
    }
    fn fill_bytes(&mut self, out: &mut [u8]) -> Option<Result<(), RandomError>> {
        (**self).fill_bytes(out)
    }
    fn as_mt19937(&mut self) -> Option<&mut Mt19937> {
        (**self).as_mt19937()
    }
}

impl<E: Engine + ?Sized> Engine for Box<E> {
    #[inline(always)]
    fn generate(&mut self) -> Result<Generated, RandomError> {
        (**self).generate()
    }
    fn mt_php_scaling(&self) -> bool {
        (**self).mt_php_scaling()
    }
    fn fill_unobservable(&mut self, out: &mut [u8]) -> Option<Result<(), RandomError>> {
        (**self).fill_unobservable(out)
    }
    fn fill_bytes(&mut self, out: &mut [u8]) -> Option<Result<(), RandomError>> {
        (**self).fill_bytes(out)
    }
    fn as_mt19937(&mut self) -> Option<&mut Mt19937> {
        (**self).as_mt19937()
    }
}

/// Any of the four native engines.
#[derive(Debug, Clone)]
pub enum NativeEngine {
    Mt19937(Box<Mt19937>),
    Pcg(PcgOneseq128XslRr64),
    Xoshiro(Xoshiro256StarStar),
    Secure(Secure),
}

impl NativeEngine {
    /// The PHP class name.
    pub fn class_name(&self) -> &'static str {
        match self {
            NativeEngine::Mt19937(_) => crate::class::MT19937,
            NativeEngine::Pcg(_) => crate::class::PCG,
            NativeEngine::Xoshiro(_) => crate::class::XOSHIRO,
            NativeEngine::Secure(_) => crate::class::SECURE,
        }
    }
}

impl Engine for NativeEngine {
    #[inline]
    fn generate(&mut self) -> Result<Generated, RandomError> {
        match self {
            NativeEngine::Mt19937(e) => e.generate(),
            NativeEngine::Pcg(e) => e.generate(),
            NativeEngine::Xoshiro(e) => e.generate(),
            NativeEngine::Secure(e) => e.generate(),
        }
    }
    fn mt_php_scaling(&self) -> bool {
        matches!(self, NativeEngine::Mt19937(e) if e.mode() == MtMode::Php)
    }
    fn fill_unobservable(&mut self, out: &mut [u8]) -> Option<Result<(), RandomError>> {
        match self {
            NativeEngine::Secure(e) => e.fill_unobservable(out),
            _ => None,
        }
    }
    fn fill_bytes(&mut self, out: &mut [u8]) -> Option<Result<(), RandomError>> {
        match self {
            NativeEngine::Mt19937(e) => e.fill_bytes(out),
            NativeEngine::Pcg(e) => e.fill_bytes(out),
            NativeEngine::Xoshiro(e) => e.fill_bytes(out),
            NativeEngine::Secure(e) => e.fill_bytes(out),
        }
    }
    fn as_mt19937(&mut self) -> Option<&mut Mt19937> {
        match self {
            NativeEngine::Mt19937(e) => Some(e),
            _ => None,
        }
    }
}

/// Fills `out` with little-endian words from `next`, the last one cut.
#[inline(always)]
pub(crate) fn fill_words<const W: usize>(out: &mut [u8], mut next: impl FnMut() -> [u8; W]) {
    let (words, rest) = out.as_chunks_mut::<W>();
    for c in words {
        *c = next();
    }
    if !rest.is_empty() {
        let w = next();
        let n = rest.len();
        rest.copy_from_slice(&w[..n]);
    }
}
