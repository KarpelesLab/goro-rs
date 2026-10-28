//! `iconv()`, `iconv_strlen()`, `iconv_substr()`, `iconv_strpos()` and
//! `iconv_strrpos()` with PHP's diagnostics (PHP manual: iconv functions;
//! tests ext/iconv/tests/*.phpt; details observed on PHP 8.5 / glibc 2.43).
//!
//! The string functions decode their input to UCS-4LE internally, which is
//! why an unknown charset reports a conversion "to \"UCS-4LE\"". Suffixes such
//! as `//IGNORE` on their charset have no effect.

use super::charset::Charset;
use super::convert::{self, ConvertError, ConvertErrorKind};
use super::names;
use super::{CharsetArg, Diag, IconvError, MAX_CHARSET_LEN};

pub(crate) const MSG_TOO_LONG: &str =
    "Encoding parameter exceeds the maximum allowed length of 64 characters";
pub(crate) const MSG_ILLEGAL: &str = "Detected an illegal character in input string";
pub(crate) const MSG_INCOMPLETE: &str =
    "Detected an incomplete multibyte character in input string";

pub(crate) fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

pub(crate) fn wrong_encoding(from: &[u8], to: &[u8]) -> Diag {
    Diag::Warning(format!(
        "Wrong encoding, conversion from \"{}\" to \"{}\" is not allowed",
        lossy(from),
        lossy(to)
    ))
}

pub(crate) fn conv_diag(e: &ConvertError) -> Diag {
    Diag::Notice(
        match e.kind {
            ConvertErrorKind::IllegalSequence => MSG_ILLEGAL,
            ConvertErrorKind::IncompleteSequence => MSG_INCOMPLETE,
        }
        .to_string(),
    )
}

/// `iconv(string $from_encoding, string $to_encoding, string $string): string|false`.
pub fn iconv(from: &[u8], to: &[u8], input: &[u8], diags: &mut Vec<Diag>) -> Option<Vec<u8>> {
    if from.len() >= MAX_CHARSET_LEN || to.len() >= MAX_CHARSET_LEN {
        diags.push(Diag::Warning(MSG_TOO_LONG.into()));
        return None;
    }
    let (Some((src, _)), Some((dst, mut flags))) =
        (names::parse_charset(from), names::parse_charset(to))
    else {
        diags.push(wrong_encoding(from, to));
        return None;
    };
    flags.ignore &= names::php_ignore(to);
    match convert::convert(src, dst, flags, input) {
        Ok(v) => Some(v),
        Err(e) => {
            diags.push(conv_diag(&e));
            None
        }
    }
}

/// Resolves the charset of a string function; `None` after a warning.
fn string_charset(charset: CharsetArg<'_>, diags: &mut Vec<Diag>) -> Option<Charset> {
    let charset = charset.checked(diags)?;
    match names::parse_charset(charset) {
        Some((cs, _)) => Some(cs),
        None => {
            diags.push(wrong_encoding(charset, b"UCS-4LE"));
            None
        }
    }
}

fn decode(cs: Charset, s: &[u8], diags: &mut Vec<Diag>) -> Option<Vec<u32>> {
    match convert::decode_all(cs, s) {
        Ok(v) => Some(v),
        Err(e) => {
            diags.push(conv_diag(&e));
            None
        }
    }
}

/// `iconv_strlen(string $string, ?string $encoding = null): int|false`.
pub fn strlen<'a>(
    s: &[u8],
    charset: impl Into<CharsetArg<'a>>,
    diags: &mut Vec<Diag>,
) -> Option<usize> {
    let cs = string_charset(charset.into(), diags)?;
    match convert::count_chars(cs, s) {
        Ok(n) => Some(n),
        Err(e) => {
            diags.push(conv_diag(&e));
            None
        }
    }
}

/// `iconv_substr(string $string, int $offset, ?int $length = null, ?string $encoding = null): string|false`.
pub fn substr<'a>(
    s: &[u8],
    offset: i64,
    length: Option<i64>,
    charset: impl Into<CharsetArg<'a>>,
    diags: &mut Vec<Diag>,
) -> Option<Vec<u8>> {
    let cs = string_charset(charset.into(), diags)?;
    let chars = decode(cs, s, diags)?;
    let total = chars.len() as i64;
    let mut start = offset;
    if start < 0 {
        start = (start + total).max(0);
    }
    if start > total {
        return Some(Vec::new());
    }
    let mut len = length.unwrap_or(total - start);
    if len < 0 {
        len += total - start;
        if len < 0 {
            return Some(Vec::new());
        }
    }
    let end = (start + len).min(total);
    let slice = &chars[start as usize..end as usize];
    if slice.is_empty() {
        return Some(Vec::new());
    }
    match convert::encode_all(cs, slice) {
        Ok(v) => Some(v),
        Err(e) => {
            diags.push(conv_diag(&e));
            None
        }
    }
}

fn find(hay: &[u32], needle: &[u32], from: usize) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// `iconv_strpos(string $haystack, string $needle, int $offset = 0, ?string $encoding = null): int|false`.
pub fn strpos<'a>(
    haystack: &[u8],
    needle: &[u8],
    offset: i64,
    charset: impl Into<CharsetArg<'a>>,
    diags: &mut Vec<Diag>,
) -> Result<Option<usize>, IconvError> {
    let Some(cs) = string_charset(charset.into(), diags) else {
        return Ok(None);
    };
    let Some(hay) = decode(cs, haystack, diags) else {
        return Ok(None);
    };
    let len = hay.len() as i64;
    let start = if offset < 0 { offset + len } else { offset };
    if start < 0 || start > len {
        return Err(IconvError::ValueError(
            "iconv_strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)"
                .into(),
        ));
    }
    if needle.is_empty() {
        return Ok(None);
    }
    let Some(ndl) = decode(cs, needle, diags) else {
        return Ok(None);
    };
    if ndl.is_empty() {
        return Ok(None);
    }
    Ok(find(&hay, &ndl, start as usize))
}

/// `iconv_strrpos(string $haystack, string $needle, ?string $encoding = null): int|false`.
pub fn strrpos<'a>(
    haystack: &[u8],
    needle: &[u8],
    charset: impl Into<CharsetArg<'a>>,
    diags: &mut Vec<Diag>,
) -> Option<usize> {
    let cs = string_charset(charset.into(), diags)?;
    if needle.is_empty() {
        return None;
    }
    let hay = decode(cs, haystack, diags)?;
    let ndl = decode(cs, needle, diags)?;
    if ndl.is_empty() || ndl.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - ndl.len())
        .rev()
        .find(|&i| hay[i..i + ndl.len()] == *ndl)
}
