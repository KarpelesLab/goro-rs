//! CRC-32 (ISO 3309 / ITU-T V.42 polynomial 0xEDB88320, reflected), the
//! checksum of ZIP entries (PKWARE APPNOTE.TXT 4.4.7).

const fn make_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static TABLE: [u32; 256] = make_table();

/// One byte into a raw (not pre/post-inverted) CRC register, as the
/// PKWARE encryption's key schedule uses it (APPNOTE 6.1.5).
#[inline]
pub fn update_byte(crc: u32, b: u8) -> u32 {
    TABLE[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8)
}

/// Continues a CRC-32 over `data` (start with 0).
pub fn update(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in data {
        c = update_byte(c, b);
    }
    !c
}

/// The CRC-32 of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    update(0, data)
}

#[cfg(test)]
mod tests {
    #[test]
    fn known() {
        assert_eq!(super::crc32(b""), 0);
        assert_eq!(super::crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(super::update(super::crc32(b"1234"), b"56789"), 0xCBF4_3926);
    }
}
