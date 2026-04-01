use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

mod aes;
mod modes;

use modes::CipherMethod;

// Option flags
const OPENSSL_RAW_DATA: i64 = 1;
const OPENSSL_ZERO_PADDING: i64 = 2;

/// Register all openssl extension functions and constants
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"openssl");
    // Functions
    vm.register_function(b"openssl_encrypt", openssl_encrypt);
    vm.register_function(b"openssl_decrypt", openssl_decrypt);
    vm.register_function(b"openssl_random_pseudo_bytes", openssl_random_pseudo_bytes);
    vm.register_function(b"openssl_digest", openssl_digest);
    vm.register_function(b"openssl_get_cipher_methods", openssl_get_cipher_methods);
    vm.register_function(b"openssl_cipher_iv_length", openssl_cipher_iv_length);
    vm.register_function(b"openssl_cipher_key_length", openssl_cipher_key_length);
    vm.register_function(b"openssl_get_md_methods", openssl_get_md_methods);
    vm.register_function(b"openssl_error_string", openssl_error_string);

    // Constants
    vm.constants.insert(b"OPENSSL_RAW_DATA".to_vec(), Value::Long(1));
    vm.constants.insert(b"OPENSSL_ZERO_PADDING".to_vec(), Value::Long(2));
    vm.constants.insert(b"OPENSSL_DONT_ZERO_PAD_KEY".to_vec(), Value::Long(4));
    vm.constants.insert(b"OPENSSL_NO_PADDING".to_vec(), Value::Long(3));
    vm.constants.insert(b"OPENSSL_PKCS1_PADDING".to_vec(), Value::Long(1));
    vm.constants.insert(
        b"OPENSSL_VERSION_NUMBER".to_vec(),
        Value::Long(0x30000000),
    );
    vm.constants.insert(
        b"OPENSSL_VERSION_TEXT".to_vec(),
        Value::String(PhpString::from_bytes(b"OpenSSL 3.0.0 (goro-rs native)")),
    );
    vm.constants.insert(b"OPENSSL_ALGO_SHA1".to_vec(), Value::Long(1));
    vm.constants.insert(b"OPENSSL_ALGO_SHA256".to_vec(), Value::Long(7));
    vm.constants.insert(b"OPENSSL_ALGO_SHA384".to_vec(), Value::Long(8));
    vm.constants.insert(b"OPENSSL_ALGO_SHA512".to_vec(), Value::Long(9));
    vm.constants.insert(b"OPENSSL_ALGO_MD5".to_vec(), Value::Long(2));
}

/// Supported cipher method names
const CIPHER_METHODS: &[&str] = &[
    "aes-128-cbc",
    "aes-128-ecb",
    "aes-128-ctr",
    "aes-192-cbc",
    "aes-192-ecb",
    "aes-192-ctr",
    "aes-256-cbc",
    "aes-256-ecb",
    "aes-256-ctr",
];

/// Supported digest method names
const DIGEST_METHODS: &[&str] = &["md5", "sha1", "sha256", "sha384", "sha512"];

// ---- Base64 helpers ----

const B64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(B64_CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(B64_CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(B64_CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(B64_CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

fn base64_decode_char(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn base64_decode(data: &str) -> Option<Vec<u8>> {
    // Strip whitespace
    let clean: Vec<u8> = data.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.is_empty() {
        return Some(Vec::new());
    }
    if clean.len() % 4 != 0 {
        return None;
    }

    let mut result = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let a = base64_decode_char(chunk[0])?;
        let b = base64_decode_char(chunk[1])?;
        let c_val = if chunk[2] == b'=' {
            None
        } else {
            Some(base64_decode_char(chunk[2])?)
        };
        let d_val = if chunk[3] == b'=' {
            None
        } else {
            Some(base64_decode_char(chunk[3])?)
        };

        let triple = ((a as u32) << 18)
            | ((b as u32) << 12)
            | ((c_val.unwrap_or(0) as u32) << 6)
            | (d_val.unwrap_or(0) as u32);

        result.push((triple >> 16) as u8);
        if c_val.is_some() {
            result.push((triple >> 8) as u8);
        }
        if d_val.is_some() {
            result.push(triple as u8);
        }
    }
    Some(result)
}

// ---- Hash/digest implementations ----

fn md5_hash(data: &[u8]) -> [u8; 16] {
    // MD5 implementation (RFC 1321)
    let s: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
        5,  9, 14, 20, 5,  9, 14, 20, 5,  9, 14, 20, 5,  9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee,
        0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
        0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa,
        0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
        0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05,
        0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039,
        0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
        0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    ];

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    // Pre-processing: padding
    let orig_len_bits = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&orig_len_bits.to_le_bytes());

    // Process each 512-bit (64-byte) block
    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            *word = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }

        let mut a = a0;
        let mut b = b0;
        let mut c = c0;
        let mut d = d0;

        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };

            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(s[i]));
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut result = [0u8; 16];
    result[0..4].copy_from_slice(&a0.to_le_bytes());
    result[4..8].copy_from_slice(&b0.to_le_bytes());
    result[8..12].copy_from_slice(&c0.to_le_bytes());
    result[12..16].copy_from_slice(&d0.to_le_bytes());
    result
}

fn sha1_hash(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let orig_len_bits = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&orig_len_bits.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };

            let temp = a.rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut result = [0u8; 20];
    result[0..4].copy_from_slice(&h0.to_be_bytes());
    result[4..8].copy_from_slice(&h1.to_be_bytes());
    result[8..12].copy_from_slice(&h2.to_be_bytes());
    result[12..16].copy_from_slice(&h3.to_be_bytes());
    result[16..20].copy_from_slice(&h4.to_be_bytes());
    result
}

fn sha256_hash(data: &[u8]) -> [u8; 32] {
    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    let orig_len_bits = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&orig_len_bits.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(k[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = [0u8; 32];
    for i in 0..8 {
        result[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    result
}

fn sha512_hash(data: &[u8]) -> Vec<u8> {
    sha512_core(data, &SHA512_IV, 64)
}

fn sha384_hash(data: &[u8]) -> Vec<u8> {
    sha512_core(data, &SHA384_IV, 48)
}

const SHA512_IV: [u64; 8] = [
    0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1,
    0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179,
];

const SHA384_IV: [u64; 8] = [
    0xcbbb9d5dc1059ed8, 0x629a292a367cd507, 0x9159015a3070dd17, 0x152fecd8f70e5939,
    0x67332667ffc00b31, 0x8eb44a8768581511, 0xdb0c2e0d64f98fa7, 0x47b5481dbefa4fa4,
];

const SHA512_K: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc,
    0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
    0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
    0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
    0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
    0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
    0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df,
    0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30,
    0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
    0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
    0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178,
    0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
    0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

fn sha512_core(data: &[u8], iv: &[u64; 8], output_len: usize) -> Vec<u8> {
    let mut h = *iv;

    let orig_len_bits = (data.len() as u128) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 128 != 112 {
        msg.push(0);
    }
    msg.extend_from_slice(&orig_len_bits.to_be_bytes());

    for chunk in msg.chunks(128) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            w[i] = u64::from_be_bytes([
                chunk[i * 8],
                chunk[i * 8 + 1],
                chunk[i * 8 + 2],
                chunk[i * 8 + 3],
                chunk[i * 8 + 4],
                chunk[i * 8 + 5],
                chunk[i * 8 + 6],
                chunk[i * 8 + 7],
            ]);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA512_K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = Vec::with_capacity(64);
    for val in &h {
        result.extend_from_slice(&val.to_be_bytes());
    }
    result.truncate(output_len);
    result
}

fn compute_digest(algo: &str, data: &[u8]) -> Option<Vec<u8>> {
    match algo {
        "md5" => Some(md5_hash(data).to_vec()),
        "sha1" => Some(sha1_hash(data).to_vec()),
        "sha256" => Some(sha256_hash(data).to_vec()),
        "sha384" => Some(sha384_hash(data)),
        "sha512" => Some(sha512_hash(data)),
        _ => None,
    }
}

// ---- PHP function implementations ----

/// openssl_encrypt(data, cipher_method, key, options=0, iv="", &tag, aad, tag_length)
fn openssl_encrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let method_str = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let key = args.get(2).unwrap_or(&Value::Null).to_php_string();
    let options = args.get(3).map(|v| v.to_long()).unwrap_or(0);
    let iv = args.get(4).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b""));

    let method = match CipherMethod::parse(&method_str) {
        Some(m) => m,
        None => {
            vm.emit_warning(&format!(
                "openssl_encrypt(): Unknown cipher algorithm \"{}\"",
                method_str
            ));
            return Ok(Value::False);
        }
    };

    // Warn if IV is wrong length (but still proceed)
    let iv_len = method.iv_length();
    if iv_len > 0 && iv.as_bytes().len() != iv_len {
        if iv.as_bytes().is_empty() {
            vm.emit_warning(&format!(
                "openssl_encrypt(): Using an empty Initialization Vector (iv) is potentially insecure and not recommended"
            ));
        } else if iv.as_bytes().len() < iv_len {
            vm.emit_warning(&format!(
                "openssl_encrypt(): IV passed is only {} bytes long, cipher expects an IV of precisely {} bytes, padding with \\0",
                iv.as_bytes().len(),
                iv_len
            ));
        } else if iv.as_bytes().len() > iv_len {
            vm.emit_warning(&format!(
                "openssl_encrypt(): IV passed is {} bytes long which is longer than the {} expected by selected cipher, truncating",
                iv.as_bytes().len(),
                iv_len
            ));
        }
    }

    let raw_data = options & OPENSSL_RAW_DATA != 0;
    let zero_padding = options & OPENSSL_ZERO_PADDING != 0;

    match modes::encrypt(&method, data.as_bytes(), key.as_bytes(), iv.as_bytes(), zero_padding) {
        Ok(encrypted) => {
            if raw_data {
                Ok(Value::String(PhpString::from_vec(encrypted)))
            } else {
                Ok(Value::String(PhpString::from_string(base64_encode(&encrypted))))
            }
        }
        Err(e) => {
            vm.emit_warning(&format!("openssl_encrypt(): {}", e));
            Ok(Value::False)
        }
    }
}

/// openssl_decrypt(data, cipher_method, key, options=0, iv="", tag, aad)
fn openssl_decrypt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data_val = args.first().unwrap_or(&Value::Null).to_php_string();
    let method_str = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let key = args.get(2).unwrap_or(&Value::Null).to_php_string();
    let options = args.get(3).map(|v| v.to_long()).unwrap_or(0);
    let iv = args.get(4).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b""));

    let method = match CipherMethod::parse(&method_str) {
        Some(m) => m,
        None => {
            vm.emit_warning(&format!(
                "openssl_decrypt(): Unknown cipher algorithm \"{}\"",
                method_str
            ));
            return Ok(Value::False);
        }
    };

    let raw_data = options & OPENSSL_RAW_DATA != 0;
    let zero_padding = options & OPENSSL_ZERO_PADDING != 0;

    // If not raw data, base64-decode the input first
    let cipher_bytes = if raw_data {
        data_val.as_bytes().to_vec()
    } else {
        match base64_decode(&data_val.to_string_lossy()) {
            Some(decoded) => decoded,
            None => {
                vm.emit_warning("openssl_decrypt(): Failed to base64 decode the input");
                return Ok(Value::False);
            }
        }
    };

    if cipher_bytes.is_empty() {
        // PHP returns empty string for empty input in some modes
        return Ok(Value::String(PhpString::from_bytes(b"")));
    }

    // Warn if IV is wrong length
    let iv_len = method.iv_length();
    if iv_len > 0 && iv.as_bytes().len() != iv_len {
        if iv.as_bytes().is_empty() {
            vm.emit_warning(&format!(
                "openssl_decrypt(): Using an empty Initialization Vector (iv) is potentially insecure and not recommended"
            ));
        } else if iv.as_bytes().len() < iv_len {
            vm.emit_warning(&format!(
                "openssl_decrypt(): IV passed is only {} bytes long, cipher expects an IV of precisely {} bytes, padding with \\0",
                iv.as_bytes().len(),
                iv_len
            ));
        } else if iv.as_bytes().len() > iv_len {
            vm.emit_warning(&format!(
                "openssl_decrypt(): IV passed is {} bytes long which is longer than the {} expected by selected cipher, truncating",
                iv.as_bytes().len(),
                iv_len
            ));
        }
    }

    match modes::decrypt(&method, &cipher_bytes, key.as_bytes(), iv.as_bytes(), zero_padding) {
        Ok(decrypted) => Ok(Value::String(PhpString::from_vec(decrypted))),
        Err(e) => {
            vm.emit_warning(&format!("openssl_decrypt(): {}", e));
            Ok(Value::False)
        }
    }
}

/// openssl_random_pseudo_bytes(length, &strong_result)
fn openssl_random_pseudo_bytes(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let length = args.first().map(|v| v.to_long()).unwrap_or(0);
    if length < 0 {
        vm.emit_warning("openssl_random_pseudo_bytes(): Length must be greater than or equal to 0");
        return Ok(Value::False);
    }
    if length == 0 {
        return Ok(Value::String(PhpString::from_bytes(b"")));
    }

    let len = length as usize;
    let mut buf = vec![0u8; len];

    // Read from /dev/urandom
    let success = read_urandom(&mut buf);

    // Note: PHP's openssl_random_pseudo_bytes sets the second arg (&$strong_result)
    // by reference. Since goro-rs doesn't support by-ref args in builtins yet,
    // we just return the bytes and ignore the strong_result parameter.

    if success {
        Ok(Value::String(PhpString::from_vec(buf)))
    } else {
        Ok(Value::False)
    }
}

/// Read random bytes from /dev/urandom
fn read_urandom(buf: &mut [u8]) -> bool {
    use std::fs::File;
    use std::io::Read;
    match File::open("/dev/urandom") {
        Ok(mut f) => f.read_exact(buf).is_ok(),
        Err(_) => {
            // Fallback: use a simple PRNG seeded from time
            // This is NOT cryptographically secure
            use std::time::SystemTime;
            let seed = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let mut state = seed;
            for byte in buf.iter_mut() {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                *byte = (state >> 33) as u8;
            }
            true
        }
    }
}

/// openssl_digest(data, digest_algo, binary=false)
fn openssl_digest(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let algo = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let binary = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);

    let algo_lower = algo.to_ascii_lowercase();
    match compute_digest(&algo_lower, data.as_bytes()) {
        Some(digest) => {
            if binary {
                Ok(Value::String(PhpString::from_vec(digest)))
            } else {
                let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
                Ok(Value::String(PhpString::from_string(hex)))
            }
        }
        None => {
            vm.emit_warning(&format!(
                "openssl_digest(): Unknown digest algorithm \"{}\"",
                algo
            ));
            Ok(Value::False)
        }
    }
}

/// openssl_get_cipher_methods(aliases=false)
fn openssl_get_cipher_methods(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    for &method in CIPHER_METHODS {
        arr.push(Value::String(PhpString::from_string(method.to_string())));
    }
    // Also add uppercase variants as "aliases"
    let aliases = _args.first().map(|v| v.is_truthy()).unwrap_or(false);
    if aliases {
        for &method in CIPHER_METHODS {
            arr.push(Value::String(PhpString::from_string(
                method.to_ascii_uppercase(),
            )));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// openssl_cipher_iv_length(cipher_method)
fn openssl_cipher_iv_length(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let method_str = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    match CipherMethod::parse(&method_str) {
        Some(method) => Ok(Value::Long(method.iv_length() as i64)),
        None => {
            vm.emit_warning(&format!(
                "openssl_cipher_iv_length(): Unknown cipher algorithm \"{}\"",
                method_str
            ));
            Ok(Value::False)
        }
    }
}

/// openssl_cipher_key_length(cipher_method)
fn openssl_cipher_key_length(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let method_str = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    match CipherMethod::parse(&method_str) {
        Some(method) => Ok(Value::Long(method.key_length() as i64)),
        None => {
            vm.emit_warning(&format!(
                "openssl_cipher_key_length(): Unknown cipher algorithm \"{}\"",
                method_str
            ));
            Ok(Value::False)
        }
    }
}

/// openssl_get_md_methods(aliases=false)
fn openssl_get_md_methods(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();
    for &method in DIGEST_METHODS {
        arr.push(Value::String(PhpString::from_string(method.to_string())));
    }
    let aliases = _args.first().map(|v| v.is_truthy()).unwrap_or(false);
    if aliases {
        // Add common aliases
        let alias_list = ["md5", "sha1", "sha256", "sha384", "sha512"];
        for alias in &alias_list {
            arr.push(Value::String(PhpString::from_string(
                alias.to_ascii_uppercase(),
            )));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

/// openssl_error_string()
fn openssl_error_string(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // We don't maintain an error queue; return false (no more errors)
    Ok(Value::False)
}
