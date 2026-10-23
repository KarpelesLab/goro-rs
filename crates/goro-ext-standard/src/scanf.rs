//! Port of php-src ext/standard/scanf.c (`ValidateFormat`,
//! `php_sscanf_internal`, `BuildCharSet`, `CharInSet`) plus the
//! `sscanf()` / `fscanf()` function wrappers (string.c / file.c).
//!
//! Strings are handled as C strings, exactly like php-src: scanning stops at
//! the first NUL byte of either the input or the format.

use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

const SCAN_NOSKIP: u32 = 0x1;
const SCAN_SUPPRESS: u32 = 0x2;
const SCAN_UNSIGNED: u32 = 0x4;
const SCAN_WIDTH: u32 = 0x8;
const SCAN_SIGNOK: u32 = 0x10;
const SCAN_NODIGITS: u32 = 0x20;
const SCAN_NOZERO: u32 = 0x40;
const SCAN_XOK: u32 = 0x80;
const SCAN_PTOK: u32 = 0x100;
const SCAN_EXPOK: u32 = 0x200;

const SCAN_MAX_ARGS: i32 = 0xFF;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"sscanf", sscanf_fn);
    vm.register_function(b"fscanf", fscanf_fn);
}

/// A NUL-terminated view over a byte string.
struct CStr<'a>(&'a [u8]);

impl<'a> CStr<'a> {
    fn new(b: &'a [u8]) -> Self {
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        CStr(&b[..end])
    }
    #[inline]
    fn at(&self, i: usize) -> u8 {
        if i < self.0.len() { self.0[i] } else { 0 }
    }
    /// `strtoul(s + i, &end, 10)` for a run of decimal digits (saturating).
    fn strtoul(&self, mut i: usize) -> (u64, usize) {
        let mut v: u64 = 0;
        let mut overflow = false;
        while self.at(i).is_ascii_digit() {
            let d = (self.at(i) - b'0') as u64;
            match v.checked_mul(10).and_then(|x| x.checked_add(d)) {
                Some(n) => v = n,
                None => overflow = true,
            }
            i += 1;
        }
        (if overflow { u64::MAX } else { v }, i)
    }
}

#[inline]
fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// Port of `ValidateFormat`. Returns the total number of substitutions or
/// the ValueError message.
fn validate_format(fmt: &CStr, num_vars: i32) -> Result<i32, String> {
    const STATIC_LIST_SIZE: usize = 16;
    let mut num_vars = num_vars;
    let mut nspace = STATIC_LIST_SIZE.max(num_vars.max(0) as usize);
    let mut nassign: Vec<i32> = vec![0; nspace];
    let mut xpg_size: i32 = 0;
    let mut obj_index: i32 = 0;
    let mut got_xpg = false;
    let mut got_sequential = false;
    let mut p = 0usize;

    let bad_index = |got_xpg: bool| -> String {
        if got_xpg {
            "\"%n$\" argument index out of range".to_string()
        } else {
            "Different numbers of variable names and field specifiers".to_string()
        }
    };
    let mixed = || "cannot mix \"%\" and \"%n$\" conversion specifiers".to_string();

    while fmt.at(p) != 0 {
        let mut ch = p;
        p += 1;
        let mut flags: u32 = 0;

        if fmt.at(ch) != b'%' {
            continue;
        }
        ch = p;
        p += 1;
        if fmt.at(ch) == b'%' {
            continue;
        }
        let mut xpg_done = false;
        if fmt.at(ch) == b'*' {
            flags |= SCAN_SUPPRESS;
            ch = p;
            p += 1;
            xpg_done = true;
        } else if fmt.at(ch).is_ascii_digit() {
            let (v, end) = fmt.strtoul(p - 1);
            let value = v as i32;
            if fmt.at(end) == b'$' {
                p = end + 1;
                ch = p;
                p += 1;
                got_xpg = true;
                if got_sequential {
                    return Err(mixed());
                }
                if value < 1 || (num_vars != 0 && value > num_vars) {
                    return Err(bad_index(got_xpg));
                } else if num_vars == 0 {
                    if value > SCAN_MAX_ARGS {
                        return Err(bad_index(got_xpg));
                    }
                    xpg_size = xpg_size.max(value);
                }
                obj_index = value - 1;
                xpg_done = true;
            }
        }
        if !xpg_done {
            // notXpg
            got_sequential = true;
            if got_xpg {
                return Err(mixed());
            }
        }

        // xpgCheckDone: parse any width specifier.
        if fmt.at(ch).is_ascii_digit() {
            let (_, end) = fmt.strtoul(p - 1);
            p = end;
            flags |= SCAN_WIDTH;
            ch = p;
            p += 1;
        }
        // Ignore size specifier.
        if matches!(fmt.at(ch), b'l' | b'L' | b'h') {
            ch = p;
            p += 1;
        }

        if flags & SCAN_SUPPRESS == 0 && num_vars != 0 && obj_index >= num_vars {
            return Err(bad_index(got_xpg));
        }

        match fmt.at(ch) {
            b'n' | b'd' | b'D' | b'i' | b'o' | b'x' | b'X' | b'u' | b'f' | b'e' | b'E' | b'g'
            | b's' | b'c' => {}
            b'[' => {
                let bad_set = || "Unmatched [ in format string".to_string();
                if fmt.at(p) == 0 {
                    return Err(bad_set());
                }
                ch = p;
                p += 1;
                if fmt.at(ch) == b'^' {
                    if fmt.at(p) == 0 {
                        return Err(bad_set());
                    }
                    ch = p;
                    p += 1;
                }
                if fmt.at(ch) == b']' {
                    if fmt.at(p) == 0 {
                        return Err(bad_set());
                    }
                    ch = p;
                    p += 1;
                }
                while fmt.at(ch) != b']' {
                    if fmt.at(p) == 0 {
                        return Err(bad_set());
                    }
                    ch = p;
                    p += 1;
                }
            }
            c => {
                // The message is built as a C string: a NUL conversion
                // character truncates it.
                let mut msg = "Bad scan conversion character \"".to_string();
                if c != 0 {
                    msg.push(c as char);
                    msg.push('"');
                }
                return Err(msg);
            }
        }

        if flags & SCAN_SUPPRESS == 0 {
            if obj_index < 0 {
                return Err(bad_index(got_xpg));
            }
            let oi = obj_index as usize;
            if oi >= nspace {
                if xpg_size > 0 {
                    nspace = xpg_size as usize;
                } else {
                    nspace += STATIC_LIST_SIZE;
                }
                if oi >= nspace {
                    nspace = oi + 1;
                }
                nassign.resize(nspace, 0);
            }
            nassign[oi] += 1;
            obj_index += 1;
        }
    }

    if num_vars == 0 {
        num_vars = if xpg_size != 0 { xpg_size } else { obj_index };
    }
    for i in 0..num_vars.max(0) as usize {
        let n = nassign.get(i).copied().unwrap_or(0);
        if n > 1 {
            return Err(
                "Variable is assigned by multiple \"%n$\" conversion specifiers".to_string(),
            );
        } else if xpg_size == 0 && n == 0 {
            return Err("Variable is not assigned by any conversion specifiers".to_string());
        }
    }
    Ok(num_vars)
}

struct CharSet {
    exclude: bool,
    chars: Vec<i8>,
    ranges: Vec<(i8, i8)>,
}

/// Port of `BuildCharSet`; `format` points just after the '['. Returns the
/// charset and the format position after the closing ']'.
fn build_char_set(fmt: &CStr, mut format: usize) -> (CharSet, usize) {
    let mut cset = CharSet {
        exclude: false,
        chars: Vec::new(),
        ranges: Vec::new(),
    };
    let f = |i: usize| fmt.at(i);
    if f(format) == b'^' {
        cset.exclude = true;
        format += 1;
    }
    // (php-src first scans ahead to size its buffers; not needed here.)
    let mut ch = format;
    format += 1;
    let mut start = f(ch) as i8;
    if f(ch) == b']' || f(ch) == b'-' {
        cset.chars.push(f(ch) as i8);
        ch = format;
        format += 1;
    }
    while f(ch) != b']' {
        if f(ch) == 0 {
            // Unreachable for validated formats; guard against looping.
            break;
        }
        if f(format) == b'-' {
            // This may be the first character of a range.
            start = f(ch) as i8;
        } else if f(ch) == b'-' {
            if f(format) == b']' {
                cset.chars.push(start);
                cset.chars.push(f(ch) as i8);
            } else {
                ch = format;
                format += 1;
                let c = f(ch) as i8;
                if start < c {
                    cset.ranges.push((start, c));
                } else {
                    cset.ranges.push((c, start));
                }
            }
        } else {
            cset.chars.push(f(ch) as i8);
        }
        ch = format;
        format += 1;
    }
    (cset, format)
}

fn char_in_set(cset: &CharSet, c: u8) -> bool {
    let ch = c as i8;
    let mut matched = cset.chars.contains(&ch);
    if !matched {
        matched = cset.ranges.iter().any(|&(s, e)| s <= ch && ch <= e);
    }
    if cset.exclude { !matched } else { matched }
}

/// C `strtol(buf, NULL, base)` / `strtoul` over an already-validated
/// numeric buffer.
fn c_strtol(buf: &[u8], base: u32, unsigned: bool) -> i64 {
    let mut i = 0;
    let mut neg = false;
    if i < buf.len() && (buf[i] == b'+' || buf[i] == b'-') {
        neg = buf[i] == b'-';
        i += 1;
    }
    if base == 16
        && i + 1 < buf.len()
        && buf[i] == b'0'
        && (buf[i + 1] == b'x' || buf[i + 1] == b'X')
    {
        if i + 2 < buf.len() && (buf[i + 2] as char).is_ascii_hexdigit() {
            i += 2;
        }
    }
    let mut mag: u64 = 0;
    let mut overflow = false;
    while i < buf.len() {
        let d = match (buf[i] as char).to_digit(base) {
            Some(d) => d as u64,
            None => break,
        };
        match mag.checked_mul(base as u64).and_then(|x| x.checked_add(d)) {
            Some(n) => mag = n,
            None => overflow = true,
        }
        i += 1;
    }
    if unsigned {
        if overflow {
            return u64::MAX as i64;
        }
        let v = if neg { mag.wrapping_neg() } else { mag };
        v as i64
    } else if neg {
        if overflow || mag > (i64::MAX as u64) + 1 {
            i64::MIN
        } else {
            (mag as i64).wrapping_neg()
        }
    } else if overflow || mag > i64::MAX as u64 {
        i64::MAX
    } else {
        mag as i64
    }
}

/// Outcome of `php_sscanf_internal`.
enum ScanResult {
    Value(Value),
    Error(String),
}

/// Port of `php_sscanf_internal` (with varStart == 0). `vars` are the
/// by-reference output arguments.
fn php_sscanf_internal(string: &[u8], format: &[u8], vars: &[Value]) -> ScanResult {
    let s = CStr::new(string);
    let fmt = CStr::new(format);
    let num_vars = vars.len() as i32;
    let arg_count = num_vars;
    let var_start: i32 = 0;

    let total_vars = match validate_format(&fmt, num_vars) {
        Ok(t) => t,
        Err(msg) => return ScanResult::Error(msg),
    };

    let mut obj_index: i32 = if num_vars != 0 { var_start } else { 0 };
    let mut results: Vec<Value> = Vec::new();
    if num_vars == 0 {
        results = vec![Value::Null; total_vars.max(0) as usize];
    }

    let assign = |results: &mut Vec<Value>, obj_index: &mut i32, v: Value| {
        if num_vars != 0 {
            if let Some(Value::Reference(r)) = vars.get(*obj_index as usize) {
                *r.borrow_mut() = v;
            }
        } else if *obj_index >= 0 {
            let idx = *obj_index as usize;
            if idx >= results.len() {
                results.resize(idx + 1, Value::Null);
            }
            results[idx] = v;
        }
        *obj_index += 1;
    };

    let mut sp = 0usize; // string position
    let mut p = 0usize; // format position
    let mut nconversions: i32 = 0;
    let mut underflow = false;
    let mut op: u8 = 0;
    let mut base: u32 = 0;
    let mut unsigned_fn = false;

    'done: while fmt.at(p) != 0 {
        let mut ch = p;
        p += 1;
        let mut flags: u32 = 0;

        // Whitespace in the format skips whitespace in the string.
        if is_c_space(fmt.at(ch)) {
            while is_c_space(s.at(sp)) {
                sp += 1;
            }
            continue;
        }

        let mut is_literal = fmt.at(ch) != b'%';
        if !is_literal {
            ch = p;
            p += 1;
            if fmt.at(ch) == b'%' {
                is_literal = true;
            }
        }
        if is_literal {
            if s.at(sp) == 0 {
                underflow = true;
                break 'done;
            }
            let sch = s.at(sp);
            sp += 1;
            if fmt.at(ch) != sch {
                break 'done;
            }
            continue;
        }

        // Assignment suppression or XPG3-style '%n$'.
        if fmt.at(ch) == b'*' {
            flags |= SCAN_SUPPRESS;
            ch = p;
            p += 1;
        } else if fmt.at(ch).is_ascii_digit() {
            let (v, end) = fmt.strtoul(p - 1);
            if fmt.at(end) == b'$' {
                p = end + 1;
                ch = p;
                p += 1;
                obj_index = (var_start as i64).wrapping_add(v as i64).wrapping_sub(1) as i32;
            }
        }

        // Width.
        let mut width: usize;
        if fmt.at(ch).is_ascii_digit() {
            let (w, end) = fmt.strtoul(p - 1);
            width = w as usize;
            p = end;
            ch = p;
            p += 1;
        } else {
            width = 0;
        }

        // Ignore size specifier.
        if matches!(fmt.at(ch), b'l' | b'L' | b'h') {
            ch = p;
            p += 1;
        }

        match fmt.at(ch) {
            b'n' => {
                if flags & SCAN_SUPPRESS == 0 && !(num_vars != 0 && obj_index >= arg_count) {
                    assign(&mut results, &mut obj_index, Value::Long(sp as i64));
                }
                nconversions += 1;
                continue;
            }
            b'd' | b'D' => {
                op = b'i';
                base = 10;
                unsigned_fn = false;
            }
            b'i' => {
                op = b'i';
                base = 0;
                unsigned_fn = false;
            }
            b'o' => {
                op = b'i';
                base = 8;
                unsigned_fn = false;
            }
            b'x' | b'X' => {
                op = b'i';
                base = 16;
                unsigned_fn = false;
            }
            b'u' => {
                op = b'i';
                base = 10;
                flags |= SCAN_UNSIGNED;
                unsigned_fn = true;
            }
            b'f' | b'e' | b'E' | b'g' => {
                op = b'f';
            }
            b's' => {
                op = b's';
            }
            b'c' => {
                op = b's';
                flags |= SCAN_NOSKIP;
                if width == 0 {
                    width = 1;
                }
            }
            b'[' => {
                op = b'[';
                flags |= SCAN_NOSKIP;
            }
            _ => {}
        }

        // We need more characters from the string to proceed.
        if s.at(sp) == 0 {
            underflow = true;
            break 'done;
        }

        // Skip leading whitespace unless suppressed by the conversion.
        if flags & SCAN_NOSKIP == 0 {
            while s.at(sp) != 0 && is_c_space(s.at(sp)) {
                sp += 1;
            }
            if s.at(sp) == 0 {
                underflow = true;
                break 'done;
            }
        }

        match op {
            b'c' | b's' => {
                if width == 0 {
                    width = usize::MAX;
                }
                let mut end = sp;
                while s.at(end) != 0 {
                    if is_c_space(s.at(end)) {
                        break;
                    }
                    end += 1;
                    width -= 1;
                    if width == 0 {
                        break;
                    }
                }
                if flags & SCAN_SUPPRESS == 0 && !(num_vars != 0 && obj_index >= arg_count) {
                    let v = Value::String(PhpString::from_bytes(&s.0[sp..end]));
                    assign(&mut results, &mut obj_index, v);
                }
                sp = end;
            }
            b'[' => {
                if width == 0 {
                    width = usize::MAX;
                }
                let mut end = sp;
                let (cset, np) = build_char_set(&fmt, p);
                p = np;
                while s.at(end) != 0 {
                    if !char_in_set(&cset, s.at(end)) {
                        break;
                    }
                    end += 1;
                    width -= 1;
                    if width == 0 {
                        break;
                    }
                }
                if sp == end {
                    // Nothing matched the range, stop processing.
                    break 'done;
                }
                if flags & SCAN_SUPPRESS == 0 && !(num_vars != 0 && obj_index >= arg_count) {
                    let v = Value::String(PhpString::from_bytes(&s.0[sp..end]));
                    assign(&mut results, &mut obj_index, v);
                }
                sp = end;
            }
            b'i' => {
                // Scan an unsigned or signed integer.
                const BUFSZ: usize = 64;
                if width == 0 || width > BUFSZ - 1 {
                    width = BUFSZ - 1;
                }
                let mut buf: Vec<u8> = Vec::with_capacity(BUFSZ);
                flags |= SCAN_SIGNOK | SCAN_NODIGITS | SCAN_NOZERO;
                while width > 0 {
                    let c = s.at(sp);
                    let accept = match c {
                        b'0' => {
                            if base == 16 {
                                flags |= SCAN_XOK;
                            }
                            if base == 0 {
                                base = 8;
                                flags |= SCAN_XOK;
                            }
                            if flags & SCAN_NOZERO != 0 {
                                flags &= !(SCAN_SIGNOK | SCAN_NODIGITS | SCAN_NOZERO);
                            } else {
                                flags &= !(SCAN_SIGNOK | SCAN_XOK | SCAN_NODIGITS);
                            }
                            true
                        }
                        b'1'..=b'7' => {
                            if base == 0 {
                                base = 10;
                            }
                            flags &= !(SCAN_SIGNOK | SCAN_XOK | SCAN_NODIGITS);
                            true
                        }
                        b'8' | b'9' => {
                            if base == 0 {
                                base = 10;
                            }
                            if base <= 8 {
                                false
                            } else {
                                flags &= !(SCAN_SIGNOK | SCAN_XOK | SCAN_NODIGITS);
                                true
                            }
                        }
                        b'A'..=b'F' | b'a'..=b'f' => {
                            if base <= 10 {
                                false
                            } else {
                                flags &= !(SCAN_SIGNOK | SCAN_XOK | SCAN_NODIGITS);
                                true
                            }
                        }
                        b'+' | b'-' => {
                            if flags & SCAN_SIGNOK != 0 {
                                flags &= !SCAN_SIGNOK;
                                true
                            } else {
                                false
                            }
                        }
                        b'x' | b'X' => {
                            if flags & SCAN_XOK != 0 && buf.len() == 1 {
                                base = 16;
                                flags &= !SCAN_XOK;
                                true
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };
                    if !accept {
                        break;
                    }
                    buf.push(c);
                    sp += 1;
                    if s.at(sp) == 0 {
                        break;
                    }
                    width -= 1;
                }

                // Back up if we only got a sign or a trailing x after a 0.
                if flags & SCAN_NODIGITS != 0 {
                    if s.at(sp) == 0 {
                        underflow = true;
                    }
                    break 'done;
                } else if matches!(buf.last(), Some(b'x') | Some(b'X')) {
                    buf.pop();
                    sp -= 1;
                }

                if flags & SCAN_SUPPRESS == 0 {
                    let value = c_strtol(&buf, if base == 0 { 10 } else { base }, unsigned_fn);
                    if num_vars != 0 && obj_index >= arg_count {
                        // php-src: `break` out of the op switch.
                    } else if flags & SCAN_UNSIGNED != 0 && value < 0 {
                        let v = Value::String(PhpString::from_string((value as u64).to_string()));
                        assign(&mut results, &mut obj_index, v);
                    } else {
                        assign(&mut results, &mut obj_index, Value::Long(value));
                    }
                }
            }
            b'f' => {
                // Scan a floating point number.
                const BUFSZ: usize = 64;
                if width == 0 || width > BUFSZ - 1 {
                    width = BUFSZ - 1;
                }
                let mut buf: Vec<u8> = Vec::with_capacity(BUFSZ);
                flags |= SCAN_SIGNOK | SCAN_NODIGITS | SCAN_PTOK | SCAN_EXPOK;
                while width > 0 {
                    let c = s.at(sp);
                    let accept = match c {
                        b'0'..=b'9' => {
                            flags &= !(SCAN_SIGNOK | SCAN_NODIGITS);
                            true
                        }
                        b'+' | b'-' => {
                            if flags & SCAN_SIGNOK != 0 {
                                flags &= !SCAN_SIGNOK;
                                true
                            } else {
                                false
                            }
                        }
                        b'.' => {
                            if flags & SCAN_PTOK != 0 {
                                flags &= !(SCAN_SIGNOK | SCAN_PTOK);
                                true
                            } else {
                                false
                            }
                        }
                        b'e' | b'E' => {
                            // An exponent is not allowed until there has been at least one digit.
                            if flags & (SCAN_NODIGITS | SCAN_EXPOK) == SCAN_EXPOK {
                                flags = (flags & !(SCAN_EXPOK | SCAN_PTOK))
                                    | SCAN_SIGNOK
                                    | SCAN_NODIGITS;
                                true
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };
                    if !accept {
                        break;
                    }
                    buf.push(c);
                    sp += 1;
                    if s.at(sp) == 0 {
                        break;
                    }
                    width -= 1;
                }

                // Back up if we saw a trailing 'e' or sign.
                if flags & SCAN_NODIGITS != 0 {
                    if flags & SCAN_EXPOK != 0 {
                        // No digits at all: scanning failed.
                        if s.at(sp) == 0 {
                            underflow = true;
                        }
                        break 'done;
                    }
                    // Bad exponent ('e' and maybe a sign).
                    let last = buf.pop();
                    sp -= 1;
                    if last != Some(b'e') && last != Some(b'E') {
                        buf.pop();
                        sp -= 1;
                    }
                }

                if flags & SCAN_SUPPRESS == 0 {
                    let dvalue = zend_strtod(&buf);
                    if !(num_vars != 0 && obj_index >= arg_count) {
                        assign(&mut results, &mut obj_index, Value::Double(dvalue));
                    }
                }
            }
            _ => {}
        }
        nconversions += 1;
    }

    if underflow && nconversions == 0 {
        if num_vars != 0 {
            ScanResult::Value(Value::Long(-1))
        } else {
            ScanResult::Value(Value::Null)
        }
    } else if num_vars != 0 {
        ScanResult::Value(Value::Long(nconversions as i64))
    } else {
        let mut arr = PhpArray::new();
        for v in results {
            arr.push(v);
        }
        ScanResult::Value(Value::Array(Rc::new(RefCell::new(arr))))
    }
}

/// `zend_strtod` on a buffer of the shape `[sign] digits [. digits] [e [sign] digits]`.
fn zend_strtod(buf: &[u8]) -> f64 {
    let s = std::str::from_utf8(buf).unwrap_or("");
    if let Ok(v) = s.parse::<f64>() {
        return v;
    }
    // Rust rejects forms like "1." followed by an exponent or a bare ".";
    // normalize by inserting zeros around the decimal point.
    let mut t = String::with_capacity(s.len() + 2);
    let bytes = s.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if c == b'.' {
            let prev_digit = i > 0 && bytes[i - 1].is_ascii_digit();
            if !prev_digit {
                t.push('0');
            }
            t.push('.');
            let next_digit = i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit();
            if !next_digit {
                t.push('0');
            }
        } else {
            t.push(c as char);
        }
    }
    t.parse::<f64>().unwrap_or(0.0)
}

fn throw(vm: &mut Vm, class: &[u8], msg: String) -> VmError {
    let exc = vm.create_exception(class, &msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg,
        line: vm.current_line,
    }
}

/// `Z_PARAM_STRING` coercion for a required string parameter.
pub(crate) fn zpp_string(
    vm: &mut Vm,
    func: &str,
    argnum: usize,
    pname: &str,
    val: &Value,
) -> Result<PhpString, VmError> {
    let v = val.deref();
    match &v {
        Value::String(s) => Ok(s.clone()),
        Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
            Ok(vm.value_to_string(&v))
        }
        Value::Null | Value::Undef => {
            let line = vm.current_line;
            vm.emit_deprecated_at(
                &format!(
                    "{}(): Passing null to parameter #{} (${}) of type string is deprecated",
                    func, argnum, pname
                ),
                line,
            );
            Ok(PhpString::from_bytes(b""))
        }
        Value::Object(obj) => {
            let class_lower: Vec<u8> = obj
                .borrow()
                .class_name
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
            let has_tostring = vm
                .classes
                .get(&class_lower)
                .map(|c| c.methods.contains_key(&b"__tostring".to_vec()))
                .unwrap_or(false);
            if has_tostring {
                vm.value_to_string_checked(&v)
            } else {
                let msg = format!(
                    "{}(): Argument #{} (${}) must be of type string, {} given",
                    func,
                    argnum,
                    pname,
                    Vm::value_type_name(&v)
                );
                let exc = vm.throw_type_error(msg.clone());
                vm.current_exception = Some(exc);
                Err(VmError {
                    message: msg,
                    line: vm.current_line,
                })
            }
        }
        _ => {
            let msg = format!(
                "{}(): Argument #{} (${}) must be of type string, {} given",
                func,
                argnum,
                pname,
                Vm::value_type_name(&v)
            );
            let exc = vm.throw_type_error(msg.clone());
            vm.current_exception = Some(exc);
            Err(VmError {
                message: msg,
                line: vm.current_line,
            })
        }
    }
}

fn arg_count_error(vm: &mut Vm, func: &str, given: usize) -> VmError {
    let msg = format!("{}() expects at least 2 arguments, {} given", func, given);
    throw(vm, b"ArgumentCountError", msg)
}

fn run_scan(vm: &mut Vm, input: &[u8], format: &[u8], vars: &[Value]) -> Result<Value, VmError> {
    match php_sscanf_internal(input, format, vars) {
        ScanResult::Value(v) => Ok(v),
        ScanResult::Error(msg) => Err(throw(vm, b"ValueError", msg)),
    }
}

/// PHP_FUNCTION(sscanf)
pub fn sscanf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Err(arg_count_error(vm, "sscanf", args.len()));
    }
    let input = zpp_string(vm, "sscanf", 1, "string", &args[0])?;
    let format = zpp_string(vm, "sscanf", 2, "format", &args[1])?;
    run_scan(vm, input.as_bytes(), format.as_bytes(), &args[2..])
}

/// PHP_FUNCTION(fscanf)
pub fn fscanf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() < 2 {
        return Err(arg_count_error(vm, "fscanf", args.len()));
    }
    // Z_PARAM_RESOURCE
    let handle = args[0].deref();
    if !matches!(handle, Value::Resource(..)) {
        let msg = format!(
            "fscanf(): Argument #1 ($stream) must be of type resource, {} given",
            Vm::value_type_name(&handle)
        );
        let exc = vm.throw_type_error(msg.clone());
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let format = zpp_string(vm, "fscanf", 2, "format", &args[1])?;
    // zend_fetch_resource2(..., "File-Handle", ...)
    if !crate::misc::has_file_handle(handle.to_long()) {
        let msg = "fscanf(): supplied resource is not a valid File-Handle resource".to_string();
        let exc = vm.throw_type_error(msg.clone());
        vm.current_exception = Some(exc);
        return Err(VmError {
            message: msg,
            line: vm.current_line,
        });
    }
    let line = match crate::misc::stream_get_line(vm, "fscanf", &handle, None) {
        Some(l) => l,
        None => return Ok(Value::False),
    };
    run_scan(vm, &line, format.as_bytes(), &args[2..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arr(v: Value) -> Vec<Value> {
        match v {
            Value::Array(a) => a.borrow().iter().map(|(_, v)| v.clone()).collect(),
            _ => panic!("not array"),
        }
    }

    #[test]
    fn validate() {
        assert_eq!(validate_format(&CStr::new(b"%d%s"), 0), Ok(2));
        assert!(validate_format(&CStr::new(b"%"), 0).is_err());
        assert_eq!(
            validate_format(&CStr::new(b"%d%s%f"), 2),
            Err("Different numbers of variable names and field specifiers".to_string())
        );
        assert_eq!(validate_format(&CStr::new(b"%2$s %1$s"), 0), Ok(2));
    }

    #[test]
    fn scan() {
        match php_sscanf_internal(b"age: 12 name: bob", b"age: %d name: %s", &[]) {
            ScanResult::Value(v) => {
                let a = arr(v);
                assert!(matches!(a[0], Value::Long(12)));
            }
            _ => panic!(),
        }
        match php_sscanf_internal(b"18446744073709551615", b"%u", &[]) {
            ScanResult::Value(v) => {
                let a = arr(v);
                assert_eq!(a[0].to_php_string().as_bytes(), b"18446744073709551615");
            }
            _ => panic!(),
        }
    }
}
