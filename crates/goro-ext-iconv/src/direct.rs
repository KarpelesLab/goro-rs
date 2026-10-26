//! glibc's direct gconv modules: conversions between two charsets that
//! do not go through UCS-4 and win over the two-step path, so they keep
//! their own mappings and quirks.
//!
//! - GBBIG5 (EUC-CN <-> BIG5): table driven; a character the other side
//!   lacks is an illegal sequence, which //IGNORE replaces with the box
//!   character (BIG5 A1BC / GB2312 A1F5) rather than dropping.
//! - GBGBK (GBK <-> EUC-CN): GBK -> EUC-CN passes the pairs GB2312 has
//!   through and rejects the others (both bytes skipped); EUC-CN -> GBK
//!   copies any pair without looking at it but wants a third byte after it
//!   (a pair ending the input is "incomplete").
//!
//! The tables and the model are checked against glibc by
//! tools/gen_iconv_sbcs/dump_direct.py.

use crate::charset::Kind;
use crate::conv::Errno;
use crate::direct_tables::{BIG5_TO_GB2312, GB2312_TO_BIG5, GBK_TO_GB2312_BAD};
use goro_ext_mbstring::mbfl::EncNo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direct {
    GbToBig5,
    Big5ToGb,
    GbkToGb,
    GbToGbk,
}

impl Direct {
    /// The direct module of a conversion `from` -> `to`, if glibc has one.
    pub fn of(from: Kind, to: Kind) -> Option<Direct> {
        use EncNo::{Big5, Cp936, EucCn};
        Some(match (from, to) {
            (Kind::Mbfl(EucCn), Kind::Mbfl(Big5)) => Direct::GbToBig5,
            (Kind::Mbfl(Big5), Kind::Mbfl(EucCn)) => Direct::Big5ToGb,
            (Kind::Mbfl(Cp936), Kind::Mbfl(EucCn)) => Direct::GbkToGb,
            (Kind::Mbfl(EucCn), Kind::Mbfl(Cp936)) => Direct::GbToGbk,
            _ => return None,
        })
    }
}

/// One step at `s` (non-empty): Ok((bytes taken, output)), or the error
/// with the bytes //IGNORE skips and what it writes instead.
enum Step {
    Out(usize, [u8; 2], usize),
    /// Illegal: skip this many bytes, writing the box character if any.
    Illegal(usize, Option<[u8; 2]>),
    Incomplete,
}

fn step(d: Direct, s: &[u8]) -> Step {
    let a = s[0];
    if a < 0x80 {
        return Step::Out(1, [a, 0], 1);
    }
    match d {
        Direct::GbToBig5 => {
            if !(0xA1..=0xF7).contains(&a) {
                return Step::Illegal(1, None);
            }
            let Some(&b) = s.get(1) else {
                return Step::Incomplete;
            };
            if b < 0xA1 {
                return Step::Illegal(1, None);
            }
            // (a trail byte 0xFF reads the next row's first entry)
            let i = (a as usize - 0xA1) * 94 + (b as usize - 0xA1);
            match GB2312_TO_BIG5.get(i).copied().unwrap_or(0) {
                0 => Step::Illegal(2, Some([0xA1, 0xBC])),
                c => Step::Out(2, c.to_be_bytes(), 2),
            }
        }
        Direct::Big5ToGb => {
            if !(0xA1..=0xF9).contains(&a) {
                return Step::Illegal(1, None);
            }
            let Some(&b) = s.get(1) else {
                return Step::Incomplete;
            };
            let t = match b {
                0x40..=0x7E => b as usize - 0x40,
                0xA1..=0xFE => b as usize - 0x62,
                _ => return Step::Illegal(1, None),
            };
            match BIG5_TO_GB2312[(a as usize - 0xA1) * 157 + t] {
                0 => Step::Illegal(2, Some([0xA1, 0xF5])),
                c => Step::Out(2, c.to_be_bytes(), 2),
            }
        }
        Direct::GbkToGb => {
            let Some(&b) = s.get(1) else {
                return Step::Incomplete;
            };
            let code = u16::from_be_bytes([a, b]);
            let bad = !(0xA1..=0xF7).contains(&a)
                || b < 0xA1
                || GBK_TO_GB2312_BAD
                    .iter()
                    .any(|&(lo, hi)| (lo..=hi).contains(&code));
            if bad {
                Step::Illegal(2, None)
            } else {
                Step::Out(2, [a, b], 2)
            }
        }
        Direct::GbToGbk => {
            // (glibc wants a byte after the pair)
            if s.len() < 3 {
                return Step::Incomplete;
            }
            Step::Out(2, [a, s[1]], 2)
        }
    }
}

/// iconv() through a direct module: converts `input[*pos..]` appending to
/// `out` (at most `limit` bytes); Ok(true) when //IGNORE skipped or boxed
/// something (glibc then ends with EILSEQ).
pub fn conv(
    d: Direct,
    ignore: bool,
    input: &[u8],
    pos: &mut usize,
    out: &mut Vec<u8>,
    limit: Option<usize>,
) -> Result<bool, Errno> {
    let start = out.len();
    let mut skipped = false;
    while *pos < input.len() {
        let room = |n: usize, out: &Vec<u8>| limit.is_none_or(|l| out.len() - start + n <= l);
        if !room(1, out) {
            return Err(Errno::TooBig);
        }
        match step(d, &input[*pos..]) {
            Step::Out(n, bytes, len) => {
                if !room(len, out) {
                    return Err(Errno::TooBig);
                }
                out.extend_from_slice(&bytes[..len]);
                *pos += n;
            }
            Step::Incomplete => return Err(Errno::Inval),
            Step::Illegal(n, boxed) => {
                if !ignore {
                    return Err(Errno::Ilseq);
                }
                if let Some(b) = boxed {
                    if !room(2, out) {
                        return Err(Errno::TooBig);
                    }
                    out.extend_from_slice(&b);
                }
                *pos += n.min(input.len() - *pos);
                skipped = true;
            }
        }
    }
    Ok(skipped)
}
