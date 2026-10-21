//! Port of php-src's CSV engine (`php_fgetcsv` / `php_fputcsv` from
//! ext/standard/file.c).  Shared by `fgetcsv()`, `str_getcsv()`,
//! `fputcsv()` and the `SplFileObject` CSV methods.
//!
//! The C code walks NUL-terminated buffers with raw pointers; this port uses
//! signed indices and reads a virtual `\0` at (and past) the end of the buffer
//! so that the edge cases php-src exhibits with `\0` delimiters/enclosures
//! (e.g. GH-12151, oss-fuzz #57392) are reproduced byte-for-byte.
//!
//! Multibyte handling: php-src steps through the buffer with `php_mblen()`.
//! In the default "C" locale every byte is a single character, which is what
//! this port assumes.

/// `PHP_CSV_NO_ESCAPE` is modelled as `None`.
pub type CsvEscape = Option<u8>;

#[inline]
fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

#[inline]
fn at(buf: &[u8], i: isize) -> u8 {
    if i >= 0 && (i as usize) < buf.len() {
        buf[i as usize]
    } else {
        0
    }
}

/// Append `buf[from..to]` to `out`, reading the virtual NUL terminator for
/// indices past the end (mirrors `zend_mempcpy` on a NUL-terminated buffer).
fn copy_range(out: &mut Vec<u8>, buf: &[u8], from: isize, to: isize) {
    let mut i = from;
    while i < to {
        out.push(at(buf, i));
        i += 1;
    }
}

/// `php_fgetcsv_lookup_trailing_spaces`: index just before a trailing
/// "\r\n", "\n" or "\r".
pub fn lookup_trailing_spaces(buf: &[u8]) -> usize {
    let len = buf.len();
    if len == 0 {
        return 0;
    }
    match buf[len - 1] {
        b'\n' => {
            if len >= 2 && buf[len - 2] == b'\r' {
                len - 2
            } else {
                len - 1
            }
        }
        b'\r' => len - 1,
        _ => len,
    }
}

/// Port of `php_fgetcsv()`.
///
/// `buf` is the first line (including its line ending).  `next_line` fetches
/// another line from the stream when an enclosure spans lines; pass `None`
/// for `str_getcsv()` (no stream).  Returns `None` for a blank line (callers
/// turn that into `[null]`).
pub fn php_fgetcsv(
    buf: Vec<u8>,
    delimiter: u8,
    enclosure: u8,
    escape: CsvEscape,
    mut next_line: Option<&mut dyn FnMut() -> Option<Vec<u8>>>,
) -> Option<Vec<Vec<u8>>> {
    let mut buf = buf;
    let mut bptr: isize = 0;
    let mut limit: isize = lookup_trailing_spaces(&buf) as isize;
    let mut line_end: isize = limit;
    let mut line_end_len: isize = buf.len() as isize - limit;
    let mut values: Vec<Vec<u8>> = Vec::new();
    let mut first_field = true;

    loop {
        let mut temp: Vec<u8> = Vec::new();
        let comp_end: usize;

        let mut inc_len: isize = if bptr < limit { 1 } else { 0 };
        if inc_len == 1 {
            let mut tmp = bptr;
            while at(&buf, tmp) != delimiter && is_c_space(at(&buf, tmp)) {
                tmp += 1;
            }
            if at(&buf, tmp) == enclosure && tmp < limit {
                bptr = tmp;
            }
        }

        if first_field && bptr == line_end {
            return None;
        }
        first_field = false;

        if inc_len != 0 && at(&buf, bptr) == enclosure {
            let mut state = 0;
            bptr += 1;
            let mut hunk_begin = bptr;

            // 2A. handle enclosure delimited field
            'quit_loop_2: loop {
                if inc_len == 0 {
                    match state {
                        2 => {
                            copy_range(&mut temp, &buf, hunk_begin, bptr - 1);
                            hunk_begin = bptr;
                            break 'quit_loop_2;
                        }
                        _ => {
                            if state == 1 {
                                copy_range(&mut temp, &buf, hunk_begin, bptr);
                                hunk_begin = bptr;
                            }
                            if hunk_begin != line_end {
                                copy_range(&mut temp, &buf, hunk_begin, bptr);
                                hunk_begin = bptr;
                            }
                            // add the embedded line end to the field
                            copy_range(&mut temp, &buf, line_end, line_end + line_end_len);

                            let new_buf = match next_line.as_mut() {
                                Some(f) => f(),
                                None => None,
                            };
                            match new_buf {
                                None => {
                                    // unterminated enclosure (or no stream)
                                    if bptr > limit {
                                        if hunk_begin == bptr {
                                            hunk_begin -= 1;
                                        }
                                        bptr -= 1;
                                    }
                                    break 'quit_loop_2;
                                }
                                Some(nb) => {
                                    buf = nb;
                                    bptr = 0;
                                    hunk_begin = 0;
                                    limit = lookup_trailing_spaces(&buf) as isize;
                                    line_end = limit;
                                    line_end_len = buf.len() as isize - limit;
                                    state = 0;
                                }
                            }
                        }
                    }
                } else {
                    // inc_len == 1: determine if the enclosure is 'real' or escaped
                    match state {
                        1 => {
                            bptr += 1;
                            state = 0;
                        }
                        2 => {
                            if at(&buf, bptr) != enclosure {
                                // real enclosure
                                copy_range(&mut temp, &buf, hunk_begin, bptr - 1);
                                hunk_begin = bptr;
                                break 'quit_loop_2;
                            }
                            copy_range(&mut temp, &buf, hunk_begin, bptr);
                            bptr += 1;
                            hunk_begin = bptr;
                            state = 0;
                        }
                        _ => {
                            let c = at(&buf, bptr);
                            if c == enclosure {
                                state = 2;
                            } else if escape == Some(c) {
                                state = 1;
                            }
                            bptr += 1;
                        }
                    }
                }
                inc_len = if bptr < limit { 1 } else { 0 };
            }

            // quit_loop_2: look up for a delimiter
            loop {
                if inc_len == 0 {
                    break;
                }
                if at(&buf, bptr) == delimiter {
                    break;
                }
                bptr += inc_len;
                inc_len = if bptr < limit { 1 } else { 0 };
            }
            // quit_loop_3
            copy_range(&mut temp, &buf, hunk_begin, bptr);
            bptr += inc_len;
            comp_end = temp.len();
        } else {
            // 2B. Handle non-enclosure field
            let hunk_begin = bptr;
            loop {
                if inc_len == 0 {
                    break;
                }
                if at(&buf, bptr) == delimiter {
                    break;
                }
                bptr += inc_len;
                inc_len = if bptr < limit { 1 } else { 0 };
            }
            // quit_loop_4
            copy_range(&mut temp, &buf, hunk_begin, bptr);
            comp_end = lookup_trailing_spaces(&temp);
            if at(&buf, bptr) == delimiter {
                bptr += 1;
            }
        }

        temp.truncate(comp_end);
        values.push(temp);

        if inc_len <= 0 {
            break;
        }
    }
    Some(values)
}

/// Port of the line-building part of `php_fputcsv()`: `fields` are the
/// already string-converted field values.  `eol` defaults to "\n".
pub fn php_fputcsv_line(
    fields: &[Vec<u8>],
    delimiter: u8,
    enclosure: u8,
    escape: CsvEscape,
    eol: Option<&[u8]>,
) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let count = fields.len();
    for (i, field) in fields.iter().enumerate() {
        let needs = field.iter().any(|&c| {
            c == delimiter
                || c == enclosure
                || escape == Some(c)
                || c == b'\n'
                || c == b'\r'
                || c == b'\t'
                || c == b' '
        });
        if needs {
            let mut escaped = false;
            out.push(enclosure);
            for &c in field.iter() {
                if escape == Some(c) {
                    escaped = true;
                } else if !escaped && c == enclosure {
                    out.push(enclosure);
                } else {
                    escaped = false;
                }
                out.push(c);
            }
            out.push(enclosure);
        } else {
            out.extend_from_slice(field);
        }
        if i + 1 != count {
            out.push(delimiter);
        }
    }
    match eol {
        Some(e) => out.extend_from_slice(e),
        None => out.push(b'\n'),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<Vec<u8>> {
        v.iter().map(|x| x.as_bytes().to_vec()).collect()
    }

    #[test]
    fn basic() {
        assert_eq!(php_fgetcsv(b"a,b,c\n".to_vec(), b',', b'"', Some(b'\\'), None), Some(s(&["a", "b", "c"])));
        assert_eq!(php_fgetcsv(b"\n".to_vec(), b',', b'"', Some(b'\\'), None), None);
        assert_eq!(php_fgetcsv(b"\"a\"\"b\",c".to_vec(), b',', b'"', Some(b'\\'), None), Some(s(&["a\"b", "c"])));
        assert_eq!(php_fgetcsv(b"a\tb\t\t \n".to_vec(), b'\t', b'"', Some(b'\\'), None), Some(s(&["a", "b", "", " "])));
    }
}
