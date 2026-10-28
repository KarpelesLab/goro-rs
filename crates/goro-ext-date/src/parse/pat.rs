//! A tiny pattern matcher for the date/time grammar.
//!
//! A pattern maps a start position to the set of positions where a match
//! can end, so alternatives and optional parts behave like a regular
//! expression and the scanner can take the longest match, as the PHP
//! manual's "Supported Date and Time Formats" grammar reads.

/// End positions of a partial match (sorted, deduplicated).
pub(crate) type Ends = Vec<usize>;

/// A pattern: all end positions of matches starting at `p`.
pub(crate) trait Pat {
    fn ends(&self, s: &[u8], p: usize, out: &mut Ends);

    /// Longest match length from `p`.
    fn longest(&self, s: &[u8], p: usize) -> Option<usize> {
        let mut out = Vec::new();
        self.ends(s, p, &mut out);
        out.into_iter().max().map(|e| e - p)
    }
}

impl<F: Fn(&[u8], usize, &mut Ends)> Pat for F {
    fn ends(&self, s: &[u8], p: usize, out: &mut Ends) {
        self(s, p, out)
    }
}

/// One byte from a set.
pub(crate) fn set(chars: &'static [u8]) -> impl Pat {
    move |s: &[u8], p: usize, out: &mut Ends| {
        if p < s.len() && chars.contains(&s[p]) {
            out.push(p + 1);
        }
    }
}

/// One byte satisfying a predicate.
pub(crate) fn byte(f: fn(u8) -> bool) -> impl Pat {
    move |s: &[u8], p: usize, out: &mut Ends| {
        if p < s.len() && f(s[p]) {
            out.push(p + 1);
        }
    }
}

/// A literal, ASCII case-insensitive.
pub(crate) fn lit(w: &'static str) -> impl Pat {
    move |s: &[u8], p: usize, out: &mut Ends| {
        let w = w.as_bytes();
        if s.len() >= p + w.len() && s[p..p + w.len()].eq_ignore_ascii_case(w) {
            out.push(p + w.len());
        }
    }
}

/// A literal, case-sensitive.
pub(crate) fn lit_cs(w: &'static str) -> impl Pat {
    move |s: &[u8], p: usize, out: &mut Ends| {
        let w = w.as_bytes();
        if s.len() >= p + w.len() && &s[p..p + w.len()] == w {
            out.push(p + w.len());
        }
    }
}

/// Any of several literals (case-insensitive).
pub(crate) fn words(ws: &'static [&'static str]) -> impl Pat {
    move |s: &[u8], p: usize, out: &mut Ends| {
        for w in ws {
            let w = w.as_bytes();
            if s.len() >= p + w.len() && s[p..p + w.len()].eq_ignore_ascii_case(w) {
                out.push(p + w.len());
            }
        }
    }
}

/// `min..=max` repetitions of a single-byte class.
pub(crate) fn rep(f: fn(u8) -> bool, min: usize, max: usize) -> impl Pat {
    move |s: &[u8], p: usize, out: &mut Ends| {
        let mut n = 0;
        while n < max && p + n < s.len() && f(s[p + n]) {
            n += 1;
            if n >= min {
                out.push(p + n);
            }
        }
        if min == 0 {
            out.push(p);
        }
    }
}

/// `n` digits exactly.
pub(crate) fn digits(min: usize, max: usize) -> impl Pat {
    rep(|c| c.is_ascii_digit(), min, max)
}

/// Optional.
pub(crate) fn opt<P: Pat>(a: P) -> impl Pat {
    move |s: &[u8], p: usize, out: &mut Ends| {
        out.push(p);
        a.ends(s, p, out);
    }
}

/// Zero or more repetitions of a pattern.
pub(crate) fn star<P: Pat>(a: P) -> impl Pat {
    move |s: &[u8], p: usize, out: &mut Ends| {
        let mut frontier = vec![p];
        let mut seen = vec![p];
        out.push(p);
        while let Some(q) = frontier.pop() {
            let mut tmp = Vec::new();
            a.ends(s, q, &mut tmp);
            for e in tmp {
                if e > q && !seen.contains(&e) {
                    seen.push(e);
                    out.push(e);
                    frontier.push(e);
                }
            }
        }
    }
}

/// Two patterns in sequence.
pub(crate) fn seq2<A: Pat, B: Pat>(a: A, b: B) -> impl Pat {
    move |s: &[u8], p: usize, out: &mut Ends| {
        let mut mid = Vec::new();
        a.ends(s, p, &mut mid);
        mid.sort_unstable();
        mid.dedup();
        for m in mid {
            b.ends(s, m, out);
        }
    }
}

/// Alternatives.
pub(crate) fn alt2<A: Pat, B: Pat>(a: A, b: B) -> impl Pat {
    move |s: &[u8], p: usize, out: &mut Ends| {
        a.ends(s, p, out);
        b.ends(s, p, out);
    }
}

/// `seq!(a, b, c, ...)`
macro_rules! seq {
    ($a:expr) => { $a };
    ($a:expr, $($rest:expr),+) => { $crate::parse::pat::seq2($a, seq!($($rest),+)) };
}

/// `alt!(a, b, c, ...)`
macro_rules! alt {
    ($a:expr) => { $a };
    ($a:expr, $($rest:expr),+) => { $crate::parse::pat::alt2($a, alt!($($rest),+)) };
}

pub(crate) use alt;
pub(crate) use seq;

/// Succeeds without consuming when the next byte satisfies `f` (or, with
/// `at_end`, at the end of input).
pub(crate) fn peek(f: fn(u8) -> bool, at_end: bool) -> impl Pat {
    move |s: &[u8], p: usize, out: &mut Ends| {
        if p >= s.len() || s[p] == 0 {
            if at_end {
                out.push(p);
            }
        } else if f(s[p]) {
            out.push(p);
        }
    }
}
