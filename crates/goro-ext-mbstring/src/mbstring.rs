//! Port of php-src ext/mbstring/mbstring.c (PHP functions on top of the
//! libmbfl port in `crate::mbfl`).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::mbfl::{self, EncNo, Encoding, MBFL_BAD_INPUT, MBFL_ENCTYPE_SBCS, MBFL_ENCTYPE_WCS2, MBFL_ENCTYPE_WCS4};
use crate::mbfl::{MBFL_OUTPUTFILTER_ILLEGAL_MODE_ENTITY, MBFL_OUTPUTFILTER_ILLEGAL_MODE_LONG, MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE};
use crate::state::{self, MbState};
use crate::zpp::*;

pub fn register(vm: &mut Vm) {
    state::register_hooks(vm);
    crate::arginfo::register(vm);
    vm.register_function(b"mb_language", mb_language);
    vm.register_function(b"mb_internal_encoding", mb_internal_encoding);
    vm.register_function(b"mb_http_input", mb_http_input);
    vm.register_function(b"mb_http_output", mb_http_output);
    vm.register_function(b"mb_detect_order", mb_detect_order);
    vm.register_function(b"mb_substitute_character", mb_substitute_character);
    vm.register_function(b"mb_preferred_mime_name", mb_preferred_mime_name);
    vm.register_function(b"mb_strlen", mb_strlen);
    vm.register_function(b"mb_convert_encoding", mb_convert_encoding);
    vm.register_function(b"mb_detect_encoding", mb_detect_encoding);
    vm.register_function(b"mb_list_encodings", mb_list_encodings);
    vm.register_function(b"mb_encoding_aliases", mb_encoding_aliases);
    vm.register_function(b"mb_check_encoding", mb_check_encoding);
    vm.register_function(b"mb_get_info", mb_get_info);
    crate::strfuncs::register(vm);
    crate::mime::register(vm);
    crate::entities::register(vm);
    crate::misc::register(vm);
    crate::gpc::register(vm);
    crate::mbregex::register(vm);
    crate::mail::register(vm);
}

/* ===== Helpers ===== */

/// Truncate at the first NUL byte (C string semantics)
pub fn cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(p) => &b[..p],
        None => b,
    }
}

pub fn is_utf8_family(no: EncNo) -> bool {
    matches!(no, EncNo::Utf8 | EncNo::Utf8Docomo | EncNo::Utf8KddiA | EncNo::Utf8KddiB | EncNo::Utf8Sb)
}

/// True for the pseudo-encodings Base64/UUencode/HTML-ENTITIES/QPrint
/// (C: `no_encoding <= mbfl_no_encoding_qprint`)
fn is_transfer_pseudo(no: EncNo) -> bool {
    matches!(no, EncNo::Pass | EncNo::Wchar | EncNo::Base64 | EncNo::Uuencode | EncNo::HtmlEnt | EncNo::Qprint)
}

/// C: `no_encoding <= mbfl_no_encoding_charset_min`
fn is_non_text_encoding(no: EncNo) -> bool {
    matches!(
        no,
        EncNo::Pass | EncNo::Wchar | EncNo::Base64 | EncNo::Uuencode | EncNo::HtmlEnt | EncNo::Qprint | EncNo::SevenBit | EncNo::EightBit
    )
}

fn emit_transfer_deprecation(vm: &mut Vm, func: &str, enc: &Encoding) {
    let msg = match enc.no {
        EncNo::Base64 => "Handling Base64 via mbstring is deprecated; use base64_encode/base64_decode instead",
        EncNo::Qprint => "Handling QPrint via mbstring is deprecated; use quoted_printable_encode/quoted_printable_decode instead",
        EncNo::HtmlEnt => "Handling HTML entities via mbstring is deprecated; use htmlspecialchars, htmlentities, or mb_encode_numericentity/mb_decode_numericentity instead",
        EncNo::Uuencode => "Handling Uuencode via mbstring is deprecated; use convert_uuencode/convert_uudecode instead",
        _ => return,
    };
    vm.emit_deprecated(&format!("{}(): {}", func, msg));
}

/// C: `php_mb_get_encoding`. `name` None => current internal encoding.
pub fn get_encoding(vm: &mut Vm, func: &str, name: Option<&[u8]>, arg_num: usize, pname: &str) -> Result<&'static Encoding, VmError> {
    state::sync(vm);
    let name = name.map(cstr);
    match name {
        None => Ok(state::current_internal_encoding()),
        Some(n) => {
            if let Some(e) = state::with(|s| match &s.last_used_encoding_name {
                Some(last) if last.eq_ignore_ascii_case(n) => s.last_used_encoding,
                _ => None,
            }) {
                return Ok(e);
            }
            match mbfl::name2encoding(n) {
            None => Err(arg_value_error(
                vm,
                func,
                arg_num,
                pname,
                &format!("must be a valid encoding, \"{}\" given", String::from_utf8_lossy(n)),
            )),
            Some(e) => {
                if is_transfer_pseudo(e.no) {
                    emit_transfer_deprecation(vm, func, e);
                }
                state::with(|s| {
                    s.last_used_encoding_name = Some(n.to_vec());
                    s.last_used_encoding = Some(e);
                });
                Ok(e)
            }
            }
        }
    }
}

/// Encoding argument at position `idx` (0-based), nullable.
pub fn encoding_arg(vm: &mut Vm, func: &str, args: &[Value], idx: usize, pname: &str) -> Result<&'static Encoding, VmError> {
    let name = str_or_null_arg(vm, func, args, idx, pname)?;
    get_encoding(vm, func, name.as_deref(), idx + 1, pname)
}

/// C: `php_mb_parse_encoding_list` in function context
pub fn parse_encoding_list_arg(vm: &mut Vm, func: &str, value: &[u8], arg_num: usize, pname: &str) -> Result<Vec<&'static Encoding>, VmError> {
    let res = state::with(|st| state::parse_encoding_list(st, value));
    match res {
        Ok(l) => Ok(l),
        Err(bad) => Err(arg_value_error(
            vm,
            func,
            arg_num,
            pname,
            &format!("contains invalid encoding \"{}\"", String::from_utf8_lossy(&bad)),
        )),
    }
}

/// C: `php_mb_parse_encoding_array`
pub fn parse_encoding_array(vm: &mut Vm, func: &str, arr: &Rc<RefCell<PhpArray>>, arg_num: usize, pname: &str) -> Result<Vec<&'static Encoding>, VmError> {
    let vals: Vec<Value> = arr.borrow().values().cloned().collect();
    let mut list = Vec::new();
    let mut included_auto = false;
    for v in vals {
        let s = {
            let v = v.deref();
            match &v {
                Value::Array(_) => {
                    vm.emit_warning("Array to string conversion");
                    b"Array".to_vec()
                }
                _ => vm.value_to_string(&v).as_bytes().to_vec(),
            }
        };
        let s = cstr(&s).to_vec();
        if s.eq_ignore_ascii_case(b"auto") {
            if !included_auto {
                included_auto = true;
                let defaults = state::with(|st| st.default_detect_order_list);
                for no in defaults {
                    if let Some(e) = mbfl::no2encoding(*no) {
                        list.push(e);
                    }
                }
            }
        } else {
            match mbfl::name2encoding(&s) {
                Some(e) => list.push(e),
                None => {
                    return Err(arg_value_error(
                        vm,
                        func,
                        arg_num,
                        pname,
                        &format!("contains invalid encoding \"{}\"", String::from_utf8_lossy(&s)),
                    ))
                }
            }
        }
    }
    Ok(list)
}

/// Decode a whole string to codepoints
#[inline]
pub fn to_wchars(enc: &Encoding, s: &[u8]) -> Vec<u32> {
    enc.decode(s)
}

/// Encode codepoints with the current substitution settings
pub fn from_wchars(enc: &Encoding, w: &[u32]) -> Vec<u8> {
    let (repl, mode) = state::illegal_settings();
    enc.encode(w, repl, mode).0
}

/// C: `mb_fast_convert` with the current substitution settings; adds to
/// the illegal_chars counter (C: `php_mb_convert_encoding_ex`).
pub fn convert_ex(input: &[u8], to: &Encoding, from: &Encoding) -> Vec<u8> {
    let (repl, mode) = state::illegal_settings();
    let (out, errors) = mbfl::fast_convert(input, from, to, repl, mode);
    state::add_illegal_chars(errors);
    out
}

/// Convert to UTF-8 using 0xFF as the error marker (for searching)
pub fn to_utf8_badmarker(input: &[u8], from: &Encoding) -> Vec<u8> {
    mbfl::fast_convert(input, from, &mbfl::basic::ENC_UTF8, 0, mbfl::MBFL_OUTPUTFILTER_ILLEGAL_MODE_BADUTF8).0
}

fn new_array(arr: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(arr)))
}

fn str_list_array(list: &[&'static Encoding]) -> Value {
    let mut arr = PhpArray::new();
    for e in list {
        arr.push(pstr(e.name.as_bytes()));
    }
    new_array(arr)
}

/* ===== mb_language / mb_internal_encoding / http in/out ===== */

fn mb_language(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    state::sync(vm);
    let name = str_or_null_arg(vm, "mb_language", args, 0, "language")?;
    match name {
        None => {
            let l = state::with(|s| s.language);
            Ok(pstr(state::no2language(l).name.as_bytes()))
        }
        Some(n) => match state::name2language(&n) {
            None => {
                state::with(|s| s.language = state::LangNo::Neutral);
                Err(arg_value_error(
                    vm,
                    "mb_language",
                    1,
                    "language",
                    &format!("must be a valid language, \"{}\" given", String::from_utf8_lossy(&n)),
                ))
            }
            Some(_) => {
                // zend_alter_ini_entry("mbstring.language")
                let v = pstr(&n);
                state::ini_set_hook(vm, b"mbstring.language", &v);
                vm.constants.insert(b"mbstring.language".to_vec(), v);
                Ok(Value::True)
            }
        },
    }
}

fn mb_internal_encoding(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    state::sync(vm);
    let name = str_or_null_arg(vm, "mb_internal_encoding", args, 0, "encoding")?;
    match name {
        None => Ok(pstr(state::current_internal_encoding().name.as_bytes())),
        Some(n) => match mbfl::name2encoding(&n) {
            None => Err(arg_value_error(
                vm,
                "mb_internal_encoding",
                1,
                "encoding",
                &format!("must be a valid encoding, \"{}\" given", String::from_utf8_lossy(&n)),
            )),
            Some(e) => {
                state::with(|s| {
                    s.current_internal_encoding = e;
                    s.internal_encoding_set = true;
                });
                Ok(Value::True)
            }
        },
    }
}

fn mb_http_input(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    state::sync(vm);
    let ty = str_or_null_arg(vm, "mb_http_input", args, 0, "type")?;
    let err = "must be one of \"G\", \"P\", \"C\", \"S\", \"I\", or \"L\"";
    let enc = match ty {
        None => state::with(|s| s.http_input_identify),
        Some(t) => {
            if t.len() != 1 {
                return Err(arg_value_error(vm, "mb_http_input", 1, "type", err));
            }
            match t[0] {
                b'G' | b'g' => state::with(|s| s.http_input_identify_get),
                b'P' | b'p' => state::with(|s| s.http_input_identify_post),
                b'C' | b'c' => state::with(|s| s.http_input_identify_cookie),
                b'S' | b's' => state::with(|s| s.http_input_identify_string),
                b'I' | b'i' => {
                    let l = state::with(|s| s.http_input_list.clone());
                    return Ok(str_list_array(&l));
                }
                b'L' | b'l' => {
                    let l = state::with(|s| s.http_input_list.clone());
                    if l.is_empty() {
                        return Ok(Value::False);
                    }
                    let names: Vec<&str> = l.iter().map(|e| e.name).collect();
                    return Ok(pstr(names.join(",").as_bytes()));
                }
                _ => return Err(arg_value_error(vm, "mb_http_input", 1, "type", err)),
            }
        }
    };
    match enc {
        Some(e) => Ok(pstr(e.name.as_bytes())),
        None => Ok(Value::False),
    }
}

fn mb_http_output(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    state::sync(vm);
    let name = str_or_null_arg(vm, "mb_http_output", args, 0, "encoding")?;
    match name {
        None => Ok(pstr(state::with(|s| s.current_http_output_encoding.name).as_bytes())),
        Some(n) => {
            if n.contains(&0) {
                return Err(type_error(
                    vm,
                    "mb_http_output(): Argument #1 ($encoding) must not contain any null bytes".to_string(),
                ));
            }
            match state::get_encoding_or_pass(&n) {
                None => Err(arg_value_error(
                    vm,
                    "mb_http_output",
                    1,
                    "encoding",
                    &format!("must be a valid encoding, \"{}\" given", String::from_utf8_lossy(&n)),
                )),
                Some(e) => {
                    state::with(|s| {
                        s.http_output_set = true;
                        s.current_http_output_encoding = e;
                    });
                    Ok(Value::True)
                }
            }
        }
    }
}

fn mb_detect_order(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    state::sync(vm);
    let arg = args.first().map(|v| v.deref());
    let list = match arg {
        None | Some(Value::Null) => {
            let l = state::with(|s| s.current_detect_order_list.clone());
            return Ok(str_list_array(&l));
        }
        Some(Value::Array(a)) => parse_encoding_array(vm, "mb_detect_order", &a, 1, "encoding")?,
        Some(_) => {
            let s = str_arg(vm, "mb_detect_order", args, 0, "encoding")?;
            parse_encoding_list_arg(vm, "mb_detect_order", &s, 1, "encoding")?
        }
    };
    if list.is_empty() {
        return Err(arg_value_error(vm, "mb_detect_order", 1, "encoding", "must specify at least one encoding"));
    }
    state::with(|s| s.current_detect_order_list = list);
    Ok(Value::True)
}

fn mb_substitute_character(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    state::sync(vm);
    let arg = args.first().map(|v| v.deref());
    match arg {
        None | Some(Value::Null) => {
            let (c, mode) = state::illegal_settings();
            Ok(match mode {
                MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE => pstr(b"none"),
                MBFL_OUTPUTFILTER_ILLEGAL_MODE_LONG => pstr(b"long"),
                MBFL_OUTPUTFILTER_ILLEGAL_MODE_ENTITY => pstr(b"entity"),
                _ => Value::Long(c as i64),
            })
        }
        Some(Value::Long(cp)) => set_subst_codepoint(vm, cp),
        Some(Value::Double(f)) if f.is_finite() && f.fract() == 0.0 => set_subst_codepoint(vm, f as i64),
        Some(Value::True) => set_subst_codepoint(vm, 1),
        Some(Value::False) => set_subst_codepoint(vm, 0),
        Some(v @ Value::String(_)) => {
            let s = match &v {
                Value::String(s) => s.as_bytes().to_vec(),
                _ => unreachable!(),
            };
            // Z_PARAM_STR_OR_LONG: numeric strings are treated as integers
            if let Ok(t) = std::str::from_utf8(&s) {
                if let Ok(n) = t.trim().parse::<i64>() {
                    if !t.is_empty() && !t.ends_with(|c: char| c.is_whitespace()) {
                        return set_subst_codepoint(vm, n);
                    }
                }
            }
            let mode = if s.eq_ignore_ascii_case(b"none") {
                MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE
            } else if s.eq_ignore_ascii_case(b"long") {
                MBFL_OUTPUTFILTER_ILLEGAL_MODE_LONG
            } else if s.eq_ignore_ascii_case(b"entity") {
                MBFL_OUTPUTFILTER_ILLEGAL_MODE_ENTITY
            } else {
                return Err(arg_value_error(
                    vm,
                    "mb_substitute_character",
                    1,
                    "substitute_character",
                    "must be \"none\", \"long\", \"entity\" or a valid codepoint",
                ));
            };
            state::with(|s| s.current_filter_illegal_mode = mode);
            Ok(Value::True)
        }
        Some(other) => Err(type_error(
            vm,
            format!(
                "mb_substitute_character(): Argument #1 ($substitute_character) must be of type string|int|null, {} given",
                type_name(&other)
            ),
        )),
    }
}

fn set_subst_codepoint(vm: &mut Vm, cp: i64) -> Result<Value, VmError> {
    if cp < 0 || cp >= 0x110000 || (0xd800..=0xdfff).contains(&cp) {
        return Err(arg_value_error(vm, "mb_substitute_character", 1, "substitute_character", "is not a valid codepoint"));
    }
    state::with(|s| {
        s.current_filter_illegal_mode = mbfl::MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR;
        s.current_filter_illegal_substchar = cp as u32;
    });
    Ok(Value::True)
}

fn mb_preferred_mime_name(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    state::sync(vm);
    let name = str_arg(vm, "mb_preferred_mime_name", args, 0, "encoding")?;
    match mbfl::name2encoding(&name) {
        None => Err(arg_value_error(
            vm,
            "mb_preferred_mime_name",
            1,
            "encoding",
            &format!("must be a valid encoding, \"{}\" given", String::from_utf8_lossy(&name)),
        )),
        Some(e) => match mbfl::preferred_mime_name(e) {
            Some(m) => Ok(pstr(m.as_bytes())),
            None => {
                vm.emit_warning(&format!(
                    "mb_preferred_mime_name(): No MIME preferred name corresponding to \"{}\"",
                    String::from_utf8_lossy(&name)
                ));
                Ok(Value::False)
            }
        },
    }
}

/* ===== mb_strlen ===== */

/// C: `mb_fast_strlen_utf8` (string assumed valid UTF-8)
pub fn fast_strlen_utf8(s: &[u8]) -> usize {
    s.iter().filter(|&&c| (c as i8) >= -64).count()
}

/// C: `mb_get_strlen`
pub fn get_strlen(s: &[u8], enc: &Encoding) -> usize {
    let char_len = enc.flag & (MBFL_ENCTYPE_SBCS | MBFL_ENCTYPE_WCS2 | MBFL_ENCTYPE_WCS4);
    if char_len != 0 {
        return s.len() / char_len as usize;
    }
    if is_utf8_family(enc.no) && std::str::from_utf8(s).is_ok() {
        return fast_strlen_utf8(s);
    }
    to_wchars(enc, s).len()
}

fn mb_strlen(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let s = str_arg(vm, "mb_strlen", args, 0, "string")?;
    let enc = encoding_arg(vm, "mb_strlen", args, 1, "encoding")?;
    Ok(Value::Long(get_strlen(&s, enc) as i64))
}

/* ===== mb_convert_encoding ===== */

/// C: `php_mb_convert_encoding`
pub fn convert_encoding(vm: &mut Vm, func: &str, input: &[u8], to: &'static Encoding, from_list: &[&'static Encoding]) -> Option<Vec<u8>> {
    let from = if from_list.len() == 1 {
        from_list[0]
    } else {
        let strict = state::with(|s| s.strict_detection);
        match guess_encoding(&[input], from_list, strict, true) {
            Some(e) => e,
            None => {
                vm.emit_warning(&format!("{}(): Unable to detect character encoding", func));
                return None;
            }
        }
    };
    Some(convert_ex(input, to, from))
}

fn convert_encoding_recursive(
    vm: &mut Vm,
    input: &Rc<RefCell<PhpArray>>,
    to: &'static Encoding,
    from_list: &[&'static Encoding],
    depth: usize,
) -> Option<PhpArray> {
    if depth > 256 {
        vm.emit_warning("mb_convert_encoding(): Cannot convert recursively referenced values");
        return None;
    }
    let entries: Vec<(ArrayKey, Value)> = input.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let mut out = PhpArray::new();
    for (k, v) in entries {
        let key = match k {
            ArrayKey::String(s) => match convert_encoding(vm, "mb_convert_encoding", s.as_bytes(), to, from_list) {
                Some(c) => ArrayKey::String(PhpString::from_vec(c)),
                None => continue,
            },
            other => other,
        };
        let v = v.deref();
        let nv = match &v {
            Value::String(s) => match convert_encoding(vm, "mb_convert_encoding", s.as_bytes(), to, from_list) {
                Some(c) => pstring(c),
                None => continue,
            },
            Value::Null | Value::True | Value::False | Value::Long(_) | Value::Double(_) => v.clone(),
            Value::Array(a) => match convert_encoding_recursive(vm, a, to, from_list, depth + 1) {
                Some(c) => new_array(c),
                None => new_array(PhpArray::new()),
            },
            _ => {
                vm.emit_warning("mb_convert_encoding(): Object is not supported");
                continue;
            }
        };
        out.set(key, nv);
    }
    Some(out)
}

fn remove_non_encodings(list: &mut Vec<&'static Encoding>) {
    list.retain(|e| !is_non_text_encoding(e.no));
}

fn mb_convert_encoding(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_convert_encoding";
    let input = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let input_arr = match &input {
        Value::Array(a) => Some(a.clone()),
        Value::String(_) | Value::Long(_) | Value::Double(_) | Value::True | Value::False | Value::Null => None,
        other => {
            return Err(type_error(
                vm,
                format!("mb_convert_encoding(): Argument #1 ($string) must be of type array|string, {} given", type_name(other)),
            ))
        }
    };
    let to_name = str_arg(vm, f, args, 1, "to_encoding")?;
    let to = get_encoding(vm, f, Some(&to_name), 2, "to_encoding")?;
    let mut from_list = match args.get(2).map(|v| v.deref()) {
        None | Some(Value::Null) => vec![state::current_internal_encoding()],
        Some(Value::Array(a)) => parse_encoding_array(vm, f, &a, 3, "from_encoding")?,
        Some(_) => {
            let s = str_arg(vm, f, args, 2, "from_encoding")?;
            parse_encoding_list_arg(vm, f, &s, 3, "from_encoding")?
        }
    };
    if from_list.len() > 1 {
        remove_non_encodings(&mut from_list);
    }
    if from_list.is_empty() {
        return Err(arg_value_error(vm, f, 3, "from_encoding", "must specify at least one encoding"));
    }
    match input_arr {
        Some(a) => {
            let r = convert_encoding_recursive(vm, &a, to, &from_list, 0).unwrap_or_default();
            Ok(new_array(r))
        }
        None => {
            let s = input.to_php_string();
            match convert_encoding(vm, f, s.as_bytes(), to, &from_list) {
                Some(r) => Ok(pstring(r)),
                None => Ok(Value::False),
            }
        }
    }
}

/* ===== Encoding detection ===== */

fn estimate_demerits(w: u32) -> u64 {
    use crate::mbfl::tables::rare_cp_bitvec::rare_codepoint_bitvec;
    if w > 0xFFFF {
        40
    } else if (0x21..=0x2F).contains(&w) {
        6
    } else if (rare_codepoint_bitvec[(w >> 5) as usize] >> (w & 0x1F)) & 1 != 0 {
        30
    } else {
        1
    }
}

/// C: `mb_check_encoding` core (`php_mb_check_encoding`)
pub fn check_encoding(input: &[u8], enc: &Encoding) -> bool {
    if enc.no == EncNo::Utf8 {
        return std::str::from_utf8(input).is_ok();
    }
    if let Some(chk) = enc.check {
        return chk(input);
    }
    if enc.to_wchar.is_none() {
        return true;
    }
    !to_wchars(enc, input).contains(&MBFL_BAD_INPUT)
}

struct Candidate {
    enc: &'static Encoding,
    demerits: u64,
    multiplier: f32,
}

/// C: `mb_guess_encoding_for_strings`
pub fn guess_encoding(strings: &[&[u8]], elist: &[&'static Encoding], strict: bool, order_significant: bool) -> Option<&'static Encoding> {
    if elist.is_empty() {
        return None;
    }
    if elist.len() == 1 {
        if strict {
            for s in strings {
                if !check_encoding(s, elist[0]) {
                    return None;
                }
            }
        }
        return Some(elist[0]);
    }
    if strings.len() == 1 && strings[0].is_empty() {
        return Some(elist[0]);
    }
    let length = elist.len();
    let mut array: Vec<Candidate> = Vec::with_capacity(length);
    'outer: for (i, enc) in elist.iter().enumerate() {
        let mut demerits = 0u64;
        if let Some(chk) = enc.check {
            for s in strings {
                if !chk(s) {
                    if strict {
                        continue 'outer;
                    } else {
                        demerits += 500;
                    }
                }
            }
        }
        let multiplier = if order_significant { (1.0 + ((0.3 * i as f64) / length as f64)) as f32 } else { 1.0f32 };
        array.push(Candidate { enc, demerits, multiplier });
    }
    // Strings are processed last to first, as in C
    for s in strings.iter().rev() {
        if !strict && array.len() <= 1 {
            // C: while ((strict || length > 1) && ...): nothing to count
        } else {
            let mut i = array.len();
            while i > 0 {
                i -= 1;
                let enc = array[i].enc;
                let mut input: &[u8] = s;
                // Skip byte order mark for UTF-8, UTF-16BE, or UTF-16LE
                match enc.no {
                    EncNo::Utf8 if input.len() >= 3 && input[..3] == [0xEF, 0xBB, 0xBF] => input = &input[3..],
                    EncNo::Utf16be if input.len() >= 2 && input[..2] == [0xFE, 0xFF] => input = &input[2..],
                    EncNo::Utf16le if input.len() >= 2 && input[..2] == [0xFF, 0xFE] => input = &input[2..],
                    _ => {}
                }
                if input.is_empty() {
                    continue;
                }
                let wchars = to_wchars(enc, input);
                let mut eliminated = false;
                let mut d = 0u64;
                for &w in &wchars {
                    if w == MBFL_BAD_INPUT {
                        if strict {
                            eliminated = true;
                            break;
                        }
                        d += 1000;
                    } else {
                        d += estimate_demerits(w);
                    }
                }
                if eliminated {
                    array.remove(i);
                } else {
                    array[i].demerits += d;
                }
            }
        }
        for c in array.iter_mut() {
            let demerits = c.demerits as f64 * c.multiplier as f64;
            c.demerits = if demerits < u64::MAX as f64 { demerits as u64 } else { u64::MAX };
        }
        if array.is_empty() {
            return None;
        }
    }
    let mut best = 0;
    for i in 1..array.len() {
        if array[i].demerits < array[best].demerits {
            best = i;
        }
    }
    Some(array[best].enc)
}

thread_local! {
    static ALL_ENCODINGS_LIST: RefCell<Option<Rc<RefCell<PhpArray>>>> = const { RefCell::new(None) };
}

pub fn is_all_encodings_list(a: &Rc<RefCell<PhpArray>>) -> bool {
    ALL_ENCODINGS_LIST.with(|l| l.borrow().as_ref().map(|x| Rc::ptr_eq(x, a)).unwrap_or(false))
}

fn mb_detect_encoding(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_detect_encoding";
    let s = str_arg(vm, f, args, 0, "string")?;
    state::sync(vm);
    let mut order_significant = true;
    let mut elist = match args.get(1).map(|v| v.deref()) {
        None | Some(Value::Null) => state::with(|st| st.current_detect_order_list.clone()),
        Some(Value::Array(a)) => {
            let is_all = ALL_ENCODINGS_LIST.with(|l| l.borrow().as_ref().map(|x| Rc::ptr_eq(x, &a)).unwrap_or(false));
            if is_all {
                order_significant = false;
            }
            parse_encoding_array(vm, f, &a, 2, "encodings")?
        }
        Some(_) => {
            let e = str_arg(vm, f, args, 1, "encodings")?;
            parse_encoding_list_arg(vm, f, &e, 2, "encodings")?
        }
    };
    if elist.is_empty() {
        return Err(arg_value_error(vm, f, 2, "encodings", "must specify at least one encoding"));
    }
    remove_non_encodings(&mut elist);
    if elist.is_empty() {
        return Ok(Value::False);
    }
    let strict = if args.len() < 3 { state::with(|st| st.strict_detection) } else { bool_arg(args, 2) };
    let ret = if elist.len() == 1 && elist[0].no == EncNo::Utf8 && std::str::from_utf8(&s).is_ok() {
        Some(elist[0])
    } else {
        guess_encoding(&[&s], &elist, strict, order_significant)
    };
    match ret {
        Some(e) => Ok(pstr(e.name.as_bytes())),
        None => Ok(Value::False),
    }
}

fn mb_list_encodings(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    state::sync(vm);
    let arr = ALL_ENCODINGS_LIST.with(|l| {
        let mut l = l.borrow_mut();
        if l.is_none() {
            let mut arr = PhpArray::new();
            for e in mbfl::supported_encodings() {
                arr.push(pstr(e.name.as_bytes()));
            }
            *l = Some(Rc::new(RefCell::new(arr)));
        }
        l.as_ref().unwrap().clone()
    });
    Ok(Value::Array(arr))
}

fn mb_encoding_aliases(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let name = str_arg(vm, "mb_encoding_aliases", args, 0, "encoding")?;
    let enc = get_encoding(vm, "mb_encoding_aliases", Some(&name), 1, "encoding")?;
    let mut arr = PhpArray::new();
    for a in enc.aliases {
        arr.push(pstr(a.as_bytes()));
    }
    Ok(new_array(arr))
}

/* ===== mb_check_encoding ===== */

fn check_encoding_recursive(vm: &mut Vm, arr: &Rc<RefCell<PhpArray>>, enc: &Encoding, depth: usize) -> bool {
    if depth > 256 {
        vm.emit_warning("mb_check_encoding(): Cannot not handle circular references");
        return false;
    }
    let entries: Vec<(ArrayKey, Value)> = arr.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, v) in entries {
        if let ArrayKey::String(s) = &k {
            if !check_encoding(s.as_bytes(), enc) {
                return false;
            }
        }
        match v.deref() {
            Value::String(s) => {
                if !check_encoding(s.as_bytes(), enc) {
                    return false;
                }
            }
            Value::Array(a) => {
                if !check_encoding_recursive(vm, &a, enc, depth + 1) {
                    return false;
                }
            }
            Value::Long(_) | Value::Double(_) | Value::Null | Value::True | Value::False => {}
            _ => return false,
        }
    }
    true
}

fn mb_check_encoding(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "mb_check_encoding";
    let input = args.first().map(|v| v.deref());
    if let Some(v) = &input {
        if !matches!(v, Value::Null | Value::Array(_) | Value::String(_) | Value::Long(_) | Value::Double(_) | Value::True | Value::False) {
            if !matches!(v, Value::Object(_)) {
                return Err(type_error(
                    vm,
                    format!("mb_check_encoding(): Argument #1 ($value) must be of type array|string|null, {} given", type_name(v)),
                ));
            }
        }
    }
    let enc = encoding_arg(vm, f, args, 1, "encoding")?;
    match input {
        Some(Value::Array(a)) => Ok(vbool(check_encoding_recursive(vm, &a, enc, 0))),
        None | Some(Value::Null) => {
            vm.emit_deprecated("mb_check_encoding(): Calling mb_check_encoding() without argument is deprecated");
            Ok(vbool(state::with(|s| s.illegalchars == 0)))
        }
        Some(_) => {
            let s = str_arg(vm, f, args, 0, "value")?;
            Ok(vbool(check_encoding(&s, enc)))
        }
    }
}

/* ===== mb_get_info ===== */

fn subst_char_value(st: &MbState) -> Value {
    match st.current_filter_illegal_mode {
        MBFL_OUTPUTFILTER_ILLEGAL_MODE_NONE => pstr(b"none"),
        MBFL_OUTPUTFILTER_ILLEGAL_MODE_LONG => pstr(b"long"),
        MBFL_OUTPUTFILTER_ILLEGAL_MODE_ENTITY => pstr(b"entity"),
        _ => Value::Long(st.current_filter_illegal_substchar as i64),
    }
}

fn enc_name(no: EncNo) -> &'static str {
    mbfl::no2encoding(no).map(|e| e.name).unwrap_or("")
}

fn mb_get_info(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    state::sync(vm);
    let ty = match args.first() {
        None => None,
        Some(_) => Some(str_arg(vm, "mb_get_info", args, 0, "type")?),
    };
    let conv_mimetypes = match vm.constants.get(b"mbstring.http_output_conv_mimetypes".as_ref()) {
        Some(v) if !v.to_php_string().is_empty() => v.to_php_string().as_bytes().to_vec(),
        _ => b"^(text/|application/xhtml\\+xml)".to_vec(),
    };
    let r = state::with(|st| {
        let lang = state::no2language(st.language);
        let onoff = |b: bool| pstr(if b { b"On" } else { b"Off" });
        let t = ty.as_ref().map(|t| t.to_ascii_lowercase());
        match t.as_deref() {
            None | Some(b"all") => {
                let mut arr = PhpArray::new();
                let mut set = |k: &str, v: Value| arr.set(ArrayKey::String(PhpString::from_bytes(k.as_bytes())), v);
                set("internal_encoding", pstr(st.current_internal_encoding.name.as_bytes()));
                if let Some(e) = st.http_input_identify {
                    set("http_input", pstr(e.name.as_bytes()));
                }
                set("http_output", pstr(st.current_http_output_encoding.name.as_bytes()));
                set("http_output_conv_mimetypes", pstr(&conv_mimetypes));
                set("mail_charset", pstr(enc_name(lang.mail_charset).as_bytes()));
                set("mail_header_encoding", pstr(enc_name(lang.mail_header_encoding).as_bytes()));
                set("mail_body_encoding", pstr(enc_name(lang.mail_body_encoding).as_bytes()));
                set("illegal_chars", Value::Long(st.illegalchars as i64));
                set("encoding_translation", onoff(st.encoding_translation));
                set("language", pstr(lang.name.as_bytes()));
                if !st.current_detect_order_list.is_empty() {
                    set("detect_order", str_list_array(&st.current_detect_order_list));
                }
                set("substitute_character", subst_char_value(st));
                set("strict_detection", onoff(st.strict_detection));
                Some(new_array(arr))
            }
            Some(b"internal_encoding") => Some(pstr(st.current_internal_encoding.name.as_bytes())),
            Some(b"http_input") => Some(st.http_input_identify.map(|e| pstr(e.name.as_bytes())).unwrap_or(Value::Null)),
            Some(b"http_output") => Some(pstr(st.current_http_output_encoding.name.as_bytes())),
            Some(b"http_output_conv_mimetypes") => Some(pstr(&conv_mimetypes)),
            Some(b"mail_charset") => Some(pstr(enc_name(lang.mail_charset).as_bytes())),
            Some(b"mail_header_encoding") => Some(pstr(enc_name(lang.mail_header_encoding).as_bytes())),
            Some(b"mail_body_encoding") => Some(pstr(enc_name(lang.mail_body_encoding).as_bytes())),
            Some(b"illegal_chars") => Some(Value::Long(st.illegalchars as i64)),
            Some(b"encoding_translation") => Some(onoff(st.encoding_translation)),
            Some(b"language") => Some(pstr(lang.name.as_bytes())),
            Some(b"detect_order") => {
                if !st.current_detect_order_list.is_empty() {
                    Some(str_list_array(&st.current_detect_order_list))
                } else {
                    Some(Value::Null)
                }
            }
            Some(b"substitute_character") => Some(subst_char_value(st)),
            Some(b"strict_detection") => Some(onoff(st.strict_detection)),
            _ => None,
        }
    });
    match r {
        Some(v) => Ok(v),
        None => {
            vm.emit_warning("mb_get_info(): argument #1 ($type) must be a valid type");
            Ok(Value::False)
        }
    }
}
