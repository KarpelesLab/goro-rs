//! ext/intl/grapheme: the grapheme_*() functions (grapheme_string.c,
//! grapheme_util.c). ICU's string search (usearch) is emulated with a
//! collator comparison of the haystack's grapheme-aligned substrings.

use std::cmp::Ordering;

use goro_core::array::PhpArray;
use goro_core::value::Value;
use goro_core::vm::Vm;
use icu_collator::options::Strength;

use crate::error::{self, U_ILLEGAL_ARGUMENT_ERROR};
use crate::zpp::value_error;
use crate::{Call, arr, s};

pub const METHODS: &[(&str, crate::Imp)] = &[];

pub const CLASSES: &[(&str, Option<crate::obj::CreateFn>)] = &[];

pub const EXTRACT_COUNT: i64 = 0;
pub const EXTRACT_MAXBYTES: i64 = 1;
pub const EXTRACT_MAXCHARS: i64 = 2;

crate::intl_functions! { pub FUNCTIONS:
    "grapheme_strlen" => grapheme_strlen,
    "grapheme_strpos" => grapheme_strpos,
    "grapheme_stripos" => grapheme_stripos,
    "grapheme_strrpos" => grapheme_strrpos,
    "grapheme_strripos" => grapheme_strripos,
    "grapheme_substr" => grapheme_substr,
    "grapheme_strstr" => grapheme_strstr,
    "grapheme_stristr" => grapheme_stristr,
    "grapheme_str_split" => grapheme_str_split,
    "grapheme_levenshtein" => grapheme_levenshtein,
    "grapheme_extract" => grapheme_extract,
}

/// grapheme_ascii_check() over `s[..len]` (reading one byte further, as
/// the C code does, to see a CR LF pair).
fn ascii_check(s: &[u8], len: usize) -> bool {
    for i in 0..len.min(s.len()) {
        if s[i] > 0x7f {
            return false;
        }
        if s.get(i + 1) == Some(&b'\n') && s[i] == b'\r' {
            return false;
        }
    }
    true
}

/// A UTF-16 view of valid UTF-8 text with its grapheme boundaries.
struct Text {
    s: String,
    /// Grapheme boundaries in UTF-16 units, 0 and the length included.
    bounds16: Vec<usize>,
    /// The same boundaries as UTF-8 byte offsets.
    bounds8: Vec<usize>,
    len16: usize,
}

impl Text {
    fn new(s: &str) -> Text {
        let bounds8 = str_bounds(s);
        let mut bounds16 = Vec::with_capacity(bounds8.len());
        let mut u = 0;
        let mut last = 0;
        for &b in &bounds8 {
            u += s[last..b].encode_utf16().count();
            bounds16.push(u);
            last = b;
        }
        Text {
            s: s.to_string(),
            len16: u,
            bounds16,
            bounds8,
        }
    }

    fn count(&self) -> usize {
        self.bounds8.len().saturating_sub(1)
    }

    /// grapheme_get_haystack_offset(): the UTF-16 position of the grapheme
    /// offset (from the end when negative), None when out of range.
    fn haystack_offset(&self, offset: i64) -> Option<usize> {
        if offset == 0 {
            return Some(0);
        }
        let n = self.count() as i64;
        if offset > 0 {
            if offset > n {
                return None;
            }
            Some(self.bounds16[offset as usize])
        } else {
            if -offset > n {
                return None;
            }
            Some(self.bounds16[(n + offset) as usize])
        }
    }

    /// The number of graphemes before UTF-16 position `pos`.
    fn graphemes_before(&self, pos: usize) -> usize {
        self.bounds16.iter().filter(|&&b| b < pos).count().min(self.count())
    }

    fn sub(&self, i: usize, j: usize) -> &str {
        &self.s[self.bounds8[i]..self.bounds8[j]]
    }
}

/// usearch: the grapheme-aligned matches of `needle` in `t` starting at
/// boundary index `from` or later, as (start index, end index).
fn find_from(
    t: &Text,
    needle: &str,
    coll: &icu_collator::CollatorBorrowed<'static>,
    from: usize,
) -> Option<(usize, usize)> {
    let n = t.count();
    for i in from..n {
        for j in i + 1..=n {
            if coll.compare(t.sub(i, j), needle) == Ordering::Equal {
                return Some((i, j));
            }
        }
    }
    None
}

fn find_last(
    t: &Text,
    needle: &str,
    coll: &icu_collator::CollatorBorrowed<'static>,
) -> Option<(usize, usize)> {
    let n = t.count();
    for i in (0..n).rev() {
        for j in i + 1..=n {
            if coll.compare(t.sub(i, j), needle) == Ordering::Equal {
                return Some((i, j));
            }
        }
    }
    None
}

/// The boundary index of UTF-16 position `pos` (the first boundary at or
/// after it).
fn bound_index(t: &Text, pos: usize) -> usize {
    t.bounds16
        .iter()
        .position(|&b| b >= pos)
        .unwrap_or(t.count())
}

/// grapheme_strpos_utf16(): Ok((grapheme position, UTF-16 position)),
/// Ok(None) when not found, Err(()) when an exception was thrown.
#[allow(clippy::too_many_arguments)]
fn strpos_utf16(
    vm: &mut Vm,
    c: &Call,
    haystack: &[u8],
    needle: &[u8],
    offset: i64,
    ignore_case: bool,
    last: bool,
    locale: &[u8],
) -> Result<Option<(usize, usize)>, ()> {
    let Ok(h) = std::str::from_utf8(haystack) else {
        error::set_global(
            vm,
            &c.name,
            error::U_INVALID_CHAR_FOUND,
            "Error converting input string to UTF-16",
        );
        return Ok(None);
    };
    let Ok(nd) = std::str::from_utf8(needle) else {
        error::set_global(
            vm,
            &c.name,
            error::U_INVALID_CHAR_FOUND,
            "Error converting needle string to UTF-16",
        );
        return Ok(None);
    };
    let t = Text::new(h);
    let offset_err = |vm: &mut Vm| {
        value_error(
            vm,
            &c.name,
            3,
            "offset",
            "must be contained in argument #1 ($haystack)",
        );
    };
    if nd.is_empty() {
        let Some(op) = t.haystack_offset(offset) else {
            offset_err(vm);
            return Err(());
        };
        let r = if last && offset >= 0 { t.len16 } else { op };
        return Ok(Some((r, r)));
    }
    let loc = String::from_utf8_lossy(locale).into_owned();
    let strength = if ignore_case {
        Some(Strength::Secondary)
    } else {
        None
    };
    let coll = match crate::common::collator(&loc, strength) {
        Ok(c) => c,
        Err(code) => {
            error::set_global(vm, &c.name, code, "Error creating search object");
            return Ok(None);
        }
    };
    let mut offset_pos = 0;
    if offset != 0 {
        let Some(op) = t.haystack_offset(offset) else {
            offset_err(vm);
            return Err(());
        };
        offset_pos = op;
    }
    let found = if last {
        if offset >= 0 {
            find_last(&t, nd, &coll).filter(|(i, _)| t.bounds16[*i] >= offset_pos)
        } else {
            let mut prev = None;
            let mut from = 0;
            loop {
                match find_from(&t, nd, &coll, from) {
                    Some((i, j)) if t.bounds16[i] <= offset_pos => {
                        prev = Some((i, j));
                        from = j.max(i + 1);
                    }
                    _ => break,
                }
            }
            prev
        }
    } else {
        find_from(&t, nd, &coll, bound_index(&t, offset_pos))
    };
    Ok(found.map(|(i, _)| {
        let p16 = t.bounds16[i];
        (t.graphemes_before(p16), p16)
    }))
}

fn memnstr(h: &[u8], from: usize, n: &[u8]) -> Option<usize> {
    if from > h.len() {
        return None;
    }
    if n.is_empty() {
        return Some(from);
    }
    h[from..].windows(n.len()).position(|w| w == n).map(|p| p + from)
}

/// OUTSIDE_STRING()
fn outside(offset: i64, len: usize) -> bool {
    offset != 0 && (offset < -(len as i64) || offset > len as i64)
}

fn check_offset(vm: &mut Vm, c: &Call, off: i64, hlen: usize) -> bool {
    if outside(off, hlen) {
        value_error(
            vm,
            &c.name,
            3,
            "offset",
            "must be contained in argument #1 ($haystack)",
        );
        return false;
    }
    true
}

fn pos_result(r: Result<Option<(usize, usize)>, ()>) -> Value {
    match r {
        Ok(Some((p, _))) => Value::Long(p as i64),
        Ok(None) => Value::False,
        Err(()) => Value::Null,
    }
}

/// grapheme_strlen()
fn grapheme_strlen(vm: &mut Vm, c: &mut Call) -> Value {
    let st = c.str(0);
    if ascii_check(&st, st.len()) {
        return Value::Long(st.len() as i64);
    }
    let Ok(t) = std::str::from_utf8(&st) else {
        error::set_global(
            vm,
            &c.name,
            error::U_INVALID_CHAR_FOUND,
            "Error converting input string to UTF-16",
        );
        return Value::Null;
    };
    Value::Long(Text::new(t).count() as i64)
}

/// grapheme_strpos()
fn grapheme_strpos(vm: &mut Vm, c: &mut Call) -> Value {
    let (h, n, off, loc) = (c.str(0), c.str(1), c.long(2), c.str(3));
    if !check_offset(vm, c, off, h.len()) {
        return Value::Null;
    }
    if off >= 0 && ascii_check(&h, h.len()) {
        return match memnstr(&h, off as usize, &n) {
            Some(p) => Value::Long(p as i64),
            None => Value::False,
        };
    }
    pos_result(strpos_utf16(vm, c, &h, &n, off, false, false, &loc))
}

/// grapheme_stripos()
fn grapheme_stripos(vm: &mut Vm, c: &mut Call) -> Value {
    let (h, n, off, loc) = (c.str(0), c.str(1), c.long(2), c.str(3));
    if !check_offset(vm, c, off, h.len()) {
        return Value::Null;
    }
    if ascii_check(&h, h.len()) {
        let noff = if off >= 0 {
            off as usize
        } else {
            (h.len() as i64 + off) as usize
        };
        let hl = h.to_ascii_lowercase();
        let nl = n.to_ascii_lowercase();
        if let Some(p) = memnstr(&hl, noff, &nl) {
            return Value::Long(p as i64);
        }
        if ascii_check(&n, n.len()) {
            return Value::False;
        }
    }
    pos_result(strpos_utf16(vm, c, &h, &n, off, true, false, &loc))
}

/// grapheme_strrpos_ascii()
fn strrpos_ascii(h: &[u8], n: &[u8], offset: i64) -> Option<usize> {
    let hl = h.len() as i64;
    let nl = n.len() as i64;
    let (p, e) = if offset >= 0 {
        (offset, hl - nl)
    } else if nl > -offset {
        (0, hl - nl)
    } else {
        (0, hl + offset)
    };
    let mut e = e;
    while e >= p {
        if e >= 0 && (e + nl) <= hl && &h[e as usize..(e + nl) as usize] == n {
            return Some((e - p + if offset > 0 { offset } else { 0 }) as usize);
        }
        e -= 1;
    }
    None
}

/// grapheme_strrpos()
fn grapheme_strrpos(vm: &mut Vm, c: &mut Call) -> Value {
    let (h, n, off, loc) = (c.str(0), c.str(1), c.long(2), c.str(3));
    if !check_offset(vm, c, off, h.len()) {
        return Value::Null;
    }
    if ascii_check(&h, h.len()) {
        if let Some(p) = strrpos_ascii(&h, &n, off) {
            return Value::Long(p as i64);
        }
        if ascii_check(&n, n.len()) {
            return Value::False;
        }
    }
    pos_result(strpos_utf16(vm, c, &h, &n, off, false, true, &loc))
}

/// grapheme_strripos()
fn grapheme_strripos(vm: &mut Vm, c: &mut Call) -> Value {
    let (h, n, off, loc) = (c.str(0), c.str(1), c.long(2), c.str(3));
    if !check_offset(vm, c, off, h.len()) {
        return Value::Null;
    }
    if ascii_check(&h, h.len()) {
        let hl = h.to_ascii_lowercase();
        let nl = n.to_ascii_lowercase();
        if let Some(p) = strrpos_ascii(&hl, &nl, off) {
            return Value::Long(p as i64);
        }
        if ascii_check(&n, n.len()) {
            return Value::False;
        }
    }
    pos_result(strpos_utf16(vm, c, &h, &n, off, true, true, &loc))
}

/// grapheme_substr_ascii()
fn substr_ascii(st: &[u8], f: i64, l: i64) -> &[u8] {
    let len = st.len() as i64;
    let mut f = f;
    if f < 0 {
        f += len;
        if f < 0 {
            f = 0;
        }
    } else if f > len {
        f = len;
    }
    let mut l = l;
    if l < 0 {
        l += len - f;
        if l < 0 {
            l = 0;
        }
    } else if l > len - f {
        l = len - f;
    }
    &st[f as usize..(f + l) as usize]
}

/// grapheme_substr()
fn grapheme_substr(vm: &mut Vm, c: &mut Call) -> Value {
    let st = c.str(0);
    let lstart = c.long(1);
    if lstart < i32::MIN as i64 || lstart > i32::MAX as i64 {
        value_error(vm, &c.name, 2, "offset", "is too large");
        return Value::Null;
    }
    let length = c.opt_long(2).unwrap_or(st.len() as i64);
    if length < i32::MIN as i64 || length > i32::MAX as i64 {
        value_error(vm, &c.name, 3, "length", "is too large");
        return Value::Null;
    }
    let mut start = lstart;
    if ascii_check(&st, st.len()) {
        return s(substr_ascii(&st, start, length));
    }
    let Ok(text) = std::str::from_utf8(&st) else {
        error::set_global(
            vm,
            &c.name,
            error::U_INVALID_CHAR_FOUND,
            "Error converting input string to UTF-16",
        );
        return Value::False;
    };
    let t = Text::new(text);
    let n = t.count() as i64;
    // the start boundary index
    let start_idx: usize;
    if start >= 0 {
        if start > n {
            return s(b"");
        }
        start_idx = start as usize;
        start = 0;
    } else if -start > n {
        start_idx = 0;
        start = 0;
    } else {
        start_idx = (n + start) as usize;
        start = 0;
    }
    let _ = start;
    let start8 = t.bounds8[start_idx];
    if length >= st.len() as i64 {
        return s(&st[start8..]);
    }
    if length == 0 {
        return s(b"");
    }
    let end8 = if length < 0 {
        // count back from the end
        if -length > n {
            return s(b"");
        }
        t.bounds8[(n + length) as usize]
    } else {
        // count forward from the start position
        t.bounds8[(start_idx as i64 + length).min(n) as usize]
    };
    if start8 > end8 {
        return s(b"");
    }
    s(&st[start8..end8])
}

/// strstr_common_handler()
fn strstr_common(vm: &mut Vm, c: &Call, ignore_case: bool) -> Value {
    let (h, n, part, loc) = (c.str(0), c.str(1), c.bool(2), c.str(3));
    if !ignore_case {
        let Some(found) = memnstr(&h, 0, &n) else {
            return Value::False;
        };
        if ascii_check(&h, h.len()) {
            return if part {
                s(&h[..found])
            } else {
                s(&h[found..])
            };
        }
    }
    let r = strpos_utf16(vm, c, &h, &n, 0, ignore_case, false, &loc);
    let uchar_pos = match r {
        Ok(Some((_, p))) => p,
        Ok(None) => return Value::False,
        Err(()) => return Value::Null,
    };
    // U8_FWD_N(haystack, ret_pos, len, uchar_pos): code points, as the C
    // code does with the UTF-16 position
    let mut ret = 0usize;
    let mut k = 0;
    while k < uchar_pos && ret < h.len() {
        ret += 1;
        while ret < h.len() && h[ret] & 0xC0 == 0x80 {
            ret += 1;
        }
        k += 1;
    }
    if part { s(&h[..ret]) } else { s(&h[ret..]) }
}

fn grapheme_strstr(vm: &mut Vm, c: &mut Call) -> Value {
    strstr_common(vm, c, false)
}

fn grapheme_stristr(vm: &mut Vm, c: &mut Call) -> Value {
    strstr_common(vm, c, true)
}

/// The grapheme boundaries of `s` (byte offsets, 0 and the length
/// included).
pub fn str_bounds(s: &str) -> Vec<usize> {
    let mut v = vec![0];
    let mut p = 0;
    for g in intl::unicode::graphemes(s) {
        p += g.len();
        v.push(p);
    }
    v
}

/// UTF-8 grapheme boundaries of possibly ill-formed text (as UText over
/// UTF-8 sees it: each ill-formed sequence is a U+FFFD of its own).
fn utf8_bounds(st: &[u8]) -> Vec<usize> {
    let mut v = vec![0];
    let mut base = 0;
    let mut rest = st;
    loop {
        match std::str::from_utf8(rest) {
            Ok(t) => {
                v.extend(str_bounds(t).into_iter().skip(1).map(|b| b + base));
                break;
            }
            Err(e) => {
                let ok = e.valid_up_to();
                let t = std::str::from_utf8(&rest[..ok]).unwrap_or("");
                v.extend(str_bounds(t).into_iter().skip(1).map(|b| b + base));
                let bad = e.error_len().unwrap_or(rest.len() - ok);
                base += ok + bad;
                v.push(base);
                rest = &st[base..];
                if rest.is_empty() {
                    break;
                }
            }
        }
    }
    v
}

/// grapheme_extract()
fn grapheme_extract(vm: &mut Vm, c: &mut Call) -> Value {
    let st = c.str(0);
    let size = c.long(1);
    let ty = c.long(2);
    let mut lstart = c.long(3);
    let has_next = c.passed(4);
    if lstart < 0 {
        lstart += st.len() as i64;
    }
    if has_next && !c.set_ref(vm, 4, Value::Long(lstart)) {
        return Value::Null;
    }
    if !(EXTRACT_COUNT..=EXTRACT_MAXCHARS).contains(&ty) {
        value_error(
            vm,
            &c.name,
            3,
            "type",
            "must be one of GRAPHEME_EXTR_COUNT, GRAPHEME_EXTR_MAXBYTES, or GRAPHEME_EXTR_MAXCHARS",
        );
        return Value::Null;
    }
    if lstart > i32::MAX as i64 || lstart < 0 || lstart as usize >= st.len() {
        error::set_global(
            vm,
            &c.name,
            U_ILLEGAL_ARGUMENT_ERROR,
            "start not contained in string",
        );
        return Value::False;
    }
    if size < 0 {
        value_error(
            vm,
            &c.name,
            2,
            "size",
            "must be greater than or equal to 0",
        );
        return Value::Null;
    }
    if size > i32::MAX as i64 {
        value_error(vm, &c.name, 2, "size", "is too large");
        return Value::Null;
    }
    if size == 0 {
        return s(b"");
    }
    let mut start = lstart as usize;
    // move to the start of a character
    while st[start] & 0xC0 == 0x80 {
        start += 1;
        if start >= st.len() {
            error::set_global(
                vm,
                &c.name,
                U_ILLEGAL_ARGUMENT_ERROR,
                "grapheme_extract: invalid input string",
            );
            return Value::False;
        }
    }
    let p = &st[start..];
    let size = size as usize;
    if ascii_check(p, (size + 1).min(p.len())) {
        let n = size.min(p.len());
        if has_next {
            c.set_ref(vm, 4, Value::Long((start + n) as i64));
        }
        return s(&p[..n]);
    }
    let bounds = utf8_bounds(p);
    let mut ret = 0usize;
    match ty {
        EXTRACT_COUNT => {
            let mut left = size;
            for &b in bounds.iter().skip(1) {
                if left == 0 {
                    break;
                }
                ret = b;
                left -= 1;
            }
        }
        EXTRACT_MAXBYTES => {
            for &b in bounds.iter().skip(1) {
                if b > size {
                    break;
                }
                ret = b;
            }
        }
        _ => {
            let mut count = 0;
            let mut csize = size;
            for &b in bounds.iter().skip(1) {
                let mut bp = ret;
                while bp < b {
                    count += 1;
                    let prev = bp;
                    bp += 1;
                    while bp < p.len() && p[bp] & 0xC0 == 0x80 && bp - prev < 4 {
                        bp += 1;
                    }
                    if prev == bp {
                        csize = 0;
                        break;
                    }
                }
                if count > csize {
                    break;
                }
                ret = bp;
            }
        }
    }
    if has_next {
        c.set_ref(vm, 4, Value::Long((start + ret) as i64));
    }
    s(&p[..ret])
}

/// grapheme_str_split()
fn grapheme_str_split(vm: &mut Vm, c: &mut Call) -> Value {
    let st = c.str(0);
    let split_len = c.long(1);
    let max = (u32::MAX / 4) as i64;
    if split_len <= 0 || split_len > max {
        value_error(
            vm,
            &c.name,
            2,
            "length",
            &format!("must be greater than 0 and less than or equal to {}", max),
        );
        return Value::Null;
    }
    let mut out = PhpArray::new();
    if st.is_empty() {
        return arr(out);
    }
    let bounds = utf8_bounds(&st);
    let n = split_len as usize;
    let mut start = 0usize;
    let mut i = 0usize;
    for &b in bounds.iter().skip(1) {
        i += 1;
        if i == n {
            out.push(s(&st[start..b]));
            start = b;
            i = 0;
        }
    }
    if i != 0 && start < st.len() {
        out.push(s(&st[start..]));
    }
    arr(out)
}

/// grapheme_levenshtein()
fn grapheme_levenshtein(vm: &mut Vm, c: &mut Call) -> Value {
    let mut s1 = c.str(0);
    let mut s2 = c.str(1);
    let (ins, rep, del) = (c.long(2), c.long(3), c.long(4));
    let loc = c.str(5);
    let max = (u32::MAX / 4) as i64;
    for (k, v, n) in [(3, ins, "insertion_cost"), (4, rep, "replacement_cost"), (5, del, "deletion_cost")] {
        if v <= 0 || v > max {
            value_error(
                vm,
                &c.name,
                k,
                n,
                &format!("must be greater than 0 and less than or equal to {}", max),
            );
            return Value::Null;
        }
    }
    if s1.len() < s2.len() && ins == rep && rep == del {
        std::mem::swap(&mut s1, &mut s2);
    }
    let (Ok(a), Ok(b)) = (std::str::from_utf8(&s1), std::str::from_utf8(&s2)) else {
        error::set_global(
            vm,
            &c.name,
            error::U_INVALID_CHAR_FOUND,
            "Error converting input string to UTF-16",
        );
        return Value::False;
    };
    let ta = Text::new(a);
    let tb = Text::new(b);
    let (n1, n2) = (ta.count(), tb.count());
    if n1 == 0 {
        return Value::Long(n2 as i64 * ins);
    }
    if n2 == 0 {
        return Value::Long(n1 as i64 * del);
    }
    let coll = match crate::common::collator(&String::from_utf8_lossy(&loc), None) {
        Ok(c) => c,
        Err(code) => {
            error::set_global(vm, &c.name, code, "Error on ucol_open");
            return Value::False;
        }
    };
    let mut p1: Vec<i64> = (0..=n2).map(|i| i as i64 * ins).collect();
    let mut p2 = vec![0i64; n2 + 1];
    for i1 in 0..n1 {
        p2[0] = p1[0] + del;
        let g1 = ta.sub(i1, i1 + 1);
        for i2 in 0..n2 {
            let g2 = tb.sub(i2, i2 + 1);
            let mut c0 = if coll.compare(g1, g2) == Ordering::Equal {
                p1[i2]
            } else {
                p1[i2] + rep
            };
            let c1 = p1[i2 + 1] + del;
            if c1 < c0 {
                c0 = c1;
            }
            let c2 = p2[i2] + ins;
            if c2 < c0 {
                c0 = c2;
            }
            p2[i2 + 1] = c0;
        }
        std::mem::swap(&mut p1, &mut p2);
    }
    Value::Long(p1[n2])
}
