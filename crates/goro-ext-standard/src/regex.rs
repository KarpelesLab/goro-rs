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
    /// Character class `[...]` or shorthand `\d`, `\w`, etc.
    CharClass {
        ranges: Vec<CharRange>,
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
    Single(u8),
    Range(u8, u8),
}

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
            Some(ch) => {
                self.advance();
                Ok(RegexNode::Literal(ch))
            }
        }
    }

    fn parse_escape(&mut self, in_class: bool) -> Result<RegexNode, String> {
        match self.advance() {
            None => Err("unexpected end of escape sequence".into()),
            Some(b'd') => Ok(RegexNode::CharClass {
                ranges: vec![CharRange::Range(b'0', b'9')],
                negated: false,
            }),
            Some(b'D') => Ok(RegexNode::CharClass {
                ranges: vec![CharRange::Range(b'0', b'9')],
                negated: true,
            }),
            Some(b'w') => Ok(RegexNode::CharClass {
                ranges: vec![
                    CharRange::Range(b'a', b'z'),
                    CharRange::Range(b'A', b'Z'),
                    CharRange::Range(b'0', b'9'),
                    CharRange::Single(b'_'),
                ],
                negated: false,
            }),
            Some(b'W') => Ok(RegexNode::CharClass {
                ranges: vec![
                    CharRange::Range(b'a', b'z'),
                    CharRange::Range(b'A', b'Z'),
                    CharRange::Range(b'0', b'9'),
                    CharRange::Single(b'_'),
                ],
                negated: true,
            }),
            Some(b's') => Ok(RegexNode::CharClass {
                ranges: vec![
                    CharRange::Single(b' '),
                    CharRange::Single(b'\t'),
                    CharRange::Single(b'\n'),
                    CharRange::Single(b'\r'),
                    CharRange::Single(0x0C), // form feed
                    CharRange::Single(0x0B), // vertical tab
                ],
                negated: false,
            }),
            Some(b'S') => Ok(RegexNode::CharClass {
                ranges: vec![
                    CharRange::Single(b' '),
                    CharRange::Single(b'\t'),
                    CharRange::Single(b'\n'),
                    CharRange::Single(b'\r'),
                    CharRange::Single(0x0C),
                    CharRange::Single(0x0B),
                ],
                negated: true,
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

        // Normal capture group
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

        let mut ranges = Vec::new();

        // Handle `]` or `-` as first character (literal)
        if self.peek() == Some(b']') {
            self.advance();
            ranges.push(CharRange::Single(b']'));
        }

        while let Some(ch) = self.peek() {
            if ch == b']' {
                self.advance();
                return Ok(RegexNode::CharClass { ranges, negated });
            }

            if ch == b'\\' {
                self.advance();
                match self.peek() {
                    Some(b'd') => {
                        self.advance();
                        ranges.push(CharRange::Range(b'0', b'9'));
                    }
                    Some(b'D') => {
                        // \D in class — we can't negate inside a range easily,
                        // just add common non-digit ranges
                        self.advance();
                        ranges.push(CharRange::Range(0, b'0' - 1));
                        ranges.push(CharRange::Range(b'9' + 1, 255));
                    }
                    Some(b'w') => {
                        self.advance();
                        ranges.push(CharRange::Range(b'a', b'z'));
                        ranges.push(CharRange::Range(b'A', b'Z'));
                        ranges.push(CharRange::Range(b'0', b'9'));
                        ranges.push(CharRange::Single(b'_'));
                    }
                    Some(b'W') => {
                        self.advance();
                        // Non-word chars — hard to express as ranges, add complement
                        ranges.push(CharRange::Range(0, b'/' ));  // before 0
                        ranges.push(CharRange::Range(b':' , b'@'));  // between 9 and A
                        ranges.push(CharRange::Range(b'[', b'^'));   // between Z and _
                        ranges.push(CharRange::Single(b'`'));        // between _ and a
                        ranges.push(CharRange::Range(b'{', 255));    // after z
                    }
                    Some(b's') => {
                        self.advance();
                        ranges.push(CharRange::Single(b' '));
                        ranges.push(CharRange::Single(b'\t'));
                        ranges.push(CharRange::Single(b'\n'));
                        ranges.push(CharRange::Single(b'\r'));
                        ranges.push(CharRange::Single(0x0C));
                        ranges.push(CharRange::Single(0x0B));
                    }
                    Some(b'S') => {
                        self.advance();
                        // non-whitespace — complement
                        ranges.push(CharRange::Range(0, 0x08));
                        ranges.push(CharRange::Single(0x0E));
                        ranges.push(CharRange::Range(0x0E, b' ' - 1));
                        ranges.push(CharRange::Range(b' ' + 1, 255));
                    }
                    Some(b'n') => {
                        self.advance();
                        ranges.push(CharRange::Single(b'\n'));
                    }
                    Some(b'r') => {
                        self.advance();
                        ranges.push(CharRange::Single(b'\r'));
                    }
                    Some(b't') => {
                        self.advance();
                        ranges.push(CharRange::Single(b'\t'));
                    }
                    Some(b'b') => {
                        // \b in character class = backspace
                        self.advance();
                        ranges.push(CharRange::Single(0x08));
                    }
                    Some(b'x') => {
                        self.advance();
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
                        ranges.push(CharRange::Single(val));
                    }
                    Some(c) => {
                        self.advance();
                        ranges.push(CharRange::Single(c));
                    }
                    None => {
                        return Err("unexpected end in char class escape".into());
                    }
                }
            } else if ch == b'-' && !ranges.is_empty() {
                // Could be a range like `a-z`
                self.advance();
                if self.peek() == Some(b']') {
                    // `-` at end is literal
                    ranges.push(CharRange::Single(b'-'));
                } else if let Some(end_ch) = self.peek() {
                    self.advance();
                    // Get the start of the range from the last entry
                    if let Some(CharRange::Single(start)) = ranges.last() {
                        let start = *start;
                        ranges.pop();
                        let actual_end = if end_ch == b'\\' {
                            // Handle escaped end of range
                            match self.advance() {
                                Some(b'n') => b'\n',
                                Some(b'r') => b'\r',
                                Some(b't') => b'\t',
                                Some(c) => c,
                                None => end_ch,
                            }
                        } else {
                            end_ch
                        };
                        ranges.push(CharRange::Range(start, actual_end));
                    } else {
                        ranges.push(CharRange::Single(b'-'));
                        ranges.push(CharRange::Single(end_ch));
                    }
                } else {
                    ranges.push(CharRange::Single(b'-'));
                }
            } else {
                self.advance();
                ranges.push(CharRange::Single(ch));
            }
        }

        Err("unclosed character class".into())
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
}

const MAX_STEPS: usize = 1_000_000;

impl<'a> MatchState<'a> {
    fn new(input: &'a [u8], num_groups: usize, flags: RegexFlags) -> Self {
        Self {
            input,
            captures: vec![None; num_groups + 1], // index 0 = full match
            flags,
            step_count: 0,
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

            RegexNode::AnyChar => {
                if pos < self.input.len() {
                    if self.flags.dotall || self.input[pos] != b'\n' {
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
                } else if pos == self.input.len() - 1 && self.input[pos] == b'\n' {
                    // $ matches before trailing newline
                    Some(pos)
                } else {
                    None
                }
            }

            RegexNode::WordBoundary => {
                let before_is_word =
                    pos > 0 && is_word_char(self.input[pos - 1], self.flags.case_insensitive);
                let after_is_word = pos < self.input.len()
                    && is_word_char(self.input[pos], self.flags.case_insensitive);
                if before_is_word != after_is_word {
                    Some(pos)
                } else {
                    None
                }
            }

            RegexNode::NonWordBoundary => {
                let before_is_word =
                    pos > 0 && is_word_char(self.input[pos - 1], self.flags.case_insensitive);
                let after_is_word = pos < self.input.len()
                    && is_word_char(self.input[pos], self.flags.case_insensitive);
                if before_is_word == after_is_word {
                    Some(pos)
                } else {
                    None
                }
            }

            RegexNode::CharClass { ranges, negated } => {
                if pos < self.input.len() {
                    let ch = self.input[pos];
                    let matches = char_in_ranges(ch, ranges, self.flags.case_insensitive);
                    if matches != *negated {
                        Some(pos + 1)
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

/// Check if a byte is in the given ranges
fn char_in_ranges(ch: u8, ranges: &[CharRange], case_insensitive: bool) -> bool {
    for range in ranges {
        match range {
            CharRange::Single(c) => {
                if case_insensitive {
                    if ch.to_ascii_lowercase() == c.to_ascii_lowercase() {
                        return true;
                    }
                } else if ch == *c {
                    return true;
                }
            }
            CharRange::Range(start, end) => {
                if case_insensitive {
                    let ch_lower = ch.to_ascii_lowercase();
                    let start_lower = start.to_ascii_lowercase();
                    let end_lower = end.to_ascii_lowercase();
                    if ch_lower >= start_lower && ch_lower <= end_lower {
                        return true;
                    }
                    // Also check uppercase range
                    let ch_upper = ch.to_ascii_uppercase();
                    let start_upper = start.to_ascii_uppercase();
                    let end_upper = end.to_ascii_uppercase();
                    if ch_upper >= start_upper && ch_upper <= end_upper {
                        return true;
                    }
                } else if ch >= *start && ch <= *end {
                    return true;
                }
            }
        }
    }
    false
}

fn is_word_char(ch: u8, _case_insensitive: bool) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
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
    if error.starts_with("Delimiter must") || error.starts_with("No ending delimiter") || error.starts_with("No ending matching delimiter") || error.starts_with("Unknown modifier") || error.starts_with("Empty regular expression") {
        format!("{}: {}", func_name, error)
    } else {
        format!("{}: Compilation failed: {}", func_name, error)
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
            b'i' => flags.case_insensitive = true,
            b'm' => flags.multiline = true,
            b's' => flags.dotall = true,
            b'x' => flags.extended = true,
            b'U' => flags.ungreedy = true,
            b'u' => flags.utf8 = true,
            b'D' => {} // Dollar end only — ignore
            b'A' => flags.anchored = true,
            b'S' => {} // Extra study — ignore
            b'X' => {} // Extra — ignore
            b'J' => {} // Allow duplicate names — ignore
            b'n' => {} // No auto capture — ignore
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

        for start_pos in start_offset..end {
            state.captures = vec![None; self.num_groups + 1];
            state.step_count = 0;
            if let Some(end_pos) = match_node_backtrack(&self.ast, &mut state, start_pos) {
                state.captures[0] = Some((start_pos, end_pos));
                return Some(RegexMatch {
                    full_match: (start_pos, end_pos),
                    groups: state.captures,
                });
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
                    offset = end + 1;
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

/// preg_match($pattern, $subject [, &$matches [, $flags [, $offset]]])
pub fn preg_match(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.preg_last_error = 0;
    let pattern = match args.first() {
        Some(v) => v.to_php_string(),
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
        return Ok(Value::False);
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
        Some(v) => v.to_php_string(),
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
                    off = end + 1;
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
                // Zero-length match: copy one char and advance
                if offset < subject.len() {
                    result.push(subject[offset]);
                }
                offset += 1;
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
        Some(v) => v.to_php_string(),
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
                // Zero-length match: advance search position past the match to avoid infinite loop
                last_split_pos = match_end;
                if match_end < input.len() {
                    search_offset = match_end + 1;
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
        for (key, subject) in subjects_arr.borrow().iter() {
            let subject_str = subject.to_php_string();
            let mut current = subject_str.as_bytes().to_vec();
            for pat in &patterns {
                let (replaced, count) = do_preg_replace_callback(vm, pat, &callback, &current, limit)?;
                current = replaced;
                total_count += count;
            }
            result.set(key.clone(), Value::String(PhpString::from_vec(current)));
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
        let (replaced, count) = do_preg_replace_callback(vm, pat, &callback, &current, limit)?;
        current = replaced;
        total_count += count;
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
) -> Result<(Vec<u8>, i64), VmError> {
    let compiled = match parse_php_regex(pattern) {
        Ok(c) => c,
        Err(_e) => {
            vm.emit_warning(&format_preg_error("preg_replace_callback()", &_e));
            return Ok((subject.to_vec(), 0));
        }
    };

    // Validate UTF-8 when /u modifier is used
    if compiled.flags.utf8 && std::str::from_utf8(subject).is_err() {
        vm.preg_last_error = 4; // PREG_BAD_UTF8_ERROR
        return Ok((subject.to_vec(), 0));
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
                if let Some((start, end)) = capture {
                    matches_arr.set(
                        ArrayKey::Int(i as i64),
                        Value::String(PhpString::from_bytes(&subject[*start..*end])),
                    );
                } else {
                    matches_arr.set(
                        ArrayKey::Int(i as i64),
                        Value::String(PhpString::empty()),
                    );
                }
            }

            // Call the callback
            let matches_val = Value::Array(Rc::new(RefCell::new(matches_arr)));
            let replacement = call_callback(vm, callback, &[matches_val])?;
            let replacement_str = replacement.to_php_string();
            result.extend_from_slice(replacement_str.as_bytes());

            count += 1;

            if match_end == offset {
                if offset < subject.len() {
                    result.push(subject[offset]);
                }
                offset += 1;
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

    Ok((result, count))
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

    // Handle array subject
    if let Value::Array(subjects_arr) = &subject_val {
        let mut result = PhpArray::new();
        let mut total_count = 0i64;
        for (key, subject) in subjects_arr.borrow().iter() {
            let subject_str = subject.to_php_string();
            let mut current = subject_str.as_bytes().to_vec();
            for (_, pair_val) in patterns_callbacks.borrow().iter() {
                // Key is pattern, value is callback — but actually in PHP, the array is pattern => callback
                // We need to iterate patterns_callbacks differently
                let _ = pair_val; // handled below
            }
            // Re-iterate properly
            for (pat_key, cb) in patterns_callbacks.borrow().iter() {
                let pat_str = match pat_key {
                    ArrayKey::String(s) => s.as_bytes().to_vec(),
                    ArrayKey::Int(_) => continue,
                };
                let (replaced, count) = do_preg_replace_callback(vm, &pat_str, &cb, &current, limit)?;
                current = replaced;
                total_count += count;
            }
            result.set(key.clone(), Value::String(PhpString::from_vec(current)));
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
    for (pat_key, cb) in patterns_callbacks.borrow().iter() {
        let pat_str = match pat_key {
            ArrayKey::String(s) => s.as_bytes().to_vec(),
            ArrayKey::Int(_) => continue,
        };
        let (replaced, count) = do_preg_replace_callback(vm, &pat_str, &cb, &current, limit)?;
        current = replaced;
        total_count += count;
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
        Some(v) => v.to_php_string(),
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
    for (key, value) in array.borrow().iter() {
        let subject = value.to_php_string();
        let matches = compiled.find(subject.as_bytes(), 0).is_some();
        if matches != invert {
            result.set(key.clone(), value.clone());
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
