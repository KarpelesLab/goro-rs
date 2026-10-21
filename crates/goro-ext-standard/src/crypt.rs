//! crypt() and password_*() — ports of php-src ext/standard/crypt.c,
//! crypt_freesec.c (DES / extended BSDI DES), php_crypt_r.c (MD5),
//! crypt_sha256.c, crypt_sha512.c, crypt_blowfish.c and password.c.
//!
//! The C implementations work on NUL-terminated strings and sometimes read
//! past the logical end of the salt (into the '$'-padded buffer built by
//! PHP_FUNCTION(crypt)), so the algorithms here take byte buffers and read
//! them through `at()`, which yields NUL past the end.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::opcode::ParamType;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::crypt_blowfish_tables::{BF_INIT_P, BF_INIT_S};

const PHP_MAX_SALT_LEN: usize = 123;
const PHP_PASSWORD_BCRYPT_COST: i64 = 12;

#[inline]
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// The C string starting at `s` (up to the first NUL).
#[inline]
fn cstr(s: &[u8]) -> &[u8] {
    match s.iter().position(|&b| b == 0) {
        Some(p) => &s[..p],
        None => s,
    }
}

// ===================================================================
// php_crypt (crypt.c)
// ===================================================================

#[inline]
fn is_valid_salt_character(c: u8) -> bool {
    (b'.'..=b'9').contains(&c) || c.is_ascii_uppercase() || c.is_ascii_lowercase()
}

/// php_crypt(): returns None on failure (the caller maps that to "*0"/"*1").
/// `password` is treated as a C string; `salt` is a NUL-terminated buffer
/// (bytes after the terminator may be read, exactly like the C code does).
pub fn php_crypt(password: &[u8], salt: &[u8]) -> Option<Vec<u8>> {
    let pw = cstr(password);
    let s0 = at(salt, 0);
    let s1 = at(salt, 1);
    if s0 == b'*' && (s1 == b'0' || s1 == b'1') {
        return None;
    }
    let s2 = at(salt, 2);
    if s0 == b'$' && s1 == b'1' && s2 == b'$' {
        md5_crypt(pw, salt)
    } else if s0 == b'$' && s1 == b'6' && s2 == b'$' {
        sha_crypt(pw, salt, ShaKind::Sha512, PHP_MAX_SALT_LEN as i32)
    } else if s0 == b'$' && s1 == b'5' && s2 == b'$' {
        sha_crypt(pw, salt, ShaKind::Sha256, PHP_MAX_SALT_LEN as i32)
    } else if s0 == b'$' && s1 == b'2' && s2 != 0 && at(salt, 3) == b'$' {
        bf_crypt(pw, salt, 16)
    } else if s0 == b'_' || (is_valid_salt_character(s0) && is_valid_salt_character(s1)) {
        // DES fallback
        let res = crypt_extended(pw, salt)?;
        if s0 == b'*' && s1 == b'0' {
            return None;
        }
        Some(res)
    } else {
        None
    }
}

// ===================================================================
// MD5 crypt (php_crypt_r.c)
// ===================================================================

const ITOA64: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn to64(out: &mut Vec<u8>, mut v: u32, n: usize) {
    for _ in 0..n {
        out.push(ITOA64[(v & 0x3f) as usize]);
        v >>= 6;
    }
}

fn md5_crypt(pw: &[u8], salt: &[u8]) -> Option<Vec<u8>> {
    use md5::{Digest, Md5};
    const MAGIC: &[u8] = b"$1$";
    let pwl = pw.len();

    let mut sp = 0usize;
    if cstr(salt).starts_with(MAGIC) {
        sp = MAGIC.len();
    }
    let mut ep = sp;
    while at(salt, ep) != 0 && at(salt, ep) != b'$' && ep < sp + 8 {
        ep += 1;
    }
    let sl = ep - sp;
    let salt_bytes: Vec<u8> = (sp..ep).map(|i| at(salt, i)).collect();

    let mut ctx = Md5::new();
    ctx.update(pw);
    ctx.update(MAGIC);
    ctx.update(&salt_bytes);

    let mut ctx1 = Md5::new();
    ctx1.update(pw);
    ctx1.update(&salt_bytes);
    ctx1.update(pw);
    let mut fin: [u8; 16] = ctx1.finalize().into();

    let mut pl = pwl as isize;
    while pl > 0 {
        let n = if pl > 16 { 16 } else { pl as usize };
        ctx.update(&fin[..n]);
        pl -= 16;
    }

    // The C code zeroes `final` before this loop.
    fin = [0u8; 16];

    let mut i = pwl;
    while i != 0 {
        if (i & 1) != 0 {
            ctx.update(&fin[..1]);
        } else {
            // i != 0 implies pwl > 0
            ctx.update(&pw[..1]);
        }
        i >>= 1;
    }

    let mut out = Vec::with_capacity(3 + sl + 1 + 22);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&salt_bytes);
    out.push(b'$');

    fin = ctx.finalize().into();

    for i in 0..1000u32 {
        let mut c = Md5::new();
        if (i & 1) != 0 {
            c.update(pw);
        } else {
            c.update(fin);
        }
        if i % 3 != 0 {
            c.update(&salt_bytes);
        }
        if i % 7 != 0 {
            c.update(pw);
        }
        if (i & 1) != 0 {
            c.update(fin);
        } else {
            c.update(pw);
        }
        fin = c.finalize().into();
    }

    let f = |i: usize| fin[i] as u32;
    to64(&mut out, (f(0) << 16) | (f(6) << 8) | f(12), 4);
    to64(&mut out, (f(1) << 16) | (f(7) << 8) | f(13), 4);
    to64(&mut out, (f(2) << 16) | (f(8) << 8) | f(14), 4);
    to64(&mut out, (f(3) << 16) | (f(9) << 8) | f(15), 4);
    to64(&mut out, (f(4) << 16) | (f(10) << 8) | f(5), 4);
    to64(&mut out, f(11), 2);
    Some(out)
}

// ===================================================================
// SHA-256 / SHA-512 crypt (crypt_sha256.c, crypt_sha512.c)
// ===================================================================

#[derive(Clone, Copy, PartialEq)]
enum ShaKind {
    Sha256,
    Sha512,
}

const SHA_ROUNDS_DEFAULT: u64 = 5000;
const SHA_ROUNDS_MIN: u64 = 1000;
const SHA_ROUNDS_MAX: u64 = 999_999_999;
const SHA_SALT_LEN_MAX: usize = 16;

/// C strtoul(s, &endp, 10) on a NUL-terminated buffer starting at `start`.
/// Returns (value, index of endp).
fn c_strtoul(s: &[u8], start: usize) -> (u64, usize) {
    let mut i = start;
    while matches!(at(s, i), b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if at(s, i) == b'+' || at(s, i) == b'-' {
        neg = at(s, i) == b'-';
        i += 1;
    }
    let digits_start = i;
    let mut val: u64 = 0;
    let mut overflow = false;
    while at(s, i).is_ascii_digit() {
        let d = (at(s, i) - b'0') as u64;
        match val.checked_mul(10).and_then(|v| v.checked_add(d)) {
            Some(v) => val = v,
            None => overflow = true,
        }
        i += 1;
    }
    if i == digits_start {
        return (0, start);
    }
    if overflow {
        return (u64::MAX, i);
    }
    (if neg { val.wrapping_neg() } else { val }, i)
}

enum ShaCtx {
    S256(sha2::Sha256),
    S512(sha2::Sha512),
}

impl ShaCtx {
    fn new(kind: ShaKind) -> Self {
        use sha2::Digest;
        match kind {
            ShaKind::Sha256 => ShaCtx::S256(sha2::Sha256::new()),
            ShaKind::Sha512 => ShaCtx::S512(sha2::Sha512::new()),
        }
    }
    fn update(&mut self, data: &[u8]) {
        use sha2::Digest;
        match self {
            ShaCtx::S256(c) => c.update(data),
            ShaCtx::S512(c) => c.update(data),
        }
    }
    fn finish(self) -> Vec<u8> {
        use sha2::Digest;
        match self {
            ShaCtx::S256(c) => c.finalize().to_vec(),
            ShaCtx::S512(c) => c.finalize().to_vec(),
        }
    }
}

fn sha_crypt(key: &[u8], salt_buf: &[u8], kind: ShaKind, mut buflen: i32) -> Option<Vec<u8>> {
    let (prefix, hlen): (&[u8], usize) = match kind {
        ShaKind::Sha256 => (b"$5$", 32),
        ShaKind::Sha512 => (b"$6$", 64),
    };
    const ROUNDS_PREFIX: &[u8] = b"rounds=";

    let mut pos = 0usize;
    if (0..prefix.len()).all(|i| at(salt_buf, i) == prefix[i]) {
        pos += prefix.len();
    }
    let mut rounds = SHA_ROUNDS_DEFAULT;
    let mut rounds_custom = false;
    if (0..ROUNDS_PREFIX.len()).all(|i| at(salt_buf, pos + i) == ROUNDS_PREFIX[i]) {
        let num = pos + ROUNDS_PREFIX.len();
        let (srounds, endp) = c_strtoul(salt_buf, num);
        if at(salt_buf, endp) == b'$' {
            pos = endp + 1;
            if !(SHA_ROUNDS_MIN..=SHA_ROUNDS_MAX).contains(&srounds) {
                return None;
            }
            rounds = srounds;
            rounds_custom = true;
        }
    }

    let mut span = 0usize;
    while at(salt_buf, pos + span) != 0 && at(salt_buf, pos + span) != b'$' {
        span += 1;
    }
    let salt_len = span.min(SHA_SALT_LEN_MAX);
    let salt: Vec<u8> = (pos..pos + salt_len).map(|i| at(salt_buf, i)).collect();
    let key_len = key.len();

    let mut ctx = ShaCtx::new(kind);
    ctx.update(key);
    ctx.update(&salt);

    let mut alt = ShaCtx::new(kind);
    alt.update(key);
    alt.update(&salt);
    alt.update(key);
    let mut alt_result = alt.finish();

    let mut cnt = key_len;
    while cnt > hlen {
        ctx.update(&alt_result);
        cnt -= hlen;
    }
    ctx.update(&alt_result[..cnt]);

    let mut cnt = key_len;
    while cnt > 0 {
        if (cnt & 1) != 0 {
            ctx.update(&alt_result);
        } else {
            ctx.update(key);
        }
        cnt >>= 1;
    }
    alt_result = ctx.finish();

    // P byte sequence
    let mut alt = ShaCtx::new(kind);
    for _ in 0..key_len {
        alt.update(key);
    }
    let temp_result = alt.finish();
    let mut p_bytes = Vec::with_capacity(key_len);
    let mut cnt = key_len;
    while cnt >= hlen {
        p_bytes.extend_from_slice(&temp_result);
        cnt -= hlen;
    }
    p_bytes.extend_from_slice(&temp_result[..cnt]);

    // S byte sequence
    let mut alt = ShaCtx::new(kind);
    for _ in 0..(16 + alt_result[0] as usize) {
        alt.update(&salt);
    }
    let temp_result = alt.finish();
    let mut s_bytes = Vec::with_capacity(salt_len);
    let mut cnt = salt_len;
    while cnt >= hlen {
        s_bytes.extend_from_slice(&temp_result);
        cnt -= hlen;
    }
    s_bytes.extend_from_slice(&temp_result[..cnt]);

    for cnt in 0..rounds {
        let mut c = ShaCtx::new(kind);
        if (cnt & 1) != 0 {
            c.update(&p_bytes);
        } else {
            c.update(&alt_result);
        }
        if cnt % 3 != 0 {
            c.update(&s_bytes);
        }
        if cnt % 7 != 0 {
            c.update(&p_bytes);
        }
        if (cnt & 1) != 0 {
            c.update(&alt_result);
        } else {
            c.update(&p_bytes);
        }
        alt_result = c.finish();
    }

    // Construct the result string, honouring the C buffer accounting.
    let mut out: Vec<u8> = Vec::with_capacity(buflen.max(0) as usize);
    out.extend_from_slice(&prefix[..prefix.len().min(buflen.max(0) as usize)]);
    buflen -= prefix.len() as i32;
    if rounds_custom {
        let s = format!("rounds={}$", rounds);
        let n = s.len() as i32;
        let avail = buflen.max(0) as usize;
        if avail > 0 {
            let w = s.len().min(avail - 1);
            out.extend_from_slice(&s.as_bytes()[..w]);
        }
        buflen -= n;
    }
    let ncopy = (buflen.max(0) as usize).min(salt_len);
    out.extend_from_slice(&salt[..ncopy]);
    buflen -= ncopy as i32;
    if buflen > 0 {
        out.push(b'$');
        buflen -= 1;
    }

    let mut b64 = |b2: u8, b1: u8, b0: u8, n: i32, out: &mut Vec<u8>| {
        let mut w: u32 = ((b2 as u32) << 16) | ((b1 as u32) << 8) | (b0 as u32);
        let mut n = n;
        while n > 0 && buflen > 0 {
            n -= 1;
            out.push(ITOA64[(w & 0x3f) as usize]);
            buflen -= 1;
            w >>= 6;
        }
    };
    let a = &alt_result;
    match kind {
        ShaKind::Sha256 => {
            b64(a[0], a[10], a[20], 4, &mut out);
            b64(a[21], a[1], a[11], 4, &mut out);
            b64(a[12], a[22], a[2], 4, &mut out);
            b64(a[3], a[13], a[23], 4, &mut out);
            b64(a[24], a[4], a[14], 4, &mut out);
            b64(a[15], a[25], a[5], 4, &mut out);
            b64(a[6], a[16], a[26], 4, &mut out);
            b64(a[27], a[7], a[17], 4, &mut out);
            b64(a[18], a[28], a[8], 4, &mut out);
            b64(a[9], a[19], a[29], 4, &mut out);
            b64(0, a[31], a[30], 3, &mut out);
        }
        ShaKind::Sha512 => {
            b64(a[0], a[21], a[42], 4, &mut out);
            b64(a[22], a[43], a[1], 4, &mut out);
            b64(a[44], a[2], a[23], 4, &mut out);
            b64(a[3], a[24], a[45], 4, &mut out);
            b64(a[25], a[46], a[4], 4, &mut out);
            b64(a[47], a[5], a[26], 4, &mut out);
            b64(a[6], a[27], a[48], 4, &mut out);
            b64(a[28], a[49], a[7], 4, &mut out);
            b64(a[50], a[8], a[29], 4, &mut out);
            b64(a[9], a[30], a[51], 4, &mut out);
            b64(a[31], a[52], a[10], 4, &mut out);
            b64(a[53], a[11], a[32], 4, &mut out);
            b64(a[12], a[33], a[54], 4, &mut out);
            b64(a[34], a[55], a[13], 4, &mut out);
            b64(a[56], a[14], a[35], 4, &mut out);
            b64(a[15], a[36], a[57], 4, &mut out);
            b64(a[37], a[58], a[16], 4, &mut out);
            b64(a[59], a[17], a[38], 4, &mut out);
            b64(a[18], a[39], a[60], 4, &mut out);
            b64(a[40], a[61], a[19], 4, &mut out);
            b64(a[62], a[20], a[41], 4, &mut out);
            b64(0, 0, a[63], 2, &mut out);
        }
    }
    if buflen <= 0 {
        return None;
    }
    Some(out)
}

// ===================================================================
// Blowfish crypt (crypt_blowfish.c)
// ===================================================================

const BF_N: usize = 16;

const BF_ITOA64: &[u8; 64] = b"./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

const BF_ATOI64: [u8; 0x60] = [
    64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 0, 1,
    54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 64, 64, 64, 64, 64,
    64, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
    17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 64, 64, 64, 64, 64,
    64, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42,
    43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 64, 64, 64, 64, 64,
];

const BF_MAGIC_W: [u32; 6] = [0x4F727068, 0x65616E42, 0x65686F6C, 0x64657253, 0x63727944, 0x6F756274];

const BF_FLAGS_BY_SUBTYPE: [u8; 26] = [2, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 4, 0];

fn bf_safe_atoi64(c: u8) -> Option<u32> {
    let t = (c as u32).wrapping_sub(0x20);
    if t >= 0x60 {
        return None;
    }
    let v = BF_ATOI64[t as usize] as u32;
    if v > 63 {
        return None;
    }
    Some(v)
}

/// BF_decode: decode `size` bytes from the setting starting at `start`.
fn bf_decode(src: &[u8], start: usize, size: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(size);
    let mut sp = start;
    loop {
        let c1 = bf_safe_atoi64(at(src, sp))?;
        let c2 = bf_safe_atoi64(at(src, sp + 1))?;
        sp += 2;
        out.push(((c1 << 2) | ((c2 & 0x30) >> 4)) as u8);
        if out.len() >= size {
            break;
        }
        let c3 = bf_safe_atoi64(at(src, sp))?;
        sp += 1;
        out.push((((c2 & 0x0F) << 4) | ((c3 & 0x3C) >> 2)) as u8);
        if out.len() >= size {
            break;
        }
        let c4 = bf_safe_atoi64(at(src, sp))?;
        sp += 1;
        out.push((((c3 & 0x03) << 6) | c4) as u8);
        if out.len() >= size {
            break;
        }
    }
    Some(out)
}

fn bf_encode(out: &mut Vec<u8>, src: &[u8]) {
    let mut i = 0usize;
    let end = src.len();
    loop {
        let mut c1 = src[i] as u32;
        i += 1;
        out.push(BF_ITOA64[(c1 >> 2) as usize]);
        c1 = (c1 & 0x03) << 4;
        if i >= end {
            out.push(BF_ITOA64[c1 as usize]);
            break;
        }
        let c2 = src[i] as u32;
        i += 1;
        c1 |= c2 >> 4;
        out.push(BF_ITOA64[c1 as usize]);
        c1 = (c2 & 0x0f) << 2;
        if i >= end {
            out.push(BF_ITOA64[c1 as usize]);
            break;
        }
        let c2 = src[i] as u32;
        i += 1;
        c1 |= c2 >> 6;
        out.push(BF_ITOA64[c1 as usize]);
        out.push(BF_ITOA64[(c2 & 0x3f) as usize]);
        if i >= end {
            break;
        }
    }
}

struct BfCtx {
    s: [u32; 1024],
    p: [u32; BF_N + 2],
}

impl BfCtx {
    #[inline(always)]
    fn f(&self, x: u32) -> u32 {
        let t1 = self.s[768 + (x & 0xff) as usize];
        let t2 = self.s[512 + ((x >> 8) & 0xff) as usize];
        let mut t3 = self.s[256 + ((x >> 16) & 0xff) as usize];
        t3 = t3.wrapping_add(self.s[(x >> 24) as usize]);
        t3 ^= t2;
        t3.wrapping_add(t1)
    }

    #[inline(always)]
    fn encrypt(&self, l: &mut u32, r: &mut u32) {
        let mut ll = *l ^ self.p[0];
        let mut rr = *r;
        let mut n = 0;
        while n < 16 {
            rr ^= self.p[n + 1] ^ self.f(ll);
            ll ^= self.p[n + 2] ^ self.f(rr);
            n += 2;
        }
        let t = rr;
        rr = ll;
        ll = t ^ self.p[BF_N + 1];
        *l = ll;
        *r = rr;
    }

    fn body(&mut self) {
        let (mut l, mut r) = (0u32, 0u32);
        let mut i = 0;
        while i < BF_N + 2 {
            self.encrypt(&mut l, &mut r);
            self.p[i] = l;
            self.p[i + 1] = r;
            i += 2;
        }
        let mut i = 0;
        while i < 1024 {
            self.encrypt(&mut l, &mut r);
            self.s[i] = l;
            self.s[i + 1] = r;
            i += 2;
        }
    }
}

/// BF_set_key: returns (expanded, initial).
fn bf_set_key(key: &[u8], flags: u8) -> ([u32; BF_N + 2], [u32; BF_N + 2]) {
    // key is a C string; the terminating NUL takes part in the cycle.
    let kat = |i: usize| -> u8 { if i < key.len() { key[i] } else { 0 } };
    let mut expanded = [0u32; BF_N + 2];
    let mut initial = [0u32; BF_N + 2];
    let bug = (flags & 1) as usize;
    let safety: u32 = ((flags as u32) & 2) << 15;
    let mut sign: u32 = 0;
    let mut diff: u32 = 0;
    let mut ptr = 0usize;
    for i in 0..BF_N + 2 {
        let mut tmp = [0u32; 2];
        for j in 0..4 {
            let c = kat(ptr);
            tmp[0] <<= 8;
            tmp[0] |= c as u32;
            tmp[1] <<= 8;
            tmp[1] |= (c as i8 as i32) as u32;
            if j != 0 {
                sign |= tmp[1] & 0x80;
            }
            if c == 0 {
                ptr = 0;
            } else {
                ptr += 1;
            }
        }
        diff |= tmp[0] ^ tmp[1];
        expanded[i] = tmp[bug];
        initial[i] = BF_INIT_P[i] ^ tmp[bug];
    }
    diff |= diff >> 16;
    diff &= 0xffff;
    diff = diff.wrapping_add(0xffff);
    sign <<= 9;
    sign &= !diff & safety;
    initial[0] ^= sign;
    (expanded, initial)
}

fn bf_crypt(key: &[u8], setting: &[u8], min: u32) -> Option<Vec<u8>> {
    let s = |i: usize| at(setting, i);
    if s(0) != b'$'
        || s(1) != b'2'
        || !(b'a'..=b'z').contains(&s(2))
        || BF_FLAGS_BY_SUBTYPE[(s(2) - b'a') as usize] == 0
        || s(3) != b'$'
        || !(b'0'..=b'3').contains(&s(4))
        || !s(5).is_ascii_digit()
        || (s(4) == b'3' && s(5) > b'1')
        || s(6) != b'$'
    {
        return None;
    }
    let flags = BF_FLAGS_BY_SUBTYPE[(s(2) - b'a') as usize];
    let cost = ((s(4) - b'0') as u32) * 10 + (s(5) - b'0') as u32;
    let mut count: u32 = 1u32 << cost;
    if count < min {
        return None;
    }
    let salt_bytes = bf_decode(setting, 7, 16)?;
    let mut salt = [0u32; 4];
    for (k, w) in salt.iter_mut().enumerate() {
        *w = u32::from_be_bytes([salt_bytes[4 * k], salt_bytes[4 * k + 1], salt_bytes[4 * k + 2], salt_bytes[4 * k + 3]]);
    }

    let (expanded, initial) = bf_set_key(key, flags);
    let mut ctx = BfCtx { s: [0u32; 1024], p: initial };
    for b in 0..4 {
        ctx.s[b * 256..(b + 1) * 256].copy_from_slice(&BF_INIT_S[b]);
    }

    let (mut l, mut r) = (0u32, 0u32);
    let mut i = 0;
    while i < BF_N + 2 {
        l ^= salt[i & 2];
        r ^= salt[(i & 2) + 1];
        ctx.encrypt(&mut l, &mut r);
        ctx.p[i] = l;
        ctx.p[i + 1] = r;
        i += 2;
    }
    let mut i = 0;
    while i < 1024 {
        l ^= salt[(BF_N + 2) & 3];
        r ^= salt[(BF_N + 3) & 3];
        ctx.encrypt(&mut l, &mut r);
        ctx.s[i] = l;
        ctx.s[i + 1] = r;
        l ^= salt[(BF_N + 4) & 3];
        r ^= salt[(BF_N + 5) & 3];
        ctx.encrypt(&mut l, &mut r);
        ctx.s[i + 2] = l;
        ctx.s[i + 3] = r;
        i += 4;
    }

    loop {
        for i in 0..BF_N + 2 {
            ctx.p[i] ^= expanded[i];
        }
        ctx.body();
        for i in 0..BF_N + 2 {
            ctx.p[i] ^= salt[i & 3];
        }
        ctx.body();
        count -= 1;
        if count == 0 {
            break;
        }
    }

    let mut output_words = [0u32; 6];
    let mut i = 0;
    while i < 6 {
        let mut l = BF_MAGIC_W[i];
        let mut r = BF_MAGIC_W[i + 1];
        for _ in 0..64 {
            ctx.encrypt(&mut l, &mut r);
        }
        output_words[i] = l;
        output_words[i + 1] = r;
        i += 2;
    }

    let mut out = Vec::with_capacity(60);
    for i in 0..(7 + 22 - 1) {
        out.push(s(i));
    }
    let last = s(7 + 22 - 1);
    out.push(BF_ITOA64[(BF_ATOI64[(last as usize).wrapping_sub(0x20)] & 0x30) as usize]);
    let mut bytes = Vec::with_capacity(24);
    for w in output_words.iter() {
        bytes.extend_from_slice(&w.to_be_bytes());
    }
    bf_encode(&mut out, &bytes[..23]);
    Some(out)
}

// ===================================================================
// DES / extended DES (crypt_freesec.c)
// ===================================================================

const DES_IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4,
    62, 54, 46, 38, 30, 22, 14, 6, 64, 56, 48, 40, 32, 24, 16, 8,
    57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3,
    61, 53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];

const DES_KEY_PERM: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18,
    10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60, 52, 44, 36,
    63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22,
    14, 6, 61, 53, 45, 37, 29, 21, 13, 5, 28, 20, 12, 4,
];

const DES_KEY_SHIFTS: [u8; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

const DES_COMP_PERM: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10,
    23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2,
    41, 52, 31, 37, 47, 55, 30, 40, 51, 45, 33, 48,
    44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];

const DES_SBOX: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7,
        0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8,
        4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0,
        15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10,
        3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1, 10, 6, 9, 11, 5,
        0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15,
        13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8,
        13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1,
        13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7,
        1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15,
        13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9,
        10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4,
        3, 15, 0, 6, 10, 1, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9,
        14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6,
        4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14,
        11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11,
        10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8,
        9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6,
        4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1,
        13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6,
        1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2,
        6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7,
        1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2,
        7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8,
        2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

const DES_PBOX: [u8; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10,
    2, 8, 24, 14, 32, 27, 3, 9, 19, 13, 30, 6, 22, 11, 4, 25,
];

const BITS8: [u8; 8] = [0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01];

#[inline]
fn bits32(i: usize) -> u32 {
    0x8000_0000u32 >> i
}

struct DesTables {
    m_sbox: Vec<[u8; 4096]>,
    psbox: [[u32; 256]; 4],
    ip_maskl: [[u32; 256]; 8],
    ip_maskr: [[u32; 256]; 8],
    fp_maskl: [[u32; 256]; 8],
    fp_maskr: [[u32; 256]; 8],
    key_perm_maskl: [[u32; 128]; 8],
    key_perm_maskr: [[u32; 128]; 8],
    comp_maskl: [[u32; 128]; 8],
    comp_maskr: [[u32; 128]; 8],
}

fn des_tables() -> &'static DesTables {
    static TABLES: OnceLock<Box<DesTables>> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut t = Box::new(DesTables {
            m_sbox: vec![[0u8; 4096]; 4],
            psbox: [[0; 256]; 4],
            ip_maskl: [[0; 256]; 8],
            ip_maskr: [[0; 256]; 8],
            fp_maskl: [[0; 256]; 8],
            fp_maskr: [[0; 256]; 8],
            key_perm_maskl: [[0; 128]; 8],
            key_perm_maskr: [[0; 128]; 8],
            comp_maskl: [[0; 128]; 8],
            comp_maskr: [[0; 128]; 8],
        });
        let bits28 = |i: usize| bits32(i + 4);
        let bits24 = |i: usize| bits32(i + 8);

        let mut u_sbox = [[0u8; 64]; 8];
        for i in 0..8 {
            for j in 0..64 {
                let b = (j & 0x20) | ((j & 1) << 4) | ((j >> 1) & 0xf);
                u_sbox[i][j] = DES_SBOX[i][b];
            }
        }
        for b in 0..4 {
            for i in 0..64 {
                for j in 0..64 {
                    t.m_sbox[b][(i << 6) | j] = (u_sbox[b << 1][i] << 4) | u_sbox[(b << 1) + 1][j];
                }
            }
        }

        let mut init_perm = [0u8; 64];
        let mut final_perm = [0u8; 64];
        let mut inv_key_perm = [255u8; 64];
        let mut inv_comp_perm = [255u8; 56];
        for i in 0..64 {
            final_perm[i] = DES_IP[i] - 1;
            init_perm[final_perm[i] as usize] = i as u8;
        }
        for i in 0..56 {
            inv_key_perm[(DES_KEY_PERM[i] - 1) as usize] = i as u8;
        }
        for i in 0..48 {
            inv_comp_perm[(DES_COMP_PERM[i] - 1) as usize] = i as u8;
        }

        for k in 0..8 {
            for i in 0..256usize {
                let (mut il, mut ir, mut fl, mut fr) = (0u32, 0u32, 0u32, 0u32);
                for j in 0..8 {
                    let inbit = 8 * k + j;
                    if (i as u8) & BITS8[j] != 0 {
                        let obit = init_perm[inbit] as usize;
                        if obit < 32 {
                            il |= bits32(obit);
                        } else {
                            ir |= bits32(obit - 32);
                        }
                        let obit = final_perm[inbit] as usize;
                        if obit < 32 {
                            fl |= bits32(obit);
                        } else {
                            fr |= bits32(obit - 32);
                        }
                    }
                }
                t.ip_maskl[k][i] = il;
                t.ip_maskr[k][i] = ir;
                t.fp_maskl[k][i] = fl;
                t.fp_maskr[k][i] = fr;
            }
            for i in 0..128usize {
                let (mut il, mut ir) = (0u32, 0u32);
                for j in 0..7 {
                    let inbit = 8 * k + j;
                    if (i as u8) & BITS8[j + 1] != 0 {
                        let obit = inv_key_perm[inbit];
                        if obit == 255 {
                            continue;
                        }
                        let obit = obit as usize;
                        if obit < 28 {
                            il |= bits28(obit);
                        } else {
                            ir |= bits28(obit - 28);
                        }
                    }
                }
                t.key_perm_maskl[k][i] = il;
                t.key_perm_maskr[k][i] = ir;
                let (mut il, mut ir) = (0u32, 0u32);
                for j in 0..7 {
                    let inbit = 7 * k + j;
                    if (i as u8) & BITS8[j + 1] != 0 {
                        let obit = inv_comp_perm[inbit];
                        if obit == 255 {
                            continue;
                        }
                        let obit = obit as usize;
                        if obit < 24 {
                            il |= bits24(obit);
                        } else {
                            ir |= bits24(obit - 24);
                        }
                    }
                }
                t.comp_maskl[k][i] = il;
                t.comp_maskr[k][i] = ir;
            }
        }

        let mut un_pbox = [0u8; 32];
        for i in 0..32 {
            un_pbox[(DES_PBOX[i] - 1) as usize] = i as u8;
        }
        for b in 0..4 {
            for i in 0..256usize {
                let mut p = 0u32;
                for j in 0..8 {
                    if (i as u8) & BITS8[j] != 0 {
                        p |= bits32(un_pbox[8 * b + j] as usize);
                    }
                }
                t.psbox[b][i] = p;
            }
        }
        t
    })
}

struct DesData {
    saltbits: u32,
    old_salt: u32,
    en_keysl: [u32; 16],
    en_keysr: [u32; 16],
}

impl DesData {
    fn new() -> Self {
        DesData { saltbits: 0, old_salt: 0, en_keysl: [0; 16], en_keysr: [0; 16] }
    }

    fn setup_salt(&mut self, salt: u32) {
        if salt == self.old_salt {
            return;
        }
        self.old_salt = salt;
        let mut saltbits = 0u32;
        let mut saltbit = 1u32;
        let mut obit = 0x800000u32;
        for _ in 0..24 {
            if salt & saltbit != 0 {
                saltbits |= obit;
            }
            saltbit <<= 1;
            obit >>= 1;
        }
        self.saltbits = saltbits;
    }

    fn setkey(&mut self, t: &DesTables, key: &[u8; 8]) {
        let rawkey0 = u32::from_be_bytes([key[0], key[1], key[2], key[3]]);
        let rawkey1 = u32::from_be_bytes([key[4], key[5], key[6], key[7]]);
        let k0 = t.key_perm_maskl[0][(rawkey0 >> 25) as usize]
            | t.key_perm_maskl[1][((rawkey0 >> 17) & 0x7f) as usize]
            | t.key_perm_maskl[2][((rawkey0 >> 9) & 0x7f) as usize]
            | t.key_perm_maskl[3][((rawkey0 >> 1) & 0x7f) as usize]
            | t.key_perm_maskl[4][(rawkey1 >> 25) as usize]
            | t.key_perm_maskl[5][((rawkey1 >> 17) & 0x7f) as usize]
            | t.key_perm_maskl[6][((rawkey1 >> 9) & 0x7f) as usize]
            | t.key_perm_maskl[7][((rawkey1 >> 1) & 0x7f) as usize];
        let k1 = t.key_perm_maskr[0][(rawkey0 >> 25) as usize]
            | t.key_perm_maskr[1][((rawkey0 >> 17) & 0x7f) as usize]
            | t.key_perm_maskr[2][((rawkey0 >> 9) & 0x7f) as usize]
            | t.key_perm_maskr[3][((rawkey0 >> 1) & 0x7f) as usize]
            | t.key_perm_maskr[4][(rawkey1 >> 25) as usize]
            | t.key_perm_maskr[5][((rawkey1 >> 17) & 0x7f) as usize]
            | t.key_perm_maskr[6][((rawkey1 >> 9) & 0x7f) as usize]
            | t.key_perm_maskr[7][((rawkey1 >> 1) & 0x7f) as usize];
        let mut shifts = 0u32;
        for round in 0..16 {
            shifts += DES_KEY_SHIFTS[round] as u32;
            let t0 = (k0 << shifts) | (k0 >> (28 - shifts));
            let t1 = (k1 << shifts) | (k1 >> (28 - shifts));
            self.en_keysl[round] = t.comp_maskl[0][((t0 >> 21) & 0x7f) as usize]
                | t.comp_maskl[1][((t0 >> 14) & 0x7f) as usize]
                | t.comp_maskl[2][((t0 >> 7) & 0x7f) as usize]
                | t.comp_maskl[3][(t0 & 0x7f) as usize]
                | t.comp_maskl[4][((t1 >> 21) & 0x7f) as usize]
                | t.comp_maskl[5][((t1 >> 14) & 0x7f) as usize]
                | t.comp_maskl[6][((t1 >> 7) & 0x7f) as usize]
                | t.comp_maskl[7][(t1 & 0x7f) as usize];
            self.en_keysr[round] = t.comp_maskr[0][((t0 >> 21) & 0x7f) as usize]
                | t.comp_maskr[1][((t0 >> 14) & 0x7f) as usize]
                | t.comp_maskr[2][((t0 >> 7) & 0x7f) as usize]
                | t.comp_maskr[3][(t0 & 0x7f) as usize]
                | t.comp_maskr[4][((t1 >> 21) & 0x7f) as usize]
                | t.comp_maskr[5][((t1 >> 14) & 0x7f) as usize]
                | t.comp_maskr[6][((t1 >> 7) & 0x7f) as usize]
                | t.comp_maskr[7][(t1 & 0x7f) as usize];
        }
    }

    /// do_des with count > 0 (encryption only, which is all crypt() uses).
    fn do_des(&self, t: &DesTables, l_in: u32, r_in: u32, count: u32) -> (u32, u32) {
        let mut l = t.ip_maskl[0][(l_in >> 24) as usize]
            | t.ip_maskl[1][((l_in >> 16) & 0xff) as usize]
            | t.ip_maskl[2][((l_in >> 8) & 0xff) as usize]
            | t.ip_maskl[3][(l_in & 0xff) as usize]
            | t.ip_maskl[4][(r_in >> 24) as usize]
            | t.ip_maskl[5][((r_in >> 16) & 0xff) as usize]
            | t.ip_maskl[6][((r_in >> 8) & 0xff) as usize]
            | t.ip_maskl[7][(r_in & 0xff) as usize];
        let mut r = t.ip_maskr[0][(l_in >> 24) as usize]
            | t.ip_maskr[1][((l_in >> 16) & 0xff) as usize]
            | t.ip_maskr[2][((l_in >> 8) & 0xff) as usize]
            | t.ip_maskr[3][(l_in & 0xff) as usize]
            | t.ip_maskr[4][(r_in >> 24) as usize]
            | t.ip_maskr[5][((r_in >> 16) & 0xff) as usize]
            | t.ip_maskr[6][((r_in >> 8) & 0xff) as usize]
            | t.ip_maskr[7][(r_in & 0xff) as usize];
        let saltbits = self.saltbits;
        let mut f = 0u32;
        for _ in 0..count {
            for round in 0..16 {
                let mut r48l = ((r & 0x00000001) << 23)
                    | ((r & 0xf8000000) >> 9)
                    | ((r & 0x1f800000) >> 11)
                    | ((r & 0x01f80000) >> 13)
                    | ((r & 0x001f8000) >> 15);
                let mut r48r = ((r & 0x0001f800) << 7)
                    | ((r & 0x00001f80) << 5)
                    | ((r & 0x000001f8) << 3)
                    | ((r & 0x0000001f) << 1)
                    | ((r & 0x80000000) >> 31);
                f = (r48l ^ r48r) & saltbits;
                r48l ^= f ^ self.en_keysl[round];
                r48r ^= f ^ self.en_keysr[round];
                f = t.psbox[0][t.m_sbox[0][(r48l >> 12) as usize] as usize]
                    | t.psbox[1][t.m_sbox[1][(r48l & 0xfff) as usize] as usize]
                    | t.psbox[2][t.m_sbox[2][(r48r >> 12) as usize] as usize]
                    | t.psbox[3][t.m_sbox[3][(r48r & 0xfff) as usize] as usize];
                f ^= l;
                l = r;
                r = f;
            }
            r = l;
            l = f;
        }
        let l_out = t.fp_maskl[0][(l >> 24) as usize]
            | t.fp_maskl[1][((l >> 16) & 0xff) as usize]
            | t.fp_maskl[2][((l >> 8) & 0xff) as usize]
            | t.fp_maskl[3][(l & 0xff) as usize]
            | t.fp_maskl[4][(r >> 24) as usize]
            | t.fp_maskl[5][((r >> 16) & 0xff) as usize]
            | t.fp_maskl[6][((r >> 8) & 0xff) as usize]
            | t.fp_maskl[7][(r & 0xff) as usize];
        let r_out = t.fp_maskr[0][(l >> 24) as usize]
            | t.fp_maskr[1][((l >> 16) & 0xff) as usize]
            | t.fp_maskr[2][((l >> 8) & 0xff) as usize]
            | t.fp_maskr[3][(l & 0xff) as usize]
            | t.fp_maskr[4][(r >> 24) as usize]
            | t.fp_maskr[5][((r >> 16) & 0xff) as usize]
            | t.fp_maskr[6][((r >> 8) & 0xff) as usize]
            | t.fp_maskr[7][(r & 0xff) as usize];
        (l_out, r_out)
    }

    fn des_cipher(&mut self, t: &DesTables, input: &[u8; 8], salt: u32, count: u32) -> [u8; 8] {
        self.setup_salt(salt);
        let rawl = u32::from_be_bytes([input[0], input[1], input[2], input[3]]);
        let rawr = u32::from_be_bytes([input[4], input[5], input[6], input[7]]);
        let (l_out, r_out) = self.do_des(t, rawl, rawr, count);
        let mut out = [0u8; 8];
        out[..4].copy_from_slice(&l_out.to_be_bytes());
        out[4..].copy_from_slice(&r_out.to_be_bytes());
        out
    }
}

fn des_ascii_to_bin(ch: u8) -> u32 {
    let sch = ch as i8 as i32;
    let mut retval = sch - b'.' as i32;
    if sch >= b'A' as i32 {
        retval = sch - (b'A' as i32 - 12);
        if sch >= b'a' as i32 {
            retval = sch - (b'a' as i32 - 38);
        }
    }
    (retval & 0x3f) as u32
}

#[inline]
fn des_ascii_is_unsafe(ch: u8) -> bool {
    ch == 0 || ch == b'\n' || ch == b':'
}

/// _crypt_extended_r()
fn crypt_extended(key: &[u8], setting: &[u8]) -> Option<Vec<u8>> {
    let t = des_tables();
    let mut data = DesData::new();
    let kat = |i: usize| -> u8 { if i < key.len() { key[i] } else { 0 } };

    let mut keybuf = [0u8; 8];
    let mut kp = 0usize;
    for b in keybuf.iter_mut() {
        *b = kat(kp) << 1;
        if kat(kp) != 0 {
            kp += 1;
        }
    }
    data.setkey(t, &keybuf);

    let mut output: Vec<u8>;
    let count: u32;
    let salt: u32;
    if at(setting, 0) == b'_' {
        let mut c = 0u32;
        for i in 1..5 {
            let value = des_ascii_to_bin(at(setting, i));
            if ITOA64[value as usize] != at(setting, i) {
                return None;
            }
            c |= value << ((i - 1) * 6);
        }
        if c == 0 {
            return None;
        }
        count = c;
        let mut s = 0u32;
        for i in 5..9 {
            let value = des_ascii_to_bin(at(setting, i));
            if ITOA64[value as usize] != at(setting, i) {
                return None;
            }
            s |= value << ((i - 5) * 6);
        }
        salt = s;

        while kat(kp) != 0 {
            keybuf = data.des_cipher(t, &keybuf, 0, 1);
            let mut q = 0usize;
            while q < 8 && kat(kp) != 0 {
                keybuf[q] ^= kat(kp) << 1;
                q += 1;
                kp += 1;
            }
            data.setkey(t, &keybuf);
        }
        output = (0..9).map(|i| at(setting, i)).collect();
    } else {
        count = 25;
        if des_ascii_is_unsafe(at(setting, 0)) || des_ascii_is_unsafe(at(setting, 1)) {
            return None;
        }
        salt = (des_ascii_to_bin(at(setting, 1)) << 6) | des_ascii_to_bin(at(setting, 0));
        output = vec![at(setting, 0), at(setting, 1)];
    }
    data.setup_salt(salt);
    let (r0, r1) = data.do_des(t, 0, 0, count);

    let mut l = r0 >> 8;
    output.push(ITOA64[((l >> 18) & 0x3f) as usize]);
    output.push(ITOA64[((l >> 12) & 0x3f) as usize]);
    output.push(ITOA64[((l >> 6) & 0x3f) as usize]);
    output.push(ITOA64[(l & 0x3f) as usize]);
    l = (r0 << 16) | ((r1 >> 16) & 0xffff);
    output.push(ITOA64[((l >> 18) & 0x3f) as usize]);
    output.push(ITOA64[((l >> 12) & 0x3f) as usize]);
    output.push(ITOA64[((l >> 6) & 0x3f) as usize]);
    output.push(ITOA64[(l & 0x3f) as usize]);
    l = r1 << 2;
    output.push(ITOA64[((l >> 12) & 0x3f) as usize]);
    output.push(ITOA64[((l >> 6) & 0x3f) as usize]);
    output.push(ITOA64[(l & 0x3f) as usize]);
    Some(output)
}

// ===================================================================
// PHP bindings
// ===================================================================

pub fn register(vm: &mut Vm) {
    vm.register_function(b"crypt", crypt_fn);
    vm.register_function(b"password_hash", password_hash_fn);
    vm.register_function(b"password_verify", password_verify_fn);
    vm.register_function(b"password_get_info", password_get_info_fn);
    vm.register_function(b"password_needs_rehash", password_needs_rehash_fn);
    vm.register_function(b"password_algos", password_algos_fn);

    let s = || Some(ParamType::Simple(b"string".to_vec()));
    let a = || Some(ParamType::Simple(b"array".to_vec()));
    let sin = || {
        Some(ParamType::Union(vec![
            ParamType::Simple(b"string".to_vec()),
            ParamType::Simple(b"int".to_vec()),
            ParamType::Simple(b"null".to_vec()),
        ]))
    };
    vm.register_builtin_param_types(b"crypt", vec![s(), s()]);
    vm.register_builtin_param_types(b"password_hash", vec![s(), sin(), a()]);
    vm.register_builtin_param_types(b"password_verify", vec![s(), s()]);
    vm.register_builtin_param_types(b"password_get_info", vec![s()]);
    vm.register_builtin_param_types(b"password_needs_rehash", vec![s(), sin(), a()]);

    let names = |n: &[&[u8]]| n.iter().map(|p| p.to_vec()).collect::<Vec<_>>();
    vm.builtin_param_names.insert(b"crypt".to_vec(), names(&[b"string", b"salt"]));
    vm.builtin_param_names.insert(b"password_hash".to_vec(), names(&[b"password", b"algo", b"options"]));
    vm.builtin_param_names.insert(b"password_verify".to_vec(), names(&[b"password", b"hash"]));
    vm.builtin_param_names.insert(b"password_get_info".to_vec(), names(&[b"hash"]));
    vm.builtin_param_names.insert(b"password_needs_rehash".to_vec(), names(&[b"hash", b"algo", b"options"]));
    vm.builtin_param_names.insert(b"password_algos".to_vec(), vec![]);

    vm.constants.insert(b"PASSWORD_BCRYPT_DEFAULT_COST".to_vec(), Value::Long(PHP_PASSWORD_BCRYPT_COST));
}

fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> VmError {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError { message: msg.to_string(), line: vm.current_line }
}

fn check_arg_count(vm: &mut Vm, fname: &str, args: &[Value], min: usize, max: usize) -> Result<(), VmError> {
    let n = args.len();
    if n >= min && n <= max {
        return Ok(());
    }
    let (kind, cnt) = if min == max {
        ("exactly", min)
    } else if n < min {
        ("at least", min)
    } else {
        ("at most", max)
    };
    let msg = format!(
        "{}() expects {} {} argument{}, {} given",
        fname,
        kind,
        cnt,
        if cnt == 1 { "" } else { "s" },
        n
    );
    Err(throw(vm, b"ArgumentCountError", &msg))
}

fn has_tostring(vm: &Vm, v: &Value) -> bool {
    if let Value::Object(o) = v {
        let lower: Vec<u8> = o.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
        return vm.classes.get(&lower).map(|c| c.get_method(b"__tostring").is_some()).unwrap_or(false);
    }
    false
}

/// Z_PARAM_STR (weak mode; the VM already reports null deprecations).
fn zpp_str(vm: &mut Vm, fname: &str, idx: usize, pname: &str, v: &Value) -> Result<PhpString, VmError> {
    let v = v.deref();
    match &v {
        Value::String(s) => Ok(s.clone()),
        Value::Array(_) | Value::Resource(_) | Value::Generator(_) => {
            let msg = format!(
                "{}(): Argument #{} (${}) must be of type string, {} given",
                fname,
                idx + 1,
                pname,
                Vm::value_type_name(&v)
            );
            Err(throw(vm, b"TypeError", &msg))
        }
        Value::Object(_) if !has_tostring(vm, &v) => {
            let msg = format!(
                "{}(): Argument #{} (${}) must be of type string, {} given",
                fname,
                idx + 1,
                pname,
                Vm::value_type_name(&v)
            );
            Err(throw(vm, b"TypeError", &msg))
        }
        _ => vm.value_to_string_checked(&v),
    }
}

/// Z_PARAM_ARRAY_HT
fn zpp_array(vm: &mut Vm, fname: &str, idx: usize, pname: &str, v: &Value) -> Result<Rc<RefCell<PhpArray>>, VmError> {
    let v = v.deref();
    match &v {
        Value::Array(a) => Ok(a.clone()),
        _ => {
            let msg = format!(
                "{}(): Argument #{} (${}) must be of type array, {} given",
                fname,
                idx + 1,
                pname,
                Vm::value_type_name(&v)
            );
            Err(throw(vm, b"TypeError", &msg))
        }
    }
}

enum AlgoArg {
    Null,
    Str(PhpString),
    Long(i64),
}

/// Z_PARAM_STR_OR_LONG_OR_NULL (weak mode)
fn zpp_str_or_long_or_null(vm: &mut Vm, fname: &str, idx: usize, pname: &str, v: &Value) -> Result<AlgoArg, VmError> {
    let v = v.deref();
    match &v {
        Value::Null | Value::Undef => Ok(AlgoArg::Null),
        Value::String(s) => Ok(AlgoArg::Str(s.clone())),
        Value::Long(n) => Ok(AlgoArg::Long(*n)),
        Value::True => Ok(AlgoArg::Long(1)),
        Value::False => Ok(AlgoArg::Long(0)),
        Value::Double(d) if d.is_finite() && *d >= -9.223372036854775808e18 && *d < 9.223372036854775808e18 => {
            if d.fract() != 0.0 {
                vm.emit_deprecated(&format!(
                    "Implicit conversion from float {} to int loses precision",
                    Value::Double(*d).to_php_string().to_string_lossy()
                ));
            }
            Ok(AlgoArg::Long(*d as i64))
        }
        Value::Double(_) => Ok(AlgoArg::Str(v.to_php_string())),
        Value::Object(_) if has_tostring(vm, &v) => Ok(AlgoArg::Str(vm.value_to_string_checked(&v)?)),
        _ => {
            let msg = format!(
                "{}(): Argument #{} (${}) must be of type string|int|null, {} given",
                fname,
                idx + 1,
                pname,
                Vm::value_type_name(&v)
            );
            Err(throw(vm, b"TypeError", &msg))
        }
    }
}

fn crypt_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    check_arg_count(vm, "crypt", args, 2, 2)?;
    let s = zpp_str(vm, "crypt", 0, "string", &args[0])?;
    let salt_in = zpp_str(vm, "crypt", 1, "salt", &args[1])?;

    let mut salt = [0u8; PHP_MAX_SALT_LEN + 1];
    for b in salt[1..PHP_MAX_SALT_LEN].iter_mut() {
        *b = b'$';
    }
    let salt_in = salt_in.as_bytes();
    let len = salt_in.len().min(PHP_MAX_SALT_LEN);
    salt[..len].copy_from_slice(&salt_in[..len]);
    salt[len] = 0;

    match php_crypt(s.as_bytes(), &salt) {
        Some(r) => Ok(Value::String(PhpString::from_vec(r))),
        None => {
            if salt[0] == b'*' && salt[1] == b'0' {
                Ok(Value::String(PhpString::from_bytes(b"*1")))
            } else {
                Ok(Value::String(PhpString::from_bytes(b"*0")))
            }
        }
    }
}

// ---------- password.c ----------

#[derive(Clone, Copy, PartialEq)]
enum PasswordAlgo {
    Bcrypt,
}

/// The registered password algorithms (php_password_algos); argon2 is not built.
fn password_algo_find(ident: &[u8]) -> Option<PasswordAlgo> {
    if ident == b"2y" { Some(PasswordAlgo::Bcrypt) } else { None }
}

fn password_algo_find_arg(arg: &AlgoArg) -> Option<PasswordAlgo> {
    match arg {
        AlgoArg::Null => Some(PasswordAlgo::Bcrypt),
        AlgoArg::Str(s) => password_algo_find(s.as_bytes()),
        AlgoArg::Long(0) | AlgoArg::Long(1) => Some(PasswordAlgo::Bcrypt),
        AlgoArg::Long(2) => password_algo_find(b"argon2i"),
        AlgoArg::Long(3) => password_algo_find(b"argon2id"),
        AlgoArg::Long(_) => None,
    }
}

fn password_algo_extract_ident(hash: &[u8]) -> Option<Vec<u8>> {
    if hash.len() < 3 {
        return None;
    }
    let rest = cstr(&hash[1..]);
    let end = rest.iter().position(|&b| b == b'$')?;
    Some(rest[..end].to_vec())
}

fn password_algo_valid(algo: PasswordAlgo, hash: &[u8]) -> bool {
    match algo {
        PasswordAlgo::Bcrypt => hash.len() == 60 && hash[0] == b'$' && hash[1] == b'2' && hash[2] == b'y',
    }
}

fn password_algo_identify_ex(hash: &[u8], default: Option<PasswordAlgo>) -> Option<PasswordAlgo> {
    let Some(ident) = password_algo_extract_ident(hash) else {
        return default;
    };
    match password_algo_find(&ident) {
        Some(a) if password_algo_valid(a, hash) => Some(a),
        _ => default,
    }
}

/// sscanf(hash, "$2y$" ZEND_LONG_FMT "$", &cost)
fn bcrypt_scan_cost(hash: &[u8], default: i64) -> i64 {
    let h = cstr(hash);
    if !h.starts_with(b"$2y$") {
        return default;
    }
    let mut i = 4;
    while i < h.len() && matches!(h[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < h.len() && (h[i] == b'+' || h[i] == b'-') {
        neg = h[i] == b'-';
        i += 1;
    }
    let start = i;
    let mut v: i64 = 0;
    while i < h.len() && h[i].is_ascii_digit() {
        v = v.wrapping_mul(10).wrapping_add((h[i] - b'0') as i64);
        i += 1;
    }
    if i == start {
        return default;
    }
    if neg { v.wrapping_neg() } else { v }
}

fn option_long(vm: &mut Vm, options: Option<&Rc<RefCell<PhpArray>>>, key: &[u8]) -> Option<i64> {
    let v = options.and_then(|o| o.borrow().get_str(key).cloned())?;
    let v = v.deref();
    Some(match &v {
        Value::Object(_) => {
            // zval_get_long on an object: warning + 1
            let cls = Vm::value_type_name(&v);
            vm.emit_warning(&format!("Object of class {} could not be converted to int", cls));
            1
        }
        _ => v.to_long(),
    })
}

fn safe_bcmp(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut r = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        r |= x ^ y;
    }
    r == 0
}

fn with_nul(s: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s);
    v.push(0);
    v
}

fn bcrypt_verify(password: &[u8], hash: &[u8]) -> bool {
    let Some(ret) = php_crypt(password, &with_nul(hash)) else {
        return false;
    };
    if hash.len() < 13 {
        return false;
    }
    safe_bcmp(&ret, hash)
}

fn os_random(buf: &mut [u8]) -> bool {
    let mut filled = 0usize;
    while filled < buf.len() {
        let r = unsafe {
            libc::getrandom(buf[filled..].as_mut_ptr() as *mut libc::c_void, buf.len() - filled, 0)
        };
        if r < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return false;
        }
        filled += r as usize;
    }
    true
}

fn base64_encode(input: &[u8]) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[(n >> 18) as usize & 63]);
        out.push(T[(n >> 12) as usize & 63]);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] } else { b'=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] } else { b'=' });
    }
    out
}

/// php_password_make_salt()
fn password_make_salt(vm: &mut Vm, length: usize) -> Result<Vec<u8>, VmError> {
    let mut buffer = vec![0u8; length * 3 / 4 + 1];
    if !os_random(&mut buffer) {
        return Err(throw(vm, b"ValueError", "Unable to generate salt"));
    }
    let enc = base64_encode(&buffer);
    if enc.len() < length {
        return Err(throw(vm, b"ValueError", "Generated salt too short"));
    }
    let mut ret = Vec::with_capacity(length);
    for &c in &enc[..length] {
        match c {
            b'+' => ret.push(b'.'),
            b'=' => return Err(throw(vm, b"ValueError", "Generated salt too short")),
            _ => ret.push(c),
        }
    }
    Ok(ret)
}

fn bcrypt_hash(vm: &mut Vm, password: &[u8], options: Option<&Rc<RefCell<PhpArray>>>) -> Result<Option<Vec<u8>>, VmError> {
    if password.contains(&0) {
        return Err(throw(vm, b"ValueError", "Bcrypt password must not contain null character"));
    }
    let cost = option_long(vm, options, b"cost").unwrap_or(PHP_PASSWORD_BCRYPT_COST);
    if !(4..=31).contains(&cost) {
        let msg = format!("Invalid bcrypt cost parameter specified: {}", cost);
        return Err(throw(vm, b"ValueError", &msg));
    }
    if options.map(|o| o.borrow().get_str(b"salt").is_some()).unwrap_or(false) {
        vm.emit_warning(
            "password_hash(): The \"salt\" option has been ignored, since providing a custom salt is no longer supported",
        );
    }
    let salt = password_make_salt(vm, 22)?;
    let mut hash = format!("$2y${:02}$", cost).into_bytes();
    hash.extend_from_slice(&salt);
    let Some(result) = php_crypt(password, &with_nul(&hash)) else {
        return Ok(None);
    };
    if result.len() < 13 {
        return Ok(None);
    }
    Ok(Some(result))
}

fn password_hash_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "password_hash";
    check_arg_count(vm, F, args, 2, 3)?;
    let password = zpp_str(vm, F, 0, "password", &args[0])?;
    let algo_arg = zpp_str_or_long_or_null(vm, F, 1, "algo", &args[1])?;
    let options = match args.get(2) {
        Some(v) => Some(zpp_array(vm, F, 2, "options", v)?),
        None => None,
    };
    let Some(algo) = password_algo_find_arg(&algo_arg) else {
        return Err(throw(
            vm,
            b"ValueError",
            "password_hash(): Argument #2 ($algo) must be a valid password hashing algorithm",
        ));
    };
    let digest = match algo {
        PasswordAlgo::Bcrypt => bcrypt_hash(vm, password.as_bytes(), options.as_ref())?,
    };
    match digest {
        Some(d) => Ok(Value::String(PhpString::from_vec(d))),
        None => Err(throw(vm, b"Error", "Password hashing failed for unknown reason")),
    }
}

fn password_verify_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "password_verify";
    check_arg_count(vm, F, args, 2, 2)?;
    let password = zpp_str(vm, F, 0, "password", &args[0])?;
    let hash = zpp_str(vm, F, 1, "hash", &args[1])?;
    let ok = match password_algo_identify_ex(hash.as_bytes(), Some(PasswordAlgo::Bcrypt)) {
        Some(PasswordAlgo::Bcrypt) => bcrypt_verify(password.as_bytes(), hash.as_bytes()),
        None => false,
    };
    Ok(if ok { Value::True } else { Value::False })
}

fn new_array(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

fn password_get_info_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "password_get_info";
    check_arg_count(vm, F, args, 1, 1)?;
    let hash = zpp_str(vm, F, 0, "hash", &args[0])?;
    let hash = hash.as_bytes();
    let key = |s: &[u8]| ArrayKey::String(PhpString::from_bytes(s));
    let mut ret = PhpArray::new();
    let mut options = PhpArray::new();
    let ident = password_algo_extract_ident(hash);
    let algo = ident.as_deref().and_then(password_algo_find);
    match algo {
        Some(a) if password_algo_valid(a, hash) => {
            ret.set(key(b"algo"), Value::String(PhpString::from_vec(ident.unwrap_or_default())));
            match a {
                PasswordAlgo::Bcrypt => {
                    ret.set(key(b"algoName"), Value::String(PhpString::from_bytes(b"bcrypt")));
                    options.set(key(b"cost"), Value::Long(bcrypt_scan_cost(hash, PHP_PASSWORD_BCRYPT_COST)));
                }
            }
            ret.set(key(b"options"), new_array(options));
        }
        _ => {
            ret.set(key(b"algo"), Value::Null);
            ret.set(key(b"algoName"), Value::String(PhpString::from_bytes(b"unknown")));
            ret.set(key(b"options"), new_array(options));
        }
    }
    Ok(new_array(ret))
}

fn password_needs_rehash_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    const F: &str = "password_needs_rehash";
    check_arg_count(vm, F, args, 2, 3)?;
    let hash = zpp_str(vm, F, 0, "hash", &args[0])?;
    let algo_arg = zpp_str_or_long_or_null(vm, F, 1, "algo", &args[1])?;
    let options = match args.get(2) {
        Some(v) => Some(zpp_array(vm, F, 2, "options", v)?),
        None => None,
    };
    let Some(new_algo) = password_algo_find_arg(&algo_arg) else {
        return Ok(Value::False);
    };
    let old_algo = password_algo_identify_ex(hash.as_bytes(), None);
    if old_algo != Some(new_algo) {
        return Ok(Value::True);
    }
    let needs = match new_algo {
        PasswordAlgo::Bcrypt => {
            let old_cost = bcrypt_scan_cost(hash.as_bytes(), PHP_PASSWORD_BCRYPT_COST);
            let new_cost = option_long(vm, options.as_ref(), b"cost").unwrap_or(PHP_PASSWORD_BCRYPT_COST);
            old_cost != new_cost
        }
    };
    Ok(if needs { Value::True } else { Value::False })
}

fn password_algos_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    check_arg_count(vm, "password_algos", args, 0, 0)?;
    let mut arr = PhpArray::new();
    arr.push(Value::String(PhpString::from_bytes(b"2y")));
    Ok(new_array(arr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(pw: &str, salt: &[u8]) -> Option<String> {
        php_crypt(pw.as_bytes(), &with_nul(salt)).map(|v| String::from_utf8_lossy(&v).into_owned())
    }

    #[test]
    fn freesec_vectors() {
        for (hash, pw) in [
            ("_J9..CCCCXBrJUJV154M", "U*U*U*U*"),
            ("_J9..CCCCXUhOBTXzaiE", "U*U***U"),
            ("_J9..SDizxmRI1GjnQuE", "zxyDPWgydbQjgq"),
            ("_K9..SaltNrQgIYUAeoY", "726 even"),
            ("_J9..SDSD5YGyRCr4W4c", ""),
            ("CCNf8Sbh3HDfQ", "U*U*U*U*"),
            ("SDbsugeBiC58A", ""),
            ("./xZjzHv5vzVE", "password"),
            ("zZDDIZ0NOlPzw", "password"),
        ] {
            assert_eq!(crypt_extended(pw.as_bytes(), &with_nul(hash.as_bytes())).map(|v| String::from_utf8(v).unwrap()), Some(hash.to_string()));
        }
    }

    #[test]
    fn blowfish_selftest() {
        let key = b"8b \xd0\xc1\xd2\xcf\xcc\xd8";
        let r = bf_crypt(key, b"$2a$00$abcdefghijklmnopqrstuu\0", 1).unwrap();
        assert_eq!(&r[29..], b"i1D709vfamulimlGcq0qq3UvuUasvEa");
        let r = bf_crypt(key, b"$2x$00$abcdefghijklmnopqrstuu\0", 1).unwrap();
        assert_eq!(&r[29..], b"VUrPmXD6q/nVSSp7pNDhCR9071IfIRe");
        let k = b"\xff\xa3" as &[u8];
        let k: Vec<u8> = [k, b"34", b"\xff\xff\xff\xa3", b"345"].concat();
        let (ae, mut ai) = bf_set_key(&k, 2);
        let (ye, yi) = bf_set_key(&k, 4);
        ai[0] ^= 0x10000;
        assert_eq!(ai[0], 0xdb9c59bc);
        assert_eq!(ye[17], 0x33343500);
        assert_eq!(ae, ye);
        assert_eq!(ai, yi);
    }

    #[test]
    fn md5_and_sha() {
        assert_eq!(c("rasmuslerdorf", b"$1$rasmusle$").as_deref(), Some("$1$rasmusle$rISCgZzpwk3UhDidwXvin0"));
        assert_eq!(
            c("Hello world!", b"$5$saltstring").as_deref(),
            Some("$5$saltstring$5B8vYYiY.CVt1RlTTf8KbXBH3hsxY/GNooZaBBGWEc5")
        );
        assert_eq!(
            c("Hello world!", b"$6$saltstring").as_deref(),
            Some("$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1")
        );
    }
}
