//! Argon2i / Argon2id version 1.3 (RFC 9106).
//!
//! purecrypto has Argon2, but its portable compression function runs at
//! about half libsodium's speed on the reference machine, so the memory
//! filling is here, with an AVX2 kernel for the BlaMka permutation on
//! x86-64 and a portable fallback. Argon2i's memory accesses are
//! data-independent; Argon2id is data-dependent after the first two slices
//! of the first pass, by design (RFC 9106 §3.4.1.3).

use crate::blake2b::Blake2b;

/// Argon2 variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// Only used by the RFC 9106 tests (PHP has no Argon2d).
    #[cfg_attr(not(test), allow(dead_code))]
    Argon2d = 0,
    Argon2i = 1,
    Argon2id = 2,
}

/// Version 0x13.
pub const VERSION: u32 = 0x13;

const QWORDS: usize = 128;
const SYNC_POINTS: u32 = 4;

#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub(crate) struct Block([u64; QWORDS]);

impl Block {
    const ZERO: Block = Block([0; QWORDS]);
}

/// Errors (all reported as libsodium's failure, "internal error").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A parameter is outside RFC 9106's ranges.
    Params,
    /// The memory could not be allocated.
    Memory,
}

/// Parameters.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub t_cost: u32,
    pub m_cost_kib: u32,
    pub lanes: u32,
    pub variant: Variant,
    pub version: u32,
}

/// `H'` (RFC 9106 §3.3): variable-length hash.
fn h_prime(out: &mut [u8], parts: &[&[u8]]) {
    let t = out.len() as u32;
    if out.len() <= 64 {
        let mut st = Blake2b::new(out.len(), b"", None, None);
        st.update(&t.to_le_bytes());
        for p in parts {
            st.update(p);
        }
        st.finalize_into(out);
        return;
    }
    let mut v = [0u8; 64];
    let mut st = Blake2b::new(64, b"", None, None);
    st.update(&t.to_le_bytes());
    for p in parts {
        st.update(p);
    }
    st.finalize_into(&mut v);
    let r = out.len().div_ceil(32) - 2;
    out[..32].copy_from_slice(&v[..32]);
    let mut pos = 32;
    for _ in 1..r {
        let mut st = Blake2b::new(64, b"", None, None);
        st.update(&v);
        st.finalize_into(&mut v);
        out[pos..pos + 32].copy_from_slice(&v[..32]);
        pos += 32;
    }
    let last = out.len() - 32 * r;
    let mut st = Blake2b::new(last, b"", None, None);
    st.update(&v);
    st.finalize_into(&mut out[pos..]);
    crate::ct::wipe(&mut v);
}

#[inline(always)]
fn fbla(a: u64, b: u64) -> u64 {
    a.wrapping_add(b)
        .wrapping_add(2u64.wrapping_mul((a as u32 as u64) * (b as u32 as u64)))
}

macro_rules! gb {
    ($v:ident, $a:expr, $b:expr, $c:expr, $d:expr) => {
        $v[$a] = fbla($v[$a], $v[$b]);
        $v[$d] = ($v[$d] ^ $v[$a]).rotate_right(32);
        $v[$c] = fbla($v[$c], $v[$d]);
        $v[$b] = ($v[$b] ^ $v[$c]).rotate_right(24);
        $v[$a] = fbla($v[$a], $v[$b]);
        $v[$d] = ($v[$d] ^ $v[$a]).rotate_right(16);
        $v[$c] = fbla($v[$c], $v[$d]);
        $v[$b] = ($v[$b] ^ $v[$c]).rotate_right(63);
    };
}

#[inline(always)]
fn permute(v: &mut [u64; 16]) {
    gb!(v, 0, 4, 8, 12);
    gb!(v, 1, 5, 9, 13);
    gb!(v, 2, 6, 10, 14);
    gb!(v, 3, 7, 11, 15);
    gb!(v, 0, 5, 10, 15);
    gb!(v, 1, 6, 11, 12);
    gb!(v, 2, 7, 8, 13);
    gb!(v, 3, 4, 9, 14);
}

/// Portable compression `G` (RFC 9106 §3.5): `next = G(prev, refb)`, XORed
/// into `next`'s old contents when `xor` is set.
fn fill_block_portable(prev: &Block, refb: &Block, next: &mut Block, xor: bool) {
    let mut r = Block::ZERO;
    for i in 0..QWORDS {
        r.0[i] = prev.0[i] ^ refb.0[i];
    }
    let mut q = r;
    for row in 0..8 {
        let mut v: [u64; 16] = q.0[16 * row..16 * row + 16].try_into().unwrap();
        permute(&mut v);
        q.0[16 * row..16 * row + 16].copy_from_slice(&v);
    }
    for col in 0..8 {
        let mut v = [0u64; 16];
        for k in 0..8 {
            v[2 * k] = q.0[2 * col + 16 * k];
            v[2 * k + 1] = q.0[2 * col + 16 * k + 1];
        }
        permute(&mut v);
        for k in 0..8 {
            q.0[2 * col + 16 * k] = v[2 * k];
            q.0[2 * col + 16 * k + 1] = v[2 * k + 1];
        }
    }
    if xor {
        for i in 0..QWORDS {
            next.0[i] ^= q.0[i] ^ r.0[i];
        }
    } else {
        for i in 0..QWORDS {
            next.0[i] = q.0[i] ^ r.0[i];
        }
    }
}

#[cfg(target_arch = "x86_64")]
// The kernels are `unsafe fn`s only for their raw loads and stores.
#[allow(unsafe_op_in_unsafe_fn)]
mod avx2 {
    use super::{Block, QWORDS};
    use core::arch::x86_64::*;

    pub(super) fn available() -> bool {
        std::is_x86_feature_detected!("avx2")
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    fn fbla(a: __m256i, b: __m256i) -> __m256i {
        let m = _mm256_mul_epu32(a, b);
        _mm256_add_epi64(_mm256_add_epi64(a, b), _mm256_add_epi64(m, m))
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    fn rotr32(x: __m256i) -> __m256i {
        _mm256_shuffle_epi32(x, 0b10_11_00_01)
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    fn rotr24(x: __m256i) -> __m256i {
        let m = _mm256_setr_epi8(
            3, 4, 5, 6, 7, 0, 1, 2, 11, 12, 13, 14, 15, 8, 9, 10, 3, 4, 5, 6, 7, 0, 1, 2, 11, 12,
            13, 14, 15, 8, 9, 10,
        );
        _mm256_shuffle_epi8(x, m)
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    fn rotr16(x: __m256i) -> __m256i {
        let m = _mm256_setr_epi8(
            2, 3, 4, 5, 6, 7, 0, 1, 10, 11, 12, 13, 14, 15, 8, 9, 2, 3, 4, 5, 6, 7, 0, 1, 10, 11,
            12, 13, 14, 15, 8, 9,
        );
        _mm256_shuffle_epi8(x, m)
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    fn rotr63(x: __m256i) -> __m256i {
        _mm256_xor_si256(_mm256_add_epi64(x, x), _mm256_srli_epi64(x, 63))
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    fn g(a: &mut __m256i, b: &mut __m256i, c: &mut __m256i, d: &mut __m256i) {
        *a = fbla(*a, *b);
        *d = rotr32(_mm256_xor_si256(*d, *a));
        *c = fbla(*c, *d);
        *b = rotr24(_mm256_xor_si256(*b, *c));
        *a = fbla(*a, *b);
        *d = rotr16(_mm256_xor_si256(*d, *a));
        *c = fbla(*c, *d);
        *b = rotr63(_mm256_xor_si256(*b, *c));
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    fn diag(b: &mut __m256i, c: &mut __m256i, d: &mut __m256i) {
        *b = _mm256_permute4x64_epi64(*b, 0b00_11_10_01);
        *c = _mm256_permute4x64_epi64(*c, 0b01_00_11_10);
        *d = _mm256_permute4x64_epi64(*d, 0b10_01_00_11);
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    fn undiag(b: &mut __m256i, c: &mut __m256i, d: &mut __m256i) {
        *b = _mm256_permute4x64_epi64(*b, 0b10_01_00_11);
        *c = _mm256_permute4x64_epi64(*c, 0b01_00_11_10);
        *d = _mm256_permute4x64_epi64(*d, 0b00_11_10_01);
    }

    /// Two independent BlaMka rounds, interleaved so their dependency
    /// chains overlap. Each `[a, b, c, d]` holds the rows of a 4×4 word
    /// matrix.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn p2(x: &mut [__m256i; 4], y: &mut [__m256i; 4]) {
        let [xa, xb, xc, xd] = x;
        let [ya, yb, yc, yd] = y;
        g(xa, xb, xc, xd);
        g(ya, yb, yc, yd);
        diag(xb, xc, xd);
        diag(yb, yc, yd);
        g(xa, xb, xc, xd);
        g(ya, yb, yc, yd);
        undiag(xb, xc, xd);
        undiag(yb, yc, yd);
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    unsafe fn load2(lo: *const u64, hi: *const u64) -> __m256i {
        let l = _mm_loadu_si128(lo as *const __m128i);
        let h = _mm_loadu_si128(hi as *const __m128i);
        _mm256_inserti128_si256(_mm256_castsi128_si256(l), h, 1)
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    unsafe fn store2(lo: *mut u64, hi: *mut u64, v: __m256i) {
        _mm_storeu_si128(lo as *mut __m128i, _mm256_castsi256_si128(v));
        _mm_storeu_si128(hi as *mut __m128i, _mm256_extracti128_si256(v, 1));
    }

    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn fill_block<H: FnOnce(u64)>(
        prev: &Block,
        refb: &Block,
        next: &mut Block,
        xor: bool,
        hint: H,
    ) {
        use core::mem::MaybeUninit;
        // R = prev ⊕ ref, kept for the final XOR; Q is permuted in place.
        let mut r = MaybeUninit::<Block>::uninit();
        let mut q = MaybeUninit::<Block>::uninit();
        let rp = r.as_mut_ptr() as *mut u64;
        let qp = q.as_mut_ptr() as *mut u64;
        let pp = prev.0.as_ptr();
        let fp = refb.0.as_ptr();
        let ld = |p: *const u64, i: usize| _mm256_loadu_si256(p.add(i) as *const __m256i);
        for row in (0..8).step_by(2) {
            let mut x = [_mm256_setzero_si256(); 4];
            let mut y = [_mm256_setzero_si256(); 4];
            for k in 0..4 {
                let i = 16 * row + 4 * k;
                x[k] = _mm256_xor_si256(ld(pp, i), ld(fp, i));
                y[k] = _mm256_xor_si256(ld(pp, i + 16), ld(fp, i + 16));
                _mm256_store_si256(rp.add(i) as *mut __m256i, x[k]);
                _mm256_store_si256(rp.add(i + 16) as *mut __m256i, y[k]);
            }
            p2(&mut x, &mut y);
            for k in 0..4 {
                let i = 16 * row + 4 * k;
                _mm256_store_si256(qp.add(i) as *mut __m256i, x[k]);
                _mm256_store_si256(qp.add(i + 16) as *mut __m256i, y[k]);
            }
        }
        let np = next.0.as_mut_ptr();
        let w = |c: usize, k: usize| qp.add(2 * c + 16 * k);
        let mut hint = Some(hint);
        for col in (0..8).step_by(2) {
            let mut x = [_mm256_setzero_si256(); 4];
            let mut y = [_mm256_setzero_si256(); 4];
            for k in 0..4 {
                x[k] = load2(w(col, 2 * k), w(col, 2 * k + 1));
                y[k] = load2(w(col + 1, 2 * k), w(col + 1, 2 * k + 1));
            }
            p2(&mut x, &mut y);
            for k in 0..4 {
                store2(w(col, 2 * k), w(col, 2 * k + 1), x[k]);
                store2(w(col + 1, 2 * k), w(col + 1, 2 * k + 1), y[k]);
            }
            if let Some(h) = hint.take() {
                // Word 0 of the result is final once column 0 is done.
                let old = if xor { *np } else { 0 };
                h(*qp ^ *rp ^ old);
            }
        }
        for i in (0..QWORDS).step_by(4) {
            let mut v = _mm256_xor_si256(
                _mm256_load_si256(qp.add(i) as *const __m256i),
                _mm256_load_si256(rp.add(i) as *const __m256i),
            );
            if xor {
                v = _mm256_xor_si256(v, _mm256_load_si256(np.add(i) as *const __m256i));
            }
            _mm256_store_si256(np.add(i) as *mut __m256i, v);
        }
    }
}

struct Filler {
    #[cfg(target_arch = "x86_64")]
    avx2: bool,
}

impl Filler {
    fn new() -> Filler {
        Filler {
            #[cfg(target_arch = "x86_64")]
            avx2: avx2::available(),
        }
    }

    #[inline(always)]
    fn fill(&self, prev: &Block, refb: &Block, next: &mut Block, xor: bool) {
        self.fill_hint(prev, refb, next, xor, |_| {});
    }

    /// [`Filler::fill`], calling `hint` with the first word of the result as
    /// soon as it is known (before the end of the computation), so the next
    /// data-dependent reference can be prefetched.
    #[inline(always)]
    fn fill_hint<H: FnOnce(u64)>(
        &self,
        prev: &Block,
        refb: &Block,
        next: &mut Block,
        xor: bool,
        hint: H,
    ) {
        #[cfg(target_arch = "x86_64")]
        if self.avx2 {
            // SAFETY: AVX2 availability was checked at construction.
            unsafe { avx2::fill_block(prev, refb, next, xor, hint) };
            return;
        }
        fill_block_portable(prev, refb, next, xor);
        hint(next.0[0]);
    }
}

fn block_from_bytes(b: &[u8]) -> Block {
    let mut out = Block::ZERO;
    for (w, c) in out.0.iter_mut().zip(b.as_chunks::<8>().0) {
        *w = u64::from_le_bytes(*c);
    }
    out
}

/// Computes Argon2 into `out` (RFC 9106 §3.2).
pub fn hash(
    params: &Params,
    password: &[u8],
    salt: &[u8],
    secret: &[u8],
    ad: &[u8],
    out: &mut [u8],
) -> Result<(), Error> {
    let p = params.lanes;
    if p == 0 || p > 0x00ff_ffff || params.t_cost == 0 || out.len() < 4 || salt.len() < 8 {
        return Err(Error::Params);
    }
    if (params.m_cost_kib as u64) < 8 * p as u64 {
        return Err(Error::Params);
    }
    if params.version != 0x13 && params.version != 0x10 {
        return Err(Error::Params);
    }
    let seg_len = params.m_cost_kib / (SYNC_POINTS * p);
    let q = seg_len * SYNC_POINTS;
    let m_prime = q * p;
    let total = m_prime as usize;
    let mut mem = Memory::new(total).ok_or(Error::Memory)?;

    // H0.
    let mut h0 = [0u8; 72];
    {
        let mut st = Blake2b::new(64, b"", None, None);
        for v in [
            p,
            out.len() as u32,
            params.m_cost_kib,
            params.t_cost,
            params.version,
            params.variant as u32,
        ] {
            st.update(&v.to_le_bytes());
        }
        for part in [password, salt, secret, ad] {
            st.update(&(part.len() as u32).to_le_bytes());
            st.update(part);
        }
        st.finalize_into(&mut h0[..64]);
    }
    let mut buf = [0u8; 1024];
    for l in 0..p {
        for j in 0..2u32 {
            h0[64..68].copy_from_slice(&j.to_le_bytes());
            h0[68..72].copy_from_slice(&l.to_le_bytes());
            h_prime(&mut buf, &[&h0]);
            mem[(l * q + j) as usize] = block_from_bytes(&buf);
        }
    }
    crate::ct::wipe(&mut h0);

    let filler = Filler::new();
    let mut address = Block::ZERO;
    let mut input = Block::ZERO;
    let zero = Block::ZERO;
    let base = mem.as_mut_ptr();
    for pass in 0..params.t_cost {
        for slice in 0..SYNC_POINTS {
            for lane in 0..p {
                let data_independent = match params.variant {
                    Variant::Argon2i => true,
                    Variant::Argon2d => false,
                    Variant::Argon2id => pass == 0 && slice < 2,
                };
                if data_independent {
                    input = Block::ZERO;
                    input.0[0] = pass as u64;
                    input.0[1] = lane as u64;
                    input.0[2] = slice as u64;
                    input.0[3] = m_prime as u64;
                    input.0[4] = params.t_cost as u64;
                    input.0[5] = params.variant as u64;
                }
                let start = if pass == 0 && slice == 0 { 2 } else { 0 };
                if data_independent && start == 2 {
                    next_addresses(&filler, &mut address, &mut input, &zero);
                }
                // The reference block of position `idx` for pseudo-random `rand`
                // (RFC 9106 §3.4.1.2 and §3.4.2).
                let ref_index = |idx: u32, rand: u64| -> usize {
                    let j1 = rand & 0xffff_ffff;
                    let j2 = rand >> 32;
                    let ref_lane = if pass == 0 && slice == 0 {
                        lane
                    } else {
                        (j2 % p as u64) as u32
                    };
                    let same = ref_lane == lane;
                    let area: u64 = if pass == 0 {
                        if same {
                            (slice * seg_len + idx - 1) as u64
                        } else {
                            (slice * seg_len) as u64 - if idx == 0 { 1 } else { 0 }
                        }
                    } else if same {
                        (q - seg_len + idx - 1) as u64
                    } else {
                        (q - seg_len) as u64 - if idx == 0 { 1 } else { 0 }
                    };
                    let x = (j1 * j1) >> 32;
                    let y = (area * x) >> 32;
                    let zz = area - 1 - y;
                    let startpos: u64 = if pass == 0 || slice == SYNC_POINTS - 1 {
                        0
                    } else {
                        ((slice + 1) * seg_len) as u64
                    };
                    let ref_col = ((startpos + zz) % q as u64) as u32;
                    (ref_lane * q + ref_col) as usize
                };
                let xor = pass > 0 && params.version == 0x13;
                for idx in start..seg_len {
                    let col = slice * seg_len + idx;
                    let cur = (lane * q + col) as usize;
                    let prev = if col == 0 {
                        cur + q as usize - 1
                    } else {
                        cur - 1
                    };
                    let rand = if data_independent {
                        if idx % QWORDS as u32 == 0 {
                            next_addresses(&filler, &mut address, &mut input, &zero);
                        }
                        address.0[(idx % QWORDS as u32) as usize]
                    } else {
                        // SAFETY: `prev` is in bounds.
                        unsafe { (*base.add(prev)).0[0] }
                    };
                    let refi = ref_index(idx, rand);
                    // The reference area excludes the block being computed.
                    assert!(
                        cur != prev && cur != refi && prev < total && refi < total && cur < total
                    );
                    if data_independent && idx + 1 < seg_len && (idx + 1) % QWORDS as u32 != 0 {
                        // The next reference is known already: start loading it.
                        let next =
                            ref_index(idx + 1, address.0[((idx + 1) % QWORDS as u32) as usize]);
                        if next < total {
                            // SAFETY: in bounds.
                            prefetch(unsafe { base.add(next) });
                        }
                    }
                    // SAFETY: the three indices are in bounds and `cur` is
                    // distinct from the two blocks only read.
                    let (pb, rb, cb) =
                        unsafe { (&*base.add(prev), &*base.add(refi), &mut *base.add(cur)) };
                    if !data_independent && idx + 1 < seg_len {
                        // The next reference depends on this block's first
                        // word: prefetch it as soon as that word is known.
                        filler.fill_hint(pb, rb, cb, xor, |w0| {
                            let next = ref_index(idx + 1, w0);
                            if next < total {
                                // SAFETY: in bounds.
                                prefetch(unsafe { base.add(next) });
                            }
                        });
                    } else {
                        filler.fill(pb, rb, cb, xor);
                    }
                }
            }
        }
    }
    let mut c = mem[(q - 1) as usize];
    for l in 1..p {
        let last = &mem[(l * q + q - 1) as usize];
        for i in 0..QWORDS {
            c.0[i] ^= last.0[i];
        }
    }
    for (chunk, w) in buf.as_chunks_mut::<8>().0.iter_mut().zip(c.0.iter()) {
        *chunk = w.to_le_bytes();
    }
    h_prime(out, &[&buf]);
    crate::ct::wipe(&mut buf);
    c.0.fill(0);
    std::hint::black_box(&c);
    drop(mem);
    Ok(())
}

/// Starts loading a block into the cache.
#[inline(always)]
fn prefetch(b: *const Block) {
    #[cfg(target_arch = "x86_64")]
    for line in 0..16 {
        // SAFETY: prefetching does not access memory architecturally; the
        // address is within the allocation.
        unsafe {
            core::arch::x86_64::_mm_prefetch::<{ core::arch::x86_64::_MM_HINT_T0 }>(
                (b as *const i8).add(64 * line),
            )
        };
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = b;
}

/// The memory blocks: anonymous pages on Linux (with transparent huge
/// pages requested: Argon2's random reads over the whole area are
/// TLB-bound with 4 KiB pages), the global allocator elsewhere. The pages
/// go back to the OS on drop; the allocator path wipes them first.
struct Memory {
    ptr: *mut Block,
    len: usize,
    #[cfg(target_os = "linux")]
    mapped: bool,
}

impl Memory {
    fn new(n: usize) -> Option<Memory> {
        let bytes = n.checked_mul(core::mem::size_of::<Block>())?;
        if bytes == 0 || bytes > isize::MAX as usize {
            return None;
        }
        #[cfg(target_os = "linux")]
        {
            // SAFETY: an anonymous private mapping; the result is checked.
            // Without MAP_NORESERVE, a size the system cannot back fails
            // here (an "internal error", as libsodium's malloc() failure)
            // rather than at first touch.
            let p = unsafe {
                libc::mmap(
                    core::ptr::null_mut(),
                    bytes,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };
            if p != libc::MAP_FAILED {
                // SAFETY: advice on our own mapping; failure is harmless.
                unsafe { libc::madvise(p, bytes, libc::MADV_HUGEPAGE) };
                return Some(Memory {
                    ptr: p as *mut Block,
                    len: n,
                    mapped: true,
                });
            }
            None
        }
        #[cfg(not(target_os = "linux"))]
        {
            let layout = std::alloc::Layout::array::<Block>(n).ok()?;
            // SAFETY: non-zero size; checked for null.
            let p = unsafe { std::alloc::alloc_zeroed(layout) } as *mut Block;
            if p.is_null() {
                return None;
            }
            Some(Memory { ptr: p, len: n })
        }
    }

    fn as_mut_ptr(&mut self) -> *mut Block {
        self.ptr
    }
}

impl core::ops::Index<usize> for Memory {
    type Output = Block;
    fn index(&self, i: usize) -> &Block {
        assert!(i < self.len);
        // SAFETY: in bounds of the allocation (zero-initialized).
        unsafe { &*self.ptr.add(i) }
    }
}

impl core::ops::IndexMut<usize> for Memory {
    fn index_mut(&mut self, i: usize) -> &mut Block {
        assert!(i < self.len);
        // SAFETY: in bounds, uniquely borrowed through `self`.
        unsafe { &mut *self.ptr.add(i) }
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        let bytes = self.len * core::mem::size_of::<Block>();
        #[cfg(target_os = "linux")]
        if self.mapped {
            // SAFETY: unmapping our own mapping, not used afterwards. The
            // kernel discards the pages.
            unsafe { libc::munmap(self.ptr as *mut libc::c_void, bytes) };
            return;
        }
        // SAFETY: the allocation is ours; wipe it before it is reused.
        unsafe {
            core::ptr::write_bytes(self.ptr as *mut u8, 0, bytes);
            std::hint::black_box(self.ptr);
            #[cfg(not(target_os = "linux"))]
            std::alloc::dealloc(
                self.ptr as *mut u8,
                std::alloc::Layout::array::<Block>(self.len).unwrap(),
            );
        }
    }
}

fn next_addresses(f: &Filler, address: &mut Block, input: &mut Block, zero: &Block) {
    input.0[6] += 1;
    let mut tmp = Block::ZERO;
    f.fill(zero, input, &mut tmp, false);
    f.fill(zero, &tmp, address, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(variant: Variant) -> Vec<u8> {
        // RFC 9106 §5 test parameters.
        let params = Params {
            t_cost: 3,
            m_cost_kib: 32,
            lanes: 4,
            variant,
            version: 0x13,
        };
        let mut out = vec![0u8; 32];
        hash(&params, &[1; 32], &[2; 16], &[3; 8], &[4; 12], &mut out).unwrap();
        out
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn rfc9106_vectors() {
        assert_eq!(
            hex(&run(Variant::Argon2d)),
            "512b391b6f1162975371d30919734294f868e3be3984f3c1a13a4db9fabe4acb"
        );
        assert_eq!(
            hex(&run(Variant::Argon2i)),
            "c814d9d1dc7f37aa13f0d77f2494bda1c8de6b016dd388d29952a4c4672b6ce8"
        );
        assert_eq!(
            hex(&run(Variant::Argon2id)),
            "0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659"
        );
    }

    #[test]
    #[ignore]
    fn bench_fill() {
        let a = Block([3; QWORDS]);
        let b = Block([5; QWORDS]);
        let mut c = Block([7; QWORDS]);
        let f = Filler::new();
        let t = std::time::Instant::now();
        for _ in 0..200_000 {
            f.fill(&a, &b, &mut c, true);
            std::hint::black_box(&mut c);
        }
        eprintln!(
            "fill_block: {:.1} ns",
            t.elapsed().as_nanos() as f64 / 200_000.0
        );
        let t = std::time::Instant::now();
        for _ in 0..200_000 {
            fill_block_portable(&a, &b, &mut c, true);
            std::hint::black_box(&mut c);
        }
        eprintln!(
            "portable: {:.1} ns",
            t.elapsed().as_nanos() as f64 / 200_000.0
        );
        for t_cost in [1, 2, 3] {
            for variant in [Variant::Argon2id, Variant::Argon2i] {
                let params = Params {
                    t_cost,
                    m_cost_kib: 65536,
                    lanes: 1,
                    variant,
                    version: 0x13,
                };
                let mut out = [0u8; 32];
                let mut best = f64::MAX;
                for _ in 0..3 {
                    let t = std::time::Instant::now();
                    hash(&params, b"password", &[1; 16], &[], &[], &mut out).unwrap();
                    best = best.min(t.elapsed().as_secs_f64() * 1e3);
                }
                eprintln!("{variant:?} t={t_cost}: {best:.2} ms");
            }
        }
    }

    #[test]
    fn portable_matches_simd() {
        let mut a = Block::ZERO;
        let mut b = Block::ZERO;
        for i in 0..QWORDS {
            a.0[i] = (i as u64).wrapping_mul(0x9e3779b97f4a7c15);
            b.0[i] = (i as u64 + 7).wrapping_mul(0xc2b2ae3d27d4eb4f);
        }
        let f = Filler::new();
        for xor in [false, true] {
            let mut x = a;
            let mut y = a;
            fill_block_portable(&a, &b, &mut x, xor);
            f.fill(&a, &b, &mut y, xor);
            assert_eq!(x.0, y.0);
        }
    }
}
