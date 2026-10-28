//! A tiny pattern matcher for the date/time grammar.
//!
//! A pattern reports every position where a match starting at `p` can
//! end (to a continuation, so nothing is allocated), so alternatives and
//! optional parts behave like a regular expression and the scanner can take
//! the longest match, as the PHP manual's "Supported Date and Time Formats"
//! grammar reads.

/// A pattern: calls `k` with every end position of a match from `p`.
pub(crate) trait Pat {
    fn ends(&self, s: &[u8], p: usize, k: &mut dyn FnMut(usize));

    /// Longest match length from `p`.
    fn longest(&self, s: &[u8], p: usize) -> Option<usize> {
        let mut best: Option<usize> = None;
        self.ends(s, p, &mut |e| {
            if best.is_none_or(|b| e > b) {
                best = Some(e);
            }
        });
        best.map(|e| e - p)
    }
}

impl<F: Fn(&[u8], usize, &mut dyn FnMut(usize))> Pat for F {
    fn ends(&self, s: &[u8], p: usize, k: &mut dyn FnMut(usize)) {
        self(s, p, k)
    }
}

/// One byte from a set.
pub(crate) fn set(chars: &'static [u8]) -> impl Pat {
    move |s: &[u8], p: usize, k: &mut dyn FnMut(usize)| {
        if p < s.len() && chars.contains(&s[p]) {
            k(p + 1);
        }
    }
}

/// One byte satisfying a predicate.
pub(crate) fn byte(f: fn(u8) -> bool) -> impl Pat {
    move |s: &[u8], p: usize, k: &mut dyn FnMut(usize)| {
        if p < s.len() && f(s[p]) {
            k(p + 1);
        }
    }
}

/// A literal, ASCII case-insensitive.
pub(crate) fn lit(w: &'static str) -> impl Pat {
    move |s: &[u8], p: usize, k: &mut dyn FnMut(usize)| {
        let w = w.as_bytes();
        if s.len() >= p + w.len() && s[p..p + w.len()].eq_ignore_ascii_case(w) {
            k(p + w.len());
        }
    }
}

/// A literal, case-sensitive.
pub(crate) fn lit_cs(w: &'static str) -> impl Pat {
    move |s: &[u8], p: usize, k: &mut dyn FnMut(usize)| {
        let w = w.as_bytes();
        if s.len() >= p + w.len() && &s[p..p + w.len()] == w {
            k(p + w.len());
        }
    }
}

/// Any of several literals (case-insensitive).
pub(crate) fn words(ws: &'static [&'static str]) -> impl Pat {
    move |s: &[u8], p: usize, k: &mut dyn FnMut(usize)| {
        for w in ws {
            let w = w.as_bytes();
            if s.len() >= p + w.len() && s[p..p + w.len()].eq_ignore_ascii_case(w) {
                k(p + w.len());
            }
        }
    }
}

/// `min..=max` repetitions of a single-byte class.
pub(crate) fn rep(f: fn(u8) -> bool, min: usize, max: usize) -> impl Pat {
    move |s: &[u8], p: usize, k: &mut dyn FnMut(usize)| {
        if min == 0 {
            k(p);
        }
        let mut n = 0;
        while n < max && p + n < s.len() && f(s[p + n]) {
            n += 1;
            if n >= min {
                k(p + n);
            }
        }
    }
}

/// `min..=max` digits.
pub(crate) fn digits(min: usize, max: usize) -> impl Pat {
    rep(|c| c.is_ascii_digit(), min, max)
}

/// Optional.
pub(crate) fn opt<P: Pat>(a: P) -> impl Pat {
    move |s: &[u8], p: usize, k: &mut dyn FnMut(usize)| {
        k(p);
        a.ends(s, p, k);
    }
}

/// Zero or more repetitions of a pattern (each repetition must consume).
pub(crate) fn star<P: Pat>(a: P) -> impl Pat {
    fn go<P: Pat>(a: &P, s: &[u8], p: usize, k: &mut dyn FnMut(usize)) {
        k(p);
        a.ends(s, p, &mut |e| {
            if e > p {
                go(a, s, e, k);
            }
        });
    }
    move |s: &[u8], p: usize, k: &mut dyn FnMut(usize)| go(&a, s, p, k)
}

/// Two patterns in sequence.
pub(crate) fn seq2<A: Pat, B: Pat>(a: A, b: B) -> impl Pat {
    move |s: &[u8], p: usize, k: &mut dyn FnMut(usize)| {
        a.ends(s, p, &mut |m| b.ends(s, m, k));
    }
}

/// Alternatives.
pub(crate) fn alt2<A: Pat, B: Pat>(a: A, b: B) -> impl Pat {
    move |s: &[u8], p: usize, k: &mut dyn FnMut(usize)| {
        a.ends(s, p, k);
        b.ends(s, p, k);
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
    move |s: &[u8], p: usize, k: &mut dyn FnMut(usize)| {
        if p >= s.len() || s[p] == 0 {
            if at_end {
                k(p);
            }
        } else if f(s[p]) {
            k(p);
        }
    }
}
