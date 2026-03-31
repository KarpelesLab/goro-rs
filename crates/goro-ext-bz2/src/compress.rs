/// bzip2 compression implementation
/// Pipeline: RLE1 -> BWT -> MTF -> RLE2 -> Huffman -> bitstream

/// CRC-32 lookup table (polynomial 0x04C11DB7, non-reflected)
/// bzip2 uses non-reflected CRC-32 (big-endian style)
fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for i in 0..256 {
        let mut crc = (i as u32) << 24;
        for _ in 0..8 {
            if crc & 0x80000000 != 0 {
                crc = (crc << 1) ^ 0x04C11DB7;
            } else {
                crc <<= 1;
            }
        }
        table[i] = crc;
    }
    table
}

fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let table = make_crc_table();
    let mut crc = crc;
    for &b in data {
        crc = (crc << 8) ^ table[((crc >> 24) ^ b as u32) as usize];
    }
    crc
}

/// Bitstream writer (MSB-first, as bzip2 requires)
struct BitWriter {
    buf: Vec<u8>,
    current: u32,
    bits_in: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            current: 0,
            bits_in: 0,
        }
    }

    fn write_bits(&mut self, value: u32, n: u8) {
        // Write n bits from value (MSB-first)
        for i in (0..n).rev() {
            self.current = (self.current << 1) | ((value >> i) & 1);
            self.bits_in += 1;
            if self.bits_in == 8 {
                self.buf.push(self.current as u8);
                self.current = 0;
                self.bits_in = 0;
            }
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.write_bits(value as u32, 8);
    }

    fn write_u32(&mut self, value: u32) {
        self.write_bits(value >> 16, 16);
        self.write_bits(value & 0xFFFF, 16);
    }

    fn flush(mut self) -> Vec<u8> {
        if self.bits_in > 0 {
            self.current <<= 8 - self.bits_in;
            self.buf.push(self.current as u8);
        }
        self.buf
    }
}

/// Step 1: RLE1 - run-length encode runs of 4+ identical bytes
/// Runs of N identical bytes (N >= 4) become: byte byte byte byte (N-4)
/// where (N-4) is stored as a single byte, allowing runs up to 259
fn rle1_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        let mut run = 1;
        while i + run < data.len() && data[i + run] == b && run < 259 {
            run += 1;
        }
        if run >= 4 {
            // Write 4 copies then the repeat count
            out.push(b);
            out.push(b);
            out.push(b);
            out.push(b);
            out.push((run - 4) as u8);
            i += run;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Step 2: Burrows-Wheeler Transform (forward)
/// Returns (transformed data, primary_index)
fn bwt_encode(data: &[u8]) -> (Vec<u8>, u32) {
    let n = data.len();
    if n == 0 {
        return (Vec::new(), 0);
    }

    // Build suffix array indices and sort by cyclic rotations
    let mut indices: Vec<u32> = (0..n as u32).collect();

    // Sort using cyclic rotation comparison
    indices.sort_unstable_by(|&a, &b| {
        let a = a as usize;
        let b = b as usize;
        for k in 0..n {
            let ca = data[(a + k) % n];
            let cb = data[(b + k) % n];
            match ca.cmp(&cb) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }
        std::cmp::Ordering::Equal
    });

    // The last column = data[(index + n - 1) % n] for each sorted rotation
    let mut last_col = Vec::with_capacity(n);
    let mut primary_index = 0u32;
    for (row, &idx) in indices.iter().enumerate() {
        if idx == 0 {
            primary_index = row as u32;
        }
        last_col.push(data[(idx as usize + n - 1) % n]);
    }

    (last_col, primary_index)
}

/// Step 3: Move-to-Front transform
/// Returns (mtf_output, symbols_used)
fn mtf_encode(data: &[u8]) -> (Vec<u16>, [bool; 256]) {
    let mut symbols_used = [false; 256];
    for &b in data {
        symbols_used[b as usize] = true;
    }

    // Build initial symbol list from used symbols only
    let mut symbol_list: Vec<u8> = (0u16..256)
        .filter(|&i| symbols_used[i as usize])
        .map(|i| i as u8)
        .collect();

    let mut out = Vec::with_capacity(data.len());
    for &b in data {
        // Find position of b in symbol_list
        let pos = symbol_list.iter().position(|&s| s == b).unwrap();
        out.push(pos as u16);
        // Move to front
        if pos > 0 {
            let ch = symbol_list.remove(pos);
            symbol_list.insert(0, ch);
        }
    }

    (out, symbols_used)
}

/// Step 4: RLE2 - zero-run-length encoding using RUNA/RUNB
/// Encodes runs of zeros using bijective base-2 numeration with RUNA(0) and RUNB(1)
/// Also adds the EOB symbol at the end
fn rle2_encode(mtf: &[u16], num_symbols: u16) -> Vec<u16> {
    // RUNA = 0, RUNB = 1, other symbols shifted by 1, EOB = num_symbols + 1
    let eob = num_symbols + 1;
    let mut out = Vec::with_capacity(mtf.len());
    let mut i = 0;
    while i < mtf.len() {
        if mtf[i] == 0 {
            // Count the run of zeros
            let mut count: u32 = 0;
            while i < mtf.len() && mtf[i] == 0 {
                count += 1;
                i += 1;
            }
            // Encode count using bijective base-2 with RUNA(0) and RUNB(1)
            // count = sum of (digit+1) * 2^position
            // where digit is 0 (RUNA) or 1 (RUNB)
            let mut run_len = count;
            let mut codes = Vec::new();
            while run_len > 0 {
                run_len -= 1;
                if run_len & 1 == 0 {
                    codes.push(0u16); // RUNA
                } else {
                    codes.push(1u16); // RUNB
                }
                run_len >>= 1;
            }
            out.extend_from_slice(&codes);
        } else {
            out.push(mtf[i] + 1); // shift symbols by 1
            i += 1;
        }
    }
    out.push(eob);
    out
}

/// Step 5: Huffman coding
/// bzip2 uses 2-6 Huffman tables, with selectors choosing per 50-symbol group

/// Build Huffman code lengths using a simple method
fn build_code_lengths(freqs: &[u32], max_len: u8) -> Vec<u8> {
    let n = freqs.len();
    if n == 0 {
        return Vec::new();
    }

    // Simple approach: use a package-merge-like algorithm
    // For bzip2 we need lengths capped at max_len (typically 17 or 20)
    let mut lengths = vec![0u8; n];

    // Find symbols that actually appear
    let total: u64 = freqs.iter().map(|&f| f as u64).sum();
    if total == 0 {
        // All zero frequencies - assign length 1 to first symbol
        if !lengths.is_empty() {
            lengths[0] = 1;
        }
        return lengths;
    }

    // Build a Huffman tree using a simple method
    // We'll use a priority queue approach
    #[derive(Clone)]
    struct Node {
        freq: u64,
        // For leaves: symbol index, for internal: -1
        symbol: i32,
        left: Option<Box<Node>>,
        right: Option<Box<Node>>,
    }

    let mut nodes: Vec<Node> = Vec::new();
    for (i, &f) in freqs.iter().enumerate() {
        // Give zero-frequency symbols a tiny frequency so they still get codes
        let freq = if f == 0 { 1 } else { f as u64 };
        nodes.push(Node {
            freq,
            symbol: i as i32,
            left: None,
            right: None,
        });
    }

    // Build tree by combining two smallest nodes repeatedly
    while nodes.len() > 1 {
        // Sort by frequency (stable sort to preserve order for equal freqs)
        nodes.sort_by_key(|n| n.freq);
        let left = nodes.remove(0);
        let right = nodes.remove(0);
        let combined = Node {
            freq: left.freq + right.freq,
            symbol: -1,
            left: Some(Box::new(left)),
            right: Some(Box::new(right)),
        };
        nodes.push(combined);
    }

    // Extract code lengths from tree
    fn assign_lengths(node: &Node, depth: u8, lengths: &mut Vec<u8>) {
        if node.symbol >= 0 {
            lengths[node.symbol as usize] = depth.max(1);
        } else {
            if let Some(ref left) = node.left {
                assign_lengths(left, depth + 1, lengths);
            }
            if let Some(ref right) = node.right {
                assign_lengths(right, depth + 1, lengths);
            }
        }
    }

    assign_lengths(&nodes[0], 0, &mut lengths);

    // Handle single-symbol case
    if n == 1 {
        lengths[0] = 1;
        return lengths;
    }

    // Cap lengths at max_len using a simple redistribution
    let mut capped = false;
    for l in lengths.iter_mut() {
        if *l > max_len {
            *l = max_len;
            capped = true;
        }
    }

    if capped {
        // Redistribute to maintain valid prefix code (Kraft inequality)
        // Simple approach: keep reducing longest codes until valid
        loop {
            let kraft_sum: f64 = lengths
                .iter()
                .map(|&l| if l > 0 { 2.0f64.powi(-(l as i32)) } else { 0.0 })
                .sum();
            if kraft_sum <= 1.0 + 1e-10 {
                break;
            }
            // Find the longest code and shorten it
            if let Some(pos) = lengths.iter().position(|&l| l == max_len) {
                lengths[pos] = max_len - 1;
            } else {
                break;
            }
        }
    }

    lengths
}

/// Generate canonical Huffman codes from code lengths
fn generate_codes(lengths: &[u8]) -> Vec<(u32, u8)> {
    let n = lengths.len();
    let mut codes = vec![(0u32, 0u8); n];

    if n == 0 {
        return codes;
    }

    let max_len = *lengths.iter().max().unwrap_or(&0);
    if max_len == 0 {
        return codes;
    }

    // Count codes of each length
    let mut bl_count = vec![0u32; max_len as usize + 1];
    for &l in lengths {
        if l > 0 {
            bl_count[l as usize] += 1;
        }
    }

    // Compute starting code for each length
    let mut next_code = vec![0u32; max_len as usize + 1];
    let mut code = 0u32;
    for bits in 1..=max_len as usize {
        code = (code + bl_count[bits - 1]) << 1;
        next_code[bits] = code;
    }

    // Assign codes
    for i in 0..n {
        let len = lengths[i];
        if len > 0 {
            codes[i] = (next_code[len as usize], len);
            next_code[len as usize] += 1;
        }
    }

    codes
}

/// Compress a single block of data
fn compress_block(
    writer: &mut BitWriter,
    data: &[u8],
    block_crc: u32,
) {
    // Block header magic: 0x314159265359 (48 bits)
    writer.write_bits(0x3141, 16);
    writer.write_bits(0x5926, 16);
    writer.write_bits(0x5359, 16);

    // Block CRC
    writer.write_u32(block_crc);

    // Randomized flag (0 = not randomized)
    writer.write_bits(0, 1);

    // Step 1: RLE1
    let rle1 = rle1_encode(data);

    // Step 2: BWT
    let (bwt_data, primary_index) = bwt_encode(&rle1);

    // Write origPtr (24 bits)
    writer.write_bits(primary_index, 24);

    // Step 3: MTF
    let (mtf_data, symbols_used) = mtf_encode(&bwt_data);

    // Count number of used symbols
    let num_used: u16 = symbols_used.iter().filter(|&&b| b).count() as u16;

    // Write symbol map (which symbols are in use)
    // First 16 bits: which groups of 16 are used
    let mut used_groups = 0u16;
    for g in 0..16 {
        let base = g * 16;
        let any_used = (base..base + 16).any(|i| symbols_used[i as usize]);
        if any_used {
            used_groups |= 1 << (15 - g);
        }
    }
    writer.write_bits(used_groups as u32, 16);

    // Then for each used group, 16 bits indicating which symbols within
    for g in 0..16u16 {
        if used_groups & (1 << (15 - g)) != 0 {
            let base = g * 16;
            let mut group_map = 0u16;
            for j in 0..16 {
                if symbols_used[(base + j) as usize] {
                    group_map |= 1 << (15 - j);
                }
            }
            writer.write_bits(group_map as u32, 16);
        }
    }

    // Step 4: RLE2
    let rle2_data = rle2_encode(&mtf_data, num_used);

    // Alpha size = num_used + 2 (for RUNA, RUNB at bottom, and EOB at top)
    let alpha_size = num_used as usize + 2;

    // Determine number of Huffman tables and selectors
    let n_symbols = rle2_data.len();
    let n_groups = match n_symbols {
        0..=199 => 2usize,
        200..=599 => 3,
        600..=1199 => 4,
        1200..=2399 => 5,
        _ => 6,
    };
    let n_selectors = (n_symbols + 49) / 50;

    // For simplicity with a single table (or few tables), assign all groups to table 0
    // Use a simple strategy: build one Huffman table from overall frequencies

    // Count frequencies
    let mut freqs = vec![0u32; alpha_size];
    for &sym in &rle2_data {
        freqs[sym as usize] += 1;
    }

    // Build code lengths for each table
    // For simplicity, we'll use n_groups identical tables
    let code_lengths = build_code_lengths(&freqs, 20);

    // All selectors point to table 0
    let selectors: Vec<u8> = vec![0; n_selectors];

    // Write number of Huffman tables
    writer.write_bits(n_groups as u32, 3);

    // Write number of selectors
    writer.write_bits(n_selectors as u32, 15);

    // Write selectors (MTF-encoded as unary)
    // Since all selectors are 0, and MTF starts with [0,1,2,...], all selectors encode as 0
    // Unary encoding: 0 -> "0", 1 -> "10", 2 -> "110", etc.
    {
        let mut sel_mtf_list: Vec<u8> = (0..n_groups as u8).collect();
        for &sel in &selectors {
            let pos = sel_mtf_list.iter().position(|&s| s == sel).unwrap();
            // Write unary: pos ones followed by a zero
            for _ in 0..pos {
                writer.write_bits(1, 1);
            }
            writer.write_bits(0, 1);
            // Move to front
            if pos > 0 {
                let v = sel_mtf_list.remove(pos);
                sel_mtf_list.insert(0, v);
            }
        }
    }

    // Write Huffman tables using delta encoding
    // For each table, write code lengths using delta encoding
    let codes = generate_codes(&code_lengths);
    for _t in 0..n_groups {
        // Starting length (5 bits) - use first symbol's length
        let start_len = code_lengths[0];
        writer.write_bits(start_len as u32, 5);

        let mut current = start_len;
        for i in 0..alpha_size {
            let target = code_lengths[i];
            while current != target {
                // Write "1" to signal a change
                writer.write_bits(1, 1);
                if current < target {
                    // Increment: write "0" after the "1"
                    writer.write_bits(0, 1);
                    current += 1;
                } else {
                    // Decrement: write "1" after the "1"
                    writer.write_bits(1, 1);
                    current -= 1;
                }
            }
            // End with "0" to signal done with this symbol
            writer.write_bits(0, 1);
        }
    }

    // Write compressed data using Huffman codes
    for &sym in &rle2_data {
        let (code, len) = codes[sym as usize];
        writer.write_bits(code, len);
    }
}

/// Main compression function
pub fn bzcompress(data: &[u8], block_size: u32) -> Vec<u8> {
    let block_bytes = block_size as usize * 100_000;

    let mut writer = BitWriter::new();

    // File header: "BZ" + "h" + block_size_char
    writer.write_u8(b'B');
    writer.write_u8(b'Z');
    writer.write_u8(b'h');
    writer.write_u8(b'0' + block_size as u8);

    // Process blocks
    let mut combined_crc: u32 = 0;
    let mut offset = 0;

    while offset < data.len() {
        let end = (offset + block_bytes).min(data.len());
        let block_data = &data[offset..end];

        // Compute CRC for this block
        let block_crc = crc32_update(0xFFFFFFFF, block_data) ^ 0xFFFFFFFF;

        // Update combined CRC: combined = ((combined << 1) | (combined >> 31)) ^ block_crc
        combined_crc = (combined_crc.wrapping_shl(1) | combined_crc.wrapping_shr(31)) ^ block_crc;

        compress_block(&mut writer, block_data, block_crc);
        offset = end;
    }

    // Handle empty data - still need to write the end marker
    if data.is_empty() {
        // No blocks to write, combined CRC stays 0
    }

    // End-of-stream marker: 0x177245385090 (48 bits)
    writer.write_bits(0x1772, 16);
    writer.write_bits(0x4538, 16);
    writer.write_bits(0x5090, 16);

    // Combined CRC
    writer.write_u32(combined_crc);

    writer.flush()
}
