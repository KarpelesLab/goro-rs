//! Additional hash algorithms using RustCrypto crates and hand-rolled implementations.

use digest::Digest;

// ===== MD2 =====
pub fn md2_hash(data: &[u8]) -> Vec<u8> {
    let mut h = md2::Md2::new();
    h.update(data);
    h.finalize().to_vec()
}

// ===== Tiger 3-pass (192-bit) =====
// PHP's tiger128,3 / tiger160,3 / tiger192,3 all use the same 3-pass Tiger
// just truncated to different output lengths.
pub fn tiger_3pass(data: &[u8]) -> Vec<u8> {
    let mut h = tiger::Tiger::new();
    h.update(data);
    h.finalize().to_vec()
}

// ===== Whirlpool =====
pub fn whirlpool_hash(data: &[u8]) -> Vec<u8> {
    let mut h = whirlpool::Whirlpool::new();
    h.update(data);
    h.finalize().to_vec()
}

// ===== GOST R 34.11-94 =====
pub fn gost_hash(data: &[u8]) -> Vec<u8> {
    let mut h = gost94::Gost94Test::new();
    h.update(data);
    h.finalize().to_vec()
}

pub fn gost_crypto_hash(data: &[u8]) -> Vec<u8> {
    let mut h = gost94::Gost94CryptoPro::new();
    h.update(data);
    h.finalize().to_vec()
}

// ===== xxHash (XXH32 / XXH64 / XXH3 / XXH128) =====
pub fn xxh32_hash(data: &[u8], seed: u32) -> u32 {
    use std::hash::Hasher;
    let mut h = twox_hash::XxHash32::with_seed(seed);
    h.write(data);
    h.finish() as u32
}

pub fn xxh64_hash(data: &[u8], seed: u64) -> u64 {
    use std::hash::Hasher;
    let mut h = twox_hash::XxHash64::with_seed(seed);
    h.write(data);
    h.finish()
}

pub fn xxh3_64_hash(data: &[u8], seed: u64) -> u64 {
    use std::hash::Hasher;
    let mut h = twox_hash::Xxh3Hash64::with_seed(seed);
    h.write(data);
    h.finish()
}

pub fn xxh3_128_hash(data: &[u8], seed: u64) -> u128 {
    twox_hash::xxh3::hash128_with_seed(data, seed)
}

// ===== Murmur3 =====
// Murmur3 32-bit ("murmur3a")
pub fn murmur3a_hash(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    let n = data.len();
    let blocks = n / 4;
    for i in 0..blocks {
        let mut k = u32::from_le_bytes([data[i*4], data[i*4+1], data[i*4+2], data[i*4+3]]);
        k = k.wrapping_mul(0xcc9e2d51);
        k = k.rotate_left(15);
        k = k.wrapping_mul(0x1b873593);
        h ^= k;
        h = h.rotate_left(13);
        h = h.wrapping_mul(5).wrapping_add(0xe6546b64);
    }
    let tail = &data[blocks*4..];
    let mut k1: u32 = 0;
    if tail.len() >= 3 { k1 ^= (tail[2] as u32) << 16; }
    if tail.len() >= 2 { k1 ^= (tail[1] as u32) << 8; }
    if !tail.is_empty() {
        k1 ^= tail[0] as u32;
        k1 = k1.wrapping_mul(0xcc9e2d51);
        k1 = k1.rotate_left(15);
        k1 = k1.wrapping_mul(0x1b873593);
        h ^= k1;
    }
    h ^= n as u32;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^= h >> 16;
    h
}

// Murmur3 32-bit x86 128-bit ("murmur3c")
pub fn murmur3c_hash(data: &[u8], seed: u32) -> [u8; 16] {
    let mut h1 = seed;
    let mut h2 = seed;
    let mut h3 = seed;
    let mut h4 = seed;

    let c1: u32 = 0x239b961b;
    let c2: u32 = 0xab0e9789;
    let c3: u32 = 0x38b34ae5;
    let c4: u32 = 0xa1e38b93;

    let n = data.len();
    let blocks = n / 16;
    for i in 0..blocks {
        let mut k1 = u32::from_le_bytes([data[i*16], data[i*16+1], data[i*16+2], data[i*16+3]]);
        let mut k2 = u32::from_le_bytes([data[i*16+4], data[i*16+5], data[i*16+6], data[i*16+7]]);
        let mut k3 = u32::from_le_bytes([data[i*16+8], data[i*16+9], data[i*16+10], data[i*16+11]]);
        let mut k4 = u32::from_le_bytes([data[i*16+12], data[i*16+13], data[i*16+14], data[i*16+15]]);

        k1 = k1.wrapping_mul(c1); k1 = k1.rotate_left(15); k1 = k1.wrapping_mul(c2); h1 ^= k1;
        h1 = h1.rotate_left(19); h1 = h1.wrapping_add(h2); h1 = h1.wrapping_mul(5).wrapping_add(0x561ccd1b);

        k2 = k2.wrapping_mul(c2); k2 = k2.rotate_left(16); k2 = k2.wrapping_mul(c3); h2 ^= k2;
        h2 = h2.rotate_left(17); h2 = h2.wrapping_add(h3); h2 = h2.wrapping_mul(5).wrapping_add(0x0bcaa747);

        k3 = k3.wrapping_mul(c3); k3 = k3.rotate_left(17); k3 = k3.wrapping_mul(c4); h3 ^= k3;
        h3 = h3.rotate_left(15); h3 = h3.wrapping_add(h4); h3 = h3.wrapping_mul(5).wrapping_add(0x96cd1c35);

        k4 = k4.wrapping_mul(c4); k4 = k4.rotate_left(18); k4 = k4.wrapping_mul(c1); h4 ^= k4;
        h4 = h4.rotate_left(13); h4 = h4.wrapping_add(h1); h4 = h4.wrapping_mul(5).wrapping_add(0x32ac3b17);
    }

    let tail = &data[blocks*16..];
    let mut k1: u32 = 0;
    let mut k2: u32 = 0;
    let mut k3: u32 = 0;
    let mut k4: u32 = 0;
    let l = tail.len();
    if l >= 15 { k4 ^= (tail[14] as u32) << 16; }
    if l >= 14 { k4 ^= (tail[13] as u32) << 8; }
    if l >= 13 { k4 ^= tail[12] as u32;
        k4 = k4.wrapping_mul(c4); k4 = k4.rotate_left(18); k4 = k4.wrapping_mul(c1); h4 ^= k4;
    }
    if l >= 12 { k3 ^= (tail[11] as u32) << 24; }
    if l >= 11 { k3 ^= (tail[10] as u32) << 16; }
    if l >= 10 { k3 ^= (tail[9] as u32) << 8; }
    if l >= 9 { k3 ^= tail[8] as u32;
        k3 = k3.wrapping_mul(c3); k3 = k3.rotate_left(17); k3 = k3.wrapping_mul(c4); h3 ^= k3;
    }
    if l >= 8 { k2 ^= (tail[7] as u32) << 24; }
    if l >= 7 { k2 ^= (tail[6] as u32) << 16; }
    if l >= 6 { k2 ^= (tail[5] as u32) << 8; }
    if l >= 5 { k2 ^= tail[4] as u32;
        k2 = k2.wrapping_mul(c2); k2 = k2.rotate_left(16); k2 = k2.wrapping_mul(c3); h2 ^= k2;
    }
    if l >= 4 { k1 ^= (tail[3] as u32) << 24; }
    if l >= 3 { k1 ^= (tail[2] as u32) << 16; }
    if l >= 2 { k1 ^= (tail[1] as u32) << 8; }
    if l >= 1 { k1 ^= tail[0] as u32;
        k1 = k1.wrapping_mul(c1); k1 = k1.rotate_left(15); k1 = k1.wrapping_mul(c2); h1 ^= k1;
    }

    h1 ^= n as u32;
    h2 ^= n as u32;
    h3 ^= n as u32;
    h4 ^= n as u32;

    h1 = h1.wrapping_add(h2); h1 = h1.wrapping_add(h3); h1 = h1.wrapping_add(h4);
    h2 = h2.wrapping_add(h1);
    h3 = h3.wrapping_add(h1);
    h4 = h4.wrapping_add(h1);

    h1 = fmix32(h1);
    h2 = fmix32(h2);
    h3 = fmix32(h3);
    h4 = fmix32(h4);

    h1 = h1.wrapping_add(h2); h1 = h1.wrapping_add(h3); h1 = h1.wrapping_add(h4);
    h2 = h2.wrapping_add(h1);
    h3 = h3.wrapping_add(h1);
    h4 = h4.wrapping_add(h1);

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&h1.to_be_bytes());
    out[4..8].copy_from_slice(&h2.to_be_bytes());
    out[8..12].copy_from_slice(&h3.to_be_bytes());
    out[12..16].copy_from_slice(&h4.to_be_bytes());
    out
}

// Murmur3 x64 128-bit ("murmur3f")
pub fn murmur3f_hash(data: &[u8], seed: u64) -> [u8; 16] {
    let c1: u64 = 0x87c37b91114253d5;
    let c2: u64 = 0x4cf5ad432745937f;
    let mut h1 = seed;
    let mut h2 = seed;

    let n = data.len();
    let blocks = n / 16;
    for i in 0..blocks {
        let mut k1 = u64::from_le_bytes([
            data[i*16], data[i*16+1], data[i*16+2], data[i*16+3],
            data[i*16+4], data[i*16+5], data[i*16+6], data[i*16+7],
        ]);
        let mut k2 = u64::from_le_bytes([
            data[i*16+8], data[i*16+9], data[i*16+10], data[i*16+11],
            data[i*16+12], data[i*16+13], data[i*16+14], data[i*16+15],
        ]);

        k1 = k1.wrapping_mul(c1); k1 = k1.rotate_left(31); k1 = k1.wrapping_mul(c2); h1 ^= k1;
        h1 = h1.rotate_left(27); h1 = h1.wrapping_add(h2); h1 = h1.wrapping_mul(5).wrapping_add(0x52dce729);

        k2 = k2.wrapping_mul(c2); k2 = k2.rotate_left(33); k2 = k2.wrapping_mul(c1); h2 ^= k2;
        h2 = h2.rotate_left(31); h2 = h2.wrapping_add(h1); h2 = h2.wrapping_mul(5).wrapping_add(0x38495ab5);
    }

    let tail = &data[blocks*16..];
    let mut k1: u64 = 0;
    let mut k2: u64 = 0;
    let l = tail.len();
    if l >= 15 { k2 ^= (tail[14] as u64) << 48; }
    if l >= 14 { k2 ^= (tail[13] as u64) << 40; }
    if l >= 13 { k2 ^= (tail[12] as u64) << 32; }
    if l >= 12 { k2 ^= (tail[11] as u64) << 24; }
    if l >= 11 { k2 ^= (tail[10] as u64) << 16; }
    if l >= 10 { k2 ^= (tail[9] as u64) << 8; }
    if l >= 9 { k2 ^= tail[8] as u64;
        k2 = k2.wrapping_mul(c2); k2 = k2.rotate_left(33); k2 = k2.wrapping_mul(c1); h2 ^= k2;
    }
    if l >= 8 { k1 ^= (tail[7] as u64) << 56; }
    if l >= 7 { k1 ^= (tail[6] as u64) << 48; }
    if l >= 6 { k1 ^= (tail[5] as u64) << 40; }
    if l >= 5 { k1 ^= (tail[4] as u64) << 32; }
    if l >= 4 { k1 ^= (tail[3] as u64) << 24; }
    if l >= 3 { k1 ^= (tail[2] as u64) << 16; }
    if l >= 2 { k1 ^= (tail[1] as u64) << 8; }
    if l >= 1 { k1 ^= tail[0] as u64;
        k1 = k1.wrapping_mul(c1); k1 = k1.rotate_left(31); k1 = k1.wrapping_mul(c2); h1 ^= k1;
    }

    h1 ^= n as u64;
    h2 ^= n as u64;
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);

    h1 = fmix64(h1);
    h2 = fmix64(h2);

    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);

    let mut out = [0u8; 16];
    out[0..8].copy_from_slice(&h1.to_be_bytes());
    out[8..16].copy_from_slice(&h2.to_be_bytes());
    out
}

fn fmix32(mut k: u32) -> u32 {
    k ^= k >> 16;
    k = k.wrapping_mul(0x85ebca6b);
    k ^= k >> 13;
    k = k.wrapping_mul(0xc2b2ae35);
    k ^= k >> 16;
    k
}

fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51afd7ed558ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ceb9fe1a85ec53);
    k ^= k >> 33;
    k
}
