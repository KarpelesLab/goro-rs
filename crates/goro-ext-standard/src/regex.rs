//! ext/pcre: the preg_* functions, a port of php-src ext/pcre/php_pcre.c on
//! top of goro-pcre (a pure-Rust port of the PCRE2 10.44 library bundled
//! with php-src).

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use goro_pcre::constants::*;
use goro_pcre::{Code, CompileContext, MatchContext, MatchData};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const PREG_PATTERN_ORDER: i64 = 1;
const PREG_SET_ORDER: i64 = 2;
const PREG_OFFSET_CAPTURE: i64 = 1 << 8;
const PREG_UNMATCHED_AS_NULL: i64 = 1 << 9;
const PREG_SPLIT_NO_EMPTY: i64 = 1;
const PREG_SPLIT_DELIM_CAPTURE: i64 = 1 << 1;
const PREG_SPLIT_OFFSET_CAPTURE: i64 = 1 << 2;
const PREG_GREP_INVERT: i64 = 1;

const PHP_PCRE_NO_ERROR: i64 = 0;
const PHP_PCRE_INTERNAL_ERROR: i64 = 1;
const PHP_PCRE_BACKTRACK_LIMIT_ERROR: i64 = 2;
const PHP_PCRE_RECURSION_LIMIT_ERROR: i64 = 3;
const PHP_PCRE_BAD_UTF8_ERROR: i64 = 4;
const PHP_PCRE_BAD_UTF8_OFFSET_ERROR: i64 = 5;
const PHP_PCRE_JIT_STACKLIMIT_ERROR: i64 = 6;

/// The match depth at which the emulated JIT runs out of its stack.
/// goro-pcre is an interpreter: pcre.jit=1 (the default, like a PHP built
/// with PCRE JIT support) gives JIT semantics instead, where
/// pcre.recursion_limit does not apply and a match nesting deeper than
/// about what PHP's 192 KiB JIT stack holds (some 10000 repetitions of a
/// capturing group) fails with PREG_JIT_STACKLIMIT_ERROR.
const JIT_DEPTH_LIMIT: u32 = 20_000;

const PCRE_CACHE_SIZE: usize = 4096;

/// A compiled pattern with the data php_pcre.c keeps per cache entry.
pub struct Pce {
    pub re: Code,
    pub compile_options: u32,
    pub capture_count: u32,
    pub name_count: u32,
    /// Group number -> name (the "subpats table").
    pub subpat_names: Option<Vec<Option<PhpString>>>,
}

thread_local! {
    static PCRE_CACHE: RefCell<goro_core::fxhash::FxHashMap<Vec<u8>, Rc<Pce>>> = Default::default();
    /// Character tables per LC_CTYPE locale (the char_tables hash).
    static CHAR_TABLES: RefCell<HashMap<Vec<u8>, &'static [u8]>> = RefCell::new(HashMap::new());
}

/// The current LC_CTYPE name when setlocale() changed it (BG(ctype_string)).
fn ctype_string() -> Option<Vec<u8>> {
    if !crate::locale::ctype_locale_active() {
        return None;
    }
    unsafe {
        let p = libc::setlocale(libc::LC_CTYPE, std::ptr::null());
        if p.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(p).to_bytes().to_vec())
    }
}

/// pcre2_maketables() for the current LC_CTYPE.
fn locale_tables(name: &[u8]) -> &'static [u8] {
    if let Some(t) = CHAR_TABLES.with(|c| c.borrow().get(name).copied()) {
        return t;
    }
    let t = goro_pcre::maketables(|b| unsafe {
        let i = b as i32;
        let lo = libc::tolower(i);
        let up = libc::toupper(i);
        goro_pcre::CtypeInfo {
            lower: if (0..256).contains(&lo) { lo as u8 } else { b },
            upper: if (0..256).contains(&up) { up as u8 } else { b },
            is_lower: libc::islower(i) != 0,
            is_upper: libc::isupper(i) != 0,
            is_alpha: libc::isalpha(i) != 0,
            is_digit: libc::isdigit(i) != 0,
            is_alnum: libc::isalnum(i) != 0,
            is_space: libc::isspace(i) != 0,
            is_xdigit: libc::isxdigit(i) != 0,
            is_graph: libc::isgraph(i) != 0,
            is_print: libc::isprint(i) != 0,
            is_punct: libc::ispunct(i) != 0,
            is_cntrl: libc::iscntrl(i) != 0,
        }
    });
    let t: &'static [u8] = Box::leak(t.into_boxed_slice());
    CHAR_TABLES.with(|c| c.borrow_mut().insert(name.to_vec(), t));
    t
}

fn throw(vm: &mut Vm, class: &[u8], msg: String) -> VmError {
    let line = vm.current_line;
    let exc = vm.create_exception(class, &msg, line);
    vm.current_exception = Some(exc);
    VmError {
        message: format!("Uncaught {}: {}", goro_core::string::msg_str(class), msg),
        line,
    }
}

/// pcre_handle_exec_error()
fn pcre_handle_exec_error(vm: &mut Vm, pcre_code: i32) {
    let preg_code = match pcre_code {
        PCRE2_ERROR_MATCHLIMIT => PHP_PCRE_BACKTRACK_LIMIT_ERROR,
        PCRE2_ERROR_JIT_STACKLIMIT => PHP_PCRE_JIT_STACKLIMIT_ERROR,
        PCRE2_ERROR_RECURSIONLIMIT => PHP_PCRE_RECURSION_LIMIT_ERROR,
        PCRE2_ERROR_BADUTFOFFSET => PHP_PCRE_BAD_UTF8_OFFSET_ERROR,
        c if (PCRE2_ERROR_UTF8_ERR21..=PCRE2_ERROR_UTF8_ERR1).contains(&c) => {
            PHP_PCRE_BAD_UTF8_ERROR
        }
        _ => PHP_PCRE_INTERNAL_ERROR,
    };
    vm.preg_last_error = preg_code;
}

/// php_pcre_get_error_msg()
fn php_pcre_get_error_msg(code: i64) -> &'static str {
    match code {
        PHP_PCRE_NO_ERROR => "No error",
        PHP_PCRE_INTERNAL_ERROR => "Internal error",
        PHP_PCRE_BAD_UTF8_ERROR => "Malformed UTF-8 characters, possibly incorrectly encoded",
        PHP_PCRE_BAD_UTF8_OFFSET_ERROR => {
            "The offset did not correspond to the beginning of a valid UTF-8 code point"
        }
        PHP_PCRE_BACKTRACK_LIMIT_ERROR => "Backtrack limit exhausted",
        PHP_PCRE_RECURSION_LIMIT_ERROR => "Recursion limit exhausted",
        PHP_PCRE_JIT_STACKLIMIT_ERROR => "JIT stack limit exhausted",
        _ => "Unknown error",
    }
}

/// C isspace() in the "C" locale.
#[inline]
fn c_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// ZEND_STRTOL(value, NULL, 10) as used by OnUpdateLong.
fn ini_strtol(v: &Value) -> i64 {
    match v {
        Value::Long(n) => *n,
        Value::True => 1,
        Value::False | Value::Null | Value::Undef => 0,
        Value::Double(d) => *d as i64,
        Value::String(s) => strtol_bytes(s.as_bytes()),
        other => strtol_bytes(other.to_php_string().as_bytes()),
    }
}

/// ini_strtol() of the value of pcre ini setting `slot` (see
/// match_context): a string value is parsed once (while the setting keeps
/// that string).
fn ini_strtol_memo(slot: usize, v: &Value) -> i64 {
    thread_local! {
        static PARSED: RefCell<[Option<(PhpString, i64)>; 3]> = const { RefCell::new([None, None, None]) };
    }
    let Value::String(s) = v else {
        return ini_strtol(v);
    };
    PARSED.with(|p| {
        let mut p = p.borrow_mut();
        match &p[slot] {
            Some((cached, n)) if cached.ptr_eq(s) => *n,
            _ => {
                let n = strtol_bytes(s.as_bytes());
                p[slot] = Some((s.clone(), n));
                n
            }
        }
    })
}

/// ZEND_STRTOL(s, NULL, 10).
fn strtol_bytes(b: &[u8]) -> i64 {
    let mut i = 0;
    while i < b.len() && c_isspace(b[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        neg = b[i] == b'-';
        i += 1;
    }
    let mut n: i64 = 0;
    let mut overflow = false;
    while i < b.len() && b[i].is_ascii_digit() {
        let d = (b[i] - b'0') as i64;
        match n.checked_mul(10).and_then(|x| {
            if neg {
                x.checked_sub(d)
            } else {
                x.checked_add(d)
            }
        }) {
            Some(x) => n = x,
            None => overflow = true,
        }
        i += 1;
    }
    if overflow {
        if neg { i64::MIN } else { i64::MAX }
    } else {
        n
    }
}

/// The match context: pcre.backtrack_limit and pcre.recursion_limit (not
/// used by the JIT: see JIT_DEPTH_LIMIT).
fn match_context(vm: &Vm) -> MatchContext {
    let jit = vm
        .constants
        .get(b"pcre.jit".as_ref())
        .is_none_or(|v| ini_strtol_memo(0, v) != 0);
    let bl = vm
        .constants
        .get(b"pcre.backtrack_limit".as_ref())
        .map(|v| ini_strtol_memo(1, v))
        .unwrap_or(1000000);
    let rl = vm
        .constants
        .get(b"pcre.recursion_limit".as_ref())
        .map(|v| ini_strtol_memo(2, v))
        .unwrap_or(100000);
    MatchContext {
        match_limit: bl as u32,
        depth_limit: rl as u32,
        jit_depth_limit: jit.then_some(JIT_DEPTH_LIMIT),
        ..Default::default()
    }
}

/// make_subpats_table()
fn make_subpats_table(re: &Code, num_subpats: usize) -> Vec<Option<PhpString>> {
    let mut names: Vec<Option<PhpString>> = vec![None; num_subpats];
    for (num, name) in re.names() {
        if (num as usize) < num_subpats {
            names[num as usize] = Some(PhpString::from_vec(name));
        }
    }
    names
}

/// pcre_get_compiled_regex_cache(): compile (or fetch from the cache) a
/// PHP pattern with delimiters and modifiers. Emits the PHP warnings.
pub fn get_compiled_regex(vm: &mut Vm, regex: &[u8], fname: &str) -> Option<Rc<Pce>> {
    let ctype = ctype_string();
    let with_ctype: Vec<u8>;
    let key: &[u8] = match &ctype {
        Some(c) => {
            with_ctype = [c.as_slice(), regex].concat();
            &with_ctype
        }
        None => regex,
    };
    if let Some(p) = PCRE_CACHE.with(|c| c.borrow().get(key).cloned()) {
        return Some(p);
    }
    let end = regex.len();
    let mut p = 0usize;
    while p < end && c_isspace(regex[p]) {
        p += 1;
    }
    if p >= end {
        vm.emit_warning(&format!("{}(): Empty regular expression", fname));
        pcre_handle_exec_error(vm, PCRE2_ERROR_INTERNAL);
        return None;
    }
    let mut delimiter = regex[p];
    p += 1;
    if delimiter.is_ascii_alphanumeric() || delimiter == b'\\' || delimiter == 0 {
        vm.emit_warning(&format!(
            "{}(): Delimiter must not be alphanumeric, backslash, or NUL byte",
            fname
        ));
        pcre_handle_exec_error(vm, PCRE2_ERROR_INTERNAL);
        return None;
    }
    let start_delimiter = delimiter;
    const BRACKETS: &[u8] = b"([{< )]}> )]}>";
    if let Some(pos) = BRACKETS.iter().position(|&b| b == delimiter) {
        delimiter = BRACKETS[pos + 5];
    }
    let end_delimiter = delimiter;
    let mut pp = p;
    if start_delimiter == end_delimiter {
        while pp < end {
            if regex[pp] == b'\\' && pp + 1 < end {
                pp += 1;
            } else if regex[pp] == delimiter {
                break;
            }
            pp += 1;
        }
    } else {
        let mut brackets = 1;
        while pp < end {
            if regex[pp] == b'\\' && pp + 1 < end {
                pp += 1;
            } else if regex[pp] == end_delimiter {
                brackets -= 1;
                if brackets <= 0 {
                    break;
                }
            } else if regex[pp] == start_delimiter {
                brackets += 1;
            }
            pp += 1;
        }
    }
    if pp >= end {
        if start_delimiter == end_delimiter {
            vm.emit_warning(&format!(
                "{}(): No ending delimiter '{}' found",
                fname,
                goro_core::string::msg_char(delimiter)
            ));
        } else {
            vm.emit_warning(&format!(
                "{}(): No ending matching delimiter '{}' found",
                fname,
                goro_core::string::msg_char(delimiter)
            ));
        }
        pcre_handle_exec_error(vm, PCRE2_ERROR_INTERNAL);
        return None;
    }
    let pattern = &regex[p..pp];
    pp += 1;

    let mut coptions: u32 = 0;
    let mut eoptions: u32 = 0;
    while pp < end {
        let c = regex[pp];
        pp += 1;
        match c {
            b'i' => coptions |= PCRE2_CASELESS,
            b'm' => coptions |= PCRE2_MULTILINE,
            b'n' => coptions |= PCRE2_NO_AUTO_CAPTURE,
            b's' => coptions |= PCRE2_DOTALL,
            b'x' => coptions |= PCRE2_EXTENDED,
            b'A' => coptions |= PCRE2_ANCHORED,
            b'D' => coptions |= PCRE2_DOLLAR_ENDONLY,
            b'r' => eoptions |= PCRE2_EXTRA_CASELESS_RESTRICT,
            b'S' | b'X' => {}
            b'U' => coptions |= PCRE2_UNGREEDY,
            b'u' => coptions |= PCRE2_UTF | PCRE2_UCP,
            b'J' => coptions |= PCRE2_DUPNAMES,
            b' ' | b'\n' | b'\r' => {}
            _ => {
                if c != 0 {
                    vm.emit_warning(&format!(
                        "{}(): Unknown modifier '{}'",
                        fname,
                        goro_core::string::msg_char(c)
                    ));
                } else {
                    vm.emit_warning(&format!("{}(): NUL byte is not a valid modifier", fname));
                }
                pcre_handle_exec_error(vm, PCRE2_ERROR_INTERNAL);
                return None;
            }
        }
    }

    let cctx = CompileContext {
        extra_options: eoptions,
        tables: ctype.as_deref().map(locale_tables),
        ..Default::default()
    };
    let re = match Code::compile(pattern, coptions, &cctx) {
        Ok(re) => re,
        Err(e) => {
            vm.emit_warning(&format!(
                "{}(): Compilation failed: {} at offset {}",
                fname,
                e.message(),
                e.offset
            ));
            pcre_handle_exec_error(vm, PCRE2_ERROR_INTERNAL);
            return None;
        }
    };

    let capture_count = re.capture_count();
    let name_count = re.name_count as u32;
    let pce = Rc::new(Pce {
        subpat_names: if name_count > 0 {
            Some(make_subpats_table(&re, capture_count as usize + 1))
        } else {
            None
        },
        re,
        compile_options: coptions,
        capture_count,
        name_count,
    });
    PCRE_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= PCRE_CACHE_SIZE {
            // pcre_clean_cache(): drop an eighth of the entries.
            let drop: Vec<Vec<u8>> = c.keys().take(PCRE_CACHE_SIZE / 8).cloned().collect();
            for k in drop {
                c.remove(&k);
            }
        }
        c.insert(key.to_vec(), pce.clone());
    });
    Some(pce)
}

#[inline]
fn str_val(subject: &[u8], start: usize, end: usize) -> Value {
    Value::String(PhpString::from_bytes(&subject[start..end]))
}

fn arr_val(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

/// populate_match_value()
fn match_value(subject: &[u8], start: usize, end: usize, unmatched_as_null: bool) -> Value {
    if start == PCRE2_UNSET {
        if unmatched_as_null {
            Value::Null
        } else {
            Value::String(PhpString::empty())
        }
    } else {
        str_val(subject, start, end)
    }
}

/// The [value, offset] pair of add_offset_pair().
fn offset_pair(subject: &[u8], start: usize, end: usize, unmatched_as_null: bool) -> Value {
    let mut pair = PhpArray::with_capacity(2);
    if start == PCRE2_UNSET {
        pair.push(if unmatched_as_null {
            Value::Null
        } else {
            Value::String(PhpString::empty())
        });
        pair.push(Value::Long(-1));
    } else {
        pair.push(str_val(subject, start, end));
        pair.push(Value::Long(start as i64));
    }
    arr_val(pair)
}

fn deep_clone(v: &Value) -> Value {
    match v {
        Value::Array(a) => arr_val(a.borrow().clone()),
        other => other.clone(),
    }
}

/// add_named(): name => value (only if absent when unmatched).
fn add_named(ht: &mut PhpArray, name: &PhpString, val: &Value, unmatched: bool) {
    let key = ArrayKey::String(name.clone());
    if !unmatched || !ht.contains_key(&key) {
        ht.set(key, deep_clone(val));
    }
}

/// add_offset_pair()
fn add_offset_pair(
    ht: &mut PhpArray,
    subject: &[u8],
    start: usize,
    end: usize,
    name: Option<&PhpString>,
    unmatched_as_null: bool,
) {
    let pair = offset_pair(subject, start, end, unmatched_as_null);
    if let Some(n) = name {
        add_named(ht, n, &pair, start == PCRE2_UNSET);
    }
    ht.push(pair);
}

/// populate_subpat_array()
#[allow(clippy::too_many_arguments)]
fn populate_subpat_array(
    ht: &mut PhpArray,
    subject: &[u8],
    offsets: &[usize],
    subpat_names: Option<&Vec<Option<PhpString>>>,
    num_subpats: usize,
    count: usize,
    mark: Option<&Vec<u8>>,
    flags: i64,
) {
    let offset_capture = (flags & PREG_OFFSET_CAPTURE) != 0;
    let unmatched_as_null = (flags & PREG_UNMATCHED_AS_NULL) != 0;
    if let Some(names) = subpat_names {
        if offset_capture {
            for i in 0..count {
                add_offset_pair(
                    ht,
                    subject,
                    offsets[2 * i],
                    offsets[2 * i + 1],
                    names[i].as_ref(),
                    unmatched_as_null,
                );
            }
            if unmatched_as_null {
                for name in names.iter().take(num_subpats).skip(count) {
                    add_offset_pair(ht, subject, PCRE2_UNSET, PCRE2_UNSET, name.as_ref(), true);
                }
            }
        } else {
            for i in 0..count {
                let val = match_value(
                    subject,
                    offsets[2 * i],
                    offsets[2 * i + 1],
                    unmatched_as_null,
                );
                if let Some(n) = &names[i] {
                    add_named(ht, n, &val, offsets[2 * i] == PCRE2_UNSET);
                }
                ht.push(val);
            }
            if unmatched_as_null {
                for name in names.iter().take(num_subpats).skip(count) {
                    if let Some(n) = name {
                        let key = ArrayKey::String(n.clone());
                        if !ht.contains_key(&key) {
                            ht.set(key, Value::Null);
                        }
                    }
                    ht.push(Value::Null);
                }
            }
        }
    } else if offset_capture {
        for i in 0..count {
            add_offset_pair(
                ht,
                subject,
                offsets[2 * i],
                offsets[2 * i + 1],
                None,
                unmatched_as_null,
            );
        }
        if unmatched_as_null {
            for _ in count..num_subpats {
                add_offset_pair(ht, subject, PCRE2_UNSET, PCRE2_UNSET, None, true);
            }
        }
    } else {
        for i in 0..count {
            ht.push(match_value(
                subject,
                offsets[2 * i],
                offsets[2 * i + 1],
                unmatched_as_null,
            ));
        }
        if unmatched_as_null {
            for _ in count..num_subpats {
                ht.push(Value::Null);
            }
        }
    }
    if let Some(m) = mark {
        let key = ArrayKey::String(PhpString::from_bytes(b"MARK"));
        if !ht.contains_key(&key) {
            ht.set(key, Value::String(PhpString::from_bytes(m)));
        }
    }
}

/// calculate_unit_length()
fn unit_length(pce: &Pce, subject: &[u8], start: usize) -> usize {
    if (pce.compile_options & PCRE2_UTF) != 0 {
        let mut e = start + 1;
        while e < subject.len() && (subject[e] & 0xc0) == 0x80 {
            e += 1;
        }
        e - start
    } else {
        1
    }
}

fn pcre_match(
    pce: &Pce,
    subject: &[u8],
    start: usize,
    options: u32,
    md: &mut MatchData,
    mctx: &MatchContext,
) -> i32 {
    pce.re.match_at(subject, start, options, md, mctx)
}

/// pcre2_match() of a compiled pattern on a whole subject (no captures
/// kept), as other extensions use it (e.g. ext/filter): the pcre2_match()
/// return code, negative when there is no match or on error.
pub fn pcre_match_subject(vm: &Vm, pce: &Pce, subject: &[u8]) -> i32 {
    let mctx = match_context(vm);
    let mut md = MatchData::new(pce.capture_count as usize + 1);
    pcre_match(pce, subject, 0, 0, &mut md, &mctx)
}

fn set_ref(arg: Option<&Value>, v: Value) {
    if let Some(Value::Reference(r)) = arg {
        *r.borrow_mut() = v;
    }
}

fn arg_str(args: &[Value], i: usize) -> PhpString {
    args.get(i)
        .map(|v| v.deref().to_php_string())
        .unwrap_or_else(PhpString::empty)
}

fn arg_long(args: &[Value], i: usize, default: i64) -> i64 {
    match args.get(i).map(|v| v.deref()) {
        None | Some(Value::Undef) => default,
        Some(v) => v.to_long(),
    }
}

/// php_do_pcre_match()
fn php_do_pcre_match(vm: &mut Vm, args: &[Value], global: bool) -> Result<Value, VmError> {
    let fname = if global {
        "preg_match_all"
    } else {
        "preg_match"
    };
    let regex = arg_str(args, 0);
    let subject = arg_str(args, 1);
    let subpats = args.get(2).filter(|v| matches!(v, Value::Reference(_)));
    let flags = arg_long(args, 3, 0);
    let start_offset = arg_long(args, 4, 0);
    let pce = match get_compiled_regex(vm, regex.as_bytes(), fname) {
        Some(p) => p,
        None => return Ok(Value::False),
    };
    if start_offset == i64::MIN {
        return Err(throw(
            vm,
            b"ValueError",
            format!(
                "{}(): Argument #5 ($offset) must be greater than {}",
                fname,
                i64::MIN
            ),
        ));
    }
    php_pcre_match_impl(
        vm,
        &pce,
        &subject,
        subpats,
        global,
        flags,
        start_offset,
        fname,
    )
}

/// php_pcre_match_impl()
#[allow(clippy::too_many_arguments)]
fn php_pcre_match_impl(
    vm: &mut Vm,
    pce: &Pce,
    subject_str: &PhpString,
    subpats: Option<&Value>,
    global: bool,
    flags: i64,
    start_offset: i64,
    fname: &str,
) -> Result<Value, VmError> {
    let subject = subject_str.as_bytes();
    let subject_len = subject.len();
    if subpats.is_some() {
        set_ref(subpats, arr_val(PhpArray::new()));
    }
    let mut subpats_order = if global { PREG_PATTERN_ORDER } else { 0 };
    let (offset_capture, unmatched_as_null) = if flags != 0 {
        if (flags & 0xff) != 0 {
            subpats_order = flags & 0xff;
            if (global && !(PREG_PATTERN_ORDER..=PREG_SET_ORDER).contains(&subpats_order))
                || (!global && subpats_order != 0)
            {
                return Err(throw(
                    vm,
                    b"ValueError",
                    format!(
                        "{}(): Argument #4 ($flags) must be a PREG_* constant",
                        fname
                    ),
                ));
            }
        }
        (
            (flags & PREG_OFFSET_CAPTURE) != 0,
            (flags & PREG_UNMATCHED_AS_NULL) != 0,
        )
    } else {
        (false, false)
    };

    let mut start_offset2: usize = if start_offset < 0 {
        subject_len.saturating_sub(start_offset.unsigned_abs() as usize)
    } else {
        start_offset as usize
    };
    if start_offset2 > subject_len {
        pcre_handle_exec_error(vm, PCRE2_ERROR_BADOFFSET);
        return Ok(Value::False);
    }

    let num_subpats = pce.capture_count as usize + 1;
    let subpat_names = if subpats.is_some() {
        pce.subpat_names.as_ref()
    } else {
        None
    };
    let mut matched: i64 = 0;
    vm.preg_last_error = PHP_PCRE_NO_ERROR;
    let mctx = match_context(vm);
    let mut md = MatchData::new(num_subpats);

    let mut match_sets: Option<Vec<PhpArray>> =
        if global && subpats.is_some() && subpats_order == PREG_PATTERN_ORDER {
            Some((0..num_subpats).map(|_| PhpArray::new()).collect())
        } else {
            None
        };
    let mut marks: Option<PhpArray> = None;
    let mut result = PhpArray::new();

    let orig_start_offset = start_offset2;
    // is_known_valid_utf8()
    let known_valid = subject_str.is_known_valid_utf8()
        && (start_offset2 == subject_len || (subject[start_offset2] & 0xc0) != 0x80);
    let options = if (pce.compile_options & PCRE2_UTF) != 0 && !known_valid {
        0
    } else {
        PCRE2_NO_UTF_CHECK
    };
    let mut count = pcre_match(pce, subject, start_offset2, options, &mut md, &mctx);

    'outer: loop {
        if count >= 0 {
            let mut c = count as usize;
            if c == 0 {
                vm.emit_notice_at(
                    &format!("{}(): Matched, but too many substrings", fname),
                    vm.current_line,
                );
                c = num_subpats;
            }
            // matched:
            loop {
                matched += 1;
                if subpats.is_some() {
                    let offsets = &md.ovector;
                    if offsets[1] < offsets[0] {
                        vm.emit_warning(&format!("{}(): Get subpatterns list failed", fname));
                        if !global || match_sets.is_none() {
                            set_ref(subpats, arr_val(result));
                        }
                        return Ok(Value::False);
                    }
                    if global {
                        if subpats_order == PREG_PATTERN_ORDER {
                            let sets = match_sets.as_mut().unwrap();
                            for i in 0..c {
                                if offset_capture {
                                    add_offset_pair(
                                        &mut sets[i],
                                        subject,
                                        offsets[2 * i],
                                        offsets[2 * i + 1],
                                        None,
                                        unmatched_as_null,
                                    );
                                } else {
                                    sets[i].push(match_value(
                                        subject,
                                        offsets[2 * i],
                                        offsets[2 * i + 1],
                                        unmatched_as_null,
                                    ));
                                }
                            }
                            if let Some(m) = &md.mark {
                                marks.get_or_insert_with(PhpArray::new).set(
                                    ArrayKey::Int(matched - 1),
                                    Value::String(PhpString::from_bytes(m)),
                                );
                            }
                            for set in sets.iter_mut().take(num_subpats).skip(c) {
                                if offset_capture {
                                    add_offset_pair(
                                        set,
                                        subject,
                                        PCRE2_UNSET,
                                        PCRE2_UNSET,
                                        None,
                                        unmatched_as_null,
                                    );
                                } else if unmatched_as_null {
                                    set.push(Value::Null);
                                } else {
                                    set.push(Value::String(PhpString::empty()));
                                }
                            }
                        } else {
                            let mut rs = PhpArray::new();
                            populate_subpat_array(
                                &mut rs,
                                subject,
                                offsets,
                                subpat_names,
                                num_subpats,
                                c,
                                md.mark.as_ref(),
                                flags,
                            );
                            result.push(arr_val(rs));
                        }
                    } else {
                        populate_subpat_array(
                            &mut result,
                            subject,
                            offsets,
                            subpat_names,
                            num_subpats,
                            c,
                            md.mark.as_ref(),
                            flags,
                        );
                        break 'outer;
                    }
                }

                start_offset2 = md.ovector[1];
                if start_offset2 == md.ovector[0] {
                    count = pcre_match(
                        pce,
                        subject,
                        start_offset2,
                        PCRE2_NO_UTF_CHECK | PCRE2_NOTEMPTY_ATSTART | PCRE2_ANCHORED,
                        &mut md,
                        &mctx,
                    );
                    if count >= 0 {
                        if global {
                            c = if count == 0 {
                                num_subpats
                            } else {
                                count as usize
                            };
                            continue;
                        } else {
                            break 'outer;
                        }
                    } else if count == PCRE2_ERROR_NOMATCH {
                        if start_offset2 < subject_len {
                            start_offset2 += unit_length(pce, subject, start_offset2);
                        } else {
                            break 'outer;
                        }
                    } else {
                        pcre_handle_exec_error(vm, count);
                        break 'outer;
                    }
                }
                break;
            }
        } else if count == PCRE2_ERROR_NOMATCH {
            break;
        } else {
            pcre_handle_exec_error(vm, count);
            break;
        }
        if !global {
            break;
        }
        count = pcre_match(
            pce,
            subject,
            start_offset2,
            PCRE2_NO_UTF_CHECK,
            &mut md,
            &mctx,
        );
    }

    if let Some(sets) = match_sets {
        if let Some(names) = subpat_names {
            for (i, set) in sets.into_iter().enumerate() {
                let wrapper = arr_val(set);
                if let Some(n) = &names[i] {
                    result.set(ArrayKey::String(n.clone()), deep_clone(&wrapper));
                }
                result.push(wrapper);
            }
        } else {
            for set in sets {
                result.push(arr_val(set));
            }
        }
        if let Some(m) = marks {
            result.set(ArrayKey::String(PhpString::from_bytes(b"MARK")), arr_val(m));
        }
    }
    if subpats.is_some() {
        set_ref(subpats, arr_val(result));
    }

    if vm.preg_last_error == PHP_PCRE_NO_ERROR {
        if (pce.compile_options & PCRE2_UTF) != 0 && orig_start_offset == 0 {
            subject_str.mark_valid_utf8();
        }
        Ok(Value::Long(matched))
    } else {
        Ok(Value::False)
    }
}

pub fn preg_match(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_do_pcre_match(vm, args, false)
}

pub fn preg_match_all(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    php_do_pcre_match(vm, args, true)
}

/// preg_get_backref(): parses \n, $n or ${n} at `walk`; returns the new
/// position and the backreference number.
fn preg_get_backref(r: &[u8], walk: usize) -> Option<(usize, usize)> {
    let at = |i: usize| -> u8 { *r.get(i).unwrap_or(&0) };
    let mut w = walk;
    let mut in_brace = false;
    if at(w + 1) == 0 {
        return None;
    }
    if at(w) == b'$' && at(w + 1) == b'{' {
        in_brace = true;
        w += 1;
    }
    w += 1;
    let mut backref;
    if at(w).is_ascii_digit() {
        backref = (at(w) - b'0') as usize;
        w += 1;
    } else {
        return None;
    }
    if at(w) != 0 && at(w).is_ascii_digit() {
        backref = backref * 10 + (at(w) - b'0') as usize;
        w += 1;
    }
    if in_brace {
        if at(w) != b'}' {
            return None;
        }
        w += 1;
    }
    Some((w, backref))
}

/// Expand a replacement string for one match (the copy loop of
/// php_pcre_replace_impl()).
fn expand_replacement(
    out: &mut Vec<u8>,
    repl: &[u8],
    subject: &[u8],
    offsets: &[usize],
    count: usize,
) {
    let mut walk = 0usize;
    let mut walk_last: u8 = 0;
    while walk < repl.len() {
        let ch = repl[walk];
        if ch == b'\\' || ch == b'$' {
            if walk_last == b'\\' {
                // *(walkbuf-1) = *walk++
                if let Some(last) = out.last_mut() {
                    *last = ch;
                }
                walk += 1;
                walk_last = 0;
                continue;
            }
            if let Some((nw, backref)) = preg_get_backref(repl, walk) {
                if backref < count && offsets[backref << 1] != PCRE2_UNSET {
                    let s = offsets[backref << 1];
                    let e = offsets[(backref << 1) + 1];
                    out.extend_from_slice(&subject[s..e]);
                }
                walk = nw;
                continue;
            }
        }
        out.push(ch);
        walk += 1;
        walk_last = ch;
    }
}

/// php_pcre_replace_impl(): Ok(None) on error.
fn php_pcre_replace_impl(
    vm: &mut Vm,
    pce: &Pce,
    subject: &[u8],
    replace: &[u8],
    mut limit: usize,
    replace_count: &mut usize,
) -> Option<Vec<u8>> {
    let num_subpats = pce.capture_count as usize + 1;
    let subject_len = subject.len();
    let mut result: Option<Vec<u8>> = None;
    let mut start_offset: usize = 0;
    let mut last_end_offset: usize = 0;
    vm.preg_last_error = PHP_PCRE_NO_ERROR;
    let mctx = match_context(vm);
    let mut md = MatchData::new(num_subpats);
    let options = if (pce.compile_options & PCRE2_UTF) != 0 {
        0
    } else {
        PCRE2_NO_UTF_CHECK
    };
    let mut count = pcre_match(pce, subject, start_offset, options, &mut md, &mctx);

    loop {
        let mut piece = last_end_offset;
        if count >= 0 && limit > 0 {
            let mut c = if count == 0 {
                vm.emit_notice_at(
                    "preg_replace(): Matched, but too many substrings",
                    vm.current_line,
                );
                num_subpats
            } else {
                count as usize
            };
            // matched:
            loop {
                if md.ovector[1] < md.ovector[0] {
                    vm.preg_last_error = PHP_PCRE_INTERNAL_ERROR;
                    return None;
                }
                *replace_count += 1;
                let res = result.get_or_insert_with(Vec::new);
                let m0 = md.ovector[0];
                if m0 > piece {
                    res.extend_from_slice(&subject[piece..m0]);
                }
                expand_replacement(res, replace, subject, &md.ovector, c);
                limit -= 1;
                start_offset = md.ovector[1];
                last_end_offset = start_offset;
                if start_offset == md.ovector[0] {
                    count = pcre_match(
                        pce,
                        subject,
                        start_offset,
                        PCRE2_NO_UTF_CHECK | PCRE2_NOTEMPTY_ATSTART | PCRE2_ANCHORED,
                        &mut md,
                        &mctx,
                    );
                    piece = start_offset;
                    if count >= 0 && limit > 0 {
                        c = if count == 0 {
                            num_subpats
                        } else {
                            count as usize
                        };
                        continue;
                    } else if count == PCRE2_ERROR_NOMATCH || limit == 0 {
                        if start_offset < subject_len {
                            start_offset += unit_length(pce, subject, start_offset);
                        } else {
                            // not_matched
                            let res = result.get_or_insert_with(Vec::new);
                            res.extend_from_slice(&subject[piece..]);
                            return result;
                        }
                    } else {
                        pcre_handle_exec_error(vm, count);
                        return None;
                    }
                }
                break;
            }
        } else if count == PCRE2_ERROR_NOMATCH || limit == 0 {
            // not_matched
            match result {
                None => return Some(subject.to_vec()),
                Some(mut res) => {
                    res.extend_from_slice(&subject[piece..]);
                    return Some(res);
                }
            }
        } else {
            pcre_handle_exec_error(vm, count);
            return None;
        }
        count = pcre_match(
            pce,
            subject,
            start_offset,
            PCRE2_NO_UTF_CHECK,
            &mut md,
            &mctx,
        );
    }
}

/// php_pcre_replace(): compile + replace.
fn php_pcre_replace(
    vm: &mut Vm,
    regex: &[u8],
    subject: &[u8],
    replace: &[u8],
    limit: usize,
    replace_count: &mut usize,
    fname: &str,
) -> Option<Vec<u8>> {
    let pce = get_compiled_regex(vm, regex, fname)?;
    php_pcre_replace_impl(vm, &pce, subject, replace, limit, replace_count)
}

/// Pattern / replacement / subject arguments: string or array.
enum StrOrArr {
    Str(PhpString),
    Arr(Rc<RefCell<PhpArray>>),
}

/// Z_PARAM_ARRAY_HT_OR_STR: arrays as is, objects need __toString().
fn zpp_str_or_arr(
    vm: &mut Vm,
    args: &[Value],
    i: usize,
    fname: &str,
    name: &str,
) -> Result<StrOrArr, VmError> {
    match args.get(i).map(|v| v.deref()) {
        Some(Value::Array(a)) => Ok(StrOrArr::Arr(a)),
        Some(Value::Object(o)) => {
            let cl = o.borrow().class_name.to_ascii_lowercase();
            let has = vm
                .classes
                .get(&cl)
                .map(|c| c.get_method(b"__tostring").is_some())
                .unwrap_or(false);
            if !has {
                let tn = goro_core::value::display_class_name(&o.borrow().class_name);
                return Err(throw(
                    vm,
                    b"TypeError",
                    format!(
                        "{}(): Argument #{} (${}) must be of type array|string, {} given",
                        fname,
                        i + 1,
                        name,
                        tn
                    ),
                ));
            }
            Ok(StrOrArr::Str(
                vm.value_to_string_checked(&Value::Object(o))?,
            ))
        }
        Some(v) => Ok(StrOrArr::Str(v.to_php_string())),
        None => Ok(StrOrArr::Str(PhpString::empty())),
    }
}

/// zval_get_tmp_string() for array elements: arrays become "Array" with a
/// warning.
fn elem_to_string(vm: &mut Vm, v: &Value) -> Result<PhpString, VmError> {
    match v.deref() {
        Value::Array(_) => {
            vm.emit_warning("Array to string conversion");
            Ok(PhpString::from_bytes(b"Array"))
        }
        other => vm.value_to_string_checked(&other),
    }
}

/// php_replace_in_subject()
fn php_replace_in_subject(
    vm: &mut Vm,
    regex: &StrOrArr,
    replace: &StrOrArr,
    subject: &[u8],
    limit: usize,
    replace_count: &mut usize,
    fname: &str,
) -> Result<Option<Vec<u8>>, VmError> {
    match regex {
        StrOrArr::Str(r) => {
            let rs = match replace {
                StrOrArr::Str(s) => s.clone(),
                StrOrArr::Arr(_) => PhpString::empty(),
            };
            Ok(php_pcre_replace(
                vm,
                r.as_bytes(),
                subject,
                rs.as_bytes(),
                limit,
                replace_count,
                fname,
            ))
        }
        StrOrArr::Arr(regex_ht) => {
            // php_pcre_replace_array()
            let regexes: Vec<Value> = regex_ht.borrow().values().cloned().collect();
            let mut subject_str = subject.to_vec();
            match replace {
                StrOrArr::Arr(replace_ht) => {
                    let replaces: Vec<Value> = replace_ht.borrow().values().cloned().collect();
                    let mut replace_idx = 0usize;
                    for regex_entry in regexes {
                        let regex_str = elem_to_string(vm, &regex_entry)?;
                        let replace_entry = if replace_idx < replaces.len() {
                            let v = replaces[replace_idx].clone();
                            replace_idx += 1;
                            elem_to_string(vm, &v)?
                        } else {
                            PhpString::empty()
                        };
                        match php_pcre_replace(
                            vm,
                            regex_str.as_bytes(),
                            &subject_str,
                            replace_entry.as_bytes(),
                            limit,
                            replace_count,
                            fname,
                        ) {
                            Some(r) => subject_str = r,
                            None => return Ok(None),
                        }
                    }
                }
                StrOrArr::Str(rs) => {
                    for regex_entry in regexes {
                        let regex_str = elem_to_string(vm, &regex_entry)?;
                        match php_pcre_replace(
                            vm,
                            regex_str.as_bytes(),
                            &subject_str,
                            rs.as_bytes(),
                            limit,
                            replace_count,
                            fname,
                        ) {
                            Some(r) => subject_str = r,
                            None => return Ok(None),
                        }
                    }
                }
            }
            Ok(Some(subject_str))
        }
    }
}

fn limit_arg(args: &[Value], i: usize) -> usize {
    // limit is zend_long; -1 means unlimited (cast to size_t).
    arg_long(args, i, -1) as u64 as usize
}

/// preg_replace_common()
fn preg_replace_common(vm: &mut Vm, args: &[Value], is_filter: bool) -> Result<Value, VmError> {
    let fname = if is_filter {
        "preg_filter"
    } else {
        "preg_replace"
    };
    let regex = zpp_str_or_arr(vm, args, 0, fname, "pattern")?;
    let replace = zpp_str_or_arr(vm, args, 1, fname, "replacement")?;
    let subject = zpp_str_or_arr(vm, args, 2, fname, "subject")?;
    let limit = limit_arg(args, 3);
    let zcount = args.get(4);

    if matches!(replace, StrOrArr::Arr(_)) && matches!(regex, StrOrArr::Str(_)) {
        return Err(throw(
            vm,
            b"TypeError",
            format!(
                "{}(): Argument #1 ($pattern) must be of type array when argument #2 ($replacement) is an array, string given",
                fname
            ),
        ));
    }

    let mut replace_count: usize = 0;
    let ret = match &subject {
        StrOrArr::Str(s) => {
            let old = replace_count;
            match php_replace_in_subject(
                vm,
                &regex,
                &replace,
                s.as_bytes(),
                limit,
                &mut replace_count,
                fname,
            )? {
                Some(r) => {
                    if !is_filter || replace_count > old {
                        Value::String(PhpString::from_vec(r))
                    } else {
                        Value::Null
                    }
                }
                None => Value::Null,
            }
        }
        StrOrArr::Arr(ht) => {
            let entries: Vec<(ArrayKey, Value)> = ht
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let mut out = PhpArray::with_capacity(entries.len());
            for (k, v) in entries {
                let old = replace_count;
                let s = elem_to_string(vm, &v)?;
                if let Some(r) = php_replace_in_subject(
                    vm,
                    &regex,
                    &replace,
                    s.as_bytes(),
                    limit,
                    &mut replace_count,
                    fname,
                )? {
                    if !is_filter || replace_count > old {
                        out.set(k, Value::String(PhpString::from_vec(r)));
                    }
                }
            }
            arr_val(out)
        }
    };
    set_ref(zcount, Value::Long(replace_count as i64));
    Ok(ret)
}

pub fn preg_replace(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    preg_replace_common(vm, args, false)
}

pub fn preg_filter(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    preg_replace_common(vm, args, true)
}

/// preg_do_repl_func(): call the callback with the match array. Err for an
/// exception; Ok(None) when the result cannot be converted to a string.
#[allow(clippy::too_many_arguments)]
fn preg_do_repl_func(
    vm: &mut Vm,
    callback: &Value,
    subject: &[u8],
    offsets: &[usize],
    subpat_names: Option<&Vec<Option<PhpString>>>,
    num_subpats: usize,
    count: usize,
    mark: Option<&Vec<u8>>,
    flags: i64,
) -> Result<Option<PhpString>, VmError> {
    // zend_call_function() does not run while an exception is pending.
    if vm.current_exception.is_some() {
        return Ok(None);
    }
    let mut arg = PhpArray::new();
    populate_subpat_array(
        &mut arg,
        subject,
        offsets,
        subpat_names,
        num_subpats,
        count,
        mark,
        flags,
    );
    let retval = if callback.is_closure_obj() {
        vm.call_callback(callback, &[arr_val(arg)])?
    } else {
        crate::misc::call_user_func(vm, &[callback.clone(), arr_val(arg)])?
    };
    if vm.current_exception.is_some() {
        return Ok(None);
    }
    match retval.deref() {
        Value::String(s) => Ok(Some(s)),
        Value::Undef => Ok(None),
        Value::Array(_) => {
            vm.emit_warning("Array to string conversion");
            Ok(Some(PhpString::from_bytes(b"Array")))
        }
        other => match vm.value_to_string_checked(&other) {
            Ok(s) => Ok(Some(s)),
            Err(e) => Err(e),
        },
    }
}

/// php_pcre_replace_func_impl()
#[allow(clippy::too_many_arguments)]
fn php_pcre_replace_func_impl(
    vm: &mut Vm,
    pce: &Pce,
    subject: &[u8],
    callback: &Value,
    mut limit: usize,
    replace_count: &mut usize,
    flags: i64,
) -> Result<Option<Vec<u8>>, VmError> {
    let num_subpats = pce.capture_count as usize + 1;
    let subject_len = subject.len();
    let subpat_names = pce.subpat_names.as_ref();
    let mut result: Option<Vec<u8>> = None;
    let mut start_offset: usize = 0;
    let mut last_end_offset: usize = 0;
    vm.preg_last_error = PHP_PCRE_NO_ERROR;
    let mctx = match_context(vm);
    let mut md = MatchData::new(num_subpats);
    let options = if (pce.compile_options & PCRE2_UTF) != 0 {
        0
    } else {
        PCRE2_NO_UTF_CHECK
    };
    let mut count = pcre_match(pce, subject, start_offset, options, &mut md, &mctx);

    loop {
        let mut piece = last_end_offset;
        if count >= 0 && limit > 0 {
            let mut c = if count == 0 {
                num_subpats
            } else {
                count as usize
            };
            loop {
                if md.ovector[1] < md.ovector[0] {
                    vm.preg_last_error = PHP_PCRE_INTERNAL_ERROR;
                    return Ok(None);
                }
                *replace_count += 1;
                let m0 = md.ovector[0];
                let ovec = md.ovector.clone();
                let mark = md.mark.clone();
                let eval = match preg_do_repl_func(
                    vm,
                    callback,
                    subject,
                    &ovec,
                    subpat_names,
                    num_subpats,
                    c,
                    mark.as_ref(),
                    flags,
                ) {
                    Ok(Some(s)) => s,
                    Ok(None) => {
                        pcre_handle_exec_error(vm, count);
                        return Ok(None);
                    }
                    Err(e) => {
                        pcre_handle_exec_error(vm, count);
                        return Err(e);
                    }
                };
                let res = result.get_or_insert_with(Vec::new);
                if m0 > piece {
                    res.extend_from_slice(&subject[piece..m0]);
                }
                res.extend_from_slice(eval.as_bytes());
                limit -= 1;
                start_offset = ovec[1];
                last_end_offset = start_offset;
                if start_offset == ovec[0] {
                    count = pcre_match(
                        pce,
                        subject,
                        start_offset,
                        PCRE2_NO_UTF_CHECK | PCRE2_NOTEMPTY_ATSTART | PCRE2_ANCHORED,
                        &mut md,
                        &mctx,
                    );
                    piece = start_offset;
                    if count >= 0 && limit > 0 {
                        c = if count == 0 {
                            num_subpats
                        } else {
                            count as usize
                        };
                        continue;
                    } else if count == PCRE2_ERROR_NOMATCH || limit == 0 {
                        if start_offset < subject_len {
                            start_offset += unit_length(pce, subject, start_offset);
                        } else {
                            let res = result.get_or_insert_with(Vec::new);
                            res.extend_from_slice(&subject[piece..]);
                            return Ok(result);
                        }
                    } else {
                        pcre_handle_exec_error(vm, count);
                        return Ok(None);
                    }
                }
                break;
            }
        } else if count == PCRE2_ERROR_NOMATCH || limit == 0 {
            match result {
                None => return Ok(Some(subject.to_vec())),
                Some(mut res) => {
                    res.extend_from_slice(&subject[piece..]);
                    return Ok(Some(res));
                }
            }
        } else {
            pcre_handle_exec_error(vm, count);
            return Ok(None);
        }
        count = pcre_match(
            pce,
            subject,
            start_offset,
            PCRE2_NO_UTF_CHECK,
            &mut md,
            &mctx,
        );
    }
}

/// php_replace_in_subject_func()
#[allow(clippy::too_many_arguments)]
fn php_replace_in_subject_func(
    vm: &mut Vm,
    regex: &StrOrArr,
    callback: &Value,
    subject: &[u8],
    limit: usize,
    replace_count: &mut usize,
    flags: i64,
    fname: &str,
) -> Result<Option<Vec<u8>>, VmError> {
    match regex {
        StrOrArr::Str(r) => {
            let pce = match get_compiled_regex(vm, r.as_bytes(), fname) {
                Some(p) => p,
                None => return Ok(None),
            };
            php_pcre_replace_func_impl(vm, &pce, subject, callback, limit, replace_count, flags)
        }
        StrOrArr::Arr(ht) => {
            let regexes: Vec<Value> = ht.borrow().values().cloned().collect();
            let mut subj = subject.to_vec();
            for regex_entry in regexes {
                let rs = elem_to_string(vm, &regex_entry)?;
                let pce = match get_compiled_regex(vm, rs.as_bytes(), fname) {
                    Some(p) => p,
                    None => return Ok(None),
                };
                match php_pcre_replace_func_impl(
                    vm,
                    &pce,
                    &subj,
                    callback,
                    limit,
                    replace_count,
                    flags,
                )? {
                    Some(r) => subj = r,
                    None => return Ok(None),
                }
            }
            Ok(Some(subj))
        }
    }
}

/// php_preg_replace_func_impl()
#[allow(clippy::too_many_arguments)]
fn php_preg_replace_func_impl(
    vm: &mut Vm,
    regex: &StrOrArr,
    callback: &Value,
    subject: &StrOrArr,
    limit: usize,
    flags: i64,
    fname: &str,
) -> Result<(Value, usize), VmError> {
    let mut replace_count: usize = 0;
    match subject {
        StrOrArr::Str(s) => {
            let r = php_replace_in_subject_func(
                vm,
                regex,
                callback,
                s.as_bytes(),
                limit,
                &mut replace_count,
                flags,
                fname,
            )?;
            Ok((
                r.map(|r| Value::String(PhpString::from_vec(r)))
                    .unwrap_or(Value::Null),
                replace_count,
            ))
        }
        StrOrArr::Arr(ht) => {
            let entries: Vec<(ArrayKey, Value)> = ht
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let mut out = PhpArray::with_capacity(entries.len());
            let mut first_err: Option<VmError> = None;
            for (k, v) in entries {
                let s = match elem_to_string(vm, &v) {
                    Ok(s) => s,
                    Err(e) => {
                        first_err.get_or_insert(e);
                        break;
                    }
                };
                // (with an exception pending, the remaining subjects are still
                // matched: the callback is not called, a match fails)
                match php_replace_in_subject_func(
                    vm,
                    regex,
                    callback,
                    s.as_bytes(),
                    limit,
                    &mut replace_count,
                    flags,
                    fname,
                ) {
                    Ok(Some(r)) => out.set(k, Value::String(PhpString::from_vec(r))),
                    Ok(None) => {}
                    Err(e) => {
                        first_err.get_or_insert(e);
                    }
                }
            }
            if let Some(e) = first_err {
                return Err(e);
            }
            Ok((arr_val(out), replace_count))
        }
    }
}

pub fn preg_replace_callback(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let regex = zpp_str_or_arr(vm, args, 0, "preg_replace_callback", "pattern")?;
    let callback = args.get(1).map(|v| v.deref()).unwrap_or(Value::Null);
    let limit = limit_arg(args, 3);
    let zcount = args.get(4);
    let flags = arg_long(args, 5, 0);
    if let Some(reason) = crate::misc::callable_error(vm, &callback) {
        return Err(throw(
            vm,
            b"TypeError",
            format!(
                "preg_replace_callback(): Argument #2 ($callback) must be a valid callback, {}",
                reason
            ),
        ));
    }
    let subject = zpp_str_or_arr(vm, args, 2, "preg_replace_callback", "subject")?;
    let (ret, count) = php_preg_replace_func_impl(
        vm,
        &regex,
        &callback,
        &subject,
        limit,
        flags,
        "preg_replace_callback",
    )?;
    set_ref(zcount, Value::Long(count as i64));
    Ok(ret)
}

pub fn preg_replace_callback_array(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let pattern = match args.first().map(|v| v.deref()) {
        Some(Value::Array(a)) => a,
        _ => return Ok(Value::Null),
    };
    let mut subject = zpp_str_or_arr(vm, args, 1, "preg_replace_callback_array", "subject")?;
    let limit = limit_arg(args, 2);
    let zcount = args.get(3);
    let flags = arg_long(args, 4, 0);
    let mut replace_count: usize = 0;
    let entries: Vec<(ArrayKey, Value)> = pattern
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, replace) in entries {
        let regex = match k {
            ArrayKey::String(s) => s,
            ArrayKey::Int(_) => {
                return Err(throw(
                    vm,
                    b"TypeError",
                    "preg_replace_callback_array(): Argument #1 ($pattern) must contain only string patterns as keys"
                        .to_string(),
                ));
            }
        };
        let replace = replace.deref();
        if crate::misc::callable_error(vm, &replace).is_some() {
            return Err(throw(
                vm,
                b"TypeError",
                "preg_replace_callback_array(): Argument #1 ($pattern) must contain only valid callbacks".to_string(),
            ));
        }
        let (retval, cnt) = php_preg_replace_func_impl(
            vm,
            &StrOrArr::Str(regex),
            &replace,
            &subject,
            limit,
            flags,
            "preg_replace_callback_array",
        )?;
        replace_count += cnt;
        match retval {
            Value::Array(a) => subject = StrOrArr::Arr(a),
            Value::String(s) => subject = StrOrArr::Str(s),
            _ => return Ok(Value::Null),
        }
    }
    set_ref(zcount, Value::Long(replace_count as i64));
    Ok(match subject {
        StrOrArr::Arr(a) => Value::Array(a),
        StrOrArr::Str(s) => Value::String(s),
    })
}

pub fn preg_split(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let regex = arg_str(args, 0);
    let subject = arg_str(args, 1);
    let limit_val = arg_long(args, 2, -1);
    let flags = arg_long(args, 3, 0);
    let pce = match get_compiled_regex(vm, regex.as_bytes(), "preg_split") {
        Some(p) => p,
        None => return Ok(Value::False),
    };
    php_pcre_split_impl(vm, &pce, subject.as_bytes(), limit_val, flags)
}

/// php_pcre_split_impl()
fn php_pcre_split_impl(
    vm: &mut Vm,
    pce: &Pce,
    subject: &[u8],
    mut limit_val: i64,
    flags: i64,
) -> Result<Value, VmError> {
    let no_empty = (flags & PREG_SPLIT_NO_EMPTY) != 0;
    let delim_capture = (flags & PREG_SPLIT_DELIM_CAPTURE) != 0;
    let offset_capture = (flags & PREG_SPLIT_OFFSET_CAPTURE) != 0;
    let subject_len = subject.len();
    let mut ret = PhpArray::new();
    let num_subpats = pce.capture_count as usize + 1;
    let mut start_offset: usize = 0;
    let mut last_match_offset: usize = 0;
    vm.preg_last_error = PHP_PCRE_NO_ERROR;

    let add_piece = |ret: &mut PhpArray, s: usize, e: usize| {
        if offset_capture {
            add_offset_pair(ret, subject, s, e, None, false);
        } else {
            ret.push(str_val(subject, s, e));
        }
    };

    'last: {
        if limit_val == -1 {
            // no limit
        } else if limit_val == 0 {
            limit_val = -1;
        } else if limit_val <= 1 {
            break 'last;
        }

        let mctx = match_context(vm);
        let mut md = MatchData::new(num_subpats);
        let options = if (pce.compile_options & PCRE2_UTF) != 0 {
            0
        } else {
            PCRE2_NO_UTF_CHECK
        };
        let mut count = pcre_match(pce, subject, start_offset, options, &mut md, &mctx);
        'outer: loop {
            if count >= 0 {
                let mut c = if count == 0 {
                    num_subpats
                } else {
                    count as usize
                };
                loop {
                    let o0 = md.ovector[0];
                    let o1 = md.ovector[1];
                    if o1 < o0 {
                        vm.preg_last_error = PHP_PCRE_INTERNAL_ERROR;
                        break 'outer;
                    }
                    if !no_empty || o0 != last_match_offset {
                        add_piece(&mut ret, last_match_offset, o0);
                        if limit_val != -1 {
                            limit_val -= 1;
                        }
                    }
                    if delim_capture {
                        for i in 1..c {
                            let s = md.ovector[2 * i];
                            let e = md.ovector[2 * i + 1];
                            if !no_empty || s != e {
                                if offset_capture {
                                    add_offset_pair(&mut ret, subject, s, e, None, false);
                                } else if s == PCRE2_UNSET {
                                    ret.push(Value::String(PhpString::empty()));
                                } else {
                                    ret.push(str_val(subject, s, e));
                                }
                            }
                        }
                    }
                    start_offset = o1;
                    last_match_offset = o1;
                    if start_offset == o0 {
                        if limit_val != -1 && limit_val <= 1 {
                            break 'outer;
                        }
                        count = pcre_match(
                            pce,
                            subject,
                            start_offset,
                            PCRE2_NO_UTF_CHECK | PCRE2_NOTEMPTY_ATSTART | PCRE2_ANCHORED,
                            &mut md,
                            &mctx,
                        );
                        if count >= 0 {
                            c = if count == 0 {
                                num_subpats
                            } else {
                                count as usize
                            };
                            continue;
                        } else if count == PCRE2_ERROR_NOMATCH {
                            if start_offset < subject_len {
                                start_offset += unit_length(pce, subject, start_offset);
                            } else {
                                break 'outer;
                            }
                        } else {
                            pcre_handle_exec_error(vm, count);
                            break 'outer;
                        }
                    }
                    break;
                }
            } else if count == PCRE2_ERROR_NOMATCH {
                break;
            } else {
                pcre_handle_exec_error(vm, count);
                break;
            }
            if limit_val != -1 && limit_val <= 1 {
                break;
            }
            count = pcre_match(
                pce,
                subject,
                start_offset,
                PCRE2_NO_UTF_CHECK,
                &mut md,
                &mctx,
            );
        }
        if vm.preg_last_error != PHP_PCRE_NO_ERROR {
            return Ok(Value::False);
        }
    }

    // last:
    let start = last_match_offset;
    if !no_empty || start < subject_len {
        add_piece(&mut ret, start, subject_len);
    }
    Ok(arr_val(ret))
}

pub fn preg_quote(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = arg_str(args, 0);
    let delim_char: u8 = match args.get(1).map(|v| v.deref()) {
        None | Some(Value::Null) | Some(Value::Undef) => 0,
        Some(v) => *v.to_php_string().as_bytes().first().unwrap_or(&0),
    };
    let input = s.as_bytes();
    if input.is_empty() {
        return Ok(Value::String(PhpString::empty()));
    }
    let mut out = Vec::with_capacity(input.len() * 2);
    for &c in input {
        match c {
            b'.' | b'\\' | b'+' | b'*' | b'?' | b'[' | b'^' | b']' | b'$' | b'(' | b')' | b'{'
            | b'}' | b'=' | b'!' | b'>' | b'<' | b'|' | b':' | b'-' | b'#' => {
                out.push(b'\\');
                out.push(c);
            }
            0 => out.extend_from_slice(b"\\000"),
            _ => {
                if c == delim_char {
                    out.push(b'\\');
                }
                out.push(c);
            }
        }
    }
    Ok(Value::String(PhpString::from_vec(out)))
}

pub fn preg_grep(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let regex = arg_str(args, 0);
    let input = match args.get(1).map(|v| v.deref()) {
        Some(Value::Array(a)) => a,
        _ => return Ok(Value::False),
    };
    let flags = arg_long(args, 2, 0);
    let pce = match get_compiled_regex(vm, regex.as_bytes(), "preg_grep") {
        Some(p) => p,
        None => return Ok(Value::False),
    };
    let invert = (flags & PREG_GREP_INVERT) != 0;
    let num_subpats = pce.capture_count as usize + 1;
    let mut ret = PhpArray::new();
    vm.preg_last_error = PHP_PCRE_NO_ERROR;
    let mctx = match_context(vm);
    let mut md = MatchData::new(num_subpats);
    let options = if (pce.compile_options & PCRE2_UTF) != 0 {
        0
    } else {
        PCRE2_NO_UTF_CHECK
    };
    let entries: Vec<(ArrayKey, Value)> = input
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, entry) in entries {
        let s = elem_to_string(vm, &entry)?;
        let count = pcre_match(&pce, s.as_bytes(), 0, options, &mut md, &mctx);
        if count >= 0 {
            if !invert {
                ret.set(k, entry);
            }
        } else if count == PCRE2_ERROR_NOMATCH {
            if invert {
                ret.set(k, entry);
            }
        } else {
            pcre_handle_exec_error(vm, count);
            break;
        }
    }
    Ok(arr_val(ret))
}

pub fn preg_last_error(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(vm.preg_last_error))
}

pub fn preg_last_error_msg(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(
        php_pcre_get_error_msg(vm.preg_last_error).as_bytes(),
    )))
}

pub fn register(vm: &mut Vm) {
    vm.register_function(b"preg_match", preg_match);
    vm.register_function(b"preg_match_all", preg_match_all);
    vm.register_function(b"preg_replace", preg_replace);
    vm.register_function(b"preg_split", preg_split);
    vm.register_function(b"preg_quote", preg_quote);
    vm.register_function(b"preg_replace_callback", preg_replace_callback);
    vm.register_function(b"preg_replace_callback_array", preg_replace_callback_array);
    vm.register_function(b"preg_grep", preg_grep);
    vm.register_function(b"preg_filter", preg_filter);
    vm.register_function(b"preg_last_error", preg_last_error);
    vm.register_function(b"preg_last_error_msg", preg_last_error_msg);

    let c = &mut vm.constants;
    c.insert(
        b"PCRE_VERSION".to_vec(),
        Value::String(PhpString::from_bytes(goro_pcre::VERSION.as_bytes())),
    );
    c.insert(b"PCRE_VERSION_MAJOR".to_vec(), Value::Long(10));
    c.insert(b"PCRE_VERSION_MINOR".to_vec(), Value::Long(44));
    c.insert(b"PCRE_JIT_SUPPORT".to_vec(), Value::True);
    c.entry(b"pcre.jit".to_vec())
        .or_insert_with(|| Value::String(PhpString::from_bytes(b"1")));
    c.entry(b"pcre.backtrack_limit".to_vec())
        .or_insert_with(|| Value::String(PhpString::from_bytes(b"1000000")));
    c.entry(b"pcre.recursion_limit".to_vec())
        .or_insert_with(|| Value::String(PhpString::from_bytes(b"100000")));
}
