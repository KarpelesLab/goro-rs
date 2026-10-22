//! Port of utf.c: routines used to translate between UTF-8, UTF-16,
//! UTF-16BE, and UTF-16LE.
//!
//! Strings are byte slices; where C relies on a zero terminator, the end
//! of the slice (or an embedded NUL byte) is the terminator.

use crate::int::*;

/// This lookup table is used to help decode the first byte of a
/// multi-byte UTF8 character.
static sqlite3Utf8Trans1: [u8; 64] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x00, 0x01, 0x02, 0x03, 0x00, 0x01, 0x00, 0x00,
];

#[inline]
fn at(z: &[u8], i: usize) -> u8 {
    if i < z.len() { z[i] } else { 0 }
}

/// WRITE_UTF8(zOut, c)
#[inline]
fn write_utf8(out: &mut Vec<u8>, c: u32) {
    if c < 0x00080 {
        out.push((c & 0xFF) as u8);
    } else if c < 0x00800 {
        out.push(0xC0 + ((c >> 6) & 0x1F) as u8);
        out.push(0x80 + (c & 0x3F) as u8);
    } else if c < 0x10000 {
        out.push(0xE0 + ((c >> 12) & 0x0F) as u8);
        out.push(0x80 + ((c >> 6) & 0x3F) as u8);
        out.push(0x80 + (c & 0x3F) as u8);
    } else {
        out.push(0xF0 + ((c >> 18) & 0x07) as u8);
        out.push(0x80 + ((c >> 12) & 0x3F) as u8);
        out.push(0x80 + ((c >> 6) & 0x3F) as u8);
        out.push(0x80 + (c & 0x3F) as u8);
    }
}

/// WRITE_UTF16LE(zOut, c)
#[inline]
fn write_utf16le(out: &mut Vec<u8>, c: u32) {
    if c <= 0xFFFF {
        out.push((c & 0x00FF) as u8);
        out.push(((c >> 8) & 0x00FF) as u8);
    } else {
        out.push((((c >> 10) & 0x003F) + (((c.wrapping_sub(0x10000)) >> 10) & 0x00C0)) as u8);
        out.push((0x00D8 + (((c.wrapping_sub(0x10000)) >> 18) & 0x03)) as u8);
        out.push((c & 0x00FF) as u8);
        out.push((0x00DC + ((c >> 8) & 0x03)) as u8);
    }
}

/// WRITE_UTF16BE(zOut, c)
#[inline]
fn write_utf16be(out: &mut Vec<u8>, c: u32) {
    if c <= 0xFFFF {
        out.push(((c >> 8) & 0x00FF) as u8);
        out.push((c & 0x00FF) as u8);
    } else {
        out.push((0x00D8 + (((c.wrapping_sub(0x10000)) >> 18) & 0x03)) as u8);
        out.push((((c >> 10) & 0x003F) + (((c.wrapping_sub(0x10000)) >> 10) & 0x00C0)) as u8);
        out.push((0x00DC + ((c >> 8) & 0x03)) as u8);
        out.push((c & 0x00FF) as u8);
    }
}

/// C: `int sqlite3AppendOneUtf8Character(char *zOut, u32 v)`
///
/// Write a single UTF8 character whose value is v into the buffer
/// starting at zOut.  zOut needs to have at least four bytes available.
/// Return the number of bytes needed to encode the new character.
pub fn sqlite3AppendOneUtf8Character(zOut: &mut [u8], v: u32) -> i32 {
    if v < 0x00080 {
        zOut[0] = (v & 0xff) as u8;
        return 1;
    }
    if v < 0x00800 {
        zOut[0] = 0xc0 + ((v >> 6) & 0x1f) as u8;
        zOut[1] = 0x80 + (v & 0x3f) as u8;
        return 2;
    }
    if v < 0x10000 {
        zOut[0] = 0xe0 + ((v >> 12) & 0x0f) as u8;
        zOut[1] = 0x80 + ((v >> 6) & 0x3f) as u8;
        zOut[2] = 0x80 + (v & 0x3f) as u8;
        return 3;
    }
    zOut[0] = 0xf0 + ((v >> 18) & 0x07) as u8;
    zOut[1] = 0x80 + ((v >> 12) & 0x3f) as u8;
    zOut[2] = 0x80 + ((v >> 6) & 0x3f) as u8;
    zOut[3] = 0x80 + (v & 0x3f) as u8;
    4
}

/// READ_UTF8(zIn, zTerm, c): read one character of z[*pi..zTerm].
#[inline]
fn read_utf8_limited(z: &[u8], pi: &mut usize, zTerm: usize) -> u32 {
    let mut c = z[*pi] as u32;
    *pi += 1;
    if c >= 0xc0 {
        c = sqlite3Utf8Trans1[(c - 0xc0) as usize] as u32;
        while *pi < zTerm && (z[*pi] & 0xc0) == 0x80 {
            c = (c << 6) + (0x3f & z[*pi]) as u32;
            *pi += 1;
        }
        if c < 0x80 || (c & 0xFFFFF800) == 0xD800 {
            c = 0xFFFD;
        }
    }
    c
}

/// C: `u32 sqlite3Utf8Read( const unsigned char **pz )`
///
/// Translate a single UTF-8 character.  Return the unicode value.
///
/// Rust: the C `const u8 **pz` becomes the string `z` and a read index
/// `*pi`, which is advanced past the character.  The string is assumed
/// zero-terminated: reading at or past the end of the slice returns 0 (and
/// still advances the index by one, like C stepping over the terminator).
pub fn sqlite3Utf8Read(z: &[u8], pi: &mut usize) -> u32 {
    let mut c = at(z, *pi) as u32;
    *pi += 1;
    if c >= 0xc0 {
        c = sqlite3Utf8Trans1[(c - 0xc0) as usize] as u32;
        while (at(z, *pi) & 0xc0) == 0x80 {
            c = (c << 6) + (0x3f & z[*pi]) as u32;
            *pi += 1;
        }
        if c < 0x80 || (c & 0xFFFFF800) == 0xD800 {
            c = 0xFFFD;
        }
    }
    c
}

/// The Utf8Read() macro of func.c: fast path for ASCII.
#[inline]
pub fn Utf8Read(z: &[u8], pi: &mut usize) -> u32 {
    let c = at(z, *pi);
    if c < 0x80 {
        *pi += 1;
        c as u32
    } else {
        sqlite3Utf8Read(z, pi)
    }
}

/// SQLITE_SKIP_UTF8(z): advance `*pi` past one UTF-8 character.
#[inline]
pub fn SQLITE_SKIP_UTF8(z: &[u8], pi: &mut usize) {
    let c = at(z, *pi);
    *pi += 1;
    if c >= 0xc0 {
        while (at(z, *pi) & 0xc0) == 0x80 {
            *pi += 1;
        }
    }
}

/// C: `int sqlite3Utf8ReadLimited( const u8 *z, int n, u32 *piOut )`
///
/// Read a single UTF8 character out of buffer z[0..n-1] and return the
/// number of bytes used to store the character in z[].
pub fn sqlite3Utf8ReadLimited(z: &[u8], n: i32, piOut: &mut u32) -> i32 {
    debug_assert!(n > 0);
    let mut n = n;
    let mut i = 1i32;
    let mut c = z[0] as u32;
    if c >= 0xc0 {
        c = sqlite3Utf8Trans1[(c - 0xc0) as usize] as u32;
        if n > 4 {
            n = 4;
        }
        while i < n && (at(z, i as usize) & 0xc0) == 0x80 {
            c = (c << 6) + (0x3f & z[i as usize]) as u32;
            i += 1;
        }
    }
    *piOut = c;
    i
}

/// C: `SQLITE_NOINLINE int sqlite3VdbeMemTranslate(Mem *pMem, u8 desiredEnc)`
///
/// This routine transforms the internal text encoding used by pMem to
/// desiredEnc.  It is an error if the string is already of the desired
/// encoding, or if pMem does not contain a string value.
pub fn sqlite3VdbeMemTranslate(_db: &mut Sqlite3, pMem: &mut Mem, desiredEnc: u8) -> i32 {
    memTranslate(pMem, desiredEnc)
}

/// The body of sqlite3VdbeMemTranslate() (it does not need the connection).
fn memTranslate(pMem: &mut Mem, desiredEnc: u8) -> i32 {
    debug_assert!((pMem.flags & MEM_Str) != 0);
    debug_assert!(pMem.enc != desiredEnc);
    debug_assert!(pMem.enc != 0);
    debug_assert!(pMem.n >= 0);

    // If the translation is between UTF-16 little and big endian, then all
    // that is required is to swap the byte order.
    if pMem.enc != SQLITE_UTF8 && desiredEnc != SQLITE_UTF8 {
        let n = (pMem.n & !1) as usize;
        let mut k = 0usize;
        while k < n {
            pMem.z.swap(k, k + 1);
            k += 2;
        }
        pMem.enc = desiredEnc;
        return SQLITE_OK;
    }

    // Set len to the maximum number of bytes required in the output buffer.
    let len: i64 = if desiredEnc == SQLITE_UTF8 {
        pMem.n &= !1;
        2 * pMem.n as i64 + 1
    } else {
        2 * pMem.n as i64 + 2
    };

    let zIn: &[u8] = &pMem.z[..pMem.n as usize];
    let zTerm = zIn.len();
    let mut out: Vec<u8> = Vec::with_capacity(len as usize);
    let mut i = 0usize;
    let newN: i32;

    if pMem.enc == SQLITE_UTF8 {
        if desiredEnc == SQLITE_UTF16LE {
            // UTF-8 -> UTF-16 Little-endian
            while i < zTerm {
                let c = read_utf8_limited(zIn, &mut i, zTerm);
                write_utf16le(&mut out, c);
            }
        } else {
            debug_assert!(desiredEnc == SQLITE_UTF16BE);
            // UTF-8 -> UTF-16 Big-endian
            while i < zTerm {
                let c = read_utf8_limited(zIn, &mut i, zTerm);
                write_utf16be(&mut out, c);
            }
        }
        newN = out.len() as i32;
        out.push(0);
    } else {
        debug_assert!(desiredEnc == SQLITE_UTF8);
        let le = pMem.enc == SQLITE_UTF16LE;
        let rd = |k: usize| -> u32 { at(zIn, k) as u32 };
        while i < zTerm {
            let mut c = if le {
                rd(i) + (rd(i + 1) << 8)
            } else {
                (rd(i) << 8) + rd(i + 1)
            };
            i += 2;
            if (0xd800..0xe000).contains(&c) && i < zTerm {
                let c2 = if le {
                    rd(i) + (rd(i + 1) << 8)
                } else {
                    (rd(i) << 8) + rd(i + 1)
                };
                i += 2;
                c = (c2 & 0x03FF) + ((c & 0x003F) << 10) + (((c & 0x03C0) + 0x0040) << 10);
            }
            write_utf8(&mut out, c);
        }
        newN = out.len() as i32;
    }
    out.push(0);
    debug_assert!((newN as i64 + if desiredEnc == SQLITE_UTF8 { 1 } else { 2 }) <= len);

    let c = MEM_Str | MEM_Term | (pMem.flags & (MEM_AffMask | MEM_Subtype));
    pMem.flags = c;
    pMem.enc = desiredEnc;
    pMem.z = out;
    pMem.n = newN;
    SQLITE_OK
}

/// C: `int sqlite3VdbeMemHandleBom(Mem *pMem)`
///
/// This routine checks for a byte-order mark at the beginning of the UTF-16
/// string stored in pMem.  If one is present, it is removed and the
/// encoding of the Mem adjusted.
pub fn sqlite3VdbeMemHandleBom(pMem: &mut Mem) -> i32 {
    let mut bom: u8 = 0;
    debug_assert!(pMem.n >= 0);
    if pMem.n > 1 {
        let b1 = pMem.z[0];
        let b2 = pMem.z[1];
        if b1 == 0xFE && b2 == 0xFF {
            bom = SQLITE_UTF16BE;
        }
        if b1 == 0xFF && b2 == 0xFE {
            bom = SQLITE_UTF16LE;
        }
    }
    if bom != 0 {
        pMem.n -= 2;
        let n = pMem.n as usize;
        pMem.z.drain(0..2);
        pMem.z.truncate(n);
        pMem.z.push(0);
        pMem.z.push(0);
        pMem.flags |= MEM_Term;
        pMem.enc = bom;
    }
    SQLITE_OK
}

/// C: `int sqlite3Utf8CharLen(const char *zIn, int nByte)`
///
/// pZ is a UTF-8 encoded unicode string.  If nByte is less than zero,
/// return the number of unicode characters in pZ up to (but not including)
/// the first 0x00 byte.  If nByte is not less than zero, return the number
/// of unicode characters in the first nByte of pZ (or up to the first 0x00,
/// whichever comes first).
pub fn sqlite3Utf8CharLen(zIn: Option<&[u8]>, nByte: i32) -> i32 {
    let z = zIn.unwrap_or(b"");
    let zTerm = if nByte >= 0 {
        nByte as usize
    } else {
        usize::MAX
    };
    let mut r = 0;
    let mut i = 0usize;
    while at(z, i) != 0 && i < zTerm {
        SQLITE_SKIP_UTF8(z, &mut i);
        r += 1;
    }
    r
}

/// C: `int sqlite3Utf8To8(unsigned char *zIn)`
///
/// Translate UTF-8 to UTF-8 in place, removing invalid characters (those
/// that decode to U+FFFD).  Returns the new length; a zero terminator is
/// written after it when there is room.
pub fn sqlite3Utf8To8(zIn: &mut [u8]) -> i32 {
    let src = zIn.to_vec();
    let mut out: Vec<u8> = Vec::with_capacity(src.len());
    let mut i = 0usize;
    while at(&src, i) != 0 && out.len() <= i {
        let c = sqlite3Utf8Read(&src, &mut i);
        if c != 0xfffd {
            write_utf8(&mut out, c);
        }
    }
    let n = out.len();
    zIn[..n].copy_from_slice(&out);
    if n < zIn.len() {
        zIn[n] = 0;
    }
    n as i32
}

/// C: `char * sqlite3Utf16to8(sqlite3 *db, const void *z, int nByte, u8 enc)`
///
/// Convert a UTF-16 string in the native encoding into a UTF-8 string.
/// If nByte is negative the string is terminated by a 0x0000 character.
pub fn sqlite3Utf16to8(_db: &mut Sqlite3, z: &[u8], nByte: i32, enc: u8) -> Option<ZStr> {
    let n = if nByte < 0 {
        let mut k = 0usize;
        while k + 1 < z.len() && (z[k] != 0 || z[k + 1] != 0) {
            k += 2;
        }
        k.min(z.len())
    } else {
        (nByte as usize).min(z.len())
    };
    let mut m = Mem {
        z: z[..n].to_vec(),
        n: n as i32,
        flags: MEM_Str,
        enc,
        ..Default::default()
    };
    if enc != SQLITE_UTF8 {
        memTranslate(&mut m, SQLITE_UTF8);
    }
    let n = m.n as usize;
    m.z.truncate(n);
    Some(m.z)
}

/// C: `int sqlite3Utf16ByteLen(const void *zIn, int nByte, int nChar)`
///
/// zIn is a UTF-16 encoded unicode string at least nByte bytes long.
/// Return the number of bytes in the first nChar unicode characters in
/// pZ.  nChar must be non-negative.  Surrogate pairs count as a single
/// character.
pub fn sqlite3Utf16ByteLen(zIn: &[u8], nByte: i32, nChar: i32) -> i32 {
    let native_le = SQLITE_UTF16NATIVE == SQLITE_UTF16LE;
    let zEnd = nByte as isize - 1;
    let mut z: isize = 0;
    let mut n = 0;
    if native_le {
        z += 1;
    }
    let b = |k: isize| -> u8 {
        if k >= 0 && (k as usize) < zIn.len() {
            zIn[k as usize]
        } else {
            0
        }
    };
    while n < nChar && z <= zEnd {
        let c = b(z);
        z += 2;
        if (0xd8..0xdc).contains(&c) && z <= zEnd && b(z) >= 0xdc && b(z) < 0xe0 {
            z += 2;
        }
        n += 1;
    }
    (z - native_le as isize) as i32
}

/// C: `void sqlite3UtfSelfTest(void)`
///
/// This routine is called from the TCL test function "translate_selftest".
/// It checks that the primitives for serializing and deserializing
/// characters in each encoding are inverses of each other.
pub fn sqlite3UtfSelfTest() {
    for i in 0u32..0x00110000 {
        let mut zBuf: Vec<u8> = Vec::with_capacity(5);
        write_utf8(&mut zBuf, i);
        let n = zBuf.len();
        assert!(n > 0 && n <= 4);
        zBuf.push(0);
        let mut k = 0usize;
        let c = sqlite3Utf8Read(&zBuf, &mut k);
        let t = if (0xD800..=0xDFFF).contains(&i) {
            0xFFFD
        } else {
            i
        };
        assert!(c == t);
        assert!(k == n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selftest() {
        sqlite3UtfSelfTest();
    }

    #[test]
    fn read_and_len() {
        let s = "aé€😀".as_bytes();
        let mut i = 0;
        assert_eq!(sqlite3Utf8Read(s, &mut i), 'a' as u32);
        assert_eq!(sqlite3Utf8Read(s, &mut i), 'é' as u32);
        assert_eq!(sqlite3Utf8Read(s, &mut i), '€' as u32);
        assert_eq!(sqlite3Utf8Read(s, &mut i), '😀' as u32);
        assert_eq!(i, s.len());
        assert_eq!(sqlite3Utf8Read(s, &mut i), 0);
        assert_eq!(sqlite3Utf8CharLen(Some(s), -1), 4);
        assert_eq!(sqlite3Utf8CharLen(Some(s), 3), 2);
        let mut b = [0u8; 4];
        assert_eq!(sqlite3AppendOneUtf8Character(&mut b, 0x20ac), 3);
        assert_eq!(&b[..3], "€".as_bytes());
        // Invalid sequences decode to U+FFFD
        let mut i = 0;
        assert_eq!(sqlite3Utf8Read(b"\xc0\x80", &mut i), 0xfffd);
    }

    #[test]
    fn utf16_roundtrip() {
        let s = "aé€😀";
        let le: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let be: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        let mut m = Mem {
            z: s.as_bytes().to_vec(),
            n: s.len() as i32,
            flags: MEM_Str,
            enc: SQLITE_UTF8,
            ..Default::default()
        };
        fn tr(m: &mut Mem, enc: u8) {
            memTranslate(m, enc);
        }
        tr(&mut m, SQLITE_UTF16LE);
        assert_eq!(m.bytes(), &le[..]);
        tr(&mut m, SQLITE_UTF16BE);
        assert_eq!(m.bytes(), &be[..]);
        tr(&mut m, SQLITE_UTF8);
        assert_eq!(m.bytes(), s.as_bytes());
        assert_eq!(sqlite3Utf16ByteLen(&le, le.len() as i32, 3), 6);
        assert_eq!(sqlite3Utf16ByteLen(&le, le.len() as i32, 4), 10);
    }
}
