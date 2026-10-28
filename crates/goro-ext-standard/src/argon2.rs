//! Argon2 (RFC 9106, version 0x13) for password_hash()/password_verify(),
//! with the PHC string format PHP uses
//! (`$argon2id$v=19$m=65536,t=4,p=1$salt$hash`, unpadded base64).

use purecrypto::hash::Blake2bMac;

/// Argon2 variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    D = 0,
    I = 1,
    Id = 2,
}

const BLOCK_WORDS: usize = 128;
type Block = [u64; BLOCK_WORDS];

fn blake2b(out_len: usize, parts: &[&[u8]]) -> Vec<u8> {
    let mut h = Blake2bMac::new_unkeyed(out_len);
    for p in parts {
        h.update(p);
    }
    let mut out = vec![0u8; out_len];
    h.finalize_into(&mut out);
    out
}

/// H' of RFC 9106 section 3.3 (variable-length hash).
fn h_prime(t: usize, x: &[u8]) -> Vec<u8> {
    let tl = (t as u32).to_le_bytes();
    if t <= 64 {
        return blake2b(t, &[&tl, x]);
    }
    let r = t.div_ceil(32) - 2;
    let mut out = Vec::with_capacity(t);
    let mut v = blake2b(64, &[&tl, x]);
    out.extend_from_slice(&v[..32]);
    for _ in 1..r {
        v = blake2b(64, &[&v]);
        out.extend_from_slice(&v[..32]);
    }
    let last = blake2b(t - 32 * r, &[&v]);
    out.extend_from_slice(&last);
    out
}

#[inline]
fn fbla(a: u64, b: u64) -> u64 {
    a.wrapping_add(b)
        .wrapping_add(2u64.wrapping_mul((a & 0xffff_ffff).wrapping_mul(b & 0xffff_ffff)))
}

#[inline]
fn gb(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize) {
    v[a] = fbla(v[a], v[b]);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = fbla(v[c], v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = fbla(v[a], v[b]);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = fbla(v[c], v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

fn perm(v: &mut [u64; 16]) {
    gb(v, 0, 4, 8, 12);
    gb(v, 1, 5, 9, 13);
    gb(v, 2, 6, 10, 14);
    gb(v, 3, 7, 11, 15);
    gb(v, 0, 5, 10, 15);
    gb(v, 1, 6, 11, 12);
    gb(v, 2, 7, 8, 13);
    gb(v, 3, 4, 9, 14);
}

/// The compression function G(X, Y).
fn compress(x: &Block, y: &Block) -> Block {
    let mut r = [0u64; BLOCK_WORDS];
    for i in 0..BLOCK_WORDS {
        r[i] = x[i] ^ y[i];
    }
    let mut q = r;
    for row in 0..8 {
        let mut v = [0u64; 16];
        v.copy_from_slice(&q[16 * row..16 * row + 16]);
        perm(&mut v);
        q[16 * row..16 * row + 16].copy_from_slice(&v);
    }
    for col in 0..8 {
        let mut v = [0u64; 16];
        for k in 0..8 {
            v[2 * k] = q[2 * col + 16 * k];
            v[2 * k + 1] = q[2 * col + 16 * k + 1];
        }
        perm(&mut v);
        for k in 0..8 {
            q[2 * col + 16 * k] = v[2 * k];
            q[2 * col + 16 * k + 1] = v[2 * k + 1];
        }
    }
    for i in 0..BLOCK_WORDS {
        q[i] ^= r[i];
    }
    q
}

fn block_from_bytes(b: &[u8]) -> Block {
    let mut out = [0u64; BLOCK_WORDS];
    for (i, w) in out.iter_mut().enumerate() {
        *w = u64::from_le_bytes(b[8 * i..8 * i + 8].try_into().unwrap());
    }
    out
}

/// Computes an Argon2 tag. `m` is in KiB (at least 8 per lane).
pub fn hash(
    variant: Variant,
    pwd: &[u8],
    salt: &[u8],
    t_cost: u32,
    m_cost: u32,
    lanes: u32,
    tag_len: usize,
) -> Vec<u8> {
    let p = lanes.max(1) as usize;
    let le = |x: u32| x.to_le_bytes();
    let h0 = blake2b(
        64,
        &[
            &le(lanes),
            &le(tag_len as u32),
            &le(m_cost),
            &le(t_cost),
            &le(0x13),
            &le(variant as u32),
            &le(pwd.len() as u32),
            pwd,
            &le(salt.len() as u32),
            salt,
            &le(0),
            &le(0),
        ],
    );
    let m_prime = (4 * p) * ((m_cost as usize).max(8 * p) / (4 * p));
    let q = m_prime / p;
    let seg = q / 4;
    let mut mem: Vec<Block> = vec![[0u64; BLOCK_WORDS]; m_prime];
    for l in 0..p {
        for j in 0..2 {
            let mut input = h0.clone();
            input.extend_from_slice(&le(j as u32));
            input.extend_from_slice(&le(l as u32));
            mem[l * q + j] = block_from_bytes(&h_prime(1024, &input));
        }
    }
    let zero = [0u64; BLOCK_WORDS];
    for pass in 0..t_cost as usize {
        for slice in 0..4 {
            for l in 0..p {
                let data_indep = match variant {
                    Variant::I => true,
                    Variant::D => false,
                    Variant::Id => pass == 0 && slice < 2,
                };
                let mut addr = [0u64; BLOCK_WORDS];
                let mut input = [0u64; BLOCK_WORDS];
                if data_indep {
                    input[0] = pass as u64;
                    input[1] = l as u64;
                    input[2] = slice as u64;
                    input[3] = m_prime as u64;
                    input[4] = t_cost as u64;
                    input[5] = variant as u64;
                }
                let start = if pass == 0 && slice == 0 { 2 } else { 0 };
                let next_addr = |input: &mut Block| -> Block {
                    input[6] += 1;
                    compress(&zero, &compress(&zero, input))
                };
                if data_indep && start != 0 {
                    addr = next_addr(&mut input);
                }
                for idx in start..seg {
                    let j = slice * seg + idx;
                    let cur = l * q + j;
                    let prev = if j == 0 { l * q + q - 1 } else { cur - 1 };
                    let rand = if data_indep {
                        if idx % BLOCK_WORDS == 0 {
                            addr = next_addr(&mut input);
                        }
                        addr[idx % BLOCK_WORDS]
                    } else {
                        mem[prev][0]
                    };
                    let j1 = rand & 0xffff_ffff;
                    let j2 = rand >> 32;
                    let ref_lane = if pass == 0 && slice == 0 {
                        l
                    } else {
                        (j2 % p as u64) as usize
                    };
                    let same = ref_lane == l;
                    let area: usize = if pass == 0 {
                        if same {
                            slice * seg + idx - 1
                        } else {
                            slice * seg - (idx == 0) as usize
                        }
                    } else if same {
                        q - seg + idx - 1
                    } else {
                        q - seg - (idx == 0) as usize
                    };
                    let x = (j1 * j1) >> 32;
                    let y = (area as u64 * x) >> 32;
                    let zz = area as u64 - 1 - y;
                    let start_pos = if pass == 0 || slice == 3 {
                        0
                    } else {
                        (slice + 1) * seg
                    };
                    let ref_idx = (start_pos + zz as usize) % q;
                    let nb = compress(&mem[prev], &mem[ref_lane * q + ref_idx]);
                    if pass == 0 {
                        mem[cur] = nb;
                    } else {
                        let old = &mut mem[cur];
                        for (o, n) in old.iter_mut().zip(nb.iter()) {
                            *o ^= n;
                        }
                    }
                }
            }
        }
    }
    let mut c = mem[q - 1];
    for l in 1..p {
        for (a, b) in c.iter_mut().zip(mem[l * q + q - 1].iter()) {
            *a ^= b;
        }
    }
    let mut cb = Vec::with_capacity(1024);
    for w in c {
        cb.extend_from_slice(&w.to_le_bytes());
    }
    h_prime(tag_len, &cb)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Unpadded standard base64 (PHC strings).
pub fn b64_encode(d: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in d.chunks(3) {
        let v = (ch[0] as u32) << 16
            | (*ch.get(1).unwrap_or(&0) as u32) << 8
            | *ch.get(2).unwrap_or(&0) as u32;
        let n = ch.len() + 1;
        for k in 0..n {
            out.push(B64[((v >> (18 - 6 * k)) & 63) as usize]);
        }
    }
    out
}

pub fn b64_decode(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut n = 0;
    for &c in s {
        let v = B64.iter().position(|&x| x == c)? as u32;
        acc = (acc << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
        }
    }
    Some(out)
}

/// The parameters of an encoded hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub variant: Variant,
    pub m: u32,
    pub t: u32,
    pub p: u32,
    pub salt: Vec<u8>,
    pub hash: Vec<u8>,
}

/// Parses `$argon2{i,d,id}$v=19$m=..,t=..,p=..$salt$hash`.
pub fn decode(s: &[u8]) -> Option<Encoded> {
    let s = std::str::from_utf8(s).ok()?;
    let mut parts = s.split('$');
    if !parts.next()?.is_empty() {
        return None;
    }
    let variant = match parts.next()? {
        "argon2i" => Variant::I,
        "argon2d" => Variant::D,
        "argon2id" => Variant::Id,
        _ => return None,
    };
    let mut next = parts.next()?;
    if let Some(v) = next.strip_prefix("v=") {
        if v != "19" {
            return None;
        }
        next = parts.next()?;
    }
    let (mut m, mut t, mut p) = (None, None, None);
    for kv in next.split(',') {
        let (k, v) = kv.split_once('=')?;
        let v: u32 = v.parse().ok()?;
        match k {
            "m" => m = Some(v),
            "t" => t = Some(v),
            "p" => p = Some(v),
            _ => return None,
        }
    }
    let salt = b64_decode(parts.next()?.as_bytes())?;
    let hash = b64_decode(parts.next()?.as_bytes())?;
    if parts.next().is_some() {
        return None;
    }
    Some(Encoded {
        variant,
        m: m?,
        t: t?,
        p: p?,
        salt,
        hash,
    })
}

/// Encodes a hash as PHP prints it.
pub fn encode(variant: Variant, m: u32, t: u32, p: u32, salt: &[u8], hash: &[u8]) -> Vec<u8> {
    let name = match variant {
        Variant::I => "argon2i",
        Variant::D => "argon2d",
        Variant::Id => "argon2id",
    };
    let mut out = format!("${}$v=19$m={},t={},p={}$", name, m, t, p).into_bytes();
    out.extend(b64_encode(salt));
    out.push(b'$');
    out.extend(b64_encode(hash));
    out
}

/// Verifies a password against an encoded hash.
pub fn verify(pwd: &[u8], encoded: &[u8]) -> bool {
    let Some(e) = decode(encoded) else {
        return false;
    };
    if e.hash.is_empty() || e.p == 0 || e.t == 0 {
        return false;
    }
    let h = hash(e.variant, pwd, &e.salt, e.t, e.m, e.p, e.hash.len());
    // Constant-time comparison.
    h.len() == e.hash.len() && h.iter().zip(&e.hash).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_vectors() {
        // test ext/standard/tests/password/password_verify_argon2.phpt
        assert!(verify(
            b"test",
            b"$argon2i$v=19$m=65536,t=3,p=1$OEVjWWs2Z3YvWlNZQ0ZmNw$JKin7ahjmh8JYvMyFcXri0Ss/Uvd3uYpD7MG6C/5Cy0"
        ));
        assert!(verify(
            b"test",
            b"$argon2id$v=19$m=1024,t=2,p=2$WS90MHJhd3AwSC5xTDJpZg$8tn2DaIJR2/UX4Cjcy2t3EZaLDL/qh+NbLQAOvTmdAg"
        ));
        assert!(!verify(
            b"argon2id",
            b"$argon2id$v=19$m=1024,t=2,p=2$WS90MHJhd3AwSC5xTDJpZg$8tn2DaIJR2/UX4Cjcy2t3EZaLDL/qh+NbLQAOvTmdAg"
        ));
    }
}
