//! deflate.c + trees.c from zlib 1.3.1 (non-FASTEST, non-LIT_MEM build).
//!
//! The port keeps zlib's data layout where it influences the output: the
//! symbol buffer overlays the pending buffer, the window is 2*w_size, hash
//! chains use 16-bit positions, and buffers sizes depend on memLevel.

use crate::checksum::{adler32, crc32};
use crate::*;

const LENGTH_CODES: usize = 29;
const LITERALS: usize = 256;
const L_CODES: usize = LITERALS + 1 + LENGTH_CODES;
const D_CODES: usize = 30;
const BL_CODES: usize = 19;
const HEAP_SIZE: usize = 2 * L_CODES + 1;
const MAX_BITS: usize = 15;
const MAX_BL_BITS: i32 = 7;
const END_BLOCK: usize = 256;
const REP_3_6: usize = 16;
const REPZ_3_10: usize = 17;
const REPZ_11_138: usize = 18;
const BUF_SIZE: i32 = 16;

const MIN_MATCH: u32 = 3;
const MAX_MATCH: u32 = 258;
const MIN_LOOKAHEAD: u32 = MAX_MATCH + MIN_MATCH + 1;
const WIN_INIT: usize = MAX_MATCH as usize;
const TOO_FAR: u32 = 4096;
const MAX_STORED: u32 = 65535;
const NIL: u32 = 0;
const PRESET_DICT: u32 = 0x20;

const STORED_BLOCK: u32 = 0;
const STATIC_TREES: u32 = 1;
const DYN_TREES: u32 = 2;

const INIT_STATE: i32 = 42;
const GZIP_STATE: i32 = 57;
const BUSY_STATE: i32 = 113;
const FINISH_STATE: i32 = 666;

const OS_CODE: u8 = 3;

const EXTRA_LBITS: [i32; LENGTH_CODES] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const EXTRA_DBITS: [i32; D_CODES] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const EXTRA_BLBITS: [i32; BL_CODES] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 7];
const BL_ORDER: [usize; BL_CODES] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// ct_data: `fc` is Freq/Code, `dl` is Dad/Len (unions in C).
#[derive(Clone, Copy, Default)]
struct Ct {
    fc: u16,
    dl: u16,
}

struct StaticTables {
    ltree: [Ct; L_CODES + 2],
    dtree: [Ct; D_CODES],
    dist_code: [u8; 512],
    length_code: [u8; 256],
    base_length: [i32; LENGTH_CODES],
    base_dist: [i32; D_CODES],
}

fn bi_reverse(mut code: u32, mut len: i32) -> u32 {
    let mut res = 0u32;
    loop {
        res |= code & 1;
        code >>= 1;
        res <<= 1;
        len -= 1;
        if len <= 0 {
            break;
        }
    }
    res >> 1
}

fn gen_codes(tree: &mut [Ct], max_code: i32, bl_count: &[u16; MAX_BITS + 1]) {
    let mut next_code = [0u16; MAX_BITS + 1];
    let mut code: u32 = 0;
    for bits in 1..=MAX_BITS {
        code = (code + bl_count[bits - 1] as u32) << 1;
        next_code[bits] = code as u16;
    }
    for n in 0..=(max_code.max(-1)) {
        if n < 0 {
            break;
        }
        let len = tree[n as usize].dl as usize;
        if len == 0 {
            continue;
        }
        tree[n as usize].fc = bi_reverse(next_code[len] as u32, len as i32) as u16;
        next_code[len] = next_code[len].wrapping_add(1);
    }
}

fn tables() -> &'static StaticTables {
    static T: std::sync::OnceLock<StaticTables> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = StaticTables {
            ltree: [Ct::default(); L_CODES + 2],
            dtree: [Ct::default(); D_CODES],
            dist_code: [0; 512],
            length_code: [0; 256],
            base_length: [0; LENGTH_CODES],
            base_dist: [0; D_CODES],
        };
        let mut length = 0usize;
        let mut code = 0usize;
        while code < LENGTH_CODES - 1 {
            t.base_length[code] = length as i32;
            for _ in 0..(1 << EXTRA_LBITS[code]) {
                t.length_code[length] = code as u8;
                length += 1;
            }
            code += 1;
        }
        t.length_code[length - 1] = code as u8;
        let mut dist = 0usize;
        code = 0;
        while code < 16 {
            t.base_dist[code] = dist as i32;
            for _ in 0..(1 << EXTRA_DBITS[code]) {
                t.dist_code[dist] = code as u8;
                dist += 1;
            }
            code += 1;
        }
        dist >>= 7;
        while code < D_CODES {
            t.base_dist[code] = (dist << 7) as i32;
            for _ in 0..(1 << (EXTRA_DBITS[code] - 7)) {
                t.dist_code[256 + dist] = code as u8;
                dist += 1;
            }
            code += 1;
        }
        let mut bl_count = [0u16; MAX_BITS + 1];
        let mut n = 0;
        while n <= 143 {
            t.ltree[n].dl = 8;
            bl_count[8] += 1;
            n += 1;
        }
        while n <= 255 {
            t.ltree[n].dl = 9;
            bl_count[9] += 1;
            n += 1;
        }
        while n <= 279 {
            t.ltree[n].dl = 7;
            bl_count[7] += 1;
            n += 1;
        }
        while n <= 287 {
            t.ltree[n].dl = 8;
            bl_count[8] += 1;
            n += 1;
        }
        gen_codes(&mut t.ltree, (L_CODES + 1) as i32, &bl_count);
        for n in 0..D_CODES {
            t.dtree[n].dl = 5;
            t.dtree[n].fc = bi_reverse(n as u32, 5) as u16;
        }
        t
    })
}

#[inline]
fn d_code(t: &StaticTables, dist: u32) -> usize {
    if dist < 256 {
        t.dist_code[dist as usize] as usize
    } else {
        t.dist_code[256 + (dist as usize >> 7)] as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TreeKind {
    L,
    D,
    Bl,
}

struct StaticDesc {
    stree: Option<&'static [Ct]>,
    extra_bits: &'static [i32],
    extra_base: usize,
    elems: usize,
    max_length: i32,
}

fn static_desc(kind: TreeKind) -> StaticDesc {
    let t = tables();
    match kind {
        TreeKind::L => StaticDesc {
            stree: Some(&t.ltree[..]),
            extra_bits: &EXTRA_LBITS,
            extra_base: LITERALS + 1,
            elems: L_CODES,
            max_length: MAX_BITS as i32,
        },
        TreeKind::D => StaticDesc {
            stree: Some(&t.dtree[..]),
            extra_bits: &EXTRA_DBITS,
            extra_base: 0,
            elems: D_CODES,
            max_length: MAX_BITS as i32,
        },
        TreeKind::Bl => StaticDesc {
            stree: None,
            extra_bits: &EXTRA_BLBITS,
            extra_base: 0,
            elems: BL_CODES,
            max_length: MAX_BL_BITS,
        },
    }
}

/// Heap / bit-length bookkeeping of trees.c (separate from the trees so
/// both can be borrowed at once).
struct HeapState {
    bl_count: [u16; MAX_BITS + 1],
    heap: [i32; 2 * L_CODES + 1],
    heap_len: i32,
    heap_max: i32,
    depth: [u8; 2 * L_CODES + 1],
    opt_len: u64,
    static_len: u64,
}

#[inline]
fn smaller(tree: &[Ct], n: i32, m: i32, depth: &[u8]) -> bool {
    let (n, m) = (n as usize, m as usize);
    tree[n].fc < tree[m].fc || (tree[n].fc == tree[m].fc && depth[n] <= depth[m])
}

impl HeapState {
    fn pqdownheap(&mut self, tree: &[Ct], mut k: i32) {
        let v = self.heap[k as usize];
        let mut j = k << 1;
        while j <= self.heap_len {
            if j < self.heap_len
                && smaller(
                    tree,
                    self.heap[(j + 1) as usize],
                    self.heap[j as usize],
                    &self.depth,
                )
            {
                j += 1;
            }
            if smaller(tree, v, self.heap[j as usize], &self.depth) {
                break;
            }
            self.heap[k as usize] = self.heap[j as usize];
            k = j;
            j <<= 1;
        }
        self.heap[k as usize] = v;
    }

    fn gen_bitlen(&mut self, tree: &mut [Ct], max_code: i32, desc: &StaticDesc) {
        let stree = desc.stree;
        let extra = desc.extra_bits;
        let base = desc.extra_base as i32;
        let max_length = desc.max_length;
        let mut overflow = 0i32;
        for b in self.bl_count.iter_mut() {
            *b = 0;
        }
        tree[self.heap[self.heap_max as usize] as usize].dl = 0;
        let mut h = self.heap_max + 1;
        while (h as usize) < HEAP_SIZE {
            let n = self.heap[h as usize];
            let mut bits = tree[tree[n as usize].dl as usize].dl as i32 + 1;
            if bits > max_length {
                bits = max_length;
                overflow += 1;
            }
            tree[n as usize].dl = bits as u16;
            h += 1;
            if n > max_code {
                continue;
            }
            self.bl_count[bits as usize] += 1;
            let mut xbits = 0;
            if n >= base {
                xbits = extra[(n - base) as usize];
            }
            let f = tree[n as usize].fc as u64;
            self.opt_len = self.opt_len.wrapping_add(f * (bits + xbits) as u64);
            if let Some(st) = stree {
                self.static_len = self
                    .static_len
                    .wrapping_add(f * (st[n as usize].dl as i32 + xbits) as u64);
            }
        }
        if overflow == 0 {
            return;
        }
        loop {
            let mut bits = max_length - 1;
            while self.bl_count[bits as usize] == 0 {
                bits -= 1;
            }
            self.bl_count[bits as usize] -= 1;
            self.bl_count[(bits + 1) as usize] += 2;
            self.bl_count[max_length as usize] -= 1;
            overflow -= 2;
            if overflow <= 0 {
                break;
            }
        }
        let mut h = HEAP_SIZE as i32;
        let mut bits = max_length;
        while bits != 0 {
            let mut n = self.bl_count[bits as usize] as i32;
            while n != 0 {
                h -= 1;
                let m = self.heap[h as usize];
                if m > max_code {
                    continue;
                }
                if tree[m as usize].dl as i32 != bits {
                    self.opt_len = self.opt_len.wrapping_add(
                        ((bits as i64 - tree[m as usize].dl as i64) * tree[m as usize].fc as i64)
                            as u64,
                    );
                    tree[m as usize].dl = bits as u16;
                }
                n -= 1;
            }
            bits -= 1;
        }
    }

    /// build_tree(): returns max_code.
    fn build_tree(&mut self, tree: &mut [Ct], kind: TreeKind) -> i32 {
        let desc = static_desc(kind);
        let stree = desc.stree;
        let elems = desc.elems;
        let mut max_code: i32 = -1;
        self.heap_len = 0;
        self.heap_max = HEAP_SIZE as i32;
        for n in 0..elems {
            if tree[n].fc != 0 {
                self.heap_len += 1;
                self.heap[self.heap_len as usize] = n as i32;
                max_code = n as i32;
                self.depth[n] = 0;
            } else {
                tree[n].dl = 0;
            }
        }
        while self.heap_len < 2 {
            let node = if max_code < 2 {
                max_code += 1;
                max_code
            } else {
                0
            };
            self.heap_len += 1;
            self.heap[self.heap_len as usize] = node;
            tree[node as usize].fc = 1;
            self.depth[node as usize] = 0;
            self.opt_len = self.opt_len.wrapping_sub(1);
            if let Some(st) = stree {
                self.static_len = self.static_len.wrapping_sub(st[node as usize].dl as u64);
            }
        }
        let mut n = self.heap_len / 2;
        while n >= 1 {
            self.pqdownheap(tree, n);
            n -= 1;
        }
        let mut node = elems as i32;
        loop {
            // pqremove
            let n = self.heap[1];
            self.heap[1] = self.heap[self.heap_len as usize];
            self.heap_len -= 1;
            self.pqdownheap(tree, 1);
            let m = self.heap[1];
            self.heap_max -= 1;
            self.heap[self.heap_max as usize] = n;
            self.heap_max -= 1;
            self.heap[self.heap_max as usize] = m;
            tree[node as usize].fc = tree[n as usize].fc.wrapping_add(tree[m as usize].fc);
            self.depth[node as usize] = (if self.depth[n as usize] >= self.depth[m as usize] {
                self.depth[n as usize]
            } else {
                self.depth[m as usize]
            })
            .wrapping_add(1);
            tree[n as usize].dl = node as u16;
            tree[m as usize].dl = node as u16;
            self.heap[1] = node;
            node += 1;
            self.pqdownheap(tree, 1);
            if self.heap_len < 2 {
                break;
            }
        }
        self.heap_max -= 1;
        self.heap[self.heap_max as usize] = self.heap[1];
        self.gen_bitlen(tree, max_code, &desc);
        let bl_count = self.bl_count;
        gen_codes(tree, max_code, &bl_count);
        max_code
    }
}

/// Bit output into the pending buffer (which the symbol buffer overlays).
struct Pending {
    buf: Vec<u8>,
    pending: usize,
    pending_out: usize,
    bi_buf: u32,
    bi_valid: i32,
}

impl Pending {
    #[inline]
    fn put_byte(&mut self, c: u8) {
        self.buf[self.pending] = c;
        self.pending += 1;
    }
    #[inline]
    fn put_short(&mut self, w: u32) {
        self.put_byte((w & 0xff) as u8);
        self.put_byte(((w >> 8) & 0xff) as u8);
    }
    #[inline]
    fn put_short_msb(&mut self, b: u32) {
        self.put_byte(((b >> 8) & 0xff) as u8);
        self.put_byte((b & 0xff) as u8);
    }
    #[inline]
    fn send_bits(&mut self, value: u32, length: i32) {
        let value = value & 0xffff;
        if self.bi_valid > BUF_SIZE - length {
            self.bi_buf = (self.bi_buf | (value << self.bi_valid)) & 0xffff;
            let b = self.bi_buf;
            self.put_short(b);
            self.bi_buf = (value >> (BUF_SIZE - self.bi_valid)) & 0xffff;
            self.bi_valid += length - BUF_SIZE;
        } else {
            self.bi_buf = (self.bi_buf | (value << self.bi_valid)) & 0xffff;
            self.bi_valid += length;
        }
    }
    #[inline]
    fn send_code(&mut self, c: usize, tree: &[Ct]) {
        self.send_bits(tree[c].fc as u32, tree[c].dl as i32);
    }
    fn bi_flush(&mut self) {
        if self.bi_valid == 16 {
            let b = self.bi_buf;
            self.put_short(b);
            self.bi_buf = 0;
            self.bi_valid = 0;
        } else if self.bi_valid >= 8 {
            let b = self.bi_buf as u8;
            self.put_byte(b);
            self.bi_buf >>= 8;
            self.bi_valid -= 8;
        }
    }
    fn bi_windup(&mut self) {
        if self.bi_valid > 8 {
            let b = self.bi_buf;
            self.put_short(b);
        } else if self.bi_valid > 0 {
            let b = self.bi_buf as u8;
            self.put_byte(b);
        }
        self.bi_buf = 0;
        self.bi_valid = 0;
    }
    /// _tr_stored_block() with the block data `data`.
    fn stored_block(&mut self, data: &[u8], last: u32) {
        self.send_bits((STORED_BLOCK << 1) + last, 3);
        self.bi_windup();
        let len = data.len() as u32;
        self.put_short(len & 0xffff);
        self.put_short(!len & 0xffff);
        self.buf[self.pending..self.pending + data.len()].copy_from_slice(data);
        self.pending += data.len();
    }
    /// _tr_align()
    fn align(&mut self) {
        self.send_bits(STATIC_TREES << 1, 3);
        self.send_code(END_BLOCK, &tables().ltree);
        self.bi_flush();
    }
    fn send_tree(&mut self, tree: &[Ct], max_code: i32, bl_tree: &[Ct]) {
        let mut prevlen: i32 = -1;
        let mut nextlen = tree[0].dl as i32;
        let mut count = 0;
        let mut max_count = 7;
        let mut min_count = 4;
        if nextlen == 0 {
            max_count = 138;
            min_count = 3;
        }
        for n in 0..=max_code {
            let curlen = nextlen;
            nextlen = tree[(n + 1) as usize].dl as i32;
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            } else if count < min_count {
                loop {
                    self.send_code(curlen as usize, bl_tree);
                    count -= 1;
                    if count == 0 {
                        break;
                    }
                }
            } else if curlen != 0 {
                if curlen != prevlen {
                    self.send_code(curlen as usize, bl_tree);
                    count -= 1;
                }
                self.send_code(REP_3_6, bl_tree);
                self.send_bits((count - 3) as u32, 2);
            } else if count <= 10 {
                self.send_code(REPZ_3_10, bl_tree);
                self.send_bits((count - 3) as u32, 3);
            } else {
                self.send_code(REPZ_11_138, bl_tree);
                self.send_bits((count - 11) as u32, 7);
            }
            count = 0;
            prevlen = curlen;
            if nextlen == 0 {
                max_count = 138;
                min_count = 3;
            } else if curlen == nextlen {
                max_count = 6;
                min_count = 3;
            } else {
                max_count = 7;
                min_count = 4;
            }
        }
    }
    fn compress_block(&mut self, ltree: &[Ct], dtree: &[Ct], sym_off: usize, sym_next: usize) {
        let t = tables();
        let mut sx = 0usize;
        if sym_next != 0 {
            loop {
                let mut dist = self.buf[sym_off + sx] as u32;
                sx += 1;
                dist += (self.buf[sym_off + sx] as u32) << 8;
                sx += 1;
                let mut lc = self.buf[sym_off + sx] as i32;
                sx += 1;
                if dist == 0 {
                    self.send_code(lc as usize, ltree);
                } else {
                    let code = t.length_code[lc as usize] as usize;
                    self.send_code(code + LITERALS + 1, ltree);
                    let extra = EXTRA_LBITS[code];
                    if extra != 0 {
                        lc -= t.base_length[code];
                        self.send_bits(lc as u32, extra);
                    }
                    dist -= 1;
                    let code = d_code(t, dist);
                    self.send_code(code, dtree);
                    let extra = EXTRA_DBITS[code];
                    if extra != 0 {
                        dist = dist.wrapping_sub(t.base_dist[code] as u32);
                        self.send_bits(dist, extra);
                    }
                }
                if sx >= sym_next {
                    break;
                }
            }
        }
        self.send_code(END_BLOCK, ltree);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockState {
    NeedMore,
    BlockDone,
    FinishStarted,
    FinishDone,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Func {
    Stored,
    Fast,
    Slow,
}

struct Config {
    good_length: u16,
    max_lazy: u16,
    nice_length: u16,
    max_chain: u16,
    func: Func,
}

const CONFIGURATION_TABLE: [Config; 10] = [
    Config {
        good_length: 0,
        max_lazy: 0,
        nice_length: 0,
        max_chain: 0,
        func: Func::Stored,
    },
    Config {
        good_length: 4,
        max_lazy: 4,
        nice_length: 8,
        max_chain: 4,
        func: Func::Fast,
    },
    Config {
        good_length: 4,
        max_lazy: 5,
        nice_length: 16,
        max_chain: 8,
        func: Func::Fast,
    },
    Config {
        good_length: 4,
        max_lazy: 6,
        nice_length: 32,
        max_chain: 32,
        func: Func::Fast,
    },
    Config {
        good_length: 4,
        max_lazy: 4,
        nice_length: 16,
        max_chain: 16,
        func: Func::Slow,
    },
    Config {
        good_length: 8,
        max_lazy: 16,
        nice_length: 32,
        max_chain: 32,
        func: Func::Slow,
    },
    Config {
        good_length: 8,
        max_lazy: 16,
        nice_length: 128,
        max_chain: 128,
        func: Func::Slow,
    },
    Config {
        good_length: 8,
        max_lazy: 32,
        nice_length: 128,
        max_chain: 256,
        func: Func::Slow,
    },
    Config {
        good_length: 32,
        max_lazy: 128,
        nice_length: 258,
        max_chain: 1024,
        func: Func::Slow,
    },
    Config {
        good_length: 32,
        max_lazy: 258,
        nice_length: 258,
        max_chain: 4096,
        func: Func::Slow,
    },
];

#[inline]
fn rank(f: i32) -> i32 {
    f * 2 - if f > 4 { 9 } else { 0 }
}

/// A deflate stream: z_stream + deflate_state.
pub struct Deflate {
    pub total_in: u64,
    pub total_out: u64,
    pub adler: u32,
    pub data_type: i32,
    pub msg: Option<&'static str>,

    status: i32,
    pb: Pending,
    pending_buf_size: usize,
    wrap: i32,
    last_flush: i32,

    w_size: u32,
    w_bits: u32,
    w_mask: u32,
    window: Vec<u8>,
    window_size: u32,
    prev: Vec<u16>,
    head: Vec<u16>,
    ins_h: u32,
    hash_mask: u32,
    hash_shift: u32,
    block_start: i64,
    match_length: u32,
    prev_match: u32,
    match_available: bool,
    strstart: u32,
    match_start: u32,
    lookahead: u32,
    prev_length: u32,
    max_chain_length: u32,
    max_lazy_match: u32,
    level: i32,
    strategy: i32,
    good_match: u32,
    nice_match: i32,

    dyn_ltree: [Ct; HEAP_SIZE],
    dyn_dtree: [Ct; 2 * D_CODES + 1],
    bl_tree: [Ct; 2 * BL_CODES + 1],
    l_max_code: i32,
    d_max_code: i32,
    hs: HeapState,

    sym_off: usize,
    sym_next: u32,
    sym_end: u32,
    matches: u32,
    insert: u32,
    high_water: u32,
}

impl Deflate {
    /// deflateInit2(): `Err(code)` on bad parameters.
    pub fn new(
        level: i32,
        method: i32,
        window_bits: i32,
        mem_level: i32,
        strategy: i32,
    ) -> Result<Box<Deflate>, i32> {
        let mut level = level;
        let mut window_bits = window_bits;
        let mut wrap = 1;
        if level == Z_DEFAULT_COMPRESSION {
            level = 6;
        }
        if window_bits < 0 {
            wrap = 0;
            if window_bits < -15 {
                return Err(Z_STREAM_ERROR);
            }
            window_bits = -window_bits;
        } else if window_bits > 15 {
            wrap = 2;
            window_bits -= 16;
        }
        if !(1..=MAX_MEM_LEVEL).contains(&mem_level)
            || method != Z_DEFLATED
            || !(8..=15).contains(&window_bits)
            || !(0..=9).contains(&level)
            || !(0..=Z_FIXED).contains(&strategy)
            || (window_bits == 8 && wrap != 1)
        {
            return Err(Z_STREAM_ERROR);
        }
        if window_bits == 8 {
            window_bits = 9;
        }
        let w_bits = window_bits as u32;
        let w_size = 1u32 << w_bits;
        let hash_bits = mem_level as u32 + 7;
        let hash_size = 1u32 << hash_bits;
        let lit_bufsize = 1u32 << (mem_level + 6);
        let pending_buf_size = lit_bufsize as usize * 4;
        let mut s = Box::new(Deflate {
            total_in: 0,
            total_out: 0,
            adler: 0,
            data_type: Z_UNKNOWN,
            msg: None,
            status: INIT_STATE,
            pb: Pending {
                buf: vec![0u8; pending_buf_size],
                pending: 0,
                pending_out: 0,
                bi_buf: 0,
                bi_valid: 0,
            },
            pending_buf_size,
            wrap,
            last_flush: 0,
            w_size,
            w_bits,
            w_mask: w_size - 1,
            window: vec![0u8; 2 * w_size as usize],
            window_size: 0,
            prev: vec![0u16; w_size as usize],
            head: vec![0u16; hash_size as usize],
            ins_h: 0,
            hash_mask: hash_size - 1,
            hash_shift: hash_bits.div_ceil(MIN_MATCH),
            block_start: 0,
            match_length: 0,
            prev_match: 0,
            match_available: false,
            strstart: 0,
            match_start: 0,
            lookahead: 0,
            prev_length: 0,
            max_chain_length: 0,
            max_lazy_match: 0,
            level,
            strategy,
            good_match: 0,
            nice_match: 0,
            dyn_ltree: [Ct::default(); HEAP_SIZE],
            dyn_dtree: [Ct::default(); 2 * D_CODES + 1],
            bl_tree: [Ct::default(); 2 * BL_CODES + 1],
            l_max_code: 0,
            d_max_code: 0,
            hs: HeapState {
                bl_count: [0; MAX_BITS + 1],
                heap: [0; 2 * L_CODES + 1],
                heap_len: 0,
                heap_max: 0,
                depth: [0; 2 * L_CODES + 1],
                opt_len: 0,
                static_len: 0,
            },
            sym_off: lit_bufsize as usize,
            sym_next: 0,
            sym_end: (lit_bufsize - 1) * 3,
            matches: 0,
            insert: 0,
            high_water: 0,
        });
        s.reset();
        Ok(s)
    }

    /// deflateResetKeep()
    pub fn reset_keep(&mut self) {
        self.total_in = 0;
        self.total_out = 0;
        self.msg = None;
        self.data_type = Z_UNKNOWN;
        self.pb.pending = 0;
        self.pb.pending_out = 0;
        if self.wrap < 0 {
            self.wrap = -self.wrap;
        }
        self.status = if self.wrap == 2 {
            GZIP_STATE
        } else {
            INIT_STATE
        };
        self.adler = if self.wrap == 2 { crc32(0, &[]) } else { 1 };
        self.last_flush = -2;
        self.tr_init();
    }

    /// deflateReset()
    pub fn reset(&mut self) {
        self.reset_keep();
        self.lm_init();
    }

    fn clear_hash(&mut self) {
        for h in self.head.iter_mut() {
            *h = 0;
        }
    }

    fn lm_init(&mut self) {
        self.window_size = 2 * self.w_size;
        self.clear_hash();
        let c = &CONFIGURATION_TABLE[self.level as usize];
        self.max_lazy_match = c.max_lazy as u32;
        self.good_match = c.good_length as u32;
        self.nice_match = c.nice_length as i32;
        self.max_chain_length = c.max_chain as u32;
        self.strstart = 0;
        self.block_start = 0;
        self.lookahead = 0;
        self.insert = 0;
        self.match_length = MIN_MATCH - 1;
        self.prev_length = MIN_MATCH - 1;
        self.match_available = false;
        self.ins_h = 0;
    }

    fn tr_init(&mut self) {
        self.pb.bi_buf = 0;
        self.pb.bi_valid = 0;
        self.init_block();
    }

    fn init_block(&mut self) {
        for n in 0..L_CODES {
            self.dyn_ltree[n].fc = 0;
        }
        for n in 0..D_CODES {
            self.dyn_dtree[n].fc = 0;
        }
        for n in 0..BL_CODES {
            self.bl_tree[n].fc = 0;
        }
        self.dyn_ltree[END_BLOCK].fc = 1;
        self.hs.opt_len = 0;
        self.hs.static_len = 0;
        self.sym_next = 0;
        self.matches = 0;
    }

    #[inline]
    fn update_hash(&self, h: u32, c: u8) -> u32 {
        ((h << self.hash_shift) ^ c as u32) & self.hash_mask
    }

    /// INSERT_STRING(): returns the previous head of the hash chain.
    #[inline]
    fn insert_string(&mut self, str: u32) -> u32 {
        self.ins_h = self.update_hash(self.ins_h, self.window[(str + MIN_MATCH - 1) as usize]);
        let h = self.head[self.ins_h as usize];
        self.prev[(str & self.w_mask) as usize] = h;
        self.head[self.ins_h as usize] = str as u16;
        h as u32
    }

    #[inline]
    fn max_dist(&self) -> u32 {
        self.w_size - MIN_LOOKAHEAD
    }

    fn slide_hash(&mut self) {
        let wsize = self.w_size;
        for p in self.head.iter_mut() {
            let m = *p as u32;
            *p = if m >= wsize { (m - wsize) as u16 } else { 0 };
        }
        for p in self.prev.iter_mut() {
            let m = *p as u32;
            *p = if m >= wsize { (m - wsize) as u16 } else { 0 };
        }
    }

    /// read_buf() into the window at `off`.
    fn read_buf_window(&mut self, io: &mut Io, off: usize, size: u32) -> u32 {
        let len = (io.avail_in() as u64).min(size as u64) as usize;
        if len == 0 {
            return 0;
        }
        let src = &io.input[io.in_pos..io.in_pos + len];
        self.window[off..off + len].copy_from_slice(src);
        self.update_check(src);
        io.in_pos += len;
        self.total_in += len as u64;
        len as u32
    }

    /// read_buf() straight into next_out.
    fn read_buf_out(&mut self, io: &mut Io, len: usize) {
        let len = len.min(io.avail_in());
        if len == 0 {
            return;
        }
        let src = &io.input[io.in_pos..io.in_pos + len];
        io.output[io.out_pos..io.out_pos + len].copy_from_slice(src);
        self.update_check(src);
        io.in_pos += len;
        self.total_in += len as u64;
    }

    #[inline]
    fn update_check(&mut self, data: &[u8]) {
        if self.wrap == 1 {
            self.adler = adler32(self.adler, data);
        } else if self.wrap == 2 {
            self.adler = crc32(self.adler, data);
        }
    }

    fn fill_window(&mut self, io: &mut Io) {
        let wsize = self.w_size;
        loop {
            let mut more = self.window_size - self.lookahead - self.strstart;
            if self.strstart >= wsize + self.max_dist() {
                let n = (wsize - more) as usize;
                self.window
                    .copy_within(wsize as usize..wsize as usize + n, 0);
                self.match_start = self.match_start.wrapping_sub(wsize);
                self.strstart -= wsize;
                self.block_start -= wsize as i64;
                if self.insert > self.strstart {
                    self.insert = self.strstart;
                }
                self.slide_hash();
                more += wsize;
            }
            if io.avail_in() == 0 {
                break;
            }
            let off = (self.strstart + self.lookahead) as usize;
            let n = self.read_buf_window(io, off, more);
            self.lookahead += n;
            if self.lookahead + self.insert >= MIN_MATCH {
                let mut str = self.strstart - self.insert;
                self.ins_h = self.window[str as usize] as u32;
                self.ins_h = self.update_hash(self.ins_h, self.window[(str + 1) as usize]);
                while self.insert != 0 {
                    self.ins_h =
                        self.update_hash(self.ins_h, self.window[(str + MIN_MATCH - 1) as usize]);
                    self.prev[(str & self.w_mask) as usize] = self.head[self.ins_h as usize];
                    self.head[self.ins_h as usize] = str as u16;
                    str += 1;
                    self.insert -= 1;
                    if self.lookahead + self.insert < MIN_MATCH {
                        break;
                    }
                }
            }
            if !(self.lookahead < MIN_LOOKAHEAD && io.avail_in() != 0) {
                break;
            }
        }
        if self.high_water < self.window_size {
            let curr = self.strstart + self.lookahead;
            if self.high_water < curr {
                let init = (self.window_size - curr).min(WIN_INIT as u32);
                for b in &mut self.window[curr as usize..(curr + init) as usize] {
                    *b = 0;
                }
                self.high_water = curr + init;
            } else if self.high_water < curr + WIN_INIT as u32 {
                let init = (curr + WIN_INIT as u32 - self.high_water)
                    .min(self.window_size - self.high_water);
                for b in
                    &mut self.window[self.high_water as usize..(self.high_water + init) as usize]
                {
                    *b = 0;
                }
                self.high_water += init;
            }
        }
    }

    /// deflateSetDictionary()
    pub fn set_dictionary(&mut self, dictionary: &[u8]) -> i32 {
        let wrap = self.wrap;
        if wrap == 2 || (wrap == 1 && self.status != INIT_STATE) || self.lookahead != 0 {
            return Z_STREAM_ERROR;
        }
        if wrap == 1 {
            self.adler = adler32(self.adler, dictionary);
        }
        self.wrap = 0;
        let mut dict = dictionary;
        if dict.len() >= self.w_size as usize {
            if wrap == 0 {
                self.clear_hash();
                self.strstart = 0;
                self.block_start = 0;
                self.insert = 0;
            }
            dict = &dict[dict.len() - self.w_size as usize..];
        }
        let mut dummy = [0u8; 0];
        let mut io = Io::new(dict, &mut dummy);
        let saved_total_in = self.total_in;
        self.fill_window(&mut io);
        while self.lookahead >= MIN_MATCH {
            let mut str = self.strstart;
            let mut n = self.lookahead - (MIN_MATCH - 1);
            loop {
                self.ins_h =
                    self.update_hash(self.ins_h, self.window[(str + MIN_MATCH - 1) as usize]);
                self.prev[(str & self.w_mask) as usize] = self.head[self.ins_h as usize];
                self.head[self.ins_h as usize] = str as u16;
                str += 1;
                n -= 1;
                if n == 0 {
                    break;
                }
            }
            self.strstart = str;
            self.lookahead = MIN_MATCH - 1;
            self.fill_window(&mut io);
        }
        // read_buf() counts the dictionary in total_in (zlib does too).
        let _ = saved_total_in;
        self.strstart += self.lookahead;
        self.block_start = self.strstart as i64;
        self.insert = self.lookahead;
        self.lookahead = 0;
        self.match_length = MIN_MATCH - 1;
        self.prev_length = MIN_MATCH - 1;
        self.match_available = false;
        self.wrap = wrap;
        Z_OK
    }

    /// deflateParams()
    pub fn params(&mut self, io: &mut Io, level: i32, strategy: i32) -> i32 {
        let level = if level == Z_DEFAULT_COMPRESSION {
            6
        } else {
            level
        };
        if !(0..=9).contains(&level) || !(0..=Z_FIXED).contains(&strategy) {
            return Z_STREAM_ERROR;
        }
        let func = CONFIGURATION_TABLE[self.level as usize].func;
        if (strategy != self.strategy || func != CONFIGURATION_TABLE[level as usize].func)
            && self.last_flush != -2
        {
            let err = self.deflate(io, Z_BLOCK);
            if err == Z_STREAM_ERROR {
                return err;
            }
            if io.avail_in() != 0
                || (self.strstart as i64 - self.block_start) + self.lookahead as i64 != 0
            {
                return Z_BUF_ERROR;
            }
        }
        if self.level != level {
            if self.level == 0 && self.matches != 0 {
                if self.matches == 1 {
                    self.slide_hash();
                } else {
                    self.clear_hash();
                }
                self.matches = 0;
            }
            self.level = level;
            let c = &CONFIGURATION_TABLE[level as usize];
            self.max_lazy_match = c.max_lazy as u32;
            self.good_match = c.good_length as u32;
            self.nice_match = c.nice_length as i32;
            self.max_chain_length = c.max_chain as u32;
        }
        self.strategy = strategy;
        Z_OK
    }

    pub fn level(&self) -> i32 {
        self.level
    }

    pub fn strategy(&self) -> i32 {
        self.strategy
    }

    /// deflatePending(): bytes pending output.
    pub fn pending(&self) -> usize {
        self.pb.pending
    }

    fn flush_pending(&mut self, io: &mut Io) {
        self.pb.bi_flush();
        let len = self.pb.pending.min(io.avail_out());
        if len == 0 {
            return;
        }
        let po = self.pb.pending_out;
        io.output[io.out_pos..io.out_pos + len].copy_from_slice(&self.pb.buf[po..po + len]);
        io.out_pos += len;
        self.pb.pending_out += len;
        self.total_out += len as u64;
        self.pb.pending -= len;
        if self.pb.pending == 0 {
            self.pb.pending_out = 0;
        }
    }

    /// deflate()
    pub fn deflate(&mut self, io: &mut Io, flush: i32) -> i32 {
        if !(0..=Z_BLOCK).contains(&flush) {
            return Z_STREAM_ERROR;
        }
        if self.status == FINISH_STATE && flush != Z_FINISH {
            self.msg = Some(z_error(Z_STREAM_ERROR));
            return Z_STREAM_ERROR;
        }
        if io.avail_out() == 0 {
            self.msg = Some(z_error(Z_BUF_ERROR));
            return Z_BUF_ERROR;
        }
        let old_flush = self.last_flush;
        self.last_flush = flush;

        if self.pb.pending != 0 {
            self.flush_pending(io);
            if io.avail_out() == 0 {
                self.last_flush = -1;
                return Z_OK;
            }
        } else if io.avail_in() == 0 && rank(flush) <= rank(old_flush) && flush != Z_FINISH {
            self.msg = Some(z_error(Z_BUF_ERROR));
            return Z_BUF_ERROR;
        }

        if self.status == FINISH_STATE && io.avail_in() != 0 {
            self.msg = Some(z_error(Z_BUF_ERROR));
            return Z_BUF_ERROR;
        }

        if self.status == INIT_STATE && self.wrap == 0 {
            self.status = BUSY_STATE;
        }
        if self.status == INIT_STATE {
            let mut header = (Z_DEFLATED as u32 + ((self.w_bits - 8) << 4)) << 8;
            let level_flags = if self.strategy >= Z_HUFFMAN_ONLY || self.level < 2 {
                0
            } else if self.level < 6 {
                1
            } else if self.level == 6 {
                2
            } else {
                3
            };
            header |= level_flags << 6;
            if self.strstart != 0 {
                header |= PRESET_DICT;
            }
            header += 31 - (header % 31);
            self.pb.put_short_msb(header);
            if self.strstart != 0 {
                let a = self.adler;
                self.pb.put_short_msb(a >> 16);
                self.pb.put_short_msb(a & 0xffff);
            }
            self.adler = 1;
            self.status = BUSY_STATE;
            self.flush_pending(io);
            if self.pb.pending != 0 {
                self.last_flush = -1;
                return Z_OK;
            }
        }
        if self.status == GZIP_STATE {
            self.adler = 0;
            self.pb.put_byte(31);
            self.pb.put_byte(139);
            self.pb.put_byte(8);
            for _ in 0..5 {
                self.pb.put_byte(0);
            }
            let xfl = if self.level == 9 {
                2
            } else if self.strategy >= Z_HUFFMAN_ONLY || self.level < 2 {
                4
            } else {
                0
            };
            self.pb.put_byte(xfl);
            self.pb.put_byte(OS_CODE);
            self.status = BUSY_STATE;
            self.flush_pending(io);
            if self.pb.pending != 0 {
                self.last_flush = -1;
                return Z_OK;
            }
        }

        if io.avail_in() != 0
            || self.lookahead != 0
            || (flush != Z_NO_FLUSH && self.status != FINISH_STATE)
        {
            let bstate = if self.level == 0 {
                self.deflate_stored(io, flush)
            } else if self.strategy == Z_HUFFMAN_ONLY {
                self.deflate_huff(io, flush)
            } else if self.strategy == Z_RLE {
                self.deflate_rle(io, flush)
            } else {
                match CONFIGURATION_TABLE[self.level as usize].func {
                    Func::Stored => self.deflate_stored(io, flush),
                    Func::Fast => self.deflate_fast(io, flush),
                    Func::Slow => self.deflate_slow(io, flush),
                }
            };
            if bstate == BlockState::FinishStarted || bstate == BlockState::FinishDone {
                self.status = FINISH_STATE;
            }
            if bstate == BlockState::NeedMore || bstate == BlockState::FinishStarted {
                if io.avail_out() == 0 {
                    self.last_flush = -1;
                }
                return Z_OK;
            }
            if bstate == BlockState::BlockDone {
                if flush == Z_PARTIAL_FLUSH {
                    self.pb.align();
                } else if flush != Z_BLOCK {
                    self.pb.stored_block(&[], 0);
                    if flush == Z_FULL_FLUSH {
                        self.clear_hash();
                        if self.lookahead == 0 {
                            self.strstart = 0;
                            self.block_start = 0;
                            self.insert = 0;
                        }
                    }
                }
                self.flush_pending(io);
                if io.avail_out() == 0 {
                    self.last_flush = -1;
                    return Z_OK;
                }
            }
        }

        if flush != Z_FINISH {
            return Z_OK;
        }
        if self.wrap <= 0 {
            return Z_STREAM_END;
        }
        if self.wrap == 2 {
            let a = self.adler;
            let t = self.total_in;
            for i in 0..4 {
                self.pb.put_byte((a >> (8 * i)) as u8);
            }
            for i in 0..4 {
                self.pb.put_byte((t >> (8 * i)) as u8);
            }
        } else {
            let a = self.adler;
            self.pb.put_short_msb(a >> 16);
            self.pb.put_short_msb(a & 0xffff);
        }
        self.flush_pending(io);
        if self.wrap > 0 {
            self.wrap = -self.wrap;
        }
        if self.pb.pending != 0 {
            Z_OK
        } else {
            Z_STREAM_END
        }
    }

    /// deflateEnd() status: Z_DATA_ERROR if the stream was freed
    /// prematurely.
    pub fn end_status(&self) -> i32 {
        if self.status == BUSY_STATE {
            Z_DATA_ERROR
        } else {
            Z_OK
        }
    }

    fn longest_match(&mut self, mut cur_match: u32) -> u32 {
        let mut chain_length = self.max_chain_length;
        let win = &self.window;
        let scan0 = self.strstart as usize;
        let mut best_len = self.prev_length as usize;
        let mut nice_match = self.nice_match;
        let limit = if self.strstart > self.max_dist() {
            self.strstart - self.max_dist()
        } else {
            NIL
        };
        let prev = &self.prev;
        let wmask = self.w_mask;
        let strend = scan0 + MAX_MATCH as usize;
        let mut scan_end1 = win[scan0 + best_len - 1];
        let mut scan_end = win[scan0 + best_len];
        if self.prev_length >= self.good_match {
            chain_length >>= 2;
        }
        if nice_match as u32 > self.lookahead {
            nice_match = self.lookahead as i32;
        }
        let mut match_start = self.match_start;
        loop {
            let m = cur_match as usize;
            if !(win[m + best_len] != scan_end
                || win[m + best_len - 1] != scan_end1
                || win[m] != win[scan0]
                || win[m + 1] != win[scan0 + 1])
            {
                let mut scan = scan0 + 2;
                let mut mt = m + 2;
                loop {
                    let mut stop = false;
                    for _ in 0..8 {
                        scan += 1;
                        mt += 1;
                        if win[scan] != win[mt] {
                            stop = true;
                            break;
                        }
                    }
                    if stop || scan >= strend {
                        break;
                    }
                }
                let len = MAX_MATCH as usize - (strend - scan);
                if len > best_len {
                    match_start = cur_match;
                    best_len = len;
                    if len as i32 >= nice_match {
                        break;
                    }
                    scan_end1 = win[scan0 + best_len - 1];
                    scan_end = win[scan0 + best_len];
                }
            }
            cur_match = prev[(cur_match & wmask) as usize] as u32;
            if cur_match <= limit {
                break;
            }
            chain_length = chain_length.wrapping_sub(1);
            if chain_length == 0 {
                break;
            }
        }
        self.match_start = match_start;
        if best_len as u32 <= self.lookahead {
            best_len as u32
        } else {
            self.lookahead
        }
    }

    #[inline]
    fn tally_lit(&mut self, c: u8) -> bool {
        let o = self.sym_off + self.sym_next as usize;
        self.pb.buf[o] = 0;
        self.pb.buf[o + 1] = 0;
        self.pb.buf[o + 2] = c;
        self.sym_next += 3;
        self.dyn_ltree[c as usize].fc = self.dyn_ltree[c as usize].fc.wrapping_add(1);
        self.sym_next == self.sym_end
    }

    #[inline]
    fn tally_dist(&mut self, distance: u32, length: u32) -> bool {
        let t = tables();
        let len = length as u8;
        let dist = distance as u16;
        let o = self.sym_off + self.sym_next as usize;
        self.pb.buf[o] = dist as u8;
        self.pb.buf[o + 1] = (dist >> 8) as u8;
        self.pb.buf[o + 2] = len;
        self.sym_next += 3;
        let dist = dist.wrapping_sub(1) as u32;
        let lc = t.length_code[len as usize] as usize + LITERALS + 1;
        self.dyn_ltree[lc].fc = self.dyn_ltree[lc].fc.wrapping_add(1);
        let dc = d_code(t, dist);
        self.dyn_dtree[dc].fc = self.dyn_dtree[dc].fc.wrapping_add(1);
        self.sym_next == self.sym_end
    }

    fn detect_data_type(&self) -> i32 {
        let mut block_mask: u32 = 0xf3ffc07f;
        for n in 0..=31 {
            if (block_mask & 1) != 0 && self.dyn_ltree[n].fc != 0 {
                return Z_BINARY;
            }
            block_mask >>= 1;
        }
        if self.dyn_ltree[9].fc != 0 || self.dyn_ltree[10].fc != 0 || self.dyn_ltree[13].fc != 0 {
            return Z_TEXT;
        }
        for n in 32..LITERALS {
            if self.dyn_ltree[n].fc != 0 {
                return Z_TEXT;
            }
        }
        Z_BINARY
    }

    fn scan_tree(bl_tree: &mut [Ct], tree: &mut [Ct], max_code: i32) {
        let mut prevlen: i32 = -1;
        let mut nextlen = tree[0].dl as i32;
        let mut count = 0;
        let mut max_count = 7;
        let mut min_count = 4;
        if nextlen == 0 {
            max_count = 138;
            min_count = 3;
        }
        tree[(max_code + 1) as usize].dl = 0xffff;
        for n in 0..=max_code {
            let curlen = nextlen;
            nextlen = tree[(n + 1) as usize].dl as i32;
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            } else if count < min_count {
                bl_tree[curlen as usize].fc =
                    bl_tree[curlen as usize].fc.wrapping_add(count as u16);
            } else if curlen != 0 {
                if curlen != prevlen {
                    bl_tree[curlen as usize].fc = bl_tree[curlen as usize].fc.wrapping_add(1);
                }
                bl_tree[REP_3_6].fc = bl_tree[REP_3_6].fc.wrapping_add(1);
            } else if count <= 10 {
                bl_tree[REPZ_3_10].fc = bl_tree[REPZ_3_10].fc.wrapping_add(1);
            } else {
                bl_tree[REPZ_11_138].fc = bl_tree[REPZ_11_138].fc.wrapping_add(1);
            }
            count = 0;
            prevlen = curlen;
            if nextlen == 0 {
                max_count = 138;
                min_count = 3;
            } else if curlen == nextlen {
                max_count = 6;
                min_count = 3;
            } else {
                max_count = 7;
                min_count = 4;
            }
        }
    }

    fn build_bl_tree(&mut self) -> i32 {
        Self::scan_tree(&mut self.bl_tree, &mut self.dyn_ltree, self.l_max_code);
        Self::scan_tree(&mut self.bl_tree, &mut self.dyn_dtree, self.d_max_code);
        self.hs.build_tree(&mut self.bl_tree, TreeKind::Bl);
        let mut max_blindex = BL_CODES as i32 - 1;
        while max_blindex >= 3 {
            if self.bl_tree[BL_ORDER[max_blindex as usize]].dl != 0 {
                break;
            }
            max_blindex -= 1;
        }
        self.hs.opt_len = self
            .hs
            .opt_len
            .wrapping_add(3 * (max_blindex as u64 + 1) + 5 + 5 + 4);
        max_blindex
    }

    fn send_all_trees(&mut self, lcodes: i32, dcodes: i32, blcodes: i32) {
        self.pb.send_bits((lcodes - 257) as u32, 5);
        self.pb.send_bits((dcodes - 1) as u32, 5);
        self.pb.send_bits((blcodes - 4) as u32, 4);
        for rank in 0..blcodes as usize {
            let l = self.bl_tree[BL_ORDER[rank]].dl as u32;
            self.pb.send_bits(l, 3);
        }
        self.pb
            .send_tree(&self.dyn_ltree, lcodes - 1, &self.bl_tree);
        self.pb
            .send_tree(&self.dyn_dtree, dcodes - 1, &self.bl_tree);
    }

    /// _tr_flush_block(): `buf` is the window offset of the block data (or
    /// None when it already slid out of the window).
    fn tr_flush_block(&mut self, buf: Option<usize>, stored_len: u64, last: u32) {
        let opt_lenb;
        let static_lenb;
        let mut max_blindex = 0;
        if self.level > 0 {
            if self.data_type == Z_UNKNOWN {
                self.data_type = self.detect_data_type();
            }
            self.l_max_code = self.hs.build_tree(&mut self.dyn_ltree, TreeKind::L);
            self.d_max_code = self.hs.build_tree(&mut self.dyn_dtree, TreeKind::D);
            max_blindex = self.build_bl_tree();
            let o = (self.hs.opt_len.wrapping_add(3 + 7)) >> 3;
            let st = (self.hs.static_len.wrapping_add(3 + 7)) >> 3;
            static_lenb = st;
            opt_lenb = if st <= o || self.strategy == Z_FIXED {
                st
            } else {
                o
            };
        } else {
            opt_lenb = stored_len + 5;
            static_lenb = opt_lenb;
        }
        if stored_len + 4 <= opt_lenb && buf.is_some() {
            let b = buf.unwrap_or(0);
            let data = &self.window[b..b + stored_len as usize];
            self.pb.stored_block(data, last);
        } else if static_lenb == opt_lenb {
            self.pb.send_bits((STATIC_TREES << 1) + last, 3);
            let t = tables();
            let (so, sn) = (self.sym_off, self.sym_next as usize);
            self.pb.compress_block(&t.ltree, &t.dtree, so, sn);
        } else {
            self.pb.send_bits((DYN_TREES << 1) + last, 3);
            self.send_all_trees(self.l_max_code + 1, self.d_max_code + 1, max_blindex + 1);
            let (so, sn) = (self.sym_off, self.sym_next as usize);
            let lt = self.dyn_ltree;
            let dt = self.dyn_dtree;
            self.pb.compress_block(&lt, &dt, so, sn);
        }
        self.init_block();
        if last != 0 {
            self.pb.bi_windup();
        }
    }

    fn flush_block_only(&mut self, io: &mut Io, last: u32) {
        let buf = if self.block_start >= 0 {
            Some(self.block_start as usize)
        } else {
            None
        };
        let len = (self.strstart as i64 - self.block_start) as u64;
        self.tr_flush_block(buf, len, last);
        self.block_start = self.strstart as i64;
        self.flush_pending(io);
    }

    fn deflate_stored(&mut self, io: &mut Io, flush: i32) -> BlockState {
        let mut min_block = (self.pending_buf_size as u32 - 5).min(self.w_size);
        let mut last = 0u32;
        let mut used = io.avail_in() as u32;
        loop {
            let mut len = MAX_STORED;
            let mut have = ((self.pb.bi_valid + 42) >> 3) as u32;
            if (io.avail_out() as u64) < have as u64 {
                break;
            }
            have = (io.avail_out() as u64 - have as u64).min(u32::MAX as u64) as u32;
            let mut left = (self.strstart as i64 - self.block_start) as u32;
            let avail_in = io.avail_in() as u64;
            if len as u64 > left as u64 + avail_in {
                len = (left as u64 + avail_in) as u32;
            }
            if len > have {
                len = have;
            }
            if len < min_block
                && ((len == 0 && flush != Z_FINISH)
                    || flush == Z_NO_FLUSH
                    || len as u64 != left as u64 + io.avail_in() as u64)
            {
                break;
            }
            last = if flush == Z_FINISH && len as u64 == left as u64 + io.avail_in() as u64 {
                1
            } else {
                0
            };
            self.pb.stored_block(&[], last);
            let p = self.pb.pending;
            self.pb.buf[p - 4] = len as u8;
            self.pb.buf[p - 3] = (len >> 8) as u8;
            self.pb.buf[p - 2] = !len as u8;
            self.pb.buf[p - 1] = (!len >> 8) as u8;
            self.flush_pending(io);
            if left != 0 {
                if left > len {
                    left = len;
                }
                let bs = self.block_start as usize;
                io.output[io.out_pos..io.out_pos + left as usize]
                    .copy_from_slice(&self.window[bs..bs + left as usize]);
                io.out_pos += left as usize;
                self.total_out += left as u64;
                self.block_start += left as i64;
                len -= left;
            }
            if len != 0 {
                self.read_buf_out(io, len as usize);
                io.out_pos += len as usize;
                self.total_out += len as u64;
            }
            if last != 0 {
                break;
            }
        }

        used -= io.avail_in() as u32;
        if used != 0 {
            if used >= self.w_size {
                self.matches = 2;
                let ws = self.w_size as usize;
                self.window[..ws].copy_from_slice(&io.input[io.in_pos - ws..io.in_pos]);
                self.strstart = self.w_size;
                self.insert = self.strstart;
            } else {
                if self.window_size - self.strstart <= used {
                    self.strstart -= self.w_size;
                    let ws = self.w_size as usize;
                    self.window.copy_within(ws..ws + self.strstart as usize, 0);
                    if self.matches < 2 {
                        self.matches += 1;
                    }
                    if self.insert > self.strstart {
                        self.insert = self.strstart;
                    }
                }
                let ss = self.strstart as usize;
                let u = used as usize;
                self.window[ss..ss + u].copy_from_slice(&io.input[io.in_pos - u..io.in_pos]);
                self.strstart += used;
                self.insert += used.min(self.w_size - self.insert);
            }
            self.block_start = self.strstart as i64;
        }
        if self.high_water < self.strstart {
            self.high_water = self.strstart;
        }

        if last != 0 {
            return BlockState::FinishDone;
        }

        if flush != Z_NO_FLUSH
            && flush != Z_FINISH
            && io.avail_in() == 0
            && self.strstart as i64 == self.block_start
        {
            return BlockState::BlockDone;
        }

        let mut have = self.window_size - self.strstart;
        if io.avail_in() as u64 > have as u64 && self.block_start >= self.w_size as i64 {
            self.block_start -= self.w_size as i64;
            self.strstart -= self.w_size;
            let ws = self.w_size as usize;
            self.window.copy_within(ws..ws + self.strstart as usize, 0);
            if self.matches < 2 {
                self.matches += 1;
            }
            have += self.w_size;
            if self.insert > self.strstart {
                self.insert = self.strstart;
            }
        }
        if have as u64 > io.avail_in() as u64 {
            have = io.avail_in() as u32;
        }
        if have != 0 {
            let ss = self.strstart as usize;
            self.read_buf_window(io, ss, have);
            self.strstart += have;
            self.insert += have.min(self.w_size - self.insert);
        }
        if self.high_water < self.strstart {
            self.high_water = self.strstart;
        }

        let have = ((self.pb.bi_valid + 42) >> 3) as u32;
        let have = (self.pending_buf_size as u32 - have).min(MAX_STORED);
        min_block = have.min(self.w_size);
        let left = (self.strstart as i64 - self.block_start) as u32;
        if left >= min_block
            || ((left != 0 || flush == Z_FINISH)
                && flush != Z_NO_FLUSH
                && io.avail_in() == 0
                && left <= have)
        {
            let len = left.min(have);
            last = if flush == Z_FINISH && io.avail_in() == 0 && len == left {
                1
            } else {
                0
            };
            let bs = self.block_start as usize;
            let data = &self.window[bs..bs + len as usize];
            self.pb.stored_block(data, last);
            self.block_start += len as i64;
            self.flush_pending(io);
        }

        if last != 0 {
            BlockState::FinishStarted
        } else {
            BlockState::NeedMore
        }
    }

    fn deflate_fast(&mut self, io: &mut Io, flush: i32) -> BlockState {
        loop {
            if self.lookahead < MIN_LOOKAHEAD {
                self.fill_window(io);
                if self.lookahead < MIN_LOOKAHEAD && flush == Z_NO_FLUSH {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            let mut hash_head = NIL;
            if self.lookahead >= MIN_MATCH {
                hash_head = self.insert_string(self.strstart);
            }
            if hash_head != NIL && self.strstart.wrapping_sub(hash_head) <= self.max_dist() {
                self.match_length = self.longest_match(hash_head);
            }
            let bflush;
            if self.match_length >= MIN_MATCH {
                bflush = self.tally_dist(
                    self.strstart - self.match_start,
                    self.match_length - MIN_MATCH,
                );
                self.lookahead -= self.match_length;
                if self.match_length <= self.max_lazy_match && self.lookahead >= MIN_MATCH {
                    self.match_length -= 1;
                    loop {
                        self.strstart += 1;
                        self.insert_string(self.strstart);
                        self.match_length -= 1;
                        if self.match_length == 0 {
                            break;
                        }
                    }
                    self.strstart += 1;
                } else {
                    self.strstart += self.match_length;
                    self.match_length = 0;
                    self.ins_h = self.window[self.strstart as usize] as u32;
                    self.ins_h =
                        self.update_hash(self.ins_h, self.window[(self.strstart + 1) as usize]);
                }
            } else {
                bflush = self.tally_lit(self.window[self.strstart as usize]);
                self.lookahead -= 1;
                self.strstart += 1;
            }
            if bflush {
                self.flush_block_only(io, 0);
                if io.avail_out() == 0 {
                    return BlockState::NeedMore;
                }
            }
        }
        self.insert = if self.strstart < MIN_MATCH - 1 {
            self.strstart
        } else {
            MIN_MATCH - 1
        };
        if flush == Z_FINISH {
            self.flush_block_only(io, 1);
            if io.avail_out() == 0 {
                return BlockState::FinishStarted;
            }
            return BlockState::FinishDone;
        }
        if self.sym_next != 0 {
            self.flush_block_only(io, 0);
            if io.avail_out() == 0 {
                return BlockState::NeedMore;
            }
        }
        BlockState::BlockDone
    }

    fn deflate_slow(&mut self, io: &mut Io, flush: i32) -> BlockState {
        loop {
            if self.lookahead < MIN_LOOKAHEAD {
                self.fill_window(io);
                if self.lookahead < MIN_LOOKAHEAD && flush == Z_NO_FLUSH {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            let mut hash_head = NIL;
            if self.lookahead >= MIN_MATCH {
                hash_head = self.insert_string(self.strstart);
            }
            self.prev_length = self.match_length;
            self.prev_match = self.match_start;
            self.match_length = MIN_MATCH - 1;

            if hash_head != NIL
                && self.prev_length < self.max_lazy_match
                && self.strstart.wrapping_sub(hash_head) <= self.max_dist()
            {
                self.match_length = self.longest_match(hash_head);
                if self.match_length <= 5
                    && (self.strategy == Z_FILTERED
                        || (self.match_length == MIN_MATCH
                            && self.strstart.wrapping_sub(self.match_start) > TOO_FAR))
                {
                    self.match_length = MIN_MATCH - 1;
                }
            }
            if self.prev_length >= MIN_MATCH && self.match_length <= self.prev_length {
                let max_insert = self.strstart + self.lookahead - MIN_MATCH;
                let bflush = self.tally_dist(
                    self.strstart - 1 - self.prev_match,
                    self.prev_length - MIN_MATCH,
                );
                self.lookahead -= self.prev_length - 1;
                self.prev_length -= 2;
                loop {
                    self.strstart += 1;
                    if self.strstart <= max_insert {
                        self.insert_string(self.strstart);
                    }
                    self.prev_length -= 1;
                    if self.prev_length == 0 {
                        break;
                    }
                }
                self.match_available = false;
                self.match_length = MIN_MATCH - 1;
                self.strstart += 1;
                if bflush {
                    self.flush_block_only(io, 0);
                    if io.avail_out() == 0 {
                        return BlockState::NeedMore;
                    }
                }
            } else if self.match_available {
                let bflush = self.tally_lit(self.window[(self.strstart - 1) as usize]);
                if bflush {
                    self.flush_block_only(io, 0);
                }
                self.strstart += 1;
                self.lookahead -= 1;
                if io.avail_out() == 0 {
                    return BlockState::NeedMore;
                }
            } else {
                self.match_available = true;
                self.strstart += 1;
                self.lookahead -= 1;
            }
        }
        if self.match_available {
            self.tally_lit(self.window[(self.strstart - 1) as usize]);
            self.match_available = false;
        }
        self.insert = if self.strstart < MIN_MATCH - 1 {
            self.strstart
        } else {
            MIN_MATCH - 1
        };
        if flush == Z_FINISH {
            self.flush_block_only(io, 1);
            if io.avail_out() == 0 {
                return BlockState::FinishStarted;
            }
            return BlockState::FinishDone;
        }
        if self.sym_next != 0 {
            self.flush_block_only(io, 0);
            if io.avail_out() == 0 {
                return BlockState::NeedMore;
            }
        }
        BlockState::BlockDone
    }

    fn deflate_rle(&mut self, io: &mut Io, flush: i32) -> BlockState {
        loop {
            if self.lookahead <= MAX_MATCH {
                self.fill_window(io);
                if self.lookahead <= MAX_MATCH && flush == Z_NO_FLUSH {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            self.match_length = 0;
            if self.lookahead >= MIN_MATCH && self.strstart > 0 {
                let win = &self.window;
                let mut scan = self.strstart as usize - 1;
                let prev = win[scan];
                if prev == win[scan + 1] && prev == win[scan + 2] && prev == win[scan + 3] {
                    scan += 3;
                    let strend = self.strstart as usize + MAX_MATCH as usize;
                    loop {
                        let mut stop = false;
                        for _ in 0..8 {
                            scan += 1;
                            if prev != win[scan] {
                                stop = true;
                                break;
                            }
                        }
                        if stop || scan >= strend {
                            break;
                        }
                    }
                    self.match_length = MAX_MATCH - (strend - scan) as u32;
                    if self.match_length > self.lookahead {
                        self.match_length = self.lookahead;
                    }
                }
            }
            let bflush;
            if self.match_length >= MIN_MATCH {
                bflush = self.tally_dist(1, self.match_length - MIN_MATCH);
                self.lookahead -= self.match_length;
                self.strstart += self.match_length;
                self.match_length = 0;
            } else {
                bflush = self.tally_lit(self.window[self.strstart as usize]);
                self.lookahead -= 1;
                self.strstart += 1;
            }
            if bflush {
                self.flush_block_only(io, 0);
                if io.avail_out() == 0 {
                    return BlockState::NeedMore;
                }
            }
        }
        self.insert = 0;
        if flush == Z_FINISH {
            self.flush_block_only(io, 1);
            if io.avail_out() == 0 {
                return BlockState::FinishStarted;
            }
            return BlockState::FinishDone;
        }
        if self.sym_next != 0 {
            self.flush_block_only(io, 0);
            if io.avail_out() == 0 {
                return BlockState::NeedMore;
            }
        }
        BlockState::BlockDone
    }

    fn deflate_huff(&mut self, io: &mut Io, flush: i32) -> BlockState {
        loop {
            if self.lookahead == 0 {
                self.fill_window(io);
                if self.lookahead == 0 {
                    if flush == Z_NO_FLUSH {
                        return BlockState::NeedMore;
                    }
                    break;
                }
            }
            self.match_length = 0;
            let bflush = self.tally_lit(self.window[self.strstart as usize]);
            self.lookahead -= 1;
            self.strstart += 1;
            if bflush {
                self.flush_block_only(io, 0);
                if io.avail_out() == 0 {
                    return BlockState::NeedMore;
                }
            }
        }
        self.insert = 0;
        if flush == Z_FINISH {
            self.flush_block_only(io, 1);
            if io.avail_out() == 0 {
                return BlockState::FinishStarted;
            }
            return BlockState::FinishDone;
        }
        if self.sym_next != 0 {
            self.flush_block_only(io, 0);
            if io.avail_out() == 0 {
                return BlockState::NeedMore;
            }
        }
        BlockState::BlockDone
    }
}
