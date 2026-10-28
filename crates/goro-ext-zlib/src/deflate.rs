//! Deflate compressor (RFC 1951) with zlib (RFC 1950) and gzip (RFC 1952)
//! wrappers, producing the same bytes as PHP 8.5's zlib functions.
//!
//! PHP's tests compare compressed output byte for byte, so match finding
//! follows the parameters zlib documents for its levels (the
//! `good`/`lazy`/`nice`/`chain` tuning of the zlib manual's `deflateTune`,
//! with the per-level values zlib publishes) and the hash-chain and lazy
//! evaluation scheme described in RFC 1951 section 4. The remaining details
//! (hash function, window sliding, block size, when a match is "too far")
//! were settled against PHP 8.5's output in tests/fixtures/deflate_php85.txt.

use crate::checksum::{Adler32, Crc32};
use crate::trees::{self, BitWriter, Trees};

const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const MIN_LOOKAHEAD: usize = MAX_MATCH + MIN_MATCH + 1;
/// Matches of length 3 farther than this are not worth it.
const TOO_FAR: usize = 4096;
const MAX_STORED: usize = 65535;

/// Flush modes (zlib manual: deflate; PHP's `ZLIB_*_FLUSH` constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flush {
    None = 0,
    Partial = 1,
    Sync = 2,
    Full = 3,
    Finish = 4,
    Block = 5,
}

impl Flush {
    pub fn from_i64(v: i64) -> Option<Flush> {
        Some(match v {
            0 => Flush::None,
            1 => Flush::Partial,
            2 => Flush::Sync,
            3 => Flush::Full,
            4 => Flush::Finish,
            5 => Flush::Block,
            _ => return None,
        })
    }
}

/// Compression strategies (zlib manual: deflateInit2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Default = 0,
    Filtered = 1,
    HuffmanOnly = 2,
    Rle = 3,
    Fixed = 4,
}

impl Strategy {
    pub fn from_i64(v: i64) -> Option<Strategy> {
        Some(match v {
            0 => Strategy::Default,
            1 => Strategy::Filtered,
            2 => Strategy::HuffmanOnly,
            3 => Strategy::Rle,
            4 => Strategy::Fixed,
            _ => return None,
        })
    }
}

/// Container around the deflate data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    Raw,
    Zlib,
    Gzip,
}

/// zlib's per-level tuning: (good_length, max_lazy, nice_length,
/// max_chain), as published for `deflateTune`.
const CONFIG: [(usize, usize, usize, usize); 10] = [
    (0, 0, 0, 0),
    (4, 4, 8, 4),
    (4, 5, 16, 8),
    (4, 6, 32, 32),
    (4, 4, 16, 16),
    (8, 16, 32, 32),
    (8, 16, 128, 128),
    (8, 32, 128, 256),
    (32, 128, 258, 1024),
    (32, 258, 258, 4096),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockState {
    NeedMore,
    BlockDone,
    FinishStarted,
    FinishDone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Init,
    Busy,
    Finish,
}

/// Result of [`Deflater::deflate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeflateStatus {
    Ok,
    StreamEnd,
    /// Nothing to do (a repeated flush without new input).
    BufError,
}

/// Parameters of `deflateInit2` (zlib manual).
#[derive(Debug, Clone, Copy)]
pub struct DeflateParams {
    /// -1 (default, 6) to 9.
    pub level: i32,
    /// 8 to 15.
    pub window_bits: u32,
    /// 1 to 9.
    pub mem_level: u32,
    pub strategy: Strategy,
    pub wrap: Wrap,
}

impl Default for DeflateParams {
    fn default() -> Self {
        DeflateParams {
            level: -1,
            window_bits: 15,
            mem_level: 8,
            strategy: Strategy::Default,
            wrap: Wrap::Zlib,
        }
    }
}

/// Invalid compressor parameters (zlib's `Z_STREAM_ERROR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamError;

/// Streaming deflate compressor.
pub struct Deflater {
    level: i32,
    strategy: Strategy,
    wrap: Wrap,
    /// The trailer has been written (zlib's negative `wrap`).
    trailer_done: bool,
    status: Status,
    last_flush: i32,

    w_bits: u32,
    w_size: usize,
    w_mask: usize,
    window_size: usize,
    /// Two windows plus padding for 8-byte compares past the end.
    window: Vec<u8>,
    prev: Vec<u16>,
    head: Vec<u16>,
    hash_shift: u32,
    hash_mask: u32,
    ins_h: u32,

    strstart: usize,
    block_start: isize,
    lookahead: usize,
    insert: usize,
    match_start: usize,
    match_length: usize,
    prev_length: usize,
    prev_match: usize,
    match_available: bool,

    good_match: usize,
    max_lazy: usize,
    nice_match: usize,
    max_chain: usize,

    pending_buf_size: usize,
    trees: Trees,
    /// Bit writer whose byte vector is zlib's pending buffer.
    bw: BitWriter,
    /// Bytes of the pending buffer already handed out.
    pend_pos: usize,
    /// Output space left in the current call, and the output itself.
    avail_out: usize,
    dest: Vec<u8>,

    /// Input of the current call not yet moved into the window.
    adler: Adler32,
    crc: Crc32,
    total_in: u64,
}

const fn rank(f: i32) -> i32 {
    f * 2 - if f > 4 { 9 } else { 0 }
}

impl Deflater {
    pub fn new(p: DeflateParams) -> Result<Deflater, StreamError> {
        let level = if p.level == -1 { 6 } else { p.level };
        if !(0..=9).contains(&level)
            || !(1..=9).contains(&p.mem_level)
            || !(8..=15).contains(&p.window_bits)
            || (p.window_bits == 8 && p.wrap != Wrap::Zlib)
        {
            return Err(StreamError);
        }
        // A window of 256 bytes is written as 512 (observed on PHP 8.5: a
        // zlib header of 0x18 for `window` 8).
        let w_bits = if p.window_bits == 8 { 9 } else { p.window_bits };
        let w_size = 1usize << w_bits;
        let hash_bits = p.mem_level + 7;
        let hash_size = 1usize << hash_bits;
        let lit_bufsize = 1usize << (p.mem_level + 6);
        let (good, lazy, nice, chain) = CONFIG[level as usize];
        let d = Deflater {
            level,
            strategy: p.strategy,
            wrap: p.wrap,
            trailer_done: false,
            status: Status::Init,
            last_flush: -2,
            w_bits,
            w_size,
            w_mask: w_size - 1,
            window_size: 2 * w_size,
            window: vec![0; 2 * w_size + 8],
            prev: vec![0; w_size],
            head: vec![0; hash_size],
            hash_shift: hash_bits.div_ceil(MIN_MATCH as u32),
            hash_mask: (hash_size - 1) as u32,
            ins_h: 0,
            strstart: 0,
            block_start: 0,
            lookahead: 0,
            insert: 0,
            match_start: 0,
            match_length: MIN_MATCH - 1,
            prev_length: MIN_MATCH - 1,
            prev_match: 0,
            match_available: false,
            good_match: good,
            max_lazy: lazy,
            nice_match: nice,
            max_chain: chain,
            pending_buf_size: lit_bufsize * 4,
            trees: Trees::new(lit_bufsize),
            bw: BitWriter::new(),
            pend_pos: 0,
            avail_out: 0,
            dest: Vec::new(),
            adler: Adler32::new(),
            crc: Crc32::new(),
            total_in: 0,
        };
        Ok(d)
    }

    /// Back to the state right after [`Deflater::new`] (zlib's
    /// `deflateReset`), keeping the parameters.
    pub fn reset(&mut self) {
        self.trailer_done = false;
        self.status = Status::Init;
        self.last_flush = -2;
        self.window.fill(0);
        self.prev.fill(0);
        self.head.fill(0);
        self.ins_h = 0;
        self.strstart = 0;
        self.block_start = 0;
        self.lookahead = 0;
        self.insert = 0;
        self.match_start = 0;
        self.match_length = MIN_MATCH - 1;
        self.prev_length = MIN_MATCH - 1;
        self.prev_match = 0;
        self.match_available = false;
        self.trees.init_block();
        self.bw.reset();
        self.pend_pos = 0;
        self.adler = Adler32::new();
        self.crc = Crc32::new();
        self.total_in = 0;
    }

    pub fn total_in(&self) -> u64 {
        self.total_in
    }

    /// Primes the window with a preset dictionary (zlib manual:
    /// deflateSetDictionary). Fails for gzip streams and once compression
    /// has started.
    pub fn set_dictionary(&mut self, dict: &[u8]) -> Result<(), StreamError> {
        if self.wrap == Wrap::Gzip
            || (self.wrap == Wrap::Zlib && self.status != Status::Init)
            || self.lookahead != 0
        {
            return Err(StreamError);
        }
        if self.wrap == Wrap::Zlib {
            self.adler.update(dict);
        }
        let mut dict = dict;
        if dict.len() >= self.w_size {
            if self.wrap == Wrap::Raw {
                self.head.fill(0);
                self.strstart = 0;
                self.block_start = 0;
                self.insert = 0;
            }
            dict = &dict[dict.len() - self.w_size..];
        }
        let mut input = dict;
        self.fill_window(&mut input, false);
        while self.lookahead >= MIN_MATCH {
            let mut s = self.strstart;
            let mut n = self.lookahead - (MIN_MATCH - 1);
            while n > 0 {
                self.insert_string(s);
                s += 1;
                n -= 1;
            }
            self.strstart = s;
            self.lookahead = MIN_MATCH - 1;
            self.fill_window(&mut input, false);
        }
        self.strstart += self.lookahead;
        self.block_start = self.strstart as isize;
        self.insert = self.lookahead;
        self.lookahead = 0;
        self.match_length = MIN_MATCH - 1;
        self.prev_length = MIN_MATCH - 1;
        self.match_available = false;
        Ok(())
    }

    /// Compresses `input` with the given flush mode, appending the output
    /// to `out`, with no limit on the output size. All of `input` is
    /// consumed.
    pub fn deflate(&mut self, input: &[u8], flush: Flush, out: &mut Vec<u8>) -> DeflateStatus {
        let mut input = input;
        loop {
            let mut avail = usize::MAX;
            let st = self.deflate_step(&mut input, flush, out, &mut avail);
            if st == DeflateStatus::BufError
                || (input.is_empty() && (flush != Flush::Finish || st == DeflateStatus::StreamEnd))
            {
                return st;
            }
        }
    }

    /// One call of zlib's `deflate()` (zlib manual) with `avail_out` bytes
    /// of output space: consumes what it can of `input`, appends at most
    /// `avail_out` bytes to `out` and decrements `avail_out`. PHP's
    /// `deflate_add()` grows its buffer between calls, and the exact output
    /// space matters for stored blocks and repeated flush markers.
    pub fn deflate_step(
        &mut self,
        input: &mut &[u8],
        flush: Flush,
        out: &mut Vec<u8>,
        avail_out: &mut usize,
    ) -> DeflateStatus {
        if *avail_out == 0 {
            return DeflateStatus::BufError;
        }
        self.dest = std::mem::take(out);
        self.avail_out = *avail_out;
        let st = self.deflate_inner(input, flush);
        *avail_out = self.avail_out;
        *out = std::mem::take(&mut self.dest);
        st
    }

    fn pending(&self) -> bool {
        self.bw.out.len() > self.pend_pos
    }

    /// Moves pending output to the caller, as far as the output space goes.
    fn flush_pending(&mut self) {
        self.bw.flush_bits();
        let avail = self.bw.out.len() - self.pend_pos;
        let n = avail.min(self.avail_out);
        if n == 0 {
            return;
        }
        if self.pend_pos == 0 && n == avail && self.dest.is_empty() {
            std::mem::swap(&mut self.dest, &mut self.bw.out);
            self.bw.out.clear();
        } else {
            self.dest
                .extend_from_slice(&self.bw.out[self.pend_pos..self.pend_pos + n]);
            self.pend_pos += n;
            if self.pend_pos == self.bw.out.len() {
                self.bw.out.clear();
                self.pend_pos = 0;
            }
        }
        self.take_out(n);
    }

    fn take_out(&mut self, n: usize) {
        if self.avail_out != usize::MAX {
            self.avail_out -= n;
        }
    }

    fn deflate_inner(&mut self, input: &mut &[u8], flush: Flush) -> DeflateStatus {
        let flush_i = flush as i32;
        let old_flush = self.last_flush;
        self.last_flush = flush_i;
        if self.pending() {
            self.flush_pending();
            if self.avail_out == 0 {
                self.last_flush = -1;
                return DeflateStatus::Ok;
            }
        } else if input.is_empty() && rank(flush_i) <= rank(old_flush) && flush != Flush::Finish {
            return DeflateStatus::BufError;
        }
        if self.status == Status::Finish && !input.is_empty() {
            return DeflateStatus::BufError;
        }
        if self.status == Status::Init {
            self.write_header();
            self.status = Status::Busy;
            self.flush_pending();
            if self.pending() {
                self.last_flush = -1;
                return DeflateStatus::Ok;
            }
        }
        if !input.is_empty()
            || self.lookahead != 0
            || (flush != Flush::None && self.status != Status::Finish)
        {
            let bstate = if self.level == 0 {
                self.deflate_stored(input, flush)
            } else {
                match self.strategy {
                    Strategy::HuffmanOnly => self.deflate_huff(input, flush),
                    Strategy::Rle => self.deflate_rle(input, flush),
                    _ if self.level <= 3 => self.deflate_fast(input, flush),
                    _ => self.deflate_slow(input, flush),
                }
            };
            if matches!(bstate, BlockState::FinishStarted | BlockState::FinishDone) {
                self.status = Status::Finish;
            }
            if matches!(bstate, BlockState::NeedMore | BlockState::FinishStarted) {
                if self.avail_out == 0 {
                    self.last_flush = -1;
                }
                return DeflateStatus::Ok;
            }
            if bstate == BlockState::BlockDone {
                match flush {
                    Flush::Partial => trees::align(&mut self.bw),
                    Flush::Block => {}
                    _ => {
                        trees::stored_block(&mut self.bw, &[], false);
                        if flush == Flush::Full {
                            self.head.fill(0);
                            if self.lookahead == 0 {
                                self.strstart = 0;
                                self.block_start = 0;
                                self.insert = 0;
                            }
                        }
                    }
                }
                self.flush_pending();
                if self.avail_out == 0 {
                    self.last_flush = -1;
                    return DeflateStatus::Ok;
                }
            }
        }
        if flush != Flush::Finish {
            return DeflateStatus::Ok;
        }
        if self.wrap == Wrap::Raw || self.trailer_done {
            return DeflateStatus::StreamEnd;
        }
        match self.wrap {
            Wrap::Zlib => {
                let a = self.adler.value();
                self.bw.out.extend_from_slice(&a.to_be_bytes());
            }
            Wrap::Gzip => {
                let c = self.crc.value();
                self.bw.out.extend_from_slice(&c.to_le_bytes());
                self.bw
                    .out
                    .extend_from_slice(&(self.total_in as u32).to_le_bytes());
            }
            Wrap::Raw => {}
        }
        self.flush_pending();
        self.trailer_done = true;
        if self.pending() {
            DeflateStatus::Ok
        } else {
            DeflateStatus::StreamEnd
        }
    }

    fn write_header(&mut self) {
        match self.wrap {
            Wrap::Raw => {}
            Wrap::Zlib => {
                let mut header = (8 + ((self.w_bits - 8) << 4)) << 8;
                let level_flags =
                    if self.strategy as u32 >= Strategy::HuffmanOnly as u32 || self.level < 2 {
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
                    header |= 0x20;
                }
                header += 31 - (header % 31);
                self.bw
                    .out
                    .extend_from_slice(&(header as u16).to_be_bytes());
                if self.strstart != 0 {
                    let a = self.adler.value();
                    self.bw.out.extend_from_slice(&a.to_be_bytes());
                }
                self.adler = Adler32::new();
            }
            Wrap::Gzip => {
                let xfl = if self.level == 9 {
                    2
                } else if self.strategy as u32 >= Strategy::HuffmanOnly as u32 || self.level < 2 {
                    4
                } else {
                    0
                };
                // Unix OS code (observed on PHP 8.5).
                self.bw
                    .out
                    .extend_from_slice(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, xfl, 3]);
            }
        }
    }

    /// Takes up to `n` bytes of input, updating the checksum.
    fn read_buf<'a>(&mut self, input: &mut &'a [u8], n: usize) -> &'a [u8] {
        let n = n.min(input.len());
        let (taken, rest) = input.split_at(n);
        *input = rest;
        match self.wrap {
            Wrap::Zlib => self.adler.update(taken),
            Wrap::Gzip => self.crc.update(taken),
            Wrap::Raw => {}
        }
        self.total_in += n as u64;
        taken
    }

    #[inline(always)]
    fn update_hash(&self, h: u32, c: u8) -> u32 {
        ((h << self.hash_shift) ^ c as u32) & self.hash_mask
    }

    /// Inserts the string at `s` in the hash chains, returning the previous
    /// head of its chain.
    #[inline(always)]
    fn insert_string(&mut self, s: usize) -> usize {
        self.ins_h = self.update_hash(self.ins_h, self.window[s + MIN_MATCH - 1]);
        let h = self.ins_h as usize;
        debug_assert!(h < self.head.len());
        // SAFETY: `h <= hash_mask == head.len() - 1` and
        // `s & w_mask < prev.len() == w_mask + 1`.
        unsafe {
            let head = *self.head.get_unchecked(h);
            *self.prev.get_unchecked_mut(s & self.w_mask) = head;
            *self.head.get_unchecked_mut(h) = s as u16;
            head as usize
        }
    }

    fn max_dist(&self) -> usize {
        self.w_size - MIN_LOOKAHEAD
    }

    fn slide_hash(&mut self) {
        let w = self.w_size as u16;
        for m in self.head.iter_mut() {
            *m = m.saturating_sub(w);
        }
        for m in self.prev.iter_mut() {
            *m = m.saturating_sub(w);
        }
    }

    /// Moves input into the window when the lookahead runs low, sliding the
    /// window when it is full. `checksum` is false for dictionaries.
    fn fill_window(&mut self, input: &mut &[u8], checksum: bool) {
        let wsize = self.w_size;
        loop {
            let mut more = self.window_size - self.lookahead - self.strstart;
            if self.strstart >= wsize + self.max_dist() {
                self.window.copy_within(wsize..2 * wsize - more, 0);
                self.match_start = self.match_start.wrapping_sub(wsize);
                self.strstart -= wsize;
                self.block_start -= wsize as isize;
                if self.insert > self.strstart {
                    self.insert = self.strstart;
                }
                self.slide_hash();
                more += wsize;
            }
            if input.is_empty() {
                break;
            }
            let dst = self.strstart + self.lookahead;
            let taken = if checksum {
                self.read_buf(input, more)
            } else {
                let n = more.min(input.len());
                let (t, r) = input.split_at(n);
                *input = r;
                t
            };
            let n = taken.len();
            self.window[dst..dst + n].copy_from_slice(taken);
            self.lookahead += n;

            // Hash the bytes left over from the previous call now that the
            // strings starting there are complete.
            if self.lookahead + self.insert >= MIN_MATCH {
                let mut s = self.strstart - self.insert;
                self.ins_h = self.window[s] as u32;
                self.ins_h = self.update_hash(self.ins_h, self.window[s + 1]);
                while self.insert > 0 {
                    self.ins_h = self.update_hash(self.ins_h, self.window[s + MIN_MATCH - 1]);
                    let h = self.ins_h as usize;
                    self.prev[s & self.w_mask] = self.head[h];
                    self.head[h] = s as u16;
                    s += 1;
                    self.insert -= 1;
                    if self.lookahead + self.insert < MIN_MATCH {
                        break;
                    }
                }
            }
            if !(self.lookahead < MIN_LOOKAHEAD && !input.is_empty()) {
                break;
            }
        }
    }

    fn flush_block_only(&mut self, last: bool) {
        let stored_len = (self.strstart as isize - self.block_start) as usize;
        let buf = if self.block_start >= 0 {
            let b = self.block_start as usize;
            Some(&self.window[b..b + stored_len])
        } else {
            None
        };
        self.trees.flush_block(
            &mut self.bw,
            buf,
            stored_len,
            last,
            self.level,
            self.strategy == Strategy::Fixed,
        );
        self.block_start = self.strstart as isize;
        self.flush_pending();
    }

    /// Walks the hash chain from `cur_match` for the longest match at
    /// `strstart` longer than `prev_length`.
    fn longest_match(&mut self, mut cur_match: usize) -> usize {
        let mut chain_length = self.max_chain;
        let scan = self.strstart;
        let mut best_len = self.prev_length;
        let mut nice_match = self.nice_match;
        let limit = if self.strstart > self.max_dist() {
            self.strstart - self.max_dist()
        } else {
            0
        };
        if self.prev_length >= self.good_match {
            chain_length >>= 2;
        }
        if nice_match > self.lookahead {
            nice_match = self.lookahead;
        }
        let w_mask = self.w_mask;
        debug_assert!(scan + MAX_MATCH + 8 <= self.window.len());
        debug_assert!((2..=MAX_MATCH).contains(&best_len));
        let w = self.window.as_ptr();
        let prev = self.prev.as_ptr();
        // SAFETY: every read is below `scan + MAX_MATCH + 2` (candidate
        // positions are before `scan`), and the window has 8 bytes of
        // padding past `window_size >= scan + MAX_MATCH` (fill_window keeps
        // `strstart <= window_size - MIN_LOOKAHEAD` whenever a match is
        // looked for). `cur_match & w_mask` indexes `prev`, whose length is
        // `w_mask + 1`.
        unsafe {
            let rd16 = |p: usize| (w.add(p) as *const u16).read_unaligned();
            let rd64 = |p: usize| (w.add(p) as *const u64).read_unaligned();
            let scan_start = rd16(scan);
            let mut scan_end = rd16(scan + best_len - 1);
            loop {
                let m = cur_match;
                if rd16(m + best_len - 1) == scan_end && rd16(m) == scan_start {
                    // Bytes 0 and 1 match; with equal hashes byte 2 does too.
                    let mut len = 2;
                    loop {
                        let d = rd64(scan + len) ^ rd64(m + len);
                        if d != 0 {
                            len += (d.trailing_zeros() / 8) as usize;
                            break;
                        }
                        len += 8;
                        if len >= MAX_MATCH {
                            break;
                        }
                    }
                    let len = len.min(MAX_MATCH);
                    if len > best_len {
                        self.match_start = m;
                        best_len = len;
                        if len >= nice_match {
                            break;
                        }
                        scan_end = rd16(scan + best_len - 1);
                    }
                }
                cur_match = *prev.add(cur_match & w_mask) as usize;
                if cur_match <= limit {
                    break;
                }
                chain_length -= 1;
                if chain_length == 0 {
                    break;
                }
            }
        }
        best_len.min(self.lookahead)
    }

    fn finish_state(&mut self, flush: Flush) -> BlockState {
        if flush == Flush::Finish {
            self.flush_block_only(true);
            if self.avail_out == 0 {
                return BlockState::FinishStarted;
            }
            return BlockState::FinishDone;
        }
        if !self.trees.syms.is_empty() {
            self.flush_block_only(false);
            if self.avail_out == 0 {
                return BlockState::NeedMore;
            }
        }
        BlockState::BlockDone
    }

    fn deflate_fast(&mut self, input: &mut &[u8], flush: Flush) -> BlockState {
        loop {
            if self.lookahead < MIN_LOOKAHEAD {
                self.fill_window(input, true);
                if self.lookahead < MIN_LOOKAHEAD && flush == Flush::None {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            let mut hash_head = 0;
            if self.lookahead >= MIN_MATCH {
                hash_head = self.insert_string(self.strstart);
            }
            if hash_head != 0 && self.strstart - hash_head <= self.max_dist() {
                self.match_length = self.longest_match(hash_head);
            }
            let bflush;
            if self.match_length >= MIN_MATCH {
                bflush = self.trees.tally_dist(
                    self.strstart - self.match_start,
                    self.match_length - MIN_MATCH,
                );
                self.lookahead -= self.match_length;
                if self.match_length <= self.max_lazy && self.lookahead >= MIN_MATCH {
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
                    self.ins_h = self.window[self.strstart] as u32;
                    self.ins_h = self.update_hash(self.ins_h, self.window[self.strstart + 1]);
                }
            } else {
                bflush = self.trees.tally_lit(self.window[self.strstart]);
                self.lookahead -= 1;
                self.strstart += 1;
            }
            if bflush {
                self.flush_block_only(false);
                if self.avail_out == 0 {
                    return BlockState::NeedMore;
                }
            }
        }
        self.insert = self.strstart.min(MIN_MATCH - 1);
        self.finish_state(flush)
    }

    fn deflate_slow(&mut self, input: &mut &[u8], flush: Flush) -> BlockState {
        loop {
            if self.lookahead < MIN_LOOKAHEAD {
                self.fill_window(input, true);
                if self.lookahead < MIN_LOOKAHEAD && flush == Flush::None {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            let mut hash_head = 0;
            if self.lookahead >= MIN_MATCH {
                hash_head = self.insert_string(self.strstart);
            }
            self.prev_length = self.match_length;
            self.prev_match = self.match_start;
            self.match_length = MIN_MATCH - 1;
            if hash_head != 0
                && self.prev_length < self.max_lazy
                && self.strstart - hash_head <= self.max_dist()
            {
                self.match_length = self.longest_match(hash_head);
                if self.match_length <= 5
                    && (self.strategy == Strategy::Filtered
                        || (self.match_length == MIN_MATCH
                            && self.strstart - self.match_start > TOO_FAR))
                {
                    self.match_length = MIN_MATCH - 1;
                }
            }
            if self.prev_length >= MIN_MATCH && self.match_length <= self.prev_length {
                let max_insert = self.strstart + self.lookahead - MIN_MATCH;
                let bflush = self.trees.tally_dist(
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
                    self.flush_block_only(false);
                    if self.avail_out == 0 {
                        return BlockState::NeedMore;
                    }
                }
            } else if self.match_available {
                let bflush = self.trees.tally_lit(self.window[self.strstart - 1]);
                if bflush {
                    self.flush_block_only(false);
                }
                self.strstart += 1;
                self.lookahead -= 1;
                if self.avail_out == 0 {
                    return BlockState::NeedMore;
                }
            } else {
                self.match_available = true;
                self.strstart += 1;
                self.lookahead -= 1;
            }
        }
        if self.match_available {
            self.trees.tally_lit(self.window[self.strstart - 1]);
            self.match_available = false;
        }
        self.insert = self.strstart.min(MIN_MATCH - 1);
        self.finish_state(flush)
    }

    fn deflate_rle(&mut self, input: &mut &[u8], flush: Flush) -> BlockState {
        loop {
            if self.lookahead <= MAX_MATCH {
                self.fill_window(input, true);
                if self.lookahead <= MAX_MATCH && flush == Flush::None {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            self.match_length = 0;
            if self.lookahead >= MIN_MATCH && self.strstart > 0 {
                let s = self.strstart;
                let w = &self.window;
                let p = w[s - 1];
                if p == w[s] && p == w[s + 1] && p == w[s + 2] {
                    let mut n = 3;
                    while n < MAX_MATCH && w[s + n] == p {
                        n += 1;
                    }
                    self.match_length = n.min(self.lookahead);
                }
            }
            let bflush;
            if self.match_length >= MIN_MATCH {
                bflush = self.trees.tally_dist(1, self.match_length - MIN_MATCH);
                self.lookahead -= self.match_length;
                self.strstart += self.match_length;
                self.match_length = 0;
            } else {
                bflush = self.trees.tally_lit(self.window[self.strstart]);
                self.lookahead -= 1;
                self.strstart += 1;
            }
            if bflush {
                self.flush_block_only(false);
                if self.avail_out == 0 {
                    return BlockState::NeedMore;
                }
            }
        }
        self.insert = 0;
        self.finish_state(flush)
    }

    fn deflate_huff(&mut self, input: &mut &[u8], flush: Flush) -> BlockState {
        loop {
            if self.lookahead == 0 {
                self.fill_window(input, true);
                if self.lookahead == 0 {
                    if flush == Flush::None {
                        return BlockState::NeedMore;
                    }
                    break;
                }
            }
            self.match_length = 0;
            let bflush = self.trees.tally_lit(self.window[self.strstart]);
            self.lookahead -= 1;
            self.strstart += 1;
            if bflush {
                self.flush_block_only(false);
                if self.avail_out == 0 {
                    return BlockState::NeedMore;
                }
            }
        }
        self.insert = 0;
        self.finish_state(flush)
    }

    /// Level 0: stored blocks of up to 65535 bytes, written straight to
    /// the output when there is room for a worthwhile block, otherwise
    /// buffered in the window and written from there.
    fn deflate_stored(&mut self, input: &mut &[u8], flush: Flush) -> BlockState {
        let mut min_block = (self.pending_buf_size - 5).min(self.w_size);
        let mut last = false;
        let start_input: &[u8] = input;
        loop {
            self.bw.flush_bits();
            let mut len = MAX_STORED;
            let hdr = ((self.bw.pending_bits() + 42) >> 3) as usize;
            if self.avail_out < hdr {
                break;
            }
            let have = self.avail_out - hdr;
            let mut left = (self.strstart as isize - self.block_start) as usize;
            if len > left + input.len() {
                len = left + input.len();
            }
            if len > have {
                len = have;
            }
            if len < min_block
                && ((len == 0 && flush != Flush::Finish)
                    || flush == Flush::None
                    || len != left + input.len())
            {
                break;
            }
            last = flush == Flush::Finish && len == left + input.len();
            self.bw.send(last as u32, 3);
            self.bw.windup();
            let l16 = len as u16;
            self.bw.out.extend_from_slice(&l16.to_le_bytes());
            self.bw.out.extend_from_slice(&(!l16).to_le_bytes());
            self.flush_pending();
            if left > 0 {
                if left > len {
                    left = len;
                }
                let b = self.block_start as usize;
                self.dest.extend_from_slice(&self.window[b..b + left]);
                self.take_out(left);
                self.block_start += left as isize;
                len -= left;
            }
            if len > 0 {
                let taken = self.read_buf(input, len);
                self.dest.extend_from_slice(taken);
                self.take_out(len);
            }
            if last {
                break;
            }
        }
        let used = start_input.len() - input.len();
        if used > 0 {
            let consumed = &start_input[..used];
            if used >= self.w_size {
                let ws = self.w_size;
                self.window[..ws].copy_from_slice(&consumed[used - ws..]);
                self.strstart = ws;
                self.insert = ws;
            } else {
                if self.window_size - self.strstart <= used {
                    self.strstart -= self.w_size;
                    let s = self.strstart;
                    let ws = self.w_size;
                    self.window.copy_within(ws..ws + s, 0);
                    if self.insert > self.strstart {
                        self.insert = self.strstart;
                    }
                }
                let s = self.strstart;
                self.window[s..s + used].copy_from_slice(consumed);
                self.strstart += used;
                self.insert += used.min(self.w_size - self.insert);
            }
            self.block_start = self.strstart as isize;
        }
        if last {
            return BlockState::FinishDone;
        }
        if flush != Flush::None
            && flush != Flush::Finish
            && input.is_empty()
            && self.strstart as isize == self.block_start
        {
            return BlockState::BlockDone;
        }
        // Buffer the remaining input in the window.
        let mut have = self.window_size - self.strstart;
        if input.len() > have && self.block_start >= self.w_size as isize {
            self.block_start -= self.w_size as isize;
            self.strstart -= self.w_size;
            let s = self.strstart;
            let ws = self.w_size;
            self.window.copy_within(ws..ws + s, 0);
            have += self.w_size;
            if self.insert > self.strstart {
                self.insert = self.strstart;
            }
        }
        if have > input.len() {
            have = input.len();
        }
        if have > 0 {
            let s = self.strstart;
            let taken = self.read_buf(input, have);
            self.window[s..s + have].copy_from_slice(taken);
            self.strstart += have;
            self.insert += have.min(self.w_size - self.insert);
        }
        // Emit a block from the window if there is enough for a worthwhile
        // one, or when flushing.
        self.bw.flush_bits();
        let hdr = ((self.bw.pending_bits() + 42) >> 3) as usize;
        let have = (self.pending_buf_size - hdr).min(MAX_STORED);
        min_block = have.min(self.w_size);
        let left = (self.strstart as isize - self.block_start) as usize;
        if left >= min_block
            || ((left > 0 || flush == Flush::Finish)
                && flush != Flush::None
                && input.is_empty()
                && left <= have)
        {
            let len = left.min(have);
            last = flush == Flush::Finish && input.is_empty() && len == left;
            let b = self.block_start as usize;
            let (bw, win) = (&mut self.bw, &self.window);
            trees::stored_block(bw, &win[b..b + len], last);
            self.block_start += len as isize;
            self.flush_pending();
        }
        if last {
            BlockState::FinishStarted
        } else {
            BlockState::NeedMore
        }
    }
}
