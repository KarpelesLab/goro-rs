//! Port of php-src ext/mbstring/php_mbregex.c on top of `crate::onig`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::mbfl::{self, Encoding};
use crate::mbstring::check_encoding;
use crate::onig::{self, Limits, RegEnc, Region, Regex, Syntax};
use crate::state;
use crate::zpp::*;

pub fn register(vm: &mut Vm) {
    vm.register_function(b"mb_regex_encoding", mb_regex_encoding);
    vm.register_function(b"mb_ereg", mb_ereg);
    vm.register_function(b"mb_eregi", mb_eregi);
    vm.register_function(b"mb_ereg_replace", mb_ereg_replace);
    vm.register_function(b"mb_eregi_replace", mb_eregi_replace);
    vm.register_function(b"mb_ereg_replace_callback", mb_ereg_replace_callback);
    vm.register_function(b"mb_split", mb_split);
    vm.register_function(b"mb_ereg_match", mb_ereg_match);
    vm.register_function(b"mb_ereg_search", mb_ereg_search);
    vm.register_function(b"mb_ereg_search_pos", mb_ereg_search_pos);
    vm.register_function(b"mb_ereg_search_regs", mb_ereg_search_regs);
    vm.register_function(b"mb_ereg_search_init", mb_ereg_search_init);
    vm.register_function(b"mb_ereg_search_getregs", mb_ereg_search_getregs);
    vm.register_function(b"mb_ereg_search_getpos", mb_ereg_search_getpos);
    vm.register_function(b"mb_ereg_search_setpos", mb_ereg_search_setpos);
    vm.register_function(b"mb_regex_set_options", mb_regex_set_options);
}

/// (names, onig encoding) as in php_mbregex.c's enc_name_map
static ENC_NAME_MAP: &[(&[&str], RegEnc)] = &[
    (&["EUC-JP", "EUCJP", "X-EUC-JP", "UJIS", "EUCJP", "EUCJP-WIN"], RegEnc::EucJp),
    (&["UTF-8", "UTF8"], RegEnc::Utf8),
    (&["UTF-16", "UTF-16BE"], RegEnc::Utf16Be),
    (&["UTF-16LE"], RegEnc::Utf16Le),
    (&["UCS-4", "UTF-32", "UTF-32BE"], RegEnc::Utf32Be),
    (&["UCS-4LE", "UTF-32LE"], RegEnc::Utf32Le),
    (&["SJIS", "CP932", "MS932", "SHIFT_JIS", "SJIS-WIN", "WINDOWS-31J"], RegEnc::Sjis),
    (&["BIG5", "BIG-5", "BIGFIVE", "CN-BIG5", "BIG-FIVE"], RegEnc::Big5),
    (&["EUC-CN", "EUCCN", "EUC_CN", "GB-2312", "GB2312"], RegEnc::EucCn),
    (&["EUC-TW", "EUCTW", "EUC_TW"], RegEnc::EucTw),
    (&["EUC-KR", "EUCKR", "EUC_KR"], RegEnc::EucKr),
    (&["KOI8", "KOI-8"], RegEnc::Koi8),
    (&["KOI8R", "KOI8-R", "KOI-8R"], RegEnc::Koi8R),
    (&["ISO-8859-1", "ISO8859-1"], RegEnc::Latin1),
    (&["ISO-8859-2", "ISO8859-2"], RegEnc::SingleByte),
    (&["ISO-8859-3", "ISO8859-3"], RegEnc::SingleByte),
    (&["ISO-8859-4", "ISO8859-4"], RegEnc::SingleByte),
    (&["ISO-8859-5", "ISO8859-5"], RegEnc::SingleByte),
    (&["ISO-8859-6", "ISO8859-6"], RegEnc::SingleByte),
    (&["ISO-8859-7", "ISO8859-7"], RegEnc::SingleByte),
    (&["ISO-8859-8", "ISO8859-8"], RegEnc::SingleByte),
    (&["ISO-8859-9", "ISO8859-9"], RegEnc::SingleByte),
    (&["ISO-8859-10", "ISO8859-10"], RegEnc::SingleByte),
    (&["ISO-8859-11", "ISO8859-11"], RegEnc::SingleByte),
    (&["ISO-8859-13", "ISO8859-13"], RegEnc::SingleByte),
    (&["ISO-8859-14", "ISO8859-14"], RegEnc::SingleByte),
    (&["ISO-8859-15", "ISO8859-15"], RegEnc::SingleByte),
    (&["ISO-8859-16", "ISO8859-16"], RegEnc::SingleByte),
    (&["ASCII", "US-ASCII", "US_ASCII", "ISO646"], RegEnc::Ascii),
];

/// Index into ENC_NAME_MAP (single-byte ISO encodings share RegEnc values)
fn name2mbctype(name: &[u8]) -> Option<usize> {
    if name.is_empty() {
        return None;
    }
    ENC_NAME_MAP.iter().position(|(names, _)| names.iter().any(|n| n.as_bytes().eq_ignore_ascii_case(name)))
}

struct MbRex {
    default_mbctype: usize,
    current_mbctype: usize,
    current_mbfl: Option<&'static Encoding>,
    cache: HashMap<Vec<u8>, (Rc<Regex>, u32, Syntax, usize)>,
    search_str: Option<Vec<u8>>,
    search_pos: usize,
    search_re: Option<Rc<Regex>>,
    search_regs: Option<Region>,
    default_options: u32,
    default_syntax: Syntax,
}

impl MbRex {
    fn new() -> MbRex {
        MbRex {
            default_mbctype: 1,
            current_mbctype: 1,
            current_mbfl: Some(&mbfl::basic::ENC_UTF8),
            cache: HashMap::new(),
            search_str: None,
            search_pos: 0,
            search_re: None,
            search_regs: None,
            default_options: onig::OPT_MULTILINE | onig::OPT_SINGLELINE,
            default_syntax: Syntax::Ruby,
        }
    }
}

thread_local! {
    static MBREX: RefCell<MbRex> = RefCell::new(MbRex::new());
}

fn with<R>(f: impl FnOnce(&mut MbRex) -> R) -> R {
    MBREX.with(|m| f(&mut m.borrow_mut()))
}

/// Called when mbstring.internal_encoding (or its fallbacks) is set from INI
pub fn on_internal_encoding_ini(name: &[u8]) {
    with(|m| {
        match name2mbctype(name) {
            Some(i) => m.default_mbctype = i,
            None => m.default_mbctype = 1,
        }
        if let Some(i) = name2mbctype(name) {
            m.current_mbctype = i;
            m.current_mbfl = mbfl::name2encoding(name);
        }
    });
}

fn mbctype_name(i: usize) -> &'static str {
    ENC_NAME_MAP[i].0[0]
}

fn regex_encoding() -> (RegEnc, usize, Option<&'static Encoding>) {
    with(|m| (ENC_NAME_MAP[m.current_mbctype].1, m.current_mbctype, m.current_mbfl))
}

fn limits(vm: &Vm) -> Limits {
    let get = |k: &[u8], d: i64| -> i64 {
        match vm.constants.get(k) {
            Some(Value::String(s)) if s.is_empty() => d,
            Some(v) => v.to_long(),
            None => d,
        }
    };
    let stack = get(b"mbstring.regex_stack_limit", 100000);
    let retry = get(b"mbstring.regex_retry_limit", 1000000);
    Limits {
        stack_limit: if stack > 0 && stack < u32::MAX as i64 { stack as usize } else { 0 },
        retry_limit: if retry > 0 && retry < u32::MAX as i64 { retry as usize } else { 0 },
    }
}

fn check_mbctype_encoding(s: &[u8]) -> bool {
    let (_, _, mbfl_enc) = regex_encoding();
    match mbfl_enc {
        Some(e) => check_encoding(s, e),
        None => true,
    }
}

/// C: `php_mbregex_compile_pattern`
fn compile_pattern(vm: &mut Vm, func: &str, pattern: &[u8], options: u32, syntax: Syntax) -> Option<Rc<Regex>> {
    let (enc, idx, _) = regex_encoding();
    if !check_mbctype_encoding(pattern) {
        vm.emit_warning(&format!("{}(): Pattern is not valid under {} encoding", func, mbctype_name(idx)));
        return None;
    }
    let cached = with(|m| m.cache.get(pattern).cloned());
    if let Some((r, o, sy, i)) = &cached {
        if *o == options && *sy == syntax && *i == idx {
            return Some(r.clone());
        }
    }
    match onig::compile(pattern, options, enc, syntax) {
        Ok(re) => {
            let re = Rc::new(re);
            with(|m| {
                if let Some((old, ..)) = &cached {
                    if m.search_re.as_ref().map(|s| Rc::ptr_eq(s, old)).unwrap_or(false) {
                        // see bug #72399
                        m.search_re = None;
                    }
                }
                m.cache.insert(pattern.to_vec(), (re.clone(), options, syntax, idx));
            });
            Some(re)
        }
        Err(e) => {
            vm.emit_warning(&format!("{}(): mbregex compile err: {}", func, e));
            None
        }
    }
}

/// C: `_php_mb_regex_init_options`
fn init_options(vm: &mut Vm, opts: &[u8]) -> Result<(u32, Syntax), VmError> {
    let mut o = 0u32;
    let mut syntax = Syntax::Ruby;
    for &c in opts {
        match c {
            b'i' => o |= onig::OPT_IGNORECASE,
            b'x' => o |= onig::OPT_EXTEND,
            b'm' => o |= onig::OPT_MULTILINE,
            b's' => o |= onig::OPT_SINGLELINE,
            b'p' => o |= onig::OPT_MULTILINE | onig::OPT_SINGLELINE,
            b'l' => o |= onig::OPT_FIND_LONGEST,
            b'n' => o |= onig::OPT_FIND_NOT_EMPTY,
            b'j' => syntax = Syntax::Java,
            b'u' => syntax = Syntax::GnuRegex,
            b'g' => syntax = Syntax::Grep,
            b'c' => syntax = Syntax::Emacs,
            b'r' => syntax = Syntax::Ruby,
            b'z' => syntax = Syntax::Perl,
            b'b' => syntax = Syntax::PosixBasic,
            b'd' => syntax = Syntax::PosixExtended,
            _ => return Err(value_error(vm, format!("Option \"{}\" is not supported", c as char))),
        }
    }
    Ok((o, syntax))
}

fn option_string(o: u32, syntax: Syntax) -> String {
    let mut s = String::new();
    if o & onig::OPT_IGNORECASE != 0 {
        s.push('i');
    }
    if o & onig::OPT_EXTEND != 0 {
        s.push('x');
    }
    if o & (onig::OPT_MULTILINE | onig::OPT_SINGLELINE) == (onig::OPT_MULTILINE | onig::OPT_SINGLELINE) {
        s.push('p');
    } else {
        if o & onig::OPT_MULTILINE != 0 {
            s.push('m');
        }
        if o & onig::OPT_SINGLELINE != 0 {
            s.push('s');
        }
    }
    if o & onig::OPT_FIND_LONGEST != 0 {
        s.push('l');
    }
    if o & onig::OPT_FIND_NOT_EMPTY != 0 {
        s.push('n');
    }
    s.push(match syntax {
        Syntax::Java => 'j',
        Syntax::GnuRegex => 'u',
        Syntax::Grep => 'g',
        Syntax::Emacs => 'c',
        Syntax::Ruby => 'r',
        Syntax::Perl => 'z',
        Syntax::PosixBasic => 'b',
        Syntax::PosixExtended => 'd',
    });
    s
}

fn new_array(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

/// Add named groups (C: mb_regex_groups_iter)
fn add_named_groups(arr: &mut PhpArray, re: &Regex, s: &[u8], region: &Region) {
    for idx in re.names_foreach_order() {
        let name = &re.names[idx].0;
        let gn = re.name_to_backref_number(name, Some(region)).unwrap_or(0);
        let key = ArrayKey::String(PhpString::from_vec(name.clone()));
        match (region.beg.get(gn).copied().flatten(), region.end.get(gn).copied().flatten()) {
            (Some(b), Some(e)) if b < e && e <= s.len() => arr.set(key, pstr(&s[b..e])),
            _ => arr.set(key, Value::False),
        }
    }
}

fn mb_regex_encoding(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    state::sync(vm);
    let name = str_or_null_arg(vm, "mb_regex_encoding", args, 0, "encoding")?;
    match name {
        None => {
            let (_, idx, _) = regex_encoding();
            Ok(pstr(mbctype_name(idx).as_bytes()))
        }
        Some(n) => match name2mbctype(&n) {
            None => Err(arg_value_error(
                vm,
                "mb_regex_encoding",
                1,
                "encoding",
                &format!("must be a valid encoding, \"{}\" given", String::from_utf8_lossy(&n)),
            )),
            Some(i) => {
                with(|m| {
                    m.current_mbctype = i;
                    m.current_mbfl = mbfl::name2encoding(&n);
                });
                Ok(Value::True)
            }
        },
    }
}

fn ereg_exec(vm: &mut Vm, args: &[Value], icase: bool) -> Result<Value, VmError> {
    let f = if icase { "mb_eregi" } else { "mb_ereg" };
    state::sync(vm);
    let pattern = str_arg(vm, f, args, 0, "pattern")?;
    let string = str_arg(vm, f, args, 1, "string")?;
    if pattern.is_empty() {
        return Err(arg_value_error(vm, f, 1, "pattern", "must not be empty"));
    }
    let out_ref = match args.get(2) {
        Some(Value::Reference(r)) => {
            *r.borrow_mut() = new_array(PhpArray::new());
            Some(r.clone())
        }
        _ => None,
    };
    if !check_mbctype_encoding(&string) {
        return Ok(Value::False);
    }
    let mut options = with(|m| m.default_options);
    if icase {
        options |= onig::OPT_IGNORECASE;
    }
    let syntax = with(|m| m.default_syntax);
    let re = match compile_pattern(vm, f, &pattern, options, syntax) {
        Some(r) => r,
        None => return Ok(Value::False),
    };
    let lim = limits(vm);
    let region = match re.search(&string, 0, &lim) {
        Ok(Some(r)) => r,
        _ => return Ok(Value::False),
    };
    if let Some(r) = out_ref {
        let mut arr = PhpArray::new();
        for i in 0..region.beg.len() {
            match (region.beg[i], region.end[i]) {
                (Some(b), Some(e)) if b < e && e <= string.len() => arr.set(ArrayKey::Int(i as i64), pstr(&string[b..e])),
                _ => arr.set(ArrayKey::Int(i as i64), Value::False),
            }
        }
        if !re.names.is_empty() {
            add_named_groups(&mut arr, &re, &string, &region);
        }
        *r.borrow_mut() = new_array(arr);
    }
    Ok(Value::True)
}

fn mb_ereg(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    ereg_exec(vm, args, false)
}
fn mb_eregi(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    ereg_exec(vm, args, true)
}

fn mbchar_bytes(s: &[u8], p: usize, enc: Option<&Encoding>) -> usize {
    if p >= s.len() {
        return 1;
    }
    match enc {
        Some(e) => {
            if let Some(t) = e.mblen_table {
                t[s[p] as usize] as usize
            } else if e.flag & mbfl::MBFL_ENCTYPE_WCS2 != 0 {
                2
            } else if e.flag & mbfl::MBFL_ENCTYPE_WCS4 != 0 {
                4
            } else {
                1
            }
        }
        None => 1,
    }
}

/// C: `mb_regex_substitute`
fn substitute(out: &mut Vec<u8>, subject: &[u8], replace: &[u8], re: &Regex, regs: &Region, enc: Option<&Encoding>) {
    let eos = replace.len();
    let mut p = 0usize;
    let num_regs = regs.beg.len();
    while p < eos {
        let clen = mbchar_bytes(replace, p, enc);
        if clen != 1 || replace[p] != b'\\' {
            let e = (p + clen).min(eos);
            out.extend_from_slice(&replace[p..e]);
            p = e;
            continue;
        }
        let sp = p;
        p += 1;
        let clen = mbchar_bytes(replace, p, enc);
        if clen != 1 || p == eos {
            out.extend_from_slice(&replace[sp..p]);
            continue;
        }
        let no: i64;
        match replace[p] {
            b'0' => {
                no = 0;
                p += 1;
            }
            b'1'..=b'9' => {
                if !re.noname_capture_active {
                    p += 1;
                    out.extend_from_slice(&replace[sp..p]);
                    continue;
                }
                no = (replace[p] - b'0') as i64;
                p += 1;
            }
            b'k' => {
                p += 1;
                let clen = mbchar_bytes(replace, p, enc);
                if clen != 1 || p == eos || (replace[p] != b'<' && replace[p] != b'\'') {
                    p = (p + clen).min(eos);
                    out.extend_from_slice(&replace[sp..p]);
                    continue;
                }
                let delim = if replace[p] == b'<' { b'>' } else { b'\'' };
                let name = p + 1;
                let mut name_end = name;
                let mut maybe_num = true;
                while name_end < eos {
                    let clen = mbchar_bytes(replace, name_end, enc);
                    if clen != 1 {
                        name_end += clen;
                        maybe_num = false;
                        continue;
                    }
                    if replace[name_end] == delim {
                        break;
                    }
                    if maybe_num && !replace[name_end].is_ascii_digit() {
                        maybe_num = false;
                    }
                    name_end += 1;
                }
                p = name_end + 1;
                if name_end - name < 1 || name_end >= eos {
                    out.extend_from_slice(&replace[sp..p.min(eos)]);
                    continue;
                }
                if maybe_num {
                    if !re.noname_capture_active {
                        out.extend_from_slice(&replace[sp..p]);
                        continue;
                    }
                    if name_end - name == 1 {
                        no = (replace[name] - b'0') as i64;
                    } else if replace[name] == b'0' {
                        // "01" is not a valid number: keep the escape sequence
                        out.extend_from_slice(&replace[sp..p]);
                        continue;
                    } else {
                        no = std::str::from_utf8(&replace[name..name_end]).ok().and_then(|t| t.parse::<i64>().ok()).unwrap_or(-1);
                    }
                } else {
                    no = re.name_to_backref_number(&replace[name..name_end], Some(regs)).map(|n| n as i64).unwrap_or(-1);
                }
            }
            _ => {
                out.extend_from_slice(&replace[sp..p]);
                continue;
            }
        }
        if no < 0 || no as usize >= num_regs {
            out.extend_from_slice(&replace[sp..p]);
            continue;
        }
        let no = no as usize;
        if let (Some(b), Some(e)) = (regs.beg[no], regs.end[no]) {
            if b < e && e <= subject.len() {
                out.extend_from_slice(&subject[b..e]);
            }
        }
    }
}

fn replace_exec(vm: &mut Vm, args: &[Value], base_options: u32, callable: bool) -> Result<Value, VmError> {
    let f = if callable {
        "mb_ereg_replace_callback"
    } else if base_options & onig::OPT_IGNORECASE != 0 {
        "mb_eregi_replace"
    } else {
        "mb_ereg_replace"
    };
    state::sync(vm);
    let pattern = str_arg(vm, f, args, 0, "pattern")?;
    let mut replace: Vec<u8> = Vec::new();
    let mut callback = Value::Null;
    if callable {
        callback = args.get(1).cloned().unwrap_or(Value::Null).deref();
    } else {
        replace = str_arg(vm, f, args, 1, "replacement")?;
    }
    let string = str_arg(vm, f, args, 2, "string")?;
    let option_str = str_or_null_arg(vm, f, args, 3, "options")?;
    let (_, _, enc) = regex_encoding();
    if !check_mbctype_encoding(&string) {
        return Ok(Value::Null);
    }
    let (options, syntax) = match option_str {
        Some(o) => {
            let (o2, s) = init_options(vm, &o)?;
            (base_options | o2, s)
        }
        None => (base_options | with(|m| m.default_options), with(|m| m.default_syntax)),
    };
    let re = match compile_pattern(vm, f, &pattern, options, syntax) {
        Some(r) => r,
        None => return Ok(Value::False),
    };
    let lim = limits(vm);
    let subj = re.prepare(&string);
    let mut out: Vec<u8> = Vec::with_capacity(string.len());
    let mut pos = 0usize;
    loop {
        let res = re.search_prepared(&subj, pos, &lim);
        let regs = match res {
            Err(e) => {
                vm.emit_warning(&format!("{}(): mbregex search failure in php_mbereg_replace_exec(): {}", f, e.message()));
                return Ok(Value::False);
            }
            Ok(None) => {
                if pos < string.len() {
                    out.extend_from_slice(&string[pos..]);
                }
                break;
            }
            Ok(Some(r)) => r,
        };
        let mbeg = regs.beg[0].unwrap_or(pos);
        let mend = regs.end[0].unwrap_or(pos);
        if mbeg >= pos {
            out.extend_from_slice(&string[pos..mbeg]);
        }
        if !callable {
            substitute(&mut out, &string, &replace, &re, &regs, enc);
        } else {
            let mut subpats = PhpArray::new();
            for i in 0..regs.beg.len() {
                match (regs.beg[i], regs.end[i]) {
                    (Some(b), Some(e)) if b <= e => subpats.push(pstr(&string[b..e])),
                    _ => subpats.push(pstr(b"")),
                }
            }
            if !re.names.is_empty() {
                add_named_groups(&mut subpats, &re, &string, &regs);
            }
            let r = vm.call_callback(&callback, &[new_array(subpats)])?;
            if vm.current_exception.is_some() {
                return Err(VmError { message: "exception in callback".into(), line: vm.current_line });
            }
            let s = vm.value_to_string(&r);
            out.extend_from_slice(s.as_bytes());
        }
        if pos < mend {
            pos = mend;
        } else {
            if pos < string.len() {
                // copy one byte (as C does)
                out.push(string[pos]);
            }
            pos += 1;
        }
        if pos > string.len() {
            break;
        }
    }
    Ok(pstring(out))
}

fn mb_ereg_replace(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    replace_exec(vm, args, 0, false)
}
fn mb_eregi_replace(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    replace_exec(vm, args, onig::OPT_IGNORECASE, false)
}
fn mb_ereg_replace_callback(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    replace_exec(vm, args, 0, true)
}

fn mb_split(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_split";
    state::sync(vm);
    let pattern = str_arg(vm, f, args, 0, "pattern")?;
    let string = str_arg(vm, f, args, 1, "string")?;
    let mut count: u64 = if present(args, 2) { long_arg(vm, f, args, 2, "limit")? as u64 } else { u64::MAX };
    count = count.saturating_sub(1);
    if !check_mbctype_encoding(&string) {
        return Ok(Value::False);
    }
    let (o, s) = with(|m| (m.default_options, m.default_syntax));
    let re = match compile_pattern(vm, f, &pattern, o, s) {
        Some(r) => r,
        None => return Ok(Value::False),
    };
    let lim = limits(vm);
    let subj = re.prepare(&string);
    let mut arr = PhpArray::new();
    let mut chunk_pos = 0usize;
    let mut pos = 0usize;
    let mut failed: Option<String> = None;
    while count != 0 && pos < string.len() {
        let regs = match re.search_prepared(&subj, pos, &lim) {
            Ok(Some(r)) => r,
            Ok(None) => break,
            Err(e) => {
                failed = Some(e.message().to_string());
                break;
            }
        };
        let beg = regs.beg[0].unwrap_or(0);
        let end = regs.end[0].unwrap_or(0);
        if pos < end {
            if beg < string.len() && beg >= chunk_pos {
                arr.push(pstr(&string[chunk_pos..beg]));
                count = count.wrapping_sub(1);
            } else {
                failed = Some("mismatch".to_string());
                break;
            }
            chunk_pos = end;
            pos = end;
        } else {
            pos += 1;
        }
    }
    if let Some(e) = failed {
        vm.emit_warning(&format!("mb_split(): mbregex search failure in mbsplit(): {}", e));
        return Ok(Value::False);
    }
    if string.len() > chunk_pos {
        arr.push(pstr(&string[chunk_pos..]));
    } else {
        arr.push(pstr(b""));
    }
    Ok(new_array(arr))
}

fn mb_ereg_match(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_ereg_match";
    state::sync(vm);
    let pattern = str_arg(vm, f, args, 0, "pattern")?;
    let string = str_arg(vm, f, args, 1, "string")?;
    let option_str = str_or_null_arg(vm, f, args, 2, "options")?;
    let (options, syntax) = match option_str {
        Some(o) => init_options(vm, &o)?,
        None => with(|m| (m.default_options, m.default_syntax)),
    };
    if !check_mbctype_encoding(&string) {
        return Ok(Value::False);
    }
    let re = match compile_pattern(vm, f, &pattern, options, syntax) {
        Some(r) => r,
        None => return Ok(Value::False),
    };
    let lim = limits(vm);
    match re.match_at(&string, 0, &lim) {
        Ok(Some(_)) => Ok(Value::True),
        _ => Ok(Value::False),
    }
}

fn search_exec(vm: &mut Vm, args: &[Value], mode: u8) -> Result<Value, VmError> {
    let f = match mode {
        1 => "mb_ereg_search_pos",
        2 => "mb_ereg_search_regs",
        _ => "mb_ereg_search",
    };
    state::sync(vm);
    let pattern = str_or_null_arg(vm, f, args, 0, "pattern")?;
    let opts = str_or_null_arg(vm, f, args, 1, "options")?;
    let (option, syntax) = match opts {
        Some(o) => init_options(vm, &o).unwrap_or((0, Syntax::Ruby)),
        None => with(|m| (m.default_options, m.default_syntax)),
    };
    with(|m| m.search_regs = None);
    if let Some(p) = pattern {
        match compile_pattern(vm, f, &p, option, syntax) {
            Some(r) => with(|m| m.search_re = Some(r)),
            None => return Ok(Value::False),
        }
    }
    let (pos, re, s) = with(|m| (m.search_pos, m.search_re.clone(), m.search_str.clone()));
    let re = match re {
        Some(r) => r,
        None => return Err(throw(vm, b"Error", "No pattern was provided".to_string())),
    };
    let s = match s {
        Some(s) => s,
        None => return Err(throw(vm, b"Error", "No string was provided".to_string())),
    };
    let lim = limits(vm);
    match re.search(&s, pos.min(s.len() + 1), &lim) {
        Ok(None) => {
            with(|m| m.search_pos = s.len());
            Ok(Value::False)
        }
        Err(e) => {
            vm.emit_warning(&format!("{}(): mbregex search failure in mbregex_search(): {}", f, e.message()));
            Ok(Value::False)
        }
        Ok(Some(regs)) => {
            let ret = match mode {
                1 => {
                    let b = regs.beg[0].unwrap_or(0);
                    let e = regs.end[0].unwrap_or(0);
                    let mut arr = PhpArray::new();
                    arr.push(Value::Long(b as i64));
                    arr.push(Value::Long(e as i64 - b as i64));
                    new_array(arr)
                }
                2 => regs_array(&re, &s, &regs),
                _ => Value::True,
            };
            let end = regs.end[0].unwrap_or(0);
            with(|m| {
                m.search_pos = if pos <= end { end } else { pos + 1 };
                m.search_regs = Some(regs);
            });
            Ok(ret)
        }
    }
}

fn regs_array(re: &Regex, s: &[u8], regs: &Region) -> Value {
    let mut arr = PhpArray::new();
    for i in 0..regs.beg.len() {
        match (regs.beg[i], regs.end[i]) {
            (Some(b), Some(e)) if b <= e && e <= s.len() => arr.set(ArrayKey::Int(i as i64), pstr(&s[b..e])),
            _ => arr.set(ArrayKey::Int(i as i64), Value::False),
        }
    }
    if !re.names.is_empty() {
        add_named_groups(&mut arr, re, s, regs);
    }
    new_array(arr)
}

fn mb_ereg_search(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    search_exec(vm, args, 0)
}
fn mb_ereg_search_pos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    search_exec(vm, args, 1)
}
fn mb_ereg_search_regs(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    search_exec(vm, args, 2)
}

fn mb_ereg_search_init(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_ereg_search_init";
    state::sync(vm);
    let s = str_arg(vm, f, args, 0, "string")?;
    let pattern = str_or_null_arg(vm, f, args, 1, "pattern")?;
    let opts = str_or_null_arg(vm, f, args, 2, "options")?;
    if let Some(p) = &pattern {
        if p.is_empty() {
            return Err(arg_value_error(vm, f, 2, "pattern", "must not be empty"));
        }
    }
    let (option, syntax) = match opts {
        Some(o) => init_options(vm, &o).unwrap_or((0, Syntax::Ruby)),
        None => with(|m| (m.default_options, m.default_syntax)),
    };
    if let Some(p) = pattern {
        match compile_pattern(vm, f, &p, option, syntax) {
            Some(r) => with(|m| m.search_re = Some(r)),
            None => return Ok(Value::False),
        }
    }
    let valid = check_mbctype_encoding(&s);
    let len = s.len();
    with(|m| {
        m.search_str = Some(s);
        m.search_pos = if valid { 0 } else { len };
        m.search_regs = None;
    });
    Ok(vbool(valid))
}

fn mb_ereg_search_getregs(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let (re, s, regs) = with(|m| (m.search_re.clone(), m.search_str.clone(), m.search_regs.clone()));
    match (re, s, regs) {
        (Some(re), Some(s), Some(regs)) => Ok(regs_array(&re, &s, &regs)),
        _ => Ok(Value::False),
    }
}

fn mb_ereg_search_getpos(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(with(|m| m.search_pos) as i64))
}

fn mb_ereg_search_setpos(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_ereg_search_setpos";
    let mut position = long_arg(vm, f, args, 0, "offset")?;
    let len = with(|m| m.search_str.as_ref().map(|s| s.len()));
    if position < 0 {
        if let Some(l) = len {
            position += l as i64;
        }
    }
    if position < 0 || len.map(|l| position as usize > l).unwrap_or(false) {
        return Err(arg_value_error(vm, f, 1, "offset", "is out of range"));
    }
    with(|m| m.search_pos = position as usize);
    Ok(Value::True)
}

fn mb_regex_set_options(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = str_or_null_arg(vm, "mb_regex_set_options", args, 0, "options")?;
    let (o, syn) = match s {
        Some(s) => {
            let (o, syn) = init_options(vm, &s)?;
            
            with(|m| {
                let prev = (m.default_options, m.default_syntax);
                m.default_options = o;
                m.default_syntax = syn;
                prev
            })
        }
        None => with(|m| (m.default_options, m.default_syntax)),
    };
    Ok(pstr(option_string(o, syn).as_bytes()))
}
