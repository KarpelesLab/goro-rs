//! Names and comments: the UTF-8 flag (general purpose bit 11) and IBM
//! code page 437, the encoding of names without it (PKWARE APPNOTE.TXT
//! appendix D). The CP437 upper half follows the Unicode consortium's
//! published mapping (VENDORS/MICSFT/PC/CP437.TXT); the lower half is
//! ASCII.

/// CP437 bytes 0x80..=0xFF.
pub const CP437_HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', //
    'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', //
    'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', //
    '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', //
    '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', //
    '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀', //
    'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', //
    '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{a0}',
];

/// CP437 bytes as UTF-8.
pub fn cp437_to_utf8(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len() + 8);
    for &c in b {
        if c < 0x80 {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(
                CP437_HIGH[(c - 0x80) as usize]
                    .encode_utf8(&mut buf)
                    .as_bytes(),
            );
        }
    }
    out
}

/// UTF-8 text as CP437, when every character has a CP437 byte.
pub fn utf8_to_cp437(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    for ch in s.chars() {
        if (ch as u32) < 0x80 {
            out.push(ch as u8);
        } else {
            let i = CP437_HIGH.iter().position(|&c| c == ch)?;
            out.push(0x80 + i as u8);
        }
    }
    Some(out)
}

/// How a name or comment is read (libzip's `ZIP_FL_ENC_*`, PHP's
/// `ZipArchive::FL_ENC_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decode {
    /// The stored bytes.
    Raw,
    /// UTF-8 when flagged or valid UTF-8, otherwise CP437 converted to
    /// UTF-8.
    Guess,
    /// UTF-8 when flagged, otherwise CP437 converted to UTF-8.
    Strict,
}

/// A stored name or comment as UTF-8 per `mode`; `utf8_flag` is general
/// purpose bit 11.
pub fn decode(b: &[u8], utf8_flag: bool, mode: Decode) -> Vec<u8> {
    if mode == Decode::Raw || utf8_flag || b.is_ascii() {
        return b.to_vec();
    }
    if mode == Decode::Guess && std::str::from_utf8(b).is_ok() {
        return b.to_vec();
    }
    cp437_to_utf8(b)
}

/// Whether bit 11 should be set for a new name: non-ASCII valid UTF-8.
pub fn needs_utf8_flag(b: &[u8]) -> bool {
    !b.is_ascii() && std::str::from_utf8(b).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp437() {
        assert_eq!(cp437_to_utf8(&[0x82, b'/']), "é/".as_bytes());
        assert_eq!(cp437_to_utf8(&[0x91]), "æ".as_bytes());
        assert_eq!(
            utf8_to_cp437("Çüéâä").unwrap(),
            vec![0x80, 0x81, 0x82, 0x83, 0x84]
        );
        assert_eq!(decode(&[0x80], false, Decode::Guess), "Ç".as_bytes());
        assert_eq!(decode("è".as_bytes(), false, Decode::Guess), "è".as_bytes());
        assert_eq!(decode(&[0x80], false, Decode::Raw), vec![0x80]);
    }
}
