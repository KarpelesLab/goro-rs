//! ext/pcre: the preg_* functions over goro_regex (PHP manual: PCRE
//! functions, Pattern Modifiers; results and messages observed on PHP 8.5
//! and in tests ext/pcre/tests/*.phpt).

use std::collections::HashMap;
use std::rc::Rc;

use goro_regex::php::{GlobalMatcher, compile as php_compile, expand_replacement, preg_error_code};
use goro_regex::{Captures, Limits, MatchError, MatchOptions, Regex, Scratch};
use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::string::sv;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "pcre",
    functions: &[
        (
            "function preg_match(string $pattern, string $subject, &$matches = null, int $flags = 0, int $offset = 0): int|false",
            preg_match,
        ),
        (
            "function preg_match_all(string $pattern, string $subject, &$matches = null, int $flags = 0, int $offset = 0): int|false",
            preg_match_all,
        ),
        (
            "function preg_replace(array|string $pattern, array|string $replacement, array|string $subject, int $limit = -1, &$count = null): array|string|null",
            preg_replace,
        ),
        (
            "function preg_filter(array|string $pattern, array|string $replacement, array|string $subject, int $limit = -1, &$count = null): array|string|null",
            preg_filter,
        ),
        (
            "function preg_replace_callback(array|string $pattern, callable $callback, array|string $subject, int $limit = -1, &$count = null, int $flags = 0): array|string|null",
            preg_replace_callback,
        ),
        (
            "function preg_replace_callback_array(array $pattern, array|string $subject, int $limit = -1, &$count = null, int $flags = 0): array|string|null",
            preg_replace_callback_array,
        ),
        (
            "function preg_split(string $pattern, string $subject, int $limit = -1, int $flags = 0): array|false",
            preg_split,
        ),
        (
            "function preg_quote(string $str, ?string $delimiter = null): string",
            preg_quote,
        ),
        (
            "function preg_grep(string $pattern, array $array, int $flags = 0): array|false",
            preg_grep,
        ),
        ("function preg_last_error(): int", preg_last_error),
        (
            "function preg_last_error_msg(): string",
            preg_last_error_msg,
        ),
    ],
    constants: &[
        ("PREG_PATTERN_ORDER", ConstVal::Int(1)),
        ("PREG_SET_ORDER", ConstVal::Int(2)),
        ("PREG_OFFSET_CAPTURE", ConstVal::Int(256)),
        ("PREG_UNMATCHED_AS_NULL", ConstVal::Int(512)),
        ("PREG_SPLIT_NO_EMPTY", ConstVal::Int(1)),
        ("PREG_SPLIT_DELIM_CAPTURE", ConstVal::Int(2)),
        ("PREG_SPLIT_OFFSET_CAPTURE", ConstVal::Int(4)),
        ("PREG_GREP_INVERT", ConstVal::Int(1)),
        ("PREG_NO_ERROR", ConstVal::Int(0)),
        ("PREG_INTERNAL_ERROR", ConstVal::Int(1)),
        ("PREG_BACKTRACK_LIMIT_ERROR", ConstVal::Int(2)),
        ("PREG_RECURSION_LIMIT_ERROR", ConstVal::Int(3)),
        ("PREG_BAD_UTF8_ERROR", ConstVal::Int(4)),
        ("PREG_BAD_UTF8_OFFSET_ERROR", ConstVal::Int(5)),
        ("PREG_JIT_STACKLIMIT_ERROR", ConstVal::Int(6)),
        ("PCRE_VERSION", ConstVal::Str("10.46 2025-08-27")),
        ("PCRE_VERSION_MAJOR", ConstVal::Int(10)),
        ("PCRE_VERSION_MINOR", ConstVal::Int(46)),
        ("PCRE_JIT_SUPPORT", ConstVal::Bool(true)),
    ],
    ini: &[
        ("pcre.backtrack_limit", "1000000"),
        ("pcre.recursion_limit", "100000"),
        ("pcre.jit", "1"),
    ],
    listed: true,
    ..Module::EMPTY
};

const OFFSET_CAPTURE: i64 = 256;
const UNMATCHED_AS_NULL: i64 = 512;
const PATTERN_ORDER: i64 = 1;
const SET_ORDER: i64 = 2;

/// Compiled patterns and the last error.
#[derive(Default)]
struct PcreState {
    cache: HashMap<PhpStr, Rc<Regex>>,
    last_error: i32,
}

fn set_error(vm: &mut Vm, code: i32) {
    vm.ext::<PcreState>().last_error = code;
}

/// Compiles (or fetches) a pattern; a failure warns and gives None.
fn regex(vm: &mut Vm, a: &Args, pat: &PhpStr) -> R<Option<Rc<Regex>>> {
    if let Some(r) = vm.ext::<PcreState>().cache.get(pat) {
        return Ok(Some(r.clone()));
    }
    match php_compile(pat) {
        Ok(re) => {
            let re = Rc::new(re);
            let st = vm.ext::<PcreState>();
            if st.cache.len() >= 4096 {
                st.cache.clear();
            }
            st.cache.insert(pat.clone(), re.clone());
            Ok(Some(re))
        }
        Err(e) => {
            set_error(vm, 1);
            vm.fwarn(a, &e.message())?;
            Ok(None)
        }
    }
}

fn ini_u32(vm: &Vm, k: &[u8], d: u32) -> u32 {
    match vm.get_ini(k) {
        Some(v) => {
            let n = goro_runtime::convert::str_to_int(&v);
            if n < 0 {
                u32::MAX
            } else {
                n.min(u32::MAX as i64) as u32
            }
        }
        None => d,
    }
}

fn limits_for(vm: &Vm, pat: &[u8]) -> Limits {
    let mut l = limits(vm);
    // (*NO_JIT) at the start of the pattern selects the interpreter (test
    // no_jit_bug70110.phpt).
    if pat.windows(9).any(|w| w == b"(*NO_JIT)") {
        l.jit_stack = None;
    }
    l
}

fn limits(vm: &Vm) -> Limits {
    let mut l = Limits::new(
        ini_u32(vm, b"pcre.backtrack_limit", 1_000_000),
        ini_u32(vm, b"pcre.recursion_limit", 100_000),
    );
    if vm
        .get_ini(b"pcre.jit")
        .is_some_and(|v| goro_runtime::convert::str_to_int(&v) != 0)
    {
        l.jit_stack = Some(Limits::PHP_JIT_STACK);
    }
    l
}

/// The value of one group in a match array.
fn group_value(subj: &[u8], caps: &Captures, i: usize, flags: i64) -> Value {
    let v = match caps.get(i) {
        Some((s, e)) if s <= e => Some((Value::from(&subj[s..e]), s as i64)),
        Some((s, _)) => Some((Value::from(&b""[..]), s as i64)),
        None => None,
    };
    let (val, off) = match v {
        Some(x) => x,
        None => (
            if flags & UNMATCHED_AS_NULL != 0 {
                Value::Null
            } else {
                Value::Str(PhpStr::empty())
            },
            -1,
        ),
    };
    if flags & OFFSET_CAPTURE != 0 {
        Value::Array(PhpArray::from_values([val, Value::Int(off)]))
    } else {
        val
    }
}

/// A match as preg_match() reports it.
fn match_array(
    re: &Regex,
    names: &[Option<Rc<[u8]>>],
    subj: &[u8],
    caps: &Captures,
    flags: i64,
) -> PhpArray {
    let n = if flags & UNMATCHED_AS_NULL != 0 {
        re.capture_count() + 1
    } else {
        caps.highest_set()
    };
    let mut out = PhpArray::new();
    for i in 0..n {
        let v = group_value(subj, caps, i, flags);
        if let Some(Some(name)) = names.get(i) {
            let k = Key::from_bytes(name);
            // Duplicate names (?J): the first group of the name that was
            // set gives the value (test bug79257.phpt).
            if out.get(k.as_ref()).is_none() {
                let best = (i..names.len())
                    .find(|&j| names[j].as_deref() == Some(&name[..]) && caps.get(j).is_some())
                    .unwrap_or(i);
                out.insert(k, group_value(subj, caps, best, flags));
            }
        }
        out.insert(Key::Int(i as i64), v);
    }
    if let Some(m) = caps.mark() {
        out.insert(Key::from("MARK"), Value::from(m));
    }
    out
}

fn names_of(re: &Regex) -> Vec<Option<Rc<[u8]>>> {
    re.group_names()
        .into_iter()
        .map(|n| n.map(|a| Rc::from(&a[..])))
        .collect()
}

fn match_error(vm: &mut Vm, e: &MatchError) {
    set_error(vm, preg_error_code(e));
}

/// Test gh16189.phpt: PHP_INT_MIN can't be negated.
fn offset_check(vm: &mut Vm, a: &Args) -> R<()> {
    if a.int_or(4, 0) == i64::MIN {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #5 ($offset) must be greater than {}", i64::MIN),
        );
    }
    Ok(())
}

/// Resolves an offset argument; None (an internal error) when outside.
fn start_offset(len: usize, off: i64) -> Option<usize> {
    let o = if off < 0 {
        (len as i64 + off).max(0)
    } else {
        off
    };
    if o as u64 > len as u64 {
        None
    } else {
        Some(o as usize)
    }
}

fn preg_match(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pat = a.str(0);
    let subj = a.str(1);
    let flags = a.int_or(3, 0);
    // The pattern is compiled before the flags are checked (test
    // match_flags3.phpt).
    let Some(re) = regex(vm, a, &pat)? else {
        return Ok(Value::Bool(false));
    };
    if flags & !(OFFSET_CAPTURE | UNMATCHED_AS_NULL) != 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($flags) must be a PREG_* constant",
        );
    }
    offset_check(vm, a)?;
    set_error(vm, 0);
    let Some(start) = start_offset(subj.len(), a.int_or(4, 0)) else {
        set_error(vm, 1);
        if a.has(2) {
            a.set_ref(2, Value::Array(PhpArray::new()));
        }
        return Ok(Value::Bool(false));
    };
    let lim = limits_for(vm, &pat);
    let mut scratch = Scratch::new();
    let mut caps = Captures::default();
    match re.exec_with(
        &mut scratch,
        &subj,
        start,
        MatchOptions::default(),
        lim,
        &mut caps,
    ) {
        Ok(true) => {
            if a.has(2) {
                let names = names_of(&re);
                let arr = match_array(&re, &names, &subj, &caps, flags);
                a.set_ref(2, Value::Array(arr));
            }
            Ok(Value::Int(1))
        }
        Ok(false) => {
            if a.has(2) {
                a.set_ref(2, Value::Array(PhpArray::new()));
            }
            Ok(Value::Int(0))
        }
        Err(e) => {
            match_error(vm, &e);
            if a.has(2) {
                a.set_ref(2, Value::Array(PhpArray::new()));
            }
            Ok(Value::Bool(false))
        }
    }
}

fn preg_match_all(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pat = a.str(0);
    let subj = a.str(1);
    let mut flags = a.int_or(3, 0);
    let order = flags & (PATTERN_ORDER | SET_ORDER);
    if order == PATTERN_ORDER | SET_ORDER
        || flags & !(PATTERN_ORDER | SET_ORDER | OFFSET_CAPTURE | UNMATCHED_AS_NULL) != 0
    {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($flags) must be a PREG_* constant",
        );
    }
    if order == 0 {
        flags |= PATTERN_ORDER;
    }
    let Some(re) = regex(vm, a, &pat)? else {
        return Ok(Value::Bool(false));
    };
    offset_check(vm, a)?;
    set_error(vm, 0);
    let Some(start) = start_offset(subj.len(), a.int_or(4, 0)) else {
        set_error(vm, 1);
        if a.has(2) {
            a.set_ref(2, Value::Array(PhpArray::new()));
        }
        return Ok(Value::Bool(false));
    };
    let names = names_of(&re);
    let ngroups = re.capture_count() + 1;
    let lim = limits_for(vm, &pat);
    let mut gm = GlobalMatcher::new(&re, &subj, start, lim);
    let mut count = 0i64;
    let mut sets: Vec<PhpArray> = Vec::new();
    let mut cols: Vec<PhpArray> = vec![PhpArray::new(); ngroups];
    let mut marks = PhpArray::new();
    // Values of duplicate-named groups that differ from their column.
    let mut dup_named: Vec<(Rc<[u8]>, Value)> = Vec::new();
    let mut named_cols: HashMap<Vec<u8>, PhpArray> = HashMap::new();
    let mut err = None;
    loop {
        match gm.next_match() {
            Ok(true) => {
                if flags & SET_ORDER != 0 {
                    sets.push(match_array(&re, &names, &subj, &gm.caps, flags));
                } else {
                    for (g, col) in cols.iter_mut().enumerate() {
                        // Duplicate names: the set group of the name.
                        let mut src = g;
                        if let Some(Some(name)) = names.get(g)
                            && names[..g].iter().any(|n| n.as_deref() == Some(&name[..]))
                        {
                            src = g;
                        } else if let Some(Some(name)) = names.get(g) {
                            src = (g..names.len())
                                .find(|&j| {
                                    names[j].as_deref() == Some(&name[..])
                                        && gm.caps.get(j).is_some()
                                })
                                .unwrap_or(g);
                        }
                        let _ = col.push(group_value(&subj, &gm.caps, g, flags));
                        if src != g
                            && let Some(Some(name)) = names.get(g)
                        {
                            dup_named
                                .push((name.clone(), group_value(&subj, &gm.caps, src, flags)));
                        }
                    }
                    if let Some(m) = gm.caps.mark() {
                        marks.insert(Key::Int(count), Value::from(m));
                    }
                    for (name, v) in dup_named.drain(..) {
                        named_cols
                            .entry(name.to_vec())
                            .or_default()
                            .insert(Key::Int(count), v);
                    }
                }
                count += 1;
            }
            Ok(false) => break,
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }
    if let Some(e) = err {
        match_error(vm, &e);
        if a.has(2) {
            a.set_ref(2, Value::Array(PhpArray::new()));
        }
        return Ok(Value::Bool(false));
    }
    if a.has(2) {
        let out = if flags & SET_ORDER != 0 {
            PhpArray::from_values(sets.into_iter().map(Value::Array))
        } else {
            let mut out = PhpArray::new();
            for (g, col) in cols.into_iter().enumerate() {
                if let Some(Some(name)) = names.get(g) {
                    let k = Key::from_bytes(name);
                    if out.get(k.as_ref()).is_none() {
                        let mut c = col.clone();
                        if let Some(over) = named_cols.get(&name[..]) {
                            for (i, v) in over.iter() {
                                c.insert(i.to_owned(), v.clone());
                            }
                        }
                        out.insert(k, Value::Array(c));
                    }
                }
                out.insert(Key::Int(g as i64), Value::Array(col));
            }
            if !marks.is_empty() {
                out.insert(Key::from("MARK"), Value::Array(marks));
            }
            out
        };
        a.set_ref(2, Value::Array(out));
    }
    Ok(Value::Int(count))
}

// ---------------------------------------------------------------- replace

enum Repl<'a> {
    Str(&'a [u8]),
    Callback(&'a Value, i64),
}

/// Replaces matches of `re` in `subj`; None on a match error.
fn replace_one(
    vm: &mut Vm,
    re: &Regex,
    subj: &[u8],
    repl: &Repl,
    limit: i64,
    count: &mut i64,
) -> R<Option<Vec<u8>>> {
    let lim = limits(vm);
    let names = match repl {
        Repl::Callback(..) => names_of(re),
        _ => Vec::new(),
    };
    let mut gm = GlobalMatcher::new(re, subj, 0, lim);
    let mut out = Vec::with_capacity(subj.len());
    let mut last = 0;
    let mut done = 0i64;
    while limit < 0 || done < limit {
        match gm.next_match() {
            Ok(true) => {
                let (s, e) = gm.caps.get(0).unwrap();
                out.extend_from_slice(&subj[last..s]);
                match repl {
                    Repl::Str(r) => expand_replacement(r, subj, &gm.caps, &mut out),
                    Repl::Callback(cb, flags) => {
                        let arr = match_array(re, &names, subj, &gm.caps, *flags);
                        let r = vm.call_value(cb, vec![Value::Array(arr)])?;
                        vm.append_to_string(&mut out, &r)?;
                    }
                }
                last = e.max(s);
                done += 1;
            }
            Ok(false) => break,
            Err(e) => {
                match_error(vm, &e);
                return Ok(None);
            }
        }
    }
    out.extend_from_slice(&subj[last..]);
    *count += done;
    Ok(Some(out))
}

fn to_strs(vm: &mut Vm, arr: &PhpArray) -> R<Vec<PhpStr>> {
    let mut out = Vec::with_capacity(arr.len());
    for (_, v) in arr.iter() {
        out.push(vm.to_php_str(&v.deref())?);
    }
    Ok(out)
}

/// Applies every pattern to one subject; None on an error.
fn replace_subject(
    vm: &mut Vm,
    a: &Args,
    pats: &[(PhpStr, ReplSpec)],
    subj: PhpStr,
    limit: i64,
    count: &mut i64,
) -> R<Option<PhpStr>> {
    let mut cur = subj;
    for (p, r) in pats {
        let Some(re) = regex(vm, a, p)? else {
            return Ok(None);
        };
        let repl = match r {
            ReplSpec::Str(s) => Repl::Str(s),
            ReplSpec::Callback(cb, f) => Repl::Callback(cb, *f),
        };
        let before = *count;
        match replace_one(vm, &re, &cur, &repl, limit, count)? {
            Some(out) => {
                if *count != before {
                    cur = PhpStr::from(out);
                }
            }
            None => return Ok(None),
        }
    }
    Ok(Some(cur))
}

enum ReplSpec {
    Str(PhpStr),
    Callback(Value, i64),
}

fn run_replace(
    vm: &mut Vm,
    a: &mut Args,
    pats: Vec<(PhpStr, ReplSpec)>,
    subject: Value,
    limit: i64,
    count_arg: usize,
    filter: bool,
) -> R<Value> {
    set_error(vm, 0);
    let mut count = 0i64;
    let result = match subject {
        Value::Array(arr) => {
            let mut out = PhpArray::new();
            for (k, v) in arr.iter() {
                let s = vm.to_php_str(&v.deref())?;
                let before = count;
                if let Some(r) = replace_subject(vm, a, &pats, s, limit, &mut count)?
                    && (!filter || count != before)
                {
                    out.insert(k.to_owned(), Value::Str(r));
                }
            }
            Value::Array(out)
        }
        other => {
            let s = vm.to_php_str(&other)?;
            match replace_subject(vm, a, &pats, s, limit, &mut count)? {
                Some(r) if !filter || count > 0 => Value::Str(r),
                _ => Value::Null,
            }
        }
    };
    if a.has(count_arg) {
        a.set_ref(count_arg, Value::Int(count));
    }
    Ok(result)
}

fn replace_common(vm: &mut Vm, a: &mut Args, filter: bool) -> R<Value> {
    let pattern = a.val(0);
    let replacement = a.val(1);
    let pats = match (&pattern, &replacement) {
        (Value::Str(p), Value::Str(r)) => vec![(p.clone(), ReplSpec::Str(r.clone()))],
        (Value::Str(_), Value::Array(_)) => {
            return vm.fthrow(
                a,
                "TypeError",
                "Argument #1 ($pattern) must be of type array when argument #2 ($replacement) is an array, string given",
            );
        }
        (Value::Array(ps), Value::Str(r)) => to_strs(vm, ps)?
            .into_iter()
            .map(|p| (p, ReplSpec::Str(r.clone())))
            .collect(),
        (Value::Array(ps), Value::Array(rs)) => {
            let ps = to_strs(vm, ps)?;
            let mut rs = to_strs(vm, rs)?.into_iter();
            ps.into_iter()
                .map(|p| (p, ReplSpec::Str(rs.next().unwrap_or_default())))
                .collect()
        }
        _ => unreachable!(),
    };
    let limit = a.int_or(3, -1);
    let subject = a.val(2);
    run_replace(vm, a, pats, subject, limit, 4, filter)
}

fn preg_replace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    replace_common(vm, a, false)
}
fn preg_filter(vm: &mut Vm, a: &mut Args) -> R<Value> {
    replace_common(vm, a, true)
}

fn callback_flags(vm: &mut Vm, a: &Args, i: usize) -> R<i64> {
    let f = a.int_or(i, 0);
    if f & !(OFFSET_CAPTURE | UNMATCHED_AS_NULL) != 0 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} ($flags) must be a PREG_* constant", i + 1),
        );
    }
    Ok(f)
}

fn preg_replace_callback(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cb = a.val(1);
    let flags = callback_flags(vm, a, 5)?;
    let pats = match a.val(0) {
        Value::Array(ps) => to_strs(vm, &ps)?
            .into_iter()
            .map(|p| (p, ReplSpec::Callback(cb.clone(), flags)))
            .collect(),
        Value::Str(p) => vec![(p, ReplSpec::Callback(cb.clone(), flags))],
        _ => unreachable!(),
    };
    let limit = a.int_or(3, -1);
    let subject = a.val(2);
    run_replace(vm, a, pats, subject, limit, 4, false)
}

fn preg_replace_callback_array(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = callback_flags(vm, a, 4)?;
    let mut pats = Vec::new();
    for (k, v) in a.array(0).iter() {
        let p = match k {
            goro_runtime::KeyRef::Str(s) => s.clone(),
            goro_runtime::KeyRef::Int(_) => {
                return vm.fthrow(
                    a,
                    "TypeError",
                    "Argument #1 ($pattern) must contain only string patterns as keys",
                );
            }
        };
        let cb = v.deref();
        if !vm.is_callable_value(&cb, false) {
            return vm.fthrow(
                a,
                "TypeError",
                "Argument #1 ($pattern) must contain only valid callbacks",
            );
        }
        pats.push((p, ReplSpec::Callback(cb, flags)));
    }
    let limit = a.int_or(2, -1);
    let subject = a.val(1);
    run_replace(vm, a, pats, subject, limit, 3, false)
}

// ---------------------------------------------------------------- split / grep / quote

fn preg_split(vm: &mut Vm, a: &mut Args) -> R<Value> {
    const NO_EMPTY: i64 = 1;
    const DELIM: i64 = 2;
    const OFFSET: i64 = 4;
    let pat = a.str(0);
    let subj = a.str(1);
    let mut limit = a.int_or(2, -1);
    let flags = a.int_or(3, 0);
    let Some(re) = regex(vm, a, &pat)? else {
        return Ok(Value::Bool(false));
    };
    set_error(vm, 0);
    if limit == 0 {
        limit = -1;
    }
    let mut out = PhpArray::new();
    let push = |out: &mut PhpArray, s: usize, e: usize| {
        let v = Value::from(&subj[s..e]);
        if flags & OFFSET != 0 {
            let _ = out.push(Value::Array(PhpArray::from_values([
                v,
                Value::Int(s as i64),
            ])));
        } else {
            let _ = out.push(v);
        }
    };
    let lim = limits(vm);
    let mut gm = GlobalMatcher::new(&re, &subj, 0, lim);
    let mut last = 0usize;
    loop {
        if limit > 0 && out.len() as i64 >= limit - 1 {
            break;
        }
        match gm.next_match() {
            Ok(true) => {
                let (s, e) = gm.caps.get(0).unwrap();
                if !(flags & NO_EMPTY != 0 && s == last) {
                    push(&mut out, last, s);
                } else if s == e {
                    continue;
                }
                if flags & DELIM != 0 {
                    let n = gm.caps.highest_set();
                    for g in 1..n {
                        if let Some((gs, ge)) = gm.caps.get(g) {
                            if flags & NO_EMPTY != 0 && gs == ge {
                                continue;
                            }
                            push(&mut out, gs, ge);
                        } else if flags & NO_EMPTY == 0 {
                            push(&mut out, 0, 0);
                        }
                    }
                }
                last = e;
            }
            Ok(false) => break,
            Err(e) => {
                match_error(vm, &e);
                return Ok(Value::Bool(false));
            }
        }
    }
    if !(flags & NO_EMPTY != 0 && last >= subj.len()) {
        push(&mut out, last, subj.len());
    }
    Ok(Value::Array(out))
}

fn preg_quote(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.bytes(0);
    let delim = a.opt_str(1).and_then(|d| d.first().copied());
    let mut out = Vec::with_capacity(s.len() + 8);
    for &c in s {
        match c {
            b'.' | b'\\' | b'+' | b'*' | b'?' | b'[' | b'^' | b']' | b'$' | b'(' | b')' | b'{'
            | b'}' | b'=' | b'!' | b'<' | b'>' | b'|' | b':' | b'-' | b'#' => {
                out.push(b'\\');
                out.push(c);
            }
            0 => out.extend_from_slice(b"\\000"),
            _ if Some(c) == delim => {
                out.push(b'\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    Ok(sv(out))
}

fn preg_grep(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let pat = a.str(0);
    let invert = a.int_or(2, 0) & 1 != 0;
    let Some(re) = regex(vm, a, &pat)? else {
        return Ok(Value::Bool(false));
    };
    set_error(vm, 0);
    let lim = limits(vm);
    let mut scratch = Scratch::new();
    let mut caps = Captures::default();
    let mut out = PhpArray::new();
    for (k, v) in a.array(1).iter() {
        let s = vm.to_php_str(&v.deref())?;
        match re.exec_with(&mut scratch, &s, 0, MatchOptions::default(), lim, &mut caps) {
            Ok(m) => {
                if m != invert {
                    // Test bug44925.phpt: shared references are kept.
                    let keep = match v {
                        Value::Ref(r) if r.refcount() > 1 => v.clone(),
                        other => other.deref(),
                    };
                    out.insert(k.to_owned(), keep);
                }
            }
            Err(e) => {
                match_error(vm, &e);
                break;
            }
        }
    }
    Ok(Value::Array(out))
}

fn preg_last_error(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(vm.ext::<PcreState>().last_error as i64))
}

fn preg_last_error_msg(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let m = match vm.ext::<PcreState>().last_error {
        0 => "No error",
        1 => "Internal error",
        2 => "Backtrack limit exhausted",
        3 => "Recursion limit exhausted",
        4 => "Malformed UTF-8 characters, possibly incorrectly encoded",
        5 => "The offset did not correspond to the beginning of a valid UTF-8 code point",
        6 => "JIT stack limit exhausted",
        _ => "Unknown error",
    };
    Ok(Value::from(m))
}
