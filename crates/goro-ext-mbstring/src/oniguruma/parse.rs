//! Port of Oniguruma 6.9.10 regparse.c (pattern parser).

#![allow(clippy::too_many_arguments)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::collapsible_if)]

use super::enc::*;
use super::regint::*;

/* ===== parse environment ===== */

#[derive(Clone, Copy, Debug)]
pub struct MemEnv {
    pub mem_node: NodeRef,
    pub empty_repeat_node: NodeRef,
}

impl Default for MemEnv {
    fn default() -> Self {
        MemEnv {
            mem_node: NULL_NODE,
            empty_repeat_node: NULL_NODE,
        }
    }
}

pub const PE_FLAG_HAS_CALL_ZERO: u32 = 1 << 0;
pub const PE_FLAG_HAS_WHOLE_OPTIONS: u32 = 1 << 1;
pub const PE_FLAG_HAS_ABSENT_STOPPER: u32 = 1 << 2;

/// UnsetAddr (call address fix-up after compilation)
#[derive(Clone, Copy, Debug)]
pub struct UnsetAddr {
    pub offset: usize,
    pub target: NodeRef,
}

/// ParseEnv (also carries the regex_t fields the parser touches and the
/// node arena)
pub struct ParseEnv<'a> {
    pub options: u32,
    pub case_fold_flag: u32,
    pub enc: &'static Encoding,
    pub syntax: &'static SyntaxType,
    pub cap_history: MemStatusType,
    pub backtrack_mem: MemStatusType,
    pub backrefed_mem: MemStatusType,
    pub pattern: &'a [u8],
    pub error: Option<Vec<u8>>,
    pub num_call: i32,
    pub num_mem: i32,
    pub num_named: i32,
    pub mem_env: Vec<MemEnv>,
    pub backref_num: i32,
    pub keep_num: i32,
    pub id_num: i32,
    pub save_alloc_num: i32,
    pub unset_addr_list: Vec<UnsetAddr>,
    pub parse_depth: u32,
    pub flags: u32,

    pub nodes: Nodes,
    /* regex_t parts */
    pub reg_options: u32,
    pub reg_case_fold_flag: u32,
    pub names: NameTable,
    pub callout_list: Vec<CalloutListEntry>,
    pub callout_tags: Vec<Vec<u8>>,
}

macro_rules! inc_parse_depth {
    ($env:expr, $d:expr) => {
        $d += 1;
        if $d > DEFAULT_PARSE_DEPTH_LIMIT {
            return ONIGERR_PARSE_DEPTH_LIMIT_OVER;
        }
    };
}

/* tokens */
pub const TK_EOT: i32 = 0;
pub const TK_CRUDE_BYTE: i32 = 1;
pub const TK_CHAR: i32 = 2;
pub const TK_STRING: i32 = 3;
pub const TK_CODE_POINT: i32 = 4;
pub const TK_ANYCHAR: i32 = 5;
pub const TK_CHAR_TYPE: i32 = 6;
pub const TK_BACKREF: i32 = 7;
pub const TK_CALL: i32 = 8;
pub const TK_ANCHOR: i32 = 9;
pub const TK_REPEAT: i32 = 10;
pub const TK_INTERVAL: i32 = 11;
pub const TK_ANYCHAR_ANYTIME: i32 = 12;
pub const TK_ALT: i32 = 13;
pub const TK_SUBEXP_OPEN: i32 = 14;
pub const TK_SUBEXP_CLOSE: i32 = 15;
pub const TK_OPEN_CC: i32 = 16;
pub const TK_QUOTE_OPEN: i32 = 17;
pub const TK_CHAR_PROPERTY: i32 = 18;
pub const TK_KEEP: i32 = 19;
pub const TK_GENERAL_NEWLINE: i32 = 20;
pub const TK_NO_NEWLINE: i32 = 21;
pub const TK_TRUE_ANYCHAR: i32 = 22;
pub const TK_TEXT_SEGMENT: i32 = 23;
pub const TK_CC_CLOSE: i32 = 24;
pub const TK_CC_RANGE: i32 = 25;
pub const TK_CC_POSIX_BRACKET_OPEN: i32 = 26;
pub const TK_CC_AND: i32 = 27;
pub const TK_CC_OPEN_CC: i32 = 28;

#[derive(Clone, Debug, Default)]
struct PToken {
    typ: i32,
    code_point_continue: bool,
    escaped: bool,
    base_num: i32,
    backp: usize,
    /* union u */
    byte: u8,
    code: u32,
    anchor: i32,
    rep_lower: i32,
    rep_upper: i32,
    rep_greedy: bool,
    rep_possessive: bool,
    br_num: i32,
    br_ref1: i32,
    br_refs: Vec<i32>,
    br_by_name: bool,
    br_exist_level: bool,
    br_level: i32,
    call_name: usize,
    call_name_end: usize,
    call_gnum: i32,
    call_by_number: bool,
    prop_ctype: i32,
    prop_not: bool,
}

/* cc parse states */
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CState {
    Value,
    Range,
    Complete,
    Start,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CVal {
    Undef,
    Sb,
    Mb,
    CProp,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RefNum {
    IsNotNum,
    IsAbsNum,
    IsRelNum,
}

const CPS_EMPTY: i32 = 0;
const CPS_START: i32 = 1;
const CPS_RANGE: i32 = 2;

#[inline]
fn is_code_point_divide(c: u32) -> bool {
    c == b' ' as u32 || c == b'\n' as u32
}

#[inline]
fn digitval(c: u32) -> u32 {
    c.wrapping_sub(b'0' as u32)
}

/* ===== code range buffers ===== */

pub type CodeRanges = Option<Vec<(u32, u32)>>;

/// add_code_range_to_buf
pub fn add_code_range_to_buf(pbuf: &mut CodeRanges, mut from: u32, mut to: u32) -> i32 {
    if from > to {
        std::mem::swap(&mut from, &mut to);
    }
    let data = pbuf.get_or_insert_with(Vec::new);
    let n = data.len();

    let mut low = 0usize;
    let mut bound = n;
    while low < bound {
        let x = (low + bound) >> 1;
        if from > data[x].1 {
            low = x + 1;
        } else {
            bound = x;
        }
    }

    let mut high = if to == !0u32 { n } else { low };
    bound = n;
    while high < bound {
        let x = (high + bound) >> 1;
        if to.wrapping_add(1) >= data[x].0 {
            high = x + 1;
        } else {
            bound = x;
        }
    }

    let inc_n = low as i64 + 1 - high as i64;
    if n as i64 + inc_n > ONIG_MAX_MULTI_BYTE_RANGES_NUM as i64 {
        return ONIGERR_TOO_MANY_MULTI_BYTE_RANGES;
    }

    if inc_n != 1 {
        if from > data[low].0 {
            from = data[low].0;
        }
        if to < data[high - 1].1 {
            to = data[high - 1].1;
        }
    }

    data.splice(low..high, [(from, to)]);
    0
}

fn add_code_range(pbuf: &mut CodeRanges, env: &ParseEnv, from: u32, to: u32) -> i32 {
    if from > to {
        if env.syntax.bv(ONIG_SYN_ALLOW_EMPTY_RANGE_IN_CC) {
            return 0;
        } else {
            return ONIGERR_EMPTY_RANGE_IN_CHAR_CLASS;
        }
    }
    add_code_range_to_buf(pbuf, from, to)
}

#[inline]
fn mbcode_start_pos(enc: &Encoding) -> u32 {
    if enc.min_enc_len > 1 { 0 } else { 0x80 }
}

fn set_all_multi_byte_range(enc: &Encoding, pbuf: &mut CodeRanges) -> i32 {
    add_code_range_to_buf(pbuf, mbcode_start_pos(enc), !0u32)
}

fn not_code_range_buf(enc: &Encoding, bbuf: &CodeRanges, pbuf: &mut CodeRanges) -> i32 {
    *pbuf = None;
    let data = match bbuf {
        Some(d) if !d.is_empty() => d,
        _ => return set_all_multi_byte_range(enc, pbuf),
    };
    let mut pre = mbcode_start_pos(enc);
    let mut to = 0u32;
    for &(from, t) in data.iter() {
        to = t;
        if pre <= from.wrapping_sub(1) {
            let r = add_code_range_to_buf(pbuf, pre, from.wrapping_sub(1));
            if r != 0 {
                return r;
            }
        }
        if to == !0u32 {
            break;
        }
        pre = to.wrapping_add(1);
    }
    if to < !0u32 {
        let r = add_code_range_to_buf(pbuf, to + 1, !0u32);
        if r != 0 {
            return r;
        }
    }
    0
}

fn or_code_range_buf(
    enc: &Encoding,
    bbuf1: &CodeRanges,
    not1: bool,
    bbuf2: &CodeRanges,
    not2: bool,
    pbuf: &mut CodeRanges,
) -> i32 {
    *pbuf = None;
    if bbuf1.is_none() && bbuf2.is_none() {
        if not1 || not2 {
            return set_all_multi_byte_range(enc, pbuf);
        }
        return 0;
    }

    let (mut b1, mut n1, mut b2, mut n2) = (bbuf1, not1, bbuf2, not2);
    if b2.is_none() {
        std::mem::swap(&mut b1, &mut b2);
        std::mem::swap(&mut n1, &mut n2);
    }

    if b1.is_none() {
        if n1 {
            return set_all_multi_byte_range(enc, pbuf);
        } else {
            if !n2 {
                *pbuf = b2.clone();
                return 0;
            } else {
                return not_code_range_buf(enc, b2, pbuf);
            }
        }
    }

    if n1 {
        std::mem::swap(&mut b1, &mut b2);
        std::mem::swap(&mut n1, &mut n2);
    }

    let data1 = b1.as_ref().unwrap();
    let mut r = 0;
    if !n2 && !n1 {
        *pbuf = b2.clone();
    } else if !n1 {
        r = not_code_range_buf(enc, b2, pbuf);
    }
    if r != 0 {
        return r;
    }

    for &(from, to) in data1.iter() {
        let r = add_code_range_to_buf(pbuf, from, to);
        if r != 0 {
            return r;
        }
    }
    0
}

fn and_code_range1(
    pbuf: &mut CodeRanges,
    mut from1: u32,
    mut to1: u32,
    data: &[(u32, u32)],
) -> i32 {
    for &(from2, to2) in data.iter() {
        if from2 < from1 {
            if to2 < from1 {
                continue;
            } else {
                from1 = to2.wrapping_add(1);
            }
        } else if from2 <= to1 {
            if to2 < to1 {
                if from1 <= from2.wrapping_sub(1) {
                    let r = add_code_range_to_buf(pbuf, from1, from2.wrapping_sub(1));
                    if r != 0 {
                        return r;
                    }
                }
                from1 = to2.wrapping_add(1);
            } else {
                to1 = from2.wrapping_sub(1);
            }
        } else {
            from1 = from2;
        }
        if from1 > to1 {
            break;
        }
    }
    if from1 <= to1 {
        let r = add_code_range_to_buf(pbuf, from1, to1);
        if r != 0 {
            return r;
        }
    }
    0
}

fn and_code_range_buf(
    bbuf1: &CodeRanges,
    not1: bool,
    bbuf2: &CodeRanges,
    not2: bool,
    pbuf: &mut CodeRanges,
) -> i32 {
    *pbuf = None;
    if bbuf1.is_none() {
        if not1 && bbuf2.is_some() {
            *pbuf = bbuf2.clone();
        }
        return 0;
    } else if bbuf2.is_none() {
        if not2 {
            *pbuf = bbuf1.clone();
        }
        return 0;
    }

    let (mut b1, mut n1, mut b2, mut n2) = (bbuf1, not1, bbuf2, not2);
    if n1 {
        std::mem::swap(&mut b1, &mut b2);
        std::mem::swap(&mut n1, &mut n2);
    }
    let data1 = b1.as_ref().unwrap();
    let data2 = b2.as_ref().unwrap();

    if !n2 && !n1 {
        for &(from1, to1) in data1.iter() {
            for &(from2, to2) in data2.iter() {
                if from2 > to1 {
                    break;
                }
                if to2 < from1 {
                    continue;
                }
                let from = from1.max(from2);
                let to = to1.min(to2);
                let r = add_code_range_to_buf(pbuf, from, to);
                if r != 0 {
                    return r;
                }
            }
        }
    } else if !n1 {
        for &(from1, to1) in data1.iter() {
            let r = and_code_range1(pbuf, from1, to1, data2);
            if r != 0 {
                return r;
            }
        }
    }
    0
}

fn bitset_invert(bs: &mut BitSet) {
    for x in bs.iter_mut() {
        *x = !*x;
    }
}

fn bitset_set_range(bs: &mut BitSet, from: u32, to: u32) {
    let mut i = from;
    while i <= to && i < SINGLE_BYTE_SIZE {
        bitset_set_bit(bs, i);
        i += 1;
    }
}

pub fn and_cclass(dest: &mut CClass, cc: &CClass, enc: &Encoding) -> i32 {
    let not1 = dest.not;
    let not2 = cc.not;
    let mut bsr1 = dest.bs;
    let mut bsr2 = cc.bs;
    if not1 {
        bitset_invert(&mut bsr1);
    }
    if not2 {
        bitset_invert(&mut bsr2);
    }
    for i in 0..8 {
        bsr1[i] &= bsr2[i];
    }
    dest.bs = bsr1;
    if not1 {
        bitset_invert(&mut dest.bs);
    }

    if !enc.is_singlebyte() {
        let mut pbuf: CodeRanges = None;
        let r;
        if not1 && not2 {
            r = or_code_range_buf(enc, &dest.mbuf, false, &cc.mbuf, false, &mut pbuf);
        } else {
            let mut r2 = and_code_range_buf(&dest.mbuf, not1, &cc.mbuf, not2, &mut pbuf);
            if r2 == 0 && not1 {
                let mut tbuf: CodeRanges = None;
                r2 = not_code_range_buf(enc, &pbuf, &mut tbuf);
                if r2 != 0 {
                    return r2;
                }
                pbuf = tbuf;
            }
            r = r2;
        }
        if r != 0 {
            return r;
        }
        dest.mbuf = pbuf;
        return r;
    }
    0
}

pub fn or_cclass(dest: &mut CClass, cc: &CClass, enc: &Encoding) -> i32 {
    let not1 = dest.not;
    let not2 = cc.not;
    let mut bsr1 = dest.bs;
    let mut bsr2 = cc.bs;
    if not1 {
        bitset_invert(&mut bsr1);
    }
    if not2 {
        bitset_invert(&mut bsr2);
    }
    for i in 0..8 {
        bsr1[i] |= bsr2[i];
    }
    dest.bs = bsr1;
    if not1 {
        bitset_invert(&mut dest.bs);
    }

    if !enc.is_singlebyte() {
        let mut pbuf: CodeRanges = None;
        let r;
        if not1 && not2 {
            r = and_code_range_buf(&dest.mbuf, false, &cc.mbuf, false, &mut pbuf);
        } else {
            let mut r2 = or_code_range_buf(enc, &dest.mbuf, not1, &cc.mbuf, not2, &mut pbuf);
            if r2 == 0 && not1 {
                let mut tbuf: CodeRanges = None;
                r2 = not_code_range_buf(enc, &pbuf, &mut tbuf);
                if r2 != 0 {
                    return r2;
                }
                pbuf = tbuf;
            }
            r = r2;
        }
        if r != 0 {
            return r;
        }
        dest.mbuf = pbuf;
        return r;
    }
    0
}

/// onig_is_in_code_range
pub fn is_in_code_range(data: &[(u32, u32)], code: u32) -> bool {
    let n = data.len();
    let mut low = 0usize;
    let mut high = n;
    while low < high {
        let x = (low + high) >> 1;
        if code > data[x].1 {
            low = x + 1;
        } else {
            high = x;
        }
    }
    low < n && code >= data[low].0
}

/// onig_is_code_in_cc_len
pub fn is_code_in_cc_len(elen: i32, code: u32, cc: &CClass) -> bool {
    let found = if elen > 1 || code >= SINGLE_BYTE_SIZE {
        match &cc.mbuf {
            None => false,
            Some(d) => is_in_code_range(d, code),
        }
    } else {
        bitset_at(&cc.bs, code)
    };
    if cc.not { !found } else { found }
}

/// onig_is_code_in_cc
pub fn is_code_in_cc(enc: &Encoding, code: u32, cc: &CClass) -> bool {
    let len = if enc.min_enc_len > 1 {
        2
    } else {
        let l = enc.code_to_mbclen(code);
        if l < 0 {
            return false;
        }
        l
    };
    is_code_in_cc_len(len, code, cc)
}

/// ADD_CODE_INTO_CC
fn add_code_into_cc(cc: &mut CClass, code: u32, enc: &Encoding) {
    if enc.min_enc_len > 1 || enc.code_to_mbclen(code) != 1 {
        add_code_range_to_buf(&mut cc.mbuf, code, code);
    } else {
        bitset_set_bit(&mut cc.bs, code);
    }
}

/* ===== node constructors ===== */

impl<'a> ParseEnv<'a> {
    pub fn new(
        pattern: &'a [u8],
        options: u32,
        case_fold_flag: u32,
        enc: &'static Encoding,
        syntax: &'static SyntaxType,
    ) -> ParseEnv<'a> {
        ParseEnv {
            options,
            case_fold_flag,
            enc,
            syntax,
            cap_history: 0,
            backtrack_mem: 0,
            backrefed_mem: 0,
            pattern,
            error: None,
            num_call: 0,
            num_mem: 0,
            num_named: 0,
            mem_env: vec![MemEnv::default()],
            backref_num: 0,
            keep_num: 0,
            id_num: 0,
            save_alloc_num: 0,
            unset_addr_list: Vec::new(),
            parse_depth: 0,
            flags: 0,
            nodes: Nodes::default(),
            reg_options: options,
            reg_case_fold_flag: case_fold_flag,
            names: NameTable::default(),
            callout_list: Vec::new(),
            callout_tags: Vec::new(),
        }
    }

    #[inline]
    fn end(&self) -> usize {
        self.pattern.len()
    }
    #[inline]
    fn enclen(&self, p: usize) -> usize {
        self.enc.mbc_enc_len(self.pattern, p)
    }
    /// PPEEK
    #[inline]
    fn ppeek(&self, p: usize, end: usize) -> u32 {
        if p < end {
            self.enc.mbc_to_code(self.pattern, p, end)
        } else {
            0
        }
    }
    /// PFETCH: returns the code, sets prev, advances p
    #[inline]
    fn pfetch(&self, p: &mut usize, prev: &mut usize, end: usize) -> u32 {
        let c = self.enc.mbc_to_code(self.pattern, *p, end);
        *prev = *p;
        *p += self.enc.mbc_enc_len(self.pattern, *p);
        c
    }
    /// PFETCH_S
    #[inline]
    fn pfetch_s(&self, p: &mut usize, end: usize) -> u32 {
        let c = self.enc.mbc_to_code(self.pattern, *p, end);
        *p += self.enc.mbc_enc_len(self.pattern, *p);
        c
    }
    #[inline]
    fn pinc(&self, p: &mut usize, prev: &mut usize) {
        *prev = *p;
        *p += self.enc.mbc_enc_len(self.pattern, *p);
    }
    #[inline]
    fn pinc_s(&self, p: &mut usize) {
        *p += self.enc.mbc_enc_len(self.pattern, *p);
    }

    pub fn set_error_string(&mut self, s: usize, e: usize) {
        let e = e.min(self.pattern.len());
        let s = s.min(e);
        self.error = Some(self.pattern[s..e].to_vec());
    }

    pub fn set_error_bytes(&mut self, b: &[u8]) {
        self.error = Some(b.to_vec());
    }

    pub fn mem_node(&self, num: i32) -> NodeRef {
        self.mem_env
            .get(num as usize)
            .map(|m| m.mem_node)
            .unwrap_or(NULL_NODE)
    }

    fn scan_env_add_mem_entry(&mut self) -> i32 {
        let need = self.num_mem + 1;
        if need > DEFAULT_MAX_CAPTURE_NUM {
            return ONIGERR_TOO_MANY_CAPTURES;
        }
        self.num_mem += 1;
        while self.mem_env.len() <= self.num_mem as usize {
            self.mem_env.push(MemEnv::default());
        }
        self.num_mem
    }

    fn scan_env_set_mem_node(&mut self, num: i32, node: NodeRef) -> i32 {
        if self.num_mem >= num {
            while self.mem_env.len() <= num as usize {
                self.mem_env.push(MemEnv::default());
            }
            self.mem_env[num as usize].mem_node = node;
            0
        } else {
            ONIGERR_PARSER_BUG
        }
    }

    fn backref_rel_to_abs(&self, rel_no: i32) -> i32 {
        if rel_no > 0 {
            if rel_no > ONIG_INT_MAX - self.num_mem {
                return ONIGERR_INVALID_BACKREF;
            }
            self.num_mem + rel_no
        } else {
            self.num_mem + 1 + rel_no
        }
    }

    pub fn node_new_cclass(&mut self) -> NodeRef {
        self.nodes.alloc(NodeKind::CClass(Box::default()))
    }

    fn node_new_ctype(&mut self, typ: i32, not: bool, options: u32) -> NodeRef {
        let ascii_mode = is_ascii_mode_ctype(typ, options);
        self.nodes.alloc(NodeKind::CType {
            ctype: typ,
            not,
            ascii_mode,
        })
    }

    fn node_new_anychar(&mut self, options: u32) -> NodeRef {
        let n = self.node_new_ctype(CTYPE_ANYCHAR, false, options);
        if opton_multiline(options) {
            self.nodes.add(n, ND_ST_MULTILINE);
        }
        n
    }

    fn node_new_no_newline(&mut self) -> NodeRef {
        self.node_new_anychar(ONIG_OPTION_NONE)
    }

    fn node_new_true_anychar(&mut self) -> NodeRef {
        self.node_new_anychar(ONIG_OPTION_MULTILINE)
    }

    pub fn node_new_list(&mut self, left: NodeRef, right: NodeRef) -> NodeRef {
        self.nodes.alloc(NodeKind::List {
            car: left,
            cdr: right,
        })
    }

    pub fn node_new_alt(&mut self, left: NodeRef, right: NodeRef) -> NodeRef {
        self.nodes.alloc(NodeKind::Alt {
            car: left,
            cdr: right,
        })
    }

    fn make_list_or_alt(&mut self, alt: bool, ns: &[NodeRef]) -> NodeRef {
        if ns.is_empty() {
            return NULL_NODE;
        }
        let right = if ns.len() == 1 {
            NULL_NODE
        } else {
            self.make_list_or_alt(alt, &ns[1..])
        };
        if alt {
            self.node_new_alt(ns[0], right)
        } else {
            self.node_new_list(ns[0], right)
        }
    }

    pub fn make_list(&mut self, ns: &[NodeRef]) -> NodeRef {
        self.make_list_or_alt(false, ns)
    }

    pub fn make_alt(&mut self, ns: &[NodeRef]) -> NodeRef {
        self.make_list_or_alt(true, ns)
    }

    pub fn node_new_anchor(&mut self, typ: i32) -> NodeRef {
        self.nodes.alloc(NodeKind::Anchor {
            body: NULL_NODE,
            typ,
            char_min_len: 0,
            char_max_len: INFINITE_LEN,
            ascii_mode: false,
            lead_node: NULL_NODE,
        })
    }

    fn node_new_anchor_with_options(&mut self, typ: i32, options: u32) -> NodeRef {
        let n = self.node_new_anchor(typ);
        let ascii_mode = opton_word_ascii(options) && is_word_anchor_type(typ);
        if let NodeKind::Anchor { ascii_mode: am, .. } = &mut self.nodes.v[n].kind {
            *am = ascii_mode;
        }
        if (typ == ANCR_TEXT_SEGMENT_BOUNDARY || typ == ANCR_NO_TEXT_SEGMENT_BOUNDARY)
            && opton_text_segment_word(options)
        {
            self.nodes.add(n, ND_ST_TEXT_SEGMENT_WORD);
        }
        n
    }

    fn node_new_backref(
        &mut self,
        backrefs: &[i32],
        by_name: bool,
        exist_level: bool,
        nest_level: i32,
    ) -> NodeRef {
        let n = self.nodes.alloc(NodeKind::BackRef {
            back: backrefs.to_vec(),
            nest_level: 0,
        });
        if by_name {
            self.nodes.add(n, ND_ST_BY_NAME);
        }
        if opton_ignorecase(self.options) {
            self.nodes.add(n, ND_ST_IGNORECASE);
        }
        if exist_level {
            self.nodes.add(n, ND_ST_NEST_LEVEL);
            if let NodeKind::BackRef { nest_level: nl, .. } = &mut self.nodes.v[n].kind {
                *nl = nest_level;
            }
        }
        for &b in backrefs {
            if b <= self.num_mem && self.mem_node(b) == NULL_NODE {
                self.nodes.add(n, ND_ST_RECURSION);
                break;
            }
        }
        self.backref_num += 1;
        n
    }

    fn node_new_backref_checker(
        &mut self,
        backrefs: &[i32],
        by_name: bool,
        exist_level: bool,
        nest_level: i32,
    ) -> NodeRef {
        let n = self.node_new_backref(backrefs, by_name, exist_level, nest_level);
        self.nodes.add(n, ND_ST_CHECKER);
        n
    }

    fn node_new_call(&mut self, name: Vec<u8>, gnum: i32, by_number: bool) -> NodeRef {
        self.nodes.alloc(NodeKind::Call {
            body: NULL_NODE,
            by_number,
            called_gnum: gnum,
            name,
            entry_count: 1,
        })
    }

    pub fn node_new_quantifier(&mut self, lower: i32, upper: i32, by_number: bool) -> NodeRef {
        let n = self.nodes.alloc(NodeKind::Quant {
            body: NULL_NODE,
            lower,
            upper,
            greedy: true,
            emptiness: BodyEmpty::IsNotEmpty,
            head_exact: NULL_NODE,
            next_head_exact: NULL_NODE,
            include_referred: false,
            empty_status_mem: 0,
        });
        if by_number {
            self.nodes.add(n, ND_ST_BY_NUMBER);
        }
        n
    }

    pub fn node_new_bag(&mut self, typ: BagType) -> NodeRef {
        self.nodes.alloc(NodeKind::Bag {
            body: NULL_NODE,
            typ,
            regnum: 0,
            called_addr: -1,
            entry_count: 1,
            called_state: 0,
            options: 0,
            then_: NULL_NODE,
            else_: NULL_NODE,
            min_len: 0,
            max_len: 0,
            min_char_len: 0,
            max_char_len: 0,
            opt_count: 0,
        })
    }

    fn node_new_bag_if_else(&mut self, cond: NodeRef, then: NodeRef, els: NodeRef) -> NodeRef {
        let n = self.node_new_bag(BagType::IfElse);
        if let NodeKind::Bag {
            body, then_, else_, ..
        } = &mut self.nodes.v[n].kind
        {
            *body = cond;
            *then_ = then;
            *else_ = els;
        }
        n
    }

    pub fn node_new_memory(&mut self, is_named: bool) -> NodeRef {
        let n = self.node_new_bag(BagType::Memory);
        if is_named {
            self.nodes.add(n, ND_ST_NAMED_GROUP);
        }
        n
    }

    fn node_new_option(&mut self, option: u32) -> NodeRef {
        let n = self.node_new_bag(BagType::Option);
        if let NodeKind::Bag { options, .. } = &mut self.nodes.v[n].kind {
            *options = option;
        }
        n
    }

    fn set_bag_regnum(&mut self, n: NodeRef, num: i32) {
        if let NodeKind::Bag { regnum, .. } = &mut self.nodes.v[n].kind {
            *regnum = num;
        }
    }

    fn node_new_group(&mut self, content: NodeRef) -> NodeRef {
        self.node_new_list(content, NULL_NODE)
    }

    fn node_drop_group(&mut self, group: NodeRef) -> NodeRef {
        self.nodes.car(group)
    }

    pub fn node_new_fail(&mut self) -> NodeRef {
        self.nodes.alloc(NodeKind::Gimmick {
            typ: GimmickType::Fail,
            detail_type: 0,
            num: 0,
            id: 0,
        })
    }

    fn node_new_save_gimmick(&mut self, save_type: i32) -> NodeRef {
        let id = self.id_num;
        self.id_num += 1;
        self.nodes.alloc(NodeKind::Gimmick {
            typ: GimmickType::Save,
            detail_type: save_type,
            num: 0,
            id,
        })
    }

    fn node_new_update_var_gimmick(&mut self, update_var_type: i32, id: i32) -> NodeRef {
        self.nodes.alloc(NodeKind::Gimmick {
            typ: GimmickType::UpdateVar,
            detail_type: update_var_type,
            num: 0,
            id,
        })
    }

    fn node_new_keep(&mut self) -> NodeRef {
        let n = self.node_new_save_gimmick(SAVE_KEEP);
        self.keep_num += 1;
        n
    }

    fn node_new_callout(&mut self, of_contents: bool, num: i32, id: i32) -> NodeRef {
        self.nodes.alloc(NodeKind::Gimmick {
            typ: GimmickType::Callout,
            detail_type: if of_contents {
                ONIG_CALLOUT_OF_CONTENTS
            } else {
                ONIG_CALLOUT_OF_NAME
            },
            num,
            id,
        })
    }

    fn gimmick_id(&self, n: NodeRef) -> i32 {
        match &self.nodes.v[n].kind {
            NodeKind::Gimmick { id, .. } => *id,
            _ => 0,
        }
    }

    pub fn node_new_str(&mut self, s: &[u8]) -> NodeRef {
        self.nodes.alloc(NodeKind::Str {
            s: s.to_vec(),
            flag: 0,
        })
    }

    fn node_new_str_with_options(&mut self, s: &[u8], options: u32) -> NodeRef {
        let n = self.node_new_str(s);
        if opton_ignorecase(options) {
            self.nodes.add(n, ND_ST_IGNORECASE);
        }
        n
    }

    fn node_new_str_crude(&mut self, s: &[u8], options: u32) -> NodeRef {
        let n = self.node_new_str_with_options(s, options);
        if let NodeKind::Str { flag, .. } = &mut self.nodes.v[n].kind {
            *flag |= ND_STRING_CRUDE;
        }
        n
    }

    pub fn node_new_empty(&mut self) -> NodeRef {
        self.node_new_str(&[])
    }

    fn node_new_str_crude_char(&mut self, c: u8, options: u32) -> NodeRef {
        self.node_new_str_crude(&[c], options)
    }

    fn pattern_slice(&self, s: usize, e: usize) -> Vec<u8> {
        self.pattern[s..e].to_vec()
    }
}

pub const SAVE_KEEP: i32 = 0;
pub const SAVE_S: i32 = 1;
pub const SAVE_RIGHT_RANGE: i32 = 2;

pub const UPDATE_VAR_KEEP_FROM_STACK_LAST: i32 = 0;
pub const UPDATE_VAR_S_FROM_STACK: i32 = 1;
pub const UPDATE_VAR_RIGHT_RANGE_FROM_STACK: i32 = 2;
pub const UPDATE_VAR_RIGHT_RANGE_FROM_S_STACK: i32 = 3;
pub const UPDATE_VAR_RIGHT_RANGE_TO_S: i32 = 4;
pub const UPDATE_VAR_RIGHT_RANGE_INIT: i32 = 5;

pub const ONIG_CALLOUT_OF_CONTENTS: i32 = 0;
pub const ONIG_CALLOUT_OF_NAME: i32 = 1;

/// OPTON_IS_ASCII_MODE_CTYPE
pub fn is_ascii_mode_ctype(ctype: i32, options: u32) -> bool {
    ctype >= 0
        && ((ctype < ONIGENC_CTYPE_ASCII as i32 && opton_posix_ascii(options))
            || (ctype == ONIGENC_CTYPE_WORD as i32 && opton_word_ascii(options))
            || (ctype == ONIGENC_CTYPE_DIGIT as i32 && opton_digit_ascii(options))
            || (ctype == ONIGENC_CTYPE_SPACE as i32 && opton_space_ascii(options)))
}

/* ===== composite constructs ===== */

impl<'a> ParseEnv<'a> {
    /// \X == (?>\O(?:\Y\O)*)
    fn make_text_segment(&mut self) -> NodeRef {
        let a = self.node_new_anchor_with_options(ANCR_NO_TEXT_SEGMENT_BOUNDARY, self.options);
        let b = self.node_new_true_anychar();
        let x = self.make_list(&[a, b]);
        let q = self.node_new_quantifier(0, INFINITE_REPEAT, true);
        self.nodes.set_body(q, x);
        let c = self.node_new_true_anychar();
        let x = self.make_list(&[c, q]);
        let bag = self.node_new_bag(BagType::StopBacktrack);
        self.nodes.set_body(bag, x);
        bag
    }

    fn make_absent_engine(
        &mut self,
        pre_save_right_id: i32,
        absent: NodeRef,
        step_one: NodeRef,
        lower: i32,
        upper: i32,
        possessive: bool,
        is_range_cutter: bool,
    ) -> NodeRef {
        let ns0 = self.node_new_save_gimmick(SAVE_S);
        let id = self.gimmick_id(ns0);
        let ns2 = self.node_new_update_var_gimmick(UPDATE_VAR_RIGHT_RANGE_FROM_S_STACK, id);
        if is_range_cutter {
            self.nodes.add(ns2, ND_ST_ABSENT_WITH_SIDE_EFFECTS);
        }
        let ns3 = self.node_new_fail();
        let x = self.make_list(&[ns0, absent, ns2, ns3]);
        let x = self.make_alt(&[x, step_one]);
        let q = self.node_new_quantifier(lower, upper, false);
        self.nodes.set_body(q, x);
        let mut n0 = q;
        if possessive {
            let b = self.node_new_bag(BagType::StopBacktrack);
            self.nodes.set_body(b, n0);
            n0 = b;
        }
        let u =
            self.node_new_update_var_gimmick(UPDATE_VAR_RIGHT_RANGE_FROM_STACK, pre_save_right_id);
        let f = self.node_new_fail();
        let x = self.make_list(&[u, f]);
        let x = self.make_alt(&[n0, x]);
        if is_range_cutter {
            self.nodes.add(x, ND_ST_SUPER);
        }
        x
    }

    fn make_absent_tail(&mut self, pre_save_right_id: i32) -> (NodeRef, NodeRef) {
        let save = self.node_new_save_gimmick(SAVE_RIGHT_RANGE);
        let id = self.gimmick_id(save);
        let u = self.node_new_update_var_gimmick(UPDATE_VAR_RIGHT_RANGE_FROM_STACK, id);
        let f = self.node_new_fail();
        let x = self.make_list(&[u, f]);
        let u2 =
            self.node_new_update_var_gimmick(UPDATE_VAR_RIGHT_RANGE_FROM_STACK, pre_save_right_id);
        let x = self.make_alt(&[u2, x]);
        (save, x)
    }

    fn make_range_clear(&mut self) -> NodeRef {
        let save = self.node_new_save_gimmick(SAVE_RIGHT_RANGE);
        let id = self.gimmick_id(save);
        let u = self.node_new_update_var_gimmick(UPDATE_VAR_RIGHT_RANGE_FROM_STACK, id);
        let f = self.node_new_fail();
        let x = self.make_list(&[u, f]);
        let u0 = self.node_new_update_var_gimmick(UPDATE_VAR_RIGHT_RANGE_INIT, 0);
        self.nodes.add(u0, ND_ST_ABSENT_WITH_SIDE_EFFECTS);
        let x = self.make_alt(&[u0, x]);
        self.nodes.add(x, ND_ST_SUPER);
        self.make_list(&[save, x])
    }

    /// returns Some((quant, body, is_possessive))
    fn is_simple_one_char_repeat(&mut self, node: NodeRef) -> Option<(NodeRef, NodeRef, bool)> {
        let mut is_possessive = false;
        let quant;
        match self.nodes.typ(node) {
            NodeType::Quant => quant = node,
            NodeType::Bag => {
                if let NodeKind::Bag {
                    typ: BagType::StopBacktrack,
                    body,
                    ..
                } = self.nodes.v[node].kind
                {
                    is_possessive = true;
                    quant = body;
                    if self.nodes.typ(quant) != NodeType::Quant {
                        return None;
                    }
                } else {
                    return None;
                }
            }
            _ => return None,
        }
        let (greedy, body) = match &self.nodes.v[quant].kind {
            NodeKind::Quant { greedy, body, .. } => (*greedy, *body),
            _ => unreachable!(),
        };
        if !greedy {
            return None;
        }
        match self.nodes.typ(body) {
            NodeType::String => {
                let s = self.nodes.str_bytes(body).to_vec();
                let mut len = 0;
                let mut i = 0;
                while i < s.len() {
                    i += self.enc.mbc_enc_len(&s, i);
                    len += 1;
                }
                if len != 1 {
                    return None;
                }
            }
            NodeType::CClass => {}
            _ => return None,
        }
        self.nodes.set_body(quant, NULL_NODE);
        Some((quant, body, is_possessive))
    }

    fn make_absent_tree_for_simple_one_char_repeat(
        &mut self,
        absent: NodeRef,
        quant: NodeRef,
        body: NodeRef,
        possessive: bool,
    ) -> NodeRef {
        let (lower, upper) = match &self.nodes.v[quant].kind {
            NodeKind::Quant { lower, upper, .. } => (*lower, *upper),
            _ => unreachable!(),
        };
        let ns0 = self.node_new_save_gimmick(SAVE_RIGHT_RANGE);
        let id1 = self.gimmick_id(ns0);
        let ns1 = self.make_absent_engine(id1, absent, body, lower, upper, possessive, false);
        let ns2 = self.node_new_update_var_gimmick(UPDATE_VAR_RIGHT_RANGE_FROM_STACK, id1);
        self.make_list(&[ns0, ns1, ns2])
    }

    fn make_absent_tree(
        &mut self,
        absent: NodeRef,
        expr: NodeRef,
        is_range_cutter: bool,
    ) -> NodeRef {
        if !is_range_cutter {
            if expr == NULL_NODE {
                /* default expr \O* */
                let quant = self.node_new_quantifier(0, INFINITE_REPEAT, false);
                let body = self.node_new_true_anychar();
                return self
                    .make_absent_tree_for_simple_one_char_repeat(absent, quant, body, false);
            } else if let Some((quant, body, possessive)) = self.is_simple_one_char_repeat(expr) {
                return self
                    .make_absent_tree_for_simple_one_char_repeat(absent, quant, body, possessive);
            }
        }

        let ns0 = self.node_new_save_gimmick(SAVE_RIGHT_RANGE);
        let id1 = self.gimmick_id(ns0);
        let ns1 = self.node_new_save_gimmick(SAVE_S);
        let id2 = self.gimmick_id(ns1);
        let any = self.node_new_true_anychar();
        let ns2 =
            self.make_absent_engine(id1, absent, any, 0, INFINITE_REPEAT, true, is_range_cutter);
        let ns3 = self.node_new_update_var_gimmick(UPDATE_VAR_S_FROM_STACK, id2);
        if is_range_cutter {
            self.make_list(&[ns0, ns1, ns2, ns3])
        } else {
            let (ns5, ns6) = self.make_absent_tail(id1);
            self.make_list(&[ns0, ns1, ns2, ns3, expr, ns5, ns6])
        }
    }

    /* ===== string nodes ===== */

    fn str_node_split_last_char(&mut self, node: NodeRef) -> NodeRef {
        let s = self.nodes.str_bytes(node).to_vec();
        if s.is_empty() {
            return NULL_NODE;
        }
        let p = get_prev_char_head(self.enc, &s, 0, s.len());
        if let Some(p) = p {
            if p > 0 {
                let rn = self.node_new_str(&s[p..]);
                let flag = self.nodes.str_flag(node);
                let status = self.nodes.status(node);
                self.nodes.str_mut(node).truncate(p);
                if let NodeKind::Str { flag: f, .. } = &mut self.nodes.v[rn].kind {
                    *f = flag;
                }
                self.nodes.v[rn].status = status;
                return rn;
            }
        }
        NULL_NODE
    }

    fn str_node_can_be_split(&self, node: NodeRef) -> bool {
        let s = self.nodes.str_bytes(node);
        if !s.is_empty() {
            return self.enc.mbc_enc_len(s, 0) < s.len();
        }
        false
    }

    /* ===== number scanners ===== */

    fn scan_number(&self, src: &mut usize, end: usize) -> i32 {
        let mut p = *src;
        let mut prev = p;
        let mut num: i32 = 0;
        while p < end {
            let c = self.pfetch(&mut p, &mut prev, end);
            if c < 128 && self.enc.is_code_digit(c) {
                let val = digitval(c) as i32;
                if (ONIG_INT_MAX - val) / 10 < num {
                    return -1;
                }
                num = num * 10 + val;
            } else {
                p = prev;
                break;
            }
        }
        *src = p;
        num
    }

    fn is_code_xdigit_ascii(&self, c: u32) -> bool {
        c < 128 && self.enc.is_code_ctype(c, ONIGENC_CTYPE_XDIGIT)
    }

    fn is_code_digit_ascii(&self, c: u32) -> bool {
        c < 128 && self.enc.is_code_digit(c)
    }

    fn xdigitval(&self, c: u32) -> u32 {
        if self.is_code_digit_ascii(c) {
            digitval(c)
        } else if self.enc.is_code_ctype(c, ONIGENC_CTYPE_UPPER) {
            c.wrapping_sub(b'A' as u32).wrapping_add(10)
        } else {
            c.wrapping_sub(b'a' as u32).wrapping_add(10)
        }
    }

    fn scan_hexadecimal_number(
        &self,
        src: &mut usize,
        end: usize,
        minlen: i32,
        maxlen: i32,
        rcode: &mut u32,
    ) -> i32 {
        let mut p = *src;
        let mut prev = p;
        let mut code: u32 = 0;
        let mut n = 0;
        while p < end && n < maxlen {
            let c = self.pfetch(&mut p, &mut prev, end);
            if self.is_code_xdigit_ascii(c) {
                n += 1;
                let val = self.xdigitval(c);
                if (u32::MAX - val) / 16 < code {
                    return ONIGERR_TOO_BIG_NUMBER;
                }
                code = (code << 4).wrapping_add(val);
            } else {
                p = prev;
                break;
            }
        }
        if n < minlen {
            return ONIGERR_INVALID_CODE_POINT_VALUE;
        }
        *rcode = code;
        *src = p;
        ONIG_NORMAL
    }

    fn scan_octal_number(
        &self,
        src: &mut usize,
        end: usize,
        minlen: i32,
        maxlen: i32,
        rcode: &mut u32,
    ) -> i32 {
        let mut p = *src;
        let mut prev = p;
        let mut code: u32 = 0;
        let mut n = 0;
        while p < end && n < maxlen {
            let c = self.pfetch(&mut p, &mut prev, end);
            if self.is_code_digit_ascii(c) && c < b'8' as u32 {
                n += 1;
                let val = digitval(c);
                if (u32::MAX - val) / 8 < code {
                    return ONIGERR_TOO_BIG_NUMBER;
                }
                code = (code << 3).wrapping_add(val);
            } else {
                p = prev;
                break;
            }
        }
        if n < minlen {
            return ONIGERR_INVALID_CODE_POINT_VALUE;
        }
        *rcode = code;
        *src = p;
        ONIG_NORMAL
    }

    fn scan_number_of_base(
        &self,
        src: &mut usize,
        end: usize,
        minlen: i32,
        rcode: &mut u32,
        base: i32,
    ) -> i32 {
        if base == 16 {
            self.scan_hexadecimal_number(src, end, minlen, 8, rcode)
        } else if base == 8 {
            self.scan_octal_number(src, end, minlen, 11, rcode)
        } else {
            ONIGERR_INVALID_CODE_POINT_VALUE
        }
    }

    /// check_code_point_sequence_cc (the C uses gotos into the loop; this
    /// is the equivalent state machine)
    fn check_code_point_sequence_cc(
        &self,
        mut p: usize,
        end: usize,
        base: i32,
        mut state: i32,
    ) -> i32 {
        let mut prev = p;
        let mut end_digit = false;
        let mut n = 0;
        let mut code = 0u32;
        let mut fetch = true;
        let mut c = 0u32;
        loop {
            if fetch {
                if p >= end {
                    break;
                }
                c = self.pfetch(&mut p, &mut prev, end);
            }
            fetch = true;
            if c == b'}' as u32 {
                if state == CPS_RANGE {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                }
                return n;
            }
            if is_code_point_divide(c) {
                while p < end {
                    c = self.pfetch(&mut p, &mut prev, end);
                    if !is_code_point_divide(c) {
                        break;
                    }
                }
                if is_code_point_divide(c) {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                }
            } else if c == b'-' as u32 {
                if state != CPS_START {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                }
                if p >= end {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                }
                end_digit = false;
                state = CPS_RANGE;
                continue; /* goto start */
            } else if end_digit {
                if base == 16 {
                    if self.is_code_xdigit_ascii(c) {
                        return ONIGERR_TOO_LONG_WIDE_CHAR_VALUE;
                    }
                } else if base == 8 && self.is_code_digit_ascii(c) && c < b'8' as u32 {
                    return ONIGERR_TOO_LONG_WIDE_CHAR_VALUE;
                }
                return ONIGERR_INVALID_CODE_POINT_VALUE;
            }

            if c == b'}' as u32 {
                if state == CPS_RANGE {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                }
                return n;
            }
            if c == b'-' as u32 {
                /* goto range */
                if state != CPS_START {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                }
                if p >= end {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                }
                end_digit = false;
                state = CPS_RANGE;
                continue;
            }

            p = prev;
            let r = self.scan_number_of_base(&mut p, end, 1, &mut code, base);
            if r != 0 {
                return r;
            }
            n += 1;
            end_digit = true;
            state = if state == CPS_RANGE {
                CPS_EMPTY
            } else {
                CPS_START
            };
            let _ = fetch;
        }
        ONIGERR_INVALID_CODE_POINT_VALUE
    }

    fn check_code_point_sequence(&self, mut p: usize, end: usize, base: i32) -> i32 {
        let mut prev = p;
        let mut end_digit = false;
        let mut n = 0;
        let mut code = 0u32;
        while p < end {
            let mut c = self.pfetch(&mut p, &mut prev, end);
            if c == b'}' as u32 {
                return n;
            }
            if is_code_point_divide(c) {
                while p < end {
                    c = self.pfetch(&mut p, &mut prev, end);
                    if !is_code_point_divide(c) {
                        break;
                    }
                }
                if is_code_point_divide(c) {
                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                }
            } else if end_digit {
                if base == 16 {
                    if self.is_code_xdigit_ascii(c) {
                        return ONIGERR_TOO_LONG_WIDE_CHAR_VALUE;
                    }
                } else if base == 8 && self.is_code_digit_ascii(c) && c < b'8' as u32 {
                    return ONIGERR_TOO_LONG_WIDE_CHAR_VALUE;
                }
                return ONIGERR_INVALID_CODE_POINT_VALUE;
            }
            if c == b'}' as u32 {
                return n;
            }
            p = prev;
            let r = self.scan_number_of_base(&mut p, end, 1, &mut code, base);
            if r != 0 {
                return r;
            }
            n += 1;
            end_digit = true;
        }
        ONIGERR_INVALID_CODE_POINT_VALUE
    }

    fn get_next_code_point(
        &self,
        src: &mut usize,
        end: usize,
        base: i32,
        in_cc: bool,
        rcode: &mut u32,
    ) -> i32 {
        let mut p = *src;
        let mut prev = p;
        while p < end {
            let c = self.pfetch(&mut p, &mut prev, end);
            if !is_code_point_divide(c) {
                if c == b'}' as u32 {
                    *src = p;
                    return 1;
                } else if c == b'-' as u32 && in_cc {
                    *src = p;
                    return 2;
                }
                p = prev;
                break;
            } else if p >= end {
                return ONIGERR_INVALID_CODE_POINT_VALUE;
            }
        }
        let r = self.scan_number_of_base(&mut p, end, 1, rcode, base);
        if r != 0 {
            return r;
        }
        *src = p;
        ONIG_NORMAL
    }
}

/* ===== tokenizer ===== */

fn get_name_end_code_point(start: u32) -> u32 {
    match start {
        0x3c => b'>' as u32,
        0x27 => b'\'' as u32,
        0x28 => b')' as u32,
        _ => 0,
    }
}

impl<'a> ParseEnv<'a> {
    fn conv_backslash_value(&self, c: u32) -> u32 {
        if self.syntax.op(ONIG_SYN_OP_ESC_CONTROL_CHARS) {
            match char::from_u32(c).unwrap_or('\0') {
                'n' => return b'\n' as u32,
                't' => return b'\t' as u32,
                'r' => return b'\r' as u32,
                'f' => return 0x0c,
                'a' => return 0x07,
                'b' => return 0x08,
                'e' => return 0x1b,
                'v' => {
                    if self.syntax.op2(ONIG_SYN_OP2_ESC_V_VTAB) {
                        return 0x0b;
                    }
                }
                _ => {}
            }
        }
        c
    }

    fn fetch_interval(&self, src: &mut usize, end: usize, tok: &mut PToken) -> i32 {
        let mut p = *src;
        let mut prev = p;
        let mut r = 0;
        let mut non_low = false;
        let syn_allow = self.syntax.bv(ONIG_SYN_ALLOW_INVALID_INTERVAL);

        if p >= end {
            if syn_allow {
                return 1;
            } else {
                return ONIGERR_END_PATTERN_AT_LEFT_BRACE;
            }
        }

        if !syn_allow {
            let c = self.ppeek(p, end);
            if c == b')' as u32 || c == b'(' as u32 || c == b'|' as u32 {
                return ONIGERR_END_PATTERN_AT_LEFT_BRACE;
            }
        }

        let mut low = self.scan_number(&mut p, end);
        if low < 0 {
            return ONIGERR_TOO_BIG_NUMBER_FOR_REPEAT_RANGE;
        }
        if low > ONIG_MAX_REPEAT_NUM {
            return ONIGERR_TOO_BIG_NUMBER_FOR_REPEAT_RANGE;
        }

        let invalid = |syn_allow: bool| -> i32 {
            if syn_allow {
                1
            } else {
                ONIGERR_INVALID_REPEAT_RANGE_PATTERN
            }
        };

        if p == *src {
            if self.syntax.bv(ONIG_SYN_ALLOW_INTERVAL_LOW_ABBREV) {
                low = 0;
                non_low = true;
            } else {
                return invalid(syn_allow);
            }
        }

        if p >= end {
            return invalid(syn_allow);
        }
        let mut c = self.pfetch(&mut p, &mut prev, end);
        let mut up;
        if c == b',' as u32 {
            let prev2 = p;
            up = self.scan_number(&mut p, end);
            if up < 0 {
                return ONIGERR_TOO_BIG_NUMBER_FOR_REPEAT_RANGE;
            }
            if up > ONIG_MAX_REPEAT_NUM {
                return ONIGERR_TOO_BIG_NUMBER_FOR_REPEAT_RANGE;
            }
            if p == prev2 {
                if non_low {
                    return invalid(syn_allow);
                }
                up = INFINITE_REPEAT;
            }
        } else {
            if non_low {
                return invalid(syn_allow);
            }
            p = prev;
            up = low;
            r = 2;
        }

        if p >= end {
            return invalid(syn_allow);
        }
        c = self.pfetch(&mut p, &mut prev, end);
        if self.syntax.op(ONIG_SYN_OP_ESC_BRACE_INTERVAL) {
            if c != MC_ESC || p >= end {
                return invalid(syn_allow);
            }
            c = self.pfetch(&mut p, &mut prev, end);
        }
        if c != b'}' as u32 {
            return invalid(syn_allow);
        }

        if !is_infinite_repeat(up) && low > up {
            if self.syntax.op2(ONIG_SYN_OP2_PLUS_POSSESSIVE_INTERVAL) {
                return ONIGERR_UPPER_SMALLER_THAN_LOWER_IN_REPEAT_RANGE;
            }
            tok.rep_possessive = true;
            std::mem::swap(&mut low, &mut up);
        } else {
            tok.rep_possessive = false;
        }

        tok.typ = TK_INTERVAL;
        tok.rep_lower = low;
        tok.rep_upper = up;
        *src = p;
        r
    }

    /// \M-, \C-, \c, or \...
    fn fetch_escaped_value_raw(&self, src: &mut usize, end: usize, val: &mut u32) -> i32 {
        let mut p = *src;
        if p >= end {
            return ONIGERR_END_PATTERN_AT_ESCAPE;
        }
        let mut c = self.pfetch_s(&mut p, end);
        let mut control = false;
        match char::from_u32(c).unwrap_or('\0') {
            'M' if self.syntax.op2(ONIG_SYN_OP2_ESC_CAPITAL_M_BAR_META) => {
                if p >= end {
                    return ONIGERR_END_PATTERN_AT_META;
                }
                c = self.pfetch_s(&mut p, end);
                if c != b'-' as u32 {
                    return ONIGERR_META_CODE_SYNTAX;
                }
                if p >= end {
                    return ONIGERR_END_PATTERN_AT_META;
                }
                c = self.pfetch_s(&mut p, end);
                if c == MC_ESC {
                    let v = self.fetch_escaped_value_raw(&mut p, end, &mut c);
                    if v < 0 {
                        return v;
                    }
                }
                c = (c & 0xff) | 0x80;
            }
            'C' if self.syntax.op2(ONIG_SYN_OP2_ESC_CAPITAL_C_BAR_CONTROL) => {
                if p >= end {
                    return ONIGERR_END_PATTERN_AT_CONTROL;
                }
                c = self.pfetch_s(&mut p, end);
                if c != b'-' as u32 {
                    return ONIGERR_CONTROL_CODE_SYNTAX;
                }
                control = true;
            }
            'c' if self.syntax.op(ONIG_SYN_OP_ESC_C_CONTROL) => {
                control = true;
            }
            _ => {
                c = self.conv_backslash_value(c);
            }
        }
        if control {
            if p >= end {
                return ONIGERR_END_PATTERN_AT_CONTROL;
            }
            c = self.pfetch_s(&mut p, end);
            if c == b'?' as u32 {
                c = 0o177;
            } else {
                if c == MC_ESC {
                    let v = self.fetch_escaped_value_raw(&mut p, end, &mut c);
                    if v < 0 {
                        return v;
                    }
                }
                c &= 0x9f;
            }
        }
        *src = p;
        *val = c;
        0
    }

    fn fetch_escaped_value(&self, src: &mut usize, end: usize, val: &mut u32) -> i32 {
        let r = self.fetch_escaped_value_raw(src, end, val);
        if r != 0 {
            return r;
        }
        let len = self.enc.code_to_mbclen(*val);
        if len < 0 {
            return len;
        }
        0
    }

    /// \k<name+n>, \k<name-n> ... ; returns 1 if a level exists, 0 if
    /// not, or an error
    fn fetch_name_with_level(
        &mut self,
        start_code: u32,
        src: &mut usize,
        end: usize,
        rname_end: &mut usize,
        rback_num: &mut i32,
        rlevel: &mut i32,
        num_type: &mut RefNum,
    ) -> i32 {
        let mut p = *src;
        let mut prev = p;
        let mut c: u32;
        *rback_num = 0;
        let mut exist_level = false;
        *num_type = RefNum::IsNotNum;
        let mut sign = 1;
        let mut pnum_head = *src;
        let end_code = get_name_end_code_point(start_code);
        *rlevel = 0;
        let mut digit_count = 0;
        let mut name_end = end;
        let mut r = 0;

        if p >= end {
            return ONIGERR_EMPTY_GROUP_NAME;
        }
        c = self.pfetch(&mut p, &mut prev, end);
        if c == end_code {
            return ONIGERR_EMPTY_GROUP_NAME;
        }
        if self.is_code_digit_ascii(c) {
            *num_type = RefNum::IsAbsNum;
            digit_count += 1;
        } else if c == b'-' as u32 {
            *num_type = RefNum::IsRelNum;
            sign = -1;
            pnum_head = p;
        } else if c == b'+' as u32 {
            *num_type = RefNum::IsRelNum;
            sign = 1;
            pnum_head = p;
        } else if !self.enc.is_code_word(c) {
            r = ONIGERR_INVALID_CHAR_IN_GROUP_NAME;
        }

        while p < end {
            name_end = p;
            c = self.pfetch(&mut p, &mut prev, end);
            if c == end_code || c == b')' as u32 || c == b'+' as u32 || c == b'-' as u32 {
                if *num_type != RefNum::IsNotNum && digit_count == 0 {
                    r = ONIGERR_INVALID_GROUP_NAME;
                }
                break;
            }
            if *num_type != RefNum::IsNotNum {
                if self.is_code_digit_ascii(c) {
                    digit_count += 1;
                } else {
                    r = ONIGERR_INVALID_GROUP_NAME;
                    *num_type = RefNum::IsNotNum;
                }
            } else if !self.enc.is_code_word(c) {
                r = ONIGERR_INVALID_CHAR_IN_GROUP_NAME;
            }
        }

        let mut goto_end = false;
        if r == 0 && c != end_code {
            let mut goto_err = true;
            if c == b'+' as u32 || c == b'-' as u32 {
                let flag = if c == b'-' as u32 { -1 } else { 1 };
                if p >= end {
                    r = ONIGERR_INVALID_CHAR_IN_GROUP_NAME;
                    goto_end = true;
                    goto_err = false;
                } else {
                    c = self.pfetch(&mut p, &mut prev, end);
                    if self.is_code_digit_ascii(c) {
                        p = prev;
                        let level = self.scan_number(&mut p, end);
                        if level < 0 {
                            return ONIGERR_TOO_BIG_NUMBER;
                        }
                        *rlevel = level * flag;
                        exist_level = true;
                        if p < end {
                            c = self.pfetch(&mut p, &mut prev, end);
                            if c == end_code {
                                goto_end = true;
                                goto_err = false;
                            }
                        }
                    }
                }
            }
            if goto_err {
                name_end = end;
                r = ONIGERR_INVALID_GROUP_NAME;
            }
        }
        let _ = goto_end;

        // end:
        if r == 0 {
            if *num_type != RefNum::IsNotNum {
                let mut ph = pnum_head;
                *rback_num = self.scan_number(&mut ph, name_end);
                if *rback_num < 0 {
                    return ONIGERR_TOO_BIG_NUMBER;
                } else if *rback_num == 0 && *num_type == RefNum::IsRelNum {
                    // goto err2
                    r = ONIGERR_INVALID_GROUP_NAME;
                    self.set_error_string(*src, name_end);
                    return r;
                }
                *rback_num *= sign;
            }
            *rname_end = name_end;
            *src = p;
            if exist_level { 1 } else { 0 }
        } else {
            self.set_error_string(*src, name_end);
            r
        }
    }

    /// ref: false -> define name (don't allow number name),
    /// true -> reference name (allow number name)
    #[allow(unused_assignments)] // the C ignores some error codes it computes
    fn fetch_name(
        &mut self,
        start_code: u32,
        src: &mut usize,
        end: usize,
        rname_end: &mut usize,
        rback_num: &mut i32,
        num_type: &mut RefNum,
        is_ref: bool,
    ) -> i32 {
        let mut p = *src;
        let mut c: u32;
        *rback_num = 0;
        let end_code = get_name_end_code_point(start_code);
        let mut digit_count = 0;
        let mut name_end = end;
        let mut pnum_head = *src;
        let mut r = 0;
        *num_type = RefNum::IsNotNum;
        let mut sign = 1;

        if p >= end {
            return ONIGERR_EMPTY_GROUP_NAME;
        }
        c = self.pfetch_s(&mut p, end);
        if c == end_code {
            return ONIGERR_EMPTY_GROUP_NAME;
        }
        if self.is_code_digit_ascii(c) {
            if is_ref {
                *num_type = RefNum::IsAbsNum;
            } else {
                r = ONIGERR_INVALID_GROUP_NAME;
            }
            digit_count += 1;
        } else if c == b'-' as u32 {
            if is_ref {
                *num_type = RefNum::IsRelNum;
                sign = -1;
                pnum_head = p;
            } else {
                r = ONIGERR_INVALID_GROUP_NAME;
            }
        } else if c == b'+' as u32 {
            if is_ref {
                *num_type = RefNum::IsRelNum;
                sign = 1;
                pnum_head = p;
            } else {
                r = ONIGERR_INVALID_GROUP_NAME;
            }
        } else if !self.enc.is_code_word(c) {
            r = ONIGERR_INVALID_CHAR_IN_GROUP_NAME;
        }

        if r == 0 {
            while p < end {
                name_end = p;
                c = self.pfetch_s(&mut p, end);
                if c == end_code || c == b')' as u32 {
                    if *num_type != RefNum::IsNotNum && digit_count == 0 {
                        r = ONIGERR_INVALID_GROUP_NAME;
                    }
                    break;
                }
                if *num_type != RefNum::IsNotNum {
                    if self.is_code_digit_ascii(c) {
                        digit_count += 1;
                    } else {
                        if !self.enc.is_code_word(c) {
                            r = ONIGERR_INVALID_CHAR_IN_GROUP_NAME;
                        } else {
                            r = ONIGERR_INVALID_GROUP_NAME;
                        }
                        *num_type = RefNum::IsNotNum;
                    }
                } else if !self.enc.is_code_word(c) {
                    r = ONIGERR_INVALID_CHAR_IN_GROUP_NAME;
                }
            }

            if c != end_code {
                r = ONIGERR_INVALID_GROUP_NAME;
                self.set_error_string(*src, name_end);
                return r;
            }

            if *num_type != RefNum::IsNotNum {
                let mut ph = pnum_head;
                *rback_num = self.scan_number(&mut ph, name_end);
                if *rback_num < 0 {
                    return ONIGERR_TOO_BIG_NUMBER;
                } else if *rback_num == 0 && *num_type == RefNum::IsRelNum {
                    r = ONIGERR_INVALID_GROUP_NAME;
                    self.set_error_string(*src, name_end);
                    return r;
                }
                *rback_num *= sign;
            }

            /* the C ignores an invalid char found in the loop here */
            *rname_end = name_end;
            *src = p;
            0
        } else {
            while p < end {
                name_end = p;
                c = self.pfetch_s(&mut p, end);
                if c == end_code || c == b')' as u32 {
                    break;
                }
            }
            if p >= end {
                name_end = end;
            }
            self.set_error_string(*src, name_end);
            r
        }
    }

    fn find_str_position(&self, s: &[u32], from: usize, to: usize) -> Option<(usize, usize)> {
        let mut p = from;
        while p < to {
            let x = self.enc.mbc_to_code(self.pattern, p, to);
            let mut q = p + self.enclen(p);
            if x == s[0] {
                let mut i = 1;
                while i < s.len() && q < to {
                    let x = self.enc.mbc_to_code(self.pattern, q, to);
                    if x != s[i] {
                        break;
                    }
                    q += self.enclen(q);
                    i += 1;
                }
                if i >= s.len() {
                    return Some((p, q));
                }
            }
            p = q;
        }
        None
    }

    fn is_mc_esc_code(&self, code: u32) -> bool {
        code == MC_ESC && !self.syntax.op2(ONIG_SYN_OP2_INEFFECTIVE_ESCAPE)
    }

    fn is_head_of_bre_subexp(&self, p: usize, end: usize) -> bool {
        let start = 0usize;
        if p > start {
            let mut p = get_prev_char_head(self.enc, self.pattern, start, p).unwrap_or(start);
            if p > start {
                let code = self.enc.mbc_to_code(self.pattern, p, end);
                if code == b'(' as u32
                    || (code == b'|' as u32 && self.syntax.op(ONIG_SYN_OP_ESC_VBAR_ALT))
                {
                    p = get_prev_char_head(self.enc, self.pattern, start, p).unwrap_or(start);
                    let code = self.enc.mbc_to_code(self.pattern, p, end);
                    if self.is_mc_esc_code(code) {
                        let mut count = 0;
                        while p > start {
                            p = get_prev_char_head(self.enc, self.pattern, start, p)
                                .unwrap_or(start);
                            let code = self.enc.mbc_to_code(self.pattern, p, end);
                            if !self.is_mc_esc_code(code) {
                                break;
                            }
                            count += 1;
                        }
                        return count % 2 == 0;
                    }
                }
            }
            false
        } else {
            true
        }
    }

    fn is_end_of_bre_subexp(&self, mut p: usize, end: usize) -> bool {
        if p == end {
            return true;
        }
        let code = self.enc.mbc_to_code(self.pattern, p, end);
        if self.is_mc_esc_code(code) {
            p += self.enclen(p);
            if p < end {
                let code = self.enc.mbc_to_code(self.pattern, p, end);
                if code == b')' as u32
                    || (code == b'|' as u32 && self.syntax.op(ONIG_SYN_OP_ESC_VBAR_ALT))
                {
                    return true;
                }
            }
        }
        false
    }

    fn is_posix_bracket_start(&self, from: usize, to: usize) -> bool {
        let mut n = 0;
        let mut p = from;
        while p < to {
            let x = self.enc.mbc_to_code(self.pattern, p, to);
            p += self.enclen(p);
            if x == b':' as u32 {
                if p < to {
                    let x = self.enc.mbc_to_code(self.pattern, p, to);
                    if x == b']' as u32 {
                        return n != 0;
                    }
                }
                return false;
            } else if x == b'^' as u32 && n == 0 {
            } else if !self.enc.is_code_ctype(x, ONIGENC_CTYPE_ALPHA) {
                break;
            }
            n += 1;
        }
        false
    }

    fn fetch_token_cc(
        &mut self,
        tok: &mut PToken,
        src: &mut usize,
        end: usize,
        state: CState,
    ) -> i32 {
        let mut p = *src;
        let mut prev = p;
        let syn = self.syntax;

        if tok.code_point_continue {
            let mut code = 0u32;
            let r = self.get_next_code_point(&mut p, end, tok.base_num, true, &mut code);
            if r == 1 {
                tok.code_point_continue = false;
            } else if r == 2 {
                tok.typ = TK_CC_RANGE;
                *src = p;
                return tok.typ;
            } else if r == 0 {
                tok.typ = TK_CODE_POINT;
                tok.code = code;
                *src = p;
                return tok.typ;
            } else {
                return r;
            }
        }

        if p >= end {
            tok.typ = TK_EOT;
            return tok.typ;
        }

        let mut c = self.pfetch(&mut p, &mut prev, end);
        tok.typ = TK_CHAR;
        tok.base_num = 0;
        tok.code = c;
        tok.escaped = false;

        if c == b']' as u32 {
            tok.typ = TK_CC_CLOSE;
        } else if c == b'-' as u32 {
            tok.typ = TK_CC_RANGE;
        } else if c == MC_ESC {
            if !syn.bv(ONIG_SYN_BACKSLASH_ESCAPE_IN_CC) {
                *src = p;
                return tok.typ;
            }
            if p >= end {
                return ONIGERR_END_PATTERN_AT_ESCAPE;
            }
            c = self.pfetch(&mut p, &mut prev, end);
            tok.escaped = true;
            tok.code = c;
            match char::from_u32(c).unwrap_or('\0') {
                'w' => {
                    tok.typ = TK_CHAR_TYPE;
                    tok.prop_ctype = ONIGENC_CTYPE_WORD as i32;
                    tok.prop_not = false;
                }
                'W' => {
                    tok.typ = TK_CHAR_TYPE;
                    tok.prop_ctype = ONIGENC_CTYPE_WORD as i32;
                    tok.prop_not = true;
                }
                'd' => {
                    tok.typ = TK_CHAR_TYPE;
                    tok.prop_ctype = ONIGENC_CTYPE_DIGIT as i32;
                    tok.prop_not = false;
                }
                'D' => {
                    tok.typ = TK_CHAR_TYPE;
                    tok.prop_ctype = ONIGENC_CTYPE_DIGIT as i32;
                    tok.prop_not = true;
                }
                's' => {
                    tok.typ = TK_CHAR_TYPE;
                    tok.prop_ctype = ONIGENC_CTYPE_SPACE as i32;
                    tok.prop_not = false;
                }
                'S' => {
                    tok.typ = TK_CHAR_TYPE;
                    tok.prop_ctype = ONIGENC_CTYPE_SPACE as i32;
                    tok.prop_not = true;
                }
                'h' => {
                    if syn.op2(ONIG_SYN_OP2_ESC_H_XDIGIT) {
                        tok.typ = TK_CHAR_TYPE;
                        tok.prop_ctype = ONIGENC_CTYPE_XDIGIT as i32;
                        tok.prop_not = false;
                    }
                }
                'H' => {
                    if syn.op2(ONIG_SYN_OP2_ESC_H_XDIGIT) {
                        tok.typ = TK_CHAR_TYPE;
                        tok.prop_ctype = ONIGENC_CTYPE_XDIGIT as i32;
                        tok.prop_not = true;
                    }
                }
                'p' | 'P' => {
                    if p < end {
                        let c2 = self.ppeek(p, end);
                        if c2 == b'{' as u32 && syn.op2(ONIG_SYN_OP2_ESC_P_BRACE_CHAR_PROPERTY) {
                            self.pinc(&mut p, &mut prev);
                            tok.typ = TK_CHAR_PROPERTY;
                            tok.prop_not = c == b'P' as u32;
                            if p < end && syn.op2(ONIG_SYN_OP2_ESC_P_BRACE_CIRCUMFLEX_NOT) {
                                let c2 = self.pfetch(&mut p, &mut prev, end);
                                if c2 == b'^' as u32 {
                                    tok.prop_not = !tok.prop_not;
                                } else {
                                    p = prev;
                                }
                            }
                        }
                    }
                }
                'o' | 'x' => {
                    if p < end {
                        let prev0 = p;
                        let is_o = c == b'o' as u32;
                        let mut code = 0u32;
                        let brace = self.ppeek(p, end) == b'{' as u32
                            && if is_o {
                                syn.op(ONIG_SYN_OP_ESC_O_BRACE_OCTAL)
                            } else {
                                syn.op(ONIG_SYN_OP_ESC_X_BRACE_HEX8)
                            };
                        if brace {
                            self.pinc(&mut p, &mut prev);
                            let r = if is_o {
                                self.scan_octal_number(&mut p, end, 0, 11, &mut code)
                            } else {
                                self.scan_hexadecimal_number(&mut p, end, 0, 8, &mut code)
                            };
                            if r < 0 {
                                return r;
                            }
                            if p < end {
                                let c2 = self.ppeek(p, end);
                                if is_o {
                                    if self.is_code_digit_ascii(c2) {
                                        return ONIGERR_TOO_LONG_WIDE_CHAR_VALUE;
                                    }
                                } else if self.is_code_xdigit_ascii(c2) {
                                    return ONIGERR_TOO_LONG_WIDE_CHAR_VALUE;
                                }
                            }
                            tok.base_num = if is_o { 8 } else { 16 };
                            // brace_code_point_entry:
                            if p > prev0 + self.enclen(prev0) {
                                if p >= end {
                                    return ONIGERR_INVALID_CODE_POINT_VALUE;
                                }
                                if self.ppeek(p, end) == b'}' as u32 {
                                    self.pinc(&mut p, &mut prev);
                                } else {
                                    let curr_state = if state == CState::Range {
                                        CPS_EMPTY
                                    } else {
                                        CPS_START
                                    };
                                    let r = self.check_code_point_sequence_cc(
                                        p,
                                        end,
                                        tok.base_num,
                                        curr_state,
                                    );
                                    if r < 0 {
                                        return r;
                                    }
                                    if r == 0 {
                                        return ONIGERR_INVALID_CODE_POINT_VALUE;
                                    }
                                    tok.code_point_continue = true;
                                }
                                tok.typ = TK_CODE_POINT;
                                tok.code = code;
                            } else {
                                p = prev0;
                            }
                        } else if !is_o && syn.op(ONIG_SYN_OP_ESC_X_HEX2) {
                            let r = self.scan_hexadecimal_number(&mut p, end, 0, 2, &mut code);
                            if r < 0 {
                                return r;
                            }
                            if p == prev0 {
                                code = 0;
                            }
                            tok.typ = TK_CRUDE_BYTE;
                            tok.base_num = 16;
                            tok.byte = code as u8;
                        }
                    }
                }
                'u' => {
                    if p < end {
                        let prev0 = p;
                        if syn.op2(ONIG_SYN_OP2_ESC_U_HEX4) {
                            let mut code = 0u32;
                            let r = self.scan_hexadecimal_number(&mut p, end, 4, 4, &mut code);
                            if r < 0 {
                                return r;
                            }
                            if p == prev0 {
                                code = 0;
                            }
                            tok.typ = TK_CODE_POINT;
                            tok.base_num = 16;
                            tok.code = code;
                        }
                    }
                }
                '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' => {
                    if syn.op(ONIG_SYN_OP_ESC_OCTAL3) {
                        p = prev;
                        let prev0 = p;
                        let mut code = 0u32;
                        let r = self.scan_octal_number(&mut p, end, 0, 3, &mut code);
                        if r < 0 {
                            return r;
                        }
                        if code >= 256 {
                            return ONIGERR_TOO_BIG_NUMBER;
                        }
                        if p == prev0 {
                            code = 0;
                        }
                        tok.typ = TK_CRUDE_BYTE;
                        tok.base_num = 8;
                        tok.byte = code as u8;
                    }
                }
                _ => {
                    p = prev;
                    let mut c2 = 0u32;
                    let r = self.fetch_escaped_value(&mut p, end, &mut c2);
                    if r < 0 {
                        return r;
                    }
                    if tok.code != c2 {
                        tok.code = c2;
                        tok.typ = TK_CODE_POINT;
                    }
                }
            }
        } else if c == b'[' as u32 {
            let mut cc_in_cc = true;
            if syn.op(ONIG_SYN_OP_POSIX_BRACKET) && self.ppeek(p, end) == b':' as u32 {
                tok.backp = p;
                self.pinc(&mut p, &mut prev);
                if self.is_posix_bracket_start(p, end) {
                    tok.typ = TK_CC_POSIX_BRACKET_OPEN;
                    cc_in_cc = false;
                } else {
                    p = prev;
                }
            }
            if cc_in_cc && syn.op2(ONIG_SYN_OP2_CCLASS_SET_OP) {
                tok.typ = TK_CC_OPEN_CC;
            }
        } else if c == b'&' as u32 {
            if syn.op2(ONIG_SYN_OP2_CCLASS_SET_OP) && p < end && self.ppeek(p, end) == b'&' as u32 {
                self.pinc(&mut p, &mut prev);
                tok.typ = TK_CC_AND;
            }
        }

        *src = p;
        tok.typ
    }
}

impl<'a> ParseEnv<'a> {
    /// greedy_check2 / possessive_check of fetch_token
    fn tok_greedy_check2(&self, tok: &mut PToken, p: &mut usize, prev: &mut usize, end: usize) {
        if *p < end
            && self.ppeek(*p, end) == b'?' as u32
            && self.syntax.op(ONIG_SYN_OP_QMARK_NON_GREEDY)
            && !tok.rep_possessive
        {
            self.pfetch(p, prev, end);
            tok.rep_greedy = false;
            tok.rep_possessive = false;
        } else {
            self.tok_possessive_check(tok, p, prev, end);
        }
    }

    fn tok_possessive_check(&self, tok: &mut PToken, p: &mut usize, prev: &mut usize, end: usize) {
        tok.rep_greedy = true;
        if *p < end
            && self.ppeek(*p, end) == b'+' as u32
            && ((self.syntax.op2(ONIG_SYN_OP2_PLUS_POSSESSIVE_REPEAT) && tok.typ != TK_INTERVAL)
                || (self.syntax.op2(ONIG_SYN_OP2_PLUS_POSSESSIVE_INTERVAL)
                    && tok.typ == TK_INTERVAL))
            && !tok.rep_possessive
        {
            self.pfetch(p, prev, end);
            tok.rep_possessive = true;
        }
    }

    /// backref_start of fetch_token (\k<...>, (?P=...))
    fn tok_backref_start(
        &mut self,
        tok: &mut PToken,
        c: u32,
        p: &mut usize,
        end: usize,
        allow_num: bool,
    ) -> i32 {
        let syn = self.syntax;
        let prev = *p;
        let mut name_end = 0usize;
        let mut back_num = 0i32;
        let mut level = 0i32;
        let mut num_type = RefNum::IsNotNum;
        let r = self.fetch_name_with_level(
            c,
            p,
            end,
            &mut name_end,
            &mut back_num,
            &mut level,
            &mut num_type,
        );
        tok.br_level = level;
        tok.br_exist_level = r == 1;
        if r < 0 {
            return r;
        }

        if num_type != RefNum::IsNotNum {
            if !allow_num {
                return ONIGERR_INVALID_BACKREF;
            }
            if num_type == RefNum::IsRelNum {
                back_num = self.backref_rel_to_abs(back_num);
            }
            if back_num <= 0 {
                return ONIGERR_INVALID_BACKREF;
            }
            if syn.bv(ONIG_SYN_STRICT_CHECK_BACKREF)
                && (back_num > self.num_mem || self.mem_node(back_num) == NULL_NODE)
            {
                return ONIGERR_INVALID_BACKREF;
            }
            tok.typ = TK_BACKREF;
            tok.br_by_name = false;
            tok.br_num = 1;
            tok.br_ref1 = back_num;
        } else {
            let backs = match self.name_to_group_numbers(prev, name_end) {
                Ok(b) => b,
                Err(e) => return e,
            };
            if backs.is_empty() {
                return ONIGERR_UNDEFINED_NAME_REFERENCE;
            }
            if syn.bv(ONIG_SYN_STRICT_CHECK_BACKREF) {
                for &b in &backs {
                    if b > self.num_mem || self.mem_node(b) == NULL_NODE {
                        return ONIGERR_INVALID_BACKREF;
                    }
                }
            }
            tok.typ = TK_BACKREF;
            tok.br_by_name = true;
            tok.br_num = backs.len() as i32;
            tok.br_ref1 = backs[0];
            tok.br_refs = backs;
        }
        0
    }

    /// call_start of fetch_token (\g<...>, (?P>...))
    fn tok_call_start(
        &mut self,
        tok: &mut PToken,
        c: u32,
        p: &mut usize,
        end: usize,
        allow_num: bool,
    ) -> i32 {
        let prev = *p;
        let mut name_end = 0usize;
        let mut gnum = 0i32;
        let mut num_type = RefNum::IsNotNum;
        let r = self.fetch_name(c, p, end, &mut name_end, &mut gnum, &mut num_type, true);
        if r < 0 {
            return r;
        }
        if num_type != RefNum::IsNotNum {
            if !allow_num {
                return ONIGERR_UNDEFINED_GROUP_REFERENCE;
            }
            if num_type == RefNum::IsRelNum {
                gnum = self.backref_rel_to_abs(gnum);
                if gnum < 0 {
                    self.set_error_string(prev, name_end);
                    return ONIGERR_UNDEFINED_GROUP_REFERENCE;
                }
            }
            tok.call_by_number = true;
            tok.call_gnum = gnum;
        } else {
            tok.call_by_number = false;
            tok.call_gnum = 0;
        }
        tok.typ = TK_CALL;
        tok.call_name = prev;
        tok.call_name_end = name_end;
        0
    }

    fn fetch_token(&mut self, tok: &mut PToken, src: &mut usize, end: usize) -> i32 {
        let syn = self.syntax;
        let mut p = *src;
        let mut prev = p;

        if tok.code_point_continue {
            let mut code = 0u32;
            let r = self.get_next_code_point(&mut p, end, tok.base_num, false, &mut code);
            if r == 1 {
                tok.code_point_continue = false;
            } else if r == 0 {
                tok.typ = TK_CODE_POINT;
                tok.code = code;
                *src = p;
                return tok.typ;
            } else {
                return r;
            }
        }

        'start: loop {
            if p >= end {
                tok.typ = TK_EOT;
                return tok.typ;
            }

            tok.typ = TK_STRING;
            tok.base_num = 0;
            tok.backp = p;

            let mut c = self.pfetch(&mut p, &mut prev, end);
            if self.is_mc_esc_code(c) {
                if p >= end {
                    return ONIGERR_END_PATTERN_AT_ESCAPE;
                }
                tok.backp = p;
                c = self.pfetch(&mut p, &mut prev, end);
                tok.code = c;
                tok.escaped = true;
                match char::from_u32(c).unwrap_or('\0') {
                    '*' => {
                        if syn.op(ONIG_SYN_OP_ESC_ASTERISK_ZERO_INF) {
                            tok.typ = TK_REPEAT;
                            tok.rep_lower = 0;
                            tok.rep_upper = INFINITE_REPEAT;
                            tok.rep_possessive = false;
                            self.tok_greedy_check2(tok, &mut p, &mut prev, end);
                        }
                    }
                    '+' => {
                        if syn.op(ONIG_SYN_OP_ESC_PLUS_ONE_INF) {
                            tok.typ = TK_REPEAT;
                            tok.rep_lower = 1;
                            tok.rep_upper = INFINITE_REPEAT;
                            tok.rep_possessive = false;
                            self.tok_greedy_check2(tok, &mut p, &mut prev, end);
                        }
                    }
                    '?' => {
                        if syn.op(ONIG_SYN_OP_ESC_QMARK_ZERO_ONE) {
                            tok.typ = TK_REPEAT;
                            tok.rep_lower = 0;
                            tok.rep_upper = 1;
                            tok.rep_possessive = false;
                            self.tok_greedy_check2(tok, &mut p, &mut prev, end);
                        }
                    }
                    '{' => {
                        if syn.op(ONIG_SYN_OP_ESC_BRACE_INTERVAL) {
                            let r = self.fetch_interval(&mut p, end, tok);
                            if r < 0 {
                                return r;
                            }
                            if r == 0 {
                                self.tok_greedy_check2(tok, &mut p, &mut prev, end);
                            } else if r == 2 {
                                if syn.bv(ONIG_SYN_FIXED_INTERVAL_IS_GREEDY_ONLY) {
                                    self.tok_possessive_check(tok, &mut p, &mut prev, end);
                                } else {
                                    self.tok_greedy_check2(tok, &mut p, &mut prev, end);
                                }
                            }
                        }
                    }
                    '|' => {
                        if syn.op(ONIG_SYN_OP_ESC_VBAR_ALT) {
                            tok.typ = TK_ALT;
                        }
                    }
                    '(' => {
                        if syn.op(ONIG_SYN_OP_ESC_LPAREN_SUBEXP) {
                            tok.typ = TK_SUBEXP_OPEN;
                        }
                    }
                    ')' => {
                        if syn.op(ONIG_SYN_OP_ESC_LPAREN_SUBEXP) {
                            tok.typ = TK_SUBEXP_CLOSE;
                        }
                    }
                    'w' | 'W' => {
                        if syn.op(ONIG_SYN_OP_ESC_W_WORD) {
                            tok.typ = TK_CHAR_TYPE;
                            tok.prop_ctype = ONIGENC_CTYPE_WORD as i32;
                            tok.prop_not = c == b'W' as u32;
                        }
                    }
                    'b' | 'B' => {
                        if syn.op(ONIG_SYN_OP_ESC_B_WORD_BOUND) {
                            tok.typ = TK_ANCHOR;
                            tok.anchor = if c == b'b' as u32 {
                                ANCR_WORD_BOUNDARY
                            } else {
                                ANCR_NO_WORD_BOUNDARY
                            };
                        }
                    }
                    'y' | 'Y' => {
                        /* the C checks the op2 flag against syn->op here */
                        if syn.op(ONIG_SYN_OP2_ESC_X_Y_TEXT_SEGMENT) {
                            tok.typ = TK_ANCHOR;
                            tok.anchor = if c == b'y' as u32 {
                                ANCR_TEXT_SEGMENT_BOUNDARY
                            } else {
                                ANCR_NO_TEXT_SEGMENT_BOUNDARY
                            };
                        }
                    }
                    '<' | '>' => {
                        if syn.op(ONIG_SYN_OP_ESC_LTGT_WORD_BEGIN_END) {
                            tok.typ = TK_ANCHOR;
                            tok.anchor = if c == b'<' as u32 {
                                ANCR_WORD_BEGIN
                            } else {
                                ANCR_WORD_END
                            };
                        }
                    }
                    's' | 'S' => {
                        if syn.op(ONIG_SYN_OP_ESC_S_WHITE_SPACE) {
                            tok.typ = TK_CHAR_TYPE;
                            tok.prop_ctype = ONIGENC_CTYPE_SPACE as i32;
                            tok.prop_not = c == b'S' as u32;
                        }
                    }
                    'd' | 'D' => {
                        if syn.op(ONIG_SYN_OP_ESC_D_DIGIT) {
                            tok.typ = TK_CHAR_TYPE;
                            tok.prop_ctype = ONIGENC_CTYPE_DIGIT as i32;
                            tok.prop_not = c == b'D' as u32;
                        }
                    }
                    'h' | 'H' => {
                        if syn.op2(ONIG_SYN_OP2_ESC_H_XDIGIT) {
                            tok.typ = TK_CHAR_TYPE;
                            tok.prop_ctype = ONIGENC_CTYPE_XDIGIT as i32;
                            tok.prop_not = c == b'H' as u32;
                        }
                    }
                    'K' => {
                        if syn.op2(ONIG_SYN_OP2_ESC_CAPITAL_K_KEEP) {
                            tok.typ = TK_KEEP;
                        }
                    }
                    'R' => {
                        if syn.op2(ONIG_SYN_OP2_ESC_CAPITAL_R_GENERAL_NEWLINE) {
                            tok.typ = TK_GENERAL_NEWLINE;
                        }
                    }
                    'N' => {
                        if syn.op2(ONIG_SYN_OP2_ESC_CAPITAL_N_O_SUPER_DOT) {
                            tok.typ = TK_NO_NEWLINE;
                        }
                    }
                    'O' => {
                        if syn.op2(ONIG_SYN_OP2_ESC_CAPITAL_N_O_SUPER_DOT) {
                            tok.typ = TK_TRUE_ANYCHAR;
                        }
                    }
                    'X' => {
                        if syn.op2(ONIG_SYN_OP2_ESC_X_Y_TEXT_SEGMENT) {
                            tok.typ = TK_TEXT_SEGMENT;
                        }
                    }
                    'A' => {
                        if syn.op(ONIG_SYN_OP_ESC_AZ_BUF_ANCHOR) {
                            tok.typ = TK_ANCHOR;
                            tok.anchor = ANCR_BEGIN_BUF;
                        }
                    }
                    'Z' => {
                        if syn.bv(ONIG_SYN_PYTHON) {
                            tok.typ = TK_ANCHOR;
                            tok.anchor = ANCR_END_BUF;
                        } else if syn.op(ONIG_SYN_OP_ESC_AZ_BUF_ANCHOR) {
                            tok.typ = TK_ANCHOR;
                            tok.anchor = ANCR_SEMI_END_BUF;
                        }
                    }
                    'z' => {
                        if syn.bv(ONIG_SYN_PYTHON) {
                            return ONIGERR_UNDEFINED_OPERATOR;
                        }
                        if syn.op(ONIG_SYN_OP_ESC_AZ_BUF_ANCHOR) {
                            tok.typ = TK_ANCHOR;
                            tok.anchor = ANCR_END_BUF;
                        }
                    }
                    'G' => {
                        if syn.op(ONIG_SYN_OP_ESC_CAPITAL_G_BEGIN_ANCHOR) {
                            tok.typ = TK_ANCHOR;
                            tok.anchor = ANCR_BEGIN_POSITION;
                        }
                    }
                    '`' => {
                        if syn.op2(ONIG_SYN_OP2_ESC_GNU_BUF_ANCHOR) {
                            tok.typ = TK_ANCHOR;
                            tok.anchor = ANCR_BEGIN_BUF;
                        }
                    }
                    '\'' => {
                        if syn.op2(ONIG_SYN_OP2_ESC_GNU_BUF_ANCHOR) {
                            tok.typ = TK_ANCHOR;
                            tok.anchor = ANCR_END_BUF;
                        }
                    }
                    'o' | 'x' => {
                        if p < end {
                            let prev0 = p;
                            let is_o = c == b'o' as u32;
                            let mut code = 0u32;
                            let brace = self.ppeek(p, end) == b'{' as u32
                                && if is_o {
                                    syn.op(ONIG_SYN_OP_ESC_O_BRACE_OCTAL)
                                } else {
                                    syn.op(ONIG_SYN_OP_ESC_X_BRACE_HEX8)
                                };
                            if brace {
                                self.pinc(&mut p, &mut prev);
                                let r = if is_o {
                                    self.scan_octal_number(&mut p, end, 0, 11, &mut code)
                                } else {
                                    self.scan_hexadecimal_number(&mut p, end, 0, 8, &mut code)
                                };
                                if r < 0 {
                                    return r;
                                }
                                if p < end {
                                    let c2 = self.ppeek(p, end);
                                    if is_o {
                                        if self.is_code_digit_ascii(c2) {
                                            return ONIGERR_TOO_LONG_WIDE_CHAR_VALUE;
                                        }
                                    } else if self.is_code_xdigit_ascii(c2) {
                                        return ONIGERR_TOO_LONG_WIDE_CHAR_VALUE;
                                    }
                                }
                                tok.base_num = if is_o { 8 } else { 16 };
                                if p > prev0 + self.enclen(prev0) {
                                    if p >= end {
                                        return ONIGERR_INVALID_CODE_POINT_VALUE;
                                    }
                                    if self.ppeek(p, end) == b'}' as u32 {
                                        self.pinc(&mut p, &mut prev);
                                    } else {
                                        let r =
                                            self.check_code_point_sequence(p, end, tok.base_num);
                                        if r < 0 {
                                            return r;
                                        }
                                        if r == 0 {
                                            return ONIGERR_INVALID_CODE_POINT_VALUE;
                                        }
                                        tok.code_point_continue = true;
                                    }
                                    tok.typ = TK_CODE_POINT;
                                    tok.code = code;
                                } else {
                                    p = prev0;
                                }
                            } else if !is_o && syn.op(ONIG_SYN_OP_ESC_X_HEX2) {
                                let r = self.scan_hexadecimal_number(&mut p, end, 0, 2, &mut code);
                                if r < 0 {
                                    return r;
                                }
                                if p == prev0 {
                                    code = 0;
                                }
                                tok.typ = TK_CRUDE_BYTE;
                                tok.base_num = 16;
                                tok.byte = code as u8;
                            }
                        }
                    }
                    'u' => {
                        if p < end {
                            let prev0 = p;
                            if syn.op2(ONIG_SYN_OP2_ESC_U_HEX4) {
                                let mut code = 0u32;
                                let r = self.scan_hexadecimal_number(&mut p, end, 4, 4, &mut code);
                                if r < 0 {
                                    return r;
                                }
                                if p == prev0 {
                                    code = 0;
                                }
                                tok.typ = TK_CODE_POINT;
                                tok.base_num = 16;
                                tok.code = code;
                            }
                        }
                    }
                    '1'..='9' => {
                        p = prev;
                        let prev0 = p;
                        let r = self.scan_number(&mut p, end);
                        let mut skip = r < 0 || r > ONIG_MAX_BACKREF_NUM;
                        if !skip {
                            if syn.op(ONIG_SYN_OP_DECIMAL_BACKREF) && (r <= self.num_mem || r <= 9)
                            {
                                if syn.bv(ONIG_SYN_STRICT_CHECK_BACKREF)
                                    && (r > self.num_mem || self.mem_node(r) == NULL_NODE)
                                {
                                    return ONIGERR_INVALID_BACKREF;
                                }
                                tok.typ = TK_BACKREF;
                                tok.br_num = 1;
                                tok.br_ref1 = r;
                                tok.br_refs = vec![r];
                                tok.br_by_name = false;
                                tok.br_exist_level = false;
                                tok.br_level = 0;
                            } else {
                                skip = true;
                            }
                        }
                        if skip {
                            // skip_backref:
                            if c == b'8' as u32 || c == b'9' as u32 {
                                p = prev0;
                                self.pinc(&mut p, &mut prev);
                            } else {
                                p = prev0;
                                // fall through to '0'
                                if syn.op(ONIG_SYN_OP_ESC_OCTAL3) {
                                    let prev1 = p;
                                    let mut code = 0u32;
                                    let r = self.scan_octal_number(&mut p, end, 0, 3, &mut code);
                                    /* the C tests the return code (never >= 256) here, not the value */
                                    if r < 0 {
                                        return ONIGERR_TOO_BIG_NUMBER;
                                    }
                                    if p == prev1 {
                                        code = 0;
                                    }
                                    tok.typ = TK_CRUDE_BYTE;
                                    tok.base_num = 8;
                                    tok.byte = code as u8;
                                } else {
                                    self.pinc(&mut p, &mut prev);
                                }
                            }
                        }
                    }
                    '0' => {
                        if syn.op(ONIG_SYN_OP_ESC_OCTAL3) {
                            let prev1 = p;
                            let mut code = 0u32;
                            let r = self.scan_octal_number(&mut p, end, 0, 2, &mut code);
                            /* the C tests the return code (never >= 256) here, not the value */
                            if r < 0 {
                                return ONIGERR_TOO_BIG_NUMBER;
                            }
                            if p == prev1 {
                                code = 0;
                            }
                            tok.typ = TK_CRUDE_BYTE;
                            tok.base_num = 8;
                            tok.byte = code as u8;
                        }
                    }
                    'k' => {
                        if p < end && syn.op2(ONIG_SYN_OP2_ESC_K_NAMED_BACKREF) {
                            let c2 = self.pfetch(&mut p, &mut prev, end);
                            if c2 == b'<' as u32 || c2 == b'\'' as u32 {
                                let r = self.tok_backref_start(tok, c2, &mut p, end, true);
                                if r < 0 {
                                    return r;
                                }
                            } else {
                                p = prev;
                            }
                        }
                    }
                    'g' => {
                        if p < end && syn.op2(ONIG_SYN_OP2_ESC_G_SUBEXP_CALL) {
                            let c2 = self.pfetch(&mut p, &mut prev, end);
                            if c2 == b'<' as u32 || c2 == b'\'' as u32 {
                                let r = self.tok_call_start(tok, c2, &mut p, end, true);
                                if r < 0 {
                                    return r;
                                }
                            } else {
                                p = prev;
                            }
                        }
                    }
                    'Q' => {
                        if syn.op2(ONIG_SYN_OP2_ESC_CAPITAL_Q_QUOTE) {
                            tok.typ = TK_QUOTE_OPEN;
                        }
                    }
                    'p' | 'P' => {
                        if p < end
                            && self.ppeek(p, end) == b'{' as u32
                            && syn.op2(ONIG_SYN_OP2_ESC_P_BRACE_CHAR_PROPERTY)
                        {
                            self.pinc(&mut p, &mut prev);
                            tok.typ = TK_CHAR_PROPERTY;
                            tok.prop_not = c == b'P' as u32;
                            if p < end && syn.op2(ONIG_SYN_OP2_ESC_P_BRACE_CIRCUMFLEX_NOT) {
                                let c2 = self.pfetch(&mut p, &mut prev, end);
                                if c2 == b'^' as u32 {
                                    tok.prop_not = !tok.prop_not;
                                } else {
                                    p = prev;
                                }
                            }
                        }
                    }
                    _ => {
                        p = prev;
                        let mut c2 = 0u32;
                        let r = self.fetch_escaped_value(&mut p, end, &mut c2);
                        if r < 0 {
                            return r;
                        }
                        if tok.code != c2 {
                            tok.typ = TK_CODE_POINT;
                            tok.code = c2;
                        } else {
                            p = tok.backp + self.enclen(tok.backp);
                        }
                    }
                }
            } else {
                tok.code = c;
                tok.escaped = false;

                match char::from_u32(c).unwrap_or('\0') {
                    '.' => {
                        if syn.op(ONIG_SYN_OP_DOT_ANYCHAR) {
                            tok.typ = TK_ANYCHAR;
                        }
                    }
                    '*' => {
                        if syn.op(ONIG_SYN_OP_ASTERISK_ZERO_INF) {
                            tok.typ = TK_REPEAT;
                            tok.rep_lower = 0;
                            tok.rep_upper = INFINITE_REPEAT;
                            tok.rep_possessive = false;
                            self.tok_greedy_check2(tok, &mut p, &mut prev, end);
                        }
                    }
                    '+' => {
                        if syn.op(ONIG_SYN_OP_PLUS_ONE_INF) {
                            tok.typ = TK_REPEAT;
                            tok.rep_lower = 1;
                            tok.rep_upper = INFINITE_REPEAT;
                            tok.rep_possessive = false;
                            self.tok_greedy_check2(tok, &mut p, &mut prev, end);
                        }
                    }
                    '?' => {
                        if syn.op(ONIG_SYN_OP_QMARK_ZERO_ONE) {
                            tok.typ = TK_REPEAT;
                            tok.rep_lower = 0;
                            tok.rep_upper = 1;
                            tok.rep_possessive = false;
                            self.tok_greedy_check2(tok, &mut p, &mut prev, end);
                        }
                    }
                    '{' => {
                        if syn.op(ONIG_SYN_OP_BRACE_INTERVAL) {
                            let r = self.fetch_interval(&mut p, end, tok);
                            if r < 0 {
                                return r;
                            }
                            if r == 0 {
                                self.tok_greedy_check2(tok, &mut p, &mut prev, end);
                            } else if r == 2 {
                                if syn.bv(ONIG_SYN_FIXED_INTERVAL_IS_GREEDY_ONLY) {
                                    self.tok_possessive_check(tok, &mut p, &mut prev, end);
                                } else {
                                    self.tok_greedy_check2(tok, &mut p, &mut prev, end);
                                }
                            }
                        }
                    }
                    '|' => {
                        if syn.op(ONIG_SYN_OP_VBAR_ALT) {
                            tok.typ = TK_ALT;
                        }
                    }
                    '(' => {
                        let mut lparen_end2 = true;
                        if p < end
                            && self.ppeek(p, end) == b'?' as u32
                            && syn.op2(ONIG_SYN_OP2_QMARK_GROUP_EFFECT)
                        {
                            let prev_q = p;
                            self.pinc(&mut p, &mut prev);
                            let mut lparen_qmark_end = true;
                            if p < end {
                                let c1 = self.ppeek(p, end);
                                if c1 == b'#' as u32 {
                                    self.pfetch(&mut p, &mut prev, end);
                                    loop {
                                        if p >= end {
                                            return ONIGERR_END_PATTERN_IN_GROUP;
                                        }
                                        let c3 = self.pfetch(&mut p, &mut prev, end);
                                        if c3 == MC_ESC {
                                            if p < end {
                                                self.pfetch(&mut p, &mut prev, end);
                                            }
                                        } else if c3 == b')' as u32 {
                                            break;
                                        }
                                    }
                                    continue 'start;
                                } else if syn.op2(ONIG_SYN_OP2_QMARK_PERL_SUBEXP_CALL) {
                                    lparen_qmark_end = false;
                                    lparen_end2 = false;
                                    let mut goto_num = false;
                                    match char::from_u32(c1).unwrap_or('\0') {
                                        '&' => {
                                            self.pinc(&mut p, &mut prev);
                                            let name = p;
                                            let mut name_end = 0usize;
                                            let mut gnum = 0;
                                            let mut num_type = RefNum::IsNotNum;
                                            let r = self.fetch_name(
                                                b'(' as u32,
                                                &mut p,
                                                end,
                                                &mut name_end,
                                                &mut gnum,
                                                &mut num_type,
                                                false,
                                            );
                                            if r < 0 {
                                                return r;
                                            }
                                            tok.typ = TK_CALL;
                                            tok.call_by_number = false;
                                            tok.call_gnum = 0;
                                            tok.call_name = name;
                                            tok.call_name_end = name_end;
                                        }
                                        'R' => {
                                            tok.typ = TK_CALL;
                                            tok.call_by_number = true;
                                            tok.call_gnum = 0;
                                            tok.call_name = p;
                                            self.pinc(&mut p, &mut prev);
                                            if self.ppeek(p, end) != b')' as u32 {
                                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                                            }
                                            tok.call_name_end = p;
                                        }
                                        '-' | '+' => {
                                            let mut ok = false;
                                            if p < end {
                                                self.pinc(&mut p, &mut prev);
                                                if p < end {
                                                    let c4 = self.ppeek(p, end);
                                                    if self.enc.is_code_digit(c4) {
                                                        p = prev;
                                                        goto_num = true;
                                                        ok = true;
                                                    }
                                                }
                                            }
                                            if !ok {
                                                p = prev_q;
                                                lparen_end2 = true;
                                            }
                                        }
                                        _ => {
                                            if !self.enc.is_code_digit(c1) {
                                                lparen_qmark_end = true;
                                                lparen_end2 = true;
                                            } else {
                                                goto_num = true;
                                            }
                                        }
                                    }
                                    if goto_num {
                                        // lparen_qmark_num:
                                        let name = p;
                                        let mut name_end = 0usize;
                                        let mut gnum = 0;
                                        let mut num_type = RefNum::IsNotNum;
                                        let r = self.fetch_name(
                                            b'(' as u32,
                                            &mut p,
                                            end,
                                            &mut name_end,
                                            &mut gnum,
                                            &mut num_type,
                                            true,
                                        );
                                        if r < 0 {
                                            return r;
                                        }
                                        if num_type == RefNum::IsNotNum {
                                            return ONIGERR_INVALID_GROUP_NAME;
                                        } else {
                                            if num_type == RefNum::IsRelNum {
                                                gnum = self.backref_rel_to_abs(gnum);
                                                if gnum < 0 {
                                                    self.set_error_string(name, name_end);
                                                    return ONIGERR_UNDEFINED_GROUP_REFERENCE;
                                                }
                                            }
                                            tok.call_by_number = true;
                                            tok.call_gnum = gnum;
                                        }
                                        tok.typ = TK_CALL;
                                        tok.call_name = name;
                                        tok.call_name_end = name_end;
                                    }
                                } else if c1 == b'P' as u32
                                    && syn.op2(ONIG_SYN_OP2_QMARK_CAPITAL_P_NAME)
                                {
                                    self.pinc(&mut p, &mut prev);
                                    if p >= end {
                                        return ONIGERR_END_PATTERN_IN_GROUP;
                                    }
                                    let c4 = self.pfetch(&mut p, &mut prev, end);
                                    lparen_qmark_end = false;
                                    lparen_end2 = false;
                                    if c4 == b'=' as u32 {
                                        let r = self.tok_backref_start(
                                            tok,
                                            b'(' as u32,
                                            &mut p,
                                            end,
                                            false,
                                        );
                                        if r < 0 {
                                            return r;
                                        }
                                    } else if c4 == b'>' as u32 {
                                        let r = self.tok_call_start(
                                            tok,
                                            b'(' as u32,
                                            &mut p,
                                            end,
                                            false,
                                        );
                                        if r < 0 {
                                            return r;
                                        }
                                    } else {
                                        p = prev_q;
                                        lparen_end2 = true;
                                    }
                                }
                            }
                            if lparen_qmark_end {
                                // PUNFETCH: back to before the '?'
                                p = prev;
                            }
                        }
                        if lparen_end2 && syn.op(ONIG_SYN_OP_LPAREN_SUBEXP) {
                            tok.typ = TK_SUBEXP_OPEN;
                        }
                    }
                    ')' => {
                        if syn.op(ONIG_SYN_OP_LPAREN_SUBEXP) {
                            tok.typ = TK_SUBEXP_CLOSE;
                        }
                    }
                    '^' => {
                        if syn.op(ONIG_SYN_OP_LINE_ANCHOR)
                            && (!syn.bv(ONIG_SYN_BRE_ANCHOR_AT_EDGE_OF_SUBEXP)
                                || self.is_head_of_bre_subexp(prev, end))
                        {
                            tok.typ = TK_ANCHOR;
                            tok.anchor = if opton_singleline(self.options) {
                                ANCR_BEGIN_BUF
                            } else {
                                ANCR_BEGIN_LINE
                            };
                        }
                    }
                    '$' => {
                        if syn.op(ONIG_SYN_OP_LINE_ANCHOR)
                            && (!syn.bv(ONIG_SYN_BRE_ANCHOR_AT_EDGE_OF_SUBEXP)
                                || self.is_end_of_bre_subexp(p, end))
                        {
                            tok.typ = TK_ANCHOR;
                            tok.anchor = if opton_singleline(self.options) {
                                ANCR_SEMI_END_BUF
                            } else {
                                ANCR_END_LINE
                            };
                        }
                    }
                    '[' => {
                        if syn.op(ONIG_SYN_OP_BRACKET_CC) {
                            tok.typ = TK_OPEN_CC;
                        }
                    }
                    '#' => {
                        if opton_extend(self.options) {
                            while p < end {
                                let c3 = self.pfetch(&mut p, &mut prev, end);
                                if self.enc.is_code_newline(c3) {
                                    break;
                                }
                            }
                            continue 'start;
                        }
                    }
                    ' ' | '\t' | '\n' | '\r' | '\x0c' => {
                        if opton_extend(self.options) {
                            continue 'start;
                        }
                    }
                    _ => {}
                }
            }
            break;
        }

        *src = p;
        tok.typ
    }
}

/* ===== character classes ===== */

fn add_ctype_to_cc_by_range(cc: &mut CClass, not: bool, sb_out: u32, mbr: &[u32]) -> i32 {
    let n = mbr[0] as usize;
    let from = |i: usize| mbr[i * 2 + 1];
    let to = |i: usize| mbr[i * 2 + 2];

    if !not {
        let mut i = 0;
        'outer: while i < n {
            let mut j = from(i);
            while j <= to(i) {
                if j >= sb_out {
                    if j > from(i) {
                        let r = add_code_range_to_buf(&mut cc.mbuf, j, to(i));
                        if r != 0 {
                            return r;
                        }
                        i += 1;
                    }
                    break 'outer;
                }
                bitset_set_bit(&mut cc.bs, j);
                j += 1;
            }
            i += 1;
        }
        // sb_end:
        while i < n {
            let r = add_code_range_to_buf(&mut cc.mbuf, from(i), to(i));
            if r != 0 {
                return r;
            }
            i += 1;
        }
    } else {
        let mut prev: u32 = 0;
        let mut sb_done = false;
        'o2: for i in 0..n {
            let mut j = prev;
            while j < from(i) {
                if j >= sb_out {
                    sb_done = true;
                    break 'o2;
                }
                bitset_set_bit(&mut cc.bs, j);
                j += 1;
            }
            prev = to(i).wrapping_add(1);
        }
        if !sb_done {
            let mut j = prev;
            while j < sb_out {
                bitset_set_bit(&mut cc.bs, j);
                j += 1;
            }
        }
        // sb_end2:
        prev = sb_out;
        for i in 0..n {
            if prev < from(i) {
                let r = add_code_range_to_buf(&mut cc.mbuf, prev, from(i) - 1);
                if r != 0 {
                    return r;
                }
            }
            prev = to(i).wrapping_add(1);
            if prev == 0 {
                return 0;
            }
        }
        let r = add_code_range_to_buf(&mut cc.mbuf, prev, MAX_CODE_POINT);
        if r != 0 {
            return r;
        }
    }
    0
}

fn add_ctype_to_cc_by_range_limit(
    cc: &mut CClass,
    not: bool,
    sb_out: u32,
    mbr: &[u32],
    limit: u32,
) -> i32 {
    let n = mbr[0] as usize;
    let from = |i: usize| mbr[i * 2 + 1];
    let to = |i: usize| mbr[i * 2 + 2];

    if !not {
        let mut i = 0;
        'outer: while i < n {
            let mut j = from(i);
            while j <= to(i) {
                if j > limit {
                    return 0;
                }
                if j >= sb_out {
                    if j > from(i) {
                        let mut t = to(i);
                        if t > limit {
                            t = limit;
                        }
                        let r = add_code_range_to_buf(&mut cc.mbuf, j, t);
                        if r != 0 {
                            return r;
                        }
                        i += 1;
                    }
                    break 'outer;
                }
                bitset_set_bit(&mut cc.bs, j);
                j += 1;
            }
            i += 1;
        }
        while i < n {
            let f = from(i);
            let mut t = to(i);
            if f > limit {
                break;
            }
            if t > limit {
                t = limit;
            }
            let r = add_code_range_to_buf(&mut cc.mbuf, f, t);
            if r != 0 {
                return r;
            }
            i += 1;
        }
    } else {
        let mut prev: u32 = 0;
        let mut goto_sb_end2 = false;
        for i in 0..n {
            let f = from(i);
            if f > limit {
                let mut j = prev;
                while j < sb_out {
                    bitset_set_bit(&mut cc.bs, j);
                    j += 1;
                }
                goto_sb_end2 = true;
                break;
            }
            let mut j = prev;
            let mut brk = false;
            while j < f {
                if j >= sb_out {
                    brk = true;
                    break;
                }
                bitset_set_bit(&mut cc.bs, j);
                j += 1;
            }
            if brk {
                goto_sb_end2 = true;
                break;
            }
            prev = to(i);
            if prev > limit {
                prev = limit;
            }
            prev = prev.wrapping_add(1);
            if prev == 0 {
                return 0;
            }
        }
        if !goto_sb_end2 {
            let mut j = prev;
            while j < sb_out {
                bitset_set_bit(&mut cc.bs, j);
                j += 1;
            }
        }
        // sb_end2:
        prev = sb_out;
        let mut goto_last = false;
        for i in 0..n {
            let f = from(i);
            if f > limit {
                goto_last = true;
                break;
            }
            if prev < f {
                let r = add_code_range_to_buf(&mut cc.mbuf, prev, f - 1);
                if r != 0 {
                    return r;
                }
            }
            prev = to(i);
            if prev > limit {
                prev = limit;
            }
            prev = prev.wrapping_add(1);
            if prev == 0 {
                return 0;
            }
        }
        let _ = goto_last;
        let r = add_code_range_to_buf(&mut cc.mbuf, prev, MAX_CODE_POINT);
        if r != 0 {
            return r;
        }
    }
    0
}

pub fn add_ctype_to_cc(
    cc: &mut CClass,
    ctype: i32,
    not: bool,
    options: u32,
    enc: &Encoding,
) -> i32 {
    let ascii_mode = is_ascii_mode_ctype(ctype, options);

    match enc.get_ctype_code_range(ctype as u32) {
        Ok((sb_out, ranges)) => {
            return if !ascii_mode {
                add_ctype_to_cc_by_range(cc, not, sb_out, ranges)
            } else {
                add_ctype_to_cc_by_range_limit(cc, not, sb_out, ranges, ASCII_LIMIT)
            };
        }
        Err(r) => {
            if r != ONIG_NO_SUPPORT_CONFIG {
                return r;
            }
        }
    }

    let is_single = enc.is_singlebyte();
    let limit = if ascii_mode {
        ASCII_LIMIT
    } else {
        SINGLE_BYTE_SIZE
    };
    let ct = ctype as u32;

    match ct {
        ONIGENC_CTYPE_ALPHA | ONIGENC_CTYPE_BLANK | ONIGENC_CTYPE_CNTRL | ONIGENC_CTYPE_DIGIT
        | ONIGENC_CTYPE_LOWER | ONIGENC_CTYPE_PUNCT | ONIGENC_CTYPE_SPACE | ONIGENC_CTYPE_UPPER
        | ONIGENC_CTYPE_XDIGIT | ONIGENC_CTYPE_ASCII | ONIGENC_CTYPE_ALNUM => {
            if not {
                for c in 0..limit {
                    if (is_single || enc.code_to_mbclen(c) == 1) && !enc.is_code_ctype(c, ct) {
                        bitset_set_bit(&mut cc.bs, c);
                    }
                }
                for c in limit..SINGLE_BYTE_SIZE {
                    if is_single || enc.code_to_mbclen(c) == 1 {
                        bitset_set_bit(&mut cc.bs, c);
                    }
                }
                if !is_single {
                    let r = set_all_multi_byte_range(enc, &mut cc.mbuf);
                    if r != 0 {
                        return r;
                    }
                }
            } else {
                for c in 0..limit {
                    if (is_single || enc.code_to_mbclen(c) == 1) && enc.is_code_ctype(c, ct) {
                        bitset_set_bit(&mut cc.bs, c);
                    }
                }
            }
        }
        ONIGENC_CTYPE_GRAPH | ONIGENC_CTYPE_PRINT | ONIGENC_CTYPE_WORD => {
            if not {
                for c in 0..limit {
                    if (is_single || enc.code_to_mbclen(c) == 1) && !enc.is_code_ctype(c, ct) {
                        bitset_set_bit(&mut cc.bs, c);
                    }
                }
                for c in limit..SINGLE_BYTE_SIZE {
                    if is_single || enc.code_to_mbclen(c) == 1 {
                        bitset_set_bit(&mut cc.bs, c);
                    }
                }
                if ascii_mode && !is_single {
                    let r = set_all_multi_byte_range(enc, &mut cc.mbuf);
                    if r != 0 {
                        return r;
                    }
                }
            } else {
                for c in 0..limit {
                    if (is_single || enc.code_to_mbclen(c) == 1) && enc.is_code_ctype(c, ct) {
                        bitset_set_bit(&mut cc.bs, c);
                    }
                }
                if !ascii_mode && !is_single {
                    let r = set_all_multi_byte_range(enc, &mut cc.mbuf);
                    if r != 0 {
                        return r;
                    }
                }
            }
        }
        _ => return ONIGERR_PARSER_BUG,
    }
    0
}

const POSIX_BRACKETS: &[(&[u8], u32)] = &[
    (b"alnum", ONIGENC_CTYPE_ALNUM),
    (b"alpha", ONIGENC_CTYPE_ALPHA),
    (b"blank", ONIGENC_CTYPE_BLANK),
    (b"cntrl", ONIGENC_CTYPE_CNTRL),
    (b"digit", ONIGENC_CTYPE_DIGIT),
    (b"graph", ONIGENC_CTYPE_GRAPH),
    (b"lower", ONIGENC_CTYPE_LOWER),
    (b"print", ONIGENC_CTYPE_PRINT),
    (b"punct", ONIGENC_CTYPE_PUNCT),
    (b"space", ONIGENC_CTYPE_SPACE),
    (b"upper", ONIGENC_CTYPE_UPPER),
    (b"xdigit", ONIGENC_CTYPE_XDIGIT),
    (b"ascii", ONIGENC_CTYPE_ASCII),
    (b"word", ONIGENC_CTYPE_WORD),
];

impl<'a> ParseEnv<'a> {
    fn prs_posix_bracket(&mut self, cc: &mut CClass, src: &mut usize, end: usize) -> i32 {
        let mut p = *src;
        let not = if self.ppeek(p, end) == b'^' as u32 {
            self.pinc_s(&mut p);
            true
        } else {
            false
        };
        for &(name, ctype) in POSIX_BRACKETS {
            if with_ascii_strncmp(self.enc, self.pattern, p, end, name, name.len()) == 0 {
                let mut q = step(self.enc, self.pattern, p, end, name.len() as i32).unwrap_or(end);
                if with_ascii_strncmp(self.enc, self.pattern, q, end, b":]", 2) != 0 {
                    break;
                }
                let r = add_ctype_to_cc(cc, ctype as i32, not, self.options, self.enc);
                if r != 0 {
                    return r;
                }
                self.pinc_s(&mut q);
                self.pinc_s(&mut q);
                *src = q;
                return 0;
            }
        }
        ONIGERR_INVALID_POSIX_BRACKET_TYPE
    }

    fn fetch_char_property_to_ctype(&mut self, src: &mut usize, end: usize) -> i32 {
        let mut p = *src;
        let start = p;
        let mut r = ONIGERR_END_PATTERN_WITH_UNMATCHED_PARENTHESIS;
        while p < end {
            let prev = p;
            let c = self.pfetch_s(&mut p, end);
            if c == b'}' as u32 {
                r = self.enc.property_name_to_ctype(&self.pattern[start..prev]);
                if r >= 0 {
                    *src = p;
                } else {
                    self.set_error_string(*src, prev);
                }
                return r;
            } else if c == b'(' as u32 || c == b')' as u32 || c == b'{' as u32 || c == b'|' as u32 {
                break;
            }
        }
        r
    }

    fn prs_char_property(
        &mut self,
        np: &mut NodeRef,
        tok: &PToken,
        src: &mut usize,
        end: usize,
    ) -> i32 {
        let ctype = self.fetch_char_property_to_ctype(src, end);
        if ctype < 0 {
            return ctype;
        }
        if ctype == ONIGENC_CTYPE_WORD as i32 {
            *np = self.node_new_ctype(ctype, tok.prop_not, self.options);
            return 0;
        }
        *np = self.node_new_cclass();
        let mut cc = CClass::default();
        let r = add_ctype_to_cc(&mut cc, ctype, false, self.options, self.enc);
        if r != 0 {
            return r;
        }
        if tok.prop_not {
            cc.not = true;
        }
        *self.nodes.cclass_mut(*np) = cc;
        0
    }

    fn cc_cprop_next(
        &self,
        cc: &mut CClass,
        pcode: u32,
        val: &mut CVal,
        state: &mut CState,
    ) -> i32 {
        if *state == CState::Range {
            return ONIGERR_CHAR_CLASS_VALUE_AT_END_OF_RANGE;
        }
        if *state == CState::Value {
            if *val == CVal::Sb {
                bitset_set_bit(&mut cc.bs, pcode);
            } else if *val == CVal::Mb {
                let r = add_code_range(&mut cc.mbuf, self, pcode, pcode);
                if r < 0 {
                    return r;
                }
            }
        }
        *state = CState::Value;
        *val = CVal::CProp;
        0
    }

    fn cc_char_next(
        &self,
        cc: &mut CClass,
        from: &mut u32,
        to: u32,
        from_raw: &mut bool,
        to_raw: bool,
        intype: CVal,
        typ: &mut CVal,
        state: &mut CState,
    ) -> i32 {
        match *state {
            CState::Value => {
                if *typ == CVal::Sb {
                    if *from > 0xff {
                        return ONIGERR_INVALID_CODE_POINT_VALUE;
                    }
                    bitset_set_bit(&mut cc.bs, *from);
                } else if *typ == CVal::Mb {
                    let r = add_code_range(&mut cc.mbuf, self, *from, *from);
                    if r < 0 {
                        return r;
                    }
                }
            }
            CState::Range => {
                let mut skip = false;
                if intype == *typ {
                    if intype == CVal::Sb {
                        if *from > 0xff || to > 0xff {
                            return ONIGERR_INVALID_CODE_POINT_VALUE;
                        }
                        if *from > to {
                            if self.syntax.bv(ONIG_SYN_ALLOW_EMPTY_RANGE_IN_CC) {
                                skip = true;
                            } else {
                                return ONIGERR_EMPTY_RANGE_IN_CHAR_CLASS;
                            }
                        }
                        if !skip {
                            bitset_set_range(&mut cc.bs, *from, to);
                        }
                    } else {
                        let r = add_code_range(&mut cc.mbuf, self, *from, to);
                        if r < 0 {
                            return r;
                        }
                    }
                } else {
                    if *from > to {
                        if self.syntax.bv(ONIG_SYN_ALLOW_EMPTY_RANGE_IN_CC) {
                            skip = true;
                        } else {
                            return ONIGERR_EMPTY_RANGE_IN_CHAR_CLASS;
                        }
                    }
                    if !skip {
                        bitset_set_range(&mut cc.bs, *from, if to < 0xff { to } else { 0xff });
                        let r = add_code_range(&mut cc.mbuf, self, *from, to);
                        if r < 0 {
                            return r;
                        }
                    }
                }
                *state = CState::Complete;
            }
            CState::Complete | CState::Start => {
                *state = CState::Value;
            }
        }
        *from_raw = to_raw;
        *from = to;
        *typ = intype;
        0
    }

    fn code_exist_check(&self, c: u32, from: usize, end: usize, ignore_escaped: bool) -> bool {
        let mut in_esc = false;
        let mut p = from;
        while p < end {
            if ignore_escaped && in_esc {
                in_esc = false;
            } else {
                let code = self.pfetch_s(&mut p, end);
                if code == c {
                    return true;
                }
                if code == MC_ESC {
                    in_esc = true;
                }
            }
        }
        false
    }

    fn prs_cc(&mut self, np: &mut NodeRef, tok: &mut PToken, src: &mut usize, end: usize) -> i32 {
        *np = NULL_NODE;
        inc_parse_depth!(self, self.parse_depth);

        let mut state = CState::Start;
        let mut prev_cc: Option<CClass> = None;
        let mut r = self.fetch_token_cc(tok, src, end, state);
        let neg;
        if r == TK_CHAR && tok.code == b'^' as u32 && !tok.escaped {
            neg = true;
            r = self.fetch_token_cc(tok, src, end, state);
        } else {
            neg = false;
        }

        if r < 0 {
            return r;
        }
        if r == TK_CC_CLOSE {
            if !self.code_exist_check(b']' as u32, *src, self.end(), true) {
                return ONIGERR_EMPTY_CHAR_CLASS;
            }
            tok.typ = TK_CHAR;
            r = TK_CHAR;
        }

        let node = self.node_new_cclass();
        *np = node;
        let mut cc = CClass::default();

        let mut and_start = false;
        let mut curr_type = CVal::Undef;
        let mut curr_code: u32 = 0;
        let mut curr_raw = false;
        let mut in_code: u32;
        let mut in_raw: bool;
        let mut in_type: CVal;

        let mut p = *src;
        while r != TK_CC_CLOSE {
            let mut fetched = false;
            // a small state machine for the C gotos
            #[derive(PartialEq)]
            enum Go {
                AnyCharIn,
                ValEntry,
                ValEntry2,
                NextCProp,
                None,
            }
            let mut go = Go::None;
            in_code = 0;
            in_raw = false;
            in_type = CVal::Undef;

            match r {
                TK_CHAR => go = Go::AnyCharIn,
                TK_CRUDE_BYTE => {
                    if !self.enc.is_singlebyte() && tok.base_num != 0 {
                        let mut buf = [0u8; ONIGENC_CODE_TO_MBC_MAXLEN];
                        let psave = p;
                        let base_num = tok.base_num;
                        buf[0] = tok.byte;
                        let mut i = 1;
                        while i < self.enc.max_enc_len {
                            r = self.fetch_token_cc(tok, &mut p, end, CState::Complete);
                            if r < 0 {
                                return r;
                            }
                            if r != TK_CRUDE_BYTE || tok.base_num != base_num {
                                fetched = true;
                                break;
                            }
                            buf[i] = tok.byte;
                            i += 1;
                        }
                        if i < self.enc.min_enc_len {
                            return ONIGERR_TOO_SHORT_MULTI_BYTE_STRING;
                        }
                        for b in buf.iter_mut().skip(i) {
                            *b = 0;
                        }
                        let len = self.enc.mbc_enc_len(&buf, 0);
                        if i < len {
                            return ONIGERR_TOO_SHORT_MULTI_BYTE_STRING;
                        } else if i > len {
                            p = psave;
                            for _ in 1..len {
                                r = self.fetch_token_cc(tok, &mut p, end, CState::Complete);
                                if r < 0 {
                                    return r;
                                }
                            }
                            fetched = false;
                        }
                        /* the C re-uses i as the fetch-back loop counter, so
                        "i == 1" there means len == 1 */
                        if len == 1 {
                            in_code = buf[0] as u32;
                            in_type = CVal::Sb;
                        } else {
                            if !self.enc.is_valid_mbc_string(&buf, 0, len) {
                                return ONIGERR_INVALID_WIDE_CHAR_VALUE;
                            }
                            in_code = self.enc.mbc_to_code(&buf, 0, buf.len());
                            in_type = CVal::Mb;
                        }
                    } else {
                        in_code = tok.byte as u32;
                        in_type = CVal::Sb;
                    }
                    in_raw = true;
                    go = Go::ValEntry2;
                }
                TK_CODE_POINT => {
                    in_code = tok.code;
                    in_raw = true;
                    go = Go::ValEntry;
                }
                TK_CC_POSIX_BRACKET_OPEN => {
                    let r2 = self.prs_posix_bracket(&mut cc, &mut p, end);
                    if r2 < 0 {
                        return r2;
                    }
                    if r2 == 1 {
                        p = tok.backp;
                        in_code = tok.code;
                        in_raw = false;
                        go = Go::ValEntry;
                    } else {
                        go = Go::NextCProp;
                    }
                }
                TK_CHAR_TYPE => {
                    let r2 = add_ctype_to_cc(
                        &mut cc,
                        tok.prop_ctype,
                        tok.prop_not,
                        self.options,
                        self.enc,
                    );
                    if r2 != 0 {
                        return r2;
                    }
                    go = Go::NextCProp;
                }
                TK_CHAR_PROPERTY => {
                    let ctype = self.fetch_char_property_to_ctype(&mut p, end);
                    if ctype < 0 {
                        return ctype;
                    }
                    let r2 = add_ctype_to_cc(&mut cc, ctype, tok.prop_not, self.options, self.enc);
                    if r2 != 0 {
                        return r2;
                    }
                    go = Go::NextCProp;
                }
                TK_CC_RANGE => {
                    if state == CState::Value {
                        r = self.fetch_token_cc(tok, &mut p, end, CState::Range);
                        if r < 0 {
                            return r;
                        }
                        fetched = true;
                        if r == TK_CC_CLOSE || r == TK_CC_AND {
                            in_code = b'-' as u32;
                            in_raw = false;
                            go = Go::ValEntry;
                        } else if curr_type == CVal::CProp {
                            if self
                                .syntax
                                .bv(ONIG_SYN_ALLOW_CHAR_TYPE_FOLLOWED_BY_MINUS_IN_CC)
                            {
                                in_code = b'-' as u32;
                                in_raw = false;
                                go = Go::ValEntry;
                            } else {
                                return ONIGERR_UNMATCHED_RANGE_SPECIFIER_IN_CHAR_CLASS;
                            }
                        } else {
                            state = CState::Range;
                        }
                    } else if state == CState::Start {
                        in_code = tok.code;
                        in_raw = false;
                        r = self.fetch_token_cc(tok, &mut p, end, CState::Value);
                        if r < 0 {
                            return r;
                        }
                        fetched = true;
                        let _ = and_start;
                        go = Go::ValEntry;
                    } else if state == CState::Range {
                        go = Go::AnyCharIn;
                    } else {
                        /* CS_COMPLETE */
                        r = self.fetch_token_cc(tok, &mut p, end, CState::Value);
                        if r < 0 {
                            return r;
                        }
                        fetched = true;
                        if r == TK_CC_CLOSE || r == TK_CC_AND {
                            in_code = b'-' as u32;
                            in_raw = false;
                            go = Go::ValEntry;
                        } else if self.syntax.bv(ONIG_SYN_ALLOW_DOUBLE_RANGE_OP_IN_CC) {
                            in_code = b'-' as u32;
                            in_raw = false;
                            go = Go::ValEntry;
                        } else {
                            return ONIGERR_UNMATCHED_RANGE_SPECIFIER_IN_CHAR_CLASS;
                        }
                    }
                }
                TK_CC_OPEN_CC => {
                    if state == CState::Value {
                        let r2 = self.cc_char_next(
                            &mut cc,
                            &mut curr_code,
                            0,
                            &mut curr_raw,
                            false,
                            curr_type,
                            &mut curr_type,
                            &mut state,
                        );
                        if r2 != 0 {
                            return r2;
                        }
                    }
                    state = CState::Complete;
                    let mut anode = NULL_NODE;
                    let r2 = self.prs_cc(&mut anode, tok, &mut p, end);
                    if r2 != 0 {
                        return r2;
                    }
                    let acc = self.nodes.cclass(anode).clone();
                    let r2 = or_cclass(&mut cc, &acc, self.enc);
                    if r2 != 0 {
                        return r2;
                    }
                }
                TK_CC_AND => {
                    if state == CState::Value {
                        let r2 = self.cc_char_next(
                            &mut cc,
                            &mut curr_code,
                            0,
                            &mut curr_raw,
                            false,
                            curr_type,
                            &mut curr_type,
                            &mut state,
                        );
                        if r2 != 0 {
                            return r2;
                        }
                    }
                    and_start = true;
                    state = CState::Start;
                    match prev_cc.as_mut() {
                        Some(pc) => {
                            let r2 = and_cclass(pc, &cc, self.enc);
                            if r2 != 0 {
                                return r2;
                            }
                        }
                        None => {
                            prev_cc = Some(std::mem::take(&mut cc));
                        }
                    }
                    cc = CClass::default();
                }
                TK_EOT => return ONIGERR_PREMATURE_END_OF_CHAR_CLASS,
                _ => return ONIGERR_PARSER_BUG,
            }

            if go == Go::AnyCharIn {
                let len = self.enc.code_to_mbclen(tok.code);
                if len < 0 {
                    return len;
                }
                in_type = if len == 1 { CVal::Sb } else { CVal::Mb };
                in_code = tok.code;
                in_raw = false;
                go = Go::ValEntry2;
            }
            if go == Go::ValEntry {
                let len = self.enc.code_to_mbclen(in_code);
                if len < 0
                    && (state != CState::Range
                        || !self
                            .syntax
                            .bv(ONIG_SYN_ALLOW_INVALID_CODE_END_OF_RANGE_IN_CC)
                        || in_code < 0x100
                        || self.enc.max_enc_len == 1)
                {
                    return len;
                }
                in_type = if len == 1 { CVal::Sb } else { CVal::Mb };
                go = Go::ValEntry2;
            }
            if go == Go::ValEntry2 {
                let r2 = self.cc_char_next(
                    &mut cc,
                    &mut curr_code,
                    in_code,
                    &mut curr_raw,
                    in_raw,
                    in_type,
                    &mut curr_type,
                    &mut state,
                );
                if r2 != 0 {
                    return r2;
                }
            }
            if go == Go::NextCProp {
                let r2 = self.cc_cprop_next(&mut cc, curr_code, &mut curr_type, &mut state);
                if r2 != 0 {
                    return r2;
                }
            }

            if fetched {
                r = tok.typ;
            } else {
                r = self.fetch_token_cc(tok, &mut p, end, state);
                if r < 0 {
                    return r;
                }
            }
        }

        if state == CState::Value {
            let r2 = self.cc_char_next(
                &mut cc,
                &mut curr_code,
                0,
                &mut curr_raw,
                false,
                curr_type,
                &mut curr_type,
                &mut state,
            );
            if r2 != 0 {
                return r2;
            }
        }

        if let Some(mut pc) = prev_cc {
            let r2 = and_cclass(&mut pc, &cc, self.enc);
            if r2 != 0 {
                return r2;
            }
            cc = pc;
        }

        cc.not = neg;
        if cc.not && self.syntax.bv(ONIG_SYN_NOT_NEWLINE_IN_NEGATIVE_CC) {
            let mut is_empty = cc.mbuf.is_none();
            if is_empty {
                is_empty = bitset_is_empty(&cc.bs);
            }
            if !is_empty && self.enc.is_code_newline(NEWLINE_CODE) {
                if self.enc.code_to_mbclen(NEWLINE_CODE) == 1 {
                    bitset_set_bit(&mut cc.bs, NEWLINE_CODE);
                } else {
                    add_code_range(&mut cc.mbuf, self, NEWLINE_CODE, NEWLINE_CODE);
                }
            }
        }
        *self.nodes.cclass_mut(node) = cc;
        *src = p;
        self.parse_depth -= 1;
        0
    }
}

/* ===== quantifier reduction ===== */

/// ?:0, *:1, +:2, ??:3, *?:4, +?:5
fn quantifier_type_num(lower: i32, upper: i32, greedy: bool) -> i32 {
    if greedy {
        if lower == 0 {
            if upper == 1 {
                return 0;
            } else if is_infinite_repeat(upper) {
                return 1;
            }
        } else if lower == 1 && is_infinite_repeat(upper) {
            return 2;
        }
    } else {
        if lower == 0 {
            if upper == 1 {
                return 3;
            } else if is_infinite_repeat(upper) {
                return 4;
            }
        } else if lower == 1 && is_infinite_repeat(upper) {
            return 5;
        }
    }
    -1
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReduceType {
    Asis,
    Del,
    A,
    P,
    AQ,
    QQ,
    PQQ,
}

use ReduceType as RQ;
const REDUCE_TYPE_TABLE: [[ReduceType; 6]; 6] = [
    [RQ::Del, RQ::A, RQ::A, RQ::QQ, RQ::AQ, RQ::Asis],
    [RQ::Del, RQ::Del, RQ::Del, RQ::PQQ, RQ::PQQ, RQ::Del],
    [RQ::A, RQ::A, RQ::Del, RQ::Asis, RQ::PQQ, RQ::Del],
    [RQ::Del, RQ::AQ, RQ::AQ, RQ::Del, RQ::AQ, RQ::AQ],
    [RQ::Del, RQ::Del, RQ::Del, RQ::Del, RQ::Del, RQ::Del],
    [RQ::Asis, RQ::A, RQ::P, RQ::AQ, RQ::AQ, RQ::Del],
];

pub fn quant_params(nodes: &Nodes, n: NodeRef) -> (i32, i32, bool) {
    match &nodes.v[n].kind {
        NodeKind::Quant {
            lower,
            upper,
            greedy,
            ..
        } => (*lower, *upper, *greedy),
        _ => panic!("not a quantifier"),
    }
}

pub fn set_quant_params(nodes: &mut Nodes, n: NodeRef, l: i32, u: i32, g: bool) {
    if let NodeKind::Quant {
        lower,
        upper,
        greedy,
        ..
    } = &mut nodes.v[n].kind
    {
        *lower = l;
        *upper = u;
        *greedy = g;
    }
}

/// onig_reduce_nested_quantifier
pub fn reduce_nested_quantifier(nodes: &mut Nodes, pnode: NodeRef) -> i32 {
    let cnode = nodes.body(pnode);
    let (pl, pu, pg) = quant_params(nodes, pnode);
    let (cl, cu, cg) = quant_params(nodes, cnode);
    let pnum = quantifier_type_num(pl, pu, pg);
    let cnum = quantifier_type_num(cl, cu, cg);
    if pnum < 0 || cnum < 0 {
        if pl == pu && cl == cu {
            let n = positive_int_multiply(pl, cl);
            if n < 0 {
                return ONIGERR_TOO_BIG_NUMBER_FOR_REPEAT_RANGE;
            }
            set_quant_params(nodes, pnode, n, n, pg);
            let cb = nodes.body(cnode);
            nodes.set_body(pnode, cb);
        }
        return 0;
    }

    match REDUCE_TYPE_TABLE[cnum as usize][pnum as usize] {
        RQ::Del => {
            nodes.copy_into(pnode, cnode);
        }
        RQ::A => {
            let cb = nodes.body(cnode);
            nodes.set_body(pnode, cb);
            set_quant_params(nodes, pnode, 0, INFINITE_REPEAT, true);
        }
        RQ::P => {
            let cb = nodes.body(cnode);
            nodes.set_body(pnode, cb);
            set_quant_params(nodes, pnode, 1, INFINITE_REPEAT, true);
        }
        RQ::AQ => {
            let cb = nodes.body(cnode);
            nodes.set_body(pnode, cb);
            set_quant_params(nodes, pnode, 0, INFINITE_REPEAT, false);
        }
        RQ::QQ => {
            let cb = nodes.body(cnode);
            nodes.set_body(pnode, cb);
            set_quant_params(nodes, pnode, 0, 1, false);
        }
        RQ::PQQ => {
            set_quant_params(nodes, pnode, 0, 1, false);
            set_quant_params(nodes, cnode, 1, INFINITE_REPEAT, true);
        }
        RQ::Asis => {}
    }
    0
}

pub fn positive_int_multiply(x: i32, y: i32) -> i32 {
    if x == 0 || y == 0 {
        return 0;
    }
    if x < ONIG_INT_MAX / y { x * y } else { -1 }
}

fn is_invalid_quantifier_target(nodes: &Nodes, node: NodeRef) -> bool {
    match nodes.typ(node) {
        NodeType::Anchor | NodeType::Gimmick => true,
        NodeType::Bag => false,
        NodeType::List => {
            let mut n = node;
            loop {
                if !is_invalid_quantifier_target(nodes, nodes.car(n)) {
                    return false;
                }
                n = nodes.cdr(n);
                if n == NULL_NODE {
                    break;
                }
            }
            false
        }
        NodeType::Alt => {
            let mut n = node;
            loop {
                if is_invalid_quantifier_target(nodes, nodes.car(n)) {
                    return true;
                }
                n = nodes.cdr(n);
                if n == NULL_NODE {
                    break;
                }
            }
            false
        }
        _ => false,
    }
}

impl<'a> ParseEnv<'a> {
    fn node_new_general_newline(&mut self) -> NodeRef {
        let mut buf = [0u8; ONIGENC_CODE_TO_MBC_MAXLEN];
        let mut bytes = Vec::new();
        let dlen = self.enc.code_to_mbc(0x0d, &mut buf);
        bytes.extend_from_slice(&buf[..dlen.max(0) as usize]);
        let alen = self.enc.code_to_mbc(NEWLINE_CODE, &mut buf);
        bytes.extend_from_slice(&buf[..alen.max(0) as usize]);
        let crnl = self.node_new_str_crude(&bytes, ONIG_OPTION_NONE);
        let ncc = self.node_new_cclass();
        let mut cc = CClass::default();
        if dlen == 1 {
            bitset_set_range(&mut cc.bs, NEWLINE_CODE, 0x0d);
        } else {
            add_code_range(&mut cc.mbuf, self, NEWLINE_CODE, 0x0d);
        }
        if self.enc.is_unicode() {
            add_code_range(&mut cc.mbuf, self, 0x85, 0x85);
            add_code_range(&mut cc.mbuf, self, 0x2028, 0x2029);
        }
        *self.nodes.cclass_mut(ncc) = cc;
        self.node_new_bag_if_else(crnl, NULL_NODE, ncc)
    }

    /* ===== names ===== */

    fn name_add(&mut self, name: usize, name_end: usize, backref: i32) -> i32 {
        if name_end <= name {
            return ONIGERR_EMPTY_GROUP_NAME;
        }
        let nm = self.pattern_slice(name, name_end);
        let idx = match self.names.find(&nm) {
            Some(i) => i,
            None => {
                self.names.entries.push(NameEntry {
                    name: nm,
                    back_refs: Vec::new(),
                });
                self.names.entries.len() - 1
            }
        };
        if !self.names.entries[idx].back_refs.is_empty()
            && !self.syntax.bv(ONIG_SYN_ALLOW_MULTIPLEX_DEFINITION_NAME)
        {
            self.set_error_string(name, name_end);
            return ONIGERR_MULTIPLEX_DEFINED_NAME;
        }
        self.names.entries[idx].back_refs.push(backref);
        0
    }

    fn name_to_group_numbers(&mut self, name: usize, name_end: usize) -> Result<Vec<i32>, i32> {
        let nm = &self.pattern[name..name_end];
        match self.names.find(nm) {
            None => {
                self.set_error_string(name, name_end);
                Err(ONIGERR_UNDEFINED_NAME_REFERENCE)
            }
            Some(i) => Ok(self.names.entries[i].back_refs.clone()),
        }
    }

    /* ===== callouts ===== */

    fn is_allowed_callout_name(&self, name: usize, name_end: usize) -> bool {
        if name >= name_end {
            return false;
        }
        let mut p = name;
        while p < name_end {
            let c = self.enc.mbc_to_code(self.pattern, p, name_end);
            let ok = (b'A' as u32..=b'Z' as u32).contains(&c)
                || (b'a' as u32..=b'z' as u32).contains(&c)
                || (b'0' as u32..=b'9' as u32).contains(&c)
                || c == b'_' as u32;
            if !ok {
                return false;
            }
            if p == name && (b'0' as u32..=b'9' as u32).contains(&c) {
                return false;
            }
            p += self.enclen(p);
        }
        true
    }

    fn is_allowed_callout_tag_name(&self, name: usize, name_end: usize) -> bool {
        self.is_allowed_callout_name(name, name_end)
    }

    fn callout_tag_entry(&mut self, tag_start: usize, tag_end: usize) -> i32 {
        if tag_end <= tag_start {
            return ONIGERR_INVALID_CALLOUT_TAG_NAME;
        }
        let tag = self.pattern_slice(tag_start, tag_end);
        if self.callout_tags.contains(&tag) {
            self.set_error_string(tag_start, tag_end);
            return ONIGERR_MULTIPLEX_DEFINED_NAME;
        }
        self.callout_tags.push(tag.clone());
        if let Some(e) = self.callout_list.last_mut() {
            e.tag = Some(tag);
        }
        ONIG_NORMAL
    }

    /// (?{...}[tag][+-]) (?{{...}}[tag][+-])
    fn prs_callout_of_contents(
        &mut self,
        np: &mut NodeRef,
        cterm: u32,
        src: &mut usize,
        end: usize,
    ) -> i32 {
        let mut p = *src;
        if p >= end {
            return ONIGERR_INVALID_CALLOUT_PATTERN;
        }
        let mut brace_nest = 0;
        while self.ppeek(p, end) == b'{' as u32 {
            brace_nest += 1;
            self.pinc_s(&mut p);
            if p >= end {
                return ONIGERR_INVALID_CALLOUT_PATTERN;
            }
        }
        loop {
            if p >= end {
                return ONIGERR_INVALID_CALLOUT_PATTERN;
            }
            let mut c = self.pfetch_s(&mut p, end);
            if c == b'}' as u32 {
                let mut i = brace_nest;
                while i > 0 {
                    if p >= end {
                        return ONIGERR_INVALID_CALLOUT_PATTERN;
                    }
                    c = self.pfetch_s(&mut p, end);
                    if c == b'}' as u32 {
                        i -= 1;
                    } else {
                        break;
                    }
                }
                if i == 0 {
                    break;
                }
            }
        }
        if p >= end {
            return ONIGERR_END_PATTERN_IN_GROUP;
        }
        let mut c = self.pfetch_s(&mut p, end);
        let mut tag: Option<(usize, usize)> = None;
        if c == b'[' as u32 {
            if p >= end {
                return ONIGERR_END_PATTERN_IN_GROUP;
            }
            let tag_start = p;
            let mut tag_end = p;
            while p < end {
                tag_end = p;
                c = self.pfetch_s(&mut p, end);
                if c == b']' as u32 {
                    break;
                }
            }
            if !self.is_allowed_callout_tag_name(tag_start, tag_end) {
                return ONIGERR_INVALID_CALLOUT_TAG_NAME;
            }
            if p >= end {
                return ONIGERR_END_PATTERN_IN_GROUP;
            }
            c = self.pfetch_s(&mut p, end);
            tag = Some((tag_start, tag_end));
        }
        if c == b'X' as u32 || c == b'<' as u32 || c == b'>' as u32 {
            if p >= end {
                return ONIGERR_END_PATTERN_IN_GROUP;
            }
            c = self.pfetch_s(&mut p, end);
        }
        if c != cterm {
            return ONIGERR_INVALID_CALLOUT_PATTERN;
        }
        self.callout_list.push(CalloutListEntry {
            of_contents: true,
            tag: None,
        });
        let num = self.callout_list.len() as i32;
        if let Some((s, e)) = tag {
            if s != e {
                let r = self.callout_tag_entry(s, e);
                if r != ONIG_NORMAL {
                    return r;
                }
            }
        }
        *np = self.node_new_callout(true, num, -1);
        *src = p;
        0
    }

    /// (*name[TAG]) (*name[TAG]{a,b,..}): no callout names are registered
    /// in PHP, so a valid name always ends in "undefined callout name".
    fn prs_callout_of_name(&mut self, cterm: u32, src: &mut usize, end: usize) -> i32 {
        let mut p = *src;
        if p >= end {
            return ONIGERR_INVALID_CALLOUT_PATTERN;
        }
        let name_start = p;
        let mut name_end;
        let mut c;
        loop {
            if p >= end {
                return ONIGERR_END_PATTERN_IN_GROUP;
            }
            name_end = p;
            c = self.pfetch_s(&mut p, end);
            if c == cterm || c == b'[' as u32 || c == b'{' as u32 {
                break;
            }
        }
        if !self.is_allowed_callout_name(name_start, name_end) {
            return ONIGERR_INVALID_CALLOUT_NAME;
        }
        if c == b'[' as u32 {
            if p >= end {
                return ONIGERR_END_PATTERN_IN_GROUP;
            }
            let tag_start = p;
            let mut tag_end = p;
            while p < end {
                tag_end = p;
                c = self.pfetch_s(&mut p, end);
                if c == b']' as u32 {
                    break;
                }
            }
            if !self.is_allowed_callout_tag_name(tag_start, tag_end) {
                return ONIGERR_INVALID_CALLOUT_TAG_NAME;
            }
            if p >= end {
                return ONIGERR_END_PATTERN_IN_GROUP;
            }
            c = self.pfetch_s(&mut p, end);
        }
        if c == b'{' as u32 {
            if p >= end {
                return ONIGERR_END_PATTERN_IN_GROUP;
            }
            let r = self.prs_callout_args_skip(b'}' as u32, &mut p, end);
            if r < 0 {
                return r;
            }
        }
        ONIGERR_UNDEFINED_CALLOUT_NAME
    }

    /// prs_callout_args in skip mode (only syntax checks)
    fn prs_callout_args_skip(&mut self, cterm: u32, src: &mut usize, end: usize) -> i32 {
        const ONIG_CALLOUT_MAX_ARGS_NUM: i32 = 4;
        let mut p = *src;
        if p >= end {
            return ONIGERR_INVALID_CALLOUT_PATTERN;
        }
        let mut c = 0u32;
        let mut n = 0;
        while n < ONIG_CALLOUT_MAX_ARGS_NUM {
            let mut cn = 0;
            let mut esc = false;
            loop {
                if p >= end {
                    return ONIGERR_INVALID_CALLOUT_PATTERN;
                }
                c = self.pfetch_s(&mut p, end);
                if esc {
                    esc = false;
                    cn += 1;
                } else if c == b'\\' as u32 {
                    esc = true;
                } else if c == cterm || c == b',' as u32 {
                    break;
                } else {
                    cn += 1;
                }
            }
            if cn != 0 {
                n += 1;
            }
            if c == cterm {
                break;
            }
        }
        if c != cterm {
            return ONIGERR_INVALID_CALLOUT_PATTERN;
        }
        *src = p;
        n
    }
}

impl<'a> ParseEnv<'a> {
    fn set_whole_options(&mut self, option: u32) -> i32 {
        if self.flags & PE_FLAG_HAS_WHOLE_OPTIONS != 0 {
            return ONIGERR_INVALID_GROUP_OPTION;
        }
        self.flags |= PE_FLAG_HAS_WHOLE_OPTIONS;
        if opton_dont_capture_group(option) {
            self.reg_options |= ONIG_OPTION_DONT_CAPTURE_GROUP;
            if option & (ONIG_OPTION_DONT_CAPTURE_GROUP | ONIG_OPTION_CAPTURE_GROUP)
                == (ONIG_OPTION_DONT_CAPTURE_GROUP | ONIG_OPTION_CAPTURE_GROUP)
            {
                return ONIGERR_INVALID_COMBINATION_OF_OPTIONS;
            }
        }
        if option & ONIG_OPTION_IGNORECASE_IS_ASCII != 0 {
            self.reg_case_fold_flag &=
                !(INTERNAL_ONIGENC_CASE_FOLD_MULTI_CHAR | ONIGENC_CASE_FOLD_TURKISH_AZERI);
            self.reg_case_fold_flag |= ONIGENC_CASE_FOLD_ASCII_ONLY;
            self.reg_options |= ONIG_OPTION_IGNORECASE_IS_ASCII;
        }
        if opton_find_longest(option) {
            self.reg_options |= ONIG_OPTION_FIND_LONGEST;
        }
        0
    }

    /// returns 0 (node), 1 (group), 2 (option only) or an error
    fn prs_bag(
        &mut self,
        np: &mut NodeRef,
        tok: &mut PToken,
        term: i32,
        src: &mut usize,
        end: usize,
    ) -> i32 {
        let mut p = *src;
        let mut prev = p;
        *np = NULL_NODE;
        if p >= end {
            return ONIGERR_END_PATTERN_WITH_UNMATCHED_PARENTHESIS;
        }

        let mut option = self.options;
        let mut c = self.ppeek(p, end);
        if c == b'?' as u32 && self.syntax.op2(ONIG_SYN_OP2_QMARK_GROUP_EFFECT) {
            self.pinc(&mut p, &mut prev);
            if p >= end {
                return ONIGERR_END_PATTERN_IN_GROUP;
            }
            c = self.pfetch(&mut p, &mut prev, end);
            let ch = char::from_u32(c).unwrap_or('\0');
            let mut named_group = false;
            let mut options_start = false;
            match ch {
                ':' => {
                    // group:
                    let r = self.fetch_token(tok, &mut p, end);
                    if r < 0 {
                        return r;
                    }
                    let r = self.prs_alts(np, tok, term, &mut p, end, false);
                    if r < 0 {
                        return r;
                    }
                    *src = p;
                    return 1;
                }
                '=' => *np = self.node_new_anchor(ANCR_PREC_READ),
                '!' => *np = self.node_new_anchor(ANCR_PREC_READ_NOT),
                '>' => *np = self.node_new_bag(BagType::StopBacktrack),
                '\'' => {
                    if self.syntax.op2(ONIG_SYN_OP2_QMARK_LT_NAMED_GROUP) {
                        named_group = true;
                    } else {
                        return ONIGERR_UNDEFINED_GROUP_OPTION;
                    }
                }
                '<' => {
                    if p >= end {
                        return ONIGERR_END_PATTERN_WITH_UNMATCHED_PARENTHESIS;
                    }
                    c = self.pfetch(&mut p, &mut prev, end);
                    if c == b'=' as u32 {
                        *np = self.node_new_anchor(ANCR_LOOK_BEHIND);
                    } else if c == b'!' as u32 {
                        *np = self.node_new_anchor(ANCR_LOOK_BEHIND_NOT);
                    } else if self.syntax.op2(ONIG_SYN_OP2_QMARK_LT_NAMED_GROUP) {
                        p = prev;
                        c = b'<' as u32;
                        named_group = true;
                    } else {
                        return ONIGERR_UNDEFINED_GROUP_OPTION;
                    }
                }
                '~' => {
                    if !self.syntax.op2(ONIG_SYN_OP2_QMARK_TILDE_ABSENT_GROUP) {
                        return ONIGERR_UNDEFINED_GROUP_OPTION;
                    }
                    if p >= end {
                        return ONIGERR_END_PATTERN_IN_GROUP;
                    }
                    let head_bar;
                    if self.ppeek(p, end) == b'|' as u32 {
                        self.pinc(&mut p, &mut prev);
                        if p >= end {
                            return ONIGERR_END_PATTERN_IN_GROUP;
                        }
                        head_bar = true;
                        if self.ppeek(p, end) == b')' as u32 {
                            self.pinc(&mut p, &mut prev);
                            *np = self.make_range_clear();
                            self.flags |= PE_FLAG_HAS_ABSENT_STOPPER;
                            *src = p;
                            return 0;
                        }
                    } else {
                        head_bar = false;
                    }
                    let r = self.fetch_token(tok, &mut p, end);
                    if r < 0 {
                        return r;
                    }
                    let mut absent = NULL_NODE;
                    let r = self.prs_alts(&mut absent, tok, term, &mut p, end, true);
                    if r < 0 {
                        return r;
                    }
                    let mut expr = NULL_NODE;
                    let mut is_range_cutter = false;
                    if head_bar {
                        let top = absent;
                        if self.nodes.typ(top) != NodeType::Alt || self.nodes.cdr(top) == NULL_NODE
                        {
                            expr = NULL_NODE;
                            is_range_cutter = true;
                            self.flags |= PE_FLAG_HAS_ABSENT_STOPPER;
                        } else {
                            absent = self.nodes.car(top);
                            expr = self.nodes.cdr(top);
                            if self.nodes.cdr(expr) == NULL_NODE {
                                expr = self.nodes.car(expr);
                            }
                        }
                    }
                    *np = self.make_absent_tree(absent, expr, is_range_cutter);
                    *src = p;
                    return 0;
                }
                '{' => {
                    if !self.syntax.op2(ONIG_SYN_OP2_QMARK_BRACE_CALLOUT_CONTENTS) {
                        return ONIGERR_UNDEFINED_GROUP_OPTION;
                    }
                    let r = self.prs_callout_of_contents(np, b')' as u32, &mut p, end);
                    if r != 0 {
                        return r;
                    }
                    *src = p;
                    return 0;
                }
                '(' => {
                    if !self.syntax.op2(ONIG_SYN_OP2_QMARK_LPAREN_IF_ELSE) {
                        return ONIGERR_UNDEFINED_GROUP_OPTION;
                    }
                    return self.prs_if_else(np, tok, term, src, p, prev, end);
                }
                '@' => {
                    if !self.syntax.op2(ONIG_SYN_OP2_ATMARK_CAPTURE_HISTORY) {
                        return ONIGERR_UNDEFINED_GROUP_OPTION;
                    }
                    /* never enabled by the syntaxes PHP uses */
                    return ONIGERR_UNDEFINED_GROUP_OPTION;
                }
                'C' | 'I' | 'L' => {
                    if !self.syntax.bv(ONIG_SYN_WHOLE_OPTIONS) {
                        return ONIGERR_UNDEFINED_GROUP_OPTION;
                    }
                    options_start = true;
                }
                'P' => {
                    if self.syntax.op2(ONIG_SYN_OP2_QMARK_CAPITAL_P_NAME) {
                        if p >= end {
                            return ONIGERR_END_PATTERN_IN_GROUP;
                        }
                        c = self.pfetch(&mut p, &mut prev, end);
                        if c == b'<' as u32 {
                            named_group = true;
                        } else {
                            return ONIGERR_UNDEFINED_GROUP_OPTION;
                        }
                    } else {
                        if !self.syntax.op2(ONIG_SYN_OP2_OPTION_ONIGURUMA) {
                            return ONIGERR_UNDEFINED_GROUP_OPTION;
                        }
                        options_start = true;
                    }
                }
                'W' | 'D' | 'S' | 'y' => {
                    if !self.syntax.op2(ONIG_SYN_OP2_OPTION_ONIGURUMA) {
                        return ONIGERR_UNDEFINED_GROUP_OPTION;
                    }
                    options_start = true;
                }
                'a' | '-' | 'i' | 'm' | 's' | 'x' => {
                    options_start = true;
                }
                _ => return ONIGERR_UNDEFINED_GROUP_OPTION,
            }

            if named_group {
                // named_group1:
                let name = p;
                let mut name_end = 0usize;
                let mut num = 0;
                let mut num_type = RefNum::IsNotNum;
                let r = self.fetch_name(
                    c,
                    &mut p,
                    end,
                    &mut name_end,
                    &mut num,
                    &mut num_type,
                    false,
                );
                if r < 0 {
                    return r;
                }
                let num = self.scan_env_add_mem_entry();
                if num < 0 {
                    return num;
                }
                let r = self.name_add(name, name_end, num);
                if r != 0 {
                    return r;
                }
                *np = self.node_new_memory(true);
                self.set_bag_regnum(*np, num);
                self.num_named += 1;
            }

            if options_start {
                let mut neg = false;
                let mut whole_options = false;
                loop {
                    match char::from_u32(c).unwrap_or('\0') {
                        ':' | ')' => {}
                        '-' => neg = true,
                        'x' => option_negate(&mut option, ONIG_OPTION_EXTEND, neg),
                        'i' => option_negate(&mut option, ONIG_OPTION_IGNORECASE, neg),
                        's' => {
                            if self.syntax.op2(ONIG_SYN_OP2_OPTION_PERL) {
                                option_negate(&mut option, ONIG_OPTION_MULTILINE, neg);
                            } else {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                        }
                        'm' => {
                            if self.syntax.op2(ONIG_SYN_OP2_OPTION_PERL) {
                                option_negate(&mut option, ONIG_OPTION_SINGLELINE, !neg);
                            } else if self
                                .syntax
                                .op2(ONIG_SYN_OP2_OPTION_ONIGURUMA | ONIG_SYN_OP2_OPTION_RUBY)
                            {
                                option_negate(&mut option, ONIG_OPTION_MULTILINE, neg);
                            } else {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                        }
                        'W' => {
                            if !self.syntax.op2(ONIG_SYN_OP2_OPTION_ONIGURUMA) {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                            option_negate(&mut option, ONIG_OPTION_WORD_IS_ASCII, neg);
                        }
                        'D' => {
                            if !self.syntax.op2(ONIG_SYN_OP2_OPTION_ONIGURUMA) {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                            option_negate(&mut option, ONIG_OPTION_DIGIT_IS_ASCII, neg);
                        }
                        'S' => {
                            if !self.syntax.op2(ONIG_SYN_OP2_OPTION_ONIGURUMA) {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                            option_negate(&mut option, ONIG_OPTION_SPACE_IS_ASCII, neg);
                        }
                        'P' => {
                            if !self.syntax.op2(ONIG_SYN_OP2_OPTION_ONIGURUMA) {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                            option_negate(&mut option, ONIG_OPTION_POSIX_IS_ASCII, neg);
                        }
                        'y' => {
                            if !self.syntax.op2(ONIG_SYN_OP2_OPTION_ONIGURUMA) {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                            if neg {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                            if p >= end {
                                return ONIGERR_END_PATTERN_IN_GROUP;
                            }
                            if self.ppeek(p, end) != b'{' as u32 {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                            self.pfetch(&mut p, &mut prev, end);
                            if p >= end {
                                return ONIGERR_END_PATTERN_IN_GROUP;
                            }
                            let c2 = self.pfetch(&mut p, &mut prev, end);
                            if c2 == b'g' as u32 {
                                if !self.enc.is_unicode() {
                                    return ONIGERR_UNDEFINED_GROUP_OPTION;
                                }
                                option_negate(
                                    &mut option,
                                    ONIG_OPTION_TEXT_SEGMENT_EXTENDED_GRAPHEME_CLUSTER,
                                    false,
                                );
                                option_negate(&mut option, ONIG_OPTION_TEXT_SEGMENT_WORD, true);
                            } else if c2 == b'w' as u32 {
                                if !self.enc.is_unicode() {
                                    return ONIGERR_UNDEFINED_GROUP_OPTION;
                                }
                                option_negate(&mut option, ONIG_OPTION_TEXT_SEGMENT_WORD, false);
                                option_negate(
                                    &mut option,
                                    ONIG_OPTION_TEXT_SEGMENT_EXTENDED_GRAPHEME_CLUSTER,
                                    true,
                                );
                            } else {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                            if p >= end {
                                return ONIGERR_END_PATTERN_IN_GROUP;
                            }
                            let c3 = self.pfetch(&mut p, &mut prev, end);
                            if c3 != b'}' as u32 {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                        }
                        'a' => {
                            if !self.syntax.bv(ONIG_SYN_PYTHON) {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                            option_negate(&mut option, ONIG_OPTION_POSIX_IS_ASCII, neg);
                        }
                        'C' => {
                            if !self.syntax.bv(ONIG_SYN_WHOLE_OPTIONS) {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                            if neg {
                                return ONIGERR_INVALID_GROUP_OPTION;
                            }
                            option_negate(&mut option, ONIG_OPTION_DONT_CAPTURE_GROUP, neg);
                            whole_options = true;
                        }
                        'I' => {
                            if !self.syntax.bv(ONIG_SYN_WHOLE_OPTIONS) {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                            if neg {
                                return ONIGERR_INVALID_GROUP_OPTION;
                            }
                            option_negate(&mut option, ONIG_OPTION_IGNORECASE_IS_ASCII, neg);
                            whole_options = true;
                        }
                        'L' => {
                            if !self.syntax.bv(ONIG_SYN_WHOLE_OPTIONS) {
                                return ONIGERR_UNDEFINED_GROUP_OPTION;
                            }
                            if neg {
                                return ONIGERR_INVALID_GROUP_OPTION;
                            }
                            option_negate(&mut option, ONIG_OPTION_FIND_LONGEST, neg);
                            whole_options = true;
                        }
                        _ => return ONIGERR_UNDEFINED_GROUP_OPTION,
                    }

                    if c == b')' as u32 {
                        *np = self.node_new_option(option);
                        if whole_options {
                            let r = self.set_whole_options(option);
                            if r != 0 {
                                return r;
                            }
                            self.nodes.add(*np, ND_ST_WHOLE_OPTIONS);
                        }
                        *src = p;
                        return 2;
                    } else if c == b':' as u32 {
                        let prev_opts = self.options;
                        self.options = option;
                        if whole_options {
                            let r = self.set_whole_options(option);
                            if r != 0 {
                                return r;
                            }
                        }
                        let r = self.fetch_token(tok, &mut p, end);
                        if r < 0 {
                            return r;
                        }
                        let mut target = NULL_NODE;
                        let r = self.prs_alts(&mut target, tok, term, &mut p, end, false);
                        self.options = prev_opts;
                        if r < 0 {
                            return r;
                        }
                        *np = self.node_new_option(option);
                        self.nodes.set_body(*np, target);
                        self.nodes.add(*np, ND_ST_WHOLE_OPTIONS);
                        *src = p;
                        return 0;
                    }

                    if p >= end {
                        return ONIGERR_END_PATTERN_IN_GROUP;
                    }
                    c = self.pfetch(&mut p, &mut prev, end);
                }
            }
        } else if c == b'*' as u32 && self.syntax.op2(ONIG_SYN_OP2_ASTERISK_CALLOUT_NAME) {
            self.pinc(&mut p, &mut prev);
            let r = self.prs_callout_of_name(b')' as u32, &mut p, end);
            return r;
        } else {
            if opton_dont_capture_group(self.options) {
                // goto group
                let r = self.fetch_token(tok, &mut p, end);
                if r < 0 {
                    return r;
                }
                let r = self.prs_alts(np, tok, term, &mut p, end, false);
                if r < 0 {
                    return r;
                }
                *src = p;
                return 1;
            }
            *np = self.node_new_memory(false);
            let num = self.scan_env_add_mem_entry();
            if num < 0 {
                return num;
            }
            self.set_bag_regnum(*np, num);
        }

        let r = self.fetch_token(tok, &mut p, end);
        if r < 0 {
            return r;
        }
        let mut target = NULL_NODE;
        let r = self.prs_alts(&mut target, tok, term, &mut p, end, false);
        if r < 0 {
            return r;
        }
        self.nodes.set_body(*np, target);

        if let NodeKind::Bag {
            typ: BagType::Memory,
            regnum,
            ..
        } = self.nodes.v[*np].kind
        {
            let r = self.scan_env_set_mem_node(regnum, *np);
            if r != 0 {
                return r;
            }
        }
        *src = p;
        0
    }

    /// the (?(cond)yes|no) part of prs_bag (p is after "(?(")
    fn prs_if_else(
        &mut self,
        np: &mut NodeRef,
        tok: &mut PToken,
        term: i32,
        src: &mut usize,
        mut p: usize,
        mut prev: usize,
        end: usize,
    ) -> i32 {
        if p >= end {
            return ONIGERR_END_PATTERN_IN_GROUP;
        }
        let mut c = self.pfetch(&mut p, &mut prev, end);
        if p >= end {
            return ONIGERR_END_PATTERN_IN_GROUP;
        }

        let mut condition = NULL_NODE;
        let condition_is_checker;
        let mut any_condition = false;

        if self.is_code_digit_ascii(c)
            || c == b'-' as u32
            || c == b'+' as u32
            || c == b'<' as u32
            || c == b'\'' as u32
        {
            let is_enclosed = c == b'<' as u32 || c == b'\'' as u32;
            if !is_enclosed {
                p = prev;
            }
            let prev_name = p;
            let mut name_end = 0usize;
            let mut back_num = 0;
            let mut level = 0;
            let mut num_type = RefNum::IsNotNum;
            let r = self.fetch_name_with_level(
                if is_enclosed { c } else { b'(' as u32 },
                &mut p,
                end,
                &mut name_end,
                &mut back_num,
                &mut level,
                &mut num_type,
            );
            let exist_level = r == 1;
            if r < 0 {
                if !is_enclosed {
                    any_condition = true;
                } else {
                    return r;
                }
            }
            if !any_condition {
                condition_is_checker = true;
                if num_type != RefNum::IsNotNum {
                    if num_type == RefNum::IsRelNum {
                        back_num = self.backref_rel_to_abs(back_num);
                    }
                    if back_num <= 0 {
                        return ONIGERR_INVALID_BACKREF;
                    }
                    if self.syntax.bv(ONIG_SYN_STRICT_CHECK_BACKREF)
                        && (back_num > self.num_mem || self.mem_node(back_num) == NULL_NODE)
                    {
                        return ONIGERR_INVALID_BACKREF;
                    }
                    condition =
                        self.node_new_backref_checker(&[back_num], false, exist_level, level);
                } else {
                    let backs = match self.name_to_group_numbers(prev_name, name_end) {
                        Ok(b) => b,
                        Err(e) => return e,
                    };
                    if backs.is_empty() {
                        return ONIGERR_UNDEFINED_NAME_REFERENCE;
                    }
                    if self.syntax.bv(ONIG_SYN_STRICT_CHECK_BACKREF) {
                        for &b in &backs {
                            if b > self.num_mem || self.mem_node(b) == NULL_NODE {
                                return ONIGERR_INVALID_BACKREF;
                            }
                        }
                    }
                    condition = self.node_new_backref_checker(&backs, true, exist_level, level);
                }
                if is_enclosed {
                    if p >= end {
                        return ONIGERR_END_PATTERN_IN_GROUP;
                    }
                    c = self.pfetch(&mut p, &mut prev, end);
                    if c != b')' as u32 {
                        return ONIGERR_END_PATTERN_IN_GROUP;
                    }
                }
            } else {
                condition_is_checker = false;
            }
        } else if c == b'?' as u32 {
            if self.syntax.op2(ONIG_SYN_OP2_QMARK_BRACE_CALLOUT_CONTENTS)
                && p < end
                && self.ppeek(p, end) == b'{' as u32
            {
                self.pfetch(&mut p, &mut prev, end);
                let r = self.prs_callout_of_contents(&mut condition, b')' as u32, &mut p, end);
                if r != 0 {
                    return r;
                }
                condition_is_checker = false;
            } else {
                any_condition = true;
                condition_is_checker = false;
            }
        } else if c == b'*' as u32 && self.syntax.op2(ONIG_SYN_OP2_ASTERISK_CALLOUT_NAME) {
            return self.prs_callout_of_name(b')' as u32, &mut p, end);
        } else {
            any_condition = true;
            condition_is_checker = false;
        }

        if any_condition {
            p = prev;
            let r = self.fetch_token(tok, &mut p, end);
            if r < 0 {
                return r;
            }
            let r = self.prs_alts(&mut condition, tok, term, &mut p, end, false);
            if r < 0 {
                return r;
            }
        }

        if p >= end {
            return ONIGERR_END_PATTERN_IN_GROUP;
        }

        if self.ppeek(p, end) == b')' as u32 {
            if !condition_is_checker {
                return ONIGERR_INVALID_IF_ELSE_SYNTAX;
            }
            self.pfetch(&mut p, &mut prev, end);
            *np = condition;
        } else {
            let mut then = NULL_NODE;
            let els;
            let then_is_empty = if self.ppeek(p, end) == b'|' as u32 {
                self.pfetch(&mut p, &mut prev, end);
                true
            } else {
                false
            };
            let r = self.fetch_token(tok, &mut p, end);
            if r < 0 {
                return r;
            }
            let mut target = NULL_NODE;
            let r = self.prs_alts(&mut target, tok, term, &mut p, end, true);
            if r < 0 {
                return r;
            }
            if then_is_empty {
                els = target;
            } else if self.nodes.typ(target) == NodeType::Alt {
                then = self.nodes.car(target);
                let cdr = self.nodes.cdr(target);
                if self.nodes.cdr(cdr) == NULL_NODE {
                    els = self.nodes.car(cdr);
                } else {
                    els = cdr;
                }
            } else {
                then = target;
                els = NULL_NODE;
            }
            *np = self.node_new_bag_if_else(condition, then, els);
        }
        *src = p;
        0
    }

    /// assign_quantifier_body: 0 normal, 1 x{1,1} ==> x, 2 split (/abc+/)
    fn assign_quantifier_body(&mut self, qnode: NodeRef, target: NodeRef, group: i32) -> i32 {
        let (ql, qu, qg) = quant_params(&self.nodes, qnode);
        if ql == 1 && qu == 1 {
            return 1;
        }
        match self.nodes.typ(target) {
            NodeType::String => {
                if group == 0 && self.str_node_can_be_split(target) {
                    let n = self.str_node_split_last_char(target);
                    if n != NULL_NODE {
                        self.nodes.set_body(qnode, n);
                        return 2;
                    }
                }
            }
            NodeType::Quant => {
                let (tl, tu, tg) = quant_params(&self.nodes, target);
                let nestq_num = quantifier_type_num(ql, qu, qg);
                let targetq_num = quantifier_type_num(tl, tu, tg);
                if targetq_num >= 0 && nestq_num < 0 {
                    if (targetq_num == 1 || targetq_num == 2)
                        && !is_infinite_repeat(qu)
                        && qu > 1
                        && qg
                    {
                        let nu = if ql == 0 { 1 } else { ql };
                        set_quant_params(&mut self.nodes, qnode, ql, nu, qg);
                    }
                } else {
                    self.nodes.set_body(qnode, target);
                    return reduce_nested_quantifier(&mut self.nodes, qnode);
                }
            }
            _ => {}
        }
        self.nodes.set_body(qnode, target);
        0
    }

    /// i_apply_case_fold over a character class (case folding applied to
    /// a parsed [..] under ignorecase). Returns the alternatives root.
    fn apply_case_fold_to_cc(&mut self, cc: &mut CClass) -> Result<NodeRef, i32> {
        let enc = self.enc;
        let mut alts: Vec<NodeRef> = Vec::new();
        let mut items: Vec<(u32, Vec<u32>)> = Vec::new();
        let r = enc.apply_all_case_fold(self.case_fold_flag, &mut |from, to| {
            items.push((from, to.to_vec()));
            0
        });
        if r != 0 {
            return Err(r);
        }
        for (from, to) in items {
            if to.len() == 1 {
                let is_in = is_code_in_cc(enc, from, cc);
                /* CASE_FOLD_IS_APPLIED_INSIDE_NEGATIVE_CCLASS */
                if (is_in && !cc.not) || (!is_in && cc.not) {
                    add_code_into_cc(cc, to[0], enc);
                }
            } else if is_code_in_cc(enc, from, cc) && !cc.not {
                let mut ns: Vec<NodeRef> = Vec::new();
                for &code in &to {
                    let index = if enc.is_unicode() {
                        unicode_fold1_key(code)
                    } else {
                        None
                    };
                    if let Some(index) = index {
                        let csnode = self.node_new_cclass();
                        let mut cs_cc = CClass::default();
                        for &u in unicode_folds1_unfolds(index) {
                            add_code_into_cc(&mut cs_cc, u, enc);
                        }
                        add_code_into_cc(&mut cs_cc, code, enc);
                        *self.nodes.cclass_mut(csnode) = cs_cc;
                        ns.push(csnode);
                    } else {
                        let mut buf = [0u8; ONIGENC_CODE_TO_MBC_MAXLEN];
                        let len = enc.code_to_mbc(code, &mut buf).max(0) as usize;
                        let last_is_str = ns
                            .last()
                            .map(|&n| self.nodes.typ(n) == NodeType::String)
                            .unwrap_or(false);
                        if !last_is_str {
                            let csnode = self.node_new_str(&buf[..len]);
                            /* index is never 0 here in the C (it is either
                            >= 0 from fold1_key or 0 for non-Unicode) */
                            if !enc.is_unicode() {
                                self.nodes.add(csnode, ND_ST_IGNORECASE);
                            } else if let NodeKind::Str { flag, .. } =
                                &mut self.nodes.v[csnode].kind
                            {
                                *flag |= ND_STRING_CASE_EXPANDED;
                            }
                            ns.push(csnode);
                        } else {
                            let last = *ns.last().unwrap();
                            self.nodes.str_mut(last).extend_from_slice(&buf[..len]);
                        }
                    }
                }
                let list_node = if ns.len() == 1 {
                    ns[0]
                } else {
                    self.make_list(&ns)
                };
                alts.push(list_node);
            }
        }
        if alts.is_empty() {
            return Ok(NULL_NODE);
        }
        Ok(self.make_alt(&alts))
    }
}

#[inline]
fn option_negate(v: &mut u32, f: u32, negative: bool) {
    if negative {
        *v &= !f;
    } else {
        *v |= f;
    }
}

impl<'a> ParseEnv<'a> {
    fn prs_exp(
        &mut self,
        np: &mut NodeRef,
        tok: &mut PToken,
        term: i32,
        src: &mut usize,
        end: usize,
        group_head: bool,
    ) -> i32 {
        let mut group;
        let mut r;
        let mut parse_depth;

        'retry: loop {
            group = 0;
            *np = NULL_NODE;
            if tok.typ == term {
                *np = self.node_new_empty();
                return tok.typ;
            }

            parse_depth = self.parse_depth;

            // Some token types jump directly to the "string" or "repeat" code
            let mut tp_is_np = true;
            let mut skip_reentry = false;

            match tok.typ {
                TK_ALT | TK_EOT => {
                    *np = self.node_new_empty();
                    return tok.typ;
                }
                TK_SUBEXP_OPEN => {
                    r = self.prs_bag(np, tok, TK_SUBEXP_CLOSE, src, end);
                    if r < 0 {
                        return r;
                    }
                    if r == 1 {
                        if !group_head {
                            group = 1;
                        } else {
                            let target = *np;
                            *np = self.node_new_group(target);
                            group = 2;
                        }
                    } else if r == 2 {
                        if self.syntax.bv(ONIG_SYN_ISOLATED_OPTION_CONTINUE_BRANCH) {
                            if let NodeKind::Bag { options, .. } = self.nodes.v[*np].kind {
                                self.options = options;
                            }
                            r = self.fetch_token(tok, src, end);
                            if r < 0 {
                                return r;
                            }
                            continue 'retry;
                        } else {
                            let prev = self.options;
                            if let NodeKind::Bag { options, .. } = self.nodes.v[*np].kind {
                                self.options = options;
                            }
                            r = self.fetch_token(tok, src, end);
                            if r < 0 {
                                return r;
                            }
                            let mut target = NULL_NODE;
                            r = self.prs_alts(&mut target, tok, term, src, end, false);
                            self.options = prev;
                            if r < 0 {
                                return r;
                            }
                            self.nodes.set_body(*np, target);
                            return tok.typ;
                        }
                    }
                }
                TK_SUBEXP_CLOSE => {
                    if !self.syntax.bv(ONIG_SYN_ALLOW_UNMATCHED_CLOSE_SUBEXP) {
                        return ONIGERR_UNMATCHED_CLOSE_PARENTHESIS;
                    }
                    if tok.escaped {
                        return self.prs_exp_crude_byte(np, tok, term, src, end, group);
                    } else {
                        return self.prs_exp_string(np, tok, term, src, end, group);
                    }
                }
                TK_STRING => {
                    return self.prs_exp_string(np, tok, term, src, end, group);
                }
                TK_CRUDE_BYTE => {
                    return self.prs_exp_crude_byte(np, tok, term, src, end, group);
                }
                TK_CODE_POINT => {
                    let mut buf = [0u8; ONIGENC_CODE_TO_MBC_MAXLEN];
                    let len = self.enc.code_to_mbclen(tok.code);
                    if len < 0 {
                        return len;
                    }
                    let len = self.enc.code_to_mbc(tok.code, &mut buf);
                    if len < 0 {
                        return len;
                    }
                    *np = self.node_new_str_with_options(&buf[..len as usize], self.options);
                }
                TK_QUOTE_OPEN => {
                    let end_op = [MC_ESC, b'E' as u32];
                    let qstart = *src;
                    let (qend, nextp) = match self.find_str_position(&end_op, qstart, end) {
                        Some((q, n)) => (q, n),
                        None => (end, end),
                    };
                    *np = self.node_new_str_with_options(&self.pattern[qstart..qend], self.options);
                    *src = nextp;
                }
                TK_CHAR_TYPE => {
                    let ct = tok.prop_ctype;
                    if ct == ONIGENC_CTYPE_WORD as i32 {
                        *np = self.node_new_ctype(ct, tok.prop_not, self.options);
                    } else if ct == ONIGENC_CTYPE_SPACE as i32
                        || ct == ONIGENC_CTYPE_DIGIT as i32
                        || ct == ONIGENC_CTYPE_XDIGIT as i32
                    {
                        *np = self.node_new_cclass();
                        let mut cc = CClass::default();
                        r = add_ctype_to_cc(&mut cc, ct, false, self.options, self.enc);
                        if r != 0 {
                            *np = NULL_NODE;
                            return r;
                        }
                        if tok.prop_not {
                            cc.not = true;
                        }
                        *self.nodes.cclass_mut(*np) = cc;
                    } else {
                        return ONIGERR_PARSER_BUG;
                    }
                }
                TK_CHAR_PROPERTY => {
                    let t = tok.clone();
                    r = self.prs_char_property(np, &t, src, end);
                    if r != 0 {
                        return r;
                    }
                }
                TK_OPEN_CC => {
                    r = self.prs_cc(np, tok, src, end);
                    if r != 0 {
                        return r;
                    }
                    if opton_ignorecase(self.options) {
                        let mut cc = self.nodes.cclass(*np).clone();
                        let alt_root = match self.apply_case_fold_to_cc(&mut cc) {
                            Ok(a) => a,
                            Err(e) => return e,
                        };
                        *self.nodes.cclass_mut(*np) = cc;
                        if alt_root != NULL_NODE {
                            let work = self.node_new_alt(*np, alt_root);
                            *np = work;
                        }
                    }
                }
                TK_ANYCHAR => {
                    *np = self.node_new_anychar(self.options);
                }
                TK_ANYCHAR_ANYTIME => {
                    let a = self.node_new_anychar(self.options);
                    let qn = self.node_new_quantifier(0, INFINITE_REPEAT, false);
                    self.nodes.set_body(qn, a);
                    *np = qn;
                }
                TK_BACKREF => {
                    let refs: Vec<i32> = if tok.br_num > 1 {
                        tok.br_refs.clone()
                    } else {
                        vec![tok.br_ref1]
                    };
                    *np = self.node_new_backref(
                        &refs,
                        tok.br_by_name,
                        tok.br_exist_level,
                        tok.br_level,
                    );
                }
                TK_CALL => {
                    let gnum = tok.call_gnum;
                    let name = self.pattern_slice(tok.call_name, tok.call_name_end);
                    *np = self.node_new_call(name, gnum, tok.call_by_number);
                    self.num_call += 1;
                    if tok.call_by_number && gnum == 0 {
                        self.flags |= PE_FLAG_HAS_CALL_ZERO;
                    }
                }
                TK_ANCHOR => {
                    *np = self.node_new_anchor_with_options(tok.anchor, self.options);
                }
                TK_REPEAT | TK_INTERVAL => {
                    if self.syntax.bv(ONIG_SYN_CONTEXT_INDEP_REPEAT_OPS) {
                        if self.syntax.bv(ONIG_SYN_CONTEXT_INVALID_REPEAT_OPS) {
                            return ONIGERR_TARGET_OF_REPEAT_OPERATOR_NOT_SPECIFIED;
                        } else {
                            *np = self.node_new_empty();
                        }
                    } else {
                        if tok.typ == TK_INTERVAL && self.syntax.op(ONIG_SYN_OP_ESC_BRACE_INTERVAL)
                        {
                            let mut s = self.pattern[tok.backp..*src].to_vec();
                            s.retain(|&b| b != b'\\');
                            *np = self.node_new_str_with_options(&s, self.options);
                            return self.prs_exp_string_tail(np, tok, term, src, end, group);
                        } else {
                            return self.prs_exp_string(np, tok, term, src, end, group);
                        }
                    }
                }
                TK_KEEP => {
                    *np = self.node_new_keep();
                }
                TK_GENERAL_NEWLINE => {
                    *np = self.node_new_general_newline();
                }
                TK_NO_NEWLINE => {
                    *np = self.node_new_no_newline();
                }
                TK_TRUE_ANYCHAR => {
                    *np = self.node_new_true_anychar();
                }
                TK_TEXT_SEGMENT => {
                    *np = self.make_text_segment();
                }
                _ => return ONIGERR_PARSER_BUG,
            }
            let _ = (tp_is_np, skip_reentry);
            tp_is_np = true;
            skip_reentry = false;
            let _ = (tp_is_np, skip_reentry);
            break;
        }

        // re_entry:
        r = self.fetch_token(tok, src, end);
        if r < 0 {
            return r;
        }
        self.prs_exp_repeat(np, tok, r, src, end, group, parse_depth, None)
    }

    /// The "tk_byte" path (string tokens concatenated)
    fn prs_exp_string(
        &mut self,
        np: &mut NodeRef,
        tok: &mut PToken,
        term: i32,
        src: &mut usize,
        end: usize,
        group: i32,
    ) -> i32 {
        let s = self.pattern[tok.backp..*src].to_vec();
        *np = self.node_new_str_with_options(&s, self.options);
        self.prs_exp_string_tail(np, tok, term, src, end, group)
    }

    /// tk_byte2: concatenate following TK_STRING tokens, then repeat
    fn prs_exp_string_tail(
        &mut self,
        np: &mut NodeRef,
        tok: &mut PToken,
        _term: i32,
        src: &mut usize,
        end: usize,
        group: i32,
    ) -> i32 {
        let r;
        loop {
            let r2 = self.fetch_token(tok, src, end);
            if r2 < 0 {
                return r2;
            }
            if r2 != TK_STRING {
                r = r2;
                break;
            }
            let s = self.pattern[tok.backp..*src].to_vec();
            self.nodes.str_mut(*np).extend_from_slice(&s);
        }
        let parse_depth = self.parse_depth;
        self.prs_exp_repeat(np, tok, r, src, end, group, parse_depth, None)
    }

    fn prs_exp_crude_byte(
        &mut self,
        np: &mut NodeRef,
        tok: &mut PToken,
        _term: i32,
        src: &mut usize,
        end: usize,
        group: i32,
    ) -> i32 {
        *np = self.node_new_str_crude_char(tok.byte, self.options);
        let mut len = 1usize;
        let r;
        loop {
            if len >= self.enc.min_enc_len {
                let s = self.nodes.str_bytes(*np);
                if len == self.enc.mbc_enc_len(s, 0) {
                    r = self.fetch_token(tok, src, end);
                    break;
                }
            }
            let r2 = self.fetch_token(tok, src, end);
            if r2 < 0 {
                return r2;
            }
            if r2 != TK_CRUDE_BYTE {
                return ONIGERR_TOO_SHORT_MULTI_BYTE_STRING;
            }
            let b = tok.byte;
            self.nodes.str_mut(*np).push(b);
            len += 1;
        }
        // tk_crude_byte_end:
        {
            let s = self.nodes.str_bytes(*np).to_vec();
            if !self.enc.is_valid_mbc_string(&s, 0, s.len()) {
                return ONIGERR_INVALID_WIDE_CHAR_VALUE;
            }
        }
        if let NodeKind::Str { flag, .. } = &mut self.nodes.v[*np].kind {
            *flag &= !ND_STRING_CRUDE;
        }
        // goto string_end -> repeat (r may be an error)
        if r < 0 {
            return r;
        }
        let parse_depth = self.parse_depth;
        self.prs_exp_repeat(np, tok, r, src, end, group, parse_depth, None)
    }

    /// The `repeat:` loop of prs_exp. `tp` is where the current target node
    /// is stored (None: *np).
    fn prs_exp_repeat(
        &mut self,
        np: &mut NodeRef,
        tok: &mut PToken,
        mut r: i32,
        src: &mut usize,
        end: usize,
        mut group: i32,
        mut parse_depth: u32,
        _unused: Option<()>,
    ) -> i32 {
        /* tp: either np itself or the car of a list cell */
        let mut tp_cell: NodeRef = NULL_NODE; /* NULL_NODE: tp == np */

        loop {
            if r == TK_REPEAT || r == TK_INTERVAL {
                let cur = if tp_cell == NULL_NODE {
                    *np
                } else {
                    self.nodes.car(tp_cell)
                };
                if is_invalid_quantifier_target(&self.nodes, cur) {
                    if self.syntax.bv(ONIG_SYN_CONTEXT_INDEP_REPEAT_OPS)
                        && self.syntax.bv(ONIG_SYN_CONTEXT_INVALID_REPEAT_OPS)
                    {
                        return ONIGERR_TARGET_OF_REPEAT_OPERATOR_INVALID;
                    }
                    return r;
                }

                parse_depth += 1;
                if parse_depth > DEFAULT_PARSE_DEPTH_LIMIT {
                    return ONIGERR_PARSE_DEPTH_LIMIT_OVER;
                }

                let qn = self.node_new_quantifier(tok.rep_lower, tok.rep_upper, r == TK_INTERVAL);
                if let NodeKind::Quant { greedy, .. } = &mut self.nodes.v[qn].kind {
                    *greedy = tok.rep_greedy;
                }
                let target = if group == 2 {
                    self.node_drop_group(cur)
                } else {
                    cur
                };
                let r2 = self.assign_quantifier_body(qn, target, group);
                if r2 < 0 {
                    return r2;
                }

                let mut qnode = qn;
                if tok.rep_possessive {
                    let en = self.node_new_bag(BagType::StopBacktrack);
                    self.nodes.set_body(en, qn);
                    qnode = en;
                }

                if r2 == 0 {
                    self.set_tp(np, tp_cell, qnode);
                } else if r2 == 1 {
                    self.set_tp(np, tp_cell, target);
                } else if r2 == 2 {
                    /* split case: /abc+/ */
                    let cur_target = if tp_cell == NULL_NODE {
                        *np
                    } else {
                        self.nodes.car(tp_cell)
                    };
                    let tmp = self.node_new_list(qnode, NULL_NODE);
                    let lst = self.node_new_list(cur_target, tmp);
                    self.set_tp(np, tp_cell, lst);
                    tp_cell = tmp;
                }
                group = 0;
                // re_entry:
                r = self.fetch_token(tok, src, end);
                if r < 0 {
                    return r;
                }
                continue;
            }
            return r;
        }
    }

    fn set_tp(&mut self, np: &mut NodeRef, tp_cell: NodeRef, v: NodeRef) {
        if tp_cell == NULL_NODE {
            *np = v;
        } else {
            self.nodes.set_car(tp_cell, v);
        }
    }

    fn prs_branch(
        &mut self,
        top: &mut NodeRef,
        tok: &mut PToken,
        term: i32,
        src: &mut usize,
        end: usize,
        group_head: bool,
    ) -> i32 {
        *top = NULL_NODE;
        inc_parse_depth!(self, self.parse_depth);

        let mut node = NULL_NODE;
        let mut r = self.prs_exp(&mut node, tok, term, src, end, group_head);
        if r < 0 {
            return r;
        }

        if r == TK_EOT || r == term || r == TK_ALT {
            *top = node;
        } else {
            *top = self.node_new_list(node, NULL_NODE);
            let mut headp_cell = *top; /* cell whose cdr is the tail */
            while r != TK_EOT && r != term && r != TK_ALT {
                let mut node = NULL_NODE;
                r = self.prs_exp(&mut node, tok, term, src, end, false);
                if r < 0 {
                    return r;
                }
                if self.nodes.typ(node) == NodeType::List {
                    self.nodes.set_cdr(headp_cell, node);
                    let mut n = node;
                    while self.nodes.cdr(n) != NULL_NODE {
                        n = self.nodes.cdr(n);
                    }
                    headp_cell = n;
                } else {
                    let cell = self.node_new_list(node, NULL_NODE);
                    self.nodes.set_cdr(headp_cell, cell);
                    headp_cell = cell;
                }
            }
        }
        self.parse_depth -= 1;
        r
    }

    /// term_tok: TK_EOT or TK_SUBEXP_CLOSE
    fn prs_alts(
        &mut self,
        top: &mut NodeRef,
        tok: &mut PToken,
        term: i32,
        src: &mut usize,
        end: usize,
        group_head: bool,
    ) -> i32 {
        *top = NULL_NODE;
        inc_parse_depth!(self, self.parse_depth);
        let save_options = self.options;

        let mut node = NULL_NODE;
        let mut r = self.prs_branch(&mut node, tok, term, src, end, group_head);
        if r < 0 {
            return r;
        }

        if r == term {
            *top = node;
        } else if r == TK_ALT {
            *top = self.node_new_alt(node, NULL_NODE);
            let mut headp_cell = *top;
            while r == TK_ALT {
                r = self.fetch_token(tok, src, end);
                if r < 0 {
                    return r;
                }
                let mut node = NULL_NODE;
                r = self.prs_branch(&mut node, tok, term, src, end, false);
                if r < 0 {
                    return r;
                }
                let cell = self.node_new_alt(node, NULL_NODE);
                self.nodes.set_cdr(headp_cell, cell);
                headp_cell = cell;
            }
            if tok.typ != term {
                return if term == TK_SUBEXP_CLOSE {
                    ONIGERR_END_PATTERN_WITH_UNMATCHED_PARENTHESIS
                } else {
                    ONIGERR_PARSER_BUG
                };
            }
        } else {
            return if term == TK_SUBEXP_CLOSE {
                ONIGERR_END_PATTERN_WITH_UNMATCHED_PARENTHESIS
            } else {
                ONIGERR_PARSER_BUG
            };
        }

        self.options = save_options;
        self.parse_depth -= 1;
        r
    }

    fn prs_regexp(&mut self, top: &mut NodeRef) -> i32 {
        let mut tok = PToken::default();
        let mut p = 0usize;
        let end = self.pattern.len();
        let r = self.fetch_token(&mut tok, &mut p, end);
        if r < 0 {
            return r;
        }
        let r = self.prs_alts(top, &mut tok, TK_EOT, &mut p, end, false);
        if r < 0 {
            return r;
        }
        0
    }

    fn make_call_zero_body(&mut self, node: NodeRef) -> NodeRef {
        let x = self.node_new_memory(false);
        self.nodes.set_body(x, node);
        self.set_bag_regnum(x, 0);
        if self.mem_env.is_empty() {
            self.mem_env.push(MemEnv::default());
        }
        self.mem_env[0].mem_node = x;
        x
    }

    /// onig_parse_tree
    pub fn parse_tree(&mut self, root: &mut NodeRef) -> i32 {
        *root = NULL_NODE;
        if !self
            .enc
            .is_valid_mbc_string(self.pattern, 0, self.pattern.len())
        {
            return ONIGERR_INVALID_WIDE_CHAR_VALUE;
        }
        let r = self.prs_regexp(root);
        if r != 0 {
            return r;
        }
        if self.flags & PE_FLAG_HAS_CALL_ZERO != 0 {
            let zero = self.make_call_zero_body(*root);
            *root = zero;
        }
        r
    }
}
