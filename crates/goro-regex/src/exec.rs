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
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            match_limit: 10_000_000,
            depth_limit: 10_000_000,
        }
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

struct Vm<'a> {
    prog: &'a Prog,
    s: &'a [u8],
    end: usize,
    utf: bool,
    start_offset: usize,
    /// Start of the current attempt.
    attempt_start: usize,
    opts: MatchOptions,
    limits: Limits,
    count: u64,
    frames: u32,
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

impl<'a> Vm<'a> {
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
        sc.bt.push(Bt { kind, a, b, c });
        Ok(())
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
                Item::Set(s) => s.high_none && (128..256).all(|b| !s.has_byte(b as u8)),
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
                    let bits = &set.bits;
                    while pos < stop {
                        let b = s[pos];
                        if bits[(b >> 6) as usize] >> (b & 63) & 1 == 0 {
                            break;
                        }
                        pos += 1;
                    }
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
        let mut removed_frames = 0u32;
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
            } else if is_frame(e.kind) {
                removed_frames += 1;
            }
        }
        sc.bt.truncate(w);
        self.frames = self.frames.saturating_sub(removed_frames);
    }

    /// Pops entries down to and including `idx`, applying undo records.
    fn unwind_to(&mut self, sc: &mut Scratch, idx: usize) {
        while sc.bt.len() > idx {
            let e = sc.bt.pop().unwrap();
            self.apply_undo(sc, e);
        }
    }

    #[inline]
    fn apply_undo(&mut self, sc: &mut Scratch, e: Bt) {
        match e.kind {
            K_SLOT => sc.slots[e.a as usize] = e.b,
            K_MARK | K_MARK_HID => {
                self.mark = e.b;
                self.frames = self.frames.saturating_sub(1);
            }
            K_MARK_UNDO => self.mark = e.b,
            K_CALL_PUSH => {
                let f = sc.calls.pop().unwrap();
                sc.arena.truncate(f.arena_off);
                self.frames = self.frames.saturating_sub(1);
            }
            K_CALL_POP => {
                let f = sc.popped.pop().unwrap();
                sc.calls.push(f);
            }
            K_SCAN => self.end = e.c,
            k if is_frame(k) => self.frames = self.frames.saturating_sub(1),
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
            match e.kind {
                K_SLOT => sc.slots[e.a as usize] = e.b,
                K_ALT => {
                    self.frames -= 1;
                    return Ok(Step::Resume(e.a, e.b));
                }
                K_ALT_NC => return Ok(Step::Resume(e.a, e.b)),
                K_ALT_LAZY => {
                    self.frames -= 1;
                    self.count += 1;
                    if self.count > self.limits.match_limit as u64 {
                        return Err(MatchError::MatchLimit);
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
                    self.frames -= 1;
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
                    self.frames -= 1;
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
                K_FRAME | K_THEN_SCOPE | K_ATOMIC | K_SR => {
                    if is_frame(e.kind) {
                        self.frames -= 1;
                    }
                }
                K_LOOK => {
                    self.frames -= 1;
                    // The assertion body failed.
                    match e.c {
                        1 | 4 => return Ok(Step::Resume(e.a + 1, e.b)),
                        3 => return Ok(Step::Resume(self.cond_no(e.a), e.b)),
                        _ => {}
                    }
                }
                K_BEHIND => {
                    self.frames -= 1;
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
                    self.frames -= 1;
                }
                K_MARK_UNDO => self.mark = e.b,
                K_VERB => {
                    self.frames -= 1;
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
                    self.frames = self.frames.saturating_sub(1);
                }
                K_CALL_POP => {
                    let f = sc.popped.pop().unwrap();
                    sc.calls.push(f);
                }
                K_SCAN => {
                    self.frames -= 1;
                    self.end = e.c;
                }
                _ => {}
            }
        }
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
                    self.frames = self.frames.saturating_sub(1);
                    // The subroutine call fails; backtrack at the outer level.
                    return self.backtrack(sc);
                }
                K_LOOK => {
                    self.frames -= 1;
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
                    self.frames -= 1;
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
                            if min < max {
                                self.push_frame(sc, K_REP_LAZY, pc, min as usize, p)?;
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
                    self.push(sc, if *count { K_ALT_BR } else { K_ALT_NC }, *alt, pos, 0);
                    pc += 1;
                }
                Inst::SplitLazy { to } => {
                    self.push_frame(sc, K_ALT, pc + 1, pos, 0)?;
                    pc = *to;
                }
                Inst::SplitTag { alt, tag } => {
                    self.push(sc, K_ALT_TAG, *alt, pos, *tag as usize);
                    pc += 1;
                }
                Inst::ThenScope { tag } => {
                    self.push(sc, K_THEN_SCOPE, *tag, 0, 0);
                    pc += 1;
                }
                Inst::Jmp(t) => pc = *t,
                Inst::CapOpen(g) => {
                    self.push_frame(sc, K_FRAME, 0, 0, 0)?;
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
                        self.push_frame(sc, K_ALT, *start, pos, 0)?;
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
                            return Err(MatchError::RecurseLoop);
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
    if start_offset > s.len() {
        return Err(MatchError::BadOffset);
    }
    let utf = prog.utf;
    if utf && !opts.no_utf_check {
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
        if let Err((e, off)) = utf8::validate(&s[from..]) {
            return Err(MatchError::BadUtf8 {
                error: e,
                offset: from + off,
            });
        }
        if start_offset > 0 && start_offset < s.len() && s[start_offset] & 0xc0 == 0x80 {
            return Err(MatchError::BadUtf8Offset);
        }
    }
    let ncap = prog.ncap as usize;
    let open_base = 2 * (ncap + 1);
    let loop_base = open_base + ncap + 1;
    let keep_slot = loop_base + prog.nloops as usize;
    let nslots = keep_slot + 1;
    sc.slots.clear();
    sc.slots.resize(nslots, UNSET);
    sc.bt.clear();
    sc.calls.clear();
    sc.popped.clear();
    sc.arena.clear();
    let mut limits = limits;
    if let Some(l) = prog.limit_match {
        limits.match_limit = limits.match_limit.min(l);
    }
    if let Some(l) = prog.limit_depth {
        limits.depth_limit = limits.depth_limit.min(l);
    }
    let mut vm = Vm {
        prog,
        s,
        end: s.len(),
        utf,
        start_offset,
        attempt_start: start_offset,
        opts,
        limits,
        count: 0,
        frames: 0,
        mark: NO_MARK,
        last_mark: NO_MARK,
        open_base,
        loop_base,
        keep_slot,
        nslots,
    };
    let st = &prog.start;
    let anchored = st.anchored || opts.anchored;
    let len = s.len();
    let mut p = start_offset;
    // Required byte: fail early when it is absent (pcre2pattern on the
    // "last single character that is required for a match").
    let mut req_pos: Option<usize> = None;
    loop {
        if len - p < st.min_len {
            break;
        }
        if !anchored {
            if st.startline && p > 0 && !vm.newline_before(p) {
                // Advance to the next line start.
                let mut q = p + 1;
                while q < len && !vm.newline_before(q) {
                    q += 1;
                }
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
                } else {
                    memchr::memmem::find(&s[p..], pre)
                };
                match found {
                    Some(k) => p += k,
                    None => break,
                }
                if st.startline && p > 0 && !vm.newline_before(p) {
                    continue;
                }
            } else if let Some(fb) = &st.first_bytes {
                while p < len && !fb[s[p] as usize] {
                    p += 1;
                }
                if p >= len {
                    break;
                }
                if st.startline && p > 0 && !vm.newline_before(p) {
                    continue;
                }
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
        let r = vm.run(sc, p)?;
        match r {
            Attempt::Match(ms, me) => {
                out_slots.clear();
                out_slots.extend_from_slice(&sc.slots[..2 * (ncap + 1)]);
                out_slots[0] = ms;
                out_slots[1] = me;
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
                let next = match r {
                    Attempt::Skip(q) if q > p => q,
                    _ => {
                        let mut q = p + if utf { utf8::len_from_lead(s[p]) } else { 1 };
                        // CRLF bumpalong (pcre2api "Newline handling when
                        // matching").
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
                        q
                    }
                };
                p = next.min(len);
            }
        }
    }
    Ok(None)
}
