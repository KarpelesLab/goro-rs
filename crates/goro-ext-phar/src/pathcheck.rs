//! phar_path_check() (php-src ext/phar/phar_path_check.re, a re2c
//! scanner, written out by hand).

/// phar_path_check_result
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pcr {
    UseQuery,
    IsOk,
    ErrDoubleSlash,
    ErrUpDir,
    ErrCurrDir,
    ErrBackSlash,
    ErrStar,
    ErrIllegalChar,
}

impl Pcr {
    /// `> pcr_is_ok`
    pub fn is_err(self) -> bool {
        self > Pcr::IsOk
    }
}

/// The result of a path check: the (possibly adjusted) path is
/// `path[start..start + len]`.
pub struct Checked {
    pub result: Pcr,
    pub start: usize,
    pub len: usize,
    pub error: &'static str,
}

impl Checked {
    pub fn path<'a>(&self, p: &'a [u8]) -> &'a [u8] {
        &p[self.start..self.start + self.len]
    }
}

/// phar_path_check(&s, &len, &error); the scanner sees the path followed
/// by its NUL terminator.
pub fn path_check(path: &[u8]) -> Checked {
    let len = path.len();
    let at = |i: usize| -> u8 { path.get(i).copied().unwrap_or(0) };
    let mk = |result, start, len, error| Checked {
        result,
        start,
        len,
        error,
    };
    if len == 1 && path[0] == b'.' {
        return mk(Pcr::ErrCurrDir, 0, len, "current directory reference");
    } else if len == 2 && path[0] == b'.' && path[1] == b'.' {
        return mk(Pcr::ErrUpDir, 0, len, "upper directory reference");
    }
    let cont = |c: u8| (0x80..=0xBF).contains(&c);
    let mut p = 0usize;
    loop {
        let c = at(p);
        let eos = |c: u8| c == b'/' || c == 0;
        match c {
            b'/' if at(p + 1) == b'/' => {
                return mk(Pcr::ErrDoubleSlash, 0, len, "double slash");
            }
            b'/' if at(p + 1) == b'.' && at(p + 2) == b'.' && eos(at(p + 3)) => {
                return mk(Pcr::ErrUpDir, 0, len, "upper directory reference");
            }
            b'/' if at(p + 1) == b'.' && eos(at(p + 2)) => {
                return mk(Pcr::ErrCurrDir, 0, len, "current directory reference");
            }
            b'\\' => return mk(Pcr::ErrBackSlash, 0, len, "back-slash"),
            b'*' => return mk(Pcr::ErrStar, 0, len, "star"),
            b'?' => {
                let start = usize::from(at(0) == b'/');
                // p is past the '?'
                return mk(Pcr::UseQuery, start, p + 1 - start - 1, "");
            }
            0 => {
                // END
                let (start, l) = if at(0) == b'/' {
                    (1, len.saturating_sub(1))
                } else {
                    (0, len)
                };
                if (p + 1) - start - 1 != l {
                    return mk(Pcr::ErrIllegalChar, start, l, "illegal character");
                }
                return mk(Pcr::IsOk, start, l, "");
            }
            0x1A..=0x7F => p += 1,
            0xC2..=0xDF if cont(at(p + 1)) => p += 2,
            0xE0 if (0xA0..=0xBF).contains(&at(p + 1)) && cont(at(p + 2)) => p += 3,
            0xE1..=0xEC | 0xEE..=0xEF if cont(at(p + 1)) && cont(at(p + 2)) => p += 3,
            0xED if (0x80..=0x9F).contains(&at(p + 1)) && cont(at(p + 2)) => p += 3,
            0xF0 if (0x90..=0xBF).contains(&at(p + 1)) && cont(at(p + 2)) && cont(at(p + 3)) => {
                p += 4
            }
            0xF1..=0xF3 if cont(at(p + 1)) && cont(at(p + 2)) && cont(at(p + 3)) => p += 4,
            0xF4 if (0x80..=0x8F).contains(&at(p + 1)) && cont(at(p + 2)) && cont(at(p + 3)) => {
                p += 4
            }
            _ => return mk(Pcr::ErrIllegalChar, 0, len, "illegal character"),
        }
    }
}
