//! Port of bitvec.c.
//!
//! A Bitvec is a bitmap of a fixed (but possibly very large) size that is
//! optimised for the case where only a few bits are set.  The structure
//! layout (bitmap / hash table / recursive sub-bitmaps) and the constants
//! are those of the 64-bit C build (BITVEC_SZ of 512 bytes), so that the
//! behaviour is identical.

use crate::consts::*;

/// Size of the Bitvec structure in bytes.
const BITVEC_SZ: usize = 512;
/// Round the union size down to the nearest pointer boundary (64-bit).
const BITVEC_USIZE: usize = ((BITVEC_SZ - (3 * 4)) / 8) * 8;
/// Size, in bits, of the bitmap element.
const BITVEC_SZELEM: u32 = 8;
/// Number of elements in a bitmap array.
const BITVEC_NELEM: usize = BITVEC_USIZE;
/// Number of bits in the bitmap array.
const BITVEC_NBIT: u32 = (BITVEC_NELEM as u32) * BITVEC_SZELEM;
/// Number of u32 values in hash table.
const BITVEC_NINT: u32 = (BITVEC_USIZE / 4) as u32;
/// Maximum number of entries in hash table before sub-dividing and
/// re-hashing.
const BITVEC_MXHASH: u32 = BITVEC_NINT / 2;
/// Number of sub-bitvec pointers.
const BITVEC_NPTR: u32 = (BITVEC_USIZE / 8) as u32;

#[inline]
fn BITVEC_HASH(x: u32) -> u32 {
    x.wrapping_mul(1) % BITVEC_NINT
}

/// The C union of the three representations.
enum BitvecU {
    Bitmap(Box<[u8; BITVEC_NELEM]>),
    Hash(Box<[u32; BITVEC_NINT as usize]>),
    Sub(Vec<Option<Box<Bitvec>>>),
}

/// A bitmap object (C struct Bitvec).
pub struct Bitvec {
    /// Maximum bit index.
    iSize: u32,
    /// Number of bits that are set - only valid for aHash element.
    nSet: u32,
    /// Number of bits handled by each apSub[] entry.
    iDivisor: u32,
    u: BitvecU,
}

impl Bitvec {
    fn new(iSize: u32) -> Bitvec {
        Bitvec {
            iSize,
            nSet: 0,
            iDivisor: 0,
            u: if iSize <= BITVEC_NBIT {
                BitvecU::Bitmap(Box::new([0u8; BITVEC_NELEM]))
            } else {
                BitvecU::Hash(Box::new([0u32; BITVEC_NINT as usize]))
            },
        }
    }
    fn aHash(&mut self) -> &mut [u32; BITVEC_NINT as usize] {
        if !matches!(self.u, BitvecU::Hash(_)) {
            self.u = BitvecU::Hash(Box::new([0u32; BITVEC_NINT as usize]));
        }
        match &mut self.u {
            BitvecU::Hash(h) => h,
            _ => unreachable!(),
        }
    }
}

/// C: `Bitvec * sqlite3BitvecCreate(u32 iSize)`
pub fn sqlite3BitvecCreate(iSize: u32) -> Option<Box<crate::bitvec::Bitvec>> {
    Some(Box::new(Bitvec::new(iSize)))
}

/// C: `int sqlite3BitvecTestNotNull(Bitvec *p, u32 i)`
pub fn sqlite3BitvecTestNotNull(p: &mut crate::bitvec::Bitvec, i: u32) -> i32 {
    let mut p: &Bitvec = p;
    let mut i = i.wrapping_sub(1);
    if i >= p.iSize {
        return 0;
    }
    while p.iDivisor != 0 {
        let bin = i / p.iDivisor;
        i %= p.iDivisor;
        match &p.u {
            BitvecU::Sub(a) => match &a[bin as usize] {
                Some(s) => p = s,
                None => return 0,
            },
            _ => return 0,
        }
    }
    if p.iSize <= BITVEC_NBIT {
        match &p.u {
            BitvecU::Bitmap(b) => {
                ((b[(i / BITVEC_SZELEM) as usize] & (1 << (i & (BITVEC_SZELEM - 1)))) != 0) as i32
            }
            _ => 0,
        }
    } else {
        let a = match &p.u {
            BitvecU::Hash(h) => h,
            _ => return 0,
        };
        let mut h = BITVEC_HASH(i);
        i += 1;
        while a[h as usize] != 0 {
            if a[h as usize] == i {
                return 1;
            }
            h = (h + 1) % BITVEC_NINT;
        }
        0
    }
}

/// C: `int sqlite3BitvecTest(Bitvec *p, u32 i)`
pub fn sqlite3BitvecTest(p: &mut crate::bitvec::Bitvec, i: u32) -> i32 {
    sqlite3BitvecTestNotNull(p, i)
}

/// sqlite3BitvecTest() on a possibly-NULL Bitvec.
pub fn sqlite3BitvecTestOpt(p: Option<&mut crate::bitvec::Bitvec>, i: u32) -> i32 {
    match p {
        Some(p) => sqlite3BitvecTestNotNull(p, i),
        None => 0,
    }
}

/// C: `int sqlite3BitvecSet(Bitvec *p, u32 i)`
pub fn sqlite3BitvecSet(p: &mut crate::bitvec::Bitvec, i: u32) -> i32 {
    debug_assert!(i > 0);
    debug_assert!(i <= p.iSize);
    let mut p: &mut Bitvec = p;
    let mut i = i - 1;
    while p.iSize > BITVEC_NBIT && p.iDivisor != 0 {
        let bin = (i / p.iDivisor) as usize;
        i %= p.iDivisor;
        let iDivisor = p.iDivisor;
        let a = match &mut p.u {
            BitvecU::Sub(a) => a,
            _ => unreachable!(),
        };
        if a[bin].is_none() {
            a[bin] = Some(Box::new(Bitvec::new(iDivisor)));
        }
        p = a[bin].as_mut().unwrap();
    }
    if p.iSize <= BITVEC_NBIT {
        if let BitvecU::Bitmap(b) = &mut p.u {
            b[(i / BITVEC_SZELEM) as usize] |= 1 << (i & (BITVEC_SZELEM - 1));
        }
        return SQLITE_OK;
    }
    let mut h = BITVEC_HASH(i);
    i += 1;
    let nSet = p.nSet;
    let a = p.aHash();
    let rehash;
    // if there wasn't a hash collision, and this doesn't completely fill
    // the hash, then just add it without worrying about sub-dividing and
    // re-hashing.
    if a[h as usize] == 0 {
        rehash = nSet >= BITVEC_NINT - 1;
    } else {
        // there was a collision, check to see if it's already in hash, if
        // not, try to find a spot for it
        loop {
            if a[h as usize] == i {
                return SQLITE_OK;
            }
            h += 1;
            if h >= BITVEC_NINT {
                h = 0;
            }
            if a[h as usize] == 0 {
                break;
            }
        }
        rehash = true;
    }
    // we didn't find it in the hash.  h points to the first available free
    // spot. check to see if this is going to make our hash too "full".
    if rehash && nSet >= BITVEC_MXHASH {
        let aiValues: [u32; BITVEC_NINT as usize] = *a;
        let mut sub = Vec::with_capacity(BITVEC_NPTR as usize);
        sub.resize_with(BITVEC_NPTR as usize, || None);
        p.u = BitvecU::Sub(sub);
        p.iDivisor = p.iSize / BITVEC_NPTR;
        if p.iSize % BITVEC_NPTR != 0 {
            p.iDivisor += 1;
        }
        if p.iDivisor < BITVEC_NBIT {
            p.iDivisor = BITVEC_NBIT;
        }
        let mut rc = sqlite3BitvecSet(p, i);
        for j in 0..BITVEC_NINT as usize {
            if aiValues[j] != 0 {
                rc |= sqlite3BitvecSet(p, aiValues[j]);
            }
        }
        return rc;
    }
    p.nSet += 1;
    p.aHash()[h as usize] = i;
    SQLITE_OK
}

/// sqlite3BitvecSet() on a possibly-NULL Bitvec.
pub fn sqlite3BitvecSetOpt(p: Option<&mut crate::bitvec::Bitvec>, i: u32) -> i32 {
    match p {
        Some(p) => sqlite3BitvecSet(p, i),
        None => SQLITE_OK,
    }
}

/// C: `void sqlite3BitvecClear(Bitvec *p, u32 i, void *pBuf)`
///
/// `pBuf` (scratch space in C) is not used.
pub fn sqlite3BitvecClear(p: &mut crate::bitvec::Bitvec, i: u32, pBuf: &mut [u8]) {
    let _ = pBuf;
    debug_assert!(i > 0);
    let mut p: &mut Bitvec = p;
    let mut i = i - 1;
    while p.iDivisor != 0 {
        let bin = (i / p.iDivisor) as usize;
        i %= p.iDivisor;
        let a = match &mut p.u {
            BitvecU::Sub(a) => a,
            _ => return,
        };
        match &mut a[bin] {
            Some(s) => p = s,
            None => return,
        }
    }
    if p.iSize <= BITVEC_NBIT {
        if let BitvecU::Bitmap(b) = &mut p.u {
            b[(i / BITVEC_SZELEM) as usize] &= !(1u8 << (i & (BITVEC_SZELEM - 1)));
        }
    } else {
        let a = p.aHash();
        let aiValues: [u32; BITVEC_NINT as usize] = *a;
        *a = [0u32; BITVEC_NINT as usize];
        let mut nSet = 0;
        for j in 0..BITVEC_NINT as usize {
            if aiValues[j] != 0 && aiValues[j] != i + 1 {
                let mut h = BITVEC_HASH(aiValues[j] - 1);
                nSet += 1;
                while a[h as usize] != 0 {
                    h += 1;
                    if h >= BITVEC_NINT {
                        h = 0;
                    }
                }
                a[h as usize] = aiValues[j];
            }
        }
        p.nSet = nSet;
    }
}

/// C: `void sqlite3BitvecDestroy(Bitvec *p)`
///
/// Memory is released when the Box is dropped; nothing to do.
pub fn sqlite3BitvecDestroy(p: &mut crate::bitvec::Bitvec) {
    let _ = p;
}

/// C: `u32 sqlite3BitvecSize(Bitvec *p)`
pub fn sqlite3BitvecSize(p: &mut crate::bitvec::Bitvec) -> u32 {
    p.iSize
}

/// C: `void sqlite3ShowBitvec(Bitvec *p)` (SQLITE_DEBUG only in C)
pub fn sqlite3ShowBitvec(p: &mut crate::bitvec::Bitvec) {
    fn show(p: Option<&mut Bitvec>, n: usize, x: u32) {
        let p = match p {
            None => {
                println!("NULL");
                return;
            }
            Some(p) => p,
        };
        print!("Bitvec {:p} iSize={}", p as *const Bitvec, p.iSize);
        if p.iSize <= BITVEC_NBIT {
            println!(" bitmap");
            print!("{:n$}   bits:", "", n = n);
            for i in 1..=BITVEC_NBIT {
                if sqlite3BitvecTest(p, i) != 0 {
                    print!(" {}", x + i);
                }
            }
            println!();
        } else if p.iDivisor == 0 {
            println!(" hash with {} entries", p.nSet);
            print!("{:n$}   bits:", "", n = n);
            for i in 0..BITVEC_NINT as usize {
                let v = p.aHash()[i];
                if v != 0 {
                    print!(" {}", x + v);
                }
            }
            println!();
        } else {
            println!(" sub-bitvec with iDivisor={}", p.iDivisor);
            let d = p.iDivisor;
            if let BitvecU::Sub(a) = &mut p.u {
                for (i, s) in a.iter_mut().enumerate() {
                    if s.is_none() {
                        continue;
                    }
                    print!("{:n$}   apSub[{}]=", "", i, n = n);
                    show(s.as_deref_mut(), n + 4, (i as u32) * d);
                }
            }
        }
    }
    show(Some(p), 0, 0);
}

/// C: `int sqlite3BitvecBuiltinTest(int sz, int *aOp)`
///
/// Signature change: `aOp` is the whole program (`&mut [i32]`).
pub fn sqlite3BitvecBuiltinTest(sz: i32, aOp: &mut [i32]) -> i32 {
    let mut pBitvec;
    let mut pV: Option<Vec<u8>>;
    if sz <= 0 {
        pBitvec = sqlite3BitvecCreate(2u32.wrapping_mul((-sz) as u32)).unwrap();
        pV = None;
    } else {
        pBitvec = sqlite3BitvecCreate(sz as u32).unwrap();
        pV = Some(vec![0u8; ((7 + sz as i64) / 8 + 1) as usize]);
    }
    let mut pTmpSpace = vec![0u8; BITVEC_SZ];
    let mut rc: i32;

    // NULL pBitvec tests
    sqlite3BitvecSetOpt(None, 1);

    let mut pc: usize = 0;
    let mut i: i32;
    loop {
        let op = aOp[pc];
        if op == 0 {
            break;
        }
        if op >= 6 {
            pc += 1;
            continue;
        }
        let mut nx;
        match op {
            1 | 2 | 5 => {
                nx = 4;
                i = aOp[pc + 2] - 1;
                aOp[pc + 2] = aOp[pc + 2].wrapping_add(aOp[pc + 3]);
            }
            _ => {
                nx = 2;
                let mut b = [0u8; 4];
                crate::random::sqlite3_randomness(4, &mut b);
                i = i32::from_ne_bytes(b);
            }
        }
        aOp[pc + 1] -= 1;
        if aOp[pc + 1] > 0 {
            nx = 0;
        }
        pc += nx;
        i = (i & 0x7fffffff) % sz;
        if (op & 1) != 0 {
            if let Some(v) = &mut pV {
                let j = (i + 1) as usize;
                v[j >> 3] |= 1 << (j & 7);
            }
            if op != 5 {
                if sqlite3BitvecSet(&mut pBitvec, (i + 1) as u32) != 0 {
                    return -1;
                }
            }
        } else {
            if let Some(v) = &mut pV {
                let j = (i + 1) as usize;
                v[j >> 3] &= !(1u8 << (j & 7));
            }
            sqlite3BitvecClear(&mut pBitvec, (i + 1) as u32, &mut pTmpSpace);
        }
    }

    // Test to make sure the linear array exactly matches the Bitvec object.
    if let Some(v) = &pV {
        rc = sqlite3BitvecTestOpt(None, 0)
            + sqlite3BitvecTest(&mut pBitvec, (sz + 1) as u32)
            + sqlite3BitvecTest(&mut pBitvec, 0)
            + (sqlite3BitvecSize(&mut pBitvec) as i32 - sz);
        for i in 1..=sz {
            let j = i as usize;
            let a = ((v[j >> 3] & (1 << (j & 7))) != 0) as i32;
            if a != sqlite3BitvecTest(&mut pBitvec, i as u32) {
                rc = i;
                break;
            }
        }
    } else {
        rc = 0;
    }
    rc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(sz: u32, ops: &[(bool, u32)]) {
        let mut b = sqlite3BitvecCreate(sz).unwrap();
        let mut v = vec![false; sz as usize + 1];
        for &(set, i) in ops {
            if set {
                assert_eq!(sqlite3BitvecSet(&mut b, i), SQLITE_OK);
                v[i as usize] = true;
            } else {
                sqlite3BitvecClear(&mut b, i, &mut []);
                v[i as usize] = false;
            }
        }
        for i in 1..=sz {
            assert_eq!(
                sqlite3BitvecTest(&mut b, i) != 0,
                v[i as usize],
                "bit {}",
                i
            );
        }
        assert_eq!(sqlite3BitvecTest(&mut b, 0), 0);
        assert_eq!(sqlite3BitvecTest(&mut b, sz + 1), 0);
    }

    #[test]
    fn bitvec_small_and_large() {
        naive(400, &[(true, 1), (true, 400), (true, 77), (false, 77)]);
        let mut ops = Vec::new();
        let mut x: u32 = 12345;
        for k in 0..5000 {
            x = x.wrapping_mul(1103515245).wrapping_add(12345);
            ops.push((k % 7 != 3, 1 + (x >> 8) % 100000));
        }
        naive(100000, &ops);
        let ops: Vec<_> = (1..=4000).map(|i| (true, i * 3)).collect();
        naive(4_000_000, &ops);
        let mut ops: Vec<_> = (1..=300).map(|i| (true, i * 1000)).collect();
        ops.extend((1..=150).map(|i| (false, i * 2000)));
        naive(400_000, &ops);
    }

    #[test]
    fn bitvec_builtin() {
        // The test-program format of sqlite3BitvecBuiltinTest
        let mut prog = [1, 400, 1, 1, 2, 400, 1, 1, 0];
        assert_eq!(sqlite3BitvecBuiltinTest(400, &mut prog), 0);
        let mut prog = [1, 4000, 1, 1, 2, 4000, 1, 1, 0];
        assert_eq!(sqlite3BitvecBuiltinTest(4000, &mut prog), 0);
        let mut prog = [1, 5000, 1, 1, 2, 2500, 1, 2, 0];
        assert_eq!(sqlite3BitvecBuiltinTest(50000, &mut prog), 0);
    }
}
