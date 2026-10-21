//! Port of pcre2_auto_possess.c: automatically convert repeats to their
//! possessive forms when what follows cannot match the repeated item.

use crate::cb::Cb;
use crate::consts::*;
use crate::tables;
use crate::util::*;

const FIRST_AUTOTAB_OP: u32 = OP_NOT_DIGIT;
const LAST_AUTOTAB_LEFT_OP: u32 = OP_EXTUNI;
const LAST_AUTOTAB_RIGHT_OP: u32 = OP_DOLLM;

#[rustfmt::skip]
static AUTOPOSSTAB: [[u8; 21]; 17] = [
  [ 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0 ],
  [ 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1 ],
  [ 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1 ],
  [ 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0 ],
  [ 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0 ],
  [ 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1 ],
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0 ],
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0 ],
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0 ],
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 ],
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 ],
  [ 0, 1, 0, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0 ],
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0 ],
  [ 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0 ],
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0 ],
  [ 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0 ],
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0 ],
];

#[rustfmt::skip]
static PROPPOSSTAB: [[u8; 14]; 14] = [
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 ],
  [ 0, 3, 0, 0, 0, 0, 3, 1, 1, 0, 0, 0, 0, 0 ],
  [ 0, 0, 2, 4, 0, 0, 9, 10, 10, 11, 0, 0, 0, 0 ],
  [ 0, 0, 5, 2, 0, 0, 15, 16, 16, 17, 0, 0, 0, 0 ],
  [ 0, 0, 0, 0, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0 ],
  [ 0, 0, 0, 0, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0 ],
  [ 0, 3, 6, 12, 0, 0, 3, 1, 1, 0, 0, 0, 0, 0 ],
  [ 0, 1, 7, 13, 0, 0, 1, 3, 3, 1, 0, 0, 0, 0 ],
  [ 0, 1, 7, 13, 0, 0, 1, 3, 3, 1, 0, 0, 0, 0 ],
  [ 0, 0, 8, 14, 0, 0, 0, 1, 1, 3, 0, 0, 0, 0 ],
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 ],
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0 ],
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 ],
  [ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 ],
];

#[rustfmt::skip]
static CATPOSSTAB: [[u8; 30]; 7] = [
  [ 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1 ],
  [ 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1 ],
  [ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1 ],
  [ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1 ],
  [ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1 ],
  [ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1 ],
  [ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0 ],
];

static POSSPROPSTAB: [[u32; 4]; 3] = [
    [ucp_L, ucp_N, ucp_N, ucp_Nl],
    [ucp_Z, ucp_Z, ucp_C, ucp_Cc],
    [ucp_L, ucp_N, ucp_P, ucp_Po],
];

fn check_char_prop(c: u32, ptype: u32, pdata: u32, negated: bool) -> bool {
    let prop = get_ucd(c);
    let gentype = tables::UCP_GENTYPE[prop.chartype as usize];
    let ct = prop.chartype as u32;
    match ptype {
        PT_LAMP => (ct == ucp_Lu || ct == ucp_Ll || ct == ucp_Lt) == negated,
        PT_GC => (pdata == gentype) == negated,
        PT_PC => (pdata == ct) == negated,
        PT_SC => (pdata == prop.script as u32) == negated,
        PT_SCX => {
            let ok = pdata == prop.script as u32
                || mapbit(
                    &tables::UCD_SCRIPT_SETS[ucd_scriptx_prop(prop) as usize..],
                    pdata,
                );
            ok == negated
        }
        PT_ALNUM => (gentype == ucp_L || gentype == ucp_N) == negated,
        PT_SPACE | PT_PXSPACE => {
            if is_hspace(c) || is_vspace(c) {
                negated
            } else {
                (gentype == ucp_Z) == negated
            }
        }
        PT_WORD => (gentype == ucp_L || gentype == ucp_N || c == b'_' as u32) == negated,
        PT_CLIST => {
            let mut p = prop.caseset as usize;
            loop {
                let v = tables::UCD_CASELESS_SETS[p];
                if c < v {
                    return !negated;
                }
                p += 1;
                if c == v {
                    return negated;
                }
            }
        }
        _ => false,
    }
}

fn get_repeat_base(c: u32) -> u32 {
    if c > OP_TYPEPOSUPTO {
        c
    } else if c >= OP_TYPESTAR {
        OP_TYPESTAR
    } else if c >= OP_NOTSTARI {
        OP_NOTSTARI
    } else if c >= OP_NOTSTAR {
        OP_NOTSTAR
    } else if c >= OP_STARI {
        OP_STARI
    } else {
        OP_STAR
    }
}

impl<'a> Cb<'a> {
    /// get_chr_property_list(): returns the position after the item, or
    /// None if the item cannot be handled.
    fn get_chr_property_list(
        &self,
        mut code: usize,
        utf: bool,
        ucp: bool,
        list: &mut [u32; 8],
    ) -> Option<usize> {
        let mut c = self.code[code] as u32;
        list[0] = c;
        list[1] = 0;
        code += 1;
        if (OP_STAR..=OP_TYPEPOSUPTO).contains(&c) {
            let base = get_repeat_base(c);
            c -= base - OP_STAR;
            if c == OP_UPTO || c == OP_MINUPTO || c == OP_EXACT || c == OP_POSUPTO {
                code += IMM2_SIZE;
            }
            list[1] = (c != OP_PLUS && c != OP_MINPLUS && c != OP_EXACT && c != OP_POSPLUS) as u32;
            match base {
                OP_STAR => list[0] = OP_CHAR,
                OP_STARI => list[0] = OP_CHARI,
                OP_NOTSTAR => list[0] = OP_NOT,
                OP_NOTSTARI => list[0] = OP_NOTI,
                OP_TYPESTAR => {
                    list[0] = self.code[code] as u32;
                    code += 1;
                }
                _ => {}
            }
            c = list[0];
        }
        match c {
            OP_NOT_DIGIT | OP_DIGIT | OP_NOT_WHITESPACE | OP_WHITESPACE | OP_NOT_WORDCHAR
            | OP_WORDCHAR | OP_ANY | OP_ALLANY | OP_ANYNL | OP_NOT_HSPACE | OP_HSPACE
            | OP_NOT_VSPACE | OP_VSPACE | OP_EXTUNI | OP_EODN | OP_EOD | OP_DOLL | OP_DOLLM => {
                Some(code)
            }
            OP_CHAR | OP_NOT => {
                let (chr, l) = getchar_test(&self.code, code, utf);
                code += l;
                list[2] = chr;
                list[3] = NOTACHAR;
                Some(code)
            }
            OP_CHARI | OP_NOTI => {
                list[0] = if c == OP_CHARI { OP_CHAR } else { OP_NOT };
                let (chr, l) = getchar_test(&self.code, code, utf);
                code += l;
                list[2] = chr;
                if chr < 128 || (chr < 256 && !utf && !ucp) {
                    list[3] = self.fcc(chr);
                } else {
                    list[3] = ucd_othercase(chr);
                }
                if chr == list[3] {
                    list[3] = NOTACHAR;
                } else {
                    list[4] = NOTACHAR;
                }
                Some(code)
            }
            OP_PROP | OP_NOTPROP => {
                if self.code[code] as u32 != PT_CLIST {
                    list[2] = self.code[code] as u32;
                    list[3] = self.code[code + 1] as u32;
                    return Some(code + 2);
                }
                let mut src = self.code[code + 1] as usize;
                let mut dest = 2usize;
                code += 2;
                loop {
                    if dest >= 8 {
                        list[2] = self.code[code] as u32;
                        list[3] = self.code[code + 1] as u32;
                        return Some(code);
                    }
                    let v = tables::UCD_CASELESS_SETS[src];
                    list[dest] = v;
                    dest += 1;
                    src += 1;
                    if v == NOTACHAR {
                        break;
                    }
                }
                list[0] = if c == OP_PROP { OP_CHAR } else { OP_NOT };
                Some(code)
            }
            OP_NCLASS | OP_CLASS | OP_XCLASS => {
                let mut end = if c == OP_XCLASS {
                    code + get(&self.code, code) - 1
                } else {
                    code + 32
                };
                match self.code[end] as u32 {
                    OP_CRSTAR | OP_CRMINSTAR | OP_CRQUERY | OP_CRMINQUERY | OP_CRPOSSTAR
                    | OP_CRPOSQUERY => {
                        list[1] = 1;
                        end += 1;
                    }
                    OP_CRPLUS | OP_CRMINPLUS | OP_CRPOSPLUS => end += 1,
                    OP_CRRANGE | OP_CRMINRANGE | OP_CRPOSRANGE => {
                        list[1] = (get2(&self.code, end + 1) == 0) as u32;
                        end += 1 + 2 * IMM2_SIZE;
                    }
                    _ => {}
                }
                list[2] = (end - code) as u32;
                Some(end)
            }
            _ => None,
        }
    }

    /// compare_opcodes()
    #[allow(clippy::too_many_arguments)]
    fn compare_opcodes(
        &self,
        mut code: usize,
        utf: bool,
        ucp: bool,
        base_list: &[u32; 8],
        base_end: usize,
        rec_limit: &mut i32,
    ) -> bool {
        let mut list = [0u32; 8];
        let mut entered_a_group = false;
        *rec_limit -= 1;
        if *rec_limit <= 0 {
            return false;
        }
        loop {
            let mut c = self.code[code] as u32;
            if c == OP_CALLOUT {
                code += op_len(c as u8);
                continue;
            }
            if c == OP_CALLOUT_STR {
                code += get(&self.code, code + 1 + 2 * LINK_SIZE);
                continue;
            }
            if c == OP_ALT {
                loop {
                    code += get(&self.code, code + 1);
                    if self.code[code] as u32 != OP_ALT {
                        break;
                    }
                }
                c = self.code[code] as u32;
            }
            match c {
                OP_END => return base_list[1] != 0,
                OP_KET | OP_KETRPOS => {
                    if base_list[1] == 0 {
                        return false;
                    }
                    let bracode = code - get(&self.code, code + 1);
                    match self.code[bracode] as u32 {
                        OP_CBRA | OP_SCBRA | OP_CBRAPOS | OP_SCBRAPOS => {
                            if self.had_recurse {
                                return false;
                            }
                        }
                        OP_SCRIPT_RUN => {
                            if base_list[0] != OP_CHAR && base_list[0] != OP_CHARI {
                                return false;
                            }
                        }
                        OP_ASSERT | OP_ASSERT_NOT | OP_ONCE => return !entered_a_group,
                        OP_ASSERTBACK | OP_ASSERTBACK_NOT => {
                            return if self.code[bracode + 1 + LINK_SIZE] as u32 == OP_VREVERSE {
                                false
                            } else {
                                !entered_a_group
                            };
                        }
                        OP_ASSERT_NA | OP_ASSERTBACK_NA => return false,
                        _ => {}
                    }
                    code += op_len(c as u8);
                    continue;
                }
                OP_ONCE | OP_BRA | OP_CBRA => {
                    let mut next_code = code + get(&self.code, code + 1);
                    code += op_len(c as u8);
                    while self.code[next_code] as u32 == OP_ALT {
                        if !self.compare_opcodes(code, utf, ucp, base_list, base_end, rec_limit) {
                            return false;
                        }
                        code = next_code + 1 + LINK_SIZE;
                        next_code += get(&self.code, next_code + 1);
                    }
                    entered_a_group = true;
                    continue;
                }
                OP_BRAZERO | OP_BRAMINZERO => {
                    let mut next_code = code + 1;
                    let nc = self.code[next_code] as u32;
                    if nc != OP_BRA && nc != OP_CBRA && nc != OP_ONCE {
                        return false;
                    }
                    loop {
                        next_code += get(&self.code, next_code + 1);
                        if self.code[next_code] as u32 != OP_ALT {
                            break;
                        }
                    }
                    next_code += 1 + LINK_SIZE;
                    if !self.compare_opcodes(next_code, utf, ucp, base_list, base_end, rec_limit) {
                        return false;
                    }
                    code += op_len(c as u8);
                    continue;
                }
                _ => {}
            }

            code = match self.get_chr_property_list(code, utf, ucp, &mut list) {
                None => return false,
                Some(p) => p,
            };

            // Which list holds characters?
            let chr_list: &[u32];
            let list_is_list: bool;
            let lp: &[u32; 8];
            if base_list[0] == OP_CHAR {
                chr_list = &base_list[2..];
                lp = &list;
                list_is_list = true;
            } else if list[0] == OP_CHAR {
                chr_list = &list[2..];
                lp = base_list;
                list_is_list = false;
            } else if base_list[0] == OP_CLASS
                || list[0] == OP_CLASS
                || (!utf && (base_list[0] == OP_NCLASS || list[0] == OP_NCLASS))
            {
                let set1: usize;
                let lp2: &[u32; 8];
                let lp2_is_list: bool;
                if base_list[0] == OP_CLASS || (!utf && base_list[0] == OP_NCLASS) {
                    set1 = base_end - base_list[2] as usize;
                    lp2 = &list;
                    lp2_is_list = true;
                } else {
                    set1 = code - list[2] as usize;
                    lp2 = base_list;
                    lp2_is_list = false;
                }
                let mut invert_bits = false;
                // set2 is either in the code or in the cbits table.
                let set2: Result<usize, usize>; // Ok(code index) / Err(cbits offset)
                match lp2[0] {
                    OP_CLASS | OP_NCLASS => {
                        set2 = Ok((if lp2_is_list { code } else { base_end }) - lp2[2] as usize);
                    }
                    OP_XCLASS => {
                        let xclass_flags = (if lp2_is_list { code } else { base_end })
                            - lp2[2] as usize
                            + LINK_SIZE;
                        let f = self.code[xclass_flags] as u32;
                        if (f & XCL_HASPROP) != 0 {
                            return false;
                        }
                        if (f & XCL_MAP) == 0 {
                            if list[1] == 0 {
                                return (f & XCL_NOT) == 0;
                            }
                            continue;
                        }
                        set2 = Ok(xclass_flags + 1);
                    }
                    OP_NOT_DIGIT => {
                        invert_bits = true;
                        set2 = Err(64);
                    }
                    OP_DIGIT => set2 = Err(64),
                    OP_NOT_WHITESPACE => {
                        invert_bits = true;
                        set2 = Err(0);
                    }
                    OP_WHITESPACE => set2 = Err(0),
                    OP_NOT_WORDCHAR => {
                        invert_bits = true;
                        set2 = Err(160);
                    }
                    OP_WORDCHAR => set2 = Err(160),
                    _ => return false,
                }
                for i in 0..32 {
                    let a = self.code[set1 + i];
                    let b = match set2 {
                        Ok(p) => self.code[p + i],
                        Err(o) => self.cbits(o + i),
                    };
                    let v = if invert_bits { a & !b } else { a & b };
                    if v != 0 {
                        return false;
                    }
                }
                if list[1] == 0 {
                    return true;
                }
                continue;
            } else {
                let leftop = base_list[0];
                let rightop = list[0];
                let mut accepted = false;
                if leftop == OP_PROP || leftop == OP_NOTPROP {
                    if rightop == OP_EOD {
                        accepted = true;
                    } else if rightop == OP_PROP || rightop == OP_NOTPROP {
                        let same = leftop == rightop;
                        let lisprop = leftop == OP_PROP;
                        let risprop = rightop == OP_PROP;
                        let bothprop = lisprop && risprop;
                        let n = PROPPOSSTAB[base_list[2] as usize][list[2] as usize];
                        let cat = |a: u32, b: u32| CATPOSSTAB[a as usize][b as usize] != 0;
                        accepted = match n {
                            0 => false,
                            1 => bothprop,
                            2 => (base_list[3] == list[3]) != same,
                            3 => !same,
                            4 => risprop && cat(base_list[3], list[3]) == same,
                            5 => lisprop && cat(list[3], base_list[3]) == same,
                            6..=8 => {
                                let p = &POSSPROPSTAB[(n - 6) as usize];
                                risprop
                                    && lisprop
                                        == (list[3] != p[0]
                                            && list[3] != p[1]
                                            && (list[3] != p[2] || !lisprop))
                            }
                            9..=11 => {
                                let p = &POSSPROPSTAB[(n - 9) as usize];
                                lisprop
                                    && risprop
                                        == (base_list[3] != p[0]
                                            && base_list[3] != p[1]
                                            && (base_list[3] != p[2] || !risprop))
                            }
                            12..=14 => {
                                let p = &POSSPROPSTAB[(n - 12) as usize];
                                risprop
                                    && lisprop
                                        == (cat(p[0], list[3])
                                            && cat(p[1], list[3])
                                            && (list[3] != p[3] || !lisprop))
                            }
                            15..=17 => {
                                let p = &POSSPROPSTAB[(n - 15) as usize];
                                lisprop
                                    && risprop
                                        == (cat(p[0], base_list[3])
                                            && cat(p[1], base_list[3])
                                            && (base_list[3] != p[3] || !risprop))
                            }
                            _ => false,
                        };
                    }
                } else {
                    accepted = (FIRST_AUTOTAB_OP..=LAST_AUTOTAB_LEFT_OP).contains(&leftop)
                        && (FIRST_AUTOTAB_OP..=LAST_AUTOTAB_RIGHT_OP).contains(&rightop)
                        && AUTOPOSSTAB[(leftop - FIRST_AUTOTAB_OP) as usize]
                            [(rightop - FIRST_AUTOTAB_OP) as usize]
                            != 0;
                }
                if !accepted {
                    return false;
                }
                if list[1] == 0 {
                    return true;
                }
                continue;
            }

            // Check each character in chr_list against lp.
            let mut ci = 0;
            loop {
                let chr = chr_list[ci];
                match lp[0] {
                    OP_CHAR => {
                        let mut oi = 2;
                        loop {
                            if chr == lp[oi] {
                                return false;
                            }
                            oi += 1;
                            if lp[oi] == NOTACHAR {
                                break;
                            }
                        }
                    }
                    OP_NOT => {
                        let mut oi = 2;
                        loop {
                            if chr == lp[oi] {
                                break;
                            }
                            oi += 1;
                            if lp[oi] == NOTACHAR {
                                break;
                            }
                        }
                        if lp[oi] == NOTACHAR {
                            return false;
                        }
                    }
                    OP_DIGIT => {
                        if chr < 256 && (self.ctypes(chr) & ctype_digit) != 0 {
                            return false;
                        }
                    }
                    OP_NOT_DIGIT => {
                        if chr > 255 || (self.ctypes(chr) & ctype_digit) == 0 {
                            return false;
                        }
                    }
                    OP_WHITESPACE => {
                        if chr < 256 && (self.ctypes(chr) & ctype_space) != 0 {
                            return false;
                        }
                    }
                    OP_NOT_WHITESPACE => {
                        if chr > 255 || (self.ctypes(chr) & ctype_space) == 0 {
                            return false;
                        }
                    }
                    OP_WORDCHAR => {
                        if chr < 255 && (self.ctypes(chr) & ctype_word) != 0 {
                            return false;
                        }
                    }
                    OP_NOT_WORDCHAR => {
                        if chr > 255 || (self.ctypes(chr) & ctype_word) == 0 {
                            return false;
                        }
                    }
                    OP_HSPACE => {
                        if is_hspace(chr) {
                            return false;
                        }
                    }
                    OP_NOT_HSPACE => {
                        if !is_hspace(chr) {
                            return false;
                        }
                    }
                    OP_ANYNL | OP_VSPACE => {
                        if is_vspace(chr) {
                            return false;
                        }
                    }
                    OP_NOT_VSPACE => {
                        if !is_vspace(chr) {
                            return false;
                        }
                    }
                    OP_DOLL | OP_EODN => {
                        if matches!(chr, 0x0d | 0x0a | 0x0b | 0x0c | 0x85 | 0x2028 | 0x2029) {
                            return false;
                        }
                    }
                    OP_EOD => {}
                    OP_PROP | OP_NOTPROP => {
                        if !check_char_prop(chr, lp[2], lp[3], lp[0] == OP_NOTPROP) {
                            return false;
                        }
                    }
                    OP_NCLASS | OP_CLASS => {
                        if lp[0] == OP_NCLASS && chr > 255 {
                            return false;
                        }
                        if chr <= 255 {
                            let cb_ = (if list_is_list { code } else { base_end }) - lp[2] as usize;
                            if (self.code[cb_ + (chr >> 3) as usize] & (1u8 << (chr & 7))) != 0 {
                                return false;
                            }
                        }
                    }
                    OP_XCLASS => {
                        let d = (if list_is_list { code } else { base_end }) - lp[2] as usize
                            + LINK_SIZE;
                        if xclass(chr, &self.code, d, utf) {
                            return false;
                        }
                    }
                    _ => return false,
                }
                ci += 1;
                if chr_list[ci] == NOTACHAR {
                    break;
                }
            }
            if list[1] == 0 {
                return true;
            }
        }
    }

    /// PRIV(auto_possessify)(): returns 0 on success, -1 on bad code.
    pub fn auto_possessify(&mut self, mut code: usize) -> i32 {
        let mut list = [0u32; 8];
        let mut rec_limit = 1000;
        let utf = (self.external_options & PCRE2_UTF) != 0;
        let ucp = (self.external_options & PCRE2_UCP) != 0;
        loop {
            let mut c = self.code[code] as u32;
            if c >= OP_TABLE_LENGTH {
                return -1;
            }
            if (OP_STAR..=OP_TYPEPOSUPTO).contains(&c) {
                c -= get_repeat_base(c) - OP_STAR;
                let end = if c <= OP_MINUPTO {
                    self.get_chr_property_list(code, utf, ucp, &mut list)
                } else {
                    None
                };
                list[1] = (c == OP_STAR || c == OP_PLUS || c == OP_QUERY || c == OP_UPTO) as u32;
                if let Some(e) = end {
                    if self.compare_opcodes(e, utf, ucp, &list, e, &mut rec_limit) {
                        let delta = match c {
                            OP_STAR => OP_POSSTAR - OP_STAR,
                            OP_MINSTAR => OP_POSSTAR - OP_MINSTAR,
                            OP_PLUS => OP_POSPLUS - OP_PLUS,
                            OP_MINPLUS => OP_POSPLUS - OP_MINPLUS,
                            OP_QUERY => OP_POSQUERY - OP_QUERY,
                            OP_MINQUERY => OP_POSQUERY - OP_MINQUERY,
                            OP_UPTO => OP_POSUPTO - OP_UPTO,
                            OP_MINUPTO => OP_POSUPTO - OP_MINUPTO,
                            _ => 0,
                        };
                        self.code[code] = self.code[code].wrapping_add(delta as u8);
                    }
                }
                c = self.code[code] as u32;
            } else if c == OP_CLASS || c == OP_NCLASS || c == OP_XCLASS {
                let repeat_opcode = if c == OP_XCLASS {
                    code + get(&self.code, code + 1)
                } else {
                    code + 1 + 32
                };
                c = self.code[repeat_opcode] as u32;
                if (OP_CRSTAR..=OP_CRMINRANGE).contains(&c) {
                    let end = self.get_chr_property_list(code, utf, ucp, &mut list);
                    list[1] = ((c & 1) == 0) as u32;
                    if let Some(e) = end {
                        if self.compare_opcodes(e, utf, ucp, &list, e, &mut rec_limit) {
                            let nv = match c {
                                OP_CRSTAR | OP_CRMINSTAR => OP_CRPOSSTAR,
                                OP_CRPLUS | OP_CRMINPLUS => OP_CRPOSPLUS,
                                OP_CRQUERY | OP_CRMINQUERY => OP_CRPOSQUERY,
                                _ => OP_CRPOSRANGE,
                            };
                            self.code[repeat_opcode] = nv as u8;
                        }
                    }
                }
                c = self.code[code] as u32;
            }

            match c {
                OP_END => return 0,
                OP_TYPESTAR | OP_TYPEMINSTAR | OP_TYPEPLUS | OP_TYPEMINPLUS | OP_TYPEQUERY
                | OP_TYPEMINQUERY | OP_TYPEPOSSTAR | OP_TYPEPOSPLUS | OP_TYPEPOSQUERY => {
                    let n = self.code[code + 1] as u32;
                    if n == OP_PROP || n == OP_NOTPROP {
                        code += 2;
                    }
                }
                OP_TYPEUPTO | OP_TYPEMINUPTO | OP_TYPEEXACT | OP_TYPEPOSUPTO => {
                    let n = self.code[code + 1 + IMM2_SIZE] as u32;
                    if n == OP_PROP || n == OP_NOTPROP {
                        code += 2;
                    }
                }
                OP_CALLOUT_STR => code += get(&self.code, code + 1 + 2 * LINK_SIZE),
                OP_XCLASS => code += get(&self.code, code + 1),
                OP_MARK | OP_COMMIT_ARG | OP_PRUNE_ARG | OP_SKIP_ARG | OP_THEN_ARG => {
                    code += self.code[code + 1] as usize;
                }
                _ => {}
            }
            code += op_len(c as u8);
            if utf && is_char_repeat_op(c) && has_extralen(self.code[code - 1]) {
                code += get_extralen(self.code[code - 1]);
            }
        }
    }
}
