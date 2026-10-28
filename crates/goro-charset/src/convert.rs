//! Conversion between encodings (`mb_convert_encoding`, `mb_check_encoding`,
//! `mb_scrub`).

use crate::Encoding;
use crate::codec::{self, CHUNK, ERR, Subst, SubstMode, transform};

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
///
/// For BASE64, Quoted-Printable and UUENCODE the values are the decoded
/// bytes.
pub fn decode(input: &[u8], enc: Encoding) -> Vec<u32> {
    if let Some(v) = transform_decode(input, enc) {
        return v;
    }
    let mut out = Vec::new();
    codec::decode_all(input, enc, &mut out);
    out
}

/// Encodes code points into `enc`, substituting what it cannot represent.
pub fn encode(cps: &[u32], enc: Encoding, sub: &Subst) -> Converted {
    match enc {
        Encoding::Base64 | Encoding::QuotedPrintable | Encoding::Uuencode => {
            let bytes: Vec<u8> = cps.iter().map(|&c| c as u8).collect();
            let bytes = match enc {
                Encoding::Base64 => transform::base64_encode(&bytes),
                Encoding::QuotedPrintable => transform::qprint_encode(&bytes),
                _ => transform::uuencode(&bytes),
            };
            return Converted { bytes, illegal: 0 };
        }
        _ => {}
    }
    let mut bytes = Vec::new();
    let illegal = codec::encode_all(cps, enc, sub, &mut bytes);
    Converted { bytes, illegal }
}

fn transform_decode(input: &[u8], enc: Encoding) -> Option<Vec<u32>> {
    Some(match enc {
        Encoding::Base64 => transform::base64_decode(input),
        Encoding::QuotedPrintable => transform::qprint_decode(input),
        Encoding::Uuencode => transform::uudecode(input),
        _ => return None,
    })
}

/// Writes decoded bytes as they are, with the substitute for errors.
fn raw_bytes(vals: &[u32], sub: &Subst) -> Converted {
    let mut c = Converted::default();
    c.bytes.reserve(vals.len());
    for &v in vals {
        if v == ERR {
            c.illegal += 1;
            if sub.mode != SubstMode::None {
                c.bytes
                    .push(if sub.ch < 0x100 { sub.ch as u8 } else { b'?' });
            }
        } else {
            c.bytes.push(v as u8);
        }
    }
    c
}

/// Converts `input` from `from` to `to` (`mb_convert_encoding`).
///
/// Converting to BASE64 or Quoted-Printable encodes the input bytes
/// whatever `from` is; converting from BASE64, Quoted-Printable or UUENCODE
/// gives the decoded bytes whatever `to` is; converting to UUENCODE takes
/// the low byte of each character (observed on PHP 8.5).
pub fn convert(input: &[u8], to: Encoding, from: Encoding, sub: &Subst) -> Converted {
    match to {
        Encoding::Base64 => {
            return Converted {
                bytes: transform::base64_encode(input),
                illegal: 0,
            };
        }
        Encoding::QuotedPrintable => {
            return Converted {
                bytes: transform::qprint_encode(input),
                illegal: 0,
            };
        }
        _ => {}
    }
    if let Some(v) = transform_decode(input, from) {
        return raw_bytes(&v, sub);
    }
    if to == Encoding::Uuencode {
        return encode(&decode(input, from), to, sub);
    }
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
    e.finish(&mut out, sub, &mut illegal);
    Converted {
        bytes: out,
        illegal,
    }
}

/// Whether `input` is valid in `enc` (`mb_check_encoding`).
pub fn check_encoding(input: &[u8], enc: Encoding) -> bool {
    match enc {
        Encoding::Utf8 => return codec::utf8_valid_up_to(input) == input.len(),
        Encoding::Base64 => return !transform::base64_decode(input).contains(&ERR),
        Encoding::QuotedPrintable | Encoding::Uuencode => return true,
        _ => {}
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
    !buf.contains(&ERR) && !d.check_failed()
}

/// Replaces invalid sequences with the substitute character (`mb_scrub`).
pub fn scrub(input: &[u8], enc: Encoding, sub: &Subst) -> Vec<u8> {
    convert(input, enc, enc, sub).bytes
}
