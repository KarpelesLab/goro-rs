//! `mb_convert_kana()`: conversions between fullwidth ("zen-kaku") and
//! halfwidth ("han-kaku") forms and between hiragana and katakana (PHP
//! manual: mb_convert_kana; test ext/mbstring/tests/mb_convert_kana.phpt;
//! observed on PHP 8.5).

use super::MbError;
use crate::tables::kana as t;
use crate::{Encoding, Subst, convert};

/// The parsed mode flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KanaMode {
    bits: u32,
}

const R_LO: u32 = 1; // r
const R_UP: u32 = 1 << 1; // R
const N_LO: u32 = 1 << 2; // n
const N_UP: u32 = 1 << 3; // N
const A_LO: u32 = 1 << 4; // a
const A_UP: u32 = 1 << 5; // A
const S_LO: u32 = 1 << 6; // s
const S_UP: u32 = 1 << 7; // S
const K_LO: u32 = 1 << 8; // k
const K_UP: u32 = 1 << 9; // K
const H_LO: u32 = 1 << 10; // h
const H_UP: u32 = 1 << 11; // H
const C_LO: u32 = 1 << 12; // c
const C_UP: u32 = 1 << 13; // C
const V_UP: u32 = 1 << 14; // V
const M_LO: u32 = 1 << 15; // m
const M_UP: u32 = 1 << 16; // M

impl KanaMode {
    /// Parses a mode string. Errors as `mb_convert_kana()` reports them
    /// (message without the function prefix).
    pub fn parse(mode: &[u8]) -> Result<KanaMode, MbError> {
        let mut bits = 0;
        for &c in mode {
            bits |= match c {
                b'r' => R_LO,
                b'R' => R_UP,
                b'n' => N_LO,
                b'N' => N_UP,
                b'a' => A_LO,
                b'A' => A_UP,
                b's' => S_LO,
                b'S' => S_UP,
                b'k' => K_LO,
                b'K' => K_UP,
                b'h' => H_LO,
                b'H' => H_UP,
                b'c' => C_LO,
                b'C' => C_UP,
                b'V' => V_UP,
                b'm' => M_LO,
                b'M' => M_UP,
                _ => {
                    return Err(MbError::ValueError(format!(
                        "Argument #2 ($mode) contains invalid flag: '{}'",
                        c as char
                    )));
                }
            };
        }
        // test ext/mbstring/tests/mb_convert_kana.phpt
        const CONFLICTS: [(u32, u32, &str, &str); 15] = [
            (A_UP, A_LO, "A", "a"),
            (A_UP, R_LO, "A", "r"),
            (A_UP, N_LO, "A", "n"),
            (R_UP, A_LO, "R", "a"),
            (N_UP, A_LO, "N", "a"),
            (R_UP, R_LO, "R", "r"),
            (N_UP, N_LO, "N", "n"),
            (S_UP, S_LO, "S", "s"),
            (H_UP, K_UP, "H", "K"),
            (C_UP, C_LO, "C", "c"),
            (M_UP, M_LO, "M", "m"),
            (H_LO, C_UP, "h", "C"),
            (H_LO, C_LO, "h", "c"),
            (K_LO, C_UP, "k", "C"),
            (K_LO, C_LO, "k", "c"),
        ];
        for (a, b, na, nb) in CONFLICTS {
            if bits & a != 0 && bits & b != 0 {
                return Err(MbError::ValueError(format!(
                    "Argument #2 ($mode) must not combine '{na}' and '{nb}' flags"
                )));
            }
        }
        Ok(KanaMode { bits })
    }

    fn has(self, b: u32) -> bool {
        self.bits & b != 0
    }
}

/// Fullwidth katakana → halfwidth katakana, from the inverse of the
/// UnicodeData <narrow> decompositions plus voiced sound marks.
fn kata_to_half(cp: u32, out: &mut Vec<u32>) -> bool {
    if let Some(i) = t::HW_TO_FW.iter().position(|&f| f as u32 == cp) {
        out.push(0xFF61 + i as u32);
        return true;
    }
    match cp {
        0x309B => out.push(0xFF9E),
        0x309C => out.push(0xFF9F),
        // (observed on PHP 8.5)
        0x30F0 => out.push(0xFF72),
        0x30F1 => out.push(0xFF74),
        0x30EE => out.push(0xFF9C),
        _ => {
            // A voiced kana: base + sound mark.
            if let Some(v) = t::VOICED.iter().find(|v| v.2 as u32 == cp)
                && let Some(i) = t::HW_TO_FW.iter().position(|&f| f == v.0)
            {
                out.push(0xFF61 + i as u32);
                out.push(if v.1 == 0x3099 { 0xFF9E } else { 0xFF9F });
                return true;
            }
            return false;
        }
    }
    true
}

/// Halfwidth katakana → fullwidth katakana.
fn half_to_kata(cp: u32) -> u32 {
    match t::HW_TO_FW[(cp - 0xFF61) as usize] as u32 {
        0x3099 => 0x309B,
        0x309A => 0x309C,
        c => c,
    }
}

/// Hiragana with a katakana counterpart for `C` (ゔ, ゕ, ゖ are left alone;
/// observed on PHP 8.5).
fn is_hiragana_convertible(cp: u32) -> bool {
    (0x3041..=0x3093).contains(&cp) || cp == 0x309D || cp == 0x309E
}

/// Katakana with a hiragana counterpart for `c` (ヴ, ヵ, ヶ are left alone).
fn is_katakana_convertible(cp: u32) -> bool {
    (0x30A1..=0x30F3).contains(&cp) || cp == 0x30FD || cp == 0x30FE
}

/// Converts code points.
pub fn convert_kana_cps(cps: &[u32], mode: KanaMode, out: &mut Vec<u32>) {
    let m = mode;
    let mut i = 0;
    while i < cps.len() {
        let cp = cps[i];
        i += 1;
        // zen-kaku → han-kaku
        if (m.has(R_LO) || m.has(A_LO))
            && ((0xFF21..=0xFF3A).contains(&cp) || (0xFF41..=0xFF5A).contains(&cp))
            || (m.has(N_LO) || m.has(A_LO)) && (0xFF10..=0xFF19).contains(&cp)
        {
            out.push(cp - 0xFEE0);
            continue;
        }
        if m.has(A_LO) {
            if (0xFF01..=0xFF5E).contains(&cp) && !matches!(cp, 0xFF02 | 0xFF07 | 0xFF3C | 0xFF5E) {
                out.push(cp - 0xFEE0);
                continue;
            }
            if cp == 0x2212 {
                out.push(0x2D);
                continue;
            }
        }
        if m.has(S_LO) && cp == 0x3000 {
            out.push(0x20);
            continue;
        }
        if m.has(M_LO) {
            let r = match cp {
                0xFFE5 | 0xFF3C => 0x5C,
                0xFFE3 | 0x203E => 0x7E,
                0x2018 | 0x2019 => 0x27,
                0x201C | 0x201D => 0x22,
                _ => 0,
            };
            if r != 0 {
                out.push(r);
                continue;
            }
        }
        if m.has(H_LO) && (0x3041..=0x3093).contains(&cp) {
            // Hiragana as katakana, then halfwidth.
            if kata_to_half(cp + 0x60, out) {
                continue;
            }
        }
        if (m.has(K_LO) || m.has(H_LO))
            && matches!(
                cp,
                0x3001 | 0x3002 | 0x300C | 0x300D | 0x309B | 0x309C | 0x30FC | 0x30FB
            )
        {
            kata_to_half(cp, out);
            continue;
        }
        if m.has(K_LO) && (0x30A1..=0x30F4).contains(&cp) && kata_to_half(cp, out) {
            continue;
        }
        // han-kaku → zen-kaku
        if (m.has(R_UP) || m.has(A_UP)) && (cp.ascii_upper() || cp.ascii_lower())
            || (m.has(N_UP) || m.has(A_UP)) && (0x30..=0x39).contains(&cp)
        {
            out.push(cp + 0xFEE0);
            continue;
        }
        if m.has(A_UP) && (0x21..=0x7E).contains(&cp) && !matches!(cp, 0x22 | 0x27 | 0x5C | 0x7E) {
            out.push(cp + 0xFEE0);
            continue;
        }
        if m.has(S_UP) && cp == 0x20 {
            out.push(0x3000);
            continue;
        }
        if m.has(M_UP) {
            let r = match cp {
                0x5C | 0xA5 => 0xFFE5,
                0x7E | 0x203E => 0xFFE3,
                0x27 => 0x2019,
                0x22 => 0x201D,
                _ => 0,
            };
            if r != 0 {
                out.push(r);
                continue;
            }
        }
        if (m.has(K_UP) || m.has(H_UP)) && (0xFF61..=0xFF9F).contains(&cp) {
            let mut fw = half_to_kata(cp);
            if m.has(V_UP)
                && let Some(&next) = cps.get(i)
                && (next == 0xFF9E || next == 0xFF9F)
            {
                let mark = if next == 0xFF9E { 0x3099 } else { 0x309A };
                // With H, ｳﾞ stays two characters: hiragana has no ゔ here
                // (observed on PHP 8.5).
                if let Some(v) = t::VOICED
                    .iter()
                    .find(|v| v.0 as u32 == fw && v.1 == mark && !(m.has(H_UP) && v.2 == 0x30F4))
                {
                    fw = v.2 as u32;
                    i += 1;
                }
            }
            if m.has(H_UP) && (0x30A1..=0x30F3).contains(&fw) {
                fw -= 0x60;
            }
            out.push(fw);
            continue;
        }
        if m.has(C_LO) && is_katakana_convertible(cp) {
            out.push(cp - 0x60);
            continue;
        }
        if m.has(C_UP) && is_hiragana_convertible(cp) {
            out.push(cp + 0x60);
            continue;
        }
        out.push(cp);
    }
}

trait AsciiU32 {
    fn ascii_upper(self) -> bool;
    fn ascii_lower(self) -> bool;
}

impl AsciiU32 for u32 {
    fn ascii_upper(self) -> bool {
        (0x41..=0x5A).contains(&self)
    }
    fn ascii_lower(self) -> bool {
        (0x61..=0x7A).contains(&self)
    }
}

/// `mb_convert_kana()`.
pub fn convert_kana(s: &[u8], mode: KanaMode, enc: Encoding, sub: &Subst) -> Vec<u8> {
    let cps = convert::decode(s, enc);
    let mut out = Vec::with_capacity(cps.len());
    convert_kana_cps(&cps, mode, &mut out);
    convert::encode(&out, enc, sub).bytes
}
