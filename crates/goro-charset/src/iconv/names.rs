//! Charset names as PHP's iconv functions accept them (observed on PHP 8.5
//! with glibc 2.43):
//!
//! - the name ends at the first NUL byte (PHP passes a C string);
//! - case is ignored, and every character other than ASCII letters, digits
//!   and `-_.:,/` is dropped ("utf 8", " UTF-8", "(UTF-8)" all work);
//! - a single trailing '/' is dropped ("UTF-8/" works, "UTF-8/X" doesn't);
//! - an empty name is the locale's charset, UTF-8 in PHP's default C.UTF-8;
//! - "//" starts the suffixes: '/'-separated groups of ','-separated
//!   TRANSLIT / IGNORE tokens (any case), read from the last group back to the
//!   first unknown one ("//X//IGNORE" ignores, "//IGNORE//X" doesn't);
//! - `iconv()` itself only treats illegal input as skippable when the output
//!   name contains "//IGNORE" (that case) at its end or followed by "//".

use super::charset::Charset;

/// Upper-cases and drops the characters glibc ignores in charset names.
pub(crate) fn normalise(name: &str) -> String {
    name.bytes()
        .take_while(|&b| b != 0)
        .filter(|b| b.is_ascii_alphanumeric() || b"-_.:,/".contains(b))
        .map(|b| char::from(b.to_ascii_uppercase()))
        .collect()
}

/// Conversion flags from the `//` suffixes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    pub translit: bool,
    pub ignore: bool,
}

/// Splits a charset argument into the charset and its suffix flags.
/// `None` when the charset is unknown.
pub fn parse_charset(arg: &[u8]) -> Option<(Charset, Flags)> {
    let arg = match arg.iter().position(|&b| b == 0) {
        Some(n) => &arg[..n],
        None => arg,
    };
    let split = arg.windows(2).position(|w| w == b"//");
    let (name, suffix) = match split {
        Some(i) => (&arg[..i], &arg[i + 2..]),
        None => (arg, &arg[arg.len()..]),
    };
    let cs = Charset::lookup(&String::from_utf8_lossy(name))?;
    let mut flags = Flags::default();
    for group in suffix.split(|&b| b == b'/').rev() {
        let mut g = Flags::default();
        let mut known = true;
        for tok in group.split(|&b| b == b',') {
            if tok.is_empty() {
                continue;
            }
            if tok.eq_ignore_ascii_case(b"TRANSLIT") {
                g.translit = true;
            } else if tok.eq_ignore_ascii_case(b"IGNORE") {
                g.ignore = true;
            } else {
                known = false;
            }
        }
        if !known {
            break;
        }
        flags.translit |= g.translit;
        flags.ignore |= g.ignore;
    }
    Some((cs, flags))
}

/// Whether PHP's `iconv()` treats an illegal sequence as skipped for this
/// output charset argument (see the module documentation).
pub(crate) fn php_ignore(arg: &[u8]) -> bool {
    let arg = match arg.iter().position(|&b| b == 0) {
        Some(n) => &arg[..n],
        None => arg,
    };
    let pat = b"//IGNORE";
    let mut i = 0;
    while i + pat.len() <= arg.len() {
        if &arg[i..i + pat.len()] == pat {
            let rest = &arg[i + pat.len()..];
            if rest.is_empty() || rest.starts_with(b"//") {
                return true;
            }
        }
        i += 1;
    }
    false
}
