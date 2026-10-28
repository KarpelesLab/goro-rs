//! Snefru-256 with 8 passes (Merkle, "A Fast Software One-Way Hash
//! Function", J. Cryptology 3(1), 1990; reference snefru.c 2.5a).
//!
//! The 512-bit block is the 256-bit chaining value followed by a 32-byte
//! input chunk, read as big-endian words; the message is followed by a
//! zero-padded partial chunk and a final chunk holding the bit count.
//! Serialized layout observed on PHP 8.5: sixteen state words (the
//! chaining value and eight words PHP leaves at zero), the 64-bit bit count
//! (high word first), a byte-wide buffer fill count (below 32) and the
//! 32-byte buffer.

use crate::engine::{Engine, Field, Kind, Val, be32, clone_box, f};
use crate::tables::snefru::SNEFRU_SBOX;

pub(crate) const SNEFRU_LAYOUT: &[Field] = &[
    f(Kind::U32, 16, 0),
    f(Kind::U32, 2, 64),
    f(Kind::U8, 1, 72),
    f(Kind::Bytes(32), 1, 73),
];

const SHIFTS: [u32; 4] = [16, 8, 16, 24];

fn hash512(state: &mut [u32; 16], chunk: &[u8; 32]) {
    let mut input = [0u32; 16];
    input[..8].copy_from_slice(&state[..8]);
    for i in 0..8 {
        input[8 + i] = be32(&chunk[i * 4..]);
    }
    let mut b = input;
    for index in 0..8 {
        let sb0 = &SNEFRU_SBOX[2 * index];
        let sb1 = &SNEFRU_SBOX[2 * index + 1];
        for &shift in &SHIFTS {
            for i in 0..16 {
                let sb = if (i / 2) & 1 == 0 { sb0 } else { sb1 };
                let e = sb[(b[i] & 0xff) as usize];
                b[(i + 1) & 15] ^= e;
                b[(i + 15) & 15] ^= e;
            }
            for w in b.iter_mut() {
                *w = w.rotate_right(shift);
            }
        }
    }
    for i in 0..8 {
        state[i] = input[i] ^ b[15 - i];
    }
}

#[derive(Clone)]
pub(crate) struct Snefru {
    state: [u32; 16],
    /// Bit count, high word first.
    count: [u32; 2],
    len: usize,
    buf: [u8; 32],
}

impl Snefru {
    pub(crate) fn new() -> Self {
        Snefru {
            state: [0; 16],
            count: [0; 2],
            len: 0,
            buf: [0; 32],
        }
    }

    fn add_bits(&mut self, bytes: usize) {
        let c = (((self.count[0] as u64) << 32) | self.count[1] as u64)
            .wrapping_add((bytes as u64).wrapping_mul(8));
        self.count = [(c >> 32) as u32, c as u32];
    }
}

impl Engine for Snefru {
    fn update(&mut self, mut data: &[u8]) {
        self.add_bits(data.len());
        if self.len != 0 {
            let take = 32 - self.len;
            if data.len() < take {
                self.buf[self.len..self.len + data.len()].copy_from_slice(data);
                self.len += data.len();
                return;
            }
            self.buf[self.len..].copy_from_slice(&data[..take]);
            let b = self.buf;
            hash512(&mut self.state, &b);
            self.buf = [0; 32];
            data = &data[take..];
            self.len = 0;
        }
        let (chunks, chunks_rest) = data.as_chunks::<32>();
        for c in chunks {
            hash512(&mut self.state, c);
        }
        let rest = chunks_rest;
        self.buf[..rest.len()].copy_from_slice(rest);
        self.len = rest.len();
    }

    fn finish(&mut self, out: &mut [u8]) {
        if self.len != 0 {
            for b in &mut self.buf[self.len..] {
                *b = 0;
            }
            let b = self.buf;
            hash512(&mut self.state, &b);
        }
        let mut last = [0u8; 32];
        last[24..28].copy_from_slice(&self.count[0].to_be_bytes());
        last[28..].copy_from_slice(&self.count[1].to_be_bytes());
        hash512(&mut self.state, &last);
        for i in 0..8 {
            out[i * 4..i * 4 + 4].copy_from_slice(&self.state[i].to_be_bytes());
        }
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        let mut v: Vec<Val> = self.state.iter().map(|&w| Val::U32(w)).collect();
        v.push(Val::U32(self.count[0]));
        v.push(Val::U32(self.count[1]));
        v.push(Val::U8(self.len as u8));
        v.push(Val::Bytes(self.buf.to_vec()));
        v
    }

    fn import(&mut self, v: &[Val]) -> bool {
        let len = v[18].u32() as usize;
        if len >= 32 {
            return false;
        }
        for i in 0..16 {
            self.state[i] = v[i].u32();
        }
        self.count = [v[16].u32(), v[17].u32()];
        self.len = len;
        self.buf.copy_from_slice(v[19].bytes());
        true
    }
}
