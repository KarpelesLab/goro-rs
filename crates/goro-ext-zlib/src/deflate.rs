/// DEFLATE compression (RFC 1951)
/// Implements LZ77 with hash chain matching and Huffman encoding.

/// Bitstream writer - writes bits LSB-first
struct BitWriter {
    output: Vec<u8>,
    current: u32,
    bits: u8,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter {
            output: Vec::new(),
            current: 0,
            bits: 0,
        }
    }

    fn write_bits(&mut self, value: u32, count: u8) {
        self.current |= value << self.bits;
        self.bits += count;
        while self.bits >= 8 {
            self.output.push(self.current as u8);
            self.current >>= 8;
            self.bits -= 8;
        }
    }

    fn flush(&mut self) {
        if self.bits > 0 {
            self.output.push(self.current as u8);
            self.current = 0;
            self.bits = 0;
        }
    }

    fn into_bytes(mut self) -> Vec<u8> {
        self.flush();
        self.output
    }
}

/// Fixed Huffman code for a literal/length symbol (RFC 1951 section 3.2.6)
fn fixed_lit_code(sym: u16) -> (u32, u8) {
    match sym {
        0..=143 => {
            // 8-bit codes: 00110000 through 10111111 (0x30..0xBF)
            let code = sym as u32 + 0x30;
            (reverse_bits_u32(code, 8), 8)
        }
        144..=255 => {
            // 9-bit codes: 110010000 through 111111111 (0x190..0x1FF)
            let code = (sym - 144) as u32 + 0x190;
            (reverse_bits_u32(code, 9), 9)
        }
        256..=279 => {
            // 7-bit codes: 0000000 through 0010111 (0x00..0x17)
            let code = (sym - 256) as u32;
            (reverse_bits_u32(code, 7), 7)
        }
        280..=287 => {
            // 8-bit codes: 11000000 through 11000111 (0xC0..0xC7)
            let code = (sym - 280) as u32 + 0xC0;
            (reverse_bits_u32(code, 8), 8)
        }
        _ => (0, 0),
    }
}

/// Fixed Huffman code for a distance symbol (all 5-bit codes)
fn fixed_dist_code(sym: u16) -> (u32, u8) {
    (reverse_bits_u32(sym as u32, 5), 5)
}

/// Reverse `count` bits of value
fn reverse_bits_u32(val: u32, count: u8) -> u32 {
    let mut result: u32 = 0;
    let mut v = val;
    for _ in 0..count {
        result = (result << 1) | (v & 1);
        v >>= 1;
    }
    result
}

/// Length base values (RFC 1951, section 3.2.5)
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13,
    15, 17, 19, 23, 27, 31, 35, 43, 51, 59,
    67, 83, 99, 115, 131, 163, 195, 227, 258,
];

/// Extra bits for length codes
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1,
    1, 1, 2, 2, 2, 2, 3, 3, 3, 3,
    4, 4, 4, 4, 5, 5, 5, 5, 0,
];

/// Distance base values
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25,
    33, 49, 65, 97, 129, 193, 257, 385, 513, 769,
    1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

/// Extra bits for distance codes
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3,
    4, 4, 5, 5, 6, 6, 7, 7, 8, 8,
    9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
];

/// Find length code index for a given match length
fn length_code(len: u16) -> Option<usize> {
    for i in (0..LENGTH_BASE.len()).rev() {
        if len >= LENGTH_BASE[i] {
            return Some(i);
        }
    }
    None
}

/// Find distance code index for a given distance
fn distance_code(dist: u16) -> Option<usize> {
    for i in (0..DIST_BASE.len()).rev() {
        if dist >= DIST_BASE[i] {
            return Some(i);
        }
    }
    None
}

/// Hash function for 3-byte sequences in hash chain
fn hash3(data: &[u8], pos: usize) -> u16 {
    if pos + 2 >= data.len() {
        return 0;
    }
    let h = (data[pos] as u32)
        | ((data[pos + 1] as u32) << 8)
        | ((data[pos + 2] as u32) << 16);
    // Mix and reduce to hash table size (15 bits = 32768 entries)
    ((h.wrapping_mul(2654435761)) >> 17) as u16
}

const HASH_SIZE: usize = 32768;
const WINDOW_SIZE: usize = 32768;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;

/// Compress data using DEFLATE with fixed Huffman codes and LZ77.
/// Level 0 = store only, 1-9 = compress with increasing effort.
pub fn deflate(data: &[u8], level: i32) -> Vec<u8> {
    if data.is_empty() {
        // Empty data: emit a single final block with end-of-block marker
        let mut writer = BitWriter::new();
        // BFINAL=1, BTYPE=01 (fixed Huffman)
        writer.write_bits(1, 1); // BFINAL
        writer.write_bits(1, 2); // BTYPE = fixed
        // End of block (symbol 256)
        let (code, len) = fixed_lit_code(256);
        writer.write_bits(code, len);
        return writer.into_bytes();
    }

    if level == 0 {
        return deflate_stored(data);
    }

    deflate_compressed(data, level)
}

/// Emit stored blocks (no compression)
fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(data.len() + (data.len() / 65535 + 1) * 5);
    let chunks: Vec<&[u8]> = data.chunks(65535).collect();
    let num_chunks = chunks.len();

    for (i, chunk) in chunks.iter().enumerate() {
        let is_final = i == num_chunks - 1;
        // BFINAL + BTYPE=00 (stored) - 3 bits, but stored blocks must be byte-aligned
        // The header byte: bit 0 = BFINAL, bits 1-2 = BTYPE (00)
        output.push(if is_final { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        let nlen = !len;
        output.push(len as u8);
        output.push((len >> 8) as u8);
        output.push(nlen as u8);
        output.push((nlen >> 8) as u8);
        output.extend_from_slice(chunk);
    }
    output
}

/// Compress with LZ77 + fixed Huffman codes
fn deflate_compressed(data: &[u8], level: i32) -> Vec<u8> {
    let mut writer = BitWriter::new();

    // BFINAL=1, BTYPE=01 (fixed Huffman)
    writer.write_bits(1, 1); // BFINAL
    writer.write_bits(1, 2); // BTYPE = fixed Huffman

    // Configure search based on level
    let max_chain = match level {
        1 => 4,
        2 => 8,
        3 => 16,
        4 => 32,
        5 => 64,
        6 => 128,
        7 => 256,
        8 => 512,
        _ => 1024, // level 9
    };

    let nice_match: usize = match level {
        1 => 8,
        2 => 16,
        3 => 32,
        4 => 64,
        5 => 128,
        _ => MAX_MATCH,
    };

    // Hash chain tables
    let mut head = vec![0u32; HASH_SIZE]; // head[hash] = most recent position + 1 (0 = none)
    let mut prev = vec![0u32; WINDOW_SIZE]; // prev[pos % WINDOW_SIZE] = previous position + 1

    let mut pos: usize = 0;

    while pos < data.len() {
        if pos + MIN_MATCH > data.len() {
            // Not enough bytes for a match, emit literals
            let (code, len) = fixed_lit_code(data[pos] as u16);
            writer.write_bits(code, len);
            pos += 1;
            continue;
        }

        let h = hash3(data, pos) as usize;

        // Find best match using hash chain
        let mut best_len: usize = MIN_MATCH - 1;
        let mut best_dist: usize = 0;
        let mut chain_entry = head[h];
        let mut chain_count = 0;

        let min_pos = if pos >= WINDOW_SIZE { pos - WINDOW_SIZE + 1 } else { 0 };

        while chain_entry > 0 && chain_count < max_chain {
            let match_pos = (chain_entry - 1) as usize;
            if match_pos < min_pos {
                break;
            }

            let dist = pos - match_pos;
            if dist > 0 && dist <= WINDOW_SIZE {
                // Compare bytes
                let max_possible = (data.len() - pos).min(MAX_MATCH);
                let mut match_len = 0;
                while match_len < max_possible
                    && data[match_pos + match_len] == data[pos + match_len]
                {
                    match_len += 1;
                }

                if match_len > best_len {
                    best_len = match_len;
                    best_dist = dist;
                    if best_len >= nice_match {
                        break;
                    }
                }
            }

            chain_entry = prev[match_pos % WINDOW_SIZE];
            chain_count += 1;
        }

        // Update hash chain
        prev[pos % WINDOW_SIZE] = head[h] as u32;
        head[h] = (pos + 1) as u32;

        if best_len >= MIN_MATCH && best_dist > 0 {
            // Emit length/distance pair
            let len_idx = length_code(best_len as u16).unwrap();
            let len_sym = 257 + len_idx as u16;
            let (code, code_len) = fixed_lit_code(len_sym);
            writer.write_bits(code, code_len);

            // Extra length bits
            let extra_val = best_len as u16 - LENGTH_BASE[len_idx];
            if LENGTH_EXTRA[len_idx] > 0 {
                writer.write_bits(extra_val as u32, LENGTH_EXTRA[len_idx]);
            }

            // Distance code
            let dist_idx = distance_code(best_dist as u16).unwrap();
            let (dcode, dcode_len) = fixed_dist_code(dist_idx as u16);
            writer.write_bits(dcode, dcode_len);

            // Extra distance bits
            let extra_dist = best_dist as u16 - DIST_BASE[dist_idx];
            if DIST_EXTRA[dist_idx] > 0 {
                writer.write_bits(extra_dist as u32, DIST_EXTRA[dist_idx]);
            }

            // Update hash chains for skipped positions
            for i in 1..best_len {
                let p = pos + i;
                if p + MIN_MATCH <= data.len() {
                    let ph = hash3(data, p) as usize;
                    prev[p % WINDOW_SIZE] = head[ph] as u32;
                    head[ph] = (p + 1) as u32;
                }
            }
            pos += best_len;
        } else {
            // Emit literal
            let (code, len) = fixed_lit_code(data[pos] as u16);
            writer.write_bits(code, len);
            pos += 1;
        }
    }

    // End of block (symbol 256)
    let (code, len) = fixed_lit_code(256);
    writer.write_bits(code, len);

    writer.into_bytes()
}
