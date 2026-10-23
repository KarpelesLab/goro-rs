//! A pure-Rust port of Oniguruma 6.9.10 (the regex library behind PHP's
//! mb_ereg* functions), restricted to what PHP uses.
//!
//! The code mirrors the C sources branch for branch (including branches
//! that happen to be identical and C-style late initialisation), so a few
//! style lints are silenced for the whole port.
#![allow(
    clippy::if_same_then_else,
    clippy::needless_late_init,
    clippy::only_used_in_recursion,
    clippy::upper_case_acronyms,
    clippy::enum_variant_names,
    clippy::manual_range_contains
)]

pub mod enc;
pub mod enc_tables;
#[cfg(test)]
mod enc_tests;
pub mod parse;
pub mod regcomp;
pub mod regexec;
pub mod regint;
pub mod unicode_data;

pub use enc::{EncKind, Encoding};
pub use regexec::{MatchParam, ONIG_REGION_NOTPOS, Region};
pub use regint::{
    ONIG_MISMATCH, ONIG_OPTION_EXTEND, ONIG_OPTION_FIND_LONGEST, ONIG_OPTION_FIND_NOT_EMPTY,
    ONIG_OPTION_IGNORECASE, ONIG_OPTION_MULTILINE, ONIG_OPTION_NONE, ONIG_OPTION_SINGLELINE,
    SyntaxType,
};

use regint::*;

/// The syntaxes PHP's mbregex can select
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Syntax {
    Java,
    GnuRegex,
    Grep,
    Emacs,
    Ruby,
    Perl,
    PosixBasic,
    PosixExtended,
}

impl Syntax {
    pub fn table(self) -> &'static SyntaxType {
        match self {
            Syntax::Java => &ONIG_SYNTAX_JAVA,
            Syntax::GnuRegex => &ONIG_SYNTAX_GNU_REGEX,
            Syntax::Grep => &ONIG_SYNTAX_GREP,
            Syntax::Emacs => &ONIG_SYNTAX_EMACS,
            Syntax::Ruby => &ONIG_SYNTAX_RUBY,
            Syntax::Perl => &ONIG_SYNTAX_PERL,
            Syntax::PosixBasic => &ONIG_SYNTAX_POSIX_BASIC,
            Syntax::PosixExtended => &ONIG_SYNTAX_POSIX_EXTENDED,
        }
    }
}

/// A compiled pattern (regex_t)
pub struct Regex {
    pub reg: regcomp::Regex,
    pub syntax: Syntax,
    pub kind: EncKind,
}

impl Regex {
    /// onig_new; the error is the message onig_error_code_to_str() makes
    pub fn new(
        pattern: &[u8],
        options: u32,
        kind: EncKind,
        syntax: Syntax,
    ) -> Result<Regex, Vec<u8>> {
        let enc = Encoding::get(kind);
        match regcomp::onig_new(pattern, options, enc, syntax.table()) {
            Ok(reg) => Ok(Regex { reg, syntax, kind }),
            Err(e) => Err(regcomp::error_code_to_str(e.code, enc, e.par.as_deref())),
        }
    }

    /// onig_get_options
    pub fn options(&self) -> u32 {
        self.reg.options
    }

    /// onig_number_of_names
    pub fn number_of_names(&self) -> usize {
        self.reg.names.entries.len()
    }

    /// (name, group numbers) in onig_foreach_name order
    pub fn names_foreach(&self) -> Vec<(&[u8], &[i32])> {
        self.reg
            .names
            .foreach_order()
            .into_iter()
            .map(|i| {
                let e = &self.reg.names.entries[i];
                (e.name.as_slice(), e.back_refs.as_slice())
            })
            .collect()
    }

    /// onig_name_to_backref_number (None for C's negative error codes)
    pub fn name_to_backref_number(&self, name: &[u8], region: Option<&Region>) -> Option<usize> {
        let i = self.reg.names.find(name)?;
        let nums = &self.reg.names.entries[i].back_refs;
        match nums.len() {
            0 => None,
            1 => Some(nums[0] as usize),
            n => {
                if let Some(r) = region {
                    for i in (0..n).rev() {
                        if r.beg
                            .get(nums[i] as usize)
                            .copied()
                            .unwrap_or(ONIG_REGION_NOTPOS)
                            != ONIG_REGION_NOTPOS
                        {
                            return Some(nums[i] as usize);
                        }
                    }
                }
                Some(nums[n - 1] as usize)
            }
        }
    }

    /// onig_noname_group_capture_is_active
    pub fn noname_group_capture_is_active(&self) -> bool {
        if opton_dont_capture_group(self.reg.options) {
            return false;
        }
        if self.number_of_names() > 0
            && self.reg.syntax.bv(ONIG_SYN_CAPTURE_ONLY_NAMED_GROUP)
            && !opton_capture_group(self.reg.options)
        {
            return false;
        }
        true
    }

    /// onig_search_with_param over s[..] from `start` to `range`;
    /// returns the match position, ONIG_MISMATCH or an error code
    pub fn search(
        &self,
        s: &[u8],
        start: usize,
        range: usize,
        region: Option<&mut Region>,
        mp: &MatchParam,
    ) -> i32 {
        regexec::onig_search(&self.reg, s, s.len(), start, range, region, 0, mp)
    }

    /// onig_match_with_param at `at`; returns the match length,
    /// ONIG_MISMATCH or an error code
    pub fn match_at(
        &self,
        s: &[u8],
        at: usize,
        region: Option<&mut Region>,
        mp: &MatchParam,
    ) -> i32 {
        regexec::onig_match(&self.reg, s, s.len(), at, region, 0, mp)
    }
}

/// onig_error_code_to_str without an error info parameter
pub fn error_code_to_str(code: i32) -> Vec<u8> {
    regcomp::error_code_to_str(code, Encoding::get(EncKind::Ascii), None)
}
