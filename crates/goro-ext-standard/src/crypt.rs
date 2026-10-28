//! `crypt()` hash schemes: traditional and extended (BSDi) DES, MD5-crypt,
//! bcrypt and SHA-256/SHA-512 crypt.
//!
//! Sources: FIPS 46-3 (DES and its tables), the crypt(3) DES scheme as
//! documented for Unix systems (salted E-box, 25 iterations, BSDi `_`
//! extension with key folding), the MD5-crypt scheme of FreeBSD
//! (documented in its crypt(3) page and many references), "A Future-
//! Adaptable Password Scheme" (Provos and Mazières, 1999) for bcrypt with
//! Blowfish (Schneier, 1993; initial boxes are the hexadecimal digits of
//! pi, computed here), and Ulrich Drepper's "Unix crypt using SHA-256 and
//! SHA-512" specification. PHP behaviours (failure strings, salt
//! handling) observed on PHP 8.5 and in tests ext/standard/tests/strings/
//! crypt*.phpt and ext/standard/tests/crypt/*.phpt.

use std::sync::OnceLock;

const ITOA64: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BCRYPT64: &[u8; 64] = b"./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

fn ascii_to_bin(c: u8) -> Option<u32> {
    ITOA64.iter().position(|&x| x == c).map(|p| p as u32)
}

// ---------------------------------------------------------------- DES

const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6,
    64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61,
    53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];
const FP: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62, 30,
    37, 5, 45, 13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19, 59, 27,
    34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
];
const E: [u8; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18,
    19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1,
];
const P: [u8; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19,
    13, 30, 6, 22, 11, 4, 25,
];
const PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60,
    52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29,
    21, 13, 5, 28, 20, 12, 4,
];
const PC2: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41, 52,
    31, 37, 47, 55, 30, 40, 51, 45, 33, 48, 44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];
const SHIFTS: [u8; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];
const SBOX: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12,
        11, 9, 5, 3, 8, 4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2, 4, 9,
        1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1,
        10, 6, 9, 11, 5, 0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, 13, 8, 10, 1, 3, 15,
        4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5,
        14, 12, 11, 15, 1, 13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, 1, 10, 13, 0, 6,
        9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2,
        12, 1, 10, 14, 9, 10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6, 10, 1,
        13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15,
        10, 3, 9, 8, 6, 4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, 11, 8, 12, 7, 1, 14,
        2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13,
        14, 0, 11, 3, 8, 9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, 4, 3, 2, 12, 9, 5,
        15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5,
        12, 2, 15, 8, 6, 1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8, 1, 4,
        10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6,
        11, 0, 14, 9, 2, 7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, 2, 1, 14, 7, 4, 10,
        8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

/// Permutes the bits of `v` (`width` bits, bit 1 = most significant) by
/// `table` (1-based positions).
fn permute(v: u64, width: u32, table: &[u8]) -> u64 {
    let mut out = 0u64;
    for &t in table {
        out = (out << 1) | ((v >> (width - t as u32)) & 1);
    }
    out
}

/// The 16 48-bit round keys of a 64-bit key.
fn des_subkeys(key: u64) -> [u64; 16] {
    let k = permute(key, 64, &PC1);
    let mut c = (k >> 28) & 0x0fff_ffff;
    let mut d = k & 0x0fff_ffff;
    let mut out = [0u64; 16];
    for (i, &s) in SHIFTS.iter().enumerate() {
        for _ in 0..s {
            c = ((c << 1) | (c >> 27)) & 0x0fff_ffff;
            d = ((d << 1) | (d >> 27)) & 0x0fff_ffff;
        }
        out[i] = permute((c << 28) | d, 56, &PC2);
    }
    out
}

/// DES encryption of `block` with the round keys, the E expansion
/// perturbed by `saltbits` (48-bit mask: a set bit `i` from the top swaps
/// expanded bits `i` and `i + 24`), `count` times.
fn des_crypt_block(block: u64, keys: &[u64; 16], saltbits: u64, count: u32) -> u64 {
    let mut b = permute(block, 64, &IP);
    for _ in 0..count {
        let mut l = (b >> 32) as u32;
        let mut r = b as u32;
        for k in keys {
            let mut e = permute(r as u64, 32, &E);
            if saltbits != 0 {
                let hi = (e >> 24) & 0xff_ffff;
                let lo = e & 0xff_ffff;
                let m = saltbits & 0xff_ffff;
                let swap = (hi ^ lo) & m;
                e ^= (swap << 24) | swap;
            }
            let x = e ^ k;
            let mut f: u32 = 0;
            for (s, sbox) in SBOX.iter().enumerate() {
                let six = ((x >> (42 - 6 * s)) & 0x3f) as usize;
                let row = ((six & 0x20) >> 4) | (six & 1);
                let col = (six >> 1) & 0xf;
                f = (f << 4) | sbox[row * 16 + col] as u32;
            }
            let f = permute(f as u64, 32, &P) as u32;
            let nl = r;
            r = l ^ f;
            l = nl;
        }
        b = ((r as u64) << 32) | l as u64;
    }
    permute(b, 64, &FP)
}

/// The salt as a mask over the 24 low expansion bits: salt bit 0 is the
/// first expansion bit of the low half.
fn salt_mask(salt: u32, bits: u32) -> u64 {
    let mut m = 0u64;
    for i in 0..bits {
        if salt & (1 << i) != 0 {
            m |= 1 << (23 - i);
        }
    }
    m
}

fn des_key_from(pw: &[u8]) -> u64 {
    let mut k = 0u64;
    for i in 0..8 {
        let c = pw.get(i).copied().unwrap_or(0);
        k = (k << 8) | ((c << 1) as u64);
    }
    k
}

fn encode_des_output(v: u64, out: &mut Vec<u8>) {
    for i in 0..10 {
        out.push(ITOA64[((v >> (58 - 6 * i)) & 0x3f) as usize]);
    }
    out.push(ITOA64[((v << 2) & 0x3f) as usize]);
}

fn des_std(pw: &[u8], setting: &[u8]) -> Option<Vec<u8>> {
    let s0 = ascii_to_bin(*setting.first()?)?;
    let s1 = ascii_to_bin(*setting.get(1)?)?;
    let salt = s0 | (s1 << 6);
    let keys = des_subkeys(des_key_from(pw));
    let r = des_crypt_block(0, &keys, salt_mask(salt, 12), 25);
    let mut out = vec![setting[0], setting[1]];
    encode_des_output(r, &mut out);
    Some(out)
}

fn des_ext(pw: &[u8], setting: &[u8]) -> Option<Vec<u8>> {
    if setting.len() < 9 {
        return None;
    }
    let mut count = 0u32;
    let mut salt = 0u32;
    for i in 0..4 {
        count |= ascii_to_bin(setting[1 + i])? << (6 * i);
        salt |= ascii_to_bin(setting[5 + i])? << (6 * i);
    }
    if count == 0 {
        return None;
    }
    let mut key = des_key_from(pw);
    let mut rest = if pw.len() > 8 { &pw[8..] } else { &[][..] };
    while !rest.is_empty() {
        let keys = des_subkeys(key);
        let enc = des_crypt_block(key, &keys, 0, 1);
        key = enc ^ des_key_from(rest);
        rest = if rest.len() > 8 { &rest[8..] } else { &[] };
    }
    let keys = des_subkeys(key);
    let r = des_crypt_block(0, &keys, salt_mask(salt, 24), count);
    let mut out = setting[..9].to_vec();
    encode_des_output(r, &mut out);
    Some(out)
}

// ---------------------------------------------------------------- MD5

fn to64(out: &mut Vec<u8>, mut v: u32, n: usize) {
    for _ in 0..n {
        out.push(ITOA64[(v & 0x3f) as usize]);
        v >>= 6;
    }
}

fn md5_crypt(pw: &[u8], setting: &[u8]) -> Vec<u8> {
    use purecrypto::hash::md5;
    let magic = b"$1$";
    let rest = &setting[3..];
    let salt_end = rest
        .iter()
        .take(8)
        .position(|&c| c == b'$')
        .unwrap_or(rest.len().min(8));
    let salt = &rest[..salt_end];
    let alt = md5(&[pw, salt, pw].concat());
    let mut ctx = Vec::new();
    ctx.extend_from_slice(pw);
    ctx.extend_from_slice(magic);
    ctx.extend_from_slice(salt);
    let mut pl = pw.len() as isize;
    while pl > 0 {
        ctx.extend_from_slice(&alt[..(pl.min(16)) as usize]);
        pl -= 16;
    }
    let mut i = pw.len();
    while i > 0 {
        if i & 1 != 0 {
            ctx.push(0);
        } else {
            ctx.push(pw[0]);
        }
        i >>= 1;
    }
    let mut fin = md5(&ctx);
    for i in 0..1000 {
        let mut c = Vec::with_capacity(64);
        if i & 1 != 0 {
            c.extend_from_slice(pw);
        } else {
            c.extend_from_slice(&fin);
        }
        if i % 3 != 0 {
            c.extend_from_slice(salt);
        }
        if i % 7 != 0 {
            c.extend_from_slice(pw);
        }
        if i & 1 != 0 {
            c.extend_from_slice(&fin);
        } else {
            c.extend_from_slice(pw);
        }
        fin = md5(&c);
    }
    let mut out = Vec::with_capacity(34);
    out.extend_from_slice(magic);
    out.extend_from_slice(salt);
    out.push(b'$');
    let f = |i: usize| fin[i] as u32;
    for (a, b, c) in [(0, 6, 12), (1, 7, 13), (2, 8, 14), (3, 9, 15), (4, 10, 5)] {
        to64(&mut out, (f(a) << 16) | (f(b) << 8) | f(c), 4);
    }
    to64(&mut out, f(11), 2);
    out
}

// ---------------------------------------------------------------- SHA-crypt

fn sha_crypt(pw: &[u8], setting: &[u8], is512: bool) -> Option<Vec<u8>> {
    let h = |d: &[u8]| -> Vec<u8> {
        if is512 {
            purecrypto::hash::sha512(d).to_vec()
        } else {
            purecrypto::hash::sha256(d).to_vec()
        }
    };
    let hlen = if is512 { 64 } else { 32 };
    let prefix: &[u8] = if is512 { b"$6$" } else { b"$5$" };
    let mut rest = &setting[3..];
    let mut rounds = 5000u64;
    let mut custom = false;
    if let Some(r) = rest.strip_prefix(b"rounds=") {
        let end = r.iter().position(|&c| c == b'$')?;
        let digits = &r[..end];
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let n: u64 = std::str::from_utf8(digits)
            .ok()?
            .parse()
            .unwrap_or(u64::MAX);
        // Observed on PHP 8.5: out-of-range rounds fail.
        if !(1000..=999_999_999).contains(&n) {
            return None;
        }
        rounds = n;
        custom = true;
        rest = &r[end + 1..];
    }
    let salt_end = rest
        .iter()
        .take(16)
        .position(|&c| c == b'$')
        .unwrap_or(rest.len().min(16));
    let salt = &rest[..salt_end];
    let b = h(&[pw, salt, pw].concat());
    let mut a = Vec::new();
    a.extend_from_slice(pw);
    a.extend_from_slice(salt);
    let mut cnt = pw.len();
    while cnt > hlen {
        a.extend_from_slice(&b);
        cnt -= hlen;
    }
    a.extend_from_slice(&b[..cnt]);
    let mut i = pw.len();
    while i > 0 {
        if i & 1 != 0 {
            a.extend_from_slice(&b);
        } else {
            a.extend_from_slice(pw);
        }
        i >>= 1;
    }
    let a = h(&a);
    let dp = h(&pw.repeat(pw.len()));
    let p: Vec<u8> = dp.iter().cycle().take(pw.len()).copied().collect();
    let ds = h(&salt.repeat(16 + a[0] as usize));
    let s: Vec<u8> = ds.iter().cycle().take(salt.len()).copied().collect();
    let mut c = a;
    for r in 0..rounds {
        let mut buf = Vec::with_capacity(hlen * 2 + p.len() * 2 + s.len());
        if r & 1 != 0 {
            buf.extend_from_slice(&p);
        } else {
            buf.extend_from_slice(&c);
        }
        if r % 3 != 0 {
            buf.extend_from_slice(&s);
        }
        if r % 7 != 0 {
            buf.extend_from_slice(&p);
        }
        if r & 1 != 0 {
            buf.extend_from_slice(&c);
        } else {
            buf.extend_from_slice(&p);
        }
        c = h(&buf);
    }
    let mut out = prefix.to_vec();
    if custom {
        out.extend_from_slice(format!("rounds={}$", rounds).as_bytes());
    }
    out.extend_from_slice(salt);
    out.push(b'$');
    let f = |i: usize| c[i] as u32;
    let b64 = |b2: u32, b1: u32, b0: u32, n: usize, out: &mut Vec<u8>| {
        to64(out, (b2 << 16) | (b1 << 8) | b0, n)
    };
    if is512 {
        let order: [(usize, usize, usize); 21] = [
            (0, 21, 42),
            (22, 43, 1),
            (44, 2, 23),
            (3, 24, 45),
            (25, 46, 4),
            (47, 5, 26),
            (6, 27, 48),
            (28, 49, 7),
            (50, 8, 29),
            (9, 30, 51),
            (31, 52, 10),
            (53, 11, 32),
            (12, 33, 54),
            (34, 55, 13),
            (56, 14, 35),
            (15, 36, 57),
            (37, 58, 16),
            (59, 17, 38),
            (18, 39, 60),
            (40, 61, 19),
            (62, 20, 41),
        ];
        for (x, y, z) in order {
            b64(f(x), f(y), f(z), 4, &mut out);
        }
        b64(0, 0, f(63), 2, &mut out);
    } else {
        let order: [(usize, usize, usize); 10] = [
            (0, 10, 20),
            (21, 1, 11),
            (12, 22, 2),
            (3, 13, 23),
            (24, 4, 14),
            (15, 25, 5),
            (6, 16, 26),
            (27, 7, 17),
            (18, 28, 8),
            (9, 19, 29),
        ];
        for (x, y, z) in order {
            b64(f(x), f(y), f(z), 4, &mut out);
        }
        b64(0, f(31), f(30), 3, &mut out);
    }
    Some(out)
}

// ---------------------------------------------------------------- Blowfish / bcrypt

/// Blowfish's initial P-array and S-boxes: the fractional hexadecimal
/// digits of pi, 1042 words, computed once with Machin's formula in
/// fixed-point arithmetic.
fn pi_words() -> &'static [u32] {
    static WORDS: OnceLock<Vec<u32>> = OnceLock::new();
    WORDS.get_or_init(|| {
        const NW: usize = 18 + 1024;
        let limbs = NW + 4; // base 2^32 fraction digits, plus guard limbs
        // Fixed point: value = int part + sum frac[i] / 2^(32(i+1)).
        // Fixed point: v[0] integer part, v[1..] base-2^32 fraction.
        fn div_small(v: &mut [u32], d: u64) -> bool {
            let mut r: u64 = 0;
            let mut nonzero = false;
            for limb in v.iter_mut() {
                let cur = (r << 32) | *limb as u64;
                *limb = (cur / d) as u32;
                r = cur % d;
                nonzero |= *limb != 0;
            }
            nonzero
        }
        fn add(a: &mut [u32], b: &[u32]) {
            let mut carry = 0u64;
            for i in (0..a.len()).rev() {
                let v = a[i] as u64 + b[i] as u64 + carry;
                a[i] = v as u32;
                carry = v >> 32;
            }
        }
        fn sub(a: &mut [u32], b: &[u32]) {
            let mut borrow = 0i64;
            for i in (0..a.len()).rev() {
                let v = a[i] as i64 - b[i] as i64 - borrow;
                borrow = (v < 0) as i64;
                a[i] = (v + (borrow << 32)) as u32;
            }
        }
        // arctan(1/x) = sum (-1)^k / ((2k+1) x^(2k+1))
        fn arctan_inv(x: u32, limbs: usize) -> (u64, Vec<u32>) {
            let mut t = vec![0u32; limbs + 1];
            t[0] = 1;
            div_small(&mut t, x as u64);
            let mut sum = t.clone();
            let x2 = x as u64 * x as u64;
            let mut k: u64 = 1;
            loop {
                if !div_small(&mut t, x2) {
                    break;
                }
                let mut q = t.clone();
                div_small(&mut q, 2 * k + 1);
                if k % 2 == 1 {
                    sub(&mut sum, &q);
                } else {
                    add(&mut sum, &q);
                }
                k += 1;
            }
            (sum[0] as u64, sum[1..].to_vec())
        }
        let (_, a) = arctan_inv(5, limbs);
        let (_, b) = arctan_inv(239, limbs);
        // pi = 16 a - 4 b, computed on [int, frac...] vectors.
        let mul = |v: &[u32], m: u64| -> Vec<u64> {
            let mut out = vec![0u64; v.len() + 1];
            let mut carry = 0u64;
            for i in (0..v.len()).rev() {
                let x = v[i] as u64 * m + carry;
                out[i + 1] = x & 0xffff_ffff;
                carry = x >> 32;
            }
            out[0] = carry;
            out
        };
        let a16 = mul(&a, 16);
        let b4 = mul(&b, 4);
        let mut pi = vec![0u32; a16.len()];
        let mut borrow = 0i64;
        for i in (0..a16.len()).rev() {
            let v = a16[i] as i64 - b4[i] as i64 - borrow;
            if v < 0 {
                pi[i] = (v + (1i64 << 32)) as u32;
                borrow = 1;
            } else {
                pi[i] = v as u32;
                borrow = 0;
            }
        }
        // pi[0] is the integer part (3); the words are the fraction.
        pi[1..=NW].to_vec()
    })
}

struct Blowfish {
    p: [u32; 18],
    s: [[u32; 256]; 4],
}

impl Blowfish {
    fn initial() -> Blowfish {
        let w = pi_words();
        let mut p = [0u32; 18];
        p.copy_from_slice(&w[..18]);
        let mut s = [[0u32; 256]; 4];
        for (i, sb) in s.iter_mut().enumerate() {
            sb.copy_from_slice(&w[18 + 256 * i..18 + 256 * (i + 1)]);
        }
        Blowfish { p, s }
    }

    #[inline]
    fn f(&self, x: u32) -> u32 {
        let a = self.s[0][(x >> 24) as usize];
        let b = self.s[1][((x >> 16) & 0xff) as usize];
        let c = self.s[2][((x >> 8) & 0xff) as usize];
        let d = self.s[3][(x & 0xff) as usize];
        (a.wrapping_add(b) ^ c).wrapping_add(d)
    }

    fn encrypt(&self, mut l: u32, mut r: u32) -> (u32, u32) {
        l ^= self.p[0];
        for i in (1..16).step_by(2) {
            r ^= self.f(l) ^ self.p[i];
            l ^= self.f(r) ^ self.p[i + 1];
        }
        r ^= self.p[17];
        (r, l)
    }

    /// ExpandKey(state, salt, key) of the Eksblowfish setup; `salt` None
    /// is the all-zero salt.
    fn expand(&mut self, key: &[u32], salt: Option<&[u32; 4]>) {
        for (i, pi) in self.p.iter_mut().enumerate() {
            *pi ^= key[i % key.len()];
        }
        let (mut l, mut r) = (0u32, 0u32);
        let mut si = 0usize;
        let mut next = |l: &mut u32, r: &mut u32| {
            if let Some(s) = salt {
                *l ^= s[si % 4];
                *r ^= s[(si + 1) % 4];
                si += 2;
            }
        };
        for i in (0..18).step_by(2) {
            next(&mut l, &mut r);
            (l, r) = self.encrypt(l, r);
            self.p[i] = l;
            self.p[i + 1] = r;
        }
        for b in 0..4 {
            for i in (0..256).step_by(2) {
                next(&mut l, &mut r);
                (l, r) = self.encrypt(l, r);
                self.s[b][i] = l;
                self.s[b][i + 1] = r;
            }
        }
    }
}

fn bcrypt_b64_decode(s: &[u8], nbytes: usize) -> Option<Vec<u8>> {
    let val = |c: u8| BCRYPT64.iter().position(|&x| x == c).map(|p| p as u32);
    let mut out = Vec::with_capacity(nbytes);
    let mut i = 0;
    while out.len() < nbytes {
        let c1 = val(*s.get(i)?)?;
        let c2 = val(*s.get(i + 1)?)?;
        out.push(((c1 << 2) | ((c2 & 0x30) >> 4)) as u8);
        if out.len() >= nbytes {
            break;
        }
        let c3 = val(*s.get(i + 2)?)?;
        out.push((((c2 & 0x0f) << 4) | ((c3 & 0x3c) >> 2)) as u8);
        if out.len() >= nbytes {
            break;
        }
        let c4 = val(*s.get(i + 3)?)?;
        out.push((((c3 & 0x03) << 6) | c4) as u8);
        i += 4;
    }
    Some(out)
}

fn bcrypt_b64_encode(d: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < d.len() {
        let c1 = d[i] as u32;
        out.push(BCRYPT64[(c1 >> 2) as usize]);
        let mut c = (c1 & 0x03) << 4;
        if i + 1 >= d.len() {
            out.push(BCRYPT64[c as usize]);
            break;
        }
        let c2 = d[i + 1] as u32;
        c |= c2 >> 4;
        out.push(BCRYPT64[c as usize]);
        c = (c2 & 0x0f) << 2;
        if i + 2 >= d.len() {
            out.push(BCRYPT64[c as usize]);
            break;
        }
        let c3 = d[i + 2] as u32;
        c |= c3 >> 6;
        out.push(BCRYPT64[c as usize]);
        out.push(BCRYPT64[(c3 & 0x3f) as usize]);
        i += 3;
    }
}

/// bcrypt (`$2a$`, `$2b$`, `$2x$`, `$2y$`): None for a malformed setting.
pub fn bcrypt(pw: &[u8], setting: &[u8]) -> Option<Vec<u8>> {
    if setting.len() < 29 || setting[0] != b'$' || setting[1] != b'2' || setting[3] != b'$' {
        return None;
    }
    let variant = setting[2];
    if !matches!(variant, b'a' | b'b' | b'x' | b'y') {
        return None;
    }
    if !setting[4].is_ascii_digit() || !setting[5].is_ascii_digit() || setting[6] != b'$' {
        return None;
    }
    let cost = ((setting[4] - b'0') * 10 + (setting[5] - b'0')) as u32;
    if !(4..=31).contains(&cost) {
        return None;
    }
    let salt = bcrypt_b64_decode(&setting[7..29], 16)?;
    // The key: the password with its NUL, at most 72 bytes, as big-endian
    // words; $2x$ sign-extends bytes (the historical bug it names).
    let mut kb: Vec<u8> = pw.iter().copied().take(72).collect();
    if kb.len() < 72 {
        kb.push(0);
    }
    let mut key = [0u32; 18];
    let mut j = 0usize;
    for w in key.iter_mut() {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = kb[j % kb.len()];
            if variant == b'x' {
                v = (v << 8) | (b as i8 as i32 as u32);
            } else {
                v = (v << 8) | b as u32;
            }
            j += 1;
        }
        *w = v;
    }
    let mut sw = [0u32; 4];
    for (i, w) in sw.iter_mut().enumerate() {
        *w = u32::from_be_bytes([
            salt[4 * i],
            salt[4 * i + 1],
            salt[4 * i + 2],
            salt[4 * i + 3],
        ]);
    }
    let mut bf = Blowfish::initial();
    bf.expand(&key, Some(&sw));
    let salt_key: Vec<u32> = sw.to_vec();
    for _ in 0..(1u64 << cost) {
        bf.expand(&key, None);
        bf.expand(&salt_key, None);
    }
    let mut ctext: [u32; 6] = [
        0x4f72_7068,
        0x6561_6e42,
        0x6568_6f6c,
        0x6465_7253,
        0x6372_7944,
        0x6f75_6274,
    ];
    for _ in 0..64 {
        for i in (0..6).step_by(2) {
            let (l, r) = bf.encrypt(ctext[i], ctext[i + 1]);
            ctext[i] = l;
            ctext[i + 1] = r;
        }
    }
    let mut bytes = Vec::with_capacity(24);
    for w in ctext {
        bytes.extend_from_slice(&w.to_be_bytes());
    }
    let mut out = setting[..7].to_vec();
    bcrypt_b64_encode(&salt, &mut out);
    bcrypt_b64_encode(&bytes[..23], &mut out);
    Some(out)
}

/// The 22-character bcrypt salt for 16 random bytes.
pub fn bcrypt_salt(bytes: &[u8; 16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(22);
    bcrypt_b64_encode(bytes, &mut out);
    out
}

// ---------------------------------------------------------------- dispatch

/// `crypt($string, $salt)`. A failure returns `*0`, or `*1` when the salt
/// itself starts with `*0` (PHP manual: crypt, "the return value is
/// guaranteed to differ from the salt on failure").
pub fn crypt(pw: &[u8], salt: &[u8]) -> Vec<u8> {
    let fail = || {
        if salt.starts_with(b"*0") {
            b"*1".to_vec()
        } else {
            b"*0".to_vec()
        }
    };
    let r = if salt.starts_with(b"$1$") {
        Some(md5_crypt(pw, salt))
    } else if salt.starts_with(b"$2") {
        bcrypt(pw, salt)
    } else if salt.starts_with(b"$5$") {
        sha_crypt(pw, salt, false)
    } else if salt.starts_with(b"$6$") {
        sha_crypt(pw, salt, true)
    } else if salt.first() == Some(&b'_') {
        des_ext(pw, salt)
    } else if salt.first() == Some(&b'$') {
        None
    } else {
        des_std(pw, salt)
    };
    r.unwrap_or_else(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn des_vector() {
        // The classic FIPS 46 worked example.
        let keys = des_subkeys(0x1334_5779_9BBC_DFF1);
        assert_eq!(
            des_crypt_block(0x0123_4567_89AB_CDEF, &keys, 0, 1),
            0x85E8_1354_0F0A_B405
        );
    }

    #[test]
    fn pi_digits() {
        let w = pi_words();
        assert_eq!(w[0], 0x243F_6A88);
        assert_eq!(w[17], 0x8979_FB1B);
        assert_eq!(w[18], 0xD131_0BA6);
        assert_eq!(w[1041], 0x3AC3_72E6);
    }

    #[test]
    fn observed() {
        // Observed on PHP 8.5.
        assert_eq!(crypt(b"rasmuslerdorf", b"rl"), b"rl.3StKT.4T8M");
        assert_eq!(
            crypt(b"rasmuslerdorf", b"_J9..rasm"),
            b"_J9..rasmBYk8r9AiWNc"
        );
        assert_eq!(
            crypt(b"rasmuslerdorf", b"$1$$"),
            b"$1$$9FgrOJK8xP1TnARfMcxvR."
        );
    }
}
