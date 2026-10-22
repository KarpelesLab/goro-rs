//! DTD validation: a port of libxml2 2.13 valid.c (regexp build), with the
//! content models compiled to automata the way xmlregexp.c does it (same
//! epsilon elimination and determinism check, so "not deterministic"
//! diagnostics match).
//!
//! Entry points:
//! - [`validate_document`]: DOMDocument::validate() (xmlValidateDocument).
//! - [`Validator`] + [`Cx`]: incremental validation used by the tree
//!   builder when parsing with XML_PARSE_DTDVALID (SAX2.c hooks) and the
//!   streaming push API ([`Cx::push_element`], [`Cx::push_cdata`],
//!   [`Cx::pop_element`]) used by readers.

use crate::parser::*;
use crate::tree::{AttrDecl, EntityType, NType, NodeId, NsId, Tree};
use std::rc::Rc;

pub const XML_DTD_ATTRIBUTE_DEFAULT: i32 = 500;
pub const XML_DTD_ATTRIBUTE_REDEFINED: i32 = 501;
pub const XML_DTD_ATTRIBUTE_VALUE: i32 = 502;
pub const XML_DTD_CONTENT_ERROR: i32 = 503;
pub const XML_DTD_CONTENT_MODEL: i32 = 504;
pub const XML_DTD_CONTENT_NOT_DETERMINIST: i32 = 505;
pub const XML_DTD_DIFFERENT_PREFIX: i32 = 506;
pub const XML_DTD_ELEM_DEFAULT_NAMESPACE: i32 = 507;
pub const XML_DTD_ELEM_NAMESPACE: i32 = 508;
pub const XML_DTD_ELEM_REDEFINED: i32 = 509;
pub const XML_DTD_EMPTY_NOTATION: i32 = 510;
pub const XML_DTD_ENTITY_TYPE: i32 = 511;
pub const XML_DTD_ID_FIXED: i32 = 512;
pub const XML_DTD_ID_REDEFINED: i32 = 513;
pub const XML_DTD_ID_SUBSET: i32 = 514;
pub const XML_DTD_INVALID_CHILD: i32 = 515;
pub const XML_DTD_INVALID_DEFAULT: i32 = 516;
pub const XML_DTD_LOAD_ERROR: i32 = 517;
pub const XML_DTD_MISSING_ATTRIBUTE: i32 = 518;
pub const XML_DTD_MIXED_CORRUPT: i32 = 519;
pub const XML_DTD_MULTIPLE_ID: i32 = 520;
pub const XML_DTD_NO_DOC: i32 = 521;
pub const XML_DTD_NO_DTD: i32 = 522;
pub const XML_DTD_NO_PREFIX: i32 = 524;
pub const XML_DTD_NO_ROOT: i32 = 525;
pub const XML_DTD_NOTATION_REDEFINED: i32 = 526;
pub const XML_DTD_NOTATION_VALUE: i32 = 527;
pub const XML_DTD_NOT_EMPTY: i32 = 528;
pub const XML_DTD_NOT_PCDATA: i32 = 529;
pub const XML_DTD_NOT_STANDALONE: i32 = 530;
pub const XML_DTD_ROOT_NAME: i32 = 531;
pub const XML_DTD_STANDALONE_WHITE_SPACE: i32 = 532;
pub const XML_DTD_UNKNOWN_ATTRIBUTE: i32 = 533;
pub const XML_DTD_UNKNOWN_ELEM: i32 = 534;
pub const XML_DTD_UNKNOWN_ENTITY_V: i32 = 535;
pub const XML_DTD_UNKNOWN_ID: i32 = 536;
pub const XML_DTD_UNKNOWN_NOTATION: i32 = 537;
pub const XML_DTD_STANDALONE_DEFAULTED: i32 = 538;
pub const XML_DTD_XMLID_VALUE: i32 = 539;
pub const XML_DTD_XMLID_TYPE: i32 = 540;
pub const XML_DTD_DUP_TOKEN: i32 = 541;

// attribute types / defaults (xmlAttributeType / xmlAttributeDefault)
pub const AT_CDATA: u8 = 1;
pub const AT_ID: u8 = 2;
pub const AT_IDREF: u8 = 3;
pub const AT_IDREFS: u8 = 4;
pub const AT_ENTITY: u8 = 5;
pub const AT_ENTITIES: u8 = 6;
pub const AT_NMTOKEN: u8 = 7;
pub const AT_NMTOKENS: u8 = 8;
pub const AT_ENUMERATION: u8 = 9;
pub const AT_NOTATION: u8 = 10;
pub const AD_NONE: u8 = 1;
pub const AD_REQUIRED: u8 = 2;
pub const AD_IMPLIED: u8 = 3;
pub const AD_FIXED: u8 = 4;
// element types
pub const ET_UNDEFINED: u8 = 0;
pub const ET_EMPTY: u8 = 1;
pub const ET_ANY: u8 = 2;
pub const ET_MIXED: u8 = 3;
pub const ET_ELEMENT: u8 = 4;

// ---------------------------------------------------------------------------
// Content model automata (xmlregexp.c subset: string atoms, no counters)

const ST_START: u8 = 1;
const ST_FINAL: u8 = 2;
const ST_TRANS: u8 = 3;
const ST_SINK: u8 = 4;
const ST_UNREACH: u8 = 5;

const MARK_NORMAL: u8 = 0;
const MARK_START: u8 = 1;
const MARK_VISITED: u8 = 2;

#[derive(Clone, Debug)]
struct RTrans {
    /// Atom index (None: epsilon).
    atom: Option<usize>,
    /// Target state; -1 eliminated, -2 epsilon being reduced.
    to: i64,
}

#[derive(Clone, Debug)]
struct RState {
    typ: u8,
    trans: Vec<RTrans>,
    trans_to: Vec<usize>,
    mark: u8,
    reached: u8,
}

/// A compiled element content model (elem->contModel).
#[derive(Clone, Debug)]
pub struct ContModel {
    atoms: Vec<String>,
    states: Vec<Option<RState>>,
    pub determinist: bool,
}

struct Builder {
    atoms: Vec<String>,
    states: Vec<Option<RState>>,
    state: usize,
}

impl Builder {
    fn new_state(&mut self) -> usize {
        self.states.push(Some(RState {
            typ: ST_TRANS,
            trans: Vec::new(),
            trans_to: Vec::new(),
            mark: MARK_NORMAL,
            reached: MARK_NORMAL,
        }));
        self.states.len() - 1
    }

    fn st(&mut self, n: usize) -> &mut RState {
        self.states[n].as_mut().unwrap()
    }

    /// xmlRegStateAddTrans
    fn add_trans(&mut self, from: usize, atom: Option<usize>, to: usize) {
        if self.states[from].is_none() || self.states[to].is_none() {
            return;
        }
        if self
            .st(from)
            .trans
            .iter()
            .any(|t| t.atom == atom && t.to == to as i64)
        {
            return;
        }
        self.st(from).trans.push(RTrans {
            atom,
            to: to as i64,
        });
        self.st(to).trans_to.push(from);
    }

    fn new_transition(&mut self, from: usize, to: Option<usize>, name: &str) -> usize {
        self.atoms.push(name.to_string());
        let atom = self.atoms.len() - 1;
        let to = match to {
            Some(t) => t,
            None => self.new_state(),
        };
        self.add_trans(from, Some(atom), to);
        to
    }

    fn new_epsilon(&mut self, from: usize, to: Option<usize>) -> usize {
        let to = match to {
            Some(t) => t,
            None => self.new_state(),
        };
        self.add_trans(from, None, to);
        to
    }

    /// xmlValidBuildAContentModel
    fn build(&mut self, ec: &ElemContent, n: usize) -> bool {
        let c = &ec.nodes[n];
        match c.ctype {
            CT_PCDATA => false,
            CT_ELEMENT => {
                let fullname = match &c.prefix {
                    Some(p) => format!("{}:{}", p, c.name),
                    None => c.name.clone(),
                };
                let oldstate = self.state;
                match c.ocur {
                    OC_OPT => {
                        self.state = self.new_transition(self.state, None, &fullname);
                        self.new_epsilon(oldstate, Some(self.state));
                    }
                    OC_PLUS => {
                        self.state = self.new_transition(self.state, None, &fullname);
                        self.new_transition(self.state, Some(self.state), &fullname);
                    }
                    OC_MULT => {
                        self.state = self.new_epsilon(self.state, None);
                        self.new_transition(self.state, Some(self.state), &fullname);
                    }
                    _ => {
                        self.state = self.new_transition(self.state, None, &fullname);
                    }
                }
                true
            }
            CT_SEQ => {
                let mut oldstate = self.state;
                let ocur = c.ocur;
                if ocur != OC_ONCE {
                    self.state = self.new_epsilon(oldstate, None);
                    oldstate = self.state;
                }
                let mut cur = n;
                loop {
                    let c1 = match ec.nodes[cur].c1 {
                        Some(x) => x,
                        None => return false,
                    };
                    if !self.build(ec, c1) {
                        return false;
                    }
                    cur = match ec.nodes[cur].c2 {
                        Some(x) => x,
                        None => return false,
                    };
                    if !(ec.nodes[cur].ctype == CT_SEQ && ec.nodes[cur].ocur == OC_ONCE) {
                        break;
                    }
                }
                if !self.build(ec, cur) {
                    return false;
                }
                let oldend = self.state;
                self.state = self.new_epsilon(oldend, None);
                match ocur {
                    OC_OPT => {
                        self.new_epsilon(oldstate, Some(self.state));
                    }
                    OC_MULT => {
                        self.new_epsilon(oldstate, Some(self.state));
                        self.new_epsilon(oldend, Some(oldstate));
                    }
                    OC_PLUS => {
                        self.new_epsilon(oldend, Some(oldstate));
                    }
                    _ => {}
                }
                true
            }
            CT_OR => {
                let ocur = c.ocur;
                if ocur == OC_PLUS || ocur == OC_MULT {
                    self.state = self.new_epsilon(self.state, None);
                }
                let oldstate = self.state;
                let oldend = self.new_state();
                let mut cur = n;
                loop {
                    self.state = oldstate;
                    let c1 = match ec.nodes[cur].c1 {
                        Some(x) => x,
                        None => return false,
                    };
                    if !self.build(ec, c1) {
                        return false;
                    }
                    self.new_epsilon(self.state, Some(oldend));
                    cur = match ec.nodes[cur].c2 {
                        Some(x) => x,
                        None => return false,
                    };
                    if !(ec.nodes[cur].ctype == CT_OR && ec.nodes[cur].ocur == OC_ONCE) {
                        break;
                    }
                }
                self.state = oldstate;
                if !self.build(ec, cur) {
                    return false;
                }
                self.new_epsilon(self.state, Some(oldend));
                self.state = self.new_epsilon(oldend, None);
                match ocur {
                    OC_OPT => {
                        self.new_epsilon(oldstate, Some(self.state));
                    }
                    OC_MULT => {
                        self.new_epsilon(oldstate, Some(self.state));
                        self.new_epsilon(oldend, Some(oldstate));
                    }
                    OC_PLUS => {
                        self.new_epsilon(oldend, Some(oldstate));
                    }
                    _ => {}
                }
                true
            }
            _ => false,
        }
    }

    /// xmlFAReduceEpsilonTransitions
    fn reduce_epsilon(&mut self, fromnr: usize, tonr: usize) {
        if self.states[fromnr].is_none() || self.states[tonr].is_none() {
            return;
        }
        let m = self.st(tonr).mark;
        if m == MARK_START || m == MARK_VISITED {
            return;
        }
        self.st(tonr).mark = MARK_VISITED;
        if self.st(tonr).typ == ST_FINAL {
            self.st(fromnr).typ = ST_FINAL;
        }
        let mut k = 0;
        while k < self.st(tonr).trans.len() {
            let t1 = self.st(tonr).trans[k].clone();
            k += 1;
            if t1.to < 0 {
                continue;
            }
            match t1.atom {
                None => {
                    if t1.to as usize != fromnr {
                        self.reduce_epsilon(fromnr, t1.to as usize);
                    }
                }
                Some(_) => self.add_trans(fromnr, t1.atom, t1.to as usize),
            }
        }
    }

    /// xmlFAFinishReduceEpsilonTransitions
    fn finish_reduce(&mut self, tonr: usize) {
        if self.states[tonr].is_none() {
            return;
        }
        let m = self.st(tonr).mark;
        if m == MARK_START || m == MARK_NORMAL {
            return;
        }
        self.st(tonr).mark = MARK_NORMAL;
        let ts: Vec<RTrans> = self.st(tonr).trans.clone();
        for t in ts {
            if t.to >= 0 && t.atom.is_none() {
                self.finish_reduce(t.to as usize);
            }
        }
    }

    /// xmlFAEliminateSimpleEpsilonTransitions
    fn eliminate_simple(&mut self) {
        for statenr in 0..self.states.len() {
            let (ok, newto, typ) = match &self.states[statenr] {
                None => continue,
                Some(s) => {
                    if s.trans.len() != 1 || s.typ == ST_UNREACH || s.typ == ST_FINAL {
                        continue;
                    }
                    let t = &s.trans[0];
                    (
                        t.atom.is_none() && t.to >= 0 && t.to as usize != statenr,
                        t.to,
                        s.typ,
                    )
                }
            };
            if !ok || typ == ST_START {
                continue;
            }
            let newto = newto as usize;
            let tt = self.st(statenr).trans_to.clone();
            for from in tt {
                if self.states[from].is_none() {
                    continue;
                }
                let mut j = 0;
                while j < self.st(from).trans.len() {
                    if self.st(from).trans[j].to == statenr as i64 {
                        self.st(from).trans[j].to = -1;
                        let a = self.st(from).trans[j].atom;
                        self.add_trans(from, a, newto);
                    }
                    j += 1;
                }
            }
            self.st(statenr).trans.clear();
            self.st(statenr).typ = ST_UNREACH;
        }
    }

    /// xmlFAEliminateEpsilonTransitions
    fn eliminate(&mut self) {
        self.eliminate_simple();
        for s in self.states.iter_mut() {
            if s.as_ref().map(|x| x.typ == ST_UNREACH).unwrap_or(false) {
                *s = None;
            }
        }
        let mut has_epsilon = false;
        for statenr in (0..self.states.len()).rev() {
            if self.states[statenr].is_none() {
                continue;
            }
            if self.st(statenr).trans.is_empty() && self.st(statenr).typ != ST_FINAL {
                self.st(statenr).typ = ST_SINK;
            }
            let mut k = 0;
            while k < self.st(statenr).trans.len() {
                let t = self.st(statenr).trans[k].clone();
                if t.atom.is_none() && t.to >= 0 {
                    if t.to as usize == statenr {
                        self.st(statenr).trans[k].to = -1;
                    } else {
                        let newto = t.to as usize;
                        has_epsilon = true;
                        self.st(statenr).trans[k].to = -2;
                        self.st(statenr).mark = MARK_START;
                        self.reduce_epsilon(statenr, newto);
                        self.finish_reduce(newto);
                        self.st(statenr).mark = MARK_NORMAL;
                    }
                }
                k += 1;
            }
        }
        if has_epsilon {
            for s in self.states.iter_mut().flatten() {
                for t in s.trans.iter_mut() {
                    if t.atom.is_none() && t.to >= 0 {
                        t.to = -1;
                    }
                }
            }
        }
        // reachability
        for s in self.states.iter_mut().flatten() {
            s.reached = MARK_NORMAL;
        }
        let mut cur = if self.states.first().map(|s| s.is_some()).unwrap_or(false) {
            Some(0)
        } else {
            None
        };
        if let Some(c) = cur {
            self.st(c).reached = MARK_START;
        }
        while let Some(c) = cur {
            self.st(c).reached = MARK_VISITED;
            let mut target = None;
            let ts = self.st(c).trans.clone();
            for t in ts {
                if t.to >= 0 && t.atom.is_some() {
                    let nt = t.to as usize;
                    if self.states[nt].is_none() {
                        continue;
                    }
                    if self.st(nt).reached == MARK_NORMAL {
                        self.st(nt).reached = MARK_START;
                        target = Some(nt);
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
            cur = target;
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

    /// xmlFAComputesDeterminism (string atoms only)
    fn determinism(&mut self) -> bool {
        let atoms = self.atoms.clone();
        let eq = |a: Option<usize>, b: Option<usize>| -> bool {
            match (a, b) {
                (Some(x), Some(y)) => x == y || atoms[x] == atoms[y],
                _ => false,
            }
        };
        for s in self.states.iter_mut().flatten() {
            if s.trans.len() < 2 {
                continue;
            }
            for k in 0..s.trans.len() {
                if s.trans[k].atom.is_none() || s.trans[k].to < 0 {
                    continue;
                }
                for i in 0..k {
                    if s.trans[i].to < 0 {
                        continue;
                    }
                    if s.trans[i].atom.is_some()
                        && s.trans[k].to == s.trans[i].to
                        && eq(s.trans[k].atom, s.trans[i].atom)
                    {
                        s.trans[i].to = -1;
                    }
                }
            }
        }
        let mut ret = true;
        for s in self.states.iter().flatten() {
            if s.trans.len() < 2 {
                continue;
            }
            for k in 0..s.trans.len() {
                if s.trans[k].atom.is_none() || s.trans[k].to < 0 {
                    continue;
                }
                for i in 0..k {
                    if s.trans[i].to < 0 {
                        continue;
                    }
                    if s.trans[i].atom.is_some() && eq(s.trans[k].atom, s.trans[i].atom) {
                        ret = false;
                    }
                }
            }
        }
        ret
    }
}

impl ContModel {
    /// xmlValidBuildContentModel: None if the model can't be built.
    pub fn build(ec: &ElemContent) -> Option<ContModel> {
        let mut b = Builder {
            atoms: Vec::new(),
            states: Vec::new(),
            state: 0,
        };
        let s0 = b.new_state();
        b.st(s0).typ = ST_START;
        b.state = s0;
        if !b.build(ec, ec.root) {
            return None;
        }
        let fin = b.state;
        b.st(fin).typ = ST_FINAL;
        b.eliminate();
        let determinist = b.determinism();
        Some(ContModel {
            atoms: b.atoms,
            states: b.states,
            determinist,
        })
    }

    fn is_final(&self, s: usize) -> bool {
        self.states
            .get(s)
            .and_then(|x| x.as_ref())
            .map(|x| x.typ == ST_FINAL)
            .unwrap_or(false)
    }
}

/// xmlRegExecCtxt on a (deterministic, compact) content model.
#[derive(Clone, Debug)]
pub struct Exec {
    model: Rc<ContModel>,
    state: usize,
    status: i32,
}

impl Exec {
    pub fn new(model: Rc<ContModel>) -> Exec {
        Exec {
            model,
            state: 0,
            status: 0,
        }
    }

    /// xmlRegExecPushString: 1 final, 0 not final, negative on error.
    pub fn push(&mut self, value: Option<&str>) -> i32 {
        if self.status != 0 {
            return self.status;
        }
        let m = self.model.clone();
        let st = match m.states.get(self.state).and_then(|s| s.as_ref()) {
            Some(s) => s,
            None => {
                self.status = -1;
                return -1;
            }
        };
        let v = match value {
            None => return if st.typ == ST_FINAL { 1 } else { 0 },
            Some(v) => v,
        };
        for t in &st.trans {
            if t.to < 0 {
                continue;
            }
            if let Some(a) = t.atom {
                if m.atoms[a] == v {
                    self.state = t.to as usize;
                    let tt = m
                        .states
                        .get(self.state)
                        .and_then(|s| s.as_ref())
                        .map(|s| s.typ)
                        .unwrap_or(ST_SINK);
                    if tt == ST_SINK {
                        break;
                    }
                    return if m.is_final(self.state) { 1 } else { 0 };
                }
            }
        }
        self.status = -1;
        -1
    }
}

// ---------------------------------------------------------------------------
// Value syntax checks

fn is_doc_name_start_char(c: u32) -> bool {
    matches!(c, 0x61..=0x7A | 0x41..=0x5A | 0x5F | 0x3A | 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2FF | 0x370..=0x37D | 0x37F..=0x1FFF | 0x200C..=0x200D | 0x2070..=0x218F | 0x2C00..=0x2FEF | 0x3001..=0xD7FF | 0xF900..=0xFDCF | 0xFDF0..=0xFFFD | 0x10000..=0xEFFFF)
}

fn is_doc_name_char(c: u32) -> bool {
    is_doc_name_start_char(c)
        || matches!(c, 0x30..=0x39 | 0x2D | 0x2E | 0xB7 | 0x300..=0x36F | 0x203F..=0x2040)
}

struct Chars<'a> {
    it: std::str::Chars<'a>,
}

impl Chars<'_> {
    fn next(&mut self) -> u32 {
        self.it.next().map(|c| c as u32).unwrap_or(0)
    }
}

pub fn validate_name_value(v: &str) -> bool {
    let mut c = Chars { it: v.chars() };
    let mut val = c.next();
    if !is_doc_name_start_char(val) {
        return false;
    }
    val = c.next();
    while is_doc_name_char(val) {
        val = c.next();
    }
    val == 0
}

pub fn validate_names_value(v: &str) -> bool {
    let mut c = Chars { it: v.chars() };
    let mut val = c.next();
    if !is_doc_name_start_char(val) {
        return false;
    }
    val = c.next();
    while is_doc_name_char(val) {
        val = c.next();
    }
    while val == 0x20 {
        while val == 0x20 {
            val = c.next();
        }
        if !is_doc_name_start_char(val) {
            return false;
        }
        val = c.next();
        while is_doc_name_char(val) {
            val = c.next();
        }
    }
    val == 0
}

pub fn validate_nmtoken_value(v: &str) -> bool {
    let mut c = Chars { it: v.chars() };
    let mut val = c.next();
    if !is_doc_name_char(val) {
        return false;
    }
    val = c.next();
    while is_doc_name_char(val) {
        val = c.next();
    }
    val == 0
}

pub fn validate_nmtokens_value(v: &str) -> bool {
    let mut c = Chars { it: v.chars() };
    let mut val = c.next();
    while matches!(val, 0x20 | 0x9 | 0xA | 0xD) {
        val = c.next();
    }
    if !is_doc_name_char(val) {
        return false;
    }
    while is_doc_name_char(val) {
        val = c.next();
    }
    while val == 0x20 {
        while val == 0x20 {
            val = c.next();
        }
        if val == 0 {
            return true;
        }
        if !is_doc_name_char(val) {
            return false;
        }
        val = c.next();
        while is_doc_name_char(val) {
            val = c.next();
        }
    }
    val == 0
}

/// xmlValidateAttributeValueInternal
pub fn validate_attribute_value(atype: u8, v: &str) -> bool {
    match atype {
        AT_ENTITIES | AT_IDREFS => validate_names_value(v),
        AT_ENTITY | AT_IDREF | AT_ID | AT_NOTATION => validate_name_value(v),
        AT_NMTOKENS | AT_ENUMERATION => validate_nmtokens_value(v),
        AT_NMTOKEN => validate_nmtoken_value(v),
        _ => true,
    }
}

/// xmlValidNormalizeString
pub fn normalize_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.trim_start_matches(' ');
    let mut pending = false;
    for ch in b.chars() {
        if ch == ' ' {
            pending = true;
        } else {
            if pending {
                out.push(' ');
                pending = false;
            }
            out.push(ch);
        }
    }
    out
}

fn is_blank_ch(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

// ---------------------------------------------------------------------------
// DTD table lookups

fn split_qname(name: &str) -> (Option<String>, String) {
    // xmlSplitQName4: a leading ':' is not a prefix separator
    match name.find(':') {
        Some(i) if i > 0 => (Some(name[..i].to_string()), name[i + 1..].to_string()),
        _ => (None, name.to_string()),
    }
}

pub fn subsets(t: &Tree, doc: NodeId) -> (Option<NodeId>, Option<NodeId>) {
    match t.doc_info(doc) {
        Some(d) => (d.int_subset, d.ext_subset),
        None => (None, None),
    }
}

/// xmlHashLookup2(dtd->elements, name, prefix)
pub fn find_elem(t: &Tree, dtd: Option<NodeId>, name: &str, prefix: Option<&str>) -> Option<usize> {
    let d = t.dtd_info(dtd?)?;
    d.elements
        .iter()
        .position(|e| e.name == name && e.prefix.as_deref() == prefix)
}

/// xmlGetDtdQAttrDesc(dtd, elem, name, prefix): index in attr_defaults
pub fn find_attr(
    t: &Tree,
    dtd: Option<NodeId>,
    elem: &str,
    name: &str,
    prefix: Option<&str>,
) -> Option<usize> {
    t.dtd_info(dtd?)?.find_attr_decl(elem, name, prefix)
}

fn attr_decl(t: &Tree, dtd: NodeId, idx: usize) -> AttrDecl {
    t.dtd_info(dtd)
        .map(|d| d.attr_defaults[idx].clone())
        .unwrap_or_default()
}

fn has_notation(t: &Tree, dtd: Option<NodeId>, name: &str) -> bool {
    match dtd.and_then(|d| t.dtd_info(d)) {
        Some(d) => d.notations.iter().any(|(n, _, _)| n == name),
        None => false,
    }
}

/// xmlGetDocEntity: (found, is unparsed)
fn doc_entity_kind(t: &Tree, doc: NodeId, name: &str) -> Option<bool> {
    let (int, ext) = subsets(t, doc);
    let standalone = t.doc_info(doc).map(|d| d.standalone).unwrap_or(-1);
    let mut subs = vec![int];
    if standalone != 1 {
        subs.push(ext);
    }
    for sub in subs.into_iter().flatten() {
        if let Some(d) = t.dtd_info(sub) {
            for (n, e) in &d.entities {
                if n == name {
                    return Some(
                        t.entity_info(*e)
                            .map(|i| i.etype == EntityType::ExternalGeneralUnparsed)
                            .unwrap_or(false),
                    );
                }
            }
        }
    }
    if matches!(name, "lt" | "gt" | "amp" | "apos" | "quot") {
        return Some(false);
    }
    None
}

fn elem_qname(t: &Tree, el: NodeId) -> Option<String> {
    t.ns_prefix(t.nodes[el].ns)
        .map(|p| format!("{}:{}", p, t.nodes[el].name))
}

fn ns_prefix_of(t: &Tree, ns: NsId) -> Option<String> {
    t.nss[ns].prefix.clone()
}

/// xmlGetLineNo
pub fn get_line_no(t: &Tree, n: NodeId) -> i32 {
    fn rec(t: &Tree, n: NodeId, depth: u32) -> i64 {
        if depth >= 5 {
            return -1;
        }
        let nd = &t.nodes[n];
        let mut result: i64 = -1;
        if matches!(
            nd.t,
            NType::Element | NType::Text | NType::Comment | NType::Pi
        ) {
            if nd.line == 65535 {
                if let Some(c) = nd.children.filter(|_| nd.t == NType::Element) {
                    result = rec(t, c, depth + 1);
                } else if let Some(x) = nd.next {
                    result = rec(t, x, depth + 1);
                } else if let Some(x) = nd.prev {
                    result = rec(t, x, depth + 1);
                }
            }
            if result == -1 || result == 65535 {
                result = nd.line as i64;
            }
        } else if let Some(p) = nd.prev.filter(|p| {
            matches!(
                t.nodes[*p].t,
                NType::Element | NType::Text | NType::Comment | NType::Pi
            )
        }) {
            result = rec(t, p, depth + 1);
        } else if let Some(p) = nd.parent.filter(|p| t.nodes[*p].t == NType::Element) {
            result = rec(t, p, depth + 1);
        }
        result
    }
    rec(t, n, 0) as i32
}

// ---------------------------------------------------------------------------
// Validation context

/// Where diagnostics go.
pub enum Out<'a> {
    /// Out-of-parser validation (DOMDocument::validate): errors are
    /// collected with the node's document URL and line, and formatted as
    /// the bare message (PHP's validity handler).
    Collect(&'a mut Vec<XmlError>),
    /// Validation while parsing (XML_VCTXT_USE_PCTXT): errors go through
    /// the parser context.
    Parser(&'a mut ParserState),
}

/// One open element in push (streaming) mode (xmlValidState).
#[derive(Clone, Debug)]
pub struct VState {
    /// (dtd, index) of the element declaration.
    pub decl: Option<(NodeId, usize)>,
    pub node: NodeId,
    pub exec: Option<Exec>,
}

/// An IDREF/IDREFS reference (xmlRef).
#[derive(Clone, Debug)]
pub struct RefEntry {
    pub value: String,
    pub attr: Option<NodeId>,
    /// Attribute name (streaming mode, where the attribute goes away).
    pub name: Option<String>,
    pub lineno: i32,
}

/// Persistent validation state (the parts of xmlValidCtxt that outlive a
/// single call, plus doc->refs).
#[derive(Clone, Debug, Default)]
pub struct Validator {
    pub refs: Vec<RefEntry>,
    pub vstate: Vec<VState>,
    /// XML_VCTXT_DTD_VALIDATED
    pub dtd_validated: bool,
    /// Streaming (reader) mode: references keep attribute names.
    pub streaming: bool,
}

/// A validation call context.
pub struct Cx<'a, 'b> {
    pub t: &'a mut Tree,
    pub v: &'a mut Validator,
    pub out: Out<'b>,
    pub doc: NodeId,
    /// ctxt->valid
    pub valid: bool,
    no_input: bool,
}

impl<'a, 'b> Cx<'a, 'b> {
    pub fn new(t: &'a mut Tree, v: &'a mut Validator, out: Out<'b>, doc: NodeId) -> Self {
        Cx {
            t,
            v,
            out,
            doc,
            valid: true,
            no_input: false,
        }
    }

    /// xmlDoErrValid
    pub fn err(&mut self, node: Option<NodeId>, code: i32, level: i32, msg: String) {
        // first element ancestor (xmlVUpdateError)
        let mut node = node;
        if let Some(mut n) = node {
            for _ in 0..10 {
                if self.t.nodes[n].t == NType::Element || self.t.nodes[n].parent.is_none() {
                    break;
                }
                n = self.t.nodes[n].parent.unwrap();
            }
            node = Some(n);
        }
        let nref = node.map(|n| {
            let nd = &self.t.nodes[n];
            let url = nd.doc.and_then(|d| self.t.doc_url(d)).or_else(|| {
                if self.t.is_doc(n) {
                    self.t.doc_url(n)
                } else {
                    None
                }
            });
            let mut line = if nd.t == NType::Element {
                nd.line as i32
            } else {
                0
            };
            if line == 0 || line == 65535 {
                line = get_line_no(self.t, n);
            }
            let elem_name = if nd.t == NType::Element {
                Some(nd.name.clone())
            } else {
                None
            };
            NodeRef {
                elem_name,
                url,
                line,
            }
        });
        match &mut self.out {
            Out::Collect(v) => {
                let mut e = XmlError::simple(FROM_VALID, code, level, msg);
                if let Some(nr) = &nref {
                    e.file = nr.url.clone();
                    e.line = nr.line;
                }
                v.push(e);
            }
            Out::Parser(p) => {
                let _ = p.report_node(FROM_VALID, code, level, msg, nref.as_ref(), self.no_input);
            }
        }
    }

    fn err_e(&mut self, node: Option<NodeId>, code: i32, msg: String) {
        self.err(node, code, ERR_ERROR, msg);
    }

    fn has_dtd(&self) -> bool {
        let (i, e) = subsets(self.t, self.doc);
        i.is_some() || e.is_some()
    }

    fn standalone(&self) -> i32 {
        self.t
            .doc_info(self.doc)
            .map(|d| d.standalone)
            .unwrap_or(-1)
    }

    fn elem_decl(&self, at: (NodeId, usize)) -> &crate::tree::ElemDecl {
        &self.t.dtd_info(at.0).unwrap().elements[at.1]
    }

    /// xmlGetDtdQAttrDesc on the internal then the external subset.
    fn lookup_attr(&self, elem: &str, name: &str, prefix: Option<&str>) -> Option<(NodeId, usize)> {
        let (int, ext) = subsets(self.t, self.doc);
        if let Some(i) = find_attr(self.t, int, elem, name, prefix) {
            return Some((int.unwrap(), i));
        }
        if let Some(i) = find_attr(self.t, ext, elem, name, prefix) {
            return Some((ext.unwrap(), i));
        }
        None
    }

    // -----------------------------------------------------------------
    // IDs and references

    /// xmlAddID with error reporting; returns false if already defined.
    pub fn add_id(&mut self, attr: NodeId, value: &str) -> bool {
        if value.is_empty() {
            return false;
        }
        let doc = match self.t.nodes[attr].doc {
            Some(d) => d,
            None => return false,
        };
        let exists = self
            .t
            .doc_info(doc)
            .map(|d| d.ids.contains_key(value))
            .unwrap_or(false);
        if exists {
            let parent = self.t.nodes[attr].parent;
            self.err_e(
                parent,
                XML_DTD_ID_REDEFINED,
                format!("ID {} already defined\n", value),
            );
            return false;
        }
        self.t.remove_id_for_attr(attr);
        if let Some(di) = self.t.doc_info_mut(doc) {
            di.ids.insert(value.to_string(), attr);
        }
        self.t.nodes[attr].atype = AT_ID;
        true
    }

    /// xmlAddRef
    pub fn add_ref(&mut self, attr: NodeId, value: &str) {
        let lineno = self.t.nodes[attr]
            .parent
            .map(|p| get_line_no(self.t, p))
            .unwrap_or(-1);
        let (a, name) = if self.v.streaming {
            (None, Some(self.t.nodes[attr].name.clone()))
        } else {
            (Some(attr), None)
        };
        self.v.refs.push(RefEntry {
            value: value.to_string(),
            attr: a,
            name,
            lineno,
        });
    }

    fn get_id(&self, v: &str) -> bool {
        self.t
            .doc_info(self.doc)
            .map(|d| d.ids.contains_key(v))
            .unwrap_or(false)
    }

    // -----------------------------------------------------------------
    // Declarations

    /// xmlValidateAttributeValue2
    fn validate_attribute_value2(&mut self, name: &str, atype: u8, value: &str) -> bool {
        let doc = self.doc;
        let mut ret = true;
        match atype {
            AT_ENTITY => {
                let mut ent = doc_entity_kind(self.t, doc, value);
                if ent.is_none() && self.standalone() == 1 {
                    if let Some(di) = self.t.doc_info_mut(doc) {
                        di.standalone = 0;
                    }
                    ent = doc_entity_kind(self.t, doc, value);
                }
                match ent {
                    None => {
                        self.err_e(
                            Some(doc),
                            XML_DTD_UNKNOWN_ENTITY_V,
                            format!(
                                "ENTITY attribute {} reference an unknown entity \"{}\"\n",
                                name, value
                            ),
                        );
                        ret = false;
                    }
                    Some(false) => {
                        self.err_e(
                            Some(doc),
                            XML_DTD_ENTITY_TYPE,
                            format!(
                                "ENTITY attribute {} reference an entity \"{}\" of wrong type\n",
                                name, value
                            ),
                        );
                        ret = false;
                    }
                    _ => {}
                }
            }
            AT_ENTITIES => {
                for nam in split_blank_tokens(value) {
                    match doc_entity_kind(self.t, doc, &nam) {
                        None => {
                            self.err_e(
                                Some(doc),
                                XML_DTD_UNKNOWN_ENTITY_V,
                                format!(
                                    "ENTITIES attribute {} reference an unknown entity \"{}\"\n",
                                    name, nam
                                ),
                            );
                            ret = false;
                        }
                        Some(false) => {
                            self.err_e(Some(doc), XML_DTD_ENTITY_TYPE, format!("ENTITIES attribute {} reference an entity \"{}\" of wrong type\n", name, nam));
                            ret = false;
                        }
                        _ => {}
                    }
                }
            }
            AT_NOTATION => {
                let (int, ext) = subsets(self.t, doc);
                if !has_notation(self.t, int, value) && !has_notation(self.t, ext, value) {
                    self.err_e(
                        Some(doc),
                        XML_DTD_UNKNOWN_NOTATION,
                        format!(
                            "NOTATION attribute {} reference an unknown notation \"{}\"\n",
                            name, value
                        ),
                    );
                    ret = false;
                }
            }
            _ => {}
        }
        ret
    }

    /// xmlValidateAttributeDecl
    pub fn validate_attribute_decl(&mut self, at: (NodeId, usize)) -> bool {
        if !self.has_dtd() {
            return false;
        }
        let attr = attr_decl(self.t, at.0, at.1);
        let anode = attr.node.or(Some(at.0));
        let mut ret = true;
        if let Some(dv) = &attr.default {
            if !validate_attribute_value(attr.atype_n, dv) {
                self.err_e(
                    anode,
                    XML_DTD_ATTRIBUTE_DEFAULT,
                    format!(
                        "Syntax of default value for attribute {} of {} is not valid\n",
                        attr.name, attr.elem
                    ),
                );
                ret = false;
            }
        }
        if attr.atype_n == AT_ID && attr.def_n != AD_IMPLIED && attr.def_n != AD_REQUIRED {
            self.err_e(
                anode,
                XML_DTD_ID_FIXED,
                format!(
                    "ID attribute {} of {} is not valid must be #IMPLIED or #REQUIRED\n",
                    attr.name, attr.elem
                ),
            );
            ret = false;
        }
        if attr.atype_n == AT_ID {
            let (int, ext) = subsets(self.t, self.doc);
            let (ep, el) = split_qname(&attr.elem);
            let nb_id = match find_elem(self.t, int, &el, ep.as_deref()) {
                Some(i) => self.scan_id_decls((int.unwrap(), i)),
                None => match int.and_then(|d| self.t.dtd_info(d)) {
                    Some(d) => d
                        .attr_defaults
                        .iter()
                        .filter(|a| a.elem == attr.elem && a.atype_n == AT_ID)
                        .count(),
                    None => 0,
                },
            };
            if nb_id > 1 {
                self.err_e(
                    anode,
                    XML_DTD_ID_SUBSET,
                    format!(
                        "Element {} has {} ID attribute defined in the internal subset : {}\n",
                        attr.elem, nb_id, attr.name
                    ),
                );
                ret = false;
            } else if ext.is_some() {
                let ext_id = match find_elem(self.t, ext, &el, ep.as_deref()) {
                    Some(i) => self.scan_id_decls((ext.unwrap(), i)),
                    None => 0,
                };
                if ext_id > 1 {
                    self.err_e(
                        anode,
                        XML_DTD_ID_SUBSET,
                        format!(
                            "Element {} has {} ID attribute defined in the external subset : {}\n",
                            attr.elem, ext_id, attr.name
                        ),
                    );
                    ret = false;
                } else if ext_id + nb_id > 1 {
                    self.err_e(anode, XML_DTD_ID_SUBSET, format!("Element {} has ID attributes defined in the internal and external subset : {}\n", attr.elem, attr.name));
                    ret = false;
                }
            }
        }
        if let Some(dv) = &attr.default {
            if !attr.tree.is_empty() && !attr.tree.iter().any(|x| x == dv) {
                self.err_e(anode, XML_DTD_ATTRIBUTE_VALUE, format!("Default value \"{}\" for attribute {} of {} is not among the enumerated set\n", dv, attr.name, attr.elem));
                ret = false;
            }
        }
        ret
    }

    /// xmlScanIDAttributeDecl without reporting
    fn scan_id_decls(&self, el: (NodeId, usize)) -> usize {
        let e = self.elem_decl(el);
        e.attrs
            .iter()
            .filter(|(d, i)| {
                self.t
                    .dtd_info(*d)
                    .map(|x| x.attr_defaults[*i].atype_n == AT_ID)
                    .unwrap_or(false)
            })
            .count()
    }

    /// xmlValidateElementDecl
    pub fn validate_element_decl(&mut self, el: (NodeId, usize)) -> bool {
        if !self.has_dtd() {
            return false;
        }
        let e = self.elem_decl(el).clone();
        let enode = e.node.or(Some(el.0));
        let mut ret = true;
        if e.etype == ET_MIXED {
            if let Some(ec) = &e.content {
                let mut cur = Some(ec.root);
                while let Some(c) = cur {
                    let cn = &ec.nodes[c];
                    if cn.ctype != CT_OR {
                        break;
                    }
                    let c1 = match cn.c1 {
                        Some(x) => x,
                        None => break,
                    };
                    if ec.nodes[c1].ctype == CT_ELEMENT {
                        let name = ec.nodes[c1].name.clone();
                        let prefix = ec.nodes[c1].prefix.clone();
                        let mut next = cn.c2;
                        while let Some(nx) = next {
                            let nn = &ec.nodes[nx];
                            if nn.ctype == CT_ELEMENT {
                                if nn.name == name && nn.prefix == prefix {
                                    let m = match &prefix {
                                        None => format!(
                                            "Definition of {} has duplicate references of {}\n",
                                            e.name, name
                                        ),
                                        Some(p) => format!(
                                            "Definition of {} has duplicate references of {}:{}\n",
                                            e.name, p, name
                                        ),
                                    };
                                    self.err_e(enode, XML_DTD_CONTENT_ERROR, m);
                                    ret = false;
                                }
                                break;
                            }
                            let nc1 = match nn.c1 {
                                Some(x) => x,
                                None => break,
                            };
                            if ec.nodes[nc1].ctype != CT_ELEMENT {
                                break;
                            }
                            if ec.nodes[nc1].name == name && ec.nodes[nc1].prefix == prefix {
                                let m = match &prefix {
                                    None => format!(
                                        "Definition of {} has duplicate references to {}\n",
                                        e.name, name
                                    ),
                                    Some(p) => format!(
                                        "Definition of {} has duplicate references to {}:{}\n",
                                        e.name, p, name
                                    ),
                                };
                                self.err_e(enode, XML_DTD_CONTENT_ERROR, m);
                                ret = false;
                            }
                            next = nn.c2;
                        }
                    }
                    cur = cn.c2;
                }
            }
        }
        let (int, ext) = subsets(self.t, self.doc);
        for sub in [int, ext] {
            if let Some(i) = find_elem(self.t, sub, &e.name, e.prefix.as_deref()) {
                let tst = &self.t.dtd_info(sub.unwrap()).unwrap().elements[i];
                if (sub.unwrap(), i) != el && tst.etype != ET_UNDEFINED {
                    let qn = match &e.prefix {
                        Some(p) => format!("{}:{}", p, e.name),
                        None => e.name.clone(),
                    };
                    let _ = qn;
                    self.err_e(
                        enode,
                        XML_DTD_ELEM_REDEFINED,
                        format!("Redefinition of element {}\n", e.name),
                    );
                    ret = false;
                }
            }
        }
        ret
    }

    // -----------------------------------------------------------------
    // Attributes and namespaces

    /// xmlValidCtxtNormalizeAttributeValue: Some(normalized) for
    /// non-CDATA declared attributes.
    pub fn normalize_attribute_value(
        &mut self,
        elem: NodeId,
        name: &str,
        value: &str,
    ) -> Option<String> {
        let (prefix, local) = split_qname(name);
        let (int, ext) = subsets(self.t, self.doc);
        let mut found: Option<(NodeId, usize)> = None;
        let mut extsubset = false;
        if let Some(q) = elem_qname(self.t, elem) {
            if let Some(i) = find_attr(self.t, int, &q, &local, prefix.as_deref()) {
                found = Some((int.unwrap(), i));
            } else if let Some(i) = find_attr(self.t, ext, &q, &local, prefix.as_deref()) {
                found = Some((ext.unwrap(), i));
                extsubset = true;
            }
        }
        let en = self.t.nodes[elem].name.clone();
        if found.is_none() {
            if let Some(i) = find_attr(self.t, int, &en, &local, prefix.as_deref()) {
                found = Some((int.unwrap(), i));
            } else if let Some(i) = find_attr(self.t, ext, &en, &local, prefix.as_deref()) {
                found = Some((ext.unwrap(), i));
                extsubset = true;
            }
        }
        let f = found?;
        if attr_decl(self.t, f.0, f.1).atype_n == AT_CDATA {
            return None;
        }
        let ret = normalize_string(value);
        if self.standalone() != 0 && extsubset && ret != value {
            self.err_e(Some(elem), XML_DTD_NOT_STANDALONE, format!("standalone: {} on {} value had to be normalized based on external subset declaration\n", name, en));
            self.valid = false;
        }
        Some(ret)
    }

    /// xmlValidateOneAttribute
    pub fn validate_one_attribute(&mut self, elem: NodeId, attr: NodeId, value: &str) -> bool {
        if !self.has_dtd() {
            return false;
        }
        let aprefix = self
            .t
            .ns_prefix(self.t.nodes[attr].ns)
            .map(|s| s.to_string());
        let aname = self.t.nodes[attr].name.clone();
        let ename = self.t.nodes[elem].name.clone();
        let mut decl = None;
        if let Some(q) = elem_qname(self.t, elem) {
            decl = self.lookup_attr(&q, &aname, aprefix.as_deref());
        }
        if decl.is_none() {
            decl = self.lookup_attr(&ename, &aname, aprefix.as_deref());
        }
        let decl = match decl {
            Some(d) => attr_decl(self.t, d.0, d.1),
            None => {
                self.err_e(
                    Some(elem),
                    XML_DTD_UNKNOWN_ATTRIBUTE,
                    format!(
                        "No declaration for attribute {} of element {}\n",
                        aname, ename
                    ),
                );
                return false;
            }
        };
        if self.t.nodes[attr].atype == AT_ID {
            self.t.remove_id_for_attr(attr);
        }
        self.t.nodes[attr].atype = decl.atype_n;
        let mut ret = true;
        if !validate_attribute_value(decl.atype_n, value) {
            self.err_e(
                Some(elem),
                XML_DTD_ATTRIBUTE_VALUE,
                format!(
                    "Syntax of value for attribute {} of {} is not valid\n",
                    aname, ename
                ),
            );
            ret = false;
        }
        let dv = decl.default.clone().unwrap_or_default();
        if decl.def_n == AD_FIXED && decl.default.as_deref() != Some(value) {
            self.err_e(
                Some(elem),
                XML_DTD_ATTRIBUTE_DEFAULT,
                format!(
                    "Value for attribute {} of {} is different from default \"{}\"\n",
                    aname, ename, dv
                ),
            );
            ret = false;
        }
        if decl.atype_n == AT_ID && !self.add_id(attr, value) {
            ret = false;
        }
        if decl.atype_n == AT_IDREF || decl.atype_n == AT_IDREFS {
            self.add_ref(attr, value);
        }
        if decl.atype_n == AT_NOTATION {
            let (int, ext) = subsets(self.t, self.doc);
            if !has_notation(self.t, int, value) && !has_notation(self.t, ext, value) {
                self.err_e(
                    Some(elem),
                    XML_DTD_UNKNOWN_NOTATION,
                    format!(
                        "Value \"{}\" for attribute {} of {} is not a declared Notation\n",
                        value, aname, ename
                    ),
                );
                ret = false;
            }
            if !decl.tree.iter().any(|x| x == value) {
                self.err_e(Some(elem), XML_DTD_NOTATION_VALUE, format!("Value \"{}\" for attribute {} of {} is not among the enumerated notations\n", value, aname, ename));
                ret = false;
            }
        }
        if decl.atype_n == AT_ENUMERATION && !decl.tree.iter().any(|x| x == value) {
            self.err_e(
                Some(elem),
                XML_DTD_ATTRIBUTE_VALUE,
                format!(
                    "Value \"{}\" for attribute {} of {} is not among the enumerated set\n",
                    value, aname, ename
                ),
            );
            ret = false;
        }
        if decl.def_n == AD_FIXED && decl.default.as_deref() != Some(value) {
            self.err_e(
                Some(elem),
                XML_DTD_ATTRIBUTE_VALUE,
                format!(
                    "Value for attribute {} of {} must be \"{}\"\n",
                    aname, ename, dv
                ),
            );
            ret = false;
        }
        ret &= self.validate_attribute_value2(&aname, decl.atype_n, value);
        ret
    }

    /// xmlValidateOneNamespace
    pub fn validate_one_namespace(
        &mut self,
        elem: NodeId,
        prefix: Option<&str>,
        ns: NsId,
        value: &str,
    ) -> bool {
        if !self.has_dtd() {
            return false;
        }
        let ename = self.t.nodes[elem].name.clone();
        let nsp = ns_prefix_of(self.t, ns);
        let mut decl = None;
        let lookup = |cx: &Self, el: &str| -> Option<(NodeId, usize)> {
            match &nsp {
                Some(p) => cx.lookup_attr(el, p, Some("xmlns")),
                None => cx.lookup_attr(el, "xmlns", None),
            }
        };
        if let Some(p) = prefix {
            decl = lookup(self, &format!("{}:{}", p, ename));
        }
        if decl.is_none() {
            decl = lookup(self, &ename);
        }
        let an = match &nsp {
            Some(p) => format!("xmlns:{}", p),
            None => "xmlns".to_string(),
        };
        let decl = match decl {
            Some(d) => attr_decl(self.t, d.0, d.1),
            None => {
                self.err_e(
                    Some(elem),
                    XML_DTD_UNKNOWN_ATTRIBUTE,
                    format!("No declaration for attribute {} of element {}\n", an, ename),
                );
                return false;
            }
        };
        let mut ret = true;
        if !validate_attribute_value(decl.atype_n, value) {
            self.err_e(
                Some(elem),
                XML_DTD_INVALID_DEFAULT,
                format!(
                    "Syntax of value for attribute {} of {} is not valid\n",
                    an, ename
                ),
            );
            ret = false;
        }
        let dv = decl.default.clone().unwrap_or_default();
        if decl.def_n == AD_FIXED && decl.default.as_deref() != Some(value) {
            self.err_e(
                Some(elem),
                XML_DTD_ATTRIBUTE_DEFAULT,
                format!(
                    "Value for attribute {} of {} is different from default \"{}\"\n",
                    an, ename, dv
                ),
            );
            ret = false;
        }
        if decl.atype_n == AT_NOTATION {
            let (int, ext) = subsets(self.t, self.doc);
            if !has_notation(self.t, int, value) && !has_notation(self.t, ext, value) {
                self.err_e(
                    Some(elem),
                    XML_DTD_UNKNOWN_NOTATION,
                    format!(
                        "Value \"{}\" for attribute {} of {} is not a declared Notation\n",
                        value, an, ename
                    ),
                );
                ret = false;
            }
            if !decl.tree.iter().any(|x| x == value) {
                self.err_e(Some(elem), XML_DTD_NOTATION_VALUE, format!("Value \"{}\" for attribute {} of {} is not among the enumerated notations\n", value, an, ename));
                ret = false;
            }
        }
        if decl.atype_n == AT_ENUMERATION && !decl.tree.iter().any(|x| x == value) {
            self.err_e(
                Some(elem),
                XML_DTD_ATTRIBUTE_VALUE,
                format!(
                    "Value \"{}\" for attribute {} of {} is not among the enumerated set\n",
                    value, an, ename
                ),
            );
            ret = false;
        }
        if decl.def_n == AD_FIXED && decl.default.as_deref() != Some(value) {
            self.err_e(
                Some(elem),
                XML_DTD_ELEM_NAMESPACE,
                format!(
                    "Value for attribute {} of {} must be \"{}\"\n",
                    an, ename, dv
                ),
            );
            ret = false;
        }
        let n2 = nsp.clone().unwrap_or_else(|| "xmlns".to_string());
        ret &= self.validate_attribute_value2(&n2, decl.atype_n, value);
        ret
    }

    // -----------------------------------------------------------------
    // Elements

    /// xmlValidGetElemDecl: ((dtd, index), extsubset)
    fn get_elem_decl(&mut self, elem: NodeId) -> Option<((NodeId, usize), bool)> {
        let (int, ext) = subsets(self.t, self.doc);
        let name = self.t.nodes[elem].name.clone();
        let prefix = self
            .t
            .ns_prefix(self.t.nodes[elem].ns)
            .map(|s| s.to_string());
        let mut r = None;
        if let Some(p) = &prefix {
            if let Some(i) = find_elem(self.t, int, &name, Some(p)) {
                r = Some(((int.unwrap(), i), false));
            } else if let Some(i) = find_elem(self.t, ext, &name, Some(p)) {
                r = Some(((ext.unwrap(), i), true));
            }
        }
        if r.is_none() {
            if let Some(i) = find_elem(self.t, int, &name, None) {
                r = Some(((int.unwrap(), i), false));
            } else if let Some(i) = find_elem(self.t, ext, &name, None) {
                r = Some(((ext.unwrap(), i), true));
            }
        }
        if r.is_none() {
            self.err_e(
                Some(elem),
                XML_DTD_UNKNOWN_ELEM,
                format!("No declaration for element {}\n", name),
            );
        }
        r
    }

    /// The compiled content model of a declaration (building it on first
    /// use, reporting non-determinism once).
    fn content_model(&mut self, el: (NodeId, usize)) -> Option<Rc<ContModel>> {
        let e = self.elem_decl(el);
        if let Some(m) = &e.model {
            return Some(m.clone());
        }
        let ec = e.content.clone()?;
        let name = e.name.clone();
        let enode = e.node;
        let m = Rc::new(ContModel::build(&ec)?);
        if let Some(d) = self.t.dtd_info_mut(el.0) {
            d.elements[el.1].model = Some(m.clone());
        }
        if !m.determinist {
            let expr = ec.snprintf(true);
            self.err_e(
                enode.or(Some(el.0)),
                XML_DTD_CONTENT_NOT_DETERMINIST,
                format!("Content model of {} is not deterministic: {}\n", name, expr),
            );
            self.valid = false;
        }
        Some(m)
    }

    fn ent_content(&self, eref: NodeId) -> Option<NodeId> {
        let ent = self.t.nodes[eref].children?;
        self.t.nodes[ent].children
    }

    fn is_blank_node(&self, n: NodeId) -> bool {
        let nd = &self.t.nodes[n];
        matches!(nd.t, NType::Text | NType::CData) && nd.content.chars().all(is_blank_ch)
    }

    /// Walk a child list descending into entity references (the nodeVPush
    /// dance of valid.c).
    fn flat_children(&self, first: Option<NodeId>) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = Vec::new();
        let mut cur = first;
        loop {
            let c = match cur {
                Some(c) => c,
                None => match stack.pop() {
                    Some(r) => {
                        cur = self.t.nodes[r].next;
                        continue;
                    }
                    None => break,
                },
            };
            if self.t.nodes[c].t == NType::EntityRef {
                if let Some(x) = self.ent_content(c) {
                    stack.push(c);
                    cur = Some(x);
                    continue;
                }
            }
            out.push(c);
            cur = self.t.nodes[c].next;
        }
        out
    }

    /// xmlSnprintfElements
    fn snprintf_elements(&self, first: Option<NodeId>) -> String {
        let size = 5000usize;
        if first.is_none() {
            return String::new();
        }
        let mut buf = String::from("(");
        let mut cur = first;
        while let Some(c) = cur {
            let len = buf.len();
            if size - len < 50 {
                if size - len > 4 && !buf.ends_with('.') {
                    buf.push_str(" ...");
                }
                return buf;
            }
            let nd = &self.t.nodes[c];
            let has_next = nd.next.is_some();
            match nd.t {
                NType::Element => {
                    let p = self.t.ns_prefix(nd.ns);
                    let qlen = nd.name.len() + p.map(|x| x.len() + 1).unwrap_or(0);
                    if size - len < qlen + 10 {
                        if size - len > 4 && !buf.ends_with('.') {
                            buf.push_str(" ...");
                        }
                        return buf;
                    }
                    if let Some(p) = p {
                        buf.push_str(p);
                        buf.push(':');
                    }
                    buf.push_str(&nd.name);
                    if has_next {
                        buf.push(' ');
                    }
                }
                NType::Text | NType::CData | NType::EntityRef => {
                    if !(nd.t == NType::Text && self.is_blank_node(c)) {
                        buf.push_str("CDATA");
                        if has_next {
                            buf.push(' ');
                        }
                    }
                }
                NType::Attribute
                | NType::Document
                | NType::HtmlDocument
                | NType::DocumentType
                | NType::DocumentFrag
                | NType::Notation
                | NType::NamespaceDecl => {
                    buf.push_str("???");
                    if has_next {
                        buf.push(' ');
                    }
                }
                _ => {}
            }
            cur = nd.next;
        }
        buf.push(')');
        buf
    }

    /// xmlValidateElementContent (regexp version)
    fn validate_element_content(
        &mut self,
        child: Option<NodeId>,
        el: (NodeId, usize),
        parent: NodeId,
    ) -> i32 {
        let model = match self.content_model(el) {
            Some(m) => m,
            None => return -1,
        };
        if !model.determinist {
            return -1;
        }
        let mut exec = Exec::new(model);
        let mut ret = 1;
        let mut failed = false;
        for c in self.flat_children(child) {
            match self.t.nodes[c].t {
                NType::Text => {
                    if !self.is_blank_node(c) {
                        ret = 0;
                        failed = true;
                        break;
                    }
                }
                NType::CData => {
                    ret = 0;
                    failed = true;
                    break;
                }
                NType::Element => {
                    let q = self.t.qname(c);
                    ret = exec.push(Some(&q));
                }
                _ => {}
            }
        }
        if !failed {
            ret = exec.push(None);
        }
        if ret != 1 {
            let e = self.elem_decl(el);
            let expr = e
                .content
                .as_ref()
                .map(|c| c.snprintf(true))
                .unwrap_or_default();
            let name = e.name.clone();
            let list = self.snprintf_elements(child);
            self.err_e(
                Some(parent),
                XML_DTD_CONTENT_MODEL,
                format!(
                    "Element {} content does not follow the DTD, expecting {}, got {}\n",
                    name, expr, list
                ),
            );
            ret = 0;
        }
        ret
    }

    /// xmlValidateOneCdataElement
    fn validate_one_cdata_element(&self, elem: NodeId) -> bool {
        for c in self.flat_children(self.t.nodes[elem].children) {
            if !matches!(
                self.t.nodes[c].t,
                NType::Comment | NType::Pi | NType::Text | NType::CData | NType::EntityRef
            ) {
                return false;
            }
        }
        true
    }

    /// Is `qname` (or `name`) among the names of a MIXED declaration?
    fn mixed_has(ec: &ElemContent, name: &str) -> bool {
        let mut cur = Some(ec.root);
        while let Some(c) = cur {
            let n = &ec.nodes[c];
            if n.ctype == CT_ELEMENT {
                if n.name == name {
                    return true;
                }
            } else if n.ctype == CT_OR
                && n.c1
                    .map(|x| ec.nodes[x].ctype == CT_ELEMENT)
                    .unwrap_or(false)
            {
                if ec.nodes[n.c1.unwrap()].name == name {
                    return true;
                }
            } else if n.ctype != CT_OR
                || n.c1.map(|x| ec.nodes[x].ctype != CT_PCDATA).unwrap_or(true)
            {
                break;
            }
            cur = n.c2;
        }
        false
    }

    /// xmlValidateOneElement
    pub fn validate_one_element(&mut self, elem: NodeId) -> bool {
        if !self.has_dtd() {
            return false;
        }
        match self.t.nodes[elem].t {
            NType::Text | NType::CData | NType::EntityRef | NType::Pi | NType::Comment => {
                return true;
            }
            NType::Element => {}
            _ => {
                self.err_e(
                    Some(elem),
                    XML_ERR_INTERNAL_ERROR,
                    "unexpected element type\n".into(),
                );
                return false;
            }
        }
        let (el, extsubset) = match self.get_elem_decl(elem) {
            Some(x) => x,
            None => return false,
        };
        let decl = self.elem_decl(el).clone();
        let ename = self.t.nodes[elem].name.clone();
        let mut ret = true;
        if self.v.vstate.is_empty() {
            match decl.etype {
                ET_UNDEFINED => {
                    self.err_e(
                        Some(elem),
                        XML_DTD_UNKNOWN_ELEM,
                        format!("No declaration for element {}\n", ename),
                    );
                    return false;
                }
                ET_EMPTY => {
                    if self.t.nodes[elem].children.is_some() {
                        self.err_e(
                            Some(elem),
                            XML_DTD_NOT_EMPTY,
                            format!(
                                "Element {} was declared EMPTY this one has content\n",
                                ename
                            ),
                        );
                        ret = false;
                    }
                }
                ET_MIXED => {
                    let ec = decl.content.clone().unwrap_or_default();
                    if ec
                        .nodes
                        .get(ec.root)
                        .map(|n| n.ctype == CT_PCDATA)
                        .unwrap_or(false)
                    {
                        ret = self.validate_one_cdata_element(elem);
                        if !ret {
                            self.err_e(
                                Some(elem),
                                XML_DTD_NOT_PCDATA,
                                format!(
                                    "Element {} was declared #PCDATA but contains non text nodes\n",
                                    ename
                                ),
                            );
                        }
                    } else {
                        let mut child = self.t.nodes[elem].children;
                        while let Some(c) = child {
                            if self.t.nodes[c].t == NType::Element {
                                let name = self.t.nodes[c].name.clone();
                                let mut ok = false;
                                if let Some(q) = elem_qname(self.t, c) {
                                    ok = Self::mixed_has(&ec, &q);
                                }
                                if !ok && !Self::mixed_has(&ec, &name) {
                                    self.err_e(Some(elem), XML_DTD_INVALID_CHILD, format!("Element {} is not declared in {} list of possible children\n", name, ename));
                                    ret = false;
                                }
                            }
                            child = self.t.nodes[c].next;
                        }
                    }
                }
                ET_ELEMENT => {
                    if self.standalone() == 1 && extsubset {
                        let mut child = self.t.nodes[elem].children;
                        while let Some(c) = child {
                            if self.t.nodes[c].t == NType::Text
                                && self.t.nodes[c].content.chars().all(is_blank_ch)
                            {
                                self.err_e(Some(elem), XML_DTD_STANDALONE_WHITE_SPACE, format!("standalone: {} declared in the external subset contains white spaces nodes\n", ename));
                                ret = false;
                                break;
                            }
                            child = self.t.nodes[c].next;
                        }
                    }
                    let first = self.t.nodes[elem].children;
                    if self.validate_element_content(first, el, elem) <= 0 {
                        ret = false;
                    }
                }
                _ => {}
            }
        }
        // [ VC: Required Attribute ]
        for (d, i) in decl.attrs.clone() {
            let attr = attr_decl(self.t, d, i);
            if attr.def_n == AD_REQUIRED {
                let mut qualified = -1;
                let mut found = false;
                if attr.prefix.is_none() && attr.name == "xmlns" {
                    found = self.t.nodes[elem]
                        .ns_def
                        .iter()
                        .any(|&ns| self.t.nss[ns].prefix.is_none());
                } else if attr.prefix.as_deref() == Some("xmlns") {
                    found = self.t.nodes[elem]
                        .ns_def
                        .iter()
                        .any(|&ns| self.t.nss[ns].prefix.as_deref() == Some(attr.name.as_str()));
                } else {
                    for a in self.t.attrs_of(elem) {
                        if self.t.nodes[a].name != attr.name {
                            continue;
                        }
                        if attr.prefix.is_some() {
                            let ns = self.t.nodes[a].ns.or(self.t.nodes[elem].ns);
                            match ns {
                                None => {
                                    if qualified < 0 {
                                        qualified = 0;
                                    }
                                }
                                Some(ns) => {
                                    if self.t.nss[ns].prefix != attr.prefix {
                                        if qualified < 1 {
                                            qualified = 1;
                                        }
                                    } else {
                                        found = true;
                                        break;
                                    }
                                }
                            }
                        } else {
                            found = true;
                            break;
                        }
                    }
                }
                if found {
                    continue;
                }
                let ap = attr.prefix.clone().unwrap_or_default();
                if qualified == -1 {
                    let m = match &attr.prefix {
                        None => {
                            format!("Element {} does not carry attribute {}\n", ename, attr.name)
                        }
                        Some(p) => format!(
                            "Element {} does not carry attribute {}:{}\n",
                            ename, p, attr.name
                        ),
                    };
                    self.err_e(Some(elem), XML_DTD_MISSING_ATTRIBUTE, m);
                    ret = false;
                } else if qualified == 0 {
                    self.err(
                        Some(elem),
                        XML_DTD_NO_PREFIX,
                        ERR_WARNING,
                        format!(
                            "Element {} required attribute {}:{} has no prefix\n",
                            ename, ap, attr.name
                        ),
                    );
                } else {
                    self.err(
                        Some(elem),
                        XML_DTD_DIFFERENT_PREFIX,
                        ERR_WARNING,
                        format!(
                            "Element {} required attribute {}:{} has different prefix\n",
                            ename, ap, attr.name
                        ),
                    );
                }
            } else if attr.def_n == AD_FIXED {
                let dv = attr.default.clone().unwrap_or_default();
                if attr.prefix.is_none() && attr.name == "xmlns" {
                    if let Some(&ns) = self.t.nodes[elem]
                        .ns_def
                        .iter()
                        .find(|&&ns| self.t.nss[ns].prefix.is_none())
                    {
                        if self.t.nss[ns].href != dv {
                            self.err_e(Some(elem), XML_DTD_ELEM_DEFAULT_NAMESPACE, format!("Element {} namespace name for default namespace does not match the DTD\n", ename));
                            ret = false;
                        }
                    }
                } else if attr.prefix.as_deref() == Some("xmlns") {
                    if let Some(&ns) = self.t.nodes[elem]
                        .ns_def
                        .iter()
                        .find(|&&ns| self.t.nss[ns].prefix.as_deref() == Some(attr.name.as_str()))
                    {
                        if self.t.nss[ns].href != dv {
                            let p = self.t.nss[ns].prefix.clone().unwrap_or_default();
                            self.err_e(
                                Some(elem),
                                XML_DTD_ELEM_NAMESPACE,
                                format!(
                                    "Element {} namespace name for {} does not match the DTD\n",
                                    ename, p
                                ),
                            );
                            ret = false;
                        }
                    }
                }
            }
        }
        ret
    }

    /// xmlValidateRoot
    pub fn validate_root(&mut self) -> bool {
        let root = match self.t.root_element(self.doc) {
            Some(r) => r,
            None => {
                self.err_e(None, XML_DTD_NO_ROOT, "no root element\n".into());
                return false;
            }
        };
        let int = subsets(self.t, self.doc).0;
        if let Some(dtd) = int {
            let dname = self.t.nodes[dtd].name.clone();
            let rname = self.t.nodes[root].name.clone();
            if dname != rname {
                if let Some(q) = elem_qname(self.t, root) {
                    if q == dname {
                        return true;
                    }
                }
                if dname == "HTML" && rname == "html" {
                    return true;
                }
                self.err_e(
                    Some(root),
                    XML_DTD_ROOT_NAME,
                    format!(
                        "root and DTD name do not match '{}' and '{}'\n",
                        rname, dname
                    ),
                );
                return false;
            }
        }
        true
    }

    /// xmlValidateElement: the subtree under `root`.
    pub fn validate_element(&mut self, root: NodeId) -> bool {
        if !self.has_dtd() {
            return false;
        }
        let mut ret = true;
        let mut elem = root;
        loop {
            ret &= self.validate_one_element(elem);
            if self.t.nodes[elem].t == NType::Element {
                for a in self.t.attrs_of(elem) {
                    let value = self.t.list_get_string(self.t.nodes[a].children, false);
                    ret &= self.validate_one_attribute(elem, a, &value);
                }
                let prefix = self
                    .t
                    .ns_prefix(self.t.nodes[elem].ns)
                    .map(|s| s.to_string());
                for ns in self.t.nodes[elem].ns_def.clone() {
                    let href = self.t.nss[ns].href.clone();
                    let p = if self.t.nodes[elem].ns.is_none() {
                        None
                    } else {
                        prefix.clone()
                    };
                    ret &= self.validate_one_namespace(elem, p.as_deref(), ns, &href);
                }
                if let Some(c) = self.t.nodes[elem].children {
                    elem = c;
                    continue;
                }
            }
            loop {
                if elem == root {
                    return ret;
                }
                if self.t.nodes[elem].next.is_some() {
                    break;
                }
                elem = match self.t.nodes[elem].parent {
                    Some(p) => p,
                    None => return ret,
                };
            }
            elem = self.t.nodes[elem].next.unwrap();
        }
    }

    /// xmlValidateDocumentFinal: check the IDREF/IDREFS references.
    pub fn validate_document_final(&mut self) -> bool {
        self.no_input = true;
        self.valid = true;
        let refs = std::mem::take(&mut self.v.refs);
        for r in &refs {
            match r.attr {
                None => {
                    let name = r.name.clone().unwrap_or_default();
                    for s in split_blank_tokens(&r.value) {
                        if !self.get_id(&s) {
                            self.err_e(
                                None,
                                XML_DTD_UNKNOWN_ID,
                                format!(
                                    "attribute {} line {} references an unknown ID \"{}\"\n",
                                    name, r.lineno, s
                                ),
                            );
                            self.valid = false;
                        }
                    }
                }
                Some(a) => {
                    let parent = self.t.nodes[a].parent;
                    let an = self.t.nodes[a].name.clone();
                    if self.t.nodes[a].atype == AT_IDREF {
                        if !self.get_id(&r.value) {
                            self.err_e(
                                parent,
                                XML_DTD_UNKNOWN_ID,
                                format!(
                                    "IDREF attribute {} references an unknown ID \"{}\"\n",
                                    an, r.value
                                ),
                            );
                            self.valid = false;
                        }
                    } else if self.t.nodes[a].atype == AT_IDREFS {
                        for s in split_blank_tokens(&r.value) {
                            if !self.get_id(&s) {
                                self.err_e(
                                    parent,
                                    XML_DTD_UNKNOWN_ID,
                                    format!(
                                        "IDREFS attribute {} references an unknown ID \"{}\"\n",
                                        an, s
                                    ),
                                );
                                self.valid = false;
                            }
                        }
                    }
                }
            }
        }
        self.v.refs = refs;
        self.no_input = false;
        self.valid
    }

    /// xmlValidateAttributeCallback
    fn validate_attribute_cb(&mut self, dtd: NodeId, idx: usize) {
        let cur = attr_decl(self.t, dtd, idx);
        if matches!(cur.atype_n, AT_ENTITY | AT_ENTITIES | AT_NOTATION) {
            if let Some(dv) = &cur.default {
                if !self.validate_attribute_value2(&cur.name, cur.atype_n, dv) {
                    self.valid = false;
                }
            }
            for v in &cur.tree {
                if !self.validate_attribute_value2(&cur.name, cur.atype_n, v) {
                    self.valid = false;
                }
            }
        }
        if cur.atype_n == AT_NOTATION {
            let (int, ext) = subsets(self.t, self.doc);
            let (ep, el) = split_qname(&cur.elem);
            let mut e = find_elem(self.t, int, &el, ep.as_deref()).map(|i| (int.unwrap(), i));
            if e.is_none() {
                e = find_elem(self.t, ext, &el, ep.as_deref()).map(|i| (ext.unwrap(), i));
            }
            if e.is_none() {
                e = find_elem(self.t, Some(dtd), &el, ep.as_deref()).map(|i| (dtd, i));
            }
            match e {
                None => {
                    self.err_e(
                        None,
                        XML_DTD_UNKNOWN_ELEM,
                        format!(
                            "attribute {}: could not find decl for element {}\n",
                            cur.name, cur.elem
                        ),
                    );
                }
                Some(e) => {
                    if self.elem_decl(e).etype == ET_EMPTY {
                        self.err_e(
                            None,
                            XML_DTD_EMPTY_NOTATION,
                            format!(
                                "NOTATION attribute {} declared for EMPTY element {}\n",
                                cur.name, cur.elem
                            ),
                        );
                        self.valid = false;
                    }
                }
            }
        }
    }

    /// xmlValidateDtdFinal
    pub fn validate_dtd_final(&mut self) -> bool {
        if !self.has_dtd() {
            return false;
        }
        self.valid = true;
        let (int, ext) = subsets(self.t, self.doc);
        for dtd in [int, ext].into_iter().flatten() {
            let n = self
                .t
                .dtd_info(dtd)
                .map(|d| d.attr_defaults.len())
                .unwrap_or(0);
            for i in 0..n {
                self.validate_attribute_cb(dtd, i);
            }
            let ents: Vec<NodeId> = self
                .t
                .dtd_info(dtd)
                .map(|d| d.entities.iter().map(|(_, e)| *e).collect())
                .unwrap_or_default();
            for e in ents {
                let info = match self.t.entity_info(e) {
                    Some(i) => i.clone(),
                    None => continue,
                };
                if info.etype == EntityType::ExternalGeneralUnparsed {
                    if let Some(nota) = info.notation.clone() {
                        if !self.validate_notation_use(&nota) {
                            self.valid = false;
                        }
                    }
                }
            }
        }
        self.valid
    }

    /// xmlValidateNotationUse
    pub fn validate_notation_use(&mut self, name: &str) -> bool {
        let (int, ext) = subsets(self.t, self.doc);
        if int.is_none() {
            return false;
        }
        if !has_notation(self.t, int, name) && !has_notation(self.t, ext, name) {
            let doc = self.doc;
            self.err_e(
                Some(doc),
                XML_DTD_UNKNOWN_NOTATION,
                format!("NOTATION {} is not declared\n", name),
            );
            return false;
        }
        true
    }

    // -----------------------------------------------------------------
    // Push (streaming) validation

    /// xmlValidatePushElement
    pub fn push_element(&mut self, elem: NodeId, qname: &str) -> bool {
        let mut ret = true;
        if let Some(state) = self.v.vstate.last().cloned() {
            if let Some(d) = state.decl {
                let decl = self.elem_decl(d).clone();
                let sname = self.t.nodes[state.node].name.clone();
                match decl.etype {
                    ET_UNDEFINED => ret = false,
                    ET_EMPTY => {
                        self.err_e(
                            Some(state.node),
                            XML_DTD_NOT_EMPTY,
                            format!(
                                "Element {} was declared EMPTY this one has content\n",
                                sname
                            ),
                        );
                        ret = false;
                    }
                    ET_MIXED => {
                        let ec = decl.content.clone().unwrap_or_default();
                        if ec
                            .nodes
                            .get(ec.root)
                            .map(|n| n.ctype == CT_PCDATA)
                            .unwrap_or(false)
                        {
                            self.err_e(
                                Some(state.node),
                                XML_DTD_NOT_PCDATA,
                                format!(
                                    "Element {} was declared #PCDATA but contains non text nodes\n",
                                    sname
                                ),
                            );
                            ret = false;
                        } else if !Self::mixed_has_q(&ec, qname) {
                            self.err_e(
                                Some(state.node),
                                XML_DTD_INVALID_CHILD,
                                format!(
                                    "Element {} is not declared in {} list of possible children\n",
                                    qname, sname
                                ),
                            );
                            ret = false;
                        }
                    }
                    ET_ELEMENT => {
                        let idx = self.v.vstate.len() - 1;
                        if let Some(ex) = self.v.vstate[idx].exec.as_mut() {
                            if ex.push(Some(qname)) < 0 {
                                self.err_e(Some(state.node), XML_DTD_CONTENT_MODEL, format!("Element {} content does not follow the DTD, Misplaced {}\n", sname, qname));
                                ret = false;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let decl = self.get_elem_decl(elem).map(|x| x.0);
        let mut exec = None;
        if let Some(d) = decl {
            if self.elem_decl(d).etype == ET_ELEMENT {
                match self.content_model(d) {
                    Some(m) => exec = Some(Exec::new(m)),
                    None => {
                        let nm = self.t.nodes[elem].name.clone();
                        let n = self.elem_decl(d).node;
                        self.err_e(
                            n,
                            XML_ERR_INTERNAL_ERROR,
                            format!("Failed to build content model regexp for {}\n", nm),
                        );
                    }
                }
            }
        }
        self.v.vstate.push(VState {
            decl,
            node: elem,
            exec,
        });
        ret
    }

    /// xmlValidateCheckMixed (QName aware)
    fn mixed_has_q(ec: &ElemContent, qname: &str) -> bool {
        let (prefix, name) = match qname.find(':') {
            Some(i) => (Some(&qname[..i]), &qname[i + 1..]),
            None => (None, qname),
        };
        let mut cur = Some(ec.root);
        while let Some(c) = cur {
            let n = &ec.nodes[c];
            let el = if n.ctype == CT_ELEMENT {
                Some(c)
            } else if n.ctype == CT_OR
                && n.c1
                    .map(|x| ec.nodes[x].ctype == CT_ELEMENT)
                    .unwrap_or(false)
            {
                n.c1
            } else if n.ctype != CT_OR
                || n.c1.map(|x| ec.nodes[x].ctype != CT_PCDATA).unwrap_or(true)
            {
                break;
            } else {
                None
            };
            if let Some(e) = el {
                let en = &ec.nodes[e];
                if en.prefix.as_deref() == prefix && en.name == name {
                    return true;
                }
            }
            cur = n.c2;
        }
        false
    }

    /// xmlValidatePushCData
    pub fn push_cdata(&mut self, data: &str) -> bool {
        if data.is_empty() {
            return true;
        }
        if let Some(state) = self.v.vstate.last().cloned() {
            if let Some(d) = state.decl {
                let et = self.elem_decl(d).etype;
                let sname = self.t.nodes[state.node].name.clone();
                match et {
                    ET_UNDEFINED => return false,
                    ET_EMPTY => {
                        self.err_e(
                            Some(state.node),
                            XML_DTD_NOT_EMPTY,
                            format!(
                                "Element {} was declared EMPTY this one has content\n",
                                sname
                            ),
                        );
                        return false;
                    }
                    ET_ELEMENT => {
                        if !data.chars().all(is_blank_ch) {
                            self.err_e(Some(state.node), XML_DTD_CONTENT_MODEL, format!("Element {} content does not follow the DTD, Text not allowed\n", sname));
                            return false;
                        }
                    }
                    _ => {}
                }
            }
        }
        true
    }

    /// xmlValidatePopElement
    pub fn pop_element(&mut self) -> bool {
        let mut ret = true;
        if let Some(mut state) = self.v.vstate.pop() {
            if let Some(d) = state.decl {
                if self.elem_decl(d).etype == ET_ELEMENT {
                    if let Some(ex) = state.exec.as_mut() {
                        if ex.push(None) <= 0 {
                            let sname = self.t.nodes[state.node].name.clone();
                            self.err_e(Some(state.node), XML_DTD_CONTENT_MODEL, format!("Element {} content does not follow the DTD, Expecting more children\n", sname));
                            ret = false;
                        }
                    }
                }
            }
        }
        ret
    }

    /// xmlValidateDocument: full validation of the document tree.
    pub fn validate_document(&mut self) -> bool {
        if !self.has_dtd() {
            self.err_e(None, XML_DTD_NO_DTD, "no DTD found!\n".into());
            return false;
        }
        // doc->ids and doc->refs are rebuilt
        let ids: Vec<NodeId> = self
            .t
            .doc_info(self.doc)
            .map(|d| d.ids.values().copied().collect())
            .unwrap_or_default();
        for a in ids {
            if self.t.nodes[a].atype == AT_ID {
                self.t.nodes[a].atype = 0;
            }
        }
        if let Some(di) = self.t.doc_info_mut(self.doc) {
            di.ids.clear();
        }
        self.v.refs.clear();
        let mut ret = self.validate_dtd_final();
        if !self.validate_root() {
            return false;
        }
        if let Some(root) = self.t.root_element(self.doc) {
            ret &= self.validate_element(root);
        } else {
            ret = false;
        }
        ret &= self.validate_document_final();
        ret
    }
}

fn split_blank_tokens(v: &str) -> Vec<String> {
    // the loop of xmlValidateRef / ENTITIES: tokens separated by blanks; a
    // leading blank yields an empty first token
    let mut out = Vec::new();
    let b: Vec<char> = v.chars().collect();
    let mut i = 0;
    while i < b.len() {
        let s = i;
        while i < b.len() && !is_blank_ch(b[i]) {
            i += 1;
        }
        out.push(b[s..i].iter().collect());
        if i >= b.len() {
            break;
        }
        while i < b.len() && is_blank_ch(b[i]) {
            i += 1;
        }
    }
    out
}

/// DOMDocument::validate(): xmlValidateDocument on a document, returning
/// the diagnostics (file = document URL, line = node line).
pub fn validate_document(doc: NodeId) -> Vec<XmlError> {
    validate_document_ex(doc).1
}

/// Like [`validate_document`], also returning the validity.
pub fn validate_document_ex(doc: NodeId) -> (bool, Vec<XmlError>) {
    let mut errors = Vec::new();
    // an external subset that was not loaded at parse time is fetched now
    let need = crate::tree::with(|t| {
        let (int, ext) = subsets(t, doc);
        let d = t.dtd_info(int?)?;
        if ext.is_some() || (d.system_id.is_none() && d.external_id.is_none()) {
            return None;
        }
        Some((d.system_id.clone(), d.external_id.clone(), t.doc_url(doc)))
    });
    if let Some((sys, pubid, url)) = need {
        let sys_uri = sys.as_ref().map(|s| match &url {
            Some(u) => crate::uri::build_uri(s, Some(u)).unwrap_or_else(|| s.clone()),
            None => s.clone(),
        });
        let (dtd, errs) = crate::sax2::parse_dtd(pubid.as_deref(), sys_uri.as_deref());
        errors.extend(errs);
        match dtd {
            Some(d) => crate::tree::with(|t| {
                t.set_tree_doc(d, Some(doc));
                if let Some(di) = t.doc_info_mut(doc) {
                    di.ext_subset = Some(d);
                }
            }),
            None => {
                let what = sys.or(pubid).unwrap_or_default();
                errors.push(XmlError::simple(
                    FROM_VALID,
                    XML_DTD_LOAD_ERROR,
                    ERR_ERROR,
                    format!("Could not load the external subset \"{}\"\n", what),
                ));
                return (false, errors);
            }
        }
    }
    let mut v = Validator::default();
    let ok = crate::tree::with(|t| {
        let mut cx = Cx::new(t, &mut v, Out::Collect(&mut errors), doc);
        cx.validate_document()
    });
    (ok, errors)
}
