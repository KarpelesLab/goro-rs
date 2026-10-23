//! Port of Oniguruma 6.9.10 regexec.c (backtracking matcher and search).

#![allow(clippy::too_many_arguments)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::collapsible_if)]

use super::enc::*;
use super::parse::*;
use super::regcomp::*;
use super::regint::*;

pub const ONIG_REGION_NOTPOS: i32 = -1;

/// OnigRegion
#[derive(Clone, Debug, Default)]
pub struct Region {
    pub beg: Vec<i32>,
    pub end: Vec<i32>,
}

impl Region {
    pub fn num_regs(&self) -> usize {
        self.beg.len()
    }
    fn resize_clear(&mut self, n: usize) {
        self.beg = vec![ONIG_REGION_NOTPOS; n];
        self.end = vec![ONIG_REGION_NOTPOS; n];
    }
    fn clear(&mut self) {
        for x in self.beg.iter_mut() {
            *x = ONIG_REGION_NOTPOS;
        }
        for x in self.end.iter_mut() {
            *x = ONIG_REGION_NOTPOS;
        }
    }
}

/// OnigMatchParam
#[derive(Clone, Copy, Debug)]
pub struct MatchParam {
    pub match_stack_limit: u32,
    pub retry_limit_in_match: u64,
    pub retry_limit_in_search: u64,
}

pub const DEFAULT_MATCH_STACK_LIMIT_SIZE: u32 = 0;
pub const DEFAULT_RETRY_LIMIT_IN_MATCH: u64 = 10000000;
pub const DEFAULT_RETRY_LIMIT_IN_SEARCH: u64 = 0;

impl Default for MatchParam {
    fn default() -> Self {
        MatchParam {
            match_stack_limit: DEFAULT_MATCH_STACK_LIMIT_SIZE,
            retry_limit_in_match: DEFAULT_RETRY_LIMIT_IN_MATCH,
            retry_limit_in_search: DEFAULT_RETRY_LIMIT_IN_SEARCH,
        }
    }
}

/* stack types */
const STK_ALT_FLAG: u32 = 0x0001;
const STK_SUPER_ALT: u32 = STK_ALT_FLAG;
const STK_ALT: u32 = 0x0002 | STK_ALT_FLAG;
const STK_MEM_START: u32 = 0x0010;
const STK_MEM_END: u32 = 0x8030;
const STK_REPEAT_INC: u32 = 0x0040;
const STK_VOID: u32 = 0x0000;
const STK_EMPTY_CHECK_START: u32 = 0x3000;
const STK_EMPTY_CHECK_END: u32 = 0x5000;
const STK_MEM_END_MARK: u32 = 0x8100;
const STK_MASK_POP_HANDLED: u32 = 0x0010;
const STK_CALL_FRAME: u32 = 0x0400 | STK_MASK_POP_HANDLED;
const STK_RETURN: u32 = 0x0500 | STK_MASK_POP_HANDLED;
const STK_SAVE_VAL: u32 = 0x0600;
const STK_MARK: u32 = 0x0704;
const STK_MASK_POP_USED: u32 = STK_ALT_FLAG;
const STK_MASK_POP_HANDLED_TIL: u32 = STK_MASK_POP_HANDLED | 0x0004;
const STK_MASK_TO_VOID_TARGET: u32 = 0x100e;
const STK_MASK_MEM_END_OR_MARK: u32 = 0x8000;

const INVALID_STACK_INDEX: isize = -1;
/// program counter of the static FinishCode op
const FINISH_PC: usize = usize::MAX;

const INIT_MATCH_STACK_SIZE: usize = 160;
const ALLOCA_PTR_NUM_LIMIT: usize = 50;

#[derive(Clone, Copy, Debug, Default)]
struct Stk {
    typ: u32,
    zid: i32,
    /// state.pcode / call_frame.ret_addr
    pcode: usize,
    /// state.pstr / mem.pstr / empty_check.pstr / val.v / mark pos
    pstr: usize,
    /// repeat_inc.count / val.type
    count: i32,
    /// mem.prev_start / mem.prev_end (StkPtrType)
    prev_start: isize,
    prev_end: isize,
}

/// MatchArg
struct MatchArg {
    stack_n: usize,
    has_stack: bool,
    options: u32,
    start: usize,
    match_stack_limit: u32,
    retry_limit_in_match: u64,
    retry_limit_in_search: u64,
    retry_limit_in_search_counter: u64,
    best_len: i32,
    best_s: usize,
    skip_search: usize,
    ptr_num: usize,
    /// the persisted stack (C: stack_p)
    stack: Vec<Stk>,
}

impl MatchArg {
    fn new(reg: &Regex, option: u32, start: usize, mp: &MatchParam) -> MatchArg {
        MatchArg {
            stack_n: 0,
            has_stack: false,
            options: option | reg.options,
            start,
            match_stack_limit: mp.match_stack_limit,
            retry_limit_in_match: mp.retry_limit_in_match,
            retry_limit_in_search: mp.retry_limit_in_search,
            retry_limit_in_search_counter: 0,
            best_len: ONIG_MISMATCH,
            best_s: 0,
            skip_search: start,
            ptr_num: ((reg.num_mem + 1) * 2) as usize,
            stack: Vec::new(),
        }
    }
}

/// The match stack with the C allocation/limit behaviour
struct Stack {
    v: Vec<Stk>,
    /// logical capacity (stk_end - stk_base)
    cap: usize,
    is_alloca: bool,
}

/// string_cmp_ic: compares s1[..mblen] with s2 at *ps2 under case
/// folding; updates *ps2 on success
fn string_cmp_ic(
    enc: &Encoding,
    case_fold_flag: u32,
    s: &[u8],
    s1: usize,
    ps2: &mut usize,
    mblen: usize,
) -> bool {
    let mut buf1 = [0u8; ONIGENC_MBC_CASE_FOLD_MAXLEN];
    let mut buf2 = [0u8; ONIGENC_MBC_CASE_FOLD_MAXLEN];
    let mut p1 = s1;
    let mut s2 = *ps2;
    let end1 = s1 + mblen;
    let end2 = s2 + mblen;
    while p1 < end1 {
        let len1 = enc.mbc_case_fold(case_fold_flag, s, &mut p1, end1, &mut buf1);
        let len2 = enc.mbc_case_fold(case_fold_flag, s, &mut s2, end2, &mut buf2);
        if len1 != len2 {
            return false;
        }
        if buf1[..len1] != buf2[..len2] {
            return false;
        }
        if s2 >= end2 {
            if p1 < end1 {
                return false;
            } else {
                break;
            }
        }
    }
    *ps2 = s2;
    true
}

fn mem_is_in_memp(mem: i32, memp: &[i32]) -> bool {
    memp.contains(&mem)
}

struct Matcher<'r, 's> {
    reg: &'r Regex,
    s: &'s [u8],
    end: usize,
}

impl<'r, 's> Matcher<'r, 's> {
    #[inline]
    fn enclen(&self, p: usize) -> usize {
        self.reg.enc.mbc_enc_len(self.s, p)
    }
    #[inline]
    fn byte(&self, p: usize) -> u8 {
        if p < self.s.len() { self.s[p] } else { 0 }
    }
    #[inline]
    fn is_newline(&self, p: usize) -> bool {
        self.reg.enc.is_mbc_newline(self.s, p, self.end)
    }
    #[inline]
    fn prev_char_head(&self, start: usize, p: usize) -> Option<usize> {
        get_prev_char_head(self.reg.enc, self.s, start, p)
    }
    #[inline]
    fn is_word_mode(&self, p: usize, ascii_mode: bool) -> bool {
        if !ascii_mode {
            self.reg.enc.is_mbc_word(self.s, p, self.end)
        } else {
            is_mbc_word_ascii(self.reg.enc, self.s, p, self.end)
        }
    }

    fn backref_match_at_nested_level(
        &self,
        stk: &[Stk],
        top: usize,
        ignore_case: bool,
        case_fold_flag: u32,
        nest: i32,
        memp: &[i32],
        ps: &mut usize,
        send: usize,
    ) -> bool {
        let mut level = 0;
        let mut pend: Option<usize> = None;
        let mut k = top as isize - 1;
        while k >= 0 {
            let e = &stk[k as usize];
            if e.typ == STK_CALL_FRAME {
                level -= 1;
            } else if e.typ == STK_RETURN {
                level += 1;
            } else if level == nest {
                if e.typ == STK_MEM_START {
                    if mem_is_in_memp(e.zid, memp) {
                        let pstart = e.pstr;
                        if let Some(pend) = pend {
                            if pend as isize - pstart as isize > send as isize - *ps as isize {
                                return false;
                            }
                            let mut ss = *ps;
                            if ignore_case {
                                if !string_cmp_ic(
                                    self.reg.enc,
                                    case_fold_flag,
                                    self.s,
                                    pstart,
                                    &mut ss,
                                    pend - pstart,
                                ) {
                                    return false;
                                }
                            } else {
                                let mut p = pstart;
                                while p < pend {
                                    if self.byte(p) != self.byte(ss) {
                                        return false;
                                    }
                                    p += 1;
                                    ss += 1;
                                }
                            }
                            *ps = ss;
                            return true;
                        }
                    }
                } else if e.typ == STK_MEM_END && mem_is_in_memp(e.zid, memp) {
                    pend = Some(e.pstr);
                }
            }
            k -= 1;
        }
        false
    }

    fn backref_check_at_nested_level(
        &self,
        stk: &[Stk],
        top: usize,
        nest: i32,
        memp: &[i32],
    ) -> bool {
        let mut level = 0;
        let mut k = top as isize - 1;
        while k >= 0 {
            let e = &stk[k as usize];
            if e.typ == STK_CALL_FRAME {
                level -= 1;
            } else if e.typ == STK_RETURN {
                level += 1;
            } else if level == nest && e.typ == STK_MEM_END && mem_is_in_memp(e.zid, memp) {
                return true;
            }
            k -= 1;
        }
        false
    }
}

const SUBEXP_CALL_MAX_NEST_LEVEL: u64 = DEFAULT_SUBEXP_CALL_MAX_NEST_LEVEL as u64;

/// match_at: match the subject s[..end] from position sstart. Returns
/// the match length, ONIG_MISMATCH or an error code.
fn match_at(
    reg: &Regex,
    s_all: &[u8],
    end: usize,
    in_right_range: usize,
    sstart: usize,
    msa: &mut MatchArg,
    region: &mut Option<&mut Region>,
) -> i32 {
    let m = Matcher { reg, s: s_all, end };
    let str_ = 0usize;
    let options = msa.options;
    let enc = reg.enc;
    let case_fold_flag = reg.case_fold_flag;
    let ops = &reg.ops;

    let mut retry_limit_in_match = msa.retry_limit_in_match;
    if msa.retry_limit_in_search != 0 {
        let rem = msa.retry_limit_in_search - msa.retry_limit_in_search_counter;
        if rem < retry_limit_in_match || retry_limit_in_match == 0 {
            retry_limit_in_match = rem;
        }
    }

    let pop_level = reg.stack_pop_level;
    let num_mem = reg.num_mem as usize;

    /* STACK_INIT */
    let mut st = if msa.has_stack {
        let mut v = std::mem::take(&mut msa.stack);
        v.clear();
        Stack {
            v,
            cap: msa.stack_n,
            is_alloca: false,
        }
    } else if msa.ptr_num > ALLOCA_PTR_NUM_LIMIT {
        Stack {
            v: Vec::with_capacity(INIT_MATCH_STACK_SIZE),
            cap: INIT_MATCH_STACK_SIZE,
            is_alloca: false,
        }
    } else {
        Stack {
            v: Vec::with_capacity(INIT_MATCH_STACK_SIZE),
            cap: INIT_MATCH_STACK_SIZE,
            is_alloca: true,
        }
    };

    let mut mem_start_stk: Vec<isize> = vec![INVALID_STACK_INDEX; num_mem + 1];
    let mut mem_end_stk: Vec<isize> = vec![INVALID_STACK_INDEX; num_mem + 1];

    let mut best_len: i32 = ONIG_MISMATCH;
    let mut s = sstart;
    let mut keep = sstart;
    /* STACK_PUSH_BOTTOM(STK_ALT, FinishCode) */
    st.v.push(Stk {
        typ: STK_ALT,
        pcode: FINISH_PC,
        ..Default::default()
    });
    let mut right_range = in_right_range;
    let mut retry_in_match_counter: u64 = 0;
    let mut subexp_call_nest_counter: u64 = 0;
    let mut p: usize = 0;
    /* the C reads `stk->zid` in STEP_BACK_NEXT: the entry that the last
    failure popped (still in memory right above the stack top) */
    let mut popped_zid: i32 = 0;

    'interp: loop {
        let mut fail = false;

        macro_rules! stack_ensure {
            () => {
                if st.cap <= st.v.len() {
                    /* stack_double */
                    let mut n = st.cap;
                    n *= 2;
                    if st.is_alloca {
                        st.is_alloca = false;
                    } else if msa.match_stack_limit != 0 && n > msa.match_stack_limit as usize {
                        if st.cap == msa.match_stack_limit as usize {
                            best_len = ONIGERR_MATCH_STACK_LIMIT_OVER;
                            break 'interp;
                        } else {
                            n = msa.match_stack_limit as usize;
                        }
                    }
                    st.cap = n;
                }
            };
        }

        macro_rules! push {
            ($e:expr) => {{
                stack_ensure!();
                st.v.push($e);
            }};
        }

        macro_rules! push_alt {
            ($pc:expr, $s:expr) => {
                push!(Stk {
                    typ: STK_ALT,
                    pcode: $pc,
                    pstr: $s,
                    ..Default::default()
                })
            };
        }

        macro_rules! data_ensure {
            ($n:expr) => {
                if (right_range as isize - s as isize) < ($n) as isize {
                    fail = true;
                }
            };
        }

        macro_rules! stack_mem_start {
            ($idx:expr) => {
                if mem_status_at(reg.push_mem_start, $idx as i32) {
                    st.v[mem_start_stk[$idx] as usize].pstr
                } else {
                    mem_start_stk[$idx] as usize
                }
            };
        }
        macro_rules! stack_mem_end {
            ($idx:expr) => {
                if mem_status_at(reg.push_mem_end, $idx as i32) {
                    st.v[mem_end_stk[$idx] as usize].pstr
                } else {
                    mem_end_stk[$idx] as usize
                }
            };
        }

        /* pop helpers that restore mem pointers */
        macro_rules! pop_handled {
            ($e:expr) => {{
                let e: Stk = $e;
                if e.typ == STK_MEM_START || e.typ == STK_MEM_END {
                    mem_start_stk[e.zid as usize] = e.prev_start;
                    mem_end_stk[e.zid as usize] = e.prev_end;
                } else if e.typ == STK_RETURN {
                    subexp_call_nest_counter += 1;
                } else if e.typ == STK_CALL_FRAME {
                    subexp_call_nest_counter = subexp_call_nest_counter.wrapping_sub(1);
                }
            }};
        }

        if p == FINISH_PC {
            /* OP_FINISH */
            if opton_find_longest(options) {
                best_len = ONIG_MISMATCH;
            }
            break 'interp;
        }

        match &ops[p] {
            Op::Finish => {
                if opton_find_longest(options) {
                    best_len = ONIG_MISMATCH;
                }
                break 'interp;
            }
            Op::End => {
                let mut goto_end_out = false;
                'end_op: {
                    if (options & ONIG_OPTION_MATCH_WHOLE_STRING) != 0 && s != end {
                        fail = true;
                        break 'end_op;
                    }
                    let n = (s - sstart) as i32;
                    if n == 0 && opton_find_not_empty(options) {
                        best_len = ONIG_MISMATCH;
                        fail = true;
                        break 'end_op;
                    }
                    if opton_find_longest(options) {
                        if n > best_len {
                            if n > msa.best_len {
                                best_len = n;
                                msa.best_len = n;
                                msa.best_s = sstart;
                            } else if s >= in_right_range && msa.best_s == sstart {
                                goto_end_out = true;
                                break 'end_op;
                            } else {
                                fail = true;
                                break 'end_op;
                            }
                        } else {
                            fail = true;
                            break 'end_op;
                        }
                    } else {
                        best_len = n;
                    }

                    if let Some(region) = region.as_deref_mut() {
                        if keep > s {
                            keep = s;
                        }
                        region.beg[0] = (keep - str_) as i32;
                        region.end[0] = (s - str_) as i32;
                        for i in 1..=num_mem {
                            if mem_end_stk[i] != INVALID_STACK_INDEX {
                                region.beg[i] = (stack_mem_start!(i) - str_) as i32;
                                region.end[i] = (stack_mem_end!(i) - str_) as i32;
                            } else {
                                region.beg[i] = ONIG_REGION_NOTPOS;
                                region.end[i] = ONIG_REGION_NOTPOS;
                            }
                        }
                    }

                    if opton_find_longest(options) {
                        fail = true;
                        break 'end_op;
                    }
                    goto_end_out = true;
                }
                if goto_end_out {
                    break 'interp;
                }
            }
            Op::Str(bytes) => {
                let tlen = bytes.len();
                data_ensure!(tlen);
                if !fail {
                    if s_all[s..s + tlen] != bytes[..] {
                        fail = true;
                    } else {
                        s += tlen;
                        p += 1;
                    }
                }
            }
            Op::CClass(bs) => {
                data_ensure!(1);
                if !fail {
                    if !bitset_at(bs, m.byte(s) as u32) || enc.is_mbc_head(s_all, s) {
                        fail = true;
                    } else {
                        s += 1;
                        p += 1;
                    }
                }
            }
            Op::CClassMb(mb) => {
                data_ensure!(1);
                if !fail {
                    if !enc.is_mbc_head(s_all, s) {
                        fail = true;
                    } else {
                        let mb_len = m.enclen(s);
                        data_ensure!(mb_len);
                        if !fail {
                            let ss = s;
                            s += mb_len;
                            let code = enc.mbc_to_code(s_all, ss, s);
                            if !super::parse::is_in_code_range(mb, code) {
                                fail = true;
                            } else {
                                p += 1;
                            }
                        }
                    }
                }
            }
            Op::CClassMix(bs, mb) => {
                data_ensure!(1);
                if !fail {
                    if enc.is_mbc_head(s_all, s) {
                        let mb_len = m.enclen(s);
                        data_ensure!(mb_len);
                        if !fail {
                            let ss = s;
                            s += mb_len;
                            let code = enc.mbc_to_code(s_all, ss, s);
                            if !super::parse::is_in_code_range(mb, code) {
                                fail = true;
                            } else {
                                p += 1;
                            }
                        }
                    } else if !bitset_at(bs, m.byte(s) as u32) {
                        fail = true;
                    } else {
                        s += 1;
                        p += 1;
                    }
                }
            }
            Op::CClassNot(bs) => {
                data_ensure!(1);
                if !fail {
                    if bitset_at(bs, m.byte(s) as u32) {
                        fail = true;
                    } else {
                        s += m.enclen(s);
                        p += 1;
                    }
                }
            }
            Op::CClassMbNot(mb) | Op::CClassMixNot(_, mb) => {
                data_ensure!(1);
                if !fail {
                    let is_head = enc.is_mbc_head(s_all, s);
                    if !is_head {
                        if let Op::CClassMixNot(bs, _) = &ops[p] {
                            if bitset_at(bs, m.byte(s) as u32) {
                                fail = true;
                            }
                        }
                        if !fail {
                            s += 1;
                            p += 1;
                        }
                    } else {
                        let mb_len = m.enclen(s);
                        if !((mb_len as isize) <= right_range as isize - s as isize) {
                            data_ensure!(1);
                            if !fail {
                                s = end;
                                p += 1;
                            }
                        } else {
                            let ss = s;
                            s += mb_len;
                            let code = enc.mbc_to_code(s_all, ss, s);
                            if super::parse::is_in_code_range(mb, code) {
                                fail = true;
                            } else {
                                p += 1;
                            }
                        }
                    }
                }
            }
            Op::AnyChar => {
                data_ensure!(1);
                if !fail {
                    let n = m.enclen(s);
                    data_ensure!(n);
                    if !fail {
                        if m.is_newline(s) {
                            fail = true;
                        } else {
                            s += n;
                            p += 1;
                        }
                    }
                }
            }
            Op::AnyCharMl => {
                data_ensure!(1);
                if !fail {
                    let n = m.enclen(s);
                    data_ensure!(n);
                    if !fail {
                        s += n;
                        p += 1;
                    }
                }
            }
            Op::AnyCharStar => {
                p += 1;
                while s < right_range {
                    push_alt!(p, s);
                    let n = m.enclen(s);
                    data_ensure!(n);
                    if fail {
                        break;
                    }
                    if m.is_newline(s) {
                        fail = true;
                        break;
                    }
                    s += n;
                }
            }
            Op::AnyCharMlStar => {
                p += 1;
                while s < right_range {
                    push_alt!(p, s);
                    let n = m.enclen(s);
                    if n > 1 {
                        data_ensure!(n);
                        if fail {
                            break;
                        }
                        s += n;
                    } else {
                        s += 1;
                    }
                }
            }
            Op::AnyCharStarPeekNext(c) => {
                let c = *c;
                p += 1;
                while s < right_range {
                    if c == m.byte(s) {
                        push_alt!(p, s);
                    }
                    let n = m.enclen(s);
                    data_ensure!(n);
                    if fail {
                        break;
                    }
                    if m.is_newline(s) {
                        fail = true;
                        break;
                    }
                    s += n;
                }
            }
            Op::AnyCharMlStarPeekNext(c) => {
                let c = *c;
                p += 1;
                while s < right_range {
                    if c == m.byte(s) {
                        push_alt!(p, s);
                    }
                    let n = m.enclen(s);
                    if n > 1 {
                        data_ensure!(n);
                        if fail {
                            break;
                        }
                        s += n;
                    } else {
                        s += 1;
                    }
                }
            }
            Op::Word | Op::WordAscii | Op::NoWord | Op::NoWordAscii => {
                data_ensure!(1);
                if !fail {
                    let (ascii, want) = match &ops[p] {
                        Op::Word => (false, true),
                        Op::WordAscii => (true, true),
                        Op::NoWord => (false, false),
                        _ => (true, false),
                    };
                    if m.is_word_mode(s, ascii) != want {
                        fail = true;
                    } else {
                        s += m.enclen(s);
                        p += 1;
                    }
                }
            }
            Op::WordBoundary(mode) => {
                let mode = *mode;
                if s == str_ {
                    data_ensure!(1);
                    if !fail && !m.is_word_mode(s, mode) {
                        fail = true;
                    }
                } else {
                    let sprev = m.prev_char_head(str_, s).unwrap_or(str_);
                    if s == end {
                        if !m.is_word_mode(sprev, mode) {
                            fail = true;
                        }
                    } else if m.is_word_mode(s, mode) == m.is_word_mode(sprev, mode) {
                        fail = true;
                    }
                }
                if !fail {
                    p += 1;
                }
            }
            Op::NoWordBoundary(mode) => {
                let mode = *mode;
                if s == str_ {
                    if s < right_range && m.is_word_mode(s, mode) {
                        fail = true;
                    }
                } else {
                    let sprev = m.prev_char_head(str_, s).unwrap_or(str_);
                    if s == end {
                        if m.is_word_mode(sprev, mode) {
                            fail = true;
                        }
                    } else if m.is_word_mode(s, mode) != m.is_word_mode(sprev, mode) {
                        fail = true;
                    }
                }
                if !fail {
                    p += 1;
                }
            }
            Op::WordBegin(mode) => {
                let mode = *mode;
                let mut ok = false;
                if s < right_range && m.is_word_mode(s, mode) {
                    if s == str_ {
                        ok = true;
                    } else {
                        let sprev = m.prev_char_head(str_, s).unwrap_or(str_);
                        if !m.is_word_mode(sprev, mode) {
                            ok = true;
                        }
                    }
                }
                if ok {
                    p += 1;
                } else {
                    fail = true;
                }
            }
            Op::WordEnd(mode) => {
                let mode = *mode;
                let mut ok = false;
                if s != str_ {
                    let sprev = m.prev_char_head(str_, s).unwrap_or(str_);
                    if m.is_word_mode(sprev, mode) && (s == end || !m.is_word_mode(s, mode)) {
                        ok = true;
                    }
                }
                if ok {
                    p += 1;
                } else {
                    fail = true;
                }
            }
            Op::TextSegmentBoundary(t, not) => {
                let sprev = m.prev_char_head(str_, s);
                let mut is_break = match t {
                    TextSegmentBoundaryType::ExtendedGraphemeCluster => {
                        egcb_is_break_position(enc, s_all, s, sprev, str_, end)
                    }
                    TextSegmentBoundaryType::Word => {
                        wb_is_break_position(enc, s_all, s, sprev, str_, end)
                    }
                };
                if *not {
                    is_break = !is_break;
                }
                if is_break {
                    p += 1;
                } else {
                    fail = true;
                }
            }
            Op::BeginBuf => {
                if s != str_
                    || (options & ONIG_OPTION_NOTBOL) != 0
                    || (options & ONIG_OPTION_NOT_BEGIN_STRING) != 0
                {
                    fail = true;
                } else {
                    p += 1;
                }
            }
            Op::EndBuf => {
                if s != end
                    || (options & ONIG_OPTION_NOTEOL) != 0
                    || (options & ONIG_OPTION_NOT_END_STRING) != 0
                {
                    fail = true;
                } else {
                    p += 1;
                }
            }
            Op::BeginLine => {
                if s == str_ {
                    if (options & ONIG_OPTION_NOTBOL) != 0 {
                        fail = true;
                    } else {
                        p += 1;
                    }
                } else if s != end {
                    let sprev = m.prev_char_head(str_, s).unwrap_or(str_);
                    if m.is_newline(sprev) {
                        p += 1;
                    } else {
                        fail = true;
                    }
                } else {
                    fail = true;
                }
            }
            Op::EndLine => {
                if s == end {
                    if (options & ONIG_OPTION_NOTEOL) != 0 {
                        fail = true;
                    } else {
                        p += 1;
                    }
                } else if m.is_newline(s) {
                    p += 1;
                } else {
                    fail = true;
                }
            }
            Op::SemiEndBuf => {
                if s == end {
                    if (options & ONIG_OPTION_NOTEOL) != 0
                        || (options & ONIG_OPTION_NOT_END_STRING) != 0
                    {
                        fail = true;
                    } else {
                        p += 1;
                    }
                } else if m.is_newline(s) && s + m.enclen(s) == end {
                    if (options & ONIG_OPTION_NOTEOL) != 0
                        || (options & ONIG_OPTION_NOT_END_STRING) != 0
                    {
                        fail = true;
                    } else {
                        p += 1;
                    }
                } else {
                    fail = true;
                }
            }
            Op::CheckPosition(t) => {
                match t {
                    CheckPositionType::SearchStart => {
                        if s != msa.start || (options & ONIG_OPTION_NOT_BEGIN_POSITION) != 0 {
                            fail = true;
                        }
                    }
                    CheckPositionType::CurrentRightRange => {
                        if s != right_range {
                            fail = true;
                        }
                    }
                }
                if !fail {
                    p += 1;
                }
            }
            Op::MemStartPush(mem) => {
                let mem = *mem as usize;
                let idx = st.v.len() as isize;
                push!(Stk {
                    typ: STK_MEM_START,
                    zid: mem as i32,
                    pstr: s,
                    prev_start: mem_start_stk[mem],
                    prev_end: mem_end_stk[mem],
                    ..Default::default()
                });
                mem_start_stk[mem] = idx;
                mem_end_stk[mem] = INVALID_STACK_INDEX;
                p += 1;
            }
            Op::MemStart(mem) => {
                mem_start_stk[*mem as usize] = s as isize;
                p += 1;
            }
            Op::MemEndPush(mem) => {
                let mem = *mem as usize;
                let idx = st.v.len() as isize;
                push!(Stk {
                    typ: STK_MEM_END,
                    zid: mem as i32,
                    pstr: s,
                    prev_start: mem_start_stk[mem],
                    prev_end: mem_end_stk[mem],
                    ..Default::default()
                });
                mem_end_stk[mem] = idx;
                p += 1;
            }
            Op::MemEnd(mem) => {
                mem_end_stk[*mem as usize] = s as isize;
                p += 1;
            }
            Op::MemEndPushRec(mem) => {
                let mem = *mem as usize;
                let si = stack_get_mem_start(&st.v, mem as i32);
                let idx = st.v.len() as isize;
                push!(Stk {
                    typ: STK_MEM_END,
                    zid: mem as i32,
                    pstr: s,
                    prev_start: mem_start_stk[mem],
                    prev_end: mem_end_stk[mem],
                    ..Default::default()
                });
                mem_end_stk[mem] = idx;
                mem_start_stk[mem] = si;
                p += 1;
            }
            Op::MemEndRec(mem) => {
                let mem = *mem as usize;
                mem_end_stk[mem] = s as isize;
                let si = stack_get_mem_start(&st.v, mem as i32);
                if mem_status_at(reg.push_mem_start, mem as i32) {
                    mem_start_stk[mem] = si;
                } else {
                    mem_start_stk[mem] = st.v[si as usize].pstr as isize;
                }
                push!(Stk {
                    typ: STK_MEM_END_MARK,
                    zid: mem as i32,
                    ..Default::default()
                });
                p += 1;
            }
            Op::Backref1 | Op::Backref2 | Op::BackrefN(_) => {
                let mem = match &ops[p] {
                    Op::Backref1 => 1usize,
                    Op::Backref2 => 2usize,
                    Op::BackrefN(n) => *n as usize,
                    _ => unreachable!(),
                };
                if mem_end_stk[mem] == INVALID_STACK_INDEX
                    || mem_start_stk[mem] == INVALID_STACK_INDEX
                {
                    fail = true;
                } else {
                    let pstart = stack_mem_start!(mem);
                    let pend = stack_mem_end!(mem);
                    let n = pend as isize - pstart as isize;
                    if n != 0 {
                        data_ensure!(n);
                        if !fail {
                            let n = n as usize;
                            if s_all[s..s + n] != s_all[pstart..pstart + n] {
                                /* STRING_CMP advances s while comparing; on
                                failure s does not matter (we backtrack) */
                                fail = true;
                            } else {
                                s += n;
                            }
                        }
                    }
                    if !fail {
                        p += 1;
                    }
                }
            }
            Op::BackrefNIc(mem) => {
                let mem = *mem as usize;
                if mem_end_stk[mem] == INVALID_STACK_INDEX
                    || mem_start_stk[mem] == INVALID_STACK_INDEX
                {
                    fail = true;
                } else {
                    let pstart = stack_mem_start!(mem);
                    let pend = stack_mem_end!(mem);
                    let n = pend as isize - pstart as isize;
                    if n != 0 {
                        data_ensure!(n);
                        if !fail
                            && !string_cmp_ic(
                                enc,
                                case_fold_flag,
                                s_all,
                                pstart,
                                &mut s,
                                n as usize,
                            )
                        {
                            fail = true;
                        }
                    }
                    if !fail {
                        p += 1;
                    }
                }
            }
            Op::BackrefMulti(ns) | Op::BackrefMultiIc(ns) => {
                let ic = matches!(&ops[p], Op::BackrefMultiIc(_));
                let mut found = false;
                for &mem in ns.iter() {
                    let mem = mem as usize;
                    if mem_end_stk[mem] == INVALID_STACK_INDEX
                        || mem_start_stk[mem] == INVALID_STACK_INDEX
                    {
                        continue;
                    }
                    let pstart = stack_mem_start!(mem);
                    let pend = stack_mem_end!(mem);
                    let n = pend as isize - pstart as isize;
                    if n != 0 {
                        if !(n <= right_range as isize - s as isize) {
                            continue;
                        }
                        let n = n as usize;
                        let mut swork = s;
                        if ic {
                            if !string_cmp_ic(enc, case_fold_flag, s_all, pstart, &mut swork, n) {
                                continue;
                            }
                        } else {
                            if s_all[s..s + n] != s_all[pstart..pstart + n] {
                                continue;
                            }
                            swork = s + n;
                        }
                        s = swork;
                    }
                    found = true;
                    break;
                }
                if !found {
                    fail = true;
                } else {
                    p += 1;
                }
            }
            Op::BackrefWithLevel(ns, level) | Op::BackrefWithLevelIc(ns, level) => {
                let ic = matches!(&ops[p], Op::BackrefWithLevelIc(..));
                if !m.backref_match_at_nested_level(
                    &st.v,
                    st.v.len(),
                    ic,
                    case_fold_flag,
                    *level,
                    ns,
                    &mut s,
                    end,
                ) {
                    fail = true;
                } else {
                    p += 1;
                }
            }
            Op::BackrefCheck(ns) => {
                let mut found = false;
                for &mem in ns.iter() {
                    let mem = mem as usize;
                    if mem_end_stk[mem] == INVALID_STACK_INDEX
                        || mem_start_stk[mem] == INVALID_STACK_INDEX
                    {
                        continue;
                    }
                    found = true;
                    break;
                }
                if !found {
                    fail = true;
                } else {
                    p += 1;
                }
            }
            Op::BackrefCheckWithLevel(ns, level) => {
                if !m.backref_check_at_nested_level(&st.v, st.v.len(), *level, ns) {
                    fail = true;
                } else {
                    p += 1;
                }
            }
            Op::EmptyCheckStart(mem) => {
                push!(Stk {
                    typ: STK_EMPTY_CHECK_START,
                    zid: *mem,
                    pstr: s,
                    ..Default::default()
                });
                p += 1;
            }
            Op::EmptyCheckEnd(mem) => {
                let k = empty_check_start_search(&st.v, *mem);
                let is_empty = st.v[k].pstr == s;
                p += 1;
                if is_empty {
                    p += 1;
                }
            }
            Op::EmptyCheckEndMemst(mem, esm) => {
                let is_empty = stack_empty_check_mem(&st.v, *mem, *esm, s);
                p += 1;
                if is_empty != 0 {
                    if is_empty == -1 {
                        fail = true;
                    } else {
                        p += 1;
                    }
                }
            }
            Op::EmptyCheckEndMemstPush(mem, esm) => {
                let mem = *mem;
                let is_empty = stack_empty_check_mem_rec(&st.v, mem, *esm, s);
                p += 1;
                if is_empty != 0 {
                    if is_empty == -1 {
                        fail = true;
                    } else {
                        p += 1;
                    }
                } else {
                    push!(Stk {
                        typ: STK_EMPTY_CHECK_END,
                        zid: mem,
                        ..Default::default()
                    });
                }
            }
            Op::Jump(addr) => {
                p = (p as isize + *addr as isize) as usize;
            }
            Op::Push(addr) => {
                let target = (p as isize + *addr as isize) as usize;
                push_alt!(target, s);
                p += 1;
            }
            Op::PushSuper(addr) => {
                let target = (p as isize + *addr as isize) as usize;
                push!(Stk {
                    typ: STK_SUPER_ALT,
                    pcode: target,
                    pstr: s,
                    ..Default::default()
                });
                p += 1;
            }
            Op::Pop => {
                st.v.pop();
                p += 1;
            }
            Op::PopToMark(id) => {
                let id = *id;
                loop {
                    let e = st.v.pop().unwrap();
                    if (e.typ & STK_MASK_POP_HANDLED_TIL) != 0 {
                        if e.typ == STK_MARK {
                            if e.zid == id {
                                break;
                            }
                        } else {
                            pop_handled!(e);
                        }
                    }
                }
                p += 1;
            }
            Op::PushOrJumpExact1(addr, c) => {
                let target = (p as isize + *addr as isize) as usize;
                if s < right_range && *c == m.byte(s) {
                    push_alt!(target, s);
                    p += 1;
                } else {
                    p = target;
                }
            }
            Op::PushIfPeekNext(addr, c) => {
                let target = (p as isize + *addr as isize) as usize;
                if s < right_range && *c == m.byte(s) {
                    push_alt!(target, s);
                }
                p += 1;
            }
            Op::Repeat(id, addr) | Op::RepeatNg(id, addr) => {
                let ng = matches!(&ops[p], Op::RepeatNg(..));
                let mem = *id;
                let target = (p as isize + *addr as isize) as usize;
                push!(Stk {
                    typ: STK_REPEAT_INC,
                    zid: mem,
                    count: 0,
                    ..Default::default()
                });
                if reg.repeat_range[mem as usize].lower == 0 {
                    if ng {
                        push_alt!(p + 1, s);
                        p = target;
                    } else {
                        push_alt!(target, s);
                        p += 1;
                    }
                } else {
                    p += 1;
                }
            }
            Op::RepeatInc(id) => {
                let mem = *id;
                let mut n = stack_get_repeat_count(&st.v, mem);
                n += 1;
                let rr = reg.repeat_range[mem as usize];
                if n >= rr.upper {
                    p += 1;
                } else if n >= rr.lower {
                    p += 1;
                    push_alt!(p, s);
                    p = rr.pcode;
                } else {
                    p = rr.pcode;
                }
                push!(Stk {
                    typ: STK_REPEAT_INC,
                    zid: mem,
                    count: n,
                    ..Default::default()
                });
            }
            Op::RepeatIncNg(id) => {
                let mem = *id;
                let mut n = stack_get_repeat_count(&st.v, mem);
                n += 1;
                push!(Stk {
                    typ: STK_REPEAT_INC,
                    zid: mem,
                    count: n,
                    ..Default::default()
                });
                let rr = reg.repeat_range[mem as usize];
                if n == rr.upper {
                    p += 1;
                } else if n >= rr.lower {
                    push_alt!(rr.pcode, s);
                    p += 1;
                } else {
                    p = rr.pcode;
                }
            }
            Op::Call(addr) => {
                if subexp_call_nest_counter == SUBEXP_CALL_MAX_NEST_LEVEL {
                    fail = true;
                } else {
                    subexp_call_nest_counter += 1;
                    let addr = *addr as usize;
                    p += 1;
                    push!(Stk {
                        typ: STK_CALL_FRAME,
                        pcode: p,
                        ..Default::default()
                    });
                    p = addr;
                }
            }
            Op::Return => {
                /* STACK_RETURN(p) */
                let mut level = 0;
                let mut k = st.v.len();
                loop {
                    k -= 1;
                    let e = &st.v[k];
                    if e.typ == STK_CALL_FRAME {
                        if level == 0 {
                            p = e.pcode;
                            break;
                        } else {
                            level -= 1;
                        }
                    } else if e.typ == STK_RETURN {
                        level += 1;
                    }
                }
                push!(Stk {
                    typ: STK_RETURN,
                    ..Default::default()
                });
                subexp_call_nest_counter = subexp_call_nest_counter.wrapping_sub(1);
            }
            Op::Move(n) => {
                let n = *n;
                if n < 0 {
                    match step_back(enc, s_all, str_, s, -n) {
                        Some(ns) => {
                            s = ns;
                            p += 1;
                        }
                        None => fail = true,
                    }
                } else {
                    let mut tlen = n;
                    while tlen > 0 {
                        let len = m.enclen(s);
                        s += len;
                        if s > end {
                            fail = true;
                            break;
                        }
                        if s == end {
                            if tlen != 1 {
                                fail = true;
                            }
                            break;
                        }
                        tlen -= 1;
                    }
                    if !fail {
                        p += 1;
                    }
                }
            }
            Op::StepBackStart(initial, remaining, addr) => {
                let tlen = *initial;
                if tlen != 0 {
                    match step_back(enc, s_all, str_, s, tlen) {
                        Some(ns) => s = ns,
                        None => fail = true,
                    }
                }
                if !fail {
                    if *remaining != 0 {
                        push!(Stk {
                            typ: STK_ALT,
                            zid: *remaining,
                            pcode: p + 1,
                            pstr: s,
                            ..Default::default()
                        });
                        p = (p as isize + *addr as isize) as usize;
                    } else {
                        p += 1;
                    }
                }
            }
            Op::StepBackNext => {
                /* stk->zid: the entry just popped by the fail (still in
                place above the stack top) */
                let mut tlen = popped_zid;
                if tlen != INFINITE_LEN as i32 {
                    tlen -= 1;
                }
                match step_back(enc, s_all, str_, s, 1) {
                    Some(ns) => {
                        s = ns;
                        if tlen != 0 {
                            push!(Stk {
                                typ: STK_ALT,
                                zid: tlen,
                                pcode: p,
                                pstr: s,
                                ..Default::default()
                            });
                        }
                        p += 1;
                    }
                    None => fail = true,
                }
            }
            Op::CutToMark(id, restore_pos) => {
                let id = *id;
                /* STACK_TO_VOID_TO_MARK */
                let mut k = st.v.len();
                loop {
                    k -= 1;
                    let e = &mut st.v[k];
                    if (e.typ & STK_MASK_TO_VOID_TARGET) != 0 {
                        if e.typ == STK_MARK {
                            if e.zid == id {
                                e.typ = STK_VOID;
                                break;
                            }
                        } else {
                            e.typ = STK_VOID;
                        }
                    }
                }
                if *restore_pos {
                    s = st.v[k].pstr;
                }
                p += 1;
            }
            Op::Mark(id, save_pos) => {
                push!(Stk {
                    typ: STK_MARK,
                    zid: *id,
                    pstr: if *save_pos { s } else { 0 },
                    ..Default::default()
                });
                p += 1;
            }
            Op::SaveVal(typ, id) => {
                let v = if *typ == SAVE_RIGHT_RANGE {
                    right_range
                } else {
                    s
                };
                push!(Stk {
                    typ: STK_SAVE_VAL,
                    zid: *id,
                    count: *typ,
                    pstr: v,
                    ..Default::default()
                });
                p += 1;
            }
            Op::UpdateVar(typ, id, clear) => {
                match *typ {
                    UPDATE_VAR_KEEP_FROM_STACK_LAST => {
                        let mut k = st.v.len();
                        while k > 0 {
                            k -= 1;
                            let e = &st.v[k];
                            if e.typ == STK_SAVE_VAL && e.count == SAVE_KEEP {
                                keep = e.pstr;
                                break;
                            }
                        }
                    }
                    UPDATE_VAR_S_FROM_STACK => {
                        if let Some(v) =
                            stack_get_save_val_type_last_id(&mut st.v, SAVE_S, *id, false)
                        {
                            s = v;
                        }
                    }
                    UPDATE_VAR_RIGHT_RANGE_FROM_S_STACK => {
                        if let Some(v) =
                            stack_get_save_val_type_last_id(&mut st.v, SAVE_S, *id, *clear)
                        {
                            right_range = v;
                        }
                    }
                    UPDATE_VAR_RIGHT_RANGE_FROM_STACK => {
                        if let Some(v) = stack_get_save_val_type_last_id(
                            &mut st.v,
                            SAVE_RIGHT_RANGE,
                            *id,
                            *clear,
                        ) {
                            right_range = v;
                        }
                    }
                    UPDATE_VAR_RIGHT_RANGE_TO_S => right_range = s,
                    UPDATE_VAR_RIGHT_RANGE_INIT => right_range = in_right_range,
                    _ => {}
                }
                p += 1;
            }
            Op::CalloutContents(_) | Op::CalloutName(..) => {
                /* no callout functions are set in PHP */
                p += 1;
            }
            Op::Fail => {
                fail = true;
            }
        }

        if fail {
            /* STACK_POP */
            loop {
                let e = match st.v.pop() {
                    Some(e) => e,
                    None => {
                        best_len = ONIGERR_STACK_BUG;
                        break 'interp;
                    }
                };
                if (e.typ & STK_MASK_POP_USED) != 0 {
                    p = e.pcode;
                    s = e.pstr;
                    popped_zid = e.zid;
                    break;
                }
                match pop_level {
                    STACK_POP_LEVEL_FREE => {}
                    STACK_POP_LEVEL_MEM_START => {
                        if e.typ == STK_MEM_START {
                            mem_start_stk[e.zid as usize] = e.prev_start;
                            mem_end_stk[e.zid as usize] = e.prev_end;
                        }
                    }
                    _ => {
                        if (e.typ & STK_MASK_POP_HANDLED) != 0 {
                            pop_handled!(e);
                        }
                    }
                }
            }
            retry_in_match_counter += 1;
            if retry_in_match_counter >= retry_limit_in_match && retry_limit_in_match != 0 {
                best_len = if retry_in_match_counter >= msa.retry_limit_in_match
                    && msa.retry_limit_in_match != 0
                {
                    ONIGERR_RETRY_LIMIT_IN_MATCH_OVER
                } else {
                    ONIGERR_RETRY_LIMIT_IN_SEARCH_OVER
                };
                break 'interp;
            }
        }
    }

    // match_at_end:
    if msa.retry_limit_in_search != 0 {
        msa.retry_limit_in_search_counter += retry_in_match_counter;
    }
    /* STACK_SAVE */
    msa.stack_n = st.cap;
    msa.has_stack = true;
    msa.stack = st.v;
    best_len
}

fn stack_get_mem_start(v: &[Stk], mnum: i32) -> isize {
    let mut level = 0;
    let mut k = v.len();
    while k > 0 {
        k -= 1;
        let e = &v[k];
        if (e.typ & STK_MASK_MEM_END_OR_MARK) != 0 && e.zid == mnum {
            level += 1;
        } else if e.typ == STK_MEM_START && e.zid == mnum {
            if level == 0 {
                break;
            }
            level -= 1;
        }
    }
    k as isize
}

fn empty_check_start_search(v: &[Stk], sid: i32) -> usize {
    let mut k = v.len();
    loop {
        k -= 1;
        if v[k].typ == STK_EMPTY_CHECK_START && v[k].zid == sid {
            return k;
        }
    }
}

fn mem_pstr(v: &[Stk], idx: isize) -> usize {
    v[idx as usize].pstr
}

/// STACK_EMPTY_CHECK_MEM: 1 empty, 0 not empty
fn stack_empty_check_mem(v: &[Stk], sid: i32, empty_status_mem: MemStatusType, s: usize) -> i32 {
    let klow = empty_check_start_search(v, sid);
    if v[klow].pstr != s {
        return 0;
    }
    let mut ms = empty_status_mem;
    let mut k = v.len();
    while k > klow {
        k -= 1;
        let ek = v[k];
        if ek.typ == STK_MEM_END && mem_status_limit_at(ms, ek.zid) {
            let mut kk = klow;
            while kk < k {
                let ekk = v[kk];
                if ekk.typ == STK_MEM_START && ekk.zid == ek.zid {
                    if ekk.prev_end == INVALID_STACK_INDEX
                        || ((mem_pstr(v, ekk.prev_end) != ek.pstr
                            || mem_pstr(v, ekk.prev_start) != mem_pstr(v, ek.prev_start))
                            && (mem_pstr(v, ek.prev_start) != ek.pstr
                                || mem_pstr(v, ekk.prev_start) != mem_pstr(v, ekk.prev_end)))
                    {
                        return 0;
                    } else {
                        ms &= !(1u32 << ek.zid);
                        break;
                    }
                }
                kk += 1;
            }
            if ms == 0 {
                break;
            }
        }
    }
    1
}

/// STACK_EMPTY_CHECK_MEM_REC
#[allow(unused_assignments)] // mirrors the C macro
fn stack_empty_check_mem_rec(
    v: &[Stk],
    sid: i32,
    empty_status_mem: MemStatusType,
    s: usize,
) -> i32 {
    let mut level = 0;
    let mut klow = v.len();
    loop {
        klow -= 1;
        let el = v[klow];
        if el.typ == STK_EMPTY_CHECK_START {
            if el.zid == sid {
                if level == 0 {
                    if el.pstr != s {
                        return 0;
                    }
                    if empty_status_mem == 0 {
                        return 1;
                    }
                    let mut ms = empty_status_mem;
                    let mut k = v.len();
                    while k > klow {
                        k -= 1;
                        let ek = v[k];
                        if ek.typ == STK_MEM_END {
                            if level == 0 && mem_status_limit_at(ms, ek.zid) {
                                let mut kk = klow + 1;
                                while kk < k {
                                    let ekk = v[kk];
                                    if ekk.typ == STK_MEM_START && ekk.zid == ek.zid {
                                        if ekk.prev_end == INVALID_STACK_INDEX
                                            || ((mem_pstr(v, ekk.prev_end) != ek.pstr
                                                || mem_pstr(v, ekk.prev_start)
                                                    != mem_pstr(v, ek.prev_start))
                                                && (mem_pstr(v, ek.prev_start) != ek.pstr
                                                    || mem_pstr(v, ekk.prev_start)
                                                        != mem_pstr(v, ekk.prev_end)))
                                        {
                                            return 0;
                                        } else {
                                            ms &= !(1u32 << ek.zid);
                                            break;
                                        }
                                    } else if ekk.typ == STK_EMPTY_CHECK_START {
                                        if ekk.zid == sid {
                                            level += 1;
                                        }
                                    } else if ekk.typ == STK_EMPTY_CHECK_END && ekk.zid == sid {
                                        level -= 1;
                                    }
                                    kk += 1;
                                }
                                level = 0;
                                if ms == 0 {
                                    break;
                                }
                            }
                        } else if ek.typ == STK_EMPTY_CHECK_START {
                            if ek.zid == sid {
                                level += 1;
                            }
                        } else if ek.typ == STK_EMPTY_CHECK_END && ek.zid == sid {
                            level -= 1;
                        }
                    }
                    return 1;
                } else {
                    level -= 1;
                }
            }
        } else if el.typ == STK_EMPTY_CHECK_END && el.zid == sid {
            level += 1;
        }
    }
}

fn stack_get_repeat_count(v: &[Stk], sid: i32) -> i32 {
    let mut k = v.len();
    loop {
        k -= 1;
        let e = &v[k];
        if e.typ == STK_REPEAT_INC {
            if e.zid == sid {
                return e.count;
            }
        } else if e.typ == STK_RETURN {
            let mut level = -1;
            loop {
                k -= 1;
                if v[k].typ == STK_CALL_FRAME {
                    level += 1;
                    if level == 0 {
                        break;
                    }
                } else if v[k].typ == STK_RETURN {
                    level -= 1;
                }
            }
        }
    }
}

fn stack_get_save_val_type_last_id(
    v: &mut [Stk],
    stype: i32,
    sid: i32,
    clear: bool,
) -> Option<usize> {
    let mut level = 0;
    let mut k = v.len();
    while k > 0 {
        k -= 1;
        let e = &mut v[k];
        if e.typ == STK_SAVE_VAL && e.count == stype && e.zid == sid {
            if level == 0 {
                let val = e.pstr;
                if clear {
                    e.typ = STK_VOID;
                }
                return Some(val);
            }
        } else if e.typ == STK_CALL_FRAME {
            level -= 1;
        } else if e.typ == STK_RETURN {
            level += 1;
        }
    }
    None
}

/* ===== search ===== */

fn slow_search(
    enc: &Encoding,
    s_all: &[u8],
    target: &[u8],
    text: usize,
    text_end: usize,
    text_range: usize,
) -> Option<usize> {
    let tlen = target.len();
    let mut end = text_end as isize - (tlen as isize - 1);
    if end > text_range as isize {
        end = text_range as isize;
    }
    let mut s = text;
    while (s as isize) < end {
        if s_all[s] == target[0] {
            let mut ok = true;
            for (i, &t) in target.iter().enumerate().skip(1) {
                if s_all.get(s + i).copied().unwrap_or(0) != t {
                    ok = false;
                    break;
                }
            }
            if ok {
                return Some(s);
            }
        }
        s += enc.mbc_enc_len(s_all, s);
    }
    None
}

fn slow_search_backward(
    enc: &Encoding,
    s_all: &[u8],
    target: &[u8],
    text: usize,
    adjust_text: usize,
    text_end: usize,
    text_start: usize,
) -> Option<usize> {
    let mut s: Option<usize>;
    let st = text_end as isize - target.len() as isize;
    if st > text_start as isize {
        s = Some(text_start);
    } else {
        s = Some(enc.left_adjust_char_head(s_all, adjust_text, st.max(0) as usize));
        if st < 0 {
            /* would be before the string start */
            s = None;
        }
    }
    while let Some(p) = s {
        if p < text {
            break;
        }
        if s_all.get(p).copied().unwrap_or(0) == target[0] {
            let mut ok = true;
            for (i, &t) in target.iter().enumerate().skip(1) {
                if s_all.get(p + i).copied().unwrap_or(0) != t {
                    ok = false;
                    break;
                }
            }
            if ok {
                return Some(p);
            }
        }
        s = get_prev_char_head(enc, s_all, adjust_text, p);
    }
    None
}

fn sunday_quick_search_step_forward(
    reg: &Regex,
    s_all: &[u8],
    text: usize,
    text_end: usize,
    text_range: usize,
) -> Option<usize> {
    let enc = reg.enc;
    let target = &reg.exact;
    let tlen1 = target.len() as isize - 1;
    let mut end = text_range as isize;
    if tlen1 > text_end as isize - end {
        end = text_end as isize - tlen1;
    }
    let map_offset = reg.map_offset as isize;
    let mut s = text as isize;
    while s < end {
        let se = s + tlen1;
        let mut pp = se;
        let mut t = tlen1;
        while s_all.get(pp as usize).copied().unwrap_or(0) == target[t as usize] {
            if t == 0 {
                return Some(s as usize);
            }
            pp -= 1;
            t -= 1;
        }
        if se + map_offset >= text_end as isize {
            break;
        }
        let skip = reg.map[s_all[(se + map_offset) as usize] as usize] as isize;
        let next = s + skip;
        if next < end {
            s = get_right_adjust_char_head(enc, s_all, s as usize, next as usize) as isize;
        } else {
            break;
        }
    }
    None
}

fn sunday_quick_search(
    reg: &Regex,
    s_all: &[u8],
    text: usize,
    text_end: usize,
    text_range: usize,
) -> Option<usize> {
    let target = &reg.exact;
    let map_offset = reg.map_offset as isize;
    let target_len = target.len() as isize;
    let tail = target_len - 1;
    let end;
    if target_len > text_end as isize - text_range as isize {
        end = text_end as isize;
        if target_len > text_end as isize - text as isize {
            return None;
        }
    } else {
        end = text_range as isize + target_len;
    }
    let mut s = text as isize + target_len - 1;
    if s < end {
        loop {
            let mut pp = s;
            let mut t = tail;
            while s_all.get(pp as usize).copied().unwrap_or(0) == target[t as usize] {
                if t == 0 {
                    return Some(pp as usize);
                }
                pp -= 1;
                t -= 1;
            }
            if text_end as isize - s <= map_offset {
                break;
            }
            let skip = reg.map[s_all[(s + map_offset) as usize] as usize] as isize;
            if skip >= end - s {
                break;
            }
            s += skip;
        }
    }
    None
}

fn map_search(
    enc: &Encoding,
    map: &[u8; CHAR_MAP_SIZE],
    s_all: &[u8],
    text: usize,
    text_range: usize,
) -> Option<usize> {
    let mut s = text;
    while s < text_range {
        if map[s_all[s] as usize] != 0 {
            return Some(s);
        }
        s += enc.mbc_enc_len(s_all, s);
    }
    None
}

fn map_search_backward(
    enc: &Encoding,
    map: &[u8; CHAR_MAP_SIZE],
    s_all: &[u8],
    text: usize,
    adjust_text: usize,
    text_start: usize,
) -> Option<usize> {
    let mut s = Some(text_start);
    while let Some(p) = s {
        if p < text {
            break;
        }
        if map[s_all.get(p).copied().unwrap_or(0) as usize] != 0 {
            return Some(p);
        }
        s = get_prev_char_head(enc, s_all, adjust_text, p);
    }
    None
}

fn forward_search(
    reg: &Regex,
    s_all: &[u8],
    end: usize,
    start: usize,
    range: usize,
) -> Option<(usize, usize)> {
    let enc = reg.enc;
    let str_ = 0usize;
    let mut p = start;
    let mut pprev: Option<usize> = None;
    if reg.dist_min != 0 {
        if (end - p) as u64 <= reg.dist_min as u64 {
            return None;
        }
        if enc.is_singlebyte() {
            p += reg.dist_min as usize;
        } else {
            let q = p + reg.dist_min as usize;
            while p < q {
                p += enc.mbc_enc_len(s_all, p);
            }
        }
    }

    loop {
        // retry:
        let found = match reg.optimize {
            OPTIMIZE_STR => slow_search(enc, s_all, &reg.exact, p, end, range),
            OPTIMIZE_STR_FAST => sunday_quick_search(reg, s_all, p, end, range),
            OPTIMIZE_STR_FAST_STEP_FORWARD => {
                sunday_quick_search_step_forward(reg, s_all, p, end, range)
            }
            OPTIMIZE_MAP => map_search(enc, &reg.map, s_all, p, range),
            _ => None,
        };
        let fp = match found {
            Some(x) if x < range => x,
            _ => return None,
        };
        p = fp;

        let mut retry_gate = false;
        if ((p - start) as u64) < reg.dist_min as u64 {
            retry_gate = true;
        } else if reg.sub_anchor != 0 {
            match reg.sub_anchor {
                ANCR_BEGIN_LINE => {
                    if p != str_ {
                        let prev = get_prev_char_head(enc, s_all, pprev.unwrap_or(str_), p);
                        let nl = prev
                            .map(|x| enc.is_mbc_newline(s_all, x, end))
                            .unwrap_or(false);
                        if !nl {
                            retry_gate = true;
                        }
                    }
                }
                ANCR_END_LINE => {
                    if p == end {
                        let prev = get_prev_char_head(enc, s_all, pprev.unwrap_or(str_), p);
                        if let Some(prev) = prev {
                            if enc.is_mbc_newline(s_all, prev, end) {
                                retry_gate = true;
                            }
                        }
                    } else if !enc.is_mbc_newline(s_all, p, end) {
                        retry_gate = true;
                    }
                }
                _ => {}
            }
        }
        if retry_gate {
            pprev = Some(p);
            p += enc.mbc_enc_len(s_all, p);
            continue;
        }

        let low;
        let high;
        if reg.dist_max == 0 {
            low = p;
            high = p;
        } else {
            let mut lo = 0usize;
            if reg.dist_max != INFINITE_LEN {
                if ((p - str_) as u64) < reg.dist_max as u64 {
                    lo = str_;
                } else {
                    lo = p - reg.dist_max as usize;
                    if lo > start {
                        lo = get_right_adjust_char_head(enc, s_all, start, lo);
                    }
                }
            }
            low = lo;
            if ((p - str_) as u64) < reg.dist_min as u64 {
                high = str_;
            } else {
                high = p - reg.dist_min as usize;
            }
        }
        return Some((low, high));
    }
}

fn backward_search(
    reg: &Regex,
    s_all: &[u8],
    end: usize,
    s: usize,
    range: usize,
    adjrange: usize,
) -> Option<(usize, usize)> {
    let enc = reg.enc;
    let str_ = 0usize;
    let mut p = Some(s);
    loop {
        // retry:
        let found = match reg.optimize {
            OPTIMIZE_STR | OPTIMIZE_STR_FAST | OPTIMIZE_STR_FAST_STEP_FORWARD => {
                slow_search_backward(enc, s_all, &reg.exact, range, adjrange, end, p?)
            }
            OPTIMIZE_MAP => map_search_backward(enc, &reg.map, s_all, range, adjrange, p?),
            _ => None,
        };
        let fp = found?;
        let pp = fp;
        if reg.sub_anchor != 0 {
            match reg.sub_anchor {
                ANCR_BEGIN_LINE => {
                    if pp != str_ {
                        let prev = get_prev_char_head(enc, s_all, str_, pp);
                        if let Some(prev) = prev {
                            if !enc.is_mbc_newline(s_all, prev, end) {
                                p = Some(prev);
                                continue;
                            }
                        }
                    }
                }
                ANCR_END_LINE => {
                    if pp == end {
                        let prev = get_prev_char_head(enc, s_all, adjrange, pp)?;
                        if enc.is_mbc_newline(s_all, prev, end) {
                            p = Some(prev);
                            continue;
                        }
                    } else if !enc.is_mbc_newline(s_all, pp, end) {
                        let np = get_prev_char_head(enc, s_all, adjrange, pp)?;
                        p = Some(np);
                        continue;
                    }
                }
                _ => {}
            }
        }
        let mut low = 0;
        let mut high = 0;
        if reg.dist_max != INFINITE_LEN {
            if ((pp - str_) as u64) < reg.dist_max as u64 {
                low = str_;
            } else {
                low = pp - reg.dist_max as usize;
            }
            if reg.dist_min != 0 {
                if ((pp - str_) as u64) < reg.dist_min as u64 {
                    high = str_;
                } else {
                    high = pp - reg.dist_min as usize;
                }
            } else {
                high = pp;
            }
            high = get_right_adjust_char_head(enc, s_all, adjrange, high);
        }
        return Some((low, high));
    }
}

/// onig_search_with_param / search_in_range: returns the match position
/// (>= 0), ONIG_MISMATCH or an error code.
pub fn onig_search(
    reg: &Regex,
    s_all: &[u8],
    end: usize,
    start: usize,
    range: usize,
    mut region: Option<&mut Region>,
    option: u32,
    mp: &MatchParam,
) -> i32 {
    let data_range = if range > start { range } else { end };
    let str_ = 0usize;
    let mut start = start;
    let mut range = range;
    let orig_start_init = start;
    let enc = reg.enc;

    if let Some(r) = region.as_deref_mut() {
        r.resize_clear((reg.num_mem + 1) as usize);
    }

    if start > end {
        return ONIG_MISMATCH;
    }

    if (option & ONIG_OPTION_CHECK_VALIDITY_OF_STRING) != 0
        && !enc.is_valid_mbc_string(s_all, str_, end)
    {
        return ONIGERR_INVALID_WIDE_CHAR_VALUE;
    }

    let forward = range > start;

    if reg.anchor != 0 && str_ < end {
        let mut min_semi_end = end;
        let mut max_semi_end = end;
        let mut goto_end_buf = false;
        if (reg.anchor & ANCR_BEGIN_POSITION) != 0 {
            range = if range > start { start + 1 } else { start };
        } else if (reg.anchor & ANCR_BEGIN_BUF) != 0 {
            if range > start {
                if start != str_ {
                    return ONIG_MISMATCH;
                }
                range = str_ + 1;
            } else if range <= str_ {
                start = str_;
                range = str_;
            } else {
                return ONIG_MISMATCH;
            }
        } else if (reg.anchor & ANCR_END_BUF) != 0 {
            min_semi_end = end;
            max_semi_end = end;
            goto_end_buf = true;
        } else if (reg.anchor & ANCR_SEMI_END_BUF) != 0 {
            let pre_end = step_back(enc, s_all, str_, end, 1);
            max_semi_end = end;
            let nl = pre_end
                .map(|x| enc.is_mbc_newline(s_all, x, end))
                .unwrap_or(false);
            if nl {
                min_semi_end = pre_end.unwrap();
                if min_semi_end > str_ && start <= min_semi_end {
                    goto_end_buf = true;
                }
            } else {
                min_semi_end = end;
                goto_end_buf = true;
            }
        } else if (reg.anchor & ANCR_ANYCHAR_INF_ML) != 0 && range > start {
            range = start + 1;
        }

        if goto_end_buf {
            // end_buf:
            if ((max_semi_end - str_) as u64) < reg.anc_dist_min as u64 {
                return ONIG_MISMATCH;
            }
            if range > start {
                if reg.anc_dist_max != INFINITE_LEN
                    && (min_semi_end as i64 - start as i64) > reg.anc_dist_max as i64
                {
                    start = min_semi_end - reg.anc_dist_max as usize;
                    if start < end {
                        start = get_right_adjust_char_head(enc, s_all, str_, start);
                    }
                }
                if ((max_semi_end as i64 - (range as i64 - 1)) as u64) < reg.anc_dist_min as u64 {
                    if ((max_semi_end - str_ + 1) as u64) < reg.anc_dist_min as u64 {
                        return ONIG_MISMATCH;
                    } else {
                        range = max_semi_end - reg.anc_dist_min as usize + 1;
                    }
                }
                if start > range {
                    return ONIG_MISMATCH;
                }
            } else {
                if reg.anc_dist_max != INFINITE_LEN
                    && (min_semi_end as i64 - range as i64) > reg.anc_dist_max as i64
                {
                    range = min_semi_end - reg.anc_dist_max as usize;
                }
                if ((max_semi_end - start) as u64) < reg.anc_dist_min as u64 {
                    if ((max_semi_end - str_) as u64) < reg.anc_dist_min as u64 {
                        return ONIG_MISMATCH;
                    } else {
                        start = max_semi_end - reg.anc_dist_min as usize;
                        start = enc.left_adjust_char_head(s_all, str_, start);
                    }
                }
                if range > start {
                    return ONIG_MISMATCH;
                }
            }
        }
    } else if str_ == end {
        /* empty string */
        if reg.threshold_len == 0 {
            let mut msa = MatchArg::new(reg, option, start, mp);
            let r = match_at(reg, s_all, end, end, start, &mut msa, &mut region);
            if r != ONIG_MISMATCH {
                if r >= 0 {
                    return start as i32;
                }
                return r;
            }
            return finish_mismatch(reg, &msa, &mut region, str_);
        }
        return ONIG_MISMATCH;
    }

    let mut msa = MatchArg::new(reg, option, orig_start_init, mp);
    let mut s = start;

    macro_rules! match_and_return_check {
        ($upper:expr) => {{
            // C steps past `end` when the search starts inside the last
            // character and then matches against the bytes behind the
            // subject (the NUL terminator and heap garbage); PHP never
            // reports such a match, so treat that position as a mismatch.
            let r = if s > end {
                ONIG_MISMATCH
            } else {
                match_at(reg, s_all, end, $upper, s, &mut msa, &mut region)
            };
            if r != ONIG_MISMATCH {
                if r >= 0 {
                    return (s - str_) as i32;
                } else {
                    return finish_error(reg, &mut region, r);
                }
            }
        }};
    }

    'search: {
        if forward {
            if reg.optimize != OPTIMIZE_NONE {
                let sch_range = if reg.dist_max != 0 {
                    if reg.dist_max == INFINITE_LEN {
                        end
                    } else if ((end - range) as u64) < reg.dist_max as u64 {
                        end
                    } else {
                        range + reg.dist_max as usize
                    }
                } else {
                    range
                };

                if ((end - start) as i64) < reg.threshold_len as i64 {
                    break 'search;
                }

                if reg.dist_max != INFINITE_LEN {
                    loop {
                        let (low, high) = match forward_search(reg, s_all, end, s, sch_range) {
                            Some(x) => x,
                            None => break 'search,
                        };
                        if s < low {
                            s = low;
                        }
                        while s <= high {
                            match_and_return_check!(data_range);
                            s += enc.mbc_enc_len(s_all, s);
                            if s < msa.skip_search {
                                s = msa.skip_search;
                            }
                        }
                        if s >= range {
                            break;
                        }
                    }
                    break 'search;
                } else {
                    if forward_search(reg, s_all, end, s, sch_range).is_none() {
                        break 'search;
                    }
                    if (reg.anchor & ANCR_ANYCHAR_INF) != 0
                        && (reg.anchor & (ANCR_LOOK_BEHIND | ANCR_PREC_READ_NOT)) == 0
                    {
                        while s < range {
                            match_and_return_check!(data_range);
                            let mut prev = s;
                            s += enc.mbc_enc_len(s_all, s);
                            if s < msa.skip_search {
                                s = msa.skip_search;
                            } else {
                                while !enc.is_mbc_newline(s_all, prev, end) && s < range {
                                    prev = s;
                                    s += enc.mbc_enc_len(s_all, s);
                                }
                            }
                        }
                        break 'search;
                    }
                }
            }

            loop {
                match_and_return_check!(data_range);
                if s >= range {
                    break;
                }
                s += enc.mbc_enc_len(s_all, s);
                if s < msa.skip_search {
                    s = msa.skip_search;
                    if s > range {
                        break;
                    }
                }
            }
        } else {
            /* backward search */
            if range < str_ {
                break 'search;
            }
            let mut orig_start = orig_start_init;
            if orig_start < end {
                orig_start += enc.mbc_enc_len(s_all, orig_start);
            }
            if reg.optimize != OPTIMIZE_NONE {
                if ((end - range) as i64) < reg.threshold_len as i64 {
                    break 'search;
                }
                let adjrange = if range < end {
                    enc.left_adjust_char_head(s_all, str_, range)
                } else {
                    end
                };
                let min_range = if ((end - range) as u64) > reg.dist_min as u64 {
                    range + reg.dist_min as usize
                } else {
                    end
                };
                if reg.dist_max != INFINITE_LEN {
                    loop {
                        let sch_start = if ((end - s) as u64) > reg.dist_max as u64 {
                            s + reg.dist_max as usize
                        } else {
                            get_prev_char_head(enc, s_all, str_, end).unwrap_or(str_)
                        };
                        let (low, high) = match backward_search(
                            reg, s_all, end, sch_start, min_range, adjrange,
                        ) {
                            Some(x) => x,
                            None => break 'search,
                        };
                        if s > high {
                            s = high;
                        }
                        let mut cur = Some(s);
                        while let Some(cs) = cur {
                            if cs < low {
                                break;
                            }
                            s = cs;
                            match_and_return_check!(orig_start);
                            cur = get_prev_char_head(enc, s_all, str_, cs);
                        }
                        match cur {
                            Some(cs) if cs >= range => s = cs,
                            _ => break,
                        }
                    }
                    break 'search;
                } else {
                    let sch_start = get_prev_char_head(enc, s_all, str_, end).unwrap_or(str_);
                    if backward_search(reg, s_all, end, sch_start, min_range, adjrange).is_none() {
                        break 'search;
                    }
                }
            }
            let mut cur = Some(s);
            while let Some(cs) = cur {
                s = cs;
                match_and_return_check!(orig_start);
                cur = get_prev_char_head(enc, s_all, str_, cs);
                match cur {
                    Some(x) if x >= range => {}
                    _ => break,
                }
            }
        }
    }

    // mismatch:
    finish_mismatch(reg, &msa, &mut region, str_)
}

fn finish_mismatch(
    reg: &Regex,
    msa: &MatchArg,
    region: &mut Option<&mut Region>,
    str_: usize,
) -> i32 {
    if opton_find_longest(reg.options) && msa.best_len >= 0 {
        return (msa.best_s - str_) as i32;
    }
    if opton_find_not_empty(reg.options) {
        if let Some(r) = region.as_deref_mut() {
            r.clear();
        }
    }
    ONIG_MISMATCH
}

fn finish_error(reg: &Regex, region: &mut Option<&mut Region>, r: i32) -> i32 {
    if opton_find_not_empty(reg.options) {
        if let Some(rg) = region.as_deref_mut() {
            rg.clear();
        }
    }
    r
}

/// onig_match_with_param
pub fn onig_match(
    reg: &Regex,
    s_all: &[u8],
    end: usize,
    at: usize,
    mut region: Option<&mut Region>,
    option: u32,
    mp: &MatchParam,
) -> i32 {
    let mut msa = MatchArg::new(reg, option, at, mp);
    if let Some(r) = region.as_deref_mut() {
        r.resize_clear((reg.num_mem + 1) as usize);
    }
    if (option & ONIG_OPTION_CHECK_VALIDITY_OF_STRING) != 0
        && !reg.enc.is_valid_mbc_string(s_all, 0, end)
    {
        return ONIGERR_INVALID_WIDE_CHAR_VALUE;
    }
    let mut r = match_at(reg, s_all, end, end, at, &mut msa, &mut region);
    if opton_find_longest(option) && r == ONIG_MISMATCH && msa.best_len >= 0 {
        r = msa.best_len;
    }
    r
}
