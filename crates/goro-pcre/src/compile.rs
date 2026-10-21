//! Second compile phase (pcre2_compile.c): compile_branch(), compile_regex()
//! and the class-building helpers, which turn the parsed pattern into the
//! byte code interpreted by the matcher.

use crate::cb::Cb;
use crate::consts::*;
use crate::parse::{PC_GRAPH, PC_PRINT, PC_PUNCT};
use crate::tables;
use crate::util::*;

static CHARTYPEOFFSET: [u32; 4] = [
    0,
    OP_STARI - OP_STAR,
    OP_NOTSTAR - OP_STAR,
    OP_NOTSTARI - OP_STAR,
];

static VERBOPS: [u32; 11] = [
    OP_MARK,
    OP_ACCEPT,
    OP_FAIL,
    OP_COMMIT,
    OP_COMMIT_ARG,
    OP_PRUNE,
    OP_PRUNE_ARG,
    OP_SKIP,
    OP_SKIP_ARG,
    OP_THEN,
    OP_THEN_ARG,
];

const CBIT_SPACE: i32 = 0;
const CBIT_XDIGIT: i32 = 32;
const CBIT_DIGIT: i32 = 64;
const CBIT_UPPER: i32 = 96;
const CBIT_LOWER: i32 = 128;
const CBIT_WORD: i32 = 160;
const CBIT_GRAPH: i32 = 192;
const CBIT_PRINT: i32 = 224;
const CBIT_PUNCT: i32 = 256;
const CBIT_CNTRL: i32 = 288;

#[rustfmt::skip]
static POSIX_CLASS_MAPS: [i32; 42] = [
    CBIT_WORD, CBIT_DIGIT, -2,
    CBIT_LOWER, -1, 0,
    CBIT_UPPER, -1, 0,
    CBIT_WORD, -1, 2,
    CBIT_PRINT, CBIT_CNTRL, 0,
    CBIT_SPACE, -1, 1,
    CBIT_CNTRL, -1, 0,
    CBIT_DIGIT, -1, 0,
    CBIT_GRAPH, -1, 0,
    CBIT_PRINT, -1, 0,
    CBIT_PUNCT, -1, 0,
    CBIT_SPACE, -1, 0,
    CBIT_WORD, -1, 0,
    CBIT_XDIGIT, -1, 0,
];

/// opcode_possessify[]: possessive versions of the repeat opcodes.
fn opcode_possessify(op: u32) -> u32 {
    match op {
        OP_STAR => OP_POSSTAR,
        OP_PLUS => OP_POSPLUS,
        OP_QUERY => OP_POSQUERY,
        OP_UPTO => OP_POSUPTO,
        OP_STARI => OP_POSSTARI,
        OP_PLUSI => OP_POSPLUSI,
        OP_QUERYI => OP_POSQUERYI,
        OP_UPTOI => OP_POSUPTOI,
        OP_NOTSTAR => OP_NOTPOSSTAR,
        OP_NOTPLUS => OP_NOTPOSPLUS,
        OP_NOTQUERY => OP_NOTPOSQUERY,
        OP_NOTUPTO => OP_NOTPOSUPTO,
        OP_NOTSTARI => OP_NOTPOSSTARI,
        OP_NOTPLUSI => OP_NOTPOSPLUSI,
        OP_NOTQUERYI => OP_NOTPOSQUERYI,
        OP_NOTUPTOI => OP_NOTPOSUPTOI,
        OP_TYPESTAR => OP_TYPEPOSSTAR,
        OP_TYPEPLUS => OP_TYPEPOSPLUS,
        OP_TYPEQUERY => OP_TYPEPOSQUERY,
        OP_TYPEUPTO => OP_TYPEPOSUPTO,
        OP_CRSTAR => OP_CRPOSSTAR,
        OP_CRPLUS => OP_CRPOSPLUS,
        OP_CRQUERY => OP_CRPOSQUERY,
        OP_CRRANGE => OP_CRPOSRANGE,
        _ => 0,
    }
}

#[inline]
fn setbit(a: &mut [u8; 32], b: u32) {
    a[(b / 8) as usize] |= 1u8 << (b & 7);
}

/// first_significant_code()
pub fn first_significant_code(code: &[u8], mut p: usize, skipassert: bool) -> usize {
    loop {
        let op = code[p] as u32;
        match op {
            OP_ASSERT_NOT | OP_ASSERTBACK | OP_ASSERTBACK_NOT | OP_ASSERTBACK_NA => {
                if !skipassert {
                    return p;
                }
                loop {
                    p += get(code, p + 1);
                    if code[p] as u32 != OP_ALT {
                        break;
                    }
                }
                p += op_len(code[p]);
            }
            OP_WORD_BOUNDARY
            | OP_NOT_WORD_BOUNDARY
            | OP_UCP_WORD_BOUNDARY
            | OP_NOT_UCP_WORD_BOUNDARY => {
                if !skipassert {
                    return p;
                }
                p += op_len(code[p]);
            }
            OP_CALLOUT | OP_CREF | OP_DNCREF | OP_RREF | OP_DNRREF | OP_FALSE | OP_TRUE => {
                p += op_len(code[p]);
            }
            OP_CALLOUT_STR => p += get(code, p + 1 + 2 * LINK_SIZE),
            OP_SKIPZERO => p += 2 + get(code, p + 2) + LINK_SIZE,
            OP_COND | OP_SCOND => {
                if code[p + 1 + LINK_SIZE] as u32 != OP_FALSE
                    || code[p + get(code, p + 1)] as u32 != OP_KET
                {
                    return p;
                }
                p += get(code, p + 1) + 1 + LINK_SIZE;
            }
            OP_MARK | OP_COMMIT_ARG | OP_PRUNE_ARG | OP_SKIP_ARG | OP_THEN_ARG => {
                p += code[p + 1] as usize + op_len(code[p]);
            }
            _ => return p,
        }
    }
}

/// get_othercase_range()
fn get_othercase_range(
    cptr: &mut u32,
    d: u32,
    ocptr: &mut u32,
    odptr: &mut u32,
    restricted: bool,
) -> i32 {
    let mut c = *cptr;
    let mut othercase = 0;
    while c <= d {
        let co = ucd_caseset(c);
        if co != 0 && (!restricted || tables::UCD_CASELESS_SETS[co as usize] > 127) {
            *ocptr = c;
            c += 1;
            *cptr = c;
            return co as i32;
        }
        othercase = ucd_othercase(c);
        if othercase != c {
            break;
        }
        c += 1;
    }
    if c > d {
        return -1;
    }
    *ocptr = othercase;
    let mut next = othercase + 1;
    c += 1;
    while c <= d {
        if ucd_caseset(c) != 0 || ucd_othercase(c) != next {
            break;
        }
        next += 1;
        c += 1;
    }
    *odptr = next - 1;
    *cptr = c;
    0
}

/// Values returned by compile_branch/compile_regex besides the code.
#[derive(Default, Clone, Copy)]
pub struct FirstReq {
    pub firstcu: u32,
    pub firstcuflags: u32,
    pub reqcu: u32,
    pub reqcuflags: u32,
}

impl<'a> Cb<'a> {
    #[inline]
    pub(crate) fn cc(&self, i: usize) -> u32 {
        self.code[i] as u32
    }
    #[inline]
    fn wc(&mut self, i: usize, v: u32) {
        self.code[i] = v as u8;
    }
    #[inline]
    fn pp(&self, i: usize) -> u32 {
        self.parsed[i]
    }
    #[inline]
    fn getoffset_at(&self, i: usize) -> usize {
        ((self.parsed[i] as usize) << 32) | self.parsed[i + 1] as usize
    }

    /// PUTCHAR
    fn putchar(&mut self, c: u32, at: usize, utf: bool) -> usize {
        if utf && c > 127 {
            let mut buf = [0u8; 8];
            let n = ord2utf(c, &mut buf);
            self.code[at..at + n].copy_from_slice(&buf[..n]);
            n
        } else {
            self.code[at] = c as u8;
            1
        }
    }

    fn ord2utf_at(&mut self, c: u32, at: usize) -> usize {
        let mut buf = [0u8; 8];
        let n = ord2utf(c, &mut buf);
        self.code[at..at + n].copy_from_slice(&buf[..n]);
        n
    }

    /// add_to_class_internal()
    #[allow(clippy::too_many_arguments)]
    fn add_to_class_internal(
        &mut self,
        classbits: &mut [u8; 32],
        uchardptr: &mut usize,
        mut options: u32,
        xoptions: u32,
        mut start: u32,
        mut end: u32,
    ) -> u32 {
        let mut classbits_end = if end <= 0xff { end } else { 0xff };
        let mut n8: u32 = 0;
        if (options & PCRE2_CASELESS) != 0 {
            if (options & (PCRE2_UTF | PCRE2_UCP)) != 0 {
                let mut oc = 0;
                let mut od = 0;
                options &= !PCRE2_CASELESS;
                let mut c = start;
                loop {
                    let rc = get_othercase_range(
                        &mut c,
                        end,
                        &mut oc,
                        &mut od,
                        (xoptions & PCRE2_EXTRA_CASELESS_RESTRICT) != 0,
                    );
                    if rc < 0 {
                        break;
                    }
                    if rc > 0 {
                        n8 += self.add_list_to_class_internal(
                            classbits,
                            uchardptr,
                            options,
                            xoptions,
                            &tables::UCD_CASELESS_SETS[rc as usize..],
                            oc,
                        );
                    } else if oc >= self.class_range_start && od <= self.class_range_end {
                        continue;
                    } else if oc < start && od >= start.wrapping_sub(1) {
                        start = oc;
                    } else if od > end && oc <= end + 1 {
                        end = od;
                        if end > classbits_end {
                            classbits_end = if end <= 0xff { end } else { 0xff };
                        }
                    } else {
                        n8 += self
                            .add_to_class_internal(classbits, uchardptr, options, xoptions, oc, od);
                    }
                }
            } else {
                let mut c = start;
                while c <= classbits_end {
                    setbit(classbits, self.fcc(c));
                    n8 += 1;
                    c += 1;
                }
            }
        }
        if (options & PCRE2_UTF) == 0 && end > MAX_NON_UTF_CHAR {
            end = MAX_NON_UTF_CHAR;
        }
        if start > self.class_range_start && end < self.class_range_end {
            return n8;
        }
        let mut c = start;
        while c <= classbits_end {
            setbit(classbits, c);
            n8 += 1;
            c += 1;
        }
        if start <= 0xff {
            start = 0x100;
        }
        if end >= start && (options & PCRE2_UTF) != 0 {
            let mut u = *uchardptr;
            if start < end {
                self.code[u] = XCL_RANGE as u8;
                u += 1;
                u += self.ord2utf_at(start, u);
                u += self.ord2utf_at(end, u);
            } else if start == end {
                self.code[u] = XCL_SINGLE as u8;
                u += 1;
                u += self.ord2utf_at(start, u);
            }
            *uchardptr = u;
        }
        n8
    }

    /// add_list_to_class_internal(): `p` is a NOTACHAR-terminated list.
    fn add_list_to_class_internal(
        &mut self,
        classbits: &mut [u8; 32],
        uchardptr: &mut usize,
        options: u32,
        xoptions: u32,
        p: &[u32],
        except: u32,
    ) -> u32 {
        let mut n8 = 0;
        let mut i = 0;
        while p[i] < NOTACHAR {
            let mut n = 0;
            if p[i] != except {
                while p[i + n + 1] == p[i] + n as u32 + 1 {
                    n += 1;
                }
                n8 += self.add_to_class_internal(
                    classbits,
                    uchardptr,
                    options,
                    xoptions,
                    p[i],
                    p[i + n],
                );
            }
            i += n + 1;
        }
        n8
    }

    fn add_to_class(
        &mut self,
        classbits: &mut [u8; 32],
        uchardptr: &mut usize,
        options: u32,
        xoptions: u32,
        start: u32,
        end: u32,
    ) -> u32 {
        self.class_range_start = start;
        self.class_range_end = end;
        self.add_to_class_internal(classbits, uchardptr, options, xoptions, start, end)
    }

    fn add_list_to_class(
        &mut self,
        classbits: &mut [u8; 32],
        uchardptr: &mut usize,
        options: u32,
        xoptions: u32,
        p: &[u32],
        except: u32,
    ) -> u32 {
        let mut n8 = 0;
        let mut i = 0;
        while p[i] < NOTACHAR {
            let mut n = 0;
            if p[i] != except {
                while p[i + n + 1] == p[i] + n as u32 + 1 {
                    n += 1;
                }
                self.class_range_start = p[i];
                self.class_range_end = p[i + n];
                n8 += self.add_to_class_internal(
                    classbits,
                    uchardptr,
                    options,
                    xoptions,
                    p[i],
                    p[i + n],
                );
            }
            i += n + 1;
        }
        n8
    }

    fn add_not_list_to_class(
        &mut self,
        classbits: &mut [u8; 32],
        uchardptr: &mut usize,
        options: u32,
        xoptions: u32,
        p: &[u32],
    ) -> u32 {
        let utf = (options & PCRE2_UTF) != 0;
        let mut n8 = 0;
        let mut i = 0;
        if p[0] > 0 {
            n8 += self.add_to_class(classbits, uchardptr, options, xoptions, 0, p[0] - 1);
        }
        while p[i] < NOTACHAR {
            while p[i + 1] == p[i] + 1 {
                i += 1;
            }
            let hi = if p[i + 1] == NOTACHAR {
                if utf { 0x10ffff } else { 0xffffffff }
            } else {
                p[i + 1] - 1
            };
            n8 += self.add_to_class(classbits, uchardptr, options, xoptions, p[i] + 1, hi);
            i += 1;
        }
        n8
    }

    /// find_dupname_details()
    fn find_dupname_details(
        &mut self,
        name: usize,
        length: u32,
        indexptr: &mut i32,
        countptr: &mut i32,
        errorcodeptr: &mut i32,
    ) -> bool {
        let nm = &self.pat[name..name + length as usize];
        let ese = self.name_entry_size as usize;
        let mut slot = self.name_table;
        let mut i = 0usize;
        while i < self.names_found as usize {
            if &self.code[slot + IMM2_SIZE..slot + IMM2_SIZE + length as usize] == nm
                && self.code[slot + IMM2_SIZE + length as usize] == 0
            {
                break;
            }
            slot += ese;
            i += 1;
        }
        if i >= self.names_found as usize {
            *errorcodeptr = ERR53;
            self.erroroffset = name;
            return false;
        }
        *indexptr = i as i32;
        let mut count = 0;
        loop {
            count += 1;
            let groupnumber = get2(&self.code, slot);
            self.backref_map |= if groupnumber < 32 {
                1u32 << groupnumber
            } else {
                1
            };
            if groupnumber > self.top_backref {
                self.top_backref = groupnumber;
            }
            i += 1;
            if i >= self.names_found as usize {
                break;
            }
            slot += ese;
            if &self.code[slot + IMM2_SIZE..slot + IMM2_SIZE + length as usize] != nm
                || self.code[slot + IMM2_SIZE + length as usize] != 0
            {
                break;
            }
        }
        *countptr = count;
        true
    }

    /// compile_branch(): returns 0 on error, +1 if the branch matched at
    /// least one character, -1 otherwise.
    #[allow(clippy::too_many_arguments)]
    pub fn compile_branch(
        &mut self,
        optionsptr: &mut u32,
        xoptionsptr: &mut u32,
        codeptr: &mut usize,
        pptrptr: &mut usize,
        errorcodeptr: &mut i32,
        fr_out: &mut FirstReq,
        lp: bool,
        lengthptr: &mut usize,
    ) -> i32 {
        let mut bravalue: u32 = 0;
        let mut okreturn: i32 = -1;
        let mut group_return: i32 = 0;
        let mut repeat_min: u32 = 0;
        let mut repeat_max: u32 = 0;
        let mut options = *optionsptr;
        let mut xoptions = *xoptionsptr;
        let mut pptr = *pptrptr;
        let mut offset: usize = 0;
        let mut length_prevgroup: usize = 0;
        let mut code = *codeptr;
        let mut last_code = code;
        let orig_code = code;
        let mut previous: usize = usize::MAX;
        let mut groupsetfirstcu = false;
        let mut had_accept = false;
        let mut matched_char = false;
        let mut previous_matched_char;
        let mut reset_caseful = false;
        let mut classbits: [u8; 32];
        let utf = (options & PCRE2_UTF) != 0;
        let ucp = (options & PCRE2_UCP) != 0;

        let mut greedy_default = ((options & PCRE2_UNGREEDY) != 0) as u32;
        let mut greedy_non_default = greedy_default ^ 1;
        let mut firstcu: u32 = 0;
        let mut reqcu: u32 = 0;
        let mut zerofirstcu: u32 = 0;
        let mut zeroreqcu: u32 = 0;
        let mut firstcuflags = REQ_UNSET;
        let mut reqcuflags = REQ_UNSET;
        let mut zerofirstcuflags = REQ_UNSET;
        let mut zeroreqcuflags = REQ_UNSET;
        let mut req_caseopt = if (options & PCRE2_CASELESS) != 0 {
            REQ_CASELESS
        } else {
            0
        };

        loop {
            let mut meta = meta_code(self.pp(pptr));
            let mut meta_arg = meta_data(self.pp(pptr));
            let mut skipunits: usize = 0;
            let mut note_group_empty;
            let mut mcbuffer = [0u8; 8];
            let mut mclength: usize = 0;
            let mut subfr = FirstReq::default();

            if lp {
                if code > self.start_workspace + self.workspace_size - WORK_SIZE_SAFETY_MARGIN {
                    *errorcodeptr = if code >= self.start_workspace + self.workspace_size {
                        ERR52
                    } else {
                        ERR86
                    };
                    return 0;
                }
                if code < last_code {
                    code = last_code;
                }
                if !(META_ASTERISK..=META_MINMAX_QUERY).contains(&meta) {
                    if OFLOW_MAX - *lengthptr < code - orig_code {
                        *errorcodeptr = ERR20;
                        return 0;
                    }
                    *lengthptr += code - orig_code;
                    if *lengthptr > MAX_PATTERN_SIZE {
                        *errorcodeptr = ERR20;
                        return 0;
                    }
                    code = orig_code;
                }
                last_code = code;
            }

            if !(META_ASTERISK..=META_MINMAX_QUERY).contains(&meta) {
                previous = code;
                if matched_char && !had_accept {
                    okreturn = 1;
                }
            }
            previous_matched_char = matched_char;
            matched_char = false;
            note_group_empty = false;

            #[derive(Clone, Copy, PartialEq)]
            enum Act {
                Meta,
                NormalChar,
                NormalCharSet,
                ClassCaselessChar,
                GroupProcess,
                HandleSingleReference,
                HandleNumericalRecursion,
                Repeat,
                VerbArg,
                Done,
            }
            let mut act = Act::Meta;

            while act != Act::Done {
                match act {
                    Act::Done => {}
                    Act::Meta => {
                        act = Act::Done;
                        match meta {
                            META_END | META_ALT | META_KET => {
                                fr_out.firstcu = firstcu;
                                fr_out.firstcuflags = firstcuflags;
                                fr_out.reqcu = reqcu;
                                fr_out.reqcuflags = reqcuflags;
                                *codeptr = code;
                                *pptrptr = pptr;
                                return okreturn;
                            }
                            META_CIRCUMFLEX => {
                                if (options & PCRE2_MULTILINE) != 0 {
                                    if firstcuflags == REQ_UNSET {
                                        firstcuflags = REQ_NONE;
                                        zerofirstcuflags = REQ_NONE;
                                    }
                                    self.wc(code, OP_CIRCM);
                                } else {
                                    self.wc(code, OP_CIRC);
                                }
                                code += 1;
                            }
                            META_DOLLAR => {
                                self.wc(
                                    code,
                                    if (options & PCRE2_MULTILINE) != 0 {
                                        OP_DOLLM
                                    } else {
                                        OP_DOLL
                                    },
                                );
                                code += 1;
                            }
                            META_DOT => {
                                matched_char = true;
                                if firstcuflags == REQ_UNSET {
                                    firstcuflags = REQ_NONE;
                                }
                                zerofirstcu = firstcu;
                                zerofirstcuflags = firstcuflags;
                                zeroreqcu = reqcu;
                                zeroreqcuflags = reqcuflags;
                                self.wc(
                                    code,
                                    if (options & PCRE2_DOTALL) != 0 {
                                        OP_ALLANY
                                    } else {
                                        OP_ANY
                                    },
                                );
                                code += 1;
                            }
                            META_CLASS_EMPTY | META_CLASS_EMPTY_NOT => {
                                matched_char = true;
                                self.wc(
                                    code,
                                    if meta == META_CLASS_EMPTY_NOT {
                                        OP_ALLANY
                                    } else {
                                        OP_FAIL
                                    },
                                );
                                code += 1;
                                if firstcuflags == REQ_UNSET {
                                    firstcuflags = REQ_NONE;
                                }
                                zerofirstcu = firstcu;
                                zerofirstcuflags = firstcuflags;
                            }
                            META_CLASS_NOT | META_CLASS => {
                                matched_char = true;
                                let negate_class = meta == META_CLASS_NOT;

                                // A single-character class.
                                if self.pp(pptr + 1) < META_END
                                    && self.pp(pptr + 2) == META_CLASS_END
                                {
                                    let c = self.pp(pptr + 1);
                                    pptr += 2;
                                    if meta == META_CLASS {
                                        meta = c;
                                        act = Act::NormalCharSet;
                                        continue;
                                    }
                                    zeroreqcu = reqcu;
                                    zeroreqcuflags = reqcuflags;
                                    if firstcuflags == REQ_UNSET {
                                        firstcuflags = REQ_NONE;
                                    }
                                    zerofirstcu = firstcu;
                                    zerofirstcuflags = firstcuflags;
                                    if (utf || ucp) && (options & PCRE2_CASELESS) != 0 {
                                        let d = ucd_caseset(c);
                                        if d != 0
                                            && ((xoptions & PCRE2_EXTRA_CASELESS_RESTRICT) == 0
                                                || tables::UCD_CASELESS_SETS[d as usize] > 127)
                                        {
                                            self.wc(code, OP_NOTPROP);
                                            self.wc(code + 1, PT_CLIST);
                                            self.wc(code + 2, d);
                                            code += 3;
                                            continue;
                                        }
                                    }
                                    self.wc(
                                        code,
                                        if (options & PCRE2_CASELESS) != 0 {
                                            OP_NOTI
                                        } else {
                                            OP_NOT
                                        },
                                    );
                                    code += 1;
                                    code += self.putchar(c, code, utf);
                                    continue;
                                }

                                // A two-character class that is a case pair.
                                if meta == META_CLASS
                                    && self.pp(pptr + 1) < META_END
                                    && self.pp(pptr + 2) < META_END
                                    && self.pp(pptr + 3) == META_CLASS_END
                                {
                                    let c = self.pp(pptr + 1);
                                    if ucd_caseset(c) == 0
                                        || ((xoptions & PCRE2_EXTRA_CASELESS_RESTRICT) != 0
                                            && c < 128
                                            && self.pp(pptr + 2) < 128)
                                    {
                                        let d = if (utf || ucp) && c > 127 {
                                            ucd_othercase(c)
                                        } else if c > 255 {
                                            c
                                        } else {
                                            self.fcc(c)
                                        };
                                        if c != d && self.pp(pptr + 2) == d {
                                            pptr += 3;
                                            meta = c;
                                            if (options & PCRE2_CASELESS) == 0 {
                                                reset_caseful = true;
                                                options |= PCRE2_CASELESS;
                                                req_caseopt = REQ_CASELESS;
                                            }
                                            act = Act::ClassCaselessChar;
                                            continue;
                                        }
                                    }
                                }

                                let mut should_flip_negation = false;
                                let mut match_all_or_no_wide_chars = false;
                                let mut xclass = false;
                                let mut class_uchardata = code + LINK_SIZE + 2;
                                let class_uchardata_base = class_uchardata;
                                let mut class_has_8bitchar: i32 = 0;
                                let mut xclass_has_prop = false;
                                classbits = [0u8; 32];

                                loop {
                                    pptr += 1;
                                    let mut m = self.pp(pptr);
                                    if m == META_CLASS_END {
                                        break;
                                    }
                                    let goto_class_literal;
                                    'cont: {
                                        if m == META_POSIX || m == META_POSIX_NEG {
                                            let local_negate = m == META_POSIX_NEG;
                                            pptr += 1;
                                            let mut posix_class = self.pp(pptr) as i32;
                                            should_flip_negation = local_negate;
                                            if (options & PCRE2_CASELESS) != 0 && posix_class <= 2 {
                                                posix_class = 0;
                                            }
                                            if (options & PCRE2_UCP) != 0
                                                && (xoptions & PCRE2_EXTRA_ASCII_POSIX) == 0
                                            {
                                                match posix_class {
                                                    PC_GRAPH | PC_PRINT | PC_PUNCT => {
                                                        self.wc(
                                                            class_uchardata,
                                                            if local_negate {
                                                                XCL_NOTPROP
                                                            } else {
                                                                XCL_PROP
                                                            },
                                                        );
                                                        self.wc(
                                                            class_uchardata + 1,
                                                            if posix_class == PC_GRAPH {
                                                                PT_PXGRAPH
                                                            } else if posix_class == PC_PRINT {
                                                                PT_PXPRINT
                                                            } else {
                                                                PT_PXPUNCT
                                                            },
                                                        );
                                                        self.wc(class_uchardata + 2, 0);
                                                        class_uchardata += 3;
                                                        xclass_has_prop = true;
                                                        break 'cont;
                                                    }
                                                    _ => {
                                                        if utf {
                                                            match_all_or_no_wide_chars |=
                                                                local_negate;
                                                        }
                                                    }
                                                }
                                            }
                                            let pc = (posix_class * 3) as usize;
                                            let mut pbits = [0u8; 32];
                                            let base = POSIX_CLASS_MAPS[pc] as usize;
                                            for (i, b) in pbits.iter_mut().enumerate() {
                                                *b = self.cbits(base + i);
                                            }
                                            let taboffset = POSIX_CLASS_MAPS[pc + 1];
                                            let mut tabopt = POSIX_CLASS_MAPS[pc + 2];
                                            if taboffset >= 0 {
                                                if tabopt >= 0 {
                                                    for (i, b) in pbits.iter_mut().enumerate() {
                                                        *b |= self.cbits(i + taboffset as usize);
                                                    }
                                                } else {
                                                    for (i, b) in pbits.iter_mut().enumerate() {
                                                        *b &= !self.cbits(i + taboffset as usize);
                                                    }
                                                }
                                            }
                                            if tabopt < 0 {
                                                tabopt = -tabopt;
                                            }
                                            if tabopt == 1 {
                                                pbits[1] &= !0x3c;
                                            } else if tabopt == 2 {
                                                pbits[11] &= 0x7f;
                                            }
                                            if local_negate {
                                                for i in 0..32 {
                                                    classbits[i] |= !pbits[i];
                                                }
                                            } else {
                                                for i in 0..32 {
                                                    classbits[i] |= pbits[i];
                                                }
                                            }
                                            class_has_8bitchar = 1;
                                            break 'cont;
                                        }

                                        if m == META_BIGVALUE {
                                            pptr += 1;
                                            m = self.pp(pptr);
                                            goto_class_literal = true;
                                        } else if m >= META_END {
                                            if meta_code(m) != META_ESCAPE {
                                                *errorcodeptr = ERR89;
                                                return 0;
                                            }
                                            let escape = meta_data(m);
                                            class_has_8bitchar += 1;
                                            match escape {
                                                ESC_d => {
                                                    for i in 0..32 {
                                                        classbits[i] |=
                                                            self.cbits(i + CBIT_DIGIT as usize);
                                                    }
                                                }
                                                ESC_D => {
                                                    should_flip_negation = true;
                                                    for i in 0..32 {
                                                        classbits[i] |=
                                                            !self.cbits(i + CBIT_DIGIT as usize);
                                                    }
                                                }
                                                ESC_w => {
                                                    for i in 0..32 {
                                                        classbits[i] |=
                                                            self.cbits(i + CBIT_WORD as usize);
                                                    }
                                                }
                                                ESC_W => {
                                                    should_flip_negation = true;
                                                    for i in 0..32 {
                                                        classbits[i] |=
                                                            !self.cbits(i + CBIT_WORD as usize);
                                                    }
                                                }
                                                ESC_s => {
                                                    for i in 0..32 {
                                                        classbits[i] |=
                                                            self.cbits(i + CBIT_SPACE as usize);
                                                    }
                                                }
                                                ESC_S => {
                                                    should_flip_negation = true;
                                                    for i in 0..32 {
                                                        classbits[i] |=
                                                            !self.cbits(i + CBIT_SPACE as usize);
                                                    }
                                                }
                                                ESC_h => {
                                                    self.add_list_to_class(
                                                        &mut classbits,
                                                        &mut class_uchardata,
                                                        options & !PCRE2_CASELESS,
                                                        xoptions,
                                                        &tables::HSPACE_LIST,
                                                        NOTACHAR,
                                                    );
                                                }
                                                ESC_H => {
                                                    self.add_not_list_to_class(
                                                        &mut classbits,
                                                        &mut class_uchardata,
                                                        options & !PCRE2_CASELESS,
                                                        xoptions,
                                                        &tables::HSPACE_LIST,
                                                    );
                                                }
                                                ESC_v => {
                                                    self.add_list_to_class(
                                                        &mut classbits,
                                                        &mut class_uchardata,
                                                        options & !PCRE2_CASELESS,
                                                        xoptions,
                                                        &tables::VSPACE_LIST,
                                                        NOTACHAR,
                                                    );
                                                }
                                                ESC_V => {
                                                    self.add_not_list_to_class(
                                                        &mut classbits,
                                                        &mut class_uchardata,
                                                        options & !PCRE2_CASELESS,
                                                        xoptions,
                                                        &tables::VSPACE_LIST,
                                                    );
                                                }
                                                ESC_p | ESC_P => {
                                                    pptr += 1;
                                                    let ptype = self.pp(pptr) >> 16;
                                                    let pdata = self.pp(pptr) & 0xffff;
                                                    self.wc(
                                                        class_uchardata,
                                                        if escape == ESC_p {
                                                            XCL_PROP
                                                        } else {
                                                            XCL_NOTPROP
                                                        },
                                                    );
                                                    self.wc(class_uchardata + 1, ptype);
                                                    self.wc(class_uchardata + 2, pdata);
                                                    class_uchardata += 3;
                                                    xclass_has_prop = true;
                                                    class_has_8bitchar -= 1;
                                                }
                                                _ => {}
                                            }
                                            break 'cont;
                                        } else {
                                            goto_class_literal = true;
                                        }

                                        if goto_class_literal {
                                            // CLASS_LITERAL
                                            let c = m;
                                            let mut d;
                                            if c == CHAR_CR || c == CHAR_NL {
                                                self.external_flags |= PCRE2_HASCRORLF;
                                            }
                                            if self.pp(pptr + 1) == META_RANGE_LITERAL
                                                || self.pp(pptr + 1) == META_RANGE_ESCAPED
                                            {
                                                pptr += 2;
                                                d = self.pp(pptr);
                                                if d == META_BIGVALUE {
                                                    pptr += 1;
                                                    d = self.pp(pptr);
                                                }
                                                if d == CHAR_CR || d == CHAR_NL {
                                                    self.external_flags |= PCRE2_HASCRORLF;
                                                }
                                                class_has_8bitchar += self.add_to_class(
                                                    &mut classbits,
                                                    &mut class_uchardata,
                                                    options,
                                                    xoptions,
                                                    c,
                                                    d,
                                                )
                                                    as i32;
                                                break 'cont;
                                            }
                                            class_has_8bitchar += self.add_to_class(
                                                &mut classbits,
                                                &mut class_uchardata,
                                                options,
                                                xoptions,
                                                m,
                                                m,
                                            )
                                                as i32;
                                        }
                                    }
                                    // CONTINUE_CLASS
                                    if class_uchardata > class_uchardata_base {
                                        xclass = true;
                                        if lp {
                                            *lengthptr += class_uchardata - class_uchardata_base;
                                            class_uchardata = class_uchardata_base;
                                        }
                                    }
                                }

                                if firstcuflags == REQ_UNSET {
                                    firstcuflags = REQ_NONE;
                                }
                                zerofirstcu = firstcu;
                                zerofirstcuflags = firstcuflags;
                                zeroreqcu = reqcu;
                                zeroreqcuflags = reqcuflags;

                                if xclass
                                    && ((options & PCRE2_UCP) != 0
                                        || xclass_has_prop
                                        || !should_flip_negation)
                                {
                                    if match_all_or_no_wide_chars
                                        || (utf
                                            && should_flip_negation
                                            && !negate_class
                                            && (options & PCRE2_UCP) == 0)
                                    {
                                        self.wc(class_uchardata, XCL_RANGE);
                                        class_uchardata += 1;
                                        if utf {
                                            class_uchardata +=
                                                self.ord2utf_at(0x100, class_uchardata);
                                            class_uchardata += self
                                                .ord2utf_at(MAX_UTF_CODE_POINT, class_uchardata);
                                        }
                                    }
                                    self.wc(class_uchardata, XCL_END);
                                    class_uchardata += 1;
                                    self.wc(code, OP_XCLASS);
                                    code += 1;
                                    code += LINK_SIZE;
                                    let mut flags = if negate_class { XCL_NOT } else { 0 };
                                    if xclass_has_prop {
                                        flags |= XCL_HASPROP;
                                    }
                                    self.wc(code, flags);
                                    if class_has_8bitchar > 0 {
                                        self.code[code] |= XCL_MAP as u8;
                                        code += 1;
                                        let n = class_uchardata - code;
                                        self.code.copy_within(code..code + n, code + 32);
                                        if negate_class && !xclass_has_prop {
                                            for b in classbits.iter_mut() {
                                                *b ^= 255;
                                            }
                                        }
                                        self.code[code..code + 32].copy_from_slice(&classbits);
                                        code = class_uchardata + 32;
                                    } else {
                                        code = class_uchardata;
                                    }
                                    put(&mut self.code, previous + 1, code - previous);
                                    continue;
                                }

                                self.wc(
                                    code,
                                    if negate_class == should_flip_negation {
                                        OP_CLASS
                                    } else {
                                        OP_NCLASS
                                    },
                                );
                                code += 1;
                                if !lp {
                                    if negate_class {
                                        for b in classbits.iter_mut() {
                                            *b ^= 255;
                                        }
                                    }
                                    self.code[code..code + 32].copy_from_slice(&classbits);
                                }
                                code += 32;
                            }

                            META_ACCEPT => {
                                self.had_accept = true;
                                had_accept = true;
                                let ad = self.assert_depth;
                                let caps: Vec<u16> = self
                                    .open_caps
                                    .iter()
                                    .rev()
                                    .take_while(|(_, d)| *d >= ad)
                                    .map(|(n, _)| *n)
                                    .collect();
                                for n in caps {
                                    if lp {
                                        *lengthptr += 1 + IMM2_SIZE;
                                    } else {
                                        self.wc(code, OP_CLOSE);
                                        put2(&mut self.code, code + 1, n as u32);
                                        code += 1 + IMM2_SIZE;
                                    }
                                }
                                self.wc(
                                    code,
                                    if self.assert_depth > 0 {
                                        OP_ASSERT_ACCEPT
                                    } else {
                                        OP_ACCEPT
                                    },
                                );
                                code += 1;
                                if firstcuflags == REQ_UNSET {
                                    firstcuflags = REQ_NONE;
                                }
                            }

                            META_PRUNE | META_SKIP | META_COMMIT | META_FAIL => {
                                if meta == META_PRUNE || meta == META_SKIP {
                                    self.had_pruneorskip = true;
                                }
                                self.wc(code, VERBOPS[((meta - META_MARK) >> 16) as usize]);
                                code += 1;
                            }
                            META_THEN => {
                                self.external_flags |= PCRE2_HASTHEN;
                                self.wc(code, OP_THEN);
                                code += 1;
                            }
                            META_THEN_ARG => {
                                self.external_flags |= PCRE2_HASTHEN;
                                act = Act::VerbArg;
                            }
                            META_PRUNE_ARG | META_SKIP_ARG => {
                                self.had_pruneorskip = true;
                                act = Act::VerbArg;
                            }
                            META_MARK | META_COMMIT_ARG => {
                                act = Act::VerbArg;
                            }

                            META_OPTIONS => {
                                pptr += 1;
                                options = self.pp(pptr);
                                *optionsptr = options;
                                pptr += 1;
                                xoptions = self.pp(pptr);
                                *xoptionsptr = xoptions;
                                greedy_default = ((options & PCRE2_UNGREEDY) != 0) as u32;
                                greedy_non_default = greedy_default ^ 1;
                                req_caseopt = if (options & PCRE2_CASELESS) != 0 {
                                    REQ_CASELESS
                                } else {
                                    0
                                };
                            }

                            META_COND_RNUMBER | META_COND_NAME | META_COND_RNAME => {
                                bravalue = OP_COND;
                                pptr += 1;
                                let length = self.pp(pptr);
                                offset = self.getoffset_at(pptr + 1);
                                pptr += 2;
                                let name = offset;
                                let nm: Vec<u8> = self.pat[name..name + length as usize].to_vec();
                                let mut found: Option<usize> = None;
                                for (i, ng) in self
                                    .named_groups
                                    .iter()
                                    .enumerate()
                                    .take(self.names_found as usize)
                                {
                                    if length == ng.length as u32
                                        && self.pat[ng.name..ng.name + length as usize] == nm[..]
                                    {
                                        found = Some(i);
                                        break;
                                    }
                                }
                                if let Some(i) = found {
                                    let ng = self.named_groups[i];
                                    if !ng.isdup {
                                        self.wc(
                                            code + 1 + LINK_SIZE,
                                            if meta == META_COND_RNAME {
                                                OP_RREF
                                            } else {
                                                OP_CREF
                                            },
                                        );
                                        put2(&mut self.code, code + 2 + LINK_SIZE, ng.number);
                                        if ng.number > self.top_backref {
                                            self.top_backref = ng.number;
                                        }
                                        skipunits = 1 + IMM2_SIZE;
                                        note_group_empty = true;
                                        act = Act::GroupProcess;
                                        continue;
                                    }
                                    // Duplicate name: handled below.
                                    self.wc(
                                        code + 1 + LINK_SIZE,
                                        if meta == META_COND_RNAME {
                                            OP_RREF
                                        } else {
                                            OP_CREF
                                        },
                                    );
                                    let mut count = 0;
                                    let mut index = 0;
                                    if !lp
                                        && !self.find_dupname_details(
                                            name,
                                            length,
                                            &mut index,
                                            &mut count,
                                            errorcodeptr,
                                        )
                                    {
                                        return 0;
                                    }
                                    self.code[code + 1 + LINK_SIZE] += 1;
                                    skipunits = 1 + 2 * IMM2_SIZE;
                                    put2(&mut self.code, code + 2 + LINK_SIZE, index as u32);
                                    put2(
                                        &mut self.code,
                                        code + 2 + LINK_SIZE + IMM2_SIZE,
                                        count as u32,
                                    );
                                    note_group_empty = true;
                                    act = Act::GroupProcess;
                                    continue;
                                }
                                // Name not found: R<digits> or an error.
                                let mut groupnumber: u32 = 0;
                                if meta == META_COND_RNUMBER {
                                    for i in 1..length as usize {
                                        groupnumber = groupnumber * 10 + (nm[i] - b'0') as u32;
                                        if groupnumber > MAX_GROUP_NUMBER {
                                            *errorcodeptr = ERR61;
                                            self.erroroffset = offset + i;
                                            return 0;
                                        }
                                    }
                                }
                                if meta != META_COND_RNUMBER || groupnumber > self.bracount {
                                    *errorcodeptr = ERR15;
                                    self.erroroffset = offset;
                                    return 0;
                                }
                                if groupnumber == 0 {
                                    groupnumber = RREF_ANY;
                                }
                                self.wc(code + 1 + LINK_SIZE, OP_RREF);
                                put2(&mut self.code, code + 2 + LINK_SIZE, groupnumber);
                                skipunits = 1 + IMM2_SIZE;
                                note_group_empty = true;
                                act = Act::GroupProcess;
                            }

                            META_COND_DEFINE => {
                                bravalue = OP_COND;
                                offset = self.getoffset_at(pptr + 1);
                                pptr += 2;
                                self.wc(code + 1 + LINK_SIZE, OP_DEFINE);
                                skipunits = 1;
                                act = Act::GroupProcess;
                            }

                            META_COND_NUMBER => {
                                bravalue = OP_COND;
                                offset = self.getoffset_at(pptr + 1);
                                pptr += 2;
                                pptr += 1;
                                let groupnumber = self.pp(pptr);
                                if groupnumber > self.bracount {
                                    *errorcodeptr = ERR15;
                                    self.erroroffset = offset;
                                    return 0;
                                }
                                if groupnumber > self.top_backref {
                                    self.top_backref = groupnumber;
                                }
                                offset -= 2;
                                self.wc(code + 1 + LINK_SIZE, OP_CREF);
                                skipunits = 1 + IMM2_SIZE;
                                put2(&mut self.code, code + 2 + LINK_SIZE, groupnumber);
                                note_group_empty = true;
                                act = Act::GroupProcess;
                            }

                            META_COND_VERSION => {
                                bravalue = OP_COND;
                                let (p1, p2, p3) =
                                    (self.pp(pptr + 1), self.pp(pptr + 2), self.pp(pptr + 3));
                                let v = if p1 > 0 {
                                    PCRE2_MAJOR > p2 || (PCRE2_MAJOR == p2 && PCRE2_MINOR >= p3)
                                } else {
                                    PCRE2_MAJOR == p2 && PCRE2_MINOR == p3
                                };
                                self.wc(code + 1 + LINK_SIZE, if v { OP_TRUE } else { OP_FALSE });
                                skipunits = 1;
                                pptr += 3;
                                note_group_empty = true;
                                act = Act::GroupProcess;
                            }

                            META_COND_ASSERT => {
                                bravalue = OP_COND;
                                note_group_empty = true;
                                act = Act::GroupProcess;
                            }

                            META_LOOKAHEAD => {
                                bravalue = OP_ASSERT;
                                self.assert_depth += 1;
                                act = Act::GroupProcess;
                            }
                            META_LOOKAHEAD_NA => {
                                bravalue = OP_ASSERT_NA;
                                self.assert_depth += 1;
                                act = Act::GroupProcess;
                            }
                            META_LOOKAHEADNOT => {
                                let p1 = self.pp(pptr + 1);
                                let p2 = self.pp(pptr + 2);
                                if p1 == META_KET
                                    && !(META_ASTERISK..=META_MINMAX_QUERY).contains(&p2)
                                {
                                    self.wc(code, OP_FAIL);
                                    code += 1;
                                    pptr += 1;
                                } else {
                                    bravalue = OP_ASSERT_NOT;
                                    self.assert_depth += 1;
                                    act = Act::GroupProcess;
                                }
                            }
                            META_LOOKBEHIND => {
                                bravalue = OP_ASSERTBACK;
                                self.assert_depth += 1;
                                act = Act::GroupProcess;
                            }
                            META_LOOKBEHINDNOT => {
                                bravalue = OP_ASSERTBACK_NOT;
                                self.assert_depth += 1;
                                act = Act::GroupProcess;
                            }
                            META_LOOKBEHIND_NA => {
                                bravalue = OP_ASSERTBACK_NA;
                                self.assert_depth += 1;
                                act = Act::GroupProcess;
                            }
                            META_ATOMIC => {
                                bravalue = OP_ONCE;
                                note_group_empty = true;
                                act = Act::GroupProcess;
                            }
                            META_SCRIPT_RUN => {
                                bravalue = OP_SCRIPT_RUN;
                                note_group_empty = true;
                                act = Act::GroupProcess;
                            }
                            META_NOCAPTURE => {
                                bravalue = OP_BRA;
                                note_group_empty = true;
                                act = Act::GroupProcess;
                            }

                            META_BACKREF_BYNAME | META_RECURSE_BYNAME => {
                                pptr += 1;
                                let length = self.pp(pptr);
                                offset = self.getoffset_at(pptr + 1);
                                pptr += 2;
                                let name = offset;
                                let mut is_dupname = false;
                                let mut groupnumber: u32 = 0;
                                let mut goto_recursion = false;
                                for i in 0..self.names_found as usize {
                                    let ng = self.named_groups[i];
                                    if length == ng.length as u32
                                        && self.pat[name..name + length as usize]
                                            == self.pat[ng.name..ng.name + length as usize]
                                    {
                                        is_dupname = ng.isdup;
                                        groupnumber = ng.number;
                                        if meta == META_RECURSE_BYNAME {
                                            meta_arg = groupnumber;
                                            goto_recursion = true;
                                            break;
                                        }
                                        self.backref_map |= if groupnumber < 32 {
                                            1u32 << groupnumber
                                        } else {
                                            1
                                        };
                                        if groupnumber > self.top_backref {
                                            self.top_backref = groupnumber;
                                        }
                                    }
                                }
                                if goto_recursion {
                                    act = Act::HandleNumericalRecursion;
                                    continue;
                                }
                                if groupnumber == 0 {
                                    *errorcodeptr = ERR15;
                                    self.erroroffset = offset;
                                    return 0;
                                }
                                if !is_dupname {
                                    meta_arg = groupnumber;
                                    act = Act::HandleSingleReference;
                                    continue;
                                }
                                let mut count = 0;
                                let mut index = 0;
                                if !lp
                                    && !self.find_dupname_details(
                                        name,
                                        length,
                                        &mut index,
                                        &mut count,
                                        errorcodeptr,
                                    )
                                {
                                    return 0;
                                }
                                if firstcuflags == REQ_UNSET {
                                    firstcuflags = REQ_NONE;
                                }
                                self.wc(
                                    code,
                                    if (options & PCRE2_CASELESS) != 0 {
                                        OP_DNREFI
                                    } else {
                                        OP_DNREF
                                    },
                                );
                                put2(&mut self.code, code + 1, index as u32);
                                put2(&mut self.code, code + 3, count as u32);
                                code += 1 + 2 * IMM2_SIZE;
                            }

                            META_CALLOUT_NUMBER => {
                                self.wc(code, OP_CALLOUT);
                                let (a, b, n) =
                                    (self.pp(pptr + 1), self.pp(pptr + 2), self.pp(pptr + 3));
                                put(&mut self.code, code + 1, a as usize);
                                put(&mut self.code, code + 1 + LINK_SIZE, b as usize);
                                self.wc(code + 1 + 2 * LINK_SIZE, n);
                                pptr += 3;
                                code += op_len(OP_CALLOUT as u8);
                            }

                            META_CALLOUT_STRING => {
                                if lp {
                                    *lengthptr += self.pp(pptr + 3) as usize + (1 + 4 * LINK_SIZE);
                                    pptr += 3;
                                    pptr += SIZEOFFSET;
                                } else {
                                    let mut length = self.pp(pptr + 3);
                                    let mut cs = code + 1 + 4 * LINK_SIZE;
                                    self.wc(code, OP_CALLOUT_STR);
                                    let (a, b) = (self.pp(pptr + 1), self.pp(pptr + 2));
                                    put(&mut self.code, code + 1, a as usize);
                                    put(&mut self.code, code + 1 + LINK_SIZE, b as usize);
                                    pptr += 3;
                                    offset = self.getoffset_at(pptr + 1);
                                    pptr += 2;
                                    let mut pp = offset;
                                    let mut delimiter = self.pat[pp] as u32;
                                    self.code[cs] = self.pat[pp];
                                    cs += 1;
                                    pp += 1;
                                    if delimiter == b'{' as u32 {
                                        delimiter = b'}' as u32;
                                    }
                                    put(&mut self.code, code + 1 + 3 * LINK_SIZE, offset + 1);
                                    loop {
                                        length -= 1;
                                        if length <= 1 {
                                            break;
                                        }
                                        if self.pat[pp] as u32 == delimiter
                                            && self.pat[pp + 1] as u32 == delimiter
                                        {
                                            self.code[cs] = delimiter as u8;
                                            cs += 1;
                                            pp += 2;
                                            length -= 1;
                                        } else {
                                            self.code[cs] = self.pat[pp];
                                            cs += 1;
                                            pp += 1;
                                        }
                                    }
                                    self.code[cs] = 0;
                                    cs += 1;
                                    put(&mut self.code, code + 1 + 2 * LINK_SIZE, cs - code);
                                    code = cs;
                                }
                            }

                            META_MINMAX_PLUS | META_MINMAX_QUERY | META_MINMAX => {
                                pptr += 1;
                                repeat_min = self.pp(pptr);
                                pptr += 1;
                                repeat_max = self.pp(pptr);
                                act = Act::Repeat;
                            }
                            META_ASTERISK | META_ASTERISK_PLUS | META_ASTERISK_QUERY => {
                                repeat_min = 0;
                                repeat_max = REPEAT_UNLIMITED;
                                act = Act::Repeat;
                            }
                            META_PLUS | META_PLUS_PLUS | META_PLUS_QUERY => {
                                repeat_min = 1;
                                repeat_max = REPEAT_UNLIMITED;
                                act = Act::Repeat;
                            }
                            META_QUERY | META_QUERY_PLUS | META_QUERY_QUERY => {
                                repeat_min = 0;
                                repeat_max = 1;
                                act = Act::Repeat;
                            }

                            META_BIGVALUE => {
                                pptr += 1;
                                act = Act::NormalChar;
                            }

                            META_BACKREF => {
                                if meta_arg < 10 {
                                    offset = self.small_ref_offset[meta_arg as usize];
                                } else {
                                    offset = self.getoffset_at(pptr + 1);
                                    pptr += 2;
                                }
                                if meta_arg > self.bracount {
                                    self.erroroffset = offset;
                                    *errorcodeptr = ERR15;
                                    return 0;
                                }
                                act = Act::HandleSingleReference;
                            }

                            META_RECURSE => {
                                offset = self.getoffset_at(pptr + 1);
                                pptr += 2;
                                if meta_arg > self.bracount {
                                    self.erroroffset = offset;
                                    *errorcodeptr = ERR15;
                                    return 0;
                                }
                                act = Act::HandleNumericalRecursion;
                            }

                            META_CAPTURE => {
                                bravalue = OP_CBRA;
                                skipunits = IMM2_SIZE;
                                put2(&mut self.code, code + 1 + LINK_SIZE, meta_arg);
                                self.lastcapture = meta_arg;
                                note_group_empty = true;
                                act = Act::GroupProcess;
                            }

                            META_ESCAPE => {
                                if meta_arg > ESC_b && meta_arg < ESC_Z {
                                    matched_char = true;
                                    if firstcuflags == REQ_UNSET {
                                        firstcuflags = REQ_NONE;
                                    }
                                }
                                zerofirstcu = firstcu;
                                zerofirstcuflags = firstcuflags;
                                zeroreqcu = reqcu;
                                zeroreqcuflags = reqcuflags;
                                if meta_arg == ESC_P || meta_arg == ESC_p {
                                    pptr += 1;
                                    let ptype = self.pp(pptr) >> 16;
                                    let pdata = self.pp(pptr) & 0xffff;
                                    if meta_arg == ESC_p && ptype == PT_ANY {
                                        self.wc(code, OP_ALLANY);
                                        code += 1;
                                    } else {
                                        self.wc(
                                            code,
                                            if meta_arg == ESC_p {
                                                OP_PROP
                                            } else {
                                                OP_NOTPROP
                                            },
                                        );
                                        self.wc(code + 1, ptype);
                                        self.wc(code + 2, pdata);
                                        code += 3;
                                    }
                                    continue;
                                }
                                if self.assert_depth > 0
                                    && meta_arg == ESC_K
                                    && (xoptions & PCRE2_EXTRA_ALLOW_LOOKAROUND_BSK) == 0
                                {
                                    *errorcodeptr = ERR99;
                                    return 0;
                                }
                                match meta_arg {
                                    ESC_C => {
                                        self.external_flags |= PCRE2_HASBKC;
                                        if !utf {
                                            meta_arg = OP_ALLANY;
                                        }
                                    }
                                    ESC_B | ESC_b => {
                                        if (options & PCRE2_UCP) != 0
                                            && (xoptions & PCRE2_EXTRA_ASCII_BSW) == 0
                                        {
                                            meta_arg = if meta_arg == ESC_B {
                                                OP_NOT_UCP_WORD_BOUNDARY
                                            } else {
                                                OP_UCP_WORD_BOUNDARY
                                            };
                                        }
                                        if self.max_lookbehind == 0 {
                                            self.max_lookbehind = 1;
                                        }
                                    }
                                    ESC_A => {
                                        if self.max_lookbehind == 0 {
                                            self.max_lookbehind = 1;
                                        }
                                    }
                                    _ => {}
                                }
                                self.wc(code, meta_arg);
                                code += 1;
                            }

                            _ => {
                                if meta >= META_END {
                                    *errorcodeptr = ERR89;
                                    return 0;
                                }
                                act = Act::NormalChar;
                            }
                        }
                    }

                    Act::VerbArg => {
                        act = Act::Done;
                        self.wc(code, VERBOPS[((meta - META_MARK) >> 16) as usize]);
                        code += 1;
                        pptr += 1;
                        let verbarglen = self.pp(pptr);
                        let mut verbculen = 0usize;
                        let tempcode = code;
                        code += 1;
                        for _ in 0..verbarglen {
                            pptr += 1;
                            let m = self.pp(pptr);
                            let n = if utf {
                                ord2utf(m, &mut mcbuffer)
                            } else {
                                mcbuffer[0] = m as u8;
                                1
                            };
                            if lp {
                                *lengthptr += n;
                            } else {
                                self.code[code..code + n].copy_from_slice(&mcbuffer[..n]);
                                code += n;
                                verbculen += n;
                            }
                        }
                        self.code[tempcode] = verbculen as u8;
                        self.code[code] = 0;
                        code += 1;
                    }

                    Act::GroupProcess => {
                        act = Act::Done;
                        self.parens_depth += 1;
                        self.wc(code, bravalue);
                        pptr += 1;
                        let mut tempcode = code;
                        let tempreqvary = self.req_varyopt;
                        length_prevgroup = 0;
                        group_return = self.compile_regex(
                            options,
                            xoptions,
                            &mut tempcode,
                            &mut pptr,
                            errorcodeptr,
                            skipunits,
                            &mut subfr,
                            lp,
                            &mut length_prevgroup,
                        );
                        if group_return == 0 {
                            return 0;
                        }
                        self.parens_depth -= 1;
                        if note_group_empty && bravalue != OP_COND && group_return > 0 {
                            matched_char = true;
                        }
                        if (OP_ASSERT..=OP_ASSERTBACK_NA).contains(&bravalue) {
                            self.assert_depth -= 1;
                        }

                        if bravalue == OP_COND && !lp {
                            let mut tc = code;
                            let mut condcount = 0;
                            loop {
                                condcount += 1;
                                tc += get(&self.code, tc + 1);
                                if self.cc(tc) == OP_KET {
                                    break;
                                }
                            }
                            if self.cc(code + LINK_SIZE + 1) == OP_DEFINE {
                                if condcount > 1 {
                                    self.erroroffset = offset;
                                    *errorcodeptr = ERR54;
                                    return 0;
                                }
                                self.wc(code + LINK_SIZE + 1, OP_FALSE);
                                bravalue = OP_DEFINE;
                            } else {
                                if condcount > 2 {
                                    self.erroroffset = offset;
                                    *errorcodeptr = ERR27;
                                    return 0;
                                }
                                if condcount == 1 {
                                    subfr.firstcuflags = REQ_NONE;
                                    subfr.reqcuflags = REQ_NONE;
                                } else if group_return > 0 {
                                    matched_char = true;
                                }
                            }
                        }

                        if lp {
                            if OFLOW_MAX - *lengthptr
                                < length_prevgroup.wrapping_sub(2 + 2 * LINK_SIZE)
                            {
                                *errorcodeptr = ERR20;
                                return 0;
                            }
                            *lengthptr += length_prevgroup - 2 - 2 * LINK_SIZE;
                            code += 1;
                            put(&mut self.code, code, 1 + LINK_SIZE);
                            code += LINK_SIZE;
                            self.wc(code, OP_KET);
                            code += 1;
                            put(&mut self.code, code, 1 + LINK_SIZE);
                            code += LINK_SIZE;
                            continue;
                        }

                        code = tempcode;
                        if bravalue == OP_DEFINE {
                            continue;
                        }

                        zeroreqcu = reqcu;
                        zeroreqcuflags = reqcuflags;
                        zerofirstcu = firstcu;
                        zerofirstcuflags = firstcuflags;
                        groupsetfirstcu = false;

                        if bravalue >= OP_ONCE {
                            if firstcuflags == REQ_UNSET && subfr.firstcuflags != REQ_UNSET {
                                if subfr.firstcuflags < REQ_NONE {
                                    firstcu = subfr.firstcu;
                                    firstcuflags = subfr.firstcuflags;
                                    groupsetfirstcu = true;
                                } else {
                                    firstcuflags = REQ_NONE;
                                }
                                zerofirstcuflags = REQ_NONE;
                            } else if subfr.firstcuflags < REQ_NONE && subfr.reqcuflags >= REQ_NONE
                            {
                                subfr.reqcu = subfr.firstcu;
                                subfr.reqcuflags = subfr.firstcuflags | tempreqvary;
                            }
                            if subfr.reqcuflags < REQ_NONE {
                                reqcu = subfr.reqcu;
                                reqcuflags = subfr.reqcuflags;
                            }
                        } else if (bravalue == OP_ASSERT || bravalue == OP_ASSERT_NA)
                            && subfr.reqcuflags < REQ_NONE
                            && subfr.firstcuflags < REQ_NONE
                        {
                            reqcu = subfr.reqcu;
                            reqcuflags = subfr.reqcuflags;
                        }
                    }

                    Act::HandleSingleReference => {
                        act = Act::Done;
                        if firstcuflags == REQ_UNSET {
                            firstcuflags = REQ_NONE;
                            zerofirstcuflags = REQ_NONE;
                        }
                        self.wc(
                            code,
                            if (options & PCRE2_CASELESS) != 0 {
                                OP_REFI
                            } else {
                                OP_REF
                            },
                        );
                        put2(&mut self.code, code + 1, meta_arg);
                        code += 1 + IMM2_SIZE;
                        self.backref_map |= if meta_arg < 32 { 1u32 << meta_arg } else { 1 };
                        if meta_arg > self.top_backref {
                            self.top_backref = meta_arg;
                        }
                    }

                    Act::HandleNumericalRecursion => {
                        act = Act::Done;
                        self.wc(code, OP_RECURSE);
                        put(&mut self.code, code + 1, meta_arg as usize);
                        code += 1 + LINK_SIZE;
                        groupsetfirstcu = false;
                        self.had_recurse = true;
                        if firstcuflags == REQ_UNSET {
                            firstcuflags = REQ_NONE;
                        }
                        zerofirstcu = firstcu;
                        zerofirstcuflags = firstcuflags;
                    }

                    Act::Repeat => {
                        act = Act::Done;
                        if previous_matched_char && repeat_min > 0 {
                            matched_char = true;
                        }
                        let reqvary = if repeat_min == repeat_max {
                            0
                        } else {
                            REQ_VARY
                        };
                        let mut op_type: u32 = 0;
                        if repeat_min == 0 {
                            firstcu = zerofirstcu;
                            firstcuflags = zerofirstcuflags;
                            reqcu = zeroreqcu;
                            reqcuflags = zeroreqcuflags;
                        }
                        let mut repeat_type: u32;
                        let mut possessive_quantifier: bool;
                        match meta {
                            META_MINMAX_PLUS | META_ASTERISK_PLUS | META_PLUS_PLUS
                            | META_QUERY_PLUS => {
                                repeat_type = 0;
                                possessive_quantifier = true;
                            }
                            META_MINMAX_QUERY | META_ASTERISK_QUERY | META_PLUS_QUERY
                            | META_QUERY_QUERY => {
                                repeat_type = greedy_non_default;
                                possessive_quantifier = false;
                            }
                            _ => {
                                repeat_type = greedy_default;
                                possessive_quantifier = false;
                            }
                        }

                        let mut tempcode = previous;
                        let mut op_previous = self.cc(previous);

                        'end_repeat: {
                            // (prop_type, prop_value) for single-item repeats.
                            let mut single: Option<(i32, i32)> = None;
                            let mut do_bracket = false;
                            match op_previous {
                                OP_CHAR | OP_CHARI | OP_NOT | OP_NOTI => {
                                    if repeat_max == 1 && repeat_min == 1 {
                                        break 'end_repeat;
                                    }
                                    op_type = CHARTYPEOFFSET[(op_previous - OP_CHAR) as usize];
                                    if utf && not_firstcu(self.code[code - 1]) {
                                        let lastchar = backchar(&self.code, code - 1);
                                        mclength = code - lastchar;
                                        mcbuffer[..mclength]
                                            .copy_from_slice(&self.code[lastchar..code]);
                                    } else {
                                        mcbuffer[0] = self.code[code - 1];
                                        mclength = 1;
                                        if op_previous <= OP_CHARI && repeat_min > 1 {
                                            reqcu = mcbuffer[0] as u32;
                                            reqcuflags = self.req_varyopt;
                                            if op_previous == OP_CHARI {
                                                reqcuflags |= REQ_CASELESS;
                                            }
                                        }
                                    }
                                    single = Some((-1, -1));
                                }
                                OP_XCLASS | OP_CLASS | OP_NCLASS | OP_REF | OP_REFI | OP_DNREF
                                | OP_DNREFI => {
                                    if repeat_max == 0 {
                                        code = previous;
                                        break 'end_repeat;
                                    }
                                    if repeat_max == 1 && repeat_min == 1 {
                                        break 'end_repeat;
                                    }
                                    if repeat_min == 0 && repeat_max == REPEAT_UNLIMITED {
                                        self.wc(code, OP_CRSTAR + repeat_type);
                                        code += 1;
                                    } else if repeat_min == 1 && repeat_max == REPEAT_UNLIMITED {
                                        self.wc(code, OP_CRPLUS + repeat_type);
                                        code += 1;
                                    } else if repeat_min == 0 && repeat_max == 1 {
                                        self.wc(code, OP_CRQUERY + repeat_type);
                                        code += 1;
                                    } else {
                                        self.wc(code, OP_CRRANGE + repeat_type);
                                        put2(&mut self.code, code + 1, repeat_min);
                                        if repeat_max == REPEAT_UNLIMITED {
                                            repeat_max = 0;
                                        }
                                        put2(&mut self.code, code + 3, repeat_max);
                                        code += 1 + 2 * IMM2_SIZE;
                                    }
                                }
                                OP_FAIL => break 'end_repeat,
                                OP_RECURSE => {
                                    if repeat_max == 1 && repeat_min == 1 && !possessive_quantifier
                                    {
                                        break 'end_repeat;
                                    }
                                    let mut skip_rest = false;
                                    if repeat_min > 0
                                        && (repeat_min != 1 || repeat_max != REPEAT_UNLIMITED)
                                    {
                                        let mut replicate = repeat_min as usize;
                                        if repeat_min == repeat_max {
                                            replicate -= 1;
                                        }
                                        if lp {
                                            let delta = replicate.checked_mul(1 + LINK_SIZE);
                                            match delta {
                                                Some(d) if OFLOW_MAX - *lengthptr >= d => {
                                                    *lengthptr += d
                                                }
                                                _ => {
                                                    *errorcodeptr = ERR20;
                                                    return 0;
                                                }
                                            }
                                        } else {
                                            for _ in 0..replicate {
                                                self.code.copy_within(
                                                    previous..previous + 1 + LINK_SIZE,
                                                    code,
                                                );
                                                previous = code;
                                                code += 1 + LINK_SIZE;
                                            }
                                        }
                                        if repeat_min == repeat_max {
                                            skip_rest = true;
                                        } else {
                                            if repeat_max != REPEAT_UNLIMITED {
                                                repeat_max -= repeat_min;
                                            }
                                            repeat_min = 0;
                                        }
                                    }
                                    if !skip_rest {
                                        self.code.copy_within(
                                            previous..previous + 1 + LINK_SIZE,
                                            previous + 1 + LINK_SIZE,
                                        );
                                        self.wc(previous, OP_BRA);
                                        op_previous = OP_BRA;
                                        put(&mut self.code, previous + 1, 2 + 2 * LINK_SIZE);
                                        self.wc(previous + 2 + 2 * LINK_SIZE, OP_KET);
                                        put(
                                            &mut self.code,
                                            previous + 3 + 2 * LINK_SIZE,
                                            2 + 2 * LINK_SIZE,
                                        );
                                        code += 2 + 2 * LINK_SIZE;
                                        length_prevgroup = 3 + 3 * LINK_SIZE;
                                        group_return = -1;
                                        do_bracket = true;
                                    }
                                }
                                OP_ASSERT | OP_ASSERT_NOT | OP_ASSERT_NA | OP_ASSERTBACK
                                | OP_ASSERTBACK_NOT | OP_ASSERTBACK_NA | OP_ONCE
                                | OP_SCRIPT_RUN | OP_BRA | OP_CBRA | OP_COND => {
                                    do_bracket = true;
                                }
                                _ => {
                                    if op_previous >= OP_EODN {
                                        *errorcodeptr = ERR10;
                                        return 0;
                                    }
                                    if repeat_max == 1 && repeat_min == 1 {
                                        break 'end_repeat;
                                    }
                                    op_type = OP_TYPESTAR - OP_STAR;
                                    mclength = 0;
                                    if op_previous == OP_PROP || op_previous == OP_NOTPROP {
                                        single = Some((
                                            self.cc(previous + 1) as i32,
                                            self.cc(previous + 2) as i32,
                                        ));
                                    } else {
                                        single = Some((-1, -1));
                                    }
                                }
                            }

                            if do_bracket {
                                let len = code - previous;
                                let mut bralink: Option<usize> = None;
                                let mut brazeroptr: Option<usize> = None;
                                if repeat_max == 1 && repeat_min == 1 && !possessive_quantifier {
                                    break 'end_repeat;
                                }
                                if op_previous == OP_COND
                                    && self.cc(previous + LINK_SIZE + 1) == OP_FALSE
                                    && self.cc(previous + get(&self.code, previous + 1)) != OP_ALT
                                {
                                    break 'end_repeat;
                                }
                                if op_previous < OP_ONCE && repeat_max == REPEAT_UNLIMITED {
                                    repeat_max = repeat_min + 1;
                                }
                                if repeat_min == 0 {
                                    if repeat_max <= 1 || repeat_max == REPEAT_UNLIMITED {
                                        self.code
                                            .copy_within(previous..previous + len, previous + 1);
                                        code += 1;
                                        if repeat_max == 0 {
                                            self.wc(previous, OP_SKIPZERO);
                                            break 'end_repeat;
                                        }
                                        brazeroptr = Some(previous);
                                        self.wc(previous, OP_BRAZERO + repeat_type);
                                        previous += 1;
                                    } else {
                                        self.code.copy_within(
                                            previous..previous + len,
                                            previous + 2 + LINK_SIZE,
                                        );
                                        code += 2 + LINK_SIZE;
                                        self.wc(previous, OP_BRAZERO + repeat_type);
                                        previous += 1;
                                        self.wc(previous, OP_BRA);
                                        previous += 1;
                                        let linkoffset = match bralink {
                                            None => 0,
                                            Some(b) => previous - b,
                                        };
                                        bralink = Some(previous);
                                        put(&mut self.code, previous, linkoffset);
                                        previous += LINK_SIZE;
                                    }
                                    if repeat_max != REPEAT_UNLIMITED {
                                        repeat_max -= 1;
                                    }
                                } else {
                                    if repeat_min > 1 {
                                        if lp {
                                            let delta = ((repeat_min - 1) as usize)
                                                .checked_mul(length_prevgroup);
                                            match delta {
                                                Some(d) if OFLOW_MAX - *lengthptr >= d => {
                                                    *lengthptr += d
                                                }
                                                _ => {
                                                    *errorcodeptr = ERR20;
                                                    return 0;
                                                }
                                            }
                                        } else {
                                            if groupsetfirstcu && reqcuflags >= REQ_NONE {
                                                reqcu = firstcu;
                                                reqcuflags = firstcuflags;
                                            }
                                            for _ in 1..repeat_min {
                                                self.code
                                                    .copy_within(previous..previous + len, code);
                                                code += len;
                                            }
                                        }
                                    }
                                    if repeat_max != REPEAT_UNLIMITED {
                                        repeat_max -= repeat_min;
                                    }
                                }

                                if repeat_max != REPEAT_UNLIMITED {
                                    if lp && repeat_max > 0 {
                                        let delta = (repeat_max as usize)
                                            .checked_mul(length_prevgroup + 1 + 2 + 2 * LINK_SIZE);
                                        match delta {
                                            Some(d)
                                                if OFLOW_MAX + (2 + 2 * LINK_SIZE) - *lengthptr
                                                    >= d =>
                                            {
                                                *lengthptr += d - (2 + 2 * LINK_SIZE);
                                            }
                                            _ => {
                                                *errorcodeptr = ERR20;
                                                return 0;
                                            }
                                        }
                                    } else {
                                        let mut i = repeat_max;
                                        while i >= 1 {
                                            self.wc(code, OP_BRAZERO + repeat_type);
                                            code += 1;
                                            if i != 1 {
                                                self.wc(code, OP_BRA);
                                                code += 1;
                                                let linkoffset = match bralink {
                                                    None => 0,
                                                    Some(b) => code - b,
                                                };
                                                bralink = Some(code);
                                                put(&mut self.code, code, linkoffset);
                                                code += LINK_SIZE;
                                            }
                                            self.code.copy_within(previous..previous + len, code);
                                            code += len;
                                            i -= 1;
                                        }
                                    }
                                    while let Some(bl) = bralink {
                                        let linkoffset = code - bl + 1;
                                        let bra = code - linkoffset;
                                        let oldlinkoffset = get(&self.code, bra + 1);
                                        bralink = if oldlinkoffset == 0 {
                                            None
                                        } else {
                                            Some(bl - oldlinkoffset)
                                        };
                                        self.wc(code, OP_KET);
                                        code += 1;
                                        put(&mut self.code, code, linkoffset);
                                        code += LINK_SIZE;
                                        put(&mut self.code, bra + 1, linkoffset);
                                    }
                                } else {
                                    let ketcode = code - 1 - LINK_SIZE;
                                    let bracode = ketcode - get(&self.code, ketcode + 1);
                                    if self.cc(bracode) == OP_ONCE && possessive_quantifier {
                                        self.wc(bracode, OP_BRA);
                                    }
                                    if self.cc(bracode) == OP_ONCE
                                        || self.cc(bracode) == OP_SCRIPT_RUN
                                    {
                                        self.wc(ketcode, OP_KETRMAX + repeat_type);
                                    } else {
                                        if !lp {
                                            if group_return < 0 {
                                                self.code[bracode] += (OP_SBRA - OP_BRA) as u8;
                                            }
                                            if self.cc(bracode) == OP_COND
                                                && self.cc(bracode + get(&self.code, bracode + 1))
                                                    != OP_ALT
                                            {
                                                self.wc(bracode, OP_SCOND);
                                            }
                                        }
                                        if possessive_quantifier {
                                            if self.cc(bracode) == OP_COND
                                                || self.cc(bracode) == OP_SCOND
                                            {
                                                let mut nlen = code - bracode;
                                                self.code.copy_within(
                                                    bracode..bracode + nlen,
                                                    bracode + 1 + LINK_SIZE,
                                                );
                                                code += 1 + LINK_SIZE;
                                                nlen += 1 + LINK_SIZE;
                                                let nb = if self.cc(bracode) == OP_COND {
                                                    OP_BRAPOS
                                                } else {
                                                    OP_SBRAPOS
                                                };
                                                self.wc(bracode, nb);
                                                self.wc(code, OP_KETRPOS);
                                                code += 1;
                                                put(&mut self.code, code, nlen);
                                                code += LINK_SIZE;
                                                put(&mut self.code, bracode + 1, nlen);
                                            } else {
                                                self.code[bracode] += 1;
                                                self.wc(ketcode, OP_KETRPOS);
                                            }
                                            if let Some(bz) = brazeroptr {
                                                self.wc(bz, OP_BRAPOSZERO);
                                            }
                                            if repeat_min < 2 {
                                                possessive_quantifier = false;
                                            }
                                        } else {
                                            self.wc(ketcode, OP_KETRMAX + repeat_type);
                                        }
                                    }
                                }
                            }

                            if let Some((prop_type, prop_value)) = single {
                                // OUTPUT_SINGLE_REPEAT
                                let oldcode = code;
                                code = previous;
                                if repeat_max == 0 {
                                    break 'end_repeat;
                                }
                                repeat_type += op_type;
                                if repeat_min == 0 {
                                    if repeat_max == REPEAT_UNLIMITED {
                                        self.wc(code, OP_STAR + repeat_type);
                                        code += 1;
                                    } else if repeat_max == 1 {
                                        self.wc(code, OP_QUERY + repeat_type);
                                        code += 1;
                                    } else {
                                        self.wc(code, OP_UPTO + repeat_type);
                                        put2(&mut self.code, code + 1, repeat_max);
                                        code += 1 + IMM2_SIZE;
                                    }
                                } else if repeat_min == 1 {
                                    if repeat_max == REPEAT_UNLIMITED {
                                        self.wc(code, OP_PLUS + repeat_type);
                                        code += 1;
                                    } else {
                                        code = oldcode;
                                        if repeat_max == 1 {
                                            break 'end_repeat;
                                        }
                                        self.wc(code, OP_UPTO + repeat_type);
                                        put2(&mut self.code, code + 1, repeat_max - 1);
                                        code += 1 + IMM2_SIZE;
                                    }
                                } else {
                                    self.wc(code, OP_EXACT + op_type);
                                    put2(&mut self.code, code + 1, repeat_min);
                                    code += 1 + IMM2_SIZE;
                                    if repeat_max != repeat_min {
                                        if mclength > 0 {
                                            self.code[code..code + mclength]
                                                .copy_from_slice(&mcbuffer[..mclength]);
                                            code += mclength;
                                        } else {
                                            self.wc(code, op_previous);
                                            code += 1;
                                            if prop_type >= 0 {
                                                self.wc(code, prop_type as u32);
                                                self.wc(code + 1, prop_value as u32);
                                                code += 2;
                                            }
                                        }
                                        if repeat_max == REPEAT_UNLIMITED {
                                            self.wc(code, OP_STAR + repeat_type);
                                            code += 1;
                                        } else {
                                            repeat_max -= repeat_min;
                                            if repeat_max == 1 {
                                                self.wc(code, OP_QUERY + repeat_type);
                                                code += 1;
                                            } else {
                                                self.wc(code, OP_UPTO + repeat_type);
                                                put2(&mut self.code, code + 1, repeat_max);
                                                code += 1 + IMM2_SIZE;
                                            }
                                        }
                                    }
                                }
                                if mclength > 0 {
                                    self.code[code..code + mclength]
                                        .copy_from_slice(&mcbuffer[..mclength]);
                                    code += mclength;
                                } else {
                                    self.wc(code, op_previous);
                                    code += 1;
                                    if prop_type >= 0 {
                                        self.wc(code, prop_type as u32);
                                        self.wc(code + 1, prop_value as u32);
                                        code += 2;
                                    }
                                }
                            }

                            if possessive_quantifier {
                                match self.cc(tempcode) {
                                    OP_TYPEEXACT => {
                                        let o = self.cc(tempcode + 1 + IMM2_SIZE);
                                        tempcode += op_len(self.code[tempcode])
                                            + if o == OP_PROP || o == OP_NOTPROP {
                                                2
                                            } else {
                                                0
                                            };
                                    }
                                    OP_CHAR | OP_CHARI | OP_NOT | OP_NOTI | OP_EXACT
                                    | OP_EXACTI | OP_NOTEXACT | OP_NOTEXACTI => {
                                        tempcode += op_len(self.code[tempcode]);
                                        if utf && has_extralen(self.code[tempcode - 1]) {
                                            tempcode += get_extralen(self.code[tempcode - 1]);
                                        }
                                    }
                                    OP_CLASS | OP_NCLASS => tempcode += 1 + 32,
                                    OP_XCLASS => tempcode += get(&self.code, tempcode + 1),
                                    _ => {}
                                }
                                let mut len = code as isize - tempcode as isize;
                                if len > 0 {
                                    let repcode = self.cc(tempcode);
                                    if repcode < OP_CALLOUT && opcode_possessify(repcode) > 0 {
                                        self.wc(tempcode, opcode_possessify(repcode));
                                    } else {
                                        let l = len as usize;
                                        self.code.copy_within(
                                            tempcode..tempcode + l,
                                            tempcode + 1 + LINK_SIZE,
                                        );
                                        code += 1 + LINK_SIZE;
                                        len += (1 + LINK_SIZE) as isize;
                                        self.wc(tempcode, OP_ONCE);
                                        self.wc(code, OP_KET);
                                        code += 1;
                                        put(&mut self.code, code, len as usize);
                                        code += LINK_SIZE;
                                        put(&mut self.code, tempcode + 1, len as usize);
                                    }
                                }
                            }
                        }
                        // END_REPEAT
                        self.req_varyopt |= reqvary;
                    }

                    Act::NormalChar | Act::NormalCharSet | Act::ClassCaselessChar => {
                        if act == Act::NormalChar {
                            meta = self.pp(pptr);
                        }
                        if act != Act::ClassCaselessChar {
                            // NORMAL_CHAR_SET
                            matched_char = true;
                            if (utf || ucp) && (options & PCRE2_CASELESS) != 0 {
                                let caseset = ucd_caseset(meta);
                                if caseset != 0
                                    && ((xoptions & PCRE2_EXTRA_CASELESS_RESTRICT) == 0
                                        || tables::UCD_CASELESS_SETS[caseset as usize] > 127)
                                {
                                    self.wc(code, OP_PROP);
                                    self.wc(code + 1, PT_CLIST);
                                    self.wc(code + 2, caseset);
                                    code += 3;
                                    if firstcuflags == REQ_UNSET {
                                        firstcuflags = REQ_NONE;
                                        zerofirstcuflags = REQ_NONE;
                                    }
                                    act = Act::Done;
                                    continue;
                                }
                            }
                        }
                        act = Act::Done;
                        // CLASS_CASELESS_CHAR
                        if utf {
                            mclength = ord2utf(meta, &mut mcbuffer);
                        } else {
                            mclength = 1;
                            mcbuffer[0] = meta as u8;
                        }
                        self.wc(
                            code,
                            if (options & PCRE2_CASELESS) != 0 {
                                OP_CHARI
                            } else {
                                OP_CHAR
                            },
                        );
                        code += 1;
                        self.code[code..code + mclength].copy_from_slice(&mcbuffer[..mclength]);
                        code += mclength;
                        if mcbuffer[0] as u32 == CHAR_CR || mcbuffer[0] as u32 == CHAR_NL {
                            self.external_flags |= PCRE2_HASCRORLF;
                        }
                        if firstcuflags == REQ_UNSET {
                            zerofirstcuflags = REQ_NONE;
                            zeroreqcu = reqcu;
                            zeroreqcuflags = reqcuflags;
                            if mclength == 1 || req_caseopt == 0 {
                                firstcu = mcbuffer[0] as u32;
                                firstcuflags = req_caseopt;
                                if mclength != 1 {
                                    reqcu = self.cc(code - 1);
                                    reqcuflags = self.req_varyopt;
                                }
                            } else {
                                firstcuflags = REQ_NONE;
                                reqcuflags = REQ_NONE;
                            }
                        } else {
                            zerofirstcu = firstcu;
                            zerofirstcuflags = firstcuflags;
                            zeroreqcu = reqcu;
                            zeroreqcuflags = reqcuflags;
                            if mclength == 1 || req_caseopt == 0 {
                                reqcu = self.cc(code - 1);
                                reqcuflags = req_caseopt | self.req_varyopt;
                            }
                        }
                        if reset_caseful {
                            options &= !PCRE2_CASELESS;
                            req_caseopt = 0;
                            reset_caseful = false;
                        }
                    }
                }
            }
            pptr += 1;
        }
    }

    /// compile_regex(): compile one group's alternatives.
    #[allow(clippy::too_many_arguments)]
    pub fn compile_regex(
        &mut self,
        mut options: u32,
        mut xoptions: u32,
        codeptr: &mut usize,
        pptrptr: &mut usize,
        errorcodeptr: &mut i32,
        skipunits: usize,
        fr_out: &mut FirstReq,
        lp: bool,
        lengthptr: &mut usize,
    ) -> i32 {
        let mut code = *codeptr;
        let mut last_branch = code;
        let start_bracket = code;
        let mut okreturn = 1;
        let mut pptr = *pptrptr;
        let mut firstcu: u32 = 0;
        let mut reqcu: u32 = 0;
        let mut firstcuflags = REQ_UNSET;
        let mut reqcuflags = REQ_UNSET;
        let mut length: usize = 2 + 2 * LINK_SIZE + skipunits;
        let mut lookbehindlength: u32;
        let lookbehindminlength: u32;

        let op = self.cc(code);
        let lookbehind = op == OP_ASSERTBACK || op == OP_ASSERTBACK_NOT || op == OP_ASSERTBACK_NA;
        if lookbehind {
            lookbehindlength = meta_data(self.pp(pptr - 1));
            lookbehindminlength = self.pp(pptr);
            pptr += SIZEOFFSET;
        } else {
            lookbehindlength = 0;
            lookbehindminlength = 0;
        }

        let pushed_cap = op == OP_CBRA;
        if pushed_cap {
            let capnumber = get2(&self.code, code + 1 + LINK_SIZE);
            self.open_caps.push((capnumber as u16, self.assert_depth));
        }

        put(&mut self.code, code + 1, 0);
        code += 1 + LINK_SIZE + skipunits;

        loop {
            if lookbehind && lookbehindlength > 0 {
                if lookbehindminlength == LOOKBEHIND_MAX || lookbehindminlength == lookbehindlength
                {
                    self.wc(code, OP_REVERSE);
                    put2(&mut self.code, code + 1, lookbehindlength);
                    code += 1 + IMM2_SIZE;
                    length += 1 + IMM2_SIZE;
                } else {
                    self.wc(code, OP_VREVERSE);
                    put2(&mut self.code, code + 1, lookbehindminlength);
                    put2(&mut self.code, code + 3, lookbehindlength);
                    code += 1 + 2 * IMM2_SIZE;
                    length += 1 + 2 * IMM2_SIZE;
                }
            }

            let mut bfr = FirstReq::default();
            let branch_return = self.compile_branch(
                &mut options,
                &mut xoptions,
                &mut code,
                &mut pptr,
                errorcodeptr,
                &mut bfr,
                lp,
                &mut length,
            );
            if branch_return == 0 {
                return 0;
            }
            if branch_return < 0 {
                okreturn = -1;
            }

            if !lp {
                if self.cc(last_branch) != OP_ALT {
                    firstcu = bfr.firstcu;
                    firstcuflags = bfr.firstcuflags;
                    reqcu = bfr.reqcu;
                    reqcuflags = bfr.reqcuflags;
                } else {
                    if firstcuflags != bfr.firstcuflags || firstcu != bfr.firstcu {
                        if firstcuflags < REQ_NONE && reqcuflags >= REQ_NONE {
                            reqcu = firstcu;
                            reqcuflags = firstcuflags;
                        }
                        firstcuflags = REQ_NONE;
                    }
                    if firstcuflags >= REQ_NONE
                        && bfr.firstcuflags < REQ_NONE
                        && bfr.reqcuflags >= REQ_NONE
                    {
                        bfr.reqcu = bfr.firstcu;
                        bfr.reqcuflags = bfr.firstcuflags;
                    }
                    if ((reqcuflags & !REQ_VARY) != (bfr.reqcuflags & !REQ_VARY))
                        || reqcu != bfr.reqcu
                    {
                        reqcuflags = REQ_NONE;
                    } else {
                        reqcu = bfr.reqcu;
                        reqcuflags |= bfr.reqcuflags;
                    }
                }
            }

            if meta_code(self.pp(pptr)) != META_ALT {
                if !lp {
                    let mut branch_length = code - last_branch;
                    loop {
                        let prev_length = get(&self.code, last_branch + 1);
                        put(&mut self.code, last_branch + 1, branch_length);
                        branch_length = prev_length;
                        last_branch -= branch_length;
                        if branch_length == 0 {
                            break;
                        }
                    }
                }
                self.wc(code, OP_KET);
                put(&mut self.code, code + 1, code - start_bracket);
                code += 1 + LINK_SIZE;
                *codeptr = code;
                *pptrptr = pptr;
                fr_out.firstcu = firstcu;
                fr_out.firstcuflags = firstcuflags;
                fr_out.reqcu = reqcu;
                fr_out.reqcuflags = reqcuflags;
                if lp {
                    if OFLOW_MAX - *lengthptr < length {
                        *errorcodeptr = ERR20;
                        return 0;
                    }
                    *lengthptr += length;
                }
                if pushed_cap {
                    self.open_caps.pop();
                }
                return okreturn;
            }

            if lp {
                code = *codeptr + 1 + LINK_SIZE + skipunits;
                length += 1 + LINK_SIZE;
            } else {
                self.wc(code, OP_ALT);
                put(&mut self.code, code + 1, code - last_branch);
                last_branch = code;
                code += 1 + LINK_SIZE;
            }
            lookbehindlength = meta_data(self.pp(pptr));
            pptr += 1;
        }
    }

    /// is_anchored()
    pub fn is_anchored(
        &self,
        mut code: usize,
        bracket_map: u32,
        atomcount: i32,
        inassert: bool,
    ) -> bool {
        loop {
            let scode = first_significant_code(&self.code, code + op_len(self.code[code]), false);
            let op = self.cc(scode);
            if op == OP_BRA || op == OP_BRAPOS || op == OP_SBRA || op == OP_SBRAPOS {
                if !self.is_anchored(scode, bracket_map, atomcount, inassert) {
                    return false;
                }
            } else if op == OP_CBRA || op == OP_CBRAPOS || op == OP_SCBRA || op == OP_SCBRAPOS {
                let n = get2(&self.code, scode + 1 + LINK_SIZE);
                let new_map = bracket_map | if n < 32 { 1u32 << n } else { 1 };
                if !self.is_anchored(scode, new_map, atomcount, inassert) {
                    return false;
                }
            } else if op == OP_ASSERT || op == OP_ASSERT_NA {
                if !self.is_anchored(scode, bracket_map, atomcount, true) {
                    return false;
                }
            } else if op == OP_COND || op == OP_SCOND {
                if self.cc(scode + get(&self.code, scode + 1)) != OP_ALT {
                    return false;
                }
                if !self.is_anchored(scode, bracket_map, atomcount, inassert) {
                    return false;
                }
            } else if op == OP_ONCE {
                if !self.is_anchored(scode, bracket_map, atomcount + 1, inassert) {
                    return false;
                }
            } else if op == OP_TYPESTAR || op == OP_TYPEMINSTAR || op == OP_TYPEPOSSTAR {
                if self.cc(scode + 1) != OP_ALLANY
                    || (bracket_map & self.backref_map) != 0
                    || atomcount > 0
                    || self.had_pruneorskip
                    || inassert
                    || (self.external_options & PCRE2_NO_DOTSTAR_ANCHOR) != 0
                {
                    return false;
                }
            } else if op != OP_SOD && op != OP_SOM && op != OP_CIRC {
                return false;
            }
            code += get(&self.code, code + 1);
            if self.cc(code) != OP_ALT {
                break;
            }
        }
        true
    }

    /// is_startline()
    pub fn is_startline(
        &self,
        mut code: usize,
        bracket_map: u32,
        atomcount: i32,
        inassert: bool,
    ) -> bool {
        loop {
            let mut scode =
                first_significant_code(&self.code, code + op_len(self.code[code]), false);
            let mut op = self.cc(scode);
            if op == OP_COND {
                scode += 1 + LINK_SIZE;
                if self.cc(scode) == OP_CALLOUT {
                    scode += op_len(OP_CALLOUT as u8);
                } else if self.cc(scode) == OP_CALLOUT_STR {
                    scode += get(&self.code, scode + 1 + 2 * LINK_SIZE);
                }
                match self.cc(scode) {
                    OP_CREF | OP_DNCREF | OP_RREF | OP_DNRREF | OP_FAIL | OP_FALSE | OP_TRUE => {
                        return false;
                    }
                    _ => {
                        if !self.is_startline(scode, bracket_map, atomcount, true) {
                            return false;
                        }
                        loop {
                            scode += get(&self.code, scode + 1);
                            if self.cc(scode) != OP_ALT {
                                break;
                            }
                        }
                        scode += 1 + LINK_SIZE;
                    }
                }
                scode = first_significant_code(&self.code, scode, false);
                op = self.cc(scode);
            }
            if op == OP_BRA || op == OP_BRAPOS || op == OP_SBRA || op == OP_SBRAPOS {
                if !self.is_startline(scode, bracket_map, atomcount, inassert) {
                    return false;
                }
            } else if op == OP_CBRA || op == OP_CBRAPOS || op == OP_SCBRA || op == OP_SCBRAPOS {
                let n = get2(&self.code, scode + 1 + LINK_SIZE);
                let new_map = bracket_map | if n < 32 { 1u32 << n } else { 1 };
                if !self.is_startline(scode, new_map, atomcount, inassert) {
                    return false;
                }
            } else if op == OP_ASSERT || op == OP_ASSERT_NA {
                if !self.is_startline(scode, bracket_map, atomcount, true) {
                    return false;
                }
            } else if op == OP_ONCE {
                if !self.is_startline(scode, bracket_map, atomcount + 1, inassert) {
                    return false;
                }
            } else if op == OP_TYPESTAR || op == OP_TYPEMINSTAR || op == OP_TYPEPOSSTAR {
                if self.cc(scode + 1) != OP_ANY
                    || (bracket_map & self.backref_map) != 0
                    || atomcount > 0
                    || self.had_pruneorskip
                    || inassert
                    || (self.external_options & PCRE2_NO_DOTSTAR_ANCHOR) != 0
                {
                    return false;
                }
            } else if op != OP_CIRC && op != OP_CIRCM {
                return false;
            }
            code += get(&self.code, code + 1);
            if self.cc(code) != OP_ALT {
                break;
            }
        }
        true
    }

    /// add_name_to_table()
    pub fn add_name_to_table(&mut self, name: usize, length: usize, groupno: u32, tablecount: u32) {
        let ese = self.name_entry_size as usize;
        let mut slot = self.name_table;
        let nm: Vec<u8> = self.pat[name..name + length].to_vec();
        for i in 0..tablecount as usize {
            let existing = &self.code[slot + IMM2_SIZE..slot + IMM2_SIZE + length];
            let mut crc = nm.as_slice().cmp(existing) as i32;
            if crc == 0 && self.code[slot + IMM2_SIZE + length] != 0 {
                crc = -1;
            }
            if crc < 0 {
                let n = (tablecount as usize - i) * ese;
                self.code.copy_within(slot..slot + n, slot + ese);
                break;
            }
            slot += ese;
        }
        put2(&mut self.code, slot, groupno);
        self.code[slot + IMM2_SIZE..slot + IMM2_SIZE + length].copy_from_slice(&nm);
        for b in &mut self.code[slot + IMM2_SIZE + length..slot + ese] {
            *b = 0;
        }
    }
}

/// find_recurse(): find the next OP_RECURSE from `p`.
pub fn find_recurse(code: &[u8], mut p: usize, utf: bool) -> Option<usize> {
    loop {
        let c = code[p] as u32;
        if c == OP_END {
            return None;
        }
        if c == OP_RECURSE {
            return Some(p);
        }
        if c == OP_XCLASS {
            p += get(code, p + 1);
        } else if c == OP_CALLOUT_STR {
            p += get(code, p + 1 + 2 * LINK_SIZE);
        } else {
            p = skip_simple_item(code, p, utf);
        }
    }
}

/// find_firstassertedcu()
pub fn find_firstassertedcu(code: &[u8], mut p: usize, flags: &mut u32, inassert: u32) -> u32 {
    let mut c: u32 = 0;
    let mut cflags = REQ_NONE;
    *flags = REQ_NONE;
    loop {
        let op0 = code[p] as u32;
        let xl = if op0 == OP_CBRA || op0 == OP_SCBRA || op0 == OP_CBRAPOS || op0 == OP_SCBRAPOS {
            IMM2_SIZE
        } else {
            0
        };
        let mut scode = first_significant_code(code, p + 1 + LINK_SIZE + xl, true);
        let op = code[scode] as u32;
        match op {
            OP_BRA | OP_BRAPOS | OP_CBRA | OP_SCBRA | OP_CBRAPOS | OP_SCBRAPOS | OP_ASSERT
            | OP_ASSERT_NA | OP_ONCE | OP_SCRIPT_RUN => {
                let mut dflags = 0;
                let d = find_firstassertedcu(
                    code,
                    scode,
                    &mut dflags,
                    inassert
                        + if op == OP_ASSERT || op == OP_ASSERT_NA {
                            1
                        } else {
                            0
                        },
                );
                if dflags >= REQ_NONE {
                    return 0;
                }
                if cflags >= REQ_NONE {
                    c = d;
                    cflags = dflags;
                } else if c != d || cflags != dflags {
                    return 0;
                }
            }
            OP_EXACT | OP_CHAR | OP_PLUS | OP_MINPLUS | OP_POSPLUS => {
                if op == OP_EXACT {
                    scode += IMM2_SIZE;
                }
                if inassert == 0 {
                    return 0;
                }
                if cflags >= REQ_NONE {
                    c = code[scode + 1] as u32;
                    cflags = 0;
                } else if c != code[scode + 1] as u32 {
                    return 0;
                }
            }
            OP_EXACTI | OP_CHARI | OP_PLUSI | OP_MINPLUSI | OP_POSPLUSI => {
                if op == OP_EXACTI {
                    scode += IMM2_SIZE;
                }
                if inassert == 0 {
                    return 0;
                }
                if code[scode + 1] >= 0x80 {
                    return 0;
                }
                if cflags >= REQ_NONE {
                    c = code[scode + 1] as u32;
                    cflags = REQ_CASELESS;
                } else if c != code[scode + 1] as u32 {
                    return 0;
                }
            }
            _ => return 0,
        }
        p += get(code, p + 1);
        if code[p] as u32 != OP_ALT {
            break;
        }
    }
    *flags = cflags;
    c
}
