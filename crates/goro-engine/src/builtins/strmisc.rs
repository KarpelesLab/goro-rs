//! More string functions of the standard extension: str_getcsv(),
//! sscanf(), pack() and unpack() (PHP manual pages of these functions).
//!
//! Details (whitespace and enclosure handling in CSV fields, scan
//! conversions, pack codes and their diagnostics) were observed on PHP 8.5
//! and checked against tests ext/standard/tests/strings/str_getcsv*.phpt,
//! sscanf*.phpt, pack*.phpt and unpack*.phpt.

use goro_runtime::{Key, PhpArray, Value};

use super::string::sv;
use crate::bytecode::CastTo;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function str_getcsv(string $string, string $separator = \",\", string $enclosure = \"\\\"\", string $escape = \"\\\\\"): array",
            str_getcsv,
        ),
        (
            "function sscanf(string $string, string $format, mixed &...$vars): array|int|null",
            sscanf,
        ),
        (
            "function pack(string $format, mixed ...$values): string",
            pack,
        ),
        (
            "function unpack(string $format, string $string, int $offset = 0): array|false",
            unpack,
        ),
    ],
    listed: false,
    ..Module::EMPTY
};

// ---------------------------------------------------------------- CSV

/// Parses one CSV record (str_getcsv(), and fgetcsv() on a read line).
/// `escape` is None for no escape character. A blank record is `[null]`.
pub fn parse_csv(line: &[u8], delim: u8, encl: u8, escape: Option<u8>) -> Vec<Option<Vec<u8>>> {
    fn strip_eol(b: &mut Vec<u8>) {
        if b.last() == Some(&b'\n') {
            b.pop();
            if b.last() == Some(&b'\r') {
                b.pop();
            }
        } else if b.last() == Some(&b'\r') {
            b.pop();
        }
    }
    let n = line.len();
    {
        let mut t = line.to_vec();
        strip_eol(&mut t);
        if t.is_empty() {
            return vec![None];
        }
    }
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let mut skip = pos;
        while skip < n && (line[skip] == b' ' || line[skip] == b'\t') && line[skip] != delim {
            skip += 1;
        }
        let mut field = Vec::new();
        if skip < n && line[skip] == encl {
            pos = skip + 1;
            let mut closed = false;
            while pos < n {
                let c = line[pos];
                if Some(c) == escape && c != encl {
                    field.push(c);
                    if pos + 1 < n {
                        field.push(line[pos + 1]);
                    }
                    pos += 2;
                    continue;
                }
                if c == encl {
                    if pos + 1 < n && line[pos + 1] == encl {
                        field.push(encl);
                        pos += 2;
                        continue;
                    }
                    pos += 1;
                    closed = true;
                    break;
                }
                field.push(c);
                pos += 1;
            }
            if !closed && escape == Some(0) {
                // Test gh12151.phpt: with a NUL escape, an unterminated
                // field ends with a NUL.
                field.push(0);
            }
            if closed {
                // Text after the closing enclosure belongs to the field.
                let tail_start = field.len();
                while pos < n && line[pos] != delim {
                    field.push(line[pos]);
                    pos += 1;
                }
                if pos >= n {
                    let mut tail = field.split_off(tail_start);
                    strip_eol(&mut tail);
                    field.extend_from_slice(&tail);
                }
            }
        } else {
            let start = pos;
            while pos < n && line[pos] != delim {
                pos += 1;
            }
            field.extend_from_slice(&line[start..pos]);
            if pos >= n {
                strip_eol(&mut field);
            }
        }
        out.push(Some(field));
        if pos >= n {
            break;
        }
        pos += 1;
        if pos >= n {
            out.push(Some(Vec::new()));
            break;
        }
    }
    out
}

fn str_getcsv(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let delim = if a.has(1) {
        a.bytes(1).to_vec()
    } else {
        b",".to_vec()
    };
    let encl = if a.has(2) {
        a.bytes(2).to_vec()
    } else {
        b"\"".to_vec()
    };
    if delim.len() != 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($separator) must be a single character",
        );
    }
    if encl.len() != 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($enclosure) must be a single character",
        );
    }
    let escape = if a.has(3) {
        let e = a.bytes(3);
        if e.len() > 1 {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #4 ($escape) must be empty or a single character",
            );
        }
        e.first().copied()
    } else {
        // Observed on PHP 8.5 (deprecated since 8.4).
        vm.fdeprecated(
            a,
            "the $escape parameter must be provided as its default value will change",
        )?;
        Some(b'\\')
    };
    let fields = parse_csv(a.bytes(0), delim[0], encl[0], escape);
    Ok(Value::Array(PhpArray::from_values(fields.into_iter().map(
        |f| match f {
            Some(b) => sv(b),
            None => Value::Null,
        },
    ))))
}

// ---------------------------------------------------------------- sscanf

enum Scanned {
    Int(i64),
    Float(f64),
    Str(Vec<u8>),
}

impl Scanned {
    fn into_value(self) -> Value {
        match self {
            Scanned::Int(i) => Value::Int(i),
            Scanned::Float(f) => Value::Float(f),
            Scanned::Str(s) => sv(s),
        }
    }
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// One conversion of the format, parsed.
struct Conv {
    suppress: bool,
    width: Option<usize>,
    argnum: Option<usize>,
    conv: u8,
    /// `%[...]` set (bytes accepted).
    set: Option<[bool; 256]>,
}

/// Parses a decimal/hex/octal integer prefix of `s` in `base` (0: auto),
/// at most `width` bytes. Returns (value as i128, consumed) or None.
fn scan_int(s: &[u8], width: usize, base: u32) -> Option<(i128, usize, bool)> {
    let lim = width.min(s.len());
    let mut i = 0;
    let mut neg = false;
    if i < lim && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut base = base;
    if base == 0 {
        if i + 1 < lim && s[i] == b'0' && (s[i + 1] == b'x' || s[i + 1] == b'X') {
            base = 16;
            i += 2;
        } else if i < lim && s[i] == b'0' {
            base = 8;
        } else {
            base = 10;
        }
    } else if base == 16 && i + 1 < lim && s[i] == b'0' && (s[i + 1] == b'x' || s[i + 1] == b'X') {
        i += 2;
    }
    let st = i;
    let mut v: i128 = 0;
    let mut overflow = false;
    while i < lim {
        let d = match s[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'a'..=b'f' => (c - b'a' + 10) as u32,
            c @ b'A'..=b'F' => (c - b'A' + 10) as u32,
            _ => 99,
        };
        if d >= base {
            break;
        }
        if !overflow {
            v = v * base as i128 + d as i128;
            if v > u64::MAX as i128 * 2 {
                overflow = true;
            }
        }
        i += 1;
    }
    if i == st {
        return None;
    }
    Some((if neg { -v } else { v }, i, overflow))
}

fn scan_float(s: &[u8], width: usize) -> Option<(f64, usize)> {
    let lim = width.min(s.len());
    let mut i = 0;
    if i < lim && (s[i] == b'+' || s[i] == b'-') {
        i += 1;
    }
    let ds = i;
    while i < lim && s[i].is_ascii_digit() {
        i += 1;
    }
    let mut nd = i - ds;
    if i < lim && s[i] == b'.' {
        i += 1;
        let fs = i;
        while i < lim && s[i].is_ascii_digit() {
            i += 1;
        }
        nd += i - fs;
    }
    if nd == 0 {
        return None;
    }
    if i < lim && (s[i] == b'e' || s[i] == b'E') {
        let mut j = i + 1;
        if j < lim && (s[j] == b'+' || s[j] == b'-') {
            j += 1;
        }
        let es = j;
        while j < lim && s[j].is_ascii_digit() {
            j += 1;
        }
        if j > es {
            i = j;
        }
    }
    let text = std::str::from_utf8(&s[..i]).ok()?;
    let v: f64 = text.parse().ok().or_else(|| {
        // "1." and ".5" forms.
        let t = text.trim_end_matches('.');
        t.parse().ok()
    })?;
    Some((v, i))
}

fn parse_format(vm: &mut Vm, fmt: &[u8]) -> R<Vec<Result<Conv, u8>>> {
    // Items: Ok(conversion) or Err(literal byte; b' ' stands for any
    // whitespace run).
    let mut items = Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        if is_ws(c) {
            while i < fmt.len() && is_ws(fmt[i]) {
                i += 1;
            }
            items.push(Err(b' '));
            continue;
        }
        if c != b'%' {
            items.push(Err(c));
            i += 1;
            continue;
        }
        i += 1;
        if i < fmt.len() && fmt[i] == b'%' {
            items.push(Err(b'%'));
            i += 1;
            continue;
        }
        let mut conv = Conv {
            suppress: false,
            width: None,
            argnum: None,
            conv: 0,
            set: None,
        };
        if i < fmt.len() && fmt[i] == b'*' {
            conv.suppress = true;
            i += 1;
        }
        // Digits: an argument number (followed by '$') or a width.
        let st = i;
        while i < fmt.len() && fmt[i].is_ascii_digit() {
            i += 1;
        }
        if i > st {
            let n: usize = std::str::from_utf8(&fmt[st..i])
                .unwrap()
                .parse()
                .unwrap_or(usize::MAX);
            if i < fmt.len() && fmt[i] == b'$' {
                i += 1;
                // Test gh15552.phpt; observed on PHP 8.5 for 0.
                if n == 0 || n > i32::MAX as usize {
                    return vm.throw("ValueError", "\"%n$\" argument index out of range");
                }
                conv.argnum = Some(n - 1);
                let st2 = i;
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    i += 1;
                }
                if i > st2 {
                    conv.width = std::str::from_utf8(&fmt[st2..i]).unwrap().parse().ok();
                }
            } else {
                conv.width = Some(n);
            }
        }
        while i < fmt.len() && matches!(fmt[i], b'h' | b'l' | b'L') {
            i += 1;
        }
        let Some(&cc) = fmt.get(i) else {
            // Observed on PHP 8.5: the message stops at the missing character.
            return vm.throw("ValueError", "Bad scan conversion character \"");
        };
        i += 1;
        match cc {
            b'd' | b'i' | b'o' | b'u' | b'x' | b'X' | b'f' | b'e' | b'E' | b'g' | b's' | b'c'
            | b'n' => conv.conv = cc,
            b'D' => conv.conv = b'd',
            b'O' => conv.conv = b'o',
            b'U' => conv.conv = b'u',
            b'[' => {
                let mut set = [false; 256];
                let neg = fmt.get(i) == Some(&b'^');
                if neg {
                    i += 1;
                }
                let mut first = true;
                let mut prev: Option<u8> = None;
                loop {
                    let Some(&b) = fmt.get(i) else {
                        return vm.throw("ValueError", "Unmatched [ in format string");
                    };
                    i += 1;
                    if b == b']' && !first {
                        break;
                    }
                    first = false;
                    if b == b'-'
                        && let Some(p) = prev
                        && fmt.get(i).is_some_and(|&n| n != b']')
                    {
                        let hi = fmt[i];
                        i += 1;
                        let (lo, hi) = if p <= hi { (p, hi) } else { (hi, p) };
                        for x in lo..=hi {
                            set[x as usize] = true;
                        }
                        prev = None;
                        continue;
                    }
                    set[b as usize] = true;
                    prev = Some(b);
                }
                if neg {
                    for x in set.iter_mut() {
                        *x = !*x;
                    }
                }
                conv.conv = b'[';
                conv.set = Some(set);
            }
            other => {
                return vm.throw(
                    "ValueError",
                    &format!(
                        "Bad scan conversion character \"{}\"",
                        String::from_utf8_lossy(&[other])
                    ),
                );
            }
        }
        items.push(Ok(conv));
    }
    // Observed on PHP 8.5: numbered and plain conversions don't mix.
    let counted = |numbered: bool| {
        items
            .iter()
            .flatten()
            .filter(|c| !c.suppress && c.conv != 0 && c.argnum.is_some() == numbered)
            .count()
    };
    if counted(true) > 0 && counted(false) > 0 {
        return vm.throw(
            "ValueError",
            "cannot mix \"%\" and \"%n$\" conversion specifiers",
        );
    }
    Ok(items)
}

fn sscanf(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    let fmt = a.str(1);
    scan_into(vm, a, &s, &fmt, 2)
}

/// sscanf() of `s` with `fmt`; the by-reference result variables, if
/// any, are the arguments from `first_var` on (fscanf() shares this).
pub(crate) fn scan_into(
    vm: &mut Vm,
    a: &mut Args,
    s: &[u8],
    fmt: &[u8],
    first_var: usize,
) -> R<Value> {
    let items = parse_format(vm, fmt)?;
    // Result slots, by argument number.
    let mut nslots = 0usize;
    let mut auto = 0usize;
    for c in items.iter().flatten() {
        if c.suppress || c.conv == 0 {
            continue;
        }
        let idx = match c.argnum {
            Some(n) => n,
            None => {
                auto += 1;
                auto - 1
            }
        };
        nslots = nslots.max(idx + 1);
    }
    let nvars = a.len().saturating_sub(first_var);
    if nvars > 0 && nvars != nslots {
        let msg = if nvars > nslots {
            "Variable is not assigned by any conversion specifiers"
        } else {
            "Different numbers of variable names and field specifiers"
        };
        return vm.throw("ValueError", msg);
    }
    let mut results: Vec<Option<Scanned>> = (0..nslots).map(|_| None).collect();
    let mut pos = 0usize;
    let mut auto = 0usize;
    let mut converted = 0i64;
    let mut eof_first = false;
    let mut any_attempt = false;
    'scan: for it in &items {
        match it {
            Err(b' ') => {
                while pos < s.len() && is_ws(s[pos]) {
                    pos += 1;
                }
            }
            Err(lit) => {
                if pos >= s.len() {
                    if !any_attempt {
                        eof_first = true;
                    }
                    break 'scan;
                }
                if s[pos] != *lit {
                    break 'scan;
                }
                pos += 1;
            }
            Ok(c) => {
                let slot = if c.suppress {
                    None
                } else {
                    Some(match c.argnum {
                        Some(n) => n,
                        None => {
                            auto += 1;
                            auto - 1
                        }
                    })
                };
                if c.conv == b'n' {
                    if let Some(i) = slot {
                        results[i] = Some(Scanned::Int(pos as i64));
                    }
                    continue;
                }
                if c.conv != b'c' && c.conv != b'[' {
                    while pos < s.len() && is_ws(s[pos]) {
                        pos += 1;
                    }
                }
                if pos >= s.len() {
                    if !any_attempt {
                        eof_first = true;
                    }
                    break 'scan;
                }
                any_attempt = true;
                let rest = &s[pos..];
                let width = c.width.filter(|&w| w > 0).unwrap_or(usize::MAX);
                let val = match c.conv {
                    b'd' | b'i' | b'o' | b'x' | b'X' | b'u' => {
                        let base = match c.conv {
                            b'd' | b'u' => 10,
                            b'i' => 0,
                            b'o' => 8,
                            _ => 16,
                        };
                        let Some((v, used, overflow)) = scan_int(rest, width, base) else {
                            break 'scan;
                        };
                        pos += used;
                        if c.conv == b'u' {
                            let u = if v < 0 {
                                (v as i64) as u64
                            } else if overflow || v > u64::MAX as i128 {
                                u64::MAX
                            } else {
                                v as u64
                            };
                            if u > i64::MAX as u64 {
                                Scanned::Str(u.to_string().into_bytes())
                            } else {
                                Scanned::Int(u as i64)
                            }
                        } else {
                            let clamped = if overflow {
                                if v < 0 { i64::MIN } else { i64::MAX }
                            } else {
                                v.clamp(i64::MIN as i128, i64::MAX as i128) as i64
                            };
                            Scanned::Int(clamped)
                        }
                    }
                    b'f' | b'e' | b'E' | b'g' => {
                        let Some((v, used)) = scan_float(rest, width) else {
                            break 'scan;
                        };
                        pos += used;
                        Scanned::Float(v)
                    }
                    b's' => {
                        let mut end = 0;
                        while end < rest.len() && end < width && !is_ws(rest[end]) {
                            end += 1;
                        }
                        pos += end;
                        Scanned::Str(rest[..end].to_vec())
                    }
                    b'c' => {
                        let w = c.width.filter(|&w| w > 0).unwrap_or(1).min(rest.len());
                        pos += w;
                        Scanned::Str(rest[..w].to_vec())
                    }
                    b'[' => {
                        let set = c.set.as_ref().unwrap();
                        let mut end = 0;
                        while end < rest.len() && end < width && set[rest[end] as usize] {
                            end += 1;
                        }
                        if end == 0 {
                            break 'scan;
                        }
                        pos += end;
                        Scanned::Str(rest[..end].to_vec())
                    }
                    _ => break 'scan,
                };
                if let Some(i) = slot {
                    results[i] = Some(val);
                    converted += 1;
                }
            }
        }
    }
    if nvars == 0 {
        if eof_first && converted == 0 {
            return Ok(Value::Null);
        }
        return Ok(Value::Array(PhpArray::from_values(
            results
                .into_iter()
                .map(|r| r.map(Scanned::into_value).unwrap_or(Value::Null)),
        )));
    }
    if eof_first && converted == 0 {
        return Ok(Value::Int(-1));
    }
    for (i, r) in results.into_iter().enumerate() {
        if let Some(v) = r {
            a.set_ref(first_var + i, v.into_value());
        }
    }
    Ok(Value::Int(converted))
}

// ---------------------------------------------------------------- pack

/// A repeater: a count, `*`, or none.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rep {
    None,
    Star,
    Count(usize),
}

fn read_rep(f: &[u8], i: &mut usize) -> Rep {
    if *i < f.len() && f[*i] == b'*' {
        *i += 1;
        return Rep::Star;
    }
    let st = *i;
    while *i < f.len() && f[*i].is_ascii_digit() {
        *i += 1;
    }
    if *i > st {
        Rep::Count(
            std::str::from_utf8(&f[st..*i])
                .unwrap()
                .parse()
                .unwrap_or(usize::MAX),
        )
    } else {
        Rep::None
    }
}

/// Integer size and byte order of a pack code: (bytes, big endian, signed).
fn int_code(c: u8) -> Option<(usize, bool, bool)> {
    Some(match c {
        b'c' => (1, false, true),
        b'C' => (1, false, false),
        b's' => (2, false, true),
        b'S' | b'v' => (2, false, false),
        b'n' => (2, true, false),
        b'i' | b'l' => (4, false, true),
        b'I' | b'L' | b'V' => (4, false, false),
        b'N' => (4, true, false),
        b'q' => (8, false, true),
        b'Q' | b'P' => (8, false, false),
        b'J' => (8, true, false),
        _ => return None,
    })
}

fn float_code(c: u8) -> Option<(usize, bool)> {
    Some(match c {
        b'f' | b'g' => (4, false),
        b'G' => (4, true),
        b'd' | b'e' => (8, false),
        b'E' => (8, true),
        _ => return None,
    })
}

fn pack(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fmt = a.str(0);
    let args: Vec<Value> = a.vals[1..].iter().map(|v| v.deref()).collect();
    let mut ai = 0usize;
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        let code = fmt[i];
        i += 1;
        let rep = read_rep(&fmt, &mut i);
        match code {
            b'a' | b'A' | b'Z' | b'h' | b'H' => {
                if ai >= args.len() {
                    return vm.throw(
                        "ValueError",
                        &format!("Type {}: not enough arguments", code as char),
                    );
                }
                let s = vm.to_php_str(&args[ai])?;
                ai += 1;
                if code == b'h' || code == b'H' {
                    let n = match rep {
                        Rep::Star => s.len(),
                        Rep::Count(n) => n,
                        Rep::None => 1,
                    };
                    let mut digits = s[..n.min(s.len())].to_vec();
                    if n > s.len() {
                        vm.fwarn(
                            a,
                            &format!("Type {}: not enough characters in string", code as char),
                        )?;
                    }
                    let mut nib = Vec::with_capacity(digits.len());
                    for d in digits.drain(..) {
                        let v = match d {
                            b'0'..=b'9' => d - b'0',
                            b'a'..=b'f' => d - b'a' + 10,
                            b'A'..=b'F' => d - b'A' + 10,
                            _ => {
                                vm.fwarn(
                                    a,
                                    &format!(
                                        "Type {}: illegal hex digit {}",
                                        code as char, d as char
                                    ),
                                )?;
                                0
                            }
                        };
                        nib.push(v);
                    }
                    for pair in nib.chunks(2) {
                        let (x, y) = (pair[0], pair.get(1).copied().unwrap_or(0));
                        out.push(if code == b'H' {
                            (x << 4) | y
                        } else {
                            (y << 4) | x
                        });
                    }
                    continue;
                }
                let n = match rep {
                    Rep::Star => s.len() + (code == b'Z') as usize,
                    Rep::Count(n) => n,
                    Rep::None => 1,
                };
                let pad = if code == b'A' { b' ' } else { 0 };
                let take = if code == b'Z' {
                    n.saturating_sub(1).min(s.len())
                } else {
                    n.min(s.len())
                };
                out.extend_from_slice(&s[..take]);
                out.extend(std::iter::repeat_n(pad, n - take));
            }
            b'x' | b'X' | b'@' => {
                let n = match rep {
                    Rep::Star => {
                        vm.fwarn(a, &format!("Type {}: '*' ignored", code as char))?;
                        1
                    }
                    Rep::Count(n) => n,
                    Rep::None => 1,
                };
                match code {
                    b'x' => out.extend(std::iter::repeat_n(0, n)),
                    b'X' => {
                        if n > out.len() {
                            vm.fwarn(a, "Type X: outside of string")?;
                            out.clear();
                        } else {
                            out.truncate(out.len() - n);
                        }
                    }
                    _ => {
                        if n > out.len() {
                            out.resize(n, 0);
                        } else {
                            out.truncate(n);
                        }
                    }
                }
            }
            c if int_code(c).is_some() || float_code(c).is_some() => {
                let n = match rep {
                    Rep::Star => args.len().saturating_sub(ai),
                    Rep::Count(n) => n,
                    Rep::None => 1,
                };
                if ai + n > args.len() {
                    return vm.throw(
                        "ValueError",
                        &format!("Type {}: too few arguments", c as char),
                    );
                }
                for _ in 0..n {
                    let v = args[ai].clone();
                    ai += 1;
                    if let Some((size, be, _)) = int_code(c) {
                        let iv = match vm.cast(CastTo::Int, v)? {
                            Value::Int(x) => x,
                            _ => 0,
                        };
                        let bytes = iv.to_le_bytes();
                        if be {
                            out.extend(bytes[..size].iter().rev());
                        } else {
                            out.extend_from_slice(&bytes[..size]);
                        }
                    } else if let Some((size, be)) = float_code(c) {
                        let fv = match vm.cast(CastTo::Float, v)? {
                            Value::Float(x) => x,
                            _ => 0.0,
                        };
                        if size == 4 {
                            let b = (fv as f32).to_bits();
                            out.extend_from_slice(&if be {
                                b.to_be_bytes()
                            } else {
                                b.to_le_bytes()
                            });
                        } else {
                            let b = fv.to_bits();
                            out.extend_from_slice(&if be {
                                b.to_be_bytes()
                            } else {
                                b.to_le_bytes()
                            });
                        }
                    }
                }
            }
            other => {
                return vm.throw(
                    "ValueError",
                    &format!("Type {}: unknown format code", other as char),
                );
            }
        }
    }
    if ai < args.len() {
        vm.fwarn(a, &format!("{} arguments unused", args.len() - ai))?;
    }
    Ok(sv(out))
}

fn unpack(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fmt = a.str(0);
    let data = a.str(1);
    let offset = a.int_or(2, 0);
    if offset < 0 || offset as usize > data.len() {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($offset) must be contained in argument #2 ($data)",
        );
    }
    let input = &data[offset as usize..];
    let n = input.len();
    let mut pos = 0usize;
    let mut out = PhpArray::new();
    let not_enough = |vm: &mut Vm, code: u8, need: usize, have: usize| -> R<Value> {
        vm.fwarn(
            a,
            &format!(
                "Type {}: not enough input values, need {} values but only {} {} provided",
                code as char,
                need,
                have,
                if have == 1 { "was" } else { "were" }
            ),
        )?;
        Ok(Value::Bool(false))
    };
    for part in fmt.split(|&c| c == b'/') {
        if part.is_empty() {
            continue;
        }
        let code = part[0];
        let mut i = 1;
        let rep = read_rep(part, &mut i);
        let name = &part[i..];
        if let Rep::Count(c) = rep
            && c > i32::MAX as usize
        {
            // Test bug69522.phpt.
            vm.fwarn(a, &format!("Type {}: integer overflow", code as char))?;
            return Ok(Value::Bool(false));
        }
        let key_for = |idx: usize, single: bool| -> Key {
            if single && !name.is_empty() {
                Key::from_bytes(name)
            } else {
                let mut k = name.to_vec();
                k.extend_from_slice((idx + 1).to_string().as_bytes());
                Key::from_bytes(&k)
            }
        };
        match code {
            b'a' | b'A' | b'Z' => {
                let len = match rep {
                    Rep::Star => n - pos,
                    Rep::Count(c) => c,
                    Rep::None => 1,
                };
                if pos + len > n {
                    return not_enough(vm, code, len, n - pos);
                }
                let mut s = input[pos..pos + len].to_vec();
                pos += len;
                match code {
                    b'A' => {
                        while s
                            .last()
                            .is_some_and(|&c| matches!(c, b' ' | 0 | b'\t' | b'\r' | b'\n'))
                        {
                            s.pop();
                        }
                    }
                    b'Z' => {
                        if let Some(z) = s.iter().position(|&c| c == 0) {
                            s.truncate(z);
                        }
                    }
                    _ => {}
                }
                out.insert(key_for(0, true), sv(s));
            }
            b'h' | b'H' => {
                let nibbles = match rep {
                    Rep::Star => (n - pos) * 2,
                    Rep::Count(c) => c,
                    Rep::None => 1,
                };
                let bytes = nibbles.div_ceil(2);
                if pos + bytes > n {
                    return not_enough(vm, code, bytes, n - pos);
                }
                // Tests gh15613.phpt, gh10940.phpt: checked once the input
                // is long enough.
                if nibbles > i32::MAX as usize - 1 {
                    return vm.fthrow(
                        a,
                        "ValueError",
                        &format!(
                            "Argument #1 ($format) repeater must be less than or equal to {}",
                            i32::MAX - 1
                        ),
                    );
                }
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let mut s = Vec::with_capacity(nibbles);
                for k in 0..nibbles {
                    let b = input[pos + k / 2];
                    let (hi, lo) = (b >> 4, b & 15);
                    let first = if code == b'H' { hi } else { lo };
                    let second = if code == b'H' { lo } else { hi };
                    s.push(HEX[if k % 2 == 0 { first } else { second } as usize]);
                }
                pos += bytes;
                out.insert(key_for(0, true), sv(s));
            }
            b'x' => {
                let cnt = match rep {
                    Rep::Count(c) => c,
                    Rep::Star => 0,
                    Rep::None => 1,
                };
                for _ in 0..cnt {
                    if pos + 1 > n {
                        return not_enough(vm, code, 1, n - pos);
                    }
                    pos += 1;
                }
            }
            b'X' => {
                // Test bug75075.phpt.
                if rep == Rep::Star {
                    vm.fwarn(a, "Type X: '*' ignored")?;
                }
                let cnt = match rep {
                    Rep::Count(c) => c,
                    _ => 1,
                };
                pos = pos.saturating_sub(cnt);
            }
            b'@' => {
                let p = match rep {
                    Rep::Count(c) => c,
                    Rep::Star => usize::MAX,
                    Rep::None => 0,
                };
                if p > n {
                    vm.fwarn(a, "Type @: outside of string")?;
                } else {
                    pos = p;
                }
            }
            c if int_code(c).is_some() || float_code(c).is_some() => {
                let size = int_code(c)
                    .map(|x| x.0)
                    .or(float_code(c).map(|x| x.0))
                    .unwrap();
                // Test unpack_bug68225.phpt: an explicit count of 1 keeps the
                // bare name.
                let (count, single) = match rep {
                    Rep::Star => ((n - pos) / size, false),
                    Rep::Count(k) => (k, k == 1),
                    Rep::None => (1, true),
                };
                for idx in 0..count {
                    if pos + size > n {
                        return not_enough(vm, c, size, n - pos);
                    }
                    let b = &input[pos..pos + size];
                    pos += size;
                    let v = if let Some((size, be, signed)) = int_code(c) {
                        let mut buf = [0u8; 8];
                        if be {
                            for (k, &x) in b.iter().rev().enumerate() {
                                buf[k] = x;
                            }
                        } else {
                            buf[..size].copy_from_slice(b);
                        }
                        let raw = u64::from_le_bytes(buf);
                        let v = if signed && size < 8 {
                            let shift = 64 - size * 8;
                            ((raw << shift) as i64) >> shift
                        } else {
                            raw as i64
                        };
                        Value::Int(v)
                    } else {
                        let (size, be) = float_code(c).unwrap();
                        if size == 4 {
                            let arr: [u8; 4] = b.try_into().unwrap();
                            let bits = if be {
                                u32::from_be_bytes(arr)
                            } else {
                                u32::from_le_bytes(arr)
                            };
                            Value::Float(f32::from_bits(bits) as f64)
                        } else {
                            let arr: [u8; 8] = b.try_into().unwrap();
                            let bits = if be {
                                u64::from_be_bytes(arr)
                            } else {
                                u64::from_le_bytes(arr)
                            };
                            Value::Float(f64::from_bits(bits))
                        }
                    };
                    out.insert(key_for(idx, single), v);
                }
            }
            other => {
                return vm.throw(
                    "ValueError",
                    &format!("Invalid format type {}", other as char),
                );
            }
        }
    }
    Ok(Value::Array(out))
}
