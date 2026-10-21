//! inftrees.c from zlib 1.3.1.

pub(crate) const ENOUGH_LENS: usize = 852;
pub(crate) const ENOUGH_DISTS: usize = 592;
pub(crate) const ENOUGH: usize = ENOUGH_LENS + ENOUGH_DISTS;
const MAXBITS: usize = 15;

#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct Code {
    pub op: u8,
    pub bits: u8,
    pub val: u16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodeType {
    Codes,
    Lens,
    Dists,
}

const LBASE: [u16; 31] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258, 0, 0,
];
const LEXT: [u16; 31] = [
    16, 16, 16, 16, 16, 16, 16, 16, 17, 17, 17, 17, 18, 18, 18, 18, 19, 19, 19, 19, 20, 20, 20, 20,
    21, 21, 21, 21, 16, 203, 77,
];
const DBASE: [u16; 32] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577, 0, 0,
];
const DEXT: [u16; 32] = [
    16, 16, 16, 16, 17, 17, 18, 18, 19, 19, 20, 20, 21, 21, 22, 22, 23, 23, 24, 24, 25, 25, 26, 26,
    27, 27, 28, 28, 29, 29, 64, 64,
];

/// inflate_table(): builds into `table[*next..]`, advancing `*next`.
/// Returns 0 on success, -1 for an invalid code, 1 if ENOUGH is too small.
pub(crate) fn inflate_table(
    ty: CodeType,
    lens: &[u16],
    codes: usize,
    table: &mut [Code],
    next_off: &mut usize,
    bits: &mut u32,
    work: &mut [u16],
) -> i32 {
    let mut count = [0u16; MAXBITS + 1];
    let mut offs = [0u16; MAXBITS + 1];
    for &l in &lens[..codes] {
        count[l as usize] += 1;
    }
    let mut root = *bits;
    let mut max = MAXBITS as u32;
    while max >= 1 {
        if count[max as usize] != 0 {
            break;
        }
        max -= 1;
    }
    if root > max {
        root = max;
    }
    if max == 0 {
        let here = Code {
            op: 64,
            bits: 1,
            val: 0,
        };
        table[*next_off] = here;
        *next_off += 1;
        table[*next_off] = here;
        *next_off += 1;
        *bits = 1;
        return 0;
    }
    let mut min = 1u32;
    while min < max {
        if count[min as usize] != 0 {
            break;
        }
        min += 1;
    }
    if root < min {
        root = min;
    }
    let mut left: i32 = 1;
    for len in 1..=MAXBITS {
        left <<= 1;
        left -= count[len] as i32;
        if left < 0 {
            return -1;
        }
    }
    if left > 0 && (ty == CodeType::Codes || max != 1) {
        return -1;
    }
    offs[1] = 0;
    for len in 1..MAXBITS {
        offs[len + 1] = offs[len] + count[len];
    }
    for sym in 0..codes {
        if lens[sym] != 0 {
            work[offs[lens[sym] as usize] as usize] = sym as u16;
            offs[lens[sym] as usize] += 1;
        }
    }
    let (base, extra, match_): (&[u16], &[u16], u32) = match ty {
        CodeType::Codes => (&[], &[], 20),
        CodeType::Lens => (&LBASE, &LEXT, 257),
        CodeType::Dists => (&DBASE, &DEXT, 0),
    };
    let mut huff: u32 = 0;
    let mut sym: usize = 0;
    let mut len: u32 = min;
    let table_start = *next_off;
    let mut next = table_start;
    let mut curr = root;
    let mut drop: u32 = 0;
    let mut low: u32 = u32::MAX;
    let mut used: u32 = 1u32 << root;
    let mask = used - 1;
    if (ty == CodeType::Lens && used as usize > ENOUGH_LENS)
        || (ty == CodeType::Dists && used as usize > ENOUGH_DISTS)
    {
        return 1;
    }
    loop {
        let mut here = Code {
            op: 0,
            bits: (len - drop) as u8,
            val: 0,
        };
        let w = work[sym] as u32;
        if w + 1 < match_ {
            here.op = 0;
            here.val = w as u16;
        } else if w >= match_ {
            here.op = extra[(w - match_) as usize] as u8;
            here.val = base[(w - match_) as usize];
        } else {
            here.op = 32 + 64;
            here.val = 0;
        }
        let incr = 1u32 << (len - drop);
        let mut fill = 1u32 << curr;
        min = fill;
        loop {
            fill -= incr;
            table[next + ((huff >> drop) + fill) as usize] = here;
            if fill == 0 {
                break;
            }
        }
        let mut incr = 1u32 << (len - 1);
        while huff & incr != 0 {
            incr >>= 1;
        }
        if incr != 0 {
            huff &= incr - 1;
            huff += incr;
        } else {
            huff = 0;
        }
        sym += 1;
        count[len as usize] -= 1;
        if count[len as usize] == 0 {
            if len == max {
                break;
            }
            len = lens[work[sym] as usize] as u32;
        }
        if len > root && (huff & mask) != low {
            if drop == 0 {
                drop = root;
            }
            next += min as usize;
            curr = len - drop;
            let mut left: i32 = 1 << curr;
            while curr + drop < max {
                left -= count[(curr + drop) as usize] as i32;
                if left <= 0 {
                    break;
                }
                curr += 1;
                left <<= 1;
            }
            used += 1u32 << curr;
            if (ty == CodeType::Lens && used as usize > ENOUGH_LENS)
                || (ty == CodeType::Dists && used as usize > ENOUGH_DISTS)
            {
                return 1;
            }
            low = huff & mask;
            table[table_start + low as usize] = Code {
                op: curr as u8,
                bits: root as u8,
                val: (next - table_start) as u16,
            };
        }
    }
    if huff != 0 {
        table[next + huff as usize] = Code {
            op: 64,
            bits: (len - drop) as u8,
            val: 0,
        };
    }
    *next_off = table_start + used as usize;
    *bits = root;
    0
}
