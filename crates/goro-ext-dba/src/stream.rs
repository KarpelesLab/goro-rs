//! The parts of php-src's stream layer (main/streams/streams.c,
//! plain_wrapper.c, memory.c) the DBA handlers use: plain files read
//! through an 8 KiB read buffer (eof once a read returns nothing), and
//! php://temp-like memory streams.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;

const CHUNK_SIZE: usize = 8192;

pub enum Backend {
    File(File),
    Memory { data: Vec<u8>, pos: usize },
}

/// php_stream
pub struct PStream {
    backend: Backend,
    readbuf: Vec<u8>,
    readpos: usize,
    position: u64,
    eof: bool,
}

pub const SEEK_SET: i32 = 0;
pub const SEEK_END: i32 = 2;

impl PStream {
    pub fn from_file(f: File) -> PStream {
        let mut f = f;
        let position = f.stream_position().unwrap_or(0);
        PStream {
            backend: Backend::File(f),
            readbuf: Vec::new(),
            readpos: 0,
            position,
            eof: false,
        }
    }

    /// php_stream_temp_create() / php_stream_memory_create()
    pub fn memory(data: Vec<u8>) -> PStream {
        PStream {
            backend: Backend::Memory { data, pos: 0 },
            readbuf: Vec::new(),
            readpos: 0,
            position: 0,
            eof: false,
        }
    }

    pub fn fd(&self) -> Option<i32> {
        match &self.backend {
            Backend::File(f) => Some(f.as_raw_fd()),
            Backend::Memory { .. } => None,
        }
    }

    pub fn is_plain_file(&self) -> bool {
        matches!(self.backend, Backend::File(_))
    }

    fn buffered(&self) -> usize {
        self.readbuf.len() - self.readpos
    }

    fn raw_read(&mut self, n: usize) -> Vec<u8> {
        match &mut self.backend {
            Backend::File(f) => {
                let mut buf = vec![0u8; n];
                loop {
                    match f.read(&mut buf) {
                        Ok(k) => {
                            if k == 0 {
                                self.eof = true;
                            }
                            buf.truncate(k);
                            return buf;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => {
                            self.eof = true;
                            return Vec::new();
                        }
                    }
                }
            }
            Backend::Memory { data, pos } => {
                let start = (*pos).min(data.len());
                let end = (start + n).min(data.len());
                let out = data[start..end].to_vec();
                *pos = end;
                if end >= data.len() {
                    self.eof = true;
                }
                out
            }
        }
    }

    /// _php_stream_fill_read_buffer()
    fn fill(&mut self) -> bool {
        if self.readpos > 0 {
            self.readbuf.drain(..self.readpos);
            self.readpos = 0;
        }
        let chunk = self.raw_read(CHUNK_SIZE);
        if chunk.is_empty() {
            return false;
        }
        self.readbuf.extend_from_slice(&chunk);
        true
    }

    /// _php_stream_read()
    pub fn read(&mut self, size: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(size.min(1 << 20));
        let mut size = size;
        while size > 0 {
            if self.buffered() > 0 {
                let n = self.buffered().min(size);
                out.extend_from_slice(&self.readbuf[self.readpos..self.readpos + n]);
                self.readpos += n;
                size -= n;
                self.position += n as u64;
                continue;
            }
            if !self.fill() {
                break;
            }
        }
        out
    }

    /// php_stream_gets(): a line up to and including "\n", at most
    /// `maxlen - 1` bytes (None: any length). None at the end of the data.
    pub fn gets(&mut self, maxlen: Option<usize>) -> Option<Vec<u8>> {
        let limit = maxlen.map(|m| m.saturating_sub(1));
        let mut out = Vec::new();
        loop {
            if limit.is_some_and(|l| out.len() >= l) {
                break;
            }
            if self.buffered() == 0 {
                if self.eof || !self.fill() {
                    break;
                }
                continue;
            }
            let avail = &self.readbuf[self.readpos..];
            let mut take = avail.len();
            if let Some(l) = limit {
                take = take.min(l - out.len());
            }
            let (n, found) = match avail[..take].iter().position(|&c| c == b'\n') {
                Some(p) => (p + 1, true),
                None => (take, false),
            };
            out.extend_from_slice(&avail[..n]);
            self.readpos += n;
            self.position += n as u64;
            if found {
                break;
            }
        }
        if out.is_empty() { None } else { Some(out) }
    }

    /// php_stream_eof()
    pub fn eof(&self) -> bool {
        if self.buffered() > 0 {
            return false;
        }
        self.eof
    }

    pub fn tell(&self) -> u64 {
        self.position
    }

    /// _php_stream_seek(): Ok(()) or Err(()) (-1).
    pub fn seek(&mut self, offset: i64, whence: i32) -> Result<(), ()> {
        self.readbuf.clear();
        self.readpos = 0;
        let r = match &mut self.backend {
            Backend::File(f) => {
                let from = match whence {
                    SEEK_SET => {
                        if offset < 0 {
                            return Err(());
                        }
                        SeekFrom::Start(offset as u64)
                    }
                    SEEK_END => SeekFrom::End(offset),
                    _ => SeekFrom::Current(offset),
                };
                // (the read buffer was dropped: the descriptor is ahead of
                // the logical position)
                if whence != SEEK_SET && whence != SEEK_END {
                    let _ = f.seek(SeekFrom::Start(self.position));
                }
                f.seek(from).map_err(|_| ())
            }
            Backend::Memory { data, pos } => {
                let base = match whence {
                    SEEK_SET => 0i64,
                    SEEK_END => data.len() as i64,
                    _ => self.position as i64,
                };
                let np = base + offset;
                if np < 0 || np as usize > data.len() {
                    return Err(());
                }
                *pos = np as usize;
                Ok(np as u64)
            }
        };
        match r {
            Ok(p) => {
                self.position = p;
                self.eof = false;
                Ok(())
            }
            Err(()) => Err(()),
        }
    }

    pub fn rewind(&mut self) {
        let _ = self.seek(0, SEEK_SET);
    }

    /// _php_stream_write(): writes go to the logical position.
    pub fn write(&mut self, data: &[u8]) -> usize {
        if data.is_empty() {
            return 0;
        }
        if !self.readbuf.is_empty() {
            self.readbuf.clear();
            self.readpos = 0;
            if let Backend::File(f) = &mut self.backend {
                let _ = f.seek(SeekFrom::Start(self.position));
            } else if let Backend::Memory { pos, .. } = &mut self.backend {
                *pos = self.position as usize;
            }
        }
        match &mut self.backend {
            Backend::File(f) => match f.write_all(data) {
                Ok(()) => {
                    self.position = f
                        .stream_position()
                        .unwrap_or(self.position + data.len() as u64);
                    data.len()
                }
                Err(_) => 0,
            },
            Backend::Memory { data: d, pos } => {
                let start = *pos;
                if start > d.len() {
                    d.resize(start, 0);
                }
                let end = start + data.len();
                if end > d.len() {
                    d.resize(end, 0);
                }
                d[start..end].copy_from_slice(data);
                *pos = end;
                self.position = end as u64;
                data.len()
            }
        }
    }

    pub fn putc(&mut self, c: u8) {
        self.write(&[c]);
    }

    pub fn flush(&mut self) -> bool {
        if let Backend::File(f) = &mut self.backend {
            let _ = f.flush();
        }
        true
    }

    /// php_stream_truncate_set_size(): 0 on success.
    pub fn truncate(&mut self, size: u64) -> i32 {
        match &mut self.backend {
            Backend::File(f) => match f.set_len(size) {
                Ok(()) => 0,
                Err(_) => -1,
            },
            Backend::Memory { data, pos } => {
                data.resize(size as usize, 0);
                if *pos > data.len() {
                    *pos = data.len();
                }
                0
            }
        }
    }

    /// Clear O_APPEND on the descriptor (dba_open() in 'c' mode).
    pub fn clear_append(&mut self) {
        if let Some(fd) = self.fd() {
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                if flags >= 0 {
                    libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_APPEND);
                }
            }
        }
    }

    /// php_stream_lock(): flock(); true when the lock was obtained.
    pub fn lock(&mut self, mode: i32) -> bool {
        match self.fd() {
            Some(fd) => loop {
                let r = unsafe { libc::flock(fd, mode) };
                if r == 0 {
                    return true;
                }
                if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                return false;
            },
            None => false,
        }
    }
}

/// php_stream_copy_to_stream_ex(): `len` None copies everything left.
pub fn copy(src: &mut PStream, dst: &mut PStream, len: Option<usize>) -> bool {
    let mut left = len;
    loop {
        let want = match left {
            Some(0) => break,
            Some(n) => n.min(CHUNK_SIZE),
            None => CHUNK_SIZE,
        };
        let chunk = src.read(want);
        if chunk.is_empty() {
            break;
        }
        if dst.write(&chunk) != chunk.len() {
            return false;
        }
        if let Some(n) = left.as_mut() {
            *n -= chunk.len();
        }
    }
    true
}
