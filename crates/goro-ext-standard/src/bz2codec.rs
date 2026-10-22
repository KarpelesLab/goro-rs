//! bzip2 with libbzip2's streaming interface (BZ2_bzCompress /
//! BZ2_bzDecompress), on top of compcol's bzip2 codec.
//!
//! compcol's encoder produces libbzip2's output byte for byte for BZ_RUN
//! then BZ_FINISH (its `encode` / `finish`) and for BZ_FLUSH (its
//! `flush`). One case is left to this layer: BZ_FLUSH / BZ_FINISH given
//! input (what BZ2_bzBuffToBuffCompress() does with all of it) do not
//! close the block after the last byte of that input even when it filled
//! the block (`handle_compress` checks `avail_in_expect == 0` first): the
//! pending run then joins the block, which compcol's `encode` would have
//! closed without it. So the blocks are cut here, the way libbzip2's
//! `copy_input_until_stop` does, by tracking the block's RLE-1 length:
//! the encoder runs at block size 9 (only the stream header depends on
//! the block size, and it is rewritten) and is flushed just before the
//! byte that fills a block, which closes the same block. (At block size 9
//! compcol cannot hold such a block: it closes it before the last run.)
//!
//! The decoder side runs compcol in single-stream mode, which stops at
//! the end of the stream and leaves the input after it unconsumed, like
//! BZ2_bzDecompress().

use compcol::{Decoder as _, Encoder as _, Status as CStatus};

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
    enc: compcol::bzip2::Encoder,
    /// libbzip2's nblockMAX.
    nblock_max: usize,
    /// RLE-1 length of the current block, the pending run excluded
    /// (libbzip2's nblock).
    nblock: usize,
    /// The pending run (state_in_ch / state_in_len).
    run_ch: Option<u8>,
    run_len: usize,
    /// Bytes taken from the encoder so far (the 4th is the block size).
    enc_out: u64,
    buf: Vec<u8>,
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
            enc: compcol::bzip2::Encoder::with_config(compcol::bzip2::EncoderConfig { level: 9 }),
            nblock_max: 100_000 * level as usize - 19,
            nblock: 0,
            run_ch: None,
            run_len: 0,
            enc_out: 0,
            buf: vec![0u8; 1 << 16],
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

    /// ADD_CHAR_TO_BLOCK() on the block length: whether the block is full
    /// after `c`.
    fn add_char(&mut self, c: u8) -> bool {
        match self.run_ch {
            Some(ch) if ch == c && self.run_len < 255 => self.run_len += 1,
            _ => {
                if self.run_ch.is_some() {
                    // add_pair_to_block()
                    let n = self.run_len;
                    self.nblock += if n < 4 { n } else { 5 };
                }
                self.run_ch = Some(c);
                self.run_len = 1;
            }
        }
        self.nblock >= self.nblock_max
    }

    /// Move the encoder's output (the first `n` bytes of `buf`) to `out`.
    fn take(&mut self, n: usize) {
        let start = self.out.len();
        self.out.extend_from_slice(&self.buf[..n]);
        if self.enc_out < 4 && self.enc_out + n as u64 >= 4 {
            // "BZh9" -> "BZh<level>"
            self.out[start + (3 - self.enc_out) as usize] = b'0' + self.level;
        }
        self.enc_out += n as u64;
    }

    fn enc_encode(&mut self, mut input: &[u8]) {
        while !input.is_empty() {
            let (p, _) = self
                .enc
                .encode(input, &mut self.buf)
                .expect("bzip2 encoding cannot fail");
            self.take(p.written);
            input = &input[p.consumed..];
        }
    }

    /// End the current block (BZ_FLUSH), or the stream when `last`.
    fn enc_end_block(&mut self, last: bool) {
        loop {
            let r = if last {
                self.enc.finish(&mut self.buf)
            } else {
                self.enc.flush(&mut self.buf, compcol::Flush::Sync)
            };
            let (p, s) = r.expect("bzip2 encoding cannot fail");
            self.take(p.written);
            if s == CStatus::StreamEnd || (!last && p.written < self.buf.len()) {
                break;
            }
        }
        self.nblock = 0;
        self.run_ch = None;
        self.run_len = 0;
    }

    /// copy_input_until_stop() over `input`, closing each block that
    /// fills; `ending` (BZ_FLUSH / BZ_FINISH): the last byte does not
    /// close its block. With `output`, stops after closing a block whose
    /// output does not fit it. Returns the bytes taken.
    fn feed(
        &mut self,
        input: &[u8],
        ending: bool,
        mut output: Option<(&mut [u8], &mut usize)>,
    ) -> usize {
        let mut start = 0;
        for (i, &c) in input.iter().enumerate() {
            if self.add_char(c) && !(ending && i + 1 == input.len()) {
                // libbzip2 closes the block once `c` is in, `c` starting
                // the run of the next block: flush just before it
                self.enc_encode(&input[start..i]);
                self.enc_end_block(false);
                self.run_ch = Some(c);
                self.run_len = 1;
                start = i;
                if let Some((out, done)) = output.as_mut() {
                    **done += self.drain(&mut out[**done..]);
                    if self.pending() {
                        self.enc_encode(&input[i..i + 1]);
                        self.total_in += i as u64 + 1;
                        return i + 1;
                    }
                }
            }
        }
        self.enc_encode(&input[start..]);
        self.total_in += input.len() as u64;
        input.len()
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
                self.feed(input, false, Some((output, &mut done)));
                Ok(Status::RunOk)
            }
            (CMode::Running, Action::Flush) | (CMode::Running, Action::Finish) => {
                let last = action == Action::Finish;
                self.feed(input, true, None);
                self.enc_end_block(last);
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
    c.feed(data, true, None);
    c.enc_end_block(true);
    std::mem::take(&mut c.out)
}

/// BZ_RUN over all of `data`, then BZ_FINISH (what BZ2_bzWrite() and
/// BZ2_bzWriteClose() or libzip do).
pub fn compress_run_finish(data: &[u8], block_size: u32, work_factor: u32) -> Vec<u8> {
    let mut c = Compress::new(block_size, work_factor);
    c.feed(data, false, None);
    c.enc_end_block(true);
    std::mem::take(&mut c.out)
}

/// BZ2_bzDecompressInit() & co.
pub struct Decompress {
    dec: compcol::bzip2::Decoder,
    /// The stream header bytes checked so far.
    magic: usize,
    ended: bool,
    total_in: u64,
    total_out: u64,
}

impl Decompress {
    /// `small` (BZ2_bzDecompressInit's small) only lowers libbzip2's memory
    /// use.
    pub fn new(_small: bool) -> Decompress {
        Decompress {
            dec: compcol::bzip2::Decoder::with_config(
                compcol::bzip2::DecoderConfig::default().with_multi_stream(false),
            ),
            magic: 0,
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

    /// BZ2_bzDecompress(): Ok while more input is needed or output is
    /// pending, StreamEnd once the stream is complete and drained. Like
    /// libbzip2 it may leave input unconsumed (see `total_in`): what
    /// follows the end of the stream, or when the output is full. No
    /// input means that no more will come.
    pub fn decompress(&mut self, input: &[u8], output: &mut [u8]) -> Result<Status, Error> {
        if self.ended {
            return Err(Error::Sequence);
        }
        // BZ_X_MAGIC_1..4
        for (i, &b) in input.iter().take(4 - self.magic).enumerate() {
            let ok = match self.magic + i {
                0 => b == b'B',
                1 => b == b'Z',
                2 => b == b'h',
                _ => (b'1'..=b'9').contains(&b),
            };
            if !ok {
                return Err(Error::DataMagic);
            }
        }
        let r = if input.is_empty() {
            match self.dec.finish(output) {
                // truncated: nothing more to decode
                Err(compcol::Error::UnexpectedEnd) => return Ok(Status::Ok),
                r => r,
            }
        } else {
            self.dec.decode(input, output)
        };
        let (mut p, mut s) = r.map_err(|_| Error::Data)?;
        if s != CStatus::StreamEnd && !output.is_empty() && p.written == output.len() {
            // compcol stops once the output is full; libbzip2 goes on
            // parsing, so that the call writing the last bytes reports the
            // end of the stream when its trailer follows.
            let rest = &input[p.consumed..];
            let r = if input.is_empty() {
                self.dec.finish(&mut [])
            } else {
                self.dec.decode(rest, &mut [])
            };
            if let Ok((q, t)) = r {
                p.consumed += q.consumed;
                s = t;
            }
        }
        self.magic = (self.magic + p.consumed).min(4);
        self.total_in += p.consumed as u64;
        self.total_out += p.written as u64;
        if s == CStatus::StreamEnd {
            self.ended = true;
            return Ok(Status::StreamEnd);
        }
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
    let mut d = Decompress::new(false);
    let mut out = Vec::with_capacity(data.len().saturating_mul(3));
    let mut buf = vec![0u8; 1 << 16];
    let mut input = data;
    while !input.is_empty() {
        let (in0, out0) = (d.total_in(), d.total_out());
        let st = match d.decompress(input, &mut buf) {
            Ok(st) => st,
            Err(e) => return (out, Err(e)),
        };
        let n = (d.total_out() - out0) as usize;
        let used = (d.total_in() - in0) as usize;
        out.extend_from_slice(&buf[..n]);
        input = &input[used..];
        if st == Status::StreamEnd {
            return (out, Ok(true));
        }
        if n == 0 && used == 0 {
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
