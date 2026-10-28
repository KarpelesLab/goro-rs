//! Regular expression engine compatible with PCRE2 syntax and semantics, for
//! ext/pcre.
//!
//! Written from PCRE2's published documentation (pcre2pattern, pcre2syntax,
//! pcre2api, pcre2unicode), the PHP manual and the observable behaviour of
//! PHP 8.5 (which bundles PCRE2 10.46); see CLEANROOM.md.
//!
//! ```
//! use goro_regex::{Regex, Flags};
//! let re = Regex::new(br"(?<year>\d{4})-(\d\d)", Flags::empty()).unwrap();
//! let mut m = re.matcher();
//! assert!(m.exec(b"on 2024-05", 0).unwrap());
//! assert_eq!(m.group(0), Some((3, 10)));
//! assert_eq!(re.group_index(b"year"), Some(1));
//! ```

pub mod ast;
pub mod class;
pub mod compile;
pub mod error;
pub mod exec;
pub mod parse;
pub mod php;
pub mod prog;
pub mod unicode;
pub mod utf8;

pub use ast::Newline;
pub use error::{CompileError, ErrorKind, MatchError, Utf8Error};
pub use exec::{Limits, MatchOptions, Scratch};

use std::sync::Arc;

/// Compile options (pcre2api: pcre2_compile() options; PHP's pattern
/// modifiers map onto these).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flags(pub u32);

impl Flags {
    pub const CASELESS: Flags = Flags(1 << 0);
    pub const MULTILINE: Flags = Flags(1 << 1);
    pub const DOTALL: Flags = Flags(1 << 2);
    pub const EXTENDED: Flags = Flags(1 << 3);
    pub const EXTENDED_MORE: Flags = Flags(1 << 4);
    pub const NO_AUTO_CAPTURE: Flags = Flags(1 << 5);
    pub const UNGREEDY: Flags = Flags(1 << 6);
    pub const ANCHORED: Flags = Flags(1 << 7);
    pub const DOLLAR_ENDONLY: Flags = Flags(1 << 8);
    pub const DUPNAMES: Flags = Flags(1 << 9);
    pub const UTF: Flags = Flags(1 << 10);
    pub const UCP: Flags = Flags(1 << 11);
    pub const CASELESS_RESTRICT: Flags = Flags(1 << 12);
    pub const NO_UTF_CHECK: Flags = Flags(1 << 13);
    pub const ALLOW_EMPTY_CLASS: Flags = Flags(1 << 14);
    pub const MATCH_UNSET_BACKREF: Flags = Flags(1 << 15);
    pub const NO_START_OPTIMIZE: Flags = Flags(1 << 16);
    pub const NO_AUTO_POSSESS: Flags = Flags(1 << 17);
    pub const NO_DOTSTAR_ANCHOR: Flags = Flags(1 << 18);

    pub const fn empty() -> Flags {
        Flags(0)
    }

    pub const fn contains(self, o: Flags) -> bool {
        self.0 & o.0 == o.0
    }

    pub const fn union(self, o: Flags) -> Flags {
        Flags(self.0 | o.0)
    }
}

impl std::ops::BitOr for Flags {
    type Output = Flags;
    fn bitor(self, o: Flags) -> Flags {
        Flags(self.0 | o.0)
    }
}

impl std::ops::BitOrAssign for Flags {
    fn bitor_assign(&mut self, o: Flags) {
        self.0 |= o.0;
    }
}

/// Full compile configuration.
#[derive(Clone, Debug)]
pub struct CompileConfig {
    pub flags: Flags,
    /// The default newline convention (PHP's PCRE2 is built with LF).
    pub newline: Newline,
    /// Whether `\R` matches only CR, LF and CRLF by default.
    pub bsr_anycrlf: bool,
}

impl Default for CompileConfig {
    fn default() -> CompileConfig {
        CompileConfig {
            flags: Flags::empty(),
            newline: Newline::Lf,
            bsr_anycrlf: false,
        }
    }
}

/// A compiled pattern.
#[derive(Debug)]
pub struct Regex {
    prog: prog::Prog,
    /// (name, group) pairs, in definition order.
    names: Vec<(Arc<[u8]>, u32)>,
}

/// A successful match: the overall match and the capture groups, as byte
/// offsets into the subject.
#[derive(Clone, Debug, Default)]
pub struct Captures {
    /// 2 entries per group (start, end); `usize::MAX` when unset.
    slots: Vec<usize>,
    mark: Option<Arc<[u8]>>,
}

impl Captures {
    /// The span of group `i` (0 = whole match), if it was set.
    #[inline]
    pub fn get(&self, i: usize) -> Option<(usize, usize)> {
        let s = *self.slots.get(2 * i)?;
        if s == usize::MAX {
            return None;
        }
        Some((s, self.slots[2 * i + 1]))
    }

    /// Number of slots pairs (capture_count + 1).
    pub fn len(&self) -> usize {
        self.slots.len() / 2
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// pcre2_match()'s positive return value: one more than the highest
    /// numbered group that was set. PHP uses it to drop trailing unset
    /// groups.
    pub fn highest_set(&self) -> usize {
        let n = self.len();
        for i in (0..n).rev() {
            if self.slots[2 * i] != usize::MAX {
                return i + 1;
            }
        }
        0
    }

    /// The (*MARK) name on the matching path (PHP's "MARK" entry).
    pub fn mark(&self) -> Option<&[u8]> {
        self.mark.as_deref()
    }

    /// The raw slots.
    pub fn slots(&self) -> &[usize] {
        &self.slots
    }
}

impl Regex {
    /// Compiles `pattern` with `flags` and the default newline convention
    /// (LF).
    pub fn new(pattern: &[u8], flags: Flags) -> Result<Regex, CompileError> {
        Regex::with_config(
            pattern,
            &CompileConfig {
                flags,
                ..CompileConfig::default()
            },
        )
    }

    pub fn with_config(pattern: &[u8], cfg: &CompileConfig) -> Result<Regex, CompileError> {
        let pcfg = parse::ParseConfig {
            flags: cfg.flags,
            newline: cfg.newline,
            bsr_anycrlf: cfg.bsr_anycrlf,
        };
        let parsed = parse::parse(pattern, &pcfg)?;
        let c = compile::compile(parsed, cfg.flags)?;
        Ok(Regex {
            prog: c.prog,
            names: c.names,
        })
    }

    /// Number of capture groups (not counting group 0).
    pub fn capture_count(&self) -> usize {
        self.prog.ncap as usize
    }

    /// Whether the pattern is in UTF mode (the `u` modifier or `(*UTF)`).
    pub fn is_utf(&self) -> bool {
        self.prog.utf
    }

    /// (name, group number) pairs in order of definition; with duplicate
    /// names a name may appear several times.
    pub fn names(&self) -> &[(Arc<[u8]>, u32)] {
        &self.names
    }

    /// For each group number (index), the group's name if it has one.
    pub fn group_names(&self) -> Vec<Option<Arc<[u8]>>> {
        let mut v = vec![None; self.prog.ncap as usize + 1];
        for (n, g) in &self.names {
            if v[*g as usize].is_none() {
                v[*g as usize] = Some(n.clone());
            }
        }
        v
    }

    /// The number of the first group with this name.
    pub fn group_index(&self, name: &[u8]) -> Option<usize> {
        self.names
            .iter()
            .find(|(n, _)| n.as_ref() == name)
            .map(|(_, g)| *g as usize)
    }

    /// A reusable matcher (holds the scratch space and the last result).
    pub fn matcher(&self) -> Matcher<'_> {
        Matcher {
            re: self,
            scratch: Scratch::new(),
            caps: Captures::default(),
            limits: Limits::default(),
            opts: MatchOptions::default(),
        }
    }

    /// Low-level match: on success fills `caps` and returns true.
    pub fn exec_with(
        &self,
        scratch: &mut Scratch,
        subject: &[u8],
        start: usize,
        opts: MatchOptions,
        limits: Limits,
        caps: &mut Captures,
    ) -> Result<bool, MatchError> {
        match exec::exec(
            &self.prog,
            scratch,
            subject,
            start,
            opts,
            limits,
            &mut caps.slots,
        )? {
            Some(m) => {
                caps.mark = m.mark;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Convenience: does the pattern match anywhere in `subject`?
    pub fn is_match(&self, subject: &[u8]) -> Result<bool, MatchError> {
        self.matcher().exec(subject, 0)
    }

    pub fn prog(&self) -> &prog::Prog {
        &self.prog
    }
}

/// Matching state bound to a [`Regex`].
pub struct Matcher<'r> {
    re: &'r Regex,
    pub scratch: Scratch,
    pub caps: Captures,
    pub limits: Limits,
    pub opts: MatchOptions,
}

impl<'r> Matcher<'r> {
    pub fn regex(&self) -> &'r Regex {
        self.re
    }

    /// Matches at or after `start` with the matcher's default options.
    pub fn exec(&mut self, subject: &[u8], start: usize) -> Result<bool, MatchError> {
        let opts = self.opts;
        self.exec_opts(subject, start, opts)
    }

    pub fn exec_opts(
        &mut self,
        subject: &[u8],
        start: usize,
        opts: MatchOptions,
    ) -> Result<bool, MatchError> {
        self.re.exec_with(
            &mut self.scratch,
            subject,
            start,
            opts,
            self.limits,
            &mut self.caps,
        )
    }

    /// Group `i` of the last successful match.
    pub fn group(&self, i: usize) -> Option<(usize, usize)> {
        self.caps.get(i)
    }

    pub fn captures(&self) -> &Captures {
        &self.caps
    }
}
