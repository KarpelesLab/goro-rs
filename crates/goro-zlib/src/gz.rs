//! gzlib.c / gzread.c / gzwrite.c from zlib 1.3.1: the gzFile layer over a
//! file descriptor (the subset PHP's zlib stream wrapper uses: gzdopen,
//! gzread, gzwrite, gzseek, gzrewind, gzeof, gzflush, gzsetparams, gzclose,
//! gzerror).

use crate::*;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

const GZBUFSIZE: usize = 8192;

#[derive(Clone, Copy, PartialEq, Eq)]
enum GzMode {
    Read,
    Write,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum How {
    Look,
    Copy,
    Gzip,
}

/// gz_state
pub struct GzFile {
    mode: GzMode,
    file: File,
    path: String,
    size: usize,
    want: usize,
    inbuf: Vec<u8>,
    outbuf: Vec<u8>,
    direct: bool,
    // reading
    how: How,
    start: i64,
    eof: bool,
    past: bool,
    // writing
    level: i32,
    strategy: i32,
    reset: bool,
    // seek request
    skip: i64,
    seek: bool,
    // error information
    err: i32,
    msg: Option<String>,
    // x.have / x.next / x.pos
    have: usize,
    next: usize,
    pos: i64,
    /// strm.next_in (offset into inbuf) / strm.avail_in
    in_next: usize,
    avail_in: usize,
    /// strm.next_out (offset into outbuf) when writing
    out_next: usize,
    inf: Option<Box<Inflate>>,
    def: Option<Box<Deflate>>,
}

fn errno_str(e: &std::io::Error) -> String {
    let s = e.to_string();
    // "No such file or directory (os error 2)" -> strerror() text
    match s.find(" (os error") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

impl GzFile {
    /// gzdopen(fd, mode): `file` is the (dup'ed) descriptor, `fd` its
    /// number for the "<fd:N>" path used in error messages.
    pub fn dopen(mut file: File, fd: i32, mode: &[u8]) -> Option<GzFile> {
        let mut gzmode = None;
        let mut append = false;
        let mut level = Z_DEFAULT_COMPRESSION;
        let mut strategy = Z_DEFAULT_STRATEGY;
        let mut direct = false;
        for &c in mode {
            if c.is_ascii_digit() {
                level = (c - b'0') as i32;
            } else {
                match c {
                    b'r' => gzmode = Some(GzMode::Read),
                    b'w' => {
                        gzmode = Some(GzMode::Write);
                        append = false;
                    }
                    b'a' => {
                        gzmode = Some(GzMode::Write);
                        append = true;
                    }
                    b'+' => return None,
                    b'f' => strategy = Z_FILTERED,
                    b'h' => strategy = Z_HUFFMAN_ONLY,
                    b'R' => strategy = Z_RLE,
                    b'F' => strategy = Z_FIXED,
                    b'T' => direct = true,
                    _ => {}
                }
            }
        }
        let gzmode = gzmode?;
        if gzmode == GzMode::Read {
            if direct {
                return None;
            }
            direct = true;
        }
        if append {
            let _ = file.seek(SeekFrom::End(0));
        }
        let mut start = 0i64;
        if gzmode == GzMode::Read {
            start = file.stream_position().map(|p| p as i64).unwrap_or(0);
        }
        let mut g = GzFile {
            mode: gzmode,
            file,
            path: format!("<fd:{}>", fd),
            size: 0,
            want: GZBUFSIZE,
            inbuf: Vec::new(),
            outbuf: Vec::new(),
            direct,
            how: How::Look,
            start,
            eof: false,
            past: false,
            level,
            strategy,
            reset: false,
            skip: 0,
            seek: false,
            err: Z_OK,
            msg: None,
            have: 0,
            next: 0,
            pos: 0,
            in_next: 0,
            avail_in: 0,
            out_next: 0,
            inf: None,
            def: None,
        };
        g.gz_reset();
        Some(g)
    }

    pub fn is_reading(&self) -> bool {
        self.mode == GzMode::Read
    }

    fn gz_reset(&mut self) {
        self.have = 0;
        if self.mode == GzMode::Read {
            self.eof = false;
            self.past = false;
            self.how = How::Look;
        } else {
            self.reset = false;
        }
        self.seek = false;
        self.gz_error(Z_OK, None);
        self.pos = 0;
        self.avail_in = 0;
    }

    fn gz_error(&mut self, err: i32, msg: Option<&str>) {
        self.msg = None;
        if err != Z_OK && err != Z_BUF_ERROR {
            self.have = 0;
        }
        self.err = err;
        let Some(msg) = msg else {
            return;
        };
        if err == Z_MEM_ERROR {
            return;
        }
        self.msg = Some(format!("{}: {}", self.path, msg));
    }

    /// gzerror(): (errnum, message)
    pub fn error(&self) -> (i32, String) {
        let m = if self.err == Z_MEM_ERROR {
            "out of memory".to_string()
        } else {
            self.msg.clone().unwrap_or_default()
        };
        (self.err, m)
    }

    /// gzeof()
    pub fn eof(&self) -> bool {
        self.mode == GzMode::Read && self.past
    }

    // ---------------------------------------------------------------- read

    /// gz_load() into `self.inbuf[off..off + len]` (or the output buffer).
    fn gz_load_into(&mut self, which: u8, off: usize, len: usize) -> Result<usize, ()> {
        let mut have = 0usize;
        let mut last: std::io::Result<usize> = Ok(1);
        while have < len {
            let buf = match which {
                0 => &mut self.inbuf[off + have..off + len],
                _ => &mut self.outbuf[off + have..off + len],
            };
            last = self.file.read(buf);
            match last {
                Ok(0) | Err(_) => break,
                Ok(n) => have += n,
            }
        }
        match last {
            Err(e) => {
                let m = errno_str(&e);
                self.gz_error(Z_ERRNO, Some(&m));
                Err(())
            }
            Ok(0) => {
                self.eof = true;
                Ok(have)
            }
            Ok(_) => Ok(have),
        }
    }

    /// gz_load() into an external buffer.
    fn gz_load_ext(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        let mut have = 0usize;
        let mut last: std::io::Result<usize> = Ok(1);
        while have < buf.len() {
            last = self.file.read(&mut buf[have..]);
            match last {
                Ok(0) | Err(_) => break,
                Ok(n) => have += n,
            }
        }
        match last {
            Err(e) => {
                let m = errno_str(&e);
                self.gz_error(Z_ERRNO, Some(&m));
                Err(())
            }
            Ok(0) => {
                self.eof = true;
                Ok(have)
            }
            Ok(_) => Ok(have),
        }
    }

    fn gz_avail(&mut self) -> Result<(), ()> {
        if self.err != Z_OK && self.err != Z_BUF_ERROR {
            return Err(());
        }
        if !self.eof {
            if self.avail_in != 0 {
                let (s, n) = (self.in_next, self.avail_in);
                self.inbuf.copy_within(s..s + n, 0);
            }
            let ai = self.avail_in;
            let got = self.gz_load_into(0, ai, self.size - ai)?;
            self.avail_in += got;
            self.in_next = 0;
        }
        Ok(())
    }

    fn gz_look(&mut self) -> Result<(), ()> {
        if self.size == 0 {
            self.inbuf = vec![0u8; self.want];
            self.outbuf = vec![0u8; self.want << 1];
            self.size = self.want;
            match Inflate::new(15 + 16) {
                Ok(i) => self.inf = Some(i),
                Err(_) => {
                    self.size = 0;
                    self.gz_error(Z_MEM_ERROR, Some("out of memory"));
                    return Err(());
                }
            }
            self.avail_in = 0;
            self.in_next = 0;
        }
        if self.avail_in < 2 {
            self.gz_avail()?;
            if self.avail_in == 0 {
                return Ok(());
            }
        }
        if self.avail_in > 1
            && self.inbuf[self.in_next] == 31
            && self.inbuf[self.in_next + 1] == 139
        {
            if let Some(i) = self.inf.as_mut() {
                i.reset();
            }
            self.how = How::Gzip;
            self.direct = false;
            return Ok(());
        }
        if !self.direct {
            self.avail_in = 0;
            self.eof = true;
            self.have = 0;
            return Ok(());
        }
        self.next = 0;
        let (s, n) = (self.in_next, self.avail_in);
        let (inb, outb) = (&self.inbuf, &mut self.outbuf);
        outb[..n].copy_from_slice(&inb[s..s + n]);
        self.have = n;
        self.avail_in = 0;
        self.how = How::Copy;
        self.direct = true;
        Ok(())
    }

    /// gz_decomp() into `out` (the internal output buffer when None);
    /// returns the number of bytes produced.
    fn gz_decomp(&mut self, mut ext: Option<&mut [u8]>) -> Result<usize, ()> {
        let mut ret = Z_OK;
        let mut produced = 0usize;
        let cap = match &ext {
            Some(b) => b.len(),
            None => self.size << 1,
        };
        loop {
            if self.avail_in == 0 {
                self.gz_avail()?;
            }
            if self.avail_in == 0 {
                self.gz_error(Z_BUF_ERROR, Some("unexpected end of file"));
                break;
            }
            let (s, n) = (self.in_next, self.avail_in);
            let inf = self.inf.as_mut().expect("inflate state");
            let input = &self.inbuf[s..s + n];
            let out: &mut [u8] = match ext.as_deref_mut() {
                Some(b) => b,
                None => &mut self.outbuf[..cap],
            };
            let mut io = Io::new(input, out);
            io.out_pos = produced;
            ret = inf.inflate(&mut io, Z_NO_FLUSH);
            let used = io.in_pos;
            produced = io.out_pos;
            let msg = inf.msg;
            self.in_next += used;
            self.avail_in -= used;
            if ret == Z_STREAM_ERROR || ret == Z_NEED_DICT {
                self.gz_error(
                    Z_STREAM_ERROR,
                    Some("internal error: inflate stream corrupt"),
                );
                return Err(());
            }
            if ret == Z_MEM_ERROR {
                self.gz_error(Z_MEM_ERROR, Some("out of memory"));
                return Err(());
            }
            if ret == Z_DATA_ERROR {
                self.gz_error(Z_DATA_ERROR, Some(msg.unwrap_or("compressed data error")));
                return Err(());
            }
            if !(produced < cap && ret != Z_STREAM_END) {
                break;
            }
        }
        if ext.is_none() {
            self.have = produced;
            self.next = 0;
        }
        if ret == Z_STREAM_END {
            self.how = How::Look;
        }
        Ok(produced)
    }

    fn gz_fetch(&mut self) -> Result<(), ()> {
        loop {
            match self.how {
                How::Look => {
                    self.gz_look()?;
                    if self.how == How::Look {
                        return Ok(());
                    }
                }
                How::Copy => {
                    let n = self.gz_load_into(1, 0, self.size << 1)?;
                    self.have = n;
                    self.next = 0;
                    return Ok(());
                }
                How::Gzip => {
                    self.gz_decomp(None)?;
                }
            }
            if !(self.have == 0 && (!self.eof || self.avail_in != 0)) {
                break;
            }
        }
        Ok(())
    }

    fn gz_skip(&mut self, mut len: i64) -> Result<(), ()> {
        while len != 0 {
            if self.have != 0 {
                let n = if self.have as i64 > len {
                    len as usize
                } else {
                    self.have
                };
                self.have -= n;
                self.next += n;
                self.pos += n as i64;
                len -= n as i64;
            } else if self.eof && self.avail_in == 0 {
                break;
            } else {
                self.gz_fetch()?;
            }
        }
        Ok(())
    }

    fn gz_read(&mut self, buf: &mut [u8]) -> usize {
        let mut len = buf.len();
        if len == 0 {
            return 0;
        }
        if self.seek {
            self.seek = false;
            if self.gz_skip(self.skip).is_err() {
                return 0;
            }
        }
        let mut got = 0usize;
        loop {
            let mut n = len;
            if self.have != 0 {
                if self.have < n {
                    n = self.have;
                }
                buf[got..got + n].copy_from_slice(&self.outbuf[self.next..self.next + n]);
                self.next += n;
                self.have -= n;
            } else if self.eof && self.avail_in == 0 {
                self.past = true;
                break;
            } else if self.how == How::Look || n < (self.size << 1) {
                if self.gz_fetch().is_err() {
                    return 0;
                }
                continue;
            } else if self.how == How::Copy {
                match self.gz_load_ext(&mut buf[got..got + n]) {
                    Ok(k) => n = k,
                    Err(()) => return 0,
                }
            } else {
                match self.gz_decomp(Some(&mut buf[got..got + n])) {
                    Ok(k) => n = k,
                    Err(()) => return 0,
                }
                self.have = 0;
            }
            len -= n;
            got += n;
            self.pos += n as i64;
            if len == 0 {
                break;
            }
        }
        got
    }

    /// gzread(): -1 on error.
    pub fn read(&mut self, buf: &mut [u8]) -> i64 {
        if self.mode != GzMode::Read || (self.err != Z_OK && self.err != Z_BUF_ERROR) {
            return -1;
        }
        let buf = if buf.len() > i32::MAX as usize {
            &mut buf[..i32::MAX as usize]
        } else {
            buf
        };
        let len = self.gz_read(buf);
        if len == 0 && self.err != Z_OK && self.err != Z_BUF_ERROR {
            return -1;
        }
        len as i64
    }

    /// gzrewind()
    pub fn rewind(&mut self) -> i32 {
        if self.mode != GzMode::Read || (self.err != Z_OK && self.err != Z_BUF_ERROR) {
            return -1;
        }
        if self.file.seek(SeekFrom::Start(self.start as u64)).is_err() {
            return -1;
        }
        self.gz_reset();
        0
    }

    /// gzseek() with SEEK_SET (0) or SEEK_CUR (1).
    pub fn seek(&mut self, offset: i64, whence: i32) -> i64 {
        let mut offset = offset;
        if self.err != Z_OK && self.err != Z_BUF_ERROR {
            return -1;
        }
        if whence != 0 && whence != 1 {
            return -1;
        }
        if whence == 0 {
            offset -= self.pos;
        } else if self.seek {
            offset += self.skip;
        }
        self.seek = false;
        if self.mode == GzMode::Read && self.how == How::Copy && self.pos + offset >= 0 {
            if self
                .file
                .seek(SeekFrom::Current(offset - self.have as i64))
                .is_err()
            {
                return -1;
            }
            self.have = 0;
            self.eof = false;
            self.past = false;
            self.seek = false;
            self.gz_error(Z_OK, None);
            self.avail_in = 0;
            self.pos += offset;
            return self.pos;
        }
        if offset < 0 {
            if self.mode != GzMode::Read {
                return -1;
            }
            offset += self.pos;
            if offset < 0 {
                return -1;
            }
            if self.rewind() == -1 {
                return -1;
            }
        }
        if self.mode == GzMode::Read {
            let n = if self.have as i64 > offset {
                offset as usize
            } else {
                self.have
            };
            self.have -= n;
            self.next += n;
            self.pos += n as i64;
            offset -= n as i64;
        }
        if offset != 0 {
            self.seek = true;
            self.skip = offset;
        }
        self.pos + offset
    }

    /// gztell()
    pub fn tell(&self) -> i64 {
        self.pos + if self.seek { self.skip } else { 0 }
    }

    // --------------------------------------------------------------- write

    fn gz_init(&mut self) -> Result<(), ()> {
        self.inbuf = vec![0u8; self.want << 1];
        if !self.direct {
            self.outbuf = vec![0u8; self.want];
            match Deflate::new(
                self.level,
                Z_DEFLATED,
                MAX_WBITS + 16,
                DEF_MEM_LEVEL,
                self.strategy,
            ) {
                Ok(d) => self.def = Some(d),
                Err(_) => {
                    self.gz_error(Z_MEM_ERROR, Some("out of memory"));
                    return Err(());
                }
            }
        }
        self.size = self.want;
        if !self.direct {
            self.out_next = 0;
            self.next = 0;
        }
        Ok(())
    }

    fn write_all_fd(&mut self, which: u8, from: usize, to: usize) -> Result<(), ()> {
        let mut p = from;
        while p < to {
            let r = match which {
                0 => self.file.write(&self.inbuf[p..to]),
                _ => self.file.write(&self.outbuf[p..to]),
            };
            match r {
                Ok(n) => p += n,
                Err(e) => {
                    let m = errno_str(&e);
                    self.gz_error(Z_ERRNO, Some(&m));
                    return Err(());
                }
            }
            if which == 1 {
                self.next = p;
            }
        }
        Ok(())
    }

    /// gz_comp() on the input `src` (None: the internal input buffer at
    /// in_next/avail_in); returns the bytes of `src` consumed.
    fn gz_comp(&mut self, flush: i32, src: Option<&[u8]>) -> Result<usize, ()> {
        if self.size == 0 {
            self.gz_init()?;
        }
        if self.direct {
            match src {
                Some(s) => {
                    let mut p = 0;
                    while p < s.len() {
                        match self.file.write(&s[p..]) {
                            Ok(n) => p += n,
                            Err(e) => {
                                let m = errno_str(&e);
                                self.gz_error(Z_ERRNO, Some(&m));
                                return Err(());
                            }
                        }
                    }
                    return Ok(s.len());
                }
                None => {
                    let (s, n) = (self.in_next, self.avail_in);
                    self.write_all_fd(0, s, s + n)?;
                    self.in_next += n;
                    self.avail_in = 0;
                    return Ok(n);
                }
            }
        }
        let avail_src = match src {
            Some(s) => s.len(),
            None => self.avail_in,
        };
        if self.reset {
            if avail_src == 0 {
                return Ok(0);
            }
            if let Some(d) = self.def.as_mut() {
                d.reset();
            }
            self.reset = false;
        }
        let mut ret = Z_OK;
        let mut consumed = 0usize;
        loop {
            let avail_out = self.size - self.out_next;
            if avail_out == 0 || (flush != Z_NO_FLUSH && (flush != Z_FINISH || ret == Z_STREAM_END))
            {
                let (from, to) = (self.next, self.out_next);
                self.write_all_fd(1, from, to)?;
                if avail_out == 0 {
                    self.out_next = 0;
                    self.next = 0;
                }
            }
            let have = self.size - self.out_next;
            let def = self.def.as_mut().expect("deflate state");
            let input: &[u8] = match src {
                Some(s) => &s[consumed..],
                None => &self.inbuf[self.in_next..self.in_next + self.avail_in],
            };
            let size = self.size;
            let mut io = Io::new(input, &mut self.outbuf[..size]);
            io.out_pos = self.out_next;
            ret = def.deflate(&mut io, flush);
            let used = io.in_pos;
            self.out_next = io.out_pos;
            consumed += used;
            if src.is_none() {
                self.in_next += used;
                self.avail_in -= used;
            }
            if ret == Z_STREAM_ERROR {
                self.gz_error(
                    Z_STREAM_ERROR,
                    Some("internal error: deflate stream corrupt"),
                );
                return Err(());
            }
            let have = have - (self.size - self.out_next);
            if have == 0 {
                break;
            }
        }
        if flush == Z_FINISH {
            self.reset = true;
        }
        Ok(consumed)
    }

    fn gz_zero(&mut self, mut len: i64) -> Result<(), ()> {
        if self.avail_in != 0 {
            self.gz_comp(Z_NO_FLUSH, None)?;
        }
        let mut first = true;
        while len != 0 {
            let n = if self.size as i64 > len {
                len as usize
            } else {
                self.size
            };
            if first {
                for b in &mut self.inbuf[..n] {
                    *b = 0;
                }
                first = false;
            }
            self.avail_in = n;
            self.in_next = 0;
            self.pos += n as i64;
            self.gz_comp(Z_NO_FLUSH, None)?;
            len -= n as i64;
        }
        Ok(())
    }

    fn gz_write(&mut self, buf: &[u8]) -> usize {
        let put = buf.len();
        let mut len = buf.len();
        if len == 0 {
            return 0;
        }
        if self.size == 0 && self.gz_init().is_err() {
            return 0;
        }
        if self.seek {
            self.seek = false;
            if self.gz_zero(self.skip).is_err() {
                return 0;
            }
        }
        let mut off = 0usize;
        if len < self.size {
            loop {
                if self.avail_in == 0 {
                    self.in_next = 0;
                }
                let have = self.in_next + self.avail_in;
                let copy = (self.size - have).min(len);
                self.inbuf[have..have + copy].copy_from_slice(&buf[off..off + copy]);
                self.avail_in += copy;
                self.pos += copy as i64;
                off += copy;
                len -= copy;
                if len != 0 && self.gz_comp(Z_NO_FLUSH, None).is_err() {
                    return 0;
                }
                if len == 0 {
                    break;
                }
            }
        } else {
            if self.avail_in != 0 && self.gz_comp(Z_NO_FLUSH, None).is_err() {
                return 0;
            }
            let mut p = 0usize;
            while p < buf.len() {
                let n = (buf.len() - p).min(u32::MAX as usize);
                self.pos += n as i64;
                match self.gz_comp(Z_NO_FLUSH, Some(&buf[p..p + n])) {
                    Ok(_) => p += n,
                    Err(()) => return 0,
                }
            }
        }
        put
    }

    /// gzwrite(): bytes written, 0 on error.
    pub fn write(&mut self, buf: &[u8]) -> i64 {
        if self.mode != GzMode::Write || self.err != Z_OK {
            return 0;
        }
        if buf.len() > i32::MAX as usize {
            self.gz_error(Z_DATA_ERROR, Some("requested length does not fit in int"));
            return 0;
        }
        self.gz_write(buf) as i64
    }

    /// gzflush()
    pub fn flush(&mut self, flush: i32) -> i32 {
        if self.mode != GzMode::Write || self.err != Z_OK {
            return Z_STREAM_ERROR;
        }
        if !(0..=Z_FINISH).contains(&flush) {
            return Z_STREAM_ERROR;
        }
        if self.seek {
            self.seek = false;
            if self.gz_zero(self.skip).is_err() {
                return self.err;
            }
        }
        let _ = self.gz_comp(flush, None);
        self.err
    }

    /// gzsetparams()
    pub fn set_params(&mut self, level: i32, strategy: i32) -> i32 {
        if self.mode != GzMode::Write || self.err != Z_OK || self.direct {
            return Z_STREAM_ERROR;
        }
        if level == self.level && strategy == self.strategy {
            return Z_OK;
        }
        if self.seek {
            self.seek = false;
            if self.gz_zero(self.skip).is_err() {
                return self.err;
            }
        }
        if self.size != 0 {
            if self.avail_in != 0 && self.gz_comp(Z_BLOCK, None).is_err() {
                return self.err;
            }
            let size = self.size;
            let def = self.def.as_mut().expect("deflate state");
            let mut io = Io::new(&[], &mut self.outbuf[..size]);
            io.out_pos = self.out_next;
            def.params(&mut io, level, strategy);
            self.out_next = io.out_pos;
        }
        self.level = level;
        self.strategy = strategy;
        Z_OK
    }

    /// gzclose(): Z_OK, or an error code.
    pub fn close(mut self) -> i32 {
        match self.mode {
            GzMode::Read => {
                if self.err == Z_BUF_ERROR {
                    Z_BUF_ERROR
                } else {
                    Z_OK
                }
            }
            GzMode::Write => {
                let mut ret = Z_OK;
                if self.seek {
                    self.seek = false;
                    if self.gz_zero(self.skip).is_err() {
                        ret = self.err;
                    }
                }
                if self.gz_comp(Z_FINISH, None).is_err() {
                    ret = self.err;
                }
                ret
            }
        }
    }
}
