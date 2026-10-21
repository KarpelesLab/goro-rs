//! The compile block (pcre2_intmodedep.h compile_block) shared by the
//! parser and the code generator.

use crate::consts::*;

#[derive(Clone, Copy, Debug)]
pub struct NamedGroup {
    /// Offset of the name in the pattern.
    pub name: usize,
    pub number: u32,
    pub length: u16,
    pub isdup: bool,
}

pub struct Cb<'a> {
    pub pat: &'a [u8],
    pub tables: &'static [u8],
    /// The code buffer: the workspace in the length-computing pass, the
    /// final name table + code in the real pass.
    pub code: Vec<u8>,
    pub start_code: usize,
    pub start_workspace: usize,
    pub workspace_size: usize,
    pub name_table: usize,
    pub small_ref_offset: [usize; 10],
    pub erroroffset: usize,
    pub names_found: u16,
    pub name_entry_size: u16,
    pub parens_depth: u16,
    pub assert_depth: u16,
    pub named_groups: Vec<NamedGroup>,
    pub external_options: u32,
    pub external_flags: u32,
    pub bracount: u32,
    pub lastcapture: u32,
    pub parsed: Vec<u32>,
    pub parsed_end: usize,
    pub groupinfo: Vec<u32>,
    pub top_backref: u32,
    pub backref_map: u32,
    pub nltype: u32,
    pub nllen: u32,
    pub nl: [u8; 4],
    pub class_range_start: u32,
    pub class_range_end: u32,
    pub req_varyopt: u32,
    pub max_varlookbehind: u32,
    pub max_lookbehind: i32,
    pub had_accept: bool,
    pub had_pruneorskip: bool,
    pub had_recurse: bool,
    pub dupnames: bool,
    pub parens_nest_limit: u32,
    pub cx_extra_options: u32,
    /// Stack of open capturing groups: (number, assert_depth).
    pub open_caps: Vec<(u16, u16)>,
}

impl<'a> Cb<'a> {
    #[inline]
    pub fn fcc(&self, c: u32) -> u32 {
        self.tables[256 + c as usize] as u32
    }
    #[inline]
    pub fn cbits(&self, i: usize) -> u8 {
        self.tables[512 + i]
    }
    #[inline]
    pub fn ctypes(&self, c: u32) -> u32 {
        self.tables[CTYPES_OFFSET + c as usize] as u32
    }

    /// IS_NEWLINE(p) for the pattern (NLBLOCK = cb, PSEND = end_pattern).
    pub fn is_newline_pat(&mut self, p: usize, utf: bool) -> bool {
        let end = self.pat.len();
        if self.nltype != NLTYPE_FIXED {
            if p < end {
                if let Some(l) = crate::util::is_newline(self.pat, p, self.nltype, end, utf) {
                    self.nllen = l as u32;
                    return true;
                }
            }
            false
        } else {
            let nllen = self.nllen as usize;
            p + nllen <= end
                && self.pat[p] == self.nl[0]
                && (nllen == 1 || self.pat[p + 1] == self.nl[1])
        }
    }
}
