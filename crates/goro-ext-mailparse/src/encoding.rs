//! mailparse_encoding.c: the transfer encodings (7bit, 8bit, BASE64,
//! Quoted-Printable) and the libmbfl-derived conversion filters between
//! them and 8bit.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Enc {
    SevenBit,
    EightBit,
    Base64,
    Qprint,
}

/// mb_name2encoding()
pub fn name2encoding(name: &[u8]) -> Option<Enc> {
    let n = name.to_ascii_lowercase();
    match n.as_slice() {
        b"base64" => Some(Enc::Base64),
        b"quoted-printable" | b"qprint" => Some(Enc::Qprint),
        b"8bit" => Some(Enc::EightBit),
        b"7bit" => Some(Enc::SevenBit),
        _ => None,
    }
}

const BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// hex2code_map
fn hex2code(c: u8) -> i32 {
    match c {
        b'0'..=b'9' => (c - b'0') as i32,
        b'A'..=b'F' => (c - b'A' + 10) as i32,
        b'a'..=b'f' => (c - b'a' + 10) as i32,
        _ => -1,
    }
}

/// An 8bit -> BASE64 / Quoted-Printable encoder (mb_convert_filter with
/// the output function appending to `out`).
pub struct Encoder {
    enc: Enc,
    status: i32,
    cache: i32,
}

impl Encoder {
    pub fn new(enc: Enc) -> Encoder {
        Encoder {
            enc,
            status: 0,
            cache: 0,
        }
    }

    /// mb_convert_filter_feed()
    pub fn feed(&mut self, c: u8, out: &mut Vec<u8>) {
        match self.enc {
            Enc::Base64 => self.base64enc(c as i32, out),
            Enc::Qprint => self.qprintenc(c as i32, out),
            // no conversion filter between 8bit and itself
            Enc::SevenBit | Enc::EightBit => out.push(c),
        }
    }

    /// mb_convert_filter_flush()
    pub fn flush(&mut self, out: &mut Vec<u8>) {
        match self.enc {
            Enc::Base64 => self.base64enc_flush(out),
            Enc::Qprint => {
                self.qprintenc(0, out);
                self.status &= !0xffff;
                self.cache = 0;
            }
            Enc::SevenBit | Enc::EightBit => {}
        }
    }

    fn base64enc(&mut self, c: i32, out: &mut Vec<u8>) {
        let n = self.status & 0xff;
        if n == 0 {
            self.status += 1;
            self.cache = (c & 0xff) << 16;
        } else if n == 1 {
            self.status += 1;
            self.cache |= (c & 0xff) << 8;
        } else {
            self.status &= !0xff;
            let len = (self.status & 0xff00) >> 8;
            if len > 72 {
                out.extend_from_slice(b"\r\n");
                self.status &= !0xff00;
            }
            self.status += 0x400;
            let n = self.cache | (c & 0xff);
            out.push(BASE64_TABLE[((n >> 18) & 0x3f) as usize]);
            out.push(BASE64_TABLE[((n >> 12) & 0x3f) as usize]);
            out.push(BASE64_TABLE[((n >> 6) & 0x3f) as usize]);
            out.push(BASE64_TABLE[(n & 0x3f) as usize]);
        }
    }

    fn base64enc_flush(&mut self, out: &mut Vec<u8>) {
        let status = self.status & 0xff;
        let cache = self.cache;
        let len = (self.status & 0xff00) >> 8;
        self.status &= !0xffff;
        self.cache = 0;
        if status >= 1 {
            if len > 72 {
                out.extend_from_slice(b"\r\n");
            }
            out.push(BASE64_TABLE[((cache >> 18) & 0x3f) as usize]);
            out.push(BASE64_TABLE[((cache >> 12) & 0x3f) as usize]);
            if status == 1 {
                out.extend_from_slice(b"==");
            } else {
                out.push(BASE64_TABLE[((cache >> 6) & 0x3f) as usize]);
                out.push(b'=');
            }
        }
    }

    fn qprintenc(&mut self, c: i32, out: &mut Vec<u8>) {
        if self.status & 0xff == 0 {
            self.cache = c;
            self.status += 1;
            return;
        }
        let s = self.cache;
        self.cache = c;
        let n = (self.status & 0xff00) >> 8;

        if s == 0 {
            out.push(0);
            self.status &= !0xff00;
            return;
        }
        if s == b'\n' as i32 || (s == b'\r' as i32 && c != b'\n' as i32) {
            out.extend_from_slice(b"\r\n");
            self.status &= !0xff00;
            return;
        } else if s == 0x0d {
            return;
        }
        if n >= 72 {
            // soft line break
            out.extend_from_slice(b"=\r\n");
            self.status &= !0xff00;
        }
        if s <= 0 || s >= 0x80 || s == b'=' as i32 {
            out.push(b'=');
            let hex = |v: i32| -> u8 {
                if v < 10 {
                    (v + 48) as u8
                } else {
                    (v + 55) as u8
                }
            };
            out.push(hex((s >> 4) & 0xf));
            out.push(hex(s & 0xf));
            self.status += 0x300;
        } else {
            out.push(s as u8);
            self.status += 0x100;
        }
    }
}

/// A BASE64 / Quoted-Printable -> 8bit decoder (the bulk decoders of
/// mb_convert_filter_feed_block()).
pub struct Decoder {
    enc: Enc,
    status: i32,
    cache: i32,
}

impl Decoder {
    /// None for the encodings that need no conversion.
    pub fn new(enc: Enc) -> Option<Decoder> {
        match enc {
            Enc::Base64 | Enc::Qprint => Some(Decoder {
                enc,
                status: 0,
                cache: 0,
            }),
            _ => None,
        }
    }

    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) {
        match self.enc {
            Enc::Base64 => self.base64(input, out),
            Enc::Qprint => self.qprint(input, out),
            _ => out.extend_from_slice(input),
        }
    }

    pub fn flush(&mut self, out: &mut Vec<u8>) {
        let status = self.status;
        let cache = self.cache;
        self.status = 0;
        self.cache = 0;
        match self.enc {
            Enc::Base64 => {
                if status >= 2 {
                    out.push(((cache >> 16) & 0xff) as u8);
                    if status >= 3 {
                        out.push(((cache >> 8) & 0xff) as u8);
                    }
                }
            }
            Enc::Qprint => {
                if status == 1 {
                    out.push(b'=');
                } else if status == 2 {
                    out.push(b'=');
                    out.push(cache as u8);
                }
            }
            _ => {}
        }
    }

    fn base64(&mut self, input: &[u8], out: &mut Vec<u8>) {
        let mut status = self.status;
        let mut cache = self.cache;
        for &c in input {
            let n: i32 = match c {
                b'\r' | b'\n' | b' ' | b'\t' | b'=' => continue,
                b'A'..=b'Z' => (c - b'A') as i32,
                b'a'..=b'z' => (c - b'a' + 26) as i32,
                b'0'..=b'9' => (c - b'0' + 52) as i32,
                b'+' => 62,
                b'/' => 63,
                // invalid character, ignored
                _ => continue,
            };
            match status {
                0 => {
                    status = 1;
                    cache = n << 18;
                }
                1 => {
                    status = 2;
                    cache |= n << 12;
                }
                2 => {
                    status = 3;
                    cache |= n << 6;
                }
                _ => {
                    status = 0;
                    let n = n | cache;
                    out.push(((n >> 16) & 0xff) as u8);
                    out.push(((n >> 8) & 0xff) as u8);
                    out.push((n & 0xff) as u8);
                }
            }
        }
        self.status = status;
        self.cache = cache;
    }

    fn qprint(&mut self, input: &[u8], out: &mut Vec<u8>) {
        let mut status = self.status;
        let mut cache = self.cache;
        for &c in input {
            match status {
                1 => {
                    if hex2code(c) >= 0 {
                        cache = c as i32;
                        status = 2;
                    } else if c == 0x0d {
                        // soft line break
                        status = 3;
                    } else if c == 0x0a {
                        status = 0;
                    } else {
                        out.push(b'=');
                        out.push(c);
                        status = 0;
                    }
                }
                2 => {
                    let m = hex2code(c);
                    if m < 0 {
                        out.push(b'=');
                        out.push(cache as u8);
                        out.push(c);
                    } else {
                        out.push(((hex2code(cache as u8) << 4) | m) as u8);
                    }
                    status = 0;
                }
                3 => {
                    if c != 0x0a {
                        out.push(c);
                    }
                    status = 0;
                }
                _ => {
                    if c == b'=' {
                        status = 1;
                    } else {
                        out.push(c);
                    }
                }
            }
        }
        self.status = status;
        self.cache = cache;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(e: Enc, s: &[u8]) -> Vec<u8> {
        let mut f = Encoder::new(e);
        let mut out = Vec::new();
        for &c in s {
            f.feed(c, &mut out);
        }
        f.flush(&mut out);
        out
    }

    fn dec(e: Enc, s: &[u8]) -> Vec<u8> {
        let mut d = Decoder::new(e).unwrap();
        let mut out = Vec::new();
        d.feed(s, &mut out);
        d.flush(&mut out);
        out
    }

    #[test]
    fn roundtrip() {
        assert_eq!(enc(Enc::Base64, b"hello"), b"aGVsbG8=");
        assert_eq!(dec(Enc::Base64, b"aGVs\r\nbG8="), b"hello");
        assert_eq!(enc(Enc::Qprint, b"text=hello.\xa3"), b"text=3Dhello.=A3");
        assert_eq!(dec(Enc::Qprint, b"a=3Db=\r\nc=zz="), b"a=bc=zz=");
    }
}
