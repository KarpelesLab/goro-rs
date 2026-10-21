//! Regular expressions and automata (libxml2 xmlregexp.c): the XML Schema
//! regular expression engine used for pattern facets, and the automata
//! (with counters) used by the XSD and RelaxNG validators to check element
//! content models with the progressive "push string" interface.
//!
//! This is a close port of libxml2 2.13 so that execution and error
//! reporting (the "Expected is ( ... )" lists) behave the same.

use crate::xmlunicode;

// Atom types (order is significant).
pub const EPSILON: i32 = 1;
pub const CHARVAL: i32 = 2;
pub const RANGES: i32 = 3;
pub const SUBREG: i32 = 4;
pub const STRING: i32 = 5;
pub const ANYCHAR: i32 = 6;
pub const ANYSPACE: i32 = 7;
pub const NOTSPACE: i32 = 8;
pub const INITNAME: i32 = 9;
pub const NOTINITNAME: i32 = 10;
pub const NAMECHAR: i32 = 11;
pub const NOTNAMECHAR: i32 = 12;
pub const DECIMAL: i32 = 13;
pub const NOTDECIMAL: i32 = 14;
pub const REALCHAR: i32 = 15;
pub const NOTREALCHAR: i32 = 16;
pub const LETTER: i32 = 100;
pub const LETTER_UPPERCASE: i32 = 101;
pub const LETTER_LOWERCASE: i32 = 102;
pub const LETTER_TITLECASE: i32 = 103;
pub const LETTER_MODIFIER: i32 = 104;
pub const LETTER_OTHERS: i32 = 105;
pub const MARK: i32 = 106;
pub const MARK_NONSPACING: i32 = 107;
pub const MARK_SPACECOMBINING: i32 = 108;
pub const MARK_ENCLOSING: i32 = 109;
pub const NUMBER: i32 = 110;
pub const NUMBER_DECIMAL: i32 = 111;
pub const NUMBER_LETTER: i32 = 112;
pub const NUMBER_OTHERS: i32 = 113;
pub const PUNCT: i32 = 114;
pub const PUNCT_CONNECTOR: i32 = 115;
pub const PUNCT_DASH: i32 = 116;
pub const PUNCT_OPEN: i32 = 117;
pub const PUNCT_CLOSE: i32 = 118;
pub const PUNCT_INITQUOTE: i32 = 119;
pub const PUNCT_FINQUOTE: i32 = 120;
pub const PUNCT_OTHERS: i32 = 121;
pub const SEPAR: i32 = 122;
pub const SEPAR_SPACE: i32 = 123;
pub const SEPAR_LINE: i32 = 124;
pub const SEPAR_PARA: i32 = 125;
pub const SYMBOL: i32 = 126;
pub const SYMBOL_MATH: i32 = 127;
pub const SYMBOL_CURRENCY: i32 = 128;
pub const SYMBOL_MODIFIER: i32 = 129;
pub const SYMBOL_OTHERS: i32 = 130;
pub const OTHER: i32 = 131;
pub const OTHER_CONTROL: i32 = 132;
pub const OTHER_FORMAT: i32 = 133;
pub const OTHER_PRIVATE: i32 = 134;
pub const OTHER_NA: i32 = 135;
pub const BLOCK_NAME: i32 = 136;

// Quantifiers
const QUANT_EPSILON: i32 = 1;
const QUANT_ONCE: i32 = 2;
const QUANT_OPT: i32 = 3;
const QUANT_MULT: i32 = 4;
const QUANT_PLUS: i32 = 5;
const QUANT_ONCEONLY: i32 = 6;
#[allow(dead_code)]
const QUANT_ALL: i32 = 7;
const QUANT_RANGE: i32 = 8;

// State types
const START_STATE: i32 = 1;
const FINAL_STATE: i32 = 2;
const TRANS_STATE: i32 = 3;
const SINK_STATE: i32 = 4;
const UNREACH_STATE: i32 = 5;

const MARK_NORMAL: i32 = 0;
const MARK_START: i32 = 1;
const MARK_VISITED: i32 = 2;

pub const AM_AUTOMATA_RNG: i32 = 1;

const REGEXP_ALL_COUNTER: i32 = 0x123456;
const REGEXP_ALL_LAX_COUNTER: i32 = 0x123457;

// Execution status
pub const REGEXP_OK: i32 = 0;
pub const REGEXP_NOT_FOUND: i32 = -1;
pub const REGEXP_INTERNAL_ERROR: i32 = -2;
#[allow(dead_code)]
pub const REGEXP_OUT_OF_MEMORY: i32 = -3;

const SEP: u8 = b'|';

pub type StateId = usize;
pub type AtomId = usize;

#[derive(Clone, Debug)]
struct Range {
    neg: i32,
    typ: i32,
    start: i32,
    end: i32,
    block: Option<String>,
}

#[derive(Clone, Debug)]
struct Atom {
    no: i32,
    typ: i32,
    quant: i32,
    min: i32,
    max: i32,
    valuep: Option<String>,
    valuep2: Option<String>,
    neg: bool,
    codepoint: i32,
    start: Option<StateId>,
    start0: Option<StateId>,
    stop: Option<StateId>,
    ranges: Vec<Range>,
    data: Option<usize>,
}

impl Atom {
    fn new(typ: i32) -> Atom {
        Atom {
            no: 0,
            typ,
            quant: QUANT_ONCE,
            min: 0,
            max: 0,
            valuep: None,
            valuep2: None,
            neg: false,
            codepoint: 0,
            start: None,
            start0: None,
            stop: None,
            ranges: Vec::new(),
            data: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Counter {
    min: i32,
    max: i32,
}

#[derive(Clone, Copy, Debug)]
struct Trans {
    atom: Option<AtomId>,
    to: i32,
    counter: i32,
    count: i32,
    nd: i32,
}

#[derive(Clone, Debug)]
struct State {
    typ: i32,
    mark: i32,
    markd: i32,
    reached: i32,
    no: usize,
    trans: Vec<Trans>,
    trans_to: Vec<usize>,
}

/// An automaton under construction (xmlAutomata / xmlRegParserCtxt).
pub struct Automata {
    string: Vec<u8>,
    cur: usize,
    pub error: i32,
    neg: i32,
    start: Option<StateId>,
    end: Option<StateId>,
    state: Option<StateId>,
    atom: Option<AtomId>,
    arena: Vec<Atom>,
    atoms: Vec<AtomId>,
    states: Vec<Option<State>>,
    counters: Vec<Counter>,
    determinist: i32,
    negs: i32,
    flags: i32,
    depth: i32,
    /// "failed to compile: ..." messages raised while parsing a regexp.
    pub errors: Vec<String>,
}

/// A compiled regexp / automaton (xmlRegexp).
pub struct Regexp {
    pub string: String,
    states: Vec<Option<State>>,
    arena: Vec<Atom>,
    nb_atoms: usize,
    counters: Vec<Counter>,
    determinist: i32,
    flags: i32,
    compact: Option<Vec<i32>>,
    transdata: Option<Vec<Option<usize>>>,
    nbstates: i32,
    string_map: Vec<String>,
}

fn str_equal_wildcard(exp: Option<&str>, val: Option<&str>) -> bool {
    let (exp, val) = match (exp, val) {
        (Some(e), Some(v)) => (e.as_bytes(), v.as_bytes()),
        (None, None) => return true,
        _ => return false,
    };
    // NUL-terminated semantics
    let at = |s: &[u8], i: usize| -> u8 { if i < s.len() { s[i] } else { 0 } };
    let (mut e, mut v) = (exp, val);
    let (mut ei, mut vi) = (0usize, 0usize);
    loop {
        if at(e, ei) != at(v, vi) {
            if at(v, vi) == b'*' {
                std::mem::swap(&mut e, &mut v);
                std::mem::swap(&mut ei, &mut vi);
            }
            if at(v, vi) != 0 && at(e, ei) != 0 && {
                let c = at(e, ei);
                ei += 1;
                c == b'*'
            } {
                loop {
                    if at(v, vi) == SEP {
                        break;
                    }
                    vi += 1;
                    if at(v, vi) == 0 {
                        break;
                    }
                }
                if at(v, vi) == 0 {
                    break;
                }
                continue;
            } else {
                return false;
            }
        }
        ei += 1;
        vi += 1;
        if at(v, vi) == 0 {
            break;
        }
    }
    at(e, ei) == 0
}

fn is_xml_char(c: i32) -> bool {
    crate::parser::is_char(c as u32)
}

fn check_character_range(
    typ: i32,
    codepoint: i32,
    neg: bool,
    start: i32,
    end: i32,
    block: Option<&str>,
) -> i32 {
    let c = codepoint as u32;
    let mut neg = neg;
    let cat = |n: &str| xmlunicode::is_cat(c, n);
    let ret: bool = match typ {
        STRING | SUBREG | RANGES | EPSILON => return -1,
        ANYCHAR => codepoint != '\n' as i32 && codepoint != '\r' as i32,
        CHARVAL => codepoint >= start && codepoint <= end,
        NOTSPACE | ANYSPACE => {
            if typ == NOTSPACE {
                neg = !neg;
            }
            matches!(codepoint, 0x0A | 0x0D | 0x09 | 0x20)
        }
        NOTINITNAME | INITNAME => {
            if typ == NOTINITNAME {
                neg = !neg;
            }
            xmlunicode::is_letter(c) || codepoint == '_' as i32 || codepoint == ':' as i32
        }
        NOTNAMECHAR | NAMECHAR => {
            if typ == NOTNAMECHAR {
                neg = !neg;
            }
            xmlunicode::is_letter(c)
                || xmlunicode::is_digit(c)
                || matches!(codepoint, 0x2E | 0x2D | 0x5F | 0x3A)
                || xmlunicode::is_combining(c)
                || xmlunicode::is_extender(c)
        }
        NOTDECIMAL | DECIMAL => {
            if typ == NOTDECIMAL {
                neg = !neg;
            }
            cat("Nd")
        }
        REALCHAR | NOTREALCHAR => {
            if typ == REALCHAR {
                neg = !neg;
            }
            cat("P") || cat("Z") || cat("C")
        }
        LETTER => cat("L"),
        LETTER_UPPERCASE => cat("Lu"),
        LETTER_LOWERCASE => cat("Ll"),
        LETTER_TITLECASE => cat("Lt"),
        LETTER_MODIFIER => cat("Lm"),
        LETTER_OTHERS => cat("Lo"),
        MARK => cat("M"),
        MARK_NONSPACING => cat("Mn"),
        MARK_SPACECOMBINING => cat("Mc"),
        MARK_ENCLOSING => cat("Me"),
        NUMBER => cat("N"),
        NUMBER_DECIMAL => cat("Nd"),
        NUMBER_LETTER => cat("Nl"),
        NUMBER_OTHERS => cat("No"),
        PUNCT => cat("P"),
        PUNCT_CONNECTOR => cat("Pc"),
        PUNCT_DASH => cat("Pd"),
        PUNCT_OPEN => cat("Ps"),
        PUNCT_CLOSE => cat("Pe"),
        PUNCT_INITQUOTE => cat("Pi"),
        PUNCT_FINQUOTE => cat("Pf"),
        PUNCT_OTHERS => cat("Po"),
        SEPAR => cat("Z"),
        SEPAR_SPACE => cat("Zs"),
        SEPAR_LINE => cat("Zl"),
        SEPAR_PARA => cat("Zp"),
        SYMBOL => cat("S"),
        SYMBOL_MATH => cat("Sm"),
        SYMBOL_CURRENCY => cat("Sc"),
        SYMBOL_MODIFIER => cat("Sk"),
        SYMBOL_OTHERS => cat("So"),
        OTHER => cat("C"),
        OTHER_CONTROL => cat("Cc"),
        OTHER_FORMAT => cat("Cf"),
        OTHER_PRIVATE => cat("Co"),
        OTHER_NA => false,
        BLOCK_NAME => {
            match xmlunicode::is_block(c, block.unwrap_or("")) {
                Some(b) => b,
                // xmlUCSIsBlock returns -1: treated as true (non-zero)
                None => {
                    return if neg { 0 } else { -1 };
                }
            }
        }
        _ => false,
    };
    let r = if ret { 1 } else { 0 };
    if neg {
        return if r == 0 { 1 } else { 0 };
    }
    r
}

fn check_character(atom: &Atom, codepoint: i32) -> i32 {
    if !is_xml_char(codepoint) {
        return -1;
    }
    match atom.typ {
        SUBREG | EPSILON => -1,
        CHARVAL => (codepoint == atom.codepoint) as i32,
        RANGES => {
            let mut accept = 0;
            for r in &atom.ranges {
                let ret = check_character_range(
                    r.typ,
                    codepoint,
                    false,
                    r.start,
                    r.end,
                    r.block.as_deref(),
                );
                if r.neg == 2 {
                    if ret != 0 {
                        return 0;
                    }
                } else if r.neg != 0 {
                    if ret == 0 {
                        accept = 1;
                    } else {
                        return 0;
                    }
                } else if ret != 0 {
                    accept = 1;
                }
            }
            accept
        }
        STRING => -1,
        _ => {
            let mut ret =
                check_character_range(atom.typ, codepoint, false, 0, 0, atom.valuep.as_deref());
            if atom.neg {
                ret = if ret == 0 { 1 } else { 0 };
            }
            ret
        }
    }
}

impl Default for Automata {
    fn default() -> Self {
        Self::new()
    }
}

impl Automata {
    fn new_ctxt(string: Option<&str>) -> Automata {
        let s = string.map(|s| s.as_bytes().to_vec()).unwrap_or_default();
        Automata {
            string: s,
            cur: 0,
            error: 0,
            neg: 0,
            start: None,
            end: None,
            state: None,
            atom: None,
            arena: Vec::new(),
            atoms: Vec::new(),
            states: Vec::new(),
            counters: Vec::new(),
            determinist: -1,
            negs: 0,
            flags: 0,
            depth: 0,
            errors: Vec::new(),
        }
    }

    /// xmlNewAutomata
    pub fn new() -> Automata {
        let mut a = Automata::new_ctxt(None);
        let s = a.state_push();
        a.state = Some(s);
        a.start = Some(s);
        a.st(s).typ = START_STATE;
        a
    }

    fn st(&mut self, id: StateId) -> &mut State {
        self.states[id].as_mut().expect("state")
    }

    // ---- parser helpers
    fn cur(&self) -> u8 {
        self.at(0)
    }
    fn at(&self, i: isize) -> u8 {
        let p = self.cur as isize + i;
        if p < 0 || p as usize >= self.string.len() {
            0
        } else {
            self.string[p as usize]
        }
    }
    fn next(&mut self) {
        self.cur += 1;
    }
    fn utf8_at_cur(&self) -> (i32, usize) {
        let rest = &self.string[self.cur.min(self.string.len())..];
        if rest.is_empty() {
            return (0, 1);
        }
        let l = match rest[0] {
            0x00..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => return (-1, 1),
        };
        if rest.len() < l {
            return (-1, 1);
        }
        match std::str::from_utf8(&rest[..l]) {
            Ok(s) => (s.chars().next().unwrap() as i32, l),
            Err(_) => (-1, 1),
        }
    }

    fn error_msg(&mut self, msg: &str) {
        self.error = 1450;
        self.errors.push(format!("failed to compile: {}", msg));
    }

    fn new_atom(&mut self, typ: i32) -> AtomId {
        self.arena.push(Atom::new(typ));
        self.arena.len() - 1
    }

    fn copy_atom(&mut self, a: AtomId) -> AtomId {
        let src = &self.arena[a];
        let mut n = Atom::new(src.typ);
        n.quant = src.quant;
        n.min = src.min;
        n.max = src.max;
        n.ranges = src.ranges.clone();
        self.arena.push(n);
        self.arena.len() - 1
    }

    fn atom_add_range(
        &mut self,
        atom: Option<AtomId>,
        neg: i32,
        typ: i32,
        start: i32,
        end: i32,
        block: Option<String>,
    ) {
        let a = match atom {
            None => {
                self.error_msg("add range: atom is NULL");
                return;
            }
            Some(a) => a,
        };
        if self.arena[a].typ != RANGES {
            self.error_msg("add range: atom is not ranges");
            return;
        }
        self.arena[a].ranges.push(Range {
            neg,
            typ,
            start,
            end,
            block,
        });
    }

    fn get_counter(&mut self) -> usize {
        self.counters.push(Counter { min: -1, max: -1 });
        self.counters.len() - 1
    }

    fn atom_push(&mut self, atom: AtomId) {
        self.arena[atom].no = self.atoms.len() as i32;
        self.atoms.push(atom);
    }

    fn state_add_trans(
        &mut self,
        state: StateId,
        atom: Option<AtomId>,
        target: StateId,
        counter: i32,
        count: i32,
    ) {
        let tno = target as i32;
        {
            let s = self.st(state);
            for t in s.trans.iter().rev() {
                if t.atom == atom && t.to == tno && t.counter == counter && t.count == count {
                    return;
                }
            }
            s.trans.push(Trans {
                atom,
                to: tno,
                counter,
                count,
                nd: 0,
            });
        }
        let sno = self.states[state].as_ref().unwrap().no;
        self.st(target).trans_to.push(sno);
    }

    fn state_push(&mut self) -> StateId {
        let no = self.states.len();
        self.states.push(Some(State {
            typ: TRANS_STATE,
            mark: MARK_NORMAL,
            markd: MARK_NORMAL,
            reached: MARK_NORMAL,
            no,
            trans: Vec::new(),
            trans_to: Vec::new(),
        }));
        no
    }

    fn gen_all_transition(&mut self, from: StateId, to: Option<StateId>, lax: bool) {
        let to = match to {
            Some(t) => t,
            None => {
                let t = self.state_push();
                self.state = Some(t);
                t
            }
        };
        self.state_add_trans(
            from,
            None,
            to,
            -1,
            if lax {
                REGEXP_ALL_LAX_COUNTER
            } else {
                REGEXP_ALL_COUNTER
            },
        );
    }

    fn gen_epsilon(&mut self, from: StateId, to: Option<StateId>) {
        let to = match to {
            Some(t) => t,
            None => {
                let t = self.state_push();
                self.state = Some(t);
                t
            }
        };
        self.state_add_trans(from, None, to, -1, -1);
    }

    fn gen_counted_epsilon(&mut self, from: StateId, to: Option<StateId>, counter: i32) {
        let to = match to {
            Some(t) => t,
            None => {
                let t = self.state_push();
                self.state = Some(t);
                t
            }
        };
        self.state_add_trans(from, None, to, counter, -1);
    }

    fn gen_counted(&mut self, from: StateId, to: Option<StateId>, counter: i32) {
        let to = match to {
            Some(t) => t,
            None => {
                let t = self.state_push();
                self.state = Some(t);
                t
            }
        };
        self.state_add_trans(from, None, to, -1, counter);
    }

    /// xmlFAGenerateTransitions
    fn gen_transitions(&mut self, from: StateId, to: Option<StateId>, atom: AtomId) -> i32 {
        if self.arena[atom].typ == SUBREG {
            let (start, start0, stop, quant) = {
                let a = &self.arena[atom];
                (a.start.unwrap(), a.start0, a.stop.unwrap(), a.quant)
            };
            if let Some(t) = to {
                if stop != t && quant != QUANT_RANGE {
                    self.gen_epsilon(stop, Some(t));
                }
            }
            match quant {
                QUANT_OPT => {
                    self.arena[atom].quant = QUANT_ONCE;
                    if to.is_none() {
                        self.gen_epsilon(start, None);
                        let s = self.state;
                        self.gen_epsilon(stop, s);
                    } else {
                        self.gen_epsilon(start, to);
                    }
                }
                QUANT_MULT => {
                    self.arena[atom].quant = QUANT_ONCE;
                    self.gen_epsilon(start, Some(stop));
                    self.gen_epsilon(stop, Some(start));
                }
                QUANT_PLUS => {
                    self.arena[atom].quant = QUANT_ONCE;
                    self.gen_epsilon(stop, Some(start));
                }
                QUANT_RANGE => {
                    let newstate = match to {
                        Some(t) => t,
                        None => self.state_push(),
                    };
                    let (amin, amax) = (self.arena[atom].min, self.arena[atom].max);
                    if amin == 0 && start0.is_none() {
                        let copy = self.copy_atom(atom);
                        self.arena[copy].quant = QUANT_ONCE;
                        self.arena[copy].min = 0;
                        self.arena[copy].max = 0;
                        if self.gen_transitions(start, None, copy) < 0 {
                            return -1;
                        }
                        let inter = self.state.unwrap();
                        let counter = self.get_counter();
                        self.counters[counter].min = amin - 1;
                        self.counters[counter].max = amax - 1;
                        self.gen_counted_epsilon(inter, Some(stop), counter as i32);
                        self.gen_counted(inter, Some(newstate), counter as i32);
                        self.gen_epsilon(start, Some(newstate));
                    } else {
                        let counter = self.get_counter();
                        self.counters[counter].min = amin - 1;
                        self.counters[counter].max = amax - 1;
                        self.gen_counted(stop, Some(newstate), counter as i32);
                        self.gen_counted_epsilon(stop, Some(start), counter as i32);
                        if amin == 0 {
                            self.gen_epsilon(start0.unwrap(), Some(newstate));
                        }
                    }
                    let a = &mut self.arena[atom];
                    a.min = 0;
                    a.max = 0;
                    a.quant = QUANT_ONCE;
                    self.state = Some(newstate);
                }
                _ => {}
            }
            self.atom_push(atom);
            return 0;
        }
        if self.arena[atom].min == 0
            && self.arena[atom].max == 0
            && self.arena[atom].quant == QUANT_RANGE
        {
            let to = match to {
                Some(t) => t,
                None => self.state_push(),
            };
            self.gen_epsilon(from, Some(to));
            self.state = Some(to);
            return 0;
        }
        let mut to = match to {
            Some(t) => t,
            None => self.state_push(),
        };
        let end = to;
        let quant = self.arena[atom].quant;
        if quant == QUANT_MULT || quant == QUANT_PLUS {
            let tmp = self.state_push();
            self.gen_epsilon(tmp, Some(to));
            to = tmp;
        }
        let mut nullable = false;
        {
            let a = &mut self.arena[atom];
            if a.quant == QUANT_RANGE && a.min == 0 && a.max > 0 {
                nullable = true;
                a.min = 1;
                if a.max == 1 {
                    a.quant = QUANT_OPT;
                }
            }
        }
        self.state_add_trans(from, Some(atom), to, -1, -1);
        self.state = Some(end);
        match self.arena[atom].quant {
            QUANT_OPT => {
                self.arena[atom].quant = QUANT_ONCE;
                self.gen_epsilon(from, Some(to));
            }
            QUANT_MULT => {
                self.arena[atom].quant = QUANT_ONCE;
                self.gen_epsilon(from, Some(to));
                self.state_add_trans(to, Some(atom), to, -1, -1);
            }
            QUANT_PLUS => {
                self.arena[atom].quant = QUANT_ONCE;
                self.state_add_trans(to, Some(atom), to, -1, -1);
            }
            QUANT_RANGE => {
                if nullable {
                    self.gen_epsilon(from, Some(to));
                }
            }
            _ => {}
        }
        self.atom_push(atom);
        0
    }

    fn reduce_epsilon(&mut self, fromnr: usize, tonr: usize, counter: i32) {
        if self.states[fromnr].is_none() || self.states[tonr].is_none() {
            return;
        }
        {
            let to = self.st(tonr);
            if to.mark == MARK_START || to.mark == MARK_VISITED {
                return;
            }
            to.mark = MARK_VISITED;
        }
        if self.st(tonr).typ == FINAL_STATE {
            self.st(fromnr).typ = FINAL_STATE;
        }
        let mut transnr = 0;
        while transnr < self.st(tonr).trans.len() {
            let t1 = self.st(tonr).trans[transnr];
            transnr += 1;
            if t1.to < 0 {
                continue;
            }
            let tcounter = if t1.counter >= 0 { t1.counter } else { counter };
            if t1.atom.is_none() {
                if t1.to != fromnr as i32 {
                    if t1.count >= 0 {
                        self.state_add_trans(fromnr, None, t1.to as usize, -1, t1.count);
                    } else {
                        self.reduce_epsilon(fromnr, t1.to as usize, tcounter);
                    }
                }
            } else {
                self.state_add_trans(fromnr, t1.atom, t1.to as usize, tcounter, -1);
            }
        }
    }

    fn finish_reduce_epsilon(&mut self, tonr: usize) {
        if self.states[tonr].is_none() {
            return;
        }
        {
            let to = self.st(tonr);
            if to.mark == MARK_START || to.mark == MARK_NORMAL {
                return;
            }
            to.mark = MARK_NORMAL;
        }
        let n = self.st(tonr).trans.len();
        for i in 0..n {
            let t1 = self.st(tonr).trans[i];
            if t1.to >= 0 && t1.atom.is_none() {
                self.finish_reduce_epsilon(t1.to as usize);
            }
        }
    }

    fn eliminate_simple_epsilon(&mut self) {
        for statenr in 0..self.states.len() {
            let (ok, newto, is_start) = match &self.states[statenr] {
                None => continue,
                Some(s) => {
                    if s.trans.len() != 1 || s.typ == UNREACH_STATE || s.typ == FINAL_STATE {
                        continue;
                    }
                    let t = s.trans[0];
                    (
                        t.atom.is_none()
                            && t.to >= 0
                            && t.to != statenr as i32
                            && t.counter < 0
                            && t.count < 0,
                        t.to,
                        s.typ == START_STATE,
                    )
                }
            };
            if !ok {
                continue;
            }
            if is_start {
                continue;
            }
            let trans_to = self.st(statenr).trans_to.clone();
            for from in trans_to {
                let n = self.states[from]
                    .as_ref()
                    .map(|s| s.trans.len())
                    .unwrap_or(0);
                for j in 0..n {
                    let t = self.st(from).trans[j];
                    if t.to == statenr as i32 {
                        self.st(from).trans[j].to = -1;
                        self.state_add_trans(from, t.atom, newto as usize, t.counter, t.count);
                    }
                }
            }
            let s = self.st(statenr);
            s.trans.clear();
            s.typ = UNREACH_STATE;
        }
    }

    /// xmlFAEliminateEpsilonTransitions
    fn eliminate_epsilon(&mut self) {
        if self.states.is_empty() {
            return;
        }
        self.eliminate_simple_epsilon();
        for s in self.states.iter_mut() {
            if s.as_ref().map(|x| x.typ == UNREACH_STATE).unwrap_or(false) {
                *s = None;
            }
        }
        let mut has_epsilon = false;
        for statenr in (0..self.states.len()).rev() {
            if self.states[statenr].is_none() {
                continue;
            }
            {
                let s = self.st(statenr);
                if s.trans.is_empty() && s.typ != FINAL_STATE {
                    s.typ = SINK_STATE;
                }
            }
            let mut transnr = 0;
            while transnr < self.st(statenr).trans.len() {
                let t = self.st(statenr).trans[transnr];
                if t.atom.is_none() && t.to >= 0 {
                    if t.to == statenr as i32 {
                        self.st(statenr).trans[transnr].to = -1;
                    } else if t.count < 0 {
                        let newto = t.to as usize;
                        has_epsilon = true;
                        self.st(statenr).trans[transnr].to = -2;
                        self.st(statenr).mark = MARK_START;
                        self.reduce_epsilon(statenr, newto, t.counter);
                        self.finish_reduce_epsilon(newto);
                        self.st(statenr).mark = MARK_NORMAL;
                    }
                }
                transnr += 1;
            }
        }
        if has_epsilon {
            for s in self.states.iter_mut().flatten() {
                for t in s.trans.iter_mut() {
                    if t.atom.is_none() && t.count < 0 && t.to >= 0 {
                        t.to = -1;
                    }
                }
            }
        }
        // unreachable states
        for s in self.states.iter_mut().flatten() {
            s.reached = MARK_NORMAL;
        }
        let mut state = if self.states[0].is_some() {
            Some(0usize)
        } else {
            None
        };
        if let Some(s) = state {
            self.st(s).reached = MARK_START;
        }
        while let Some(sid) = state {
            let mut target = None;
            self.st(sid).reached = MARK_VISITED;
            let trans = self.st(sid).trans.clone();
            for t in trans {
                if t.to >= 0 && (t.atom.is_some() || t.count >= 0) {
                    let newto = t.to as usize;
                    match &mut self.states[newto] {
                        None => continue,
                        Some(ns) => {
                            if ns.reached == MARK_NORMAL {
                                ns.reached = MARK_START;
                                target = Some(newto);
                            }
                        }
                    }
                }
            }
            if target.is_none() {
                for statenr in 1..self.states.len() {
                    if let Some(s) = &self.states[statenr] {
                        if s.reached == MARK_START {
                            target = Some(statenr);
                            break;
                        }
                    }
                }
            }
            state = target;
        }
        for s in self.states.iter_mut() {
            if s.as_ref()
                .map(|x| x.reached == MARK_NORMAL)
                .unwrap_or(false)
            {
                *s = None;
            }
        }
    }

    // ---------------------------------------------------------------
    // Regexp parser

    fn is_char_cur(&mut self) -> i32 {
        let (c, _) = self.utf8_at_cur();
        if c < 0 {
            self.error_msg("Invalid UTF-8");
            return 0;
        }
        if matches!(
            c,
            0x2E | 0x5C | 0x3F | 0x2A | 0x2B | 0x28 | 0x29 | 0x7C | 0x5B | 0x5D | 0
        ) {
            return -1;
        }
        c
    }

    fn parse_char_prop(&mut self) {
        let mut cur = self.cur();
        let mut block: Option<String> = None;
        let typ;
        macro_rules! sub {
            ($($ch:expr => $t:expr),* ; $def:expr) => {{
                self.next();
                cur = self.cur();
                let mut r = $def;
                $( if cur == $ch { self.next(); r = $t; } else )* {}
                r
            }};
        }
        if cur == b'L' {
            typ = sub!(b'u' => LETTER_UPPERCASE, b'l' => LETTER_LOWERCASE, b't' => LETTER_TITLECASE, b'm' => LETTER_MODIFIER, b'o' => LETTER_OTHERS; LETTER);
        } else if cur == b'M' {
            typ = sub!(b'n' => MARK_NONSPACING, b'c' => MARK_SPACECOMBINING, b'e' => MARK_ENCLOSING; MARK);
        } else if cur == b'N' {
            typ =
                sub!(b'd' => NUMBER_DECIMAL, b'l' => NUMBER_LETTER, b'o' => NUMBER_OTHERS; NUMBER);
        } else if cur == b'P' {
            typ = sub!(b'c' => PUNCT_CONNECTOR, b'd' => PUNCT_DASH, b's' => PUNCT_OPEN, b'e' => PUNCT_CLOSE, b'i' => PUNCT_INITQUOTE, b'f' => PUNCT_FINQUOTE, b'o' => PUNCT_OTHERS; PUNCT);
        } else if cur == b'Z' {
            typ = sub!(b's' => SEPAR_SPACE, b'l' => SEPAR_LINE, b'p' => SEPAR_PARA; SEPAR);
        } else if cur == b'S' {
            typ = sub!(b'm' => SYMBOL_MATH, b'c' => SYMBOL_CURRENCY, b'k' => SYMBOL_MODIFIER, b'o' => SYMBOL_OTHERS; SYMBOL);
        } else if cur == b'C' {
            typ = sub!(b'c' => OTHER_CONTROL, b'f' => OTHER_FORMAT, b'o' => OTHER_PRIVATE, b'n' => OTHER_NA; OTHER);
        } else if cur == b'I' {
            self.next();
            cur = self.cur();
            if cur != b's' {
                self.error_msg("IsXXXX expected");
                return;
            }
            self.next();
            let start = self.cur;
            while self.cur().is_ascii_alphanumeric() || self.cur() == 0x2D {
                self.next();
            }
            typ = BLOCK_NAME;
            block = Some(String::from_utf8_lossy(&self.string[start..self.cur]).into_owned());
        } else {
            self.error_msg("Unknown char property");
            return;
        }
        match self.atom {
            None => {
                let a = self.new_atom(typ);
                self.arena[a].valuep = block;
                self.atom = Some(a);
            }
            Some(a) if self.arena[a].typ == RANGES => {
                let neg = self.neg;
                self.atom_add_range(Some(a), neg, typ, 0, 0, block);
            }
            _ => {}
        }
    }

    fn parse_escaped_codeunit(&mut self) -> i32 {
        let mut val = 0i32;
        for _ in 0..4 {
            self.next();
            val *= 16;
            let c = self.cur();
            match c {
                b'0'..=b'9' => val += (c - b'0') as i32,
                b'A'..=b'F' => val += (c - b'A') as i32 + 10,
                b'a'..=b'f' => val += (c - b'a') as i32 + 10,
                _ => {
                    self.error_msg("Expecting hex digit");
                    return -1;
                }
            }
        }
        val
    }

    fn parse_escaped_codepoint(&mut self) -> i32 {
        let mut val = self.parse_escaped_codeunit();
        if (0xD800..=0xDBFF).contains(&val) {
            self.next();
            if self.cur() == b'\\' {
                self.next();
                if self.cur() == b'u' {
                    let low = self.parse_escaped_codeunit();
                    if (0xDC00..=0xDFFF).contains(&low) {
                        return (val - 0xD800) * 0x400 + (low - 0xDC00) + 0x10000;
                    }
                }
            }
            self.error_msg("Invalid low surrogate pair code unit");
            val = -1;
        }
        val
    }

    fn parse_char_class_esc(&mut self) {
        if self.cur() == b'.' {
            match self.atom {
                None => self.atom = Some(self.new_atom(ANYCHAR)),
                Some(a) if self.arena[a].typ == RANGES => {
                    let neg = self.neg;
                    self.atom_add_range(Some(a), neg, ANYCHAR, 0, 0, None);
                }
                _ => {}
            }
            self.next();
            return;
        }
        if self.cur() != b'\\' {
            self.error_msg("Escaped sequence: expecting \\");
            return;
        }
        self.next();
        let cur = self.cur();
        if cur == b'p' || cur == b'P' {
            self.next();
            if self.cur() != b'{' {
                self.error_msg("Expecting '{'");
                return;
            }
            self.next();
            self.parse_char_prop();
            if cur == b'P' {
                if let Some(a) = self.atom {
                    self.arena[a].neg = true;
                }
            }
            if self.cur() != b'}' {
                self.error_msg("Expecting '}'");
                return;
            }
            self.next();
        } else if b"nrt\\|.?*+(){}-[]^!\"#$%,/:;=>@`~u".contains(&cur) && cur != 0 {
            match self.atom {
                None => {
                    let a = self.new_atom(CHARVAL);
                    self.atom = Some(a);
                    let cp = match cur {
                        b'n' => '\n' as i32,
                        b'r' => '\r' as i32,
                        b't' => '\t' as i32,
                        b'u' => {
                            let c = self.parse_escaped_codepoint();
                            if c < 0 {
                                return;
                            }
                            c
                        }
                        _ => cur as i32,
                    };
                    self.arena[a].codepoint = cp;
                }
                Some(a) if self.arena[a].typ == RANGES => {
                    let c = match cur {
                        b'n' => '\n' as i32,
                        b'r' => '\r' as i32,
                        b't' => '\t' as i32,
                        _ => cur as i32,
                    };
                    let neg = self.neg;
                    self.atom_add_range(Some(a), neg, CHARVAL, c, c, None);
                }
                _ => {}
            }
            self.next();
        } else if b"sSiIcCdDwW".contains(&cur) && cur != 0 {
            let typ = match cur {
                b's' => ANYSPACE,
                b'S' => NOTSPACE,
                b'i' => INITNAME,
                b'I' => NOTINITNAME,
                b'c' => NAMECHAR,
                b'C' => NOTNAMECHAR,
                b'd' => DECIMAL,
                b'D' => NOTDECIMAL,
                b'w' => REALCHAR,
                _ => NOTREALCHAR,
            };
            self.next();
            match self.atom {
                None => self.atom = Some(self.new_atom(typ)),
                Some(a) if self.arena[a].typ == RANGES => {
                    let neg = self.neg;
                    self.atom_add_range(Some(a), neg, typ, 0, 0, None);
                }
                _ => {}
            }
        } else {
            self.error_msg("Wrong escape sequence, misuse of character '\\'");
        }
    }

    fn parse_char_range(&mut self) {
        if self.cur() == 0 {
            self.error_msg("Expecting ']'");
            return;
        }
        let mut cur = self.cur();
        let start;
        let mut end;
        let mut len;
        if cur == b'\\' {
            self.next();
            cur = self.cur();
            start = match cur {
                b'n' => 0xA,
                b'r' => 0xD,
                b't' => 0x9,
                b'\\' | b'|' | b'.' | b'-' | b'^' | b'?' | b'*' | b'+' | b'{' | b'}' | b'('
                | b')' | b'[' | b']' => cur as i32,
                _ => {
                    self.error_msg("Invalid escape value");
                    return;
                }
            };
            end = start;
            len = 1;
        } else if cur != 0x5B && cur != 0x5D {
            let (c, l) = self.utf8_at_cur();
            if c < 0 {
                self.error_msg("Invalid UTF-8");
                return;
            }
            start = c;
            end = c;
            len = l;
        } else {
            self.error_msg("Expecting a char range");
            return;
        }
        if start == '-' as i32 && self.at(1) != b']' && self.at(-1) != b'[' && self.at(-1) != b'^' {
            self.cur += len;
            return;
        }
        self.cur += len;
        cur = self.cur();
        if cur != b'-' || self.at(1) == b'[' || self.at(1) == b']' {
            let (a, neg) = (self.atom, self.neg);
            self.atom_add_range(a, neg, CHARVAL, start, end, None);
            return;
        }
        self.next();
        cur = self.cur();
        if cur == b'\\' {
            self.next();
            cur = self.cur();
            end = match cur {
                b'n' => 0xA,
                b'r' => 0xD,
                b't' => 0x9,
                b'\\' | b'|' | b'.' | b'-' | b'^' | b'?' | b'*' | b'+' | b'{' | b'}' | b'('
                | b')' | b'[' | b']' => cur as i32,
                _ => {
                    self.error_msg("Invalid escape value");
                    return;
                }
            };
            len = 1;
        } else if cur != 0 && cur != 0x5B && cur != 0x5D {
            let (c, l) = self.utf8_at_cur();
            if c < 0 {
                self.error_msg("Invalid UTF-8");
                return;
            }
            end = c;
            len = l;
        } else {
            self.error_msg("Expecting the end of a char range");
            return;
        }
        if end < start {
            self.error_msg("End of range is before start of range");
        } else {
            self.cur += len;
            let (a, neg) = (self.atom, self.neg);
            self.atom_add_range(a, neg, CHARVAL, start, end, None);
        }
    }

    fn parse_pos_char_group(&mut self) {
        loop {
            if self.cur() == b'\\' {
                self.parse_char_class_esc();
            } else {
                self.parse_char_range();
            }
            if !(self.cur() != b']' && self.cur() != b'-' && self.cur() != 0 && self.error == 0) {
                break;
            }
        }
    }

    fn parse_char_group(&mut self) {
        let neg = self.neg;
        if self.cur() == b'^' {
            self.next();
            self.neg = if self.neg != 0 { 0 } else { 1 };
            self.parse_pos_char_group();
            self.neg = neg;
        }
        while self.cur() != b']' && self.error == 0 {
            if self.cur() == b'-' && self.at(1) == b'[' {
                self.next();
                self.next();
                self.neg = 2;
                self.parse_char_group();
                self.neg = neg;
                if self.cur() == b']' {
                    self.next();
                } else {
                    self.error_msg("charClassExpr: ']' expected");
                }
                break;
            } else {
                self.parse_pos_char_group();
            }
        }
    }

    fn parse_char_class(&mut self) {
        if self.cur() == b'[' {
            self.next();
            self.atom = Some(self.new_atom(RANGES));
            self.parse_char_group();
            if self.cur() == b']' {
                self.next();
            } else {
                self.error_msg("xmlFAParseCharClass: ']' expected");
            }
        } else {
            self.parse_char_class_esc();
        }
    }

    fn parse_quant_exact(&mut self) -> i32 {
        let mut ret: i32 = 0;
        let mut ok = false;
        let mut overflow = false;
        while self.cur().is_ascii_digit() {
            if ret > i32::MAX / 10 {
                overflow = true;
            } else {
                let digit = (self.cur() - b'0') as i32;
                ret *= 10;
                if ret > i32::MAX - digit {
                    overflow = true;
                } else {
                    ret += digit;
                }
            }
            ok = true;
            self.next();
        }
        if !ok || overflow {
            return -1;
        }
        ret
    }

    fn parse_quantifier(&mut self) -> bool {
        let cur = self.cur();
        if cur == b'?' || cur == b'*' || cur == b'+' {
            if let Some(a) = self.atom {
                self.arena[a].quant = match cur {
                    b'?' => QUANT_OPT,
                    b'*' => QUANT_MULT,
                    _ => QUANT_PLUS,
                };
            }
            self.next();
            return true;
        }
        if cur == b'{' {
            let mut min = 0;
            let mut max = 0;
            self.next();
            let c = self.parse_quant_exact();
            if c >= 0 {
                min = c;
            } else {
                self.error_msg("Improper quantifier");
            }
            if self.cur() == b',' {
                self.next();
                if self.cur() == b'}' {
                    max = i32::MAX;
                } else {
                    let c = self.parse_quant_exact();
                    if c >= 0 {
                        max = c;
                    } else {
                        self.error_msg("Improper quantifier");
                    }
                }
            }
            if self.cur() == b'}' {
                self.next();
            } else {
                self.error_msg("Unterminated quantifier");
            }
            if max == 0 {
                max = min;
            }
            if let Some(a) = self.atom {
                let a = &mut self.arena[a];
                a.quant = QUANT_RANGE;
                a.min = min;
                a.max = max;
            }
            return true;
        }
        false
    }

    fn parse_atom(&mut self) -> i32 {
        let codepoint = self.is_char_cur();
        if codepoint > 0 {
            let a = self.new_atom(CHARVAL);
            self.atom = Some(a);
            let (c, l) = self.utf8_at_cur();
            if c < 0 {
                self.error_msg("Invalid UTF-8");
                return -1;
            }
            self.arena[a].codepoint = c;
            self.cur += l;
            return 1;
        }
        let cur = self.cur();
        if cur == b'|' || cur == 0 || cur == b')' {
            return 0;
        }
        if cur == b'(' {
            self.next();
            if self.depth >= 50 {
                self.error_msg("xmlFAParseAtom: maximum nesting depth exceeded");
                return -1;
            }
            let s = self.state.unwrap();
            self.gen_epsilon(s, None);
            let start0 = self.state;
            let s = self.state.unwrap();
            self.gen_epsilon(s, None);
            let start = self.state;
            let oldend = self.end;
            self.end = None;
            self.atom = None;
            self.depth += 1;
            self.parse_regexp(false);
            self.depth -= 1;
            if self.cur() == b')' {
                self.next();
            } else {
                self.error_msg("xmlFAParseAtom: expecting ')'");
            }
            let a = self.new_atom(SUBREG);
            self.arena[a].start = start;
            self.arena[a].start0 = start0;
            self.arena[a].stop = self.state;
            self.atom = Some(a);
            self.end = oldend;
            return 1;
        }
        if cur == b'[' || cur == b'\\' || cur == b'.' {
            self.parse_char_class();
            return 1;
        }
        0
    }

    fn parse_piece(&mut self) -> i32 {
        self.atom = None;
        let ret = self.parse_atom();
        if ret == 0 {
            return 0;
        }
        if self.atom.is_none() {
            self.error_msg("internal: no atom generated");
        }
        self.parse_quantifier();
        1
    }

    fn parse_branch(&mut self, to: Option<StateId>) -> i32 {
        let mut previous = self.state.unwrap();
        let mut ret = self.parse_piece();
        if ret == 0 {
            self.gen_epsilon(previous, to);
        } else {
            let c = self.cur();
            let t = if c == b'|' || c == b')' || c == 0 {
                to
            } else {
                None
            };
            let atom = match self.atom {
                Some(a) => a,
                None => return -1,
            };
            if self.gen_transitions(previous, t, atom) < 0 {
                self.atom = None;
                return -1;
            }
            previous = self.state.unwrap();
            self.atom = None;
        }
        while ret != 0 && self.error == 0 {
            ret = self.parse_piece();
            if ret != 0 {
                let c = self.cur();
                let t = if c == b'|' || c == b')' || c == 0 {
                    to
                } else {
                    None
                };
                let atom = match self.atom {
                    Some(a) => a,
                    None => return -1,
                };
                if self.gen_transitions(previous, t, atom) < 0 {
                    self.atom = None;
                    return -1;
                }
                previous = self.state.unwrap();
                self.atom = None;
            }
        }
        0
    }

    fn parse_regexp(&mut self, top: bool) {
        let start = self.state;
        self.end = None;
        self.parse_branch(None);
        if top {
            let s = self.state.unwrap();
            self.st(s).typ = FINAL_STATE;
        }
        if self.cur() != b'|' {
            self.end = self.state;
            return;
        }
        let end = self.state;
        while self.cur() == b'|' && self.error == 0 {
            self.next();
            self.state = start;
            self.end = None;
            self.parse_branch(end);
        }
        if !top {
            self.state = end;
            self.end = end;
        }
    }

    // ---------------------------------------------------------------
    // Determinism

    fn equal_atoms(&self, a1: Option<AtomId>, a2: Option<AtomId>, deep: bool) -> bool {
        if a1 == a2 {
            return true;
        }
        let (a1, a2) = match (a1, a2) {
            (Some(a), Some(b)) => (&self.arena[a], &self.arena[b]),
            _ => return false,
        };
        if a1.typ != a2.typ {
            return false;
        }
        match a1.typ {
            STRING => {
                if !deep {
                    // distinct atoms own distinct copies of their strings
                    false
                } else {
                    a1.valuep == a2.valuep
                }
            }
            CHARVAL => a1.codepoint == a2.codepoint,
            _ => false,
        }
    }

    fn compare_atoms(&self, a1: Option<AtomId>, a2: Option<AtomId>, deep: bool) -> bool {
        if a1 == a2 {
            return true;
        }
        let (mut a1, mut a2) = match (a1, a2) {
            (Some(a), Some(b)) => (&self.arena[a], &self.arena[b]),
            _ => return false,
        };
        if a1.typ == ANYCHAR || a2.typ == ANYCHAR {
            return true;
        }
        if a1.typ > a2.typ {
            std::mem::swap(&mut a1, &mut a2);
        }
        let mut ret;
        if a1.typ != a2.typ {
            ret = compare_atom_types(a1.typ, a2.typ);
            if !ret {
                return false;
            }
        }
        match a1.typ {
            STRING => {
                if !deep {
                    ret = true;
                } else {
                    let v1 = a1.valuep.as_deref().unwrap_or("");
                    let v2 = a2.valuep.as_deref().unwrap_or("");
                    let c1 = v1.contains('|');
                    let c2 = v2.contains('|');
                    if c1 != c2 {
                        return false;
                    }
                    ret = str_equal_wildcard(Some(v1), Some(v2));
                }
            }
            EPSILON => return true,
            CHARVAL => {
                if a2.typ == CHARVAL {
                    ret = a1.codepoint == a2.codepoint;
                } else {
                    let r = check_character(a2, a1.codepoint);
                    ret = r != 0;
                }
            }
            RANGES => {
                if a2.typ == RANGES {
                    ret = false;
                    'o: for r1 in &a1.ranges {
                        for r2 in &a2.ranges {
                            if compare_ranges(r1, r2) == 1 {
                                ret = true;
                                break 'o;
                            }
                        }
                    }
                } else {
                    ret = true;
                }
            }
            _ => return true,
        }
        if a1.neg != a2.neg {
            ret = !ret;
        }
        if !ret {
            return false;
        }
        true
    }

    fn recurse_determinism(
        &mut self,
        state: Option<StateId>,
        fromnr: i32,
        tonr: i32,
        atom: Option<AtomId>,
    ) -> bool {
        let sid = match state {
            Some(s) if self.states[s].is_some() => s,
            _ => return true,
        };
        if self.st(sid).markd == MARK_VISITED {
            return true;
        }
        let deep = self.flags & AM_AUTOMATA_RNG == 0;
        let mut ret = true;
        let nb = self.st(sid).trans.len();
        for transnr in 0..nb {
            let t1 = self.st(sid).trans[transnr];
            if t1.to < 0 || t1.to == fromnr {
                continue;
            }
            if t1.atom.is_none() {
                self.st(sid).markd = MARK_VISITED;
                if !self.recurse_determinism(Some(t1.to as usize), fromnr, tonr, atom) {
                    ret = false;
                }
                continue;
            }
            if self.compare_atoms(t1.atom, atom, deep) {
                if t1.to != tonr || !self.equal_atoms(t1.atom, atom, deep) {
                    ret = false;
                }
                self.st(sid).trans[transnr].nd = 1;
            }
        }
        ret
    }

    fn finish_recurse_determinism(&mut self, state: Option<StateId>) {
        let sid = match state {
            Some(s) if self.states[s].is_some() => s,
            _ => return,
        };
        if self.st(sid).markd != MARK_VISITED {
            return;
        }
        self.st(sid).markd = 0;
        let nb = self.st(sid).trans.len();
        for i in 0..nb {
            let t1 = self.st(sid).trans[i];
            if t1.atom.is_none() && t1.to >= 0 {
                self.finish_recurse_determinism(Some(t1.to as usize));
            }
        }
    }

    /// xmlFAComputesDeterminism
    fn computes_determinism(&mut self) -> i32 {
        if self.determinist != -1 {
            return self.determinist;
        }
        let deep = self.flags & AM_AUTOMATA_RNG == 0;
        for statenr in 0..self.states.len() {
            if self.states[statenr].is_none() {
                continue;
            }
            let n = self.st(statenr).trans.len();
            if n < 2 {
                continue;
            }
            for transnr in 0..n {
                let t1 = self.st(statenr).trans[transnr];
                if t1.atom.is_none() || t1.to < 0 {
                    continue;
                }
                for i in 0..transnr {
                    let t2 = self.st(statenr).trans[i];
                    if t2.to < 0 {
                        continue;
                    }
                    if t2.atom.is_some()
                        && t1.to == t2.to
                        && self.equal_atoms(t1.atom, t2.atom, deep)
                        && t1.counter == t2.counter
                        && t1.count == t2.count
                    {
                        self.st(statenr).trans[i].to = -1;
                    }
                }
            }
        }
        let mut ret = 1;
        for statenr in 0..self.states.len() {
            if self.states[statenr].is_none() {
                continue;
            }
            let n = self.st(statenr).trans.len();
            if n < 2 {
                continue;
            }
            let mut last: Option<usize> = None;
            for transnr in 0..n {
                let t1 = self.st(statenr).trans[transnr];
                if t1.atom.is_none() || t1.to < 0 {
                    continue;
                }
                for i in 0..transnr {
                    let t2 = self.st(statenr).trans[i];
                    if t2.to < 0 {
                        continue;
                    }
                    if t2.atom.is_some() {
                        if self.compare_atoms(t1.atom, t2.atom, true) {
                            if t1.to != t2.to
                                || t1.counter == t2.counter
                                || !self.equal_atoms(t1.atom, t2.atom, deep)
                            {
                                ret = 0;
                            }
                            self.st(statenr).trans[transnr].nd = 1;
                            self.st(statenr).trans[i].nd = 1;
                            last = Some(transnr);
                        }
                    } else {
                        let res = self.recurse_determinism(
                            Some(t2.to as usize),
                            statenr as i32,
                            t1.to,
                            t1.atom,
                        );
                        self.finish_recurse_determinism(Some(t2.to as usize));
                        if !res {
                            self.st(statenr).trans[transnr].nd = 1;
                            last = Some(transnr);
                            ret = 0;
                        }
                    }
                }
            }
            if let Some(l) = last {
                self.st(statenr).trans[l].nd = 2;
            }
        }
        self.determinist = ret;
        ret
    }

    // ---------------------------------------------------------------
    // Public automata API (xmlAutomata*)

    pub fn init_state(&self) -> StateId {
        self.start.unwrap()
    }

    pub fn set_flags(&mut self, flags: i32) {
        self.flags |= flags;
    }

    pub fn set_final_state(&mut self, s: StateId) {
        self.st(s).typ = FINAL_STATE;
    }

    fn concat(token: &str, token2: Option<&str>) -> String {
        match token2 {
            Some(t2) if !t2.is_empty() => format!("{}|{}", token, t2),
            _ => token.to_string(),
        }
    }

    pub fn new_transition(
        &mut self,
        from: StateId,
        to: Option<StateId>,
        token: &str,
        data: Option<usize>,
    ) -> Option<StateId> {
        self.new_transition2(from, to, token, None, data)
    }

    pub fn new_transition2(
        &mut self,
        from: StateId,
        to: Option<StateId>,
        token: &str,
        token2: Option<&str>,
        data: Option<usize>,
    ) -> Option<StateId> {
        let a = self.new_atom(STRING);
        self.arena[a].data = data;
        self.arena[a].valuep = Some(Self::concat(token, token2));
        if self.gen_transitions(from, to, a) < 0 {
            return None;
        }
        Some(to.unwrap_or_else(|| self.state.unwrap()))
    }

    pub fn new_neg_trans(
        &mut self,
        from: StateId,
        to: Option<StateId>,
        token: &str,
        token2: Option<&str>,
        data: Option<usize>,
    ) -> Option<StateId> {
        let a = self.new_atom(STRING);
        self.arena[a].data = data;
        self.arena[a].neg = true;
        let v = Self::concat(token, token2);
        let mut msg = format!("not {}", v);
        if msg.len() > 198 {
            let mut cut = 198;
            while !msg.is_char_boundary(cut) {
                cut -= 1;
            }
            msg.truncate(cut);
        }
        self.arena[a].valuep = Some(v);
        self.arena[a].valuep2 = Some(msg);
        if self.gen_transitions(from, to, a) < 0 {
            return None;
        }
        self.negs += 1;
        Some(to.unwrap_or_else(|| self.state.unwrap()))
    }

    pub fn new_count_trans2(
        &mut self,
        from: StateId,
        to: Option<StateId>,
        token: &str,
        token2: Option<&str>,
        min: i32,
        max: i32,
        data: Option<usize>,
    ) -> Option<StateId> {
        if min < 0 || max < min || max < 1 {
            return None;
        }
        let a = self.new_atom(STRING);
        self.arena[a].valuep = Some(Self::concat(token, token2));
        self.arena[a].data = data;
        self.arena[a].min = if min == 0 { 1 } else { min };
        self.arena[a].max = max;
        let counter = self.get_counter();
        self.counters[counter].min = min;
        self.counters[counter].max = max;
        let to = match to {
            Some(t) => t,
            None => self.state_push(),
        };
        self.state_add_trans(from, Some(a), to, counter as i32, -1);
        self.atom_push(a);
        self.state = Some(to);
        if min == 0 {
            self.gen_epsilon(from, Some(to));
        }
        Some(to)
    }

    pub fn new_count_trans(
        &mut self,
        from: StateId,
        to: Option<StateId>,
        token: &str,
        min: i32,
        max: i32,
        data: Option<usize>,
    ) -> Option<StateId> {
        self.new_count_trans2(from, to, token, None, min, max, data)
    }

    pub fn new_once_trans2(
        &mut self,
        from: StateId,
        to: Option<StateId>,
        token: &str,
        token2: Option<&str>,
        min: i32,
        max: i32,
        data: Option<usize>,
    ) -> Option<StateId> {
        if min < 1 || max < min {
            return None;
        }
        let a = self.new_atom(STRING);
        self.arena[a].valuep = Some(Self::concat(token, token2));
        self.arena[a].data = data;
        self.arena[a].quant = QUANT_ONCEONLY;
        self.arena[a].min = min;
        self.arena[a].max = max;
        let counter = self.get_counter();
        self.counters[counter].min = 1;
        self.counters[counter].max = 1;
        let to = match to {
            Some(t) => t,
            None => self.state_push(),
        };
        self.state_add_trans(from, Some(a), to, counter as i32, -1);
        self.atom_push(a);
        self.state = Some(to);
        Some(to)
    }

    pub fn new_state(&mut self) -> StateId {
        self.state_push()
    }

    pub fn new_epsilon(&mut self, from: StateId, to: Option<StateId>) -> StateId {
        self.gen_epsilon(from, to);
        to.unwrap_or_else(|| self.state.unwrap())
    }

    pub fn new_all_trans(&mut self, from: StateId, to: Option<StateId>, lax: bool) -> StateId {
        self.gen_all_transition(from, to, lax);
        to.unwrap_or_else(|| self.state.unwrap())
    }

    pub fn new_counter(&mut self, min: i32, max: i32) -> i32 {
        let c = self.get_counter();
        self.counters[c].min = min;
        self.counters[c].max = max;
        c as i32
    }

    pub fn new_counted_trans(
        &mut self,
        from: StateId,
        to: Option<StateId>,
        counter: i32,
    ) -> StateId {
        self.gen_counted_epsilon(from, to, counter);
        to.unwrap_or_else(|| self.state.unwrap())
    }

    pub fn new_counter_trans(
        &mut self,
        from: StateId,
        to: Option<StateId>,
        counter: i32,
    ) -> StateId {
        self.gen_counted(from, to, counter);
        to.unwrap_or_else(|| self.state.unwrap())
    }

    /// xmlAutomataIsDeterminist
    pub fn is_determinist(&mut self) -> bool {
        self.computes_determinism() == 1
    }

    /// xmlAutomataCompile
    pub fn compile(mut self) -> Option<Regexp> {
        if self.error != 0 {
            return None;
        }
        self.eliminate_epsilon();
        if self.error != 0 {
            return None;
        }
        Some(self.into_regexp())
    }

    /// xmlRegEpxFromParse
    fn into_regexp(mut self) -> Regexp {
        if self.determinist == -1 {
            // xmlRegexpIsDeterminist on the compiled form
            self.determinist = -1;
            self.computes_determinism();
        }
        let string = String::from_utf8_lossy(&self.string).into_owned();
        let mut ret = Regexp {
            string,
            states: Vec::new(),
            arena: Vec::new(),
            nb_atoms: self.atoms.len(),
            counters: self.counters.clone(),
            determinist: self.determinist,
            flags: self.flags,
            compact: None,
            transdata: None,
            nbstates: 0,
            string_map: Vec::new(),
        };
        let first_is_string = self
            .atoms
            .first()
            .map(|&a| self.arena[a].typ == STRING)
            .unwrap_or(false);
        if ret.determinist != 0 && ret.counters.is_empty() && self.negs == 0 && first_is_string {
            // compact representation
            let mut state_remap = vec![-1i32; self.states.len()];
            let mut nbstates = 0;
            for (i, s) in self.states.iter().enumerate() {
                if s.is_some() {
                    state_remap[i] = nbstates;
                    nbstates += 1;
                }
            }
            let mut string_map: Vec<String> = Vec::new();
            let mut string_remap = vec![0usize; self.atoms.len()];
            let mut ok = true;
            for (i, &aid) in self.atoms.iter().enumerate() {
                let a = &self.arena[aid];
                if a.typ == STRING && a.quant == QUANT_ONCE {
                    let v = a.valuep.clone().unwrap_or_default();
                    match string_map.iter().position(|s| *s == v) {
                        Some(j) => string_remap[i] = j,
                        None => {
                            string_remap[i] = string_map.len();
                            string_map.push(v);
                        }
                    }
                } else {
                    ok = false;
                    break;
                }
            }
            if ok {
                let nbatoms = string_map.len();
                let mut transitions = vec![0i32; (nbstates as usize + 1) * (nbatoms + 1)];
                let mut transdata: Option<Vec<Option<usize>>> = None;
                let mut not_determ = false;
                'outer: for (i, s) in self.states.iter().enumerate() {
                    let stateno = state_remap[i];
                    if stateno == -1 {
                        continue;
                    }
                    let s = s.as_ref().unwrap();
                    transitions[stateno as usize * (nbatoms + 1)] = s.typ;
                    for t in &s.trans {
                        if t.to < 0 || t.atom.is_none() {
                            continue;
                        }
                        let aid = t.atom.unwrap();
                        let atomno = string_remap[self.arena[aid].no as usize];
                        if self.arena[aid].data.is_some() && transdata.is_none() {
                            transdata = Some(vec![None; nbstates as usize * nbatoms]);
                        }
                        let targetno = state_remap[t.to as usize];
                        let idx = stateno as usize * (nbatoms + 1) + atomno + 1;
                        let prev = transitions[idx];
                        if prev != 0 {
                            if prev != targetno + 1 {
                                ret.determinist = 0;
                                not_determ = true;
                                break 'outer;
                            }
                        } else {
                            transitions[idx] = targetno + 1;
                            if let Some(td) = transdata.as_mut() {
                                td[stateno as usize * nbatoms + atomno] = self.arena[aid].data;
                            }
                        }
                    }
                }
                if !not_determ {
                    ret.determinist = 1;
                    ret.compact = Some(transitions);
                    ret.transdata = transdata;
                    ret.string_map = string_map;
                    ret.nbstates = nbstates;
                    ret.nb_atoms = 0;
                    return ret;
                }
            }
        }
        ret.states = std::mem::take(&mut self.states);
        ret.arena = std::mem::take(&mut self.arena);
        ret
    }
}

fn compare_ranges(r1: &Range, r2: &Range) -> i32 {
    let (mut r1, mut r2) = (r1, r2);
    if r1.typ == RANGES
        || r2.typ == RANGES
        || r2.typ == SUBREG
        || r1.typ == SUBREG
        || r1.typ == STRING
        || r2.typ == STRING
    {
        return -1;
    }
    if r1.typ > r2.typ {
        std::mem::swap(&mut r1, &mut r2);
    }
    let mut ret: bool;
    if r1.typ == ANYCHAR || r2.typ == ANYCHAR {
        ret = true;
    } else if r1.typ == EPSILON || r2.typ == EPSILON {
        return 0;
    } else if r1.typ == r2.typ {
        if r1.typ != CHARVAL {
            ret = true;
        } else {
            ret = !(r1.end < r2.start || r2.end < r1.start);
        }
    } else if r1.typ == CHARVAL {
        let neg = (r1.neg == 0) != (r2.neg == 0);
        for cp in r1.start..=r1.end {
            let r = check_character_range(r2.typ, cp, false, r2.start, r2.end, r2.block.as_deref());
            if r < 0 {
                return -1;
            }
            if (neg && r == 0) || (!neg && r == 1) {
                return 1;
            }
        }
        return 0;
    } else if r1.typ == BLOCK_NAME || r2.typ == BLOCK_NAME {
        if r1.typ == r2.typ {
            ret = r1.block == r2.block;
        } else {
            return 1;
        }
    } else if r1.typ < LETTER || r2.typ < LETTER {
        if (r1.typ == ANYSPACE && r2.typ == NOTSPACE)
            || (r1.typ == INITNAME && r2.typ == NOTINITNAME)
            || (r1.typ == NAMECHAR && r2.typ == NOTNAMECHAR)
            || (r1.typ == DECIMAL && r2.typ == NOTDECIMAL)
            || (r1.typ == REALCHAR && r2.typ == NOTREALCHAR)
        {
            ret = false;
        } else {
            return 1;
        }
    } else {
        let t2 = r2.typ;
        match r1.typ {
            LETTER => ret = (LETTER_UPPERCASE..=LETTER_OTHERS).contains(&t2),
            MARK => ret = (MARK_NONSPACING..=MARK_ENCLOSING).contains(&t2),
            NUMBER => ret = (NUMBER_DECIMAL..=NUMBER_OTHERS).contains(&t2),
            PUNCT => ret = (PUNCT_CONNECTOR..=PUNCT_OTHERS).contains(&t2),
            SEPAR => ret = (SEPAR_SPACE..=SEPAR_PARA).contains(&t2),
            SYMBOL => ret = (SYMBOL_MATH..=SYMBOL_OTHERS).contains(&t2),
            OTHER => ret = (OTHER_CONTROL..=OTHER_PRIVATE).contains(&t2),
            _ => {
                if (LETTER..BLOCK_NAME).contains(&t2) {
                    ret = false;
                } else {
                    return 1;
                }
            }
        }
    }
    if (r1.neg == 0) != (r2.neg == 0) {
        ret = !ret;
    }
    ret as i32
}

fn compare_atom_types(t1: i32, t2: i32) -> bool {
    let simple = |t: i32| matches!(t, EPSILON | CHARVAL | RANGES | SUBREG | STRING | ANYCHAR);
    if simple(t1) || simple(t2) {
        return true;
    }
    if t1 == t2 {
        return true;
    }
    let (t1, t2) = if t1 > t2 { (t2, t1) } else { (t1, t2) };
    let inr = |a: i32, b: i32| (a..=b).contains(&t2);
    match t1 {
        ANYSPACE => {
            if t2 == NOTSPACE
                || inr(LETTER, LETTER_OTHERS)
                || inr(NUMBER, NUMBER_OTHERS)
                || inr(MARK, MARK_ENCLOSING)
                || inr(PUNCT, PUNCT_OTHERS)
                || inr(SYMBOL, SYMBOL_OTHERS)
            {
                return false;
            }
        }
        INITNAME => {
            if t2 == NOTINITNAME
                || inr(NUMBER, NUMBER_OTHERS)
                || inr(MARK, MARK_ENCLOSING)
                || inr(SEPAR, SEPAR_PARA)
                || inr(PUNCT, PUNCT_OTHERS)
                || inr(SYMBOL, SYMBOL_OTHERS)
                || inr(OTHER, OTHER_NA)
            {
                return false;
            }
        }
        NAMECHAR => {
            if t2 == NOTNAMECHAR
                || inr(MARK, MARK_ENCLOSING)
                || inr(PUNCT, PUNCT_OTHERS)
                || inr(SEPAR, SEPAR_PARA)
                || inr(SYMBOL, SYMBOL_OTHERS)
                || inr(OTHER, OTHER_NA)
            {
                return false;
            }
        }
        DECIMAL => {
            if t2 == NOTDECIMAL
                || t2 == REALCHAR
                || inr(LETTER, LETTER_OTHERS)
                || inr(MARK, MARK_ENCLOSING)
                || inr(PUNCT, PUNCT_OTHERS)
                || inr(SEPAR, SEPAR_PARA)
                || inr(SYMBOL, SYMBOL_OTHERS)
                || inr(OTHER, OTHER_NA)
            {
                return false;
            }
        }
        REALCHAR => {
            if t2 == NOTDECIMAL
                || inr(MARK, MARK_ENCLOSING)
                || inr(PUNCT, PUNCT_OTHERS)
                || inr(SEPAR, SEPAR_PARA)
                || inr(SYMBOL, SYMBOL_OTHERS)
                || inr(OTHER, OTHER_NA)
            {
                return false;
            }
        }
        NOTSPACE | NOTINITNAME | NOTNAMECHAR | NOTDECIMAL | NOTREALCHAR => {}
        LETTER => return t2 <= LETTER_OTHERS,
        LETTER_UPPERCASE..=LETTER_OTHERS => return false,
        MARK => return t2 <= MARK_ENCLOSING,
        MARK_NONSPACING..=MARK_ENCLOSING => return false,
        NUMBER => return t2 <= NUMBER_OTHERS,
        NUMBER_DECIMAL..=NUMBER_OTHERS => return false,
        PUNCT => return t2 <= PUNCT_OTHERS,
        PUNCT_CONNECTOR..=PUNCT_OTHERS => return false,
        SEPAR => return t2 <= SEPAR_PARA,
        SEPAR_SPACE..=SEPAR_PARA => return false,
        SYMBOL => return t2 <= SYMBOL_OTHERS,
        SYMBOL_MATH..=SYMBOL_OTHERS => return false,
        OTHER => return t2 <= OTHER_NA,
        OTHER_CONTROL..=OTHER_NA => return false,
        _ => {}
    }
    true
}

/// Result of compiling an XML Schema regular expression.
pub fn regexp_compile(re: &str) -> Result<Regexp, Vec<String>> {
    let mut ctxt = Automata::new_ctxt(Some(re));
    let s = ctxt.state_push();
    ctxt.state = Some(s);
    ctxt.start = Some(s);
    ctxt.end = None;
    ctxt.parse_regexp(true);
    if ctxt.cur() != 0 {
        ctxt.error_msg("xmlFAParseRegExp: extra characters");
    }
    if ctxt.error != 0 {
        return Err(ctxt.errors);
    }
    ctxt.end = ctxt.state;
    let st = ctxt.start.unwrap();
    ctxt.st(st).typ = START_STATE;
    let e = ctxt.end.unwrap();
    ctxt.st(e).typ = FINAL_STATE;
    ctxt.eliminate_epsilon();
    if ctxt.error != 0 {
        return Err(ctxt.errors);
    }
    Ok(ctxt.into_regexp())
}

#[derive(Clone)]
struct Rollback {
    state: Option<StateId>,
    index: i32,
    nextbranch: i32,
    counts: Vec<i32>,
}

impl std::fmt::Debug for Regexp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Regexp({:?})", self.string)
    }
}

impl Regexp {
    pub fn is_determinist(&self) -> bool {
        self.determinist == 1
    }

    fn st(&self, id: usize) -> &State {
        self.states[id].as_ref().expect("state")
    }

    /// xmlRegexpExec: 1 match, 0 no match, <0 error.
    pub fn exec(&self, content: &str) -> i32 {
        if self.states.is_empty() {
            return REGEXP_INTERNAL_ERROR;
        }
        let mut input: Vec<i32> = content.chars().map(|c| c as i32).collect();
        input.push(0);
        let nbc = self.counters.len();
        let mut counts: Vec<i32> = vec![0; nbc];
        let mut rollbacks: Vec<Rollback> = Vec::new();
        let mut status = REGEXP_OK;
        let mut state: Option<StateId> = Some(0);
        if self.states[0].is_none() {
            return REGEXP_INTERNAL_ERROR;
        }
        let mut transno: i32 = 0;
        let mut index: i32 = 0;
        let has_counts = nbc > 0;
        macro_rules! save {
            () => {{
                rollbacks.push(Rollback {
                    state,
                    index,
                    nextbranch: transno + 1,
                    counts: counts.clone(),
                });
            }};
        }
        let ch = |i: i32| input[i as usize];
        while status == REGEXP_OK
            && state.is_some()
            && (ch(index) != 0 || self.st(state.unwrap()).typ != FINAL_STATE)
        {
            let sid = state.unwrap();
            let mut len: i32 = 1;
            let mut do_rollback = false;
            if ch(index) == 0 && !has_counts {
                let s = self.st(sid);
                if (transno as usize) < s.trans.len() {
                    let t = s.trans[transno as usize];
                    if t.to >= 0 {
                        let a = &self.arena[t.atom.unwrap()];
                        if !(a.min == 0 && a.max > 0) {
                            do_rollback = true;
                        }
                    }
                } else {
                    do_rollback = true;
                }
            }
            let mut progressed = false;
            if !do_rollback {
                let ntrans = self.st(sid).trans.len() as i32;
                while transno < ntrans {
                    let t = self.st(sid).trans[transno as usize];
                    if t.to < 0 {
                        transno += 1;
                        continue;
                    }
                    let mut ret: i32 = 0;
                    let mut deter = true;
                    if t.count >= 0 {
                        if !has_counts {
                            status = REGEXP_INTERNAL_ERROR;
                            break;
                        }
                        let count = counts[t.count as usize];
                        let c = self.counters[t.count as usize];
                        ret = (count >= c.min && count <= c.max) as i32;
                        if ret == 1 && c.min != c.max {
                            deter = false;
                        }
                    } else if t.atom.is_none() {
                        status = REGEXP_INTERNAL_ERROR;
                        break;
                    } else if ch(index) != 0 {
                        let atom = &self.arena[t.atom.unwrap()];
                        let codepoint = ch(index);
                        len = 1;
                        ret = check_character(atom, codepoint);
                        if ret == 1 && atom.min >= 0 && atom.max > 0 {
                            let to = t.to as usize;
                            if t.counter >= 0 {
                                let c = self.counters[t.counter as usize];
                                if counts[t.counter as usize] >= c.max {
                                    transno += 1;
                                    continue;
                                }
                            }
                            if ntrans > transno + 1 {
                                save!();
                            }
                            if t.counter >= 0 {
                                counts[t.counter as usize] += 1;
                            }
                            let mut transcount = 1;
                            loop {
                                if transcount == atom.max {
                                    break;
                                }
                                index += len;
                                if ch(index) == 0 {
                                    index -= len;
                                    break;
                                }
                                if transcount >= atom.min {
                                    let save_transno = transno;
                                    let save_state = state;
                                    transno = -1;
                                    state = Some(to);
                                    save!();
                                    transno = save_transno;
                                    state = save_state;
                                }
                                ret = check_character(atom, ch(index));
                                transcount += 1;
                                if ret != 1 {
                                    break;
                                }
                            }
                            if transcount < atom.min {
                                ret = 0;
                            }
                            if ret < 0 {
                                ret = 0;
                            }
                            if ret == 0 {
                                do_rollback = true;
                                break;
                            }
                            if t.counter >= 0 {
                                counts[t.counter as usize] -= 1;
                            }
                        } else if ret == 0 && atom.min == 0 && atom.max > 0 {
                            len = 0;
                            ret = 1;
                        }
                    } else {
                        let atom = &self.arena[t.atom.unwrap()];
                        if atom.min == 0 && atom.max > 0 {
                            len = 0;
                            ret = 1;
                        }
                    }
                    if ret == 1 {
                        if t.nd == 1 || (t.count >= 0 && !deter && ntrans > transno + 1) {
                            save!();
                        }
                        if t.counter >= 0 {
                            let c = self.counters[t.counter as usize];
                            if counts[t.counter as usize] >= c.max {
                                transno += 1;
                                continue;
                            }
                            counts[t.counter as usize] += 1;
                        }
                        if t.count >= 0 && t.count < REGEXP_ALL_COUNTER {
                            counts[t.count as usize] = 0;
                        }
                        state = Some(t.to as usize);
                        transno = 0;
                        if t.atom.is_some() {
                            index += len;
                        }
                        progressed = true;
                        break;
                    } else if ret < 0 {
                        status = REGEXP_INTERNAL_ERROR;
                        break;
                    }
                    transno += 1;
                }
                if status != REGEXP_OK {
                    break;
                }
                if progressed {
                    continue;
                }
                if !do_rollback && (transno != 0 || self.st(sid).trans.is_empty()) {
                    do_rollback = true;
                }
            }
            if do_rollback {
                // xmlFARegExecRollBack
                match rollbacks.pop() {
                    None => status = REGEXP_NOT_FOUND,
                    Some(r) => {
                        state = r.state;
                        index = r.index;
                        transno = r.nextbranch;
                        if has_counts {
                            counts = r.counts;
                        }
                    }
                }
            }
        }
        if state.is_none() {
            return REGEXP_INTERNAL_ERROR;
        }
        match status {
            REGEXP_OK => 1,
            REGEXP_NOT_FOUND => 0,
            s => s,
        }
    }
}

/// Progressive execution context (xmlRegExecCtxt) for string tokens.
pub struct ExecCtxt {
    pub status: i32,
    comp: std::rc::Rc<Regexp>,
    state: Option<StateId>,
    transno: i32,
    rollbacks: Vec<Rollback>,
    counts: Vec<i32>,
    err_counts: Vec<i32>,
    input_stack: Vec<(Option<String>, Option<usize>)>,
    index: i32,
    err_state_no: i32,
    err_state: Option<StateId>,
    err_string: Option<String>,
    /// (transition data, push data) for each transition taken by the last
    /// push (the libxml2 callback invocations).
    pub callbacks: Vec<(Option<usize>, Option<usize>)>,
}

impl ExecCtxt {
    /// xmlRegNewExecCtxt
    pub fn new(comp: std::rc::Rc<Regexp>) -> ExecCtxt {
        let nbc = comp.counters.len();
        let state = if comp.compact.is_none() {
            Some(0)
        } else {
            None
        };
        ExecCtxt {
            status: REGEXP_OK,
            comp,
            state,
            transno: 0,
            rollbacks: Vec::new(),
            counts: vec![0; nbc],
            err_counts: vec![0; nbc],
            input_stack: Vec::new(),
            index: 0,
            err_state_no: -1,
            err_state: None,
            err_string: None,
            callbacks: Vec::new(),
        }
    }

    fn input_at(&self, i: i32) -> (Option<String>, Option<usize>) {
        if i >= 0 && (i as usize) < self.input_stack.len() {
            self.input_stack[i as usize].clone()
        } else {
            (None, None)
        }
    }

    fn save(&mut self) {
        let counts = if self.comp.counters.is_empty() {
            Vec::new()
        } else {
            self.counts.clone()
        };
        self.rollbacks.push(Rollback {
            state: self.state,
            index: self.index,
            nextbranch: self.transno + 1,
            counts,
        });
    }

    fn rollback(&mut self) {
        if self.status != REGEXP_OK {
            return;
        }
        match self.rollbacks.pop() {
            None => self.status = REGEXP_NOT_FOUND,
            Some(r) => {
                self.state = r.state;
                self.index = r.index;
                self.transno = r.nextbranch;
                if !self.comp.counters.is_empty() {
                    self.counts = r.counts;
                }
            }
        }
    }

    fn compact_push(&mut self, value: Option<&str>, data: Option<usize>) -> i32 {
        let comp = self.comp.clone();
        let compact = comp.compact.as_ref().unwrap();
        let nbs = comp.string_map.len();
        let state = self.index as usize;
        let value = match value {
            None => {
                return (compact[state * (nbs + 1)] == FINAL_STATE) as i32;
            }
            Some(v) => v,
        };
        for i in 0..nbs {
            let target = compact[state * (nbs + 1) + i + 1];
            if target > 0 && target <= comp.nbstates {
                let target = (target - 1) as usize;
                if str_equal_wildcard(Some(&comp.string_map[i]), Some(value)) {
                    self.index = target as i32;
                    if let Some(td) = comp.transdata.as_ref() {
                        self.callbacks.push((td[state * nbs + i], data));
                    }
                    if compact[target * (nbs + 1)] == SINK_STATE {
                        break;
                    }
                    if compact[target * (nbs + 1)] == FINAL_STATE {
                        return 1;
                    }
                    return 0;
                }
            }
        }
        self.err_state_no = state as i32;
        self.status = REGEXP_NOT_FOUND;
        self.err_string = Some(value.to_string());
        self.status
    }

    /// xmlRegExecPushString (value None signals the end of input).
    pub fn push_string(&mut self, value: Option<&str>, data: Option<usize>) -> i32 {
        self.callbacks.clear();
        self.push_internal(value, data, false)
    }

    /// xmlRegExecPushString2
    pub fn push_string2(&mut self, value: &str, value2: Option<&str>, data: Option<usize>) -> i32 {
        self.callbacks.clear();
        if self.status != REGEXP_OK {
            return self.status;
        }
        match value2 {
            None => self.push_internal(Some(value), data, false),
            Some(v2) => {
                let s = format!("{}|{}", value, v2);
                if self.comp.compact.is_some() {
                    self.compact_push(Some(&s), data)
                } else {
                    self.push_internal(Some(&s), data, true)
                }
            }
        }
    }

    fn push_internal(&mut self, value: Option<&str>, data: Option<usize>, compound: bool) -> i32 {
        if self.status != REGEXP_OK {
            return self.status;
        }
        if self.comp.compact.is_some() {
            return self.compact_push(value, data);
        }
        let comp = self.comp.clone();
        let mut value: Option<String> = value.map(|s| s.to_string());
        let mut data = data;
        let mut is_final = false;
        let mut progress = true;
        if value.is_none() {
            if comp.st(self.state.unwrap()).typ == FINAL_STATE {
                return 1;
            }
            is_final = true;
        }
        if value.is_some() && !self.input_stack.is_empty() {
            self.input_stack.push((value.clone(), data));
            let (v, d) = self.input_at(self.index);
            value = v;
            data = d;
        }
        let has_counts = !comp.counters.is_empty();
        while self.status == REGEXP_OK
            && (value.is_some() || (is_final && comp.st(self.state.unwrap()).typ != FINAL_STATE))
        {
            let sid = self.state.unwrap();
            let mut do_rollback = false;
            let mut progressed = false;
            if value.is_none() && !has_counts {
                do_rollback = true;
            } else {
                let ntrans = comp.st(sid).trans.len() as i32;
                while self.transno < ntrans {
                    let t = comp.st(sid).trans[self.transno as usize];
                    if t.to < 0 {
                        self.transno += 1;
                        continue;
                    }
                    let mut ret: i32 = 0;
                    if t.count == REGEXP_ALL_LAX_COUNTER {
                        ret = 0;
                        if value.is_none() && is_final {
                            ret = 1;
                        } else if let Some(v) = value.as_deref() {
                            for (i, t2) in comp.st(sid).trans.iter().enumerate() {
                                if t2.counter < 0 || i == self.transno as usize {
                                    continue;
                                }
                                let c = comp.counters[t2.counter as usize];
                                let count = self.counts[t2.counter as usize];
                                let aval = t2.atom.and_then(|a| comp.arena[a].valuep.as_deref());
                                if count < c.max && t2.atom.is_some() && aval == Some(v) {
                                    ret = 0;
                                    break;
                                }
                                if count >= c.min
                                    && count < c.max
                                    && t2.atom.is_some()
                                    && aval == Some(v)
                                {
                                    ret = 1;
                                    break;
                                }
                            }
                        }
                    } else if t.count == REGEXP_ALL_COUNTER {
                        ret = 1;
                        for (i, t2) in comp.st(sid).trans.iter().enumerate() {
                            if t2.counter < 0 || i == self.transno as usize {
                                continue;
                            }
                            let c = comp.counters[t2.counter as usize];
                            let count = self.counts[t2.counter as usize];
                            if count < c.min || count > c.max {
                                ret = 0;
                                break;
                            }
                        }
                    } else if t.count >= 0 {
                        let count = self.counts[t.count as usize];
                        let c = comp.counters[t.count as usize];
                        ret = (count >= c.min && count <= c.max) as i32;
                    } else if t.atom.is_none() {
                        self.status = REGEXP_INTERNAL_ERROR;
                        break;
                    } else if let Some(v) = value.clone() {
                        let atom = &comp.arena[t.atom.unwrap()];
                        let mut r = str_equal_wildcard(atom.valuep.as_deref(), Some(&v));
                        if atom.neg {
                            r = !r;
                            if !compound {
                                r = false;
                            }
                        }
                        ret = r as i32;
                        if ret == 1 && t.counter >= 0 {
                            let count = self.counts[t.counter as usize];
                            let c = comp.counters[t.counter as usize];
                            if count >= c.max {
                                ret = 0;
                            }
                        }
                        if ret == 1 && atom.min > 0 && atom.max > 0 {
                            let to = t.to as usize;
                            if ntrans > self.transno + 1 {
                                if self.input_stack.is_empty() {
                                    self.input_stack.push((value.clone(), data));
                                }
                                self.save();
                            }
                            let mut transcount = 1;
                            loop {
                                if transcount == atom.max {
                                    break;
                                }
                                self.index += 1;
                                let (nv, nd) = self.input_at(self.index);
                                value = nv;
                                data = nd;
                                if value.is_none() {
                                    self.index -= 1;
                                    break;
                                }
                                if transcount >= atom.min {
                                    let save_transno = self.transno;
                                    let save_state = self.state;
                                    self.transno = -1;
                                    self.state = Some(to);
                                    if self.input_stack.is_empty() {
                                        self.input_stack.push((value.clone(), data));
                                    }
                                    self.save();
                                    self.transno = save_transno;
                                    self.state = save_state;
                                }
                                ret = (value.as_deref() == atom.valuep.as_deref()) as i32;
                                transcount += 1;
                                if ret != 1 {
                                    break;
                                }
                            }
                            if transcount < atom.min {
                                ret = 0;
                            }
                            if ret < 0 {
                                ret = 0;
                            }
                            if ret == 0 {
                                do_rollback = true;
                                break;
                            }
                        }
                    }
                    if ret == 1 {
                        if let Some(a) = t.atom {
                            if data.is_some() {
                                self.callbacks.push((comp.arena[a].data, data));
                            }
                        }
                        if ntrans > self.transno + 1 {
                            if self.input_stack.is_empty() {
                                self.input_stack.push((value.clone(), data));
                            }
                            self.save();
                        }
                        if t.counter >= 0 {
                            self.counts[t.counter as usize] += 1;
                        }
                        if t.count >= 0 && t.count < REGEXP_ALL_COUNTER {
                            self.counts[t.count as usize] = 0;
                        }
                        if comp.states[t.to as usize]
                            .as_ref()
                            .map(|s| s.typ == SINK_STATE)
                            .unwrap_or(false)
                        {
                            self.err_string = value.clone();
                            self.err_state = self.state;
                            self.err_counts = self.counts.clone();
                        }
                        self.state = Some(t.to as usize);
                        self.transno = 0;
                        if t.atom.is_some() {
                            if !self.input_stack.is_empty() {
                                self.index += 1;
                                if (self.index as usize) < self.input_stack.len() {
                                    let (nv, nd) = self.input_at(self.index);
                                    value = nv;
                                    data = nd;
                                } else {
                                    value = None;
                                    data = None;
                                }
                            } else {
                                value = None;
                                data = None;
                            }
                        }
                        progressed = true;
                        break;
                    } else if ret < 0 {
                        self.status = REGEXP_INTERNAL_ERROR;
                        break;
                    }
                    self.transno += 1;
                }
                if self.status != REGEXP_OK {
                    break;
                }
                if progressed {
                    progress = true;
                    continue;
                }
                if !do_rollback && (self.transno != 0 || comp.st(sid).trans.is_empty()) {
                    do_rollback = true;
                }
            }
            if do_rollback {
                if progress
                    && self.state.is_some()
                    && comp.st(self.state.unwrap()).typ != SINK_STATE
                {
                    progress = false;
                    self.err_string = value.clone();
                    self.err_state = self.state;
                    if has_counts {
                        self.err_counts = self.counts.clone();
                    }
                }
                self.rollback();
                if !self.input_stack.is_empty() && self.status == REGEXP_OK {
                    let (nv, nd) = self.input_at(self.index);
                    value = nv;
                    data = nd;
                }
            }
        }
        if self.status == REGEXP_OK {
            return (comp.st(self.state.unwrap()).typ == FINAL_STATE) as i32;
        }
        self.status
    }

    /// xmlRegExecGetValues: (values, nbval, nbneg, terminal)
    fn get_values(&self, err: bool, maxval: usize) -> Option<(Vec<String>, usize, usize, bool)> {
        let comp = &self.comp;
        let mut values = Vec::new();
        let mut nbval = 0;
        let mut nbneg = 0;
        let terminal;
        if let Some(compact) = comp.compact.as_ref() {
            let nbs = comp.string_map.len();
            let state = if err {
                if self.err_state_no == -1 {
                    return None;
                }
                self.err_state_no as usize
            } else {
                self.index as usize
            };
            terminal = compact[state * (nbs + 1)] == FINAL_STATE;
            for i in 0..nbs {
                if values.len() >= maxval {
                    break;
                }
                let target = compact[state * (nbs + 1) + i + 1];
                if target > 0
                    && target <= comp.nbstates
                    && compact[(target as usize - 1) * (nbs + 1)] != SINK_STATE
                {
                    values.push(comp.string_map[i].clone());
                    nbval += 1;
                }
            }
            for i in 0..nbs {
                if values.len() >= maxval {
                    break;
                }
                let target = compact[state * (nbs + 1) + i + 1];
                if target > 0
                    && target <= comp.nbstates
                    && compact[(target as usize - 1) * (nbs + 1)] == SINK_STATE
                {
                    values.push(comp.string_map[i].clone());
                    nbneg += 1;
                }
            }
        } else {
            terminal = self
                .state
                .map(|s| comp.st(s).typ == FINAL_STATE)
                .unwrap_or(false);
            let state = if err { self.err_state? } else { self.state? };
            let st = comp.st(state);
            for t in &st.trans {
                if values.len() >= maxval {
                    break;
                }
                if t.to < 0 {
                    continue;
                }
                let atom = match t.atom {
                    Some(a) => &comp.arena[a],
                    None => continue,
                };
                if atom.valuep.is_none() {
                    continue;
                }
                let v = if atom.neg {
                    atom.valuep2.clone()
                } else {
                    atom.valuep.clone()
                };
                if t.count == REGEXP_ALL_LAX_COUNTER || t.count == REGEXP_ALL_COUNTER {
                } else if t.counter >= 0 {
                    let count = if err {
                        self.err_counts[t.counter as usize]
                    } else {
                        self.counts[t.counter as usize]
                    };
                    let c = comp.counters[t.counter as usize];
                    if count < c.max {
                        values.push(v.unwrap_or_default());
                        nbval += 1;
                    }
                } else if comp.states[t.to as usize]
                    .as_ref()
                    .map(|s| s.typ != SINK_STATE)
                    .unwrap_or(false)
                {
                    values.push(v.unwrap_or_default());
                    nbval += 1;
                }
            }
            for t in &st.trans {
                if values.len() >= maxval {
                    break;
                }
                if t.to < 0 {
                    continue;
                }
                let atom = match t.atom {
                    Some(a) => &comp.arena[a],
                    None => continue,
                };
                if atom.valuep.is_none() {
                    continue;
                }
                if t.count == REGEXP_ALL_LAX_COUNTER
                    || t.count == REGEXP_ALL_COUNTER
                    || t.counter >= 0
                {
                    continue;
                }
                if comp.states[t.to as usize]
                    .as_ref()
                    .map(|s| s.typ == SINK_STATE)
                    .unwrap_or(false)
                {
                    let v = if atom.neg {
                        atom.valuep2.clone()
                    } else {
                        atom.valuep.clone()
                    };
                    values.push(v.unwrap_or_default());
                    nbneg += 1;
                }
            }
        }
        Some((values, nbval, nbneg, terminal))
    }

    /// xmlRegExecNextValues
    pub fn next_values(&self, maxval: usize) -> Option<(Vec<String>, usize, usize, bool)> {
        self.get_values(false, maxval)
    }

    /// xmlRegExecErrInfo: (error string, values, nbval, nbneg, terminal)
    pub fn err_info(
        &self,
        maxval: usize,
    ) -> (Option<String>, Option<(Vec<String>, usize, usize, bool)>) {
        let s = if self.status != REGEXP_OK {
            self.err_string.clone()
        } else {
            None
        };
        (s, self.get_values(true, maxval))
    }

    #[allow(dead_code)]
    fn unused(&self) -> (i32, usize) {
        (self.comp.flags, self.comp.nb_atoms)
    }
}

#[allow(dead_code)]
const _UNUSED: [i32; 2] = [QUANT_EPSILON, QUANT_ALL];

#[cfg(test)]
mod tests {
    use super::*;

    fn m(re: &str, s: &str) -> i32 {
        regexp_compile(re).map(|r| r.exec(s)).unwrap_or(-100)
    }

    #[test]
    fn patterns() {
        assert_eq!(m("[a-z]+", "abc"), 1);
        assert_eq!(m("[a-z]+", "abC"), 0);
        assert_eq!(m("[a-z]+", ""), 0);
        assert_eq!(m("\\d{3}-\\d{4}", "555-1234"), 1);
        assert_eq!(m("\\d{3}-\\d{4}", "55-1234"), 0);
        assert_eq!(m("(ab|cd)*e", "abcdabe"), 1);
        assert_eq!(m("(ab|cd)*e", "e"), 1);
        assert_eq!(m("(ab|cd)*e", "abce"), 0);
        assert_eq!(m("a{2,3}", "a"), 0);
        assert_eq!(m("a{2,3}", "aa"), 1);
        assert_eq!(m("a{2,3}", "aaa"), 1);
        assert_eq!(m("a{2,3}", "aaaa"), 0);
        assert_eq!(m("[^abc]", "d"), 1);
        assert_eq!(m("[^abc]", "a"), 0);
        assert_eq!(m("\\p{Lu}+", "ABC"), 1);
        assert_eq!(m("\\p{Lu}+", "AbC"), 0);
        assert_eq!(m("[a-z-[aeiou]]+", "bcd"), 1);
        assert_eq!(m("[a-z-[aeiou]]+", "bad"), 0);
        assert_eq!(m("(a|b)?c", "c"), 1);
        assert_eq!(m("(a|b)?c", "bc"), 1);
        assert_eq!(m("(a|b){0,2}c", "abc"), 1);
        assert_eq!(m("(a|b){0,2}c", "abac"), 0);
        assert_eq!(m("[A-Z]{2}\\d*", "AB12"), 1);
        assert_eq!(m(".*", "anything"), 1);
        assert_eq!(m("\\i\\c*", "foo-bar"), 1);
        assert_eq!(m("\\i\\c*", "1foo"), 0);
        assert!(regexp_compile("[a-").is_err());
        assert!(regexp_compile("(a").is_err());
    }

    #[test]
    fn automata() {
        // sequence a, b?, c
        let mut am = Automata::new();
        let s0 = am.init_state();
        let s1 = am.new_transition(s0, None, "a", Some(1)).unwrap();
        let s2 = am.new_transition(s1, None, "b", Some(2)).unwrap();
        am.new_epsilon(s1, Some(s2));
        let s3 = am.new_transition(s2, None, "c", Some(3)).unwrap();
        am.set_final_state(s3);
        let re = std::rc::Rc::new(am.compile().unwrap());
        let mut ex = ExecCtxt::new(re.clone());
        assert_eq!(ex.push_string(Some("a"), Some(10)), 0);
        assert_eq!(ex.push_string(Some("c"), Some(11)), 1);
        assert_eq!(ex.push_string(None, None), 1);
        let mut ex = ExecCtxt::new(re.clone());
        assert_eq!(ex.push_string(Some("a"), Some(10)), 0);
        assert!(ex.push_string(Some("d"), Some(11)) < 0);
        let (s, v) = ex.err_info(10);
        assert_eq!(s.as_deref(), Some("d"));
        assert_eq!(v.unwrap().0, vec!["b".to_string(), "c".to_string()]);
    }
}
