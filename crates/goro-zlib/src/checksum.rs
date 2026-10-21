//! adler32.c and crc32.c.

const BASE: u32 = 65521;
const NMAX: usize = 5552;

/// adler32(adler, buf, len)
pub fn adler32(adler: u32, buf: &[u8]) -> u32 {
    let mut a = adler & 0xffff;
    let mut b = (adler >> 16) & 0xffff;
    if buf.len() == 1 {
        a += buf[0] as u32;
        if a >= BASE {
            a -= BASE;
        }
        b += a;
        if b >= BASE {
            b -= BASE;
        }
        return a | (b << 16);
    }
    if buf.len() < 16 {
        for &c in buf {
            a += c as u32;
            b += a;
        }
        if a >= BASE {
            a -= BASE;
        }
        b %= BASE;
        return a | (b << 16);
    }
    for chunk in buf.chunks(NMAX) {
        for &c in chunk {
            a += c as u32;
            b += a;
        }
        a %= BASE;
        b %= BASE;
    }
    a | (b << 16)
}

fn crc_table() -> &'static [u32; 256] {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, e) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb88320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *e = c;
        }
        t
    })
}

/// crc32(crc, buf, len)
pub fn crc32(crc: u32, buf: &[u8]) -> u32 {
    let t = crc_table();
    let mut c = !crc;
    for &b in buf {
        c = t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}
