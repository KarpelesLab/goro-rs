//! Lookbehind length checking on the parsed pattern (pcre2_compile.c
//! parsed_skip, get_grouplength, get_branchlength, set_lookbehind_lengths,
//! check_lookbehinds).

use crate::cb::Cb;
use crate::consts::*;

const PSKIP_ALT: u32 = 0;
const PSKIP_CLASS: u32 = 1;
const PSKIP_KET: u32 = 2;

impl<'a> Cb<'a> {
    #[inline]
    fn gp(&self, i: usize) -> u32 {
        *self.parsed.get(i).unwrap_or(&META_END)
    }
    #[inline]
    fn readplusoffset(&self, i: usize) -> usize {
        ((self.gp(i + 1) as usize) << 32) | self.gp(i + 2) as usize
    }

    /// parsed_skip()
    pub fn parsed_skip(&self, mut pptr: usize, skiptype: u32) -> Option<usize> {
        let mut nestlevel = 0u32;
        loop {
            let v = self.gp(pptr);
            let meta = meta_code(v);
            let mut skip_extra = true;
            match meta {
                META_END => return None,
                META_BACKREF => {
                    if meta_data(v) >= 10 {
                        pptr += SIZEOFFSET;
                    }
                }
                META_ESCAPE => match meta_data(v) {
                    ESC_P | ESC_p => pptr += 1,
                    ESC_g | ESC_k => pptr += 1 + SIZEOFFSET,
                    _ => {}
                },
                META_MARK | META_COMMIT_ARG | META_PRUNE_ARG | META_SKIP_ARG | META_THEN_ARG => {
                    pptr += self.gp(pptr + 1) as usize;
                }
                META_CLASS_END => {
                    if skiptype == PSKIP_CLASS {
                        return Some(pptr);
                    }
                }
                META_ATOMIC | META_CAPTURE | META_COND_ASSERT | META_COND_DEFINE
                | META_COND_NAME | META_COND_NUMBER | META_COND_RNAME | META_COND_RNUMBER
                | META_COND_VERSION | META_LOOKAHEAD | META_LOOKAHEADNOT | META_LOOKAHEAD_NA
                | META_LOOKBEHIND | META_LOOKBEHINDNOT | META_LOOKBEHIND_NA | META_NOCAPTURE
                | META_SCRIPT_RUN => {
                    nestlevel += 1;
                }
                META_ALT => {
                    if nestlevel == 0 && skiptype == PSKIP_ALT {
                        return Some(pptr);
                    }
                }
                META_KET => {
                    if nestlevel == 0 {
                        return Some(pptr);
                    }
                    nestlevel -= 1;
                }
                _ => {
                    if v < META_END {
                        skip_extra = false;
                    }
                }
            }
            if skip_extra {
                let m = ((meta >> 16) & 0x7fff) as usize;
                if m >= META_EXTRA_LENGTHS.len() {
                    return None;
                }
                pptr += META_EXTRA_LENGTHS[m] as usize;
            }
            pptr += 1;
        }
    }

    /// get_grouplength()
    #[allow(clippy::too_many_arguments)]
    fn get_grouplength(
        &mut self,
        pptrptr: &mut usize,
        minptr: &mut i32,
        isinline: bool,
        errcodeptr: &mut i32,
        lcptr: &mut i32,
        group: u32,
        recurses: &mut Vec<usize>,
    ) -> i32 {
        let gi = 2 * group as usize;
        let mut grouplength: i32 = -1;
        let mut groupminlength: i32 = i32::MAX;
        if group > 0 && (self.external_flags & PCRE2_DUPCAPUSED) == 0 {
            let groupinfo = self.groupinfo[gi];
            if (groupinfo & GI_NOT_FIXED_LENGTH) != 0 {
                return -1;
            }
            if (groupinfo & GI_SET_FIXED_LENGTH) != 0 {
                if isinline {
                    match self.parsed_skip(*pptrptr, PSKIP_KET) {
                        Some(p) => *pptrptr = p,
                        None => {
                            // C would store NULL; callers then fault. Treat
                            // as an internal error.
                            *errcodeptr = ERR90;
                            return -1;
                        }
                    }
                }
                *minptr = self.groupinfo[gi + 1] as i32;
                return (groupinfo & GI_FIXED_LENGTH_MASK) as i32;
            }
        }
        loop {
            let mut branchminlength: i32 = 0;
            let branchlength =
                self.get_branchlength(pptrptr, &mut branchminlength, errcodeptr, lcptr, recurses);
            if branchlength < 0 {
                if group > 0 {
                    self.groupinfo[gi] |= GI_NOT_FIXED_LENGTH;
                }
                return -1;
            }
            if branchlength > grouplength {
                grouplength = branchlength;
            }
            if branchminlength < groupminlength {
                groupminlength = branchminlength;
            }
            if self.gp(*pptrptr) == META_KET {
                break;
            }
            *pptrptr += 1;
        }
        if group > 0 {
            self.groupinfo[gi] |= GI_SET_FIXED_LENGTH | grouplength as u32;
            self.groupinfo[gi + 1] = groupminlength as u32;
        }
        *minptr = groupminlength;
        grouplength
    }

    /// get_branchlength()
    fn get_branchlength(
        &mut self,
        pptrptr: &mut usize,
        minptr: &mut i32,
        errcodeptr: &mut i32,
        lcptr: &mut i32,
        recurses: &mut Vec<usize>,
    ) -> i32 {
        let mut branchlength: i32 = 0;
        let mut branchminlength: i32 = 0;
        let mut lastitemlength: u32 = 0;
        let mut lastitemminlength: u32 = 0;
        let mut pptr = *pptrptr;
        let mut offset: usize;

        let lc = *lcptr;
        *lcptr += 1;
        if lc > 2000 {
            *errcodeptr = ERR35;
            return -1;
        }

        loop {
            let mut itemlength: u32 = 0;
            let mut itemminlength: u32 = 0;
            let mut group: u32 = 0;
            let v = self.gp(pptr);

            // Labels modelled as flags.
            let mut isnotfixed = false;
            let mut recurse_or_backref = false;
            let mut check_group = false;
            let mut repetition: Option<(u32, u32)> = None;

            if v < META_END {
                itemlength = 1;
                itemminlength = 1;
            } else {
                match meta_code(v) {
                    META_KET | META_ALT => {
                        *pptrptr = pptr;
                        *minptr = branchminlength;
                        return branchlength;
                    }
                    META_ACCEPT | META_FAIL => {
                        match self.parsed_skip(pptr, PSKIP_ALT) {
                            None => {
                                *errcodeptr = ERR90;
                                return -1;
                            }
                            Some(p) => pptr = p,
                        }
                        *pptrptr = pptr;
                        *minptr = branchminlength;
                        return branchlength;
                    }
                    META_MARK | META_COMMIT_ARG | META_PRUNE_ARG | META_SKIP_ARG
                    | META_THEN_ARG => {
                        pptr += self.gp(pptr + 1) as usize + 1;
                    }
                    META_CIRCUMFLEX | META_COMMIT | META_DOLLAR | META_PRUNE | META_SKIP
                    | META_THEN => {}
                    META_OPTIONS => pptr += 2,
                    META_BIGVALUE => {
                        itemlength = 1;
                        itemminlength = 1;
                        pptr += 1;
                    }
                    META_CLASS | META_CLASS_NOT => {
                        itemlength = 1;
                        itemminlength = 1;
                        match self.parsed_skip(pptr, PSKIP_CLASS) {
                            None => {
                                *errcodeptr = ERR90;
                                return -1;
                            }
                            Some(p) => pptr = p,
                        }
                    }
                    META_CLASS_EMPTY_NOT | META_DOT => {
                        itemlength = 1;
                        itemminlength = 1;
                    }
                    META_CALLOUT_NUMBER => pptr += 3,
                    META_CALLOUT_STRING => pptr += 3 + SIZEOFFSET,
                    META_ESCAPE => {
                        let escape = meta_data(v);
                        if escape == ESC_X {
                            return -1;
                        }
                        if escape == ESC_R {
                            itemminlength = 1;
                            itemlength = 2;
                        } else if escape > ESC_b && escape < ESC_Z {
                            if (self.external_options & PCRE2_UTF) != 0 && escape == ESC_C {
                                *errcodeptr = ERR36;
                                return -1;
                            }
                            itemlength = 1;
                            itemminlength = 1;
                            if escape == ESC_p || escape == ESC_P {
                                pptr += 1;
                            }
                        }
                    }
                    META_LOOKAHEAD | META_LOOKAHEADNOT | META_LOOKAHEAD_NA => {
                        let mut rp = pptr;
                        *errcodeptr =
                            self.check_lookbehinds(pptr + 1, Some(&mut rp), recurses, lcptr);
                        if *errcodeptr != 0 {
                            return -1;
                        }
                        pptr = rp;
                        match self.gp(pptr + 1) {
                            META_ASTERISK | META_ASTERISK_PLUS | META_ASTERISK_QUERY
                            | META_PLUS | META_PLUS_PLUS | META_PLUS_QUERY | META_QUERY
                            | META_QUERY_PLUS | META_QUERY_QUERY => pptr += 1,
                            META_MINMAX | META_MINMAX_PLUS | META_MINMAX_QUERY => pptr += 3,
                            _ => {}
                        }
                    }
                    META_LOOKBEHIND | META_LOOKBEHINDNOT | META_LOOKBEHIND_NA => {
                        if !self.set_lookbehind_lengths(&mut pptr, errcodeptr, lcptr, recurses) {
                            return -1;
                        }
                    }
                    META_BACKREF_BYNAME | META_RECURSE_BYNAME => {
                        let mc = meta_code(v);
                        if mc == META_BACKREF_BYNAME
                            && (self.external_options & PCRE2_MATCH_UNSET_BACKREF) != 0
                        {
                            isnotfixed = true;
                        } else {
                            pptr += 1;
                            let length = self.gp(pptr) as usize;
                            offset = self.readplusoffset(pptr);
                            pptr += 2;
                            let name = offset;
                            let mut is_dupname = false;
                            for i in 0..self.names_found as usize {
                                let ng = self.named_groups[i];
                                if length == ng.length as usize
                                    && self.pat[name..name + length]
                                        == self.pat[ng.name..ng.name + length]
                                {
                                    group = ng.number;
                                    is_dupname = ng.isdup;
                                    break;
                                }
                            }
                            if group == 0 {
                                *errcodeptr = ERR15;
                                self.erroroffset = offset;
                                return -1;
                            }
                            if mc == META_RECURSE_BYNAME
                                || (!is_dupname && (self.external_flags & PCRE2_DUPCAPUSED) == 0)
                            {
                                recurse_or_backref = true;
                            } else {
                                isnotfixed = true;
                            }
                        }
                        // offset is used by RECURSE_OR_BACKREF_LENGTH
                        if recurse_or_backref {
                            offset = self.readplusoffset(pptr - 2);
                            let _ = offset;
                        }
                    }
                    META_BACKREF => {
                        if (self.external_options & PCRE2_MATCH_UNSET_BACKREF) != 0
                            || (self.external_flags & PCRE2_DUPCAPUSED) != 0
                        {
                            isnotfixed = true;
                        } else {
                            group = meta_data(v);
                            if group < 10 {
                                recurse_or_backref = true;
                            } else {
                                // Fall through to META_RECURSE handling.
                                pptr += 2;
                                recurse_or_backref = true;
                            }
                        }
                    }
                    META_RECURSE => {
                        group = meta_data(v);
                        pptr += 2;
                        recurse_or_backref = true;
                    }
                    META_COND_DEFINE => {
                        pptr = self.parsed_skip(pptr + 1, PSKIP_KET).unwrap_or(pptr);
                    }
                    META_COND_NAME | META_COND_NUMBER | META_COND_RNAME | META_COND_RNUMBER => {
                        pptr += 2 + SIZEOFFSET;
                        check_group = true;
                    }
                    META_COND_ASSERT => {
                        pptr += 1;
                        check_group = true;
                    }
                    META_COND_VERSION => {
                        pptr += 4;
                        check_group = true;
                    }
                    META_CAPTURE | META_ATOMIC | META_NOCAPTURE | META_SCRIPT_RUN => {
                        if meta_code(v) == META_CAPTURE {
                            group = meta_data(v);
                        }
                        pptr += 1;
                        check_group = true;
                    }
                    META_QUERY | META_QUERY_PLUS | META_QUERY_QUERY => {
                        repetition = Some((0, 1));
                    }
                    META_MINMAX | META_MINMAX_PLUS | META_MINMAX_QUERY => {
                        let min = self.gp(pptr + 1);
                        let max = self.gp(pptr + 2);
                        pptr += 2;
                        repetition = Some((min, max));
                    }
                    _ => isnotfixed = true,
                }
            }

            if let Some((min, max)) = repetition {
                if max != REPEAT_UNLIMITED {
                    if lastitemlength != 0
                        && max != 0
                        && ((i32::MAX - branchlength) as u32) / lastitemlength < max - 1
                    {
                        *errcodeptr = ERR87;
                        return -1;
                    }
                    if min == 0 {
                        branchminlength -= lastitemminlength as i32;
                    } else {
                        itemminlength = (min - 1) * lastitemminlength;
                    }
                    if max == 0 {
                        branchlength -= lastitemlength as i32;
                    } else {
                        itemlength = (max - 1) * lastitemlength;
                    }
                } else {
                    isnotfixed = true;
                }
            }

            if recurse_or_backref {
                // RECURSE_OR_BACKREF_LENGTH
                let off = if meta_code(v) == META_BACKREF && meta_data(v) < 10 {
                    self.small_ref_offset[meta_data(v) as usize]
                } else {
                    self.readplusoffset(pptr - 2)
                };
                if group > self.bracount {
                    self.erroroffset = off;
                    *errcodeptr = ERR15;
                    return -1;
                }
                if group == 0 {
                    isnotfixed = true;
                } else {
                    let mut gptr = 0usize;
                    loop {
                        let g = self.gp(gptr);
                        if g == META_END {
                            break;
                        }
                        if meta_code(g) == META_BIGVALUE {
                            gptr += 1;
                        } else if g == (META_CAPTURE | group) {
                            break;
                        }
                        gptr += 1;
                    }
                    let gptrend = match self.parsed_skip(gptr + 1, PSKIP_KET) {
                        None => {
                            *errcodeptr = ERR90;
                            return -1;
                        }
                        Some(p) => p,
                    };
                    if pptr > gptr && pptr < gptrend {
                        isnotfixed = true;
                    } else if recurses.contains(&gptr) {
                        isnotfixed = true;
                    } else {
                        recurses.push(gptr);
                        let mut gp2 = gptr + 1;
                        let mut groupminlength: i32 = 0;
                        let grouplength = self.get_grouplength(
                            &mut gp2,
                            &mut groupminlength,
                            false,
                            errcodeptr,
                            lcptr,
                            group,
                            recurses,
                        );
                        recurses.pop();
                        if grouplength < 0 {
                            if *errcodeptr == 0 {
                                isnotfixed = true;
                            } else {
                                return -1;
                            }
                        } else {
                            itemlength = grouplength as u32;
                            itemminlength = groupminlength as u32;
                        }
                    }
                }
            }

            if check_group && !isnotfixed {
                let mut groupminlength: i32 = 0;
                let grouplength = self.get_grouplength(
                    &mut pptr,
                    &mut groupminlength,
                    true,
                    errcodeptr,
                    lcptr,
                    group,
                    recurses,
                );
                if grouplength < 0 {
                    return -1;
                }
                itemlength = grouplength as u32;
                itemminlength = groupminlength as u32;
            }

            if isnotfixed {
                *errcodeptr = ERR25;
                return -1;
            }

            if i32::MAX - branchlength < itemlength as i32 || {
                branchlength += itemlength as i32;
                branchlength > LOOKBEHIND_MAX as i32
            } {
                *errcodeptr = ERR87;
                return -1;
            }
            branchminlength += itemminlength as i32;
            lastitemlength = itemlength;
            lastitemminlength = itemminlength;
            pptr += 1;
        }
    }

    /// set_lookbehind_lengths()
    fn set_lookbehind_lengths(
        &mut self,
        pptrptr: &mut usize,
        errcodeptr: &mut i32,
        lcptr: &mut i32,
        recurses: &mut Vec<usize>,
    ) -> bool {
        let mut bptr = *pptrptr;
        let gbptr = bptr;
        let mut maxlength: i32 = 0;
        let mut minlength: i32 = i32::MAX;
        let mut variable = false;
        let offset = self.readplusoffset(bptr);
        *pptrptr += SIZEOFFSET;
        loop {
            let mut branchminlength: i32 = 0;
            *pptrptr += 1;
            let branchlength =
                self.get_branchlength(pptrptr, &mut branchminlength, errcodeptr, lcptr, recurses);
            if branchlength < 0 {
                if *errcodeptr == 0 {
                    *errcodeptr = ERR25;
                }
                if self.erroroffset == PCRE2_UNSET {
                    self.erroroffset = offset;
                }
                return false;
            }
            if branchlength != branchminlength {
                variable = true;
            }
            if branchminlength < minlength {
                minlength = branchminlength;
            }
            if branchlength > maxlength {
                maxlength = branchlength;
            }
            if branchlength > self.max_lookbehind {
                self.max_lookbehind = branchlength;
            }
            self.parsed[bptr] |= branchlength as u32;
            bptr = *pptrptr;
            if meta_code(self.gp(bptr)) != META_ALT {
                break;
            }
        }
        if variable && maxlength as u32 > self.max_varlookbehind {
            *errcodeptr = ERR100;
            self.erroroffset = offset;
            return false;
        }
        self.parsed[gbptr + 1] = if variable {
            minlength as u32
        } else {
            LOOKBEHIND_MAX
        };
        true
    }

    /// check_lookbehinds()
    pub fn check_lookbehinds(
        &mut self,
        mut pptr: usize,
        retptr: Option<&mut usize>,
        recurses: &mut Vec<usize>,
        lcptr: &mut i32,
    ) -> i32 {
        let mut errorcode = 0;
        let mut nestlevel: i32 = 0;
        self.erroroffset = PCRE2_UNSET;
        let mut retptr = retptr;
        loop {
            let v = self.gp(pptr);
            if v == META_END {
                break;
            }
            if v < META_END {
                pptr += 1;
                continue;
            }
            match meta_code(v) {
                META_ESCAPE => {
                    let e = v - META_ESCAPE;
                    if e == ESC_P || e == ESC_p {
                        pptr += 1;
                    }
                }
                META_KET => {
                    nestlevel -= 1;
                    if nestlevel < 0 {
                        if let Some(r) = retptr.as_deref_mut() {
                            *r = pptr;
                        }
                        return 0;
                    }
                }
                META_ATOMIC | META_CAPTURE | META_COND_ASSERT | META_LOOKAHEAD
                | META_LOOKAHEADNOT | META_LOOKAHEAD_NA | META_NOCAPTURE | META_SCRIPT_RUN => {
                    nestlevel += 1
                }
                META_ACCEPT | META_ALT | META_ASTERISK | META_ASTERISK_PLUS
                | META_ASTERISK_QUERY | META_BACKREF | META_CIRCUMFLEX | META_CLASS
                | META_CLASS_EMPTY | META_CLASS_EMPTY_NOT | META_CLASS_END | META_CLASS_NOT
                | META_COMMIT | META_DOLLAR | META_DOT | META_FAIL | META_PLUS | META_PLUS_PLUS
                | META_PLUS_QUERY | META_PRUNE | META_QUERY | META_QUERY_PLUS
                | META_QUERY_QUERY | META_RANGE_ESCAPED | META_RANGE_LITERAL | META_SKIP
                | META_THEN => {}
                META_RECURSE => pptr += SIZEOFFSET,
                META_BACKREF_BYNAME | META_RECURSE_BYNAME => pptr += 1 + SIZEOFFSET,
                META_COND_DEFINE => {
                    pptr += SIZEOFFSET;
                    nestlevel += 1;
                }
                META_COND_NAME | META_COND_NUMBER | META_COND_RNAME | META_COND_RNUMBER => {
                    pptr += 1 + SIZEOFFSET;
                    nestlevel += 1;
                }
                META_COND_VERSION => {
                    pptr += 3;
                    nestlevel += 1;
                }
                META_CALLOUT_STRING => pptr += 3 + SIZEOFFSET,
                META_BIGVALUE | META_POSIX | META_POSIX_NEG => pptr += 1,
                META_MINMAX | META_MINMAX_QUERY | META_MINMAX_PLUS | META_OPTIONS => pptr += 2,
                META_CALLOUT_NUMBER => pptr += 3,
                META_MARK | META_COMMIT_ARG | META_PRUNE_ARG | META_SKIP_ARG | META_THEN_ARG => {
                    pptr += 1 + self.gp(pptr + 1) as usize;
                }
                META_LOOKBEHIND | META_LOOKBEHINDNOT | META_LOOKBEHIND_NA => {
                    if !self.set_lookbehind_lengths(&mut pptr, &mut errorcode, lcptr, recurses) {
                        return errorcode;
                    }
                }
                _ => return ERR70,
            }
            pptr += 1;
        }
        0
    }
}
