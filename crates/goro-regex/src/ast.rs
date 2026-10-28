//! The parsed form of a pattern. Option settings such as caseless or
//! multiline are resolved into the nodes while parsing (pcre2pattern: "the
//! effects of option settings happen at compile time").

use crate::class::CharSet;
use std::sync::Arc;

/// A newline convention (pcre2pattern: "Newline conventions").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Newline {
    Cr,
    Lf,
    CrLf,
    AnyCrLf,
    Any,
    Nul,
}

/// Quantifier flavour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Greed {
    Greedy,
    Lazy,
    Possessive,
}

/// Simple assertions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assertion {
    /// `\A`
    StartSubject,
    /// `^` (`multiline` resolved).
    Circumflex { multiline: bool },
    /// `$` (`multiline`, `dollar_endonly` resolved).
    Dollar { multiline: bool, endonly: bool },
    /// `\Z`
    EndSubjectOrNewline,
    /// `\z`
    EndSubject,
    /// `\b` / `\B`; `ucp` selects the Unicode definition of \w.
    WordBoundary { negated: bool, ucp: bool },
    /// `\G`
    StartOfMatch,
}

/// Kinds of parenthesised groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupKind {
    NonCapture,
    Capture(u32),
    Atomic,
    LookAhead { negated: bool, atomic: bool },
    LookBehind { negated: bool, atomic: bool },
    ScriptRun { atomic: bool },
}

/// Backtracking control verbs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verb {
    Accept,
    Fail,
    Mark,
    Commit,
    Prune,
    Skip,
    /// `(*SKIP:NAME)`: skip to the most recent `(*MARK:NAME)`.
    SkipName,
    Then,
}

/// A reference to a group by number or name, resolved after parsing.
#[derive(Clone, Debug)]
pub struct GroupRef {
    /// The group number (0 until a name is resolved).
    pub num: u32,
    pub name: Option<Arc<[u8]>>,
    /// Where PCRE2 reports a reference to a non-existent group.
    pub offset: usize,
    /// All groups the reference designates (duplicate names give several),
    /// filled in by the resolver.
    pub groups: Vec<u32>,
}

impl GroupRef {
    pub fn num(num: u32, offset: usize) -> GroupRef {
        GroupRef {
            num,
            name: None,
            offset,
            groups: Vec::new(),
        }
    }
    pub fn name(name: &[u8], offset: usize) -> GroupRef {
        GroupRef {
            num: 0,
            name: Some(Arc::from(name)),
            offset,
            groups: Vec::new(),
        }
    }
}

/// Conditions of `(?(condition)yes|no)`.
#[derive(Clone, Debug)]
pub enum Condition {
    /// True if any of the groups has been set.
    GroupSet(GroupRef),
    /// `(?(R)`: in any recursion; `(?(Rn)`, `(?(R&name)`: most recent
    /// recursion is into one of these groups.
    Recursion(Option<GroupRef>),
    /// `(?(DEFINE)`: always false.
    Define,
    /// `(?(VERSION...)`: evaluated at compile time.
    Version(bool),
    /// An assertion group.
    Assert(Box<Node>),
}

/// A pattern node.
#[derive(Clone, Debug)]
pub enum Node {
    Empty,
    /// One character; `caseless` already accounts for whether the character
    /// has other cases.
    Char {
        c: u32,
        caseless: bool,
    },
    /// `.` (`dotall` resolved).
    Dot {
        dotall: bool,
    },
    /// `\N` (never matches a newline).
    NotNewline,
    /// `\C`
    CodeUnit,
    /// A class or character type.
    Set(Arc<CharSet>),
    /// `\R`
    Newline {
        anycrlf: bool,
    },
    /// `\X`
    Grapheme,
    Assert(Assertion),
    /// `\K`
    Keep,
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Group {
        kind: GroupKind,
        body: Box<Node>,
        offset: usize,
    },
    Repeat {
        node: Box<Node>,
        min: u32,
        max: Option<u32>,
        greed: Greed,
    },
    /// Backreference to the first set group of `groups` (duplicate names give
    /// several).
    Backref {
        target: GroupRef,
        caseless: bool,
        case_restrict: bool,
    },
    /// Subroutine call or recursion (0 = whole pattern).
    Call {
        target: GroupRef,
    },
    /// `(?(cond)yes|no)`; `branches` counts the alternatives as written (for
    /// the "more than two branches" check), `offset` is the group start.
    Cond {
        cond: Condition,
        yes: Box<Node>,
        no: Option<Box<Node>>,
        branches: usize,
        offset: usize,
    },
    /// `(*scan_substring:(groups)...)`
    ScanSubstring {
        groups: Vec<GroupRef>,
        body: Box<Node>,
    },
    Verb {
        verb: Verb,
        name: Option<Arc<[u8]>>,
    },
}

impl Node {
    pub fn seq(mut v: Vec<Node>) -> Node {
        match v.len() {
            0 => Node::Empty,
            1 => v.pop().unwrap(),
            _ => Node::Seq(v),
        }
    }
}
