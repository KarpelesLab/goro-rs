//! Port of pcre2_study.c: compute the minimum subject length and the
//! starting code unit bitmap.

use crate::code::Code;
use crate::consts::*;
use crate::tables;
use crate::util::*;

const MAX_CACHE_BACKREF: usize = 128;

const SSB_FAIL: i32 = 0;
const SSB_DONE: i32 = 1;
const SSB_CONTINUE: i32 = 2;
const SSB_UNKNOWN: i32 = 3;
const SSB_TOODEEP: i32 = 4;

impl Code {
    #[inline]
    fn b(&self, i: usize) -> u32 {
        self.block[i] as u32
    }

    #[allow(clippy::too_many_arguments)]
    fn find_minlength(
        &self,
        code: usize,
        startcode: usize,
        utf: bool,
        recurses: &mut Vec<usize>,
        countptr: &mut i32,
        backref_cache: &mut [i32; MAX_CACHE_BACKREF + 1],
    ) -> i32 {
        let bl = &self.block;
        let mut length: i32 = -1;
        let mut branchlength: i32 = 0;
        let mut prev_cap_recno: i32 = -1;
        let mut prev_cap_d: i32 = 0;
        let mut prev_recurse_recno: i32 = -1;
        let mut prev_recurse_d: i32 = 0;
        let mut once_fudge: usize = 0;
        let mut had_recurse = false;
        let dupcapused = (self.flags & PCRE2_DUPCAPUSED) != 0;
        let mut nextbranch = code + get(bl, code + 1);
        let mut cc = code + 1 + LINK_SIZE;

        let op0 = self.b(code);
        if (OP_SBRA..=OP_SCOND).contains(&op0) {
            return 0;
        }
        if op0 == OP_CBRA || op0 == OP_CBRAPOS {
            cc += IMM2_SIZE;
        }
        let cnt = *countptr;
        *countptr += 1;
        if cnt > 1000 {
            return -1;
        }

        loop {
            if branchlength >= u16::MAX as i32 {
                branchlength = u16::MAX as i32;
                cc = nextbranch;
            }
            let op = self.b(cc);
            let mut d: i32;
            match op {
                OP_COND | OP_SCOND | OP_BRA | OP_ONCE | OP_SCRIPT_RUN | OP_SBRA | OP_BRAPOS
                | OP_SBRAPOS => {
                    if op == OP_COND || op == OP_SCOND {
                        let cs = cc + get(bl, cc + 1);
                        if self.b(cs) != OP_ALT {
                            cc = cs + 1 + LINK_SIZE;
                            continue;
                        }
                    } else if op == OP_BRA
                        && self.b(cc + 1 + LINK_SIZE) == OP_RECURSE
                        && self.b(cc + 2 * (1 + LINK_SIZE)) == OP_KET
                    {
                        once_fudge = 1 + LINK_SIZE;
                        cc += 1 + LINK_SIZE;
                        continue;
                    }
                    d = self.find_minlength(cc, startcode, utf, recurses, countptr, backref_cache);
                    if d < 0 {
                        return d;
                    }
                    branchlength += d;
                    loop {
                        cc += get(bl, cc + 1);
                        if self.b(cc) != OP_ALT {
                            break;
                        }
                    }
                    cc += 1 + LINK_SIZE;
                }
                OP_CBRA | OP_SCBRA | OP_CBRAPOS | OP_SCBRAPOS => {
                    let recno = get2(bl, cc + 1 + LINK_SIZE) as i32;
                    if dupcapused || recno != prev_cap_recno {
                        prev_cap_recno = recno;
                        prev_cap_d = self.find_minlength(
                            cc,
                            startcode,
                            utf,
                            recurses,
                            countptr,
                            backref_cache,
                        );
                        if prev_cap_d < 0 {
                            return prev_cap_d;
                        }
                    }
                    branchlength += prev_cap_d;
                    loop {
                        cc += get(bl, cc + 1);
                        if self.b(cc) != OP_ALT {
                            break;
                        }
                    }
                    cc += 1 + LINK_SIZE;
                }
                OP_ACCEPT | OP_ASSERT_ACCEPT => return -1,
                OP_ALT | OP_KET | OP_KETRMAX | OP_KETRMIN | OP_KETRPOS | OP_END => {
                    if length < 0 || (!had_recurse && branchlength < length) {
                        length = branchlength;
                    }
                    if op != OP_ALT || length == 0 {
                        return length;
                    }
                    nextbranch = cc + get(bl, cc + 1);
                    cc += 1 + LINK_SIZE;
                    branchlength = 0;
                    had_recurse = false;
                }
                OP_ASSERT | OP_ASSERT_NOT | OP_ASSERTBACK | OP_ASSERTBACK_NOT | OP_ASSERT_NA
                | OP_ASSERTBACK_NA => {
                    loop {
                        cc += get(bl, cc + 1);
                        if self.b(cc) != OP_ALT {
                            break;
                        }
                    }
                    cc += op_len(bl[cc]);
                }
                OP_REVERSE
                | OP_VREVERSE
                | OP_CREF
                | OP_DNCREF
                | OP_RREF
                | OP_DNRREF
                | OP_FALSE
                | OP_TRUE
                | OP_CALLOUT
                | OP_SOD
                | OP_SOM
                | OP_EOD
                | OP_EODN
                | OP_CIRC
                | OP_CIRCM
                | OP_DOLL
                | OP_DOLLM
                | OP_NOT_WORD_BOUNDARY
                | OP_WORD_BOUNDARY
                | OP_NOT_UCP_WORD_BOUNDARY
                | OP_UCP_WORD_BOUNDARY => {
                    cc += op_len(bl[cc]);
                }
                OP_CALLOUT_STR => cc += get(bl, cc + 1 + 2 * LINK_SIZE),
                OP_BRAZERO | OP_BRAMINZERO | OP_BRAPOSZERO | OP_SKIPZERO => {
                    cc += op_len(bl[cc]);
                    loop {
                        cc += get(bl, cc + 1);
                        if self.b(cc) != OP_ALT {
                            break;
                        }
                    }
                    cc += 1 + LINK_SIZE;
                }
                OP_CHAR | OP_CHARI | OP_NOT | OP_NOTI | OP_PLUS | OP_PLUSI | OP_MINPLUS
                | OP_MINPLUSI | OP_POSPLUS | OP_POSPLUSI | OP_NOTPLUS | OP_NOTPLUSI
                | OP_NOTMINPLUS | OP_NOTMINPLUSI | OP_NOTPOSPLUS | OP_NOTPOSPLUSI => {
                    branchlength += 1;
                    cc += 2;
                    if utf && has_extralen(bl[cc - 1]) {
                        cc += get_extralen(bl[cc - 1]);
                    }
                }
                OP_TYPEPLUS | OP_TYPEMINPLUS | OP_TYPEPOSPLUS => {
                    branchlength += 1;
                    let n = self.b(cc + 1);
                    cc += if n == OP_PROP || n == OP_NOTPROP {
                        4
                    } else {
                        2
                    };
                }
                OP_EXACT | OP_EXACTI | OP_NOTEXACT | OP_NOTEXACTI => {
                    branchlength += get2(bl, cc + 1) as i32;
                    cc += 2 + IMM2_SIZE;
                    if utf && has_extralen(bl[cc - 1]) {
                        cc += get_extralen(bl[cc - 1]);
                    }
                }
                OP_TYPEEXACT => {
                    branchlength += get2(bl, cc + 1) as i32;
                    let n = self.b(cc + 1 + IMM2_SIZE);
                    cc += 2
                        + IMM2_SIZE
                        + if n == OP_PROP || n == OP_NOTPROP {
                            2
                        } else {
                            0
                        };
                }
                OP_PROP | OP_NOTPROP | OP_NOT_DIGIT | OP_DIGIT | OP_NOT_WHITESPACE
                | OP_WHITESPACE | OP_NOT_WORDCHAR | OP_WORDCHAR | OP_ANY | OP_ALLANY
                | OP_EXTUNI | OP_HSPACE | OP_NOT_HSPACE | OP_VSPACE | OP_NOT_VSPACE => {
                    if op == OP_PROP || op == OP_NOTPROP {
                        cc += 2;
                    }
                    branchlength += 1;
                    cc += 1;
                }
                OP_ANYNL => {
                    branchlength += 1;
                    cc += 1;
                }
                OP_ANYBYTE => {
                    if utf {
                        return -1;
                    }
                    branchlength += 1;
                    cc += 1;
                }
                OP_TYPESTAR | OP_TYPEMINSTAR | OP_TYPEQUERY | OP_TYPEMINQUERY | OP_TYPEPOSSTAR
                | OP_TYPEPOSQUERY => {
                    let n = self.b(cc + 1);
                    if n == OP_PROP || n == OP_NOTPROP {
                        cc += 2;
                    }
                    cc += op_len(op as u8);
                }
                OP_TYPEUPTO | OP_TYPEMINUPTO | OP_TYPEPOSUPTO => {
                    let n = self.b(cc + 1 + IMM2_SIZE);
                    if n == OP_PROP || n == OP_NOTPROP {
                        cc += 2;
                    }
                    cc += op_len(op as u8);
                }
                OP_CLASS | OP_NCLASS | OP_XCLASS => {
                    if op == OP_XCLASS {
                        cc += get(bl, cc + 1);
                    } else {
                        cc += op_len(OP_CLASS as u8);
                    }
                    match self.b(cc) {
                        OP_CRPLUS | OP_CRMINPLUS | OP_CRPOSPLUS => {
                            branchlength += 1;
                            cc += 1;
                        }
                        OP_CRSTAR | OP_CRMINSTAR | OP_CRQUERY | OP_CRMINQUERY | OP_CRPOSSTAR
                        | OP_CRPOSQUERY => {
                            cc += 1;
                        }
                        OP_CRRANGE | OP_CRMINRANGE | OP_CRPOSRANGE => {
                            branchlength += get2(bl, cc + 1) as i32;
                            cc += 1 + 2 * IMM2_SIZE;
                        }
                        _ => branchlength += 1,
                    }
                }
                OP_DNREF | OP_DNREFI | OP_REF | OP_REFI => {
                    if op == OP_DNREF || op == OP_DNREFI {
                        if !dupcapused && (self.overall_options & PCRE2_MATCH_UNSET_BACKREF) == 0 {
                            let mut count = get2(bl, cc + 1 + IMM2_SIZE) as i32;
                            let mut slot =
                                get2(bl, cc + 1) as usize * self.name_entry_size as usize;
                            d = i32::MAX;
                            while count > 0 {
                                count -= 1;
                                let recno = get2(bl, slot) as usize;
                                let dd: i32;
                                if recno <= backref_cache[0] as usize && backref_cache[recno] >= 0 {
                                    dd = backref_cache[recno];
                                } else {
                                    let cs = match find_bracket(bl, startcode, utf, recno as i32) {
                                        None => return -2,
                                        Some(p) => p,
                                    };
                                    let mut ce = cs;
                                    loop {
                                        ce += get(bl, ce + 1);
                                        if self.b(ce) != OP_ALT {
                                            break;
                                        }
                                    }
                                    let mut d2 = 0;
                                    if !dupcapused
                                        || find_bracket(bl, ce, utf, recno as i32).is_none()
                                    {
                                        if cc > cs && cc < ce {
                                            had_recurse = true;
                                        } else if recurses.contains(&cs) {
                                            had_recurse = true;
                                        } else {
                                            recurses.push(cs);
                                            d2 = self.find_minlength(
                                                cs,
                                                startcode,
                                                utf,
                                                recurses,
                                                countptr,
                                                backref_cache,
                                            );
                                            recurses.pop();
                                            if d2 < 0 {
                                                return d2;
                                            }
                                        }
                                    }
                                    dd = d2;
                                    backref_cache[recno] = dd;
                                    let mut i = backref_cache[0] as usize + 1;
                                    while i < recno {
                                        backref_cache[i] = -1;
                                        i += 1;
                                    }
                                    backref_cache[0] = recno as i32;
                                }
                                if dd < d {
                                    d = dd;
                                }
                                if d <= 0 {
                                    break;
                                }
                                slot += self.name_entry_size as usize;
                            }
                        } else {
                            d = 0;
                        }
                        cc += 1 + 2 * IMM2_SIZE;
                    } else {
                        let recno = get2(bl, cc + 1) as usize;
                        if recno <= backref_cache[0] as usize && backref_cache[recno] >= 0 {
                            d = backref_cache[recno];
                        } else {
                            d = 0;
                            if (self.overall_options & PCRE2_MATCH_UNSET_BACKREF) == 0 {
                                let cs = match find_bracket(bl, startcode, utf, recno as i32) {
                                    None => return -2,
                                    Some(p) => p,
                                };
                                let mut ce = cs;
                                loop {
                                    ce += get(bl, ce + 1);
                                    if self.b(ce) != OP_ALT {
                                        break;
                                    }
                                }
                                if !dupcapused || find_bracket(bl, ce, utf, recno as i32).is_none()
                                {
                                    if cc > cs && cc < ce {
                                        had_recurse = true;
                                    } else if recurses.contains(&cs) {
                                        had_recurse = true;
                                    } else {
                                        recurses.push(cs);
                                        d = self.find_minlength(
                                            cs,
                                            startcode,
                                            utf,
                                            recurses,
                                            countptr,
                                            backref_cache,
                                        );
                                        recurses.pop();
                                        if d < 0 {
                                            return d;
                                        }
                                    }
                                }
                            }
                            backref_cache[recno] = d;
                            let mut i = backref_cache[0] as usize + 1;
                            while i < recno {
                                backref_cache[i] = -1;
                                i += 1;
                            }
                            backref_cache[0] = recno as i32;
                        }
                        cc += 1 + IMM2_SIZE;
                    }
                    // REPEAT_BACK_REFERENCE
                    let min: i32 = match self.b(cc) {
                        OP_CRSTAR | OP_CRMINSTAR | OP_CRQUERY | OP_CRMINQUERY | OP_CRPOSSTAR
                        | OP_CRPOSQUERY => {
                            cc += 1;
                            0
                        }
                        OP_CRPLUS | OP_CRMINPLUS | OP_CRPOSPLUS => {
                            cc += 1;
                            1
                        }
                        OP_CRRANGE | OP_CRMINRANGE | OP_CRPOSRANGE => {
                            let m = get2(bl, cc + 1) as i32;
                            cc += 1 + 2 * IMM2_SIZE;
                            m
                        }
                        _ => 1,
                    };
                    if (d > 0 && (i32::MAX / d) < min)
                        || (u16::MAX as i32 - branchlength) < min.wrapping_mul(d)
                    {
                        branchlength = u16::MAX as i32;
                    } else {
                        branchlength += min * d;
                    }
                }
                OP_RECURSE => {
                    let cs = startcode + get(bl, cc + 1);
                    let mut ce = cs;
                    let recno = get2(bl, cs + 1 + LINK_SIZE) as i32;
                    if recno == prev_recurse_recno {
                        branchlength += prev_recurse_d;
                    } else {
                        loop {
                            ce += get(bl, ce + 1);
                            if self.b(ce) != OP_ALT {
                                break;
                            }
                        }
                        if cc > cs && cc < ce {
                            had_recurse = true;
                        } else if recurses.contains(&cs) {
                            had_recurse = true;
                        } else {
                            recurses.push(cs);
                            prev_recurse_d = self.find_minlength(
                                cs,
                                startcode,
                                utf,
                                recurses,
                                countptr,
                                backref_cache,
                            );
                            recurses.pop();
                            if prev_recurse_d < 0 {
                                return prev_recurse_d;
                            }
                            prev_recurse_recno = recno;
                            branchlength += prev_recurse_d;
                        }
                    }
                    cc += 1 + LINK_SIZE + once_fudge;
                    once_fudge = 0;
                }
                OP_UPTO | OP_UPTOI | OP_NOTUPTO | OP_NOTUPTOI | OP_MINUPTO | OP_MINUPTOI
                | OP_NOTMINUPTO | OP_NOTMINUPTOI | OP_POSUPTO | OP_POSUPTOI | OP_NOTPOSUPTO
                | OP_NOTPOSUPTOI | OP_STAR | OP_STARI | OP_NOTSTAR | OP_NOTSTARI | OP_MINSTAR
                | OP_MINSTARI | OP_NOTMINSTAR | OP_NOTMINSTARI | OP_POSSTAR | OP_POSSTARI
                | OP_NOTPOSSTAR | OP_NOTPOSSTARI | OP_QUERY | OP_QUERYI | OP_NOTQUERY
                | OP_NOTQUERYI | OP_MINQUERY | OP_MINQUERYI | OP_NOTMINQUERY | OP_NOTMINQUERYI
                | OP_POSQUERY | OP_POSQUERYI | OP_NOTPOSQUERY | OP_NOTPOSQUERYI => {
                    cc += op_len(op as u8);
                    if utf && has_extralen(bl[cc - 1]) {
                        cc += get_extralen(bl[cc - 1]);
                    }
                }
                OP_MARK | OP_COMMIT_ARG | OP_PRUNE_ARG | OP_SKIP_ARG | OP_THEN_ARG => {
                    cc += op_len(op as u8) + bl[cc + 1] as usize;
                }
                OP_CLOSE | OP_COMMIT | OP_FAIL | OP_PRUNE | OP_SET_SOM | OP_SKIP | OP_THEN => {
                    cc += op_len(op as u8);
                }
                _ => return -3,
            }
        }
    }

    #[inline]
    fn set_bit(&mut self, c: u32) {
        self.start_bitmap[(c / 8) as usize] |= 1u8 << (c & 7);
    }

    fn set_table_bit(&mut self, mut p: usize, caseless: bool, utf: bool, ucp: bool) -> usize {
        let mut c = self.b(p);
        p += 1;
        self.set_bit(c);
        if utf && c >= 0xc0 {
            let (cc, l) = getutf8(&self.block, p - 1, c);
            c = cc;
            p += l - 1;
        }
        if caseless {
            if utf || ucp {
                c = ucd_othercase(c);
                if utf {
                    let mut buff = [0u8; 6];
                    ord2utf(c, &mut buff);
                    self.set_bit(buff[0] as u32);
                } else if c < 256 {
                    self.set_bit(c);
                }
            } else {
                let f = self.tables[256 + c as usize] as u32;
                self.set_bit(f);
            }
        }
        p
    }

    fn set_type_bits(&mut self, cbit_type: usize, table_limit: usize) {
        for c in 0..table_limit {
            self.start_bitmap[c] |= self.tables[c + 512 + cbit_type];
        }
        if table_limit == 32 {
            return;
        }
        for c in 128u32..256 {
            if (self.tables[512 + (c / 8) as usize] & (1u8 << (c & 7))) != 0 {
                let mut buff = [0u8; 6];
                ord2utf(c, &mut buff);
                self.set_bit(buff[0] as u32);
            }
        }
    }

    fn set_nottype_bits(&mut self, cbit_type: usize, table_limit: usize) {
        for c in 0..table_limit {
            self.start_bitmap[c] |= !self.tables[c + 512 + cbit_type];
        }
        if table_limit != 32 {
            for c in 24..32 {
                self.start_bitmap[c] = 0xff;
            }
        }
    }

    fn set_hspace_bits(&mut self, utf: bool) {
        self.set_bit(CHAR_HT);
        self.set_bit(CHAR_SPACE);
        if utf {
            self.set_bit(0xc2);
            self.set_bit(0xe1);
            self.set_bit(0xe2);
            self.set_bit(0xe3);
        } else {
            self.set_bit(0xa0);
        }
    }

    fn set_vspace_bits(&mut self, utf: bool) {
        self.set_bit(CHAR_LF);
        self.set_bit(CHAR_VT);
        self.set_bit(CHAR_FF);
        self.set_bit(CHAR_CR);
        if utf {
            self.set_bit(0xc2);
            self.set_bit(0xe2);
        } else {
            self.set_bit(CHAR_NEL);
        }
    }

    fn set_start_bits(&mut self, mut code: usize, utf: bool, ucp: bool, depthptr: &mut i32) -> i32 {
        let mut yield_ = SSB_DONE;
        let table_limit: usize = if utf { 16 } else { 32 };
        *depthptr += 1;
        if *depthptr > 1000 {
            return SSB_TOODEEP;
        }
        loop {
            let mut try_next = true;
            let mut tcode = code + 1 + LINK_SIZE;
            let op0 = self.b(code);
            if op0 == OP_CBRA || op0 == OP_SCBRA || op0 == OP_CBRAPOS || op0 == OP_SCBRAPOS {
                tcode += IMM2_SIZE;
            }
            while try_next {
                let mut classmap: Option<usize> = None;
                let op = self.b(tcode);
                // The ASSERT case may "continue" to the bracket handling.
                let mut do_bracket = false;
                match op {
                    OP_ACCEPT | OP_ASSERT_ACCEPT | OP_ALLANY | OP_ANY | OP_ANYBYTE | OP_CIRCM
                    | OP_CLOSE | OP_COMMIT | OP_COMMIT_ARG | OP_COND | OP_CREF | OP_FALSE
                    | OP_TRUE | OP_DNCREF | OP_DNREF | OP_DNREFI | OP_DNRREF | OP_DOLL
                    | OP_DOLLM | OP_END | OP_EOD | OP_EODN | OP_EXTUNI | OP_FAIL | OP_MARK
                    | OP_NOT | OP_NOTEXACT | OP_NOTEXACTI | OP_NOTI | OP_NOTMINPLUS
                    | OP_NOTMINPLUSI | OP_NOTMINQUERY | OP_NOTMINQUERYI | OP_NOTMINSTAR
                    | OP_NOTMINSTARI | OP_NOTMINUPTO | OP_NOTMINUPTOI | OP_NOTPLUS
                    | OP_NOTPLUSI | OP_NOTPOSPLUS | OP_NOTPOSPLUSI | OP_NOTPOSQUERY
                    | OP_NOTPOSQUERYI | OP_NOTPOSSTAR | OP_NOTPOSSTARI | OP_NOTPOSUPTO
                    | OP_NOTPOSUPTOI | OP_NOTPROP | OP_NOTQUERY | OP_NOTQUERYI | OP_NOTSTAR
                    | OP_NOTSTARI | OP_NOTUPTO | OP_NOTUPTOI | OP_NOT_HSPACE | OP_NOT_VSPACE
                    | OP_PRUNE | OP_PRUNE_ARG | OP_RECURSE | OP_REF | OP_REFI | OP_REVERSE
                    | OP_VREVERSE | OP_RREF | OP_SCOND | OP_SET_SOM | OP_SKIP | OP_SKIP_ARG
                    | OP_SOD | OP_SOM | OP_THEN | OP_THEN_ARG => return SSB_FAIL,
                    OP_CIRC => tcode += op_len(OP_CIRC as u8),
                    OP_PROP => {
                        if self.b(tcode + 1) != PT_CLIST {
                            return SSB_FAIL;
                        }
                        let mut p = self.b(tcode + 2) as usize;
                        loop {
                            let mut c = tables::UCD_CASELESS_SETS[p];
                            p += 1;
                            if c == NOTACHAR {
                                break;
                            }
                            if utf {
                                let mut buff = [0u8; 6];
                                ord2utf(c, &mut buff);
                                c = buff[0] as u32;
                            }
                            if c > 0xff {
                                self.set_bit(0xff);
                            } else {
                                self.set_bit(c);
                            }
                        }
                        try_next = false;
                    }
                    OP_WORD_BOUNDARY
                    | OP_NOT_WORD_BOUNDARY
                    | OP_UCP_WORD_BOUNDARY
                    | OP_NOT_UCP_WORD_BOUNDARY => {
                        tcode += 1;
                    }
                    OP_ASSERT | OP_ASSERT_NA => {
                        let mut ncode = tcode + get(&self.block, tcode + 1);
                        while self.b(ncode) == OP_ALT {
                            ncode += get(&self.block, ncode + 1);
                        }
                        ncode += 1 + LINK_SIZE;
                        loop {
                            match self.b(ncode) {
                                OP_ASSERT | OP_ASSERT_NOT | OP_ASSERTBACK | OP_ASSERTBACK_NOT
                                | OP_ASSERT_NA | OP_ASSERTBACK_NA => {
                                    ncode += get(&self.block, ncode + 1);
                                    while self.b(ncode) == OP_ALT {
                                        ncode += get(&self.block, ncode + 1);
                                    }
                                    ncode += 1 + LINK_SIZE;
                                }
                                OP_WORD_BOUNDARY
                                | OP_NOT_WORD_BOUNDARY
                                | OP_UCP_WORD_BOUNDARY
                                | OP_NOT_UCP_WORD_BOUNDARY => ncode += 1,
                                OP_CALLOUT => ncode += op_len(OP_CALLOUT as u8),
                                OP_CALLOUT_STR => {
                                    ncode += get(&self.block, ncode + 1 + 2 * LINK_SIZE)
                                }
                                _ => break,
                            }
                        }
                        let nop = self.b(ncode);
                        let jump = match nop {
                            OP_PROP => self.b(ncode + 1) == PT_CLIST,
                            OP_ANYNL | OP_CHAR | OP_CHARI | OP_EXACT | OP_EXACTI | OP_HSPACE
                            | OP_MINPLUS | OP_MINPLUSI | OP_PLUS | OP_PLUSI | OP_POSPLUS
                            | OP_POSPLUSI | OP_VSPACE | OP_DIGIT | OP_NOT_DIGIT | OP_WORDCHAR
                            | OP_NOT_WORDCHAR | OP_WHITESPACE | OP_NOT_WHITESPACE => true,
                            _ => false,
                        };
                        if jump {
                            tcode = ncode;
                            continue;
                        }
                        do_bracket = true;
                    }
                    OP_BRA | OP_SBRA | OP_CBRA | OP_SCBRA | OP_BRAPOS | OP_SBRAPOS | OP_CBRAPOS
                    | OP_SCBRAPOS | OP_ONCE | OP_SCRIPT_RUN => do_bracket = true,
                    OP_ALT => {
                        yield_ = SSB_CONTINUE;
                        try_next = false;
                    }
                    OP_KET | OP_KETRMAX | OP_KETRMIN | OP_KETRPOS => return SSB_CONTINUE,
                    OP_CALLOUT => tcode += op_len(OP_CALLOUT as u8),
                    OP_CALLOUT_STR => tcode += get(&self.block, tcode + 1 + 2 * LINK_SIZE),
                    OP_ASSERT_NOT | OP_ASSERTBACK | OP_ASSERTBACK_NOT | OP_ASSERTBACK_NA => {
                        loop {
                            tcode += get(&self.block, tcode + 1);
                            if self.b(tcode) != OP_ALT {
                                break;
                            }
                        }
                        tcode += 1 + LINK_SIZE;
                    }
                    OP_BRAZERO | OP_BRAMINZERO | OP_BRAPOSZERO => {
                        tcode += 1;
                        let rc = self.set_start_bits(tcode, utf, ucp, depthptr);
                        if rc == SSB_FAIL || rc == SSB_UNKNOWN || rc == SSB_TOODEEP {
                            return rc;
                        }
                        loop {
                            tcode += get(&self.block, tcode + 1);
                            if self.b(tcode) != OP_ALT {
                                break;
                            }
                        }
                        tcode += 1 + LINK_SIZE;
                    }
                    OP_SKIPZERO => {
                        tcode += 1;
                        loop {
                            tcode += get(&self.block, tcode + 1);
                            if self.b(tcode) != OP_ALT {
                                break;
                            }
                        }
                        tcode += 1 + LINK_SIZE;
                    }
                    OP_STAR | OP_MINSTAR | OP_POSSTAR | OP_QUERY | OP_MINQUERY | OP_POSQUERY => {
                        tcode = self.set_table_bit(tcode + 1, false, utf, ucp);
                    }
                    OP_STARI | OP_MINSTARI | OP_POSSTARI | OP_QUERYI | OP_MINQUERYI
                    | OP_POSQUERYI => {
                        tcode = self.set_table_bit(tcode + 1, true, utf, ucp);
                    }
                    OP_UPTO | OP_MINUPTO | OP_POSUPTO => {
                        tcode = self.set_table_bit(tcode + 1 + IMM2_SIZE, false, utf, ucp);
                    }
                    OP_UPTOI | OP_MINUPTOI | OP_POSUPTOI => {
                        tcode = self.set_table_bit(tcode + 1 + IMM2_SIZE, true, utf, ucp);
                    }
                    OP_EXACT | OP_CHAR | OP_PLUS | OP_MINPLUS | OP_POSPLUS => {
                        if op == OP_EXACT {
                            tcode += IMM2_SIZE;
                        }
                        self.set_table_bit(tcode + 1, false, utf, ucp);
                        try_next = false;
                    }
                    OP_EXACTI | OP_CHARI | OP_PLUSI | OP_MINPLUSI | OP_POSPLUSI => {
                        if op == OP_EXACTI {
                            tcode += IMM2_SIZE;
                        }
                        self.set_table_bit(tcode + 1, true, utf, ucp);
                        try_next = false;
                    }
                    OP_HSPACE => {
                        self.set_hspace_bits(utf);
                        try_next = false;
                    }
                    OP_ANYNL | OP_VSPACE => {
                        self.set_vspace_bits(utf);
                        try_next = false;
                    }
                    OP_NOT_DIGIT => {
                        self.set_nottype_bits(64, table_limit);
                        try_next = false;
                    }
                    OP_DIGIT => {
                        self.set_type_bits(64, table_limit);
                        try_next = false;
                    }
                    OP_NOT_WHITESPACE => {
                        self.set_nottype_bits(0, table_limit);
                        try_next = false;
                    }
                    OP_WHITESPACE => {
                        self.set_type_bits(0, table_limit);
                        try_next = false;
                    }
                    OP_NOT_WORDCHAR => {
                        self.set_nottype_bits(160, table_limit);
                        try_next = false;
                    }
                    OP_WORDCHAR => {
                        self.set_type_bits(160, table_limit);
                        try_next = false;
                    }
                    OP_TYPEPLUS | OP_TYPEMINPLUS | OP_TYPEPOSPLUS => tcode += 1,
                    OP_TYPEEXACT => tcode += 1 + IMM2_SIZE,
                    OP_TYPEUPTO | OP_TYPEMINUPTO | OP_TYPEPOSUPTO | OP_TYPESTAR
                    | OP_TYPEMINSTAR | OP_TYPEPOSSTAR | OP_TYPEQUERY | OP_TYPEMINQUERY
                    | OP_TYPEPOSQUERY => {
                        if op == OP_TYPEUPTO || op == OP_TYPEMINUPTO || op == OP_TYPEPOSUPTO {
                            tcode += IMM2_SIZE;
                        }
                        match self.b(tcode + 1) {
                            OP_HSPACE => self.set_hspace_bits(utf),
                            OP_ANYNL | OP_VSPACE => self.set_vspace_bits(utf),
                            OP_NOT_DIGIT => self.set_nottype_bits(64, table_limit),
                            OP_DIGIT => self.set_type_bits(64, table_limit),
                            OP_NOT_WHITESPACE => self.set_nottype_bits(0, table_limit),
                            OP_WHITESPACE => self.set_type_bits(0, table_limit),
                            OP_NOT_WORDCHAR => self.set_nottype_bits(160, table_limit),
                            OP_WORDCHAR => self.set_type_bits(160, table_limit),
                            _ => return SSB_FAIL,
                        }
                        tcode += 2;
                    }
                    OP_XCLASS | OP_NCLASS | OP_CLASS => {
                        let mut handle_classmap_directly = false;
                        if op == OP_XCLASS {
                            let xclassflags = self.b(tcode + 1 + LINK_SIZE);
                            if (xclassflags & XCL_HASPROP) != 0
                                || (xclassflags & (XCL_MAP | XCL_NOT)) == XCL_NOT
                            {
                                return SSB_FAIL;
                            }
                            classmap = if (xclassflags & XCL_MAP) == 0 {
                                None
                            } else {
                                Some(tcode + 1 + LINK_SIZE + 1)
                            };
                            if utf && (xclassflags & XCL_NOT) == 0 {
                                let mut p = tcode
                                    + 1
                                    + LINK_SIZE
                                    + 1
                                    + if classmap.is_none() { 0 } else { 32 };
                                tcode += get(&self.block, tcode + 1);
                                loop {
                                    let t = self.b(p);
                                    p += 1;
                                    match t {
                                        XCL_SINGLE => {
                                            let b = self.b(p);
                                            p += 1;
                                            while (self.b(p) & 0xc0) == 0x80 {
                                                p += 1;
                                            }
                                            self.set_bit(b);
                                        }
                                        XCL_RANGE => {
                                            let mut b = self.b(p);
                                            p += 1;
                                            while (self.b(p) & 0xc0) == 0x80 {
                                                p += 1;
                                            }
                                            let e = self.b(p);
                                            p += 1;
                                            while (self.b(p) & 0xc0) == 0x80 {
                                                p += 1;
                                            }
                                            while b <= e {
                                                self.set_bit(b);
                                                b += 1;
                                            }
                                        }
                                        XCL_END => break,
                                        _ => return SSB_UNKNOWN,
                                    }
                                }
                                handle_classmap_directly = true;
                            }
                        }
                        if !handle_classmap_directly {
                            if (op == OP_XCLASS || op == OP_NCLASS) && utf {
                                self.start_bitmap[24] |= 0xf0;
                                for i in 25..32 {
                                    self.start_bitmap[i] = 0xff;
                                }
                            }
                            if op == OP_XCLASS {
                                tcode += get(&self.block, tcode + 1);
                            } else {
                                tcode += 1;
                                classmap = Some(tcode);
                                tcode += 32;
                            }
                        }
                        // HANDLE_CLASSMAP
                        if let Some(cm) = classmap {
                            if utf {
                                for c in 0..16 {
                                    self.start_bitmap[c] |= self.block[cm + c];
                                }
                                let mut c: u32 = 128;
                                while c < 256 {
                                    if (self.block[cm + (c / 8) as usize] & (1u8 << (c & 7))) != 0 {
                                        let d = (c >> 6) | 0xc0;
                                        self.set_bit(d);
                                        c = (c & 0xc0) + 0x40 - 1;
                                    }
                                    c += 1;
                                }
                            } else {
                                for c in 0..32 {
                                    self.start_bitmap[c] |= self.block[cm + c];
                                }
                            }
                        }
                        match self.b(tcode) {
                            OP_CRSTAR | OP_CRMINSTAR | OP_CRQUERY | OP_CRMINQUERY
                            | OP_CRPOSSTAR | OP_CRPOSQUERY => {
                                tcode += 1;
                            }
                            OP_CRRANGE | OP_CRMINRANGE | OP_CRPOSRANGE => {
                                if get2(&self.block, tcode + 1) == 0 {
                                    tcode += 1 + 2 * IMM2_SIZE;
                                } else {
                                    try_next = false;
                                }
                            }
                            _ => try_next = false,
                        }
                    }
                    _ => return SSB_UNKNOWN,
                }
                if do_bracket {
                    let rc = self.set_start_bits(tcode, utf, ucp, depthptr);
                    if rc == SSB_DONE {
                        try_next = false;
                    } else if rc == SSB_CONTINUE {
                        loop {
                            tcode += get(&self.block, tcode + 1);
                            if self.b(tcode) != OP_ALT {
                                break;
                            }
                        }
                        tcode += 1 + LINK_SIZE;
                    } else {
                        return rc;
                    }
                }
            }
            code += get(&self.block, code + 1);
            if self.b(code) != OP_ALT {
                break;
            }
        }
        yield_
    }

    /// PRIV(study)(): returns 0 on success, or a non-zero internal error.
    pub(crate) fn study(&mut self) -> i32 {
        let mut count = 0;
        let utf = (self.overall_options & PCRE2_UTF) != 0;
        let ucp = (self.overall_options & PCRE2_UCP) != 0;
        let code = self.code_start;

        if (self.flags & (PCRE2_FIRSTSET | PCRE2_STARTLINE)) == 0 {
            let mut depth = 0;
            let rc = self.set_start_bits(code, utf, ucp, &mut depth);
            if rc == SSB_UNKNOWN {
                return 1;
            }
            if rc == SSB_DONE {
                let mut a: i32 = -1;
                let mut b: i32 = -1;
                let mut flags = PCRE2_FIRSTMAPSET;
                'done: {
                    for i in 0..32usize {
                        let x = self.start_bitmap[i];
                        if x != 0 {
                            let y = x & (!x).wrapping_add(1);
                            if y != x {
                                break 'done;
                            }
                            let c = (i * 8) as i32 + x.trailing_zeros() as i32;
                            if utf && c > 127 {
                                break 'done;
                            }
                            if a < 0 {
                                a = c;
                            } else if b < 0 {
                                let mut d = self.tables[256 + c as usize] as i32;
                                if utf || ucp {
                                    if ucd_caseset(c as u32) != 0 {
                                        break 'done;
                                    }
                                    if c > 127 {
                                        d = ucd_othercase(c as u32) as i32;
                                    }
                                }
                                if d != a {
                                    break 'done;
                                }
                                b = c;
                            } else {
                                break 'done;
                            }
                        }
                    }
                    if a >= 0
                        && ((self.flags & PCRE2_LASTSET) == 0
                            || (self.last_codeunit != a as u32
                                && (b < 0 || self.last_codeunit != b as u32)))
                    {
                        self.first_codeunit = a as u32;
                        flags = PCRE2_FIRSTSET;
                        if b >= 0 {
                            flags |= PCRE2_FIRSTCASELESS;
                        }
                    }
                }
                self.flags |= flags;
            }
        }

        if (self.flags & (PCRE2_MATCH_EMPTY | PCRE2_HASACCEPT)) == 0
            && self.top_backref as usize <= MAX_CACHE_BACKREF
        {
            let mut backref_cache = [0i32; MAX_CACHE_BACKREF + 1];
            let mut recurses = Vec::new();
            let min = self.find_minlength(
                code,
                code,
                utf,
                &mut recurses,
                &mut count,
                &mut backref_cache,
            );
            match min {
                -1 => {}
                -2 => return 2,
                -3 => return 3,
                _ => {
                    self.minlength = if min > u16::MAX as i32 {
                        u16::MAX
                    } else {
                        min as u16
                    }
                }
            }
        }
        0
    }
}
