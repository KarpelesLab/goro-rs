//! Port of random.c: the pseudo-random number generator (ChaCha20).
//!
//! All threads share a single random number generator (a global protected
//! by a mutex, as the C SQLITE_MUTEX_STATIC_PRNG).
//!
//! Rust: the generator is seeded as the unix VFS xRandomness() does
//! (44 bytes from /dev/urandom, or the time and pid if that cannot be
//! opened), honouring sqlite3Config.iPrngSeed like sqlite3OsRandomness().
//! The VFS object is not consulted (there is no global VFS registry handle
//! here; sqlite3_vfs_find() would be the C path).

use std::sync::Mutex;

/// The current state of the generator.
#[derive(Clone, Copy)]
struct sqlite3PrngType {
    /// 64 bytes of chacha20 state
    s: [u32; 16],
    /// Output bytes
    out: [u8; 64],
    /// Output bytes remaining
    n: u8,
}

const PRNG_INIT: sqlite3PrngType = sqlite3PrngType {
    s: [0; 16],
    out: [0; 64],
    n: 0,
};

static sqlite3Prng: Mutex<sqlite3PrngType> = Mutex::new(PRNG_INIT);
static sqlite3SavedPrng: Mutex<sqlite3PrngType> = Mutex::new(PRNG_INIT);

#[inline]
fn QR(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[a] = x[a].wrapping_add(x[b]);
    x[d] ^= x[a];
    x[d] = x[d].rotate_left(16);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] ^= x[c];
    x[b] = x[b].rotate_left(12);
    x[a] = x[a].wrapping_add(x[b]);
    x[d] ^= x[a];
    x[d] = x[d].rotate_left(8);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] ^= x[c];
    x[b] = x[b].rotate_left(7);
}

/// The RFC-7539 ChaCha20 block function
fn chacha_block(out: &mut [u8; 64], inp: &[u32; 16]) {
    let mut x = *inp;
    for _ in 0..10 {
        QR(&mut x, 0, 4, 8, 12);
        QR(&mut x, 1, 5, 9, 13);
        QR(&mut x, 2, 6, 10, 14);
        QR(&mut x, 3, 7, 11, 15);
        QR(&mut x, 0, 5, 10, 15);
        QR(&mut x, 1, 6, 11, 12);
        QR(&mut x, 2, 7, 8, 13);
        QR(&mut x, 3, 4, 9, 14);
    }
    for i in 0..16 {
        // (u32*)out: native byte order
        out[i * 4..i * 4 + 4].copy_from_slice(&x[i].wrapping_add(inp[i]).to_ne_bytes());
    }
}

/// sqlite3OsRandomness(pVfs, nByte, zBuf) with the unix VFS.
fn osRandomness(zBuf: &mut [u8]) {
    for b in zBuf.iter_mut() {
        *b = 0;
    }
    let seed = crate::global::sqlite3GlobalConfig().iPrngSeed;
    if seed != 0 {
        let s = seed.to_ne_bytes();
        let n = zBuf.len().min(s.len());
        zBuf[..n].copy_from_slice(&s[..n]);
        return;
    }
    use std::io::Read;
    match std::fs::File::open("/dev/urandom") {
        Ok(mut f) => {
            let _ = f.read(zBuf);
        }
        Err(_) => {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let tb = t.to_ne_bytes();
            zBuf[..8].copy_from_slice(&tb);
            let pid = std::process::id() as i32;
            zBuf[8..12].copy_from_slice(&pid.to_ne_bytes());
        }
    }
}

/// C: `void sqlite3_randomness(int N, void *pBuf)`
///
/// Return N random bytes.  N<=0 (or an empty buffer) resets the generator
/// so that it is reseeded on the next call.
pub fn sqlite3_randomness(N: i32, pBuf: &mut [u8]) {
    let mut wsdPrng = sqlite3Prng.lock().unwrap_or_else(|e| e.into_inner());
    if N <= 0 || pBuf.is_empty() {
        wsdPrng.s[0] = 0;
        return;
    }
    let mut N = (N as usize).min(pBuf.len());

    // Initialize the state of the random number generator once, the first
    // time this routine is called.
    if wsdPrng.s[0] == 0 {
        const chacha20_init: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];
        wsdPrng.s[..4].copy_from_slice(&chacha20_init);
        let mut seed = [0u8; 44];
        osRandomness(&mut seed);
        for k in 0..11 {
            wsdPrng.s[4 + k] = u32::from_ne_bytes([
                seed[k * 4],
                seed[k * 4 + 1],
                seed[k * 4 + 2],
                seed[k * 4 + 3],
            ]);
        }
        wsdPrng.s[15] = wsdPrng.s[12];
        wsdPrng.s[12] = 0;
        wsdPrng.n = 0;
    }

    let mut zBuf = 0usize;
    loop {
        let n = wsdPrng.n as usize;
        if N <= n {
            pBuf[zBuf..zBuf + N].copy_from_slice(&wsdPrng.out[n - N..n]);
            wsdPrng.n -= N as u8;
            break;
        }
        if n > 0 {
            pBuf[zBuf..zBuf + n].copy_from_slice(&wsdPrng.out[..n]);
            N -= n;
            zBuf += n;
        }
        wsdPrng.s[12] = wsdPrng.s[12].wrapping_add(1);
        let s = wsdPrng.s;
        chacha_block(&mut wsdPrng.out, &s);
        wsdPrng.n = 64;
    }
}

/// C: `void sqlite3PrngSaveState(void)`
pub fn sqlite3PrngSaveState() {
    let cur = *sqlite3Prng.lock().unwrap_or_else(|e| e.into_inner());
    *sqlite3SavedPrng.lock().unwrap_or_else(|e| e.into_inner()) = cur;
}

/// C: `void sqlite3PrngRestoreState(void)`
pub fn sqlite3PrngRestoreState() {
    let saved = *sqlite3SavedPrng.lock().unwrap_or_else(|e| e.into_inner());
    *sqlite3Prng.lock().unwrap_or_else(|e| e.into_inner()) = saved;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chacha20_rfc7539_block() {
        // RFC 7539 section 2.3.2 test vector.
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        let nonce: [u8; 12] = [0, 0, 0, 9, 0, 0, 0, 0x4a, 0, 0, 0, 0];
        let mut s = [0u32; 16];
        s[..4].copy_from_slice(&[0x61707865, 0x3320646e, 0x79622d32, 0x6b206574]);
        for k in 0..8 {
            s[4 + k] = u32::from_le_bytes(key[k * 4..k * 4 + 4].try_into().unwrap());
        }
        s[12] = 1;
        for k in 0..3 {
            s[13 + k] = u32::from_le_bytes(nonce[k * 4..k * 4 + 4].try_into().unwrap());
        }
        let mut out = [0u8; 64];
        chacha_block(&mut out, &s);
        assert_eq!(&out[..8], &[0x10, 0xf1, 0xe7, 0xe4, 0xd1, 0x3b, 0x59, 0x15]);
    }

    #[test]
    fn randomness_save_restore() {
        let mut a = [0u8; 100];
        sqlite3_randomness(100, &mut a);
        sqlite3PrngSaveState();
        let mut b = [0u8; 37];
        sqlite3_randomness(37, &mut b);
        sqlite3PrngRestoreState();
        let mut c = [0u8; 37];
        sqlite3_randomness(37, &mut c);
        assert_eq!(b, c);
    }
}
