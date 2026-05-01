//! Basic regex engine for PHP preg_* functions.
//!
//! Supports: literal chars, `.`, `*`, `+`, `?`, `^`, `$`, `[abc]`, `[a-z]`, `[^abc]`,
//! `\d`, `\w`, `\s`, `\D`, `\W`, `\S`, `\b`, `|`, `()` capture groups,
//! `i` flag, escaped special chars, backreferences in replacements.
//!
//! This is NOT a full PCRE implementation — just enough for common PHP test patterns.

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

// ============================================================================
// Regex AST
// ============================================================================

#[derive(Debug, Clone)]
enum RegexNode {
    /// Literal byte
    Literal(u8),
    /// Literal codepoint (used in UTF-8 mode for non-ASCII)
    LiteralCp(u32),
    /// `.` — any char except newline (unless `s` flag)
    AnyChar,
    /// `^` — start of string (or line in multiline mode)
    StartAnchor,
    /// `$` — end of string (or line in multiline mode)
    EndAnchor,
    /// `\b` — word boundary
    WordBoundary,
    /// `\B` — non-word boundary
    NonWordBoundary,
    /// `\K` — reset match start
    ResetMatchStart,
    /// Character class `[...]` or shorthand `\d`, `\w`, etc.
    CharClass {
        ranges: Vec<CharRange>,
        classes: Vec<UnicodeClassKind>,
        /// Anti-classes: character kinds that should NOT be matched.
        /// A char matches the class if it matches any positive criteria (ranges/classes)
        /// OR doesn't match any anti-class (when considering anti-classes as positive complements).
        /// Concretely: anti_classes entries contribute (complement of class) to the positive set.
        anti_classes: Vec<UnicodeClassKind>,
        negated: bool,
    },
    /// Quantifier: greedy `*`, `+`, `?`, `{n,m}`
    Quantifier {
        node: Box<RegexNode>,
        min: usize,
        max: Option<usize>, // None = unlimited
        greedy: bool,
    },
    /// Alternation `a|b`
    Alternation(Vec<RegexNode>),
    /// Sequence of nodes
    Sequence(Vec<RegexNode>),
    /// Capture group `(...)`
    Group {
        index: usize, // 1-based group number
        node: Box<RegexNode>,
    },
    /// Non-capturing group `(?:...)`
    NonCapturingGroup {
        node: Box<RegexNode>,
    },
    /// Lookahead `(?=...)` or `(?!...)`
    Lookahead {
        node: Box<RegexNode>,
        positive: bool,
    },
    /// Lookbehind `(?<=...)` or `(?<!...)`
    Lookbehind {
        node: Box<RegexNode>,
        positive: bool,
    },
    /// Backreference `\1`, `\2`, etc.
    Backreference(usize),
}

#[derive(Debug, Clone)]
enum CharRange {
    Single(u32),
    Range(u32, u32),
}

/// Kinds of Unicode/POSIX-style character classes that require function-based classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnicodeClassKind {
    // Unicode property \p{...}
    Letter,              // L
    Mark,                // M
    Number,              // N
    NumberDecimal,       // Nd
    Punctuation,         // P
    Symbol,              // S
    Separator,           // Z
    Other,               // C
    LowercaseLetter,     // Ll
    UppercaseLetter,     // Lu
    TitlecaseLetter,     // Lt
    ModifierLetter,      // Lm
    OtherLetter,         // Lo
    // Scripts
    ScriptCyrillic,
    ScriptGreek,
    ScriptLatin,
    ScriptHan,
    ScriptArabic,
    ScriptHebrew,
    ScriptHiragana,
    ScriptKatakana,
    // Unicode-aware shorthand
    UniWord,      // \w with unicode semantics
    UniDigit,     // \d with unicode semantics
    UniSpace,     // \s with unicode semantics
    // POSIX classes (byte-level)
    PosixAlpha,
    PosixUpper,
    PosixLower,
    PosixDigit,
    PosixXDigit,
    PosixAlnum,
    PosixSpace,
    PosixBlank,
    PosixCntrl,
    PosixGraph,
    PosixPrint,
    PosixPunct,
    PosixWord,
    PosixAscii,
}

#[derive(Debug, Clone)]
struct NegatedClass(UnicodeClassKind);

// ============================================================================
// Flags
// ============================================================================

#[derive(Debug, Clone, Default)]
struct RegexFlags {
    case_insensitive: bool, // i
    multiline: bool,        // m
    dotall: bool,           // s
    extended: bool,         // x
    ungreedy: bool,         // U
    anchored: bool,         // A
    utf8: bool,             // u
    no_auto_capture: bool,  // n — make unnamed groups non-capturing
    dollar_end_only: bool,  // D — $ matches only end, not before trailing \n
}

// ============================================================================
// Parser
// ============================================================================

struct RegexParser<'a> {
    input: &'a [u8],
    pos: usize,
    group_count: usize,
    flags: RegexFlags,
    group_names: Vec<(usize, Vec<u8>)>,
}

impl<'a> RegexParser<'a> {
    fn new(input: &'a [u8], flags: RegexFlags) -> Self {
        Self {
            input,
            pos: 0,
            group_count: 0,
            flags,
            group_names: Vec::new(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let ch = self.input.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn parse(&mut self) -> Result<RegexNode, String> {
        let node = self.parse_alternation()?;
        if self.pos < self.input.len() {
            // There shouldn't be remaining input at top level
            // (unless it's a `)` which is handled by group parsing)
        }
        Ok(node)
    }

    fn parse_alternation(&mut self) -> Result<RegexNode, String> {
        let mut branches = vec![self.parse_sequence()?];
        while self.peek() == Some(b'|') {
            self.advance();
            branches.push(self.parse_sequence()?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().unwrap())
        } else {
            Ok(RegexNode::Alternation(branches))
        }
    }

    fn parse_sequence(&mut self) -> Result<RegexNode, String> {
        let mut nodes = Vec::new();
        while let Some(ch) = self.peek() {
            if ch == b')' || ch == b'|' {
                break;
            }
            nodes.push(self.parse_quantified()?);
        }
        if nodes.len() == 1 {
            Ok(nodes.pop().unwrap())
        } else {
            Ok(RegexNode::Sequence(nodes))
        }
    }

    fn parse_quantified(&mut self) -> Result<RegexNode, String> {
        let node = self.parse_atom()?;
        if let Some(ch) = self.peek() {
            let default_greedy = !self.flags.ungreedy;
            match ch {
                b'*' => {
                    self.advance();
                    let greedy = if self.peek() == Some(b'?') {
                        self.advance();
                        !default_greedy
                    } else {
                        default_greedy
                    };
                    Ok(RegexNode::Quantifier {
                        node: Box::new(node),
                        min: 0,
                        max: None,
                        greedy,
                    })
                }
                b'+' => {
                    self.advance();
                    let greedy = if self.peek() == Some(b'?') {
                        self.advance();
                        !default_greedy
                    } else if self.peek() == Some(b'+') {
                        // Possessive quantifier `++` — treat as greedy for now
                        self.advance();
                        true
                    } else {
                        default_greedy
                    };
                    Ok(RegexNode::Quantifier {
                        node: Box::new(node),
                        min: 1,
                        max: None,
                        greedy,
                    })
                }
                b'?' => {
                    self.advance();
                    let greedy = if self.peek() == Some(b'?') {
                        self.advance();
                        !default_greedy
                    } else {
                        default_greedy
                    };
                    Ok(RegexNode::Quantifier {
                        node: Box::new(node),
                        min: 0,
                        max: Some(1),
                        greedy,
                    })
                }
                b'{' => {
                    // Try to parse {n}, {n,}, {n,m}
                    let saved_pos = self.pos;
                    self.advance(); // skip `{`
                    if let Some((min, max)) = self.parse_braces() {
                        let greedy = if self.peek() == Some(b'?') {
                            self.advance();
                            !default_greedy
                        } else {
                            default_greedy
                        };
                        Ok(RegexNode::Quantifier {
                            node: Box::new(node),
                            min,
                            max,
                            greedy,
                        })
                    } else {
                        // Not a valid quantifier, treat `{` as literal
                        self.pos = saved_pos;
                        Ok(node)
                    }
                }
                _ => Ok(node),
            }
        } else {
            Ok(node)
        }
    }

    fn parse_braces(&mut self) -> Option<(usize, Option<usize>)> {
        let start = self.pos;
        let min = self.parse_number()?;
        match self.peek() {
            Some(b'}') => {
                self.advance();
                Some((min, Some(min))) // {n} — exact
            }
            Some(b',') => {
                self.advance();
                if self.peek() == Some(b'}') {
                    self.advance();
                    Some((min, None)) // {n,} — at least n
                } else {
                    let max = self.parse_number();
                    if self.peek() == Some(b'}') {
                        self.advance();
                        Some((min, Some(max.unwrap_or(min)))) // {n,m}
                    } else {
                        self.pos = start;
                        None
                    }
                }
            }
            _ => {
                self.pos = start;
                None
            }
        }
    }

    fn parse_number(&mut self) -> Option<usize> {
        let mut n: usize = 0;
        let mut found = false;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                n = n.saturating_mul(10).saturating_add((ch - b'0') as usize);
                self.advance();
                found = true;
            } else {
                break;
            }
        }
        if found {
            Some(n)
        } else {
            None
        }
    }

    fn parse_atom(&mut self) -> Result<RegexNode, String> {
        match self.peek() {
            None => Err("unexpected end of regex".into()),
            Some(b'(') => self.parse_group(),
            Some(b'[') => self.parse_char_class(),
            Some(b'^') => {
                self.advance();
                Ok(RegexNode::StartAnchor)
            }
            Some(b'$') => {
                self.advance();
                Ok(RegexNode::EndAnchor)
            }
            Some(b'.') => {
                self.advance();
                Ok(RegexNode::AnyChar)
            }
            Some(b'\\') => {
                self.advance();
                self.parse_escape(false)
            }
            Some(b'*') | Some(b'+') | Some(b'?') => {
                Err("quantifier does not follow a repeatable item".into())
            }
            Some(ch) => {
                if self.flags.utf8 && ch >= 0x80 {
                    let cp = self.read_utf8_cp();
                    Ok(RegexNode::LiteralCp(cp))
                } else {
                    self.advance();
                    Ok(RegexNode::Literal(ch))
                }
            }
        }
    }

    fn parse_escape(&mut self, in_class: bool) -> Result<RegexNode, String> {
        match self.advance() {
            None => Err("unexpected end of escape sequence".into()),
            Some(b'd') => Ok(if self.flags.utf8 {
                RegexNode::CharClass {
                    ranges: vec![],
                    classes: vec![UnicodeClassKind::UniDigit],
                    anti_classes: vec![],
                    negated: false,
                }
            } else {
                RegexNode::CharClass {
                    ranges: vec![CharRange::Range(b'0' as u32, b'9' as u32)],
                    classes: vec![],
                    anti_classes: vec![],
                    negated: false,
                }
            }),
            Some(b'D') => Ok(if self.flags.utf8 {
                RegexNode::CharClass {
                    ranges: vec![],
                    classes: vec![UnicodeClassKind::UniDigit],
                    anti_classes: vec![],
                    negated: true,
                }
            } else {
                RegexNode::CharClass {
                    ranges: vec![CharRange::Range(b'0' as u32, b'9' as u32)],
                    classes: vec![],
                    anti_classes: vec![],
                    negated: true,
                }
            }),
            Some(b'w') => Ok(if self.flags.utf8 {
                RegexNode::CharClass {
                    ranges: vec![],
                    classes: vec![UnicodeClassKind::UniWord],
                    anti_classes: vec![],
                    negated: false,
                }
            } else {
                RegexNode::CharClass {
                    ranges: vec![
                        CharRange::Range(b'a' as u32, b'z' as u32),
                        CharRange::Range(b'A' as u32, b'Z' as u32),
                        CharRange::Range(b'0' as u32, b'9' as u32),
                        CharRange::Single(b'_' as u32),
                    ],
                    classes: vec![],
                    anti_classes: vec![],
                    negated: false,
                }
            }),
            Some(b'W') => Ok(if self.flags.utf8 {
                RegexNode::CharClass {
                    ranges: vec![],
                    classes: vec![UnicodeClassKind::UniWord],
                    anti_classes: vec![],
                    negated: true,
                }
            } else {
                RegexNode::CharClass {
                    ranges: vec![
                        CharRange::Range(b'a' as u32, b'z' as u32),
                        CharRange::Range(b'A' as u32, b'Z' as u32),
                        CharRange::Range(b'0' as u32, b'9' as u32),
                        CharRange::Single(b'_' as u32),
                    ],
                    classes: vec![],
                    anti_classes: vec![],
                    negated: true,
                }
            }),
            Some(b's') => Ok(if self.flags.utf8 {
                RegexNode::CharClass {
                    ranges: vec![],
                    classes: vec![UnicodeClassKind::UniSpace],
                    anti_classes: vec![],
                    negated: false,
                }
            } else {
                RegexNode::CharClass {
                    ranges: vec![
                        CharRange::Single(b' ' as u32),
                        CharRange::Single(b'\t' as u32),
                        CharRange::Single(b'\n' as u32),
                        CharRange::Single(b'\r' as u32),
                        CharRange::Single(0x0C), // form feed
                        CharRange::Single(0x0B), // vertical tab
                    ],
                    classes: vec![],
                    anti_classes: vec![],
                    negated: false,
                }
            }),
            Some(b'S') => Ok(if self.flags.utf8 {
                RegexNode::CharClass {
                    ranges: vec![],
                    classes: vec![UnicodeClassKind::UniSpace],
                    anti_classes: vec![],
                    negated: true,
                }
            } else {
                RegexNode::CharClass {
                    ranges: vec![
                        CharRange::Single(b' ' as u32),
                        CharRange::Single(b'\t' as u32),
                        CharRange::Single(b'\n' as u32),
                        CharRange::Single(b'\r' as u32),
                        CharRange::Single(0x0C),
                        CharRange::Single(0x0B),
                    ],
                    classes: vec![],
                    anti_classes: vec![],
                    negated: true,
                }
            }),
            Some(b'b') => {
                if in_class {
                    // \b inside character class is backspace (0x08)
                    Ok(RegexNode::Literal(0x08))
                } else {
                    Ok(RegexNode::WordBoundary)
                }
            }
            Some(b'B') => {
                if in_class {
                    Ok(RegexNode::Literal(b'B'))
                } else {
                    Ok(RegexNode::NonWordBoundary)
                }
            }
            Some(b'A') if !in_class => Ok(RegexNode::StartAnchor), // \A = start of subject
            Some(b'Z') if !in_class => Ok(RegexNode::EndAnchor),   // \Z = end of subject (or before final \n)
            Some(b'z') if !in_class => Ok(RegexNode::EndAnchor),   // \z = absolute end of subject
            Some(b'K') if !in_class => Ok(RegexNode::ResetMatchStart),
            Some(b'p') => self.parse_unicode_prop(false),
            Some(b'P') if !in_class => self.parse_unicode_prop(true),
            Some(b'P') => self.parse_unicode_prop(true),
            Some(b'n') => Ok(RegexNode::Literal(b'\n')),
            Some(b'r') => Ok(RegexNode::Literal(b'\r')),
            Some(b't') => Ok(RegexNode::Literal(b'\t')),
            Some(b'a') => Ok(RegexNode::Literal(0x07)), // bell
            Some(b'e') => Ok(RegexNode::Literal(0x1B)), // escape
            Some(b'f') => Ok(RegexNode::Literal(0x0C)), // form feed
            Some(b'x') => {
                // \xHH - hex escape
                let mut val: u8 = 0;
                let has_brace = self.peek() == Some(b'{');
                if has_brace {
                    self.advance();
                }
                for _ in 0..2 {
                    match self.peek() {
                        Some(c) if c.is_ascii_hexdigit() => {
                            self.advance();
                            val = val * 16 + hex_val(c);
                        }
                        _ => break,
                    }
                }
                if has_brace && self.peek() == Some(b'}') {
                    self.advance();
                }
                Ok(RegexNode::Literal(val))
            }
            Some(b'0') => {
                // Octal escape \0, \0nn
                let mut val: u8 = 0;
                for _ in 0..2 {
                    match self.peek() {
                        Some(c) if c >= b'0' && c <= b'7' => {
                            self.advance();
                            val = val * 8 + (c - b'0');
                        }
                        _ => break,
                    }
                }
                Ok(RegexNode::Literal(val))
            }
            Some(ch) if ch >= b'1' && ch <= b'9' && !in_class => {
                // Backreference \1 through \9
                let group_num = (ch - b'0') as usize;
                Ok(RegexNode::Backreference(group_num))
            }
            Some(ch) => {
                // Escaped literal (covers `\.`, `\\`, `\/`, `\(`, etc.)
                Ok(RegexNode::Literal(ch))
            }
        }
    }

    /// Parse `\p{Property}` or `\pL` (single-letter form).
    /// If `negated` is true, this was `\P{...}`.
    fn parse_unicode_prop(&mut self, negated: bool) -> Result<RegexNode, String> {
        let name: Vec<u8> = if self.peek() == Some(b'{') {
            self.advance();
            let mut name = Vec::new();
            while let Some(ch) = self.peek() {
                if ch == b'}' {
                    self.advance();
                    break;
                }
                name.push(ch);
                self.advance();
            }
            name
        } else if let Some(ch) = self.advance() {
            vec![ch]
        } else {
            return Err("malformed property escape".into());
        };

        // The name may start with ^ for negation
        let (inv, name) = if let Some(&b'^') = name.first() {
            (true, name[1..].to_vec())
        } else {
            (false, name)
        };

        let kind = match name.as_slice() {
            b"L" => UnicodeClassKind::Letter,
            b"M" => UnicodeClassKind::Mark,
            b"N" => UnicodeClassKind::Number,
            b"Nd" => UnicodeClassKind::NumberDecimal,
            b"P" => UnicodeClassKind::Punctuation,
            b"S" => UnicodeClassKind::Symbol,
            b"Z" => UnicodeClassKind::Separator,
            b"C" => UnicodeClassKind::Other,
            b"Ll" => UnicodeClassKind::LowercaseLetter,
            b"Lu" => UnicodeClassKind::UppercaseLetter,
            b"Lt" => UnicodeClassKind::TitlecaseLetter,
            b"Lm" => UnicodeClassKind::ModifierLetter,
            b"Lo" => UnicodeClassKind::OtherLetter,
            b"Cyrillic" => UnicodeClassKind::ScriptCyrillic,
            b"Greek" => UnicodeClassKind::ScriptGreek,
            b"Latin" => UnicodeClassKind::ScriptLatin,
            b"Han" => UnicodeClassKind::ScriptHan,
            b"Arabic" => UnicodeClassKind::ScriptArabic,
            b"Hebrew" => UnicodeClassKind::ScriptHebrew,
            b"Hiragana" => UnicodeClassKind::ScriptHiragana,
            b"Katakana" => UnicodeClassKind::ScriptKatakana,
            _ => {
                return Err(format!(
                    "Unknown property name after \\p",
                ));
            }
        };

        let final_negated = negated ^ inv;
        Ok(RegexNode::CharClass {
            ranges: vec![],
            classes: vec![kind],
            anti_classes: vec![],
            negated: final_negated,
        })
    }

    fn parse_group(&mut self) -> Result<RegexNode, String> {
        self.advance(); // skip `(`

        // Check for special group types
        if self.peek() == Some(b'?') {
            self.advance();
            match self.peek() {
                Some(b':') => {
                    self.advance();
                    let inner = self.parse_alternation()?;
                    if self.peek() != Some(b')') {
                        return Err("unclosed group".into());
                    }
                    self.advance();
                    return Ok(RegexNode::NonCapturingGroup {
                        node: Box::new(inner),
                    });
                }
                Some(b'\'') => {
                    // (?'name'...) — named capture group with single-quote syntax
                    self.advance();
                    let mut name = Vec::new();
                    while let Some(ch) = self.peek() {
                        if ch == b'\'' {
                            self.advance();
                            break;
                        }
                        name.push(ch);
                        self.advance();
                    }
                    self.group_count += 1;
                    let index = self.group_count;
                    if !name.is_empty() {
                        self.group_names.push((index, name));
                    }
                    let inner = self.parse_alternation()?;
                    if self.peek() != Some(b')') {
                        return Err("unclosed group".into());
                    }
                    self.advance();
                    return Ok(RegexNode::Group {
                        index,
                        node: Box::new(inner),
                    });
                }
                Some(b'=') => {
                    self.advance();
                    let inner = self.parse_alternation()?;
                    if self.peek() != Some(b')') {
                        return Err("unclosed lookahead".into());
                    }
                    self.advance();
                    return Ok(RegexNode::Lookahead {
                        node: Box::new(inner),
                        positive: true,
                    });
                }
                Some(b'!') => {
                    self.advance();
                    let inner = self.parse_alternation()?;
                    if self.peek() != Some(b')') {
                        return Err("unclosed lookahead".into());
                    }
                    self.advance();
                    return Ok(RegexNode::Lookahead {
                        node: Box::new(inner),
                        positive: false,
                    });
                }
                Some(b'<') => {
                    self.advance();
                    match self.peek() {
                        Some(b'=') => {
                            self.advance();
                            let inner = self.parse_alternation()?;
                            if self.peek() != Some(b')') {
                                return Err("unclosed lookbehind".into());
                            }
                            self.advance();
                            return Ok(RegexNode::Lookbehind {
                                node: Box::new(inner),
                                positive: true,
                            });
                        }
                        Some(b'!') => {
                            self.advance();
                            let inner = self.parse_alternation()?;
                            if self.peek() != Some(b')') {
                                return Err("unclosed lookbehind".into());
                            }
                            self.advance();
                            return Ok(RegexNode::Lookbehind {
                                node: Box::new(inner),
                                positive: false,
                            });
                        }
                        _ => {
                            // (?<name>...) — named capture group
                            // Validate name doesn't start with a digit
                            if let Some(first) = self.peek() {
                                if first.is_ascii_digit() {
                                    return Err("subpattern name must start with a non-digit".into());
                                }
                            }
                            let mut name = Vec::new();
                            while let Some(ch) = self.peek() {
                                if ch == b'>' {
                                    self.advance();
                                    break;
                                }
                                name.push(ch);
                                self.advance();
                            }
                            self.group_count += 1;
                            let index = self.group_count;
                            if !name.is_empty() {
                                self.group_names.push((index, name));
                            }
                            let inner = self.parse_alternation()?;
                            if self.peek() != Some(b')') {
                                return Err("unclosed group".into());
                            }
                            self.advance();
                            return Ok(RegexNode::Group {
                                index,
                                node: Box::new(inner),
                            });
                        }
                    }
                }
                Some(b'P') => {
                    self.advance();
                    // (?P<name>...) or (?P=name) — named capture/backreference
                    if self.peek() == Some(b'<') {
                        self.advance();
                        // Validate the name doesn't start with a digit
                        if let Some(first) = self.peek() {
                            if first.is_ascii_digit() {
                                return Err("subpattern name must start with a non-digit".into());
                            }
                        }
                        // Capture the name
                        let mut name = Vec::new();
                        while let Some(ch) = self.peek() {
                            if ch == b'>' {
                                self.advance();
                                break;
                            }
                            name.push(ch);
                            self.advance();
                        }
                        self.group_count += 1;
                        let index = self.group_count;
                        if !name.is_empty() {
                            self.group_names.push((index, name));
                        }
                        let inner = self.parse_alternation()?;
                        if self.peek() != Some(b')') {
                            return Err("unclosed group".into());
                        }
                        self.advance();
                        return Ok(RegexNode::Group {
                            index,
                            node: Box::new(inner),
                        });
                    } else {
                        // (?P=name) — treat as non-capturing for now
                        while let Some(ch) = self.peek() {
                            if ch == b')' {
                                break;
                            }
                            self.advance();
                        }
                        if self.peek() == Some(b')') {
                            self.advance();
                        }
                        return Ok(RegexNode::Sequence(vec![]));
                    }
                }
                _ => {
                    // (?imsxU...) inline flags — apply and treat as non-capturing group
                    // or skip unknown group types
                    while let Some(ch) = self.peek() {
                        match ch {
                            b'i' => {
                                self.flags.case_insensitive = true;
                                self.advance();
                            }
                            b'm' => {
                                self.flags.multiline = true;
                                self.advance();
                            }
                            b's' => {
                                self.flags.dotall = true;
                                self.advance();
                            }
                            b'x' => {
                                self.flags.extended = true;
                                self.advance();
                            }
                            b'U' => {
                                self.flags.ungreedy = true;
                                self.advance();
                            }
                            b')' => {
                                self.advance();
                                // Inline flags with no subpattern — affects rest of pattern
                                return Ok(RegexNode::Sequence(vec![]));
                            }
                            b':' => {
                                self.advance();
                                let inner = self.parse_alternation()?;
                                if self.peek() != Some(b')') {
                                    return Err("unclosed group".into());
                                }
                                self.advance();
                                return Ok(RegexNode::NonCapturingGroup {
                                    node: Box::new(inner),
                                });
                            }
                            _ => {
                                // Skip unknown
                                self.advance();
                            }
                        }
                    }
                    return Ok(RegexNode::Sequence(vec![]));
                }
            }
        }

        // Normal capture group — but if no_auto_capture is set, treat as non-capturing
        if self.flags.no_auto_capture {
            let inner = self.parse_alternation()?;
            if self.peek() != Some(b')') {
                return Err("unclosed group".into());
            }
            self.advance();
            return Ok(RegexNode::NonCapturingGroup {
                node: Box::new(inner),
            });
        }
        self.group_count += 1;
        let index = self.group_count;
        let inner = self.parse_alternation()?;
        if self.peek() != Some(b')') {
            return Err("unclosed group".into());
        }
        self.advance();
        Ok(RegexNode::Group {
            index,
            node: Box::new(inner),
        })
    }

    fn parse_char_class(&mut self) -> Result<RegexNode, String> {
        self.advance(); // skip `[`
        let negated = if self.peek() == Some(b'^') {
            self.advance();
            true
        } else {
            false
        };

        let mut ranges: Vec<CharRange> = Vec::new();
        let mut classes: Vec<UnicodeClassKind> = Vec::new();
        // Track negated classes — we include their complement by adding every char range
        // OR handle via the top-level match function. We'll collect "anti-classes" separately.
        let mut anti_classes: Vec<UnicodeClassKind> = Vec::new();

        // Handle `]` or `-` as first character (literal)
        if self.peek() == Some(b']') {
            self.advance();
            ranges.push(CharRange::Single(b']' as u32));
        }

        while let Some(ch) = self.peek() {
            if ch == b']' {
                self.advance();
                // Anti-classes: we need a way to represent "char not of class X" inside a positive class.
                // Implement this by making the class use the negation strategy:
                // If there are only anti_classes and no positive ones, then we can flip negated.
                // Otherwise store them as negated sub-classes via a simple encoding: we'll use
                // paired classes (anti) with a marker. For now, use a dedicated encoding: push
                // each anti class as a CharClass wrapped into a special literal range structure —
                // easier approach: store anti_classes alongside classes and check on match.
                return Ok(RegexNode::CharClass { ranges, classes, anti_classes, negated });
            }

            if ch == b'[' && self.peek_ahead(1) == Some(b':') {
                // POSIX class like [:alpha:]
                let saved = self.pos;
                self.advance(); // [
                self.advance(); // :
                let mut name = Vec::new();
                let mut inv = false;
                if self.peek() == Some(b'^') {
                    inv = true;
                    self.advance();
                }
                while let Some(c) = self.peek() {
                    if c == b':' {
                        break;
                    }
                    name.push(c);
                    self.advance();
                }
                if self.peek() == Some(b':') && self.peek_ahead(1) == Some(b']') {
                    self.advance(); // :
                    self.advance(); // ]
                    let kind = match name.as_slice() {
                        b"alpha" => UnicodeClassKind::PosixAlpha,
                        b"upper" => UnicodeClassKind::PosixUpper,
                        b"lower" => UnicodeClassKind::PosixLower,
                        b"digit" => UnicodeClassKind::PosixDigit,
                        b"xdigit" => UnicodeClassKind::PosixXDigit,
                        b"alnum" => UnicodeClassKind::PosixAlnum,
                        b"space" => UnicodeClassKind::PosixSpace,
                        b"blank" => UnicodeClassKind::PosixBlank,
                        b"cntrl" => UnicodeClassKind::PosixCntrl,
                        b"graph" => UnicodeClassKind::PosixGraph,
                        b"print" => UnicodeClassKind::PosixPrint,
                        b"punct" => UnicodeClassKind::PosixPunct,
                        b"word" => UnicodeClassKind::PosixWord,
                        b"ascii" => UnicodeClassKind::PosixAscii,
                        _ => {
                            // Unknown POSIX class — treat as literal `[`
                            self.pos = saved;
                            ranges.push(CharRange::Single(b'[' as u32));
                            self.advance();
                            continue;
                        }
                    };
                    if inv {
                        anti_classes.push(kind);
                    } else {
                        classes.push(kind);
                    }
                    continue;
                } else {
                    // Not a POSIX class
                    self.pos = saved;
                    self.advance();
                    ranges.push(CharRange::Single(b'[' as u32));
                    continue;
                }
            }

            if ch == b'\\' {
                self.advance();
                match self.peek() {
                    Some(b'd') => {
                        self.advance();
                        if self.flags.utf8 {
                            classes.push(UnicodeClassKind::UniDigit);
                        } else {
                            ranges.push(CharRange::Range(b'0' as u32, b'9' as u32));
                        }
                    }
                    Some(b'D') => {
                        self.advance();
                        if self.flags.utf8 {
                            anti_classes.push(UnicodeClassKind::UniDigit);
                        } else {
                            ranges.push(CharRange::Range(0, b'0' as u32 - 1));
                            ranges.push(CharRange::Range(b'9' as u32 + 1, 0x10FFFF));
                        }
                    }
                    Some(b'w') => {
                        self.advance();
                        if self.flags.utf8 {
                            classes.push(UnicodeClassKind::UniWord);
                        } else {
                            ranges.push(CharRange::Range(b'a' as u32, b'z' as u32));
                            ranges.push(CharRange::Range(b'A' as u32, b'Z' as u32));
                            ranges.push(CharRange::Range(b'0' as u32, b'9' as u32));
                            ranges.push(CharRange::Single(b'_' as u32));
                        }
                    }
                    Some(b'W') => {
                        self.advance();
                        if self.flags.utf8 {
                            anti_classes.push(UnicodeClassKind::UniWord);
                        } else {
                            ranges.push(CharRange::Range(0, b'/' as u32));  // before 0
                            ranges.push(CharRange::Range(b':' as u32, b'@' as u32));
                            ranges.push(CharRange::Range(b'[' as u32, b'^' as u32));
                            ranges.push(CharRange::Single(b'`' as u32));
                            ranges.push(CharRange::Range(b'{' as u32, 0x10FFFF));
                        }
                    }
                    Some(b's') => {
                        self.advance();
                        if self.flags.utf8 {
                            classes.push(UnicodeClassKind::UniSpace);
                        } else {
                            ranges.push(CharRange::Single(b' ' as u32));
                            ranges.push(CharRange::Single(b'\t' as u32));
                            ranges.push(CharRange::Single(b'\n' as u32));
                            ranges.push(CharRange::Single(b'\r' as u32));
                            ranges.push(CharRange::Single(0x0C));
                            ranges.push(CharRange::Single(0x0B));
                        }
                    }
                    Some(b'S') => {
                        self.advance();
                        if self.flags.utf8 {
                            anti_classes.push(UnicodeClassKind::UniSpace);
                        } else {
                            ranges.push(CharRange::Range(0, 0x08));
                            ranges.push(CharRange::Single(0x0E));
                            ranges.push(CharRange::Range(0x0E, b' ' as u32 - 1));
                            ranges.push(CharRange::Range(b' ' as u32 + 1, 0x10FFFF));
                        }
                    }
                    Some(b'n') => {
                        self.advance();
                        ranges.push(CharRange::Single(b'\n' as u32));
                    }
                    Some(b'r') => {
                        self.advance();
                        ranges.push(CharRange::Single(b'\r' as u32));
                    }
                    Some(b't') => {
                        self.advance();
                        ranges.push(CharRange::Single(b'\t' as u32));
                    }
                    Some(b'b') => {
                        // \b in character class = backspace
                        self.advance();
                        ranges.push(CharRange::Single(0x08));
                    }
                    Some(b'x') => {
                        self.advance();
                        let mut val: u32 = 0;
                        let has_brace = self.peek() == Some(b'{');
                        if has_brace {
                            self.advance();
                            while let Some(c) = self.peek() {
                                if c.is_ascii_hexdigit() {
                                    self.advance();
                                    val = val.saturating_mul(16) + hex_val(c) as u32;
                                } else {
                                    break;
                                }
                            }
                            if self.peek() == Some(b'}') {
                                self.advance();
                            }
                        } else {
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(c) if c.is_ascii_hexdigit() => {
                                        self.advance();
                                        val = val * 16 + hex_val(c) as u32;
                                    }
                                    _ => break,
                                }
                            }
                        }
                        ranges.push(CharRange::Single(val));
                    }
                    Some(b'p') => {
                        self.advance();
                        let node = self.parse_unicode_prop(false)?;
                        if let RegexNode::CharClass { classes: mut sub, negated: n, .. } = node {
                            if n {
                                anti_classes.append(&mut sub);
                            } else {
                                classes.append(&mut sub);
                            }
                        }
                    }
                    Some(b'P') => {
                        self.advance();
                        let node = self.parse_unicode_prop(true)?;
                        if let RegexNode::CharClass { classes: mut sub, negated: n, .. } = node {
                            if n {
                                anti_classes.append(&mut sub);
                            } else {
                                classes.append(&mut sub);
                            }
                        }
                    }
                    Some(c) => {
                        self.advance();
                        ranges.push(CharRange::Single(c as u32));
                    }
                    None => {
                        return Err("unexpected end in char class escape".into());
                    }
                }
            } else if ch == b'-' && !ranges.is_empty() && self.peek_ahead(1) != Some(b']') {
                // Could be a range like `a-z`
                self.advance();
                if let Some(end_ch) = self.peek() {
                    // Read the end char (possibly escaped or UTF-8)
                    let end_cp: u32 = if end_ch == b'\\' {
                        self.advance();
                        match self.advance() {
                            Some(b'n') => b'\n' as u32,
                            Some(b'r') => b'\r' as u32,
                            Some(b't') => b'\t' as u32,
                            Some(b'x') => {
                                let mut val: u32 = 0;
                                let has_brace = self.peek() == Some(b'{');
                                if has_brace {
                                    self.advance();
                                    while let Some(c) = self.peek() {
                                        if c.is_ascii_hexdigit() {
                                            self.advance();
                                            val = val.saturating_mul(16) + hex_val(c) as u32;
                                        } else { break; }
                                    }
                                    if self.peek() == Some(b'}') { self.advance(); }
                                } else {
                                    for _ in 0..2 {
                                        if let Some(c) = self.peek() {
                                            if c.is_ascii_hexdigit() {
                                                self.advance();
                                                val = val * 16 + hex_val(c) as u32;
                                            } else { break; }
                                        } else { break; }
                                    }
                                }
                                val
                            }
                            Some(c) => c as u32,
                            None => end_ch as u32,
                        }
                    } else if self.flags.utf8 && end_ch >= 0x80 {
                        self.read_utf8_cp()
                    } else {
                        self.advance();
                        end_ch as u32
                    };
                    // Get the start of the range from the last entry
                    if let Some(CharRange::Single(start)) = ranges.last() {
                        let start = *start;
                        ranges.pop();
                        ranges.push(CharRange::Range(start, end_cp));
                    } else {
                        ranges.push(CharRange::Single(b'-' as u32));
                        ranges.push(CharRange::Single(end_cp));
                    }
                } else {
                    ranges.push(CharRange::Single(b'-' as u32));
                }
            } else if self.flags.utf8 && ch >= 0x80 {
                let cp = self.read_utf8_cp();
                ranges.push(CharRange::Single(cp));
            } else {
                self.advance();
                ranges.push(CharRange::Single(ch as u32));
            }
        }

        Err("unclosed character class".into())
    }

    fn peek_ahead(&self, n: usize) -> Option<u8> {
        self.input.get(self.pos + n).copied()
    }

    /// Decode a UTF-8 codepoint at current position, advancing past it.
    /// Returns the codepoint, or the byte value if decoding fails.
    fn read_utf8_cp(&mut self) -> u32 {
        let (cp, len) = decode_utf8_at(self.input, self.pos);
        self.pos += len.max(1);
        cp
    }
}

fn hex_val(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

// ============================================================================
// Matcher
// ============================================================================

/// Match state tracking capture groups
#[derive(Clone)]
struct MatchState<'a> {
    input: &'a [u8],
    captures: Vec<Option<(usize, usize)>>, // (start, end) for each group
    flags: RegexFlags,
    step_count: usize,
    /// If `\K` was encountered during the match, this holds the position where
    /// the reported match should start. None means start from initial position.
    match_start_override: Option<usize>,
}

const MAX_STEPS: usize = 1_000_000;

impl<'a> MatchState<'a> {
    fn new(input: &'a [u8], num_groups: usize, flags: RegexFlags) -> Self {
        Self {
            input,
            captures: vec![None; num_groups + 1], // index 0 = full match
            flags,
            step_count: 0,
            match_start_override: None,
        }
    }

    /// Try to match `node` starting at position `pos`. Returns the end position if successful.
    fn try_match(&mut self, node: &RegexNode, pos: usize) -> Option<usize> {
        self.step_count += 1;
        if self.step_count > MAX_STEPS {
            return None;
        }

        match node {
            RegexNode::Literal(ch) => {
                if pos < self.input.len() {
                    let input_ch = self.input[pos];
                    if self.flags.case_insensitive {
                        if input_ch.to_ascii_lowercase() == ch.to_ascii_lowercase() {
                            Some(pos + 1)
                        } else {
                            None
                        }
                    } else if input_ch == *ch {
                        Some(pos + 1)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }

            RegexNode::LiteralCp(cp) => {
                if pos < self.input.len() {
                    let (input_cp, len) = decode_utf8_at(self.input, pos);
                    let matches = if self.flags.case_insensitive {
                        ascii_lc_cp(input_cp) == ascii_lc_cp(*cp)
                    } else {
                        input_cp == *cp
                    };
                    if matches {
                        Some(pos + len.max(1))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }

            RegexNode::AnyChar => {
                if pos < self.input.len() {
                    if self.flags.utf8 {
                        // Check for newline
                        let (cp, len) = decode_utf8_at(self.input, pos);
                        if !self.flags.dotall && cp == b'\n' as u32 {
                            None
                        } else {
                            Some(pos + len.max(1))
                        }
                    } else if self.flags.dotall || self.input[pos] != b'\n' {
                        Some(pos + 1)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }

            RegexNode::StartAnchor => {
                if pos == 0 {
                    Some(pos)
                } else if self.flags.multiline && pos > 0 && self.input[pos - 1] == b'\n' {
                    Some(pos)
                } else {
                    None
                }
            }

            RegexNode::EndAnchor => {
                if pos == self.input.len() {
                    Some(pos)
                } else if self.flags.multiline && pos < self.input.len() && self.input[pos] == b'\n'
                {
                    Some(pos)
                } else if !self.flags.dollar_end_only && pos == self.input.len() - 1 && self.input[pos] == b'\n' {
                    // $ matches before trailing newline (unless /D modifier)
                    Some(pos)
                } else {
                    None
                }
            }

            RegexNode::ResetMatchStart => {
                self.match_start_override = Some(pos);
                Some(pos)
            }

            RegexNode::WordBoundary => {
                let (before_word, after_word) = self.word_boundary_states(pos);
                if before_word != after_word {
                    Some(pos)
                } else {
                    None
                }
            }

            RegexNode::NonWordBoundary => {
                let (before_word, after_word) = self.word_boundary_states(pos);
                if before_word == after_word {
                    Some(pos)
                } else {
                    None
                }
            }

            RegexNode::CharClass { ranges, classes, anti_classes, negated } => {
                if pos < self.input.len() {
                    let (cp, len) = if self.flags.utf8 {
                        decode_utf8_at(self.input, pos)
                    } else {
                        (self.input[pos] as u32, 1)
                    };
                    let matches = char_matches_class(cp, ranges, classes, anti_classes, self.flags.case_insensitive);
                    if matches != *negated {
                        Some(pos + len.max(1))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }

            RegexNode::Sequence(nodes) => {
                let mut current_pos = pos;
                for n in nodes {
                    match self.try_match(n, current_pos) {
                        Some(new_pos) => current_pos = new_pos,
                        None => return None,
                    }
                }
                Some(current_pos)
            }

            RegexNode::Alternation(branches) => {
                for branch in branches {
                    let mut state = self.clone();
                    if let Some(end_pos) = state.try_match(branch, pos) {
                        // Propagate captures from successful branch
                        self.captures = state.captures;
                        self.match_start_override = state.match_start_override;
                        self.step_count = state.step_count;
                        return Some(end_pos);
                    }
                    self.step_count = state.step_count;
                }
                None
            }

            RegexNode::Group { index, node } => {
                let old_capture = self.captures.get(*index).cloned().flatten();
                let result = self.try_match(node, pos);
                if let Some(end_pos) = result {
                    if *index < self.captures.len() {
                        self.captures[*index] = Some((pos, end_pos));
                    }
                    Some(end_pos)
                } else {
                    // Restore old capture on failure
                    if *index < self.captures.len() {
                        self.captures[*index] = old_capture;
                    }
                    None
                }
            }

            RegexNode::NonCapturingGroup { node } => self.try_match(node, pos),

            RegexNode::Lookahead { node, positive } => {
                let mut state = self.clone();
                let matched = state.try_match(node, pos).is_some();
                self.step_count = state.step_count;
                if matched == *positive {
                    Some(pos) // Lookahead doesn't consume input
                } else {
                    None
                }
            }

            RegexNode::Lookbehind { node, positive } => {
                // Simple lookbehind: try matching from various start positions before `pos`
                // This is a simplification — real PCRE requires fixed-length lookbehinds
                let max_lookback = pos.min(256); // limit lookback length
                let mut found = false;
                for start in (pos.saturating_sub(max_lookback)..=pos).rev() {
                    let mut state = self.clone();
                    if let Some(end) = state.try_match(node, start) {
                        if end == pos {
                            found = true;
                            self.step_count = state.step_count;
                            break;
                        }
                    }
                    self.step_count += 1;
                    if self.step_count > MAX_STEPS {
                        return None;
                    }
                }
                if found == *positive {
                    Some(pos)
                } else {
                    None
                }
            }

            RegexNode::Backreference(group_num) => {
                if let Some(Some((start, end))) = self.captures.get(*group_num) {
                    let captured = &self.input[*start..*end];
                    let len = captured.len();
                    if pos + len <= self.input.len() {
                        let slice = &self.input[pos..pos + len];
                        let matches = if self.flags.case_insensitive {
                            slice
                                .iter()
                                .zip(captured.iter())
                                .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
                        } else {
                            slice == captured
                        };
                        if matches {
                            Some(pos + len)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    // Group not yet captured — match empty string (PCRE behavior)
                    Some(pos)
                }
            }

            RegexNode::Quantifier {
                node,
                min,
                max,
                greedy,
            } => {
                if *greedy {
                    self.match_quantifier_greedy(node, pos, *min, *max)
                } else {
                    self.match_quantifier_lazy(node, pos, *min, *max)
                }
            }
        }
    }

    fn match_quantifier_greedy(
        &mut self,
        node: &RegexNode,
        pos: usize,
        min: usize,
        max: Option<usize>,
    ) -> Option<usize> {
        // First, match minimum required times
        let mut current_pos = pos;
        let mut match_positions = vec![pos];

        let effective_max = max.unwrap_or(usize::MAX);

        for _i in 0..effective_max {
            if self.step_count > MAX_STEPS {
                return None;
            }
            let mut state = self.clone();
            match state.try_match(node, current_pos) {
                Some(new_pos) => {
                    self.step_count = state.step_count;
                    // Guard against zero-length matches causing infinite loop
                    if new_pos == current_pos && match_positions.len() > min {
                        break;
                    }
                    self.captures = state.captures;
                    current_pos = new_pos;
                    match_positions.push(current_pos);
                }
                None => {
                    self.step_count = state.step_count;
                    break;
                }
            }
        }

        // Check we got enough matches
        if match_positions.len() - 1 < min {
            return None;
        }

        // Greedy: return the maximum position (already at the end)
        // The caller (sequence) will try to continue from here; if it fails,
        // the quantifier needs to backtrack. We handle this by returning the furthest position.
        // But since our sequence matching is simple, we need a different approach for backtracking.

        // For proper backtracking, we return positions from longest to shortest.
        // But our architecture doesn't support that directly. Instead, we just return the
        // maximum match. For cases that need backtracking (e.g., `.*foo`), we use a different
        // approach in the sequence matcher.

        // Actually, this function is only called as part of try_match which is called from
        // sequence. The sequence iterates nodes. Backtracking needs to happen at the sequence level.
        // Our current architecture doesn't support that, so we need to handle it here.

        // Return max position - the sequence continuation will determine if it's valid.
        // If the sequence continues and fails, the whole sequence fails.
        // For proper backtracking, we would need to be called with a "continuation" callback.

        Some(current_pos)
    }

    fn match_quantifier_lazy(
        &mut self,
        node: &RegexNode,
        pos: usize,
        min: usize,
        max: Option<usize>,
    ) -> Option<usize> {
        let mut current_pos = pos;
        let _effective_max = max.unwrap_or(usize::MAX);

        // Match minimum required times
        for _i in 0..min {
            if self.step_count > MAX_STEPS {
                return None;
            }
            match self.try_match(node, current_pos) {
                Some(new_pos) => {
                    current_pos = new_pos;
                }
                None => return None,
            }
        }

        // Lazy: return the minimum position
        Some(current_pos)
    }
}

/// Decode a UTF-8 codepoint at `pos`. Returns (codepoint, byte_length).
/// If the bytes don't form valid UTF-8, returns the raw byte value with length 1.
fn decode_utf8_at(input: &[u8], pos: usize) -> (u32, usize) {
    if pos >= input.len() {
        return (0, 0);
    }
    let b0 = input[pos];
    if b0 < 0x80 {
        return (b0 as u32, 1);
    }
    // Determine byte length from leading bits
    let (len, mut cp): (usize, u32) = if b0 & 0xE0 == 0xC0 {
        (2, (b0 & 0x1F) as u32)
    } else if b0 & 0xF0 == 0xE0 {
        (3, (b0 & 0x0F) as u32)
    } else if b0 & 0xF8 == 0xF0 {
        (4, (b0 & 0x07) as u32)
    } else {
        return (b0 as u32, 1);
    };
    if pos + len > input.len() {
        return (b0 as u32, 1);
    }
    for i in 1..len {
        let b = input[pos + i];
        if b & 0xC0 != 0x80 {
            return (b0 as u32, 1);
        }
        cp = (cp << 6) | (b & 0x3F) as u32;
    }
    (cp, len)
}

/// ASCII-lowercase a codepoint (safe for Unicode: only affects A-Z).
fn ascii_lc_cp(cp: u32) -> u32 {
    if (b'A' as u32..=b'Z' as u32).contains(&cp) {
        cp + 32
    } else {
        cp
    }
}

/// ASCII-uppercase a codepoint.
fn ascii_uc_cp(cp: u32) -> u32 {
    if (b'a' as u32..=b'z' as u32).contains(&cp) {
        cp - 32
    } else {
        cp
    }
}

/// Check if a codepoint matches a set of ranges + classes, with anti_classes adding
/// (complement of X) to the positive set.
fn char_matches_class(
    cp: u32,
    ranges: &[CharRange],
    classes: &[UnicodeClassKind],
    anti_classes: &[UnicodeClassKind],
    case_insensitive: bool,
) -> bool {
    // Check ranges
    for range in ranges {
        match range {
            CharRange::Single(c) => {
                if case_insensitive {
                    if ascii_lc_cp(cp) == ascii_lc_cp(*c) {
                        return true;
                    }
                } else if cp == *c {
                    return true;
                }
            }
            CharRange::Range(start, end) => {
                if case_insensitive {
                    let cp_l = ascii_lc_cp(cp);
                    let s_l = ascii_lc_cp(*start);
                    let e_l = ascii_lc_cp(*end);
                    if cp_l >= s_l && cp_l <= e_l {
                        return true;
                    }
                    let cp_u = ascii_uc_cp(cp);
                    let s_u = ascii_uc_cp(*start);
                    let e_u = ascii_uc_cp(*end);
                    if cp_u >= s_u && cp_u <= e_u {
                        return true;
                    }
                } else if cp >= *start && cp <= *end {
                    return true;
                }
            }
        }
    }
    // Check classes (positive match)
    for k in classes {
        if check_class(cp, *k) {
            return true;
        }
    }
    // Check anti_classes (complement of X is in positive set)
    for k in anti_classes {
        if !check_class(cp, *k) {
            return true;
        }
    }
    false
}

fn check_class(cp: u32, kind: UnicodeClassKind) -> bool {
    use UnicodeClassKind::*;
    match kind {
        Letter | UniWord if cp < 0x80 => {
            if kind == UniWord {
                (cp >= b'a' as u32 && cp <= b'z' as u32)
                    || (cp >= b'A' as u32 && cp <= b'Z' as u32)
                    || (cp >= b'0' as u32 && cp <= b'9' as u32)
                    || cp == b'_' as u32
            } else {
                (cp >= b'a' as u32 && cp <= b'z' as u32)
                    || (cp >= b'A' as u32 && cp <= b'Z' as u32)
            }
        }
        UniWord => {
            // word char: letter, digit, mark, or connector punctuation (roughly _)
            is_unicode_letter(cp) || is_unicode_mark(cp) || is_unicode_decimal_digit(cp) || cp == b'_' as u32
        }
        UniDigit => is_unicode_decimal_digit(cp),
        UniSpace => is_unicode_space(cp),
        Letter => is_unicode_letter(cp),
        Mark => is_unicode_mark(cp),
        Number => is_unicode_number(cp),
        NumberDecimal => is_unicode_decimal_digit(cp),
        Punctuation => is_unicode_punctuation(cp),
        Symbol => is_unicode_symbol(cp),
        Separator => is_unicode_separator(cp),
        Other => is_unicode_other(cp),
        LowercaseLetter => is_unicode_lowercase_letter(cp),
        UppercaseLetter => is_unicode_uppercase_letter(cp),
        TitlecaseLetter => false, // rare, skip
        ModifierLetter => false,
        OtherLetter => is_unicode_letter(cp) && !is_unicode_lowercase_letter(cp) && !is_unicode_uppercase_letter(cp),
        ScriptCyrillic => is_cyrillic(cp),
        ScriptGreek => is_greek(cp),
        ScriptLatin => is_latin(cp),
        ScriptHan => is_han(cp),
        ScriptArabic => is_arabic(cp),
        ScriptHebrew => is_hebrew(cp),
        ScriptHiragana => is_hiragana(cp),
        ScriptKatakana => is_katakana(cp),
        PosixAlpha => (cp as u8).is_ascii_alphabetic() && cp < 0x80,
        PosixUpper => cp >= b'A' as u32 && cp <= b'Z' as u32,
        PosixLower => cp >= b'a' as u32 && cp <= b'z' as u32,
        PosixDigit => cp >= b'0' as u32 && cp <= b'9' as u32,
        PosixXDigit => (cp >= b'0' as u32 && cp <= b'9' as u32)
            || (cp >= b'a' as u32 && cp <= b'f' as u32)
            || (cp >= b'A' as u32 && cp <= b'F' as u32),
        PosixAlnum => cp < 0x80 && (cp as u8).is_ascii_alphanumeric(),
        PosixSpace => cp == b' ' as u32 || cp == b'\t' as u32 || cp == b'\n' as u32
            || cp == b'\r' as u32 || cp == 0x0B || cp == 0x0C,
        PosixBlank => cp == b' ' as u32 || cp == b'\t' as u32,
        PosixCntrl => cp < 0x20 || cp == 0x7F,
        PosixGraph => cp >= 0x21 && cp <= 0x7E,
        PosixPrint => cp >= 0x20 && cp <= 0x7E,
        PosixPunct => cp < 0x80 && (cp as u8).is_ascii_punctuation(),
        PosixWord => cp < 0x80 && ((cp as u8).is_ascii_alphanumeric() || cp == b'_' as u32),
        PosixAscii => cp < 0x80,
    }
}

/// Simplified Unicode letter check — covers commonly-used ranges.
fn is_unicode_letter(cp: u32) -> bool {
    match cp {
        // ASCII letters
        0x41..=0x5A | 0x61..=0x7A => true,
        // Latin-1 supplement letters
        0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0xFF => true,
        // Latin Extended-A/B, IPA Extensions
        0x100..=0x2AF => true,
        // Greek
        0x370..=0x373 | 0x376..=0x377 | 0x37A..=0x37D | 0x37F
            | 0x386 | 0x388..=0x38A | 0x38C | 0x38E..=0x3A1 | 0x3A3..=0x3FF => true,
        // Cyrillic
        0x400..=0x481 | 0x48A..=0x4FF | 0x500..=0x52F => true,
        // Armenian
        0x531..=0x556 | 0x561..=0x587 => true,
        // Hebrew letters
        0x5D0..=0x5EA | 0x5EF..=0x5F2 => true,
        // Arabic letters
        0x620..=0x64A | 0x66E..=0x66F | 0x671..=0x6D3 | 0x6D5 | 0x6E5..=0x6E6
            | 0x6EE..=0x6EF | 0x6FA..=0x6FC | 0x6FF => true,
        // Devanagari
        0x904..=0x939 => true,
        // Thai
        0xE01..=0xE30 | 0xE32..=0xE33 | 0xE40..=0xE46 => true,
        // Sinhala
        0xD85..=0xD96 | 0xD9A..=0xDB1 | 0xDB3..=0xDBB | 0xDBD | 0xDC0..=0xDC6 => true,
        // Hiragana
        0x3041..=0x3096 => true,
        // Katakana
        0x30A1..=0x30FA => true,
        // CJK
        0x4E00..=0x9FFF => true,
        0x3400..=0x4DBF => true,
        0x20000..=0x2A6DF => true,
        // Hangul
        0xAC00..=0xD7A3 => true,
        // Fullwidth / halfwidth Latin letters
        0xFF21..=0xFF3A | 0xFF41..=0xFF5A => true,
        _ => false,
    }
}

fn is_unicode_mark(cp: u32) -> bool {
    match cp {
        // Combining diacriticals
        0x300..=0x36F | 0x483..=0x487 | 0x591..=0x5BD | 0x5BF | 0x5C1..=0x5C2
            | 0x5C4..=0x5C5 | 0x5C7
            | 0x610..=0x61A | 0x64B..=0x65F | 0x670 | 0x6D6..=0x6DC
            | 0x6DF..=0x6E4 | 0x6E7..=0x6E8 | 0x6EA..=0x6ED => true,
        0x900..=0x903 | 0x93A..=0x93C | 0x93E..=0x94F | 0x951..=0x957
            | 0x962..=0x963 => true,
        0xDCA..=0xDDF | 0xDF2..=0xDF3 => true,
        0x20D0..=0x20FF => true,
        _ => false,
    }
}

fn is_unicode_number(cp: u32) -> bool {
    is_unicode_decimal_digit(cp)
        || matches!(cp, 0xB2..=0xB3 | 0xB9 | 0xBC..=0xBE | 0x2070..=0x2189)
}

fn is_unicode_decimal_digit(cp: u32) -> bool {
    match cp {
        0x30..=0x39 => true, // ASCII digits
        0x660..=0x669 => true, // Arabic-Indic
        0x6F0..=0x6F9 => true, // Extended Arabic-Indic
        0x7C0..=0x7C9 => true, // NKo
        0x966..=0x96F => true, // Devanagari
        0x9E6..=0x9EF => true, // Bengali
        0xA66..=0xA6F => true, // Gurmukhi
        0xAE6..=0xAEF => true, // Gujarati
        0xB66..=0xB6F => true, // Oriya
        0xBE6..=0xBEF => true, // Tamil
        0xC66..=0xC6F => true, // Telugu
        0xCE6..=0xCEF => true, // Kannada
        0xD66..=0xD6F => true, // Malayalam
        0xE50..=0xE59 => true, // Thai
        0xED0..=0xED9 => true, // Lao
        0xF20..=0xF29 => true, // Tibetan
        0xFF10..=0xFF19 => true, // Fullwidth
        _ => false,
    }
}

fn is_unicode_space(cp: u32) -> bool {
    match cp {
        0x9..=0xD => true,
        0x20 => true,
        0x85 => true,
        0xA0 => true,
        0x1680 => true,
        0x2000..=0x200A => true,
        0x2028..=0x2029 => true,
        0x202F => true,
        0x205F => true,
        0x3000 => true,
        _ => false,
    }
}

fn is_unicode_punctuation(cp: u32) -> bool {
    match cp {
        0x21..=0x23 | 0x25..=0x2A | 0x2C..=0x2F | 0x3A..=0x3B | 0x3F..=0x40
            | 0x5B..=0x5D | 0x5F | 0x7B | 0x7D => true,
        0xA1 | 0xA7 | 0xAB | 0xB6..=0xB7 | 0xBB | 0xBF => true,
        0x2010..=0x2027 | 0x2030..=0x205E => true,
        0x3001..=0x3003 | 0x3008..=0x3011 | 0x3014..=0x301F => true,
        _ => false,
    }
}

fn is_unicode_symbol(cp: u32) -> bool {
    match cp {
        0x24 | 0x2B | 0x3C..=0x3E | 0x5E | 0x60 | 0x7C | 0x7E => true,
        0xA2..=0xA6 | 0xA8..=0xA9 | 0xAC | 0xAE..=0xB1 | 0xB4 | 0xB8 | 0xD7 | 0xF7 => true,
        0x20A0..=0x20CF => true,
        0x2100..=0x214F | 0x2190..=0x21FF | 0x2200..=0x22FF => true,
        _ => false,
    }
}

fn is_unicode_separator(cp: u32) -> bool {
    matches!(cp, 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028..=0x2029 | 0x202F | 0x205F | 0x3000)
}

fn is_unicode_other(cp: u32) -> bool {
    cp < 0x20 || cp == 0x7F || (cp >= 0x80 && cp <= 0x9F)
}

fn is_unicode_lowercase_letter(cp: u32) -> bool {
    match cp {
        0x61..=0x7A => true,
        0xDF..=0xF6 | 0xF8..=0xFF => true,
        _ => false,
    }
}

fn is_unicode_uppercase_letter(cp: u32) -> bool {
    match cp {
        0x41..=0x5A => true,
        0xC0..=0xD6 | 0xD8..=0xDE => true,
        _ => false,
    }
}

fn is_cyrillic(cp: u32) -> bool {
    matches!(cp, 0x400..=0x4FF | 0x500..=0x52F | 0x1C80..=0x1C88 | 0x2DE0..=0x2DFF | 0xA640..=0xA69F)
}

fn is_greek(cp: u32) -> bool {
    matches!(cp, 0x370..=0x3FF | 0x1F00..=0x1FFE)
}

fn is_latin(cp: u32) -> bool {
    matches!(cp,
        0x41..=0x5A | 0x61..=0x7A | 0xAA | 0xBA
        | 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2AF
        | 0x2B0..=0x2B8 | 0x2E0..=0x2E4
        | 0x1D00..=0x1D25 | 0x1D2C..=0x1D5C | 0x1D62..=0x1D65
        | 0x1E00..=0x1EFF
    )
}

fn is_han(cp: u32) -> bool {
    matches!(cp, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x2A6DF | 0x2A700..=0x2EBEF | 0xF900..=0xFAFF)
}

fn is_arabic(cp: u32) -> bool {
    matches!(cp, 0x600..=0x6FF | 0x750..=0x77F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}

fn is_hebrew(cp: u32) -> bool {
    matches!(cp, 0x590..=0x5FF | 0xFB1D..=0xFB4F)
}

fn is_hiragana(cp: u32) -> bool {
    matches!(cp, 0x3041..=0x3096 | 0x309D..=0x309F)
}

fn is_katakana(cp: u32) -> bool {
    matches!(cp, 0x30A1..=0x30FA | 0x30FD..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9D)
}

fn is_word_char(ch: u8, _case_insensitive: bool) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}

impl<'a> MatchState<'a> {
    /// Compute whether the "before" and "after" positions are word characters,
    /// honouring UTF-8 mode when enabled.
    fn word_boundary_states(&self, pos: usize) -> (bool, bool) {
        let before_word = if pos == 0 {
            false
        } else if self.flags.utf8 {
            // Find the codepoint ending at `pos`
            let start = utf8_cp_start_before(self.input, pos);
            let (cp, _) = decode_utf8_at(self.input, start);
            is_cp_word_char(cp, self.flags.utf8)
        } else {
            is_word_char(self.input[pos - 1], self.flags.case_insensitive)
        };
        let after_word = if pos >= self.input.len() {
            false
        } else if self.flags.utf8 {
            let (cp, _) = decode_utf8_at(self.input, pos);
            is_cp_word_char(cp, self.flags.utf8)
        } else {
            is_word_char(self.input[pos], self.flags.case_insensitive)
        };
        (before_word, after_word)
    }
}

/// Find the start of the UTF-8 codepoint that ends at or before `pos`.
fn utf8_cp_start_before(input: &[u8], pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut i = pos;
    while i > 0 {
        i -= 1;
        let b = input[i];
        if b < 0x80 || b & 0xC0 == 0xC0 {
            return i;
        }
        if pos.saturating_sub(i) >= 4 {
            return pos - 1;
        }
    }
    0
}

fn is_cp_word_char(cp: u32, utf8: bool) -> bool {
    if utf8 {
        is_unicode_letter(cp) || is_unicode_decimal_digit(cp) || is_unicode_mark(cp) || cp == b'_' as u32
    } else {
        (cp as u8).is_ascii_alphanumeric() || cp == b'_' as u32
    }
}

// ============================================================================
// Count capture groups in a regex AST
// ============================================================================

fn count_groups(node: &RegexNode) -> usize {
    match node {
        RegexNode::Group { index, node } => {
            let sub = count_groups(node);
            (*index).max(sub)
        }
        RegexNode::NonCapturingGroup { node } => count_groups(node),
        RegexNode::Lookahead { node, .. } => count_groups(node),
        RegexNode::Lookbehind { node, .. } => count_groups(node),
        RegexNode::Quantifier { node, .. } => count_groups(node),
        RegexNode::Sequence(nodes) | RegexNode::Alternation(nodes) => {
            nodes.iter().map(|n| count_groups(n)).max().unwrap_or(0)
        }
        _ => 0,
    }
}

// ============================================================================
// High-level matching with backtracking support
// ============================================================================

/// Match result
#[derive(Debug, Clone)]
pub struct RegexMatch {
    pub full_match: (usize, usize),
    pub groups: Vec<Option<(usize, usize)>>,
}

/// Backtracking-capable matcher for sequences with quantifiers.
/// This re-implements matching with proper backtracking for sequences
/// containing greedy quantifiers followed by more nodes.
fn match_sequence_backtrack(
    nodes: &[RegexNode],
    state: &mut MatchState,
    pos: usize,
) -> Option<usize> {
    if nodes.is_empty() {
        return Some(pos);
    }

    if state.step_count > MAX_STEPS {
        return None;
    }

    let first = &nodes[0];
    let rest = &nodes[1..];

    match first {
        RegexNode::Quantifier {
            node,
            min,
            max,
            greedy,
        } => {
            // Collect all possible match lengths
            let effective_max = max.unwrap_or(usize::MAX);
            let mut positions = Vec::new();
            let mut current_pos = pos;
            positions.push((current_pos, state.captures.clone()));

            for _i in 0..effective_max {
                if state.step_count > MAX_STEPS {
                    return None;
                }
                let mut sub_state = state.clone();
                match sub_state.try_match(node, current_pos) {
                    Some(new_pos) => {
                        state.step_count = sub_state.step_count;
                        if new_pos == current_pos && positions.len() > *min {
                            break;
                        }
                        current_pos = new_pos;
                        positions.push((current_pos, sub_state.captures.clone()));
                    }
                    None => {
                        state.step_count = sub_state.step_count;
                        break;
                    }
                }
            }

            if positions.len() - 1 < *min {
                return None;
            }

            // Trim to minimum
            let min_idx = *min;

            if *greedy {
                // Try from longest match backwards
                for i in (min_idx..positions.len()).rev() {
                    if state.step_count > MAX_STEPS {
                        return None;
                    }
                    let (try_pos, ref caps) = positions[i];
                    let mut sub_state = state.clone();
                    sub_state.captures = caps.clone();
                    if let Some(end) = match_sequence_backtrack(rest, &mut sub_state, try_pos) {
                        state.captures = sub_state.captures;
                        state.match_start_override = sub_state.match_start_override;
                        state.step_count = sub_state.step_count;
                        return Some(end);
                    }
                    state.step_count = sub_state.step_count;
                }
                None
            } else {
                // Lazy: try from shortest match forwards
                for i in min_idx..positions.len() {
                    if state.step_count > MAX_STEPS {
                        return None;
                    }
                    let (try_pos, ref caps) = positions[i];
                    let mut sub_state = state.clone();
                    sub_state.captures = caps.clone();
                    if let Some(end) = match_sequence_backtrack(rest, &mut sub_state, try_pos) {
                        state.captures = sub_state.captures;
                        state.match_start_override = sub_state.match_start_override;
                        state.step_count = sub_state.step_count;
                        return Some(end);
                    }
                    state.step_count = sub_state.step_count;
                }
                None
            }
        }

        RegexNode::Sequence(sub_nodes) => {
            // Flatten: match sub_nodes then rest
            let mut combined = sub_nodes.clone();
            combined.extend_from_slice(rest);
            match_sequence_backtrack(&combined, state, pos)
        }

        RegexNode::Alternation(branches) => {
            for branch in branches {
                if state.step_count > MAX_STEPS {
                    return None;
                }
                let mut sub_state = state.clone();
                // Build a temporary sequence: [branch, rest...]
                let mut combined = vec![branch.clone()];
                combined.extend_from_slice(rest);
                if let Some(end) = match_sequence_backtrack(&combined, &mut sub_state, pos) {
                    state.captures = sub_state.captures;
                    state.match_start_override = sub_state.match_start_override;
                    state.step_count = sub_state.step_count;
                    return Some(end);
                }
                state.step_count = sub_state.step_count;
            }
            None
        }

        RegexNode::Group { index, node } => {
            let old_capture = state.captures.get(*index).cloned().flatten();

            // For a group containing a quantifier, we need to handle backtracking
            // between the group's quantifier and the rest of the sequence.
            // We do this by collecting all possible match positions for the group content,
            // then trying rest from each position (greedy: longest first, lazy: shortest first).
            match node.as_ref() {
                RegexNode::Quantifier {
                    node: inner_node,
                    min,
                    max,
                    greedy,
                } => {
                    let effective_max = max.unwrap_or(usize::MAX);
                    let mut positions = Vec::new();
                    let mut current_pos = pos;
                    positions.push((current_pos, state.captures.clone()));

                    for _ in 0..effective_max {
                        if state.step_count > MAX_STEPS {
                            return None;
                        }
                        let mut sub_state = state.clone();
                        match sub_state.try_match(inner_node, current_pos) {
                            Some(new_pos) => {
                                state.step_count = sub_state.step_count;
                                if new_pos == current_pos && positions.len() > *min {
                                    break;
                                }
                                current_pos = new_pos;
                                positions.push((current_pos, sub_state.captures.clone()));
                            }
                            None => {
                                state.step_count = sub_state.step_count;
                                break;
                            }
                        }
                    }

                    if positions.len() - 1 < *min {
                        if *index < state.captures.len() {
                            state.captures[*index] = old_capture;
                        }
                        return None;
                    }

                    let iter: Box<dyn Iterator<Item = usize>> = if *greedy {
                        Box::new((*min..positions.len()).rev())
                    } else {
                        Box::new(*min..positions.len())
                    };

                    for i in iter {
                        if state.step_count > MAX_STEPS {
                            return None;
                        }
                        let (try_pos, ref caps) = positions[i];
                        let mut sub_state = state.clone();
                        sub_state.captures = caps.clone();
                        if *index < sub_state.captures.len() {
                            sub_state.captures[*index] = Some((pos, try_pos));
                        }
                        if let Some(end) =
                            match_sequence_backtrack(rest, &mut sub_state, try_pos)
                        {
                            state.captures = sub_state.captures;
                            state.step_count = sub_state.step_count;
                            return Some(end);
                        }
                        state.step_count = sub_state.step_count;
                    }

                    if *index < state.captures.len() {
                        state.captures[*index] = old_capture;
                    }
                    None
                }
                _ => {
                    // Non-quantifier group: match inner content with backtracking,
                    // then try rest.

                    // For alternation inside group, each branch is tried:
                    let group_results = collect_match_positions(node, state, pos);

                    for (group_end, caps) in group_results {
                        if state.step_count > MAX_STEPS {
                            return None;
                        }
                        let mut sub_state = state.clone();
                        sub_state.captures = caps;
                        if *index < sub_state.captures.len() {
                            sub_state.captures[*index] = Some((pos, group_end));
                        }
                        if let Some(end) =
                            match_sequence_backtrack(rest, &mut sub_state, group_end)
                        {
                            state.captures = sub_state.captures;
                            state.step_count = sub_state.step_count;
                            return Some(end);
                        }
                        state.step_count = sub_state.step_count;
                    }

                    if *index < state.captures.len() {
                        state.captures[*index] = old_capture;
                    }
                    None
                }
            }
        }

        other => {
            // Non-quantifier node: match normally, then continue with rest
            let mut sub_state = state.clone();
            if let Some(new_pos) = sub_state.try_match(other, pos) {
                if let Some(end) = match_sequence_backtrack(rest, &mut sub_state, new_pos) {
                    state.captures = sub_state.captures;
                    state.match_start_override = sub_state.match_start_override;
                    state.step_count = sub_state.step_count;
                    return Some(end);
                }
                state.step_count = sub_state.step_count;
            } else {
                state.step_count = sub_state.step_count;
            }
            None
        }
    }
}

/// Match a single node with backtracking support (for top-level or group contents)
fn match_node_backtrack(node: &RegexNode, state: &mut MatchState, pos: usize) -> Option<usize> {
    match node {
        RegexNode::Sequence(nodes) => match_sequence_backtrack(nodes, state, pos),
        RegexNode::Alternation(branches) => {
            for branch in branches {
                if state.step_count > MAX_STEPS {
                    return None;
                }
                let mut sub_state = state.clone();
                if let Some(end) = match_node_backtrack(branch, &mut sub_state, pos) {
                    state.captures = sub_state.captures;
                    state.match_start_override = sub_state.match_start_override;
                    state.step_count = sub_state.step_count;
                    return Some(end);
                }
                state.step_count = sub_state.step_count;
            }
            None
        }
        _ => {
            // Wrap in a single-element sequence for uniform handling
            match_sequence_backtrack(&[node.clone()], state, pos)
        }
    }
}

fn _flatten_to_sequence(node: &RegexNode) -> Vec<RegexNode> {
    match node {
        RegexNode::Sequence(nodes) => nodes.clone(),
        other => vec![other.clone()],
    }
}

/// Collect all possible match end positions for a node (for backtracking through groups).
fn collect_match_positions(
    node: &RegexNode,
    state: &mut MatchState,
    pos: usize,
) -> Vec<(usize, Vec<Option<(usize, usize)>>)> {
    let mut results = Vec::new();
    match node {
        RegexNode::Alternation(branches) => {
            for branch in branches {
                if state.step_count > MAX_STEPS {
                    break;
                }
                let mut sub_state = state.clone();
                if let Some(end) = match_node_backtrack(branch, &mut sub_state, pos) {
                    state.step_count = sub_state.step_count;
                    results.push((end, sub_state.captures));
                } else {
                    state.step_count = sub_state.step_count;
                }
            }
        }
        _ => {
            let mut sub_state = state.clone();
            if let Some(end) = match_node_backtrack(node, &mut sub_state, pos) {
                state.step_count = sub_state.step_count;
                results.push((end, sub_state.captures));
            } else {
                state.step_count = sub_state.step_count;
            }
        }
    }
    results
}

// ============================================================================
// Top-level regex compilation and matching
// ============================================================================

/// Compiled regex
pub struct CompiledRegex {
    ast: RegexNode,
    flags: RegexFlags,
    num_groups: usize,
    group_names: Vec<(usize, Vec<u8>)>, // (group_index, name)
}

/// Format a PCRE error message following PHP conventions.
/// Delimiter/modifier errors are reported directly, other errors use "Compilation failed:" prefix.
fn format_preg_error(func_name: &str, error: &str) -> String {
    if error.starts_with("Delimiter must") || error.starts_with("No ending delimiter") || error.starts_with("No ending matching delimiter") || error.starts_with("Unknown modifier") || error.starts_with("Empty regular expression") || error.starts_with("NUL byte is not") {
        format!("{}: {}", func_name, error)
    } else if error.contains(" at offset ") {
        format!("{}: Compilation failed: {}", func_name, error)
    } else {
        // Approximate PCRE's offset reporting
        format!("{}: Compilation failed: {} at offset 0", func_name, error)
    }
}

/// Parse a PHP regex pattern like `/pattern/flags` or `~pattern~flags`
pub fn parse_php_regex(pattern: &[u8]) -> Result<CompiledRegex, String> {
    if pattern.is_empty() {
        return Err("empty pattern".into());
    }

    // Find delimiter
    let delimiter = pattern[0];
    if delimiter.is_ascii_alphanumeric() || delimiter == b'\\' || delimiter == 0 {
        return Err("Delimiter must not be alphanumeric, backslash, or NUL byte".into());
    }

    let closing_delimiter = match delimiter {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b'<' => b'>',
        d => d,
    };

    // Find closing delimiter (search from end to handle unescaped delimiters in pattern)
    let mut end_pos = None;
    let mut i = pattern.len() - 1;
    while i > 0 {
        if pattern[i] == closing_delimiter {
            // Check it's not escaped
            let mut backslashes = 0;
            let mut j = i;
            while j > 1 && pattern[j - 1] == b'\\' {
                backslashes += 1;
                j -= 1;
            }
            if backslashes % 2 == 0 {
                end_pos = Some(i);
                break;
            }
        }
        i -= 1;
    }

    let end_pos = end_pos.ok_or_else(|| {
        if closing_delimiter != delimiter {
            format!("No ending matching delimiter '{}' found", char::from(closing_delimiter))
        } else {
            format!("No ending delimiter '{}' found", char::from(delimiter))
        }
    })?;

    let regex_body = &pattern[1..end_pos];
    let flags_str = &pattern[end_pos + 1..];

    let mut flags = RegexFlags::default();
    for &flag_byte in flags_str {
        match flag_byte {
            0 => {
                return Err("NUL byte is not a valid modifier".into());
            }
            b'i' => flags.case_insensitive = true,
            b'm' => flags.multiline = true,
            b's' => flags.dotall = true,
            b'x' => flags.extended = true,
            b'U' => flags.ungreedy = true,
            b'u' => flags.utf8 = true,
            b'D' => flags.dollar_end_only = true,
            b'A' => flags.anchored = true,
            b'S' => {} // Extra study — ignore
            b'X' => {} // Extra — ignore
            b'J' => {} // Allow duplicate names — ignore
            b'n' => flags.no_auto_capture = true,
            b'r' => {} // Caseless restrict — ignore
            b'\r' | b'\n' | b' ' => {} // trailing whitespace
            _ => {
                return Err(format!("Unknown modifier '{}'", char::from(flag_byte)));
            }
        }
    }

    // Pre-process regex body for extended mode (strip comments and unescaped whitespace)
    let processed_body;
    let body = if flags.extended {
        processed_body = strip_extended_whitespace(regex_body);
        &processed_body[..]
    } else {
        regex_body
    };

    let mut parser = RegexParser::new(body, flags.clone());
    let ast = parser.parse()?;
    let num_groups = count_groups(&ast);

    // Update flags from parser (inline flag changes)
    let final_flags = parser.flags;

    Ok(CompiledRegex {
        ast,
        flags: final_flags,
        num_groups,
        group_names: parser.group_names,
    })
}

fn strip_extended_whitespace(input: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(input.len());
    let mut i = 0;
    let mut in_class = false;
    while i < input.len() {
        let ch = input[i];
        if ch == b'\\' && i + 1 < input.len() {
            result.push(ch);
            result.push(input[i + 1]);
            i += 2;
            continue;
        }
        if ch == b'[' && !in_class {
            in_class = true;
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == b']' && in_class {
            in_class = false;
            result.push(ch);
            i += 1;
            continue;
        }
        if !in_class {
            if ch == b'#' {
                // Comment: skip to end of line
                while i < input.len() && input[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if ch.is_ascii_whitespace() {
                i += 1;
                continue;
            }
        }
        result.push(ch);
        i += 1;
    }
    result
}

impl CompiledRegex {
    /// Find the first match in the input, starting from `start_offset`.
    pub fn find(&self, input: &[u8], start_offset: usize) -> Option<RegexMatch> {
        let mut state = MatchState::new(input, self.num_groups, self.flags.clone());

        // Check if pattern is anchored (starts with ^ or has A flag)
        let is_anchored = if self.flags.anchored {
            true
        } else if !self.flags.multiline {
            match &self.ast {
                RegexNode::StartAnchor => true,
                RegexNode::Sequence(nodes) => {
                    matches!(nodes.first(), Some(RegexNode::StartAnchor))
                }
                _ => false,
            }
        } else {
            false
        };

        let end = if is_anchored {
            start_offset + 1
        } else {
            input.len() + 1
        };

        let mut start_pos = start_offset;
        while start_pos < end {
            state.captures = vec![None; self.num_groups + 1];
            state.step_count = 0;
            state.match_start_override = None;
            if let Some(end_pos) = match_node_backtrack(&self.ast, &mut state, start_pos) {
                let real_start = state.match_start_override.unwrap_or(start_pos);
                state.captures[0] = Some((real_start, end_pos));
                return Some(RegexMatch {
                    full_match: (real_start, end_pos),
                    groups: state.captures,
                });
            }
            // Advance by one codepoint in UTF-8 mode, otherwise one byte
            if self.flags.utf8 && start_pos < input.len() {
                let (_, len) = decode_utf8_at(input, start_pos);
                start_pos += len.max(1);
            } else {
                start_pos += 1;
            }
        }
        None
    }

    /// Find all non-overlapping matches
    pub fn find_all(&self, input: &[u8]) -> Vec<RegexMatch> {
        let mut matches = Vec::new();
        let mut offset = 0;
        while offset <= input.len() {
            if let Some(m) = self.find(input, offset) {
                let start = m.full_match.0;
                let end = m.full_match.1;
                matches.push(m);
                if start == end {
                    // Zero-length match: advance past the match position
                    let step = if self.flags.utf8 && end < input.len() {
                        let (_, len) = decode_utf8_at(input, end);
                        len.max(1)
                    } else {
                        1
                    };
                    offset = end + step;
                } else {
                    offset = end;
                }
            } else {
                break;
            }
        }
        matches
    }
}

// ============================================================================
// PHP preg_* function implementations
// ============================================================================

/// Ensure the pattern argument is a string (not array/object).
/// On array/object, throws a TypeError and returns Err (caller propagates).
fn ensure_string_pattern(vm: &mut Vm, val: &Value, func: &str) -> Result<PhpString, VmError> {
    match val {
        Value::Array(_) => {
            let msg = format!("{}(): Argument #1 ($pattern) must be of type string, array given", func);
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            Err(VmError { message: "Uncaught TypeError".to_string(), line: 0 })
        }
        Value::Object(obj) => {
            let class_name = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
            let msg = format!("{}(): Argument #1 ($pattern) must be of type string, {} given", func, class_name);
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            Err(VmError { message: "Uncaught TypeError".to_string(), line: 0 })
        }
        _ => Ok(val.to_php_string()),
    }
}

/// preg_match($pattern, $subject [, &$matches [, $flags [, $offset]]])
pub fn preg_match(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.preg_last_error = 0;
    let pattern = match args.first() {
        Some(v) => ensure_string_pattern(vm, v, "preg_match")?,
        None => {
            return Ok(Value::False);
        }
    };

    if pattern.is_empty() || pattern.as_bytes().iter().all(|b| b.is_ascii_whitespace()) {
        vm.emit_warning("preg_match(): Empty regular expression");
        return Ok(Value::False);
    }

    let subject = match args.get(1) {
        Some(v) => v.to_php_string(),
        None => {
            return Ok(Value::False);
        }
    };

    let flags = args.get(3).map(|v| v.to_long()).unwrap_or(0);
    let offset_capture = (flags & 256) != 0; // PREG_OFFSET_CAPTURE
    let unmatched_as_null = (flags & 512) != 0; // PREG_UNMATCHED_AS_NULL

    let offset = if let Some(v) = args.get(4) {
        let o = v.to_long();
        if o < 0 {
            // Negative offset counts from end
            let len = subject.len() as i64;
            if o < -(len) {
                // Offset too large
                if o == i64::MIN {
                    let exc = vm.create_exception(b"ValueError", &format!("preg_match(): Argument #5 ($offset) must be greater than or equal to {}", -(subject.len() as i64)), 0);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: "Uncaught ValueError".to_string(), line: 0 });
                }
                0
            } else {
                (len + o) as usize
            }
        } else {
            o as usize
        }
    } else {
        0
    };

    // If offset exceeds subject length, return false
    if offset > subject.len() {
        return Ok(Value::False);
    }

    let compiled = match parse_php_regex(pattern.as_bytes()) {
        Ok(c) => c,
        Err(_e) => {
            let msg = format_preg_error("preg_match()", &_e);
            vm.emit_warning(&msg);
            return Ok(Value::False);
        }
    };

    let input = subject.as_bytes();

    // Validate UTF-8 when /u modifier is used
    if compiled.flags.utf8 && std::str::from_utf8(input).is_err() {
        vm.preg_last_error = 4; // PREG_BAD_UTF8_ERROR
        // Clear matches array too
        if let Some(matches_ref) = args.get(2) {
            if let Value::Reference(r) = matches_ref {
                *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(PhpArray::new())));
            }
        }
        return Ok(Value::False);
    }

    // Validate offset is on a codepoint boundary for /u
    if compiled.flags.utf8 && offset > 0 && offset < input.len() {
        let b = input[offset];
        if b & 0xC0 == 0x80 {
            // Continuation byte — offset is in the middle of a codepoint
            vm.preg_last_error = 5; // PREG_BAD_UTF8_OFFSET_ERROR
            if let Some(matches_ref) = args.get(2) {
                if let Value::Reference(r) = matches_ref {
                    *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(PhpArray::new())));
                }
            }
            return Ok(Value::False);
        }
    }

    let result = compiled.find(input, offset);

    // If matches parameter was provided, try to fill it
    if let Some(matches_ref) = args.get(2) {
        if let Some(ref m) = result {
            let mut arr = PhpArray::new();
            for (i, capture) in m.groups.iter().enumerate() {
                let val = if let Some((start, end)) = capture {
                    let matched_text = &input[*start..*end];
                    if offset_capture {
                        let mut pair = PhpArray::new();
                        pair.push(Value::String(PhpString::from_bytes(matched_text)));
                        pair.push(Value::Long(*start as i64));
                        Value::Array(Rc::new(RefCell::new(pair)))
                    } else {
                        Value::String(PhpString::from_bytes(matched_text))
                    }
                } else if offset_capture {
                    let mut pair = PhpArray::new();
                    pair.push(if unmatched_as_null { Value::Null } else { Value::String(PhpString::empty()) });
                    pair.push(Value::Long(-1));
                    Value::Array(Rc::new(RefCell::new(pair)))
                } else if unmatched_as_null {
                    Value::Null
                } else {
                    Value::String(PhpString::empty())
                };
                // Add named group entry before the numeric entry
                if let Some((_, name)) = compiled.group_names.iter().find(|(idx, _)| *idx == i) {
                    arr.set(ArrayKey::String(PhpString::from_vec(name.clone())), val.clone());
                }
                arr.set(ArrayKey::Int(i as i64), val);
            }
            // Try to write back to reference
            if let Value::Reference(r) = matches_ref {
                *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(arr)));
            } else if let Value::Array(a) = matches_ref {
                let mut a = a.borrow_mut();
                *a = arr;
            }
        } else {
            // No match - set matches to empty array
            if let Value::Reference(r) = matches_ref {
                *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(PhpArray::new())));
            }
        }
    }

    match result {
        Some(_) => Ok(Value::Long(1)),
        None => Ok(Value::Long(0)),
    }
}

/// preg_match_all($pattern, $subject [, &$matches [, $flags [, $offset]]])
pub fn preg_match_all(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.preg_last_error = 0;
    let pattern = match args.first() {
        Some(v) => ensure_string_pattern(vm, v, "preg_match_all")?,
        None => {
            return Ok(Value::False);
        }
    };

    let subject = match args.get(1) {
        Some(v) => v.to_php_string(),
        None => {
            return Ok(Value::False);
        }
    };

    let flags = args.get(3).map(|v| v.to_long()).unwrap_or(0);
    let set_order = (flags & 2) != 0; // PREG_SET_ORDER
    let offset_capture = (flags & 256) != 0; // PREG_OFFSET_CAPTURE
    let unmatched_as_null = (flags & 512) != 0; // PREG_UNMATCHED_AS_NULL

    let offset = if let Some(v) = args.get(4) {
        let o = v.to_long();
        if o < 0 {
            let len = subject.len() as i64;
            if o < -(len) {
                if o == i64::MIN {
                    let exc = vm.create_exception(b"ValueError", &format!("preg_match_all(): Argument #5 ($offset) must be greater than or equal to {}", -(subject.len() as i64)), 0);
                    vm.current_exception = Some(exc);
                    return Err(VmError { message: "Uncaught ValueError".to_string(), line: 0 });
                }
                0
            } else {
                (len + o) as usize
            }
        } else {
            o as usize
        }
    } else {
        0
    };

    let compiled = match parse_php_regex(pattern.as_bytes()) {
        Ok(c) => c,
        Err(_e) => {
            vm.emit_warning(&format_preg_error("preg_match_all()", &_e));
            return Ok(Value::False);
        }
    };

    let input = subject.as_bytes();

    // Validate UTF-8 when /u modifier is used
    if compiled.flags.utf8 && std::str::from_utf8(input).is_err() {
        vm.preg_last_error = 4; // PREG_BAD_UTF8_ERROR
        // Set empty matches array if parameter was provided
        if let Some(matches_ref) = args.get(2) {
            let empty_arr = Value::Array(Rc::new(RefCell::new(PhpArray::new())));
            if let Value::Reference(r) = matches_ref {
                *r.borrow_mut() = empty_arr;
            }
        }
        return Ok(Value::False);
    }

    let all_matches = {
        let mut matches = Vec::new();
        let mut off = offset;
        while off <= input.len() {
            if let Some(m) = compiled.find(input, off) {
                let start = m.full_match.0;
                let end = m.full_match.1;
                matches.push(m);
                if start == end {
                    // Zero-length match: advance past the match position
                    let step = if compiled.flags.utf8 && end < input.len() {
                        let (_, len) = decode_utf8_at(input, end);
                        len.max(1)
                    } else {
                        1
                    };
                    off = end + step;
                } else {
                    off = end;
                }
            } else {
                break;
            }
        }
        matches
    };

    let match_count = all_matches.len() as i64;

    // Fill matches array if provided
    if let Some(matches_ref) = args.get(2) {
        let num_groups = compiled.num_groups + 1;
        let result;

        if set_order {
            // PREG_SET_ORDER — each element is an array of all groups for that match
            let mut set_arr = PhpArray::new();
            for m in &all_matches {
                let mut match_arr = PhpArray::new();
                for group_idx in 0..num_groups {
                    let val = if let Some(Some((start, end))) = m.groups.get(group_idx) {
                        if offset_capture {
                            let mut pair = PhpArray::new();
                            pair.push(Value::String(PhpString::from_bytes(&input[*start..*end])));
                            pair.push(Value::Long(*start as i64));
                            Value::Array(Rc::new(RefCell::new(pair)))
                        } else {
                            Value::String(PhpString::from_bytes(&input[*start..*end]))
                        }
                    } else if offset_capture {
                        let mut pair = PhpArray::new();
                        pair.push(if unmatched_as_null { Value::Null } else { Value::String(PhpString::empty()) });
                        pair.push(Value::Long(-1));
                        Value::Array(Rc::new(RefCell::new(pair)))
                    } else if unmatched_as_null {
                        Value::Null
                    } else {
                        Value::String(PhpString::empty())
                    };
                    // Add named group entry before numeric
                    if let Some((_, name)) = compiled.group_names.iter().find(|(idx, _)| *idx == group_idx) {
                        match_arr.set(ArrayKey::String(PhpString::from_vec(name.clone())), val.clone());
                    }
                    match_arr.set(ArrayKey::Int(group_idx as i64), val);
                }
                set_arr.push(Value::Array(Rc::new(RefCell::new(match_arr))));
            }
            result = set_arr;
        } else {
            // PREG_PATTERN_ORDER (default) — group each capture group across all matches
            let mut pattern_arr = PhpArray::new();
            for group_idx in 0..num_groups {
                let mut group_arr = PhpArray::new();
                for m in &all_matches {
                    if let Some(Some((start, end))) = m.groups.get(group_idx) {
                        if offset_capture {
                            let mut pair = PhpArray::new();
                            pair.push(Value::String(PhpString::from_bytes(&input[*start..*end])));
                            pair.push(Value::Long(*start as i64));
                            group_arr.push(Value::Array(Rc::new(RefCell::new(pair))));
                        } else {
                            group_arr.push(Value::String(PhpString::from_bytes(&input[*start..*end])));
                        }
                    } else if offset_capture {
                        let mut pair = PhpArray::new();
                        pair.push(if unmatched_as_null { Value::Null } else { Value::String(PhpString::empty()) });
                        pair.push(Value::Long(-1));
                        group_arr.push(Value::Array(Rc::new(RefCell::new(pair))));
                    } else if unmatched_as_null {
                        group_arr.push(Value::Null);
                    } else {
                        group_arr.push(Value::String(PhpString::empty()));
                    }
                }
                // Add named group entry before numeric
                let val = Value::Array(Rc::new(RefCell::new(group_arr.clone())));
                if let Some((_, name)) = compiled.group_names.iter().find(|(idx, _)| *idx == group_idx) {
                    pattern_arr.set(ArrayKey::String(PhpString::from_vec(name.clone())), val.clone());
                }
                pattern_arr.set(ArrayKey::Int(group_idx as i64), val);
            }
            result = pattern_arr;
        }

        if let Value::Reference(r) = matches_ref {
            *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(result)));
        } else if let Value::Array(a) = matches_ref {
            *a.borrow_mut() = result;
        }
    }

    Ok(Value::Long(match_count))
}

/// preg_replace($pattern, $replacement, $subject [, $limit [, &$count]])
pub fn preg_replace(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.preg_last_error = 0;
    let pattern_val = match args.first() {
        Some(v) => {
            if let Value::Object(obj) = v {
                let class_name = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
                let msg = format!("preg_replace(): Argument #1 ($pattern) must be of type array|string, {} given", class_name);
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: "Uncaught TypeError".to_string(), line: 0 });
            }
            v.clone()
        }
        None => return Ok(Value::Null),
    };
    let replacement_val = match args.get(1) {
        Some(v) => {
            if let Value::Object(obj) = v {
                let class_name = String::from_utf8_lossy(&obj.borrow().class_name).to_string();
                let msg = format!("preg_replace(): Argument #2 ($replacement) must be of type array|string, {} given", class_name);
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: "Uncaught TypeError".to_string(), line: 0 });
            }
            // If replacement is array but pattern is not, TypeError
            if matches!(v, Value::Array(_)) && !matches!(&pattern_val, Value::Array(_)) {
                let msg = "preg_replace(): Argument #1 ($pattern) must be of type array when argument #2 ($replacement) is an array, string given".to_string();
                let exc = vm.create_exception(b"TypeError", &msg, 0);
                vm.current_exception = Some(exc);
                return Err(VmError { message: "Uncaught TypeError".to_string(), line: 0 });
            }
            v.clone()
        }
        None => return Ok(Value::Null),
    };
    let subject_val = match args.get(2) {
        Some(v) => v.clone(),
        None => return Ok(Value::Null),
    };
    let limit = args
        .get(3)
        .map(|v| v.to_long())
        .unwrap_or(-1);

    let mut total_count = 0i64;

    // Handle array pattern + array replacement
    if let Value::Array(patterns_arr) = &pattern_val {
        let patterns: Vec<Value> = patterns_arr.borrow().iter().map(|(_, v)| v.clone()).collect();
        let replacements: Vec<Value> = if let Value::Array(repl_arr) = &replacement_val {
            repl_arr.borrow().iter().map(|(_, v)| v.clone()).collect()
        } else {
            // Single replacement string for all patterns
            vec![replacement_val.clone(); patterns.len()]
        };

        if let Value::Array(subjects_arr) = &subject_val {
            // Array subject
            let mut result = PhpArray::new();
            for (key, subject) in subjects_arr.borrow().iter() {
                let subject_str = subject.to_php_string();
                let mut current = subject_str.as_bytes().to_vec();
                let mut had_error = false;
                for (i, pat) in patterns.iter().enumerate() {
                    let repl = replacements.get(i).unwrap_or(&Value::String(PhpString::empty())).to_php_string();
                    match do_preg_replace(vm, pat.to_php_string().as_bytes(), repl.as_bytes(), &current, limit) {
                        Some((replaced, cnt)) => {
                            current = replaced;
                            total_count += cnt;
                        }
                        None => { had_error = true; break; }
                    }
                }
                if had_error {
                    // return Null for the whole result on error
                    return Ok(Value::Null);
                }
                result.set(key.clone(), Value::String(PhpString::from_vec(current)));
            }
            if let Some(count_ref) = args.get(4) {
                if let Value::Reference(r) = count_ref {
                    *r.borrow_mut() = Value::Long(total_count);
                }
            }
            return Ok(Value::Array(Rc::new(RefCell::new(result))));
        } else {
            // String subject
            let subject_str = subject_val.to_php_string();
            let mut current = subject_str.as_bytes().to_vec();
            for (i, pat) in patterns.iter().enumerate() {
                let repl = replacements.get(i).unwrap_or(&Value::String(PhpString::empty())).to_php_string();
                match do_preg_replace(vm, pat.to_php_string().as_bytes(), repl.as_bytes(), &current, limit) {
                    Some((replaced, cnt)) => {
                        current = replaced;
                        total_count += cnt;
                    }
                    None => return Ok(Value::Null),
                }
            }
            if let Some(count_ref) = args.get(4) {
                if let Value::Reference(r) = count_ref {
                    *r.borrow_mut() = Value::Long(total_count);
                }
            }
            return Ok(Value::String(PhpString::from_vec(current)));
        }
    }

    let pattern = pattern_val.to_php_string();
    let replacement = replacement_val.to_php_string();

    // Handle array subject
    if let Value::Array(subjects_arr) = &subject_val {
        let mut result = PhpArray::new();
        for (key, subject) in subjects_arr.borrow().iter() {
            let subject_str = subject.to_php_string();
            match do_preg_replace(
                vm,
                pattern.as_bytes(),
                replacement.as_bytes(),
                subject_str.as_bytes(),
                limit,
            ) {
                Some((replaced, cnt)) => {
                    total_count += cnt;
                    result.set(key.clone(), Value::String(PhpString::from_vec(replaced)));
                }
                None => return Ok(Value::Null),
            }
        }
        if let Some(count_ref) = args.get(4) {
            if let Value::Reference(r) = count_ref {
                *r.borrow_mut() = Value::Long(total_count);
            }
        }
        return Ok(Value::Array(Rc::new(RefCell::new(result))));
    }

    let subject = subject_val.to_php_string();
    let (replaced, cnt) = match do_preg_replace(
        vm,
        pattern.as_bytes(),
        replacement.as_bytes(),
        subject.as_bytes(),
        limit,
    ) {
        Some(r) => r,
        None => return Ok(Value::Null),
    };
    total_count += cnt;

    if let Some(count_ref) = args.get(4) {
        if let Value::Reference(r) = count_ref {
            *r.borrow_mut() = Value::Long(total_count);
        }
    }

    Ok(Value::String(PhpString::from_vec(replaced)))
}

fn do_preg_replace(vm: &mut Vm, pattern: &[u8], replacement: &[u8], subject: &[u8], limit: i64) -> Option<(Vec<u8>, i64)> {
    let compiled = match parse_php_regex(pattern) {
        Ok(c) => c,
        Err(_e) => {
            vm.emit_warning(&format_preg_error("preg_replace()", &_e));
            return None;
        }
    };

    // Validate UTF-8 when /u modifier is used
    if compiled.flags.utf8 && std::str::from_utf8(subject).is_err() {
        vm.preg_last_error = 4; // PREG_BAD_UTF8_ERROR
        return None;
    }

    let mut result = Vec::new();
    let mut offset = 0;
    let mut count = 0i64;
    let effective_limit = if limit < 0 { i64::MAX } else { limit };

    while offset <= subject.len() && count < effective_limit {
        if let Some(m) = compiled.find(subject, offset) {
            let (match_start, match_end) = m.full_match;

            // Copy everything before the match
            result.extend_from_slice(&subject[offset..match_start]);

            // Apply replacement with backreferences
            apply_replacement(&mut result, replacement, subject, &m);

            count += 1;

            if match_end == offset {
                // Zero-length match: copy one codepoint and advance past it
                if offset < subject.len() {
                    let step = if compiled.flags.utf8 {
                        let (_, len) = decode_utf8_at(subject, offset);
                        len.max(1)
                    } else {
                        1
                    };
                    result.extend_from_slice(&subject[offset..offset + step]);
                    offset += step;
                } else {
                    offset += 1;
                }
            } else {
                offset = match_end;
            }
        } else {
            break;
        }
    }

    // Copy remaining
    if offset <= subject.len() {
        result.extend_from_slice(&subject[offset..]);
    }

    Some((result, count))
}

fn apply_replacement(result: &mut Vec<u8>, replacement: &[u8], subject: &[u8], m: &RegexMatch) {
    let mut i = 0;
    while i < replacement.len() {
        if replacement[i] == b'$' || replacement[i] == b'\\' {
            let escape_char = replacement[i];
            i += 1;
            if i < replacement.len() {
                if replacement[i] == b'{' {
                    // ${n} syntax
                    i += 1;
                    let mut num = 0usize;
                    let mut found_digit = false;
                    while i < replacement.len() && replacement[i].is_ascii_digit() {
                        num = num * 10 + (replacement[i] - b'0') as usize;
                        i += 1;
                        found_digit = true;
                    }
                    if i < replacement.len() && replacement[i] == b'}' {
                        i += 1;
                    }
                    if found_digit {
                        if let Some(Some((start, end))) = m.groups.get(num) {
                            result.extend_from_slice(&subject[*start..*end]);
                        }
                    }
                } else if replacement[i].is_ascii_digit() {
                    let mut num = (replacement[i] - b'0') as usize;
                    i += 1;
                    // Multi-digit backreferences (up to 2 digits)
                    if i < replacement.len() && replacement[i].is_ascii_digit() {
                        let two_digit = num * 10 + (replacement[i] - b'0') as usize;
                        if two_digit <= m.groups.len() {
                            num = two_digit;
                            i += 1;
                        }
                    }
                    if let Some(Some((start, end))) = m.groups.get(num) {
                        result.extend_from_slice(&subject[*start..*end]);
                    }
                } else if escape_char == b'\\' && replacement[i] == b'\\' {
                    result.push(b'\\');
                    i += 1;
                } else if escape_char == b'\\' && replacement[i] == b'$' {
                    result.push(b'$');
                    i += 1;
                } else {
                    // Not a backreference, output the escape char and continue
                    result.push(escape_char);
                    // Don't skip the next char, it will be processed in the next iteration
                }
            } else {
                result.push(escape_char);
            }
        } else {
            result.push(replacement[i]);
            i += 1;
        }
    }
}

/// preg_split($pattern, $subject [, $limit [, $flags]])
pub fn preg_split(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.preg_last_error = 0;
    let pattern = match args.first() {
        Some(v) => ensure_string_pattern(vm, v, "preg_split")?,
        None => return Ok(Value::Null),
    };

    let subject = match args.get(1) {
        Some(v) => v.to_php_string(),
        None => return Ok(Value::Null),
    };

    let limit = args.get(2).map(|v| v.to_long()).unwrap_or(-1);
    let flags = args.get(3).map(|v| v.to_long()).unwrap_or(0);

    let no_empty = (flags & 1) != 0; // PREG_SPLIT_NO_EMPTY
    let delim_capture = (flags & 2) != 0; // PREG_SPLIT_DELIM_CAPTURE
    let offset_capture = (flags & 4) != 0; // PREG_SPLIT_OFFSET_CAPTURE

    let compiled = match parse_php_regex(pattern.as_bytes()) {
        Ok(c) => c,
        Err(_e) => {
            vm.emit_warning(&format_preg_error("preg_split()", &_e));
            return Ok(Value::False);
        }
    };

    let input = subject.as_bytes();

    // Validate UTF-8 when /u modifier is used
    if compiled.flags.utf8 && std::str::from_utf8(input).is_err() {
        vm.preg_last_error = 4; // PREG_BAD_UTF8_ERROR
        return Ok(Value::False);
    }

    let mut result = PhpArray::new();
    let mut parts = 0i64;
    let effective_limit = if limit <= 0 { i64::MAX } else { limit };

    // Helper to push a value, optionally with offset capture
    let push_value = |result: &mut PhpArray, text: &[u8], text_offset: usize, offset_capture: bool| {
        if offset_capture {
            let mut pair = PhpArray::new();
            pair.push(Value::String(PhpString::from_bytes(text)));
            pair.push(Value::Long(text_offset as i64));
            result.push(Value::Array(Rc::new(RefCell::new(pair))));
        } else {
            result.push(Value::String(PhpString::from_bytes(text)));
        }
    };

    // last_split_pos: position where the current piece started (beginning of next piece after last split)
    // search_offset: position to search for next match from
    let mut last_split_pos = 0usize;
    let mut search_offset = 0usize;

    while search_offset <= input.len() && parts < effective_limit - 1 {
        if let Some(m) = compiled.find(input, search_offset) {
            let (match_start, match_end) = m.full_match;

            // Emit the piece from last_split_pos to match_start
            let part = &input[last_split_pos..match_start];
            if !no_empty || !part.is_empty() {
                push_value(&mut result, part, last_split_pos, offset_capture);
                parts += 1;
            }

            // Add captured groups if PREG_SPLIT_DELIM_CAPTURE
            if delim_capture {
                for i in 1..m.groups.len() {
                    if let Some(Some((start, end))) = m.groups.get(i) {
                        let captured = &input[*start..*end];
                        if !no_empty || !captured.is_empty() {
                            push_value(&mut result, captured, *start, offset_capture);
                        }
                    } else if !no_empty {
                        push_value(&mut result, b"", match_start, offset_capture);
                    }
                }
            }

            if match_start == match_end {
                // Zero-length match: advance search position past the match to avoid infinite loop.
                // In UTF-8 mode, advance by a full codepoint.
                last_split_pos = match_end;
                if match_end < input.len() {
                    let step = if compiled.flags.utf8 {
                        let (_, len) = decode_utf8_at(input, match_end);
                        len.max(1)
                    } else {
                        1
                    };
                    search_offset = match_end + step;
                } else {
                    search_offset = match_end + 1;
                    break;
                }
            } else {
                search_offset = match_end;
                last_split_pos = match_end;
            }
        } else {
            break;
        }
    }

    // Add remaining
    let remaining = &input[last_split_pos..];
    if !no_empty || !remaining.is_empty() {
        push_value(&mut result, remaining, last_split_pos, offset_capture);
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// preg_quote($str [, $delimiter])
pub fn preg_quote(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let input = match args.first() {
        Some(v) => v.to_php_string(),
        None => return Ok(Value::String(PhpString::empty())),
    };

    let delimiter = args.get(1).map(|v| {
        let s = v.to_php_string();
        if s.as_bytes().is_empty() {
            None
        } else {
            Some(s.as_bytes()[0])
        }
    }).flatten();

    let mut result = Vec::with_capacity(input.len() * 2);
    for &ch in input.as_bytes() {
        match ch {
            0 => {
                // Null byte: escape as \000
                result.push(b'\\');
                result.push(b'0');
                result.push(b'0');
                result.push(b'0');
            }
            b'\\' | b'+' | b'*' | b'?' | b'[' | b'^' | b']' | b'$' | b'(' | b')' | b'{'
            | b'}' | b'=' | b'!' | b'<' | b'>' | b'|' | b':' | b'-' | b'.' | b'#' => {
                result.push(b'\\');
                result.push(ch);
            }
            _ => {
                if let Some(d) = delimiter {
                    if ch == d {
                        result.push(b'\\');
                    }
                }
                result.push(ch);
            }
        }
    }

    Ok(Value::String(PhpString::from_vec(result)))
}

/// Call a PHP callback with the given arguments.
/// Handles string function names, array callbacks [obj, method], and closure objects.
fn call_callback(vm: &mut Vm, callback: &Value, call_args: &[Value]) -> Result<Value, VmError> {
    let (func_name, captured) = match callback {
        Value::String(s) => (s.as_bytes().to_vec(), vec![]),
        Value::Array(arr) => {
            let arr = arr.borrow();
            let vals: Vec<Value> = arr.values().cloned().collect();
            if vals.len() >= 2 {
                // [class/object, method] callback
                let first = &vals[0];
                let method = vals[1].to_php_string();
                match first {
                    Value::String(class_name) => {
                        // Static method: ["ClassName", "methodName"]
                        let mut name = class_name.as_bytes().to_vec();
                        name.extend_from_slice(b"::");
                        name.extend_from_slice(method.as_bytes());
                        (name, vec![])
                    }
                    Value::Object(_obj) => {
                        // Instance method: [$obj, "methodName"]
                        let class_name = first.to_php_string();
                        let mut name = class_name.as_bytes().to_vec();
                        name.extend_from_slice(b"::");
                        name.extend_from_slice(method.as_bytes());
                        (name, vec![first.clone()])
                    }
                    Value::Reference(r) => {
                        // Reference to object
                        let inner = r.borrow().clone();
                        if let Value::Object(_) = &inner {
                            let class_name = inner.to_php_string();
                            let mut name = class_name.as_bytes().to_vec();
                            name.extend_from_slice(b"::");
                            name.extend_from_slice(method.as_bytes());
                            (name, vec![inner])
                        } else {
                            let mut name = inner.to_php_string().as_bytes().to_vec();
                            name.extend_from_slice(b"::");
                            name.extend_from_slice(method.as_bytes());
                            (name, vec![])
                        }
                    }
                    _ => {
                        let mut name = first.to_php_string().as_bytes().to_vec();
                        name.extend_from_slice(b"::");
                        name.extend_from_slice(method.as_bytes());
                        (name, vec![])
                    }
                }
            } else if vals.len() == 1 {
                (vals[0].to_php_string().as_bytes().to_vec(), vec![])
            } else {
                return Ok(Value::Null);
            }
        }
        Value::Object(obj) => {
            // Closure object — call __invoke
            let class_lower: Vec<u8> = obj
                .borrow()
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            let class_name_orig = obj.borrow().class_name.clone();
            let has_invoke = vm
                .classes
                .get(&class_lower)
                .map(|c| c.methods.contains_key(&b"__invoke".to_vec()))
                .unwrap_or(false);
            if has_invoke {
                let mut func_name = class_name_orig;
                func_name.extend_from_slice(b"::__invoke");
                let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
                    let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                    // First CV is $this
                    if !fn_cvs.is_empty() {
                        fn_cvs[0] = callback.clone();
                    }
                    let mut idx = 1;
                    for arg in call_args {
                        if idx < fn_cvs.len() {
                            fn_cvs[idx] = arg.clone();
                            idx += 1;
                        }
                    }
                    return vm.execute_fn(&user_fn, fn_cvs);
                }
            }
            return Ok(Value::Null);
        }
        _ => return Ok(Value::Null),
    };

    let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();

    // Try builtin first
    if let Some(builtin) = vm.functions.get(&func_lower).copied() {
        return builtin(vm, call_args);
    }

    // Try user function
    if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
        let mut fn_cvs = vec![Value::Undef; user_fn.cv_names.len()];
        let mut idx = 0;
        for cv in &captured {
            if idx < fn_cvs.len() {
                fn_cvs[idx] = cv.clone();
                idx += 1;
            }
        }
        for arg in call_args {
            if idx < fn_cvs.len() {
                fn_cvs[idx] = arg.clone();
                idx += 1;
            }
        }
        return vm.execute_fn(&user_fn, fn_cvs);
    }

    Ok(Value::Null)
}

/// preg_replace_callback($pattern, $callback, $subject [, $limit [, &$count [, $flags]]])
pub fn preg_replace_callback(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.preg_last_error = 0;
    let pattern_val = match args.first() {
        Some(v) => v.clone(),
        None => return Ok(Value::Null),
    };
    let callback = match args.get(1) {
        Some(v) => v.clone(),
        None => return Ok(Value::Null),
    };
    let subject_val = match args.get(2) {
        Some(v) => v.clone(),
        None => return Ok(Value::Null),
    };
    let limit = args.get(3).map(|v| v.to_long()).unwrap_or(-1);
    let flags = args.get(5).map(|v| v.to_long()).unwrap_or(0);

    // Handle array pattern
    let patterns: Vec<Vec<u8>> = if let Value::Array(patterns_arr) = &pattern_val {
        patterns_arr.borrow().iter().map(|(_, v)| v.to_php_string().as_bytes().to_vec()).collect()
    } else {
        vec![pattern_val.to_php_string().as_bytes().to_vec()]
    };

    // Handle array subject
    if let Value::Array(subjects_arr) = &subject_val {
        let mut result = PhpArray::new();
        let mut total_count = 0i64;
        // Collect entries first to avoid borrow overlap when emitting warnings
        let entries: Vec<(ArrayKey, Value)> = subjects_arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (key, subject) in entries {
            if matches!(subject, Value::Array(_)) {
                vm.emit_warning("Array to string conversion");
            }
            let subject_str = subject.to_php_string();
            let mut current = subject_str.as_bytes().to_vec();
            for pat in &patterns {
                match do_preg_replace_callback(vm, pat, &callback, &current, limit, flags)? {
                    Some((replaced, count)) => {
                        current = replaced;
                        total_count += count;
                    }
                    None => return Ok(Value::Null),
                }
            }
            result.set(key, Value::String(PhpString::from_vec(current)));
        }
        // Set count if provided
        if let Some(count_ref) = args.get(4) {
            if let Value::Reference(r) = count_ref {
                *r.borrow_mut() = Value::Long(total_count);
            }
        }
        return Ok(Value::Array(Rc::new(RefCell::new(result))));
    }

    // String subject
    let subject = subject_val.to_php_string();
    let mut current = subject.as_bytes().to_vec();
    let mut total_count = 0i64;
    for pat in &patterns {
        match do_preg_replace_callback(vm, pat, &callback, &current, limit, flags)? {
            Some((replaced, count)) => {
                current = replaced;
                total_count += count;
            }
            None => return Ok(Value::Null),
        }
    }

    // Set count if provided
    if let Some(count_ref) = args.get(4) {
        if let Value::Reference(r) = count_ref {
            *r.borrow_mut() = Value::Long(total_count);
        }
    }

    Ok(Value::String(PhpString::from_vec(current)))
}

fn do_preg_replace_callback(
    vm: &mut Vm,
    pattern: &[u8],
    callback: &Value,
    subject: &[u8],
    limit: i64,
    flags: i64,
) -> Result<Option<(Vec<u8>, i64)>, VmError> {
    do_preg_replace_callback_fn(vm, pattern, callback, subject, limit, flags, "preg_replace_callback")
}

fn do_preg_replace_callback_fn(
    vm: &mut Vm,
    pattern: &[u8],
    callback: &Value,
    subject: &[u8],
    limit: i64,
    flags: i64,
    func_name: &str,
) -> Result<Option<(Vec<u8>, i64)>, VmError> {
    let offset_capture = (flags & 256) != 0; // PREG_OFFSET_CAPTURE
    let unmatched_as_null = (flags & 512) != 0; // PREG_UNMATCHED_AS_NULL

    let func_full = format!("{}()", func_name);
    if pattern.is_empty() || pattern.iter().all(|b| b.is_ascii_whitespace()) {
        vm.emit_warning(&format!("{}: Empty regular expression", func_full));
        return Ok(None);
    }
    let compiled = match parse_php_regex(pattern) {
        Ok(c) => c,
        Err(_e) => {
            vm.emit_warning(&format_preg_error(&func_full, &_e));
            return Ok(None);
        }
    };

    // Validate UTF-8 when /u modifier is used
    if compiled.flags.utf8 && std::str::from_utf8(subject).is_err() {
        vm.preg_last_error = 4; // PREG_BAD_UTF8_ERROR
        return Ok(None);
    }

    let mut result = Vec::new();
    let mut offset = 0;
    let mut count = 0i64;
    let effective_limit = if limit < 0 { i64::MAX } else { limit };

    while offset <= subject.len() && count < effective_limit {
        if let Some(m) = compiled.find(subject, offset) {
            let (match_start, match_end) = m.full_match;

            // Copy everything before the match
            result.extend_from_slice(&subject[offset..match_start]);

            // Build matches array for callback
            let mut matches_arr = PhpArray::new();
            for (i, capture) in m.groups.iter().enumerate() {
                let val = if let Some((start, end)) = capture {
                    let text = Value::String(PhpString::from_bytes(&subject[*start..*end]));
                    if offset_capture {
                        let mut pair = PhpArray::new();
                        pair.push(text);
                        pair.push(Value::Long(*start as i64));
                        Value::Array(Rc::new(RefCell::new(pair)))
                    } else {
                        text
                    }
                } else if offset_capture {
                    let mut pair = PhpArray::new();
                    pair.push(if unmatched_as_null { Value::Null } else { Value::String(PhpString::empty()) });
                    pair.push(Value::Long(-1));
                    Value::Array(Rc::new(RefCell::new(pair)))
                } else if unmatched_as_null {
                    Value::Null
                } else {
                    Value::String(PhpString::empty())
                };
                // Insert named key before numeric, matching preg_match behavior
                if let Some((_, name)) = compiled.group_names.iter().find(|(idx, _)| *idx == i) {
                    matches_arr.set(
                        ArrayKey::String(PhpString::from_vec(name.clone())),
                        val.clone(),
                    );
                }
                matches_arr.set(ArrayKey::Int(i as i64), val);
            }

            // Call the callback
            let matches_val = Value::Array(Rc::new(RefCell::new(matches_arr)));
            let replacement = call_callback(vm, callback, &[matches_val])?;
            let replacement_str = replacement.to_php_string();
            result.extend_from_slice(replacement_str.as_bytes());

            count += 1;

            if match_end == offset {
                if offset < subject.len() {
                    let step = if compiled.flags.utf8 {
                        let (_, len) = decode_utf8_at(subject, offset);
                        len.max(1)
                    } else {
                        1
                    };
                    result.extend_from_slice(&subject[offset..offset + step]);
                    offset += step;
                } else {
                    offset += 1;
                }
            } else {
                offset = match_end;
            }
        } else {
            break;
        }
    }

    // Copy remaining
    if offset <= subject.len() {
        result.extend_from_slice(&subject[offset..]);
    }

    Ok(Some((result, count)))
}

/// preg_replace_callback_array($patterns_and_callbacks, $subject [, $limit [, &$count [, $flags]]])
pub fn preg_replace_callback_array(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let patterns_callbacks = match args.first() {
        Some(Value::Array(arr)) => arr.clone(),
        _ => return Ok(Value::Null),
    };
    let subject_val = match args.get(1) {
        Some(v) => v.clone(),
        None => return Ok(Value::Null),
    };
    let limit = args.get(2).map(|v| v.to_long()).unwrap_or(-1);
    let flags = args.get(4).map(|v| v.to_long()).unwrap_or(0);

    // Collect patterns first — and validate that all keys are strings
    let pc_entries: Vec<(ArrayKey, Value)> = patterns_callbacks
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    // Check for numeric keys (invalid pattern)
    for (pk, _) in &pc_entries {
        if matches!(pk, ArrayKey::Int(_)) {
            let msg = "preg_replace_callback_array(): Argument #1 ($pattern) must contain only string patterns as keys".to_string();
            let exc = vm.create_exception(b"TypeError", &msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: "Uncaught TypeError".to_string(), line: 0 });
        }
    }

    // Handle array subject
    if let Value::Array(subjects_arr) = &subject_val {
        let mut result = PhpArray::new();
        let mut total_count = 0i64;
        let subj_entries: Vec<(ArrayKey, Value)> = subjects_arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (key, subject) in subj_entries {
            if matches!(subject, Value::Array(_)) {
                vm.emit_warning("Array to string conversion");
            }
            let subject_str = subject.to_php_string();
            let mut current = subject_str.as_bytes().to_vec();
            for (pat_key, cb) in &pc_entries {
                let pat_str = match pat_key {
                    ArrayKey::String(s) => s.as_bytes().to_vec(),
                    ArrayKey::Int(_) => continue,
                };
                match do_preg_replace_callback_fn(vm, &pat_str, cb, &current, limit, flags, "preg_replace_callback_array")? {
                    Some((replaced, count)) => {
                        current = replaced;
                        total_count += count;
                    }
                    None => return Ok(Value::Null),
                }
            }
            result.set(key, Value::String(PhpString::from_vec(current)));
        }
        if let Some(count_ref) = args.get(3) {
            if let Value::Reference(r) = count_ref {
                *r.borrow_mut() = Value::Long(total_count);
            }
        }
        return Ok(Value::Array(Rc::new(RefCell::new(result))));
    }

    // String subject
    let subject = subject_val.to_php_string();
    let mut current = subject.as_bytes().to_vec();
    let mut total_count = 0i64;
    for (pat_key, cb) in &pc_entries {
        let pat_str = match pat_key {
            ArrayKey::String(s) => s.as_bytes().to_vec(),
            ArrayKey::Int(_) => continue,
        };
        match do_preg_replace_callback_fn(vm, &pat_str, cb, &current, limit, flags, "preg_replace_callback_array")? {
            Some((replaced, count)) => {
                current = replaced;
                total_count += count;
            }
            None => return Ok(Value::Null),
        }
    }
    if let Some(count_ref) = args.get(3) {
        if let Value::Reference(r) = count_ref {
            *r.borrow_mut() = Value::Long(total_count);
        }
    }

    Ok(Value::String(PhpString::from_vec(current)))
}

/// preg_grep($pattern, $array [, $flags])
pub fn preg_grep(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let pattern = match args.first() {
        Some(v) => ensure_string_pattern(vm, v, "preg_grep")?,
        None => return Ok(Value::False),
    };

    let array = match args.get(1) {
        Some(Value::Array(arr)) => arr.clone(),
        _ => return Ok(Value::False),
    };

    let flags = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let invert = (flags & 1) != 0; // PREG_GREP_INVERT

    let compiled = match parse_php_regex(pattern.as_bytes()) {
        Ok(c) => c,
        Err(_e) => {
            vm.emit_warning(&format_preg_error("preg_grep()", &_e));
            return Ok(Value::False);
        }
    };

    let mut result = PhpArray::new();
    // Collect entries first to avoid borrow overlap when emitting warnings
    let entries: Vec<(ArrayKey, Value)> = array.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (key, value) in entries {
        if matches!(value, Value::Array(_)) {
            vm.emit_warning("Array to string conversion");
        }
        let subject = value.to_php_string();
        let matches = compiled.find(subject.as_bytes(), 0).is_some();
        if matches != invert {
            result.set(key, value);
        }
    }

    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

/// preg_filter($pattern, $replacement, $subject [, $limit [, &$count]])
pub fn preg_filter(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let pattern_val = match args.first() {
        Some(v) => v.clone(),
        None => return Ok(Value::Null),
    };
    let replacement_val = match args.get(1) {
        Some(v) => v.clone(),
        None => return Ok(Value::Null),
    };
    let subject_val = match args.get(2) {
        Some(v) => v.clone(),
        None => return Ok(Value::Null),
    };
    let limit = args.get(3).map(|v| v.to_long()).unwrap_or(-1);

    // preg_filter is like preg_replace but only returns subjects where there was a match
    // For array subject: return only elements where at least one replacement was made
    // For string subject: return null if no match found

    let patterns: Vec<(Vec<u8>, Vec<u8>)> = if let Value::Array(pat_arr) = &pattern_val {
        let replacements: Vec<Value> = if let Value::Array(repl_arr) = &replacement_val {
            repl_arr.borrow().iter().map(|(_, v)| v.clone()).collect()
        } else {
            vec![replacement_val.clone(); pat_arr.borrow().len()]
        };
        pat_arr.borrow().iter().enumerate().map(|(i, (_, v))| {
            let repl = replacements.get(i).unwrap_or(&Value::String(PhpString::empty())).to_php_string();
            (v.to_php_string().as_bytes().to_vec(), repl.as_bytes().to_vec())
        }).collect()
    } else {
        vec![(
            pattern_val.to_php_string().as_bytes().to_vec(),
            replacement_val.to_php_string().as_bytes().to_vec(),
        )]
    };

    if let Value::Array(subjects_arr) = &subject_val {
        let mut result = PhpArray::new();
        for (key, subject) in subjects_arr.borrow().iter() {
            let subject_str = subject.to_php_string();
            let mut current = subject_str.as_bytes().to_vec();
            let mut had_match = false;
            for (pat, repl) in &patterns {
                let compiled = match parse_php_regex(pat) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                if compiled.find(&current, 0).is_some() {
                    had_match = true;
                    if let Some((replaced, _cnt)) = do_preg_replace(vm, pat, repl, &current, limit) {
                        current = replaced;
                    }
                }
            }
            if had_match {
                result.set(key.clone(), Value::String(PhpString::from_vec(current)));
            }
        }
        return Ok(Value::Array(Rc::new(RefCell::new(result))));
    }

    // String subject
    let subject = subject_val.to_php_string();
    let mut current = subject.as_bytes().to_vec();
    let mut had_match = false;
    for (pat, repl) in &patterns {
        let compiled = match parse_php_regex(pat) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if compiled.find(&current, 0).is_some() {
            had_match = true;
            if let Some((replaced, _cnt)) = do_preg_replace(vm, pat, repl, &current, limit) {
                current = replaced;
            }
        }
    }
    if had_match {
        Ok(Value::String(PhpString::from_vec(current)))
    } else {
        Ok(Value::Null)
    }
}

/// preg_last_error()
pub fn preg_last_error(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(vm.preg_last_error))
}

/// preg_last_error_msg()
pub fn preg_last_error_msg(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let msg = match vm.preg_last_error {
        0 => "No error",
        1 => "Internal error",
        2 => "Backtrack limit exhausted",
        3 => "Recursion limit exhausted",
        4 => "Malformed UTF-8 characters, possibly incorrectly encoded",
        5 => "Offset doesn't correspond to the beginning of a valid UTF-8 code point",
        6 => "JIT stack limit exhausted",
        _ => "Unknown error",
    };
    Ok(Value::String(PhpString::from_bytes(msg.as_bytes())))
}

// ============================================================================
// Registration
// ============================================================================

pub fn register(vm: &mut Vm) {
    vm.register_function(b"preg_match", preg_match);
    vm.register_function(b"preg_match_all", preg_match_all);
    vm.register_function(b"preg_replace", preg_replace);
    vm.register_function(b"preg_split", preg_split);
    vm.register_function(b"preg_quote", preg_quote);
    vm.register_function(b"preg_replace_callback", preg_replace_callback);
    vm.register_function(b"preg_replace_callback_array", preg_replace_callback_array);
    vm.register_function(b"preg_grep", preg_grep);
    vm.register_function(b"preg_filter", preg_filter);
    vm.register_function(b"preg_last_error", preg_last_error);
    vm.register_function(b"preg_last_error_msg", preg_last_error_msg);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_match(pattern: &str, subject: &str) -> bool {
        let compiled = parse_php_regex(pattern.as_bytes()).expect("regex should parse");
        compiled.find(subject.as_bytes(), 0).is_some()
    }

    fn test_captures(pattern: &str, subject: &str) -> Vec<String> {
        let compiled = parse_php_regex(pattern.as_bytes()).expect("regex should parse");
        let input = subject.as_bytes();
        match compiled.find(input, 0) {
            Some(m) => m
                .groups
                .iter()
                .map(|g| {
                    if let Some((start, end)) = g {
                        String::from_utf8_lossy(&input[*start..*end]).into_owned()
                    } else {
                        String::new()
                    }
                })
                .collect(),
            None => vec![],
        }
    }

    #[test]
    fn test_literal() {
        assert!(test_match("/abc/", "xyzabcdef"));
        assert!(!test_match("/abc/", "xyzdef"));
    }

    #[test]
    fn test_dot() {
        assert!(test_match("/a.c/", "abc"));
        assert!(test_match("/a.c/", "aXc"));
        assert!(!test_match("/a.c/", "a\nc"));
        assert!(test_match("/a.c/s", "a\nc")); // dotall
    }

    #[test]
    fn test_anchors() {
        assert!(test_match("/^abc/", "abcdef"));
        assert!(!test_match("/^abc/", "xabcdef"));
        assert!(test_match("/abc$/", "xyzabc"));
        assert!(!test_match("/abc$/", "abcdef"));
    }

    #[test]
    fn test_quantifiers() {
        assert!(test_match("/ab*c/", "ac"));
        assert!(test_match("/ab*c/", "abc"));
        assert!(test_match("/ab*c/", "abbc"));
        assert!(test_match("/ab+c/", "abc"));
        assert!(!test_match("/ab+c/", "ac"));
        assert!(test_match("/ab?c/", "ac"));
        assert!(test_match("/ab?c/", "abc"));
        assert!(!test_match("/ab?c/", "abbc"));
    }

    #[test]
    fn test_char_class() {
        assert!(test_match("/[abc]/", "b"));
        assert!(!test_match("/[abc]/", "d"));
        assert!(test_match("/[a-z]/", "m"));
        assert!(!test_match("/[a-z]/", "M"));
        assert!(test_match("/[^abc]/", "d"));
        assert!(!test_match("/[^abc]/", "b"));
    }

    #[test]
    fn test_shorthand_classes() {
        assert!(test_match("/\\d+/", "123"));
        assert!(!test_match("/^\\d+$/", "12a3"));
        assert!(test_match("/\\w+/", "hello_world"));
        assert!(test_match("/\\s/", "hello world"));
    }

    #[test]
    fn test_alternation() {
        assert!(test_match("/cat|dog/", "I have a cat"));
        assert!(test_match("/cat|dog/", "I have a dog"));
        assert!(!test_match("/cat|dog/", "I have a fish"));
    }

    #[test]
    fn test_groups() {
        let caps = test_captures("/(\\w+)@(\\w+)/", "user@host");
        assert_eq!(caps.len(), 3);
        assert_eq!(caps[0], "user@host");
        assert_eq!(caps[1], "user");
        assert_eq!(caps[2], "host");
    }

    #[test]
    fn test_case_insensitive() {
        assert!(test_match("/abc/i", "ABC"));
        assert!(test_match("/abc/i", "AbC"));
    }

    #[test]
    fn test_word_boundary() {
        assert!(test_match("/\\bword\\b/", "a word here"));
        assert!(!test_match("/\\bword\\b/", "awordhere"));
    }

    #[test]
    fn test_backtracking() {
        // `.*` should backtrack to allow `foo` to match
        assert!(test_match("/^.*foo$/", "hello foo"));
        assert!(test_match("/^(.*)foo$/", "hello foo"));
    }

    #[test]
    fn test_braces() {
        assert!(test_match("/a{3}/", "aaa"));
        assert!(!test_match("/a{3}/", "aa"));
        assert!(test_match("/a{2,4}/", "aaa"));
        assert!(!test_match("/^a{2,4}$/", "a"));
        assert!(test_match("/a{2,}/", "aaa"));
    }

    #[test]
    fn test_php_version_pattern() {
        // Common SKIPIF pattern
        assert!(test_match("/^8\\.5/", "8.5.4"));
        assert!(!test_match("/^8\\.5/", "7.4.0"));
    }

    #[test]
    fn test_different_delimiters() {
        assert!(test_match("~abc~", "xyzabcdef"));
        assert!(test_match("#abc#", "xyzabcdef"));
        assert!(test_match("{abc}", "xyzabcdef"));
    }

    #[test]
    fn test_preg_replace_simple() {
        let result = do_preg_replace_test("/abc/", "XYZ", "abc");
        assert_eq!(result, "XYZ");
    }

    #[test]
    fn test_preg_replace_backreference() {
        let result = do_preg_replace_test("/(\\w+)@(\\w+)/", "$2-$1", "user@host");
        assert_eq!(result, "host-user");
    }

    fn do_preg_replace_test(pattern: &str, replacement: &str, subject: &str) -> String {
        let compiled = parse_php_regex(pattern.as_bytes()).expect("regex should parse");
        let input = subject.as_bytes();
        let mut result = Vec::new();
        let mut offset = 0;

        if let Some(m) = compiled.find(input, 0) {
            let (match_start, match_end) = m.full_match;
            result.extend_from_slice(&input[offset..match_start]);
            apply_replacement(&mut result, replacement.as_bytes(), input, &m);
            offset = match_end;
        }
        result.extend_from_slice(&input[offset..]);
        String::from_utf8_lossy(&result).into_owned()
    }

    #[test]
    fn test_non_capturing_group() {
        assert!(test_match("/(?:abc)+/", "abcabc"));
        let caps = test_captures("/(?:abc)(def)/", "abcdef");
        assert_eq!(caps.len(), 2);
        assert_eq!(caps[0], "abcdef");
        assert_eq!(caps[1], "def");
    }

    #[test]
    fn test_escaped_delimiter() {
        assert!(test_match("/a\\/b/", "a/b"));
    }

    #[test]
    fn test_multiline() {
        assert!(test_match("/^line2$/m", "line1\nline2\nline3"));
        assert!(!test_match("/^line2$/", "line1\nline2\nline3"));
    }

    #[test]
    fn test_hex_escape() {
        assert!(test_match("/\\x41/", "A"));
        assert!(test_match("/[\\x41-\\x5A]/", "M"));
    }

    #[test]
    fn test_greedy_vs_lazy() {
        let caps = test_captures("/<(.+)>/", "<a>b<c>");
        assert_eq!(caps[1], "a>b<c"); // greedy

        let caps = test_captures("/<(.+?)>/", "<a>b<c>");
        assert_eq!(caps[1], "a"); // lazy
    }

    #[test]
    fn test_lookahead() {
        assert!(test_match("/foo(?=bar)/", "foobar"));
        assert!(!test_match("/foo(?=bar)/", "foobaz"));
        assert!(test_match("/foo(?!bar)/", "foobaz"));
        assert!(!test_match("/foo(?!bar)/", "foobar"));
    }
}
