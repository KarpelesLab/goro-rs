//! gd_io.c / gd_io_dp.c: the gdIOCtx helpers over in-memory buffers.
//!
//! Readers work on a byte slice (the PHP layer reads files and streams
//! fully before decoding); writers append to a growable buffer and support
//! seeking back (the GD2 writer patches its chunk index).

pub const EOF: i32 = -1;

/// A reading gdIOCtx (gdNewDynamicCtxEx() over existing data)
pub struct In<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> In<'a> {
    pub fn new(data: &'a [u8]) -> In<'a> {
        In { data, pos: 0 }
    }

    /// getC: a byte or EOF
    pub fn get_c(&mut self) -> i32 {
        if self.pos < self.data.len() {
            let c = self.data[self.pos];
            self.pos += 1;
            c as i32
        } else {
            EOF
        }
    }

    /// gdGetByte()
    pub fn get_byte(&mut self) -> Option<i32> {
        let c = self.get_c();
        if c == EOF { None } else { Some(c) }
    }

    /// gdGetWord(): big endian
    pub fn get_word(&mut self) -> Option<i32> {
        let a = self.get_byte()?;
        let b = self.get_byte()?;
        Some((a << 8) + b)
    }

    /// gdGetWordLSB(): little endian, as a signed short
    pub fn get_word_lsb(&mut self) -> Option<i16> {
        let lo = self.get_byte()?;
        let hi = self.get_byte()?;
        Some(((hi << 8) | lo) as i16)
    }

    /// gdGetInt(): big endian
    pub fn get_int(&mut self) -> Option<i32> {
        let a = self.get_byte()? as u32;
        let b = self.get_byte()? as u32;
        let c = self.get_byte()? as u32;
        let d = self.get_byte()? as u32;
        Some(
            ((a << 24)
                .wrapping_add(b << 16)
                .wrapping_add(c << 8)
                .wrapping_add(d)) as i32,
        )
    }

    /// gdGetIntLSB()
    pub fn get_int_lsb(&mut self) -> Option<i32> {
        let a = self.get_byte()? as u32;
        let b = self.get_byte()? as u32;
        let c = self.get_byte()? as u32;
        let d = self.get_byte()? as u32;
        Some((a | (b << 8) | (c << 16) | (d << 24)) as i32)
    }

    /// gdGetBuf(): bytes read, or EOF (-1) when nothing is left
    pub fn get_buf(&mut self, buf: &mut [u8]) -> i32 {
        let remain = self.data.len().saturating_sub(self.pos);
        if remain == 0 {
            return if buf.is_empty() { 0 } else { EOF };
        }
        let n = remain.min(buf.len());
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        n as i32
    }

    /// gdSeek(): false when out of range
    pub fn seek(&mut self, pos: i32) -> bool {
        if pos < 0 || pos as usize > self.data.len() {
            return false;
        }
        self.pos = pos as usize;
        true
    }

    /// gdTell()
    pub fn tell(&self) -> i64 {
        self.pos as i64
    }

    pub fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos.min(self.data.len())..]
    }
}

/// A writing gdIOCtx (gdNewDynamicCtx())
#[derive(Default)]
pub struct Out {
    pub buf: Vec<u8>,
    pub pos: usize,
}

impl Out {
    pub fn new() -> Out {
        Out::default()
    }

    pub fn put_c(&mut self, c: i32) {
        self.put_buf(&[c as u8]);
    }

    /// gdPutWord(): big endian
    pub fn put_word(&mut self, w: i32) {
        self.put_buf(&[(w >> 8) as u8, (w & 0xff) as u8]);
    }

    /// Putword(): little endian
    pub fn put_word_lsb(&mut self, w: i32) {
        self.put_buf(&[(w & 0xff) as u8, ((w / 256) & 0xff) as u8]);
    }

    /// gdPutInt(): big endian
    pub fn put_int(&mut self, w: i32) {
        self.put_buf(&[
            (w >> 24) as u8,
            ((w >> 16) & 0xff) as u8,
            ((w >> 8) & 0xff) as u8,
            (w & 0xff) as u8,
        ]);
    }

    /// gdPutIntLSB
    pub fn put_int_lsb(&mut self, w: i32) {
        self.put_buf(&w.to_le_bytes());
    }

    pub fn put_buf(&mut self, b: &[u8]) -> i32 {
        let end = self.pos + b.len();
        if end > self.buf.len() {
            self.buf.resize(end, 0);
        }
        self.buf[self.pos..end].copy_from_slice(b);
        self.pos = end;
        b.len() as i32
    }

    pub fn seek(&mut self, pos: i32) -> bool {
        if pos < 0 {
            return false;
        }
        let p = pos as usize;
        if p > self.buf.len() {
            self.buf.resize(p, 0);
        }
        self.pos = p;
        true
    }

    pub fn tell(&self) -> i64 {
        self.pos as i64
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}
