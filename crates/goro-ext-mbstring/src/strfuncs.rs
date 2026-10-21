//! mbstring.c string functions: searching, substrings, width, case
//! conversion, trimming, splitting, mb_ord/mb_chr, mb_str_pad, mb_scrub.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::mbfl::{self, EncNo, Encoding, MBFL_BAD_INPUT, MBFL_ENCTYPE_SBCS, MBFL_ENCTYPE_WCS2, MBFL_ENCTYPE_WCS4};
use crate::mbstring::*;
use crate::state;
use crate::unicode::CaseMode;
use crate::zpp::*;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"mb_strpos", mb_strpos);
    vm.register_function(b"mb_strrpos", mb_strrpos);
    vm.register_function(b"mb_stripos", mb_stripos);
    vm.register_function(b"mb_strripos", mb_strripos);
    vm.register_function(b"mb_strstr", mb_strstr);
    vm.register_function(b"mb_strrchr", mb_strrchr);
    vm.register_function(b"mb_stristr", mb_stristr);
    vm.register_function(b"mb_strrichr", mb_strrichr);
    vm.register_function(b"mb_substr_count", mb_substr_count);
    vm.register_function(b"mb_substr", mb_substr);
    vm.register_function(b"mb_strcut", mb_strcut);
    vm.register_function(b"mb_strwidth", mb_strwidth);
    vm.register_function(b"mb_strimwidth", mb_strimwidth);
    vm.register_function(b"mb_convert_case", mb_convert_case);
    vm.register_function(b"mb_strtoupper", mb_strtoupper);
    vm.register_function(b"mb_strtolower", mb_strtolower);
    vm.register_function(b"mb_ucfirst", mb_ucfirst);
    vm.register_function(b"mb_lcfirst", mb_lcfirst);
    vm.register_function(b"mb_trim", mb_trim);
    vm.register_function(b"mb_ltrim", mb_ltrim);
    vm.register_function(b"mb_rtrim", mb_rtrim);
    vm.register_function(b"mb_str_split", mb_str_split);
    vm.register_function(b"mb_ord", mb_ord);
    vm.register_function(b"mb_chr", mb_chr);
    vm.register_function(b"mb_str_pad", mb_str_pad);
    vm.register_function(b"mb_scrub", mb_scrub);
}

fn new_array(arr: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(arr)))
}

/* ===== Searching ===== */

pub const MBFL_ERROR_NOT_FOUND: usize = usize::MAX;
pub const MBFL_ERROR_ENCODING: usize = usize::MAX - 3;
pub const MBFL_ERROR_EMPTY: usize = usize::MAX - 7;
pub const MBFL_ERROR_OFFSET: usize = usize::MAX - 15;
pub const MBFL_SUBSTR_UNTIL_END: usize = usize::MAX;

#[inline]
pub fn mbfl_is_error(n: usize) -> bool {
    n >= MBFL_ERROR_OFFSET
}

fn offset_to_pointer_utf8(s: &[u8], start: usize, offset: i64) -> Option<usize> {
    let end = s.len();
    if offset < 0 {
        let mut pos = end;
        let mut offset = offset;
        while offset < 0 {
            if pos <= start {
                return None;
            }
            pos -= 1;
            let c = s[pos];
            if c < 0x80 || (c & 0xC0) != 0x80 {
                offset += 1;
            }
        }
        Some(pos)
    } else {
        let tbl = &crate::mbfl::tables::filt_utf8::mblen_table_utf8;
        let mut pos = start;
        let mut offset = offset;
        while offset > 0 {
            offset -= 1;
            if pos >= end {
                return None;
            }
            pos += tbl[s[pos] as usize] as usize;
        }
        Some(pos)
    }
}

/// zend_memnstr: first occurrence of needle in hay[from..end]
pub fn memnstr(hay: &[u8], from: usize, needle: &[u8], end: usize) -> Option<usize> {
    let end = end.min(hay.len());
    if needle.is_empty() {
        return Some(from);
    }
    if from > end || needle.len() > end - from {
        return None;
    }
    let h = &hay[from..end];
    if needle.len() == 1 {
        return h.iter().position(|&c| c == needle[0]).map(|p| p + from);
    }
    h.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// zend_memnrstr: last occurrence of needle in hay[from..end]
pub fn memnrstr(hay: &[u8], from: usize, needle: &[u8], end: usize) -> Option<usize> {
    let end = end.min(hay.len());
    if needle.is_empty() {
        return Some(end);
    }
    if from > end || needle.len() > end - from {
        return None;
    }
    let h = &hay[from..end];
    h.windows(needle.len()).rposition(|w| w == needle).map(|p| p + from)
}

/// C: `mb_find_strpos`
pub fn find_strpos(haystack: &[u8], needle: &[u8], enc: &Encoding, offset: i64, reverse: bool) -> usize {
    let (h, n): (std::borrow::Cow<[u8]>, std::borrow::Cow<[u8]>) = if !is_utf8_family(enc.no) {
        (to_utf8_badmarker(haystack, enc).into(), to_utf8_badmarker(needle, enc).into())
    } else {
        (haystack.into(), needle.into())
    };
    let Some(mut off_ptr) = offset_to_pointer_utf8(&h, 0, offset) else {
        return MBFL_ERROR_OFFSET;
    };
    if h.len() < n.len() {
        return MBFL_ERROR_NOT_FOUND;
    }
    let found = if !reverse {
        memnstr(&h, off_ptr, &n, h.len())
    } else if offset >= 0 {
        memnrstr(&h, off_ptr, &n, h.len())
    } else {
        // Note: PHP measures the length of the *original* needle here
        let needle_len = fast_strlen_utf8(needle);
        off_ptr = offset_to_pointer_utf8(&h, off_ptr, needle_len as i64).unwrap_or(h.len());
        memnrstr(&h, 0, &n, off_ptr)
    };
    match found {
        Some(p) => fast_strlen_utf8(&h[..p]),
        None => MBFL_ERROR_NOT_FOUND,
    }
}

fn handle_strpos_error(vm: &mut Vm, func: &str, error: usize) -> Result<(), VmError> {
    match error {
        MBFL_ERROR_NOT_FOUND => Ok(()),
        MBFL_ERROR_ENCODING => {
            vm.emit_warning(&format!("{}(): Conversion error", func));
            Ok(())
        }
        MBFL_ERROR_OFFSET => Err(arg_value_error(vm, func, 3, "offset", "must be contained in argument #1 ($haystack)")),
        _ => Err(value_error(vm, "mb_strpos(): Unknown error".to_string())),
    }
}

/// C: `php_mb_stripos`
pub fn stripos(reverse: bool, haystack: &[u8], needle: &[u8], offset: i64, enc: &Encoding) -> usize {
    use crate::unicode::convert_case;
    let u8e = &mbfl::basic::ENC_UTF8;
    let h = convert_case(CaseMode::FoldSimple, haystack, enc, u8e, mbfl::MBFL_OUTPUTFILTER_ILLEGAL_MODE_BADUTF8, 0);
    let n = convert_case(CaseMode::FoldSimple, needle, enc, u8e, mbfl::MBFL_OUTPUTFILTER_ILLEGAL_MODE_BADUTF8, 0);
    find_strpos(&h, &n, u8e, offset, reverse)
}

fn strpos_common(vm: &mut Vm, args: &[Value], func: &str, ci: bool, reverse: bool) -> Result<Value, VmError> {
    let h = str_arg(vm, func, args, 0, "haystack")?;
    let n = str_arg(vm, func, args, 1, "needle")?;
    let offset = if present(args, 2) { long_arg(vm, func, args, 2, "offset")? } else { 0 };
    let enc = encoding_arg(vm, func, args, 3, "encoding")?;
    let r = if ci { stripos(reverse, &h, &n, offset, enc) } else { find_strpos(&h, &n, enc, offset, reverse) };
    if !mbfl_is_error(r) {
        Ok(Value::Long(r as i64))
    } else {
        handle_strpos_error(vm, func, r)?;
        Ok(Value::False)
    }
}

fn mb_strpos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    strpos_common(vm, args, "mb_strpos", false, false)
}
fn mb_strrpos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    strpos_common(vm, args, "mb_strrpos", false, true)
}
fn mb_stripos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    strpos_common(vm, args, "mb_stripos", true, false)
}
fn mb_strripos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    strpos_common(vm, args, "mb_strripos", true, true)
}

/* ===== Substrings ===== */

/// C: `mb_get_substr_slow`
pub fn get_substr_slow(input: &[u8], from: usize, len: usize, enc: &Encoding) -> Vec<u8> {
    let w = to_wchars(enc, input);
    if from >= w.len() || len == 0 {
        return Vec::new();
    }
    let end = if len >= w.len() - from { w.len() } else { from + len };
    from_wchars(enc, &w[from..end])
}

/// C: `mb_get_substr`
pub fn get_substr(input: &[u8], from: usize, len: usize, enc: &Encoding) -> Vec<u8> {
    let in_len = input.len();
    if len == 0 || (from >= in_len && enc.no != EncNo::SjisMac) {
        return Vec::new();
    }
    let flag = (enc.flag & (MBFL_ENCTYPE_SBCS | MBFL_ENCTYPE_WCS2 | MBFL_ENCTYPE_WCS4)) as usize;
    if flag != 0 {
        let from = from.saturating_mul(flag);
        let len = len.saturating_mul(flag);
        if from >= in_len {
            return Vec::new();
        }
        let rest = &input[from..];
        let len = len.min(rest.len());
        return rest[..len].to_vec();
    }
    get_substr_slow(input, from, len, enc)
}

fn strstr_variants(vm: &mut Vm, args: &[Value], func: &str, reverse: bool, ci: bool) -> Result<Value, VmError> {
    let h = str_arg(vm, func, args, 0, "haystack")?;
    let n = str_arg(vm, func, args, 1, "needle")?;
    let part = bool_arg(args, 2);
    let enc = encoding_arg(vm, func, args, 3, "encoding")?;
    let r = if ci { stripos(reverse, &h, &n, 0, enc) } else { find_strpos(&h, &n, enc, 0, reverse) };
    if mbfl_is_error(r) {
        return Ok(Value::False);
    }
    if part {
        Ok(pstring(get_substr(&h, 0, r, enc)))
    } else {
        Ok(pstring(get_substr(&h, r, MBFL_SUBSTR_UNTIL_END, enc)))
    }
}

fn mb_strstr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    strstr_variants(vm, args, "mb_strstr", false, false)
}
fn mb_strrchr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    strstr_variants(vm, args, "mb_strrchr", true, false)
}
fn mb_stristr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    strstr_variants(vm, args, "mb_stristr", false, true)
}
fn mb_strrichr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    strstr_variants(vm, args, "mb_strrichr", true, true)
}

fn mb_substr_count(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_substr_count";
    let h = str_arg(vm, f, args, 0, "haystack")?;
    let n = str_arg(vm, f, args, 1, "needle")?;
    if n.is_empty() {
        return Err(arg_value_error(vm, f, 2, "needle", "must not be empty"));
    }
    let enc = encoding_arg(vm, f, args, 2, "encoding")?;
    let (hu, nu) = if is_utf8_family(enc.no) {
        let hu = if std::str::from_utf8(&h).is_ok() { h.clone() } else { to_utf8_badmarker(&h, enc) };
        let nu = if std::str::from_utf8(&n).is_ok() { n.clone() } else { to_utf8_badmarker(&n, enc) };
        (hu, nu)
    } else {
        let hu = to_utf8_badmarker(&h, enc);
        let nu = to_utf8_badmarker(&n, enc);
        if nu.is_empty() {
            return Err(arg_value_error(vm, f, 2, "needle", "must not be empty"));
        }
        (hu, nu)
    };
    let mut result = 0i64;
    if hu.len() >= nu.len() {
        let mut p = 0;
        while let Some(pos) = memnstr(&hu, p, &nu, hu.len()) {
            p = pos + nu.len();
            result += 1;
        }
    }
    Ok(Value::Long(result))
}

fn mb_substr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_substr";
    let s = str_arg(vm, f, args, 0, "string")?;
    let from = long_arg(vm, f, args, 1, "start")?;
    let len = long_or_null_arg(vm, f, args, 2, "length")?;
    if from == i64::MIN {
        return Err(arg_value_error(vm, f, 2, "start", &format!("must be between {} and {}", i64::MIN + 1, i64::MAX)));
    }
    if len == Some(i64::MIN) {
        return Err(arg_value_error(vm, f, 3, "length", &format!("must be between {} and {}", i64::MIN + 1, i64::MAX)));
    }
    let enc = encoding_arg(vm, f, args, 3, "encoding")?;
    let mut mblen = 0usize;
    if from < 0 || len.map(|l| l < 0).unwrap_or(false) {
        mblen = get_strlen(&s, enc);
    }
    let real_from = if from >= 0 {
        from as usize
    } else if ((-from) as usize) < mblen {
        (mblen as i64 + from) as usize
    } else {
        0
    };
    let real_len = match len {
        None => MBFL_SUBSTR_UNTIL_END,
        Some(l) if l >= 0 => l as usize,
        Some(l) => {
            if real_from < mblen && ((-l) as usize) < mblen - real_from {
                ((mblen - real_from) as i64 + l) as usize
            } else {
                0
            }
        }
    };
    Ok(pstring(get_substr(&s, real_from, real_len, enc)))
}

/// mbfl_strcut fallback for encodings without cut/mblen/fixed width
/// (stateful encodings like UTF-7 or ISO-2022-*). libmbfl feeds the input
/// byte by byte through conversion filters, re-encoding the characters
/// completed after byte `from`, and keeps the longest output (including
/// any closing escape sequence) which fits in `len` bytes. We emulate
/// "characters completed after byte k" with the longest common prefix of
/// the decoding of s[..k] and the decoding of the whole string.
fn strcut_generic(s: &[u8], from: usize, len: usize, enc: &Encoding) -> Vec<u8> {
    let (to_wchar, from_wchar) = match (enc.to_wchar, enc.from_wchar) {
        (Some(t), Some(f)) => (t, f),
        _ => return s[from.min(s.len())..(from + len).min(s.len())].to_vec(),
    };
    let n = s.len();
    let from = from.min(n);
    let len = len.min(n - from);
    let full = to_wchars(enc, s);
    let decode_prefix = |k: usize| -> Vec<u32> {
        let mut w = Vec::new();
        let mut st = 0u32;
        to_wchar(&s[..k], &mut w, &mut st);
        w
    };
    // A trailing error marker is only emitted by the old filters once the
    // next byte arrives (or at the final flush, where it is dropped)
    let lcp = |w: &[u32]| -> usize {
        let w = if w.last() == Some(&MBFL_BAD_INPUT) { &w[..w.len() - 1] } else { w };
        w.iter().zip(full.iter()).take_while(|(a, b)| a == b).count()
    };
    let start = lcp(&decode_prefix(from));
    let encode = |w: &[u32], end: bool| -> Vec<u8> {
        let mut buf = mbfl::ConvBuf::new(w.len() * 2, b'?' as u32, mbfl::MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR);
        from_wchar(w, &mut buf, end);
        if end && w.is_empty() {
            return Vec::new();
        }
        buf.out
    };
    let mut bk = start; // number of completed chars in the accepted output
    let mut e = from;
    while e < n {
        e += 1;
        let pref = decode_prefix(e);
        let done = lcp(&pref).max(start);
        if encode(&full[start..done], false).len() > len {
            break;
        }
        let flushed: Vec<u32> = if pref.len() > start { pref[start..].to_vec() } else { Vec::new() };
        if encode(&flushed, true).len() > len {
            break;
        }
        bk = done;
    }
    encode(&full[start..bk], true)
}

fn mb_strcut(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_strcut";
    let s = str_arg(vm, f, args, 0, "string")?;
    let mut from = long_arg(vm, f, args, 1, "start")?;
    let len = long_or_null_arg(vm, f, args, 2, "length")?;
    let enc = encoding_arg(vm, f, args, 3, "encoding")?;
    let slen = s.len() as i64;
    let mut len = len.unwrap_or(slen);
    if from < 0 {
        from += slen;
        if from < 0 {
            from = 0;
        }
    }
    if len < 0 {
        len += slen - from;
        if len < 0 {
            len = 0;
        }
    }
    if from > slen || len == 0 {
        return Ok(pstr(b""));
    }
    let from = from as usize;
    let len = len as usize;
    if let Some(cut) = enc.cut {
        return Ok(pstring(cut(&s, from, len)));
    }
    let char_len = (enc.flag & (MBFL_ENCTYPE_SBCS | MBFL_ENCTYPE_WCS2 | MBFL_ENCTYPE_WCS4)) as usize;
    if char_len != 0 {
        let from = from & !(char_len - 1);
        let len = len.min(s.len() - from);
        let len = len & !(char_len - 1);
        return Ok(pstr(&s[from..from + len]));
    }
    if let Some(mbtab) = enc.mblen_table {
        let mut p = 0usize;
        let mut m = 0usize;
        let q = from;
        while p < q {
            m = mbtab[s[p] as usize] as usize;
            p += m;
        }
        if p > q {
            p -= m;
        }
        let start = p;
        let end = if len >= s.len() - start {
            s.len()
        } else {
            let q = p + len;
            while p < q {
                m = mbtab[s[p] as usize] as usize;
                p += m;
            }
            if p > q {
                p -= m;
            }
            p
        };
        return Ok(pstr(&s[start..end.min(s.len())]));
    }
    Ok(pstring(strcut_generic(&s, from, len, enc)))
}

/* ===== Width ===== */

pub fn character_width(c: u32) -> usize {
    use crate::mbfl::tables::eaw_table::*;
    if c < FIRST_DOUBLEWIDTH_CODEPOINT as u32 {
        return 1;
    }
    let mut lo = 0usize;
    let mut hi = mbfl_eaw_table.len();
    while lo < hi {
        let probe = (lo + hi) / 2;
        if (c as i64) < mbfl_eaw_table[probe][0] as i64 {
            hi = probe;
        } else if (c as i64) > mbfl_eaw_table[probe][1] as i64 {
            lo = probe + 1;
        } else {
            return 2;
        }
    }
    1
}

pub fn get_strwidth(s: &[u8], enc: &Encoding) -> usize {
    to_wchars(enc, s).iter().map(|&w| character_width(w)).sum()
}

fn mb_strwidth(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = str_arg(vm, "mb_strwidth", args, 0, "string")?;
    let enc = encoding_arg(vm, "mb_strwidth", args, 1, "encoding")?;
    Ok(Value::Long(get_strwidth(&s, enc) as i64))
}

/// C: `mb_trim_string`
fn trim_string(input: &[u8], marker: &[u8], enc: &Encoding, from: usize, width: usize) -> Vec<u8> {
    let w = to_wchars(enc, input);
    let mut remaining_width = width;
    let mut input_err = false;
    let mut cut_needed = false;
    for &c in w.iter().skip(from) {
        let cw = character_width(c);
        input_err |= c == MBFL_BAD_INPUT;
        if remaining_width < cw {
            cut_needed = true;
            break;
        }
        remaining_width -= cw;
    }
    if !cut_needed {
        if !input_err {
            if from == 0 {
                return input.to_vec();
            }
            return get_substr(input, from, MBFL_SUBSTR_UNTIL_END, enc);
        }
        return get_substr_slow(input, from, MBFL_SUBSTR_UNTIL_END, enc);
    }
    let marker_width = get_strwidth(marker, enc);
    if width <= marker_width {
        return marker.to_vec();
    }
    let mut width = width - marker_width;
    let mut end = from;
    while end < w.len() {
        let cw = character_width(w[end]);
        if width < cw {
            break;
        }
        width -= cw;
        end += 1;
    }
    let mut out = from_wchars(enc, &w[from.min(w.len())..end]);
    out.extend_from_slice(marker);
    out
}

fn mb_strimwidth(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_strimwidth";
    let s = str_arg(vm, f, args, 0, "string")?;
    let mut from = long_arg(vm, f, args, 1, "start")?;
    let mut width = long_arg(vm, f, args, 2, "width")?;
    let marker = if present(args, 3) { str_arg(vm, f, args, 3, "trim_marker")? } else { Vec::new() };
    let enc = encoding_arg(vm, f, args, 4, "encoding")?;
    if from != 0 {
        let str_len = get_strlen(&s, enc) as i64;
        if from < 0 {
            from += str_len;
        }
        if from < 0 || from > str_len {
            return Err(arg_value_error(vm, f, 2, "start", "is out of range"));
        }
    }
    if width < 0 {
        vm.emit_deprecated("mb_strimwidth(): passing a negative integer to argument #3 ($width) is deprecated");
        width += get_strwidth(&s, enc) as i64;
        if from > 0 {
            let trimmed = get_substr(&s, 0, from as usize, enc);
            width -= get_strwidth(&trimmed, enc) as i64;
        }
        if width < 0 {
            return Err(arg_value_error(vm, f, 3, "width", "is out of range"));
        }
    }
    Ok(pstring(trim_string(&s, &marker, enc, from as usize, width as usize)))
}

/* ===== Case conversion ===== */

pub fn convert_case_cur(mode: CaseMode, s: &[u8], enc: &Encoding) -> Vec<u8> {
    let (repl, m) = state::illegal_settings();
    crate::unicode::convert_case(mode, s, enc, enc, m, repl)
}

fn mb_convert_case(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_convert_case";
    let s = str_arg(vm, f, args, 0, "string")?;
    let mode = long_arg(vm, f, args, 1, "mode")?;
    let enc = encoding_arg(vm, f, args, 2, "encoding")?;
    let Some(mode) = CaseMode::from_long(mode) else {
        return Err(arg_value_error(vm, f, 2, "mode", "must be one of the MB_CASE_* constants"));
    };
    Ok(pstring(convert_case_cur(mode, &s, enc)))
}

fn mb_strtoupper(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = str_arg(vm, "mb_strtoupper", args, 0, "string")?;
    let enc = encoding_arg(vm, "mb_strtoupper", args, 1, "encoding")?;
    Ok(pstring(convert_case_cur(CaseMode::Upper, &s, enc)))
}

fn mb_strtolower(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = str_arg(vm, "mb_strtolower", args, 0, "string")?;
    let enc = encoding_arg(vm, "mb_strtolower", args, 1, "encoding")?;
    Ok(pstring(convert_case_cur(CaseMode::Lower, &s, enc)))
}

fn ulcfirst(vm: &mut Vm, args: &[Value], func: &str, mode: CaseMode) -> Result<Value, VmError> {
    let s = str_arg(vm, func, args, 0, "string")?;
    let enc = encoding_arg(vm, func, args, 1, "encoding")?;
    let first = get_substr(&s, 0, 1, enc);
    let head = convert_case_cur(mode, &first, enc);
    if first == head {
        return Ok(pstring(s));
    }
    let mut r = head;
    r.extend_from_slice(&get_substr(&s, 1, MBFL_SUBSTR_UNTIL_END, enc));
    Ok(pstring(r))
}

fn mb_ucfirst(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    ulcfirst(vm, args, "mb_ucfirst", CaseMode::Title)
}
fn mb_lcfirst(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    ulcfirst(vm, args, "mb_lcfirst", CaseMode::Lower)
}

/* ===== Trim ===== */

const MB_LTRIM: u8 = 1;
const MB_RTRIM: u8 = 2;

fn trim_each_wchar(s: &[u8], is_trim: &dyn Fn(u32) -> bool, mut mode: u8, enc: &Encoding) -> Vec<u8> {
    let w = to_wchars(enc, s);
    let total = w.len();
    let mut left = 0;
    let mut right = 0;
    for &c in &w {
        if is_trim(c) {
            if mode & MB_LTRIM != 0 {
                left += 1;
            }
            if mode & MB_RTRIM != 0 {
                right += 1;
            }
        } else {
            mode &= !MB_LTRIM;
            if mode & MB_RTRIM != 0 {
                right = 0;
            }
        }
    }
    if left == 0 && right == 0 {
        return s.to_vec();
    }
    get_substr(s, left, total - (right + left), enc)
}

fn do_trim(vm: &mut Vm, args: &[Value], func: &str, mode: u8) -> Result<Value, VmError> {
    let s = str_arg(vm, func, args, 0, "string")?;
    let what = str_or_null_arg(vm, func, args, 1, "characters")?;
    let enc = encoding_arg(vm, func, args, 2, "encoding")?;
    let r = match what {
        Some(what) => {
            if what.is_empty() {
                s.clone()
            } else {
                let set: std::collections::HashSet<u32> = to_wchars(enc, &what).into_iter().collect();
                trim_each_wchar(&s, &|c| set.contains(&c), mode, enc)
            }
        }
        None => {
            const DEFAULT: [u32; 27] = [
                0x20, 0x0C, 0x0A, 0x0D, 0x09, 0x0B, 0x00, 0xA0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007,
                0x2008, 0x2009, 0x200A, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000, 0x85, 0x180E,
            ];
            trim_each_wchar(&s, &|c| DEFAULT.contains(&c), mode, enc)
        }
    };
    Ok(pstring(r))
}

fn mb_trim(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    do_trim(vm, args, "mb_trim", MB_LTRIM | MB_RTRIM)
}
fn mb_ltrim(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    do_trim(vm, args, "mb_ltrim", MB_LTRIM)
}
fn mb_rtrim(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    do_trim(vm, args, "mb_rtrim", MB_RTRIM)
}

/* ===== mb_str_split ===== */

fn mb_str_split(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_str_split";
    let s = str_arg(vm, f, args, 0, "string")?;
    let split_len = if present(args, 1) { long_arg(vm, f, args, 1, "length")? } else { 1 };
    if split_len <= 0 {
        return Err(arg_value_error(vm, f, 2, "length", "must be greater than 0"));
    } else if split_len > (u32::MAX / 4) as i64 {
        return Err(arg_value_error(vm, f, 2, "length", "is too large"));
    }
    let enc = encoding_arg(vm, f, args, 2, "encoding")?;
    let mut arr = PhpArray::new();
    if s.is_empty() {
        return Ok(new_array(arr));
    }
    let split_len = split_len as usize;
    let char_len = (enc.flag & (MBFL_ENCTYPE_SBCS | MBFL_ENCTYPE_WCS2 | MBFL_ENCTYPE_WCS4)) as usize;
    if char_len != 0 {
        let chunk_len = char_len * split_len;
        for c in s.chunks(chunk_len) {
            arr.push(pstr(c));
        }
    } else if let Some(mbtab) = enc.mblen_table {
        let e = s.len();
        let mut p = 0;
        while p < e {
            let chunk = p;
            let mut n = 0;
            while n < split_len && p < e {
                p += mbtab[s[p] as usize] as usize;
                n += 1;
            }
            if p > e {
                p = e;
            }
            arr.push(pstr(&s[chunk..p]));
        }
    } else {
        let w = to_wchars(enc, &s);
        for c in w.chunks(split_len) {
            arr.push(pstring(from_wchars(enc, c)));
        }
    }
    Ok(new_array(arr))
}

/* ===== mb_ord / mb_chr ===== */

/// C: `php_mb_is_unsupported_no_encoding`
pub fn is_unsupported_for_ord(no: EncNo) -> bool {
    matches!(
        no,
        EncNo::Pass
            | EncNo::Wchar
            | EncNo::Base64
            | EncNo::Uuencode
            | EncNo::HtmlEnt
            | EncNo::Qprint
            | EncNo::Utf7
            | EncNo::Utf7imap
            | EncNo::Jis
            | EncNo::Iso2022jp
            | EncNo::Iso2022jp2004
            | EncNo::Iso2022jpKddi
            | EncNo::Iso2022jpms
            | EncNo::Cp50220
            | EncNo::Cp50221
            | EncNo::Cp50222
    )
}

fn mb_ord(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_ord";
    let s = str_arg(vm, f, args, 0, "string")?;
    if s.is_empty() {
        return Err(arg_value_error(vm, f, 1, "string", "must not be empty"));
    }
    let enc = encoding_arg(vm, f, args, 1, "encoding")?;
    if is_unsupported_for_ord(enc.no) {
        return Err(value_error(vm, format!("mb_ord() does not support the \"{}\" encoding", enc.name)));
    }
    // Decoding a bounded prefix is enough to get the first character
    let prefix = &s[..s.len().min(16)];
    let w = to_wchars(enc, prefix);
    match w.first() {
        None => Ok(Value::False),
        Some(&c) if c == MBFL_BAD_INPUT => Ok(Value::False),
        Some(&c) => Ok(Value::Long(c as i64)),
    }
}

fn mb_chr(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_chr";
    let cp = long_arg(vm, f, args, 0, "codepoint")?;
    let enc = encoding_arg(vm, f, args, 1, "encoding")?;
    if is_unsupported_for_ord(enc.no) {
        return Err(value_error(vm, format!("mb_chr() does not support the \"{}\" encoding", enc.name)));
    }
    if !(0..=0x10ffff).contains(&cp) {
        return Ok(Value::False);
    }
    if is_utf8_family(enc.no) {
        if cp > 0xd7ff && cp < 0xe000 {
            return Ok(Value::False);
        }
        let mut out = Vec::new();
        mbfl::basic::push_utf8(&mut out, cp as u32);
        return Ok(pstring(out));
    }
    let (repl, mode) = state::illegal_settings();
    // C converts from UCS-4BE, so U+D800-DFFF etc. come through as-is
    let (out, errors) = enc.encode(&[cp as u32], repl, mode);
    if errors != 0 {
        return Ok(Value::False);
    }
    Ok(pstring(out))
}

/* ===== mb_str_pad ===== */

fn mb_str_pad(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_str_pad";
    let input = str_arg(vm, f, args, 0, "string")?;
    let pad_to_length = long_arg(vm, f, args, 1, "length")?;
    let pad = if present(args, 2) { str_arg(vm, f, args, 2, "pad_string")? } else { b" ".to_vec() };
    let pad_type = if present(args, 3) { long_arg(vm, f, args, 3, "pad_type")? } else { 1 };
    let enc = encoding_arg(vm, f, args, 4, "encoding")?;
    let input_length = get_strlen(&input, enc);
    if pad_to_length < 0 || (pad_to_length as usize) <= input_length {
        return Ok(pstring(input));
    }
    if pad.is_empty() {
        return Err(arg_value_error(vm, f, 3, "pad_string", "must not be empty"));
    }
    if !(0..=2).contains(&pad_type) {
        return Err(arg_value_error(vm, f, 4, "pad_type", "must be STR_PAD_LEFT, STR_PAD_RIGHT, or STR_PAD_BOTH"));
    }
    let pad_length = get_strlen(&pad, enc);
    if pad_length == 0 {
        return Err(arg_value_error(vm, f, 3, "pad_string", "must not be empty"));
    }
    let num = pad_to_length as usize - input_length;
    let (left_pad, right_pad) = match pad_type {
        1 => (0, num),
        0 => (num, 0),
        _ => (num / 2, num - num / 2),
    };
    let build = |n: usize| -> Vec<u8> {
        let mut v = Vec::new();
        for _ in 0..(n / pad_length) {
            v.extend_from_slice(&pad);
        }
        v.extend_from_slice(&get_substr(&pad, 0, n % pad_length, enc));
        v
    };
    let mut out = build(left_pad);
    out.extend_from_slice(&input);
    out.extend_from_slice(&build(right_pad));
    Ok(pstring(out))
}

fn mb_scrub(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = str_arg(vm, "mb_scrub", args, 0, "string")?;
    let enc = encoding_arg(vm, "mb_scrub", args, 1, "encoding")?;
    if enc.no == EncNo::Utf8 && std::str::from_utf8(&s).is_ok() {
        return Ok(pstring(s));
    }
    Ok(pstring(convert_ex(&s, enc, enc)))
}
