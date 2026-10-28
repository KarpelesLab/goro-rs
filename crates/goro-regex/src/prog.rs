//! The compiled form executed by the backtracking matcher.

use crate::ast::{Assertion, Greed, Newline};
use crate::class::CharSet;
use std::sync::Arc;

pub type Pc = u32;

/// A matcher for exactly one character (or code unit).
#[derive(Clone, Debug)]
pub enum Item {
    /// One byte (a character below 128, or any byte in non-UTF mode).
    Byte(u8),
    /// Either of two bytes (ASCII caseless).
    Byte2(u8, u8),
    /// One of a few code points (UTF caseless characters).
    Chars(Arc<[u32]>),
    /// A class.
    Set(Arc<CharSet>),
    /// Any character (`.` with dotall).
    Any,
    /// Any character except a newline (`.`, `\N`).
    AnyNoNl,
    /// One code unit (`\C`).
    CodeUnit,
}

/// What kind of scope a lookaround barrier opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookKind {
    Positive,
    Negative,
    /// Non-atomic positive (`(*napla:`).
    NonAtomic,
    /// Assertion used as a condition: `negated` selects (?! / (?<!.
    Cond {
        negated: bool,
    },
}

#[derive(Clone, Debug)]
pub enum Inst {
    /// Match one character with an item.
    One(Item),
    /// A literal byte string (case-sensitive), index into `Prog::lits`.
    Lit(u32),
    /// An ASCII-caseless literal (stored lowercased), index into `Prog::lits`.
    LitFold(u32),
    /// Repeat an item.
    Rep {
        item: Item,
        min: u32,
        max: u32,
        greed: Greed,
    },
    /// `\R`
    Newline {
        anycrlf: bool,
    },
    /// `\X`
    Grapheme,
    Assert(Assertion),
    /// Push a choice to `alt`, continue with the next instruction (an
    /// optional or repeated group: one frame).
    Split {
        alt: Pc,
    },
    /// The next branch of an alternation; `count`: trying it counts
    /// towards the match limit.
    Branch {
        alt: Pc,
        count: bool,
    },
    /// Push a choice to the next instruction, continue at `to` (lazy).
    SplitLazy {
        to: Pc,
    },
    /// Alternative of a group containing (*THEN): like Split but tagged.
    SplitTag {
        alt: Pc,
        tag: u32,
    },
    /// Marks entry into an alternation group whose branches contain (*THEN).
    ThenScope {
        tag: u32,
    },
    Jmp(Pc),
    /// Records the start of capture group n.
    CapOpen(u32),
    /// Ends capture group n (or returns from a call into group n).
    CapClose(u32),
    /// A frame-only marker that counts towards the match and depth limits
    /// like PCRE2's group entry.
    GroupFrame,
    /// Records the iteration start of a group loop.
    LoopStart {
        slot: u32,
    },
    /// End of a loop iteration: exits on an empty iteration, otherwise
    /// loops back to `start` (greedy) or offers it as a choice (lazy).
    LoopEnd {
        slot: u32,
        start: Pc,
        greedy: bool,
    },
    AtomicStart,
    AtomicEnd,
    /// Starts a lookaround; `end` is the matching LookEnd.
    LookStart {
        kind: LookKind,
        end: Pc,
    },
    /// Lookbehind branch: step back between min and max characters.
    Behind {
        min: u32,
        max: u32,
    },
    /// End of a lookbehind branch: the position must be back where the
    /// lookbehind started.
    BehindCheck,
    /// End of a lookaround; `no` is the no-branch of a conditional.
    LookEnd {
        kind: LookKind,
        no: Pc,
    },
    Backref {
        groups: Arc<[u32]>,
        caseless: bool,
        restrict: bool,
    },
    Call {
        group: u32,
    },
    /// Condition: any of the groups set.
    CondRef {
        groups: Arc<[u32]>,
        no: Pc,
    },
    /// Condition: recursion (None = any recursion).
    CondRecurse {
        groups: Option<Arc<[u32]>>,
        no: Pc,
    },
    /// Verbs. Names index `Prog::names`.
    Mark(u32),
    Commit,
    Prune,
    Skip,
    SkipName(u32),
    Then {
        tag: u32,
    },
    /// (*ACCEPT): closes the listed capture groups first.
    Accept {
        close: Arc<[u32]>,
    },
    Fail,
    /// Sets the mark name without making it visible to (*SKIP:NAME)
    /// (the names of (*PRUNE:NAME) and friends).
    SetMarkName(u32),
    /// `\K`
    Keep,
    /// Script runs.
    ScriptRunStart,
    ScriptRunEnd,
    /// Scan substring: run the body anchored on the first set group.
    ScanStart {
        groups: Arc<[u32]>,
        end: Pc,
    },
    ScanEnd,
    Match,
}

/// One step of a deterministic pattern (see [`OnePass`]).
#[derive(Clone, Debug)]
pub enum OpStep {
    One(Item),
    Lit(u32),
    LitFold(u32),
    Rep {
        item: Item,
        min: u32,
        max: u32,
    },
    /// The single backtracking point: a greedy or lazy repeat.
    BackRep {
        item: Item,
        min: u32,
        max: u32,
        lazy: bool,
    },
    Open(u32),
    Close(u32),
    Assert(Assertion),
    Keep,
    /// A positive lookahead whose body is the next `n` steps (characters,
    /// literals, possessive repeats and assertions only).
    Look(u32),
}

/// A pattern that never needs to backtrack: a straight sequence of
/// characters, literals, possessive (or provably deterministic) repeats,
/// capture groups and assertions. Matched without the backtracking VM.
#[derive(Clone, Debug)]
pub struct OnePass {
    pub steps: Vec<OpStep>,
    /// The frames PCRE2 would use at most for an attempt (the fast path is
    /// only taken when the limits exceed this).
    pub frames: u32,
    /// Some greedy repeats were treated as deterministic; PCRE2 would count
    /// one frame per character they give back.
    pub has_greedy: bool,
    /// The first step (after group openings) is an unbounded repeat of a
    /// single-byte item with min >= 1 and nothing later depends on the
    /// start position: after a failure at p, starts inside the same run
    /// fail too.
    pub lead_run: Option<usize>,
    /// The leading run has a maximum (possessive, with `lead_lit`): only
    /// the literal search uses it.
    pub lead_bounded: bool,
    /// Index of the [`OpStep::BackRep`], if any.
    pub back: Option<usize>,
    /// With `lead_run`: the literal that follows the run (for a possessive
    /// run, one the run cannot match).
    pub lead_lit: Option<Vec<u8>>,
    /// Optional possessive repeats between the leading run and `lead_lit`.
    pub lead_mid: Vec<OpStep>,
    pub lead_finder: Option<memchr::memmem::Finder<'static>>,
    /// Lookahead steps, and those before the backtracking repeat (each
    /// costs PCRE2 one frame while it runs).
    pub looks: u32,
    pub pre_looks: u32,
}

/// Start-of-match optimisation data (pcre2api: "start-up optimizations").
#[derive(Clone, Debug, Default)]
pub struct StartInfo {
    /// The match can only start at the start offset (or line starts for
    /// multiline ^).
    pub anchored: bool,
    /// Every match starts after a newline or at the subject start (multiline
    /// `^` in every branch).
    pub startline: bool,
    /// A literal every match starts with.
    pub prefix: Vec<u8>,
    /// Whether the prefix is ASCII-caseless.
    pub prefix_fold: bool,
    /// Possible first bytes of a match.
    pub first_bytes: Option<Box<[bool; 256]>>,
    /// A byte that must appear somewhere at or after the match start
    /// (PCRE2's "last literal" check), with a caseless alternative.
    pub required: Option<(u8, u8)>,
    /// The minimum match length in code units.
    pub min_len: usize,
    /// Searcher for `prefix`.
    pub finder: Option<memchr::memmem::Finder<'static>>,
    /// Byte sets every match has at offsets 0, 1, ... from its start
    /// (only when there is no literal prefix).
    pub sets: Vec<[u64; 4]>,
    /// Which of `sets` to search for, and its bytes when there are at most
    /// three.
    pub anchor: Option<(usize, Vec<u8>)>,
    /// A word-boundary assertion every match starts with.
    pub lead_wb: Option<(bool, bool)>,
    /// How to scan for `first_bytes`.
    pub first_scan: FirstScan,
}

/// Accelerated scans for the first byte of a match.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FirstScan {
    #[default]
    Table,
    One(u8),
    Two(u8, u8),
    Three(u8, u8, u8),
    /// Only bytes >= 0x80 can start a match.
    HighBit,
}

#[derive(Debug)]
pub struct Prog {
    pub insts: Vec<Inst>,
    pub lits: Vec<Box<[u8]>>,
    pub names: Vec<Arc<[u8]>>,
    /// Start pc of each capture group (index = group number; 0 = pattern).
    pub group_pc: Vec<Pc>,
    pub ncap: u32,
    /// Number of loop slots.
    pub nloops: u32,
    pub utf: bool,
    pub ucp: bool,
    pub newline: Newline,
    pub start: StartInfo,
    /// The longest lookbehind in characters (for UTF checks with offsets).
    pub max_lookbehind: u32,
    pub has_verbs: bool,
    pub notempty: bool,
    pub notempty_atstart: bool,
    pub limit_match: Option<u32>,
    pub limit_depth: Option<u32>,
    pub limit_heap: Option<u32>,
    pub match_unset_backref: bool,
    pub turkish: bool,
    /// The pattern contains an explicit CR or LF (pcre2api: CRLF bumpalong).
    pub has_cr_lf: bool,
    pub onepass: Option<OnePass>,
}
