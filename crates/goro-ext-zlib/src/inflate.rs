/// DEFLATE decompression (RFC 1951)
/// Implements full inflate with support for stored, fixed Huffman, and dynamic Huffman blocks.

/// Error type for inflate operations
#[derive(Debug)]
pub enum InflateError {
    InvalidData,
    OutputTooLarge,
    UnexpectedEnd,
}

/// Bitstream reader - reads bits LSB-first from a byte slice
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,    // byte position
    bit: u8,       // bit position within current byte (0-7)
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0, bit: 0 }
    }

    /// Read n bits (up to 25) LSB-first, returning them as a u32
    fn read_bits(&mut self, n: u8) -> Result<u32, InflateError> {
        let mut result: u32 = 0;
        let mut bits_read: u8 = 0;
        while bits_read < n {
            if self.pos >= self.data.len() {
                return Err(InflateError::UnexpectedEnd);
            }
            let available = 8 - self.bit;
            let to_read = (n - bits_read).min(available);
            let mask = (1u32 << to_read) - 1;
            let bits = ((self.data[self.pos] >> self.bit) as u32) & mask;
            result |= bits << bits_read;
            bits_read += to_read;
            self.bit += to_read;
            if self.bit >= 8 {
                self.bit = 0;
                self.pos += 1;
            }
        }
        Ok(result)
    }

    /// Align to next byte boundary
    fn align_to_byte(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }

    /// Read a u16 from the stream (byte-aligned, little-endian)
    fn read_u16_le(&mut self) -> Result<u16, InflateError> {
        self.align_to_byte();
        if self.pos + 2 > self.data.len() {
            return Err(InflateError::UnexpectedEnd);
        }
        let val = (self.data[self.pos] as u16) | ((self.data[self.pos + 1] as u16) << 8);
        self.pos += 2;
        Ok(val)
    }
}

/// Huffman tree for decoding
struct HuffmanTree {
    /// For each code length (index = code length), store (first_code, first_index)
    /// Entries store the decoded symbol
    symbols: Vec<u16>,
    /// counts[i] = number of codes with length i
    counts: Vec<u16>,
    /// Lookup table for fast decoding of short codes (up to FAST_BITS)
    fast_table: Vec<i16>,  // symbol or -1 if not decodable in fast path
    fast_lengths: Vec<u8>, // code length for fast table entry
}

const FAST_BITS: u8 = 9;
const FAST_SIZE: usize = 1 << FAST_BITS as usize;

impl HuffmanTree {
    /// Build a Huffman tree from an array of code lengths.
    /// code_lengths[i] = length of code for symbol i, 0 means symbol is not present.
    fn from_lengths(code_lengths: &[u8]) -> Result<Self, InflateError> {
        let max_len = *code_lengths.iter().max().unwrap_or(&0) as usize;
        if max_len > 15 {
            return Err(InflateError::InvalidData);
        }

        // Count occurrences of each code length
        let mut counts = vec![0u16; max_len + 1];
        for &len in code_lengths {
            counts[len as usize] += 1;
        }
        counts[0] = 0; // codes of length 0 don't exist

        // Compute first code for each length (RFC 1951 section 3.2.2)
        let mut next_code = vec![0u32; max_len + 1];
        let mut code: u32 = 0;
        for bits in 1..=max_len {
            code = (code + counts[bits - 1] as u32) << 1;
            next_code[bits] = code;
        }

        // Assign codes to symbols and build symbol table sorted by (length, code)
        let num_symbols = code_lengths.len();
        let total_codes: usize = counts.iter().map(|&c| c as usize).sum();
        let mut symbols = vec![0u16; total_codes.max(1)];

        // Build offsets: offset[len] = starting index in symbols for codes of that length
        let mut offsets = vec![0usize; max_len + 2];
        for i in 1..=max_len {
            offsets[i + 1] = offsets[i] + counts[i] as usize;
        }

        // Place symbols
        let mut offset_copy = offsets.clone();
        for sym in 0..num_symbols {
            let len = code_lengths[sym] as usize;
            if len > 0 {
                let idx = offset_copy[len];
                if idx < symbols.len() {
                    symbols[idx] = sym as u16;
                }
                offset_copy[len] += 1;
            }
        }

        // Build fast lookup table
        let mut fast_table = vec![-1i16; FAST_SIZE];
        let mut fast_lengths = vec![0u8; FAST_SIZE];

        for sym in 0..num_symbols {
            let len = code_lengths[sym] as usize;
            if len > 0 && len <= FAST_BITS as usize {
                // Assign the code for this symbol
                let c = next_code[len];
                next_code[len] += 1;
                // Reverse bits for fast lookup (we read LSB first)
                let rev = reverse_bits(c as u16, len as u8);
                // Fill all fast table entries that share this prefix
                let step = 1usize << len;
                let mut idx = rev as usize;
                while idx < FAST_SIZE {
                    fast_table[idx] = sym as i16;
                    fast_lengths[idx] = len as u8;
                    idx += step;
                }
            }
        }

        Ok(HuffmanTree {
            symbols,
            counts,
            fast_table,
            fast_lengths,
        })
    }

    /// Decode one symbol from the bitstream
    fn decode(&self, reader: &mut BitReader) -> Result<u16, InflateError> {
        // Peek at FAST_BITS bits for fast path
        if reader.pos < reader.data.len() {
            // Gather up to FAST_BITS bits without consuming them
            let mut peek: u32 = 0;
            let mut available: u8 = 0;
            let mut byte_pos = reader.pos;
            let mut bit_pos = reader.bit;
            while available < FAST_BITS && byte_pos < reader.data.len() {
                let bits_in_byte = 8 - bit_pos;
                let to_take = (FAST_BITS - available).min(bits_in_byte);
                let mask = (1u32 << to_take) - 1;
                let bits = ((reader.data[byte_pos] >> bit_pos) as u32) & mask;
                peek |= bits << available;
                available += to_take;
                bit_pos += to_take;
                if bit_pos >= 8 {
                    bit_pos = 0;
                    byte_pos += 1;
                }
            }
            let idx = (peek & (FAST_SIZE as u32 - 1)) as usize;
            if self.fast_table[idx] >= 0 {
                let sym = self.fast_table[idx] as u16;
                let len = self.fast_lengths[idx];
                // Consume len bits
                reader.read_bits(len)?;
                return Ok(sym);
            }
        }

        // Slow path: decode bit by bit
        let mut code: u32 = 0;
        let mut first: u32 = 0;
        let mut index: usize = 0;
        for len in 1..self.counts.len() {
            code |= reader.read_bits(1)?;
            let count = self.counts[len] as u32;
            if code < first + count {
                let sym_idx = index + (code - first) as usize;
                if sym_idx < self.symbols.len() {
                    return Ok(self.symbols[sym_idx]);
                }
                return Err(InflateError::InvalidData);
            }
            index += count as usize;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(InflateError::InvalidData)
    }
}

/// Reverse the bottom `len` bits of `val`
fn reverse_bits(val: u16, len: u8) -> u16 {
    let mut result: u16 = 0;
    let mut v = val;
    for _ in 0..len {
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

/// Build fixed Huffman trees (RFC 1951, section 3.2.6)
fn build_fixed_lit_tree() -> HuffmanTree {
    let mut lengths = [0u8; 288];
    for i in 0..=143 { lengths[i] = 8; }
    for i in 144..=255 { lengths[i] = 9; }
    for i in 256..=279 { lengths[i] = 7; }
    for i in 280..=287 { lengths[i] = 8; }
    HuffmanTree::from_lengths(&lengths).unwrap()
}

fn build_fixed_dist_tree() -> HuffmanTree {
    let lengths = [5u8; 32];
    HuffmanTree::from_lengths(&lengths).unwrap()
}

/// Order of code length alphabet codes for dynamic Huffman tables
const CODELEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Inflate (decompress) raw DEFLATE data.
/// Returns the decompressed bytes.
/// If max_length is Some(n), limits output to n bytes.
pub fn inflate(data: &[u8], max_length: Option<usize>) -> Result<Vec<u8>, InflateError> {
    let mut reader = BitReader::new(data);
    let mut output = Vec::with_capacity(data.len().saturating_mul(2).min(65536));
    let max_len = max_length.unwrap_or(usize::MAX);

    loop {
        // Read BFINAL bit
        let bfinal = reader.read_bits(1)?;
        // Read BTYPE (2 bits)
        let btype = reader.read_bits(2)?;

        match btype {
            0 => {
                // Stored block (no compression)
                let len = reader.read_u16_le()?;
                let nlen = reader.read_u16_le()?;
                if len != !nlen {
                    return Err(InflateError::InvalidData);
                }
                let len = len as usize;
                if reader.pos + len > reader.data.len() {
                    return Err(InflateError::UnexpectedEnd);
                }
                if output.len() + len > max_len {
                    return Err(InflateError::OutputTooLarge);
                }
                output.extend_from_slice(&reader.data[reader.pos..reader.pos + len]);
                reader.pos += len;
            }
            1 => {
                // Fixed Huffman codes
                let lit_tree = build_fixed_lit_tree();
                let dist_tree = build_fixed_dist_tree();
                inflate_block(&mut reader, &lit_tree, &dist_tree, &mut output, max_len)?;
            }
            2 => {
                // Dynamic Huffman codes
                let hlit = reader.read_bits(5)? as usize + 257;
                let hdist = reader.read_bits(5)? as usize + 1;
                let hclen = reader.read_bits(4)? as usize + 4;

                // Read code length code lengths
                let mut codelen_lengths = [0u8; 19];
                for i in 0..hclen {
                    codelen_lengths[CODELEN_ORDER[i]] = reader.read_bits(3)? as u8;
                }

                let codelen_tree = HuffmanTree::from_lengths(&codelen_lengths)?;

                // Decode literal/length + distance code lengths
                let total = hlit + hdist;
                let mut lengths = vec![0u8; total];
                let mut i = 0;
                while i < total {
                    let sym = codelen_tree.decode(&mut reader)?;
                    match sym {
                        0..=15 => {
                            lengths[i] = sym as u8;
                            i += 1;
                        }
                        16 => {
                            // Repeat previous length 3-6 times
                            if i == 0 {
                                return Err(InflateError::InvalidData);
                            }
                            let repeat = reader.read_bits(2)? as usize + 3;
                            let prev = lengths[i - 1];
                            for _ in 0..repeat {
                                if i >= total {
                                    return Err(InflateError::InvalidData);
                                }
                                lengths[i] = prev;
                                i += 1;
                            }
                        }
                        17 => {
                            // Repeat 0 for 3-10 times
                            let repeat = reader.read_bits(3)? as usize + 3;
                            for _ in 0..repeat {
                                if i >= total {
                                    return Err(InflateError::InvalidData);
                                }
                                lengths[i] = 0;
                                i += 1;
                            }
                        }
                        18 => {
                            // Repeat 0 for 11-138 times
                            let repeat = reader.read_bits(7)? as usize + 11;
                            for _ in 0..repeat {
                                if i >= total {
                                    return Err(InflateError::InvalidData);
                                }
                                lengths[i] = 0;
                                i += 1;
                            }
                        }
                        _ => return Err(InflateError::InvalidData),
                    }
                }

                let lit_tree = HuffmanTree::from_lengths(&lengths[..hlit])?;
                let dist_tree = HuffmanTree::from_lengths(&lengths[hlit..hlit + hdist])?;
                inflate_block(&mut reader, &lit_tree, &dist_tree, &mut output, max_len)?;
            }
            _ => {
                return Err(InflateError::InvalidData);
            }
        }

        if bfinal != 0 {
            break;
        }
    }

    Ok(output)
}

/// Decompress a single Huffman-encoded block
fn inflate_block(
    reader: &mut BitReader,
    lit_tree: &HuffmanTree,
    dist_tree: &HuffmanTree,
    output: &mut Vec<u8>,
    max_len: usize,
) -> Result<(), InflateError> {
    loop {
        let sym = lit_tree.decode(reader)?;
        match sym {
            0..=255 => {
                // Literal byte
                if output.len() >= max_len {
                    return Err(InflateError::OutputTooLarge);
                }
                output.push(sym as u8);
            }
            256 => {
                // End of block
                return Ok(());
            }
            257..=285 => {
                // Length/distance pair
                let len_idx = (sym - 257) as usize;
                if len_idx >= LENGTH_BASE.len() {
                    return Err(InflateError::InvalidData);
                }
                let length = LENGTH_BASE[len_idx] as usize
                    + reader.read_bits(LENGTH_EXTRA[len_idx])? as usize;

                let dist_sym = dist_tree.decode(reader)? as usize;
                if dist_sym >= DIST_BASE.len() {
                    return Err(InflateError::InvalidData);
                }
                let distance = DIST_BASE[dist_sym] as usize
                    + reader.read_bits(DIST_EXTRA[dist_sym])? as usize;

                if distance > output.len() {
                    return Err(InflateError::InvalidData);
                }

                if output.len() + length > max_len {
                    return Err(InflateError::OutputTooLarge);
                }

                // Copy from sliding window - must handle overlapping copies
                let start = output.len() - distance;
                for i in 0..length {
                    let byte = output[start + (i % distance)];
                    output.push(byte);
                }
            }
            _ => {
                return Err(InflateError::InvalidData);
            }
        }
    }
}
