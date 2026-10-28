//! The format layer under `gzopen()` and the `compress.zlib://` wrapper:
//! a gzip file reader and writer over any byte source or sink, following
//! the zlib manual's description of `gzopen`/`gzread`/`gzwrite`/`gzseek`/
//! `gztell`/`gzeof`/`gzrewind`/`gzflush`/`gzclose`.
//!
//! - Reading decompresses concatenated gzip members, copies a file that
//!   does not start with the gzip magic number as is, ignores anything that
//!   is not a gzip member after one, and returns what it has when a member
//!   is cut short (no error). Corrupt data is an error.
//! - Writing compresses into one gzip member (memory level 8). PHP flushes
//!   a stream it wrote to before closing it, so the engine calls
//!   [`GzWriter::flush`] with [`Flush::Sync`] then [`GzWriter::close`]
//!   (observed on PHP 8.5: the file ends with a sync marker and an empty
//!   final block).
//! - Seeking is forward only when writing (the gap is filled with zero
//!   bytes) and emulated when reading (backwards means starting over).
//!
//! Input is read and compressed in 8 KiB units like zlib's default buffer
//! size, which matters for level 0's stored blocks (observed on PHP 8.5).

use crate::deflate::{DeflateParams, DeflateStatus, Deflater, Flush, Strategy, Wrap};
use crate::inflate::{InflateFlush, InflateStatus, InflateWrap, Inflater};
use std::io::{self, Read, Seek, SeekFrom, Write};

const BUF: usize = 8192;

/// A parsed `gzopen()` mode string (zlib manual: gzopen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GzMode {
    /// True for `r`, false for `w` and `a`.
    pub read: bool,
    pub append: bool,
    /// -1 for the default.
    pub level: i32,
    pub strategy: Strategy,
    /// `T`: write without compression.
    pub transparent: bool,
    /// `x`: create the file, failing if it exists.
    pub exclusive: bool,
}

/// Why `gzopen()` rejects a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GzModeError {
    /// `+` in the mode: "Cannot open a zlib stream for reading and writing
    /// at the same time!" (checked before the file is opened).
    ReadWrite,
    /// No `r`, `w` or `a`, or `T` with `r`: "gzopen failed" (after the
    /// file was opened).
    Invalid,
}

impl GzModeError {
    /// PHP's warning, without the `gzopen(): ` prefix.
    pub fn message(self) -> &'static str {
        match self {
            GzModeError::ReadWrite => {
                "Cannot open a zlib stream for reading and writing at the same time!"
            }
            GzModeError::Invalid => "gzopen failed",
        }
    }
}

/// Parses a `gzopen()` mode: `r`/`w`/`a` (the last one wins), a digit for
/// the level, `f`/`h`/`R`/`F` for the filtered, Huffman-only, RLE and
/// fixed strategies, `T` for transparent writing, `x` for exclusive
/// creation; other characters (`b`, `e`, ...) are ignored.
pub fn parse_mode(mode: &[u8]) -> Result<GzMode, GzModeError> {
    if mode.contains(&b'+') {
        return Err(GzModeError::ReadWrite);
    }
    let mut m = GzMode {
        read: false,
        append: false,
        level: -1,
        strategy: Strategy::Default,
        transparent: false,
        exclusive: false,
    };
    let mut have = false;
    for &c in mode {
        match c {
            b'0'..=b'9' => m.level = (c - b'0') as i32,
            b'r' => {
                m.read = true;
                m.append = false;
                have = true;
            }
            b'w' => {
                m.read = false;
                m.append = false;
                have = true;
            }
            b'a' => {
                m.read = false;
                m.append = true;
                have = true;
            }
            b'x' => m.exclusive = true,
            b'f' => m.strategy = Strategy::Filtered,
            b'h' => m.strategy = Strategy::HuffmanOnly,
            b'R' => m.strategy = Strategy::Rle,
            b'F' => m.strategy = Strategy::Fixed,
            b'T' => m.transparent = true,
            _ => {}
        }
    }
    if !have || (m.read && m.transparent) {
        return Err(GzModeError::Invalid);
    }
    Ok(m)
}

/// Seek origin for [`GzReader::seek`] and [`GzWriter::seek`]; zlib does
/// not support seeking from the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Whence {
    Set,
    Cur,
}

/// An error that stops reading: corrupt data or an I/O failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GzError {
    /// `ZLIB_DATA_ERROR` or `ZLIB_ERRNO`.
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum How {
    Look,
    Copy,
    Gzip,
}

/// Reads a gzip (or plain) file.
pub struct GzReader<R: Read + Seek> {
    src: R,
    start: Option<u64>,
    inbuf: Vec<u8>,
    in_pos: usize,
    out: Vec<u8>,
    out_pos: usize,
    how: How,
    direct: bool,
    in_eof: bool,
    past: bool,
    pos: u64,
    skip: Option<u64>,
    inf: Inflater,
    err: Option<GzError>,
    /// The last member ended short of its trailer.
    short: bool,
}

impl<R: Read + Seek> GzReader<R> {
    /// Starts reading at the source's current position.
    pub fn new(mut src: R) -> Self {
        let start = src.stream_position().ok();
        GzReader {
            src,
            start,
            inbuf: Vec::with_capacity(BUF),
            in_pos: 0,
            out: Vec::new(),
            out_pos: 0,
            how: How::Look,
            direct: true,
            in_eof: false,
            past: false,
            pos: 0,
            skip: None,
            inf: Inflater::new(InflateWrap::Gzip, 15),
            err: None,
            short: false,
        }
    }

    /// The underlying source.
    pub fn into_inner(self) -> R {
        self.src
    }

    fn avail_in(&self) -> usize {
        self.inbuf.len() - self.in_pos
    }

    fn have(&self) -> usize {
        self.out.len() - self.out_pos
    }

    fn fail(&mut self, code: i64, msg: impl Into<String>) {
        self.err = Some(GzError {
            code,
            message: msg.into(),
        });
        self.out.clear();
        self.out_pos = 0;
    }

    /// Fills the input buffer up to 8 KiB, keeping unread bytes.
    fn avail(&mut self) -> Result<(), ()> {
        if self.err.is_some() {
            return Err(());
        }
        if !self.in_eof {
            self.inbuf.drain(..self.in_pos);
            self.in_pos = 0;
            let mut tmp = [0u8; BUF];
            while self.inbuf.len() < BUF {
                let want = BUF - self.inbuf.len();
                match self.src.read(&mut tmp[..want]) {
                    Ok(0) => {
                        self.in_eof = true;
                        break;
                    }
                    Ok(n) => self.inbuf.extend_from_slice(&tmp[..n]),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        self.fail(-1, e.to_string());
                        return Err(());
                    }
                }
            }
        }
        Ok(())
    }

    /// Looks for a gzip header at the current input position.
    fn look(&mut self) -> Result<(), ()> {
        if self.avail_in() < 2 {
            self.avail()?;
            if self.avail_in() == 0 {
                return Ok(());
            }
        }
        let rest = &self.inbuf[self.in_pos..];
        if rest.len() > 1 && rest[0] == 0x1f && rest[1] == 0x8b {
            self.inf.reset();
            self.how = How::Gzip;
            self.direct = false;
            return Ok(());
        }
        if !self.direct {
            // Trailing garbage after a gzip member is ignored.
            self.in_pos = self.inbuf.len();
            self.in_eof = true;
            self.out.clear();
            self.out_pos = 0;
            return Ok(());
        }
        self.out.clear();
        self.out.extend_from_slice(rest);
        self.out_pos = 0;
        self.in_pos = self.inbuf.len();
        self.how = How::Copy;
        Ok(())
    }

    /// Decompresses into the output buffer, at most `room` bytes.
    fn decomp(&mut self, room: usize) -> Result<(), ()> {
        self.out.clear();
        self.out_pos = 0;
        let mut st = InflateStatus::Ok;
        while self.out.len() < room && st != InflateStatus::StreamEnd {
            if self.avail_in() == 0 {
                self.avail()?;
            }
            if self.avail_in() == 0 {
                // Cut short: keep what was decoded, like zlib's
                // "unexpected end of file" (not an error for gzread).
                self.short = true;
                break;
            }
            let lim = room - self.out.len();
            let (n, s) = self.inf.inflate(
                &self.inbuf[self.in_pos..],
                &mut self.out,
                lim,
                InflateFlush::None,
            );
            self.in_pos += n;
            st = s;
            match st {
                InflateStatus::DataError(m) => {
                    self.fail(-3, m);
                    return Err(());
                }
                InflateStatus::NeedDict => {
                    self.fail(-2, "internal error: inflate stream corrupt");
                    return Err(());
                }
                _ => {}
            }
        }
        if st == InflateStatus::StreamEnd {
            self.how = How::Look;
        }
        Ok(())
    }

    /// Gets more output into the buffer.
    fn fetch(&mut self) -> Result<(), ()> {
        loop {
            match self.how {
                How::Look => {
                    self.look()?;
                    if self.how == How::Look {
                        return Ok(());
                    }
                }
                How::Copy => {
                    self.out.clear();
                    self.out_pos = 0;
                    let mut tmp = vec![0u8; 2 * BUF];
                    let mut got = 0;
                    while got < tmp.len() {
                        match self.src.read(&mut tmp[got..]) {
                            Ok(0) => {
                                self.in_eof = true;
                                break;
                            }
                            Ok(n) => got += n,
                            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                            Err(e) => {
                                self.fail(-1, e.to_string());
                                return Err(());
                            }
                        }
                    }
                    self.out.extend_from_slice(&tmp[..got]);
                    return Ok(());
                }
                How::Gzip => self.decomp(2 * BUF)?,
            }
            if !(self.have() == 0 && (!self.in_eof || self.avail_in() > 0)) {
                return Ok(());
            }
        }
    }

    fn skip_bytes(&mut self, mut len: u64) -> Result<(), ()> {
        while len > 0 {
            if self.have() > 0 {
                let n = (self.have() as u64).min(len) as usize;
                self.out_pos += n;
                self.pos += n as u64;
                len -= n as u64;
            } else if self.in_eof && self.avail_in() == 0 {
                break;
            } else {
                self.fetch()?;
            }
        }
        Ok(())
    }

    fn fatal(&self) -> bool {
        self.err.is_some()
    }

    /// `gzread()`: reads up to `buf.len()` uncompressed bytes. `Ok(0)` at
    /// the end of the data; an error for corrupt data. Data read before an
    /// error is returned first, the error on the next call.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, GzError> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        if buf.is_empty() {
            return Ok(0);
        }
        if let Some(s) = self.skip.take()
            && self.skip_bytes(s).is_err()
        {
            return Err(self.err.clone().unwrap());
        }
        let mut got = 0;
        while got < buf.len() {
            if self.have() > 0 {
                let n = self.have().min(buf.len() - got);
                buf[got..got + n].copy_from_slice(&self.out[self.out_pos..self.out_pos + n]);
                self.out_pos += n;
                got += n;
                self.pos += n as u64;
            } else if self.in_eof && self.avail_in() == 0 {
                self.past = true;
                break;
            } else if self.fetch().is_err() {
                break;
            }
        }
        if got == 0
            && let Some(e) = &self.err
        {
            return Err(e.clone());
        }
        Ok(got)
    }

    /// `gzeof()`: true once a read went past the end of the data.
    pub fn eof(&self) -> bool {
        self.past
    }

    /// True when the data ended in the middle of a gzip member.
    pub fn truncated(&self) -> bool {
        self.short
    }

    /// `gztell()`: the position in the uncompressed data.
    pub fn tell(&self) -> i64 {
        (self.pos + self.skip.unwrap_or(0)) as i64
    }

    /// `gzrewind()`: back to the start (the source must be seekable).
    pub fn rewind(&mut self) -> Result<(), GzError> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        let unseekable = || GzError {
            code: -1,
            message: "cannot seek".into(),
        };
        let start = self.start.ok_or_else(unseekable)?;
        self.src
            .seek(SeekFrom::Start(start))
            .map_err(|_| unseekable())?;
        self.out.clear();
        self.out_pos = 0;
        self.in_eof = false;
        self.past = false;
        self.how = How::Look;
        self.skip = None;
        self.err = None;
        self.short = false;
        self.pos = 0;
        self.inbuf.clear();
        self.in_pos = 0;
        Ok(())
    }

    /// `gzseek()`: returns the new position, or -1. Seeking backwards
    /// starts over from the beginning; seeking forward is done lazily on
    /// the next read, so a position past the end is accepted.
    pub fn seek(&mut self, offset: i64, whence: Whence) -> i64 {
        if self.fatal() {
            return -1;
        }
        let mut offset = match whence {
            Whence::Set => offset - self.pos as i64,
            Whence::Cur => offset + self.skip.unwrap_or(0) as i64,
        };
        self.skip = None;
        if self.how == How::Copy && self.pos as i64 + offset >= 0 {
            // Plain file: seek the source directly.
            let rel = offset - self.have() as i64;
            if self.src.seek(SeekFrom::Current(rel)).is_err() {
                return -1;
            }
            self.out.clear();
            self.out_pos = 0;
            self.in_eof = false;
            self.past = false;
            self.err = None;
            self.inbuf.clear();
            self.in_pos = 0;
            self.pos = (self.pos as i64 + offset) as u64;
            return self.pos as i64;
        }
        if offset < 0 {
            offset += self.pos as i64;
            if offset < 0 {
                return -1;
            }
            if self.rewind().is_err() {
                return -1;
            }
        }
        let n = (self.have() as i64).min(offset) as usize;
        self.out_pos += n;
        self.pos += n as u64;
        offset -= n as i64;
        if offset > 0 {
            self.skip = Some(offset as u64);
        }
        self.pos as i64 + offset
    }
}

/// Writes a gzip file.
pub struct GzWriter<W: Write> {
    sink: W,
    d: Option<Deflater>,
    level: i32,
    strategy: Strategy,
    direct: bool,
    /// Buffered input (zlib's 8 KiB input buffer).
    inbuf: Vec<u8>,
    /// Space left in the 8 KiB output buffer, and output not yet written.
    avail_out: usize,
    out: Vec<u8>,
    pos: u64,
    skip: Option<u64>,
    reset: bool,
    err: Option<io::Error>,
}

impl<W: Write> GzWriter<W> {
    /// A writer with the level (-1 for the default) and strategy of the
    /// mode; `transparent` writes the data uncompressed.
    pub fn new(sink: W, level: i32, strategy: Strategy, transparent: bool) -> Self {
        GzWriter {
            sink,
            d: None,
            level,
            strategy,
            direct: transparent,
            inbuf: Vec::with_capacity(BUF),
            avail_out: 0,
            out: Vec::new(),
            pos: 0,
            skip: None,
            reset: false,
            err: None,
        }
    }

    /// From a parsed write mode.
    pub fn from_mode(sink: W, mode: &GzMode) -> Self {
        Self::new(sink, mode.level, mode.strategy, mode.transparent)
    }

    fn init(&mut self) {
        if self.d.is_none() && !self.direct {
            self.d = Some(
                Deflater::new(DeflateParams {
                    level: self.level,
                    window_bits: 15,
                    mem_level: 8,
                    strategy: self.strategy,
                    wrap: Wrap::Gzip,
                })
                .expect("valid parameters"),
            );
            self.avail_out = BUF;
        }
    }

    fn write_out(&mut self) -> io::Result<()> {
        if !self.out.is_empty() {
            let r = self.sink.write_all(&self.out);
            self.out.clear();
            r?;
        }
        Ok(())
    }

    /// zlib's internal compression step: runs deflate on `input` until it
    /// produces no more output.
    fn comp(&mut self, input: &[u8], flush: Flush) -> io::Result<()> {
        self.init();
        if self.direct {
            return self.sink.write_all(input);
        }
        if self.reset {
            if input.is_empty() {
                return Ok(());
            }
            self.d.as_mut().unwrap().reset();
            self.reset = false;
        }
        let mut input = input;
        let mut ret = DeflateStatus::Ok;
        loop {
            if self.avail_out == 0
                || (flush != Flush::None
                    && (flush != Flush::Finish || ret == DeflateStatus::StreamEnd))
            {
                self.write_out()?;
                if self.avail_out == 0 {
                    self.avail_out = BUF;
                }
            }
            let have = self.avail_out;
            let d = self.d.as_mut().unwrap();
            ret = d.deflate_step(&mut input, flush, &mut self.out, &mut self.avail_out);
            if have == self.avail_out {
                break;
            }
        }
        if flush == Flush::Finish {
            self.reset = true;
        }
        Ok(())
    }

    /// Compresses the buffered input.
    fn comp_buffered(&mut self, flush: Flush) -> io::Result<()> {
        let buf = std::mem::take(&mut self.inbuf);
        let r = self.comp(&buf, flush);
        self.inbuf = buf;
        self.inbuf.clear();
        r
    }

    fn zero(&mut self, mut len: u64) -> io::Result<()> {
        if !self.inbuf.is_empty() {
            self.comp_buffered(Flush::None)?;
        }
        let zeros = vec![0u8; BUF];
        while len > 0 {
            let n = (len as usize).min(BUF);
            self.pos += n as u64;
            self.comp(&zeros[..n], Flush::None)?;
            len -= n as u64;
        }
        Ok(())
    }

    fn check(&self) -> io::Result<()> {
        match &self.err {
            Some(e) => Err(io::Error::new(e.kind(), e.to_string())),
            None => Ok(()),
        }
    }

    fn remember<T>(&mut self, r: io::Result<T>) -> io::Result<T> {
        if let Err(e) = &r {
            self.err = Some(io::Error::new(e.kind(), e.to_string()));
        }
        r
    }

    fn apply_skip(&mut self) -> io::Result<()> {
        if let Some(s) = self.skip.take() {
            let r = self.zero(s);
            self.remember(r)?;
        }
        Ok(())
    }

    /// `gzwrite()`: compresses `data`, returning the number of bytes taken
    /// (all of them unless an error occurred).
    pub fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.check()?;
        if data.is_empty() {
            return Ok(0);
        }
        self.init();
        self.apply_skip()?;
        if data.len() < BUF {
            let mut data = data;
            while !data.is_empty() {
                let copy = (BUF - self.inbuf.len()).min(data.len());
                self.inbuf.extend_from_slice(&data[..copy]);
                self.pos += copy as u64;
                data = &data[copy..];
                if !data.is_empty() {
                    let r = self.comp_buffered(Flush::None);
                    self.remember(r)?;
                }
            }
        } else {
            if !self.inbuf.is_empty() {
                let r = self.comp_buffered(Flush::None);
                self.remember(r)?;
            }
            self.pos += data.len() as u64;
            let r = self.comp(data, Flush::None);
            self.remember(r)?;
        }
        Ok(data.len())
    }

    /// `gzflush()` with a flush mode (`fflush()` on a gz stream uses
    /// [`Flush::Sync`]).
    pub fn flush(&mut self, flush: Flush) -> io::Result<()> {
        self.check()?;
        self.apply_skip()?;
        let r = self.comp_buffered(flush);
        self.remember(r)?;
        let r = self.write_out();
        self.remember(r)?;
        let r = self.sink.flush();
        self.remember(r)
    }

    /// `gztell()`.
    pub fn tell(&self) -> i64 {
        (self.pos + self.skip.unwrap_or(0)) as i64
    }

    /// `gzseek()` while writing: forward only, the gap reads back as zero
    /// bytes. Returns the new position or -1.
    pub fn seek(&mut self, offset: i64, whence: Whence) -> i64 {
        if self.err.is_some() {
            return -1;
        }
        let offset = match whence {
            Whence::Set => offset - self.pos as i64,
            Whence::Cur => offset + self.skip.unwrap_or(0) as i64,
        };
        self.skip = None;
        if offset < 0 {
            return -1;
        }
        if offset > 0 {
            self.skip = Some(offset as u64);
        }
        self.pos as i64 + offset
    }

    /// `gzclose()`: finishes the gzip member and returns the sink.
    pub fn close(mut self) -> io::Result<W> {
        self.check()?;
        self.apply_skip()?;
        self.comp_buffered(Flush::Finish)?;
        self.write_out()?;
        self.sink.flush()?;
        Ok(self.sink)
    }
}
