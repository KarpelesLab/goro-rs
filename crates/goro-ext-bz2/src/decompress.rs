/// bzip2 decompression implementation
/// Pipeline: bitstream -> Huffman decode -> undo RLE2 -> undo MTF -> undo BWT -> undo RLE1

/// CRC-32 lookup table (polynomial 0x04C11DB7, non-reflected)
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

/// Bitstream reader (MSB-first, as bzip2 requires)
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u8, // bits remaining in current byte (8 = full, 0 = need next byte)
    current: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bit: 0,
            current: 0,
        }
    }

    fn read_bit(&mut self) -> Result<u32, &'static str> {
        if self.bit == 0 {
            if self.pos >= self.data.len() {
                return Err("unexpected end of data");
            }
            self.current = self.data[self.pos];
            self.pos += 1;
            self.bit = 8;
        }
        self.bit -= 1;
        Ok(((self.current >> self.bit) & 1) as u32)
    }

    fn read_bits(&mut self, n: u8) -> Result<u32, &'static str> {
        let mut value = 0u32;
        for _ in 0..n {
            value = (value << 1) | self.read_bit()?;
        }
        Ok(value)
    }

    fn read_u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.read_bits(8)? as u8)
    }

    fn read_u32(&mut self) -> Result<u32, &'static str> {
        let hi = self.read_bits(16)?;
        let lo = self.read_bits(16)?;
        Ok((hi << 16) | lo)
    }
}

/// Huffman decoder table for a single table
struct HuffmanTable {
    min_len: u8,
    max_len: u8,
    // For each code length, the base code value and starting symbol index
    limits: Vec<u32>,  // max code value for each length (+1)
    bases: Vec<u32>,   // base symbol index for each length
    perms: Vec<u16>,   // permutation: symbol ordered by (length, code)
}

impl HuffmanTable {
    fn from_lengths(lengths: &[u8]) -> Self {
        let n = lengths.len();
        let min_len = *lengths.iter().filter(|&&l| l > 0).min().unwrap_or(&1);
        let max_len = *lengths.iter().max().unwrap_or(&1);

        // Count codes of each length
        let mut bl_count = vec![0u32; max_len as usize + 1];
        for &l in lengths {
            if l > 0 {
                bl_count[l as usize] += 1;
            }
        }

        // Build bases and limits
        let range = (max_len - min_len + 1) as usize;
        let mut bases = vec![0u32; range];
        let mut limits = vec![0u32; range];

        // Build the permutation array
        let mut perms = vec![0u16; n];
        let mut idx = 0;
        for len in min_len..=max_len {
            for sym in 0..n {
                if lengths[sym] == len {
                    perms[idx] = sym as u16;
                    idx += 1;
                }
            }
        }

        // Compute bases and limits for each code length
        let mut code = 0u32;
        let mut perm_idx = 0u32;
        for len in min_len..=max_len {
            let li = (len - min_len) as usize;
            bases[li] = perm_idx.wrapping_sub(code);
            code += bl_count[len as usize];
            limits[li] = code.wrapping_sub(1);
            code <<= 1;
            perm_idx += bl_count[len as usize];
        }

        Self {
            min_len,
            max_len,
            limits,
            bases,
            perms,
        }
    }

    fn decode(&self, reader: &mut BitReader) -> Result<u16, &'static str> {
        let mut code = 0u32;
        for _ in 0..self.min_len {
            code = (code << 1) | reader.read_bit()?;
        }

        for len in self.min_len..=self.max_len {
            let li = (len - self.min_len) as usize;
            if code <= self.limits[li] {
                let idx = code.wrapping_add(self.bases[li]) as usize;
                if idx < self.perms.len() {
                    return Ok(self.perms[idx]);
                }
                return Err("invalid Huffman code");
            }
            code = (code << 1) | reader.read_bit()?;
        }
        Err("invalid Huffman code")
    }
}

/// Undo Move-to-Front transform
fn mtf_decode(data: &[u16], used_symbols: &[u8]) -> Vec<u8> {
    let mut symbol_list = used_symbols.to_vec();
    let mut out = Vec::with_capacity(data.len());

    for &pos in data {
        let pos = pos as usize;
        let ch = symbol_list[pos];
        out.push(ch);
        if pos > 0 {
            symbol_list.remove(pos);
            symbol_list.insert(0, ch);
        }
    }

    out
}

/// Undo BWT using the T-transform
fn bwt_decode(last_col: &[u8], primary_index: u32) -> Vec<u8> {
    let n = last_col.len();
    if n == 0 {
        return Vec::new();
    }

    // Count occurrences of each byte
    let mut count = [0u32; 256];
    for &b in last_col {
        count[b as usize] += 1;
    }

    // Build cumulative frequency table
    let mut cum_freq = [0u32; 256];
    let mut sum = 0u32;
    for i in 0..256 {
        cum_freq[i] = sum;
        sum += count[i];
    }

    // Build T-transform
    let mut transform = vec![0u32; n];
    let mut running_count = cum_freq;
    for i in 0..n {
        let b = last_col[i] as usize;
        transform[i] = running_count[b];
        running_count[b] += 1;
    }

    // Reconstruct original data by following the chain
    let mut out = vec![0u8; n];
    let mut idx = primary_index as usize;
    for i in (0..n).rev() {
        out[i] = last_col[idx];
        idx = transform[idx] as usize;
    }

    out
}

/// Undo RLE1 - decode runs encoded as byte*4 + count
fn rle1_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        out.push(b);
        i += 1;

        // Check for run of 4 identical bytes
        if i < data.len() && data[i] == b {
            out.push(b);
            i += 1;
            if i < data.len() && data[i] == b {
                out.push(b);
                i += 1;
                if i < data.len() && data[i] == b {
                    out.push(b);
                    i += 1;
                    // Next byte is the repeat count
                    if i < data.len() {
                        let count = data[i] as usize;
                        i += 1;
                        for _ in 0..count {
                            out.push(b);
                        }
                    }
                }
            }
        }
    }
    out
}

/// Decompress a bzip2 stream
pub fn bzdecompress(data: &[u8]) -> Result<Vec<u8>, i64> {
    if data.len() < 4 {
        return Err(-1); // BZ_DATA_ERROR
    }

    let mut reader = BitReader::new(data);

    // Read file header
    let b = reader.read_u8().map_err(|_| -1i64)?;
    let z = reader.read_u8().map_err(|_| -1i64)?;
    let h = reader.read_u8().map_err(|_| -1i64)?;
    let level = reader.read_u8().map_err(|_| -1i64)?;

    if b != b'B' || z != b'Z' || h != b'h' || level < b'1' || level > b'9' {
        return Err(-1); // BZ_DATA_ERROR
    }

    let _block_size = (level - b'0') as usize;

    let mut output = Vec::new();
    let mut combined_crc: u32 = 0;

    loop {
        // Read block magic (48 bits)
        let magic_hi = reader.read_bits(16).map_err(|_| -1i64)? as u64;
        let magic_mid = reader.read_bits(16).map_err(|_| -1i64)? as u64;
        let magic_lo = reader.read_bits(16).map_err(|_| -1i64)? as u64;
        let magic = (magic_hi << 32) | (magic_mid << 16) | magic_lo;

        if magic == 0x177245385090 {
            // End-of-stream marker
            let stored_crc = reader.read_u32().map_err(|_| -1i64)?;
            if stored_crc != combined_crc {
                return Err(-4); // BZ_DATA_ERROR
            }
            break;
        }

        if magic != 0x314159265359 {
            return Err(-1); // BZ_DATA_ERROR - bad block magic
        }

        // Block CRC
        let stored_block_crc = reader.read_u32().map_err(|_| -1i64)?;

        // Randomized flag
        let randomized = reader.read_bit().map_err(|_| -1i64)?;
        if randomized != 0 {
            return Err(-1); // We don't support randomized blocks (deprecated in bzip2)
        }

        // origPtr (24 bits) - primary index for BWT
        let orig_ptr = reader.read_bits(24).map_err(|_| -1i64)?;

        // Read symbol map
        let used_groups = reader.read_bits(16).map_err(|_| -1i64)?;
        let mut symbols_in_use = [false; 256];

        for g in 0..16u32 {
            if used_groups & (1 << (15 - g)) != 0 {
                let group_map = reader.read_bits(16).map_err(|_| -1i64)?;
                for j in 0..16u32 {
                    if group_map & (1 << (15 - j)) != 0 {
                        symbols_in_use[(g * 16 + j) as usize] = true;
                    }
                }
            }
        }

        let used_symbols: Vec<u8> = (0..256u16)
            .filter(|&i| symbols_in_use[i as usize])
            .map(|i| i as u8)
            .collect();
        let n_in_use = used_symbols.len();
        let alpha_size = n_in_use + 2; // +2 for RUNA, RUNB / EOB

        // Number of Huffman tables
        let n_groups = reader.read_bits(3).map_err(|_| -1i64)? as usize;
        if n_groups < 2 || n_groups > 6 {
            return Err(-1);
        }

        // Number of selectors
        let n_selectors = reader.read_bits(15).map_err(|_| -1i64)? as usize;
        if n_selectors == 0 {
            return Err(-1);
        }

        // Read selectors (MTF-encoded unary)
        let mut selectors_mtf = Vec::with_capacity(n_selectors);
        for _ in 0..n_selectors {
            let mut j = 0u8;
            loop {
                let bit = reader.read_bit().map_err(|_| -1i64)?;
                if bit == 0 {
                    break;
                }
                j += 1;
                if j as usize >= n_groups {
                    return Err(-1);
                }
            }
            selectors_mtf.push(j);
        }

        // Undo MTF on selectors
        let mut sel_list: Vec<u8> = (0..n_groups as u8).collect();
        let mut selectors = Vec::with_capacity(n_selectors);
        for &mtf_val in &selectors_mtf {
            let pos = mtf_val as usize;
            if pos >= sel_list.len() {
                return Err(-1);
            }
            let val = sel_list[pos];
            selectors.push(val);
            if pos > 0 {
                sel_list.remove(pos);
                sel_list.insert(0, val);
            }
        }

        // Read Huffman code lengths for each table
        let mut tables = Vec::with_capacity(n_groups);
        for _ in 0..n_groups {
            let mut lengths = vec![0u8; alpha_size];
            let mut curr = reader.read_bits(5).map_err(|_| -1i64)? as i32;

            for i in 0..alpha_size {
                loop {
                    if curr < 1 || curr > 20 {
                        return Err(-1);
                    }
                    let bit = reader.read_bit().map_err(|_| -1i64)?;
                    if bit == 0 {
                        break;
                    }
                    let direction = reader.read_bit().map_err(|_| -1i64)?;
                    if direction == 0 {
                        curr += 1;
                    } else {
                        curr -= 1;
                    }
                }
                lengths[i] = curr as u8;
            }

            tables.push(HuffmanTable::from_lengths(&lengths));
        }

        // Decode symbols using Huffman tables
        let eob = (alpha_size - 1) as u16;
        let mut decoded_mtf: Vec<u16> = Vec::new();
        let mut group_idx = 0usize;
        let mut group_pos = 0usize;

        loop {
            let table_idx = if group_idx < selectors.len() {
                selectors[group_idx] as usize
            } else {
                return Err(-1);
            };

            let sym = tables[table_idx].decode(&mut reader).map_err(|_| -1i64)?;

            if sym == eob {
                break;
            }

            if sym == 0 || sym == 1 {
                // RUNA or RUNB - decode zero run
                // Collect consecutive RUNA/RUNB symbols into a single run
                let mut run_len = 0u32;
                let mut power = 1u32;
                let mut s = sym;

                loop {
                    if s == 0 {
                        run_len += power; // RUNA
                    } else {
                        run_len += 2 * power; // RUNB
                    }
                    power <<= 1;

                    // Count this symbol
                    group_pos += 1;
                    if group_pos >= 50 {
                        group_pos = 0;
                        group_idx += 1;
                    }

                    // Peek at next symbol to see if the run continues
                    let next_table_idx = if group_idx < selectors.len() {
                        selectors[group_idx] as usize
                    } else {
                        break;
                    };

                    let saved_pos = reader.pos;
                    let saved_bit = reader.bit;
                    let saved_current = reader.current;

                    match tables[next_table_idx].decode(&mut reader) {
                        Ok(next_sym) if next_sym == 0 || next_sym == 1 => {
                            s = next_sym;
                            // Continue the run
                        }
                        Ok(_) => {
                            // Not part of the run, restore reader state
                            reader.pos = saved_pos;
                            reader.bit = saved_bit;
                            reader.current = saved_current;
                            break;
                        }
                        Err(_) => {
                            reader.pos = saved_pos;
                            reader.bit = saved_bit;
                            reader.current = saved_current;
                            break;
                        }
                    }
                }

                // Emit run_len zeros (MTF index 0)
                for _ in 0..run_len {
                    decoded_mtf.push(0);
                }
            } else {
                // Regular symbol (shifted by 1 due to RUNA/RUNB at 0,1)
                decoded_mtf.push(sym - 1);

                // Count this symbol
                group_pos += 1;
                if group_pos >= 50 {
                    group_pos = 0;
                    group_idx += 1;
                }
            }
        }

        // Undo MTF
        let bwt_data = mtf_decode(&decoded_mtf, &used_symbols);

        // Validate orig_ptr
        if orig_ptr as usize >= bwt_data.len() && !bwt_data.is_empty() {
            return Err(-1);
        }

        // Undo BWT
        let rle1_data = bwt_decode(&bwt_data, orig_ptr);

        // Undo RLE1
        let block_data = rle1_decode(&rle1_data);

        // Verify block CRC
        let computed_crc = crc32_update(0xFFFFFFFF, &block_data) ^ 0xFFFFFFFF;
        if computed_crc != stored_block_crc {
            return Err(-4); // BZ_DATA_ERROR
        }

        // Update combined CRC
        combined_crc = (combined_crc.wrapping_shl(1) | combined_crc.wrapping_shr(31)) ^ computed_crc;

        output.extend_from_slice(&block_data);
    }

    Ok(output)
}
