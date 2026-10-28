//! Turns the parsed pattern into a [`Prog`]: resolves group references,
//! checks the constraints PCRE2 enforces after parsing (lookbehind lengths,
//! conditional branches), emits instructions and computes start-of-match
//! data.

use crate::ast::*;
use crate::class::{self, CaseMode, CharSet};
use crate::error::{CompileError, ErrorKind as E};
use crate::parse::Parsed;
use crate::prog::*;
use crate::utf8;
use std::collections::HashMap;
use std::sync::Arc;

type CResult<T> = Result<T, CompileError>;

/// pcre2api: the default maximum length of a variable-length lookbehind
/// branch is 255 characters.
const MAX_VARLOOKBEHIND: u32 = 255;
/// Fixed-length lookbehind branches may be up to 65535 characters.
const MAX_LOOKBEHIND: u32 = 65535;
/// PCRE2 built with LINK_SIZE=2 limits compiled patterns to 64K code units;
/// our estimate of PCRE2's compiled size is checked against this.
const MAX_PATTERN_SIZE: usize = 65536;

pub struct Compiled {
    pub prog: Prog,
    pub names: Vec<(Arc<[u8]>, u32)>,
}

pub fn compile(mut parsed: Parsed, flags: crate::Flags) -> CResult<Compiled> {
    let mut node = std::mem::replace(&mut parsed.node, Node::Empty);
    // Before references are checked (observed on PHP 8.5).
    if parsed.has_ext_class {
        let mut lb = false;
        walk(&node, &mut |x| {
            if let Node::Group {
                kind: GroupKind::LookBehind { .. },
                ..
            } = x
            {
                lb = true;
            }
        });
        if lb {
            return Err(CompileError::new(E::InternalCheckLookbehinds, 0));
        }
    }
    Resolver {
        ncap: parsed.ncap,
        names: &parsed.names,
    }
    .resolve(&mut node)?;
    // Group nodes are needed for lookbehind length computation of calls.
    let mut groups = HashMap::new();
    collect_groups(&node, &mut groups);
    let mut r = Checker {
        group_nodes: groups,
        lengths: HashMap::new(),
        utf: parsed.utf,
        enclosing: Vec::new(),
    };
    r.check(&node)?;
    // Conditional groups with more than two branches are reported after
    // references and lookbehinds, at offset 0 (observed on PHP 8.5).
    let mut too_many = false;
    walk(&node, &mut |x| {
        if let Node::Cond { cond, branches, .. } = x
            && !matches!(cond, Condition::Define)
            && *branches > 2
        {
            too_many = true;
        }
    });
    if too_many {
        return Err(CompileError::new(E::TooManyConditionBranches, 0));
    }
    let max_lookbehind = r.max_lookbehind(&node);

    // 7: the enclosing bracket, its end and OP_END.
    let size = estimate_size(&node, parsed.utf) + 7;
    if size > MAX_PATTERN_SIZE {
        return Err(CompileError::new(E::PatternTooLarge, parsed.pattern_len));
    }

    let mut em = Emitter {
        insts: Vec::new(),
        lits: Vec::new(),
        names: Vec::new(),
        group_pc: vec![0; parsed.ncap as usize + 1],
        nloops: 0,
        utf: parsed.utf,
        ucp: parsed.ucp,
        open_caps: Vec::new(),
        then_tags: Vec::new(),
        next_tag: 1,
        has_verbs: false,
        no_auto_possess: parsed.no_auto_possess,
    };
    em.emit(&node, Cx::END)?;
    em.push(Inst::Match);
    let start = if parsed.no_start_opt {
        // Explicit anchors still anchor; the .* rule is a start-up
        // optimisation and is disabled with the others.
        StartInfo {
            anchored: flags.contains(crate::Flags::ANCHORED)
                || anchored(&node, false, true, &AnchorCtx::new(&node)),
            ..StartInfo::default()
        }
    } else {
        start_info(&node, &parsed, flags)
    };
    let prog = Prog {
        insts: em.insts,
        lits: em.lits,
        names: em.names,
        group_pc: em.group_pc,
        ncap: parsed.ncap,
        nloops: em.nloops,
        utf: parsed.utf,
        ucp: parsed.ucp,
        newline: parsed.newline,
        start,
        max_lookbehind,
        has_verbs: em.has_verbs,
        notempty: parsed.notempty,
        notempty_atstart: parsed.notempty_atstart,
        limit_match: parsed.limit_match,
        limit_depth: parsed.limit_depth,
        limit_heap: parsed.limit_heap,
        match_unset_backref: flags.contains(crate::Flags::MATCH_UNSET_BACKREF),
        turkish: parsed.turkish,
        has_cr_lf: parsed.has_cr_lf,
        onepass: None,
    };
    let mut prog = prog;
    prog.onepass = build_onepass(&prog);
    if prog.start.prefix.len() > 1 {
        prog.start.finder = Some(memchr::memmem::Finder::new(&prog.start.prefix).into_owned());
    }
    Ok(Compiled {
        prog,
        names: parsed.names,
    })
}

fn collect_groups<'n>(n: &'n Node, out: &mut HashMap<u32, &'n Node>) {
    walk(n, &mut |x| {
        if let Node::Group {
            kind: GroupKind::Capture(g),
            ..
        } = x
        {
            out.entry(*g).or_insert(x);
        }
    });
}

/// Pre-order traversal.
fn walk<'n>(n: &'n Node, f: &mut dyn FnMut(&'n Node)) {
    f(n);
    match n {
        Node::Seq(v) | Node::Alt(v) => v.iter().for_each(|c| walk(c, f)),
        Node::Group { body, .. } => walk(body, f),
        Node::Repeat { node, .. } => walk(node, f),
        Node::Cond { cond, yes, no, .. } => {
            if let Condition::Assert(a) = cond {
                walk(a, f);
            }
            walk(yes, f);
            if let Some(n) = no {
                walk(n, f);
            }
        }
        Node::ScanSubstring { body, .. } => walk(body, f),
        _ => {}
    }
}

struct Resolver<'a> {
    ncap: u32,
    names: &'a [(Arc<[u8]>, u32)],
}

struct Checker<'n> {
    group_nodes: HashMap<u32, &'n Node>,
    lengths: HashMap<u32, Option<(u32, u32)>>,
    utf: bool,
    /// Capture groups enclosing the node being checked.
    enclosing: Vec<u32>,
}

impl Resolver<'_> {
    fn groups_named(&self, name: &[u8]) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        for (n, g) in self.names {
            if n.as_ref() == name && !v.contains(g) {
                v.push(*g);
            }
        }
        v
    }

    fn resolve_ref(&self, r: &mut GroupRef, allow_zero: bool) -> CResult<()> {
        if let Some(name) = &r.name {
            let gs = self.groups_named(name);
            if gs.is_empty() {
                return Err(CompileError::new(E::BadSubpatternReference, r.offset));
            }
            r.num = gs[0];
            r.groups = gs;
        } else {
            if r.num > self.ncap || (r.num == 0 && !allow_zero) {
                return Err(CompileError::new(E::BadSubpatternReference, r.offset));
            }
            r.groups = vec![r.num];
        }
        Ok(())
    }

    fn resolve(&mut self, n: &mut Node) -> CResult<()> {
        match n {
            Node::Seq(v) | Node::Alt(v) => {
                for c in v {
                    self.resolve(c)?;
                }
            }
            Node::Group { body, .. } => self.resolve(body)?,
            Node::Repeat { node, .. } => self.resolve(node)?,
            Node::Backref { target, .. } => self.resolve_ref(target, false)?,
            Node::Call { target } => self.resolve_ref(target, true)?,
            Node::ScanSubstring { groups, body } => {
                for g in groups.iter_mut() {
                    self.resolve_ref(g, false)?;
                }
                self.resolve(body)?;
            }
            Node::Cond {
                cond,
                yes,
                no,
                branches,
                offset,
            } => {
                match cond {
                    Condition::GroupSet(r) => self.resolve_ref(r, false)?,
                    Condition::Recursion(Some(r)) => {
                        // Bare (?(Rn): a group named "Rn" wins.
                        if let Some(name) = r.name.clone()
                            && name.first() == Some(&b'R')
                            && name[1..].iter().all(u8::is_ascii_digit)
                            && r.num != u32::MAX - 1
                        {
                            let gs = self.groups_named(&name);
                            if !gs.is_empty() {
                                r.num = gs[0];
                                r.groups = gs;
                                *cond = Condition::GroupSet(r.clone());
                            } else if name.len() == 1 {
                                *cond = Condition::Recursion(None);
                            } else {
                                r.name = None;
                                self.resolve_ref(r, false)?;
                            }
                        } else {
                            self.resolve_ref(r, false)?;
                        }
                    }
                    Condition::Assert(a) => self.resolve(a)?,
                    _ => {}
                }
                if matches!(cond, Condition::Define) && *branches > 1 {
                    return Err(CompileError::new(E::DefineTooManyBranches, *offset + 3));
                }
                self.resolve(yes)?;
                if let Some(no) = no {
                    self.resolve(no)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Checker<'_> {
    /// Checks lookbehinds (in pattern order).
    fn check(&mut self, n: &Node) -> CResult<()> {
        match n {
            Node::Seq(v) | Node::Alt(v) => {
                for c in v {
                    self.check(c)?;
                }
            }
            Node::Group {
                kind: GroupKind::LookBehind { .. },
                body,
                offset,
            } => {
                self.check_lookbehind(body, *offset)?;
                self.check(body)?;
            }
            Node::Group {
                kind: GroupKind::Capture(g),
                body,
                ..
            } => {
                self.enclosing.push(*g);
                self.check(body)?;
                self.enclosing.pop();
            }
            Node::Group { body, .. } => self.check(body)?,
            Node::Repeat { node, .. } => self.check(node)?,
            Node::Cond { cond, yes, no, .. } => {
                if let Condition::Assert(a) = cond {
                    self.check(a)?;
                }
                self.check(yes)?;
                if let Some(no) = no {
                    self.check(no)?;
                }
            }
            Node::ScanSubstring { body, .. } => self.check(body)?,
            _ => {}
        }
        Ok(())
    }

    fn check_lookbehind(&mut self, body: &Node, offset: usize) -> CResult<()> {
        let branches: Vec<&Node> = match body {
            Node::Alt(v) => v.iter().collect(),
            other => vec![other],
        };
        for b in branches {
            if self.utf && contains_code_unit(b) {
                return Err(CompileError::new(E::LookbehindBadBackslashC, offset));
            }
            let mut stack = Vec::new();
            match self.length(b, &mut stack) {
                None => return Err(CompileError::new(E::LookbehindNotFixedLength, offset)),
                Some((min, max)) => {
                    if min != max && max > MAX_VARLOOKBEHIND {
                        return Err(CompileError::new(
                            E::VariableLengthLookbehindTooLong,
                            offset,
                        ));
                    }
                    if max > MAX_LOOKBEHIND {
                        return Err(CompileError::new(E::LookbehindTooLong, offset));
                    }
                }
            }
        }
        Ok(())
    }

    fn max_lookbehind(&mut self, n: &Node) -> u32 {
        let mut best = 0;
        let mut lbs = Vec::new();
        walk(n, &mut |x| {
            if let Node::Group {
                kind: GroupKind::LookBehind { .. },
                body,
                ..
            } = x
            {
                lbs.push(&**body);
            }
            if let Node::Assert(Assertion::WordBoundary { .. }) = x {
                lbs.push(x);
            }
        });
        for b in lbs {
            if let Node::Assert(_) = b {
                best = best.max(1);
                continue;
            }
            let mut st = Vec::new();
            if let Some((_, max)) = self.length(b, &mut st) {
                best = best.max(max);
            }
        }
        best
    }

    /// (min, max) length in characters, or None if unbounded.
    fn length(&mut self, n: &Node, stack: &mut Vec<u32>) -> Option<(u32, u32)> {
        Some(match n {
            Node::Empty | Node::Assert(_) | Node::Keep | Node::Verb { .. } => (0, 0),
            Node::Char { .. }
            | Node::Dot { .. }
            | Node::NotNewline
            | Node::CodeUnit
            | Node::Set(_) => (1, 1),
            Node::Newline { .. } => (1, 2),
            Node::Grapheme => return None,
            Node::Seq(v) => {
                let (mut a, mut b) = (0u32, 0u32);
                for c in v {
                    let (x, y) = self.length(c, stack)?;
                    a = a.saturating_add(x);
                    b = b.saturating_add(y);
                }
                (a, b)
            }
            Node::Alt(v) => {
                let (mut a, mut b) = (u32::MAX, 0u32);
                for c in v {
                    let (x, y) = self.length(c, stack)?;
                    a = a.min(x);
                    b = b.max(y);
                }
                (a, b)
            }
            Node::Group { kind, body, .. } => match kind {
                GroupKind::LookAhead { .. } | GroupKind::LookBehind { .. } => (0, 0),
                _ => self.length(body, stack)?,
            },
            Node::Repeat { node, min, max, .. } => {
                let (x, y) = self.length(node, stack)?;
                let max = (*max)?;
                if y == 0 {
                    (0, 0)
                } else {
                    (x.saturating_mul(*min), y.saturating_mul(max))
                }
            }
            Node::Backref { target, .. } => {
                // A reference to a group that contains the lookbehind has no
                // known length (observed on PHP 8.5).
                if target.groups.len() != 1 || self.enclosing.contains(&target.groups[0]) {
                    return None;
                }
                self.group_length(target.groups[0], stack)?
            }
            Node::Call { target } => self.group_length(target.num, stack)?,
            Node::Cond { yes, no, .. } => {
                let (a, b) = self.length(yes, stack)?;
                let (c, d) = match no {
                    Some(n) => self.length(n, stack)?,
                    None => (0, 0),
                };
                (a.min(c), b.max(d))
            }
            Node::ScanSubstring { .. } => (0, 0),
        })
    }

    fn group_length(&mut self, g: u32, stack: &mut Vec<u32>) -> Option<(u32, u32)> {
        if g == 0 || stack.contains(&g) {
            return None;
        }
        if let Some(l) = self.lengths.get(&g) {
            return *l;
        }
        let node = *self.group_nodes.get(&g)?;
        stack.push(g);
        let l = match node {
            Node::Group { body, .. } => self.length(body, stack),
            _ => None,
        };
        stack.pop();
        self.lengths.insert(g, l);
        l
    }
}

fn contains_code_unit(n: &Node) -> bool {
    let mut found = false;
    walk(n, &mut |x| {
        if matches!(x, Node::CodeUnit) {
            found = true;
        }
    });
    found
}

/// PCRE2's compiled size of a node in code units (LINK_SIZE 2), for the
/// "regular expression is too large" check. The rules reproduce the
/// limits observed on PHP 8.5: 32764 literal characters, (a){1,3855},
/// (?:ab|cd){3854}, ([a-z]){1598}, (a|b|c){1,2427}, ...
fn estimate_size(n: &Node, utf: bool) -> usize {
    fn char_len(c: u32, utf: bool) -> usize {
        if !utf || c < 0x80 {
            1
        } else if c < 0x800 {
            2
        } else if c < 0x10000 {
            3
        } else {
            4
        }
    }
    match n {
        Node::Empty => 0,
        Node::Char { c, .. } => 1 + char_len(*c, utf),
        Node::Dot { .. }
        | Node::NotNewline
        | Node::CodeUnit
        | Node::Newline { .. }
        | Node::Grapheme => 1,
        Node::Set(s) => s.op_size as usize,
        Node::Assert(_) | Node::Keep => 1,
        Node::Seq(v) => v.iter().map(|c| estimate_size(c, utf)).sum(),
        Node::Alt(v) => {
            v.iter().map(|c| estimate_size(c, utf)).sum::<usize>() + 3 * (v.len().max(1) - 1)
        }
        Node::Group { kind, body, .. } => {
            let b = estimate_size(body, utf);
            match kind {
                GroupKind::Capture(_) => b + 8,
                _ => b + 6,
            }
        }
        Node::Repeat { node, min, max, .. } => {
            let b = estimate_size(node, utf);
            let (min, max) = (*min as usize, max.map(|m| m as usize));
            let class = matches!(&**node, Node::Set(s) if s.op_size >= 33);
            let single = matches!(
                **node,
                Node::Char { .. }
                    | Node::Dot { .. }
                    | Node::NotNewline
                    | Node::CodeUnit
                    | Node::Set(_)
            );
            if class {
                // The class followed by a repeat opcode.
                return b + if max.is_none() && min <= 1 || max == Some(1) {
                    1
                } else {
                    5
                };
            }
            if single {
                // Repeat opcodes replace a character's opcode; a type such
                // as \d or . is preceded by one; counts take two more
                // units; {n,m} needs an exact part and an upto part.
                let b = if matches!(**node, Node::Char { .. }) {
                    b
                } else {
                    b + 1
                };
                return match (min, max) {
                    (0 | 1, None) | (0, Some(1)) => b,
                    (_, Some(m)) if m == min => b + 2,
                    (0, Some(_)) => b + 2,
                    (_, Some(_)) => 2 * (b + 2),
                    (_, None) => (b + 2) + b,
                };
            }
            // Groups are replicated: mandatory copies, then one optional
            // copy (BRAZERO, a wrapping bracket and the group) per extra
            // repetition; an unbounded repeat ends in a looping copy.
            match max {
                None => {
                    if min == 0 {
                        1 + b
                    } else {
                        min.saturating_mul(b)
                    }
                }
                Some(m) => {
                    let mandatory = min.saturating_mul(b);
                    let optional = (m - min).saturating_mul(1 + 3 + b + 3);
                    mandatory.saturating_add(optional)
                }
            }
        }
        Node::Backref { .. } => 3,
        Node::Call { .. } => 3,
        Node::Cond { cond, yes, no, .. } => {
            let c = match cond {
                Condition::Assert(a) => estimate_size(a, utf),
                _ => 3,
            };
            3 + c
                + estimate_size(yes, utf)
                + no.as_ref().map_or(0, |n| estimate_size(n, utf) + 3)
                + 3
        }
        Node::ScanSubstring { body, .. } => estimate_size(body, utf) + 8,
        Node::Verb { name, .. } => 1 + name.as_ref().map_or(0, |n| n.len() + 2),
    }
}

// ---------------------------------------------------------------------------
// Emission

struct Emitter {
    insts: Vec<Inst>,
    lits: Vec<Box<[u8]>>,
    names: Vec<Arc<[u8]>>,
    group_pc: Vec<Pc>,
    nloops: u32,
    utf: bool,
    ucp: bool,
    /// Capture groups enclosing the current position (within the current
    /// lookaround), for (*ACCEPT).
    open_caps: Vec<u32>,
    /// Innermost alternation tags for (*THEN).
    then_tags: Vec<u32>,
    next_tag: u32,
    has_verbs: bool,
    no_auto_possess: bool,
}

/// Recognises deterministic programs (see [`OnePass`]).
fn build_onepass(prog: &Prog) -> Option<OnePass> {
    let insts = &prog.insts;
    let mut steps = Vec::new();
    let mut frames = 2u32;
    let mut has_greedy = false;
    let mut depends_on_start = false;
    let mut back: Option<usize> = None;
    for (pc, ins) in insts.iter().enumerate() {
        match ins {
            Inst::One(i) => steps.push(OpStep::One(i.clone())),
            Inst::Lit(i) => steps.push(OpStep::Lit(*i)),
            Inst::LitFold(i) => steps.push(OpStep::LitFold(*i)),
            Inst::CapOpen(g) => {
                frames += 1;
                steps.push(OpStep::Open(*g));
            }
            Inst::CapClose(g) => steps.push(OpStep::Close(*g)),
            Inst::Assert(a) => {
                if matches!(a, Assertion::StartOfMatch) {
                    depends_on_start = true;
                }
                steps.push(OpStep::Assert(*a));
            }
            Inst::Keep => {
                depends_on_start = true;
                steps.push(OpStep::Keep);
            }
            Inst::Rep {
                item,
                min,
                max,
                greed,
            } => {
                let make_back = |steps: &mut Vec<OpStep>, back: &mut Option<usize>, lazy: bool| {
                    if back.is_some() {
                        return false;
                    }
                    *back = Some(steps.len());
                    steps.push(OpStep::BackRep {
                        item: item.clone(),
                        min: *min,
                        max: *max,
                        lazy,
                    });
                    true
                };
                match greed {
                    Greed::Possessive => {}
                    Greed::Lazy => {
                        if !make_back(&mut steps, &mut back, true) {
                            return None;
                        }
                        frames += 1;
                        continue;
                    }
                    Greed::Greedy => {
                        // Deterministic if only $, \z or \Z can follow and
                        // giving back characters cannot reach a newline.
                        let next = insts[pc + 1..]
                            .iter()
                            .find(|i| !matches!(i, Inst::CapClose(_)));
                        let at_dollar = matches!(
                            next,
                            Some(Inst::Assert(
                                Assertion::Dollar { .. }
                                    | Assertion::EndSubject
                                    | Assertion::EndSubjectOrNewline
                            ))
                        );
                        let after = insts[pc + 1..]
                            .iter()
                            .skip_while(|i| matches!(i, Inst::CapClose(_)))
                            .nth(1);
                        let then_end = matches!(after, Some(Inst::Match));
                        let no_nl = matches!(item, Item::AnyNoNl)
                            || [0x0a, 0x0b, 0x0c, 0x0d, 0x85, 0x2028, 0x2029]
                                .iter()
                                .all(|&c| !item_matches_cp(item, c));
                        frames += 1;
                        if !(at_dollar && then_end && no_nl) {
                            if !make_back(&mut steps, &mut back, false) {
                                return None;
                            }
                            continue;
                        }
                        has_greedy = true;
                    }
                }
                steps.push(OpStep::Rep {
                    item: item.clone(),
                    min: *min,
                    max: *max,
                });
            }
            Inst::Match => {
                if pc + 1 != insts.len() {
                    return None;
                }
                break;
            }
            _ => return None,
        }
    }
    // With a CRLF-aware bumpalong some start positions are never tried
    // (pcre2api "Newline handling when matching"); skipping by runs or
    // literals would try them.
    let crlf_bump = matches!(
        prog.newline,
        Newline::CrLf | Newline::AnyCrLf | Newline::Any
    ) && !prog.has_cr_lf;
    if crlf_bump {
        depends_on_start = true;
    }
    // The leading run optimisation (group openings and \b may come first).
    let first = steps.iter().position(|s| {
        !matches!(
            s,
            OpStep::Open(_) | OpStep::Assert(Assertion::WordBoundary { negated: false, .. })
        )
    });
    let lead_run = match first.map(|i| (i, &steps[i])) {
        Some((i, OpStep::Rep { item, min, max } | OpStep::BackRep { item, min, max, .. }))
            if *min >= 1 && *max == u32::MAX && !depends_on_start =>
        {
            let single = match item {
                Item::Byte(_) | Item::Byte2(..) => true,
                Item::Set(s) => !prog.utf || (s.ascii_only()),
                _ => !prog.utf && !matches!(item, Item::Chars(_)),
            };
            // Lookbehind-like assertions after the run look at the same
            // characters whatever the start; \b right after the run does too.
            single.then_some(i)
        }
        _ => None,
    };
    // A possessive leading run followed by a literal byte the run cannot
    // match: matches are found from occurrences of that byte.
    // For a backtracking leading repeat, the literal may also lie inside
    // the run: a match then starts at the beginning of the run that holds
    // an occurrence of the literal.
    let lead_lit = lead_run.and_then(|i| {
        let lit: Vec<u8> = match steps[i + 1..]
            .iter()
            .find(|s| !matches!(s, OpStep::Close(_)))?
        {
            OpStep::One(Item::Byte(b)) => vec![*b],
            OpStep::Lit(l) => prog.lits[*l as usize].to_vec(),
            _ => return None,
        };
        match &steps[i] {
            OpStep::Rep { item, .. } => (!item_matches_cp(item, lit[0] as u32)).then_some(lit),
            OpStep::BackRep { lazy: false, .. } => Some(lit),
            _ => None,
        }
    });
    let lead_finder = lead_lit
        .as_ref()
        .filter(|l| l.len() > 1)
        .map(|l| memchr::memmem::Finder::new(l).into_owned());
    Some(OnePass {
        steps,
        frames,
        has_greedy,
        lead_run,
        back,
        lead_lit,
        lead_finder,
    })
}

/// What follows a node being emitted.
#[derive(Clone, Copy)]
struct Cx<'a> {
    /// Nothing that can fail follows within the pattern (so giving back
    /// characters cannot help).
    last: bool,
    /// The node that follows, when known.
    follow: Option<&'a Node>,
}

impl Cx<'_> {
    const NONE: Cx<'static> = Cx {
        last: false,
        follow: None,
    };
    const END: Cx<'static> = Cx {
        last: true,
        follow: None,
    };
}

fn contains_capture(n: &Node) -> bool {
    let mut found = false;
    walk(n, &mut |x| {
        if let Node::Group {
            kind: GroupKind::Capture(_),
            ..
        } = x
        {
            found = true;
        }
    });
    found
}

/// Whether a (*THEN) inside `n` binds to an alternation enclosing `n`.
fn then_binds_outside(n: &Node) -> bool {
    match n {
        Node::Verb {
            verb: Verb::Then, ..
        } => true,
        Node::Seq(v) => v.iter().any(then_binds_outside),
        Node::Alt(v) => v.len() < 2 && v.iter().any(then_binds_outside),
        Node::Group { kind, body, .. } => match kind {
            GroupKind::LookAhead { .. } | GroupKind::LookBehind { .. } => false,
            _ => then_binds_outside(body),
        },
        Node::Repeat { node, .. } => then_binds_outside(node),
        Node::Cond { yes, no, .. } => {
            then_binds_outside(yes) || no.as_ref().is_some_and(|n| then_binds_outside(n))
        }
        _ => false,
    }
}

/// Converts a single-character node to an item.
fn item_of(n: &Node, utf: bool, ucp: bool) -> Option<Item> {
    Some(match n {
        Node::Char { c, caseless } => char_item(*c, *caseless, utf, ucp),
        Node::Dot { dotall: true } => Item::Any,
        Node::Dot { dotall: false } | Node::NotNewline => Item::AnyNoNl,
        Node::CodeUnit => Item::CodeUnit,
        Node::Set(s) => set_item(s, utf),
        _ => return None,
    })
}

fn char_item(c: u32, caseless: bool, utf: bool, ucp: bool) -> Item {
    if !caseless {
        if c < 0x80 || !utf {
            return Item::Byte(c as u8);
        }
        return Item::Chars(Arc::from(vec![c].as_slice()));
    }
    let mode = if utf || ucp {
        CaseMode::Unicode
    } else {
        CaseMode::Ascii
    };
    let mut v = Vec::new();
    class::case_equivalents(c, mode, &mut v);
    if !utf {
        v.retain(|&x| x < 256);
    }
    v.sort_unstable();
    v.dedup();
    if v.len() == 2 && v.iter().all(|&x| x < 0x80 || !utf) {
        return Item::Byte2(v[0] as u8, v[1] as u8);
    }
    if v.len() == 1 && (c < 0x80 || !utf) {
        return Item::Byte(c as u8);
    }
    Item::Chars(Arc::from(v.as_slice()))
}

fn set_item(s: &Arc<CharSet>, utf: bool) -> Item {
    if (!utf || s.high_none)
        && let Some(b) = s.single_byte()
        && (b < 0x80 || !utf)
    {
        return Item::Byte(b);
    }
    Item::Set(s.clone())
}

impl Emitter {
    fn pc(&self) -> Pc {
        self.insts.len() as Pc
    }

    fn push(&mut self, i: Inst) -> usize {
        self.insts.push(i);
        self.insts.len() - 1
    }

    fn name_idx(&mut self, name: &Arc<[u8]>) -> u32 {
        if let Some(i) = self.names.iter().position(|n| n == name) {
            return i as u32;
        }
        self.names.push(name.clone());
        (self.names.len() - 1) as u32
    }

    fn patch_split(&mut self, at: usize, target: Pc) {
        match &mut self.insts[at] {
            Inst::Split { alt } => *alt = target,
            Inst::Branch { alt, .. } => *alt = target,
            Inst::SplitTag { alt, .. } => *alt = target,
            Inst::SplitLazy { to } => *to = target,
            Inst::Jmp(t) => *t = target,
            Inst::LookStart { end, .. } => *end = target,
            Inst::LookEnd { no, .. } => *no = target,
            Inst::CondRef { no, .. } => *no = target,
            Inst::CondRecurse { no, .. } => *no = target,
            Inst::ScanStart { end, .. } => *end = target,
            _ => unreachable!("patch of non-jump"),
        }
    }

    /// Emits a literal run.
    fn emit_lit(&mut self, bytes: Vec<u8>, fold: bool) {
        if bytes.len() == 1 && !fold {
            self.push(Inst::One(Item::Byte(bytes[0])));
            return;
        }
        if bytes.len() == 1 && fold {
            let b = bytes[0];
            self.push(Inst::One(Item::Byte2(b.to_ascii_uppercase(), b)));
            return;
        }
        self.lits.push(bytes.into_boxed_slice());
        let idx = (self.lits.len() - 1) as u32;
        self.push(if fold {
            Inst::LitFold(idx)
        } else {
            Inst::Lit(idx)
        });
    }

    /// `cx` describes what follows the node (for auto-possessification).
    fn emit<'a>(&mut self, n: &'a Node, cx: Cx<'a>) -> CResult<()> {
        match n {
            Node::Empty => {}
            Node::Char { .. } => {
                let item = item_of(n, self.utf, self.ucp).unwrap();
                self.push(Inst::One(item));
            }
            Node::Seq(v) => self.emit_seq(v, cx)?,
            Node::Dot { .. } | Node::NotNewline | Node::CodeUnit | Node::Set(_) => {
                let item = item_of(n, self.utf, self.ucp).unwrap();
                self.push(Inst::One(item));
            }
            Node::Newline { anycrlf } => {
                self.push(Inst::Newline { anycrlf: *anycrlf });
            }
            Node::Grapheme => {
                self.push(Inst::Grapheme);
            }
            Node::Assert(a) => {
                self.push(Inst::Assert(*a));
            }
            Node::Keep => {
                self.push(Inst::Keep);
            }
            Node::Alt(v) => self.emit_alt(v, cx, true)?,
            Node::Group { kind, body, .. } => self.emit_group(*kind, body, cx)?,
            Node::Repeat {
                node,
                min,
                max,
                greed,
            } => self.emit_repeat(node, *min, *max, *greed, cx)?,
            Node::Backref {
                target,
                caseless,
                case_restrict,
            } => {
                self.push(Inst::Backref {
                    groups: Arc::from(target.groups.as_slice()),
                    caseless: *caseless,
                    restrict: *case_restrict,
                });
            }
            Node::Call { target } => {
                self.push(Inst::Call { group: target.num });
            }
            Node::Cond { cond, yes, no, .. } => self.emit_cond(cond, yes, no.as_deref(), cx)?,
            Node::Verb { verb, name } => self.emit_verb(verb, name.as_ref()),
            Node::ScanSubstring { groups, body } => {
                let gs: Vec<u32> = groups
                    .iter()
                    .flat_map(|g| g.groups.iter().copied())
                    .collect();
                let at = self.push(Inst::ScanStart {
                    groups: Arc::from(gs.as_slice()),
                    end: 0,
                });
                let saved = std::mem::take(&mut self.open_caps);
                self.emit(body, Cx::NONE)?;
                self.open_caps = saved;
                let end = self.pc();
                self.push(Inst::ScanEnd);
                self.patch_split(at, end);
            }
        }
        Ok(())
    }

    fn emit_seq<'a>(&mut self, v: &'a [Node], cx: Cx<'a>) -> CResult<()> {
        let mut i = 0;
        while i < v.len() {
            // Merge runs of literal characters.
            if let Node::Char { caseless, .. } = &v[i] {
                let fold = *caseless;
                let mut bytes = Vec::new();
                let mut j = i;
                while j < v.len() {
                    match &v[j] {
                        Node::Char { c, caseless: cl } if *cl == fold => {
                            if fold {
                                // Only ASCII letters fold with LitFold.
                                if *c >= 0x80 || !(*c as u8).is_ascii_alphabetic() {
                                    break;
                                }
                                if self.utf || self.ucp {
                                    // k and s have non-ASCII variants.
                                    let lc = (*c as u8).to_ascii_lowercase();
                                    if lc == b'k' || lc == b's' {
                                        break;
                                    }
                                }
                                bytes.push((*c as u8).to_ascii_lowercase());
                            } else if self.utf {
                                utf8::encode(*c, &mut bytes);
                            } else {
                                bytes.push(*c as u8);
                            }
                            j += 1;
                        }
                        _ => break,
                    }
                }
                // A repeat binds only to its own character, so the run
                // never includes repeated items.
                if j > i {
                    self.emit_lit(bytes, fold);
                    i = j;
                    continue;
                }
            }
            let icx = if i + 1 == v.len() {
                cx
            } else {
                Cx {
                    last: false,
                    follow: Some(&v[i + 1]),
                }
            };
            self.emit(&v[i], icx)?;
            i += 1;
        }
        Ok(())
    }

    /// `count_branches`: whether trying a later branch counts towards the
    /// match limit (observed on PHP 8.5: it does at the top level and in
    /// capture groups, not in non-capturing groups).
    fn emit_alt<'a>(&mut self, v: &'a [Node], cx: Cx<'a>, count_branches: bool) -> CResult<()> {
        let tag = if v.len() > 1 && v.iter().any(then_binds_outside) {
            let t = self.next_tag;
            self.next_tag += 1;
            self.push(Inst::ThenScope { tag: t });
            Some(t)
        } else {
            None
        };
        if let Some(t) = tag {
            self.then_tags.push(t);
        } else if v.len() > 1 {
            // A plain alternation still scopes (*THEN) found deeper.
            self.then_tags.push(0);
        }
        let mut jumps = Vec::new();
        for (k, b) in v.iter().enumerate() {
            let split = if k + 1 < v.len() {
                Some(self.push(match tag {
                    Some(t) => Inst::SplitTag { alt: 0, tag: t },
                    None => Inst::Branch {
                        alt: 0,
                        count: count_branches,
                    },
                }))
            } else {
                None
            };
            self.emit(b, cx)?;
            if k + 1 < v.len() {
                jumps.push(self.push(Inst::Jmp(0)));
            }
            if let Some(s) = split {
                let here = self.pc();
                self.patch_split(s, here);
            }
        }
        let end = self.pc();
        for j in jumps {
            self.patch_split(j, end);
        }
        if v.len() > 1 {
            self.then_tags.pop();
        }
        Ok(())
    }

    fn emit_group<'a>(&mut self, kind: GroupKind, body: &'a Node, cx: Cx<'a>) -> CResult<()> {
        match kind {
            GroupKind::NonCapture => {
                // A group with alternatives is a bracket of its own (one
                // frame); single-branch groups are not.
                match body {
                    Node::Alt(v) if v.len() > 1 => {
                        self.push(Inst::GroupFrame);
                        self.emit_alt(v, cx, false)?
                    }
                    _ => self.emit(body, cx)?,
                }
            }
            GroupKind::Capture(g) => {
                let pc = self.pc();
                if self.group_pc[g as usize] == 0 {
                    self.group_pc[g as usize] = pc;
                }
                self.push(Inst::CapOpen(g));
                self.open_caps.push(g);
                self.emit(body, cx)?;
                self.open_caps.pop();
                self.push(Inst::CapClose(g));
            }
            GroupKind::Atomic => {
                self.push(Inst::AtomicStart);
                // Nothing can backtrack into a completed atomic group.
                self.emit(body, Cx::END)?;
                self.push(Inst::AtomicEnd);
            }
            GroupKind::LookAhead { negated, atomic } => {
                let kind = if negated {
                    LookKind::Negative
                } else if atomic {
                    LookKind::Positive
                } else {
                    LookKind::NonAtomic
                };
                self.emit_look(kind, body, false)?;
            }
            GroupKind::LookBehind { negated, atomic } => {
                let kind = if negated {
                    LookKind::Negative
                } else if atomic {
                    LookKind::Positive
                } else {
                    LookKind::NonAtomic
                };
                self.emit_look(kind, body, true)?;
            }
            GroupKind::ScriptRun { .. } => {
                self.push(Inst::ScriptRunStart);
                self.emit(body, Cx::NONE)?;
                self.push(Inst::ScriptRunEnd);
            }
        }
        Ok(())
    }

    /// Emits a lookaround. Returns the index of the LookEnd.
    fn emit_look(&mut self, kind: LookKind, body: &Node, behind: bool) -> CResult<usize> {
        let start = self.push(Inst::LookStart { kind, end: 0 });
        let saved_caps = std::mem::take(&mut self.open_caps);
        let saved_tags = std::mem::take(&mut self.then_tags);
        // A lookbehind branch must end where the lookbehind started, so
        // its repeats may need to give characters back.
        let bcx = if kind == LookKind::NonAtomic || behind {
            Cx::NONE
        } else {
            Cx::END
        };
        if behind {
            let branches: Vec<&Node> = match body {
                Node::Alt(v) => v.iter().collect(),
                other => vec![other],
            };
            let tag = if branches.len() > 1 && branches.iter().any(|b| then_binds_outside(b)) {
                let t = self.next_tag;
                self.next_tag += 1;
                self.push(Inst::ThenScope { tag: t });
                Some(t)
            } else {
                None
            };
            self.then_tags.push(tag.unwrap_or(0));
            let mut jumps = Vec::new();
            for (k, b) in branches.iter().enumerate() {
                let split = if k + 1 < branches.len() {
                    Some(self.push(match tag {
                        Some(t) => Inst::SplitTag { alt: 0, tag: t },
                        None => Inst::Branch {
                            alt: 0,
                            count: true,
                        },
                    }))
                } else {
                    None
                };
                let (min, max) = branch_len(b);
                self.push(Inst::Behind { min, max });
                self.emit(b, bcx)?;
                self.push(Inst::BehindCheck);
                if k + 1 < branches.len() {
                    jumps.push(self.push(Inst::Jmp(0)));
                }
                if let Some(s) = split {
                    let here = self.pc();
                    self.patch_split(s, here);
                }
            }
            let end = self.pc();
            for j in jumps {
                self.patch_split(j, end);
            }
            self.then_tags.pop();
        } else {
            self.emit(body, bcx)?;
        }
        self.open_caps = saved_caps;
        self.then_tags = saved_tags;
        let end = self.push(Inst::LookEnd { kind, no: 0 });
        self.patch_split(start, end as Pc);
        Ok(end)
    }

    fn emit_cond<'a>(
        &mut self,
        cond: &Condition,
        yes: &'a Node,
        no: Option<&'a Node>,
        cx: Cx<'a>,
    ) -> CResult<()> {
        // The condition's test jumps to `no_label` when false.
        let test_at = match cond {
            Condition::GroupSet(r) => Some(self.push(Inst::CondRef {
                groups: Arc::from(r.groups.as_slice()),
                no: 0,
            })),
            Condition::Recursion(r) => Some(self.push(Inst::CondRecurse {
                groups: r.as_ref().map(|r| Arc::from(r.groups.as_slice())),
                no: 0,
            })),
            Condition::Define => Some(self.push(Inst::Jmp(0))),
            Condition::Version(true) => None,
            Condition::Version(false) => Some(self.push(Inst::Jmp(0))),
            Condition::Assert(a) => {
                let Node::Group { kind, body, .. } = &**a else {
                    unreachable!()
                };
                let (negated, behind) = match kind {
                    GroupKind::LookAhead { negated, .. } => (*negated, false),
                    GroupKind::LookBehind { negated, .. } => (*negated, true),
                    _ => unreachable!(),
                };
                Some(self.emit_look(LookKind::Cond { negated }, body, behind)?)
            }
        };
        self.emit(yes, cx)?;
        let jmp_end = self.push(Inst::Jmp(0));
        let no_label = self.pc();
        if let Some(n) = no {
            self.emit(n, cx)?;
        }
        let end = self.pc();
        self.patch_split(jmp_end, end);
        if let Some(t) = test_at {
            self.patch_split(t, no_label);
        }
        Ok(())
    }

    fn emit_verb(&mut self, verb: &Verb, name: Option<&Arc<[u8]>>) {
        self.has_verbs = true;
        let idx = name.map(|n| self.name_idx(n));
        match verb {
            Verb::Mark => {
                self.push(Inst::Mark(idx.unwrap()));
            }
            Verb::Accept => {
                if let Some(i) = idx {
                    self.push(Inst::SetMarkName(i));
                }
                let close: Vec<u32> = self.open_caps.iter().rev().copied().collect();
                self.push(Inst::Accept {
                    close: Arc::from(close.as_slice()),
                });
            }
            Verb::Fail => {
                if let Some(i) = idx {
                    self.push(Inst::SetMarkName(i));
                }
                self.push(Inst::Fail);
            }
            Verb::Commit => {
                if let Some(i) = idx {
                    self.push(Inst::SetMarkName(i));
                }
                self.push(Inst::Commit);
            }
            Verb::Prune => {
                if let Some(i) = idx {
                    self.push(Inst::SetMarkName(i));
                }
                self.push(Inst::Prune);
            }
            Verb::Skip => {
                self.push(Inst::Skip);
            }
            Verb::SkipName => {
                self.push(Inst::SkipName(idx.unwrap()));
            }
            Verb::Then => {
                if let Some(i) = idx {
                    self.push(Inst::SetMarkName(i));
                }
                let tag = self.then_tags.last().copied().unwrap_or(0);
                self.push(Inst::Then { tag });
            }
        }
    }

    fn emit_repeat<'a>(
        &mut self,
        node: &'a Node,
        min: u32,
        max: Option<u32>,
        greed: Greed,
        cx: Cx<'a>,
    ) -> CResult<()> {
        let (last, next) = (cx.last, cx.follow);
        if let Some(item) = item_of(node, self.utf, self.ucp) {
            let mut greed = greed;
            if greed == Greed::Greedy && !self.no_auto_possess && can_possessify(&item, next, last)
            {
                greed = Greed::Possessive;
            }
            // A lazy repeat followed by something it cannot match is
            // possessified too (observed on PHP 8.5: /[a-z]+?1/ needs no
            // backtracking frames).
            if greed == Greed::Lazy
                && !self.no_auto_possess
                && next.is_some()
                && can_possessify(&item, next, false)
            {
                greed = Greed::Possessive;
            }
            let max = max.unwrap_or(u32::MAX);
            if min == max && min > 0 {
                greed = Greed::Possessive;
            }
            self.push(Inst::Rep {
                item,
                min,
                max,
                greed,
            });
            return Ok(());
        }
        if max == Some(0) {
            // {0}: groups are kept (callable) but skipped.
            if contains_capture(node) {
                let j = self.push(Inst::Jmp(0));
                self.emit(node, Cx::NONE)?;
                let end = self.pc();
                self.patch_split(j, end);
            }
            return Ok(());
        }
        if greed == Greed::Possessive {
            self.push(Inst::AtomicStart);
            self.emit_repeat(node, min, max, Greed::Greedy, Cx::NONE)?;
            self.push(Inst::AtomicEnd);
            return Ok(());
        }
        let greedy = greed == Greed::Greedy;
        // Mandatory copies (all but one when the maximum is unbounded, so
        // that X+ is a single loop).
        let unbounded = max.is_none();
        let fixed = if unbounded && min > 0 { min - 1 } else { min };
        for _ in 0..fixed {
            self.emit(node, Cx::NONE)?;
        }
        if unbounded {
            let slot = self.nloops;
            self.nloops += 1;
            if min == 0 {
                // X*: optional entry.
                let s = if greedy {
                    self.push(Inst::Split { alt: 0 })
                } else {
                    self.push(Inst::SplitLazy { to: 0 })
                };
                let body = self.pc();
                self.push(Inst::LoopStart { slot });
                self.emit(node, Cx::NONE)?;
                self.push(Inst::LoopEnd {
                    slot,
                    start: body,
                    greedy,
                });
                let end = self.pc();
                if greedy {
                    self.patch_split(s, end);
                } else {
                    // SplitLazy jumps to the exit and offers the body.
                    self.insts[s] = Inst::SplitLazy { to: end };
                }
            } else {
                let body = self.pc();
                self.push(Inst::LoopStart { slot });
                self.emit(node, Cx::NONE)?;
                self.push(Inst::LoopEnd {
                    slot,
                    start: body,
                    greedy,
                });
            }
            return Ok(());
        }
        let max = max.unwrap();
        let optional = max - min;
        let mut splits = Vec::new();
        for _ in 0..optional {
            let s = if greedy {
                self.push(Inst::Split { alt: 0 })
            } else {
                self.push(Inst::SplitLazy { to: 0 })
            };
            splits.push(s);
            self.emit(node, Cx::NONE)?;
        }
        let end = self.pc();
        for s in splits {
            self.patch_split(s, end);
        }
        Ok(())
    }
}

/// The (min, max) character length of a lookbehind branch (already checked
/// to be bounded).
fn branch_len(n: &Node) -> (u32, u32) {
    fn len(n: &Node) -> (u32, u32) {
        match n {
            Node::Empty | Node::Assert(_) | Node::Keep | Node::Verb { .. } => (0, 0),
            Node::Char { .. }
            | Node::Dot { .. }
            | Node::NotNewline
            | Node::CodeUnit
            | Node::Set(_) => (1, 1),
            Node::Newline { .. } => (1, 2),
            Node::Grapheme => (1, u32::MAX),
            Node::Seq(v) => v.iter().fold((0, 0), |(a, b), c| {
                let (x, y) = len(c);
                (a.saturating_add(x), b.saturating_add(y))
            }),
            Node::Alt(v) => v.iter().fold((u32::MAX, 0), |(a, b), c| {
                let (x, y) = len(c);
                (a.min(x), b.max(y))
            }),
            Node::Group { kind, body, .. } => match kind {
                GroupKind::LookAhead { .. } | GroupKind::LookBehind { .. } => (0, 0),
                _ => len(body),
            },
            Node::Repeat { node, min, max, .. } => {
                let (x, y) = len(node);
                (
                    x.saturating_mul(*min),
                    y.saturating_mul(max.unwrap_or(u32::MAX)),
                )
            }
            // Backrefs and calls were checked; the matcher tries all lengths
            // in the computed range, so a wide range is only slower.
            Node::Backref { .. } | Node::Call { .. } => (0, MAX_VARLOOKBEHIND),
            Node::Cond { yes, no, .. } => {
                let (a, b) = len(yes);
                let (c, d) = no.as_ref().map_or((0, 0), |n| len(n));
                (a.min(c), b.max(d))
            }
            Node::ScanSubstring { .. } => (0, 0),
        }
    }
    let (a, b) = len(n);
    (a, b.min(MAX_LOOKBEHIND))
}

/// Whether a greedy repeat of `item` can never usefully give back characters
/// (pcre2pattern: "PCRE2 has an optimization that automatically
/// possessifies certain simple pattern constructs").
fn can_possessify(item: &Item, next: Option<&Node>, last: bool) -> bool {
    match next {
        None => last,
        // $, \z and \Z can only match at a newline or the end, so an item
        // that cannot match a newline need not give anything back
        // (observed on PHP 8.5: /(\w+)$/ uses no backtracking frame; .*$
        // does).
        Some(Node::Assert(
            Assertion::Dollar { .. } | Assertion::EndSubject | Assertion::EndSubjectOrNewline,
        )) => {
            !matches!(item, Item::Any | Item::AnyNoNl | Item::CodeUnit)
                && [0x0a, 0x0b, 0x0c, 0x0d, 0x85, 0x2028, 0x2029]
                    .iter()
                    .all(|&c| !item_matches_cp(item, c))
        }
        Some(n) => {
            let Some(next_first) = node_first_chars(n) else {
                return false;
            };
            disjoint(item, &next_first)
        }
    }
}

/// The set of characters the node must start with (for single-character
/// nodes and literals only).
fn node_first_chars(n: &Node) -> Option<Vec<FirstChar>> {
    match n {
        Node::Char { c, caseless } => {
            let mut v = Vec::new();
            if *caseless {
                class::case_equivalents(*c, CaseMode::Unicode, &mut v);
            } else {
                v.push(*c);
            }
            Some(v.into_iter().map(FirstChar::Cp).collect())
        }
        Node::Set(s) => Some(vec![FirstChar::Set(s.clone())]),
        Node::Seq(v) => v.first().and_then(node_first_chars),
        Node::Repeat { node, min, .. } if *min > 0 => node_first_chars(node),
        _ => None,
    }
}

enum FirstChar {
    Cp(u32),
    Set(Arc<CharSet>),
}

fn item_matches_cp(item: &Item, c: u32) -> bool {
    match item {
        Item::Byte(b) => c == *b as u32,
        Item::Byte2(a, b) => c == *a as u32 || c == *b as u32,
        Item::Chars(v) => v.contains(&c),
        Item::Set(s) => s.matches(c),
        Item::Any | Item::CodeUnit => true,
        Item::AnyNoNl => true,
    }
}

fn disjoint(item: &Item, next: &[FirstChar]) -> bool {
    for f in next {
        match f {
            FirstChar::Cp(c) => {
                if item_matches_cp(item, *c) {
                    return false;
                }
            }
            FirstChar::Set(s) => {
                // Only decide for sets fully described below 256.
                if !s.high_none {
                    if let Item::Set(is) = item
                        && is.high_none
                    {
                        // item has no high members; compare low parts.
                    } else if let Item::Byte(_) | Item::Byte2(..) = item {
                    } else {
                        return false;
                    }
                }
                for c in 0..256u32 {
                    if s.matches(c) && item_matches_cp(item, c) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Start-of-match information

struct AnchorCtx {
    /// Groups referenced by backreferences (an implicit .* anchor is not
    /// valid inside them).
    backref_groups: Vec<u32>,
    has_prune_skip: bool,
}

impl AnchorCtx {
    fn new(n: &Node) -> AnchorCtx {
        let mut c = AnchorCtx {
            backref_groups: Vec::new(),
            has_prune_skip: false,
        };
        walk(n, &mut |x| match x {
            Node::Backref { target, .. } => c.backref_groups.extend(target.groups.iter().copied()),
            Node::Verb {
                verb: Verb::Prune | Verb::Skip | Verb::SkipName,
                ..
            } => c.has_prune_skip = true,
            _ => {}
        });
        c
    }
}

/// Whether every match must start at the start offset.
fn anchored(n: &Node, in_atomic: bool, no_dotstar: bool, ctx: &AnchorCtx) -> bool {
    match n {
        Node::Assert(Assertion::StartSubject)
        | Node::Assert(Assertion::StartOfMatch)
        | Node::Assert(Assertion::Circumflex { multiline: false }) => true,
        Node::Seq(v) => v
            .first()
            .is_some_and(|f| anchored(f, in_atomic, no_dotstar, ctx)),
        Node::Alt(v) => v.iter().all(|b| anchored(b, in_atomic, no_dotstar, ctx)),
        Node::Group { kind, body, .. } => match kind {
            GroupKind::NonCapture => anchored(body, in_atomic, no_dotstar, ctx),
            GroupKind::Capture(g) => {
                if ctx.backref_groups.contains(g) {
                    // .* inside a referenced group does not anchor.
                    anchored(body, true, no_dotstar, ctx)
                } else {
                    anchored(body, in_atomic, no_dotstar, ctx)
                }
            }
            GroupKind::Atomic => anchored(body, true, no_dotstar, ctx),
            _ => false,
        },
        Node::Repeat {
            node,
            min: 0,
            max: None,
            greed,
        } => {
            !no_dotstar
                && !in_atomic
                && !ctx.has_prune_skip
                && *greed != Greed::Possessive
                && matches!(**node, Node::Dot { dotall: true })
        }
        _ => false,
    }
}

/// Whether every match starts at the subject start or after a newline.
fn startline(n: &Node, no_dotstar: bool, ctx: &AnchorCtx) -> bool {
    match n {
        Node::Assert(Assertion::Circumflex { multiline: true }) => true,
        Node::Seq(v) => v.first().is_some_and(|f| startline(f, no_dotstar, ctx)),
        Node::Alt(v) => v.iter().all(|b| startline(b, no_dotstar, ctx)),
        Node::Group {
            kind: GroupKind::NonCapture,
            body,
            ..
        } => startline(body, no_dotstar, ctx),
        Node::Group {
            kind: GroupKind::Capture(g),
            body,
            ..
        } if !ctx.backref_groups.contains(g) => startline(body, no_dotstar, ctx),
        _ => {
            let _ = no_dotstar;
            false
        }
    }
}

/// First-byte computation result: the set, and whether the node can match
/// the empty string (so following nodes contribute).
struct First {
    bytes: [bool; 256],
    nullable: bool,
    /// Gave up (the set would be "anything").
    any: bool,
}

impl First {
    fn empty_nullable() -> First {
        First {
            bytes: [false; 256],
            nullable: true,
            any: false,
        }
    }
    fn any() -> First {
        First {
            bytes: [true; 256],
            nullable: false,
            any: true,
        }
    }
}

fn add_cp_first(bytes: &mut [bool; 256], c: u32, utf: bool) {
    if utf {
        let mut v = Vec::new();
        utf8::encode(c, &mut v);
        bytes[v[0] as usize] = true;
    } else if c < 256 {
        bytes[c as usize] = true;
    }
}

fn first_of(n: &Node, utf: bool, depth: u32) -> First {
    if depth > 50 {
        return First::any();
    }
    let mut f = First {
        bytes: [false; 256],
        nullable: false,
        any: false,
    };
    match n {
        Node::Empty | Node::Keep => return First::empty_nullable(),
        Node::Assert(_) => return First::empty_nullable(),
        Node::Verb { verb, .. } => {
            return match verb {
                // (*ACCEPT) can end the match anywhere; (*FAIL) matches
                // nothing; other verbs are zero-width.
                Verb::Accept => First::any(),
                Verb::Fail => First {
                    bytes: [false; 256],
                    nullable: false,
                    any: false,
                },
                _ => First::empty_nullable(),
            };
        }
        Node::Char { c, caseless } => {
            if *caseless {
                let mut v = Vec::new();
                class::case_equivalents(*c, CaseMode::Unicode, &mut v);
                for x in v {
                    if !utf && x >= 256 {
                        continue;
                    }
                    add_cp_first(&mut f.bytes, x, utf);
                }
            } else {
                add_cp_first(&mut f.bytes, *c, utf);
            }
        }
        Node::Set(s) => {
            for c in 0..256u32 {
                if s.matches(c) {
                    add_cp_first(&mut f.bytes, c, utf);
                }
            }
            if utf && !s.high_none {
                for b in 0xc4..=0xf4 {
                    f.bytes[b] = true;
                }
            }
        }
        Node::Dot { .. } | Node::NotNewline | Node::CodeUnit | Node::Grapheme => {
            return First::any();
        }
        Node::Newline { .. } => {
            for b in [0x0a, 0x0b, 0x0c, 0x0d] {
                f.bytes[b] = true;
            }
            if utf {
                f.bytes[0xc2] = true;
                f.bytes[0xe2] = true;
            } else {
                f.bytes[0x85] = true;
            }
        }
        Node::Seq(v) => {
            let mut acc = First::empty_nullable();
            for c in v {
                let cf = first_of(c, utf, depth + 1);
                if cf.any {
                    return First::any();
                }
                for i in 0..256 {
                    acc.bytes[i] |= cf.bytes[i];
                }
                if !cf.nullable {
                    acc.nullable = false;
                    return acc;
                }
            }
            return acc;
        }
        Node::Alt(v) => {
            let mut acc = First {
                bytes: [false; 256],
                nullable: false,
                any: false,
            };
            for c in v {
                let cf = first_of(c, utf, depth + 1);
                if cf.any {
                    return First::any();
                }
                for i in 0..256 {
                    acc.bytes[i] |= cf.bytes[i];
                }
                acc.nullable |= cf.nullable;
            }
            return acc;
        }
        Node::Group { kind, body, .. } => match kind {
            GroupKind::LookAhead { negated: false, .. } => {
                // A positive lookahead constrains the first character but we
                // keep it simple: treat as zero-width.
                return First::empty_nullable();
            }
            GroupKind::LookAhead { .. } | GroupKind::LookBehind { .. } => {
                return First::empty_nullable();
            }
            _ => return first_of(body, utf, depth + 1),
        },
        Node::Repeat { node, min, .. } => {
            let mut cf = first_of(node, utf, depth + 1);
            if *min == 0 {
                cf.nullable = true;
            }
            return cf;
        }
        Node::Backref { .. } | Node::Call { .. } | Node::ScanSubstring { .. } => {
            return First::any();
        }
        Node::Cond { .. } => return First::any(),
    }
    f
}

/// Minimum match length in characters (0 if unknown).
fn min_len(n: &Node, depth: u32) -> u32 {
    if depth > 50 {
        return 0;
    }
    match n {
        Node::Char { .. } | Node::Dot { .. } | Node::NotNewline | Node::CodeUnit | Node::Set(_) => {
            1
        }
        Node::Newline { .. } | Node::Grapheme => 1,
        Node::Seq(v) => v
            .iter()
            .map(|c| min_len(c, depth + 1))
            .fold(0u32, u32::saturating_add),
        Node::Alt(v) => v.iter().map(|c| min_len(c, depth + 1)).min().unwrap_or(0),
        Node::Group { kind, body, .. } => match kind {
            GroupKind::LookAhead { .. } | GroupKind::LookBehind { .. } => 0,
            _ => min_len(body, depth + 1),
        },
        Node::Repeat { node, min, .. } => min_len(node, depth + 1).saturating_mul(*min),
        Node::Cond { yes, no, .. } => {
            min_len(yes, depth + 1).min(no.as_ref().map_or(0, |n| min_len(n, depth + 1)))
        }
        _ => 0,
    }
}

/// Literal prefix every match starts with.
fn literal_prefix(n: &Node, utf: bool, out: &mut Vec<u8>) -> bool {
    // Returns true if the whole node was consumed as literal (so the caller
    // may continue with the next sibling).
    match n {
        Node::Char { c, caseless: false } => {
            if utf {
                utf8::encode(*c, out);
            } else {
                out.push(*c as u8);
            }
            true
        }
        Node::Seq(v) => {
            for c in v {
                if !literal_prefix(c, utf, out) {
                    return false;
                }
            }
            true
        }
        Node::Group {
            kind: GroupKind::NonCapture | GroupKind::Atomic | GroupKind::Capture(_),
            body,
            ..
        } => literal_prefix(body, utf, out),
        Node::Repeat { node, min, max, .. } if *min > 0 => {
            let mut tmp = Vec::new();
            if literal_prefix(node, utf, &mut tmp) {
                for _ in 0..(*min).min(8) {
                    out.extend_from_slice(&tmp);
                }
                *max == Some(*min) && *min <= 8
            } else {
                out.extend_from_slice(&tmp);
                false
            }
        }
        _ => false,
    }
}

/// A byte that must occur in every match (the last literal of a sequence of
/// mandatory items), PCRE2's "required code unit".
fn required_byte(n: &Node, utf: bool) -> Option<(u8, u8)> {
    fn last_req(n: &Node, utf: bool) -> Option<(u8, u8)> {
        match n {
            Node::Char { c, caseless } => {
                if *c < 0x80 {
                    let b = *c as u8;
                    if *caseless {
                        if utf && (b.eq_ignore_ascii_case(&b'k') || b.eq_ignore_ascii_case(&b's')) {
                            return None;
                        }
                        Some((b.to_ascii_lowercase(), b.to_ascii_uppercase()))
                    } else {
                        Some((b, b))
                    }
                } else if !utf && !*caseless {
                    Some((*c as u8, *c as u8))
                } else if utf && !*caseless {
                    let mut v = Vec::new();
                    utf8::encode(*c, &mut v);
                    let l = *v.last().unwrap();
                    Some((l, l))
                } else {
                    None
                }
            }
            Node::Seq(v) => {
                for c in v.iter().rev() {
                    if let Some(r) = last_req(c, utf) {
                        return Some(r);
                    }
                    if !zero_width_or_optional_safe(c) {
                        return None;
                    }
                }
                None
            }
            Node::Group {
                kind: GroupKind::NonCapture | GroupKind::Capture(_) | GroupKind::Atomic,
                body,
                ..
            } => last_req(body, utf),
            Node::Repeat { node, min, .. } if *min > 0 => last_req(node, utf),
            Node::Alt(v) => {
                let mut r: Option<(u8, u8)> = None;
                for b in v {
                    let x = last_req(b, utf)?;
                    match r {
                        None => r = Some(x),
                        Some(y) if y == x => {}
                        _ => return None,
                    }
                }
                r
            }
            _ => None,
        }
    }
    // Items that may sit after the required literal without breaking the
    // guarantee: anything that cannot end the match early.
    fn zero_width_or_optional_safe(n: &Node) -> bool {
        match n {
            Node::Verb {
                verb: Verb::Accept, ..
            } => false,
            Node::Seq(v) | Node::Alt(v) => v.iter().all(zero_width_or_optional_safe),
            Node::Group { body, .. } => zero_width_or_optional_safe(body),
            Node::Repeat { node, .. } => zero_width_or_optional_safe(node),
            Node::Cond { yes, no, .. } => {
                zero_width_or_optional_safe(yes)
                    && no.as_ref().is_none_or(|n| zero_width_or_optional_safe(n))
            }
            Node::Call { .. } => false,
            _ => true,
        }
    }
    let mut has_accept = false;
    walk(n, &mut |x| {
        if let Node::Verb {
            verb: Verb::Accept, ..
        } = x
        {
            has_accept = true;
        }
    });
    if has_accept {
        return None;
    }
    last_req(n, utf)
}

/// The code unit every match must start with, looking through verbs, \K,
/// assertions and groups (PCRE2's "first code unit"; observed on PHP 8.5:
/// /(*COMMIT)\K\BA/i skips to the first "a", and /(\b(*COMMIT)2)\1?/ is
/// tried at every position because of the backreference).
fn first_unit(n: &Node, utf: bool) -> Option<(u8, u8)> {
    fn go(n: &Node, utf: bool, br: &[u32]) -> Result<Option<(u8, u8)>, ()> {
        // Ok(None): zero-width, look further; Ok(Some): found; Err: none.
        match n {
            Node::Empty | Node::Keep | Node::Assert(_) => Ok(None),
            Node::Group {
                kind: GroupKind::LookAhead { .. } | GroupKind::LookBehind { .. },
                ..
            } => Ok(None),
            // A group that a backreference refers to gives no first code
            // unit (observed on PHP 8.5: /(\b(*COMMIT)2)\1?/ is tried at
            // every position).
            Node::Group {
                kind: GroupKind::Capture(g),
                ..
            } if br.contains(g) => Err(()),
            Node::Verb { verb, .. } => match verb {
                Verb::Fail | Verb::Accept => Err(()),
                _ => Ok(None),
            },
            // A one-byte set acts as that byte (observed on PHP 8.5:
            // /(*COMMIT)[2].*/ skips to the first "2").
            Node::Set(s) => match s.single_byte() {
                Some(b) if b < 0x80 || !utf => Ok(Some((b, b))),
                _ => Err(()),
            },
            // Caseless characters beyond ASCII give none (observed on PHP
            // 8.5: /(*COMMIT)é/iu is tried at every position).
            Node::Char { c, caseless: true } if *c >= 0x80 => Err(()),
            // A conditional whose branches start alike (or are empty).
            Node::Cond { yes, no, .. } => {
                let a = go(yes, utf, br)?;
                let b = match no {
                    Some(n) => go(n, utf, br)?,
                    None => None,
                };
                if a == b { Ok(a) } else { Err(()) }
            }
            Node::Char { c, caseless } => {
                let mut v = Vec::new();
                if *caseless {
                    class::case_equivalents(*c, CaseMode::Unicode, &mut v);
                } else {
                    v.push(*c);
                }
                let mut units: Vec<u8> = v
                    .iter()
                    .filter(|&&x| utf || x < 256)
                    .map(|&x| {
                        let mut e = Vec::new();
                        if utf {
                            utf8::encode(x, &mut e);
                        } else {
                            e.push(x as u8);
                        }
                        e[0]
                    })
                    .collect();
                units.sort_unstable();
                units.dedup();
                match units.len() {
                    1 => Ok(Some((units[0], units[0]))),
                    2 if units[0].is_ascii_uppercase() && units[1] == units[0] + 32 => {
                        Ok(Some((units[0], units[1])))
                    }
                    _ => Err(()),
                }
            }
            Node::Seq(v) => {
                for c in v {
                    if let Some(u) = go(c, utf, br)? {
                        return Ok(Some(u));
                    }
                }
                Ok(None)
            }
            Node::Alt(v) => {
                let mut r = None;
                for b in v {
                    let u = go(b, utf, br)?.ok_or(())?;
                    match r {
                        None => r = Some(u),
                        Some(x) if x == u => {}
                        _ => return Err(()),
                    }
                }
                Ok(r)
            }
            Node::Group {
                kind: GroupKind::NonCapture | GroupKind::Capture(_) | GroupKind::Atomic,
                body,
                ..
            } => go(body, utf, br),
            Node::Repeat { node, min, .. } if *min > 0 => go(node, utf, br),
            _ => Err(()),
        }
    }
    let ctx = AnchorCtx::new(n);
    go(n, utf, &ctx.backref_groups).ok().flatten()
}

fn start_info(n: &Node, parsed: &Parsed, flags: crate::Flags) -> StartInfo {
    let ctx = AnchorCtx::new(n);
    let no_dotstar = parsed.no_dotstar_anchor;
    let mut s = StartInfo {
        anchored: flags.contains(crate::Flags::ANCHORED) || anchored(n, false, no_dotstar, &ctx),
        startline: false,
        ..StartInfo::default()
    };
    if !s.anchored {
        s.startline = startline(n, no_dotstar, &ctx);
    }
    let mut has_accept = false;
    walk(n, &mut |x| {
        if let Node::Verb {
            verb: Verb::Accept, ..
        } = x
        {
            has_accept = true;
        }
    });
    // (*ACCEPT) can end a match early.
    s.min_len = if has_accept {
        0
    } else {
        min_len(n, 0) as usize
    };
    let mut prefix = Vec::new();
    literal_prefix(n, parsed.utf, &mut prefix);
    if !prefix.is_empty() {
        s.prefix = prefix;
    }
    let f = first_of(n, parsed.utf, 0);
    if !f.any && !f.nullable && s.prefix.is_empty() {
        s.first_bytes = Some(Box::new(f.bytes));
    }
    // With (*COMMIT) or (*SKIP) the positions skipped by a start-up
    // optimisation are observable. PCRE2 still skips to a required first
    // code unit (pcre2pattern: "(*COMMIT)abc" matches "xyzabc") but builds
    // no start bitmap (observed on PHP 8.5: /(*COMMIT)a{0,2}2|c/ does not
    // match "A-1AbAb2Bca"), and only the first code unit counts, not a
    // longer literal prefix.
    let mut commit_or_skip = false;
    walk(n, &mut |x| {
        if let Node::Verb {
            verb: Verb::Commit | Verb::Skip | Verb::SkipName,
            ..
        } = x
        {
            commit_or_skip = true;
        }
    });
    if commit_or_skip {
        s.prefix.clear();
        s.first_bytes = None;
        if let Some((a, b)) = first_unit(n, parsed.utf) {
            let mut fb = [false; 256];
            fb[a as usize] = true;
            fb[b as usize] = true;
            s.first_bytes = Some(Box::new(fb));
        }
    }
    s.required = required_byte(n, parsed.utf);
    // The required byte is only useful when it is not just the first byte.
    if let Some((a, _)) = s.required
        && s.prefix.len() == 1
        && s.prefix[0] == a
    {
        s.required = None;
    }
    if !commit_or_skip {
        if s.prefix.len() < 2 {
            let mut sets = Vec::new();
            fixed_sets(n, parsed.utf, &mut sets);
            if sets.len() >= 2 {
                let (idx, cnt) = sets
                    .iter()
                    .enumerate()
                    .map(|(i, set)| (i, set.iter().map(|w| w.count_ones()).sum::<u32>()))
                    .min_by_key(|&(_, c)| c)
                    .unwrap();
                if cnt <= 3 {
                    let bytes: Vec<u8> = (0..256u32)
                        .filter(|&b| sets[idx][(b >> 6) as usize] >> (b & 63) & 1 != 0)
                        .map(|b| b as u8)
                        .collect();
                    s.anchor = Some((idx, bytes));
                }
                s.sets = sets;
            }
        }
        if let Some(Node::Assert(Assertion::WordBoundary { negated, ucp })) = first_item(n) {
            s.lead_wb = Some((*negated, *ucp));
        }
    }
    if let Some(fb) = &s.first_bytes {
        let set: Vec<u8> = (0..256).filter(|&b| fb[b]).map(|b| b as u8).collect();
        s.first_scan = match set.as_slice() {
            [a] => FirstScan::One(*a),
            [a, b] => FirstScan::Two(*a, *b),
            [a, b, c] => FirstScan::Three(*a, *b, *c),
            _ if set.iter().all(|&b| b >= 0x80) => FirstScan::HighBit,
            _ => FirstScan::Table,
        };
    }
    s
}

/// The first node of the pattern, looking into sequences.
fn first_item(n: &Node) -> Option<&Node> {
    match n {
        Node::Seq(v) => v.first().and_then(first_item),
        other => Some(other),
    }
}

/// Byte sets that every match has at offsets 0, 1, 2, ... (stops at the
/// first item whose width in bytes is not fixed).
fn fixed_sets(n: &Node, utf: bool, out: &mut Vec<[u64; 4]>) -> bool {
    const MAX: usize = 16;
    fn single(b: u8) -> [u64; 4] {
        let mut s = [0u64; 4];
        s[(b >> 6) as usize] |= 1 << (b & 63);
        s
    }
    if out.len() >= MAX {
        return false;
    }
    match n {
        Node::Empty | Node::Keep | Node::Assert(_) => true,
        Node::Char { c, caseless } => {
            if !*caseless {
                if utf {
                    let mut v = Vec::new();
                    utf8::encode(*c, &mut v);
                    for b in v {
                        out.push(single(b));
                    }
                } else {
                    out.push(single(*c as u8));
                }
                return true;
            }
            let mut v = Vec::new();
            class::case_equivalents(*c, CaseMode::Unicode, &mut v);
            if !utf {
                v.retain(|&x| x < 256);
            }
            let mut set = [0u64; 4];
            let mut all_one_byte = true;
            for x in v {
                let mut e = Vec::new();
                if utf {
                    utf8::encode(x, &mut e);
                } else {
                    e.push(x as u8);
                }
                all_one_byte &= e.len() == 1;
                set[(e[0] >> 6) as usize] |= 1 << (e[0] & 63);
            }
            out.push(set);
            all_one_byte
        }
        Node::Set(s) => {
            let one_byte = !utf || (s.ascii_only());
            let mut set = [0u64; 4];
            for b in 0..256u32 {
                if one_byte {
                    if s.has_byte(b as u8) {
                        set[(b >> 6) as usize] |= 1 << (b & 63);
                    }
                } else if s.matches(b) {
                    let mut e = Vec::new();
                    utf8::encode(b, &mut e);
                    set[(e[0] >> 6) as usize] |= 1 << (e[0] & 63);
                }
            }
            if !one_byte {
                if !s.high_none {
                    for b in 0xc4..=0xf4u32 {
                        set[(b >> 6) as usize] |= 1 << (b & 63);
                    }
                }
                out.push(set);
                return false;
            }
            out.push(set);
            true
        }
        Node::Seq(v) => {
            for c in v {
                if !fixed_sets(c, utf, out) {
                    return false;
                }
            }
            true
        }
        Node::Group {
            kind: GroupKind::NonCapture | GroupKind::Capture(_) | GroupKind::Atomic,
            body,
            ..
        } => fixed_sets(body, utf, out),
        Node::Repeat { node, min, max, .. } => {
            for _ in 0..*min {
                if !fixed_sets(node, utf, out) {
                    return false;
                }
            }
            *max == Some(*min)
        }
        Node::Alt(v) => {
            // Union per offset, up to the shortest branch's fixed part.
            let mut per: Vec<Vec<[u64; 4]>> = Vec::new();
            for b in v {
                let mut o = Vec::new();
                fixed_sets(b, utf, &mut o);
                if o.is_empty() {
                    return false;
                }
                per.push(o);
            }
            let n = per.iter().map(|o| o.len()).min().unwrap_or(0);
            for i in 0..n {
                let mut set = [0u64; 4];
                for o in &per {
                    for k in 0..4 {
                        set[k] |= o[i][k];
                    }
                }
                if out.len() >= MAX {
                    break;
                }
                out.push(set);
            }
            false
        }
        _ => false,
    }
}
