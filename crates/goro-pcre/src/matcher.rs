//! Port of pcre2_match.c (the backtracking interpreter, non-JIT).
//!
//! The C code uses a vector of heap frames and implements RMATCH/RRETURN
//! with gotos; here each opcode handler returns an `Act` telling the driver
//! loop to recurse (push a frame and start a new match), return (pop a
//! frame and resume the caller at its return id) or continue.

use crate::code::Code;
use crate::consts::*;
use crate::tables;
use crate::util::*;

const MATCH_MATCH: i32 = 1;
const MATCH_NOMATCH: i32 = 0;
const MATCH_ACCEPT: i32 = -999;
const MATCH_KETRPOS: i32 = -998;
const MATCH_COMMIT: i32 = -997;
const MATCH_PRUNE: i32 = -996;
const MATCH_SKIP: i32 = -995;
const MATCH_SKIP_ARG: i32 = -994;
const MATCH_THEN: i32 = -993;
const MATCH_BACKTRACK_MAX: i32 = MATCH_THEN;
const MATCH_BACKTRACK_MIN: i32 = MATCH_COMMIT;

const GF_CAPTURE: u32 = 0x00010000;
const GF_NOCAPTURE: u32 = 0x00020000;
const GF_CONDASSERT: u32 = 0x00030000;
const GF_RECURSE: u32 = 0x00040000;
#[inline]
fn gf_idmask(a: u32) -> u32 {
    a & 0xffff0000
}
#[inline]
fn gf_datamask(a: u32) -> u32 {
    a & 0x0000ffff
}

const RECURSE_UNSET: u32 = 0xffffffff;
const UNSET: usize = usize::MAX;

const REPTYPE_MIN: u32 = 0;
const REPTYPE_MAX: u32 = 1;
const REPTYPE_POS: u32 = 2;

static REP_MIN: [u32; 11] = [0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0];
static REP_MAX: [u32; 11] = [
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    1,
    1,
    0,
    0,
    u32::MAX,
    u32::MAX,
    1,
];
static REP_TYP: [u32; 12] = [
    REPTYPE_MAX,
    REPTYPE_MIN,
    REPTYPE_MAX,
    REPTYPE_MIN,
    REPTYPE_MAX,
    REPTYPE_MIN,
    REPTYPE_MAX,
    REPTYPE_MIN,
    REPTYPE_POS,
    REPTYPE_POS,
    REPTYPE_POS,
    REPTYPE_POS,
];

// Resume ids (the RMxx labels of the C code).
const RM_BRA_ALT: u8 = 1;
const RM_GROUPLOOP: u8 = 2;
const RM_ASSERT: u8 = 3;
const RM_ASSERT_NOT: u8 = 4;
const RM_COND_ASSERT: u8 = 5;
const RM_KETRMIN: u8 = 6;
const RM_KETRMAX: u8 = 7;
const RM_POS_GROUP: u8 = 8;
const RM_BRAZERO: u8 = 9;
const RM_BRAMINZERO: u8 = 10;
const RM_RECURSE: u8 = 11;
const RM_MARK: u8 = 12;
const RM_COMMIT: u8 = 13;
const RM_PRUNE: u8 = 14;
const RM_PRUNE_ARG: u8 = 15;
const RM_SKIP: u8 = 16;
const RM_SKIP_ARG: u8 = 17;
const RM_THEN: u8 = 18;
const RM_THEN_ARG: u8 = 19;
const RM_REF_MIN: u8 = 20;
const RM_REF_MAX_SAME: u8 = 21;
const RM_REF_MAX_DIFF: u8 = 22;
const RM_CLASS_MIN: u8 = 23;
const RM_CLASS_MAX: u8 = 24;
const RM_CHARI_MIN: u8 = 25;
const RM_CHARI_MAX: u8 = 26;
const RM_CHAR_MIN: u8 = 27;
const RM_CHAR_MAX: u8 = 28;
const RM_NOTI_MIN: u8 = 29;
const RM_NOTI_MAX: u8 = 30;
const RM_NOT_MIN: u8 = 31;
const RM_NOT_MAX: u8 = 32;
const RM_TYPE_MIN: u8 = 33;
const RM_TYPE_MAX: u8 = 34;
const RM_SCOND: u8 = 35;
const RM_COMMIT_ARG: u8 = 36;
const RM_VREVERSE: u8 = 37;
const RM_XCLASS_MIN: u8 = 100;
const RM_XCLASS_MAX: u8 = 101;
const RM_CLASS_MAX_UTF: u8 = 201;
const RM_CHAR_UTF_MIN: u8 = 202;
const RM_CHAR_UTF_MAX: u8 = 203;
const RM_NOTI_MAX_UTF: u8 = 205;
const RM_NOT_MAX_UTF: u8 = 207;
const RM_TYPE_MAX_EXTUNI: u8 = 220;
const RM_TYPE_MAX_UTF: u8 = 221;
const RM_TYPE_MAX_PROP: u8 = 222;

#[derive(Clone, Copy)]
struct Frame {
    ecode: usize,
    temp_sptr: [usize; 2],
    length: usize,
    back_frame: usize,
    temp_size: usize,
    rdepth: u32,
    group_frame_type: u32,
    temp_32: [u32; 4],
    return_id: u8,
    op: u8,
    occu: [u8; 6],
    eptr: usize,
    start_match: usize,
    mark: usize,
    recurse_last_used: usize,
    current_recurse: u32,
    capture_last: u32,
    last_group_offset: usize,
    offset_top: usize,
    // Locals of the C match() that we keep per frame where the C code
    // relies on them not being changed by nested calls.
    proptype: i32,
}

impl Default for Frame {
    fn default() -> Self {
        Frame {
            ecode: 0,
            temp_sptr: [0; 2],
            length: 0,
            back_frame: 1,
            temp_size: 0,
            rdepth: 0,
            group_frame_type: 0,
            temp_32: [0; 4],
            return_id: 0,
            op: 0,
            occu: [0; 6],
            eptr: 0,
            start_match: 0,
            mark: UNSET,
            recurse_last_used: 0,
            current_recurse: RECURSE_UNSET,
            capture_last: 0,
            last_group_offset: UNSET,
            offset_top: 0,
            proptype: -1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
enum Phase {
    Single,
    MinPhase,
    MinLoop,
    MaxScan,
}

enum Act {
    Next,
    Rmatch(usize, u8, u32),
    Rreturn(i32),
    Ret(i32),
}

/// Match-time limits (pcre2_match_context).
#[derive(Clone, Copy, Debug)]
pub struct MatchContext {
    pub match_limit: u32,
    pub depth_limit: u32,
    pub heap_limit: u32,
    pub offset_limit: usize,
    /// Emulated JIT matching (pcre2_jit_compile() done): a match that JIT
    /// could run ignores the depth limits and fails with
    /// PCRE2_ERROR_JIT_STACKLIMIT past this nesting depth instead.
    pub jit_depth_limit: Option<u32>,
}

impl Default for MatchContext {
    fn default() -> Self {
        MatchContext {
            match_limit: MATCH_LIMIT,
            depth_limit: MATCH_LIMIT_DEPTH,
            heap_limit: HEAP_LIMIT,
            offset_limit: PCRE2_UNSET,
            jit_depth_limit: None,
        }
    }
}

/// Results of a match (pcre2_match_data).
#[derive(Clone, Debug, Default)]
pub struct MatchData {
    pub ovector: Vec<usize>,
    pub oveccount: usize,
    pub rc: i32,
    /// The (*MARK) name, if any.
    pub mark: Option<Vec<u8>>,
    pub startchar: usize,
    pub leftchar: usize,
    pub rightchar: usize,
}

impl MatchData {
    pub fn new(oveccount: usize) -> Self {
        let oveccount = oveccount.max(1);
        MatchData {
            ovector: vec![PCRE2_UNSET; oveccount * 2],
            oveccount,
            ..Default::default()
        }
    }
}

thread_local! {
    /// The frame and ovector vectors of the last finished match, reused
    /// (with their capacity) by the next one.
    static BUFFERS: std::cell::Cell<(Vec<Frame>, Vec<usize>)> =
        const { std::cell::Cell::new((Vec::new(), Vec::new())) };
}

impl Drop for Matcher<'_> {
    fn drop(&mut self) {
        let mut frames = std::mem::take(&mut self.frames);
        let mut ovec = std::mem::take(&mut self.ovec);
        // Keep the pooled buffers small: a pathological match can grow them
        // to megabytes.
        if frames.capacity() <= 1024 && ovec.capacity() <= 16384 {
            frames.clear();
            ovec.clear();
            BUFFERS.with(|b| b.set((frames, ovec)));
        }
    }
}

struct Matcher<'a> {
    code: &'a [u8],
    subj: &'a [u8],
    tables: &'static [u8],
    frames: Vec<Frame>,
    ovec: Vec<usize>,
    ovs: usize,
    f: usize,
    top_bracket: u16,

    match_limit: u32,
    match_limit_depth: u32,
    /// The depth limit is the emulated JIT stack's.
    jit: bool,
    match_call_count: u32,
    hasthen: bool,
    start_offset: usize,
    end_offset_top: usize,
    bsr_convention: u16,
    name_entry_size: usize,
    name_table: usize,
    start_code: usize,
    check_subject: usize,
    end_subject: usize,
    true_end_subject: usize,
    end_match_ptr: usize,
    start_used_ptr: usize,
    last_used_ptr: usize,
    mark: usize,
    nomatch_mark: usize,
    verb_ecode_ptr: usize,
    verb_skip_ptr: usize,
    verb_skip_name: usize,
    verb_current_recurse: u32,
    moptions: u32,
    poptions: u32,
    skip_arg_count: u32,
    ignore_skip_arg: u32,
    nltype: u32,
    nllen: u32,
    nl: [u8; 4],
    utf: bool,
    ucp: bool,
    assert_accept_frame: usize,
    branch_end: usize,
    out_ovector: Vec<usize>,
    oveccount: usize,
}

impl<'a> Matcher<'a> {
    #[inline]
    fn lcc(&self, c: u32) -> u32 {
        self.tables[c as usize] as u32
    }
    #[inline]
    fn fcc(&self, c: u32) -> u32 {
        self.tables[256 + c as usize] as u32
    }
    #[inline]
    fn ctypes(&self, c: u32) -> u32 {
        self.tables[CTYPES_OFFSET + c as usize] as u32
    }
    #[inline]
    fn cb(&self, i: usize) -> u32 {
        self.code[i] as u32
    }
    #[inline]
    fn sb(&self, i: usize) -> u32 {
        self.subj[i] as u32
    }

    /// IS_NEWLINE(p) (NLBLOCK = mb, PSEND = end_subject).
    fn is_newline(&mut self, p: usize) -> bool {
        if self.nltype != NLTYPE_FIXED {
            if p < self.end_subject {
                if let Some(l) = is_newline(self.subj, p, self.nltype, self.end_subject, self.utf) {
                    self.nllen = l as u32;
                    return true;
                }
            }
            false
        } else {
            let nllen = self.nllen as usize;
            p + nllen <= self.end_subject
                && self.subj[p] == self.nl[0]
                && (nllen == 1 || self.subj[p + 1] == self.nl[1])
        }
    }

    /// WAS_NEWLINE(p) (PSSTART = start_subject).
    fn was_newline(&mut self, p: usize) -> bool {
        if self.nltype != NLTYPE_FIXED {
            if p > 0 {
                if let Some(l) = was_newline(self.subj, p, self.nltype, 0, self.utf) {
                    self.nllen = l as u32;
                    return true;
                }
            }
            false
        } else {
            let nllen = self.nllen as usize;
            p >= nllen
                && self.subj[p - nllen] == self.nl[0]
                && (nllen == 1 || self.subj[p - nllen + 1] == self.nl[1])
        }
    }

    /// GETCHARINCTEST
    #[inline]
    fn getc_inc(&self, eptr: &mut usize) -> u32 {
        let (c, l) = getchar_test(self.subj, *eptr, self.utf);
        *eptr += l;
        c
    }

    #[inline]
    fn fwd_char(&self, mut p: usize) -> usize {
        p += 1;
        if self.utf {
            while p < self.end_subject && (self.subj[p] & 0xc0) == 0x80 {
                p += 1;
            }
        }
        p
    }

    /// Does character `c` have the Unicode property (ptype, pvalue)?
    /// None for an unknown property type.
    fn has_prop(&self, c: u32, ptype: u32, pvalue: u32) -> Option<bool> {
        let prop = get_ucd(c);
        let ct = prop.chartype as u32;
        let g = tables::UCP_GENTYPE[prop.chartype as usize];
        Some(match ptype {
            PT_ANY => true,
            PT_LAMP => ct == ucp_Lu || ct == ucp_Ll || ct == ucp_Lt,
            PT_GC => pvalue == g,
            PT_PC => pvalue == ct,
            PT_SC => pvalue == prop.script as u32,
            PT_SCX => {
                pvalue == prop.script as u32
                    || mapbit(
                        &tables::UCD_SCRIPT_SETS[ucd_scriptx_prop(prop) as usize..],
                        pvalue,
                    )
            }
            PT_ALNUM => g == ucp_L || g == ucp_N,
            PT_SPACE | PT_PXSPACE => {
                if is_hspace(c) || is_vspace(c) {
                    true
                } else {
                    g == ucp_Z
                }
            }
            PT_WORD => g == ucp_L || g == ucp_N || ct == ucp_Mn || ct == ucp_Pc,
            PT_CLIST => {
                let mut p = pvalue as usize;
                loop {
                    let v = tables::UCD_CASELESS_SETS[p];
                    if c < v {
                        break false;
                    }
                    p += 1;
                    if c == v {
                        break true;
                    }
                }
            }
            PT_UCNC => {
                c == b'$' as u32
                    || c == b'@' as u32
                    || c == b'`' as u32
                    || (0xa0..=0xd7ff).contains(&c)
                    || c >= 0xe000
            }
            PT_BIDICL => ucd_bidiclass_prop(prop) == pvalue,
            PT_BOOL => mapbit(
                &tables::UCD_BOOLPROP_SETS[ucd_bprops_prop(prop) as usize..],
                pvalue,
            ),
            _ => return None,
        })
    }

    /// Match one item of an OP_TYPE* repeat (or a single type opcode) at
    /// `eptr` (< end_subject). Returns Ok((matched, new_eptr)); on failure
    /// new_eptr is where the C code leaves Feptr, which matters because it
    /// feeds last_used_ptr. Err(code) for an internal error.
    fn type_one(
        &mut self,
        ctype: u32,
        proptype: i32,
        propvalue: u32,
        eptr: usize,
        phase: Phase,
    ) -> Result<(bool, usize), i32> {
        let utf = self.utf;
        if proptype >= 0 {
            let (c, l) = getchar_test(self.subj, eptr, utf);
            let notmatch = ctype == OP_NOTPROP;
            return match self.has_prop(c, proptype as u32, propvalue) {
                None => Err(PCRE2_ERROR_INTERNAL),
                Some(h) => Ok(if h != notmatch {
                    (true, eptr + l)
                } else {
                    (
                        false,
                        if phase == Phase::MaxScan {
                            eptr
                        } else {
                            eptr + l
                        },
                    )
                }),
            };
        }
        match ctype {
            OP_EXTUNI => {
                let (c, l) = getchar_test(self.subj, eptr, utf);
                Ok((
                    true,
                    extuni(c, self.subj, eptr + l, 0, self.end_subject, utf, None),
                ))
            }
            OP_ANY => {
                if self.is_newline(eptr) {
                    Ok((false, eptr))
                } else {
                    Ok((true, self.fwd_char(eptr)))
                }
            }
            OP_ALLANY => Ok((true, self.fwd_char(eptr))),
            OP_ANYBYTE => Ok((true, eptr + 1)),
            _ => {
                let (c, l) = getchar_test(self.subj, eptr, utf);
                let p = eptr + l;
                let (ok, newp, inc_on_fail) = match ctype {
                    OP_ANYNL => match c {
                        0x0d => (
                            true,
                            if p < self.end_subject && self.subj[p] == b'\n' {
                                p + 1
                            } else {
                                p
                            },
                            true,
                        ),
                        0x0a => (true, p, true),
                        0x0b | 0x0c | 0x85 | 0x2028 | 0x2029 => {
                            (self.bsr_convention != PCRE2_BSR_ANYCRLF as u16, p, true)
                        }
                        _ => (false, p, true),
                    },
                    OP_NOT_HSPACE => (!is_hspace(c), p, true),
                    OP_HSPACE => (is_hspace(c), p, true),
                    OP_NOT_VSPACE => (!is_vspace(c), p, true),
                    OP_VSPACE => (is_vspace(c), p, true),
                    OP_NOT_DIGIT => (!(c < 256 && (self.ctypes(c) & ctype_digit) != 0), p, utf),
                    OP_DIGIT => (c < 256 && (self.ctypes(c) & ctype_digit) != 0, p, false),
                    OP_NOT_WHITESPACE => {
                        (!(c < 256 && (self.ctypes(c) & ctype_space) != 0), p, false)
                    }
                    OP_WHITESPACE => (c < 256 && (self.ctypes(c) & ctype_space) != 0, p, false),
                    OP_NOT_WORDCHAR => (!(c < 256 && (self.ctypes(c) & ctype_word) != 0), p, false),
                    OP_WORDCHAR => (c < 256 && (self.ctypes(c) & ctype_word) != 0, p, false),
                    _ => return Err(PCRE2_ERROR_INTERNAL),
                };
                if ok {
                    return Ok((true, newp));
                }
                let inc = match phase {
                    Phase::Single | Phase::MinLoop => true,
                    Phase::MinPhase => inc_on_fail,
                    Phase::MaxScan => false,
                };
                Ok((false, if inc { p } else { eptr }))
            }
        }
    }

    /// match_ref(): returns (rc, length): rc 0 = matched, -1 = no match,
    /// 1 = hit end of subject.
    fn match_ref(&self, offset: usize, caseless: bool) -> (i32, usize) {
        let f = self.f;
        let fr = &self.frames[f];
        let ov = &self.ovec[f * self.ovs..];
        if offset >= fr.offset_top || ov[offset] == UNSET {
            if (self.poptions & PCRE2_MATCH_UNSET_BACKREF) != 0 {
                return (0, 0);
            }
            return (-1, 0);
        }
        let eptr_start = fr.eptr;
        let mut eptr = eptr_start;
        let mut p = ov[offset];
        let length = ov[offset + 1] - ov[offset];
        let s = self.subj;
        if caseless {
            if self.utf || (self.poptions & PCRE2_UCP) != 0 {
                let endptr = p + length;
                while p < endptr {
                    if eptr >= self.end_subject {
                        return (1, 0);
                    }
                    let (c, d);
                    if self.utf {
                        let (cc, l1) = getchar(s, eptr);
                        eptr += l1;
                        let (dd, l2) = getchar(s, p);
                        p += l2;
                        c = cc;
                        d = dd;
                    } else {
                        c = s[eptr] as u32;
                        eptr += 1;
                        d = s[p] as u32;
                        p += 1;
                    }
                    let ur = get_ucd(d);
                    if c != d && c != (d as i32 + ur.other_case) as u32 {
                        let mut pp = ur.caseset as usize;
                        loop {
                            let v = tables::UCD_CASELESS_SETS[pp];
                            if c < v {
                                return (-1, 0);
                            }
                            pp += 1;
                            if c == v {
                                break;
                            }
                        }
                    }
                }
            } else {
                for _ in 0..length {
                    if eptr >= self.end_subject {
                        return (1, 0);
                    }
                    if self.lcc(s[p] as u32) != self.lcc(s[eptr] as u32) {
                        return (-1, 0);
                    }
                    p += 1;
                    eptr += 1;
                }
            }
        } else {
            if self.end_subject - eptr < length {
                return (1, 0);
            }
            if s[p..p + length] != s[eptr..eptr + length] {
                return (-1, 0);
            }
            eptr += length;
        }
        (0, eptr - eptr_start)
    }

    fn new_frame(&mut self, ecode: usize, gft: u32) -> Act {
        let f = self.f;
        let fr = &mut self.frames[f];
        fr.group_frame_type = gft;
        fr.ecode = ecode;
        fr.back_frame = 1;
        if gft != 0 {
            fr.last_group_offset = f;
            if gf_idmask(gft) == GF_RECURSE {
                fr.current_recurse = gf_datamask(gft);
            }
        }
        let rdepth = fr.rdepth;
        let c = self.match_call_count;
        self.match_call_count = c.wrapping_add(1);
        if c >= self.match_limit {
            return Act::Ret(PCRE2_ERROR_MATCHLIMIT);
        }
        if rdepth >= self.match_limit_depth {
            return Act::Ret(if self.jit {
                PCRE2_ERROR_JIT_STACKLIMIT
            } else {
                PCRE2_ERROR_DEPTHLIMIT
            });
        }
        Act::Next
    }

    /// The match() function: returns MATCH_xxx or a negative error.
    fn run(&mut self, start_eptr: usize, start_ecode: usize) -> i32 {
        self.f = 0;
        if self.frames.is_empty() {
            self.frames.push(Frame::default());
        }
        self.frames[0] = Frame {
            rdepth: 0,
            capture_last: 0,
            current_recurse: RECURSE_UNSET,
            start_match: start_eptr,
            eptr: start_eptr,
            mark: UNSET,
            offset_top: 0,
            last_group_offset: UNSET,
            ..Frame::default()
        };
        if self.ovec.len() < self.ovs {
            self.ovec.resize(self.ovs, UNSET);
        }
        for v in self.ovec[..self.ovs].iter_mut() {
            *v = UNSET;
        }
        self.assert_accept_frame = UNSET;
        self.branch_end = UNSET;
        let mut act = self.new_frame(start_ecode, 0);
        loop {
            act = match act {
                Act::Next => self.exec(),
                Act::Rmatch(ecode, id, gft) => {
                    let f = self.f;
                    #[cfg(feature = "trace")]
                    eprintln!(
                        "++ RMATCH {} frame={} type={:x} eptr={} ecode={}",
                        id,
                        self.frames[f].rdepth + 1,
                        gft,
                        self.frames[f].eptr,
                        ecode
                    );
                    self.frames[f].return_id = id;
                    let n = f + 1;
                    if self.frames.len() <= n {
                        self.frames.push(Frame::default());
                    }
                    if self.ovec.len() < (n + 1) * self.ovs {
                        self.ovec.resize((n + 1) * self.ovs, UNSET);
                    }
                    self.frames[n] = self.frames[f];
                    if self.ovs > 0 {
                        self.ovec
                            .copy_within(f * self.ovs..(f + 1) * self.ovs, n * self.ovs);
                    }
                    self.frames[n].rdepth = self.frames[f].rdepth + 1;
                    self.f = n;
                    self.new_frame(ecode, gft)
                }
                Act::Rreturn(rrc) => {
                    let f = self.f;
                    if self.frames[f].eptr > self.last_used_ptr {
                        self.last_used_ptr = self.frames[f].eptr;
                    }
                    if self.frames[f].rdepth == 0 {
                        return rrc;
                    }
                    self.f = f - self.frames[f].back_frame;
                    let id = self.frames[self.f].return_id;
                    #[cfg(feature = "trace")]
                    eprintln!("++ RETURN {} to RM{}", rrc, id);
                    self.resume(id, rrc)
                }
                Act::Ret(rc) => return rc,
            };
        }
    }

    /// Execute opcodes in the current frame until a recursion or return.
    #[allow(clippy::cognitive_complexity)]
    fn exec(&mut self) -> Act {
        let f = self.f;
        let ovs = self.ovs;
        macro_rules! F {
            () => {
                self.frames[f]
            };
        }
        macro_rules! OV {
            ($i:expr) => {
                self.ovec[f * ovs + $i]
            };
        }
        macro_rules! rmatch {
            ($e:expr, $id:expr) => {
                return Act::Rmatch($e, $id, 0)
            };
            ($e:expr, $id:expr, $g:expr) => {
                return Act::Rmatch($e, $id, $g)
            };
        }
        macro_rules! rreturn {
            ($v:expr) => {
                return Act::Rreturn($v)
            };
        }
        macro_rules! nomatch_if_end {
            () => {
                if F!().eptr >= self.end_subject {
                    rreturn!(MATCH_NOMATCH);
                }
            };
        }
        let utf = self.utf;
        let ucp = self.ucp;

        loop {
            let ecode = F!().ecode;
            let op = self.code[ecode] as u32;
            F!().op = op as u8;
            match op {
                OP_CLOSE => {
                    if F!().current_recurse == RECURSE_UNSET {
                        let number = get2(self.code, ecode + 1) as usize;
                        let mut offset = F!().last_group_offset;
                        let p;
                        loop {
                            if offset == UNSET {
                                return Act::Ret(PCRE2_ERROR_INTERNAL);
                            }
                            let n = offset;
                            let pp = n - 1;
                            if self.frames[n].group_frame_type == (GF_CAPTURE | number as u32) {
                                p = pp;
                                break;
                            }
                            offset = self.frames[pp].last_group_offset;
                        }
                        let off = (number << 1) - 2;
                        F!().capture_last = number as u32;
                        OV!(off) = self.frames[p].eptr;
                        OV!(off + 1) = F!().eptr;
                        if off >= F!().offset_top {
                            F!().offset_top = off + 2;
                        }
                    }
                    F!().ecode += op_len(self.code[ecode]);
                }

                OP_ASSERT_ACCEPT => {
                    if F!().eptr > self.last_used_ptr {
                        self.last_used_ptr = F!().eptr;
                    }
                    self.assert_accept_frame = f;
                    rreturn!(MATCH_ACCEPT);
                }

                OP_ACCEPT | OP_END => {
                    if op == OP_ACCEPT && F!().current_recurse != RECURSE_UNSET {
                        let mut offset = F!().last_group_offset;
                        let p;
                        loop {
                            if offset == UNSET {
                                return Act::Ret(PCRE2_ERROR_INTERNAL);
                            }
                            let n = offset;
                            let pp = n - 1;
                            if gf_idmask(self.frames[n].group_frame_type) == GF_RECURSE {
                                p = pp;
                                break;
                            }
                            offset = self.frames[pp].last_group_offset;
                        }
                        // F = P: continue in the caller's frame, which has
                        // its own ovector.
                        self.frames[p].eptr = F!().eptr;
                        self.frames[p].mark = F!().mark;
                        self.frames[p].start_match = F!().start_match;
                        self.f = p;
                        self.frames[p].ecode += 1 + LINK_SIZE;
                        return Act::Next;
                    }
                    let eptr = F!().eptr;
                    let sm = F!().start_match;
                    if eptr == sm
                        && ((self.moptions & PCRE2_NOTEMPTY) != 0
                            || ((self.moptions & PCRE2_NOTEMPTY_ATSTART) != 0
                                && sm == self.start_offset))
                    {
                        rreturn!(MATCH_NOMATCH);
                    }
                    if eptr < self.end_subject
                        && ((self.moptions | self.poptions) & PCRE2_ENDANCHORED) != 0
                    {
                        if op == OP_END {
                            rreturn!(MATCH_NOMATCH);
                        }
                        return Act::Ret(MATCH_NOMATCH);
                    }
                    self.end_match_ptr = eptr;
                    self.end_offset_top = F!().offset_top;
                    self.mark = F!().mark;
                    if eptr > self.last_used_ptr {
                        self.last_used_ptr = eptr;
                    }
                    self.out_ovector[0] = sm;
                    self.out_ovector[1] = eptr;
                    let tb = self.top_bracket as usize;
                    let mut i = 2 * if tb + 1 > self.oveccount {
                        self.oveccount
                    } else {
                        tb + 1
                    };
                    for k in 0..(i - 2) {
                        self.out_ovector[2 + k] = OV!(k);
                    }
                    let ot = F!().offset_top;
                    while i > ot + 2 {
                        i -= 1;
                        self.out_ovector[i] = PCRE2_UNSET;
                    }
                    return Act::Ret(MATCH_MATCH);
                }

                OP_ANY | OP_ALLANY => {
                    if op == OP_ANY && self.is_newline(F!().eptr) {
                        rreturn!(MATCH_NOMATCH);
                    }
                    nomatch_if_end!();
                    let e = self.fwd_char(F!().eptr);
                    F!().eptr = e;
                    F!().ecode += 1;
                }

                OP_ANYBYTE => {
                    nomatch_if_end!();
                    F!().eptr += 1;
                    F!().ecode += 1;
                }

                OP_CHAR => {
                    if utf {
                        let (_, len) = getchar(self.code, ecode + 1);
                        let eptr = F!().eptr;
                        if len > self.end_subject - eptr {
                            rreturn!(MATCH_NOMATCH);
                        }
                        for k in 0..len {
                            if self.code[ecode + 1 + k] != self.subj[eptr + k] {
                                // C advances Feptr past the failing byte.
                                F!().eptr = eptr + k + 1;
                                rreturn!(MATCH_NOMATCH);
                            }
                        }
                        F!().ecode += 1 + len;
                        F!().eptr += len;
                    } else {
                        nomatch_if_end!();
                        let eptr = F!().eptr;
                        F!().eptr += 1;
                        if self.code[ecode + 1] != self.subj[eptr] {
                            rreturn!(MATCH_NOMATCH);
                        }
                        F!().ecode += 2;
                    }
                }

                OP_CHARI => {
                    nomatch_if_end!();
                    let eptr = F!().eptr;
                    if utf {
                        let (fc, len) = getchar(self.code, ecode + 1);
                        if fc < 128 {
                            let cc = self.sb(eptr);
                            if self.lcc(fc) != self.lcc(cc) {
                                rreturn!(MATCH_NOMATCH);
                            }
                            F!().ecode += 2;
                            F!().eptr += 1;
                        } else {
                            let (dc, dl) = getchar(self.subj, eptr);
                            F!().eptr += dl;
                            F!().ecode += 1 + len;
                            if dc != fc && dc != ucd_othercase(fc) {
                                rreturn!(MATCH_NOMATCH);
                            }
                        }
                    } else if ucp {
                        let cc = self.sb(eptr);
                        let fc = self.cb(ecode + 1);
                        if fc < 128 {
                            if self.lcc(fc) != self.lcc(cc) {
                                rreturn!(MATCH_NOMATCH);
                            }
                        } else if cc != fc && cc != ucd_othercase(fc) {
                            rreturn!(MATCH_NOMATCH);
                        }
                        F!().eptr += 1;
                        F!().ecode += 2;
                    } else {
                        if self.lcc(self.cb(ecode + 1)) != self.lcc(self.sb(eptr)) {
                            rreturn!(MATCH_NOMATCH);
                        }
                        F!().eptr += 1;
                        F!().ecode += 2;
                    }
                }

                OP_NOT | OP_NOTI => {
                    nomatch_if_end!();
                    let eptr = F!().eptr;
                    if utf {
                        let (mut ch, cl) = getchar(self.code, ecode + 1);
                        F!().ecode += 1 + cl;
                        let (fc, fl) = getchar(self.subj, eptr);
                        F!().eptr += fl;
                        if ch == fc {
                            rreturn!(MATCH_NOMATCH);
                        } else if op == OP_NOTI {
                            ch = if ch > 127 {
                                ucd_othercase(ch)
                            } else {
                                self.fcc(ch)
                            };
                            if ch == fc {
                                rreturn!(MATCH_NOMATCH);
                            }
                        }
                    } else if ucp {
                        let fc = self.sb(eptr);
                        F!().eptr += 1;
                        let mut ch = self.cb(ecode + 1);
                        F!().ecode += 2;
                        if ch == fc {
                            rreturn!(MATCH_NOMATCH);
                        } else if op == OP_NOTI {
                            ch = if ch > 127 {
                                ucd_othercase(ch)
                            } else {
                                self.fcc(ch)
                            };
                            if ch == fc {
                                rreturn!(MATCH_NOMATCH);
                            }
                        }
                    } else {
                        let ch = self.cb(ecode + 1);
                        let fc = self.sb(eptr);
                        F!().eptr += 1;
                        if ch == fc || (op == OP_NOTI && self.fcc(ch) == fc) {
                            rreturn!(MATCH_NOMATCH);
                        }
                        F!().ecode += 2;
                    }
                }

                // ---- Single character repeats ----
                OP_EXACT | OP_EXACTI | OP_POSUPTO | OP_POSUPTOI | OP_UPTO | OP_UPTOI
                | OP_MINUPTO | OP_MINUPTOI | OP_POSSTAR | OP_POSSTARI | OP_POSPLUS
                | OP_POSPLUSI | OP_POSQUERY | OP_POSQUERYI | OP_STAR | OP_STARI | OP_MINSTAR
                | OP_MINSTARI | OP_PLUS | OP_PLUSI | OP_MINPLUS | OP_MINPLUSI | OP_QUERY
                | OP_QUERYI | OP_MINQUERY | OP_MINQUERYI => {
                    let reptype;
                    match op {
                        OP_EXACT | OP_EXACTI => {
                            let n = get2(self.code, ecode + 1);
                            F!().temp_32[0] = n;
                            F!().temp_32[1] = n;
                            reptype = REPTYPE_MAX; // unused
                            F!().ecode += 1 + IMM2_SIZE;
                        }
                        OP_POSUPTO | OP_POSUPTOI | OP_UPTO | OP_UPTOI | OP_MINUPTO
                        | OP_MINUPTOI => {
                            reptype = match op {
                                OP_POSUPTO | OP_POSUPTOI => REPTYPE_POS,
                                OP_UPTO | OP_UPTOI => REPTYPE_MAX,
                                _ => REPTYPE_MIN,
                            };
                            F!().temp_32[0] = 0;
                            F!().temp_32[1] = get2(self.code, ecode + 1);
                            F!().ecode += 1 + IMM2_SIZE;
                        }
                        OP_POSSTAR | OP_POSSTARI => {
                            reptype = REPTYPE_POS;
                            F!().temp_32[0] = 0;
                            F!().temp_32[1] = u32::MAX;
                            F!().ecode += 1;
                        }
                        OP_POSPLUS | OP_POSPLUSI => {
                            reptype = REPTYPE_POS;
                            F!().temp_32[0] = 1;
                            F!().temp_32[1] = u32::MAX;
                            F!().ecode += 1;
                        }
                        OP_POSQUERY | OP_POSQUERYI => {
                            reptype = REPTYPE_POS;
                            F!().temp_32[0] = 0;
                            F!().temp_32[1] = 1;
                            F!().ecode += 1;
                        }
                        _ => {
                            let fc = (op - if op < OP_STARI { OP_STAR } else { OP_STARI }) as usize;
                            F!().ecode += 1;
                            F!().temp_32[0] = REP_MIN[fc];
                            F!().temp_32[1] = REP_MAX[fc];
                            reptype = REP_TYP[fc];
                        }
                    }
                    // REPEATCHAR
                    let caseless = op >= OP_STARI;
                    if utf {
                        let ec = F!().ecode;
                        let (fc, len) = getchar(self.code, ec);
                        F!().length = len;
                        F!().temp_sptr[1] = ec; // Lcharptr
                        F!().ecode += len;
                        if len > 1 {
                            let mut occu = [0u8; 6];
                            let mut oclen = 0;
                            if caseless {
                                let othercase = ucd_othercase(fc);
                                if othercase != fc {
                                    oclen = ord2utf(othercase, &mut occu);
                                }
                            }
                            F!().temp_size = oclen;
                            F!().occu = occu;
                            let lmin = F!().temp_32[0];
                            for _ in 0..lmin {
                                match self.char_utf_step() {
                                    Some(e) => F!().eptr = e,
                                    None => rreturn!(MATCH_NOMATCH),
                                }
                            }
                            if F!().temp_32[0] == F!().temp_32[1] {
                                continue;
                            }
                            if reptype == REPTYPE_MIN {
                                rmatch!(F!().ecode, RM_CHAR_UTF_MIN);
                            }
                            F!().temp_sptr[0] = F!().eptr;
                            let mut i = F!().temp_32[0];
                            while i < F!().temp_32[1] {
                                match self.char_utf_step() {
                                    Some(e) => F!().eptr = e,
                                    None => break,
                                }
                                i += 1;
                            }
                            if reptype != REPTYPE_POS {
                                if F!().eptr <= F!().temp_sptr[0] {
                                    continue;
                                }
                                rmatch!(F!().ecode, RM_CHAR_UTF_MAX);
                            }
                            continue;
                        }
                        F!().temp_32[2] = fc;
                    } else {
                        let ec = F!().ecode;
                        F!().temp_32[2] = self.cb(ec);
                        F!().ecode += 1;
                    }
                    let lc = F!().temp_32[2];
                    if caseless {
                        let loc = if ucp && !utf && lc > 127 {
                            ucd_othercase(lc)
                        } else {
                            self.fcc(lc)
                        };
                        F!().temp_32[3] = loc;
                        let lmin = F!().temp_32[0];
                        for _ in 0..lmin {
                            nomatch_if_end!();
                            let cc = self.sb(F!().eptr);
                            if lc != cc && loc != cc {
                                rreturn!(MATCH_NOMATCH);
                            }
                            F!().eptr += 1;
                        }
                        if F!().temp_32[0] == F!().temp_32[1] {
                            continue;
                        }
                        if reptype == REPTYPE_MIN {
                            rmatch!(F!().ecode, RM_CHARI_MIN);
                        }
                        F!().temp_sptr[0] = F!().eptr;
                        let mut i = F!().temp_32[0];
                        while i < F!().temp_32[1] {
                            if F!().eptr >= self.end_subject {
                                break;
                            }
                            let cc = self.sb(F!().eptr);
                            if lc != cc && loc != cc {
                                break;
                            }
                            F!().eptr += 1;
                            i += 1;
                        }
                        if reptype != REPTYPE_POS {
                            if F!().eptr == F!().temp_sptr[0] {
                                continue;
                            }
                            rmatch!(F!().ecode, RM_CHARI_MAX);
                        }
                    } else {
                        let lmin = F!().temp_32[0];
                        for _ in 0..lmin {
                            nomatch_if_end!();
                            let e = F!().eptr;
                            F!().eptr += 1;
                            if lc != self.sb(e) {
                                rreturn!(MATCH_NOMATCH);
                            }
                        }
                        if F!().temp_32[0] == F!().temp_32[1] {
                            continue;
                        }
                        if reptype == REPTYPE_MIN {
                            rmatch!(F!().ecode, RM_CHAR_MIN);
                        }
                        F!().temp_sptr[0] = F!().eptr;
                        let mut i = F!().temp_32[0];
                        while i < F!().temp_32[1] {
                            if F!().eptr >= self.end_subject {
                                break;
                            }
                            if lc != self.sb(F!().eptr) {
                                break;
                            }
                            F!().eptr += 1;
                            i += 1;
                        }
                        if reptype != REPTYPE_POS {
                            if F!().eptr <= F!().temp_sptr[0] {
                                continue;
                            }
                            rmatch!(F!().ecode, RM_CHAR_MAX);
                        }
                    }
                }

                // ---- Negated single character repeats ----
                OP_NOTEXACT | OP_NOTEXACTI | OP_NOTUPTO | OP_NOTUPTOI | OP_NOTMINUPTO
                | OP_NOTMINUPTOI | OP_NOTPOSSTAR | OP_NOTPOSSTARI | OP_NOTPOSPLUS
                | OP_NOTPOSPLUSI | OP_NOTPOSQUERY | OP_NOTPOSQUERYI | OP_NOTPOSUPTO
                | OP_NOTPOSUPTOI | OP_NOTSTAR | OP_NOTSTARI | OP_NOTMINSTAR | OP_NOTMINSTARI
                | OP_NOTPLUS | OP_NOTPLUSI | OP_NOTMINPLUS | OP_NOTMINPLUSI | OP_NOTQUERY
                | OP_NOTQUERYI | OP_NOTMINQUERY | OP_NOTMINQUERYI => {
                    let reptype;
                    match op {
                        OP_NOTEXACT | OP_NOTEXACTI => {
                            let n = get2(self.code, ecode + 1);
                            F!().temp_32[0] = n;
                            F!().temp_32[1] = n;
                            reptype = REPTYPE_MAX;
                            F!().ecode += 1 + IMM2_SIZE;
                        }
                        OP_NOTUPTO | OP_NOTUPTOI | OP_NOTMINUPTO | OP_NOTMINUPTOI
                        | OP_NOTPOSUPTO | OP_NOTPOSUPTOI => {
                            F!().temp_32[0] = 0;
                            F!().temp_32[1] = get2(self.code, ecode + 1);
                            reptype = match op {
                                OP_NOTUPTO | OP_NOTUPTOI => REPTYPE_MAX,
                                OP_NOTMINUPTO | OP_NOTMINUPTOI => REPTYPE_MIN,
                                _ => REPTYPE_POS,
                            };
                            F!().ecode += 1 + IMM2_SIZE;
                        }
                        OP_NOTPOSSTAR | OP_NOTPOSSTARI => {
                            reptype = REPTYPE_POS;
                            F!().temp_32[0] = 0;
                            F!().temp_32[1] = u32::MAX;
                            F!().ecode += 1;
                        }
                        OP_NOTPOSPLUS | OP_NOTPOSPLUSI => {
                            reptype = REPTYPE_POS;
                            F!().temp_32[0] = 1;
                            F!().temp_32[1] = u32::MAX;
                            F!().ecode += 1;
                        }
                        OP_NOTPOSQUERY | OP_NOTPOSQUERYI => {
                            reptype = REPTYPE_POS;
                            F!().temp_32[0] = 0;
                            F!().temp_32[1] = 1;
                            F!().ecode += 1;
                        }
                        _ => {
                            let fc = (op
                                - if op >= OP_NOTSTARI {
                                    OP_NOTSTARI
                                } else {
                                    OP_NOTSTAR
                                }) as usize;
                            F!().ecode += 1;
                            F!().temp_32[0] = REP_MIN[fc];
                            F!().temp_32[1] = REP_MAX[fc];
                            reptype = REP_TYP[fc];
                        }
                    }
                    // REPEATNOTCHAR
                    let ec = F!().ecode;
                    let (lc, l) = getchar_test(self.code, ec, utf);
                    F!().ecode += l;
                    F!().temp_32[2] = lc;
                    let caseless = op >= OP_NOTSTARI;
                    let loc = if caseless {
                        if (utf || ucp) && lc > 127 {
                            ucd_othercase(lc)
                        } else {
                            self.fcc(lc)
                        }
                    } else {
                        lc
                    };
                    F!().temp_32[3] = loc;
                    let lmin = F!().temp_32[0];
                    for _ in 0..lmin {
                        nomatch_if_end!();
                        let mut e = F!().eptr;
                        let d = self.getc_inc(&mut e);
                        if lc == d || loc == d {
                            // Non-UTF caseless does not advance before failing.
                            if utf || !caseless {
                                F!().eptr = e;
                            }
                            rreturn!(MATCH_NOMATCH);
                        }
                        F!().eptr = e;
                    }
                    if F!().temp_32[0] == F!().temp_32[1] {
                        continue;
                    }
                    if reptype == REPTYPE_MIN {
                        rmatch!(F!().ecode, if caseless { RM_NOTI_MIN } else { RM_NOT_MIN });
                    }
                    F!().temp_sptr[0] = F!().eptr;
                    let mut i = F!().temp_32[0];
                    while i < F!().temp_32[1] {
                        if F!().eptr >= self.end_subject {
                            break;
                        }
                        let (d, len) = getchar_test(self.subj, F!().eptr, utf);
                        if lc == d || loc == d {
                            break;
                        }
                        F!().eptr += len;
                        i += 1;
                    }
                    if reptype != REPTYPE_POS {
                        if utf {
                            if F!().eptr <= F!().temp_sptr[0] {
                                continue;
                            }
                            rmatch!(
                                F!().ecode,
                                if caseless {
                                    RM_NOTI_MAX_UTF
                                } else {
                                    RM_NOT_MAX_UTF
                                }
                            );
                        } else {
                            if F!().eptr == F!().temp_sptr[0] {
                                continue;
                            }
                            rmatch!(F!().ecode, if caseless { RM_NOTI_MAX } else { RM_NOT_MAX });
                        }
                    }
                }

                // ---- Character classes ----
                OP_NCLASS | OP_CLASS => {
                    F!().temp_sptr[1] = ecode + 1; // Lbyte_map
                    F!().ecode += 1 + 32;
                    let reptype = self.read_cr_repeat();
                    let map = ecode + 1;
                    let lmin = F!().temp_32[0];
                    for _ in 0..lmin {
                        nomatch_if_end!();
                        let mut e = F!().eptr;
                        let fc = self.getc_inc(&mut e);
                        F!().eptr = e;
                        if !self.class_ok(op, map, fc) {
                            rreturn!(MATCH_NOMATCH);
                        }
                    }
                    if F!().temp_32[0] == F!().temp_32[1] {
                        continue;
                    }
                    if reptype == REPTYPE_MIN {
                        rmatch!(F!().ecode, RM_CLASS_MIN);
                    }
                    F!().temp_sptr[0] = F!().eptr;
                    let mut i = F!().temp_32[0];
                    while i < F!().temp_32[1] {
                        if F!().eptr >= self.end_subject {
                            break;
                        }
                        let (fc, len) = getchar_test(self.subj, F!().eptr, utf);
                        if !self.class_ok(op, map, fc) {
                            break;
                        }
                        F!().eptr += len;
                        i += 1;
                    }
                    if reptype == REPTYPE_POS {
                        continue;
                    }
                    if utf {
                        rmatch!(F!().ecode, RM_CLASS_MAX_UTF);
                    }
                    // while (Feptr >= Lstart_eptr) { RMATCH ... }
                    rmatch!(F!().ecode, RM_CLASS_MAX);
                }

                OP_XCLASS => {
                    F!().temp_sptr[1] = ecode + 1 + LINK_SIZE;
                    F!().ecode += get(self.code, ecode + 1);
                    let reptype = self.read_cr_repeat();
                    let data = ecode + 1 + LINK_SIZE;
                    let lmin = F!().temp_32[0];
                    for _ in 0..lmin {
                        nomatch_if_end!();
                        let mut e = F!().eptr;
                        let fc = self.getc_inc(&mut e);
                        F!().eptr = e;
                        if !xclass(fc, self.code, data, utf) {
                            rreturn!(MATCH_NOMATCH);
                        }
                    }
                    if F!().temp_32[0] == F!().temp_32[1] {
                        continue;
                    }
                    if reptype == REPTYPE_MIN {
                        rmatch!(F!().ecode, RM_XCLASS_MIN);
                    }
                    F!().temp_sptr[0] = F!().eptr;
                    let mut i = F!().temp_32[0];
                    while i < F!().temp_32[1] {
                        if F!().eptr >= self.end_subject {
                            break;
                        }
                        let (fc, len) = getchar_test(self.subj, F!().eptr, utf);
                        if !xclass(fc, self.code, data, utf) {
                            break;
                        }
                        F!().eptr += len;
                        i += 1;
                    }
                    if reptype == REPTYPE_POS {
                        continue;
                    }
                    rmatch!(F!().ecode, RM_XCLASS_MAX);
                }

                // ---- Generic character types ----
                OP_NOT_DIGIT | OP_DIGIT | OP_NOT_WHITESPACE | OP_WHITESPACE | OP_NOT_WORDCHAR
                | OP_WORDCHAR | OP_NOT_HSPACE | OP_HSPACE | OP_NOT_VSPACE | OP_VSPACE => {
                    nomatch_if_end!();
                    match self.type_one(op, -1, 0, F!().eptr, Phase::Single) {
                        Err(e) => return Act::Ret(e),
                        Ok((ok, e)) => {
                            F!().eptr = e;
                            if !ok {
                                rreturn!(MATCH_NOMATCH);
                            }
                        }
                    }
                    F!().ecode += 1;
                }

                OP_ANYNL => {
                    nomatch_if_end!();
                    match self.type_one(op, -1, 0, F!().eptr, Phase::Single) {
                        Err(e) => return Act::Ret(e),
                        Ok((ok, e)) => {
                            F!().eptr = e;
                            if !ok {
                                rreturn!(MATCH_NOMATCH);
                            }
                        }
                    }
                    F!().ecode += 1;
                }

                OP_PROP | OP_NOTPROP => {
                    nomatch_if_end!();
                    let mut e = F!().eptr;
                    let fc = self.getc_inc(&mut e);
                    F!().eptr = e;
                    let notmatch = op == OP_NOTPROP;
                    match self.has_prop(fc, self.cb(ecode + 1), self.cb(ecode + 2)) {
                        None => return Act::Ret(PCRE2_ERROR_INTERNAL),
                        Some(h) => {
                            if h == notmatch {
                                rreturn!(MATCH_NOMATCH);
                            }
                        }
                    }
                    F!().ecode += 3;
                }

                OP_EXTUNI => {
                    nomatch_if_end!();
                    let mut e = F!().eptr;
                    let fc = self.getc_inc(&mut e);
                    F!().eptr = extuni(fc, self.subj, e, 0, self.end_subject, utf, None);
                    F!().ecode += 1;
                }

                // ---- Repeated character types ----
                OP_TYPEEXACT | OP_TYPEUPTO | OP_TYPEMINUPTO | OP_TYPEPOSSTAR | OP_TYPEPOSPLUS
                | OP_TYPEPOSQUERY | OP_TYPEPOSUPTO | OP_TYPESTAR | OP_TYPEMINSTAR | OP_TYPEPLUS
                | OP_TYPEMINPLUS | OP_TYPEQUERY | OP_TYPEMINQUERY => {
                    let reptype;
                    match op {
                        OP_TYPEEXACT => {
                            let n = get2(self.code, ecode + 1);
                            F!().temp_32[0] = n;
                            F!().temp_32[1] = n;
                            reptype = REPTYPE_MAX;
                            F!().ecode += 1 + IMM2_SIZE;
                        }
                        OP_TYPEUPTO | OP_TYPEMINUPTO => {
                            F!().temp_32[0] = 0;
                            F!().temp_32[1] = get2(self.code, ecode + 1);
                            reptype = if op == OP_TYPEMINUPTO {
                                REPTYPE_MIN
                            } else {
                                REPTYPE_MAX
                            };
                            F!().ecode += 1 + IMM2_SIZE;
                        }
                        OP_TYPEPOSSTAR => {
                            reptype = REPTYPE_POS;
                            F!().temp_32[0] = 0;
                            F!().temp_32[1] = u32::MAX;
                            F!().ecode += 1;
                        }
                        OP_TYPEPOSPLUS => {
                            reptype = REPTYPE_POS;
                            F!().temp_32[0] = 1;
                            F!().temp_32[1] = u32::MAX;
                            F!().ecode += 1;
                        }
                        OP_TYPEPOSQUERY => {
                            reptype = REPTYPE_POS;
                            F!().temp_32[0] = 0;
                            F!().temp_32[1] = 1;
                            F!().ecode += 1;
                        }
                        OP_TYPEPOSUPTO => {
                            reptype = REPTYPE_POS;
                            F!().temp_32[0] = 0;
                            F!().temp_32[1] = get2(self.code, ecode + 1);
                            F!().ecode += 1 + IMM2_SIZE;
                        }
                        _ => {
                            let fc = (op - OP_TYPESTAR) as usize;
                            F!().ecode += 1;
                            F!().temp_32[0] = REP_MIN[fc];
                            F!().temp_32[1] = REP_MAX[fc];
                            reptype = REP_TYP[fc];
                        }
                    }
                    // REPEATTYPE
                    let ec = F!().ecode;
                    let lctype = self.cb(ec);
                    F!().ecode += 1;
                    F!().temp_32[2] = lctype;
                    let proptype: i32;
                    if lctype == OP_PROP || lctype == OP_NOTPROP {
                        proptype = self.cb(ec + 1) as i32;
                        F!().temp_32[3] = self.cb(ec + 2);
                        F!().ecode += 2;
                    } else {
                        proptype = -1;
                    }
                    F!().proptype = proptype;
                    let propvalue = F!().temp_32[3];

                    let lmin = F!().temp_32[0];
                    if lmin > 0 {
                        if proptype >= 0 && proptype as u32 == PT_ANY && lctype == OP_NOTPROP {
                            rreturn!(MATCH_NOMATCH);
                        }
                        for _ in 0..lmin {
                            nomatch_if_end!();
                            match self.type_one(
                                lctype,
                                proptype,
                                propvalue,
                                F!().eptr,
                                Phase::MinPhase,
                            ) {
                                Err(e) => return Act::Ret(e),
                                Ok((ok, e)) => {
                                    F!().eptr = e;
                                    if !ok {
                                        rreturn!(MATCH_NOMATCH);
                                    }
                                }
                            }
                        }
                    }
                    if F!().temp_32[0] == F!().temp_32[1] {
                        continue;
                    }
                    if reptype == REPTYPE_MIN {
                        rmatch!(F!().ecode, RM_TYPE_MIN);
                    }
                    F!().temp_sptr[0] = F!().eptr;
                    let lmax = F!().temp_32[1];
                    let mut i = F!().temp_32[0];
                    if proptype < 0 && lctype == OP_ALLANY && (!utf || lmax == u32::MAX) {
                        // Fast path (identical result to scanning).
                        let rem = self.end_subject - F!().eptr;
                        let want = (lmax - i) as usize;
                        F!().eptr += if utf { rem } else { want.min(rem) };
                    } else if proptype < 0 && lctype == OP_ANYBYTE {
                        let rem = self.end_subject - F!().eptr;
                        let want = (lmax - i) as usize;
                        F!().eptr += want.min(rem);
                    } else {
                        while i < lmax {
                            if F!().eptr >= self.end_subject {
                                break;
                            }
                            match self.type_one(
                                lctype,
                                proptype,
                                propvalue,
                                F!().eptr,
                                Phase::MaxScan,
                            ) {
                                Err(e) => return Act::Ret(e),
                                Ok((false, _)) => break,
                                Ok((true, e)) => F!().eptr = e,
                            }
                            i += 1;
                        }
                    }
                    if reptype == REPTYPE_POS {
                        continue;
                    }
                    if F!().eptr <= F!().temp_sptr[0] {
                        continue;
                    }
                    let id = if proptype >= 0 {
                        RM_TYPE_MAX_PROP
                    } else if lctype == OP_EXTUNI {
                        RM_TYPE_MAX_EXTUNI
                    } else if utf {
                        RM_TYPE_MAX_UTF
                    } else {
                        RM_TYPE_MAX
                    };
                    rmatch!(F!().ecode, id);
                }

                // ---- Back references ----
                OP_DNREF | OP_DNREFI | OP_REF | OP_REFI => {
                    let caseless = op == OP_DNREFI || op == OP_REFI;
                    F!().temp_32[2] = caseless as u32;
                    if op == OP_DNREF || op == OP_DNREFI {
                        let mut count = get2(self.code, ecode + 1 + IMM2_SIZE);
                        let mut slot = self.name_table
                            + get2(self.code, ecode + 1) as usize * self.name_entry_size;
                        F!().ecode += 1 + 2 * IMM2_SIZE;
                        while count > 0 {
                            count -= 1;
                            let loff = ((get2(self.code, slot) as usize) << 1) - 2;
                            F!().temp_size = loff;
                            if loff < F!().offset_top && OV!(loff) != UNSET {
                                break;
                            }
                            slot += self.name_entry_size;
                        }
                    } else {
                        F!().temp_size = ((get2(self.code, ecode + 1) as usize) << 1) - 2;
                        F!().ecode += 1 + IMM2_SIZE;
                    }
                    // REF_REPEAT
                    let ec = F!().ecode;
                    let reptype;
                    match self.cb(ec) {
                        OP_CRSTAR | OP_CRMINSTAR | OP_CRPLUS | OP_CRMINPLUS | OP_CRQUERY
                        | OP_CRMINQUERY => {
                            let fc = (self.cb(ec) - OP_CRSTAR) as usize;
                            F!().ecode += 1;
                            F!().temp_32[0] = REP_MIN[fc];
                            F!().temp_32[1] = REP_MAX[fc];
                            reptype = REP_TYP[fc];
                        }
                        OP_CRRANGE | OP_CRMINRANGE => {
                            F!().temp_32[0] = get2(self.code, ec + 1);
                            let mut mx = get2(self.code, ec + 1 + IMM2_SIZE);
                            reptype = REP_TYP[(self.cb(ec) - OP_CRSTAR) as usize];
                            if mx == 0 {
                                mx = u32::MAX;
                            }
                            F!().temp_32[1] = mx;
                            F!().ecode += 1 + 2 * IMM2_SIZE;
                        }
                        _ => {
                            let (rrc, length) = self.match_ref(F!().temp_size, caseless);
                            if rrc != 0 {
                                if rrc > 0 {
                                    F!().eptr = self.end_subject;
                                }
                                rreturn!(MATCH_NOMATCH);
                            }
                            F!().eptr += length;
                            continue;
                        }
                    }
                    let loff = F!().temp_size;
                    if loff < F!().offset_top && OV!(loff) != UNSET {
                        if OV!(loff) == OV!(loff + 1) {
                            continue;
                        }
                    } else if F!().temp_32[0] == 0
                        || (self.poptions & PCRE2_MATCH_UNSET_BACKREF) != 0
                    {
                        continue;
                    }
                    let lmin = F!().temp_32[0];
                    for _ in 0..lmin {
                        let (rrc, slength) = self.match_ref(loff, caseless);
                        if rrc != 0 {
                            if rrc > 0 {
                                F!().eptr = self.end_subject;
                            }
                            rreturn!(MATCH_NOMATCH);
                        }
                        F!().eptr += slength;
                    }
                    if F!().temp_32[0] == F!().temp_32[1] {
                        continue;
                    }
                    if reptype == REPTYPE_MIN {
                        rmatch!(F!().ecode, RM_REF_MIN);
                    }
                    let mut samelengths = true;
                    F!().temp_sptr[0] = F!().eptr; // Lstart
                    F!().length = OV!(loff + 1).wrapping_sub(OV!(loff));
                    let mut i = F!().temp_32[0];
                    while i < F!().temp_32[1] {
                        let (rrc, slength) = self.match_ref(loff, caseless);
                        if rrc != 0 {
                            break;
                        }
                        if slength != F!().length {
                            samelengths = false;
                        }
                        F!().eptr += slength;
                        i += 1;
                    }
                    if samelengths {
                        // while (Feptr >= Lstart) { RMATCH; Feptr -= Flength; }
                        rmatch!(F!().ecode, RM_REF_MAX_SAME);
                    }
                    F!().temp_32[1] = i;
                    rmatch!(F!().ecode, RM_REF_MAX_DIFF);
                }

                // ---- Groups ----
                OP_BRAZERO => {
                    F!().temp_sptr[0] = ecode + 1;
                    rmatch!(ecode + 1, RM_BRAZERO);
                }
                OP_BRAMINZERO => {
                    let mut ne = ecode + 1;
                    loop {
                        ne += get(self.code, ne + 1);
                        if self.cb(ne) != OP_ALT {
                            break;
                        }
                    }
                    F!().temp_sptr[0] = ne;
                    rmatch!(ne + 1 + LINK_SIZE, RM_BRAMINZERO);
                }
                OP_SKIPZERO => {
                    let mut e = ecode + 1;
                    loop {
                        e += get(self.code, e + 1);
                        if self.cb(e) != OP_ALT {
                            break;
                        }
                    }
                    F!().ecode = e + 1 + LINK_SIZE;
                }

                OP_BRAPOSZERO | OP_BRAPOS | OP_SBRAPOS | OP_CBRAPOS | OP_SCBRAPOS => {
                    let mut e = ecode;
                    if op == OP_BRAPOSZERO {
                        F!().temp_32[2] = 1; // Lzero_allowed
                        e += 1;
                        F!().ecode = e;
                    } else {
                        F!().temp_32[2] = 0;
                    }
                    let o2 = self.cb(e);
                    if o2 == OP_CBRAPOS || o2 == OP_SCBRAPOS {
                        let number = get2(self.code, e + 1 + LINK_SIZE);
                        F!().temp_32[0] = GF_CAPTURE | number;
                    } else {
                        F!().temp_32[0] = GF_NOCAPTURE;
                    }
                    // POSSESSIVE_GROUP
                    F!().temp_32[1] = 0; // Lmatched_once
                    F!().temp_sptr[1] = e; // Lstart_group
                    F!().temp_sptr[0] = F!().eptr; // Lstart_eptr
                    let gft = F!().temp_32[0];
                    rmatch!(e + op_len(self.code[e]), RM_POS_GROUP, gft);
                }

                OP_BRA => {
                    if self.hasthen || F!().rdepth == 0 {
                        F!().temp_32[0] = 0;
                        rmatch!(ecode + op_len(self.code[ecode]), RM_GROUPLOOP, 0);
                    }
                    let nb = ecode + get(self.code, ecode + 1);
                    F!().temp_sptr[0] = nb;
                    if self.cb(nb) != OP_ALT {
                        F!().ecode += op_len(self.code[ecode]);
                        continue;
                    }
                    rmatch!(ecode + op_len(self.code[ecode]), RM_BRA_ALT);
                }

                OP_CBRA | OP_SCBRA | OP_ONCE | OP_SCRIPT_RUN | OP_SBRA => {
                    let ft = if op == OP_CBRA || op == OP_SCBRA {
                        GF_CAPTURE | get2(self.code, ecode + 1 + LINK_SIZE)
                    } else {
                        GF_NOCAPTURE | op
                    };
                    F!().temp_32[0] = ft;
                    rmatch!(ecode + op_len(self.code[ecode]), RM_GROUPLOOP, ft);
                }

                OP_RECURSE => {
                    let bracode = self.start_code + get(self.code, ecode + 1);
                    let number = if bracode == self.start_code {
                        0
                    } else {
                        get2(self.code, bracode + 1 + LINK_SIZE)
                    };
                    if F!().current_recurse != RECURSE_UNSET {
                        let mut offset = F!().last_group_offset;
                        while offset != UNSET {
                            let n = offset;
                            let p = n - 1;
                            if self.frames[n].group_frame_type == (GF_RECURSE | number) {
                                if F!().eptr == self.frames[p].eptr
                                    && self.last_used_ptr == self.frames[p].recurse_last_used
                                    && (self.moptions & PCRE2_DISABLE_RECURSELOOP_CHECK) == 0
                                    // The JIT has no loop check: it recurses until
                                    // its stack runs out.
                                    && !self.jit
                                {
                                    return Act::Ret(PCRE2_ERROR_RECURSELOOP);
                                }
                                break;
                            }
                            offset = self.frames[p].last_group_offset;
                        }
                    }
                    F!().recurse_last_used = self.last_used_ptr;
                    F!().temp_sptr[0] = bracode;
                    F!().temp_32[0] = GF_RECURSE | number;
                    rmatch!(
                        bracode + op_len(self.code[bracode]),
                        RM_RECURSE,
                        GF_RECURSE | number
                    );
                }

                OP_ASSERT | OP_ASSERTBACK | OP_ASSERT_NA | OP_ASSERTBACK_NA => {
                    let ft = GF_NOCAPTURE | op;
                    F!().temp_32[0] = ft;
                    rmatch!(ecode + op_len(self.code[ecode]), RM_ASSERT, ft);
                }

                OP_ASSERT_NOT | OP_ASSERTBACK_NOT => {
                    let ft = GF_NOCAPTURE | op;
                    F!().temp_32[0] = ft;
                    rmatch!(ecode + op_len(self.code[ecode]), RM_ASSERT_NOT, ft);
                }

                OP_CALLOUT | OP_CALLOUT_STR => {
                    // No callout function is ever set: just skip.
                    let length = if op == OP_CALLOUT {
                        op_len(OP_CALLOUT as u8)
                    } else {
                        get(self.code, ecode + 1 + 2 * LINK_SIZE)
                    };
                    F!().ecode += length;
                }

                OP_COND | OP_SCOND => {
                    let mut flength = get(self.code, ecode + 1);
                    if self.cb(ecode + flength) != OP_ALT {
                        flength -= 1 + LINK_SIZE;
                    }
                    let mut e = ecode + 1 + LINK_SIZE;
                    let co = self.cb(e);
                    if co == OP_CALLOUT || co == OP_CALLOUT_STR {
                        let length = if co == OP_CALLOUT {
                            op_len(OP_CALLOUT as u8)
                        } else {
                            get(self.code, e + 1 + 2 * LINK_SIZE)
                        };
                        e += length;
                        flength -= length;
                    }
                    F!().ecode = e;
                    F!().length = flength;
                    let mut condition = false;
                    match self.cb(e) {
                        OP_RREF => {
                            if F!().current_recurse != RECURSE_UNSET {
                                let number = get2(self.code, e + 1);
                                condition = number == RREF_ANY || number == F!().current_recurse;
                            }
                        }
                        OP_DNRREF => {
                            if F!().current_recurse != RECURSE_UNSET {
                                let mut count = get2(self.code, e + 1 + IMM2_SIZE);
                                let mut slot = self.name_table
                                    + get2(self.code, e + 1) as usize * self.name_entry_size;
                                while count > 0 {
                                    count -= 1;
                                    let number = get2(self.code, slot);
                                    condition = number == F!().current_recurse;
                                    if condition {
                                        break;
                                    }
                                    slot += self.name_entry_size;
                                }
                            }
                        }
                        OP_CREF => {
                            let offset = ((get2(self.code, e + 1) as usize) << 1) - 2;
                            condition = offset < F!().offset_top && OV!(offset) != UNSET;
                        }
                        OP_DNCREF => {
                            let mut count = get2(self.code, e + 1 + IMM2_SIZE);
                            let mut slot = self.name_table
                                + get2(self.code, e + 1) as usize * self.name_entry_size;
                            while count > 0 {
                                count -= 1;
                                let offset = ((get2(self.code, slot) as usize) << 1) - 2;
                                condition = offset < F!().offset_top && OV!(offset) != UNSET;
                                if condition {
                                    break;
                                }
                                slot += self.name_entry_size;
                            }
                        }
                        OP_FALSE | OP_FAIL => {}
                        OP_TRUE => condition = true,
                        a => {
                            // An assertion condition.
                            F!().temp_32[0] = (a == OP_ASSERT || a == OP_ASSERTBACK) as u32; // Lpositive
                            F!().temp_sptr[0] = e; // Lstart_branch
                            rmatch!(e + op_len(self.code[e]), RM_COND_ASSERT, GF_CONDASSERT | a);
                        }
                    }
                    return self.cond_after(condition, false);
                }

                OP_REVERSE => {
                    let mut number = get2(self.code, ecode + 1);
                    if utf {
                        while number > 0 {
                            number -= 1;
                            if F!().eptr <= self.check_subject {
                                rreturn!(MATCH_NOMATCH);
                            }
                            let e = backchar(self.subj, F!().eptr - 1);
                            F!().eptr = e;
                        }
                    } else {
                        if number as usize > F!().eptr {
                            rreturn!(MATCH_NOMATCH);
                        }
                        F!().eptr -= number as usize;
                    }
                    if F!().eptr < self.start_used_ptr {
                        self.start_used_ptr = F!().eptr;
                    }
                    F!().ecode += 1 + IMM2_SIZE;
                }

                OP_VREVERSE => {
                    let lmin = get2(self.code, ecode + 1);
                    let mut lmax = get2(self.code, ecode + 1 + IMM2_SIZE);
                    F!().temp_sptr[0] = F!().eptr;
                    if utf {
                        let mut i = 0;
                        while i < lmax {
                            if F!().eptr == 0 {
                                if i < lmin {
                                    rreturn!(MATCH_NOMATCH);
                                }
                                lmax = i;
                                break;
                            }
                            let e = backchar(self.subj, F!().eptr - 1);
                            F!().eptr = e;
                            i += 1;
                        }
                    } else {
                        let diff = F!().eptr;
                        let available = if diff > 65535 { 65535 } else { diff as u32 };
                        if lmin > available {
                            rreturn!(MATCH_NOMATCH);
                        }
                        if lmax > available {
                            lmax = available;
                        }
                        F!().eptr -= lmax as usize;
                    }
                    F!().temp_32[0] = lmin;
                    F!().temp_32[1] = lmax;
                    rmatch!(ecode + 1 + 2 * IMM2_SIZE, RM_VREVERSE);
                }

                OP_ALT => {
                    self.branch_end = ecode;
                    let mut e = ecode;
                    loop {
                        e += get(self.code, e + 1);
                        if self.cb(e) != OP_ALT {
                            break;
                        }
                    }
                    F!().ecode = e;
                }

                OP_KET | OP_KETRMIN | OP_KETRMAX | OP_KETRPOS => {
                    let bracode = ecode - get(self.code, ecode + 1);
                    if self.branch_end == UNSET {
                        self.branch_end = ecode;
                    }
                    let mut branch_start = bracode;
                    while branch_start + get(self.code, branch_start + 1) != self.branch_end {
                        branch_start += get(self.code, branch_start + 1);
                    }
                    self.branch_end = UNSET;

                    let bop = self.cb(bracode);
                    let mut p: usize = UNSET;
                    let mut n: usize = UNSET;
                    if bop != OP_BRA && bop != OP_COND {
                        n = F!().last_group_offset;
                        p = n - 1;
                        F!().last_group_offset = self.frames[p].last_group_offset;
                        if gf_idmask(self.frames[n].group_frame_type) == GF_CONDASSERT {
                            let ot = F!().offset_top;
                            if ovs > 0 {
                                self.ovec.copy_within(f * ovs..f * ovs + ot, p * ovs);
                            }
                            self.frames[p].offset_top = ot;
                            self.frames[p].mark = F!().mark;
                            F!().back_frame = f - p;
                            rreturn!(MATCH_MATCH);
                        }
                    }

                    match bop {
                        OP_BRA => {
                            if !(F!().current_recurse != 0
                                || self.cb(ecode + 1 + LINK_SIZE) != OP_END)
                            {
                                let offset = F!().last_group_offset;
                                if offset == UNSET {
                                    return Act::Ret(PCRE2_ERROR_INTERNAL);
                                }
                                let n2 = offset;
                                let p2 = n2 - 1;
                                F!().last_group_offset = self.frames[p2].last_group_offset;
                                let ot = F!().offset_top;
                                if ovs > 0 {
                                    self.ovec.copy_within(p2 * ovs..p2 * ovs + ot, f * ovs);
                                }
                                F!().offset_top = self.frames[p2].offset_top;
                                F!().capture_last = self.frames[p2].capture_last;
                                F!().current_recurse = self.frames[p2].current_recurse;
                                F!().ecode = self.frames[p2].ecode + 1 + LINK_SIZE;
                                continue;
                            }
                        }
                        OP_COND | OP_SCOND => {}
                        OP_ASSERTBACK_NA | OP_ASSERT_NA => {
                            if bop == OP_ASSERTBACK_NA
                                && self.cb(branch_start + 1 + LINK_SIZE) == OP_VREVERSE
                                && F!().eptr != self.frames[p].eptr
                            {
                                rreturn!(MATCH_NOMATCH);
                            }
                            if F!().eptr > self.last_used_ptr {
                                self.last_used_ptr = F!().eptr;
                            }
                            F!().eptr = self.frames[p].eptr;
                        }
                        OP_ASSERTBACK | OP_ASSERT | OP_ONCE => {
                            if bop == OP_ASSERTBACK
                                && self.cb(branch_start + 1 + LINK_SIZE) == OP_VREVERSE
                                && F!().eptr != self.frames[p].eptr
                            {
                                rreturn!(MATCH_NOMATCH);
                            }
                            if bop != OP_ONCE {
                                if F!().eptr > self.last_used_ptr {
                                    self.last_used_ptr = F!().eptr;
                                }
                                F!().eptr = self.frames[p].eptr;
                            }
                            F!().back_frame = f - p;
                            loop {
                                let pe = self.frames[p].ecode;
                                let y = get(self.code, pe + 1);
                                if self.cb(pe + y) != OP_ALT {
                                    break;
                                }
                                self.frames[p].ecode += y;
                            }
                        }
                        OP_ASSERTBACK_NOT | OP_ASSERT_NOT => {
                            if bop == OP_ASSERTBACK_NOT
                                && self.cb(branch_start + 1 + LINK_SIZE) == OP_VREVERSE
                                && F!().eptr != self.frames[p].eptr
                            {
                                rreturn!(MATCH_NOMATCH);
                            }
                            rreturn!(MATCH_MATCH);
                        }
                        OP_SCRIPT_RUN => {
                            if !script_run(self.subj, self.frames[p].eptr, F!().eptr, utf) {
                                rreturn!(MATCH_NOMATCH);
                            }
                        }
                        OP_CBRA | OP_CBRAPOS | OP_SCBRA | OP_SCBRAPOS => {
                            let number = get2(self.code, bracode + 1 + LINK_SIZE);
                            if F!().current_recurse == number {
                                let p2 = n - 1;
                                let ot = F!().offset_top;
                                if ovs > 0 {
                                    self.ovec.copy_within(p2 * ovs..p2 * ovs + ot, f * ovs);
                                }
                                F!().offset_top = self.frames[p2].offset_top;
                                F!().capture_last = self.frames[p2].capture_last;
                                F!().current_recurse = self.frames[p2].current_recurse;
                                F!().ecode = self.frames[p2].ecode + 1 + LINK_SIZE;
                                continue;
                            }
                            let offset = ((number as usize) << 1) - 2;
                            F!().capture_last = number;
                            OV!(offset) = self.frames[p].eptr;
                            OV!(offset + 1) = F!().eptr;
                            if offset >= F!().offset_top {
                                F!().offset_top = offset + 2;
                            }
                        }
                        _ => {}
                    }

                    if op == OP_KETRPOS {
                        // Copy eptr..ovector into P.
                        let cur = F!();
                        let pf = &mut self.frames[p];
                        pf.eptr = cur.eptr;
                        pf.start_match = cur.start_match;
                        pf.mark = cur.mark;
                        pf.recurse_last_used = cur.recurse_last_used;
                        pf.current_recurse = cur.current_recurse;
                        pf.capture_last = cur.capture_last;
                        pf.last_group_offset = cur.last_group_offset;
                        pf.offset_top = cur.offset_top;
                        if ovs > 0 {
                            self.ovec.copy_within(f * ovs..(f + 1) * ovs, p * ovs);
                        }
                        rreturn!(MATCH_KETRPOS);
                    }

                    if op != OP_KET && (p == UNSET || F!().eptr != self.frames[p].eptr) {
                        if op == OP_KETRMIN {
                            rmatch!(ecode + 1 + LINK_SIZE, RM_KETRMIN);
                        }
                        rmatch!(bracode, RM_KETRMAX);
                    }
                    F!().ecode += 1 + LINK_SIZE;
                }

                OP_CIRC => {
                    if F!().eptr != 0 || (self.moptions & PCRE2_NOTBOL) != 0 {
                        rreturn!(MATCH_NOMATCH);
                    }
                    F!().ecode += 1;
                }
                OP_SOD => {
                    if F!().eptr != 0 {
                        rreturn!(MATCH_NOMATCH);
                    }
                    F!().ecode += 1;
                }
                OP_DOLL | OP_EOD | OP_EODN => {
                    let mut assert_nl_or_eos = op == OP_EODN;
                    if op == OP_DOLL {
                        if (self.moptions & PCRE2_NOTEOL) != 0 {
                            rreturn!(MATCH_NOMATCH);
                        }
                        if (self.poptions & PCRE2_DOLLAR_ENDONLY) == 0 {
                            assert_nl_or_eos = true;
                        }
                    }
                    if assert_nl_or_eos {
                        let e = F!().eptr;
                        if e < self.end_subject {
                            let nl = self.is_newline(e);
                            if !nl || e != self.end_subject - self.nllen as usize {
                                rreturn!(MATCH_NOMATCH);
                            }
                        }
                    } else if F!().eptr < self.true_end_subject {
                        rreturn!(MATCH_NOMATCH);
                    }
                    F!().ecode += 1;
                }
                OP_CIRCM => {
                    let e = F!().eptr;
                    if (self.moptions & PCRE2_NOTBOL) != 0 && e == 0 {
                        rreturn!(MATCH_NOMATCH);
                    }
                    if e != 0
                        && ((e == self.end_subject && (self.poptions & PCRE2_ALT_CIRCUMFLEX) == 0)
                            || !self.was_newline(e))
                    {
                        rreturn!(MATCH_NOMATCH);
                    }
                    F!().ecode += 1;
                }
                OP_DOLLM => {
                    let e = F!().eptr;
                    if e < self.end_subject {
                        if !self.is_newline(e) {
                            rreturn!(MATCH_NOMATCH);
                        }
                    } else if (self.moptions & PCRE2_NOTEOL) != 0 {
                        rreturn!(MATCH_NOMATCH);
                    }
                    F!().ecode += 1;
                }
                OP_SOM => {
                    if F!().eptr != self.start_offset {
                        rreturn!(MATCH_NOMATCH);
                    }
                    F!().ecode += 1;
                }
                OP_SET_SOM => {
                    F!().start_match = F!().eptr;
                    F!().ecode += 1;
                }

                OP_NOT_WORD_BOUNDARY
                | OP_WORD_BOUNDARY
                | OP_NOT_UCP_WORD_BOUNDARY
                | OP_UCP_WORD_BOUNDARY => {
                    let e = F!().eptr;
                    let ucpb = op == OP_UCP_WORD_BOUNDARY || op == OP_NOT_UCP_WORD_BOUNDARY;
                    let isword = |m: &Self, c: u32| -> bool {
                        if ucpb {
                            let ct = ucd_chartype(c);
                            let cat = tables::UCP_GENTYPE[ct as usize];
                            cat == ucp_L || cat == ucp_N || ct == ucp_Mn || ct == ucp_Pc
                        } else {
                            c < 256 && (m.ctypes(c) & ctype_word) != 0
                        }
                    };
                    let prev_is_word = if e == self.check_subject {
                        false
                    } else {
                        let mut lastptr = e - 1;
                        let fc = if utf {
                            lastptr = backchar(self.subj, lastptr);
                            getchar(self.subj, lastptr).0
                        } else {
                            self.sb(lastptr)
                        };
                        if lastptr < self.start_used_ptr {
                            self.start_used_ptr = lastptr;
                        }
                        isword(self, fc)
                    };
                    let cur_is_word = if e >= self.end_subject {
                        false
                    } else {
                        let mut nextptr = e + 1;
                        let fc = if utf {
                            nextptr = forwardchar_test(self.subj, nextptr, self.end_subject);
                            getchar(self.subj, e).0
                        } else {
                            self.sb(e)
                        };
                        if nextptr > self.last_used_ptr {
                            self.last_used_ptr = nextptr;
                        }
                        isword(self, fc)
                    };
                    F!().ecode += 1;
                    let fail = if op == OP_WORD_BOUNDARY || op == OP_UCP_WORD_BOUNDARY {
                        cur_is_word == prev_is_word
                    } else {
                        cur_is_word != prev_is_word
                    };
                    if fail {
                        rreturn!(MATCH_NOMATCH);
                    }
                }

                OP_MARK => {
                    F!().mark = ecode + 2;
                    self.nomatch_mark = ecode + 2;
                    rmatch!(
                        ecode + op_len(self.code[ecode]) + self.code[ecode + 1] as usize,
                        RM_MARK
                    );
                }
                OP_FAIL => rreturn!(MATCH_NOMATCH),
                OP_COMMIT => rmatch!(ecode + op_len(self.code[ecode]), RM_COMMIT),
                OP_COMMIT_ARG => {
                    F!().mark = ecode + 2;
                    self.nomatch_mark = ecode + 2;
                    rmatch!(
                        ecode + op_len(self.code[ecode]) + self.code[ecode + 1] as usize,
                        RM_COMMIT_ARG
                    );
                }
                OP_PRUNE => rmatch!(ecode + op_len(self.code[ecode]), RM_PRUNE),
                OP_PRUNE_ARG => {
                    F!().mark = ecode + 2;
                    self.nomatch_mark = ecode + 2;
                    rmatch!(
                        ecode + op_len(self.code[ecode]) + self.code[ecode + 1] as usize,
                        RM_PRUNE_ARG
                    );
                }
                OP_SKIP => rmatch!(ecode + op_len(self.code[ecode]), RM_SKIP),
                OP_SKIP_ARG => {
                    self.skip_arg_count += 1;
                    if self.skip_arg_count <= self.ignore_skip_arg {
                        F!().ecode += op_len(self.code[ecode]) + self.code[ecode + 1] as usize;
                        continue;
                    }
                    rmatch!(
                        ecode + op_len(self.code[ecode]) + self.code[ecode + 1] as usize,
                        RM_SKIP_ARG
                    );
                }
                OP_THEN => rmatch!(ecode + op_len(self.code[ecode]), RM_THEN),
                OP_THEN_ARG => {
                    F!().mark = ecode + 2;
                    self.nomatch_mark = ecode + 2;
                    rmatch!(
                        ecode + op_len(self.code[ecode]) + self.code[ecode + 1] as usize,
                        RM_THEN_ARG
                    );
                }

                _ => return Act::Ret(PCRE2_ERROR_INTERNAL),
            }
        }
    }

    /// Tail of OP_COND/OP_SCOND once the condition is known.
    fn cond_after(&mut self, condition: bool, is_assert: bool) -> Act {
        let f = self.f;
        if condition && is_assert {
            loop {
                let e = self.frames[f].ecode;
                self.frames[f].ecode += get(self.code, e + 1);
                if self.cb(self.frames[f].ecode) != OP_ALT {
                    break;
                }
            }
        }
        let e = self.frames[f].ecode;
        if condition {
            self.frames[f].ecode += op_len(self.code[e]);
        } else {
            self.frames[f].ecode += self.frames[f].length;
        }
        if self.frames[f].op as u32 == OP_SCOND {
            let ec = self.frames[f].ecode;
            return Act::Rmatch(ec, RM_SCOND, GF_NOCAPTURE | OP_SCOND);
        }
        Act::Next
    }

    /// Read the OP_CRxxx repeat after a class into Lmin/Lmax; returns the
    /// repeat type.
    fn read_cr_repeat(&mut self) -> u32 {
        let f = self.f;
        let e = self.frames[f].ecode;
        let c = self.cb(e);
        match c {
            OP_CRSTAR | OP_CRMINSTAR | OP_CRPLUS | OP_CRMINPLUS | OP_CRQUERY | OP_CRMINQUERY
            | OP_CRPOSSTAR | OP_CRPOSPLUS | OP_CRPOSQUERY => {
                let fc = (c - OP_CRSTAR) as usize;
                self.frames[f].ecode += 1;
                self.frames[f].temp_32[0] = REP_MIN[fc];
                self.frames[f].temp_32[1] = REP_MAX[fc];
                REP_TYP[fc]
            }
            OP_CRRANGE | OP_CRMINRANGE | OP_CRPOSRANGE => {
                self.frames[f].temp_32[0] = get2(self.code, e + 1);
                let mut mx = get2(self.code, e + 1 + IMM2_SIZE);
                if mx == 0 {
                    mx = u32::MAX;
                }
                self.frames[f].temp_32[1] = mx;
                self.frames[f].ecode += 1 + 2 * IMM2_SIZE;
                REP_TYP[(c - OP_CRSTAR) as usize]
            }
            _ => {
                self.frames[f].temp_32[0] = 1;
                self.frames[f].temp_32[1] = 1;
                REPTYPE_MAX
            }
        }
    }

    #[inline]
    fn class_ok(&self, op: u32, map: usize, fc: u32) -> bool {
        if fc > 255 {
            op != OP_CLASS
        } else {
            (self.code[map + (fc / 8) as usize] & (1u8 << (fc & 7))) != 0
        }
    }

    /// One step of a UTF multi-byte character repeat (REPEATCHAR with
    /// Flength > 1): returns the new eptr or None.
    fn char_utf_step(&self) -> Option<usize> {
        let fr = &self.frames[self.f];
        let e = fr.eptr;
        let len = fr.length;
        let cp = fr.temp_sptr[1];
        if e + len <= self.end_subject && self.subj[e..e + len] == self.code[cp..cp + len] {
            return Some(e + len);
        }
        let ol = fr.temp_size;
        if ol > 0 && e + ol <= self.end_subject && self.subj[e..e + ol] == fr.occu[..ol] {
            return Some(e + ol);
        }
        None
    }

    /// Code after an RMATCH returns (the L_RMxx labels).
    fn resume(&mut self, id: u8, mut rrc: i32) -> Act {
        let f = self.f;
        let ovs = self.ovs;
        let utf = self.utf;
        macro_rules! F {
            () => {
                self.frames[f]
            };
        }
        macro_rules! rmatch {
            ($e:expr, $id:expr) => {
                return Act::Rmatch($e, $id, 0)
            };
            ($e:expr, $id:expr, $g:expr) => {
                return Act::Rmatch($e, $id, $g)
            };
        }
        macro_rules! rreturn {
            ($v:expr) => {
                return Act::Rreturn($v)
            };
        }
        macro_rules! nomatch_unless_ok {
            () => {
                if rrc != MATCH_NOMATCH {
                    rreturn!(rrc);
                }
            };
        }
        // For minimizing repeats: count, then check for end of subject.
        macro_rules! min_step {
            () => {{
                nomatch_unless_ok!();
                let lmin = F!().temp_32[0];
                F!().temp_32[0] = lmin.wrapping_add(1);
                if lmin >= F!().temp_32[1] {
                    rreturn!(MATCH_NOMATCH);
                }
                if F!().eptr >= self.end_subject {
                    rreturn!(MATCH_NOMATCH);
                }
            }};
        }

        match id {
            RM_BRA_ALT => {
                nomatch_unless_ok!();
                F!().ecode = F!().temp_sptr[0];
                let ec = F!().ecode;
                let nb = ec + get(self.code, ec + 1);
                F!().temp_sptr[0] = nb;
                if self.cb(nb) != OP_ALT {
                    F!().ecode += op_len(self.code[ec]);
                    return Act::Next;
                }
                rmatch!(ec + op_len(self.code[ec]), RM_BRA_ALT);
            }
            RM_GROUPLOOP => {
                let ec = F!().ecode;
                if rrc == MATCH_THEN {
                    let next_ecode = ec + get(self.code, ec + 1);
                    if self.verb_ecode_ptr < next_ecode
                        && (self.cb(ec) == OP_ALT || self.cb(next_ecode) == OP_ALT)
                    {
                        rrc = MATCH_NOMATCH;
                    }
                }
                nomatch_unless_ok!();
                F!().ecode += get(self.code, ec + 1);
                let ec = F!().ecode;
                if self.cb(ec) != OP_ALT {
                    rreturn!(MATCH_NOMATCH);
                }
                let ft = F!().temp_32[0];
                rmatch!(ec + op_len(self.code[ec]), RM_GROUPLOOP, ft);
            }
            RM_ASSERT => {
                if rrc == MATCH_ACCEPT {
                    let aaf = self.assert_accept_frame;
                    let ot = self.frames[aaf].offset_top;
                    if ovs > 0 {
                        self.ovec.copy_within(aaf * ovs..aaf * ovs + ot, f * ovs);
                    }
                    F!().offset_top = ot;
                    F!().mark = self.frames[aaf].mark;
                    let mut e = F!().ecode;
                    loop {
                        e += get(self.code, e + 1);
                        if self.cb(e) != OP_ALT {
                            break;
                        }
                    }
                    F!().ecode = e + 1 + LINK_SIZE;
                    return Act::Next;
                }
                if rrc != MATCH_NOMATCH && rrc != MATCH_THEN {
                    rreturn!(rrc);
                }
                let ec = F!().ecode;
                F!().ecode += get(self.code, ec + 1);
                let ec = F!().ecode;
                if self.cb(ec) != OP_ALT {
                    rreturn!(MATCH_NOMATCH);
                }
                let ft = F!().temp_32[0];
                rmatch!(ec + op_len(self.code[ec]), RM_ASSERT, ft);
            }
            RM_ASSERT_NOT => {
                match rrc {
                    MATCH_ACCEPT | MATCH_MATCH => rreturn!(MATCH_NOMATCH),
                    MATCH_NOMATCH | MATCH_THEN => {
                        let ec = F!().ecode;
                        F!().ecode += get(self.code, ec + 1);
                        let ec = F!().ecode;
                        if self.cb(ec) == OP_ALT {
                            let ft = F!().temp_32[0];
                            rmatch!(ec + op_len(self.code[ec]), RM_ASSERT_NOT, ft);
                        }
                    }
                    MATCH_COMMIT | MATCH_SKIP | MATCH_PRUNE => {
                        let mut e = F!().ecode;
                        loop {
                            e += get(self.code, e + 1);
                            if self.cb(e) != OP_ALT {
                                break;
                            }
                        }
                        F!().ecode = e;
                    }
                    _ => rreturn!(rrc),
                }
                // ASSERT_NOT_FAILED
                F!().ecode += 1 + LINK_SIZE;
                Act::Next
            }
            RM_COND_ASSERT => {
                let positive = F!().temp_32[0] != 0;
                let condition;
                match rrc {
                    MATCH_ACCEPT | MATCH_MATCH => {
                        if rrc == MATCH_ACCEPT {
                            let aaf = self.assert_accept_frame;
                            let ot = self.frames[aaf].offset_top;
                            if ovs > 0 {
                                self.ovec.copy_within(aaf * ovs..aaf * ovs + ot, f * ovs);
                            }
                            F!().offset_top = ot;
                        }
                        condition = positive;
                    }
                    MATCH_NOMATCH | MATCH_THEN => {
                        let sb = F!().temp_sptr[0];
                        let sb = sb + get(self.code, sb + 1);
                        F!().temp_sptr[0] = sb;
                        if self.cb(sb) == OP_ALT {
                            let a = self.cb(F!().ecode);
                            rmatch!(
                                sb + op_len(self.code[sb]),
                                RM_COND_ASSERT,
                                GF_CONDASSERT | a
                            );
                        }
                        condition = !positive;
                    }
                    MATCH_COMMIT | MATCH_SKIP | MATCH_PRUNE => condition = !positive,
                    _ => rreturn!(rrc),
                }
                self.cond_after(condition, true)
            }
            RM_KETRMIN => {
                nomatch_unless_ok!();
                let ec = F!().ecode;
                F!().ecode -= get(self.code, ec + 1);
                Act::Next
            }
            RM_KETRMAX => {
                nomatch_unless_ok!();
                F!().ecode += 1 + LINK_SIZE;
                Act::Next
            }
            RM_POS_GROUP => {
                if rrc == MATCH_KETRPOS {
                    F!().temp_32[1] = 1;
                    if F!().eptr == F!().temp_sptr[0] {
                        let mut e = F!().ecode;
                        loop {
                            e += get(self.code, e + 1);
                            if self.cb(e) != OP_ALT {
                                break;
                            }
                        }
                        F!().ecode = e;
                    } else {
                        F!().ecode = F!().temp_sptr[1];
                        F!().temp_sptr[0] = F!().eptr;
                        let ec = F!().ecode;
                        let gft = F!().temp_32[0];
                        rmatch!(ec + op_len(self.code[ec]), RM_POS_GROUP, gft);
                    }
                } else {
                    let ec = F!().ecode;
                    if rrc == MATCH_THEN {
                        let next_ecode = ec + get(self.code, ec + 1);
                        if self.verb_ecode_ptr < next_ecode
                            && (self.cb(ec) == OP_ALT || self.cb(next_ecode) == OP_ALT)
                        {
                            rrc = MATCH_NOMATCH;
                        }
                    }
                    nomatch_unless_ok!();
                    F!().ecode += get(self.code, ec + 1);
                    let ec = F!().ecode;
                    if self.cb(ec) == OP_ALT {
                        F!().temp_sptr[0] = F!().eptr;
                        let gft = F!().temp_32[0];
                        rmatch!(ec + op_len(self.code[ec]), RM_POS_GROUP, gft);
                    }
                }
                if F!().temp_32[1] != 0 || F!().temp_32[2] != 0 {
                    F!().ecode += 1 + LINK_SIZE;
                    return Act::Next;
                }
                rreturn!(MATCH_NOMATCH);
            }
            RM_BRAZERO => {
                nomatch_unless_ok!();
                let mut ne = F!().temp_sptr[0];
                loop {
                    ne += get(self.code, ne + 1);
                    if self.cb(ne) != OP_ALT {
                        break;
                    }
                }
                F!().ecode = ne + 1 + LINK_SIZE;
                Act::Next
            }
            RM_BRAMINZERO => {
                nomatch_unless_ok!();
                F!().ecode += 1;
                Act::Next
            }
            RM_RECURSE => {
                let sb = F!().temp_sptr[0];
                let next_ecode = sb + get(self.code, sb + 1);
                let ft = F!().temp_32[0];
                if (MATCH_BACKTRACK_MIN..=MATCH_BACKTRACK_MAX).contains(&rrc)
                    && self.verb_current_recurse == (ft ^ GF_RECURSE)
                {
                    if rrc == MATCH_THEN
                        && self.verb_ecode_ptr < next_ecode
                        && (self.cb(sb) == OP_ALT || self.cb(next_ecode) == OP_ALT)
                    {
                        rrc = MATCH_NOMATCH;
                    } else {
                        rreturn!(MATCH_NOMATCH);
                    }
                }
                nomatch_unless_ok!();
                F!().temp_sptr[0] = next_ecode;
                if self.cb(next_ecode) != OP_ALT {
                    rreturn!(MATCH_NOMATCH);
                }
                rmatch!(next_ecode + op_len(self.code[next_ecode]), RM_RECURSE, ft);
            }
            RM_MARK => {
                let ec = F!().ecode;
                if rrc == MATCH_SKIP_ARG && self.code_strcmp(ec + 2, self.verb_skip_name) {
                    self.verb_skip_ptr = F!().eptr;
                    rreturn!(MATCH_SKIP);
                }
                rreturn!(rrc);
            }
            RM_COMMIT | RM_COMMIT_ARG => {
                nomatch_unless_ok!();
                self.verb_current_recurse = F!().current_recurse;
                rreturn!(MATCH_COMMIT);
            }
            RM_PRUNE | RM_PRUNE_ARG => {
                nomatch_unless_ok!();
                self.verb_current_recurse = F!().current_recurse;
                rreturn!(MATCH_PRUNE);
            }
            RM_SKIP => {
                nomatch_unless_ok!();
                self.verb_skip_ptr = F!().eptr;
                self.verb_current_recurse = F!().current_recurse;
                rreturn!(MATCH_SKIP);
            }
            RM_SKIP_ARG => {
                nomatch_unless_ok!();
                self.verb_skip_name = F!().ecode + 2;
                self.verb_current_recurse = F!().current_recurse;
                rreturn!(MATCH_SKIP_ARG);
            }
            RM_THEN | RM_THEN_ARG => {
                nomatch_unless_ok!();
                self.verb_ecode_ptr = F!().ecode;
                self.verb_current_recurse = F!().current_recurse;
                rreturn!(MATCH_THEN);
            }
            RM_SCOND => rreturn!(rrc),

            RM_VREVERSE => {
                nomatch_unless_ok!();
                let lmax = F!().temp_32[1];
                F!().temp_32[1] = lmax.wrapping_sub(1);
                if lmax <= F!().temp_32[0] {
                    rreturn!(MATCH_NOMATCH);
                }
                F!().eptr += 1;
                if utf {
                    let e = forwardchar_test(self.subj, F!().eptr, self.end_subject);
                    F!().eptr = e;
                }
                let ec = F!().ecode;
                rmatch!(ec + 1 + 2 * IMM2_SIZE, RM_VREVERSE);
            }

            // ---- Back references ----
            RM_REF_MIN => {
                nomatch_unless_ok!();
                let lmin = F!().temp_32[0];
                F!().temp_32[0] = lmin.wrapping_add(1);
                if lmin >= F!().temp_32[1] {
                    rreturn!(MATCH_NOMATCH);
                }
                let (r, slength) = self.match_ref(F!().temp_size, F!().temp_32[2] != 0);
                if r != 0 {
                    if r > 0 {
                        F!().eptr = self.end_subject;
                    }
                    rreturn!(MATCH_NOMATCH);
                }
                F!().eptr += slength;
                rmatch!(F!().ecode, RM_REF_MIN);
            }
            RM_REF_MAX_SAME => {
                nomatch_unless_ok!();
                let len = F!().length;
                let e = F!().eptr;
                // Feptr -= Flength; while (Feptr >= Lstart)
                if e < len || e - len < F!().temp_sptr[0] {
                    rreturn!(MATCH_NOMATCH);
                }
                F!().eptr = e - len;
                rmatch!(F!().ecode, RM_REF_MAX_SAME);
            }
            RM_REF_MAX_DIFF => {
                nomatch_unless_ok!();
                if F!().eptr == F!().temp_sptr[0] {
                    rreturn!(MATCH_NOMATCH);
                }
                F!().eptr = F!().temp_sptr[0];
                F!().temp_32[1] -= 1;
                let caseless = F!().temp_32[2] != 0;
                let mut i = F!().temp_32[0];
                while i < F!().temp_32[1] {
                    let (_, slength) = self.match_ref(F!().temp_size, caseless);
                    F!().eptr += slength;
                    i += 1;
                }
                rmatch!(F!().ecode, RM_REF_MAX_DIFF);
            }

            // ---- Classes ----
            RM_CLASS_MIN => {
                min_step!();
                let op = F!().op as u32;
                let map = F!().temp_sptr[1];
                let mut e = F!().eptr;
                let fc = self.getc_inc(&mut e);
                F!().eptr = e;
                if !self.class_ok(op, map, fc) {
                    rreturn!(MATCH_NOMATCH);
                }
                rmatch!(F!().ecode, RM_CLASS_MIN);
            }
            RM_CLASS_MAX => {
                // while (Feptr >= Lstart_eptr) { RMATCH; Feptr--; }
                nomatch_unless_ok!();
                if F!().eptr == F!().temp_sptr[0] {
                    rreturn!(MATCH_NOMATCH);
                }
                F!().eptr -= 1;
                rmatch!(F!().ecode, RM_CLASS_MAX);
            }
            RM_CLASS_MAX_UTF => {
                nomatch_unless_ok!();
                let e = F!().eptr;
                if e <= F!().temp_sptr[0] {
                    rreturn!(MATCH_NOMATCH);
                }
                F!().eptr = backchar(self.subj, e - 1);
                rmatch!(F!().ecode, RM_CLASS_MAX_UTF);
            }
            RM_XCLASS_MIN => {
                min_step!();
                let data = F!().temp_sptr[1];
                let mut e = F!().eptr;
                let fc = self.getc_inc(&mut e);
                F!().eptr = e;
                if !xclass(fc, self.code, data, utf) {
                    rreturn!(MATCH_NOMATCH);
                }
                rmatch!(F!().ecode, RM_XCLASS_MIN);
            }
            RM_XCLASS_MAX => {
                nomatch_unless_ok!();
                let e = F!().eptr;
                if e <= F!().temp_sptr[0] {
                    rreturn!(MATCH_NOMATCH);
                }
                let mut ne = e - 1;
                if utf {
                    ne = backchar(self.subj, ne);
                }
                F!().eptr = ne;
                rmatch!(F!().ecode, RM_XCLASS_MAX);
            }

            // ---- Character repeats ----
            RM_CHAR_UTF_MIN => {
                nomatch_unless_ok!();
                let lmin = F!().temp_32[0];
                F!().temp_32[0] = lmin.wrapping_add(1);
                if lmin >= F!().temp_32[1] {
                    rreturn!(MATCH_NOMATCH);
                }
                match self.char_utf_step() {
                    Some(e) => F!().eptr = e,
                    None => rreturn!(MATCH_NOMATCH),
                }
                rmatch!(F!().ecode, RM_CHAR_UTF_MIN);
            }
            RM_CHAR_UTF_MAX => {
                nomatch_unless_ok!();
                let e = backchar(self.subj, F!().eptr - 1);
                F!().eptr = e;
                if e <= F!().temp_sptr[0] {
                    return Act::Next;
                }
                rmatch!(F!().ecode, RM_CHAR_UTF_MAX);
            }
            RM_CHARI_MIN => {
                min_step!();
                let cc = self.sb(F!().eptr);
                if F!().temp_32[2] != cc && F!().temp_32[3] != cc {
                    rreturn!(MATCH_NOMATCH);
                }
                F!().eptr += 1;
                rmatch!(F!().ecode, RM_CHARI_MIN);
            }
            RM_CHARI_MAX => {
                F!().eptr -= 1;
                nomatch_unless_ok!();
                if F!().eptr == F!().temp_sptr[0] {
                    return Act::Next;
                }
                rmatch!(F!().ecode, RM_CHARI_MAX);
            }
            RM_CHAR_MIN => {
                min_step!();
                let e = F!().eptr;
                F!().eptr += 1;
                if F!().temp_32[2] != self.sb(e) {
                    rreturn!(MATCH_NOMATCH);
                }
                rmatch!(F!().ecode, RM_CHAR_MIN);
            }
            RM_CHAR_MAX => {
                F!().eptr -= 1;
                nomatch_unless_ok!();
                if F!().eptr <= F!().temp_sptr[0] {
                    return Act::Next;
                }
                rmatch!(F!().ecode, RM_CHAR_MAX);
            }
            RM_NOTI_MIN | RM_NOT_MIN => {
                min_step!();
                let mut e = F!().eptr;
                let d = self.getc_inc(&mut e);
                if F!().temp_32[2] == d || F!().temp_32[3] == d {
                    if utf || id == RM_NOT_MIN {
                        F!().eptr = e;
                    }
                    rreturn!(MATCH_NOMATCH);
                }
                F!().eptr = e;
                rmatch!(F!().ecode, id);
            }
            RM_NOTI_MAX | RM_NOT_MAX => {
                nomatch_unless_ok!();
                F!().eptr -= 1;
                if F!().eptr == F!().temp_sptr[0] {
                    return Act::Next;
                }
                rmatch!(F!().ecode, id);
            }
            RM_NOTI_MAX_UTF | RM_NOT_MAX_UTF => {
                nomatch_unless_ok!();
                let e = backchar(self.subj, F!().eptr - 1);
                F!().eptr = e;
                if e <= F!().temp_sptr[0] {
                    return Act::Next;
                }
                rmatch!(F!().ecode, id);
            }

            // ---- Type repeats ----
            RM_TYPE_MIN => {
                nomatch_unless_ok!();
                let lmin = F!().temp_32[0];
                F!().temp_32[0] = lmin.wrapping_add(1);
                if lmin >= F!().temp_32[1] {
                    rreturn!(MATCH_NOMATCH);
                }
                if F!().eptr >= self.end_subject {
                    rreturn!(MATCH_NOMATCH);
                }
                let lctype = F!().temp_32[2];
                let proptype = F!().proptype;
                let propvalue = F!().temp_32[3];
                if utf && proptype < 0 && lctype == OP_ANYBYTE {
                    // The C code uses GETCHARINC here, so \C advances by a
                    // whole character in a minimizing repeat.
                    let mut e = F!().eptr;
                    self.getc_inc(&mut e);
                    F!().eptr = e;
                    rmatch!(F!().ecode, RM_TYPE_MIN);
                }
                match self.type_one(lctype, proptype, propvalue, F!().eptr, Phase::MinLoop) {
                    Err(e) => return Act::Ret(e),
                    Ok((ok, e)) => {
                        F!().eptr = e;
                        if !ok {
                            rreturn!(MATCH_NOMATCH);
                        }
                    }
                }
                rmatch!(F!().ecode, RM_TYPE_MIN);
            }
            RM_TYPE_MAX_PROP => {
                nomatch_unless_ok!();
                let mut e = F!().eptr - 1;
                if utf {
                    e = backchar(self.subj, e);
                }
                F!().eptr = e;
                if e <= F!().temp_sptr[0] {
                    return Act::Next;
                }
                rmatch!(F!().ecode, RM_TYPE_MAX_PROP);
            }
            RM_TYPE_MAX_EXTUNI => {
                nomatch_unless_ok!();
                let start = F!().temp_sptr[0];
                let mut e = F!().eptr - 1;
                let mut fc;
                if !utf {
                    fc = self.sb(e);
                } else {
                    e = backchar(self.subj, e);
                    fc = getchar(self.subj, e).0;
                }
                let mut rgb = ucd_graphbreak(fc);
                loop {
                    if e <= start {
                        break;
                    }
                    let mut fptr = e - 1;
                    if !utf {
                        fc = self.sb(fptr);
                    } else {
                        fptr = backchar(self.subj, fptr);
                        fc = getchar(self.subj, fptr).0;
                    }
                    let lgb = ucd_graphbreak(fc);
                    if (tables::UCP_GBTABLE[lgb as usize] & (1u32 << rgb)) == 0 {
                        break;
                    }
                    e = fptr;
                    rgb = lgb;
                }
                F!().eptr = e;
                if e <= start {
                    return Act::Next;
                }
                rmatch!(F!().ecode, RM_TYPE_MAX_EXTUNI);
            }
            RM_TYPE_MAX_UTF => {
                nomatch_unless_ok!();
                let mut e = backchar(self.subj, F!().eptr - 1);
                let start = F!().temp_sptr[0];
                if F!().temp_32[2] == OP_ANYNL
                    && e > start
                    && self.subj[e] == b'\n'
                    && self.subj[e - 1] == b'\r'
                {
                    e -= 1;
                }
                F!().eptr = e;
                if e <= start {
                    return Act::Next;
                }
                rmatch!(F!().ecode, RM_TYPE_MAX_UTF);
            }
            RM_TYPE_MAX => {
                nomatch_unless_ok!();
                let mut e = F!().eptr - 1;
                let start = F!().temp_sptr[0];
                if F!().temp_32[2] == OP_ANYNL
                    && e > start
                    && self.subj[e] == b'\n'
                    && self.subj[e - 1] == b'\r'
                {
                    e -= 1;
                }
                F!().eptr = e;
                if e == start {
                    return Act::Next;
                }
                rmatch!(F!().ecode, RM_TYPE_MAX);
            }

            _ => Act::Ret(PCRE2_ERROR_INTERNAL),
        }
    }

    /// strcmp() of two NUL-terminated names in the code.
    fn code_strcmp(&self, a: usize, b: usize) -> bool {
        if b == UNSET {
            return false;
        }
        let mut i = 0;
        loop {
            let ca = self.code[a + i];
            let cb = self.code[b + i];
            if ca != cb {
                return false;
            }
            if ca == 0 {
                return true;
            }
            i += 1;
        }
    }
}

fn mark_name(code: &[u8], p: usize) -> Option<Vec<u8>> {
    if p == UNSET {
        return None;
    }
    // The name is preceded by its length byte.
    let len = code[p - 1] as usize;
    Some(code[p..p + len].to_vec())
}

impl Code {
    /// pcre2_match(): returns the number of captured pairs (>0), 0 if the
    /// ovector was too small, PCRE2_ERROR_NOMATCH or another negative error.
    pub fn match_at(
        &self,
        subject: &[u8],
        start_offset: usize,
        mut options: u32,
        md: &mut MatchData,
        mcontext: &MatchContext,
    ) -> i32 {
        const PUBLIC_MATCH_OPTIONS: u32 = PCRE2_ANCHORED
            | PCRE2_ENDANCHORED
            | PCRE2_NOTBOL
            | PCRE2_NOTEOL
            | PCRE2_NOTEMPTY
            | PCRE2_NOTEMPTY_ATSTART
            | PCRE2_NO_UTF_CHECK
            | PCRE2_PARTIAL_HARD
            | PCRE2_PARTIAL_SOFT
            | PCRE2_NO_JIT
            | PCRE2_COPY_MATCHED_SUBJECT
            | PCRE2_DISABLE_RECURSELOOP_CHECK;
        if (options & !PUBLIC_MATCH_OPTIONS) != 0 {
            return PCRE2_ERROR_BADOPTION;
        }
        let length = subject.len();
        let mut start_match = start_offset;
        let mut req_cu_ptr: isize = start_match as isize - 1;
        let true_end_subject = length;
        let mut end_subject = length;
        if start_offset > length {
            return PCRE2_ERROR_BADOFFSET;
        }
        // (*NOTEMPTY) and (*NOTEMPTY_ATSTART) set in the pattern.
        if (self.flags & PCRE2_NOTEMPTY_SET) != 0 {
            options |= PCRE2_NOTEMPTY;
        }
        if (self.flags & PCRE2_NE_ATST_SET) != 0 {
            options |= PCRE2_NOTEMPTY_ATSTART;
        }
        let utf = (self.overall_options & PCRE2_UTF) != 0;
        let allow_invalid = (self.overall_options & PCRE2_MATCH_INVALID_UTF) != 0;
        let ucp = (self.overall_options & PCRE2_UCP) != 0;
        if (options & (PCRE2_PARTIAL_HARD | PCRE2_PARTIAL_SOFT)) != 0 {
            // Partial matching is not supported by this port.
            return PCRE2_ERROR_BADOPTION;
        }
        if mcontext.offset_limit != PCRE2_UNSET
            && (self.overall_options & PCRE2_USE_OFFSET_LIMIT) == 0
        {
            return PCRE2_ERROR_BADOFFSETLIMIT;
        }
        md.mark = None;
        md.startchar = 0;

        let mut check_subject = 0usize;
        let mut fragment_options: u32 = 0;
        if utf && ((options & PCRE2_NO_UTF_CHECK) == 0 || allow_invalid) {
            let mut skipped_bad_start = false;
            if allow_invalid {
                while start_match < end_subject && not_firstcu(subject[start_match]) {
                    start_match += 1;
                    skipped_bad_start = true;
                }
            } else if start_match < end_subject && not_firstcu(subject[start_match]) {
                if start_offset > 0 {
                    md.rc = PCRE2_ERROR_BADUTFOFFSET;
                    return PCRE2_ERROR_BADUTFOFFSET;
                }
                md.rc = PCRE2_ERROR_UTF8_ERR20;
                return PCRE2_ERROR_UTF8_ERR20;
            }
            check_subject = start_match;
            if !skipped_bad_start {
                let mut i = self.max_lookbehind;
                while i > 0 && check_subject > 0 {
                    check_subject -= 1;
                    while check_subject > 0 && (subject[check_subject] & 0xc0) == 0x80 {
                        check_subject -= 1;
                    }
                    i -= 1;
                }
            }
            loop {
                match valid_utf(&subject[check_subject..]) {
                    Ok(()) => break,
                    Err((rc, off)) => {
                        md.rc = rc;
                        md.startchar = off + check_subject;
                        if !allow_invalid || rc > 0 {
                            return rc;
                        }
                        end_subject = md.startchar;
                        if end_subject < start_match {
                            check_subject = end_subject + 1;
                            while check_subject < start_match && not_firstcu(subject[check_subject])
                            {
                                check_subject += 1;
                            }
                            end_subject = true_end_subject;
                        } else {
                            fragment_options = PCRE2_NOTEOL;
                            break;
                        }
                    }
                }
            }
        }

        let anchored = ((self.overall_options | options) & PCRE2_ANCHORED) != 0;
        let firstline = !anchored && (self.overall_options & PCRE2_FIRSTLINE) != 0;
        let startline = (self.flags & PCRE2_STARTLINE) != 0;
        let bumpalong_limit = if mcontext.offset_limit == PCRE2_UNSET {
            true_end_subject
        } else {
            mcontext.offset_limit
        };

        let ovs = self.top_bracket as usize * 2;
        // pcre2_match() runs the JIT code unless the pattern has (*NO_JIT)
        // or the options include one JIT does not support.
        let jit_depth = mcontext.jit_depth_limit.filter(|_| {
            self.flags & PCRE2_NOJIT == 0
                && options & (PCRE2_ANCHORED | PCRE2_ENDANCHORED | PCRE2_NO_JIT) == 0
        });
        let (mut frames, mut ovec) = BUFFERS.with(|b| b.take());
        frames.reserve(16);
        ovec.reserve(16 * ovs);
        let mut m = Matcher {
            code: &self.block,
            subj: subject,
            tables: self.tables,
            frames,
            ovec,
            ovs,
            f: 0,
            top_bracket: self.top_bracket,
            match_limit: mcontext.match_limit.min(self.limit_match),
            match_limit_depth: jit_depth
                .unwrap_or_else(|| mcontext.depth_limit.min(self.limit_depth)),
            jit: jit_depth.is_some(),
            match_call_count: 0,
            hasthen: (self.flags & PCRE2_HASTHEN) != 0,
            start_offset,
            end_offset_top: 0,
            bsr_convention: self.bsr_convention,
            name_entry_size: self.name_entry_size as usize,
            name_table: 0,
            start_code: self.code_start,
            check_subject,
            end_subject,
            true_end_subject,
            end_match_ptr: 0,
            start_used_ptr: 0,
            last_used_ptr: 0,
            mark: UNSET,
            nomatch_mark: UNSET,
            verb_ecode_ptr: 0,
            verb_skip_ptr: 0,
            verb_skip_name: UNSET,
            verb_current_recurse: 0,
            moptions: 0,
            poptions: self.overall_options,
            skip_arg_count: 0,
            ignore_skip_arg: 0,
            nltype: NLTYPE_FIXED,
            nllen: 1,
            nl: [b'\n', 0, 0, 0],
            utf,
            ucp,
            assert_accept_frame: UNSET,
            branch_end: UNSET,
            out_ovector: std::mem::take(&mut md.ovector),
            oveccount: md.oveccount,
        };
        if m.out_ovector.len() < 2 * md.oveccount {
            m.out_ovector.resize(2 * md.oveccount, PCRE2_UNSET);
        }
        match self.newline_convention as u32 {
            PCRE2_NEWLINE_CR => {
                m.nllen = 1;
                m.nl[0] = b'\r';
            }
            PCRE2_NEWLINE_LF => {
                m.nllen = 1;
                m.nl[0] = b'\n';
            }
            PCRE2_NEWLINE_NUL => {
                m.nllen = 1;
                m.nl[0] = 0;
            }
            PCRE2_NEWLINE_CRLF => {
                m.nllen = 2;
                m.nl[0] = b'\r';
                m.nl[1] = b'\n';
            }
            PCRE2_NEWLINE_ANY => m.nltype = NLTYPE_ANY,
            PCRE2_NEWLINE_ANYCRLF => m.nltype = NLTYPE_ANYCRLF,
            _ => {
                md.ovector = std::mem::take(&mut m.out_ovector);
                return PCRE2_ERROR_INTERNAL;
            }
        }

        let fcc = |c: u32| self.tables[256 + c as usize] as u32;
        let mut has_first_cu = false;
        let mut has_req_cu = false;
        let mut first_cu: u8 = 0;
        let mut first_cu2: u8 = 0;
        let mut req_cu: u8 = 0;
        let mut req_cu2: u8 = 0;
        let mut start_bits: Option<&[u8; 32]> = None;
        if (self.flags & PCRE2_FIRSTSET) != 0 {
            has_first_cu = true;
            first_cu = self.first_codeunit as u8;
            first_cu2 = first_cu;
            if (self.flags & PCRE2_FIRSTCASELESS) != 0 {
                first_cu2 = fcc(first_cu as u32) as u8;
                if first_cu > 127 && ucp && !utf {
                    first_cu2 = ucd_othercase(first_cu as u32) as u8;
                }
            }
        } else if !startline && (self.flags & PCRE2_FIRSTMAPSET) != 0 {
            start_bits = Some(&self.start_bitmap);
        }
        if (self.flags & PCRE2_LASTSET) != 0 {
            has_req_cu = true;
            req_cu = self.last_codeunit as u8;
            req_cu2 = req_cu;
            if (self.flags & PCRE2_LASTCASELESS) != 0 {
                req_cu2 = fcc(req_cu as u32) as u8;
                if req_cu > 127 && ucp && !utf {
                    req_cu2 = ucd_othercase(req_cu as u32) as u8;
                }
            }
        }

        let memchr = |s: &[u8], from: usize, to: usize, c: u8| -> Option<usize> {
            memchr::memchr(c, &s[from..to]).map(|p| p + from)
        };

        let mut rc: i32;
        #[allow(unused_assignments)]
        'fragment: loop {
            let mut memchr_found_first_cu: Option<usize> = None;
            let mut memchr_found_first_cu2: Option<usize> = None;
            rc = MATCH_NOMATCH;
            loop {
                let new_start_match: usize;
                if (self.overall_options & PCRE2_NO_START_OPTIMIZE) == 0 {
                    if firstline {
                        let mut t = start_match;
                        if utf {
                            while t < end_subject && !m.is_newline(t) {
                                t += 1;
                                while t < end_subject && (subject[t] & 0xc0) == 0x80 {
                                    t += 1;
                                }
                            }
                        } else {
                            while t < end_subject && !m.is_newline(t) {
                                t += 1;
                            }
                        }
                        end_subject = t;
                    }
                    if anchored {
                        if has_first_cu || start_bits.is_some() {
                            let mut ok = start_match < end_subject;
                            if ok {
                                let c = subject[start_match];
                                ok = has_first_cu && (c == first_cu || c == first_cu2);
                                if !ok {
                                    if let Some(sb) = start_bits {
                                        ok = (sb[(c / 8) as usize] & (1u8 << (c & 7))) != 0;
                                    }
                                }
                            }
                            if !ok {
                                rc = MATCH_NOMATCH;
                                break;
                            }
                        }
                    } else if has_first_cu {
                        if first_cu != first_cu2 {
                            let pp1 = if memchr_found_first_cu.is_none_or(|v| start_match > v) {
                                let r = memchr(subject, start_match, end_subject, first_cu);
                                memchr_found_first_cu = Some(r.unwrap_or(end_subject));
                                r
                            } else {
                                let v = memchr_found_first_cu.unwrap();
                                if v == end_subject { None } else { Some(v) }
                            };
                            let pp2 = if memchr_found_first_cu2.is_none_or(|v| start_match > v) {
                                let r = memchr(subject, start_match, end_subject, first_cu2);
                                memchr_found_first_cu2 = Some(r.unwrap_or(end_subject));
                                r
                            } else {
                                let v = memchr_found_first_cu2.unwrap();
                                if v == end_subject { None } else { Some(v) }
                            };
                            start_match = match (pp1, pp2) {
                                (None, None) => end_subject,
                                (None, Some(b)) => b,
                                (Some(a), None) => a,
                                (Some(a), Some(b)) => a.min(b),
                            };
                        } else {
                            start_match = memchr(subject, start_match, end_subject, first_cu)
                                .unwrap_or(end_subject);
                        }
                        if start_match >= m.end_subject {
                            rc = MATCH_NOMATCH;
                            break;
                        }
                    } else if startline {
                        if start_match > start_offset {
                            if utf {
                                while start_match < end_subject && !m.was_newline(start_match) {
                                    start_match += 1;
                                    while start_match < end_subject
                                        && (subject[start_match] & 0xc0) == 0x80
                                    {
                                        start_match += 1;
                                    }
                                }
                            } else {
                                while start_match < end_subject && !m.was_newline(start_match) {
                                    start_match += 1;
                                }
                            }
                            if subject[start_match - 1] == b'\r'
                                && (m.nltype == NLTYPE_ANY || m.nltype == NLTYPE_ANYCRLF)
                                && start_match < end_subject
                                && subject[start_match] == b'\n'
                            {
                                start_match += 1;
                            }
                        }
                    } else if let Some(sb) = start_bits {
                        while start_match < end_subject {
                            let c = subject[start_match];
                            if (sb[(c / 8) as usize] & (1u8 << (c & 7))) != 0 {
                                break;
                            }
                            start_match += 1;
                        }
                        if start_match >= m.end_subject {
                            rc = MATCH_NOMATCH;
                            break;
                        }
                    }

                    // Restore end_subject (FIRSTLINE may have changed it).
                    end_subject = m.end_subject;

                    if end_subject - start_match < self.minlength as usize {
                        rc = MATCH_NOMATCH;
                        break;
                    }
                    let mut p = start_match + if has_first_cu { 1 } else { 0 };
                    if has_req_cu && p as isize > req_cu_ptr {
                        let check_length = end_subject - start_match;
                        if check_length < REQ_CU_MAX as usize
                            || (!anchored && check_length < REQ_CU_MAX as usize * 1000)
                        {
                            if req_cu != req_cu2 {
                                let pp = p;
                                p = match memchr(subject, pp, end_subject, req_cu) {
                                    Some(x) => x,
                                    None => memchr(subject, pp, end_subject, req_cu2)
                                        .unwrap_or(end_subject),
                                };
                            } else {
                                p = memchr(subject, p, end_subject, req_cu).unwrap_or(end_subject);
                            }
                            if p >= end_subject {
                                rc = MATCH_NOMATCH;
                                break;
                            }
                            req_cu_ptr = p as isize;
                        }
                    }
                }

                if start_match > bumpalong_limit {
                    rc = MATCH_NOMATCH;
                    break;
                }

                m.start_used_ptr = start_match;
                m.last_used_ptr = start_match;
                m.moptions = options | fragment_options;
                m.match_call_count = 0;
                m.end_offset_top = 0;
                m.skip_arg_count = 0;

                rc = m.run(start_match, self.code_start);

                match rc {
                    MATCH_SKIP_ARG => {
                        new_start_match = start_match;
                        m.ignore_skip_arg = m.skip_arg_count;
                    }
                    MATCH_SKIP if m.verb_skip_ptr > start_match => {
                        new_start_match = m.verb_skip_ptr;
                    }
                    MATCH_SKIP | MATCH_NOMATCH | MATCH_PRUNE | MATCH_THEN => {
                        m.ignore_skip_arg = 0;
                        let mut n = start_match + 1;
                        if utf {
                            while n < end_subject && (subject[n] & 0xc0) == 0x80 {
                                n += 1;
                            }
                        }
                        new_start_match = n;
                    }
                    MATCH_COMMIT => {
                        rc = MATCH_NOMATCH;
                        break;
                    }
                    _ => break,
                }

                rc = MATCH_NOMATCH;
                if firstline && m.is_newline(start_match) {
                    break;
                }
                start_match = new_start_match;
                if anchored || start_match > end_subject {
                    break;
                }
                if start_match > start_offset
                    && subject[start_match - 1] == b'\r'
                    && start_match < end_subject
                    && subject[start_match] == b'\n'
                    && (self.flags & PCRE2_HASCRORLF) == 0
                    && (m.nltype == NLTYPE_ANY || m.nltype == NLTYPE_ANYCRLF || m.nllen == 2)
                {
                    start_match += 1;
                }
                m.mark = UNSET;
            }

            // ENDLOOP: invalid UTF fragments.
            if utf && end_subject != true_end_subject && rc == MATCH_NOMATCH {
                loop {
                    start_match = end_subject + 1;
                    while start_match < true_end_subject && not_firstcu(subject[start_match]) {
                        start_match += 1;
                    }
                    if start_match >= true_end_subject {
                        rc = MATCH_NOMATCH;
                        break;
                    }
                    m.check_subject = start_match;
                    match valid_utf(&subject[start_match..]) {
                        Ok(()) => {
                            end_subject = true_end_subject;
                            m.end_subject = end_subject;
                            fragment_options = PCRE2_NOTBOL;
                            continue 'fragment;
                        }
                        Err((r, off)) => {
                            if r < 0 {
                                end_subject = start_match + off;
                                m.end_subject = end_subject;
                                if end_subject > start_match {
                                    fragment_options = PCRE2_NOTBOL | PCRE2_NOTEOL;
                                    continue 'fragment;
                                }
                            } else {
                                rc = r;
                                break;
                            }
                        }
                    }
                }
            }
            break;
        }

        md.ovector = std::mem::take(&mut m.out_ovector);
        if rc == MATCH_MATCH {
            md.rc = if m.end_offset_top >= 2 * md.oveccount {
                0
            } else {
                (m.end_offset_top / 2 + 1) as i32
            };
            md.mark = mark_name(&self.block, m.mark);
            md.startchar = start_match;
            md.leftchar = m.start_used_ptr;
            md.rightchar = m.last_used_ptr.max(m.end_match_ptr);
            return md.rc;
        }
        md.mark = mark_name(&self.block, m.nomatch_mark);
        if rc != MATCH_NOMATCH {
            md.rc = rc;
        } else {
            md.rc = PCRE2_ERROR_NOMATCH;
        }
        md.rc
    }
}
