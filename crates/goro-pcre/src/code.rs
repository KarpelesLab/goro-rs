//! The compiled pattern (pcre2_real_code) and pcre2_compile().

use crate::cb::Cb;
use crate::compile::{FirstReq, find_firstassertedcu, find_recurse};
use crate::consts::*;
use crate::tables;
use crate::util::*;

/// A compiled regular expression.
#[derive(Clone, Debug)]
pub struct Code {
    pub tables: &'static [u8],
    pub start_bitmap: [u8; 32],
    pub compile_options: u32,
    pub overall_options: u32,
    pub extra_options: u32,
    pub flags: u32,
    pub limit_heap: u32,
    pub limit_match: u32,
    pub limit_depth: u32,
    pub first_codeunit: u32,
    pub last_codeunit: u32,
    pub bsr_convention: u16,
    pub newline_convention: u16,
    pub max_lookbehind: u16,
    pub minlength: u16,
    pub top_bracket: u16,
    pub top_backref: u16,
    pub name_entry_size: u16,
    pub name_count: u16,
    /// Name table followed by the byte code.
    pub block: Vec<u8>,
    pub code_start: usize,
}

/// A compile error: PCRE2 error code and offset in the pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileError {
    pub code: i32,
    pub offset: usize,
}

impl CompileError {
    pub fn message(&self) -> &'static str {
        error_message(self.code)
    }
}

/// Compile-context settings (pcre2_compile_context).
#[derive(Clone, Copy, Debug)]
pub struct CompileContext {
    pub bsr_convention: u16,
    pub newline_convention: u16,
    pub parens_nest_limit: u32,
    pub extra_options: u32,
    pub max_varlookbehind: u32,
    pub max_pattern_length: usize,
    /// Character tables (from `maketables`); None for the default "C" tables.
    pub tables: Option<&'static [u8]>,
}

impl Default for CompileContext {
    fn default() -> Self {
        CompileContext {
            bsr_convention: PCRE2_BSR_UNICODE as u16,
            newline_convention: PCRE2_NEWLINE_LF as u16,
            parens_nest_limit: PARENS_NEST_LIMIT,
            extra_options: 0,
            max_varlookbehind: MAX_VARLOOKBEHIND,
            max_pattern_length: usize::MAX,
            tables: None,
        }
    }
}

const PUBLIC_LITERAL_COMPILE_OPTIONS: u32 = PCRE2_ANCHORED
    | PCRE2_AUTO_CALLOUT
    | PCRE2_CASELESS
    | PCRE2_ENDANCHORED
    | PCRE2_FIRSTLINE
    | PCRE2_LITERAL
    | PCRE2_MATCH_INVALID_UTF
    | PCRE2_NO_START_OPTIMIZE
    | PCRE2_NO_UTF_CHECK
    | PCRE2_USE_OFFSET_LIMIT
    | PCRE2_UTF;

const PUBLIC_COMPILE_OPTIONS: u32 = PUBLIC_LITERAL_COMPILE_OPTIONS
    | PCRE2_ALLOW_EMPTY_CLASS
    | PCRE2_ALT_BSUX
    | PCRE2_ALT_CIRCUMFLEX
    | PCRE2_ALT_VERBNAMES
    | PCRE2_DOLLAR_ENDONLY
    | PCRE2_DOTALL
    | PCRE2_DUPNAMES
    | PCRE2_EXTENDED
    | PCRE2_EXTENDED_MORE
    | PCRE2_MATCH_UNSET_BACKREF
    | PCRE2_MULTILINE
    | PCRE2_NEVER_BACKSLASH_C
    | PCRE2_NEVER_UCP
    | PCRE2_NEVER_UTF
    | PCRE2_NO_AUTO_CAPTURE
    | PCRE2_NO_AUTO_POSSESS
    | PCRE2_NO_DOTSTAR_ANCHOR
    | PCRE2_UCP
    | PCRE2_UNGREEDY;

const PUBLIC_LITERAL_COMPILE_EXTRA_OPTIONS: u32 =
    PCRE2_EXTRA_MATCH_LINE | PCRE2_EXTRA_MATCH_WORD | PCRE2_EXTRA_CASELESS_RESTRICT;

const PUBLIC_COMPILE_EXTRA_OPTIONS: u32 = PUBLIC_LITERAL_COMPILE_EXTRA_OPTIONS
    | PCRE2_EXTRA_ALLOW_SURROGATE_ESCAPES
    | PCRE2_EXTRA_BAD_ESCAPE_IS_LITERAL
    | PCRE2_EXTRA_ESCAPED_CR_IS_LF
    | PCRE2_EXTRA_ALT_BSUX
    | PCRE2_EXTRA_ALLOW_LOOKAROUND_BSK
    | PCRE2_EXTRA_ASCII_BSD
    | PCRE2_EXTRA_ASCII_BSS
    | PCRE2_EXTRA_ASCII_BSW
    | PCRE2_EXTRA_ASCII_POSIX
    | PCRE2_EXTRA_ASCII_DIGIT;

const PSO_OPT: u8 = 0;
const PSO_FLG: u8 = 1;
const PSO_NL: u8 = 2;
const PSO_BSR: u8 = 3;
const PSO_LIMH: u8 = 4;
const PSO_LIMM: u8 = 5;
const PSO_LIMD: u8 = 6;

static PSO_LIST: [(&[u8], u8, u32); 21] = [
    (b"UTF8)", PSO_OPT, PCRE2_UTF),
    (b"UTF)", PSO_OPT, PCRE2_UTF),
    (b"UCP)", PSO_OPT, PCRE2_UCP),
    (b"NOTEMPTY)", PSO_FLG, PCRE2_NOTEMPTY_SET),
    (b"NOTEMPTY_ATSTART)", PSO_FLG, PCRE2_NE_ATST_SET),
    (b"NO_AUTO_POSSESS)", PSO_OPT, PCRE2_NO_AUTO_POSSESS),
    (b"NO_DOTSTAR_ANCHOR)", PSO_OPT, PCRE2_NO_DOTSTAR_ANCHOR),
    (b"NO_JIT)", PSO_FLG, PCRE2_NOJIT),
    (b"NO_START_OPT)", PSO_OPT, PCRE2_NO_START_OPTIMIZE),
    (b"LIMIT_HEAP=", PSO_LIMH, 0),
    (b"LIMIT_MATCH=", PSO_LIMM, 0),
    (b"LIMIT_DEPTH=", PSO_LIMD, 0),
    (b"LIMIT_RECURSION=", PSO_LIMD, 0),
    (b"CR)", PSO_NL, PCRE2_NEWLINE_CR),
    (b"LF)", PSO_NL, PCRE2_NEWLINE_LF),
    (b"CRLF)", PSO_NL, PCRE2_NEWLINE_CRLF),
    (b"ANY)", PSO_NL, PCRE2_NEWLINE_ANY),
    (b"NUL)", PSO_NL, PCRE2_NEWLINE_NUL),
    (b"ANYCRLF)", PSO_NL, PCRE2_NEWLINE_ANYCRLF),
    (b"BSR_ANYCRLF)", PSO_BSR, PCRE2_BSR_ANYCRLF),
    (b"BSR_UNICODE)", PSO_BSR, PCRE2_BSR_UNICODE),
];

impl Code {
    /// pcre2_compile()
    pub fn compile(
        pattern: &[u8],
        mut options: u32,
        ccontext: &CompileContext,
    ) -> Result<Code, CompileError> {
        let patlen = pattern.len();
        let err = |code: i32, offset: usize| Err(CompileError { code, offset });

        if (options & PCRE2_MATCH_INVALID_UTF) != 0 {
            options |= PCRE2_UTF;
        }
        if (options & !PUBLIC_COMPILE_OPTIONS) != 0
            || (ccontext.extra_options & !PUBLIC_COMPILE_EXTRA_OPTIONS) != 0
        {
            return err(ERR17, 0);
        }
        if (options & PCRE2_LITERAL) != 0
            && ((options & !PUBLIC_LITERAL_COMPILE_OPTIONS) != 0
                || (ccontext.extra_options & !PUBLIC_LITERAL_COMPILE_EXTRA_OPTIONS) != 0)
        {
            return err(ERR92, 0);
        }
        if patlen > ccontext.max_pattern_length {
            return err(ERR88, 0);
        }

        let tables: &'static [u8] = ccontext.tables.unwrap_or(&tables::DEFAULT_TABLES);
        let mut cb = Cb {
            pat: pattern,
            tables,
            code: vec![0u8; COMPILE_WORK_SIZE * 4 + 1024],
            start_code: 0,
            start_workspace: 0,
            workspace_size: COMPILE_WORK_SIZE,
            name_table: 0,
            small_ref_offset: [PCRE2_UNSET; 10],
            erroroffset: 0,
            names_found: 0,
            name_entry_size: 0,
            parens_depth: 0,
            assert_depth: 0,
            named_groups: Vec::new(),
            external_options: options,
            external_flags: 0,
            bracount: 0,
            lastcapture: 0,
            parsed: Vec::new(),
            parsed_end: 0,
            groupinfo: Vec::new(),
            top_backref: 0,
            backref_map: 0,
            nltype: NLTYPE_FIXED,
            nllen: 0,
            nl: [0; 4],
            class_range_start: 0,
            class_range_end: 0,
            req_varyopt: 0,
            max_varlookbehind: ccontext.max_varlookbehind,
            max_lookbehind: 0,
            had_accept: false,
            had_pruneorskip: false,
            had_recurse: false,
            dupnames: false,
            parens_nest_limit: ccontext.parens_nest_limit,
            cx_extra_options: ccontext.extra_options,
            open_caps: Vec::new(),
        };

        let mut setflags: u32 = 0;
        let mut limit_heap = u32::MAX;
        let mut limit_match = u32::MAX;
        let mut limit_depth = u32::MAX;
        let mut newline: u32 = 0;
        let mut bsr: u32 = 0;

        // Check for global one-time option settings at the start.
        let mut skipatstart = 0usize;
        if (options & PCRE2_LITERAL) == 0 {
            while patlen - skipatstart >= 2
                && pattern[skipatstart] == b'('
                && pattern[skipatstart + 1] == b'*'
            {
                let mut found = false;
                for &(name, ty, value) in PSO_LIST.iter() {
                    if patlen - skipatstart - 2 >= name.len()
                        && &pattern[skipatstart + 2..skipatstart + 2 + name.len()] == name
                    {
                        skipatstart += name.len() + 2;
                        match ty {
                            PSO_OPT => cb.external_options |= value,
                            PSO_FLG => setflags |= value,
                            PSO_NL => {
                                newline = value;
                                setflags |= PCRE2_NL_SET;
                            }
                            PSO_BSR => {
                                bsr = value;
                                setflags |= PCRE2_BSR_SET;
                            }
                            _ => {
                                let mut c: u32 = 0;
                                let mut pp = skipatstart;
                                let at = |i: usize| *pattern.get(i).unwrap_or(&0);
                                if !at(pp).is_ascii_digit() {
                                    return err(ERR60, pp);
                                }
                                while at(pp).is_ascii_digit() {
                                    if c > u32::MAX / 10 - 1 {
                                        break;
                                    }
                                    c = c * 10 + (at(pp) - b'0') as u32;
                                    pp += 1;
                                }
                                let ch = at(pp);
                                pp += 1;
                                if ch != b')' {
                                    return err(ERR60, pp);
                                }
                                if ty == PSO_LIMH {
                                    limit_heap = c;
                                } else if ty == PSO_LIMM {
                                    limit_match = c;
                                } else {
                                    limit_depth = c;
                                }
                                skipatstart = pp;
                            }
                        }
                        found = true;
                        break;
                    }
                }
                if !found {
                    break;
                }
            }
        }
        let ptr = skipatstart;

        let utf = (cb.external_options & PCRE2_UTF) != 0;
        if utf {
            if (options & PCRE2_NEVER_UTF) != 0 {
                return err(ERR74, ptr);
            }
            if (options & PCRE2_NO_UTF_CHECK) == 0 {
                if let Err((code, offset)) = valid_utf(pattern) {
                    return err(code, offset);
                }
            }
        }
        let ucp = (cb.external_options & PCRE2_UCP) != 0;
        if ucp && (cb.external_options & PCRE2_NEVER_UCP) != 0 {
            return err(ERR75, ptr);
        }

        if bsr == 0 {
            bsr = ccontext.bsr_convention as u32;
        }
        if newline == 0 {
            newline = ccontext.newline_convention as u32;
        }
        cb.nltype = NLTYPE_FIXED;
        match newline {
            PCRE2_NEWLINE_CR => {
                cb.nllen = 1;
                cb.nl[0] = b'\r';
            }
            PCRE2_NEWLINE_LF => {
                cb.nllen = 1;
                cb.nl[0] = b'\n';
            }
            PCRE2_NEWLINE_NUL => {
                cb.nllen = 1;
                cb.nl[0] = 0;
            }
            PCRE2_NEWLINE_CRLF => {
                cb.nllen = 2;
                cb.nl[0] = b'\r';
                cb.nl[1] = b'\n';
            }
            PCRE2_NEWLINE_ANY => cb.nltype = NLTYPE_ANY,
            PCRE2_NEWLINE_ANYCRLF => cb.nltype = NLTYPE_ANYCRLF,
            _ => return err(ERR56, ptr),
        }

        let mut parsed_size_needed = patlen - skipatstart;
        if (ccontext.extra_options & (PCRE2_EXTRA_MATCH_WORD | PCRE2_EXTRA_MATCH_LINE)) != 0 {
            parsed_size_needed += 4;
        }
        if (options & PCRE2_AUTO_CALLOUT) != 0 {
            parsed_size_needed = (parsed_size_needed + 1) * 5;
        }
        cb.parsed = vec![0u32; parsed_size_needed + 1 + 64];
        cb.parsed_end = parsed_size_needed + 1;

        let mut has_lookbehind = false;
        let errorcode = cb.parse_regex(ptr, cb.external_options, &mut has_lookbehind);
        if errorcode != 0 {
            return err(errorcode, cb.erroroffset);
        }

        if has_lookbehind {
            let mut loopcount = 0;
            cb.groupinfo = vec![0u32; 2 * (cb.bracount as usize + 1) + 2];
            let mut recurses = Vec::new();
            let errorcode = cb.check_lookbehinds(0, None, &mut recurses, &mut loopcount);
            if errorcode != 0 {
                return err(errorcode, cb.erroroffset);
            }
        }

        // Pass 1: compute the length.
        cb.erroroffset = patlen;
        let mut pptr = 0usize;
        let mut code = 0usize;
        cb.code[0] = OP_BRA as u8;
        let mut length: usize = 1;
        let mut errorcode = 0;
        let mut fr = FirstReq::default();
        cb.compile_regex(
            cb.external_options,
            ccontext.extra_options,
            &mut code,
            &mut pptr,
            &mut errorcode,
            0,
            &mut fr,
            true,
            &mut length,
        );
        if errorcode != 0 {
            return err(errorcode, cb.erroroffset);
        }
        if length > MAX_PATTERN_SIZE {
            return err(ERR20, cb.erroroffset);
        }

        // Pass 2: the real compile.
        let name_table_size = cb.names_found as usize * cb.name_entry_size as usize;
        cb.code = vec![0u8; name_table_size + length + 1024];
        cb.parens_depth = 0;
        cb.assert_depth = 0;
        cb.lastcapture = 0;
        cb.name_table = 0;
        cb.start_code = name_table_size;
        cb.start_workspace = 0;
        cb.req_varyopt = 0;
        cb.had_accept = false;
        cb.had_pruneorskip = false;
        cb.open_caps.clear();
        let codestart = name_table_size;

        if cb.names_found > 0 {
            let ngs = cb.named_groups.clone();
            for (i, ng) in ngs.iter().enumerate().take(cb.names_found as usize) {
                cb.add_name_to_table(ng.name, ng.length as usize, ng.number, i as u32);
            }
        }

        let mut re = Code {
            tables,
            start_bitmap: [0; 32],
            compile_options: options,
            overall_options: cb.external_options,
            extra_options: ccontext.extra_options,
            flags: 1 | cb.external_flags | setflags,
            limit_heap,
            limit_match,
            limit_depth,
            first_codeunit: 0,
            last_codeunit: 0,
            bsr_convention: bsr as u16,
            newline_convention: newline as u16,
            max_lookbehind: 0,
            minlength: 0,
            top_bracket: 0,
            top_backref: 0,
            name_entry_size: cb.name_entry_size,
            name_count: cb.names_found,
            block: Vec::new(),
            code_start: codestart,
        };

        let mut pptr = 0usize;
        let mut code = codestart;
        cb.code[code] = OP_BRA as u8;
        let mut dummy_len = 0usize;
        let mut fr = FirstReq::default();
        let regexrc = cb.compile_regex(
            re.overall_options,
            ccontext.extra_options,
            &mut code,
            &mut pptr,
            &mut errorcode,
            0,
            &mut fr,
            false,
            &mut dummy_len,
        );
        if regexrc < 0 {
            re.flags |= PCRE2_MATCH_EMPTY;
        }
        re.top_bracket = cb.bracount as u16;
        re.top_backref = cb.top_backref as u16;
        re.max_lookbehind = cb.max_lookbehind as u16;
        let mut firstcu = fr.firstcu;
        let mut firstcuflags = fr.firstcuflags;
        let mut reqcu = fr.reqcu;
        let mut reqcuflags = fr.reqcuflags;

        if cb.had_accept {
            reqcu = 0;
            reqcuflags = REQ_NONE;
            re.flags |= PCRE2_HASACCEPT;
        }

        if errorcode == 0 {
            cb.code[code] = OP_END as u8;
            code += 1;
            let usedlength = code - codestart;
            if usedlength > length {
                errorcode = ERR23;
            }
        }

        // Fill in group numbers for OP_RECURSE.
        if errorcode == 0 && cb.had_recurse {
            let mut rcode = find_recurse(&cb.code, codestart, utf);
            let mut cache: Vec<(u32, usize)> = Vec::new();
            while let Some(rc) = rcode {
                let groupnumber = get(&cb.code, rc + 1) as u32;
                let rgroup = if groupnumber == 0 {
                    codestart
                } else {
                    let mut search_from = codestart;
                    let mut rg = None;
                    for &(gn, g) in cache.iter() {
                        if gn == groupnumber {
                            rg = Some(g);
                            break;
                        }
                        if groupnumber > gn {
                            search_from = g;
                        }
                    }
                    match rg {
                        Some(g) => g,
                        None => {
                            match find_bracket(&cb.code, search_from, utf, groupnumber as i32) {
                                None => {
                                    errorcode = ERR53;
                                    break;
                                }
                                Some(g) => {
                                    cache.insert(0, (groupnumber, g));
                                    cache.truncate(8);
                                    g
                                }
                            }
                        }
                    }
                };
                put(&mut cb.code, rc + 1, rgroup - codestart);
                rcode = find_recurse(&cb.code, rc + 1 + LINK_SIZE, utf);
            }
        }

        if errorcode == 0
            && (re.overall_options & PCRE2_NO_AUTO_POSSESS) == 0
            && cb.auto_possessify(codestart) != 0
        {
            errorcode = ERR80;
        }
        if errorcode != 0 {
            return err(errorcode, cb.erroroffset);
        }

        if (re.overall_options & PCRE2_ANCHORED) == 0 && cb.is_anchored(codestart, 0, 0, false) {
            re.overall_options |= PCRE2_ANCHORED;
        }

        let do_start = (re.overall_options & PCRE2_NO_START_OPTIMIZE) == 0;
        let mut minminlength = 0u16;
        if do_start {
            if firstcuflags >= REQ_NONE {
                firstcu = find_firstassertedcu(&cb.code, codestart, &mut firstcuflags, 0);
            }
            if firstcuflags < REQ_NONE {
                re.first_codeunit = firstcu;
                re.flags |= PCRE2_FIRSTSET;
                minminlength += 1;
                if (firstcuflags & REQ_CASELESS) != 0 {
                    if firstcu < 128 || (!utf && !ucp && firstcu < 255) {
                        if cb.fcc(firstcu) != firstcu {
                            re.flags |= PCRE2_FIRSTCASELESS;
                        }
                    } else if ucp && !utf && ucd_othercase(firstcu) != firstcu {
                        re.flags |= PCRE2_FIRSTCASELESS;
                    }
                }
            } else if (re.overall_options & PCRE2_ANCHORED) == 0
                && cb.is_startline(codestart, 0, 0, false)
            {
                re.flags |= PCRE2_STARTLINE;
            }

            if reqcuflags < REQ_NONE {
                if (re.overall_options & PCRE2_UTF) == 0
                    || firstcuflags >= REQ_NONE
                    || (firstcu & 0x80) == 0
                    || (reqcu & 0x80) == 0
                {
                    minminlength += 1;
                }
                if (re.overall_options & PCRE2_ANCHORED) == 0 || (reqcuflags & REQ_VARY) != 0 {
                    re.last_codeunit = reqcu;
                    re.flags |= PCRE2_LASTSET;
                    if (reqcuflags & REQ_CASELESS) != 0 {
                        if reqcu < 128 || (!utf && !ucp && reqcu < 255) {
                            if cb.fcc(reqcu) != reqcu {
                                re.flags |= PCRE2_LASTCASELESS;
                            }
                        } else if ucp && !utf && ucd_othercase(reqcu) != reqcu {
                            re.flags |= PCRE2_LASTCASELESS;
                        }
                    }
                }
            }
        }

        cb.code.truncate(code);
        re.block = std::mem::take(&mut cb.code);

        if do_start {
            if re.study() != 0 {
                return err(ERR31, cb.erroroffset);
            }
            if (re.flags & PCRE2_FIRSTMAPSET) != 0 && minminlength == 0 {
                minminlength = 1;
            }
            if re.minlength < minminlength {
                re.minlength = minminlength;
            }
        }
        Ok(re)
    }

    /// Number of capturing groups (PCRE2_INFO_CAPTURECOUNT).
    pub fn capture_count(&self) -> u32 {
        self.top_bracket as u32
    }

    /// The name table as (group number, name) pairs, in table order.
    pub fn names(&self) -> Vec<(u32, Vec<u8>)> {
        let mut out = Vec::new();
        let ese = self.name_entry_size as usize;
        for i in 0..self.name_count as usize {
            let slot = i * ese;
            let n = get2(&self.block, slot);
            let name = &self.block[slot + IMM2_SIZE..slot + ese];
            let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            out.push((n, name[..len].to_vec()));
        }
        out
    }
}

/// The C library character classification of one byte, for `maketables`.
#[derive(Clone, Copy, Default, Debug)]
pub struct CtypeInfo {
    pub lower: u8,
    pub upper: u8,
    pub is_lower: bool,
    pub is_upper: bool,
    pub is_alpha: bool,
    pub is_digit: bool,
    pub is_alnum: bool,
    pub is_space: bool,
    pub is_xdigit: bool,
    pub is_graph: bool,
    pub is_print: bool,
    pub is_punct: bool,
    pub is_cntrl: bool,
}

/// pcre2_maketables(): build character tables from the classification of
/// each byte in the current locale.
pub fn maketables(ct: impl Fn(u8) -> CtypeInfo) -> Vec<u8> {
    let info: Vec<CtypeInfo> = (0..=255u8).map(ct).collect();
    let mut t = vec![0u8; TABLES_LENGTH];
    for i in 0..256 {
        t[i] = info[i].lower;
        t[256 + i] = if info[i].is_lower {
            info[i].upper
        } else {
            info[i].lower
        };
    }
    let cb = 512;
    for (i, ci) in info.iter().enumerate() {
        let bit = 1u8 << (i & 7);
        let byte = i / 8;
        if ci.is_digit {
            t[cb + 64 + byte] |= bit;
        }
        if ci.is_upper {
            t[cb + 96 + byte] |= bit;
        }
        if ci.is_lower {
            t[cb + 128 + byte] |= bit;
        }
        if ci.is_alnum || i == b'_' as usize {
            t[cb + 160 + byte] |= bit;
        }
        if ci.is_space {
            t[cb + byte] |= bit;
        }
        if ci.is_xdigit {
            t[cb + 32 + byte] |= bit;
        }
        if ci.is_graph {
            t[cb + 192 + byte] |= bit;
        }
        if ci.is_print {
            t[cb + 224 + byte] |= bit;
        }
        if ci.is_punct {
            t[cb + 256 + byte] |= bit;
        }
        if ci.is_cntrl {
            t[cb + 288 + byte] |= bit;
        }
    }
    for (i, ci) in info.iter().enumerate() {
        let mut x = 0u8;
        if ci.is_space {
            x += 0x01;
        }
        if ci.is_alpha {
            x += 0x02;
        }
        if ci.is_lower {
            x += 0x04;
        }
        if ci.is_digit {
            x += 0x08;
        }
        if ci.is_alnum || i == b'_' as usize {
            x += 0x10;
        }
        t[CTYPES_OFFSET + i] = x;
    }
    t
}
