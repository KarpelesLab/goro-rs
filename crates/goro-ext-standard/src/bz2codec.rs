//! bzip2 with libbzip2's streaming interface (BZ2_bzCompress /
//! BZ2_bzDecompress), on top of compcol's bzip2 codec.
//!
//! compcol's encoder does the per-block work (RLE-1, BWT, MTF, RLE-2,
//! Huffman tables and selectors match libbzip2 byte for byte); this layer
//! cuts the blocks itself, the way libbzip2's `copy_input_until_stop` /
//! `handle_compress` do, and splices the blocks into one bit stream:
//!
//! - a block ends once the *flushed* RLE-1 output reaches
//!   `100000 * blockSize100k - 19` bytes, checked before each input byte;
//!   the pending run is not part of it (it starts the next block). compcol
//!   0.7.3 cuts as soon as the RLE-1 length *including* the pending run
//!   reaches the limit, so it would split such a run differently.
//! - BZ_FLUSH ends the current block without ending the stream (compcol has
//!   no bzip2 flush); what is output then follows libbzip2's bit writer,
//!   which keeps the bits of its last write (the block's end-of-block code)
//!   until the next one.
//!
//! Each block is encoded by a fresh compcol encoder at block size 9 (only
//! the stream header depends on the block size), from which the block's
//! bits (between the stream header and the end-of-stream trailer) are
//! taken.
//!
//! The decoder side reports where a stream ends (BZ_STREAM_END with the
//! trailing input left unconsumed) only when asked to track it: compcol
//! absorbs all input it is given, so the end is found by locating the
//! byte-aligned end-of-stream trailer in the input fed.

use compcol::{Decoder as _, Encoder as _, Status as CStatus};

const BLOCK_MAGIC: u64 = 0x3141_5926_5359;
const EOS_MAGIC: u64 = 0x1772_4538_5090;

/// BZ_RUN / BZ_FLUSH / BZ_FINISH
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Run,
    Flush,
    Finish,
}

/// The non-error return codes of BZ2_bzCompress / BZ2_bzDecompress.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// BZ_OK
    Ok,
    /// BZ_RUN_OK
    RunOk,
    /// BZ_FLUSH_OK
    FlushOk,
    /// BZ_FINISH_OK
    FinishOk,
    /// BZ_STREAM_END
    StreamEnd,
}

/// The error return codes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    /// BZ_SEQUENCE_ERROR
    Sequence,
    /// BZ_PARAM_ERROR
    Param,
    /// BZ_DATA_ERROR
    Data,
    /// BZ_DATA_ERROR_MAGIC
    DataMagic,
}

impl Error {
    /// The libbzip2 return code.
    pub fn code(self) -> i32 {
        match self {
            Error::Sequence => -1,
            Error::Param => -2,
            Error::Data => -4,
            Error::DataMagic => -5,
        }
    }
}

/// libbzip2's bit writer (bsW): a write first moves the whole bytes held
/// to `out`, so up to 7 + n bits of the last write of n bits stay held
/// until the next write or bsFinishWrite(). That decides how much a
/// BZ_FLUSH outputs.
struct BitSink {
    acc: u64,
    live: u32,
}

impl BitSink {
    fn put(&mut self, out: &mut Vec<u8>, n: u32, v: u32) {
        debug_assert!(n <= 32);
        while self.live >= 8 {
            self.live -= 8;
            out.push((self.acc >> self.live) as u8);
        }
        self.acc = (self.acc << n) | (v as u64 & ((1u64 << n) - 1));
        self.live += n;
        self.acc &= (1u64 << self.live) - 1;
    }

    /// bsFinishWrite(): pad the last byte with zero bits.
    fn finish(&mut self, out: &mut Vec<u8>) {
        while self.live >= 8 {
            self.live -= 8;
            out.push((self.acc >> self.live) as u8);
        }
        if self.live > 0 {
            out.push((self.acc << (8 - self.live)) as u8);
        }
        self.acc = 0;
        self.live = 0;
    }
}

/// Reads the header of the block starting at bit `pos` of `z` (after its
/// magic) up to its code-length tables: the code length of the
/// end-of-block symbol in the table of the last group, i.e. the size of
/// the last write of the block.
fn eob_code_len(z: &[u8], mut pos: usize) -> Option<u32> {
    let total = z.len() * 8;
    let mut take = |n: usize| -> Option<u64> {
        if pos + n > total {
            return None;
        }
        let v = get_bits(z, pos, n);
        pos += n;
        Some(v)
    };
    take(32 + 1 + 24)?; // CRC, randomised, origPtr
    let stripes = take(16)?;
    let mut in_use = 0;
    for i in 0..16 {
        if stripes & (1 << (15 - i)) != 0 {
            in_use += take(16)?.count_ones() as usize;
        }
    }
    let alpha = in_use + 2;
    let groups = take(3)? as usize;
    let selectors = take(15)? as usize;
    if !(2..=6).contains(&groups) || selectors == 0 {
        return None;
    }
    let mut mtf: Vec<u8> = (0..groups as u8).collect();
    let mut last = 0;
    for _ in 0..selectors {
        let mut j = 0;
        while take(1)? == 1 {
            j += 1;
            if j >= groups {
                return None;
            }
        }
        let v = mtf.remove(j);
        mtf.insert(0, v);
        last = v;
    }
    for t in 0..groups {
        let mut cur = take(5)? as i64;
        for _ in 0..alpha {
            while take(1)? == 1 {
                if take(1)? == 0 {
                    cur += 1;
                } else {
                    cur -= 1;
                }
            }
        }
        if t == last as usize {
            return Some(cur as u32);
        }
    }
    None
}

fn get_bits(data: &[u8], pos: usize, n: usize) -> u64 {
    let mut v = 0u64;
    for i in pos..pos + n {
        v = (v << 1) | ((data[i / 8] >> (7 - (i % 8))) & 1) as u64;
    }
    v
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CMode {
    Running,
    Flushing,
    Finishing,
    Ended,
}

/// BZ2_bzCompressInit() & co.
pub struct Compress {
    level: u8,
    nblock_max: usize,
    /// Raw input bytes of the current block (the flushed runs).
    raw: Vec<u8>,
    /// RLE-1 length of `raw` (libbzip2's nblock).
    nblock: usize,
    /// The pending run (state_in_ch / state_in_len).
    run_ch: Option<u8>,
    run_len: usize,
    header_done: bool,
    combined_crc: u32,
    bits: BitSink,
    out: Vec<u8>,
    out_pos: usize,
    mode: CMode,
    total_in: u64,
    total_out: u64,
}

impl Compress {
    /// `block_size` (1..=9) in 100k units. The work factor only selects
    /// libbzip2's sorting algorithm and never changes the output.
    pub fn new(block_size: u32, _work_factor: u32) -> Compress {
        let level = block_size.clamp(1, 9) as u8;
        Compress {
            level,
            nblock_max: 100_000 * level as usize - 19,
            raw: Vec::new(),
            nblock: 0,
            run_ch: None,
            run_len: 0,
            header_done: false,
            combined_crc: 0,
            bits: BitSink { acc: 0, live: 0 },
            out: Vec::new(),
            out_pos: 0,
            mode: CMode::Running,
            total_in: 0,
            total_out: 0,
        }
    }

    pub fn total_in(&self) -> u64 {
        self.total_in
    }

    pub fn total_out(&self) -> u64 {
        self.total_out
    }

    /// add_pair_to_block()
    fn add_pair(&mut self) {
        if let Some(ch) = self.run_ch {
            let n = self.run_len;
            self.raw.extend(std::iter::repeat_n(ch, n));
            self.nblock += if n < 4 { n } else { 5 };
        }
    }

    /// ADD_CHAR_TO_BLOCK()
    fn add_char(&mut self, c: u8) {
        match self.run_ch {
            Some(ch) if ch == c && self.run_len < 255 => self.run_len += 1,
            _ => {
                self.add_pair();
                self.run_ch = Some(c);
                self.run_len = 1;
            }
        }
    }

    /// flush_RL()
    fn flush_rl(&mut self) {
        self.add_pair();
        self.run_ch = None;
        self.run_len = 0;
    }

    /// copy_input_until_stop(), compressing each full block.
    fn absorb(&mut self, input: &[u8]) {
        for &c in input {
            if self.nblock >= self.nblock_max {
                self.compress_block(false);
            }
            self.add_char(c);
        }
        self.total_in += input.len() as u64;
    }

    /// BZ2_compressBlock()
    fn compress_block(&mut self, last: bool) {
        if !self.header_done {
            self.header_done = true;
            for &b in b"BZh" {
                self.bits.put(&mut self.out, 8, b as u32);
            }
            self.bits.put(&mut self.out, 8, (b'0' + self.level) as u32);
        }
        if !self.raw.is_empty() {
            let raw = std::mem::take(&mut self.raw);
            self.emit_block(&raw);
            self.raw = raw;
            self.raw.clear();
            self.nblock = 0;
        }
        if last {
            let out = &mut self.out;
            self.bits.put(out, 24, (EOS_MAGIC >> 24) as u32);
            self.bits.put(out, 24, (EOS_MAGIC & 0xff_ffff) as u32);
            self.bits.put(out, 16, self.combined_crc >> 16);
            self.bits.put(out, 16, self.combined_crc & 0xffff);
            self.bits.finish(out);
        }
    }

    /// Encode `raw` as one block with compcol and append its bits.
    fn emit_block(&mut self, raw: &[u8]) {
        let mut enc =
            compcol::bzip2::Encoder::with_config(compcol::bzip2::EncoderConfig { level: 9 });
        let mut z = Vec::with_capacity(raw.len() / 2 + 128);
        let mut buf = vec![0u8; 1 << 16];
        let mut pos = 0;
        while pos < raw.len() {
            let (p, _) = enc
                .encode(&raw[pos..], &mut buf)
                .expect("bzip2 encoding cannot fail");
            z.extend_from_slice(&buf[..p.written]);
            pos += p.consumed;
        }
        loop {
            let (p, s) = enc.finish(&mut buf).expect("bzip2 encoding cannot fail");
            z.extend_from_slice(&buf[..p.written]);
            if s == CStatus::StreamEnd {
                break;
            }
        }
        // z: "BZh9", block magic, block CRC, ..., EOS magic, combined CRC,
        // zero padding.
        debug_assert!(z.len() >= 4 + 10 + 10);
        debug_assert_eq!(get_bits(&z, 32, 48), BLOCK_MAGIC);
        let block_crc = u32::from_be_bytes([z[10], z[11], z[12], z[13]]);
        let total = z.len() * 8;
        // Find the trailer: pad bits are zero; prefer the reading whose
        // combined CRC is the block CRC (a single block).
        let mut end = None;
        for pad in 0..8 {
            let t = total - pad - 80;
            if get_bits(&z, total - pad, pad) == 0 && get_bits(&z, t, 48) == EOS_MAGIC {
                let crc = get_bits(&z, t + 48, 32) as u32;
                if crc == block_crc {
                    end = Some((t, 1u32, crc));
                    break;
                }
                end.get_or_insert((t, 2, crc));
            }
        }
        let (block_end, nblocks, crc) = end.expect("bzip2 trailer");
        // compcol split the block in two (a run straddling the end of a
        // full 900k block): the stream stays valid, fold both CRCs.
        self.combined_crc = self.combined_crc.rotate_left(nblocks) ^ crc;
        // the block's bits: all but the last code written at once, then that
        // last code (the end-of-block symbol)
        let bits = block_end - 32;
        let last = if nblocks == 1 {
            eob_code_len(&z, 32 + 48).unwrap_or(0) as usize
        } else {
            0
        }
        .min(bits);
        let out = &mut self.out;
        let bulk = bits - last;
        for &b in &z[4..4 + bulk / 8] {
            self.bits.put(out, 8, b as u32);
        }
        let mut p = 32 + bulk / 8 * 8;
        for n in [bulk % 8, last] {
            let mut n = n;
            while n > 0 {
                let k = n.min(24);
                self.bits.put(out, k as u32, get_bits(&z, p, k) as u32);
                p += k;
                n -= k;
            }
        }
    }

    fn drain(&mut self, output: &mut [u8]) -> usize {
        let n = (self.out.len() - self.out_pos).min(output.len());
        output[..n].copy_from_slice(&self.out[self.out_pos..self.out_pos + n]);
        self.out_pos += n;
        if self.out_pos == self.out.len() {
            self.out.clear();
            self.out_pos = 0;
        }
        self.total_out += n as u64;
        n
    }

    fn pending(&self) -> bool {
        self.out_pos < self.out.len()
    }

    /// BZ2_bzCompress(): all of `input` is consumed by the first call of a
    /// flush/finish; its later calls (to drain the output) must pass no
    /// input.
    pub fn compress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        action: Action,
    ) -> Result<Status, Error> {
        match (self.mode, action) {
            (CMode::Running, Action::Run) => {
                // handle_compress(): no input is taken while compressed
                // output is pending.
                let mut done = self.drain(output);
                if self.pending() {
                    return Ok(Status::RunOk);
                }
                for (i, &c) in input.iter().enumerate() {
                    if self.nblock >= self.nblock_max {
                        self.compress_block(false);
                        done += self.drain(&mut output[done..]);
                        if self.pending() {
                            self.total_in += i as u64;
                            return Ok(Status::RunOk);
                        }
                    }
                    self.add_char(c);
                }
                self.total_in += input.len() as u64;
                if self.nblock >= self.nblock_max {
                    self.compress_block(false);
                    self.drain(&mut output[done..]);
                }
                Ok(Status::RunOk)
            }
            (CMode::Running, Action::Flush) | (CMode::Running, Action::Finish) => {
                self.absorb(input);
                self.flush_rl();
                let last = action == Action::Finish;
                self.compress_block(last);
                self.mode = if last {
                    CMode::Finishing
                } else {
                    CMode::Flushing
                };
                self.compress(&[], output, action)
            }
            (CMode::Flushing, Action::Flush) => {
                if !input.is_empty() {
                    return Err(Error::Sequence);
                }
                self.drain(output);
                if self.pending() {
                    Ok(Status::FlushOk)
                } else {
                    self.mode = CMode::Running;
                    Ok(Status::RunOk)
                }
            }
            (CMode::Finishing, Action::Finish) => {
                if !input.is_empty() {
                    return Err(Error::Sequence);
                }
                self.drain(output);
                if self.pending() {
                    Ok(Status::FinishOk)
                } else {
                    self.mode = CMode::Ended;
                    Ok(Status::StreamEnd)
                }
            }
            _ => Err(Error::Sequence),
        }
    }
}

/// One-shot compression (BZ2_bzBuffToBuffCompress()).
pub fn compress(data: &[u8], block_size: u32, work_factor: u32) -> Vec<u8> {
    let mut c = Compress::new(block_size, work_factor);
    c.absorb(data);
    c.flush_rl();
    c.compress_block(true);
    std::mem::take(&mut c.out)
}

/// BZ_RUN over all of `data`, then BZ_FINISH (what BZ2_bzWrite() and
/// BZ2_bzWriteClose() or libzip do).
pub fn compress_run_finish(data: &[u8], block_size: u32, work_factor: u32) -> Vec<u8> {
    let mut c = Compress::new(block_size, work_factor);
    c.absorb(data);
    if c.nblock >= c.nblock_max {
        c.compress_block(false);
    }
    c.flush_rl();
    c.compress_block(true);
    std::mem::take(&mut c.out)
}

/// BZ2_bzDecompressInit() & co.
pub struct Decompress {
    dec: compcol::bzip2::Decoder,
    /// The stream header bytes checked so far.
    magic: usize,
    /// All input of this stream, when the end must be located.
    hist: Option<Vec<u8>>,
    ended: bool,
    total_in: u64,
    total_out: u64,
}

impl Decompress {
    /// `small` (BZ2_bzDecompressInit's small) only lowers libbzip2's memory
    /// use. `track_end`: report the exact end of the stream (the input after
    /// it left unconsumed), at the price of keeping the stream's input.
    pub fn new(_small: bool, track_end: bool) -> Decompress {
        Decompress {
            dec: compcol::bzip2::Decoder::new(),
            magic: 0,
            hist: track_end.then(Vec::new),
            ended: false,
            total_in: 0,
            total_out: 0,
        }
    }

    pub fn total_in(&self) -> u64 {
        self.total_in
    }

    pub fn total_out(&self) -> u64 {
        self.total_out
    }

    /// Where the stream in `hist` ends: after the first end-of-stream
    /// trailer (48-bit magic, 32-bit CRC, zero padding to a byte).
    fn stream_len(hist: &[u8]) -> Option<usize> {
        let mut win = 0u64;
        let mut nbits = 0usize;
        for (i, &byte) in hist.iter().enumerate().skip(4) {
            for k in (0..8).rev() {
                win = (win << 1) | ((byte >> k) & 1) as u64;
                nbits += 1;
                if nbits >= 48 && win & 0xffff_ffff_ffff == EOS_MAGIC {
                    // the magic ends at bit `pos` (exclusive)
                    let pos = i * 8 + (8 - k);
                    let end = (pos + 32).div_ceil(8);
                    if end <= hist.len() && get_bits(hist, pos + 32, end * 8 - pos - 32) == 0 {
                        return Some(end);
                    }
                }
            }
        }
        None
    }

    /// BZ2_bzDecompress(): Ok while more input is needed or output is
    /// pending, StreamEnd once the stream is complete and drained (all of
    /// `input` is consumed otherwise).
    pub fn decompress(&mut self, input: &[u8], output: &mut [u8]) -> Result<Status, Error> {
        if self.ended {
            return Err(Error::Sequence);
        }
        // BZ_X_MAGIC_1..4
        for &b in input.iter().take(4 - self.magic) {
            let ok = match self.magic {
                0 => b == b'B',
                1 => b == b'Z',
                2 => b == b'h',
                _ => (b'1'..=b'9').contains(&b),
            };
            if !ok {
                return Err(Error::DataMagic);
            }
            self.magic += 1;
        }
        if let Some(h) = self.hist.as_mut() {
            h.extend_from_slice(input);
        }
        let (p, s) = self.dec.decode(input, output).map_err(|_| Error::Data)?;
        self.total_out += p.written as u64;
        if s == CStatus::StreamEnd {
            self.ended = true;
            let consumed = match self.hist.take() {
                Some(h) => {
                    let before = h.len() - input.len();
                    Self::stream_len(&h)
                        .map(|end| end.saturating_sub(before).min(input.len()))
                        .unwrap_or(input.len())
                }
                None => input.len(),
            };
            self.total_in += consumed as u64;
            return Ok(Status::StreamEnd);
        }
        self.total_in += p.consumed as u64;
        Ok(Status::Ok)
    }

    /// No more input will come: writes decoded bytes still pending to
    /// `output` (call again while it fills) and tells whether the stream
    /// ended (else it is truncated once nothing more is written).
    pub fn finish(&mut self, output: &mut [u8]) -> Result<(usize, bool), Error> {
        if self.ended {
            return Ok((0, true));
        }
        if self.magic < 4 {
            return Ok((0, false));
        }
        let before = self.total_out;
        let st = self.decompress(&[], output)?;
        Ok(((self.total_out - before) as usize, st == Status::StreamEnd))
    }
}

/// One-shot decompression of the first stream of `data`: the data decoded
/// and Ok(true) when the stream ended, Ok(false) when it is truncated, or
/// the libbzip2 error.
pub fn decompress(data: &[u8]) -> (Vec<u8>, Result<bool, Error>) {
    let mut d = Decompress::new(false, false);
    let mut out = Vec::with_capacity(data.len().saturating_mul(3));
    let mut buf = vec![0u8; 1 << 16];
    let mut input = data;
    loop {
        let produced = d.total_out();
        let st = match d.decompress(input, &mut buf) {
            Ok(st) => st,
            Err(e) => return (out, Err(e)),
        };
        let n = (d.total_out() - produced) as usize;
        out.extend_from_slice(&buf[..n]);
        input = &[];
        if st == Status::StreamEnd {
            return (out, Ok(true));
        }
        if n < buf.len() {
            break;
        }
    }
    loop {
        match d.finish(&mut buf) {
            Ok((n, done)) => {
                out.extend_from_slice(&buf[..n]);
                if done {
                    return (out, Ok(true));
                }
                if n == 0 {
                    return (out, Ok(false));
                }
            }
            Err(e) => return (out, Err(e)),
        }
    }
}
