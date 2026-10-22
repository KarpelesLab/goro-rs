//! encoding.c: character set detection.

const F: u8 = 0;
const T: u8 = 1;
const I: u8 = 2;
const X: u8 = 3;

#[rustfmt::skip]
static TEXT_CHARS: [u8; 256] = [
    F, F, F, F, F, F, F, T, T, T, T, T, T, T, F, F,
    F, F, F, F, F, F, F, F, F, F, F, T, F, F, F, F,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, F,
    X, X, X, X, X, T, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,
];

fn looks(buf: &[u8], ubuf: &mut Vec<u32>, ok: impl Fn(u8) -> bool) -> bool {
    ubuf.clear();
    for &c in buf {
        if !ok(TEXT_CHARS[c as usize]) {
            return false;
        }
        ubuf.push(c as u32);
    }
    true
}

fn looks_ascii(buf: &[u8], u: &mut Vec<u32>) -> bool {
    looks(buf, u, |t| t == T)
}
fn looks_latin1(buf: &[u8], u: &mut Vec<u32>) -> bool {
    looks(buf, u, |t| t == T || t == I)
}
fn looks_extended(buf: &[u8], u: &mut Vec<u32>) -> bool {
    looks(buf, u, |t| t == T || t == I || t == X)
}

const XX: u8 = 0xF1;
const AS: u8 = 0xF0;
const S1: u8 = 0x02;
const S2: u8 = 0x13;
const S3: u8 = 0x03;
const S4: u8 = 0x23;
const S5: u8 = 0x34;
const S6: u8 = 0x04;
const S7: u8 = 0x44;

fn first(c: u8) -> u8 {
    match c {
        0x00..=0x7f => AS,
        0x80..=0xc1 => XX,
        0xc2..=0xdf => S1,
        0xe0 => S2,
        0xed => S4,
        0xe1..=0xef => S3,
        0xf0 => S5,
        0xf1..=0xf3 => S6,
        0xf4 => S7,
        _ => XX,
    }
}

const ACCEPT: [(u8, u8); 16] = [
    (0x80, 0xBF),
    (0xA0, 0xBF),
    (0x80, 0x9F),
    (0x90, 0xBF),
    (0x80, 0x8F),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
];

/// file_looks_utf8(): -1 invalid, 0 odd controls, 1 7-bit, 2 UTF-8
pub fn looks_utf8(buf: &[u8], mut ubuf: Option<&mut Vec<u32>>) -> i32 {
    if let Some(u) = ubuf.as_deref_mut() {
        u.clear();
    }
    let mut gotone = false;
    let mut ctrl = false;
    let n = buf.len();
    let mut i = 0usize;
    while i < n {
        let c0 = buf[i];
        if c0 & 0x80 == 0 {
            if TEXT_CHARS[c0 as usize] != T {
                ctrl = true;
            }
            if let Some(u) = ubuf.as_deref_mut() {
                u.push(c0 as u32);
            }
        } else if c0 & 0x40 == 0 {
            return -1;
        } else {
            let x = first(c0);
            let ar = ACCEPT[(x >> 4) as usize];
            if x == XX {
                return -1;
            }
            let (mut c, following) = if c0 & 0x20 == 0 {
                ((c0 & 0x1f) as u32, 1)
            } else if c0 & 0x10 == 0 {
                ((c0 & 0x0f) as u32, 2)
            } else if c0 & 0x08 == 0 {
                ((c0 & 0x07) as u32, 3)
            } else if c0 & 0x04 == 0 {
                ((c0 & 0x03) as u32, 4)
            } else if c0 & 0x02 == 0 {
                ((c0 & 0x01) as u32, 5)
            } else {
                return -1;
            };
            for k in 0..following {
                i += 1;
                if i >= n {
                    return if ctrl {
                        0
                    } else if gotone {
                        2
                    } else {
                        1
                    };
                }
                let b = buf[i];
                if k == 0 && (b < ar.0 || b > ar.1) {
                    return -1;
                }
                if b & 0x80 == 0 || b & 0x40 != 0 {
                    return -1;
                }
                c = (c << 6) + (b & 0x3f) as u32;
            }
            if let Some(u) = ubuf.as_deref_mut() {
                u.push(c);
            }
            gotone = true;
        }
        i += 1;
    }
    if ctrl {
        0
    } else if gotone {
        2
    } else {
        1
    }
}

fn looks_utf8_with_bom(buf: &[u8], u: &mut Vec<u32>) -> i32 {
    if buf.len() > 3 && buf[0] == 0xef && buf[1] == 0xbb && buf[2] == 0xbf {
        looks_utf8(&buf[3..], Some(u))
    } else {
        -1
    }
}

fn looks_utf7(buf: &[u8], u: &mut Vec<u32>) -> i32 {
    if buf.len() > 4 && buf[0] == b'+' && buf[1] == b'/' && buf[2] == b'v' {
        match buf[3] {
            b'8' | b'9' | b'+' | b'/' => {
                u.clear();
                1
            }
            _ => -1,
        }
    } else {
        -1
    }
}

fn looks_ucs16(bf: &[u8], u: &mut Vec<u32>) -> i32 {
    if bf.len() < 2 {
        return 0;
    }
    let bigend = if bf[0] == 0xff && bf[1] == 0xfe {
        false
    } else if bf[0] == 0xfe && bf[1] == 0xff {
        true
    } else {
        return 0;
    };
    u.clear();
    let mut hi: u32 = 0;
    let mut i = 2;
    while i + 1 < bf.len() {
        let mut uc = if bigend {
            (bf[i + 1] as u32) | ((bf[i] as u32) << 8)
        } else {
            (bf[i] as u32) | ((bf[i + 1] as u32) << 8)
        };
        uc &= 0xffff;
        if uc == 0xfffe || uc == 0xffff || (0xfdd0..=0xfdef).contains(&uc) {
            return 0;
        }
        if hi != 0 {
            if !(0xdc00..=0xdfff).contains(&uc) {
                return 0;
            }
            uc = 0x10000 + 0x400 * (hi - 1) + (uc - 0xdc00);
            hi = 0;
        }
        if uc < 128 && TEXT_CHARS[uc as usize] != T {
            return 0;
        }
        u.push(uc);
        if (0xd800..=0xdbff).contains(&uc) {
            hi = uc - 0xd800 + 1;
        }
        if (0xdc00..=0xdfff).contains(&uc) {
            return 0;
        }
        i += 2;
    }
    1 + bigend as i32
}

fn looks_ucs32(bf: &[u8], u: &mut Vec<u32>) -> i32 {
    if bf.len() < 4 {
        return 0;
    }
    let bigend = if bf[0] == 0xff && bf[1] == 0xfe && bf[2] == 0 && bf[3] == 0 {
        false
    } else if bf[0] == 0 && bf[1] == 0 && bf[2] == 0xfe && bf[3] == 0xff {
        true
    } else {
        return 0;
    };
    u.clear();
    let mut i = 4;
    while i + 3 < bf.len() {
        let c = if bigend {
            u32::from_be_bytes(bf[i..i + 4].try_into().unwrap())
        } else {
            u32::from_le_bytes(bf[i..i + 4].try_into().unwrap())
        };
        u.push(c);
        if c == 0xfffe {
            return 0;
        }
        if c < 128 && TEXT_CHARS[c as usize] != T {
            return 0;
        }
        i += 4;
    }
    1 + bigend as i32
}

#[rustfmt::skip]
static EBCDIC_TO_ASCII: [u8; 256] = [
  0,   1,   2,   3, 156,   9, 134, 127, 151, 141, 142,  11,  12,  13,  14,  15,
 16,  17,  18,  19, 157, 133,   8, 135,  24,  25, 146, 143,  28,  29,  30,  31,
128, 129, 130, 131, 132,  10,  23,  27, 136, 137, 138, 139, 140,   5,   6,   7,
144, 145,  22, 147, 148, 149, 150,   4, 152, 153, 154, 155,  20,  21, 158,  26,
b' ', 160, 161, 162, 163, 164, 165, 166, 167, 168, 213, b'.', b'<', b'(', b'+', b'|',
b'&', 169, 170, 171, 172, 173, 174, 175, 176, 177, b'!', b'$', b'*', b')', b';', b'~',
b'-', b'/', 178, 179, 180, 181, 182, 183, 184, 185, 203, b',', b'%', b'_', b'>', b'?',
186, 187, 188, 189, 190, 191, 192, 193, 194, b'`', b':', b'#', b'@', b'\'', b'=', b'"',
195, b'a', b'b', b'c', b'd', b'e', b'f', b'g', b'h', b'i', 196, 197, 198, 199, 200, 201,
202, b'j', b'k', b'l', b'm', b'n', b'o', b'p', b'q', b'r', b'^', 204, 205, 206, 207, 208,
209, 229, b's', b't', b'u', b'v', b'w', b'x', b'y', b'z', 210, 211, 212, b'[', 214, 215,
216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 227, 228, b']', 230, 231,
b'{', b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H', b'I', 232, 233, 234, 235, 236, 237,
b'}', b'J', b'K', b'L', b'M', b'N', b'O', b'P', b'Q', b'R', 238, 239, 240, 241, 242, 243,
b'\\', 159, b'S', b'T', b'U', b'V', b'W', b'X', b'Y', b'Z', 244, 245, 246, 247, 248, 249,
b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', 250, 251, 252, 253, 254, 255,
];

pub struct Encoding {
    pub text: bool,
    pub ubuf: Vec<u32>,
    pub code: &'static str,
    pub code_mime: &'static str,
    pub typ: &'static str,
}

/// file_encoding()
pub fn file_encoding(buf: &[u8], encoding_max: usize) -> Encoding {
    let buf = &buf[..buf.len().min(encoding_max)];
    let mut u: Vec<u32> = Vec::new();
    let mut e = Encoding {
        text: true,
        ubuf: Vec::new(),
        code: "unknown",
        code_mime: "binary",
        typ: "text",
    };
    if looks_ascii(buf, &mut u) {
        if looks_utf7(buf, &mut u) > 0 {
            e.code = "Unicode text, UTF-7";
            e.code_mime = "utf-7";
        } else {
            e.code = "ASCII";
            e.code_mime = "us-ascii";
        }
    } else if looks_utf8_with_bom(buf, &mut u) > 0 {
        e.code = "Unicode text, UTF-8 (with BOM)";
        e.code_mime = "utf-8";
    } else if looks_utf8(buf, Some(&mut u)) > 1 {
        e.code = "Unicode text, UTF-8";
        e.code_mime = "utf-8";
    } else {
        let t = looks_ucs32(buf, &mut u);
        if t != 0 {
            if t == 1 {
                e.code = "Unicode text, UTF-32, little-endian";
                e.code_mime = "utf-32le";
            } else {
                e.code = "Unicode text, UTF-32, big-endian";
                e.code_mime = "utf-32be";
            }
        } else {
            let t = looks_ucs16(buf, &mut u);
            if t != 0 {
                if t == 1 {
                    e.code = "Unicode text, UTF-16, little-endian";
                    e.code_mime = "utf-16le";
                } else {
                    e.code = "Unicode text, UTF-16, big-endian";
                    e.code_mime = "utf-16be";
                }
            } else if looks_latin1(buf, &mut u) {
                e.code = "ISO-8859";
                e.code_mime = "iso-8859-1";
            } else if looks_extended(buf, &mut u) {
                e.code = "Non-ISO extended-ASCII";
                e.code_mime = "unknown-8bit";
            } else {
                let nbuf: Vec<u8> = buf.iter().map(|&c| EBCDIC_TO_ASCII[c as usize]).collect();
                if looks_ascii(&nbuf, &mut u) {
                    e.code = "EBCDIC";
                    e.code_mime = "ebcdic";
                } else if looks_latin1(&nbuf, &mut u) {
                    e.code = "International EBCDIC";
                    e.code_mime = "ebcdic";
                } else {
                    e.text = false;
                    e.typ = "binary";
                }
            }
        }
    }
    e.ubuf = u;
    e
}
