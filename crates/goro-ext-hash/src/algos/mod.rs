//! Ports of the php-src ext/hash algorithm implementations.

pub mod gost;
pub mod haval;
pub mod md;
pub mod murmur;
pub mod ripemd;
pub mod sha;
pub mod sha3;
pub mod simple;
pub mod snefru;
pub mod tiger;
pub mod whirlpool;
pub mod xxhash;

/// The `PADDING` array of the MD-style hashes.
pub static PADDING: [u8; 128] = {
    let mut p = [0u8; 128];
    p[0] = 0x80;
    p
};

/// Update of the MD4/MD5/SHA-1/SHA-256/RIPEMD/HAVAL family: `count` is the
/// message length in bits (lsw first), `buffer` the pending partial block.
pub fn md32_update<const B: usize>(
    count: &mut [u32; 2],
    buffer: &mut [u8; B],
    input: &[u8],
    mut transform: impl FnMut(&[u8]),
) {
    let mut index = ((count[0] >> 3) as usize) & (B - 1);
    let bits = (input.len() as u32) << 3;
    count[0] = count[0].wrapping_add(bits);
    if count[0] < bits {
        count[1] = count[1].wrapping_add(1);
    }
    count[1] = count[1].wrapping_add((input.len() >> 29) as u32);
    let part_len = B - index;
    let mut i;
    if input.len() >= part_len {
        buffer[index..].copy_from_slice(&input[..part_len]);
        transform(&buffer[..]);
        i = part_len;
        while i + B - 1 < input.len() {
            transform(&input[i..i + B]);
            i += B;
        }
        index = 0;
    } else {
        i = 0;
    }
    let rest = input.len() - i;
    buffer[index..index + rest].copy_from_slice(&input[i..]);
}

/// Same as `md32_update` with 64-bit counters (SHA-384/512).
pub fn md64_update<const B: usize>(
    count: &mut [u64; 2],
    buffer: &mut [u8; B],
    input: &[u8],
    mut transform: impl FnMut(&[u8]),
) {
    let mut index = ((count[0] >> 3) as usize) & (B - 1);
    let bits = (input.len() as u64) << 3;
    count[0] = count[0].wrapping_add(bits);
    if count[0] < bits {
        count[1] = count[1].wrapping_add(1);
    }
    count[1] = count[1].wrapping_add((input.len() as u64) >> 61);
    let part_len = B - index;
    let mut i;
    if input.len() >= part_len {
        buffer[index..].copy_from_slice(&input[..part_len]);
        transform(&buffer[..]);
        i = part_len;
        while i + B - 1 < input.len() {
            transform(&input[i..i + B]);
            i += B;
        }
        index = 0;
    } else {
        i = 0;
    }
    let rest = input.len() - i;
    buffer[index..index + rest].copy_from_slice(&input[i..]);
}

pub fn le_words<const N: usize>(block: &[u8]) -> [u32; N] {
    let mut x = [0u32; N];
    for (k, w) in x.iter_mut().enumerate() {
        *w = u32::from_le_bytes(block[4 * k..4 * k + 4].try_into().unwrap());
    }
    x
}

pub fn be_words<const N: usize>(block: &[u8]) -> [u32; N] {
    let mut x = [0u32; N];
    for (k, w) in x.iter_mut().enumerate() {
        *w = u32::from_be_bytes(block[4 * k..4 * k + 4].try_into().unwrap());
    }
    x
}

/// Store `words` little endian into `out` (as many bytes as `out` holds).
pub fn encode_le(out: &mut [u8], words: &[u32]) {
    for (k, b) in out.iter_mut().enumerate() {
        *b = (words[k / 4] >> (8 * (k % 4))) as u8;
    }
}

/// Store `words` big endian into `out` (as many bytes as `out` holds).
pub fn encode_be(out: &mut [u8], words: &[u32]) {
    for (k, b) in out.iter_mut().enumerate() {
        *b = (words[k / 4] >> (8 * (3 - k % 4))) as u8;
    }
}
