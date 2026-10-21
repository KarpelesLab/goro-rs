//! Helpers shared by several libmbfl filters (mostly from the top of
//! mbfilter_cjk.c).

/// `tbl` contains inclusive ranges, each represented by a pair of u16s
/// (C: `mbfl_bisec_srch`)
pub fn mbfl_bisec_srch(w: i32, tbl: &[u16], n: i32) -> i32 {
    let mut l: i32 = 0;
    let mut r: i32 = n - 1;
    while l <= r {
        let probe = (l + r) >> 1;
        let lo = tbl[(2 * probe) as usize] as i32;
        let hi = tbl[(2 * probe + 1) as usize] as i32;
        if w < lo {
            r = probe - 1;
        } else if w > hi {
            l = probe + 1;
        } else {
            return probe;
        }
    }
    -1
}

/// `tbl` contains single values, not ranges (C: `mbfl_bisec_srch2`)
pub fn mbfl_bisec_srch2(w: i32, tbl: &[u16], n: i32) -> i32 {
    let mut l: i32 = 0;
    let mut r: i32 = n - 1;
    while l <= r {
        let probe = (l + r) >> 1;
        let val = tbl[probe as usize] as i32;
        if w < val {
            r = probe - 1;
        } else if w > val {
            l = probe + 1;
        } else {
            return probe;
        }
    }
    -1
}

/// C: `mbfl_binary_search_paired_sorted_table`; returns the second element
/// of the matching pair.
pub fn mbfl_binary_search_paired_sorted_table(w: u32, tbl: &[[u16; 2]], n: i32) -> Option<u16> {
    let mut r = n;
    let mut l = 0;
    while l < r {
        let probe = (l + r) >> 1;
        let k = tbl[probe as usize][0] as u32;
        if w < k {
            r = probe;
        } else if w > k {
            l = probe + 1;
        } else {
            return Some(tbl[probe as usize][1]);
        }
    }
    None
}

/// Regional Indicator codepoint for an uppercase ASCII letter (C: `NFLAGS`)
#[inline]
pub const fn nflags(c: u8) -> u32 {
    0x1F1A5 + c as u32
}

pub const nflags_s: [[u8; 2]; 10] = [*b"CN", *b"DE", *b"ES", *b"FR", *b"GB", *b"IT", *b"JP", *b"KR", *b"RU", *b"US"];
pub const nflags_kddi: [[u8; 2]; 6] = [*b"FR", *b"DE", *b"IT", *b"GB", *b"CN", *b"KR"];
pub const nflags_sb: [[u8; 2]; 10] = [*b"JP", *b"US", *b"FR", *b"DE", *b"IT", *b"GB", *b"ES", *b"RU", *b"CN", *b"KR"];
pub const nflags_code_kddi: [i32; 10] = [0x2549, 0x2546, 0x24C0, 0x2545, 0x2548, 0x2547, 0x2750, 0x254A, 0x24C1, 0x27F7];
pub const nflags_code_sb: [i32; 10] = [0x2B0A, 0x2B05, 0x2B08, 0x2B04, 0x2B07, 0x2B06, 0x2B02, 0x2B0B, 0x2B09, 0x2B03];

/// number -> (ku*94)+ten value for telephone keypad character
#[inline]
pub const fn docomo_keypad(n: i32) -> i32 {
    if n == 0 { 0x296F } else { 0x2965 + n }
}
pub const DOCOMO_KEYPAD_HASH: i32 = 0x2964;

/// C: `SJIS_ENCODE(c1, c2, s1, s2)`; returns (s1, s2)
#[inline]
pub fn sjis_encode(c1: u32, c2: u32) -> (u32, u32) {
    let s1 = ((c1.wrapping_sub(1)) >> 1) + if c1 < 0x5F { 0x71 } else { 0xB1 };
    let mut s2 = c2;
    if c1 & 1 != 0 {
        if c2 < 0x60 {
            s2 = s2.wrapping_sub(1);
        }
        s2 = s2.wrapping_add(0x20);
    } else {
        s2 = s2.wrapping_add(0x7e);
    }
    (s1, s2)
}

/// C: `SJIS_DECODE(c1, c2, s1, s2)`; returns (s1, s2)
#[inline]
pub fn sjis_decode(c1: u32, c2: u32) -> (u32, u32) {
    let mut s1 = if c1 < 0xa0 {
        ((c1.wrapping_sub(0x81)) << 1).wrapping_add(0x21)
    } else {
        ((c1.wrapping_sub(0xc1)) << 1).wrapping_add(0x21)
    };
    let mut s2 = c2;
    if c2 < 0x9f {
        if c2 < 0x7f {
            s2 += 1;
        }
        s2 = s2.wrapping_sub(0x20);
    } else {
        s1 += 1;
        s2 = s2.wrapping_sub(0x7e);
    }
    (s1, s2)
}
