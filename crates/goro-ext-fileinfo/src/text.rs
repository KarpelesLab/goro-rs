//! Text and character-encoding classification of a buffer, and the
//! description suffixes ("with CRLF line terminators", "with very long
//! lines (N)", ...).
//!
//! The classes, charset names and wording are the ones PHP 8.5's finfo
//! reports (observed on PHP 8.5 and tests ext/fileinfo/tests/bug68819_*.phpt,
//! finfo_buffer_basic.phpt, gh13344.phpt). The EBCDIC table is IBM code
//! page 037 (CCSID 37) mapped to ISO 8859-1.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enc {
    Ascii,
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
    Latin1,
    Extended,
    Ebcdic,
    IntlEbcdic,
    Binary,
}

#[derive(Debug, Clone)]
pub struct TextInfo {
    pub enc: Enc,
    /// The decoded characters (for the line analysis and text rules).
    pub chars: Vec<u32>,
}

/// Bytes accepted in "ASCII text": BEL..CR, ESC, printable ASCII, and NEL
/// (observed on PHP 8.5: "\x85" counts as a NEL line terminator).
pub fn ascii_text(c: u32) -> bool {
    matches!(c, 7..=13 | 27 | 0x20..=0x7e | 0x85)
}

fn latin1_text(c: u32) -> bool {
    ascii_text(c) || (0xa0..=0xff).contains(&c)
}

fn extended_text(c: u32) -> bool {
    latin1_text(c) || (0x80..=0x9f).contains(&c)
}

/// IBM code page 037 to ISO 8859-1.
const CP037: [u8; 256] = [
    0x00, 0x01, 0x02, 0x03, 0x9c, 0x09, 0x86, 0x7f, 0x97, 0x8d, 0x8e, 0x0b, 0x0c, 0x0d, 0x0e,
    0x0f, //
    0x10, 0x11, 0x12, 0x13, 0x9d, 0x85, 0x08, 0x87, 0x18, 0x19, 0x92, 0x8f, 0x1c, 0x1d, 0x1e,
    0x1f, //
    0x80, 0x81, 0x82, 0x83, 0x84, 0x0a, 0x17, 0x1b, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x05, 0x06,
    0x07, //
    0x90, 0x91, 0x16, 0x93, 0x94, 0x95, 0x96, 0x04, 0x98, 0x99, 0x9a, 0x9b, 0x14, 0x15, 0x9e,
    0x1a, //
    0x20, 0xa0, 0xe2, 0xe4, 0xe0, 0xe1, 0xe3, 0xe5, 0xe7, 0xf1, 0xa2, 0x2e, 0x3c, 0x28, 0x2b,
    0x7c, //
    0x26, 0xe9, 0xea, 0xeb, 0xe8, 0xed, 0xee, 0xef, 0xec, 0xdf, 0x21, 0x24, 0x2a, 0x29, 0x3b,
    0xac, //
    0x2d, 0x2f, 0xc2, 0xc4, 0xc0, 0xc1, 0xc3, 0xc5, 0xc7, 0xd1, 0xa6, 0x2c, 0x25, 0x5f, 0x3e,
    0x3f, //
    0xf8, 0xc9, 0xca, 0xcb, 0xc8, 0xcd, 0xce, 0xcf, 0xcc, 0x60, 0x3a, 0x23, 0x40, 0x27, 0x3d,
    0x22, //
    0xd8, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0xab, 0xbb, 0xf0, 0xfd, 0xfe,
    0xb1, //
    0xb0, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f, 0x70, 0x71, 0x72, 0xaa, 0xba, 0xe6, 0xb8, 0xc6,
    0xa4, //
    0xb5, 0x7e, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0xa1, 0xbf, 0xd0, 0xdd, 0xde,
    0xae, //
    0x5e, 0xa3, 0xa5, 0xb7, 0xa9, 0xa7, 0xb6, 0xbc, 0xbd, 0xbe, 0x5b, 0x5d, 0xaf, 0xa8, 0xb4,
    0xd7, //
    0x7b, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0xad, 0xf4, 0xf6, 0xf2, 0xf3,
    0xf5, //
    0x7d, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, 0x50, 0x51, 0x52, 0xb9, 0xfb, 0xfc, 0xf9, 0xfa,
    0xff, //
    0x5c, 0xf7, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0xb2, 0xd4, 0xd6, 0xd2, 0xd3,
    0xd5, //
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0xb3, 0xdb, 0xdc, 0xd9, 0xda,
    0x9f, //
];

/// Strict UTF-8 decoding (no overlongs, surrogates or values past
/// U+10FFFF); None when invalid.
fn decode_utf8(b: &[u8]) -> Option<Vec<u32>> {
    let s = match std::str::from_utf8(b) {
        Ok(s) => s,
        // A sequence cut by the end of the examined bytes.
        Err(e) if e.error_len().is_none() && e.valid_up_to() > 0 => {
            std::str::from_utf8(&b[..e.valid_up_to()]).ok()?
        }
        Err(_) => return None,
    };
    Some(s.chars().map(|c| c as u32).collect())
}

/// The encoding is decided on the first 64 KiB (observed on PHP 8.5: a
/// file whose first non-ASCII byte comes later is "ASCII text").
const ENCODING_MAX: usize = 65536;

impl TextInfo {
    pub fn analyze(b: &[u8]) -> TextInfo {
        let b = &b[..b.len().min(ENCODING_MAX)];
        let bin = TextInfo {
            enc: Enc::Binary,
            chars: Vec::new(),
        };
        // UTF-32 byte order marks.
        if b.len() >= 4 && (b[..4] == [0xff, 0xfe, 0, 0] || b[..4] == [0, 0, 0xfe, 0xff]) {
            let le = b[0] == 0xff;
            let chars: Vec<u32> = b[4..]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| {
                    if le {
                        u32::from_le_bytes([c[0], c[1], c[2], c[3]])
                    } else {
                        u32::from_be_bytes([c[0], c[1], c[2], c[3]])
                    }
                })
                .collect();
            return TextInfo {
                enc: if le { Enc::Utf32Le } else { Enc::Utf32Be },
                chars,
            };
        }
        if b.iter().all(|&c| ascii_text(c as u32)) {
            return TextInfo {
                enc: Enc::Ascii,
                chars: b.iter().map(|&c| c as u32).collect(),
            };
        }
        if let Some(rest) = b.strip_prefix(b"\xef\xbb\xbf")
            && let Some(ch) = decode_utf8(rest)
            && ch.iter().all(|&c| c >= 0x80 || ascii_text(c))
        {
            return TextInfo {
                enc: if rest.is_empty() {
                    Enc::Utf8
                } else {
                    Enc::Utf8Bom
                },
                chars: ch,
            };
        }
        if let Some(ch) = decode_utf8(b)
            && ch.iter().all(|&c| c >= 0x80 || ascii_text(c))
        {
            return TextInfo {
                enc: Enc::Utf8,
                chars: ch,
            };
        }
        if b.len() >= 2 && (b[..2] == [0xff, 0xfe] || b[..2] == [0xfe, 0xff]) {
            let le = b[0] == 0xff;
            let chars: Vec<u32> = b[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| {
                    if le {
                        u16::from_le_bytes([c[0], c[1]]) as u32
                    } else {
                        u16::from_be_bytes([c[0], c[1]]) as u32
                    }
                })
                .collect();
            if chars.iter().all(|&c| c >= 0x80 || ascii_text(c)) {
                return TextInfo {
                    enc: if le { Enc::Utf16Le } else { Enc::Utf16Be },
                    chars,
                };
            }
            return bin;
        }
        let chars: Vec<u32> = b.iter().map(|&c| c as u32).collect();
        if chars.iter().all(|&c| latin1_text(c)) {
            return TextInfo {
                enc: Enc::Latin1,
                chars,
            };
        }
        if chars.iter().all(|&c| extended_text(c)) {
            return TextInfo {
                enc: Enc::Extended,
                chars,
            };
        }
        let conv: Vec<u32> = b.iter().map(|&c| CP037[c as usize] as u32).collect();
        if conv.iter().all(|&c| ascii_text(c)) {
            return TextInfo {
                enc: Enc::Ebcdic,
                chars: conv,
            };
        }
        if conv.iter().all(|&c| latin1_text(c)) {
            return TextInfo {
                enc: Enc::IntlEbcdic,
                chars: conv,
            };
        }
        bin
    }

    pub fn is_text(&self) -> bool {
        match self.enc {
            Enc::Binary => false,
            Enc::Utf32Le | Enc::Utf32Be => self.utf32_valid(),
            _ => true,
        }
    }

    fn utf32_valid(&self) -> bool {
        self.chars
            .iter()
            .all(|&c| c <= 0x10ffff && !(0xd800..0xe000).contains(&c))
    }

    pub fn charset(&self) -> &'static str {
        match self.enc {
            Enc::Ascii => "us-ascii",
            Enc::Utf8 | Enc::Utf8Bom => "utf-8",
            Enc::Utf16Le => "utf-16le",
            Enc::Utf16Be => "utf-16be",
            Enc::Utf32Le => "utf-32le",
            Enc::Utf32Be => "utf-32be",
            Enc::Latin1 => "iso-8859-1",
            Enc::Extended => "unknown-8bit",
            Enc::Ebcdic | Enc::IntlEbcdic => "ebcdic",
            Enc::Binary => "binary",
        }
    }

    /// The encoding name in a description ("ASCII", "Unicode text, UTF-8").
    fn name(&self) -> &'static str {
        match self.enc {
            Enc::Ascii => "ASCII",
            Enc::Utf8 => "Unicode text, UTF-8",
            Enc::Utf8Bom => "Unicode text, UTF-8 (with BOM)",
            Enc::Utf16Le => "Unicode text, UTF-16, little-endian",
            Enc::Utf16Be => "Unicode text, UTF-16, big-endian",
            Enc::Utf32Le => "Unicode text, UTF-32, little-endian",
            Enc::Utf32Be => "Unicode text, UTF-32, big-endian",
            Enc::Latin1 => "ISO-8859",
            Enc::Extended => "Non-ISO extended-ASCII",
            Enc::Ebcdic => "EBCDIC",
            Enc::IntlEbcdic => "International EBCDIC",
            Enc::Binary => "data",
        }
    }

    /// "ASCII text executable, with CRLF line terminators" and so on.
    pub fn describe(&self) -> String {
        if matches!(self.enc, Enc::Utf32Le | Enc::Utf32Be) {
            return self.name().to_string();
        }
        let mut s = format!("{} text", self.name());
        let c = &self.chars;
        if c.len() >= 2 && c[0] == '#' as u32 && c[1] == '!' as u32 {
            s.push_str(" executable");
        }
        let (mut crlf, mut cr, mut lf, mut nel) = (false, false, false, false);
        let mut longest = 0usize;
        let mut cur = 0usize;
        let mut esc = false;
        let mut bs = false;
        let mut i = 0;
        while i < c.len() {
            let ch = c[i];
            match ch {
                0x0d => {
                    if c.get(i + 1) == Some(&0x0a) {
                        crlf = true;
                        i += 1;
                    } else {
                        cr = true;
                    }
                    longest = longest.max(cur);
                    cur = 0;
                }
                0x0a => {
                    lf = true;
                    longest = longest.max(cur);
                    cur = 0;
                }
                0x85 => {
                    nel = true;
                    longest = longest.max(cur);
                    cur = 0;
                }
                _ => {
                    if ch == 0x1b {
                        esc = true;
                    }
                    if ch == 0x08 {
                        bs = true;
                    }
                    cur += 1;
                }
            }
            i += 1;
        }
        longest = longest.max(cur);
        if longest > 300 {
            s.push_str(&format!(", with very long lines ({})", longest));
        }
        if !(crlf || cr || lf || nel) {
            s.push_str(", with no line terminators");
        } else if crlf || cr || nel {
            let mut kinds = Vec::new();
            if crlf {
                kinds.push("CRLF");
            }
            if cr {
                kinds.push("CR");
            }
            if lf {
                kinds.push("LF");
            }
            if nel {
                kinds.push("NEL");
            }
            s.push_str(&format!(", with {} line terminators", kinds.join(", ")));
        }
        if esc {
            s.push_str(", with escape sequences");
        }
        if bs {
            s.push_str(", with overstriking");
        }
        s
    }

    /// The buffer the text rules see: UTF-16 and UTF-32 are converted to
    /// UTF-8 (without the byte order mark), other encodings are used as is.
    pub fn text_buffer<'a>(&self, raw: &'a [u8]) -> std::borrow::Cow<'a, [u8]> {
        match self.enc {
            Enc::Utf16Le | Enc::Utf16Be | Enc::Utf32Le | Enc::Utf32Be => {
                let mut s = String::new();
                for &c in &self.chars {
                    s.push(char::from_u32(c).unwrap_or('\u{fffd}'));
                }
                std::borrow::Cow::Owned(s.into_bytes())
            }
            _ => std::borrow::Cow::Borrowed(raw),
        }
    }
}

// ----------------------------------------------------------------- JSON

/// RFC 8259 JSON. Returns the number of top-level values (objects or
/// arrays), each on its own line, or None when the text is not JSON.
pub fn json_values(b: &[u8]) -> Option<usize> {
    let mut p = Json { b, i: 0, depth: 0 };
    let mut n = 0;
    p.ws();
    loop {
        if p.i >= b.len() {
            break;
        }
        if !matches!(b[p.i], b'{' | b'[') {
            return None;
        }
        p.value()?;
        n += 1;
        // Blanks up to the end of the line, then the next value.
        let mut nl = false;
        while p.i < b.len() && matches!(b[p.i], b' ' | b'\t' | b'\r' | b'\n') {
            if b[p.i] == b'\n' {
                nl = true;
            }
            p.i += 1;
        }
        if p.i < b.len() && !nl {
            return None;
        }
    }
    (n > 0).then_some(n)
}

struct Json<'a> {
    b: &'a [u8],
    i: usize,
    depth: usize,
}

impl Json<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\r' | b'\n') {
            self.i += 1;
        }
    }
    fn lit(&mut self, s: &[u8]) -> Option<()> {
        if self.b[self.i..].starts_with(s) {
            self.i += s.len();
            Some(())
        } else {
            None
        }
    }
    fn value(&mut self) -> Option<()> {
        self.ws();
        let c = *self.b.get(self.i)?;
        match c {
            b'{' => {
                self.depth += 1;
                if self.depth > 500 {
                    return None;
                }
                self.i += 1;
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        if self.b.get(self.i) != Some(&b'"') {
                            return None;
                        }
                        self.string()?;
                        self.ws();
                        if self.b.get(self.i) != Some(&b':') {
                            return None;
                        }
                        self.i += 1;
                        self.value()?;
                        self.ws();
                        match self.b.get(self.i) {
                            Some(b',') => self.i += 1,
                            Some(b'}') => {
                                self.i += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                self.depth -= 1;
            }
            b'[' => {
                self.depth += 1;
                if self.depth > 500 {
                    return None;
                }
                self.i += 1;
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                } else {
                    loop {
                        self.value()?;
                        self.ws();
                        match self.b.get(self.i) {
                            Some(b',') => self.i += 1,
                            Some(b']') => {
                                self.i += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                self.depth -= 1;
            }
            b'"' => self.string()?,
            b't' => self.lit(b"true")?,
            b'f' => self.lit(b"false")?,
            b'n' => self.lit(b"null")?,
            b'-' | b'0'..=b'9' => self.number()?,
            _ => return None,
        }
        Some(())
    }
    fn string(&mut self) -> Option<()> {
        self.i += 1;
        loop {
            let c = *self.b.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return Some(()),
                b'\\' => {
                    let e = *self.b.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                        b'u' => {
                            let h = self.b.get(self.i..self.i + 4)?;
                            if !h.iter().all(|c| c.is_ascii_hexdigit()) {
                                return None;
                            }
                            self.i += 4;
                        }
                        _ => return None,
                    }
                }
                0..=0x1f => return None,
                _ => {}
            }
        }
    }
    fn number(&mut self) -> Option<()> {
        let st = self.i;
        if self.b.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        match self.b.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                while self.b.get(self.i).is_some_and(|c| c.is_ascii_digit()) {
                    self.i += 1;
                }
            }
            _ => return None,
        }
        if self.b.get(self.i) == Some(&b'.') {
            self.i += 1;
            if !self.b.get(self.i).is_some_and(|c| c.is_ascii_digit()) {
                return None;
            }
            while self.b.get(self.i).is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.b.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.b.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !self.b.get(self.i).is_some_and(|c| c.is_ascii_digit()) {
                return None;
            }
            while self.b.get(self.i).is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        (self.i > st).then_some(())
    }
}

// ------------------------------------------------------------------ CSV

/// RFC 4180 comma-separated values: at least two lines with the same
/// number (three or more) of fields.
pub fn is_csv(b: &[u8]) -> bool {
    let mut lines = 0;
    let mut want = 0usize;
    let mut fields = 1usize;
    let mut i = 0;
    let mut quoted = false;
    let mut at_start = true;
    let mut line_has = false;
    while i < b.len() {
        let c = b[i];
        if quoted {
            if c == b'"' {
                if b.get(i + 1) == Some(&b'"') {
                    i += 2;
                    continue;
                }
                quoted = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' if at_start => {
                quoted = true;
                at_start = false;
                line_has = true;
            }
            b',' => {
                fields += 1;
                at_start = true;
                line_has = true;
            }
            b'\r' | b'\n' => {
                if c == b'\r' && b.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                if line_has {
                    if lines == 0 {
                        want = fields;
                    } else if fields != want {
                        return false;
                    }
                    lines += 1;
                }
                fields = 1;
                at_start = true;
                line_has = false;
            }
            _ => {
                at_start = false;
                line_has = true;
            }
        }
        i += 1;
    }
    if line_has {
        if lines == 0 {
            want = fields;
        } else if fields != want {
            return false;
        }
        lines += 1;
    }
    lines >= 2 && want >= 3 && !quoted
}
