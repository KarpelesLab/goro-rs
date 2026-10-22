//! Domain to ASCII / to Unicode as lexbor does them (lexbor/unicode/idna.c
//! with LXB_UNICODE_IDNA_FLAG_CHECK_BIDI | CHECK_JOINERS, which lexbor
//! accepts but does not implement): UTS #46 mapping and NFC, label
//! splitting on U+002E, Punycode (lexbor/punycode/punycode.c) and the
//! validity criteria lexbor checks (no "xn--" prefix, no U+002E, only
//! valid or deviation code points). There are no hyphen, bidi, joiner or
//! DNS length checks.
//!
//! The UTS #46 mapping data comes from ICU4X (through idna_adapter), as
//! for the rest of the engine's IDNA support.

#![allow(clippy::result_unit_err)]

use super::whatwg::{DECODE_ERROR, decode_valid_utf8_single};

fn adapter() -> &'static idna_adapter::Adapter {
    static A: idna_adapter::Adapter = idna_adapter::Adapter::new();
    &A
}

/// Whether `cp` is a valid (or deviation) code point for UTS #46
/// non-transitional processing.
fn is_valid_cp(cp: u32) -> bool {
    let Some(c) = char::from_u32(cp) else {
        return false;
    };
    if c == '\u{FFFD}' {
        return false;
    }
    let mut it = adapter().normalize_validate(std::iter::once(c));
    it.next() == Some(c) && it.next().is_none()
}

fn starts_xn(cps: &[u32]) -> bool {
    cps.len() >= 4
        && (cps[0] == 'x' as u32 || cps[0] == 'X' as u32)
        && (cps[1] == 'n' as u32 || cps[1] == 'N' as u32)
        && cps[2] == '-' as u32
        && cps[3] == '-' as u32
}

/// lxb_unicode_idna_validity_criteria_cp() (no CHECK_HYPHENS, no
/// transitional processing)
fn validity_criteria(cps: &[u32]) -> bool {
    if starts_xn(cps) {
        return false;
    }
    cps.iter().all(|&cp| cp != '.' as u32 && is_valid_cp(cp))
}

/// lxb_unicode_idna_processing_body(): map, normalize and hand each label
/// to `cb` (with the labels lexbor calls it for).
fn processing(data: &[u8], cb: &mut dyn FnMut(&[u32]) -> Result<(), ()>) -> Result<(), ()> {
    // Decode
    let mut cps: Vec<u32> = Vec::with_capacity(data.len());
    let mut p = 0usize;
    while p < data.len() {
        let cp = decode_valid_utf8_single(data, &mut p, data.len());
        if cp > 0x10FFFF || cp == DECODE_ERROR {
            return Err(());
        }
        cps.push(cp);
    }
    // Code points that are not scalar values are disallowed: the label
    // holding them fails the validity criteria.
    let mut chars = Vec::with_capacity(cps.len());
    for cp in cps {
        match char::from_u32(cp) {
            Some(c) => chars.push(c),
            None => return Err(()),
        }
    }
    // Map + NFC (disallowed code points come out as U+FFFD, which fails
    // the validity criteria as the original code point would)
    let mapped: Vec<u32> = adapter()
        .map_normalize(chars.into_iter())
        .map(|c| c as u32)
        .collect();
    // lxb_unicode_idna_norm_c_cb()
    let mut start = 0usize;
    for (i, &cp) in mapped.iter().enumerate() {
        if cp == '.' as u32 {
            norm_c_send(&mapped[start..i], cb)?;
            start = i + 1;
        }
    }
    if mapped.len() > start || mapped.last() == Some(&('.' as u32)) {
        norm_c_send(&mapped[start..], cb)?;
    }
    Ok(())
}

/// lxb_unicode_idna_norm_c_send()
fn norm_c_send(label: &[u32], cb: &mut dyn FnMut(&[u32]) -> Result<(), ()>) -> Result<(), ()> {
    if starts_xn(label) {
        if let Some(decoded) = punycode_decode(&label[4..]) {
            if validity_criteria(&decoded) && cb(&decoded).is_ok() {
                return Ok(());
            }
        }
    }
    if !validity_criteria(label) {
        return Err(());
    }
    cb(label)
}

/// lxb_unicode_idna_to_ascii()
pub fn to_ascii(data: &[u8]) -> Result<Vec<u8>, ()> {
    let mut out: Vec<u8> = Vec::new();
    processing(data, &mut |label| {
        let (enc, unchanged) = punycode_encode(label)?;
        if !unchanged {
            out.extend_from_slice(b"xn--");
        }
        out.extend_from_slice(&enc);
        out.push(b'.');
        Ok(())
    })?;
    out.pop();
    Ok(out)
}

/// lxb_unicode_idna_to_unicode()
pub fn to_unicode(data: &[u8]) -> Result<Vec<u8>, ()> {
    let mut out: Vec<u8> = Vec::new();
    processing(data, &mut |label| {
        for &cp in label {
            encode_utf8(cp, &mut out)?;
        }
        out.push(b'.');
        Ok(())
    })?;
    out.pop();
    Ok(out)
}

/// lxb_encoding_encode_utf_8_single()
fn encode_utf8(cp: u32, out: &mut Vec<u8>) -> Result<(), ()> {
    if cp < 0x80 {
        out.push(cp as u8);
    } else if cp < 0x800 {
        out.push((0xC0 | (cp >> 6)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
    } else if cp < 0x10000 {
        out.push((0xE0 | (cp >> 12)) as u8);
        out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
    } else if cp < 0x110000 {
        out.push((0xF0 | (cp >> 18)) as u8);
        out.push((0x80 | ((cp >> 12) & 0x3F)) as u8);
        out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
    } else {
        return Err(());
    }
    Ok(())
}

// ----------------------------------------------------------------------
// Punycode (RFC 3492, lexbor/punycode/punycode.c)

const BASE: u32 = 36;
const TMIN: u32 = 1;
const TMAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 0x80;
const DELIMITER: u32 = '-' as u32;

fn encode_digit(d: u32) -> u8 {
    (d + 22 + 75 * (d < 26) as u32) as u8
}

fn decode_digit(cp: u32) -> u32 {
    if cp.wrapping_sub(48) < 10 {
        cp - 22
    } else if cp.wrapping_sub(65) < 26 {
        cp - 65
    } else if cp.wrapping_sub(97) < 26 {
        cp - 97
    } else {
        BASE
    }
}

fn adapt(mut delta: u32, numpoints: u32, firsttime: bool) -> u32 {
    delta = if firsttime { delta / DAMP } else { delta >> 1 };
    delta += delta / numpoints;
    let mut k = 0;
    while delta > ((BASE - TMIN) * TMAX) / 2 {
        delta /= BASE - TMIN;
        k += BASE;
    }
    k + (BASE - TMIN + 1) * delta / (delta + SKEW)
}

/// lxb_punycode_encode_cp(): the encoding and whether the label was all
/// basic code points (output unchanged, no "xn--" prefix).
fn punycode_encode(cps: &[u32]) -> Result<(Vec<u8>, bool), ()> {
    let mut out: Vec<u8> = cps
        .iter()
        .filter(|&&c| c < 0x80)
        .map(|&c| c as u8)
        .collect();
    let b = out.len();
    let mut h = b;
    if h >= cps.len() {
        return Ok((out, true));
    }
    if b > 0 {
        out.push(DELIMITER as u8);
    }
    let mut n = INITIAL_N;
    let mut bias = INITIAL_BIAS;
    let mut delta: u32 = 0;
    while h < cps.len() {
        let m = cps
            .iter()
            .copied()
            .filter(|&c| c >= n)
            .min()
            .unwrap_or(u32::MAX);
        let hh = (h + 1) as u32;
        if m - n > (u32::MAX - delta) / hh {
            return Err(());
        }
        delta += (m - n) * hh;
        n = m;
        for &c in cps {
            if c < n {
                delta = delta.wrapping_add(1);
                if delta == 0 {
                    return Err(());
                }
            }
            if c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias {
                        TMIN
                    } else if k >= bias + TMAX {
                        TMAX
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    out.push(encode_digit(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                out.push(encode_digit(q));
                bias = adapt(delta, (h + 1) as u32, h == b);
                delta = 0;
                h += 1;
            }
        }
        delta = delta.wrapping_add(1);
        n += 1;
    }
    Ok((out, false))
}

/// lxb_punycode_decode_cp()
fn punycode_decode(data: &[u32]) -> Option<Vec<u32>> {
    let length = data.len();
    let mut delimiter = length;
    while delimiter != 0 {
        delimiter -= 1;
        if data[delimiter] == DELIMITER {
            break;
        }
    }
    let mut out: Vec<u32> = Vec::new();
    for &cp in &data[..delimiter] {
        if cp >= 0x80 {
            return None;
        }
        out.push(cp);
    }
    let mut i: u32 = 0;
    let mut n = INITIAL_N;
    let mut bias = INITIAL_BIAS;
    let mut inp = if delimiter != 0 { delimiter + 1 } else { 0 };
    while inp < length {
        let oldi = i;
        let mut w: u32 = 1;
        let mut k = BASE;
        loop {
            if inp >= length {
                return None;
            }
            let digit = decode_digit(data[inp]);
            inp += 1;
            if digit >= BASE {
                return None;
            }
            if digit > (u32::MAX - i) / w {
                return None;
            }
            i += digit * w;
            let t = if k <= bias {
                TMIN
            } else if k >= bias + TMAX {
                TMAX
            } else {
                k - bias
            };
            if digit < t {
                break;
            }
            if w > u32::MAX / (BASE - t) {
                return None;
            }
            w *= BASE - t;
            k += BASE;
        }
        let h = (out.len() + 1) as u32;
        bias = adapt(i - oldi, h, oldi == 0);
        if i / h > u32::MAX - n {
            return None;
        }
        n += i / h;
        i %= h;
        out.insert(i as usize, n);
        i += 1;
    }
    Some(out)
}
