//! Character sets: `[...]` classes, `\d`-style types and `\p{..}` properties.
//!
//! A set is resolved at compile time into a 256-bit map for code points below
//! 256 (so properties, POSIX names and case folding cost nothing there) plus a
//! list of items tested for larger code points.

use crate::unicode;

/// A test applied to code points >= 256.
#[derive(Clone, Debug)]
pub enum HighItem {
    /// Inclusive range of code points.
    Range(u32, u32),
    /// General category in the mask (bit = index into `unicode::GC_NAMES`).
    Gc {
        mask: u32,
        neg: bool,
    },
    Script {
        idx: u8,
        neg: bool,
    },
    Scx {
        idx: u8,
        neg: bool,
    },
    Bidi {
        idx: u8,
        neg: bool,
    },
    Bprop {
        idx: u8,
        neg: bool,
    },
    /// Everything (e.g. `\D` outside UCP mode, `\P{Cn}`-style complements are
    /// expressed with `neg`).
    All,
    /// Everything except the listed ranges (used for `\H`, `\V`).
    NotRanges(Vec<(u32, u32)>),
    /// An arbitrary predicate (POSIX classes in UCP mode and the like).
    Pred {
        f: fn(u32) -> bool,
        neg: bool,
    },
    /// A set expression of an extended class `(?[...])`.
    Expr(std::sync::Arc<SetExpr>),
}

/// Set operations of Perl extended classes (pcre2pattern "Perl extended
/// character classes").
#[derive(Clone, Debug)]
pub enum SetExpr {
    Leaf(std::sync::Arc<CharSet>),
    Not(Box<SetExpr>),
    And(Box<SetExpr>, Box<SetExpr>),
    Or(Box<SetExpr>, Box<SetExpr>),
    Diff(Box<SetExpr>, Box<SetExpr>),
    Xor(Box<SetExpr>, Box<SetExpr>),
}

impl SetExpr {
    pub fn eval(&self, c: u32) -> bool {
        match self {
            SetExpr::Leaf(s) => s.matches(c),
            SetExpr::Not(a) => !a.eval(c),
            SetExpr::And(a, b) => a.eval(c) && b.eval(c),
            SetExpr::Or(a, b) => a.eval(c) || b.eval(c),
            SetExpr::Diff(a, b) => a.eval(c) && !b.eval(c),
            SetExpr::Xor(a, b) => a.eval(c) != b.eval(c),
        }
    }

    /// The resolved set.
    pub fn into_set(self) -> CharSet {
        let mut set = CharSet::default();
        for c in 0..256u32 {
            if self.eval(c) {
                set.bits[(c >> 6) as usize] |= 1 << (c & 63);
            }
        }
        set.items.push(HighItem::Expr(std::sync::Arc::new(self)));
        set.finish();
        set
    }
}

impl HighItem {
    #[inline]
    fn matches(&self, c: u32) -> bool {
        match self {
            HighItem::Range(lo, hi) => *lo <= c && c <= *hi,
            HighItem::Gc { mask, neg } => (mask >> unicode::gc(c) & 1 != 0) != *neg,
            HighItem::Script { idx, neg } => (unicode::script(c) == *idx) != *neg,
            HighItem::Scx { idx, neg } => unicode::scx_contains(c, *idx) != *neg,
            HighItem::Bidi { idx, neg } => (unicode::bidi(c) == *idx) != *neg,
            HighItem::Bprop { idx, neg } => unicode::has_bprop(c, *idx) != *neg,
            HighItem::All => true,
            HighItem::NotRanges(r) => !r.iter().any(|&(lo, hi)| lo <= c && c <= hi),
            HighItem::Pred { f, neg } => f(c) != *neg,
            HighItem::Expr(e) => e.eval(c),
        }
    }
}

/// A resolved character set.
#[derive(Clone, Debug)]
pub struct CharSet {
    /// Membership of code points 0..=255 (negation already applied).
    pub bits: [u64; 4],
    /// Sorted, merged ranges for code points >= 256 (positive items).
    pub ranges: Vec<(u32, u32)>,
    /// Other tests for code points >= 256.
    pub items: Vec<HighItem>,
    /// Whether the >= 256 part is negated (`[^...]`).
    pub negated: bool,
    /// Precomputed: every code point >= 256 is in the set.
    pub high_all: bool,
    /// Precomputed: no code point >= 256 is in the set.
    pub high_none: bool,
    /// Membership of code points 0..=255 as a lookup table (filled by
    /// `finish`).
    pub lut: [bool; 256],
    /// PCRE2's compiled size of the item (1 for \d and friends, 3 for
    /// \p, 33 for a class bitmap, more with characters above 255).
    pub op_size: u16,
}

impl Default for CharSet {
    fn default() -> CharSet {
        CharSet {
            bits: [0; 4],
            ranges: Vec::new(),
            items: Vec::new(),
            negated: false,
            high_all: false,
            high_none: true,
            lut: [false; 256],
            op_size: 33,
        }
    }
}

impl CharSet {
    /// Whether PCRE2 compiles this as a class (rather than a character
    /// type such as \w or \p, or a negated character such as [^b]); a
    /// greedy repeat of a class counts one more try (observed on PHP 8.5:
    /// /[a-z]+ing/ needs one more match limit than /\w+ing/ or /[^b]+ing/).
    pub fn pcre_class(&self) -> bool {
        if self.op_size < 33 {
            return false;
        }
        let missing: u32 = self.bits.iter().map(|w| w.count_zeros()).sum();
        if self.high_all && (missing == 1 || missing == 2) {
            let mut v = (0..256u32).filter(|&c| !self.lut[c as usize]);
            let a = v.next().unwrap_or(0);
            let b = v.next();
            return !(b.is_none()
                || b.is_some_and(|b| b < 0x80 && (a as u8).to_ascii_lowercase() == b as u8));
        }
        true
    }

    #[inline(always)]
    pub fn has_byte(&self, b: u8) -> bool {
        self.lut[b as usize]
    }

    #[inline(always)]
    pub fn matches(&self, c: u32) -> bool {
        if c < 256 {
            return self.lut[c as usize];
        }
        if self.high_none {
            return false;
        }
        if self.high_all {
            return true;
        }
        self.matches_high(c)
    }

    #[inline(never)]
    fn matches_high(&self, c: u32) -> bool {
        let hit = self.ranges_contain(c) || self.items.iter().any(|i| i.matches(c));
        hit != self.negated
    }

    fn ranges_contain(&self, c: u32) -> bool {
        let r = &self.ranges;
        if r.len() < 8 {
            return r.iter().any(|&(lo, hi)| lo <= c && c <= hi);
        }
        // Binary search for the last range starting at or before c.
        let i = r.partition_point(|&(lo, _)| lo <= c);
        i > 0 && c <= r[i - 1].1
    }

    /// No member is above 127 (the set matches single bytes in UTF-8).
    #[inline(always)]
    pub fn ascii_only(&self) -> bool {
        self.high_none && self.bits[2] == 0 && self.bits[3] == 0
    }

    pub fn set_bit(&mut self, c: u32) {
        self.bits[(c >> 6) as usize] |= 1 << (c & 63);
    }

    /// Number of members below 256.
    pub fn low_count(&self) -> u32 {
        self.bits.iter().map(|w| w.count_ones()).sum()
    }

    /// The single byte value, if the set is exactly one code point below 256
    /// and nothing above.
    pub fn single_byte(&self) -> Option<u8> {
        if !self.high_none || self.low_count() != 1 {
            return None;
        }
        (0..256u32)
            .find(|&c| self.has_byte(c as u8))
            .map(|c| c as u8)
    }

    /// Computes `high_all` / `high_none` after building.
    pub fn finish(&mut self) {
        for b in 0..256usize {
            self.lut[b] = self.bits[b >> 6] >> (b & 63) & 1 != 0;
        }
        self.ranges.sort_unstable();
        let mut merged: Vec<(u32, u32)> = Vec::with_capacity(self.ranges.len());
        for &(lo, hi) in &self.ranges {
            if let Some(last) = merged.last_mut()
                && lo <= last.1.saturating_add(1)
            {
                if hi > last.1 {
                    last.1 = hi;
                }
                continue;
            }
            merged.push((lo, hi));
        }
        self.ranges = merged;
        let any_all = self.items.iter().any(|i| matches!(i, HighItem::All))
            || self
                .ranges
                .iter()
                .any(|&(lo, hi)| lo <= 256 && hi >= 0x10ffff);
        let empty = self.ranges.is_empty() && self.items.is_empty();
        if any_all {
            self.high_all = !self.negated;
            self.high_none = self.negated;
        } else if empty {
            self.high_all = self.negated;
            self.high_none = !self.negated;
        } else {
            self.high_all = false;
            self.high_none = false;
        }
    }
}

/// Builds a [`CharSet`] from class items.
#[derive(Clone, Debug, Default)]
pub struct ClassBuilder {
    pub bits: [u64; 4],
    pub ranges: Vec<(u32, u32)>,
    pub items: Vec<HighItem>,
}

/// How case-insensitive matching treats a character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseMode {
    /// No folding.
    None,
    /// ASCII-only folding (non-UTF, non-UCP mode: the C locale tables).
    Ascii,
    /// Unicode folding (UTF or UCP mode).
    Unicode,
    /// Unicode folding without ASCII/non-ASCII mixing
    /// (PCRE2_EXTRA_CASELESS_RESTRICT).
    UnicodeRestrict,
}

/// The characters case-equivalent to `c` (including `c`), at most a handful.
pub fn case_equivalents(c: u32, mode: CaseMode, out: &mut Vec<u32>) {
    out.clear();
    out.push(c);
    match mode {
        CaseMode::None => {}
        CaseMode::Ascii => {
            if c < 128 && (c as u8).is_ascii_alphabetic() {
                out.push(c ^ 0x20);
            }
        }
        CaseMode::Unicode | CaseMode::UnicodeRestrict => {
            let set = unicode::case_set(c);
            for &o in set {
                if o == c {
                    continue;
                }
                if mode == CaseMode::UnicodeRestrict && ((o < 128) != (c < 128)) {
                    continue;
                }
                out.push(o);
            }
        }
    }
}

impl ClassBuilder {
    pub fn new() -> ClassBuilder {
        ClassBuilder::default()
    }

    #[inline]
    fn set_bit(&mut self, c: u32) {
        self.bits[(c >> 6) as usize] |= 1 << (c & 63);
    }

    /// Adds one code point (no case folding).
    pub fn add_char(&mut self, c: u32) {
        if c < 256 {
            self.set_bit(c);
        } else {
            self.ranges.push((c, c));
        }
    }

    /// Adds a character and its case variants.
    pub fn add_char_caseless(&mut self, c: u32, mode: CaseMode) {
        let mut eq = Vec::new();
        case_equivalents(c, mode, &mut eq);
        for e in eq {
            self.add_char(e);
        }
    }

    /// Adds an inclusive range (no case folding).
    pub fn add_range(&mut self, lo: u32, hi: u32) {
        if lo > hi {
            return;
        }
        let mut c = lo;
        while c <= hi && c < 256 {
            self.set_bit(c);
            c += 1;
        }
        if hi >= 256 {
            self.ranges.push((lo.max(256), hi));
        }
    }

    /// Adds a range and the case variants of everything in it.
    pub fn add_range_caseless(&mut self, lo: u32, hi: u32, mode: CaseMode) {
        self.add_range(lo, hi);
        match mode {
            CaseMode::None => {}
            CaseMode::Ascii => {
                for c in lo..=hi.min(127) {
                    if (c as u8).is_ascii_alphabetic() {
                        self.set_bit(c ^ 0x20);
                    }
                }
            }
            CaseMode::Unicode | CaseMode::UnicodeRestrict => {
                if hi - lo < 512 {
                    let mut eq = Vec::new();
                    for c in lo..=hi {
                        case_equivalents(c, mode, &mut eq);
                        for &e in &eq[1..] {
                            self.add_char(e);
                        }
                    }
                } else {
                    // Walk the case sets instead of the range.
                    for set in unicode::CASE_SETS.iter() {
                        for &m in set.iter() {
                            if lo <= m && m <= hi {
                                for &o in set.iter() {
                                    if mode == CaseMode::UnicodeRestrict && ((o < 128) != (m < 128))
                                    {
                                        continue;
                                    }
                                    self.add_char(o);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Adds every code point below 256 satisfying `pred`, and `high` for the
    /// rest.
    pub fn add_pred(&mut self, pred: impl Fn(u32) -> bool, high: Option<HighItem>) {
        for c in 0..256u32 {
            if pred(c) {
                self.set_bit(c);
            }
        }
        if let Some(h) = high {
            match h {
                HighItem::Range(lo, hi) => self.ranges.push((lo, hi)),
                other => self.items.push(other),
            }
        }
    }

    /// Merges another set's contents (union).
    pub fn add_set(&mut self, other: &CharSet) {
        for i in 0..4 {
            self.bits[i] |= other.bits[i];
        }
        if other.negated {
            // Union with a complemented high part: express as an item.
            if other.high_all {
                self.items.push(HighItem::All);
            } else if !other.high_none {
                // Fall back: complement of (ranges ∪ items) is not
                // representable as one item in general; approximate by
                // enumerating the non-range items' negations when there is a
                // single item.
                if other.ranges.is_empty() && other.items.len() == 1 {
                    self.items.push(negate_item(&other.items[0]));
                } else if other.items.is_empty() {
                    self.items.push(HighItem::NotRanges(other.ranges.clone()));
                } else {
                    self.items.push(HighItem::All);
                }
            }
        } else {
            self.ranges.extend_from_slice(&other.ranges);
            self.items.extend(other.items.iter().cloned());
        }
    }

    /// Produces the set, applying `negated` (from `[^`).
    pub fn build(mut self, negated: bool) -> CharSet {
        if negated {
            for w in self.bits.iter_mut() {
                *w = !*w;
            }
        }
        // Normalise single-range items.
        let mut set = CharSet {
            bits: self.bits,
            ranges: std::mem::take(&mut self.ranges),
            items: std::mem::take(&mut self.items),
            negated,
            high_all: false,
            high_none: true,
            lut: [false; 256],
            op_size: 33,
        };
        set.finish();
        if !set.high_none && !set.high_all {
            // An extended class: header, bitmap, then its items.
            set.op_size = (4 + 32 + 7 * set.ranges.len() + 3 * set.items.len()).min(60000) as u16;
        }
        set
    }
}

/// The complement of a high item.
pub fn negate_item(i: &HighItem) -> HighItem {
    match i {
        HighItem::Range(lo, hi) => HighItem::NotRanges(vec![(*lo, *hi)]),
        HighItem::Gc { mask, neg } => HighItem::Gc {
            mask: *mask,
            neg: !neg,
        },
        HighItem::Script { idx, neg } => HighItem::Script {
            idx: *idx,
            neg: !neg,
        },
        HighItem::Scx { idx, neg } => HighItem::Scx {
            idx: *idx,
            neg: !neg,
        },
        HighItem::Bidi { idx, neg } => HighItem::Bidi {
            idx: *idx,
            neg: !neg,
        },
        HighItem::Bprop { idx, neg } => HighItem::Bprop {
            idx: *idx,
            neg: !neg,
        },
        HighItem::Pred { f, neg } => HighItem::Pred { f: *f, neg: !neg },
        HighItem::Expr(e) => {
            HighItem::Expr(std::sync::Arc::new(SetExpr::Not(Box::new((**e).clone()))))
        }
        HighItem::All => HighItem::NotRanges(vec![(0, 0x7fff_ffff)]),
        HighItem::NotRanges(r) => {
            if r.len() == 1 {
                HighItem::Range(r[0].0, r[0].1)
            } else {
                // Not needed by the parser; keep it conservative.
                HighItem::All
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Character type predicates (pcre2pattern: "Generic character types").

/// The horizontal space characters (pcre2pattern lists them; they do not
/// depend on PCRE2_UCP).
pub const HSPACE: &[u32] = &[
    0x09, 0x20, 0xa0, 0x1680, 0x180e, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006,
    0x2007, 0x2008, 0x2009, 0x200a, 0x202f, 0x205f, 0x3000,
];
pub const HSPACE_HIGH: &[(u32, u32)] = &[
    (0x1680, 0x1680),
    (0x180e, 0x180e),
    (0x2000, 0x200a),
    (0x202f, 0x202f),
    (0x205f, 0x205f),
    (0x3000, 0x3000),
];
/// The vertical space characters.
pub const VSPACE: &[u32] = &[0x0a, 0x0b, 0x0c, 0x0d, 0x85, 0x2028, 0x2029];
pub const VSPACE_HIGH: &[(u32, u32)] = &[(0x2028, 0x2029)];

#[inline]
pub fn is_hspace(c: u32) -> bool {
    match c {
        0x09 | 0x20 | 0xa0 => true,
        _ if c < 0x1680 => false,
        0x1680 | 0x180e | 0x2000..=0x200a | 0x202f | 0x205f | 0x3000 => true,
        _ => false,
    }
}

#[inline]
pub fn is_vspace(c: u32) -> bool {
    matches!(c, 0x0a..=0x0d | 0x85 | 0x2028 | 0x2029)
}

/// Bit mask over general categories.
pub const fn gc_bit(g: u8) -> u32 {
    1 << g
}

pub const GC_MASK_L: u32 = gc_bit(unicode::GC_LL)
    | gc_bit(unicode::GC_LM)
    | gc_bit(unicode::GC_LO)
    | gc_bit(unicode::GC_LT)
    | gc_bit(unicode::GC_LU);
pub const GC_MASK_LC: u32 =
    gc_bit(unicode::GC_LL) | gc_bit(unicode::GC_LT) | gc_bit(unicode::GC_LU);
pub const GC_MASK_N: u32 = gc_bit(unicode::GC_ND) | gc_bit(unicode::GC_NL) | gc_bit(unicode::GC_NO);
pub const GC_MASK_M: u32 = gc_bit(unicode::GC_MC) | gc_bit(unicode::GC_ME) | gc_bit(unicode::GC_MN);
pub const GC_MASK_P: u32 = gc_bit(unicode::GC_PC)
    | gc_bit(unicode::GC_PD)
    | gc_bit(unicode::GC_PE)
    | gc_bit(unicode::GC_PF)
    | gc_bit(unicode::GC_PI)
    | gc_bit(unicode::GC_PO)
    | gc_bit(unicode::GC_PS);
pub const GC_MASK_S: u32 = gc_bit(unicode::GC_SC)
    | gc_bit(unicode::GC_SK)
    | gc_bit(unicode::GC_SM)
    | gc_bit(unicode::GC_SO);
pub const GC_MASK_Z: u32 = gc_bit(unicode::GC_ZL) | gc_bit(unicode::GC_ZP) | gc_bit(unicode::GC_ZS);
pub const GC_MASK_C: u32 = gc_bit(unicode::GC_CC)
    | gc_bit(unicode::GC_CF)
    | gc_bit(unicode::GC_CN)
    | gc_bit(unicode::GC_CO)
    | gc_bit(unicode::GC_CS);
/// \w in UCP mode: L, N, Mn, Pc (pcre2pattern, release 10.43).
pub const GC_MASK_WORD: u32 =
    GC_MASK_L | GC_MASK_N | gc_bit(unicode::GC_MN) | gc_bit(unicode::GC_PC);
pub const GC_MASK_ALNUM: u32 = GC_MASK_L | GC_MASK_N;

#[inline]
pub fn gc_in(c: u32, mask: u32) -> bool {
    mask >> unicode::gc(c) & 1 != 0
}

/// ASCII word character (the default C-locale tables).
#[inline(always)]
pub fn is_ascii_word(c: u32) -> bool {
    c < 128 && ((c as u8).is_ascii_alphanumeric() || c == b'_' as u32)
}

#[inline(always)]
pub fn is_ascii_space(c: u32) -> bool {
    matches!(c, 0x09..=0x0d | 0x20)
}

/// \w in UCP mode.
#[inline]
pub fn is_ucp_word(c: u32) -> bool {
    if c < 128 {
        return is_ascii_word(c);
    }
    gc_in(c, GC_MASK_WORD)
}

/// \s in UCP mode: \p{Z}, \h or \v.
#[inline]
pub fn is_ucp_space(c: u32) -> bool {
    if c < 128 {
        return is_ascii_space(c);
    }
    is_hspace(c) || is_vspace(c) || gc_in(c, GC_MASK_Z)
}

/// Xps / Xsp: \p{Z} or tab, LF, VT, FF, CR.
#[inline]
pub fn is_posix_space_ucp(c: u32) -> bool {
    matches!(c, 0x09..=0x0d) || gc_in(c, GC_MASK_Z)
}

/// Xuc: $, @, `, and code points >= U+00A0 except surrogates.
#[inline]
pub fn is_xuc(c: u32) -> bool {
    c == 0x24 || c == 0x40 || c == 0x60 || (c >= 0xa0 && !(0xd800..=0xdfff).contains(&c))
}

/// The POSIX class names, in PCRE2's documented list.
pub const POSIX_NAMES: &[&str] = &[
    "alpha", "lower", "upper", "alnum", "ascii", "blank", "cntrl", "digit", "graph", "print",
    "punct", "space", "word", "xdigit",
];

/// ASCII definition of a POSIX class (C locale).
pub fn posix_ascii(name: &str, c: u32) -> bool {
    if c >= 128 {
        return false;
    }
    let b = c as u8;
    match name {
        "alpha" => b.is_ascii_alphabetic(),
        "lower" => b.is_ascii_lowercase(),
        "upper" => b.is_ascii_uppercase(),
        "alnum" => b.is_ascii_alphanumeric(),
        "ascii" => true,
        "blank" => b == b' ' || b == b'\t',
        "cntrl" => b < 32 || b == 127,
        "digit" => b.is_ascii_digit(),
        "graph" => (33..127).contains(&b),
        "print" => (32..127).contains(&b),
        "punct" => b.is_ascii_punctuation(),
        "space" => is_ascii_space(c),
        "word" => is_ascii_word(c),
        "xdigit" => b.is_ascii_hexdigit(),
        _ => false,
    }
}

/// UCP definitions of [:graph:], [:print:], [:punct:] (pcre2pattern: "Four
/// other POSIX classes are handled specially in UCP mode").
pub fn ucp_graph(c: u32) -> bool {
    if c == 0x061c || c == 0x180e || (0x2066..=0x2069).contains(&c) {
        return false;
    }
    gc_in(
        c,
        GC_MASK_L | GC_MASK_M | GC_MASK_N | GC_MASK_P | GC_MASK_S | gc_bit(unicode::GC_CF),
    )
}

pub fn ucp_print(c: u32) -> bool {
    ucp_graph(c) || (unicode::gc(c) == unicode::GC_ZS)
}

pub fn ucp_punct(c: u32) -> bool {
    gc_in(c, GC_MASK_P) || (c < 256 && gc_in(c, GC_MASK_S))
}

pub fn ucp_xdigit(c: u32) -> bool {
    (c < 128 && (c as u8).is_ascii_hexdigit())
        || (0xff10..=0xff19).contains(&c)
        || (0xff21..=0xff26).contains(&c)
        || (0xff41..=0xff46).contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_sets() {
        let mut b = ClassBuilder::new();
        b.add_range(b'a' as u32, b'z' as u32);
        b.add_char(0x100);
        let s = b.build(false);
        assert!(s.matches('q' as u32));
        assert!(!s.matches('A' as u32));
        assert!(s.matches(0x100));
        assert!(!s.matches(0x101));
        let mut b = ClassBuilder::new();
        b.add_char(b'a' as u32);
        let s = b.build(true);
        assert!(!s.matches('a' as u32));
        assert!(s.matches('b' as u32));
        assert!(s.matches(0x1000));
    }

    #[test]
    fn caseless() {
        let mut b = ClassBuilder::new();
        b.add_char_caseless('k' as u32, CaseMode::Unicode);
        let s = b.build(false);
        assert!(s.matches('K' as u32));
        assert!(s.matches(0x212a));
        let mut b = ClassBuilder::new();
        b.add_char_caseless('k' as u32, CaseMode::UnicodeRestrict);
        let s = b.build(false);
        assert!(s.matches('K' as u32));
        assert!(!s.matches(0x212a));
        let mut b = ClassBuilder::new();
        b.add_range_caseless(0x100, 0x17f, CaseMode::Unicode);
        let s = b.build(false);
        assert!(s.matches(0x101));
        assert!(!s.matches(' ' as u32));
    }
}
