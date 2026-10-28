//! The backtracking matcher.
//!
//! Choice points, undo records for captures and other registers, and scope
//! barriers (atomic groups, lookarounds, subroutine calls) all live on one
//! explicit stack, so deep patterns need heap, not native stack. The match
//! and depth limits count "frames" the way pcre2api describes PCRE2's
//! interpreter: one per backtracking point.

use crate::ast::{Assertion, Greed, Newline};
use crate::class;
use crate::error::MatchError;
use crate::prog::*;
use crate::unicode;
use crate::utf8;
use std::sync::Arc;

const UNSET: usize = usize::MAX;
const NO_MARK: usize = u32::MAX as usize;

// Backtrack entry kinds.
const K_SLOT: u32 = 0;
const K_ALT: u32 = 1;
const K_ALT_TAG: u32 = 2;
const K_REP_GREEDY: u32 = 3;
const K_REP_LAZY: u32 = 4;
const K_FRAME: u32 = 5;
const K_THEN_SCOPE: u32 = 6;
const K_ATOMIC: u32 = 7;
const K_LOOK: u32 = 8;
const K_BEHIND: u32 = 9;
const K_MARK: u32 = 10;
const K_MARK_UNDO: u32 = 11;
/// A (*MARK) inside a completed atomic group or assertion: still a frame
/// and an undo record, no longer visible to (*SKIP:NAME).
const K_MARK_HID: u32 = 20;
/// Emulated JIT stack use only.
const K_JIT8: u32 = 21;
const K_VERB: u32 = 12;
const K_CALL_PUSH: u32 = 13;
const K_CALL_POP: u32 = 14;
const K_SR: u32 = 15;
const K_SCAN: u32 = 16;
/// A branch alternative: not a frame while pending; trying it counts as
/// one (PCRE2 tries each branch of a group with a new backtracking frame).
const K_ALT_BR: u32 = 17;
/// A branch alternative of a non-capturing group (not counted).
const K_ALT_NC: u32 = 18;
/// A lazy quantifier's "one more" alternative: a frame while pending, and
/// trying it counts again.
const K_ALT_LAZY: u32 = 19;

// Verb codes in K_VERB.a
const V_COMMIT: u32 = 0;
const V_PRUNE: u32 = 1;
const V_SKIP: u32 = 2;
const V_SKIPNAME: u32 = 3;
const V_THEN: u32 = 4;

#[inline(always)]
fn is_undo(kind: u32) -> bool {
    matches!(
        kind,
        K_SLOT | K_MARK | K_MARK_UNDO | K_MARK_HID | K_CALL_PUSH | K_CALL_POP
    )
}

#[inline(always)]
fn is_frame(kind: u32) -> bool {
    matches!(
        kind,
        K_ALT
            | K_ALT_LAZY
            | K_CALL_PUSH
            | K_MARK
            | K_MARK_HID
            | K_REP_GREEDY
            | K_REP_LAZY
            | K_FRAME
            | K_ATOMIC
            | K_LOOK
            | K_BEHIND
            | K_VERB
            | K_SCAN
    )
}

#[derive(Clone, Copy, Debug)]
struct Bt {
    kind: u32,
    a: u32,
    b: usize,
    c: usize,
}

#[derive(Clone, Copy, Debug)]
struct CallFrame {
    group: u32,
    ret_pc: Pc,
    arena_off: usize,
    entry_pos: usize,
    bt_height: usize,
}

/// Matching limits (PHP's pcre.backtrack_limit and pcre.recursion_limit).
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub match_limit: u32,
    pub depth_limit: u32,
    /// Emulates PCRE2's JIT: the depth limit is not used; instead the
    /// backtracking state is charged against a stack of this many bytes
    /// and running out gives [`MatchError::JitStackLimit`]. The charges
    /// reproduce the limits observed on PHP 8.5 with pcre.jit=1 (a 192 KiB
    /// stack: /(a)*/ handles 8190 characters, /(?:a)*$/ 24574).
    pub jit_stack: Option<u32>,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            match_limit: 10_000_000,
            depth_limit: 10_000_000,
            jit_stack: None,
        }
    }
}

impl Limits {
    /// The interpreter's limits (pcre.jit=0).
    pub fn new(match_limit: u32, depth_limit: u32) -> Limits {
        Limits {
            match_limit,
            depth_limit,
            jit_stack: None,
        }
    }

    /// PHP's JIT stack size (pcre.jit=1), in bytes.
    pub const PHP_JIT_STACK: u32 = 192 * 1024;
}

/// Bytes of emulated JIT stack an entry holds.
#[inline(always)]
fn jit_cost(e: &Bt) -> u64 {
    match e.kind {
        K_ALT | K_ALT_LAZY | K_JIT8 => 8,
        K_ALT_BR | K_ALT_NC | K_ALT_TAG => 16,
        K_REP_GREEDY | K_REP_LAZY => 16,
        K_FRAME => e.a as u64,
        K_ATOMIC | K_LOOK | K_BEHIND | K_VERB => 8,
        K_MARK | K_MARK_HID | K_SCAN => 16,
        K_CALL_PUSH => 32,
        _ => 0,
    }
}

/// Options for one match call (pcre2api: pcre2_match options).
#[derive(Clone, Copy, Debug, Default)]
pub struct MatchOptions {
    pub notbol: bool,
    pub noteol: bool,
    pub notempty: bool,
    pub notempty_atstart: bool,
    pub anchored: bool,
    pub no_utf_check: bool,
}

/// Reusable matching state.
#[derive(Debug, Default)]
pub struct Scratch {
    bt: Vec<Bt>,
    slots: Vec<usize>,
    calls: Vec<CallFrame>,
    popped: Vec<CallFrame>,
    arena: Vec<usize>,
}

impl Scratch {
    pub fn new() -> Scratch {
        Scratch::default()
    }
}

/// The outcome of one match attempt at one start position.
enum Attempt {
    Match(usize, usize),
    NoMatch,
    Commit,
    Prune,
    Skip(usize),
}

/// Result data of a successful match.
pub struct MatchData {
    pub start: usize,
    pub end: usize,
    pub mark: Option<Arc<[u8]>>,
}

struct Vm<'p, 's> {
    prog: &'p Prog,
    s: &'s [u8],
    end: usize,
    utf: bool,
    start_offset: usize,
    /// Start of the current attempt.
    attempt_start: usize,
    opts: MatchOptions,
    limits: Limits,
    count: u64,
    frames: u32,
    /// Emulated JIT stack limit (0: off) and use.
    jit_limit: u64,
    jit_bytes: u64,
    mark: usize,
    last_mark: usize,
    // Slot layout.
    open_base: usize,
    loop_base: usize,
    keep_slot: usize,
    nslots: usize,
}

macro_rules! fail {
    ($self:ident, $sc:ident, $pc:ident, $pos:ident) => {{
        match $self.backtrack($sc)? {
            Step::Resume(p, q) => {
                $pc = p;
                $pos = q;
                continue;
            }
            Step::Done(a) => return Ok(a),
        }
    }};
}

enum Step {
    Resume(Pc, usize),
    Done(Attempt),
}

enum VerbAct {
    Commit,
    Prune,
    Skip(usize),
    Then(usize),
}

impl Vm<'_, '_> {
    #[inline(always)]
    fn push(&mut self, sc: &mut Scratch, kind: u32, a: u32, b: usize, c: usize) {
        sc.bt.push(Bt { kind, a, b, c });
    }

    #[inline(always)]
    fn push_frame(
        &mut self,
        sc: &mut Scratch,
        kind: u32,
        a: u32,
        b: usize,
        c: usize,
    ) -> Result<(), MatchError> {
        self.count += 1;
        self.frames += 1;
        if self.count > self.limits.match_limit as u64 {
            return Err(MatchError::MatchLimit);
        }
        if self.frames > self.limits.depth_limit {
            return Err(MatchError::DepthLimit);
        }
        let e = Bt { kind, a, b, c };
        if self.jit_limit != 0 {
            self.jit_bytes += jit_cost(&e);
            if self.jit_bytes >= self.jit_limit {
                return Err(MatchError::JitStackLimit);
            }
        }
        sc.bt.push(e);
        Ok(())
    }

    /// Pushes a branch alternative (not a frame).
    #[inline(always)]
    fn push_alt(
        &mut self,
        sc: &mut Scratch,
        kind: u32,
        a: u32,
        b: usize,
        c: usize,
    ) -> Result<(), MatchError> {
        let e = Bt { kind, a, b, c };
        if self.jit_limit != 0 {
            self.jit_bytes += jit_cost(&e);
            if self.jit_bytes >= self.jit_limit {
                return Err(MatchError::JitStackLimit);
            }
        }
        sc.bt.push(e);
        Ok(())
    }

    /// Accounting for an entry leaving the stack.
    #[inline(always)]
    fn release(&mut self, e: &Bt) {
        if is_frame(e.kind) {
            self.frames -= 1;
        }
        if self.jit_limit != 0 {
            self.jit_bytes -= jit_cost(e);
        }
    }

    #[inline(always)]
    fn set_slot(&mut self, sc: &mut Scratch, i: usize, v: usize) {
        let old = sc.slots[i];
        if old != v {
            sc.bt.push(Bt {
                kind: K_SLOT,
                a: i as u32,
                b: old,
                c: 0,
            });
            sc.slots[i] = v;
        }
    }

    // -----------------------------------------------------------------------
    // Character helpers

    #[inline(always)]
    fn decode(&self, pos: usize) -> (u32, usize) {
        if self.utf {
            utf8::decode(&self.s[..self.end], pos)
        } else {
            (self.s[pos] as u32, 1)
        }
    }

    /// Length of a newline at `pos`, if any (the pattern's newline
    /// convention).
    #[inline]
    fn newline_at(&self, pos: usize) -> Option<usize> {
        if pos >= self.end {
            return None;
        }
        let b = self.s[pos];
        match self.prog.newline {
            Newline::Lf => (b == b'\n').then_some(1),
            Newline::Cr => (b == b'\r').then_some(1),
            Newline::Nul => (b == 0).then_some(1),
            Newline::CrLf => {
                (b == b'\r' && pos + 1 < self.end && self.s[pos + 1] == b'\n').then_some(2)
            }
            Newline::AnyCrLf => match b {
                b'\r' if pos + 1 < self.end && self.s[pos + 1] == b'\n' => Some(2),
                b'\r' | b'\n' => Some(1),
                _ => None,
            },
            Newline::Any => match b {
                b'\r' if pos + 1 < self.end && self.s[pos + 1] == b'\n' => Some(2),
                0x0a..=0x0d => Some(1),
                0x85 if !self.utf => Some(1),
                0xc2 if self.utf => (pos + 1 < self.end && self.s[pos + 1] == 0x85).then_some(2),
                0xe2 if self.utf => (pos + 2 < self.end
                    && self.s[pos + 1] == 0x80
                    && matches!(self.s[pos + 2], 0xa8 | 0xa9))
                .then_some(3),
                _ => None,
            },
        }
    }

    /// Whether a newline ends exactly at `pos`.
    #[inline]
    fn newline_before(&self, pos: usize) -> bool {
        if pos == 0 {
            return false;
        }
        match self.prog.newline {
            Newline::Lf => self.s[pos - 1] == b'\n',
            Newline::Cr => self.s[pos - 1] == b'\r',
            Newline::Nul => self.s[pos - 1] == 0,
            Newline::CrLf => pos >= 2 && self.s[pos - 2] == b'\r' && self.s[pos - 1] == b'\n',
            Newline::AnyCrLf => {
                let b = self.s[pos - 1];
                if b == b'\n' {
                    true
                } else if b == b'\r' {
                    // A CR followed by LF is not the end of a newline.
                    !(pos < self.end && self.s[pos] == b'\n')
                } else {
                    false
                }
            }
            Newline::Any => {
                let b = self.s[pos - 1];
                match b {
                    b'\n' | 0x0b | 0x0c => true,
                    b'\r' => !(pos < self.end && self.s[pos] == b'\n'),
                    0x85 if !self.utf => true,
                    0x85 if self.utf => pos >= 2 && self.s[pos - 2] == 0xc2,
                    0xa8 | 0xa9 if self.utf => {
                        pos >= 3 && self.s[pos - 3] == 0xe2 && self.s[pos - 2] == 0x80
                    }
                    _ => false,
                }
            }
        }
    }

    /// Matches one item at `pos`, returning the new position.
    #[inline(always)]
    fn match_item(&self, item: &Item, pos: usize) -> Option<usize> {
        if pos >= self.end {
            return None;
        }
        let b = self.s[pos];
        match item {
            Item::Byte(x) => (b == *x).then_some(pos + 1),
            Item::Byte2(x, y) => (b == *x || b == *y).then_some(pos + 1),
            Item::Chars(v) => {
                let (c, l) = self.decode(pos);
                v.contains(&c).then_some(pos + l)
            }
            Item::Set(set) => {
                if b < 0x80 || !self.utf {
                    set.has_byte(b).then_some(pos + 1)
                } else {
                    let (c, l) = utf8::decode(&self.s[..self.end], pos);
                    set.matches(c).then_some(pos + l)
                }
            }
            Item::Any => {
                if self.utf {
                    Some((pos + utf8::len_from_lead(b)).min(self.end))
                } else {
                    Some(pos + 1)
                }
            }
            Item::AnyNoNl => {
                if self.is_nl_char(pos) {
                    None
                } else if self.utf {
                    Some((pos + utf8::len_from_lead(b)).min(self.end))
                } else {
                    Some(pos + 1)
                }
            }
            Item::CodeUnit => Some(pos + 1),
        }
    }

    /// Whether the character at `pos` is a newline for `.`/`\N` purposes
    /// (pcre2pattern: with CRLF, dot does not match CR followed by LF).
    #[inline(always)]
    fn is_nl_char(&self, pos: usize) -> bool {
        let b = self.s[pos];
        match self.prog.newline {
            Newline::Lf => b == b'\n',
            Newline::Cr => b == b'\r',
            Newline::Nul => b == 0,
            Newline::CrLf => b == b'\r' && pos + 1 < self.end && self.s[pos + 1] == b'\n',
            Newline::AnyCrLf => b == b'\r' || b == b'\n',
            Newline::Any => self.newline_at(pos).is_some(),
        }
    }

    /// Whether `item` matches exactly one byte (for fast repeat loops and
    /// simple backtracking).
    #[inline(always)]
    fn item_single_byte(&self, item: &Item) -> bool {
        !self.utf
            || match item {
                Item::Byte(_) | Item::Byte2(..) | Item::CodeUnit => true,
                Item::Set(s) => s.ascii_only(),
                _ => false,
            }
    }

    /// Consumes up to `max` items from `pos`; returns (new pos, count).
    #[inline]
    fn scan_items(&self, item: &Item, mut pos: usize, max: u32) -> (usize, u32) {
        let s = self.s;
        let end = self.end;
        let limit = if max == u32::MAX {
            usize::MAX
        } else {
            max as usize
        };
        if self.item_single_byte(item) {
            let stop = if limit == usize::MAX {
                end
            } else {
                end.min(pos.saturating_add(limit))
            };
            let start = pos;
            match item {
                Item::Byte(x) => {
                    while pos < stop && s[pos] == *x {
                        pos += 1;
                    }
                }
                Item::Byte2(x, y) => {
                    while pos < stop && (s[pos] == *x || s[pos] == *y) {
                        pos += 1;
                    }
                }
                Item::Set(set) => {
                    let lut = &set.lut;
                    pos += s[pos..stop]
                        .iter()
                        .position(|&b| !lut[b as usize])
                        .unwrap_or(stop - pos);
                }
                Item::Any | Item::CodeUnit => pos = stop,
                Item::AnyNoNl => {
                    if self.prog.newline == Newline::Lf {
                        pos = match memchr::memchr(b'\n', &s[pos..stop]) {
                            Some(k) => pos + k,
                            None => stop,
                        };
                    } else {
                        while pos < stop && !self.is_nl_char(pos) {
                            pos += 1;
                        }
                    }
                }
                Item::Chars(v) => {
                    while pos < stop && v.contains(&(s[pos] as u32)) {
                        pos += 1;
                    }
                }
            }
            return (pos, (pos - start) as u32);
        }
        // Multi-byte items in UTF mode.
        let mut n: u32 = 0;
        if let Item::Any = item
            && max == u32::MAX
        {
            // Count is not needed by callers for unbounded Any beyond min
            // checks; still count characters for correctness of min.
            while pos < end {
                pos += utf8::len_from_lead(s[pos]);
                n += 1;
            }
            return (pos.min(end), n);
        }
        if let Item::Set(set) = item {
            let lut = &set.lut;
            while (n as usize) < limit && pos < end {
                let b = s[pos];
                if b < 0x80 {
                    if !lut[b as usize] {
                        break;
                    }
                    pos += 1;
                } else {
                    let (c, l) = utf8::decode(&s[..end], pos);
                    if !set.matches(c) {
                        break;
                    }
                    pos += l;
                }
                n += 1;
            }
            return (pos, n);
        }
        while (n as usize) < limit {
            match self.match_item(item, pos) {
                Some(p) => {
                    pos = p;
                    n += 1;
                }
                None => break,
            }
        }
        (pos, n)
    }

    fn is_word_at(&self, pos: usize, ucp: bool) -> bool {
        let (c, _) = self.decode(pos);
        if ucp {
            class::is_ucp_word(c)
        } else {
            class::is_ascii_word(c)
        }
    }

    fn is_word_before(&self, pos: usize, ucp: bool) -> bool {
        let c = if self.utf {
            utf8::decode_before(self.s, pos).0
        } else {
            self.s[pos - 1] as u32
        };
        if ucp {
            class::is_ucp_word(c)
        } else {
            class::is_ascii_word(c)
        }
    }

    fn check_assert(&self, a: Assertion, pos: usize) -> bool {
        match a {
            Assertion::StartSubject => pos == 0,
            Assertion::Circumflex { multiline } => {
                if pos == 0 {
                    return !self.opts.notbol;
                }
                multiline && pos < self.end && self.newline_before(pos)
                    || multiline && pos == self.end && false
            }
            Assertion::Dollar { multiline, endonly } => {
                if multiline {
                    if pos == self.end {
                        return !self.opts.noteol;
                    }
                    return self.newline_at(pos).is_some();
                }
                if pos == self.end {
                    return !self.opts.noteol;
                }
                if endonly || self.opts.noteol {
                    return false;
                }
                matches!(self.newline_at(pos), Some(l) if pos + l == self.end)
            }
            Assertion::EndSubjectOrNewline => {
                pos == self.end || matches!(self.newline_at(pos), Some(l) if pos + l == self.end)
            }
            Assertion::EndSubject => pos == self.end,
            Assertion::WordBoundary { negated, ucp } => {
                let before = pos > 0 && self.is_word_before(pos, ucp);
                let after = pos < self.end && self.is_word_at(pos, ucp);
                (before != after) != negated
            }
            Assertion::StartOfMatch => pos == self.start_offset,
        }
    }

    /// `\R` at pos.
    fn match_newline_seq(&self, pos: usize, anycrlf: bool) -> Option<usize> {
        if pos >= self.end {
            return None;
        }
        let b = self.s[pos];
        match b {
            b'\r' => {
                if pos + 1 < self.end && self.s[pos + 1] == b'\n' {
                    Some(pos + 2)
                } else {
                    Some(pos + 1)
                }
            }
            b'\n' => Some(pos + 1),
            0x0b | 0x0c if !anycrlf => Some(pos + 1),
            0x85 if !anycrlf && !self.utf => Some(pos + 1),
            0xc2 if !anycrlf && self.utf => {
                (pos + 1 < self.end && self.s[pos + 1] == 0x85).then_some(pos + 2)
            }
            0xe2 if !anycrlf && self.utf => (pos + 2 < self.end
                && self.s[pos + 1] == 0x80
                && matches!(self.s[pos + 2], 0xa8 | 0xa9))
            .then_some(pos + 3),
            _ => None,
        }
    }

    /// `\X` at pos (pcre2pattern "Extended grapheme clusters").
    fn match_grapheme(&self, pos: usize) -> Option<usize> {
        if pos >= self.end {
            return None;
        }
        let (c0, l0) = self.decode(pos);
        let mut p = pos + l0;
        let mut prev = unicode::gbp(c0);
        // Regional indicators seen in the current run.
        let mut ri_count = if prev == unicode::GBP_REGIONAL_INDICATOR {
            1
        } else {
            0
        };
        // For emoji ZWJ sequences: Extended_Pictographic Extend* ZWJ x ExtPict.
        let mut ext_pict_seq = prev == unicode::GBP_EXTENDED_PICTOGRAPHIC;
        if prev == unicode::GBP_CONTROL || prev == unicode::GBP_LF {
            return Some(p);
        }
        if prev == unicode::GBP_CR {
            if p < self.end && self.s[p] == b'\n' {
                return Some(p + 1);
            }
            return Some(p);
        }
        while p < self.end {
            let (c, l) = self.decode(p);
            let cur = unicode::gbp(c);
            use unicode::*;
            let join = match (prev, cur) {
                (_, GBP_CR) | (_, GBP_LF) | (_, GBP_CONTROL) => false,
                (GBP_L, GBP_L | GBP_V | GBP_LV | GBP_LVT) => true,
                (GBP_LV | GBP_V, GBP_V | GBP_T) => true,
                (GBP_LVT | GBP_T, GBP_T) => true,
                (_, GBP_EXTEND | GBP_ZWJ | GBP_SPACINGMARK) => true,
                (GBP_PREPEND, _) => true,
                (GBP_ZWJ, GBP_EXTENDED_PICTOGRAPHIC) => ext_pict_seq,
                (GBP_REGIONAL_INDICATOR, GBP_REGIONAL_INDICATOR) => ri_count % 2 == 1,
                _ => false,
            };
            if !join {
                break;
            }
            // Track state.
            if cur == GBP_REGIONAL_INDICATOR {
                ri_count += 1;
            } else {
                ri_count = 0;
            }
            if cur == GBP_EXTENDED_PICTOGRAPHIC {
                ext_pict_seq = true;
            } else if cur != GBP_EXTEND && cur != GBP_ZWJ {
                ext_pict_seq = false;
            }
            prev = cur;
            p += l;
        }
        Some(p)
    }

    /// Steps back `n` characters from `pos`; None if not enough.
    #[inline]
    fn back_chars(&self, pos: usize, n: u32) -> Option<usize> {
        if !self.utf {
            return pos.checked_sub(n as usize);
        }
        let mut p = pos;
        for _ in 0..n {
            if p == 0 {
                return None;
            }
            p = utf8::prev_start(self.s, p);
        }
        Some(p)
    }

    fn backref_match(
        &self,
        groups: &[u32],
        caseless: bool,
        restrict: bool,
        sc: &Scratch,
        pos: usize,
    ) -> Option<usize> {
        let mut found = None;
        for &g in groups {
            let st = sc.slots[2 * g as usize];
            if st != UNSET {
                found = Some((st, sc.slots[2 * g as usize + 1]));
                break;
            }
        }
        let Some((st, en)) = found else {
            return if self.prog.match_unset_backref {
                Some(pos)
            } else {
                None
            };
        };
        let len = en - st;
        if !caseless {
            if pos + len > self.end || self.s[st..en] != self.s[pos..pos + len] {
                return None;
            }
            return Some(pos + len);
        }
        if !self.utf && !self.prog.ucp {
            if pos + len > self.end || !self.s[st..en].eq_ignore_ascii_case(&self.s[pos..pos + len])
            {
                return None;
            }
            return Some(pos + len);
        }
        // Unicode caseless comparison, character by character.
        let mut a = st;
        let mut p = pos;
        while a < en {
            if p >= self.end {
                return None;
            }
            let (c1, l1) = self.decode(a);
            let (c2, l2) = self.decode(p);
            if c1 != c2 {
                let set = unicode::case_set(c1);
                if !set.contains(&c2) || (restrict && (c1 < 128) != (c2 < 128)) {
                    return None;
                }
            }
            a += l1;
            p += l2;
        }
        Some(p)
    }

    // -----------------------------------------------------------------------
    // Stack manipulation

    /// Removes the barrier at `idx` and every frame above it, keeping undo
    /// records (atomic groups and positive assertions keep their captures).
    fn cut(&mut self, sc: &mut Scratch, idx: usize) {
        let len = sc.bt.len();
        let mut w = idx;
        for r in idx..len {
            let e = sc.bt[r];
            if is_undo(e.kind) {
                let mut e = e;
                if e.kind == K_MARK {
                    // No longer visible to (*SKIP:NAME).
                    e.kind = K_MARK_HID;
                }
                sc.bt[w] = e;
                w += 1;
            } else {
                self.release(&e);
            }
        }
        sc.bt.truncate(w);
    }

    /// Pops entries down to and including `idx`, applying undo records.
    fn unwind_to(&mut self, sc: &mut Scratch, idx: usize) {
        while sc.bt.len() > idx {
            let e = sc.bt.pop().unwrap();
            self.release(&e);
            self.apply_undo(sc, e);
        }
    }

    #[inline]
    fn apply_undo(&mut self, sc: &mut Scratch, e: Bt) {
        match e.kind {
            K_SLOT => sc.slots[e.a as usize] = e.b,
            K_MARK | K_MARK_HID | K_MARK_UNDO => self.mark = e.b,
            K_CALL_PUSH => {
                let f = sc.calls.pop().unwrap();
                sc.arena.truncate(f.arena_off);
            }
            K_CALL_POP => {
                let f = sc.popped.pop().unwrap();
                sc.calls.push(f);
            }
            K_SCAN => self.end = e.c,
            _ => {}
        }
    }

    fn find_look(&self, sc: &Scratch, end_pc: Pc) -> usize {
        let mut i = sc.bt.len();
        while i > 0 {
            i -= 1;
            let e = &sc.bt[i];
            if e.kind == K_LOOK && e.a == end_pc {
                return i;
            }
        }
        unreachable!("lookaround barrier not found")
    }

    fn find_kind(&self, sc: &Scratch, kind: u32) -> usize {
        let mut i = sc.bt.len();
        while i > 0 {
            i -= 1;
            if sc.bt[i].kind == kind {
                return i;
            }
        }
        unreachable!("barrier not found")
    }

    fn look_kind_code(k: LookKind) -> usize {
        match k {
            LookKind::Positive => 0,
            LookKind::Negative => 1,
            LookKind::NonAtomic => 2,
            LookKind::Cond { negated: false } => 3,
            LookKind::Cond { negated: true } => 4,
        }
    }

    /// The no-branch pc of a conditional assertion whose LookEnd is at `end`.
    fn cond_no(&self, end: Pc) -> Pc {
        match &self.prog.insts[end as usize] {
            Inst::LookEnd { no, .. } => *no,
            _ => unreachable!(),
        }
    }

    /// Returns from the innermost subroutine call.
    fn do_return(&mut self, sc: &mut Scratch) -> Pc {
        let f = sc.calls.pop().unwrap();
        sc.popped.push(f);
        sc.bt.push(Bt {
            kind: K_CALL_POP,
            a: 0,
            b: 0,
            c: 0,
        });
        // Restore everything but \K's slot.
        for i in 0..self.nslots {
            if i == self.keep_slot {
                continue;
            }
            let saved = sc.arena[f.arena_off + i];
            if sc.slots[i] != saved {
                let old = sc.slots[i];
                sc.bt.push(Bt {
                    kind: K_SLOT,
                    a: i as u32,
                    b: old,
                    c: 0,
                });
                sc.slots[i] = saved;
            }
        }
        f.ret_pc
    }

    // -----------------------------------------------------------------------
    // Backtracking

    fn backtrack(&mut self, sc: &mut Scratch) -> Result<Step, MatchError> {
        loop {
            let Some(e) = sc.bt.pop() else {
                return Ok(Step::Done(Attempt::NoMatch));
            };
            self.release(&e);
            match e.kind {
                K_SLOT => sc.slots[e.a as usize] = e.b,
                K_ALT => {
                    return Ok(Step::Resume(e.a, e.b));
                }
                K_ALT_NC => return Ok(Step::Resume(e.a, e.b)),
                K_ALT_LAZY => {
                    // Taking the lazy alternative keeps a JIT frame
                    // (observed on PHP 8.5: /(a)*?$/ handles 8191
                    // characters, 24 bytes each).
                    if self.jit_limit != 0 {
                        self.push_alt(sc, K_JIT8, 0, 0, 0)?;
                    }
                    return Ok(Step::Resume(e.a, e.b));
                }
                K_ALT_BR | K_ALT_TAG => {
                    self.count += 1;
                    if self.count > self.limits.match_limit as u64 {
                        return Err(MatchError::MatchLimit);
                    }
                    return Ok(Step::Resume(e.a, e.b));
                }
                K_REP_GREEDY => {
                    let min_pos = e.b;
                    let cur = e.c;
                    if cur <= min_pos {
                        continue;
                    }
                    let Inst::Rep { item, .. } = &self.prog.insts[e.a as usize] else {
                        unreachable!()
                    };
                    let np = if self.item_single_byte(item) {
                        cur - 1
                    } else {
                        utf8::prev_start(self.s, cur)
                    };
                    // Skip positions where the next literal cannot match.
                    let np = self.skip_greedy(e.a, min_pos, np);
                    let Some(np) = np else { continue };
                    if np > min_pos {
                        self.push_frame(sc, K_REP_GREEDY, e.a, min_pos, np)?;
                    } else {
                        self.count += 1;
                        if self.count > self.limits.match_limit as u64 {
                            return Err(MatchError::MatchLimit);
                        }
                    }
                    return Ok(Step::Resume(e.a + 1, np));
                }
                K_REP_LAZY => {
                    let Inst::Rep { item, max, .. } = &self.prog.insts[e.a as usize] else {
                        unreachable!()
                    };
                    let count = e.b as u32;
                    if count >= *max {
                        continue;
                    }
                    let Some(np) = self.match_item(item, e.c) else {
                        continue;
                    };
                    let (count, np) = self.lazy_advance(e.a, count + 1, np);
                    if self.count > self.limits.match_limit as u64 {
                        return Err(MatchError::MatchLimit);
                    }
                    let count = count - 1;
                    if count + 1 < *max {
                        self.push_frame(sc, K_REP_LAZY, e.a, (count + 1) as usize, np)?;
                    } else {
                        self.count += 1;
                        if self.count > self.limits.match_limit as u64 {
                            return Err(MatchError::MatchLimit);
                        }
                    }
                    return Ok(Step::Resume(e.a + 1, np));
                }
                K_FRAME | K_THEN_SCOPE | K_ATOMIC | K_SR => if is_frame(e.kind) {},
                K_LOOK => {
                    // The assertion body failed.
                    match e.c {
                        1 | 4 => return Ok(Step::Resume(e.a + 1, e.b)),
                        3 => return Ok(Step::Resume(self.cond_no(e.a), e.b)),
                        _ => {}
                    }
                }
                K_BEHIND => {
                    let Inst::Behind { min, .. } = &self.prog.insts[e.a as usize] else {
                        unreachable!()
                    };
                    let len = e.c as u32;
                    if len < *min {
                        continue;
                    }
                    let Some(p) = self.back_chars(e.b, len) else {
                        continue;
                    };
                    if len > *min {
                        self.push_frame(sc, K_BEHIND, e.a, e.b, (len - 1) as usize)?;
                    }
                    return Ok(Step::Resume(e.a + 1, p));
                }
                K_MARK | K_MARK_HID => {
                    self.mark = e.b;
                }
                K_MARK_UNDO => self.mark = e.b,
                K_VERB => {
                    let act = match e.a {
                        V_COMMIT => VerbAct::Commit,
                        V_PRUNE => VerbAct::Prune,
                        V_SKIP => VerbAct::Skip(e.b),
                        V_SKIPNAME => {
                            // Search the path for (*MARK:NAME).
                            let name = e.b;
                            let mut found = None;
                            for x in sc.bt.iter().rev() {
                                if x.kind == K_MARK && x.a as usize == name {
                                    found = Some(x.c);
                                    break;
                                }
                            }
                            match found {
                                Some(p) => VerbAct::Skip(p),
                                None => continue,
                            }
                        }
                        _ => VerbAct::Then(e.c),
                    };
                    return self.unwind_verb(sc, act);
                }
                K_CALL_PUSH => {
                    let f = sc.calls.pop().unwrap();
                    sc.arena.truncate(f.arena_off);
                }
                K_CALL_POP => {
                    let f = sc.popped.pop().unwrap();
                    sc.calls.push(f);
                }
                K_SCAN => {
                    self.end = e.c;
                }
                _ => {}
            }
        }
    }

    /// For a lazy single-byte repeat followed by a literal byte, takes the
    /// items that cannot be followed by that byte without trying the rest
    /// of the pattern after each (counting each skipped try as PCRE2
    /// would). Returns the new count and position.
    #[inline]
    fn lazy_advance(&mut self, rep_pc: Pc, mut count: u32, mut pos: usize) -> (u32, usize) {
        let next = match self.prog.insts.get(rep_pc as usize + 1) {
            Some(Inst::One(Item::Byte(b))) => *b,
            Some(Inst::Lit(i)) => self.prog.lits[*i as usize][0],
            _ => return (count, pos),
        };
        let Inst::Rep { item, max, .. } = &self.prog.insts[rep_pc as usize] else {
            return (count, pos);
        };
        if !self.item_single_byte(item) {
            return (count, pos);
        }
        let start = pos;
        while count < *max && pos < self.end && self.s[pos] != next {
            match self.match_item(item, pos) {
                Some(p) => pos = p,
                None => break,
            }
            count += 1;
        }
        self.count += (pos - start) as u64;
        (count, pos)
    }

    /// For a greedy single-item repeat followed by a literal byte, moves the
    /// retry position back to where that byte occurs (counting the skipped
    /// tries as PCRE2 would count its retries).
    #[inline]
    fn skip_greedy(&mut self, rep_pc: Pc, min_pos: usize, np: usize) -> Option<usize> {
        let next = match self.prog.insts.get(rep_pc as usize + 1) {
            Some(Inst::One(Item::Byte(b))) => *b,
            Some(Inst::Lit(i)) => self.prog.lits[*i as usize][0],
            _ => return Some(np),
        };
        let Inst::Rep { item, .. } = &self.prog.insts[rep_pc as usize] else {
            return Some(np);
        };
        if !self.item_single_byte(item) {
            return Some(np);
        }
        let mut p = np;
        loop {
            if p < self.end && self.s[p] == next {
                return Some(p);
            }
            if p <= min_pos {
                return if p == min_pos { Some(p) } else { None };
            }
            p -= 1;
            self.count += 1;
        }
    }

    fn unwind_verb(&mut self, sc: &mut Scratch, act: VerbAct) -> Result<Step, MatchError> {
        loop {
            let Some(e) = sc.bt.pop() else {
                return Ok(Step::Done(match act {
                    VerbAct::Commit => Attempt::Commit,
                    VerbAct::Prune | VerbAct::Then(_) => Attempt::Prune,
                    VerbAct::Skip(p) => Attempt::Skip(p),
                }));
            };
            self.release(&e);
            match e.kind {
                K_ALT_TAG => {
                    if let VerbAct::Then(t) = act
                        && t != 0
                        && e.c == t
                    {
                        self.count += 1;
                        return Ok(Step::Resume(e.a, e.b));
                    }
                }
                K_THEN_SCOPE => {
                    if let VerbAct::Then(t) = act
                        && t != 0
                        && e.a as usize == t
                    {
                        // The alternation is exhausted: fail normally.
                        return self.backtrack(sc);
                    }
                }
                K_CALL_PUSH => {
                    let f = sc.calls.pop().unwrap();
                    sc.arena.truncate(f.arena_off);
                    // The subroutine call fails; backtrack at the outer level.
                    return self.backtrack(sc);
                }
                K_LOOK => {
                    match e.c {
                        // Negative assertion: true.
                        1 | 4 => return Ok(Step::Resume(e.a + 1, e.b)),
                        // Conditional positive: false.
                        3 => return Ok(Step::Resume(self.cond_no(e.a), e.b)),
                        _ => {
                            if let VerbAct::Then(_) = act {
                                // (*THEN) cannot escape an assertion: it is
                                // false.
                                return self.backtrack(sc);
                            }
                        }
                    }
                }
                K_SCAN => {
                    self.end = e.c;
                    if let VerbAct::Then(_) = act {
                        return self.backtrack(sc);
                    }
                }
                _ => self.apply_undo(sc, e),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Main loop

    fn run(&mut self, sc: &mut Scratch, start: usize) -> Result<Attempt, MatchError> {
        let insts = &self.prog.insts[..];
        let mut pc: Pc = 0;
        let mut pos = start;
        self.attempt_start = start;
        // pcre2api: a counter "incremented each time round its main
        // matching loop"; the minimum limits observed on PHP 8.5 for a
        // plain literal match are 2 for both.
        self.count = 2;
        self.frames = 2;
        self.jit_bytes = 0;
        if self.count > self.limits.match_limit as u64 {
            return Err(MatchError::MatchLimit);
        }
        if self.frames > self.limits.depth_limit {
            return Err(MatchError::DepthLimit);
        }
        loop {
            match &insts[pc as usize] {
                Inst::One(item) => match self.match_item(item, pos) {
                    Some(p) => {
                        pos = p;
                        pc += 1;
                    }
                    None => fail!(self, sc, pc, pos),
                },
                Inst::Lit(i) => {
                    let lit = &self.prog.lits[*i as usize];
                    if pos + lit.len() <= self.end && self.s[pos..pos + lit.len()] == **lit {
                        pos += lit.len();
                        pc += 1;
                    } else {
                        fail!(self, sc, pc, pos)
                    }
                }
                Inst::LitFold(i) => {
                    let lit = &self.prog.lits[*i as usize];
                    let n = lit.len();
                    if pos + n <= self.end
                        && self.s[pos..pos + n]
                            .iter()
                            .zip(lit.iter())
                            .all(|(a, b)| a | 0x20 == *b)
                    {
                        pos += n;
                        pc += 1;
                    } else {
                        fail!(self, sc, pc, pos)
                    }
                }
                Inst::Rep {
                    item,
                    min,
                    max,
                    greed,
                } => {
                    let (min, max) = (*min, *max);
                    // Mandatory part.
                    let (p, n) =
                        self.scan_items(item, pos, if *greed == Greed::Lazy { min } else { max });
                    if n < min {
                        fail!(self, sc, pc, pos);
                    }
                    match greed {
                        Greed::Possessive => {
                            pos = p;
                            pc += 1;
                        }
                        Greed::Greedy => {
                            if n > min {
                                let min_pos = if self.item_single_byte(item) {
                                    pos + min as usize
                                } else {
                                    let (mp, _) = self.scan_items(item, pos, min);
                                    mp
                                };
                                self.push_frame(sc, K_REP_GREEDY, pc, min_pos, p)?;
                            }
                            pos = p;
                            pc += 1;
                        }
                        Greed::Lazy => {
                            let (n, p) = self.lazy_advance(pc, min, p);
                            if self.count > self.limits.match_limit as u64 {
                                return Err(MatchError::MatchLimit);
                            }
                            if n < max {
                                self.push_frame(sc, K_REP_LAZY, pc, n as usize, p)?;
                            }
                            pos = p;
                            pc += 1;
                        }
                    }
                }
                Inst::Newline { anycrlf } => match self.match_newline_seq(pos, *anycrlf) {
                    Some(p) => {
                        pos = p;
                        pc += 1;
                    }
                    None => fail!(self, sc, pc, pos),
                },
                Inst::Grapheme => match self.match_grapheme(pos) {
                    Some(p) => {
                        pos = p;
                        pc += 1;
                    }
                    None => fail!(self, sc, pc, pos),
                },
                Inst::Assert(a) => {
                    if self.check_assert(*a, pos) {
                        pc += 1;
                    } else {
                        fail!(self, sc, pc, pos)
                    }
                }
                Inst::Split { alt } => {
                    self.push_frame(sc, K_ALT, *alt, pos, 0)?;
                    pc += 1;
                }
                Inst::Branch { alt, count } => {
                    self.push_alt(sc, if *count { K_ALT_BR } else { K_ALT_NC }, *alt, pos, 0)?;
                    pc += 1;
                }
                Inst::SplitLazy { to } => {
                    self.push_frame(sc, K_ALT_LAZY, pc + 1, pos, 0)?;
                    pc = *to;
                }
                Inst::SplitTag { alt, tag } => {
                    self.push_alt(sc, K_ALT_TAG, *alt, pos, *tag as usize)?;
                    pc += 1;
                }
                Inst::ThenScope { tag } => {
                    self.push(sc, K_THEN_SCOPE, *tag, 0, 0);
                    pc += 1;
                }
                Inst::Jmp(t) => pc = *t,
                Inst::CapOpen(g) => {
                    self.push_frame(sc, K_FRAME, 16, 0, 0)?;
                    let i = self.open_base + *g as usize;
                    self.set_slot(sc, i, pos);
                    pc += 1;
                }
                Inst::CapClose(g) => {
                    let g = *g;
                    if let Some(f) = sc.calls.last()
                        && f.group == g
                    {
                        pc = self.do_return(sc);
                        continue;
                    }
                    let st = sc.slots[self.open_base + g as usize];
                    self.set_slot(sc, 2 * g as usize, st);
                    self.set_slot(sc, 2 * g as usize + 1, pos);
                    pc += 1;
                }
                Inst::GroupFrame => {
                    self.push_frame(sc, K_FRAME, 0, 0, 0)?;
                    pc += 1;
                }
                Inst::LoopStart { slot } => {
                    let i = self.loop_base + *slot as usize;
                    self.set_slot(sc, i, pos);
                    pc += 1;
                }
                Inst::LoopEnd {
                    slot,
                    start,
                    greedy,
                } => {
                    let i = self.loop_base + *slot as usize;
                    if sc.slots[i] == pos {
                        // Empty iteration: no more iterations.
                        pc += 1;
                    } else if *greedy {
                        self.push_frame(sc, K_ALT, pc + 1, pos, 0)?;
                        pc = *start;
                    } else {
                        self.push_frame(sc, K_ALT_LAZY, *start, pos, 0)?;
                        pc += 1;
                    }
                }
                Inst::AtomicStart => {
                    self.push_frame(sc, K_ATOMIC, 0, pos, 0)?;
                    pc += 1;
                }
                Inst::AtomicEnd => {
                    let idx = self.find_kind(sc, K_ATOMIC);
                    self.cut(sc, idx);
                    pc += 1;
                }
                Inst::LookStart { kind, end } => {
                    self.push_frame(sc, K_LOOK, *end, pos, Self::look_kind_code(*kind))?;
                    pc += 1;
                }
                Inst::Behind { min, max } => {
                    let (min, max) = (*min, *max);
                    if min == max {
                        match self.back_chars(pos, min) {
                            Some(p) => {
                                pos = p;
                                pc += 1;
                            }
                            None => fail!(self, sc, pc, pos),
                        }
                    } else {
                        // Longest first (observed on PHP 8.5: (?<=(\w{1,3}))x
                        // captures three characters).
                        let mut len = max;
                        let p = loop {
                            if let Some(p) = self.back_chars(pos, len) {
                                break Some(p);
                            }
                            if len == min {
                                break None;
                            }
                            len -= 1;
                        };
                        let Some(p) = p else { fail!(self, sc, pc, pos) };
                        if len > min {
                            self.push_frame(sc, K_BEHIND, pc, pos, (len - 1) as usize)?;
                        }
                        pos = p;
                        pc += 1;
                    }
                }
                Inst::BehindCheck => {
                    // The innermost lookaround barrier is ours.
                    let idx = self.find_kind(sc, K_LOOK);
                    if sc.bt[idx].b == pos {
                        pc += 1;
                    } else {
                        fail!(self, sc, pc, pos)
                    }
                }
                Inst::LookEnd { kind, no } => {
                    let idx = self.find_look(sc, pc);
                    let saved = sc.bt[idx].b;
                    match kind {
                        LookKind::Positive | LookKind::Cond { negated: false } => {
                            self.cut(sc, idx);
                            pos = saved;
                            pc += 1;
                        }
                        LookKind::NonAtomic => {
                            pos = saved;
                            pc += 1;
                        }
                        LookKind::Negative => {
                            self.unwind_to(sc, idx);
                            fail!(self, sc, pc, pos)
                        }
                        LookKind::Cond { negated: true } => {
                            self.cut(sc, idx);
                            pos = saved;
                            pc = *no;
                        }
                    }
                }
                Inst::Backref {
                    groups,
                    caseless,
                    restrict,
                } => match self.backref_match(groups, *caseless, *restrict, sc, pos) {
                    Some(p) => {
                        pos = p;
                        pc += 1;
                    }
                    None => fail!(self, sc, pc, pos),
                },
                Inst::Call { group } => {
                    let g = *group;
                    for f in sc.calls.iter().rev() {
                        if f.group == g && f.entry_pos == pos {
                            // With the JIT the recursion runs until the stack is
                            // exhausted (observed on PHP 8.5: /(?R)/ gives
                            // PREG_JIT_STACKLIMIT_ERROR).
                            return Err(if self.jit_limit != 0 {
                                MatchError::JitStackLimit
                            } else {
                                MatchError::RecurseLoop
                            });
                        }
                    }
                    self.count += 1;
                    self.frames += 1;
                    if self.count > self.limits.match_limit as u64 {
                        return Err(MatchError::MatchLimit);
                    }
                    if self.frames > self.limits.depth_limit {
                        return Err(MatchError::DepthLimit);
                    }
                    let arena_off = sc.arena.len();
                    sc.arena.extend_from_slice(&sc.slots);
                    sc.calls.push(CallFrame {
                        group: g,
                        ret_pc: pc + 1,
                        arena_off,
                        entry_pos: pos,
                        bt_height: sc.bt.len(),
                    });
                    sc.bt.push(Bt {
                        kind: K_CALL_PUSH,
                        a: 0,
                        b: 0,
                        c: 0,
                    });
                    if self.jit_limit != 0 {
                        self.jit_bytes += 32;
                        if self.jit_bytes >= self.jit_limit {
                            return Err(MatchError::JitStackLimit);
                        }
                    }
                    if sc.calls.len() as u32 >= self.limits.depth_limit {
                        return Err(MatchError::DepthLimit);
                    }
                    // Enter the group's body past its CapOpen: the captures
                    // of a call are restored on return anyway, and PCRE2
                    // counts no group frame for a recursion.
                    let t = self.prog.group_pc[g as usize];
                    pc = if g == 0 { t } else { t + 1 };
                }
                Inst::CondRef { groups, no } => {
                    if groups.iter().any(|&g| sc.slots[2 * g as usize] != UNSET) {
                        pc += 1;
                    } else {
                        pc = *no;
                    }
                }
                Inst::CondRecurse { groups, no } => {
                    let t = match (sc.calls.last(), groups) {
                        (None, _) => false,
                        (Some(_), None) => true,
                        (Some(f), Some(gs)) => gs.contains(&f.group),
                    };
                    pc = if t { pc + 1 } else { *no };
                }
                Inst::Mark(n) => {
                    let old = self.mark;
                    self.push_frame(sc, K_MARK, *n, old, pos)?;
                    self.mark = *n as usize;
                    self.last_mark = self.mark;
                    pc += 1;
                }
                Inst::SetMarkName(n) => {
                    let old = self.mark;
                    self.push(sc, K_MARK_UNDO, 0, old, 0);
                    self.mark = *n as usize;
                    self.last_mark = self.mark;
                    pc += 1;
                }
                Inst::Commit => {
                    self.push_frame(sc, K_VERB, V_COMMIT, 0, 0)?;
                    pc += 1;
                }
                Inst::Prune => {
                    self.push_frame(sc, K_VERB, V_PRUNE, 0, 0)?;
                    pc += 1;
                }
                Inst::Skip => {
                    self.push_frame(sc, K_VERB, V_SKIP, pos, 0)?;
                    pc += 1;
                }
                Inst::SkipName(n) => {
                    self.push_frame(sc, K_VERB, V_SKIPNAME, *n as usize, 0)?;
                    pc += 1;
                }
                Inst::Then { tag } => {
                    self.push_frame(sc, K_VERB, V_THEN, 0, *tag as usize)?;
                    pc += 1;
                }
                Inst::Fail => fail!(self, sc, pc, pos),
                Inst::Accept { close } => {
                    // Innermost scope: an assertion barrier or a call.
                    let mut barrier = None;
                    let mut i = sc.bt.len();
                    while i > 0 {
                        i -= 1;
                        let k = sc.bt[i].kind;
                        if k == K_LOOK || k == K_SCAN {
                            barrier = Some(i);
                            break;
                        }
                    }
                    let call_h = sc.calls.last().map(|f| f.bt_height);
                    for &g in close.iter() {
                        let st = sc.slots[self.open_base + g as usize];
                        self.set_slot(sc, 2 * g as usize, st);
                        self.set_slot(sc, 2 * g as usize + 1, pos);
                    }
                    match (barrier, call_h) {
                        (Some(b), ch) if ch.is_none_or(|h| b >= h) => {
                            let e = sc.bt[b];
                            if e.kind == K_SCAN {
                                self.end = e.c;
                                let saved = e.b;
                                self.cut(sc, b);
                                pos = saved;
                                pc = e.a + 1;
                                continue;
                            }
                            let saved = e.b;
                            match e.c {
                                1 => {
                                    self.unwind_to(sc, b);
                                    fail!(self, sc, pc, pos)
                                }
                                4 => {
                                    self.cut(sc, b);
                                    pos = saved;
                                    pc = self.cond_no(e.a);
                                }
                                _ => {
                                    self.cut(sc, b);
                                    pos = saved;
                                    pc = e.a + 1;
                                }
                            }
                        }
                        (_, Some(_)) => {
                            pc = self.do_return(sc);
                        }
                        _ => {
                            let ms = self.match_start(sc, start);
                            if self.reject_empty(ms, pos) {
                                fail!(self, sc, pc, pos)
                            }
                            return Ok(Attempt::Match(ms, pos));
                        }
                    }
                }
                Inst::Keep => {
                    let k = self.keep_slot;
                    self.set_slot(sc, k, pos);
                    pc += 1;
                }
                Inst::ScriptRunStart => {
                    self.push(sc, K_SR, 0, pos, 0);
                    pc += 1;
                }
                Inst::ScriptRunEnd => {
                    let idx = self.find_kind(sc, K_SR);
                    let st = sc.bt[idx].b;
                    if self.is_script_run(st, pos) {
                        pc += 1;
                    } else {
                        fail!(self, sc, pc, pos)
                    }
                }
                Inst::ScanStart { groups, end } => {
                    let mut found = None;
                    for &g in groups.iter() {
                        let st = sc.slots[2 * g as usize];
                        if st != UNSET {
                            found = Some((st, sc.slots[2 * g as usize + 1]));
                            break;
                        }
                    }
                    let Some((st, en)) = found else {
                        fail!(self, sc, pc, pos)
                    };
                    let old_end = self.end;
                    self.push_frame(sc, K_SCAN, *end, pos, old_end)?;
                    self.end = en;
                    pos = st;
                    pc += 1;
                }
                Inst::ScanEnd => {
                    let idx = self.find_kind(sc, K_SCAN);
                    let e = sc.bt[idx];
                    self.end = e.c;
                    self.cut(sc, idx);
                    pos = e.b;
                    pc += 1;
                }
                Inst::Match => {
                    if let Some(f) = sc.calls.last()
                        && f.group == 0
                    {
                        pc = self.do_return(sc);
                        continue;
                    }
                    let ms = self.match_start(sc, start);
                    if self.reject_empty(ms, pos) {
                        fail!(self, sc, pc, pos)
                    }
                    return Ok(Attempt::Match(ms, pos));
                }
            }
        }
    }

    /// PCRE2_NOTEMPTY / PCRE2_NOTEMPTY_ATSTART, from the match options or
    /// the (*NOTEMPTY) / (*NOTEMPTY_ATSTART) pattern items.
    #[inline]
    fn reject_empty(&self, ms: usize, pos: usize) -> bool {
        pos == ms
            && (self.opts.notempty
                || self.prog.notempty
                || ((self.opts.notempty_atstart || self.prog.notempty_atstart)
                    && ms == self.start_offset))
    }

    /// Matches a deterministic pattern at `start`. `run_end` receives the
    /// end of the leading repeat's run (see [`OnePass::lead_run`]).
    #[inline]
    fn run_onepass(
        &mut self,
        op: &OnePass,
        start: usize,
        slots: &mut [usize],
        run_end: &mut usize,
    ) -> Result<Option<(usize, usize)>, MatchError> {
        let Some(b) = op.back else {
            return Ok(self
                .onepass_steps(op, 0, op.steps.len(), start, start, slots, run_end, UNSET)
                .map(|(pos, keep)| (if keep == UNSET { start } else { keep }, pos)));
        };
        let Some((pos, keep)) = self.onepass_steps(op, 0, b, start, start, slots, run_end, UNSET)
        else {
            return Ok(None);
        };
        let OpStep::BackRep {
            item,
            min,
            max,
            lazy,
        } = &op.steps[b]
        else {
            unreachable!()
        };
        let (min, max) = (*min, *max);
        let single = self.item_single_byte(item);
        // The first byte the rest of the pattern needs, if known.
        let next = match op.steps.get(b + 1) {
            Some(OpStep::One(Item::Byte(x))) => Some(*x),
            Some(OpStep::Lit(i)) => Some(self.prog.lits[*i as usize][0]),
            _ => None,
        };
        // The first try is the repeat's own frame.
        self.count = op.frames as u64 - 1;
        let limit = self.limits.match_limit as u64;
        let n_steps = op.steps.len();
        let finish = |me: usize, k: usize| (if k == UNSET { start } else { k }, me);
        if !*lazy {
            let (pmax, n) = self.scan_items(item, pos, max);
            if Some(b) == op.lead_run {
                *run_end = pmax;
            }
            if n < min {
                return Ok(None);
            }
            let min_pos = if single {
                pos + min as usize
            } else {
                self.scan_items(item, pos, min).0
            };
            let mut e = pmax;
            loop {
                if let Some(nb) = next {
                    while e > min_pos && (e >= self.end || self.s[e] != nb) {
                        e = if single {
                            e - 1
                        } else {
                            utf8::prev_start(self.s, e)
                        };
                        self.count += 1;
                    }
                }
                self.count += 1;
                if self.count > limit {
                    return Err(MatchError::MatchLimit);
                }
                if let Some((me, k)) =
                    self.onepass_steps(op, b + 1, n_steps, e, start, slots, run_end, keep)
                {
                    let (ms, me) = finish(me, k);
                    if !self.reject_empty(ms, me) {
                        return Ok(Some((ms, me)));
                    }
                }
                if e <= min_pos {
                    return Ok(None);
                }
                e = if single {
                    e - 1
                } else {
                    utf8::prev_start(self.s, e)
                };
            }
        }
        let (mut e, mut n) = self.scan_items(item, pos, min);
        if n < min {
            return Ok(None);
        }
        loop {
            if let Some(nb) = next
                && single
            {
                while n < max && e < self.end && self.s[e] != nb {
                    match self.match_item(item, e) {
                        Some(p) => e = p,
                        None => break,
                    }
                    n += 1;
                    self.count += 1;
                }
            }
            self.count += 1;
            if self.count > limit {
                return Err(MatchError::MatchLimit);
            }
            if let Some((me, k)) =
                self.onepass_steps(op, b + 1, n_steps, e, start, slots, run_end, keep)
            {
                let (ms, me) = finish(me, k);
                if !self.reject_empty(ms, me) {
                    return Ok(Some((ms, me)));
                }
            }
            if n >= max {
                return Ok(None);
            }
            match self.match_item(item, e) {
                Some(p) => e = p,
                None => return Ok(None),
            }
            n += 1;
        }
    }

    /// Runs deterministic steps `from..to` at `pos`; returns the end
    /// position and the \K position.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    fn onepass_steps(
        &self,
        op: &OnePass,
        from: usize,
        to: usize,
        mut pos: usize,
        _start: usize,
        slots: &mut [usize],
        run_end: &mut usize,
        mut keep: usize,
    ) -> Option<(usize, usize)> {
        for (k, st) in op.steps[from..to].iter().enumerate() {
            let k = k + from;
            match st {
                OpStep::One(item) => pos = self.match_item(item, pos)?,
                OpStep::Lit(i) => {
                    let lit = &self.prog.lits[*i as usize];
                    if pos + lit.len() > self.end || self.s[pos..pos + lit.len()] != **lit {
                        return None;
                    }
                    pos += lit.len();
                }
                OpStep::LitFold(i) => {
                    let lit = &self.prog.lits[*i as usize];
                    let n = lit.len();
                    if pos + n > self.end
                        || !self.s[pos..pos + n]
                            .iter()
                            .zip(lit.iter())
                            .all(|(a, b)| a | 0x20 == *b)
                    {
                        return None;
                    }
                    pos += n;
                }
                OpStep::Rep { item, min, max } => {
                    let (p, n) = self.scan_items(item, pos, *max);
                    if Some(k) == op.lead_run {
                        *run_end = p;
                    }
                    if n < *min {
                        return None;
                    }
                    pos = p;
                }
                OpStep::Open(g) => slots[self.open_base + *g as usize] = pos,
                OpStep::Close(g) => {
                    let g = *g as usize;
                    slots[2 * g] = slots[self.open_base + g];
                    slots[2 * g + 1] = pos;
                }
                OpStep::Assert(a) => {
                    if !self.check_assert(*a, pos) {
                        return None;
                    }
                }
                OpStep::Keep => keep = pos,
                OpStep::BackRep { .. } => unreachable!(),
            }
        }
        Some((pos, keep))
    }

    fn match_start(&self, sc: &Scratch, start: usize) -> usize {
        let k = sc.slots[self.keep_slot];
        if k == UNSET { start } else { k }
    }

    /// pcre2unicode "Script runs".
    fn is_script_run(&self, st: usize, en: usize) -> bool {
        let mut p = st;
        let mut nchars = 0;
        // Intersection of script sets, represented by a candidate list.
        let mut cand: Option<Vec<u8>> = None;
        // Han combinations: track which "virtual" scripts are possible.
        let mut digit_zero: Option<u32> = None;
        let han = unicode::lookup_script("Han").unwrap_or(0);
        let hira = unicode::lookup_script("Hiragana").unwrap_or(0);
        let kana = unicode::lookup_script("Katakana").unwrap_or(0);
        let hang = unicode::lookup_script("Hangul").unwrap_or(0);
        let bopo = unicode::lookup_script("Bopomofo").unwrap_or(0);
        // Virtual scripts (pcre2unicode "The Chinese Han script"): Japanese,
        // Korean and Taiwanese Mandarin combine Han with other scripts.
        const JPAN: u8 = 253;
        const KORE: u8 = 254;
        const HANB: u8 = 255;
        while p < en {
            let (c, l) = self.decode(p);
            p += l;
            nchars += 1;
            if unicode::gc(c) == unicode::GC_ND {
                // All digits must come from the same block of ten.
                let zero = c - (numeric_digit_value(c));
                match digit_zero {
                    None => digit_zero = Some(zero),
                    Some(z) if z != zero => return false,
                    _ => {}
                }
            }
            let r = unicode::record(c);
            let set: Vec<u8> = if r.scx == 0 {
                vec![r.script]
            } else {
                unicode::SCX_SETS[r.scx as usize].to_vec()
            };
            if set.len() == 1
                && (set[0] == unicode::SCRIPT_INHERITED || set[0] == unicode::SCRIPT_COMMON)
            {
                continue;
            }
            let mut expanded = Vec::new();
            for &sx in &set {
                if sx == han {
                    expanded.extend_from_slice(&[han, JPAN, KORE, HANB]);
                } else if sx == hira || sx == kana {
                    expanded.push(JPAN);
                } else if sx == hang {
                    expanded.push(KORE);
                } else if sx == bopo {
                    expanded.push(HANB);
                } else {
                    expanded.push(sx);
                }
            }
            cand = Some(match cand {
                None => expanded,
                Some(c0) => {
                    let mut out: Vec<u8> =
                        c0.into_iter().filter(|x| expanded.contains(x)).collect();
                    // Han + Hiragana/Katakana excludes Hangul/Bopomofo and
                    // vice versa.
                    out.dedup();
                    out
                }
            });
            if cand.as_ref().is_some_and(|c| c.is_empty()) {
                return nchars < 2;
            }
        }
        true
    }
}

/// The first byte >= 0x80 at or after `p`.
#[inline]
fn find_high_bit(s: &[u8], mut p: usize) -> Option<usize> {
    while p + 8 <= s.len() {
        let w = u64::from_le_bytes(s[p..p + 8].try_into().unwrap());
        let m = w & 0x8080_8080_8080_8080;
        if m != 0 {
            return Some(p + (m.trailing_zeros() / 8) as usize);
        }
        p += 8;
    }
    (p..s.len()).find(|&q| s[q] >= 0x80)
}

fn numeric_digit_value(c: u32) -> u32 {
    // Decimal digits come in runs of ten starting at a code point whose
    // value is zero; find the start of the run.
    let mut z = c;
    let mut k = 0;
    while k < 9 && z > 0 && unicode::gc(z - 1) == unicode::GC_ND {
        z -= 1;
        k += 1;
    }
    (c - z) % 10
}

/// Matching state for one subject: set up once, then [`Searcher::find`]
/// can be called repeatedly (the global matching loop of
/// preg_match_all() and friends).
pub struct Searcher<'p, 's> {
    vm: Vm<'p, 's>,
    /// The subject has been checked for valid UTF-8 from this offset on.
    utf_checked_from: Option<usize>,
}

impl<'p, 's> Searcher<'p, 's> {
    pub fn new(prog: &'p Prog, s: &'s [u8], limits: Limits) -> Searcher<'p, 's> {
        let ncap = prog.ncap as usize;
        let open_base = 2 * (ncap + 1);
        let loop_base = open_base + ncap + 1;
        let keep_slot = loop_base + prog.nloops as usize;
        let nslots = keep_slot + 1;
        let mut limits = limits;
        if let Some(l) = prog.limit_match {
            limits.match_limit = limits.match_limit.min(l);
        }
        if let Some(l) = prog.limit_depth {
            limits.depth_limit = limits.depth_limit.min(l);
        }
        if limits.jit_stack.is_some() {
            // The JIT does not use the depth limit (pcre2api).
            limits.depth_limit = u32::MAX;
        }
        Searcher {
            vm: Vm {
                prog,
                s,
                end: s.len(),
                utf: prog.utf,
                start_offset: 0,
                attempt_start: 0,
                opts: MatchOptions::default(),
                limits,
                count: 0,
                frames: 0,
                jit_limit: limits.jit_stack.map_or(0, |j| j as u64),
                jit_bytes: 0,
                mark: NO_MARK,
                last_mark: NO_MARK,
                open_base,
                loop_base,
                keep_slot,
                nslots,
            },
            utf_checked_from: None,
        }
    }

    /// PCRE2's UTF checks (pcre2unicode "Validity of UTF strings").
    fn check_utf(&mut self, start_offset: usize, opts: MatchOptions) -> Result<(), MatchError> {
        let s = self.vm.s;
        let prog = self.vm.prog;
        if opts.no_utf_check {
            // pcre2api PCRE2_NO_UTF_CHECK: no checks at all, not even of
            // the offset (PHP passes it after the first match of a global
            // loop, so /\C/u walks through every byte).
            return Ok(());
        }
        if start_offset < s.len() && s[start_offset] & 0xc0 == 0x80 {
            return Err(MatchError::BadUtf8Offset);
        }
        // Check from the longest lookbehind before the start offset.
        let mut from = start_offset;
        for _ in 0..prog.max_lookbehind {
            if from == 0 {
                break;
            }
            from -= 1;
            while from > 0 && s[from] & 0xc0 == 0x80 {
                from -= 1;
            }
        }
        if self.utf_checked_from.is_some_and(|c| c <= from) {
            return Ok(());
        }
        if let Err((e, off)) = utf8::validate(&s[from..]) {
            return Err(MatchError::BadUtf8 {
                error: e,
                offset: from + off,
            });
        }
        self.utf_checked_from = Some(from);
        Ok(())
    }

    /// Where to try next after a failed attempt at `p` (pcre2api "Newline
    /// handling when matching": a CRLF is skipped as a unit when the
    /// pattern has no explicit CR or LF).
    #[inline]
    fn bump(&self, p: usize) -> usize {
        let s = self.vm.s;
        let prog = self.vm.prog;
        let len = s.len();
        let mut q = p + if self.vm.utf {
            utf8::len_from_lead(s[p])
        } else {
            1
        };
        if matches!(
            prog.newline,
            Newline::CrLf | Newline::AnyCrLf | Newline::Any
        ) && s[p] == b'\r'
            && q < len
            && s[q] == b'\n'
            && !prog.has_cr_lf
        {
            q += 1;
        }
        q.min(len)
    }

    /// Finds the first match at or after `start_offset`.
    pub fn find(
        &mut self,
        sc: &mut Scratch,
        start_offset: usize,
        opts: MatchOptions,
        out_slots: &mut Vec<usize>,
    ) -> Result<Option<MatchData>, MatchError> {
        let s = self.vm.s;
        let prog = self.vm.prog;
        let len = s.len();
        if start_offset > len {
            return Err(MatchError::BadOffset);
        }
        let utf = prog.utf;
        if utf {
            self.check_utf(start_offset, opts)?;
        }
        let ncap = prog.ncap as usize;
        let (open_base, keep_slot, nslots) = (self.vm.open_base, self.vm.keep_slot, self.vm.nslots);
        if sc.slots.len() != nslots {
            sc.slots.clear();
            sc.slots.resize(nslots, UNSET);
        } else {
            // Only the captures and \K can hold values from a previous
            // match (the other registers are written before they are read).
            sc.slots[..open_base].fill(UNSET);
            sc.slots[keep_slot] = UNSET;
        }
        if !sc.bt.is_empty() {
            sc.bt.clear();
        }
        if !sc.calls.is_empty() || !sc.popped.is_empty() || !sc.arena.is_empty() {
            sc.calls.clear();
            sc.popped.clear();
            sc.arena.clear();
        }
        let vm = &mut self.vm;
        vm.end = len;
        vm.start_offset = start_offset;
        vm.opts = opts;
        vm.mark = NO_MARK;
        let limits = vm.limits;
        let st = &prog.start;
        let anchored = st.anchored || opts.anchored;
        let mut p = start_offset;
        let onepass = prog.onepass.as_ref().filter(|op| {
            limits.match_limit > op.frames
                && limits.depth_limit > op.frames
                && (!op.has_greedy || (len as u64) < limits.match_limit as u64 / 2)
        });
        // Required byte: fail early when it is absent (pcre2pattern on the
        // "last single character that is required for a match").
        let mut req_pos: Option<usize> = None;
        loop {
            if len - p < st.min_len {
                break;
            }
            if let Some(op) = onepass
                && let (Some(lit), Some(li)) = (&op.lead_lit, op.lead_run)
                && !anchored
            {
                // Find the literal, then the start of the run before it.
                let found = match &op.lead_finder {
                    Some(f) => f.find(&s[p..]),
                    None => memchr::memchr(lit[0], &s[p..]),
                };
                let Some(k) = found else { break };
                let q = p + k;
                let (OpStep::Rep { item, min, .. } | OpStep::BackRep { item, min, .. }) =
                    &op.steps[li]
                else {
                    unreachable!()
                };
                let mut r = q;
                while r > p && self.vm.match_item(item, r - 1).is_some() {
                    r -= 1;
                }
                if q - r < *min as usize {
                    p = q + 1;
                    continue;
                }
                let mut run_end = 0;
                match self.vm.run_onepass(op, r, &mut sc.slots, &mut run_end)? {
                    Some((ms, me)) if !self.vm.reject_empty(ms, me) => {
                        out_slots.clear();
                        out_slots.extend_from_slice(&sc.slots[..2 * (ncap + 1)]);
                        out_slots[0] = ms;
                        out_slots[1] = me;
                        return Ok(Some(MatchData {
                            start: ms,
                            end: me,
                            mark: None,
                        }));
                    }
                    _ => {
                        // Starts inside the run can only do worse.
                        p = (q + 1).max(run_end + 1);
                        continue;
                    }
                }
            }
            if !anchored {
                if st.startline && p > 0 && !self.vm.newline_before(p) {
                    // Advance to the next line start.
                    let q = if prog.newline == Newline::Lf {
                        match memchr::memchr(b'\n', &s[p..]) {
                            Some(k) => p + k + 1,
                            None => len,
                        }
                    } else {
                        let mut q = p + 1;
                        while q < len && !self.vm.newline_before(q) {
                            q += 1;
                        }
                        q
                    };
                    if q >= len {
                        break;
                    }
                    p = q;
                    continue;
                }
                if !st.prefix.is_empty() {
                    let pre = &st.prefix;
                    let found = if pre.len() == 1 {
                        memchr::memchr(pre[0], &s[p..])
                    } else if let Some(f) = &st.finder {
                        f.find(&s[p..])
                    } else {
                        memchr::memmem::find(&s[p..], pre)
                    };
                    match found {
                        Some(k) => p += k,
                        None => break,
                    }
                    if st.startline && p > 0 && !self.vm.newline_before(p) {
                        continue;
                    }
                } else if let Some((ai, bytes)) = &st.anchor {
                    // Search for the rarest fixed-offset byte set.
                    let from = p + ai;
                    if from >= len {
                        break;
                    }
                    let hay = &s[from..];
                    let found = match bytes.as_slice() {
                        [a] => memchr::memchr(*a, hay),
                        [a, b] => memchr::memchr2(*a, *b, hay),
                        [a, b, c] => memchr::memchr3(*a, *b, *c, hay),
                        _ => None,
                    };
                    match found {
                        Some(k) => p = from + k - ai,
                        None => break,
                    }
                    if st.startline && p > 0 && !self.vm.newline_before(p) {
                        continue;
                    }
                } else if let Some(fb) = &st.first_bytes {
                    let found = match st.first_scan {
                        FirstScan::One(a) => memchr::memchr(a, &s[p..]).map(|k| p + k),
                        FirstScan::Two(a, b) => memchr::memchr2(a, b, &s[p..]).map(|k| p + k),
                        FirstScan::Three(a, b, c) => {
                            memchr::memchr3(a, b, c, &s[p..]).map(|k| p + k)
                        }
                        FirstScan::HighBit => find_high_bit(s, p),
                        FirstScan::Table => {
                            s[p..].iter().position(|&b| fb[b as usize]).map(|k| p + k)
                        }
                    };
                    match found {
                        Some(q) => p = q,
                        None => break,
                    }
                    if st.startline && p > 0 && !self.vm.newline_before(p) {
                        continue;
                    }
                }
                if !st.sets.is_empty() {
                    let n = st.sets.len();
                    if p + n > len {
                        break;
                    }
                    let ok = st
                        .sets
                        .iter()
                        .zip(&s[p..p + n])
                        .all(|(set, &b)| set[(b >> 6) as usize] >> (b & 63) & 1 != 0);
                    if !ok {
                        p += 1;
                        continue;
                    }
                }
                if let Some((negated, ucp)) = st.lead_wb
                    && !self
                        .vm
                        .check_assert(Assertion::WordBoundary { negated, ucp }, p)
                {
                    if !negated && !ucp && p < len && class::is_ascii_word(s[p] as u32) {
                        // Inside a word: no \b until the word ends.
                        while p < len && class::is_ascii_word(s[p] as u32) {
                            p += 1;
                        }
                        continue;
                    }
                    p += if utf && p < len {
                        utf8::len_from_lead(s[p])
                    } else {
                        1
                    };
                    if p > len {
                        break;
                    }
                    continue;
                }
                if let Some((a, b)) = st.required
                    && req_pos.is_none_or(|r| r < p)
                {
                    let found = if a == b {
                        memchr::memchr(a, &s[p..])
                    } else {
                        memchr::memchr2(a, b, &s[p..])
                    };
                    match found {
                        Some(k) => req_pos = Some(p + k),
                        None => break,
                    }
                }
            }
            if let Some(op) = onepass {
                let mut run_end = 0;
                match self.vm.run_onepass(op, p, &mut sc.slots, &mut run_end)? {
                    Some((ms, me)) if !self.vm.reject_empty(ms, me) => {
                        out_slots.clear();
                        out_slots.extend_from_slice(&sc.slots[..2 * (ncap + 1)]);
                        out_slots[0] = ms;
                        out_slots[1] = me;
                        return Ok(Some(MatchData {
                            start: ms,
                            end: me,
                            mark: None,
                        }));
                    }
                    _ => {
                        if anchored || p >= len {
                            break;
                        }
                        p = if op.lead_run.is_some() && run_end > p {
                            // Starts inside the same run fail the same way.
                            run_end + 1
                        } else {
                            self.bump(p)
                        };
                        if p > len {
                            break;
                        }
                        continue;
                    }
                }
            }
            let r = self.vm.run(sc, p)?;
            match r {
                Attempt::Match(ms, me) => {
                    out_slots.clear();
                    out_slots.extend_from_slice(&sc.slots[..2 * (ncap + 1)]);
                    out_slots[0] = ms;
                    out_slots[1] = me;
                    let vm = &self.vm;
                    let mark = (vm.mark != NO_MARK).then(|| prog.names[vm.mark].clone());
                    return Ok(Some(MatchData {
                        start: ms,
                        end: me,
                        mark,
                    }));
                }
                Attempt::Commit => break,
                Attempt::NoMatch | Attempt::Prune | Attempt::Skip(_) => {
                    // The stack is empty and every slot change was undone.
                    if anchored || p >= len {
                        break;
                    }
                    p = match r {
                        Attempt::Skip(q) if q > p => q.min(len),
                        _ => self.bump(p),
                    };
                }
            }
        }
        Ok(None)
    }
}

/// Runs a match: tries start positions from `start_offset` using the
/// start-of-match optimisations.
pub fn exec(
    prog: &Prog,
    sc: &mut Scratch,
    s: &[u8],
    start_offset: usize,
    opts: MatchOptions,
    limits: Limits,
    out_slots: &mut Vec<usize>,
) -> Result<Option<MatchData>, MatchError> {
    Searcher::new(prog, s, limits).find(sc, start_offset, opts, out_slots)
}
