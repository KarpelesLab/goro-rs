//! Port of Oniguruma 6.9.10 regcomp.c (tree tuning, optimization info and
//! byte-code generation).

#![allow(clippy::too_many_arguments)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::needless_range_loop)]

use super::enc::*;
use super::parse::*;
use super::regint::*;

/* ===== operations ===== */

pub const OPSIZE: i32 = 1;
pub const SIZE_INC: i32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckPositionType {
    SearchStart = 0,
    CurrentRightRange = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSegmentBoundaryType {
    ExtendedGraphemeCluster = 0,
    Word = 1,
}

#[derive(Clone, Debug)]
pub enum Op {
    Finish,
    End,
    /// OP_STR_* (exact byte string; the C variants only differ in speed)
    Str(Box<[u8]>),
    CClass(Box<BitSet>),
    CClassMb(Box<[(u32, u32)]>),
    CClassMix(Box<BitSet>, Box<[(u32, u32)]>),
    CClassNot(Box<BitSet>),
    CClassMbNot(Box<[(u32, u32)]>),
    CClassMixNot(Box<BitSet>, Box<[(u32, u32)]>),
    AnyChar,
    AnyCharMl,
    AnyCharStar,
    AnyCharMlStar,
    AnyCharStarPeekNext(u8),
    AnyCharMlStarPeekNext(u8),
    Word,
    WordAscii,
    NoWord,
    NoWordAscii,
    WordBoundary(bool),
    NoWordBoundary(bool),
    WordBegin(bool),
    WordEnd(bool),
    TextSegmentBoundary(TextSegmentBoundaryType, bool),
    BeginBuf,
    EndBuf,
    BeginLine,
    EndLine,
    SemiEndBuf,
    CheckPosition(CheckPositionType),
    Backref1,
    Backref2,
    BackrefN(i32),
    BackrefNIc(i32),
    BackrefMulti(Box<[i32]>),
    BackrefMultiIc(Box<[i32]>),
    BackrefWithLevel(Box<[i32]>, i32),
    BackrefWithLevelIc(Box<[i32]>, i32),
    BackrefCheck(Box<[i32]>),
    BackrefCheckWithLevel(Box<[i32]>, i32),
    MemStart(i32),
    MemStartPush(i32),
    MemEndPush(i32),
    MemEndPushRec(i32),
    MemEnd(i32),
    MemEndRec(i32),
    Fail,
    Jump(i32),
    Push(i32),
    PushSuper(i32),
    Pop,
    PopToMark(i32),
    PushOrJumpExact1(i32, u8),
    PushIfPeekNext(i32, u8),
    Repeat(i32, i32),
    RepeatNg(i32, i32),
    RepeatInc(i32),
    RepeatIncNg(i32),
    EmptyCheckStart(i32),
    EmptyCheckEnd(i32),
    EmptyCheckEndMemst(i32, MemStatusType),
    EmptyCheckEndMemstPush(i32, MemStatusType),
    Move(i32),
    /// (initial, remaining, addr)
    StepBackStart(i32, i32, i32),
    StepBackNext,
    /// (id, restore_pos)
    CutToMark(i32, bool),
    /// (id, save_pos)
    Mark(i32, bool),
    /// (type, id)
    SaveVal(i32, i32),
    /// (type, id, clear)
    UpdateVar(i32, i32, bool),
    Call(i32),
    Return,
    CalloutContents(i32),
    CalloutName(i32, i32),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RepeatRange {
    pub lower: i32,
    pub upper: i32,
    /// index of the repeated body's first op
    pub pcode: usize,
}

/* optimize flags */
pub const OPTIMIZE_NONE: i32 = 0;
pub const OPTIMIZE_STR: i32 = 1;
pub const OPTIMIZE_STR_FAST: i32 = 2;
pub const OPTIMIZE_STR_FAST_STEP_FORWARD: i32 = 3;
pub const OPTIMIZE_MAP: i32 = 4;

/* stack pop levels */
pub const STACK_POP_LEVEL_FREE: i32 = 0;
pub const STACK_POP_LEVEL_MEM_START: i32 = 1;
pub const STACK_POP_LEVEL_ALL: i32 = 2;

/// regex_t
pub struct Regex {
    pub ops: Vec<Op>,
    pub num_mem: i32,
    pub num_repeat: i32,
    pub num_empty_check: i32,
    pub num_call: i32,
    pub capture_history: MemStatusType,
    pub push_mem_start: MemStatusType,
    pub push_mem_end: MemStatusType,
    pub stack_pop_level: i32,
    pub repeat_range: Vec<RepeatRange>,

    pub enc: &'static Encoding,
    pub options: u32,
    pub syntax: &'static SyntaxType,
    pub case_fold_flag: u32,
    pub names: NameTable,

    pub optimize: i32,
    pub threshold_len: i32,
    pub anchor: i32,
    pub anc_dist_min: u32,
    pub anc_dist_max: u32,
    pub sub_anchor: i32,
    pub exact: Vec<u8>,
    pub map: [u8; CHAR_MAP_SIZE],
    pub map_offset: i32,
    pub dist_min: u32,
    pub dist_max: u32,
    pub callout_num: i32,
}

/* ===== length helpers ===== */

#[derive(Clone, Copy, Debug, Default)]
pub struct MinMaxLen {
    pub min: u32,
    pub max: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MinMaxCharLen {
    pub min: u32,
    pub max: u32,
    pub min_is_sure: bool,
}

pub fn distance_add(d1: u32, d2: u32) -> u32 {
    if d1 == INFINITE_LEN || d2 == INFINITE_LEN {
        INFINITE_LEN
    } else if d1 <= INFINITE_LEN - d2 {
        d1 + d2
    } else {
        INFINITE_LEN
    }
}

pub fn distance_multiply(d: u32, m: i32) -> u32 {
    if m == 0 {
        return 0;
    }
    if d < INFINITE_LEN / m as u32 {
        d * m as u32
    } else {
        INFINITE_LEN
    }
}

fn len_multiply_cmp(x: u32, y: i32, v: u32) -> i32 {
    if x == 0 || y == 0 {
        return -1;
    }
    if x < INFINITE_LEN / y as u32 {
        let xy = x * y as u32;
        if xy > v {
            1
        } else if xy == v {
            0
        } else {
            -1
        }
    } else if v == INFINITE_LEN {
        0
    } else {
        1
    }
}

fn mmcl_fixed(c: &MinMaxCharLen) -> bool {
    c.min == c.max && c.min != INFINITE_LEN
}

fn mmcl_set(l: &mut MinMaxCharLen, len: u32) {
    l.min = len;
    l.max = len;
    l.min_is_sure = true;
}

fn mmcl_set_min_max(l: &mut MinMaxCharLen, min: u32, max: u32, min_is_sure: bool) {
    l.min = min;
    l.max = max;
    l.min_is_sure = min_is_sure;
}

fn mmcl_add(to: &mut MinMaxCharLen, add: &MinMaxCharLen) {
    to.min = distance_add(to.min, add.min);
    to.max = distance_add(to.max, add.max);
    to.min_is_sure = add.min_is_sure && to.min_is_sure;
}

fn mmcl_multiply(to: &mut MinMaxCharLen, m: i32) {
    to.min = distance_multiply(to.min, m);
    to.max = distance_multiply(to.max, m);
}

fn mmcl_repeat_range_multiply(to: &mut MinMaxCharLen, mlow: i32, mhigh: i32) {
    to.min = distance_multiply(to.min, mlow);
    if is_infinite_repeat(mhigh) {
        to.max = INFINITE_LEN;
    } else {
        to.max = distance_multiply(to.max, mhigh);
    }
}

fn mmcl_alt_merge(to: &mut MinMaxCharLen, alt: &MinMaxCharLen) {
    if to.min > alt.min {
        to.min = alt.min;
        to.min_is_sure = alt.min_is_sure;
    } else if to.min == alt.min && alt.min_is_sure {
        to.min_is_sure = true;
    }
    if to.max < alt.max {
        to.max = alt.max;
    }
}

fn mml_is_equal(a: &MinMaxLen, b: &MinMaxLen) -> bool {
    a.min == b.min && a.max == b.max
}

fn mml_set_min_max(l: &mut MinMaxLen, min: u32, max: u32) {
    l.min = min;
    l.max = max;
}

fn mml_clear(l: &mut MinMaxLen) {
    l.min = 0;
    l.max = 0;
}

fn mml_add(to: &mut MinMaxLen, add: &MinMaxLen) {
    to.min = distance_add(to.min, add.min);
    to.max = distance_add(to.max, add.max);
}

fn mml_alt_merge(to: &mut MinMaxLen, alt: &MinMaxLen) {
    if to.min > alt.min {
        to.min = alt.min;
    }
    if to.max < alt.max {
        to.max = alt.max;
    }
}

pub const CHAR_LEN_NORMAL: i32 = 0;
pub const CHAR_LEN_TOP_ALT_FIXED: i32 = 1;

pub const QUANTIFIER_EXPAND_LIMIT_SIZE: u32 = 10;

#[inline]
fn is_real_ignorecase(nodes: &Nodes, n: NodeRef) -> bool {
    nodes.has(n, ND_ST_IGNORECASE) && !nodes.str_is_crude(n)
}

/// Accessors used by the tuning code
pub fn bag_typ(nodes: &Nodes, n: NodeRef) -> BagType {
    match &nodes.v[n].kind {
        NodeKind::Bag { typ, .. } => *typ,
        _ => panic!("not a bag"),
    }
}

pub fn bag_regnum(nodes: &Nodes, n: NodeRef) -> i32 {
    match &nodes.v[n].kind {
        NodeKind::Bag { regnum, .. } => *regnum,
        _ => panic!("not a bag"),
    }
}

pub fn bag_then_else(nodes: &Nodes, n: NodeRef) -> (NodeRef, NodeRef) {
    match &nodes.v[n].kind {
        NodeKind::Bag { then_, else_, .. } => (*then_, *else_),
        _ => panic!("not a bag"),
    }
}

pub fn anchor_typ(nodes: &Nodes, n: NodeRef) -> i32 {
    match &nodes.v[n].kind {
        NodeKind::Anchor { typ, .. } => *typ,
        _ => panic!("not an anchor"),
    }
}

#[inline]
pub fn anchor_has_body(typ: i32) -> bool {
    typ < ANCR_BEGIN_BUF
}

pub fn backrefs(nodes: &Nodes, n: NodeRef) -> &[i32] {
    match &nodes.v[n].kind {
        NodeKind::BackRef { back, .. } => back,
        _ => panic!("not a backref"),
    }
}

/* ===== compile context ===== */

pub struct Ctx<'e, 'a> {
    pub env: &'e mut ParseEnv<'a>,
    pub reg: Regex,
}

macro_rules! nd {
    ($s:expr) => {
        $s.env.nodes
    };
}

impl<'e, 'a> Ctx<'e, 'a> {
    #[inline]
    fn add_op(&mut self, op: Op) {
        self.reg.ops.push(op);
    }

    /// COP_CURR_OFFSET
    #[inline]
    fn cop_curr_offset(&self) -> i32 {
        self.reg.ops.len() as i32 - 1
    }

    /* fixed size pattern node only */
    fn node_char_len1(&mut self, node: NodeRef, ci: &mut MinMaxCharLen, mut level: i32) -> i32 {
        let mut tci = MinMaxCharLen::default();
        let mut r = CHAR_LEN_NORMAL;
        level += 1;

        match nd!(self).typ(node) {
            NodeType::List => {
                let mut first = true;
                let mut n = node;
                loop {
                    r = self.node_char_len1(nd!(self).car(n), &mut tci, level);
                    if r < 0 {
                        break;
                    }
                    if first {
                        *ci = tci;
                        first = false;
                    } else {
                        mmcl_add(ci, &tci);
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::Alt => {
                r = self.node_char_len1(nd!(self).car(node), ci, level);
                if r >= 0 {
                    let mut fixed = true;
                    let mut n = nd!(self).cdr(node);
                    while n != NULL_NODE {
                        r = self.node_char_len1(nd!(self).car(n), &mut tci, level);
                        if r < 0 {
                            break;
                        }
                        if !mmcl_fixed(&tci) {
                            fixed = false;
                        }
                        mmcl_alt_merge(ci, &tci);
                        n = nd!(self).cdr(n);
                    }
                    if r >= 0 {
                        r = CHAR_LEN_NORMAL;
                        if !mmcl_fixed(ci) && fixed && level == 1 {
                            r = CHAR_LEN_TOP_ALT_FIXED;
                        }
                    }
                }
            }
            NodeType::String => {
                if is_real_ignorecase(&nd!(self), node)
                    && self.env.case_fold_flag & ONIGENC_CASE_FOLD_ASCII_ONLY == 0
                {
                    r = ONIGERR_INVALID_LOOK_BEHIND_PATTERN;
                } else {
                    let s = nd!(self).str_bytes(node);
                    let mut clen = 0u32;
                    let mut i = 0;
                    while i < s.len() {
                        i += self.reg.enc.mbc_enc_len(s, i);
                        clen = distance_add(clen, 1);
                    }
                    mmcl_set(ci, clen);
                }
            }
            NodeType::Quant => {
                let (lower, upper, _) = quant_params(&nd!(self), node);
                if lower == upper {
                    if upper == 0 {
                        mmcl_set(ci, 0);
                    } else {
                        r = self.node_char_len1(nd!(self).body(node), ci, level);
                        if r >= 0 {
                            mmcl_multiply(ci, lower);
                        }
                    }
                } else {
                    r = self.node_char_len1(nd!(self).body(node), ci, level);
                    if r >= 0 {
                        mmcl_repeat_range_multiply(ci, lower, upper);
                    }
                }
            }
            NodeType::Call => {
                if nd!(self).has(node, ND_ST_RECURSION) {
                    mmcl_set_min_max(ci, 0, INFINITE_LEN, false);
                } else {
                    r = self.node_char_len1(nd!(self).body(node), ci, level);
                }
            }
            NodeType::CType | NodeType::CClass => mmcl_set(ci, 1),
            NodeType::Bag => match bag_typ(&nd!(self), node) {
                BagType::Memory => {
                    if nd!(self).has(node, ND_ST_FIXED_CLEN) {
                        let (mn, mx) = match &nd!(self).v[node].kind {
                            NodeKind::Bag {
                                min_char_len,
                                max_char_len,
                                ..
                            } => (*min_char_len, *max_char_len),
                            _ => unreachable!(),
                        };
                        let sure = nd!(self).has(node, ND_ST_FIXED_CLEN_MIN_SURE);
                        mmcl_set_min_max(ci, mn, mx, sure);
                    } else if nd!(self).has(node, ND_ST_MARK1) {
                        mmcl_set_min_max(ci, 0, INFINITE_LEN, false);
                    } else {
                        nd!(self).add(node, ND_ST_MARK1);
                        r = self.node_char_len1(nd!(self).body(node), ci, level);
                        nd!(self).remove(node, ND_ST_MARK1);
                        if r >= 0 {
                            if let NodeKind::Bag {
                                min_char_len,
                                max_char_len,
                                ..
                            } = &mut nd!(self).v[node].kind
                            {
                                *min_char_len = ci.min;
                                *max_char_len = ci.max;
                            }
                            nd!(self).add(node, ND_ST_FIXED_CLEN);
                            if ci.min_is_sure {
                                nd!(self).add(node, ND_ST_FIXED_CLEN_MIN_SURE);
                            }
                        }
                    }
                    if r >= 0 {
                        ci.min_is_sure = false;
                    }
                }
                BagType::Option | BagType::StopBacktrack => {
                    r = self.node_char_len1(nd!(self).body(node), ci, level);
                }
                BagType::IfElse => {
                    let mut eci = MinMaxCharLen::default();
                    r = self.node_char_len1(nd!(self).body(node), ci, level);
                    if r >= 0 {
                        let (then, els) = bag_then_else(&nd!(self), node);
                        if then != NULL_NODE {
                            r = self.node_char_len1(then, &mut tci, level);
                            if r >= 0 {
                                mmcl_add(ci, &tci);
                            }
                        }
                        if r >= 0 {
                            if els != NULL_NODE {
                                r = self.node_char_len1(els, &mut eci, level);
                            } else {
                                mmcl_set(&mut eci, 0);
                            }
                            if r >= 0 {
                                mmcl_alt_merge(ci, &eci);
                            }
                        }
                    }
                }
            },
            NodeType::Gimmick => mmcl_set(ci, 0),
            NodeType::Anchor => {
                mmcl_set(ci, 0);
                ci.min_is_sure = false;
            }
            NodeType::BackRef => {
                if nd!(self).has(node, ND_ST_CHECKER) {
                    mmcl_set(ci, 0);
                    ci.min_is_sure = false;
                } else if nd!(self).has(node, ND_ST_RECURSION) {
                    if nd!(self).has(node, ND_ST_NEST_LEVEL) {
                        mmcl_set_min_max(ci, 0, INFINITE_LEN, false);
                    } else {
                        mmcl_set_min_max(ci, 0, 0, false);
                    }
                } else {
                    let backs = backrefs(&nd!(self), node).to_vec();
                    let m0 = self.env.mem_node(backs[0]);
                    r = self.node_char_len1(m0, ci, level);
                    if r >= 0 {
                        if !mmcl_fixed(ci) {
                            ci.min_is_sure = false;
                        }
                        for &b in &backs[1..] {
                            let m = self.env.mem_node(b);
                            r = self.node_char_len1(m, &mut tci, level);
                            if r < 0 {
                                break;
                            }
                            if !mmcl_fixed(&tci) {
                                tci.min_is_sure = false;
                            }
                            mmcl_alt_merge(ci, &tci);
                        }
                    }
                }
            }
        }
        r
    }

    fn node_char_len(&mut self, node: NodeRef, ci: &mut MinMaxCharLen) -> i32 {
        self.node_char_len1(node, ci, 0)
    }

    /* ===== byte code generation ===== */

    fn compile_quant_body_with_empty_check(&mut self, qn: NodeRef) -> i32 {
        let (body, emptiness, empty_status_mem) = match &nd!(self).v[qn].kind {
            NodeKind::Quant {
                body,
                emptiness,
                empty_status_mem,
                ..
            } => (*body, *emptiness, *empty_status_mem),
            _ => unreachable!(),
        };
        let saved_num_empty_check = self.reg.num_empty_check;

        if emptiness != BodyEmpty::IsNotEmpty {
            self.add_op(Op::EmptyCheckStart(self.reg.num_empty_check));
            self.reg.num_empty_check += 1;
        }

        let r = self.compile_tree(body);
        if r != 0 {
            return r;
        }

        if emptiness != BodyEmpty::IsNotEmpty {
            match emptiness {
                BodyEmpty::MayBeEmpty => self.add_op(Op::EmptyCheckEnd(saved_num_empty_check)),
                BodyEmpty::MayBeEmptyMem => {
                    if nd!(self).has(qn, ND_ST_EMPTY_STATUS_CHECK) && empty_status_mem != 0 {
                        self.add_op(Op::EmptyCheckEndMemst(
                            saved_num_empty_check,
                            empty_status_mem,
                        ));
                    } else {
                        self.add_op(Op::EmptyCheckEnd(saved_num_empty_check));
                    }
                }
                BodyEmpty::MayBeEmptyRec => {
                    self.add_op(Op::EmptyCheckEndMemstPush(
                        saved_num_empty_check,
                        empty_status_mem,
                    ));
                }
                BodyEmpty::IsNotEmpty => {}
            }
        }
        0
    }

    fn compile_call(&mut self, node: NodeRef) -> i32 {
        self.add_op(Op::Call(0));
        let offset = self.cop_curr_offset() as usize;
        let target = nd!(self).body(node);
        self.env.unset_addr_list.push(UnsetAddr { offset, target });
        0
    }

    fn compile_tree_n_times(&mut self, node: NodeRef, n: i32) -> i32 {
        for _ in 0..n {
            let r = self.compile_tree(node);
            if r != 0 {
                return r;
            }
        }
        0
    }

    fn compile_length_string_node(&self, node: NodeRef) -> i32 {
        let s = nd!(self).str_bytes(node);
        if s.is_empty() {
            return 0;
        }
        let enc = self.reg.enc;
        let mut p = 0;
        let mut prev_len = enc.mbc_enc_len(s, p);
        p += prev_len;
        let mut rlen = 0;
        while p < s.len() {
            let len = enc.mbc_enc_len(s, p);
            if len != prev_len {
                rlen += 1;
                prev_len = len;
            }
            p += len;
        }
        rlen + 1
    }

    fn compile_length_string_crude_node(&self, node: NodeRef) -> i32 {
        if nd!(self).str_bytes(node).is_empty() {
            0
        } else {
            1
        }
    }

    fn add_compile_string(&mut self, s: &[u8]) {
        self.add_op(Op::Str(s.to_vec().into_boxed_slice()));
    }

    fn compile_string_node(&mut self, node: NodeRef) -> i32 {
        let s = nd!(self).str_bytes(node).to_vec();
        if s.is_empty() {
            return 0;
        }
        let enc = self.reg.enc;
        let mut p = 0;
        let mut prev = 0;
        let mut prev_len = enc.mbc_enc_len(&s, p);
        p += prev_len;
        let mut slen = 1;
        while p < s.len() {
            let len = enc.mbc_enc_len(&s, p);
            if len == prev_len {
                slen += 1;
            } else {
                let e = (prev + prev_len * slen).min(s.len());
                self.add_compile_string(&s[prev..e]);
                prev = p;
                slen = 1;
                prev_len = len;
            }
            p += len;
        }
        let e = (prev + prev_len * slen).min(s.len());
        self.add_compile_string(&s[prev..e]);
        0
    }

    fn compile_string_crude_node(&mut self, node: NodeRef) -> i32 {
        let s = nd!(self).str_bytes(node).to_vec();
        if s.is_empty() {
            return 0;
        }
        self.add_compile_string(&s);
        0
    }

    fn compile_cclass_node(&mut self, node: NodeRef) -> i32 {
        let cc = nd!(self).cclass(node).clone();
        match &cc.mbuf {
            None => {
                let bs = Box::new(cc.bs);
                self.add_op(if cc.not {
                    Op::CClassNot(bs)
                } else {
                    Op::CClass(bs)
                });
            }
            Some(mb) => {
                let mb: Box<[(u32, u32)]> = mb.clone().into_boxed_slice();
                if self.reg.enc.min_enc_len > 1 || bitset_is_empty(&cc.bs) {
                    self.add_op(if cc.not {
                        Op::CClassMbNot(mb)
                    } else {
                        Op::CClassMb(mb)
                    });
                } else {
                    let bs = Box::new(cc.bs);
                    self.add_op(if cc.not {
                        Op::CClassMixNot(bs, mb)
                    } else {
                        Op::CClassMix(bs, mb)
                    });
                }
            }
        }
        0
    }

    fn entry_repeat_range(&mut self, id: i32, lower: i32, upper: i32, ops_index: usize) {
        while self.reg.repeat_range.len() <= id as usize {
            self.reg.repeat_range.push(RepeatRange::default());
        }
        self.reg.repeat_range[id as usize] = RepeatRange {
            lower,
            upper: if is_infinite_repeat(upper) {
                0x7fffffff
            } else {
                upper
            },
            pcode: ops_index,
        };
    }

    fn compile_range_repeat_node(&mut self, qn: NodeRef, target_len: i32) -> i32 {
        let (lower, upper, greedy) = quant_params(&nd!(self), qn);
        let num_repeat = self.reg.num_repeat;
        self.reg.num_repeat += 1;

        let addr = SIZE_INC + target_len + OPSIZE;
        self.add_op(if greedy {
            Op::Repeat(num_repeat, addr)
        } else {
            Op::RepeatNg(num_repeat, addr)
        });
        let idx = (self.cop_curr_offset() + OPSIZE) as usize;
        self.entry_repeat_range(num_repeat, lower, upper, idx);

        let r = self.compile_quant_body_with_empty_check(qn);
        if r != 0 {
            return r;
        }
        self.add_op(if greedy {
            Op::RepeatInc(num_repeat)
        } else {
            Op::RepeatIncNg(num_repeat)
        });
        0
    }

    fn is_anychar_infinite_greedy(&self, qn: NodeRef) -> bool {
        let (_, upper, greedy) = quant_params(&nd!(self), qn);
        greedy && is_infinite_repeat(upper) && nd!(self).is_anychar(nd!(self).body(qn))
    }

    fn quant_extra(&self, qn: NodeRef) -> (BodyEmpty, NodeRef, NodeRef, bool) {
        match &nd!(self).v[qn].kind {
            NodeKind::Quant {
                emptiness,
                head_exact,
                next_head_exact,
                include_referred,
                ..
            } => (*emptiness, *head_exact, *next_head_exact, *include_referred),
            _ => unreachable!(),
        }
    }

    fn compile_length_quantifier_node(&mut self, qn: NodeRef) -> i32 {
        let (lower, upper, greedy) = quant_params(&nd!(self), qn);
        let (emptiness, head_exact, next_head_exact, include_referred) = self.quant_extra(qn);
        let infinite = is_infinite_repeat(upper);
        let tlen = self.compile_length_tree(nd!(self).body(qn));
        if tlen < 0 {
            return tlen;
        }
        if tlen == 0 {
            return 0;
        }

        if self.is_anychar_infinite_greedy(qn)
            && (lower <= 1
                || len_multiply_cmp(tlen as u32, lower, QUANTIFIER_EXPAND_LIMIT_SIZE) <= 0)
        {
            return OPSIZE + tlen * lower;
        }

        let mut mod_tlen = tlen;
        if emptiness != BodyEmpty::IsNotEmpty {
            mod_tlen += OPSIZE + OPSIZE;
        }

        let len;
        if infinite
            && (lower <= 1
                || len_multiply_cmp(tlen as u32, lower, QUANTIFIER_EXPAND_LIMIT_SIZE) <= 0)
        {
            let mut l = if lower == 1 && tlen > QUANTIFIER_EXPAND_LIMIT_SIZE as i32 {
                OPSIZE
            } else {
                tlen * lower
            };
            if greedy {
                let _ = (head_exact, next_head_exact);
                l += OPSIZE + mod_tlen + OPSIZE;
            } else {
                l += OPSIZE + mod_tlen + OPSIZE;
            }
            len = l;
        } else if upper == 0 {
            len = if include_referred { OPSIZE + tlen } else { 0 };
        } else if !infinite
            && greedy
            && (upper == 1
                || len_multiply_cmp(
                    tlen as u32 + OPSIZE as u32,
                    upper,
                    QUANTIFIER_EXPAND_LIMIT_SIZE,
                ) <= 0)
        {
            len = tlen * lower + (OPSIZE + tlen) * (upper - lower);
        } else if !greedy && upper == 1 && lower == 0 {
            len = OPSIZE + OPSIZE + tlen;
        } else {
            len = OPSIZE + mod_tlen + OPSIZE;
        }
        len
    }

    fn compile_quantifier_node(&mut self, qn: NodeRef) -> i32 {
        let (lower, upper, greedy) = quant_params(&nd!(self), qn);
        let (emptiness, head_exact, next_head_exact, include_referred) = self.quant_extra(qn);
        let infinite = is_infinite_repeat(upper);
        let body = nd!(self).body(qn);
        let tlen = self.compile_length_tree(body);
        if tlen < 0 {
            return tlen;
        }
        if tlen == 0 {
            return 0;
        }

        if self.is_anychar_infinite_greedy(qn)
            && (lower <= 1
                || len_multiply_cmp(tlen as u32, lower, QUANTIFIER_EXPAND_LIMIT_SIZE) <= 0)
        {
            let r = self.compile_tree_n_times(body, lower);
            if r != 0 {
                return r;
            }
            let ml = nd!(self).has(body, ND_ST_MULTILINE);
            if next_head_exact != NULL_NODE {
                let c = nd!(self).str_bytes(next_head_exact)[0];
                self.add_op(if ml {
                    Op::AnyCharMlStarPeekNext(c)
                } else {
                    Op::AnyCharStarPeekNext(c)
                });
            } else {
                self.add_op(if ml {
                    Op::AnyCharMlStar
                } else {
                    Op::AnyCharStar
                });
            }
            return 0;
        }

        let mut mod_tlen = tlen;
        if emptiness != BodyEmpty::IsNotEmpty {
            mod_tlen += OPSIZE + OPSIZE;
        }

        if infinite
            && (lower <= 1
                || len_multiply_cmp(tlen as u32, lower, QUANTIFIER_EXPAND_LIMIT_SIZE) <= 0)
        {
            if lower == 1 && tlen > QUANTIFIER_EXPAND_LIMIT_SIZE as i32 {
                let addr = if greedy {
                    OPSIZE + SIZE_INC
                } else {
                    OPSIZE + SIZE_INC
                };
                self.add_op(Op::Jump(addr));
            } else {
                let r = self.compile_tree_n_times(body, lower);
                if r != 0 {
                    return r;
                }
            }

            if greedy {
                let addr;
                if head_exact != NULL_NODE {
                    let c = nd!(self).str_bytes(head_exact)[0];
                    self.add_op(Op::PushOrJumpExact1(SIZE_INC + mod_tlen + OPSIZE, c));
                    let r = self.compile_quant_body_with_empty_check(qn);
                    if r != 0 {
                        return r;
                    }
                    addr = -(mod_tlen + OPSIZE);
                } else if next_head_exact != NULL_NODE {
                    let c = nd!(self).str_bytes(next_head_exact)[0];
                    self.add_op(Op::PushIfPeekNext(SIZE_INC + mod_tlen + OPSIZE, c));
                    let r = self.compile_quant_body_with_empty_check(qn);
                    if r != 0 {
                        return r;
                    }
                    addr = -(mod_tlen + OPSIZE);
                } else {
                    self.add_op(Op::Push(SIZE_INC + mod_tlen + OPSIZE));
                    let r = self.compile_quant_body_with_empty_check(qn);
                    if r != 0 {
                        return r;
                    }
                    addr = -(mod_tlen + OPSIZE);
                }
                self.add_op(Op::Jump(addr));
            } else {
                self.add_op(Op::Jump(mod_tlen + SIZE_INC));
                let r = self.compile_quant_body_with_empty_check(qn);
                if r != 0 {
                    return r;
                }
                self.add_op(Op::Push(-mod_tlen));
            }
        } else if upper == 0 {
            if include_referred {
                self.add_op(Op::Jump(tlen + SIZE_INC));
                let r = self.compile_tree(body);
                if r != 0 {
                    return r;
                }
            }
        } else if !infinite
            && greedy
            && (upper == 1
                || len_multiply_cmp(
                    tlen as u32 + OPSIZE as u32,
                    upper,
                    QUANTIFIER_EXPAND_LIMIT_SIZE,
                ) <= 0)
        {
            let n = upper - lower;
            let r = self.compile_tree_n_times(body, lower);
            if r != 0 {
                return r;
            }
            for i in 0..n {
                let v = positive_int_multiply(n - i, tlen + OPSIZE);
                if v < 0 {
                    return ONIGERR_TOO_BIG_NUMBER_FOR_REPEAT_RANGE;
                }
                self.add_op(Op::Push(v));
                let r = self.compile_tree(body);
                if r != 0 {
                    return r;
                }
            }
        } else if !greedy && upper == 1 && lower == 0 {
            self.add_op(Op::Push(SIZE_INC + OPSIZE));
            self.add_op(Op::Jump(tlen + SIZE_INC));
            let r = self.compile_tree(body);
            if r != 0 {
                return r;
            }
        } else {
            return self.compile_range_repeat_node(qn, mod_tlen);
        }
        0
    }

    fn bag_fields(&self, n: NodeRef) -> (BagType, i32, NodeRef) {
        match &nd!(self).v[n].kind {
            NodeKind::Bag {
                typ, regnum, body, ..
            } => (*typ, *regnum, *body),
            _ => unreachable!(),
        }
    }

    fn compile_length_bag_node(&mut self, node: NodeRef) -> i32 {
        let (typ, regnum, body) = self.bag_fields(node);
        if typ == BagType::Option {
            return self.compile_length_tree(body);
        }
        let mut tlen = 0;
        if body != NULL_NODE {
            tlen = self.compile_length_tree(body);
            if tlen < 0 {
                return tlen;
            }
        }
        let len;
        match typ {
            BagType::Memory => {
                let called = nd!(self).has(node, ND_ST_CALLED);
                let recursion = nd!(self).has(node, ND_ST_RECURSION);
                if regnum == 0 && called {
                    return tlen + OPSIZE + OPSIZE + OPSIZE;
                }
                if called {
                    len = OPSIZE + tlen + OPSIZE + OPSIZE + OPSIZE + OPSIZE;
                } else if recursion {
                    len = OPSIZE + tlen + OPSIZE;
                } else {
                    len = OPSIZE + tlen + OPSIZE;
                }
            }
            BagType::StopBacktrack => {
                if nd!(self).has(node, ND_ST_STRICT_REAL_REPEAT) {
                    let qn = body;
                    let (lower, _, _) = quant_params(&nd!(self), qn);
                    let tlen = self.compile_length_tree(nd!(self).body(qn));
                    if tlen < 0 {
                        return tlen;
                    }
                    let v = positive_int_multiply(lower, tlen);
                    if v < 0 {
                        return ONIGERR_TOO_BIG_NUMBER_FOR_REPEAT_RANGE;
                    }
                    len = v + OPSIZE + tlen + OPSIZE + OPSIZE;
                } else {
                    len = OPSIZE + tlen + OPSIZE;
                }
            }
            BagType::IfElse => {
                let (then, els) = bag_then_else(&nd!(self), node);
                let mut l = self.compile_length_tree(body);
                if l < 0 {
                    return l;
                }
                l += OPSIZE + OPSIZE + OPSIZE;
                if then != NULL_NODE {
                    let t = self.compile_length_tree(then);
                    if t < 0 {
                        return t;
                    }
                    l += t;
                }
                l += OPSIZE + OPSIZE;
                if els != NULL_NODE {
                    let t = self.compile_length_tree(els);
                    if t < 0 {
                        return t;
                    }
                    l += t;
                }
                len = l;
            }
            BagType::Option => len = 0,
        }
        len
    }

    fn compile_bag_memory_node(&mut self, node: NodeRef) -> i32 {
        let (_, regnum, body) = self.bag_fields(node);
        let called = nd!(self).has(node, ND_ST_CALLED);
        let recursion = nd!(self).has(node, ND_ST_RECURSION);

        if called {
            self.add_op(Op::Call(0));
            let called_addr = self.cop_curr_offset() + 1 + OPSIZE;
            if let NodeKind::Bag {
                called_addr: ca, ..
            } = &mut nd!(self).v[node].kind
            {
                *ca = called_addr;
            }
            nd!(self).add(node, ND_ST_FIXED_ADDR);
            let last = self.reg.ops.len() - 1;
            self.reg.ops[last] = Op::Call(called_addr);

            if regnum == 0 {
                let len = self.compile_length_tree(body) + OPSIZE;
                self.add_op(Op::Jump(len + SIZE_INC));
                let r = self.compile_tree(body);
                if r != 0 {
                    return r;
                }
                self.add_op(Op::Return);
                return 0;
            } else {
                let len = self.compile_length_tree(body) + OPSIZE + OPSIZE + OPSIZE;
                self.add_op(Op::Jump(len + SIZE_INC));
            }
        }

        if mem_status_at0(self.reg.push_mem_start, regnum) {
            self.add_op(Op::MemStartPush(regnum));
        } else {
            self.add_op(Op::MemStart(regnum));
        }

        let r = self.compile_tree(body);
        if r != 0 {
            return r;
        }

        if mem_status_at0(self.reg.push_mem_end, regnum) {
            self.add_op(if recursion {
                Op::MemEndPushRec(regnum)
            } else {
                Op::MemEndPush(regnum)
            });
        } else {
            self.add_op(if recursion {
                Op::MemEndRec(regnum)
            } else {
                Op::MemEnd(regnum)
            });
        }

        if called {
            self.add_op(Op::Return);
        }
        0
    }

    fn compile_bag_node(&mut self, node: NodeRef) -> i32 {
        let (typ, _, body) = self.bag_fields(node);
        match typ {
            BagType::Memory => self.compile_bag_memory_node(node),
            BagType::Option => self.compile_tree(body),
            BagType::StopBacktrack => {
                if nd!(self).has(node, ND_ST_STRICT_REAL_REPEAT) {
                    let qn = body;
                    let (lower, _, _) = quant_params(&nd!(self), qn);
                    let qbody = nd!(self).body(qn);
                    let r = self.compile_tree_n_times(qbody, lower);
                    if r != 0 {
                        return r;
                    }
                    let len = self.compile_length_tree(qbody);
                    if len < 0 {
                        return len;
                    }
                    self.add_op(Op::Push(SIZE_INC + len + OPSIZE + OPSIZE));
                    let r = self.compile_tree(qbody);
                    if r != 0 {
                        return r;
                    }
                    self.add_op(Op::Pop);
                    self.add_op(Op::Jump(-(OPSIZE + len + OPSIZE)));
                } else {
                    let mid = self.env.id_num;
                    self.env.id_num += 1;
                    self.add_op(Op::Mark(mid, false));
                    let r = self.compile_tree(body);
                    if r != 0 {
                        return r;
                    }
                    self.add_op(Op::CutToMark(mid, false));
                }
                0
            }
            BagType::IfElse => {
                let (then, els) = bag_then_else(&nd!(self), node);
                let cond = body;
                let mid = self.env.id_num;
                self.env.id_num += 1;
                self.add_op(Op::Mark(mid, false));

                let cond_len = self.compile_length_tree(cond);
                if cond_len < 0 {
                    return cond_len;
                }
                let then_len = if then != NULL_NODE {
                    let t = self.compile_length_tree(then);
                    if t < 0 {
                        return t;
                    }
                    t
                } else {
                    0
                };
                let jump_len = cond_len + then_len + OPSIZE + OPSIZE;
                self.add_op(Op::Push(SIZE_INC + jump_len));

                let r = self.compile_tree(cond);
                if r != 0 {
                    return r;
                }
                self.add_op(Op::CutToMark(mid, false));

                if then != NULL_NODE {
                    let r = self.compile_tree(then);
                    if r != 0 {
                        return r;
                    }
                }

                let else_len = if els != NULL_NODE {
                    let t = self.compile_length_tree(els);
                    if t < 0 {
                        return t;
                    }
                    t
                } else {
                    0
                };
                self.add_op(Op::Jump(OPSIZE + else_len + SIZE_INC));
                self.add_op(Op::CutToMark(mid, false));
                if els != NULL_NODE {
                    return self.compile_tree(els);
                }
                0
            }
        }
    }

    fn anchor_fields(&self, n: NodeRef) -> (i32, NodeRef, u32, u32, bool, NodeRef) {
        match &nd!(self).v[n].kind {
            NodeKind::Anchor {
                typ,
                body,
                char_min_len,
                char_max_len,
                ascii_mode,
                lead_node,
            } => (
                *typ,
                *body,
                *char_min_len,
                *char_max_len,
                *ascii_mode,
                *lead_node,
            ),
            _ => unreachable!(),
        }
    }

    fn compile_length_anchor_node(&mut self, node: NodeRef) -> i32 {
        let (typ, body, cmin, cmax, _, lead_node) = self.anchor_fields(node);
        let mut tlen = 0;
        if body != NULL_NODE {
            tlen = self.compile_length_tree(body);
            if tlen < 0 {
                return tlen;
            }
        }
        let has_stopper = self.env.flags & PE_FLAG_HAS_ABSENT_STOPPER != 0;
        match typ {
            ANCR_PREC_READ => OPSIZE + tlen + OPSIZE,
            ANCR_PREC_READ_NOT => OPSIZE * 5 + tlen,
            ANCR_LOOK_BEHIND => {
                if cmin == cmax {
                    OPSIZE * 3 + tlen
                } else {
                    let mut len = OPSIZE * 12 + tlen;
                    if lead_node != NULL_NODE {
                        let llen = self.compile_length_tree(lead_node);
                        if llen < 0 {
                            return llen;
                        }
                        len += OPSIZE + llen;
                    }
                    if has_stopper {
                        len += OPSIZE * 2;
                    }
                    len
                }
            }
            ANCR_LOOK_BEHIND_NOT => {
                if cmin == cmax {
                    OPSIZE * 6 + tlen
                } else {
                    let mut len = OPSIZE * 14 + tlen;
                    if lead_node != NULL_NODE {
                        let llen = self.compile_length_tree(lead_node);
                        if llen < 0 {
                            return llen;
                        }
                        len += OPSIZE + llen;
                    }
                    if has_stopper {
                        len += OPSIZE * 2;
                    }
                    len
                }
            }
            _ => OPSIZE,
        }
    }

    fn compile_anchor_look_behind_node(&mut self, node: NodeRef) -> i32 {
        let (_, body, cmin, cmax, _, lead_node) = self.anchor_fields(node);
        let has_stopper = self.env.flags & PE_FLAG_HAS_ABSENT_STOPPER != 0;
        if cmin == cmax {
            let mid = self.env.id_num;
            self.env.id_num += 1;
            self.add_op(Op::Mark(mid, false));
            self.add_op(Op::StepBackStart(cmin as i32, 0, 1));
            let r = self.compile_tree(body);
            if r != 0 {
                return r;
            }
            self.add_op(Op::CutToMark(mid, false));
        } else {
            if lead_node != NULL_NODE {
                let mut ci = MinMaxCharLen::default();
                let r = self.node_char_len(lead_node, &mut ci);
                if r < 0 {
                    return r;
                }
                self.add_op(Op::Move(-(ci.min as i32)));
                let r = self.compile_tree(lead_node);
                if r != 0 {
                    return r;
                }
            }
            let mid1 = self.env.id_num;
            self.env.id_num += 1;
            self.add_op(Op::SaveVal(SAVE_RIGHT_RANGE, mid1));
            self.add_op(Op::UpdateVar(UPDATE_VAR_RIGHT_RANGE_TO_S, 0, false));
            let mid2 = self.env.id_num;
            self.env.id_num += 1;
            self.add_op(Op::Mark(mid2, false));
            self.add_op(Op::Push(SIZE_INC + OPSIZE));
            self.add_op(Op::Jump(SIZE_INC + OPSIZE + OPSIZE));
            self.add_op(Op::UpdateVar(
                UPDATE_VAR_RIGHT_RANGE_FROM_STACK,
                mid1,
                false,
            ));
            self.add_op(Op::Fail);
            let mut mid3 = 0;
            if has_stopper {
                mid3 = self.env.id_num;
                self.env.id_num += 1;
                self.add_op(Op::SaveVal(SAVE_RIGHT_RANGE, mid3));
            }
            let diff = if cmax != INFINITE_LEN {
                (cmax - cmin) as i32
            } else {
                INFINITE_LEN as i32
            };
            self.add_op(Op::StepBackStart(cmin as i32, diff, 2));
            self.add_op(Op::StepBackNext);
            let r = self.compile_tree(body);
            if r != 0 {
                return r;
            }
            if has_stopper {
                self.add_op(Op::UpdateVar(
                    UPDATE_VAR_RIGHT_RANGE_FROM_STACK,
                    mid3,
                    false,
                ));
            }
            self.add_op(Op::CheckPosition(CheckPositionType::CurrentRightRange));
            self.add_op(Op::CutToMark(mid2, false));
            self.add_op(Op::UpdateVar(UPDATE_VAR_RIGHT_RANGE_FROM_STACK, mid1, true));
        }
        0
    }

    fn compile_anchor_look_behind_not_node(&mut self, node: NodeRef) -> i32 {
        let (_, body, cmin, cmax, _, lead_node) = self.anchor_fields(node);
        let has_stopper = self.env.flags & PE_FLAG_HAS_ABSENT_STOPPER != 0;
        let len = self.compile_length_tree(body);

        if cmin == cmax {
            let mid = self.env.id_num;
            self.env.id_num += 1;
            self.add_op(Op::Mark(mid, false));
            self.add_op(Op::Push(SIZE_INC + OPSIZE + len + OPSIZE + OPSIZE));
            self.add_op(Op::StepBackStart(cmin as i32, 0, 1));
            let r = self.compile_tree(body);
            if r != 0 {
                return r;
            }
            self.add_op(Op::PopToMark(mid));
            self.add_op(Op::Fail);
            self.add_op(Op::Pop);
        } else {
            let mid1 = self.env.id_num;
            self.env.id_num += 1;
            self.add_op(Op::SaveVal(SAVE_RIGHT_RANGE, mid1));
            self.add_op(Op::UpdateVar(UPDATE_VAR_RIGHT_RANGE_TO_S, 0, false));
            let mid2 = self.env.id_num;
            self.env.id_num += 1;
            self.add_op(Op::Mark(mid2, false));

            let mut push_addr = SIZE_INC + OPSIZE * 7 + len;
            if has_stopper {
                push_addr += OPSIZE * 2;
            }
            self.add_op(Op::Push(push_addr));
            let push_idx = self.reg.ops.len() - 1;

            if lead_node != NULL_NODE {
                let clen = self.compile_length_tree(lead_node);
                push_addr += OPSIZE + clen;
                self.reg.ops[push_idx] = Op::Push(push_addr);
                let mut ci = MinMaxCharLen::default();
                let r = self.node_char_len(lead_node, &mut ci);
                if r < 0 {
                    return r;
                }
                self.add_op(Op::Move(-(ci.min as i32)));
                let r = self.compile_tree(lead_node);
                if r != 0 {
                    return r;
                }
            }

            let mut mid3 = 0;
            if has_stopper {
                mid3 = self.env.id_num;
                self.env.id_num += 1;
                self.add_op(Op::SaveVal(SAVE_RIGHT_RANGE, mid3));
            }

            let diff = if cmax != INFINITE_LEN {
                (cmax - cmin) as i32
            } else {
                INFINITE_LEN as i32
            };
            self.add_op(Op::StepBackStart(cmin as i32, diff, 2));
            self.add_op(Op::StepBackNext);
            let r = self.compile_tree(body);
            if r != 0 {
                return r;
            }
            if has_stopper {
                self.add_op(Op::UpdateVar(
                    UPDATE_VAR_RIGHT_RANGE_FROM_STACK,
                    mid3,
                    false,
                ));
            }
            self.add_op(Op::CheckPosition(CheckPositionType::CurrentRightRange));
            self.add_op(Op::PopToMark(mid2));
            self.add_op(Op::UpdateVar(
                UPDATE_VAR_RIGHT_RANGE_FROM_STACK,
                mid1,
                false,
            ));
            self.add_op(Op::Pop);
            self.add_op(Op::Fail);
            self.add_op(Op::UpdateVar(
                UPDATE_VAR_RIGHT_RANGE_FROM_STACK,
                mid1,
                false,
            ));
            self.add_op(Op::Pop);
            self.add_op(Op::Pop);
        }
        0
    }

    fn compile_anchor_node(&mut self, node: NodeRef) -> i32 {
        let (typ, body, _, _, ascii_mode, _) = self.anchor_fields(node);
        match typ {
            ANCR_BEGIN_BUF => self.add_op(Op::BeginBuf),
            ANCR_END_BUF => self.add_op(Op::EndBuf),
            ANCR_BEGIN_LINE => self.add_op(Op::BeginLine),
            ANCR_END_LINE => self.add_op(Op::EndLine),
            ANCR_SEMI_END_BUF => self.add_op(Op::SemiEndBuf),
            ANCR_BEGIN_POSITION => self.add_op(Op::CheckPosition(CheckPositionType::SearchStart)),
            ANCR_WORD_BOUNDARY => self.add_op(Op::WordBoundary(ascii_mode)),
            ANCR_NO_WORD_BOUNDARY => self.add_op(Op::NoWordBoundary(ascii_mode)),
            ANCR_WORD_BEGIN => self.add_op(Op::WordBegin(ascii_mode)),
            ANCR_WORD_END => self.add_op(Op::WordEnd(ascii_mode)),
            ANCR_TEXT_SEGMENT_BOUNDARY | ANCR_NO_TEXT_SEGMENT_BOUNDARY => {
                let t = if nd!(self).has(node, ND_ST_TEXT_SEGMENT_WORD) {
                    TextSegmentBoundaryType::Word
                } else {
                    TextSegmentBoundaryType::ExtendedGraphemeCluster
                };
                self.add_op(Op::TextSegmentBoundary(
                    t,
                    typ == ANCR_NO_TEXT_SEGMENT_BOUNDARY,
                ));
            }
            ANCR_PREC_READ => {
                let mid = self.env.id_num;
                self.env.id_num += 1;
                self.add_op(Op::Mark(mid, true));
                let r = self.compile_tree(body);
                if r != 0 {
                    return r;
                }
                self.add_op(Op::CutToMark(mid, true));
            }
            ANCR_PREC_READ_NOT => {
                let len = self.compile_length_tree(body);
                if len < 0 {
                    return len;
                }
                let mid = self.env.id_num;
                self.env.id_num += 1;
                self.add_op(Op::Push(SIZE_INC + OPSIZE + len + OPSIZE + OPSIZE + OPSIZE));
                self.add_op(Op::Mark(mid, false));
                let r = self.compile_tree(body);
                if r != 0 {
                    return r;
                }
                self.add_op(Op::PopToMark(mid));
                self.add_op(Op::Pop);
                self.add_op(Op::Fail);
            }
            ANCR_LOOK_BEHIND => return self.compile_anchor_look_behind_node(node),
            ANCR_LOOK_BEHIND_NOT => return self.compile_anchor_look_behind_not_node(node),
            _ => return ONIGERR_TYPE_BUG,
        }
        0
    }

    fn compile_gimmick_node(&mut self, node: NodeRef) -> i32 {
        let (typ, detail_type, num, id) = match &nd!(self).v[node].kind {
            NodeKind::Gimmick {
                typ,
                detail_type,
                num,
                id,
            } => (*typ, *detail_type, *num, *id),
            _ => unreachable!(),
        };
        match typ {
            GimmickType::Fail => self.add_op(Op::Fail),
            GimmickType::Save => self.add_op(Op::SaveVal(detail_type, id)),
            GimmickType::UpdateVar => self.add_op(Op::UpdateVar(detail_type, id, false)),
            GimmickType::Callout => {
                if detail_type == ONIG_CALLOUT_OF_NAME {
                    self.add_op(Op::CalloutName(num, id));
                } else {
                    self.add_op(Op::CalloutContents(num));
                }
            }
        }
        0
    }

    pub fn compile_length_tree(&mut self, node: NodeRef) -> i32 {
        match nd!(self).typ(node) {
            NodeType::List => {
                let mut len = 0;
                let mut n = node;
                loop {
                    let r = self.compile_length_tree(nd!(self).car(n));
                    if r < 0 {
                        return r;
                    }
                    len += r;
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                len
            }
            NodeType::Alt => {
                let mut r = 0;
                let mut cnt = 0;
                let mut n = node;
                loop {
                    r += self.compile_length_tree(nd!(self).car(n));
                    cnt += 1;
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                r + (OPSIZE + OPSIZE) * (cnt - 1)
            }
            NodeType::String => {
                if nd!(self).str_is_crude(node) {
                    self.compile_length_string_crude_node(node)
                } else {
                    self.compile_length_string_node(node)
                }
            }
            NodeType::CClass => OPSIZE,
            NodeType::CType => OPSIZE,
            NodeType::BackRef => OPSIZE,
            NodeType::Call => OPSIZE,
            NodeType::Quant => self.compile_length_quantifier_node(node),
            NodeType::Bag => self.compile_length_bag_node(node),
            NodeType::Anchor => self.compile_length_anchor_node(node),
            NodeType::Gimmick => OPSIZE,
        }
    }

    pub fn compile_tree(&mut self, node: NodeRef) -> i32 {
        match nd!(self).typ(node) {
            NodeType::List => {
                let mut n = node;
                loop {
                    let r = self.compile_tree(nd!(self).car(n));
                    if r != 0 {
                        return r;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::Alt => {
                let mut len = 0;
                let mut x = node;
                loop {
                    len += self.compile_length_tree(nd!(self).car(x));
                    if nd!(self).cdr(x) != NULL_NODE {
                        len += OPSIZE + OPSIZE;
                    }
                    x = nd!(self).cdr(x);
                    if x == NULL_NODE {
                        break;
                    }
                }
                let pos = self.cop_curr_offset() + 1 + len;
                let mut n = node;
                loop {
                    let len = self.compile_length_tree(nd!(self).car(n));
                    if nd!(self).cdr(n) != NULL_NODE {
                        let addr = SIZE_INC + len + OPSIZE;
                        if nd!(self).has(n, ND_ST_SUPER) {
                            self.add_op(Op::PushSuper(addr));
                        } else {
                            self.add_op(Op::Push(addr));
                        }
                    }
                    let r = self.compile_tree(nd!(self).car(n));
                    if r != 0 {
                        return r;
                    }
                    if nd!(self).cdr(n) != NULL_NODE {
                        let len = pos - (self.cop_curr_offset() + 1);
                        self.add_op(Op::Jump(len));
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::String => {
                if nd!(self).str_is_crude(node) {
                    self.compile_string_crude_node(node)
                } else {
                    self.compile_string_node(node)
                }
            }
            NodeType::CClass => self.compile_cclass_node(node),
            NodeType::CType => {
                let (ctype, not, ascii_mode) = match &nd!(self).v[node].kind {
                    NodeKind::CType {
                        ctype,
                        not,
                        ascii_mode,
                    } => (*ctype, *not, *ascii_mode),
                    _ => unreachable!(),
                };
                if ctype == CTYPE_ANYCHAR {
                    if nd!(self).has(node, ND_ST_MULTILINE) {
                        self.add_op(Op::AnyCharMl);
                    } else {
                        self.add_op(Op::AnyChar);
                    }
                } else if ctype == ONIGENC_CTYPE_WORD as i32 {
                    self.add_op(match (ascii_mode, not) {
                        (false, false) => Op::Word,
                        (false, true) => Op::NoWord,
                        (true, false) => Op::WordAscii,
                        (true, true) => Op::NoWordAscii,
                    });
                } else {
                    return ONIGERR_TYPE_BUG;
                }
                0
            }
            NodeType::BackRef => {
                let (back, nest_level) = match &nd!(self).v[node].kind {
                    NodeKind::BackRef { back, nest_level } => (back.clone(), *nest_level),
                    _ => unreachable!(),
                };
                let ic = nd!(self).has(node, ND_ST_IGNORECASE);
                let nl = nd!(self).has(node, ND_ST_NEST_LEVEL);
                /* backref_general.ns is stored in reverse order */
                let rev: Box<[i32]> = back
                    .iter()
                    .rev()
                    .copied()
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                if nd!(self).has(node, ND_ST_CHECKER) {
                    if nl {
                        self.add_op(Op::BackrefCheckWithLevel(rev, nest_level));
                    } else {
                        self.add_op(Op::BackrefCheck(rev));
                    }
                } else if nl {
                    if ic {
                        self.add_op(Op::BackrefWithLevelIc(rev, nest_level));
                    } else {
                        self.add_op(Op::BackrefWithLevel(rev, nest_level));
                    }
                } else if back.len() == 1 {
                    let n = back[0];
                    if ic {
                        self.add_op(Op::BackrefNIc(n));
                    } else {
                        match n {
                            1 => self.add_op(Op::Backref1),
                            2 => self.add_op(Op::Backref2),
                            _ => self.add_op(Op::BackrefN(n)),
                        }
                    }
                } else if ic {
                    self.add_op(Op::BackrefMultiIc(rev));
                } else {
                    self.add_op(Op::BackrefMulti(rev));
                }
                0
            }
            NodeType::Call => self.compile_call(node),
            NodeType::Quant => self.compile_quantifier_node(node),
            NodeType::Bag => self.compile_bag_node(node),
            NodeType::Anchor => self.compile_anchor_node(node),
            NodeType::Gimmick => self.compile_gimmick_node(node),
        }
    }
}

/* ===== named group renumbering ===== */

impl<'e, 'a> Ctx<'e, 'a> {
    /// make_named_capture_number_map; plink is (parent, which-link)
    fn make_named_capture_number_map(
        &mut self,
        node: NodeRef,
        map: &mut [i32],
        counter: &mut i32,
    ) -> Result<(i32, NodeRef), i32> {
        // returns (r, replacement node for *plink)
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let car = nd!(self).car(n);
                    let (r, nn) = self.make_named_capture_number_map(car, map, counter)?;
                    let _ = r;
                    nd!(self).set_car(n, nn);
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                Ok((0, node))
            }
            NodeType::Quant => {
                let body = nd!(self).body(node);
                let (r, nb) = self.make_named_capture_number_map(body, map, counter)?;
                nd!(self).set_body(node, nb);
                if r == 1 && nd!(self).typ(nb) == NodeType::Quant {
                    let r2 = reduce_nested_quantifier(&mut nd!(self), node);
                    if r2 < 0 {
                        return Err(r2);
                    }
                    return Ok((r2, node));
                }
                Ok((0, node))
            }
            NodeType::Bag => {
                let (typ, regnum, body) = self.bag_fields(node);
                if typ == BagType::Memory {
                    if nd!(self).has(node, ND_ST_NAMED_GROUP) {
                        *counter += 1;
                        map[regnum as usize] = *counter;
                        if let NodeKind::Bag { regnum: rn, .. } = &mut nd!(self).v[node].kind {
                            *rn = *counter;
                        }
                        let (_, nb) = self.make_named_capture_number_map(body, map, counter)?;
                        nd!(self).set_body(node, nb);
                        Ok((0, node))
                    } else {
                        let (_, nb) = self.make_named_capture_number_map(body, map, counter)?;
                        Ok((1, nb))
                    }
                } else if typ == BagType::IfElse {
                    let (_, nb) = self.make_named_capture_number_map(body, map, counter)?;
                    nd!(self).set_body(node, nb);
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let (_, nt) = self.make_named_capture_number_map(then, map, counter)?;
                        if let NodeKind::Bag { then_, .. } = &mut nd!(self).v[node].kind {
                            *then_ = nt;
                        }
                    }
                    if els != NULL_NODE {
                        let (_, ne) = self.make_named_capture_number_map(els, map, counter)?;
                        if let NodeKind::Bag { else_, .. } = &mut nd!(self).v[node].kind {
                            *else_ = ne;
                        }
                    }
                    Ok((0, node))
                } else {
                    let (_, nb) = self.make_named_capture_number_map(body, map, counter)?;
                    nd!(self).set_body(node, nb);
                    Ok((0, node))
                }
            }
            NodeType::Anchor => {
                let body = nd!(self).body(node);
                if body != NULL_NODE {
                    let (_, nb) = self.make_named_capture_number_map(body, map, counter)?;
                    nd!(self).set_body(node, nb);
                }
                Ok((0, node))
            }
            _ => Ok((0, node)),
        }
    }

    fn renumber_backref_traverse(&mut self, node: NodeRef, map: &[i32]) -> i32 {
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let r = self.renumber_backref_traverse(nd!(self).car(n), map);
                    if r != 0 {
                        return r;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::Quant => self.renumber_backref_traverse(nd!(self).body(node), map),
            NodeType::Bag => {
                let r = self.renumber_backref_traverse(nd!(self).body(node), map);
                if r != 0 {
                    return r;
                }
                if bag_typ(&nd!(self), node) == BagType::IfElse {
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let r = self.renumber_backref_traverse(then, map);
                        if r != 0 {
                            return r;
                        }
                    }
                    if els != NULL_NODE {
                        let r = self.renumber_backref_traverse(els, map);
                        if r != 0 {
                            return r;
                        }
                    }
                }
                0
            }
            NodeType::BackRef => {
                if !nd!(self).has(node, ND_ST_BY_NAME) {
                    return ONIGERR_NUMBERED_BACKREF_OR_CALL_NOT_ALLOWED;
                }
                if let NodeKind::BackRef { back, .. } = &mut nd!(self).v[node].kind {
                    let mut nb = Vec::new();
                    for &b in back.iter() {
                        let n = map[b as usize];
                        if n > 0 {
                            nb.push(n);
                        }
                    }
                    *back = nb;
                }
                0
            }
            NodeType::Anchor => {
                let body = nd!(self).body(node);
                if body != NULL_NODE {
                    self.renumber_backref_traverse(body, map)
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    fn numbered_ref_check(&self, node: NodeRef) -> i32 {
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let r = self.numbered_ref_check(nd!(self).car(n));
                    if r != 0 {
                        return r;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::Anchor => {
                let body = nd!(self).body(node);
                if body == NULL_NODE {
                    0
                } else {
                    self.numbered_ref_check(body)
                }
            }
            NodeType::Quant => self.numbered_ref_check(nd!(self).body(node)),
            NodeType::Bag => {
                let r = self.numbered_ref_check(nd!(self).body(node));
                if r != 0 {
                    return r;
                }
                if bag_typ(&nd!(self), node) == BagType::IfElse {
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let r = self.numbered_ref_check(then);
                        if r != 0 {
                            return r;
                        }
                    }
                    if els != NULL_NODE {
                        let r = self.numbered_ref_check(els);
                        if r != 0 {
                            return r;
                        }
                    }
                }
                0
            }
            NodeType::BackRef => {
                if !nd!(self).has(node, ND_ST_BY_NAME) {
                    ONIGERR_NUMBERED_BACKREF_OR_CALL_NOT_ALLOWED
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    fn disable_noname_group_capture(&mut self, root: &mut NodeRef) -> i32 {
        let num_mem = self.env.num_mem as usize;
        let mut map = vec![0i32; num_mem + 1];
        let mut counter = 0;
        match self.make_named_capture_number_map(*root, &mut map, &mut counter) {
            Ok((_, nr)) => *root = nr,
            Err(e) => return e,
        }
        let r = self.renumber_backref_traverse(*root, &map);
        if r != 0 {
            return r;
        }
        let mut pos = 1;
        for i in 1..=num_mem {
            if map[i] > 0 {
                self.env.mem_env[pos] = self.env.mem_env[i];
                pos += 1;
            }
        }
        let loc = self.env.cap_history;
        self.env.cap_history = 0;
        for i in 1..=ONIG_MAX_CAPTURE_HISTORY_GROUP {
            if mem_status_at(loc, i) && (i as usize) < map.len() {
                mem_status_on_simple(&mut self.env.cap_history, map[i as usize]);
            }
        }
        self.env.num_mem = self.env.num_named;
        self.reg.num_mem = self.env.num_named;

        /* onig_renumber_name_table */
        for e in self.env.names.entries.iter_mut() {
            for b in e.back_refs.iter_mut() {
                *b = map[*b as usize];
            }
        }
        0
    }

    fn fix_unset_addr_list(&mut self) -> i32 {
        let list = std::mem::take(&mut self.env.unset_addr_list);
        for us in &list {
            if !nd!(self).has(us.target, ND_ST_FIXED_ADDR) {
                if nd!(self).has(us.target, ND_ST_CALLED) {
                    return ONIGERR_PARSER_BUG;
                }
                continue;
            }
            let addr = match &nd!(self).v[us.target].kind {
                NodeKind::Bag { called_addr, .. } => *called_addr,
                _ => return ONIGERR_PARSER_BUG,
            };
            self.reg.ops[us.offset] = Op::Call(addr);
        }
        self.env.unset_addr_list = list;
        0
    }

    /* ===== exclusiveness / head literals ===== */

    /// x is not included y ==> true
    fn is_exclusive(&self, mut x: NodeRef, mut y: NodeRef) -> bool {
        let enc = self.reg.enc;
        loop {
            let ytype = nd!(self).typ(y);
            match nd!(self).typ(x) {
                NodeType::CType => {
                    let (xct, xnot, xam) = ctype_fields(&nd!(self), x);
                    if xct == CTYPE_ANYCHAR {
                        return false;
                    }
                    match ytype {
                        NodeType::CType => {
                            let (yct, ynot, yam) = ctype_fields(&nd!(self), y);
                            if yct == CTYPE_ANYCHAR {
                                return false;
                            }
                            return yct == xct && ynot != xnot && yam == xam;
                        }
                        NodeType::CClass | NodeType::String => {
                            std::mem::swap(&mut x, &mut y);
                            continue;
                        }
                        _ => return false,
                    }
                }
                NodeType::CClass => {
                    let xc = nd!(self).cclass(x);
                    match ytype {
                        NodeType::CType => {
                            let (yct, ynot, yam) = ctype_fields(&nd!(self), y);
                            if yct == CTYPE_ANYCHAR {
                                return false;
                            }
                            if yct == ONIGENC_CTYPE_WORD as i32 {
                                let range = if yam { 128 } else { SINGLE_BYTE_SIZE };
                                if !ynot {
                                    if xc.mbuf.is_none() && !xc.not {
                                        for i in 0..range {
                                            if bitset_at(&xc.bs, i) && enc.is_code_word(i) {
                                                return false;
                                            }
                                        }
                                        return true;
                                    }
                                    return false;
                                } else {
                                    if xc.mbuf.is_some() {
                                        return false;
                                    }
                                    if xc.not {
                                        return false;
                                    }
                                    for i in 0..range {
                                        if !enc.is_code_word(i) && bitset_at(&xc.bs, i) {
                                            return false;
                                        }
                                    }
                                    for i in range..SINGLE_BYTE_SIZE {
                                        if bitset_at(&xc.bs, i) {
                                            return false;
                                        }
                                    }
                                    return true;
                                }
                            }
                            return false;
                        }
                        NodeType::CClass => {
                            let yc = nd!(self).cclass(y);
                            for i in 0..SINGLE_BYTE_SIZE {
                                let v = bitset_at(&xc.bs, i);
                                if (v && !xc.not) || (!v && xc.not) {
                                    let v = bitset_at(&yc.bs, i);
                                    if (v && !yc.not) || (!v && yc.not) {
                                        return false;
                                    }
                                }
                            }
                            return (xc.mbuf.is_none() && !xc.not)
                                || (yc.mbuf.is_none() && !yc.not);
                        }
                        NodeType::String => {
                            std::mem::swap(&mut x, &mut y);
                            continue;
                        }
                        _ => return false,
                    }
                }
                NodeType::String => {
                    let xs = nd!(self).str_bytes(x);
                    if xs.is_empty() {
                        return false;
                    }
                    match ytype {
                        NodeType::CType => {
                            let (yct, ynot, yam) = ctype_fields(&nd!(self), y);
                            if yct == CTYPE_ANYCHAR {
                                return false;
                            }
                            if yct == ONIGENC_CTYPE_WORD as i32 {
                                let w = if !yam {
                                    enc.is_mbc_word(xs, 0, xs.len())
                                } else {
                                    is_mbc_word_ascii(enc, xs, 0, xs.len())
                                };
                                return if w { ynot } else { !ynot };
                            }
                            return false;
                        }
                        NodeType::CClass => {
                            let cc = nd!(self).cclass(y);
                            let lim = enc.max_enc_len.min(xs.len());
                            let code = enc.mbc_to_code(xs, 0, lim);
                            return !is_code_in_cc(enc, code, cc);
                        }
                        NodeType::String => {
                            let ys = nd!(self).str_bytes(y);
                            let len = xs.len().min(ys.len());
                            for i in 0..len {
                                if ys[i] != xs[i] {
                                    return true;
                                }
                            }
                            return false;
                        }
                        _ => return false,
                    }
                }
                _ => return false,
            }
        }
    }

    fn get_tree_head_literal(&self, node: NodeRef, exact: bool) -> NodeRef {
        match nd!(self).typ(node) {
            NodeType::BackRef | NodeType::Alt | NodeType::Call => NULL_NODE,
            NodeType::CType => {
                let (ct, _, _) = ctype_fields(&nd!(self), node);
                if ct == CTYPE_ANYCHAR || exact {
                    NULL_NODE
                } else {
                    node
                }
            }
            NodeType::CClass => {
                if !exact {
                    node
                } else {
                    NULL_NODE
                }
            }
            NodeType::List => self.get_tree_head_literal(nd!(self).car(node), exact),
            NodeType::String => {
                if nd!(self).str_bytes(node).is_empty() {
                    return NULL_NODE;
                }
                if !exact || !is_real_ignorecase(&nd!(self), node) {
                    node
                } else {
                    NULL_NODE
                }
            }
            NodeType::Quant => {
                let (lower, _, _) = quant_params(&nd!(self), node);
                if lower > 0 {
                    let (_, head_exact, _, _) = self.quant_extra(node);
                    if head_exact != NULL_NODE {
                        head_exact
                    } else {
                        self.get_tree_head_literal(nd!(self).body(node), exact)
                    }
                } else {
                    NULL_NODE
                }
            }
            NodeType::Bag => match bag_typ(&nd!(self), node) {
                BagType::Option | BagType::Memory | BagType::StopBacktrack => {
                    self.get_tree_head_literal(nd!(self).body(node), exact)
                }
                _ => NULL_NODE,
            },
            NodeType::Anchor => {
                if anchor_typ(&nd!(self), node) == ANCR_PREC_READ {
                    self.get_tree_head_literal(nd!(self).body(node), exact)
                } else {
                    NULL_NODE
                }
            }
            NodeType::Gimmick => NULL_NODE,
        }
    }

    /// GET_VALUE_NONE -1, IGNORE 0, FOUND 1
    fn get_tree_tail_literal(
        &mut self,
        node: NodeRef,
        rnode: &mut NodeRef,
        mut nest_level: i32,
    ) -> i32 {
        nest_level += 1;
        if nest_level >= 16 {
            return -1;
        }
        match nd!(self).typ(node) {
            NodeType::List => {
                let cdr = nd!(self).cdr(node);
                if cdr == NULL_NODE {
                    self.get_tree_tail_literal(nd!(self).car(node), rnode, nest_level)
                } else {
                    let r = self.get_tree_tail_literal(cdr, rnode, nest_level);
                    if r == 0 {
                        self.get_tree_tail_literal(nd!(self).car(node), rnode, nest_level)
                    } else {
                        r
                    }
                }
            }
            NodeType::Call => self.get_tree_tail_literal(nd!(self).body(node), rnode, nest_level),
            NodeType::CType => {
                let (ct, _, _) = ctype_fields(&nd!(self), node);
                if ct == CTYPE_ANYCHAR {
                    -1
                } else {
                    *rnode = node;
                    1
                }
            }
            NodeType::CClass => {
                *rnode = node;
                1
            }
            NodeType::String => {
                if nd!(self).str_bytes(node).is_empty() {
                    0
                } else if is_real_ignorecase(&nd!(self), node) {
                    -1
                } else {
                    *rnode = node;
                    1
                }
            }
            NodeType::Quant => {
                let (lower, _, _) = quant_params(&nd!(self), node);
                if lower != 0 {
                    self.get_tree_tail_literal(nd!(self).body(node), rnode, nest_level)
                } else {
                    -1
                }
            }
            NodeType::Bag => {
                if bag_typ(&nd!(self), node) == BagType::Memory {
                    if nd!(self).has(node, ND_ST_MARK1) {
                        -1
                    } else {
                        nd!(self).add(node, ND_ST_MARK1);
                        let r = self.get_tree_tail_literal(nd!(self).body(node), rnode, nest_level);
                        nd!(self).remove(node, ND_ST_MARK1);
                        r
                    }
                } else {
                    self.get_tree_tail_literal(nd!(self).body(node), rnode, nest_level)
                }
            }
            NodeType::Anchor | NodeType::Gimmick => 0,
            NodeType::Alt | NodeType::BackRef => -1,
        }
    }

    fn check_called_node_in_look_behind(&mut self, node: NodeRef, not: bool) -> i32 {
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let r = self.check_called_node_in_look_behind(nd!(self).car(n), not);
                    if r != 0 {
                        return r;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::Quant => self.check_called_node_in_look_behind(nd!(self).body(node), not),
            NodeType::Bag => {
                let typ = bag_typ(&nd!(self), node);
                if typ == BagType::Memory {
                    if nd!(self).has(node, ND_ST_MARK1) {
                        return 0;
                    }
                    nd!(self).add(node, ND_ST_MARK1);
                    let r = self.check_called_node_in_look_behind(nd!(self).body(node), not);
                    nd!(self).remove(node, ND_ST_MARK1);
                    r
                } else {
                    let mut r = self.check_called_node_in_look_behind(nd!(self).body(node), not);
                    if r == 0 && typ == BagType::IfElse {
                        let (then, els) = bag_then_else(&nd!(self), node);
                        if then != NULL_NODE {
                            r = self.check_called_node_in_look_behind(then, not);
                            if r != 0 {
                                return r;
                            }
                        }
                        if els != NULL_NODE {
                            r = self.check_called_node_in_look_behind(els, not);
                        }
                    }
                    r
                }
            }
            NodeType::Anchor => {
                let body = nd!(self).body(node);
                if body != NULL_NODE {
                    self.check_called_node_in_look_behind(body, not)
                } else {
                    0
                }
            }
            NodeType::Gimmick => {
                if nd!(self).has(node, ND_ST_ABSENT_WITH_SIDE_EFFECTS) {
                    1
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    fn check_node_in_look_behind(&mut self, node: NodeRef, not: bool, used: &mut bool) -> i32 {
        const ALLOWED_BAG_IN_LB: u32 = (1 << BagType::Memory as u32)
            | (1 << BagType::Option as u32)
            | (1 << BagType::StopBacktrack as u32)
            | (1 << BagType::IfElse as u32);
        const ALLOWED_BAG_IN_LB_NOT: u32 = (1 << BagType::Option as u32)
            | (1 << BagType::StopBacktrack as u32)
            | (1 << BagType::IfElse as u32);
        const ALLOWED_ANCHOR_IN_LB: i32 = ANCR_LOOK_BEHIND
            | ANCR_BEGIN_LINE
            | ANCR_END_LINE
            | ANCR_BEGIN_BUF
            | ANCR_BEGIN_POSITION
            | ANCR_WORD_BOUNDARY
            | ANCR_NO_WORD_BOUNDARY
            | ANCR_WORD_BEGIN
            | ANCR_WORD_END
            | ANCR_TEXT_SEGMENT_BOUNDARY
            | ANCR_NO_TEXT_SEGMENT_BOUNDARY;
        const ALLOWED_ANCHOR_IN_LB_NOT: i32 = ANCR_LOOK_BEHIND
            | ANCR_LOOK_BEHIND_NOT
            | ANCR_BEGIN_LINE
            | ANCR_END_LINE
            | ANCR_BEGIN_BUF
            | ANCR_BEGIN_POSITION
            | ANCR_WORD_BOUNDARY
            | ANCR_NO_WORD_BOUNDARY
            | ANCR_WORD_BEGIN
            | ANCR_WORD_END
            | ANCR_TEXT_SEGMENT_BOUNDARY
            | ANCR_NO_TEXT_SEGMENT_BOUNDARY;

        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let r = self.check_node_in_look_behind(nd!(self).car(n), not, used);
                    if r != 0 {
                        return r;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::Quant => self.check_node_in_look_behind(nd!(self).body(node), not, used),
            NodeType::Bag => {
                let typ = bag_typ(&nd!(self), node);
                let mask = if not {
                    ALLOWED_BAG_IN_LB_NOT
                } else {
                    ALLOWED_BAG_IN_LB
                };
                if (1u32 << typ as u32) & mask == 0 {
                    return 1;
                }
                let r = self.check_node_in_look_behind(nd!(self).body(node), not, used);
                if r != 0 {
                    return r;
                }
                if typ == BagType::Memory {
                    if nd!(self).has(node, ND_ST_BACKREF)
                        || nd!(self).has(node, ND_ST_CALLED)
                        || nd!(self).has(node, ND_ST_REFERENCED)
                    {
                        *used = true;
                    }
                } else if typ == BagType::IfElse {
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let r = self.check_node_in_look_behind(then, not, used);
                        if r != 0 {
                            return r;
                        }
                    }
                    if els != NULL_NODE {
                        return self.check_node_in_look_behind(els, not, used);
                    }
                }
                0
            }
            NodeType::Anchor => {
                let t = anchor_typ(&nd!(self), node);
                let mask = if not {
                    ALLOWED_ANCHOR_IN_LB_NOT
                } else {
                    ALLOWED_ANCHOR_IN_LB
                };
                if t & mask == 0 {
                    return 1;
                }
                let body = nd!(self).body(node);
                if body != NULL_NODE {
                    return self.check_node_in_look_behind(body, not, used);
                }
                0
            }
            NodeType::Gimmick => {
                if nd!(self).has(node, ND_ST_ABSENT_WITH_SIDE_EFFECTS) {
                    return 1;
                }
                if let NodeKind::Gimmick {
                    typ: GimmickType::Save,
                    detail_type: SAVE_KEEP,
                    ..
                } = nd!(self).v[node].kind
                {
                    *used = true;
                }
                0
            }
            NodeType::Call => {
                if nd!(self).has(node, ND_ST_RECURSION) {
                    *used = true;
                    0
                } else {
                    self.check_called_node_in_look_behind(nd!(self).body(node), not)
                }
            }
            _ => 0,
        }
    }

    fn node_min_byte_len(&mut self, node: NodeRef) -> u32 {
        let mut len: u32 = 0;
        match nd!(self).typ(node) {
            NodeType::BackRef => {
                if !nd!(self).has(node, ND_ST_CHECKER) && !nd!(self).has(node, ND_ST_RECURSION) {
                    let backs = backrefs(&nd!(self), node).to_vec();
                    len = self.node_min_byte_len(self.env.mem_node(backs[0]));
                    for &b in &backs[1..] {
                        let tmin = self.node_min_byte_len(self.env.mem_node(b));
                        if len > tmin {
                            len = tmin;
                        }
                    }
                }
            }
            NodeType::Call => {
                let t = nd!(self).body(node);
                if nd!(self).has(t, ND_ST_FIXED_MIN) {
                    len = bag_min_len(&nd!(self), t);
                } else {
                    len = self.node_min_byte_len(t);
                }
            }
            NodeType::List => {
                let mut n = node;
                loop {
                    let tmin = self.node_min_byte_len(nd!(self).car(n));
                    len = distance_add(len, tmin);
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::Alt => {
                let mut y = node;
                loop {
                    let x = nd!(self).car(y);
                    let tmin = self.node_min_byte_len(x);
                    if y == node {
                        len = tmin;
                    } else if len > tmin {
                        len = tmin;
                    }
                    y = nd!(self).cdr(y);
                    if y == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::String => len = nd!(self).str_bytes(node).len() as u32,
            NodeType::CType | NodeType::CClass => len = self.reg.enc.min_enc_len as u32,
            NodeType::Quant => {
                let (lower, _, _) = quant_params(&nd!(self), node);
                if lower > 0 {
                    len = self.node_min_byte_len(nd!(self).body(node));
                    len = distance_multiply(len, lower);
                }
            }
            NodeType::Bag => {
                let typ = bag_typ(&nd!(self), node);
                match typ {
                    BagType::Memory => {
                        if nd!(self).has(node, ND_ST_FIXED_MIN) {
                            len = bag_min_len(&nd!(self), node);
                        } else if nd!(self).has(node, ND_ST_MARK1) {
                            len = 0;
                        } else {
                            nd!(self).add(node, ND_ST_MARK1);
                            len = self.node_min_byte_len(nd!(self).body(node));
                            nd!(self).remove(node, ND_ST_MARK1);
                            if let NodeKind::Bag { min_len, .. } = &mut nd!(self).v[node].kind {
                                *min_len = len;
                            }
                            nd!(self).add(node, ND_ST_FIXED_MIN);
                        }
                    }
                    BagType::Option | BagType::StopBacktrack => {
                        len = self.node_min_byte_len(nd!(self).body(node));
                    }
                    BagType::IfElse => {
                        len = self.node_min_byte_len(nd!(self).body(node));
                        let (then, els) = bag_then_else(&nd!(self), node);
                        if then != NULL_NODE {
                            len = len.wrapping_add(self.node_min_byte_len(then));
                        }
                        let elen = if els != NULL_NODE {
                            self.node_min_byte_len(els)
                        } else {
                            0
                        };
                        if elen < len {
                            len = elen;
                        }
                    }
                }
            }
            NodeType::Gimmick => {
                if let NodeKind::Gimmick {
                    typ: GimmickType::Fail,
                    ..
                } = nd!(self).v[node].kind
                {
                    len = INFINITE_LEN;
                }
            }
            NodeType::Anchor => {}
        }
        len
    }

    fn check_backrefs(&mut self, node: NodeRef) -> i32 {
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let r = self.check_backrefs(nd!(self).car(n));
                    if r != 0 {
                        return r;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::Anchor => {
                if !anchor_has_body(anchor_typ(&nd!(self), node)) {
                    0
                } else {
                    self.check_backrefs(nd!(self).body(node))
                }
            }
            NodeType::Quant => self.check_backrefs(nd!(self).body(node)),
            NodeType::Bag => {
                let r = self.check_backrefs(nd!(self).body(node));
                if bag_typ(&nd!(self), node) == BagType::IfElse {
                    if r != 0 {
                        return r;
                    }
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let r = self.check_backrefs(then);
                        if r != 0 {
                            return r;
                        }
                    }
                    if els != NULL_NODE {
                        return self.check_backrefs(els);
                    }
                }
                r
            }
            NodeType::BackRef => {
                let backs = backrefs(&nd!(self), node).to_vec();
                for &b in &backs {
                    if b > self.env.num_mem {
                        return ONIGERR_INVALID_BACKREF;
                    }
                    let m = self.env.mem_node(b);
                    if m != NULL_NODE {
                        nd!(self).add(m, ND_ST_BACKREF);
                    }
                }
                0
            }
            _ => 0,
        }
    }

    fn set_empty_repeat_node_trav(&mut self, node: NodeRef, mut empty: NodeRef) -> i32 {
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let r = self.set_empty_repeat_node_trav(nd!(self).car(n), empty);
                    if r != 0 {
                        return r;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::Anchor => {
                let t = anchor_typ(&nd!(self), node);
                if !anchor_has_body(t) {
                    return 0;
                }
                if t == ANCR_PREC_READ || t == ANCR_LOOK_BEHIND {
                    empty = NULL_NODE;
                }
                self.set_empty_repeat_node_trav(nd!(self).body(node), empty)
            }
            NodeType::Quant => {
                let (emptiness, _, _, _) = self.quant_extra(node);
                if emptiness != BodyEmpty::IsNotEmpty {
                    empty = node;
                }
                self.set_empty_repeat_node_trav(nd!(self).body(node), empty)
            }
            NodeType::Bag => {
                let body = nd!(self).body(node);
                if body != NULL_NODE {
                    let r = self.set_empty_repeat_node_trav(body, empty);
                    if r != 0 {
                        return r;
                    }
                }
                let (typ, regnum, _) = self.bag_fields(node);
                if typ == BagType::Memory {
                    if nd!(self).has(node, ND_ST_BACKREF) && empty != NULL_NODE {
                        self.env.mem_env[regnum as usize].empty_repeat_node = empty;
                    }
                } else if typ == BagType::IfElse {
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let r = self.set_empty_repeat_node_trav(then, empty);
                        if r != 0 {
                            return r;
                        }
                    }
                    if els != NULL_NODE {
                        return self.set_empty_repeat_node_trav(els, empty);
                    }
                }
                0
            }
            _ => 0,
        }
    }

    fn is_ancestor_node(&self, node: NodeRef, mut me: NodeRef) -> bool {
        loop {
            let parent = nd!(self).parent(me);
            if parent == NULL_NODE {
                return false;
            }
            if parent == node {
                return true;
            }
            me = parent;
        }
    }

    fn set_empty_status_check_trav(&mut self, node: NodeRef) {
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    self.set_empty_status_check_trav(nd!(self).car(n));
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::Anchor => {
                if anchor_has_body(anchor_typ(&nd!(self), node)) {
                    self.set_empty_status_check_trav(nd!(self).body(node));
                }
            }
            NodeType::Quant => self.set_empty_status_check_trav(nd!(self).body(node)),
            NodeType::Bag => {
                let body = nd!(self).body(node);
                if body != NULL_NODE {
                    self.set_empty_status_check_trav(body);
                }
                if bag_typ(&nd!(self), node) == BagType::IfElse {
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        self.set_empty_status_check_trav(then);
                    }
                    if els != NULL_NODE {
                        self.set_empty_status_check_trav(els);
                    }
                }
            }
            NodeType::BackRef => {
                let backs = backrefs(&nd!(self), node).to_vec();
                for &b in &backs {
                    let ernode = self.env.mem_env[b as usize].empty_repeat_node;
                    if ernode != NULL_NODE && !self.is_ancestor_node(ernode, node) {
                        if let NodeKind::Quant {
                            empty_status_mem, ..
                        } = &mut nd!(self).v[ernode].kind
                        {
                            mem_status_limit_on(empty_status_mem, b);
                        }
                        nd!(self).add(ernode, ND_ST_EMPTY_STATUS_CHECK);
                        let m = self.env.mem_env[b as usize].mem_node;
                        nd!(self).add(m, ND_ST_EMPTY_STATUS_CHECK);
                    }
                }
            }
            _ => {}
        }
    }

    fn set_parent_node_trav(&mut self, node: NodeRef, parent: NodeRef) {
        nd!(self).set_parent(node, parent);
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let car = nd!(self).car(n);
                    self.set_parent_node_trav(car, n);
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::Anchor => {
                if anchor_has_body(anchor_typ(&nd!(self), node)) {
                    self.set_parent_node_trav(nd!(self).body(node), node);
                }
            }
            NodeType::Quant => self.set_parent_node_trav(nd!(self).body(node), node),
            NodeType::Bag => {
                let body = nd!(self).body(node);
                if body != NULL_NODE {
                    self.set_parent_node_trav(body, node);
                }
                if bag_typ(&nd!(self), node) == BagType::IfElse {
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        self.set_parent_node_trav(then, node);
                    }
                    if els != NULL_NODE {
                        self.set_parent_node_trav(els, node);
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn ctype_fields(nodes: &Nodes, n: NodeRef) -> (i32, bool, bool) {
    match &nodes.v[n].kind {
        NodeKind::CType {
            ctype,
            not,
            ascii_mode,
        } => (*ctype, *not, *ascii_mode),
        _ => panic!("not a ctype"),
    }
}

fn bag_min_len(nodes: &Nodes, n: NodeRef) -> u32 {
    match &nodes.v[n].kind {
        NodeKind::Bag { min_len, .. } => *min_len,
        _ => 0,
    }
}

/* ===== recursion checks ===== */

const RECURSION_EXIST: i32 = 1 << 0;
const RECURSION_MUST: i32 = 1 << 1;
const RECURSION_INFINITE: i32 = 1 << 2;

const IN_RECURSION: i32 = 1 << 0;
const FOUND_CALLED_NODE: i32 = 1;

pub const IN_ALT: i32 = 1 << 0;
pub const IN_NOT: i32 = 1 << 1;
pub const IN_REAL_REPEAT: i32 = 1 << 2;
pub const IN_VAR_REPEAT: i32 = 1 << 3;
pub const IN_ZERO_REPEAT: i32 = 1 << 4;
pub const IN_MULTI_ENTRY: i32 = 1 << 5;
pub const IN_PREC_READ: i32 = 1 << 6;
pub const IN_LOOK_BEHIND: i32 = 1 << 7;
pub const IN_PEEK: i32 = 1 << 8;

impl<'e, 'a> Ctx<'e, 'a> {
    fn infinite_recursive_call_check(&mut self, node: NodeRef, mut head: i32) -> i32 {
        let mut r = 0;
        match nd!(self).typ(node) {
            NodeType::List => {
                let mut x = node;
                loop {
                    let car = nd!(self).car(x);
                    let ret = self.infinite_recursive_call_check(car, head);
                    if ret < 0 || (ret & RECURSION_INFINITE) != 0 {
                        return ret;
                    }
                    r |= ret;
                    if head != 0 {
                        let min = self.node_min_byte_len(car);
                        if min != 0 {
                            head = 0;
                        }
                    }
                    x = nd!(self).cdr(x);
                    if x == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::Alt => {
                let mut must = RECURSION_MUST;
                let mut n = node;
                loop {
                    let ret = self.infinite_recursive_call_check(nd!(self).car(n), head);
                    if ret < 0 || (ret & RECURSION_INFINITE) != 0 {
                        return ret;
                    }
                    r |= ret & RECURSION_EXIST;
                    must &= ret;
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                r |= must;
            }
            NodeType::Quant => {
                let (lower, upper, _) = quant_params(&nd!(self), node);
                if upper == 0 {
                    return 0;
                }
                r = self.infinite_recursive_call_check(nd!(self).body(node), head);
                if r < 0 {
                    return r;
                }
                if (r & RECURSION_MUST) != 0 && lower == 0 {
                    r &= !RECURSION_MUST;
                }
            }
            NodeType::Anchor => {
                if anchor_has_body(anchor_typ(&nd!(self), node)) {
                    r = self.infinite_recursive_call_check(nd!(self).body(node), head);
                }
            }
            NodeType::Call => {
                r = self.infinite_recursive_call_check(nd!(self).body(node), head);
            }
            NodeType::Bag => {
                let typ = bag_typ(&nd!(self), node);
                if typ == BagType::Memory {
                    if nd!(self).has(node, ND_ST_MARK2) {
                        return 0;
                    } else if nd!(self).has(node, ND_ST_MARK1) {
                        return if head == 0 {
                            RECURSION_EXIST | RECURSION_MUST
                        } else {
                            RECURSION_EXIST | RECURSION_MUST | RECURSION_INFINITE
                        };
                    } else {
                        nd!(self).add(node, ND_ST_MARK2);
                        r = self.infinite_recursive_call_check(nd!(self).body(node), head);
                        nd!(self).remove(node, ND_ST_MARK2);
                    }
                } else if typ == BagType::IfElse {
                    let body = nd!(self).body(node);
                    let ret = self.infinite_recursive_call_check(body, head);
                    if ret < 0 || (ret & RECURSION_INFINITE) != 0 {
                        return ret;
                    }
                    r |= ret;
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let min = if head != 0 {
                            self.node_min_byte_len(body)
                        } else {
                            0
                        };
                        let ret = self
                            .infinite_recursive_call_check(then, if min != 0 { 0 } else { head });
                        if ret < 0 || (ret & RECURSION_INFINITE) != 0 {
                            return ret;
                        }
                        r |= ret;
                    }
                    if els != NULL_NODE {
                        let eret = self.infinite_recursive_call_check(els, head);
                        if eret < 0 || (eret & RECURSION_INFINITE) != 0 {
                            return eret;
                        }
                        r |= eret & RECURSION_EXIST;
                        if (eret & RECURSION_MUST) == 0 {
                            r &= !RECURSION_MUST;
                        }
                    } else {
                        r &= !RECURSION_MUST;
                    }
                } else {
                    r = self.infinite_recursive_call_check(nd!(self).body(node), head);
                }
            }
            _ => {}
        }
        r
    }

    fn infinite_recursive_call_check_trav(&mut self, node: NodeRef) -> i32 {
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let r = self.infinite_recursive_call_check_trav(nd!(self).car(n));
                    if r != 0 {
                        return r;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::Anchor => {
                if !anchor_has_body(anchor_typ(&nd!(self), node)) {
                    0
                } else {
                    self.infinite_recursive_call_check_trav(nd!(self).body(node))
                }
            }
            NodeType::Quant => self.infinite_recursive_call_check_trav(nd!(self).body(node)),
            NodeType::Bag => {
                let typ = bag_typ(&nd!(self), node);
                if typ == BagType::Memory {
                    if nd!(self).has(node, ND_ST_RECURSION) && nd!(self).has(node, ND_ST_CALLED) {
                        nd!(self).add(node, ND_ST_MARK1);
                        let ret = self.infinite_recursive_call_check(nd!(self).body(node), 1);
                        if ret < 0 {
                            return ret;
                        } else if (ret & (RECURSION_MUST | RECURSION_INFINITE)) != 0 {
                            return ONIGERR_NEVER_ENDING_RECURSION;
                        }
                        nd!(self).remove(node, ND_ST_MARK1);
                    }
                } else if typ == BagType::IfElse {
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let r = self.infinite_recursive_call_check_trav(then);
                        if r != 0 {
                            return r;
                        }
                    }
                    if els != NULL_NODE {
                        let r = self.infinite_recursive_call_check_trav(els);
                        if r != 0 {
                            return r;
                        }
                    }
                }
                self.infinite_recursive_call_check_trav(nd!(self).body(node))
            }
            _ => 0,
        }
    }

    fn recursive_call_check(&mut self, node: NodeRef) -> i32 {
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut r = 0;
                let mut n = node;
                loop {
                    r |= self.recursive_call_check(nd!(self).car(n));
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                r
            }
            NodeType::Anchor => {
                if !anchor_has_body(anchor_typ(&nd!(self), node)) {
                    0
                } else {
                    self.recursive_call_check(nd!(self).body(node))
                }
            }
            NodeType::Quant => self.recursive_call_check(nd!(self).body(node)),
            NodeType::Call => {
                let body = nd!(self).body(node);
                let r = self.recursive_call_check(body);
                if r != 0 && nd!(self).has(body, ND_ST_MARK1) {
                    nd!(self).add(node, ND_ST_RECURSION);
                }
                r
            }
            NodeType::Bag => {
                let typ = bag_typ(&nd!(self), node);
                if typ == BagType::Memory {
                    if nd!(self).has(node, ND_ST_MARK2) {
                        0
                    } else if nd!(self).has(node, ND_ST_MARK1) {
                        1
                    } else {
                        nd!(self).add(node, ND_ST_MARK2);
                        let r = self.recursive_call_check(nd!(self).body(node));
                        nd!(self).remove(node, ND_ST_MARK2);
                        r
                    }
                } else if typ == BagType::IfElse {
                    let mut r = 0;
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        r |= self.recursive_call_check(then);
                    }
                    if els != NULL_NODE {
                        r |= self.recursive_call_check(els);
                    }
                    r |= self.recursive_call_check(nd!(self).body(node));
                    r
                } else {
                    self.recursive_call_check(nd!(self).body(node))
                }
            }
            _ => 0,
        }
    }

    fn recursive_call_check_trav(&mut self, node: NodeRef, state: i32) -> i32 {
        let mut r = 0;
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let ret = self.recursive_call_check_trav(nd!(self).car(n), state);
                    if ret == FOUND_CALLED_NODE {
                        r = FOUND_CALLED_NODE;
                    } else if ret < 0 {
                        return ret;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::Quant => {
                r = self.recursive_call_check_trav(nd!(self).body(node), state);
                let (_, upper, _) = quant_params(&nd!(self), node);
                if upper == 0 && r == FOUND_CALLED_NODE {
                    if let NodeKind::Quant {
                        include_referred, ..
                    } = &mut nd!(self).v[node].kind
                    {
                        *include_referred = true;
                    }
                }
            }
            NodeType::Anchor => {
                if anchor_has_body(anchor_typ(&nd!(self), node)) {
                    r = self.recursive_call_check_trav(nd!(self).body(node), state);
                }
            }
            NodeType::Bag => {
                let (typ, regnum, body) = self.bag_fields(node);
                if typ == BagType::Memory {
                    let mut check = false;
                    if nd!(self).has(node, ND_ST_CALLED) {
                        r = FOUND_CALLED_NODE;
                        check = true;
                    } else if (state & IN_RECURSION) != 0 {
                        check = true;
                    }
                    if check && !nd!(self).has(node, ND_ST_RECURSION) {
                        nd!(self).add(node, ND_ST_MARK1);
                        let ret = self.recursive_call_check(body);
                        if ret != 0 {
                            nd!(self).add(node, ND_ST_RECURSION);
                            mem_status_on(&mut self.env.backtrack_mem, regnum);
                        }
                        nd!(self).remove(node, ND_ST_MARK1);
                    }
                }
                let mut state1 = state;
                if nd!(self).has(node, ND_ST_RECURSION) {
                    state1 |= IN_RECURSION;
                }
                let ret = self.recursive_call_check_trav(body, state1);
                if ret == FOUND_CALLED_NODE {
                    r = FOUND_CALLED_NODE;
                }
                if typ == BagType::IfElse {
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let ret = self.recursive_call_check_trav(then, state1);
                        if ret == FOUND_CALLED_NODE {
                            r = FOUND_CALLED_NODE;
                        }
                    }
                    if els != NULL_NODE {
                        let ret = self.recursive_call_check_trav(els, state1);
                        if ret == FOUND_CALLED_NODE {
                            r = FOUND_CALLED_NODE;
                        }
                    }
                }
            }
            _ => {}
        }
        r
    }

    /* ===== string list reduction ===== */

    fn reduce_string_list(&mut self, mut node: NodeRef) -> i32 {
        let enc = self.reg.enc;
        match nd!(self).typ(node) {
            NodeType::List => {
                let mut prev = NULL_NODE;
                let mut prev_node = NULL_NODE;
                loop {
                    let next_node = nd!(self).cdr(node);
                    let curr = nd!(self).car(node);
                    if nd!(self).typ(curr) == NodeType::String {
                        if prev == NULL_NODE
                            || nd!(self).str_flag(curr) != nd!(self).str_flag(prev)
                            || nd!(self).status(curr) != nd!(self).status(prev)
                        {
                            prev = curr;
                            prev_node = node;
                        } else {
                            let add = nd!(self).str_bytes(curr).to_vec();
                            nd!(self).str_mut(prev).extend_from_slice(&add);
                            /* remove_from_list(prev_node, node) */
                            if nd!(self).cdr(prev_node) == node {
                                let c = nd!(self).cdr(node);
                                nd!(self).set_cdr(prev_node, c);
                                nd!(self).set_cdr(node, NULL_NODE);
                            }
                        }
                    } else {
                        if prev != NULL_NODE {
                            let s = nd!(self).str_bytes(prev);
                            if !enc.is_valid_mbc_string(s, 0, s.len()) {
                                return ONIGERR_INVALID_WIDE_CHAR_VALUE;
                            }
                            prev = NULL_NODE;
                        }
                        let r = self.reduce_string_list(curr);
                        if r != 0 {
                            return r;
                        }
                        prev_node = node;
                    }
                    node = next_node;
                    if node == NULL_NODE {
                        break;
                    }
                }
                if prev != NULL_NODE {
                    let s = nd!(self).str_bytes(prev);
                    if !enc.is_valid_mbc_string(s, 0, s.len()) {
                        return ONIGERR_INVALID_WIDE_CHAR_VALUE;
                    }
                }
                0
            }
            NodeType::Alt => {
                loop {
                    let r = self.reduce_string_list(nd!(self).car(node));
                    if r != 0 {
                        return r;
                    }
                    node = nd!(self).cdr(node);
                    if node == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::String => {
                let s = nd!(self).str_bytes(node);
                if !enc.is_valid_mbc_string(s, 0, s.len()) {
                    return ONIGERR_INVALID_WIDE_CHAR_VALUE;
                }
                0
            }
            NodeType::Anchor => {
                let body = nd!(self).body(node);
                if body == NULL_NODE {
                    0
                } else {
                    self.reduce_string_list(body)
                }
            }
            NodeType::Quant => self.reduce_string_list(nd!(self).body(node)),
            NodeType::Bag => {
                let r = self.reduce_string_list(nd!(self).body(node));
                if r != 0 {
                    return r;
                }
                if bag_typ(&nd!(self), node) == BagType::IfElse {
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let r = self.reduce_string_list(then);
                        if r != 0 {
                            return r;
                        }
                    }
                    if els != NULL_NODE {
                        let r = self.reduce_string_list(els);
                        if r != 0 {
                            return r;
                        }
                    }
                }
                0
            }
            _ => 0,
        }
    }

    /* ===== look-behind ===== */

    /// (?<=A|B) ==> (?<=A)|(?<=B), (?<!A|B) ==> (?<!A)(?<!B)
    fn divide_look_behind_alternatives(&mut self, node: NodeRef) -> i32 {
        let anc_type = anchor_typ(&nd!(self), node);
        let head = nd!(self).body(node);
        let np = nd!(self).car(head);
        nd!(self).swap(node, head);
        /* now `node` holds the alt, `head` holds the anchor */
        nd!(self).set_car(node, head);
        nd!(self).set_body(head, np);

        let mut np = node;
        loop {
            np = nd!(self).cdr(np);
            if np == NULL_NODE {
                break;
            }
            let insert_node = nd!(self).alloc(nd!(self).v[head].kind.clone());
            nd!(self).v[insert_node].status = nd!(self).v[head].status;
            nd!(self).v[insert_node].parent = nd!(self).v[head].parent;
            let car = nd!(self).car(np);
            nd!(self).set_body(insert_node, car);
            nd!(self).set_car(np, insert_node);
        }

        if anc_type == ANCR_LOOK_BEHIND_NOT {
            let mut np = node;
            loop {
                /* alt -> list */
                let (car, cdr) = (nd!(self).car(np), nd!(self).cdr(np));
                nd!(self).v[np].kind = NodeKind::List { car, cdr };
                np = cdr;
                if np == NULL_NODE {
                    break;
                }
            }
        }
        0
    }

    fn node_reduce_in_look_behind(&mut self, node: NodeRef) -> i32 {
        if nd!(self).typ(node) != NodeType::Quant {
            return 0;
        }
        let body = nd!(self).body(node);
        let t = nd!(self).typ(body);
        if t == NodeType::String
            || t == NodeType::CType
            || t == NodeType::CClass
            || t == NodeType::BackRef
        {
            let (lower, _, greedy) = quant_params(&nd!(self), node);
            set_quant_params(&mut nd!(self), node, lower, lower, greedy);
            if lower == 0 {
                return 1;
            }
        }
        0
    }

    fn list_reduce_in_look_behind(&mut self, mut node: NodeRef) -> i32 {
        match nd!(self).typ(node) {
            NodeType::Quant => {
                let r = self.node_reduce_in_look_behind(node);
                if r > 0 { 0 } else { r }
            }
            NodeType::List => {
                let mut r;
                loop {
                    r = self.node_reduce_in_look_behind(nd!(self).car(node));
                    if r <= 0 {
                        break;
                    }
                    node = nd!(self).cdr(node);
                    if node == NULL_NODE {
                        break;
                    }
                }
                r
            }
            _ => 0,
        }
    }

    fn alt_reduce_in_look_behind(&mut self, mut node: NodeRef) -> i32 {
        match nd!(self).typ(node) {
            NodeType::Alt => {
                let mut r;
                loop {
                    r = self.list_reduce_in_look_behind(nd!(self).car(node));
                    if r != 0 {
                        break;
                    }
                    node = nd!(self).cdr(node);
                    if node == NULL_NODE {
                        break;
                    }
                }
                r
            }
            _ => self.list_reduce_in_look_behind(node),
        }
    }

    fn tune_look_behind(&mut self, node: NodeRef, state: i32) -> i32 {
        let atyp = anchor_typ(&nd!(self), node);
        let mut used = false;
        let body = nd!(self).body(node);
        let r = self.check_node_in_look_behind(body, atyp == ANCR_LOOK_BEHIND_NOT, &mut used);
        if r < 0 {
            return r;
        }
        if r > 0 {
            return ONIGERR_INVALID_LOOK_BEHIND_PATTERN;
        }

        let state1 = if atyp == ANCR_LOOK_BEHIND_NOT {
            state | IN_NOT | IN_LOOK_BEHIND
        } else {
            state | IN_LOOK_BEHIND
        };

        let r = self.tune_tree(body, state1);
        if r != 0 {
            return r;
        }
        let body = nd!(self).body(node);
        let r = self.alt_reduce_in_look_behind(body);
        if r != 0 {
            return r;
        }

        let mut ci = MinMaxCharLen::default();
        let mut r = self.node_char_len(body, &mut ci);
        if r >= 0 {
            if (ci.max != INFINITE_LEN && ci.max > LOOK_BEHIND_MAX_CHAR_LEN)
                || ci.min > LOOK_BEHIND_MAX_CHAR_LEN
            {
                return ONIGERR_INVALID_LOOK_BEHIND_PATTERN;
            }

            if ci.min == 0 && ci.min_is_sure && !used {
                if atyp == ANCR_LOOK_BEHIND_NOT {
                    /* onig_node_reset_fail */
                    nd!(self).v[node].kind = NodeKind::Gimmick {
                        typ: GimmickType::Fail,
                        detail_type: 0,
                        num: 0,
                        id: 0,
                    };
                } else {
                    /* onig_node_reset_empty */
                    nd!(self).v[node].kind = NodeKind::Str {
                        s: Vec::new(),
                        flag: 0,
                    };
                }
                return 0;
            }

            let mut normal = false;
            if r == CHAR_LEN_TOP_ALT_FIXED {
                if self.env.syntax.bv(ONIG_SYN_DIFFERENT_LEN_ALT_LOOK_BEHIND) {
                    r = self.divide_look_behind_alternatives(node);
                    if r == 0 {
                        r = self.tune_tree(node, state);
                    }
                } else if self.env.syntax.bv(ONIG_SYN_VARIABLE_LEN_LOOK_BEHIND) {
                    normal = true;
                } else {
                    r = ONIGERR_INVALID_LOOK_BEHIND_PATTERN;
                }
            } else {
                normal = true;
            }
            if normal {
                if ci.min == INFINITE_LEN {
                    r = ONIGERR_INVALID_LOOK_BEHIND_PATTERN;
                } else if ci.min != ci.max && !self.env.syntax.bv(ONIG_SYN_VARIABLE_LEN_LOOK_BEHIND)
                {
                    r = ONIGERR_INVALID_LOOK_BEHIND_PATTERN;
                } else {
                    let lead = match &nd!(self).v[node].kind {
                        NodeKind::Anchor { lead_node, .. } => *lead_node,
                        _ => NULL_NODE,
                    };
                    if lead == NULL_NODE {
                        if let NodeKind::Anchor {
                            char_min_len,
                            char_max_len,
                            ..
                        } = &mut nd!(self).v[node].kind
                        {
                            *char_min_len = ci.min;
                            *char_max_len = ci.max;
                        }
                        let mut tail = NULL_NODE;
                        let rr = self.get_tree_tail_literal(body, &mut tail, 0);
                        if rr == 1 {
                            let copy = nd!(self).alloc(nd!(self).v[tail].kind.clone());
                            nd!(self).v[copy].status = nd!(self).v[tail].status;
                            nd!(self).v[copy].parent = nd!(self).v[tail].parent;
                            if let NodeKind::Anchor { lead_node, .. } = &mut nd!(self).v[node].kind
                            {
                                *lead_node = copy;
                            }
                        }
                    }
                    r = ONIG_NORMAL;
                }
            }
        }
        r
    }

    fn tune_next(&mut self, mut node: NodeRef, next_node: NodeRef) -> i32 {
        let mut called = false;
        loop {
            match nd!(self).typ(node) {
                NodeType::Quant => {
                    let (lower, upper, greedy) = quant_params(&nd!(self), node);
                    if greedy && is_infinite_repeat(upper) {
                        if !called {
                            let n = self.get_tree_head_literal(next_node, true);
                            if n != NULL_NODE && nd!(self).str_bytes(n)[0] != 0 {
                                if let NodeKind::Quant {
                                    next_head_exact, ..
                                } = &mut nd!(self).v[node].kind
                                {
                                    *next_head_exact = n;
                                }
                            }
                        }
                        if lower <= 1 {
                            let body = nd!(self).body(node);
                            if is_strict_real_node(&nd!(self), body) {
                                let x = self.get_tree_head_literal(body, false);
                                if x != NULL_NODE {
                                    let y = self.get_tree_head_literal(next_node, false);
                                    if y != NULL_NODE && self.is_exclusive(x, y) {
                                        let en = self.env.node_new_bag(BagType::StopBacktrack);
                                        nd!(self).add(en, ND_ST_STRICT_REAL_REPEAT);
                                        nd!(self).swap(node, en);
                                        nd!(self).set_body(node, en);
                                    }
                                }
                            }
                        }
                    }
                    return 0;
                }
                NodeType::Bag => {
                    if bag_typ(&nd!(self), node) == BagType::Memory {
                        if nd!(self).has(node, ND_ST_CALLED) {
                            called = true;
                        }
                        node = nd!(self).body(node);
                        continue;
                    }
                    return 0;
                }
                _ => return 0,
            }
        }
    }

    /* ===== case fold string expansion ===== */

    fn make_code_list_to_string(&mut self, codes: &[u32]) -> Result<NodeRef, i32> {
        let enc = self.reg.enc;
        let mut s = Vec::new();
        for &c in codes {
            let mut buf = [0u8; ONIGENC_CODE_TO_MBC_MAXLEN];
            let len = enc.code_to_mbc(c, &mut buf);
            if len < 0 {
                return Err(len);
            }
            s.extend_from_slice(&buf[..len as usize]);
        }
        Ok(self.env.node_new_str(&s))
    }

    fn unravel_cf_node_add(&mut self, rlist: &mut NodeRef, add: NodeRef) {
        if *rlist == NULL_NODE {
            *rlist = self.env.node_new_list(add, NULL_NODE);
        } else {
            let mut l = *rlist;
            while nd!(self).cdr(l) != NULL_NODE {
                l = nd!(self).cdr(l);
            }
            let n = self.env.node_new_list(add, NULL_NODE);
            nd!(self).set_cdr(l, n);
        }
    }

    fn unravel_cf_string_add(
        &mut self,
        rlist: &mut NodeRef,
        rsn: &mut NodeRef,
        s: &[u8],
        flag: u32,
    ) {
        if *rsn != NULL_NODE && nd!(self).str_flag(*rsn) == flag {
            nd!(self).str_mut(*rsn).extend_from_slice(s);
        } else {
            let sn = self.env.node_new_str(s);
            if let NodeKind::Str { flag: f, .. } = &mut nd!(self).v[sn].kind {
                *f = flag;
            }
            self.unravel_cf_node_add(rlist, sn);
            *rsn = sn;
        }
    }

    fn new_cclass_with_code_list(&mut self, codes: &[u32]) -> NodeRef {
        let enc = self.reg.enc;
        let node = self.env.node_new_cclass();
        let mut cc = CClass::default();
        for &c in codes {
            if enc.min_enc_len > 1 || enc.code_to_mbclen(c) != 1 {
                add_code_range_to_buf(&mut cc.mbuf, c, c);
            } else {
                bitset_set_bit(&mut cc.bs, c);
            }
        }
        *nd!(self).cclass_mut(node) = cc;
        node
    }

    fn unravel_cf_string_alt_or_cc_add(
        &mut self,
        rlist: &mut NodeRef,
        items: &[CaseFoldCodeItem],
        s: &[u8],
    ) -> i32 {
        let enc = self.reg.enc;
        let node;
        if items.iter().all(|it| it.code_len == 1) {
            let mut codes = vec![enc.mbc_to_code(s, 0, s.len())];
            for it in items {
                codes.push(it.code[0]);
            }
            node = self.new_cclass_with_code_list(&codes);
        } else {
            let snode = self.env.node_new_str(s);
            node = self.env.node_new_alt(snode, NULL_NODE);
            let mut curr = node;
            for it in items {
                let sn = match self.make_code_list_to_string(&it.code[..it.code_len as usize]) {
                    Ok(n) => n,
                    Err(e) => return e,
                };
                let alt = self.env.node_new_alt(sn, NULL_NODE);
                nd!(self).set_cdr(curr, alt);
                curr = alt;
            }
        }
        self.unravel_cf_node_add(rlist, node);
        0
    }

    fn unravel_cf_look_behind_add(
        &mut self,
        rlist: &mut NodeRef,
        rsn: &mut NodeRef,
        items: &[CaseFoldCodeItem],
        s: &[u8],
        one_len: usize,
    ) -> i32 {
        let enc = self.reg.enc;
        let found = items
            .iter()
            .any(|it| it.byte_len as usize == one_len && it.code_len == 1);
        if !found {
            self.unravel_cf_string_add(rlist, rsn, &s[..one_len], 0);
        } else {
            let mut codes = vec![enc.mbc_to_code(s, 0, one_len)];
            for it in items {
                if it.byte_len as usize == one_len && it.code_len == 1 {
                    codes.push(it.code[0]);
                }
            }
            let node = self.new_cclass_with_code_list(&codes);
            self.unravel_cf_node_add(rlist, node);
            *rsn = NULL_NODE;
        }
        0
    }

    fn unravel_case_fold_string(&mut self, node: NodeRef, state: i32) -> i32 {
        if nd!(self).str_flag(node) & ND_STRING_CASE_EXPANDED != 0 {
            return 0;
        }
        nd!(self).remove(node, ND_ST_IGNORECASE);
        let s = nd!(self).str_bytes(node).to_vec();
        if s.is_empty() {
            return 0;
        }
        let in_look_behind = (state & IN_LOOK_BEHIND) != 0;
        let enc = self.reg.enc;
        let flag = self.reg.case_fold_flag;
        let end = s.len();

        let mut list = NULL_NODE;
        let mut sn = NULL_NODE;
        let mut p = 0usize;
        let mut items = [CaseFoldCodeItem::default(); ONIGENC_GET_CASE_FOLD_CODES_MAX_NUM];
        while p < end {
            let mut n = enc.get_case_fold_codes_by_str(flag, &s, p, end, &mut items);
            if n < 0 {
                return n;
            }
            let one_len = enc.mbc_enc_len(&s, p);
            let q;
            if n == 0 {
                q = (p + one_len).min(end);
                self.unravel_cf_string_add(&mut list, &mut sn, &s[p..q], 0);
            } else if in_look_behind {
                q = p + one_len;
                if items[0].byte_len as usize != one_len {
                    let r = enc.get_case_fold_codes_by_str(flag, &s, p, q, &mut items);
                    if r < 0 {
                        return r;
                    }
                    n = r;
                }
                let r = self.unravel_cf_look_behind_add(
                    &mut list,
                    &mut sn,
                    &items[..n as usize],
                    &s[p..],
                    one_len,
                );
                if r != 0 {
                    return r;
                }
            } else {
                let mut min_len = INFINITE_LEN;
                let mut max_len = 0u32;
                for it in &items[..n as usize] {
                    let len = it.byte_len as u32;
                    if len < min_len {
                        min_len = len;
                    }
                    if len > max_len {
                        max_len = len;
                    }
                }
                if min_len != max_len {
                    return ONIGERR_PARSER_BUG;
                }
                q = p + max_len as usize;
                let r =
                    self.unravel_cf_string_alt_or_cc_add(&mut list, &items[..n as usize], &s[p..q]);
                if r != 0 {
                    return r;
                }
                sn = NULL_NODE;
            }
            p = q;
        }

        if list != NULL_NODE {
            /* node_list_len(list) == 1 */
            if nd!(self).cdr(list) == NULL_NODE {
                let car = nd!(self).car(list);
                nd!(self).swap(node, car);
            } else {
                nd!(self).swap(node, list);
            }
        } else {
            nd!(self).swap(node, sn);
        }
        0
    }

    fn quantifiers_memory_node_info(&self, node: NodeRef) -> BodyEmpty {
        let mut r = BodyEmpty::MayBeEmpty;
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let v = self.quantifiers_memory_node_info(nd!(self).car(n));
                    if v > r {
                        r = v;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::Call => {
                if nd!(self).has(node, ND_ST_RECURSION) {
                    return BodyEmpty::MayBeEmptyRec;
                }
                r = self.quantifiers_memory_node_info(nd!(self).body(node));
            }
            NodeType::Quant => {
                let (_, upper, _) = quant_params(&nd!(self), node);
                if upper != 0 {
                    r = self.quantifiers_memory_node_info(nd!(self).body(node));
                }
            }
            NodeType::Bag => match bag_typ(&nd!(self), node) {
                BagType::Memory => {
                    if nd!(self).has(node, ND_ST_RECURSION) {
                        return BodyEmpty::MayBeEmptyRec;
                    }
                    return BodyEmpty::MayBeEmptyMem;
                }
                BagType::Option | BagType::StopBacktrack => {
                    r = self.quantifiers_memory_node_info(nd!(self).body(node));
                }
                BagType::IfElse => {
                    r = self.quantifiers_memory_node_info(nd!(self).body(node));
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let v = self.quantifiers_memory_node_info(then);
                        if v > r {
                            r = v;
                        }
                    }
                    if els != NULL_NODE {
                        let v = self.quantifiers_memory_node_info(els);
                        if v > r {
                            r = v;
                        }
                    }
                }
            },
            _ => {}
        }
        r
    }

    fn check_call_reference(&mut self, cn: NodeRef) -> i32 {
        let (by_number, mut called_gnum, name) = match &nd!(self).v[cn].kind {
            NodeKind::Call {
                by_number,
                called_gnum,
                name,
                ..
            } => (*by_number, *called_gnum, name.clone()),
            _ => unreachable!(),
        };
        if by_number {
            if self.env.num_named > 0
                && self.env.syntax.bv(ONIG_SYN_CAPTURE_ONLY_NAMED_GROUP)
                && !opton_capture_group(self.env.options)
            {
                return ONIGERR_NUMBERED_BACKREF_OR_CALL_NOT_ALLOWED;
            }
            if called_gnum > self.env.num_mem {
                self.env.set_error_bytes(&name);
                return ONIGERR_UNDEFINED_GROUP_REFERENCE;
            }
        } else {
            match self.env.names.find(&name) {
                None => {
                    self.env.set_error_bytes(&name);
                    return ONIGERR_UNDEFINED_NAME_REFERENCE;
                }
                Some(i) => {
                    let refs = &self.env.names.entries[i].back_refs;
                    if refs.is_empty() {
                        self.env.set_error_bytes(&name);
                        return ONIGERR_UNDEFINED_NAME_REFERENCE;
                    } else if refs.len() > 1 {
                        self.env.set_error_bytes(&name);
                        return ONIGERR_MULTIPLEX_DEFINITION_NAME_CALL;
                    }
                    called_gnum = refs[0];
                    if let NodeKind::Call { called_gnum: g, .. } = &mut nd!(self).v[cn].kind {
                        *g = called_gnum;
                    }
                }
            }
        }
        // set_call_attr:
        let target = self.env.mem_node(called_gnum);
        nd!(self).set_body(cn, target);
        if target == NULL_NODE {
            self.env.set_error_bytes(&name);
            return ONIGERR_UNDEFINED_NAME_REFERENCE;
        }
        nd!(self).add(target, ND_ST_REFERENCED);
        0
    }
}

fn is_strict_real_node(nodes: &Nodes, node: NodeRef) -> bool {
    match nodes.typ(node) {
        NodeType::String => !nodes.str_bytes(node).is_empty(),
        NodeType::CClass | NodeType::CType => true,
        _ => false,
    }
}

/* ===== tuning ===== */

impl<'e, 'a> Ctx<'e, 'a> {
    fn check_whole_options_position(&self, mut node: NodeRef) -> i32 {
        let mut is_list = false;
        if self.env.flags & PE_FLAG_HAS_CALL_ZERO != 0 {
            node = nd!(self).body(node);
        }
        loop {
            match nd!(self).typ(node) {
                NodeType::List => {
                    if nd!(self).cdr(node) != NULL_NODE {
                        is_list = true;
                    }
                    node = nd!(self).car(node);
                    continue;
                }
                NodeType::Bag => {
                    if bag_typ(&nd!(self), node) == BagType::Option
                        && nd!(self).has(node, ND_ST_WHOLE_OPTIONS)
                    {
                        if is_list && nd!(self).body(node) != NULL_NODE {
                            break;
                        }
                        return 0;
                    }
                    break;
                }
                _ => break,
            }
        }
        ONIGERR_INVALID_GROUP_OPTION
    }

    fn tune_call2_call(&mut self, node: NodeRef) {
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    self.tune_call2_call(nd!(self).car(n));
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::Quant => self.tune_call2_call(nd!(self).body(node)),
            NodeType::Anchor => {
                if anchor_has_body(anchor_typ(&nd!(self), node)) {
                    self.tune_call2_call(nd!(self).body(node));
                }
            }
            NodeType::Bag => {
                let typ = bag_typ(&nd!(self), node);
                if typ == BagType::Memory {
                    if !nd!(self).has(node, ND_ST_MARK1) {
                        nd!(self).add(node, ND_ST_MARK1);
                        self.tune_call2_call(nd!(self).body(node));
                        nd!(self).remove(node, ND_ST_MARK1);
                    }
                } else if typ == BagType::IfElse {
                    self.tune_call2_call(nd!(self).body(node));
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        self.tune_call2_call(then);
                    }
                    if els != NULL_NODE {
                        self.tune_call2_call(els);
                    }
                } else {
                    self.tune_call2_call(nd!(self).body(node));
                }
            }
            NodeType::Call => {
                if !nd!(self).has(node, ND_ST_MARK1) {
                    nd!(self).add(node, ND_ST_MARK1);
                    let called = nd!(self).body(node);
                    if let NodeKind::Call { entry_count, .. } = &mut nd!(self).v[node].kind {
                        *entry_count += 1;
                    }
                    nd!(self).add(called, ND_ST_CALLED);
                    if let NodeKind::Bag { entry_count, .. } = &mut nd!(self).v[called].kind {
                        *entry_count += 1;
                    }
                    self.tune_call2_call(called);
                    nd!(self).remove(node, ND_ST_MARK1);
                }
            }
            _ => {}
        }
    }

    fn tune_call(&mut self, node: NodeRef, mut state: i32) -> i32 {
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let r = self.tune_call(nd!(self).car(n), state);
                    if r != 0 {
                        return r;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::Quant => {
                let (_, upper, _) = quant_params(&nd!(self), node);
                if upper == 0 {
                    state |= IN_ZERO_REPEAT;
                }
                self.tune_call(nd!(self).body(node), state)
            }
            NodeType::Anchor => {
                if anchor_has_body(anchor_typ(&nd!(self), node)) {
                    self.tune_call(nd!(self).body(node), state)
                } else {
                    0
                }
            }
            NodeType::Bag => {
                let typ = bag_typ(&nd!(self), node);
                if typ == BagType::Memory {
                    if (state & IN_ZERO_REPEAT) != 0 {
                        nd!(self).add(node, ND_ST_IN_ZERO_REPEAT);
                        if let NodeKind::Bag { entry_count, .. } = &mut nd!(self).v[node].kind {
                            *entry_count -= 1;
                        }
                    }
                    self.tune_call(nd!(self).body(node), state)
                } else if typ == BagType::IfElse {
                    let r = self.tune_call(nd!(self).body(node), state);
                    if r != 0 {
                        return r;
                    }
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let r = self.tune_call(then, state);
                        if r != 0 {
                            return r;
                        }
                    }
                    if els != NULL_NODE {
                        return self.tune_call(els, state);
                    }
                    0
                } else {
                    self.tune_call(nd!(self).body(node), state)
                }
            }
            NodeType::Call => {
                if (state & IN_ZERO_REPEAT) != 0 {
                    nd!(self).add(node, ND_ST_IN_ZERO_REPEAT);
                    if let NodeKind::Call { entry_count, .. } = &mut nd!(self).v[node].kind {
                        *entry_count -= 1;
                    }
                }
                self.check_call_reference(node)
            }
            _ => 0,
        }
    }

    fn tune_call2(&mut self, node: NodeRef) -> i32 {
        match nd!(self).typ(node) {
            NodeType::List | NodeType::Alt => {
                let mut n = node;
                loop {
                    let r = self.tune_call2(nd!(self).car(n));
                    if r != 0 {
                        return r;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::Quant => {
                let (_, upper, _) = quant_params(&nd!(self), node);
                if upper != 0 {
                    self.tune_call2(nd!(self).body(node))
                } else {
                    0
                }
            }
            NodeType::Anchor => {
                if anchor_has_body(anchor_typ(&nd!(self), node)) {
                    self.tune_call2(nd!(self).body(node))
                } else {
                    0
                }
            }
            NodeType::Bag => {
                let mut r = 0;
                if !nd!(self).has(node, ND_ST_IN_ZERO_REPEAT) {
                    r = self.tune_call2(nd!(self).body(node));
                }
                if r != 0 {
                    return r;
                }
                if bag_typ(&nd!(self), node) == BagType::IfElse {
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let r = self.tune_call2(then);
                        if r != 0 {
                            return r;
                        }
                    }
                    if els != NULL_NODE {
                        r = self.tune_call2(els);
                    }
                }
                r
            }
            NodeType::Call => {
                if !nd!(self).has(node, ND_ST_IN_ZERO_REPEAT) {
                    self.tune_call2_call(node);
                }
                0
            }
            _ => 0,
        }
    }

    fn tune_called_state_call(&mut self, node: NodeRef, mut state: i32) {
        match nd!(self).typ(node) {
            NodeType::Alt | NodeType::List => {
                if nd!(self).typ(node) == NodeType::Alt {
                    state |= IN_ALT;
                }
                let mut n = node;
                loop {
                    self.tune_called_state_call(nd!(self).car(n), state);
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::Quant => {
                let (lower, upper, _) = quant_params(&nd!(self), node);
                if is_infinite_repeat(upper) || upper >= 2 {
                    state |= IN_REAL_REPEAT;
                }
                if lower != upper {
                    state |= IN_VAR_REPEAT;
                }
                if (state & IN_PEEK) != 0 {
                    nd!(self).add(node, ND_ST_INPEEK);
                }
                self.tune_called_state_call(nd!(self).body(node), state);
            }
            NodeType::Anchor => {
                let t = anchor_typ(&nd!(self), node);
                match t {
                    ANCR_PREC_READ_NOT | ANCR_LOOK_BEHIND_NOT => {
                        state |= IN_NOT | IN_PEEK;
                        self.tune_called_state_call(nd!(self).body(node), state);
                    }
                    ANCR_PREC_READ | ANCR_LOOK_BEHIND => {
                        state |= IN_PEEK;
                        self.tune_called_state_call(nd!(self).body(node), state);
                    }
                    _ => {}
                }
            }
            NodeType::Bag => {
                let typ = bag_typ(&nd!(self), node);
                if typ == BagType::Memory {
                    let called_state = match &nd!(self).v[node].kind {
                        NodeKind::Bag { called_state, .. } => *called_state,
                        _ => 0,
                    };
                    if nd!(self).has(node, ND_ST_MARK1) {
                        if (!called_state & state) != 0 {
                            if let NodeKind::Bag { called_state, .. } = &mut nd!(self).v[node].kind
                            {
                                *called_state |= state;
                            }
                            self.tune_called_state_call(nd!(self).body(node), state);
                        }
                    } else {
                        nd!(self).add(node, ND_ST_MARK1);
                        if let NodeKind::Bag { called_state, .. } = &mut nd!(self).v[node].kind {
                            *called_state |= state;
                        }
                        self.tune_called_state_call(nd!(self).body(node), state);
                        nd!(self).remove(node, ND_ST_MARK1);
                    }
                } else if typ == BagType::IfElse {
                    state |= IN_ALT;
                    self.tune_called_state_call(nd!(self).body(node), state);
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        self.tune_called_state_call(then, state);
                    }
                    if els != NULL_NODE {
                        self.tune_called_state_call(els, state);
                    }
                } else {
                    self.tune_called_state_call(nd!(self).body(node), state);
                }
            }
            NodeType::Call => {
                if (state & IN_PEEK) != 0 {
                    nd!(self).add(node, ND_ST_INPEEK);
                }
                if (state & IN_REAL_REPEAT) != 0 {
                    nd!(self).add(node, ND_ST_IN_REAL_REPEAT);
                }
                self.tune_called_state_call(nd!(self).body(node), state);
            }
            _ => {}
        }
    }

    fn tune_called_state(&mut self, node: NodeRef, mut state: i32) {
        match nd!(self).typ(node) {
            NodeType::Alt | NodeType::List => {
                if nd!(self).typ(node) == NodeType::Alt {
                    state |= IN_ALT;
                }
                let mut n = node;
                loop {
                    self.tune_called_state(nd!(self).car(n), state);
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::Call => {
                if (state & IN_PEEK) != 0 {
                    nd!(self).add(node, ND_ST_INPEEK);
                }
                if (state & IN_REAL_REPEAT) != 0 {
                    nd!(self).add(node, ND_ST_IN_REAL_REPEAT);
                }
                self.tune_called_state_call(node, state);
            }
            NodeType::Bag => {
                let typ = bag_typ(&nd!(self), node);
                match typ {
                    BagType::Memory | BagType::Option | BagType::StopBacktrack => {
                        if typ == BagType::Memory {
                            if let NodeKind::Bag {
                                entry_count,
                                called_state,
                                ..
                            } = &mut nd!(self).v[node].kind
                            {
                                if *entry_count > 1 {
                                    state |= IN_MULTI_ENTRY;
                                }
                                *called_state |= state;
                            }
                        }
                        self.tune_called_state(nd!(self).body(node), state);
                    }
                    BagType::IfElse => {
                        state |= IN_ALT;
                        self.tune_called_state(nd!(self).body(node), state);
                        let (then, els) = bag_then_else(&nd!(self), node);
                        if then != NULL_NODE {
                            self.tune_called_state(then, state);
                        }
                        if els != NULL_NODE {
                            self.tune_called_state(els, state);
                        }
                    }
                }
            }
            NodeType::Quant => {
                let (lower, upper, _) = quant_params(&nd!(self), node);
                if is_infinite_repeat(upper) || upper >= 2 {
                    state |= IN_REAL_REPEAT;
                }
                if lower != upper {
                    state |= IN_VAR_REPEAT;
                }
                if (state & IN_PEEK) != 0 {
                    nd!(self).add(node, ND_ST_INPEEK);
                }
                self.tune_called_state(nd!(self).body(node), state);
            }
            NodeType::Anchor => {
                let t = anchor_typ(&nd!(self), node);
                match t {
                    ANCR_PREC_READ_NOT | ANCR_LOOK_BEHIND_NOT => {
                        state |= IN_NOT | IN_PEEK;
                        self.tune_called_state(nd!(self).body(node), state);
                    }
                    ANCR_PREC_READ | ANCR_LOOK_BEHIND => {
                        state |= IN_PEEK;
                        self.tune_called_state(nd!(self).body(node), state);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn tune_anchor(&mut self, node: NodeRef, state: i32) -> i32 {
        let t = anchor_typ(&nd!(self), node);
        match t {
            ANCR_PREC_READ => self.tune_tree(nd!(self).body(node), state | IN_PREC_READ),
            ANCR_PREC_READ_NOT => {
                self.tune_tree(nd!(self).body(node), state | IN_PREC_READ | IN_NOT)
            }
            ANCR_LOOK_BEHIND | ANCR_LOOK_BEHIND_NOT => self.tune_look_behind(node, state),
            _ => 0,
        }
    }

    fn tune_quant(&mut self, node: NodeRef, mut state: i32) -> i32 {
        let body = nd!(self).body(node);
        if (state & IN_REAL_REPEAT) != 0 {
            nd!(self).add(node, ND_ST_IN_REAL_REPEAT);
        }
        if (state & IN_MULTI_ENTRY) != 0 {
            nd!(self).add(node, ND_ST_IN_MULTI_ENTRY);
        }

        let (lower, upper, greedy) = quant_params(&nd!(self), node);
        if is_infinite_repeat(upper) || upper >= 1 {
            let d = self.node_min_byte_len(body);
            if d == 0 {
                let e = self.quantifiers_memory_node_info(body);
                if let NodeKind::Quant { emptiness, .. } = &mut nd!(self).v[node].kind {
                    *emptiness = e;
                }
            }
        }

        if is_infinite_repeat(upper) || upper >= 2 {
            state |= IN_REAL_REPEAT;
        }
        if lower != upper {
            state |= IN_VAR_REPEAT;
        }

        let r = self.tune_tree(body, state);
        if r != 0 {
            return r;
        }
        let body = nd!(self).body(node);

        /* expand string */
        const EXPAND_STRING_MAX_LENGTH: i32 = 100;
        if nd!(self).typ(body) == NodeType::String
            && !is_infinite_repeat(lower)
            && lower == upper
            && lower > 1
            && lower <= EXPAND_STRING_MAX_LENGTH
        {
            let len = nd!(self).str_bytes(body).len() as i32;
            if len * lower <= EXPAND_STRING_MAX_LENGTH {
                let bs = nd!(self).str_bytes(body).to_vec();
                let flag = nd!(self).str_flag(body);
                let status = nd!(self).status(body);
                let mut s = Vec::with_capacity(bs.len() * lower as usize);
                for _ in 0..lower {
                    s.extend_from_slice(&bs);
                }
                /* node_conv_to_str_node: memset 0 (parent too) */
                nd!(self).v[node] = Node {
                    kind: NodeKind::Str { s, flag },
                    status,
                    parent: NULL_NODE,
                };
                return 0;
            }
        }

        let (emptiness, _, _, _) = self.quant_extra(node);
        if greedy && emptiness == BodyEmpty::IsNotEmpty {
            if nd!(self).typ(body) == NodeType::Quant {
                let (_, tqn_head, _, _) = self.quant_extra(body);
                if tqn_head != NULL_NODE {
                    if let NodeKind::Quant { head_exact, .. } = &mut nd!(self).v[node].kind {
                        *head_exact = tqn_head;
                    }
                    if let NodeKind::Quant { head_exact, .. } = &mut nd!(self).v[body].kind {
                        *head_exact = NULL_NODE;
                    }
                }
            } else {
                let he = self.get_tree_head_literal(body, true);
                if let NodeKind::Quant { head_exact, .. } = &mut nd!(self).v[node].kind {
                    *head_exact = he;
                }
            }
        }
        0
    }

    /// tune_tree: empty loop checks, case fold expansion, memory status
    /// flags, head_exact, look-behind checks, repeated string expansion
    pub fn tune_tree(&mut self, node: NodeRef, mut state: i32) -> i32 {
        match nd!(self).typ(node) {
            NodeType::List => {
                let mut prev = NULL_NODE;
                let mut n = node;
                loop {
                    let car = nd!(self).car(n);
                    let mut r = self.tune_tree(car, state);
                    if prev != NULL_NODE && r == 0 {
                        r = self.tune_next(prev, nd!(self).car(n));
                    }
                    if r != 0 {
                        return r;
                    }
                    prev = nd!(self).car(n);
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::Alt => {
                let mut n = node;
                loop {
                    let r = self.tune_tree(nd!(self).car(n), state | IN_ALT);
                    if r != 0 {
                        return r;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
                0
            }
            NodeType::String => {
                if is_real_ignorecase(&nd!(self), node) {
                    self.unravel_case_fold_string(node, state)
                } else {
                    0
                }
            }
            NodeType::BackRef => {
                let backs = backrefs(&nd!(self), node).to_vec();
                for &b in &backs {
                    if b > self.env.num_mem {
                        return ONIGERR_INVALID_BACKREF;
                    }
                    mem_status_on(&mut self.env.backrefed_mem, b);
                    mem_status_on(&mut self.env.backtrack_mem, b);
                }
                0
            }
            NodeType::Bag => {
                let (typ, regnum, body) = self.bag_fields(node);
                match typ {
                    BagType::Option => {
                        let options = self.reg.options;
                        if let NodeKind::Bag { options: o, .. } = nd!(self).v[node].kind {
                            self.reg.options = o;
                        }
                        let r = self.tune_tree(body, state);
                        self.reg.options = options;
                        r
                    }
                    BagType::Memory => {
                        if let NodeKind::Bag { called_state, .. } = nd!(self).v[node].kind {
                            state |= called_state;
                        }
                        if (state & (IN_ALT | IN_NOT | IN_VAR_REPEAT | IN_MULTI_ENTRY)) != 0
                            || nd!(self).has(node, ND_ST_RECURSION)
                        {
                            mem_status_on(&mut self.env.backtrack_mem, regnum);
                        }
                        self.tune_tree(body, state)
                    }
                    BagType::StopBacktrack => {
                        let target = body;
                        let r = self.tune_tree(target, state);
                        let target = nd!(self).body(node);
                        if nd!(self).typ(target) == NodeType::Quant {
                            let (tl, tu, tg) = quant_params(&nd!(self), target);
                            if is_infinite_repeat(tu) && tl <= 1 && tg {
                                let tb = nd!(self).body(target);
                                if is_strict_real_node(&nd!(self), tb) {
                                    nd!(self).add(node, ND_ST_STRICT_REAL_REPEAT);
                                }
                            }
                        }
                        r
                    }
                    BagType::IfElse => {
                        let r = self.tune_tree(body, state | IN_ALT);
                        if r != 0 {
                            return r;
                        }
                        let (then, els) = bag_then_else(&nd!(self), node);
                        if then != NULL_NODE {
                            let r = self.tune_tree(then, state | IN_ALT);
                            if r != 0 {
                                return r;
                            }
                        }
                        if els != NULL_NODE {
                            return self.tune_tree(els, state | IN_ALT);
                        }
                        0
                    }
                }
            }
            NodeType::Quant => {
                if (state & (IN_PREC_READ | IN_LOOK_BEHIND)) != 0 {
                    nd!(self).add(node, ND_ST_INPEEK);
                }
                self.tune_quant(node, state)
            }
            NodeType::Anchor => self.tune_anchor(node, state),
            _ => 0,
        }
    }
}

/* ===== optimization info ===== */

const OPT_EXACT_MAXLEN: usize = 24;

#[derive(Clone, Copy, Default)]
struct OptAnc {
    left: i32,
    right: i32,
}

#[derive(Clone, Copy)]
struct OptStr {
    mm: MinMaxLen,
    anc: OptAnc,
    reach_end: bool,
    len: usize,
    s: [u8; OPT_EXACT_MAXLEN],
}

impl Default for OptStr {
    fn default() -> Self {
        OptStr {
            mm: MinMaxLen::default(),
            anc: OptAnc::default(),
            reach_end: false,
            len: 0,
            s: [0; OPT_EXACT_MAXLEN],
        }
    }
}

#[derive(Clone, Copy)]
struct OptMap {
    mm: MinMaxLen,
    anc: OptAnc,
    value: i32,
    map: [u8; CHAR_MAP_SIZE],
}

impl Default for OptMap {
    fn default() -> Self {
        OptMap {
            mm: MinMaxLen::default(),
            anc: OptAnc::default(),
            value: 0,
            map: [0; CHAR_MAP_SIZE],
        }
    }
}

#[derive(Clone, Copy, Default)]
struct OptNode {
    len: MinMaxLen,
    anc: OptAnc,
    sb: OptStr,
    sm: OptStr,
    spr: OptStr,
    map: OptMap,
}

#[derive(Clone, Copy)]
struct OptEnv {
    mm: MinMaxLen,
}

fn map_position_value(enc: &Encoding, i: usize) -> i32 {
    const VALS: [i16; 128] = [
        5, 1, 1, 1, 1, 1, 1, 1, 1, 10, 10, 1, 1, 10, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 12, 4, 7, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 5,
        5, 5, 5, 5, 5, 5, 6, 6, 6, 6, 7, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6,
        6, 6, 5, 6, 5, 5, 5, 5, 6, 6, 6, 6, 7, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6,
        6, 6, 6, 6, 5, 5, 5, 5, 1,
    ];
    if i < VALS.len() {
        if i == 0 && enc.min_enc_len > 1 {
            20
        } else {
            VALS[i] as i32
        }
    } else {
        4
    }
}

fn distance_value(mm: &MinMaxLen) -> i32 {
    const DIST_VALS: [i16; 100] = [
        1000, 500, 333, 250, 200, 167, 143, 125, 111, 100, 91, 83, 77, 71, 67, 63, 59, 56, 53, 50,
        48, 45, 43, 42, 40, 38, 37, 36, 34, 33, 32, 31, 30, 29, 29, 28, 27, 26, 26, 25, 24, 24, 23,
        23, 22, 22, 21, 21, 20, 20, 20, 19, 19, 19, 18, 18, 18, 17, 17, 17, 16, 16, 16, 16, 15, 15,
        15, 15, 14, 14, 14, 14, 14, 14, 13, 13, 13, 13, 13, 13, 12, 12, 12, 12, 12, 12, 11, 11, 11,
        11, 11, 11, 11, 11, 11, 10, 10, 10, 10, 10,
    ];
    if mm.max == INFINITE_LEN {
        return 0;
    }
    let d = mm.max.wrapping_sub(mm.min);
    if (d as usize) < DIST_VALS.len() {
        DIST_VALS[d as usize] as i32
    } else {
        1
    }
}

fn comp_distance_value(d1: &MinMaxLen, d2: &MinMaxLen, mut v1: i32, mut v2: i32) -> i32 {
    if v2 <= 0 {
        return -1;
    }
    if v1 <= 0 {
        return 1;
    }
    v1 *= distance_value(d1);
    v2 *= distance_value(d2);
    if v2 > v1 {
        return 1;
    }
    if v2 < v1 {
        return -1;
    }
    if d2.min < d1.min {
        return 1;
    }
    if d2.min > d1.min {
        return -1;
    }
    0
}

fn concat_opt_anc_info(
    to: &mut OptAnc,
    left: &OptAnc,
    right: &OptAnc,
    left_len: u32,
    right_len: u32,
) {
    *to = OptAnc::default();
    to.left = left.left;
    if left_len == 0 {
        to.left |= right.left;
    }
    to.right = right.right;
    if right_len == 0 {
        to.right |= left.right;
    } else {
        to.right |= left.right & ANCR_PREC_READ_NOT;
    }
}

fn is_left(a: i32) -> bool {
    !(a == ANCR_END_BUF
        || a == ANCR_SEMI_END_BUF
        || a == ANCR_END_LINE
        || a == ANCR_PREC_READ
        || a == ANCR_PREC_READ_NOT)
}

fn is_set_opt_anc_info(to: &OptAnc, anc: i32) -> bool {
    (to.left & anc) != 0 || (to.right & anc) != 0
}

fn add_opt_anc_info(to: &mut OptAnc, anc: i32) {
    if is_left(anc) {
        to.left |= anc;
    } else {
        to.right |= anc;
    }
}

fn remove_opt_anc_info(to: &mut OptAnc, anc: i32) {
    if is_left(anc) {
        to.left &= !anc;
    } else {
        to.right &= !anc;
    }
}

fn alt_merge_opt_anc_info(to: &mut OptAnc, add: &OptAnc) {
    to.left &= add.left;
    to.right &= add.right;
}

fn is_full_opt_exact(e: &OptStr) -> bool {
    e.len >= OPT_EXACT_MAXLEN
}

fn clear_opt_exact(e: &mut OptStr) {
    mml_clear(&mut e.mm);
    e.anc = OptAnc::default();
    e.reach_end = false;
    e.len = 0;
    e.s[0] = 0;
}

fn concat_opt_exact(to: &mut OptStr, add: &OptStr, enc: &Encoding) -> i32 {
    let mut r = 0;
    let mut p = 0usize;
    let end = add.len;
    let mut i = to.len;
    while p < end {
        let len = enc.mbc_enc_len(&add.s, p);
        if i + len > OPT_EXACT_MAXLEN {
            r = 1;
            break;
        }
        let mut j = 0;
        while j < len && p < end {
            to.s[i] = add.s[p];
            i += 1;
            p += 1;
            j += 1;
        }
    }
    to.len = i;
    to.reach_end = if p == end { add.reach_end } else { false };

    let mut tanc = OptAnc::default();
    concat_opt_anc_info(&mut tanc, &to.anc, &add.anc, 1, 1);
    if !to.reach_end {
        tanc.right = 0;
    }
    to.anc = tanc;
    r
}

fn concat_opt_exact_str(to: &mut OptStr, s: &[u8], enc: &Encoding) {
    let mut i = to.len;
    let mut p = 0usize;
    let end = s.len();
    while p < end && i < OPT_EXACT_MAXLEN {
        let len = enc.mbc_enc_len(s, p);
        if i + len > OPT_EXACT_MAXLEN {
            break;
        }
        let mut j = 0;
        while j < len && p < end {
            to.s[i] = s[p];
            i += 1;
            p += 1;
            j += 1;
        }
    }
    to.len = i;
    if p >= end {
        to.reach_end = true;
    }
}

fn alt_merge_opt_exact(to: &mut OptStr, add: &OptStr, enc: &Encoding) {
    if add.len == 0 || to.len == 0 {
        clear_opt_exact(to);
        return;
    }
    if !mml_is_equal(&to.mm, &add.mm) {
        clear_opt_exact(to);
        return;
    }
    let mut i = 0usize;
    while i < to.len && i < add.len {
        if to.s[i] != add.s[i] {
            break;
        }
        let len = enc.mbc_enc_len(&to.s, i);
        let mut j = 1;
        while j < len {
            if to.s.get(i + j) != add.s.get(i + j) {
                break;
            }
            j += 1;
        }
        if j < len {
            break;
        }
        i += len;
    }
    if !add.reach_end || i < add.len || i < to.len {
        to.reach_end = false;
    }
    to.len = i;
    alt_merge_opt_anc_info(&mut to.anc, &add.anc);
    if !to.reach_end {
        to.anc.right = 0;
    }
}

fn select_opt_exact(enc: &Encoding, now: &mut OptStr, alt: &OptStr) {
    let mut vn = now.len as i32;
    let mut va = alt.len as i32;
    if va == 0 {
        return;
    } else if vn == 0 {
        *now = *alt;
        return;
    } else if vn <= 2 && va <= 2 {
        va = map_position_value(enc, now.s[0] as usize);
        vn = map_position_value(enc, alt.s[0] as usize);
        if now.len > 1 {
            vn += 5;
        }
        if alt.len > 1 {
            va += 5;
        }
    }
    vn *= 2;
    va *= 2;
    if comp_distance_value(&now.mm, &alt.mm, vn, va) > 0 {
        *now = *alt;
    }
}

fn clear_opt_map(map: &mut OptMap) {
    *map = OptMap::default();
}

fn add_char_opt_map(m: &mut OptMap, c: u8, enc: &Encoding) {
    if m.map[c as usize] == 0 {
        m.map[c as usize] = 1;
        m.value += map_position_value(enc, c as usize);
    }
}

fn select_opt_map(now: &mut OptMap, alt: &OptMap) {
    const Z: i32 = 1 << 15;
    if alt.value == 0 {
        return;
    }
    if now.value == 0 {
        *now = *alt;
        return;
    }
    let vn = Z / now.value;
    let va = Z / alt.value;
    if comp_distance_value(&now.mm, &alt.mm, vn, va) > 0 {
        *now = *alt;
    }
}

fn comp_opt_exact_or_map(e: &OptStr, m: &OptMap) -> i32 {
    const COMP_EM_BASE: i32 = 20;
    if m.value <= 0 {
        return -1;
    }
    let case_value = 3;
    let ae = COMP_EM_BASE * e.len as i32 * case_value;
    let am = COMP_EM_BASE * 5 * 2 / m.value;
    comp_distance_value(&e.mm, &m.mm, ae, am)
}

fn alt_merge_opt_map(enc: &Encoding, to: &mut OptMap, add: &OptMap) {
    if to.value == 0 {
        return;
    }
    if add.value == 0 || to.mm.max < add.mm.min {
        clear_opt_map(to);
        return;
    }
    mml_alt_merge(&mut to.mm, &add.mm);
    let mut val = 0;
    for i in 0..CHAR_MAP_SIZE {
        if add.map[i] != 0 {
            to.map[i] = 1;
        }
        if to.map[i] != 0 {
            val += map_position_value(enc, i);
        }
    }
    to.value = val;
    alt_merge_opt_anc_info(&mut to.anc, &add.anc);
}

fn set_bound_node_opt_info(opt: &mut OptNode, plen: &MinMaxLen) {
    opt.sb.mm = *plen;
    opt.spr.mm = *plen;
    opt.map.mm = *plen;
}

fn clear_node_opt_info(opt: &mut OptNode) {
    mml_clear(&mut opt.len);
    opt.anc = OptAnc::default();
    clear_opt_exact(&mut opt.sb);
    clear_opt_exact(&mut opt.sm);
    clear_opt_exact(&mut opt.spr);
    clear_opt_map(&mut opt.map);
}

fn concat_left_node_opt_info(enc: &Encoding, to: &mut OptNode, add: &mut OptNode) {
    let mut tanc = OptAnc::default();
    concat_opt_anc_info(&mut tanc, &to.anc, &add.anc, to.len.max, add.len.max);
    to.anc = tanc;

    if add.sb.len > 0 && to.len.max == 0 {
        concat_opt_anc_info(&mut tanc, &to.anc, &add.sb.anc, to.len.max, add.len.max);
        add.sb.anc = tanc;
    }

    if add.map.value > 0 && to.len.max == 0 && add.map.mm.max == 0 {
        add.map.anc.left |= to.anc.left;
    }

    let sb_reach = to.sb.reach_end;
    let sm_reach = to.sm.reach_end;

    if add.len.max != 0 {
        to.sb.reach_end = false;
        to.sm.reach_end = false;
    }

    if add.sb.len > 0 {
        if sb_reach {
            concat_opt_exact(&mut to.sb, &add.sb, enc);
            clear_opt_exact(&mut add.sb);
        } else if sm_reach {
            concat_opt_exact(&mut to.sm, &add.sb, enc);
            clear_opt_exact(&mut add.sb);
        }
    }
    select_opt_exact(enc, &mut to.sm, &add.sb);
    select_opt_exact(enc, &mut to.sm, &add.sm);

    if to.spr.len > 0 {
        if add.len.max > 0 {
            if to.spr.mm.max == 0 {
                let spr = to.spr;
                select_opt_exact(enc, &mut to.sb, &spr);
            } else {
                let spr = to.spr;
                select_opt_exact(enc, &mut to.sm, &spr);
            }
        }
    } else if add.spr.len > 0 {
        to.spr = add.spr;
    }

    select_opt_map(&mut to.map, &add.map);
    mml_add(&mut to.len, &add.len);
}

fn alt_merge_node_opt_info(to: &mut OptNode, add: &OptNode, enc: &Encoding) {
    alt_merge_opt_anc_info(&mut to.anc, &add.anc);
    alt_merge_opt_exact(&mut to.sb, &add.sb, enc);
    alt_merge_opt_exact(&mut to.sm, &add.sm, enc);
    alt_merge_opt_exact(&mut to.spr, &add.spr, enc);
    alt_merge_opt_map(enc, &mut to.map, &add.map);
    mml_alt_merge(&mut to.len, &add.len);
}

const MAX_ND_OPT_INFO_REF_COUNT: i32 = 5;

impl<'e, 'a> Ctx<'e, 'a> {
    fn node_max_byte_len(&mut self, node: NodeRef) -> u32 {
        let mut len: u32 = 0;
        match nd!(self).typ(node) {
            NodeType::List => {
                let mut n = node;
                loop {
                    let tmax = self.node_max_byte_len(nd!(self).car(n));
                    len = distance_add(len, tmax);
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::Alt => {
                let mut n = node;
                loop {
                    let tmax = self.node_max_byte_len(nd!(self).car(n));
                    if len < tmax {
                        len = tmax;
                    }
                    n = nd!(self).cdr(n);
                    if n == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::String => len = nd!(self).str_bytes(node).len() as u32,
            NodeType::CType | NodeType::CClass => len = self.reg.enc.max_enc_len as u32,
            NodeType::BackRef => {
                if !nd!(self).has(node, ND_ST_CHECKER) {
                    if nd!(self).has(node, ND_ST_RECURSION) {
                        if nd!(self).has(node, ND_ST_NEST_LEVEL) {
                            len = INFINITE_LEN;
                        }
                    } else {
                        let backs = backrefs(&nd!(self), node).to_vec();
                        for &b in &backs {
                            let tmax = self.node_max_byte_len(self.env.mem_node(b));
                            if len < tmax {
                                len = tmax;
                            }
                        }
                    }
                }
            }
            NodeType::Call => {
                if !nd!(self).has(node, ND_ST_RECURSION) {
                    len = self.node_max_byte_len(nd!(self).body(node));
                } else {
                    len = INFINITE_LEN;
                }
            }
            NodeType::Quant => {
                let (_, upper, _) = quant_params(&nd!(self), node);
                if upper != 0 {
                    len = self.node_max_byte_len(nd!(self).body(node));
                    if len != 0 {
                        if !is_infinite_repeat(upper) {
                            len = distance_multiply(len, upper);
                        } else {
                            len = INFINITE_LEN;
                        }
                    }
                }
            }
            NodeType::Bag => match bag_typ(&nd!(self), node) {
                BagType::Memory => {
                    if nd!(self).has(node, ND_ST_FIXED_MAX) {
                        len = match &nd!(self).v[node].kind {
                            NodeKind::Bag { max_len, .. } => *max_len,
                            _ => 0,
                        };
                    } else if nd!(self).has(node, ND_ST_MARK1) {
                        len = INFINITE_LEN;
                    } else {
                        nd!(self).add(node, ND_ST_MARK1);
                        len = self.node_max_byte_len(nd!(self).body(node));
                        nd!(self).remove(node, ND_ST_MARK1);
                        if let NodeKind::Bag { max_len, .. } = &mut nd!(self).v[node].kind {
                            *max_len = len;
                        }
                        nd!(self).add(node, ND_ST_FIXED_MAX);
                    }
                }
                BagType::Option | BagType::StopBacktrack => {
                    len = self.node_max_byte_len(nd!(self).body(node));
                }
                BagType::IfElse => {
                    len = self.node_max_byte_len(nd!(self).body(node));
                    let (then, els) = bag_then_else(&nd!(self), node);
                    if then != NULL_NODE {
                        let tlen = self.node_max_byte_len(then);
                        len = distance_add(len, tlen);
                    }
                    let elen = if els != NULL_NODE {
                        self.node_max_byte_len(els)
                    } else {
                        0
                    };
                    if elen > len {
                        len = elen;
                    }
                }
            },
            _ => {}
        }
        len
    }

    fn optimize_nodes(&mut self, node: NodeRef, opt: &mut OptNode, env: &OptEnv) -> i32 {
        let enc = self.reg.enc;
        let mut r = 0;
        let mut xo = OptNode::default();
        clear_node_opt_info(opt);
        set_bound_node_opt_info(opt, &env.mm);

        match nd!(self).typ(node) {
            NodeType::List => {
                let mut nenv = *env;
                let mut nd = node;
                loop {
                    r = self.optimize_nodes(nd!(self).car(nd), &mut xo, &nenv);
                    if r == 0 {
                        mml_add(&mut nenv.mm, &xo.len);
                        concat_left_node_opt_info(enc, opt, &mut xo);
                    }
                    if r != 0 {
                        break;
                    }
                    nd = nd!(self).cdr(nd);
                    if nd == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::Alt => {
                let mut nd = node;
                loop {
                    r = self.optimize_nodes(nd!(self).car(nd), &mut xo, env);
                    if r == 0 {
                        if nd == node {
                            *opt = xo;
                        } else {
                            alt_merge_node_opt_info(opt, &xo, enc);
                        }
                    }
                    if r != 0 {
                        break;
                    }
                    nd = nd!(self).cdr(nd);
                    if nd == NULL_NODE {
                        break;
                    }
                }
            }
            NodeType::String => {
                let s = nd!(self).str_bytes(node).to_vec();
                let slen = s.len() as u32;
                concat_opt_exact_str(&mut opt.sb, &s, enc);
                if slen > 0 {
                    add_char_opt_map(&mut opt.map, s[0], enc);
                }
                mml_set_min_max(&mut opt.len, slen, slen);
            }
            NodeType::CClass => {
                let cc = nd!(self).cclass(node).clone();
                if cc.mbuf.is_some() || cc.not {
                    let min = enc.min_enc_len as u32;
                    let max = enc.max_enc_len as u32;
                    mml_set_min_max(&mut opt.len, min, max);
                } else {
                    for i in 0..SINGLE_BYTE_SIZE {
                        let z = bitset_at(&cc.bs, i);
                        if (z && !cc.not) || (!z && cc.not) {
                            add_char_opt_map(&mut opt.map, i as u8, enc);
                        }
                    }
                    mml_set_min_max(&mut opt.len, 1, 1);
                }
            }
            NodeType::CType => {
                let max = enc.max_enc_len as u32;
                let min;
                if max == 1 {
                    min = 1;
                    let (ct, not, ascii_mode) = ctype_fields(&nd!(self), node);
                    if ct == ONIGENC_CTYPE_WORD as i32 {
                        let range = if ascii_mode { 128 } else { SINGLE_BYTE_SIZE };
                        if not {
                            for i in 0..range {
                                if !enc.is_code_word(i) {
                                    add_char_opt_map(&mut opt.map, i as u8, enc);
                                }
                            }
                            for i in range..SINGLE_BYTE_SIZE {
                                add_char_opt_map(&mut opt.map, i as u8, enc);
                            }
                        } else {
                            for i in 0..range {
                                if enc.is_code_word(i) {
                                    add_char_opt_map(&mut opt.map, i as u8, enc);
                                }
                            }
                        }
                    }
                } else {
                    min = enc.min_enc_len as u32;
                }
                mml_set_min_max(&mut opt.len, min, max);
            }
            NodeType::Anchor => {
                let t = anchor_typ(&nd!(self), node);
                match t {
                    ANCR_BEGIN_BUF | ANCR_BEGIN_POSITION | ANCR_BEGIN_LINE | ANCR_END_BUF
                    | ANCR_SEMI_END_BUF | ANCR_END_LINE | ANCR_PREC_READ_NOT | ANCR_LOOK_BEHIND => {
                        add_opt_anc_info(&mut opt.anc, t);
                    }
                    ANCR_PREC_READ => {
                        r = self.optimize_nodes(nd!(self).body(node), &mut xo, env);
                        if r == 0 {
                            if xo.sb.len > 0 {
                                opt.spr = xo.sb;
                            } else if xo.sm.len > 0 {
                                opt.spr = xo.sm;
                            }
                            opt.spr.reach_end = false;
                            if xo.map.value > 0 {
                                opt.map = xo.map;
                            }
                        }
                    }
                    _ => {}
                }
            }
            NodeType::BackRef => {
                if !nd!(self).has(node, ND_ST_CHECKER) {
                    let min = self.node_min_byte_len(node);
                    let max = self.node_max_byte_len(node);
                    mml_set_min_max(&mut opt.len, min, max);
                }
            }
            NodeType::Call => {
                if nd!(self).has(node, ND_ST_RECURSION) {
                    mml_set_min_max(&mut opt.len, 0, INFINITE_LEN);
                } else {
                    r = self.optimize_nodes(nd!(self).body(node), opt, env);
                }
            }
            NodeType::Quant => {
                let (lower, upper, greedy) = quant_params(&nd!(self), node);
                if upper == 0 {
                    mml_set_min_max(&mut opt.len, 0, 0);
                    return 0;
                }
                let body = nd!(self).body(node);
                r = self.optimize_nodes(body, &mut xo, env);
                if r != 0 {
                    return r;
                }
                if lower > 0 {
                    *opt = xo;
                    if xo.sb.len > 0 && xo.sb.reach_end {
                        let mut i = 2;
                        while i <= lower && !is_full_opt_exact(&opt.sb) {
                            let rc = concat_opt_exact(&mut opt.sb, &xo.sb, enc);
                            if rc > 0 {
                                break;
                            }
                            i += 1;
                        }
                        if i < lower {
                            opt.sb.reach_end = false;
                        }
                    }
                    if lower != upper {
                        opt.sb.reach_end = false;
                        opt.sm.reach_end = false;
                    }
                    if lower > 1 {
                        opt.sm.reach_end = false;
                    }
                }
                let max;
                if is_infinite_repeat(upper) {
                    if env.mm.max == 0 && nd!(self).is_anychar(body) && greedy {
                        if nd!(self).has(body, ND_ST_MULTILINE) {
                            add_opt_anc_info(&mut opt.anc, ANCR_ANYCHAR_INF_ML);
                        } else {
                            add_opt_anc_info(&mut opt.anc, ANCR_ANYCHAR_INF);
                        }
                    }
                    max = if xo.len.max > 0 { INFINITE_LEN } else { 0 };
                } else {
                    max = distance_multiply(xo.len.max, upper);
                }
                let min = distance_multiply(xo.len.min, lower);
                mml_set_min_max(&mut opt.len, min, max);
            }
            NodeType::Bag => {
                let (typ, regnum, body) = self.bag_fields(node);
                match typ {
                    BagType::StopBacktrack | BagType::Option => {
                        r = self.optimize_nodes(body, opt, env);
                    }
                    BagType::Memory => {
                        let opt_count =
                            if let NodeKind::Bag { opt_count, .. } = &mut nd!(self).v[node].kind {
                                *opt_count += 1;
                                *opt_count
                            } else {
                                0
                            };
                        if opt_count > MAX_ND_OPT_INFO_REF_COUNT {
                            let mut min = 0;
                            let mut max = INFINITE_LEN;
                            let (ml, xl) = match &nd!(self).v[node].kind {
                                NodeKind::Bag {
                                    min_len, max_len, ..
                                } => (*min_len, *max_len),
                                _ => (0, 0),
                            };
                            if nd!(self).has(node, ND_ST_FIXED_MIN) {
                                min = ml;
                            }
                            if nd!(self).has(node, ND_ST_FIXED_MAX) {
                                max = xl;
                            }
                            mml_set_min_max(&mut opt.len, min, max);
                        } else {
                            r = self.optimize_nodes(body, opt, env);
                            if is_set_opt_anc_info(&opt.anc, ANCR_ANYCHAR_INF_MASK)
                                && mem_status_at0(self.env.backrefed_mem, regnum)
                            {
                                remove_opt_anc_info(&mut opt.anc, ANCR_ANYCHAR_INF_MASK);
                            }
                        }
                    }
                    BagType::IfElse => {
                        let (then, els) = bag_then_else(&nd!(self), node);
                        if els != NULL_NODE {
                            let mut nenv = *env;
                            r = self.optimize_nodes(body, &mut xo, &nenv);
                            if r == 0 {
                                mml_add(&mut nenv.mm, &xo.len);
                                concat_left_node_opt_info(enc, opt, &mut xo);
                                if then != NULL_NODE {
                                    r = self.optimize_nodes(then, &mut xo, &nenv);
                                    if r == 0 {
                                        concat_left_node_opt_info(enc, opt, &mut xo);
                                    }
                                }
                                r = self.optimize_nodes(els, &mut xo, env);
                                if r == 0 {
                                    alt_merge_node_opt_info(opt, &xo, enc);
                                }
                            }
                        }
                    }
                }
            }
            NodeType::Gimmick => {}
        }
        r
    }

    fn set_sunday_quick_search_or_bmh_skip_table(&mut self, case_expand: bool) -> i32 {
        let enc = self.reg.enc;
        let s = self.reg.exact.clone();
        let end = s.len();
        let mut offset = enc.skip_offset() as i32;
        if offset == ENC_SKIP_OFFSET_1_OR_0 as i32 {
            let mut p = 0usize;
            loop {
                let len = enc.mbc_enc_len(&s, p);
                if p + len >= end {
                    offset = if len == 1 { 1 } else { 0 };
                    break;
                }
                p += len;
            }
        }
        let len = end as i32;
        if len + offset >= 255 {
            return ONIGERR_PARSER_BUG;
        }
        self.reg.map_offset = offset;
        for i in 0..CHAR_MAP_SIZE {
            self.reg.map[i] = (len + offset) as u8;
        }
        let mut p = 0usize;
        let mut items = [CaseFoldCodeItem::default(); ONIGENC_GET_CASE_FOLD_CODES_MAX_NUM];
        while p < end {
            let mut clen = enc.mbc_enc_len(&s, p);
            if p + clen > end {
                clen = end - p;
            }
            let len = (end - p) as i32;
            for j in 0..clen {
                let z = len - j as i32 + (offset - 1);
                if z <= 0 {
                    break;
                }
                self.reg.map[s[p + j] as usize] = z as u8;
            }
            if case_expand {
                let n =
                    enc.get_case_fold_codes_by_str(self.reg.case_fold_flag, &s, p, end, &mut items);
                for k in 0..n.max(0) as usize {
                    let mut buf = [0u8; ONIGENC_CODE_TO_MBC_MAXLEN];
                    enc.code_to_mbc(items[k].code[0], &mut buf);
                    for j in 0..clen {
                        let z = len - j as i32 + (offset - 1);
                        if z <= 0 {
                            break;
                        }
                        if self.reg.map[buf[j] as usize] as i32 > z {
                            self.reg.map[buf[j] as usize] = z as u8;
                        }
                    }
                }
            }
            p += clen;
        }
        0
    }

    fn set_optimize_exact(&mut self, e: &OptStr) -> i32 {
        if e.len == 0 {
            return 0;
        }
        self.reg.exact = e.s[..e.len].to_vec();
        let allow_reverse = {
            let ex = &self.reg.exact;
            self.reg.enc.is_allowed_reverse_match(ex, 0, ex.len())
        };
        if e.len >= 2 || (e.len >= 1 && allow_reverse) {
            let r = self.set_sunday_quick_search_or_bmh_skip_table(false);
            if r != 0 {
                return r;
            }
            self.reg.optimize = if allow_reverse {
                OPTIMIZE_STR_FAST
            } else {
                OPTIMIZE_STR_FAST_STEP_FORWARD
            };
        } else {
            self.reg.optimize = OPTIMIZE_STR;
        }
        self.reg.dist_min = e.mm.min;
        self.reg.dist_max = e.mm.max;
        if self.reg.dist_min != INFINITE_LEN {
            let n = self.reg.exact.len() as u32;
            self.reg.threshold_len = self.reg.dist_min.wrapping_add(n) as i32;
        }
        0
    }

    fn set_optimize_map(&mut self, m: &OptMap) {
        self.reg.map = m.map;
        self.reg.optimize = OPTIMIZE_MAP;
        self.reg.dist_min = m.mm.min;
        self.reg.dist_max = m.mm.max;
        if self.reg.dist_min != INFINITE_LEN {
            self.reg.threshold_len =
                self.reg
                    .dist_min
                    .wrapping_add(self.reg.enc.min_enc_len as u32) as i32;
        }
    }

    fn set_sub_anchor(&mut self, anc: &OptAnc) {
        self.reg.sub_anchor |= anc.left & ANCR_BEGIN_LINE;
        self.reg.sub_anchor |= anc.right & ANCR_END_LINE;
    }

    fn set_optimize_info_from_tree(&mut self, node: NodeRef) -> i32 {
        let env = OptEnv {
            mm: MinMaxLen::default(),
        };
        let mut opt = OptNode::default();
        let r = self.optimize_nodes(node, &mut opt, &env);
        if r != 0 {
            return r;
        }

        self.reg.anchor = opt.anc.left
            & (ANCR_BEGIN_BUF
                | ANCR_BEGIN_POSITION
                | ANCR_ANYCHAR_INF
                | ANCR_ANYCHAR_INF_ML
                | ANCR_LOOK_BEHIND);
        if (opt.anc.left & (ANCR_LOOK_BEHIND | ANCR_PREC_READ_NOT)) != 0 {
            self.reg.anchor &= !ANCR_ANYCHAR_INF_ML;
        }
        self.reg.anchor |= opt.anc.right & (ANCR_END_BUF | ANCR_SEMI_END_BUF | ANCR_PREC_READ_NOT);

        if (self.reg.anchor & (ANCR_END_BUF | ANCR_SEMI_END_BUF)) != 0 {
            self.reg.anc_dist_min = opt.len.min;
            self.reg.anc_dist_max = opt.len.max;
        }

        let mut r = 0;
        if opt.sb.len > 0 || opt.sm.len > 0 {
            let sm = opt.sm;
            select_opt_exact(self.reg.enc, &mut opt.sb, &sm);
            if opt.map.value > 0 && comp_opt_exact_or_map(&opt.sb, &opt.map) > 0 {
                self.set_optimize_map(&opt.map);
                self.set_sub_anchor(&opt.map.anc);
            } else {
                r = self.set_optimize_exact(&opt.sb);
                self.set_sub_anchor(&opt.sb.anc);
            }
        } else if opt.map.value > 0 {
            self.set_optimize_map(&opt.map);
            self.set_sub_anchor(&opt.map.anc);
        } else {
            self.reg.sub_anchor |= opt.anc.left & ANCR_BEGIN_LINE;
            if opt.len.max == 0 {
                self.reg.sub_anchor |= opt.anc.right & ANCR_END_LINE;
            }
        }
        r
    }
}

/* ===== compile driver ===== */

impl<'e, 'a> Ctx<'e, 'a> {
    fn parse_and_tune(&mut self, rroot: &mut NodeRef) -> i32 {
        let mut root = NULL_NODE;
        let r = self.env.parse_tree(&mut root);
        if r != 0 {
            return r;
        }
        self.reg.num_mem = self.env.num_mem;
        self.reg.options = self.env.reg_options;
        self.reg.case_fold_flag = self.env.reg_case_fold_flag;

        if self.env.flags & PE_FLAG_HAS_WHOLE_OPTIONS != 0 {
            let r = self.check_whole_options_position(root);
            if r != 0 {
                return r;
            }
        }

        let r = self.reduce_string_list(root);
        if r != 0 {
            return r;
        }

        if self.env.num_named > 0
            && self.env.syntax.bv(ONIG_SYN_CAPTURE_ONLY_NAMED_GROUP)
            && !opton_capture_group(self.reg.options)
        {
            let r = if self.env.num_named != self.env.num_mem {
                self.disable_noname_group_capture(&mut root)
            } else {
                self.numbered_ref_check(root)
            };
            if r != 0 {
                return r;
            }
        }

        let r = self.check_backrefs(root);
        if r != 0 {
            return r;
        }

        if self.env.num_call > 0 {
            let r = self.tune_call(root, 0);
            if r != 0 {
                return r;
            }
            let r = self.tune_call2(root);
            if r != 0 {
                return r;
            }
            let r = self.recursive_call_check_trav(root, 0);
            if r < 0 {
                return r;
            }
            let r = self.infinite_recursive_call_check_trav(root);
            if r != 0 {
                return r;
            }
            self.tune_called_state(root, 0);
        }
        self.reg.num_call = self.env.num_call;

        let r = self.tune_tree(root, 0);
        if r != 0 {
            return r;
        }

        if self.env.backref_num != 0 {
            self.set_parent_node_trav(root, NULL_NODE);
            let r = self.set_empty_repeat_node_trav(root, NULL_NODE);
            if r != 0 {
                return r;
            }
            self.set_empty_status_check_trav(root);
        }

        *rroot = root;
        0
    }

    fn compile(&mut self) -> i32 {
        let mut root = NULL_NODE;
        let r = self.parse_and_tune(&mut root);
        if r != 0 {
            return r;
        }

        self.reg.capture_history = self.env.cap_history;
        self.reg.push_mem_start = self.env.backtrack_mem | self.env.cap_history;
        if !self.env.callout_list.is_empty() {
            self.reg.push_mem_end = self.reg.push_mem_start;
        } else if mem_status_is_all_on(self.reg.push_mem_start) {
            self.reg.push_mem_end = self.env.backrefed_mem | self.env.cap_history;
        } else {
            self.reg.push_mem_end =
                self.reg.push_mem_start & (self.env.backrefed_mem | self.env.cap_history);
        }
        self.reg.callout_num = self.env.callout_list.len() as i32;

        let r = self.set_optimize_info_from_tree(root);
        if r != 0 {
            return r;
        }

        let r = self.compile_tree(root);
        if r != 0 {
            return r;
        }
        if self.env.keep_num > 0 {
            self.add_op(Op::UpdateVar(UPDATE_VAR_KEEP_FROM_STACK_LAST, 0, false));
        }
        self.add_op(Op::End);

        if self.env.num_call > 0 {
            let r = self.fix_unset_addr_list();
            if r != 0 {
                return r;
            }
        }

        if self.reg.push_mem_end != 0 || self.reg.callout_num != 0 || self.env.num_call > 0 {
            self.reg.stack_pop_level = STACK_POP_LEVEL_ALL;
        } else if self.reg.push_mem_start != 0 {
            self.reg.stack_pop_level = STACK_POP_LEVEL_MEM_START;
        } else {
            self.reg.stack_pop_level = STACK_POP_LEVEL_FREE;
        }
        0
    }
}

/// Compile error: code and the einfo parameter (error string)
#[derive(Debug, Clone)]
pub struct CompileError {
    pub code: i32,
    pub par: Option<Vec<u8>>,
}

/// onig_new
pub fn onig_new(
    pattern: &[u8],
    mut option: u32,
    enc: &'static Encoding,
    syntax: &'static SyntaxType,
) -> Result<Regex, CompileError> {
    let mut case_fold_flag = ONIGENC_CASE_FOLD_DEFAULT;

    /* onig_reg_init */
    if (option & (ONIG_OPTION_DONT_CAPTURE_GROUP | ONIG_OPTION_CAPTURE_GROUP))
        == (ONIG_OPTION_DONT_CAPTURE_GROUP | ONIG_OPTION_CAPTURE_GROUP)
    {
        return Err(CompileError {
            code: ONIGERR_INVALID_COMBINATION_OF_OPTIONS,
            par: None,
        });
    }
    if (option & ONIG_OPTION_NEGATE_SINGLELINE) != 0 {
        option |= syntax.options;
        option &= !ONIG_OPTION_SINGLELINE;
    } else {
        option |= syntax.options;
    }
    if (option & ONIG_OPTION_IGNORECASE_IS_ASCII) != 0 {
        case_fold_flag &=
            !(INTERNAL_ONIGENC_CASE_FOLD_MULTI_CHAR | ONIGENC_CASE_FOLD_TURKISH_AZERI);
        case_fold_flag |= ONIGENC_CASE_FOLD_ASCII_ONLY;
    }

    let reg = Regex {
        ops: Vec::new(),
        num_mem: 0,
        num_repeat: 0,
        num_empty_check: 0,
        num_call: 0,
        capture_history: 0,
        push_mem_start: 0,
        push_mem_end: 0,
        stack_pop_level: 0,
        repeat_range: Vec::new(),
        enc,
        options: option,
        syntax,
        case_fold_flag,
        names: NameTable::default(),
        optimize: OPTIMIZE_NONE,
        threshold_len: 0,
        anchor: 0,
        anc_dist_min: 0,
        anc_dist_max: 0,
        sub_anchor: 0,
        exact: Vec::new(),
        map: [0; CHAR_MAP_SIZE],
        map_offset: 0,
        dist_min: 0,
        dist_max: 0,
        callout_num: 0,
    };

    let mut env = ParseEnv::new(pattern, option, case_fold_flag, enc, syntax);
    let mut ctx = Ctx { env: &mut env, reg };
    let r = ctx.compile();
    if r != 0 {
        return Err(CompileError {
            code: r,
            par: ctx.env.error.clone(),
        });
    }
    let mut reg = ctx.reg;
    reg.names = std::mem::take(&mut env.names);
    Ok(reg)
}

fn error_code_to_format(code: i32) -> &'static str {
    match code {
        ONIG_MISMATCH => "mismatch",
        ONIG_NO_SUPPORT_CONFIG => "no support in this configuration",
        ONIG_ABORT => "abort",
        ONIGERR_MEMORY => "fail to memory allocation",
        ONIGERR_MATCH_STACK_LIMIT_OVER => "match-stack limit over",
        ONIGERR_PARSE_DEPTH_LIMIT_OVER => "parse depth limit over",
        ONIGERR_RETRY_LIMIT_IN_MATCH_OVER => "retry-limit-in-match over",
        ONIGERR_RETRY_LIMIT_IN_SEARCH_OVER => "retry-limit-in-search over",
        ONIGERR_SUBEXP_CALL_LIMIT_IN_SEARCH_OVER => "subexp-call-limit-in-search over",
        ONIGERR_TYPE_BUG => "undefined type (bug)",
        ONIGERR_PARSER_BUG => "internal parser error (bug)",
        ONIGERR_STACK_BUG => "stack error (bug)",
        ONIGERR_UNDEFINED_BYTECODE => "undefined bytecode (bug)",
        ONIGERR_UNEXPECTED_BYTECODE => "unexpected bytecode (bug)",
        ONIGERR_DEFAULT_ENCODING_IS_NOT_SET => "default multibyte-encoding is not set",
        ONIGERR_SPECIFIED_ENCODING_CANT_CONVERT_TO_WIDE_CHAR => {
            "can't convert to wide-char on specified multibyte-encoding"
        }
        ONIGERR_FAIL_TO_INITIALIZE => "fail to initialize",
        ONIGERR_INVALID_ARGUMENT => "invalid argument",
        ONIGERR_END_PATTERN_AT_LEFT_BRACE => "end pattern at left brace",
        ONIGERR_END_PATTERN_AT_LEFT_BRACKET => "end pattern at left bracket",
        ONIGERR_EMPTY_CHAR_CLASS => "empty char-class",
        ONIGERR_PREMATURE_END_OF_CHAR_CLASS => "premature end of char-class",
        ONIGERR_END_PATTERN_AT_ESCAPE => "end pattern at escape",
        ONIGERR_END_PATTERN_AT_META => "end pattern at meta",
        ONIGERR_END_PATTERN_AT_CONTROL => "end pattern at control",
        ONIGERR_META_CODE_SYNTAX => "invalid meta-code syntax",
        ONIGERR_CONTROL_CODE_SYNTAX => "invalid control-code syntax",
        ONIGERR_CHAR_CLASS_VALUE_AT_END_OF_RANGE => "char-class value at end of range",
        ONIGERR_CHAR_CLASS_VALUE_AT_START_OF_RANGE => "char-class value at start of range",
        ONIGERR_UNMATCHED_RANGE_SPECIFIER_IN_CHAR_CLASS => {
            "unmatched range specifier in char-class"
        }
        ONIGERR_TARGET_OF_REPEAT_OPERATOR_NOT_SPECIFIED => {
            "target of repeat operator is not specified"
        }
        ONIGERR_TARGET_OF_REPEAT_OPERATOR_INVALID => "target of repeat operator is invalid",
        ONIGERR_NESTED_REPEAT_OPERATOR => "nested repeat operator",
        ONIGERR_UNMATCHED_CLOSE_PARENTHESIS => "unmatched close parenthesis",
        ONIGERR_END_PATTERN_WITH_UNMATCHED_PARENTHESIS => "end pattern with unmatched parenthesis",
        ONIGERR_END_PATTERN_IN_GROUP => "end pattern in group",
        ONIGERR_UNDEFINED_GROUP_OPTION => "undefined group option",
        ONIGERR_INVALID_GROUP_OPTION => "invalid group option",
        ONIGERR_INVALID_POSIX_BRACKET_TYPE => "invalid POSIX bracket type",
        ONIGERR_INVALID_LOOK_BEHIND_PATTERN => "invalid pattern in look-behind",
        ONIGERR_INVALID_REPEAT_RANGE_PATTERN => "invalid repeat range {lower,upper}",
        ONIGERR_TOO_BIG_NUMBER => "too big number",
        ONIGERR_TOO_BIG_NUMBER_FOR_REPEAT_RANGE => "too big number for repeat range",
        ONIGERR_UPPER_SMALLER_THAN_LOWER_IN_REPEAT_RANGE => {
            "upper is smaller than lower in repeat range"
        }
        ONIGERR_EMPTY_RANGE_IN_CHAR_CLASS => "empty range in char class",
        ONIGERR_MISMATCH_CODE_LENGTH_IN_CLASS_RANGE => {
            "mismatch multibyte code length in char-class range"
        }
        ONIGERR_TOO_MANY_MULTI_BYTE_RANGES => "too many multibyte code ranges are specified",
        ONIGERR_TOO_SHORT_MULTI_BYTE_STRING => "too short multibyte code string",
        ONIGERR_TOO_BIG_BACKREF_NUMBER => "too big backref number",
        ONIGERR_INVALID_BACKREF => "invalid backref number/name",
        ONIGERR_NUMBERED_BACKREF_OR_CALL_NOT_ALLOWED => {
            "numbered backref/call is not allowed. (use name)"
        }
        ONIGERR_TOO_MANY_CAPTURES => "too many captures",
        ONIGERR_TOO_BIG_WIDE_CHAR_VALUE => "too big wide-char value",
        ONIGERR_TOO_LONG_WIDE_CHAR_VALUE => "too long wide-char value",
        ONIGERR_UNDEFINED_OPERATOR => "undefined operator",
        ONIGERR_INVALID_CODE_POINT_VALUE => "invalid code point value",
        ONIGERR_EMPTY_GROUP_NAME => "group name is empty",
        ONIGERR_INVALID_GROUP_NAME => "invalid group name <%n>",
        ONIGERR_INVALID_CHAR_IN_GROUP_NAME => "invalid char in group name <%n>",
        ONIGERR_UNDEFINED_NAME_REFERENCE => "undefined name <%n> reference",
        ONIGERR_UNDEFINED_GROUP_REFERENCE => "undefined group <%n> reference",
        ONIGERR_MULTIPLEX_DEFINED_NAME => "multiplex defined name <%n>",
        ONIGERR_MULTIPLEX_DEFINITION_NAME_CALL => "multiplex definition name <%n> call",
        ONIGERR_NEVER_ENDING_RECURSION => "never ending recursion",
        ONIGERR_GROUP_NUMBER_OVER_FOR_CAPTURE_HISTORY => {
            "group number is too big for capture history"
        }
        ONIGERR_INVALID_CHAR_PROPERTY_NAME => "invalid character property name {%n}",
        ONIGERR_INVALID_IF_ELSE_SYNTAX => "invalid if-else syntax",
        ONIGERR_INVALID_ABSENT_GROUP_PATTERN => "invalid absent group pattern",
        ONIGERR_INVALID_ABSENT_GROUP_GENERATOR_PATTERN => "invalid absent group generator pattern",
        ONIGERR_INVALID_CALLOUT_PATTERN => "invalid callout pattern",
        ONIGERR_INVALID_CALLOUT_NAME => "invalid callout name",
        ONIGERR_UNDEFINED_CALLOUT_NAME => "undefined callout name",
        ONIGERR_INVALID_CALLOUT_BODY => "invalid callout body",
        ONIGERR_INVALID_CALLOUT_TAG_NAME => "invalid callout tag name",
        ONIGERR_INVALID_CALLOUT_ARG => "invalid callout arg",
        ONIGERR_NOT_SUPPORTED_ENCODING_COMBINATION => "not supported encoding combination",
        ONIGERR_INVALID_COMBINATION_OF_OPTIONS => "invalid combination of options",
        ONIGERR_VERY_INEFFICIENT_PATTERN => "very inefficient pattern",
        ONIGERR_LIBRARY_IS_NOT_INITIALIZED => "library is not initialized",
        _ => "undefined error code",
    }
}

const MAX_ERROR_PAR_LEN: usize = 30;

fn to_ascii(enc: &Encoding, s: Option<&[u8]>, buf_size: usize) -> (Vec<u8>, bool) {
    let s = match s {
        None => return (Vec::new(), false),
        Some(s) => s,
    };
    let mut buf: Vec<u8> = Vec::new();
    if enc.min_enc_len > 1 {
        let mut p = 0usize;
        let end = s.len();
        while p < end {
            let code = enc.mbc_to_code(s, p, end);
            if code >= 0x80 {
                if code > 0xffff && buf.len() + 10 <= buf_size {
                    buf.extend_from_slice(format!("\\x{:02x}", (code >> 24) & 0xff).as_bytes());
                    buf.extend_from_slice(format!("{:02x}", (code >> 16) & 0xff).as_bytes());
                    buf.extend_from_slice(format!("{:02x}", (code >> 8) & 0xff).as_bytes());
                    buf.extend_from_slice(format!("{:02x}", code & 0xff).as_bytes());
                } else if buf.len() + 6 <= buf_size {
                    buf.extend_from_slice(format!("\\x{:02x}", (code >> 8) & 0xff).as_bytes());
                    buf.extend_from_slice(format!("{:02x}", code & 0xff).as_bytes());
                } else {
                    break;
                }
            } else {
                buf.push(code as u8);
            }
            p += enc.mbc_enc_len(s, p);
            if buf.len() >= buf_size {
                break;
            }
        }
        (buf, p < end)
    } else {
        let len = s.len().min(buf_size);
        buf.extend_from_slice(&s[..len]);
        (buf, buf_size < s.len())
    }
}

/// onig_error_code_to_str
pub fn error_code_to_str(code: i32, enc: &Encoding, par: Option<&[u8]>) -> Vec<u8> {
    let fmt = error_code_to_format(code);
    match code {
        ONIGERR_UNDEFINED_NAME_REFERENCE
        | ONIGERR_UNDEFINED_GROUP_REFERENCE
        | ONIGERR_MULTIPLEX_DEFINED_NAME
        | ONIGERR_MULTIPLEX_DEFINITION_NAME_CALL
        | ONIGERR_INVALID_GROUP_NAME
        | ONIGERR_INVALID_CHAR_IN_GROUP_NAME
        | ONIGERR_INVALID_CHAR_PROPERTY_NAME => {
            let (parbuf, is_over) = to_ascii(enc, par, MAX_ERROR_PAR_LEN - 3);
            let mut out = Vec::new();
            let q = fmt.as_bytes();
            let mut i = 0;
            while i < q.len() {
                if q[i] == b'%' && q.get(i + 1) == Some(&b'n') {
                    out.extend_from_slice(&parbuf);
                    if is_over {
                        out.extend_from_slice(b"...");
                    }
                    i += 2;
                } else {
                    out.push(q[i]);
                    i += 1;
                }
            }
            out
        }
        _ => fmt.as_bytes().to_vec(),
    }
}
