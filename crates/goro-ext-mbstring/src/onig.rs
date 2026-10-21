//! A backtracking regular expression engine compatible with the parts of
//! Oniguruma (Ruby syntax) used by PHP's mb_ereg* functions.
//!
//! Patterns and subjects are decoded into "codes" (Unicode codepoints for
//! Unicode encodings, raw multibyte values for legacy CJK encodings, like
//! Oniguruma does) and matched by a small VM with an explicit backtrack
//! stack, so match-stack and retry limits can be enforced.

use crate::unicode;

/* ===== Options and syntax ===== */

pub const OPT_IGNORECASE: u32 = 1;
pub const OPT_EXTEND: u32 = 2;
pub const OPT_MULTILINE: u32 = 4; // '.' matches newline
pub const OPT_SINGLELINE: u32 = 8; // '^' -> '\A', '$' -> '\Z'
pub const OPT_FIND_LONGEST: u32 = 16;
pub const OPT_FIND_NOT_EMPTY: u32 = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Syntax {
    Java,
    GnuRegex,
    Grep,
    Emacs,
    Ruby,
    Perl,
    PosixBasic,
    PosixExtended,
}

/* ===== Encodings ===== */

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegEnc {
    Utf8,
    Utf16Be,
    Utf16Le,
    Utf32Be,
    Utf32Le,
    EucJp,
    Sjis,
    Big5,
    EucCn,
    EucTw,
    EucKr,
    Koi8,
    Koi8R,
    Latin1,
    SingleByte, // other ISO-8859-x
    Ascii,
}

impl RegEnc {
    fn is_unicode(self) -> bool {
        matches!(self, RegEnc::Utf8 | RegEnc::Utf16Be | RegEnc::Utf16Le | RegEnc::Utf32Be | RegEnc::Utf32Le | RegEnc::Latin1)
    }

    /// Length in bytes of the character starting at s[p]
    fn char_len(self, s: &[u8], p: usize) -> usize {
        let c = s[p];
        let n = match self {
            RegEnc::Utf8 => {
                if c < 0x80 {
                    1
                } else if c < 0xC0 {
                    1
                } else if c < 0xE0 {
                    2
                } else if c < 0xF0 {
                    3
                } else if c < 0xF8 {
                    4
                } else {
                    1
                }
            }
            RegEnc::Utf16Be | RegEnc::Utf16Le => {
                if p + 1 < s.len() {
                    let u = if self == RegEnc::Utf16Be { ((c as u32) << 8) | s[p + 1] as u32 } else { ((s[p + 1] as u32) << 8) | c as u32 };
                    if (0xD800..0xDC00).contains(&u) { 4 } else { 2 }
                } else {
                    1
                }
            }
            RegEnc::Utf32Be | RegEnc::Utf32Le => 4,
            RegEnc::EucJp => {
                if c == 0x8F {
                    3
                } else if c == 0x8E || (0xA1..=0xFE).contains(&c) {
                    2
                } else {
                    1
                }
            }
            RegEnc::Sjis => {
                if (0x81..=0x9F).contains(&c) || (0xE0..=0xFC).contains(&c) {
                    2
                } else {
                    1
                }
            }
            RegEnc::Big5 | RegEnc::EucKr | RegEnc::EucCn => {
                if (0xA1..=0xFE).contains(&c) || (self == RegEnc::Big5 && (0x81..=0xFE).contains(&c)) {
                    2
                } else {
                    1
                }
            }
            RegEnc::EucTw => {
                if c == 0x8E {
                    4
                } else if (0xA1..=0xFE).contains(&c) {
                    2
                } else {
                    1
                }
            }
            _ => 1,
        };
        n.min(s.len() - p).max(1)
    }

    fn decode_char(self, b: &[u8]) -> u32 {
        match self {
            RegEnc::Utf8 => match b.len() {
                1 => b[0] as u32,
                2 => (((b[0] & 0x1F) as u32) << 6) | (b[1] & 0x3F) as u32,
                3 => (((b[0] & 0x0F) as u32) << 12) | (((b[1] & 0x3F) as u32) << 6) | (b[2] & 0x3F) as u32,
                _ => (((b[0] & 0x07) as u32) << 18) | (((b[1] & 0x3F) as u32) << 12) | (((b[2] & 0x3F) as u32) << 6) | (b[3] & 0x3F) as u32,
            },
            RegEnc::Utf16Be | RegEnc::Utf16Le => {
                let unit = |i: usize| -> u32 {
                    if i + 1 >= b.len() {
                        return 0;
                    }
                    if self == RegEnc::Utf16Be {
                        ((b[i] as u32) << 8) | b[i + 1] as u32
                    } else {
                        ((b[i + 1] as u32) << 8) | b[i] as u32
                    }
                };
                if b.len() < 2 {
                    return b[0] as u32;
                }
                let u = unit(0);
                if b.len() == 4 {
                    let l = unit(2);
                    0x10000 + ((u - 0xD800) << 10) + (l.wrapping_sub(0xDC00) & 0x3FF)
                } else {
                    u
                }
            }
            RegEnc::Utf32Be => {
                if b.len() < 4 {
                    return b[0] as u32;
                }
                u32::from_be_bytes([b[0], b[1], b[2], b[3]])
            }
            RegEnc::Utf32Le => {
                if b.len() < 4 {
                    return b[0] as u32;
                }
                u32::from_le_bytes([b[0], b[1], b[2], b[3]])
            }
            _ => {
                let mut v = 0u32;
                for &x in b {
                    v = (v << 8) | x as u32;
                }
                v
            }
        }
    }

    /// Decode a whole string into codes and the byte offset of each code
    /// (offs has one extra entry: the total length).
    pub fn decode_all(self, s: &[u8]) -> (Vec<u32>, Vec<usize>) {
        let mut codes = Vec::with_capacity(s.len());
        let mut offs = Vec::with_capacity(s.len() + 1);
        let mut p = 0;
        while p < s.len() {
            let n = self.char_len(s, p);
            codes.push(self.decode_char(&s[p..p + n]));
            offs.push(p);
            p += n;
        }
        offs.push(s.len());
        (codes, offs)
    }
}

/* ===== Character types ===== */

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ctype {
    Alpha,
    Blank,
    Cntrl,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Space,
    Upper,
    XDigit,
    Word,
    Alnum,
    Ascii,
    Any,
    Hex,      // \h
    Gc(&'static [usize]), // Unicode general categories (prop indices)
    Range(u32, u32), // script ranges etc.
    Ranges(&'static [(u32, u32)]),
    Newline,
}

fn is_space_code(c: u32) -> bool {
    matches!(c, 0x09..=0x0D | 0x20 | 0x85 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000)
}

fn is_pc(c: u32) -> bool {
    matches!(c, 0x5F | 0x203F | 0x2040 | 0x2054 | 0xFE33 | 0xFE34 | 0xFE4D..=0xFE4F | 0xFF3F)
}

const LETTER: &[usize] = &[unicode::UC_LU, unicode::UC_LL, unicode::UC_LT, unicode::UC_LM, unicode::UC_LO];

fn uni_is(c: u32, props: &[usize]) -> bool {
    unicode::is_prop(c, props)
}

fn ctype_match(ct: Ctype, c: u32, enc: RegEnc) -> bool {
    let ascii = c < 0x80;
    if !enc.is_unicode() && !ascii {
        // Legacy multibyte encodings: multibyte characters are word/graph/print
        return match ct {
            Ctype::Word | Ctype::Graph | Ctype::Print | Ctype::Any => c > 0xFF || enc == RegEnc::Koi8 || enc == RegEnc::Koi8R || enc == RegEnc::SingleByte,
            Ctype::Gc(_) | Ctype::Range(..) | Ctype::Ranges(_) => false,
            _ => false,
        };
    }
    match ct {
        Ctype::Any => true,
        Ctype::Ascii => c < 0x80,
        Ctype::Alpha => {
            if ascii {
                (c as u8).is_ascii_alphabetic()
            } else {
                uni_is(c, LETTER) || uni_is(c, &[unicode::UC_NL])
            }
        }
        Ctype::Blank => c == 0x09 || c == 0x20 || (!ascii && uni_is(c, &[unicode::UC_ZS])),
        Ctype::Cntrl => {
            if ascii {
                c < 0x20 || c == 0x7F
            } else {
                (0x80..0xA0).contains(&c) || uni_is(c, &[unicode::UC_C])
            }
        }
        Ctype::Digit => {
            if ascii {
                (c as u8).is_ascii_digit()
            } else {
                uni_is(c, &[unicode::UC_ND])
            }
        }
        Ctype::Lower => {
            if ascii {
                (c as u8).is_ascii_lowercase()
            } else {
                uni_is(c, &[unicode::UC_LL])
            }
        }
        Ctype::Upper => {
            if ascii {
                (c as u8).is_ascii_uppercase()
            } else {
                uni_is(c, &[unicode::UC_LU])
            }
        }
        Ctype::Space => is_space_code(c),
        Ctype::XDigit | Ctype::Hex => ascii && (c as u8).is_ascii_hexdigit(),
        Ctype::Word => {
            if ascii {
                (c as u8).is_ascii_alphanumeric() || c == b'_' as u32
            } else {
                uni_is(c, LETTER)
                    || uni_is(c, &[unicode::UC_MN, unicode::UC_MC, unicode::UC_ME, unicode::UC_ND, unicode::UC_NL])
                    || is_pc(c)
            }
        }
        Ctype::Alnum => ctype_match(Ctype::Alpha, c, enc) || ctype_match(Ctype::Digit, c, enc),
        Ctype::Punct => {
            if ascii {
                (c as u8).is_ascii_punctuation()
            } else {
                uni_is(c, &[unicode::UC_P])
            }
        }
        Ctype::Graph => {
            if ascii {
                (0x21..0x7F).contains(&c)
            } else {
                !is_space_code(c) && !uni_is(c, &[unicode::UC_C, 11 /* Cn */, 9 /* Cs */]) && !(0x80..0xA0).contains(&c)
            }
        }
        Ctype::Print => {
            if ascii {
                (0x20..0x7F).contains(&c)
            } else {
                ctype_match(Ctype::Graph, c, enc) || uni_is(c, &[unicode::UC_ZS])
            }
        }
        Ctype::Gc(props) => uni_is(c, props),
        Ctype::Range(lo, hi) => c >= lo && c <= hi,
        Ctype::Ranges(rs) => rs.iter().any(|&(lo, hi)| c >= lo && c <= hi),
        Ctype::Newline => c == 0x0A,
    }
}

fn posix_bracket(name: &str) -> Option<Ctype> {
    Some(match name {
        "alnum" => Ctype::Alnum,
        "alpha" => Ctype::Alpha,
        "ascii" => Ctype::Ascii,
        "blank" => Ctype::Blank,
        "cntrl" => Ctype::Cntrl,
        "digit" => Ctype::Digit,
        "graph" => Ctype::Graph,
        "lower" => Ctype::Lower,
        "print" => Ctype::Print,
        "punct" => Ctype::Punct,
        "space" => Ctype::Space,
        "upper" => Ctype::Upper,
        "xdigit" => Ctype::XDigit,
        "word" => Ctype::Word,
        _ => return None,
    })
}

fn property(name: &str) -> Option<Ctype> {
    let n: String = name.chars().filter(|c| *c != '_' && *c != ' ' && *c != '-').collect::<String>().to_ascii_lowercase();
    use unicode::*;
    Some(match n.as_str() {
        "alnum" => Ctype::Alnum,
        "alpha" | "alphabetic" => Ctype::Alpha,
        "ascii" => Ctype::Ascii,
        "blank" => Ctype::Blank,
        "cntrl" => Ctype::Cntrl,
        "digit" => Ctype::Digit,
        "graph" => Ctype::Graph,
        "lower" | "lowercase" => Ctype::Lower,
        "print" => Ctype::Print,
        "punct" | "punctuation" | "p" => Ctype::Gc(&[UC_P]),
        "space" | "whitespace" => Ctype::Space,
        "upper" | "uppercase" => Ctype::Upper,
        "xdigit" => Ctype::XDigit,
        "word" => Ctype::Word,
        "any" => Ctype::Any,
        "l" | "letter" => Ctype::Gc(LETTER),
        "lu" | "uppercaseletter" => Ctype::Gc(&[UC_LU]),
        "ll" | "lowercaseletter" => Ctype::Gc(&[UC_LL]),
        "lt" | "titlecaseletter" => Ctype::Gc(&[UC_LT]),
        "lm" | "modifierletter" => Ctype::Gc(&[UC_LM]),
        "lo" | "otherletter" => Ctype::Gc(&[UC_LO]),
        "m" | "mark" | "combiningmark" => Ctype::Gc(&[UC_MN, UC_MC, UC_ME]),
        "mn" | "nonspacingmark" => Ctype::Gc(&[UC_MN]),
        "mc" | "spacingmark" => Ctype::Gc(&[UC_MC]),
        "me" | "enclosingmark" => Ctype::Gc(&[UC_ME]),
        "n" | "number" => Ctype::Gc(&[UC_ND, UC_NL, UC_NO]),
        "nd" | "decimalnumber" => Ctype::Gc(&[UC_ND]),
        "nl" | "letternumber" => Ctype::Gc(&[UC_NL]),
        "no" | "othernumber" => Ctype::Gc(&[UC_NO]),
        "s" | "symbol" => Ctype::Gc(&[17, 18, 19, 20]),
        "sm" | "mathsymbol" => Ctype::Gc(&[17]),
        "sc" | "currencysymbol" => Ctype::Gc(&[18]),
        "sk" | "modifiersymbol" => Ctype::Gc(&[19]),
        "so" | "othersymbol" => Ctype::Gc(&[20]),
        "z" | "separator" => Ctype::Gc(&[UC_ZS, 7, 8]),
        "zs" | "spaceseparator" => Ctype::Gc(&[UC_ZS]),
        "zl" | "lineseparator" => Ctype::Gc(&[7]),
        "zp" | "paragraphseparator" => Ctype::Gc(&[8]),
        "c" | "other" => Ctype::Gc(&[UC_C, 9, 10, 11]),
        "cc" | "control" => Ctype::Gc(&[UC_C]),
        "co" | "privateuse" => Ctype::Gc(&[10]),
        "cn" | "unassigned" => Ctype::Gc(&[11]),
        "cs" | "surrogate" => Ctype::Gc(&[9]),
        "hiragana" => Ctype::Ranges(&[(0x3041, 0x3096), (0x309D, 0x309F), (0x1B001, 0x1B11E), (0x1F200, 0x1F200)]),
        "katakana" => Ctype::Ranges(&[(0x30A1, 0x30FA), (0x30FD, 0x30FF), (0x31F0, 0x31FF), (0x32D0, 0x32FE), (0x3300, 0x3357), (0xFF66, 0xFF6F), (0xFF71, 0xFF9D), (0x1B000, 0x1B000)]),
        "han" => Ctype::Ranges(&[(0x2E80, 0x2E99), (0x2E9B, 0x2EF3), (0x2F00, 0x2FD5), (0x3005, 0x3005), (0x3007, 0x3007), (0x3021, 0x3029), (0x3038, 0x303B), (0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xF900, 0xFA6D), (0xFA70, 0xFAD9), (0x20000, 0x2FA1D), (0x30000, 0x3134A)]),
        "latin" => Ctype::Ranges(&[(0x41, 0x5A), (0x61, 0x7A), (0xAA, 0xAA), (0xBA, 0xBA), (0xC0, 0xD6), (0xD8, 0xF6), (0xF8, 0x2B8), (0x2E0, 0x2E4), (0x1D00, 0x1D25), (0x1E00, 0x1EFF), (0x2C60, 0x2C7F), (0xA722, 0xA7FF), (0xFF21, 0xFF3A), (0xFF41, 0xFF5A)]),
        "greek" => Ctype::Ranges(&[(0x370, 0x3FF), (0x1F00, 0x1FFF)]),
        "cyrillic" => Ctype::Ranges(&[(0x400, 0x52F), (0x1C80, 0x1C88), (0x2DE0, 0x2DFF), (0xA640, 0xA69F)]),
        "arabic" => Ctype::Ranges(&[(0x600, 0x6FF), (0x750, 0x77F), (0x8A0, 0x8FF), (0xFB50, 0xFDFF), (0xFE70, 0xFEFF)]),
        "hebrew" => Ctype::Ranges(&[(0x591, 0x5F4), (0xFB1D, 0xFB4F)]),
        "hangul" => Ctype::Ranges(&[(0x1100, 0x11FF), (0x3131, 0x318E), (0xA960, 0xA97F), (0xAC00, 0xD7A3), (0xD7B0, 0xD7FF), (0xFFA0, 0xFFDC)]),
        "thai" => Ctype::Range(0xE01, 0xE5B),
        "common" => Ctype::Ranges(&[(0x0, 0x40), (0x5B, 0x60), (0x7B, 0xA9), (0xAB, 0xB9), (0xBB, 0xBF), (0xD7, 0xD7), (0xF7, 0xF7), (0x2000, 0x206F), (0x3000, 0x3004), (0xFF01, 0xFF20)]),
        _ => return None,
    })
}

/* ===== Character classes ===== */

#[derive(Clone, Debug, Default)]
pub struct CharClass {
    negated: bool,
    ranges: Vec<(u32, u32)>,
    ctypes: Vec<(Ctype, bool)>, // (type, negated)
    subs: Vec<CharClass>,
    and: Vec<CharClass>, // intersection operands (all must match)
}

impl CharClass {
    fn matches_raw(&self, c: u32, enc: RegEnc) -> bool {
        let mut m = self.ranges.iter().any(|&(lo, hi)| c >= lo && c <= hi)
            || self.ctypes.iter().any(|&(ct, neg)| ctype_match(ct, c, enc) != neg)
            || self.subs.iter().any(|s| s.matches_raw(c, enc));
        if !self.and.is_empty() {
            // Intersection: the class is (own items, if any) && each operand
            let has_own = !self.ranges.is_empty() || !self.ctypes.is_empty() || !self.subs.is_empty();
            if !has_own {
                m = true;
            }
            m = m && self.and.iter().all(|s| s.matches_raw(c, enc));
        }
        m != self.negated
    }

    fn matches(&self, c: u32, ci: bool, enc: RegEnc) -> bool {
        if self.matches_raw(c, enc) {
            return true;
        }
        if ci {
            for v in case_variants(c, enc) {
                if v != c && self.matches_raw(v, enc) {
                    return true;
                }
            }
        }
        false
    }
}

fn case_variants(c: u32, enc: RegEnc) -> Vec<u32> {
    let mut v = Vec::with_capacity(4);
    if c < 0x80 {
        let b = c as u8;
        v.push(b.to_ascii_lowercase() as u32);
        v.push(b.to_ascii_uppercase() as u32);
        return v;
    }
    if !enc.is_unicode() {
        return v;
    }
    let utf8 = &crate::mbfl::basic::ENC_UTF8;
    v.push(unicode::tolower_simple(c, utf8));
    v.push(unicode::toupper_simple(c, utf8));
    v.push(unicode::tofold_simple(c, utf8));
    v
}

#[inline]
fn fold(c: u32, enc: RegEnc) -> u32 {
    if c < 0x80 {
        return (c as u8).to_ascii_lowercase() as u32;
    }
    if !enc.is_unicode() {
        return c;
    }
    unicode::tofold_simple(c, &crate::mbfl::basic::ENC_UTF8)
}

/* ===== AST ===== */

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Anchor {
    BeginLine,
    EndLine,
    BeginBuf,
    EndBuf,
    SemiEndBuf,
    WordBound,
    NotWordBound,
    BeginPosition,
}

#[derive(Clone, Debug)]
enum Node {
    Empty,
    Char(u32, bool),
    Any(bool), // dotall
    Class(Box<CharClass>, bool),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    /// Group; capture number assigned in parse order (0 = none)
    Group(Box<Node>, usize),
    Repeat { node: Box<Node>, min: u32, max: Option<u32>, greedy: bool, possessive: bool },
    Atomic(Box<Node>),
    Look { node: Box<Node>, ahead: bool, negate: bool },
    Backref(Vec<usize>, bool),
    /// Backref by provisional number (resolved after parsing)
    BackrefNum(usize, bool),
    BackrefName(Vec<u8>, bool),
    Anchor(Anchor),
    Keep,
}

/* ===== Parser ===== */

pub struct ParseError(pub String);

struct Parser<'a> {
    p: &'a [u32],
    pos: usize,
    enc: RegEnc,
    syntax: Syntax,
    ngroups: usize,
    /// (name, provisional group number) in definition order
    names: Vec<(Vec<u8>, usize)>,
    has_numbered_backref: bool,
    depth: usize,
}

fn err<T>(s: &str) -> Result<T, ParseError> {
    Err(ParseError(s.to_string()))
}

fn name_bytes(codes: &[u32], enc: RegEnc) -> Vec<u8> {
    let mut out = Vec::new();
    for &c in codes {
        match enc {
            RegEnc::Utf8 => crate::mbfl::basic::push_utf8(&mut out, c),
            _ => {
                if c > 0xFFFFFF {
                    out.push((c >> 24) as u8);
                }
                if c > 0xFFFF {
                    out.push((c >> 16) as u8);
                }
                if c > 0xFF {
                    out.push((c >> 8) as u8);
                }
                out.push(c as u8);
            }
        }
    }
    out
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u32> {
        self.p.get(self.pos).copied()
    }
    fn peek_at(&self, n: usize) -> Option<u32> {
        self.p.get(self.pos + n).copied()
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c as u32) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn is_ruby_like(&self) -> bool {
        matches!(self.syntax, Syntax::Ruby | Syntax::Perl | Syntax::Java)
    }

    fn skip_extended(&mut self, opts: u32) {
        if opts & OPT_EXTEND == 0 {
            return;
        }
        loop {
            match self.peek() {
                Some(c) if c == b' ' as u32 || c == b'\t' as u32 || c == b'\n' as u32 || c == b'\r' as u32 || c == 0x0C || c == 0x0B => {
                    self.pos += 1;
                }
                Some(c) if c == b'#' as u32 => {
                    while let Some(c) = self.peek() {
                        self.pos += 1;
                        if c == b'\n' as u32 {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
    }

    fn parse_alt(&mut self, opts: &mut u32, in_group: bool) -> Result<Node, ParseError> {
        self.depth += 1;
        if self.depth > 4096 {
            return err("parse depth limit over");
        }
        let mut alts = Vec::new();
        let mut cur_opts = *opts;
        loop {
            let seq = self.parse_seq(&mut cur_opts, in_group)?;
            alts.push(seq);
            if self.eat('|') {
                continue;
            }
            break;
        }
        self.depth -= 1;
        // (?i) style option changes within a group persist to the end of the group only
        if !in_group {
            *opts = cur_opts;
        }
        if alts.len() == 1 {
            Ok(alts.pop().unwrap())
        } else {
            Ok(Node::Alt(alts))
        }
    }

    fn parse_seq(&mut self, opts: &mut u32, in_group: bool) -> Result<Node, ParseError> {
        let mut items: Vec<Node> = Vec::new();
        loop {
            self.skip_extended(*opts);
            let c = match self.peek() {
                None => break,
                Some(c) => c,
            };
            if c == b'|' as u32 {
                break;
            }
            if c == b')' as u32 {
                if !in_group {
                    return err("unmatched close parenthesis");
                }
                break;
            }
            // Option-only group (?imx-imx) affects the rest of the enclosing group
            if c == b'(' as u32 && self.peek_at(1) == Some(b'?' as u32) {
                if let Some((new_opts, consumed)) = self.try_option_only_group(*opts)? {
                    self.pos += consumed;
                    // The rest of the sequence (and following alternatives) uses the new options
                    let rest = self.parse_alt(&mut new_opts.clone(), in_group)?;
                    items.push(rest);
                    break;
                }
            }
            let atom = self.parse_atom(*opts)?;
            let atom = match atom {
                Some(a) => a,
                None => continue,
            };
            let atom = self.parse_quantifiers(atom, *opts)?;
            items.push(atom);
        }
        Ok(match items.len() {
            0 => Node::Empty,
            1 => items.pop().unwrap(),
            _ => Node::Seq(items),
        })
    }

    /// Parses "(?imx-imx)" at the current position (not followed by ':').
    fn try_option_only_group(&self, opts: u32) -> Result<Option<(u32, usize)>, ParseError> {
        let mut i = self.pos + 2;
        let mut o = opts;
        let mut neg = false;
        let mut any = false;
        while let Some(&c) = self.p.get(i) {
            let ch = char::from_u32(c).unwrap_or('\0');
            match ch {
                '-' => neg = true,
                'i' | 'm' | 'x' | 's' => {
                    let bit = match ch {
                        'i' => OPT_IGNORECASE,
                        'x' => OPT_EXTEND,
                        'm' => {
                            if self.syntax == Syntax::Perl || self.syntax == Syntax::Java {
                                // Perl 'm': negate singleline
                                if neg {
                                    o |= OPT_SINGLELINE;
                                } else {
                                    o &= !OPT_SINGLELINE;
                                }
                                any = true;
                                i += 1;
                                continue;
                            }
                            OPT_MULTILINE
                        }
                        _ => {
                            if self.syntax == Syntax::Ruby {
                                return err("undefined group option");
                            }
                            OPT_MULTILINE
                        }
                    };
                    if neg {
                        o &= !bit;
                    } else {
                        o |= bit;
                    }
                    any = true;
                }
                ')' => {
                    if !any && !neg {
                        return Ok(None);
                    }
                    return Ok(Some((o, i - self.pos + 1)));
                }
                _ => return Ok(None),
            }
            i += 1;
        }
        Ok(None)
    }

    fn parse_number(&mut self) -> Option<u64> {
        let start = self.pos;
        let mut v: u64 = 0;
        while let Some(c) = self.peek() {
            if (b'0' as u32..=b'9' as u32).contains(&c) {
                v = v.saturating_mul(10).saturating_add((c - b'0' as u32) as u64);
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start { None } else { Some(v) }
    }

    /// Try to parse "{n,m}" at current position. Returns None (and does not
    /// consume) if it is not a valid interval.
    fn parse_interval(&mut self) -> Result<Option<(u32, Option<u32>)>, ParseError> {
        let save = self.pos;
        if !self.eat('{') {
            return Ok(None);
        }
        let low = self.parse_number();
        let mut high: Option<u64>;
        let mut has_comma = false;
        if self.eat(',') {
            has_comma = true;
            high = self.parse_number();
        } else {
            high = low;
        }
        if !self.eat('}') || (low.is_none() && (!has_comma || high.is_none())) {
            self.pos = save;
            return Ok(None);
        }
        let low = low.unwrap_or(0);
        if !has_comma {
            high = Some(low);
        }
        if low > 100000 || high.map(|h| h > 100000).unwrap_or(false) {
            return err("too big number for repeat range");
        }
        if let Some(h) = high {
            if h < low {
                return err("upper is smaller than lower in repeat range");
            }
        }
        Ok(Some((low as u32, high.map(|h| h as u32))))
    }

    fn parse_quantifiers(&mut self, mut atom: Node, opts: u32) -> Result<Node, ParseError> {
        loop {
            self.skip_extended(opts);
            let c = match self.peek() {
                Some(c) => c,
                None => return Ok(atom),
            };
            let (min, max, fixed) = if c == b'*' as u32 {
                self.pos += 1;
                (0, None, false)
            } else if c == b'+' as u32 {
                self.pos += 1;
                (1, None, false)
            } else if c == b'?' as u32 {
                self.pos += 1;
                (0, Some(1), false)
            } else if c == b'{' as u32 {
                match self.parse_interval()? {
                    Some((lo, hi)) => (lo, hi, hi == Some(lo)),
                    None => return Ok(atom),
                }
            } else {
                return Ok(atom);
            };
            if matches!(atom, Node::Anchor(_) | Node::Keep) && self.syntax == Syntax::Ruby {
                return err("target of repeat operator is invalid");
            }
            let mut greedy = true;
            let mut possessive = false;
            if !fixed || self.syntax != Syntax::Ruby {
                if self.peek() == Some(b'?' as u32) {
                    self.pos += 1;
                    greedy = false;
                } else if self.peek() == Some(b'+' as u32) && c != b'{' as u32 && self.is_ruby_like() {
                    self.pos += 1;
                    possessive = true;
                }
            }
            atom = Node::Repeat { node: Box::new(atom), min, max, greedy, possessive };
        }
    }

    fn parse_group_name(&mut self, close: u32) -> Result<Vec<u32>, ParseError> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == close {
                let name = self.p[start..self.pos].to_vec();
                self.pos += 1;
                if name.is_empty() {
                    return err("group name is empty");
                }
                return Ok(name);
            }
            if c == b')' as u32 {
                break;
            }
            self.pos += 1;
        }
        err("invalid group name <>")
    }

    fn validate_name(&self, name: &[u32]) -> Result<(), ParseError> {
        let first = name[0];
        let ok_first = !(b'0' as u32..=b'9' as u32).contains(&first) && first != b'-' as u32 && first != b'+' as u32;
        let ok_rest = name.iter().all(|&c| c >= 0x80 || (c as u8).is_ascii_alphanumeric() || c == b'_' as u32);
        if !ok_first || !ok_rest {
            let nb = String::from_utf8_lossy(&name_bytes(name, self.enc)).to_string();
            if !ok_first && name.iter().all(|&c| (b'0' as u32..=b'9' as u32).contains(&c) || c == b'-' as u32 || c == b'+' as u32) {
                return Err(ParseError(format!("invalid group name <{}>", nb)));
            }
            return Err(ParseError(format!("invalid char in group name <{}>", nb)));
        }
        Ok(())
    }

    fn parse_atom(&mut self, opts: u32) -> Result<Option<Node>, ParseError> {
        let c = self.peek().unwrap();
        self.pos += 1;
        let ci = opts & OPT_IGNORECASE != 0;
        let ch = char::from_u32(c).unwrap_or('\u{FFFD}');
        match ch {
            '(' => self.parse_group(opts).map(Some),
            '[' => {
                let cc = self.parse_class(opts)?;
                Ok(Some(Node::Class(Box::new(cc), ci)))
            }
            '.' => Ok(Some(Node::Any(opts & OPT_MULTILINE != 0))),
            '^' => Ok(Some(Node::Anchor(if opts & OPT_SINGLELINE != 0 { Anchor::BeginBuf } else { Anchor::BeginLine }))),
            '$' => Ok(Some(Node::Anchor(if opts & OPT_SINGLELINE != 0 { Anchor::SemiEndBuf } else { Anchor::EndLine }))),
            '*' | '+' | '?' => err("target of repeat operator is not specified"),
            '{' => {
                self.pos -= 1;
                if self.parse_interval()?.is_some() {
                    return err("target of repeat operator is not specified");
                }
                self.pos += 1;
                Ok(Some(Node::Char(c, ci)))
            }
            '\\' => self.parse_escape(opts),
            _ => Ok(Some(Node::Char(c, ci))),
        }
    }

    fn parse_group(&mut self, opts: u32) -> Result<Node, ParseError> {
        let mut inner_opts = opts;
        if self.eat('?') {
            let c = match self.peek() {
                Some(c) => c,
                None => return err("end pattern in group"),
            };
            let ch = char::from_u32(c).unwrap_or('\0');
            self.pos += 1;
            match ch {
                ':' => {}
                '=' | '!' => {
                    let body = self.parse_alt(&mut inner_opts, true)?;
                    self.expect_close()?;
                    return Ok(Node::Look { node: Box::new(body), ahead: true, negate: ch == '!' });
                }
                '>' => {
                    let body = self.parse_alt(&mut inner_opts, true)?;
                    self.expect_close()?;
                    return Ok(Node::Atomic(Box::new(body)));
                }
                '#' => {
                    while let Some(c) = self.peek() {
                        self.pos += 1;
                        if c == b')' as u32 {
                            return Ok(Node::Empty);
                        }
                    }
                    return err("end pattern in group");
                }
                '<' if self.peek() == Some(b'=' as u32) || self.peek() == Some(b'!' as u32) => {
                    let negate = self.peek() == Some(b'!' as u32);
                    self.pos += 1;
                    let body = self.parse_alt(&mut inner_opts, true)?;
                    self.expect_close()?;
                    if char_length(&body).is_none() && alt_lengths(&body).is_none() {
                        return err("invalid pattern in look-behind");
                    }
                    return Ok(Node::Look { node: Box::new(body), ahead: false, negate });
                }
                '<' | '\'' => {
                    let close = if ch == '<' { b'>' as u32 } else { b'\'' as u32 };
                    let name = self.parse_group_name(close)?;
                    self.validate_name(&name)?;
                    self.ngroups += 1;
                    let num = self.ngroups;
                    let nb = name_bytes(&name, self.enc);
                    self.names.push((nb, num));
                    let body = self.parse_alt(&mut inner_opts, true)?;
                    self.expect_close()?;
                    return Ok(Node::Group(Box::new(body), num));
                }
                _ => {
                    // Option group (?imx-imx:...)
                    self.pos -= 1;
                    let mut neg = false;
                    let mut o = opts;
                    loop {
                        let c = match self.peek() {
                            Some(c) => c,
                            None => return err("end pattern in group"),
                        };
                        self.pos += 1;
                        match char::from_u32(c).unwrap_or('\0') {
                            '-' => neg = true,
                            'i' => o = if neg { o & !OPT_IGNORECASE } else { o | OPT_IGNORECASE },
                            'x' => o = if neg { o & !OPT_EXTEND } else { o | OPT_EXTEND },
                            'm' if self.syntax == Syntax::Ruby => o = if neg { o & !OPT_MULTILINE } else { o | OPT_MULTILINE },
                            'm' => o = if neg { o | OPT_SINGLELINE } else { o & !OPT_SINGLELINE },
                            's' if self.syntax != Syntax::Ruby => o = if neg { o & !OPT_MULTILINE } else { o | OPT_MULTILINE },
                            ':' => break,
                            ')' => {
                                // handled by try_option_only_group normally
                                return Ok(Node::Empty);
                            }
                            _ => return err("undefined group option"),
                        }
                    }
                    inner_opts = o;
                    let body = self.parse_alt(&mut inner_opts, true)?;
                    self.expect_close()?;
                    return Ok(Node::Group(Box::new(body), 0));
                }
            }
            let body = self.parse_alt(&mut inner_opts, true)?;
            self.expect_close()?;
            return Ok(Node::Group(Box::new(body), 0));
        }
        // Plain capturing group (may become non-capturing if named groups exist)
        self.ngroups += 1;
        let num = self.ngroups;
        let body = self.parse_alt(&mut inner_opts, true)?;
        self.expect_close()?;
        Ok(Node::Group(Box::new(body), num | PLAIN_FLAG))
    }

    fn expect_close(&mut self) -> Result<(), ParseError> {
        if self.eat(')') {
            Ok(())
        } else {
            err("end pattern with unmatched parenthesis")
        }
    }

    fn parse_hex(&mut self, max: usize) -> Option<u32> {
        let mut v: u32 = 0;
        let mut n = 0;
        while n < max {
            match self.peek().and_then(|c| char::from_u32(c)).and_then(|c| c.to_digit(16)) {
                Some(d) => {
                    v = v.wrapping_mul(16) + d;
                    self.pos += 1;
                    n += 1;
                }
                None => break,
            }
        }
        if n == 0 { None } else { Some(v) }
    }

    fn parse_octal(&mut self, max: usize) -> u32 {
        let mut v: u32 = 0;
        let mut n = 0;
        while n < max {
            match self.peek() {
                Some(c) if (b'0' as u32..=b'7' as u32).contains(&c) => {
                    v = v * 8 + (c - b'0' as u32);
                    self.pos += 1;
                    n += 1;
                }
                _ => break,
            }
        }
        v
    }

    /// Escapes valid both inside and outside character classes that
    /// produce a single code. Returns None if not such an escape.
    fn parse_char_escape(&mut self, c: u32) -> Result<Option<u32>, ParseError> {
        let ch = char::from_u32(c).unwrap_or('\0');
        Ok(Some(match ch {
            't' => 0x09,
            'n' => 0x0A,
            'r' => 0x0D,
            'f' => 0x0C,
            'v' => 0x0B,
            'a' => 0x07,
            'e' => 0x1B,
            'x' => {
                if self.eat('{') {
                    let v = self.parse_hex(8);
                    if !self.eat('}') || v.is_none() {
                        return err("invalid code point value");
                    }
                    let v = v.unwrap();
                    if self.enc.is_unicode() && v > 0x10FFFF {
                        return err("invalid code point value");
                    }
                    v
                } else {
                    match self.parse_hex(2) {
                        Some(v) => v,
                        None => 0,
                    }
                }
            }
            'u' if self.syntax == Syntax::Ruby || self.syntax == Syntax::Java => {
                let save = self.pos;
                let mut v = 0u32;
                for _ in 0..4 {
                    match self.peek().and_then(|c| char::from_u32(c)).and_then(|c| c.to_digit(16)) {
                        Some(d) => {
                            v = v * 16 + d;
                            self.pos += 1;
                        }
                        None => {
                            self.pos = save;
                            return Ok(Some(b'u' as u32));
                        }
                    }
                }
                v
            }
            'o' if self.peek() == Some(b'{' as u32) => {
                self.pos += 1;
                let v = self.parse_octal(11);
                if !self.eat('}') {
                    return err("invalid code point value");
                }
                v
            }
            '0' => self.parse_octal(2),
            'c' => {
                let x = match self.peek() {
                    Some(x) => x,
                    None => return err("end pattern at control"),
                };
                self.pos += 1;
                let x = if x == b'\\' as u32 {
                    match self.peek() {
                        Some(y) => {
                            self.pos += 1;
                            self.parse_char_escape(y)?.unwrap_or(y)
                        }
                        None => return err("end pattern at escape"),
                    }
                } else {
                    x
                };
                if x == b'?' as u32 { 0x7F } else { x & 0x9F }
            }
            'C' if self.peek() == Some(b'-' as u32) => {
                self.pos += 1;
                let x = match self.peek() {
                    Some(x) => x,
                    None => return err("end pattern at control"),
                };
                self.pos += 1;
                if x == b'?' as u32 { 0x7F } else { x & 0x9F }
            }
            'M' if self.peek() == Some(b'-' as u32) => {
                self.pos += 1;
                let x = match self.peek() {
                    Some(x) => x,
                    None => return err("end pattern at meta"),
                };
                self.pos += 1;
                (x & 0xFF) | 0x80
            }
            _ => return Ok(None),
        }))
    }

    fn parse_property(&mut self, negate: bool) -> Result<(Ctype, bool), ParseError> {
        // after \p or \P
        if !self.eat('{') {
            return err("invalid character property name {}");
        }
        let mut neg = negate;
        if self.eat('^') {
            neg = !neg;
        }
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'}' as u32 {
                break;
            }
            self.pos += 1;
        }
        let name: String = self.p[start..self.pos].iter().filter_map(|&c| char::from_u32(c)).collect();
        if !self.eat('}') {
            return Err(ParseError(format!("invalid character property name {{{}}}", name)));
        }
        match property(&name) {
            Some(ct) => Ok((ct, neg)),
            None => Err(ParseError(format!("invalid character property name {{{}}}", name))),
        }
    }

    fn ctype_escape(&mut self, ch: char) -> Result<Option<(Ctype, bool)>, ParseError> {
        Ok(Some(match ch {
            'w' => (Ctype::Word, false),
            'W' => (Ctype::Word, true),
            'd' => (Ctype::Digit, false),
            'D' => (Ctype::Digit, true),
            's' => (Ctype::Space, false),
            'S' => (Ctype::Space, true),
            'h' if self.syntax == Syntax::Ruby => (Ctype::Hex, false),
            'H' if self.syntax == Syntax::Ruby => (Ctype::Hex, true),
            'p' => return self.parse_property(false).map(Some),
            'P' => return self.parse_property(true).map(Some),
            _ => return Ok(None),
        }))
    }

    fn parse_escape(&mut self, opts: u32) -> Result<Option<Node>, ParseError> {
        let ci = opts & OPT_IGNORECASE != 0;
        let c = match self.peek() {
            Some(c) => c,
            None => return err("end pattern at escape"),
        };
        self.pos += 1;
        let ch = char::from_u32(c).unwrap_or('\0');
        if let Some((ct, neg)) = self.ctype_escape(ch)? {
            let cc = CharClass { ctypes: vec![(ct, neg)], ..Default::default() };
            return Ok(Some(Node::Class(Box::new(cc), false)));
        }
        match ch {
            'A' => return Ok(Some(Node::Anchor(Anchor::BeginBuf))),
            'z' => return Ok(Some(Node::Anchor(Anchor::EndBuf))),
            'Z' => return Ok(Some(Node::Anchor(Anchor::SemiEndBuf))),
            'b' => return Ok(Some(Node::Anchor(Anchor::WordBound))),
            'B' => return Ok(Some(Node::Anchor(Anchor::NotWordBound))),
            'G' => return Ok(Some(Node::Anchor(Anchor::BeginPosition))),
            'K' => return Ok(Some(Node::Keep)),
            'R' => {
                // (?>\r\n|[\n\v\f\r\x85  ])
                let cc = CharClass { ranges: vec![(0x0A, 0x0D), (0x85, 0x85), (0x2028, 0x2029)], ..Default::default() };
                return Ok(Some(Node::Atomic(Box::new(Node::Alt(vec![
                    Node::Seq(vec![Node::Char(0x0D, false), Node::Char(0x0A, false)]),
                    Node::Class(Box::new(cc), false),
                ])))));
            }
            'N' => return Ok(Some(Node::Any(false))),
            'O' => return Ok(Some(Node::Any(true))),
            'X' => {
                let nonmark = CharClass { ctypes: vec![(Ctype::Gc(&[unicode::UC_MN, unicode::UC_MC, unicode::UC_ME]), true)], ..Default::default() };
                let mark = CharClass { ctypes: vec![(Ctype::Gc(&[unicode::UC_MN, unicode::UC_MC, unicode::UC_ME]), false)], ..Default::default() };
                return Ok(Some(Node::Atomic(Box::new(Node::Alt(vec![
                    Node::Seq(vec![Node::Char(0x0D, false), Node::Char(0x0A, false)]),
                    Node::Seq(vec![
                        Node::Class(Box::new(nonmark), false),
                        Node::Repeat { node: Box::new(Node::Class(Box::new(mark), false)), min: 0, max: None, greedy: true, possessive: false },
                    ]),
                    Node::Any(true),
                ])))));
            }
            'k' if self.peek() == Some(b'<' as u32) || self.peek() == Some(b'\'' as u32) => {
                let close = if self.peek() == Some(b'<' as u32) { b'>' as u32 } else { b'\'' as u32 };
                self.pos += 1;
                let name = self.parse_group_name(close)?;
                let all_digits = name.iter().all(|&c| (b'0' as u32..=b'9' as u32).contains(&c));
                if all_digits {
                    let n: usize = name.iter().fold(0usize, |a, &c| a.saturating_mul(10).saturating_add((c - b'0' as u32) as usize));
                    self.has_numbered_backref = true;
                    return Ok(Some(Node::BackrefNum(n, ci)));
                }
                if name[0] == b'-' as u32 || name[0] == b'+' as u32 {
                    let n: usize = name[1..].iter().fold(0usize, |a, &c| a * 10 + (c.wrapping_sub(b'0' as u32)) as usize);
                    let target = if name[0] == b'-' as u32 { self.ngroups + 1 - n.min(self.ngroups + 1) } else { self.ngroups + n };
                    self.has_numbered_backref = true;
                    return Ok(Some(Node::BackrefNum(target, ci)));
                }
                return Ok(Some(Node::BackrefName(name_bytes(&name, self.enc), ci)));
            }
            'g' if self.peek() == Some(b'<' as u32) || self.peek() == Some(b'\'' as u32) => {
                return err("never ending recursion");
            }
            '1'..='9' => {
                self.pos -= 1;
                let save = self.pos;
                let n = self.parse_number().unwrap_or(0) as usize;
                if n <= 9 || n <= self.ngroups {
                    if n > 1000 {
                        return err("invalid backref number/name");
                    }
                    self.has_numbered_backref = true;
                    return Ok(Some(Node::BackrefNum(n, ci)));
                }
                // Octal escape
                self.pos = save;
                if (b'0' as u32..=b'7' as u32).contains(&c) {
                    let v = self.parse_octal(3);
                    return Ok(Some(Node::Char(v, ci)));
                }
                self.pos += 1;
                return Ok(Some(Node::Char(c, ci)));
            }
            _ => {}
        }
        if let Some(v) = self.parse_char_escape(c)? {
            return Ok(Some(Node::Char(v, ci)));
        }
        Ok(Some(Node::Char(c, ci)))
    }

    fn parse_class(&mut self, opts: u32) -> Result<CharClass, ParseError> {
        let mut cc = CharClass::default();
        if self.eat('^') {
            cc.negated = true;
        }
        let mut first = true;
        let mut prev: Option<u32> = None; // last single code (for ranges)
        loop {
            let c = match self.peek() {
                Some(c) => c,
                None => return err("premature end of char-class"),
            };
            if c == b']' as u32 {
                if first {
                    // ']' at the start is literal if another ']' follows
                    if !self.p[self.pos + 1..].contains(&(b']' as u32)) {
                        return err("empty char-class");
                    }
                    self.pos += 1;
                    cc.ranges.push((c, c));
                    prev = Some(c);
                    first = false;
                    continue;
                }
                self.pos += 1;
                break;
            }
            first = false;
            if c == b'&' as u32 && self.peek_at(1) == Some(b'&' as u32) {
                self.pos += 2;
                // Everything parsed so far is the left operand; the rest is the right operand
                let left = std::mem::take(&mut cc);
                let negated = left.negated;
                let mut left = left;
                left.negated = false;
                let mut right = self.parse_class_rest(opts)?;
                right.negated = false;
                let mut res = CharClass { negated, ..Default::default() };
                let left_empty = left.ranges.is_empty() && left.ctypes.is_empty() && left.subs.is_empty() && left.and.is_empty();
                if !left_empty {
                    res.and.push(left);
                }
                res.and.push(right);
                return Ok(res);
            }
            if c == b'[' as u32 {
                // POSIX bracket?
                if self.peek_at(1) == Some(b':' as u32) {
                    if let Some((ct, neg, len)) = self.try_posix_bracket()? {
                        self.pos += len;
                        cc.ctypes.push((ct, neg));
                        prev = None;
                        continue;
                    }
                }
                self.pos += 1;
                let sub = self.parse_class(opts)?;
                cc.subs.push(sub);
                prev = None;
                continue;
            }
            if c == b'-' as u32 && prev.is_some() && self.peek_at(1).map(|n| n != b']' as u32).unwrap_or(false) {
                // Range
                self.pos += 1;
                let lo = prev.unwrap();
                let hi_c = self.peek().unwrap();
                let hi = if hi_c == b'\\' as u32 {
                    self.pos += 1;
                    let e = match self.peek() {
                        Some(e) => e,
                        None => return err("end pattern at escape"),
                    };
                    self.pos += 1;
                    let ech = char::from_u32(e).unwrap_or('\0');
                    if self.ctype_escape(ech)?.is_some() {
                        return err("char-class value at end of range");
                    }
                    match self.parse_char_escape(e)? {
                        Some(v) => v,
                        None => {
                            if ech == 'b' {
                                0x08
                            } else {
                                e
                            }
                        }
                    }
                } else if hi_c == b'[' as u32 {
                    return err("char-class value at end of range");
                } else {
                    self.pos += 1;
                    hi_c
                };
                if hi < lo {
                    return err("empty range in char class");
                }
                // replace the single lo entry by the range
                if let Some(last) = cc.ranges.last_mut() {
                    if *last == (lo, lo) {
                        *last = (lo, hi);
                    } else {
                        cc.ranges.push((lo, hi));
                    }
                } else {
                    cc.ranges.push((lo, hi));
                }
                prev = None;
                continue;
            }
            self.pos += 1;
            let v = if c == b'\\' as u32 {
                let e = match self.peek() {
                    Some(e) => e,
                    None => return err("end pattern at escape"),
                };
                self.pos += 1;
                let ech = char::from_u32(e).unwrap_or('\0');
                if let Some((ct, neg)) = self.ctype_escape(ech)? {
                    cc.ctypes.push((ct, neg));
                    prev = None;
                    continue;
                }
                match self.parse_char_escape(e)? {
                    Some(v) => v,
                    None => {
                        if ech == 'b' {
                            0x08
                        } else if (b'1' as u32..=b'7' as u32).contains(&e) {
                            self.pos -= 1;
                            self.parse_octal(3)
                        } else {
                            e
                        }
                    }
                }
            } else {
                c
            };
            cc.ranges.push((v, v));
            prev = Some(v);
        }
        Ok(cc)
    }

    /// The right-hand side of '&&' extends to the closing bracket.
    fn parse_class_rest(&mut self, opts: u32) -> Result<CharClass, ParseError> {
        // Reuse parse_class logic on a synthetic class start (no negation)
        let c = self.parse_class_inner(opts)?;
        Ok(c)
    }

    fn parse_class_inner(&mut self, opts: u32) -> Result<CharClass, ParseError> {
        // Same as parse_class but without '^' handling and allowing an
        // immediate ']' (empty right operand)
        if self.peek() == Some(b']' as u32) {
            self.pos += 1;
            return Ok(CharClass { negated: true, ..Default::default() });
        }
        if self.peek() == Some(b'^' as u32) {
            // a literal '^' here would be odd; treat as negation like Oniguruma does not; keep literal
        }
        let saved_first = self.peek();
        let _ = saved_first;
        // Temporarily parse as a class whose first ']' closes it
        let mut cc = CharClass::default();
        let mut prev: Option<u32> = None;
        loop {
            let c = match self.peek() {
                Some(c) => c,
                None => return err("premature end of char-class"),
            };
            if c == b']' as u32 {
                self.pos += 1;
                break;
            }
            if c == b'&' as u32 && self.peek_at(1) == Some(b'&' as u32) {
                self.pos += 2;
                let right = self.parse_class_inner(opts)?;
                let mut res = CharClass::default();
                res.and.push(cc);
                res.and.push(right);
                return Ok(res);
            }
            if c == b'[' as u32 {
                if self.peek_at(1) == Some(b':' as u32) {
                    if let Some((ct, neg, len)) = self.try_posix_bracket()? {
                        self.pos += len;
                        cc.ctypes.push((ct, neg));
                        prev = None;
                        continue;
                    }
                }
                self.pos += 1;
                let sub = self.parse_class(opts)?;
                cc.subs.push(sub);
                prev = None;
                continue;
            }
            if c == b'-' as u32 && prev.is_some() && self.peek_at(1).map(|n| n != b']' as u32).unwrap_or(false) {
                self.pos += 1;
                let lo = prev.unwrap();
                let hi_c = self.peek().unwrap();
                self.pos += 1;
                let hi = if hi_c == b'\\' as u32 {
                    let e = self.peek().unwrap_or(0);
                    self.pos += 1;
                    self.parse_char_escape(e)?.unwrap_or(e)
                } else {
                    hi_c
                };
                if hi < lo {
                    return err("empty range in char class");
                }
                if let Some(last) = cc.ranges.last_mut() {
                    if *last == (lo, lo) {
                        *last = (lo, hi);
                    }
                }
                prev = None;
                continue;
            }
            self.pos += 1;
            let v = if c == b'\\' as u32 {
                let e = match self.peek() {
                    Some(e) => e,
                    None => return err("end pattern at escape"),
                };
                self.pos += 1;
                let ech = char::from_u32(e).unwrap_or('\0');
                if let Some((ct, neg)) = self.ctype_escape(ech)? {
                    cc.ctypes.push((ct, neg));
                    prev = None;
                    continue;
                }
                match self.parse_char_escape(e)? {
                    Some(v) => v,
                    None => {
                        if ech == 'b' {
                            0x08
                        } else {
                            e
                        }
                    }
                }
            } else {
                c
            };
            cc.ranges.push((v, v));
            prev = Some(v);
        }
        Ok(cc)
    }

    fn try_posix_bracket(&self) -> Result<Option<(Ctype, bool, usize)>, ParseError> {
        // at "[:"
        let mut i = self.pos + 2;
        let mut neg = false;
        if self.p.get(i) == Some(&(b'^' as u32)) {
            neg = true;
            i += 1;
        }
        let start = i;
        while let Some(&c) = self.p.get(i) {
            if c == b':' as u32 {
                break;
            }
            if !(c < 0x80 && (c as u8).is_ascii_alphabetic()) {
                return Ok(None);
            }
            i += 1;
        }
        if self.p.get(i) != Some(&(b':' as u32)) || self.p.get(i + 1) != Some(&(b']' as u32)) {
            return Ok(None);
        }
        let name: String = self.p[start..i].iter().map(|&c| c as u8 as char).collect();
        match posix_bracket(&name) {
            Some(ct) => Ok(Some((ct, neg, i + 2 - self.pos))),
            None => err("invalid POSIX bracket type"),
        }
    }
}

const PLAIN_FLAG: usize = 1 << 40;

/// Fixed length (in characters) of a node, if it has one
fn char_length(n: &Node) -> Option<usize> {
    match n {
        Node::Empty | Node::Anchor(_) | Node::Keep | Node::Look { .. } => Some(0),
        Node::Char(..) | Node::Any(_) | Node::Class(..) => Some(1),
        Node::Seq(v) => {
            let mut t = 0;
            for x in v {
                t += char_length(x)?;
            }
            Some(t)
        }
        Node::Alt(v) => {
            let first = char_length(&v[0])?;
            for x in &v[1..] {
                if char_length(x)? != first {
                    return None;
                }
            }
            Some(first)
        }
        Node::Group(b, _) | Node::Atomic(b) => char_length(b),
        Node::Repeat { node, min, max, .. } => {
            if Some(*min) == *max {
                Some(char_length(node)? * *min as usize)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// For a top-level alternation in a look-behind, each branch may have a
/// different fixed length (ONIG_SYN_DIFFERENT_LEN_ALT_LOOK_BEHIND).
fn alt_lengths(n: &Node) -> Option<Vec<usize>> {
    match n {
        Node::Alt(v) => v.iter().map(char_length).collect(),
        Node::Group(b, _) => alt_lengths(b),
        _ => char_length(n).map(|l| vec![l]),
    }
}

fn nullable(n: &Node) -> bool {
    match n {
        Node::Empty | Node::Anchor(_) | Node::Keep | Node::Look { .. } | Node::Backref(..) | Node::BackrefNum(..) | Node::BackrefName(..) => true,
        Node::Char(..) | Node::Any(_) | Node::Class(..) => false,
        Node::Seq(v) => v.iter().all(nullable),
        Node::Alt(v) => v.iter().any(nullable),
        Node::Group(b, _) | Node::Atomic(b) => nullable(b),
        Node::Repeat { node, min, .. } => *min == 0 || nullable(node),
    }
}

/* ===== Program ===== */

#[derive(Clone, Debug)]
enum Inst {
    Char(u32),
    CharCi(u32),
    Any,
    AnyNl,
    Class(usize, bool),
    Split(usize, usize),
    Jmp(usize),
    Save(usize),
    Assert(Anchor),
    Backref(Vec<usize>, bool),
    LoopSave(usize),
    LoopCheck(usize, usize), // (reg, exit target if no progress)
    /// Run sub-program [start..] atomically; continue with its end position
    Atomic(usize),
    /// Lookaround: (start of sub-program, ahead, negate, lengths for look-behind alts)
    Look(usize, bool, bool, Vec<(usize, usize)>),
    Keep,
    Match,
}

pub struct Regex {
    prog: Vec<Inst>,
    classes: Vec<CharClass>,
    pub num_groups: usize,
    /// names in definition order with their group numbers
    pub names: Vec<(Vec<u8>, Vec<usize>)>,
    pub noname_capture_active: bool,
    nloops: usize,
    enc: RegEnc,
    options: u32,
    anchored_start: bool,
}

struct Compiler {
    prog: Vec<Inst>,
    classes: Vec<CharClass>,
    nloops: usize,
    enc: RegEnc,
}

impl Compiler {
    fn emit(&mut self, i: Inst) -> usize {
        self.prog.push(i);
        self.prog.len() - 1
    }

    fn compile(&mut self, n: &Node) {
        match n {
            Node::Empty => {}
            Node::Char(c, ci) => {
                if *ci {
                    let f = fold(*c, self.enc);
                    self.emit(Inst::CharCi(f));
                } else {
                    self.emit(Inst::Char(*c));
                }
            }
            Node::Any(dotall) => {
                self.emit(if *dotall { Inst::AnyNl } else { Inst::Any });
            }
            Node::Class(cc, ci) => {
                self.classes.push((**cc).clone());
                let idx = self.classes.len() - 1;
                self.emit(Inst::Class(idx, *ci));
            }
            Node::Seq(v) => {
                for x in v {
                    self.compile(x);
                }
            }
            Node::Alt(v) => {
                let mut jumps = Vec::new();
                for (i, x) in v.iter().enumerate() {
                    if i + 1 < v.len() {
                        let split = self.emit(Inst::Split(0, 0));
                        self.compile(x);
                        jumps.push(self.emit(Inst::Jmp(0)));
                        let next = self.prog.len();
                        self.prog[split] = Inst::Split(split + 1, next);
                    } else {
                        self.compile(x);
                    }
                }
                let end = self.prog.len();
                for j in jumps {
                    self.prog[j] = Inst::Jmp(end);
                }
            }
            Node::Group(b, num) => {
                if *num > 0 {
                    self.emit(Inst::Save(num * 2));
                    self.compile(b);
                    self.emit(Inst::Save(num * 2 + 1));
                } else {
                    self.compile(b);
                }
            }
            Node::Repeat { node, min, max, greedy, possessive } => {
                if *possessive {
                    let inner = Node::Repeat { node: node.clone(), min: *min, max: *max, greedy: true, possessive: false };
                    self.compile(&Node::Atomic(Box::new(inner)));
                    return;
                }
                for _ in 0..*min {
                    self.compile(node);
                }
                match max {
                    None => {
                        let can_empty = nullable(node);
                        let l = self.emit(Inst::Split(0, 0));
                        let reg = if can_empty {
                            let r = self.nloops;
                            self.nloops += 1;
                            self.emit(Inst::LoopSave(r));
                            Some(r)
                        } else {
                            None
                        };
                        self.compile(node);
                        let check = reg.map(|r| self.emit(Inst::LoopCheck(r, 0)));
                        self.emit(Inst::Jmp(l));
                        let exit = self.prog.len();
                        self.prog[l] = if *greedy { Inst::Split(l + 1, exit) } else { Inst::Split(exit, l + 1) };
                        if let (Some(ci), Some(r)) = (check, reg) {
                            self.prog[ci] = Inst::LoopCheck(r, exit);
                        }
                    }
                    Some(mx) => {
                        let extra = mx - min;
                        let mut splits = Vec::new();
                        for _ in 0..extra {
                            splits.push(self.emit(Inst::Split(0, 0)));
                            self.compile(node);
                        }
                        let end = self.prog.len();
                        for s in splits {
                            self.prog[s] = if *greedy { Inst::Split(s + 1, end) } else { Inst::Split(end, s + 1) };
                        }
                    }
                }
            }
            Node::Atomic(b) => {
                let at = self.emit(Inst::Atomic(0));
                let skip = self.emit(Inst::Jmp(0));
                let start = self.prog.len();
                self.compile(b);
                self.emit(Inst::Match);
                let after = self.prog.len();
                self.prog[at] = Inst::Atomic(start);
                self.prog[skip] = Inst::Jmp(after);
            }
            Node::Look { node, ahead, negate } => {
                if *ahead {
                    let at = self.emit(Inst::Look(0, true, *negate, vec![]));
                    let skip = self.emit(Inst::Jmp(0));
                    let start = self.prog.len();
                    self.compile(node);
                    self.emit(Inst::Match);
                    let after = self.prog.len();
                    self.prog[at] = Inst::Look(start, true, *negate, vec![]);
                    self.prog[skip] = Inst::Jmp(after);
                } else {
                    // One sub-program per alternative (each with its own length)
                    let alts: Vec<Node> = match strip_group(node) {
                        Node::Alt(v) if char_length(node).is_none() => v.clone(),
                        _ => vec![(**node).clone()],
                    };
                    let at = self.emit(Inst::Look(0, false, *negate, vec![]));
                    let skip = self.emit(Inst::Jmp(0));
                    let mut subs = Vec::new();
                    for a in &alts {
                        let len = char_length(a).unwrap_or(0);
                        let start = self.prog.len();
                        self.compile(a);
                        self.emit(Inst::Match);
                        subs.push((start, len));
                    }
                    let after = self.prog.len();
                    self.prog[at] = Inst::Look(0, false, *negate, subs);
                    self.prog[skip] = Inst::Jmp(after);
                }
            }
            Node::Backref(groups, ci) => {
                self.emit(Inst::Backref(groups.clone(), *ci));
            }
            Node::BackrefNum(..) | Node::BackrefName(..) => unreachable!(),
            Node::Anchor(a) => {
                self.emit(Inst::Assert(*a));
            }
            Node::Keep => {
                self.emit(Inst::Keep);
            }
        }
    }
}

fn strip_group(n: &Node) -> &Node {
    match n {
        Node::Group(b, 0) => strip_group(b),
        _ => n,
    }
}

/// Resolve capture numbering: when named groups exist, plain groups do
/// not capture and named groups are renumbered in order.
fn resolve(n: Node, map: &dyn Fn(usize) -> usize, names: &[(Vec<u8>, Vec<usize>)], has_names: bool) -> Result<Node, ParseError> {
    Ok(match n {
        Node::Seq(v) => Node::Seq(v.into_iter().map(|x| resolve(x, map, names, has_names)).collect::<Result<_, _>>()?),
        Node::Alt(v) => Node::Alt(v.into_iter().map(|x| resolve(x, map, names, has_names)).collect::<Result<_, _>>()?),
        Node::Group(b, num) => {
            let plain = num & PLAIN_FLAG != 0;
            let raw = num & !PLAIN_FLAG;
            let newnum = if num == 0 || (plain && has_names) { 0 } else { map(raw) };
            Node::Group(Box::new(resolve(*b, map, names, has_names)?), newnum)
        }
        Node::Repeat { node, min, max, greedy, possessive } => Node::Repeat { node: Box::new(resolve(*node, map, names, has_names)?), min, max, greedy, possessive },
        Node::Atomic(b) => Node::Atomic(Box::new(resolve(*b, map, names, has_names)?)),
        Node::Look { node, ahead, negate } => Node::Look { node: Box::new(resolve(*node, map, names, has_names)?), ahead, negate },
        Node::BackrefNum(n, ci) => {
            if has_names {
                return err("numbered backref/call is not allowed. (use name)");
            }
            Node::Backref(vec![n], ci)
        }
        Node::BackrefName(name, ci) => match names.iter().find(|(nm, _)| *nm == name) {
            Some((_, nums)) => Node::Backref(nums.clone(), ci),
            None => return Err(ParseError(format!("undefined name <{}> reference", String::from_utf8_lossy(&name)))),
        },
        other => other,
    })
}

pub fn compile(pattern: &[u8], options: u32, enc: RegEnc, syntax: Syntax) -> Result<Regex, String> {
    let (codes, _) = enc.decode_all(pattern);
    let mut parser = Parser { p: &codes, pos: 0, enc, syntax, ngroups: 0, names: Vec::new(), has_numbered_backref: false, depth: 0 };
    let mut opts = options;
    let ast = parser.parse_alt(&mut opts, false).map_err(|e| e.0)?;
    if parser.pos < codes.len() {
        return Err("unmatched close parenthesis".to_string());
    }
    let has_names = !parser.names.is_empty();
    // Group numbering
    let (num_groups, mapping): (usize, Vec<usize>) = if has_names {
        let mut m = vec![0usize; parser.ngroups + 1];
        for (i, (_, raw)) in parser.names.iter().enumerate() {
            m[*raw] = i + 1;
        }
        (parser.names.len(), m)
    } else {
        ((0..=parser.ngroups).count() - 1, (0..=parser.ngroups).collect())
    };
    // names -> group numbers (merged duplicates, in first-definition order)
    let mut names: Vec<(Vec<u8>, Vec<usize>)> = Vec::new();
    for (name, raw) in &parser.names {
        let n = mapping[*raw];
        match names.iter_mut().find(|(nm, _)| nm == name) {
            Some(e) => e.1.push(n),
            None => names.push((name.clone(), vec![n])),
        }
    }
    // Numbered backrefs must refer to existing groups
    if !has_names {
        fn check(n: &Node, max: usize) -> Result<(), String> {
            match n {
                Node::BackrefNum(k, _) => {
                    if *k > max || *k == 0 {
                        return Err("invalid backref number/name".to_string());
                    }
                    Ok(())
                }
                Node::Seq(v) | Node::Alt(v) => v.iter().try_for_each(|x| check(x, max)),
                Node::Group(b, _) | Node::Atomic(b) => check(b, max),
                Node::Repeat { node, .. } | Node::Look { node, .. } => check(node, max),
                _ => Ok(()),
            }
        }
        check(&ast, parser.ngroups)?;
    }
    let ast = resolve(ast, &|raw| mapping[raw], &names, has_names).map_err(|e| e.0)?;
    let anchored_start = starts_with_begin_buf(&ast);
    let mut c = Compiler { prog: Vec::new(), classes: Vec::new(), nloops: 0, enc };
    c.compile(&ast);
    c.emit(Inst::Match);
    Ok(Regex {
        prog: c.prog,
        classes: c.classes,
        num_groups,
        names,
        noname_capture_active: !has_names,
        nloops: c.nloops,
        enc,
        options,
        anchored_start,
    })
}

fn starts_with_begin_buf(n: &Node) -> bool {
    match n {
        Node::Anchor(Anchor::BeginBuf) => true,
        Node::Seq(v) => v.first().map(starts_with_begin_buf).unwrap_or(false),
        Node::Group(b, _) => starts_with_begin_buf(b),
        _ => false,
    }
}

/* ===== Matcher ===== */

#[derive(Debug, PartialEq, Eq)]
pub enum MatchError {
    StackLimit,
    RetryLimit,
}

impl MatchError {
    pub fn message(&self) -> &'static str {
        match self {
            MatchError::StackLimit => "match-stack limit over",
            MatchError::RetryLimit => "retry-limit-in-match over",
        }
    }
}

pub struct Limits {
    pub stack_limit: usize, // 0 = unlimited
    pub retry_limit: usize, // 0 = unlimited
}

enum Frame {
    Alt(usize, usize),
    Slot(usize, Option<usize>),
    Loop(usize, Option<usize>),
    Keep(Option<usize>),
    Caps(Vec<Option<usize>>),
}

struct Exec<'a> {
    re: &'a Regex,
    codes: &'a [u32],
    start_pos: usize, // \G
    slots: Vec<Option<usize>>,
    loops: Vec<Option<usize>>,
    keep: Option<usize>,
    limits: &'a Limits,
    retries: usize,
    not_empty: bool,
}

impl<'a> Exec<'a> {
    fn is_word(&self, i: usize) -> bool {
        i < self.codes.len() && ctype_match(Ctype::Word, self.codes[i], self.re.enc)
    }

    fn assert(&self, a: Anchor, pos: usize) -> bool {
        let n = self.codes.len();
        let nl = 0x0A;
        match a {
            Anchor::BeginBuf => pos == 0,
            Anchor::EndBuf => pos == n,
            Anchor::SemiEndBuf => pos == n || (pos + 1 == n && self.codes[pos] == nl),
            Anchor::BeginLine => pos == 0 || self.codes[pos - 1] == nl,
            Anchor::EndLine => pos == n || self.codes[pos] == nl,
            Anchor::WordBound => {
                let a = pos > 0 && self.is_word(pos - 1);
                let b = self.is_word(pos);
                a != b
            }
            Anchor::NotWordBound => {
                let a = pos > 0 && self.is_word(pos - 1);
                let b = self.is_word(pos);
                a == b
            }
            Anchor::BeginPosition => pos == self.start_pos,
        }
    }

    /// Run the program from `ip` at `pos`. Returns the end position on
    /// reaching Match.
    fn run(&mut self, ip0: usize, pos0: usize, depth: usize) -> Result<Option<usize>, MatchError> {
        if depth > 200 {
            return Err(MatchError::StackLimit);
        }
        let mut stack: Vec<Frame> = Vec::new();
        let mut ip = ip0;
        let mut pos = pos0;
        let codes = self.codes;
        let enc = self.re.enc;
        loop {
            let ok: bool = match &self.re.prog[ip] {
                Inst::Match => {
                    if depth == 0 && self.not_empty && pos == pos0 {
                        false
                    } else {
                        return Ok(Some(pos));
                    }
                }
                Inst::Char(c) => {
                    if pos < codes.len() && codes[pos] == *c {
                        pos += 1;
                        ip += 1;
                        true
                    } else {
                        false
                    }
                }
                Inst::CharCi(c) => {
                    if pos < codes.len() && fold(codes[pos], enc) == *c {
                        pos += 1;
                        ip += 1;
                        true
                    } else {
                        false
                    }
                }
                Inst::Any => {
                    if pos < codes.len() && codes[pos] != 0x0A {
                        pos += 1;
                        ip += 1;
                        true
                    } else {
                        false
                    }
                }
                Inst::AnyNl => {
                    if pos < codes.len() {
                        pos += 1;
                        ip += 1;
                        true
                    } else {
                        false
                    }
                }
                Inst::Class(idx, ci) => {
                    if pos < codes.len() && self.re.classes[*idx].matches(codes[pos], *ci, enc) {
                        pos += 1;
                        ip += 1;
                        true
                    } else {
                        false
                    }
                }
                Inst::Split(a, b) => {
                    stack.push(Frame::Alt(*b, pos));
                    if self.limits.stack_limit > 0 && stack.len() + 1 > self.limits.stack_limit {
                        return Err(MatchError::StackLimit);
                    }
                    ip = *a;
                    true
                }
                Inst::Jmp(t) => {
                    ip = *t;
                    true
                }
                Inst::Save(s) => {
                    stack.push(Frame::Slot(*s, self.slots[*s]));
                    self.slots[*s] = Some(pos);
                    ip += 1;
                    true
                }
                Inst::Keep => {
                    stack.push(Frame::Keep(self.keep));
                    self.keep = Some(pos);
                    ip += 1;
                    true
                }
                Inst::Assert(a) => {
                    if self.assert(*a, pos) {
                        ip += 1;
                        true
                    } else {
                        false
                    }
                }
                Inst::LoopSave(r) => {
                    stack.push(Frame::Loop(*r, self.loops[*r]));
                    self.loops[*r] = Some(pos);
                    ip += 1;
                    true
                }
                Inst::LoopCheck(r, exit) => {
                    if self.loops[*r] == Some(pos) {
                        ip = *exit;
                    } else {
                        ip += 1;
                    }
                    true
                }
                Inst::Backref(groups, ci) => {
                    // Use the last group that has matched (for duplicate names)
                    let mut found = None;
                    for g in groups.iter().rev() {
                        if let (Some(b), Some(e)) = (self.slots[g * 2], self.slots[g * 2 + 1]) {
                            found = Some((b, e));
                            break;
                        }
                    }
                    match found {
                        None => false,
                        Some((b, e)) => {
                            let len = e.saturating_sub(b);
                            if pos + len > codes.len() {
                                false
                            } else {
                                let eq = (0..len).all(|k| {
                                    let x = codes[b + k];
                                    let y = codes[pos + k];
                                    x == y || (*ci && fold(x, enc) == fold(y, enc))
                                });
                                if eq {
                                    pos += len;
                                    ip += 1;
                                    true
                                } else {
                                    false
                                }
                            }
                        }
                    }
                }
                Inst::Atomic(start) => {
                    let start = *start;
                    let snapshot = self.slots.clone();
                    let keep = self.keep;
                    match self.run(start, pos, depth + 1)? {
                        Some(end) => {
                            stack.push(Frame::Caps(snapshot));
                            stack.push(Frame::Keep(keep));
                            pos = end;
                            ip += 1;
                            true
                        }
                        None => false,
                    }
                }
                Inst::Look(start, ahead, negate, subs) => {
                    let (start, ahead, negate) = (*start, *ahead, *negate);
                    let subs = subs.clone();
                    let snapshot = self.slots.clone();
                    let mut matched = false;
                    if ahead {
                        matched = self.run(start, pos, depth + 1)?.is_some();
                    } else {
                        for (st, len) in subs {
                            if pos >= len {
                                if let Some(end) = self.run(st, pos - len, depth + 1)? {
                                    if end == pos {
                                        matched = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    if matched != negate {
                        if negate {
                            self.slots = snapshot;
                        } else {
                            stack.push(Frame::Caps(snapshot));
                        }
                        ip += 1;
                        true
                    } else {
                        self.slots = snapshot;
                        false
                    }
                }
            };
            if !ok {
                // Backtrack
                loop {
                    match stack.pop() {
                        None => return Ok(None),
                        Some(Frame::Alt(i, p)) => {
                            self.retries += 1;
                            if self.limits.retry_limit > 0 && self.retries > self.limits.retry_limit {
                                return Err(MatchError::RetryLimit);
                            }
                            ip = i;
                            pos = p;
                            break;
                        }
                        Some(Frame::Slot(s, v)) => self.slots[s] = v,
                        Some(Frame::Loop(r, v)) => self.loops[r] = v,
                        Some(Frame::Keep(v)) => self.keep = v,
                        Some(Frame::Caps(v)) => self.slots = v,
                    }
                }
            }
        }
    }
}

pub struct Subject {
    codes: Vec<u32>,
    offs: Vec<usize>,
}

/// Match region: byte offsets per group (None = did not participate)
#[derive(Clone, Debug)]
pub struct Region {
    pub beg: Vec<Option<usize>>,
    pub end: Vec<Option<usize>>,
}

impl Regex {
    pub fn options(&self) -> u32 {
        self.options
    }

    /// onig_name_to_backref_number: the last matched group for a name
    pub fn name_to_backref_number(&self, name: &[u8], region: Option<&Region>) -> Option<usize> {
        let (_, nums) = self.names.iter().find(|(n, _)| n == name)?;
        if let Some(r) = region {
            for &n in nums.iter().rev() {
                if r.beg.get(n).copied().flatten().is_some() {
                    return Some(n);
                }
            }
        }
        nums.last().copied()
    }

    fn exec_at(&self, codes: &[u32], start: usize, at: usize, limits: &Limits) -> Result<Option<(Vec<Option<usize>>, usize)>, MatchError> {
        let mut ex = Exec {
            re: self,
            codes,
            start_pos: start,
            slots: vec![None; (self.num_groups + 1) * 2],
            loops: vec![None; self.nloops],
            keep: None,
            limits,
            retries: 0,
            not_empty: self.options & OPT_FIND_NOT_EMPTY != 0,
        };
        match ex.run(0, at, 0)? {
            Some(end) => {
                let mut slots = ex.slots;
                slots[0] = Some(ex.keep.unwrap_or(at));
                slots[1] = Some(end);
                Ok(Some((slots, end)))
            }
            None => Ok(None),
        }
    }

    /// Decode a subject once for repeated searches
    pub fn prepare(&self, subject: &[u8]) -> Subject {
        let (codes, offs) = self.enc.decode_all(subject);
        Subject { codes, offs }
    }

    /// onig_search: find the first match starting at byte `start` or later.
    pub fn search(&self, subject: &[u8], start: usize, limits: &Limits) -> Result<Option<Region>, MatchError> {
        let s = self.prepare(subject);
        self.search_prepared(&s, start, limits)
    }

    pub fn search_prepared(&self, s: &Subject, start: usize, limits: &Limits) -> Result<Option<Region>, MatchError> {
        let start_idx = match s.offs.binary_search(&start) {
            Ok(i) => i,
            Err(i) => i,
        };
        let mut i = start_idx;
        while i <= s.codes.len() {
            if self.anchored_start && i > 0 {
                break;
            }
            if let Some((slots, _)) = self.exec_at(&s.codes, start_idx, i, limits)? {
                return Ok(Some(self.make_region(&slots, &s.offs)));
            }
            i += 1;
        }
        Ok(None)
    }

    /// onig_match: match anchored at byte `at`. Returns the region.
    pub fn match_at(&self, subject: &[u8], at: usize, limits: &Limits) -> Result<Option<Region>, MatchError> {
        let (codes, offs) = self.enc.decode_all(subject);
        let idx = match offs.binary_search(&at) {
            Ok(i) => i,
            Err(i) => i,
        };
        match self.exec_at(&codes, idx, idx, limits)? {
            Some((slots, _)) => Ok(Some(self.make_region(&slots, &offs))),
            None => Ok(None),
        }
    }

    fn make_region(&self, slots: &[Option<usize>], offs: &[usize]) -> Region {
        let n = self.num_groups + 1;
        let mut beg = Vec::with_capacity(n);
        let mut end = Vec::with_capacity(n);
        for g in 0..n {
            match (slots[g * 2], slots[g * 2 + 1]) {
                (Some(b), Some(e)) => {
                    beg.push(Some(offs[b]));
                    end.push(Some(offs[e]));
                }
                _ => {
                    beg.push(None);
                    end.push(None);
                }
            }
        }
        Region { beg, end }
    }
}
