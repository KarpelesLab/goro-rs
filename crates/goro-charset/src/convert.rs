//! Conversion between encodings (`mb_convert_encoding`, `mb_check_encoding`,
//! `mb_scrub`).

use crate::Encoding;
use crate::codec::{self, CHUNK, ERR, Subst};

/// The result of a conversion.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Converted {
    /// The converted bytes.
    pub bytes: Vec<u8>,
    /// How many characters were invalid or unrepresentable (what mbstring
    /// adds to `mb_get_info('illegal_chars')`).
    pub illegal: usize,
}

/// Decodes `input` into code points ([`ERR`] for invalid sequences).
pub fn decode(input: &[u8], enc: Encoding) -> Vec<u32> {
    let mut out = Vec::new();
    codec::decode_all(input, enc, &mut out);
    out
}

/// Encodes code points into `enc`, substituting what it cannot represent.
pub fn encode(cps: &[u32], enc: Encoding, sub: &Subst) -> Converted {
    let mut bytes = Vec::new();
    let illegal = codec::encode_all(cps, enc, sub, &mut bytes);
    Converted { bytes, illegal }
}

/// Converts `input` from `from` to `to` (`mb_convert_encoding`).
pub fn convert(input: &[u8], to: Encoding, from: Encoding, sub: &Subst) -> Converted {
    if from == Encoding::Utf8 && to == Encoding::Utf8 {
        let valid = codec::utf8_valid_up_to(input);
        if valid == input.len() {
            return Converted {
                bytes: input.to_vec(),
                illegal: 0,
            };
        }
    }
    let mut d = codec::decoder(from);
    let mut e = codec::encoder(to);
    let mut out = Vec::with_capacity(input.len() + input.len() / 4);
    let mut buf: Vec<u32> = Vec::with_capacity(CHUNK + 64);
    let mut pos = 0;
    let mut illegal = 0;
    while pos < input.len() {
        buf.clear();
        d.decode(input, &mut pos, &mut buf, CHUNK);
        e.encode(&buf, &mut out, sub, &mut illegal);
    }
    buf.clear();
    d.finish(&mut buf);
    e.encode(&buf, &mut out, sub, &mut illegal);
    e.finish(&mut out);
    Converted {
        bytes: out,
        illegal,
    }
}

/// Whether `input` is valid in `enc` (`mb_check_encoding`).
pub fn check_encoding(input: &[u8], enc: Encoding) -> bool {
    if enc == Encoding::Utf8 {
        return codec::utf8_valid_up_to(input) == input.len();
    }
    let mut d = codec::decoder(enc);
    let mut buf: Vec<u32> = Vec::with_capacity(CHUNK + 64);
    let mut pos = 0;
    while pos < input.len() {
        buf.clear();
        d.decode(input, &mut pos, &mut buf, CHUNK);
        if buf.contains(&ERR) {
            return false;
        }
    }
    buf.clear();
    d.finish(&mut buf);
    !buf.contains(&ERR)
}

/// Replaces invalid sequences with the substitute character (`mb_scrub`).
pub fn scrub(input: &[u8], enc: Encoding, sub: &Subst) -> Vec<u8> {
    convert(input, enc, enc, sub).bytes
}
