//! The string operations behind the `mb_*` functions.

use crate::Encoding;
use crate::codec::{self, CHUNK};

/// The number of characters in `s` (`mb_strlen`).
///
/// For UCS-2, UCS-4 and UTF-32 it is the byte length divided by the unit
/// size, as mbstring computes it (observed on PHP 8.5: a BOM counts, a
/// trailing partial unit does not).
pub fn strlen(s: &[u8], enc: Encoding) -> usize {
    if let Some(w) = enc.fixed_unit_width() {
        return s.len() / w;
    }
    match enc {
        Encoding::EightBit | Encoding::Iso8859_1 => s.len(),
        Encoding::Utf8 if codec::utf8_valid_up_to(s) == s.len() => {
            s.iter().filter(|&&b| (b as i8) >= -0x40).count()
        }
        _ => {
            let mut d = codec::decoder(enc);
            let mut buf = Vec::with_capacity(CHUNK + 64);
            let mut pos = 0;
            let mut n = 0;
            while pos < s.len() {
                buf.clear();
                d.decode(s, &mut pos, &mut buf, CHUNK);
                n += buf.len();
            }
            buf.clear();
            d.finish(&mut buf);
            n + buf.len()
        }
    }
}
