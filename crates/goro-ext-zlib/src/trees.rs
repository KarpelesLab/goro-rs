//! Huffman coding of deflate blocks (RFC 1951 section 3.2).
//!
//! The literal/length and distance symbols of a block are buffered by the
//! match finder, then coded with whichever of a stored, fixed-code or
//! dynamic-code block is smallest. PHP's output is compared byte for byte by
//! its tests, so the code lengths are built the way PHP 8.5's output shows
//! (observed on PHP 8.5 against tests/fixtures/deflate_php85.txt): a binary
//! heap ordered by frequency then subtree depth, lengths limited by moving
//! overflowing leaves down, and the code-length alphabet coded with the
//! repeat codes 16/17/18 in runs of at most 6/10/138.

pub(crate) const LENGTH_CODES: usize = 29;
pub(crate) const LITERALS: usize = 256;
pub(crate) const L_CODES: usize = LITERALS + 1 + LENGTH_CODES;
pub(crate) const D_CODES: usize = 30;
pub(crate) const BL_CODES: usize = 19;
const HEAP_SIZE: usize = 2 * L_CODES + 1;
const MAX_BITS: usize = 15;
const MAX_BL_BITS: usize = 7;
const END_BLOCK: usize = 256;
const REP_3_6: usize = 16;
const REPZ_3_10: usize = 17;
const REPZ_11_138: usize = 18;

const EXTRA_LBITS: [u8; LENGTH_CODES] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const EXTRA_DBITS: [u8; D_CODES] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const EXTRA_BLBITS: [u8; BL_CODES] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 7];
/// Order in which the code-length code lengths are sent (RFC 1951 3.2.7).
const BL_ORDER: [usize; BL_CODES] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Lookup tables derived from RFC 1951's length and distance code ranges.
pub(crate) struct Tables {
    /// Length code (0..29) for each match length minus 3.
    pub length_code: [u8; 256],
    pub base_length: [u8; LENGTH_CODES],
    /// Distance code for distance-1 < 256, then for (distance-1) >> 7.
    pub dist_code: [u8; 512],
    pub base_dist: [u16; D_CODES],
    pub static_ltree_code: [u16; L_CODES + 2],
    pub static_ltree_len: [u8; L_CODES + 2],
    pub static_dtree_code: [u16; D_CODES],
}

const fn bi_reverse(mut code: u32, len: u32) -> u32 {
    let mut res = 0;
    let mut i = 0;
    while i < len {
        res = (res << 1) | (code & 1);
        code >>= 1;
        i += 1;
    }
    res
}

const fn build_tables() -> Tables {
    let mut length_code = [0u8; 256];
    let mut base_length = [0u8; LENGTH_CODES];
    let mut length = 0usize;
    let mut code = 0;
    while code < LENGTH_CODES - 1 {
        base_length[code] = length as u8;
        let mut n = 0;
        while n < (1 << EXTRA_LBITS[code]) {
            length_code[length] = code as u8;
            length += 1;
            n += 1;
        }
        code += 1;
    }
    // Length 258 has its own code (285) even though 284 could reach it.
    length_code[255] = (LENGTH_CODES - 1) as u8;
    base_length[LENGTH_CODES - 1] = 255;

    let mut dist_code = [0u8; 512];
    let mut base_dist = [0u16; D_CODES];
    let mut dist = 0usize;
    code = 0;
    while code < 16 {
        base_dist[code] = dist as u16;
        let mut n = 0;
        while n < (1 << EXTRA_DBITS[code]) {
            dist_code[dist] = code as u8;
            dist += 1;
            n += 1;
        }
        code += 1;
    }
    dist >>= 7;
    while code < D_CODES {
        base_dist[code] = (dist << 7) as u16;
        let mut n = 0;
        while n < (1 << (EXTRA_DBITS[code] - 7)) {
            dist_code[256 + dist] = code as u8;
            dist += 1;
            n += 1;
        }
        code += 1;
    }

    // Fixed codes (RFC 1951 3.2.6).
    let mut static_ltree_len = [0u8; L_CODES + 2];
    let mut bl_count = [0u32; MAX_BITS + 1];
    let mut n = 0;
    while n < L_CODES + 2 {
        let l = if n <= 143 {
            8
        } else if n <= 255 {
            9
        } else if n <= 279 {
            7
        } else {
            8
        };
        static_ltree_len[n] = l;
        bl_count[l as usize] += 1;
        n += 1;
    }
    let mut next_code = [0u32; MAX_BITS + 1];
    let mut c = 0u32;
    let mut bits = 1;
    while bits <= MAX_BITS {
        c = (c + bl_count[bits - 1]) << 1;
        next_code[bits] = c;
        bits += 1;
    }
    let mut static_ltree_code = [0u16; L_CODES + 2];
    n = 0;
    while n < L_CODES + 2 {
        let l = static_ltree_len[n] as usize;
        static_ltree_code[n] = bi_reverse(next_code[l], l as u32) as u16;
        next_code[l] += 1;
        n += 1;
    }
    let mut static_dtree_code = [0u16; D_CODES];
    n = 0;
    while n < D_CODES {
        static_dtree_code[n] = bi_reverse(n as u32, 5) as u16;
        n += 1;
    }
    Tables {
        length_code,
        base_length,
        dist_code,
        base_dist,
        static_ltree_code,
        static_ltree_len,
        static_dtree_code,
    }
}

pub(crate) static TABLES: Tables = build_tables();

#[inline(always)]
pub(crate) fn d_code(dist: usize) -> usize {
    if dist < 256 {
        TABLES.dist_code[dist] as usize
    } else {
        TABLES.dist_code[256 + (dist >> 7)] as usize
    }
}

/// LSB-first bit writer (RFC 1951 3.1.1) over a byte vector.
pub(crate) struct BitWriter {
    pub out: Vec<u8>,
    buf: u64,
    nbits: u32,
}

impl BitWriter {
    pub fn new() -> Self {
        BitWriter {
            out: Vec::new(),
            buf: 0,
            nbits: 0,
        }
    }

    #[inline(always)]
    pub fn send(&mut self, value: u32, len: u32) {
        self.buf |= (value as u64) << self.nbits;
        self.nbits += len;
        if self.nbits >= 32 {
            self.out.extend_from_slice(&(self.buf as u32).to_le_bytes());
            self.buf >>= 32;
            self.nbits -= 32;
        }
    }

    /// Writes out the whole bytes of the bit buffer, keeping at most 7 bits.
    pub fn flush_bits(&mut self) {
        while self.nbits >= 8 {
            self.out.push(self.buf as u8);
            self.buf >>= 8;
            self.nbits -= 8;
        }
    }

    /// Pads to a byte boundary and writes everything out.
    pub fn windup(&mut self) {
        self.flush_bits();
        if self.nbits > 0 {
            self.out.push(self.buf as u8);
        }
        self.buf = 0;
        self.nbits = 0;
    }

    pub fn pending_bits(&self) -> u32 {
        self.nbits
    }

    pub fn reset(&mut self) {
        self.out.clear();
        self.buf = 0;
        self.nbits = 0;
    }
}

/// One Huffman tree under construction: `n` leaves plus internal nodes.
#[derive(Clone)]
struct Tree<const N: usize> {
    freq: [u32; N],
    len: [u16; N],
    code: [u16; N],
    dad: [u16; N],
    max_code: usize,
}

impl<const N: usize> Tree<N> {
    fn new() -> Self {
        Tree {
            freq: [0; N],
            len: [0; N],
            code: [0; N],
            dad: [0; N],
            max_code: 0,
        }
    }
}

/// Which alphabet a tree codes, for its extra bits and static lengths.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Lit,
    Dist,
    Bl,
}

/// Block state: symbol frequencies, scratch space for tree building, and
/// the cost estimates of the candidate block types.
pub(crate) struct Trees {
    ltree: Box<Tree<HEAP_SIZE>>,
    dtree: Box<Tree<{ 2 * D_CODES + 1 }>>,
    bltree: Box<Tree<{ 2 * BL_CODES + 1 }>>,
    heap: [u16; HEAP_SIZE],
    heap_len: usize,
    heap_max: usize,
    depth: [u8; HEAP_SIZE],
    bl_count: [u16; MAX_BITS + 1],
    /// Bit length of the block with dynamic trees (including tree headers).
    opt_len: u64,
    /// Bit length of the block with the fixed trees.
    static_len: u64,
    /// Buffered symbols: `dist << 8 | lc`, dist 0 for literals.
    pub syms: Vec<u32>,
    pub sym_limit: usize,
}

impl Trees {
    pub fn new(lit_bufsize: usize) -> Self {
        let mut t = Trees {
            ltree: Box::new(Tree::new()),
            dtree: Box::new(Tree::new()),
            bltree: Box::new(Tree::new()),
            heap: [0; HEAP_SIZE],
            heap_len: 0,
            heap_max: 0,
            depth: [0; HEAP_SIZE],
            bl_count: [0; MAX_BITS + 1],
            opt_len: 0,
            static_len: 0,
            syms: Vec::with_capacity(lit_bufsize),
            sym_limit: lit_bufsize - 1,
        };
        t.init_block();
        t
    }

    pub fn init_block(&mut self) {
        self.ltree.freq[..L_CODES].fill(0);
        self.dtree.freq[..D_CODES].fill(0);
        self.bltree.freq[..BL_CODES].fill(0);
        self.ltree.freq[END_BLOCK] = 1;
        self.opt_len = 0;
        self.static_len = 0;
        self.syms.clear();
    }

    /// Records a literal; true when the block is full.
    #[inline(always)]
    pub fn tally_lit(&mut self, c: u8) -> bool {
        self.syms.push(c as u32);
        self.ltree.freq[c as usize] += 1;
        self.syms.len() == self.sym_limit
    }

    /// Records a match of `len + 3` bytes at `dist`; true when the block is
    /// full.
    #[inline(always)]
    pub fn tally_dist(&mut self, dist: usize, len: usize) -> bool {
        self.syms.push(((dist as u32) << 8) | len as u32);
        self.ltree.freq[TABLES.length_code[len] as usize + LITERALS + 1] += 1;
        self.dtree.freq[d_code(dist - 1)] += 1;
        self.syms.len() == self.sym_limit
    }

    /// Emits the buffered block; `stored` is the block's input when it is
    /// still in the window (a stored block is only possible then).
    pub fn flush_block(
        &mut self,
        bw: &mut BitWriter,
        stored: Option<&[u8]>,
        stored_len: usize,
        last: bool,
        level: i32,
        fixed: bool,
    ) {
        let (opt_lenb, static_lenb, max_blindex);
        if level > 0 {
            self.build_tree(Kind::Lit);
            self.build_tree(Kind::Dist);
            max_blindex = self.build_bl_tree();
            let o = self.opt_len.wrapping_add(3 + 7) >> 3;
            let s = self.static_len.wrapping_add(3 + 7) >> 3;
            static_lenb = s;
            opt_lenb = if s <= o || fixed { s } else { o };
        } else {
            max_blindex = 0;
            opt_lenb = stored_len as u64 + 5;
            static_lenb = opt_lenb;
        }
        match stored {
            Some(buf) if stored_len as u64 + 4 <= opt_lenb => {
                stored_block(bw, buf, last);
            }
            _ if static_lenb == opt_lenb => {
                bw.send(2 + last as u32, 3);
                self.compress_block(bw, true);
            }
            _ => {
                bw.send(4 + last as u32, 3);
                self.send_all_trees(
                    bw,
                    self.ltree.max_code + 1,
                    self.dtree.max_code + 1,
                    max_blindex + 1,
                );
                self.compress_block(bw, false);
            }
        }
        self.init_block();
        if last {
            bw.windup();
        }
    }

    fn smaller(freq: &[u32], depth: &[u8], n: usize, m: usize) -> bool {
        freq[n] < freq[m] || (freq[n] == freq[m] && depth[n] <= depth[m])
    }

    fn pqdownheap(&mut self, freq: &[u32], mut k: usize) {
        let v = self.heap[k] as usize;
        let mut j = k << 1;
        while j <= self.heap_len {
            if j < self.heap_len
                && Self::smaller(
                    freq,
                    &self.depth,
                    self.heap[j + 1] as usize,
                    self.heap[j] as usize,
                )
            {
                j += 1;
            }
            if Self::smaller(freq, &self.depth, v, self.heap[j] as usize) {
                break;
            }
            self.heap[k] = self.heap[j];
            k = j;
            j <<= 1;
        }
        self.heap[k] = v as u16;
    }

    fn build_tree(&mut self, kind: Kind) {
        // Work on a detached tree to keep the borrow checker out of the heap
        // routines; the trees are small.
        match kind {
            Kind::Lit => {
                let mut t = std::mem::replace(&mut self.ltree, Box::new(Tree::new()));
                self.build_tree_on(
                    &mut t.freq,
                    &mut t.len,
                    &mut t.dad,
                    &mut t.max_code,
                    L_CODES,
                    kind,
                );
                gen_codes(&mut t.code, &t.len, t.max_code, &self.bl_count);
                self.ltree = t;
            }
            Kind::Dist => {
                let mut t = std::mem::replace(&mut self.dtree, Box::new(Tree::new()));
                self.build_tree_on(
                    &mut t.freq,
                    &mut t.len,
                    &mut t.dad,
                    &mut t.max_code,
                    D_CODES,
                    kind,
                );
                gen_codes(&mut t.code, &t.len, t.max_code, &self.bl_count);
                self.dtree = t;
            }
            Kind::Bl => {
                let mut t = std::mem::replace(&mut self.bltree, Box::new(Tree::new()));
                self.build_tree_on(
                    &mut t.freq,
                    &mut t.len,
                    &mut t.dad,
                    &mut t.max_code,
                    BL_CODES,
                    kind,
                );
                gen_codes(&mut t.code, &t.len, t.max_code, &self.bl_count);
                self.bltree = t;
            }
        }
    }

    fn build_tree_on(
        &mut self,
        freq: &mut [u32],
        len: &mut [u16],
        dad: &mut [u16],
        max_code_out: &mut usize,
        elems: usize,
        kind: Kind,
    ) {
        let mut max_code: isize = -1;
        self.heap_len = 0;
        self.heap_max = HEAP_SIZE;
        for n in 0..elems {
            if freq[n] != 0 {
                self.heap_len += 1;
                self.heap[self.heap_len] = n as u16;
                max_code = n as isize;
                self.depth[n] = 0;
            } else {
                len[n] = 0;
            }
        }
        // A code needs at least two symbols, so add dummy ones.
        while self.heap_len < 2 {
            let node = if max_code < 2 {
                max_code += 1;
                max_code as usize
            } else {
                0
            };
            self.heap_len += 1;
            self.heap[self.heap_len] = node as u16;
            freq[node] = 1;
            self.depth[node] = 0;
            self.opt_len = self.opt_len.wrapping_sub(1);
            if let Some(sl) = static_len_of(kind, node) {
                self.static_len = self.static_len.wrapping_sub(sl as u64);
            }
        }
        let max_code = max_code as usize;
        *max_code_out = max_code;

        let mut n = self.heap_len / 2;
        while n >= 1 {
            self.pqdownheap(freq, n);
            n -= 1;
        }

        let mut node = elems;
        loop {
            // Take the two least frequent nodes and join them.
            let n = self.heap[1] as usize;
            self.heap[1] = self.heap[self.heap_len];
            self.heap_len -= 1;
            self.pqdownheap(freq, 1);
            let m = self.heap[1] as usize;
            self.heap_max -= 1;
            self.heap[self.heap_max] = n as u16;
            self.heap_max -= 1;
            self.heap[self.heap_max] = m as u16;
            freq[node] = freq[n] + freq[m];
            self.depth[node] = self.depth[n].max(self.depth[m]) + 1;
            dad[n] = node as u16;
            dad[m] = node as u16;
            self.heap[1] = node as u16;
            node += 1;
            self.pqdownheap(freq, 1);
            if self.heap_len < 2 {
                break;
            }
        }
        self.heap_max -= 1;
        self.heap[self.heap_max] = self.heap[1];

        self.gen_bitlen(freq, len, dad, max_code, kind);
    }

    fn gen_bitlen(
        &mut self,
        freq: &[u32],
        len: &mut [u16],
        dad: &[u16],
        max_code: usize,
        kind: Kind,
    ) {
        let max_length = if kind == Kind::Bl {
            MAX_BL_BITS
        } else {
            MAX_BITS
        };
        self.bl_count = [0; MAX_BITS + 1];
        len[self.heap[self.heap_max] as usize] = 0;
        let mut overflow = 0i32;
        let mut h = self.heap_max + 1;
        while h < HEAP_SIZE {
            let n = self.heap[h] as usize;
            let mut bits = len[dad[n] as usize] as usize + 1;
            if bits > max_length {
                bits = max_length;
                overflow += 1;
            }
            len[n] = bits as u16;
            h += 1;
            if n > max_code {
                continue;
            }
            self.bl_count[bits] += 1;
            let xbits = extra_bits(kind, n);
            let f = freq[n] as u64;
            self.opt_len = self.opt_len.wrapping_add(f * (bits as u64 + xbits as u64));
            if let Some(sl) = static_len_of(kind, n) {
                self.static_len = self.static_len.wrapping_add(f * (sl as u64 + xbits as u64));
            }
        }
        if overflow == 0 {
            return;
        }
        // Find the first bit length that could increase, move a leaf from
        // the overflowing level next to it.
        loop {
            let mut bits = max_length - 1;
            while self.bl_count[bits] == 0 {
                bits -= 1;
            }
            self.bl_count[bits] -= 1;
            self.bl_count[bits + 1] += 2;
            self.bl_count[max_length] -= 1;
            overflow -= 2;
            if overflow <= 0 {
                break;
            }
        }
        // Reassign lengths to the leaves in order of increasing frequency.
        let mut h = HEAP_SIZE;
        let mut bits = max_length;
        while bits != 0 {
            let mut n = self.bl_count[bits];
            while n != 0 {
                h -= 1;
                let m = self.heap[h] as usize;
                if m > max_code {
                    continue;
                }
                if len[m] as usize != bits {
                    let delta = bits as i64 - len[m] as i64;
                    self.opt_len =
                        (self.opt_len as i64).wrapping_add(delta * freq[m] as i64) as u64;
                    len[m] = bits as u16;
                }
                n -= 1;
            }
            bits -= 1;
        }
    }

    /// Counts the code-length symbols needed to send `tree`'s lengths.
    fn scan_tree(&mut self, which: Kind) {
        let (len, max_code): (&mut [u16], usize) = match which {
            Kind::Lit => (&mut self.ltree.len[..], self.ltree.max_code),
            _ => (&mut self.dtree.len[..], self.dtree.max_code),
        };
        let blfreq = &mut self.bltree.freq;
        let mut prevlen: i32 = -1;
        let mut nextlen = len[0] as i32;
        let mut count = 0;
        let (mut max_count, mut min_count) = if nextlen == 0 { (138, 3) } else { (7, 4) };
        len[max_code + 1] = 0xffff; // guard
        for n in 0..=max_code {
            let curlen = nextlen;
            nextlen = len[n + 1] as i32;
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            } else if count < min_count {
                blfreq[curlen as usize] += count as u32;
            } else if curlen != 0 {
                if curlen != prevlen {
                    blfreq[curlen as usize] += 1;
                }
                blfreq[REP_3_6] += 1;
            } else if count <= 10 {
                blfreq[REPZ_3_10] += 1;
            } else {
                blfreq[REPZ_11_138] += 1;
            }
            count = 0;
            prevlen = curlen;
            (max_count, min_count) = if nextlen == 0 {
                (138, 3)
            } else if curlen == nextlen {
                (6, 3)
            } else {
                (7, 4)
            };
        }
    }

    fn send_tree(&self, bw: &mut BitWriter, which: Kind, max_code: usize) {
        let len: &[u16] = match which {
            Kind::Lit => &self.ltree.len[..],
            _ => &self.dtree.len[..],
        };
        let bl = &self.bltree;
        let send_code = |bw: &mut BitWriter, c: usize| bw.send(bl.code[c] as u32, bl.len[c] as u32);
        let mut prevlen: i32 = -1;
        let mut nextlen = len[0] as i32;
        let mut count = 0;
        let (mut max_count, mut min_count) = if nextlen == 0 { (138, 3) } else { (7, 4) };
        for n in 0..=max_code {
            let curlen = nextlen;
            nextlen = len[n + 1] as i32;
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            } else if count < min_count {
                while count != 0 {
                    send_code(bw, curlen as usize);
                    count -= 1;
                }
            } else if curlen != 0 {
                if curlen != prevlen {
                    send_code(bw, curlen as usize);
                    count -= 1;
                }
                send_code(bw, REP_3_6);
                bw.send(count as u32 - 3, 2);
            } else if count <= 10 {
                send_code(bw, REPZ_3_10);
                bw.send(count as u32 - 3, 3);
            } else {
                send_code(bw, REPZ_11_138);
                bw.send(count as u32 - 11, 7);
            }
            count = 0;
            prevlen = curlen;
            (max_count, min_count) = if nextlen == 0 {
                (138, 3)
            } else if curlen == nextlen {
                (6, 3)
            } else {
                (7, 4)
            };
        }
    }

    fn build_bl_tree(&mut self) -> usize {
        self.scan_tree(Kind::Lit);
        self.scan_tree(Kind::Dist);
        self.build_tree(Kind::Bl);
        let mut max_blindex = BL_CODES - 1;
        while max_blindex >= 3 {
            if self.bltree.len[BL_ORDER[max_blindex]] != 0 {
                break;
            }
            max_blindex -= 1;
        }
        self.opt_len = self
            .opt_len
            .wrapping_add(3 * (max_blindex as u64 + 1) + 5 + 5 + 4);
        max_blindex
    }

    fn send_all_trees(&self, bw: &mut BitWriter, lcodes: usize, dcodes: usize, blcodes: usize) {
        bw.send((lcodes - 257) as u32, 5);
        bw.send((dcodes - 1) as u32, 5);
        bw.send((blcodes - 4) as u32, 4);
        for &o in &BL_ORDER[..blcodes] {
            bw.send(self.bltree.len[o] as u32, 3);
        }
        self.send_tree(bw, Kind::Lit, lcodes - 1);
        self.send_tree(bw, Kind::Dist, dcodes - 1);
    }

    fn compress_block(&self, bw: &mut BitWriter, fixed: bool) {
        let t = &TABLES;
        let (lcode, llen, dcode, dlen): (&[u16], &[u8], &[u16], &[u8]);
        let mut llen_buf = [0u8; L_CODES + 2];
        let mut dlen_buf = [0u8; D_CODES];
        if fixed {
            lcode = &t.static_ltree_code;
            llen = &t.static_ltree_len;
            dcode = &t.static_dtree_code;
            dlen_buf.fill(5);
            dlen = &dlen_buf;
        } else {
            lcode = &self.ltree.code[..];
            for (d, s) in llen_buf.iter_mut().zip(self.ltree.len.iter()) {
                *d = *s as u8;
            }
            llen = &llen_buf;
            dcode = &self.dtree.code[..];
            for (d, s) in dlen_buf.iter_mut().zip(self.dtree.len.iter()) {
                *d = *s as u8;
            }
            dlen = &dlen_buf;
        }
        for &sym in &self.syms {
            let dist = (sym >> 8) as usize;
            let lc = (sym & 0xff) as usize;
            if dist == 0 {
                bw.send(lcode[lc] as u32, llen[lc] as u32);
            } else {
                let code = t.length_code[lc] as usize;
                let lsym = code + LITERALS + 1;
                // Length code, its extra bits, distance code and its extra
                // bits fit in 15 + 5 + 15 + 13 bits: write them in two sends.
                let extra = EXTRA_LBITS[code] as u32;
                let mut v = lcode[lsym] as u32;
                let mut n = llen[lsym] as u32;
                if extra != 0 {
                    v |= ((lc - t.base_length[code] as usize) as u32) << n;
                    n += extra;
                }
                bw.send(v, n);
                let d = dist - 1;
                let dc = d_code(d);
                let mut v = dcode[dc] as u32;
                let mut n = dlen[dc] as u32;
                let extra = EXTRA_DBITS[dc] as u32;
                if extra != 0 {
                    v |= ((d - t.base_dist[dc] as usize) as u32) << n;
                    n += extra;
                }
                bw.send(v, n);
            }
        }
        bw.send(lcode[END_BLOCK] as u32, llen[END_BLOCK] as u32);
    }
}

fn extra_bits(kind: Kind, n: usize) -> u32 {
    match kind {
        Kind::Lit => {
            if n > LITERALS {
                EXTRA_LBITS[n - LITERALS - 1] as u32
            } else {
                0
            }
        }
        Kind::Dist => EXTRA_DBITS[n] as u32,
        Kind::Bl => EXTRA_BLBITS[n] as u32,
    }
}

fn static_len_of(kind: Kind, n: usize) -> Option<u32> {
    match kind {
        Kind::Lit => Some(TABLES.static_ltree_len[n] as u32),
        Kind::Dist => Some(5),
        Kind::Bl => None,
    }
}

fn gen_codes(code: &mut [u16], len: &[u16], max_code: usize, bl_count: &[u16; MAX_BITS + 1]) {
    let mut next_code = [0u32; MAX_BITS + 1];
    let mut c = 0u32;
    for bits in 1..=MAX_BITS {
        c = (c + bl_count[bits - 1] as u32) << 1;
        next_code[bits] = c;
    }
    for n in 0..=max_code {
        let l = len[n] as usize;
        if l == 0 {
            continue;
        }
        code[n] = bi_reverse(next_code[l], l as u32) as u16;
        next_code[l] += 1;
    }
}

/// Writes a stored block (RFC 1951 3.2.4) holding `buf`.
pub(crate) fn stored_block(bw: &mut BitWriter, buf: &[u8], last: bool) {
    bw.send(last as u32, 3);
    bw.windup();
    let len = buf.len() as u16;
    bw.out.extend_from_slice(&len.to_le_bytes());
    bw.out.extend_from_slice(&(!len).to_le_bytes());
    bw.out.extend_from_slice(buf);
}

/// The empty fixed-code block used by a partial flush.
pub(crate) fn align(bw: &mut BitWriter) {
    bw.send(2, 3);
    bw.send(
        TABLES.static_ltree_code[END_BLOCK] as u32,
        TABLES.static_ltree_len[END_BLOCK] as u32,
    );
    bw.flush_bits();
}
