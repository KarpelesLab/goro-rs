/// AES block cipher implementation (FIPS 197)
/// Supports AES-128, AES-192, and AES-256.

const BLOCK_SIZE: usize = 16;

// Forward S-Box
const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

// Inverse S-Box
const INV_SBOX: [u8; 256] = [
    0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3, 0xd7, 0xfb,
    0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44, 0xc4, 0xde, 0xe9, 0xcb,
    0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c, 0x95, 0x0b, 0x42, 0xfa, 0xc3, 0x4e,
    0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2, 0x76, 0x5b, 0xa2, 0x49, 0x6d, 0x8b, 0xd1, 0x25,
    0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68, 0x98, 0x16, 0xd4, 0xa4, 0x5c, 0xcc, 0x5d, 0x65, 0xb6, 0x92,
    0x6c, 0x70, 0x48, 0x50, 0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15, 0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84,
    0x90, 0xd8, 0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7, 0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06,
    0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02, 0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13, 0x8a, 0x6b,
    0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc, 0xea, 0x97, 0xf2, 0xcf, 0xce, 0xf0, 0xb4, 0xe6, 0x73,
    0x96, 0xac, 0x74, 0x22, 0xe7, 0xad, 0x35, 0x85, 0xe2, 0xf9, 0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e,
    0x47, 0xf1, 0x1a, 0x71, 0x1d, 0x29, 0xc5, 0x89, 0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b,
    0xfc, 0x56, 0x3e, 0x4b, 0xc6, 0xd2, 0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4,
    0x1f, 0xdd, 0xa8, 0x33, 0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f,
    0x60, 0x51, 0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef,
    0xa0, 0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53, 0x99, 0x61,
    0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63, 0x55, 0x21, 0x0c, 0x7d,
];

// Round constants for key expansion
const RCON: [u8; 11] = [
    0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36,
];

/// Multiply by 2 in GF(2^8)
#[inline]
fn xtime(a: u8) -> u8 {
    if a & 0x80 != 0 {
        (a << 1) ^ 0x1b
    } else {
        a << 1
    }
}

/// Multiply two bytes in GF(2^8)
#[inline]
fn gmul(a: u8, b: u8) -> u8 {
    let mut result = 0u8;
    let mut aa = a;
    let mut bb = b;
    for _ in 0..8 {
        if bb & 1 != 0 {
            result ^= aa;
        }
        let hi = aa & 0x80;
        aa <<= 1;
        if hi != 0 {
            aa ^= 0x1b;
        }
        bb >>= 1;
    }
    result
}

/// AES key schedule parameters
#[derive(Clone, Copy)]
pub enum AesKeySize {
    Aes128, // 16 bytes, 10 rounds
    Aes192, // 24 bytes, 12 rounds
    Aes256, // 32 bytes, 14 rounds
}

impl AesKeySize {
    pub fn key_len(self) -> usize {
        match self {
            AesKeySize::Aes128 => 16,
            AesKeySize::Aes192 => 24,
            AesKeySize::Aes256 => 32,
        }
    }

    pub fn num_rounds(self) -> usize {
        match self {
            AesKeySize::Aes128 => 10,
            AesKeySize::Aes192 => 12,
            AesKeySize::Aes256 => 14,
        }
    }

    fn nk(self) -> usize {
        self.key_len() / 4
    }

    fn num_round_key_words(self) -> usize {
        4 * (self.num_rounds() + 1)
    }

    #[allow(dead_code)]
    pub fn from_key_len(len: usize) -> Option<AesKeySize> {
        match len {
            16 => Some(AesKeySize::Aes128),
            24 => Some(AesKeySize::Aes192),
            32 => Some(AesKeySize::Aes256),
            _ => None,
        }
    }
}

/// Expanded round keys
pub struct AesKey {
    round_keys: Vec<[u8; 4]>,
    key_size: AesKeySize,
}

impl AesKey {
    /// Perform AES key expansion
    pub fn new(key: &[u8], key_size: AesKeySize) -> Self {
        let nk = key_size.nk();
        let num_words = key_size.num_round_key_words();
        let mut w: Vec<[u8; 4]> = Vec::with_capacity(num_words);

        // Copy key into first nk words
        for i in 0..nk {
            let base = i * 4;
            w.push([
                key.get(base).copied().unwrap_or(0),
                key.get(base + 1).copied().unwrap_or(0),
                key.get(base + 2).copied().unwrap_or(0),
                key.get(base + 3).copied().unwrap_or(0),
            ]);
        }

        // Expand
        for i in nk..num_words {
            let mut temp = w[i - 1];
            if i % nk == 0 {
                // RotWord
                temp = [temp[1], temp[2], temp[3], temp[0]];
                // SubWord
                temp = [
                    SBOX[temp[0] as usize],
                    SBOX[temp[1] as usize],
                    SBOX[temp[2] as usize],
                    SBOX[temp[3] as usize],
                ];
                // XOR with Rcon
                temp[0] ^= RCON[i / nk];
            } else if nk > 6 && i % nk == 4 {
                // Extra SubWord for AES-256
                temp = [
                    SBOX[temp[0] as usize],
                    SBOX[temp[1] as usize],
                    SBOX[temp[2] as usize],
                    SBOX[temp[3] as usize],
                ];
            }
            let prev = w[i - nk];
            w.push([
                prev[0] ^ temp[0],
                prev[1] ^ temp[1],
                prev[2] ^ temp[2],
                prev[3] ^ temp[3],
            ]);
        }

        AesKey {
            round_keys: w,
            key_size,
        }
    }

    /// Get round key as a 16-byte block (4 words)
    fn round_key(&self, round: usize) -> [u8; BLOCK_SIZE] {
        let base = round * 4;
        let mut key = [0u8; BLOCK_SIZE];
        for i in 0..4 {
            key[i * 4] = self.round_keys[base + i][0];
            key[i * 4 + 1] = self.round_keys[base + i][1];
            key[i * 4 + 2] = self.round_keys[base + i][2];
            key[i * 4 + 3] = self.round_keys[base + i][3];
        }
        key
    }

    /// Encrypt a single 16-byte block
    pub fn encrypt_block(&self, input: &[u8; BLOCK_SIZE]) -> [u8; BLOCK_SIZE] {
        let nr = self.key_size.num_rounds();
        let mut state = *input;

        // Initial AddRoundKey
        add_round_key(&mut state, &self.round_key(0));

        // Main rounds
        for round in 1..nr {
            sub_bytes(&mut state);
            shift_rows(&mut state);
            mix_columns(&mut state);
            add_round_key(&mut state, &self.round_key(round));
        }

        // Final round (no MixColumns)
        sub_bytes(&mut state);
        shift_rows(&mut state);
        add_round_key(&mut state, &self.round_key(nr));

        state
    }

    /// Decrypt a single 16-byte block
    pub fn decrypt_block(&self, input: &[u8; BLOCK_SIZE]) -> [u8; BLOCK_SIZE] {
        let nr = self.key_size.num_rounds();
        let mut state = *input;

        // Initial AddRoundKey with last round key
        add_round_key(&mut state, &self.round_key(nr));

        // Main rounds (in reverse)
        for round in (1..nr).rev() {
            inv_shift_rows(&mut state);
            inv_sub_bytes(&mut state);
            add_round_key(&mut state, &self.round_key(round));
            inv_mix_columns(&mut state);
        }

        // Final round (no InvMixColumns)
        inv_shift_rows(&mut state);
        inv_sub_bytes(&mut state);
        add_round_key(&mut state, &self.round_key(0));

        state
    }
}

/// AddRoundKey: XOR state with round key
#[inline]
fn add_round_key(state: &mut [u8; BLOCK_SIZE], round_key: &[u8; BLOCK_SIZE]) {
    for i in 0..BLOCK_SIZE {
        state[i] ^= round_key[i];
    }
}

/// SubBytes: Apply S-Box substitution
#[inline]
fn sub_bytes(state: &mut [u8; BLOCK_SIZE]) {
    for byte in state.iter_mut() {
        *byte = SBOX[*byte as usize];
    }
}

/// InvSubBytes: Apply inverse S-Box substitution
#[inline]
fn inv_sub_bytes(state: &mut [u8; BLOCK_SIZE]) {
    for byte in state.iter_mut() {
        *byte = INV_SBOX[*byte as usize];
    }
}

/// ShiftRows: Cyclic left shift of rows
/// State is stored column-major: state[col*4 + row]
#[inline]
fn shift_rows(state: &mut [u8; BLOCK_SIZE]) {
    // Row 0: no shift
    // Row 1: shift left by 1
    let tmp = state[1];
    state[1] = state[5];
    state[5] = state[9];
    state[9] = state[13];
    state[13] = tmp;

    // Row 2: shift left by 2
    let tmp0 = state[2];
    let tmp1 = state[6];
    state[2] = state[10];
    state[6] = state[14];
    state[10] = tmp0;
    state[14] = tmp1;

    // Row 3: shift left by 3 (= shift right by 1)
    // Row 3 elements are at indices 3, 7, 11, 15
    let tmp = state[15];
    state[15] = state[11];
    state[11] = state[7];
    state[7] = state[3];
    state[3] = tmp;
}

/// InvShiftRows: Cyclic right shift of rows
#[inline]
fn inv_shift_rows(state: &mut [u8; BLOCK_SIZE]) {
    // Row 1: shift right by 1
    let tmp = state[13];
    state[13] = state[9];
    state[9] = state[5];
    state[5] = state[1];
    state[1] = tmp;

    // Row 2: shift right by 2
    let tmp0 = state[2];
    let tmp1 = state[6];
    state[2] = state[10];
    state[6] = state[14];
    state[10] = tmp0;
    state[14] = tmp1;

    // Row 3: shift right by 3 (= shift left by 1)
    let tmp = state[3];
    state[3] = state[7];
    state[7] = state[11];
    state[11] = state[15];
    state[15] = tmp;
}

/// MixColumns: Matrix multiplication in GF(2^8)
/// Each column [s0, s1, s2, s3] is multiplied by the matrix:
/// [2, 3, 1, 1]
/// [1, 2, 3, 1]
/// [1, 1, 2, 3]
/// [3, 1, 1, 2]
#[inline]
fn mix_columns(state: &mut [u8; BLOCK_SIZE]) {
    for col in 0..4 {
        let base = col * 4;
        let s0 = state[base];
        let s1 = state[base + 1];
        let s2 = state[base + 2];
        let s3 = state[base + 3];

        state[base] = xtime(s0) ^ xtime(s1) ^ s1 ^ s2 ^ s3;
        state[base + 1] = s0 ^ xtime(s1) ^ xtime(s2) ^ s2 ^ s3;
        state[base + 2] = s0 ^ s1 ^ xtime(s2) ^ xtime(s3) ^ s3;
        state[base + 3] = xtime(s0) ^ s0 ^ s1 ^ s2 ^ xtime(s3);
    }
}

/// InvMixColumns: Inverse matrix multiplication in GF(2^8)
/// Matrix:
/// [14, 11, 13,  9]
/// [ 9, 14, 11, 13]
/// [13,  9, 14, 11]
/// [11, 13,  9, 14]
#[inline]
fn inv_mix_columns(state: &mut [u8; BLOCK_SIZE]) {
    for col in 0..4 {
        let base = col * 4;
        let s0 = state[base];
        let s1 = state[base + 1];
        let s2 = state[base + 2];
        let s3 = state[base + 3];

        state[base] = gmul(s0, 14) ^ gmul(s1, 11) ^ gmul(s2, 13) ^ gmul(s3, 9);
        state[base + 1] = gmul(s0, 9) ^ gmul(s1, 14) ^ gmul(s2, 11) ^ gmul(s3, 13);
        state[base + 2] = gmul(s0, 13) ^ gmul(s1, 9) ^ gmul(s2, 14) ^ gmul(s3, 11);
        state[base + 3] = gmul(s0, 11) ^ gmul(s1, 13) ^ gmul(s2, 9) ^ gmul(s3, 14);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes128_encrypt_decrypt() {
        // NIST FIPS 197 Appendix B test vector
        let key = [
            0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6,
            0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f, 0x3c,
        ];
        let plaintext = [
            0x32, 0x43, 0xf6, 0xa8, 0x88, 0x5a, 0x30, 0x8d,
            0x31, 0x31, 0x98, 0xa2, 0xe0, 0x37, 0x07, 0x34,
        ];
        let expected = [
            0x39, 0x25, 0x84, 0x1d, 0x02, 0xdc, 0x09, 0xfb,
            0xdc, 0x11, 0x85, 0x97, 0x19, 0x6a, 0x0b, 0x32,
        ];

        let aes_key = AesKey::new(&key, AesKeySize::Aes128);
        let ciphertext = aes_key.encrypt_block(&plaintext);
        assert_eq!(ciphertext, expected);

        let decrypted = aes_key.decrypt_block(&ciphertext);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_aes256_encrypt_decrypt() {
        // NIST test vector
        let key = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
            0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
            0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
        ];
        let plaintext = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
            0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        ];
        let expected = [
            0x8e, 0xa2, 0xb7, 0xca, 0x51, 0x67, 0x45, 0xbf,
            0xea, 0xfc, 0x49, 0x90, 0x4b, 0x49, 0x60, 0x89,
        ];

        let aes_key = AesKey::new(&key, AesKeySize::Aes256);
        let ciphertext = aes_key.encrypt_block(&plaintext);
        assert_eq!(ciphertext, expected);

        let decrypted = aes_key.decrypt_block(&ciphertext);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_roundtrip_all_sizes() {
        let data = [0x42u8; 16];

        for (key_bytes, ks) in [
            (&[0xAA; 16][..], AesKeySize::Aes128),
            (&[0xBB; 24][..], AesKeySize::Aes192),
            (&[0xCC; 32][..], AesKeySize::Aes256),
        ] {
            let aes_key = AesKey::new(key_bytes, ks);
            let enc = aes_key.encrypt_block(&data);
            let dec = aes_key.decrypt_block(&enc);
            assert_eq!(dec, data);
        }
    }
}
